// 会话能力与访问限制：后端类型与实例能力、授权范围键、关联视图校验，以及只读/侧边/子代理等访问限制的统一查询；依赖模型目录与关联视图的动作能力在 action-capabilities.js。

import { creating, selectedId, session, thread } from "../../core/state.js";
import { creationBackend } from "../composer/creation-state.js";
import { relatedNavigationOwner, relatedSessionView, relatedThreadViews } from "./related-state.js";

export function claudeThreadId(id) {
  // 子代理 ID 绑定远程主会话及原生 agentId，仍走 Claude 读取路由，不能误投 Codex。
  return typeof id === "string" && /^claude-(?:[0-9a-f-]{36}|agent-[0-9a-f-]{36}-[A-Za-z0-9_-]{1,64})$/i.test(id);
}
export function activeBackend() {
  return creating ? creationBackend : conversationBackend(thread || { id: selectedId });
}
export function isClaude() { return activeBackend() === "claude"; }
export function claudeNativeQueue() { return isClaude() && thread?.capabilities?.queue === true; }
export function queueCapability(name, id = selectedId) {
  // Claude 队列能力由当前会话报告；Codex 桌面离线时也必须保留已支持的图片编辑与回执恢复。
  return claudeThreadId(id) || thread?.id === id && thread?.backend === "claude"
    ? thread?.id === id && thread.capabilities?.[name] === true : session?.capabilities?.[name] === true;
}
export function backendAvailable(value = session) {
  // 用户要求 Codex 仅使用服务模式；旧桌面连接标记不能让服务离线的实例显示为可发送。
  return isClaude() ? value?.capabilities?.claudeSessions === true : value?.serviceConnected === true;
}
export function approvalsAllowed() { return isClaude() ? session?.claudeAllowApprovals === true : session?.allowApprovals === true; }
export function backendCapability(name) {
  return isClaude() && ["imageInput", "projectlessCreation", "sendText"].includes(name)
    ? session?.capabilities?.claudeSessions === true : session?.capabilities?.[name] === true;
}
export function conversationBackend(item) {
  return item?.backend || (claudeThreadId(item?.id) ? "claude" : "codex");
}
export function privateInputScope(value) {
  return typeof value?.device?.id === "string"
    ? JSON.stringify([value.device.id, value.device.createdAtMs ?? null]) : null;
}
export function validRelatedThreadId(value) {
  return typeof value === "string" && /^[A-Za-z0-9_-]{1,120}$/.test(value);
}
export function normalizeRelatedView(value, id = value?.threadId) {
  const scope = privateInputScope(session);
  const parent = value?.parentThreadId, source = value?.openedFromThreadId;
  return scope && value?.scope === scope && value.threadId === id && validRelatedThreadId(id)
    && (parent == null || validRelatedThreadId(parent) && id !== parent)
    && (source == null || validRelatedThreadId(source) && id !== source)
    && (parent != null || source != null)
    && ["subagent", "side"].includes(value.kind)
    ? { scope, threadId: id, ...(parent != null ? { parentThreadId: parent } : {}),
      ...(source != null ? { openedFromThreadId: source } : {}), kind: value.kind,
      ...(typeof value.name === "string" ? { name: value.name } : {}),
      ...(typeof value.ephemeral === "boolean" ? { ephemeral: value.ephemeral } : {}),
      ...(value.expired === true ? { expired: true } : {}),
      ...(value.returnToParent === true ? { returnToParent: true } : {}),
      ...(value.owner === relatedNavigationOwner ? { owner: value.owner } : {}) } : null;
}
export function subagentsEnabled() {
  // Claude 仅在当前后台明确声明读取能力时复用子代理列表/只读详情；侧边会话能力独立处理。
  if (isClaude()) return thread?.capabilities?.subagents === true;
  // 子智能体与侧边会话使用独立能力字段，兼容旧后台并分别约束只读与写入入口。
  return session?.capabilities?.serviceSubagents === true;
}
export function sideThreadsEnabled() {
  if (isClaude()) return false;
  // 用户选择服务独立管理临时侧边会话；只有新后台声明能力时开放，旧后台仍保持禁用。
  return session?.capabilities?.serviceSideThreads === true;
}
export function serviceSideThread() {
  return thread?.id === selectedId && thread?.serviceSide === true;
}
export function threadReadOnly(id = selectedId) {
  return !!session?.readOnly || !!id && (
    thread?.id === id && (thread.readOnly === true || thread.viewKind === "subagent")
    || normalizeRelatedView(relatedThreadViews.get(id), id)?.kind === "subagent"
    || relatedSessionView?.threadId === id && relatedSessionView.kind === "subagent"
  );
}
export function threadActionReadOnly(action, id = selectedId) {
  if (!threadReadOnly(id)) return false;
  // 用户要求过期侧边阅读后可确认已读；三个操作入口共用此判断，设备只读授权和子代理仍禁止提交。
  return !(action === "markRead" && !session?.readOnly && thread?.id === id
    && thread.viewKind === "side" && thread.sideThreadExpired === true);
}
export function threadReadOnlyMessage() {
  return session?.readOnly ? "当前设备为只读模式" : "当前会话仅支持只读查看";
}
export function isActiveThread() {
  const status = thread?.status?.status || thread?.status;
  const statusType = typeof status === "string" ? status : status?.type;
  return ["active", "inProgress", "running", "pending"].includes(statusType);
}
