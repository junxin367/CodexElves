// 在系统默认浏览器中打开会话工作区内的本机文件。

import { isAndroidContainer, openAndroidBrowser } from "../platform.js";
import { localFileReference } from "../file-reference.js";
import { notice } from "../ui/notice.js";
import { copyText } from "../ui/clipboard.js";
import { creating, pageLeaving, selectedId, session, thread } from "../core/state.js";
import { api } from "./api.js";
import { privateInputScope } from "./session/capabilities.js";
import { fileOpenInMenu } from "./session/open-in.js";
import { relatedSessionView } from "./session/related-state.js";
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

function localFileMenuOwner() {
  const scope = privateInputScope(session), threadId = currentFileBrowserThreadId();
  if (!scope || !threadId || pageLeaving || thread?.id !== selectedId) return null;
  const item = { ...thread, id: threadId };
  return { item, key: JSON.stringify([scope, selectedId, threadId, thread.cwd || "",
    thread.instanceId || "", thread.wslDistribution || "", thread.wslUser || ""]) };
}

export function localFileReferenceActions({ openFile } = {}) {
  const owner = localFileMenuOwner();
  const owns = () => !!owner && localFileMenuOwner()?.key === owner.key;
  const preview = (path, line) => {
    if (!owns()) return;
    const reference = localFileReference(path, true);
    if (!reference) return;
    path = reference.path;
    // 文件页原本按 Git 根目录处理相对路径；正文引用必须相对会话 cwd，先附着明确目录，避免子目录会话打开同名根文件。
    const cwd = owner.item.cwd?.replaceAll("\\", "/").replace(/\/+$/, "");
    const target = !/^(?:\/|[a-z]:\/)/i.test(path) && cwd
      ? `${cwd}/${path.split("/").filter(part => part !== ".").join("/")}` : path;
    return openFile?.(target, line ?? reference.line);
  };
  return { openFile: preview, getMenuOptions(path, line) {
    const opening = fileOpenInMenu(path, { getItem: () => owns() ? owner.item : null });
    // 用户要求正文文件菜单固定此顺序；Explorer 定位所点文件，复制不附加行号，旧会话/授权的操作不得借用当前身份。
    return [
      { value: "openFile", label: "打开文件", icon: "folder", disabled: !owns() || typeof openFile !== "function",
        action: () => preview(path, line) },
      { value: "revealFile", label: "在 File Explorer 打开", icon: "folderClosed", disabled: opening.disabled,
        action: () => opening.openTarget({ target: "fileManager" }) },
      { ...opening, value: "openIn", label: "打开方式", icon: "window" },
      { value: "copyPath", label: "复制路径", icon: "copy", separatorBefore: true, disabled: !owns(),
        action: () => { if (owns()) void copyText(path); } }
    ];
  } };
}
