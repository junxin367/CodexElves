// 目标输入：/goal 命令、目标草稿与回执恢复、首轮发送后设置目标的结果核对。

import { recoveryStorageMessage } from "../../recovery-store.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { busy, connected, creating, selectedId, selectionToken, session, stopping, thread } from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { pendingFeedbackReady, updateComposer } from "../composer/composer.js";
import {
  clearSubmittedDraft, draftKey, drafts, goalDrafts, matchingDraft, newDraftKey, recoveryScope,
  rememberDraft, restoreDraft, snapshotDraft, storeDraft, submittedDrafts
} from "../composer/drafts.js";
import { selectCreatedThread } from "../composer/new-chat.js";
import { promptSelection, setPromptSelection, syncPromptTrailingBreak } from "../composer/prompt-editor.js";
import { renderSelectedSkills, selectedSkills, slashQuery } from "../composer/skills.js";
import { compactionLocked } from "../conversation/compaction.js";
import { reloadThread } from "../conversation/history.js";
import {
  pending, pendingTimer, resolvePending, savePending, setPending, validSkillSelection
} from "../conversation/pending.js";
import { selectThread } from "../conversation/selection.js";
import { goalVersion, setGoalVersion } from "./status.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/queued-edit.js";
import {
  claudeCommandSupported, isClaude, serviceSideThread, threadReadOnly, threadReadOnlyMessage,
  validRelatedThreadId
} from "../session/capabilities.js";
import { composerScroll } from "../shell.js";
import { threadDeleted, threadDeletionLocked } from "../sidebar/actions.js";
import { loadSidebar } from "../sidebar/list.js";

export let selectedGoal = null;
export function setSelectedGoal(value) { return selectedGoal = value; }
export function validGoalComposerDraft(value) {
  return typeof value?.text === "string" && Array.isArray(value.skills) && validSkillSelection(value.skills) && Array.isArray(value.attachments)
    && typeof value.goalMode === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(value.goalMode);
}
export function goalComposerAction(action, payload) {
  return payload?.goalMode === true && ["create", "setGoal"].includes(action);
}
export function goalSubmissionState(value, action) {
  return value && ["create", "setGoal"].includes(value.action) && (!action || value.action === action)
    && ["preparing", "firstTurn", "goalSet", "complete"].includes(value.phase)
    && (value.threadId === null || typeof value.threadId === "string" && /^[a-zA-Z0-9_-]{1,120}$/.test(value.threadId))
    && typeof value.firstTurnAccepted === "boolean"
    && ["not-attempted", "succeeded", "failed", "unknown"].includes(value.goalSet) ? value : null;
}
export function goalComposerStorageRecord(record) {
  if (record?.recoveryRef) {
    const { draft, ...metadata } = record;
    const { images, ...payload } = record.payload || {};
    return { ...metadata, payload };
  }
  if (!goalComposerAction(record?.action, record?.payload) || !validGoalComposerDraft(record.draft)) return record;
  let imageIndex = 0;
  return { ...record, draft: { ...record.draft, attachments: record.draft.attachments.map(entry => {
    if (entry.kind !== "inline") return entry;
    const index = imageIndex++, image = record.payload.images?.[index];
    if (!image || image.dataUrl !== entry.dataUrl) return entry;
    const { dataUrl, ...metadata } = entry;
    return { ...metadata, goalImageIndex: index };
  }) } };
}
export function hydrateGoalComposerRecord(record) {
  if (!goalComposerAction(record?.action, record?.payload) || !record.draft) return record;
  return { ...record, draft: { ...record.draft, attachments: (record.draft.attachments || []).map(entry => {
    if (entry.kind !== "inline" || !Object.hasOwn(entry, "goalImageIndex")) return entry;
    const image = Number.isInteger(entry.goalImageIndex) && record.payload.images?.[entry.goalImageIndex];
    if (!image || typeof image.dataUrl !== "string") { record.invalid = true; return entry; }
    const { goalImageIndex, ...metadata } = entry;
    return { ...metadata, dataUrl: image.dataUrl };
  }) }, ...(record.invalid ? { invalid: true } : {}) };
}
export const goalComposerRecoveries = new Map();
// 从会话存储恢复目标创建的失败/未知回执，供目标输入恢复提示使用。
export function restoreGoalComposerRecoveries() {
  try {
    const stored = JSON.parse(sessionStorage.getItem("codex-elves-goal-composer-recoveries") || "[]");
    if (Array.isArray(stored)) for (const saved of stored) {
      const record = hydrateGoalComposerRecord(saved);
      const state = goalSubmissionState(record?.goalSubmission, record?.action);
      if (record?.invalid || typeof record?.deviceId !== "string" || typeof record.id !== "string"
          || !/^[a-zA-Z0-9-]{1,100}$/.test(record.id) || !state?.threadId
          || record.action !== "create" || !validGoalComposerDraft(record.draft) && typeof record.recoveryRef !== "string"
          || !["failed", "unknown"].includes(record.recoveryState)
          || !state.firstTurnAccepted && record.manuallyVerified !== true && record.confirmedCreated !== true) continue;
      goalComposerRecoveries.set(state.threadId, record);
    }
  } catch {}
}
export const goalComposerPlaceholder = "描述你的目标，定义可衡量的成果，以获得最佳效果";
function unfinishedGoal(goal = thread?.goal) {
  return !!goal && goal.status !== "complete" && goal.status !== "completed";
}
export function goalComposerReason({ submitting = false } = {}) {
  if (isClaude() && !claudeCommandSupported("goal")) return "当前 Claude 实例尚未确认原生目标能力";
  if (!session || !creating && (!selectedId || thread?.id !== selectedId)) return "请先打开会话或新建会话";
  if (threadReadOnly()) return threadReadOnlyMessage();
  if (serviceSideThread()) return "临时侧边会话不创建持续目标，请返回主会话设置";
  if (!isClaude() && session.capabilities?.goalComposer !== true) return "当前服务尚不支持创建目标，请更新远程服务；目标草稿会保留";
  if (editingMessage || editingQueuedMessage) return "请先完成消息或队列编辑";
  if (threadDeletionLocked() || threadDeleted(selectedId)) return "会话正在删除或已删除，请先核对删除结果";
  if (!isClaude() && submitting && compactionLocked()) return "可继续编辑目标，压缩完成后再创建";
  if (!creating && unfinishedGoal()) return "此会话已有未完成目标，请通过上方目标栏编辑";
  if (!connected) return "连接恢复后可创建目标";
  if (busy || stopping) return "请等待当前提交完成";
  if (pending) return "请先核对上次提交回执，勿重复创建目标";
  if (goalComposerRecoveries.get(selectedId)?.recoveryState === "unknown")
    return "目标设置结果未知，请先查回执或在本机核对";
  return "";
}
export function goalCommandMatches(query) {
  return ["目标", "goal", "持续追求"].some(value => value.includes(query));
}
export function goalPrefix(text, skills = selectedSkills) {
  if (isClaude() && !claudeCommandSupported("goal")) return null;
  const match = /^(\/go+al)(?=\s|$)\s*/i.exec(String(text ?? ""));
  if (match && skills.some(skill => (skill._offset ?? 0) < match[1].length)) return null;
  return match ? { start: 0, end: match[0].length } : null;
}
export function selectGoalCommand(range = slashQuery()) {
  const reason = goalComposerReason();
  if (reason) { notice(reason, true); return false; }
  const prompt = $("#prompt");
  if (range) prompt.setRangeText("", range.start, range.end, "end");
  const selection = promptSelection(prompt);
  selectedGoal ||= randomId();
  renderSelectedSkills();
  rememberDraft();
  closeComposerMenus(); updateComposer();
  composerScroll?.expand(); prompt.focus({ preventScroll: true });
  setPromptSelection(prompt, selection.start, selection.end, selection);
  return true;
}
function clearGoalComposerMode() {
  if (!selectedGoal) return;
  const prompt = $("#prompt");
  prompt?.editHistory?.begin("token");
  selectedGoal = null;
  for (const node of prompt?.querySelectorAll(".inline-goal") || []) node.remove();
  if (prompt) syncPromptTrailingBreak(prompt);
  rememberDraft(); updateComposer(); prompt?.focus({ preventScroll: true });
}
export function updateGoalComposerPending(record) {
  try { savePending(record); }
  catch {
    clearTimeout(pendingTimer);
    setPending(record);
    notice("目标回执已保留在当前页面，但浏览器无法更新会话存储；请保持页面打开并核对。", true);
  }
}
export function persistGoalComposerRecoveries(required = false) {
  try {
    if (goalComposerRecoveries.size)
      sessionStorage.setItem("codex-elves-goal-composer-recoveries", JSON.stringify([...goalComposerRecoveries.values()]
        .filter(record => record.recoveryState !== "unknown" || pending?.id !== record.id)
        .map(goalComposerStorageRecord)));
    else sessionStorage.removeItem("codex-elves-goal-composer-recoveries");
    return true;
  } catch {
    if (required) throw new Error("无法保存已创建会话的目标草稿，请保留页面并核对回执。");
    notice("无法保存目标恢复草稿，请保留页面并完成核对。", true);
    return false;
  }
}
export function goalRecordThreadId(record) {
  return goalSubmissionState(record?.goalSubmission, record?.action)?.threadId
    || (typeof record?.result?.threadId === "string" ? record.result.threadId : null)
    || record?.payload?.threadId || null;
}
function goalPayloadForThread(record, threadId) {
  const payload = { ...record.payload, threadId };
  delete payload.projectId;
  if (record.draft) submittedDrafts.set(payload, record.draft);
  return payload;
}
export function restoreGoalComposerDrafts() {
  const restore = (record, key) => {
    if (record.deviceId !== session?.device.id || !validGoalComposerDraft(record.draft)) return;
    submittedDrafts.set(record.payload, record.draft);
    if (!drafts.has(key) && !goalDrafts.has(key)) storeDraft(key, record.draft);
  };
  for (const [id, record] of goalComposerRecoveries) restore(record, id);
  if (!pending || !goalComposerAction(pending.action, pending.payload)) return;
  const state = goalSubmissionState(pending.goalSubmission, pending.action);
  if (pending.invalid || pending.goalSubmission && !state || pending.manuallyVerified && !state?.threadId) {
    pending.invalid = true;
    return;
  }
  const key = pending.action === "create" && (state?.firstTurnAccepted || pending.manuallyVerified
      || pending.confirmedCreated || pending.result?.status === "created")
    ? state?.threadId : pending.payload.threadId || newDraftKey(pending.payload.projectId, pending.payload.backend, pending.payload.instanceId);
  if (key) restore(pending, key);
}
export function goalReceiptOutcome(receipt, action) {
  const result = receipt?.result || {}, state = goalSubmissionState(result.goalSubmission, action);
  if (receipt?.state === "completed" && result.ok === true
      && state?.phase === "complete" && state.goalSet === "succeeded") return "succeeded";
  if (receipt?.state === "pending") return "pending";
  if (receipt?.state === "unknown" || state?.goalSet === "unknown"
      || result.outcomeUnknown === true && state?.goalSet !== "failed") return "unknown";
  if (receipt?.state === "failed" || receipt?.state === "completed" && result.ok === false) return "failed";
  return "unknown";
}
function goalComposerReceiptMessage(record, outcome) {
  const state = goalSubmissionState(record.goalSubmission, record.action);
  if (record.action === "create" && state?.firstTurnAccepted) {
    if (outcome === "succeeded") return "会话和目标已创建";
    if (outcome === "failed") return "会话及首条输入已创建，但目标未设置成功。草稿已保存在该会话，只能重试设置目标。";
    return "会话及首条输入已创建，目标设置结果待确认。请查原回执或在本机核对，不会重建会话或重发首条。";
  }
  if (record.action === "create" && (record.confirmedCreated || record.result?.status === "created")) {
    return outcome === "failed"
      ? "会话已创建，首条输入未确认接受，目标尚未设置。草稿已保存在该会话，后续只会设置目标。"
      : "会话已创建，首条输入或目标结果待确认。原回执已保留，不会重建会话或重发首条。";
  }
  if (outcome === "succeeded") return "目标已创建";
  if (outcome === "failed") return record.result?.message || "目标未创建，草稿与目标模式已保留";
  return record.action === "create"
    ? "新建目标的首条输入结果待确认。草稿与原回执已保留，不会自动重建会话或重发。"
    : "目标设置结果待确认。草稿与原回执已保留，不会自动重复设置。";
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
function dismissGoalComposerRecovery(record) {
  const id = goalRecordThreadId(record);
  if (!id || record?.deviceId !== session?.device.id || record.recoveryState !== "failed"
      || goalComposerRecoveries.get(id) !== record || pending?.id === record.id) return false;
  goalComposerRecoveries.delete(id);
  if (persistGoalComposerRecoveries() && record.recoveryRef && recoveryScope)
    void recoveryStore.remove(recoveryScope, "pending", record.recoveryRef).catch(() => {});
  updateComposer();
  return true;
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
