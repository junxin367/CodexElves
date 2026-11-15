// 关联会话共享状态：关联与侧边会话视图、操作记录与计时器、侧边新建与返回状态。

import { $ } from "../../ui/dom.js";
import { creating, selectedId, sidebar } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { sessionMenu } from "../shell-state.js";

export let relatedSessionView = null;
export function setRelatedSessionView(value) { return relatedSessionView = value; }
export let relatedBackPending = false;
export function setRelatedBackPending(value) { return relatedBackPending = value; }
export let relatedExitTransition = null;
export function setRelatedExitTransition(value) { return relatedExitTransition = value; }
export let sideCreationView = null;
export function setSideCreationView(value) { return sideCreationView = value; }
export let relatedMenuTimer = null, relatedOperationTimer = null, relatedSessionCheckTimer = null;
export function setRelatedOperationTimer(value) { return relatedOperationTimer = value; }
export function setRelatedSessionCheckTimer(value) { return relatedSessionCheckTimer = value; }
export function setRelatedMenuTimer(value) { return relatedMenuTimer = value; }
export let transientHistoryView = null, restoringTransientHistory = null;
export function setTransientHistoryView(value) { return transientHistoryView = value; }
export function setRestoringTransientHistory(value) { return restoringTransientHistory = value; }
export const relatedThreadViews = new Map(), relatedThreadOperations = new Map();
export const relatedHistoryKey = "codexElvesRelatedSession", relatedOperationStorageKey = "codex-elves-related-operations";
export const sideCreationHistoryKey = "codexElvesSideCreation";
export const conversationHistoryKey = "codexElvesConversationPosition";
export const relatedNavigationOwner = randomId();
export function syncSideThreadUnread() {
  const trigger = $("#open-session-tools");
  if (!trigger) return;
  const unread = !creating && !relatedSessionView && (sidebar?.unreadSideThreads || [])
    .some(item => item.parentThreadId === selectedId);
  trigger.classList.toggle("has-unread-side-thread", unread);
  const label = unread ? "会话菜单，有未读的侧边会话" : "会话菜单";
  trigger.setAttribute("aria-label", label); trigger.title = label;
}
export function sessionMenuIsOpen() {
  if (relatedSessionView) return false;
  return typeof sessionMenu?.isOpen === "function" ? sessionMenu.isOpen() : !!sessionMenu?.isOpen;
}
export function conversationHistoryPosition(state) {
  const value = state?.[conversationHistoryKey];
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}
export function stopRelatedSessionTimers() {
  clearTimeout(relatedMenuTimer); relatedMenuTimer = null;
  clearTimeout(relatedOperationTimer); relatedOperationTimer = null;
  clearTimeout(relatedSessionCheckTimer); relatedSessionCheckTimer = null;
}
export function persistRelatedOperations(required = false) {
  try {
    const records = [...relatedThreadOperations.values()].map(record => ({
      id: record.id, action: record.action, scope: record.scope, parentThreadId: record.parentThreadId,
      threadId: record.threadId, state: record.state, selectionToken: record.selectionToken, autoOpen: record.autoOpen,
      time: record.time
    }));
    if (records.length) sessionStorage.setItem(relatedOperationStorageKey, JSON.stringify(records));
    else sessionStorage.removeItem(relatedOperationStorageKey);
  } catch {
    if (required) throw new Error("无法保存侧边会话操作回执，操作未提交。请允许浏览器会话存储。");
  }
}
export function relatedOperationKey(record) {
  return JSON.stringify([record.scope, record.action, record.parentThreadId, record.threadId || null]);
}
