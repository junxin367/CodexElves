/**
 * 长期离线回归：覆盖来源枚举、文件完整性、JSON 原始事实、身份绑定及报告门禁。
 * 所有样本均在独立 OS 临时目录创建并自行清理，不接触真实会话或服务。
 */
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname, resolve, sep } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { runInNewContext } from "node:vm";
import {
  createEvidenceReference, verifyEvidenceReference, jsonPointer, screenshotFileType,
  listRemoteAccessSourceInputs, sourceFingerprint, verifyNativeEvidence,
  verifyRuntimeEvidence, verifyRemoteAccessReport, verifyNaturalQueueHistory,
  verifyQuestionReceiptRecoveryFacts, verifyQuestionReceiptRecoveryStage,
  verifyQueueEditCancelFacts, verifyQueueEditCancelStage,
  classifyQueueRecoveryTransition, verifyQueueRecoveryTerminal,
  verifyQueueDequeueRaceConsumption, verifyQueueDequeueRaceFacts, verifyQueueDequeueRaceStage,
} from "./remote-access-e2e-evidence.mjs";
import { findQuestionAnswerTurn, findQueueSteeringTurn, createRemoteAccessE2EContext, runRemoteAccessCase, runRemoteAccessE2E,
  REMOTE_ACCESS_E2E_CASES, REMOTE_ACCESS_E2E_CONTRACTS,
  REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS } from "./remote-access-e2e.mjs";

const executeFile = promisify(execFile);
const origin = "https://remote.example.test";
const runId = "evidence_run_123";
const scriptVersion = "test-runner-2";
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lZkAAAAASUVORK5CYII=", "base64");
const jpegSignatureSample = Uint8Array.from([255, 216, 255, 224, 0, 16, 74, 70, 73, 70, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 255, 217]);

function fifoSample() {
  const entries = ["A", "B"].map(label => ({
    queueId: `physical-${label}`, clientUserMessageId: `logical-${label}`,
    prompt: `队列 ${label}\n\n保留空行`, expectedReply: `REPLY-${label}`,
  }));
  const turns = [
    { turnId: "base", status: "completed", turnStartedAtMs: 100, durationMs: 100, items: [] },
    ...entries.map((entry, index) => ({
      turnId: `next-${index}`, status: "completed", turnStartedAtMs: 210 + index * 100, durationMs: 90,
      params: { input: [] }, // 真实自动承接可无 params.input；不能据此判未接收。
      items: [
        { type: "userMessage", id: `user-${index}`, clientId: entry.clientUserMessageId,
          content: [{ type: "text", text: `${entry.prompt}\n` }] },
        { type: "agentMessage", text: entry.expectedReply },
      ],
    })),
  ];
  return { history: { threadId: "owned", turns, queuedMessages: [], isComplete: true },
    expected: { threadId: "owned", previousTurnId: "base", entries } };
}

test("两条自然队列按持久消息身份验收，允许物理与逻辑 ID 不同及空 turn.params", () => {
  const { history, expected } = fifoSample();
  const result = verifyNaturalQueueHistory(history, expected);
  assert.deepEqual(result.consumed.map(value => value.turnId), ["next-0", "next-1"]);
  assert.deepEqual(result.consumed.map(value => value.clientUserMessageId), ["logical-A", "logical-B"]);
});

test("同文案的旧消息、重复消费、错会话及只测一条不能冒充两条 FIFO 通过", () => {
  for (const mutate of [
    ({ history }) => { history.threadId = "other"; },
    ({ history }) => { history.isComplete = false; },
    ({ history }) => { history.turns[1].items[0].clientId = "old-message"; },
    ({ history }) => { history.turns[1].items.push(structuredClone(history.turns[1].items[0])); },
    ({ expected }) => { expected.entries.pop(); },
    ({ expected }) => { expected.entries[1].clientUserMessageId = expected.entries[0].clientUserMessageId; },
  ]) {
    const sample = fifoSample();
    mutate(sample);
    assert.throws(() => verifyNaturalQueueHistory(sample.history, sample.expected), /FIFO/);
  }
});

test("FIFO 拒绝抢先执行、顺序颠倒、失败轮、正文丢空行及残留队列", () => {
  for (const mutate of [
    ({ history }) => { history.turns[1].turnStartedAtMs = 150; },
    ({ history }) => { [history.turns[1], history.turns[2]] = [history.turns[2], history.turns[1]]; },
    ({ history }) => { history.turns[2].status = "failed"; },
    ({ history }) => { history.turns[1].items[0].content[0].text = "队列 A\n保留空行\n"; },
    ({ history }) => { history.turns[1].items[1].text = "旧 REPLY-A"; },
    ({ history }) => { history.queuedMessages.push({ id: "physical-A" }); },
    ({ history }) => { delete history.turns[1].durationMs; },
    ({ history }) => { history.turns[0].items.push({ type: "steeringUserMessage", status: "accepted",
      clientUserMessageId: "logical-A" }); },
    ({ history }) => { history.turns[0].items.push({ type: "steeringUserMessage", status: "accepted",
      restoreMessage: { id: "physical-A", clientUserMessageId: "logical-A" } }); },
    ({ history }) => { history.turns[0].items.push({ type: "steeringUserMessage", status: "accepted",
      serverUserMessageId: "user-0" }); },
  ]) {
    const sample = fifoSample();
    mutate(sample);
    assert.throws(() => verifyNaturalQueueHistory(sample.history, sample.expected), /FIFO/);
  }
});

function queueRecoverySample(kind = "stop", outcome = "retained", baseTime = Date.now() - 30_000) {
  const threadId = "thread-recovery";
  const activeTurnId = "active-turn";
  const queueId = "physical-recovery";
  const clientUserMessageId = "client-recovery";
  const queueActionId = "run:queue:send";
  const prompt = "停止或失败后的原队列正文";
  const expectedReply = "QUEUE-RECOVERED";
  const input = [{ type: "text", text: `${prompt}\n`, text_elements: [] }];
  const original = { id: queueId, clientUserMessageId, input: structuredClone(input) };
  const acceptedItem = { type: "steeringUserMessage", status: "accepted", clientUserMessageId,
    input: structuredClone(input) };
  const activeStatus = kind === "stop" ? "interrupted" : "failed";
  const oldTurn = status => ({ turnId: activeTurnId, status, items: [] });
  const acceptedTurn = status => ({ turnId: "accepted-turn", status,
    items: [structuredClone(acceptedItem), ...(status === "completed"
      ? [{ type: "agentMessage", text: expectedReply }] : [])] });
  const snapshot = (phase, offset, nativeTurns, queuedMessages) => ({
    threadId, phase, readId: `${phase}-read`, capturedAt: new Date(baseTime + offset).toISOString(),
    queueRecovery: { historyComplete: true, nativeTurns, queuedMessages },
  });
  const before = snapshot("before", 1_000, [oldTurn("inProgress")], [structuredClone(original)]);
  const transition = snapshot("transition", 2_000,
    [oldTurn(activeStatus), ...(outcome === "automatically-accepted" ? [acceptedTurn("inProgress")] : [])],
    outcome === "retained" ? [structuredClone(original)] : []);
  const terminal = snapshot("terminal", 3_000, [oldTurn(activeStatus), acceptedTurn("completed")], []);
  const receipt = { actionId: queueActionId, facts: {
    actionId: queueActionId, threadId, operation: "queue-send", receiptId: "queue-receipt",
    receiptState: "completed", success: true, deliveryState: "queued", queueId,
    clientUserMessageId, input: structuredClone(input),
  } };
  const expected = { kind, threadId, activeTurnId, queueId, clientUserMessageId,
    queueActionId, prompt, expectedReply };
  return { before, transition, terminal, receipt, expected };
}

test("停止与失败队列恢复接受原生自动承接或保留原Q两分支，并以同client完整input唯一收口", () => {
  for (const kind of ["stop", "failure"]) {
    for (const outcome of ["automatically-accepted", "retained"]) {
      const sample = queueRecoverySample(kind, outcome);
      assert.equal(classifyQueueRecoveryTransition(sample, sample.expected).outcome, outcome);
      assert.equal(verifyQueueRecoveryTerminal(sample, { ...sample.expected, outcome }).outcome, outcome);
    }
  }
  const conflict = queueRecoverySample("stop", "retained");
  conflict.transition.queueRecovery.nativeTurns.push({
    turnId: "unexpected-turn", status: "inProgress",
    items: [{ type: "steeringUserMessage", status: "accepted", clientUserMessageId: conflict.expected.clientUserMessageId,
      input: structuredClone(conflict.before.queueRecovery.queuedMessages[0].input) }],
  });
  assert.equal(classifyQueueRecoveryTransition(conflict, conflict.expected).outcome, "unknown");
  assert.throws(() => verifyQueueRecoveryTerminal(conflict, { ...conflict.expected, outcome: "retained" }), /未知/);
});

test("停止与失败队列恢复拒绝client错配、完整input变化、重复接受和错误最终回复", () => {
  const wrongClient = queueRecoverySample("failure", "automatically-accepted");
  wrongClient.receipt.facts.clientUserMessageId = "other-client";
  assert.throws(() => classifyQueueRecoveryTransition(wrongClient, wrongClient.expected), /client ID/);

  const changedInput = queueRecoverySample("stop", "automatically-accepted");
  changedInput.transition.queueRecovery.nativeTurns[1].items[0].input[0].text += "被改写";
  assert.throws(() => classifyQueueRecoveryTransition(changedInput, changedInput.expected), /完整 input/);

  const duplicate = queueRecoverySample("failure", "automatically-accepted");
  duplicate.terminal.queueRecovery.nativeTurns[1].items.push(
    structuredClone(duplicate.terminal.queueRecovery.nativeTurns[1].items[0]));
  assert.throws(() => verifyQueueRecoveryTerminal(duplicate,
    { ...duplicate.expected, outcome: "automatically-accepted" }), /且仅有一次接受/);

  const wrongReply = queueRecoverySample("stop", "retained");
  wrongReply.terminal.queueRecovery.nativeTurns[1].items[1].text = "其他回复";
  assert.throws(() => verifyQueueRecoveryTerminal(wrongReply,
    { ...wrongReply.expected, outcome: "retained" }), /唯一准确回复/);
});

// 队列取消的长期离线反例只验证验收资产，不代表真实取消已执行。
function queueCancelSample() {
  const original = { id: "physical-original", clientUserMessageId: "logical-original", input: [
    { type: "text", text: "原文\n\n保留空行\n", text_elements: [{ start: 0, end: 2, type: "mention", path: "original.txt" }] },
    { type: "image", image_url: "data:image/png;base64,original", detail: "high" },
    { type: "skill", name: "example", path: "/original/SKILL.md" },
  ] };
  const neighbor = { id: "physical-next", clientUserMessageId: "logical-next",
    input: [{ type: "text", text: "后面的消息\n" }] };
  const before = { threadId: "thread-main", queuedMessages: [original, neighbor] };
  const editing = { threadId: before.threadId, editId: "edit-1", editState: "editing",
    originalMessageId: original.id, originalClientUserMessageId: original.clientUserMessageId,
    originalInput: structuredClone(original.input), originalIndex: 0 };
  const restored = { ...structuredClone(original), id: "physical-restored" };
  const after = { threadId: before.threadId, editId: editing.editId, editState: "canceled",
    queuedMessages: [restored, structuredClone(neighbor)], nativeTurns: [], historyComplete: true,
    cancelReceipt: { requestId: "cancel-receipt", state: "completed", result: {
      ok: true, threadId: before.threadId, editId: editing.editId,
      editState: "canceled", messageId: restored.id,
    } } };
  return { data: { before, editing, after },
    expected: { threadId: before.threadId, queueId: original.id, editId: editing.editId } };
}

test("队列取消按原ED恢复完整input和位置，允许有血缘的新物理ID", () => {
  const { data, expected } = queueCancelSample();
  assert.deepEqual(verifyQueueEditCancelFacts(data, expected), {
    queueId: "physical-restored", clientUserMessageId: "logical-original", editId: "edit-1", originalIndex: 0,
  });
});

test("队列取消拒绝串ED、正文或隐藏输入丢失、错误位置、重复对象及已消费影子", () => {
  for (const [name, mutate] of [
    ["串ED", value => { value.editing.editId = "other-edit"; }],
    ["取出前原input被替换", value => { value.editing.originalInput[0].text = "修改稿"; }],
    ["skill丢失", value => { value.after.queuedMessages[0].input.pop(); }],
    ["图片细节变化", value => { value.after.queuedMessages[0].input[1].detail = "auto"; }],
    ["正文空行丢失", value => { value.after.queuedMessages[0].input[0].text = "原文\n保留空行\n"; }],
    ["隐藏引用变化", value => { value.after.queuedMessages[0].input[0].text_elements[0].path = "other.txt"; }],
    ["位置改变", value => { value.after.queuedMessages.reverse(); }],
    ["重复逻辑对象", value => { value.after.queuedMessages.push(structuredClone(value.after.queuedMessages[0])); }],
    ["取消回执unknown", value => { value.after.cancelReceipt.state = "unknown"; }],
    ["取消回执其他ED", value => { value.after.cancelReceipt.result.editId = "other-edit"; }],
    ["回执物理ID不对应", value => { value.after.cancelReceipt.result.messageId = "unrelated"; }],
    ["不完整历史", value => { value.after.historyComplete = false; }],
    ["已消费仍残留", value => { value.after.nativeTurns = [{ items: [
      { type: "userMessage", clientId: "logical-original" },
    ] }]; }],
    ["引导不能冒充取消", value => { value.after.nativeTurns = [{ items: [
      { type: "steeringUserMessage", restoreMessage: { clientUserMessageId: "logical-original" } },
    ] }]; }],
  ]) {
    const { data, expected } = queueCancelSample();
    mutate(data);
    assert.throws(() => verifyQueueEditCancelFacts(data, expected), /队列取消/, name);
  }
});

async function queueCancelStageFixture(t) {
  const fixture = await reportFixture(t);
  const { root, report, options } = fixture, caseId = "queue.edit_cancel";
  const { data, expected } = queueCancelSample();
  const time = index => new Date(Date.parse(report.runStartedAt) + 1_000 + index * 200).toISOString();
  const queueAction = { id: `${runId}:${caseId}:queue-A`, caseId, kind: "queue-send",
    threadId: expected.threadId, status: "confirmed", prompt: "原文\n\n保留空行",
    result: { queueId: expected.queueId }, startedAt: time(0), finishedAt: time(1) };
  const cancelAction = { id: `${runId}:${caseId}:edit-cancel-A`, caseId, kind: "queue-edit-cancel",
    threadId: expected.threadId, queueId: expected.queueId, editId: expected.editId,
    status: "confirmed", startedAt: time(5), finishedAt: time(11),
    result: { queueId: "physical-restored", receiptId: "cancel-receipt", editId: expected.editId } };
  const stage = { kind: "queue-edit-cancel", threadId: expected.threadId, controlThreadId: "thread-control",
    queueId: expected.queueId, editId: expected.editId,
    queueActionId: queueAction.id, cancelActionId: cancelAction.id, cancelStartedAt: time(8) };
  const driverSteps = [];
  for (const [phase, index] of [["before", 4], ["editing", 6], ["after", 10]]) {
    const raw = { ...data[phase], phase, operation: "observe-queue-edit-cancel",
      runId, caseId, origin, sourceFingerprint: report.sourceFingerprint, readId: `${phase}-read`, capturedAt: time(index) };
    const reference = await createEvidenceReference(await put(root, `cancel-${phase}.json`, JSON.stringify(raw)), { baseDir: root });
    stage[phase] = { source: "rpc", ...reference, threadId: expected.threadId, readId: raw.readId,
      rawFacts: Object.fromEntries(Object.keys(raw).map(key => [key, `/${key}`])) };
    driverSteps.push({ kind: "driver", driver: "native", operation: raw.operation, records: [stage[phase]],
      request: { contractVersion: "remote-e2e-driver/2", operation: raw.operation, runId, caseId,
        origin, sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt,
        actions: [], threadIds: [expected.threadId], payload: { phase }, expectedFacts: { phase } } });
  }
  const mainDraft = `[remote-e2e:${runId}:main-ordinary-draft]`,
    controlDraft = `[remote-e2e:${runId}:control-ordinary-draft]`;
  for (const [name, index] of [["controlBeforeUi", 2], ["beforeUi", 3], ["beforeCancelUi", 7],
    ["afterUi", 9], ["controlAfterUi", 12], ["returnedUi", 13]]) {
    const editing = name === "beforeCancelUi";
    const observation = { at: time(index), threadId: name.startsWith("control") ? "thread-control" : expected.threadId,
      prompt: editing ? `${queueAction.prompt}\n取消编辑的临时文字` : name.startsWith("control") ? controlDraft : mainDraft,
      promptEditable: true, loading: false, editing, pendingText: "",
      send: { action: editing ? "save" : "send", enabled: true } };
    stage[name] = { kind: "ui", threadId: observation.threadId, observation,
      files: {
        // DOM故意没有contenteditable的值；通过依据是实际值JSON，不是摘要里碰巧有文字。
        dom: await createEvidenceReference(await put(root, `${name}.dom.txt`, '- textbox "消息输入"\n'), { baseDir: root }),
        screenshot: await createEvidenceReference(await put(root, `${name}.png`, png), { baseDir: root }),
        observation: await createEvidenceReference(await put(root, `${name}.json`, JSON.stringify(observation)), { baseDir: root }),
      } };
  }
  stage.beforeCancelEditor = await createEvidenceReference(await put(root, "cancel-editor.json", JSON.stringify({
    threadId: expected.threadId, editId: expected.editId, capturedAt: stage.beforeCancelUi.observation.at,
    value: stage.beforeCancelUi.observation.prompt,
  })), { baseDir: root });
  const actions = [queueAction, cancelAction];
  const receipts = [];
  for (const [index, action] of actions.entries())
    receipts.push(await proof(root, { caseId, sourceFingerprint: report.sourceFingerprint,
      threadId: expected.threadId, turnId: undefined, operation: action.kind, actionId: action.id,
      receiptId: index ? "cancel-receipt" : "queue-receipt", receiptState: "completed", queueId: expected.queueId },
    { actionId: action.id, turnId: undefined }, `cancel-action-${index}.json`));
  report.actions = Object.fromEntries(actions.map(value => [value.id, value]));
  report.threads = { main: { id: expected.threadId, runId }, control: { id: "thread-control", runId } };
  const nativeRequest = { runId, caseId, origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt, actions, threadIds: [expected.threadId] };
  report.cases = [{ ...report.cases[0], id: caseId, nativeExpectations: { threadIds: [expected.threadId] },
    evidence: [...Object.entries(stage).filter(([, value]) => value?.kind === "ui").map(([, value]) => value),
      ...driverSteps, { kind: "native", records: receipts, request: nativeRequest }, stage] }];
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: { ...REMOTE_ACCESS_E2E_CONTRACTS[caseId],
    requiredDriverSteps: REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS[caseId] } };
  return { ...fixture, stage, request: { ...nativeRequest, ...expected, controlThreadId: "thread-control",
    queueAction, cancelAction }, queueAction, cancelAction };
}

test("队列取消以实际编辑器值JSON绑定thread和ED，DOM不含值仍可核完整输入与双草稿", async t => {
  const { stage, request, root, report, options } = await queueCancelStageFixture(t);
  assert.equal((await verifyQueueEditCancelStage(stage, request, { baseDir: root })).queueId, "physical-restored");
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, true, result.problems.join("\n"));
});

test("队列取消拒绝未实际改稿、缺值文件、串ED/时间、丢草稿及加载未完成的发送状态", async t => {
  const fixture = await queueCancelStageFixture(t);
  const { root, request } = fixture;
  for (const [name, mutate] of [
    ["没有实际改稿", async stage => {
      stage.beforeCancelUi.observation.prompt = request.queueAction.prompt;
      await replaceUiObservation(stage.beforeCancelUi);
      const editor = JSON.parse(await readFile(join(root, stage.beforeCancelEditor.path), "utf8"));
      editor.value = request.queueAction.prompt;
      await writeFile(join(root, stage.beforeCancelEditor.path), JSON.stringify(editor));
      stage.beforeCancelEditor = await createEvidenceReference(join(root, stage.beforeCancelEditor.path), { baseDir: root });
    }],
    ["只保存DOM摘要", async stage => { delete stage.beforeCancelEditor; }],
    ["丢失实际值JSON", async stage => { delete stage.beforeCancelUi.files.observation; }],
    ["串ED", async stage => { stage.editId = "different-edit"; }],
    ["取消前取值过晚", async stage => { stage.cancelStartedAt = request.cancelAction.startedAt; }],
    ["控制草稿丢失", async stage => { stage.controlAfterUi.observation.prompt = ""; await replaceUiObservation(stage.controlAfterUi); }],
    ["主草稿变成改稿", async stage => { stage.afterUi.observation.prompt = stage.beforeCancelUi.observation.prompt; await replaceUiObservation(stage.afterUi); }],
    ["仍在加载", async stage => { stage.returnedUi.observation.loading = true; await replaceUiObservation(stage.returnedUi); }],
    ["发送禁用", async stage => { stage.returnedUi.observation.send.enabled = false; await replaceUiObservation(stage.returnedUi); }],
    ["发送状态缺失", async stage => { delete stage.returnedUi.observation.send; await replaceUiObservation(stage.returnedUi); }],
  ]) {
    const stage = structuredClone(fixture.stage);
    // 每次变体使用自己的原始UI/编辑器文件，避免串改上一个变体。
    for (const key of ["controlBeforeUi", "beforeUi", "beforeCancelUi", "afterUi", "controlAfterUi", "returnedUi"]) {
      const file = `${name}-${key}.json`;
      stage[key].files.observation = await createEvidenceReference(
        await put(root, file, JSON.stringify(stage[key].observation)), { baseDir: root });
    }
    stage.beforeCancelEditor = await createEvidenceReference(await put(root, `${name}-editor.json`, JSON.stringify({
      threadId: request.threadId, editId: request.editId, capturedAt: stage.beforeCancelUi.observation.at,
      value: stage.beforeCancelUi.observation.prompt,
    })), { baseDir: root });
    await mutate(stage);
    await assert.rejects(verifyQueueEditCancelStage(stage, request, { baseDir: root }), /队列取消|path/, name);
  }
  async function replaceUiObservation(ui) {
    const path = join(root, ui.files.observation.path);
    await writeFile(path, JSON.stringify(ui.observation));
    ui.files.observation = await createEvidenceReference(path, { baseDir: root });
  }
});

test("队列取消报告不能删除专项或弱化契约，也不能用错误原input和错位恢复骗过门禁", async t => {
  const { root, report, options } = await queueCancelStageFixture(t);
  const stripped = structuredClone(report);
  stripped.cases[0].evidence = stripped.cases[0].evidence.filter(value => value.kind !== "queue-edit-cancel");
  stripped.cases[0].nativeExpectations.requiresQueueEditCancel = false;
  const weakOptions = { ...options, caseContracts: { "queue.edit_cancel": { requiresQueueEditCancel: false } } };
  let result = await verifyRemoteAccessReport(stripped, weakOptions);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /固定用例缺少唯一专项/);
  for (const [name, mutate] of [
    ["原input改变", raw => { raw.queuedMessages[0].input[2].path = "/wrong/SKILL.md"; }],
    ["恢复位置改变", raw => { raw.queuedMessages.reverse(); }],
  ]) {
    const changed = structuredClone(report), stage = changed.cases[0].evidence.find(value => value.kind === "queue-edit-cancel");
    const raw = JSON.parse(await readFile(join(root, stage.after.path), "utf8"));
    mutate(raw);
    const reference = await createEvidenceReference(await put(root, `${name}.json`, JSON.stringify(raw)), { baseDir: root });
    const old = stage.after;
    stage.after = { ...old, ...reference };
    for (const step of changed.cases[0].evidence.filter(value => value.kind === "driver" && value.request.payload.phase === "after"))
      step.records = [stage.after];
    result = await verifyRemoteAccessReport(changed, options);
    assert.equal(result.passed, false);
    assert.match(result.problems.join("\n"), /队列取消/);
  }
});

async function queueRecoveryReportFixture(t, trigger, outcome) {
  const fixture = await reportFixture(t);
  const { root, report, options, ui } = fixture;
  const caseId = trigger === "stop" ? "queue.after_stop" : "queue.after_failure";
  const baseTime = Date.parse(report.runStartedAt) + 5_000;
  const sample = queueRecoverySample(trigger, outcome, baseTime);
  const threadId = sample.expected.threadId;
  const recoveryUi = { ...structuredClone(ui), threadId,
    observation: { ...ui.observation, threadId } };
  const at = offset => new Date(baseTime + offset).toISOString();
  const queueActionId = `${runId}:${caseId}:queue-send`;
  const stopActionId = `${runId}:${caseId}:stop-with-queue`;
  const failureActionId = `${runId}:${caseId}:driver-environment-fail-active-upstream-once`;
  const recoveryActionId = `${runId}:${caseId}:resume-queue-after-${trigger}`;
  const leaseKey = `${runId}:${caseId}:environment`;
  const instanceId = `${runId}-isolated-service`;
  const originalState = { upstream: "healthy" };
  const actions = [];

  const queueAction = {
    id: queueActionId, caseId, kind: "queue-send", threadId, status: "confirmed",
    prompt: sample.expected.prompt, startedAt: at(200), finishedAt: at(500),
    result: { queueId: sample.expected.queueId },
  };
  actions.push(queueAction);
  if (trigger === "stop") {
    actions.push({
      id: stopActionId, caseId, kind: "stop", threadId, turnId: sample.expected.activeTurnId,
      status: "confirmed", startedAt: at(1_200), finishedAt: at(1_800),
      result: { turnId: sample.expected.activeTurnId },
    });
  }
  let recoveryAction = null;
  if (outcome === "retained") {
    recoveryAction = {
      id: recoveryActionId, caseId, kind: "queue-steer", threadId,
      queueId: sample.expected.queueId, turnId: "accepted-turn", status: "confirmed",
      startedAt: at(2_200), finishedAt: at(2_800), result: { turnId: "accepted-turn" },
    };
    actions.push(recoveryAction);
  }
  let failureAction = null;
  if (trigger === "failure") {
    failureAction = {
      id: failureActionId, caseId, kind: "fail-active-upstream", threadId,
      status: "confirmed", startedAt: at(1_200), finishedAt: at(1_600),
      result: {},
    };
    actions.push(failureAction);
  }

  const leaseAcquireId = `${runId}:${caseId}:lease-acquire-environment-lease-all`;
  const leaseRestoreId = `${runId}:${caseId}:lease-restore-environment-resource-upstream`;
  let leaseAcquireAction = null, leaseRestoreAction = null;
  if (trigger === "failure") {
    leaseAcquireAction = {
      id: leaseAcquireId, caseId, kind: "acquire-environment-lease", threadId,
      status: "confirmed", startedAt: at(100), finishedAt: at(150), result: {},
    };
    leaseRestoreAction = {
      id: leaseRestoreId, caseId, kind: "restore-environment-resource", threadId,
      status: "confirmed", startedAt: at(1_700), finishedAt: at(1_850), result: {},
    };
    actions.push(leaseAcquireAction, leaseRestoreAction);
  }

  const records = [];
  async function addRecord(name, raw, recordChanges = {}) {
    const record = await proof(root, {
      caseId, sourceFingerprint: report.sourceFingerprint, ...raw,
    }, {
      actionId: raw.actionId, threadId: raw.threadId || threadId,
      turnId: raw.turnId, receiptId: raw.receiptId, readId: raw.readId,
      ...recordChanges,
    }, `queue-recovery-${trigger}-${outcome}-${name}.json`);
    records.push(record);
    return record;
  }
  async function driverStep(driver, operation, record, {
    phase, capturedAfter, expectedFacts = {}, expectations = [], stepActions = [],
  } = {}) {
    return {
      kind: "driver", driver, operation, records: [record],
      request: {
        contractVersion: "remote-e2e-driver/2", runId, caseId, operation, origin,
        sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt,
        capturedAfter, actions: stepActions, threadId, threadIds: [threadId],
        ...(phase ? { payload: { phase } } : {}),
        expectedFacts, expectations,
      },
    };
  }

  const phaseSteps = [];
  for (const phase of ["before", "transition", "terminal"]) {
    const source = sample[phase];
    const record = await addRecord(phase, {
      ...source, operation: "observe-queue-boundary", activeTurnId: sample.expected.activeTurnId,
      actionId: undefined, receiptId: undefined, turnId: undefined,
    });
    phaseSteps.push(await driverStep("native", "observe-queue-boundary", record, {
      phase, capturedAfter: at(999 + phaseSteps.length * 1_000),
      expectedFacts: { operation: "observe-queue-boundary", phase },
    }));
  }

  const queueReceipt = await addRecord("queue-receipt", {
    actionId: queueActionId, operation: "queue-send", receiptId: "queue-receipt",
    threadId, turnId: undefined, capturedAt: at(700), receiptState: "completed", success: true,
    deliveryState: "queued", queueId: sample.expected.queueId,
    clientUserMessageId: sample.expected.clientUserMessageId,
    input: structuredClone(sample.before.queueRecovery.queuedMessages[0].input),
  });
  const nativeRecords = [queueReceipt];
  if (trigger === "stop") {
    nativeRecords.push(await addRecord("stop-receipt", {
      actionId: stopActionId, operation: "stop", receiptId: "stop-receipt", threadId,
      turnId: sample.expected.activeTurnId, capturedAt: at(1_900),
      receiptState: "completed", success: true, turnStatus: "interrupted",
    }));
  }
  if (recoveryAction) {
    nativeRecords.push(await addRecord("recovery-receipt", {
      actionId: recoveryAction.id, operation: "queue-steer", receiptId: "recovery-receipt",
      threadId, turnId: "accepted-turn", capturedAt: at(2_750),
      receiptState: "completed", success: true, queueId: sample.expected.queueId,
    }));
  }

  const driverSteps = [...phaseSteps];
  if (failureAction) {
    const failureRecord = await addRecord("upstream-failure", {
      actionId: failureAction.id, operation: "fail-active-upstream", receiptId: "failure-receipt",
      threadId, turnId: undefined, capturedAt: at(1_500), receiptState: "completed", success: true,
      affectedTurnId: sample.expected.activeTurnId, realUpstreamFailure: true,
    });
    driverSteps.push(await driverStep("environment", "fail-active-upstream", failureRecord, {
      capturedAfter: at(1_400), stepActions: [failureAction],
      expectedFacts: { affectedTurnId: sample.expected.activeTurnId, realUpstreamFailure: true },
    }));
  }
  if (trigger === "failure") {
    const leaseFacts = {
      leaseKey, leaseOwnerRunId: runId, instanceId, originalState,
      unownedStateChanged: false, scopeKind: "dedicated-test-service",
    };
    const acquisition = await addRecord("lease-acquire", {
      actionId: leaseAcquireAction.id, operation: "acquire-environment-lease",
      receiptId: "lease-acquire-receipt", threadId, turnId: undefined, capturedAt: at(140),
      receiptState: "completed", success: true, ...leaseFacts,
    });
    const restore = await addRecord("lease-restore", {
      actionId: leaseRestoreAction.id, operation: "restore-environment-resource",
      receiptId: "lease-restore-receipt", threadId, turnId: undefined, capturedAt: at(1_850),
      receiptState: "completed", success: true, resource: "upstream", restoreDisposition: "restored",
      ...leaseFacts,
    });
    const inspect = await addRecord("lease-inspect", {
      actionId: undefined, operation: "inspect-environment-lease",
      receiptId: "lease-inspect-receipt", threadId, turnId: undefined, capturedAt: at(2_900),
      ...leaseFacts, currentState: originalState, stateMatchesOriginal: true,
    });
    const expectations = Object.entries({
      leaseKey, leaseOwnerRunId: runId, instanceId, originalState,
      unownedStateChanged: false, scopeKind: "dedicated-test-service",
    }).map(([fact, value]) => ({ fact, op: "equals", value }));
    driverSteps.push(
      await driverStep("environment", "acquire-environment-lease", acquisition, {
        capturedAfter: at(130), stepActions: [leaseAcquireAction], expectations,
      }),
      await driverStep("environment", "restore-environment-resource", restore, {
        capturedAfter: at(1_840), stepActions: [leaseRestoreAction],
        expectedFacts: { resource: "upstream" }, expectations,
      }),
      await driverStep("environment", "inspect-environment-lease", inspect, {
        capturedAfter: at(2_890), expectedFacts: { stateMatchesOriginal: true }, expectations,
      }),
    );
  }

  const stage = {
    kind: "queue-recovery", caseId, trigger, threadId,
    activeTurnId: sample.expected.activeTurnId, queueId: sample.expected.queueId,
    clientUserMessageId: sample.expected.clientUserMessageId, queueActionId,
    triggerActionId: trigger === "stop" ? stopActionId : null,
    recoveryActionId: recoveryAction?.id || null,
    prompt: sample.expected.prompt, expectedReply: sample.expected.expectedReply, outcome,
  };
  report.actions = Object.fromEntries(actions.map(action => [action.id, action]));
  report.threads = { main: { id: threadId, runId } };
  const nativeActions = [queueAction, ...(trigger === "stop" ? [actions.find(action => action.kind === "stop")] : []),
    ...(recoveryAction ? [recoveryAction] : [])];
  const nativeRequest = { runId, caseId, origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt, actions: nativeActions, threadIds: [threadId] };
  report.cases = [{
    ...report.cases[0], id: caseId, nativeExpectations: { threadIds: [threadId] },
    evidence: [recoveryUi, ...driverSteps, { kind: "native", records: nativeRecords, request: nativeRequest }, stage],
  }];
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = {
    [caseId]: {
      ...REMOTE_ACCESS_E2E_CONTRACTS[caseId],
      requiredDriverSteps: REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS[caseId],
    },
  };
  return { ...fixture, stage };
}

test("停止与失败队列报告门禁通过两种原生策略，且拒绝缺少专项或伪造分支", async t => {
  for (const trigger of ["stop", "failure"]) {
    for (const outcome of ["automatically-accepted", "retained"]) {
      const fixture = await queueRecoveryReportFixture(t, trigger, outcome);
      const result = await verifyRemoteAccessReport(fixture.report, fixture.options);
      assert.equal(result.passed, true, result.problems.join("\n"));

      const missingStage = structuredClone(fixture.report);
      missingStage.cases[0].evidence = missingStage.cases[0].evidence
        .filter(value => value.kind !== "queue-recovery");
      const weakened = {
        ...fixture.options,
        caseContracts: { [missingStage.cases[0].id]: { requiresQueueRecovery: false } },
      };
      const blocked = await verifyRemoteAccessReport(missingStage, weakened);
      assert.equal(blocked.passed, false);
      assert.match(blocked.problems.join("\n"), /缺少唯一专项记录/);

      const changedOutcome = structuredClone(fixture.report);
      const stage = changedOutcome.cases[0].evidence.find(value => value.kind === "queue-recovery");
      stage.outcome = outcome === "retained" ? "automatically-accepted" : "retained";
      const mismatch = await verifyRemoteAccessReport(changedOutcome, fixture.options);
      assert.equal(mismatch.passed, false);
      assert.match(mismatch.problems.join("\n"), /恢复分支|queue-recovery|专项记录/);
    }
  }
});

test("原生文件门禁要求 FIFO 原回执身份及历史，不能仅提供 queueEmpty=true", async t => {
  const root = await sandbox(t);
  const { history, expected } = fifoSample();
  const request = {
    caseId: "queue.automatic",
    threadIds: ["owned"],
    actions: [
      { id: "base-action", status: "confirmed", kind: "send", threadId: "owned", turnId: "base" },
      ...expected.entries.map((entry, index) => ({ id: `action-${index}`, status: "confirmed",
        kind: "queue-send", threadId: "owned", queueId: entry.queueId })),
    ],
    naturalQueue: { threadId: "owned", previousTurnId: "base",
      entries: expected.entries.map((entry, index) => ({ ...entry, actionId: `action-${index}` })) },
  };
  const records = [];
  const baseRaw = { threadId: "owned", receiptId: "base-receipt", operation: "send",
    receiptState: "completed", turnId: "base" };
  records.push({ source: "rpc",
    ...await createEvidenceReference(await put(root, "base.json", JSON.stringify(baseRaw))),
    threadId: "owned", receiptId: baseRaw.receiptId, actionId: "base-action", outcomePointer: "",
    rawFacts: Object.fromEntries(Object.keys(baseRaw).map(key => [key, `/${key}`])) });
  for (const [index, entry] of expected.entries.entries()) {
    const raw = { threadId: "owned", receiptId: `receipt-${index}`, operation: "queue-send",
      receiptState: "completed", deliveryState: "queued", queueId: entry.queueId,
      clientUserMessageId: entry.clientUserMessageId };
    const artifact = await createEvidenceReference(await put(root, `queue-${index}.json`, JSON.stringify(raw)));
    records.push({ source: "rpc", ...artifact, threadId: "owned", receiptId: raw.receiptId,
      actionId: `action-${index}`, outcomePointer: "",
      rawFacts: Object.fromEntries(Object.keys(raw).map(key => [key, `/${key}`])) });
  }
  const raw = { ...history, readId: "native-final-read", nativeTurns: history.turns, queueEmpty: true };
  const artifact = await createEvidenceReference(await put(root, "history.json", JSON.stringify(raw)));
  const snapshot = { source: "rpc", ...artifact, threadId: "owned", readId: raw.readId,
    rawFacts: { threadId: "/threadId", readId: "/readId", nativeTurns: "/nativeTurns",
      queuedMessages: "/queuedMessages", historyComplete: "/isComplete" } };
  await verifyNativeEvidence([...records, snapshot], request);
  await assert.rejects(verifyNativeEvidence([...records, snapshot], { ...request, naturalQueue: undefined }),
    /FIFO 固定用例/);
  await assert.rejects(verifyNativeEvidence([...records, snapshot], {
    ...request, actions: [...request.actions, { id: "steer", kind: "queue-steer" }],
  }), /FIFO 固定用例/);
  await assert.rejects(verifyNativeEvidence(records, request), /FIFO.*原始快照/);
  const missingClient = structuredClone(records);
  delete missingClient[1].rawFacts.clientUserMessageId;
  await assert.rejects(verifyNativeEvidence([...missingClient, snapshot], request), /FIFO.*原排队回执/);
  const booleanOnly = { ...snapshot, rawFacts: { threadId: "/threadId", readId: "/readId", queueEmpty: "/queueEmpty" } };
  await assert.rejects(verifyNativeEvidence([...records, booleanOnly], request), /FIFO.*原始快照/);
});

test("引导活动轮取自可见处理状态，普通草稿改变发送按钮不能误判空闲", () => {
  const active = { id: "active-before-click", processing: true };
  assert.equal(findQueueSteeringTurn({ turns: [{ id: "old", processing: false }, active],
    send: { action: "queue" }, prompt: "未发送普通草稿" }), active);
  assert.equal(findQueueSteeringTurn({ turns: [{ id: "old", processing: false }] }), null);
  assert.equal(findQueueSteeringTurn({ turns: [{ id: "pending-local", processing: true }] }), null);
  assert.equal(findQueueSteeringTurn({ turns: [active, { id: "other", processing: true }] }), null);
});

async function steeringBeforeProof(root) {
  return createEvidenceReference(await put(root, "steer-before.json", JSON.stringify({
    at: new Date(Date.now() - 2_000).toISOString(), threadId: "thread-main", loading: false,
    turns: [{ id: "active-before-click", processing: true }],
    queue: [{ id: "queued-D", steerEnabled: true }],
  })));
}

test("队列引导必须由原始回执证明进入点击前的活动轮，不能进入新轮也算通过", async t => {
  const root = await sandbox(t);
  const actionId = `${runId}:queue.steer:steer`;
  const request = baseRequest({
    caseId: "queue.steer", turnIds: ["active-before-click"],
    expectedFacts: { turnId: "active-before-click" },
    queueSteerBefore: await steeringBeforeProof(root),
    actions: [{ id: actionId, kind: "queue-steer", status: "confirmed", threadId: "thread-main",
      startedAt: new Date(Date.now() - 1_000).toISOString(),
      queueId: "queued-D", turnId: "active-before-click", result: { turnId: "active-before-click" } }],
  });
  const raw = { caseId: "queue.steer", actionId, operation: "steerQueued", queueId: "queued-D",
    receiptState: "completed", turnStatus: undefined, turnId: "active-before-click" };
  const original = await proof(root, raw, { actionId }, "steer-original.json");
  await verifyNativeEvidence([original], request, { baseDir: root });
  const newTurn = await proof(root, { ...raw, turnId: "new-turn-after-click" },
    { actionId }, "steer-new-turn.json");
  await assert.rejects(verifyNativeEvidence([newTurn], request, { baseDir: root }), /其他轮次|turnId/);
});

test("队列引导报告不能删除操作前轮次或用操作后轮次补造基线", async t => {
  const root = await sandbox(t);
  const actionId = `${runId}:queue.steer:steer`;
  const request = baseRequest({
    caseId: "queue.steer", turnIds: ["active-before-click"],
    expectedFacts: { turnId: "active-before-click" },
    queueSteerBefore: await steeringBeforeProof(root),
    actions: [{ id: actionId, kind: "queue-steer", status: "confirmed", threadId: "thread-main",
      startedAt: new Date(Date.now() - 1_000).toISOString(),
      queueId: "queued-D", turnId: "active-before-click", result: { turnId: "active-before-click" } }],
  });
  const record = await proof(root, { caseId: "queue.steer", actionId, operation: "steerQueued",
    queueId: "queued-D", receiptState: "completed", turnStatus: undefined, turnId: "active-before-click" },
  { actionId }, "steer.json");
  for (const mutate of [
    value => { delete value.actions[0].turnId; },
    value => { value.actions[0].result.turnId = "new-turn-after-click"; },
    value => { value.turnIds = []; },
    value => { delete value.expectedFacts.turnId; },
    value => { delete value.queueSteerBefore; },
    value => { value.actions[0].startedAt = new Date(Date.now() - 5_000).toISOString(); },
  ]) {
    const changed = structuredClone(request);
    mutate(changed);
    await assert.rejects(verifyNativeEvidence([record], changed, { baseDir: root }), /引导固定用例/);
  }
  const rewritten = structuredClone(request);
  rewritten.actions[0].turnId = rewritten.actions[0].result.turnId = "new-turn-after-click";
  rewritten.turnIds = ["new-turn-after-click"];
  rewritten.expectedFacts.turnId = "new-turn-after-click";
  const rewrittenRecord = await proof(root, { caseId: "queue.steer", actionId, operation: "steerQueued",
    queueId: "queued-D", receiptState: "completed", turnStatus: undefined, turnId: "new-turn-after-click" },
  { actionId }, "steer-rewritten-result.json");
  await assert.rejects(verifyNativeEvidence([rewrittenRecord], rewritten, { baseDir: root }),
    /引导固定用例的原始观察/);
});

test("异步问答原轮已结束时，最终回复可以位于不含原提问标记的新回答轮", () => {
  const spec = { threadId: "main", marker: "question-marker", expected: "ANSWER-OK" };
  const origin = { id: "question-turn", error: "", users: [{ text: "question-marker" }], agents: [] };
  const answer = { id: "answer-turn", error: "", users: [{ text: "颜色？ 蓝色" }], agents: [{ text: "ANSWER-OK" }] };
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [origin, answer], send: { action: "send" } }, spec,
    { originatingTurnId: origin.id, beforeTurnIds: [origin.id], completedOrigin: true }), answer);
});

test("问答最终回复不能借用旧轮次、其他会话或重复候选", () => {
  const spec = { threadId: "main", expected: "ANSWER-OK" };
  const old = { id: "old", agents: [{ text: "ANSWER-OK" }] };
  const answer = { id: "new", agents: [{ text: "ANSWER-OK" }] };
  const scope = { originatingTurnId: "old", beforeTurnIds: ["old"], completedOrigin: true };
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [old] }, spec, scope), null);
  assert.equal(findQuestionAnswerTurn({ threadId: "other", turns: [answer] }, spec, scope), null);
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [answer, { ...answer, id: "duplicate" }] }, spec, scope), null);
});

test("活动轮问答允许原轮引导回复，但未结束、失败或临时占位不能算完成", () => {
  const spec = { threadId: "main", expected: "ANSWER-OK" };
  const turn = { id: "active", agents: [{ text: "ANSWER-OK" }] };
  const scope = { originatingTurnId: turn.id, beforeTurnIds: [turn.id], completedOrigin: false };
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [turn], send: { action: "send" } }, spec, scope), turn);
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [turn], send: { action: "stop" } }, spec, scope), null);
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [{ ...turn, error: "failed" }] }, spec, scope), null);
  assert.equal(findQuestionAnswerTurn({ threadId: "main", turns: [{ ...turn, id: "pending-answer" }] }, spec, scope), null);
});

// 合成样本只测试验收器拒绝错误证据的能力，不计作真实问答或浏览器通过。
function questionRecoverySample() {
  const baseTime = Date.now() - 60_000;
  const time = index => new Date(baseTime + index * 1000).toISOString();
  const threadId = "main", controlThreadId = "control", originalTurn = "question-turn", itemId = "question-call";
  const requestId = JSON.stringify(["async-question", originalTurn, itemId]);
  const titles = ["颜色？", "形状？"], values = ["绿色", "方形"];
  const questions = titles.map((title, index) => {
    const id = JSON.stringify(["request_user_input_async", itemId, index]);
    return { key: { threadId, hostId: "local", entityKey: "native-entity", itemId: id },
      sourceTurnId: originalTurn, sourceTurnStatus: "completed",
      state: { question: { id, sourceItemId: itemId, questionIndex: index, title, options: [values[index]] },
        turnId: originalTurn, lastSubmission: null, isSkipped: false, isSubmitting: false } };
  });
  const answers = questions.map((value, index) => ({
    questionItemId: value.state.question.id, question: titles[index], answer: values[index],
  }));
  const ask = { id: "ask-action", kind: "send", status: "confirmed", threadId,
    marker: "[question-marker]", expected: "QUESTION-ANSWER-OK", prompt: "请提问\n\n[question-marker]",
    startedAt: time(1) };
  const answer = { id: "answer-action", kind: "answer", status: "confirmed", threadId,
    questionRequestId: requestId, originatingTurnId: originalTurn, answers, startedAt: time(5),
    result: { threadId, receiptId: "original-receipt", answerTurnId: "answer-turn" } };
  const receipt = { requestId: "original-receipt", state: "completed",
    result: { ok: true, threadId, requestId } };
  const device = { id: "device-A", createdAtMs: 123 };
  const old = [{ id: "old-turn", status: "completed", items: [{ type: "agentMessage", text: "已有内容" }] }];
  const before = { threadId, capturedAt: time(0), nativeTurns: old, queuedMessages: [],
    historyRequest: { method: "thread/read", params: { threadId, includeTurns: true } } };
  const pending = { threadId, capturedAt: time(3), requestId, originatingTurnId: originalTurn,
    nativeQuestionStates: questions };
  const history = { threadId, capturedAt: time(10), historyRequest: structuredClone(before.historyRequest),
    queuedMessages: [], nativeQuestionStates: questions.map((value, index) => ({
      ...structuredClone(value), state: { ...structuredClone(value.state), lastSubmission: values[index] },
    })), nativeTurns: [...structuredClone(old),
      { id: originalTurn, status: "completed", items: [
        { type: "userMessage", content: [{ type: "text", text: `${ask.prompt}\n` }] },
      ] },
      { id: "answer-turn", status: "completed", items: [
        { type: "userMessage", content: [{ type: "text",
          text: `<send_user_message_question_reply>\n${JSON.stringify(answers)}\n</send_user_message_question_reply>` }] },
        { type: "agentMessage", text: ask.expected },
      ] },
    ] };
  const unknown = { threadId, capturedAt: time(5.7), device, answerReceipt: receipt,
    clientReceipts: [{ id: "original-receipt", threadId, requestId, deviceId: device.id, state: "unknown" }] };
  const networkRequest = (id, index, type, method, path, extra = {}) => ({
    method: "Network.requestWillBeSent", params: { requestId: id, loaderId: index < 6 ? "old-document" : "new-document",
      wallTime: Date.parse(time(index)) / 1000, timestamp: 100 + index,
      type, request: { method, url: `${origin}${path}`, ...extra } },
  });
  const delivery = { threadId, capturedAt: time(11), device: structuredClone(device), clientReceipts: [],
    eventsTruncated: false, networkStartedAt: time(0), networkFinishedAt: time(11),
    networkEvents: [
      networkRequest("answer-post", 5.1, "Fetch", "POST", "/api/action/answer", {
        headers: { "Idempotency-Key": "original-receipt" },
        postData: JSON.stringify({ threadId, requestId, method: "remote/requestUserInputAsync",
          response: { answers: Object.fromEntries(answers.map(entry => [entry.questionItemId, { answers: [entry.answer] }])) } }),
      }),
      { method: "Network.loadingFailed", params: { requestId: "answer-post", errorText: "net::ERR_FAILED" } },
      networkRequest("new-page", 6.1, "Document", "GET", "/"),
      networkRequest("original-get", 6.5, "Fetch", "GET", "/api/receipts/original-receipt"),
      { method: "Network.responseReceived", params: { requestId: "original-get", timestamp: 106.6,
        response: { status: 200, url: `${origin}/api/receipts/original-receipt` } } },
      networkRequest("ask-post", 1.1, "Fetch", "POST", "/api/action/send", {
        postData: JSON.stringify({ threadId, prompt: ask.prompt }),
      }),
      networkRequest("pending-page", 2.5, "Document", "GET", "/"),
      { method: "Network.loadingFinished", params: { requestId: "original-get", timestamp: 106.7 } },
    ], responseBodies: [{ requestId: "original-get", body: { body: JSON.stringify(receipt), base64Encoded: false } }] };
  return { data: { before, pendingInitial: { ...structuredClone(pending), capturedAt: time(2) },
    pending, unknown, answered: { ...structuredClone(history), capturedAt: time(5.75) }, history, delivery },
    expected: { origin, threadId, controlThreadId, ask, answer,
    questionReloadStartedAt: time(2.4), reloadStartedAt: time(6),
    reloadedUiAt: time(7), recoveredUiAt: time(8), finishedUiAt: time(9),
    mainDraft: "本轮主草稿", controlDraft: "本轮控制草稿" }, time };
}

test("问答回执恢复从完整历史、真实网络形状和逐题原状态核对单次回答", () => {
  const { data, expected } = questionRecoverySample();
  assert.deepEqual(verifyQuestionReceiptRecoveryFacts(data, expected), {
    originalReceiptId: "original-receipt", originatingTurnId: "question-turn", answerTurnId: "answer-turn",
    questionIds: data.pending.nativeQuestionStates.map(value => value.state.question.id),
    answerSubmissions: 1, recoveredReads: 1,
  });
});

test("问答恢复允许底层完成时刻早于响应事件派发，但两者均不能早于原GET或晚于解锁", () => {
  const sample = questionRecoverySample();
  const events = sample.data.delivery.networkEvents;
  const response = events.find(event => event.method === "Network.responseReceived");
  const finished = events.find(event => event.method === "Network.loadingFinished");
  // 真实浏览器曾报告 loadingFinished 比 responseReceived 早 1.917ms。
  // 两个字段的取值阶段不同，不据该差值引入固定容差。
  finished.params.timestamp = response.params.timestamp - 0.001917;
  assert.equal(verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected).originalReceiptId, "original-receipt");
  const started = events.find(event => event.params.requestId === "original-get"
    && event.method === "Network.requestWillBeSent").params.timestamp;
  finished.params.timestamp = started - 0.1;
  assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /新文档没有实际 GET/);
});

test("问答恢复不能用较早的底层完成时刻掩盖解锁之后才派发的响应事件", () => {
  const sample = questionRecoverySample();
  const response = sample.data.delivery.networkEvents.find(event => event.method === "Network.responseReceived");
  response.params.timestamp = 109; // UI 解锁为同一时间轴上的108，完成仍为106.7。
  assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /新文档没有实际 GET/);
});

test("问答恢复拒绝错会话、授权代次、原题、实体和答案串值", () => {
  for (const mutate of [
    ({ data }) => { data.unknown.threadId = "other"; },
    ({ data }) => { data.delivery.device.id = "device-B"; },
    ({ data }) => { data.delivery.device.createdAtMs++; },
    ({ data }) => { data.pendingInitial.requestId = "old-request"; },
    ({ data }) => { data.history.nativeQuestionStates[0].key.entityKey = "replacement"; },
    ({ data }) => { data.history.nativeQuestionStates[0].state.lastSubmission = "方形"; },
    ({ data }) => { data.history.nativeQuestionStates.pop(); },
    ({ data }) => { data.unknown.clientReceipts[0].id = "other-receipt"; },
    ({ expected }) => { expected.answer.questionRequestId = "other-question"; },
    ({ expected }) => { expected.answer.answers[0].questionItemId = "different-question"; },
  ]) {
    const sample = questionRecoverySample(); mutate(sample);
    assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /问答回执恢复/);
  }
});

test("问答恢复拒绝刷新前 UI 冒充新文档结果", () => {
  const { data, expected, time } = questionRecoverySample();
  const document = data.delivery.networkEvents[2].params, query = data.delivery.networkEvents[3].params;
  document.wallTime = Date.parse(time(7.2)) / 1000; document.timestamp = 107.2;
  query.wallTime = Date.parse(time(7.4)) / 1000; query.timestamp = 107.4;
  data.delivery.networkEvents[4].params.timestamp = 107.5;
  data.delivery.networkEvents.at(-1).params.timestamp = 107.6;
  // GET 确实早于删除解锁截图，但名为“刷新后”的截图仍在真实新文档之前。
  assert.throws(() => verifyQuestionReceiptRecoveryFacts(data, expected), /刷新后 UI 早于新文档/);
});

test("问答恢复拒绝终态逐题状态与原轮身份或成功终态矛盾", () => {
  for (const mutate of [
    value => { value.state.turnId = "wrong-turn"; },
    value => { value.sourceTurnStatus = "failed"; },
  ]) {
    const { data, expected } = questionRecoverySample();
    mutate(data.history.nativeQuestionStates[0]);
    assert.throws(() => verifyQuestionReceiptRecoveryFacts(data, expected), /已接受答案串题/);
  }
});

test("问答恢复不能借用后来才完成的回答来解释先前的删除禁用", () => {
  const { data, expected } = questionRecoverySample();
  data.answered.nativeTurns.at(-1).status = "inProgress";
  assert.throws(() => verifyQuestionReceiptRecoveryFacts(data, expected), /删除锁检查前/);
});

test("问答恢复拒绝部分历史、旧历史改写、重复回答、失败轮和已发送草稿", () => {
  for (const mutate of [
    ({ data }) => { data.before.historyRequest.params.includeTurns = false; },
    ({ data }) => { data.history.nativeTurns[0].items[0].text = "被回滚"; },
    ({ data }) => { data.history.nativeTurns.push(structuredClone(data.history.nativeTurns.at(-1))); },
    ({ data }) => { data.history.nativeTurns.at(-1).items.push(structuredClone(data.history.nativeTurns.at(-1).items[0])); },
    ({ data }) => { data.history.nativeTurns.at(-1).status = "failed"; },
    ({ data }) => { data.history.queuedMessages.push({ id: "leftover" }); },
    ({ data, expected }) => { data.history.nativeTurns.at(-1).items.push({
      type: "userMessage", content: [{ type: "text", text: expected.controlDraft }] }); },
  ]) {
    const sample = questionRecoverySample(); mutate(sample);
    assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /问答回执恢复/);
  }
});

test("问答恢复拒绝未真正丢包、截断记录、重复提交、其他业务写入和刷新前 GET", () => {
  for (const mutate of [
    ({ data }) => { data.delivery.networkEvents.splice(1, 1); },
    ({ data }) => { data.delivery.eventsTruncated = true; },
    ({ data, time }) => { data.delivery.networkStartedAt = time(5.2); },
    ({ data, time }) => { data.delivery.networkFinishedAt = time(8); },
    ({ data }) => { const repeated = structuredClone(data.delivery.networkEvents[0]);
      repeated.params.requestId = "answer-again"; data.delivery.networkEvents.push(repeated); },
    ({ data }) => { const extra = structuredClone(data.delivery.networkEvents[0]);
      extra.params.requestId = "send-instead"; extra.params.request.url = `${origin}/api/action/send`;
      data.delivery.networkEvents.push(extra); },
    ({ data }) => { data.delivery.networkEvents[3].params.loaderId = "old-document"; },
    ({ data, time }) => { data.delivery.networkEvents[3].params.wallTime = Date.parse(time(5.8)) / 1000; },
    ({ data }) => { data.delivery.networkEvents[3].params.request.url = `${origin}/api/receipts/other-receipt`; },
    ({ data }) => { data.delivery.networkEvents[0].params.request.headers["Idempotency-Key"] = "wrong"; },
    ({ data }) => { data.delivery.networkEvents.at(-1).params.timestamp = 110; },
  ]) {
    const sample = questionRecoverySample(); mutate(sample);
    assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /问答回执恢复/);
  }
});

test("问答恢复不把 404、pending、unknown、伪 completed 或仍残留的原锁判通过", () => {
  for (const mutate of [
    ({ data }) => { data.delivery.networkEvents[4].params.response.status = 404; },
    ({ data }) => { data.delivery.responseBodies[0].body.body = JSON.stringify({ ...data.unknown.answerReceipt, state: "pending" }); },
    ({ data }) => { data.delivery.responseBodies[0].body.body = JSON.stringify({ ...data.unknown.answerReceipt, state: "unknown" }); },
    ({ data }) => { data.unknown.answerReceipt.result.ok = false; },
    ({ data }) => { data.unknown.answerReceipt.result.outcomeUnknown = true; },
    ({ data }) => { data.unknown.answerReceipt.result.requestId = "old-native-request"; },
    ({ data }) => { data.delivery.clientReceipts.push(structuredClone(data.unknown.clientReceipts[0])); },
  ]) {
    const sample = questionRecoverySample(); mutate(sample);
    assert.throws(() => verifyQuestionReceiptRecoveryFacts(sample.data, sample.expected), /问答回执恢复/);
  }
});

async function questionRecoveryStageFixture(t) {
  const root = await sandbox(t), sample = questionRecoverySample();
  const { data, expected, time } = sample;
  const request = { ...expected, runId, caseId: "recovery.question_receipt", origin,
    sourceFingerprint: "a".repeat(64), runStartedAt: time(-1), capturedBefore: time(15) };
  const stage = { kind: "question-receipt-recovery", threadId: expected.threadId,
    controlThreadId: expected.controlThreadId, askActionId: expected.ask.id, answerActionId: expected.answer.id,
    questionReloadStartedAt: expected.questionReloadStartedAt, reloadStartedAt: expected.reloadStartedAt,
    mainDraft: `[remote-e2e:${runId}:question-receipt-main-draft]`,
    controlDraft: `[remote-e2e:${runId}:question-receipt-control-draft]` };
  for (const phase of ["before", "pendingInitial", "pending", "unknown", "answered", "history", "delivery"]) {
    const raw = { ...data[phase], runId, origin, sourceFingerprint: request.sourceFingerprint,
      caseId: request.caseId, operation: "observe-question-recovery", phase, readId: `read-${phase}` };
    const reference = await createEvidenceReference(await put(root, `${phase}.json`, JSON.stringify(raw)));
    stage[phase] = { ...reference, source: "rpc", threadId: expected.threadId, readId: raw.readId,
      rawFacts: Object.fromEntries(Object.keys(raw).map(key => [key, `/${key}`])) };
  }
  for (const [name, index, control] of [
    ["controlBeforeUi", 3.5, true], ["beforeAnswerUi", 4, false], ["unknownUi", 5.8, false],
    ["reloadedUi", 7, false], ["unlockedUi", 8, false], ["controlAfterUi", 9, true],
  ]) {
    const observation = { at: time(index), threadId: control ? expected.controlThreadId : expected.threadId,
      loading: false, promptEditable: true, prompt: control ? stage.controlDraft : stage.mainDraft,
      approvalText: name === "beforeAnswerUi" ? "颜色？形状？" : "", notice: "", confirmation: null,
      send: { action: "send" },
      menuItems: ["unknownUi", "unlockedUi"].includes(name)
        ? [{ name: "删除会话", visible: true, enabled: name === "unlockedUi" }] : [] };
    stage[name] = { kind: "ui", threadId: observation.threadId, observation, files: {
      dom: await createEvidenceReference(await put(root, `${name}.dom.txt`, `- textbox: ${observation.prompt}\n`)),
      screenshot: await createEvidenceReference(await put(root, `${name}.png`, png)),
      observation: await createEvidenceReference(await put(root, `${name}.json`, JSON.stringify(observation))),
    } };
  }
  return { root, request, stage, time };
}

test("问答恢复文件门禁重读七个阶段、准确草稿和真实删除入口状态", async t => {
  const { root, request, stage } = await questionRecoveryStageFixture(t);
  const result = await verifyQuestionReceiptRecoveryStage(stage, request, { baseDir: root });
  assert.equal(result.originalReceiptId, "original-receipt");
  assert.equal(result.answerSubmissions, 1);
});

test("问答恢复拒绝只有汇总、删除锁未解除、草稿交换、伪改观察和阶段倒退", async t => {
  for (const mutate of [
    async ({ stage }) => { stage.history = { verified: true }; },
    async ({ stage }) => { stage.unlockedUi.observation.menuItems[0].enabled = false;
      await writeFile(stage.unlockedUi.files.observation.path, JSON.stringify(stage.unlockedUi.observation));
      stage.unlockedUi.files.observation = await createEvidenceReference(stage.unlockedUi.files.observation.path); },
    async ({ stage }) => { stage.controlAfterUi.observation.prompt = stage.mainDraft; },
    async ({ stage }) => { [stage.mainDraft, stage.controlDraft] = [stage.controlDraft, stage.mainDraft]; },
    async ({ stage, time }) => { stage.reloadStartedAt = time(8); },
    async ({ stage }) => { stage.unknownUi.observation.threadId = "control"; },
    async ({ stage }) => { stage.reloadedUi.observation.notice = "回答已提交";
      await writeFile(stage.reloadedUi.files.observation.path, JSON.stringify(stage.reloadedUi.observation));
      stage.reloadedUi.files.observation = await createEvidenceReference(stage.reloadedUi.files.observation.path); },
    async ({ stage }) => { stage.pending = stage.pendingInitial; },
    async ({ stage }) => { stage.unknownUi.observation.send.action = "stop";
      await writeFile(stage.unknownUi.files.observation.path, JSON.stringify(stage.unknownUi.observation));
      stage.unknownUi.files.observation = await createEvidenceReference(stage.unknownUi.files.observation.path); },
  ]) {
    const sample = await questionRecoveryStageFixture(t); await mutate(sample);
    await assert.rejects(verifyQuestionReceiptRecoveryStage(sample.stage, sample.request, { baseDir: sample.root }));
  }
});

test("问答恢复固定用例即使删除专项记录和要求也不能以旧通用回执通过", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const caseId = "recovery.question_receipt";
  const record = await proof(root, { sourceFingerprint: report.sourceFingerprint, caseId,
    operation: "read", receiptState: "completed" }, { actionId: undefined });
  report.actions = {};
  report.cases[0] = { ...report.cases[0], id: caseId,
    nativeExpectations: { expectedFacts: { receiptState: "completed" } },
    evidence: [ui, { kind: "native", records: [record], request: {
      runId, caseId, actions: [], expectedFacts: { receiptState: "completed" },
    } }] };
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: { requiresQuestionReceiptRecovery: false } };
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /问答回执恢复固定用例缺少唯一专项记录/);
});

test("截图按原始字节识别 PNG、JPEG 和 WebP，而不依赖文件后缀", () => {
  assert.deepEqual(screenshotFileType(png), { extension: "png", mimeType: "image/png" });
  assert.deepEqual(screenshotFileType(jpegSignatureSample), { extension: "jpg", mimeType: "image/jpeg" });
  assert.deepEqual(screenshotFileType(Uint8Array.from([82, 73, 70, 70, 4, 0, 0, 0, 87, 69, 66, 80])),
    { extension: "webp", mimeType: "image/webp" });
});

test("截图拒绝文本、空字节和缺少结束标记的 JPEG", () => {
  for (const bytes of [new Uint8Array(), new TextEncoder().encode("not an image"), jpegSignatureSample.slice(0, -2)])
    assert.throws(() => screenshotFileType(bytes), /截图证据/);
});

async function sandbox(t) {
  const directory = await mkdtemp(join(tmpdir(), "remote-access-evidence-"));
  const cleanupTarget = await realpath(directory);
  const temporaryRoot = await realpath(tmpdir());
  assert.ok(cleanupTarget.startsWith(`${temporaryRoot}${sep}`));
  t.after(async () => {
    const current = await realpath(directory);
    assert.equal(current, cleanupTarget);
    await rm(cleanupTarget, { recursive: true, force: true });
  });
  return directory;
}

async function put(root, path, value) {
  const absolute = join(root, path);
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, value);
  return absolute;
}

test("跨 VM 的真实形状 DOM 观察在正式 runner 保存后仍与 JSON 证据逐值一致", async t => {
  const outputDir = await sandbox(t);
  const dom = runInNewContext(`({
    canCreate: true, connection: "12 ms", send: { label: "发送消息", enabled: false },
    sidebar: [{ id: "existing", title: "已有会话", selected: false }],
    turns: [], menuItems: [], prompt: "", promptEditable: true, loading: false
  })`);
  assert.notEqual(Object.getPrototypeOf(dom.sidebar), Array.prototype);
  // 此处只验证浏览器工具跨 VM 返回 JSON 数据的接入边界，不是功能 E2E。
  const tab = { id: "vm-contract-tab", url: async () => origin,
    reload: async () => { throw new Error("此只读观察用例不应刷新"); },
    playwright: { evaluate: async () => dom, domSnapshot: async () => '- button "新建会话"\n' },
    screenshot: async () => png };
  const context = await createRemoteAccessE2EContext({ tab, targetOrigin: origin, runId, outputDir });
  await runRemoteAccessCase("read.ready", context);
  const entry = context.report.cases.find(value => value.id === "read.ready");
  assert.equal(entry.status, "passed");
  const saved = JSON.parse(await readFile(join(outputDir, "report.json"), "utf8"));
  const savedEntry = saved.cases.find(value => value.id === "read.ready");
  assert.deepEqual(entry.evidence.filter(value => value.kind === "ui").map(value => value.observation),
    savedEntry.evidence.filter(value => value.kind === "ui").map(value => value.observation));
});

test("浏览器工具环境没有全局 process 或 Buffer 时仍能保存和核验证据", async t => {
  const root = await sandbox(t);
  const path = await put(root, "observation.json", JSON.stringify({ threadId: "owned-thread" }));
  const script = `
    import nativeProcess from "node:process";
    const [moduleUrl, evidencePath] = nativeProcess.argv.slice(1);
    delete globalThis.process;
    delete globalThis.Buffer;
    const evidence = await import(moduleUrl);
    const reference = await evidence.createEvidenceReference(evidencePath);
    const verified = await evidence.verifyEvidenceReference(reference);
    console.log(JSON.stringify({
      sha256: verified.sha256,
      bytes: verified.bytes.length,
      processGlobal: typeof globalThis.process,
      bufferGlobal: typeof globalThis.Buffer
    }));
  `;
  const { stdout } = await executeFile(process.execPath, [
    "--input-type=module", "-e", script,
    new URL("./remote-access-e2e-evidence.mjs", import.meta.url).href, path,
  ], { windowsHide: true });
  const result = JSON.parse(stdout);
  assert.match(result.sha256, /^[a-f0-9]{64}$/);
  assert.equal(result.bytes, (await readFile(path)).length);
  assert.equal(result.processGlobal, "undefined");
  assert.equal(result.bufferGlobal, "undefined");
});

async function sourceRepository(t) {
  const root = await sandbox(t);
  await executeFile("git", ["init", "-q", root], { windowsHide: true });
  const files = {
    ".gitignore": "temp/\n**/build/\n**/local.properties\n**/*.jks\n**/ignored-module.js\n",
    "Cargo.toml": "[workspace]\nmembers=[]\n",
    "Cargo.lock": "# fixture\n",
    "assets/remote-access/app.js": "export const app = 1;\n",
    "assets/remote-access/recovery-store.js": "export const recovery = 1;\n",
    "assets/remote-access/markdown.js": "export const markdown = 1;\n",
    "assets/remote-access/nested/platform.js": "export const platform = 1;\n",
    "assets/inject/renderer-features.js": "// native runtime bridge\n",
    "crates/codex-elves-core/src/remote_access.rs": "mod notifications;\n",
    "crates/codex-elves-core/src/remote_access/context_usage.rs": "// context\n",
    "crates/codex-elves-core/src/remote_access/prompt_optimization.rs": "// prompt route\n",
    "crates/codex-elves-core/src/remote_access/notifications.rs": "// notifications\n",
    "crates/codex-elves-core/src/codex_service.rs": "// service\n",
    "crates/codex-elves-core/src/prompt_optimize.rs": "// optimizer\n",
    "crates/codex-elves-core/src/routes.rs": "// routes\n",
    "apps/codex-elves-android/app/src/main/java/example/ResourceStore.java": "// update\n",
    "apps/codex-elves-android/app/src/main/java/example/NotificationService.java": "// notification\n",
    "apps/codex-elves-android/app/src/main/AndroidManifest.xml": "<manifest />\n",
    "apps/codex-elves-android/app/build.gradle": "// bundledWebResources\n",
    "apps/codex-elves-android/version.properties": "versionName=0.0.1\n",
    "scripts/build-android.ps1": "# android\n",
    "scripts/remote-access-e2e.mjs": "// runner\n",
    "scripts/remote-access-e2e-evidence.mjs": "// verifier\n",
    "scripts/remote-access-e2e-environment.mjs": "// environment lease\n",
  };
  for (const [path, bytes] of Object.entries(files)) await put(root, path, bytes);
  return { root, files };
}

function baseRequest(extra = {}) {
  return {
    runId, caseId: "session.send_stream", origin, sourceFingerprint: "a".repeat(64),
    runStartedAt: new Date(Date.now() - 60_000).toISOString(),
    threadIds: ["thread-main"], turnIds: ["turn-1"],
    expectedFacts: { turnStatus: "completed" },
    actions: [{
      id: `${runId}:session.send_stream:send`, caseId: "session.send_stream",
      kind: "send", status: "confirmed", threadId: "thread-main",
      result: { turnId: "turn-1", receiptId: "receipt-1" },
    }],
    ...extra,
  };
}

async function proof(root, rawChanges = {}, recordChanges = {}, file = "native.json") {
  const raw = {
    runId, caseId: "session.send_stream", origin, sourceFingerprint: "a".repeat(64),
    capturedAt: new Date().toISOString(), threadId: "thread-main",
    turnId: "turn-1", receiptId: "receipt-1",
    actionId: `${runId}:session.send_stream:send`,
    operation: "send", turnStatus: "completed", prompt: `[${runId}] 原始消息`,
    ...rawChanges,
  };
  const path = await put(root, file, `${JSON.stringify(raw, null, 2)}\n`);
  const reference = await createEvidenceReference(path, { baseDir: root });
  return {
    source: "rpc", evidencePath: reference.path, sha256: reference.sha256, size: reference.size,
    outcomePointer: "",
    actionId: `${runId}:session.send_stream:send`,
    threadId: raw.threadId, turnId: raw.turnId, receiptId: raw.receiptId,
    rawFacts: Object.fromEntries(Object.entries(raw).filter(([, value]) => value !== undefined)
      .map(([key]) => [key, `/${key}`])),
    ...recordChanges,
  };
}

async function reportFixture(t) {
  const { root } = await sourceRepository(t);
  const fingerprint = await sourceFingerprint({ root });
  const native = await proof(root, { sourceFingerprint: fingerprint });
  const domPath = await put(root, "ui.dom.txt", "- heading \"回归会话\"\n- text: 原始消息\n");
  const pngPath = await put(root, "ui.png", png);
  const finishedAt = new Date(Date.now() + 1_000).toISOString();
  const ui = {
    kind: "ui", threadId: "thread-main", observation: { threadId: "thread-main" },
    files: {
      dom: await createEvidenceReference(domPath, { baseDir: root }),
      screenshot: await createEvidenceReference(pngPath, { baseDir: root }),
    },
  };
  const request = baseRequest({ sourceFingerprint: fingerprint });
  const report = {
    schemaVersion: 1, scriptVersion, sourceFingerprint: fingerprint, runId, origin,
    startedAt: request.runStartedAt, runStartedAt: request.runStartedAt, finishedAt,
    overall: "passed", actions: Object.fromEntries(request.actions.map(action => [action.id, action])),
    cases: [{
      id: "session.send_stream", required: true, status: "passed", stepsCompleted: true, inProgress: false,
      attempts: [{ status: "passed", finishedAt }],
      nativeExpectations: { threadIds: ["thread-main"], turnIds: ["turn-1"] },
      evidence: [ui, { kind: "native", records: [native], request }],
    }],
  };
  const options = {
    root, baseDir: root, scriptVersion,
    requiredCases: [{ id: "session.send_stream", required: true }],
    caseContracts: { "session.send_stream": { expectedFacts: { turnStatus: "completed" } } },
  };
  return { root, report, options, native, ui };
}

async function restoreReportFixture(t) {
  const { root, report, options } = await reportFixture(t);
  const originalInput = `原输入\n[${runId}]`;
  const optimizedPrompt = `优化后的输入\n[${runId}]`;
  const actionId = `${runId}:optimizer.restore:optimize`;
  const request = baseRequest({
    sourceFingerprint: report.sourceFingerprint, caseId: "optimizer.restore", turnIds: [],
    expectedFacts: { originalInput, optimizationState: "completed" },
    actions: [{ id: actionId, caseId: "optimizer.restore", kind: "optimizePrompt",
      status: "confirmed", threadId: "thread-main" }],
  });
  const record = await proof(root, {
    sourceFingerprint: report.sourceFingerprint, caseId: "optimizer.restore",
    actionId, operation: "optimizePrompt", receiptState: "completed",
    originalInput, optimizedPrompt, optimizationState: "completed",
  }, { actionId }, "optimization.json");
  const instant = Date.now();
  async function uiCapture(label, prompt, offset) {
    return {
      kind: "ui", threadId: "thread-main", at: new Date(instant + offset).toISOString(),
      observation: { threadId: "thread-main", prompt, at: new Date(instant + offset + 10).toISOString() },
      files: {
        dom: await createEvidenceReference(await put(root, `${label}.dom.txt`, `- textbox "消息输入": ${JSON.stringify(prompt)}\n`), { baseDir: root }),
        screenshot: await createEvidenceReference(await put(root, `${label}.png`, png), { baseDir: root }),
      },
    };
  }
  const beforeUi = await uiCapture("before-restore", optimizedPrompt, -500);
  const afterUi = await uiCapture("after-restore", originalInput, -300);
  const restore = {
    kind: "local-ui-restore", threadId: "thread-main", originalInput, optimizedPrompt,
    at: new Date(instant - 100).toISOString(), beforeUi, afterUi,
  };
  report.actions = { [actionId]: request.actions[0] };
  report.cases[0] = {
    ...report.cases[0], id: "optimizer.restore", nativeExpectations: request,
    evidence: [beforeUi, afterUi, { kind: "native", records: [record], request }, restore],
  };
  options.requiredCases = [{ id: "optimizer.restore", required: true }];
  options.caseContracts = { "optimizer.restore": { requiredUiRestores: 1 } };
  return { root, report, options, restore, record };
}

async function leaseReportFixture(t) {
  const { root, report, options, ui } = await reportFixture(t);
  const caseId = "recovery.reconnect";
  const leaseKey = `${runId}:${caseId}:environment`;
  const instanceId = `${runId}-dedicated-instance`;
  const originalState = { cdp: true, service: false };
  const instant = Date.now();
  const actions = {};
  const stages = [];
  async function stage(operation, suffix, offset, extra, mutation) {
    const actionId = `${runId}:${caseId}:${suffix}`;
    const receiptId = `${suffix}-receipt`;
    const capturedAt = new Date(instant + offset).toISOString();
    const action = { id: actionId, caseId, kind: operation, threadId: "thread-main",
      status: "confirmed", result: { receiptId } };
    if (mutation) actions[actionId] = action;
    const record = await proof(root, {
      caseId, sourceFingerprint: report.sourceFingerprint, actionId, operation, capturedAt, receiptId,
      receiptState: "completed", leaseKey, leaseOwnerRunId: runId, instanceId,
      unownedStateChanged: false, ...extra,
    }, { actionId: mutation ? actionId : undefined }, `${suffix}.json`);
    const value = {
      kind: "driver", driver: "environment", operation, records: [record], uiEvidence: [],
      request: {
        contractVersion: "remote-e2e-driver/2", runId, caseId, operation, origin,
        sourceFingerprint: report.sourceFingerprint, runStartedAt: report.runStartedAt,
        capturedAfter: new Date(instant + offset - 1).toISOString(),
        threadIds: ["thread-main"], actions: mutation ? [action] : [],
        expectedFacts: { leaseKey, leaseOwnerRunId: runId, instanceId, unownedStateChanged: false,
          ...(operation === "inspect-environment-lease" ? { stateMatchesOriginal: true } : {}) },
      },
    };
    stages.push(value);
    return value;
  }
  await stage("acquire-environment-lease", "lease-acquire", -2_000,
    { originalState, leaseAcquired: true }, true);
  await stage("restore-environment-resource", "restore-cdp", -1_500,
    { resource: "cdp", restoreDisposition: "restored" }, true);
  await stage("restore-environment-resource", "restore-service", -1_200,
    { resource: "service", restoreDisposition: "already-original" }, true);
  const inspection = await stage("inspect-environment-lease", "lease-inspect", -1_000,
    { originalState, currentState: originalState, stateMatchesOriginal: true, connected: true }, false);
  report.actions = actions;
  report.cases[0] = {
    ...report.cases[0], id: caseId, nativeExpectations: { threadIds: ["thread-main"] },
    evidence: [ui, ...stages],
  };
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: {
    expectedFacts: { connected: true },
    requiredDriverSteps: [
      { driver: "environment", operation: "acquire-environment-lease", count: 1 },
      { driver: "environment", operation: "restore-environment-resource", count: 2 },
      { driver: "environment", operation: "inspect-environment-lease", count: 1 },
    ],
  } };
  return { root, report, options, inspection, stages };
}

test("源码指纹覆盖网页模块、全部 core 子模块、Android 更新通知、runner 和自身", async t => {
  const { root, files } = await sourceRepository(t);
  const inputs = await listRemoteAccessSourceInputs({ root });
  for (const path of Object.keys(files)) assert.ok(inputs.includes(path), path);
  const first = await sourceFingerprint({ root });
  await put(root, "crates/codex-elves-core/src/remote_access/new_route.rs", "// new route\n");
  assert.notEqual(await sourceFingerprint({ root }), first);
  const beforeHelper = await sourceFingerprint({ root });
  await put(root, "scripts/remote-access-e2e-driver.mjs", "// new execution helper\n");
  assert.ok((await listRemoteAccessSourceInputs({ root })).includes("scripts/remote-access-e2e-driver.mjs"));
  assert.notEqual(await sourceFingerprint({ root }), beforeHelper);
});

test("源码指纹尊重 ignore，并排除构建产物和本机签名凭据", async t => {
  const { root } = await sourceRepository(t);
  const first = await sourceFingerprint({ root });
  for (const path of [
    "assets/remote-access/ignored-module.js", "assets/remote-access/build/generated.js",
    "apps/codex-elves-android/local.properties", "apps/codex-elves-android/release.jks",
    "apps/codex-elves-android/secrets.json", "temp/capture.json",
    "scripts/remote-access-e2e-environment.test.mjs", "scripts/remote-access-e2e-evidence.test.mjs",
  ]) await put(root, path, "local-only\n");
  assert.equal(await sourceFingerprint({ root }), first);
});

test("修改曾漏掉的输入逐项使源码指纹失效", async t => {
  const { root } = await sourceRepository(t);
  for (const path of [
    "assets/remote-access/nested/platform.js",
    "crates/codex-elves-core/src/remote_access/context_usage.rs",
    "crates/codex-elves-core/src/codex_service.rs",
    "crates/codex-elves-core/src/prompt_optimize.rs",
    "crates/codex-elves-core/src/routes.rs",
    "apps/codex-elves-android/app/src/main/java/example/ResourceStore.java",
    "apps/codex-elves-android/app/src/main/java/example/NotificationService.java",
    "apps/codex-elves-android/app/build.gradle",
    "scripts/remote-access-e2e.mjs", "scripts/remote-access-e2e-evidence.mjs",
    "scripts/remote-access-e2e-environment.mjs",
  ]) {
    const before = await sourceFingerprint({ root });
    await put(root, path, `${await readFile(join(root, path), "utf8")}// changed\n`);
    assert.notEqual(await sourceFingerprint({ root }), before, path);
  }
});

test("源码输入丢失时拒绝生成看似有效的指纹", async t => {
  const { root } = await sourceRepository(t);
  await rm(join(root, "assets/remote-access/app.js"));
  await assert.rejects(sourceFingerprint({ root }), /缺少必需输入/);
});

test("证据引用按原始字节核对 SHA256、大小和相对路径", async t => {
  const root = await sandbox(t);
  const file = await put(root, "capture.json", "\uFEFF{\"ok\":true}\r\n");
  const reference = await createEvidenceReference(file, { baseDir: root });
  const verified = await verifyEvidenceReference(reference, { baseDir: root });
  assert.equal(verified.path, resolve(file));
  assert.equal(verified.bytes.toString("utf8"), "\uFEFF{\"ok\":true}\r\n");
  await assert.rejects(verifyEvidenceReference({ ...reference, size: reference.size + 1 }, { baseDir: root }), /大小不匹配/);
});

test("证据路径丢失、缺少哈希和文件篡改均被拒绝", async t => {
  const root = await sandbox(t);
  const path = await put(root, "capture.json", "{\"ok\":true}");
  const reference = await createEvidenceReference(path, { baseDir: root });
  await assert.rejects(verifyEvidenceReference({ path: reference.path }, { baseDir: root }), /SHA-256/);
  await writeFile(path, "{\"ok\":false}");
  await assert.rejects(verifyEvidenceReference(reference, { baseDir: root }), /SHA-256 不匹配/);
  await rm(path);
  await assert.rejects(verifyEvidenceReference(reference, { baseDir: root }), /ENOENT/);
});

test("JSON Pointer 支持根、转义和数组，并拒绝缺失字段及错误转义", () => {
  const value = { "a/b": { "~key": [false] } };
  assert.equal(jsonPointer(value, "/a~1b/~0key/0"), false);
  assert.equal(jsonPointer(value, ""), value);
  assert.throws(() => jsonPointer(value, "/missing"), /没有 JSON Pointer/);
  assert.throws(() => jsonPointer(value, "/a~2b"), /转义无效/);
});

test("真实原始事实满足固定语义、身份和本轮来源时接受", async t => {
  const root = await sandbox(t);
  const record = await proof(root);
  const [verified] = await verifyNativeEvidence([record], baseRequest({
    expectations: [{ fact: "prompt", op: "includes", value: runId }],
  }), { baseDir: root });
  assert.equal(verified.facts.turnStatus, "completed");
  assert.equal(verified.sha256, record.sha256);
});

test("verified:true 和调用方伪造 facts 不能代替文件及指针", async t => {
  const root = await sandbox(t);
  await assert.rejects(verifyNativeEvidence([{
    verified: true, source: "rpc", threadId: "thread-main", receiptId: "receipt-1",
    actionId: baseRequest().actions[0].id, facts: { turnStatus: "completed" },
  }], baseRequest(), { baseDir: root }), /path/);
  const record = await proof(root, {}, { rawFacts: {}, verified: true, facts: { turnStatus: "completed" } });
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /rawFacts/);
});

for (const [name, rawChanges, recordChanges, message] of [
  ["thread", { threadId: "other-thread" }, {}, /其他会话/],
  ["turn", { turnId: "other-turn" }, {}, /其他轮次/],
  ["receipt", { receiptId: "other-receipt" }, {}, /回执不匹配/],
  ["record thread", {}, { threadId: "other-thread" }, /threadId/],
  ["record turn", {}, { turnId: "other-turn" }, /turnId/],
  ["record receipt", {}, { receiptId: "other-receipt" }, /receiptId/],
  ["action", { actionId: "other-action" }, {}, /actionId/],
  ["operation", { operation: "stop" }, {}, /operation/],
]) {
  test(`原生证据拒绝错误 ${name} 身份`, async t => {
    const root = await sandbox(t);
    const record = await proof(root, rawChanges, recordChanges);
    await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), message);
  });
}

test("记录自报 expectations 和 verified:true 不能削弱 runner 的固定语义", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { turnStatus: "running" }, {
    verified: true, expectations: [{ fact: "turnStatus", op: "equals", value: "running" }],
  });
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /固定契约.*turnStatus/);
});

test("原生证据拒绝跨轮时间、其他 origin 和其他源码指纹", async t => {
  const root = await sandbox(t);
  for (const [changes, message] of [
    [{ capturedAt: "2000-01-01T00:00:00Z" }, /早于本轮/],
    [{ capturedAt: "2100-01-01T00:00:00Z" }, /未来/],
    [{ origin: "https://other.example.test" }, /origin/],
    [{ sourceFingerprint: "b".repeat(64) }, /sourceFingerprint/],
  ]) await assert.rejects(verifyNativeEvidence([await proof(root, changes)], baseRequest(), { baseDir: root }), message);
});

test("原始时间和 JSON Pointer 缺失时不能用 verified 继续", async t => {
  const root = await sandbox(t);
  const record = await proof(root);
  delete record.rawFacts.capturedAt;
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /取证时间/);
  record.rawFacts.capturedAt = "/does-not-exist";
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /没有 JSON Pointer/);
});

test("停止必须有真实终态，普通 accepted 回执不能视为停止完成", async t => {
  const root = await sandbox(t);
  const request = baseRequest({
    actions: [{ ...baseRequest().actions[0], kind: "stop" }], expectedFacts: {},
  });
  const record = await proof(root, { operation: "stop", turnStatus: "running" });
  await assert.rejects(verifyNativeEvidence([record], request, { baseDir: root }), /turnStatus/);
  const stopped = await proof(root, { operation: "stop", turnStatus: "interrupted" });
  assert.equal((await verifyNativeEvidence([stopped], request, { baseDir: root })).length, 1);
});

test("兼容 readStatePointers，并核对父会话聚合与完成轮次", async t => {
  const root = await sandbox(t);
  const record = await proof(root, {
    parentThreadId: "parent", hasUnreadTurn: true,
    parentSideUnread: [{ id: "thread-main", parentThreadId: "parent" }],
  });
  record.readStatePointers = record.rawFacts;
  delete record.rawFacts;
  const request = baseRequest({
    actions: [], expectedFacts: {}, operation: "read-state",
    expectedReadState: { threadId: "thread-main", turnId: "turn-1",
      hasUnreadTurn: true, parentThreadId: "parent", parentSideUnread: true },
  });
  assert.equal((await verifyNativeEvidence([record], request, { baseDir: root }))[0].facts.parentSideUnread, true);
  await assert.rejects(verifyNativeEvidence([record], {
    ...request, expectedReadState: { ...request.expectedReadState, hasUnreadTurn: false },
  }, { baseDir: root }), /hasUnreadTurn/);
});

test("没有 runner 固定语义时拒绝接受任意相等值", async t => {
  const root = await sandbox(t);
  const record = await proof(root, {}, { expectations: [{ fact: "turnStatus", op: "equals", value: "completed" }] });
  await assert.rejects(verifyNativeEvidence([record], {
    threadIds: ["thread-main"], actions: [],
  }, { baseDir: root }), /runner 定义/);
});

async function runtimeFixture(t, loadedBody = "// runtime asset") {
  const root = await sandbox(t);
  const managerExecutable = await createEvidenceReference(await put(root, "installed-manager.exe", "MZ fixture"), { baseDir: root });
  const remoteAsset = await createEvidenceReference(await put(root, "installed-app.js", "// runtime asset"), { baseDir: root });
  const fields = {
    managerVersion: "1.2.3", managerExecutableSha256: managerExecutable.sha256,
    codexVersion: "9.0.0", remoteAssetSha256: remoteAsset.sha256, serviceProtocolVersion: 6,
  };
  const record = await proof(root, fields);
  const browserAsset = await createEvidenceReference(await put(root, "browser-app.js", loadedBody), { baseDir: root });
  const capture = {
    capture: { runId, caseId: "session.send_stream", origin, sourceFingerprint: "a".repeat(64),
      capturedAt: new Date().toISOString() },
    request: { method: "Network.requestWillBeSent", params: {
      requestId: "script-1", type: "Script", request: { url: `${origin}/app.js` },
    } },
    response: { method: "Network.responseReceived", params: {
      requestId: "script-1", type: "Script", response: { url: `${origin}/app.js`, status: 200 },
    } },
    body: { base64Encoded: false, body: loadedBody },
  };
  const capturePath = await put(root, "browser-resource.json", JSON.stringify(capture));
  const browserAssetEvidence = await createEvidenceReference(capturePath, { baseDir: root });
  const evidence = { ...fields, managerExecutable, remoteAsset, browserAsset, browserAssetEvidence, records: [record],
    evidencePaths: [{ path: record.evidencePath, sha256: record.sha256 }] };
  return { root, evidence, capture, capturePath };
}

test("运行版本必须读取实际安装资产、浏览器实际脚本及版本原始文件", async t => {
  const { root, evidence } = await runtimeFixture(t);
  assert.equal((await verifyRuntimeEvidence(evidence, baseRequest({ actions: [], expectedFacts: {}, turnIds: [] }), { baseDir: root })).length, 1);
  await writeFile(join(root, "installed-app.js"), "// changed runtime");
  await assert.rejects(verifyRuntimeEvidence(evidence, baseRequest({ actions: [], expectedFacts: {} }), { baseDir: root }), /SHA-256/);
});

test("版本数字相同但浏览器仍使用旧缓存脚本时必须拒绝验收", async t => {
  const { root, evidence } = await runtimeFixture(t, "// old cached runtime");
  await assert.rejects(verifyRuntimeEvidence(evidence,
    baseRequest({ actions: [], expectedFacts: {}, turnIds: [] }), { baseDir: root }), /不同版本网页资源/);
});

test("浏览器资源证据拒绝另行预取、错误请求身份或跨来源拼接", async t => {
  for (const alter of [
    capture => { capture.request.params.type = "Fetch"; },
    capture => { capture.response.params.requestId = "another-request"; },
    capture => { capture.request.params.request.url = "https://another.example.test/app.js"; },
    capture => { capture.capture.capturedAt = "2020-01-01T00:00:00.000Z"; },
    capture => { capture.body.body = "// forged browser response"; },
  ]) {
    const { root, evidence, capture, capturePath } = await runtimeFixture(t);
    alter(capture);
    await writeFile(capturePath, JSON.stringify(capture));
    evidence.browserAssetEvidence = await createEvidenceReference(capturePath, { baseDir: root });
    await assert.rejects(verifyRuntimeEvidence(evidence,
      baseRequest({ actions: [], expectedFacts: {}, turnIds: [] }), { baseDir: root }), /浏览器|脚本请求/);
  }
});

test("报告读取截图、DOM 与原生文件后接受完整 happy path", async t => {
  const { root, report, options } = await reportFixture(t);
  const path = await put(root, "report.json", JSON.stringify(report));
  const result = await verifyRemoteAccessReport(path, options);
  assert.deepEqual(result, { passed: true, problems: [], runId });
});

test("报告拒绝源码已变化或 runner 版本不一致", async t => {
  const { root, report, options } = await reportFixture(t);
  let result = await verifyRemoteAccessReport({ ...report, scriptVersion: "old" }, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /脚本版本/);
  await put(root, "crates/codex-elves-core/src/codex_service.rs", "// changed service");
  result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /当前源代码/);
});

test("报告拒绝文件丢失、DOM 篡改、原生篡改和伪造 PNG", async t => {
  for (const kind of ["missing", "dom", "native", "png"]) {
    const { root, report, options, native, ui } = await reportFixture(t);
    if (kind === "missing") await rm(join(root, ui.files.screenshot.path));
    if (kind === "dom") await writeFile(join(root, ui.files.dom.path), "tampered");
    if (kind === "native") await writeFile(join(root, native.evidencePath), "{\"verified\":true}");
    if (kind === "png") {
      await writeFile(join(root, ui.files.screenshot.path), "not an image");
      ui.files.screenshot = await createEvidenceReference(join(root, ui.files.screenshot.path), { baseDir: root });
    }
    const result = await verifyRemoteAccessReport(report, options);
    assert.equal(result.passed, false, kind);
    assert.match(result.problems.join("\n"), /ENOENT|SHA-256|PNG/, kind);
  }
});

test("报告拒绝必测案例缺失、重复、未完成、进行中和 unknown 操作", async t => {
  const { report, options } = await reportFixture(t);
  for (const change of [
    value => { value.cases = []; },
    value => { value.cases.push(structuredClone(value.cases[0])); },
    value => { value.cases[0].status = "not_run"; },
    value => { value.cases[0].stepsCompleted = false; },
    value => { value.cases[0].inProgress = true; },
    value => { Object.values(value.actions)[0].status = "unknown"; },
  ]) {
    const changed = structuredClone(report);
    change(changed);
    assert.equal((await verifyRemoteAccessReport(changed, options)).passed, false);
  }
});

test("报告不能靠 passed、nativeVerified、verified 布尔绕过证据", async t => {
  const { report, options } = await reportFixture(t);
  const entry = report.cases[0];
  entry.nativeVerified = true;
  entry.evidence = [{ kind: "ui", verified: true }, { kind: "native", records: [{ verified: true }] }];
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /path/);
});

test("报告的固定 case 契约不能被报告自报的较弱期待替换", async t => {
  const { root, report, options } = await reportFixture(t);
  const record = await proof(root, { sourceFingerprint: report.sourceFingerprint, turnStatus: "running" });
  report.cases[0].nativeExpectations = { expectedFacts: { turnStatus: "running" } };
  report.cases[0].evidence[1].records = [record];
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /turnStatus/);
});

test("FIFO 报告不能删除专项契约和终态历史后继续通过", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const { history, expected } = fifoSample();
  const caseId = "queue.automatic", threadId = "thread-main";
  const baseId = `${runId}:${caseId}:base`;
  const actions = [{ id: baseId, caseId, kind: "send", status: "confirmed", threadId, turnId: "base" }];
  const records = [await proof(root, { caseId, sourceFingerprint: report.sourceFingerprint,
    actionId: baseId, turnId: "base", receiptId: "base-receipt", receiptState: "completed" },
  { actionId: baseId }, "fifo-base.json")];
  const entries = [];
  for (const [index, entry] of expected.entries.entries()) {
    const actionId = `${runId}:${caseId}:queue-${index}`;
    actions.push({ id: actionId, caseId, kind: "queue-send", status: "confirmed", threadId, queueId: entry.queueId });
    entries.push({ ...entry, actionId });
    records.push(await proof(root, { caseId, sourceFingerprint: report.sourceFingerprint,
      actionId, operation: "queue-send", turnId: undefined, receiptId: `queue-receipt-${index}`,
      receiptState: "completed", deliveryState: "queued", queueId: entry.queueId,
      clientUserMessageId: entry.clientUserMessageId },
    { actionId }, `fifo-queue-${index}.json`));
  }
  const snapshot = await proof(root, { caseId, sourceFingerprint: report.sourceFingerprint,
    actionId: undefined, turnId: undefined, receiptId: "history-read", operation: "read",
    nativeTurns: history.turns, queuedMessages: [], historyComplete: true },
  { actionId: undefined }, "fifo-completed-history.json");
  const expectations = { threadIds: [threadId], naturalQueue: { threadId, previousTurnId: "base", entries } };
  const request = { runId, caseId, origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt, actions, ...expectations };
  report.actions = Object.fromEntries(actions.map(action => [action.id, action]));
  report.cases = [{ ...report.cases[0], id: caseId, nativeExpectations: expectations,
    evidence: [ui, { kind: "native", records: [...records, snapshot], request }] }];
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: REMOTE_ACCESS_E2E_CONTRACTS[caseId] };
  const complete = await verifyRemoteAccessReport(report, options);
  assert.equal(complete.passed, true, complete.problems.join("\n"));
  delete report.cases[0].nativeExpectations.naturalQueue;
  report.cases[0].nativeExpectations.requiresNaturalQueue = false;
  delete report.cases[0].evidence[1].request.naturalQueue;
  report.cases[0].evidence[1].records = records;
  const stripped = await verifyRemoteAccessReport(report, options);
  assert.equal(stripped.passed, false);
  assert.match(stripped.problems.join("\n"), /FIFO 固定用例/);
});

test("完整引导报告保留操作前轮次，删除该基线后不能用结果轮次通过", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const caseId = "queue.steer", threadId = "thread-main", turnId = "active-before-click";
  const actionId = `${runId}:${caseId}:steer`;
  const action = { id: actionId, caseId, kind: "queue-steer", status: "confirmed",
    startedAt: new Date(Date.now() - 1_000).toISOString(),
    threadId, queueId: "queued-D", turnId, result: { turnId } };
  const expectations = { threadIds: [threadId], turnIds: [turnId], expectedFacts: { turnId },
    queueSteerBefore: await steeringBeforeProof(root) };
  const request = { runId, caseId, origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt, actions: [action], ...expectations };
  const record = await proof(root, { caseId, sourceFingerprint: report.sourceFingerprint, actionId,
    operation: "steerQueued", queueId: "queued-D", turnId, receiptState: "completed", turnStatus: undefined },
  { actionId }, "steer-report.json");
  report.actions = { [actionId]: action };
  report.cases = [{ ...report.cases[0], id: caseId, nativeExpectations: expectations,
    evidence: [ui, { kind: "native", records: [record], request }] }];
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: REMOTE_ACCESS_E2E_CONTRACTS[caseId] || {} };
  const complete = await verifyRemoteAccessReport(report, options);
  assert.equal(complete.passed, true, complete.problems.join("\n"));
  const rewritten = structuredClone(report);
  const rewrittenCase = rewritten.cases[0];
  rewritten.actions[actionId].turnId = rewritten.actions[actionId].result.turnId = "new-turn-after-click";
  rewrittenCase.nativeExpectations.turnIds = ["new-turn-after-click"];
  rewrittenCase.nativeExpectations.expectedFacts.turnId = "new-turn-after-click";
  const rewrittenRequest = rewrittenCase.evidence[1].request;
  rewrittenRequest.turnIds = ["new-turn-after-click"];
  rewrittenRequest.expectedFacts.turnId = "new-turn-after-click";
  rewrittenRequest.actions[0].turnId = rewrittenRequest.actions[0].result.turnId = "new-turn-after-click";
  rewrittenCase.evidence[1].records = [await proof(root, { caseId,
    sourceFingerprint: report.sourceFingerprint, actionId, operation: "steerQueued",
    queueId: "queued-D", turnId: "new-turn-after-click", receiptState: "completed", turnStatus: undefined },
  { actionId }, "steer-report-rewritten.json")];
  const rewrittenResult = await verifyRemoteAccessReport(rewritten, options);
  assert.equal(rewrittenResult.passed, false);
  assert.match(rewrittenResult.problems.join("\n"), /引导固定用例的原始观察/);
  delete action.turnId;
  const missingBaseline = await verifyRemoteAccessReport(report, options);
  assert.equal(missingBaseline.passed, false);
  assert.match(missingBaseline.problems.join("\n"), /引导固定用例/);
});

test("外部驱动场景也必须由原始文件满足固定契约并完成每一步", async t => {
  const { root, report, options } = await reportFixture(t);
  const record = await proof(root, {
    sourceFingerprint: report.sourceFingerprint, caseId: "scenario.approval", approvalStatus: "approved",
  }, { actionId: undefined });
  report.actions = {};
  report.cases[0] = {
    ...report.cases[0], id: "scenario.approval", nativeExpectations: undefined,
    evidence: [report.cases[0].evidence[0], {
      kind: "manual-scenario", status: "passed", threadId: "thread-main",
      steps: [{ id: "approve", status: "passed", records: [record] }], nativeEvidence: [record],
    }],
  };
  options.requiredCases = [{ id: "scenario.approval", required: true }];
  options.caseContracts = { "scenario.approval": { expectedFacts: { approvalStatus: "approved" } } };
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  report.cases[0].evidence[1].steps[0].status = "unknown";
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
  report.cases[0].evidence[1].steps[0].status = "passed";
  options.caseContracts["scenario.approval"].expectedFacts.approvalStatus = "rejected";
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
});

test("typed driver 的原始 operation 与动作身份严格一致，动作中取证无需预填 confirmed", async t => {
  const root = await sandbox(t);
  const request = baseRequest({
    contractVersion: "remote-e2e-driver/2", operation: "rename-thread",
    expectedFacts: { title: "新标题" },
    actions: [{ id: baseRequest().actions[0].id, kind: "rename-thread", threadId: "thread-main" }],
  });
  const record = await proof(root, { operation: "rename-thread", title: "新标题" });
  assert.equal((await verifyNativeEvidence([record], request, { baseDir: root })).length, 1);
  const wrong = await proof(root, { operation: "observe-thread", title: "新标题" });
  await assert.rejects(verifyNativeEvidence([wrong], request, { baseDir: root }), /operation/);
});

test("报告离线重验 typed driver 每步原始事实及桌面界面文件", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const record = await proof(root, {
    sourceFingerprint: report.sourceFingerprint, operation: "observe-thread", title: "原生标题",
  }, { actionId: undefined });
  report.actions = {};
  report.cases[0].nativeExpectations = undefined;
  report.cases[0].evidence = [ui, {
    kind: "driver", driver: "desktop", operation: "observe-thread",
    records: [record], uiEvidence: [{ dom: ui.files.dom, screenshot: ui.files.screenshot }],
    request: {
      runId, caseId: "session.send_stream", contractVersion: "remote-e2e-driver/2",
      operation: "observe-thread", threadIds: ["thread-main"], actions: [],
      expectations: [{ fact: "title", op: "equals", value: "原生标题" }],
    },
  }];
  options.caseContracts = { "session.send_stream": { expectedFacts: { title: "原生标题" } } };
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  report.cases[0].evidence[1].request.expectations[0].value = "伪造标题";
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
  report.cases[0].evidence[1].request.expectations[0].value = "原生标题";
  report.cases[0].evidence[1].uiEvidence[0].screenshot = { path: "missing.png", sha256: "a".repeat(64) };
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
});

test("同案例跨会话分阶段原生核验均须满足各自请求，最后阶段不能覆盖前面期待", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const mainRequest = baseRequest({
    sourceFingerprint: report.sourceFingerprint,
    expectedFacts: { turnStatus: "completed", modelId: "gpt-a", protocol: "responses" },
  });
  const controlAction = {
    ...mainRequest.actions[0], id: `${runId}:session.send_stream:control`, threadId: "thread-control",
    result: { turnId: "turn-2", receiptId: "receipt-2" },
  };
  const controlRequest = baseRequest({
    sourceFingerprint: report.sourceFingerprint, threadIds: ["thread-control"], turnIds: ["turn-2"],
    actions: [controlAction], expectedFacts: { turnStatus: "completed", modelId: "gpt-b", protocol: "responses" },
  });
  const main = await proof(root, {
    sourceFingerprint: report.sourceFingerprint, modelId: "gpt-a", protocol: "responses",
  }, {}, "native-main.json");
  const control = await proof(root, {
    sourceFingerprint: report.sourceFingerprint, threadId: "thread-control", turnId: "turn-2",
    receiptId: "receipt-2", actionId: controlAction.id, modelId: "gpt-b", protocol: "responses",
  }, { actionId: controlAction.id }, "native-control.json");
  report.actions[controlAction.id] = controlAction;
  report.cases[0].nativeExpectations = {
    threadIds: ["thread-control"], turnIds: ["turn-2"], model: { id: "gpt-b", protocol: "responses" },
  };
  report.cases[0].evidence = [
    ui, { kind: "native", records: [main], request: mainRequest },
    { kind: "native", records: [control], request: controlRequest },
  ];
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  mainRequest.expectedFacts.modelId = "gpt-b";
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /modelId/);
});

test("JSON-RPC 数值回执 ID 按原始类型严格比较，包含零值", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { receiptId: 0 });
  const request = baseRequest();
  request.actions[0].result.receiptId = 0;
  assert.equal((await verifyNativeEvidence([record], request, { baseDir: root }))[0].facts.receiptId, 0);
  request.actions[0].result.receiptId = "0";
  await assert.rejects(verifyNativeEvidence([record], request, { baseDir: root }), /回执不匹配/);
});

test("固定契约支持必要的包含、排除和枚举比较，拒绝未知操作", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { queueIds: ["queue-a", "queue-b"] });
  const request = baseRequest({
    expectations: [
      { fact: "prompt", op: "includes", value: runId },
      { fact: "queueIds", op: "includes", value: "queue-a" },
      { fact: "queueIds", op: "notIncludes", value: "queue-removed" },
      { fact: "turnStatus", op: "oneOf", value: ["completed", "interrupted"] },
      { fact: "threadId", op: "notEquals", value: "other-thread" },
    ],
  });
  assert.equal((await verifyNativeEvidence([record], request, { baseDir: root })).length, 1);
  request.expectations.push({ fact: "turnStatus", op: "trustVerified", value: true });
  await assert.rejects(verifyNativeEvidence([record], request, { baseDir: root }), /不支持/);
});

for (const [kind, operation, facts] of [
  ["send", "send", {}],
  ["create-send", "createThread", { created: true }],
  ["side-close", "closeSideThread", { closed: true }],
]) {
  test(`${kind} 身份和operation正确但原始receiptState未知/失败时拒绝`, async t => {
    const root = await sandbox(t);
    const request = baseRequest({
      actions: [{ ...baseRequest().actions[0], kind }], expectedFacts: {},
    });
    for (const receiptState of ["unknown", "failed"]) {
      const record = await proof(root, { ...facts, operation, receiptState });
      delete record.rawFacts.receiptState;
      await assert.rejects(verifyNativeEvidence([record], request, { baseDir: root }), /回执结果未成功/);
    }
    const completed = await proof(root, { ...facts, operation, receiptState: "completed" });
    assert.equal((await verifyNativeEvidence([completed], request, { baseDir: root })).length, 1);
  });
}

test("原始outcomeUnknown=true不能靠省略指针或success=true掩盖", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { outcomeUnknown: true, success: true, receiptState: "completed" });
  delete record.rawFacts.outcomeUnknown;
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /outcomeUnknown/);
  const failed = await proof(root, { success: false });
  delete failed.rawFacts.success;
  await assert.rejects(verifyNativeEvidence([failed], baseRequest(), { baseDir: root }), /success/);
  const completedButRejected = await proof(root, { receiptState: "completed", nativeResult: { ok: false } });
  delete completedButRejected.rawFacts.nativeResult;
  await assert.rejects(verifyNativeEvidence([completedButRejected], baseRequest(), { baseDir: root }), /ok/);
});

test("动作证据必须显式限定回执范围，只读快照不强制有receiptState", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { closed: true });
  delete record.outcomePointer;
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /outcomePointer/);
  assert.equal((await verifyNativeEvidence([record], baseRequest({
    actions: [], turnIds: [], expectedFacts: { closed: true }, closed: true,
  }), { baseDir: root })).length, 1);
});

test("只扫描指定本次回执，历史中无关unknown不会误拒", async t => {
  const root = await sandbox(t);
  const record = await proof(root, { receiptState: "completed" });
  const path = join(root, record.evidencePath);
  const original = JSON.parse(await readFile(path, "utf8"));
  await writeFile(path, JSON.stringify({
    current: original, history: [{ receiptState: "unknown", outcomeUnknown: true }],
  }));
  const reference = await createEvidenceReference(path, { baseDir: root });
  Object.assign(record, { sha256: reference.sha256, size: reference.size, outcomePointer: "/current" });
  record.rawFacts = Object.fromEntries(Object.entries(record.rawFacts).map(([name, pointer]) => [name, `/current${pointer}`]));
  assert.equal((await verifyNativeEvidence([record], baseRequest(), { baseDir: root })).length, 1);
  record.rawFacts.turnStatus = "/history/0/receiptState";
  await assert.rejects(verifyNativeEvidence([record], baseRequest(), { baseDir: root }), /不在指定回执范围/);
});

test("create无成功回执时必须有真实created事实，side-close必须明确closed=true", async t => {
  const root = await sandbox(t);
  const createRequest = baseRequest({ expectedFacts: {}, actions: [{ ...baseRequest().actions[0], kind: "create-send" }] });
  const create = await proof(root, { operation: "createThread" });
  await assert.rejects(verifyNativeEvidence([create], createRequest, { baseDir: root }), /created/);
  const created = await proof(root, { operation: "createThread", created: true });
  assert.equal((await verifyNativeEvidence([created], createRequest, { baseDir: root })).length, 1);
  const closeRequest = baseRequest({ expectedFacts: {}, closed: true,
    actions: [{ ...baseRequest().actions[0], kind: "side-close" }] });
  const close = await proof(root, { operation: "closeSideThread", receiptState: "completed", closed: false });
  await assert.rejects(verifyNativeEvidence([close], closeRequest, { baseDir: root }), /closed=true/);
  const closed = await proof(root, { operation: "closeSideThread", closed: true });
  assert.equal((await verifyNativeEvidence([closed], closeRequest, { baseDir: root })).length, 1);
});

for (const secondThreadId of ["thread-control", "thread-main"]) {
  test(`同组title与parent事实不能从${secondThreadId === "thread-main" ? "同会话不同时刻" : "两个不同会话"}拼接`, async t => {
    const root = await sandbox(t);
    const first = await proof(root, { title: "wanted", parentThreadId: "wrong-parent" }, {}, "first.json");
    const second = await proof(root, {
      threadId: secondThreadId, receiptId: "receipt-2", title: "other-title", parentThreadId: "wanted-parent",
    }, {}, "second.json");
    const request = baseRequest({
      actions: [], threadIds: [...new Set(["thread-main", secondThreadId])], turnIds: [],
      expectedFacts: { title: "wanted", parentThreadId: "wanted-parent" },
    });
    await assert.rejects(verifyNativeEvidence([first, second], request, { baseDir: root }), /不能跨会话或不同回执时刻/);
  });
}

function queueDequeueRaceSample(kind = "edit", baseTime = Date.now() - 30_000) {
  const at = offset => new Date(baseTime + offset).toISOString();
  const expected = { attemptedKind: kind, origin, tabId: "race-tab", threadId: "race-thread",
    activeTurnId: "natural-base", queueId: "physical-Q", clientUserMessageId: "logical-Q",
    queueActionId: "queue-action", prompt: "原队列正文" };
  const input = [{ type: "text", text: `${expected.prompt}\n`, text_elements: [] }];
  const active = { turnId: expected.activeTurnId, status: "inProgress",
    turnStartedAtMs: baseTime, durationMs: null, items: [] };
  const ended = { ...active, status: "completed", durationMs: 3_000 };
  const accepted = { turnId: "consumed-Q", status: "completed", turnStartedAtMs: baseTime + 3_100,
    durationMs: 300, items: [{ type: "userMessage", id: "accepted-Q", clientId: expected.clientUserMessageId,
      content: structuredClone(input) }] };
  const snapshot = (phase, offset, nativeTurns, serverQueue, nativeQueue) => ({
    threadId: expected.threadId, phase, readId: `${phase}-read`, capturedAt: at(offset),
    queueRace: { historyComplete: true, nativeTurns, serverQueue, nativeQueue },
  });
  const before = snapshot("before", 1_000, [active], [{
    id: expected.queueId, clientUserMessageId: expected.clientUserMessageId, input,
  }], [{ id: expected.queueId, clientUserMessageId: expected.clientUserMessageId,
    createdAt: baseTime - 1_000, text: expected.prompt }]);
  const consumed = snapshot("consumed", 5_000, [ended, accepted], [], []);
  const after = snapshot("after", 7_000, structuredClone(consumed.queueRace.nativeTurns), [], []);
  const body = { threadId: expected.threadId, messageId: expected.queueId, ...(kind === "edit" ? {
    editId: "original-ED", clientUserMessageId: expected.clientUserMessageId,
    originalText: expected.prompt, createdAt: baseTime - 1_000,
  } : {}) };
  const request = { url: `${origin}/api/action/${kind === "edit" ? "beginQueuedEdit" : "removeQueued"}`,
    method: "POST", headers: { "Idempotency-Key": "original-A", Referer: `${origin}/` },
    postData: JSON.stringify(body), isSameSite: true, mixedContentType: "none" };
  // 按已观察的 CDP 差异形态分别构造；不复制整个 Network.Request 冒充 Fetch.Request。
  // 已观察样本是 Response 暂停，仅用于确认字段形态差异，不是本测试的真实 Request 阶段验收。
  const fetchRequest = { url: request.url, method: "POST", postData: JSON.stringify(body),
    headers: { "idempotency-key": "original-A", Accept: "*/*", Cookie: "fixture-only=redacted", Origin: origin } };
  const events = [
    { method: "Network.requestWillBeSent", params: { requestId: "network-Q",
      wallTime: (baseTime + 1_500) / 1000, request } },
    { method: "Fetch.requestPaused", params: { requestId: "fetch-Q", networkId: "network-Q",
      request: fetchRequest } },
  ];
  const held = { threadId: expected.threadId, tabId: expected.tabId, capturedAt: at(2_000),
    requestTrace: { truncated: false, events, commands: [] } };
  const expired = { requestId: "original-A", state: "failed", result: {
    ok: false, code: 409, threadId: expected.threadId, messageId: expected.queueId, message: "此队列消息已执行或不存在",
  } };
  const released = { threadId: expected.threadId, tabId: expected.tabId, capturedAt: at(6_500),
    requestTrace: { truncated: false, events: [...structuredClone(events),
      { method: "Network.responseReceived", params: { requestId: "network-Q", response: { status: 200 } } }],
    commands: [
      { method: "Fetch.continueRequest", params: { requestId: "fetch-Q" }, result: {},
        startedAt: at(6_000), finishedAt: at(6_100) },
      { method: "Network.getResponseBody", params: { requestId: "network-Q" },
        startedAt: at(6_200), finishedAt: at(6_300), result: {
          base64Encoded: false, body: JSON.stringify(expired),
        } },
    ] } };
  const receipt = { actionId: expected.queueActionId, facts: {
    operation: "queue-send", threadId: expected.threadId, queueId: expected.queueId,
    clientUserMessageId: expected.clientUserMessageId, receiptId: "queue-A", receiptState: "completed",
    success: true, deliveryState: "queued", input: structuredClone(input),
  } };
  return { before, held, consumed, released, after, receipt, expected };
}

test("自然出队竞争不需要starting/consuming，只凭原Q、唯一接受及同一旧POST过期闭环", () => {
  for (const kind of ["edit", "remove"]) {
    const sample = queueDequeueRaceSample(kind);
    assert.equal(sample.before.queueRace.nativeQueue[0].submission, undefined);
    const prepared = verifyQueueDequeueRaceConsumption(sample, sample.expected);
    assert.equal(prepared.accepted.item.id, "accepted-Q");
    assert.equal(verifyQueueDequeueRaceFacts(sample, sample.expected).receiptId, "original-A");
  }
});

test("app-server真实队列形态使用server与native组合投影，非原子local差集可以为空", () => {
  for (const kind of ["edit", "remove"]) {
    const sample = queueDequeueRaceSample(kind);
    const raw = {
      serverQueue: { pages: [{ data: sample.before.queueRace.serverQueue, nextCursor: null }],
        data: sample.before.queueRace.serverQueue, nextCursor: null },
      nativeQueue: { ok: true, queuedMessages: sample.before.queueRace.nativeQueue, queuedEdits: [] },
      localQueue: { messages: [], source: "native-combined-minus-server-snapshot", atomicWithServer: false },
    };
    raw.nativeQueue.queuedMessages[0].context = { prompt: sample.expected.prompt, addedFiles: [] };
    sample.before.queueRace.serverQueue = raw.serverQueue.data;
    sample.before.queueRace.nativeQueue = raw.nativeQueue.queuedMessages;
    sample.before.queueRace.localQueue = raw.localQueue; // 仅留原始来源说明，不作为独立物理队列。
    assert.equal(raw.localQueue.messages.length, 0);
    assert.equal(raw.localQueue.atomicWithServer, false);
    assert.equal(verifyQueueDequeueRaceFacts(sample, sample.expected).receiptId, "original-A");
  }
});

test("组合投影缺失、串client/正文、重复或残留不能用非原子差集替代", () => {
  for (const mutate of [
    data => { data.before.queueRace.localQueue = data.before.queueRace.nativeQueue;
      delete data.before.queueRace.nativeQueue; },
    data => { data.before.queueRace.nativeQueue = []; },
    data => { data.before.queueRace.nativeQueue[0].clientUserMessageId = "wrong-client"; },
    data => { data.before.queueRace.nativeQueue[0].text += "改写"; },
    data => { data.before.queueRace.nativeQueue[0].context = { prompt: "改写" }; },
    data => { data.before.queueRace.nativeQueue.push(structuredClone(data.before.queueRace.nativeQueue[0])); },
    data => { data.before.queueRace.nativeQueue[0].createdAt -= 1; },
    data => { data.after.queueRace.nativeQueue.push({ id: data.expected.queueId }); },
  ]) {
    const sample = queueDequeueRaceSample();
    mutate(sample);
    assert.throws(() => verifyQueueDequeueRaceFacts(sample, sample.expected), /自然出队竞争/);
  }
});

test("显式旧POST序列窗口排除此前入队send，但不能排除窗口内的重发或引导", () => {
  const sample = queueDequeueRaceSample("remove");
  const prefix = { sequence: 90, method: "Network.requestWillBeSent", params: {
    requestId: "original-queue-send", request: { method: "POST", url: `${origin}/api/action/send` },
  } };
  for (const phase of ["held", "released"]) {
    const trace = sample[phase].requestTrace;
    // 真实 CDP 可以先派发 Fetch 暂停，再派发同 networkId 的 Network 请求事件。
    const [network, paused, ...rest] = trace.events;
    const raw = [prefix, { ...paused, sequence: 100 }, { ...network, sequence: 101 },
      ...rest.map((event, index) => ({ ...event, sequence: 102 + index }))];
    trace.events = raw.filter(event => event.sequence >= 100);
  }
  assert.equal(verifyQueueDequeueRaceFacts(sample, sample.expected).receiptId, "original-A");
  for (const path of ["send", "steerQueued"]) {
    const invalid = structuredClone(sample);
    invalid.released.requestTrace.events.push({ ...prefix, sequence: 102, params: {
      requestId: "unexpected-write", request: { method: "POST", url: `${origin}/api/action/${path}` },
    } });
    assert.throws(() => verifyQueueDequeueRaceFacts(invalid, invalid.expected), /暂缓窗口夹带/);
  }
});

test("同networkId的Network与Fetch允许真实CDP字段差异，原请求业务身份不变才通过", () => {
  for (const kind of ["edit", "remove"]) {
    const sample = queueDequeueRaceSample(kind);
    const network = sample.held.requestTrace.events[0].params.request;
    const paused = sample.held.requestTrace.events[1].params.request;
    assert.notDeepEqual(network, paused);
    assert.equal(network.isSameSite, true);
    assert.equal(paused.isSameSite, undefined);
    assert.equal(network.headers.Referer, `${origin}/`);
    assert.equal(paused.headers.Referer, undefined);
    assert.equal(network.headers.Accept, undefined);
    assert.equal(paused.headers.Accept, "*/*");
    const laterNetwork = sample.released.requestTrace.events[0].params.request;
    const laterPaused = sample.released.requestTrace.events[1].params.request;
    delete laterNetwork.isSameSite; delete laterNetwork.headers.Referer;
    laterNetwork.initialPriority = "High";
    laterPaused.headers.Cookie = "fixture-only=changed-observation";
    laterPaused.headers["IDEMPOTENCY-KEY"] = laterPaused.headers["idempotency-key"];
    delete laterPaused.headers["idempotency-key"];
    sample.released.requestTrace.commands[0].params.interceptResponse = false;
    assert.equal(verifyQueueDequeueRaceFacts(sample, sample.expected).receiptId, "original-A");
    const responsePause = queueDequeueRaceSample(kind);
    responsePause.held.requestTrace.events[1].params.responseStatusCode = 200;
    assert.throws(() => verifyQueueDequeueRaceFacts(responsePause, responsePause.expected), /请求阶段暂缓/);
  }
});

test("Network与Fetch的URL、POST、原正文或幂等键变化不能被相同networkId掩盖", () => {
  const changes = [
    request => { request.url += "?changed=1"; },
    request => { request.url = request.url.replace(origin, "https://other.example.test"); },
    request => { request.method = "PATCH"; },
    request => { request.postData += " "; },
    request => {
      const key = Object.keys(request.headers).find(name => name.toLowerCase() === "idempotency-key");
      request.headers[key] = "different-A";
    },
  ];
  for (const kind of ["edit", "remove"]) {
    for (const phase of ["held", "released"]) {
      for (const change of changes) {
        const sample = queueDequeueRaceSample(kind);
        change(sample[phase].requestTrace.events[1].params.request);
        assert.throws(() => verifyQueueDequeueRaceFacts(sample, sample.expected), /自然出队竞争/);
      }
    }
    for (const change of changes) {
      const sample = queueDequeueRaceSample(kind);
      for (const event of sample.released.requestTrace.events.slice(0, 2)) change(event.params.request);
      assert.throws(() => verifyQueueDequeueRaceFacts(sample, sample.expected), /自然出队竞争/);
    }
    const ambiguous = queueDequeueRaceSample(kind);
    ambiguous.held.requestTrace.events[1].params.request.headers["Idempotency-Key"] = "conflicting-A";
    assert.throws(() => verifyQueueDequeueRaceFacts(ambiguous, ambiguous.expected), /自然出队竞争/);
  }
});

test("自然出队竞争拒绝错误身份、输入变化、重复接受和不成立的自然顺序", () => {
  for (const mutate of [
    data => { data.before.queueRace.serverQueue[0].clientUserMessageId = "other-client"; },
    data => { data.before.queueRace.serverQueue[0].input[0].text += "被改写"; },
    data => { data.consumed.queueRace.nativeTurns[1].items.push(
      structuredClone(data.consumed.queueRace.nativeTurns[1].items[0])); },
    data => { data.consumed.queueRace.nativeTurns[0].status = "interrupted"; },
    data => { data.consumed.queueRace.nativeTurns[1].turnStartedAtMs -= 1_000; },
    data => { data.consumed.queueRace.nativeQueue.push({ id: "physical-Q" }); },
    data => { data.held.tabId = "other-tab"; },
    data => { data.held.requestTrace.events[1].params.responseStatusCode = 200; },
    data => { data.held.requestTrace.events[1].params.responseErrorReason = "Failed"; },
    data => { data.held.requestTrace.commands.push({
      method: "Fetch.continueRequest", params: { requestId: "fetch-Q" }, result: {},
    }); },
    data => { data.held.capturedAt = data.consumed.capturedAt; },
    data => { data.released.requestTrace.commands[0].startedAt = data.held.capturedAt; },
    data => { data.released.requestTrace.commands[1].startedAt = data.held.capturedAt; },
    data => { data.released.requestTrace.commands[0].params.postData = "{}"; },
    data => { data.released.requestTrace.events.push(structuredClone(data.released.requestTrace.events[0])); },
    data => { data.released.requestTrace.events[0].params.request.headers["Idempotency-Key"] = "new-A"; },
    data => { data.after.queueRace.nativeTurns[1].items[0].content[0].text = "迟到旧编辑覆盖"; },
    data => { data.after.readId = data.consumed.readId; },
  ]) {
    const sample = queueDequeueRaceSample();
    mutate(sample);
    assert.throws(() => verifyQueueDequeueRaceFacts(sample, sample.expected), /自然出队竞争/);
  }
});

test("迟到删除的成功清理、unknown、错误A及非409不能冒充阻止出队", () => {
  for (const mutate of [
    value => { value.state = "completed"; value.result.ok = true; },
    value => { value.state = "unknown"; value.result.outcomeUnknown = true; },
    value => { value.requestId = "new-A"; },
    value => { value.result.code = 503; },
    value => { value.result.threadId = "other-thread"; },
  ]) {
    const sample = queueDequeueRaceSample("remove");
    const body = sample.released.requestTrace.commands[1].result;
    const receipt = JSON.parse(body.body);
    mutate(receipt); body.body = JSON.stringify(receipt);
    assert.throws(() => verifyQueueDequeueRaceFacts(sample, sample.expected), /原 A 没有明确过期拒绝/);
  }
});

async function queueDequeueRaceReportFixture(t, kind) {
  const { root, report, options, ui } = await reportFixture(t);
  const caseId = `queue.dequeue_${kind}_race`, baseTime = Date.parse(report.runStartedAt) + 5_000;
  const sample = queueDequeueRaceSample(kind, baseTime), { expected } = sample;
  const queueActionId = `${runId}:${caseId}:queue-send`, raceActionId = `${runId}:${caseId}:race-${kind}`;
  expected.queueActionId = queueActionId; sample.receipt.actionId = queueActionId;
  const at = offset => new Date(baseTime + offset).toISOString();
  const queueAction = { id: queueActionId, caseId, kind: "queue-send", status: "confirmed",
    threadId: expected.threadId, prompt: expected.prompt, startedAt: at(100), finishedAt: at(500),
    result: { queueId: expected.queueId } };
  const raceAction = { id: raceActionId, caseId, kind: `queue-race-${kind}`, status: "confirmed",
    threadId: expected.threadId, queueId: expected.queueId, startedAt: at(1_200), finishedAt: at(7_500),
    result: { receiptId: "original-A" } };
  const driverStages = [];
  for (const [phase, driver, operation] of [
    ["before", "native", "observe-dequeue-window"],
    ["held", "environment", "observe-queue-race-request"],
    ["consumed", "native", "observe-queue-race"],
    ["released", "environment", "release-queue-race-request"],
    ["after", "native", "observe-queue-race"],
  ]) {
    const raw = { ...sample[phase], phase, operation, runId, caseId,
      origin, sourceFingerprint: report.sourceFingerprint, readId: `${phase}-read`,
      actionId: undefined, receiptId: undefined, turnId: undefined };
    const record = await proof(root, raw, {
      actionId: undefined, receiptId: undefined, turnId: undefined,
      threadId: expected.threadId, readId: raw.readId,
    }, `race-${kind}-${phase}.json`);
    driverStages.push({ step: { kind: "driver", driver, operation, records: [record], request: {
      runId, caseId, operation, origin, sourceFingerprint: report.sourceFingerprint,
      threadId: expected.threadId, threadIds: [expected.threadId], actions: [],
      payload: { phase }, expectations: [{ fact: "phase", op: "equals", value: phase }],
    } }, records: [record] });
  }
  const queueReceipt = await proof(root, { ...sample.receipt.facts,
    actionId: queueActionId, caseId, sourceFingerprint: report.sourceFingerprint,
    turnId: undefined, capturedAt: at(600),
  }, { actionId: queueActionId, turnId: undefined, threadId: expected.threadId,
    receiptId: sample.receipt.facts.receiptId }, `race-${kind}-queue-receipt.json`);
  const rejection = await proof(root, {
    actionId: raceActionId, caseId, sourceFingerprint: report.sourceFingerprint,
    threadId: expected.threadId, turnId: undefined, operation: kind === "edit" ? "beginQueuedEdit" : "removeQueued",
    receiptId: "original-A", receiptState: "failed", code: 409, success: false, capturedAt: at(6_500),
  }, { actionId: raceActionId, turnId: undefined, threadId: expected.threadId, receiptId: "original-A" },
  `race-${kind}-rejection.json`);
  const stage = { kind: "queue-dequeue-race", ...expected, caseId, raceActionId };
  const nativeRequest = { runId, caseId, origin, sourceFingerprint: report.sourceFingerprint,
    runStartedAt: report.runStartedAt, actions: [queueAction, raceAction], threadIds: [expected.threadId] };
  const nativeRecords = await verifyNativeEvidence([queueReceipt, rejection], nativeRequest, { baseDir: root });
  report.actions = { [queueActionId]: queueAction, [raceActionId]: raceAction };
  report.threads = { main: { id: expected.threadId, runId } };
  report.cases = [{ ...report.cases[0], id: caseId, nativeExpectations: { threadIds: [expected.threadId] },
    evidence: [{ ...ui, threadId: expected.threadId, observation: { threadId: expected.threadId } },
      ...driverStages.map(value => value.step), { kind: "native", records: [queueReceipt, rejection], request: nativeRequest }, stage] }];
  options.requiredCases = [{ id: caseId, required: true }];
  options.caseContracts = { [caseId]: REMOTE_ACCESS_E2E_CONTRACTS[caseId] };
  return { root, report, options, stage, nativeRecords, driverStages };
}

test("自然出队竞争报告从原件重验原A与前后读取，不能删除专项或替换请求阶段", async t => {
  for (const kind of ["edit", "remove"]) {
    const fixture = await queueDequeueRaceReportFixture(t, kind), { report, options, stage } = fixture;
    const request = { report, driverStages: fixture.driverStages, nativeRecords: fixture.nativeRecords,
      caseId: stage.caseId, threadId: stage.threadId };
    assert.equal((await verifyQueueDequeueRaceStage(stage, request, options)).receiptId, "original-A");
    const verified = await verifyRemoteAccessReport(report, options);
    assert.equal(verified.passed, true, verified.problems.join("\n"));
    const stripped = structuredClone(report);
    stripped.cases[0].evidence = stripped.cases[0].evidence.filter(value => value.kind !== "queue-dequeue-race");
    const weak = { ...options, caseContracts: { [stage.caseId]: { requiresQueueDequeueRace: false } } };
    const missing = await verifyRemoteAccessReport(stripped, weak);
    assert.equal(missing.passed, false);
    assert.match(missing.problems.join("\n"), /固定用例缺少唯一专项/);
    const held = fixture.driverStages.find(value => value.step.request.payload.phase === "held");
    const file = resolve(fixture.root, held.records[0].evidencePath);
    const raw = JSON.parse(await readFile(file, "utf8"));
    raw.requestTrace.events[1].params.responseStatusCode = 200;
    await writeFile(file, JSON.stringify(raw));
    Object.assign(held.records[0], await createEvidenceReference(file, { baseDir: fixture.root }));
    await assert.rejects(verifyQueueDequeueRaceStage(stage, request, options), /请求阶段暂缓/);
  }
});

test("队列race原生failed/409可留证，unknown与普通failed仍拒绝", async t => {
  const root = await sandbox(t);
  const request = baseRequest({
    actions: [{ ...baseRequest().actions[0], kind: "queue-race-edit" }],
    expectedFacts: { staleWriteAccepted: false, consumedCount: 1 },
  });
  const record = await proof(root, {
    operation: "editQueuedMessage", receiptState: "rejected", success: false,
    staleWriteAccepted: false, consumedCount: 1,
  });
  assert.equal((await verifyNativeEvidence([record], request, { baseDir: root })).length, 1);
  const unknown = await proof(root, {
    operation: "editQueuedMessage", receiptState: "unknown", staleWriteAccepted: false, consumedCount: 1,
  });
  await assert.rejects(verifyNativeEvidence([unknown], request, { baseDir: root }), /回执结果未成功/);
  const expired = await proof(root, {
    operation: "beginQueuedEdit", receiptState: "failed", success: false, code: 409,
    staleWriteAccepted: false, consumedCount: 1,
  });
  assert.equal((await verifyNativeEvidence([expired], request, { baseDir: root })).length, 1);
  const failed = await proof(root, {
    operation: "beginQueuedEdit", receiptState: "failed", success: false, code: 503,
    staleWriteAccepted: false, consumedCount: 1,
  });
  await assert.rejects(verifyNativeEvidence([failed], request, { baseDir: root }), /回执结果未成功/);
});

test("报告重验外部接入的原始创建回执、文件哈希和DOM消息标记", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const marker = `[remote-e2e:${runId}:main]`;
  const actionId = `${runId}:adopt:main`;
  const creation = {
    id: actionId, caseId: "external.adopt", kind: "external-created-this-run",
    threadId: "thread-main", status: "confirmed", result: { threadId: "thread-main", receiptId: "creation-receipt" },
  };
  const record = await proof(root, {
    caseId: "external.adopt", actionId, operation: "createThread", prompt: marker,
    sourceFingerprint: report.sourceFingerprint, receiptId: "creation-receipt", receiptState: "created",
  }, { actionId }, "creation.json");
  const dom = await put(root, "ui.dom.txt", `- heading "回归会话"\n- text: ${marker}\n`);
  ui.files.dom = await createEvidenceReference(dom, { baseDir: root });
  report.actions[actionId] = creation;
  report.adoptions = [{
    key: "main", actionId, threadId: "thread-main", receiptId: "creation-receipt",
    marker, record, uiEvidence: ui,
  }];
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  await writeFile(join(root, record.evidencePath), "{\"verified\":true}");
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /external.adopt.*SHA-256/);
  report.adoptions = [];
  assert.match((await verifyRemoteAccessReport(report, options)).problems.join("\n"), /缺少唯一的原始创建证据/);
});

test("报告按源码固定次数核验驱动步骤，删除中间步骤或重复引用同一回执均拒绝", async t => {
  const { root, report, options, ui } = await reportFixture(t);
  const records = [
    await proof(root, {
      sourceFingerprint: report.sourceFingerprint, operation: "observe-thread", title: "真实标题",
    }, { actionId: undefined }, "observe-1.json"),
    await proof(root, {
      sourceFingerprint: report.sourceFingerprint, operation: "observe-thread", title: "真实标题", receiptId: "read-2",
    }, { actionId: undefined }, "observe-2.json"),
  ];
  const steps = records.map(record => ({
    kind: "driver", driver: "desktop", operation: "observe-thread", records: [record],
    uiEvidence: [{ dom: ui.files.dom, screenshot: ui.files.screenshot }],
    request: {
      runId, caseId: "session.send_stream", contractVersion: "remote-e2e-driver/2",
      operation: "observe-thread", actions: [], threadIds: ["thread-main"],
      expectedFacts: { title: "真实标题" },
    },
  }));
  report.actions = {};
  report.cases[0].nativeExpectations = undefined;
  report.cases[0].evidence = [ui, ...steps];
  options.caseContracts["session.send_stream"].requiredDriverSteps = [
    { driver: "desktop", operation: "observe-thread", count: 2 },
  ];
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  report.cases[0].evidence = [ui, steps[0]];
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
  report.cases[0].evidence.push(structuredClone(steps[0]));
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /缺少独立的驱动步骤/);
  report.cases[0].evidence = [
    ui, { ...steps[0], records }, { ...steps[1], records: [...records].reverse() },
  ];
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, false);
});

test("真实native操作别名兼容editMessage/pinProject/setGoal，错误operation仍拒绝", async t => {
  const root = await sandbox(t);
  for (const [kind, operation] of [
    ["edit-message", "editMessage"], ["project-pin", "pinProject"], ["create-goal", "setGoal"],
    ["approve", "approve"], ["answer", "answer"], ["compact", "compact"],
    ["optimizePrompt", "optimizePrompt"], ["cancelPromptOptimize", "cancelPromptOptimize"],
    ["queue-send", "send"], ["queue-edit-cancel", "cancelQueuedEdit"],
    ["queue-edit-save", "editQueued"], ["queue-race-edit", "beginQueuedEdit"],
    ["queue-race-remove", "removeQueued"], ["queue-reorder", "reorderQueued"],
    ["queue-remove", "removeQueued"], ["queue-steer", "steerQueued"],
  ]) {
    const request = baseRequest({ actions: [{ ...baseRequest().actions[0], kind,
      ...(kind.startsWith("queue-") ? { queueId: "queue-1" } : {}) }] });
    const record = await proof(root, { operation, receiptState: "completed", queueId: "queue-1", removedQueueId: "queue-1" });
    assert.equal((await verifyNativeEvidence([record], request, { baseDir: root })).length, 1, kind);
    const wrong = await proof(root, { operation: "unrelatedAction", receiptState: "completed" });
    await assert.rejects(verifyNativeEvidence([wrong], request, { baseDir: root }), /operation/, kind);
  }
});

test("恢复原文必须核对前后UI文件、同会话、文本、时间顺序及原始优化输入输出", async t => {
  const { report, options } = await restoreReportFixture(t);
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  for (const change of [
    value => { value.cases[0].evidence[3].afterUi.observation.threadId = "other-thread"; },
    value => { value.cases[0].evidence[3].beforeUi.observation.prompt = "other-text"; },
    value => { value.cases[0].evidence[3].afterUi.observation.prompt = "other-text"; },
    value => { value.cases[0].evidence[3].afterUi.at = value.runStartedAt; },
    value => { value.cases[0].evidence[3].originalInput = "伪造原输入"; },
    value => { value.cases[0].evidence[3].optimizedPrompt = "伪造优化输出"; },
    value => { value.cases[0].evidence[3].beforeUi.files.dom.sha256 = "b".repeat(64); },
    value => { value.cases[0].evidence[3].afterUi.files.screenshot.sha256 = "b".repeat(64); },
  ]) {
    const changed = structuredClone(report);
    change(changed);
    assert.equal((await verifyRemoteAccessReport(changed, options)).passed, false);
  }
});

test("删除恢复步骤或原始优化输出不能靠requiredUiRestores或passed标签通过", async t => {
  const { report, options } = await restoreReportFixture(t);
  const withoutRestore = structuredClone(report);
  withoutRestore.cases[0].evidence.pop();
  assert.match((await verifyRemoteAccessReport(withoutRestore, options)).problems.join("\n"), /缺少恢复原文/);
  const withoutOriginalOutput = structuredClone(report);
  delete withoutOriginalOutput.cases[0].evidence[2].records[0].rawFacts.optimizedPrompt;
  assert.match((await verifyRemoteAccessReport(withoutOriginalOutput, options)).problems.join("\n"), /原始优化回执/);
  options.caseContracts["optimizer.restore"].requiredUiRestores = 2;
  report.cases[0].evidence.push(structuredClone(report.cases[0].evidence[3]));
  assert.match((await verifyRemoteAccessReport(report, options)).problems.join("\n"), /实际 1 次/);
});

test("文件选择driver与桌面driver一样必须有DOM和PNG，不能离线删掉界面证据", async t => {
  const { report, options, ui, native } = await reportFixture(t);
  report.actions = {};
  report.cases[0].nativeExpectations = undefined;
  report.cases[0].evidence = [ui, {
    kind: "driver", driver: "files", operation: "send", records: [native], uiEvidence: [],
    request: {
      runId, caseId: "session.send_stream", operation: "send", actions: [],
      expectedFacts: { turnStatus: "completed" }, threadIds: ["thread-main"],
    },
  }];
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /文件选择驱动缺少/);
});

async function requiredCaseReportFixture(t) {
  const outputDir = await sandbox(t);
  const tab = {
    id: "unit-required-cases-tab",
    url: async () => origin,
    screenshot: async () => png,
    reload: async () => { throw new Error("必测定义检查不得刷新真实页面"); },
    playwright: {
      domSnapshot: async () => '- heading "本机报告控制流替身"\n',
      evaluate: async () => ({
        title: "新建会话", sidebar: [], turns: [], prompt: "", connection: "已连接",
        send: { label: "发送消息", enabled: true }, canCreate: true, promptEditable: true,
      }),
    },
  };
  const options = { tab, runId: "unit_required_cases", targetOrigin: origin, outputDir };
  const context = await createRemoteAccessE2EContext(options);
  // 仅构造报告状态，验证恢复与汇总；不代表这些业务案例真实执行通过。
  for (const entry of context.report.cases) Object.assign(entry, {
    status: "passed", stepsCompleted: true, inProgress: false,
    attempts: [{ status: "passed", finishedAt: new Date().toISOString() }],
    evidence: [{ kind: "unit-only", caseId: entry.id }],
  });
  context.report.actions.original = {
    id: "original-action", caseId: "session.send_stream", status: "confirmed",
    kind: "send", result: { threadId: "unit-main", turnId: "unit-turn" },
  };
  return { context, options, report: structuredClone(context.report) };
}

test("runner必测定义：恢复报告不能用缺失或false required隐藏not_run和blocked", async t => {
  const { options, report } = await requiredCaseReportFixture(t);
  const definition = REMOTE_ACCESS_E2E_CASES.find(value => value.id === "scenario.approval");
  for (const status of ["not_run", "blocked"]) {
    for (const required of [undefined, false]) {
      await t.test(`${status}/${required === undefined ? "missing" : "false"}`, async () => {
        const previous = structuredClone(report);
        const entry = previous.cases.find(value => value.id === definition.id);
        Object.assign(entry, {
          status, stepsCompleted: false, suite: "report-defined-suite", title: "报告自填标题",
          reason: "原步骤尚未完成",
        });
        if (required === undefined) delete entry.required;
        else entry.required = required;
        entry.attempts.push({ status, finishedAt: new Date().toISOString() });
        const expectedEntry = { ...structuredClone(entry), ...definition };
        const expectedActions = structuredClone(previous.actions);
        const resumed = await createRemoteAccessE2EContext({ ...options, resumeReport: previous });
        assert.equal(resumed.report.overall, status, "未完成必测案例不能被排除后汇总为passed");
        assert.deepEqual(resumed.report.cases.find(value => value.id === definition.id), expectedEntry);
        assert.deepEqual(resumed.report.actions, expectedActions, "恢复定义不能修改原动作");
        const saved = JSON.parse(await readFile(join(options.outputDir, "report.json"), "utf8"));
        assert.equal(saved.overall, status);
        assert.deepEqual(saved.cases.find(value => value.id === definition.id), expectedEntry);
      });
    }
  }
});

test("runner必测定义：恢复报告缺失案例补not_run，正常passed保持原执行证据", async t => {
  const { options, report } = await requiredCaseReportFixture(t);
  const definition = REMOTE_ACCESS_E2E_CASES.find(value => value.id === "scenario.approval");
  await t.test("缺失案例", async () => {
    const previous = structuredClone(report);
    previous.cases = previous.cases.filter(value => value.id !== definition.id);
    const resumed = await createRemoteAccessE2EContext({ ...options, resumeReport: previous });
    assert.equal(resumed.report.overall, "not_run");
    assert.deepEqual(resumed.report.cases.find(value => value.id === definition.id), {
      ...definition, status: "not_run", elapsedMs: 0, attempts: [], evidence: [],
    });
    assert.deepEqual(resumed.report.actions, report.actions);
  });
  await t.test("正常passed", async () => {
    const resumed = await createRemoteAccessE2EContext({ ...options, resumeReport: structuredClone(report) });
    assert.equal(resumed.report.overall, "passed");
    assert.deepEqual(resumed.report.cases, report.cases);
    assert.deepEqual(resumed.report.actions, report.actions);
  });
});

test("runner必测定义：实时汇总按源码ID核对，不受报告required或缺项影响", async t => {
  const { context, report } = await requiredCaseReportFixture(t);
  context.options.caseIds = []; // 仅重新汇总，不执行任何案例处理器。
  for (const status of ["not_run", "blocked", "missing"]) {
    await t.test(status, async () => {
      context.report.cases = structuredClone(report.cases);
      const targetId = "scenario.approval";
      if (status === "missing") context.report.cases = context.report.cases.filter(value => value.id !== targetId);
      else Object.assign(context.report.cases.find(value => value.id === targetId), { status, required: false });
      const result = await runRemoteAccessE2E(context);
      assert.equal(result.overall, status === "missing" ? "not_run" : status);
    });
  }
});

test("真实runner缺少外部driver时blocked且零提交，仅用本机tab替身检查控制流", async t => {
  const outputDir = await sandbox(t);
  const { createRemoteAccessE2EContext, runRemoteAccessCase } = await import("./remote-access-e2e.mjs");
  let reloads = 0;
  const tab = {
    id: "unit-only-tab",
    url: async () => origin,
    screenshot: async () => png,
    reload: async () => { reloads++; throw new Error("缺少驱动时不应进入页面重载"); },
    playwright: {
      domSnapshot: async () => "- heading \"本机控制流替身\"\n",
      evaluate: async () => ({
        title: "新建会话", sidebar: [], turns: [], prompt: "", connection: "已连接",
        send: { label: "发送消息", enabled: true }, canCreate: true, promptEditable: true,
      }),
    },
  };
  const context = await createRemoteAccessE2EContext({
    tab, runId: "unit_missing_drivers", targetOrigin: origin, outputDir,
    authorization: { allowFaults: true, allowDrafts: true, allowQueueChanges: true },
    environmentTarget: {
      kind: "dedicated-test-service", instanceId: "unit_missing_drivers-service",
      origin, servicePort: 45555, processId: 987654321,
      workspace: join(outputDir, "unit_missing_drivers-workspace"),
    },
  });
  for (const caseId of [
    "scenario.approval",
    "optimizer.no_injection", "android.background_resume", "queue.edit_draft",
    "queue.dequeue_edit_race", "queue.dequeue_remove_race",
  ]) {
    await runRemoteAccessCase(caseId, context);
    const entry = context.report.cases.find(value => value.id === caseId);
    assert.equal(entry.status, "blocked", caseId);
    assert.match(entry.reason, /缺少真实 .* 驱动/, caseId);
  }
  assert.equal(context.report.messageSubmissions, 0);
  assert.equal(Object.keys(context.report.actions).length, 0);
  assert.equal(reloads, 0);
  context.options.drivers = Object.fromEntries(["native", "environment"].map(name => [name, {
    contractVersion: "remote-e2e-driver/2",
    operations: name === "native" ? ["observe-dequeue-window", "observe-queue-race"] : [],
    perform: async () => { throw new Error("缺能力时不能启动故障"); },
    observe: async () => { throw new Error("缺能力时不能造证据"); },
  }]));
  context.options.retryCases = ["queue.dequeue_edit_race", "queue.dequeue_remove_race"];
  for (const caseId of ["queue.dequeue_edit_race", "queue.dequeue_remove_race"]) {
    await runRemoteAccessCase(caseId, context);
    const entry = context.report.cases.find(value => value.id === caseId);
    assert.equal(entry.status, "blocked");
    assert.match(entry.reason, /environment 驱动缺少明确能力：arm-queue-race-request.*release-queue-race-request/);
  }
  assert.equal(context.report.messageSubmissions, 0);
  assert.equal(Object.keys(context.report.actions).length, 0);
});

test("排队草稿runner在fill未改变实际contenteditable时停止，不能发送伪造编辑值", async t => {
  const outputDir = await sandbox(t), mainId = "unit-main", controlId = "unit-control";
  const markers = { main: `[remote-e2e:${runId}:main]`, control: `[remote-e2e:${runId}:control]` };
  const queue = ["A", "B", "C"].map(key => ({
    id: `queue-${key}`, clientUserMessageId: `client-${key}`,
    marker: `[remote-e2e:${runId}:queue-${key}]`,
    text: `原正文${key}\n[remote-e2e:${runId}:queue-${key}]`,
  }));
  const drafts = { [mainId]: "", [controlId]: "" };
  let selected = mainId, editing = false, sendClicks = 0, editFills = 0;
  const locator = (selector, name) => ({
    filter() { return this; }, first() { return this; },
    locator(child) { return locator(selector, child); },
    getByRole(role, input) { return locator(selector, input.name); },
    count: async () => 1, isEnabled: async () => true,
    isVisible: async () => name !== "打开侧边栏",
    click: async () => {
      const thread = selector?.match(/data-thread-id="([^"]+)"/)?.[1];
      if (thread) selected = thread;
      if (name === "编辑消息") { editing = true; drafts[mainId] = queue[0].text; }
      if (name === "发送消息") sendClicks++;
    },
    fill: async value => {
      if (editing) editFills++; // 模拟fill调用返回，但真实contenteditable保持原文。
      else drafts[selected] = value;
    },
  });
  const tab = { id: "unit-queue-draft-tab", url: async () => `${origin}/#thread=${selected}`,
    reload: async () => { throw new Error("此控制流反例不得刷新或创建真实对象"); },
    screenshot: async () => png,
    playwright: {
      locator: selector => locator(selector),
      getByRole: (role, input) => locator(null, input.name),
      domSnapshot: async () => '- textbox "消息输入"\n',
      evaluate: async () => ({
        title: selected, loading: false, promptEditable: true, editing, prompt: drafts[selected], pendingText: "",
        send: { action: editing || drafts[selected] ? "send" : selected === mainId ? "stop" : "send", enabled: true },
        sidebar: [{ id: mainId, visible: true }, { id: controlId, visible: true }],
        turns: [{ id: "created", users: [{ text: selected === mainId ? markers.main : markers.control }], agents: [] },
          ...(selected === mainId ? [{ id: "active", users: [], agents: [] }] : [])],
        queue: selected === mainId ? queue : [], menuItems: [],
      }),
    } };
  const native = { contractVersion: "remote-e2e-driver/2", operations: ["observe-queue-edit-cancel"],
    perform: async () => { throw new Error("只读原生驱动不得执行写操作"); },
    observe: async request => {
      const { phase } = request.payload;
      assert.ok(["before", "editing"].includes(phase), "未实际改稿时不应进入重新发送阶段");
      const input = [{ type: "text", text: `${queue[0].text}\n` }];
      const raw = { runId, caseId: request.caseId, origin, sourceFingerprint: request.sourceFingerprint,
        operation: request.operation, phase, threadId: mainId, capturedAt: new Date().toISOString(),
        readId: `unit-${phase}`, ...(phase === "before" ? { queuedMessages: queue.map(value => ({
          id: value.id, clientUserMessageId: value.clientUserMessageId,
          input: [{ type: "text", text: `${value.text}\n` }],
        })) } : { editId: "unit-edit", editState: "editing", originalMessageId: queue[0].id,
          originalClientUserMessageId: queue[0].clientUserMessageId, originalInput: input, originalIndex: 0 }) };
      const reference = await createEvidenceReference(
        await put(outputDir, `unit-${phase}.json`, JSON.stringify(raw)), { baseDir: outputDir });
      return { records: [{ source: "rpc", ...reference, threadId: mainId, readId: raw.readId,
        rawFacts: Object.fromEntries(Object.keys(raw).map(key => [key, `/${key}`])) }] };
    } };
  const ctx = await createRemoteAccessE2EContext({ tab, runId, targetOrigin: origin, outputDir,
    timeoutMs: 40, pollMs: 1, authorization: { allowQueueChanges: true, allowDrafts: true }, drivers: { native } });
  for (const [key, id] of [["main", mainId], ["control", controlId]]) {
    ctx.report.actions[`unit-create-${key}`] = { status: "confirmed" };
    ctx.report.threads[key] = { id, key, marker: markers[key], kind: "main",
      createdBy: "remote-access-e2e", runId, creationActionId: `unit-create-${key}` };
  }
  ctx.report.windows.editing = { turnId: "active" };
  for (const [index, key] of ["A", "B", "C"].entries()) {
    const row = queue[index], actionId = `unit-queue-${key}`;
    ctx.report.queue[key] = { ...row, queueId: row.id, threadId: mainId, actionId };
    ctx.report.actions[actionId] = { id: actionId, caseId: "queue.edit_draft", status: "confirmed",
      kind: "queue-send", threadId: mainId, prompt: row.text, result: { queueId: row.id } };
  }
  await runRemoteAccessCase("queue.edit_draft", ctx);
  const entry = ctx.report.cases.find(value => value.id === "queue.edit_draft");
  assert.equal(entry.status, "failed");
  assert.match(entry.reason, /真实编辑器已改成与原文不同/);
  assert.equal(editFills, 1);
  assert.equal(sendClicks, 0);
  assert.equal(ctx.report.messageSubmissions, 0);
  assert.equal(entry.evidence.some(value => value.kind === "queue-edit-draft"), false);
});

test("离线租约直接核对acquire原始状态与最终inspect，false原状态不能被布尔或自填期待替代", async t => {
  const { root, report, options, inspection } = await leaseReportFixture(t);
  assert.equal((await verifyRemoteAccessReport(report, options)).passed, true);
  const originalRecord = inspection.records[0];
  const path = join(root, originalRecord.evidencePath);
  const originalBytes = await readFile(path);
  const raw = JSON.parse(originalBytes.toString("utf8"));
  for (const changes of [
    { currentState: { cdp: true, service: true } },
    { originalState: { cdp: true, service: true }, currentState: { cdp: true, service: true } },
    { instanceId: "different-instance" },
    { leaseOwnerRunId: "different-run" },
    { leaseKey: `${runId}:different-case:environment` },
    { capturedAt: new Date(Date.parse(raw.capturedAt) - 10_000).toISOString() },
  ]) {
    const changed = structuredClone(report);
    const step = changed.cases[0].evidence.at(-1);
    await writeFile(path, JSON.stringify({ ...raw, ...changes, stateMatchesOriginal: true }));
    const reference = await createEvidenceReference(path, { baseDir: root });
    Object.assign(step.records[0], { sha256: reference.sha256, size: reference.size });
    // 即使报告自填的阶段期待跟着错误值变化，原始 acquire 仍是验收基线。
    for (const name of ["instanceId", "leaseOwnerRunId", "leaseKey"])
      if (changes[name]) step.request.expectedFacts[name] = changes[name];
    if (changes.capturedAt) step.request.capturedAfter = new Date(Date.parse(changes.capturedAt) - 1).toISOString();
    const result = await verifyRemoteAccessReport(changed, options);
    assert.equal(result.passed, false, JSON.stringify(changes));
    assert.match(result.problems.join("\n"), /环境租约/, JSON.stringify(changes));
    await writeFile(path, originalBytes);
  }
});

test("租约验收必须看最后一次原始inspect，不能择取更早的成功快照", async t => {
  const { root, report, options, inspection } = await leaseReportFixture(t);
  const original = JSON.parse(await readFile(join(root, inspection.records[0].evidencePath), "utf8"));
  const capturedAt = new Date(Date.now() - 100).toISOString();
  const record = await proof(root, {
    ...original, capturedAt, receiptId: "latest-inspect-receipt",
    currentState: { cdp: true, service: true }, originalState: { cdp: true, service: true },
    stateMatchesOriginal: true,
  }, { actionId: undefined }, "latest-inspect.json");
  report.cases[0].evidence.push({
    ...structuredClone(inspection), records: [record],
    request: { ...structuredClone(inspection.request), capturedAfter: new Date(Date.parse(capturedAt) - 1).toISOString() },
  });
  const result = await verifyRemoteAccessReport(report, options);
  assert.equal(result.passed, false);
  assert.match(result.problems.join("\n"), /最终 inspect.*acquire 不一致/);
});
