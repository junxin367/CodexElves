/**
 * 长期 CDP 控制流回归：全部使用 mock，不打开浏览器、不发送业务请求，
 * 不构成 recovery.question_receipt 的真实 E2E 或成功业务回执证据。
 */
import test from "node:test";
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { randomUUID } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, rmdir, unlink, writeFile } from "node:fs/promises";
import { resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { isDeepStrictEqual } from "node:util";
import { runInNewContext } from "node:vm";
import {
  createQuestionReceiptRecoveryDriver, QUESTION_RECEIPT_EVENT_METHODS,
} from "./remote-access-e2e-receipt-driver.mjs";
import { verifyNativeEvidence } from "./remote-access-e2e-evidence.mjs";
import { withRemoteAccessEnvironmentLease } from "./remote-access-e2e-environment.mjs";

const tempRoot = fileURLToPath(new URL("../temp/", import.meta.url));
const origin = "http://127.0.0.1:55447";
const questionId = JSON.stringify(["async-question", "origin-turn", "source-item"]);
const copy = value => structuredClone(value);
const foreign = value => runInNewContext("JSON.parse(payload)", { payload: JSON.stringify(value) });
const clearingFetch = command => command.method === "Fetch.enable" && command.params.patterns.length === 0;

async function originalRecord(result) {
  assert.ok(result.records?.length, "终态必须保留原始 JSON 记录");
  return JSON.parse(await readFile(result.records.at(-1).evidencePath, "utf8"));
}

async function until(condition, description = "mock 控制流事件", timeoutMs = 2_000) {
  const expires = Date.now() + timeoutMs;
  do {
    if (condition()) return;
    await delay(5);
  } while (Date.now() < expires);
  assert.fail(`等待超时：${description}`);
}

async function fixture(t, configuration = {}) {
  await mkdir(tempRoot, { recursive: true });
  const outputDir = await mkdtemp(resolve(tempRoot, "receipt-driver-control-"));
  const runId = `receipt-driver-control-${randomUUID()}`, tabId = `tab-${randomUUID()}`;
  const threadId = `main-${runId}`, leaseKey = `${runId}:recovery.question_receipt:environment`;
  const scope = { kind: "authorized-tab", instanceId: `test-${runId}`, origin, servicePort: 55447, tabId };
  const state = {
    tabInfo: { id: tabId, url: `${origin}/?thread=${threadId}` },
    frame: { id: "main-frame", loaderId: "loader-initial", url: `${origin}/` },
    metadata: { device: { id: "real-device-from-readonly-callback", createdAtMs: Date.now() - 100,
      token: "never-write-device-token" }, clientReceipts: [] },
    events: [], commands: [], reads: [], fetchBodies: new Map(), networkBodies: new Map(), pauses: new Map(),
    nextReadError: null, nextTruncated: false, pageSize: configuration.pageSize || 1000,
    onCommand: null, onReadEvents: null, sequence: 0,
  };
  function emit(method, params) {
    const event = { method, params: copy(params), sequence: ++state.sequence, source: { tabId } };
    state.events.push(event);
    if (method === "Fetch.requestPaused") state.pauses.set(params.requestId, params);
    return event;
  }
  const rawCdp = {
    async send(method, params) {
      state.commands.push({ method, params: copy(params) });
      if (state.onCommand) {
        const value = await state.onCommand(method, params);
        if (value !== undefined) return value;
      }
      if (method === "Page.getFrameTree") return { frameTree: { frame: copy(state.frame) } };
      if (method === "Fetch.getResponseBody") {
        assert.ok(state.fetchBodies.has(params.requestId), "mock 必须显式提供实际模拟正文");
        return copy(state.fetchBodies.get(params.requestId));
      }
      if (method === "Network.getResponseBody") {
        assert.ok(state.networkBodies.has(params.requestId), "mock 必须显式提供实际模拟 GET 正文");
        return copy(state.networkBodies.get(params.requestId));
      }
      if (method === "Fetch.failRequest") {
        const paused = state.pauses.get(params.requestId);
        if (paused?.networkId) {
          emit("Network.loadingFailed", { requestId: paused.networkId,
            timestamp: Date.now() / 1000, errorText: "net::ERR_ABORTED", canceled: true });
          if (paused.request.method === "POST") {
            state.metadata.clientReceipts = [{
              id: paused.request.headers["Idempotency-Key"], threadId,
              requestId: questionId, deviceId: state.metadata.device.id, state: "unknown",
              extraOriginalField: { preserved: "完整原始客户端记录" },
            }];
          }
        }
        state.pauses.delete(params.requestId);
      }
      if (method === "Fetch.continueRequest") state.pauses.delete(params.requestId);
      return {};
    },
    async readEvents(options) {
      state.reads.push(copy(options));
      if (state.nextReadError) {
        const error = state.nextReadError; state.nextReadError = null;
        throw error;
      }
      const selected = state.events.filter(event => event.sequence > options.afterSequence);
      const events = selected.slice(0, Math.min(options.limit, state.pageSize));
      if (events.length === 0 && options.timeoutMs > 0) await delay(2);
      const truncated = state.nextTruncated; state.nextTruncated = false;
      if (state.onReadEvents) await state.onReadEvents(options);
      return { cursor: events.at(-1)?.sequence ?? options.afterSequence, events: copy(events),
        hasMore: selected.length > events.length, truncated };
    },
  };
  const toRealm = configuration.crossRealm ? foreign : copy;
  const cdp = configuration.crossRealm ? {
    async send(...args) { return foreign(await rawCdp.send(...args)); },
    async readEvents(...args) { return foreign(await rawCdp.readEvents(...args)); },
  } : rawCdp;
  const driverOptions = {
    tab: toRealm({ id: tabId }), cdp, outputDir, scope: toRealm(scope),
    readTabInfo: async () => toRealm(state.tabInfo),
    readClientMetadata: async () => toRealm(state.metadata),
    newTabProof: toRealm({ tabId, runId, createdAt: new Date().toISOString(),
      initialUrl: origin, noExistingFetchInterception: true }),
    timeoutMs: configuration.timeoutMs || 150, commandTimeoutMs: configuration.commandTimeoutMs || 200,
    pollIntervalMs: 3, eventPollTimeoutMs: 10,
  };
  const driver = createQuestionReceiptRecoveryDriver(driverOptions);
  const extraDrivers = [];
  t.after(async () => {
    try {
      for (const other of extraDrivers) await other.close();
      await driver.close();
    } finally {
      const absolute = resolve(outputDir);
      assert.ok(absolute.startsWith(`${resolve(tempRoot)}${sep}`), "只清理本测试明确创建的 temp 子目录");
      await rm(absolute, { recursive: true, force: true });
    }
  });
  const allRecords = [], actions = [];
  function request(operation, payload = {}, mutation = true) {
    const action = mutation ? {
      id: `${runId}:${operation}:${randomUUID()}`, kind: operation, threadId, status: "confirmed",
    } : null;
    if (action) actions.push(action);
    return {
      contractVersion: "remote-e2e-driver/2", operation, runId, caseId: "recovery.question_receipt",
      origin, sourceFingerprint: "a".repeat(64), runStartedAt: new Date(Date.now() - 1_000).toISOString(),
      capturedAfter: new Date().toISOString(), threadId, threadIds: [threadId],
      ownedThreadIds: [threadId], outputDir,
      ...(action ? { actionId: action.id } : {}), actions: action ? [action] : [],
      expectations: [
        { fact: "threadId", op: "equals", value: threadId },
        { fact: "leaseKey", op: "equals", value: leaseKey },
        { fact: "leaseOwnerRunId", op: "equals", value: runId },
        { fact: "instanceId", op: "equals", value: scope.instanceId },
        { fact: "scopeKind", op: "equals", value: "authorized-tab" },
        { fact: "servicePort", op: "equals", value: 55447 },
        { fact: "tabId", op: "equals", value: tabId },
        { fact: "unownedStateChanged", op: "equals", value: false },
      ],
      payload: {
        environmentLease: { leaseKey, scope }, ...payload,
        ...(operation.includes("environment-") ? { leaseKey, scope, restoreOnlyOwnedChanges: true } : {}),
      },
    };
  }
  async function call(operation, payload = {}, mutation = true) {
    const input = request(operation, payload, mutation);
    const result = await driver[mutation ? "perform" : "observe"](toRealm(input));
    if (result.status === "blocked") return { input, result, records: result.records || [] };
    const records = await verifyNativeEvidence(result.records, input, { baseDir: outputDir });
    allRecords.push(...result.records);
    return { input, result, records, facts: records[0].facts };
  }
  async function start() {
    const acquired = await call("acquire-environment-lease", { resources: ["receipts"] });
    assert.equal(acquired.facts.leaseAcquired, true);
    const started = await call("start-question-recovery-observation", { onlyCurrentTab: true, readOnly: true });
    assert.equal(started.facts.observationStarted, true);
  }
  async function arm(receiptReadFault) {
    const result = await call("drop-question-answer-receipt", {
      threadId, requestId: questionId, onlyAction: "answer",
      preserveServerExecution: true, holdOnlyOriginalReceiptReadsUntilNewDocument: true,
      ...(receiptReadFault === undefined ? {} : { receiptReadFault }),
    });
    assert.equal(result.facts.receiptFaultArmed, true);
    return result;
  }
  function answer({ networkId = "answer-original", fetchId = "fetch-answer-original",
    targetThreadId = threadId, requestId = questionId, receiptId = "original-receipt", skip = false,
    receiptState = "completed" } = {}) {
    const request = {
      url: `${origin}/api/action/answer`, method: "POST",
      headers: { "Idempotency-Key": receiptId, Cookie: "never-write-cookie", Authorization: "Bearer never-write-auth" },
      postData: JSON.stringify({ threadId: targetThreadId, requestId, method: "remote/requestUserInputAsync",
        response: skip ? { skip: true } : { answers: { q1: { answers: ["批准答案"] } } } }),
    };
    const receipt = { requestId: receiptId, state: receiptState,
      result: { ok: receiptState === "completed", threadId: targetThreadId, requestId } };
    emit("Network.requestWillBeSent", { requestId: networkId, loaderId: "loader-initial", frameId: "main-frame",
      wallTime: Date.now() / 1000, timestamp: Date.now() / 1000, type: "Fetch", request });
    emit("Network.responseReceived", { requestId: networkId, timestamp: Date.now() / 1000, type: "Fetch",
      response: { status: 200, url: request.url, headers: { "Set-Cookie": "never-write-set-cookie" } } });
    state.fetchBodies.set(fetchId, { body: JSON.stringify(receipt), base64Encoded: false });
    emit("Fetch.requestPaused", { requestId: fetchId, networkId, request, resourceType: "Fetch",
      frameId: "main-frame", responseStatusCode: 200,
      responseHeaders: [{ name: "Set-Cookie", value: "never-write-set-cookie" }] });
    return receipt;
  }
  function receiptGet({ networkId, fetchId, receiptId = "original-receipt", loaderId = "loader-initial",
    resourceType = "Fetch", frameId = "main-frame" }) {
    const request = { url: `${origin}/api/receipts/${encodeURIComponent(receiptId)}`, method: "GET",
      headers: { Cookie: "never-write-cookie" } };
    const networkEvent = emit("Network.requestWillBeSent", { requestId: networkId, loaderId, frameId,
      wallTime: Date.now() / 1000, timestamp: Date.now() / 1000, type: resourceType, request });
    const pausedEvent = emit("Fetch.requestPaused", { requestId: fetchId, networkId, request, resourceType, frameId });
    return { networkEvent, pausedEvent };
  }
  function document({ loaderId = "loader-reloaded", frameId = "main-frame", networkId = randomUUID() } = {}) {
    emit("Network.requestWillBeSent", { requestId: networkId, loaderId, frameId,
      wallTime: Date.now() / 1000, timestamp: Date.now() / 1000, type: "Document",
      request: { method: "GET", url: `${origin}/`, headers: {} } });
    if (frameId === "main-frame") state.frame.loaderId = loaderId;
  }
  async function unknown() {
    return call("observe-question-recovery", { phase: "unknown", questionRequestId: questionId }, false);
  }
  async function release() {
    return call("release-question-receipt-on-reload", {
      originalReceiptId: "original-receipt", questionRequestId: questionId, onlyCurrentTab: true,
    });
  }
  return { driver, driverOptions, registerDriver: value => { extraDrivers.push(value); return value; },
    state, scope, outputDir, runId, tabId, threadId, leaseKey, cdp,
    emit, call, request, start, arm, answer, receiptGet, document, unknown, release, allRecords, actions };
}

test("mock 控制流：租约绑定真实 tab，恢复的原始 JSON 可由现有动作门禁重读", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.stateMatchesOriginal, true);
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.deepEqual(inspected.facts.currentState, inspected.facts.originalState);
  assert.equal(inspected.facts.currentState.receipts.driverObservationActive, false);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.enable" && command.params.patterns.length === 0));
  assert.equal(f.state.commands.some(command => command.method === "Fetch.disable"), false);
  assert.equal(f.state.commands.some(command => command.method === "Network.disable"), false);
  await verifyNativeEvidence(f.allRecords, {
    runId: f.runId, caseId: "recovery.question_receipt", origin, sourceFingerprint: "a".repeat(64),
    threadIds: [f.threadId], runStartedAt: new Date(Date.now() - 5_000).toISOString(), actions: f.actions,
  }, { baseDir: f.outputDir });
  const readCount = f.state.reads.length;
  await delay(15);
  assert.equal(f.state.reads.length, readCount, "恢复后后台读取已经停止");
});

test("mock 控制流：故障只配置显式 Fetch/XHR 类型，不拦截浏览器管理的 Document 导航", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  const enabling = f.state.commands.find(command => command.method === "Fetch.enable"
    && command.params.patterns?.length);
  assert.ok(enabling);
  assert.equal(enabling.params.patterns.length, 4);
  assert.ok(enabling.params.patterns.every(pattern => ["Fetch", "XHR"].includes(pattern.resourceType)));
  for (const resourceType of ["Fetch", "XHR"]) {
    assert.ok(enabling.params.patterns.some(pattern => pattern.resourceType === resourceType
      && pattern.urlPattern === `${origin}/api/action/answer*` && pattern.requestStage === "Response"));
    assert.ok(enabling.params.patterns.some(pattern => pattern.resourceType === resourceType
      && pattern.urlPattern === `${origin}/api/receipts/*` && pattern.requestStage === "Request"));
  }
});

test("mock 控制流：仅准确原 thread/request 的成功回答丢一次，其他请求立即透传", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.answer({ networkId: "other-thread", fetchId: "fetch-other-thread", targetThreadId: "unowned-thread" });
  f.answer({ networkId: "other-question", fetchId: "fetch-other-question", requestId: "other-native-request" });
  f.answer({ networkId: "skip-question", fetchId: "fetch-skip-question", skip: true });
  const receipt = f.answer();
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-answer-original"));
  f.answer({ networkId: "answer-second", fetchId: "fetch-answer-second" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-answer-second"));
  const unknown = await f.unknown();
  assert.deepEqual(unknown.facts.answerReceipt, receipt);
  assert.equal(unknown.facts.clientReceipts[0].extraOriginalField.preserved, "完整原始客户端记录");
  assert.equal(unknown.facts.device.token, undefined);
  const failedAnswers = f.state.commands.filter(command => command.method === "Fetch.failRequest"
    && command.params.requestId.startsWith("fetch-answer"));
  assert.equal(failedAnswers.length, 1);
  for (const id of ["fetch-other-thread", "fetch-other-question", "fetch-skip-question"])
    assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest" && command.params.requestId === id));
  const raw = await readFile(unknown.result.records[0].evidencePath, "utf8");
  for (const secret of ["never-write-auth", "never-write-cookie", "never-write-set-cookie", "never-write-device-token"])
    assert.equal(raw.includes(secret), false);
  assert.ok(raw.includes("Idempotency-Key") && raw.includes("answer-original"));
});

test("mock 控制流：释放登记不放行，只有新主文档 loader 才放行同 ID GET 并读取实际正文", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  const receipt = f.answer();
  await f.unknown();
  f.receiptGet({ networkId: "finally-get", fetchId: "fetch-finally-get" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-finally-get"));
  const armed = await f.release();
  assert.equal(armed.facts.releaseOnNewDocument, true);
  f.receiptGet({ networkId: "before-document", fetchId: "fetch-before-document" });
  f.receiptGet({ networkId: "unrelated-get", fetchId: "fetch-unrelated-get", receiptId: "other-receipt" });
  f.document({ frameId: "iframe", loaderId: "iframe-new-loader" });
  f.receiptGet({ networkId: "after-iframe", fetchId: "fetch-after-iframe" });
  f.document({ loaderId: "loader-initial", networkId: "same-loader-document" });
  f.receiptGet({ networkId: "after-old-loader", fetchId: "fetch-after-old-loader" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-after-old-loader"));
  for (const id of ["fetch-before-document", "fetch-after-iframe", "fetch-after-old-loader"])
    assert.ok(f.state.commands.some(command => command.method === "Fetch.failRequest" && command.params.requestId === id));
  assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-unrelated-get"));
  await delay(2);
  f.document({ networkId: "real-reload" });
  f.receiptGet({ networkId: "recovery-get", fetchId: "fetch-recovery-get", loaderId: "loader-reloaded" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-recovery-get"));
  const getBody = { body: Buffer.from(JSON.stringify(receipt)).toString("base64"), base64Encoded: true };
  f.state.networkBodies.set("recovery-get", getBody);
  f.emit("Network.responseReceived", { requestId: "recovery-get", timestamp: Date.now() / 1000,
    type: "Fetch", response: { status: 200, url: `${origin}/api/receipts/original-receipt`,
      headers: { "Set-Cookie": "never-write-set-cookie" } } });
  f.emit("Network.loadingFinished", { requestId: "recovery-get", timestamp: Date.now() / 1000 });
  f.state.metadata.clientReceipts = [];
  f.state.metadata.device.lastSeenAtMs = Date.now();
  const delivery = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(delivery.facts.eventsTruncated, false);
  assert.deepEqual(delivery.facts.responseBodies, [{ requestId: "recovery-get", body: getBody }]);
  assert.deepEqual(delivery.facts.clientReceipts, []);
  assert.ok(delivery.facts.networkEvents.some(event => event.method === "Network.loadingFailed"));
  assert.ok(delivery.facts.networkEvents.some(event => event.method === "Network.loadingFinished"));
  assert.ok(f.state.commands.some(command => command.method === "Network.getResponseBody"
    && command.params.requestId === "recovery-get"));
  assert.equal(delivery.facts.networkEvents.find(event => event.params.requestId === "answer-original")
    .params.request.headers.Authorization, undefined);
  const action = { id: `${f.runId}:original-answer`, kind: "answer", threadId: f.threadId,
    questionRequestId: questionId, status: "confirmed", result: { receiptId: "original-receipt" } };
  const finalRequest = { ...f.request("observe-question-recovery", {}, false), actions: [action], expectations: [] };
  delete finalRequest.operation; delete finalRequest.contractVersion;
  const evidence = await f.driver.readAnswerReceiptEvidence(finalRequest, action);
  const [verified] = await verifyNativeEvidence(evidence.records, finalRequest, { baseDir: f.outputDir });
  assert.equal(verified.facts.receiptState, "completed");
  assert.equal(verified.facts.receiptId, "original-receipt");
});

test("mock 控制流：hasMore 始终沿同 methods/cursor 取完，不遗漏批尾 Fetch 暂停", async t => {
  const f = await fixture(t, { pageSize: 2 });
  await f.start(); await f.arm();
  for (let index = 0; index < 7; index++)
    f.emit("Network.requestWillBeSent", { requestId: `unrelated-${index}`, loaderId: "loader-initial",
      wallTime: Date.now() / 1000, timestamp: Date.now() / 1000, type: "Fetch",
      request: { url: `${origin}/api/state`, method: "GET", headers: {} } });
  f.answer();
  const unknown = await f.unknown();
  assert.equal(unknown.facts.clientReceipts[0].state, "unknown");
  for (const read of f.state.reads) {
    assert.deepEqual(read.methods, [...QUESTION_RECEIPT_EVENT_METHODS]);
    assert.equal(read.limit, 1000);
  }
  assert.ok(f.state.reads.some((read, index, reads) => index > 0
    && read.afterSequence > reads[index - 1].afterSequence && read.timeoutMs === 0));
});

test("mock 控制流：未捕获 unknown 时阻塞，不补造记录、不重答，异常后撤回自身拦截", async t => {
  const f = await fixture(t, { timeoutMs: 40 });
  await f.start(); await f.arm();
  const observation = await f.unknown();
  assert.equal(observation.result.status, "blocked");
  assert.match(observation.result.reason, /未捕获/);
  assert.deepEqual(f.state.metadata.clientReceipts, []);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.enable" && command.params.patterns.length === 0));
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.facts.stateMatchesOriginal, true);
});

test("mock 控制流：读取 answer 正文失败会放开原暂停请求并保留 blocked", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.onCommand = (method) => {
    if (method === "Fetch.getResponseBody") throw new Error("模拟 CDP 正文读取失败");
  };
  f.answer();
  const observation = await f.unknown();
  assert.equal(observation.result.status, "blocked");
  assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-answer-original"));
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.enable" && command.params.patterns.length === 0));
});

test("mock 控制流：failRequest 结果未知不重复丢包，仍撤回配置并继续原暂停请求", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.onCommand = (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-answer-original")
      throw new Error("模拟 failRequest 已派发但响应未知");
  };
  f.answer();
  const observation = await f.unknown();
  assert.equal(observation.result.status, "blocked");
  assert.equal(f.state.commands.filter(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-answer-original").length, 1);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-answer-original"));
  assert.ok(f.state.commands.some(command => command.method === "Fetch.enable" && command.params.patterns.length === 0));
});

test("mock 控制流：只有 GET 开始和客户端锁消失不足以交付，必须实际 loadingFinished 与正文", async t => {
  const f = await fixture(t, { timeoutMs: 40 });
  await f.start(); await f.arm(); f.answer(); await f.unknown(); await f.release();
  await delay(2);
  f.document({ networkId: "reload-no-finished" });
  f.receiptGet({ networkId: "get-not-finished", fetchId: "fetch-get-not-finished", loaderId: "loader-reloaded" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-get-not-finished"));
  f.state.metadata.clientReceipts = [];
  const result = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(result.result.status, "blocked");
  assert.match(result.result.reason, /未实际完成/);
  assert.equal(f.state.commands.some(command => command.method === "Network.getResponseBody"), false);
});

test("mock 控制流：事件读取错误不挂住观察，恢复后停止后台循环", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.nextReadError = new Error("模拟 readEvents 断开");
  const observation = await f.unknown();
  assert.equal(observation.result.status, "blocked");
  assert.match(observation.result.reason, /后台观察失败/);
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.facts.stateMatchesOriginal, true);
  const reads = f.state.reads.length;
  await delay(15);
  assert.equal(f.state.reads.length, reads);
});

test("mock 控制流：截断不伪称恢复，保护未知暂停状态", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.nextTruncated = true;
  const result = await f.unknown();
  assert.equal(result.result.status, "blocked");
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.result.status, "blocked");
  const raw = await originalRecord(inspected.result);
  assert.equal(raw.observation.stateMatchesOriginal, false);
  assert.equal(raw.observation.currentState.receipts.unobservedPausedRequestsPossible, true);
});

test("mock 控制流：服务端 pending 不能伪装 completed 成功后丢包", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.answer({ receiptState: "pending" });
  const result = await f.unknown();
  assert.equal(result.result.status, "blocked");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-answer-original"));
});

test("mock 控制流：租约拒绝 45222、实际 URL 越界与既有 Fetch 接管", async t => {
  const f = await fixture(t);
  f.state.tabInfo.url = "http://127.0.0.1:45222/";
  const outOfScope = await f.call("acquire-environment-lease", { resources: ["receipts"] });
  assert.equal(outOfScope.result.status, "blocked");
  assert.equal(f.state.commands.length, 0);
  f.state.tabInfo.url = `${origin}/`;
  f.emit("Fetch.requestPaused", { requestId: "preexisting-other-owner", request: {
    method: "GET", url: `${origin}/api/receipts/other`, headers: {},
  } });
  const existing = await f.call("acquire-environment-lease", { resources: ["receipts"] });
  assert.equal(existing.result.status, "blocked");
  assert.match(existing.result.reason, /不能接管/);
  assert.equal(f.state.commands.length, 0);
  assert.throws(() => createQuestionReceiptRecoveryDriver({
    tab: { id: f.tabId }, cdp: f.cdp, outputDir: f.outputDir,
    scope: { ...f.scope, kind: "dedicated-test-service" }, readClientMetadata() {},
  }), /authorized-tab/);
});

test("mock 跨 realm：scope/request/CDP/元数据均来自不同 vm，完整恢复仍按 JSON 原值校验", async t => {
  const f = await fixture(t, { crossRealm: true, timeoutMs: 500 });
  const scopeFromOtherRealm = foreign(f.scope);
  assert.equal(JSON.stringify(scopeFromOtherRealm), JSON.stringify(f.scope));
  assert.equal(isDeepStrictEqual(scopeFromOtherRealm, f.scope), false, "复现真实 CUA 的原型差异");
  assert.notEqual(Object.getPrototypeOf(scopeFromOtherRealm), Object.getPrototypeOf(f.scope));
  await f.start(); await f.arm();
  const receipt = f.answer();
  const unknown = await f.unknown();
  assert.equal(unknown.facts.clientReceipts[0].extraOriginalField.preserved, "完整原始客户端记录");
  await f.release(); await delay(2);
  f.document({ networkId: "cross-realm-document" });
  f.receiptGet({ networkId: "cross-realm-get", fetchId: "fetch-cross-realm-get", loaderId: "loader-reloaded" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-cross-realm-get"));
  f.state.networkBodies.set("cross-realm-get", { body: JSON.stringify(receipt), base64Encoded: false });
  f.emit("Network.responseReceived", { requestId: "cross-realm-get", timestamp: Date.now() / 1000,
    type: "Fetch", response: { status: 200, url: `${origin}/api/receipts/original-receipt`, headers: {} } });
  f.emit("Network.loadingFinished", { requestId: "cross-realm-get", timestamp: Date.now() / 1000 });
  f.state.metadata.clientReceipts = [];
  const delivery = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(delivery.facts.eventsTruncated, false);
  assert.equal(JSON.parse(delivery.facts.responseBodies[0].body.body).requestId, "original-receipt");
  const answer = { id: `${f.runId}:answer-from-other-realm`, kind: "answer", threadId: f.threadId,
    questionRequestId: questionId, status: "confirmed", result: { receiptId: "original-receipt" } };
  const request = { runId: f.runId, caseId: "recovery.question_receipt", origin,
    sourceFingerprint: "a".repeat(64), actions: [answer] };
  const evidence = await f.driver.readAnswerReceiptEvidence(foreign(request), foreign(answer));
  const [verified] = await verifyNativeEvidence(evidence.records, request, { baseDir: f.outputDir });
  assert.equal(verified.facts.receiptSuccess, true);
  const restored = await f.call("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true });
  assert.equal(restored.facts.restoreDisposition, "restored");
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.deepEqual(inspected.facts.currentState, inspected.facts.originalState);
});

test("mock 跨 realm：获取前失败的租约调度保留原错误，恢复无副作用并检查原状态一致", async t => {
  const f = await fixture(t, { crossRealm: true });
  f.state.nextReadError = new Error("模拟获取前只读 baseline 失败");
  let acquired, restored, inspected;
  await assert.rejects(withRemoteAccessEnvironmentLease({
    leaseKey: f.leaseKey, runId: f.runId, origin, tabId: f.tabId, scope: f.scope, resources: ["receipts"],
    acquire: async () => {
      acquired = await f.call("acquire-environment-lease", { resources: ["receipts"] });
      assert.equal(acquired.result.status, "blocked");
      throw new Error(acquired.result.reason);
    },
    run: () => assert.fail("获取失败后不能开始故障"),
    restoreResource: async request => {
      assert.equal(request.acquisitionUnknown, true);
      restored = await f.call("restore-environment-resource", {
        resource: request.resource, acquisitionUnknown: request.acquisitionUnknown, originalState: null,
      });
      assert.equal(restored.result.status, undefined);
      return restored.result;
    },
    verifyRestored: async () => {
      inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
      assert.equal(inspected.facts.stateMatchesOriginal, true);
    },
  }), /模拟获取前只读 baseline 失败/);
  assert.equal(restored.facts.restoreDisposition, "lease-absent-no-mutation");
  assert.equal(restored.facts.leaseAcquired, false);
  assert.deepEqual(inspected.facts.currentState, inspected.facts.originalState);
  assert.equal(f.state.commands.length, 0, "收尾不发送任何 CDP 命令，不触碰其他拦截");
  const raw = JSON.parse(await readFile(restored.result.records[0].evidencePath, "utf8"));
  assert.equal(raw.raw.acquisitionAttempts[0].phase, "failed-before-lease");
  assert.equal(raw.raw.acquisitionAttempts[0].leaseRegistrationAttempted, false);
  assert.deepEqual(raw.raw.mutationAttempts, []);
  assert.equal(raw.raw.restoration.noCdpCommandIssued, true);
  const failed = JSON.parse(await readFile(acquired.result.records[0].evidencePath, "utf8"));
  assert.equal(failed.observation.noMutationProven, true);
  assert.equal(failed.observation.receiptState, undefined, "不能确认失败的 acquire");
});

test("mock 跨 realm：相同 tabId 不能掩盖 scope 字段、类型、额外键或原租约身份变化", async t => {
  const f = await fixture(t, { crossRealm: true });
  f.state.nextReadError = new Error("模拟获取前失败");
  assert.equal((await f.call("acquire-environment-lease", { resources: ["receipts"] })).result.status, "blocked");
  const changes = [
    input => { input.payload.scope.instanceId += "-other"; },
    input => { input.payload.scope.servicePort = "55447"; },
    input => { input.payload.scope.extraField = "不属于原 scope"; },
    input => { input.payload.environmentLease.scope = { ...f.scope, instanceId: `${f.scope.instanceId}-conflict` }; },
    input => { input.payload.environmentLease.leaseKey += "-conflict"; },
    input => {
      input.payload.leaseKey += "-other";
      input.payload.environmentLease.leaseKey = input.payload.leaseKey;
    },
    input => { input.sourceFingerprint = "b".repeat(64); },
    input => { input.threadId += "-other"; input.ownedThreadIds.push(input.threadId); },
  ];
  for (const change of changes) {
    const input = copy(f.request("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true }));
    change(input);
    const result = await f.driver.perform(foreign(input));
    assert.equal(result.status, "blocked");
  }
  assert.equal(f.state.commands.length, 0);
  const correct = await f.call("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true });
  assert.equal(correct.facts.restoreDisposition, "lease-absent-no-mutation");
});

test("mock 控制流：新驱动没有本实例获取失败证据时，拒绝将未知获取解释成无副作用", async t => {
  const f = await fixture(t);
  const restored = await f.call("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true });
  assert.equal(restored.result.status, "blocked");
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.result.status, "blocked");
  assert.equal(f.state.commands.length, 0);
  const raw = JSON.parse(await readFile(restored.result.records[0].evidencePath, "utf8"));
  assert.equal(raw.observation.noMutationProven, false);
  assert.deepEqual(raw.raw.acquisitionAttempts, []);
});

test("mock 控制流：已有其他驱动所有权或先前 Fetch 暂停时，无租约收尾不能绕过拒绝", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  const other = f.registerDriver(createQuestionReceiptRecoveryDriver(f.driverOptions));
  const before = f.state.commands.length;
  const acquired = await other.perform(f.request("acquire-environment-lease", { resources: ["receipts"] }));
  assert.equal(acquired.status, "blocked");
  const restored = await other.perform(f.request("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true }));
  assert.equal(restored.status, "blocked");
  assert.equal(f.state.commands.length, before, "不清除其他驱动的 Fetch patterns");
  const raw = JSON.parse(await readFile(restored.records[0].evidencePath, "utf8"));
  assert.equal(raw.raw.acquisitionAttempts[0].ownershipRejected, true);

  const paused = await fixture(t);
  paused.emit("Fetch.requestPaused", { requestId: "preexisting-pause", request: {
    method: "GET", url: `${origin}/api/receipts/existing`, headers: {},
  } });
  assert.equal((await paused.call("acquire-environment-lease", { resources: ["receipts"] })).result.status, "blocked");
  const pauseRestore = await paused.call("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true });
  assert.equal(pauseRestore.result.status, "blocked");
  assert.equal(paused.state.commands.length, 0);
});

test("mock 控制流：acquire 已登记但取证落盘失败，未知获取仍按原租约恢复，不能标记 lease absent", async t => {
  const f = await fixture(t);
  f.state.onReadEvents = async () => {
    f.state.onReadEvents = null;
    assert.ok(resolve(f.outputDir).startsWith(`${resolve(tempRoot)}${sep}`));
    await rmdir(f.outputDir); // 只能删除本测试创建且仍为空的目录。
    await writeFile(f.outputDir, "模拟证据目录不可写：同名文件占位", { flag: "wx" });
  };
  const acquired = await f.call("acquire-environment-lease", { resources: ["receipts"] });
  assert.equal(acquired.result.status, "blocked");
  await unlink(f.outputDir); // 明确删除本测试上面创建的占位文件。
  await mkdir(f.outputDir);
  const restored = await f.call("restore-environment-resource", { resource: "receipts", acquisitionUnknown: true });
  assert.equal(restored.facts.restoreDisposition, "already-original");
  assert.notEqual(restored.facts.restoreDisposition, "lease-absent-no-mutation");
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.facts.stateMatchesOriginal, true);
  const raw = JSON.parse(await readFile(restored.result.records[0].evidencePath, "utf8"));
  assert.equal(raw.raw.acquisitionAttempts[0].leaseRegistrationAttempted, true);
  assert.equal(raw.raw.acquisitionAttempts[0].phase, "failed-after-registration");
});

test("mock 关闭：第一次清理失败仍持有原租约，只允许继续恢复，确认后才释放所有权", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.onCommand = (method, params) => {
    if (clearingFetch({ method, params })) throw new Error("模拟清理时 CDP 不可用");
  };
  const first = await f.driver.close();
  assert.equal(first.status, "blocked");
  const firstRaw = await originalRecord(first);
  assert.equal(firstRaw.observation.restorationConfirmed, false);
  assert.equal(firstRaw.observation.restoreDisposition, "restore-unconfirmed");
  assert.equal(firstRaw.observation.receiptState, undefined);
  const unchangedFailureBytes = await readFile(first.records[0].evidencePath, "utf8");
  const clearCount = f.state.commands.filter(clearingFetch).length;
  const inspecting = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspecting.result.status, "blocked");
  assert.equal((await originalRecord(inspecting.result)).observation.driverClosed, false);
  const cannotArm = await f.call("drop-question-answer-receipt", {
    threadId: f.threadId, requestId: questionId, onlyAction: "answer",
    preserveServerExecution: true, holdOnlyOriginalReceiptReadsUntilNewDocument: true,
  });
  assert.equal(cannotArm.result.status, "blocked");
  assert.equal(f.state.commands.filter(clearingFetch).length, clearCount, "检查和非法布置故障不隐式重试清理");

  const contender = f.registerDriver(createQuestionReceiptRecoveryDriver(f.driverOptions));
  const contested = await contender.perform(f.request("acquire-environment-lease", { resources: ["receipts"] }));
  assert.equal(contested.status, "blocked", "失败 close 不释放 tab/CDP 所有权");
  const wrong = copy(f.request("restore-environment-resource", { resource: "receipts" }));
  wrong.payload.leaseKey += "-other"; wrong.payload.environmentLease.leaseKey = wrong.payload.leaseKey;
  assert.equal((await f.driver.perform(wrong)).status, "blocked");
  assert.equal(f.state.commands.filter(clearingFetch).length, clearCount);

  f.state.onCommand = null;
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.restorationConfirmed, true);
  const restoredRaw = await originalRecord(restored.result);
  assert.equal(restoredRaw.raw.cleanupAttempts.length, 2);
  assert.match(restoredRaw.raw.cleanupAttempts[0].errors[0], /CDP 不可用/);
  assert.ok(restoredRaw.raw.previousCleanupEvidence.some(record => record.evidencePath === first.records[0].evidencePath));
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.facts.restorationConfirmed, true);
  assert.equal(inspected.facts.driverClosed, false);
  const closed = await f.driver.close();
  assert.equal(closed.status, undefined);
  assert.equal((await originalRecord(closed)).observation.restorationConfirmed, true);
  const afterClose = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(afterClose.facts.driverClosed, true);
  assert.deepEqual(afterClose.facts.currentState, afterClose.facts.originalState);
  const commands = f.state.commands.length;
  assert.deepEqual(await f.driver.close(), closed, "已确认的重复 close 返回原证据，不能清空记录");
  assert.equal(f.state.commands.length, commands);
  assert.equal(await readFile(first.records[0].evidencePath, "utf8"), unchangedFailureBytes);
  const nextOwner = await contender.perform(f.request("acquire-environment-lease", { resources: ["receipts"] }));
  assert.equal(nextOwner.status, undefined, "实际关闭确认后才允许后续所有者获取");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
});

test("mock 关闭：清理超时后保留失败证据，环境恢复后的第二次 close 能重新清理", async t => {
  const f = await fixture(t, { commandTimeoutMs: 40 });
  await f.start(); await f.arm();
  f.state.onCommand = (method, params) => {
    if (clearingFetch({ method, params })) return new Promise(() => {});
  };
  const first = await f.driver.close();
  assert.equal(first.status, "blocked");
  const firstRaw = await originalRecord(first);
  assert.match(firstRaw.raw.cleanupErrors[0], /超时/);
  const inspecting = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspecting.result.status, "blocked");
  assert.equal((await originalRecord(inspecting.result)).observation.driverClosed, false);
  assert.equal(f.state.commands.filter(clearingFetch).length, 1);
  f.state.onCommand = null;
  const second = await f.driver.close();
  assert.equal(second.status, undefined);
  const raw = await originalRecord(second);
  assert.equal(raw.observation.restorationConfirmed, true);
  assert.equal(raw.raw.cleanupAttempts.length, 2);
  assert.equal(raw.raw.cleanupAttempts[0].restorationConfirmed, false);
  assert.equal(raw.raw.cleanupAttempts[1].restorationConfirmed, true);
  assert.equal(f.state.commands.filter(clearingFetch).length, 2);
  assert.equal(f.state.commands.filter(command => command.method === "Fetch.enable" && command.params.patterns.length > 0).length, 1);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
});

test("mock 关闭：并发 close 与显式 restore 共享正在执行的清理，检查等待同一终态", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  f.state.onCommand = async (method, params) => {
    if (clearingFetch({ method, params })) { await gate; return {}; }
  };
  const first = f.driver.close(), second = f.driver.close();
  assert.equal(first, second, "并发 close 直接复用正在执行的 Promise");
  const restoreRequest = f.request("restore-environment-resource", { resource: "receipts" });
  const inspectRequest = f.request("inspect-environment-lease", { resources: ["receipts"] }, false);
  const restoring = f.driver.perform(restoreRequest), inspecting = f.driver.observe(inspectRequest);
  await until(() => f.state.commands.some(clearingFetch));
  assert.equal(f.state.commands.filter(clearingFetch).length, 1);
  release();
  const [closed, concurrentClose, restored, inspected] = await Promise.all([first, second, restoring, inspecting]);
  for (const result of [closed, concurrentClose, restored, inspected]) assert.equal(result.status, undefined);
  await verifyNativeEvidence(restored.records, restoreRequest, { baseDir: f.outputDir });
  await verifyNativeEvidence(inspected.records, inspectRequest, { baseDir: f.outputDir });
  assert.equal((await originalRecord(inspected)).observation.restorationConfirmed, true);
  assert.equal(f.state.commands.filter(clearingFetch).length, 1);
  assert.equal((await originalRecord(closed)).raw.cleanupAttempts.length, 1);
});

test("mock 关闭：两个并发 restore 共享同一次失败，终态后的新调用才开始下一次恢复", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  f.state.onCommand = async (method, params) => {
    if (clearingFetch({ method, params })) {
      await gate;
      throw new Error("模拟共享清理失败");
    }
  };
  const request1 = f.request("restore-environment-resource", { resource: "receipts" });
  const request2 = f.request("restore-environment-resource", { resource: "receipts" });
  const first = f.driver.perform(request1), second = f.driver.perform(request2);
  const inspection = f.driver.observe(f.request("inspect-environment-lease", { resources: ["receipts"] }, false));
  await until(() => f.state.commands.some(clearingFetch));
  release();
  const results = await Promise.all([first, second, inspection]);
  for (const result of results) assert.equal(result.status, "blocked");
  const raw1 = await originalRecord(results[0]), raw2 = await originalRecord(results[1]);
  assert.equal(raw1.observation.cleanupAttemptId, raw2.observation.cleanupAttemptId);
  assert.equal(f.state.commands.filter(clearingFetch).length, 1);
  f.state.onCommand = null;
  const retry = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(retry.facts.restorationConfirmed, true);
  assert.notEqual(retry.facts.cleanupAttemptId, raw1.observation.cleanupAttemptId);
  assert.equal(f.state.commands.filter(clearingFetch).length, 2);
  assert.equal((await f.driver.close()).status, undefined);
});

test("mock 关闭：状态字段相同但清理读取失败时，close/restore/inspect 都保留未确认", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.onReadEvents = options => {
    if (options.timeoutMs === 0) {
      f.state.onReadEvents = null;
      throw new Error("模拟清理时读取暂停事件失败");
    }
  };
  const first = await f.driver.close();
  assert.equal(first.status, "blocked");
  const raw = await originalRecord(first);
  assert.equal(raw.observation.stateMatchesOriginal, true, "本地字段相同不足以证明失败读取没有漏暂停");
  assert.equal(raw.observation.restorationConfirmed, false);
  const inspecting = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspecting.result.status, "blocked");
  assert.equal((await originalRecord(inspecting.result)).observation.restorationConfirmed, false);
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.restorationConfirmed, true);
  const after = await f.driver.close();
  assert.equal(after.status, undefined);
  assert.equal(f.state.commands.filter(clearingFetch).length, 1, "第一次清除已确认，后续只补齐读取核对");
});

test("mock 关闭：事件截断后重复 close 和 restore 持续 blocked，并保留每次证据及所有权", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  f.state.nextTruncated = true;
  assert.equal((await f.unknown()).result.status, "blocked");
  const first = await f.driver.close();
  assert.equal(first.status, "blocked");
  const firstBytes = await readFile(first.records[0].evidencePath, "utf8");
  const second = await f.driver.close();
  assert.equal(second.status, "blocked");
  assert.notEqual(second.records[0].evidencePath, first.records[0].evidencePath);
  const raw = await originalRecord(second);
  assert.equal(raw.observation.restorationConfirmed, false);
  assert.equal(raw.observation.currentState.receipts.unobservedPausedRequestsPossible, true);
  assert.ok(raw.raw.previousCleanupEvidence.some(record => record.evidencePath === first.records[0].evidencePath));
  assert.equal(await readFile(first.records[0].evidencePath, "utf8"), firstBytes);
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.result.status, "blocked");
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.result.status, "blocked");
  assert.equal((await originalRecord(inspected.result)).observation.driverClosed, false);
  const other = f.registerDriver(createQuestionReceiptRecoveryDriver(f.driverOptions));
  assert.equal((await other.perform(f.request("acquire-environment-lease", { resources: ["receipts"] }))).status, "blocked");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"), false);
});

test("mock 读取故障：缺省仍为 network-error，原回执 GET 继续失败，不能冒充 pause 变体", async t => {
  const f = await fixture(t);
  await f.start();
  assert.equal((await f.arm()).facts.receiptReadFault, "network-error");
  f.answer(); await f.unknown();
  f.receiptGet({ networkId: "default-failed-get", fetchId: "fetch-default-failed-get" });
  await until(() => f.state.commands.some(command =>
    command.method === "Fetch.failRequest" && command.params.requestId === "fetch-default-failed-get"));
  assert.equal(f.state.commands.some(command =>
    command.method === "Fetch.continueRequest" && command.params.requestId === "fetch-default-failed-get"), false);
  const raw = await originalRecord((await f.unknown()).result);
  assert.equal(raw.observation.receiptReadFault, "network-error");
});

test("mock pause：只挂起原 ID Fetch/XHR GET，登记后撤旧读，新 Document 的同 loader 才放行", async t => {
  const f = await fixture(t, { crossRealm: true, timeoutMs: 500 });
  await f.start();
  assert.equal((await f.arm("pause")).facts.receiptReadFault, "pause");
  const receipt = f.answer(); await f.unknown();
  const held = f.receiptGet({ networkId: "pause-old-get", fetchId: "fetch-pause-old-get" });
  f.receiptGet({ networkId: "pause-unrelated", fetchId: "fetch-pause-unrelated", receiptId: "other-id" });
  f.receiptGet({ networkId: "pause-document-type", fetchId: "fetch-pause-document-type", resourceType: "Document" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= f.state.sequence);
  assert.equal(f.state.commands.some(command =>
    ["Fetch.failRequest", "Fetch.continueRequest"].includes(command.method)
    && command.params.requestId === "fetch-pause-old-get"), false);
  assert.ok(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-pause-unrelated"));
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-pause-document-type"), false, "Browser Use 管理的 Document 只能观察");
  const heldRaw = await originalRecord((await f.unknown()).result);
  assert.equal(heldRaw.raw.pausedRequests.find(state => state.requestId === "fetch-pause-old-get").held, true);

  let releaseFrame;
  const frameGate = new Promise(resolve => { releaseFrame = resolve; });
  f.state.onCommand = async (method) => {
    if (method === "Page.getFrameTree") { await frameGate; return { frameTree: { frame: copy(f.state.frame) } }; }
  };
  const releasing = f.release();
  await until(() => f.state.commands.filter(command => command.method === "Page.getFrameTree").length === 2);
  f.receiptGet({ networkId: "pause-during-frame", fetchId: "fetch-pause-during-frame", resourceType: "XHR" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= f.state.sequence);
  releaseFrame();
  const released = await releasing;
  assert.equal(released.facts.releaseOnNewDocument, true);
  assert.equal(released.facts.receiptReadFault, "pause");
  f.state.onCommand = null;
  for (const id of ["fetch-pause-old-get", "fetch-pause-during-frame"]) {
    assert.equal(f.state.commands.filter(command => command.method === "Fetch.failRequest"
      && command.params.requestId === id).length, 1);
    assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
      && command.params.requestId === id), false);
  }
  f.receiptGet({ networkId: "pause-after-register", fetchId: "fetch-pause-after-register" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-pause-after-register"));
  await delay(2);
  f.document({ networkId: "pause-reload-document" });
  f.receiptGet({ networkId: "pause-late-old-loader", fetchId: "fetch-pause-late-old-loader" });
  f.receiptGet({ networkId: "pause-new-document-get", fetchId: "fetch-pause-new-document-get",
    loaderId: "loader-reloaded", resourceType: "XHR" });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-pause-new-document-get"));
  assert.ok(f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-pause-late-old-loader"));
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-pause-late-old-loader"), false);
  f.state.networkBodies.set("pause-new-document-get", { body: JSON.stringify(receipt), base64Encoded: false });
  f.emit("Network.responseReceived", { requestId: "pause-new-document-get", timestamp: Date.now() / 1000,
    type: "XHR", response: { status: 200, url: `${origin}/api/receipts/original-receipt`, headers: {} } });
  f.emit("Network.loadingFinished", { requestId: "pause-new-document-get", timestamp: Date.now() / 1000 });
  f.state.metadata.clientReceipts = [];
  const delivery = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(delivery.facts.receiptReadFault, "pause");
  assert.equal(delivery.facts.networkEvents.filter(event =>
    event.method === "Network.requestWillBeSent" && event.params.request.method === "POST"
    && event.params.request.url.endsWith("/api/action/answer")).length, 1);
  const raw = await originalRecord(delivery.result);
  assert.ok(raw.raw.pausedRequests.find(state => state.requestId === "fetch-pause-old-get").retired);
  assert.equal(held.networkEvent.params.loaderId, "loader-initial");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.fulfillRequest"), false);
});

test("mock pause 乱序：同批次 XHR 暂停先于 Fetch 网络元数据，按新主文档身份只放行一次", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown(); await f.release();
  f.document({ networkId: "same-page-document" });
  const request = { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} };
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-same-page-get", networkId: "same-page-get",
    resourceType: "XHR", frameId: "main-frame", request });
  const metadata = f.emit("Network.requestWillBeSent", { requestId: "same-page-get", frameId: "main-frame",
    loaderId: "loader-reloaded", wallTime: Date.now() / 1000, timestamp: Date.now() / 1000,
    type: "Fetch", request });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-same-page-get"));
  const raw = await originalRecord((await f.unknown()).result);
  const pages = (await readFile(raw.raw.journalPath, "utf8")).trim().split("\n").map(line => JSON.parse(line));
  assert.ok(pages.some(page => page.events?.some(event => event.sequence === paused.sequence)
    && page.events.some(event => event.sequence === metadata.sequence)), "两个事件确实来自同一读取批次");
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-same-page-get");
  assert.equal(state.requestMetadata.sequence, metadata.sequence);
  assert.equal(state.requestMetadata.params.type, "Fetch");
  assert.equal(state.pausedEvent.params.resourceType, "XHR");
  assert.equal(state.awaitingNetworkMetadata, false);
  assert.equal(state.allowedNewDocumentRead, true);
  assert.equal(f.state.commands.filter(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-same-page-get").length, 1);
  assert.equal(state.commandAttempts[0].action, "continue");
});

test("mock pause 乱序：跨批次暂停先保留，准确 frame/loader 到齐后继续同一 GET 并完成取证", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); const receipt = f.answer(); await f.unknown(); await f.release();
  f.document({ networkId: "late-metadata-document" });
  const request = { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} };
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-late-metadata-get", networkId: "late-metadata-get",
    resourceType: "XHR", frameId: "main-frame", request });
  await until(() => f.state.reads.at(-1)?.afterSequence >= paused.sequence);
  const held = (await originalRecord((await f.unknown()).result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-late-metadata-get");
  assert.equal(held.held, true);
  assert.equal(held.awaitingNetworkMetadata, true);
  assert.equal(held.requestMetadata, undefined);
  assert.equal(held.allowedNewDocumentRead, false);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-late-metadata-get"), false);

  const metadata = f.emit("Network.requestWillBeSent", { requestId: "late-metadata-get", frameId: "main-frame",
    loaderId: "loader-reloaded", wallTime: Date.now() / 1000, timestamp: Date.now() / 1000,
    type: "Fetch", request });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-late-metadata-get"));
  const body = { body: JSON.stringify(receipt), base64Encoded: false };
  f.state.networkBodies.set("late-metadata-get", body);
  f.emit("Network.responseReceived", { requestId: "late-metadata-get", timestamp: Date.now() / 1000,
    type: "Fetch", response: { status: 200, url: request.url, headers: {} } });
  f.emit("Network.loadingFinished", { requestId: "late-metadata-get", timestamp: Date.now() / 1000 });
  f.state.metadata.clientReceipts = [];
  const delivery = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(delivery.result.status, undefined);
  assert.deepEqual(delivery.facts.responseBodies, [{ requestId: "late-metadata-get", body }]);
  const state = (await originalRecord(delivery.result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-late-metadata-get");
  assert.equal(state.requestMetadata.sequence, metadata.sequence);
  assert.equal(state.awaitingNetworkMetadata, false);
  assert.equal(state.allowedNewDocumentRead, true);
  assert.equal(f.state.commands.filter(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-late-metadata-get").length, 1);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-late-metadata-get"), false);
});

test("mock pause 乱序：GET 元数据先收到而新 Document 后收到，仍按实际文档时间重判暂停", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown(); await f.release();
  const documentTime = Date.now() / 1000;
  const request = { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} };
  f.emit("Fetch.requestPaused", { requestId: "fetch-document-arrives-last", networkId: "document-arrives-last",
    resourceType: "XHR", frameId: "main-frame", request });
  const metadata = f.emit("Network.requestWillBeSent", { requestId: "document-arrives-last", frameId: "main-frame",
    loaderId: "loader-reloaded", wallTime: documentTime + 0.001, timestamp: documentTime + 0.001,
    type: "Fetch", request });
  await until(() => f.state.reads.at(-1)?.afterSequence >= metadata.sequence);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-document-arrives-last"), false);
  const document = f.emit("Network.requestWillBeSent", { requestId: "late-document-event", frameId: "main-frame",
    loaderId: "loader-reloaded", wallTime: documentTime, timestamp: documentTime, type: "Document",
    request: { url: `${origin}/`, method: "GET", headers: {} } });
  await until(() => f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-document-arrives-last"));
  const state = (await originalRecord((await f.unknown()).result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-document-arrives-last");
  assert.ok(metadata.sequence < document.sequence);
  assert.equal(state.allowedNewDocumentRead, true);
  assert.equal(state.commandAttempts.length, 1);
  assert.equal(state.commandAttempts[0].action, "continue");
});

test("mock pause 乱序：最终缺少 Network 元数据时保持挂起并阻塞交付，随后只撤回自身暂停", async t => {
  const f = await fixture(t, { timeoutMs: 50 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown(); await f.release();
  f.document({ networkId: "no-metadata-document" });
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-no-network-metadata", networkId: "no-network-metadata",
    resourceType: "XHR", frameId: "main-frame",
    request: { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} } });
  await until(() => f.state.reads.at(-1)?.afterSequence >= paused.sequence);
  const held = (await originalRecord((await f.unknown()).result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-no-network-metadata");
  assert.equal(held.held, true);
  assert.equal(held.awaitingNetworkMetadata, true);
  assert.equal(held.retired, false);
  assert.equal(held.requestMetadata, undefined);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-no-network-metadata"), false);
  f.state.metadata.clientReceipts = [];
  const delivery = await f.call("observe-question-recovery", { phase: "delivery", originalReceiptId: "original-receipt" }, false);
  assert.equal(delivery.result.status, "blocked");
  assert.match(delivery.result.reason, /未实际完成同 ID 回执 GET/);
  const raw = await originalRecord(delivery.result);
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-no-network-metadata");
  assert.equal(state.allowedNewDocumentRead, false);
  assert.equal(state.requestMetadata, undefined);
  assert.equal(state.commandAttempts.length, 1);
  assert.equal(state.commandAttempts[0].action, "fail");
  assert.equal(state.commandAttempts[0].disposition, "restore-owned-paused-request");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-no-network-metadata"), false);
});

test("mock pause 乱序：刷新登记前缺少准确元数据会阻塞，不提前把未知请求当旧读撤销", async t => {
  const f = await fixture(t, { timeoutMs: 50 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-no-metadata-before-release",
    networkId: "no-metadata-before-release", resourceType: "XHR", frameId: "main-frame",
    request: { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} } });
  await until(() => f.state.reads.at(-1)?.afterSequence >= paused.sequence);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-no-metadata-before-release"), false);
  const release = await f.release();
  assert.equal(release.result.status, "blocked");
  assert.match(release.result.reason, /缺少匹配的原始 Network 终态/);
  const raw = await originalRecord(release.result);
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-no-metadata-before-release");
  assert.equal(raw.raw.fault.releaseReady, false);
  assert.equal(state.requestMetadata, undefined);
  assert.equal(state.allowedNewDocumentRead, false);
  assert.equal(state.commandAttempts.length, 1);
  assert.equal(state.commandAttempts[0].action, "fail");
  assert.equal(state.commandAttempts[0].disposition, "restore-owned-paused-request");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-no-metadata-before-release"), false);
});

test("mock pause 乱序：新 Document 后才到的准确旧 loader 元数据仍只撤销一次，不能按接收顺序放行", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown(); await f.release();
  f.document({ networkId: "old-loader-metadata-document" });
  const request = { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} };
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-delayed-old-loader", networkId: "delayed-old-loader",
    resourceType: "XHR", frameId: "main-frame", request });
  await until(() => f.state.reads.at(-1)?.afterSequence >= paused.sequence);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-delayed-old-loader"), false);
  const metadata = f.emit("Network.requestWillBeSent", { requestId: "delayed-old-loader", frameId: "main-frame",
    loaderId: "loader-initial", wallTime: Date.now() / 1000, timestamp: Date.now() / 1000,
    type: "Fetch", request });
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-delayed-old-loader"));
  const state = (await originalRecord((await f.unknown()).result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-delayed-old-loader");
  assert.equal(state.requestMetadata.sequence, metadata.sequence);
  assert.equal(state.awaitingNetworkMetadata, false);
  assert.equal(state.allowedNewDocumentRead, false);
  assert.equal(state.commandAttempts.length, 1);
  assert.equal(state.commandAttempts[0].action, "fail");
  assert.equal(state.commandAttempts[0].disposition, "retire-old-document-receipt-read");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-delayed-old-loader"), false);
});

test("mock pause 乱序：取消终态先收到，待原 Network 请求到齐后凭 ID 和网络时间退休而不发送控制", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const requestTime = Date.now() / 1000;
  const request = { url: `${origin}/api/receipts/original-receipt`, method: "GET", headers: {} };
  const paused = f.emit("Fetch.requestPaused", { requestId: "fetch-cancel-before-metadata", networkId: "cancel-before-metadata",
    resourceType: "XHR", frameId: "main-frame", request });
  await until(() => f.state.reads.at(-1)?.afterSequence >= paused.sequence);
  f.state.pauses.delete("fetch-cancel-before-metadata");
  const cancellation = f.emit("Network.loadingFailed", { requestId: "cancel-before-metadata",
    timestamp: requestTime + 0.001, errorText: "net::ERR_ABORTED", canceled: true });
  await until(() => f.state.reads.at(-1)?.afterSequence >= cancellation.sequence);
  const unconfirmed = (await originalRecord((await f.unknown()).result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-cancel-before-metadata");
  assert.equal(unconfirmed.retired, false);
  assert.equal(unconfirmed.terminalProof, undefined);
  assert.equal(unconfirmed.commandAttempts.length, 0);
  const metadata = f.emit("Network.requestWillBeSent", { requestId: "cancel-before-metadata", frameId: "main-frame",
    loaderId: "loader-initial", wallTime: requestTime, timestamp: requestTime, type: "Fetch", request });
  await until(() => f.state.reads.at(-1)?.afterSequence >= metadata.sequence);
  const released = await f.release();
  assert.equal(released.result.status, undefined);
  const state = (await originalRecord(released.result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-cancel-before-metadata");
  assert.equal(state.retired, true);
  assert.equal(state.confirmation, "network-terminal");
  assert.equal(state.terminalProof.event.sequence, cancellation.sequence);
  assert.equal(state.terminalProof.request.sequence, metadata.sequence);
  assert.ok(state.terminalProof.event.sequence < state.terminalProof.request.sequence);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-cancel-before-metadata"), false);
});

test("mock pause：Chrome 已取消的旧 GET 只有匹配原始 Network 终态后才退休", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "chrome-canceled-old", fetchId: "fetch-chrome-canceled-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  f.state.pauses.delete("fetch-chrome-canceled-old");
  const cancellation = f.emit("Network.loadingFailed", { requestId: "chrome-canceled-old",
    timestamp: get.networkEvent.params.timestamp + 0.001, errorText: "net::ERR_ABORTED", canceled: true });
  await until(() => f.state.reads.at(-1)?.afterSequence >= cancellation.sequence);
  const release = await f.release();
  assert.equal(release.result.status, undefined);
  const state = (await originalRecord(release.result)).raw.pausedRequests
    .find(value => value.requestId === "fetch-chrome-canceled-old");
  assert.equal(state.retired, true);
  assert.equal(state.confirmation, "network-terminal");
  assert.equal(state.terminalProof.event.method, "Network.loadingFailed");
  assert.equal(state.terminalProof.event.sequence, cancellation.sequence);
  assert.equal(state.terminalProof.request.params.requestId, "chrome-canceled-old");
  assert.equal(f.state.commands.some(command => ["Fetch.failRequest", "Fetch.continueRequest"].includes(command.method)
    && command.params.requestId === "fetch-chrome-canceled-old"), false);
});

test("mock pause：撤销报错后等待后续真实取消事件，保留错误，不凭错误文本认定退休", async t => {
  const f = await fixture(t, { timeoutMs: 500 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "delayed-cancel-old", fetchId: "fetch-delayed-cancel-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  let cancellation;
  f.state.onCommand = (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-delayed-cancel-old") {
      void delay(10).then(() => {
        f.state.pauses.delete(params.requestId);
        cancellation = f.emit("Network.loadingFailed", { requestId: "delayed-cancel-old",
          timestamp: get.networkEvent.params.timestamp + 0.002, errorText: "net::ERR_ABORTED", canceled: true });
      });
      throw new Error("模拟任意 CDP 错误；不能据此认定请求已取消");
    }
  };
  const release = await f.release();
  assert.equal(release.result.status, undefined);
  const raw = await originalRecord(release.result);
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-delayed-cancel-old");
  assert.equal(state.retired, true);
  assert.match(state.commandAttempts[0].error, /任意 CDP 错误/);
  assert.equal(state.terminalProof.event.sequence, cancellation.sequence);
  assert.equal(f.state.commands.filter(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-delayed-cancel-old").length, 1);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-delayed-cancel-old"), false);
});

test("mock pause：Invalid InterceptionId 文本及错 ID/倒退时间的终态不能代替真实取消证据", async t => {
  const f = await fixture(t, { timeoutMs: 40 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "no-proof-old", fetchId: "fetch-no-proof-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  f.emit("Network.loadingFailed", { requestId: "different-request-id",
    timestamp: get.networkEvent.params.timestamp + 0.001, errorText: "net::ERR_ABORTED", canceled: true });
  f.emit("Network.loadingFailed", { requestId: "no-proof-old",
    timestamp: get.networkEvent.params.timestamp - 1, errorText: "net::ERR_ABORTED", canceled: true });
  f.state.onCommand = (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-no-proof-old")
      throw new Error("Invalid InterceptionId");
  };
  const release = await f.release();
  assert.equal(release.result.status, "blocked");
  const raw = await originalRecord(release.result);
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-no-proof-old");
  assert.equal(state.retired, false);
  assert.equal(state.terminalProof, undefined);
  assert.ok(state.invalidTerminals.length > 0);
  assert.match(state.commandAttempts[0].error, /Invalid InterceptionId/);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-no-proof-old"), false);
  assert.equal((await f.driver.close()).status, "blocked");
});

test("mock pause：release 与清理并发时，同一旧 GET 不会同时收到 fail 和 continue", async t => {
  const f = await fixture(t, { timeoutMs: 500 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "concurrent-pause-old", fetchId: "fetch-concurrent-pause-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  let finishFail;
  const gate = new Promise(resolve => { finishFail = resolve; });
  f.state.onCommand = async (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-concurrent-pause-old") {
      await gate;
    }
  };
  const releasing = f.release();
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-concurrent-pause-old"));
  const closing = f.driver.close();
  await delay(5);
  assert.equal(f.state.commands.filter(command => command.params.requestId === "fetch-concurrent-pause-old"
    && ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)).length, 1);
  finishFail();
  const [released, closed] = await Promise.all([releasing, closing]);
  assert.equal(released.result.status, "blocked", "开始清理后不能继续发布刷新闸门");
  assert.equal(closed.status, undefined);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-concurrent-pause-old"), false);
  assert.equal(f.state.metadata.clientReceipts[0].state, "unknown");
});

test("mock pause：旧 GET 尚未退休就出现新主文档时阻塞，不把提前导航算作刷新恢复", async t => {
  const f = await fixture(t, { timeoutMs: 500 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "early-document-old", fetchId: "fetch-early-document-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  let finish;
  const gate = new Promise(resolve => { finish = resolve; });
  f.state.onCommand = async (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-early-document-old") await gate;
  };
  const releasing = f.release();
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-early-document-old"));
  f.document({ networkId: "too-early-document" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= f.state.sequence);
  finish();
  const result = await releasing;
  assert.equal(result.result.status, "blocked");
  assert.match(result.result.reason, /旧回执 GET 退休确认之前/);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-early-document-old"), false);
});

test("mock pause：撤销命令超时仍在执行时，清理不能并发发送另一个控制命令", async t => {
  const f = await fixture(t, { timeoutMs: 50, commandTimeoutMs: 35 });
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "timeout-pause-old", fetchId: "fetch-timeout-pause-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  f.state.onCommand = (method, params) => {
    if (method === "Fetch.failRequest" && params.requestId === "fetch-timeout-pause-old") return new Promise(() => {});
  };
  assert.equal((await f.release()).result.status, "blocked");
  assert.equal((await f.driver.close()).status, "blocked");
  assert.equal(f.state.commands.filter(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "fetch-timeout-pause-old").length, 1);
  f.state.pauses.delete("fetch-timeout-pause-old");
  f.emit("Network.loadingFailed", { requestId: "timeout-pause-old",
    timestamp: get.networkEvent.params.timestamp + 0.001, errorText: "net::ERR_ABORTED", canceled: true });
  assert.equal((await f.driver.close()).status, undefined, "匹配的真实取消证据可以确认请求已经退休");
});

test("mock pause：旧 GET 的实际完成不能伪装成 Chrome 取消", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "unexpected-completed-old", fetchId: "fetch-unexpected-completed-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  const completed = f.emit("Network.loadingFinished", { requestId: "unexpected-completed-old",
    timestamp: get.networkEvent.params.timestamp + 0.001 });
  await until(() => f.state.reads.at(-1)?.afterSequence >= completed.sequence);
  const unknown = await f.unknown();
  assert.equal(unknown.result.status, "blocked");
  assert.match(unknown.result.reason, /旧回执 GET/);
  const raw = await originalRecord(unknown.result);
  const state = raw.raw.pausedRequests.find(value => value.requestId === "fetch-unexpected-completed-old");
  assert.equal(state.terminalProof.event.method, "Network.loadingFinished");
  assert.equal(raw.observation.receiptState, undefined);
});

test("mock pause：普通恢复只撤本租约，取消被挂起的原 GET，不放行旧响应或改写 unknown", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer(); await f.unknown();
  const get = f.receiptGet({ networkId: "ordinary-restore-old", fetchId: "fetch-ordinary-restore-old" });
  await until(() => f.state.reads.at(-1)?.afterSequence >= get.pausedEvent.sequence);
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.restorationConfirmed, true);
  assert.equal(f.state.commands.filter(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-ordinary-restore-old").length, 1);
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-ordinary-restore-old"), false);
  assert.equal(f.state.metadata.clientReceipts[0].state, "unknown");
  assert.equal(f.state.commands.some(command => command.method === "Fetch.fulfillRequest"), false);
  assert.ok(f.state.commands.some(clearingFetch));
});

test("mock pause：网络日志落盘失败发生在处理暂停前，也不能把原 GET 当无关请求放行", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm("pause"); f.answer();
  const unknown = await f.unknown();
  const journalPath = (await originalRecord(unknown.result)).raw.journalPath;
  assert.ok(resolve(journalPath).startsWith(`${resolve(f.outputDir)}${sep}`));
  let finishRead, reading = false;
  const gate = new Promise(resolve => { finishRead = resolve; });
  f.state.onReadEvents = async () => {
    f.state.onReadEvents = null; reading = true;
    await gate;
  };
  const get = f.receiptGet({ networkId: "journal-failure-old", fetchId: "fetch-journal-failure-old" });
  await until(() => reading);
  await unlink(journalPath);
  await mkdir(journalPath); // 本测试创建的同名目录使后续 appendFile 真实失败。
  finishRead();
  await until(() => f.state.commands.some(command => command.method === "Fetch.failRequest"
    && command.params.requestId === "fetch-journal-failure-old"));
  assert.equal(f.state.commands.some(command => command.method === "Fetch.continueRequest"
    && command.params.requestId === "fetch-journal-failure-old"), false);
  assert.equal(f.state.metadata.clientReceipts[0].state, "unknown");
  assert.equal((await f.unknown()).result.status, "blocked");
  assert.ok(get.pausedEvent.sequence > 0);
});

test("mock 读取故障：显式非法模式不能静默退回 network-error", async t => {
  for (const receiptReadFault of [null, "", "drop", 1]) {
    const f = await fixture(t);
    await f.start();
    const result = await f.call("drop-question-answer-receipt", {
      threadId: f.threadId, requestId: questionId, onlyAction: "answer",
      preserveServerExecution: true, holdOnlyOriginalReceiptReadsUntilNewDocument: true, receiptReadFault,
    });
    assert.equal(result.result.status, "blocked");
    assert.match(result.result.reason, /receiptReadFault/);
    assert.equal(f.state.commands.some(command => command.method === "Fetch.enable" && command.params.patterns.length > 0), false);
  }
});

test("mock Document：live4 的 seq51 Document/200 与 seq84 同 ID 终态不阻塞获取，也不归本驱动恢复", async t => {
  const f = await fixture(t);
  const requestId = "interception-job-20892.0", networkId = "8D888FDDA4B951D3D8528EF39A8C0108";
  const request = { url: `${origin}/`, method: "GET", headers: { Cookie: "never-save-managed-cookie" } };
  f.state.sequence = 49;
  f.emit("Network.requestWillBeSent", { requestId: networkId, loaderId: "managed-document-loader",
    frameId: "main-frame", wallTime: Date.now() / 1000, timestamp: Date.now() / 1000,
    type: "Document", request });
  f.emit("Fetch.requestPaused", { requestId, networkId, frameId: "main-frame", resourceType: "Document",
    request, responseStatusCode: 200, responseHeaders: [{ name: "Set-Cookie", value: "never-save-managed-cookie" }] });
  f.state.sequence = 83;
  f.emit("Network.loadingFinished", { requestId: networkId, timestamp: Date.now() / 1000 });
  const acquired = await f.call("acquire-environment-lease", { resources: ["receipts"] });
  assert.equal(acquired.facts.leaseAcquired, true);
  assert.deepEqual(acquired.facts.originalState.receipts.driverPausedRequestIds, []);
  const raw = await originalRecord(acquired.result);
  assert.equal(raw.raw.baseline.events.find(event => event.sequence === 51).params.resourceType, "Document");
  assert.equal(raw.raw.baseline.events.find(event => event.sequence === 84).params.requestId, networkId);
  assert.equal(raw.raw.browserManagedDocumentPauses[0].params.requestId, requestId);
  assert.equal(JSON.stringify(raw).includes("never-save-managed-cookie"), false);
  await f.call("start-question-recovery-observation", { onlyCurrentTab: true, readOnly: true });
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.restorationConfirmed, true);
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.facts.stateMatchesOriginal, true);
  assert.equal((await f.driver.close()).status, undefined);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)), false);
});

test("mock Document：观察期间和布置故障后仍不登记、不控制未结束的 Browser Use Document", async t => {
  const f = await fixture(t);
  await f.start();
  const first = f.emit("Fetch.requestPaused", { requestId: "managed-document-before-arm", networkId: "managed-document-network-1",
    resourceType: "Document", frameId: "main-frame", request: { url: `${origin}/`, method: "GET", headers: {} },
    responseStatusCode: 200 });
  await until(() => f.state.reads.at(-1)?.afterSequence >= first.sequence);
  await f.arm();
  const second = f.emit("Fetch.requestPaused", { requestId: "managed-document-after-arm", networkId: "managed-document-network-2",
    resourceType: "Document", frameId: "main-frame", request: { url: `${origin}/`, method: "GET", headers: {} },
    responseStatusCode: 200 });
  await until(() => f.state.reads.at(-1)?.afterSequence >= second.sequence);
  const restored = await f.call("restore-environment-resource", { resource: "receipts" });
  assert.equal(restored.facts.restorationConfirmed, true);
  assert.deepEqual(restored.facts.currentState.receipts.driverPausedRequestIds, []);
  const raw = await originalRecord(restored.result);
  assert.equal(raw.raw.browserManagedDocumentPauses.length, 2);
  assert.equal(raw.raw.pausedRequests.some(state => state.pausedEvent?.params.resourceType === "Document"), false);
  for (const id of ["managed-document-before-arm", "managed-document-after-arm"]) {
    assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
      && command.params.requestId === id), false);
    assert.equal(f.state.pauses.has(id), true, "本驱动没有替 Browser Use 解除 Document 暂停");
  }
});

test("mock Document：有 Document 观察也不能接管 baseline 中未知的非 Document 暂停", async t => {
  const f = await fixture(t);
  f.emit("Fetch.requestPaused", { requestId: "managed-document-mixed", networkId: "managed-document-mixed-network",
    resourceType: "Document", request: { url: `${origin}/`, method: "GET", headers: {} }, responseStatusCode: 200 });
  f.emit("Fetch.requestPaused", { requestId: "unknown-xhr-before-acquire", networkId: "unknown-xhr-network",
    resourceType: "XHR", request: { url: `${origin}/api/receipts/unknown`, method: "GET", headers: {} } });
  const acquired = await f.call("acquire-environment-lease", { resources: ["receipts"] });
  assert.equal(acquired.result.status, "blocked");
  assert.match(acquired.result.reason, /非 Document/);
  const raw = await originalRecord(acquired.result);
  assert.equal(raw.raw.preexistingPauseObserved, true);
  assert.equal(raw.raw.browserManagedDocumentPauses.length, 1);
  assert.equal(raw.raw.baselineEvents.length, 2);
  assert.equal(f.state.commands.length, 0);
});

test("mock Document：常规观察发现不匹配本驱动 patterns 的非 Document 暂停时拒绝接管", async t => {
  const f = await fixture(t);
  await f.start(); await f.arm();
  const event = f.emit("Fetch.requestPaused", { requestId: "unknown-xhr-other-owner", networkId: "unknown-xhr-other-network",
    resourceType: "XHR", request: { url: `${origin}/api/unowned`, method: "GET", headers: {} }, responseStatusCode: 200 });
  await until(() => f.state.reads.at(-1)?.afterSequence >= event.sequence || f.state.commands.some(clearingFetch));
  const inspected = await f.call("inspect-environment-lease", { resources: ["receipts"] }, false);
  assert.equal(inspected.result.status, "blocked");
  const raw = await originalRecord(inspected.result);
  assert.equal(raw.raw.pausedRequests.some(state => state.requestId === "unknown-xhr-other-owner"), false);
  assert.equal(f.state.commands.some(command => ["Fetch.continueRequest", "Fetch.failRequest"].includes(command.method)
    && command.params.requestId === "unknown-xhr-other-owner"), false);
  assert.equal(f.state.pauses.has("unknown-xhr-other-owner"), true);
});
