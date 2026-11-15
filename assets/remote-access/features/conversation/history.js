// 历史读取：分页读取、Claude 历史排序、增量合并与重新读取会话。

import { recordDiagnostic } from "../../diagnostics.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  pageLeaving, selectedId, selectionVersion, setThread, setTurns, sidebar, thread, turns
} from "../../core/state.js";
import { api } from "../api.js";
import {
  blockingRequestState, mergeBlockingRequestsRead, notifiedQuestions, questionVersion,
  renderApprovals, replaceAsyncQuestions
} from "../approvals.js";
import { renderModelMenu } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import { fastModeVersion, reconcileRemoteFastPreference } from "../composer/models.js";
import { compactionVersion, currentCompaction, normalizeCompaction, reconcileCompaction } from "./compaction.js";
import { notifiedStatus, notifiedTokenUsage, statusVersion, tokenUsageVersion } from "./events.js";
import { rememberTranscript, renderMessages, setRecentHistoryPage, syncHistoryButtons } from "./history-view.js";
import { setThreadReadFailed, setThreadReadVersion, syncThreadHash, threadReadVersion } from "./selection.js";
import {
  enrichTurnTranscriptEvents, mergeTranscriptEvents, refreshTranscriptEvents,
  replaceTranscriptEvents, transcriptEvents
} from "./transcript-events.js";
import { goalVersion, notifiedGoal } from "../goal/status.js";
import { messageImageMediaReads, messageImageReadSequence, setMessageImageReadSequence } from "../messages/media.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { applyQueueRead, notifiedQueue, notifiedQueueEdits, queueVersion } from "../queue/queue.js";
import { syncEditableMessageMetadata } from "../queue/queued-edit.js";
import { claudeThreadId, normalizeRelatedView, threadLocation } from "../session/capabilities.js";
import {
  foregroundReadGeneration, recoveryErrorSilenceDepth, refreshTimer, setRefreshTimer
} from "../session/lifecycle.js";
import {
  applyRelatedThreadMetadata, clearMissingThreadSelection, isMissingThreadError,
  relatedSessionView, relatedThreadViews, setRelatedSessionView, sideThreadExpired
} from "../session/related.js";
import {
  discardPersistentThreadDisplay, rememberThreadDisplay, retireMissingEphemeralTranscript,
  threadEphemeralState
} from "../session/thread-cache.js";
import { threadDeleted } from "../sidebar/actions.js";

export let historyCursor = null, historyLoading = false;
export function setHistoryCursor(value) { return historyCursor = value; }
export function setHistoryLoading(value) { return historyLoading = value; }
export let historyLoadingVersion = -1;
export function setHistoryLoadingVersion(value) { return historyLoadingVersion = value; }
export let historyInitialized = false;
export function setHistoryInitialized(value) { return historyInitialized = value; }
export let historyError = "", historyController = null, historyLoadingTurnId = null;
export function setHistoryError(value) { return historyError = value; }
export function setHistoryController(value) { return historyController = value; }
export function setHistoryLoadingTurnId(value) { return historyLoadingTurnId = value; }
export let latestHistoryReadFailed = false;
export function setLatestHistoryReadFailed(value) { return latestHistoryReadFailed = value; }
export let historyLoadingOlder = false;
export function setHistoryLoadingOlder(value) { return historyLoadingOlder = value; }
export let historyAutoFailed = false;
export function setHistoryAutoFailed(value) { return historyAutoFailed = value; }
export let transcriptChecking = false;
export function setTranscriptChecking(value) { return transcriptChecking = value; }
export let messageVersion = 0;
export function setMessageVersion(value) { return messageVersion = value; }
let claudeHistoryOrder = null;
export let threadRequest = null, historyRequest = null;
export function setThreadRequest(value) { return threadRequest = value; }
export function setHistoryRequest(value) { return historyRequest = value; }
export const itemVersions = new Map(), turnVersions = new Map();
export const turnFieldVersions = new Map();
export const partialItems = new Set();
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
  historyRequest = request;
  try { await request.promise; }
  finally { if (historyRequest === request) historyRequest = null; }
}
async function readHistoryPage(older, version, itemsTurnId, generation = foregroundReadGeneration) {
  if (pageLeaving || document.hidden || generation !== foregroundReadGeneration
      || version !== selectionVersion || !selectedId || threadDeleted(selectedId)) return;
  // 执行过期不等于历史不可读取；仍从后台补齐最终回复和分页内容，不恢复发送能力。
  const itemTurn = itemsTurnId ? turns.get(itemsTurnId) : null;
  if ((itemsTurnId && !itemTurn?.itemsNextCursor) || (older && !itemsTurnId && !historyCursor)) return;
  historyLoading = true; historyLoadingVersion = version;
  const latestPage = !older && !itemsTurnId;
  historyLoadingTurnId = itemsTurnId; historyLoadingOlder = older; historyAutoFailed = false;
  if (latestPage) historyError = "";
  syncHistoryButtons();
  if (!historyInitialized) renderMessages();
  const id = selectedId, readVersion = messageVersion, checkingCached = transcriptChecking && latestPage,
    mediaReadVersion = setMessageImageReadSequence(messageImageReadSequence + 1);
  const initialIds = new Set(turns.keys());
  const controller = new AbortController();
  const historyStartedAt = performance.now();
  recordDiagnostic("remote.history", { phase: "request" });
  historyController = controller;
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
    if (checkingCached && !pruneCachedHistory(result.data, readVersion)) historyCursor = result.nextCursor;
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
      if (older || !historyInitialized) historyCursor = result.nextCursor;
    }
    if (!itemsTurnId && claudeThreadId(id)) {
      const order = orderClaudeHistoryTurns(result.turnIds, readVersion);
      const cursor = claudeHistoryCursor(order);
      if (cursor !== undefined) historyCursor = cursor;
    }
    historyInitialized = true;
    if (latestPage) { transcriptChecking = false; latestHistoryReadFailed = false; }
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
    if (latestPage) latestHistoryReadFailed = true;
    if (recoveryErrorSilenceDepth) {
      historyAutoFailed = false;
      return;
    }
    historyAutoFailed = true;
    if (!historyInitialized) {
      historyError = `最新消息加载失败：${error.message}`;
      renderMessages();
    } else {
      if (latestPage) {
        historyError = `最新消息加载失败：${error.message}`;
        renderMessages();
      }
      notice(error.message, true);
    }
  }
  finally {
    if (historyController === controller) {
      historyController = null; historyLoading = false; historyLoadingTurnId = null; historyLoadingOlder = false;
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
  if (changed) messageVersion = Math.max(messageVersion, version);
  return changed;
}
export function orderClaudeHistoryTurns(ids, readVersion = messageVersion) {
  const sameSelection = claudeHistoryOrder?.threadId === selectedId
    && claudeHistoryOrder.selectionVersion === selectionVersion;
  // 用户要求长历史与实时消息保持原生顺序；较早 HTTP 回包不能撤回新通知给出的顺序。
  if (Array.isArray(ids) && (!sameSelection || readVersion >= claudeHistoryOrder.version)) {
    claudeHistoryOrder = { threadId: selectedId, selectionVersion, version: readVersion, ids: [...ids] };
  } else if (!sameSelection) return ids;
  const order = claudeHistoryOrder.ids, known = new Set(order);
  const entries = order.filter(id => turns.has(id)).map(id => [id, turns.get(id)]);
  // 原生列表尚未包含的实时输入或本地预览继续保留，不能把补历史等同删除。
  entries.push(...[...turns].filter(([id]) => !known.has(id)));
  turns.clear();
  for (const [id, turn] of entries) turns.set(id, turn);
  return order;
}
export function claudeHistoryCursor(ids) {
  if (!Array.isArray(ids)) return undefined;
  if (!ids.length) return null;
  if (!ids.some(id => turns.has(id))) return undefined;
  // 从最新轮次向前寻找连续已加载区间；零散旧缓存不能让中间未读取的轮次被跳过。
  let first = ids.length;
  while (first > 0 && turns.has(ids[first - 1])) --first;
  return first === 0 ? null : String(first);
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
function mergeLiveText(snapshot, live, partial = false, newer = true) {
  snapshot = String(snapshot || ""); live = String(live || "");
  if (!live || snapshot.startsWith(live) || (partial && snapshot.includes(live))) return snapshot;
  if (!snapshot || live.startsWith(snapshot)) return live;
  if (!newer) return snapshot;
  if (!partial) return live;
  const prefix = new Uint32Array(live.length);
  for (let index = 1, matched = 0; index < live.length; index++) {
    while (matched && live[index] !== live[matched]) matched = prefix[matched - 1];
    if (live[index] === live[matched]) matched++;
    prefix[index] = matched;
  }
  let overlap = 0;
  for (const character of snapshot.slice(-live.length).split("")) {
    while (overlap && (overlap === live.length || character !== live[overlap])) overlap = prefix[overlap - 1];
    if (character === live[overlap]) overlap++;
  }
  // 此处仅处理读取开始后收到的未知开头增量；即使零重叠，也要保留历史前缀。
  return snapshot + live.slice(overlap);
}
export function normalizeDisplayedUserMessage(item) {
  if (!item || item.type !== "steeringUserMessage"
      || item.status !== "accepted" && typeof item.serverUserMessageId !== "string") return item;
  const id = typeof item.serverUserMessageId === "string" && item.serverUserMessageId
    ? item.serverUserMessageId : item.id;
  if (typeof id !== "string" || !id) return item;
  const content = Array.isArray(item.input) ? item.input
    : Array.isArray(item.content) ? item.content : [];
  return {
    ...item,
    type: "userMessage",
    id,
    ...(typeof (item.clientUserMessageId ?? item.clientId) === "string"
      ? { clientId: item.clientUserMessageId ?? item.clientId } : {}),
    content
  };
}
export function normalizeDisplayedTurnItems(items) {
  const byId = new Map();
  for (const source of items) {
    const item = normalizeDisplayedUserMessage(source);
    // 原生正文和 steered 确认共用消息 ID；分页、实时轮次与重进缓存必须一致保留一份正文。
    if (byId.get(item.id)?.type === "userMessage" && item.type === "steered") continue;
    byId.set(item.id, item);
  }
  return [...byId.values()];
}
export function completedItem(item) {
  // 普通消息的 completed 可能仅为原生展示推导，不能证明这份正文已经定稿。
  return item?.remoteItemCompleted === true || Number.isFinite(item?.completedAtMs)
    || item?.type === "contextCompaction" && item.completed === true
    || ["completed", "failed", "interrupted"].includes(item?.status);
}
export function mergeRemoteImageProjection(previous, snapshot, item, mediaReadVersion = 0) {
  const prior = previous && (messageImageMediaReads.get(previous) || messageImageMediaReads.get(previous.remoteImages)),
    retained = messageImageMediaReads.get(item) || messageImageMediaReads.get(item.remoteImages);
  if (!prior && !retained && !(mediaReadVersion > 0 && Array.isArray(snapshot?.remoteImages))) return item;
  const assets = new Map((item.remoteImages || []).map(asset => [asset.source, asset]));
  if (!assets.size) return item;
  const reads = new Map(prior || []);
  for (const [source, read] of retained || [])
    if (!reads.has(source) || reads.get(source).version < read.version) reads.set(source, read);
  // 用户要求已完成正文仍可恢复图片；媒体映射独立按读取发起序号更新，旧缓存/迟到历史不能撤回新 URL。
  // 序号只保存在 WeakMap 中，不进入会话正文缓存，页面重载后首次真实读取仍能恢复服务重启后的注册。
  if (mediaReadVersion > 0) for (const asset of snapshot?.remoteImages || []) {
    if (!assets.has(asset?.source) || !/^\/api\/images\/[A-Za-z0-9_-]{20,128}$/.test(asset?.url || "")
        || (reads.get(asset.source)?.version || 0) > mediaReadVersion) continue;
    reads.set(asset.source, { version: mediaReadVersion, asset });
  }
  for (const [source, read] of reads) if (assets.has(source)) assets.set(source, read.asset);
  const images = [...assets.values()];
  const projected = JSON.stringify(images) === JSON.stringify(item.remoteImages) ? item : { ...item, remoteImages: images };
  if (reads.size) {
    messageImageMediaReads.set(projected, reads);
    // 流式正文会浅拷贝条目；共享媒体数组也保留读取序号，复制正文不能丢失图片的新旧关系。
    messageImageMediaReads.set(projected.remoteImages, reads);
  }
  return projected;
}
export function mergeItemProjection(previous, item) {
  // 单条状态事件也可能晚于正文到达，不能用同 ID 的 steered 把已接受的引导内容改回空状态。
  if (previous?.id === item.id && previous.type === "userMessage" && item.type === "steered") return previous;
  const projection = {};
  for (const field of ["messageMetadata", "sentAtMs", "startedAtMs", "completedAtMs", "completed",
    "remoteImages", "remoteVideos", "clientId", "remoteCompactionInput", "remoteHistoryContext", "remoteItemCompleted"])
    if (!Object.hasOwn(item, field) && Object.hasOwn(previous || {}, field)) projection[field] = previous[field];
  const merged = mergeRemoteImageProjection(previous, null, { ...projection, ...item });
  return previous && JSON.stringify(previous) === JSON.stringify(merged) ? previous : merged;
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
  threadRequest = request;
  try { await request.promise; }
  finally { if (threadRequest === request) threadRequest = null; }
}
