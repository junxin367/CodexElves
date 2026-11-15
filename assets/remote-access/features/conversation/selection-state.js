// 会话选择状态：读取版本与失败标记、关联会话返回、自动新建抑制，以及地址栏同步与欢迎页节点。

import { element } from "../../ui/dom.js";
import { selectedId, sidebar } from "../../core/state.js";
import { isClaude, normalizeRelatedView } from "../session/capabilities.js";
import {
  conversationHistoryKey, conversationHistoryPosition, relatedExitTransition, relatedHistoryKey,
  relatedSessionView
} from "../session/related-state.js";

export let selectionController = null;
export function setSelectionController(value) { return selectionController = value; }
export let threadReadFailed = false;
export function setThreadReadFailed(value) { return threadReadFailed = value; }
export let suppressAutomaticNewChat = false;
export function setSuppressAutomaticNewChat(value) { return suppressAutomaticNewChat = value; }
export let threadReadVersion = 0;
export function setThreadReadVersion(value) { return threadReadVersion = value; }
export let relatedReturnTranscript = null;
export function setRelatedReturnTranscript(value) { return relatedReturnTranscript = value; }
export let relatedSelectionVersion = 0;
export function setRelatedSelectionVersion(value) { return relatedSelectionVersion = value; }
export function selectedThreadProjectId() {
  if (!selectedId || !sidebar) return "";
  const projects = sidebar.projects || [];
  const summary = [...(sidebar.recent || []), ...(sidebar.pinnedThreads || [])]
    .find(item => item.id === selectedId);
  const projectId = summary?.projectId !== undefined ? summary.projectId
    : projects.find(project => project.threads?.some(item => item.id === selectedId))?.id;
  return projects.some(project => project.id === projectId) ? projectId : "";
}
export function syncThreadHash(id, { replace = false } = {}) {
  if (relatedExitTransition && id === selectedId) return;
  const hash = new URLSearchParams(location.hash.slice(1));
  const previousId = hash.get("thread");
  hash.set("thread", id);
  const state = { ...(history.state || {}) };
  state[conversationHistoryKey] = conversationHistoryPosition(state) + (previousId !== id && !replace ? 1 : 0);
  if (relatedSessionView?.threadId === id) state[relatedHistoryKey] = normalizeRelatedView(relatedSessionView, id);
  else delete state[relatedHistoryKey];
  // 只在新会话条目中清除旧工具路由，保留此前变更页的历史条目。
  if (previousId !== id) delete state.codexElvesTools;
  const url = `${location.pathname}${location.search}#${hash}`;
  if (!replace && previousId !== id) history.pushState(state, "", url);
  else history.replaceState(state, "", url);
}
export function syncNewChatHash() {
  const hash = new URLSearchParams(location.hash.slice(1));
  if (!hash.has("thread")) return;
  hash.delete("thread");
  const state = { ...(history.state || {}) };
  state[conversationHistoryKey] = conversationHistoryPosition(state) + 1;
  delete state[relatedHistoryKey]; delete state.codexElvesTools;
  // 新建会话是独立历史条目，工具页返回时不能再命中上一条会话。
  history.pushState(state, "", `${location.pathname}${location.search}${hash.size ? `#${hash}` : ""}`);
}
export function welcomeNode() {
  const node = element("div", "welcome");
  // 用户要求说明所选 Claude 实例的职责，不引用默认 WSL 名称，避免与当前选择的实例不一致。
  node.append(element("h1", "", isClaude() ? "开始 Claude 会话" : "继续本机的工作"),
    element("p", "", isClaude() ? "选择项目，输入消息开始工作。" : "选择左侧会话，发送会话。"),
    element("small", "", isClaude()
      ? "消息、工具和上下文由所选 Claude 实例处理。"
      : "消息、审批和执行状态与 Codex 桌面共享。"));
  return node;
}
