// 侧栏列表：分页读取、最近/项目分组渲染与客户端未读角标同步；本地已读、优先显示与折叠分组等状态在 list-state.js。

import { $, button, element } from "../../ui/dom.js";
import { copyText } from "../../ui/clipboard.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { activePopup, closePopup, fitPopup, openPopup, renderPopup, togglePopup } from "../../ui/popup.js";
import { setSelectOptions } from "../../ui/select.js";
import { selectThread } from "../../core/actions.js";
import { connected, pageLeaving, selectedId, session, setSidebar, sidebar, thread } from "../../core/state.js";
import { api, promoteReadPriority } from "../api.js";
import { appearance } from "../appearance/theme.js";
import { newChatProjectOptions } from "../composer/creation-state.js";
import { changeCreationProject } from "../composer/new-chat.js";
import { startNewChat } from "../conversation/selection.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { claudeThreadId, isActiveThread, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { foregroundReadGeneration, initialAuthenticationRequired } from "../session/connection-state.js";
import { enabledInstanceIds } from "../session/instances.js";
import { threadOpenInMenu } from "../session/open-in.js";
import { syncSideThreadUnread } from "../session/related-state.js";
import { workspaceSidePanel } from "../session/side-panel.js";
import { cacheDisplayData, sidebarSnapshot } from "../session/thread-cache.js";
import {
  bindSidebarRowMenu, changeSidebar, filterSideThreadSidebar, independentThreadWindowAvailable,
  openIndependentThreadWindow, renameSidebarThread, restoreSidebarPreferences, rowMenu,
  setRecentPinnedVisible, sidebarFilters
} from "./actions.js";
import { filterDeletedSidebar, threadDeletionChecks, threadDeletionKey, threadDeletions } from "./deletion-state.js";
import { projectTypeVisible, renderSessionFilter, sessionTypeVisible } from "./filters.js";
import {
  clientUnreadBadgeRequest, clientUnreadBadgeTimer, clientUnreadThreadCount, collapsed,
  collapsedSidebarSections, markSidebarThreadReadLocally, mergeIncompleteSidebar,
  projectVisibleThreadCounts, recentLimit, recentPriorityIds, recentPriorityKnownIds,
  recentPriorityOpenedAt, recentPrioritySelectionSuppressions, search,
  setClientUnreadBadgeRequest, setClientUnreadBadgeTimer, setRecentLimit, setRecentPriorityIds,
  setRecentPriorityKnownIds, setRecentPriorityOpenedAt, setSidebarFilterDeviceId, setSidebarRequest,
  setSidebarVersion, sidebarAttentionState, sidebarFilterDeviceId, sidebarReadRefreshSuppressions,
  sidebarRecentDateGroups, sidebarRequest, sidebarThreadActive, sidebarTimestamp, sidebarUpdatedAt, sidebarVersion
} from "./list-state.js";
import { checkThreadDeletion, deleteSidebarThread, threadDeletionConflict } from "./thread-deletion.js";

function clientUnreadBadgeAvailable() {
  return !workspaceSidePanel && typeof window.__CODEX_ELVES_UNREAD_BADGE__?.setCount === "function";
}

// 用户要求固定为重命名、置顶、独立窗口、复制、打开方式、删除；复制和打开方式使用二级菜单。
// WSL 会话 ID 必须带真实发行版名称，优先复制后端明确提供的 nativeId，不把 CLI 命令当作 ID。
export function threadMenuOptions(item, {
  pinned = false, deletion = null, deleting = false, independentWindow = false,
  deletionCheckPending = false, deletionConflict = false, openIn = null,
  openWindow, togglePinned, rename, deleteThread
} = {}) {
  const projectPath = typeof item?.cwd === "string" && item.cwd.trim() ? item.cwd : "";
  const nativeId = typeof item?.nativeId === "string" ? item.nativeId.trim() : "";
  const sessionId = nativeId || (typeof item?.id === "string" ? item.id.trim() : "");
  const distribution = typeof item?.wslDistribution === "string" ? item.wslDistribution.trim() : "";
  const wsl = typeof item?.instanceId === "string"
    ? item.instanceId.includes(":wsl:") : !!distribution;
  const copiedId = sessionId && (!wsl || distribution) ? `${wsl ? `${distribution} ` : ""}${sessionId}` : "";
  const copies = [
    { value: "copyWorkingPath", label: "复制工作路径", icon: "folderClosed",
      disabled: !projectPath, action: () => void copyText(projectPath) },
    { value: "copySessionId", label: "复制会话 ID", icon: "copy",
      disabled: !copiedId, action: () => void copyText(copiedId) }
  ];
  return [
    { value: "rename", label: "重命名", icon: "edit", disabled: !!deletion, action: rename },
    { value: "pin", label: pinned ? "取消置顶" : "置顶", icon: "pin", disabled: !!deletion, action: togglePinned },
    { value: "openWindow", label: "独立窗口打开", icon: "window",
      disabled: !!deletion || !independentWindow, action: openWindow },
    { value: "copy", label: "复制", icon: "copy", separatorBefore: true,
      submenu: copies, submenuKey: JSON.stringify([projectPath, copiedId]), submenuWidth: 200 },
    { ...openIn, value: "openIn", label: "打开方式", icon: "folder",
      disabled: !!deletion || !openIn || openIn.disabled },
    { value: "deleteThread",
      label: deleting ? deletion?.state === "submitting" ? "正在删除会话…" : "核对删除结果" : "删除会话",
      icon: "trash", danger: true, separatorBefore: true,
      disabled: deleting ? deletion?.state === "submitting" || deletionCheckPending
        : !!deletion || deletionConflict,
      action: deleteThread }
  ];
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
  setClientUnreadBadgeRequest(request);
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
    finally { if (clientUnreadBadgeRequest === request) setClientUnreadBadgeRequest(null); }
  })();
  return request.promise;
}
export function stopClientUnreadBadgePolling(clear = false) {
  clearInterval(clientUnreadBadgeTimer); setClientUnreadBadgeTimer(null);
  clientUnreadBadgeRequest?.controller.abort();
  setClientUnreadBadgeRequest(null);
  if (clear && clientUnreadBadgeAvailable()) window.__CODEX_ELVES_UNREAD_BADGE__.setCount(0);
}
export function startClientUnreadBadgePolling() {
  stopClientUnreadBadgePolling();
  if (!clientUnreadBadgeAvailable() || !session || !document.hidden || pageLeaving) return;
  // 仅独立客户端主窗口在后台保留轻量未读刷新；实时事件沿用原 1.2 秒合并，
  // 无事件或断线后以 15 秒只读查询补齐，不恢复正文、模型、用量等前台轮询。
  void refreshClientUnreadBadge();
  setClientUnreadBadgeTimer(setInterval(() => void refreshClientUnreadBadge(), 15000));
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
  const version = setSidebarVersion(sidebarVersion + 1), deviceId = session?.device.id;
  if (sidebarFilterDeviceId !== deviceId) {
    setSidebarFilterDeviceId(deviceId);
    restoreSidebarPreferences();
    setRecentPriorityIds(null);
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
      if (result.projectPreferencesWarning && result.projectPreferencesWarning !== sidebar?.projectPreferencesWarning)
        notice(result.projectPreferencesWarning, true);
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
  request.promise = promise; setSidebarRequest(request);
  try { await promise; }
  finally {
    request.releasePriority?.();
    if (sidebarRequest?.promise === promise) setSidebarRequest(null);
  }
}
function captureSidebarPopup(root) {
  const popup = activePopup && root.contains(activePopup.trigger) ? activePopup : null;
  if (!popup) return null;
  const box = popup.trigger.getBoundingClientRect();
  return { popup, id: popup.trigger.id, position: popup.position
    ? { x: popup.position.x - box.left, y: popup.position.y - box.top } : null };
}
function restoreSidebarPopup(state) {
  if (!state || activePopup !== state.popup) return;
  const { popup } = state, trigger = document.getElementById(state.id);
  if (!trigger || trigger.disabled) { closePopup(); return; }
  // 用户要求右键菜单不能因侧栏刷新再次弹出，也不能沿用刷新前的屏幕坐标；
  // 保留同一弹层节点，仅换绑同会话的新行，并按行内偏移计算当前锚点。
  popup.trigger.setAttribute("aria-expanded", "false");
  popup.trigger.removeAttribute("aria-controls");
  popup.trigger = trigger;
  if (state.position) {
    const box = trigger.getBoundingClientRect();
    popup.position = { x: box.left + state.position.x, y: box.top + state.position.y };
  }
  trigger.setAttribute("aria-expanded", "true");
  trigger.setAttribute("aria-controls", popup.node.id);
  renderPopup();
}
export function recentPriorityMenuOptions(showPinned, togglePinnedSection) {
  // 用户要求勾选后显示“√”，表示显示置顶分组，不是把优先事项中的全部会话设为置顶。
  return [{ value: "pinned", label: "置顶", icon: "pin", checkable: true,
    selected: showPinned === true, checkmark: "√", keepOpen: true, action: togglePinnedSection }];
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
  const popupState = captureSidebarPopup(root);
  const focusId = root.contains(document.activeElement) ? document.activeElement.id : null;
  const focusedThread = document.activeElement.closest(".thread-row")?.dataset.threadId;
  const focusedMenu = document.activeElement.classList.contains("row-menu-button");
  root.replaceChildren();
  // 用户要求按会话实例筛选所有分组，并与实例开关取交集；保留原快照和未读状态。
  const typeMatches = item => sessionTypeVisible(item);
  const preferences = {
    mode: sidebarFilters.mode ?? sidebar.preferences.mode,
    chats: sidebarFilters.chats ?? sidebar.preferences.chats,
    projects: sidebarFilters.projects ?? sidebar.preferences.projects,
    pinned: sidebarFilters.pinned ?? sidebar.preferences.pinned,
    showPinned: sidebarFilters.showPinned === true
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
    const active = attention !== "waiting" && sidebarThreadActive(item);
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
      const menu = rowMenu(domId("thread-menu", `${scope}:${item.id}`), `${item.title} 的会话菜单`,
        threadMenuOptions(item, {
          pinned, deletion, deleting, independentWindow: independentThreadWindowAvailable(),
          openIn: threadOpenInMenu(item, {
            isAvailable: () => !threadDeletions.has(threadDeletionKey(item.id))
          }),
          deletionCheckPending: threadDeletionChecks.has(threadDeletionKey(item.id)),
          deletionConflict: threadDeletionConflict(item.id),
          openWindow: () => openIndependentThreadWindow(item),
          togglePinned: () => changeSidebar({ threadId: item.id, pinned: !pinned }),
          rename: () => renameSidebarThread(item),
          deleteThread: () => deleting ? checkThreadDeletion(deletion) : deleteSidebarThread(item)
        }));
      menu.popupClassName = "thread-row-popup";
      // 与 Codex 参考菜单一致保留图标列和子菜单箭头；宽度与 style.css 中的紧凑会话菜单同步核对。
      menu.popupWidth = 224;
      container.append(menu);
      bindSidebarRowMenu(container, menu);
    }
    return container;
  }
  function appendSection(key, label, children, action = null, { keepEmpty = false } = {}) {
    if (!children.length && !keepEmpty) return;
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
    if (!projectTypeVisible(project)) return null;
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
    setRecentPriorityIds(null);
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
      setRecentPriorityOpenedAt(Date.now());
      setRecentPriorityKnownIds(new Set(mergedRecent.map(item => item.id)));
      setRecentPriorityIds(new Set(candidates.map(item => item.id)));
    } else {
      const available = new Set(mergedRecent.filter(eligible).map(item => item.id));
      const created = mergedRecent.filter(item => eligible(item) && !recentPriorityKnownIds.has(item.id)
        && sidebarTimestamp(item.createdAt) >= Math.floor(recentPriorityOpenedAt / 1000) * 1000);
      setRecentPriorityIds(new Set([
        ...created.map(item => item.id), ...[...recentPriorityIds].filter(id => available.has(id)),
        ...candidates.map(item => item.id)
      ]));
      for (const item of mergedRecent) recentPriorityKnownIds.add(item.id);
    }
    // 日期分组始终按原生最近活动展示；仅打开旧会话不重排，也不继承桌面的手动会话顺序。
    const recent = sortThreads(mergedRecent.filter(matches), "updated_at");
    const recentById = new Map(recent.map(item => [item.id, item]));
    const priority = [...recentPriorityIds].flatMap(id => recentById.has(id) ? [recentById.get(id)] : []);
    const actions = element("span", "priority-section-actions");
    const clear = priority.some(item => sidebarAttentionState(item) === "idle")
      ? button("", "清除已读会话", () => {
        for (const item of mergedRecent)
          if (!needsPriority(item)) recentPriorityIds.delete(item.id);
        renderSidebar();
      }, "priority-clear-button") : null;
    if (clear) { clear.id = "clear-recent-priority"; clear.append(icon("eraser")); actions.append(clear); }
    const scope = privateInputScope(session);
    const options = button("", "优先事项设置", () => togglePopup(options), "icon-button priority-options-button");
    options.id = "recent-priority-options";
    options.setAttribute("aria-haspopup", "menu"); options.setAttribute("aria-expanded", "false");
    options.append(icon("slidersHorizontal"));
    options.menuOptions = recentPriorityMenuOptions(preferences.showPinned, () => {
      if (!scope || scope !== privateInputScope(session)) return;
      setRecentPinnedVisible(!preferences.showPinned);
    });
    options.popupWidth = 160;
    options.onkeydown = event => {
      if (["ArrowDown", "ArrowUp"].includes(event.key)) {
        event.preventDefault(); openPopup(options, { edge: event.key === "ArrowUp" ? "last" : "first" });
      }
    };
    // 用户要求设置图标固定在最右侧，清除已读在其左侧；空优先事项也保留标题和入口。
    // 内部分组键沿用 list:priority，改名不清除已有折叠及优先事项状态。
    actions.append(options);
    appendSection("list:priority", "优先事项", priority.map(item => row(item, "recent", true)), actions, { keepEmpty: true });
    if (preferences.showPinned) {
      appendSection("list:pinned", "置顶",
        sortThreads(sidebar.pinnedThreads.filter(matches), preferences.pinned).map(item => row(item, "pinned", true)),
        null, { keepEmpty: true });
    }
    // 置顶分组开启时不在日期分组重复显示；有未读/执行状态的置顶会话仍保留原有优先事项提升。
    const dated = recent.filter(item => !recentPriorityIds.has(item.id)
      && (!preferences.showPinned || !pinnedIds.has(item.id)));
    const visible = search ? dated : dated.slice(0, recentLimit);
    for (const group of sidebarRecentDateGroups(visible)) {
      const children = group.items.map(item => row(item, "recent", true));
      if (group.key === "history") {
        if (visible.length < dated.length)
          children.push(button("显示更多会话", "显示更多会话", () => { setRecentLimit(recentLimit + (40)); renderSidebar(); }, "load-more"));
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
  if (popupState) restoreSidebarPopup(popupState);
  else if (focusId) {
    const restored = document.getElementById(focusId) || [...root.querySelectorAll(".thread-row")]
      .find(item => item.dataset.threadId === focusedThread)?.querySelector(focusedMenu ? ".row-menu-button" : ".thread-link");
    restored?.focus({ preventScroll: true });
  }
  fitPopup();
}
