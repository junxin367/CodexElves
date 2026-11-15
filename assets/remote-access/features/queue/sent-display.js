// 已发送消息的展示接替：发送回执确认后在正文中展示，并按原生消息身份去重接替。

import { selectedId, session, turns } from "../../core/state.js";
import { renderMessages } from "../conversation/history-view.js";
import { queueMessageIdentity, queueMessageText, renderQueueArea } from "./queue.js";
import { privateInputScope } from "../session/capabilities.js";

export const sentMessageDisplays = new Map();
export const retiredSendPayloads = new WeakSet();
export function rememberSendDisplay(payload) {
  // 用户要求发送和引导成功后才进入正文；这里只保存原输入，不据此展示、确认送达或重发。
  if (!payload?.threadId || !payload.clientUserMessageId || !session || retiredSendPayloads.has(payload)) return null;
  const key = JSON.stringify([payload.threadId, payload.clientUserMessageId]);
  const existing = sentMessageDisplays.get(key);
  if (existing?.scope === privateInputScope(session)) return existing;
  const record = {
    payload, threadId: payload.threadId, clientId: payload.clientUserMessageId,
    scope: privateInputScope(session), confirmed: false,
    previousTurnId: payload.threadId === selectedId ? [...turns.keys()].at(-1) : null,
    index: payload.threadId === selectedId ? turns.size : Number.MAX_SAFE_INTEGER
  };
  sentMessageDisplays.set(key, record);
  // 与会话展示缓存一样保留最近 32 个会话；同一会话已显示的成功消息不按条数淘汰。
  const ids = [...new Set([...sentMessageDisplays.values()].map(value => value.threadId))];
  for (const id of ids.filter(id => id !== selectedId && id !== payload.threadId)
    .slice(0, Math.max(0, ids.length - 32))) clearSentMessageDisplays(id);
  return record;
}
export function clearSentMessageDisplays(id, { retire = false } = {}) {
  for (const [key, record] of sentMessageDisplays)
    if (record.threadId === id) {
      // 显式历史回退使旧派发失效；迟到成功回执不能重新创建该输入的成功展示，弱引用不额外保留正文。
      if (retire) retiredSendPayloads.add(record.payload);
      sentMessageDisplays.delete(key);
    }
}
export function settleSentMessageDisplays(id) {
  // 用户反馈切换会话后，旧消息只剩提问并被补到正文末尾或执行中轮次之前，看起来时序错乱、回复丢失。
  // 进入新视图时，此前已被原生正文接替的成功记录改以本次读取的历史为准：所在轮次已加载时照常留在原位，
  // 位于未加载的更早历史时不再按旧窗口位置补插。仍未见原生正文的成功投影不受影响，继续遵守
  // “空队列、迟到快照或历史暂未同步时不消失”。须与 selectThread 清空轮次处及 withSentMessageDisplays 一并核对。
  for (const record of sentMessageDisplays.values())
    if (record.threadId === id && record.nativeItem) record.settled = true;
}
function sentMessageIdentities(item) {
  return [item?.clientId, item?.clientUserMessageId, item?.serverClientUserMessageId,
    item?.submissionOptions?.clientUserMessageId, item?.restoreMessage?.clientUserMessageId,
    item?.restoreMessage?.id, item?.id].filter(value => typeof value === "string" && value);
}
export function sentMessageDisplayFor(item, id = selectedId) {
  const identities = sentMessageIdentities(item);
  return [...sentMessageDisplays.values()].find(record => record.threadId === id
    && record.scope === privateInputScope(session)
    && [record.clientId, record.messageId, ...sentMessageIdentities(record.item)]
      .some(identity => identities.includes(identity))) || null;
}
export function confirmSendDisplay(payload, result, { steering = false } = {}) {
  if (!payload?.threadId || result?.threadId && result.threadId !== payload.threadId
      || !steering && result?.clientUserMessageId && result.clientUserMessageId !== payload.clientUserMessageId
      || result?.ok === false || result?.outcomeUnknown === true
      || !steering && result?.deliveryState !== "accepted" && result?.result?.status !== "sent"
        && !(result?.ok === true && !Array.isArray(result.queuedMessages) && !result.queueSyncPending)) return false;
  const record = rememberSendDisplay(payload);
  if (!record) return false;
  const changed = !record.confirmed;
  record.confirmed = true;
  if (changed) {
    let order = 0;
    for (const value of sentMessageDisplays.values()) order = Math.max(order, value.confirmationOrder || 0);
    record.confirmationOrder = order + 1;
    if (payload.threadId === selectedId) {
      record.previousTurnId = [...turns.keys()].at(-1) || null;
      record.index = turns.size;
    }
  }
  record.turnId ||= result?.turnId || result?.result?.turnId || null;
  record.messageId ||= result?.messageId || null;
  if (!record.item) record.item = {
    id: `remote-confirmed-input:${record.clientId}`, type: "userMessage", clientId: record.clientId,
    // 成功回执可展示正文，但该投影仍不能作为原生回执核对或已发送消息编辑的依据。
    remotePendingInput: true, remoteConfirmedInput: true, remoteDirectSend: true,
    content: [
      ...(payload.prompt ? [{ type: "text", text: payload.prompt }] : []),
      ...(payload.images || []).map(image => ({ type: "image", url: image.dataUrl ?? image.url ?? image.src })),
      ...(payload.skills || []).map(skill => ({ type: "skill", name: skill.name, path: skill.path }))
    ]
  };
  return changed;
}
export function confirmedSendDisplayFor(message, id = selectedId) {
  const record = sentMessageDisplayFor(message, id);
  return record?.confirmed ? record : null;
}
export function reconcileSentMessageDisplays(id, messages) {
  if (!Array.isArray(messages)) return false;
  let changed = false;
  for (const message of messages) {
    const record = sentMessageDisplayFor(message, id) || rememberSendDisplay({
      threadId: id, clientUserMessageId: queueMessageIdentity(message) || message.id,
      prompt: queueMessageText(message), images: message.context?.imageAttachments || message.images || [],
      skills: message.context?.skills || message.skills || []
    });
    if (record && message.remoteDeliveryState === "accepted") changed = confirmSendDisplay(record.payload, {
      threadId: id, messageId: message.id, deliveryState: "accepted"
    }) || changed;
  }
  return changed;
}
export function withSentMessageDisplays(source = [...turns.values()]) {
  if (!selectedId || !session) return source;
  const nativeIds = new Set(source.flatMap(turn => (turn.items || [])
    .filter(item => item.type === "userMessage" && !item.remotePendingInput)
    .flatMap(sentMessageIdentities)));
  // 重进缓存时先恢复成功身份；真实消息即使在另一轮到达，也应优先接替旧成功投影。
  for (const [index, turn] of source.entries()) for (const item of turn.items || []) {
    if (!item.remoteConfirmedInput || item.type !== "userMessage") continue;
    const record = sentMessageDisplayFor(item) || rememberSendDisplay({
      threadId: selectedId, clientUserMessageId: item.clientId ?? item.clientUserMessageId ?? item.id
    });
    if (record && !record.item) {
      record.item = item; record.confirmed = true; record.turnId = turn.id; record.index = index;
      // 缓存里的原生正文副本来自此前视图，同样以本次读取的历史为准；未见原生正文的成功投影仍需桥接显示。
      if (!item.remotePendingInput) record.settled = true;
    }
  }
  const displayed = source.map(turn => ({ ...turn, items: (turn.items || []).filter(item =>
    !item.remotePendingInput || item.remoteCompactionInput
      || item.remoteConfirmedInput && !sentMessageIdentities(item).some(identity => nativeIds.has(identity))) }));
  const sourceIds = new Set(source.map(turn => turn.id));
  const byIdentity = new Map();
  const indexRecord = record => {
    for (const identity of [record.clientId, record.messageId, ...sentMessageIdentities(record.item)])
      if (identity) byIdentity.set(identity, record);
  };
  for (const record of sentMessageDisplays.values())
    if (record.threadId === selectedId && record.scope === privateInputScope(session)) indexRecord(record);
  const received = new Set();
  for (const [turnIndex, turn] of displayed.entries()) {
    for (const [itemIndex, item] of turn.items.entries()) {
      if (item.type !== "userMessage" || !item.id || item.remoteCompactionInput || item.remoteHistoryContext
          || item.remotePendingInput && !item.remoteConfirmedInput) continue;
      let record = sentMessageIdentities(item).map(identity => byIdentity.get(identity)).find(Boolean);
      // 只保留本次发送/引导及缓存中已有成功标记的输入；原生显式退役的内部摘要继续按原规则清理。
      if (!record && item.remoteConfirmedInput) record = rememberSendDisplay({
        threadId: selectedId, clientUserMessageId: item.clientId ?? item.clientUserMessageId ?? item.id
      });
      if (!record) continue;
      if (received.has(record)) {
        turn.items = turn.items.filter(candidate => candidate !== item);
        continue;
      }
      received.add(record);
      record.confirmed = true;
      // 原生正文接替成功投影；保留其稳定身份和位置，迟到/缺项快照不能让已显示消息消失。
      if (!item.remotePendingInput || !record.item) {
        const previous = record.item;
        record.item = record.nativeItem === item ? record.item
          : { ...item, remoteConfirmedInput: true,
            ...(previous?.content?.length && !item.content?.length ? { content: previous.content,
              ...(!item.remoteImages && previous.remoteImages ? { remoteImages: previous.remoteImages } : {}) } : {}) };
        record.nativeItem = item;
        turn.items[itemIndex] = record.item;
      }
      indexRecord(record);
      record.turnId = turn.id; record.index = turnIndex; record.itemIndex = itemIndex;
      record.previousTurnId = displayed[turnIndex - 1]?.id || null;
      record.nextTurnId = displayed[turnIndex + 1]?.id || null;
      record.nextItemId = turn.items[itemIndex + 1]?.id || null;
    }
  }
  for (const record of [...sentMessageDisplays.values()].sort((left, right) =>
    (left.confirmationOrder ?? Infinity) - (right.confirmationOrder ?? Infinity))) {
    if (!record.confirmed || !record.item || received.has(record)
        || record.threadId !== selectedId || record.scope !== privateInputScope(session)) continue;
    let turn = displayed.find(turn => turn.id === record.turnId);
    // 此前视图已接替的记录只在所在轮次已加载时补回缺项；轮次属于未加载的更早历史时不再按旧位置补插，
    // 避免切换会话后旧消息出现在正文末尾或执行中轮次之前。本视图内接替或刚确认的消息仍按下方原规则保持可见。
    if (!turn && record.settled) continue;
    if (!turn) {
      turn = { id: record.turnId || `remote-confirmed-turn:${record.clientId}`, status: "completed", items: [] };
      const next = displayed.findIndex(turn => turn.id === record.nextTurnId);
      const previous = displayed.findIndex(turn => turn.id === record.previousTurnId);
      let index = next >= 0 ? next : previous >= 0 ? previous + 1 : Math.min(record.index, displayed.length);
      // 同一历史空档内连续确认的消息按实际成功顺序接续，不能反向插到前一条成功消息之前。
      if (next < 0) while (index < displayed.length && !sourceIds.has(displayed[index].id)) index++;
      displayed.splice(index, 0, turn);
    }
    const next = turn.items.findIndex(item => item.id === record.nextItemId);
    turn.items.splice(next >= 0 ? next : Math.min(record.itemIndex ?? turn.items.length, turn.items.length), 0, record.item);
  }
  return displayed;
}
export function finishSendDisplay(record, failed = false) {
  if (!record) return;
  const key = JSON.stringify([record.threadId, record.clientId]);
  if (sentMessageDisplays.get(key) !== record) return;
  if (failed && !record.confirmed) sentMessageDisplays.delete(key);
  if (record.threadId === selectedId && record.scope === privateInputScope(session)) {
    renderQueueArea(); renderMessages();
  }
}
