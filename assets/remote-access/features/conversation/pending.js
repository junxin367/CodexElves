// 写操作与回执：发送/创建/编辑等写入的幂等提交、待确认回执的保存与核对。

import { recoveryStorageMessage } from "../../recovery-store.js";
import { notice } from "../../ui/notice.js";
import { creating, selectedId, selectionToken, selectionVersion, session, thread, turns } from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { updateComposer } from "../composer/composer.js";
import {
  clearSubmittedDraft, recoverFailedInput, recoveryGeneration, recoveryScope, restorePrivateInputs,
  setRecoveryScope, smallInputStorageRecord, snapshotDraft, submittedDrafts
} from "../composer/drafts.js";
import { selectCreatedThread } from "../composer/new-chat.js";
import { renderMessages } from "./history-view.js";
import { reloadThread } from "./history.js";
import { selectThread } from "./selection.js";
import {
  applyGoalComposerReceipt, goalComposerAction, goalComposerRecoveries, goalComposerStorageRecord,
  goalReceiptOutcome, goalSubmissionState, hydrateGoalComposerRecord, updateGoalComposerPending,
  validGoalComposerDraft
} from "../goal/composer.js";
import { goalVersion } from "../goal/status.js";
import { cancelMessageEdit, editingMessage } from "../messages/message-edit.js";
import {
  applyQueuedSendResult, applyQueueReorderResult, applyQueueSnapshot, confirmQueuedSteerDisplay,
  pendingSteer, queueSnapshotIsVersioned, queueVersion, reloadQueue, setPendingSteer
} from "../queue/queue.js";
import {
  checkQueuedMessageEdit, completeQueuedEditAction, finishQueuedMessageEdit, queuedEditActions,
  queuedEdits
} from "../queue/queued-edit.js";
import { clearSentMessageDisplays } from "../queue/sent-display.js";
import { claudeThreadId, privateInputScope, threadActionReadOnly } from "../session/capabilities.js";

export let pendingTimer = null;
export function setPendingTimer(value) { return pendingTimer = value; }
export function validSkillSelection(value) {
  return value == null || (Array.isArray(value) && value.length <= 16 && value.every(skill =>
    skill && typeof skill.name === "string" && skill.name.length > 0 && skill.name.length <= 256
      && typeof skill.path === "string" && skill.path.length > 0 && skill.path.length <= 4096));
}
export function validQueuedImageSources(value) {
  return Array.isArray(value) && value.length <= 4
    && value.every(source => typeof source === "string" && source.length > 0 && source.length <= 32_767
      && !/[\u0000-\u001f\u007f]/u.test(source))
    && new Set(value).size === value.length;
}
export let pending = null;
export function setPending(value) { return pending = value; }
export let pendingSendRequest = null;
export function setPendingSendRequest(value) { return pendingSendRequest = value; }
// 从会话存储恢复未完成的写操作回执；校验失败时记为无效，交由恢复流程提示用户。
export function restoreStoredPending() {
  try {
    const stored = hydrateGoalComposerRecord(JSON.parse(sessionStorage.getItem("codex-elves-pending") || "null"));
    if (stored) pending = typeof stored.id === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(stored.id)
      && (stored.action === "reorderQueued"
        ? typeof stored.payload?.threadId === "string" && Array.isArray(stored.payload.messageIds)
          && Array.isArray(stored.payload.expectedMessageIds)
          && stored.payload.messageIds.length > 0
          && stored.payload.messageIds.length === stored.payload.expectedMessageIds.length
          && new Set(stored.payload.messageIds).size === stored.payload.messageIds.length
          && stored.payload.messageIds.every(id => typeof id === "string" && stored.payload.expectedMessageIds.includes(id))
        : queuedEditActions.has(stored.action)
        ? typeof stored.payload?.editId === "string" && typeof stored.payload?.threadId === "string"
        : ["steerQueued", "removeQueued"].includes(stored.action)
        ? typeof stored.payload?.threadId === "string" && typeof stored.payload?.messageId === "string"
        : ["send", "create", "editMessage", "setGoal"].includes(stored.action)
          && typeof stored.payload?.prompt === "string" && validSkillSelection(stored.payload.skills)
          && (stored.action !== "setGoal" || stored.payload.goalMode === true && typeof stored.payload.threadId === "string")
          && (stored.payload.goalMode !== true || goalComposerAction(stored.action, stored.payload)
            && (validGoalComposerDraft(stored.draft) || typeof stored.recoveryRef === "string")))
      ? stored : { invalid: true };
  } catch { pending = { invalid: true }; }
}
export function savePending(value) {
  if (value) {
    if (!Number.isFinite(value.time)) value.time = Date.now();
    try { sessionStorage.setItem("codex-elves-pending", JSON.stringify(goalComposerStorageRecord(value))); }
    catch (error) {
      if (!value.recoveryRef) throw new Error(recoveryStorageMessage(error, true));
      // 完整记录已先写入 IndexedDB，刷新时也可以通过原 ID 找回。
    }
  } else {
    try { sessionStorage.removeItem("codex-elves-pending"); }
    catch { notice("无法清理浏览器回执记录，重新打开页面后可能仍需人工核对。", true); }
    const previous = pending;
    if (previous?.recoveryRef && recoveryScope)
      void recoveryStore.remove(recoveryScope, "active", previous.recoveryRef).catch(() => {});
    if (previous?.recoveryRef && recoveryScope
        && ![...goalComposerRecoveries.values()].some(record => record.id === previous.id))
      void recoveryStore.remove(recoveryScope, "pending", previous.recoveryRef).catch(() => {});
  }
  clearTimeout(pendingTimer);
  pending = value;
}
export async function mutate(action, payload, { reliable = false, receiptId = null } = {}) {
  if (payload?.threadId && threadActionReadOnly(action, payload.threadId))
    throw Object.assign(new Error("此会话仅供查看，不能提交操作。"), { code: 403, notSubmitted: true });
  const queueAction = ["steerQueued", "removeQueued"].includes(action);
  if (queueAction && pending)
    throw Object.assign(new Error("队列操作仍待确认，请先查询原回执，不要重复引导或删除。"), { notSubmitted: true });
  const trackedOperation = queueAction || ["deleteThread", "createSideThread", "closeSideThread"].includes(action);
  const version = selectionVersion;
  const deviceId = session?.device.id, generation = recoveryGeneration;
  const operationScope = privateInputScope(session);
  const currentAuthorization = () => generation === recoveryGeneration && deviceId === session?.device.id
    && operationScope === privateInputScope(session);
  const id = receiptId || (reliable && pending ? pending.id : randomId());
  const goalAction = goalComposerAction(action, payload);
  if (reliable && !pending) {
    try {
      const record = {
        id, action, payload, deviceId: session.device.id, selectionToken, time: Date.now(),
        ...(["send", "create", "setGoal"].includes(action) ? {
          draft: submittedDrafts.get(payload) || snapshotDraft(),
          ...(goalAction ? { goalReadVersion: goalVersion } : {})
        } : {})
      };
      if (record.draft) {
        const currentScope = privateInputScope(session);
        try {
          await recoveryStore.activate(currentScope, { replace: false });
          if (generation !== recoveryGeneration || deviceId !== session?.device.id)
            throw new Error("设备授权已更换，操作未提交。");
          setRecoveryScope(currentScope);
          record.recoveryScope = currentScope;
          await recoveryStore.put(currentScope, [{ kind: "pending", id, value: record },
            { kind: "active", id, value: { id } }]);
          record.recoveryRef = id;
        } catch (error) {
          if (generation !== recoveryGeneration || deviceId !== session?.device.id || error.name === "RecoveryAuthorizationError") throw error;
          record.recoveryScope = currentScope;
          if (!smallInputStorageRecord(record)) throw error;
          // 大小受控的记录仍可使用会话存储回执；大图片必须先可靠存入 IndexedDB。
          notice("浏览器本地草稿存储不可用；本次回执将保存在会话存储，未发送草稿仅保留在本页。", true);
        }
        if (generation !== recoveryGeneration || deviceId !== session?.device.id)
          throw new Error("设备授权已更换，操作未提交。");
      }
      savePending(record);
    } catch (error) {
      error.message = recoveryStorageMessage(error, true);
      error.notSubmitted = true;
      if (goalAction) Object.assign(error, { notSubmitted: true, goalReceipt: {
        state: "failed", result: { ok: false, message: error.message, code: "receipt-storage-unavailable" }
      } });
      throw error;
    }
  }
  const ownsPending = () => reliable && pending?.id === id;
  let result;
  try { result = await api(`/api/action/${action}`, payload, id); }
  catch (error) {
    if (!currentAuthorization()) throw error;
    if (error.notSubmitted) {
      if (goalAction) Object.assign(error, { receiptId: id, goalReceipt: {
        state: "failed", result: { ok: false, notSent: true, message: error.message }
      } });
      if (ownsPending()) {
        const unsent = pending;
        try { await recoverFailedInput(unsent); }
        catch {
          error.message += " 本地恢复记录更新失败，请保留当前输入。";
        } finally {
          if (pending === unsent) savePending(null);
        }
      }
      throw error;
    }
    if (goalAction && ownsPending()) {
      const rejected = error.outcomeUnknown !== true && Number.isInteger(error.code)
        && error.code >= 400 && error.code < 500 && error.code !== 408;
      const receipt = { state: rejected ? "failed" : "unknown",
        result: { ok: false, message: error.message, code: error.code, outcomeUnknown: !rejected } };
      Object.assign(error, { goalReceipt: receipt, receiptId: id, result: receipt.result, outcomeUnknown: !rejected });
      updateGoalComposerPending({ ...pending, result: receipt.result, receiptState: receipt.state });
      throw error;
    }
    if (trackedOperation) {
      const rejected = error.outcomeUnknown !== true && Number.isInteger(error.code)
        && error.code >= 400 && error.code < 500 && error.code !== 408;
      Object.assign(error, { receiptId: id, operationState: rejected ? "failed" : "unknown", outcomeUnknown: !rejected });
    }
    if (reliable) {
      // A network failure does not prove the native client rejected the message.
      const rejected = error.outcomeUnknown !== true && Number.isInteger(error.code) && error.code >= 400 && error.code < 500
        && ![408, 409].includes(error.code);
      if (rejected && ownsPending()) {
        await recoverFailedInput(pending);
        if (ownsPending()) savePending(null);
      } else {
        if (ownsPending() && error.operationState !== "failed")
          error.pendingFeedback = true;
        if (ownsPending()) void resolvePending();
        if (action === "send") error.deliveryPending = true;
      }
    }
    throw error;
  }
  if (goalAction) {
    const data = result.result || {};
    const goalSubmission = goalSubmissionState(data.goalSubmission, action);
    if (ownsPending()) updateGoalComposerPending({ ...pending, result: data, receiptState: result.state,
      ...(goalSubmission ? { goalSubmission } : {}) });
    if (goalReceiptOutcome(result, action) === "succeeded") return data;
    throw Object.assign(new Error(data.message || (result.state === "pending"
      ? "目标提交仍在处理，请稍后查回执" : "目标未确认完成，草稿与回执已保留")), {
      goalReceipt: result, receiptId: id, result: data, goalSubmission,
      code: data.code, outcomeUnknown: ["pending", "unknown"].includes(goalReceiptOutcome(result, action))
    });
  }
  if (result.state === "pending") {
    const confirmed = (action === "send" || action === "editMessage" && claudeThreadId(payload?.threadId)) && ownsPending()
      ? await confirmPendingSend(pending, result.result).catch(() => null) : null;
    if (confirmed && ownsPending()) result = { state: "completed", result: confirmed };
    else {
      if (ownsPending()) void resolvePending();
      throw Object.assign(new Error("操作仍在处理，请稍后查看会话或回执"), { deliveryPending: action === "send",
        pendingFeedback: ownsPending(),
        ...(trackedOperation ? { receiptId: id, operationState: "pending", outcomeUnknown: true } : {}),
        ...(["approve", "answer"].includes(action) ? { approvalState: "pending" } : {}) });
    }
  }
  if (result.state === "unknown") {
    const confirmed = action === "send" && ownsPending()
      ? await confirmPendingSend(pending, result.result).catch(() => null) : null;
    if (confirmed && ownsPending()) result = { state: "completed", result: confirmed };
    else {
      if (ownsPending() && (queueAction || action === "send"
          || queuedEditActions.has(action) && session.capabilities?.queueEditRecovery === true))
        void resolvePending();
      throw Object.assign(new Error(result.result?.message || "送达状态未知，请在会话中确认，勿重复发送"), {
        pendingFeedback: ownsPending(),
        ...(trackedOperation ? { receiptId: id, operationState: "unknown", outcomeUnknown: true } : {}),
        ...(["approve", "answer"].includes(action) ? { approvalState: "unknown" } : {}),
        deliveryPending: action === "send"
          && typeof (result.result?.clientUserMessageId ?? payload.clientUserMessageId) === "string"
      });
    }
  }
  if (ownsPending() && result.state === "failed") await recoverFailedInput(pending);
  if (ownsPending()) savePending(null);
  if (result.state === "failed") {
    if (action === "create" && result.result?.threadId && creating && selectionVersion === version) {
      await selectCreatedThread(result.result.threadId);
    }
    throw Object.assign(new Error(result.result?.message || "原生操作未完成"), {
      result: result.result, goalSubmission: result.result?.goalSubmission, code: result.result?.code,
      ...(trackedOperation ? { receiptId: id, operationState: "failed", outcomeUnknown: result.result?.outcomeUnknown === true }
        : ["approve", "answer"].includes(action) ? { approvalState: "failed" } : {})
    });
  }
  return result.result;
}
export function reportMutationError(error) {
  if (error.pendingFeedback) { updateComposer(); return; }
  notice(error.message, true);
}
export function acceptedSendItem(items, clientId) {
  return items.find(item => (item.type === "userMessage"
    || item.type === "steeringUserMessage" && item.status === "accepted")
    && !item.remotePendingInput && !item.remoteCompactionInput
    && (item.clientId === clientId || item.clientUserMessageId === clientId));
}
async function confirmPendingSend(current, result = {}) {
  const threadId = current.payload?.threadId;
  const clientId = result?.clientUserMessageId ?? current.payload?.clientUserMessageId;
  const historicalEdit = current.action === "editMessage" && claudeThreadId(threadId);
  if (current.action !== "send" && !historicalEdit || typeof threadId !== "string"
      || typeof clientId !== "string" || !/^[a-zA-Z0-9_-]{1,120}$/.test(clientId)
      || result?.threadId && result.threadId !== threadId
      || result?.clientUserMessageId && current.payload?.clientUserMessageId
        && result.clientUserMessageId !== current.payload.clientUserMessageId) return null;
  if (current.deviceId && current.deviceId !== session?.device.id) return null;
  current.confirmationClientUserMessageId = clientId;
  // 原生接受事件已到达时立即收敛回执；不再被慢速或未返回的 sendState 查询挡住。
  // 仅使用稳定轮次下的真实用户消息，排除发送占位和等待准入，保留未知结果不重发。
  const accepted = selectedId === threadId
    ? acceptedSendItem([...turns.values()].filter(turn => (turn.id || turn.turnId)
      && !turn.pendingUserMessageAdmission).flatMap(turn => turn.items || []), clientId) : null;
  if (accepted) return { ok: true, confirmed: true, deliveryState: "accepted",
    threadId, clientUserMessageId: clientId, messageId: accepted.serverUserMessageId || accepted.id };
  if (pendingSendRequest?.current === current) return pendingSendRequest.promise;
  const promise = (async () => {
    if (session.capabilities?.queueSendRecovery === true) {
      const state = await api("/api/read/sendState", { threadId, clientUserMessageId: clientId });
      return state.confirmed === true && state.threadId === threadId
        && state.clientUserMessageId === clientId
        && (historicalEdit ? state.deliveryState === "accepted" : ["queued", "accepted"].includes(state.deliveryState))
        ? state : null;
    }
    const queue = await api("/api/read/queue", { threadId });
    const queued = queue.threadId === threadId && (queue.queuedMessages || []).find(message =>
      (message.clientUserMessageId ?? message.submissionOptions?.clientUserMessageId
        ?? (!message.serverQueuedMessageId ? message.id : null)) === clientId);
    if (queued && !historicalEdit) return { ...queue, ok: true, confirmed: true, deliveryState: "queued",
      clientUserMessageId: clientId, messageId: queued.id };
    const history = await api("/api/read/history", { threadId, cursor: null });
    const delivered = history.threadId === threadId
      && acceptedSendItem((history.data || []).flatMap(turn => turn.items || []), clientId);
    return delivered ? { ok: true, confirmed: true, deliveryState: "accepted",
      threadId, clientUserMessageId: clientId, messageId: delivered.id } : null;
  })();
  const request = { current, promise }; pendingSendRequest = request;
  try { return await promise; }
  finally { if (pendingSendRequest === request) pendingSendRequest = null; }
}
export async function resolvePending() {
  if (!pending || !session) return;
  if (pending.recoveryUnavailable) {
    try { await restorePrivateInputs(session); }
    catch (error) { notice(recoveryStorageMessage(error), true); return; }
    if (pending?.recoveryUnavailable) { notice("本地恢复记录仍不可读取，原请求标识已保留，请先核对本机会话。", true); return; }
    return resolvePending();
  }
  const current = pending;
  const deviceId = session.device.id;
  const queueAction = ["steerQueued", "removeQueued"].includes(current.action);
  const operationScope = queueAction ? privateInputScope(session) : null, generation = recoveryGeneration;
  const version = selectionVersion;
  const queueReadVersion = queueVersion;
  if (current.invalid || (current.deviceId && current.deviceId !== deviceId)) {
    notice("回执记录损坏或属于旧设备授权。请先检查原生会话，再人工解除锁定；不会自动重发。", true);
    updateComposer(); return;
  }
  const stillCurrent = () => pending === current && session?.device.id === deviceId
    && (!queueAction || generation === recoveryGeneration && operationScope === privateInputScope(session));
  const retry = delay => {
    clearTimeout(pendingTimer);
    pendingTimer = setTimeout(() => { if (stillCurrent()) void resolvePending(); }, delay);
  };
  try {
    let receipt = await api(`/api/receipts/${current.id}`);
    if (!stillCurrent()) return;
    if (goalComposerAction(current.action, current.payload)) {
      const outcome = await applyGoalComposerReceipt(current, receipt, { allowSelection: true });
      if (outcome === "pending" && pending?.id === current.id) {
        clearTimeout(pendingTimer);
        pendingTimer = setTimeout(() => { if (pending?.id === current.id) void resolvePending(); }, 2000);
      }
      return;
    }
    if ((current.action === "send" || current.action === "editMessage" && claudeThreadId(current.payload?.threadId))
        && ["pending", "unknown"].includes(receipt.state)) {
      const confirmed = await confirmPendingSend(current, receipt.result);
      if (!stillCurrent()) return;
      if (confirmed) receipt = { state: "completed", result: confirmed };
      else if (receipt.state === "unknown") {
        if (typeof (receipt.result?.clientUserMessageId ?? current.payload?.clientUserMessageId) === "string") retry(5000);
        else notice("回执缺少客户端消息标识，无法自动核对。请在会话中确认后再解除锁定。", true);
        updateComposer(); return;
      }
    }
    if (receipt.state === "pending") { retry(2000); return; }
    if (receipt.state === "unknown") {
      if (queuedEditActions.has(current.action)) {
        if (session.capabilities?.queueEditRecovery === true) {
          const state = await checkQueuedMessageEdit(current.payload.threadId, current.payload.editId);
          if (stillCurrent() && state?.operationPending) retry(2000);
        } else notice("队列编辑结果尚未确认，请更新远程服务后核对；不会重复保存或恢复消息。", true);
      } else if (queueAction) retry(5000);
      updateComposer(); return;
    }
    if (receipt.state === "failed") {
      try { await recoverFailedInput(current); }
      catch (error) {
        notice(error.message || "输入恢复失败，原请求回执已保留，请勿重新发送。", true);
        retry(5000); updateComposer(); return;
      }
    }
    if (!stillCurrent()) return;
    if (queueAction && receipt.state === "completed"
        && (receipt.result?.threadId !== current.payload.threadId
          || receipt.result?.messageId !== current.payload.messageId)) {
      notice("队列操作回执的会话或消息不匹配，原请求标识已保留，请继续核对原回执。", true);
      updateComposer(); return;
    }
    savePending(null);
    if (queueAction) {
      const id = current.payload.threadId;
      if (receipt.state === "completed") {
        if (current.action === "steerQueued")
          confirmQueuedSteerDisplay(id, current.payload.messageId, receipt.result);
        if (selectedId === id && thread?.id === id) {
          const result = receipt.result;
          if (current.selectionToken === selectionToken && queueReadVersion === queueVersion || queueSnapshotIsVersioned(result))
            applyQueueSnapshot(id, Array.isArray(result.queuedMessages) ? result.queuedMessages
              : (thread.queuedMessages || []).filter(message => message?.id !== current.payload.messageId),
            result.queuedEdits, result);
          if (pendingSteer?.threadId === id && pendingSteer.messageId === current.payload.messageId) {
            setPendingSteer(null); renderMessages();
          }
          const refresh = current.action === "steerQueued" ? reloadThread() : reloadQueue();
          void refresh.catch(error => {
            if (deviceId === session?.device.id && generation === recoveryGeneration
                && operationScope === privateInputScope(session)) notice(error.message, true);
          });
        }
        notice(current.action === "steerQueued" ? "队列引导结果已确认" : "队列删除结果已确认");
      } else notice(receipt.result?.message || "队列操作未完成，请核对原消息", true);
      updateComposer(); return;
    }
    if (current.action === "reorderQueued") {
      if (receipt.state === "completed") {
        applyQueueReorderResult(current.payload.threadId, receipt.result,
          current.selectionToken === selectionToken ? queueReadVersion : -1);
        notice("队列顺序已更新");
      } else {
        if (selectedId === current.payload.threadId) void reloadQueue().catch(error => notice(error.message, true));
        notice(receipt.result?.message || "队列排序未完成，请重新核对顺序", true);
      }
      updateComposer();
      return;
    }
    if (queuedEditActions.has(current.action)) {
      if (receipt.state === "completed") {
        completeQueuedEditAction(current.action, current.payload, receipt.result,
          current.selectionToken === selectionToken ? queueReadVersion : -1);
        notice(current.action === "beginQueuedEdit" ? "排队消息已取回输入框"
          : current.action === "cancelQueuedEdit" ? "已取消编辑并恢复队列消息"
          : current.action === "dismissQueuedEdit" ? "已结束待确认编辑，未保存、删除或重发消息" : "队列消息修改已确认保存");
      } else {
        if (current.action === "beginQueuedEdit")
          finishQueuedMessageEdit(queuedEdits.get(current.payload.threadId));
        notice(receipt.result?.message || "队列编辑操作未完成，草稿已保留", true);
      }
      updateComposer();
      return;
    }
    const queued = receipt.state === "completed" && current.action === "send"
      && applyQueuedSendResult(current.payload.threadId, receipt.result,
        current.selectionToken === selectionToken ? queueReadVersion : -1, current.payload);
    if (receipt.state === "completed") {
      if (current.action === "editMessage") {
        clearSentMessageDisplays(current.payload.threadId, { retire: true });
        if (editingMessage?.threadId === current.payload.threadId) cancelMessageEdit({ focus: false });
      } else clearSubmittedDraft(current.payload,
        (selectedId === current.payload.threadId || (creating && current.action === "create")) && current.selectionToken === selectionToken);
    } else notice(receipt.result?.message || "上一条消息未发送", true);
    if (current.action === "create" && receipt.result?.threadId && creating
        && current.selectionToken === selectionToken && version === selectionVersion) await selectCreatedThread(receipt.result.threadId);
    else if (selectedId === current.payload.threadId) {
      if (current.action === "editMessage" && receipt.state === "completed")
        await selectThread(selectedId, { closeSidebar: false, force: true });
      else if (!queued) await reloadThread();
    }
    updateComposer();
  } catch (error) {
    if (!stillCurrent()) return;
    if (error.code === 404) {
      if (goalComposerAction(current.action, current.payload)) {
        updateGoalComposerPending({ ...current, receiptState: "unknown" });
        notice("未找到原目标回执。草稿和原请求标识已保留，请在本机核对；不会自动重复设置或重发首条。", true);
        updateComposer();
        return;
      }
      if (queueAction) {
        // 旧引导可能根本未派发；不能凭 404 判断成功，只在原生历史确认已接收且队列已移除后结束等待。
        if (claudeThreadId(current.payload.threadId)) {
          const queue = await api("/api/read/queue", { threadId: current.payload.threadId }).catch(() => null);
          if (!stillCurrent()) return;
          if (queue?.threadId === current.payload.threadId && Array.isArray(queue.queuedMessages)
              && !queue.queuedMessages.some(message => message.id === current.payload.messageId)) {
            const history = await api("/api/read/history", { threadId: current.payload.threadId }).catch(() => null);
            if (!stillCurrent()) return;
            const received = history?.threadId === current.payload.threadId && Array.isArray(history.data)
              && history.data.some(turn => turn.items?.some(item => item.type === "userMessage"
                && item.clientId === current.payload.messageId && !item.remotePendingInput));
            if (received) {
              savePending(null);
              confirmQueuedSteerDisplay(current.payload.threadId, current.payload.messageId,
                { ok: true, threadId: current.payload.threadId, messageId: current.payload.messageId });
              if (pendingSteer?.threadId === current.payload.threadId
                  && pendingSteer.messageId === current.payload.messageId) setPendingSteer(null);
              if (selectedId === current.payload.threadId && thread?.id === selectedId) {
                if (queueReadVersion === queueVersion || queueSnapshotIsVersioned(queue))
                  applyQueueSnapshot(selectedId, queue.queuedMessages, queue.queuedEdits, queue);
                else void reloadQueue().catch(() => {});
                renderMessages();
              }
              updateComposer();
              return;
            }
          }
        }
        // 未确认的原请求继续保留为页面内提示，避免每次恢复连接都弹同一条红色错误。
        if (!current.receiptMissing) savePending({ ...current, receiptMissing: true });
        updateComposer(); return;
      }
      if (queuedEditActions.has(current.action) && session.capabilities?.queueEditRecovery === true)
        await checkQueuedMessageEdit(current.payload.threadId, current.payload.editId);
      else if (current.action === "send" && current.payload?.clientUserMessageId) {
        const confirmed = await confirmPendingSend(current).catch(() => null);
        if (confirmed && stillCurrent()) {
          savePending(null);
          applyQueuedSendResult(current.payload.threadId, confirmed,
            current.selectionToken === selectionToken ? queueReadVersion : -1, current.payload);
          clearSubmittedDraft(current.payload,
            selectedId === current.payload.threadId && current.selectionToken === selectionToken);
          updateComposer();
          if (selectedId === current.payload.threadId) await reloadThread();
        } else if (stillCurrent()) retry(5000);
      }
      else notice("未找到上一条消息的回执。不会自动重发；请先检查会话。", true);
      updateComposer();
    } else retry(5000);
  }
}
