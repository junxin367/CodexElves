// 侧栏会话操作：行菜单与长按、侧栏视图偏好、重命名、工作区文件菜单与独立窗口；删除会话在 thread-deletion.js，删除记录在 deletion-state.js。

import { isAndroidContainer } from "../../platform.js";
import { normalizeWorkspacePreview, openWorkspacePreviewWindow } from "../../workspace-preview.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { closePopup, openPopup, togglePopup } from "../../ui/popup.js";
import { closeComposerMenus, loadSidebar, mutate, renderSidebar, saveSharedSettings } from "../../core/actions.js";
import { connected, selectedId, session, sidebar, thread } from "../../core/state.js";
import { recoveryGeneration } from "../composer/drafts.js";
import { normalizeRelatedView, privateInputScope, validRelatedThreadId } from "../session/capabilities.js";
import { relatedThreadViews } from "../session/related-state.js";
import { cacheDisplayData, sidebarSnapshot } from "../session/thread-cache.js";
import { threadDeleted, threadDeletionLocked } from "./deletion-state.js";
import { sharedSettingsState, subscribeSharedSettings } from "../settings/shared-state.js";

export let sidebarBusy = false;
export function setSidebarBusy(value) { return sidebarBusy = value; }
export let sidebarFilters = { mode: null, chats: null, projects: null, pinned: null, showPinned: false };
export let sidebarPreferenceScope = null;
export function setSidebarPreferenceScope(value) { return sidebarPreferenceScope = value; }
// 侧栏偏好与其他非外观设置共用服务快照，不再按设备或浏览器存储。
export function installSidebarPreferenceSync() {
  subscribeSharedSettings(() => {
    sidebarPreferenceScope = null;
    restoreSidebarPreferences();
    if ($("#sidebar-content")) renderSidebar();
  });
}
export function rowMenu(id, label, options) {
  const node = button("", label, () => togglePopup(node), "row-menu-button");
  node.id = id; node.append(icon("more")); node.menuOptions = options;
  node.setAttribute("aria-haspopup", "menu"); node.setAttribute("aria-expanded", "false");
  node.disabled = sidebarBusy || !connected;
  node.onkeydown = event => {
    if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); openPopup(node, { edge: event.key === "ArrowUp" ? "last" : "first" }); }
  };
  return node;
}
let sidebarLongPress = null;
export function bindSidebarRowMenu(container, menu) {
  // 刷新侧栏后仍由原手势消费释放事件，不把它误当成一次新的点击。
  if (sidebarLongPress?.rowId === menu.id) Object.assign(sidebarLongPress, { container, menu });
  container.addEventListener("pointerdown", event => {
    if (!event.isPrimary || event.button !== 0 || !["touch", "pen"].includes(event.pointerType)
        || event.target.closest(".row-menu-button") || menu.disabled) return;
    if (sidebarLongPress && !sidebarLongPress.ended && sidebarLongPress.pointerId !== event.pointerId) return;
    sidebarLongPress?.dispose();
    const controller = new AbortController(), startX = event.clientX, startY = event.clientY;
    const press = { container, menu, rowId: menu.id, pointerId: event.pointerId,
      opened: false, cancelled: false, ended: false, timer: null };
    press.dispose = () => {
      clearTimeout(press.timer); controller.abort();
      if (sidebarLongPress === press) sidebarLongPress = null;
    };
    const invalidate = () => { press.cancelled = true; clearTimeout(press.timer); };
    press.open = position => {
      clearTimeout(press.timer);
      if (press.opened || press.cancelled || press.menu.disabled || !press.menu.isConnected) return;
      press.opened = true;
      openPopup(press.menu, { position });
    };
    sidebarLongPress = press;
    const finish = pointerEvent => {
      if (pointerEvent.pointerId !== press.pointerId) return;
      clearTimeout(press.timer); press.ended = true;
      // 浏览器可能先用 pointercancel 接管长按，再派发 contextmenu。
      // 取消本地定时器，但仍由 contextmenu 自身决定是否打开；滑动和多指保持取消。
      if (press.opened) {
        pointerEvent.preventDefault();
        pointerEvent.stopPropagation();
      }
    };
    window.addEventListener("pointerdown", pointerEvent => {
      if (!press.ended && pointerEvent.pointerId !== press.pointerId
          && ["touch", "pen"].includes(pointerEvent.pointerType)) invalidate();
      else press.dispose();
    }, { capture: true, signal: controller.signal });
    window.addEventListener("pointermove", pointerEvent => {
      if (pointerEvent.pointerId === press.pointerId
          && Math.hypot(pointerEvent.clientX - startX, pointerEvent.clientY - startY) > 10) invalidate();
    }, { capture: true, signal: controller.signal });
    window.addEventListener("pointerup", finish, { capture: true, signal: controller.signal });
    window.addEventListener("pointercancel", finish, { capture: true, signal: controller.signal });
    const isRowEvent = pointerEvent => press.container.contains(pointerEvent.target)
      || pointerEvent.target.closest(".thread-row")?.querySelector(".row-menu-button")?.id === press.rowId;
    window.addEventListener("mousedown", pointerEvent => {
      if (press.opened && isRowEvent(pointerEvent)) pointerEvent.preventDefault();
    }, { capture: true, signal: controller.signal });
    window.addEventListener("click", pointerEvent => {
      if (!press.opened || !isRowEvent(pointerEvent)) return;
      pointerEvent.preventDefault(); pointerEvent.stopImmediatePropagation();
    }, { capture: true, signal: controller.signal });
    window.addEventListener("keydown", press.dispose, { capture: true, signal: controller.signal });
    window.addEventListener("blur", press.dispose, { signal: controller.signal });
    window.addEventListener("scroll", () => { if (!press.opened) invalidate(); },
      { capture: true, signal: controller.signal });
    press.timer = setTimeout(() => press.open({ x: startX, y: startY }), 550);
  });
  container.oncontextmenu = event => {
    event.preventDefault();
    const position = event.clientX || event.clientY ? { x: event.clientX, y: event.clientY } : null;
    if (sidebarLongPress?.rowId === menu.id) sidebarLongPress.open(position);
    else openPopup(menu, { position });
  };
  container.addEventListener("keydown", event => {
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
    event.preventDefault(); sidebarLongPress?.dispose(); openPopup(menu);
  });
}
export function filterSideThreadSidebar(value) {
  if (!value) return;
  const sideIds = new Set(value.sideThreadIds || []);
  for (const [id, view] of relatedThreadViews)
    if (normalizeRelatedView(view, id)?.kind === "side") sideIds.add(id);
  value.sideThreadIds = [...sideIds];
  const keep = item => !sideIds.has(item.id);
  value.recent = value.recent.filter(keep);
  value.pinnedThreads = value.pinnedThreads.filter(keep);
  for (const project of value.projects) project.threads = project.threads.filter(keep);
}
export function restoreSidebarPreferences() {
  const scope = privateInputScope(session);
  if (scope === sidebarPreferenceScope) return;
  sidebarPreferenceScope = scope;
  sidebarFilters = { mode: null, chats: null, projects: null, pinned: null, showPinned: false };
  if (!scope) return;
  if (sharedSettingsState.scope === scope && sharedSettingsState.ready)
    Object.assign(sidebarFilters, sharedSettingsState.settings.sidebar);
}
function saveSidebarPreferences(patch) {
  const scope = privateInputScope(session);
  if (!scope || !sidebar) return;
  restoreSidebarPreferences();
  return saveSharedSettings({ sidebar: patch }).catch(error => {
    if (scope === privateInputScope(session)) notice(error.message, true);
  });
}
function changeSidebarView(mode) {
  if (!["project", "list"].includes(mode)) return;
  return saveSidebarPreferences({ mode });
}
export function setRecentPinnedVisible(value) {
  if (typeof value !== "boolean") return;
  // 用户要求非外观设置跨端共享；只改分组显隐，不派发会话置顶操作。
  return saveSidebarPreferences({ showPinned: value });
}
export async function changeSidebar(payload) {
  if (payload.target === "grouping") return changeSidebarView(payload.grouping);
  const filterKey = ["chats", "projects", "pinned"].includes(payload.target) ? payload.target : null;
  if (filterKey) {
    if (["manual", "updated_at"].includes(payload.mode))
      saveSidebarPreferences({ [filterKey]: payload.mode });
    return;
  }
  if (!payload.threadId && !payload.projectId) return;
  if (!session || sidebarBusy || payload.threadId && (threadDeletionLocked(payload.threadId) || threadDeleted(payload.threadId))) return;
  const deviceId = session.device.id;
  const focus = $("#sidebar-content").contains(document.activeElement) ? document.activeElement : null;
  const focusedThread = focus?.closest(".thread-row")?.dataset.threadId;
  if (session.readOnly || !connected) return;
  sidebarBusy = true; renderSidebar();
  try { await mutate(payload.threadId ? "pinThread" : "pinProject", payload); await loadSidebar(null, { fresh: true }); }
  catch (error) {
    notice(error.message, true);
  }
  finally {
    if (session?.device.id === deviceId) {
      sidebarBusy = false; renderSidebar();
      if (focus && document.activeElement === document.body) {
        const restored = document.getElementById(focus.id) || [...$("#sidebar-content").querySelectorAll(".thread-row")]
          .find(item => item.dataset.threadId === focusedThread)?.querySelector(focus.classList.contains("row-menu-button") ? ".row-menu-button" : ".thread-link");
        restored?.focus({ preventScroll: true });
      }
    }
  }
}
export function openWorkspaceFileContextMenu(event, preview, context) {
  if (!independentThreadWindowAvailable()) return;
  const target = normalizeWorkspacePreview({ ...preview, ...(context.threadId ? { threadId: context.threadId }
    : context.creating && context.projectId ? { projectId: context.projectId } : {}) });
  const trigger = event.currentTarget;
  if (!target || !trigger?.isConnected) return;
  event.preventDefault(); event.stopPropagation();
  trigger.setAttribute("aria-label", trigger.getAttribute("aria-label") || trigger.title || preview.path);
  trigger.setAttribute("aria-haspopup", "menu");
  trigger.menuOptions = [{ value: "openPreviewWindow", label: "在独立窗口中打开", icon: "window", action: () => {
    // 文件菜单保存原会话/项目身份；切换页面后不能借用当前会话，授权改变后不再打开旧内容。
    if (context.deviceId !== session?.device.id || context.cacheScope !== privateInputScope(session)) return;
    openWorkspacePreviewWindow(target, message => notice(message, true));
  } }];
  openPopup(trigger, { position: event.clientX || event.clientY ? { x: event.clientX, y: event.clientY } : null });
}
export function independentThreadWindowAvailable() {
  return !isAndroidContainer() && matchMedia("(min-width:761px)").matches;
}
export function openIndependentThreadWindow(item) {
  if (!independentThreadWindowAvailable() || !validRelatedThreadId(item?.id)) return;
  try {
    const bridge = window.__CODEX_ELVES_SESSION_WINDOWS__;
    if (typeof bridge?.open === "function") {
      void Promise.resolve(bridge.open(item.id)).catch(error => notice(String(error?.message || error || "无法打开独立窗口"), true));
      return;
    }
    if (typeof window.__TAURI__?.core?.invoke === "function") {
      notice("当前客户端版本尚不支持独立会话窗口，请更新客户端。", true); return;
    }
    // PC 网页使用浏览器独立窗口；只传当前来源和会话 ID，不复制访问密钥，也不切换原页面。
    const url = new URL("/", location.origin);
    url.search = "sessionWindow=1"; url.hash = `thread=${encodeURIComponent(item.id)}`;
    const popup = window.open(url.href, "_blank", "popup,width=1280,height=860");
    if (popup) popup.opener = null;
    else notice("独立窗口被浏览器阻止，请允许弹出窗口后重试。", true);
  } catch (error) {
    notice(error.message || "无法打开独立窗口", true);
  }
}
export function renameSidebarThread(item) {
  if (!session || session.readOnly || !connected || sidebarBusy || !item?.id
      || threadDeletionLocked(item.id) || threadDeleted(item.id) || $("#rename-thread-dialog")) return;
  closePopup(); closeComposerMenus();
  const trigger = document.activeElement, scope = privateInputScope(session), generation = recoveryGeneration;
  const currentAuthorization = () => session && privateInputScope(session) === scope && recoveryGeneration === generation;
  const dialog = element("dialog", "confirmation-dialog thread-rename-dialog");
  dialog.id = "rename-thread-dialog";
  dialog.setAttribute("aria-labelledby", "thread-rename-title");
  const form = element("form");
  const heading = element("h2", "", "重命名会话"); heading.id = "thread-rename-title";
  const field = element("label", "thread-rename-field", "会话名称");
  const input = element("input");
  input.id = "thread-rename-input"; input.type = "text"; input.value = item.title || "";
  input.maxLength = 200; input.required = true; input.autocomplete = "off";
  input.setAttribute("aria-describedby", "thread-rename-error");
  field.append(input);
  const error = element("p", "thread-rename-error");
  error.id = "thread-rename-error"; error.hidden = true; error.setAttribute("role", "alert");
  const actions = element("div", "confirmation-actions");
  const cancel = button("取消", "取消重命名", () => dialog.close("cancel"));
  const save = element("button", "primary", "保存"); save.type = "submit";
  actions.append(cancel, save); form.append(heading, field, error, actions); dialog.append(form);
  let saving = false;
  input.oninput = () => { error.hidden = true; input.removeAttribute("aria-invalid"); };
  // 用户要求从左侧菜单弹窗改名；沿用后端 1–200 字符限制，只有成功回执才能更新各列表和当前标题。
  form.onsubmit = async event => {
    event.preventDefault();
    if (saving) return;
    const title = input.value.trim();
    const showError = message => { error.textContent = message; error.hidden = false; };
    if (!title || title.length > 200) {
      input.setAttribute("aria-invalid", "true"); showError("会话名称需为 1–200 个字符。"); input.focus(); return;
    }
    if (!currentAuthorization() || session.readOnly || !connected || threadDeletionLocked(item.id) || threadDeleted(item.id)) {
      showError("当前无法修改此会话名称，请检查连接和授权后重试。"); return;
    }
    if (title === item.title) { dialog.close("unchanged"); return; }
    saving = true; error.hidden = true; input.disabled = true; save.disabled = true; cancel.disabled = true;
    save.textContent = "保存中…"; form.setAttribute("aria-busy", "true");
    try {
      const result = await mutate("rename", { threadId: item.id, title });
      if (!currentAuthorization()) { dialog.close("cancel"); return; }
      if (result?.ok !== true) throw new Error("会话名称尚未确认保存，请刷新后核对。");
      const rows = [...(sidebar?.recent || []), ...(sidebar?.pinnedThreads || []),
        ...(sidebar?.projects || []).flatMap(project => project.threads || [])];
      for (const row of rows) if (row.id === item.id) row.title = title;
      if (thread?.id === item.id) thread.title = title;
      if (selectedId === item.id) {
        const heading = $("#thread-title");
        if (heading) heading.textContent = title;
      }
      if (sidebar) cacheDisplayData("sidebar", "main", sidebarSnapshot(sidebar));
      renderSidebar(); dialog.close("saved");
      void loadSidebar(null, { fresh: true }).catch(failure => {
        if (currentAuthorization()) notice(`名称已保存，列表刷新失败：${failure.message}`, true);
      });
    } catch (failure) {
      if (currentAuthorization() && dialog.open) showError(failure.message || "重命名失败，请重试。");
    } finally {
      saving = false; input.disabled = false; save.disabled = false; cancel.disabled = false;
      save.textContent = "保存"; form.removeAttribute("aria-busy");
      if (dialog.open) input.focus();
    }
  };
  dialog.oncancel = event => { if (saving) event.preventDefault(); };
  dialog.onclose = () => {
    dialog.remove(); document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger?.isConnected && document.activeElement === document.body) trigger.focus({ preventScroll: true });
  };
  document.body.append(dialog); document.body.classList.add("modal-open");
  dialog.showModal(); input.focus(); input.select();
}
