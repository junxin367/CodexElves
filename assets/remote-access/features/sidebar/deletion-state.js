// 会话删除状态：删除记录的持久化与恢复、删除中判断与侧栏过滤。api.js 据此拒绝对删除中或已删除会话的操作，本模块须保持在最底层，只依赖 core。

import { selectedId, session } from "../../core/state.js";

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
export function setThreadDeletion(key, record, required = false) {
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
