// 会话能力与访问限制：后端类型与实例能力、授权范围键、关联视图校验，以及只读/侧边/子代理等访问限制的统一查询。

import { creating, selectedId, session, thread } from "../../core/state.js";
import { modelContextKey, modelDefaults, modelsScope } from "../composer/models.js";
import { creationBackend } from "../composer/new-chat.js";
import { availableSessionInstances, sessionInstanceEnabled } from "./instances.js";
import { relatedNavigationOwner, relatedSessionView, relatedThreadViews, relatedViewForThread } from "./related.js";

export function claudeThreadId(id) {
  // 子代理 ID 绑定远程主会话及原生 agentId，仍走 Claude 读取路由，不能误投 Codex。
  return typeof id === "string" && /^claude-(?:[0-9a-f-]{36}|agent-[0-9a-f-]{36}-[A-Za-z0-9_-]{1,64})$/i.test(id);
}
export function activeBackend() {
  return creating ? creationBackend : conversationBackend(thread || { id: selectedId });
}
export function isClaude() { return activeBackend() === "claude"; }
export function claudeCommandSupported(name) {
  // 原生命令只接入共享 CLI 会话，旧 print 会话不能借用实例目录的能力开放入口。
  if (!creating && thread?.capabilities?.queue !== true) return false;
  const catalog = modelsScope === modelContextKey() ? modelDefaults.nativeCapabilities : null;
  return typeof catalog?.[name] === "boolean" ? catalog[name]
    : !creating && thread?.capabilities?.[name === "goal" ? "goalComposer" : name] === true;
}
export function newChatBackendOptions() {
  // 用户要求类型与环境实例分别控制；未接通的执行适配器只在设置中展示，不能悄悄改投默认实例。
  return availableSessionInstances().filter(instance => instance.ready && sessionInstanceEnabled(instance.id))
    .map(instance => ({ value: instance.id, backend: instance.backend, label: instance.label,
      icon: ["codex", "claude"].includes(instance.backend) ? instance.backend : "allSessions" }));
}
export function threadLocation(value) {
  // 用户要求顶部路径前仅显示非本机实例的实际环境名（如发行版名），不拼接类型、“本机”或“WSL”标签。
  const distribution = value?.wslDistribution
    || availableSessionInstances().find(instance => instance.id === value?.instanceId)?.distribution;
  return [distribution, value?.cwd].filter(Boolean).join(" · ");
}
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
export function threadSentMessageEditAllowed(id = selectedId) {
  if (claudeThreadId(id)) return thread?.id === id && thread.capabilities?.messageEdit === true
    && thread.messageEditReady === true && !threadReadOnly(id);
  if (!id) return false;
  const view = relatedViewForThread(id);
  return view?.kind !== "side" && !(thread?.id === id && thread.viewKind === "side");
}
export function isActiveThread() {
  const status = thread?.status?.status || thread?.status;
  const statusType = typeof status === "string" ? status : status?.type;
  return ["active", "inProgress", "running", "pending"].includes(statusType);
}
