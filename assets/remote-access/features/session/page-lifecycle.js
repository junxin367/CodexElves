// 页面生命周期：前后台切换时暂停与恢复，以及连接恢复重试。

import { notice } from "../../ui/notice.js";
import { selectThread, showWelcome } from "../../core/actions.js";
import { pageLeaving, selectedId, session, setPageLeaving } from "../../core/state.js";
import { recoverQuestionReceipts } from "../approval-requests.js";
import { resetClaudeUsage, syncClaudeUsage } from "../composer/claude-usage.js";
import { syncComposerMetrics } from "../composer/composer.js";
import { closeContextUsageTip, resetComposerMetrics } from "../composer/metrics.js";
import { stopCatalogRefresh } from "../composer/skill-catalog.js";
import { refreshCachedCatalogs } from "../composer/skills.js";
import { rememberTranscript } from "../conversation/transcript.js";
import { stopGoalLiveTimer, syncGoalLiveTimer } from "../goal/state.js";
import { stopActivityTimer, syncActivityTimer } from "../messages/activity-state.js";
import { drainPendingNotificationOpen } from "../notifications.js";
import { finishQueueDrag } from "../queue/queue.js";
import { normalizeRelatedView } from "./capabilities.js";
import {
  cancelConnectionProbe, connecting, initialAuthenticationRequired, recordRuntimeDiagnostic,
  setStreamRetry, streamRetry, withRecoveryErrorSilence
} from "./connection-state.js";
import { refreshConnectedSession, setConnection } from "./connection.js";
import {
  connect, ensureStream, probeConnectionRecovery, rememberPageInputs, retireForegroundReads,
  startConnectionPoll, stopConnectionPoll, suspendPage
} from "./lifecycle.js";
import { handleInitialConnectionError, showConnectionRecovery } from "./login.js";
import { relatedExitTransition, relatedHistoryKey, relatedSessionView } from "./related-state.js";
import { scheduleRelatedOperationCheck, scheduleRelatedSessionCheck } from "./related.js";
import { rememberReadingPosition } from "./thread-cache.js";
import { startClientUnreadBadgePolling, stopClientUnreadBadgePolling, syncClientUnreadBadge } from "../sidebar/list.js";

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
      clearTimeout(streamRetry); setStreamRetry(null);
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
