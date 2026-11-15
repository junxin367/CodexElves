// 目标输入：/goal 命令、目标草稿与恢复记录的保存和恢复、发送时的目标载荷；提交确认与回执核对在 composer-submission.js，共享状态在 goal/state.js。

import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closeComposerMenus, updateComposer } from "../../core/actions.js";
import { busy, connected, creating, selectedId, session, stopping, thread } from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import {
  drafts, goalDrafts, newDraftKey, recoveryScope, rememberDraft, storeDraft, submittedDrafts
} from "../composer/drafts.js";
import { promptSelection, setPromptSelection, syncPromptTrailingBreak } from "../composer/prompt-editor.js";
import { renderSelectedSkills } from "../composer/selected-skills.js";
import { selectedSkills, slashQuery } from "../composer/skill-catalog.js";
import { compactionLocked } from "../conversation/compaction.js";
import { pending, validSkillSelection } from "../conversation/pending-state.js";
import {
  goalComposerAction, goalComposerRecoveries, goalRecordThreadId, goalSubmissionState,
  hydrateGoalComposerRecord, selectedGoal, setSelectedGoal, unfinishedGoal
} from "./state.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/state.js";
import { claudeCommandSupported } from "../session/action-capabilities.js";
import { isClaude, serviceSideThread, threadReadOnly, threadReadOnlyMessage } from "../session/capabilities.js";
import { composerScroll } from "../shell-state.js";
import { threadDeleted, threadDeletionLocked } from "../sidebar/deletion-state.js";

export function validGoalComposerDraft(value) {
  return typeof value?.text === "string" && Array.isArray(value.skills) && validSkillSelection(value.skills) && Array.isArray(value.attachments)
    && typeof value.goalMode === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(value.goalMode);
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
  (selectedGoal || setSelectedGoal(randomId()));
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
  setSelectedGoal(null);
  for (const node of prompt?.querySelectorAll(".inline-goal") || []) node.remove();
  if (prompt) syncPromptTrailingBreak(prompt);
  rememberDraft(); updateComposer(); prompt?.focus({ preventScroll: true });
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
export function goalPayloadForThread(record, threadId) {
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
export function dismissGoalComposerRecovery(record) {
  const id = goalRecordThreadId(record);
  if (!id || record?.deviceId !== session?.device.id || record.recoveryState !== "failed"
      || goalComposerRecoveries.get(id) !== record || pending?.id === record.id) return false;
  goalComposerRecoveries.delete(id);
  if (persistGoalComposerRecoveries() && record.recoveryRef && recoveryScope)
    void recoveryStore.remove(recoveryScope, "pending", record.recoveryRef).catch(() => {});
  updateComposer();
  return true;
}
