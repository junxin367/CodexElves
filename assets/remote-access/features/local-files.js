// 在系统默认浏览器中打开会话工作区内的本机文件。

import { isAndroidContainer, openAndroidBrowser } from "../platform.js";
import { notice } from "../ui/notice.js";
import { creating, selectedId } from "../core/state.js";
import { api } from "./api.js";
import { relatedSessionView } from "./session/related.js";
import { workspaceSidePanel } from "./session/side-panel.js";

function currentFileBrowserThreadId() {
  const parentThreadId = relatedSessionView?.kind === "side"
    ? relatedSessionView.openedFromThreadId || relatedSessionView.parentThreadId : null;
  return !creating ? parentThreadId || selectedId : null;
}
function localFileBrowserUrl(path) {
  const threadId = currentFileBrowserThreadId();
  if (!threadId || typeof path !== "string" || !path.trim()) {
    throw new Error("当前会话没有可用的工作区文件");
  }
  const normalized = path.trim().replaceAll("\\", "/");
  const segments = normalized.split("/").filter(Boolean);
  if (!segments.length || segments.some(segment => segment === "." || segment === "..")) {
    throw new Error("文件路径无效");
  }
  const url = new URL(location.href);
  // Linux/WSL 绝对路径的前导斜杠也须保留在路由参数中；否则 /home/... 会被当成项目内相对路径。
  const absolutePrefix = normalized.startsWith("//") ? "%2F%2F" : normalized.startsWith("/")
    && !/^\/[a-z]:\//i.test(normalized) ? "%2F" : "";
  url.pathname = `/api/workspace/view/${encodeURIComponent(threadId)}/${
    absolutePrefix + segments.map(segment => encodeURIComponent(segment)).join("/")
  }`;
  url.search = "";
  url.hash = "";
  return url.href;
}
export async function openLocalFileInBrowser(path) {
  try {
    // 用户要求 PC 的“浏览器”直接打开原文件；由已授权本地服务调用默认浏览器，不打开内置窗口或文件 HTTP 地址。
    if (!isAndroidContainer() && (workspaceSidePanel || typeof window.__TAURI__?.core?.invoke === "function"
        || matchMedia("(min-width:761px)").matches)) {
      const threadId = currentFileBrowserThreadId();
      if (!threadId || typeof path !== "string" || !path.trim())
        throw new Error("当前会话没有可用的工作区文件");
      await api("/api/workspace/open-browser", { threadId, path });
      return;
    }
    const url = localFileBrowserUrl(path);
    if (isAndroidContainer()) {
      return openAndroidBrowser(url).catch(error => notice(error.message || "无法打开浏览器", true));
    }
    const popup = window.open(url, "_blank", "noopener,noreferrer");
    if (!popup) notice("浏览器窗口被阻止，请允许打开新窗口后重试。", true);
  } catch (error) {
    notice(error.message || "无法打开本地文件", true);
  }
}
// Markdown 中的本机文件链接通过页面事件交给系统默认浏览器打开。
export function installLocalFileOpenListener() {
  window.addEventListener("codex-elves-open-local-file", event => {
    openLocalFileInBrowser(event.detail?.path);
  });
}
