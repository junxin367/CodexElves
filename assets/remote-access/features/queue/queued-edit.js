// 排队消息编辑：编辑占位、恢复与提交。

import {
  attachmentsFromQueuedMessage, composerAttachmentsLoading, composerAttachmentsPayload,
  composerHasAttachments, composerRemoteAttachmentSources, setComposerAttachments
} from "../../attachments.js";
import { sha256Hex } from "../../sha256.js";
import { copyText } from "../../ui/clipboard.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import {
  busy, connected, selectedId, selectionVersion, session, setBusy, stopping, thread, turns
} from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import {
  inputDraftEmpty, persistQueuedEditDraft, persistQueuedEdits, rememberDraft,
  rememberQueuedMessageEdit, restoreDraft, restoreQueuedMessageEdit, snapshotDraft, storeDraft
} from "../composer/drafts.js";
import {
  modelChoiceVersion, selectedEffort, selectedModel, setSelectedEffort, setSelectedModel
} from "../composer/models.js";
import { setPromptSelection } from "../composer/prompt-editor.js";
import { renderSelectedSkills, setSelectedSkills } from "../composer/skills.js";
import { compactionLocked } from "../conversation/compaction.js";
import {
  mutate, pending, reportMutationError, savePending, validQueuedImageSources, validSkillSelection
} from "../conversation/pending.js";
import { setSelectedGoal } from "../goal/composer.js";
import { editingMessage } from "../messages/message-edit.js";
import {
  applyQueuedSendResult, applyQueueRead, notifiedQueueEdits, queuedActionBusy, queueMessageText,
  queueVersion, reloadQueue, setNotifiedQueueEdits
} from "./queue.js";
import { queueCapability, threadReadOnly } from "../session/capabilities.js";

export let editingQueuedMessage = null;
export function setEditingQueuedMessage(value) { return editingQueuedMessage = value; }
export const queuedEdits = new Map();
export const queuedEditActions = new Set(["beginQueuedEdit", "editQueued", "cancelQueuedEdit", "dismissQueuedEdit"]);
export const queuedEditChecks = new Map();
export let queuedEditCheckVersion = 0;
export function validQueuedMessageEdit(edit) {
  return !!edit && typeof edit.threadId === "string" && typeof edit.editId === "string"
    && typeof edit.deviceId === "string" && typeof edit.message?.id === "string"
    && typeof edit.originalText === "string" && typeof edit.text === "string"
    && typeof edit.backup?.text === "string" && validSkillSelection(edit.backup.skills)
    && (edit.backup.goalMode == null || typeof edit.backup.goalMode === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(edit.backup.goalMode))
    && (!(edit.imageRemoval === true || edit.imageEditing === true)
      || validQueuedImageSources(edit.originalImageSources) && Array.isArray(edit.attachments))
    && !(edit.imageEditing === true && edit.imageRemoval !== true)
    && ["starting", "editing"].includes(edit.state);
}
// 从会话存储恢复未完成的排队消息编辑。
export function restoreQueuedEdits() {
  try {
    const stored = JSON.parse(sessionStorage.getItem("codex-elves-queue-edits") || "[]");
    if (Array.isArray(stored)) for (const edit of stored) {
      if (!validQueuedMessageEdit(edit)) continue;
      queuedEdits.set(edit.threadId, edit);
    }
  } catch {}
}
async function queuedEditFingerprint(action, payload) {
  const value = JSON.stringify([action, action === "editQueued" ? payload.prompt.trim() : null,
    payload.retainedImageSources ?? null,
    (payload.images || []).map(image => [image.name, image.mime, image.dataUrl, image.size])]);
  return sha256Hex(value);
}
export async function checkQueuedMessageEdit(threadId, editId) {
  if (!connected || !queueCapability("queueEditRecovery", threadId)) return null;
  const existing = queuedEditChecks.get(editId), deviceId = session.device.id;
  if (existing?.deviceId === deviceId && existing.promise) return existing.promise;
  const request = { deviceId, loading: true, result: null, error: "", promise: null, queueReadVersion: queueVersion };
  queuedEditChecks.set(editId, request); ++queuedEditCheckVersion; updateComposer();
  request.promise = (async () => {
    try {
      const result = await api("/api/read/queueEdit", { threadId, editId });
      if (session?.device.id !== deviceId || queuedEditChecks.get(editId) !== request) return null;
      if (result.threadId !== threadId || result.editId !== editId) throw new Error("队列编辑状态归属不一致，请重新读取");
      request.result = result;
      if (selectedId === threadId) applyQueueRead(threadId, result, request.queueReadVersion);
      const current = pending;
      if (queuedEditActions.has(current?.action) && current.deviceId === deviceId
          && current.payload.threadId === threadId && current.payload.editId === editId && !result.operationPending) {
        const beginning = current.action === "beginQueuedEdit"
          && ["editing", "committed", "canceled"].includes(result.editState);
        const dismissed = current.action === "dismissQueuedEdit" && result.editState === "missing";
        const terminal = ["committed", "canceled"].includes(result.editState)
          && result.result?.ok === true
          && result.operationFingerprint === await queuedEditFingerprint(current.action, current.payload);
        if (pending === current && (beginning || dismissed || terminal)) {
          savePending(null);
          completeQueuedEditAction(current.action, current.payload,
            { ...result.result, ...result, editState: dismissed ? "dismissed" : result.editState }, queueVersion);
          notice(beginning && result.editState === "editing" ? "排队消息已取回输入框"
            : dismissed ? "已结束待确认编辑，未保存、删除或重发消息" : "队列编辑完成状态已核对");
        }
      }
      return result;
    } catch (error) {
      if (session?.device.id === deviceId && queuedEditChecks.get(editId) === request)
        request.error = error.message || "无法核对队列编辑状态";
      return null;
    } finally {
      request.loading = false; request.promise = null; ++queuedEditCheckVersion;
      if (session?.device.id === deviceId) updateComposer();
    }
  })();
  return request.promise;
}
export function renderQueuedEditRecoveryControls(root, threadId, editId) {
  if (session?.capabilities?.queueEditRecovery !== true) {
    root.append(element("span", "queued-edit-recovery-text", "更新远程服务后可在此核对和结束待确认编辑。"));
    return;
  }
  const check = queuedEditChecks.get(editId);
  const result = check?.deviceId === session.device.id ? check.result : null;
  const actions = element("div", "queued-edit-recovery-actions");
  const read = button(check?.loading ? "核对中…" : "核对编辑状态", "只读取本机编辑状态，不会重新保存或恢复消息",
    () => void checkQueuedMessageEdit(threadId, editId), "text-button");
  read.disabled = !connected || !!check?.loading;
  actions.append(read);
  const edit = queuedEdits.get(threadId);
  const draft = edit?.editId === editId ? edit.text : result?.draftText || "";
  if (draft) actions.append(button("复制修改文字", "复制本次编辑的文字，不会发送消息", () => void copyText(draft), "text-button"));
  if (result?.canResume && editingQueuedMessage?.editId === editId
      && queuedEditActions.has(pending?.action) && pending.payload.editId === editId) {
    const resume = button("继续编辑", "核对后继续本机保留的编辑，不会重发上次请求",
      () => void resumePendingQueuedMessageEdit(threadId, editId), "text-button");
    resume.disabled = !connected || threadReadOnly(threadId) || busy || !!check?.loading;
    actions.append(resume);
  }
  if (result?.canDismiss) {
    const finish = button("结束待确认编辑", "核对后只结束编辑标记，不会修改队列或重发消息",
      () => void dismissQueuedMessageEdit(threadId, editId), "text-button");
    finish.disabled = !connected || threadReadOnly(threadId) || busy || !!check?.loading;
    actions.append(finish);
  }
  root.append(actions);
  if (check?.error) root.append(element("p", "queued-edit-recovery-text", check.error));
  else if (result) root.append(element("p", "queued-edit-recovery-text", result.operationPending
    ? "本机仍在处理此次编辑，请稍后再次核对。"
    : ["committed", "canceled"].includes(result.editState)
      ? "本机已完成编辑；若当前草稿与已完成操作不同，请保留草稿后核对。"
      : result.canDismiss
      ? "无法确认上次保存或恢复是否完成。请先核对会话和当前队列；需要保留的修改请先复制。"
        : "本机编辑仍可继续，请勿重复取出原消息。"));
}
async function resumePendingQueuedMessageEdit(threadId, editId) {
  const current = pending, deviceId = session?.device.id;
  if (!queuedEditActions.has(current?.action) || current.deviceId !== deviceId
      || current.payload.threadId !== threadId || current.payload.editId !== editId
      || editingQueuedMessage?.editId !== editId || selectedId !== threadId || busy || threadReadOnly(threadId)) return;
  if (!await confirmAction("继续队列编辑",
    "本机仍保留可恢复的编辑。继续只解除此页的待确认锁定，不会重发上次请求，也不会自动保存或取消；后续保存和取消仍由本机编辑标识防止重复恢复。",
    "继续编辑")) return;
  const result = await checkQueuedMessageEdit(threadId, editId);
  if (pending !== current || session?.device.id !== deviceId || selectedId !== threadId
      || editingQueuedMessage?.editId !== editId || !result?.canResume || !connected || threadReadOnly(threadId) || busy) return;
  savePending(null); updateComposer(); $("#prompt").focus({ preventScroll: true });
}
async function dismissQueuedMessageEdit(threadId, editId) {
  const deviceId = session?.device.id, check = queuedEditChecks.get(editId);
  if (!check?.result?.canDismiss || !connected || threadReadOnly(threadId) || busy
      || selectedId !== threadId || thread?.id !== threadId) return;
  if (pending && (!queuedEditActions.has(pending.action) || pending.payload.editId !== editId
      || pending.payload.threadId !== threadId || pending.deviceId !== deviceId)) return;
  if (!await confirmAction("结束待确认编辑",
    "请先核对会话和当前队列。此操作只结束编辑标记，不会保存、恢复、删除或重发队列消息。结束后恢复编辑前的草稿；本次修改及附件不会重新提交，需要保留请先复制或备份。",
    "已核对，结束编辑", true)) return;
  if (session?.device.id !== deviceId || selectedId !== threadId || !connected || threadReadOnly(threadId) || busy) return;
  if (pending && (!queuedEditActions.has(pending.action) || pending.payload.editId !== editId
      || pending.payload.threadId !== threadId || pending.deviceId !== deviceId)) return;
  const readVersion = queueVersion;
  const previousPending = pending;
  if (pending) savePending(null);
  setBusy(true); updateComposer();
  let dismissed = false;
  try {
    const payload = { threadId, editId, confirmed: true };
    const result = await mutate("dismissQueuedEdit", payload, { reliable: true });
    dismissed = true;
    completeQueuedEditAction("dismissQueuedEdit", payload, result, readVersion);
    if (selectedId === threadId) await reloadQueue();
    notice("已结束待确认编辑，未保存、删除或重发消息");
  } catch (error) {
    if (!dismissed && !pending && previousPending && session?.device.id === deviceId) savePending(previousPending);
    notice(error.message, true);
  }
  finally { setBusy(false); updateComposer(); }
}
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
  editingQueuedMessage = edit;
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
    editingQueuedMessage = null;
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
export function syncEditableMessageMetadata() {
  // 任意历史编辑能力由逐条原生元数据提供；不能再用“最后一条”的单值覆盖全部消息。
  if (thread?.capabilities?.messageEditHistory === true) return;
  if (thread?.id !== selectedId || !Object.hasOwn(thread, "editableMessage")) return;
  const editable = thread.editableMessage;
  for (const [turnId, turn] of turns) {
    let changed = false;
    const items = (turn.items || []).map(item => {
      if (item.type !== "userMessage") return item;
      const allowed = editable?.turnId === turnId && editable.messageId === item.id;
      if (item.messageMetadata?.editable === allowed) return item;
      changed = true;
      return { ...item, messageMetadata: { ...item.messageMetadata, turnId, messageId: item.id, editable: allowed } };
    });
    if (changed) turns.set(turnId, { ...turn, items });
  }
}
