// 会话切换：选择会话、新建会话、欢迎页与地址栏同步。

import { clearComposerAttachments } from "../../attachments.js";
import { $, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import { setSelectValue } from "../../ui/select.js";
import {
  connected, creating, pageLeaving, selectedId, selectionVersion, session, setCreating,
  setSelectedId, setSelectionToken, setSelectionVersion, setThread, sidebar, thread, turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import {
  blockingRequestState, mergeBlockingRequestsRead, notifiedQuestions, questionVersion,
  renderApprovals, replaceAsyncQuestions, setNotifiedQuestions
} from "../approvals.js";
import { renderModelMenu } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import {
  draftKey, rememberDraft, resetComposerOptions, restoreDraft, restoreQueuedMessageEdit
} from "../composer/drafts.js";
import { fastModeVersion, reconcileRemoteFastPreference } from "../composer/models.js";
import {
  assignCreationBackend, creationInstanceId, setCreationInstanceId, setCreationProject
} from "../composer/new-chat.js";
import { renderSelectedSkills, setSelectedSkills } from "../composer/skills.js";
import { compactionVersion, currentCompaction, normalizeCompaction, reconcileCompaction } from "./compaction.js";
import {
  notifiedStatus, notifiedTokenUsage, setNotifiedTokenUsage, statusVersion, tokenUsageVersion
} from "./events.js";
import {
  historyPlaceholder, rememberTranscript, renderMessages, resetMessages, restoreTranscript,
  scheduleMessages, setRecentHistoryPage
} from "./history-view.js";
import {
  historyInitialized, historyLoading, historyRequest, loadHistory, messageVersion, reloadThread,
  setHistoryCursor, setHistoryInitialized, setTranscriptChecking, threadRequest
} from "./history.js";
import { refreshTranscriptEvents, replaceTranscriptEvents, transcriptEvents } from "./transcript-events.js";
import { setSelectedGoal } from "../goal/composer.js";
import { goalVersion, notifiedGoal, setNotifiedGoal, stopGoalLiveTimer } from "../goal/status.js";
import { cancelMessageEdit } from "../messages/message-edit.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import {
  applyQueueRead, notifiedQueue, notifiedQueueEdits, queueSnapshotVersion, queueVersion,
  setPendingSteer
} from "../queue/queue.js";
import { setEditingQueuedMessage, syncEditableMessageMetadata } from "../queue/queued-edit.js";
import { settleSentMessageDisplays } from "../queue/sent-display.js";
import {
  backendAvailable, isActiveThread, isClaude, newChatBackendOptions, normalizeRelatedView,
  privateInputScope, sideThreadsEnabled, subagentsEnabled, threadLocation, validRelatedThreadId
} from "../session/capabilities.js";
import { setConnection } from "../session/connection.js";
import { recoveryErrorSilenceDepth, selectedThreadReadIncomplete, stream } from "../session/lifecycle.js";
import {
  animateRelatedSessionExit, applyRelatedThreadMetadata, clearMissingThreadSelection,
  conversationHistoryKey, conversationHistoryPosition, dismissSideCreation,
  finishRelatedSessionExit, isMissingThreadError, prepareRelatedSessionExit, relatedExitTransition,
  relatedHistoryKey, relatedMenuTimer, relatedNavigationOwner, relatedSessionView,
  relatedViewForThread, setRelatedBackPending, setRelatedMenuTimer, setRelatedSessionView,
  sideCreationHistoryKey, sideCreationView, syncRelatedSessionLayout
} from "../session/related.js";
import {
  currentThreadDisplay, discardPersistentThreadDisplay, optionalCachedRead,
  persistentThreadDisplayAllowed, readScope, rememberReadingPosition, rememberThreadDisplay,
  retireMissingEphemeralTranscript, threadEphemeralState
} from "../session/thread-cache.js";
import { closeMobileSidebar, sessionMenu, sessionTools } from "../shell.js";
import { applyThreadDeletion, threadDeleted } from "../sidebar/actions.js";
import { loadSidebar, renderSidebar } from "../sidebar/list.js";

export let selectionController = null;
export function setSelectionController(value) { return selectionController = value; }
export let threadReadFailed = false;
export function setThreadReadFailed(value) { return threadReadFailed = value; }
export let suppressAutomaticNewChat = false;
export function setSuppressAutomaticNewChat(value) { return suppressAutomaticNewChat = value; }
export let threadReadVersion = 0;
export function setThreadReadVersion(value) { return threadReadVersion = value; }
export let relatedReturnTranscript = null;
export function setRelatedReturnTranscript(value) { return relatedReturnTranscript = value; }
export let relatedSelectionVersion = 0;
export function setRelatedSelectionVersion(value) { return relatedSelectionVersion = value; }
function selectedThreadProjectId() {
  if (!selectedId || !sidebar) return "";
  const projects = sidebar.projects || [];
  const summary = [...(sidebar.recent || []), ...(sidebar.pinnedThreads || [])]
    .find(item => item.id === selectedId);
  const projectId = summary?.projectId !== undefined ? summary.projectId
    : projects.find(project => project.threads?.some(item => item.id === selectedId))?.id;
  return projects.some(project => project.id === projectId) ? projectId : "";
}
export async function startNewChat(projectId, { focus = true, closeSidebar = true } = {}) {
  if (relatedSessionView) return;
  suppressAutomaticNewChat = false;
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
  setRelatedSessionView(null); relatedReturnTranscript = null; sessionMenu?.close(); syncRelatedSessionLayout();
  setSelectionToken(randomId());
  setCreationProject($("#project-select").value); setNotifiedTokenUsage(null); ++threadReadVersion;
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
  suppressAutomaticNewChat = false;
  const navigationVersion = ++relatedSelectionVersion;
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
    relatedReturnTranscript = null;
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
  const read = ++threadReadVersion, usageVersion = tokenUsageVersion, fastReadVersion = fastModeVersion,
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
  selectionController = controller;
  try {
    const result = await api("/api/read/thread", { threadId: id }, undefined, controller.signal);
    if (version !== selectionVersion || read !== threadReadVersion || selectedId !== id || threadDeleted(id)) return;
    setThread(reconcileRemoteFastPreference(result.thread, fastReadVersion));
    if (thread?.id !== id) throw new Error("会话返回内容与当前会话不一致");
    threadReadFailed = false;
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
      threadReadFailed = true;
      if (error.code === 404) retireMissingEphemeralTranscript(id, error);
      if (!recoveryErrorSilenceDepth && (error.code === 404 || !currentThreadDisplay()))
        notice(`会话设置读取失败，可点击刷新重试：${error.message}`, true);
    }
  } finally {
    if (selectionController === controller) selectionController = null;
  }
  await latest;
  if (version === selectionVersion && !isActiveThread())
    void markVisibleThreadRead(id).catch(error => notice(error.message, true));
}
export function syncThreadHash(id, { replace = false } = {}) {
  if (relatedExitTransition && id === selectedId) return;
  const hash = new URLSearchParams(location.hash.slice(1));
  const previousId = hash.get("thread");
  hash.set("thread", id);
  const state = { ...(history.state || {}) };
  state[conversationHistoryKey] = conversationHistoryPosition(state) + (previousId !== id && !replace ? 1 : 0);
  if (relatedSessionView?.threadId === id) state[relatedHistoryKey] = normalizeRelatedView(relatedSessionView, id);
  else delete state[relatedHistoryKey];
  // 只在新会话条目中清除旧工具路由，保留此前变更页的历史条目。
  if (previousId !== id) delete state.codexElvesTools;
  const url = `${location.pathname}${location.search}#${hash}`;
  if (!replace && previousId !== id) history.pushState(state, "", url);
  else history.replaceState(state, "", url);
}
function syncNewChatHash() {
  const hash = new URLSearchParams(location.hash.slice(1));
  if (!hash.has("thread")) return;
  hash.delete("thread");
  const state = { ...(history.state || {}) };
  state[conversationHistoryKey] = conversationHistoryPosition(state) + 1;
  delete state[relatedHistoryKey]; delete state.codexElvesTools;
  // 新建会话是独立历史条目，工具页返回时不能再命中上一条会话。
  history.pushState(state, "", `${location.pathname}${location.search}${hash.size ? `#${hash}` : ""}`);
}
export function welcomeNode() {
  const node = element("div", "welcome");
  // 用户要求说明所选 Claude 实例的职责，不引用默认 WSL 名称，避免与当前选择的实例不一致。
  node.append(element("h1", "", isClaude() ? "开始 Claude 会话" : "继续本机的工作"),
    element("p", "", isClaude() ? "选择项目，输入消息开始工作。" : "选择左侧会话，发送会话。"),
    element("small", "", isClaude()
      ? "消息、工具和上下文由所选 Claude 实例处理。"
      : "消息、审批和执行状态与 Codex 桌面共享。"));
  return node;
}
export function showWelcome({ startNew = true, clearHash = false } = {}) {
  ++relatedSelectionVersion;
  finishRelatedSessionExit();
  setRelatedSessionView(null); relatedReturnTranscript = null; sessionMenu?.close(); syncRelatedSessionLayout();
  setPendingSteer(null);
  if (session && !session.readOnly && startNew) {
    suppressAutomaticNewChat = false;
    void startNewChat("", { focus: false, closeSidebar: false });
    return;
  }
  cancelMessageEdit({ focus: false });
  if (!clearHash) { rememberDraft(); rememberTranscript({ retainView: true }); }
  resetMessages();
  setEditingQueuedMessage(null);
  setSelectionVersion(selectionVersion + 1); ++threadReadVersion; setSelectionToken(randomId());
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
