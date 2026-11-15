// 页面骨架的共享引用：会话菜单、会话工具、输入区滚动容器、侧栏布局回调与就绪通知；由 shell() 创建后写入，其他模块只读取。

let appReadySent = false;
export function announceAppReady() {
  if (appReadySent) return;
  appReadySent = true;
  window.dispatchEvent(new Event("codex-elves-app-ready"));
}
export let messagesResizeObserver = null;
export function setMessagesResizeObserver(value) { return messagesResizeObserver = value; }
export let composerScroll = null;
export function setComposerScroll(value) { return composerScroll = value; }
export let desktopSidebarOpen = true, sidebarLayoutCleanup = null, closeMobileSidebar = () => {};
export function setDesktopSidebarOpen(value) { return desktopSidebarOpen = value; }
export function setSidebarLayoutCleanup(value) { return sidebarLayoutCleanup = value; }
export function setCloseMobileSidebar(value) { return closeMobileSidebar = value; }
export let sessionTools = null;
export function setSessionTools(value) { return sessionTools = value; }
export let sessionMenu = null, syncRelatedSidebarLayout = null;
export function setSyncRelatedSidebarLayout(value) { return syncRelatedSidebarLayout = value; }
export function setSessionMenu(value) { return sessionMenu = value; }
export let workspaceOpenIn = null;
export function setWorkspaceOpenIn(value) { return workspaceOpenIn = value; }
