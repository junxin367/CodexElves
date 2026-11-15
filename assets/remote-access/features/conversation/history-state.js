// 历史与正文视图的共享状态：历史读取进度、游标与失败标记，正文滚动、跟随、已渲染条目与分页加载按钮。

import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { selectedId, selectionVersion, turns } from "../../core/state.js";
import { isActiveThread } from "../session/capabilities.js";

export let historyLoadFrame = null, historyScrollTop = 0, historyLoadVisibleItems = false;
export function setHistoryLoadFrame(value) { return historyLoadFrame = value; }
export function setHistoryLoadVisibleItems(value) { return historyLoadVisibleItems = value; }
export function setHistoryScrollTop(value) { return historyScrollTop = value; }
export let recentHistoryPage = null;
export function setRecentHistoryPage(value) { return recentHistoryPage = value; }
export let messageFrame = null;
export function setMessageFrame(value) { return messageFrame = value; }
export let followMessages = true;
export function setFollowMessages(value) { return followMessages = value; }
export const renderedItems = new Map(), renderedTurns = new Map();
export function updateScrollControl() {
  const messages = $("#messages"), control = $("#scroll-to-bottom");
  if (!messages || !control) return;
  const away = messages.scrollHeight - messages.scrollTop - messages.clientHeight >= 100;
  control.hidden = !selectedId || !away;
  const running = isActiveThread();
  const state = running ? "working" : "latest";
  if (control.dataset.state !== state) {
    control.dataset.state = state;
    if (running) {
      const dots = element("span", "scroll-working-dots");
      dots.setAttribute("aria-hidden", "true");
      dots.append(element("span"), element("span"), element("span"));
      control.replaceChildren(dots);
    } else control.replaceChildren(icon("arrowUp"));
  }
  control.setAttribute("aria-label", running ? "会话正在执行，跳转到最新消息" : "跳转到最新消息");
  control.title = running ? "会话正在执行，回到最新消息" : "回到最新消息";
}
export function syncHistoryButtons() {
  const loading = historyLoading && historyLoadingVersion === selectionVersion;
  for (const node of document.querySelectorAll("#messages .history-load")) {
    node.disabled = loading;
    const requested = historyLoadingTurnId ? node.dataset.turnId === historyLoadingTurnId
      : historyLoadingOlder ? node.dataset.historyPage === "older" && !node.dataset.turnId : !node.dataset.historyPage;
    node.textContent = loading && requested
      ? "加载中…" : node.dataset.label;
  }
}
export function historyLoadButton(label, action, turnId = "") {
  const node = button(label, label, action, "load-more history-load");
  node.dataset.label = label; node.dataset.turnId = turnId;
  return node;
}
export function visibleMessageAnchor(root) {
  const box = root.getBoundingClientRect();
  const candidates = [...root.querySelectorAll("[data-key]"), ...root.querySelectorAll("[data-activity-key]"), ...root.querySelectorAll(".turn")];
  for (const node of candidates) {
    const rect = node.getBoundingClientRect();
    if (rect.height && rect.bottom > box.top && rect.top < box.bottom)
      return { node, key: node.dataset.key, offset: rect.top - box.top };
  }
  return null;
}
// 用户要求 Claude 和 Codex 都保留 TODO 统计标题；仅统计原生 turn.plan 的状态，随计划更新，不从正文推测任务。
export function renderTurnPlanTitle(plan) {
  const count = status => plan.filter(step => step.status === status).length;
  return element("div", "turn-plan-title",
    `${plan.length} 个任务（${count("completed")} 个已完成，${count("inProgress")} 个进行中，${count("pending")} 个待办）`);
}
export function legacySentDisplayTurn(turn) {
  // 旧版本曾把补插出的已接替提问写成只有 id/status/items 的完成轮次，用户升级后旧缓存仍会把旧消息插到执行中轮次前后。
  // 真实轮次带有原生或分页字段；仍未见原生正文的成功投影带 remotePendingInput，均不在此剔除范围内。
  return turn?.status === "completed" && Object.keys(turn).every(key => ["id", "status", "items"].includes(key))
    && Array.isArray(turn.items) && turn.items.length > 0
    && turn.items.every(item => item?.type === "userMessage" && item.remoteConfirmedInput === true && !item.remotePendingInput);
}

export let historyCursor = null, historyLoading = false;
export function setHistoryCursor(value) { return historyCursor = value; }
export function setHistoryLoading(value) { return historyLoading = value; }
export let historyLoadingVersion = -1;
export function setHistoryLoadingVersion(value) { return historyLoadingVersion = value; }
export let historyInitialized = false;
export function setHistoryInitialized(value) { return historyInitialized = value; }
export let historyError = "", historyController = null, historyLoadingTurnId = null;
export function setHistoryError(value) { return historyError = value; }
export function setHistoryController(value) { return historyController = value; }
export function setHistoryLoadingTurnId(value) { return historyLoadingTurnId = value; }
export let latestHistoryReadFailed = false;
export function setLatestHistoryReadFailed(value) { return latestHistoryReadFailed = value; }
export let historyLoadingOlder = false;
export function setHistoryLoadingOlder(value) { return historyLoadingOlder = value; }
export let historyAutoFailed = false;
export function setHistoryAutoFailed(value) { return historyAutoFailed = value; }
export let transcriptChecking = false;
export function setTranscriptChecking(value) { return transcriptChecking = value; }
export let messageVersion = 0;
export function setMessageVersion(value) { return messageVersion = value; }
let claudeHistoryOrder = null;
export let threadRequest = null, historyRequest = null;
export function setThreadRequest(value) { return threadRequest = value; }
export function setHistoryRequest(value) { return historyRequest = value; }
export const itemVersions = new Map(), turnVersions = new Map();
export const turnFieldVersions = new Map();
export const partialItems = new Set();
export function orderClaudeHistoryTurns(ids, readVersion = messageVersion) {
  const sameSelection = claudeHistoryOrder?.threadId === selectedId
    && claudeHistoryOrder.selectionVersion === selectionVersion;
  // 用户要求长历史与实时消息保持原生顺序；较早 HTTP 回包不能撤回新通知给出的顺序。
  if (Array.isArray(ids) && (!sameSelection || readVersion >= claudeHistoryOrder.version)) {
    claudeHistoryOrder = { threadId: selectedId, selectionVersion, version: readVersion, ids: [...ids] };
  } else if (!sameSelection) return ids;
  const order = claudeHistoryOrder.ids, known = new Set(order);
  const entries = order.filter(id => turns.has(id)).map(id => [id, turns.get(id)]);
  // 原生列表尚未包含的实时输入或本地预览继续保留，不能把补历史等同删除。
  entries.push(...[...turns].filter(([id]) => !known.has(id)));
  turns.clear();
  for (const [id, turn] of entries) turns.set(id, turn);
  return order;
}
export function claudeHistoryCursor(ids) {
  if (!Array.isArray(ids)) return undefined;
  if (!ids.length) return null;
  if (!ids.some(id => turns.has(id))) return undefined;
  // 从最新轮次向前寻找连续已加载区间；零散旧缓存不能让中间未读取的轮次被跳过。
  let first = ids.length;
  while (first > 0 && turns.has(ids[first - 1])) --first;
  return first === 0 ? null : String(first);
}
export function mergeLiveText(snapshot, live, partial = false, newer = true) {
  snapshot = String(snapshot || ""); live = String(live || "");
  if (!live || snapshot.startsWith(live) || (partial && snapshot.includes(live))) return snapshot;
  if (!snapshot || live.startsWith(snapshot)) return live;
  if (!newer) return snapshot;
  if (!partial) return live;
  const prefix = new Uint32Array(live.length);
  for (let index = 1, matched = 0; index < live.length; index++) {
    while (matched && live[index] !== live[matched]) matched = prefix[matched - 1];
    if (live[index] === live[matched]) matched++;
    prefix[index] = matched;
  }
  let overlap = 0;
  for (const character of snapshot.slice(-live.length).split("")) {
    while (overlap && (overlap === live.length || character !== live[overlap])) overlap = prefix[overlap - 1];
    if (character === live[overlap]) overlap++;
  }
  // 此处仅处理读取开始后收到的未知开头增量；即使零重叠，也要保留历史前缀。
  return snapshot + live.slice(overlap);
}
export function normalizeDisplayedUserMessage(item) {
  if (!item || item.type !== "steeringUserMessage"
      || item.status !== "accepted" && typeof item.serverUserMessageId !== "string") return item;
  const id = typeof item.serverUserMessageId === "string" && item.serverUserMessageId
    ? item.serverUserMessageId : item.id;
  if (typeof id !== "string" || !id) return item;
  const content = Array.isArray(item.input) ? item.input
    : Array.isArray(item.content) ? item.content : [];
  return {
    ...item,
    type: "userMessage",
    id,
    ...(typeof (item.clientUserMessageId ?? item.clientId) === "string"
      ? { clientId: item.clientUserMessageId ?? item.clientId } : {}),
    content
  };
}
export function normalizeDisplayedTurnItems(items) {
  const byId = new Map();
  for (const source of items) {
    const item = normalizeDisplayedUserMessage(source);
    // 原生正文和 steered 确认共用消息 ID；分页、实时轮次与重进缓存必须一致保留一份正文。
    if (byId.get(item.id)?.type === "userMessage" && item.type === "steered") continue;
    byId.set(item.id, item);
  }
  return [...byId.values()];
}
export function completedItem(item) {
  // 普通消息的 completed 可能仅为原生展示推导，不能证明这份正文已经定稿。
  return item?.remoteItemCompleted === true || Number.isFinite(item?.completedAtMs)
    || item?.type === "contextCompaction" && item.completed === true
    || ["completed", "failed", "interrupted"].includes(item?.status);
}
