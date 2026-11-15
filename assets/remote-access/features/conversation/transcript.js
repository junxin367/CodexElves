// 正文暂存与重置：切换会话前记住已渲染的正文，切换后清空消息区状态。

import { selectedId, session, sidebar, thread, turns } from "../../core/state.js";
import { transcriptCache } from "../../core/stores.js";
import { selectedEffort, selectedModel } from "../composer/options-state.js";
import { clearPendingFeedbackTimer } from "../composer/send-state.js";
import { setNotifiedStatus } from "./event-sync.js";
import {
  historyController, historyCursor, historyInitialized, historyLoadFrame, itemVersions,
  messageFrame, partialItems, recentHistoryPage, renderedItems, renderedTurns, setFollowMessages,
  setHistoryAutoFailed, setHistoryController, setHistoryError, setHistoryLoadFrame,
  setHistoryLoadVisibleItems, setHistoryLoading, setHistoryLoadingOlder, setHistoryLoadingTurnId,
  setHistoryLoadingVersion, setHistoryRequest, setHistoryScrollTop, setLatestHistoryReadFailed,
  setMessageFrame, setRecentHistoryPage, setTranscriptChecking, turnFieldVersions, turnVersions
} from "./history-state.js";
import {
  selectionController, setRelatedReturnTranscript, setSelectionController, setThreadReadFailed
} from "./selection-state.js";
import { transcriptEvents } from "./transcript-events.js";
import { renderedActivities, stopActivityTimer } from "../messages/activity-state.js";
import { messageImageRefreshes } from "../messages/media-state.js";
import { finishQueueDrag } from "../queue/queue.js";
import { withSentMessageDisplays } from "../queue/sent-display.js";
import {
  queueVersion, retiredQueueEpochs, setNotifiedQueue, setNotifiedQueueEdits,
  setQueueSnapshotVersion, setQueueVersion
} from "../queue/state.js";
import { normalizeRelatedView, privateInputScope } from "../session/capabilities.js";
import { relatedSessionView } from "../session/related-state.js";
import {
  cacheDisplayData, readingPosition, rememberReadingPosition, threadEphemeralState
} from "../session/thread-cache.js";
import { rememberThreadDisplay } from "../session/thread-display.js";
import { composerScroll } from "../shell-state.js";
import { threadDeleted } from "../sidebar/deletion-state.js";

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
  cancelAnimationFrame(historyLoadFrame); setHistoryLoadFrame(null); setHistoryAutoFailed(false);
  setHistoryScrollTop(0); setHistoryLoadVisibleItems(false);
  cancelAnimationFrame(messageFrame); setMessageFrame(null);
  for (const entry of renderedItems.values()) entry.node.messageActionBinding?.dispose();
  renderedItems.clear(); renderedTurns.clear(); renderedActivities.clear(); itemVersions.clear(); turnVersions.clear(); turnFieldVersions.clear();
  partialItems.clear();
  setFollowMessages(true);
  setTranscriptChecking(false);
  setRecentHistoryPage(null);
  setNotifiedStatus(null);
  setNotifiedQueue(null); setNotifiedQueueEdits(null); setQueueSnapshotVersion(null); retiredQueueEpochs.clear(); setQueueVersion(queueVersion + 1);
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
