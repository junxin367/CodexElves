/**
 * 问答回执恢复的真实 CDP 环境驱动。只接收调用方明确提供的 tab/CDP 和只读元数据入口；
 * 不发现浏览器、不读取凭据、不提交业务请求、不写页面存储。
 *
 * readClientMetadata({tab, scope, threadId, phase, originalReceiptId}) 必须返回实际的
 * {device:{id,createdAtMs,...}, clientReceipts:[完整 question receipt records]}。
 * readTabInfo(tab) 返回实际 {id,url}；省略时使用 tab.url()，不接受缓存的 URL 属性。
 * newTabProof 是调用方对本轮新建且由本驱动独占 Fetch 的 tab 的创建记录：
 * {tabId,runId,createdAt,initialUrl,noExistingFetchInterception:true}。
 *
 * CDP 只使用 send(method, params, options) 和 readEvents({afterSequence,limit,
 * methods,target?,timeoutMs})。所有事件读取始终使用同一 methods 和连续 cursor。
 * 传输值按 JSON 协议在本模块内重新解析，跨 realm 的 Object/Array 原型不参与授权比较。
 * 无租约恢复只接受本实例已记录的、发生于租约登记和任何 CDP 变更之前的获取失败；
 * 新实例或曾尝试变更的未知获取不能被当成无副作用。
 * 清理开始后只允许原租约的恢复/检查。并发清理共享进行中的工作，终态失败可再次恢复；
 * 已确认的关闭保留原证据，未确认的关闭不会释放所有权或伪装成空结果成功。
 * drop-question-answer-receipt.payload.receiptReadFault 缺省为 network-error；
 * pause 只挂起准确原 ID 的 Fetch/XHR GET，登记刷新边界后撤销旧读，新主文档才放行。
 * mock 控制流测试不能作为真实故障 E2E 的通过证据。
 */
import { Buffer } from "node:buffer";
import { createHash, randomUUID } from "node:crypto";
import { appendFile, mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { isDeepStrictEqual } from "node:util";
import { validateRemoteAccessEnvironmentScope } from "./remote-access-e2e-environment.mjs";

export const QUESTION_RECEIPT_DRIVER_VERSION = "remote-e2e-driver/2";
export const QUESTION_RECEIPT_EVENT_METHODS = Object.freeze([
  "Network.requestWillBeSent", "Network.responseReceived",
  "Network.loadingFailed", "Network.loadingFinished", "Fetch.requestPaused",
]);
const operations = Object.freeze([
  "acquire-environment-lease", "restore-environment-resource", "inspect-environment-lease",
  "start-question-recovery-observation", "drop-question-answer-receipt",
  "release-question-receipt-on-reload", "observe-question-recovery",
]);
const ownedCdp = new WeakMap();
const ownedTabs = new Map();
const secretName = /^(?:authorization|proxy-authorization|cookie|set-cookie|x-api-key|api[-_]?key|access[-_]?token|refresh[-_]?token|token|token[-_]?hash|secret|password|pairingCode|pairCode)$/i;
const nonempty = value => typeof value === "string" && value.trim().length > 0;
const iso = () => new Date().toISOString();
// structuredClone 可由宿主 realm 提供，结果的 prototype 不一定属于本模块。
// driver/2 的跨边界数据是 JSON，必须用本模块 JSON.parse 归一化后再严格比较。
const clone = value => JSON.parse(JSON.stringify(value));
const readOnlyCdpMethods = new Set([
  "Page.getFrameTree", "Fetch.getResponseBody", "Network.getResponseBody",
]);

function invariant(value, message) {
  if (!value) throw new Error(message);
}

function url(value) {
  try { return new URL(value); } catch { return null; }
}

function redactedUrl(value) {
  const parsed = url(value);
  if (!parsed) return value;
  parsed.username = ""; parsed.password = ""; parsed.hash = "";
  for (const name of [...parsed.searchParams.keys()])
    if (secretName.test(name) || /^(?:auth|credential|ticket|code)$/i.test(name))
      parsed.searchParams.set(name, "[redacted]");
  return parsed.href;
}

function redact(value, key = "") {
  if (value == null || typeof value !== "object") {
    if (typeof value === "string" && /^(?:url|documentURL|initialUrl|currentUrl)$/i.test(key))
      return redactedUrl(value);
    if (typeof value === "string" && key === "postData") {
      try { return JSON.stringify(redact(JSON.parse(value))); }
      catch { return "[redacted non-JSON request body]"; }
    }
    return value;
  }
  if (/headers$/i.test(key)) {
    if (Array.isArray(value)) return value.filter(item => /^idempotency-key$/i.test(item.name));
    return Object.fromEntries(Object.entries(value).filter(([name]) => /^idempotency-key$/i.test(name)));
  }
  if (Array.isArray(value)) return value.map(item => redact(item));
  return Object.fromEntries(Object.entries(value).filter(([name]) => !secretName.test(name))
    .map(([name, item]) => [name, redact(item, name)]));
}

function errorText(error) {
  return String(error?.message || error).replace(/Bearer\s+\S+/gi, "Bearer [redacted]")
    .replace(/((?:token|password|secret|api[-_]?key)=)[^&\s]+/gi, "$1[redacted]");
}

async function deadline(promise, milliseconds, label) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} 超时；结果仍未知`)), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}

function parseBody(body) {
  invariant(typeof body?.body === "string" && typeof body.base64Encoded === "boolean",
    "CDP 未返回实际响应正文");
  return JSON.parse(Buffer.from(body.body, body.base64Encoded ? "base64" : "utf8").toString("utf8"));
}

function successfulReceipt(value, fault) {
  return value?.requestId === fault.originalReceiptId && value.state === "completed"
    && value.result?.ok === true && value.result.outcomeUnknown !== true
    && value.result.threadId === fault.threadId && value.result.requestId === fault.questionRequestId;
}

function requestBody(request) {
  try { return JSON.parse(request.postData); } catch { return null; }
}

function answerMatches(request, target, origin) {
  const address = url(request?.url), body = requestBody(request || {});
  return request?.method === "POST" && address?.origin === origin
    && address.pathname === "/api/action/answer" && body?.threadId === target.threadId
    && body.requestId === target.questionRequestId && body.method === "remote/requestUserInputAsync"
    && body.response && typeof body.response === "object" && body.response.skip !== true
    && body.response.answers && typeof body.response.answers === "object";
}

function idempotencyKey(request) {
  return Object.entries(request?.headers || {}).find(([name]) => /^idempotency-key$/i.test(name))?.[1];
}

/**
 * 返回值可直接放入 runner 的 drivers.environment。close() 供调用方 finally 使用；
 * readAnswerReceiptEvidence(request, action) 只把已捕获的真实恢复 GET 关联到原 answer 动作，
 * 供 nativeEvidence 最终复核使用，不发送、补发或重新查询业务请求。
 */
export function createQuestionReceiptRecoveryDriver({
  tab, cdp = tab?.cdp, outputDir, scope, readClientMetadata, readTabInfo,
  newTabProof, eventTarget, commandOptions,
  timeoutMs = 20_000, commandTimeoutMs = 5_000, pollIntervalMs = 25, eventPollTimeoutMs = 200,
} = {}) {
  invariant(tab && tab.id != null, "回执驱动必须明确提供真实 tab");
  invariant(cdp && typeof cdp.send === "function" && typeof cdp.readEvents === "function",
    "回执驱动需要明确绑定该 tab 的 send/readEvents CDP");
  invariant(nonempty(outputDir) && typeof readClientMetadata === "function", "缺少输出目录或真实只读客户端元数据入口");
  invariant(scope?.kind === "authorized-tab" && scope.tabId === tab.id, "回执驱动只接受当前 authorized-tab 的 receipts 租约");
  const root = resolve(outputDir), driverToken = randomUUID();
  const targetScope = clone(scope), proof = clone(newTabProof || {});
  const normalizedEventTarget = eventTarget === undefined ? undefined : clone(eventTarget);
  const normalizedCommandOptions = commandOptions === undefined ? undefined : clone(commandOptions);
  const readTab = readTabInfo || (async value => {
    invariant(typeof value.url === "function", "tab 没有实时 url()，必须提供只读 readTabInfo");
    return { id: value.id, url: await value.url() };
  });
  invariant([timeoutMs, commandTimeoutMs, pollIntervalMs, eventPollTimeoutMs].every(value =>
    Number.isSafeInteger(value) && value > 0), "驱动超时参数必须是正整数");
  invariant(eventPollTimeoutMs <= 1_000, "事件轮询最长为 1000ms，以保证异常恢复及时执行");

  let lease = null, lastRequest = null, cursor = 0, baseline = null, frame = null;
  let observation = null, fault = null, loopTask = null, loopError = null;
  let stopRequested = false, restoring = false, recoveryTask = null, closed = false;
  let cleanupStarted = false, lastRecovery = null, closeTask = null, closeResult = null;
  let fetchPossiblyOwned = false, fetchPatterns = [], cleanupUncertain = false;
  let serial = Promise.resolve(), eventReadTail = Promise.resolve();
  let acquisitionAttempt = null, absentRecovery = null, preexistingPauseObserved = false;
  const acquisitionAttempts = [], mutationAttempts = [];
  const cleanupAttempts = [], cleanupEvidence = [], closeAttempts = [];
  const baselineEvents = [], browserManagedDocumentPauses = [];
  const documentSequences = new Set();
  const pendingPaused = new Map(), commands = [], requests = new Map(), responses = new Map();
  const completed = new Map(), failed = new Map(), bodies = new Map(), bodyErrors = [];
  const pauseStates = new Map(), requestEvents = new Map(), terminalEvents = new Map();

  async function send(method, params = {}, execution) {
    const command = { method, params: redact(clone(params)), startedAt: iso() };
    commands.push(command);
    if (!readOnlyCdpMethods.has(method)) mutationAttempts.push(command);
    try {
      const wire = Promise.resolve().then(() => cdp.send(method, params, normalizedCommandOptions));
      if (execution) {
        execution.pending = true;
        wire.then(result => {
          execution.pending = false;
          command.wireSettledAt = iso();
          try { command.wireResult = redact(clone(result ?? {})); execution.onAck(); }
          catch (error) { command.wireError = errorText(error); }
        }, error => {
          execution.pending = false; command.wireSettledAt = iso(); command.wireError = errorText(error);
        });
      }
      const wireResult = await deadline(wire, commandTimeoutMs, method);
      const result = clone(wireResult ?? {});
      command.finishedAt = iso(); command.result = redact(clone(result ?? {}));
      return result;
    } catch (error) {
      command.finishedAt = iso(); command.error = errorText(error);
      throw error;
    }
  }

  async function actualTab() {
    const value = clone(await deadline(Promise.resolve().then(() => readTab(tab)), commandTimeoutMs, "读取授权 tab"));
    invariant(value?.id === tab.id && url(value.url)?.origin === targetScope.origin,
      "授权 tab 的实际 identity 或当前 URL 已离开租约来源");
    invariant(Number(url(value.url).port) !== 45222, "禁止对 45222 用户服务进行回执故障演练");
    return redact(clone(value));
  }

  function ownedState() {
    return { receipts: {
      tabId: tab.id, driverObservationActive: observation?.active === true,
      driverFetchPatterns: clone(fetchPatterns),
      driverPausedRequestIds: [...pendingPaused.keys()].sort(),
      driverAnswerDropArmed: fault?.enabled === true && !fault.dropAttempted,
      driverReceiptReadBlocked: fault?.enabled === true && fault.dropAttempted && !fault.releaseDocument,
      driverReleaseOnNewDocument: fault?.releaseArmed === true && !fault.releaseDocument,
      unobservedPausedRequestsPossible: cleanupUncertain,
    } };
  }

  function scopeFacts(binding) {
    return {
      leaseKey: binding.leaseKey, leaseOwnerRunId: binding.runId,
      instanceId: targetScope.instanceId, scopeKind: targetScope.kind,
      servicePort: Number(targetScope.servicePort || url(targetScope.origin).port), tabId: tab.id,
      unownedStateChanged: false,
    };
  }

  async function save(request, fields, raw = {}, {
    action, outcomePointer = "/observation", completedControl = false, controlBinding = lease,
  } = {}) {
    await mkdir(root, { recursive: true });
    const readId = fields.readId || randomUUID(), actionId = action?.id ?? request.actionId ?? request.actions?.[0]?.id;
    const value = clone({
      capture: { runId: request.runId, caseId: request.caseId, origin: request.origin,
        sourceFingerprint: request.sourceFingerprint, capturedAt: iso() },
      observation: {
        operation: request.operation, threadId: request.threadId, readId,
        ...(controlBinding ? scopeFacts(controlBinding) : {}), ...(actionId ? { actionId } : {}), ...fields,
        ...(completedControl ? { receiptKind: "driver-control", receiptState: "completed" } : {}),
      },
      raw: redact(clone({ ...raw, browserManagedDocumentPauses })),
    });
    const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
    const path = resolve(root, `question-receipt-${request.operation}-${readId.replace(/[^a-zA-Z0-9-]/g, "-")}-${randomUUID()}.json`);
    await writeFile(path, bytes, { flag: "wx" });
    const rawFacts = Object.fromEntries(Object.keys(value.observation).map(name => [name, `/observation/${name}`]));
    for (const name of ["runId", "caseId", "origin", "sourceFingerprint", "capturedAt"])
      rawFacts[name] = `/capture/${name}`;
    return {
      source: "bridge", evidencePath: path, size: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      threadId: value.observation.threadId, readId,
      ...(actionId ? { actionId } : {}), outcomePointer, rawFacts,
    };
  }

  async function saveCleanup(request, fields, raw = {}, options = {}) {
    const record = await save(request, fields, {
      ...raw, pausedRequests: pausedEvidence(), cleanupAttempts, closeAttempts, previousCleanupEvidence: cleanupEvidence,
    }, options);
    cleanupEvidence.push(clone(record));
    return record;
  }

  function assertOperational() {
    if (loopError) throw new Error(`CDP 后台观察失败，仅允许恢复原租约，原回答保持未知：${errorText(loopError)}`);
    invariant(!closed && !cleanupStarted, "清理已开始，只允许按同一原租约继续恢复或检查；不能重新获取或布置故障");
  }

  async function frameInfo() {
    const result = await send("Page.getFrameTree");
    const value = result?.frameTree?.frame;
    invariant(nonempty(value?.id) && nonempty(value.loaderId) && url(value.url)?.origin === targetScope.origin,
      "缺少授权 tab 当前主 frame 和 loader 的真实身份");
    return clone(value);
  }

  function requestBinding(request) {
    invariant(!closed || ["restore-environment-resource", "inspect-environment-lease"].includes(request?.operation),
      "回执驱动已关闭，只允许原租约的恢复结果核对");
    invariant(request?.contractVersion === QUESTION_RECEIPT_DRIVER_VERSION
      && operations.includes(request.operation), "回执驱动协议或 operation 不支持");
    invariant(request.caseId === "recovery.question_receipt" && nonempty(request.sourceFingerprint),
      "回执驱动仅接受 recovery.question_receipt 及本轮源码指纹");
    invariant(request.origin === targetScope.origin && resolve(request.outputDir) === root,
      "回执驱动的来源或输出目录不匹配");
    invariant(nonempty(request.threadId) && request.ownedThreadIds?.includes(request.threadId),
      "回执驱动不能操作本轮之外的会话");
    validateRemoteAccessEnvironmentScope(targetScope, {
      runId: request.runId, origin: request.origin, tabId: tab.id, resources: ["receipts"],
    });
    const scopes = [request.payload?.scope, request.payload?.environmentLease?.scope]
      .filter(value => value !== undefined);
    invariant(scopes.length > 0 && scopes.every(value => isDeepStrictEqual(value, targetScope)),
      "请求没有绑定同一份明确的 authorized-tab scope");
    const keys = [request.payload?.leaseKey, request.payload?.environmentLease?.leaseKey]
      .filter(value => value !== undefined);
    const key = keys[0];
    invariant(nonempty(key) && key.includes(request.runId) && keys.every(value => value === key),
      "回执请求缺少一致的本轮原 leaseKey");
    invariant((request.actions || []).length <= 1, "环境步骤只能绑定一个具体动作");
    return {
      leaseKey: key, runId: request.runId, caseId: request.caseId,
      threadId: request.threadId, sourceFingerprint: request.sourceFingerprint,
      origin: request.origin, scope: clone(targetScope),
    };
  }

  function sameBinding(left, right) {
    return Boolean(left && right && ["leaseKey", "runId", "caseId", "threadId", "sourceFingerprint", "origin"]
      .every(name => left[name] === right[name]) && isDeepStrictEqual(left.scope, right.scope));
  }

  function validate(request, { acquisition = false, cleanup = false, allowAbsent = false } = {}) {
    const binding = requestBinding(request);
    const existing = lease || (allowAbsent ? absentRecovery?.binding || acquisitionAttempt?.binding : null);
    if (!acquisition) invariant(sameBinding(binding, existing), "回执操作不属于本驱动记录的原租约或获取尝试");
    if (!cleanup) {
      assertOperational();
      invariant(!lease?.restored, "原回执租约已经恢复，不能重新执行故障");
    }
    lastRequest = clone(request);
    return binding;
  }

  function acquisitionStep(phase, detail = {}) {
    if (!acquisitionAttempt) return;
    acquisitionAttempt.phase = phase;
    acquisitionAttempt.steps.push({ phase, capturedAt: iso(), ...detail });
  }

  function noMutationProof(binding) {
    return Boolean(sameBinding(binding, acquisitionAttempt?.binding) && acquisitionAttempt.failedBeforeLease === true
      && acquisitionAttempt.leaseRegistrationAttempted === false && !acquisitionAttempt.ownershipRejected
      && !lease && !observation && !fault && mutationAttempts.length === 0
      && !fetchPossiblyOwned && fetchPatterns.length === 0 && pendingPaused.size === 0
      && !preexistingPauseObserved && !cleanupUncertain
      && !ownedCdp.has(cdp) && !ownedTabs.has(tab.id)
      && isDeepStrictEqual(ownedState(), acquisitionAttempt.originalState));
  }

  async function restoreAbsentLease(request) {
    const binding = requestBinding(request);
    invariant(request.payload.acquisitionUnknown === true && noMutationProof(binding),
      "没有本实例获取前无副作用的完整证据，不能把未知获取当作 lease-absent-no-mutation");
    const suppliedOriginal = request.payload.originalState;
    invariant(suppliedOriginal == null || isDeepStrictEqual(suppliedOriginal, acquisitionAttempt.originalState),
      "无租约恢复的原状态与本实例保存的获取前状态不一致");
    const currentState = ownedState();
    const record = await save(request, {
      resource: "receipts", restoreDisposition: "lease-absent-no-mutation", leaseAcquired: false,
      originalState: acquisitionAttempt.originalState, currentState, stateMatchesOriginal: true,
    }, { acquisitionAttempts, mutationAttempts, commands, newTabProof: proof,
      restoration: { noCdpCommandIssued: true, capturedAt: iso() } },
    { completedControl: true, controlBinding: binding });
    absentRecovery = { binding, originalState: clone(acquisitionAttempt.originalState), record };
    return { records: [record] };
  }

  async function readPageMetadata(phase) {
    const actual = await actualTab();
    const original = clone(await deadline(Promise.resolve().then(() => readClientMetadata({
      tab, scope: clone(targetScope), threadId: lease.threadId, phase,
      originalReceiptId: fault?.originalReceiptId,
    })), commandTimeoutMs, "读取真实客户端回执元数据"));
    invariant(nonempty(original?.device?.id) && Number.isFinite(original.device.createdAtMs)
      && Array.isArray(original.clientReceipts), "客户端元数据缺少真实 device 或完整 clientReceipts");
    const rawClientMetadata = redact(clone(original));
    // 身份只取合同规定的稳定原始字段；lastSeenAtMs 等活动字段仍保留在 raw，
    // 不把正常访问时间变化误判成设备换授权。question receipts 一项也不筛掉。
    return { actualTab: actual, rawClientMetadata, metadata: {
      ...rawClientMetadata, device: { id: original.device.id, createdAtMs: original.device.createdAtMs },
    } };
  }

  function readArguments(timeout) {
    return { afterSequence: cursor, limit: 1000, methods: [...QUESTION_RECEIPT_EVENT_METHODS],
      ...(normalizedEventTarget !== undefined ? { target: normalizedEventTarget } : {}), timeoutMs: timeout };
  }

  async function readEvents(timeout) {
    const page = clone(await deadline(Promise.resolve().then(() => cdp.readEvents(readArguments(timeout))),
      commandTimeoutMs + timeout, "读取 CDP 事件"));
    invariant(page && Number.isSafeInteger(page.cursor) && page.cursor >= cursor
      && Array.isArray(page.events) && typeof page.hasMore === "boolean"
      && typeof page.truncated === "boolean", "CDP 事件分页缺少有效 cursor/hasMore/truncated");
    let previous = cursor;
    for (const event of page.events) {
      invariant(QUESTION_RECEIPT_EVENT_METHODS.includes(event.method) && Number.isSafeInteger(event.sequence)
        && event.sequence > previous && event.sequence <= page.cursor, "CDP 事件顺序、methods 或 cursor 无效");
      previous = event.sequence;
    }
    invariant(!page.hasMore || page.cursor > cursor, "CDP hasMore 未推进 cursor，不能可靠取完事件");
    return page;
  }

  function pausedEvidence() {
    // retired 表示 Fetch 暂停已由命令确认或原始 Network 终态解除，不表示业务成功。
    return [...pauseStates.values()].map(state => ({
      requestId: state.params.requestId, networkId: state.params.networkId,
      pausedEvent: state.event, held: state.held, originalReceiptRead: state.originalReceiptRead,
      awaitingNetworkMetadata: state.awaitingNetworkMetadata,
      requestMetadata: requestEvents.get(state.params.networkId),
      allowedNewDocumentRead: state.allowedNewDocumentRead, action: state.action,
      commandStillExecuting: state.execution?.pending === true,
      retired: state.retired, confirmation: state.confirmation, terminalProof: state.terminalProof,
      commandAttempts: state.commandAttempts, invalidTerminals: state.invalidTerminals,
    }));
  }

  function managedDocumentPause(event) {
    return event.method === "Fetch.requestPaused" && event.params?.resourceType === "Document";
  }

  function expectedOwnedPause(event) {
    const paused = event.params, address = url(paused?.request?.url);
    if (!fault || !["Fetch", "XHR"].includes(paused?.resourceType) || address?.origin !== targetScope.origin)
      return false;
    const responseStage = paused.responseStatusCode !== undefined || paused.responseErrorReason !== undefined;
    return address.pathname.startsWith("/api/action/answer") && responseStage
      || address.pathname.startsWith("/api/receipts/") && !responseStage;
  }

  function registerPaused(event) {
    invariant(!managedDocumentPause(event), "Browser Use 管理的 Document 暂停不能登记为本驱动所有");
    const id = event.params.requestId, previous = pauseStates.get(id);
    if (previous) {
      invariant(isDeepStrictEqual(previous.params, event.params), "同一 Fetch requestId 的暂停身份发生变化");
      return previous;
    }
    const receiptRead = Boolean(originalReceiptRead(event.params));
    const state = { params: event.params, event,
      held: receiptRead && fault.receiptReadFault === "pause" && !fault.releaseArmed && !restoring,
      originalReceiptRead: receiptRead,
      awaitingNetworkMetadata: false, allowedNewDocumentRead: false, retired: false, action: null, inFlight: null,
      commandAttempts: [], invalidTerminals: [] };
    pauseStates.set(id, state); pendingPaused.set(id, event.params);
    return state;
  }

  function matchingTerminal(state, event) {
    const networkId = state.params.networkId, started = requestEvents.get(networkId);
    return nonempty(networkId) && started && event.params?.requestId === networkId
      && ["Network.loadingFailed", "Network.loadingFinished"].includes(event.method)
      && Number.isFinite(event.params.timestamp) && Number.isFinite(started.params.timestamp)
      && event.params.timestamp >= started.params.timestamp
      && started.params.request?.url === state.params.request?.url
      && started.params.request?.method === state.params.request?.method
      && (event.method !== "Network.loadingFailed" || nonempty(event.params.errorText));
  }

  function confirmTerminal(state) {
    const events = terminalEvents.get(state.params.networkId) || [];
    const terminal = events.find(event => matchingTerminal(state, event));
    if (!terminal) return false;
    if (state.originalReceiptRead && newDocumentRead(state)) state.allowedNewDocumentRead = true;
    state.terminalProof = { request: requestEvents.get(state.params.networkId), event: terminal };
    state.retired = true; state.held = false;
    state.confirmation ||= "network-terminal";
    pendingPaused.delete(state.params.requestId);
    invariant(restoring || terminal.method !== "Network.loadingFinished" || !state.originalReceiptRead
      || state.allowedNewDocumentRead || fault?.receiptReadFault !== "pause",
    "受控旧回执 GET 实际完成，不能当作 Chrome 取消或刷新恢复");
    return true;
  }

  function originalReceiptRead(paused) {
    const address = url(paused.request?.url);
    return paused.request?.method === "GET" && ["Fetch", "XHR"].includes(paused.resourceType)
      && address?.origin === targetScope.origin && fault?.originalReceiptId
      && address.pathname === `/api/receipts/${encodeURIComponent(fault.originalReceiptId)}`;
  }

  function newDocumentRead(state) {
    const document = fault?.releaseDocument, request = requestEvents.get(state.params.networkId);
    // Fetch 与 Network 的到达顺序不固定。null 是未能判断，不能当成旧文档读而撤销。
    if (!request || !nonempty(request.params.frameId) || !nonempty(request.params.loaderId)
      || !Number.isFinite(request.params.wallTime)
      || request.params.request?.url !== state.params.request?.url
      || request.params.request?.method !== state.params.request?.method) return null;
    if (!document) {
      const previous = fault?.releaseFrame;
      if (!previous || request.params.frameId !== previous.id || request.params.loaderId === previous.loaderId)
        return false;
      return null; // 同 frame 的另一 loader 要等准确的新 Document 身份，不能按到达先后猜。
    }
    if (request.params.frameId !== document.params.frameId || request.params.loaderId !== document.params.loaderId)
      return false;
    return request.params.wallTime >= document.params.wallTime ? true : null;
  }

  function pendingOldReceiptReads() {
    return [...pauseStates.values()].filter(state =>
      state.originalReceiptRead && !state.allowedNewDocumentRead && !state.retired);
  }

  function controlPaused(requestId, action, disposition, {
    attemptKey = randomUUID(), retry = false, allowTerminalProof = false, deferError = false,
    requireCommandAck = false, allowActionChange = false,
  } = {}) {
    const state = pauseStates.get(requestId);
    if (!state) return Promise.resolve({ confirmed: true, noOwnedPause: true });
    if (state.inFlight) return state.inFlight;
    if (allowTerminalProof) confirmTerminal(state);
    if (state.retired) {
      if (requireCommandAck) invariant(state.confirmation === "cdp-ack", "原 POST 已终止，但本驱动丢回包命令未获确认");
      return Promise.resolve({ confirmed: true, confirmation: state.confirmation });
    }
    if (state.execution?.pending) {
      const result = { confirmed: false, error: "同一暂停请求的前次 CDP 控制命令仍在执行，不能发送第二种控制" };
      return deferError ? Promise.resolve(result) : Promise.reject(new Error(result.error));
    }
    // 同一暂停请求一次只执行一种控制。旧回执读固定撤销；原 POST 的失败命令已结束后，
    // 只有清理可以顺序放行。命令尚未返回时绝不并发发送 continue 和 fail。
    const previous = state.commandAttempts.at(-1);
    if (previous && (!retry || previous.attemptKey === attemptKey)) {
      const result = { confirmed: false, error: previous.error || "暂停请求的控制结果仍未知" };
      if (!deferError) return Promise.reject(new Error(result.error));
      return Promise.resolve(result);
    }
    if (!state.action || allowActionChange && previous?.error) state.action = action;
    const attempt = { attemptKey, action: state.action, disposition, startedAt: iso() };
    state.commandAttempts.push(attempt);
    const execution = { pending: false, onAck: () => {
      if (state.execution !== execution || state.retired) return;
      state.retired = true; state.held = false; state.confirmation = "cdp-ack";
      pendingPaused.delete(requestId);
      attempt.wireAcknowledgedAt = iso();
    } };
    state.execution = execution;
    const task = (async () => {
      try {
        await send(state.action === "fail" ? "Fetch.failRequest" : "Fetch.continueRequest",
          { requestId, ...(state.action === "fail" ? { errorReason: "Aborted" } : {}) }, execution);
        state.retired = true; state.held = false; state.confirmation = "cdp-ack";
        pendingPaused.delete(requestId);
        Object.assign(attempt, { finishedAt: iso(), confirmation: "cdp-ack" });
        observation?.controls.push({ requestId, disposition, action: state.action, capturedAt: iso() });
        return { confirmed: true, confirmation: "cdp-ack" };
      } catch (error) {
        Object.assign(attempt, { finishedAt: iso(), error: errorText(error) });
        observation?.controls.push({ requestId, disposition, action: state.action,
          capturedAt: iso(), controlError: errorText(error) });
        if (allowTerminalProof && confirmTerminal(state)) {
          attempt.confirmation = "network-terminal";
          return { confirmed: true, confirmation: "network-terminal" };
        }
        if (deferError) return { confirmed: false, error: errorText(error) };
        throw error;
      }
    })();
    state.inFlight = task;
    const settled = () => { if (state.inFlight === task) state.inFlight = null; };
    task.then(settled, settled);
    return task;
  }

  function continuePaused(requestId, disposition = "continue-unrelated-request", options = {}) {
    return controlPaused(requestId, "continue", disposition, options);
  }

  function failPaused(requestId, disposition, options = {}) {
    return controlPaused(requestId, "fail", disposition, options);
  }

  async function handlePaused(event) {
    // Browser Use 的 Document 导航只观察；即使没有终态，也绝不恢复或控制它。
    if (managedDocumentPause(event)) return;
    const paused = event.params, request = paused.request, state = pauseStates.get(paused.requestId);
    if (originalReceiptRead(paused)) {
      state.originalReceiptRead = true;
      const documentRead = newDocumentRead(state);
      if (fault.receiptReadFault === "pause") {
        state.awaitingNetworkMetadata = documentRead === null;
        if (confirmTerminal(state)) return;
        if (documentRead === null && !restoring) {
          state.held = true;
          observation.controls.push({ requestId: paused.requestId,
            disposition: "wait-exact-network-request-metadata", capturedAt: iso() });
          return;
        }
      }
      if (fault.receiptReadFault === "pause" && documentRead !== true) {
        if (!restoring && fault.enabled && !fault.releaseArmed) {
          state.held = true;
          if (confirmTerminal(state)) return;
          observation.controls.push({ requestId: paused.requestId,
            disposition: "pause-original-receipt-until-reload-registration", capturedAt: iso() });
          return;
        }
        return failPaused(paused.requestId, documentRead === null
          ? "restore-receipt-read-without-network-metadata" : "retire-old-document-receipt-read", {
          attemptKey: fault.releaseAttemptKey || `event-${event.sequence}`,
          allowTerminalProof: true, deferError: true,
        });
      }
      if (fault.receiptReadFault === "pause") state.allowedNewDocumentRead = true;
    }
    if (restoring || !fault?.enabled) return continuePaused(paused.requestId, "restore-or-unrelated-request", {
      allowTerminalProof: true, deferError: true,
    });
    if (state.originalReceiptRead && !fault.releaseDocument) {
      // 立即失败使 finally 的自动核对不能抢先完成；不制造 HTTP 响应，也不悬挂旧文档请求。
      return failPaused(paused.requestId, "block-original-receipt-until-new-document");
    }
    if (fault.dropAttempted || !answerMatches(request, fault, targetScope.origin)
      || !Number.isInteger(paused.responseStatusCode)) return continuePaused(paused.requestId);
    const network = requests.get(paused.networkId);
    invariant(nonempty(paused.networkId) && network && answerMatches(network.request, fault, targetScope.origin),
      "目标 answer 的 Fetch 与原 POST Network.requestWillBeSent 未准确关联");
    const originalId = idempotencyKey(request), originalNetworkId = idempotencyKey(network.request);
    invariant(nonempty(originalId) && originalId === originalNetworkId, "目标 answer 缺少一致的真实 Idempotency-Key");
    fault.originalReceiptId = originalId; fault.networkRequestId = paused.networkId;
    fault.fetchRequestId = paused.requestId;
    invariant(paused.responseStatusCode >= 200 && paused.responseStatusCode < 300,
      "真实 answer HTTP 未成功，不执行成功回包丢失故障");
    const body = await send("Fetch.getResponseBody", { requestId: paused.requestId });
    const receipt = parseBody(body);
    fault.answerResponseBody = clone(body); fault.answerReceipt = clone(receipt);
    fault.answerPausedEvent = clone(event); fault.answerPost = clone(network);
    invariant(successfulReceipt(receipt, fault), "真实 answer 回执不是同一原题的 completed/ok，不丢弃或伪造成功正文");
    if (restoring) return continuePaused(paused.requestId, "restore-before-answer-drop");
    // 在发送 failRequest 前先封死一次性闸门；命令超时也不会再次丢回答或重放。
    fault.dropAttempted = true;
    await failPaused(paused.requestId, "drop-one-completed-answer-response", { requireCommandAck: true });
    fault.dropAcknowledgedAt = iso();
  }

  async function handleEvent(event) {
    const params = event.params;
    if (event.method === "Fetch.requestPaused") return handlePaused(event);
    if (event.method === "Network.requestWillBeSent") {
      if (requests.has(params.requestId)) {
        observation.eventsTruncated = true;
        throw new Error("同一 Network requestId 出现重定向或重复记录，当前专项不能证明完整唯一请求");
      }
      requests.set(params.requestId, params);
      requestEvents.set(params.requestId, event);
      if (fault?.releaseArmed && !fault.releaseDocument && !restoring
        && event.sequence > fault.releaseAfterSequence && params.type === "Document"
        && params.frameId === fault.releaseFrame.id && nonempty(params.loaderId)
        && params.loaderId !== fault.releaseFrame.loaderId
        && url(params.request?.url)?.origin === targetScope.origin
        && params.wallTime * 1000 >= Date.parse(fault.releaseArmedAt)) {
        invariant(fault.receiptReadFault !== "pause" || fault.releaseReady,
          "新主文档在已知旧回执 GET 退休确认之前开始，不能验收刷新恢复");
        fault.releaseDocument = clone(event);
        observation.controls.push({ disposition: "release-at-new-document", capturedAt: iso(), event: clone(event) });
      }
      for (const state of pauseStates.values()) {
        if (state.awaitingNetworkMetadata && !state.retired
          && (state.params.networkId === params.requestId || fault?.releaseDocument?.sequence === event.sequence))
          await handlePaused(state.event);
      }
    } else if (event.method === "Network.responseReceived") {
      responses.set(params.requestId, params);
    } else if (event.method === "Network.loadingFailed") {
      failed.set(params.requestId, params);
      for (const state of pauseStates.values()) {
        if (state.params.networkId !== params.requestId) continue;
        if (matchingTerminal(state, event)) confirmTerminal(state);
        else state.invalidTerminals.push(clone(event));
      }
    } else if (event.method === "Network.loadingFinished") {
      completed.set(params.requestId, params);
      for (const state of pauseStates.values()) {
        if (state.params.networkId !== params.requestId) continue;
        if (!matchingTerminal(state, event)) { state.invalidTerminals.push(clone(event)); continue; }
        confirmTerminal(state);
        invariant(restoring || !state.originalReceiptRead || state.allowedNewDocumentRead
          || fault?.receiptReadFault !== "pause",
        "受控旧回执 GET 在新文档前实际完成，不能当作 Chrome 取消或刷新恢复");
      }
      const request = requests.get(params.requestId), address = url(request?.request?.url);
      if (request?.request?.method !== "GET" || address?.origin !== targetScope.origin
        || !address.pathname.startsWith("/api/receipts/") || bodies.has(params.requestId)) return;
      try {
        const body = await send("Network.getResponseBody", { requestId: params.requestId });
        parseBody(body);
        bodies.set(params.requestId, { requestId: params.requestId, body: redact(clone(body)) });
      } catch (error) {
        bodyErrors.push({ requestId: params.requestId, error: errorText(error), capturedAt: iso() });
        throw error;
      }
    }
  }

  function pump(timeout, options = {}) {
    // 前台终态取证与后台循环共享一个读取队列，绝不并发消费同一 cursor。
    const task = eventReadTail.then(() => pumpPage(timeout, options));
    eventReadTail = task.catch(() => {});
    return task;
  }

  async function pumpPage(timeout, { baselineOnly = false } = {}) {
    let more;
    do {
      const page = await readEvents(more ? 0 : timeout);
      const sanitized = page.events.map(event => redact(clone(event)));
      for (const event of sanitized) {
        if (managedDocumentPause(event) && !documentSequences.has(event.sequence)) {
          documentSequences.add(event.sequence); browserManagedDocumentPauses.push(event);
        }
      }
      if (baselineOnly) {
        baselineEvents.push(...sanitized);
        if (page.events.some(event => event.method === "Fetch.requestPaused" && !managedDocumentPause(event)))
          preexistingPauseObserved = true;
        invariant(!page.events.some(event => event.method === "Fetch.requestPaused" && !managedDocumentPause(event)),
          "新 tab 已有非 Document 的 Fetch.requestPaused，不能接管其他驱动或未知拦截");
      } else {
        observation.events.push(...sanitized);
        for (const event of sanitized) {
          if (event.method === "Fetch.requestPaused" && !managedDocumentPause(event)) {
            if (!expectedOwnedPause(event)) {
              cleanupUncertain = true;
              throw new Error("发现不属于本驱动 Fetch/XHR patterns 的未知暂停，不能接管或控制");
            }
            registerPaused(event);
          }
          if (["Network.loadingFailed", "Network.loadingFinished"].includes(event.method)) {
            const events = terminalEvents.get(event.params.requestId) || [];
            events.push(event); terminalEvents.set(event.params.requestId, events);
          }
        }
        if (page.truncated) {
          observation.eventsTruncated = true; cleanupUncertain = true;
        }
        await appendFile(observation.journalPath, `${JSON.stringify({
          capturedAt: iso(), fromCursor: cursor, cursor: page.cursor,
          hasMore: page.hasMore, truncated: page.truncated, events: sanitized,
        })}\n`, "utf8");
        // 整页已保存且所有暂停 ID 已登记。处理器失败时恢复从下一页继续，不能重放旧事件。
        cursor = page.cursor;
        if (page.truncated) throw new Error("CDP 事件已截断，不能补造网络窗口或宣告故障恢复通过");
        for (const event of sanitized) await handleEvent(event);
      }
      cursor = page.cursor; more = page.hasMore;
    } while (more);
  }

  function startLoop() {
    stopRequested = false;
    loopTask = (async () => {
      try {
        while (!stopRequested) {
          const before = cursor;
          await pump(eventPollTimeoutMs);
          if (cursor === before && !stopRequested) await delay(pollIntervalMs);
        }
      } catch (error) {
        loopError = error;
        if (observation) observation.errors.push({ capturedAt: iso(), error: errorText(error) });
        stopRequested = true;
        if (!restoring) await restoreControls({ fromLoop: true });
      }
    })();
  }

  function restoreControls({ fromLoop = false } = {}) {
    if (recoveryTask) return recoveryTask;
    // 只复用已经确认的终态；前次失败保留在历史中，新的显式恢复可以重试。
    if (lastRecovery?.restorationConfirmed && lease
      && isDeepStrictEqual(ownedState(), lease.originalState))
      return Promise.resolve(clone(lastRecovery));
    cleanupStarted = true; restoring = true;
    const attempt = { id: randomUUID(), startedAt: iso(), fromLoop, beforeState: ownedState() };
    cleanupAttempts.push(attempt);
    const task = (async () => {
      const errors = [];
      if (fault) { fault.enabled = false; fault.releaseArmed = false; }
      if (fetchPossiblyOwned) {
        try {
          // 此 CDP 不支持 Fetch.disable。只清除此驱动在新 tab 上拥有的 patterns。
          await send("Fetch.enable", { patterns: [] });
          fetchPatterns = []; fetchPossiblyOwned = false;
        } catch (error) { errors.push(error); }
      }
      stopRequested = true;
      if (!fromLoop && loopTask) {
        try { await loopTask; } catch (error) { errors.push(error); }
      }
      if (observation && !cleanupUncertain) {
        try { await pump(0); } catch (error) { errors.push(error); }
      }
      const ownedIds = [...pendingPaused.keys()];
      const released = await Promise.allSettled(ownedIds.map(id => {
        const state = pauseStates.get(id);
        const retireOldRead = state?.originalReceiptRead && fault?.receiptReadFault === "pause"
          && !state.allowedNewDocumentRead;
        return controlPaused(id, retireOldRead ? "fail" : "continue", "restore-owned-paused-request", {
          attemptKey: attempt.id, retry: true, allowTerminalProof: true, deferError: true,
          allowActionChange: !state?.originalReceiptRead,
        });
      }));
      errors.push(...released.filter(result => result.status === "rejected").map(result => result.reason));
      if (observation && !cleanupUncertain) {
        try { await pump(0); } catch (error) { errors.push(error); }
      }
      for (let index = 0; index < ownedIds.length; index++) {
        const state = pauseStates.get(ownedIds[index]);
        if (state && !state.retired) confirmTerminal(state);
        if (state && !state.retired && released[index].status === "fulfilled")
          errors.push(new Error(released[index].value.error || "暂停请求缺少确认或匹配的 Network 终态"));
      }
      if (observation) { observation.active = false; observation.finishedAt = iso(); }
      const currentState = ownedState();
      const matches = Boolean(lease && isDeepStrictEqual(currentState, lease.originalState));
      const restorationConfirmed = matches && errors.length === 0 && !cleanupUncertain;
      if (lease) lease.restored = restorationConfirmed;
      const result = { attemptId: attempt.id, errors: errors.map(errorText),
        stateMatchesOriginal: matches, restorationConfirmed, currentState };
      Object.assign(attempt, clone(result), { finishedAt: iso() });
      lastRecovery = clone(result);
      return result;
    })().catch(error => {
      const result = { attemptId: attempt.id, errors: [errorText(error)],
        stateMatchesOriginal: Boolean(lease && isDeepStrictEqual(ownedState(), lease.originalState)),
        restorationConfirmed: false, currentState: ownedState() };
      if (lease) lease.restored = false;
      Object.assign(attempt, clone(result), { finishedAt: iso() });
      lastRecovery = clone(result);
      return result;
    });
    recoveryTask = task;
    const settled = () => { if (recoveryTask === task) recoveryTask = null; };
    task.then(settled, settled);
    return task;
  }

  async function healthy() {
    if (loopError) {
      if (recoveryTask) await recoveryTask;
      throw new Error(`CDP 后台观察失败，已尝试撤回本驱动控制，原回答保持未知：${errorText(loopError)}`);
    }
    invariant(!restoring && observation?.active, "回执网络观察尚未开始或已停止");
  }

  async function waitFor(read, condition, description) {
    const until = Date.now() + timeoutMs;
    let latest;
    do {
      await healthy();
      latest = await read();
      if (condition(latest)) return latest;
      await delay(pollIntervalMs);
    } while (Date.now() < until && !stopRequested);
    await healthy();
    throw new Error(`${description}：未捕获真实条件；不重答、不制造存储或成功回执`);
  }

  async function retireReadsBeforeReload() {
    const until = Date.now() + timeoutMs;
    do {
      await healthy();
      const oldReads = pendingOldReceiptReads().filter(state => newDocumentRead(state) === false);
      await Promise.all(oldReads.map(state => failPaused(state.params.requestId,
        "retire-old-document-receipt-read", {
          attemptKey: fault.releaseAttemptKey, allowTerminalProof: true, deferError: true,
        })));
      await pump(0);
      if (pendingOldReceiptReads().length === 0) {
        fault.releaseReady = true;
        return;
      }
      await delay(pollIntervalMs);
    } while (Date.now() < until);
    throw new Error("旧回执 GET 的撤销未确认，且缺少匹配的原始 Network 终态；不放行刷新读取");
  }

  async function acquire(request) {
    const binding = validate(request, { acquisition: true });
    invariant(!lease && !absentRecovery, "该实例已登记或已完成原租约收尾，不能重新获取");
    acquisitionAttempt = {
      binding, startedAt: iso(), originalState: ownedState(), phase: "request-validated", steps: [],
      leaseRegistrationAttempted: false, failedBeforeLease: false, ownershipRejected: false,
    };
    acquisitionAttempts.push(acquisitionAttempt);
    acquisitionStep("request-validated");
    invariant(request.payload.resources?.length === 1 && request.payload.resources[0] === "receipts",
      "本驱动只获取 receipts 一个资源");
    acquisitionStep("resources-validated");
    acquisitionAttempt.ownershipRejected = ownedCdp.has(cdp) || ownedTabs.has(tab.id);
    invariant(!acquisitionAttempt.ownershipRejected, "该 tab/CDP 已有租约，禁止覆盖既有拦截");
    acquisitionStep("ownership-checked");
    invariant(proof.tabId === tab.id && proof.runId === request.runId && nonempty(proof.createdAt)
      && Number.isFinite(Date.parse(proof.createdAt)) && Date.parse(proof.createdAt) <= Date.now() + 5_000
      && proof.noExistingFetchInterception === true
      && (proof.initialUrl === "about:blank" || url(proof.initialUrl)?.origin === request.origin),
    "必须提供本轮新 tab 的创建记录及无既有 Fetch 拦截的明确所有权");
    acquisitionStep("new-tab-proof-checked");
    const actual = await actualTab();
    assertOperational();
    acquisitionStep("tab-read", { actualTab: actual });
    await pump(0, { baselineOnly: true });
    assertOperational();
    acquisitionStep("baseline-read", { cursor });
    baseline = { cursor, capturedAt: iso(), newTabProof: redact(proof), actualTab: actual, events: baselineEvents };
    // 在本地登记所有权之前记录可能已执行的边界；后续落盘失败也不能走无租约路径。
    acquisitionAttempt.leaseRegistrationAttempted = true;
    acquisitionStep("registering-lease");
    lease = {
      ...binding, restored: false, originalState: ownedState(),
    };
    ownedCdp.set(cdp, driverToken); ownedTabs.set(tab.id, driverToken);
    acquisitionStep("lease-registered");
    return { records: [await save(request, { leaseAcquired: true, originalState: lease.originalState },
      { baseline, acquisitionAttempts }, { completedControl: true })] };
  }

  async function perform(request) {
    if (request.operation === "acquire-environment-lease") return acquire(request);
    validate(request, {
      cleanup: request.operation === "restore-environment-resource",
      allowAbsent: request.operation === "restore-environment-resource" && request.payload.acquisitionUnknown === true,
    });
    if (request.operation === "restore-environment-resource") {
      invariant(request.payload.resource === "receipts" && request.payload.restoreOnlyOwnedChanges === true,
        "恢复只能撤回原 receipts 租约拥有的改变");
      if (!lease) return restoreAbsentLease(request);
      const wasOriginal = isDeepStrictEqual(ownedState(), lease.originalState);
      // 在任何异步 tab 读取之前登记清理并取得共享任务，防止并发 restore 被串行队列
      // 拆成两次尝试，也防止读取期间仍有操作重新布置故障。
      const recovery = restoreControls();
      const actual = await actualTab();
      const restored = await recovery;
      const record = await saveCleanup(request, {
        resource: "receipts", restoreDisposition: restored.restorationConfirmed
          ? wasOriginal ? "already-original" : "restored" : "restore-unconfirmed",
        originalState: lease.originalState, currentState: restored.currentState,
        stateMatchesOriginal: restored.stateMatchesOriginal, restorationConfirmed: restored.restorationConfirmed,
        cleanupAttemptId: restored.attemptId,
      }, { actualTab: actual, cleanupErrors: restored.errors, acquisitionAttempts, mutationAttempts, commands },
      { completedControl: restored.restorationConfirmed });
      if (!restored.restorationConfirmed)
        return { status: "blocked", reason: `本驱动控制恢复未全部确认：${restored.errors.join("；") || "仍有未知拦截"}`, records: [record] };
      return { records: [record] };
    }
    const actual = await actualTab();
    assertOperational();
    if (request.operation === "start-question-recovery-observation") {
      invariant(!observation && request.payload.onlyCurrentTab === true && request.payload.readOnly === true,
        "只允许一次当前 tab 的只读完整观察");
      frame = await frameInfo();
      assertOperational();
      await mkdir(root, { recursive: true });
      assertOperational();
      observation = {
        active: true, startedAt: iso(), finishedAt: null, eventsTruncated: false,
        journalPath: resolve(root, `question-receipt-network-${randomUUID()}.ndjson`),
        events: [], controls: [], errors: [],
      };
      await appendFile(observation.journalPath, `${JSON.stringify({
        capture: { runId: request.runId, caseId: request.caseId, origin: request.origin,
          sourceFingerprint: request.sourceFingerprint, capturedAt: observation.startedAt },
        baseline, mainFrame: redact(frame), networkStartedAt: observation.startedAt,
      })}\n`, "utf8");
      assertOperational();
      await send("Network.enable", { maxPostDataSize: 1_048_576 });
      assertOperational();
      startLoop();
      return { records: [await save(request, { observationStarted: true, networkStartedAt: observation.startedAt },
        { actualTab: actual, baseline, mainFrame: frame, journalPath: observation.journalPath, commands },
        { completedControl: true })] };
    }
    await healthy();
    if (request.operation === "drop-question-answer-receipt") {
      const receiptReadFault = request.payload.receiptReadFault === undefined ? "network-error" : request.payload.receiptReadFault;
      invariant(["network-error", "pause"].includes(receiptReadFault), "receiptReadFault 仅支持 network-error 或 pause");
      invariant(!fault && request.payload.threadId === lease.threadId && nonempty(request.payload.requestId)
        && request.payload.onlyAction === "answer" && request.payload.preserveServerExecution === true
        && request.payload.holdOnlyOriginalReceiptReadsUntilNewDocument === true,
      "丢回包必须精确指定原 thread/question request，保留服务执行并仅阻断原回执");
      fault = { enabled: true, threadId: lease.threadId, questionRequestId: request.payload.requestId,
        receiptReadFault, armedAt: iso(), dropAttempted: false, releaseArmed: false,
        releaseReady: false, releaseDocument: null };
      // Browser Use 保留 Document 导航；明确只拦截网页 fetch/XHR 的业务回包。
      fetchPatterns = ["Fetch", "XHR"].flatMap(resourceType => [
        { urlPattern: `${request.origin}/api/action/answer*`, resourceType, requestStage: "Response" },
        { urlPattern: `${request.origin}/api/receipts/*`, resourceType, requestStage: "Request" },
      ]);
      fetchPossiblyOwned = true;
      await send("Fetch.enable", { patterns: clone(fetchPatterns) });
      assertOperational();
      return { records: [await save(request, {
        receiptFaultArmed: true, questionRequestId: fault.questionRequestId, onlyAction: "answer", receiptReadFault,
      }, { actualTab: actual, fetchPatterns, commands }, { completedControl: true })] };
    }
    if (request.operation === "release-question-receipt-on-reload") {
      invariant(fault?.unknownObserved && fault.dropAcknowledgedAt && !fault.releaseArmed && !fault.releaseDocument
        && request.payload.originalReceiptId === fault.originalReceiptId
        && request.payload.questionRequestId === fault.questionRequestId && request.payload.onlyCurrentTab === true,
      "释放必须绑定已经实际捕获 unknown 的同一原回执与原题");
      fault.releaseFrame = await frameInfo();
      assertOperational();
      fault.releaseAfterSequence = cursor; fault.releaseArmedAt = iso(); fault.releaseArmed = true;
      fault.releaseAttemptKey = randomUUID();
      if (fault.receiptReadFault === "pause") await retireReadsBeforeReload();
      else fault.releaseReady = true;
      assertOperational();
      return { records: [await save(request, {
        releaseOnNewDocument: true, originalReceiptId: fault.originalReceiptId,
        questionRequestId: fault.questionRequestId, receiptReadFault: fault.receiptReadFault,
      }, { actualTab: actual, releaseFrame: fault.releaseFrame,
        releaseAfterSequence: fault.releaseAfterSequence, releaseArmedAt: fault.releaseArmedAt,
        pausedRequests: pausedEvidence(), commands },
      { completedControl: true })] };
    }
    throw new Error(`perform 不支持 ${request.operation}`);
  }

  function recoveredRead() {
    if (!fault?.releaseDocument) return null;
    const document = fault.releaseDocument.params;
    for (const [requestId, request] of requests) {
      const address = url(request.request?.url), response = responses.get(requestId);
      const finish = completed.get(requestId), capturedBody = bodies.get(requestId);
      if (request.request?.method !== "GET" || address?.origin !== targetScope.origin
        || address.pathname !== `/api/receipts/${encodeURIComponent(fault.originalReceiptId)}`
        || request.loaderId !== document.loaderId || request.wallTime < document.wallTime
        || response?.response?.status !== 200 || !finish || failed.has(requestId) || !capturedBody) continue;
      if (!successfulReceipt(parseBody(capturedBody.body), fault)) continue;
      return { requestId, request, response, finish, body: capturedBody.body };
    }
    return null;
  }

  async function observe(request) {
    const binding = validate(request, {
      cleanup: request.operation === "inspect-environment-lease",
      allowAbsent: request.operation === "inspect-environment-lease" && absentRecovery !== null,
    });
    if (request.operation === "inspect-environment-lease") {
      if (recoveryTask) await recoveryTask;
      if (!lease) {
        invariant(absentRecovery && noMutationProof(binding),
          "无租约收尾后的状态不再满足获取前无副作用证据，不能宣告原状态一致");
        const currentState = ownedState();
        return { records: [await save(request, {
          leaseAcquired: false, restoreDisposition: "lease-absent-no-mutation",
          originalState: absentRecovery.originalState, currentState,
          stateMatchesOriginal: isDeepStrictEqual(currentState, absentRecovery.originalState),
        }, { acquisitionAttempts, mutationAttempts, commands, absentRecoveryRecord: absentRecovery.record },
        { controlBinding: binding })] };
      }
      const actual = await actualTab(), currentState = ownedState();
      const stateMatchesOriginal = isDeepStrictEqual(currentState, lease.originalState);
      const restorationConfirmed = stateMatchesOriginal && (!cleanupStarted || lastRecovery?.restorationConfirmed === true);
      const records = [await save(request, {
        originalState: lease.originalState, currentState,
        stateMatchesOriginal, restorationConfirmed, driverClosed: closed,
        cleanupAttemptId: lastRecovery?.attemptId,
      }, { actualTab: actual, commands, cleanupUncertain, cleanupAttempts,
        cleanupEvidence, pausedRequests: pausedEvidence(), cleanupErrors: lastRecovery?.errors || [] })];
      if (!restorationConfirmed)
        return { status: "blocked", reason: "原租约的资源恢复尚未确认，只允许继续核对或恢复", records };
      return { records };
    }
    invariant(request.operation === "observe-question-recovery", "observe 仅支持问答回执观察和租约检查");
    const phase = request.payload.phase;
    invariant(["unknown", "delivery"].includes(phase), "环境回执驱动不提供原生历史阶段");
    await healthy();
    if (phase === "unknown") {
      invariant(fault && request.payload.questionRequestId === fault.questionRequestId, "unknown 必须绑定准确原题");
      const snapshot = await waitFor(() => readPageMetadata(phase), ({ metadata }) =>
        fault.dropAcknowledgedAt && failed.has(fault.networkRequestId)
        && successfulReceipt(fault.answerReceipt, fault) && metadata.clientReceipts.length === 1
        && metadata.clientReceipts[0].id === fault.originalReceiptId
        && metadata.clientReceipts[0].threadId === fault.threadId
        && metadata.clientReceipts[0].requestId === fault.questionRequestId
        && metadata.clientReceipts[0].deviceId === metadata.device.id
        && metadata.clientReceipts[0].state === "unknown",
      "未捕获丢回包后的真实客户端 unknown 和服务端成功正文");
      fault.unknownObserved = true; fault.unknownDevice = clone(snapshot.metadata.device);
      return { records: [await save(request, {
        phase, originalReceiptId: fault.originalReceiptId, questionRequestId: fault.questionRequestId,
        receiptReadFault: fault.receiptReadFault,
        answerReceipt: fault.answerReceipt, clientReceipts: snapshot.metadata.clientReceipts,
        device: snapshot.metadata.device,
      }, { ...snapshot, answerPost: fault.answerPost, answerPausedEvent: fault.answerPausedEvent,
        answerResponseBody: fault.answerResponseBody, answerLoadingFailed: failed.get(fault.networkRequestId),
        controls: observation.controls, pausedRequests: pausedEvidence(), commands, journalPath: observation.journalPath })] };
    }
    invariant(fault?.unknownObserved && request.payload.originalReceiptId === fault.originalReceiptId,
      "delivery 必须绑定此前实际 unknown 的同一原回执");
    await waitFor(() => readPageMetadata(phase), ({ metadata }) =>
      recoveredRead() && metadata.clientReceipts.length === 0
      && pendingOldReceiptReads().length === 0
      && isDeepStrictEqual(metadata.device, fault.unknownDevice),
    "新文档未实际完成同 ID 回执 GET，或客户端原锁/设备身份不符合恢复条件");
    await pump(0);
    const snapshot = await readPageMetadata(phase);
    invariant(recoveredRead() && snapshot.metadata.clientReceipts.length === 0
      && pendingOldReceiptReads().length === 0
      && isDeepStrictEqual(snapshot.metadata.device, fault.unknownDevice), "终态复读时原回执或设备身份已变化");
    // 结束时间取于最后一次 drain 开始前；该次完整 drain 覆盖这个已知截止时间，
    // 不把尚在桥接队列里的事件排除后再填写一个更晚的 wall-clock 时间。
    const networkFinishedAt = iso();
    await pump(0);
    await healthy();
    return { records: [await save(request, {
      phase, originalReceiptId: fault.originalReceiptId,
      receiptReadFault: fault.receiptReadFault,
      networkStartedAt: observation.startedAt, networkFinishedAt,
      eventsTruncated: observation.eventsTruncated,
      networkEvents: observation.events.filter(event => event.method.startsWith("Network.")),
      responseBodies: [...bodies.values()], clientReceipts: snapshot.metadata.clientReceipts,
      device: snapshot.metadata.device,
    }, { ...snapshot, fetchEvents: observation.events.filter(event => event.method === "Fetch.requestPaused"),
      releaseDocument: fault.releaseDocument, controls: observation.controls, pausedRequests: pausedEvidence(), commands, bodyErrors,
      journalPath: observation.journalPath })] };
  }

  async function blocked(request, error) {
    const reason = errorText(error);
    const records = [];
    let binding = null;
    try { binding = requestBinding(request); } catch { /* 未通过来源/完整 scope 校验的请求没有恢复权限。 */ }
    if (request?.operation === "acquire-environment-lease" && sameBinding(binding, acquisitionAttempt?.binding)) {
      acquisitionAttempt.failedBeforeLease = !lease && !acquisitionAttempt.leaseRegistrationAttempted;
      acquisitionStep(acquisitionAttempt.failedBeforeLease ? "failed-before-lease" : "failed-after-registration",
        { error: reason });
    }
    if (lease && sameBinding(binding, lease)) {
      // 操作失败首次触发安全恢复；清理已开始后，错误或只读检查不隐式重试失败命令。
      // 新的恢复尝试只由显式 restore 或 close 发起。
      const recovered = cleanupStarted
        ? recoveryTask ? await recoveryTask : lastRecovery || {
          errors: [], currentState: ownedState(), stateMatchesOriginal: false, restorationConfirmed: false,
        } : await restoreControls();
      try {
        records.push(await save(request, { driverError: reason, originalState: lease.originalState,
          currentState: recovered.currentState, stateMatchesOriginal: recovered.stateMatchesOriginal,
          restorationConfirmed: recovered.restorationConfirmed },
        { cleanupErrors: recovered.errors, acquisitionAttempts, mutationAttempts, commands, observation, fault, bodyErrors,
          pausedRequests: pausedEvidence(), cleanupAttempts, cleanupEvidence }));
      } catch { /* 落盘失败不能跳过实际恢复，也不能生成成功记录。 */ }
    } else if (binding) {
      try {
        records.push(await save(request, {
          driverError: reason, driverLeasePresent: lease !== null,
          acquisitionFailedBeforeLease: sameBinding(binding, acquisitionAttempt?.binding)
            && acquisitionAttempt.failedBeforeLease,
          noMutationProven: noMutationProof(binding),
        }, { acquisitionAttempts, mutationAttempts, commands, preexistingPauseObserved, baselineEvents,
          pausedRequests: pausedEvidence() },
        { controlBinding: binding }));
      } catch { /* 失败记录不可写时仍保持 blocked，不生成成功收尾。 */ }
    }
    return { status: "blocked", reason, ...(records.length ? { records } : {}) };
  }

  function dispatch(kind, request) {
    let normalized, normalizationError;
    try { normalized = clone(request); }
    catch (error) { normalizationError = new Error(`驱动请求不是可解析的 JSON：${errorText(error)}`); }
    const execute = async () => {
      try {
        if (normalizationError) throw normalizationError;
        return await (kind === "perform" ? perform(normalized) : observe(normalized));
      } catch (error) { return blocked(normalized, error); }
    };
    const cleanup = kind === "perform" && normalized?.operation === "restore-environment-resource"
      || kind === "observe" && normalized?.operation === "inspect-environment-lease";
    // 清理不能排在长时间的业务观察之后；所有恢复路径仍由 recoveryTask 去重，
    // 检查只等待该任务，绝不隐式开启下一次恢复。
    const task = cleanup ? execute() : serial.then(execute);
    if (!cleanup) serial = task.catch(() => {});
    return task;
  }

  function close() {
    if (closeTask) return closeTask;
    if (closed) return Promise.resolve(clone(closeResult));
    cleanupStarted = true;
    const attempt = { id: randomUUID(), startedAt: iso() };
    closeAttempts.push(attempt);
    const task = (async () => {
      const records = [];
      try {
        if (!lease) {
          invariant(!observation && !fault && !fetchPossiblyOwned && pendingPaused.size === 0
            && mutationAttempts.length === 0 && !cleanupUncertain,
          "无原租约但存在未确认控制，不能宣告关闭");
          if (absentRecovery) {
            invariant(noMutationProof(absentRecovery.binding), "无租约收尾的原始无副作用证据已失效");
            records.push(await saveCleanup({
              ...lastRequest, operation: "restore-environment-resource", actions: [], actionId: undefined,
            }, { resource: "receipts", leaseAcquired: false, restoreDisposition: "lease-absent-no-mutation",
              originalState: absentRecovery.originalState, currentState: ownedState(),
              stateMatchesOriginal: true, restorationConfirmed: true,
            }, { acquisitionAttempts, mutationAttempts, commands },
            { completedControl: true, controlBinding: absentRecovery.binding }));
          }
        } else {
          const restored = await restoreControls();
          invariant(lastRequest, "原租约缺少可保存的关闭上下文");
          records.push(await saveCleanup({
            ...lastRequest, operation: "restore-environment-resource", actions: [], actionId: undefined,
          }, {
            resource: "receipts", restoreDisposition: restored.restorationConfirmed ? "restored" : "restore-unconfirmed",
            originalState: lease.originalState, currentState: restored.currentState,
            stateMatchesOriginal: restored.stateMatchesOriginal, restorationConfirmed: restored.restorationConfirmed,
            cleanupAttemptId: restored.attemptId,
          }, { cleanupErrors: restored.errors, acquisitionAttempts, mutationAttempts, commands },
          { completedControl: restored.restorationConfirmed }));
          if (!restored.restorationConfirmed) {
            const reason = `关闭回执驱动时恢复未全部确认：${restored.errors.join("；") || "仍有未知拦截"}`;
            Object.assign(attempt, { finishedAt: iso(), error: reason, records: clone(records) });
            return { status: "blocked", reason, records };
          }
        }
        // 清理确认和证据落盘均完成后才结束生命周期；失败仍保留原租约所有权。
        Object.assign(attempt, { finishedAt: iso(), records: clone(records) });
        if (ownedCdp.get(cdp) === driverToken) ownedCdp.delete(cdp);
        if (ownedTabs.get(tab.id) === driverToken) ownedTabs.delete(tab.id);
        closed = true;
        closeResult = clone({ records });
        return clone(closeResult);
      } catch (error) {
        const reason = `关闭回执驱动未确认：${errorText(error)}`;
        Object.assign(attempt, { finishedAt: iso(), error: reason, records: clone(records) });
        return { status: "blocked", reason, records: records.length ? records : clone(cleanupEvidence) };
      }
    })();
    closeTask = task;
    const settled = () => { if (closeTask === task) closeTask = null; };
    task.then(settled, settled);
    return task;
  }

  async function readAnswerReceiptEvidence(request, action) {
    request = clone(request); action = clone(action);
    invariant(lease && request.runId === lease.runId && request.caseId === lease.caseId
      && request.origin === targetScope.origin && request.sourceFingerprint === lease.sourceFingerprint,
    "最终回答回执读取不属于原运行");
    const read = recoveredRead();
    invariant(read && action?.kind === "answer" && nonempty(action.id)
      && action.threadId === fault.threadId && action.questionRequestId === fault.questionRequestId
      && action.result?.receiptId === fault.originalReceiptId, "最终回答动作没有绑定真实原 POST 与恢复 GET");
    const receipt = parseBody(read.body);
    // operation 来自实际原 POST URL；成功状态、身份、ID 均指向原始 GET 正文。
    const original = {
      capture: { runId: request.runId, caseId: request.caseId, origin: request.origin,
        sourceFingerprint: request.sourceFingerprint, capturedAt: iso() },
      request: { action: url(fault.answerPost.request.url).pathname.split("/").at(-1),
        post: fault.answerPost, get: read.request },
      receipt, networkReadId: read.requestId, responseBody: read.body,
    };
    await mkdir(root, { recursive: true });
    const bytes = Buffer.from(`${JSON.stringify(redact(original), null, 2)}\n`);
    const path = resolve(root, `question-receipt-recovered-answer-${randomUUID()}.json`);
    await writeFile(path, bytes, { flag: "wx" });
    return { records: [{
      source: "bridge", actionId: action.id, threadId: receipt.result.threadId,
      receiptId: receipt.requestId, readId: read.requestId, evidencePath: path, size: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"), outcomePointer: "",
      rawFacts: {
        operation: "/request/action", threadId: "/receipt/result/threadId",
        receiptId: "/receipt/requestId", readId: "/networkReadId", receiptState: "/receipt/state",
        receiptSuccess: "/receipt/result/ok", questionRequestId: "/receipt/result/requestId",
        runId: "/capture/runId", caseId: "/capture/caseId", origin: "/capture/origin",
        sourceFingerprint: "/capture/sourceFingerprint", capturedAt: "/capture/capturedAt",
      },
    }] };
  }

  return Object.freeze({
    contractVersion: QUESTION_RECEIPT_DRIVER_VERSION, operations,
    perform: request => dispatch("perform", request), observe: request => dispatch("observe", request),
    close, readAnswerReceiptEvidence,
  });
}
