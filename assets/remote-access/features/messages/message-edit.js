// 已发送消息的编辑：进入/取消编辑、编辑器与提交。

import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  closeComposerMenus, loadSidebar, mutate, renderMessages, scheduleMessages, selectThread,
  updateComposer
} from "../../core/actions.js";
import {
  busy, connected, selectedId, selectionVersion, session, setBusy, stopping, thread, turns
} from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { rememberDraft } from "../composer/drafts.js";
import { currentModelId } from "../composer/model-choice.js";
import { selectedEffort, selectedModel } from "../composer/options-state.js";
import { resizeEditorInput } from "../composer/prompt-editor.js";
import { compactionLocked } from "../conversation/compaction.js";
import { pending, reportMutationError } from "../conversation/pending-state.js";
import { messageCopyText } from "./user-content.js";
import { clearSentMessageDisplays } from "../queue/sent-display.js";
import { editingQueuedMessage, queuedActionBusy } from "../queue/state.js";
import { threadSentMessageEditAllowed } from "../session/action-capabilities.js";
import { isActiveThread, threadReadOnly } from "../session/capabilities.js";

export let editingMessage = null;
export function setEditingMessage(value) { return editingMessage = value; }
export function beginMessageEdit(threadId, item) {
  const metadata = item.messageMetadata;
  if (threadId !== selectedId || thread?.id !== threadId || metadata?.editable !== true
      || threadReadOnly(threadId) || !threadSentMessageEditAllowed(threadId)
      || !connected || busy || stopping || pending || compactionLocked() || isActiveThread()) return;
  if (editingMessage || editingQueuedMessage) { (editingMessage?.input || $("#prompt")).focus(); return; }
  rememberDraft();
  editingMessage = {
    threadId, turnId: metadata.turnId, messageId: metadata.messageId,
    text: messageCopyText(item), error: "",
    ...(thread.capabilities?.messageEditHistory === true
      ? { historyHead: thread.editHistoryHead, clientUserMessageId: randomId() } : {})
  };
  closeComposerMenus(); updateComposer(); renderMessages();
  editingMessage.input?.focus({ preventScroll: true });
}
export function cancelMessageEdit({ focus = true } = {}) {
  if (!editingMessage) return;
  editingMessage = null;
  updateComposer(); scheduleMessages();
  if (focus) $("#prompt")?.focus({ preventScroll: true });
}
function messageEditStale(edit) {
  if (thread?.capabilities?.messageEditHistory === true) {
    // Claude 按每条原生用户消息编辑；保留开始编辑时的历史头，CLI 新增内容后需重新核对编辑目标。
    const item = turns.get(edit.turnId)?.items?.find(item => item.id === edit.messageId && item.type === "userMessage");
    return !edit.historyHead || edit.historyHead !== thread.editHistoryHead || item?.messageMetadata?.editable !== true;
  }
  return Object.hasOwn(thread || {}, "editableMessage")
    && (thread.editableMessage?.turnId !== edit.turnId || thread.editableMessage?.messageId !== edit.messageId);
}
export function updateMessageEditor() {
  const edit = editingMessage;
  if (!edit?.form) return;
  resizeEditorInput(edit.input);
  const locked = busy || !!pending;
  const stale = messageEditStale(edit);
  const unavailable = !connected || threadReadOnly(edit.threadId) || !threadSentMessageEditAllowed(edit.threadId)
    || stopping || compactionLocked() || isActiveThread() || stale;
  const unsupportedModelSettings = !!(selectedModel && selectedModel !== thread?.model
    || selectedEffort && selectedEffort !== thread?.reasoningEffort)
    && thread?.capabilities?.messageEditModelSettings !== true && session?.capabilities?.messageEditModelSettings !== true;
  edit.form.setAttribute("aria-busy", String(locked));
  edit.input.readOnly = locked || unavailable;
  edit.send.disabled = locked || unavailable || unsupportedModelSettings || !edit.text.trim();
  edit.cancel.disabled = locked;
  edit.status.textContent = pending ? "正在确认送达，请勿重复发送。" : edit.error
    || (unsupportedModelSettings ? "当前服务版本不支持编辑消息时切换模型或思考等级，请更新程序后再发送。"
      : unavailable ? "会话状态已变化，暂不能重新发送。可取消后重新编辑。" : "");
  edit.status.hidden = !edit.status.textContent;
}
export function messageEditor() {
  const edit = editingMessage;
  if (!edit.form) {
    const form = element("form", "message-editor");
    const input = element("textarea", "message-editor-input");
    input.setAttribute("aria-label", thread?.capabilities?.messageEditHistory === true ? "编辑历史消息" : "编辑最后一条消息");
    input.value = edit.text; input.rows = 2;
    const cancel = button("取消", "取消编辑消息", () => cancelMessageEdit(), "message-editor-cancel");
    const send = element("button", "message-editor-send", "发送");
    send.type = "submit"; send.setAttribute("aria-label", "发送修改后的消息");
    const actions = element("div", "message-editor-actions");
    const status = element("small", "message-editor-status error-text");
    status.setAttribute("role", "status"); status.hidden = true;
    actions.append(status, cancel, send);
    form.append(input, actions);
    form.onsubmit = event => { event.preventDefault(); void submitMessageEdit(); };
    input.oninput = () => { edit.text = input.value; edit.error = ""; updateMessageEditor(); };
    input.onkeydown = event => {
      if (event.isComposing) return;
      if (event.key === "Escape" && !busy && !pending) { event.preventDefault(); cancelMessageEdit(); }
      else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault(); void submitMessageEdit();
      }
    };
    Object.assign(edit, { form, input, send, cancel, status });
  }
  updateMessageEditor();
  return edit.form;
}
export function queuedEditSubmittingAction() {
  const edit = editingQueuedMessage;
  if (edit?.state !== "editing" || !busy || edit.threadId !== selectedId
      || !queuedActionBusy.has(`${edit.threadId}:${edit.message.id}`)
      || pending?.deviceId !== session?.device.id || pending?.payload?.threadId !== edit.threadId
      || pending.payload.editId !== edit.editId) return "";
  return ["cancelQueuedEdit", "editQueued"].includes(pending.action) ? pending.action : "";
}
async function submitMessageEdit() {
  const edit = editingMessage;
  if (!edit || busy || stopping || pending || threadReadOnly(edit.threadId) || !threadSentMessageEditAllowed(edit.threadId) || !connected
      || edit.threadId !== selectedId || thread?.id !== selectedId || compactionLocked() || isActiveThread()) return;
  if (messageEditStale(edit)) {
    edit.error = thread.capabilities?.messageEditHistory === true
      ? "会话历史已变化，请取消后重新编辑。" : "最后一条消息已变化，请取消后重新编辑。";
    updateMessageEditor(); return;
  }
  const prompt = edit.text.trim(), version = selectionVersion;
  if (!prompt) { notice("请输入修改后的消息"); return; }
  const model = selectedModel || (selectedEffort ? currentModelId() : ""), reasoningEffort = selectedEffort;
  const modelSettingsSupported = thread.capabilities?.messageEditModelSettings === true
    || session.capabilities?.messageEditModelSettings === true;
  if (!modelSettingsSupported && (selectedModel && selectedModel !== thread.model
      || selectedEffort && selectedEffort !== thread.reasoningEffort)) {
    updateMessageEditor(); return;
  }
  edit.error = "";
  closeComposerMenus();
  setBusy(true); updateComposer();
  try {
    if (editingMessage !== edit || selectedId !== edit.threadId || version !== selectionVersion
        || threadReadOnly(edit.threadId) || !threadSentMessageEditAllowed(edit.threadId)
        || !connected || compactionLocked() || isActiveThread())
      throw new Error("会话状态已变化，请重新核对后编辑");
    await mutate("editMessage", {
      threadId: edit.threadId, turnId: edit.turnId, messageId: edit.messageId, prompt,
      ...(edit.historyHead ? { historyHead: edit.historyHead, clientUserMessageId: edit.clientUserMessageId } : {}),
      ...(modelSettingsSupported && model ? { model } : {}),
      ...(modelSettingsSupported && reasoningEffort ? { reasoningEffort } : {})
    }, { reliable: true });
    clearSentMessageDisplays(edit.threadId, { retire: true });
    if (editingMessage === edit) cancelMessageEdit({ focus: false });
    if (version === selectionVersion && selectedId === edit.threadId)
      await selectThread(edit.threadId, { closeSidebar: false, force: true });
    await loadSidebar(null, { fresh: true });
  } catch (error) {
    if (editingMessage === edit && !error.pendingFeedback) edit.error = error.message;
    reportMutationError(error);
  }
  finally { setBusy(false); updateComposer(); }
}
