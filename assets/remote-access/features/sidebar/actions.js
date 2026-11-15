// 侧栏会话操作：行菜单与长按、删除会话及其回执核对、侧栏视图偏好、重命名与独立窗口。

import { isAndroidContainer } from "../../platform.js";
import { createSidebarPreferenceStore } from "../../recovery-store.js";
import { normalizeWorkspacePreview, openWorkspacePreviewWindow } from "../../workspace-preview.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { closePopup, openPopup, togglePopup } from "../../ui/popup.js";
import { busy, connected, selectedId, session, sidebar, stopping, thread } from "../../core/state.js";
import { readCache, recoveryStore, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import {
  approvalDrafts, approvalReplies, blockingRequestStates, questionDrafts, questionReplyReceipts,
  renderApprovals
} from "../approvals.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import { deleteDraft, persistQueuedEdits, recoveryGeneration, recoveryScope } from "../composer/drafts.js";
import { cancelPromptOptimization, promptOptimizations } from "../composer/prompt-optimization.js";
import { compactionLocked, compactions, persistCompactions } from "../conversation/compaction.js";
import { mutate, pending, savePending } from "../conversation/pending.js";
import { showWelcome } from "../conversation/selection.js";
import {
  goalComposerAction, goalComposerRecoveries, goalRecordThreadId, persistGoalComposerRecoveries
} from "../goal/composer.js";
import {
  goalEditDraft, goalEditPending, setGoalEditDraft, setGoalEditPending, stopGoalLiveTimer
} from "../goal/status.js";
import { readMarkRequests } from "../messages/user-content.js";
import { queuedActionBusy } from "../queue/queue.js";
import { queuedEditChecks, queuedEdits } from "../queue/queued-edit.js";
import { clearSentMessageDisplays } from "../queue/sent-display.js";
import {
  claudeThreadId, normalizeRelatedView, privateInputScope, validRelatedThreadId
} from "../session/capabilities.js";
import { relatedThreadViews } from "../session/related.js";
import {
  cacheDisplayData, readScope, sidebarSnapshot, threadDisplayKey, threadDisplays
} from "../session/thread-cache.js";
import { sessionMenu } from "../shell.js";
import {
  loadSidebar, recentPriorityIds, recentPriorityKnownIds, recentPrioritySelectionSuppressions,
  renderSidebar
} from "./list.js";

export let sidebarBusy = false;
export function setSidebarBusy(value) { return sidebarBusy = value; }
export const threadDeletions = new Map(), threadDeletionChecks = new Set();
export const threadDeletionStorageKey = "codex-elves-thread-deletions";
// 从会话存储恢复删除会话的待核对记录；提交中的记录恢复为结果未知。
export function restoreThreadDeletions() {
  try {
    const records = JSON.parse(sessionStorage.getItem(threadDeletionStorageKey) || "[]");
    if (Array.isArray(records)) for (const record of records) {
      if (typeof record?.deviceId !== "string" || typeof record.threadId !== "string"
          || !/^[a-zA-Z0-9_-]{1,120}$/.test(record.threadId)
          || !["submitting", "pending", "unknown", "deleted"].includes(record.state)
          || record.state !== "deleted" && (typeof record.id !== "string" || !/^[a-zA-Z0-9-]{1,100}$/.test(record.id))) continue;
      threadDeletions.set(JSON.stringify([record.deviceId, record.threadId]),
        { ...record, state: record.state === "submitting" ? "unknown" : record.state });
    }
  } catch {}
}
export let sidebarFilters = { mode: null, chats: null, projects: null, pinned: null };
const sidebarPreferenceStore = createSidebarPreferenceStore();
export let sidebarPreferenceScope = null;
export function setSidebarPreferenceScope(value) { return sidebarPreferenceScope = value; }
// 同一浏览器其他页面修改侧栏偏好时，按当前授权范围重新读取并刷新侧栏。
export function installSidebarPreferenceSync() {
  window.addEventListener("storage", event => {
    if (!sidebarPreferenceStore.ownsStorageEvent(privateInputScope(session), event)) return;
    sidebarPreferenceScope = null;
    restoreSidebarPreferences();
    renderSidebar();
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
export function threadDeletionKey(id, deviceId = session?.device.id) {
  return JSON.stringify([deviceId, id]);
}
export function threadDeleted(id, deviceId = session?.device.id) {
  return threadDeletions.get(threadDeletionKey(id, deviceId))?.state === "deleted";
}
export function threadDeletionLocked(id = selectedId) {
  const record = threadDeletions.get(threadDeletionKey(id));
  return !!record && record.state !== "deleted";
}
function setThreadDeletion(key, record, required = false) {
  const previous = threadDeletions.get(key);
  if (record && !Number.isFinite(record.time)) record.time = Date.now();
  if (record) threadDeletions.set(key, record);
  else threadDeletions.delete(key);
  try {
    sessionStorage.setItem(threadDeletionStorageKey,
      JSON.stringify([...threadDeletions.values()].filter(value => value.state !== "confirming")));
  } catch {
    if (required) {
      if (previous) threadDeletions.set(key, previous); else threadDeletions.delete(key);
      throw new Error("无法保存删除回执，会话未删除。请允许此网页使用浏览器会话存储。");
    }
  }
}
export function filterDeletedSidebar(value, deviceId = session?.device.id) {
  if (!value) return;
  const keep = item => !threadDeleted(item.id, deviceId);
  value.recent = value.recent.filter(keep);
  value.pinnedThreads = value.pinnedThreads.filter(keep);
  for (const project of value.projects) project.threads = project.threads.filter(keep);
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
export function applyThreadDeletion(id, deviceId = session?.device.id) {
  transcriptCache.remove(deviceId, id);
  if (session?.device.id === deviceId) threadDisplays.delete(threadDisplayKey(id));
  if (readScope && session?.device.id === deviceId) {
    void readCache.remove(readScope, "transcript", id).catch(() => {});
    void readCache.remove(readScope, "thread", id).catch(() => {});
  }
  const key = threadDeletionKey(id, deviceId), previous = threadDeletions.get(key);
  setThreadDeletion(key, { ...previous, deviceId, threadId: id, state: "deleted" });
  if (session?.device.id !== deviceId) return;
  clearSentMessageDisplays(id);
  sessionMenu?.removeThread(id);
  if (selectedId === id) {
    stopGoalLiveTimer(); showWelcome();
  }
  // showWelcome 会先保存当前草稿；永久删除后再清理，避免把草稿重新留在内存。
  deleteDraft(id);
  const removedGoalRecovery = goalComposerRecoveries.get(id);
  goalComposerRecoveries.delete(id);
  if (persistGoalComposerRecoveries() && removedGoalRecovery?.recoveryRef && recoveryScope)
    void recoveryStore.remove(recoveryScope, "pending", removedGoalRecovery.recoveryRef).catch(() => {});
  for (const [scope, record] of promptOptimizations)
    if (JSON.parse(scope)[0] === id) { cancelPromptOptimization(record); promptOptimizations.delete(scope); }
  readMarkRequests.delete(id);
  const queuedEdit = queuedEdits.get(id);
  if (queuedEdit) queuedEditChecks.delete(queuedEdit.editId);
  queuedEdits.delete(id); persistQueuedEdits();
  compactions.delete(id); persistCompactions();
  blockingRequestStates.delete(key);
  for (const [entryKey, reply] of approvalReplies)
    if (reply.deviceId === deviceId && reply.threadId === id) approvalReplies.delete(entryKey);
  for (const [entryKey, draft] of approvalDrafts)
    if (draft.deviceId === deviceId && draft.threadId === id) approvalDrafts.delete(entryKey);
  for (const [entryKey, reply] of questionReplyReceipts)
    if (reply.deviceId === deviceId && reply.threadId === id) {
      questionReplyReceipts.delete(entryKey); questionDrafts.delete(entryKey);
    }
  for (const entryKey of questionDrafts.keys()) {
    const [draftDevice, draftThread] = JSON.parse(entryKey);
    if (draftDevice === deviceId && draftThread === id) questionDrafts.delete(entryKey);
  }
  try { sessionStorage.setItem("codex-elves-question-receipts", JSON.stringify([...questionReplyReceipts.values()])); } catch {}
  if (goalEditDraft?.threadId === id) { setGoalEditDraft(null); $("#goal-edit-dialog")?.close(); }
  if (goalEditPending?.deviceId === deviceId && goalEditPending.payload.threadId === id) setGoalEditPending(null);
  if (pending?.deviceId === deviceId && (pending.payload?.threadId === id
      || goalComposerAction(pending.action, pending.payload) && goalRecordThreadId(pending) === id)) savePending(null);
  recentPriorityIds?.delete(id); recentPriorityKnownIds.delete(id);
  recentPrioritySelectionSuppressions.delete(id);
  filterDeletedSidebar(sidebar);
  if (sidebar) cacheDisplayData("sidebar", "main", sidebarSnapshot(sidebar));
  const hash = new URLSearchParams(location.hash.slice(1));
  if (hash.get("thread") === id) {
    hash.delete("thread");
    history.replaceState(null, "", `${location.pathname}${location.search}${hash.size ? `#${hash}` : ""}`);
  }
  renderSidebar(); updateComposer(); renderApprovals();
}
export function threadDeletionConflict(id) {
  return busy || stopping || sidebarBusy || pending?.payload?.threadId === id
    || compactionLocked(id) || queuedEdits.has(id)
    || goalEditPending?.deviceId === session?.device.id && goalEditPending.payload.threadId === id
    || [...queuedActionBusy].some(key => key.startsWith(`${id}:`))
    || [...approvalReplies.values()].some(reply => reply.deviceId === session?.device.id && reply.threadId === id
      && ["submitting", "pending", "unknown"].includes(reply.state))
    || [...questionReplyReceipts.values()].some(reply => reply.deviceId === session?.device.id && reply.threadId === id);
}
export async function deleteSidebarThread(item) {
  if (!session || session.readOnly || !connected || threadDeletionConflict(item.id)) return;
  const deviceId = session.device.id, key = threadDeletionKey(item.id, deviceId);
  if (threadDeletions.has(key)) return;
  let record = { id: randomId(), deviceId, threadId: item.id, title: item.title || "未命名会话", state: "confirming" };
  threadDeletions.set(key, record);
  try {
    const claude = claudeThreadId(item.id);
    const confirmed = await confirmAction(claude ? "移除 Claude 会话" : "删除会话",
      claude ? `将从远程列表移除“${record.title}”。Claude 原生会话历史仍保留，项目文件不受影响。`
        : `将永久删除“${record.title}”及其会话记录，此操作不可恢复。确定删除吗？`,
      claude ? "移除会话" : "删除会话", true);
    if (!confirmed || session?.device.id !== deviceId || session.readOnly || !connected) return;
    if (threadDeletions.get(key) !== record || threadDeletionConflict(item.id)) return;
    record = { ...record, state: "submitting" };
    setThreadDeletion(key, record, true);
    renderSidebar(); updateComposer();
    const result = await mutate("deleteThread", { threadId: item.id }, { receiptId: record.id });
    if (result?.ok !== true || result.deleted !== true || result.threadId !== item.id)
      throw Object.assign(new Error(result?.message || "删除结果尚未确认，请核对回执，勿重复删除。"), { outcomeUnknown: true });
    applyThreadDeletion(item.id, deviceId);
    if (session?.device.id === deviceId) {
      notice(claude ? "已从远程列表移除 Claude 会话" : result.status === "partial" ? result.message || "会话已永久删除，部分列表清理未完成。" : "会话已永久删除",
        result.status === "partial");
      void loadSidebar(null, { fresh: true }).catch(() => {});
    }
  } catch (error) {
    if (!threadDeleted(item.id, deviceId)) {
      if (error.outcomeUnknown || ["pending", "unknown"].includes(error.operationState))
        setThreadDeletion(key, { ...record, state: error.operationState === "pending" ? "pending" : "unknown" });
      else setThreadDeletion(key, null);
    }
    if (session?.device.id === deviceId && !threadDeleted(item.id, deviceId)) notice(error.message, true);
  } finally {
    if (threadDeletions.get(key)?.state === "confirming") threadDeletions.delete(key);
    if (session?.device.id === deviceId) { renderSidebar(); updateComposer(); }
  }
}
export async function checkThreadDeletion(record) {
  const key = threadDeletionKey(record.threadId, record.deviceId);
  if (!session || session.device.id !== record.deviceId || !connected || threadDeletionChecks.has(key)
      || threadDeletions.get(key)?.id !== record.id || threadDeleted(record.threadId)) return;
  threadDeletionChecks.add(key);
  try {
    const receipt = await api(`/api/receipts/${record.id}`);
    if (threadDeleted(record.threadId, record.deviceId) || threadDeletions.get(key)?.id !== record.id) return;
    const result = receipt.result;
    if (result?.ok === true && result.deleted === true && result.threadId === record.threadId) {
      applyThreadDeletion(record.threadId, record.deviceId);
      if (session?.device.id === record.deviceId) notice(result.message || "已确认会话永久删除", result.status === "partial");
    } else if (receipt.state === "failed" && result?.outcomeUnknown !== true) {
      setThreadDeletion(key, null);
      if (session?.device.id === record.deviceId) notice(result?.message || "会话未删除，可重新操作", true);
    } else {
      setThreadDeletion(key, { ...record, state: receipt.state === "pending" ? "pending" : "unknown" });
      if (session?.device.id === record.deviceId) notice(result?.message || "删除结果尚未确认，请在本机核对；不会自动重试删除。", true);
    }
  } catch (error) {
    if (session?.device.id === record.deviceId)
      notice(error.code === 404 ? "未找到删除回执，请在本机核对；不会自动重试删除。" : error.message, true);
  } finally {
    threadDeletionChecks.delete(key);
    if (session?.device.id === record.deviceId) { renderSidebar(); updateComposer(); }
  }
}
export function restoreSidebarPreferences() {
  const scope = privateInputScope(session);
  if (scope === sidebarPreferenceScope) return;
  sidebarPreferenceScope = scope;
  sidebarFilters = { mode: null, chats: null, projects: null, pinned: null };
  if (!scope) return;
  try { Object.assign(sidebarFilters, sidebarPreferenceStore.read(scope)); }
  catch { /* 存储不可读时仍显示可用的会话数据，用户修改时说明保存失败。 */ }
}
function saveSidebarPreferences(patch) {
  const scope = privateInputScope(session);
  if (!scope || !sidebar) return;
  restoreSidebarPreferences();
  Object.assign(sidebarFilters, patch);
  try { Object.assign(sidebarFilters, sidebarPreferenceStore.write(scope, patch)); }
  catch {
    notice("侧栏偏好已在本页生效，但浏览器本地保存失败，刷新后可能丢失。", true);
  }
  renderSidebar();
}
function changeSidebarView(mode) {
  if (!["project", "list"].includes(mode)) return;
  saveSidebarPreferences({ mode });
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
