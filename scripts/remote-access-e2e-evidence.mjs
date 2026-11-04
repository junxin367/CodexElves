/**
 * 远程访问回归的文件证据门禁。只读取本机文件；不连接服务、不发送消息。
 * rawFacts 的值必须是原始 JSON 的 JSON Pointer，布尔声明不能代替原始事实。
 */
import { readFile, stat } from "node:fs/promises";
import { resolve, relative, dirname, isAbsolute, sep } from "node:path";
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify, isDeepStrictEqual } from "node:util";
import { fileURLToPath } from "node:url";
import { Buffer } from "node:buffer";

export const REMOTE_ACCESS_EVIDENCE_VERSION = "1.3.6";
const executeFile = promisify(execFile);
const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
const hashPattern = /^[a-fA-F0-9]{64}$/;
const sources = new Set(["rpc", "bridge", "native-log", "android-device", "desktop-ui"]);
const identityNames = ["threadId", "turnId", "receiptId", "readId", "eventId", "parentThreadId"];
const receiptNames = new Set(["receiptId", "readId", "eventId"]);
const successfulReceiptStates = new Set([
  "completed", "confirmed", "succeeded", "success", "created", "sent",
  "queued", "enqueued", "closed", "deleted", "resolved", "interrupted", "cancelled", "canceled",
]);
const rawOutcomes = new WeakMap();
const createKinds = new Set(["create-send", "side-create", "create-side", "create-thread", "external-created-this-run"]);
const inputRoots = [
  ".gitignore", "Cargo.toml", "Cargo.lock",
  "assets/remote-access", "assets/inject",
  // 远程 routes 和 service 的内部实现会跨 Rust 子模块调用，保守覆盖整个 core 源码。
  "crates/codex-elves-core/src", "crates/codex-elves-core/Cargo.toml",
  "apps/codex-elves-android",
  "scripts/remote-access-e2e.mjs", "scripts/remote-access-e2e-evidence.mjs",
  ":(glob)scripts/remote-access-e2e*.mjs",
  "scripts/check-remote-access-assets.mjs", "scripts/build-android.ps1",
  "scripts/android-build-common.ps1", "scripts/sync-android-build.ps1",
  "scripts/publish-android-release.ps1", ".github/workflows/android-apk.yml",
];
const essentialInputs = [
  "assets/remote-access/native.js", "assets/remote-access/app.js",
  "crates/codex-elves-core/src/remote_access.rs",
  "crates/codex-elves-core/src/codex_service.rs",
  "crates/codex-elves-core/src/prompt_optimize.rs",
  "crates/codex-elves-core/src/routes.rs",
  "scripts/remote-access-e2e.mjs", "scripts/remote-access-e2e-evidence.mjs",
  "scripts/remote-access-e2e-environment.mjs",
];
const operations = {
  "create-send": ["create-send", "create", "createThread", "thread/start"],
  "side-create": ["side-create", "createSideThread", "thread/fork", "thread/start"],
  send: ["send", "sendMessage", "sendFollowUpMessage", "turn/start", "message/send"],
  "queue-send": ["queue-send", "send", "enqueue", "queueMessage", "thread/queue/add"],
  model: ["model", "setModel", "thread/settings/update", "thread/model/update"],
  "queue-edit-cancel": ["queue-edit-cancel", "cancelQueuedEdit", "cancelQueuedMessageEdit", "thread/queue/edit/cancel"],
  "queue-edit-save": ["queue-edit-save", "editQueued", "editQueuedMessage", "thread/queue/edit"],
  "queue-race-edit": ["queue-race-edit", "beginQueuedEdit", "editQueued", "beginQueuedMessageEdit", "editQueuedMessage"],
  "queue-race-remove": ["queue-race-remove", "removeQueuedMessage", "removeQueued"],
  "queue-reorder": ["queue-reorder", "reorderQueued", "reorderQueuedMessages", "thread/queue/reorder"],
  "queue-remove": ["queue-remove", "removeQueued", "removeQueuedMessage", "thread/queue/remove"],
  "queue-steer": ["queue-steer", "steerQueued", "steerQueuedMessage", "turn/steer"],
  stop: ["stop", "interrupt", "interruptTurn", "turn/interrupt"],
  "side-close": ["side-close", "closeSideThread", "thread/close"],
  pin: ["pin", "pinThread", "thread/pin"],
  rename: ["rename", "renameThread", "thread/name/set"],
  "edit-message": ["edit-message", "editMessage"],
  "project-pin": ["project-pin", "pinProject"],
  "create-goal": ["create-goal", "setGoal"],
  "permanent-delete": ["permanent-delete", "deleteThread", "thread/delete"],
  "external-created-this-run": ["create", "createThread", "thread/start"],
};

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function nonempty(value) {
  return typeof value === "string" && value.trim().length > 0;
}

/** 取消派发前即可校验的原队列与ED基线。 */
export function verifyQueueEditCancelEditingFacts({ before, editing }, { threadId, queueId, editId }) {
  const require = (value, message) => invariant(value, `队列取消：${message}`);
  require([before, editing].every(value => value?.threadId === threadId), "原生阶段串会话");
  require(nonempty(queueId) && nonempty(editId) && Array.isArray(before.queuedMessages), "缺少原队列或准确ED");
  const originals = before.queuedMessages.filter(value => value.id === queueId);
  require(originals.length === 1, "取出前没有唯一原队列对象");
  const original = originals[0], index = before.queuedMessages.indexOf(original);
  require(nonempty(original.clientUserMessageId) && Array.isArray(original.input) && original.input.length > 0,
    "原对象缺少逻辑身份或完整原input");
  require(editing.editId === editId && editing.editState === "editing"
    && editing.originalMessageId === queueId && editing.originalIndex === index
    && editing.originalClientUserMessageId === original.clientUserMessageId
    && isDeepStrictEqual(editing.originalInput, original.input), "编辑事务不属于原对象、原位置或原input");
  return { original, index };
}

/** 忙时取消的原输入和位置以取出前原生队列为基线，不能从恢复结果反填。 */
export function verifyQueueEditCancelFacts({ before, editing, after }, { threadId, queueId, editId }) {
  const require = (value, message) => invariant(value, `队列取消：${message}`);
  const { original, index } = verifyQueueEditCancelEditingFacts({ before, editing }, { threadId, queueId, editId });
  require(after?.threadId === threadId && Array.isArray(after.queuedMessages), "缺少同会话的恢复队列");
  const receipt = after.cancelReceipt, result = receipt?.result;
  require(after.editId === editId && after.editState === "canceled"
    && nonempty(receipt?.requestId) && receipt.state === "completed" && result?.ok === true
    && result.threadId === threadId && result.editId === editId && result.editState === "canceled",
  "没有同一ED的真实取消完成回执");
  const restored = after.queuedMessages.filter(value => value.clientUserMessageId === original.clientUserMessageId);
  require(restored.length === 1 && nonempty(restored[0].id) && result.messageId === restored[0].id
    && isDeepStrictEqual(restored[0].input, original.input), "恢复对象身份、物理ID血缘或完整原input不一致");
  require(after.queuedMessages.length === before.queuedMessages.length
    && before.queuedMessages.every((value, position) => {
      const next = after.queuedMessages[position];
      return nonempty(value.clientUserMessageId) && next.clientUserMessageId === value.clientUserMessageId
        && isDeepStrictEqual(next.input, value.input) && (position === index || next.id === value.id);
    }), "取消后原位置或其他队列对象改变");
  require(new Set(after.queuedMessages.map(value => value.id)).size === after.queuedMessages.length
    && new Set(after.queuedMessages.map(value => value.clientUserMessageId)).size === after.queuedMessages.length,
  "恢复队列包含重复物理或逻辑对象");
  require(after.historyComplete === true && Array.isArray(after.nativeTurns)
    && !after.nativeTurns.some(turn => (turn.items || []).some(item =>
      item.type === "userMessage" && item.clientId === original.clientUserMessageId
      || item.type === "steeringUserMessage" && [
        item.clientId, item.clientUserMessageId, item.serverClientUserMessageId,
        item.restoreMessage?.clientUserMessageId, item.restoreMessage?.submissionOptions?.clientUserMessageId,
      ].includes(original.clientUserMessageId))),
  "恢复对象已经被消费、引导或缺少完整历史，不能把影子队列算通过");
  return { queueId: restored[0].id, clientUserMessageId: original.clientUserMessageId, editId, originalIndex: index };
}

function queueRecoveryRequirement(condition, message) {
  invariant(condition, `停止／失败队列恢复：${message}`);
}

function queueRecoveryTurnId(turn) {
  return turn?.turnId || turn?.id;
}

function queueRecoveryClientItems(turns, clientUserMessageId) {
  const matches = [];
  for (const turn of turns) {
    for (const item of turn.items || []) {
      if (item.type === "userMessage" && item.clientId === clientUserMessageId) {
        matches.push({ accepted: true, turn, item, input: item.content });
        continue;
      }
      if (item.type !== "steeringUserMessage") continue;
      const clientIds = [
        item.clientId, item.clientUserMessageId, item.serverClientUserMessageId,
        item.restoreMessage?.clientUserMessageId,
        item.restoreMessage?.submissionOptions?.clientUserMessageId,
      ];
      if (!clientIds.includes(clientUserMessageId)) continue;
      matches.push({
        accepted: item.status === "accepted",
        turn,
        item,
        input: item.input || item.restoreMessage?.input || item.restoreMessage?.submissionOptions?.input,
      });
    }
  }
  return matches;
}

function queueRecoveryHasExpectedTextInput(input, prompt) {
  if (!Array.isArray(input) || input.length !== 1) return false;
  const [item] = input;
  return item?.type === "text"
    && item.text === `${prompt}\n`
    && (item.text_elements === undefined
      || Array.isArray(item.text_elements) && item.text_elements.length === 0)
    && Object.keys(item).every(key => ["type", "text", "text_elements"].includes(key));
}

function queueRecoverySnapshotFacts(record, phase, expected) {
  queueRecoveryRequirement(record?.threadId === expected.threadId && record.phase === phase
    && typeof record.readId === "string" && record.readId.trim()
    && Number.isFinite(Date.parse(record.capturedAt)),
  `缺少 ${phase} 阶段的同会话原始读取身份`);
  const snapshot = record.queueRecovery;
  queueRecoveryRequirement(snapshot?.historyComplete === true
    && Array.isArray(snapshot.nativeTurns) && Array.isArray(snapshot.queuedMessages),
  `${phase} 阶段缺少完整原生历史或队列`);
  return snapshot;
}

function verifyQueueRecoveryReceipt(record, expected) {
  const facts = record?.facts || record;
  const receiptActionId = record?.actionId || facts?.actionId;
  queueRecoveryRequirement(receiptActionId === expected.queueActionId
    && facts?.threadId === expected.threadId
    && operations["queue-send"].includes(facts.operation)
    && nonempty(facts.receiptId)
    && successfulReceiptStates.has(facts.receiptState)
    && (facts.success === true || facts.receiptSuccess === true)
    && facts.deliveryState === "queued"
    && facts.queueId === expected.queueId
    && nonempty(facts.clientUserMessageId),
  "原 queue-send 没有准确的 completed/queued 回执、queue ID 与 client ID");
  return facts;
}

/**
 * 停止或失败后只在原生完整历史中唯一接受，或原 Q 完整保留时给出可操作结论。
 * 任何缺回执、缺完整快照、身份变化或状态冲突都返回 unknown。
 */
export function classifyQueueRecoveryTransition({ before, transition, receipt }, expected) {
  const require = (condition, message) => queueRecoveryRequirement(condition, message);
  require(["stop", "failure"].includes(expected.kind)
    && nonempty(expected.threadId) && nonempty(expected.activeTurnId)
    && nonempty(expected.queueId) && nonempty(expected.prompt)
    && nonempty(expected.expectedReply) && nonempty(expected.queueActionId),
  "runner 固定契约缺少准确会话、活动轮、原队列或逻辑消息");
  const receiptFacts = verifyQueueRecoveryReceipt(receipt, expected);
  const beforeState = queueRecoverySnapshotFacts(before, "before", expected);
  const transitionState = queueRecoverySnapshotFacts(transition, "transition", expected);
  require(new Set([before.readId, transition.readId]).size === 2
    && Date.parse(before.capturedAt) < Date.parse(transition.capturedAt),
  "转场前后重复使用读取证据或时间顺序倒退");
  require(receiptFacts.clientUserMessageId === expected.clientUserMessageId
    && nonempty(expected.clientUserMessageId),
  "queue-send 原回执的 client ID 与原 Q 不一致");
  const originalRows = beforeState.queuedMessages.filter(value => value.id === expected.queueId);
  require(beforeState.queuedMessages.length === 1 && originalRows.length === 1,
    "发送前基线不是本轮唯一原 Q");
  const original = originalRows[0];
  require(original.clientUserMessageId === expected.clientUserMessageId
    && Array.isArray(original.input)
    && queueRecoveryHasExpectedTextInput(original.input, expected.prompt)
    && (!receiptFacts.input || isDeepStrictEqual(receiptFacts.input, original.input)),
  "原 Q 的 client 或完整纯文本 input 与正式 queue-send 回执不一致");
  require(!queueRecoveryClientItems(beforeState.nativeTurns, expected.clientUserMessageId).length,
    "Q 在停止／失败前已经被正式接受");
  const activeBefore = beforeState.nativeTurns.filter(turn =>
    queueRecoveryTurnId(turn) === expected.activeTurnId);
  require(activeBefore.length === 1 && activeBefore[0].status === "inProgress",
    "操作前原活动轮身份或状态不匹配");
  const activeAfter = transitionState.nativeTurns.filter(turn =>
    queueRecoveryTurnId(turn) === expected.activeTurnId);
  const allowedAfterStatuses = expected.kind === "stop"
    ? ["interrupted", "cancelled", "canceled"] : ["failed"];
  require(activeAfter.length === 1 && allowedAfterStatuses.includes(activeAfter[0].status),
    "原活动轮没有得到本案例要求的停止／失败终态");
  const accepted = queueRecoveryClientItems(transitionState.nativeTurns, expected.clientUserMessageId);
  const retained = transitionState.queuedMessages.filter(value => value.id === expected.queueId);
  if (accepted.length === 1 && accepted[0].accepted && retained.length === 0) {
    require(isDeepStrictEqual(accepted[0].input, original.input),
      "自动接受的正式消息与原 Q 完整 input 不一致");
    return { outcome: "automatically-accepted", original };
  }
  if (accepted.length === 0 && retained.length === 1
      && transitionState.queuedMessages.length === 1
      && retained[0].clientUserMessageId === expected.clientUserMessageId
      && isDeepStrictEqual(retained[0].input, original.input))
    return { outcome: "retained", original };
  return { outcome: "unknown", original };
}

/** 终态必须以同一 client、完整 input、真实最终轮与原生空队列闭环。 */
export function verifyQueueRecoveryTerminal({ before, transition, terminal, receipt }, expected) {
  const require = (condition, message) => queueRecoveryRequirement(condition, message);
  const classified = classifyQueueRecoveryTransition({ before, transition, receipt }, expected);
  require(classified.outcome !== "unknown", "转场结果未知，不能引导或报告恢复完成");
  require(expected.outcome === classified.outcome,
    "runner 记录的恢复分支与原生队列/历史结果不一致");
  const terminalState = queueRecoverySnapshotFacts(terminal, "terminal", expected);
  require(new Set([before.readId, transition.readId, terminal.readId]).size === 3
    && Date.parse(transition.capturedAt) < Date.parse(terminal.capturedAt),
  "终态读取复用了旧证据或采集时间倒退");
  require(terminalState.queuedMessages.length === 0, "终态仍有原队列或其他未处置队列");
  const accepted = queueRecoveryClientItems(terminalState.nativeTurns, expected.clientUserMessageId);
  require(accepted.length === 1 && accepted[0].accepted
    && isDeepStrictEqual(accepted[0].input, classified.original.input),
  "原 client 没有且仅有一次接受，或最终完整 input 已改变");
  const finalTurnId = queueRecoveryTurnId(accepted[0].turn);
  const finalTurns = terminalState.nativeTurns.filter(turn => queueRecoveryTurnId(turn) === finalTurnId);
  require(finalTurns.length === 1 && finalTurns[0].status === "completed" && !finalTurns[0].error
    && (finalTurns[0].items || []).filter(item =>
      item.type === "agentMessage" && item.text === expected.expectedReply).length === 1,
  "原 client 所属最终轮没有唯一准确回复和 completed 终态");
  return {
    outcome: classified.outcome,
    queueId: expected.queueId,
    clientUserMessageId: expected.clientUserMessageId,
    turnId: finalTurnId,
  };
}

function queueRaceRequirement(condition, message) {
  invariant(condition, `自然出队竞争：${message}`);
}

function queueRaceSnapshot(record, phase, expected) {
  const snapshot = record?.queueRace;
  queueRaceRequirement(record?.threadId === expected.threadId && record.phase === phase
    && nonempty(record.readId) && Number.isFinite(timestamp(record.capturedAt))
    && snapshot?.historyComplete === true && Array.isArray(snapshot.nativeTurns)
    && Array.isArray(snapshot.serverQueue) && Array.isArray(snapshot.nativeQueue),
  `${phase} 缺少同会话完整历史、server 队列、native 组合显示投影或读取证据身份`);
  return snapshot;
}

function queueRaceIdempotencyKey(request) {
  const entries = Object.entries(request?.headers || {}).filter(([key]) =>
    key.toLowerCase() === "idempotency-key");
  return entries.length === 1 ? entries[0][1] : undefined;
}

// CDP 的可选元数据和普通请求头可以不同；原 POST 的 URL、正文和幂等身份必须完整保留。
function sameQueueRaceRequest(left, right) {
  const receiptId = queueRaceIdempotencyKey(left);
  return nonempty(left?.url) && left.url === right?.url
    && left.method === "POST" && right.method === "POST"
    && typeof left.postData === "string" && left.postData === right.postData
    && nonempty(receiptId) && receiptId === queueRaceIdempotencyKey(right);
}

function queueRaceRequest(record, expected) {
  const require = queueRaceRequirement, trace = record?.requestTrace;
  require(record?.threadId === expected.threadId && record.tabId === expected.tabId
    && trace?.truncated === false && Array.isArray(trace.events) && Array.isArray(trace.commands),
  "缺少准确测试 tab 的完整请求事件与实际控制命令");
  const route = `/api/action/${expected.attemptedKind === "edit" ? "beginQueuedEdit" : "removeQueued"}`;
  const requests = trace.events.filter(event => event.method === "Network.requestWillBeSent"
    && event.params?.request?.method === "POST" && new URL(event.params.request.url).pathname === route);
  require(requests.length === 1, "原始队列 POST 缺失或重复派发");
  const event = requests[0], request = event.params.request, networkId = event.params.requestId;
  require(nonempty(networkId) && new URL(request.url).origin === expected.origin
    && Number.isFinite(event.params.wallTime), "原始 POST 缺少 network ID、时间或属于其他 origin");
  let body;
  try { body = JSON.parse(request.postData); } catch {}
  const receiptId = queueRaceIdempotencyKey(request);
  require(nonempty(receiptId) && body?.threadId === expected.threadId && body.messageId === expected.queueId,
    "原 POST 缺少原 A 或准确 threadId/messageId");
  const pauses = trace.events.filter(value => value.method === "Fetch.requestPaused"
    && value.params?.networkId === networkId);
  require(pauses.length === 1 && nonempty(pauses[0].params.requestId)
    && pauses[0].params.responseStatusCode === undefined
    && pauses[0].params.responseErrorReason === undefined
    && sameQueueRaceRequest(pauses[0].params.request, request),
  "必须在请求阶段暂缓同一旧 POST，不能用回包暂停替代");
  require(!trace.events.some(value => value.method === "Network.requestWillBeSent"
    && value.params?.request?.method === "POST"
    && new URL(value.params.request.url).pathname.startsWith("/api/action/")
    && ![route, "/api/action/markRead"].includes(new URL(value.params.request.url).pathname)),
  "暂缓窗口夹带了重新发送、引导或其他写操作");
  return { trace, event, request, body, networkId, receiptId, pause: pauses[0] };
}

/** nativeQueue 是原生组合显示投影，不能用非原子的组合减 server 差集替代。 */
export function verifyQueueDequeueRaceConsumption({ before, held, consumed, receipt }, expected) {
  const require = queueRaceRequirement;
  require(["edit", "remove"].includes(expected.attemptedKind) && nonempty(expected.threadId)
    && nonempty(expected.activeTurnId) && nonempty(expected.queueId)
    && nonempty(expected.clientUserMessageId) && nonempty(expected.queueActionId)
    && nonempty(expected.prompt) && nonempty(expected.tabId), "缺少本轮原 T/Q/client/动作或测试 tab");
  const beforeState = queueRaceSnapshot(before, "before", expected);
  const consumedState = queueRaceSnapshot(consumed, "consumed", expected);
  const queuedReceipt = verifyQueueRecoveryReceipt(receipt, expected);
  const original = beforeState.serverQueue.find(value => value.id === expected.queueId);
  const native = beforeState.nativeQueue.find(value => value.id === expected.queueId);
  require(beforeState.serverQueue.length === 1 && beforeState.nativeQueue.length === 1
    && original?.clientUserMessageId === expected.clientUserMessageId
    && native?.clientUserMessageId === expected.clientUserMessageId
    && native.text === expected.prompt
    && (native.context?.prompt === undefined || native.context.prompt === expected.prompt)
    && queuedReceipt.clientUserMessageId === expected.clientUserMessageId
    && queueRecoveryHasExpectedTextInput(original.input, expected.prompt)
    && (!queuedReceipt.input || isDeepStrictEqual(queuedReceipt.input, original.input)),
  "server 原 Q、native 组合显示投影、正式 queued 回执、client 或完整纯文本 input 不一致");
  require(!queueRecoveryClientItems(beforeState.nativeTurns, expected.clientUserMessageId).length,
    "操作基线的原 Q 已经被接受");
  const active = beforeState.nativeTurns.filter(turn => queueRecoveryTurnId(turn) === expected.activeTurnId);
  const ended = consumedState.nativeTurns.filter(turn => queueRecoveryTurnId(turn) === expected.activeTurnId);
  require(active.length === 1 && active[0].status === "inProgress"
    && ended.length === 1 && ended[0].status === "completed" && !ended[0].error,
  "原 T 没有自然完成，不能借用停止、失败或另一轮");
  const accepted = queueRecoveryClientItems(consumedState.nativeTurns, expected.clientUserMessageId);
  require(accepted.length === 1 && accepted[0].accepted && accepted[0].item.type === "userMessage"
    && accepted[0].turn.status === "completed" && !accepted[0].turn.error
    && nonempty(accepted[0].item.id) && queueRecoveryTurnId(accepted[0].turn) !== expected.activeTurnId
    && isDeepStrictEqual(accepted[0].input, original.input)
    && consumedState.serverQueue.length === 0 && consumedState.nativeQueue.length === 0,
  "原 client 未唯一自然接受、完整 input 改变或仍有队列残留");
  const completedAt = ended[0].turnStartedAtMs + ended[0].durationMs;
  require(Number.isFinite(ended[0].turnStartedAtMs) && Number.isFinite(ended[0].durationMs)
    && Number.isFinite(accepted[0].turn.turnStartedAtMs)
    && accepted[0].turn.turnStartedAtMs >= completedAt, "原 Q 在原 T 自然结束前已经执行");
  const oldTurns = beforeState.nativeTurns.filter(turn => queueRecoveryTurnId(turn) !== expected.activeTurnId);
  require(consumedState.nativeTurns.length === beforeState.nativeTurns.length + 1
    && oldTurns.every(turn => isDeepStrictEqual(turn, consumedState.nativeTurns.find(value =>
      queueRecoveryTurnId(value) === queueRecoveryTurnId(turn)))), "自然消费改写旧历史或多出其他轮次");
  const originalRequest = queueRaceRequest(held, expected);
  if (expected.attemptedKind === "edit")
    require(nonempty(originalRequest.body.editId)
      && originalRequest.body.clientUserMessageId === expected.clientUserMessageId
      && Number.isFinite(native.createdAt) && originalRequest.body.createdAt === native.createdAt
      && originalRequest.body.originalText === expected.prompt,
    "原 begin POST 的 ED/client/createdAt/原文不属于该 Q");
  require(!originalRequest.trace.commands.some(command =>
    command.params?.requestId === originalRequest.pause.params.requestId)
    && !originalRequest.trace.events.some(event =>
      event.method === "Network.responseReceived" && event.params?.requestId === originalRequest.networkId),
  "旧 POST 在原 Q 消费前已经放行或收到回包");
  const times = [timestamp(before.capturedAt), originalRequest.event.params.wallTime * 1000,
    timestamp(held.capturedAt), timestamp(consumed.capturedAt)];
  require(times.every(Number.isFinite) && times.every((value, index) => !index || value > times[index - 1])
    && new Set([before.readId, consumed.readId]).size === 2,
  "原 Q 可见、旧 POST 暂缓与自然消费读取的顺序不成立");
  return { originalRequest, original, accepted: accepted[0], consumedState };
}

/** 只接受自然消费之后放行同一旧 POST 并明确过期；清理已接受残留不算阻止出队。 */
export function verifyQueueDequeueRaceFacts(data, expected) {
  const require = queueRaceRequirement;
  const { originalRequest, accepted, consumedState } = verifyQueueDequeueRaceConsumption(data, expected);
  const released = queueRaceRequest(data.released, expected);
  require(released.networkId === originalRequest.networkId
    && released.pause.params.requestId === originalRequest.pause.params.requestId
    && sameQueueRaceRequest(released.request, originalRequest.request)
    && sameQueueRaceRequest(released.pause.params.request, originalRequest.pause.params.request),
  "放行时换了原 POST/network ID/A 或改写请求");
  const controls = released.trace.commands.filter(command =>
    command.params?.requestId === originalRequest.pause.params.requestId);
  require(controls.length === 1 && controls[0].method === "Fetch.continueRequest"
    && Object.keys(controls[0].params).every(key => ["requestId", "interceptResponse"].includes(key))
    && (controls[0].params.interceptResponse === undefined || controls[0].params.interceptResponse === false)
    && controls[0].result && !controls[0].error
    && timestamp(controls[0].startedAt) > timestamp(data.consumed.capturedAt)
    && timestamp(controls[0].finishedAt) >= timestamp(controls[0].startedAt),
  "缺少自然消费后对同一旧 POST 的唯一无改写放行命令");
  const bodies = released.trace.commands.filter(command => command.method === "Network.getResponseBody"
    && command.params?.requestId === originalRequest.networkId);
  require(bodies.length === 1 && bodies[0].result?.base64Encoded === false
    && nonempty(bodies[0].result.body) && !bodies[0].error
    && timestamp(bodies[0].startedAt) >= timestamp(controls[0].finishedAt)
    && timestamp(bodies[0].finishedAt) >= timestamp(bodies[0].startedAt)
    && timestamp(data.released.capturedAt) >= timestamp(bodies[0].finishedAt),
  "缺少放行后原 network 请求的真实回包正文与读取时间");
  let result;
  try { result = JSON.parse(bodies[0].result.body); } catch {}
  require(result?.requestId === originalRequest.receiptId
    && ["failed", "rejected"].includes(result.state) && result.result?.ok === false
    && result.result.code === 409 && result.result.outcomeUnknown !== true
    && (result.result.threadId == null || result.result.threadId === expected.threadId)
    && (result.result.messageId == null || result.result.messageId === expected.queueId),
  "原 A 没有明确过期拒绝；unknown或成功清理残留均不能当作阻止出队");
  const responses = released.trace.events.filter(event => event.method === "Network.responseReceived"
    && event.params?.requestId === originalRequest.networkId);
  require(responses.length === 1 && [200, 409].includes(responses[0].params.response?.status),
    "原 POST 缺少对应的真实 HTTP 响应");
  const after = queueRaceSnapshot(data.after, "after", expected);
  require(isDeepStrictEqual(after.nativeTurns, consumedState.nativeTurns)
    && after.serverQueue.length === 0 && after.nativeQueue.length === 0
    && data.after.readId !== data.consumed.readId && data.after.readId !== data.before.readId
    && timestamp(data.released.capturedAt) >= timestamp(controls[0].finishedAt)
    && timestamp(data.after.capturedAt) > timestamp(data.released.capturedAt),
  "放行旧写后历史、空队列或终态读取顺序不成立");
  return { receiptId: originalRequest.receiptId, networkId: originalRequest.networkId,
    clientUserMessageId: expected.clientUserMessageId, turnId: queueRecoveryTurnId(accepted.turn) };
}

/** 自动出队必须核持久 userMessage.clientId；turn.params 可以没有输入。 */
export function verifyNaturalQueueHistory(history, { threadId, previousTurnId, entries }) {
  invariant(history?.threadId === threadId, "FIFO 原始历史属于其他会话");
  invariant(Array.isArray(history.turns) && Array.isArray(history.queuedMessages),
    "FIFO 缺少原始轮次或最终队列");
  invariant(history.isComplete === true, "FIFO 历史未完整，不能证明没有遗漏或重复消费");
  invariant(Array.isArray(entries) && entries.length >= 2, "FIFO 必须至少验收两条消息");
  invariant(new Set(entries.map(entry => entry.queueId)).size === entries.length
    && new Set(entries.map(entry => entry.clientUserMessageId)).size === entries.length,
  "FIFO 队列身份或逻辑消息身份重复");
  const turns = history.turns;
  const turnId = turn => turn.turnId || turn.id;
  const previous = turns.filter(turn => turnId(turn) === previousTurnId);
  invariant(previous.length === 1 && previous[0].status === "completed" && !previous[0].error,
    "FIFO 基础轮没有唯一成功终态");
  const timing = turn => {
    invariant(Number.isFinite(turn.turnStartedAtMs) && Number.isFinite(turn.durationMs)
      && turn.durationMs >= 0, "FIFO 原始轮次缺少有效起止时间");
    return { startedAtMs: turn.turnStartedAtMs, completedAtMs: turn.turnStartedAtMs + turn.durationMs };
  };
  let previousEnd = timing(previous[0]).completedAtMs;
  let previousIndex = turns.indexOf(previous[0]);
  const consumed = [];
  for (const entry of entries) {
    invariant(nonempty(entry.queueId) && nonempty(entry.clientUserMessageId)
      && nonempty(entry.prompt) && nonempty(entry.expectedReply), "FIFO 缺少原消息身份或固定预期");
    const matches = turns.flatMap(turn => (turn.items || [])
      .filter(item => item.type === "userMessage" && item.clientId === entry.clientUserMessageId)
      .map(item => ({ turn, item })));
    invariant(matches.length === 1, "FIFO 逻辑消息未被唯一消费");
    const { turn, item } = matches[0];
    const steered = turns.some(candidate => (candidate.items || []).some(other =>
      other.type === "steeringUserMessage" && (
        [other.clientId, other.clientUserMessageId, other.serverClientUserMessageId,
          other.restoreMessage?.clientUserMessageId,
          other.restoreMessage?.submissionOptions?.clientUserMessageId].includes(entry.clientUserMessageId)
        || other.restoreMessage?.id === entry.queueId
        || nonempty(item.id) && other.serverUserMessageId === item.id
      )));
    invariant(!steered, "FIFO 消息包含引导记录，不能作为自然承接通过");
    invariant(nonempty(turnId(turn)) && !turnId(turn).startsWith("pending-")
      && turnId(turn) !== previousTurnId
      && turns.filter(candidate => turnId(candidate) === turnId(turn)).length === 1
      && turn.status === "completed" && !turn.error, "FIFO 后续轮次身份或终态无效");
    // 原生普通文本输入会追加一个换行；不能 trim 掉用户原本的空行或空格。
    invariant(Array.isArray(item.content) && item.content.length === 1
      && item.content[0].type === "text"
      && [entry.prompt, `${entry.prompt}\n`].includes(item.content[0].text),
    "FIFO 持久用户输入与原排队正文不一致");
    invariant((turn.items || []).filter(item =>
      item.type === "agentMessage" && item.text === entry.expectedReply).length === 1,
    "FIFO 缺少同一消费轮的唯一准确回复");
    const times = timing(turn), index = turns.indexOf(turn);
    invariant(index > previousIndex && times.startedAtMs >= previousEnd,
      "FIFO 消费顺序错误，或前一轮完成前已抢先执行");
    consumed.push({ queueId: entry.queueId, clientUserMessageId: entry.clientUserMessageId,
      turnId: turnId(turn), ...times });
    previousIndex = index;
    previousEnd = times.completedAtMs;
  }
  invariant(history.queuedMessages.length === 0, "FIFO 完成后原生队列仍有消息");
  return { threadId, previousTurnId, consumed };
}

function validIdentity(name, value) {
  return nonempty(value) || receiptNames.has(name) && Number.isSafeInteger(value) && value >= 0;
}

/**
 * 问答回包丢失的固定业务契约。输入由下方文件门禁重新读取，不能用计数或 passed
 * 代替完整 HTTP 事件、同题原生状态、完整历史及客户端原回执记录。
 */
export function verifyQuestionReceiptRecoveryFacts(data, expected) {
  const { before, pendingInitial, pending, unknown, answered: answeredSnapshot, history, delivery } = data;
  const { threadId, controlThreadId, ask, answer, questionReloadStartedAt, reloadStartedAt,
    reloadedUiAt, recoveredUiAt, finishedUiAt } = expected;
  const require = (value, message) => invariant(value, `问答回执恢复：${message}`);
  require(nonempty(threadId) && nonempty(controlThreadId) && threadId !== controlThreadId,
    "缺少两个不同的测试会话");
  require(ask?.kind === "send" && answer?.kind === "answer"
    && ask.status === "confirmed" && answer.status === "confirmed"
    && actionThreadId(ask) === threadId && actionThreadId(answer) === threadId
    && nonempty(ask.prompt) && nonempty(ask.marker) && nonempty(ask.expected),
  "没有绑定唯一原提问与原回答动作");
  require([before, pendingInitial, pending, unknown, answeredSnapshot, history, delivery].every(value => value?.threadId === threadId),
    "阶段证据属于其他会话");
  require(pendingInitial.requestId === pending.requestId
    && pendingInitial.originatingTurnId === pending.originatingTurnId
    && isDeepStrictEqual(pendingInitial.nativeQuestionStates, pending.nativeQuestionStates),
  "未答题刷新后身份或逐题状态改变");
  const turnId = turn => turn.turnId || turn.id;
  const contentText = item => (item.content || []).filter(value => value.type === "text").map(value => value.text).join("");
  for (const value of [before, answeredSnapshot, history]) {
    require(value.historyRequest?.method === "thread/read"
      && value.historyRequest.params?.threadId === threadId && value.historyRequest.params?.includeTurns === true
      && Array.isArray(value.nativeTurns) && Array.isArray(value.queuedMessages) && value.queuedMessages.length === 0,
    "缺少 includeTurns=true 的完整原始历史或最终队列不为空");
    require(new Set(value.nativeTurns.map(turnId)).size === value.nativeTurns.length
      && value.nativeTurns.every(turn => nonempty(turnId(turn)) && !turnId(turn).startsWith("pending-")),
    "历史轮次身份重复或包含临时占位");
  }
  const old = before.nativeTurns, turns = history.nativeTurns;
  require(turns.length === old.length + 2 && isDeepStrictEqual(turns.slice(0, old.length), old),
    "旧完整历史被改写，或新增轮次不是唯一提问与唯一回答");
  const origin = turns[old.length], answered = turns[old.length + 1];
  require(isDeepStrictEqual(answeredSnapshot.nativeTurns, turns),
    "删除锁检查前没有独立读取到与终态一致的完整已完成回答");
  const originalTurnId = turnId(origin), answerTurnId = turnId(answered);
  require([origin, answered].every(turn => turn.status === "completed" && !turn.error)
    && answerTurnId === answer.result?.answerTurnId && originalTurnId === pending.originatingTurnId,
  "原问题或回答轮的身份/成功终态不匹配");
  const userMessages = turns.flatMap(turn => (turn.items || []).filter(item => item.type === "userMessage")
    .map(item => ({ turnId: turnId(turn), text: contentText(item) })));
  const asks = userMessages.filter(item => item.text.includes(ask.marker));
  require(asks.length === 1 && asks[0].turnId === originalTurnId
    && [ask.prompt, `${ask.prompt}\n`].includes(asks[0].text), "提问没有在完整历史中唯一出现或正文不一致");
  require((answered.items || []).filter(item => item.type === "agentMessage" && item.text === ask.expected).length === 1,
    "实际回答轮缺少唯一准确回复");
  require(nonempty(pending.requestId) && pending.requestId === answer.questionRequestId
    && originalTurnId === answer.originatingTurnId && Array.isArray(pending.nativeQuestionStates)
    && pending.nativeQuestionStates.length >= 2, "原题身份缺失或没有覆盖多题回答");
  const questions = pending.nativeQuestionStates;
  const ids = questions.map(value => value.state?.question?.id);
  require(ids.every(nonempty) && new Set(ids).size === ids.length
    && Array.isArray(answer.answers) && answer.answers.length === ids.length,
  "问题 ID 重复、缺失或回答数不一致");
  const expectedEntries = questions.map(value => {
    const question = value.state?.question;
    const supplied = answer.answers.filter(entry => entry.questionItemId === question?.id);
    require(value.key?.threadId === threadId && nonempty(value.key?.entityKey)
      && value.key.itemId === question?.id && value.sourceTurnId === originalTurnId
      && value.sourceTurnStatus === "completed" && value.state.turnId === originalTurnId
      && value.state.lastSubmission === null && value.state.isSkipped === false && value.state.isSubmitting === false
      && supplied.length === 1 && supplied[0].question === question.title && nonempty(supplied[0].answer),
    "提交前没有同一原题、原实体的 completed 未答事实");
    require(pending.requestId === JSON.stringify(["async-question", originalTurnId, question.sourceItemId]),
      "外层原生请求没有绑定准确 turn/item");
    return supplied[0];
  });
  require(Array.isArray(history.nativeQuestionStates) && history.nativeQuestionStates.length === questions.length,
    "回答后缺少准确逐题原生状态");
  for (const question of questions) {
    const states = history.nativeQuestionStates.filter(value => isDeepStrictEqual(value.key, question.key));
    const expectedAnswer = expectedEntries.find(value => value.questionItemId === question.state.question.id);
    require(states.length === 1 && states[0].sourceTurnId === originalTurnId
      && states[0].sourceTurnStatus === "completed" && states[0].state.turnId === originalTurnId
      && isDeepStrictEqual(states[0].state.question, question.state.question)
      && states[0].state.lastSubmission === expectedAnswer.answer
      && states[0].state.isSkipped === false && states[0].state.isSubmitting === false,
    "已接受答案串题、串实体、被跳过或仍在提交");
  }
  const replies = [];
  for (const item of userMessages) {
    for (const match of item.text.matchAll(/<send_user_message_question_reply>([\s\S]*?)<\/send_user_message_question_reply>/g)) {
      let entries;
      try { entries = JSON.parse(match[1]); } catch { require(false, "历史回答封套不是有效 JSON"); }
      if (Array.isArray(entries) && entries.some(entry => ids.includes(entry.questionItemId)))
        replies.push({ turnId: item.turnId, entries });
    }
  }
  require(replies.length === 1 && replies[0].turnId === answerTurnId
    && isDeepStrictEqual(replies[0].entries, expectedEntries), "完整历史中答案缺失、重复或与批准回答不符");
  for (const draft of [expected.mainDraft, expected.controlDraft])
    require(nonempty(draft) && !userMessages.some(value => value.text.includes(draft)), "普通草稿被发送或缺少原稿");

  const originalId = answer.result?.receiptId;
  require(nonempty(originalId) && Array.isArray(unknown.clientReceipts) && unknown.clientReceipts.length === 1
    && unknown.clientReceipts[0].id === originalId && unknown.clientReceipts[0].threadId === threadId
    && unknown.clientReceipts[0].requestId === pending.requestId && unknown.clientReceipts[0].state === "unknown",
  "没有实际观察到原回答的未知回执");
  require(nonempty(unknown.device?.id) && Number.isFinite(unknown.device?.createdAtMs)
    && unknown.clientReceipts[0].deviceId === unknown.device.id
    && isDeepStrictEqual(delivery.device, unknown.device), "恢复前后授权身份发生变化");
  require(Array.isArray(delivery.clientReceipts) && delivery.clientReceipts.length === 0,
    "查询成功后原回答锁仍残留");
  const receiptMatches = receipt => receipt?.requestId === originalId && receipt.state === "completed"
    && receipt.result?.ok === true && receipt.result.outcomeUnknown !== true
    && receipt.result.threadId === threadId && receipt.result.requestId === pending.requestId;
  require(receiptMatches(unknown.answerReceipt), "丢回包时原回答没有明确成功，不能验收成功回执恢复");
  require(delivery.eventsTruncated === false && Array.isArray(delivery.networkEvents)
    && Array.isArray(delivery.responseBodies)
    && timestamp(delivery.networkStartedAt) <= timestamp(ask.startedAt)
    && timestamp(delivery.networkFinishedAt) >= timestamp(finishedUiAt),
  "网络记录截断或未覆盖提交至最后界面核验，无法证明没有重答");
  const requests = delivery.networkEvents.filter(event => event.method === "Network.requestWillBeSent").map(event => event.params);
  require(requests.every(event => nonempty(event.requestId) && event.request
    && Number.isFinite(event.wallTime) && Number.isFinite(event.timestamp))
    && new Set(requests.map(event => event.requestId)).size === requests.length, "网络请求记录身份不完整或重复");
  const requestUrl = request => new URL(request.url);
  const answerPosts = requests.filter(event => event.request.method === "POST"
    && requestUrl(event.request).pathname === "/api/action/answer");
  require(answerPosts.length === 1, "完整观察期间发生零次或多次回答提交");
  const asksOnWire = requests.filter(event => event.request.method === "POST"
    && requestUrl(event.request).pathname === "/api/action/send");
  require(asksOnWire.length === 1, "原提问请求缺失或出现额外普通消息");
  let askBody;
  try { askBody = JSON.parse(asksOnWire[0].request.postData); } catch { require(false, "缺少原提问请求正文"); }
  require(requestUrl(asksOnWire[0].request).origin === expected.origin
    && askBody.threadId === threadId && askBody.prompt === ask.prompt
    && asksOnWire[0].wallTime * 1000 >= timestamp(ask.startedAt)
    && asksOnWire[0].wallTime * 1000 <= timestamp(pendingInitial.capturedAt),
  "原提问 HTTP 与批准消息或阶段不一致");
  require(!requests.some(event => event.request.method === "POST"
    && requestUrl(event.request).pathname.startsWith("/api/action/")
    && !["/api/action/send", "/api/action/answer", "/api/action/markRead"].includes(requestUrl(event.request).pathname)),
  "恢复期间出现额外业务写入，不能用新消息或重建会话解除原锁");
  const submitted = answerPosts[0];
  const headers = Object.fromEntries(Object.entries(submitted.request.headers || {}).map(([key, value]) => [key.toLowerCase(), value]));
  let body;
  try { body = JSON.parse(submitted.request.postData); } catch { require(false, "缺少原回答请求正文"); }
  const answers = Object.fromEntries(expectedEntries.map(entry => [entry.questionItemId, { answers: [entry.answer] }]));
  require(headers["idempotency-key"] === originalId && requestUrl(submitted.request).origin === expected.origin
    && body.threadId === threadId && body.requestId === pending.requestId
    && body.method === "remote/requestUserInputAsync" && isDeepStrictEqual(body.response, { answers })
    && submitted.wallTime * 1000 >= timestamp(answer.startedAt)
    && submitted.wallTime * 1000 <= timestamp(unknown.capturedAt),
  "真实 POST 没有绑定原回答 ID、原题和准确答案");
  require(delivery.networkEvents.some(event => event.method === "Network.loadingFailed"
    && event.params?.requestId === submitted.requestId && nonempty(event.params.errorText)),
  "未真实丢失原回答 HTTP 回包");
  const documents = requests.filter(event => event.type === "Document"
    && requestUrl(event.request).origin === expected.origin && event.wallTime * 1000 >= timestamp(reloadStartedAt));
  require(documents.length === 1 && nonempty(documents[0].loaderId)
    && documents[0].wallTime * 1000 <= timestamp(reloadedUiAt),
  "没有唯一的真实刷新新文档，或刷新后 UI 早于新文档");
  const pendingDocuments = requests.filter(event => event.type === "Document"
    && requestUrl(event.request).origin === expected.origin
    && event.wallTime * 1000 >= timestamp(questionReloadStartedAt)
    && event.wallTime * 1000 <= timestamp(pending.capturedAt));
  require(pendingDocuments.length === 1 && nonempty(pendingDocuments[0].loaderId)
    && pendingDocuments[0].loaderId !== documents[0].loaderId, "未答题阶段未发生独立的真实文档刷新");
  const reads = requests.filter(event => event.request.method === "GET"
    && requestUrl(event.request).origin === expected.origin
    && requestUrl(event.request).pathname === `/api/receipts/${encodeURIComponent(originalId)}`
    && event.loaderId === documents[0].loaderId && event.wallTime >= documents[0].wallTime);
  require(reads.some(event => {
    const responses = delivery.networkEvents.filter(value => value.method === "Network.responseReceived"
      && value.params?.requestId === event.requestId && value.params.response?.status === 200
      && value.params.response.url === event.request.url);
    const finished = delivery.networkEvents.filter(value => value.method === "Network.loadingFinished"
      && value.params?.requestId === event.requestId);
    const bodies = delivery.responseBodies.filter(value => value.requestId === event.requestId);
    if (responses.length !== 1 || finished.length !== 1 || bodies.length !== 1
      || typeof bodies[0].body?.body !== "string" || typeof bodies[0].body?.base64Encoded !== "boolean"
      || !Number.isFinite(responses[0].params.timestamp) || !Number.isFinite(finished[0].params.timestamp)
      || responses[0].params.timestamp < event.timestamp
      // Chromium 的响应事件取派发时刻，完成事件可携带更早的底层完成时刻。
      // 两者都须属于本请求，并在解锁 UI 之前出现；不能强制它们互相递增。
      || finished[0].params.timestamp < event.timestamp
      || (event.wallTime + Math.max(responses[0].params.timestamp, finished[0].params.timestamp)
        - event.timestamp) * 1000 > timestamp(recoveredUiAt)) return false;
    try {
      const raw = bodies[0].body;
      return receiptMatches(JSON.parse(raw.base64Encoded ? Buffer.from(raw.body, "base64").toString("utf8") : raw.body));
    } catch { return false; }
  }), "新文档没有实际 GET 同一原回执并收到匹配成功响应");
  return { originalReceiptId: originalId, originatingTurnId: originalTurnId, answerTurnId,
    questionIds: ids, answerSubmissions: answerPosts.length, recoveredReads: reads.length };
}

function hashBytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function sourceInputAllowed(path) {
  if (path.startsWith("scripts/remote-access-e2e") && path.endsWith(".test.mjs")) return false;
  if (/(^|\/)(?:node_modules|\.pnpm|target|dist|build|temp|test-results|\.git|\.idea|\.gradle|\.cxx|toolchains?|caches?)(\/|$)/i.test(path))
    return false;
  if (/(^|\/)(?:\.env(?:\..*)?|local\.properties|key(?:store)?\.properties|signing(?:[-.].*)?\.properties|secrets?(?:\..*)?|credentials?(?:\..*)?)$/i.test(path)
    || /\.(?:jks|keystore|p12|pfx|apk|aab|exe|class|log)$/i.test(path)) return false;
  if (path.startsWith("apps/codex-elves-android/")) {
    return /\/src\/main\//.test(path)
      || /(?:^|\/)(?:build\.gradle(?:\.kts)?|settings\.gradle(?:\.kts)?|gradle\.properties|version\.properties|gradlew(?:\.bat)?|gradle-wrapper\.properties|gradle-wrapper\.jar|proguard-rules\.pro|\.gitignore)$/.test(path);
  }
  return true;
}

/** 使用 Git 的 ignore 规则，包括尚未提交但未被忽略的新模块。 */
export async function listRemoteAccessSourceInputs({ root = repositoryRoot } = {}) {
  const { stdout } = await executeFile("git", [
    "-C", resolve(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ...inputRoots,
  ], { encoding: "utf8", maxBuffer: 16 * 1024 * 1024, windowsHide: true });
  const paths = [...new Set(stdout.split("\0").filter(Boolean).map(path => path.replaceAll("\\", "/")))]
    .filter(sourceInputAllowed).sort();
  for (const path of essentialInputs) invariant(paths.includes(path), `源码指纹缺少必需输入：${path}`);
  return paths;
}

export async function sourceFingerprint(options = {}) {
  const root = options.root || repositoryRoot;
  const hash = createHash("sha256").update(`remote-access-source:${REMOTE_ACCESS_EVIDENCE_VERSION}\0`);
  for (const path of await listRemoteAccessSourceInputs({ root })) {
    hash.update(path).update("\0").update(await readFile(resolve(root, path))).update("\0");
  }
  return hash.digest("hex");
}

/** 建立引用时即读取原始字节；sha256 不对 JSON 重新序列化后计算。 */
export async function createEvidenceReference(path, { baseDir } = {}) {
  invariant(nonempty(path), "证据路径不能为空");
  const absolutePath = resolve(baseDir || repositoryRoot, path);
  const metadata = await stat(absolutePath);
  invariant(metadata.isFile(), `证据不是文件：${absolutePath}`);
  const bytes = await readFile(absolutePath);
  return {
    path: baseDir ? relative(resolve(baseDir), absolutePath).split(sep).join("/") : absolutePath,
    sha256: hashBytes(bytes), size: bytes.length,
  };
}

/** 保留浏览器工具返回的原始图像格式，不把 JPEG 字节误标成 PNG。 */
export function screenshotFileType(bytes) {
  const starts = signature => signature.every((byte, index) => bytes?.[index] === byte);
  if (starts([137, 80, 78, 71, 13, 10, 26, 10]))
    return { extension: "png", mimeType: "image/png" };
  if (starts([255, 216, 255]) && bytes.length >= 4
      && bytes[bytes.length - 2] === 255 && bytes[bytes.length - 1] === 217)
    return { extension: "jpg", mimeType: "image/jpeg" };
  if (starts([82, 73, 70, 70]) && [87, 69, 66, 80].every((byte, index) => bytes?.[index + 8] === byte))
    return { extension: "webp", mimeType: "image/webp" };
  throw new Error("截图证据不是受支持的 PNG、JPEG 或 WebP 原始文件");
}

/** 不接受仅路径或 verified:true；调用方必须保存取证时的哈希。 */
export async function verifyEvidenceReference(reference, { baseDir = repositoryRoot } = {}) {
  invariant(reference && typeof reference === "object" && nonempty(reference.path), "证据引用缺少 path");
  invariant(hashPattern.test(reference.sha256 || ""), `证据引用缺少有效 SHA-256：${reference.path}`);
  const path = isAbsolute(reference.path) ? reference.path : resolve(baseDir, reference.path);
  const metadata = await stat(path);
  invariant(metadata.isFile(), `证据不是文件：${path}`);
  const bytes = await readFile(path);
  const sha256 = hashBytes(bytes);
  invariant(sha256 === reference.sha256.toLowerCase(), `证据文件 SHA-256 不匹配：${reference.path}`);
  if (reference.size !== undefined)
    invariant(Number.isSafeInteger(reference.size) && reference.size === bytes.length, `证据文件大小不匹配：${reference.path}`);
  invariant(bytes.length > 0, `证据文件为空：${reference.path}`);
  return { path, sha256, bytes };
}

export function jsonPointer(value, pointer) {
  invariant(typeof pointer === "string" && (pointer === "" || pointer.startsWith("/")), "原始字段必须使用 JSON Pointer");
  invariant(!/~(?:[^01]|$)/.test(pointer), `JSON Pointer 转义无效：${pointer}`);
  if (pointer === "") return value;
  let current = value;
  for (const encodedKey of pointer.slice(1).split("/")) {
    const key = encodedKey.replaceAll("~1", "/").replaceAll("~0", "~");
    invariant(current !== null && typeof current === "object" && Object.hasOwn(current, key),
      `原始证据没有 JSON Pointer 字段：${pointer}`);
    current = current[key];
  }
  return current;
}

function expectationMatches(actual, expectation) {
  const expected = expectation.value;
  switch (expectation.op) {
    case "equals": return isDeepStrictEqual(actual, expected);
    case "notEquals": return !isDeepStrictEqual(actual, expected);
    case "includes": return typeof actual === "string" && typeof expected === "string"
      ? expected.length > 0 && actual.includes(expected)
      : Array.isArray(actual) && actual.some(value => isDeepStrictEqual(value, expected));
    case "notIncludes": return typeof actual === "string" && typeof expected === "string"
      ? expected.length > 0 && !actual.includes(expected)
      : Array.isArray(actual) && !actual.some(value => isDeepStrictEqual(value, expected));
    case "oneOf": return Array.isArray(expected) && expected.length > 0
      && expected.some(value => isDeepStrictEqual(value, actual));
    default: throw new Error(`不支持的证据比较操作：${expectation.op}`);
  }
}

function verifyExpectations(factSets, expectations, label) {
  invariant(Array.isArray(expectations), `${label} expectations 必须是数组`);
  for (const expectation of expectations) {
    invariant(expectation && nonempty(expectation.fact) && Object.hasOwn(expectation, "value"),
      `${label} 存在无效的语义断言`);
    invariant(factSets.some(facts => Object.hasOwn(facts, expectation.fact)
      && expectationMatches(facts[expectation.fact], expectation)),
    `${label} 原始事实不满足 ${expectation.fact} ${expectation.op} ${JSON.stringify(expectation.value)}`);
  }
}

function verifyCorrelatedExpectations(factSets, expectations, label) {
  if (expectations.length === 0) return;
  const failures = [];
  // 同一份原始观察必须同时证明整组断言；不能跨会话或跨读取时刻凑字段。
  for (const facts of factSets) {
    try {
      verifyExpectations([facts], expectations, label);
      return;
    } catch (error) { failures.push(error.message); }
  }
  throw new Error(`${label} 不能跨会话或不同回执时刻拼接语义事实：${failures.join("；") || "没有原始事实"}`);
}

function inspectRawOutcomes(raw, path = "") {
  if (!raw || typeof raw !== "object") return [];
  const problems = [];
  for (const [name, value] of Object.entries(raw)) {
    const pointer = `${path}/${name.replaceAll("~", "~0").replaceAll("/", "~1")}`;
    if (name === "outcomeUnknown" && value === true) problems.push({ pointer, name, value });
    if (name === "receiptState" && !successfulReceiptStates.has(value) && value !== "accepted")
      problems.push({ pointer, name, value });
    if (["receiptSuccess", "success", "ok"].includes(name) && value === false)
      problems.push({ pointer, name, value });
    problems.push(...inspectRawOutcomes(value, pointer));
  }
  return problems;
}

function verifyActionOutcome(record, action, request) {
  const facts = record.facts;
  const outcome = rawOutcomes.get(record);
  invariant(outcome, `动作 ${action.id} 缺少明确的 outcomePointer 回执范围`);
  const allowedOutside = new Set([
    "runId", "caseId", "origin", "sourceFingerprint", "capturedAt", "timestamp", "submittedAt",
  ]);
  for (const [name, pointer] of Object.entries(outcome.pointers)) {
    if (allowedOutside.has(name)) continue;
    invariant(outcome.pointer === "" || pointer === outcome.pointer || pointer.startsWith(`${outcome.pointer}/`),
      `动作 ${action.id} 的 ${name} 字段不在指定回执范围内`);
  }
  invariant(identityNames.some(name => receiptNames.has(name)
    && outcome.pointers[name] !== undefined && validIdentity(name, facts[name])),
  `动作 ${action.id} 的回执范围缺少真实标识`);
  const expectedRaceRejection = ["queue-race-edit", "queue-race-remove"].includes(action.kind)
    && (facts.receiptState === "rejected" && facts.staleWriteAccepted === false
      || facts.receiptState === "failed" && facts.code === 409 && facts.outcomeUnknown !== true);
  const problems = outcome.problems.filter(problem =>
    !(expectedRaceRejection && (problem.name === "receiptState" && ["rejected", "failed"].includes(problem.value)
      || ["receiptSuccess", "success", "ok"].includes(problem.name) && problem.value === false)));
  invariant(problems.length === 0, `动作 ${action.id} 的原始回执结果未成功：${
    problems.map(problem => `${problem.pointer}=${JSON.stringify(problem.value)}`).join("；")}`);
  if (facts.receiptState !== undefined)
    invariant(successfulReceiptStates.has(facts.receiptState) || facts.receiptState === "accepted" || expectedRaceRejection,
      `动作 ${action.id} 的 receiptState 未成功`);
  for (const name of ["success", "receiptSuccess"]) {
    if (facts[name] !== undefined)
      invariant(facts[name] === true || expectedRaceRejection && facts[name] === false,
        `动作 ${action.id} 的原始 ${name} 未成功`);
  }
  const acknowledged = successfulReceiptStates.has(facts.receiptState)
    || facts.success === true || facts.receiptSuccess === true || expectedRaceRejection;
  if (["side-close", "close-side", "close-thread"].includes(action.kind))
    invariant(facts.closed === true, `动作 ${action.id} 缺少原生 closed=true 完成事实`);
  if (createKinds.has(action.kind)) {
    const concreteCreation = facts.created === true
      && (action.kind !== "create-send" || nonempty(facts.turnId));
    invariant(acknowledged || concreteCreation, `动作 ${action.id} 缺少成功创建回执或 created 完成事实`);
  }
  if (["send", "send-message", "submit-message"].includes(action.kind))
    invariant(acknowledged || nonempty(facts.turnId), `动作 ${action.id} 缺少成功发送回执或原生 turnId`);
  if (action.kind === "queue-send")
    invariant(acknowledged || nonempty(facts.queueId), `动作 ${action.id} 缺少成功排队回执或原生 queueId`);
  const ownCompletion = [
    ...actionComparisons(action, request), ...requestComparisons(request),
  ].filter(expectation => ![
    "threadId", "turnId", "parentThreadId", "receiptId", "readId", "eventId",
    "queueId", "removedQueueId", "previousTurnId", "nextTurnId", "origin", "sourceFingerprint", "operation",
  ].includes(expectation.fact));
  const concreteSend = ["send", "send-message", "submit-message"].includes(action.kind) && nonempty(facts.turnId);
  const concreteQueue = action.kind === "queue-send" && nonempty(facts.queueId);
  const concreteCreate = createKinds.has(action.kind)
    && facts.created === true && (action.kind !== "create-send" || nonempty(facts.turnId));
  invariant(acknowledged || concreteSend || concreteQueue || concreteCreate
    || ownCompletion.length > 0 && ownCompletion.every(expectation =>
      Object.hasOwn(facts, expectation.fact) && expectationMatches(facts[expectation.fact], expectation)),
  `动作 ${action.id} 缺少成功回执或同一原始观察中的完成事实：${ownCompletion.map(value => value.fact).join(", ")}`);
}

function expectedFactComparisons(expectedFacts = {}) {
  invariant(expectedFacts && typeof expectedFacts === "object" && !Array.isArray(expectedFacts), "expectedFacts 必须是对象");
  return Object.entries(expectedFacts).map(([fact, value]) => ({ fact, op: "equals", value }));
}

function requestComparisons(request) {
  const comparisons = [...expectedFactComparisons(request.expectedFacts), ...(request.expectations || [])];
  for (const name of ["parentThreadId", "pinned", "title", "deleted", "closed", "queueOrder", "removedQueueId",
    "previousTurnId", "nextTurnId", "origin", "sourceFingerprint"]) {
    if (request[name] !== undefined) comparisons.push({ fact: name, op: "equals", value: request[name] });
  }
  if (request.model) {
    comparisons.push({ fact: "modelId", op: "equals", value: request.model.id });
    comparisons.push({ fact: "protocol", op: "equals", value: request.model.protocol });
  }
  if (request.expectedReadState) {
    const expected = request.expectedReadState;
    for (const name of ["threadId", "turnId", "hasUnreadTurn", "parentThreadId", "parentSideUnread"])
      if (expected[name] !== undefined) comparisons.push({ fact: name, op: "equals", value: expected[name] });
    comparisons.push({ fact: "turnStatus", op: "equals", value: "completed" });
  }
  if (request.contractVersion && request.operation)
    comparisons.push({ fact: "operation", op: "equals", value: request.operation });
  return comparisons;
}

function timestamp(value) {
  return typeof value === "number" ? value : Date.parse(value);
}

function verifyFreshness(facts, request) {
  const lower = request.capturedAfter || request.runStartedAt;
  const value = facts.capturedAt ?? facts.timestamp ?? facts.submittedAt;
  if (lower !== undefined) invariant(value !== undefined, "原生证据缺少本轮取证时间的原始字段");
  if (value === undefined) return;
  const actualTime = timestamp(value);
  invariant(Number.isFinite(actualTime), "原始证据的取证时间无效");
  invariant(actualTime <= Date.now() + 5_000, "原始证据的取证时间在未来");
  if (lower !== undefined) {
    const lowerTime = timestamp(lower);
    invariant(Number.isFinite(lowerTime) && actualTime >= lowerTime, "原始证据早于本轮或当前动作");
  }
  if (request.capturedBefore !== undefined) {
    const upperTime = timestamp(request.capturedBefore);
    invariant(Number.isFinite(upperTime) && actualTime <= upperTime + 5_000, "原始证据晚于本轮取证范围");
  }
}

function actionThreadId(action) {
  return ["create-send", "side-create"].includes(action.kind)
    ? action.result?.threadId : action.threadId || action.result?.threadId;
}

function actionComparisons(action, request) {
  const comparisons = [];
  const queueId = action.queueId || action.result?.queueId;
  if (action.kind === "stop")
    comparisons.push({ fact: "turnStatus", op: "oneOf", value: ["interrupted", "cancelled", "canceled"] });
  if (action.kind === "pin") comparisons.push({ fact: "pinned", op: "equals", value: true });
  if (["side-close", "close-side", "close-thread"].includes(action.kind))
    comparisons.push({ fact: "closed", op: "equals", value: true });
  if (action.kind === "permanent-delete") comparisons.push({ fact: "deleted", op: "equals", value: true });
  if (action.kind === "rename") {
    const title = request.title ?? action.result?.title;
    invariant(nonempty(title), "改名动作缺少预期标题");
    comparisons.push({ fact: "title", op: "equals", value: title });
  }
  if (action.kind === "queue-remove")
    comparisons.push({ fact: "removedQueueId", op: "equals", value: action.queueId });
  if (["queue-send", "queue-edit-cancel", "queue-edit-save", "queue-reorder", "queue-steer"].includes(action.kind) && queueId)
    comparisons.push({ fact: "queueId", op: "equals", value: queueId });
  return comparisons;
}

/**
 * records: [{source,evidencePath,sha256,threadId,receiptId,outcomePointer:"",...,
 *   rawFacts:{threadId:"/result/threadId",receiptId:"/id",status:"/result/status"},
 *   expectations:[{fact:"status",op:"equals",value:"completed"}]}]
 * request.expectedFacts / request.expectations 来自 runner 固定契约；records 的断言只作附加校验。
 * 返回值中的 facts 始终重新从磁盘解析；不信任输入中的 facts / verified。
 */
export async function verifyNativeEvidence(records, request = {}, options = {}) {
  invariant(Array.isArray(records) && records.length > 0, "原生核验没有返回原始文件证据");
  const actions = request.actions || [];
  invariant(Array.isArray(actions), "原生核验 actions 必须是数组");
  if (request.caseId === "queue.steer") {
    const action = actions[0];
    invariant(actions.length === 1 && action?.kind === "queue-steer" && nonempty(action.turnId)
      && action.result?.turnId === action.turnId,
    "引导固定用例缺少点击前的活动轮，或操作后轮次与其不同");
    invariant(request.turnIds?.length === 1 && request.turnIds[0] === action.turnId
      && request.expectedFacts?.turnId === action.turnId,
    "引导固定用例缺少原始回执与操作前活动轮的绑定");
    invariant(request.queueSteerBefore, "引导固定用例缺少点击前原始观察文件");
    const artifact = await verifyEvidenceReference(request.queueSteerBefore, options);
    const before = JSON.parse(artifact.bytes.toString("utf8").replace(/^\uFEFF/, ""));
    const active = (before.turns || []).filter(turn => turn.processing === true
      && nonempty(turn.id) && !turn.id.startsWith("pending-"));
    invariant(before.threadId === action.threadId && before.loading === false
      && active.length === 1 && active[0].id === action.turnId
      && before.queue?.some(row => row.id === action.queueId && row.steerEnabled === true),
    "引导固定用例的原始观察没有对应的唯一活动轮和可引导队列");
    invariant(Number.isFinite(timestamp(before.at)) && Number.isFinite(timestamp(action.startedAt))
      && timestamp(before.at) <= timestamp(action.startedAt),
    "引导固定用例的原始观察必须采集于操作开始前");
    verifyFreshness({ capturedAt: before.at }, request);
  }
  if (request.caseId === "queue.automatic" || request.requiresNaturalQueue === true) {
    invariant(request.naturalQueue && Array.isArray(request.naturalQueue.entries)
      && request.naturalQueue.entries.length === 2, "FIFO 固定用例缺少两条消息的专项契约");
    invariant(actions.length === 3 && actions.filter(action => action.kind === "send").length === 1
      && actions.filter(action => action.kind === "queue-send").length === 2,
    "FIFO 固定用例必须只有基础发送与两条原入队动作，不能夹带引导或改队列");
    const queueActions = actions.filter(action => action.kind === "queue-send");
    invariant(new Set(request.naturalQueue.entries.map(entry => entry.actionId)).size === 2
      && request.naturalQueue.entries.every(entry => queueActions.some(action =>
        action.id === entry.actionId && (action.queueId || action.result?.queueId) === entry.queueId)),
    "FIFO 专项契约没有绑定两条原入队动作");
  }
  const mandatory = requestComparisons(request);
  invariant(actions.length || request.expectedReadState || request.defaultSideTabTitle === true || mandatory.some(value =>
    !["origin", "sourceFingerprint", "threadId", "turnId"].includes(value.fact)),
  "原生核验缺少由 runner 定义的动作或预期语义");
  const verified = [];
  for (const record of records) {
    invariant(record && sources.has(record.source), "原生证据来源无效");
    const artifact = await verifyEvidenceReference({
      path: record.evidencePath || record.path, sha256: record.sha256, size: record.size,
    }, options);
    let raw;
    try { raw = JSON.parse(artifact.bytes.toString("utf8").replace(/^\uFEFF/, "")); }
    catch (error) { throw new Error(`原生证据不是有效 JSON：${artifact.path}；${error.message}`); }
    const pointers = { ...(record.readStatePointers || {}), ...(record.rawFacts || {}) };
    invariant(Object.keys(pointers).length > 0, "原生证据缺少 rawFacts JSON Pointer");
    const facts = Object.fromEntries(Object.entries(pointers).map(([name, pointer]) => [name, jsonPointer(raw, pointer)]));
    if (request.operation !== "runtime-version" || record.threadId !== undefined || facts.threadId !== undefined)
      invariant(nonempty(record.threadId) && facts.threadId === record.threadId, "原始原生证据的 threadId 与记录不匹配");
    for (const name of identityNames) {
      if (record[name] !== undefined)
        invariant(validIdentity(name, record[name]) && facts[name] === record[name], `原始原生证据的 ${name} 与记录不匹配`);
    }
    invariant([...receiptNames].some(name => validIdentity(name, record[name]) && facts[name] === record[name]),
      "原生证据缺少原始 receiptId/readId/eventId 标识及 JSON Pointer");
    for (const name of ["runId", "caseId"]) {
      if (facts[name] !== undefined && request[name] !== undefined)
        invariant(facts[name] === request[name], `原生证据属于其他 ${name}`);
    }
    for (const name of ["origin", "sourceFingerprint"]) {
      if (request[name] !== undefined)
        invariant(facts[name] === request[name], `原始证据缺少或不匹配本轮 ${name}`);
    }
    if (request.threadIds?.length)
      invariant(request.threadIds.includes(facts.threadId), "原生证据对应其他会话");
    if (request.turnIds?.length && facts.turnId !== undefined)
      invariant(request.turnIds.includes(facts.turnId), "原生证据对应其他轮次");
    if (Array.isArray(facts.parentSideUnread) && request.expectedReadState?.parentThreadId) {
      facts.parentSideUnread = facts.parentSideUnread.some(value =>
        value?.id === request.expectedReadState.threadId
        && value.parentThreadId === request.expectedReadState.parentThreadId);
    }
    verifyFreshness(facts, request);
    if (record.expectations !== undefined) verifyExpectations([facts], record.expectations, "记录附加断言");
    const normalized = { ...record, evidencePath: artifact.path, sha256: artifact.sha256, facts };
    if (record.outcomePointer !== undefined) {
      const scope = jsonPointer(raw, record.outcomePointer);
      invariant(scope && typeof scope === "object" && !Array.isArray(scope), "outcomePointer 必须指向明确的回执对象");
      rawOutcomes.set(normalized, {
        pointer: record.outcomePointer, pointers, problems: inspectRawOutcomes(scope, record.outcomePointer),
      });
    }
    verified.push(normalized);
  }
  for (const action of actions) {
    invariant(nonempty(action.id) && (action.status === undefined || action.status === "confirmed"),
      "原生核验遇到未确认的动作");
    const matching = verified.filter(record => record.actionId === action.id);
    invariant(matching.length > 0, `缺少动作的原始证据：${action.id}`);
    const expectedThreadId = actionThreadId(action);
    invariant(matching.some(record => !expectedThreadId || record.facts.threadId === expectedThreadId),
      `动作证据对应其他会话：${action.id}`);
    for (const record of matching) {
      verifyActionOutcome(record, action, request);
      if (expectedThreadId) invariant(record.facts.threadId === expectedThreadId, `动作证据对应其他会话：${action.id}`);
      if (record.facts.actionId !== undefined)
        invariant(record.facts.actionId === action.id, `原始 actionId 不匹配：${action.id}`);
      if (record.facts.operation !== undefined || record.facts.actionId === undefined) {
        const aliases = operations[action.kind] || [action.kind];
        invariant(aliases.includes(record.facts.operation), `原始 operation 不属于 ${action.kind} 动作：${action.id}`);
      }
      const expectedTurnId = action.turnId || action.result?.turnId;
      if (expectedTurnId) invariant(record.facts.turnId === expectedTurnId, `动作证据对应其他轮次：${action.id}`);
      const expectedReceiptId = action.receiptId ?? action.result?.receiptId;
      if (expectedReceiptId !== undefined)
        invariant(record.facts.receiptId === expectedReceiptId, `动作证据的原始回执不匹配：${action.id}`);
    }
    verifyCorrelatedExpectations(matching.map(record => record.facts), actionComparisons(action, request), `动作 ${action.id}`);
  }
  for (const threadId of request.threadIds || [])
    invariant(verified.some(record => record.facts.threadId === threadId), `缺少会话的原始证据：${threadId}`);
  for (const turnId of request.turnIds || [])
    invariant(verified.some(record => record.facts.turnId === turnId), `缺少轮次的原始证据：${turnId}`);
  verifyCorrelatedExpectations(verified.map(record => record.facts), mandatory, "runner 固定契约");
  if (request.defaultSideTabTitle === true) {
    invariant(verified.some(record => record.facts.nativeTabThreadId === record.facts.threadId
      && nonempty(record.facts.nativeTabTitle)),
    "原始 nativeTabThreadId/nativeTabTitle 未证明同一侧边标签的默认名称");
  }
  if (request.naturalQueue) {
    const { threadId, previousTurnId, entries } = request.naturalQueue;
    invariant(Array.isArray(entries) && entries.length >= 2, "FIFO runner 没有登记两条原消息");
    const bound = entries.map(entry => {
      const receipts = verified.filter(record => record.actionId === entry.actionId
        && record.facts.threadId === threadId
        && record.facts.queueId === entry.queueId
        && record.facts.deliveryState === "queued"
        && nonempty(record.facts.clientUserMessageId));
      invariant(receipts.length === 1, "FIFO 缺少唯一原排队回执及 clientUserMessageId");
      return { ...entry, clientUserMessageId: receipts[0].facts.clientUserMessageId };
    });
    const snapshots = verified.filter(record => record.facts.threadId === threadId
      && Array.isArray(record.facts.nativeTurns) && Array.isArray(record.facts.queuedMessages));
    invariant(snapshots.length === 1, "FIFO 必须提供一份完整的同会话终态历史和队列原始快照");
    verifyNaturalQueueHistory({
      threadId, turns: snapshots[0].facts.nativeTurns, queuedMessages: snapshots[0].facts.queuedMessages,
      isComplete: snapshots[0].facts.historyComplete,
    }, { threadId, previousTurnId, entries: bound });
  }
  return verified;
}

/** 版本信息来自原始文件，安装资产本身也必须能读取并重新核对。 */
export async function verifyRuntimeEvidence(evidence, request = {}, options = {}) {
  invariant(evidence && typeof evidence === "object", "缺少运行版本证据");
  const expectedFacts = {};
  for (const name of ["managerVersion", "managerExecutableSha256", "codexVersion",
    "remoteAssetSha256", "adapterVersion"]) {
    invariant(evidence[name] !== undefined && evidence[name] !== null && evidence[name] !== "", `运行版本证据缺少 ${name}`);
    expectedFacts[name] = evidence[name];
  }
  invariant(hashPattern.test(evidence.managerExecutableSha256) && hashPattern.test(evidence.remoteAssetSha256),
    "运行资产 SHA-256 格式无效");
  const manager = await verifyEvidenceReference(evidence.managerExecutable, options);
  const remoteAsset = await verifyEvidenceReference(evidence.remoteAsset, options);
  const browserAsset = await verifyEvidenceReference(evidence.browserAsset, options);
  const browserCapture = await verifyEvidenceReference(evidence.browserAssetEvidence, options);
  const loaded = JSON.parse(browserCapture.bytes.toString("utf8").replace(/^\uFEFF/, ""));
  invariant(loaded.capture?.origin === request.origin
    && loaded.capture?.sourceFingerprint === request.sourceFingerprint
    && loaded.capture?.runId === request.runId && loaded.capture?.caseId === request.caseId,
  "浏览器实际资源证据不属于本轮来源、案例或源码");
  const capturedAt = timestamp(loaded.capture?.capturedAt);
  invariant(Number.isFinite(capturedAt) && capturedAt >= timestamp(request.runStartedAt)
    && capturedAt <= Date.now() + 5000
    && (request.capturedBefore === undefined || capturedAt <= timestamp(request.capturedBefore)),
  "浏览器实际资源取证时间无效");
  const browserRequest = loaded.request?.params, browserResponse = loaded.response?.params;
  invariant(loaded.request?.method === "Network.requestWillBeSent"
    && loaded.response?.method === "Network.responseReceived"
    && nonempty(browserRequest?.requestId) && browserRequest.requestId === browserResponse?.requestId
    && browserRequest.type === "Script" && browserResponse.type === "Script",
  "缺少同一浏览器脚本请求的原始请求及响应，预取或另行下载不能代替实际加载");
  const assetUrl = new URL(browserRequest.request?.url);
  invariant(assetUrl.origin === request.origin && assetUrl.pathname === "/app.js"
    && browserResponse.response?.url === assetUrl.href,
  "浏览器实际脚本来源或响应 URL 不匹配");
  invariant(browserResponse.response.status >= 200 && browserResponse.response.status < 300
    || browserResponse.response.status === 304, "浏览器实际脚本请求未成功");
  invariant(typeof loaded.body?.body === "string" && typeof loaded.body?.base64Encoded === "boolean",
    "浏览器原始脚本响应缺少实际内容");
  const loadedBytes = Buffer.from(loaded.body.body, loaded.body.base64Encoded ? "base64" : "utf8");
  invariant(hashBytes(loadedBytes) === browserAsset.sha256,
    "浏览器原始响应内容与保存的实际脚本不匹配");
  invariant(browserAsset.sha256 === remoteAsset.sha256,
    "浏览器仍在使用不同版本网页资源，不能用服务端或源码文件代替实际执行资源");
  invariant(manager.sha256 === evidence.managerExecutableSha256.toLowerCase(), "实际 manager 安装文件与版本证据不匹配");
  invariant(remoteAsset.sha256 === evidence.remoteAssetSha256.toLowerCase(), "实际网页资产与版本证据不匹配");
  invariant(Array.isArray(evidence.evidencePaths) && evidence.evidencePaths.length > 0, "缺少运行版本原始文件引用");
  for (const reference of evidence.evidencePaths) await verifyEvidenceReference(reference, options);
  return verifyNativeEvidence(evidence.records, {
    ...request, expectedFacts,
    expectations: [...(request.expectations || []), ...expectedFactComparisons(request.expectedFacts)],
    operation: "runtime-version",
  }, options);
}

function caseRequest(report, entry, definition, caseContracts) {
  const contract = caseContracts?.[entry.id] || definition.nativeContract || {};
  const actions = Object.values(report.actions || {}).filter(action => action.caseId === entry.id);
  const stageRequests = (entry.evidence || []).filter(value =>
    value.kind === "native" || value.kind === "driver").map(value => value.request).filter(Boolean);
  const request = {
    ...entry.nativeExpectations, ...contract,
    runId: report.runId, caseId: entry.id, origin: report.origin,
    sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt || report.startedAt,
    capturedBefore: report.finishedAt || report.updatedAt,
    actions,
    expectedFacts: { ...entry.nativeExpectations?.expectedFacts, ...contract.expectedFacts },
    expectations: [...(entry.nativeExpectations?.expectations || []), ...(contract.expectations || [])],
  };
  // 一个案例可在两个会话上依次核验；最后阶段不能覆盖前一阶段的身份范围。
  const threadIds = [...new Set([
    ...(request.threadIds || []), ...stageRequests.flatMap(value => value.threadIds || []),
    ...actions.map(actionThreadId).filter(nonempty),
  ])];
  const turnIds = [...new Set([
    ...(request.turnIds || []), ...stageRequests.flatMap(value => value.turnIds || []),
    ...actions.map(action => action.turnId || action.result?.turnId).filter(nonempty),
  ])];
  if (threadIds.length) request.threadIds = threadIds;
  if (turnIds.length) request.turnIds = turnIds;
  return request;
}

async function verifyUiEvidence(evidence, options) {
  const dom = await verifyEvidenceReference(evidence.files?.dom, options);
  const screenshot = await verifyEvidenceReference(evidence.files?.screenshot, options);
  invariant(dom.bytes.toString("utf8").trim().length > 0, "DOM 证据为空");
  screenshotFileType(screenshot.bytes);
  invariant(evidence.observation && typeof evidence.observation === "object", "UI 证据缺少实际 DOM 观察");
  if (evidence.threadId !== undefined && evidence.observation.threadId !== undefined)
    invariant(evidence.threadId === evidence.observation.threadId, "UI 证据会话身份不一致");
  return { dom, screenshot };
}

async function verifyDriverUiEvidence(evidence, options) {
  const dom = await verifyEvidenceReference(evidence.dom, options);
  const screenshot = await verifyEvidenceReference(evidence.screenshot, options);
  invariant(dom.bytes.toString("utf8").trim().length > 0, "外部驱动的 DOM/可访问性树为空");
  screenshotFileType(screenshot.bytes);
}

const sideReadKey = record => JSON.stringify([record.sha256, record.threadId,
  record.receiptId, record.readId, record.eventId]);

/** contenteditable 的实际值只从保存的 observation/editor JSON 读取，不要求DOM摘要包含它。 */
export async function verifyQueueEditCancelStage(stage, request, options = {}) {
  const { threadId, controlThreadId, queueAction, cancelAction } = request;
  const require = (value, message) => invariant(value, `队列取消：${message}`);
  require(stage?.kind === "queue-edit-cancel" && stage.threadId === threadId
    && nonempty(controlThreadId) && controlThreadId !== threadId && stage.controlThreadId === controlThreadId
    && queueAction?.kind === "queue-send" && queueAction.status === "confirmed"
    && cancelAction?.kind === "queue-edit-cancel" && cancelAction.status === "confirmed"
    && stage.queueActionId === queueAction.id && stage.cancelActionId === cancelAction.id
    && actionThreadId(queueAction) === threadId && actionThreadId(cancelAction) === threadId
    && stage.queueId === (queueAction.queueId || queueAction.result?.queueId)
    && stage.queueId === cancelAction.queueId && stage.editId === cancelAction.editId,
  "缺少同一原队列、ED与两个准确动作");
  const data = {};
  for (const phase of ["before", "editing", "after"]) {
    const [record] = await verifyNativeEvidence([stage[phase]], {
      runId: request.runId, caseId: request.caseId, origin: request.origin,
      sourceFingerprint: request.sourceFingerprint, runStartedAt: request.runStartedAt,
      capturedBefore: request.capturedBefore, actions: [], threadIds: [threadId],
      expectedFacts: { operation: "observe-queue-edit-cancel", phase },
    }, options);
    data[phase] = record.facts;
  }
  require(new Set(["before", "editing", "after"].map(phase => sideReadKey(stage[phase]))).size === 3,
    "复用了原生阶段观察");
  const ui = {};
  for (const name of ["controlBeforeUi", "beforeUi", "beforeCancelUi", "afterUi", "controlAfterUi", "returnedUi"]) {
    const capture = stage[name];
    await verifyUiEvidence(capture, options);
    const artifact = await verifyEvidenceReference(capture.files?.observation, options);
    const actual = JSON.parse(artifact.bytes.toString("utf8").replace(/^\uFEFF/, ""));
    require(isDeepStrictEqual(actual, capture.observation), "UI观察与原始JSON文件不一致");
    verifyFreshness({ capturedAt: actual.at }, request);
    require(actual.threadId === (name.startsWith("control") ? controlThreadId : threadId)
      && actual.loading === false && actual.promptEditable === true, "UI串会话、尚未加载完成或编辑器不可用");
    ui[name] = actual;
  }
  const editorFile = await verifyEvidenceReference(stage.beforeCancelEditor, options);
  const editor = JSON.parse(editorFile.bytes.toString("utf8").replace(/^\uFEFF/, ""));
  require(nonempty(queueAction.prompt)
    && editor.threadId === threadId && editor.editId === stage.editId
    && editor.capturedAt === ui.beforeCancelUi.at && editor.value === ui.beforeCancelUi.prompt
    && editor.value === `${queueAction.prompt}\n取消编辑的临时文字`
    && editor.value !== queueAction.prompt && ui.beforeCancelUi.editing === true,
  "取消前没有实际不同改稿、准确ED或编辑器实际值");
  const mainDraft = ui.beforeUi.prompt, controlDraft = ui.controlBeforeUi.prompt;
  require(nonempty(mainDraft) && nonempty(controlDraft) && mainDraft !== controlDraft
    && [mainDraft, controlDraft].every(value => value.includes(`[remote-e2e:${request.runId}:`))
    && ui.beforeUi.editing === false && ui.controlBeforeUi.editing === false
    && mainDraft !== editor.value && controlDraft !== editor.value, "缺少两个独立的本轮原普通草稿");
  for (const name of ["afterUi", "returnedUi", "controlAfterUi"]) {
    require(ui[name].prompt === (name.startsWith("control") ? controlDraft : mainDraft)
      && ui[name].editing === false && !ui[name].pendingText
      && ui[name].send?.action === "send" && ui[name].send.enabled === true,
    "取消后普通草稿、无锁或加载完成后的可发送状态未恢复");
  }
  const facts = verifyQueueEditCancelFacts(data, {
    threadId, queueId: stage.queueId, editId: stage.editId,
  });
  const original = data.before.queuedMessages.find(value => value.id === stage.queueId);
  const originalText = original.input.filter(value => value.type === "text").map(value => value.text).join("");
  require([queueAction.prompt, `${queueAction.prompt}\n`].includes(originalText),
    "原入队动作正文与取出前真实input不一致");
  require(cancelAction.result?.receiptId === data.after.cancelReceipt.requestId
    && cancelAction.result?.queueId === facts.queueId, "取消动作未绑定原回执或恢复后的物理对象");
  const times = [ui.controlBeforeUi.at, ui.beforeUi.at, data.before.capturedAt, cancelAction.startedAt,
    data.editing.capturedAt, editor.capturedAt, stage.cancelStartedAt, ui.afterUi.at,
    data.after.capturedAt, cancelAction.finishedAt, ui.controlAfterUi.at, ui.returnedUi.at].map(timestamp);
  require(times.every(Number.isFinite) && times.every((value, index) => !index || value >= times[index - 1]),
    "阶段时间倒退，或取消前使用了取消后的编辑器观察");
  return facts;
}

async function verifyQueueEditCancel(entry, report, driverStages, options) {
  const stages = entry.evidence.filter(value => value.kind === "queue-edit-cancel");
  invariant(stages.length === 1, "队列取消固定用例缺少唯一专项记录，不能删除契约后通过");
  const stage = stages[0], main = report.threads?.main, control = report.threads?.control;
  invariant(main?.runId === report.runId && control?.runId === report.runId
    && stage.threadId === main.id && stage.controlThreadId === control.id, "队列取消缺少本轮主/控制会话归属");
  const cancel = report.actions?.[stage.cancelActionId], queued = report.actions?.[stage.queueActionId];
  invariant(cancel?.caseId === entry.id && queued?.caseId === entry.id
    && Object.values(report.actions || {}).filter(value =>
      value.caseId === entry.id && value.kind === "queue-edit-cancel").length === 1,
  "队列取消重复派发或借用旧动作");
  for (const phase of ["before", "editing", "after"])
    invariant(driverStages.some(({ step, records }) => step.driver === "native"
      && step.operation === "observe-queue-edit-cancel" && step.request.payload?.phase === phase
      && records.some(record => sideReadKey(record) === sideReadKey(stage[phase]))),
    "队列取消没有关联本阶段真实原生读取");
  return verifyQueueEditCancelStage(stage, {
    runId: report.runId, caseId: entry.id, origin: report.origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt || report.startedAt, capturedBefore: report.finishedAt,
    threadId: main.id, controlThreadId: control.id, queueAction: queued, cancelAction: cancel,
  }, options);
}

/** 停止/失败队列固定案例必须绑定三次真实原生读取和准确回执。 */
export async function verifyQueueRecoveryStage(stage, request, options = {}) {
  const require = (value, message) => invariant(value, `停止／失败队列恢复：${message}`);
  const { report, driverStages, nativeRecords } = request;
  const expectedTrigger = request.caseId === "queue.after_stop" ? "stop"
    : request.caseId === "queue.after_failure" ? "failure" : null;
  const queueAction = report.actions?.[stage?.queueActionId];
  const triggerAction = report.actions?.[stage?.triggerActionId];
  const threadId = stage?.threadId;
  require(stage?.kind === "queue-recovery"
    && expectedTrigger !== null && stage.trigger === expectedTrigger
    && nonempty(threadId) && nonempty(stage.activeTurnId)
    && nonempty(stage.queueId) && nonempty(stage.clientUserMessageId)
    && nonempty(stage.prompt) && nonempty(stage.expectedReply)
    && queueAction?.caseId === stage.caseId && queueAction.kind === "queue-send"
    && queueAction.status === "confirmed" && actionThreadId(queueAction) === threadId
    && (queueAction.queueId || queueAction.result?.queueId) === stage.queueId
    && queueAction.prompt === stage.prompt,
  "缺少本轮唯一原 queue-send、client、完整消息或触发轮");
  const queueActions = Object.values(report.actions || {}).filter(action =>
    action.caseId === request.caseId && action.kind === "queue-send");
  require(queueActions.length === 1 && queueActions[0] === queueAction,
    "停止／失败案例必须只有一次原 queue-send，不能重复发送或借用旧消息");
  require(stage.caseId === request.caseId && stage.threadId === request.threadId
    && stage.clientUserMessageId.trim(), "专项记录与报告案例/会话身份不匹配");

  const phaseRecords = {};
  for (const phase of ["before", "transition", "terminal"]) {
    const candidates = driverStages.filter(({ step }) => step.driver === "native"
      && step.operation === "observe-queue-boundary"
      && step.request.threadId === threadId
      && step.request.payload?.phase === phase);
    require(candidates.length === 1 && candidates[0].records.length === 1,
      `缺少唯一 ${phase} 阶段的原生队列完整读取`);
    const [verified] = await verifyNativeEvidence(candidates[0].records, {
      runId: report.runId, caseId: request.caseId, origin: report.origin,
      sourceFingerprint: report.sourceFingerprint,
      runStartedAt: report.runStartedAt || report.startedAt, capturedBefore: report.finishedAt,
      actions: [], threadIds: [threadId], expectedFacts: {
        operation: "observe-queue-boundary", phase,
      },
    }, options);
    phaseRecords[phase] = verified;
  }
  require(new Set(["before", "transition", "terminal"].map(phase => sideReadKey(phaseRecords[phase]))).size === 3,
    "停止／失败前后复用了相同原生读取");
  const phaseTimes = ["before", "transition", "terminal"].map(phase =>
    timestamp(phaseRecords[phase].facts.capturedAt));
  require(Number.isFinite(timestamp(queueAction.finishedAt))
    && timestamp(queueAction.finishedAt) <= phaseTimes[0]
    && phaseTimes[0] < phaseTimes[1] && phaseTimes[1] < phaseTimes[2],
  "原 queue-send、停止／失败转场和终态读取的时间顺序不成立");

  const queueReceipts = nativeRecords.filter(record => record.actionId === queueAction.id
    && record.facts.threadId === threadId && record.facts.queueId === stage.queueId
    && record.facts.clientUserMessageId === stage.clientUserMessageId
    && record.facts.deliveryState === "queued");
  require(queueReceipts.length === 1, "原 Q 缺少唯一正式 queued 回执及 clientUserMessageId");

  const expected = {
    kind: stage.trigger,
    threadId,
    activeTurnId: stage.activeTurnId,
    queueId: stage.queueId,
    clientUserMessageId: stage.clientUserMessageId,
    queueActionId: queueAction.id,
    prompt: stage.prompt,
    expectedReply: stage.expectedReply,
  };
  const facts = verifyQueueRecoveryTerminal({
    before: phaseRecords.before.facts,
    transition: phaseRecords.transition.facts,
    terminal: phaseRecords.terminal.facts,
    receipt: queueReceipts[0],
  }, { ...expected, outcome: stage.outcome });
  require(facts.outcome === stage.outcome, "执行报告的自动/显式恢复分支与原生事实不一致");

  if (stage.trigger === "stop") {
    require(triggerAction?.caseId === request.caseId && triggerAction.kind === "stop"
      && triggerAction.status === "confirmed" && triggerAction.threadId === threadId
      && triggerAction.turnId === stage.activeTurnId
      && timestamp(triggerAction.startedAt) >= phaseTimes[0]
      && timestamp(triggerAction.startedAt) <= phaseTimes[1],
    "停止动作没有绑定点击前的准确活动轮");
    require(Object.values(report.actions || {}).filter(action =>
      action.caseId === request.caseId && action.kind === "stop").length === 1,
    "停止案例必须只有一次绑定原活动轮的 stop 操作");
    const stopReceipts = nativeRecords.filter(record => record.actionId === triggerAction.id
      && record.facts.threadId === threadId && record.facts.turnId === stage.activeTurnId
      && ["interrupted", "cancelled", "canceled"].includes(record.facts.turnStatus));
    require(stopReceipts.length === 1, "原停止动作缺少同一活动轮的原生终态回执");
  } else {
    require(stage.triggerActionId === null && !Object.values(report.actions || {}).some(action =>
      action.caseId === request.caseId && action.kind === "stop"),
    "失败案例不能夹带停止动作或借用 stop 分支的回执");
    const failures = driverStages.filter(({ step, records }) => step.driver === "environment"
      && step.operation === "fail-active-upstream"
      && records.some(record => record.facts.threadId === threadId
        && record.facts.affectedTurnId === stage.activeTurnId
        && record.facts.realUpstreamFailure === true));
    require(failures.length === 1, "真实上游失败证据没有绑定原活动轮");
    const failureTime = timestamp(failures[0].records.find(record =>
      record.facts.threadId === threadId && record.facts.affectedTurnId === stage.activeTurnId
      && record.facts.realUpstreamFailure === true).facts.capturedAt);
    require(Number.isFinite(failureTime) && phaseTimes[0] <= failureTime && failureTime <= phaseTimes[1],
      "真实上游失败发生时间不在原队列基线与失败后读取之间");
  }

  const recoveryActions = Object.values(report.actions || {}).filter(action =>
    action.caseId === request.caseId && action.kind === "queue-steer"
    && action.threadId === threadId && action.queueId === stage.queueId);
  if (stage.outcome === "automatically-accepted") {
    require(stage.recoveryActionId === null && recoveryActions.length === 0,
      "原生已自动接受后又执行了手动引导");
  } else {
    const recoveryAction = report.actions?.[stage.recoveryActionId];
    require(stage.outcome === "retained" && recoveryActions.length === 1
      && recoveryAction === recoveryActions[0] && recoveryAction.status === "confirmed"
      && recoveryAction.result?.turnId === facts.turnId
      && timestamp(recoveryAction.finishedAt) <= phaseTimes[2],
    "原 Q 保留时没有唯一一次绑定原 queue ID 的显式恢复");
    const recoveryReceipts = nativeRecords.filter(record => record.actionId === recoveryAction.id
      && record.facts.threadId === threadId && record.facts.queueId === stage.queueId);
    require(recoveryReceipts.length === 1, "显式恢复缺少原 queue ID 的正式原生回执");
  }
  return facts;
}

/** 同一原 A 的暂停、自然消费和放行证据必须能从归档原件重新证明。 */
export async function verifyQueueDequeueRaceStage(stage, request, options = {}) {
  const require = queueRaceRequirement, { report, driverStages, nativeRecords } = request;
  const kind = request.caseId === "queue.dequeue_edit_race" ? "edit"
    : request.caseId === "queue.dequeue_remove_race" ? "remove" : null;
  const queueAction = report.actions?.[stage?.queueActionId], raceAction = report.actions?.[stage?.raceActionId];
  require(kind && stage?.kind === "queue-dequeue-race" && stage.attemptedKind === kind
    && stage.caseId === request.caseId && stage.threadId === request.threadId
    && report.threads?.main?.id === stage.threadId && report.threads.main.runId === report.runId
    && queueAction?.kind === "queue-send" && queueAction.caseId === request.caseId
    && queueAction.status === "confirmed" && queueAction.threadId === stage.threadId
    && (queueAction.queueId || queueAction.result?.queueId) === stage.queueId
    && queueAction.prompt === stage.prompt
    && raceAction?.kind === `queue-race-${kind}` && raceAction.status === "confirmed"
    && raceAction.caseId === request.caseId && raceAction.threadId === stage.threadId
    && raceAction.queueId === stage.queueId, "专项记录缺少本轮原 Q 和唯一旧写动作");
  require(Object.values(report.actions || {}).filter(action => action.caseId === request.caseId
    && action.kind === "queue-send").length === 1
    && Object.values(report.actions || {}).filter(action => action.caseId === request.caseId
      && /^queue-race-/.test(action.kind)).length === 1,
  "重复入队或派发了第二份旧写");
  const records = {};
  for (const [phase, driver, operation] of [
    ["before", "native", "observe-dequeue-window"],
    ["held", "environment", "observe-queue-race-request"],
    ["consumed", "native", "observe-queue-race"],
    ["released", "environment", "release-queue-race-request"],
    ["after", "native", "observe-queue-race"],
  ]) {
    const stages = driverStages.filter(({ step }) => step.driver === driver && step.operation === operation
      && step.request.threadId === stage.threadId && step.request.payload?.phase === phase);
    require(stages.length === 1 && stages[0].records.length === 1, `缺少唯一 ${phase} 原始驱动步骤`);
    const [record] = await verifyNativeEvidence(stages[0].records, {
      runId: report.runId, caseId: request.caseId, origin: report.origin,
      sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt || report.startedAt,
      capturedBefore: report.finishedAt, actions: [], threadIds: [stage.threadId],
      expectedFacts: { operation, phase },
    }, options);
    records[phase] = record;
  }
  require(new Set(Object.values(records).map(sideReadKey)).size === 5, "复用了暂停或原生阶段原件");
  const receipts = nativeRecords.filter(record => record.actionId === queueAction.id
    && record.facts.queueId === stage.queueId && record.facts.deliveryState === "queued");
  require(receipts.length === 1, "原 Q 缺少唯一 queued 回执");
  const expected = { ...stage, origin: report.origin };
  const result = verifyQueueDequeueRaceFacts({
    ...Object.fromEntries(Object.entries(records).map(([phase, record]) => [phase, record.facts])),
    receipt: receipts[0],
  }, expected);
  const rejections = nativeRecords.filter(record => record.actionId === raceAction.id
    && record.facts.receiptId === result.receiptId && record.facts.threadId === stage.threadId
    && ["failed", "rejected"].includes(record.facts.receiptState) && record.facts.code === 409);
  require(rejections.length === 1 && raceAction.result?.receiptId === result.receiptId
    && timestamp(queueAction.finishedAt) <= timestamp(records.before.facts.capturedAt)
    && timestamp(raceAction.startedAt) >= timestamp(records.before.facts.capturedAt)
    && timestamp(raceAction.finishedAt) >= timestamp(records.after.facts.capturedAt),
  "报告动作未绑定原 A 或旧写／原生读取时间顺序");
  return result;
}

/** 将七份独立原始观察和六个真实界面阶段绑定，离线重验也走同一入口。 */
export async function verifyQuestionReceiptRecoveryStage(stage, request, options = {}) {
  const { threadId, controlThreadId, ask, answer } = request;
  invariant(stage?.kind === "question-receipt-recovery" && stage.threadId === threadId
    && stage.controlThreadId === controlThreadId && stage.askActionId === ask?.id
    && stage.answerActionId === answer?.id, "问答回执恢复缺少准确原动作及两个会话的专项记录");
  invariant(stage.mainDraft === `[remote-e2e:${request.runId}:question-receipt-main-draft]`
    && stage.controlDraft === `[remote-e2e:${request.runId}:question-receipt-control-draft]`,
  "问答恢复草稿未绑定本轮两个独立输入，不能交换预期");
  const data = {};
  for (const phase of ["before", "pendingInitial", "pending", "unknown", "answered", "history", "delivery"]) {
    const [verified] = await verifyNativeEvidence([stage[phase]], {
      runId: request.runId, caseId: request.caseId, origin: request.origin,
      sourceFingerprint: request.sourceFingerprint, runStartedAt: request.runStartedAt,
      capturedBefore: request.capturedBefore, actions: [], threadIds: [threadId],
      expectedFacts: { operation: "observe-question-recovery", phase },
    }, options);
    data[phase] = verified.facts;
  }
  invariant(new Set(["before", "pendingInitial", "pending", "unknown", "answered", "history", "delivery"]
    .map(phase => sideReadKey(stage[phase]))).size === 7, "问答恢复复用了阶段观察");
  const ui = {};
  for (const name of ["controlBeforeUi", "beforeAnswerUi", "unknownUi", "reloadedUi", "unlockedUi", "controlAfterUi"]) {
    const capture = stage[name];
    const { dom } = await verifyUiEvidence(capture, options);
    const raw = await verifyEvidenceReference(capture.files?.observation, options);
    const value = JSON.parse(raw.bytes.toString("utf8").replace(/^\uFEFF/, ""));
    invariant(isDeepStrictEqual(value, capture.observation), "问答恢复 UI 与原始观察文件不一致");
    verifyFreshness({ capturedAt: value.at }, request);
    const control = name.startsWith("control");
    invariant(value.threadId === (control ? controlThreadId : threadId) && !value.loading
      && value.promptEditable === true, "问答恢复 UI 未显示准确会话的可用编辑器");
    invariant(value.prompt === (control ? stage.controlDraft : stage.mainDraft)
      && domContainsText(dom.bytes, value.prompt), "问答恢复覆盖或丢失普通草稿");
    ui[name] = value;
  }
  const deletion = value => (value.menuItems || []).filter(item => item.name === "删除会话" && item.visible);
  invariant(deletion(ui.unknownUi).length === 1 && deletion(ui.unknownUi)[0].enabled === false,
    "没有真实观察到未知回执关联的删除锁");
  invariant(!["stop", "sending"].includes(ui.unknownUi.send?.action),
    "不能把仍在生成期间的删除禁用当作未知回执锁");
  invariant(deletion(ui.unlockedUi).length === 1 && deletion(ui.unlockedUi)[0].enabled === true
    && !ui.unlockedUi.confirmation && !ui.unlockedUi.approvalText,
  "原回执恢复后删除入口未解除或仍有原问题卡片");
  invariant(!ui.reloadedUi.approvalText && !ui.reloadedUi.notice?.includes("回答已提交"),
    "刷新后已回答问题复活或仍显示已移除的成功提示");
  const times = [data.before.capturedAt, ask.startedAt, data.pendingInitial.capturedAt,
    stage.questionReloadStartedAt, data.pending.capturedAt,
    ui.controlBeforeUi.at, ui.beforeAnswerUi.at, answer.startedAt, data.unknown.capturedAt, data.answered.capturedAt,
    ui.unknownUi.at, stage.reloadStartedAt, ui.reloadedUi.at, ui.unlockedUi.at,
    ui.controlAfterUi.at, data.history.capturedAt, data.delivery.capturedAt].map(timestamp);
  invariant(times.every(Number.isFinite) && times.every((value, index) => !index || value >= times[index - 1]),
    "问答恢复阶段时间倒退或复用刷新前的观察");
  return verifyQuestionReceiptRecoveryFacts(data, { ...request, questionReloadStartedAt: stage.questionReloadStartedAt,
    reloadStartedAt: stage.reloadStartedAt,
    mainDraft: stage.mainDraft, controlDraft: stage.controlDraft,
    reloadedUiAt: ui.reloadedUi.at, recoveredUiAt: ui.unlockedUi.at, finishedUiAt: ui.controlAfterUi.at });
}

async function verifyQuestionReceiptRecovery(entry, report, driverStages, options) {
  const stages = entry.evidence.filter(value => value.kind === "question-receipt-recovery");
  invariant(stages.length === 1, "问答回执恢复固定用例缺少唯一专项记录，不能删除契约后通过");
  const stage = stages[0], ask = report.actions?.[stage.askActionId], answer = report.actions?.[stage.answerActionId];
  const main = report.threads?.main, control = report.threads?.control;
  invariant(main?.runId === report.runId && control?.runId === report.runId
    && stage.threadId === main.id && stage.controlThreadId === control.id, "问答恢复对象没有本轮归属");
  const business = Object.values(report.actions || {}).filter(action => action.caseId === entry.id
    && ["send", "answer"].includes(action.kind));
  invariant(business.length === 2 && business.includes(ask) && business.includes(answer)
    && ask.caseId === entry.id && answer.caseId === entry.id, "问答恢复不能借用旧动作或重复提交");
  for (const phase of ["before", "pendingInitial", "pending", "unknown", "answered", "history", "delivery"]) {
    const driver = ["unknown", "delivery"].includes(phase) ? "environment" : "native";
    invariant(driverStages.some(({ step, records }) => step.driver === driver
      && step.operation === "observe-question-recovery" && step.request.payload?.phase === phase
      && records.some(record => sideReadKey(record) === sideReadKey(stage[phase]))),
    "问答恢复没有关联本阶段实际驱动步骤");
  }
  return verifyQuestionReceiptRecoveryStage(stage, {
    runId: report.runId, caseId: entry.id, origin: report.origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt || report.startedAt, capturedBefore: report.finishedAt,
    threadId: main.id, controlThreadId: control.id, ask, answer,
  }, options);
}

/** 同 ID tab.title 是唯一名称依据；稳定采样不推导语言、编号、长度或首条正文。 */
export async function verifySideNamePhase(stage, request, options = {}) {
  const { threadId, parentThreadId, messageAction, expectedTitle, defaultBlank } = request;
  invariant(nonempty(threadId) && nonempty(parentThreadId)
    && stage.threadId === threadId && stage.parentThreadId === parentThreadId,
  "侧边命名阶段缺少准确的本轮侧边及父 ID");
  const captures = {};
  for (const name of ["beforeUi", "detailUi", "listUi", "reloadedUi"]) {
    const ui = stage[name];
    const { dom } = await verifyUiEvidence(ui, options);
    const artifact = await verifyEvidenceReference(ui.files.observation, options);
    const observation = JSON.parse(artifact.bytes.toString("utf8").replace(/^\uFEFF/, ""));
    invariant(isDeepStrictEqual(observation, ui.observation), "侧边命名 UI 观察与原始 JSON 文件不一致");
    invariant(Number.isFinite(timestamp(observation.at)), "侧边命名 UI 缺少实际观察时间");
    verifyFreshness({ capturedAt: observation.at }, request);
    captures[name] = { observation, dom };
  }
  const before = captures.beforeUi.observation;
  invariant(before.threadId === threadId && before.related === true && !before.loading,
    "侧边命名起点未显示准确侧边");
  if (defaultBlank) invariant(Array.isArray(before.turns) && before.turns.length === 0,
    "默认名必须在首条消息前的空侧边验证");
  if (messageAction) {
    invariant(stage.afterMessageActionId === messageAction.id && messageAction.status === "confirmed"
      && messageAction.kind === "send" && actionThreadId(messageAction) === threadId
      && nonempty(messageAction.marker) && nonempty(messageAction.expected)
      && timestamp(before.at) >= timestamp(messageAction.finishedAt),
    "侧边命名未绑定同一会话的已确认真实消息");
    const turns = (before.turns || []).filter(turn => turn.users?.some(user => user.text.includes(messageAction.marker)));
    invariant(turns.length === 1 && nonempty(turns[0].id) && !turns[0].id.startsWith("pending-")
      && !turns[0].error && turns[0].agents?.some(agent => agent.text.includes(messageAction.expected))
      && !["stop", "sending"].includes(before.send?.action),
    "侧边命名起点没有同一真实消息完成后的原始 UI 证据");
  }
  invariant(Array.isArray(stage.nativeReads) && stage.nativeReads.length === 3,
    "侧边命名缺少两次稳定原生采样及刷新后的独立核对");
  const reads = [];
  for (const record of stage.nativeReads) {
    const [verified] = await verifyNativeEvidence([record], {
      ...request, actions: [], threadIds: [threadId], capturedAfter: before.at,
      expectedFacts: { threadId, parentThreadId, nativeTabThreadId: threadId,
        visibleThreadId: threadId, surface: "codex-desktop", operation: "observe-thread" },
    }, options);
    invariant(nonempty(verified.facts.nativeTabTitle)
      && verified.facts.visibleTitle === verified.facts.nativeTabTitle,
    "侧边命名没有同一观察中的原生及 Codex 可见名称");
    reads.push(verified);
  }
  invariant(new Set(reads.map(sideReadKey)).size === 3
    && new Set(reads.map(record => [record.facts.receiptId, record.facts.readId, record.facts.eventId]
      .filter(value => value !== undefined).join(":"))).size === 3,
  "侧边命名重复使用同一原始读取身份");
  const title = reads[1].facts.nativeTabTitle;
  invariant(reads.every(record => record.facts.nativeTabTitle === title)
    && timestamp(reads[1].facts.capturedAt) - timestamp(reads[0].facts.capturedAt) >= 1_000,
  "同 ID 原生名称没有稳定，或刷新后观察已是另一名称");
  if (expectedTitle !== undefined) invariant(title === expectedTitle, "原生侧边自定义名称被后续消息覆盖或回退");
  for (const name of ["detailUi", "reloadedUi"]) {
    const { observation, dom } = captures[name];
    invariant(observation.threadId === threadId && observation.related === true
      && !observation.loading && observation.title === title && domContainsText(dom.bytes, title),
    "侧边详情名称、ID 或原始 DOM 与同 ID 原生标签不一致");
  }
  const list = captures.listUi;
  const rows = (list.observation.sideEntries || []).filter(entry => entry.id === threadId);
  invariant(list.observation.threadId === parentThreadId && rows.length === 1
    && rows[0].visible === true && rows[0].title.trim() === title && domContainsText(list.dom.bytes, title),
  "侧边列表名称或准确父子 ID 与原生标签不一致");
  const times = [before.at, reads[0].facts.capturedAt, reads[1].facts.capturedAt,
    captures.detailUi.observation.at, list.observation.at, stage.reloadStartedAt,
    captures.reloadedUi.observation.at, reads[2].facts.capturedAt].map(timestamp);
  invariant(times.every(Number.isFinite) && times.every((value, index) => !index || value >= times[index - 1]),
    "侧边命名阶段使用旧观察或刷新前后的取证顺序错误");
  invariant(captures.reloadedUi.observation.promptEditable === true, "刷新后的侧边未恢复可用状态");
  return { title, reads, beforeAt: times[0], finishedAt: times.at(-1) };
}

async function verifySideNames(entry, report, contract, driverStages, options) {
  const definition = contract.sideName;
  if (!definition) return;
  const side = report.threads?.[definition.key];
  invariant(side?.runId === report.runId && nonempty(side.id) && nonempty(side.parentThreadId),
    "侧边命名缺少本轮登记的准确侧边对象");
  const phases = entry.evidence.filter(value => value.kind === "side-name");
  invariant(phases.length === definition.phases.length
    && phases.every((value, index) => value.phase === definition.phases[index]),
  "侧边命名缺少源码要求的完整阶段");
  let previous;
  for (const phase of phases) {
    const messageAction = definition.afterMessage ? report.actions?.[phase.afterMessageActionId] : undefined;
    if (definition.afterMessage) invariant(messageAction, "侧边命名缺少首条或后续真实消息动作");
    if (definition.afterMessage)
      invariant(messageAction.caseId === (entry.id === "side.first_message_name" ? "lifecycle.side_create" : entry.id),
        "侧边命名借用了其他案例的消息，不能证明本阶段首条或后续发送");
    if (entry.id === "side.first_message_name") invariant(messageAction.marker === side.marker, "首条名称验收借用了其他消息");
    let expectedTitle;
    if (definition.custom) {
      const sequence = phase.phase === "rename-1" ? 1 : 2;
      const rename = Object.values(report.actions || {}).filter(action =>
        action.caseId === "side.desktop_rename" && action.kind === "rename-side-tab"
        && action.threadId === side.id && action.status === "confirmed"
        && action.id.endsWith(`-${sequence}`));
      invariant(rename.length === 1, "自定义名称缺少对应的原生改名动作");
      const result = await verifyNativeEvidence(rename[0].result?.records, {
        runId: report.runId, caseId: "side.desktop_rename", origin: report.origin,
        sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt || report.startedAt,
        capturedBefore: report.finishedAt, actions: [rename[0]], threadIds: [side.id],
      }, options);
      const native = result.filter(record => record.facts.nativeTabThreadId === side.id
        && nonempty(record.facts.nativeTabTitle) && record.facts.visibleThreadId === side.id
        && record.facts.visibleTitle === record.facts.nativeTabTitle);
      invariant(native.length === 1, "自定义名称没有同 ID 原生改名后的原始标签");
      expectedTitle = native[0].facts.nativeTabTitle;
    }
    for (const [index, record] of (phase.nativeReads || []).entries())
      invariant(driverStages.some(({ step, records }) => step.driver === "desktop"
        && step.operation === "observe-thread"
        && step.request.payload?.phase === (index < 2 ? phase.phase : `${phase.phase}-after-reload`)
        && records.some(candidate => sideReadKey(candidate) === sideReadKey(record))),
      "侧边命名采样未关联本阶段真实 desktop/observe-thread");
    for (const [suffix, selected] of [["", phase.nativeReads?.slice(0, 2)], ["-after-reload", phase.nativeReads?.slice(2)]]) {
      const observed = driverStages.filter(({ step }) => step.driver === "desktop"
        && step.operation === "observe-thread" && step.request.payload?.phase === `${phase.phase}${suffix}`)
        .flatMap(({ records }) => records.filter(record => record.facts.threadId === side.id
          && record.facts.nativeTabThreadId === side.id && record.facts.visibleThreadId === side.id))
        .sort((left, right) => timestamp(left.facts.capturedAt) - timestamp(right.facts.capturedAt));
      const latest = observed.slice(-(suffix ? 1 : 2));
      invariant(selected?.length === latest.length
        && selected.every((record, index) => sideReadKey(record) === sideReadKey(latest[index])),
      "侧边命名没有采用该阶段最新的原始观察，不能择取更早同名快照");
    }
    const actual = await verifySideNamePhase(phase, {
      runId: report.runId, caseId: entry.id, origin: report.origin,
      sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt || report.startedAt,
      capturedBefore: report.finishedAt, threadId: side.id, parentThreadId: side.parentThreadId,
      messageAction, expectedTitle, defaultBlank: entry.id === "side.default_name",
    }, options);
    invariant(!previous || actual.beforeAt >= previous.finishedAt, "连续改名阶段沿用了前一阶段的旧观察");
    previous = actual;
  }
}

function domContainsText(bytes, text) {
  const dom = bytes.toString("utf8");
  const normalized = value => value.replace(/\s+/gu, " ").trim();
  return dom.includes(text) || dom.includes(JSON.stringify(text).slice(1, -1))
    || normalized(dom).includes(normalized(text));
}

async function verifyLocalUiRestore(restore, nativeRecords, report, options) {
  invariant(nonempty(restore.threadId) && nonempty(restore.originalInput)
    && nonempty(restore.optimizedPrompt) && restore.originalInput !== restore.optimizedPrompt,
  "恢复原文缺少准确会话、原输入或实际优化文本");
  const before = restore.beforeUi, after = restore.afterUi;
  const beforeFiles = await verifyUiEvidence(before, options);
  const afterFiles = await verifyUiEvidence(after, options);
  invariant(before.observation.threadId === restore.threadId && after.observation.threadId === restore.threadId,
    "恢复原文前后不是同一会话");
  invariant(before.observation.prompt === restore.optimizedPrompt && after.observation.prompt === restore.originalInput,
    "恢复原文前后的 composer 文本不匹配");
  invariant(domContainsText(beforeFiles.dom.bytes, restore.optimizedPrompt)
    && domContainsText(afterFiles.dom.bytes, restore.originalInput),
  "恢复原文的原始 DOM 没有对应的优化文本或原输入");
  const times = [
    report.runStartedAt || report.startedAt, before.at, before.observation.at,
    after.at, after.observation.at, restore.at, report.finishedAt,
  ].map(timestamp);
  invariant(times.every(Number.isFinite) && times.every((value, index) => index === 0 || value >= times[index - 1]),
    "恢复原文的取证时间顺序无效");
  const optimization = nativeRecords.find(record => record.facts.threadId === restore.threadId
    && record.facts.optimizationState === "completed"
    && record.facts.originalInput === restore.originalInput
    && record.facts.optimizedPrompt === restore.optimizedPrompt);
  invariant(optimization,
  "恢复原文没有同一原始优化回执的 originalInput/optimizedPrompt 完成事实");
  return JSON.stringify([beforeFiles.dom.sha256, afterFiles.dom.sha256,
    beforeFiles.screenshot.sha256, afterFiles.screenshot.sha256, optimization.sha256]);
}

function verifyEnvironmentLeaseHistory(stages, report, entry) {
  const operations = new Set(["acquire-environment-lease", "restore-environment-resource", "inspect-environment-lease"]);
  const leaseStages = stages.filter(value => value.step.driver === "environment" && operations.has(value.step.operation));
  if (!leaseStages.length) return;
  const expectedLeaseKey = `${report.runId}:${entry.id}:environment`;
  const groups = new Map();
  for (const stage of leaseStages) {
    const records = stage.records.filter(record => record.facts.leaseKey !== undefined
      || record.facts.leaseOwnerRunId !== undefined || record.facts.instanceId !== undefined);
    invariant(records.length > 0, "环境租约缺少同一原始记录中的租约与实例身份");
    for (const record of records) {
      const facts = record.facts;
      invariant(facts.leaseKey === expectedLeaseKey && facts.leaseOwnerRunId === report.runId
        && nonempty(facts.instanceId), "环境租约原始证据属于其他 run/case/lease/instance");
      const identity = JSON.stringify([facts.leaseKey, facts.instanceId]);
      if (!groups.has(identity)) groups.set(identity, { acquisitions: [], inspections: [], restoreCount: 0 });
      const group = groups.get(identity);
      if (stage.step.operation === "acquire-environment-lease" && Object.hasOwn(facts, "originalState"))
        group.acquisitions.push(record);
      if (stage.step.operation === "inspect-environment-lease" && Object.hasOwn(facts, "currentState"))
        group.inspections.push(record);
      if (stage.step.operation === "restore-environment-resource") group.restoreCount++;
    }
  }
  for (const group of groups.values()) {
    invariant(group.acquisitions.length > 0, "环境租约缺少原始 acquire originalState");
    const originalState = group.acquisitions[0].facts.originalState;
    invariant(originalState && typeof originalState === "object" && !Array.isArray(originalState)
      && Object.keys(originalState).length > 0, "环境租约的原始 originalState 无效");
    invariant(group.acquisitions.every(record => isDeepStrictEqual(record.facts.originalState, originalState)),
      "同一环境租约的原始 originalState 发生矛盾");
    invariant(group.restoreCount > 0 && group.inspections.length > 0, "环境租约缺少恢复动作或最终原始检查");
    const captureTime = record => timestamp(record.facts.capturedAt ?? record.facts.timestamp ?? record.facts.submittedAt);
    const acquiredAt = Math.max(...group.acquisitions.map(captureTime));
    const inspectedAt = Math.max(...group.inspections.map(captureTime));
    invariant(Number.isFinite(acquiredAt) && Number.isFinite(inspectedAt) && inspectedAt >= acquiredAt,
      "环境租约的最终原始检查早于 acquire");
    const finalInspections = group.inspections.filter(record => captureTime(record) === inspectedAt);
    invariant(finalInspections.every(record => isDeepStrictEqual(record.facts.originalState, originalState)
      && isDeepStrictEqual(record.facts.currentState, originalState)),
    "环境租约最终 inspect 的原始 originalState/currentState 与 acquire 不一致；布尔声明不能替代状态");
  }
}

/**
 * 离线验收。requiredCases / scriptVersion / caseContracts 必须由当前 runner 传入，
 * 不能从待验收报告本身取出后作为“预期值”。
 */
export async function verifyRemoteAccessReport(reportOrPath, {
  requiredCases, scriptVersion, root = repositoryRoot, baseDir, caseContracts = {},
} = {}) {
  const problems = [];
  let report;
  let reportDirectory = baseDir;
  try {
    if (typeof reportOrPath === "string") {
      const reportPath = resolve(reportOrPath);
      reportDirectory ||= dirname(reportPath);
      report = JSON.parse((await readFile(reportPath, "utf8")).replace(/^\uFEFF/, ""));
    } else report = reportOrPath;
    invariant(report && typeof report === "object", "报告不是 JSON 对象");
    invariant(nonempty(scriptVersion), "报告门禁缺少当前 runner 的 scriptVersion");
    invariant(Array.isArray(requiredCases) && requiredCases.length > 0, "报告门禁缺少当前必测案例定义");
    invariant(nonempty(report.runId) && nonempty(report.origin), "报告缺少运行身份或服务 origin");
    invariant(new URL(report.origin).origin === report.origin, "报告的服务 origin 无效");
    invariant(report.scriptVersion === scriptVersion, "回归脚本版本不匹配");
    invariant(report.sourceFingerprint === await sourceFingerprint({ root }), "报告与当前源代码不匹配，需重新回归");
    invariant(Array.isArray(report.cases), "报告缺少案例列表");
    invariant(Number.isFinite(timestamp(report.runStartedAt || report.startedAt)), "报告缺少本轮起始时间");
    invariant(nonempty(report.finishedAt) && Number.isFinite(timestamp(report.finishedAt)), "报告尚未完成");
    invariant(timestamp(report.finishedAt) <= Date.now() + 5_000, "报告完成时间在未来");
    invariant(timestamp(report.finishedAt) >= timestamp(report.runStartedAt || report.startedAt), "报告完成时间早于本轮起始时间");
  } catch (error) {
    return { passed: false, problems: [error.message], runId: report?.runId };
  }
  const options = { baseDir: reportDirectory || repositoryRoot };
  const allActions = Object.values(report.actions || {});
  if (allActions.some(action => action.status !== "confirmed")) problems.push("存在送达不明或未确认的操作");
  const adoptions = report.adoptions || [];
  for (const action of allActions.filter(value => value.caseId === "external.adopt")) {
    if (adoptions.filter(value => value.actionId === action.id).length !== 1)
      problems.push(`${action.id}: 外部接入缺少唯一的原始创建证据`);
  }
  for (const adoption of adoptions) {
    try {
      const action = report.actions?.[adoption.actionId];
      invariant(action && action.caseId === "external.adopt" && action.kind === "external-created-this-run",
        "外部接入缺少本轮创建动作");
      invariant(adoption.record && adoption.record.threadId === adoption.threadId
        && adoption.record.receiptId === adoption.receiptId && adoption.record.actionId === action.id,
      "外部接入的原始创建身份不匹配");
      invariant(nonempty(adoption.marker) && adoption.marker.includes(report.runId), "外部接入缺少本轮唯一消息标记");
      await verifyNativeEvidence([adoption.record], {
        runId: report.runId, caseId: "external.adopt", origin: report.origin,
        sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt || report.startedAt,
        capturedBefore: report.finishedAt, threadIds: [adoption.threadId], actions: [action],
        expectations: adoption.record.rawFacts?.prompt
          ? [{ fact: "prompt", op: "includes", value: adoption.marker }] : [],
      }, options);
      await verifyUiEvidence(adoption.uiEvidence, options);
      invariant(adoption.uiEvidence.observation.threadId === adoption.threadId, "外部接入界面属于其他会话");
      const dom = await verifyEvidenceReference(adoption.uiEvidence.files.dom, options);
      invariant(dom.bytes.toString("utf8").includes(adoption.marker), "外部接入的原始 DOM 没有本轮完整消息标记");
    } catch (error) { problems.push(`external.adopt:${adoption.key || adoption.threadId}: ${error.message}`); }
  }
  const definitions = requiredCases.filter(value => typeof value === "string" || value.required !== false)
    .map(value => typeof value === "string" ? { id: value } : value);
  if (definitions.length === 0) problems.push("当前 runner 没有定义必测案例");
  if (new Set(definitions.map(value => value.id)).size !== definitions.length) problems.push("当前必测案例定义重复");
  for (const definition of definitions) {
    const entries = report.cases.filter(value => value.id === definition.id);
    if (entries.length !== 1) {
      problems.push(`${definition.id}: 案例缺失或重复`);
      continue;
    }
    const entry = entries[0];
    try {
      invariant(entry.status === "passed", `状态为 ${entry.status || "missing"}`);
      invariant(entry.stepsCompleted === true && !entry.inProgress, "案例步骤未完成");
      invariant(Array.isArray(entry.attempts) && entry.attempts.some(attempt => attempt.status === "passed"
        && nonempty(attempt.finishedAt) && Number.isFinite(timestamp(attempt.finishedAt))
        && timestamp(attempt.finishedAt) >= timestamp(report.runStartedAt || report.startedAt)
        && timestamp(attempt.finishedAt) <= timestamp(report.finishedAt) + 5_000),
      "案例缺少完成的执行记录");
      invariant(Array.isArray(entry.evidence) && entry.evidence.length > 0, "案例没有执行证据");
      const uiEvidence = entry.evidence.filter(value => value.kind === "ui");
      invariant(uiEvidence.length > 0, "案例缺少截图和 DOM 证据");
      for (const evidence of uiEvidence) await verifyUiEvidence(evidence, options);
      const request = caseRequest(report, entry, definition, caseContracts);
      const caseContract = caseContracts[entry.id] || definition.nativeContract || {};
      for (const requiredStep of caseContract.requiredDriverSteps || []) {
        invariant(nonempty(requiredStep.driver) && nonempty(requiredStep.operation)
          && Number.isSafeInteger(requiredStep.count) && requiredStep.count > 0, "源码的驱动步骤契约无效");
        const matches = entry.evidence.filter(value => value.kind === "driver"
          && value.driver === requiredStep.driver && value.operation === requiredStep.operation);
        const proofKeys = matches.map(value => [...new Set((value.records || []).map(record =>
          JSON.stringify([record.sha256, record.threadId, record.receiptId, record.readId, record.eventId])))].sort());
        const uniqueSteps = new Set(proofKeys.map(value => JSON.stringify(value)));
        const uniqueRecords = new Set(proofKeys.flat());
        const independentCount = Math.min(uniqueSteps.size, uniqueRecords.size);
        invariant(independentCount >= requiredStep.count,
          `缺少独立的驱动步骤：${requiredStep.driver}/${requiredStep.operation}，需要 ${requiredStep.count} 次，实际 ${independentCount} 次`);
      }
      const native = entry.evidence.flatMap(value => value.kind === "native" || value.kind === "driver" ? value.records || []
        : value.kind === "manual-scenario" || value.kind === "external-scenario"
          ? value.nativeEvidence || value.records || []
          : []);
      const requiresNative = request.actions.length > 0 || entry.nativeExpectations
        || definition.nativeContract || caseContracts[entry.id] || /^scenario\./.test(entry.id);
      const verifiedNative = requiresNative ? await verifyNativeEvidence(native, request, options) : [];
      const restores = entry.evidence.filter(value => value.kind === "local-ui-restore");
      const restoreKeys = new Set();
      for (const restore of restores)
        restoreKeys.add(await verifyLocalUiRestore(restore, verifiedNative, report, options));
      if (caseContract.requiredUiRestores !== undefined) {
        invariant(Number.isSafeInteger(caseContract.requiredUiRestores) && caseContract.requiredUiRestores > 0,
          "源码的恢复原文次数契约无效");
        invariant(restoreKeys.size >= caseContract.requiredUiRestores,
          `缺少恢复原文的真实 UI 步骤，需要 ${caseContract.requiredUiRestores} 次，实际 ${restoreKeys.size} 次`);
      }
      for (const stage of entry.evidence.filter(value => value.kind === "native")) {
        invariant(stage.request && stage.request.caseId === entry.id && stage.request.runId === report.runId,
          "原生核验阶段缺少本轮的准确请求");
        await verifyNativeEvidence(stage.records, {
          ...stage.request, runId: report.runId, caseId: entry.id,
          origin: report.origin, sourceFingerprint: report.sourceFingerprint,
          runStartedAt: report.runStartedAt || report.startedAt, capturedBefore: report.finishedAt,
        }, options);
      }
      const verifiedDriverStages = [];
      for (const step of entry.evidence.filter(value => value.kind === "driver")) {
        invariant(step.request && step.operation === step.request.operation, "外部驱动步骤缺少准确操作请求");
        invariant(step.request.caseId === entry.id && step.request.runId === report.runId,
          "外部驱动请求属于其他案例或轮次");
        const fixed = {
          ...step.request,
          runId: report.runId, caseId: entry.id, origin: report.origin,
          sourceFingerprint: report.sourceFingerprint,
          runStartedAt: report.runStartedAt || report.startedAt,
          capturedBefore: report.finishedAt,
          // 每个步骤保留自己的阶段断言；整个案例的最终契约已在上面重新验收。
          expectedFacts: step.request.expectedFacts || {},
          expectations: [...(step.request.expectations || []),
            { fact: "operation", op: "equals", value: step.operation }],
        };
        if (fixed.capturedAfter !== undefined)
          invariant(timestamp(fixed.capturedAfter) >= timestamp(fixed.runStartedAt),
            "外部驱动步骤起点早于本轮");
        const records = await verifyNativeEvidence(step.records, fixed, options);
        verifiedDriverStages.push({ step, records });
        if (["desktop", "android", "files"].includes(step.driver))
          invariant(Array.isArray(step.uiEvidence) && step.uiEvidence.length > 0,
            "桌面、Android 或文件选择驱动缺少真实界面证据");
        for (const evidence of step.uiEvidence || []) await verifyDriverUiEvidence(evidence, options);
      }
      verifyEnvironmentLeaseHistory(verifiedDriverStages, report, entry);
      await verifySideNames(entry, report, caseContract, verifiedDriverStages, options);
      if (entry.id === "queue.edit_cancel" || caseContract.requiresQueueEditCancel === true)
        await verifyQueueEditCancel(entry, report, verifiedDriverStages, options);
      if (entry.id === "recovery.question_receipt" || caseContract.requiresQuestionReceiptRecovery === true)
        await verifyQuestionReceiptRecovery(entry, report, verifiedDriverStages, options);
      if (entry.id === "queue.after_stop" || entry.id === "queue.after_failure"
          || caseContract.requiresQueueRecovery === true) {
        const stages = entry.evidence.filter(value => value.kind === "queue-recovery");
        invariant(stages.length === 1, "停止／失败队列恢复固定用例缺少唯一专项记录，不能删除契约后通过");
        await verifyQueueRecoveryStage(stages[0], {
          report, driverStages: verifiedDriverStages, nativeRecords: verifiedNative,
          caseId: entry.id, threadId: stages[0].threadId,
        }, options);
      }
      if (["queue.dequeue_edit_race", "queue.dequeue_remove_race"].includes(entry.id)
          || caseContract.requiresQueueDequeueRace === true) {
        const stages = entry.evidence.filter(value => value.kind === "queue-dequeue-race");
        invariant(stages.length === 1, "自然出队竞争固定用例缺少唯一专项记录，不能弱化契约后通过");
        await verifyQueueDequeueRaceStage(stages[0], {
          report, driverStages: verifiedDriverStages, nativeRecords: verifiedNative,
          caseId: entry.id, threadId: stages[0].threadId,
        }, options);
      }
      for (const read of entry.evidence.filter(value => value.kind === "native-read-state")) {
        // 旧已读证据若没有保存指针和原始读取标识，明确拒绝，不能退回信任 actual。
        const record = read.record || read.proof || read.records?.[0] || read;
        await verifyNativeEvidence([record], {
          ...request, actions: [], operation: "read-state", expectedReadState: read.expected,
        }, options);
      }
      if (entry.id === "read.runtime_version") {
        const runtime = entry.evidence.find(value => value.kind === "runtime-version")?.evidence;
        await verifyRuntimeEvidence(runtime || report.versionEvidence, { ...request, actions: [] }, options);
      }
      for (const scenario of entry.evidence.filter(value =>
        value.kind === "manual-scenario" || value.kind === "external-scenario")) {
        invariant(scenario.status === "passed" && nonempty(scenario.threadId)
          && Array.isArray(scenario.steps) && scenario.steps.length > 0, "外部场景缺少完成步骤或会话身份");
        invariant(native.some(record => record.threadId === scenario.threadId), "外部场景与原始证据不属于同一会话");
        for (const step of scenario.steps) {
          invariant(step && typeof step === "object" && step.status === "passed", "外部场景存在未完成步骤");
          const records = step.records || step.nativeEvidence;
          if (records) await verifyNativeEvidence(records, {
            ...request, actions: [], threadIds: [scenario.threadId],
            expectedFacts: step.expectedFacts || request.expectedFacts,
            expectations: [...request.expectations, ...(step.expectations || [])],
          }, options);
        }
      }
    } catch (error) { problems.push(`${definition.id}: ${error.message}`); }
  }
  // 必测以外也不能夹带送达不明、正在执行或“通过但无证据”的案例。
  for (const entry of report.cases) {
    if (entry.status === "unknown" || entry.inProgress) problems.push(`${entry.id}: 存在未结束或未知的案例状态`);
    if (entry.status === "passed" && (!entry.stepsCompleted || !entry.evidence?.length))
      problems.push(`${entry.id}: 通过状态缺少完成步骤或证据`);
  }
  return { passed: problems.length === 0, problems, runId: report.runId };
}
