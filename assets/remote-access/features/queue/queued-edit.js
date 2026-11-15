// 排队消息编辑：开始与结束编辑、恢复未完成的编辑并提交；编辑状态在 queue/state.js，未完成编辑的核对与恢复控件在 queued-edit-recovery.js。

import {
  attachmentsFromQueuedMessage, composerAttachmentsLoading, composerAttachmentsPayload,
  composerHasAttachments, composerRemoteAttachmentSources, setComposerAttachments
} from "../../attachments.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import { applyQueuedSendResult, closeComposerMenus, mutate, updateComposer } from "../../core/actions.js";
import { busy, connected, selectedId, selectionVersion, session, setBusy, stopping, thread } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { restoreDraft, restoreQueuedMessageEdit } from "../composer/draft-restore.js";
import {
  inputDraftEmpty, persistQueuedEditDraft, persistQueuedEdits, rememberDraft,
  rememberQueuedMessageEdit, snapshotDraft, storeDraft
} from "../composer/drafts.js";
import {
  modelChoiceVersion, selectedEffort, selectedModel, setSelectedEffort, setSelectedModel
} from "../composer/options-state.js";
import { setPromptSelection } from "../composer/prompt-editor.js";
import { renderSelectedSkills } from "../composer/selected-skills.js";
import { setSelectedSkills } from "../composer/skill-catalog.js";
import { compactionLocked } from "../conversation/compaction.js";
import { pending, reportMutationError, validQueuedImageSources } from "../conversation/pending-state.js";
import { setSelectedGoal } from "../goal/state.js";
import { editingMessage } from "../messages/message-edit.js";
import {
  editingQueuedMessage, notifiedQueueEdits, queueMessageText, queueVersion, queuedActionBusy,
  queuedEdits, setEditingQueuedMessage, setNotifiedQueueEdits
} from "./state.js";
import { queueCapability, threadReadOnly } from "../session/capabilities.js";

export function restoreUnfinishedQueuedMessageEdit() {
  if (busy || pending || editingMessage || editingQueuedMessage && editingQueuedMessage.state !== "starting" || !connected || threadReadOnly()
      || stopping || compactionLocked() || !selectedId || thread?.id !== selectedId) return false;
  const summary = thread.queuedEdits?.find(edit => edit.state === "editing"
    && typeof edit.editId === "string" && typeof edit.messageId === "string" && typeof edit.originalText === "string");
  if (!summary) return false;
  const local = queuedEdits.get(selectedId);
  if (local && (local.deviceId !== session?.device.id || local.editId !== summary.editId)) return false;
  if (editingQueuedMessage && local !== editingQueuedMessage) return false;
  if (local?.state === "starting" && editingQueuedMessage === local) {
    // 本地停在取回中、服务端已确认同一编辑时只补齐状态；存在任何待确认回执时仍不能走到这里。
    local.state = "editing"; persistQueuedEdits(); restoreQueuedMessageEdit(); return true;
  }
  const currentDraft = snapshotDraft();
  const isBackupDraft = local && JSON.stringify({
    text: currentDraft.text, skills: currentDraft.skills, attachments: currentDraft.attachments,
    goalMode: currentDraft.goalMode
  }) === JSON.stringify({
    text: local.backup?.text || "", skills: local.backup?.skills || [],
    attachments: local.backup?.attachments || [], goalMode: local.backup?.goalMode || null
  });
  if (!inputDraftEmpty(currentDraft) && !isBackupDraft) return false;
  if (local) { restoreQueuedMessageEdit(); return true; }
  // 用户要求未完成的编辑直接回到输入框，不再显示“继续编辑”引导。
  // 只恢复已确认 editing 的本地视图，不重新取出、取消或发送；已有普通草稿和待确认回执优先保留。
  rememberDraft();
  const edit = {
    threadId: selectedId, editId: summary.editId, deviceId: session.device.id, state: "editing",
    message: { id: summary.messageId }, originalText: summary.originalText || "", text: summary.originalText || "",
    attachments: [], originalImageSources: [], imageRemoval: false,
    backup: snapshotDraft(), model: selectedModel, effort: selectedEffort, modelChoiceVersion
  };
  queuedEdits.set(selectedId, edit);
  persistQueuedEdits();
  restoreQueuedMessageEdit();
  return true;
}
export async function beginQueuedMessageEdit(threadId, message) {
  const messageId = message?.id;
  if (!threadId || !messageId || selectedId !== threadId || thread?.id !== threadId
      || typeof message.clientUserMessageId !== "string" || !Number.isFinite(message.createdAt)
      || threadReadOnly(threadId) || !connected || busy || stopping || pending || compactionLocked(threadId)
      || queuedActionBusy.has(`${threadId}:${messageId}`)) return;
  if (editingMessage || editingQueuedMessage) { $("#prompt").focus(); return; }
  const version = selectionVersion;
  let attachments;
  setBusy(true); updateComposer();
  try { attachments = await attachmentsFromQueuedMessage(message); }
  catch (error) { notice(error.message || "无法读取排队消息的图片，请刷新后重试", true); return; }
  finally { setBusy(false); updateComposer(); }
  if (selectionVersion !== version || selectedId !== threadId || thread?.id !== threadId
      || editingMessage || editingQueuedMessage || pending || !connected || threadReadOnly(threadId)
      || compactionLocked(threadId)) return;
  rememberDraft();
  const originalImageSources = attachments.map(attachment => attachment.source);
  const imageCount = Array.isArray(message.context?.imageAttachments) ? message.context.imageAttachments.length
    : Array.isArray(message.remoteImages) ? message.remoteImages.length : 0;
  const imageRemoval = (queueCapability("queuedImageRemoval") || queueCapability("queuedImageEditing"))
    && attachments.length === imageCount
    && validQueuedImageSources(originalImageSources);
  const imageEditing = imageRemoval && queueCapability("queuedImageEditing");
  const edit = {
    threadId, editId: randomId(), deviceId: session.device.id, state: "starting",
    message: { id: message.id, clientUserMessageId: message.clientUserMessageId, createdAt: message.createdAt },
    originalText: queueMessageText(message), text: queueMessageText(message),
    attachments, originalImageSources, imageRemoval, imageEditing,
    backup: snapshotDraft(), model: selectedModel, effort: selectedEffort, modelChoiceVersion
  };
  queuedEdits.set(threadId, edit);
  try { persistQueuedEdits(true); }
  catch (error) { queuedEdits.delete(threadId); notice(error.message, true); return; }
  setEditingQueuedMessage(edit);
  closePopup(); closeComposerMenus();
  await requestQueuedMessageEdit(edit);
}
async function requestQueuedMessageEdit(edit) {
  if (!edit || edit.state !== "starting" || busy || pending || !connected || threadReadOnly(edit.threadId)
      || edit.deviceId !== session?.device.id || compactionLocked(edit.threadId)) return;
  const payload = {
    threadId: edit.threadId, messageId: edit.message.id, editId: edit.editId, originalText: edit.originalText,
    clientUserMessageId: edit.message.clientUserMessageId, createdAt: edit.message.createdAt,
    ...(edit.imageRemoval ? { expectedImageSources: edit.originalImageSources } : {})
  };
  const readVersion = queueVersion;
  setBusy(true); updateComposer();
  try {
    const result = await mutate("beginQueuedEdit", payload, { reliable: true });
    completeQueuedEditAction("beginQueuedEdit", payload, result, readVersion);
  } catch (error) {
    if (!pending) finishQueuedMessageEdit(edit);
    reportMutationError(error);
  } finally { setBusy(false); updateComposer(); }
  if (editingQueuedMessage === edit && edit.state === "editing") {
    $("#prompt").focus({ preventScroll: true });
    setPromptSelection($("#prompt"), $("#prompt").value.length);
  }
}
export function finishQueuedMessageEdit(edit) {
  if (!edit) return;
  if (queuedEdits.get(edit.threadId) !== edit) return;
  queuedEdits.delete(edit.threadId);
  if (typeof persistQueuedEditDraft === "function") void persistQueuedEditDraft(edit, true).catch(() => {});
  if (selectedId === edit.threadId && thread?.id === edit.threadId) {
    thread.queuedEdits = (thread.queuedEdits || []).filter(value => value.editId !== edit.editId);
    if (notifiedQueueEdits) setNotifiedQueueEdits(notifiedQueueEdits.filter(value => value.editId !== edit.editId));
  }
  storeDraft(edit.threadId, edit.backup);
  persistQueuedEdits();
  if (editingQueuedMessage === edit) {
    setEditingQueuedMessage(null);
    if (selectedId === edit.threadId && $("#prompt")) {
      restoreDraft(edit.threadId);
      // 用户要求模型/深度只供后续新发送使用；队列编辑结束不能把期间的新选择恢复为旧值。
      if (edit.modelChoiceVersion == null || edit.modelChoiceVersion === modelChoiceVersion) {
        setSelectedModel(edit.model || ""); setSelectedEffort(edit.effort || "");
      }
    }
  }
}
export function completeQueuedEditAction(action, payload, result, readVersion) {
  const edit = queuedEdits.get(payload.threadId);
  applyQueuedSendResult(payload.threadId, result, readVersion);
  if (!edit || edit.editId !== payload.editId || edit.deviceId !== session?.device.id) { updateComposer(); return; }
  if (action === "beginQueuedEdit" && !["committed", "canceled"].includes(result?.editState)) {
    // 用户要求与 Codex 一致：取回后作为未发送输入，不显示编辑提示或取消按钮；
    // 未再次点击发送就不恢复队列，原位置仅在 editQueued 提交时使用。
    edit.state = "editing"; persistQueuedEdits();
    if (editingQueuedMessage === edit && selectedId === edit.threadId) {
      $("#prompt").value = edit.text;
      setSelectedSkills([]); setSelectedGoal(null); setSelectedModel(setSelectedEffort(""));
      setComposerAttachments(edit.attachments || [],
        { readOnly: edit.imageEditing !== true, removableRemote: edit.imageRemoval === true });
      renderSelectedSkills();
    }
  } else finishQueuedMessageEdit(edit);
  updateComposer();
}
export async function submitQueuedMessageEdit() {
  const edit = editingQueuedMessage;
  if (!edit || edit.state !== "editing" || busy || stopping || pending || threadReadOnly(edit.threadId) || !connected
      || edit.threadId !== selectedId || thread?.id !== selectedId || compactionLocked()) return;
  const prompt = $("#prompt").value.trim();
  if (composerAttachmentsLoading()) { notice("图片正在读取，请稍后发送"); return; }
  if (!prompt && !composerHasAttachments()) { notice("请输入消息或保留至少一张图片", true); return; }
  const images = edit.imageEditing ? composerAttachmentsPayload() : [];
  if (images.length && !queueCapability("queuedImageEditing")) {
    notice("当前运行的服务尚不支持编辑队列图片，修改已保留，请更新程序后再发送。", true);
    return;
  }
  try { rememberQueuedMessageEdit(true); }
  catch (error) { notice(error.message, true); return; }
  const { threadId, message } = edit, messageId = message.id;
  const key = `${threadId}:${messageId}`, readVersion = queueVersion;
  if (queuedActionBusy.has(key)) return;
  queuedActionBusy.add(key);
  setBusy(true);
  const payload = {
    threadId,
    editId: edit.editId,
    prompt,
    ...(edit.imageRemoval ? { retainedImageSources: composerRemoteAttachmentSources() } : {}),
    ...(images.length ? { images } : {})
  };
  try {
    // 继续使用取回时的 editId，让服务端按原队列位置提交，不能作为一条新消息追加。
    const request = mutate("editQueued", payload, { reliable: true });
    updateComposer();
    const result = await request;
    completeQueuedEditAction("editQueued", payload, result, readVersion);
    if (selectedId === threadId) $("#prompt").focus({ preventScroll: true });
  } catch (error) {
    reportMutationError(error);
  } finally {
    queuedActionBusy.delete(key);
    setBusy(false); updateComposer();
  }
}
