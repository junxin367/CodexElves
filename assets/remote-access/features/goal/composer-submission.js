// 目标输入的提交确认：更新待确认状态、应用原生回执、手动结束，以及恢复提示的渲染。

import { recoveryStorageMessage } from "../../recovery-store.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { loadSidebar, reloadThread, resolvePending, selectThread, updateComposer } from "../../core/actions.js";
import { busy, connected, creating, selectedId, selectionToken, session, thread } from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { api } from "../api.js";
import { clearSubmittedDraft, restoreDraft } from "../composer/draft-restore.js";
import {
  draftKey, drafts, goalDrafts, matchingDraft, newDraftKey, recoveryScope, snapshotDraft,
  storeDraft
} from "../composer/drafts.js";
import { selectCreatedThread } from "../composer/new-chat-settings.js";
import { pendingFeedbackReady } from "../composer/send-state.js";
import { pending, pendingTimer, setPending } from "../conversation/pending-state.js";
import { savePending } from "../conversation/pending-store.js";
import { dismissGoalComposerRecovery, goalPayloadForThread, persistGoalComposerRecoveries } from "./composer.js";
import {
  goalComposerAction, goalComposerReceiptMessage, goalComposerRecoveries, goalReceiptOutcome,
  goalRecordThreadId, goalSubmissionState, goalVersion, setGoalVersion, unfinishedGoal
} from "./state.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/state.js";
import { threadReadOnly, validRelatedThreadId } from "../session/capabilities.js";
import { threadDeleted, threadDeletionLocked } from "../sidebar/deletion-state.js";

export function updateGoalComposerPending(record) {
  try { savePending(record); }
  catch {
    clearTimeout(pendingTimer);
    setPending(record);
    notice("目标回执已保留在当前页面，但浏览器无法更新会话存储；请保持页面打开并核对。", true);
  }
}
export async function applyGoalComposerReceipt(record, receipt, { allowSelection = false, manuallyVerified = false } = {}) {
  if (!record || record.deviceId !== session?.device.id || !goalComposerAction(record.action, record.payload)) return;
  const result = receipt?.result || {};
  if (receipt?.state === "completed" && result.ok === true && result.backend === "claude"
      && result.nativeCommand?.name === "goal" && typeof result.nativeCommand.id === "string"
      && validRelatedThreadId(result.threadId) && (!record.payload.threadId || record.payload.threadId === result.threadId)) {
    // Claude 回执仅确认原生命令受理；清空已提交草稿，目标栏等原生记录，正文不提前插入。
    const owns = record.selectionToken === selectionToken && draftKey() === (record.payload.threadId
      || newDraftKey(record.payload.projectId, record.payload.backend, record.payload.instanceId));
    clearSubmittedDraft(record.payload, owns);
    if (pending?.id === record.id) savePending(null);
    goalComposerRecoveries.delete(result.threadId); persistGoalComposerRecoveries();
    if (record.action === "create" && allowSelection && owns && creating)
      await selectCreatedThread(result.threadId).catch(error => notice(`命令已受理，会话读取失败：${error.message}`, true));
    else if (selectedId === result.threadId)
      await reloadThread().catch(error => notice(`命令已受理，状态读取失败：${error.message}`, true));
    void loadSidebar(null, { fresh: true }).catch(() => {});
    notice(result.nativeCommand.state === "completed" ? "目标命令已执行" : "目标命令已交给 Claude，等待原生执行");
    updateComposer();
    return "succeeded";
  }
  const state = goalSubmissionState(result.goalSubmission, record.action)
    || goalSubmissionState(record.goalSubmission, record.action);
  const deletedThreadId = state?.threadId || record.payload.threadId;
  if (deletedThreadId && threadDeleted(deletedThreadId, record.deviceId)) {
    goalComposerRecoveries.delete(deletedThreadId); persistGoalComposerRecoveries();
    if (pending?.id === record.id) savePending(null);
    return "failed";
  }
  const outcome = manuallyVerified ? "failed" : goalReceiptOutcome(receipt, record.action);
  const confirmedCreated = record.action === "create" && (record.confirmedCreated || result.status === "created"
    || state?.firstTurnAccepted || manuallyVerified);
  const updated = { ...record, result, receiptState: receipt?.state || "unknown",
    ...(confirmedCreated ? { confirmedCreated: true } : {}),
    ...(state ? { goalSubmission: state } : {}), ...(manuallyVerified ? { manuallyVerified: true } : {}) };
  if (record.recoveryRef && recoveryScope) {
    try { await recoveryStore.put(recoveryScope, [{ kind: "pending", id: record.recoveryRef, value: updated }]); }
    catch (error) {
      updateGoalComposerPending(updated);
      notice(`目标结果已收到，但恢复记录未能更新：${recoveryStorageMessage(error)}`, true);
      return;
    }
    if (record.deviceId !== session?.device.id) return;
  }
  const sourceKey = record.payload.threadId || newDraftKey(record.payload.projectId, record.payload.backend, record.payload.instanceId);
  const ownsSelection = record.selectionToken === selectionToken && draftKey() === sourceKey;
  const unchangedCurrent = !ownsSelection || matchingDraft(snapshotDraft(), record.payload);
  const threadId = state?.threadId || record.payload.threadId || null;
  const consumedCreation = record.action === "create" && threadId && confirmedCreated;
  if (consumedCreation) {
    // 已确认创建会话后，后续恢复始终绑定其 ID，不能回到 create。
    if (!record.consumed) clearSubmittedDraft(record.payload, ownsSelection && creating);
    updated.consumed = true;
    if (outcome !== "succeeded") {
      const previous = goalComposerRecoveries.get(threadId);
      const recovery = { ...updated, recoveryState: outcome === "failed" ? "failed" : "unknown" };
      goalComposerRecoveries.set(threadId, recovery);
      if (!previous && !drafts.has(threadId) && !goalDrafts.has(threadId)) storeDraft(threadId, record.draft);
      try {
        // 明确失败时把同一记录移到恢复存储，避免临时存两份图片占满配额。
        if (outcome === "failed" && pending?.id === record.id) sessionStorage.removeItem("codex-elves-pending");
        persistGoalComposerRecoveries(true);
      }
      catch (error) {
        if (pending?.id === record.id) updateGoalComposerPending(updated);
        notice(error.message, true);
        return;
      }
    } else {
      clearSubmittedDraft(goalPayloadForThread(record, threadId), selectedId === threadId && !editingMessage && !editingQueuedMessage);
      goalComposerRecoveries.delete(threadId); persistGoalComposerRecoveries();
    }
  } else if (outcome === "succeeded") {
    clearSubmittedDraft(record.payload, ownsSelection && selectedId === record.payload.threadId);
    goalComposerRecoveries.delete(threadId); persistGoalComposerRecoveries();
  }
  if (outcome === "succeeded" && thread?.id === threadId && record.goalReadVersion === goalVersion && result.goal) {
    setGoalVersion(goalVersion + 1); thread.goal = result.goal;
  }
  if (pending?.id === record.id) {
    if (["succeeded", "failed"].includes(outcome)) savePending(null);
    else updateGoalComposerPending(updated);
  }
  if (consumedCreation && allowSelection && ownsSelection && creating) {
    if (unchangedCurrent) await selectThread(threadId, { closeSidebar: false });
    else await selectCreatedThread(threadId);
  }
  if (["succeeded", "failed"].includes(outcome))
    void loadSidebar(null, { fresh: true }).catch(error => notice(`目标提交已处理，但会话列表更新失败：${error.message}`, true));
  if (outcome === "succeeded" && selectedId === threadId)
    void reloadThread().catch(error => notice(`目标已创建，但会话同步失败：${error.message}`, true));
  notice(goalComposerReceiptMessage(updated, outcome), outcome !== "succeeded");
  updateComposer();
  return outcome;
}
export async function manuallyResolveGoalComposer(record) {
  if (pending?.id !== record.id || record.deviceId !== session?.device.id || busy || !connected) return;
  const id = goalRecordThreadId(record);
  const description = id
    ? "请先在本机核对首条输入和目标。本操作只解除本次锁定；已创建会话只允许继续设置目标，不会重新创建或重发首条。若已有未完成目标，请从目标栏编辑。"
    : "请先在本机确认本次操作没有创建会话、首条输入也没有执行。解除锁定后仍保留草稿；若无法确认，请继续保留原回执，不要再次提交。";
  if (!await confirmAction("核对目标提交", description, "已在本机核对，解除锁定", true)
      || pending?.id !== record.id || record.deviceId !== session?.device.id || busy || !connected) return;
  if (id) {
    try {
      const read = await api("/api/read/thread", { threadId: id });
      if (pending?.id !== record.id || record.deviceId !== session?.device.id) return;
      if (read.thread?.id !== id) throw new Error("会话读取结果不匹配，请保留原回执继续核对");
      // 只核对会话存在与当前目标，不把相同正文当成本次提交成功证据。
      if (selectedId === id) await reloadThread();
      const state = goalSubmissionState(record.goalSubmission, record.action)
        || { action: record.action, phase: record.action === "create" ? "firstTurn" : "goalSet",
          threadId: id, firstTurnAccepted: false, goalSet: "unknown" };
      await applyGoalComposerReceipt({ ...record, goalSubmission: state },
        { state: "failed", result: { ...record.result, goalSubmission: state } },
        { allowSelection: true, manuallyVerified: true });
    } catch (error) { notice(`核对失败，仍保留目标提交锁定：${error.message}`, true); return; }
  } else {
    savePending(null); updateComposer();
  }
  notice("已按人工核对解除锁定，目标草稿已保留。已有会话只会重试设置目标。");
}
export function renderGoalComposerRecovery(root) {
  const record = goalComposerRecoveries.get(selectedId) || (creating
    ? [...goalComposerRecoveries.values()].reverse().find(value =>
      value.deviceId === session?.device.id && newDraftKey(value.payload.projectId, value.payload.backend, value.payload.instanceId) === draftKey())
    : null);
  if (!record || record.deviceId !== session?.device.id || pending?.id === record.id) return;
  if (record.recoveryState === "unknown" && !pendingFeedbackReady(record)) return;
  const recoveryThreadId = goalRecordThreadId(record);
  const box = element("div", "pending-banner goal-composer-recovery", unfinishedGoal()
    ? "此会话已有未完成目标，请从上方目标栏核对或编辑；本次目标草稿仍保留。"
    : record.recoveryState === "unknown" ? "正在确认目标设置结果。"
      : goalComposerReceiptMessage(record, record.recoveryState));
  const dismiss = record.recoveryState === "failed"
    ? button("不再恢复", "仅关闭本次本地恢复提示，保留当前草稿和已保存目标",
      () => dismissGoalComposerRecovery(record), "text-button") : null;
  if (selectedId !== recoveryThreadId) {
    const open = button("打开已创建会话", "打开已创建会话中的目标草稿，不会重建或重发首条",
      () => void selectThread(recoveryThreadId, { closeSidebar: false }), "text-button");
    open.disabled = !connected;
    box.append(open);
    if (dismiss) box.append(dismiss);
    root.append(box);
    return;
  }
  if (record.recoveryState === "unknown") {
    const check = button("查原回执", "只查询原目标提交回执，不会重新设置目标", async () => {
      if (pending?.id === record.id) return resolvePending();
      try {
        const receipt = await api(`/api/receipts/${record.id}`);
        await applyGoalComposerReceipt(record, receipt);
      } catch (error) { notice(`原目标回执尚未确认：${error.message}`, true); }
    }, "text-button");
    check.disabled = !connected;
    const confirm = button("已人工核对", "在本机核对后解除目标设置锁定", async () => {
      if (pending && pending.id !== record.id) { notice("请先核对当前提交回执", true); return; }
      if (!pending) savePending(record);
      await manuallyResolveGoalComposer(pending);
    }, "text-button");
    confirm.disabled = !connected || busy || !!pending;
    box.append(check, confirm);
  }
  const restore = button("恢复目标草稿", "将本次保留的目标草稿恢复到输入框", () => {
    if (busy || editingMessage || editingQueuedMessage || threadReadOnly() || threadDeletionLocked()) return;
    const current = snapshotDraft(), payload = goalPayloadForThread(record, selectedId);
    if (!matchingDraft(current, payload) && (current.text || current.skills.length || current.attachments.length || current.goalMode)) {
      notice("当前输入已有其他草稿，请先保留该草稿，再恢复本次目标。", true); return;
    }
    storeDraft(selectedId, record.draft); restoreDraft(selectedId); updateComposer();
    $("#prompt").focus({ preventScroll: true });
  }, "text-button");
  restore.disabled = busy || !!editingMessage || !!editingQueuedMessage || threadReadOnly() || threadDeletionLocked();
  box.append(restore);
  if (dismiss) box.append(dismiss);
  root.append(box);
}
