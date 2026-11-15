// 上下文压缩：压缩状态与回执、生命周期事件、横幅与压缩命令。

import { confirmAction } from "../../ui/dialog.js";
import { button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { reloadThread, updateComposer } from "../../core/actions.js";
import { busy, connected, selectedId, session, stopping, thread } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { pendingFeedbackReady } from "../composer/send-state.js";
import { pending } from "./pending-state.js";
import { claudeNativeQueue, isClaude, serviceSideThread, threadReadOnly } from "../session/capabilities.js";

export let compactionVersion = 0;
export const compactions = new Map();
// 从会话存储恢复进行中或结果未知的上下文压缩记录。
export function restoreCompactions() {
  try {
    const stored = JSON.parse(sessionStorage.getItem("codex-elves-compact-pending") || "[]");
    if (Array.isArray(stored)) for (const value of stored) {
      if (typeof value?.threadId !== "string" || !value.threadId
          || !["pending", "running", "unknown"].includes(value.state)) continue;
      const receiptId = typeof value.receiptId === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(value.receiptId)
        ? value.receiptId : null;
      compactions.set(value.threadId, {
        state: value.state, threadId: value.threadId, receiptId,
        deviceId: typeof value.deviceId === "string" ? value.deviceId : null,
        acknowledged: value.acknowledged === true,
        turnId: typeof value.turnId === "string" ? value.turnId : null,
        itemId: typeof value.itemId === "string" ? value.itemId : null,
        baselineTurnId: typeof value.baselineTurnId === "string" ? value.baselineTurnId : null,
        baselineItemId: typeof value.baselineItemId === "string" ? value.baselineItemId : null,
        time: Number.isFinite(value.time) ? value.time : undefined
      });
    }
  } catch {}
}
export function normalizeCompaction(value) {
  const state = ["pending", "running", "unknown"].includes(value?.state) ? value.state : "idle";
  return {
    state,
    turnId: typeof value?.turnId === "string" ? value.turnId : null,
    itemId: typeof value?.itemId === "string" ? value.itemId : null
  };
}
export function currentCompaction(id = selectedId) {
  if (!id) return { state: "idle", turnId: null, itemId: null };
  return compactions.get(id) || (thread?.id === id ? normalizeCompaction(thread.compaction) : { state: "idle", turnId: null, itemId: null });
}
export function compactionLocked(id = selectedId) {
  return ["pending", "running", "unknown"].includes(currentCompaction(id).state);
}
export function compactionInProgress(value = currentCompaction()) {
  if (isClaude()) return value.state === "running";
  // 用户要求压缩时显示可用的停止按钮；未确认送达或结果未知的压缩不能凭本地锁定状态发起停止。
  return value.state === "running" || value.state === "pending" && (value.acknowledged || value.nativeConfirmed);
}
export function compactionQueueSendReason(id = selectedId) {
  // Claude 普通发送继续由原生实际执行状态决定是否排队；压缩回执不能阻塞聊天主流程。
  if (isClaude() && claudeNativeQueue()) return "";
  if (serviceSideThread()) return "临时侧边会话压缩期间不能排队发送，请等待压缩完成";
  const current = currentCompaction(id);
  if (!compactionLocked(id)) return "";
  if (current.deviceId && current.deviceId !== session?.device.id)
    return "压缩回执属于旧设备授权，可继续编辑草稿；请重新读取或在本机核对后再发送。";
  if (current.state === "unknown")
    return "压缩结果仍待确认，可继续编辑草稿；不会自动重发压缩请求，请查回执、重新读取或在本机核对后再发送。";
  if (current.state === "pending" && !current.acknowledged && !current.nativeConfirmed)
    return "压缩请求尚未确认，可继续编辑草稿；不会自动重发压缩请求，请查回执或重新读取后再发送。";
  if (session?.capabilities?.compactionQueueSend !== true)
    return "当前服务尚不支持压缩期间发送排队消息，可继续编辑草稿；请更新远程服务或等待压缩完成。";
  return "";
}
export function persistCompactions(required = false) {
  const stored = [...compactions.values()].filter(value => compactionLocked(value.threadId)).map(value => ({
    state: value.state, threadId: value.threadId, receiptId: value.receiptId || null,
    deviceId: value.deviceId || null, acknowledged: value.acknowledged === true,
    turnId: value.turnId || null, itemId: value.itemId || null,
    baselineTurnId: value.baselineTurnId || null, baselineItemId: value.baselineItemId || null,
    time: value.time
  }));
  try {
    if (stored.length) sessionStorage.setItem("codex-elves-compact-pending", JSON.stringify(stored));
    else sessionStorage.removeItem("codex-elves-compact-pending");
  } catch {
    if (required) throw new Error("无法保存压缩回执，压缩请求未提交。请允许此网页使用浏览器会话存储。");
    notice("无法保存压缩回执；请保持页面打开，并在本机核对压缩状态。", true);
  }
}
function setCompactionState(id, value, { required = false } = {}) {
  const normalized = {
    ...value, ...normalizeCompaction(value), threadId: id,
    receiptId: typeof value?.receiptId === "string" ? value.receiptId : null,
    deviceId: typeof value?.deviceId === "string" ? value.deviceId : null,
    acknowledged: value?.acknowledged === true,
    nativeConfirmed: value?.nativeConfirmed === true,
    time: Number.isFinite(value?.time) ? value.time : Date.now(),
    baselineTurnId: typeof value?.baselineTurnId === "string" ? value.baselineTurnId : null,
    baselineItemId: typeof value?.baselineItemId === "string" ? value.baselineItemId : null
  };
  compactions.set(id, normalized);
  ++compactionVersion;
  if (thread?.id === id) thread.compaction = normalizeCompaction(normalized);
  persistCompactions(required);
  return normalized;
}
function compactionAdvanced(current, projection) {
  if (!projection.itemId && !projection.turnId) return false;
  return projection.itemId !== current.baselineItemId || projection.turnId !== current.baselineTurnId;
}
export function reconcileCompaction(id, value) {
  const projection = normalizeCompaction(value), current = compactions.get(id);
  if (projection.state !== "idle") {
    if (current?.state === "running" && projection.state === "pending") {
      if (thread?.id === id) thread.compaction = normalizeCompaction(current);
      return current;
    }
    return setCompactionState(id, {
      ...current, ...projection,
      // 原生读取确认的 pending 与本页尚未送达的请求分开，不能改写回执确认状态。
      nativeConfirmed: current?.nativeConfirmed === true
        || !current?.receiptId || compactionAdvanced(current, projection)
    });
  }
  if (!current || current.state === "idle" || !current.receiptId || current.state === "running"
      || current.acknowledged || compactionAdvanced(current, projection)) {
    return setCompactionState(id, projection);
  }
  if (thread?.id === id) thread.compaction = normalizeCompaction(current);
  return current;
}
export function renderCompactionBanner(root, id, compaction) {
  if (!id || !compactionLocked(id)) return;
  const unavailable = compaction.deviceId && session?.device.id && compaction.deviceId !== session.device.id;
  if (!unavailable && isClaude() && compaction.state === "pending"
      && thread?.nativeCommands?.some(command => command.name === "compact" && command.state === "pending")) {
    root.append(element("div", "pending-banner", "压缩命令已受理，等待原生执行"));
    return;
  }
  const sendReason = compactionQueueSendReason(id);
  if (compaction.state === "running" && !unavailable && !sendReason) return;
  const awaitingReceipt = compaction.state === "unknown"
    || compaction.state === "pending" && !compaction.acknowledged && !compaction.nativeConfirmed;
  if (!unavailable && awaitingReceipt && !pendingFeedbackReady(compaction)) return;
  if (!unavailable && !awaitingReceipt && !sendReason) return;
  const messages = {
    pending: awaitingReceipt ? "正在确认压缩结果。" : sendReason,
    running: sendReason || "正在压缩此会话的上下文；可继续编辑并发送普通消息，消息会加入原生队列，在压缩完成后执行。",
    unknown: "正在确认压缩结果。"
  };
  const box = element("div", "pending-banner", unavailable
    ? `${sendReason}不会自动重发压缩请求。`
    : messages[compaction.state]);
  if (!unavailable && compaction.receiptId && ["pending", "unknown"].includes(compaction.state)) {
    const check = button("查回执", "只查询压缩请求回执，不会重新提交", () => void resolveCompactionReceipt(id), "text-button");
    check.disabled = !connected;
    box.append(check);
  }
  const reload = button("重新读取", "从原生会话重新读取压缩状态", () => void recoverCompaction(id), "text-button");
  reload.disabled = !connected || selectedId !== id;
  box.append(reload);
  if (unavailable || compaction.state === "unknown") {
    box.append(button("已人工确认", "确认在本机核对后解除压缩锁定", async () => {
      const current = compactions.get(id);
      if (await confirmAction("解除压缩锁定", "请先检查本机原生会话。解除锁定不会重新压缩；仅在已确认压缩已完成或未执行后继续。", "已核对，解除锁定", true)
          && compactions.get(id) === current) {
        setCompactionState(id, { state: "idle", turnId: current?.turnId, itemId: current?.itemId });
        updateComposer();
      }
    }, "text-button"));
  }
  root.append(box);
}
async function applyCompactionReceipt(id, receiptId, receipt) {
  const current = compactions.get(id);
  if (!current || current.receiptId !== receiptId || current.state === "idle") return;
  if (receipt.state === "pending") {
    updateComposer();
    return;
  }
  if (receipt.state === "unknown") {
    setCompactionState(id, { ...current, state: "unknown" });
    updateComposer();
    return;
  }
  if (receipt.state === "failed") {
    setCompactionState(id, { state: "idle", turnId: current.turnId, itemId: current.itemId });
    notice(receipt.result?.message || "原生压缩请求未完成", true);
    updateComposer();
    if (selectedId === id) await reloadThread();
    return;
  }
  if (receipt.state !== "completed") {
    setCompactionState(id, { ...current, state: "unknown" });
    notice("压缩回执状态无法识别。不会自动重发，请在本机核对。", true);
    updateComposer();
    return;
  }
  const acknowledged = setCompactionState(id, { ...current, acknowledged: true });
  if (receipt.result?.compaction) reconcileCompaction(id, receipt.result.compaction);
  // 用户要求远程压缩完成时不再弹出提示；仍核对回执、解除压缩状态并刷新会话。
  updateComposer();
  if (selectedId === id && compactions.get(id)?.receiptId === acknowledged.receiptId) await reloadThread();
}
async function resolveCompactionReceipt(id) {
  const current = compactions.get(id);
  if (!current?.receiptId || !session || !connected) return;
  if (current.deviceId && current.deviceId !== session.device.id) {
    notice("压缩回执属于旧设备授权。请在本机核对后人工解除锁定。", true);
    return;
  }
  try {
    const receipt = await api(`/api/receipts/${current.receiptId}`);
    await applyCompactionReceipt(id, current.receiptId, receipt);
  } catch (error) {
    if (compactions.get(id)?.receiptId !== current.receiptId) return;
    notice(error.code === 404
      ? "未找到压缩回执。不会自动重发；请重新读取会话或在本机核对。"
      : `查询压缩回执失败：${error.message}`, true);
  }
}
async function recoverCompaction(id) {
  if (!connected || selectedId !== id) return;
  try {
    await reloadThread();
    const reason = compactionQueueSendReason(id);
    notice(compactionLocked(id)
      ? reason || "压缩尚未结束，可继续编辑并发送普通消息到原生队列，压缩完成后执行。"
      : "已确认压缩结束，可以继续发送消息。", !!reason);
  } catch (error) { notice(`重新读取压缩状态失败：${error.message}`, true); }
  updateComposer();
}
export async function compactContext() {
  if (stopping || busy || pending || threadReadOnly() || !connected || !selectedId
      || thread?.id !== selectedId || compactionLocked(selectedId)) return;
  const id = selectedId, baseline = normalizeCompaction(thread.compaction), receiptId = randomId();
  try {
    setCompactionState(id, {
      state: "pending", receiptId, deviceId: session.device.id, acknowledged: false,
      turnId: null, itemId: null, baselineTurnId: baseline.turnId, baselineItemId: baseline.itemId
    }, { required: true });
  } catch (error) {
    notice(error.message, true);
    return;
  }
  updateComposer();
  try {
    const receipt = await api("/api/action/compact", { threadId: id }, receiptId);
    await applyCompactionReceipt(id, receiptId, receipt);
  } catch (error) {
    const current = compactions.get(id);
    if (current?.receiptId !== receiptId) return;
    if (error.code && error.code !== 503) {
      setCompactionState(id, { state: "idle", turnId: current.turnId, itemId: current.itemId });
      notice(error.message, true);
    } else {
      setCompactionState(id, { ...current, state: "unknown" });
      notice("未收到压缩回执。不会自动重发；请查回执、重新读取，或在本机核对后解除锁定。", true);
    }
  } finally { updateComposer(); }
}
function compactionEventMatches(current, item, turnId) {
  if (!current || current.state === "idle") return true;
  if (current.itemId && current.itemId !== "pending-manual-context-compaction"
      && item?.id && current.itemId !== item.id) return false;
  if (current.turnId && turnId && current.turnId !== turnId) return false;
  return !(current.receiptId && !current.itemId && !current.turnId && item?.source === "automatic");
}
export function receiveCompactionLifecycle(event, params) {
  const id = params.threadId;
  if (typeof id !== "string" || !id) return;
  const turnId = params.turnId || params.turn?.id || null;
  const items = params.item?.type === "contextCompaction" ? [params.item]
    : (params.turn?.items || []).filter(item => item?.type === "contextCompaction");
  let current = compactions.get(id), changed = false, completed = false;
  for (const item of items) {
    if (!compactionEventMatches(current, item, turnId)) continue;
    const itemCompleted = event.method === "item/completed" || item.completed === true
      || ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(item.status);
    if (itemCompleted) {
      setCompactionState(id, { state: "idle", turnId, itemId: item.id });
      current = compactions.get(id); changed = true; completed = true;
    } else if (event.method === "item/started" || item.completed === false) {
      current = setCompactionState(id, { ...current, state: "running", nativeConfirmed: true, turnId, itemId: item.id });
      changed = true;
    }
  }
  if (!completed && event.method === "turn/completed" && current?.state === "running"
      && current.turnId && current.turnId === turnId) {
    setCompactionState(id, { state: "idle", turnId, itemId: current.itemId });
    changed = true; completed = true;
  }
  if (changed && selectedId === id) {
    updateComposer();
    if (completed) {
      const failed = items.some(item => item.status === "failed") || params.turn?.status === "failed";
      // 实时事件与回执保持一致：成功结束静默更新，失败仍显示错误，不能因去掉完成提示而隐藏异常。
      if (failed) notice("上下文压缩失败，请查看原生会话。", true);
    }
  }
}
