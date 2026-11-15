// 连接生命周期：事件流、轮询、断开/重连、页面挂起与初始连接；实时流、计时器与前台读取代次等状态在 connection-state.js，页面恢复与重试在 page-lifecycle.js。

import { clearRemoteChannels } from "../../channels.js";
import { createRemoteEventStream, sharedEventsAvailable } from "../../events-client.js";
import { postPlatformMessage } from "../../platform.js";
import { recoveryStorageMessage } from "../../recovery-store.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  loadModels, loadSidebar, receive, renderApprovals, renderSidebar, resolvePending, shell,
  showWelcome, updateComposer
} from "../../core/actions.js";
import {
  connected, creating, nativeConnectionVersion, pageLeaving, selectedId, selectionVersion, session,
  setConnected, setPageLeaving, setSelectedId, setSelectionVersion, setSession, setSidebar,
  setThread, sidebar, turns
} from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { recoverQuestionReceipts } from "../approval-requests.js";
import { approvalResizeObserver, setApprovalResizeObserver } from "../approval-state.js";
import { resetClaudeUsage } from "../composer/claude-usage.js";
import { restorePrivateInputs } from "../composer/draft-restore.js";
import { recoveryGeneration, rememberDraft, restoreSmallInputRecoveries } from "../composer/drafts.js";
import { closeContextUsageTip, resetComposerMetrics } from "../composer/metrics.js";
import { clearPendingFeedbackTimer } from "../composer/send-state.js";
import { stopCatalogRefresh } from "../composer/skill-catalog.js";
import { refreshCachedCatalogs, refreshSkillCatalog } from "../composer/skills.js";
import {
  historyController, historyRequest, setHistoryController, setHistoryLoading,
  setHistoryLoadingOlder, setHistoryLoadingTurnId, setHistoryRequest, setThreadRequest,
  syncHistoryButtons, threadRequest
} from "../conversation/history-state.js";
import { pending, pendingTimer } from "../conversation/pending-state.js";
import {
  selectionController, setSelectionController, setThreadReadVersion, suppressAutomaticNewChat,
  threadReadVersion
} from "../conversation/selection-state.js";
import { selectedThreadReadIncomplete } from "../conversation/selection.js";
import { transcriptEvents } from "../conversation/transcript-events.js";
import { rememberTranscript, resetMessages } from "../conversation/transcript.js";
import { restoreGoalComposerDrafts } from "../goal/composer.js";
import { stopGoalLiveTimer } from "../goal/state.js";
import { stopActivityTimer } from "../messages/activity-state.js";
import { drainPendingNotificationOpen, setPendingNotificationOpen } from "../notifications.js";
import { finishQueueDrag } from "../queue/queue.js";
import { backendAvailable, privateInputScope } from "./capabilities.js";
import {
  cancelConnectionProbe, connecting, connectionCycleVersion, connectionErrorMessage,
  connectionPollPending, connectionPollVersion, connectionRecoveryRequest, foregroundReadGeneration,
  initialAuthenticationRequired, recordRuntimeDiagnostic, refreshAll, refreshTimer,
  renderConnectionIndicator, sessionRecoveryReads, setAuthenticationRequired, setConnecting,
  setConnectionPollPending, setConnectionPollVersion, setConnectionRecoveryRequest,
  setForegroundReadGeneration, setRefreshRequest, setRefreshTimer, setSidebarTimer, setStream,
  setStreamBackoff, setStreamRetry, sidebarTimer, stream, streamBackoff, streamRetry,
  withRecoveryErrorSilence
} from "./connection-state.js";
import { readConnectionSession, setConnection, syncConnectionState } from "./connection.js";
import { loadSessionInstances } from "./instance-catalog.js";
import { stopRelatedSessionTimers } from "./related-state.js";
import { resetRelatedSessions } from "./related.js";
import { rememberReadingPosition } from "./thread-cache.js";
import { restoreReadViews } from "./thread-display.js";
import { sessionMenu } from "../shell-state.js";
import { setSidebarPreferenceScope } from "../sidebar/actions.js";
import {
  setSidebarRequest, setSidebarVersion,
  sidebarReadRefreshSuppressions, sidebarRequest, sidebarVersion
} from "../sidebar/list-state.js";
import { stopClientUnreadBadgePolling } from "../sidebar/list.js";

export function disconnect() {
  stopClientUnreadBadgePolling(true);
  resetClaudeUsage({ clearCache: true });
  resetComposerMetrics();
  clearPendingFeedbackTimer();
  $("#video-preview-dialog")?.close();
  clearRemoteChannels();
  resetRelatedSessions();
  setSidebarPreferenceScope(null);
  stopCatalogRefresh();
  stopGoalLiveTimer();
  closeContextUsageTip();
  approvalResizeObserver?.disconnect(); setApprovalResizeObserver(null);
  stream?.close(); setStream(null); stopConnectionPoll();
  cancelConnectionProbe();
  clearTimeout(refreshTimer); clearTimeout(sidebarTimer); clearTimeout(pendingTimer);
  setRefreshTimer(null);
  clearTimeout(streamRetry); setStreamRetry(null);
  setSelectionVersion(selectionVersion + 1); setSidebarVersion(sidebarVersion + 1); setConnected(false);
  sidebarReadRefreshSuppressions.clear();
  resetMessages();
}
export async function reconnectChangedAuthorization() {
  disconnect();
  setPendingNotificationOpen(null);
  setSession(null); setSelectedId(null); setSidebar(null); setThread(null);
  turns.clear(); transcriptEvents.clear();
  await connect();
}
function openStream({ suppressErrors = false } = {}) {
  if (pageLeaving || !session || (document.hidden && !sharedEventsAvailable())) return;
  stream?.close();
  const current = createRemoteEventStream();
  if (pageLeaving || (document.hidden && current.transport === "fallback")) {
    current.close();
    return;
  }
  setStream(current);
  recordRuntimeDiagnostic("stream.create", { phase: "create" });
  const deviceId = session.device.id;
  const isCurrent = () => stream === current && session?.device.id === deviceId;
  current.onopen = async () => {
    if (!isCurrent()) return;
    // 内层线路已经恢复时，撤销外层旧重试，避免刚连上的 socket 又被计时器关闭。
    clearTimeout(streamRetry); setStreamRetry(null);
    recordRuntimeDiagnostic("stream.open", { phase: "open" });
    setStreamBackoff(1000);
    try {
      const refresh = async () => {
        if (!await syncConnectionState(current)) return;
        void resolvePending();
        await refreshAll({ fresh: true });
      };
      await (suppressErrors ? withRecoveryErrorSilence(refresh) : refresh());
    } catch (error) { if (isCurrent() && !suppressErrors) notice(error.message, true); }
  };
  current.onmessage = event => { if (isCurrent()) { try { receive(JSON.parse(event.data)); } catch {} } };
  current.onerror = event => {
    if (!isCurrent()) return;
    recordRuntimeDiagnostic("stream.error", { phase: "failure", reason: event?.reason, errorType: event?.name });
    setConnection(false);
    if (streamRetry !== null) return;
    const delay = streamBackoff;
    setStreamBackoff(Math.min(streamBackoff * 2, 30000));
    reconnectStream(delay);
  };
}
function reconnectStream(delay = streamBackoff, options = {}) {
  if (pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
  clearTimeout(streamRetry);
  recordRuntimeDiagnostic("stream.retry", { phase: "retry", delayMs: delay });
  setStreamRetry(setTimeout(() => {
    setStreamRetry(null);
    if (!session || pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
    recordRuntimeDiagnostic("stream.retry", { phase: "start" });
    try { openStream(options); }
    catch {
      if (session && !pageLeaving)
        reconnectStream(setStreamBackoff(Math.min(streamBackoff * 2, 30000)), options);
    }
  }, delay));
}
export function ensureStream(options = {}) {
  if (pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
  if (session && !streamRetry && (!stream || stream.readyState === EventSource.CLOSED))
    reconnectStream(0, options);
}
export function stopConnectionPoll() {
  setConnectionPollVersion(connectionPollVersion + 1);
  clearInterval(connect.poll);
  connect.poll = null;
  setConnectionPollPending(null);
}
export function startConnectionPoll(deviceId) {
  stopConnectionPoll();
  if (pageLeaving) return;
  const version = connectionPollVersion;
  connect.poll = setInterval(async () => {
    if (pageLeaving || document.hidden || session?.device.id !== deviceId || connectionPollPending) return;
    const source = stream, nativeVersion = nativeConnectionVersion, connectionVersion = connectionCycleVersion;
    const isCurrent = () => !pageLeaving && version === connectionPollVersion && stream === source
      && session?.device.id === deviceId && nativeVersion === nativeConnectionVersion
      && connectionVersion === connectionCycleVersion;
    const request = { version };
    setConnectionPollPending(request);
    ensureStream();
    try {
      let current;
      try {
        current = await readConnectionSession(undefined, undefined, { resourcePriority: "background" });
      } catch {
        if (isCurrent()) setConnection(false);
        return;
      }
      if (!isCurrent()) return;
      if (privateInputScope(current) !== privateInputScope(session)) { await reconnectChangedAuthorization(); return; }
      const wasConnected = connected;
      setSession(current);
      setConnection(backendAvailable(current) && stream?.readyState === EventSource.OPEN);
      // 连接在线不等于当前会话已读完；只补失败或未完成的读取，成功后停止补读。
      if (connected && (!wasConnected || selectedThreadReadIncomplete()
          && !selectionController && !threadRequest && !historyRequest))
        void withRecoveryErrorSilence(() => refreshAll({ fresh: true })).catch(() => {});
      void recoverQuestionReceipts();
      renderApprovals();
      void loadSidebar(null, { resourcePriority: "background" }).catch(() => {});
    }
    finally { if (connectionPollPending === request) setConnectionPollPending(null); }
  }, 15000);
}
export function retireForegroundReads(reason = "页面进入后台") {
  clearPendingFeedbackTimer();
  setForegroundReadGeneration(foregroundReadGeneration + 1);
  setThreadReadVersion(threadReadVersion + 1);
  const controllers = [selectionController, historyController, threadRequest?.controller, sidebarRequest?.controller];
  setSelectionController(null); setHistoryController(null);
  setThreadRequest(null); setHistoryRequest(null); setRefreshRequest(null); setSidebarRequest(null);
  setHistoryLoading(false); setHistoryLoadingTurnId(null); setHistoryLoadingOlder(false);
  for (const controller of controllers) controller?.abort(new DOMException(reason, "AbortError"));
  // 仅退役显示读取；保留草稿、当前内容、写请求及未知回执，不把取消当作读取失败。
  clearTimeout(refreshTimer); setRefreshTimer(null);
  clearTimeout(sidebarTimer); setSidebarTimer(null);
  syncHistoryButtons();
}
export function suspendPage() {
  if (pageLeaving) return;
  stopActivityTimer();
  stopGoalLiveTimer();
  finishQueueDrag();
  sessionMenu?.close(); stopRelatedSessionTimers();
  rememberPageInputs();
  closeContextUsageTip();
  resetClaudeUsage();
  resetComposerMetrics();
  setPageLeaving(true);
  stopClientUnreadBadgePolling(true);
  retireForegroundReads();
  stopCatalogRefresh();
  stream?.close(); setStream(null);
  stopConnectionPoll();
  cancelConnectionProbe();
  clearTimeout(streamRetry); setStreamRetry(null);
}
export function rememberPageInputs() {
  if (session && $("#prompt")) { rememberDraft(); rememberTranscript(); rememberReadingPosition(); }
}
export async function connect(signal) {
  if (connecting) return connecting;
  const promise = connectPage(signal);
  setConnecting(promise);
  try { return await promise; }
  finally { if (connecting === promise) setConnecting(null); }
}
async function connectPage(signal) {
  const previousSession = session;
  let latest, storageError = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    const requestGeneration = recoveryGeneration;
    let expected = null;
    try { expected = await recoveryStore.observe(); } catch {}
    if (requestGeneration !== recoveryGeneration) throw new Error("会话读取已取消，当前授权状态已改变。");
    latest = await readConnectionSession(signal, expected);
    const observation = sessionRecoveryReads.get(latest) || { expected, requestGeneration };
    if (observation.requestGeneration !== recoveryGeneration)
      throw new Error("会话读取已取消，当前授权状态已改变。");
    setSession(latest);
    try {
      await restorePrivateInputs(latest, observation);
      break;
    } catch (error) {
      if (error.name === "RecoveryAuthorizationError") {
        if (session === latest) setSession(previousSession);
        if (attempt === 0 && !signal?.aborted) {
          cancelConnectionProbe();
          continue;
        }
        throw new Error("设备授权仍在其他页面变更，请稍后重新连接；已保存的新授权草稿未被清理。");
      }
      storageError = error;
      if (pending?.recoveryRef && !pending.draft) Object.assign(pending, { invalid: true, recoveryUnavailable: true });
      break;
    }
  }
  if (session !== latest) return;
  if (signal?.aborted) throw new Error("连接已取消，请重试。");
  await restoreReadViews(session);
  if (session !== latest) return;
  if (signal?.aborted) throw new Error("连接已取消，请重试。");
  setAuthenticationRequired(false);
  restoreSmallInputRecoveries();
  postPlatformMessage("authenticated");
  setPageLeaving(false);
  setConnected(false); shell(); restoreGoalComposerDrafts();
  void loadSessionInstances().then(() => loadSidebar(null, { fresh: true })).catch(() => {});
  if (sidebar) renderSidebar();
  if (storageError) notice(recoveryStorageMessage(storageError), true);
  setConnection(false);
  openStream();
  const deviceId = session.device.id;
  startConnectionPoll(deviceId);
  void recoverQuestionReceipts();
  // 用户要求首次进入及上次停在新建时立即可编辑；侧栏读取不能阻塞新建页，
  // 已有会话仍先恢复缓存和原会话身份，不在后台刷新结束后覆盖用户的新选择。
  if (session?.device.id === deviceId && !selectedId && !creating
      && !new URLSearchParams(location.hash.slice(1)).get("thread")) {
    const startNew = !suppressAutomaticNewChat;
    showWelcome({ startNew });
  }
  try { await refreshAll(); }
  catch (error) { if (session?.device.id === deviceId) notice(error.message, true); }
  updateComposer(); void resolvePending(); drainPendingNotificationOpen();
}
export function probeConnectionRecovery() {
  if (!session || pageLeaving || document.hidden || initialAuthenticationRequired)
    return Promise.resolve(false);
  const scope = privateInputScope(session);
  if (connectionRecoveryRequest?.scope === scope) return connectionRecoveryRequest.promise;
  const request = { scope, promise: null };
  setConnectionRecoveryRequest(request);
  // 用户要求点击“连接中断”和网络恢复都主动探测；复用同一请求，保留草稿和未知写回执。
  // 更换轮询代次后仅取消旧连接探测，防止旧网络的迟到结果覆盖新连接状态。
  request.promise = withRecoveryErrorSilence(async () => {
    if (pageLeaving || document.hidden || scope !== privateInputScope(session)) return false;
    startConnectionPoll(session.device.id);
    cancelConnectionProbe();
    ensureStream({ suppressErrors: true });
    const applied = await syncConnectionState(stream, { recoveryProbe: true });
    if (applied && !pageLeaving && !document.hidden && scope === privateInputScope(session)) {
      refreshCachedCatalogs();
      void recoverQuestionReceipts();
      void withRecoveryErrorSilence(() => refreshAll({ fresh: true })).catch(() => {});
    }
    return applied;
  }).finally(() => {
    if (connectionRecoveryRequest !== request) return;
    setConnectionRecoveryRequest(null);
    renderConnectionIndicator();
  });
  renderConnectionIndicator();
  return request.promise;
}
export async function refreshConnectionFromIndicator() {
  const scope = privateInputScope(session);
  try {
    const applied = await probeConnectionRecovery();
    if (applied && !pageLeaving && !document.hidden && scope === privateInputScope(session))
      await Promise.all([loadModels(true), refreshSkillCatalog()]);
  } catch (error) {
    if (!pageLeaving && !document.hidden && scope === privateInputScope(session))
      notice(connectionErrorMessage(error), true);
  }
}
