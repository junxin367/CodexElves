// 排队编辑恢复：核对未完成的排队编辑，放弃或继续恢复，并渲染恢复控件。

import { copyText } from "../../ui/clipboard.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { mutate, updateComposer } from "../../core/actions.js";
import { busy, connected, selectedId, session, setBusy, thread } from "../../core/state.js";
import { api } from "../api.js";
import { pending } from "../conversation/pending-state.js";
import { savePending } from "../conversation/pending-store.js";
import { applyQueueRead, reloadQueue } from "./queue.js";
import { completeQueuedEditAction } from "./queued-edit.js";
import {
  editingQueuedMessage, queueVersion, queuedEditActions, queuedEditCheckVersion, queuedEditChecks,
  queuedEditFingerprint, queuedEdits, setQueuedEditCheckVersion
} from "./state.js";
import { queueCapability, threadReadOnly } from "../session/capabilities.js";

export async function checkQueuedMessageEdit(threadId, editId) {
  if (!connected || !queueCapability("queueEditRecovery", threadId)) return null;
  const existing = queuedEditChecks.get(editId), deviceId = session.device.id;
  if (existing?.deviceId === deviceId && existing.promise) return existing.promise;
  const request = { deviceId, loading: true, result: null, error: "", promise: null, queueReadVersion: queueVersion };
  queuedEditChecks.set(editId, request); setQueuedEditCheckVersion(queuedEditCheckVersion + 1); updateComposer();
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
      request.loading = false; request.promise = null; setQueuedEditCheckVersion(queuedEditCheckVersion + 1);
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
