// 审批与问答请求：应用并合并阻塞请求快照、使其失效，提交审批与问答，应用与恢复问答回执；面板渲染在 approvals.js，状态在 approval-state.js。

import { notice } from "../ui/notice.js";
import { mutate, reloadThread, renderApprovals, renderSidebar } from "../core/actions.js";
import { connected, pageLeaving, selectedId, session, thread } from "../core/state.js";
import { randomId } from "../core/util.js";
import { api } from "./api.js";
import {
  approvalReplies, approvalReplyKey, blockingApprovalIdentity, blockingRequestState,
  notifiedQuestions, questionDrafts, questionReceiptChecks, questionReplyReceipts, questionVersion,
  replaceAsyncQuestions, replaceBlockingRequests, requestsSnapshotIsVersioned, setApprovalSignature,
  setNotifiedQuestions, setQuestionReceipt, setQuestionVersion
} from "./approval-state.js";
import { approvalsAllowed, privateInputScope, threadReadOnly } from "./session/capabilities.js";

export function applyBlockingRequestsSnapshot(id, requests, snapshot = null, { readVersion = null, render = true } = {}) {
  if (typeof id !== "string" || !Array.isArray(requests) || snapshot?.requestsKnown === false) return false;
  const state = blockingRequestState(id);
  if (requestsSnapshotIsVersioned(snapshot)) {
    if (state.retiredEpochs.has(snapshot.requestsEpoch)) return true;
    if (state.epoch === snapshot.requestsEpoch && snapshot.requestsRevision <= state.revision) return true;
    if (state.epoch !== snapshot.requestsEpoch) {
      if (readVersion !== null && readVersion !== state.generation) return true;
      if (state.epoch) {
        state.retiredEpochs.add(state.epoch); ++state.sourceVersion;
        for (const [key, reply] of approvalReplies)
          if (reply.deviceId === session?.device.id && reply.threadId === id) approvalReplies.delete(key);
      }
      state.epoch = snapshot.requestsEpoch; state.completed.clear();
    }
    state.revision = snapshot.requestsRevision;
  } else {
    if (state.epoch || readVersion !== null && readVersion !== state.generation) return true;
  }
  state.requests = requests.filter(request => request.method !== "remote/requestUserInputAsync"
    && (state.epoch && (readVersion === null || readVersion === state.generation)
      || !state.completed.has(JSON.stringify(request.id))));
  ++state.generation;
  for (const [key, reply] of approvalReplies) {
    if (reply.deviceId === session?.device.id && reply.threadId === id
        && !state.requests.some(request => request.id === reply.requestId && request.method === reply.method))
      approvalReplies.delete(key);
  }
  if (selectedId === id && thread?.id === id) {
    replaceBlockingRequests(thread, state.requests);
    if (render) renderApprovals();
  }
  return true;
}
export function mergeBlockingRequestsRead(id, latest, readVersion) {
  applyBlockingRequestsSnapshot(id, latest.requests, latest, { readVersion, render: false });
  const state = blockingRequestState(id);
  if (state.requests) replaceBlockingRequests(latest, state.requests);
  else if (readVersion !== state.generation) replaceBlockingRequests(latest, []);
}
export function invalidateBlockingRequests(id, requestId, resolved = false) {
  const state = blockingRequestState(id);
  if (!state.requests && selectedId === id && thread?.id === id)
    state.requests = (thread.requests || []).filter(request => request.method !== "remote/requestUserInputAsync");
  ++state.generation;
  if (!resolved || !["string", "number"].includes(typeof requestId)) return;
  state.completed.add(JSON.stringify(requestId));
  if (state.requests) state.requests = state.requests.filter(request => request.id !== requestId);
  for (const [key, reply] of approvalReplies) {
    if (reply.deviceId === session?.device.id && reply.threadId === id && reply.requestId === requestId)
      approvalReplies.delete(key);
  }
  if (selectedId === id && thread?.id === id && state.requests) {
    replaceBlockingRequests(thread, state.requests); renderApprovals();
  }
}
export async function submitBlockingRequest(requestThreadId, request, action, value, identity = blockingApprovalIdentity(requestThreadId, request)) {
  if (!connected || threadReadOnly(requestThreadId) || !approvalsAllowed() || thread?.id !== requestThreadId) return;
  const current = (thread.requests || []).find(value => value.id === request.id && value.method === request.method);
  if (!current || blockingApprovalIdentity(requestThreadId, current) !== identity) return;
  const deviceId = session.device.id, key = approvalReplyKey(requestThreadId, request, deviceId);
  const state = blockingRequestState(requestThreadId), sourceVersion = state.sourceVersion, requestsReadVersion = state.generation;
  if (approvalReplies.has(key)) return;
  const record = { deviceId, threadId: requestThreadId, requestId: request.id, method: request.method,
    state: "submitting", time: Date.now() };
  approvalReplies.set(key, record);
  setApprovalSignature(null); renderApprovals();
  try {
    const result = await mutate(action, { threadId: requestThreadId, requestId: request.id, method: request.method, ...value });
    if (session?.device.id !== deviceId || blockingRequestState(requestThreadId).sourceVersion !== sourceVersion) return;
    const applied = applyBlockingRequestsSnapshot(requestThreadId, result?.requests, result, { readVersion: requestsReadVersion });
    if (requestsSnapshotIsVersioned(result) && result.requestsEpoch !== blockingRequestState(requestThreadId).epoch) {
      if (approvalReplies.get(key) === record) approvalReplies.delete(key);
      return;
    }
    if (approvalReplies.get(key) === record) record.state = result?.outcomeUnknown ? "unknown" : "pending";
    if (!applied && selectedId === requestThreadId) await reloadThread();
  } catch (error) {
    if (session?.device.id !== deviceId || blockingRequestState(requestThreadId).sourceVersion !== sourceVersion) return;
    if (approvalReplies.get(key) === record) {
      const notSubmitted = error.notSubmitted === true && error.outcomeUnknown !== true;
      if (notSubmitted || error.outcomeUnknown !== true && (error.approvalState === "failed" && error.code !== 409
          || error.code && ![409, 503].includes(error.code))) approvalReplies.delete(key);
      else record.state = error.approvalState === "pending" || error.code === 409 ? "pending" : "unknown";
    }
    notice(error.message, true);
    if (selectedId === requestThreadId) void reloadThread().catch(readError => notice(readError.message, true));
  } finally {
    if (session?.device.id === deviceId && selectedId === requestThreadId
        && blockingRequestState(requestThreadId).sourceVersion === sourceVersion) {
      setApprovalSignature(null); renderApprovals();
    }
  }
}
async function applyQuestionReceipt(key, receipt) {
  const record = questionReplyReceipts.get(key);
  if (!record || record.deviceId !== session?.device.id || receipt?.requestId !== record.id
      || receipt.result?.threadId && receipt.result.threadId !== record.threadId
      || receipt.result?.requestId && receipt.result.requestId !== record.requestId) return;
  const outcomeUnknown = receipt.result?.outcomeUnknown === true;
  if (receipt.state === "completed" && receipt.result?.ok === true && !outcomeUnknown) {
    setQuestionReceipt(key, null); questionDrafts.delete(key);
    renderSidebar();
    if (selectedId === record.threadId) {
      const requests = thread?.id === record.threadId ? thread.requests : notifiedQuestions;
      setQuestionVersion(questionVersion + 1);
      setNotifiedQuestions(Array.isArray(requests) ? requests.filter(request =>
        request.method === "remote/requestUserInputAsync" && request.id !== record.requestId) : null);
      if (thread?.id === record.threadId) replaceAsyncQuestions(thread, notifiedQuestions || []);
      setApprovalSignature(null); renderApprovals();
      await reloadThread();
    }
    if (receipt.result?.skipped === true) notice("问题已跳过");
  } else if (receipt.state === "failed" && !outcomeUnknown) {
    setQuestionReceipt(key, null);
    setApprovalSignature(null); renderApprovals(); renderSidebar();
    throw new Error(receipt.result?.message || "回答未提交，请核对问题后重试");
  } else {
    setQuestionReceipt(key, { ...record, state: receipt.state === "pending" && !outcomeUnknown ? "pending" : "unknown" });
    setApprovalSignature(null); renderApprovals();
  }
}
export async function recoverQuestionReceipts() {
  if (!session || pageLeaving || document.hidden || navigator.onLine === false) return;
  const scope = privateInputScope(session), deviceId = session.device.id, checks = [];
  for (const [key, record] of questionReplyReceipts) {
    if (record.deviceId !== deviceId) continue;
    const active = questionReceiptChecks.get(key);
    if (active?.id === record.id) { checks.push(active.promise); continue; }
    const checking = { id: record.id, promise: null };
    questionReceiptChecks.set(key, checking);
    checking.promise = (async () => {
      try {
        // 查询既有回执不依赖原生连接，也绝不重新提交回答或跳过操作。
        const receipt = await api(`/api/receipts/${encodeURIComponent(record.id)}`, undefined, undefined, undefined,
          { resourcePriority: "background" });
        if (scope !== privateInputScope(session) || questionReplyReceipts.get(key)?.id !== record.id) return;
        await applyQuestionReceipt(key, receipt);
      } catch {
        // 404、断网或本地存储失败均不能证明原回答未派发，保留原锁待下次核对。
      } finally {
        if (questionReceiptChecks.get(key) === checking) questionReceiptChecks.delete(key);
      }
    })();
    checks.push(checking.promise);
  }
  await Promise.allSettled(checks);
}
export async function submitAsyncQuestion(requestThreadId, request, answers, { skip = false } = {}) {
  if (!connected || threadReadOnly(requestThreadId) || !approvalsAllowed() || thread?.id !== requestThreadId) return;
  const key = JSON.stringify([session.device.id, requestThreadId, request.id]);
  if (questionReplyReceipts.has(key) || request.params?.submissionState) return;
  const record = { id: randomId(), deviceId: session.device.id, threadId: requestThreadId,
    requestId: request.id, state: "pending", ...(skip ? { skip: true } : {}) };
  try { setQuestionReceipt(key, record); }
  catch (error) { notice(error.message, true); return; }
  setApprovalSignature(null); renderApprovals();
  try {
    const receipt = await api("/api/action/answer", {
      threadId: requestThreadId, requestId: request.id, method: request.method,
      response: skip ? { skip: true } : { answers }
    }, record.id);
    await applyQuestionReceipt(key, receipt);
  } catch (error) {
    if (questionReplyReceipts.get(key)?.id === record.id) {
      const notSubmitted = error.notSubmitted === true && error.outcomeUnknown !== true;
      if (notSubmitted || error.outcomeUnknown !== true && error.code && error.code !== 503) setQuestionReceipt(key, null);
      else setQuestionReceipt(key, { ...record, state: "unknown" });
    }
    notice(error.message, true);
    if (error.code === 409 && selectedId === requestThreadId)
      void reloadThread().catch(() => {});
  } finally {
    setApprovalSignature(null); renderApprovals();
    void recoverQuestionReceipts();
  }
}
