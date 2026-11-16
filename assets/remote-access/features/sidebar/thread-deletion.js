// 会话删除：确认后删除会话，应用删除结果并核对尚未确认的删除。

import { confirmAction } from "../../ui/dialog.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  loadSidebar, mutate, renderApprovals, renderSidebar, showWelcome, updateComposer
} from "../../core/actions.js";
import { busy, connected, selectedId, session, sidebar, stopping } from "../../core/state.js";
import { readCache, recoveryStore, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import {
  approvalDrafts, approvalReplies, blockingRequestStates, questionDrafts, questionReplyReceipts
} from "../approval-state.js";
import { deleteDraft, persistQueuedEdits, recoveryScope } from "../composer/drafts.js";
import { cancelPromptOptimization, promptOptimizations } from "../composer/prompt-optimization.js";
import { compactionLocked, compactions, persistCompactions } from "../conversation/compaction.js";
import { pending } from "../conversation/pending-state.js";
import { savePending } from "../conversation/pending-store.js";
import { persistGoalComposerRecoveries } from "../goal/composer.js";
import {
  goalComposerAction, goalComposerRecoveries, goalEditDraft, goalEditPending, goalRecordThreadId,
  setGoalEditDraft, setGoalEditPending, stopGoalLiveTimer
} from "../goal/state.js";
import { readMarkRequests } from "../messages/user-content.js";
import { clearSentMessageDisplays } from "../queue/sent-display.js";
import { queuedActionBusy, queuedEditChecks, queuedEdits } from "../queue/state.js";
import { claudeThreadId } from "../session/capabilities.js";
import {
  cacheDisplayData, readScope, sidebarSnapshot, threadDisplayKey, threadDisplays
} from "../session/thread-cache.js";
import { sessionMenu } from "../shell-state.js";
import { sidebarBusy } from "./actions.js";
import {
  filterDeletedSidebar, setThreadDeletion, threadDeleted, threadDeletionChecks, threadDeletionKey,
  threadDeletions
} from "./deletion-state.js";
import { recentPriorityIds, recentPriorityKnownIds, recentPrioritySelectionSuppressions } from "./list-state.js";

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
function deletionMessage(message) {
  // 用户上报的旧删除回执误用了发送文案；只修正文案，原 unknown 回执和禁止重发约束保持不变。
  return ["送达状态未知，请查看原生会话，不要重复发送",
    "服务曾重启，消息送达状态未知，请查看原生会话，不要重复发送",
    "送达状态未知，请在会话中确认，勿重复发送"].includes(message)
    ? "删除结果尚未确认，请在原生客户端核对，勿重复删除。" : message;
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
        // 用户要求全部侧边空闲才允许删除；确认文案同时说明自动关闭侧边及执行中限制。
        : `将永久删除“${record.title}”、其会话记录及原生子智能体会话，并关闭所属临时侧边会话，项目文件不受影响。此操作不可恢复。主会话或任一侧边会话执行中的任务需先停止，其他客户端占用的会话需在对应客户端处理。确定删除吗？`,
      claude ? "移除会话" : "删除会话", true);
    if (!confirmed || session?.device.id !== deviceId || session.readOnly || !connected) return;
    if (threadDeletions.get(key) !== record || threadDeletionConflict(item.id)) return;
    record = { ...record, state: "submitting" };
    setThreadDeletion(key, record, true);
    renderSidebar(); updateComposer();
    const result = await mutate("deleteThread", { threadId: item.id }, { receiptId: record.id });
    if (result?.ok !== true || result.deleted !== true || result.threadId !== item.id || result.outcomeUnknown === true)
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
    if (session?.device.id === deviceId && !threadDeleted(item.id, deviceId)) notice(deletionMessage(error.message), true);
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
    if (receipt?.requestId !== record.id) {
      setThreadDeletion(key, { ...record, state: "unknown" });
      if (session?.device.id === record.deviceId)
        notice("删除回执与原请求不匹配，原记录已保留；不会重复删除。", true);
      return;
    }
    const result = receipt.result;
    if (receipt.state === "completed" && result?.outcomeUnknown !== true
        && result?.ok === true && result.deleted === true && result.threadId === record.threadId) {
      applyThreadDeletion(record.threadId, record.deviceId);
      if (session?.device.id === record.deviceId) notice(result.message || "已确认会话永久删除", result.status === "partial");
    } else if (receipt.state === "failed" && result?.outcomeUnknown !== true
        && result?.ok === false && result?.deleted !== true) {
      setThreadDeletion(key, null);
      if (session?.device.id === record.deviceId) notice(deletionMessage(result?.message) || "会话未删除，可重新操作", true);
    } else {
      setThreadDeletion(key, { ...record, state: receipt.state === "pending" ? "pending" : "unknown" });
      if (session?.device.id === record.deviceId) notice(deletionMessage(result?.message) || "删除结果尚未确认，请在本机核对；不会自动重试删除。", true);
    }
  } catch (error) {
    if (session?.device.id === record.deviceId)
      notice(error.code === 404 ? "未找到删除回执，请在本机核对；不会自动重试删除。" : error.message, true);
  } finally {
    threadDeletionChecks.delete(key);
    if (session?.device.id === record.deviceId) { renderSidebar(); updateComposer(); }
  }
}
