// 会话正文视图：调度并渲染消息列表、恢复已暂存的正文；滚动、跟随与分页按钮等视图状态在 history-state.js，正文暂存在 transcript.js，加载占位在 history.js。

import { renderMessageActions } from "../../message-actions.js";
import { copyText } from "../../ui/clipboard.js";
import { $, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { renderItem, renderQueueArea } from "../../core/actions.js";
import {
  busy, connected, creating, selectedId, selectionVersion, session, setTurns, stopping, thread,
  turns
} from "../../core/state.js";
import { readCache, transcriptCache } from "../../core/stores.js";
import { setSelectedEffort, setSelectedModel } from "../composer/options-state.js";
import { resizeEditorInput } from "../composer/prompt-editor.js";
import { skillRenderVersion } from "../composer/skill-catalog.js";
import { compactionLocked } from "./compaction.js";
import {
  followMessages, historyCursor, historyError, historyInitialized, historyLoadButton,
  legacySentDisplayTurn, messageFrame, renderTurnPlanTitle, renderedItems, renderedTurns,
  setFollowMessages, setHistoryCursor, setHistoryInitialized, setHistoryScrollTop, setMessageFrame,
  sessionPlan, setRecentHistoryPage, setSessionPlan, setTranscriptChecking, syncHistoryButtons, turnFieldVersions,
  updateScrollControl, visibleMessageAnchor
} from "./history-state.js";
import { historyPlaceholder, loadHistory, scheduleHistoryLoad } from "./history.js";
import { pending } from "./pending-state.js";
import { relatedReturnTranscript, setRelatedReturnTranscript, welcomeNode } from "./selection-state.js";
import {
  completedManualCompactionAfterReply, replaceTranscriptEvents, transcriptEventTypes
} from "./transcript-events.js";
import {
  activityPresentation, claudeTurnAwaitingStart, finalMessageIndex, renderedActivities,
  syncActivityTimer
} from "../messages/activity-state.js";
import { renderActivity } from "../messages/activity.js";
import { renderEditedFiles } from "../messages/edited-files.js";
import { reasoningPreviewText, reconcileChildren } from "../messages/item-presentation.js";
import { markdownRenderVersion, scheduleMermaidBlocks } from "../messages/media-state.js";
import { beginMessageEdit, editingMessage } from "../messages/message-edit.js";
import { claudeQuestionReplies } from "../messages/question-reply.js";
import { isSubagentItem, subagentTool } from "../messages/subagents.js";
import {
  loadMessageSkillChoices, messageCopyText, textContent, userMessageHasSkills
} from "../messages/user-content.js";
import { reconcileSentMessageDisplays, withSentMessageDisplays } from "../queue/sent-display.js";
import { editingQueuedMessage } from "../queue/state.js";
import { threadSentMessageEditAllowed } from "../session/action-capabilities.js";
import {
  isActiveThread, normalizeRelatedView, privateInputScope, subagentsEnabled, threadReadOnly
} from "../session/capabilities.js";
import { relatedSessionView, relatedThreadViews, setRelatedSessionView } from "../session/related-state.js";
import { syncRelatedSessionLayout } from "../session/related.js";
import { currentThreadDisplay, readScope } from "../session/thread-cache.js";
import { composerScroll } from "../shell-state.js";

export function scheduleMessages() {
  if (messageFrame != null) return;
  setMessageFrame(requestAnimationFrame(() => { setMessageFrame(null); renderMessages(); }));
}
function historyGapIndex(turn) {
  const items = turn.items || [];
  let index = items.findIndex(item => item.id === turn.itemsCursorAnchor);
  if (index < 0) index = items.findIndex(item => !["userMessage", "modelChanged", "modelRerouted"].includes(item.type));
  if (index < 0) index = items.length;
  while (index > 0 && transcriptEventTypes.has(items[index - 1].type) && !items[index - 1].remoteHistoryContext) index--;
  return index;
}
export function renderMessages(keepScroll = true) {
  cancelAnimationFrame(messageFrame); setMessageFrame(null);
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
  // 用户要求 Claude 原生任务清单属于整个会话：完整清单或原生轮次标记识别 Claude，
  // 统一放到正文所有轮次之后，旧服务才从已加载轮次寻找最新快照。
  const claudeSession = sorted.some(turn => turn?._nativeUserId) || Array.isArray(sessionPlan);
  // 用户要求新页面不等待下一次任务更新；新服务提供完整会话快照，旧服务才从已加载轮次兼容读取。
  let claudePlan = Array.isArray(sessionPlan) ? sessionPlan : null;
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
      // 已回答的 Claude 提问是用户发出的回复，与用户消息一样在原位置单独显示，不并入折叠的工作分组。
      if (item.type === "userMessage" || transcriptEventTypes.has(item.type) || claudeQuestionReplies(item)) {
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
    if (!claudeSession && turn.plan?.length) {
      const plan = element("ul", "turn-plan");
      for (const step of turn.plan) plan.append(element("li", step.status, `${step.status === "completed" ? "✓" : step.status === "inProgress" ? "◌" : "○"} ${step.step}`));
      items.push(renderTurnPlanTitle(turn.plan), plan);
    }
    // 用户要求手动压缩位于本轮所有收尾内容之后；“已编辑文件”和任务清单由这里追加，
    // 必须先渲染这些结果再放压缩提示，避免提示出现在文件卡片和任务统计之前。
    for (const item of trailingManualCompactions) items.push(renderMessage(item));
    reconcileChildren(group, items);
    if (items.length) children.push(group);
    if (claudeSession && !Array.isArray(sessionPlan) && Array.isArray(turn.plan)) claudePlan = turn.plan;
  }
  if (claudeSession && claudePlan?.length) {
    const plan = element("section", "claude-session-plan");
    plan.dataset.planScope = "claude-session";
    const title = renderTurnPlanTitle(claudePlan);
    const list = element("ul", "turn-plan");
    for (const step of claudePlan)
      list.append(element("li", step.status,
        `${step.status === "completed" ? "✓" : step.status === "inProgress" ? "◌" : "○"} ${step.step}`));
    plan.append(title, list);
    children.push(plan);
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
  setHistoryScrollTop(root.scrollTop);
  updateScrollControl();
  scheduleHistoryLoad();
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
  if (!turnFieldVersions.has("$session:plan")) setSessionPlan(Array.isArray(cached.sessionPlan) ? cached.sessionPlan : null);
  if (view) {
    for (const [key, value] of view.items) renderedItems.set(key, value);
    for (const [key, value] of view.groups) renderedTurns.set(key, value);
    for (const [key, value] of view.activities) renderedActivities.set(key, value);
  }
  replaceTranscriptEvents(cached.transcriptEvents);
  setHistoryCursor(cached.historyCursor); setHistoryInitialized(true);
  setRecentHistoryPage(cached);
  setFollowMessages(cached.followMessages); setTranscriptChecking(true);
  renderMessages();
  const root = $("#messages");
  if (root) {
    root.scrollTop = cached.followMessages ? root.scrollHeight : cached.scrollTop || 0;
    const anchor = cached.anchor && renderedItems.get(cached.anchor.key)?.node;
    if (!cached.followMessages && anchor?.isConnected)
      root.scrollTop += anchor.getBoundingClientRect().top - root.getBoundingClientRect().top - cached.anchor.offset;
    setHistoryScrollTop(root.scrollTop);
  }
  return cached;
}
