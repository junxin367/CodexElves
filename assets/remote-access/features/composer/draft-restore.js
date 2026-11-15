// 草稿与私有输入的恢复和清除：切换会话时恢复草稿与排队编辑，恢复发送失败的输入，清除已提交草稿，授权变化时清除并恢复全部私有输入。

import { clearComposerAttachments, composerAttachmentsLoading, setComposerAttachments } from "../../attachments.js";
import { permissionModeKnown } from "../../permissions.js";
import { recoveryStorageMessage } from "../../recovery-store.js";
import { $ } from "../../ui/dom.js";
import { selectedId, session, setBusy } from "../../core/state.js";
import { recoveryStore, transcriptCache } from "../../core/stores.js";
import {
  approvalDrafts, approvalReplies, blockingRequestStates, questionDrafts, questionReceiptChecks,
  questionReplyReceipts
} from "../approval-state.js";
import {
  attachmentDrafts, deleteDraft, draftKey, draftWrites, drafts, failedInputRecoveries, goalDrafts,
  inputDraftEmpty, matchingDraft, newDraftKey, ordinaryInputRecord, persistInputDraft,
  persistSmallInputRecoveries, recoveryGeneration, recoveryScope, setRecoveryGeneration,
  setRecoveryScope, skillDrafts, smallInputStorageRecord, snapshotDraft, storeDraft, storedDraft,
  submittedDrafts
} from "./drafts.js";
import { resetComposerMetrics } from "./metrics.js";
import {
  draftPermissionChoices, fastModeVersion, instancePermissionCatalogs, instancePermissionReads,
  modelChoiceVersion, permissionSelections, permissionWrites, remoteFastWrites, selectedEffort,
  selectedModel, setFastModeVersion, setRemoteFastState, setRemoteFastWriting, setSelectedDraftFast,
  setSelectedEffort, setSelectedModel
} from "./options-state.js";
import { resizeComposerInput } from "./prompt-editor.js";
import { promptOptimizationSettingsDialog } from "./prompt-optimization.js";
import { renderSelectedSkills } from "./selected-skills.js";
import { clearPendingFeedbackTimer, sending, setSending } from "./send-state.js";
import { setSelectedSkills } from "./skill-catalog.js";
import { compactions } from "../conversation/compaction.js";
import {
  pending, pendingTimer, setPending, setPendingSendRequest, validSkillSelection
} from "../conversation/pending-state.js";
import { assignGoalEditPending, goalComposerRecoveries, setSelectedGoal } from "../goal/state.js";
import { editingMessage, setEditingMessage } from "../messages/message-edit.js";
import { sentMessageDisplays } from "../queue/sent-display.js";
import { editingQueuedMessage, queuedEdits, setEditingQueuedMessage, validQueuedMessageEdit } from "../queue/state.js";
import { isClaude, privateInputScope } from "../session/capabilities.js";
import { setSessionInstanceRequest, setSessionInstances } from "../session/instances.js";
import { resetRelatedSessions } from "../session/related.js";
import { clearReadViews } from "../session/thread-cache.js";
import { threadDeletionStorageKey, threadDeletions } from "../sidebar/deletion-state.js";

export function clearPrivateInputs({ persisted = true } = {}) {
  promptOptimizationSettingsDialog?.close();
  resetComposerMetrics();
  clearPendingFeedbackTimer();
  let previousScope = recoveryScope || privateInputScope(session);
  if (!previousScope) try { previousScope = sessionStorage.getItem("codex-elves-input-owner"); } catch {}
  setRecoveryGeneration(recoveryGeneration + 1); setRecoveryScope(null);
  setSelectedDraftFast(false);
  setRemoteFastState(setRemoteFastWriting(null));
  remoteFastWrites.clear(); setFastModeVersion(fastModeVersion + 1);
  draftPermissionChoices.clear(); permissionSelections.clear(); permissionWrites.clear();
  instancePermissionCatalogs.clear(); instancePermissionReads.clear();
  setSessionInstances([]); setSessionInstanceRequest(null);
  resetRelatedSessions({ persisted: true });
  drafts.clear(); skillDrafts.clear(); attachmentDrafts.clear(); goalDrafts.clear();
  draftWrites.clear(); failedInputRecoveries.clear(); goalComposerRecoveries.clear();
  queuedEdits.clear(); questionDrafts.clear(); questionReplyReceipts.clear(); questionReceiptChecks.clear(); compactions.clear();
  sentMessageDisplays.clear();
  blockingRequestStates.clear(); approvalReplies.clear(); approvalDrafts.clear(); threadDeletions.clear();
  setPending(null); setPendingSendRequest(null); setEditingQueuedMessage(null); setEditingMessage(null);
  if (sending) { setSending(null); setBusy(false); }
  assignGoalEditPending(null);
  setSelectedSkills([]); setSelectedGoal(null); transcriptCache.clear();
  void clearReadViews();
  clearTimeout(pendingTimer);
  for (const key of ["codex-elves-pending", "codex-elves-input-owner", "codex-elves-goal-composer-recoveries",
    "codex-elves-queue-edits", "codex-elves-question-receipts", "codex-elves-compact-pending",
    "codex-elves-goal-edit-pending", "codex-elves-failed-inputs", threadDeletionStorageKey]) {
    try { sessionStorage.removeItem(key); } catch {}
  }
  return persisted ? recoveryStore.clear(previousScope) : Promise.resolve();
}
export async function restorePrivateInputs(value, { expected = null, requestGeneration = recoveryGeneration } = {}) {
  const scope = privateInputScope(value);
  if (requestGeneration !== recoveryGeneration)
    throw Object.assign(new Error("会话读取已取消，当前授权状态已改变。"), { name: "RecoveryAuthorizationError" });
  // 先在同一 IDB 事务核对读取开始时的 owner/代次，再允许清理旧授权数据。
  await recoveryStore.activate(scope, { replace: !!expected, expected });
  if (requestGeneration !== recoveryGeneration || privateInputScope(session) !== scope) return;
  let previousScope;
  try { previousScope = sessionStorage.getItem("codex-elves-input-owner"); } catch {}
  if (recoveryScope && recoveryScope !== scope || previousScope && previousScope !== scope
      || pending?.deviceId && pending.deviceId !== value.device.id) {
    await clearPrivateInputs({ persisted: false });
  }
  const currentGeneration = recoveryGeneration;
  setRecoveryScope(scope);
  try { sessionStorage.setItem("codex-elves-input-owner", scope); } catch {}
  const [savedDrafts, savedPending, savedFailures, activeReceipts, savedQueueEdits] = await Promise.all([
    recoveryStore.list(scope, "draft"), recoveryStore.list(scope, "pending"), recoveryStore.list(scope, "failed"),
    recoveryStore.list(scope, "active"), recoveryStore.list(scope, "queue-edit")
  ]);
  if (currentGeneration !== recoveryGeneration || privateInputScope(session) !== scope) return;
  const hydrate = record => {
    if (!record?.recoveryRef) return record;
    const full = savedPending.find(entry => entry.id === record.recoveryRef)?.value;
    return full && full.deviceId === value.device.id && full.recoveryScope === scope
      ? { ...full, ...record, payload: { ...full.payload, ...record.payload }, draft: full.draft,
        ...(record.recoveryUnavailable ? { invalid: false, recoveryUnavailable: false } : {}) }
      : { ...record, invalid: true };
  };
  setPending(hydrate(pending));
  for (const [id, record] of goalComposerRecoveries) goalComposerRecoveries.set(id, hydrate(record));
  // IndexedDB 本身保存完整回执，sessionStorage 被限制时仍可找回原请求标识。
  if (!pending) {
    const candidates = savedPending.map(entry => entry.value)
      .filter(record => record.deviceId === value.device.id && record.recoveryScope === scope
        && activeReceipts.some(active => active.id === record.id)
        && ![...goalComposerRecoveries.values()].some(recovery => recovery.id === record.id));
    const latest = candidates.sort((left, right) => right.time - left.time)[0];
    if (latest) setPending({ ...latest, recoveryRef: latest.id });
  }
  if (pending?.draft) submittedDrafts.set(pending.payload, pending.draft);
  // 用户要求客户端重开后直接恢复最后的编辑正文和附件；恢复记录与普通草稿分开，不能跨授权借用。
  for (const { id, value: edit } of savedQueueEdits)
    if (validQueuedMessageEdit(edit) && edit.editId === id && edit.deviceId === value.device.id
        && edit.recoveryScope === scope && !queuedEdits.has(edit.threadId))
      queuedEdits.set(edit.threadId, edit);
  for (const { id: savedId, value: draft } of savedDrafts) {
    const id = savedId.startsWith("$new:claude:") && value.claudeInstanceId
      ? newDraftKey(savedId.slice("$new:claude:".length), "claude", value.claudeInstanceId) : savedId;
    if (typeof draft?.text !== "string" || !validSkillSelection(draft.skills) || !Array.isArray(draft.attachments)
        || id !== savedId && savedDrafts.some(entry => entry.id === id)) continue;
    if (drafts.has(id)) continue;
    if (id !== savedId) {
      // 旧默认 Claude 草稿先持久化到实例键再移除旧键；迁移失败保留原数据，不影响其他环境草稿。
      await recoveryStore.put(scope, [{ kind: "draft", id, value: draft }]);
      if (currentGeneration !== recoveryGeneration || privateInputScope(session) !== scope) return;
      await recoveryStore.remove(scope, "draft", savedId);
    }
    drafts.set(id, draft.text); skillDrafts.set(id, draft.skills || []);
    attachmentDrafts.set(id, draft.attachments);
    if (draft.goalMode) goalDrafts.set(id, draft.goalMode);
    if (id.startsWith("$new:") && typeof draft.permissionMode === "string"
        && (permissionModeKnown("codex", draft.permissionMode) || permissionModeKnown("claude", draft.permissionMode)))
      draftPermissionChoices.set(id, draft.permissionMode);
  }
  for (const { id, value: record } of savedFailures)
    if (record.deviceId === value.device.id && record.recoveryScope === scope) failedInputRecoveries.set(id, record);
}
export async function recoverFailedInput(record) {
  if (!ordinaryInputRecord(record) || record.deviceId !== session?.device.id
      || record.recoveryScope && record.recoveryScope !== privateInputScope(session)) return;
  if (record.recoveryRef && !record.draft) throw new Error("本地草稿暂时无法读取，原回执已保留。请恢复存储后再核对，勿重新发送。");
  const scope = recoveryScope || privateInputScope(session), generation = recoveryGeneration;
  const key = record.payload.threadId || newDraftKey(record.payload.projectId, record.payload.backend, record.payload.instanceId);
  const draft = record.draft || { text: record.payload.prompt || "", skills: record.payload.skills || [],
    attachments: (record.payload.images || []).map(image => ({ ...image, kind: "inline" })), goalMode: null };
  const recovery = { ...record, draft, recoveryScope: scope };
  try { await recoveryStore.put(scope, [{ kind: "failed", id: record.id, value: recovery }]); }
  catch (error) {
    if (!smallInputStorageRecord(recovery)) throw new Error(recoveryStorageMessage(error));
    recovery.sessionRecovery = true;
    failedInputRecoveries.set(record.id, recovery);
    try { persistSmallInputRecoveries(); }
    catch (storageError) { throw new Error(recoveryStorageMessage(storageError)); }
  }
  if (generation !== recoveryGeneration || record.deviceId !== session?.device.id) return;
  failedInputRecoveries.set(record.id, recovery);
  const isCurrent = draftKey() === key && !editingQueuedMessage && !editingMessage;
  if (isCurrent && composerAttachmentsLoading()) return;
  const existing = isCurrent ? snapshotDraft() : storedDraft(key);
  if (!inputDraftEmpty(existing) && !matchingDraft(existing, record.payload)) return;
  if (record.action === "create" && !draft.permissionMode && typeof record.payload.permissionMode === "string")
    draft.permissionMode = record.payload.permissionMode;
  storeDraft(key, draft);
  if (isCurrent) { restoreDraft(key); resizeComposerInput(); }
  if (recovery.sessionRecovery) return;
  await persistInputDraft(key);
  await recoveryStore.remove(scope, "failed", record.id);
  if (scope === recoveryScope) failedInputRecoveries.delete(record.id);
}
export function restoreDraft(key) {
  $("#prompt").value = drafts.get(key) || "";
  setSelectedSkills((skillDrafts.get(key) || []).map(skill => ({ ...skill })));
  setSelectedGoal(goalDrafts.get(key) || null);
  setComposerAttachments(attachmentDrafts.get(key) || []);
  renderSelectedSkills();
}
export function restoreQueuedMessageEdit() {
  const edit = queuedEdits.get(selectedId);
  setEditingQueuedMessage(edit?.deviceId === session?.device.id ? edit : null);
  if (!editingQueuedMessage) return;
  $("#prompt").value = edit.state === "editing" ? edit.text : edit.backup.text;
  setSelectedSkills(edit.state === "editing" ? [] : edit.backup.skills || []);
  setSelectedGoal(edit.state === "editing" ? null : edit.backup.goalMode || null);
  // 模型选择属于后续发送，不存入队列消息；每次恢复编辑时记录当前选择版本，结束编辑不能覆盖后来选择。
  edit.modelChoiceVersion = modelChoiceVersion;
  setComposerAttachments(edit.state === "editing" ? edit.attachments || [] : edit.backup.attachments || [],
    { readOnly: edit.state === "editing" && edit.imageEditing !== true,
      removableRemote: edit.state === "editing" && edit.imageRemoval === true });
  renderSelectedSkills();
}
export function clearSubmittedDraft(payload, current = true) {
  const choice = submittedDrafts.get(payload)?.modelChoice;
  // 用户要求拒绝原生模型切换后显示原模型；送达后交给原生设置状态展示，不保留已提交的本地覆盖值。
  // 菜单后来做出的新选择有独立版本，迟到回执不能清掉新草稿的模型或等级。
  if (current && choice && choice.version === modelChoiceVersion && isClaude()) {
    if (selectedModel === choice.model) setSelectedModel("");
    if (selectedEffort === choice.effort) setSelectedEffort("");
  }
  const key = payload.threadId || newDraftKey(payload.projectId, payload.backend, payload.instanceId);
  for (const [id, record] of failedInputRecoveries) {
    const recoveryKey = record.payload.threadId || newDraftKey(record.payload.projectId, record.payload.backend, record.payload.instanceId);
    if (recoveryKey !== key || !matchingDraft(record.draft, payload)) continue;
    failedInputRecoveries.delete(id);
    if (!record.sessionRecovery && recoveryScope) void recoveryStore.remove(recoveryScope, "failed", id).catch(() => {});
  }
  try { persistSmallInputRecoveries(); } catch {}
  if (current && composerAttachmentsLoading()) return;
  // 新建会话清理前先固定当前编辑器快照；显式权限属于匹配字段，deleteDraft 会同步删除权限选择。
  const currentDraft = current ? snapshotDraft() : null;
  if (matchingDraft(storedDraft(key), payload)) deleteDraft(key);
  if (currentDraft && matchingDraft(currentDraft, payload)) {
    $("#prompt").value = ""; setSelectedSkills([]); setSelectedGoal(null); clearComposerAttachments(); renderSelectedSkills();
  }
}
