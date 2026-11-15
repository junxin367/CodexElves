// 目标共享状态：目标状态展示与计时，目标编辑草稿与回执状态，目标输入的恢复记录与校验辅助。

import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { connected, pageLeaving, selectedId, session, thread } from "../../core/state.js";

let goalEditorBaseViewport = {
  width: window.innerWidth,
  height: window.visualViewport?.height || window.innerHeight
};
export let goalVersion = 0, notifiedGoal = null;
export function setGoalVersion(value) { return goalVersion = value; }
export function setNotifiedGoal(value) { return notifiedGoal = value; }
const goalStatusLabels = {
  active: "运行中",
  paused: "已暂停",
  blocked: "已阻塞",
  budgetLimited: "预算受限",
  usageLimited: "用量受限",
  complete: "已完成"
};
const goalStatusTitles = {
  active: "进行中的目标",
  paused: "已暂停的目标",
  blocked: "已阻塞的目标",
  budgetLimited: "预算受限的目标",
  usageLimited: "用量受限的目标",
  complete: "已完成的目标"
};
export function goalStatusView(status) {
  const tone = ["active", "paused", "blocked", "budgetLimited", "usageLimited", "complete"].includes(status) ? status : "unknown";
  return { label: goalStatusLabels[status] || status || "状态未知", title: goalStatusTitles[status] || "目标", tone };
}
function formatGoalTokens(value) {
  if (!Number.isFinite(value) || value < 0) return null;
  return new Intl.NumberFormat("zh-CN").format(Math.floor(value));
}
function formatGoalDuration(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) return null;
  let remaining = Math.floor(seconds);
  const days = Math.floor(remaining / 86400); remaining %= 86400;
  const hours = Math.floor(remaining / 3600); remaining %= 3600;
  const minutes = Math.floor(remaining / 60); remaining %= 60;
  const parts = [];
  if (days) parts.push(`${days}d`);
  if (hours || days) parts.push(`${hours}h`);
  if (minutes || hours || days) parts.push(`${minutes}m`);
  if (remaining || !parts.length) parts.push(`${remaining}s`);
  return parts.join("");
}
let goalLiveTimer = null;
function goalDurationSeconds(goal, now = Date.now()) {
  const seconds = goal?.timeUsedSeconds;
  if (!Number.isFinite(seconds) || seconds < 0) return null;
  // Claude 只展示原生返回的耗时，等待评估期间不自行累计。
  if (goal.source === "claude") return seconds;
  if (goal.status !== "active" || !Number.isFinite(goal.updatedAt) || goal.updatedAt < 0) return seconds;
  const milliseconds = seconds * 1000 + now - goal.updatedAt * 1000;
  return Number.isFinite(milliseconds) ? Math.max(milliseconds, 0) / 1000 : seconds;
}
export function stopGoalLiveTimer() {
  clearInterval(goalLiveTimer);
  goalLiveTimer = null;
}
export function syncGoalLiveTimer() {
  const root = $("#goal-area"), goal = thread?.goal;
  if (!session || !connected || pageLeaving || document.hidden
      || !root || root.hidden || thread?.id !== selectedId || root.dataset.goalThread !== selectedId
      || goal?.source === "claude" || goal?.status !== "active" || !Number.isFinite(goal.timeUsedSeconds) || goal.timeUsedSeconds < 0
      || !Number.isFinite(goal.updatedAt) || goal.updatedAt < 0) {
    stopGoalLiveTimer();
    return;
  }
  updateGoalDurationView(root, goal);
  if (goalLiveTimer == null) goalLiveTimer = setInterval(syncGoalLiveTimer, 1000);
}
const goalEditStorageKey = "codex-elves-goal-edit-pending";
export const editableGoalStatuses = ["active", "paused", "blocked", "budgetLimited", "usageLimited"];
export function validGoalEditPayload(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const operation = value.operation ?? "edit";
  const bytes = text => typeof text === "string" ? new TextEncoder().encode(text.trim()).byteLength : 0;
  return typeof value.threadId === "string" && /^[a-zA-Z0-9_-]{1,120}$/.test(value.threadId)
    && bytes(value.objective) > 0 && bytes(value.objective) <= 16 * 1024
    && Number.isSafeInteger(value.expectedCreatedAt) && value.expectedCreatedAt >= 0
    && bytes(value.expectedObjective) > 0 && bytes(value.expectedObjective) <= 16 * 1024
    && editableGoalStatuses.includes(value.expectedStatus)
    && ["edit", "pause", "resume", "delete"].includes(operation)
    && (operation === "edit" || value.objective.trim() === value.expectedObjective.trim())
    && (operation !== "pause" || value.expectedStatus === "active")
    && (operation !== "resume" || ["paused", "blocked"].includes(value.expectedStatus));
}
export let goalEditPending = (() => {
  try {
    const value = JSON.parse(sessionStorage.getItem(goalEditStorageKey) || "null");
    return value && typeof value.id === "string" && /^[a-zA-Z0-9-]{1,100}$/.test(value.id)
      && typeof value.deviceId === "string" && validGoalEditPayload(value.payload)
      && ["submitting", "pending", "unknown"].includes(value.state)
      ? { ...value, state: value.state === "submitting" ? "unknown" : value.state } : null;
  } catch { return null; }
})();
export function assignGoalEditPending(value) { return goalEditPending = value; }
export let goalEditReceiptRequest = null, goalEditPollTimer = null, goalEditDraft = null;
export function setGoalEditReceiptRequest(value) { return goalEditReceiptRequest = value; }
export function setGoalEditPollTimer(value) { return goalEditPollTimer = value; }
export function setGoalEditDraft(value) { return goalEditDraft = value; }
export function setGoalEditPending(value, required = false) {
  if (value && !Number.isFinite(value.time)) value.time = Date.now();
  try {
    if (value) sessionStorage.setItem(goalEditStorageKey, JSON.stringify(value));
    else sessionStorage.removeItem(goalEditStorageKey);
  } catch {
    if (required) throw new Error("无法保存目标操作回执，操作未提交。请允许此网页使用浏览器会话存储。");
    notice("无法保存目标操作回执；请在本机核对目标状态。", true);
  }
  clearTimeout(goalEditPollTimer);
  goalEditPollTimer = null;
  goalEditPending = value;
}
export function goalEditBaseline(goal) {
  const objective = typeof goal?.objective === "string" ? goal.objective.trim() : "";
  const status = typeof goal?.status === "string" ? goal.status : "";
  const createdAt = Number.isFinite(goal?.createdAt) && goal.createdAt >= 0 ? Math.floor(goal.createdAt) : null;
  return objective && editableGoalStatuses.includes(status) && createdAt != null
    ? { objective, status, createdAt } : null;
}
export function sameGoalEditBaseline(goal, baseline) {
  const current = goalEditBaseline(goal);
  return !!current && !!baseline && current.objective === baseline.objective
    && current.status === baseline.status && current.createdAt === baseline.createdAt;
}
export function fitGoalEditor() {
  const viewport = window.visualViewport;
  const width = window.innerWidth, height = viewport?.height || window.innerHeight;
  // 同一宽度下保留键盘收起时的高度；转屏后重新建立基准。
  goalEditorBaseViewport = {
    width,
    height: width === goalEditorBaseViewport.width ? Math.max(goalEditorBaseViewport.height, height) : height
  };
  const dialog = $("#goal-edit-dialog");
  if (!dialog?.open) return;
  if (width <= 760) {
    if (dialog.goalEditorViewportWidth === width) return;
    dialog.goalEditorViewportWidth = width;
    // 打开时锁定尺寸，后续键盘引起的 visualViewport resize/scroll 不再改变弹窗。
    dialog.style.setProperty("--goal-edit-viewport-height", `${goalEditorBaseViewport.height}px`);
    return;
  }
  delete dialog.goalEditorViewportWidth;
  dialog.style.setProperty("--goal-edit-viewport-height", `${height}px`);
  dialog.style.setProperty("--goal-edit-viewport-top", `${viewport?.offsetTop || 0}px`);
}
function updateGoalDurationView(root, goal) {
  const duration = formatGoalDuration(goalDurationSeconds(goal));
  const durationNode = root.querySelector(".goal-duration");
  if (durationNode) {
    if (durationNode.textContent !== (duration || "")) durationNode.textContent = duration || "";
    durationNode.hidden = duration == null;
  }
  const row = root.querySelector(".goal-metric-duration");
  if (row) {
    row.hidden = duration == null;
    const output = row.querySelector("dd");
    if (output && duration != null && output.textContent !== duration) output.textContent = duration;
  }
  return duration;
}
export function updateGoalLiveView(root, goal) {
  const duration = updateGoalDurationView(root, goal);
  const tokensUsed = formatGoalTokens(goal?.tokensUsed);
  const tokenBudget = formatGoalTokens(goal?.tokenBudget);
  const values = {
    "tokens-used": tokensUsed,
    "token-budget": goal?.tokenBudget == null ? goal?.source === "claude" ? null : "无上限" : tokenBudget,
    "tokens-remaining": tokenBudget != null && tokensUsed != null
      ? formatGoalTokens(Math.max(0, goal.tokenBudget - goal.tokensUsed)) : null,
    iterations: goal?.source === "claude" && Number.isSafeInteger(goal.iterations) && goal.iterations >= 0
      ? String(goal.iterations) : null,
    duration
  };
  const metrics = root.querySelector(".goal-metrics");
  if (metrics) {
    let visible = 0;
    for (const [key, value] of Object.entries(values)) {
      const row = root.querySelector(`.goal-metric-${key}`);
      if (!row) continue;
      row.hidden = value == null;
      const output = row.querySelector("dd");
      if (output && value != null) output.textContent = value;
      if (value != null) visible++;
    }
    metrics.hidden = visible === 0;
  }
  syncGoalLiveTimer();
}

export let selectedGoal = null;
export function setSelectedGoal(value) { return selectedGoal = value; }
export function goalComposerAction(action, payload) {
  return payload?.goalMode === true && ["create", "setGoal"].includes(action);
}
export function goalSubmissionState(value, action) {
  return value && ["create", "setGoal"].includes(value.action) && (!action || value.action === action)
    && ["preparing", "firstTurn", "goalSet", "complete"].includes(value.phase)
    && (value.threadId === null || typeof value.threadId === "string" && /^[a-zA-Z0-9_-]{1,120}$/.test(value.threadId))
    && typeof value.firstTurnAccepted === "boolean"
    && ["not-attempted", "succeeded", "failed", "unknown"].includes(value.goalSet) ? value : null;
}
export function hydrateGoalComposerRecord(record) {
  if (!goalComposerAction(record?.action, record?.payload) || !record.draft) return record;
  return { ...record, draft: { ...record.draft, attachments: (record.draft.attachments || []).map(entry => {
    if (entry.kind !== "inline" || !Object.hasOwn(entry, "goalImageIndex")) return entry;
    const image = Number.isInteger(entry.goalImageIndex) && record.payload.images?.[entry.goalImageIndex];
    if (!image || typeof image.dataUrl !== "string") { record.invalid = true; return entry; }
    const { goalImageIndex, ...metadata } = entry;
    return { ...metadata, dataUrl: image.dataUrl };
  }) }, ...(record.invalid ? { invalid: true } : {}) };
}
export const goalComposerRecoveries = new Map();
export const goalComposerPlaceholder = "描述你的目标，定义可衡量的成果，以获得最佳效果";
export function unfinishedGoal(goal = thread?.goal) {
  return !!goal && goal.status !== "complete" && goal.status !== "completed";
}
export function goalCommandMatches(query) {
  return ["目标", "goal", "持续追求"].some(value => value.includes(query));
}
export function goalRecordThreadId(record) {
  return goalSubmissionState(record?.goalSubmission, record?.action)?.threadId
    || (typeof record?.result?.threadId === "string" ? record.result.threadId : null)
    || record?.payload?.threadId || null;
}
export function goalReceiptOutcome(receipt, action) {
  const result = receipt?.result || {}, state = goalSubmissionState(result.goalSubmission, action);
  if (receipt?.state === "completed" && result.ok === true
      && state?.phase === "complete" && state.goalSet === "succeeded") return "succeeded";
  if (receipt?.state === "pending") return "pending";
  if (receipt?.state === "unknown" || state?.goalSet === "unknown"
      || result.outcomeUnknown === true && state?.goalSet !== "failed") return "unknown";
  if (receipt?.state === "failed" || receipt?.state === "completed" && result.ok === false) return "failed";
  return "unknown";
}
export function goalComposerReceiptMessage(record, outcome) {
  const state = goalSubmissionState(record.goalSubmission, record.action);
  if (record.action === "create" && state?.firstTurnAccepted) {
    if (outcome === "succeeded") return "会话和目标已创建";
    if (outcome === "failed") return "会话及首条输入已创建，但目标未设置成功。草稿已保存在该会话，只能重试设置目标。";
    return "会话及首条输入已创建，目标设置结果待确认。请查原回执或在本机核对，不会重建会话或重发首条。";
  }
  if (record.action === "create" && (record.confirmedCreated || record.result?.status === "created")) {
    return outcome === "failed"
      ? "会话已创建，首条输入未确认接受，目标尚未设置。草稿已保存在该会话，后续只会设置目标。"
      : "会话已创建，首条输入或目标结果待确认。原回执已保留，不会重建会话或重发首条。";
  }
  if (outcome === "succeeded") return "目标已创建";
  if (outcome === "failed") return record.result?.message || "目标未创建，草稿与目标模式已保留";
  return record.action === "create"
    ? "新建目标的首条输入结果待确认。草稿与原回执已保留，不会自动重建会话或重发。"
    : "目标设置结果待确认。草稿与原回执已保留，不会自动重复设置。";
}
