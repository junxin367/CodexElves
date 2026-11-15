// 执行活动渲染：活动区与命令分组；最终消息定位、活动摘要与计时在 activity-state.js。

import { element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { scheduleMessages } from "../../core/actions.js";
import { thread } from "../../core/state.js";
import { compactionLocked } from "../conversation/compaction.js";
import { scheduleHistoryLoad } from "../conversation/history.js";
import { renderedActivities, updateActivitySummary, workGroupSummary } from "./activity-state.js";
import { reconcileChildren } from "./item-presentation.js";
import { renderImageViewGroup, setMarkdown } from "./media.js";
import { skillReadInfo } from "./tool-info.js";
import { renderToolOutput } from "./tool-output.js";
import { textContent } from "./user-content.js";

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
