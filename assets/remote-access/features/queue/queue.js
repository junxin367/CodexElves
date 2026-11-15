// 排队消息：队列快照、拖动排序、引导与删除及队列区渲染。

import { renderQueuedMessageAttachments } from "../../attachments.js";
import { recordDiagnostic } from "../../diagnostics.js";
import { performAndroidHapticFeedback } from "../../platform.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { closePopup, openPopup, togglePopup } from "../../ui/popup.js";
import { busy, connected, selectedId, selectionVersion, session, setBusy, stopping, thread } from "../../core/state.js";
import { api } from "../api.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import { persistQueuedEditDraft, persistQueuedEdits, recoveryGeneration } from "../composer/drafts.js";
import { compactionLocked } from "../conversation/compaction.js";
import { scheduleReload } from "../conversation/events.js";
import { renderMessages } from "../conversation/history-view.js";
import { reloadThread } from "../conversation/history.js";
import { mutate, pending, reportMutationError } from "../conversation/pending.js";
import { editingMessage } from "../messages/message-edit.js";
import {
  beginQueuedMessageEdit, editingQueuedMessage, finishQueuedMessageEdit, queuedEditActions,
  queuedEditCheckVersion, queuedEdits, renderQueuedEditRecoveryControls,
  restoreUnfinishedQueuedMessageEdit
} from "./queued-edit.js";
import {
  confirmedSendDisplayFor, confirmSendDisplay, finishSendDisplay, reconcileSentMessageDisplays,
  rememberSendDisplay, sentMessageDisplayFor
} from "./sent-display.js";
import { isActiveThread, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { currentThreadDisplay, rememberThreadDisplay } from "../session/thread-cache.js";

export let queueVersion = 0, notifiedQueue = null, notifiedQueueEdits = null, queueRequest = null;
export function setQueueVersion(value) { return queueVersion = value; }
export function setNotifiedQueue(value) { return notifiedQueue = value; }
export function setNotifiedQueueEdits(value) { return notifiedQueueEdits = value; }
export let queueSnapshotVersion = null;
export function setQueueSnapshotVersion(value) { return queueSnapshotVersion = value; }
export const retiredQueueEpochs = new Set();
export let queueDrag = null, queueReorder = null;
export let pendingSteer = null;
export function setPendingSteer(value) { return pendingSteer = value; }
export const queuedActionBusy = new Set();
const sameQueueOrder = (left, right) => left.length === right.length && left.every((id, index) => id === right[index]);
function queueReorderAvailable() {
  const messages = thread?.queuedMessages;
  return (thread?.backend !== "claude" || thread.capabilities?.remoteQueue === true)
    && !!selectedId && thread?.id === selectedId && connected && !threadReadOnly()
    && !busy && !stopping && !pending && !compactionLocked() && !queuedActionBusy.size
    && editingQueuedMessage?.state !== "starting"
    && Array.isArray(messages) && messages.length > 1
    && messages.every(message => message?.id && message.remoteDeliveryState !== "accepted"
      && !confirmedSendDisplayFor(message)
      && (!message.submission?.status || message.submission.status === "queued"));
}
function moveQueueItem(ids, messageId, index) {
  const order = ids.filter(id => id !== messageId);
  order.splice(Math.max(0, Math.min(index, order.length)), 0, messageId);
  return order;
}
function queueDisplayMessages(messages) {
  if (!Array.isArray(messages)) return messages;
  const reorder = queueReorder?.deviceId === session?.device.id ? queueReorder
    : pending?.action === "reorderQueued" && pending.deviceId === session?.device.id ? pending.payload : null;
  if (reorder?.threadId !== selectedId
      || !sameQueueOrder(messages.map(message => message.id), reorder.expectedMessageIds))
    return messages.filter(message => !confirmedSendDisplayFor(message));
  const byId = new Map(messages.map(message => [message.id, message]));
  return reorder.messageIds.map(id => byId.get(id)).filter(message => message && !confirmedSendDisplayFor(message));
}
export function applyQueueReorderResult(id, result, readVersion) {
  if (id !== selectedId || thread?.id !== id) return;
  if (!Array.isArray(result?.queuedMessages)
      || readVersion !== queueVersion && !queueSnapshotIsVersioned(result)
      || !applyQueueSnapshot(id, result.queuedMessages, result.queuedEdits, result))
    void reloadQueue().catch(error => notice(`队列顺序同步失败：${error.message}`, true));
}
async function reorderQueuedMessages(threadId, expectedMessageIds, messageIds, focusId = null) {
  if (threadId !== selectedId || !queueReorderAvailable()
      || sameQueueOrder(expectedMessageIds, messageIds)) return;
  if (!sameQueueOrder((thread.queuedMessages || []).map(message => message.id), expectedMessageIds)) {
    notice("队列已变化，请重新调整顺序", true);
    return;
  }
  const payload = { threadId, messageIds, expectedMessageIds }, readVersion = queueVersion;
  const preview = { ...payload, deviceId: session.device.id };
  queueReorder = preview;
  setBusy(true); updateComposer();
  try {
    const result = await mutate("reorderQueued", payload, { reliable: true });
    if (session?.device.id === preview.deviceId) applyQueueReorderResult(threadId, result, readVersion);
  } catch (error) {
    reportMutationError(error);
    if (!pending && selectedId === threadId) void reloadQueue().catch(error => notice(error.message, true));
  } finally {
    if (queueReorder === preview) queueReorder = null;
    setBusy(false); updateComposer();
    if (focusId && selectedId === threadId) {
      const row = [...document.querySelectorAll(".queued-message[data-message-id]")].find(row => row.dataset.messageId === focusId);
      if (row) {
        $(".queued-drag-handle", row)?.focus({ preventScroll: true });
        const queue = row.closest(".queued-messages");
        const bounds = queue.getBoundingClientRect(), rowBounds = row.getBoundingClientRect();
        if (rowBounds.top < bounds.top) queue.scrollTop -= bounds.top - rowBounds.top;
        else if (rowBounds.bottom > bounds.bottom) queue.scrollTop += rowBounds.bottom - bounds.bottom;
      }
    }
  }
}
export function finishQueueDrag({ commit = false, render = true, hapticEnd = false } = {}) {
  const drag = queueDrag;
  if (!drag) return;
  queueDrag = null;
  clearTimeout(drag.timer); cancelAnimationFrame(drag.frame);
  document.removeEventListener("pointermove", drag.move);
  document.removeEventListener("pointerup", drag.up);
  document.removeEventListener("pointercancel", drag.cancel);
  drag.queue.removeEventListener("lostpointercapture", drag.cancel);
  if (drag.queue.hasPointerCapture?.(drag.pointerId)) drag.queue.releasePointerCapture(drag.pointerId);
  drag.row.classList.remove("queued-dragging");
  drag.row.removeAttribute("aria-grabbed");
  drag.queue.classList.remove("queue-reordering");
  const root = $("#queued-area");
  if (root) delete root.dataset.queueSignature;
  if (commit && drag.active && selectedId === drag.threadId
      && !sameQueueOrder(drag.ids, drag.order) && queueReorderAvailable()) {
    const hapticEndAllowed = sameQueueOrder((thread?.queuedMessages || []).map(message => message.id), drag.ids);
    void reorderQueuedMessages(drag.threadId, drag.ids, drag.order);
    if (hapticEnd && drag.userInitiated && hapticEndAllowed) performAndroidHapticFeedback("drag-end");
  } else if (render) renderQueueArea();
}
function moveQueueDrag(drag) {
  if (queueDrag !== drag || !drag.active) return;
  const previousOrder = [...drag.queue.children].map(row => row.dataset.messageId);
  const rows = [...drag.queue.children].filter(row => row !== drag.row);
  const before = rows.find(row => {
    const rect = row.getBoundingClientRect();
    return drag.y < rect.top + rect.height / 2;
  });
  if (drag.row.nextElementSibling !== (before || null)) drag.queue.insertBefore(drag.row, before || null);
  const order = [...drag.queue.children].map(row => row.dataset.messageId);
  drag.order = order;
  if (drag.userInitiated && !sameQueueOrder(previousOrder, order))
    performAndroidHapticFeedback("drag-tick");
}
function scrollQueueDrag(drag) {
  if (queueDrag !== drag || !drag.active || drag.frame != null) return;
  const scroll = drag.queue;
  if (!scroll || scroll.scrollHeight <= scroll.clientHeight) return;
  const rect = scroll.getBoundingClientRect();
  const direction = drag.y < rect.top + 28 ? -1 : drag.y > rect.bottom - 28 ? 1 : 0;
  if (!direction) return;
  drag.frame = requestAnimationFrame(() => {
    drag.frame = null;
    if (queueDrag !== drag) return;
    const before = scroll.scrollTop;
    scroll.scrollTop += direction * 8;
    if (before !== scroll.scrollTop) { moveQueueDrag(drag); scrollQueueDrag(drag); }
  });
}
function beginQueueDrag(event, handle, row, queue, messageId) {
  if (event.button !== 0 || event.isPrimary === false || queueDrag || !queueReorderAvailable()) return;
  event.preventDefault();
  closePopup(); closeComposerMenus();
  const drag = {
    threadId: selectedId, ids: thread.queuedMessages.map(message => message.id),
    messageId, handle, row, queue, pointerId: event.pointerId,
    x: event.clientX, y: event.clientY, startY: event.clientY,
    userInitiated: event.isTrusted === true, active: false, order: [], timer: null, frame: null
  };
  queueDrag = drag;
  const activate = () => {
    if (queueDrag !== drag || !handle.isConnected || !queueReorderAvailable()) { finishQueueDrag(); return; }
    if (drag.active) return;
    drag.active = true; drag.order = [...drag.ids];
    row.classList.add("queued-dragging"); row.setAttribute("aria-grabbed", "true");
    queue.classList.add("queue-reordering");
    if (drag.userInitiated) performAndroidHapticFeedback("drag-start");
  };
  const touch = event.pointerType !== "mouse";
  if (touch) drag.timer = setTimeout(activate, 240);
  drag.move = move => {
    if (queueDrag !== drag || move.pointerId !== drag.pointerId) return;
    const distance = Math.hypot(move.clientX - drag.x, move.clientY - drag.startY);
    if (!drag.active) {
      if (touch && distance > 10) { finishQueueDrag(); return; }
      if (!touch && distance >= 4) activate();
    }
    if (!drag.active) return;
    move.preventDefault(); drag.y = move.clientY;
    moveQueueDrag(drag); scrollQueueDrag(drag);
  };
  drag.up = up => {
    if (up.pointerId === drag.pointerId)
      finishQueueDrag({ commit: true, hapticEnd: up.isTrusted === true });
  };
  drag.cancel = cancel => {
    if (cancel.pointerId !== drag.pointerId || cancel.type === "lostpointercapture" && cancel.target !== queue) return;
    finishQueueDrag({ commit: cancel.type === "lostpointercapture" && cancel.buttons === 0 });
  };
  document.addEventListener("pointermove", drag.move, { passive: false });
  document.addEventListener("pointerup", drag.up);
  document.addEventListener("pointercancel", drag.cancel);
  queue.addEventListener("lostpointercapture", drag.cancel);
  try { queue.setPointerCapture(event.pointerId); } catch {}
}
export function queueSnapshotIsVersioned(snapshot) {
  return typeof snapshot?.queueEpoch === "string" && snapshot.queueEpoch.length > 0
    && Number.isSafeInteger(snapshot.queueRevision) && snapshot.queueRevision >= 0;
}
export function queueMessageIdentity(message) {
  return message?.clientUserMessageId ?? message?.submissionOptions?.clientUserMessageId
    ?? (!message?.serverQueuedMessageId ? message?.id : null);
}
function queueSnapshotRemovedMessage(previous, next) {
  const nextIds = new Set((next || []).map(queueMessageIdentity).filter(Boolean));
  return (previous || []).some(message => {
    const id = queueMessageIdentity(message);
    return id && !nextIds.has(id);
  });
}
function reconcileQueuedEditRecovery(threadId, edits) {
  if (!Array.isArray(edits)) return;
  const activeIds = new Set(edits.map(edit => edit?.editId).filter(id => typeof id === "string"));
  for (const [id, edit] of queuedEdits) {
    if (id !== threadId || activeIds.has(edit.editId)) continue;
    const current = pending;
    if (queuedEditActions.has(current?.action) && current.deviceId === session?.device?.id
        && current.payload?.threadId === threadId && current.payload?.editId === edit.editId) continue;
    if (editingQueuedMessage === edit) finishQueuedMessageEdit(edit);
    else {
      queuedEdits.delete(id);
      persistQueuedEdits();
      if (typeof persistQueuedEditDraft === "function") void persistQueuedEditDraft(edit, true).catch(() => {});
    }
  }
}
export function applyQueueSnapshot(id, messages, edits, snapshot = null, { authoritative = false, readVersion = null } = {}) {
  if (id !== selectedId || !Array.isArray(messages)) return false;
  const previousMessages = thread?.id === id && Array.isArray(thread.queuedMessages)
    ? thread.queuedMessages : Array.isArray(notifiedQueue) ? notifiedQueue : [];
  const removedMessage = queueSnapshotRemovedMessage(previousMessages, messages);
  if (queueSnapshotIsVersioned(snapshot)) {
    if (retiredQueueEpochs.has(snapshot.queueEpoch)) return true;
    if (queueSnapshotVersion?.queueEpoch === snapshot.queueEpoch
        && snapshot.queueRevision <= queueSnapshotVersion.queueRevision) return true;
    if (queueSnapshotVersion && queueSnapshotVersion.queueEpoch !== snapshot.queueEpoch) {
      if (!authoritative || readVersion !== null && readVersion !== queueVersion) return false;
      retiredQueueEpochs.add(queueSnapshotVersion.queueEpoch);
    }
    queueSnapshotVersion = { queueEpoch: snapshot.queueEpoch, queueRevision: snapshot.queueRevision };
  } else {
    if (queueSnapshotVersion) return false;
    queueSnapshotVersion = null;
  }
  notifiedQueue = messages; ++queueVersion;
  if (Array.isArray(edits)) notifiedQueueEdits = edits;
  reconcileQueuedEditRecovery(id, edits);
  if (thread?.id === id) {
    thread.queuedMessages = messages;
    if (Array.isArray(edits)) thread.queuedEdits = edits;
  }
  rememberThreadDisplay({ id, queuedMessages: messages, queueSnapshotVersion });
  const sentDisplayChanged = reconcileSentMessageDisplays(id, messages);
  if (typeof restoreUnfinishedQueuedMessageEdit === "function" && restoreUnfinishedQueuedMessageEdit()) updateComposer();
  else renderQueueArea();
  if (sentDisplayChanged || messages.some(message => confirmedSendDisplayFor(message))) renderMessages();
  if (removedMessage) scheduleReload();
  return true;
}
export function applyQueueRead(id, snapshot, readVersion) {
  if (!Array.isArray(snapshot?.queuedMessages)) return true;
  if (readVersion !== queueVersion && !queueSnapshotIsVersioned(snapshot)) return true;
  const applied = applyQueueSnapshot(id, snapshot.queuedMessages, snapshot.queuedEdits, snapshot,
    { authoritative: true, readVersion });
  if (!applied && id === selectedId)
    void reloadQueue().catch(error => notice(`队列正在重新核对：${error.message}`, true));
  return applied;
}
export async function reloadQueue() {
  if (!selectedId) return;
  if (queueRequest?.version === selectionVersion) {
    queueRequest.again = true;
    return queueRequest.promise;
  }
  const id = selectedId, version = selectionVersion;
  const request = { version, again: false, promise: null };
  request.promise = (async () => {
    do {
      request.again = false;
      const readVersion = queueVersion;
      const result = await api("/api/read/queue", { threadId: id });
      if (version !== selectionVersion || selectedId !== id) return;
      if ((readVersion === queueVersion || queueSnapshotIsVersioned(result))
          && !applyQueueSnapshot(id, result.queuedMessages, result.queuedEdits, result, { authoritative: true, readVersion }))
        request.again = true;
    } while (request.again && version === selectionVersion && selectedId === id);
  })();
  queueRequest = request;
  try { await request.promise; }
  finally { if (queueRequest === request) queueRequest = null; }
}
export function applyQueuedSendResult(id, result, readVersion, payload = null) {
  if (!id || result?.threadId && result.threadId !== id) return false;
  if (payload && confirmSendDisplay(payload, result) && id === selectedId) {
    renderMessages();
    scheduleReload();
  }
  if (!Array.isArray(result?.queuedMessages) && !result?.queueSyncPending) return false;
  if (id !== selectedId) reconcileSentMessageDisplays(id, result.queuedMessages, { removeMissing: false });
  if (id === selectedId) {
    if (!Array.isArray(result.queuedMessages)
        || readVersion !== queueVersion && !queueSnapshotIsVersioned(result)
        || !applyQueueSnapshot(id, result.queuedMessages, result.queuedEdits, result))
      void reloadQueue().catch(error => recordDiagnostic("remote.send.queue", {
        phase: "failure", errorType: error?.name || "Error"
      }));
  }
  return true;
}
export function queueMessageText(message) {
  return message?.text ?? message?.context?.prompt ?? message?.prompt ?? message?.message?.prompt ?? "";
}
export function confirmQueuedSteerDisplay(id, messageId, result) {
  let record = sentMessageDisplayFor({ id: messageId }, id);
  if (!record && thread?.id === id) {
    const message = thread.queuedMessages?.find(message => message.id === messageId);
    if (message) {
      record = rememberSendDisplay({
        threadId: id, clientUserMessageId: queueMessageIdentity(message) || messageId,
        prompt: queueMessageText(message), images: message.context?.imageAttachments || message.images || [],
        skills: message.context?.skills || message.skills || []
      });
      if (record) record.messageId = messageId;
    }
  }
  if (record) confirmSendDisplay(record.payload, result, { steering: true });
}
function renderQueuedMessages(messages, previous = null) {
  messages = queueDisplayMessages(messages);
  if (editingQueuedMessage?.threadId === selectedId && editingQueuedMessage.state === "editing")
    messages = messages?.filter(message => message?.id !== editingQueuedMessage.message.id);
  if (!Array.isArray(messages) || !messages.length) return null;
  const threadId = selectedId, active = isActiveThread();
  const previousAttachments = new Map([...previous?.querySelectorAll(".queued-message") || []]
    .map(row => [row.dataset.messageId, $(".queued-message-attachments", row)]));
  const queue = element("section", "queued-messages");
  const acceptedCount = messages.filter(message => message?.remoteDeliveryState === "accepted").length;
  queue.setAttribute("aria-label", `${thread?.capabilities?.remoteQueue === true ? "远程待发送队列" : "原生消息队列"}，${messages.length - acceptedCount} 条待执行`
    + (acceptedCount ? `，${acceptedCount} 条已发送待同步` : ""));
  queue.setAttribute("role", "list");
  for (const message of messages) {
    const row = element("div", "queued-message");
    const messageId = message?.id;
    const accepted = message?.remoteDeliveryState === "accepted";
    row.dataset.messageId = messageId || "";
    row.setAttribute("role", "listitem");
    const key = `${threadId}:${messageId}`;
    const awaitingReceipt = message.submission?.status === "outcome-unknown"
      || pending?.action === "steerQueued" && pending.payload?.threadId === threadId
        && pending.payload?.messageId === messageId;
    // 用户要求按钮符合实际操作：暂停消息在会话恢复执行后用于引导，不能沿用旧 pausedReason 显示重试。
    const retryable = !active && !accepted && !awaitingReceipt
      && typeof message.pausedReason === "string" && !!message.pausedReason.trim()
      && (!message.submission?.status || message.submission.status === "queued");
    const queueIcon = button("", "拖动调整排队顺序", event => event.preventDefault(), "queued-message-icon queued-drag-handle");
    queueIcon.title = "拖动消息或左侧手柄排序；手机请长按手柄，也可使用上下方向键";
    queueIcon.setAttribute("aria-keyshortcuts", "ArrowUp ArrowDown");
    queueIcon.disabled = !queueReorderAvailable();
    queueIcon.hidden = threadReadOnly(threadId);
    row.dataset.reorderable = String(!queueIcon.disabled);
    queueIcon.append(icon("queue"));
    queueIcon.onpointerdown = event => beginQueueDrag(event, queueIcon, row, queue, messageId);
    queueIcon.oncontextmenu = event => { event.preventDefault(); event.stopPropagation(); };
    queueIcon.ondragstart = event => event.preventDefault();
    row.onpointerdown = event => {
      if (event.pointerType !== "mouse" || event.target.closest("button,a,input,textarea,select")) return;
      beginQueueDrag(event, queueIcon, row, queue, messageId);
    };
    queueIcon.onkeydown = event => {
      if (!["ArrowUp", "ArrowDown"].includes(event.key) || queueDrag || !queueReorderAvailable()) return;
      event.preventDefault();
      const ids = thread.queuedMessages.map(message => message.id);
      const next = moveQueueItem(ids, messageId,
        ids.indexOf(messageId) + (event.key === "ArrowUp" ? -1 : 1));
      if (event.isTrusted === true && !sameQueueOrder(ids, next))
        performAndroidHapticFeedback("drag-tick");
      void reorderQueuedMessages(threadId, ids, next, messageId);
    };
    const attachmentSignature = JSON.stringify([message.remoteImages, message.context?.imageAttachments]);
    const previousAttachment = previousAttachments.get(messageId);
    const thumbnail = previousAttachment?.querySelector("img");
    const attachments = previousAttachment?.remoteAttachmentSignature === attachmentSignature
        && thumbnail && (!thumbnail.complete || thumbnail.naturalWidth > 0)
      ? previousAttachment : typeof renderQueuedMessageAttachments === "function"
        ? renderQueuedMessageAttachments(message) : null;
    if (attachments) attachments.remoteAttachmentSignature = attachmentSignature;
    row.append(queueIcon, ...(attachments ? [attachments] : []),
      element("p", "queued-message-text", queueMessageText(message) || "待执行消息"));
    const unavailable = !threadId || !messageId || thread?.id !== threadId
      || !Array.isArray(thread.queuedMessages) || threadReadOnly(threadId) || !connected
      || busy || stopping || !!pending || queuedActionBusy.has(key) || compactionLocked(threadId)
      || !!message.submission?.status && message.submission.status !== "queued";
    const steering = queuedActionBusy.has(key) && pending?.action === "steerQueued"
      && pending.payload?.threadId === threadId && pending.payload?.messageId === messageId;
    const steer = button("", accepted ? "消息已发送，队列记录待同步"
      : steering ? "正在引导此队列消息" : awaitingReceipt ? "队列消息结果待确认" : retryable ? "重试此队列消息" : "引导此队列消息",
      () => void steerQueuedMessage(threadId, messageId), "queued-steer-button");
    steer.append(steering ? element("span", "spinner") : icon(retryable ? "retry" : "steer"),
      element("span", "", accepted ? "已发送" : steering ? "引导中" : awaitingReceipt ? "待确认" : retryable ? "重试" : "引导"));
    // 异常恢复沿用原有手动重试入口；显示服务端暂停原因，不能因重新打开页面自动恢复或重发。
    if (retryable) steer.title = message.pausedReason;
    steer.disabled = accepted || awaitingReceipt || unavailable || message.canSendNow === false;
    const menu = button("", "排队消息操作", () => togglePopup(menu), "queued-menu-button");
    menu.id = `queued-menu-${messageId}`;
    menu.append(icon("more"));
    menu.setAttribute("aria-haspopup", "menu");
    menu.setAttribute("aria-expanded", "false");
    menu.disabled = unavailable || thread?.backend === "claude" && accepted;
    menu.popupClassName = "queued-message-popup";
    menu.menuOptions = [
      {
        label: "编辑消息",
        icon: "edit",
        disabled: accepted || !!editingQueuedMessage || !!editingMessage || busy || !!pending
          || typeof message?.clientUserMessageId !== "string" || !Number.isFinite(message?.createdAt),
        action: () => beginQueuedMessageEdit(threadId, message)
      },
      {
        label: accepted ? "移除已发送记录" : "删除排队消息",
        value: "remove",
        icon: "trash",
        action: () => void removeQueuedMessage(threadId, messageId)
      }
    ];
    menu.onkeydown = event => {
      if (["ArrowDown", "ArrowUp"].includes(event.key)) {
        event.preventDefault();
        openPopup(menu, { edge: event.key === "ArrowUp" ? "last" : "first" });
      }
    };
    row.oncontextmenu = event => {
      event.preventDefault();
      if (thread?.backend === "claude" && (thread.capabilities?.remoteQueue !== true || accepted)) return;
      if (queueDrag || event.target.closest(".queued-drag-handle")) return;
      openPopup(menu, { position: { x: event.clientX, y: event.clientY } });
    };
    // 用户改为远程服务保存待发消息：具备 remoteQueue 时允许逐条引导、编辑、删除和排序；
    // 已投递给原生的旧记录仍只展示接收状态，不能伪装成可修改的未发送消息。
    if (thread?.backend === "claude" && thread.capabilities?.remoteQueue !== true && thread.capabilities?.queueSendNow === true
        && message === messages[0] && !threadReadOnly(threadId)) {
      const sendAll = button("立即执行全部", "停止当前回复并执行全部排队消息，保留原版 CLI 未发送草稿",
        () => void steerQueuedMessage(threadId, messageId), "queued-steer-button");
      sendAll.disabled = !active || unavailable || awaitingReceipt || accepted;
      row.append(sendAll);
    }
    if (!threadReadOnly(threadId) && (thread?.backend !== "claude" || thread.capabilities?.remoteQueue === true))
      row.append(steer, ...(thread?.backend === "claude" && accepted ? [] : [menu]));
    if (thread?.backend === "claude" && thread.capabilities?.remoteQueue !== true) queueIcon.hidden = true;
    queue.append(row);
  }
  return queue;
}
export function renderQueueArea() {
  const root = $("#queued-area");
  if (!root) return;
  const messages = currentThreadDisplay()?.queuedMessages;
  const ready = thread?.id === selectedId && Array.isArray(thread.queuedMessages);
  // 队列内容可保持不变，但停止/恢复会改变按钮文案与图标，必须纳入刷新判断。
  const signature = JSON.stringify([selectedId, messages, ready, isActiveThread(), thread?.queuedEdits,
    thread?.capabilities?.queueSendNow, thread?.capabilities?.remoteQueue, session?.device.id, threadReadOnly(), connected,
    compactionLocked(), [...queuedActionBusy], editingQueuedMessage?.message.id, editingQueuedMessage?.state,
    !!editingMessage, busy, stopping, pending?.id, queueReorder, queuedEditCheckVersion,
    queueDisplayMessages(messages)?.map(message => message.id)]);
  if (root.dataset.queueSignature === signature) return;
  const previous = root.dataset.queueThread === selectedId && root.dataset.queueDevice === session?.device.id
    ? root.querySelector(".queued-messages") : null;
  const scrollTop = previous?.scrollTop || 0;
  if (queueDrag) {
    const changed = queueDrag.threadId === selectedId
      && !sameQueueOrder(queueDrag.ids, (messages || []).map(message => message.id));
    finishQueueDrag({ render: false });
    if (changed) notice("队列已更新，拖动已取消，请重新调整顺序");
  }
  root.dataset.queueSignature = signature;
  root.dataset.queueThread = selectedId || "";
  root.dataset.queueDevice = session?.device.id || "";
  const queue = renderQueuedMessages(messages, previous);
  const recovery = !threadReadOnly() && !editingQueuedMessage && thread?.queuedEdits?.find(edit => edit.state !== "editing"
    && typeof edit.editId === "string"
    && !(queuedEditActions.has(pending?.action) && pending.payload?.editId === edit.editId));
  const children = queue ? [queue] : [];
  if (recovery) {
    const row = element("div", "queued-edit-recovery");
    const status = element("p", "queued-edit-recovery-text", "正在确认队列编辑结果。");
    row.append(status);
    renderQueuedEditRecoveryControls(row, selectedId, recovery.editId);
    children.push(row);
  }
  root.hidden = !children.length;
  root.replaceChildren(...children);
  if (queue) queue.scrollTop = scrollTop;
}
async function removeQueuedMessage(threadId, messageId) {
  if (!threadId || !messageId || threadReadOnly(threadId) || !connected || busy || stopping || pending) return;
  const operationScope = privateInputScope(session), generation = recoveryGeneration;
  const currentAuthorization = () => operationScope === privateInputScope(session) && generation === recoveryGeneration;
  const key = `${threadId}:${messageId}`;
  if (queuedActionBusy.has(key)) return;
  queuedActionBusy.add(key);
  if (selectedId === threadId) renderQueueArea();
  const readVersion = queueVersion;
  try {
    const request = mutate("removeQueued", { threadId, messageId }, { reliable: true });
    updateComposer();
    const result = await request;
    if (!currentAuthorization() || selectedId !== threadId || !thread) return;
    const applied = (readVersion === queueVersion || queueSnapshotIsVersioned(result))
      && applyQueueSnapshot(threadId, Array.isArray(result?.queuedMessages)
          ? result.queuedMessages
          : (thread.queuedMessages || []).filter(message => message?.id !== messageId), result?.queuedEdits, result);
    if (!applied || result?.queueSyncPending)
      void reloadQueue().catch(error => { if (currentAuthorization()) notice(error.message, true); });
  } catch (error) {
    if (currentAuthorization()) reportMutationError(error);
  } finally {
    queuedActionBusy.delete(key);
    if (currentAuthorization() && selectedId === threadId) updateComposer();
  }
}
async function steerQueuedMessage(threadId, messageId) {
  if (!threadId || !messageId || threadReadOnly(threadId) || !connected || busy || stopping || pending
      || compactionLocked(threadId)) return;
  const operationScope = privateInputScope(session), generation = recoveryGeneration;
  const currentAuthorization = () => operationScope === privateInputScope(session) && generation === recoveryGeneration;
  const key = `${threadId}:${messageId}`;
  if (queuedActionBusy.has(key)) return;
  const queuedMessage = thread?.id === threadId
    ? thread.queuedMessages?.find(message => message?.id === messageId) : null;
  const preview = {
    threadId,
    messageId,
    clientId: queuedMessage?.clientUserMessageId
      ?? queuedMessage?.submissionOptions?.clientUserMessageId
      ?? null,
    text: queueMessageText(queuedMessage)
  };
  const sendDisplay = rememberSendDisplay({
    threadId, clientUserMessageId: preview.clientId || messageId, prompt: preview.text,
    images: queuedMessage?.context?.imageAttachments || queuedMessage?.images || [],
    skills: queuedMessage?.context?.skills || queuedMessage?.skills || []
  });
  if (sendDisplay) sendDisplay.messageId = messageId;
  queuedActionBusy.add(key);
  // 引导中不插入正文；成功后使用同一份确认展示缓存，不能因历史尚未同步而移除。
  pendingSteer = preview;
  recordDiagnostic("remote.steer", { phase: "request" });
  renderMessages();
  if (selectedId === threadId) renderQueueArea();
  const readVersion = queueVersion;
  const startedAt = performance.now();
  try {
    const request = mutate("steerQueued", { threadId, messageId }, { reliable: true });
    updateComposer();
    const result = await request;
    if (!currentAuthorization()) return;
    if (sendDisplay) confirmSendDisplay(sendDisplay.payload, result, { steering: true });
    if (pendingSteer === preview) pendingSteer = null;
    if (selectedId === threadId && thread?.id === threadId) {
      if (readVersion === queueVersion || queueSnapshotIsVersioned(result))
        applyQueueSnapshot(threadId, Array.isArray(result?.queuedMessages)
          ? result.queuedMessages
          : (thread.queuedMessages || []).filter(message => message?.id !== messageId), result?.queuedEdits, result);
      renderMessages();
      const reloadStartedAt = performance.now();
      void reloadThread().then(() => {
        if (!currentAuthorization()) return;
        recordDiagnostic("remote.steer.reload", {
          phase: "complete", durationMs: performance.now() - reloadStartedAt
        });
      }).catch(error => {
        if (!currentAuthorization()) return;
        recordDiagnostic("remote.steer.reload", {
          phase: "failure", durationMs: performance.now() - reloadStartedAt,
          errorType: error?.name || "Error", reason: "network-error"
        });
        notice(error.message, true);
      });
    }
    recordDiagnostic("remote.steer", {
      phase: "complete", durationMs: performance.now() - startedAt
    });
  } catch (error) {
    if (!currentAuthorization()) return;
    finishSendDisplay(sendDisplay, !pending);
    if (pendingSteer === preview) {
      pendingSteer = null;
      renderMessages();
    }
    recordDiagnostic("remote.steer", {
      phase: "failure", durationMs: performance.now() - startedAt,
      errorType: error?.name || "Error", reason: "network-error"
    });
    reportMutationError(error);
  } finally {
    if (pendingSteer === preview && (!currentAuthorization() || selectedId !== threadId)) pendingSteer = null;
    queuedActionBusy.delete(key);
    if (currentAuthorization() && selectedId === threadId) updateComposer();
  }
}
