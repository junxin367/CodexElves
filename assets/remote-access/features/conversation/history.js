// 历史读取：分页读取、Claude 历史排序、增量合并、重新读取会话，以及历史加载占位与调度；读取进度与正文视图状态在 history-state.js。

import { recordDiagnostic } from "../../diagnostics.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { renderApprovals, renderMessages, updateComposer } from "../../core/actions.js";
import {
  pageLeaving, selectedId, selectionVersion, session, setSelectionVersion, setThread, setTurns,
  sidebar, thread, turns
} from "../../core/state.js";
import { transcriptCache } from "../../core/stores.js";
import { api } from "../api.js";
import { mergeBlockingRequestsRead } from "../approval-requests.js";
import { blockingRequestState, notifiedQuestions, questionVersion, replaceAsyncQuestions } from "../approval-state.js";
import { renderModelMenu } from "../composer/composer-menus.js";
import { reconcileRemoteFastPreference } from "../composer/model-catalog.js";
import { fastModeVersion } from "../composer/options-state.js";
import { compactionVersion, currentCompaction, normalizeCompaction, reconcileCompaction } from "./compaction.js";
import { notifiedStatus, notifiedTokenUsage, statusVersion, tokenUsageVersion } from "./event-sync.js";
import {
  claudeHistoryCursor, completedItem, historyAutoFailed, historyController, historyCursor,
  historyError, historyInitialized, historyLoadFrame, historyLoadVisibleItems, historyLoading,
  historyRequest, itemVersions, mergeLiveText, messageVersion, normalizeDisplayedTurnItems,
  orderClaudeHistoryTurns, partialItems, setHistoryAutoFailed, setHistoryController,
  setHistoryCursor, setHistoryError, setHistoryInitialized, setHistoryLoadFrame,
  setHistoryLoadVisibleItems, setHistoryLoading, setHistoryLoadingOlder, setHistoryLoadingTurnId,
  setHistoryLoadingVersion, setHistoryRequest, setLatestHistoryReadFailed, setMessageVersion,
  setRecentHistoryPage, setThreadRequest, setTranscriptChecking, syncHistoryButtons, threadRequest,
  transcriptChecking, turnFieldVersions, turnVersions
} from "./history-state.js";
import { setThreadReadFailed, setThreadReadVersion, syncThreadHash, threadReadVersion } from "./selection-state.js";
import {
  enrichTurnTranscriptEvents, mergeItemProjection, mergeRemoteImageProjection,
  mergeTranscriptEvents, refreshTranscriptEvents, replaceTranscriptEvents, transcriptEvents
} from "./transcript-events.js";
import { rememberTranscript, resetMessages } from "./transcript.js";
import { goalVersion, notifiedGoal } from "../goal/state.js";
import { messageImageReadSequence, setMessageImageReadSequence } from "../messages/media-state.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { applyQueueRead } from "../queue/queue.js";
import { notifiedQueue, notifiedQueueEdits, queueVersion, syncEditableMessageMetadata } from "../queue/state.js";
import { claudeThreadId, normalizeRelatedView } from "../session/capabilities.js";
import {
  foregroundReadGeneration, recoveryErrorSilenceDepth, refreshTimer, setRefreshTimer
} from "../session/connection-state.js";
import { threadLocation } from "../session/instances.js";
import { relatedSessionView, relatedThreadViews, setRelatedSessionView } from "../session/related-state.js";
import {
  applyRelatedThreadMetadata, clearMissingThreadSelection, isMissingThreadError, sideThreadExpired
} from "../session/related.js";
import {
  discardPersistentThreadDisplay, threadDisplayKey, threadDisplays, threadEphemeralState
} from "../session/thread-cache.js";
import { rememberThreadDisplay } from "../session/thread-display.js";
import { threadDeleted } from "../sidebar/deletion-state.js";

export async function loadHistory(older = false, version = selectionVersion, itemsTurnId = null) {
  if (pageLeaving || document.hidden || version !== selectionVersion || !selectedId || threadDeleted(selectedId)) return;
  const generation = foregroundReadGeneration;
  if ((older || itemsTurnId) && transcriptChecking) return;
  if (historyRequest?.version === version && historyRequest.generation === generation) {
    if (!older) historyRequest.latestPending = true;
    return historyRequest.promise;
  }
  const id = selectedId, request = { version, generation, latestPending: false, promise: null };
  request.promise = Promise.resolve().then(async () => {
    do {
      if (generation !== foregroundReadGeneration) return;
      request.latestPending = false;
      await readHistoryPage(older, version, itemsTurnId, generation);
      older = false; itemsTurnId = null;
    } while (request.latestPending && generation === foregroundReadGeneration && version === selectionVersion && selectedId === id);
  });
  setHistoryRequest(request);
  try { await request.promise; }
  finally { if (historyRequest === request) setHistoryRequest(null); }
}
async function readHistoryPage(older, version, itemsTurnId, generation = foregroundReadGeneration) {
  if (pageLeaving || document.hidden || generation !== foregroundReadGeneration
      || version !== selectionVersion || !selectedId || threadDeleted(selectedId)) return;
  // 执行过期不等于历史不可读取；仍从后台补齐最终回复和分页内容，不恢复发送能力。
  const itemTurn = itemsTurnId ? turns.get(itemsTurnId) : null;
  if ((itemsTurnId && !itemTurn?.itemsNextCursor) || (older && !itemsTurnId && !historyCursor)) return;
  setHistoryLoading(true); setHistoryLoadingVersion(version);
  const latestPage = !older && !itemsTurnId;
  setHistoryLoadingTurnId(itemsTurnId); setHistoryLoadingOlder(older); setHistoryAutoFailed(false);
  if (latestPage) setHistoryError("");
  syncHistoryButtons();
  if (!historyInitialized) renderMessages();
  const id = selectedId, readVersion = messageVersion, checkingCached = transcriptChecking && latestPage,
    mediaReadVersion = setMessageImageReadSequence(messageImageReadSequence + 1);
  const initialIds = new Set(turns.keys());
  const controller = new AbortController();
  const historyStartedAt = performance.now();
  recordDiagnostic("remote.history", { phase: "request" });
  setHistoryController(controller);
  const cursorAnchor = itemTurn?.itemsCursorAnchor || itemTurn?.items?.[0]?.id;
  try {
    const payload = itemsTurnId ? {
      threadId: id, turnId: itemsTurnId, itemCursor: itemTurn.itemsNextCursor,
      turnStatus: itemTurn.status, turnStartedAt: itemTurn.startedAt, turnCompletedAt: itemTurn.completedAt
    } : { threadId: id, cursor: older ? historyCursor : null };
    let result;
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        result = await api("/api/read/history", payload, undefined, controller.signal);
        break;
      } catch (error) {
        // 只重读未能取得完整正文的历史请求；业务错误和完整 JSON 的协议错误交给原有失败入口。
        if (attempt || !error.responseReadFailed || controller.signal.aborted || pageLeaving
            || version !== selectionVersion || selectedId !== id || threadDeleted(id)) throw error;
      }
    }
    if (controller.signal.aborted || generation !== foregroundReadGeneration
        || pageLeaving || version !== selectionVersion || selectedId !== id || threadDeleted(id)) return;
    if (!Array.isArray(result?.data)) throw new Error("会话历史返回格式无效");
    if (itemsTurnId && (result.scope !== "items" || result.turnId !== itemsTurnId)) throw new Error("会话消息分页归属无效");
    if (!itemsTurnId && claudeThreadId(id))
      retireClaudeHistoryTurns(result.turnIds, readVersion, messageVersion + 1);
    // 用户要求旧摘要同步删除后不能被迟到读取复活；仅过滤本次读取开始后已明确删除的 Claude 轮次，不影响分页或 Codex。
    if (!itemsTurnId && claudeThreadId(id))
      result.data = result.data.filter(turn => turns.has(turn.id) || (turnVersions.get(turn.id) || 0) <= readVersion);
    if (result.ephemeral === true) {
      if (thread?.id === id) thread.ephemeral = true;
      if (relatedSessionView?.threadId === id) setRelatedSessionView({ ...relatedSessionView, ephemeral: true });
      const view = normalizeRelatedView(relatedThreadViews.get(id), id);
      if (view) relatedThreadViews.set(id, { ...view, ephemeral: true });
      discardPersistentThreadDisplay(id);
    }
    if (result.sideHistoryVersion === 1) {
      const inherited = new Set(result.inheritedTurnIds || []);
      for (const turnId of inherited) turns.delete(turnId);
      for (const [key, event] of transcriptEvents) if (inherited.has(event.turnId)) transcriptEvents.delete(key);
    }
    if (latestPage) setRecentHistoryPage({
      turns: [...result.data].reverse(), transcriptEvents: result.transcriptEvents || [],
      historyCursor: result.nextCursor, followMessages: true, scrollTop: 0,
      ...(typeof result.ephemeral === "boolean" ? { ephemeral: result.ephemeral }
        : typeof threadEphemeralState(id) === "boolean" ? { ephemeral: threadEphemeralState(id) } : {}),
      ...(result.sideHistoryVersion === 1 ? { sideHistoryVersion: 1 } : {})
    });
    if (checkingCached && !pruneCachedHistory(result.data, readVersion)) setHistoryCursor(result.nextCursor);
    mergeTranscriptEvents(result.transcriptEvents);
    if (itemsTurnId) {
      const merged = mergeHistoryTurn({
        id: itemsTurnId, items: result.data, itemsPageComplete: result.itemsPageComplete,
        itemsComplete: result.itemsComplete, itemsNextCursor: result.itemsNextCursor,
        itemsView: result.itemsComplete ? "full" : "summary"
      }, readVersion, { older: true, beforeId: cursorAnchor, mediaReadVersion });
      turns.set(itemsTurnId, merged);
    } else {
      const chronological = [...result.data].reverse().map(turn =>
        mergeHistoryTurn(turn, readVersion, { cached: checkingCached, mediaReadVersion }));
      if (older) {
        const ids = new Set(chronological.map(turn => turn.id));
        setTurns(new Map([...chronological.map(turn => [turn.id, turn]), ...[...turns].filter(([key]) => !ids.has(key))]));
      } else {
        const pageIds = new Set(chronological.map(turn => turn.id)), known = [...turns.values()];
        const firstShared = known.findIndex(turn => pageIds.has(turn.id));
        const earlier = historyInitialized ? known.filter((turn, index) => !pageIds.has(turn.id)
          && (firstShared < 0 ? initialIds.has(turn.id) : index < firstShared)) : [];
        const earlierIds = new Set(earlier.map(turn => turn.id));
        const later = known.filter(turn => !pageIds.has(turn.id) && !earlierIds.has(turn.id));
        setTurns(new Map([...earlier, ...chronological, ...later].map(turn => [turn.id, turn])));
      }
      if (older || !historyInitialized) setHistoryCursor(result.nextCursor);
    }
    if (!itemsTurnId && claudeThreadId(id)) {
      const order = orderClaudeHistoryTurns(result.turnIds, readVersion);
      const cursor = claudeHistoryCursor(order);
      if (cursor !== undefined) setHistoryCursor(cursor);
    }
    setHistoryInitialized(true);
    if (latestPage) { setTranscriptChecking(false); setLatestHistoryReadFailed(false); }
    syncEditableMessageMetadata();
    renderMessages(!older);
    recordDiagnostic("remote.history", {
      phase: "complete", durationMs: performance.now() - historyStartedAt
    });
    rememberTranscript();
    if (latestPage && sideThreadExpired(id) && (thread?.hasUnreadTurn === true
        || sidebar?.unreadSideThreads?.some(item => item.id === id)))
      void markVisibleThreadRead(id).catch(error => notice(error.message, true));
  } catch (error) {
    if (controller.signal.aborted || generation !== foregroundReadGeneration) return;
    recordDiagnostic("remote.history", {
      phase: "failure", durationMs: performance.now() - historyStartedAt,
      errorType: error?.name || "Error",
      reason: Number.isFinite(error?.code) ? "http-error" : "network-error"
    });
    if (controller.signal.aborted || pageLeaving || version !== selectionVersion || selectedId !== id || threadDeleted(id)) return;
    if (isMissingThreadError(error, id) && selectedId === id) {
      await clearMissingThreadSelection(id).catch(expiredError => notice(expiredError.message, true));
      return;
    }
    if (!itemsTurnId && error.code === 404) retireMissingEphemeralTranscript(id, error);
    if (latestPage) setLatestHistoryReadFailed(true);
    if (recoveryErrorSilenceDepth) {
      setHistoryAutoFailed(false);
      return;
    }
    setHistoryAutoFailed(true);
    if (!historyInitialized) {
      setHistoryError(`最新消息加载失败：${error.message}`);
      renderMessages();
    } else {
      if (latestPage) {
        setHistoryError(`最新消息加载失败：${error.message}`);
        renderMessages();
      }
      notice(error.message, true);
    }
  }
  finally {
    if (historyController === controller) {
      setHistoryController(null); setHistoryLoading(false); setHistoryLoadingTurnId(null); setHistoryLoadingOlder(false);
      syncHistoryButtons();
    }
  }
}
export function retireClaudeHistoryTurns(ids, readVersion, version) {
  if (!Array.isArray(ids)) return false;
  const current = new Set(ids);
  let changed = false;
  for (const [key, value] of turns) {
    // 用户要求旧中断气泡从缓存退役；只处理已确认原生轮次，不能清掉待发送预览或迟于 HTTP 读取的新流消息。
    if (!value._nativeUserId || current.has(key) || (turnVersions.get(key) || 0) > readVersion) continue;
    turns.delete(key); turnVersions.set(key, version); changed = true;
    for (const item of value.items || []) {
      itemVersions.set(`${key}:${item.id}`, version); partialItems.delete(`${key}:${item.id}`);
    }
    for (const [eventId, event] of transcriptEvents) if (event.turnId === key) transcriptEvents.delete(eventId);
  }
  if (changed) setMessageVersion(Math.max(messageVersion, version));
  return changed;
}
function pruneCachedHistory(snapshots, readVersion) {
  const ids = new Set(snapshots.map(turn => turn.id)), known = [...turns.values()];
  const firstShared = known.findIndex(turn => ids.has(turn.id));
  const earlier = firstShared < 0 ? [] : known.slice(0, firstShared);
  const earlierIds = new Set(earlier.map(turn => turn.id));
  const live = turn => (turnVersions.get(turn.id) || 0) > readVersion
    || ["plan", "diff"].some(field => (turnFieldVersions.get(`${turn.id}:${field}`) || 0) > readVersion)
    || (turn.items || []).some(item => (itemVersions.get(`${turn.id}:${item.id}`) || 0) > readVersion);
  for (const turn of known) if (!ids.has(turn.id) && !earlierIds.has(turn.id) && !live(turn)) {
    turns.delete(turn.id);
    for (const [key, event] of transcriptEvents) if (event.turnId === turn.id) transcriptEvents.delete(key);
  }
  for (const snapshot of snapshots) {
    const current = turns.get(snapshot.id);
    if (!current || snapshot.itemsPageComplete !== true || snapshot.itemsComplete === true) continue;
    const itemIds = new Set((snapshot.items || []).map(item => item.id));
    const anchors = new Set((snapshot.items || []).filter(item =>
      !item.remoteHistoryContext && !item.remoteCompactionInput).map(item => item.id));
    const first = (current.items || []).findIndex(item => anchors.has(item.id));
    const items = (current.items || []).filter((item, index) => itemIds.has(item.id)
      || first >= 0 && index < first || (itemVersions.get(`${current.id}:${item.id}`) || 0) > readVersion);
    turns.set(current.id, { ...current, items });
  }
  return earlier.length > 0;
}
function mergeHistoryTurn(snapshot, readVersion, { older = false, beforeId = null, cached = false, mediaReadVersion = 0 } = {}) {
  if (Array.isArray(snapshot?.items)) {
    snapshot = { ...snapshot, items: normalizeDisplayedTurnItems(snapshot.items) };
  }
  const current = turns.get(snapshot.id);
  const withImages = turn => {
    if (!current?.items?.some(item => item.remoteImages?.length) && !snapshot.items?.some(item => item.remoteImages?.length))
      return turn;
    const previousImages = new Map((current?.items || []).map(item => [item.id, item]));
    const snapshotImages = new Map((snapshot.items || []).map(item => [item.id, item]));
    return { ...turn, items: (turn.items || []).map(item =>
      mergeRemoteImageProjection(previousImages.get(item.id), snapshotImages.get(item.id), item, mediaReadVersion)) };
  };
  const contextItems = (snapshot.items || []).filter(item => item.remoteHistoryContext);
  if (contextItems.length)
    snapshot = { ...snapshot, items: snapshot.items.filter(item => !item.remoteHistoryContext) };
  // 跨页补投影的轮次开头和压缩输入不能充当当前页的定位点。
  const snapshotAnchor = snapshot.items?.find(item => !item.remoteCompactionInput && !item.remoteHistoryContext)?.id || null;
  if (snapshot._nativeUserId && snapshot.itemsComplete === true) {
    // Claude 每页提供完整轮次；不能套用 Codex 增量“保留所有新条目”的规则，否则正式正文到达后仍留下旧终端预览。
    // 读取开始后收到的正式通知优先；同一正文基线内只合并可延长的临时气泡，不混合两份正式内容。
    const authoritative = current && (turnVersions.get(snapshot.id) || 0) > readVersion ? current : snapshot;
    const isPreview = item => item._nativePreview === true
      || typeof item.id === "string" && item.id.startsWith("native-live-");
    const items = (authoritative.items || []).filter(item => !isPreview(item));
    const basis = authoritative._nativePreviewBasis;
    let preview = null;
    if (["inProgress", "pending", "unknown"].includes(authoritative.status) && !authoritative._nativeDone
        && /^[0-9a-f]{64}$/i.test(basis || "")) {
      for (const candidate of [authoritative, current]) {
        if (!candidate || candidate._nativeUserId !== authoritative._nativeUserId
            || candidate._process !== authoritative._process || candidate._nativePreviewBasis !== basis) continue;
        const next = candidate.items?.find(item => item._nativePreview === true
          && item.id === `native-live-${authoritative._nativeUserId}` && typeof item.text === "string");
        if (next && (!preview || next.text.replace(/\s/gu, "").startsWith(preview.text.replace(/\s/gu, ""))))
          preview = next;
      }
    }
    if (preview) items.push(preview);
    return enrichTurnTranscriptEvents(withImages({ ...authoritative, items,
      itemsCursorAnchor: items.find(item => !item.remoteCompactionInput)?.id || snapshotAnchor }));
  }
  if (!current) return enrichTurnTranscriptEvents(withImages({
    ...snapshot, items: [...contextItems, ...(snapshot.items || [])], itemsCursorAnchor: snapshotAnchor
  }));
  const streaming = snapshot.status === "inProgress" && current.status === "inProgress";
  const previous = new Map((current.items || []).map(item => [item.id, item]));
  let items = (snapshot.items || []).map(item => {
    if (item.remotePendingInput && typeof item.clientId === "string") {
      const delivered = [...previous.values()].find(previous => previous.type === "userMessage"
        && !previous.remotePendingInput && previous.clientId === item.clientId);
      if (delivered) item = delivered;
    }
    const live = previous.get(item.id), key = `${snapshot.id}:${item.id}`;
    previous.delete(item.id);
    if (!live) return item;
    // 旧缓存或迟到分页仍可能只含同 ID 的引导状态，不能退化已经恢复的正文。
    if (live.type === "userMessage" && item.type === "steered") return live;
    const newer = (itemVersions.get(key) || 0) > readVersion;
    if (cached && !newer) return mergeItemProjection(live, item);
    const liveComplete = completedItem(live), snapshotComplete = completedItem(item);
    if (liveComplete || snapshotComplete) {
      let keepLive = liveComplete && !snapshotComplete;
      if (liveComplete && snapshotComplete) {
        const comparable = Number.isFinite(live.completedAtMs) && Number.isFinite(item.completedAtMs);
        keepLive = comparable && live.completedAtMs !== item.completedAtMs
          ? live.completedAtMs > item.completedAtMs
          : newer || !comparable && (itemVersions.has(key) || Number.isFinite(live.completedAtMs));
      }
      partialItems.delete(key);
      // 完成记录的正文、分段、phase 和状态必须来自同一份权威内容。
      return keepLive ? mergeItemProjection(item, live) : mergeItemProjection(live, item);
    }
    if (newer || streaming && itemVersions.has(key)) {
      const merged = newer ? { ...item, ...live } : { ...live, ...item };
      if (Array.isArray(item.remoteImages) || Array.isArray(live.remoteImages))
        merged.remoteImages = [...new Map([...(item.remoteImages || []), ...(live.remoteImages || [])].map(image => [image.source, image])).values()];
      if (Array.isArray(item.remoteVideos) || Array.isArray(live.remoteVideos))
        merged.remoteVideos = [...new Map([...(item.remoteVideos || []), ...(live.remoteVideos || [])].map(video => [video.source, video])).values()];
      for (const field of ["text", "aggregatedOutput"]) if (typeof item[field] === "string" || typeof live[field] === "string")
        merged[field] = mergeLiveText(item[field], live[field], partialItems.has(key), newer);
      for (const field of ["summary", "content"]) if (live.type === "reasoning" && Array.isArray(item[field]) && Array.isArray(live[field])) {
        merged[field] = item[field].map((part, index) => mergeLiveText(part?.text ?? part, live[field][index]?.text ?? live[field][index], partialItems.has(key), newer));
        merged[field].push(...live[field].slice(item[field].length));
      }
      partialItems.delete(key);
      return merged;
    }
    partialItems.delete(key);
    return JSON.stringify(live) === JSON.stringify(item) ? live : item;
  });
  const pagination = { itemsCursorAnchor: snapshotAnchor || current.itemsCursorAnchor || null };
  if (snapshot.itemsPageComplete === true && (older || snapshot.itemsComplete !== true)) {
    const pageIds = new Set(items.map(item => item.id)), currentItems = current.items || [];
    const pageAnchors = new Set(items.filter(item => !item.remoteCompactionInput && !item.remoteHistoryContext).map(item => item.id));
    const overlap = currentItems.findIndex(item => pageAnchors.has(item.id));
    const remaining = currentItems.filter(item => !pageIds.has(item.id));
    let insertion = beforeId ? remaining.findIndex(item => item.id === beforeId) : -1;
    if (insertion < 0 && overlap >= 0)
      insertion = currentItems.slice(0, overlap).filter(item => !pageIds.has(item.id)).length;
    if (insertion < 0 && !older)
      insertion = remaining.findIndex(item => (itemVersions.get(`${snapshot.id}:${item.id}`) || 0) > readVersion);
    if (insertion < 0) insertion = older ? 0 : remaining.length;
    items = [...remaining.slice(0, insertion), ...items, ...remaining.slice(insertion)];
    if (!older && overlap >= 0) {
      const earlierPrefix = current.itemsCursorAnchor && pageIds.has(current.itemsCursorAnchor)
        && snapshotAnchor !== current.itemsCursorAnchor;
      if (current.itemsComplete === true || !earlierPrefix && current.itemsNextCursor) {
        pagination.itemsComplete = current.itemsComplete;
        pagination.itemsNextCursor = current.itemsNextCursor;
        pagination.itemsView = current.itemsComplete ? "full" : "summary";
        pagination.itemsCursorAnchor = current.itemsCursorAnchor;
      }
    }
  } else {
    for (const item of previous.values()) {
      const key = `${snapshot.id}:${item.id}`;
      // 历史分页可能尚未落盘当前轮的实时条目，不能将缺失当作删除。
      if ((itemVersions.get(key) || 0) <= readVersion && !(streaming && itemVersions.has(key))) continue;
      const index = current.items.findIndex(candidate => candidate.id === item.id);
      const next = current.items.slice(index + 1).find(candidate => items.some(item => item.id === candidate.id));
      const insertion = next ? items.findIndex(item => item.id === next.id) : items.length;
      items.splice(insertion, 0, item);
    }
  }
  const newer = (turnVersions.get(snapshot.id) || 0) > readVersion;
  const liveFields = Object.fromEntries(["plan", "diff"]
    .filter(field => (turnFieldVersions.get(`${snapshot.id}:${field}`) || 0) > readVersion)
    .map(field => [field, current[field]]));
  const timing = Object.fromEntries(["startedAt", "completedAt", "durationMs", "turnStartedAtMs", "turnCompletedAtMs"]
    .filter(field => Number.isFinite(current[field]) && (newer || !Number.isFinite(snapshot[field])))
    .map(field => [field, current[field]]));
  if (contextItems.length) {
    const contextIds = new Set(contextItems.map(item => item.id));
    const loaded = new Map(items.map(item => [item.id, item]));
    items = [...contextItems.map(item => loaded.get(item.id) || item), ...items.filter(item => !contextIds.has(item.id))];
  }
  return enrichTurnTranscriptEvents(withImages({
    ...current, ...snapshot, ...pagination, ...timing, ...liveFields, items,
    ...(newer ? { status: current.status, error: current.error } : {})
  }));
}
export async function reloadThread() {
  if (pageLeaving || document.hidden || !selectedId || threadDeleted(selectedId)) return;
  clearTimeout(refreshTimer); setRefreshTimer(null);
  const generation = foregroundReadGeneration;
  if (threadRequest?.version === selectionVersion && threadRequest.generation === generation) {
    threadRequest.again = true; return threadRequest.promise;
  }
  const id = selectedId, version = selectionVersion;
  const request = { version, generation, controller: new AbortController(), again: false, promise: null };
  request.promise = (async () => {
    do {
      if (generation !== foregroundReadGeneration || request.controller.signal.aborted) return;
      request.again = false;
      const read = setThreadReadVersion(threadReadVersion + 1), usageVersion = tokenUsageVersion, fastReadVersion = fastModeVersion,
        transcriptReadVersion = messageVersion,
        questionReadVersion = questionVersion, requestsReadVersion = blockingRequestState(id).generation,
        runtimeVersion = statusVersion, goalReadVersion = goalVersion, compactionReadVersion = compactionVersion,
        queueReadVersion = queueVersion;
      const historyRead = loadHistory(false, version);
      let latest;
      try { ({ thread: latest } = await api("/api/read/thread", { threadId: id }, undefined, request.controller.signal)); }
      catch (error) {
        if (generation !== foregroundReadGeneration || request.controller.signal.aborted) return;
        if (version === selectionVersion && selectedId === id) setThreadReadFailed(true);
        if (version === selectionVersion && selectedId === id && isMissingThreadError(error, id)) {
          await historyRead.catch(() => {});
          if (generation !== foregroundReadGeneration || request.controller.signal.aborted
              || version !== selectionVersion || selectedId !== id || !isMissingThreadError(error, id)) return;
          await clearMissingThreadSelection(id);
          return;
        }
        if (version === selectionVersion && selectedId === id && error.code === 404)
          retireMissingEphemeralTranscript(id, error);
        if (request.again && version === selectionVersion) continue;
        throw error;
      }
      if (generation !== foregroundReadGeneration || request.controller.signal.aborted
          || version !== selectionVersion || read !== threadReadVersion || selectedId !== id || threadDeleted(id)) return;
      if (questionReadVersion !== questionVersion && !notifiedQuestions) {
        request.again = true;
        continue;
      }
      if (usageVersion !== tokenUsageVersion) latest.tokenUsage = notifiedTokenUsage;
      if (runtimeVersion !== statusVersion && notifiedStatus) latest.status = notifiedStatus;
      if (goalReadVersion !== goalVersion) latest.goal = notifiedGoal;
      applyQueueRead(id, latest, queueReadVersion);
      if (notifiedQueue) {
        latest.queuedMessages = notifiedQueue;
        if (notifiedQueueEdits) latest.queuedEdits = notifiedQueueEdits;
      }
      setThread(reconcileRemoteFastPreference(latest, fastReadVersion));
      if (thread?.id !== id) throw new Error("会话返回内容与当前会话不一致");
      setThreadReadFailed(false);
      rememberThreadDisplay();
      applyRelatedThreadMetadata(thread);
      mergeBlockingRequestsRead(id, thread, requestsReadVersion);
      if (questionReadVersion !== questionVersion && notifiedQuestions)
        replaceAsyncQuestions(thread, notifiedQuestions);
      replaceTranscriptEvents(thread.transcriptEvents, transcriptReadVersion);
      refreshTranscriptEvents();
      syncEditableMessageMetadata();
      if (!$("#model-settings").hidden) renderModelMenu();
      if (compactionReadVersion === compactionVersion) reconcileCompaction(id, thread.compaction);
      else thread.compaction = normalizeCompaction(currentCompaction(id));
      $("#thread-title").textContent = thread.viewKind === "subagent" ? thread.agentName || thread.title || "子代理" : thread.title;
      $("#thread-path").textContent = threadLocation(thread); syncThreadHash(id);
      updateComposer(); renderApprovals();
      await historyRead;
    } while (request.again && generation === foregroundReadGeneration && version === selectionVersion && selectedId === id);
  })();
  setThreadRequest(request);
  try { await request.promise; }
  finally { if (threadRequest === request) setThreadRequest(null); }
}

export function retireMissingEphemeralTranscript(id, error) {
  if (id !== selectedId || threadEphemeralState(id) !== true
      || relatedSessionView?.kind === "side" || relatedThreadViews.get(id)?.kind === "side"
      || thread?.id === id && thread.viewKind === "side"
      || /thread not loaded:/i.test(error?.message || "")) return;
  // 原生已确认不存在：使同轮回包失效，并允许重新选择同一 ID 核对原生状态。
  setSelectionVersion(selectionVersion + 1);
  resetMessages();
  setThread(null);
  threadDisplays.delete(threadDisplayKey(id));
  transcriptCache.remove(session?.device.id, id);
  discardPersistentThreadDisplay(id);
  turns.clear(); transcriptEvents.clear(); setRecentHistoryPage(null); setHistoryCursor(null);
  setHistoryInitialized(false); setTranscriptChecking(false);
  setHistoryError("此原生临时会话已关闭或不存在");
  renderMessages();
}

export function historyPlaceholder() {
  const node = element("div", "empty-conversation");
  if (historyError) {
    node.append(element("p", "error-text", historyError),
      button("重试", "重试加载最新消息", () => void loadHistory(), "text-button"));
  } else {
    node.setAttribute("role", "status");
    node.append(element("span", "spinner"), document.createTextNode(" 正在加载最新消息…"));
  }
  return node;
}
export function scheduleHistoryLoad(visibleItems = false) {
  (historyLoadVisibleItems || setHistoryLoadVisibleItems(visibleItems));
  if (historyLoadFrame != null) return;
  setHistoryLoadFrame(requestAnimationFrame(() => {
    setHistoryLoadFrame(null);
    const includeVisibleItems = historyLoadVisibleItems;
    setHistoryLoadVisibleItems(false);
    const root = $("#messages");
    if (!root || !selectedId || !historyInitialized || transcriptChecking || historyLoading || historyAutoFailed || pageLeaving) return;
    const box = root.getBoundingClientRect();
    for (const node of root.querySelectorAll(".history-load[data-history-page]")) {
      if (node.dataset.turnId && node.closest("details:not([open])")) continue;
      const rect = node.getBoundingClientRect();
      if (!node.disabled && rect.height && rect.bottom >= box.top - 120
          && rect.top <= (includeVisibleItems && node.dataset.turnId ? box.bottom : Math.min(box.bottom, box.top + 120))) {
        const version = selectionVersion;
        void loadHistory(true, version, node.dataset.turnId || null).then(() => {
          if (version === selectionVersion) scheduleHistoryLoad(includeVisibleItems);
        });
        break;
      }
    }
  }));
}
