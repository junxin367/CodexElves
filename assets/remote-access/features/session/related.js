// 关联会话：侧边会话与子代理会话的打开、返回、过期、创建回执及临时历史导航。

import { isAndroidContainer } from "../../platform.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import {
  connected, creating, pageLeaving, selectedId, selectionToken, selectionVersion, session,
  setThread, sidebar, thread, turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { pendingFeedbackReady, updateComposer } from "../composer/composer.js";
import { currentEffort, currentModelId } from "../composer/models.js";
import { permissionWrites } from "../composer/permissions.js";
import { renderMessages } from "../conversation/history-view.js";
import {
  reloadThread, setHistoryError, setHistoryInitialized, setTranscriptChecking
} from "../conversation/history.js";
import { mutate } from "../conversation/pending.js";
import {
  relatedSelectionVersion, selectThread, setRelatedReturnTranscript, setRelatedSelectionVersion,
  setSuppressAutomaticNewChat, showWelcome, syncThreadHash
} from "../conversation/selection.js";
import { isSubagentItem } from "../messages/subagents.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import {
  normalizeRelatedView, privateInputScope, sideThreadsEnabled, subagentsEnabled, threadReadOnly,
  validRelatedThreadId
} from "./capabilities.js";
import { retryConnectionRecovery } from "./lifecycle.js";
import { workspaceSidePanel } from "./side-panel.js";
import {
  discardPersistentThreadDisplay, readScope, syncRelatedModelDisplay, threadDisplayKey,
  threadDisplays
} from "./thread-cache.js";
import { sessionMenu, sessionTools, setSessionMenu, syncRelatedSidebarLayout } from "../shell.js";
import { filterSideThreadSidebar } from "../sidebar/actions.js";
import { markSidebarThreadReadLocally, renderSidebar, sidebarReadRefreshSuppressions } from "../sidebar/list.js";

export let relatedSessionView = null;
export function setRelatedSessionView(value) { return relatedSessionView = value; }
export let relatedBackPending = false;
export function setRelatedBackPending(value) { return relatedBackPending = value; }
export let relatedExitTransition = null;
export let sideCreationView = null;
export let relatedMenuTimer = null, relatedOperationTimer = null, relatedSessionCheckTimer = null;
export function setRelatedMenuTimer(value) { return relatedMenuTimer = value; }
let transientHistoryView = null, restoringTransientHistory = null;
export const relatedThreadViews = new Map(), relatedThreadOperations = new Map();
export const relatedHistoryKey = "codexElvesRelatedSession", relatedOperationStorageKey = "codex-elves-related-operations";
export const sideCreationHistoryKey = "codexElvesSideCreation";
export const conversationHistoryKey = "codexElvesConversationPosition";
export const relatedNavigationOwner = randomId();
function rememberRelatedView(id, kind, parentThreadId, name = "", { openedFromThreadId = null, ephemeral } = {}) {
  const known = normalizeRelatedView(relatedThreadViews.get(id), id);
  const value = normalizeRelatedView({ scope: privateInputScope(session), threadId: id,
    parentThreadId: parentThreadId ?? (known?.kind === kind ? known.parentThreadId : null),
    openedFromThreadId, kind, name,
    ephemeral: ephemeral === true || known?.ephemeral === true ? true : ephemeral ?? known?.ephemeral });
  if (value) relatedThreadViews.set(id, value);
  if (value?.ephemeral === true && known?.ephemeral !== true) discardPersistentThreadDisplay(id);
  return value;
}
export function relatedViewForThread(id) {
  return normalizeRelatedView(relatedSessionView, id)
    || normalizeRelatedView(history.state?.[relatedHistoryKey], id)
    || normalizeRelatedView(relatedThreadViews.get(id), id);
}
export function syncRelatedSessionLayout() {
  let view = normalizeRelatedView(relatedSessionView, selectedId);
  if (!view && thread?.id === selectedId && thread.viewKind === "subagent"
      && typeof thread.parentThreadId === "string" && validRelatedThreadId(thread.parentThreadId)
      && thread.parentThreadId !== selectedId) {
    // 直接打开/重连子代理 URL 时，以后端确认的主会话关系恢复只读布局和返回入口，不依赖旧页面内存。
    view = rememberRelatedView(selectedId, "subagent", thread.parentThreadId, thread.agentName || thread.title || "",
      { openedFromThreadId: thread.parentThreadId, ephemeral: thread.ephemeral });
    relatedSessionView = view;
  }
  const workspace = $(".workspace");
  workspace?.classList.toggle("related-session-view", !!view);
  workspace?.classList.toggle("related-session-readonly", view?.kind === "subagent");
  if (!view) workspace?.classList.remove("related-session-prepared");
  if (view) workspace?.classList.remove("sidebar-open");
  for (const id of ["open-sidebar", "connection-state"]) {
    const control = $(`#${id}`);
    if (control) control.hidden = !!view;
  }
  const sessionMenuTrigger = $("#open-session-tools");
  if (sessionMenuTrigger) sessionMenuTrigger.hidden = false;
  const back = $("#related-session-back");
  if (back) { back.hidden = !view; back.disabled = relatedBackPending; }
  syncRelatedModelDisplay();
  const expired = sideThreadExpired();
  const composer = $("#composer");
  if (composer) composer.hidden = view?.kind === "subagent" || expired;
  const expiredBanner = $("#side-thread-expired");
  if (expiredBanner) expiredBanner.hidden = !expired;
  const pendingArea = $("#pending-area");
  if (pendingArea) pendingArea.hidden = view?.kind === "subagent";
  syncSideThreadUnread();
  sessionMenu?.updateContext();
  syncRelatedSidebarLayout?.();
  sessionMenu?.reposition?.();
  scheduleRelatedSessionCheck();
}
export function syncSideThreadUnread() {
  const trigger = $("#open-session-tools");
  if (!trigger) return;
  const unread = !creating && !relatedSessionView && (sidebar?.unreadSideThreads || [])
    .some(item => item.parentThreadId === selectedId);
  trigger.classList.toggle("has-unread-side-thread", unread);
  const label = unread ? "会话菜单，有未读的侧边会话" : "会话菜单";
  trigger.setAttribute("aria-label", label); trigger.title = label;
}
export function finishRelatedSessionExit() {
  const transition = relatedExitTransition;
  if (!transition) return;
  relatedExitTransition = null;
  clearTimeout(transition.timer);
  transition.conversation.removeEventListener("animationend", transition.onAnimation);
  transition.conversation.removeEventListener("animationcancel", transition.onAnimation);
  transition.layer.remove();
  transition.workspace.inert = transition.wasInert;
  relatedBackPending = false;
  syncRelatedSessionLayout();
  transition.resolve();
}
export function prepareRelatedSessionExit() {
  finishRelatedSessionExit();
  const workspace = $(".workspace"), conversation = workspace?.querySelector(".conversation");
  if (!conversation || matchMedia("(prefers-reduced-motion: reduce)").matches) return null;
  if (typeof CSSStyleSheet !== "function" || typeof CSSStyleSheet.prototype.replaceSync !== "function"
      || typeof ShadowRoot !== "function" || !("adoptedStyleSheets" in ShadowRoot.prototype)) return null;
  let style;
  try {
    const sheet = document.querySelector('link[rel="stylesheet"][href$="/style.css"]')?.sheet;
    if (!sheet) return null;
    style = new CSSStyleSheet();
    style.replaceSync([...sheet.cssRules].map(rule => rule.cssText).join("\n"));
  } catch { return null; }
  const layer = element("div", "related-session-exit-layer");
  layer.inert = true; layer.setAttribute("aria-hidden", "true");
  const shadow = layer.attachShadow({ mode: "closed" });
  shadow.adoptedStyleSheets = [style];
  const frame = workspace.cloneNode(false);
  frame.removeAttribute("id"); frame.classList.add("related-session-prepared");
  Object.assign(frame.dataset, document.documentElement.dataset);
  const snapshot = conversation.cloneNode(true);
  snapshot.style.background = "var(--bg)";
  const transform = getComputedStyle(conversation).transform;
  snapshot.style.transform = transform === "none" ? "translate3d(0,0,0)" : transform;
  snapshot.style.setProperty("--related-session-exit-from", snapshot.style.transform);
  const originals = [conversation, ...conversation.querySelectorAll("*")];
  const copies = [snapshot, ...snapshot.querySelectorAll("*")];
  const scrollPositions = [];
  for (const [index, source] of originals.entries()) {
    if (source.scrollTop || source.scrollLeft)
      scrollPositions.push([copies[index], source.scrollTop, source.scrollLeft]);
    if (source instanceof HTMLInputElement) {
      copies[index].value = source.value; copies[index].checked = source.checked;
    } else if (source instanceof HTMLTextAreaElement || source instanceof HTMLSelectElement)
      copies[index].value = source.value;
  }
  frame.append(snapshot); shadow.append(frame); document.body.append(layer);
  for (const [node, top, left] of scrollPositions) { node.scrollTop = top; node.scrollLeft = left; }
  const transition = { workspace, conversation: snapshot, frame, layer, wasInert: workspace.inert, timer: null,
    promise: null, resolve: null, onAnimation: null };
  transition.promise = new Promise(resolve => { transition.resolve = resolve; });
  transition.onAnimation = event => {
    if (event.target === snapshot && event.animationName === "related-session-exit")
      finishRelatedSessionExit();
  };
  relatedExitTransition = transition;
  relatedBackPending = true;
  syncRelatedSessionLayout();
  workspace.inert = true;
  snapshot.addEventListener("animationend", transition.onAnimation);
  snapshot.addEventListener("animationcancel", transition.onAnimation);
  transition.timer = setTimeout(finishRelatedSessionExit, 250);
  return transition;
}
export function animateRelatedSessionExit(transition) {
  if (!transition || relatedExitTransition !== transition) return Promise.resolve();
  clearTimeout(transition.timer);
  transition.frame.classList.add("related-session-exiting");
  transition.timer = setTimeout(finishRelatedSessionExit, 250);
  return transition.promise;
}
export function applyRelatedThreadMetadata(value) {
  if (!value || value.id !== selectedId) return;
  const previous = normalizeRelatedView(relatedSessionView, value.id);
  const inferred = validRelatedThreadId(value.parentThreadId) && value.parentThreadId !== value.id
    && ["subagent", "side"].includes(value.viewKind)
    ? rememberRelatedView(value.id, value.viewKind, value.parentThreadId, value.agentName || value.title || "",
      { ephemeral: value.ephemeral }) : null;
  if (inferred) {
    // 原生直接父会话和本次打开来源不同：主会话菜单也会列出孙级子代理。
    const source = previous?.kind === inferred.kind
      ? previous.openedFromThreadId || (previous.returnToParent ? previous.parentThreadId : null) : null;
    // 打开来源只用于返回，不作为原生类型或父关系的证据；完成后退出运行列表也不改变来源。
    relatedSessionView = { ...rememberRelatedView(value.id, inferred.kind, inferred.parentThreadId, inferred.name,
      { openedFromThreadId: source, ephemeral: value.ephemeral }),
      ...(source && previous.returnToParent
        ? { returnToParent: true } : {}),
      ...(source && previous.owner
        ? { owner: previous.owner } : {}) };
  } else if (relatedSessionView?.threadId === value.id) {
    // 历史路由只是恢复提示，真实会话关系以原生返回为准。
    relatedThreadViews.delete(value.id);
    relatedSessionView = null;
  }
  if (!inferred && value.ephemeral === true) discardPersistentThreadDisplay(value.id);
  syncRelatedSessionLayout();
  syncThreadHash(value.id);
}
function sessionMenuIsOpen() {
  if (relatedSessionView) return false;
  return typeof sessionMenu?.isOpen === "function" ? sessionMenu.isOpen() : !!sessionMenu?.isOpen;
}
export function conversationHistoryPosition(state) {
  const value = state?.[conversationHistoryKey];
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}
export function rememberTransientHistory() {
  transientHistoryView = { scope: privateInputScope(session), threadId: selectedId,
    state: history.state, url: location.href };
}
export function handleTransientHistoryPop(event) {
  if (restoringTransientHistory) {
    const scope = restoringTransientHistory.scope;
    restoringTransientHistory = null;
    if (scope === privateInputScope(session)) event.stopImmediatePropagation();
    return;
  }
  const preview = document.querySelector("#image-preview-dialog[open], #video-preview-dialog[open]");
  // 会话菜单在手机和 PC 均固定展开，不消费浏览器返回；此处仅为媒体预览恢复原历史位置。
  const saved = transientHistoryView;
  if (!preview || !saved || saved.scope !== privateInputScope(session) || saved.threadId !== selectedId) return;
  const from = conversationHistoryPosition(saved.state), to = conversationHistoryPosition(event.state);
  const depth = state => state?.codexElvesTools?.route === "detail" ? 2 : state?.codexElvesTools ? 1 : 0;
  const direction = from !== to ? from > to ? 1 : -1
    : depth(saved.state) !== depth(event.state) ? depth(saved.state) > depth(event.state) ? 1 : -1 : 0;
  if (!direction) return;
  event.stopImmediatePropagation();
  preview.close();
  // popstate 本身不可取消；恢复原历史位置，让下一次返回再离开当前页面。
  restoringTransientHistory = { scope: saved.scope };
  history.go(direction);
}
export function stopRelatedSessionTimers() {
  clearTimeout(relatedMenuTimer); relatedMenuTimer = null;
  clearTimeout(relatedOperationTimer); relatedOperationTimer = null;
  clearTimeout(relatedSessionCheckTimer); relatedSessionCheckTimer = null;
}
export function scheduleRelatedSessionCheck() {
  clearTimeout(relatedSessionCheckTimer);
  relatedSessionCheckTimer = null;
  const view = normalizeRelatedView(relatedSessionView, selectedId);
  if (view
      && (view.kind === "subagent" ? subagentsEnabled() : sideThreadsEnabled())
      && connected && !document.hidden) {
    const id = selectedId, scope = privateInputScope(session), version = selectionVersion;
    // 用户要求 Codex 仅保留服务模式；关联会话统一每 2 秒只读补齐，隐藏或离页时停止，不恢复执行。
    relatedSessionCheckTimer = setTimeout(async () => {
      relatedSessionCheckTimer = null;
      if (scope !== privateInputScope(session) || selectedId !== id || version !== selectionVersion
          || document.hidden || !connected || relatedSessionView?.kind !== view.kind) return;
      try { await reloadThread(); } catch {}
      if (scope === privateInputScope(session) && selectedId === id && version === selectionVersion)
        scheduleRelatedSessionCheck();
    }, 2000);
    return;
  }
}

export function isMissingThreadError(error, id) {
  const code = Number(error?.code);
  if (!validRelatedThreadId(id) || [401, 403].includes(code) || code >= 500 && code !== 503
      || typeof error?.readThreadId === "string" && error.readThreadId !== id
      || typeof error?.readThreadScope === "string" && error.readThreadScope !== privateInputScope(session)) return false;
  const message = typeof error?.message === "string" ? error.message : "";
  const missing = [...message.matchAll(/(thread not loaded:|unknown thread:|no rollout found for thread id )\s*["']?([A-Za-z0-9_-]+)/gi)];
  if (missing.length && !missing.some(match => match[2] === id)) return false;
  if (error?.readThreadId !== id && !missing.some(match => match[2] === id)) return false;
  const side = normalizeRelatedView(relatedSessionView, id)?.kind === "side";
  // 原生 -32600 目前经服务层映射为 HTTP 503；仅准确侧边 ID 的缺失原因可作为过期证据。
  if (code === 503) return side && missing.some(match => match[2] === id);
  if (code === 410) return true;
  if (error?.reason === "side_thread_expired") return side;
  if (missing.some(match => !match[1].toLowerCase().startsWith("thread not loaded:"))) return true;
  return side && (missing.some(match => match[2] === id)
    || code === 404 && message.includes("会话已关闭或不存在"));
}
export function sideThreadExpired(id = selectedId) {
  const view = normalizeRelatedView(relatedSessionView, id)
    || normalizeRelatedView(relatedThreadViews.get(id), id);
  return view?.kind === "side" && (view.expired === true || thread?.id === id && thread.sideThreadExpired === true);
}
async function retireExpiredSideThread(id) {
  const current = normalizeRelatedView(relatedSessionView, id);
  if (id !== selectedId || current?.kind !== "side") return false;
  const expired = { ...current, expired: true };
  relatedSessionView = expired;
  relatedThreadViews.set(id, expired);
  sessionMenu?.markSideThreadRead(id);
  sessionMenu?.removeThread(id);
  if (sidebar) {
    markSidebarThreadReadLocally(id);
    sidebar.unreadSideThreads = (sidebar.unreadSideThreads || []).filter(item => item.id !== id);
  }
  if (thread?.id === id) {
    setThread({ ...thread, readOnly: true, sideThreadExpired: true, sideThreadClosed: false });
  } else {
    setThread({
      id, title: current.name || "新建侧边会话", viewKind: "side", readOnly: true,
      sideThreadExpired: true, parentThreadId: current.parentThreadId || current.openedFromThreadId
    });
  }
  setHistoryInitialized(true);
  setTranscriptChecking(false);
  // 读取失败且本页没有保留正文时，不能将“未读到历史”显示成“此会话暂无消息”。
  setHistoryError(turns.size ? "" : "此侧边会话的历史暂时不可读取，请刷新重试。");
  renderMessages();
  syncRelatedSessionLayout();
  renderSidebar();
  return true;
}
export async function clearMissingThreadSelection(id) {
  if (id !== selectedId || !session) return false;
  if (normalizeRelatedView(relatedSessionView, id)?.kind === "side")
    return retireExpiredSideThread(id);
  if (normalizeRelatedView(relatedSessionView, id)?.kind === "subagent") {
    // 只读子智能体已释放时回到原主会话，不能为查看历史重新运行或遗留可编辑输入框。
    await returnToParentSession();
    notice("子智能体已不可读取，已返回主会话", true);
    return true;
  }
  setSuppressAutomaticNewChat(true);
  resetRelatedSessions();
  showWelcome({ startNew: false, clearHash: true });
  transcriptCache.remove(session.device.id, id);
  threadDisplays.delete(threadDisplayKey(id));
  discardPersistentThreadDisplay(id);
  notice("当前会话已不存在，已返回会话列表，请重新选择会话。", true);
  retryConnectionRecovery();
  return true;
}
export function resetRelatedSessions({ persisted = false } = {}) {
  dismissSideCreation({ canceled: true });
  setRelatedSelectionVersion(relatedSelectionVersion + 1);
  finishRelatedSessionExit();
  stopRelatedSessionTimers();
  sessionMenu?.dispose(); setSessionMenu(null);
  relatedSessionView = null; setRelatedReturnTranscript(null); relatedBackPending = false;
  transientHistoryView = restoringTransientHistory = null;
  relatedThreadViews.clear(); relatedThreadOperations.clear();
  if (persisted) try { sessionStorage.removeItem(relatedOperationStorageKey); } catch {}
}
export async function loadRelatedThreads(parentThreadId) {
  if (!subagentsEnabled() && !sideThreadsEnabled()) return {
    ok: true, threadId: parentThreadId, subagents: [], sideThreads: [],
    completedSubagents: [], completedSubagentCount: 0, subagentsComplete: true, canCreateSideThread: false
  };
  if (!session || !validRelatedThreadId(parentThreadId))
    throw new Error("请先选择主会话");
  const scope = privateInputScope(session);
  const result = await api("/api/read/relatedThreads", { threadId: parentThreadId });
  if (scope !== privateInputScope(session)) throw new Error("当前授权已变化");
  if (result?.threadId !== parentThreadId || !Array.isArray(result.subagents) || !Array.isArray(result.sideThreads))
    throw new Error("关联会话返回内容无效");
  return {
    ...result,
    sideThreads: result.sideThreads.map(item => sidebarReadRefreshSuppressions.has(item.id)
      ? { ...item, hasUnreadTurn: false } : item)
  };
}
export function adoptRelatedMenu(result) {
  const parentThreadId = result.threadId;
  if (selectedId !== parentThreadId || relatedSessionView) return;
  const openSideIds = new Set(result.sideThreads.map(item => item.id));
  const closedIds = [...relatedThreadViews.values()].filter(view => view.kind === "side"
    && (view.parentThreadId || view.openedFromThreadId) === parentThreadId
    && !openSideIds.has(view.threadId)).map(view => view.threadId);
  if (closedIds.length) void retireNativeSideThreads(closedIds, { notify: false });
  for (const item of [...result.subagents, ...(result.completedSubagents || [])])
    rememberRelatedView(item.id, "subagent", item.parentThreadId ?? null, item.name,
      { openedFromThreadId: parentThreadId, ephemeral: item.ephemeral });
  for (const item of result.sideThreads) rememberRelatedView(item.id, "side", null, item.title,
    { openedFromThreadId: parentThreadId });
  filterSideThreadSidebar(sidebar);
  renderSidebar();
}
export async function retireNativeSideThreads(ids, { notify = true, reason = "closed" } = {}) {
  const closed = new Set(ids.filter(validRelatedThreadId));
  const current = normalizeRelatedView(relatedSessionView, selectedId);
  for (const id of closed) {
    sessionMenu?.removeThread(id);
    relatedThreadViews.delete(id);
    transcriptCache.remove(session?.device.id, id);
    threadDisplays.delete(threadDisplayKey(id));
    discardPersistentThreadDisplay(id);
    if (readScope) void readCache.remove(readScope, "transcript", id).catch(() => {});
  }
  if (sidebar?.unreadSideThreads)
    sidebar.unreadSideThreads = sidebar.unreadSideThreads.filter(item => !closed.has(item.id));
  syncSideThreadUnread();
  if (current?.kind === "side" && closed.has(current.threadId)) {
    if (thread?.id === current.threadId) setThread({ ...thread, readOnly: true, sideThreadClosed: true });
    setHistoryInitialized(false);
    updateComposer();
    if (notify) notice(reason === "expired" ? "侧边会话已过期，已返回主会话" : "此侧边会话已在 Codex 关闭");
    await returnToParentSession();
  }
}
export function scheduleRelatedMenuRefresh(event, { suppressReadRefresh = false } = {}) {
  const params = event.params || {}, id = params.threadId ?? params.conversationId ?? params.thread?.id;
  if (!["thread/started", "thread/deleted", "turn/started", "turn/completed",
    "thread/status/changed", "thread/name/updated", "remote/sidebarChanged", "remote/relatedThreadsChanged",
    "remote/resync", "remote/reconnected",
    "item/started", "item/completed"].includes(event.method)
    || event.method.startsWith("item/") && !isSubagentItem(params.item || {})) return;
  if (suppressReadRefresh && event.method === "remote/sidebarChanged") return;
  const relevant = sessionMenu?.invalidate(event.method === "thread/deleted" ? null : id, params.parentThreadId);
  if (!relevant || relatedSessionView || !selectedId || !connected || relatedMenuTimer) return;
  const scope = privateInputScope(session), parent = selectedId;
  relatedMenuTimer = setTimeout(() => {
    relatedMenuTimer = null;
    if (scope === privateInputScope(session) && selectedId === parent && !relatedSessionView)
      void Promise.resolve(sessionMenu.refresh({ background: true })).catch(() => {});
  }, 450);
}
export async function openRelatedThread(item, kind, parentThreadId = selectedId, {
  owner = relatedNavigationOwner, initialThread = null, replaceHistory = false,
  preparedView = false, onViewReady = null, revealSidePanel = true
} = {}) {
  if (kind === "subagent" ? !subagentsEnabled() : !sideThreadsEnabled()) return;
  if (relatedSessionView || !validRelatedThreadId(item?.id) || parentThreadId !== selectedId) return;
  if (kind === "side" && sessionTools?.openSideThread?.(item, { reveal: revealSidePanel })) {
    onViewReady?.();
    return;
  }
  const view = rememberRelatedView(item.id, kind, null, item.name || item.title || "",
    { openedFromThreadId: parentThreadId, ephemeral: item.ephemeral });
  if (!view) return;
  if (kind === "side") {
    if (sessionMenu?.isSuspended()) sessionMenu.resume();
  } else sessionMenu?.close();
  closePopup(); closeComposerMenus();
  await selectThread(item.id, { closeSidebar: false, relatedView: { ...view, owner, returnToParent: true },
    initialThread, replaceHistory, preparedView, onViewReady });
  if (kind === "side" && selectedId === item.id)
    await markVisibleThreadRead(item.id, { opened: true });
}
export async function returnToParentSession() {
  const view = normalizeRelatedView(relatedSessionView, selectedId);
  if (!view || relatedBackPending) return;
  const parentThreadId = view.openedFromThreadId || view.parentThreadId;
  if (!validRelatedThreadId(parentThreadId)) return;
  const historyView = normalizeRelatedView(history.state?.[relatedHistoryKey], selectedId);
  // 重载后的 owner 已失效，旧历史上一项不一定是父会话；只有本页创建的路由才可直接后退。
  if (historyView?.owner === relatedNavigationOwner && view.owner === relatedNavigationOwner
      && historyView.returnToParent && view.returnToParent) {
    relatedBackPending = true; syncRelatedSessionLayout();
    history.back();
    return;
  }
  await selectThread(parentThreadId,
    { closeSidebar: false, relatedView: null, replaceHistory: true });
}
export function createDesktopSideThread() {
  if (workspaceSidePanel || isAndroidContainer() || !matchMedia("(min-width:761px)").matches
      || !session || creating || relatedSessionView || !connected || !sideThreadsEnabled()
      || thread?.id !== selectedId || threadReadOnly() || document.hidden
      || document.querySelector("dialog[open]") || sessionMenu?.canCreateSideThread?.() === false) return false;
  void runRelatedOperation("createSideThread").catch(error => notice(error.message, true));
  return true;
}
export function handleSideThreadShortcut(event) {
  // 用户要求沿用 Codex 新建侧边快捷键：Windows/Linux Ctrl+Alt+S，macOS Cmd+Option+S。
  // 用物理 KeyS 兼容 Option 产生特殊字符；输入法、AltGr 和长按重复不触发创建。
  if (event.defaultPrevented || event.isComposing || event.repeat || event.shiftKey || !event.altKey
      || !!event.ctrlKey === !!event.metaKey || event.getModifierState?.("AltGraph")
      || event.code !== "KeyS" && event.key?.toLowerCase() !== "s"
      || document.querySelector("dialog[open]")) return false;
  if (workspaceSidePanel) {
    if (window.parent === window || document.hidden
        || document.documentElement.dataset.workspaceSidePanelVisible === "false") return false;
    event.preventDefault();
    // iframe 的键盘事件不会冒泡到主页面；父页面另核对同源、来源窗口及可见标签，再走原创建回执。
    window.parent.postMessage({ type: "codex-elves-create-side-thread" }, location.origin);
    return true;
  }
  if (!createDesktopSideThread()) return false;
  event.preventDefault();
  return true;
}
export function showSideCreation(record, { recordHistory = true } = {}) {
  if (sideCreationView?.record === record) return;
  const workspace = $(".workspace");
  if (!workspace || sideCreationView) return;
  const dock = sessionTools?.openSideCreation?.({
    requestId: record.id,
    onClose: () => {
      if (sideCreationView?.record === record) dismissSideCreation({ canceled: true });
    }
  });
  if (dock) {
    sideCreationView = { record, dock };
    return;
  }
  const panel = element("section", "session-tools-screen side-creation-screen");
  panel.setAttribute("aria-label", "新建侧边会话");
  const header = element("header", "session-tools-heading");
  const back = button("返回", "返回主会话", requestSideCreationBack, "text-button");
  header.append(element("h1", "", "新建侧边会话"), back);
  const status = element("div", "side-creation-status");
  status.setAttribute("role", "status");
  status.append(element("span", "spinner"), element("p", "", "正在创建侧边会话…"));
  panel.append(header, status);
  sideCreationView = { record, workspace, panel, status, wasInert: workspace.inert };
  workspace.inert = true;
  document.body.append(panel);
  panel.getBoundingClientRect();
  panel.classList.add("session-tools-visible");
  if (recordHistory) history.pushState({ ...(history.state || {}), [sideCreationHistoryKey]: {
      owner: relatedNavigationOwner, requestId: record.id
    } }, "", location.href);
  back.focus({ preventScroll: true });
}
export function updateSideCreationStatus(record) {
  if (sideCreationView?.record !== record) return;
  const unknown = record.state === "unknown" && pendingFeedbackReady(record);
  if (sideCreationView.dock) { sideCreationView.dock.update(unknown); return; }
  sideCreationView.status.querySelector("p").textContent = unknown
    ? "正在确认创建结果，可返回主会话查看。" : "正在创建侧边会话…";
  sideCreationView.status.querySelector(".spinner").hidden = unknown;
}
export function dismissSideCreation({ canceled = false, animate = false } = {}) {
  const view = sideCreationView;
  if (!view) return;
  sideCreationView = null;
  if (canceled) {
    view.record.autoOpen = false;
    if (ownsRelatedOperation(view.record)) persistRelatedOperations();
  }
  if (view.dock) { view.dock.close(); return; }
  view.workspace.inert = view.wasInert;
  if (!animate || matchMedia("(prefers-reduced-motion: reduce)").matches) { view.panel.remove(); return; }
  view.panel.inert = true;
  view.panel.classList.remove("session-tools-visible");
  const finish = () => { clearTimeout(timer); view.panel.remove(); };
  const timer = setTimeout(finish, 250);
  view.panel.addEventListener("transitionend", event => {
    if (event.target === view.panel && event.propertyName === "transform") finish();
  });
}
export function requestSideCreationBack() {
  const view = sideCreationView;
  if (!view) return;
  const owned = history.state?.[sideCreationHistoryKey]?.owner === relatedNavigationOwner
    && history.state[sideCreationHistoryKey].requestId === view.record.id;
  dismissSideCreation({ canceled: true, animate: true });
  if (owned) history.back();
}
function persistRelatedOperations(required = false) {
  try {
    const records = [...relatedThreadOperations.values()].map(record => ({
      id: record.id, action: record.action, scope: record.scope, parentThreadId: record.parentThreadId,
      threadId: record.threadId, state: record.state, selectionToken: record.selectionToken, autoOpen: record.autoOpen,
      time: record.time
    }));
    if (records.length) sessionStorage.setItem(relatedOperationStorageKey, JSON.stringify(records));
    else sessionStorage.removeItem(relatedOperationStorageKey);
  } catch {
    if (required) throw new Error("无法保存侧边会话操作回执，操作未提交。请允许浏览器会话存储。");
  }
}
function relatedOperationKey(record) {
  return JSON.stringify([record.scope, record.action, record.parentThreadId, record.threadId || null]);
}
export function ownsRelatedOperation(record) {
  return record.scope === privateInputScope(session)
    && relatedThreadOperations.get(relatedOperationKey(record)) === record;
}
export function restoreRelatedOperations() {
  const scope = privateInputScope(session);
  try {
    const records = JSON.parse(sessionStorage.getItem(relatedOperationStorageKey) || "[]");
    if (Array.isArray(records)) for (const record of records) {
      if (record?.scope !== scope || !/^[A-Za-z0-9-]{1,100}$/.test(record.id)
          || !validRelatedThreadId(record.parentThreadId)
          || !["createSideThread", "closeSideThread"].includes(record.action)
          || record.action === "closeSideThread" && (!validRelatedThreadId(record.threadId)
            || record.threadId === record.parentThreadId)) continue;
      relatedThreadOperations.set(relatedOperationKey(record), { ...record,
        state: record.state === "pending" ? "pending" : "unknown", selectionToken: null, autoOpen: false });
    }
  } catch {}
  scheduleRelatedOperationCheck();
}
export function scheduleRelatedOperationCheck() {
  clearTimeout(relatedOperationTimer); relatedOperationTimer = null;
  if (!session || !connected || pageLeaving || document.hidden) return;
  const records = [...relatedThreadOperations.values()].filter(record =>
    ownsRelatedOperation(record) && record.state === "pending" && !record.request);
  if (!records.length) return;
  relatedOperationTimer = setTimeout(() => {
    relatedOperationTimer = null;
    for (const record of records) if (ownsRelatedOperation(record))
      void resolveRelatedOperation(record, { allowSelection: record.autoOpen === true }).catch(() => {});
  }, 2000);
}
async function finishRelatedOperation(record, result, { allowSelection = false } = {}) {
  if (!ownsRelatedOperation(record)) return;
  if (result?.ok !== true || result.parentThreadId !== record.parentThreadId
      || !validRelatedThreadId(result.threadId) || result.threadId === record.parentThreadId
      || record.action === "closeSideThread" && (result.threadId !== record.threadId || result.closed !== true))
    throw Object.assign(new Error("侧边会话回执内容无效，请在本机核对。不会重复执行"), { outcomeUnknown: true });
  relatedThreadOperations.delete(relatedOperationKey(record)); persistRelatedOperations();
  if (record.action === "createSideThread") {
    sessionMenu?.updateSideThread(record.parentThreadId, { id: result.threadId,
      title: result.thread?.title || "新建侧边会话", hasUnreadTurn: false });
    rememberRelatedView(result.threadId, "side", record.parentThreadId);
    filterSideThreadSidebar(sidebar);
    renderSidebar();
    if (allowSelection && record.autoOpen !== false && !relatedSessionView && selectedId === record.parentThreadId
        && record.selectionToken === selectionToken) {
      const prepared = sideCreationView?.record === record;
      if (prepared && history.state?.[sideCreationHistoryKey]?.requestId === record.id) {
        const state = { ...history.state }; delete state[sideCreationHistoryKey];
        history.replaceState(state, "", location.href);
      }
      await openRelatedThread({ id: result.threadId, title: result.thread?.title || "新建侧边会话" }, "side",
        record.parentThreadId, { initialThread: record.state === "submitting"
            && result.synchronization?.status !== "failed" ? result.thread : null,
          replaceHistory: prepared, preparedView: prepared,
          // 用户隐藏等待标签后，创建成功只更新保留的标签，不自动把已隐藏面板重新展开。
          revealSidePanel: sideCreationView?.dock?.isVisible?.() ?? true,
          onViewReady: prepared ? () => dismissSideCreation() : null });
    } else {
      if (sideCreationView?.record === record) requestSideCreationBack();
    }
    if (record.scope === privateInputScope(session) && typeof result.warning === "string" && result.warning.trim())
      notice(`侧边会话已创建。${result.warning.trim()}`, true);
  } else {
    sessionMenu?.updateSideThread(record.parentThreadId, { id: result.threadId }, { remove: true });
    await retireNativeSideThreads([result.threadId], { notify: false });
  }
  updateComposer();
  if (sessionMenuIsOpen()) void Promise.resolve(sessionMenu.refresh()).catch(() => {});
}
async function resolveRelatedOperation(record, { allowSelection = false } = {}) {
  if (!ownsRelatedOperation(record) || !connected) return;
  if (record.request) return record.request;
  record.request = (async () => {
    try {
      const receipt = await api(`/api/receipts/${record.id}`);
      if (!ownsRelatedOperation(record)) return;
      if (receipt.state === "completed") await finishRelatedOperation(record, receipt.result, { allowSelection });
      else if (receipt.state === "failed" && receipt.result?.outcomeUnknown !== true) {
        relatedThreadOperations.delete(relatedOperationKey(record)); persistRelatedOperations();
        if (sideCreationView?.record === record) requestSideCreationBack();
        notice(receipt.result?.message || "侧边会话操作未完成", true);
      } else {
        record.state = receipt.state === "pending" ? "pending" : "unknown"; persistRelatedOperations();
        updateSideCreationStatus(record);
      }
    } catch (error) {
      if (!ownsRelatedOperation(record)) return;
      record.state = "unknown"; persistRelatedOperations();
      updateSideCreationStatus(record);
      notice(`侧边会话操作待核对：${error.message}。原请求标识已保留，不会重复执行`, true);
    } finally {
      record.request = null;
      if (record.scope === privateInputScope(session)) { scheduleRelatedOperationCheck(); updateComposer(); }
    }
  })();
  return record.request;
}
export async function runRelatedOperation(action, item = null) {
  if (!sideThreadsEnabled()) return;
  const parentThreadId = selectedId, scope = privateInputScope(session);
  if (!session || relatedSessionView || !validRelatedThreadId(parentThreadId) || thread?.id !== parentThreadId
      || threadReadOnly(parentThreadId) || !connected) return;
  if (action === "closeSideThread") {
    const view = normalizeRelatedView(relatedThreadViews.get(item?.id), item?.id);
    if ((view?.openedFromThreadId || view?.parentThreadId) !== parentThreadId || view.kind !== "side") return;
    if (!await confirmAction("关闭侧边会话", "关闭后，此临时侧边会话及其内容将无法恢复。确定关闭吗？", "关闭会话", true)
        || selectedId !== parentThreadId || scope !== privateInputScope(session) || relatedSessionView) return;
  }
  const record = { id: randomId(), action, scope, parentThreadId,
    ...(item ? { threadId: item.id } : {}), state: "submitting", selectionToken, time: Date.now(),
    autoOpen: action === "createSideThread" };
  const key = relatedOperationKey(record), existing = relatedThreadOperations.get(key);
  if (existing) {
    if (action === "createSideThread") {
      existing.autoOpen = true; existing.selectionToken = selectionToken;
      showSideCreation(existing); updateSideCreationStatus(existing);
    }
    if (existing.state === "submitting") return;
    await resolveRelatedOperation(existing, { allowSelection: true });
    return;
  }
  let creationSettings = {};
  if (action === "createSideThread") {
    // 与 App 等待主会话设置保存的行为一致；权限失败或等待期间切换会话/授权时不能继承旧值创建。
    const writing = permissionWrites.get(parentThreadId);
    if (writing?.scope === scope && !await writing.completion) return;
    if (selectedId !== parentThreadId || thread?.id !== parentThreadId || relatedSessionView
        || record.selectionToken !== selectionToken || scope !== privateInputScope(session)
        || !connected || threadReadOnly(parentThreadId) || relatedThreadOperations.has(key)) return;
    // 创建时冻结当前模型和深度，包括下拉菜单已选但尚未发送的值；不写回主会话或新建会话默认值。
    const model = currentModelId();
    if (model) creationSettings = { model, reasoningEffort: currentEffort() || null };
  }
  relatedThreadOperations.set(key, record);
  try { persistRelatedOperations(true); }
  catch (error) { relatedThreadOperations.delete(key); throw error; }
  updateComposer();
  if (action === "createSideThread") showSideCreation(record);
  try {
    const payload = action === "createSideThread" ? { threadId: parentThreadId, ...creationSettings }
      : { threadId: item.id, parentThreadId };
    const result = await mutate(action, payload, { receiptId: record.id });
    await finishRelatedOperation(record, result, { allowSelection: true });
  } catch (error) {
    if (!ownsRelatedOperation(record)) return;
    if (error.notSubmitted || error.operationState === "failed" && !error.outcomeUnknown) {
      relatedThreadOperations.delete(key);
      if (sideCreationView?.record === record) requestSideCreationBack();
    } else record.state = error.operationState === "pending" ? "pending" : "unknown";
    updateSideCreationStatus(record);
    persistRelatedOperations(); scheduleRelatedOperationCheck();
    throw error;
  } finally { if (scope === privateInputScope(session)) updateComposer(); }
}
export function renderRelatedOperationStatus(root) {
  if (relatedSessionView) return;
  for (const record of relatedThreadOperations.values()) {
    if (!ownsRelatedOperation(record) || record.parentThreadId !== selectedId
        || sideCreationView?.record === record || !pendingFeedbackReady(record)) continue;
    const box = element("div", "pending-banner", record.action === "createSideThread"
      ? "正在确认侧边会话创建结果。" : "正在确认侧边会话关闭结果。");
    const check = button("查回执", "查询原操作回执，不会重复创建或关闭",
      () => void resolveRelatedOperation(record, { allowSelection: true }), "text-button");
    check.disabled = !connected || !!record.request || record.state === "submitting";
    box.append(check); root.append(box);
  }
}
