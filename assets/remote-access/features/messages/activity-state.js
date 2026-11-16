// 活动分组状态：已渲染活动与计时器、最终消息定位，以及活动摘要更新。

import { pageLeaving, session } from "../../core/state.js";
import { claudeToolPresentation, genericToolPresentation, reasoningPreviewText } from "./item-presentation.js";
import { skillReadInfo } from "./tool-info.js";
import { textContent } from "./user-content.js";
import { currentThreadDisplay } from "../session/thread-cache.js";

let activityTimer = null;
export const renderedActivities = new Map();
export function finalMessageIndex(turn) {
  const items = turn.items || [];
  let trailingCompletedWork = false;
  for (let index = items.length - 1; index >= 0; index--) {
    const item = items[index];
    if (item.type === "contextCompaction") continue;
    if (item.type === "agentMessage") {
      if (!textContent(item).trim() || item.delivery === "async" || item.requestUserInputAsync != null) return -1;
      if (item.phase === "commentary") return -1;
      return !trailingCompletedWork || item.phase === "final_answer" ? index : -1;
    }
    if (turn.status !== "inProgress" && (item.type === "reasoning"
        || ["commandExecution", "dynamicToolCall"].includes(item.type) && !["inProgress", "running"].includes(item.status))) {
      trailingCompletedWork = true; continue;
    }
    return -1;
  }
  return -1;
}
export function claudeTurnAwaitingStart(turn) {
  // 用户要求已接收后持续提供反馈；只显示原生已接收的待启动轮次，不把等待状态当成执行或入队依据。
  return typeof turn?._nativeUserId === "string" && !!turn._nativeUserId
    && turn.status === "pending" && turn._nativeDone !== true
    && (turn.items || []).some(item => item.type === "userMessage" && !item.remotePendingInput)
    && !(turn.items || []).some(item => item.type === "agentMessage" && textContent(item).trim());
}
function activityTimestamp(value, milliseconds = false) {
  if (!Number.isFinite(value) || value <= 0) return null;
  return milliseconds || value >= 100_000_000_000 ? value : value * 1000;
}
export function activityPresentation(turn, running, now = Date.now()) {
  if (claudeTurnAwaitingStart(turn)) {
    // 原生确认等待须与执行区分；保留消息已接收事实，不假报执行中、停止或要求重发。
    const dialog = currentThreadDisplay()?.nativeDialog;
    const waiting = dialog?.state === "submitted" ? "正在确认操作结果"
      : dialog?.kind === "model" ? "等待确认模型切换"
      : dialog?.kind === "effort" ? "等待确认思考深度切换"
      : dialog ? "等待 Claude 本机确认" : "等待 Claude 开始";
    return { text: `已接收，${waiting}`, ticking: false, live: false };
  }
  const final = (turn.items || []).findLast(item => item.type === "agentMessage" && item.phase === "final_answer"
    && item.delivery !== "async" && item.requestUserInputAsync == null && textContent(item).trim());
  const started = activityTimestamp(turn.turnStartedAtMs, true) ?? activityTimestamp(turn.startedAt);
  const live = running && !final;
  let duration = null;
  if (turn.status !== "interrupted") {
    if (live && started != null) duration = Math.max(now - started, 0);
    else if (!live && Number.isFinite(turn.durationMs) && turn.durationMs >= 0) duration = turn.durationMs;
    else if (!live && started != null) {
      const completed = activityTimestamp(turn.turnCompletedAtMs, true) ?? activityTimestamp(turn.completedAt)
        ?? activityTimestamp(final?.startedAtMs ?? final?.sentAtMs ?? final?.messageMetadata?.sentAtMs, true);
      if (completed != null && completed >= started) duration = completed - started;
    }
  }
  if (duration == null) return { text: "活动", ticking: false, live };
  const prefix = live ? "已处理" : "用时";
  // 用户要求会话活动耗时从 1 秒开始显示；只调整展示秒数，保留原始毫秒耗时和真实计时状态。
  const seconds = Math.max(1, Math.floor(duration / 1000)), minutes = Math.floor(seconds / 60), hours = Math.floor(minutes / 60);
  const time = `${hours ? `${hours}小时` : ""}${minutes % 60 ? `${minutes % 60}分` : ""}${seconds % 60 || !minutes ? `${seconds % 60}秒` : ""}`;
  return { text: `${prefix} ${time}`, ticking: live, live };
}
export function updateActivitySummary(activity) {
  const presentation = activityPresentation(activity.turn, activity.running);
  activity.ticking = activity.showSummary && presentation.ticking;
  activity.summary.setAttribute("aria-label", `${presentation.text}${activity.workCount ? `，${activity.workCount} 条记录` : ""}${activity.running ? "，仍在处理" : ""}`);
  activity.summary.title = presentation.text;
  if (activity.label.textContent !== presentation.text) activity.label.textContent = presentation.text;
}
export function stopActivityTimer() {
  clearInterval(activityTimer); activityTimer = null;
}
export function syncActivityTimer() {
  if (pageLeaving || document.hidden || !session) { stopActivityTimer(); return; }
  let ticking = false;
  for (const activity of renderedActivities.values()) {
    if (!activity.node.isConnected || !activity.showSummary) continue;
    updateActivitySummary(activity);
    ticking ||= activity.ticking;
  }
  if (!ticking) stopActivityTimer();
  else if (activityTimer == null) activityTimer = setInterval(syncActivityTimer, 1000);
}
export function workGroupSummary(items, running = false) {
  const commands = items.filter(item => item.type === "commandExecution" && !skillReadInfo(item));
  const changes = items.filter(item => item.type === "fileChange");
  const tools = items.filter(item => ["mcpToolCall", "dynamicToolCall"].includes(item.type));
  // Claude 内置工具不是集成：按动作汇总为“浏览了文件 / 使用了工具”，Claude 的 MCP 工具仍按服务器名显示集成。
  const claudeTools = tools.filter(item => item.source === "claude" && item.type === "dynamicToolCall");
  const integrationTools = tools.filter(item => !claudeTools.includes(item));
  const explored = claudeTools.some(item => claudeToolPresentation(item).explore);
  const usedTools = claudeTools.some(item => !claudeToolPresentation(item).explore);
  const searches = items.filter(item => item.type === "webSearch");
  const reasoning = items.filter(item => item.type === "reasoning");
  const paths = new Set(changes.flatMap(item => item.changes || []).map(change => change.path).filter(Boolean));
  const toolName = item => {
    const surface = item.result?._meta?.["codex/toolSurface"];
    if (surface?.kind === "browserUse") {
      if (surface.backend === "chrome" || surface.browserFamily === "chrome") return "Chrome";
      if (surface.backend === "edge" || surface.browserFamily === "edge") return "Edge";
      return "浏览器";
    }
    if (item.pluginId === "unified-computer-use@openai-bundled") return "Chrome";
    return item.server || item.tool || "工具";
  };
  const names = [...new Set(integrationTools.map(toolName))];
  const name = names.length > 1 ? `${names.slice(0, -1).join("、")}和${names.at(-1)}` : names[0];
  const integrations = names.length > 0 && !names.includes("浏览器");
  const edited = changes.length ? "编辑了多个文件" : "";
  let text = `${name ? `已使用 ${name}${integrations ? " 集成" : ""}` : ""}${explored ? "浏览了文件" : ""}${usedTools ? "使用了工具" : ""}${searches.length ? "搜索了网页" : ""}${edited}${commands.length ? "运行了命令" : ""}` || (reasoning.length ? "思考" : "已完成操作");
  let currentTool = null, thinking = false;
  if (running) {
    const current = items.at(-1);
    thinking = current?.type === "reasoning"
      || ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(current?.status);
    if (!thinking && current && tools.includes(current)) currentTool = genericToolPresentation(current);
    text = thinking ? current?.type === "reasoning"
      ? reasoningPreviewText(current).replace(/\s+/gu, " ").trim() || "正在思考" : "正在思考"
      : current?.type === "commandExecution" && !skillReadInfo(current) ? "正在运行命令"
      : current?.type === "fileChange" ? "正在修改文件"
      : current?.type === "webSearch" ? "正在搜索网页"
      : currentTool?.description || "正在执行操作";
  }
  const failed = items.filter(item => item.status === "failed"
    || item.type === "commandExecution" && item.exitCode != null && item.exitCode !== 0).length;
  const counts = [
    commands.length ? `${commands.length} 条命令` : "",
    tools.length ? `${tools.length} 次工具调用` : "",
    changes.length ? paths.size ? `${paths.size} 个文件修改` : "有文件修改" : "",
    searches.length ? `${searches.length} 次网页搜索` : "",
    reasoning.length ? `${reasoning.length} 条思考记录` : "",
    failed ? `${failed} 项失败，展开查看详情` : ""
  ].filter(Boolean);
  return {
    text, thinking, description: counts.join("，"),
    icon: currentTool?.icon || (names.length === 1 && ["Chrome", "Edge"].includes(name) || !tools.length && searches.length
      ? "globe" : tools.length ? "tools" : changes.length ? "fileChange" : "terminal")
  };
}
