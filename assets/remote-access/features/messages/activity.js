// 执行活动：最终消息定位、活动摘要与计时、命令分组渲染。

import { element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { pageLeaving, session, thread } from "../../core/state.js";
import { compactionLocked } from "../conversation/compaction.js";
import { scheduleHistoryLoad, scheduleMessages } from "../conversation/history-view.js";
import { claudeToolPresentation, genericToolPresentation, reasoningPreviewText, reconcileChildren } from "./items.js";
import { renderImageViewGroup, setMarkdown } from "./media.js";
import { renderToolOutput, skillReadInfo } from "./tool-output.js";
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
  if (duration < 1000) return { text: `${prefix} <1秒`, ticking: live, live };
  const seconds = Math.floor(duration / 1000), minutes = Math.floor(seconds / 60), hours = Math.floor(minutes / 60);
  const time = `${hours ? `${hours}小时` : ""}${minutes % 60 ? `${minutes % 60}分` : ""}${seconds % 60 || !minutes ? `${seconds % 60}秒` : ""}`;
  return { text: `${prefix} ${time}`, ticking: live, live };
}
function updateActivitySummary(activity) {
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
function workGroupSummary(items, running = false) {
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
function renderCommandGroup(items, previous, renderMessage, running = false, historyLoad = null) {
  const records = Array.isArray(items) ? items : [];
  const group = previous?.classList?.contains("command-group") ? previous : element("details", "command-group");
  let state = group.commandGroupState;
  if (!state) {
    const summary = element("summary", "command-group-summary");
    const visual = element("span", "command-group-state");
    const title = element("span", "command-group-title");
    title.setAttribute("role", "status");
    title.setAttribute("aria-live", "polite");
    const chevron = icon("chevron");
    chevron.classList.add("command-group-chevron");
    summary.append(visual, title, chevron);
    const panel = element("div", "command-group-panel");
    group.append(summary, panel);
    state = {
      summary, visual, title, panel,
      items: [], renderMessage: null, scrollTop: 0, openItems: new Set(),
      renderBody() {
        const scrollTop = panel.childElementCount ? panel.scrollTop : state.scrollTop;
        if (panel.childElementCount) state.openItems = new Set(
          [...panel.querySelectorAll(":scope > details[data-command-group-item][open]")]
            .map(node => node.dataset.commandGroupItem));
        const children = state.items.map((item, index) => {
          const key = String(item.id ?? index);
          const node = state.renderMessage(item, true);
          node.dataset.commandGroupItem = key;
          if (state.openItems.has(key) && node instanceof HTMLDetailsElement && !node.open) {
            node.open = true;
            renderToolOutput(node, item);
          }
          return node;
        });
        if (state.historyLoad) children.unshift(state.historyLoad);
        reconcileChildren(panel, children);
        panel.scrollTop = scrollTop;
      },
      clearBody() {
        if (panel.childElementCount) {
          state.scrollTop = panel.scrollTop;
          state.openItems = new Set(
            [...panel.querySelectorAll(":scope > details[data-command-group-item][open]")]
              .map(node => node.dataset.commandGroupItem));
        }
        panel.replaceChildren();
      }
    };
    group.commandGroupState = state;
    group.ontoggle = () => {
      if (!group.isConnected) return;
      if (group.open) {
        state.renderBody();
        if (state.historyLoad) scheduleHistoryLoad(true);
      }
      else state.clearBody();
    };
  }
  state.items = records;
  state.renderMessage = renderMessage;
  state.historyLoad = historyLoad;
  const active = running;
  const presentation = workGroupSummary(records, active);
  if (!records.length && historyLoad) {
    presentation.text = "较早的过程记录";
    presentation.description = "展开后加载较早的过程记录";
  }
  const groupClass = `command-group${active ? " running" : ""}${presentation.thinking ? " thinking-group" : ""}`;
  if (group.className !== groupClass) group.className = groupClass;
  if (group.getAttribute("aria-busy") !== String(active)) group.setAttribute("aria-busy", String(active));
  const label = `${presentation.text}，${presentation.description}`;
  if (state.summary.getAttribute("aria-label") !== label) {
    state.summary.setAttribute("aria-label", label);
    state.summary.title = label;
  }
  if (state.icon !== presentation.icon) {
    state.visual.replaceChildren(icon(presentation.icon));
    state.icon = presentation.icon;
  }
  const titleClass = `command-group-title${active ? " working-label" : ""}${presentation.thinking ? " markdown-inline" : ""}`;
  if (state.title.className !== titleClass) state.title.className = titleClass;
  if (presentation.thinking) setMarkdown(state.title, presentation.text, [], { inline: true });
  else {
    state.title.markdownSource = null;
    state.title.markdownHtml = null;
    if (state.title.textContent !== presentation.text) state.title.textContent = presentation.text;
  }
  if (group.open) state.renderBody();
  else state.clearBody();
  return group;
}
export function renderActivity(turn, work, running, collapsible, renderMessage, showSummary = true, historyLoad = null) {
  const key = `${turn.id}:${work[0]?.id || "pending-thinking"}`;
  let activity = renderedActivities.get(key);
  if (!activity && work.length) {
    const pendingKey = `${turn.id}:pending-thinking`;
    activity = renderedActivities.get(pendingKey);
    if (activity) renderedActivities.delete(pendingKey);
  }
  if (!activity) {
    const previous = work.map(item => renderedActivities.get(`${turn.id}:${item.id}`)).find(Boolean);
    const node = element("details", "activity-group");
    const summary = element("summary", "activity-summary"), state = element("span"), label = element("span");
    const body = element("div", "activity-body");
    summary.append(state, label, icon("chevron")); node.append(summary, body);
    activity = { node, summary, state, label, body, manualOpen: previous?.manualOpen ?? null,
      commandGroups: previous?.commandGroups || new Map(), imageGroups: previous?.imageGroups || new Map() };
    summary.onclick = event => {
      if (!activity.collapsible) { event.preventDefault(); return; }
      activity.manualOpen = !node.open;
    };
    node.ontoggle = () => {
      if (!node.isConnected) return;
      scheduleMessages();
      if (node.open && activity.historyLoad) scheduleHistoryLoad(true);
    };
  }
  renderedActivities.set(key, activity);
  const { node, summary, state, label, body } = activity;
  if (activity.historyLoad !== historyLoad) activity.historyLoad?.remove();
  activity.historyLoad = historyLoad;
  node.dataset.activityKey = key;
  activity.turn = turn; activity.running = running; activity.workCount = work.length; activity.showSummary = showSummary;
  collapsible &&= showSummary;
  activity.collapsible = collapsible;
  node.classList.toggle("collapsible", collapsible);
  node.classList.toggle("activity-continuation", !showSummary);
  const open = collapsible ? activity.manualOpen ?? false : true;
  if (node.open !== open) node.open = open;
  summary.className = `activity-summary${collapsible ? "" : " uncollapsible"}`;
  summary.hidden = !showSummary;
  summary.removeAttribute("role");
  state.className = "activity-complete";
  label.className = "";
  updateActivitySummary(activity);
  if (node.open || !running && [...activity.commandGroups.values()].some(group => group.classList.contains("running"))) {
    const children = [], commandKeys = new Set(), imageKeys = new Set();
    let pendingHistoryLoad = historyLoad;
    let previousHistoryGroup = activity.commandGroups.get("history");
    const activeFlags = thread?.status?.status?.activeFlags || thread?.status?.activeFlags || [];
    const working = running && !compactionLocked() && !thread?.requests?.length
      && !activeFlags.some(flag => ["waitingOnApproval", "waitingOnUserInput"].includes(flag));
    if (!work.length && working) {
      const thinking = body.querySelector(":scope > .thinking-status") || element("div", "thinking-status working-label", "正在思考");
      thinking.setAttribute("role", "status");
      children.push(thinking);
    }
    const visibleWork = work.filter(item => !["agentMessage", "plan"].includes(item.type) || textContent(item).trim());
    const isCommandRecord = item => item.type === "commandExecution" ? !skillReadInfo(item)
      : ["fileChange", "mcpToolCall", "dynamicToolCall", "webSearch", "reasoning"].includes(item.type);
    for (let index = 0; index < visibleWork.length;) {
      if (visibleWork[index].type === "imageView") {
        const records = [];
        while (index < visibleWork.length && visibleWork[index].type === "imageView") records.push(visibleWork[index++]);
        const imageKey = records[0].id;
        const group = renderImageViewGroup(records, activity.imageGroups.get(imageKey));
        activity.imageGroups.set(imageKey, group); imageKeys.add(imageKey);
        children.push(group); continue;
      }
      if (!isCommandRecord(visibleWork[index])) {
        children.push(renderMessage(visibleWork[index++], true)); continue;
      }
      const records = [];
      while (index < visibleWork.length && isCommandRecord(visibleWork[index])) records.push(visibleWork[index++]);
      const commandKey = records[0].id;
      const active = working && (records.some(item => ["inProgress", "running"].includes(item.status))
        || records.at(-1) === turn.items?.at(-1));
      const previous = activity.commandGroups.get(commandKey) || records.map(item => activity.commandGroups.get(item.id)).find(Boolean)
        || previousHistoryGroup;
      previousHistoryGroup = null;
      const group = renderCommandGroup(records, previous, renderMessage, active, pendingHistoryLoad);
      pendingHistoryLoad = null;
      activity.commandGroups.set(commandKey, group); commandKeys.add(commandKey);
      children.push(group);
    }
    if (pendingHistoryLoad) {
      const group = renderCommandGroup([], activity.commandGroups.get("history"), renderMessage, false, pendingHistoryLoad);
      activity.commandGroups.set("history", group); commandKeys.add("history");
      children.unshift(group);
    }
    reconcileChildren(body, children);
    for (const key of activity.commandGroups.keys()) if (!commandKeys.has(key)) activity.commandGroups.delete(key);
    for (const key of activity.imageGroups.keys()) if (!imageKeys.has(key)) activity.imageGroups.delete(key);
  }
  return node;
}
