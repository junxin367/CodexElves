// 会话正文视图：历史加载占位、消息列表渲染与正文缓存的保存/恢复。

import { renderMessageActions } from "../../message-actions.js";
import { copyText } from "../../ui/clipboard.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import {
  busy, connected, creating, pageLeaving, selectedId, selectionVersion, session, setTurns, sidebar,
  stopping, thread, turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { clearPendingFeedbackTimer } from "../composer/composer.js";
import { selectedEffort, selectedModel, setSelectedEffort, setSelectedModel } from "../composer/models.js";
import { resizeEditorInput } from "../composer/prompt-editor.js";
import { skillRenderVersion } from "../composer/skills.js";
import { compactionLocked } from "./compaction.js";
import { setNotifiedStatus } from "./events.js";
import {
  historyAutoFailed, historyController, historyCursor, historyError, historyInitialized,
  historyLoading, historyLoadingOlder, historyLoadingTurnId, historyLoadingVersion, itemVersions,
  loadHistory, partialItems, setHistoryAutoFailed, setHistoryController, setHistoryCursor,
  setHistoryError, setHistoryInitialized, setHistoryLoading, setHistoryLoadingOlder,
  setHistoryLoadingTurnId, setHistoryLoadingVersion, setHistoryRequest, setLatestHistoryReadFailed,
  setTranscriptChecking, transcriptChecking, turnFieldVersions, turnVersions
} from "./history.js";
import { pending } from "./pending.js";
import {
  relatedReturnTranscript, selectionController, setRelatedReturnTranscript, setSelectionController,
  setThreadReadFailed, welcomeNode
} from "./selection.js";
import {
  completedManualCompactionAfterReply, replaceTranscriptEvents, transcriptEvents,
  transcriptEventTypes
} from "./transcript-events.js";
import {
  activityPresentation, claudeTurnAwaitingStart, finalMessageIndex, renderActivity,
  renderedActivities, stopActivityTimer, syncActivityTimer
} from "../messages/activity.js";
import { renderEditedFiles } from "../messages/edited-files.js";
import { reasoningPreviewText, reconcileChildren, renderItem } from "../messages/items.js";
import { markdownRenderVersion, messageImageRefreshes, scheduleMermaidBlocks } from "../messages/media.js";
import { beginMessageEdit, editingMessage } from "../messages/message-edit.js";
import { isSubagentItem, subagentTool } from "../messages/subagents.js";
import {
  loadMessageSkillChoices, messageCopyText, textContent, userMessageHasSkills
} from "../messages/user-content.js";
import {
  finishQueueDrag, queueVersion, renderQueueArea, retiredQueueEpochs, setNotifiedQueue,
  setNotifiedQueueEdits, setQueueSnapshotVersion, setQueueVersion
} from "../queue/queue.js";
import { editingQueuedMessage } from "../queue/queued-edit.js";
import { reconcileSentMessageDisplays, withSentMessageDisplays } from "../queue/sent-display.js";
import {
  isActiveThread, normalizeRelatedView, privateInputScope, subagentsEnabled, threadReadOnly,
  threadSentMessageEditAllowed
} from "../session/capabilities.js";
import {
  relatedSessionView, relatedThreadViews, setRelatedSessionView, syncRelatedSessionLayout
} from "../session/related.js";
import {
  cacheDisplayData, currentThreadDisplay, readingPosition, readScope, rememberReadingPosition,
  rememberThreadDisplay, threadEphemeralState
} from "../session/thread-cache.js";
import { composerScroll } from "../shell.js";
import { threadDeleted } from "../sidebar/actions.js";

export let historyLoadFrame = null, historyScrollTop = 0, historyLoadVisibleItems = false;
export function setHistoryScrollTop(value) { return historyScrollTop = value; }
export let recentHistoryPage = null;
export function setRecentHistoryPage(value) { return recentHistoryPage = value; }
let messageFrame = null;
export let followMessages = true;
export function setFollowMessages(value) { return followMessages = value; }
export const renderedItems = new Map(), renderedTurns = new Map();
export function resetMessages() {
  for (const request of messageImageRefreshes.values()) request.controller.abort();
  messageImageRefreshes.clear();
  clearPendingFeedbackTimer();
  composerScroll?.expand();
  stopActivityTimer();
  finishQueueDrag({ render: false });
  historyController?.abort(); setHistoryController(null);
  setHistoryRequest(null);
  selectionController?.abort(); setSelectionController(null);
  setHistoryLoading(false); setHistoryLoadingVersion(-1); setHistoryLoadingTurnId(null); setHistoryLoadingOlder(false); setHistoryError("");
  setThreadReadFailed(false); setLatestHistoryReadFailed(false);
  cancelAnimationFrame(historyLoadFrame); historyLoadFrame = null; setHistoryAutoFailed(false);
  historyScrollTop = 0; historyLoadVisibleItems = false;
  cancelAnimationFrame(messageFrame); messageFrame = null;
  for (const entry of renderedItems.values()) entry.node.messageActionBinding?.dispose();
  renderedItems.clear(); renderedTurns.clear(); renderedActivities.clear(); itemVersions.clear(); turnVersions.clear(); turnFieldVersions.clear();
  partialItems.clear();
  followMessages = true;
  setTranscriptChecking(false);
  recentHistoryPage = null;
  setNotifiedStatus(null);
  setNotifiedQueue(null); setNotifiedQueueEdits(null); setQueueSnapshotVersion(null); retiredQueueEpochs.clear(); setQueueVersion(queueVersion + 1);
}
export function scheduleMessages() {
  if (messageFrame != null) return;
  messageFrame = requestAnimationFrame(() => { messageFrame = null; renderMessages(); });
}
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
export function historyPlaceholder() {
  const node = element("div", "empty-conversation");
  if (historyError) {
    node.append(element("p", "error-text", historyError),
      button("重试", "重试加载最新消息", () => void loadHistory(), "text-button"));
  } else {
    node.setAttribute("role", "status");
    node.append(element("span", "spinner"), document.createTextNode(" 正在加载最新消息…"));
  }
  return node;
}
export function scheduleHistoryLoad(visibleItems = false) {
  historyLoadVisibleItems ||= visibleItems;
  if (historyLoadFrame != null) return;
  historyLoadFrame = requestAnimationFrame(() => {
    historyLoadFrame = null;
    const includeVisibleItems = historyLoadVisibleItems;
    historyLoadVisibleItems = false;
    const root = $("#messages");
    if (!root || !selectedId || !historyInitialized || transcriptChecking || historyLoading || historyAutoFailed || pageLeaving) return;
    const box = root.getBoundingClientRect();
    for (const node of root.querySelectorAll(".history-load[data-history-page]")) {
      if (node.dataset.turnId && node.closest("details:not([open])")) continue;
      const rect = node.getBoundingClientRect();
      if (!node.disabled && rect.height && rect.bottom >= box.top - 120
          && rect.top <= (includeVisibleItems && node.dataset.turnId ? box.bottom : Math.min(box.bottom, box.top + 120))) {
        const version = selectionVersion;
        void loadHistory(true, version, node.dataset.turnId || null).then(() => {
          if (version === selectionVersion) scheduleHistoryLoad(includeVisibleItems);
        });
        break;
      }
    }
  });
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
function historyLoadButton(label, action, turnId = "") {
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
function historyGapIndex(turn) {
  const items = turn.items || [];
  let index = items.findIndex(item => item.id === turn.itemsCursorAnchor);
  if (index < 0) index = items.findIndex(item => !["userMessage", "modelChanged", "modelRerouted"].includes(item.type));
  if (index < 0) index = items.length;
  while (index > 0 && transcriptEventTypes.has(items[index - 1].type) && !items[index - 1].remoteHistoryContext) index--;
  return index;
}
// 用户要求 Claude 和 Codex 都保留 TODO 统计标题；仅统计原生 turn.plan 的状态，随计划更新，不从正文推测任务。
function renderTurnPlanTitle(plan) {
  const count = status => plan.filter(step => step.status === status).length;
  return element("div", "turn-plan-title",
    `${plan.length} 个任务（${count("completed")} 个已完成，${count("inProgress")} 个进行中，${count("pending")} 个待办）`);
}
export function renderMessages(keepScroll = true) {
  cancelAnimationFrame(messageFrame); messageFrame = null;
  const root = $("#messages");
  if (!root) return;
  scheduleMermaidBlocks();
  const bottom = followMessages && !composerScroll?.collapsed;
  const oldHeight = root.scrollHeight, oldTop = root.scrollTop;
  const anchor = !keepScroll || !bottom ? visibleMessageAnchor(root) : null;
  const children = [], itemKeys = new Set(), turnKeys = new Set(), activityKeys = new Set();
  const relatedViewKind = relatedSessionView?.kind || "";
  if (reconcileSentMessageDisplays(selectedId, currentThreadDisplay()?.queuedMessages)) renderQueueArea();
  const sorted = withSentMessageDisplays();
  const lastRealTurn = [...sorted].reverse().find(turn => turns.has(turn.id));
  loadMessageSkillChoices(sorted.flatMap(turn => turn.items || []));
  if (historyError && sorted.length)
    children.push(historyLoadButton("最新记录加载失败，重试", () => void loadHistory()));
  if (historyCursor) {
    const load = root.querySelector(":scope > .history-load[data-history-page='older']")
      || historyLoadButton("加载更早的消息", () => void loadHistory(true));
    load.dataset.turnId = "";
    load.dataset.historyPage = "older";
    children.push(load);
  }
  for (const turn of sorted) {
    const group = renderedTurns.get(turn.id) || element("section", "turn"), items = [];
    group.dataset.turnId = turn.id; renderedTurns.set(turn.id, group); turnKeys.add(turn.id);
    let historyLoad = null;
    if (turn.itemsNextCursor) {
      historyLoad = group.querySelector(".history-load")
        || historyLoadButton("加载更早的过程记录", () => void loadHistory(true, selectionVersion, turn.id), turn.id);
      historyLoad.dataset.historyPage = "older";
    }
    const turnItems = turn.items || [], finalIndex = finalMessageIndex(turn),
      turnRunning = turn === lastRealTurn && turn.status === "inProgress"
        && (thread == null || isActiveThread());
    for (const item of turnItems) itemKeys.add(`${turn.id}:${item.id}`);
    const renderMessage = (item, inActivity = false) => {
      const key = `${turn.id}:${item.id}`, cached = renderedItems.get(key);
      const running = turnRunning && (["inProgress", "running"].includes(item.status) || item.type === "reasoning" && item === turnItems.at(-1));
      const editing = item.type === "userMessage" && editingMessage?.threadId === selectedId && editingMessage.messageId === item.id;
      const skillKey = userMessageHasSkills(item)
        ? JSON.stringify([skillRenderVersion, (item.content || []).filter(part => part.type === "skill")]) : "";
      const relatedEnabled = subagentsEnabled();
      const node = cached?.item === item && cached.inActivity === inActivity && cached.running === running
        && cached.editing === editing && cached.skillKey === skillKey && cached.markdownVersion === markdownRenderVersion
        && cached.relatedViewKind === relatedViewKind && cached.relatedEnabled === relatedEnabled
        ? cached.node : renderItem(item, cached?.item.type === item.type ? cached.node : null, inActivity, running);
      node.dataset.key = key;
      renderedItems.set(key, { item, node, inActivity, running, editing, skillKey, relatedViewKind, relatedEnabled, markdownVersion: markdownRenderVersion });
      if (editing) node.messageActionBinding?.dispose();
      else if (item.type === "userMessage" || item.type === "agentMessage") {
        const threadId = selectedId;
        renderMessageActions(node, item, {
          threadId, inActivity, readOnly: threadReadOnly(threadId) || !threadSentMessageEditAllowed(threadId),
          turnInProgress: isActiveThread(),
          editSupported: !item.remoteDirectSend && thread?.id === selectedId && connected && !busy && !stopping && !pending
            && !editingMessage && !editingQueuedMessage && !compactionLocked()
            && threadSentMessageEditAllowed(threadId),
          completed: turn.status !== "inProgress" || item.status === "completed"
        }, {
          icon,
          copy: message => {
            const text = messageCopyText(message);
            if (text) void copyText(text); else notice("这条消息没有可复制的文字");
          },
          edit: message => beginMessageEdit(threadId, message)
        });
      }
      return node;
    };
    let work = [];
    const content = [];
    // 用户要求恢复后单独压缩仍位于旧回复之后；最后回复独立展示时，只暂存其后的时间线事件，保留此前事件和活动折叠位置。
    const afterFinalEvents = [];
    const trailingManualCompactions = [];
    const historyGap = historyGapIndex(turn);
    const flushActivity = () => {
      if (!work.length) return;
      const collapsible = finalIndex >= 0 && turn.status !== "interrupted" && turnItems[finalIndex].phase !== "commentary"
        && (turn.status !== "inProgress" || turnItems[finalIndex].phase === "final_answer");
      content.push({ work, collapsible });
      work = [];
    };
    for (let index = 0; index < turnItems.length; index++) {
      if (historyLoad && index === historyGap) {
        flushActivity(); content.push({ historyLoad });
      }
      const item = turnItems[index];
      if (index === finalIndex) continue;
      if (finalIndex >= 0 && index < finalIndex
          && completedManualCompactionAfterReply(item, turnItems, index)) {
        trailingManualCompactions.push(item);
        continue;
      }
      if (finalIndex >= 0 && index > finalIndex && transcriptEventTypes.has(item.type)) {
        (completedManualCompactionAfterReply(item, turnItems, index)
          ? trailingManualCompactions : afterFinalEvents).push(item);
        continue;
      }
      if (isSubagentItem(item) && ["wait", "wait_agent"].includes(subagentTool(item))) continue;
      if (item.type === "reasoning" && !reasoningPreviewText(item) && !(turnRunning && item === turnItems.at(-1))) continue;
      if (["agentMessage", "plan"].includes(item.type) && !textContent(item).trim()) continue;
      if (item.type === "userMessage" || transcriptEventTypes.has(item.type)) {
        flushActivity(); content.push({ item });
      }
      else work.push(item);
    }
    flushActivity();
    if (historyLoad && historyGap === turnItems.length) content.push({ historyLoad });
    const activeFlags = thread?.status?.status?.activeFlags || thread?.status?.activeFlags || [];
    // 轮次开始事件可能先于用户消息；空活动占位须等本轮用户消息可见后再出现。
    const awaitingClaude = turn === lastRealTurn && claudeTurnAwaitingStart(turn);
    if ((turnRunning || awaitingClaude) && finalIndex < 0 && !content.at(-1)?.work
        && turnItems.some(item => item.type === "userMessage")
        && !compactionLocked() && !thread?.requests?.length
        && !activeFlags.some(flag => ["waitingOnApproval", "waitingOnUserInput"].includes(flag)))
      content.push({ work: [], collapsible: false });
    if (historyLoad) {
      const index = content.findIndex(part => part.historyLoad);
      let activity = content.slice(index + 1).find(part => part.work);
      if (!activity) {
        activity = { work: [], collapsible: false };
        content.splice(index, 1, activity);
      } else content.splice(index, 1);
      activity.historyLoad = historyLoad;
    }
    const activities = content.filter(part => part.work);
    const live = activityPresentation(turn, turnRunning).live;
    const header = live ? activities[0] : activities.at(-1);
    for (const part of content) {
      if (part.item) { items.push(renderMessage(part.item)); continue; }
      const activity = renderActivity(turn, part.work, live, part.collapsible, renderMessage, part === header, part.historyLoad);
      activityKeys.add(activity.dataset.activityKey); items.push(activity);
    }
    if (finalIndex >= 0) {
      items.push(renderMessage(turnItems[finalIndex]));
      for (const item of afterFinalEvents) items.push(renderMessage(item));
    }
    if (finalIndex >= 0 && (turn.status !== "inProgress" || turnItems[finalIndex].phase === "final_answer")) {
      const editedFiles = renderEditedFiles(turn, group.querySelector(":scope > .edited-files-card"));
      if (editedFiles) items.push(editedFiles);
    }
    if (turn.error) items.push(element("div", "turn-error", turn.error.message || "此轮执行失败"));
    // 原生已结束但没有回复时保留已接收消息，结束等待提示；不把未知的结束原因说成用户主动停止。
    // 出字前停止由连接端按终端输入框确认为原生撤回（Rust _nativeWithdrawn）：原文回到原版终端输入框，不进入对话。
    if (turn._nativeEndedWithoutReply === true)
      items.push(element("small", "turn-end muted", turn._nativeWithdrawn === true
        ? "已撤回：Claude 在开始回复前停止，原文已放回原版终端输入框，不会进入对话"
        : "本轮已结束，未生成回复"));
    if (turn.status === "interrupted") {
      // 用户要求 Claude 与 Codex 的轮次中断统一显示“已停止”；原生原文仅保留在悬浮说明，工具状态独立展示。
      // 仅使用已确认的中断状态，不伪装成用户正文，也不据此推断用户按了哪个按钮。
      const native = turn.nativeInterruption?.source === "claude";
      const note = element("small", "turn-end muted", "已停止");
      if (native) {
        note.dataset.nativeInterruption = turn.nativeInterruption.id;
        note.title = `Claude 原生中断记录：${turn.nativeInterruption.text}`;
      }
      items.push(note);
    }
    if (turn.plan?.length) {
      const plan = element("ul", "turn-plan");
      for (const step of turn.plan) plan.append(element("li", step.status, `${step.status === "completed" ? "✓" : step.status === "inProgress" ? "◌" : "○"} ${step.step}`));
      items.push(renderTurnPlanTitle(turn.plan), plan);
    }
    // 用户要求手动压缩位于本轮所有收尾内容之后；“已编辑文件”和任务清单由这里追加，
    // 必须先渲染这些结果再放压缩提示，避免提示出现在文件卡片和任务统计之前。
    for (const item of trailingManualCompactions) items.push(renderMessage(item));
    reconcileChildren(group, items);
    if (items.length) children.push(group);
  }
  if (!sorted.length) children.push(creating ? welcomeNode()
    : selectedId && (!historyInitialized || historyError) ? historyPlaceholder()
    : element("div", "empty-conversation", "此会话暂无消息。"));
  reconcileChildren(root, children);
  resizeEditorInput(editingMessage?.input);
  for (const key of renderedItems.keys()) if (!itemKeys.has(key)) {
    renderedItems.get(key).node.messageActionBinding?.dispose(); renderedItems.delete(key);
  }
  for (const key of renderedTurns.keys()) if (!turnKeys.has(key)) renderedTurns.delete(key);
  for (const key of renderedActivities.keys()) if (!activityKeys.has(key)) renderedActivities.delete(key);
  syncActivityTimer();
  syncHistoryButtons();
  if (keepScroll && bottom) root.scrollTop = root.scrollHeight;
  else {
    const node = anchor?.key ? renderedItems.get(anchor.key)?.node : anchor?.node;
    if (node?.isConnected && node.getBoundingClientRect().height)
      root.scrollTop += node.getBoundingClientRect().top - root.getBoundingClientRect().top - anchor.offset;
    else if (!keepScroll) root.scrollTop = oldTop + root.scrollHeight - oldHeight;
  }
  historyScrollTop = root.scrollTop;
  updateScrollControl();
  scheduleHistoryLoad();
}
export function rememberTranscript({ retainView = false, retainRelatedParent = false } = {}) {
  rememberThreadDisplay();
  if (!session || !selectedId || !historyInitialized || threadDeleted(selectedId)) return;
  const ephemeral = threadEphemeralState(selectedId);
  const summary = [...(sidebar?.recent || []), ...(sidebar?.pinnedThreads || []),
    ...(sidebar?.projects || []).flatMap(project => project.threads || [])].find(item => item.id === selectedId);
  // 补插轮次只用于维持当前视图：缓存只保留已加载轮次和仍未见原生正文的成功投影，不把补出的原生副本写成历史轮次，
  // 避免用户切回会话或重启后，旧消息被当作真实历史插到执行中轮次前后。
  const currentTurns = withSentMessageDisplays().flatMap(turn => {
    if (turns.has(turn.id)) return [turn];
    const items = turn.items.filter(item => item.remotePendingInput);
    return items.length ? [{ ...turn, items }] : [];
  }), events = [...transcriptEvents.values()];
  const recentStart = currentTurns.findIndex(turn => turn.id === recentHistoryPage?.turns?.[0]?.id);
  // 最近页起点和游标来自服务器；合并该页之后收到的新轮次，避免回退窗口遗漏刚完成的回复。
  const recentTurns = recentStart >= 0 ? currentTurns.slice(recentStart) : null;
  const recentIds = new Set(recentTurns?.map(turn => turn.id));
  const recentWindow = recentTurns ? { ...recentHistoryPage, turns: recentTurns,
    transcriptEvents: events.filter(event => recentIds.has(event.turnId)) } : recentHistoryPage;
  const snapshot = {
    turns: currentTurns, transcriptEvents: events, historyCursor,
    ...(typeof ephemeral === "boolean" ? { ephemeral } : {}),
    ...readingPosition(), recentWindow, recentPage: recentHistoryPage,
    title: thread?.title || summary?.title || "",
    cwd: thread?.cwd || summary?.cwd || "",
    ...(recentHistoryPage?.sideHistoryVersion === 1 ? { sideHistoryVersion: 1 } : {}),
    ...(relatedSessionView ? { relatedView: normalizeRelatedView(relatedSessionView, selectedId) } : {})
  };
  // 仅离开会话时保留渲染节点，和正文共用容量限制；超限回退窗口时不复用整页视图。
  const view = retainView ? {
    turns: new Map(currentTurns.map(turn => [turn.id, turn])), items: new Map(renderedItems),
    groups: new Map(renderedTurns), activities: new Map(renderedActivities)
  } : null;
  // 子页仅保留一层返回位置，正文超出普通缓存窗口时也保留主会话原节点。
  if (retainRelatedParent && view) setRelatedReturnTranscript({
    scope: privateInputScope(session), threadId: selectedId, snapshot, view,
    selectedModel, selectedEffort
  });
  const retained = transcriptCache.put(session.device.id, selectedId, snapshot, view);
  if (retained) {
    const completed = retained.turns.filter(turn => ["completed", "failed", "interrupted"].includes(turn.status));
    const completedIds = new Set(completed.map(turn => turn.id));
    if (completed.length) cacheDisplayData("transcript", selectedId, {
      ...retained, turns: completed,
      transcriptEvents: (retained.transcriptEvents || []).filter(event => completedIds.has(event.turnId)),
      ...(completed.length === retained.turns.length ? {} : { followMessages: true, scrollTop: 0, anchor: null })
    });
  }
  rememberReadingPosition();
}
function legacySentDisplayTurn(turn) {
  // 旧版本曾把补插出的已接替提问写成只有 id/status/items 的完成轮次，用户升级后旧缓存仍会把旧消息插到执行中轮次前后。
  // 真实轮次带有原生或分页字段；仍未见原生正文的成功投影带 remotePendingInput，均不在此剔除范围内。
  return turn?.status === "completed" && Object.keys(turn).every(key => ["id", "status", "items"].includes(key))
    && Array.isArray(turn.items) && turn.items.length > 0
    && turn.items.every(item => item?.type === "userMessage" && item.remoteConfirmedInput === true && !item.remotePendingInput);
}
export function restoreTranscript(id) {
  const returning = relatedReturnTranscript?.scope === privateInputScope(session)
    && relatedReturnTranscript.threadId === id ? relatedReturnTranscript : null;
  const cached = returning?.snapshot || transcriptCache.get(session?.device.id, id);
  if (!cached) return null;
  const cachedView = normalizeRelatedView(cached.relatedView, id);
  if ((relatedSessionView?.kind === "side" || cachedView?.kind === "side") && cached.sideHistoryVersion !== 1) {
    transcriptCache.remove(session?.device.id, id);
    if (readScope) void readCache.remove(readScope, "transcript", id).catch(() => {});
    return null;
  }
  const view = returning?.view || transcriptCache.takeView(session.device.id, id);
  if (returning) {
    setSelectedModel(returning.selectedModel); setSelectedEffort(returning.selectedEffort);
    setRelatedReturnTranscript(null);
  }
  if (!relatedSessionView && cachedView) {
    setRelatedSessionView(cachedView);
    if (!relatedThreadViews.has(id)) relatedThreadViews.set(id, cachedView);
    syncRelatedSessionLayout();
  }
  setTurns(view?.turns || new Map(cached.turns.filter(turn => !legacySentDisplayTurn(turn)).map(turn => [turn.id, turn])));
  if (view) {
    for (const [key, value] of view.items) renderedItems.set(key, value);
    for (const [key, value] of view.groups) renderedTurns.set(key, value);
    for (const [key, value] of view.activities) renderedActivities.set(key, value);
  }
  replaceTranscriptEvents(cached.transcriptEvents);
  setHistoryCursor(cached.historyCursor); setHistoryInitialized(true);
  recentHistoryPage = cached;
  followMessages = cached.followMessages; setTranscriptChecking(true);
  renderMessages();
  const root = $("#messages");
  if (root) {
    root.scrollTop = cached.followMessages ? root.scrollHeight : cached.scrollTop || 0;
    const anchor = cached.anchor && renderedItems.get(cached.anchor.key)?.node;
    if (!cached.followMessages && anchor?.isConnected)
      root.scrollTop += anchor.getBoundingClientRect().top - root.getBoundingClientRect().top - cached.anchor.offset;
    historyScrollTop = root.scrollTop;
  }
  return cached;
}
