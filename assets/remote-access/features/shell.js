// 主界面骨架：生成工作区布局、绑定侧栏/输入区/会话菜单等控件并挂载会话工具；创建的共享引用写入 shell-state.js 供其他模块读取。

import {
  handleComposerImagePaste, handleComposerLocalPaste, handleComposerLocalPasteShortcut,
  mountComposerAttachments, openVideoPreview
} from "../attachments.js";
import { prepareRemoteTunnelAccess, remoteChannelRoutes, requestRemote } from "../channels.js";
import { permissionOptions } from "../permissions.js";
import { openAndroidBrowser, postPlatformMessage } from "../platform.js";
import { createWorkspaceBranch } from "../session-files.js";
import { mountSessionMenu } from "../session-menu.js";
import { mountSessionTools } from "../session-tools.js";
import { copyText } from "../ui/clipboard.js";
import { confirmAction } from "../ui/dialog.js";
import { $, button, element } from "../ui/dom.js";
import { icon } from "../ui/icons.js";
import { notice } from "../ui/notice.js";
import { activePopup, closePopup, openPopup, togglePopup } from "../ui/popup.js";
import { selectControl, setSelectValue } from "../ui/select.js";
import { composerTooltips, installComposerTooltips, setComposerTooltips } from "../ui/tooltip.js";
import {
  closeComposerMenus, loadModels, renderSidebar, requireAuthentication, updateComposer
} from "../core/actions.js";
import { app, connected, creating, nativeConnectionVersion, selectedId, session, sidebar } from "../core/state.js";
import { api } from "./api.js";
import { openAppearance } from "./appearance/dialog.js";
import { setApprovalSignature } from "./approval-state.js";
import { fitClaudeUsageTip, mountClaudeUsage, resetClaudeUsage } from "./composer/claude-usage.js";
import { closeSkillMenu, fitApprovalArea, fitComposerMenu, renderModelMenu } from "./composer/composer-menus.js";
import { mountComposerScroll, sendMessage, stopGeneration } from "./composer/composer.js";
import { creationInstanceId, creationProject } from "./composer/creation-state.js";
import { clearPrivateInputs } from "./composer/draft-restore.js";
import { rememberDraft } from "./composer/drafts.js";
import {
  closeContextUsageTip, fitContextUsageTip, mountContextUsage, resetComposerMetrics
} from "./composer/metrics.js";
import { modelContextKey } from "./composer/model-catalog.js";
import { toggleRemoteFast } from "./composer/models.js";
import { changeCreationBackend, changeCreationProject } from "./composer/new-chat.js";
import { modelsLoadedAt, setModelQuery } from "./composer/options-state.js";
import { changeComposerPermissions, loadInstancePermissionCatalog } from "./composer/permissions.js";
import { configurePromptEditor } from "./composer/prompt-edit-history.js";
import { resizeEditorInput } from "./composer/prompt-editor.js";
import { activatePromptOptimization, openPromptOptimizationSettings } from "./composer/prompt-optimization.js";
import { removeAdjacentPromptToken, selectSkill } from "./composer/selected-skills.js";
import { moveSlashIndex, skillIndex } from "./composer/skill-catalog.js";
import {
  handlePromptInput, renderSkillMenu, selectCompactCommand, selectPromptOptimizationCommand, slashMenuEntries, updateSkillMenu
} from "./composer/skills.js";
import {
  followMessages, historyScrollTop, setFollowMessages, setHistoryScrollTop, updateScrollControl
} from "./conversation/history-state.js";
import { scheduleHistoryLoad } from "./conversation/history.js";
import { savePending } from "./conversation/pending-store.js";
import { welcomeNode } from "./conversation/selection-state.js";
import { startNewChat } from "./conversation/selection.js";
import { selectGoalCommand } from "./goal/composer.js";
import { openImagePreview } from "./messages/media-state.js";
import { editingMessage } from "./messages/message-edit.js";
import { createSubagentIcon, subagentName } from "./messages/subagents.js";
import { markVisibleThreadRead } from "./messages/user-content.js";
import { setPendingNotificationOpen } from "./notifications.js";
import {
  isActiveThread, isClaude, privateInputScope, sideThreadsEnabled, subagentsEnabled, threadReadOnly
} from "./session/capabilities.js";
import {
  connectionRttDescription, currentChannelMeasurements, loginPageCleanup,
  markMessageChannelUnavailable, setAuthenticationRequired
} from "./session/connection-state.js";
import { activeInstanceId, newChatBackendOptions } from "./session/instances.js";
import { mountWorkspaceOpenIn } from "./session/open-in.js";
import { disconnect, refreshConnectionFromIndicator } from "./session/lifecycle.js";
import { showLogin } from "./session/login.js";
import { relatedSessionView, relatedThreadOperations, stopRelatedSessionTimers } from "./session/related-state.js";
import {
  adoptRelatedMenu, createDesktopSideThread, loadRelatedThreads, openRelatedThread,
  ownsRelatedOperation, rememberTransientHistory, restoreRelatedOperations, returnToParentSession,
  runRelatedOperation, syncRelatedSessionLayout
} from "./session/related.js";
import { workspaceSidePanel } from "./session/side-panel.js";
import { relatedMenuCache, scheduleReadingPosition, workspaceViewCache } from "./session/thread-cache.js";
import {
  announceAppReady, closeMobileSidebar, composerScroll, desktopSidebarOpen, messagesResizeObserver,
  sessionMenu, sessionTools, setCloseMobileSidebar, setComposerScroll, setDesktopSidebarOpen,
  setMessagesResizeObserver, setSessionMenu, setSessionTools, setSidebarLayoutCleanup,
  setSyncRelatedSidebarLayout, setWorkspaceOpenIn, sidebarLayoutCleanup, syncRelatedSidebarLayout, workspaceOpenIn
} from "./shell-state.js";
import { changeSidebar, openWorkspaceFileContextMenu, sidebarFilters } from "./sidebar/actions.js";
import { renderSessionFilter } from "./sidebar/filters.js";
import { setSearch } from "./sidebar/list-state.js";

export function shell() {
  loginPageCleanup?.();
  closePopup();
  sessionMenu?.dispose(); setSessionMenu(null);
  stopRelatedSessionTimers();
  sessionTools?.dispose(); setSessionTools(null);
  workspaceOpenIn?.dispose(); setWorkspaceOpenIn(null);
  closeContextUsageTip();
  resetClaudeUsage();
  resetComposerMetrics();
  setApprovalSignature(null);
  setSearch("");
  composerScroll?.dispose(); setComposerScroll(null);
  sidebarLayoutCleanup?.(); setSidebarLayoutCleanup(null); setCloseMobileSidebar(() => {});
  app.innerHTML = `<div class="workspace">
    <div class="sidebar-backdrop"></div>
    <aside id="session-sidebar" class="sidebar" aria-label="会话侧边栏">
      <div class="sidebar-header"><strong>✦ CodexElves</strong><div class="sidebar-header-actions"><button id="sidebar-type-filter" class="icon-button sidebar-type-filter" type="button" title="筛选会话实例" aria-label="筛选会话实例" aria-haspopup="menu"></button><button id="sidebar-view-toggle" class="icon-button sidebar-view-toggle" type="button" aria-label="切换会话视图" disabled></button><button id="sidebar-search-toggle" class="icon-button sidebar-search-toggle" type="button" title="搜索会话与项目" aria-label="搜索会话与项目" aria-controls="sidebar-search" aria-expanded="false" aria-pressed="false"></button></div></div>
      <div class="sidebar-action"><button id="new-chat" class="new-chat" type="button">＋ 新建会话</button><input id="sidebar-search" class="search" type="search" placeholder="搜索会话与项目" aria-label="搜索会话与项目" autocomplete="off" hidden></div>
      <div id="sidebar-content" class="sidebar-content"></div>
      <div class="sidebar-footer"><div class="footer-actions"><button id="appearance-button" class="footer-button" type="button" aria-label="设置"></button><button id="logout" class="footer-button" type="button"></button></div></div>
    </aside>
    <main class="conversation">
      <header class="conversation-header"><button id="open-sidebar" class="icon-button" type="button" aria-label="打开侧边栏" aria-controls="session-sidebar"></button><div class="thread-heading"><div class="thread-title-line"><span id="thread-backend-icon" class="thread-backend-icon" aria-hidden="true" hidden></span><strong id="thread-title">远程会话</strong></div><span id="thread-path" class="muted"></span></div><button id="connection-state" class="connection-state pending" type="button" aria-live="polite" title="${connectionRttDescription}" aria-label="连接中…。${connectionRttDescription}">连接中…</button><button id="open-session-tools" class="icon-button session-tools-trigger" type="button" title="会话菜单" aria-label="会话菜单" aria-haspopup="menu" aria-expanded="false"></button><span id="related-session-model" class="related-session-model muted" hidden></span><button id="related-session-back" class="text-button" type="button" aria-label="返回主会话" hidden>返回</button></header>
      <div class="messages-shell"><div id="messages" class="messages" role="log" aria-label="会话消息"></div><button id="scroll-to-bottom" class="scroll-to-bottom" type="button" aria-label="跳转到最新消息" hidden></button></div>
      <div class="composer-area"><div class="composer-context"><div id="queued-area" hidden></div><div id="goal-area" hidden></div></div><div id="pending-area"></div><div id="project-select-slot" hidden></div><div class="composer-anchor"><div id="approval-area" class="approval-area" aria-live="polite"></div><div id="side-thread-expired" class="side-thread-expired-banner" role="status" hidden><strong>侧边会话已过期</strong><span>该会话已结束，当前仅可查看已保留的消息。</span></div><form id="composer" class="composer">
        <div class="composer-input"><div class="composer-input-content">
        <div id="prompt" class="prompt-editor" contenteditable="true" role="textbox" aria-multiline="true" data-placeholder="给 Codex 发送消息，输入 / 选择命令或 Skill…" aria-label="消息输入" aria-autocomplete="list" aria-controls="skill-menu" aria-expanded="false" spellcheck="true"></div>
        </div></div>
        <div class="composer-toolbar"><div class="composer-controls"><button id="composer-add" class="icon-button composer-add-button" type="button" aria-label="添加图片、压缩上下文或设置目标" aria-haspopup="menu" aria-expanded="false"></button><button id="prompt-optimize" class="icon-button prompt-optimize-button" type="button" aria-label="优化提示词" aria-busy="false" disabled>✨</button><button id="context-info" class="context-info muted" type="button" aria-label="查看上下文用量" aria-controls="context-usage-tooltip" aria-expanded="false"></button><button id="fast-button" class="icon-button fast-button" type="button" aria-label="开启本会话 Fast" aria-pressed="false" aria-busy="false" disabled hidden><span class="fast-bolt" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h8l-1 8 11-12h-8V2Z"/></svg></span></button><button id="model-button" class="text-button model-button" type="button" aria-label="选择模型与思考深度" aria-haspopup="menu" aria-controls="model-settings" aria-expanded="false">正在读取模型…</button></div><div class="composer-send"><span id="send-status" class="composer-send-status" role="status" aria-live="polite"></span><button id="send" class="send-button" type="submit" aria-label="发送消息"></button></div></div>
        <div id="model-settings" class="composer-popover" role="menu" aria-label="模型与思考深度" hidden></div>
        <div id="skill-menu" class="composer-popover skill-menu" role="listbox" aria-label="命令与 Skill" hidden></div>
      </form><span id="session-metrics" class="composer-metrics" tabindex="0" aria-label="会话 Token 统计" hidden></span><button id="claude-usage" class="claude-usage" type="button" aria-label="查看 Claude 账号信息" aria-controls="claude-usage-tooltip" aria-expanded="false" hidden></button></div></div>
    </main>
  </div>`;
  configurePromptEditor($("#prompt"));
  mountContextUsage();
  mountClaudeUsage();
  (composerTooltips || setComposerTooltips(installComposerTooltips()));
  // 用户要求移除顶部 CLI 连接按钮，不再创建或按会话类型恢复该入口。
  $("#open-sidebar").append(icon("menu"));
  // 用户要求 Open in 位于会话菜单左侧；独立客户端内创建，右侧标签页开关保持原有位置。
  setWorkspaceOpenIn(mountWorkspaceOpenIn({ anchor: $("#open-session-tools"),
    beforeOpen: () => { sessionMenu?.close(); closeComposerMenus(); } }));
  $("#related-session-back").onclick = () => void returnToParentSession().catch(error => notice(error.message, true));
  // 用户要求提示词优化图标恢复正常前景色；矢量图标与 / 菜单共用 sparkles，避免 emoji 在 WebView2 中发灰。
  $("#prompt-optimize").replaceChildren(icon("sparkles"));
  $("#prompt-optimize").onclick = () => void activatePromptOptimization();
  $("#fast-button").onclick = () => void toggleRemoteFast();
  $("#prompt-optimize").oncontextmenu = event => {
    event.preventDefault();
    // 触摸长按不触发 PC 右键设置，避免手机横屏时因视口变宽而改变原交互。
    if (event.pointerType !== "touch") void openPromptOptimizationSettings();
  };
  $("#prompt-optimize").onpointerdown = event => {
    if (event.isPrimary && event.button === 0 && document.activeElement === $("#prompt")) event.preventDefault();
  };
  mountComposerAttachments({
    form: $("#composer"),
    prompt: $("#prompt"),
    onChange: () => { rememberDraft(); updateComposer(); },
    notice,
    onPreview: openImagePreview
  });
  setComposerScroll(mountComposerScroll());
  announceAppReady();
  const add = $("#composer-add");
  add.append(icon("plus"));
  add.setAttribute("aria-label", "添加附件或会话操作");
  // 用户要求权限紧邻“+”；小屏只展示图标，空白由此入口后的自动外边距吸收。
  const permissions = selectControl("permission-button", "选择会话权限", permissionOptions("codex"), "configured",
    "permission-button", selected => [icon(selected?.icon || "shield"),
      element("span", "permission-button-label", selected?.shortLabel || selected?.label || "权限"), icon("chevron")]);
  // 实际模式尚未确认时保持占位，不自动选中列表第一项，也不为占位增加特殊权限选项。
  permissions.selectAllowEmpty = true;
  permissions.popupDirection = "up";
  permissions.popupClassName = "composer-permissions-popup";
  permissions.onchange = () => void changeComposerPermissions();
  // 权限菜单沿用 Select 自带的点击开关；这里只读取 Claude 模式，避免同一次点击打开后又被关闭。
  permissions.addEventListener("click", () => {
    if (isClaude()) void loadInstancePermissionCatalog(activeInstanceId());
  });
  const permissionKeydown = permissions.onkeydown;
  permissions.onkeydown = event => {
    permissionKeydown(event);
    if (isClaude() && ["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key))
      void loadInstancePermissionCatalog(activeInstanceId());
  };
  add.after(permissions);
  add.popupDirection = "up";
  add.popupClassName = "composer-add-popup";
  add.popupWidth = 300;
  add.onclick = () => {
    togglePopup(add);
    if (activePopup?.trigger === add) activePopup.list.querySelector("button:not(:disabled)")?.focus();
  };
  add.onkeydown = event => {
    if (["ArrowDown", "ArrowUp"].includes(event.key)) {
      event.preventDefault(); openPopup(add, { edge: event.key === "ArrowUp" ? "last" : "first" });
      if (document.activeElement === add) activePopup?.list.querySelector("button:not(:disabled)")?.focus();
    }
  };
  $("#messages").append(welcomeNode());
  $("#messages").onscroll = () => {
    const messages = $("#messages");
    const wasFollowing = followMessages;
    const scrollingUp = messages.scrollTop < historyScrollTop;
    setHistoryScrollTop(messages.scrollTop);
    setFollowMessages(!composerScroll?.collapsed && messages.scrollHeight - messages.scrollTop - messages.clientHeight < 100);
    updateScrollControl();
    scheduleHistoryLoad(scrollingUp);
    scheduleReadingPosition();
    if (!wasFollowing && followMessages && !isActiveThread())
      void markVisibleThreadRead().catch(error => notice(error.message, true));
  };
  $("#scroll-to-bottom").onclick = () => {
    const messages = $("#messages");
    composerScroll?.expand();
    $(".composer-input").getAnimations().forEach(animation => animation.finish());
    setFollowMessages(true); messages.scrollTop = messages.scrollHeight;
    updateScrollControl();
    if (!isActiveThread())
      void markVisibleThreadRead().catch(error => notice(error.message, true));
  };
  messagesResizeObserver?.disconnect();
  setMessagesResizeObserver(new ResizeObserver(() => {
    resizeEditorInput(editingMessage?.input);
    const messages = $("#messages");
    if (messages && followMessages) messages.scrollTop = messages.scrollHeight;
    fitApprovalArea();
    fitContextUsageTip();
    fitClaudeUsageTip();
    updateScrollControl();
    scheduleHistoryLoad();
  }));
  messagesResizeObserver.observe($("#messages"));
  messagesResizeObserver.observe($("#composer"));
  messagesResizeObserver.observe($(".composer-area"));
  $("#sidebar-view-toggle").append(icon("folderClosed"));
  $("#sidebar-search-toggle").append(icon("search"));
  const projectSelect = selectControl("project-select", "新会话项目", [], "", "new-chat-project-select", selected =>
    [...(selected?.value ? [icon("folder")] : []), element("span", "select-label", selected?.label || "不在项目中工作")]);
  projectSelect.selectSearchable = true;
  projectSelect.popupClassName = "project-picker-popup";
  projectSelect.popupDirection = "up";
  projectSelect.popupWidth = 400;
  projectSelect.popupMaxHeight = 400;
  const projectPicker = $("#project-select-slot");
  projectPicker.classList.add("new-chat-project");
  const projectControl = element("div", "new-chat-project-control");
  const clearProject = button("", "不在项目中工作", () => {
    if (projectSelect.disabled || !projectSelect.value) return;
    closePopup();
    setSelectValue(projectSelect, "");
    projectSelect.dispatchEvent(new Event("change", { bubbles: true }));
    projectSelect.focus();
  }, "new-chat-project-clear");
  clearProject.id = "clear-new-chat-project";
  clearProject.hidden = true;
  clearProject.append(icon("closeCircle"));
  projectControl.append(projectSelect, clearProject);
  const branchBadge = createWorkspaceBranch();
  branchBadge.id = "new-chat-branch";
  branchBadge.classList.add("new-chat-branch");
  branchBadge.setAttribute("aria-live", "polite");
  // 用户要求类型按钮去掉箭头、保留品牌图标和名称；列表中的两类会话也共用相同标识。
  const backendSelect = selectControl("new-chat-backend", "会话实例",
    newChatBackendOptions(),
    creationInstanceId, "new-chat-backend", selected => {
      if (!selected) return [element("span", "select-label", "暂无已启用实例")];
      const mark = icon(selected.icon);
      mark.classList.add(`backend-icon-${selected.backend}`);
      return [mark, element("span", "select-label", selected.label)];
    });
  backendSelect.popupClassName = "backend-picker-popup";
  backendSelect.popupDirection = "up";
  // 类型菜单原先使用非搜索菜单的 200px 宽度；用户要求将它作为最小宽度，内容较长时自然撑开。
  backendSelect.popupWidth = 200;
  backendSelect.popupAutoWidth = true;
  backendSelect.onchange = changeCreationBackend;
  // 用户指定入口位于分支右侧，仅新建时可选；已有会话不渲染后端切换入口。
  projectPicker.append(projectControl, branchBadge, backendSelect);
  $("#send").append(icon("arrowUp"));
  $("#logout").append(icon("logout"), element("span", "", "退出"));
  $("#appearance-button").append(icon("settings"), element("span", "", "设置"));
  $("#appearance-button").title = "设置";
  const workspace = $(".workspace"), sidebarPanel = $("#session-sidebar");
  const compactSidebarMedia = matchMedia("(max-width: 760px)");
  const sidebarBackdrop = $(".sidebar-backdrop");
  // 用户要求遮罩在展开 100ms 后即可关闭，不再等待完整滑入动画；
  // 仍只接受保护期后在遮罩开始并结束的独立手势，避免展开时的同一次触摸误关。
  const sidebarBackdropGuardMs = 100;
  let sidebarBackdropReadyAt = 0, sidebarBackdropPress = null;
  const sidebarIsOpen = () => compactSidebarMedia.matches
    ? workspace.classList.contains("sidebar-open") : desktopSidebarOpen;
  const syncSidebarLayout = () => {
    const compact = compactSidebarMedia.matches;
    const open = !relatedSessionView && sidebarIsOpen();
    if (!compact || !open) {
      sidebarBackdropReadyAt = 0; sidebarBackdropPress = null;
    }
    const toggle = $("#open-sidebar");
    workspace.classList.toggle("sidebar-collapsed", !compact && !open);
    if (!compact) workspace.classList.remove("sidebar-open");
    sidebarPanel.inert = !open;
    sidebarPanel.setAttribute("aria-hidden", String(!open));
    $(".conversation").inert = compact && open;
    const label = open ? "收起侧边栏" : "打开侧边栏";
    toggle.setAttribute("aria-expanded", String(open));
    toggle.setAttribute("aria-label", label);
    toggle.title = label;
  };
  const focusSidebarControl = () => (compactSidebarMedia.matches && sidebarIsOpen()
    ? sidebarPanel.querySelector("button:not(:disabled)") : $("#open-sidebar"))?.focus({ preventScroll: true });
  const setSidebarOpen = (open, { focus = true } = {}) => {
    if (relatedSessionView) return;
    closePopup(); closeComposerMenus();
    const opening = open && !sidebarIsOpen();
    if (opening) {
      renderSidebar();
    }
    if (compactSidebarMedia.matches) workspace.classList.toggle("sidebar-open", open);
    else setDesktopSidebarOpen(open);
    syncSidebarLayout();
    sidebarBackdropPress = null;
    if (opening && compactSidebarMedia.matches) sidebarBackdropReadyAt = performance.now() + sidebarBackdropGuardMs;
    if (focus) focusSidebarControl();
  };
  const closeSidebar = ({ focus = true } = {}) => setSidebarOpen(false, { focus });
  setCloseMobileSidebar(({ focus = true } = {}) => {
    if (!compactSidebarMedia.matches) return;
    setSidebarOpen(false, { focus });
  });
  const onSidebarMediaChange = () => {
    workspace.classList.remove("sidebar-open");
    syncSidebarLayout();
  };
  compactSidebarMedia.addEventListener("change", onSidebarMediaChange);
  setSyncRelatedSidebarLayout(syncSidebarLayout);
  setSidebarLayoutCleanup(() => {
    compactSidebarMedia.removeEventListener("change", onSidebarMediaChange);
    if (syncRelatedSidebarLayout === syncSidebarLayout) setSyncRelatedSidebarLayout(null);
  });
  syncSidebarLayout();
  $("#open-sidebar").onclick = () => setSidebarOpen(!sidebarIsOpen());
  sidebarBackdrop.onpointerdown = event => {
    // 用事件发生时间判定，慢设备排队后才派发的早期触摸也不能越过保护期。
    sidebarBackdropPress = compactSidebarMedia.matches && sidebarIsOpen()
      && event.target === sidebarBackdrop && event.isPrimary !== false && event.button === 0
      && event.timeStamp >= sidebarBackdropReadyAt
      ? { pointerId: event.pointerId, released: false } : null;
  };
  sidebarBackdrop.onpointerup = event => {
    if (!sidebarBackdropPress || sidebarBackdropPress.pointerId !== event.pointerId) return;
    sidebarBackdropPress.released = event.target === sidebarBackdrop;
  };
  sidebarBackdrop.onpointercancel = () => { sidebarBackdropPress = null; };
  sidebarBackdrop.onclick = event => {
    const press = sidebarBackdropPress;
    sidebarBackdropPress = null;
    event.preventDefault(); event.stopPropagation();
    if (!compactSidebarMedia.matches || !sidebarIsOpen() || event.target !== sidebarBackdrop
        || !press?.released || (event.pointerId != null && event.pointerId !== press.pointerId)) return;
    closeSidebar();
  };
  const searchInput = $("#sidebar-search"), searchToggle = $("#sidebar-search-toggle");
  const typeFilter = $("#sidebar-type-filter");
  typeFilter.popupClassName = "sidebar-type-popup";
  typeFilter.onclick = () => {
    renderSessionFilter(); togglePopup(typeFilter);
  };
  typeFilter.onkeydown = event => {
    if (!["ArrowDown", "ArrowUp"].includes(event.key)) return;
    event.preventDefault(); renderSessionFilter();
    openPopup(typeFilter, { edge: event.key === "ArrowUp" ? "last" : "first" });
  };
  const setSidebarSearchOpen = open => {
    closePopup();
    searchInput.hidden = !open;
    $("#new-chat").hidden = open;
    searchToggle.setAttribute("aria-expanded", String(open));
    searchToggle.setAttribute("aria-pressed", String(open));
    if (open) searchInput.focus({ preventScroll: true });
    else {
      searchInput.value = ""; setSearch(""); renderSidebar();
      searchToggle.focus({ preventScroll: true });
    }
  };
  searchToggle.onclick = () => setSidebarSearchOpen(searchInput.hidden);
  searchInput.oninput = event => { setSearch(event.target.value.trim().toLowerCase()); renderSidebar(); };
  searchInput.onkeydown = event => {
    if (event.key !== "Escape" || event.isComposing) return;
    event.preventDefault(); event.stopPropagation(); setSidebarSearchOpen(false);
  };
  $("#new-chat").onclick = () => startNewChat();
  $("#logout").onclick = async () => {
    const deviceId = session.device.id;
    if (!await confirmAction("退出", "将撤销当前浏览器的访问授权；再次进入需要重新输入访问密钥。", "退出", true) || session?.device.id !== deviceId) return;
    $("#logout").disabled = true;
    try {
      await api("/api/logout", {});
      disconnect(); savePending(null);
      setPendingNotificationOpen(null);
      await clearPrivateInputs().catch(() => {});
      setAuthenticationRequired(true);
      postPlatformMessage("logged-out");
      showLogin();
    }
    catch (error) { notice(error.message, true); if ($("#logout")) $("#logout").disabled = false; }
  };
  $("#connection-state").onclick = refreshConnectionFromIndicator;
  const toolsTrigger = $("#open-session-tools");
  toolsTrigger.append(icon("tools"));
  setSessionTools(mountSessionTools({
    workspace: $(".workspace"), trigger: $("#open-session-tools"),
    sidePanel: workspaceSidePanel,
    onFileContextMenu: openWorkspaceFileContextMenu,
    createSideThread: createDesktopSideThread,
    getContext: () => ({ deviceId: session?.device.id, threadId: selectedId, creating,
      projectId: creationProject, readOnly: threadReadOnly(), cacheScope: privateInputScope(session) }),
    cache: workspaceViewCache,
    request: (path, options, consume) => {
      const measurements = currentChannelMeasurements();
      return requestRemote(path, options, consume, measurements, origin => markMessageChannelUnavailable(origin, measurements));
    },
    beforeOpen: () => { sessionMenu?.close(); closePopup(); closeComposerMenus(); closeMobileSidebar({ focus: false }); },
    onUnauthorized: requireAuthentication,
    openImagePreview,
    openVideoPreview: (trigger, source, poster, name) => openVideoPreview(trigger, source, poster, name, prepareRemoteTunnelAccess),
    copyText
  }));
  setSessionMenu(mountSessionMenu({
    trigger: toolsTrigger,
    getContext: () => {
      const parentThreadId = relatedSessionView?.kind === "side"
        ? relatedSessionView.openedFromThreadId || relatedSessionView.parentThreadId : null;
      return { deviceId: session?.device.id, cacheScope: privateInputScope(session),
        threadId: !creating ? parentThreadId || selectedId : null, readOnly: threadReadOnly(), connected,
        subagentsEnabled: subagentsEnabled(),
        sideThreadsEnabled: sideThreadsEnabled(),
        sideThreadView: relatedSessionView?.kind === "side",
        // Claude 和 Codex 的关联列表均由服务读取，菜单打开时沿用每秒只读核对。
        relatedScope: JSON.stringify([session?.device.id, nativeConnectionVersion]),
        createPending: [...relatedThreadOperations.values()].some(record => ownsRelatedOperation(record)
          && record.parentThreadId === (parentThreadId || selectedId) && record.action === "createSideThread") };
    },
    cache: relatedMenuCache,
    onData: adoptRelatedMenu,
    load: loadRelatedThreads,
    renderSubagentIcon: item => createSubagentIcon(subagentName({ displayName: item.name })),
    openChanges: () => sessionTools?.open("changes"),
    openBrowser: () => {
      if (!remoteChannelRoutes().some(route => route.name === "FRP")) {
        notice("浏览器网络要求较高，需要在管理端开启FRP", true);
        return Promise.resolve();
      }
      return openAndroidBrowser();
    },
    openFiles: () => sessionTools?.open("files"),
    openSubagent: item => openRelatedThread(item, "subagent"),
    createSideThread: () => runRelatedOperation("createSideThread"),
    openSideThread: item => openRelatedThread(item, "side"),
    closeSideThread: item => runRelatedOperation("closeSideThread", item),
    beforeOpen: () => {
      closePopup(); closeComposerMenus(); closeMobileSidebar({ focus: false }); rememberTransientHistory();
    },
    onError: error => notice(error.message || "无法读取会话菜单", true)
  }));
  restoreRelatedOperations();
  syncRelatedSessionLayout();
  $("#appearance-button").onclick = openAppearance;
  $("#sidebar-view-toggle").onclick = () => void changeSidebar({
    target: "grouping", mode: "updated_at",
    grouping: (sidebarFilters.mode ?? sidebar.preferences.mode) === "project" ? "list" : "project"
  });
  $("#composer").onsubmit = async event => {
    event.preventDefault();
    if (threadReadOnly()) return;
    if ($("#send").dataset.action === "stop") await stopGeneration();
    else await sendMessage();
  };
  $("#send").onpointerdown = event => {
    if (event.isPrimary && event.button === 0
        && event.currentTarget.dataset.action !== "stop" && document.activeElement === $("#prompt"))
      event.preventDefault();
  };
  $("#prompt").onkeydown = event => {
    if (threadReadOnly()) { event.preventDefault(); return; }
    if (event.isComposing) return;
    handleComposerLocalPasteShortcut(event);
    if ((event.ctrlKey || event.metaKey) && !event.altKey && !$("#prompt").editHistory?.composing
        && ["z", "y"].includes(event.key.toLowerCase())) {
      event.preventDefault();
      const redo = event.key.toLowerCase() === "y" || event.shiftKey;
      $("#prompt").editHistory?.[redo ? "redo" : "undo"]();
      return;
    }
    if (!$("#skill-menu").hidden && ["ArrowDown", "ArrowUp", "Enter", "Tab"].includes(event.key) && !event.ctrlKey && !event.metaKey) {
      const entries = slashMenuEntries();
      if (entries.some(entry => entry.enabled)) {
        event.preventDefault();
        if (event.key === "ArrowDown") { moveSlashIndex(entries, 1); renderSkillMenu(); }
        else if (event.key === "ArrowUp") { moveSlashIndex(entries, -1); renderSkillMenu(); }
        else {
          const entry = entries[skillIndex];
          if (entry?.type === "compact") selectCompactCommand();
          else if (entry?.type === "goal") selectGoalCommand();
          else if (entry?.type === "promptOptimize") selectPromptOptimizationCommand();
          else if (entry?.type === "skill") selectSkill(entry.skill);
        }
        return;
      }
    }
    if (["Backspace", "Delete"].includes(event.key)
        && removeAdjacentPromptToken($("#prompt"), event.key === "Backspace" ? -1 : 1)) {
      event.preventDefault(); return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !event.isComposing) {
      event.preventDefault(); void sendMessage();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      $("#prompt").setRangeText("\n", $("#prompt").selectionStart, $("#prompt").selectionEnd, "end");
      handlePromptInput(event, { preserveCanonical: true });
    }
  };
  $("#prompt").onbeforeinput = event => {
    if (threadReadOnly()) { event.preventDefault(); return; }
    if (!event.isComposing && !$("#prompt").editHistory?.composing
        && ["historyUndo", "historyRedo"].includes(event.inputType)) {
      event.preventDefault();
      $("#prompt").editHistory?.[event.inputType === "historyUndo" ? "undo" : "redo"]();
      return;
    }
    $("#prompt").editHistory?.begin(event.inputType, false);
    const multilineInput = ["insertText", "insertFromPaste", "insertFromDrop"].includes(event.inputType);
    const insertedText = typeof event.data === "string" ? event.data
      : event.dataTransfer?.getData("text/plain") || "";
    if (!event.isComposing && multilineInput && /[\r\n]/.test(insertedText)) {
      event.preventDefault();
      const text = insertedText.replace(/\r\n?/g, "\n");
      $("#prompt").setRangeText(text, $("#prompt").selectionStart, $("#prompt").selectionEnd, "end");
      handlePromptInput(event, { preserveCanonical: true });
      return;
    }
    if (!event.isComposing && ["deleteContentBackward", "deleteContentForward"].includes(event.inputType)
        && removeAdjacentPromptToken($("#prompt"), event.inputType === "deleteContentBackward" ? -1 : 1)) {
      event.preventDefault(); return;
    }
    if (!event.isComposing && ["insertParagraph", "insertLineBreak"].includes(event.inputType)) {
      event.preventDefault();
      $("#prompt").setRangeText("\n", $("#prompt").selectionStart, $("#prompt").selectionEnd, "end");
      handlePromptInput(event, { preserveCanonical: true });
    }
  };
  $("#prompt").onpaste = event => {
    if (threadReadOnly()) { event.preventDefault(); return; }
    // 用户要求独立客户端 Ctrl+V 添加本机文件/文件夹；纯文字与截图继续走原有输入和图片处理。
    if (handleComposerLocalPaste(event)) return;
    if (handleComposerImagePaste(event)) return;
    event.preventDefault();
    const text = (event.clipboardData?.getData("text/plain") || "").replace(/\r\n?/g, "\n");
    $("#prompt").setRangeText(text, $("#prompt").selectionStart, $("#prompt").selectionEnd, "end");
    handlePromptInput(event, { preserveCanonical: true });
  };
  $("#prompt").oninput = event => handlePromptInput(event, {
    preserveCanonical: ["insertParagraph", "insertLineBreak"].includes(event.inputType)
  });
  $("#prompt").oncompositionstart = () => $("#prompt").editHistory?.startComposition();
  $("#prompt").oncompositionend = event => {
    $("#prompt").editHistory?.endComposition();
    handlePromptInput(event);
  };
  $("#prompt").onkeyup = event => {
    if (!event.isComposing && ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) void updateSkillMenu();
  };
  $("#prompt").onselect = () => void updateSkillMenu();
  document.onselectionchange = () => {
    if (document.activeElement === $("#prompt")) void updateSkillMenu();
  };
  $("#prompt").onblur = event => { if (!event.relatedTarget?.closest("#skill-menu")) closeSkillMenu(); };
  $("#prompt").onclick = () => void updateSkillMenu();
  $("#project-select").onchange = changeCreationProject;
  $("#model-button").onclick = async () => {
    if (threadReadOnly()) return;
    const opening = $("#model-settings").hidden;
    closePopup(); closeComposerMenus();
    if (!opening) return;
    setModelQuery("");
    $("#model-settings").hidden = false; $("#model-button").setAttribute("aria-expanded", "true");
    const cached = !!modelsLoadedAt;
    const menu = $("#model-settings"), owner = privateInputScope(session), contextKey = modelContextKey();
    const connectionGeneration = nativeConnectionVersion;
    const current = () => $("#model-settings") === menu && !menu.hidden && privateInputScope(session) === owner
      && modelContextKey() === contextKey && (nativeConnectionVersion) === connectionGeneration;
    if (cached) {
      renderModelMenu(); $('#model-settings [aria-checked="true"]:not(:disabled)')?.focus();
    } else menu.replaceChildren();
    // 历史读取只提供当前模型；用户点击后才加载该实例的完整选项，并明确显示加载或失败状态。
    if (!cached || isClaude()) menu.append(element("p", "popover-empty muted", "正在读取可用模型…"));
    fitComposerMenu(menu);
    try {
      await loadModels(false, { includeAvailable: isClaude() });
      if (!current()) return;
      renderModelMenu();
      if (!cached) $('#model-settings [aria-checked="true"]:not(:disabled)')?.focus();
    } catch (error) {
      if (!current()) return;
      renderModelMenu();
      menu.prepend(element("p", "popover-empty error-text", error.message));
      fitComposerMenu(menu);
    }
  };
  $("#model-settings").onkeydown = event => {
    if (event.isComposing) return;
    if (event.target === $("#model-search") && ["Home", "End"].includes(event.key)) return;
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const choices = [...$("#model-settings").querySelectorAll("button:not(:disabled)")];
    if (!choices.length) return;
    event.preventDefault();
    const current = choices.indexOf(document.activeElement);
    const index = event.key === "Home" ? 0 : event.key === "End" ? choices.length - 1
      : (current + (event.key === "ArrowDown" ? 1 : choices.length - 1)) % choices.length;
    choices[index].focus();
  };
  void loadModels().catch(() => {});
}
