// 侧栏列表：分页读取、最近/项目分组渲染、未读角标与桌面可见性观测。

import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { activePopup, closePopup, fitPopup, openPopup } from "../../ui/popup.js";
import { setSelectOptions } from "../../ui/select.js";
import {
  connected, creating, nativeConnectionVersion, pageLeaving, selectedId, session, setSidebar,
  sidebar, thread
} from "../../core/state.js";
import { api, promoteReadPriority } from "../api.js";
import { appearance } from "../appearance/theme.js";
import { changeCreationProject, newChatProjectOptions } from "../composer/new-chat.js";
import { selectThread, startNewChat } from "../conversation/selection.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { claudeThreadId, isActiveThread, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { enabledInstanceIds } from "../session/instances.js";
import { foregroundReadGeneration } from "../session/lifecycle.js";
import { initialAuthenticationRequired } from "../session/login.js";
import { relatedSessionView, syncSideThreadUnread } from "../session/related.js";
import { workspaceSidePanel } from "../session/side-panel.js";
import { cacheDisplayData, sidebarSnapshot } from "../session/thread-cache.js";
import {
  bindSidebarRowMenu, changeSidebar, checkThreadDeletion, deleteSidebarThread,
  filterDeletedSidebar, filterSideThreadSidebar, independentThreadWindowAvailable,
  openIndependentThreadWindow, renameSidebarThread, restoreSidebarPreferences, rowMenu,
  sidebarFilters, threadDeletionChecks, threadDeletionConflict, threadDeletionKey, threadDeletions
} from "./actions.js";
import { renderSessionFilter, sessionFilterValue, sessionTypeVisible } from "./filters.js";

export let collapsed = new Set(), search = "";
export function setSearch(value) { return search = value; }
export let sidebarVersion = 0;
export function setSidebarVersion(value) { return sidebarVersion = value; }
let desktopObservationTimer = null, desktopObservationRequest = null, desktopObservationLast = null;
export const projectVisibleThreadCounts = new Map();
export const collapsedSidebarSections = new Set();
export let recentLimit = 40;
export function setRecentLimit(value) { return recentLimit = value; }
export let recentPriorityIds = null;
export function setRecentPriorityIds(value) { return recentPriorityIds = value; }
// Opening an older thread can refresh its runtime state without starting work.
export const recentPrioritySelectionSuppressions = new Set();
// Marking a thread read must not turn a selection-only refresh into a list reorder.
export const sidebarReadRefreshSuppressions = new Set();
export let recentPriorityKnownIds = new Set(), recentPriorityOpenedAt = 0;
let sidebarFilterDeviceId = null;
export let sidebarRequest = null;
export function setSidebarRequest(value) { return sidebarRequest = value; }
function mergeIncompleteSidebar(previous, current) {
  if (current.complete !== false || current.nativeBridge !== "harness"
      || previous?.nativeBridge !== "harness") return current;
  // 服务首批目录不完整时继续展示同一后端的已知会话；新数据优先，
  // 完整目录返回后整体核对，不能把部分结果当作删除证据；置顶目录独立判断完整性。
  const freshIds = new Set([...current.recent, ...current.pinnedThreads,
    ...current.projects.flatMap(project => project.threads)].map(item => item.id));
  const oldProjects = new Map(previous.projects.map(project => [project.id, project]));
  const projectIds = new Set(current.projects.map(project => project.id));
  // 用户要求服务模式与桌面置顶一致：原生置顶已完整时立即接受取消，不能被未完成的最近分页补回。
  const completePins = current.pinnedThreadsComplete === true;
  const pinnedIds = new Set(current.pinnedThreads.map(item => item.id));
  const mergeRows = (old, fresh) => [...fresh, ...old.filter(item => !freshIds.has(item.id) && !claudeThreadId(item.id))]
    .map(item => item.projectId && !projectIds.has(item.projectId) ? { ...item, projectId: null } : item)
    .map(item => completePins ? { ...item, pinned: pinnedIds.has(item.id) } : item);
  const oldRecent = completePins
    ? [...new Map([...previous.pinnedThreads, ...previous.recent].map(item => [item.id, item])).values()]
    : previous.recent;
  return { ...current,
    recent: mergeRows(oldRecent, current.recent),
    pinnedThreads: completePins ? current.pinnedThreads : mergeRows(previous.pinnedThreads, current.pinnedThreads),
    // 项目目录始终完整，只有会话分页未完成；不能把旧服务合成的工作目录补回项目列表。
    projects: current.projects.map(project => ({ ...project,
      threads: mergeRows(oldProjects.get(project.id)?.threads || [], project.threads) }))
  };
}
let clientUnreadBadgeTimer = null, clientUnreadBadgeRequest = null;
function clientUnreadBadgeAvailable() {
  return !workspaceSidePanel && typeof window.__CODEX_ELVES_UNREAD_BADGE__?.setCount === "function";
}
function clientUnreadThreadCount(value) {
  const unread = new Set(), sideIds = new Set(value?.sideThreadIds || []);
  // 对照本机 Windows Codex：按未读主会话去重，不按消息/轮次累加；
  // 最近、项目、置顶重复出现仍只算 1，侧边子会话/子代理不单独增加任务栏数字。
  for (const item of [...(value?.recent || []), ...(value?.pinnedThreads || []),
    ...(value?.projects || []).flatMap(project => project.threads || [])]) {
    if (!item?.id || item.archived === true || sideIds.has(item.id)
        || item.parentThreadId && item.parentThreadId !== item.id
        || item.threadSource === "subagent" || item.source?.subAgent || item.source?.subagent) continue;
    const hasUnread = typeof item.hasUnreadTurn === "boolean"
      ? item.hasUnreadTurn : item.priority?.attentionState === "unread";
    if (hasUnread) unread.add(item.id);
  }
  return unread.size;
}
export function syncClientUnreadBadge(value = sidebar) {
  if (!clientUnreadBadgeAvailable()) return;
  // 数字来自完整的当前侧栏快照，搜索、分组折叠及会话实例快捷筛选不改变未读总数。
  window.__CODEX_ELVES_UNREAD_BADGE__.setCount(session && !initialAuthenticationRequired
    ? clientUnreadThreadCount(value) : 0);
}
async function refreshClientUnreadBadge() {
  if (!clientUnreadBadgeAvailable() || !session || pageLeaving || !document.hidden
      || initialAuthenticationRequired || navigator.onLine === false) return;
  if (clientUnreadBadgeRequest) {
    clientUnreadBadgeRequest.again = true;
    return clientUnreadBadgeRequest.promise;
  }
  const scope = privateInputScope(session), instances = enabledInstanceIds();
  const request = { controller: new AbortController(), again: false, promise: null };
  const current = () => clientUnreadBadgeRequest === request && !request.controller.signal.aborted
    && document.hidden && !pageLeaving && session && privateInputScope(session) === scope
    && JSON.stringify(enabledInstanceIds()) === JSON.stringify(instances);
  clientUnreadBadgeRequest = request;
  request.promise = (async () => {
    try {
      do {
        request.again = false;
        const value = await api("/api/read/sidebar", { cursor: null, enabledInstanceIds: instances },
          undefined, request.controller.signal, { resourcePriority: "background" });
        if (!current()) return;
        filterDeletedSidebar(value);
        filterSideThreadSidebar(value);
        syncClientUnreadBadge(mergeIncompleteSidebar(sidebar, value));
      } while (request.again && current());
    } catch { /* 后台读取失败保留最后确认的数字，不伪造零值或阻塞聊天。 */ }
    finally { if (clientUnreadBadgeRequest === request) clientUnreadBadgeRequest = null; }
  })();
  return request.promise;
}
export function stopClientUnreadBadgePolling(clear = false) {
  clearInterval(clientUnreadBadgeTimer); clientUnreadBadgeTimer = null;
  clientUnreadBadgeRequest?.controller.abort();
  clientUnreadBadgeRequest = null;
  if (clear && clientUnreadBadgeAvailable()) window.__CODEX_ELVES_UNREAD_BADGE__.setCount(0);
}
export function startClientUnreadBadgePolling() {
  stopClientUnreadBadgePolling();
  if (!clientUnreadBadgeAvailable() || !session || !document.hidden || pageLeaving) return;
  // 仅独立客户端主窗口在后台保留轻量未读刷新；实时事件沿用原 1.2 秒合并，
  // 无事件或断线后以 15 秒只读查询补齐，不恢复正文、模型、用量等前台轮询。
  void refreshClientUnreadBadge();
  clientUnreadBadgeTimer = setInterval(() => void refreshClientUnreadBadge(), 15000);
}
export async function loadSidebar(cursor = null, { fresh = false, resourcePriority = "foreground" } = {}) {
  if (pageLeaving) return;
  if (document.hidden) return refreshClientUnreadBadge();
  const generation = foregroundReadGeneration;
  if (sidebarRequest?.deviceId === session?.device.id && sidebarRequest.generation === generation) {
    promoteReadPriority(sidebarRequest, resourcePriority);
    if (fresh || cursor !== sidebarRequest.cursor) {
      sidebarRequest.again = true;
      sidebarRequest.nextCursor = fresh ? null : cursor;
    }
    return sidebarRequest.promise;
  }
  const version = ++sidebarVersion, deviceId = session?.device.id;
  if (sidebarFilterDeviceId !== deviceId) {
    sidebarFilterDeviceId = deviceId;
    restoreSidebarPreferences();
    recentPriorityIds = null;
    recentPrioritySelectionSuppressions.clear();
    sidebarReadRefreshSuppressions.clear();
    projectVisibleThreadCounts.clear(); collapsedSidebarSections.clear();
  }
  const request = { deviceId, generation, controller: new AbortController(), cursor, nextCursor: cursor, again: false, resourcePriority,
    apiStarted: false, releasePriority: null, promise: null };
  const promise = (async () => {
    do {
      if (generation !== foregroundReadGeneration || request.controller.signal.aborted) return;
      request.again = false; request.cursor = request.nextCursor;
      let result;
      try {
        request.apiStarted = true;
        result = await api("/api/read/sidebar", { cursor: request.cursor, enabledInstanceIds: enabledInstanceIds() }, undefined, request.controller.signal,
          { resourcePriority: request.resourcePriority });
      }
      catch (error) {
        if (generation !== foregroundReadGeneration || request.controller.signal.aborted) return;
        if (request.again && deviceId === session?.device.id) continue;
        throw error;
      }
      if (generation !== foregroundReadGeneration || version !== sidebarVersion || !session || deviceId !== session.device.id) return;
      const applyReadSuppressions = items => (items || []).map(item => {
        if (!sidebarReadRefreshSuppressions.has(item.id)) return item;
        const priority = item.priority?.attentionState === "unread"
          ? { ...item.priority, attentionState: item.priority.active === true ? "active" : "idle" }
          : item.priority;
        return { ...item, hasUnreadTurn: false, ...(priority ? { priority } : {}) };
      });
      const isMainThread = item => item.threadSource !== "subagent" && !item.source?.subAgent && !item.source?.subagent;
      result.recent = applyReadSuppressions(result.recent.filter(isMainThread));
      result.pinnedThreads = applyReadSuppressions(result.pinnedThreads.filter(isMainThread));
      for (const project of result.projects)
        project.threads = applyReadSuppressions(project.threads.filter(isMainThread));
      result.unreadSideThreads = result.unreadSideThreads || [];
      for (const id of sidebarReadRefreshSuppressions) markSidebarThreadReadLocally(id, result);
      filterDeletedSidebar(result, deviceId);
      filterSideThreadSidebar(result);
      result = mergeIncompleteSidebar(sidebar, result);
      filterDeletedSidebar(result, deviceId);
      if (JSON.stringify(sidebar) !== JSON.stringify(result)) {
        setSidebar(result);
        cacheDisplayData("sidebar", "main", sidebarSnapshot(result));
        renderSidebar();
      }
      // 侧栏没有变化时，会话也可能刚完成加载或恢复连接。
      const selected = [...sidebar.recent, ...sidebar.pinnedThreads].find(item => item.id === selectedId);
      if (selected?.hasUnreadTurn && !isActiveThread())
        void markVisibleThreadRead().catch(error => notice(error.message, true));
    } while (request.again && deviceId === session?.device.id && version === sidebarVersion);
  })();
  request.promise = promise; sidebarRequest = request;
  try { await promise; }
  finally {
    request.releasePriority?.();
    if (sidebarRequest?.promise === promise) sidebarRequest = null;
  }
}
// 用户要求服务模式会话按优先级、今天、昨天、前天、近四个星期日历日和历史分组；
// 服务端已统一输出毫秒，
// 这里兼容旧缓存中的 Unix 秒。阈值 1e11 与服务时间戳转换及
// codex_service.rs UNIX_SECONDS_LIMIT 同步。
function sidebarTimestamp(value) {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return value < 100_000_000_000 ? value * 1000 : value;
}
function sidebarUpdatedAt(item) {
  return sidebarTimestamp(item.updatedAt) || sidebarTimestamp(item.recencyAt) || sidebarTimestamp(item.createdAt);
}
function sidebarRecentDateGroups(items, now = Date.now()) {
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const starts = Array.from({ length: 7 }, (_, offset) => {
    const date = new Date(today);
    date.setDate(date.getDate() - offset);
    return date;
  });
  const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
  const labels = ["今天", "昨天", "前天", ...starts.slice(3).map(date => `星期${weekdays[date.getDay()]}`)];
  const groups = labels.map((label, index) => ({
    key: index < 3 ? ["today", "yesterday", "before-yesterday"][index] : `weekday-${index}`,
    label,
    items: []
  }));
  const history = { key: "history", label: "历史", items: [] };
  for (const item of Array.isArray(items) ? items : []) {
    const timestamp = sidebarUpdatedAt(item);
    const index = starts.findIndex((start, offset) => timestamp >= start.getTime()
      && (offset === 0 || timestamp < starts[offset - 1].getTime()));
    (index >= 0 ? groups[index] : history).items.push(item);
  }
  // 用户要求最近会话依次显示优先级、今天、昨天、前天、近四个星期日历日和历史；
  // 空分组由 appendSection 跳过，但固定返回顺序，避免跨周、跨月时改变位置。
  return [...groups, history];
}
export function markSidebarThreadReadLocally(id, value = sidebar) {
  if (!value || typeof id !== "string" || !id) return;
  const parents = new Set((value.unreadSideThreads || [])
    .filter(item => item.id === id).map(item => item.parentThreadId));
  if (Array.isArray(value.unreadSideThreads))
    value.unreadSideThreads = value.unreadSideThreads.filter(item => item.id !== id);
  const update = item => {
    // 用户要求读完侧边后消除所属主会话的蓝点；同一父会话还有其它未读侧边时继续保留。
    if (item && parents.has(item.id) && item.sideThreadAttention)
      item.sideThreadAttention = { ...item.sideThreadAttention,
        unread: value.unreadSideThreads.some(side => side.parentThreadId === item.id) };
    if (!item || item.id !== id) return;
    item.hasUnreadTurn = false;
    if (item.priority?.attentionState === "unread") {
      item.priority = { ...item.priority, attentionState: item.priority.active === true ? "active" : "idle" };
    }
  };
  value.recent.forEach(update);
  value.pinnedThreads.forEach(update);
  value.projects.forEach(project => project.threads.forEach(update));
}
export function sidebarAttentionState(item) {
  const side = item.sideThreadAttention;
  const own = item.priority?.attentionState;
  const status = item.status?.status || item.status;
  const type = typeof status === "string" ? status : status?.type;
  if (side?.waiting || own === "waiting" || !own && (type === "waiting" || (status?.activeFlags || []).some(flag =>
    flag === "waitingOnApproval" || flag === "waitingOnUserInput"))) return "waiting";
  if (side?.unread || own === "unread" || item.hasUnreadTurn) return "unread";
  return side?.active || own === "active" || !own && (type === "active" || type === "inProgress") ? "active" : "idle";
}
function desktopObservationIds() {
  const ids = new Set();
  const add = id => {
    if (typeof id === "string" && /^[a-z0-9_-]{1,120}$/i.test(id) && !claudeThreadId(id)) ids.add(id);
  };
  if (!creating) add(relatedSessionView?.parentThreadId || selectedId);
  const root = $("#sidebar-content"), bounds = root?.getBoundingClientRect();
  if (bounds?.width > 0 && bounds.right > 0 && bounds.left < window.innerWidth) {
    for (const row of root.querySelectorAll(".thread-row")) {
      const rect = row.getBoundingClientRect();
      if (rect.height > 0 && rect.bottom > bounds.top && rect.top < bounds.bottom) add(row.dataset.threadId);
      if (ids.size >= 128) break; // 与 observeThreads 的单批上限保持一致。
    }
  }
  return [...ids].sort();
}
export function resetDesktopObservation() {
  clearTimeout(desktopObservationTimer);
  desktopObservationTimer = null;
  const previous = desktopObservationRequest;
  // 页面或连接代次已变时立即退役旧读取，不等待被取消的网络 promise 才允许重新观察。
  desktopObservationRequest = null;
  desktopObservationLast = null;
  previous?.controller.abort();
}
export function scheduleDesktopObservation() {
  if (pageLeaving || document.hidden || !session?.capabilities?.desktopSessionSync) {
    resetDesktopObservation();
    return;
  }
  clearTimeout(desktopObservationTimer);
  // 用户要求服务模式同步桌面会话：只观察当前会话及列表可见项，滚动合并 150ms；
  // 沿用连接轮询每 15 秒续期，对应后端 45 秒兴趣窗口，不轮询整份消息正文。
  desktopObservationTimer = setTimeout(() => void syncDesktopObservation(), 150);
}
async function syncDesktopObservation() {
  desktopObservationTimer = null;
  if (pageLeaving || document.hidden || !session?.capabilities?.desktopSessionSync) return;
  if (desktopObservationRequest && (desktopObservationRequest.generation !== foregroundReadGeneration
      || desktopObservationRequest.deviceId !== session.device.id
      || desktopObservationRequest.revision !== nativeConnectionVersion)) resetDesktopObservation();
  if (desktopObservationRequest) { desktopObservationRequest.again = true; return; }
  const ids = desktopObservationIds(), deviceId = session.device.id, revision = nativeConnectionVersion;
  const signature = JSON.stringify([deviceId, revision, ids]), now = Date.now();
  // 12 秒内合并重复渲染，确保既有 15 秒轮询仍能在后端 45 秒到期前续上观察。
  if (desktopObservationLast?.signature === signature && now - desktopObservationLast.at < 12000) return;
  const request = { controller: new AbortController(), generation: foregroundReadGeneration, deviceId, revision, again: false };
  desktopObservationRequest = request;
  try {
    await api("/api/read/observeThreads", { threadIds: ids }, undefined, request.controller.signal,
      { resourcePriority: "background" });
    if (!request.controller.signal.aborted && request.generation === foregroundReadGeneration
        && session?.device.id === deviceId && nativeConnectionVersion === revision)
      desktopObservationLast = { signature, at: now };
  } catch {
    // 观察失败由后台重连和下一次连接轮询恢复，不阻塞输入或修改已有发送回执。
  } finally {
    if (desktopObservationRequest === request) {
      desktopObservationRequest = null;
      if (request.again) scheduleDesktopObservation();
    }
  }
}
export function renderSidebar() {
  renderSessionFilter();
  syncSideThreadUnread();
  const root = $("#sidebar-content");
  if (!root || !sidebar) return;
  restoreSidebarPreferences();
  filterDeletedSidebar(sidebar);
  filterSideThreadSidebar(sidebar);
  syncClientUnreadBadge();
  const scrollTop = root.scrollTop;
  const popupState = activePopup && root.contains(activePopup.trigger)
    ? { id: activePopup.trigger.id, index: activePopup.index, focused: activePopup.node.contains(document.activeElement), position: activePopup.position } : null;
  const focusId = root.contains(document.activeElement) ? document.activeElement.id : null;
  const focusedThread = document.activeElement.closest(".thread-row")?.dataset.threadId;
  const focusedMenu = document.activeElement.classList.contains("row-menu-button");
  if (popupState) closePopup();
  root.replaceChildren();
  // 用户要求按会话实例筛选所有分组，并与实例开关取交集；保留原快照和未读状态。
  const filter = sessionFilterValue();
  const typeMatches = item => sessionTypeVisible(item);
  const preferences = {
    mode: sidebarFilters.mode ?? sidebar.preferences.mode,
    chats: sidebarFilters.chats ?? sidebar.preferences.chats,
    projects: sidebarFilters.projects ?? sidebar.preferences.projects,
    pinned: sidebarFilters.pinned ?? sidebar.preferences.pinned
  };
  const viewToggle = $("#sidebar-view-toggle");
  const viewLabel = preferences.mode === "project" ? "切换到最近会话" : "切换到项目视图";
  viewToggle.title = viewLabel; viewToggle.setAttribute("aria-label", viewLabel);
  viewToggle.removeAttribute("aria-busy"); viewToggle.disabled = false;
  if (viewToggle.dataset.mode !== preferences.mode) {
    viewToggle.dataset.mode = preferences.mode;
    viewToggle.replaceChildren(icon(preferences.mode === "project" ? "recent" : "folderClosed"));
  }
  const pinnedIds = new Set(sidebar.pinnedThreads.map(item => item.id));
  const projectNames = new Map(sidebar.projects.map(project => [project.id, project.name]));
  const domId = (type, id) => `${type}-${encodeURIComponent(id)}`;
  const normalizedSearch = search.replace(/[\\/]+/g, " ");
  const searchMatches = item => !search || `${item.title || ""} ${item.name || ""} ${projectNames.get(item.projectId) || ""}
    ${item.cwd || ""} ${(item.roots || []).join(" ")}`
    .toLowerCase().replace(/[\\/]+/g, " ").includes(normalizedSearch);
  const matches = item => typeMatches(item) && searchMatches(item);
  const sortThreads = (items, mode) => mode === "updated_at"
    ? [...items].sort((left, right) => sidebarUpdatedAt(right) - sidebarUpdatedAt(left)
      || sidebarTimestamp(right.createdAt) - sidebarTimestamp(left.createdAt) || left.id.localeCompare(right.id))
    : [...items];
  const allowedProjectThreadLimits = new Set([5, 10, 20, 50]);
  const projectThreadLimit = allowedProjectThreadLimits.has(appearance.projectThreadLimit)
    ? appearance.projectThreadLimit : 5;
  const isActive = item => {
    const status = item.status?.status || item.status;
    const type = typeof status === "string" ? status : status?.type;
    return type === "active" || type === "inProgress";
  };
  const openThread = async item => {
    await selectThread(item.id);
    if (!item.hasUnreadTurn || threadReadOnly(item.id) || !connected || selectedId !== item.id || thread?.id !== item.id) return;
    try {
      await markVisibleThreadRead(item.id);
    } catch (error) { notice(error.message, true); }
  };
  const threadProjectName = item => projectNames.get(item.projectId)
    || String(item.cwd || "").replace(/[\\/]+$/, "").split(/[\\/]/).pop()
    || "未分配项目";
  function row(item, scope = "recent", recentView = false) {
    const container = element("div", `thread-row${recentView ? " thread-row-recent" : ""}${selectedId === item.id ? " selected" : ""}`);
    container.dataset.threadId = item.id;
    const title = button(item.title, item.title, () => void openThread(item).catch(error => notice(error.message, true)), "thread-link");
    title.id = domId("thread", `${scope}:${item.id}`);
    let titleLine = title;
    if (recentView) {
      // 用户要求最近会话图标只占标题行；项目名与标题行同起点，不因图标预留额外一列。
      titleLine = element("span", "thread-label-line");
      titleLine.append(element("span", "thread-label", item.title));
      const copy = element("span", "thread-copy");
      copy.append(titleLine, element("span", "thread-project", threadProjectName(item)));
      title.replaceChildren(copy);
    } else title.replaceChildren(element("span", "thread-label", item.title));
    if (selectedId === item.id) title.setAttribute("aria-current", "page");
    const attention = sidebarAttentionState(item);
    const active = attention !== "waiting" && (item.sideThreadAttention?.active === true
      || (typeof item.priority?.active === "boolean" ? item.priority.active : isActive(item)));
    // 用户要求 Codex 与 Claude 列表均显示对应品牌，置顶不再用通用聊天图标；
    // 最近列表仅在标题行放图标，与项目/置顶列表共用 1em 尺寸、3px 间距和文字居中对齐规则。
    const backend = item.backend === "claude" || claudeThreadId(item.id) ? "claude" : "codex";
    const mark = icon(backend);
    mark.classList.add("session-type-icon", `${backend}-session-icon`);
    mark.title = backend === "claude" ? "Claude" : "Codex";
    titleLine.prepend(mark);
    container.append(title);
    if (active || attention === "unread" || attention === "waiting") {
      const state = element("span", `thread-state${active ? " active" : " unread"}`);
      state.title = attention === "waiting" ? "等待你的回复" : active ? "正在运行" : "有未读消息";
      state.setAttribute("role", "img"); state.setAttribute("aria-label", state.title);
      container.append(state);
    }
    if (!session.readOnly) {
      const pinned = pinnedIds.has(item.id) || item.pinned;
      const deletion = threadDeletions.get(threadDeletionKey(item.id));
      const deleting = deletion && deletion.state !== "confirming";
      const menu = rowMenu(domId("thread-menu", `${scope}:${item.id}`), `${item.title} 的会话菜单`, [
        { label: "打开会话", icon: "chat", action: () => void openThread(item).catch(error => notice(error.message, true)) },
        ...(independentThreadWindowAvailable() ? [{ value: "openWindow", label: "打开独立窗口", icon: "window",
          disabled: !!deletion, action: () => openIndependentThreadWindow(item) }] : []),
        { label: pinned ? "取消置顶会话" : "置顶会话", icon: "pin", disabled: !!deletion,
          action: () => changeSidebar({ threadId: item.id, pinned: !pinned }) },
        { value: "rename", label: "重命名", icon: "edit", disabled: !!deletion,
          action: () => renameSidebarThread(item) },
        { value: "deleteThread", label: deleting ? deletion.state === "submitting" ? "正在删除会话…" : "核对删除结果" : "删除会话",
          icon: "trash", danger: true, disabled: deleting ? deletion.state === "submitting" || threadDeletionChecks.has(threadDeletionKey(item.id))
            : !!deletion || threadDeletionConflict(item.id),
          action: () => deleting ? checkThreadDeletion(deletion) : deleteSidebarThread(item) }
      ]);
      menu.popupClassName = "thread-row-popup";
      container.append(menu);
      bindSidebarRowMenu(container, menu);
    }
    return container;
  }
  function appendSection(key, label, children, action = null) {
    if (!children.length) return;
    const bodyId = domId("sidebar-section", key);
    const expanded = !!search || !collapsedSidebarSections.has(key);
    const heading = element("div", `section-label${key === "list:priority" ? " priority-section-label" : ""}`);
    const toggle = button("", `${label}分组`, () => {
      if (search) return;
      collapsedSidebarSections.has(key) ? collapsedSidebarSections.delete(key) : collapsedSidebarSections.add(key);
      renderSidebar();
    }, "section-toggle");
    toggle.id = `${bodyId}-toggle`;
    toggle.setAttribute("aria-expanded", String(expanded));
    toggle.setAttribute("aria-controls", bodyId);
    toggle.append(element("span", "section-label-text", label), icon("chevron"));
    heading.append(toggle);
    if (action) heading.append(action);
    const body = element("div", "sidebar-section-body");
    body.id = bodyId; body.hidden = !expanded; body.append(...children);
    root.append(heading, body);
  }
  function projectGroup(project) {
    if (filter !== "all" && !project.threads.some(typeMatches)) return null;
    const available = sortThreads(project.threads.filter(item => !pinnedIds.has(item.id) && typeMatches(item)), preferences.projects);
    const items = searchMatches(project) && search ? available : available.filter(matches);
    if (search && !searchMatches(project) && !items.length) return null;
    const group = element("div", "project-group");
    const header = element("div", "project-header");
    const projectTitle = button("", project.name, () => {
      if (search) return;
      if (collapsed.has(project.id)) collapsed.delete(project.id);
      else {
        collapsed.add(project.id);
        projectVisibleThreadCounts.delete(project.id);
      }
      renderSidebar();
    }, "project-title");
    projectTitle.id = domId("project", project.id);
    const projectExpanded = !!search || !collapsed.has(project.id);
    projectTitle.setAttribute("aria-expanded", String(projectExpanded));
    projectTitle.append(icon(projectExpanded ? "folder" : "folderClosed"), element("span", "project-label", project.name));
    projectTitle.title = project.cwd || project.name;
    header.append(projectTitle);
    if (!session.readOnly) header.append(rowMenu(domId("project-menu", project.id), `${project.name} 的项目菜单`, [
      { label: "在此项目新建会话", icon: "plus", action: () => startNewChat(project.id) },
      { label: project.pinned ? "取消置顶项目" : "置顶项目", icon: "pin", action: () => changeSidebar({ projectId: project.id, pinned: !project.pinned }) }
    ]));
    group.append(header);
    if (projectExpanded) {
      const visible = search ? items : items.slice(0, projectVisibleThreadCounts.get(project.id) ?? projectThreadLimit);
      visible.forEach(item => group.append(row(item, project.id)));
      if (visible.length < items.length) group.append(button("查看更多", "查看更多", () => {
        projectVisibleThreadCounts.set(project.id, Math.min(items.length, visible.length + projectThreadLimit));
        renderSidebar();
      }, "load-more project-load-more"));
      if (!items.length) group.append(element("small", "empty-project",
        project.threads.some(typeMatches) ? "会话已移至置顶区" : project.threads.length ? "没有符合实例筛选的会话" : "暂无会话"));
    }
    return group;
  }
  if (preferences.mode === "project") {
    recentPriorityIds = null;
    const pinnedChildren = sortThreads(sidebar.pinnedThreads.filter(matches), preferences.pinned)
      .map(item => row(item, "pinned"));
    pinnedChildren.push(...sidebar.projects.filter(project => project.pinned).map(projectGroup).filter(Boolean));
    appendSection("project:pinned", "置顶", pinnedChildren);
    appendSection("project:projects", "项目",
      sidebar.projects.filter(project => !project.pinned).map(projectGroup).filter(Boolean));
    const unassigned = sortThreads(sidebar.recent.filter(item => !item.projectId && !pinnedIds.has(item.id) && matches(item)), preferences.chats);
    appendSection("project:recent", "最近", unassigned.map(item => row(item)));
  } else {
    const mergedRecent = [];
    const mergedIds = new Set();
    for (const item of [...sidebar.recent, ...sidebar.pinnedThreads]) {
      if (mergedIds.has(item.id)) continue;
      mergedIds.add(item.id); mergedRecent.push(item);
    }
    const eligible = item => item.priority?.eligible !== false;
    const needsPriority = item => eligible(item) && sidebarAttentionState(item) !== "idle";
    const attentionOrder = { waiting: 0, unread: 1, active: 2, idle: 3 };
    // Keep an already-promoted thread sticky, but do not promote the selected
    // thread from a selection-only refresh until a real activity event arrives.
    const candidates = mergedRecent.filter(item => needsPriority(item)
      && !(item.id === selectedId && recentPrioritySelectionSuppressions.has(item.id))).sort((left, right) =>
      attentionOrder[sidebarAttentionState(left)] - attentionOrder[sidebarAttentionState(right)]
      || sidebarUpdatedAt(right) - sidebarUpdatedAt(left));
    if (recentPriorityIds === null) {
      recentPriorityOpenedAt = Date.now();
      recentPriorityKnownIds = new Set(mergedRecent.map(item => item.id));
      recentPriorityIds = new Set(candidates.map(item => item.id));
    } else {
      const available = new Set(mergedRecent.filter(eligible).map(item => item.id));
      const created = mergedRecent.filter(item => eligible(item) && !recentPriorityKnownIds.has(item.id)
        && sidebarTimestamp(item.createdAt) >= Math.floor(recentPriorityOpenedAt / 1000) * 1000);
      recentPriorityIds = new Set([
        ...created.map(item => item.id), ...[...recentPriorityIds].filter(id => available.has(id)),
        ...candidates.map(item => item.id)
      ]);
      for (const item of mergedRecent) recentPriorityKnownIds.add(item.id);
    }
    // 日期分组始终按最近更新展示，不继承桌面的手动会话顺序。
    const recent = sortThreads(mergedRecent.filter(matches), "updated_at");
    const recentById = new Map(recent.map(item => [item.id, item]));
    const priority = [...recentPriorityIds].flatMap(id => recentById.has(id) ? [recentById.get(id)] : []);
    if (priority.length) {
      const clear = priority.some(item => sidebarAttentionState(item) === "idle")
        ? button("", "清除已读会话", () => {
          for (const item of mergedRecent)
            if (!needsPriority(item)) recentPriorityIds.delete(item.id);
          renderSidebar();
        }, "priority-clear-button") : null;
      if (clear) { clear.id = "clear-recent-priority"; clear.append(icon("eraser")); }
      appendSection("list:priority", "优先级", priority.map(item => row(item, "recent", true)), clear);
    }
    const dated = recent.filter(item => !recentPriorityIds.has(item.id));
    const visible = search ? dated : dated.slice(0, recentLimit);
    for (const group of sidebarRecentDateGroups(visible)) {
      const children = group.items.map(item => row(item, "recent", true));
      if (group.key === "history") {
        if (visible.length < dated.length)
          children.push(button("显示更多会话", "显示更多会话", () => { recentLimit += 40; renderSidebar(); }, "load-more"));
        else if (sidebar.nextCursor)
          children.push(button("加载更多会话", "加载更多会话", () => {
            void loadSidebar(sidebar.nextCursor).catch(error => notice(error.message, true));
          }, "load-more"));
      }
      appendSection(`list:${group.key}`, group.label, children);
    }
  }
  if (preferences.mode === "project" && sidebar.nextCursor) root.append(button("加载更多会话", "加载更多会话", () => {
    void loadSidebar(sidebar.nextCursor).catch(error => notice(error.message, true));
  }, "load-more"));
  if (!root.querySelector(".thread-row, .project-group"))
    root.append(element("p", "empty-project", search ? "没有匹配的会话或项目" : "暂无最近会话"));
  setSelectOptions($("#project-select"), newChatProjectOptions(sidebar.projects));
  changeCreationProject();
  root.scrollTop = scrollTop;
  if (popupState) {
    const trigger = document.getElementById(popupState.id);
    if (trigger && !trigger.disabled) {
      openPopup(trigger, { focus: false, position: popupState.position });
      if (activePopup) {
        activePopup.index = popupState.index;
        if (popupState.focused) activePopup.list.querySelectorAll("button")[popupState.index]?.focus({ preventScroll: true });
      }
    }
  } else if (focusId) {
    const restored = document.getElementById(focusId) || [...root.querySelectorAll(".thread-row")]
      .find(item => item.dataset.threadId === focusedThread)?.querySelector(focusedMenu ? ".row-menu-button" : ".thread-link");
    restored?.focus({ preventScroll: true });
  }
  fitPopup();
  scheduleDesktopObservation();
}
