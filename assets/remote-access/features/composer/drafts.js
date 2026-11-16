// 输入草稿存储：草稿与失败输入记录的读写、小型输入恢复记录、排队编辑草稿的保存；恢复与清除流程在 draft-restore.js。

import { composerAttachmentSnapshot } from "../../attachments.js";
import { permissionModeKnown } from "../../permissions.js";
import { recoveryStorageMessage } from "../../recovery-store.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { creating, selectedId, session } from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { creationBackend, creationInstanceId, creationProject } from "./creation-state.js";
import { draftPermissionChoices } from "./options-state.js";
import { composerPrompt, sameSkills, syncSelectedSkillsFromPrompt } from "./prompt-editor.js";
import { selectedSkills } from "./skill-catalog.js";
import { validSkillSelection } from "../conversation/pending-state.js";
import { goalComposerAction, selectedGoal, setSelectedGoal } from "../goal/state.js";
import { editingQueuedMessage, queuedEdits } from "../queue/state.js";
import { privateInputScope } from "../session/capabilities.js";
import { threadEphemeralState } from "../session/thread-cache.js";

export const drafts = new Map();
export const skillDrafts = new Map();
export const attachmentDrafts = new Map();
export const goalDrafts = new Map();
export const draftWrites = new Map(), failedInputRecoveries = new Map();
export let recoveryScope = null, recoveryGeneration = 0, draftStorageError = "";
export function setRecoveryGeneration(value) { return recoveryGeneration = value; }
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
export function storedDraft(key) {
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
export function persistSmallInputRecoveries() {
  const serialized = JSON.stringify([...failedInputRecoveries.values()].filter(record => record.sessionRecovery));
  if (serialized.length > 256 * 1024) throw Object.assign(new Error("小文字恢复记录已达到会话存储上限。"), { name: "QuotaExceededError" });
  sessionStorage.setItem("codex-elves-failed-inputs", serialized);
}
export function ordinaryInputRecord(record) {
  return record && ["send", "create"].includes(record.action) && !goalComposerAction(record.action, record.payload);
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
      .some(name => image[name] !== previous.attachments[index]?.[name])
      || JSON.stringify(image.files || []) !== JSON.stringify(previous.attachments[index]?.files || [])
      || JSON.stringify(image.directories || []) !== JSON.stringify(previous.attachments[index]?.directories || []))
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
      === JSON.stringify(payload.images || [])
    // 文件/文件夹随草稿保存；旧发送回执不能清除后来选择的同名但内容不同的本机附件。
    && JSON.stringify((draft.attachments || []).filter(entry => ["file", "folder"].includes(entry.kind)))
      === JSON.stringify(payload.files || []);
}
