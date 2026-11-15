// 会话切换：选择会话、新建会话、显示欢迎页，判断所选会话是否读取完整；读取版本与地址栏同步等状态在 selection-state.js。

import { clearComposerAttachments } from "../../attachments.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import { setSelectValue } from "../../ui/select.js";
import {
  loadSidebar, reloadThread, renderApprovals, renderMessages, renderSidebar, scheduleMessages,
  updateComposer
} from "../../core/actions.js";
import {
  connected, creating, pageLeaving, selectedId, selectionVersion, session, setCreating,
  setSelectedId, setSelectionToken, setSelectionVersion, setThread, sidebar, thread, turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { mergeBlockingRequestsRead } from "../approval-requests.js";
import {
  blockingRequestState, notifiedQuestions, questionVersion, replaceAsyncQuestions,
  setNotifiedQuestions
} from "../approval-state.js";
import { renderModelMenu } from "../composer/composer-menus.js";
import {
  assignCreationBackend, creationInstanceId, setCreationInstanceId, setCreationProject
} from "../composer/creation-state.js";
import { restoreDraft, restoreQueuedMessageEdit } from "../composer/draft-restore.js";
import { draftKey, rememberDraft } from "../composer/drafts.js";
import { reconcileRemoteFastPreference } from "../composer/model-catalog.js";
import { resetComposerOptions } from "../composer/new-chat.js";
import { fastModeVersion } from "../composer/options-state.js";
import { renderSelectedSkills } from "../composer/selected-skills.js";
import { setSelectedSkills } from "../composer/skill-catalog.js";
import { compactionVersion, currentCompaction, normalizeCompaction, reconcileCompaction } from "./compaction.js";
import {
  notifiedStatus, notifiedTokenUsage, setNotifiedTokenUsage, statusVersion, tokenUsageVersion
} from "./event-sync.js";
import {
  historyError, historyInitialized, historyLoading, historyRequest, latestHistoryReadFailed,
  messageVersion, setHistoryCursor, setHistoryInitialized, setRecentHistoryPage,
  setTranscriptChecking, threadRequest, transcriptChecking
} from "./history-state.js";
import { restoreTranscript } from "./history-view.js";
import { historyPlaceholder, loadHistory, retireMissingEphemeralTranscript } from "./history.js";
import {
  relatedReturnTranscript, relatedSelectionVersion, selectedThreadProjectId, selectionController,
  setRelatedReturnTranscript, setRelatedSelectionVersion, setSelectionController,
  setSuppressAutomaticNewChat, setThreadReadFailed, setThreadReadVersion, syncNewChatHash,
  syncThreadHash, threadReadFailed, threadReadVersion, welcomeNode
} from "./selection-state.js";
import { refreshTranscriptEvents, replaceTranscriptEvents, transcriptEvents } from "./transcript-events.js";
import { rememberTranscript, resetMessages } from "./transcript.js";
import { goalVersion, notifiedGoal, setNotifiedGoal, setSelectedGoal, stopGoalLiveTimer } from "../goal/state.js";
import { cancelMessageEdit } from "../messages/message-edit.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { applyQueueRead } from "../queue/queue.js";
import { settleSentMessageDisplays } from "../queue/sent-display.js";
import {
  notifiedQueue, notifiedQueueEdits, queueSnapshotVersion, queueVersion, setEditingQueuedMessage,
  setPendingSteer, syncEditableMessageMetadata
} from "../queue/state.js";
import {
  backendAvailable, isActiveThread, normalizeRelatedView, privateInputScope, sideThreadsEnabled,
  subagentsEnabled, validRelatedThreadId
} from "../session/capabilities.js";
import { recoveryErrorSilenceDepth, stream } from "../session/connection-state.js";
import { setConnection } from "../session/connection.js";
import { newChatBackendOptions, threadLocation } from "../session/instances.js";
import {
  relatedMenuTimer, relatedNavigationOwner, relatedSessionView, setRelatedBackPending,
  setRelatedMenuTimer, setRelatedSessionView, sideCreationHistoryKey, sideCreationView
} from "../session/related-state.js";
import {
  animateRelatedSessionExit, applyRelatedThreadMetadata, clearMissingThreadSelection,
  dismissSideCreation, finishRelatedSessionExit, isMissingThreadError, prepareRelatedSessionExit,
  relatedViewForThread, sideThreadExpired, syncRelatedSessionLayout
} from "../session/related.js";
import {
  currentThreadDisplay, discardPersistentThreadDisplay, optionalCachedRead,
  persistentThreadDisplayAllowed, readScope, rememberReadingPosition, threadEphemeralState
} from "../session/thread-cache.js";
import { rememberThreadDisplay } from "../session/thread-display.js";
import { closeMobileSidebar, sessionMenu, sessionTools } from "../shell-state.js";
import { threadDeleted } from "../sidebar/deletion-state.js";
import { applyThreadDeletion } from "../sidebar/thread-deletion.js";

export async function startNewChat(projectId, { focus = true, closeSidebar = true } = {}) {
  if (relatedSessionView) return;
  setSuppressAutomaticNewChat(false);
  cancelMessageEdit({ focus: false });
  if (!session || session.readOnly) return;
  if (closeSidebar) closeMobileSidebar({ focus: false });
  if (projectId === undefined && selectedId && !sidebar) {
    const version = selectionVersion, id = selectedId, deviceId = session.device.id;
    try { await loadSidebar(); } catch {}
    if (version !== selectionVersion || selectedId !== id || session?.device.id !== deviceId
        || session.readOnly || !connected) return;
  }
  const project = projectId === undefined ? selectedThreadProjectId() : projectId;
  closePopup();
  if (selectedId || creating) { rememberDraft(); rememberTranscript({ retainView: true }); }
  resetMessages();
  setEditingQueuedMessage(null);
  setSelectValue($("#project-select"), project);
  setSelectionVersion(selectionVersion + 1); setCreating(true); setSelectedId(null); setThread(null); setHistoryCursor(null); setHistoryInitialized(false); turns.clear(); transcriptEvents.clear(); renderMessages();
  setRelatedSessionView(null); setRelatedReturnTranscript(null); sessionMenu?.close(); syncRelatedSessionLayout();
  setSelectionToken(randomId());
  setCreationProject($("#project-select").value); setNotifiedTokenUsage(null); setThreadReadVersion(threadReadVersion + 1);
  const instance = newChatBackendOptions().find(value => value.value === creationInstanceId) || newChatBackendOptions()[0];
  setCreationInstanceId(instance?.value || "codex:local"); assignCreationBackend(instance?.backend || "codex");
  if ($("#new-chat-backend")) setSelectValue($("#new-chat-backend"), creationInstanceId);
  setConnection(backendAvailable() && stream?.readyState === EventSource.OPEN);
  syncNewChatHash();
  sessionTools?.updateContext();
  restoreDraft(draftKey()); resetComposerOptions(); renderApprovals();
  $("#thread-title").textContent = "新建会话"; $("#thread-path").textContent = "在本机创建会话";
  $("#project-select-slot").hidden = false;
  renderSidebar(); updateComposer();
  rememberReadingPosition();
  if (focus) $("#prompt").focus();
}
export async function selectThread(id, { closeSidebar = true, force = false, relatedView = undefined, replaceHistory = false,
  initialThread = null, preparedView = false, onViewReady = null } = {}) {
  if (!validRelatedThreadId(id) || !session) return;
  setSuppressAutomaticNewChat(false);
  const navigationVersion = setRelatedSelectionVersion(relatedSelectionVersion + 1);
  const nextRelatedView = relatedView === undefined ? relatedViewForThread(id) : normalizeRelatedView(relatedView, id);
  if (nextRelatedView
      && (nextRelatedView.kind === "side" ? !sideThreadsEnabled() : !subagentsEnabled())) {
    const parent = nextRelatedView.openedFromThreadId || nextRelatedView.parentThreadId;
    if (validRelatedThreadId(parent) && parent !== id)
      return selectThread(parent, { closeSidebar, force, relatedView: null, replaceHistory: true });
    return showWelcome();
  }
  const currentRelatedView = normalizeRelatedView(relatedSessionView, selectedId);
  const menuHeld = sessionMenu?.isOpen?.() || sessionMenu?.isSuspended?.();
  const returningToParent = !!(menuHeld && !nextRelatedView && currentRelatedView?.kind === "side"
    && (currentRelatedView.openedFromThreadId === id || currentRelatedView.parentThreadId === id));
  const initialSide = !force && nextRelatedView?.kind === "side" && initialThread?.id === id
    && initialThread.viewKind === "side" && initialThread.sideHistoryVersion === 1
    && initialThread.parentThreadId === (nextRelatedView.parentThreadId || nextRelatedView.openedFromThreadId)
    ? initialThread : null;
  if (threadDeleted(id)) {
    if (!selectedId || selectedId === id) applyThreadDeletion(id);
    return;
  }
  if (sideCreationView && !preparedView && id !== sideCreationView.record.parentThreadId) {
    const view = sideCreationView;
    dismissSideCreation({ canceled: true });
    const route = history.state?.[sideCreationHistoryKey];
    if (route?.owner === relatedNavigationOwner && route.requestId === view.record.id) {
      const state = { ...history.state }; delete state[sideCreationHistoryKey];
      history.replaceState(state, "", location.href);
    }
  }
  const exitTransition = normalizeRelatedView(relatedSessionView, selectedId) && id !== selectedId
    ? prepareRelatedSessionExit() : (finishRelatedSessionExit(), null);
  if (!force && selectedId === id && !creating
      && JSON.stringify(nextRelatedView) === JSON.stringify(normalizeRelatedView(relatedSessionView, id))
      && (thread?.id === id || historyInitialized || selectionController || historyLoading)) {
    setRelatedSessionView(nextRelatedView);
    syncRelatedSessionLayout(); syncThreadHash(id, { replace: replaceHistory });
    if (closeSidebar) { closePopup(); closeMobileSidebar(); }
    if (returningToParent && sessionMenu?.isSuspended()) sessionMenu.resume();
    if (selectedThreadReadIncomplete() && !selectionController && !threadRequest && !historyRequest)
      await reloadThread();
    return;
  }
  stopGoalLiveTimer();
  cancelMessageEdit({ focus: false });
  if (closeSidebar) closePopup();
  rememberDraft();
  if (!force) rememberTranscript({ retainView: true,
    retainRelatedParent: !!nextRelatedView
      && selectedId === (nextRelatedView.openedFromThreadId || nextRelatedView.parentThreadId) });
  if (relatedReturnTranscript && id !== relatedReturnTranscript.threadId
      && (nextRelatedView?.openedFromThreadId || nextRelatedView?.parentThreadId) !== relatedReturnTranscript.threadId)
    setRelatedReturnTranscript(null);
  setRelatedBackPending(!!exitTransition);
  if (!returningToParent && nextRelatedView?.kind !== "side") sessionMenu?.close();
  clearTimeout(relatedMenuTimer); setRelatedMenuTimer(null);
  resetMessages();
  setPendingSteer(null);
  setEditingQueuedMessage(null);
  const version = setSelectionVersion(selectionVersion + 1);
  setSelectionToken(randomId());
  setSelectedId(id); setCreating(false); setThread(initialSide); turns.clear(); transcriptEvents.clear(); setHistoryCursor(null); setHistoryInitialized(false);
  // 用户反馈切换会话后旧消息被补插、时序错乱：清空旧轮次后，之前已被原生接替的成功记录以本次读取的历史为准。
  settleSentMessageDisplays(id);
  setConnection(backendAvailable() && stream?.readyState === EventSource.OPEN);
  setRelatedSessionView(nextRelatedView);
  if (preparedView) $(".workspace")?.classList.add("related-session-prepared");
  syncRelatedSessionLayout();
  sessionTools?.updateContext();
  restoreDraft(id); resetComposerOptions(); restoreQueuedMessageEdit();
  setNotifiedTokenUsage(null); setNotifiedGoal(null);
  setNotifiedQuestions(null);
  const read = setThreadReadVersion(threadReadVersion + 1), usageVersion = tokenUsageVersion, fastReadVersion = fastModeVersion,
    runtimeVersion = statusVersion, transcriptReadVersion = messageVersion,
    questionReadVersion = questionVersion,
    requestsReadVersion = blockingRequestState(id).generation,
    goalReadVersion = goalVersion, compactionReadVersion = compactionVersion, queueReadVersion = queueVersion;
  renderApprovals();
  $("#project-select-slot").hidden = true;
  $("#messages").replaceChildren();
  let cached = force || initialSide || preparedView ? null : restoreTranscript(id);
  if (initialSide) {
    rememberThreadDisplay();
    setHistoryInitialized(true); setTranscriptChecking(false);
    setRecentHistoryPage({ turns: [], transcriptEvents: [], historyCursor: null,
      followMessages: true, scrollTop: 0, sideHistoryVersion: 1 });
    mergeBlockingRequestsRead(id, thread, requestsReadVersion);
    applyQueueRead(id, thread, queueReadVersion);
    reconcileCompaction(id, thread.compaction);
    renderMessages();
  } else if (!cached) $("#messages").replaceChildren(historyPlaceholder());
  const readDisplay = !currentThreadDisplay() && nextRelatedView?.kind !== "side";
  const readTranscript = !force && !initialSide && !preparedView && !cached;
  updateComposer();
  if (closeSidebar) closeMobileSidebar();
  if ((readDisplay || readTranscript) && readScope && threadEphemeralState(id) !== true) {
    const owner = readScope;
    const [display, snapshot] = await optionalCachedRead(Promise.all([
      readDisplay ? readCache.get(owner, "thread", id) : null,
      readTranscript ? readCache.get(owner, "transcript", id) : null
    ]), 50) || [];
    if (version !== selectionVersion || selectedId !== id || owner !== readScope || threadDeleted(id)) return;
    if (display?.id === id && persistentThreadDisplayAllowed(id, display))
      rememberThreadDisplay({ ...reconcileRemoteFastPreference(display, fastReadVersion),
        ...(usageVersion !== tokenUsageVersion ? { tokenUsage: notifiedTokenUsage } : {}),
        ...(goalReadVersion !== goalVersion ? { goal: notifiedGoal } : {}),
        ...(queueReadVersion !== queueVersion && Array.isArray(notifiedQueue)
          ? { queuedMessages: notifiedQueue, queueSnapshotVersion } : {}) },
        { persist: usageVersion !== tokenUsageVersion });
    else if (display && !persistentThreadDisplayAllowed(id, display))
      discardPersistentThreadDisplay(id);
    if (snapshot?.turns?.length && persistentThreadDisplayAllowed(id, snapshot)) {
      transcriptCache.put(session.device.id, id, snapshot);
      cached = restoreTranscript(id);
    } else if (snapshot && !persistentThreadDisplayAllowed(id, snapshot)) {
      discardPersistentThreadDisplay(id);
    }
  }
  rememberReadingPosition();
  const summary = [...(sidebar?.recent || []), ...(sidebar?.pinnedThreads || []),
    ...(sidebar?.projects || []).flatMap(project => project.threads || [])].find(item => item.id === id);
  $("#thread-title").textContent = initialSide?.title || relatedSessionView?.name || summary?.title || cached?.title || "正在打开会话";
  $("#thread-path").textContent = threadLocation(initialSide) || threadLocation(summary) || threadLocation(cached) || "正在读取会话设置…";
  renderSidebar(); updateComposer();
  if (exitTransition) {
    const scope = privateInputScope(session);
    await animateRelatedSessionExit(exitTransition);
    if (navigationVersion !== relatedSelectionVersion || version !== selectionVersion || selectedId !== id
        || scope !== privateInputScope(session) || threadDeleted(id)) return;
  }
  syncThreadHash(id, { replace: replaceHistory });
  onViewReady?.();
  if (returningToParent && sessionMenu?.isSuspended() && !relatedSessionView && selectedId === id) {
    sessionMenu.resume();
  }
  if (initialSide) return;
  if (pageLeaving || document.hidden) return;
  const latest = loadHistory(false, version);
  const controller = new AbortController();
  setSelectionController(controller);
  try {
    const result = await api("/api/read/thread", { threadId: id }, undefined, controller.signal);
    if (version !== selectionVersion || read !== threadReadVersion || selectedId !== id || threadDeleted(id)) return;
    setThread(reconcileRemoteFastPreference(result.thread, fastReadVersion));
    if (thread?.id !== id) throw new Error("会话返回内容与当前会话不一致");
    setThreadReadFailed(false);
    applyRelatedThreadMetadata(thread);
    mergeBlockingRequestsRead(id, thread, requestsReadVersion);
    if (questionReadVersion !== questionVersion && notifiedQuestions)
      replaceAsyncQuestions(thread, notifiedQuestions);
    replaceTranscriptEvents(thread.transcriptEvents, transcriptReadVersion);
    refreshTranscriptEvents();
    if (!$("#model-settings").hidden) renderModelMenu();
    if (usageVersion !== tokenUsageVersion) thread.tokenUsage = notifiedTokenUsage;
    if (runtimeVersion !== statusVersion && notifiedStatus) thread.status = notifiedStatus;
    if (goalReadVersion !== goalVersion) thread.goal = notifiedGoal;
    applyQueueRead(id, thread, queueReadVersion);
    if (notifiedQueue) {
      thread.queuedMessages = notifiedQueue;
      if (notifiedQueueEdits) thread.queuedEdits = notifiedQueueEdits;
    }
    if (compactionReadVersion === compactionVersion) reconcileCompaction(id, thread.compaction);
    else thread.compaction = normalizeCompaction(currentCompaction(id));
    rememberThreadDisplay();
    $("#thread-title").textContent = thread.viewKind === "subagent" ? thread.agentName || thread.title || "子代理" : thread.title;
    $("#thread-path").textContent = threadLocation(thread);
    syncEditableMessageMetadata();
    updateComposer(); renderApprovals();
    if (historyInitialized) scheduleMessages();
  } catch (error) {
    if (!controller.signal.aborted && version === selectionVersion && selectedId === id
        && isMissingThreadError(error, id)) {
      await clearMissingThreadSelection(id);
      return;
    }
    if (!controller.signal.aborted && version === selectionVersion && selectedId === id) {
      setThreadReadFailed(true);
      if (error.code === 404) retireMissingEphemeralTranscript(id, error);
      if (!recoveryErrorSilenceDepth && (error.code === 404 || !currentThreadDisplay()))
        notice(`会话设置读取失败，可点击刷新重试：${error.message}`, true);
    }
  } finally {
    if (selectionController === controller) setSelectionController(null);
  }
  await latest;
  if (version === selectionVersion && !isActiveThread())
    void markVisibleThreadRead(id).catch(error => notice(error.message, true));
}
export function showWelcome({ startNew = true, clearHash = false } = {}) {
  setRelatedSelectionVersion(relatedSelectionVersion + 1);
  finishRelatedSessionExit();
  setRelatedSessionView(null); setRelatedReturnTranscript(null); sessionMenu?.close(); syncRelatedSessionLayout();
  setPendingSteer(null);
  if (session && !session.readOnly && startNew) {
    setSuppressAutomaticNewChat(false);
    void startNewChat("", { focus: false, closeSidebar: false });
    return;
  }
  cancelMessageEdit({ focus: false });
  if (!clearHash) { rememberDraft(); rememberTranscript({ retainView: true }); }
  resetMessages();
  setEditingQueuedMessage(null);
  setSelectionVersion(selectionVersion + 1); setThreadReadVersion(threadReadVersion + 1); setSelectionToken(randomId());
  setSelectedId(null); setThread(null); setCreating(false); setHistoryCursor(null); setHistoryInitialized(false); turns.clear(); transcriptEvents.clear(); setNotifiedTokenUsage(null);
  if (clearHash) {
    const hash = new URLSearchParams(location.hash.slice(1));
    if (hash.has("thread")) {
      hash.delete("thread");
      history.replaceState(null, "", `${location.pathname}${location.search}${hash.size ? `#${hash}` : ""}`);
    }
  }
  sessionTools?.updateContext();
  $("#prompt").value = ""; setSelectedSkills([]); setSelectedGoal(null); clearComposerAttachments(); resetComposerOptions();
  $("#thread-title").textContent = "远程会话"; $("#thread-path").textContent = "";
  $("#project-select-slot").hidden = true;
  $("#messages").replaceChildren(welcomeNode());
  renderSelectedSkills(); renderApprovals(); renderSidebar(); updateComposer();
  rememberReadingPosition();
}

export function selectedThreadReadIncomplete() {
  return !!selectedId && !creating && !threadDeleted(selectedId) && !sideThreadExpired(selectedId)
    && (thread?.id !== selectedId || threadReadFailed || latestHistoryReadFailed
      || !historyInitialized || transcriptChecking || !!historyError);
}
