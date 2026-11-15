// 会话正文事件：Claude 事件与压缩完成等正文事件的合并与刷新，以及历史条目与远程图片投影的合并。

import { turns } from "../../core/state.js";
import { completedItem, itemVersions } from "./history-state.js";
import { messageImageMediaReads } from "../messages/media-state.js";

export const transcriptEvents = new Map();
// Claude 事件行（Rust claude_tools.rs 生成）：claudeTaskNotification 为后台命令、监控事件、后台子代理通知开启的
// 自动续跑轮次起点；claudeLocalCommand 为终端执行的本地斜杠命令；claudeNotice 为离开期间回顾、原生提示与 Stop Hook。
export const transcriptEventTypes = new Set(["modelChanged", "modelRerouted", "contextCompaction",
  "claudeTaskNotification", "claudeLocalCommand", "claudeNotice"]);
export function replaceTranscriptEvents(entries, readVersion = null) {
  // 用户要求切回/刷新不丢压缩记录；读取开始后收到的事件比这次快照新，不能被空列表清掉。
  const live = Number.isFinite(readVersion) ? [...transcriptEvents.entries()]
    .filter(([key]) => (itemVersions.get(key) || 0) > readVersion).map(([, entry]) => entry) : [];
  transcriptEvents.clear();
  for (const entry of entries || []) {
    const item = entry?.item;
    if (typeof entry?.turnId !== "string" || typeof item?.id !== "string"
        || !transcriptEventTypes.has(item.type)) continue;
    transcriptEvents.set(`${entry.turnId}:${item.id}`, {
      turnId: entry.turnId,
      item,
      beforeItemId: typeof entry.beforeItemId === "string" ? entry.beforeItemId : null
    });
  }
  mergeTranscriptEvents(live);
}
export function mergeTranscriptEvents(entries) {
  for (const entry of entries || []) {
    const item = entry?.item;
    if (typeof entry?.turnId !== "string" || typeof item?.id !== "string"
        || !transcriptEventTypes.has(item.type)) continue;
    const key = `${entry.turnId}:${item.id}`, previous = transcriptEvents.get(key);
    // 已收到的完成证据不能被迟到的进行中历史覆盖；失败和中断状态也随完成记录一起保留。
    const merged = item.type === "contextCompaction" && completedItem(previous?.item) && !completedItem(item)
      ? mergeItemProjection(item, previous.item) : item;
    transcriptEvents.set(key, {
      turnId: entry.turnId,
      item: merged,
      beforeItemId: typeof entry.beforeItemId === "string" ? entry.beforeItemId : null
    });
  }
}
export function completedManualCompactionAfterReply(item, items, index) {
  // 用户要求上一轮回复完成后手动点击压缩时，完成提示必须位于该轮最终回复之后；
  // 只有后面没有新的可见用户输入时才适用，自动压缩和后续新问题保持原生顺序。
  return item?.type === "contextCompaction" && item.source === "manual" && completedItem(item)
    && !items.some((candidate, candidateIndex) => candidateIndex > index
      && candidate.type === "userMessage" && !candidate.remoteCompactionInput && !candidate.remoteHistoryContext);
}
export function enrichTurnTranscriptEvents(turn) {
  const items = [...(turn.items || [])];
  const turnStatus = typeof turn.status === "string" ? turn.status : turn.status?.type;
  // 切回时可能只读到压缩开始记录；终态轮次不能继续显示压缩中，已有条目完成证据仍优先。
  const settledItems = values => values.map(item => item.type === "contextCompaction" && !item.remoteCompactionLifecycle && !completedItem(item)
    && ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(turnStatus)
    ? { ...item, completed: true, status: turnStatus } : item);
  const pending = [];
  let initialCompaction = null;
  const events = [...transcriptEvents.values()].filter(entry => entry.turnId === turn.id);
  const eventIds = new Set(events.map(entry => entry.item.id));
  for (const item of items) {
    if (item.type === "modelChanged" && !eventIds.has(item.id))
      events.push({ item, beforeItemId: item.beforeItemId || null });
  }
  const beforeInput = () => {
    let index = items.findIndex(item => item.type === "userMessage");
    if (index < 0) index = items.findIndex(item => !["modelChanged", "modelRerouted"].includes(item.type));
    return index < 0 ? items.length : index;
  };
  for (const entry of events) {
    if (!initialCompaction && entry.item.type === "contextCompaction" && entry.item.source !== "manual")
      initialCompaction = entry;
    const existing = items.findIndex(item => item.id === entry.item.id);
    if (existing >= 0) {
      const item = { ...items[existing], ...entry.item,
        ...(items[existing].type === "contextCompaction" && items[existing].completed === true ? { completed: true } : {}) };
      // Claude 同一轮中途换配置时保留原生请求位置，不能把后半轮的变化提前到用户问题之前。
      if (item.type !== "modelChanged" || item.remoteInlineEvent) { items[existing] = item; continue; }
      items.splice(existing, 1);
      pending.push({ ...entry, item });
    } else pending.push(entry);
  }
  while (pending.length) {
    let inserted = false;
    for (let index = 0; index < pending.length;) {
      const entry = pending[index];
      const insertion = entry.beforeItemId ? items.findIndex(item => item.id === entry.beforeItemId)
        : entry.item.type === "modelChanged" && !entry.item.remoteInlineEvent ? beforeInput() : items.length;
      if (insertion < 0) { index++; continue; }
      items.splice(insertion, 0, entry.item);
      pending.splice(index, 1);
      inserted = true;
    }
    if (inserted) continue;
    for (const entry of pending.splice(0)) {
      const insertion = ["modelChanged", "modelRerouted"].includes(entry.item.type) && !entry.item.remoteInlineEvent
        ? beforeInput() : items.length;
      items.splice(insertion, 0, entry.item);
    }
  }
  // 用户要求手动压缩在“上一轮回复完成后点击压缩”时显示在会话末尾；
  // 旧服务或迟到快照可能把同一条完成事件插到最终回复前，只在其后没有新用户输入时调整。
  const manualCompactions = items.filter((item, index) =>
    completedManualCompactionAfterReply(item, items, index));
  if (manualCompactions.length) {
    const moving = new Set(manualCompactions);
    items.splice(0, items.length, ...items.filter(item => !moving.has(item)), ...manualCompactions);
  }
  const projected = items.find(item => item.type === "userMessage" && item.remoteCompactionInput === true);
  if (projected) {
    const delivered = items.find(item => item.type === "userMessage" && !item.remotePendingInput
      && item.clientId === projected.clientId);
    const input = delivered && !delivered.remoteCompactionInput
      ? { ...delivered, remoteCompactionInput: true } : delivered || projected;
    const index = initialCompaction ? items.findIndex(item => item.id === initialCompaction.item.id)
      : items.findIndex(item => item.type === "contextCompaction" && item.source !== "manual");
    if (index >= 0) {
      const compaction = items[index];
      const missingAnchor = initialCompaction?.beforeItemId
        && !items.some(item => item.id === initialCompaction.beforeItemId);
      const rest = items.filter(item => item !== projected && item !== delivered
        && !(item.remotePendingInput && item.clientId === projected.clientId)
        && (!missingAnchor || item !== compaction));
      if (missingAnchor) {
        const before = rest.findIndex(item => !["modelChanged", "modelRerouted"].includes(item.type));
        rest.splice(before < 0 ? rest.length : before, 0, input, compaction);
      } else {
        const before = rest.findIndex(item => item.id === compaction.id);
        rest.splice(before, 0, input);
      }
      return { ...turn, items: settledItems(rest) };
    }
  }
  return { ...turn, items: settledItems(items) };
}
export function refreshTranscriptEvents() {
  for (const [id, turn] of turns) turns.set(id, enrichTurnTranscriptEvents(turn));
}

export function mergeRemoteImageProjection(previous, snapshot, item, mediaReadVersion = 0) {
  const prior = previous && (messageImageMediaReads.get(previous) || messageImageMediaReads.get(previous.remoteImages)),
    retained = messageImageMediaReads.get(item) || messageImageMediaReads.get(item.remoteImages);
  if (!prior && !retained && !(mediaReadVersion > 0 && Array.isArray(snapshot?.remoteImages))) return item;
  const assets = new Map((item.remoteImages || []).map(asset => [asset.source, asset]));
  if (!assets.size) return item;
  const reads = new Map(prior || []);
  for (const [source, read] of retained || [])
    if (!reads.has(source) || reads.get(source).version < read.version) reads.set(source, read);
  // 用户要求已完成正文仍可恢复图片；媒体映射独立按读取发起序号更新，旧缓存/迟到历史不能撤回新 URL。
  // 序号只保存在 WeakMap 中，不进入会话正文缓存，页面重载后首次真实读取仍能恢复服务重启后的注册。
  if (mediaReadVersion > 0) for (const asset of snapshot?.remoteImages || []) {
    if (!assets.has(asset?.source) || !/^\/api\/images\/[A-Za-z0-9_-]{20,128}$/.test(asset?.url || "")
        || (reads.get(asset.source)?.version || 0) > mediaReadVersion) continue;
    reads.set(asset.source, { version: mediaReadVersion, asset });
  }
  for (const [source, read] of reads) if (assets.has(source)) assets.set(source, read.asset);
  const images = [...assets.values()];
  const projected = JSON.stringify(images) === JSON.stringify(item.remoteImages) ? item : { ...item, remoteImages: images };
  if (reads.size) {
    messageImageMediaReads.set(projected, reads);
    // 流式正文会浅拷贝条目；共享媒体数组也保留读取序号，复制正文不能丢失图片的新旧关系。
    messageImageMediaReads.set(projected.remoteImages, reads);
  }
  return projected;
}
export function mergeItemProjection(previous, item) {
  // 单条状态事件也可能晚于正文到达，不能用同 ID 的 steered 把已接受的引导内容改回空状态。
  if (previous?.id === item.id && previous.type === "userMessage" && item.type === "steered") return previous;
  const projection = {};
  for (const field of ["messageMetadata", "sentAtMs", "startedAtMs", "completedAtMs", "completed",
    "remoteImages", "remoteVideos", "clientId", "remoteCompactionInput", "remoteHistoryContext", "remoteItemCompleted"])
    if (!Object.hasOwn(item, field) && Object.hasOwn(previous || {}, field)) projection[field] = previous[field];
  const merged = mergeRemoteImageProjection(previous, null, { ...projection, ...item });
  return previous && JSON.stringify(previous) === JSON.stringify(merged) ? previous : merged;
}
