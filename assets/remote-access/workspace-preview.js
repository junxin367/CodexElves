import { activateRemoteChannels, clearRemoteChannels, requestRemote } from "./channels.js";
import { safeInlineImageSource, fitMediaPreview, bindMediaPreviewDismiss } from "./attachments.js";
import { mountWorkspacePreview } from "./session-tools.js";

const appearanceKey = "codex-elves-preview-appearance";
const colorTokens = new Set(["bg", "text", "accent", "on-accent", "muted", "side", "code", "hover", "line",
  "composer-bg", "composer-hover", "control-bg", "control-hover", "menu-divider", "tooltip-bg", "tooltip-text", "danger", "green"]);
const sizeTokens = new Set(["font-size", "code-font-size", "file-reading-font-size"]);
const fontTokens = new Set(["ui-font-family", "code-font-family", "ui-font-weight", "code-font-weight"]);

export function normalizeWorkspacePreview(value) {
  if (!value || typeof value !== "object" || !["file", "change"].includes(value.kind)
      || typeof value.path !== "string" || !value.path || value.path.length > 32768 || /[\u0000-\u001f\u007f]/u.test(value.path)) return null;
  const thread = typeof value.threadId === "string" && /^[A-Za-z0-9_-]{1,120}$/.test(value.threadId);
  const project = typeof value.projectId === "string" && value.projectId.length > 0 && value.projectId.length <= 4096
    && !/[\u0000-\u001f\u007f]/u.test(value.projectId);
  if (thread === project || value.threadId != null && !thread || value.projectId != null && !project) return null;
  const result = { kind: value.kind, path: value.path, ...(thread ? { threadId: value.threadId } : { projectId: value.projectId }) };
  if (value.line != null) {
    if (!Number.isSafeInteger(value.line) || value.line < 1 || value.line > 4294967295) return null;
    result.line = value.line;
  }
  if (value.kind === "change") {
    const repository = value.repository ?? "";
    if (!["staged", "unstaged", "untracked"].includes(value.section) || typeof repository !== "string"
        || repository.length > 16384 || /[\\:\u0000-\u001f\u007f]/u.test(repository)
        || repository && repository.split("/").some(part => !part || [".", "..", ".git"].includes(part.toLowerCase()))) return null;
    Object.assign(result, { section: value.section, repository });
  } else if (value.section != null || value.repository != null) return null;
  return result;
}

export function workspacePreviewUrl(origin, value) {
  const target = normalizeWorkspacePreview(value);
  if (!target) throw new Error("文件预览目标无效。");
  const url = new URL("/file-preview.html", origin);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) throw new Error("文件预览服务地址无效。");
  url.hash = new URLSearchParams(Object.entries(target).map(([key, value]) => [key, String(value)])).toString();
  return url.href;
}

function publishAppearance() {
  const root = document.documentElement, style = getComputedStyle(root);
  const values = Object.fromEntries([...colorTokens, ...sizeTokens, ...fontTokens]
    .map(name => [name, style.getPropertyValue(`--${name}`).trim()]));
  try {
    const snapshot = JSON.stringify({ theme: root.dataset.theme, values });
    if (localStorage.getItem(appearanceKey) !== snapshot) localStorage.setItem(appearanceKey, snapshot);
  } catch { /* 不允许保存外观时，独立预览仍可使用默认主题。 */ }
  if (!publishAppearance.observer && typeof MutationObserver === "function") {
    // 只在首次打开文件窗口后同步已经计算好的主题变量；不复制配色公式，也不把大背景图或访问凭据写入此记录。
    publishAppearance.observer = new MutationObserver(publishAppearance);
    publishAppearance.observer.observe(root, { attributes: true, attributeFilter: ["style", "data-theme"] });
  }
}

export function openWorkspacePreviewWindow(value, onError = () => {}) {
  try {
    const target = normalizeWorkspacePreview(value);
    if (!target) throw new Error("文件预览目标无效。");
    publishAppearance();
    const bridge = window.__CODEX_ELVES_SESSION_WINDOWS__;
    if (typeof bridge?.openPreview === "function") {
      void Promise.resolve(bridge.openPreview(target)).catch(error => onError(String(error?.message || error)));
      return;
    }
    if (typeof window.__TAURI__?.core?.invoke === "function")
      throw new Error("当前客户端版本尚不支持独立文件预览，请更新客户端。");
    const popup = window.open(workspacePreviewUrl(location.origin, target), "_blank", "popup,width=1280,height=860");
    if (!popup) throw new Error("预览窗口被浏览器阻止，请允许弹出窗口后重试。");
    popup.opener = null;
  } catch (error) { onError(error.message || "无法打开文件预览窗口。"); }
}

function applyAppearance() {
  try {
    const snapshot = JSON.parse(localStorage.getItem(appearanceKey) || "null");
    if (!snapshot || !["light", "dark"].includes(snapshot.theme)) return;
    const root = document.documentElement;
    root.dataset.theme = snapshot.theme; root.style.colorScheme = snapshot.theme;
    for (const [name, value] of Object.entries(snapshot.values || {})) {
      if (typeof value !== "string" || value.length > 512 || /[;{}<>\u0000-\u001f]/u.test(value)) continue;
      const valid = colorTokens.has(name) ? CSS.supports("color", value)
        : sizeTokens.has(name) ? /^\d+(?:\.\d+)?px$/.test(value) && parseFloat(value) >= 6 && parseFloat(value) <= 24
        : fontTokens.has(name) && CSS.supports(name.endsWith("weight") ? "font-weight" : "font-family", value);
      if (valid) root.style.setProperty(`--${name}`, value);
    }
  } catch { /* 损坏的显示缓存不影响授权与文件读取。 */ }
}

function previewImage(link, image) {
  const source = safeInlineImageSource(image.originalImageSource || link.getAttribute("href") || image.src, { allowSvg: true });
  if (!source) return;
  const dialog = document.createElement("dialog"); dialog.className = "image-preview-dialog";
  dialog.id = "image-preview-dialog";
  dialog.setAttribute("aria-label", image.alt || "图片预览");
  const panel = document.createElement("figure"); panel.className = "image-preview-panel";
  const close = document.createElement("button"); close.type = "button"; close.className = "image-preview-close";
  close.textContent = "×"; close.setAttribute("aria-label", "关闭图片预览"); close.onclick = () => dialog.close();
  const viewport = document.createElement("div"); viewport.className = "standalone-preview-image";
  const picture = document.createElement("img"); picture.className = "image-preview-image"; picture.alt = image.alt || "";
  viewport.append(picture); panel.append(close, viewport); dialog.append(panel); document.body.append(dialog);
  const layout = fitMediaPreview(dialog, panel, viewport, null, { fillAvailable: true });
  const disposeDismiss = bindMediaPreviewDismiss(dialog, picture, viewport);
  dialog.onclose = () => { disposeDismiss(); layout.dispose(); dialog.remove(); };
  picture.onload = () => layout.resize(); picture.src = source; dialog.showModal(); layout.resize(); close.focus();
}

async function copyPreviewText(text) {
  let message = "已复制", failed = false;
  try {
    if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
    else {
      const field = document.createElement("textarea");
      field.value = text; field.readOnly = true; field.style.position = "fixed"; field.style.opacity = "0";
      document.body.append(field);
      try { field.select(); if (!document.execCommand("copy")) throw new Error("copy failed"); }
      finally { field.remove(); }
    }
  } catch { message = "无法复制，请手动选择内容。"; failed = true; }
  const notice = document.querySelector("#notice");
  if (notice) {
    notice.textContent = message; notice.className = `file-preview-notice visible${failed ? " error" : ""}`;
    clearTimeout(copyPreviewText.timer);
    copyPreviewText.timer = setTimeout(() => { notice.className = "file-preview-notice"; }, 6500);
  }
}

function startPreviewPage() {
  let renderer = null, request = null, generation = 0;
  const host = document.querySelector("#file-preview");
  const params = new URLSearchParams(location.hash.slice(1)), raw = Object.fromEntries(params);
  if (raw.line != null) raw.line = /^\d+$/.test(raw.line) ? Number(raw.line) : NaN;
  const target = normalizeWorkspacePreview(raw);
  const showError = message => {
    renderer?.dispose(); renderer = null; clearRemoteChannels();
    const status = document.createElement("p"); status.className = "session-tools-status error-text";
    status.setAttribute("role", "status"); status.textContent = message;
    const retry = document.createElement("button"); retry.type = "button"; retry.textContent = "重试";
    retry.onclick = () => void load(); host.replaceChildren(status, retry);
  };
  async function load() {
    const version = ++generation; request?.abort(); renderer?.dispose(); renderer = null; clearRemoteChannels();
    host.textContent = "正在读取文件预览…";
    request = new AbortController();
    const controller = request, timeout = setTimeout(() => controller.abort(), 15000);
    try {
      if (!target) throw new Error("文件预览参数无效，请从文件或变更列表重新打开。");
      const session = await requestRemote("/api/session", { credentials: "same-origin", signal: controller.signal }, async response => {
        const data = await response.json();
        if (!response.ok) throw new Error(response.status === 401
          ? "访问授权已失效，请在原会话窗口重新登录后重试。" : data.message || "无法连接远程服务。");
        return data;
      });
      if (version !== generation || controller.signal.aborted) return;
      if (typeof session.device?.id !== "string") throw new Error("远程授权响应无效。");
      activateRemoteChannels(session.device.id); host.replaceChildren();
      const context = { deviceId: session.device.id, cacheScope: JSON.stringify([session.device.id, session.device.createdAtMs ?? null]),
        readOnly: !!session.readOnly, ...(target.threadId ? { threadId: target.threadId } : { creating: true, projectId: target.projectId }) };
      // 此页仅执行现有文件读取接口，不启动会话、轮询聊天、恢复输入回执或复制主窗口草稿。
      renderer = mountWorkspacePreview({ workspace: host, getContext: () => context, openImagePreview: previewImage,
        onUnauthorized: () => showError("访问授权已失效，请在原会话窗口重新登录后重试。"),
        copyText: copyPreviewText }, target);
      document.title = `${target.path.split(/[\\/]/).at(-1)}${target.kind === "change" ? " · 变更" : ""} · CodexElves`;
    } catch (error) {
      if (version === generation) showError(controller.signal.aborted ? "读取超时，请检查网络后重试。" : error.message);
    } finally { clearTimeout(timeout); }
  }
  window.addEventListener("pagehide", () => { ++generation; request?.abort(); renderer?.dispose(); renderer = null; clearRemoteChannels(); });
  window.addEventListener("pageshow", event => { if (event.persisted) void load(); });
  window.addEventListener("storage", event => { if (event.key === appearanceKey) applyAppearance(); });
  applyAppearance(); void load();
}

if (typeof location !== "undefined" && location.pathname === "/file-preview.html") startPreviewPage();
