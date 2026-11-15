// 子代理条目：识别、状态与图标渲染。

import { element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { selectedId } from "../../core/state.js";
import { subagentsEnabled, validRelatedThreadId } from "../session/capabilities.js";
import { relatedSessionView } from "../session/related-state.js";
import { openRelatedThread } from "../session/related.js";

export function isSubagentItem(item) {
  return ["sub_agent_activity", "collab_agent_tool_call", "multi_agent_action"].includes(subagentToken(item.type));
}
function isSubagentMessage(item) {
  return subagentToken(item.type) === "sub_agent_activity"
    && (item.isMessage === true || subagentToken(item.kind) === "interacted");
}
function subagentToken(value) {
  return String(value || "").replace(/([a-z\d])([A-Z])/g, "$1_$2").replace(/[\s-]+/g, "_").toLowerCase();
}
export function subagentTool(item) {
  return subagentToken(item.action || item.tool);
}
function subagentThreadIds(item) {
  const ids = [
    item.agentThreadId, item.agent_thread_id,
    ...item.receiverThreadIds || item.receiver_thread_ids || [],
    ...(item.receiverThreads || []).map(receiver => receiver?.threadId),
    ...Object.keys(item.agentsStates || item.agents_states || {})
  ];
  return [...new Set(ids.map(id => String(id || "").trim()).filter(Boolean))];
}
function subagentStates(item) {
  return Object.values(item.agentsStates || item.agents_states || {}).map(state =>
    subagentToken(typeof state === "string" ? state : state?.status));
}
export function subagentName(item) {
  if (typeof item.displayName === "string" && item.displayName.trim()) return item.displayName.trim();
  const name = String(item.agentPath || item.agent_path || "").split("/").map(part => part.trim())
    .filter(part => part && part !== "root").at(-1)?.replace(/[_-]+/g, " ").replace(/\s+/g, " ").trim();
  if (name) return name[0].toLocaleUpperCase() + name.slice(1).toLocaleLowerCase();
  const receiver = (item.receiverThreads || []).map(value => value?.thread).find(Boolean);
  const receiverName = receiver?.agentNickname || receiver?.title
    || (item.receiverAgents || item.receiver_agents || []).map(value =>
      typeof value === "string" ? value : value?.agentNickname || value?.displayName || value?.name).find(Boolean);
  if (receiverName) return receiverName;
  const count = subagentThreadIds(item).length;
  return {
    spawn_agent: "启动子代理", send_input: "发送子代理消息", send_message: "发送子代理消息",
    followup_task: "发送子代理任务", resume_agent: "恢复子代理",
    wait: count > 1 ? `等待 ${count} 个子代理` : "等待子代理",
    wait_agent: count > 1 ? `等待 ${count} 个子代理` : "等待子代理",
    close_agent: "关闭子代理", interrupt_agent: "中断子代理", list_agents: "子代理状态"
  }[subagentTool(item)] || "子代理";
}
function subagentPresentation(item, running = false) {
  const kind = subagentToken(item.displayStatus || item.kind);
  const activity = {
    active: ["active", "开始工作"], started: ["active", "开始工作"], running: ["active", "执行中"],
    updated: ["updated", "已更新"], interacted: ["updated", "已更新"],
    interrupted: ["interrupted", "已中断"], completed: ["completed", "已完成"],
    closed: ["closed", "已关闭"], failed: ["failed", "执行失败"], errored: ["failed", "执行失败"],
    error: ["failed", "执行失败"], not_found: ["failed", "执行失败"]
  }[kind];
  if (activity) return { state: activity[0], label: activity[1], active: activity[0] === "active", failed: activity[0] === "failed" };
  const tool = subagentTool(item), status = subagentToken(item.status), states = subagentStates(item);
  const has = (...values) => states.some(value => values.includes(value));
  const callActive = running || ["in_progress", "running", "pending"].includes(status);
  if (["failed", "errored", "error", "not_found"].includes(status) || has("errored", "not_found", "failed", "error")) {
    return { state: "failed", label: tool === "wait" || tool === "wait_agent" ? "等待结束 · 有失败" : "执行失败", active: false, failed: true };
  }
  if (tool === "close_agent" && !callActive) return { state: "closed", label: "已关闭", active: false, failed: false };
  if (tool === "interrupt_agent" && !callActive) return { state: "interrupted", label: "已中断", active: false, failed: false };
  if (callActive) {
    return {
      state: "active",
      label: {
        spawn_agent: "正在启动", send_input: "正在发送", send_message: "正在发送",
        followup_task: "正在发送", resume_agent: "正在恢复",
        wait: "正在等待", wait_agent: "正在等待",
        close_agent: "正在关闭", interrupt_agent: "正在中断", list_agents: "正在刷新"
      }[tool] || "执行中",
      active: true,
      failed: false
    };
  }
  if (status === "interrupted") {
    return { state: "interrupted", label: "已中断", active: false, failed: false };
  }
  if (has("pending_init", "running")) {
    return { state: "active", label: tool === "wait" || tool === "wait_agent" ? "等待结束 · 仍在执行" : "已启动 · 执行中", active: true, failed: false };
  }
  if (has("interrupted")) return { state: "interrupted", label: "已中断", active: false, failed: false };
  if (has("shutdown")) return { state: "closed", label: "已关闭", active: false, failed: false };
  const completedStates = ["completed", "complete", "done", "success", "succeeded"];
  if (states.length && states.every(value => completedStates.includes(value))) {
    return { state: "completed", label: "已完成", active: false, failed: false };
  }
  return {
    state: "unknown",
    label: {
      spawn_agent: "已启动", send_input: "消息已发送", send_message: "消息已发送",
      followup_task: "任务已发送", resume_agent: "已恢复",
      wait: "等待结束", wait_agent: "等待结束", list_agents: "状态已刷新"
    }[tool] || "状态未知",
    active: false,
    failed: false
  };
}
function subagentIcon(name) {
  const shapes = [
    '<circle cx="10" cy="5" r="3"/><circle cx="14.33" cy="7.5" r="3"/><circle cx="14.33" cy="12.5" r="3"/><circle cx="10" cy="15" r="3"/><circle cx="5.67" cy="12.5" r="3"/><circle cx="5.67" cy="7.5" r="3"/><circle cx="10" cy="10" r="3"/>',
    '<path d="M10 10C3 10 1 5 5 3 9 1 10 6 10 10ZM10 10C10 3 15 1 17 5 19 9 14 10 10 10ZM10 10C17 10 19 15 15 17 11 19 10 14 10 10ZM10 10C10 17 5 19 3 15 1 11 6 10 10 10Z"/>',
    '<path d="M10 1.5 12.5 7.5 18.5 10 12.5 12.5 10 18.5 7.5 12.5 1.5 10 7.5 7.5Z"/>',
    '<path d="M10 2v16M2 10h16M4.35 4.35l11.3 11.3M4.35 15.65l11.3-11.3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>',
    '<path d="M10 2 18 10 10 18 2 10Z"/>',
    Array.from({ length: 16 }, (_, index) =>
      `<path d="M10 2v5.5" transform="rotate(${index * 22.5} 10 10)"/>`).join("")
  ];
  let hash = 2166136261;
  for (const character of name) {
    hash ^= character.codePointAt(0);
    hash = Math.imul(hash, 16777619);
  }
  const variant = (hash >>> 0) % shapes.length;
  return { shape: shapes[variant], variant };
}
export function createSubagentIcon(name) {
  const identity = subagentIcon(name);
  const icon = element("span", "subagent-icon");
  icon.dataset.variant = identity.variant;
  icon.setAttribute("aria-hidden", "true");
  icon.innerHTML = `<svg viewBox="0 0 20 20" fill="currentColor" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" aria-hidden="true" focusable="false">${identity.shape}</svg>`;
  return icon;
}
export function renderSubagentItem(item, previous, running = false) {
  const node = previous || element("article");
  const name = subagentName(item), presentation = subagentPresentation(item, running);
  node.className = `subagent-item${presentation.active ? " running" : ""}${presentation.failed ? " failed" : ""}`;
  node.dataset.status = presentation.state;
  const icon = createSubagentIcon(name);
  const state = element("span", "subagent-state");
  state.textContent = presentation.label;
  node.replaceChildren(icon, element("strong", "subagent-title", name), state);
  node.setAttribute("aria-label", `${name}，${presentation.label}`);
  if (presentation.active) node.setAttribute("role", "status"); else node.removeAttribute("role");
  const ids = subagentThreadIds(item).filter(id => validRelatedThreadId(id) && id !== selectedId);
  const disabled = !subagentsEnabled();
  node.classList.toggle("subagent-item-disabled", disabled);
  if (disabled) node.setAttribute("aria-disabled", "true"); else node.removeAttribute("aria-disabled");
  const canOpen = !disabled && !relatedSessionView && ids.length === 1, parentThreadId = selectedId;
  node.onclick = canOpen ? () => void openRelatedThread({ id: ids[0], name }, "subagent", parentThreadId)
    .catch(error => notice(error.message, true)) : null;
  node.onkeydown = canOpen ? event => {
    if (!["Enter", " "].includes(event.key)) return;
    event.preventDefault(); node.click();
  } : null;
  if (canOpen) {
    node.tabIndex = 0; node.setAttribute("role", "button"); node.title = `查看 ${name} 的会话`;
  } else {
    node.removeAttribute("tabindex"); node.removeAttribute("title");
  }
  return node;
}
