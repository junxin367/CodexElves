// 独立客户端的工作区打开入口；目录、应用检测与启动均由远程服务独立完成。
import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { activePopup, closePopup, openPopup, renderPopup } from "../../ui/popup.js";
import { creating, nativeConnectionVersion, pageLeaving, selectedId, session, thread } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { validateRemoteReceipt } from "../../channels.js";
import { api } from "../api.js";
import { privateInputScope, threadReadOnly } from "./capabilities.js";
import { relatedSessionView } from "./related-state.js";
import { workspaceSidePanel } from "./side-panel.js";

// 用户要求补齐 IDEA、WSL 和 PowerShell；ID 须与独立服务 open_in.rs 的 TARGETS 同步，不能在前端再次过滤掉已支持应用。
const applicationIds = new Set(["vscode", "visualStudio", "sublimeText", "githubDesktop",
  "fileManager", "gitBash", "cmder", "rider", "intellij", "powershell", "wsl"]);
const applicationPaths = {
  vscode: '<path d="M17.25 2.8 7.57 11.6 3.4 8.43 1.5 9.5v5l1.9 1.08 4.17-3.18 9.68 8.8L22.5 19V5l-5.25-2.2Zm-.2 5.08v8.24L11.75 12l5.3-4.12Z"/>',
  visualStudio: '<path d="m17.34 2.5-8.8 6.7-4.42-3.42L1.5 7.1v9.8l2.62 1.32 4.42-3.42 8.8 6.7 5.16-2.06V4.56L17.34 2.5ZM4.65 9.35 7.7 12l-3.05 2.65v-5.3Zm12.5-1.82v8.94L11.28 12l5.87-4.47Z"/>',
  githubDesktop: '<path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.86c-2.78.6-3.37-1.18-3.37-1.18-.45-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.61.07-.61 1 .07 1.53 1.03 1.53 1.03.9 1.52 2.34 1.08 2.91.83.09-.64.35-1.08.64-1.33-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.98 1.03-2.68-.1-.26-.45-1.27.1-2.64 0 0 .84-.27 2.75 1.02A9.6 9.6 0 0 1 12 6.84c.85 0 1.71.11 2.51.34 1.91-1.29 2.75-1.02 2.75-1.02.55 1.37.2 2.38.1 2.64.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.69-4.57 4.94.36.31.68.92.68 1.85v2.74c0 .27.18.58.69.48A10 10 0 0 0 12 2Z"/>',
  fileManager: '<path d="M2 6a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2Z"/>'
};

export function clientOpenInContext() {
  // 用户要求仅接入 Windows 独立客户端；普通网页、Android、关联阅读页及无会话页面不新增入口。
  if (typeof window.__TAURI__?.core?.invoke !== "function" || pageLeaving || creating || !selectedId
      || thread?.id !== selectedId || !session?.capabilities?.workspaceOpenIn || threadReadOnly()
      || relatedSessionView || workspaceSidePanel) return null;
  const scope = privateInputScope(session);
  if (!scope) return null;
  // 用户要求 WSL 只显示于实际执行的 WSL 会话实例；Windows 挂载目录仍可能是 WSL 会话，不能按 cwd 的路径格式推断。
  const wslInstance = typeof thread.instanceId === "string" && thread.instanceId.includes(":wsl:")
    && typeof thread.wslDistribution === "string" && !!thread.wslDistribution;
  return { scope, threadId: selectedId,
    wslInstance,
    key: JSON.stringify([scope, selectedId, nativeConnectionVersion, thread.cwd || "",
      thread.instanceId || "", thread.wslDistribution || "", thread.wslUser || ""]) };
}

function applicationIcon(target) {
  const node = element("span", `workspace-open-in-app-icon workspace-open-in-${target || "fallback"}`);
  node.setAttribute("aria-hidden", "true");
  if (applicationPaths[target])
    node.innerHTML = `<svg viewBox="0 0 24 24" fill="currentColor">${applicationPaths[target]}</svg>`;
  else if (target === "sublimeText") node.textContent = "S";
  else if (["rider", "intellij"].includes(target)) node.append(element("span", "", target === "intellij" ? "IJ" : "RD"));
  else node.append(icon(["gitBash", "cmder", "powershell", "wsl"].includes(target) ? "terminal" : "window"));
  return node;
}

function normalizeTargets(result, context) {
  if (result?.ok !== true || typeof result.root !== "string" || !result.root
      || result.root.length > 32768 || !Array.isArray(result.targets))
    throw new Error("工作区应用列表无效，请重新打开会话。");
  const seen = new Set();
  const targets = result.targets.filter(target => target && applicationIds.has(target.target)
      && (target.target !== "wsl" || context?.wslInstance === true)
      && target.available === true && typeof target.label === "string" && target.label.length <= 80
      && target.label.trim() && !seen.has(target.target) && seen.add(target.target))
    .map(({ target, label }) => ({ target, label }));
  return { root: result.root, targets };
}

function preferenceKey(context, root) {
  // 与原逻辑一致按目录记住首选应用；仅保存应用 ID，按当前服务 Origin、设备授权和目录隔离。
  return `codex-elves-open-in-v1:${JSON.stringify([context.scope, root])}`;
}
function preferredTarget(context, result) {
  let preferred = "";
  try { preferred = localStorage.getItem(preferenceKey(context, result.root)) || ""; } catch {}
  return result.targets.find(target => target.target === preferred) || result.targets[0] || null;
}

export function mountWorkspaceOpenIn({ anchor, beforeOpen = () => {} }) {
  const group = element("div", "workspace-open-in");
  group.hidden = true; group.setAttribute("role", "group"); group.setAttribute("aria-label", "打开工作区");
  const primary = button("", "用首选应用打开工作区", () => void activate(), "workspace-open-in-primary");
  const arrow = button("", "选择打开方式", () => void showMenu(), "workspace-open-in-arrow");
  primary.type = arrow.type = "button";
  arrow.setAttribute("aria-haspopup", "menu"); arrow.setAttribute("aria-expanded", "false");
  arrow.append(icon("chevron"));
  arrow.popupClassName = "workspace-open-in-menu"; arrow.popupWidth = 178;
  // 用户要求 Open in 固定向左；首次打开、列表刷新和全局重定位均使用自身定位，保留 178px 宽度及 6px 垂直间隔。
  arrow.popupFit = fitMenu;
  group.append(primary, arrow); anchor.before(group);

  let contextKey = null, loaded = null, pending = null, opening = false, disposed = false;
  const current = () => !disposed && group.isConnected ? clientOpenInContext() : null;
  const owns = context => !!context && current()?.key === context.key;
  const ownsPopup = () => activePopup?.trigger === arrow;
  function setPrimary(target) {
    primary.replaceChildren(applicationIcon(target?.target));
    const title = target ? `用 ${target.label} 打开工作区` : "用首选应用打开工作区";
    primary.title = title; primary.setAttribute("aria-label", title);
  }
  function syncDisabled() {
    const valid = !!current();
    primary.disabled = !valid || opening || loaded?.targets.length === 0;
    arrow.disabled = !valid || opening;
    primary.setAttribute("aria-busy", String(opening));
  }
  function close() {
    if (ownsPopup()) closePopup();
  }
  function cancelRead() {
    pending?.controller.abort();
    pending = null;
  }
  async function readTargets(context, force = false) {
    if (!force && loaded && loaded.key === context.key && Date.now() - loaded.at < 5000) return loaded;
    if (pending?.key === context.key) return pending.promise;
    const request = { key: context.key, controller: new AbortController(), promise: null };
    pending = request;
    request.promise = api("/api/workspace/open-in-targets", { threadId: context.threadId },
      undefined, request.controller.signal).then(value => {
      const result = normalizeTargets(value, context);
      if (owns(context) && pending === request) {
        loaded = { ...result, key: context.key, at: Date.now() };
        setPrimary(preferredTarget(context, result)); syncDisabled();
      }
      return result;
    }).finally(() => { if (pending === request) pending = null; });
    return request.promise;
  }
  function fitMenu() {
    if (!ownsPopup()) return;
    const box = group.getBoundingClientRect(), viewport = window.visualViewport;
    const left = viewport?.offsetLeft || 0, top = viewport?.offsetTop || 0;
    const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
    if (group.hidden || !current() || box.bottom <= top || box.top >= top + height) { close(); return; }
    const popup = activePopup.node, below = Math.max(0, top + height - box.bottom - 14);
    const above = Math.max(0, box.top - top - 14);
    popup.style.width = `${Math.min(178, width - 16)}px`;
    popup.style.maxHeight = `${Math.min(360, Math.max(below, above))}px`;
    const menuHeight = popup.getBoundingClientRect().height;
    const upward = menuHeight > below && above > below;
    // 用户要求远程 Open in 固定向左展开，右边缘对齐整个按钮组；两侧保留 8px 留白，空间不足时只平移、不反向展开。
    const menuWidth = popup.getBoundingClientRect().width;
    popup.style.left = `${Math.max(left + 8, Math.min(box.right - menuWidth, left + width - menuWidth - 8))}px`;
    popup.style.top = `${upward ? Math.max(top + 8, box.top - menuHeight - 6) : box.bottom + 6}px`;
  }
  function decorateMenu() {
    if (!ownsPopup()) return;
    for (const option of activePopup.list.querySelectorAll("button[data-value]"))
      if (applicationIds.has(option.dataset.value)) option.prepend(applicationIcon(option.dataset.value));
    fitMenu();
  }
  function menuOptions(context, result) {
    return result.targets.length
      ? result.targets.map(target => ({ value: target.target, label: target.label,
        action: () => activate(target, context) }))
      : [{ label: "未检测到可用应用", disabled: true }];
  }
  async function showMenu() {
    if (ownsPopup()) { closePopup(true); return; }
    const context = current();
    if (!context || opening) return;
    beforeOpen();
    // 用户要求去掉弹出菜单顶部的 Open in 描述：只展示应用列表，保留按钮的无障碍名称。
    arrow.menuOptions = loaded?.key === context.key ? menuOptions(context, loaded)
      : [{ label: "正在加载可用应用…", disabled: true }];
    openPopup(arrow);
    decorateMenu();
    try {
      const result = await readTargets(context, true);
      if (!owns(context) || !ownsPopup()) return;
      arrow.menuOptions = menuOptions(context, result);
      renderPopup(); decorateMenu();
      const first = activePopup.list.querySelector("button:not(:disabled)");
      if (document.activeElement === arrow || !activePopup.node.contains(document.activeElement)) first?.focus();
    } catch (error) {
      if (!owns(context) || !ownsPopup() || error.name === "AbortError") return;
      arrow.menuOptions = [{ label: "重新加载应用列表", action: () => showMenu() }];
      renderPopup(); fitMenu(); notice(error.message || "无法读取工作区应用", true);
    }
  }
  async function activate(target = null, expected = null) {
    const context = expected || current();
    if (opening || !owns(context)) return;
    opening = true; close(); syncDisabled();
    let dispatching = false, timer = null, operationKey = null, requestId = null;
    try {
      // 首选按钮同样先刷新真实可用应用；异步准备后再次核对会话，旧点击不能打开切换后的会话目录。
      const result = await readTargets(context, true);
      if (!owns(context)) return;
      const chosen = target ? result.targets.find(value => value.target === target.target) : preferredTarget(context, result);
      if (!chosen) throw new Error("未找到可用的打开方式，请重新选择应用。");
      const controller = new AbortController();
      timer = setTimeout(() => controller.abort(), 45000);
      // 复用项目持久回执抵御浏览器自身的 POST 重试；结果未知时重开页面/再次点击也只核对同一请求。
      operationKey = `codex-elves-open-in-request-v1:${JSON.stringify([context.scope, context.threadId, chosen.target])}`;
      try {
        requestId = sessionStorage.getItem(operationKey);
        if (!/^[A-Za-z0-9-]{1,100}$/.test(requestId || "")) {
          requestId = randomId();
          sessionStorage.setItem(operationKey, requestId);
        }
      } catch { throw new Error("无法保留工作区打开请求，请允许客户端存储后重试。"); }
      dispatching = true;
      const response = await api("/api/workspace/open-in",
        { threadId: context.threadId, target: chosen.target }, requestId, controller.signal);
      const receipt = validateRemoteReceipt(response, requestId);
      if (receipt.state === "failed") {
        sessionStorage.removeItem(operationKey);
        throw Object.assign(new Error(receipt.result?.message || "无法打开工作区"),
          { code: receipt.result?.code || 422 });
      }
      if (receipt.state !== "completed") throw new Error("无法确认应用启动结果");
      const opened = receipt.result;
      if (opened.target !== chosen.target || typeof opened.root !== "string")
        throw new Error("无法确认应用启动结果");
      sessionStorage.removeItem(operationKey);
      if (!owns(context)) return;
      setPrimary(chosen);
      try { localStorage.setItem(preferenceKey(context, opened.root), chosen.target); }
      catch { notice("应用启动请求已提交，但首选打开方式未能保存。", true); }
      // 用户要求打开后显示前台；后台已启动但未能确认前台时明确提示，保留成功回执和首选应用，不重发请求。
      if (typeof opened.warning === "string" && opened.warning.trim()) notice(opened.warning);
    } catch (error) {
      if (!owns(context)) return;
      if (!dispatching && error.name === "AbortError") return;
      const unknown = dispatching && (error.outcomeUnknown === true
        || !error.notSubmitted && !Number.isInteger(error.code));
      if (dispatching && !unknown && (error.notSubmitted || Number.isInteger(error.code)))
        try { sessionStorage.removeItem(operationKey); } catch {}
      notice(unknown ? "未能确认应用启动结果，请先查看远程电脑，勿重复点击。"
        : error.message || "无法打开工作区", true);
    } finally {
      clearTimeout(timer); opening = false;
      if (!disposed) syncDisabled();
    }
  }
  function updateContext() {
    const context = current();
    if ((context?.key || null) === contextKey) { syncDisabled(); return; }
    contextKey = context?.key || null;
    close(); cancelRead(); loaded = null; setPrimary(null);
    group.hidden = !context; syncDisabled();
    if (context) void readTargets(context).catch(error => {
      if (owns(context) && error.name !== "AbortError") primary.title = error.message || "无法读取工作区应用";
    });
  }
  const resize = typeof ResizeObserver === "function" ? new ResizeObserver(fitMenu) : null;
  resize?.observe(anchor.parentElement);
  const visibility = () => { if (document.hidden) { close(); cancelRead(); } };
  window.addEventListener("resize", fitMenu); window.addEventListener("blur", close);
  window.visualViewport?.addEventListener("resize", fitMenu);
  document.addEventListener("visibilitychange", visibility);
  setPrimary(null); updateContext();
  return { updateContext, close, dispose() {
    if (disposed) return;
    close(); cancelRead(); disposed = true; resize?.disconnect(); group.remove();
    window.removeEventListener("resize", fitMenu); window.removeEventListener("blur", close);
    window.visualViewport?.removeEventListener("resize", fitMenu);
    document.removeEventListener("visibilitychange", visibility);
  } };
}
