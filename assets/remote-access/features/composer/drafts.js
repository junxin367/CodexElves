// 输入草稿与私有输入恢复：草稿存取、失败输入恢复、队列编辑草稿与切换授权时的清理/恢复。

import {
  clearComposerAttachments, composerAttachmentsLoading, composerAttachmentSnapshot,
  setComposerAttachments
} from "../../attachments.js";
import { permissionModeKnown } from "../../permissions.js";
import { recoveryStorageMessage } from "../../recovery-store.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { creating, selectedId, session, setBusy } from "../../core/state.js";
import { recoveryStore, transcriptCache } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import {
  approvalDrafts, approvalReplies, blockingRequestStates, questionDrafts, questionReceiptChecks,
  questionReplyReceipts
} from "../approvals.js";
import { closeComposerMenus, setModelQuery } from "./composer-menus.js";
import { clearPendingFeedbackTimer, sending, setSending } from "./composer.js";
import { resetComposerMetrics } from "./metrics.js";
import {
  fastModeVersion, modelChoiceVersion, remoteFastWrites, selectedEffort, selectedModel,
  setFastModeVersion, setModelChoiceVersion, setRemoteFastState, setRemoteFastWriting,
  setSelectedDraftFast, setSelectedEffort, setSelectedModel
} from "./models.js";
import { creationBackend, creationInstanceId, creationProject, setCodexCreationDefaultsState } from "./new-chat.js";
import {
  draftPermissionChoices, instancePermissionCatalogs, instancePermissionReads,
  permissionSelections, permissionWrites
} from "./permissions.js";
import { composerPrompt, resizeComposerInput, sameSkills, syncSelectedSkillsFromPrompt } from "./prompt-editor.js";
import { promptOptimizationSettingsDialog } from "./prompt-optimization.js";
import {
  globalSkillChoices, renderSelectedSkills, scheduleCatalogRefresh, selectedSkills,
  setAppliedGlobalSkills, setAppliedSkillCatalog, setMessageSkillLoadScope, setSelectedSkills,
  setSkillChoices, setSkillLoading, setSkillLoadVersion, setSkillRenderVersion, setSkillScope,
  skillLoadVersion, skillRenderVersion
} from "./skills.js";
import { compactions } from "../conversation/compaction.js";
import {
  pending, pendingTimer, setPending, setPendingSendRequest, validSkillSelection
} from "../conversation/pending.js";
import { goalComposerAction, goalComposerRecoveries, selectedGoal, setSelectedGoal } from "../goal/composer.js";
import { assignGoalEditPending } from "../goal/status.js";
import { editingMessage, setEditingMessage } from "../messages/message-edit.js";
import {
  editingQueuedMessage, queuedEdits, setEditingQueuedMessage, validQueuedMessageEdit
} from "../queue/queued-edit.js";
import { sentMessageDisplays } from "../queue/sent-display.js";
import { isClaude, privateInputScope } from "../session/capabilities.js";
import { setSessionInstanceRequest, setSessionInstances } from "../session/instances.js";
import { resetRelatedSessions } from "../session/related.js";
import { clearReadViews, threadEphemeralState } from "../session/thread-cache.js";
import { threadDeletions, threadDeletionStorageKey } from "../sidebar/actions.js";

export const drafts = new Map();
export const skillDrafts = new Map();
export const attachmentDrafts = new Map();
export const goalDrafts = new Map();
export const draftWrites = new Map(), failedInputRecoveries = new Map();
export let recoveryScope = null, recoveryGeneration = 0, draftStorageError = "";
export function setRecoveryScope(value) { return recoveryScope = value; }
export const submittedDrafts = new WeakMap();
export function newDraftKey(projectId, backend = "codex", instanceId = "") {
  // 用户要求实例固定；草稿和失败恢复按实例隔离，旧 Claude 记录只迁移到原默认实例。
  if (!instanceId && backend === "claude") instanceId = session?.claudeInstanceId || "";
  if (instanceId && instanceId !== "codex:local") return `$new:instance:${instanceId}:${projectId || ""}`;
  return backend === "claude" ? `$new:claude:${projectId || ""}` : `$new:${projectId || ""}`;
}
export function snapshotDraft() {
  syncSelectedSkillsFromPrompt();
  return {
    text: $("#prompt").value,
    skills: selectedSkills.map(skill => ({ ...skill })),
    attachments: composerAttachmentSnapshot(),
    goalMode: selectedGoal,
    ...(draftPermissionChoices.has(draftKey()) ? { permissionMode: draftPermissionChoices.get(draftKey()) } : {})
  };
}
export function draftKey() { return selectedId || newDraftKey(creationProject, creationBackend, creationInstanceId); }
function storedDraft(key) {
  return {
    text: drafts.get(key) || "",
    skills: skillDrafts.get(key) || [],
    attachments: attachmentDrafts.get(key) || [],
    goalMode: goalDrafts.get(key) || null,
    ...(draftPermissionChoices.has(key) ? { permissionMode: draftPermissionChoices.get(key) } : {})
  };
}
export function storeDraft(key, draft) {
  drafts.set(key, draft.text); skillDrafts.set(key, draft.skills || []);
  attachmentDrafts.set(key, draft.attachments || []);
  if (draft.goalMode) goalDrafts.set(key, draft.goalMode);
  else goalDrafts.delete(key);
  if (key.startsWith("$new:") && typeof draft.permissionMode === "string"
      && (permissionModeKnown("codex", draft.permissionMode) || permissionModeKnown("claude", draft.permissionMode)))
    draftPermissionChoices.set(key, draft.permissionMode);
  void persistInputDraft(key).catch(() => {});
}
export function deleteDraft(key) {
  drafts.delete(key); skillDrafts.delete(key); attachmentDrafts.delete(key); goalDrafts.delete(key);
  draftPermissionChoices.delete(key);
  void persistInputDraft(key, true).catch(() => {});
}
export function inputDraftEmpty(draft) {
  return !draft?.text && !draft?.skills?.length && !draft?.attachments?.length && !draft?.goalMode && !draft?.permissionMode;
}
export function persistInputDraft(key, remove = false) {
  if (!recoveryScope || !session) return Promise.resolve();
  const scope = recoveryScope, generation = recoveryGeneration;
  let job = draftWrites.get(key);
  if (job?.scope === scope) { job.again = true; job.remove = remove; return job.promise; }
  job = { scope, again: false, remove, promise: null };
  job.promise = (async () => {
    do {
      job.again = false;
      if (generation !== recoveryGeneration || scope !== recoveryScope) return;
      const draft = storedDraft(key);
      if (job.remove || inputDraftEmpty(draft)) await recoveryStore.remove(scope, "draft", key);
      else await recoveryStore.put(scope, [{ kind: "draft", id: key, value: draft }]);
      draftStorageError = "";
    } while (job.again);
  })().catch(error => {
    if (generation === recoveryGeneration && draftStorageError !== error.name) {
      draftStorageError = error.name || "Error";
      if ($("#notice")) notice(recoveryStorageMessage(error), true);
    }
    throw error;
  }).finally(() => { if (draftWrites.get(key) === job) draftWrites.delete(key); });
  draftWrites.set(key, job);
  return job.promise;
}
export function clearPrivateInputs({ persisted = true } = {}) {
  promptOptimizationSettingsDialog?.close();
  resetComposerMetrics();
  clearPendingFeedbackTimer();
  let previousScope = recoveryScope || privateInputScope(session);
  if (!previousScope) try { previousScope = sessionStorage.getItem("codex-elves-input-owner"); } catch {}
  ++recoveryGeneration; recoveryScope = null;
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
  recoveryScope = scope;
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
export function restoreSmallInputRecoveries() {
  try {
    const records = JSON.parse(sessionStorage.getItem("codex-elves-failed-inputs") || "[]");
    if (Array.isArray(records)) for (const record of records)
      if (record?.deviceId === session?.device.id && record.recoveryScope === privateInputScope(session)
          && typeof record.draft?.text === "string" && validSkillSelection(record.draft.skills))
        failedInputRecoveries.set(record.id, record);
  } catch {}
}
export function smallInputStorageRecord(record) {
  const serialized = JSON.stringify(record);
  return serialized.length <= 128 * 1024 ? serialized : null;
}
function persistSmallInputRecoveries() {
  const serialized = JSON.stringify([...failedInputRecoveries.values()].filter(record => record.sessionRecovery));
  if (serialized.length > 256 * 1024) throw Object.assign(new Error("小文字恢复记录已达到会话存储上限。"), { name: "QuotaExceededError" });
  sessionStorage.setItem("codex-elves-failed-inputs", serialized);
}
function ordinaryInputRecord(record) {
  return record && ["send", "create"].includes(record.action) && !goalComposerAction(record.action, record.payload);
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
export function rememberDraft() {
  // 欢迎页/只读初始页没有激活的草稿，空编辑器不能覆盖已恢复的 $new 草稿。
  if (!selectedId && !creating) return;
  if (editingQueuedMessage?.threadId === selectedId) { rememberQueuedMessageEdit(); return; }
  const key = draftKey(), draft = snapshotDraft(), previous = storedDraft(key);
  const changed = draft.text !== previous.text || !sameSkills(draft.skills, previous.skills)
    || draft.skills.some((skill, index) => (skill._offset ?? 0) !== (previous.skills[index]?._offset ?? 0))
    || draft.attachments.length !== previous.attachments.length
    || draft.attachments.some((image, index) => ["kind", "name", "mime", "size", "dataUrl", "source", "url"]
      .some(name => image[name] !== previous.attachments[index]?.[name]))
    || (draft.goalMode || null) !== (previous.goalMode || null)
    || (draft.permissionMode || null) !== (previous.permissionMode || null);
  // 切换或卸载时，未编辑的本页旧稿不能覆盖其他页面已经保存的新稿。
  if (!changed) return;
  if (selectedGoal && previous.goalMode === selectedGoal) {
    // 编辑后即使又改回相同文字，也属于新草稿，不能被旧回执清空。
    setSelectedGoal(randomId()); draft.goalMode = selectedGoal;
  }
  storeDraft(key, draft);
}
export function restoreDraft(key) {
  $("#prompt").value = drafts.get(key) || "";
  setSelectedSkills((skillDrafts.get(key) || []).map(skill => ({ ...skill })));
  setSelectedGoal(goalDrafts.get(key) || null);
  setComposerAttachments(attachmentDrafts.get(key) || []);
  renderSelectedSkills();
}
export function persistQueuedEditDraft(edit, remove = false) {
  if (!recoveryScope || !session || edit?.deviceId !== session.device.id
      || threadEphemeralState(edit.threadId) === true) return Promise.resolve();
  const scope = recoveryScope, generation = recoveryGeneration, key = `$queue-edit:${edit.editId}`;
  let job = draftWrites.get(key);
  if (job?.scope === scope) { job.again = true; job.remove = remove; job.edit = edit; return job.promise; }
  job = { scope, edit, again: false, remove, promise: null };
  job.promise = (async () => {
    do {
      job.again = false;
      if (generation !== recoveryGeneration || scope !== recoveryScope) return;
      // 普通草稿和队列编辑使用不同记录；编辑文字时不能覆盖其他窗口后来保存的普通输入。
      const value = { ...job.edit, message: { ...job.edit.message }, backup: { ...job.edit.backup }, recoveryScope: scope };
      if (job.remove) await recoveryStore.remove(scope, "queue-edit", edit.editId);
      else await recoveryStore.put(scope, [{ kind: "queue-edit", id: edit.editId, value }]);
      draftStorageError = "";
    } while (job.again);
  })().catch(error => {
    if (generation === recoveryGeneration && scope === recoveryScope && draftStorageError !== error.name) {
      draftStorageError = error.name || "Error";
      notice(recoveryStorageMessage(error), true);
    }
    throw error;
  }).finally(() => { if (draftWrites.get(key) === job) draftWrites.delete(key); });
  draftWrites.set(key, job);
  return job.promise;
}
export function persistQueuedEdits(required = false) {
  try {
    if (queuedEdits.size) sessionStorage.setItem("codex-elves-queue-edits", JSON.stringify([...queuedEdits.values()]));
    else sessionStorage.removeItem("codex-elves-queue-edits");
  } catch {
    if (required) throw new Error("无法保存队列编辑草稿，操作未提交。请允许此网页使用浏览器会话存储。");
    notice("无法保存队列编辑草稿，请保持页面打开并完成发送。", true);
  }
  if (typeof persistQueuedEditDraft === "function" && selectedId && queuedEdits.get(selectedId)?.deviceId
      && queuedEdits.get(selectedId).deviceId === session?.device?.id)
    void persistQueuedEditDraft(queuedEdits.get(selectedId)).catch(() => {});
}
export function rememberQueuedMessageEdit(required = false) {
  if (editingQueuedMessage?.state !== "editing" || !$("#prompt")) return;
  editingQueuedMessage.text = $("#prompt").value;
  editingQueuedMessage.attachments = composerAttachmentSnapshot();
  persistQueuedEdits(required);
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
export function matchingDraft(draft, payload) {
  // Skill 跨过空白时可能得到相同 prompt，回执还需核对提交时的实际位置。
  const submitted = submittedDrafts.get(payload);
  return (!submitted || draft.text === submitted.text
      && JSON.stringify(draft.skills.map(skill => skill._offset ?? 0))
        === JSON.stringify(submitted.skills.map(skill => skill._offset ?? 0))
      && (draft.goalMode || null) === (submitted.goalMode || null))
    && !!draft.goalMode === (payload.goalMode === true)
    // 默认或记住的权限由发送阶段补齐；回执仍比较提交时的原草稿，避免误判未修改或清除后来显式选择。
    && (draft.permissionMode || "configured") === ((submitted || payload).permissionMode || "configured")
    && composerPrompt(draft.text, draft.skills, payload.goalMode === true) === payload.prompt
    && sameSkills(draft.skills, payload.skills || [])
    && JSON.stringify((draft.attachments || []).filter(entry => entry.kind === "inline")
      .map(({ name, mime, dataUrl, size }) => ({ name, mime, dataUrl, size })))
      === JSON.stringify(payload.images || []);
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
export function resetComposerOptions() {
  setSelectedModel(setSelectedEffort(""));
  setCodexCreationDefaultsState(null);
  setModelChoiceVersion(modelChoiceVersion + 1);
  setSelectedDraftFast(false);
  setRemoteFastState(null);
  setModelQuery("");
  setSkillChoices(isClaude() ? [] : [...globalSkillChoices]); setSkillScope(""); setSkillLoading(null); setSkillLoadVersion(skillLoadVersion + 1);
  setMessageSkillLoadScope(""); setSkillRenderVersion(skillRenderVersion + 1);
  setAppliedSkillCatalog(setAppliedGlobalSkills(null));
  closeComposerMenus();
  scheduleCatalogRefresh();
}
