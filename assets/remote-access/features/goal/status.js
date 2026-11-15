// 目标状态：渲染目标状态与目标编辑器，提交目标编辑、状态修改与删除并核对回执；计时、编辑草稿与回执状态在 goal/state.js。

import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { reloadThread } from "../../core/actions.js";
import { connected, selectedId, session, thread } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { syncPendingFeedbackTimer } from "../composer/composer.js";
import { pendingFeedbackReady } from "../composer/send-state.js";
import {
  editableGoalStatuses, fitGoalEditor, goalEditBaseline, goalEditDraft, goalEditPending,
  goalEditPollTimer, goalEditReceiptRequest, goalStatusView, goalVersion, sameGoalEditBaseline,
  setGoalEditDraft, setGoalEditPending, setGoalEditPollTimer, setGoalEditReceiptRequest,
  setGoalVersion, setNotifiedGoal, stopGoalLiveTimer, updateGoalLiveView, validGoalEditPayload
} from "./state.js";
import { isClaude, threadReadOnly } from "../session/capabilities.js";
import { currentThreadDisplay } from "../session/thread-cache.js";

function scheduleGoalEditReceipt() {
  clearTimeout(goalEditPollTimer);
  if (goalEditPending?.state !== "pending") return;
  setGoalEditPollTimer(setTimeout(() => {
    void resolveGoalEditReceipt().catch(error => {
      const current = goalEditPending;
      if (current?.state !== "pending") return;
      setGoalEditPending({ ...current, state: "unknown" });
      renderGoalStatus();
      notice(`查询目标操作回执失败：${error.message}。不会重复提交，请稍后手动查回执。`, true);
    });
  }, 2000));
}
async function applyGoalEditReceipt(record, receipt) {
  if (goalEditPending?.id !== record.id) return receipt?.state || "stale";
  if (receipt.state === "pending") {
    setGoalEditPending({ ...record, state: "pending" });
    scheduleGoalEditReceipt();
    renderGoalStatus();
    return "pending";
  }
  if (receipt.state === "unknown") {
    setGoalEditPending({ ...record, state: "unknown" });
    renderGoalStatus();
    return "unknown";
  }
  setGoalEditPending(null);
  if (receipt.state === "failed") {
    const error = Object.assign(new Error(receipt.result?.message || "原生目标更新未完成"),
      { code: receipt.result?.code || 503 });
    if (error.code === 409 && selectedId === record.payload.threadId) {
      await reloadThread().catch(() => {});
    }
    renderGoalStatus();
    throw error;
  }
  if (receipt.state !== "completed") {
    renderGoalStatus();
    throw new Error("目标操作回执状态无效");
  }
  if (receipt.result?.backend === "claude" && receipt.result?.ok === true
      && receipt.result?.nativeCommand?.name === "goal"
      && receipt.result.threadId === record.payload.threadId) {
    setGoalEditDraft(null);
    notice("目标命令已交给 Claude，状态将随原生执行更新");
    renderGoalStatus();
    if (selectedId === record.payload.threadId) await reloadThread();
    return "completed";
  }
  const operation = record.payload.operation ?? "edit";
  const updated = receipt.result?.goal;
  const updatedBaseline = goalEditBaseline(updated);
  const expectedStatus = operation === "pause" ? "paused"
    : operation === "resume" ? "active" : record.payload.expectedStatus;
  if (receipt.result?.threadId !== record.payload.threadId
      || (operation === "delete" ? updated !== null
        : !updatedBaseline || updatedBaseline.createdAt !== record.payload.expectedCreatedAt
          || updatedBaseline.objective !== record.payload.objective
          || updatedBaseline.status !== expectedStatus)) {
    setGoalEditPending({ ...record, state: "unknown" });
    renderGoalStatus();
    throw new Error("目标操作回执内容无效。不会重复提交，请在桌面核对");
  }
  if (selectedId === record.payload.threadId && thread?.id === record.payload.threadId
      && sameGoalEditBaseline(thread.goal, {
        objective: record.payload.expectedObjective, status: record.payload.expectedStatus,
        createdAt: record.payload.expectedCreatedAt
      })) {
    setNotifiedGoal(updated);
    setGoalVersion(goalVersion + 1);
    thread.goal = updated;
  }
  if (goalEditDraft?.threadId === record.payload.threadId) setGoalEditDraft(null);
  renderGoalStatus();
  if (operation === "edit") notice("目标已更新");
  return "completed";
}
async function resolveGoalEditReceipt() {
  const record = goalEditPending;
  if (!record || !session || goalEditReceiptRequest) return goalEditReceiptRequest;
  if (record.deviceId !== session.device.id) {
    renderGoalStatus();
    notice("目标操作回执属于旧设备授权。请在桌面核对后解除锁定。", true);
    return null;
  }
  setGoalEditReceiptRequest((async () => {
    try {
      return await applyGoalEditReceipt(record, await api(`/api/receipts/${record.id}`));
    } catch (error) {
      if (goalEditPending?.id !== record.id) throw error;
      if (error.code === 404) {
        setGoalEditPending({ ...record, state: "unknown" });
        renderGoalStatus();
        notice("未找到目标操作回执。不会自动重试，请在桌面核对后解除锁定。", true);
      } else if (record.state === "pending") scheduleGoalEditReceipt();
      throw error;
    } finally { setGoalEditReceiptRequest(null); }
  })());
  return goalEditReceiptRequest;
}
function beginGoalEdit(goal) {
  if (threadReadOnly() || !selectedId || thread?.id !== selectedId) return;
  const pendingRecord = goalEditPending?.payload.threadId === thread?.id ? goalEditPending : null;
  const baseline = goalEditBaseline(goal) || (pendingRecord ? {
    objective: pendingRecord.payload.expectedObjective,
    status: pendingRecord.payload.expectedStatus,
    createdAt: pendingRecord.payload.expectedCreatedAt
  } : null);
  if (!baseline) { notice("当前目标不可编辑，请重新读取会话", true); return; }
  if (!pendingRecord && (threadReadOnly() || !connected)) {
    notice(threadReadOnly() ? "当前为只读访问，不能编辑目标" : "本机连接中断，不能编辑目标", true);
    return;
  }
  if (goalEditDraft?.threadId === thread?.id) {
    renderGoalEditor(goal, pendingRecord);
    queueMicrotask(() => $("#goal-edit-input")?.focus());
    return;
  }
  setGoalEditDraft({
    threadId: thread.id, baseline,
    objective: pendingRecord?.payload.objective || baseline.objective
  });
  renderGoalStatus();
  queueMicrotask(() => $("#goal-edit-input")?.focus());
}
function cancelGoalEdit() {
  const dialog = $("#goal-edit-dialog");
  setGoalEditDraft(null);
  if (dialog?.open) dialog.close("cancel");
  else dialog?.remove();
  renderGoalStatus();
  queueMicrotask(() => $("#goal-area .goal-edit-button")?.focus());
}
async function submitGoalEdit() {
  const draft = goalEditDraft;
  if (!draft || goalEditPending || threadReadOnly(draft.threadId) || !connected || selectedId !== draft.threadId
      || thread?.id !== draft.threadId) return;
  if (!sameGoalEditBaseline(thread.goal, draft.baseline)) {
    await reloadThread().catch(() => {});
    renderGoalStatus();
    notice("目标已发生变化，请重新打开后编辑", true);
    return;
  }
  const objective = draft.objective.trim();
  const payload = {
    threadId: draft.threadId, objective, expectedCreatedAt: draft.baseline.createdAt,
    expectedObjective: draft.baseline.objective, expectedStatus: draft.baseline.status
  };
  if (!validGoalEditPayload(payload)) { notice("目标内容不能为空或超过 16 KiB", true); return; }
  await submitGoalUpdate(payload);
}
async function changeGoalState(operation, goal) {
  const id = selectedId, baseline = goalEditBaseline(goal);
  if (!baseline || thread?.id !== id || goalEditPending || goalEditDraft
      || threadReadOnly(id) || !connected) return;
  const payload = { threadId: id, operation, objective: baseline.objective,
    expectedCreatedAt: baseline.createdAt, expectedObjective: baseline.objective,
    expectedStatus: baseline.status };
  if (!validGoalEditPayload(payload)) return;
  if (operation === "delete" && !await confirmAction("删除目标",
    "删除当前目标？会话消息会保留。", "删除目标", true)) return;
  if (selectedId !== id || thread?.id !== id || threadReadOnly(id) || !connected
      || goalEditPending || goalEditDraft) return;
  if (!sameGoalEditBaseline(thread.goal, baseline)) {
    notice("目标已发生变化，请确认当前状态后再操作", true);
    return;
  }
  await submitGoalUpdate(payload);
}
async function submitGoalUpdate(payload) {
  const record = { id: randomId(), deviceId: session.device.id, payload, state: "submitting" };
  try {
    setGoalEditPending(record, true);
    renderGoalStatus();
    const receipt = await api("/api/action/updateGoal", payload, record.id);
    await applyGoalEditReceipt(record, receipt);
  } catch (error) {
    if (goalEditPending?.id === record.id && error.code && error.code !== 503) setGoalEditPending(null);
    else if (goalEditPending?.id === record.id) setGoalEditPending({ ...record, state: "unknown" });
    renderGoalStatus();
    notice(error.message, true);
  }
}
function renderGoalEditor(goal, pendingRecord) {
  const draft = goalEditDraft?.threadId === thread?.id ? goalEditDraft : null;
  let dialog = $("#goal-edit-dialog");
  if (!draft) {
    if (dialog?.open) dialog.close("cancel");
    else dialog?.remove();
    return;
  }
  const baseline = draft.baseline;
  if (dialog?.dataset.threadId !== draft.threadId) {
    dialog?.remove();
    dialog = null;
  }
  if (dialog) {
    dialog.goalEditorSync?.(goal, pendingRecord);
    return;
  }

  const trigger = document.activeElement;
  dialog = element("dialog", "goal-edit-dialog");
  dialog.id = "goal-edit-dialog";
  dialog.dataset.threadId = draft.threadId;
  dialog.setAttribute("aria-labelledby", "goal-edit-title");
  const heading = element("div", "goal-edit-heading");
  const controlPending = pendingRecord && (pendingRecord.payload.operation ?? "edit") !== "edit";
  const title = element("h2", "", controlPending ? "核对目标操作" : "编辑目标");
  title.id = "goal-edit-title";
  const close = button("", "关闭目标编辑并取消草稿", cancelGoalEdit, "icon-button goal-edit-close");
  close.append(icon("close"));
  heading.append(title, close);
  const scroll = element("div", "goal-edit-scroll");
  const field = element("div", "goal-edit-field");
  const input = element("textarea", "goal-edit-input");
  input.id = "goal-edit-input";
  input.value = draft.objective;
  input.maxLength = isClaude() ? 4000 : 16 * 1024;
  input.rows = 4;
  input.setAttribute("aria-label", "目标内容");
  const count = element("span", "goal-edit-count muted");
  field.append(input, count);
  const status = element("div", "goal-edit-status muted");
  status.hidden = true;
  scroll.append(field, status);
  const actions = element("div", "goal-edit-actions");
  const receiptActions = element("div", "goal-edit-receipt-actions");
  const primaryActions = element("div", "goal-edit-primary-actions");
  const revert = button("还原", "还原目标编辑内容", () => {
    if (goalEditPending) return;
    input.value = baseline.objective;
    sync();
    input.focus();
  });
  const cancel = button("取消", "取消目标编辑草稿", cancelGoalEdit);
  const check = button("查回执", "只查询目标操作回执，不会重复提交", () => {
    check.disabled = true;
    void resolveGoalEditReceipt().catch(error => notice(`查询目标操作回执失败：${error.message}`, true))
      .finally(() => renderGoalStatus());
  });
  const unlock = button("已在桌面核对", "确认在桌面核对后解除目标操作锁定", async () => {
    const current = goalEditPending;
    if (await confirmAction("解除目标操作锁定",
      "请先在 Codex 桌面核对目标状态。解除锁定不会再次提交；仅在已确认操作完成或未执行后继续。",
      "已核对，解除锁定", true) && goalEditPending === current) {
      setGoalEditPending(null);
      setGoalEditDraft(null);
      renderGoalStatus();
    }
  });
  const save = button("保存", "保存目标内容", () => void submitGoalEdit(), "primary");
  receiptActions.append(check, unlock);
  primaryActions.append(revert, cancel, save);
  actions.append(receiptActions, primaryActions);
  dialog.append(heading, scroll, actions);
  document.body.append(dialog);

  const sync = (currentGoal = thread?.goal, currentRecord = goalEditPending?.payload.threadId === thread?.id ? goalEditPending : null) => {
    const bytes = new TextEncoder().encode(input.value.trim()).byteLength;
    const record = currentRecord;
    const controlPending = record && (record.payload.operation ?? "edit") !== "edit";
    title.textContent = controlPending ? "核对目标操作" : "编辑目标";
    const unavailable = !!record && record.deviceId !== session?.device.id;
    if (goalEditDraft?.threadId === thread?.id) goalEditDraft.objective = input.value;
    const tooLong = isClaude() ? input.value.trim().length > 4000 : bytes > 16 * 1024;
    count.textContent = isClaude() ? `${input.value.trim().length} / 4,000`
      : `${bytes.toLocaleString("zh-CN")} / 16,384 B`;
    count.classList.toggle("error-text", tooLong);
    const baselineChanged = !sameGoalEditBaseline(currentGoal, baseline);
    const feedbackReady = !!record && pendingFeedbackReady(record);
    status.textContent = record ? unavailable ? "回执属于旧设备授权，已锁定重复提交"
      : feedbackReady ? "正在确认目标操作结果。" : "正在处理目标操作…"
      : baselineChanged ? "原生目标已发生变化，当前草稿未丢失；请取消后重新打开编辑。" : "";
    status.hidden = !status.textContent;
    status.classList.toggle("error-text", unavailable || baselineChanged && !record);
    check.hidden = !record || unavailable || !feedbackReady;
    check.disabled = !connected || !!goalEditReceiptRequest;
    unlock.hidden = !record || !unavailable && !feedbackReady;
    revert.hidden = !!controlPending;
    save.hidden = !!controlPending;
    revert.disabled = !!record;
    input.disabled = !!record;
    save.disabled = !!record || threadReadOnly() || !connected || !input.value.trim()
      || tooLong || input.value.trim() === baseline.objective || baselineChanged;
    fitGoalEditor();
  };
  input.oninput = () => sync();
  input.onkeydown = event => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !save.disabled) {
      event.preventDefault();
      void submitGoalEdit();
    }
  };
  dialog.goalEditorSync = sync;
  let backdropPressed = false;
  const onBackdrop = event => {
    const box = dialog.getBoundingClientRect();
    return event.target === dialog && (event.clientX < box.left || event.clientX > box.right
      || event.clientY < box.top || event.clientY > box.bottom);
  };
  dialog.onpointerdown = event => { backdropPressed = onBackdrop(event); };
  dialog.onpointermove = event => { if (!onBackdrop(event)) backdropPressed = false; };
  dialog.onpointercancel = () => { backdropPressed = false; };
  dialog.onpointerup = event => {
    if (backdropPressed && onBackdrop(event)) cancelGoalEdit();
    backdropPressed = false;
  };
  dialog.oncancel = event => { event.preventDefault(); cancelGoalEdit(); };
  dialog.onclose = () => {
    dialog.remove();
    document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger?.isConnected && document.activeElement === document.body) trigger.focus();
  };
  document.body.classList.add("modal-open");
  dialog.showModal();
  sync(goal, pendingRecord);
  if (!input.disabled) input.focus();
}
export function renderGoalStatus() {
  syncPendingFeedbackTimer();
  const root = $("#goal-area");
  if (!root) { stopGoalLiveTimer(); return null; }
  const display = currentThreadDisplay(), goal = display?.goal;
  const ready = thread?.id === selectedId && Object.hasOwn(thread, "goal");
  const objective = typeof goal?.objective === "string" ? goal.objective.trim() : "";
  const status = typeof goal?.status === "string" ? goal.status : "";
  const hasCurrentGoal = !!objective && !!status && status !== "complete";
  const pendingRecord = goalEditPending?.payload.threadId === thread?.id ? goalEditPending : null;
  const editing = !!goalEditDraft && goalEditDraft.threadId === thread?.id;
  const signature = JSON.stringify([display?.id, objective, status, goal?.createdAt ?? null, goal?.reason ?? null, ready,
    threadReadOnly(), connected, editing, !!goalEditPending, !!goalEditDraft,
    pendingRecord ? [pendingRecord.id, pendingRecord.state, pendingRecord.deviceId === session?.device.id,
      pendingFeedbackReady(pendingRecord)] : null]);
  if (root.dataset.goalSignature === signature) {
    updateGoalLiveView(root, goal);
    renderGoalEditor(goal, pendingRecord);
    return root.firstElementChild;
  }
  const expanded = root.dataset.goalThread === display?.id && root.querySelector(".goal-card")?.open;
  root.dataset.goalSignature = signature;
  root.dataset.goalThread = display?.id || "";
  root.replaceChildren();
  root.hidden = !hasCurrentGoal && (!pendingRecord || !pendingFeedbackReady(pendingRecord));
  if (root.hidden) {
    stopGoalLiveTimer();
    renderGoalEditor(goal, pendingRecord);
    return null;
  }
  if (!hasCurrentGoal && pendingRecord) {
    stopGoalLiveTimer();
    const recovery = element("div", "goal-recovery-row");
    recovery.append(icon("target"), element("span", "", "正在确认目标操作结果。"));
    const recover = button("核对", "核对目标操作回执", () => beginGoalEdit(null), "text-button");
    recovery.append(recover);
    root.append(recovery);
    renderGoalEditor(goal, pendingRecord);
    if (pendingRecord.state === "pending") queueMicrotask(() => void resolveGoalEditReceipt().catch(() => {}));
    return recovery;
  }

  const state = goalStatusView(status);
  const shell = element("div", "goal-card-shell");
  const card = element("details", `goal-card goal-${state.tone}`);
  card.open = !!expanded;
  const summary = element("summary", "goal-summary");
  summary.title = "查看目标详情";
  const heading = element("span", "goal-heading");
  heading.append(icon("target"), element("strong", "", state.title));
  summary.append(heading, element("span", "goal-preview", objective));
  summary.append(element("span", "goal-duration"));
  const body = element("div", "goal-body");
  body.append(element("p", "goal-objective", objective));
  if (goal?.source === "claude" && typeof goal.reason === "string" && goal.reason)
    body.append(element("p", "muted", goal.reason));

  const metrics = element("dl", "goal-metrics");
  const addMetric = (key, label) => {
    const row = element("div", `goal-metric goal-metric-${key}`);
    row.append(element("dt", "", label), element("dd"));
    metrics.append(row);
  };
  addMetric("tokens-used", "已用 Token");
  addMetric("token-budget", "Token 预算");
  addMetric("tokens-remaining", "剩余 Token");
  addMetric("duration", "运行时间");
  // 沿用既有目标详情布局，只在原生明确报告轮次时显示，不增加固定占位。
  addMetric("iterations", "执行轮次");
  body.append(metrics);
  card.append(summary, body);
  const controls = element("div", "goal-controls");
  const edit = button("", pendingRecord ? "核对目标操作回执" : editing ? "继续编辑目标" : "编辑目标",
    () => beginGoalEdit(goal), "goal-edit-button");
  edit.replaceChildren(icon("edit"));
  edit.hidden = threadReadOnly();
  const editable = editableGoalStatuses.includes(status);
  edit.disabled = !ready || !pendingRecord && (!editable || threadReadOnly() || !connected);
  edit.title = pendingRecord ? "目标操作待确认，点击核对回执"
    : editing ? "继续编辑目标"
    : !editable ? "已完成目标不能编辑"
      : threadReadOnly() ? "当前为只读访问"
        : !connected ? "本机连接中断" : "编辑目标";
  const running = status === "active";
  const toggle = button("", running ? "暂停目标" : "开始目标",
    () => void changeGoalState(running ? "pause" : "resume", goal), "goal-action-button");
  toggle.append(icon(running ? "pause" : "play"));
  // Claude 首期仅对接原生创建/替换/清除；不把停止或重新创建伪装成暂停、恢复。
  toggle.hidden = goal?.source === "claude";
  const remove = button("", "删除目标", () => void changeGoalState("delete", goal),
    "goal-action-button goal-delete-button");
  remove.append(icon("trash"));
  const locked = !ready || !!goalEditPending || !!goalEditDraft || !connected || threadReadOnly();
  toggle.disabled = locked || !["active", "paused", "blocked"].includes(status);
  remove.disabled = locked || !editable;
  if (status === "budgetLimited" || status === "usageLimited")
    toggle.title = status === "budgetLimited" ? "目标预算已用尽" : "当前用量受限";
  controls.hidden = threadReadOnly();
  controls.append(edit, toggle, remove);
  shell.append(card, controls);
  root.append(shell);
  updateGoalLiveView(root, goal);
  renderGoalEditor(goal, pendingRecord);
  if (pendingRecord?.state === "pending") queueMicrotask(() => void resolveGoalEditReceipt().catch(() => {}));
  return card;
}
