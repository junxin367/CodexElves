// PC 侧边会话/子智能体面板与独立会话窗口的页面模式，以及向外层标签同步执行状态。

import { connected, creating, pageLeaving, selectedId, thread } from "../../core/state.js";
import { markVisibleThreadRead } from "../messages/user-content.js";
import { sidebarAttentionState } from "../sidebar/list-state.js";

// 用户要求 PC 子会话参照侧边会话使用右侧标签内的独立页面；外层标签已提供名称，
// 因此面板内隐藏整段顶部信息、主侧栏与返回入口，子智能体仍沿用原生只读限制。
// 文件工具在本面板内打开，避免嵌套第二个标签面板；手机普通关联会话页不使用此标记。
export const workspaceSidePanel = location.pathname === "/side-thread";
// 按页面地址标记独立会话窗口或 PC 侧边面板模式，样式据此隐藏侧栏等区域。
export function applyWorkspaceWindowMode() {
  // 用户要求独立窗口保留完整会话/标签页，仅隐藏左侧列表；不能复用会禁用顶部与标签功能的侧边 iframe 模式。
  if (!workspaceSidePanel && new URLSearchParams(location.search).get("sessionWindow") === "1")
    document.documentElement.dataset.sessionWindow = "true";
  if (workspaceSidePanel) {
    document.documentElement.dataset.workspaceSidePanel = "true";
    document.documentElement.dataset.workspaceSidePanelVisible = "false";
  }
}
// 用户要求独立会话窗口在任务栏显示会话标题（如会话窗口显示会话标题、文件预览窗口显示文件标题）：
// 页面标题跟随顶部会话标题，Windows 客户端监听页面标题变化后更新原生窗口标题，不向网页开放窗口命令。
// 顶部标题在多处更新，且 shell() 会重建整个 #app，因此只监听 #app 子节点替换并重新绑定标题元素；
// 默认标题“远程会话”及无标题时回退页面原标题。文件预览页由 workspace-preview.js 自行设置文件标题。
export function installSessionWindowTitleSync() {
  if (document.documentElement.dataset.sessionWindow !== "true") return;
  const host = document.getElementById("app");
  if (!host) return;
  const fallback = document.title;
  let heading = null, applied = fallback;
  const sync = () => {
    const name = (heading?.textContent || "").trim();
    const next = name && name !== "远程会话" ? `${name} · CodexElves` : fallback;
    if (next !== applied) document.title = applied = next;
  };
  const headingObserver = new MutationObserver(sync);
  const bind = () => {
    const current = document.getElementById("thread-title");
    if (current !== heading) {
      headingObserver.disconnect();
      heading = current;
      if (heading) headingObserver.observe(heading, { childList: true, characterData: true, subtree: true });
    }
    sync();
  };
  new MutationObserver(bind).observe(host, { childList: true });
  bind();
}
let workspaceSidePanelStateSignature = "";
export function syncWorkspaceSidePanelState(force = false) {
  if (!workspaceSidePanel || window.parent === window || creating || !selectedId || thread?.id !== selectedId) return;
  const status = thread.status?.status || thread.status;
  const type = typeof status === "string" ? status : status?.type;
  // 用户要求侧边标签显示真实执行状态：沿用原生状态及等待判断，不把发送中、排队或待确认占位当成执行中。
  const running = connected && !pageLeaving && ["active", "inProgress", "running"].includes(type)
    && sidebarAttentionState(thread) !== "waiting";
  const signature = JSON.stringify([selectedId, running]);
  if (!force && signature === workspaceSidePanelStateSignature) return;
  workspaceSidePanelStateSignature = signature;
  window.parent.postMessage({ type: "codex-elves-side-panel-state", threadId: selectedId, running }, location.origin);
}
// PC 侧边面板接收外层标签的显隐通知，只对实际显示的标签确认已读。
export function installSidePanelMessageListener() {
  window.addEventListener("message", event => {
    if (!workspaceSidePanel || event.source !== window.parent || event.origin !== location.origin
        || event.data?.type !== "codex-elves-side-panel-visibility" || typeof event.data.visible !== "boolean") return;
    // iframe 的 document.hidden 不随标签隐藏改变；只对实际显示的侧边标签确认已读，后台标签继续保留未读状态。
    document.documentElement.dataset.workspaceSidePanelVisible = String(event.data.visible);
    syncWorkspaceSidePanelState(true);
    if (event.data.visible) void markVisibleThreadRead().catch(() => {});
  });
}
