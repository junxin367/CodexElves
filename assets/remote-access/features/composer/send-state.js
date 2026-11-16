// 发送状态：发送中标记与诊断状态、待确认反馈的延迟与计时器、排队编辑是否带原图片的判断，以及 Claude 新建会话前的项目目录信任确认；输入区、审批、目标与队列共用。

import { confirmAction } from "../../ui/dialog.js";
import { api } from "../api.js";

export let sending = null;
export function setSending(value) { return sending = value; }
export let composerDiagnosticState = "";
export function setComposerDiagnosticState(value) { return composerDiagnosticState = value; }
const pendingFeedbackDelayMs = 5000;
let pendingFeedbackTimer = null;
export function setPendingFeedbackTimer(value) { return pendingFeedbackTimer = value; }
export function pendingFeedbackDelay(record) {
  // Claude 压缩命令受理后迟迟不开始才提示，时长与 compaction.js 的 nativeCompactionStartGraceMs 保持一致。
  if (record?.action === "nativeCompactionStart") return 20000;
  // 用户要求：队列操作未确认满 8 秒才补充等待提醒；其他操作仍为 5 秒，不改变真实失败提示或回执轮询。
  return ["steerQueued", "removeQueued", "reorderQueued", "beginQueuedEdit", "editQueued", "cancelQueuedEdit", "dismissQueuedEdit"]
    .includes(record?.action) ? 8000 : pendingFeedbackDelayMs;
}
export function pendingFeedbackReady(record) {
  return !Number.isFinite(record?.time) || Date.now() - record.time >= pendingFeedbackDelay(record);
}
export function clearPendingFeedbackTimer() {
  clearTimeout(pendingFeedbackTimer);
  pendingFeedbackTimer = null;
}
export function queuedEditHasOriginalImages(edit) {
  // 用户要求普通队列消息恢复到输入框后按普通消息编辑；只有原消息确实带图片时，
  // 才显示“原图片会保留”提示，不能因为处于队列编辑态就误报图片限制。
  return edit?.imageRemoval === true || edit?.imageEditing === true
    || Array.isArray(edit?.originalImageSources) && edit.originalImageSources.length > 0
    || Array.isArray(edit?.attachments) && edit.attachments.length > 0;
}
export async function prepareClaudeProjectCreation(payload, stillCurrent) {
  const result = await api("/api/read/projectTrust", {
    backend: "claude", instanceId: payload.instanceId, projectId: payload.projectId
  });
  if (!stillCurrent()) return false;
  if (result?.instanceId !== payload.instanceId || typeof result?.trustPath !== "string" || !result.trustPath)
    throw new Error("项目目录信任检查结果不匹配，草稿已保留，请重新选择项目后重试。");
  if (result.trusted === true) return true;
  // 用户要求在远程完成一次目录信任；明确显示实例和真实信任边界，取消和切换项目均不写入或发送。
  const confirmed = await confirmAction("信任 Claude 项目目录",
    `实例：${result.instanceLabel || payload.instanceId}\n目录：${result.trustPath}\n`
      + "允许 Claude 加载此项目的规则和工具配置。仅确认你信任的项目；确认后继续发送当前消息。",
    "信任并继续", false, "project-trust-confirmation");
  if (!confirmed || !stillCurrent()) return false;
  payload.trustProject = true;
  payload.expectedProjectPath = result.trustPath;
  return true;
}
