// 实时事件处理：接收服务事件并更新会话、侧栏、队列与审批。

import { handleRemoteChannelEvent } from "../../channels.js";
import { receiveDiagnosticRequest } from "../../diagnostics.js";
import { notice } from "../../ui/notice.js";
import {
  nativeConnectionVersion, selectedId, session, setNativeConnectionVersion, setThread, thread,
  turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import {
  applyBlockingRequestsSnapshot, blockingRequestState, invalidateBlockingRequests,
  notifiedQuestions, questionVersion, renderApprovals, replaceAsyncQuestions, setNotifiedQuestions,
  setQuestionVersion
} from "../approvals.js";
import { sending, updateComposer } from "../composer/composer.js";
import { rememberRemoteFastPreference } from "../composer/models.js";
import { rememberPermissionSelection } from "../composer/permissions.js";
import { refreshSkillCatalog } from "../composer/skills.js";
import { receiveCompactionLifecycle, reconcileCompaction } from "./compaction.js";
import { rememberTranscript, scheduleMessages } from "./history-view.js";
import {
  claudeHistoryCursor, completedItem, itemVersions, mergeItemProjection, messageVersion,
  normalizeDisplayedTurnItems, normalizeDisplayedUserMessage, orderClaudeHistoryTurns,
  partialItems, reloadThread, retireClaudeHistoryTurns, setHistoryCursor, setMessageVersion,
  turnFieldVersions, turnVersions
} from "./history.js";
import { acceptedSendItem, pending, resolvePending } from "./pending.js";
import { selectThread } from "./selection.js";
import { enrichTurnTranscriptEvents, mergeTranscriptEvents, transcriptEventTypes } from "./transcript-events.js";
import { goalVersion, notifiedGoal, renderGoalStatus, setGoalVersion, setNotifiedGoal } from "../goal/status.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import {
  applyQueueSnapshot, queueVersion, reloadQueue, setNotifiedQueue, setNotifiedQueueEdits,
  setQueueVersion
} from "../queue/queue.js";
import { clearSentMessageDisplays, reconcileSentMessageDisplays, retiredSendPayloads } from "../queue/sent-display.js";
import { backendAvailable } from "../session/capabilities.js";
import { refreshConnectedSession, setConnection } from "../session/connection.js";
import {
  refreshTimer, setRefreshTimer, setSidebarTimer, sidebarTimer, stream, withRecoveryErrorSilence
} from "../session/lifecycle.js";
import { requireAuthentication } from "../session/login.js";
import { relatedSessionView, retireNativeSideThreads, scheduleRelatedMenuRefresh } from "../session/related.js";
import {
  currentThreadDisplay, readScope, rememberBackgroundThreadDisplay, rememberThreadDisplay,
  threadDisplayKey, threadDisplays
} from "../session/thread-cache.js";
import { applyThreadDeletion, threadDeleted } from "../sidebar/actions.js";
import {
  loadSidebar, recentPrioritySelectionSuppressions, resetDesktopObservation,
  scheduleDesktopObservation, sidebarReadRefreshSuppressions
} from "../sidebar/list.js";

export let tokenUsageVersion = 0, notifiedTokenUsage = null;
export function setNotifiedTokenUsage(value) { return notifiedTokenUsage = value; }
export let statusVersion = 0, notifiedStatus = null;
export function setNotifiedStatus(value) { return notifiedStatus = value; }
export function scheduleReload() {
  if (refreshTimer) return;
  setRefreshTimer(setTimeout(() => {
    setRefreshTimer(null);
    void reloadThread().catch(error => notice(error.message, true));
  }, 450));
}
function scheduleSidebarRefresh() {
  clearTimeout(sidebarTimer);
  setSidebarTimer(setTimeout(() => void loadSidebar(null, { fresh: true }).catch(() => {}), 1200));
}
function receiveClaudeHistory(params) {
  const id = params.threadId;
  // 临时正文不改变其他会话的标题、未读或队列；只让正式快照触发侧栏读取，避免流式片段反复刷新列表。
  if (id !== selectedId) { if (!params.preview) scheduleSidebarRefresh(); return; }
  if (!params.thread || params.thread.id !== id || !Array.isArray(params.turns)) return;
  const version = setMessageVersion(messageVersion + 1);
  if (!params.preview) {
    setThread({ ...thread, ...params.thread });
    setNotifiedGoal(thread.goal); setGoalVersion(goalVersion + 1);
    if (thread.compaction) reconcileCompaction(id, thread.compaction);
    notifiedStatus = thread.status; ++statusVersion;
    setNotifiedQueue(thread.queuedMessages); setNotifiedQueueEdits(thread.queuedEdits || []); setQueueVersion(queueVersion + 1);
  }
  // 用户要求同步后旧内部摘要气泡消失；完整快照给出全部有效轮次 ID，仅移除已失效的原生轮次，不删除待发送预览。
  if (!params.preview) retireClaudeHistoryTurns(params.turnIds, version, version);
  // 共享会话提供同一原生历史的完整投影；替换对应轮次，CLI 消息不依赖刷新或再次发送。
  let applied = false;
  for (const snapshot of [...params.turns].reverse()) {
    if (!snapshot?.id || !Array.isArray(snapshot.items)) continue;
    let value = { ...snapshot, items: normalizeDisplayedTurnItems(snapshot.items) };
    if (params.preview) {
      // 用户要求 Claude 正文稳定：终端预览只能更新当前临时气泡，不能用其附带的旧快照覆盖正式消息。
      // 原生正文基线变化、连接切换或轮次结束后，迟到预览必须丢弃；旧服务无基线时等待正式历史。
      const current = turns.get(value.id), basis = value._nativePreviewBasis;
      if (!current || current.status !== "inProgress" || !/^[0-9a-f]{64}$/i.test(basis || "")
          || current._nativePreviewBasis !== basis || !current._process || current._process !== value._process
          || current._nativeUserId !== value._nativeUserId) continue;
      const id = `native-live-${current._nativeUserId}`;
      const preview = value.items.find(item => item.id === id && item.type === "agentMessage"
        && item._nativePreview === true && typeof item.text === "string" && item.text.trim());
      if (!preview) continue;
      const previous = current.items?.find(item => item.id === id && item._nativePreview === true);
      if (previous) {
        const before = previous.text.replace(/\s/gu, ""), after = preview.text.replace(/\s/gu, "");
        if (before === after || !after.startsWith(before)) continue;
      }
      value = { ...current, items: [...(current.items || []).filter(item => item.id !== id), preview] };
    }
    turns.set(value.id, enrichTurnTranscriptEvents(value));
    applied = true;
    if (!params.preview) turnVersions.set(value.id, version);
    for (const item of value.items) {
      if (params.preview && item._nativePreview !== true) continue;
      itemVersions.set(`${value.id}:${item.id}`, version);
      partialItems.delete(`${value.id}:${item.id}`);
    }
  }
  if (!params.preview) {
    const order = orderClaudeHistoryTurns(params.turnIds, version);
    const cursor = claudeHistoryCursor(order);
    if (cursor !== undefined) setHistoryCursor(cursor);
  }
  // 正文流只刷新消息；完整原生快照再保存缓存和更新侧栏，避免逐字重复写历史。
  if (params.preview) { if (applied) scheduleMessages(); return; }
  reconcileSentMessageDisplays(id, thread.queuedMessages);
  rememberThreadDisplay(thread);
  rememberTranscript();
  updateComposer();
  scheduleMessages();
  scheduleSidebarRefresh();
  if (params.completed) void markVisibleThreadRead().catch(() => {});
}
export function receive(event) {
  if (handleRemoteChannelEvent(event)) return;
  const rawParams = event.params || {};
  const eventThreadId = rawParams.threadId ?? rawParams.conversationId ?? rawParams.thread?.id;
  if (event.method === "claude/history/updated") {
    receiveClaudeHistory(rawParams);
    return;
  }
  const suppressSidebarRefresh = event.method === "remote/sidebarChanged"
    && rawParams.reason === "read"
    && typeof eventThreadId === "string" && sidebarReadRefreshSuppressions.has(eventThreadId);
  scheduleRelatedMenuRefresh(event, { suppressReadRefresh: suppressSidebarRefresh });
  if (event.method === "remote/notification") return;
  if (event.method === "remote/unauthorized") { requireAuthentication(); return; }
  if (["remote/disconnected", "remote/reconnected"].includes(event.method)) {
    const available = event.method === "remote/reconnected";
    const serviceAvailable = event.params?.serviceConnected ?? available;
    setNativeConnectionVersion(nativeConnectionVersion + 1);
    if (session) {
      session.serviceConnected = serviceAvailable;
    }
    setConnection(backendAvailable() && stream?.readyState === EventSource.OPEN);
    resetDesktopObservation();
    if (available) {
      scheduleDesktopObservation();
      void withRecoveryErrorSilence(() => refreshConnectedSession()).catch(() => {});
    }
    return;
  }
  if (event.method === "remote/resync") {
    if (typeof rawParams.threadId === "string") {
      // 桌面重订阅只补对应会话，避免多个列表订阅同时恢复时重复刷新整个页面。
      scheduleSidebarRefresh();
      if (rawParams.threadId === selectedId) scheduleReload();
      return;
    }
    // 服务重启可能保留同一设备授权；全量重连不能被 12 秒的旧订阅去重窗口挡住。
    resetDesktopObservation();
    scheduleDesktopObservation();
    void withRecoveryErrorSilence(() => refreshConnectedSession()).catch(() => {});
    return;
  }
  const params = { ...event.params || {} };
  const displayItem = normalizeDisplayedUserMessage(params.item);
  const displayTurnItems = Array.isArray(params.turn?.items)
    ? normalizeDisplayedTurnItems(params.turn.items) : null;
  const turnChanged = displayTurnItems
    && (displayTurnItems.length !== params.turn.items.length
      || displayTurnItems.some((item, index) => item !== params.turn.items[index]));
  if (displayItem !== params.item) params.item = displayItem;
  if (turnChanged) params.turn = { ...params.turn, items: displayTurnItems };
  if (event.method === "remote/diagnosticsRequested") {
    receiveDiagnosticRequest(params);
    return;
  }
  if (typeof eventThreadId === "string") {
    if (["thread/started", "turn/started", "turn/completed"].includes(event.method))
      sidebarReadRefreshSuppressions.delete(eventThreadId);
    const runtimeStatus = params.status?.status ?? params.status;
    const runtimeType = typeof runtimeStatus === "string" ? runtimeStatus : runtimeStatus?.type;
    if (["thread/started", "turn/started", "remote/requestsChanged"].includes(event.method)
        || event.method === "thread/status/changed"
          && ["active", "inProgress", "running", "pending"].includes(runtimeType))
      recentPrioritySelectionSuppressions.delete(eventThreadId);
  }
  if (event.method === "remote/relatedThreadsChanged") {
    void retireNativeSideThreads(Array.isArray(params.closedThreadIds) ? params.closedThreadIds : [])
      .catch(error => notice(error.message, true));
    if (relatedSessionView?.kind === "side" && relatedSessionView.parentThreadId === eventThreadId)
      scheduleReload();
    scheduleSidebarRefresh();
    return;
  }
  if (event.method === "remote/sidebarChanged" && eventThreadId === selectedId
      && relatedSessionView?.kind === "side" && typeof params.title === "string" && params.title !== thread?.title)
    scheduleReload();
  if (event.method === "thread/deleted" && typeof eventThreadId === "string") {
    applyThreadDeletion(eventThreadId);
    void loadSidebar(null, { fresh: true }).catch(() => {});
    return;
  }
  if (typeof eventThreadId === "string" && threadDeleted(eventThreadId)) return;
  if (event.method === "thread/settings/updated" && typeof params.fastMode === "boolean") {
    rememberRemoteFastPreference(params.threadId, params.fastMode);
    if (params.threadId === selectedId) updateComposer();
  }
  if (event.method === "thread/settings/updated" && typeof params.permissionMode === "string") {
    rememberPermissionSelection(params.threadId, params.permissionMode, params.permissionRevision || 0);
    if (selectedId === params.threadId) updateComposer();
  }
  rememberBackgroundThreadDisplay(event, params);
  if (event.method === "remote/requestsChanged" || event.method === "serverRequest/resolved") {
    const id = params.threadId ?? params.conversationId;
    let resolved = event.method === "serverRequest/resolved";
    if (resolved && typeof id === "string") {
      const state = blockingRequestState(id);
      if (state.retiredEpochs.has(params.requestsEpoch)) return;
      if (state.epoch && params.requestsEpoch !== state.epoch) resolved = false;
    }
    const snapshot = event.method === "remote/requestsChanged"
      && applyBlockingRequestsSnapshot(id, params.requests, params);
    if (!snapshot && (typeof id !== "string" || id === selectedId)) {
      if (typeof id === "string") invalidateBlockingRequests(id, params.requestId, resolved);
      else if (selectedId) invalidateBlockingRequests(selectedId);
      scheduleReload();
    } else if (!snapshot && typeof id === "string") {
      invalidateBlockingRequests(id, params.requestId, resolved);
    }
    scheduleSidebarRefresh();
    return;
  }
  const pendingClientId = pending?.payload?.clientUserMessageId ?? pending?.confirmationClientUserMessageId;
  if (!sending && pending?.action === "send" && params.threadId === pending.payload?.threadId
      && typeof pendingClientId === "string" && params.item && acceptedSendItem([params.item], pendingClientId))
    void resolvePending();
  if (event.method === "remote/asyncQuestionsChanged") {
    scheduleSidebarRefresh();
    if (params.threadId === selectedId) {
      setNotifiedQuestions(Array.isArray(params.requests) ? params.requests : []);
      setQuestionVersion(questionVersion + 1);
      if (thread?.id === selectedId) {
        const previous = (thread.requests || []).filter(request => request.method === "remote/requestUserInputAsync");
        replaceAsyncQuestions(thread, notifiedQuestions); renderApprovals();
        if (previous.some(request => !notifiedQuestions.some(current => current.id === request.id))) scheduleReload();
      }
    }
    return;
  }
  if (params.item?.type === "contextCompaction" && ["item/started", "item/completed"].includes(event.method)) {
    params.item = { ...params.item, completed: event.method === "item/completed" };
  }
  if (event.method === "thread/queue/changed") {
    if (params.threadId === selectedId && !applyQueueSnapshot(params.threadId, params.queuedMessages, params.queuedEdits, params))
      void reloadQueue().catch(error => notice(error.message, true));
    if (!sending && pending?.action === "send" && params.threadId === pending.payload?.threadId
        && typeof pendingClientId === "string" && (params.queuedMessages || []).some(message =>
          (message.clientUserMessageId ?? message.submissionOptions?.clientUserMessageId
            ?? (!message.serverQueuedMessageId ? message.id : null)) === pendingClientId))
      void resolvePending();
    return;
  }
  if (event.method === "remote/threadReverted" && typeof eventThreadId === "string") {
    if (pending?.action === "send" && pending.payload?.threadId === eventThreadId)
      retiredSendPayloads.add(pending.payload);
    clearSentMessageDisplays(eventThreadId, { retire: true });
    transcriptCache.remove(session?.device.id, eventThreadId);
    threadDisplays.delete(threadDisplayKey(eventThreadId));
    if (readScope) {
      void readCache.remove(readScope, "transcript", eventThreadId).catch(() => {});
      void readCache.remove(readScope, "thread", eventThreadId).catch(() => {});
    }
    if (eventThreadId === selectedId)
      void selectThread(selectedId, { closeSidebar: false, force: true }).catch(error => notice(error.message, true));
    return;
  }
  receiveCompactionLifecycle(event, params);
  if (params.threadId === selectedId) {
    const version = setMessageVersion(messageVersion + 1);
    if (event.method === "thread/goal/updated" || event.method === "thread/goal/cleared") {
      setNotifiedGoal(event.method === "thread/goal/updated" ? params.goal : null);
      setGoalVersion(goalVersion + 1);
      if (thread) thread.goal = notifiedGoal;
      rememberThreadDisplay({ id: selectedId, goal: notifiedGoal });
      renderGoalStatus();
    }
    if (event.method === "thread/tokenUsage/updated") {
      notifiedTokenUsage = params.tokenUsage; ++tokenUsageVersion;
      if (thread) thread.tokenUsage = params.tokenUsage;
      const display = currentThreadDisplay();
      rememberThreadDisplay({ ...display, id: selectedId, tokenUsage: params.tokenUsage },
        { persist: !!thread || !!display?.model });
      updateComposer();
    }
    if (event.method === "thread/status/changed") {
      notifiedStatus = params.status; ++statusVersion;
      if (thread) thread.status = notifiedStatus;
      updateComposer(); scheduleMessages();
      const status = params.status?.status || params.status;
      const statusType = typeof status === "string" ? status : status?.type;
      if (!["active", "inProgress", "running", "pending"].includes(statusType))
        scheduleReload();
    }
    const turnId = params.turnId || params.turn?.id;
    if (turnId) {
      if (transcriptEventTypes.has(params.item?.type))
        mergeTranscriptEvents([{ turnId, item: params.item }]);
      mergeTranscriptEvents((params.turn?.items || []).filter(item => transcriptEventTypes.has(item?.type))
        .map(item => ({ turnId, item })));
      let turn = turns.get(turnId);
      if (!turn) { turn = { id: turnId, status: "inProgress", items: [] }; turns.set(turnId, turn); }
      if (["turn/started", "turn/completed"].includes(event.method)) {
        const timing = Object.fromEntries(["startedAt", "completedAt", "durationMs", "turnStartedAtMs", "turnCompletedAtMs"]
          .filter(field => Number.isFinite(turn[field])).map(field => [field, turn[field]]));
        const items = new Map((turn.items || []).map(item => [item.id, item]));
        for (const item of params.turn?.items || []) {
          const incoming = event.method === "turn/completed" ? { ...item, remoteItemCompleted: true } : item;
          items.set(item.id, mergeItemProjection(items.get(item.id), incoming));
          itemVersions.set(`${turnId}:${item.id}`, version); partialItems.delete(`${turnId}:${item.id}`);
        }
        Object.assign(turn, params.turn, { items: [...items.values()] });
        for (const [field, value] of Object.entries(timing)) if (!Number.isFinite(turn[field])) turn[field] = value;
        if (Number.isFinite(params.startedAtMs)) turn.turnStartedAtMs = params.startedAtMs;
        if (Number.isFinite(params.completedAtMs)) turn.turnCompletedAtMs = params.completedAtMs;
        turn.status = event.method === "turn/started" ? "inProgress" : params.turn?.status || "completed";
        turnVersions.set(turnId, version);
        notifiedStatus = { type: event.method === "turn/started" ? "active" : "idle" }; ++statusVersion;
        if (thread) thread.status = notifiedStatus;
        updateComposer();
        if (event.method === "turn/completed") {
          scheduleReload();
          void markVisibleThreadRead().catch(error => notice(error.message, true));
        }
      }
      if (params.item) {
        let index = turn.items.findIndex(item => item.id === params.item.id);
        if (index < 0 && params.item.type === "userMessage" && typeof params.item.clientId === "string")
          index = turn.items.findIndex(item => item.remotePendingInput && item.clientId === params.item.clientId);
        const previous = index >= 0 ? turn.items[index] : null;
        if (event.method !== "item/started" || !completedItem(previous)) {
          let item = { ...mergeItemProjection(previous, params.item), ...(!params.item.status
            ? event.method === "item/completed" ? { status: "completed" } : event.method === "item/started" ? { status: "inProgress" } : {} : {}),
            ...(event.method === "item/completed" ? { remoteItemCompleted: true } : {}) };
          if (Number.isFinite(params.startedAtMs)) item.startedAtMs = params.startedAtMs;
          if (Number.isFinite(params.completedAtMs)) item.completedAtMs = params.completedAtMs;
          if (event.method === "item/completed" && completedItem(previous)
              && Number.isFinite(previous.completedAtMs) && Number.isFinite(item.completedAtMs)
              && previous.completedAtMs > item.completedAtMs)
            item = mergeItemProjection(item, previous);
          if (index < 0) turn.items.push(item); else turn.items[index] = item;
          itemVersions.set(`${turnId}:${item.id}`, version); partialItems.delete(`${turnId}:${item.id}`);
        }
      }
      if (event.method === "turn/plan/updated") {
        turn.plan = params.plan; turnFieldVersions.set(`${turnId}:plan`, version);
      }
      if (event.method === "turn/diff/updated") {
        turn.diff = params.diff; turnFieldVersions.set(`${turnId}:diff`, version);
      }
      if (params.itemId && (params.delta != null || event.method === "item/reasoning/summaryPartAdded" || event.method === "item/fileChange/patchUpdated")) {
        let index = turn.items.findIndex(item => item.id === params.itemId);
        let item = index < 0 ? null : turn.items[index];
        if (!item) {
          const type = event.method.includes("commandExecution") ? "commandExecution" : event.method.includes("reasoning") ? "reasoning"
            : event.method.includes("fileChange") ? "fileChange" : event.method.includes("/plan/") ? "plan" : "agentMessage";
          item = { id: params.itemId, type, status: "inProgress" };
          index = turn.items.length; turn.items.push(item);
          partialItems.add(`${turnId}:${item.id}`); scheduleReload();
        } else if (completedItem(item)) {
          return;
        }
        item = { ...item };
        if (event.method.includes("commandExecution")) item.aggregatedOutput = (item.aggregatedOutput || "") + params.delta;
        else if (event.method.includes("reasoning")) {
          const field = event.method === "item/reasoning/textDelta" ? "content" : "summary";
          const partIndex = params[field === "content" ? "contentIndex" : "summaryIndex"] || 0;
          const parts = Array.isArray(item[field]) ? [...item[field]] : item[field] ? [item[field]] : [];
          while (parts.length <= partIndex) parts.push("");
          const part = parts[partIndex], text = (typeof part === "string" ? part : part?.text || "") + (params.delta || "");
          parts[partIndex] = typeof part === "object" && part ? { ...part, text } : text;
          item[field] = parts;
        } else if (event.method === "item/fileChange/patchUpdated") item.changes = params.changes;
        else item.text = (item.text || "") + params.delta;
        turn.items[index] = item; itemVersions.set(`${turnId}:${item.id}`, version);
      }
      turns.set(turnId, enrichTurnTranscriptEvents(turn));
      scheduleMessages();
      if (event.method === "turn/completed") rememberTranscript();
    }
    if (["turn/started", "thread/settings/updated"].includes(event.method)) scheduleReload();
  }
  if (event.method === "skills/changed") {
    void refreshSkillCatalog().catch(() => {});
  }
  if (!suppressSidebarRefresh && ["thread/started", "turn/started", "turn/completed", "remote/sidebarChanged", "thread/name/updated", "thread/status/changed",
    "project/changed", "thread/project/updated",
    "remote/requestsChanged", "serverRequest/resolved"].includes(event.method)) {
    scheduleSidebarRefresh();
  }
}
