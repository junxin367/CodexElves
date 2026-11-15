// 页面与连接生命周期：事件流、轮询、断开/重连、页面挂起与恢复、初始连接。

import { clearRemoteChannels } from "../../channels.js";
import { recordDiagnostic } from "../../diagnostics.js";
import { createRemoteEventStream, sharedEventsAvailable } from "../../events-client.js";
import { postPlatformMessage } from "../../platform.js";
import { recoveryStorageMessage } from "../../recovery-store.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  connected, creating, nativeConnectionVersion, pageLeaving, selectedId, selectionVersion, session,
  setConnected, setPageLeaving, setSelectedId, setSelectionVersion, setSession, setSidebar,
  setThread, sidebar, thread, turns
} from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import {
  approvalResizeObserver, recoverQuestionReceipts, renderApprovals, setApprovalResizeObserver
} from "../approvals.js";
import { resetClaudeUsage, syncClaudeUsage } from "../composer/claude-usage.js";
import { clearPendingFeedbackTimer, updateComposer } from "../composer/composer.js";
import {
  recoveryGeneration, rememberDraft, restorePrivateInputs, restoreSmallInputRecoveries
} from "../composer/drafts.js";
import { closeContextUsageTip, resetComposerMetrics, syncComposerMetrics } from "../composer/metrics.js";
import { loadModels } from "../composer/models.js";
import { refreshCachedCatalogs, refreshSkillCatalog, stopCatalogRefresh } from "../composer/skills.js";
import { receive } from "../conversation/events.js";
import { rememberTranscript, resetMessages, syncHistoryButtons } from "../conversation/history-view.js";
import {
  historyController, historyError, historyInitialized, historyRequest, latestHistoryReadFailed,
  reloadThread, setHistoryController, setHistoryLoading, setHistoryLoadingOlder,
  setHistoryLoadingTurnId, setHistoryRequest, setThreadRequest, threadRequest, transcriptChecking
} from "../conversation/history.js";
import { pending, pendingTimer, resolvePending } from "../conversation/pending.js";
import {
  selectionController, selectThread, setSelectionController, setThreadReadVersion, showWelcome,
  suppressAutomaticNewChat, threadReadFailed, threadReadVersion
} from "../conversation/selection.js";
import { transcriptEvents } from "../conversation/transcript-events.js";
import { restoreGoalComposerDrafts } from "../goal/composer.js";
import { stopGoalLiveTimer, syncGoalLiveTimer } from "../goal/status.js";
import { stopActivityTimer, syncActivityTimer } from "../messages/activity.js";
import { drainPendingNotificationOpen, setPendingNotificationOpen } from "../notifications.js";
import { finishQueueDrag } from "../queue/queue.js";
import { backendAvailable, normalizeRelatedView, privateInputScope } from "./capabilities.js";
import {
  cancelConnectionProbe, connectionCycleVersion, connectionPollVersion, readConnectionSession,
  refreshConnectedSession, renderConnectionIndicator, sessionRecoveryReads, setConnection,
  setConnectionPollVersion, syncConnectionState
} from "./connection.js";
import { loadSessionInstances } from "./instances.js";
import {
  connectionErrorMessage, handleInitialConnectionError, initialAuthenticationRequired,
  setAuthenticationRequired, showConnectionRecovery
} from "./login.js";
import {
  relatedExitTransition, relatedHistoryKey, relatedSessionView, resetRelatedSessions,
  scheduleRelatedOperationCheck, scheduleRelatedSessionCheck, sideThreadExpired,
  stopRelatedSessionTimers
} from "./related.js";
import { rememberReadingPosition, restoreReadViews } from "./thread-cache.js";
import { sessionMenu, shell } from "../shell.js";
import { setSidebarPreferenceScope, threadDeleted } from "../sidebar/actions.js";
import {
  loadSidebar, renderSidebar, resetDesktopObservation, scheduleDesktopObservation,
  setSidebarRequest, setSidebarVersion, sidebarReadRefreshSuppressions, sidebarRequest,
  sidebarVersion, startClientUnreadBadgePolling, stopClientUnreadBadgePolling,
  syncClientUnreadBadge
} from "../sidebar/list.js";

export let stream = null;
export let sidebarTimer = null, refreshTimer = null;
export function setSidebarTimer(value) { return sidebarTimer = value; }
export function setRefreshTimer(value) { return refreshTimer = value; }
let streamRetry = null, streamBackoff = 1000;
let connectionPollPending = null;
export let connectionRecoveryRequest = null;
export let recoveryErrorSilenceDepth = 0;
let connecting = null;
let refreshRequest = null;
export let foregroundReadGeneration = 0;
export function disconnect() {
  stopClientUnreadBadgePolling(true);
  resetDesktopObservation();
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
  stream?.close(); stream = null; stopConnectionPoll();
  cancelConnectionProbe();
  clearTimeout(refreshTimer); clearTimeout(sidebarTimer); clearTimeout(pendingTimer);
  refreshTimer = null;
  clearTimeout(streamRetry); streamRetry = null;
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
export function withRecoveryErrorSilence(task) {
  recoveryErrorSilenceDepth++;
  return Promise.resolve().then(task).finally(() => { recoveryErrorSilenceDepth--; });
}
export async function refreshAll({ fresh = false } = {}) {
  if (!session || pageLeaving || document.hidden) return;
  const generation = foregroundReadGeneration;
  if (refreshRequest?.deviceId === session.device.id && refreshRequest.generation === generation) {
    if (fresh) refreshRequest.again = true;
    return refreshRequest.promise;
  }
  const deviceId = session?.device.id;
  const request = { deviceId, generation, again: false, promise: null };
  const promise = (async () => {
    do {
      if (generation !== foregroundReadGeneration) return;
      request.again = false;
      const sidebarRead = loadSidebar(null, { fresh }).catch(() => null);
      const id = selectedId || (!creating ? new URLSearchParams(location.hash.slice(1)).get("thread") : null);
      try {
        await (id ? selectedId
          ? reloadThread() : selectThread(id, { closeSidebar: false }) : Promise.resolve());
      } catch (error) {
        if (generation !== foregroundReadGeneration) return;
        if (!request.again || deviceId !== session?.device.id) throw error;
      }
      await sidebarRead;
      fresh = true;
    } while (request.again && generation === foregroundReadGeneration && deviceId === session?.device.id);
  })();
  request.promise = promise; refreshRequest = request;
  try { await promise; }
  finally { if (refreshRequest?.promise === promise) refreshRequest = null; }
}
function openStream({ suppressErrors = false } = {}) {
  if (pageLeaving || !session || (document.hidden && !sharedEventsAvailable())) return;
  stream?.close();
  const current = createRemoteEventStream();
  if (pageLeaving || (document.hidden && current.transport === "fallback")) {
    current.close();
    return;
  }
  stream = current;
  recordRuntimeDiagnostic("stream.create", { phase: "create" });
  const deviceId = session.device.id;
  const isCurrent = () => stream === current && session?.device.id === deviceId;
  current.onopen = async () => {
    if (!isCurrent()) return;
    // 内层线路已经恢复时，撤销外层旧重试，避免刚连上的 socket 又被计时器关闭。
    clearTimeout(streamRetry); streamRetry = null;
    recordRuntimeDiagnostic("stream.open", { phase: "open" });
    streamBackoff = 1000;
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
    streamBackoff = Math.min(streamBackoff * 2, 30000);
    reconnectStream(delay);
  };
}
function reconnectStream(delay = streamBackoff, options = {}) {
  if (pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
  clearTimeout(streamRetry);
  recordRuntimeDiagnostic("stream.retry", { phase: "retry", delayMs: delay });
  streamRetry = setTimeout(() => {
    streamRetry = null;
    if (!session || pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
    recordRuntimeDiagnostic("stream.retry", { phase: "start" });
    try { openStream(options); }
    catch {
      if (session && !pageLeaving)
        reconnectStream(streamBackoff = Math.min(streamBackoff * 2, 30000), options);
    }
  }, delay);
}
function ensureStream(options = {}) {
  if (pageLeaving || (document.hidden && !sharedEventsAvailable())) return;
  if (session && !streamRetry && (!stream || stream.readyState === EventSource.CLOSED))
    reconnectStream(0, options);
}
function stopConnectionPoll() {
  setConnectionPollVersion(connectionPollVersion + 1);
  clearInterval(connect.poll);
  connect.poll = null;
  connectionPollPending = null;
}
function startConnectionPoll(deviceId) {
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
    connectionPollPending = request;
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
      scheduleDesktopObservation();
      setConnection(backendAvailable(current) && stream?.readyState === EventSource.OPEN);
      // 连接在线不等于当前会话已读完；只补失败或未完成的读取，成功后停止补读。
      if (connected && (!wasConnected || selectedThreadReadIncomplete()
          && !selectionController && !threadRequest && !historyRequest))
        void withRecoveryErrorSilence(() => refreshAll({ fresh: true })).catch(() => {});
      void recoverQuestionReceipts();
      renderApprovals();
      void loadSidebar(null, { resourcePriority: "background" }).catch(() => {});
    }
    finally { if (connectionPollPending === request) connectionPollPending = null; }
  }, 15000);
}
export function selectedThreadReadIncomplete() {
  return !!selectedId && !creating && !threadDeleted(selectedId) && !sideThreadExpired(selectedId)
    && (thread?.id !== selectedId || threadReadFailed || latestHistoryReadFailed
      || !historyInitialized || transcriptChecking || !!historyError);
}
function retireForegroundReads(reason = "页面进入后台") {
  clearPendingFeedbackTimer();
  ++foregroundReadGeneration;
  setThreadReadVersion(threadReadVersion + 1);
  const controllers = [selectionController, historyController, threadRequest?.controller, sidebarRequest?.controller];
  resetDesktopObservation();
  setSelectionController(null); setHistoryController(null);
  setThreadRequest(null); setHistoryRequest(null); refreshRequest = null; setSidebarRequest(null);
  setHistoryLoading(false); setHistoryLoadingTurnId(null); setHistoryLoadingOlder(false);
  for (const controller of controllers) controller?.abort(new DOMException(reason, "AbortError"));
  // 仅退役显示读取；保留草稿、当前内容、写请求及未知回执，不把取消当作读取失败。
  clearTimeout(refreshTimer); refreshTimer = null;
  clearTimeout(sidebarTimer); sidebarTimer = null;
  syncHistoryButtons();
}
function suspendPage() {
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
  stream?.close(); stream = null;
  stopConnectionPoll();
  cancelConnectionProbe();
  clearTimeout(streamRetry); streamRetry = null;
}
function rememberPageInputs() {
  if (session && $("#prompt")) { rememberDraft(); rememberTranscript(); rememberReadingPosition(); }
}
export function recordRuntimeDiagnostic(name, fields = {}) {
  recordDiagnostic(name, {
    threadId: selectedId, visible: !document.hidden, online: navigator.onLine !== false,
    pageLeaving, connected, readyState: stream?.readyState ?? EventSource.CLOSED,
    generation: foregroundReadGeneration, ...fields
  });
}
function resumePage() {
  const wasLeaving = pageLeaving;
  setPageLeaving(false);
  recordRuntimeDiagnostic("page.resume");
  if (!session) { retryConnectionRecovery(); return; }
  if (!wasLeaving) return;
  refreshCachedCatalogs();
  syncGoalLiveTimer();
  startConnectionPoll(session.device.id);
  scheduleRelatedOperationCheck();
  ensureStream({ suppressErrors: wasLeaving });
  void withRecoveryErrorSilence(() => refreshConnectedSession())
    .catch(() => {}).finally(drainPendingNotificationOpen);
}
export async function connect(signal) {
  if (connecting) return connecting;
  const promise = connectPage(signal);
  connecting = promise;
  try { return await promise; }
  finally { if (connecting === promise) connecting = null; }
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
function probeConnectionRecovery() {
  if (!session || pageLeaving || document.hidden || initialAuthenticationRequired)
    return Promise.resolve(false);
  const scope = privateInputScope(session);
  if (connectionRecoveryRequest?.scope === scope) return connectionRecoveryRequest.promise;
  const request = { scope, promise: null };
  connectionRecoveryRequest = request;
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
    connectionRecoveryRequest = null;
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
export function retryConnectionRecovery({ suppressStreamErrors = false, recoveryProbe = false } = {}) {
  if (pageLeaving || document.hidden || navigator.onLine === false || initialAuthenticationRequired || connecting) return;
  if (!session) { void connect().catch(handleInitialConnectionError); return; }
  if (recoveryProbe) {
    void probeConnectionRecovery().catch(() => {});
    return;
  }
  void recoverQuestionReceipts();
  refreshCachedCatalogs();
  ensureStream({ suppressErrors: suppressStreamErrors });
  void withRecoveryErrorSilence(() => refreshConnectedSession())
    .catch(() => {});
}
// 网络、可见性与页面离开/返回等页面生命周期事件。
export function installPageLifecycle() {
  window.addEventListener("online", () => {
    retryConnectionRecovery({ recoveryProbe: true });
  });
  window.addEventListener("offline", () => {
    stopCatalogRefresh();
    retireForegroundReads("网络已离线");
    if (session) setConnection(false);
    else if (!initialAuthenticationRequired) showConnectionRecovery(new TypeError("网络已离线"));
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      stopCatalogRefresh();
      resetClaudeUsage();
      resetComposerMetrics();
      if (session) { rememberTranscript(); rememberReadingPosition(); }
      retireForegroundReads();
      stopActivityTimer();
      stopGoalLiveTimer();
      finishQueueDrag();
      closeContextUsageTip();
      stopConnectionPoll();
      cancelConnectionProbe();
      clearTimeout(streamRetry); streamRetry = null;
      startClientUnreadBadgePolling();
      return;
    }
    stopClientUnreadBadgePolling();
    syncClientUnreadBadge();
    recordRuntimeDiagnostic("page.visible");
    if (pageLeaving) { resumePage(); return; }
    syncClaudeUsage();
    syncComposerMetrics();
    syncActivityTimer();
    syncGoalLiveTimer();
    scheduleRelatedSessionCheck();
    if (session) startConnectionPoll(session.device.id);
    retryConnectionRecovery({ suppressStreamErrors: true });
  });
  window.addEventListener("pagehide", suspendPage);
  window.addEventListener("blur", () => finishQueueDrag());
  window.addEventListener("beforeunload", () => {
    recordRuntimeDiagnostic("page.beforeunload");
    // 导航可能被取消或交给 App 内置浏览器；只有 pagehide 才表示当前文档真正离开。
    rememberPageInputs();
  });
  window.addEventListener("pageshow", resumePage);
  window.addEventListener("popstate", event => {
    if (!session) return;
    const id = new URLSearchParams(location.hash.slice(1)).get("thread");
    const view = normalizeRelatedView(event.state?.[relatedHistoryKey], id);
    if (id && (relatedExitTransition || id !== selectedId || JSON.stringify(view) !== JSON.stringify(relatedSessionView)))
      void selectThread(id, { closeSidebar: false, relatedView: view }).catch(error => notice(error.message, true));
    else if (!id && selectedId) showWelcome();
  });
}
