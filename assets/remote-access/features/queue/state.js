// 队列共享状态：拖拽排序、队列快照版本与已通知内容，以及排队编辑的状态与校验。

import { performAndroidHapticFeedback } from "../../platform.js";
import { sha256Hex } from "../../sha256.js";
import { selectedId, thread, turns } from "../../core/state.js";
import { validQueuedImageSources, validSkillSelection } from "../conversation/pending-state.js";

export let editingQueuedMessage = null;
export function setEditingQueuedMessage(value) { return editingQueuedMessage = value; }
export const queuedEdits = new Map();
export const queuedEditActions = new Set(["beginQueuedEdit", "editQueued", "cancelQueuedEdit", "dismissQueuedEdit"]);
export const queuedEditChecks = new Map();
export let queuedEditCheckVersion = 0;
export function setQueuedEditCheckVersion(value) { return queuedEditCheckVersion = value; }
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
export async function queuedEditFingerprint(action, payload) {
  const value = JSON.stringify([action, action === "editQueued" ? payload.prompt.trim() : null,
    payload.retainedImageSources ?? null,
    (payload.images || []).map(image => [image.name, image.mime, image.dataUrl, image.size])]);
  return sha256Hex(value);
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

export let queueVersion = 0, notifiedQueue = null, notifiedQueueEdits = null, queueRequest = null;
export function setQueueRequest(value) { return queueRequest = value; }
export function setQueueVersion(value) { return queueVersion = value; }
export function setNotifiedQueue(value) { return notifiedQueue = value; }
export function setNotifiedQueueEdits(value) { return notifiedQueueEdits = value; }
export let queueSnapshotVersion = null;
export function setQueueSnapshotVersion(value) { return queueSnapshotVersion = value; }
export const retiredQueueEpochs = new Set();
export let queueDrag = null, queueReorder = null;
export function setQueueDrag(value) { return queueDrag = value; }
export function setQueueReorder(value) { return queueReorder = value; }
export let pendingSteer = null;
export function setPendingSteer(value) { return pendingSteer = value; }
export const queuedActionBusy = new Set();
export const sameQueueOrder = (left, right) => left.length === right.length && left.every((id, index) => id === right[index]);
export function moveQueueItem(ids, messageId, index) {
  const order = ids.filter(id => id !== messageId);
  order.splice(Math.max(0, Math.min(index, order.length)), 0, messageId);
  return order;
}
export function moveQueueDrag(drag) {
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
export function scrollQueueDrag(drag) {
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
export function queueSnapshotIsVersioned(snapshot) {
  return typeof snapshot?.queueEpoch === "string" && snapshot.queueEpoch.length > 0
    && Number.isSafeInteger(snapshot.queueRevision) && snapshot.queueRevision >= 0;
}
export function queueMessageIdentity(message) {
  return message?.clientUserMessageId ?? message?.submissionOptions?.clientUserMessageId
    ?? (!message?.serverQueuedMessageId ? message?.id : null);
}
export function queueSnapshotRemovedMessage(previous, next) {
  const nextIds = new Set((next || []).map(queueMessageIdentity).filter(Boolean));
  return (previous || []).some(message => {
    const id = queueMessageIdentity(message);
    return id && !nextIds.has(id);
  });
}
export function queueMessageText(message) {
  return message?.text ?? message?.context?.prompt ?? message?.prompt ?? message?.message?.prompt ?? "";
}
