import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { createSidebarPreferenceStore } from "../assets/remote-access/recovery-store.js";

function storage() {
  const records = new Map();
  return {
    records,
    getItem: key => records.get(key) ?? null,
    setItem: (key, value) => records.set(key, value),
  };
}

test("侧栏选择在重建客户端后恢复，并按设备授权隔离", () => {
  const disk = storage(), first = createSidebarPreferenceStore({ storage: disk });
  first.write('["device-a",1]', { mode: "project", chats: "manual" });
  first.write('["device-b",1]', { mode: "list" });
  const reopened = createSidebarPreferenceStore({ storage: disk });
  assert.deepEqual(reopened.read('["device-a",1]'), { mode: "project", chats: "manual" });
  assert.deepEqual(reopened.read('["device-b",1]'), { mode: "list" });
  assert.deepEqual(reopened.read('["device-a",2]'), {});
});

test("两个独立客户端的选择不会通过共享桌面配置互相覆盖", () => {
  const a = createSidebarPreferenceStore({ storage: storage() });
  const b = createSidebarPreferenceStore({ storage: storage() });
  a.write("same-desktop", { mode: "project", pinned: "manual" });
  b.write("same-desktop", { mode: "list", pinned: "updated_at" });
  assert.deepEqual(a.read("same-desktop"), { mode: "project", pinned: "manual" });
  assert.deepEqual(b.read("same-desktop"), { mode: "list", pinned: "updated_at" });
});

test("同一客户端的多页局部修改保留其它已保存选项", () => {
  const disk = storage();
  const a = createSidebarPreferenceStore({ storage: disk });
  const b = createSidebarPreferenceStore({ storage: disk });
  a.write("device", { mode: "project" });
  b.write("device", { chats: "manual" });
  a.write("device", { pinned: "updated_at" });
  assert.deepEqual(b.read("device"), { mode: "project", chats: "manual", pinned: "updated_at" });
});

test("损坏或未来版本偏好不会当成有效数据，未知字段不进入保存结果", () => {
  const disk = storage(), store = createSidebarPreferenceStore({ storage: disk });
  store.write("device", { mode: "list", unknown: "not-a-preference", projects: "invalid" });
  assert.deepEqual(store.read("device"), { mode: "list" });
  const key = [...disk.records.keys()][0];
  disk.records.set(key, "{broken");
  assert.deepEqual(store.read("device"), {});
  disk.records.set(key, JSON.stringify({ version: 999, preferences: { mode: "project" } }));
  assert.deepEqual(store.read("device"), {});
  assert.throws(() => store.write("", { mode: "project" }), /授权范围/);
});

test("存储事件只匹配当前授权范围", () => {
  const disk = storage(), store = createSidebarPreferenceStore({ storage: disk });
  store.write("device-a", { mode: "project" });
  const key = [...disk.records.keys()][0];
  assert.equal(store.ownsStorageEvent("device-a", { key }), true);
  assert.equal(store.ownsStorageEvent("device-b", { key }), false);
  assert.equal(store.ownsStorageEvent(null, { key }), false);
});

const app = await readFile(new URL("../assets/remote-access/app.js", import.meta.url), "utf8");
const channels = await readFile(new URL("../assets/remote-access/channels.js", import.meta.url), "utf8");

function goalControlHarness({ status = "active", readOnly = false } = {}) {
  const disk = storage(), calls = [], notices = [];
  disk.removeItem = key => disk.records.delete(key);
  const goal = { objective: "手机目标操作回归", status, createdAt: 100, updatedAt: 110,
    tokenBudget: 8000, tokensUsed: 1200, timeUsedSeconds: 60 };
  const context = {
    sessionStorage: disk, session: { device: { id: "device-a" } },
    thread: { id: "main", goal, messages: ["保留原会话消息"] }, selectedId: "main",
    goalVersion: 0, connected: true, TextEncoder, queueMicrotask,
    threadReadOnly: () => readOnly, renderGoalStatus() {}, reloadThread: async () => {},
    setTimeout: () => 1, clearTimeout() {}, randomId: () => `goal-${calls.length}`,
    notice: text => notices.push(text), confirmAction: async () => true,
    api: async (path, payload, receiptId) => {
      calls.push({ path, payload, receiptId });
      return context.respond(path, payload, receiptId);
    },
    respond: (_path, payload, receiptId) => ({ requestId: receiptId, state: "completed", result: {
      threadId: "main", goal: payload.operation === "delete" ? null
        : { ...goal, objective: payload.objective,
          status: payload.operation === "pause" ? "paused"
            : payload.operation === "resume" ? "active" : payload.expectedStatus },
    } }),
  };
  vm.createContext(context);
  const from = app.indexOf('const goalEditStorageKey =');
  const to = app.indexOf("function renderGoalEditor(", from);
  assert.ok(from >= 0 && to > from);
  vm.runInContext(app.slice(from, to), context);
  return { context, calls, notices, disk,
    pending: () => vm.runInContext("goalEditPending", context) };
}

test("目标暂停、开始、删除提交准确目标，保留计数、预算和会话消息", async () => {
  const { context, calls } = goalControlHarness();
  for (const [operation, status] of [["pause", "paused"], ["resume", "active"]]) {
    await context.changeGoalState(operation, context.thread.goal);
    assert.equal(context.thread.goal.status, status);
    assert.equal(context.thread.goal.tokensUsed, 1200);
    assert.equal(context.thread.goal.tokenBudget, 8000);
    assert.equal(context.thread.goal.createdAt, 100);
  }
  await context.changeGoalState("delete", context.thread.goal);
  assert.equal(context.thread.goal, null);
  assert.deepEqual(context.thread.messages, ["保留原会话消息"]);
  assert.deepEqual(calls.map(call => call.payload.operation), ["pause", "resume", "delete"]);
  assert.ok(calls.every(call => call.path === "/api/action/updateGoal"
    && call.payload.threadId === "main" && call.payload.expectedCreatedAt === 100));
});

test("只读、预算受限和删除确认期间目标变化时不派发错误操作", async () => {
  const locked = goalControlHarness({ readOnly: true });
  await locked.context.changeGoalState("pause", locked.context.thread.goal);
  assert.equal(locked.calls.length, 0);
  const budget = goalControlHarness({ status: "budgetLimited" });
  await budget.context.changeGoalState("resume", budget.context.thread.goal);
  assert.equal(budget.calls.length, 0);
  const changed = goalControlHarness();
  changed.context.confirmAction = async () => {
    changed.context.thread.goal = { ...changed.context.thread.goal, objective: "另一个目标", createdAt: 200 };
    return true;
  };
  await changed.context.changeGoalState("delete", changed.context.thread.goal);
  assert.equal(changed.calls.length, 0);
  assert.equal(changed.context.thread.goal.objective, "另一个目标");
});

test("目标操作结果未知时只查原回执，不重复暂停或删除，不覆盖后来目标", async () => {
  const { context, calls, pending } = goalControlHarness();
  const original = structuredClone(context.thread.goal);
  context.respond = () => ({ state: "unknown", result: { outcomeUnknown: true } });
  await context.changeGoalState("pause", original);
  const receiptId = pending().id;
  await context.changeGoalState("pause", original);
  await context.changeGoalState("delete", original);
  assert.equal(calls.length, 1);
  context.thread.goal = { ...original, objective: "后来创建的目标", createdAt: 200 };
  context.respond = () => ({ state: "completed", result: {
    threadId: "main", goal: { ...original, status: "paused" },
  } });
  await context.resolveGoalEditReceipt();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].path, `/api/receipts/${receiptId}`);
  assert.equal(pending(), null);
  assert.equal(context.thread.goal.objective, "后来创建的目标");
});

test("原有目标正文编辑仍兼容没有operation的请求，不改变目标暂停状态", async () => {
  const { context, calls } = goalControlHarness({ status: "paused" });
  vm.runInContext(`goalEditDraft = { threadId:"main",
    baseline:goalEditBaseline(thread.goal), objective:"修改后的目标" }`, context);
  await context.submitGoalEdit();
  assert.equal(calls.length, 1);
  assert.equal(Object.hasOwn(calls[0].payload, "operation"), false);
  assert.equal(context.thread.goal.objective, "修改后的目标");
  assert.equal(context.thread.goal.status, "paused");
});

function queueDeliveryHarness(messages) {
  const element = (tag, className = "", text = "") => ({
    tag, className, text, dataset: {}, attributes: {}, children: [],
    setAttribute(name, value) { this.attributes[name] = value; },
    append(...children) { this.children.push(...children); },
  });
  const notices = [];
  const context = {
    element, icon: name => element("svg", name),
    button: (label, aria, action, className) => {
      const node = element("button", className, label);
      node.setAttribute("aria-label", aria);
      node.action = action;
      return node;
    },
    selectedId: "main", thread: { id: "main", queuedMessages: messages },
    queueVersion: 1, connected: true, busy: false, stopping: false, pending: null,
    queuedActionBusy: new Set(), editingQueuedMessage: null, editingMessage: null,
    threadReadOnly: () => false, compactionLocked: () => false,
    queueDisplayMessages: value => value, queueMessageText: value => value.text,
    applyQueueSnapshot: () => true, queueSnapshotIsVersioned: () => true,
    notice: value => notices.push(value),
  };
  vm.createContext(context);
  for (const [start, end] of [
    ["function queueReorderAvailable()", "function moveQueueItem("],
    ["function applyQueuedSendResult(", "function queueMessageText("],
    ["function renderQueuedMessages(", "function renderQueueArea("],
  ]) {
    const from = app.indexOf(start), to = app.indexOf(end, from);
    assert.ok(from >= 0 && to > from);
    vm.runInContext(app.slice(from, to), context);
  }
  return { context, notices };
}

test("已接受的本地队列残留不算待执行，不提供引导、编辑或重排，保留明确移除入口", () => {
  const messages = [
    { id: "consumed", clientUserMessageId: "client-a", createdAt: 1, text: "已接受的原消息",
      submission: { status: "queued" }, remoteDeliveryState: "accepted", remoteQueueSyncPending: true },
    { id: "waiting", clientUserMessageId: "client-b", createdAt: 2, text: "尚未执行的消息",
      submission: { status: "queued" } },
  ];
  const { context, notices } = queueDeliveryHarness(messages);
  const queue = context.renderQueuedMessages(messages);
  assert.equal(queue.attributes["aria-label"], "原生消息队列，1 条待执行，1 条已发送待同步");
  const accepted = queue.children.find(row => row.dataset.messageId === "consumed");
  const status = accepted.children.find(node => node.className === "queued-steer-button");
  assert.equal(status.disabled, true);
  assert.equal(status.attributes["aria-label"], "消息已发送，队列记录待同步");
  const menu = accepted.children.find(node => node.className === "queued-menu-button");
  assert.equal(menu.disabled, false, "只限制受影响动作，保留用户显式移除记录");
  assert.equal(menu.menuOptions.find(option => option.label === "编辑消息").disabled, true);
  assert.equal(menu.menuOptions.find(option => option.value === "remove").label, "移除已发送记录");
  assert.equal(context.queueReorderAvailable(), false);
  const waiting = queue.children.find(row => row.dataset.messageId === "waiting");
  assert.equal(waiting.children.find(node => node.className === "queued-steer-button").disabled, false);
  context.applyQueuedSendResult("main", {
    threadId: "main", deliveryState: "accepted", queueSyncPending: true, queuedMessages: messages,
  }, 1);
  assert.deepEqual(notices, [], "已发送状态由队列条目展示，不额外弹出提示");
});

test("普通队列仍按待执行显示，保留引导、编辑、删除及排序", () => {
  const messages = ["a", "b"].map((id, index) => ({
    id, clientUserMessageId: `client-${id}`, createdAt: index + 1, text: `待执行-${id}`,
    submission: { status: "queued" },
  }));
  const { context } = queueDeliveryHarness(messages);
  const queue = context.renderQueuedMessages(messages);
  assert.equal(queue.attributes["aria-label"], "原生消息队列，2 条待执行");
  assert.equal(context.queueReorderAvailable(), true);
  for (const row of queue.children) {
    assert.equal(row.children.find(node => node.className === "queued-steer-button").disabled, false);
    const menu = row.children.find(node => node.className === "queued-menu-button");
    assert.equal(menu.menuOptions.find(option => option.label === "编辑消息").disabled, false);
    assert.equal(menu.menuOptions.find(option => option.value === "remove").label, "删除排队消息");
  }
});

test("原生暂停的队列消息显示可用重试按钮，点击仍操作原消息", () => {
  for (const submission of [undefined, { status: "queued" }]) {
    const message = { id: "original-queue", text: "原图文消息", clientUserMessageId: "original-client",
      createdAt: 1, pausedReason: "原生准备失败", ...(submission ? { submission } : {}) };
    const { context } = queueDeliveryHarness([message]);
    const calls = [];
    context.steerQueuedMessage = (threadId, messageId) => calls.push({ threadId, messageId });
    const row = context.renderQueuedMessages([message]).children[0];
    const retry = row.children.find(node => node.className === "queued-steer-button");
    assert.equal(retry.attributes["aria-label"], "重试此队列消息");
    assert.equal(retry.children[0].className, "retry");
    assert.equal(retry.children[1].text, "重试");
    assert.equal(retry.disabled, false);
    retry.action();
    assert.deepEqual(calls, [{ threadId: "main", messageId: "original-queue" }]);
  }
});

test("结果未知或正在核对回执时不把队列消息变成可重复提交的重试", () => {
  for (const uncertain of ["native", "receipt"]) {
    const message = { id: "original-queue", text: "原消息", pausedReason: "之前的异常",
      ...(uncertain === "native" ? { submission: { status: "outcome-unknown" } } : {}) };
    const { context } = queueDeliveryHarness([message]);
    if (uncertain === "receipt") context.pending = {
      action: "steerQueued", payload: { threadId: "main", messageId: message.id },
    };
    const row = context.renderQueuedMessages([message]).children[0];
    const action = row.children.find(node => node.className === "queued-steer-button");
    assert.equal(action.children[1].text, "待确认");
    assert.equal(action.disabled, true);
  }
});

test("普通引导、原生禁止发送及已接受残留保持各自状态", () => {
  const messages = [
    { id: "normal", text: "普通消息" },
    { id: "blocked", text: "暂不支持", pausedReason: "宿主暂不可用", canSendNow: false },
    { id: "accepted", text: "已接受", pausedReason: "旧异常", remoteDeliveryState: "accepted" },
  ];
  const { context } = queueDeliveryHarness(messages);
  const actions = context.renderQueuedMessages(messages).children
    .map(row => row.children.find(node => node.className === "queued-steer-button"));
  assert.equal(actions[0].children[1].text, "引导");
  assert.equal(actions[0].disabled, false);
  assert.equal(actions[1].children[1].text, "重试");
  assert.equal(actions[1].disabled, true);
  assert.equal(actions[2].children[1].text, "已发送");
  assert.equal(actions[2].disabled, true);
});

function draftOwnerHarness() {
  const empty = () => ({ text: "", skills: [], attachments: [], goalMode: null });
  const stored = new Map([
    ["$new:", { text: "尚未发送的新会话草稿\n保留第二行", skills: [{ name: "只读检查", path: "/test/skill" }],
      attachments: [{ kind: "inline", name: "test.png", dataUrl: "data:image/png;base64,dGVzdA==" }], goalMode: null }],
    ["owned-thread", { ...empty(), text: "会话自身草稿" }],
  ]);
  const writes = [], queued = [];
  const context = {
    selectedId: null, creating: false, creationProject: "",
    editingQueuedMessage: null, selectedGoal: null, currentDraft: empty(),
    draftKey: () => context.selectedId || `$new:${context.creationProject}`,
    snapshotDraft: () => structuredClone(context.currentDraft),
    storedDraft: key => structuredClone(stored.get(key) || empty()),
    storeDraft: (key, value) => {
      writes.push({ key, value: structuredClone(value) });
      stored.set(key, structuredClone(value));
    },
    rememberQueuedMessageEdit: () => queued.push(context.editingQueuedMessage.editId),
    sameSkills: (left, right) => JSON.stringify(left) === JSON.stringify(right),
    randomId: () => "new-goal-draft",
  };
  vm.createContext(context);
  const from = app.indexOf("function rememberDraft()"), to = app.indexOf("function restoreDraft(", from);
  assert.ok(from >= 0 && to > from);
  vm.runInContext(app.slice(from, to), context);
  return { context, stored, writes, queued };
}

test("欢迎页和只读初始页没有草稿所属会话，空输入不得覆盖已恢复的新会话草稿", () => {
  const { context, stored, writes } = draftOwnerHarness();
  const original = structuredClone(stored.get("$new:"));
  context.rememberDraft();
  // 页面退出时会再次尝试记忆输入；仍不能把未激活的空编辑器当成用户清空。
  context.rememberDraft();
  assert.equal(writes.length, 0);
  assert.deepEqual(stored.get("$new:"), original);
  context.creating = true;
  context.currentDraft = structuredClone(stored.get("$new:"));
  context.rememberDraft();
  assert.deepEqual(stored.get("$new:"), original);
});

test("真实新会话和已有会话中的主动清空仍保存到各自草稿，不能因保护欢迎页而失效", () => {
  const { context, stored } = draftOwnerHarness();
  context.creating = true;
  context.rememberDraft();
  assert.equal(stored.get("$new:").text, "");
  assert.equal(stored.get("owned-thread").text, "会话自身草稿");
  context.creating = false;
  context.selectedId = "owned-thread";
  context.rememberDraft();
  assert.equal(stored.get("owned-thread").text, "");
});

test("队列编辑的草稿继续使用准确编辑事务，不覆盖普通新会话草稿", () => {
  const { context, stored, writes, queued } = draftOwnerHarness();
  const original = structuredClone(stored.get("$new:"));
  context.selectedId = "owned-thread";
  context.editingQueuedMessage = { threadId: "owned-thread", editId: "owned-edit" };
  context.currentDraft.text = "正在修改队列消息";
  context.rememberDraft();
  assert.deepEqual(queued, ["owned-edit"]);
  assert.equal(writes.length, 0);
  assert.deepEqual(stored.get("$new:"), original);
});

function queuedEditPersistenceHarness({ storageFails = false } = {}) {
  const { context, writes: draftWrites } = draftOwnerHarness();
  const persisted = [], notices = [], submissions = [];
  const prompt = { value: "修改后的队列消息 ", querySelector: () => null, focus() {} };
  const attachments = [{ name: "保留的图片.png", kind: "inline" }];
  const edit = {
    threadId: "owned-thread", editId: "owned-edit", state: "editing",
    message: { id: "queued-a" }, text: "原始消息", attachments: [],
  };
  Object.assign(context, {
    selectedId: edit.threadId, thread: { id: edit.threadId }, editingQueuedMessage: edit,
    queuedEdits: new Map([[edit.threadId, edit]]), queueVersion: 1, queuedActionBusy: new Set(),
    session: { capabilities: {} }, connected: true, busy: false, stopping: false, pending: null,
    $: () => prompt, composerAttachmentSnapshot: () => structuredClone(attachments),
    composerAttachmentsLoading: () => false, composerHasAttachments: () => false,
    threadReadOnly: () => false, compactionLocked: () => false,
    forgetPromptCanonicalText() {}, restorePromptCanonicalContent() {},
    syncSelectedSkillsFromPrompt() {}, syncPromptTrailingBreak() {},
    updateComposer() {}, closeSkillMenu() {}, updateSkillMenu() {}, skillIndex: 0,
    completeQueuedEditAction() {},
    notice: message => notices.push(message),
    sessionStorage: {
      setItem(key, value) {
        persisted.push({ key, value });
        if (storageFails) throw new Error("quota");
      },
      removeItem() { assert.fail("仍有队列编辑时不能删除其存储"); },
    },
    mutate: async (action, payload) => { submissions.push({ action, payload }); return {}; },
  });
  vm.runInContext([
    sourceSection("function persistQueuedEdits(", "function restoreQueuedMessageEdit("),
    sourceSection("function handlePromptInput(", 'document.addEventListener("pointerdown"'),
    sourceSection("async function submitQueuedMessageEdit(", "function syncEditableMessageMetadata("),
  ].join("\n"), context);
  return { context, prompt, edit, attachments, persisted, notices, submissions, draftWrites };
}

test("队列编辑普通输入只持久化一次最终文本和附件，不写普通草稿", () => {
  const { context, prompt, attachments, persisted, draftWrites } = queuedEditPersistenceHarness();
  context.restorePromptCanonicalContent = () => { prompt.value = "还原后的规范文本"; };
  context.handlePromptInput({}, { preserveCanonical: true });
  assert.equal(persisted.length, 1);
  const [saved] = JSON.parse(persisted[0].value);
  assert.equal(saved.text, prompt.value);
  assert.deepEqual(saved.attachments, attachments);
  assert.equal(draftWrites.length, 0);
});

test("队列编辑组合输入仍保存一次当前文本，并延后规范化", () => {
  const { context, prompt, persisted, draftWrites } = queuedEditPersistenceHarness();
  context.restorePromptCanonicalContent = () => assert.fail("组合输入尚未完成");
  context.syncSelectedSkillsFromPrompt = () => assert.fail("组合输入尚未完成");
  context.handlePromptInput({ isComposing: true }, { preserveCanonical: true });
  assert.equal(persisted.length, 1);
  assert.equal(JSON.parse(persisted[0].value)[0].text, prompt.value);
  assert.equal(draftWrites.length, 0);
});

test("队列编辑保存只持久化一次最新草稿，再提交可靠写入", async () => {
  const { context, prompt, attachments, persisted, submissions } = queuedEditPersistenceHarness();
  await context.submitQueuedMessageEdit();
  assert.equal(persisted.length, 1);
  const [saved] = JSON.parse(persisted[0].value);
  assert.equal(saved.text, prompt.value);
  assert.deepEqual(saved.attachments, attachments);
  assert.equal(submissions.length, 1);
  assert.equal(submissions[0].action, "editQueued");
  assert.equal(submissions[0].payload.prompt, prompt.value.trim());
  assert.equal(context.busy, false);
});

test("队列编辑存储失败时保留最新草稿、提示一次并阻止提交", async () => {
  const { context, prompt, edit, attachments, persisted, notices, submissions }
    = queuedEditPersistenceHarness({ storageFails: true });
  await context.submitQueuedMessageEdit();
  assert.equal(persisted.length, 1);
  assert.equal(submissions.length, 0);
  assert.equal(notices.length, 1);
  assert.match(notices[0], /无法保存队列编辑草稿，操作未提交/);
  assert.equal(edit.text, prompt.value);
  assert.deepEqual(edit.attachments, attachments);
  assert.equal(edit.state, "editing");
  assert.equal(context.busy, false);
  assert.equal(context.queuedActionBusy.size, 0);
});

function queuedEditFeedbackHarness() {
  const h = queuedEditPersistenceHarness(), { context } = h, response = deferred();
  const element = (tag, className = "", text = "") => ({
    tag, className, text, dataset: {}, attributes: {}, children: [], disabled: false,
    get textContent() { return this.text + this.children.map(node => node.textContent).join(""); },
    set textContent(value) { this.text = value; this.children = []; },
    setAttribute(key, value) { this.attributes[key] = value; },
    append(...children) { this.children.push(...children); },
    replaceChildren(...children) { this.text = ""; this.children = children; },
    querySelector(selector) {
      for (const node of this.children) {
        if (selector.startsWith(".") ? node.className.split(" ").includes(selector.slice(1)) : node.tag === selector)
          return node;
        const nested = node.querySelector?.(selector);
        if (nested) return nested;
      }
      return null;
    },
  });
  const area = element("div"), send = element("button"), status = element("span");
  Object.assign(context, {
    element, icon: name => element("svg", name), send,
    button: (label, aria, action, className) => {
      const node = element("button", className, label); node.setAttribute("aria-label", aria); node.action = action;
      return node;
    },
    document: { createTextNode: text => element("#text", "", text) },
    $: (selector, root) => root ? root.querySelector(selector)
      : selector === "#edit-message-area" ? area : selector === "#send-status" ? status : h.prompt,
    session: { device: { id: "device-a" }, capabilities: {} },
    sending: null, compacting: false, goalMode: false, editing: true, editingMessage: null, active: false, hasAttachments: false,
    unsupportedImages: false, unsupportedProjectless: false, goalReason: "", queueOnly: false,
    compactionSendReason: "", compaction: { state: "idle" }, deleting: false, prompt: h.prompt.value,
    pageLeaving: false, stream: null, composerDiagnosticState: "", creating: false,
    privateInputScope: () => "device-a", recordRuntimeDiagnostic() {},
    mutate: async (action, payload) => {
      context.pending = { id: "edit-receipt", action, payload, deviceId: "device-a", time: Date.now() };
      try {
        const result = await response.promise;
        if (result.outcomeUnknown) throw Object.assign(new Error("操作结果未确认"), { pendingFeedback: true });
        context.pending = null;
        return result;
      } catch (error) {
        if (!error.pendingFeedback) context.pending = null;
        throw error;
      }
    },
  });
  h.edit.deviceId = "device-a";
  vm.runInContext([
    sourceSection("function queuedEditSubmittingAction(", "async function submitMessageEdit("),
    sourceSection("function reportMutationError(", "function acceptedSendItem("),
    sourceSection("const pendingFeedbackDelayMs =", "function clearPendingFeedbackTimer("),
    `function renderSendFeedback(){${sourceSection("  const queuedEditAction = queuedEditSubmittingAction();",
      '  $("#prompt").disabled =')}}`,
  ].join("\n"), context);
  context.updateComposer = () => { context.renderMessageEditBanner(); context.renderSendFeedback(); };
  context.updateComposer();
  return { ...h, response, area, send, status };
}

test("取消队列编辑立即显示取消中和spinner，明确失败后恢复原标签和可编辑状态", async () => {
  const h = queuedEditFeedbackHarness(), operation = h.context.submitQueuedMessageEdit(true);
  const cancel = h.area.querySelector("button");
  assert.match(cancel.textContent, /取消中/);
  assert.ok(cancel.querySelector(".spinner"));
  assert.equal(cancel.disabled, true);
  assert.equal(h.send.attributes["aria-label"], "取消中…");
  assert.ok(h.send.querySelector(".spinner"));
  assert.equal(h.send.disabled, true);
  h.response.reject(new Error("取消操作明确失败"));
  await operation;
  const restored = h.area.querySelector("button");
  assert.equal(restored.textContent, "取消编辑");
  assert.equal(restored.disabled, false);
  assert.equal(restored.querySelector(".spinner"), null);
  assert.equal(h.send.attributes["aria-label"], "保存队列消息修改");
  assert.equal(h.send.querySelector(".spinner"), null);
  assert.equal(h.send.disabled, false);
  assert.equal(h.status.textContent, "");
  assert.deepEqual(h.notices, ["取消操作明确失败"]);
});

test("保存队列编辑立即显示保存中，成功结束编辑后恢复普通发送入口", async () => {
  const h = queuedEditFeedbackHarness();
  h.context.completeQueuedEditAction = () => {
    h.context.editingQueuedMessage = null; h.context.editing = false;
  };
  const operation = h.context.submitQueuedMessageEdit();
  assert.equal(h.status.textContent, "保存中…");
  assert.equal(h.send.attributes["aria-label"], "保存中…");
  assert.ok(h.send.querySelector(".spinner"));
  assert.equal(h.send.disabled, true);
  assert.equal(h.area.querySelector("button").textContent, "取消编辑");
  h.response.resolve({});
  await operation;
  assert.equal(h.status.textContent, "");
  assert.equal(h.area.hidden, true);
  assert.equal(h.send.attributes["aria-label"], "发送消息");
  assert.equal(h.send.querySelector(".spinner"), null);
  assert.equal(h.send.disabled, false);
});

test("取消和保存未知回执停止在途spinner但保持锁定，五秒前无额外提示", async () => {
  for (const cancel of [false, true]) {
    const h = queuedEditFeedbackHarness(), operation = h.context.submitQueuedMessageEdit(cancel);
    h.response.resolve({ outcomeUnknown: true });
    await operation;
    const record = h.context.pending;
    assert.equal(h.context.busy, false);
    assert.equal(h.area.querySelector("button").textContent, "取消编辑");
    assert.equal(h.area.querySelector("button").disabled, true);
    assert.equal(h.send.disabled, true);
    assert.equal(h.area.querySelector(".spinner"), null);
    assert.equal(h.send.querySelector(".spinner"), null);
    assert.equal(h.context.pendingFeedbackReady(record), false);
    record.time -= 5000;
    assert.equal(h.context.pendingFeedbackReady(record), true);
    assert.equal(h.context.pending, record);
    assert.deepEqual(h.notices, []);
  }
});

test("其他editId的提交不显示当前编辑的取消中或保存中，开始编辑仍只显示正在取回", () => {
  const h = queuedEditFeedbackHarness();
  h.context.busy = true;
  h.context.queuedActionBusy.add(`${h.edit.threadId}:${h.edit.message.id}`);
  h.context.pending = { action: "cancelQueuedEdit", deviceId: "device-a",
    payload: { threadId: h.edit.threadId, editId: "other-edit" } };
  h.context.updateComposer();
  assert.equal(h.area.querySelector("button").textContent, "取消编辑");
  assert.equal(h.area.querySelector(".spinner"), null);
  assert.equal(h.send.querySelector(".spinner"), null);
  h.edit.state = "starting";
  h.context.pending.payload.editId = h.edit.editId;
  h.context.updateComposer();
  assert.match(h.area.textContent, /正在取回排队消息/);
  assert.equal(h.area.querySelector("button").textContent, "重新取回");
  assert.equal(h.area.querySelector(".spinner"), null);
});

function questionReceiptHarness(readReceipt) {
  const writes = [], calls = [], reloads = [];
  const key = JSON.stringify(["device-a", "thread-a", "question-a"]);
  const record = { id: "receipt-a", deviceId: "device-a", threadId: "thread-a",
    requestId: "question-a", state: "unknown" };
  const context = {
    session: { device: { id: "device-a", createdAtMs: 1 } },
    selectedId: "thread-a", questionVersion: 0, notifiedQuestions: null,
    document: { hidden: false }, navigator: { onLine: true }, pageLeaving: false,
    thread: { id: "thread-a", requests: [
      { id: "question-a", method: "remote/requestUserInputAsync" },
      { id: "question-new", method: "remote/requestUserInputAsync" },
    ] },
    questionReplyReceipts: new Map([[key, record]]), questionReceiptChecks: new Map(),
    questionDrafts: new Map([[key, new Map([["question-field", "未提交的私密答案"]])]]),
    sessionStorage: { setItem: (name, value) => writes.push({ name, value }) },
    privateInputScope: session => JSON.stringify([session?.device?.id, session?.device?.createdAtMs]),
    renderApprovals() {}, renderSidebar() {}, notice() {}, approvalSignature: null,
    reloadThread: async () => { reloads.push(context.thread?.id); },
    api: async (path, payload) => { calls.push({ path, payload }); return readReceipt(path); },
  };
  vm.createContext(context);
  const from = app.indexOf("function replaceAsyncQuestions("), to = app.indexOf("function renderAsyncQuestion(", from);
  assert.ok(from >= 0 && to > from);
  vm.runInContext(app.slice(from, to), context);
  return { context, key, record, writes, calls, reloads };
}

test("问答未知回执只查询原ID，完成后解除锁且不清除后来的问题", async () => {
  const { context, key, calls, writes, reloads } = questionReceiptHarness(async () => ({
    requestId: "receipt-a", state: "completed",
    result: { ok: true, threadId: "thread-a", requestId: "question-a" },
  }));
  await context.recoverQuestionReceipts?.();
  assert.deepEqual(calls, [{ path: "/api/receipts/receipt-a", payload: undefined }]);
  assert.equal(context.questionReplyReceipts.has(key), false);
  assert.deepEqual(Array.from(context.thread.requests, request => request.id), ["question-new"]);
  assert.deepEqual(reloads, ["thread-a"]);
  assert.ok(writes.every(write => !write.value.includes("未提交的私密答案")));
});

test("问答明确失败解锁但保留答案草稿，unknown和404不能解锁或重发", async () => {
  for (const outcome of ["failed", "unknown", "not-found", "failed-unknown"]) {
    const { context, key, calls } = questionReceiptHarness(async () => {
      if (outcome === "not-found") throw Object.assign(new Error("未找到"), { code: 404 });
      return { requestId: "receipt-a", state: outcome === "failed-unknown" ? "failed" : outcome,
        result: { message: "未完成", outcomeUnknown: outcome === "failed-unknown" } };
    });
    await context.recoverQuestionReceipts?.();
    assert.equal(calls.length, 1);
    assert.equal(calls[0].path, "/api/receipts/receipt-a");
    assert.equal(calls[0].payload, undefined);
    assert.equal(context.questionReplyReceipts.has(key), outcome !== "failed");
    assert.equal(context.questionDrafts.get(key).get("question-field"), "未提交的私密答案");
  }
});

test("并发问答回执核对共用一次读取，不能清除同键后来创建的操作锁", async () => {
  let finish;
  const { context, key, calls } = questionReceiptHarness(() => new Promise(resolve => { finish = resolve; }));
  const first = context.recoverQuestionReceipts?.(), second = context.recoverQuestionReceipts?.();
  assert.equal(calls.length, 1);
  context.questionReplyReceipts.set(key, { ...context.questionReplyReceipts.get(key), id: "receipt-new" });
  finish({ requestId: "receipt-a", state: "completed", result: { ok: true } });
  await Promise.all([first, second]);
  assert.equal(context.questionReplyReceipts.get(key).id, "receipt-new");
  assert.equal(context.questionDrafts.has(key), true);
  assert.equal(context.questionReceiptChecks.size, 0);
});

test("迟到的初始问答回执和换授权后的读取不能清除新记录", async () => {
  const direct = questionReceiptHarness(async () => ({}));
  direct.context.questionReplyReceipts.set(direct.key, { ...direct.record, id: "receipt-new" });
  await direct.context.applyQuestionReceipt(direct.key, {
    requestId: "receipt-a", state: "completed", result: { ok: true },
  });
  assert.equal(direct.context.questionReplyReceipts.get(direct.key)?.id, "receipt-new");
  let finish;
  const scoped = questionReceiptHarness(() => new Promise(resolve => { finish = resolve; }));
  const recovery = scoped.context.recoverQuestionReceipts?.();
  assert.equal(scoped.calls.length, 1);
  scoped.context.session = { device: { id: "device-b", createdAtMs: 2 } };
  finish({ requestId: "receipt-a", state: "completed", result: { ok: true } });
  await recovery;
  assert.equal(scoped.context.questionReplyReceipts.has(scoped.key), true);
});

test("问答回执核对可在原生断开或只读时进行，但后台页和其他设备记录不发起读取", async () => {
  for (const situation of ["native-unavailable", "read-only", "hidden", "other-device"]) {
    const { context, calls } = questionReceiptHarness(async () => ({
      requestId: "receipt-a", state: "pending", result: null,
    }));
    context.connected = false;
    if (situation === "read-only") context.session.readOnly = true;
    if (situation === "hidden") context.document.hidden = true;
    if (situation === "other-device") context.session.device.id = "device-b";
    await context.recoverQuestionReceipts?.();
    assert.equal(calls.length, ["hidden", "other-device"].includes(situation) ? 0 : 1, situation);
  }
});

test("切到另一会话后核对原回答，只解除原锁，不改当前会话的问题和草稿", async () => {
  const { context, key, reloads } = questionReceiptHarness(async () => ({
    requestId: "receipt-a", state: "completed",
    result: { ok: true, threadId: "thread-a", requestId: "question-a" },
  }));
  const other = { id: "thread-b", requests: [{ id: "other-question", method: "remote/requestUserInputAsync" }] };
  context.thread = other; context.selectedId = other.id;
  context.ordinaryDraft = "另一会话中未发送的两行草稿\n保留内容";
  await context.recoverQuestionReceipts();
  assert.equal(context.questionReplyReceipts.has(key), false);
  assert.equal(context.thread, other);
  assert.deepEqual(other.requests, [{ id: "other-question", method: "remote/requestUserInputAsync" }]);
  assert.equal(context.ordinaryDraft, "另一会话中未发送的两行草稿\n保留内容");
  assert.deepEqual(reloads, []);
});

function toolRenderer() {
  function element(tag, className = "", text = "") {
    const node = { tagName: tag.toUpperCase(), className, ownText: String(text), children: [], attributes: {},
      append(...children) { this.children.push(...children); },
      replaceChildren(...children) { this.children = children; },
      setAttribute(name, value) { this.attributes[name] = value; },
      get textContent() { return this.ownText + this.children.map(child => child?.textContent ?? child).join(""); },
    };
    node.classList = { add(...names) { node.className += ` ${names.join(" ")}`; } };
    return node;
  }
  const context = {
    element, icon: () => element("svg"),
    isSubagentItem: () => false, transcriptEventTypes: new Set(),
    skillReadInfo: () => null, renderToolOutput: () => {},
    genericToolPresentation: () => ({ description: "测试工具", icon: "tool" }),
    commandText: item => item.command || "",
  };
  vm.createContext(context);
  const start = app.indexOf("function renderItem("), end = app.indexOf("function reconcileChildren(", start);
  assert.ok(start >= 0 && end > start);
  vm.runInContext(app.slice(start, end), context);
  return context.renderItem;
}

test("工具证据未知时实际渲染状态未知，不能因父轮运行或结束显示完成勾号", () => {
  const render = toolRenderer();
  for (const type of ["sleep", "reasoning", "dynamicToolCall", "commandExecution"]) {
    for (const parentRunning of [false, true]) {
      const node = render({ id: "unknown-tool", type, status: "unknown", remoteItemCompleted: false },
        null, false, parentRunning);
      assert.match(node.textContent, /状态未知/);
      assert.doesNotMatch(node.textContent, /✓|已完成思考|已运行|进行中/);
    }
  }
});

test("等待工具在真实进行中与完成两态分别渲染，不以预计时长冒充完成", () => {
  const render = toolRenderer();
  const active = render({ type: "sleep", id: "sleep", status: "inProgress",
    startedAtMs: 10, durationMs: 15000, remoteItemCompleted: false }, null, false, true);
  assert.match(active.textContent, /进行中/);
  assert.doesNotMatch(active.textContent, /✓/);
  const completed = render({ type: "sleep", id: "sleep", status: "completed",
    startedAtMs: 10, completedAtMs: 15010, remoteItemCompleted: true }, null, false, false);
  assert.match(completed.textContent, /✓/);
  assert.doesNotMatch(completed.textContent, /进行中|状态未知/);
});

test("失败与中断的工具条目不能渲染成功勾号", () => {
  const render = toolRenderer();
  for (const status of ["failed", "interrupted", "cancelled", "canceled"]) {
    const node = render({ type: "sleep", id: "sleep", status }, null, false, false);
    assert.doesNotMatch(node.textContent, /✓/);
    assert.match(node.textContent, status === "failed" ? /失败/ : /已中断/);
  }
});

function transportClient(fetchResponse) {
  const calls = [];
  const context = {
    URL, Headers, Response, AbortController, DOMException, TextEncoder, performance,
    setTimeout, clearTimeout, setInterval, clearInterval,
    location: new URL("http://127.0.0.1:54321/"),
    document: { hidden: false }, navigator: { onLine: true },
    getAndroidAppInfo: async () => ({}), isAndroidContainer: () => false,
    diagnosticLocation: value => ({ origin: value }), recordDiagnostic: () => {},
    traceDiagnosticRequest: () => ({ response() {}, finish() {} }),
    session: null, pageLeaving: false,
    threadReadOnly: () => false, threadDeletionLocked: () => false, threadDeleted: () => false,
    beginPwaForegroundWork: () => () => {}, currentChannelMeasurements: () => [],
    markMessageChannelUnavailable: () => {}, requireAuthentication: () => {},
    fetch: async (url, options) => {
      const call = { path: new URL(url).pathname, options };
      calls.push(call);
      return fetchResponse(call, calls.length);
    },
  };
  vm.createContext(context);
  // 运行完整传输实现及实际 api 消费器；只替换浏览器/网络外部依赖。
  vm.runInContext(channels.replace(/^import .*;\r?\n/gm, "").replace(/^export /gm, ""), context);
  const apiFrom = app.indexOf("async function api("), apiTo = app.indexOf("function promoteReadPriority(", apiFrom);
  assert.ok(apiFrom >= 0 && apiTo > apiFrom);
  vm.runInContext(app.slice(apiFrom, apiTo), context);
  return { context, calls };
}

const jsonResponse = (value, status = 200, headers = {}) => new Response(JSON.stringify(value), {
  status, headers: { "Content-Type": "application/json; charset=utf-8", ...headers },
});

test("单项原生读取返回业务503后，主会话状态读取和单次发送仍能使用同一线路", async () => {
  const { context, calls } = transportClient(({ path, options }) => {
    if (path === "/api/read/relatedThreads")
      return jsonResponse({ ok: false, message: "侧边会话暂不可用" }, 503);
    if (path === "/api/session") return jsonResponse({ ok: true, serviceConnected: true });
    return jsonResponse({ requestId: options.headers.get("Idempotency-Key"),
      state: "completed", result: { ok: true, threadId: "owned" } });
  });
  await assert.rejects(context.api("/api/read/relatedThreads", { threadId: "owned" }),
    error => error.code === 503 && error.message === "侧边会话暂不可用");
  assert.equal((await context.api("/api/session")).serviceConnected, true);
  assert.equal((await context.api("/api/action/send", { threadId: "owned", prompt: "测试" }, "only-once")).state,
    "completed");
  assert.deepEqual(calls.map(call => call.path), ["/api/read/relatedThreads", "/api/session", "/api/action/send"]);
});

test("迟到的业务503保持中性，不能清除并发服务状态失败建立的退避", async () => {
  let replySession, replyFeature;
  const { context, calls } = transportClient(({ path, options }) => {
    if (path === "/api/session") return new Promise(resolve => { replySession = resolve; });
    if (path === "/api/read/relatedThreads") return new Promise(resolve => { replyFeature = resolve; });
    return jsonResponse({ requestId: options.headers.get("Idempotency-Key"),
      state: "completed", result: { ok: true } });
  });
  const sessionRead = context.api("/api/session").catch(error => error);
  const featureRead = context.api("/api/read/relatedThreads", { threadId: "owned" }).catch(error => error);
  replySession(jsonResponse({ ok: false, message: "服务暂不可用" }, 503));
  assert.equal((await sessionRead).code, 503);
  await assert.rejects(context.api("/api/action/send", { threadId: "owned", prompt: "测试" }, "before-feature"),
    error => error.notSubmitted === true);
  replyFeature(jsonResponse({ ok: false, message: "侧边会话暂不可用" }, 503));
  assert.equal((await featureRead).code, 503);
  await assert.rejects(context.api("/api/action/send", { threadId: "owned", prompt: "测试" }, "after-feature"),
    error => error.notSubmitted === true);
  assert.deepEqual(calls.map(call => call.path), ["/api/session", "/api/read/relatedThreads"]);
});

test("网关503、非业务JSON、429及Retry-After仍阻止冷却期内的新请求", async () => {
  for (const response of [
    () => new Response("<html>gateway unavailable</html>", { status: 503 }),
    () => jsonResponse({ message: "unknown gateway" }, 503),
    () => jsonResponse({ ok: false, message: "限流" }, 429, { "Retry-After": "60" }),
    () => jsonResponse({ ok: false, message: "稍后重试" }, 503, { "Retry-After": "30" }),
  ]) {
    const { context, calls } = transportClient(response);
    await assert.rejects(context.api("/api/read/relatedThreads", { threadId: "owned" }));
    await assert.rejects(context.api("/api/action/send", { threadId: "owned", prompt: "测试" }, "only-once"),
      error => error.notSubmitted === true);
    assert.equal(calls.length, 1, "传输退避期间不得偷偷发送");
  }
});

test("整个服务状态读取失败与破损回包仍按线路失败处理", async () => {
  for (const response of [
    () => jsonResponse({ ok: false, message: "服务已停止" }, 503),
    () => new Response("{broken", { status: 200, headers: { "Content-Type": "application/json" } }),
  ]) {
    const { context, calls } = transportClient(response);
    await assert.rejects(context.api("/api/session"));
    await assert.rejects(context.api("/api/read/thread", { threadId: "owned" }),
      error => error.notSubmitted === true);
    assert.equal(calls.length, 1);
  }
});

test("未知写回执仍保持未知并且只派发一次，不因业务错误隔离而重放", async () => {
  const { context, calls } = transportClient(() =>
    jsonResponse({ ok: false, message: "结果待确认", outcomeUnknown: true }, 503));
  await assert.rejects(context.api("/api/action/send", { threadId: "owned", prompt: "测试" }, "only-once"),
    error => error.code === 503 && error.outcomeUnknown === true);
  assert.equal(calls.length, 1);
});

const from = app.indexOf("function restoreSidebarPreferences()");
const to = app.indexOf("async function changeSidebar(payload)", from);
assert.ok(from >= 0 && to > from);

function client(disk, extra = {}) {
  const context = {
    session: { device: { id: "device-a", createdAtMs: 1 }, readOnly: false },
    sidebar: { preferences: { mode: "list" } },
    sidebarFilters: { mode: null, chats: null, projects: null, pinned: null },
    sidebarPreferenceScope: null,
    sidebarPreferenceStore: createSidebarPreferenceStore({ storage: disk }),
    privateInputScope: value => value?.device
      ? JSON.stringify([value.device.id, value.device.createdAtMs]) : null,
    rendered: [], notices: [], bridgeCalls: 0, connected: false,
    mutate: () => { context.bridgeCalls++; throw new Error("CDP 与 CLI 均不可用"); },
    notice: message => context.notices.push(message),
    renderSidebar: () => context.rendered.push(context.sidebarFilters.mode),
    ...extra,
  };
  vm.createContext(context);
  vm.runInContext(app.slice(from, to), context);
  return context;
}

test("CDP、CLI 不可用且连接断开时，实际切换函数仍持久保存并刷新恢复", () => {
  const disk = storage(), current = client(disk);
  current.changeSidebarView("project");
  assert.equal(current.sidebarFilters.mode, "project");
  assert.equal(current.bridgeCalls, 0);
  const reopened = client(disk);
  reopened.restoreSidebarPreferences();
  assert.equal(reopened.sidebarFilters.mode, "project");
  assert.equal(reopened.bridgeCalls, 0);
});

test("只读授权允许修改本客户端视图，不调用任何会话写入", () => {
  const current = client(storage());
  current.session.readOnly = true;
  current.changeSidebarView("project");
  current.changeSidebarView("list");
  assert.equal(current.sidebarFilters.mode, "list");
  assert.equal(current.bridgeCalls, 0);
  assert.equal(current.notices.length, 0);
});

test("快速切换以最后一次选择为准，服务器偏好刷新不覆盖本地值", () => {
  const current = client(storage());
  for (const mode of ["project", "list", "project"]) current.changeSidebarView(mode);
  current.sidebar.preferences.mode = "list";
  current.restoreSidebarPreferences();
  assert.equal(current.sidebarFilters.mode, "project");
  assert.equal(current.sidebarPreferenceStore.read('["device-a",1]').mode, "project");
  assert.equal(current.bridgeCalls, 0);
});

test("本地保存失败保留本页选择并明确提示，不能报成 CDP 故障或撤销切换", () => {
  const disk = storage();
  disk.setItem = () => { throw Object.assign(new Error("quota"), { name: "QuotaExceededError" }); };
  const current = client(disk);
  current.changeSidebarView("project");
  assert.equal(current.sidebarFilters.mode, "project");
  assert.equal(current.rendered.at(-1), "project");
  assert.equal(current.notices.length, 1);
  assert.match(current.notices[0], /本地保存失败/);
  assert.equal(current.bridgeCalls, 0);
});

test("更换设备授权不继承旧偏好，旧设备重新打开仍能恢复", () => {
  const current = client(storage());
  current.changeSidebarView("project");
  current.session = { device: { id: "device-b", createdAtMs: 1 } };
  current.restoreSidebarPreferences();
  assert.equal(current.sidebarFilters.mode, null);
  current.changeSidebarView("list");
  current.session = { device: { id: "device-a", createdAtMs: 1 } };
  current.restoreSidebarPreferences();
  assert.equal(current.sidebarFilters.mode, "project");
});

function sourceSection(start, end) {
  const from = app.indexOf(start), to = app.indexOf(end, from);
  assert.ok(from >= 0 && to > from);
  return app.slice(from, to);
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function queueReloadHarness() {
  const timers = new Map(), threadReads = [], historyReads = [], notices = [];
  let timerId = 0;
  const nodes = new Map([
    ["#model-settings", { hidden: true }], ["#thread-title", {}], ["#thread-path", {}],
  ]);
  const context = {
    AbortController, performance, pageLeaving: false, document: { hidden: false },
    selectedId: "main", selectionVersion: 1, foregroundReadGeneration: 0,
    thread: { id: "main", queuedMessages: [{ id: "queued-a", text: "引导消息" }] },
    threadRequest: null, threadReadVersion: 0,
    tokenUsageVersion: 0, questionVersion: 0, statusVersion: 0, goalVersion: 0, compactionVersion: 0,
    queueVersion: 0, queueSnapshotVersion: null, retiredQueueEpochs: new Set(),
    notifiedQueue: null, notifiedQueueEdits: null, pendingSteer: null,
    connected: true, queuedActionBusy: new Set(), pending: null, busy: false, stopping: false,
    session: { device: { id: "device-a", createdAtMs: 1 } }, recoveryGeneration: 0,
    privateInputScope: value => JSON.stringify([value?.device?.id, value?.device?.createdAtMs]),
    threadDeleted: () => false, threadReadOnly: () => false, compactionLocked: () => false,
    blockingRequestState: () => ({ generation: 0 }),
    queueMessageText: message => message?.text || "", $: selector => nodes.get(selector),
    setTimeout(callback, delay) { const id = ++timerId; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); }, refreshTimer: null,
    notice: message => notices.push(message),
    recordDiagnostic() {}, renderMessages() {}, renderQueueArea() {},
    rememberThreadDisplay() {}, applyRelatedThreadMetadata() {}, mergeBlockingRequestsRead() {},
    replaceTranscriptEvents() {}, refreshTranscriptEvents() {}, syncEditableMessageMetadata() {},
    reconcileCompaction() {}, syncThreadHash() {}, updateComposer() {}, renderApprovals() {},
    loadHistory: async () => { historyReads.push(context.selectedId); },
    api: async path => {
      assert.equal(path, "/api/read/thread");
      const read = deferred(); threadReads.push(read); return read.promise;
    },
    mutate: async (action, payload, options) => {
      assert.equal(action, "steerQueued");
      assert.equal(payload.threadId, "main");
      assert.equal(payload.messageId, "queued-a");
      assert.equal(options.reliable, true);
      return { threadId: "main", turnId: "active-turn", queuedMessages: [] };
    },
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function queueSnapshotIsVersioned(", "async function reloadQueue("),
    sourceSection("async function steerQueuedMessage(", "async function beginQueuedMessageEdit("),
    sourceSection("async function reloadThread()", "const goalStatusLabels"),
    sourceSection("function scheduleReload()", "function compactionEventMatches("),
  ].join("\n"), context);
  const runTimers = () => {
    for (const [id, timer] of [...timers]) {
      timers.delete(id);
      timer.callback();
    }
  };
  const completeRead = index => threadReads[index].resolve({
    thread: { id: "main", title: "会话", cwd: "", queuedMessages: context.thread.queuedMessages },
  });
  return { context, timers, threadReads, historyReads, notices, runTimers, completeRead };
}

test("引导成功后的主动读取合并队列移除安排的刷新，只读取一次会话和历史", async () => {
  const { context, timers, threadReads, historyReads, notices, runTimers, completeRead } = queueReloadHarness();
  await context.steerQueuedMessage("main", "queued-a");
  assert.equal(context.thread.queuedMessages.length, 0);
  assert.equal(threadReads.length, 1);
  assert.equal(historyReads.length, 1);
  assert.equal(timers.size, 0);
  assert.equal(context.refreshTimer, null);
  runTimers();
  const request = context.threadRequest;
  assert.equal(request.again, false);
  completeRead(0);
  await request.promise;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(threadReads.length, 1);
  assert.equal(historyReads.length, 1);
  assert.equal(notices.length, 0);
});

test("主动读取开始后的新刷新事件仍触发下一次读取，不丢失后续更新", async () => {
  const { context, timers, threadReads, historyReads, runTimers, completeRead } = queueReloadHarness();
  context.scheduleReload();
  const read = context.reloadThread();
  context.scheduleReload();
  assert.equal(timers.size, 1, "主动读取只合并开始前已排队的刷新");
  runTimers();
  assert.equal(context.threadRequest.again, true);
  completeRead(0);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(threadReads.length, 2);
  assert.equal(historyReads.length, 2);
  completeRead(1);
  await read;
  assert.equal(context.threadRequest, null);
});

function queuedReceiptHarness({ disk = storage(), refresh = async () => {} } = {}) {
  disk.removeItem = key => disk.records.delete(key);
  const requests = [], notices = [], refreshes = [], timers = new Map(), clearedDrafts = [];
  const draft = { text: "操作期间保留的普通草稿", skills: [{ name: "skill-a", path: "/skill-a" }],
    attachments: [{ kind: "inline", dataUrl: "data:image/png;base64,AQID" }] };
  const area = { children: [], append(node) { this.children.push(node); } };
  let sequence = 0;
  const context = {
    performance, sessionStorage: disk, session: { device: { id: "device-a", createdAtMs: 1 }, capabilities: {} },
    selectedId: "main", thread: { id: "main", queuedMessages: [{ id: "queued-a", text: "原队列正文" }] },
    selectionVersion: 1, selectionToken: "selected-a", recoveryGeneration: 0, recoveryScope: null,
    connected: true, busy: false, stopping: false, pending: null, pendingTimer: null, sending: null,
    queuedActionBusy: new Set(), queuedEditActions: new Set(["beginQueuedEdit", "editQueued", "cancelQueuedEdit"]),
    pendingSteer: null, queueVersion: 0, goalVersion: 0, submittedDrafts: new Map(),
    goalComposerRecoveries: new Map(), editingMessage: null,
    privateInputScope: value => JSON.stringify([value?.device?.id, value?.device?.createdAtMs]),
    goalComposerAction: () => false, goalComposerStorageRecord: value => value,
    hydrateGoalComposerRecord: value => value, validSkillSelection: () => true,
    threadReadOnly: () => false, compactionLocked: () => false, isActiveThread: () => true,
    randomId: () => `queue-receipt-${++sequence}`, snapshotDraft: () => draft,
    recoveryStorageMessage: error => error.message,
    recoveryStore: { activate: async () => {}, put: async () => {}, remove: async () => {} },
    clearSubmittedDraft: (...args) => { clearedDrafts.push(args); draft.text = ""; },
    setTimeout(callback, delay) { const id = ++sequence; timers.set(id, { callback, delay }); return id; },
    clearTimeout: id => timers.delete(id),
    api: (path, payload, requestId) => {
      const response = deferred(); requests.push({ path, payload, requestId, ...response }); return response.promise;
    },
    queueMessageText: message => message?.text || "",
    queueSnapshotIsVersioned: value => Number.isInteger(value?.queueRevision),
    applyQueueSnapshot: (id, messages) => {
      if (id !== context.selectedId) return false;
      context.thread.queuedMessages = messages; ++context.queueVersion; return true;
    },
    reloadThread: () => { refreshes.push("thread"); return refresh(); },
    reloadQueue: () => { refreshes.push("queue"); return refresh(); },
    notice: (message, error) => notices.push({ message, error }),
    updateComposer() {}, renderMessages() {}, renderQueueArea() {}, recordDiagnostic() {},
    $: () => area,
    element: (_tag, _className, text) => ({ text, children: [], append(node) { this.children.push(node); } }),
    button: (label, aria, action) => ({ label, aria, action }),
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("const pendingFeedbackDelayMs =", "function clearPendingFeedbackTimer("),
    sourceSection("function savePending(", "function updateGoalComposerPending("),
    sourceSection("function ordinaryInputRecord(", "function rememberDraft("),
    sourceSection("async function mutate(", "function acceptedSendItem("),
    sourceSection("async function resolvePending()", "let loginPageCleanup"),
    sourceSection("async function removeQueuedMessage(", "async function beginQueuedMessageEdit("),
    sourceSection("async function stopGeneration()", "async function applyCompactionReceipt("),
    `function renderPending(){${sourceSection("  if (pending && (!sending || pending.payload !== sending.payload)",
      "  renderFailedInputRecovery(")}}`,
  ].join("\n"), context);
  context.pending = vm.runInContext(`(()=>{${sourceSection("let pending = null;", "const goalComposerRecoveries")}
    return pending;})()`, context);
  const receipt = (index, state, result = {}) => requests[index].resolve({
    requestId: requests[index].requestId || context.pending?.id, state, result,
  });
  const completed = index => receipt(index, "completed", {
    ok: true, threadId: "main", messageId: "queued-a", turnId: "active-turn",
    queueEpoch: "current", queueRevision: 2, queuedMessages: [],
  });
  const invoke = action => action === "steerQueued"
    ? context.steerQueuedMessage("main", "queued-a") : context.removeQueuedMessage("main", "queued-a");
  return { context, requests, notices, refreshes, timers, draft, area, disk, clearedDrafts, receipt, completed, invoke };
}

test("队列引导和删除unknown保留原回执，刷新只读恢复且不清普通草稿", async () => {
  for (const action of ["steerQueued", "removeQueued"]) {
    const first = queuedReceiptHarness(), original = structuredClone(first.draft);
    const operation = first.invoke(action);
    assert.equal(first.requests.length, 1);
    const id = first.requests[0].requestId;
    assert.equal(first.context.pending.id, id);
    first.receipt(0, "unknown", { ok: false, outcomeUnknown: true });
    await operation;
    assert.equal(first.notices.length, 0, "快速返回未确认时不得抢先弹出错误提示");
    assert.equal(first.requests[1].path, `/api/receipts/${id}`);
    await first.invoke(action);
    await first.invoke(action === "steerQueued" ? "removeQueued" : "steerQueued");
    assert.equal(first.requests.length, 2, "未知期间不能再次引导、删除或普通发送补偿");
    first.receipt(1, "unknown", { ok: false, outcomeUnknown: true });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(first.context.pending.id, id);
    first.context.renderPending();
    assert.equal(first.area.children.length, 0, "正常等待的前五秒不额外显示回执横幅");
    first.context.pending.time -= 5000;
    first.context.renderPending();
    const banner = first.area.children[0];
    assert.match(banner.text, /正在确认队列.*结果/);
    assert.deepEqual(banner.children.map(node => node.label), ["查回执"]);
    assert.match(banner.children[0].aria, /只查询原队列操作回执/);
    assert.deepEqual(first.draft, original);

    const reopened = queuedReceiptHarness({ disk: first.disk });
    assert.equal(reopened.context.pending.id, id, "刷新必须接受这两个既有动作的原记录");
    assert.equal(reopened.context.pending.invalid, undefined);
    reopened.draft.text = "刷新后继续编辑的新普通稿";
    const retained = structuredClone(reopened.draft);
    const recovery = reopened.context.resolvePending();
    assert.equal(reopened.requests.length, 1);
    assert.equal(reopened.requests[0].path, `/api/receipts/${id}`);
    reopened.completed(0);
    await recovery;
    assert.equal(reopened.context.pending, null);
    assert.equal(reopened.disk.getItem("codex-elves-pending"), null);
    assert.deepEqual(reopened.draft, retained);
    assert.deepEqual(reopened.clearedDrafts, []);
    assert.equal(reopened.requests.filter(request => request.path.startsWith("/api/action/")).length, 0);
    assert.deepEqual(reopened.refreshes, [action === "steerQueued" ? "thread" : "queue"]);
  }
});

test("队列写回包中断仍只查同一原回执，明确失败解锁且保留普通草稿", async () => {
  for (const action of ["steerQueued", "removeQueued"]) {
    const fixture = queuedReceiptHarness(), original = structuredClone(fixture.draft);
    const operation = fixture.invoke(action), id = fixture.requests[0].requestId;
    fixture.requests[0].reject(new TypeError("response body interrupted"));
    await operation;
    assert.equal(fixture.notices.length, 0, "传输中断不等于原生操作失败，等待原回执期间不立即弹红提示");
    assert.equal(fixture.context.pending.id, id);
    assert.equal(fixture.requests[1].path, `/api/receipts/${id}`);
    fixture.receipt(1, "failed", { ok: false, code: 409, message: "原队列已过期" });
    await new Promise(resolve => setImmediate(resolve));
    assert.ok(fixture.notices.some(item => item.error === true && item.message === "原队列已过期"));
    assert.equal(fixture.context.pending, null);
    assert.deepEqual(fixture.draft, original);
    assert.deepEqual(fixture.clearedDrafts, []);
    assert.equal(fixture.requests.filter(request => request.path.startsWith("/api/action/")).length, 1);
    assert.deepEqual(fixture.refreshes, []);
  }
});

test("队列原回执404或结果身份不符继续保锁，不重复写入", async () => {
  for (const action of ["steerQueued", "removeQueued"]) {
    const fixture = queuedReceiptHarness(), operation = fixture.invoke(action);
    const id = fixture.requests[0].requestId;
    fixture.receipt(0, "unknown", { ok: false, outcomeUnknown: true });
    await operation;
    fixture.requests[1].reject(Object.assign(new Error("original receipt missing"), { code: 404 }));
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(fixture.context.pending.id, id);
    const check = fixture.context.resolvePending();
    fixture.receipt(2, "completed", { ok: true, threadId: "other-thread", messageId: "queued-a" });
    await check;
    assert.equal(fixture.context.pending.id, id);
    await fixture.invoke(action);
    assert.equal(fixture.requests.filter(request => request.path.startsWith("/api/action/")).length, 1);
    assert.deepEqual(fixture.clearedDrafts, []);
  }
});

test("正常可靠引导不等待后续会话读取，也不额外查询原回执", async () => {
  const read = deferred(), fixture = queuedReceiptHarness({ refresh: () => read.promise });
  const operation = fixture.invoke("steerQueued"), original = structuredClone(fixture.draft);
  fixture.completed(0);
  await operation;
  assert.equal(fixture.context.pending, null);
  assert.deepEqual(fixture.refreshes, ["thread"]);
  assert.equal(fixture.requests.length, 1, "正常成功不串行追加回执或队列读取");
  assert.deepEqual(fixture.draft, original);
  read.resolve();
});

test("旧授权的可靠请求错误不显示旧提示，不查询回执或改新pending", async () => {
  for (const action of ["send", "steerQueued", "removeQueued"]) {
    for (const change of ["device", "created-at", "generation"]) {
      const fixture = queuedReceiptHarness(), original = structuredClone(fixture.draft);
      const operation = fixture.context.mutate(action,
        { threadId: "main", messageId: "queued-a", prompt: "原提交", skills: [] }, { reliable: true }).catch(error => error);
      await new Promise(resolve => setImmediate(resolve));
      assert.equal(fixture.requests.length, 1);
      if (change === "generation") ++fixture.context.recoveryGeneration;
      else fixture.context.session = { ...fixture.context.session, device: {
        id: change === "device" ? "device-b" : "device-a", createdAtMs: change === "created-at" ? 2 : 1,
      } };
      const newPending = { id: "new-original-receipt", action: "send" };
      fixture.context.pending = newPending;
      fixture.requests[0].reject(Object.assign(new Error("old response failed"), { code: 503 }));
      await operation;
      assert.equal(fixture.context.pending, newPending);
      assert.deepEqual(fixture.notices, []);
      assert.equal(fixture.requests.length, 1);
      assert.deepEqual(fixture.refreshes, []);
      assert.deepEqual(fixture.draft, original);
    }
  }
});

test("停止回执cleanupPending仅提示清理尚未确认，正常清理仍显示原成功提示", async () => {
  for (const cleanupPending of [true, false]) {
    const fixture = queuedReceiptHarness(), operation = fixture.context.stopGeneration();
    assert.equal(fixture.requests[0].path, "/api/action/stop");
    fixture.receipt(0, "completed", { ok: true, threadId: "main", cleanupPending });
    await operation;
    assert.equal(fixture.notices.length, 1);
    assert.match(fixture.notices[0].message, cleanupPending ? /后台终端清理尚未确认/ : /已请求停止并清理/);
    assert.equal(fixture.context.stopping, false);
    assert.equal(fixture.requests.length, 1);
  }
});

function sidebarReadHarness() {
  const reads = [], notices = [];
  const context = {
    AbortController, pageLeaving: false, document: { hidden: false, activeElement: {} },
    foregroundReadGeneration: 0, sidebarVersion: 0, sidebarRequest: null,
    session: { device: { id: "device-a" }, readOnly: false },
    sidebarFilterDeviceId: "device-a", sidebar: null, selectedId: null,
    sidebarBusy: false, connected: true, sidebarReadRefreshSuppressions: new Set(),
    promoteReadPriority: () => {}, filterDeletedSidebar: () => {}, filterSideThreadSidebar: () => {},
    cacheDisplayData: () => {}, sidebarSnapshot: value => value, renderSidebar: () => {},
    threadDeletionLocked: () => false, threadDeleted: () => false,
    $: () => ({ contains: () => false }),
    notice: message => notices.push(message),
    mutate: async () => ({ ok: true }),
    api: async (path, payload) => {
      assert.equal(path, "/api/read/sidebar");
      const request = { ...deferred(), payload };
      reads.push(request);
      return request.promise;
    },
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("async function loadSidebar(", "function sidebarUpdatedAt("),
    sourceSection("async function changeSidebar(", "function confirmAction("),
  ].join("\n"), context);
  const snapshot = pinned => ({
    recent: pinned ? [] : [{ id: "main", title: "主会话" }],
    pinnedThreads: pinned ? [{ id: "main", title: "主会话" }] : [],
    projects: [], nextCursor: null,
  });
  return { context, reads, notices, snapshot };
}

test("并发侧栏普通读取共享原请求，不在完成后重复读取", async () => {
  const { context, reads, snapshot } = sidebarReadHarness();
  const first = context.loadSidebar();
  const second = context.loadSidebar();
  const poll = context.loadSidebar(null, { resourcePriority: "background" });
  assert.equal(reads.length, 1);
  reads[0].resolve(snapshot(false));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 1);
  await Promise.all([first, second, poll]);
  assert.equal(context.sidebar.recent[0].id, "main");
});

test("置顶完成时已有旧侧栏读取，必须补读并显示新的置顶结果", async () => {
  const { context, reads, snapshot, notices } = sidebarReadHarness();
  const oldRead = context.loadSidebar();
  const pin = context.changeSidebar({ threadId: "main", pinned: true });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 1);
  reads[0].resolve(snapshot(false));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 2);
  reads[1].resolve(snapshot(true));
  await Promise.all([oldRead, pin]);
  assert.equal(context.sidebar.pinnedThreads[0].id, "main");
  assert.equal(context.sidebar.recent.length, 0);
  assert.equal(context.sidebarBusy, false);
  assert.deepEqual(notices, []);
});

test("侧栏旧请求失败后仍完成已要求的新读取，普通并发读不增加第三次请求", async () => {
  const { context, reads, snapshot } = sidebarReadHarness();
  const first = context.loadSidebar();
  const fresh = context.loadSidebar(null, { fresh: true });
  reads[0].reject(new Error("旧读取失败"));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 2);
  const poll = context.loadSidebar();
  reads[1].resolve(snapshot(true));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 2);
  await Promise.all([first, fresh, poll]);
  assert.equal(context.sidebar.pinnedThreads[0].id, "main");
});

function lifecycle() {
  const reads = [], refreshes = [];
  const pendingWrite = new AbortController();
  const context = {
    AbortController, DOMException, clearTimeout,
    clearPendingFeedbackTimer() {},
    session: { device: { id: "device-a" } },
    selectedId: "thread-a", selectionVersion: 1, threadReadVersion: 0,
    foregroundReadGeneration: 0, pageLeaving: false, document: { hidden: false },
    historyRequest: null, threadRequest: null, refreshRequest: null, sidebarRequest: null,
    selectionController: null, historyController: null,
    historyLoading: false, historyLoadingTurnId: null, historyLoadingOlder: false,
    refreshTimer: null, sidebarTimer: null, transcriptChecking: false,
    threadDeleted: () => false, syncHistoryButtons: () => {},
    pending: { id: "original-write", state: "unknown", controller: pendingWrite },
    drafts: new Map([["thread-a", "保留原草稿"]]),
    turns: new Map([["turn-a", { id: "turn-a", text: "保留已显示内容" }]]),
    readHistoryPage: async (_older, _version, _items, generation) => {
      const job = { ...deferred(), generation, controller: new AbortController() };
      reads.push(job); context.historyController = job.controller;
      await job.promise;
    },
    loadSidebar: async () => {},
    reloadThread: async () => {
      const job = deferred(); refreshes.push(job); await job.promise;
    },
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("async function loadHistory(", "async function readHistoryPage("),
    sourceSection("async function refreshAll(", "function openStream("),
    sourceSection("function retireForegroundReads(", "function suspendPage("),
  ].join("\n"), context);
  return { context, reads, refreshes, pendingWrite };
}

test("返回前台必须启动新历史读取，不复用后台旧 promise；旧完成不清理新请求", async () => {
  const { context, reads } = lifecycle();
  const oldRead = context.loadHistory();
  await new Promise(resolve => setImmediate(resolve));
  const oldRequest = context.historyRequest;
  oldRequest.latestPending = true;
  context.document.hidden = true;
  context.retireForegroundReads();
  assert.equal(reads[0].controller.signal.aborted, true);
  context.document.hidden = false;
  const newRead = context.loadHistory();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 2);
  assert.notEqual(reads[0].generation, reads[1].generation);
  const newRequest = context.historyRequest;
  reads[0].resolve();
  await oldRead;
  assert.equal(context.historyRequest, newRequest);
  assert.equal(reads.length, 2, "后台旧请求的 latestPending 不得派生第三次读取");
  reads[1].resolve();
  await newRead;
  assert.equal(context.historyRequest, null);
});

test("后台刷新任务退役后不阻挡前台刷新，也不覆盖新刷新所有权", async () => {
  const { context, refreshes } = lifecycle();
  const oldRefresh = context.refreshAll();
  context.refreshRequest.again = true;
  context.retireForegroundReads();
  const newRefresh = context.refreshAll({ fresh: true });
  const newRequest = context.refreshRequest;
  assert.equal(refreshes.length, 2);
  refreshes[0].resolve();
  await oldRefresh;
  assert.equal(context.refreshRequest, newRequest);
  assert.equal(refreshes.length, 2);
  refreshes[1].resolve();
  await newRefresh;
});

test("后台只取消显示读取，保留原写请求、未知回执、草稿和已显示内容", () => {
  const { context, pendingWrite } = lifecycle();
  const selection = new AbortController(), sidebar = new AbortController(), thread = new AbortController();
  context.selectionController = selection;
  context.sidebarRequest = { controller: sidebar };
  context.threadRequest = { controller: thread };
  const pending = context.pending, drafts = context.drafts, turns = context.turns;
  context.retireForegroundReads();
  assert.equal(selection.signal.aborted, true);
  assert.equal(sidebar.signal.aborted, true);
  assert.equal(thread.signal.aborted, true);
  assert.equal(pendingWrite.signal.aborted, false);
  assert.equal(context.pending, pending);
  assert.equal(context.pending.state, "unknown");
  assert.equal(context.drafts, drafts);
  assert.equal(context.turns, turns);
  assert.equal(context.drafts.get("thread-a"), "保留原草稿");
});

test("后台事件不会重建历史请求或整页刷新；前台后可以正常发起", async () => {
  const { context, reads, refreshes } = lifecycle();
  context.document.hidden = true;
  await context.loadHistory();
  await context.refreshAll({ fresh: true });
  assert.equal(reads.length, 0);
  assert.equal(refreshes.length, 0);
  context.document.hidden = false;
  const read = context.loadHistory();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(reads.length, 1);
  reads[0].resolve();
  await read;
});

// 摘自 119 的 closed-test-probe.json；测试不依赖本机 dist 取证目录。
const expiredSideProbe = {
  threadId: "01a19a9a-0435-70b1-b979-e041ed4e25ed",
  thread: { code: -32600, message: "thread not loaded: 01a19a9a-0435-70b1-b979-e041ed4e25ed" },
};

function sideExpiryHarness(response) {
  const { context, calls } = transportClient(response), scheduled = new Map(), menuChanges = [], notices = [];
  const node = () => ({ hidden: false, attributes: {}, classList: { toggle() {}, remove() {} },
    setAttribute(name, value) { this.attributes[name] = value; } });
  const nodes = new Map([".workspace", "#composer", "#side-thread-expired", "#open-sidebar", "#connection-state",
    "#open-session-tools", "#related-session-back", "#pending-area"].map(selector => [selector, node()]));
  const id = expiredSideProbe.threadId, parent = "test-parent";
  const device = { id: "side-device", createdAtMs: 1 }, scope = JSON.stringify([device.id, device.createdAtMs]);
  const view = { scope, threadId: id, parentThreadId: parent, kind: "side", name: "已保留的侧边会话" };
  Object.assign(context, {
    session: { device, readOnly: false }, selectedId: id, selectionVersion: 1,
    relatedSessionView: view, relatedThreadViews: new Map([[id, view]]), relatedNavigationOwner: "local-page",
    thread: { id, title: "已保留的侧边会话", viewKind: "side", parentThreadId: parent, ephemeral: true },
    turns: new Map([["saved-turn", { id: "saved-turn", items: [{ type: "agentMessage", text: "保留已有消息" }] }]]),
    drafts: new Map([[id, { text: "保留未发送草稿", skills: [{ name: "chosen", path: "/chosen/SKILL.md" }],
      attachments: [{ name: "kept.png", dataUrl: "data:image/png;base64,a2VlcA==" }] }]]),
    historyInitialized: true, transcriptChecking: true, historyError: "", recentHistoryPage: null,
    creating: false, relatedBackPending: false, connected: true, sidebar: { unreadSideThreads: [{ id }] },
    sessionMenu: { markSideThreadRead: value => menuChanges.push(["read", value]),
      removeThread: value => menuChanges.push(["remove", value]), updateContext() {}, reposition() {} },
    syncRelatedModelDisplay() {}, syncRelatedSidebarLayout: null, markSidebarThreadReadLocally() {},
    $: selector => nodes.get(selector), renderMessages() {}, renderSidebar() {},
    resetRelatedSessions() { assert.fail("缺失识别不得清除当前侧边关系"); },
    showWelcome() { assert.fail("侧边过期或暂未加载不得跳到欢迎页"); },
    retryConnectionRecovery() { assert.fail("单个会话错误不得触发全局连接恢复"); },
    resetMessages() { assert.fail("暂未加载不得丢弃已显示消息"); },
    notice: message => notices.push(message), readScope: null,
    relatedSessionCheckTimer: null, refreshTimer: null, foregroundReadGeneration: 0,
    threadRequest: null, threadReadVersion: 0, tokenUsageVersion: 0, questionVersion: 0, statusVersion: 0,
    goalVersion: 0, compactionVersion: 0, queueVersion: 0, blockingRequestState: () => ({ generation: 0 }),
    loadHistory: async () => {}, threadDeleted: () => false,
    historyLoading: false, historyLoadingVersion: 0, historyLoadingTurnId: null, historyLoadingOlder: false,
    historyAutoFailed: false, historyCursor: null, historyController: null, messageVersion: 0,
    recoveryErrorSilenceDepth: 0, syncHistoryButtons() {},
    retireNativeSideThreads() { assert.fail("读取失败不得伪造原生关闭事件"); },
    setTimeout(callback, delay) {
      if (delay !== 10000) return setTimeout(callback, delay);
      const token = {}; scheduled.set(token, callback); return token;
    },
    clearTimeout(token) { if (!scheduled.delete(token)) clearTimeout(token); },
  });
  vm.runInContext([
    sourceSection("function privateInputScope(", "function threadSentMessageEditAllowed("),
    sourceSection("function syncRelatedSessionLayout()", "function finishRelatedSessionExit("),
    sourceSection("function scheduleRelatedSessionCheck()", "function resetRelatedSessions("),
    sourceSection("function threadEphemeralState(", "function persistentThreadDisplayAllowed("),
    sourceSection("async function reloadThread()", "const goalStatusLabels"),
    sourceSection("async function readHistoryPage(", "function pruneCachedHistory("),
  ].join("\n"), context);
  context.syncRelatedSessionLayout();
  const retained = { turns: context.turns, turnsJson: JSON.stringify([...context.turns]),
    drafts: context.drafts, draftsJson: JSON.stringify([...context.drafts]) };
  const assertRetained = () => {
    assert.equal(context.turns, retained.turns);
    assert.equal(JSON.stringify([...context.turns]), retained.turnsJson);
    assert.equal(context.drafts, retained.drafts);
    assert.equal(JSON.stringify([...context.drafts]), retained.draftsJson);
    assert.equal(context.connected, true);
    assert.equal(calls.some(call => call.path.startsWith("/api/action/")), false);
  };
  return { context, calls, nodes, id, parent, menuChanges, notices, assertRetained,
    runCheck: async () => {
      const [token, callback] = [...scheduled][0];
      scheduled.delete(token);
      await callback();
    } };
}

test("准确侧边的中文404和原始119缺失证据进入已有过期态，隐藏输入框并保留消息草稿", async () => {
  for (const message of ["会话已关闭或不存在", "原生临时侧边会话已关闭或不存在", expiredSideProbe.thread.message]) {
    for (const endpoint of ["thread", "history"]) {
      const fixture = sideExpiryHarness(() => jsonResponse({ ok: false, message },
        message === expiredSideProbe.thread.message ? 503 : 404));
      const { context, nodes, id, assertRetained, calls } = fixture;
      if (endpoint === "thread") await context.reloadThread();
      else await context.readHistoryPage(false, context.selectionVersion);
      assert.equal(context.sideThreadExpired(id), true, `${endpoint}: ${message}`);
      assert.equal(nodes.get("#composer").hidden, true);
      assert.equal(nodes.get("#side-thread-expired").hidden, false);
      assert.equal(context.thread.readOnly, true);
      assert.equal(context.historyError, "");
      assert.equal(context.historyInitialized, true);
      assert.equal(context.selectedId, id);
      assert.equal(calls.length, 1, "只读取原请求，不重试、不重发");
      assertRetained();
    }
  }
});

test("缺失错误归属原请求ID和授权，网络503、权限403、他人ID或无缺失原因均不判过期", async () => {
  const fixture = sideExpiryHarness(() => jsonResponse({ ok: false, message: "会话已关闭或不存在" }, 404));
  const { context, id, nodes } = fixture;
  const captured = await context.api("/api/read/thread", { threadId: id }).catch(error => error);
  assert.equal(captured.readThreadId, id);
  assert.equal(captured.readThreadScope, context.privateInputScope(context.session));
  assert.equal(context.isMissingThreadError(captured, id), true);
  for (const error of [
    Object.assign(new Error("会话已关闭或不存在"), { code: 503, readThreadId: id }),
    Object.assign(new Error("thread not loaded: other-side"), { code: 503, readThreadId: id }),
    Object.assign(new Error(`thread not loaded: ${id}`), { code: 403, readThreadId: id }),
    Object.assign(new Error("thread not loaded: other-side"), { code: 404, readThreadId: id }),
    Object.assign(new Error("会话已关闭或不存在"), { code: 404, readThreadId: "other-side" }),
    Object.assign(new Error("HTTP 404"), { code: 404, readThreadId: id }),
    Object.assign(new Error("会话已关闭或不存在"), { code: 404 }),
  ]) assert.equal(context.isMissingThreadError(error, id), false, error.message);
  context.session = { device: { ...context.session.device, createdAtMs: 2 } };
  context.relatedSessionView = { ...context.relatedSessionView, scope: context.privateInputScope(context.session) };
  assert.equal(context.isMissingThreadError(captured, id), false, "旧授权错误不能作用于新授权下同ID");
  assert.equal(nodes.get("#composer").hidden, false);
  assert.equal(nodes.get("#side-thread-expired").hidden, true);
});

test("静默恢复时最新历史读取失败仍保留待补读状态，不误判过期或丢失缓存草稿", async () => {
  const h = sideExpiryHarness(() => jsonResponse({ ok: false, message: "临时无法读取历史" }, 503));
  h.context.recoveryErrorSilenceDepth = 1;
  h.context.transcriptChecking = false;
  await h.context.readHistoryPage(false, h.context.selectionVersion);
  assert.equal(h.context.latestHistoryReadFailed, true);
  assert.equal(h.context.sideThreadExpired(h.id), false);
  assert.equal(h.notices.length, 0);
  h.assertRetained();
});

test("父会话的中文或英文缺失失败不误判当前侧边，准确侧边缺失才显示过期提示", async () => {
  for (const parentFails of [true, false]) {
    for (const language of ["zh", "en"]) {
      let fixture;
      fixture = sideExpiryHarness(({ path, options }) => {
        const body = JSON.parse(options.body);
        if (path === "/api/read/thread") return parentFails
          ? jsonResponse({ thread: { id: body.threadId } })
          : jsonResponse({ ok: false, message: language === "zh" ? "会话已关闭或不存在"
            : `thread not loaded: ${fixture.id}` }, 404);
        assert.equal(path, "/api/read/relatedThreads");
        return parentFails ? jsonResponse({ ok: false, message: language === "zh" ? "会话已关闭或不存在"
          : `unknown thread: ${fixture.parent}` }, 404)
          : jsonResponse({ sideThreads: [{ id: fixture.id }] });
      });
      await fixture.runCheck();
      assert.equal(fixture.context.sideThreadExpired(), !parentFails);
      assert.equal(fixture.nodes.get("#composer").hidden, !parentFails);
      assert.equal(fixture.nodes.get("#side-thread-expired").hidden, parentFails);
      fixture.assertRetained();
      assert.equal(fixture.calls.length, 2, "轮询只保留原有两项读取");
    }
  }
});

test("无父关系、旧授权或裸旧ID不推断为侧边，主会话暂时notLoaded保持可见消息", async () => {
  for (const kind of ["bare", "no-parent", "old-scope", "main", "main-503"]) {
    let fixture;
    fixture = sideExpiryHarness(() => jsonResponse({ ok: false,
      message: kind.startsWith("main") ? `thread not loaded: ${fixture.id}` : "原生临时侧边会话已关闭或不存在" },
    kind === "main-503" ? 503 : 404));
    const { context, nodes } = fixture;
    if (kind === "bare" || kind.startsWith("main")) {
      context.relatedSessionView = null; context.relatedThreadViews.clear();
      context.thread = { id: fixture.id, ephemeral: kind.startsWith("main") };
    } else if (kind === "no-parent") {
      context.relatedSessionView = { ...context.relatedSessionView, parentThreadId: null };
      context.relatedThreadViews.clear();
    } else context.relatedSessionView = { ...context.relatedSessionView, scope: "old-authorization" };
    context.syncRelatedSessionLayout();
    await assert.rejects(context.reloadThread(), error => error.code === (kind === "main-503" ? 503 : 404));
    assert.equal(context.thread.sideThreadExpired, undefined, kind);
    assert.equal(nodes.get("#composer").hidden, false, kind);
    assert.equal(nodes.get("#side-thread-expired").hidden, true, kind);
    fixture.assertRetained();
  }
});

test("503和403的实际历史读取不清消息或隐藏输入框，也不发起全局断连和写入", async () => {
  for (const [status, message] of [
    [503, "会话已关闭或不存在"], [503, "服务暂不可用"], [503, "thread not loaded: other-side"],
    [403, "会话已关闭或不存在"], [403, expiredSideProbe.thread.message],
  ]) {
    const fixture = sideExpiryHarness(() => jsonResponse({ ok: false, message }, status));
    await fixture.context.readHistoryPage(false, fixture.context.selectionVersion);
    assert.equal(fixture.context.sideThreadExpired(), false);
    assert.equal(fixture.nodes.get("#composer").hidden, false);
    assert.equal(fixture.nodes.get("#side-thread-expired").hidden, true);
    assert.equal(fixture.calls.length, 1);
    fixture.assertRetained();
  }
});

test("旧会话和旧授权的迟到读取不改变当前页，reload等待历史后也重新核对归属", async () => {
  for (const path of ["thread", "history", "poll", "after-history"]) {
    const response = deferred(), history = deferred();
    let fixture;
    fixture = sideExpiryHarness(({ path: requestPath }) => requestPath === "/api/read/relatedThreads"
      ? jsonResponse({ sideThreads: [{ id: fixture.id }] }) : response.promise);
    const { context, nodes, id } = fixture;
    if (path === "after-history") context.loadHistory = () => history.promise;
    const run = path === "history" ? context.readHistoryPage(false, context.selectionVersion)
      : path === "poll" ? fixture.runCheck() : context.reloadThread();
    const captured = run.catch(error => error);
    if (path === "after-history") {
      response.resolve(jsonResponse({ ok: false, message: "会话已关闭或不存在" }, 404));
      await new Promise(resolve => setImmediate(resolve));
    }
    if (path === "thread" || path === "history") {
      context.selectedId = "new-current"; ++context.selectionVersion;
      context.relatedSessionView = null; context.thread = { id: "new-current" };
    } else {
      context.session = { device: { ...context.session.device, createdAtMs: 2 } };
      context.relatedSessionView = { ...context.relatedSessionView, scope: context.privateInputScope(context.session) };
    }
    context.syncRelatedSessionLayout();
    response.resolve(jsonResponse({ ok: false, message: "会话已关闭或不存在" }, 404));
    history.resolve();
    await captured;
    assert.equal(context.thread.sideThreadExpired, undefined, path);
    assert.equal(context.relatedThreadViews.get(id)?.expired, undefined, path);
    assert.equal(nodes.get("#composer").hidden, false, path);
    assert.equal(nodes.get("#side-thread-expired").hidden, true, path);
    fixture.assertRetained();
  }
});

function sidebarDeletionHarness() {
  const reply = deferred(), calls = [], notices = [], disk = new Map(), removedDrafts = [];
  const target = { id: "delete-target", title: "本轮删除目标" };
  const control = { id: "control", title: "另一会话" };
  const controlRecord = {
    id: "control-receipt", deviceId: "device-a", threadId: control.id, state: "unknown",
  };
  const context = {
    URLSearchParams, session: { device: { id: "device-a" }, readOnly: false }, connected: true,
    selectedId: control.id, busy: false, stopping: false, sidebarBusy: false,
    pending: null, goalEditPending: null, goalEditDraft: null, recoveryScope: null,
    threadDeletionStorageKey: "codex-elves-thread-deletions",
    threadDeletions: new Map([[JSON.stringify(["device-a", control.id]), controlRecord]]),
    sessionStorage: { setItem: (key, value) => disk.set(key, value) },
    randomId: () => "original-delete-receipt", confirmAction: async () => true,
    mutate: async (action, payload, options) => {
      calls.push({ action, payload, options }); return reply.promise;
    },
    notice: (message, error) => notices.push({ message, error }),
    loadSidebar: async () => { assert.fail("未知或原生已确认的迟到失败不得额外重新读取侧栏"); },
    compactionLocked: () => false, queuedActionBusy: new Set(), queuedEditChecks: new Set(),
    drafts: new Map([[target.id, "删除目标草稿"], [control.id, "另一会话草稿"]]),
    skillDrafts: new Map(), attachmentDrafts: new Map(), goalDrafts: new Map(),
    persistInputDraft: async (id, remove) => { removedDrafts.push({ id, remove }); },
    transcriptCache: { remove() {} }, threadDisplays: new Map(), threadDisplayKey: id => id,
    readScope: null, sessionMenu: { removeThread() {} }, persistGoalComposerRecoveries: () => true,
    persistQueuedEdits() {}, persistCompactions() {}, cancelPromptOptimization() {},
    recentPriorityIds: new Set([target.id, control.id]),
    recentPriorityKnownIds: new Set([target.id, control.id]), recentPrioritySelectionSuppressions: new Set(),
    sidebar: { recent: [target, control], pinnedThreads: [target, control],
      projects: [{ id: "project", threads: [target, control] }] },
    cacheDisplayData() {}, sidebarSnapshot: value => value,
    location: { hash: "#thread=control", pathname: "/", search: "" },
    history: { replaceState() { assert.fail("删除其他会话不得改写当前会话路由"); } },
    renderSidebar() {}, updateComposer() {}, renderApprovals() {},
  };
  for (const name of ["goalComposerRecoveries", "promptOptimizations", "readMarkRequests", "queuedEdits",
    "compactions", "blockingRequestStates", "approvalReplies", "approvalDrafts",
    "questionReplyReceipts", "questionDrafts"]) context[name] = new Map();
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function deleteDraft(", "function inputDraftEmpty("),
    sourceSection("function threadDeletionKey(", "async function checkThreadDeletion("),
  ].join("\n"), context);
  const failReply = () => reply.reject(Object.assign(new TypeError("Failed to fetch"),
    { outcomeUnknown: true, operationState: "unknown" }));
  const assertControlRetained = () => {
    assert.equal(context.selectedId, control.id);
    assert.equal(context.drafts.get(control.id), "另一会话草稿");
    assert.equal(context.threadDeletions.get(context.threadDeletionKey(control.id, "device-a")), controlRecord);
    assert.equal(context.sidebar.recent.some(item => item.id === control.id), true);
    assert.equal(removedDrafts.some(record => record.id === control.id), false);
  };
  const persisted = () => JSON.parse(disk.get(context.threadDeletionStorageKey));
  return { context, reply, calls, notices, target, control, failReply, persisted, assertControlRetained };
}

test("删除挂起期间同设备同会话的原生删除事实先生效，迟到HTTP失败不得重建unknown或提示失败", async () => {
  const h = sidebarDeletionHarness();
  const deletion = h.context.deleteSidebarThread(h.target);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.calls.length, 1);
  assert.equal(h.calls[0].action, "deleteThread");
  assert.equal(h.calls[0].payload.threadId, h.target.id);
  assert.equal(h.calls[0].options.receiptId, "original-delete-receipt");
  const key = h.context.threadDeletionKey(h.target.id, "device-a");
  assert.equal(h.context.threadDeletions.get(key).state, "submitting");
  h.context.applyThreadDeletion(h.target.id, "device-a");
  const confirmed = h.context.threadDeletions.get(key);
  assert.equal(confirmed.state, "deleted");
  assert.equal(confirmed.id, "original-delete-receipt");
  assert.equal(h.context.drafts.has(h.target.id), false);
  assert.equal(h.context.sidebar.recent.some(item => item.id === h.target.id), false);
  h.failReply();
  await deletion;
  assert.equal(h.context.threadDeletions.get(key), confirmed);
  assert.equal(h.persisted().find(record => record.threadId === h.target.id).state, "deleted");
  assert.deepEqual(h.notices, []);
  assert.equal(h.calls.length, 1);
  h.assertControlRetained();
});

test("删除尚无原生确认时迟到HTTP失败仍保留准确原回执unknown和原错误提示", async () => {
  const h = sidebarDeletionHarness();
  const deletion = h.context.deleteSidebarThread(h.target);
  await new Promise(resolve => setImmediate(resolve));
  h.failReply();
  await deletion;
  const record = h.context.threadDeletions.get(h.context.threadDeletionKey(h.target.id, "device-a"));
  assert.equal(record.id, "original-delete-receipt");
  assert.equal(record.state, "unknown");
  assert.equal(h.persisted().find(item => item.threadId === h.target.id).state, "unknown");
  assert.equal(h.context.drafts.get(h.target.id), "删除目标草稿");
  assert.equal(h.context.sidebar.recent.some(item => item.id === h.target.id), true);
  assert.deepEqual(h.notices, [{ message: "Failed to fetch", error: true }]);
  assert.equal(h.calls.length, 1);
  h.assertControlRetained();
});

for (const [label, deletedId, deviceId] of [
  ["其他设备的同ID", "delete-target", "device-b"],
  ["同设备的其他会话", "another-deleted-thread", "device-a"],
])
  test(`${label}已删除不得压制当前未确认删除的HTTP错误或改写其原回执`, async () => {
    const h = sidebarDeletionHarness();
    const deletion = h.context.deleteSidebarThread(h.target);
    await new Promise(resolve => setImmediate(resolve));
    h.context.applyThreadDeletion(deletedId, deviceId);
    const otherKey = h.context.threadDeletionKey(deletedId, deviceId);
    const confirmedOther = h.context.threadDeletions.get(otherKey);
    h.failReply();
    await deletion;
    const record = h.context.threadDeletions.get(h.context.threadDeletionKey(h.target.id, "device-a"));
    assert.equal(record.id, "original-delete-receipt");
    assert.equal(record.state, "unknown");
    assert.equal(h.context.threadDeletions.get(otherKey), confirmedOther);
    assert.equal(confirmedOther.state, "deleted");
    assert.equal(h.context.drafts.get(h.target.id), "删除目标草稿");
    assert.deepEqual(h.notices, [{ message: "Failed to fetch", error: true }]);
    assert.equal(h.calls.length, 1);
    h.assertControlRetained();
  });

function sendRecoveryHarness() {
  const actions = [], draftRemovals = [], notices = [], reads = [];
  const prompt = { value: "专用测试草稿" }, originalText = prompt.value;
  let nextId = 0;
  const context = {
    TextEncoder, clearTimeout, session: { device: { id: "device-a", createdAtMs: 1 }, capabilities: {} },
    selectedId: "main", thread: { id: "main" }, selectionVersion: 1, selectionToken: "selection-a",
    recoveryScope: '["device-a",1]', recoveryGeneration: 0, connected: true, creating: false,
    busy: false, stopping: false, pending: null, sending: null, pendingSendRequest: null,
    editingQueuedMessage: null, editingMessage: null, goalEditPending: null, pendingTimer: null,
    selectedSkills: [], selectedGoal: null, selectedModel: "", selectedEffort: "", goalVersion: 0, queueVersion: 0,
    snapshotDraft: () => ({ text: prompt.value, skills: [], attachments: [], goalMode: null }),
    storedDraft: key => ({ text: context.drafts.get(key) || "", skills: [], attachments: [], goalMode: null }),
    threadDeletionLocked: () => false, threadDeleted: () => false, threadReadOnly: () => false,
    composerAttachmentsLoading: () => false, composerAttachmentsPayload: () => [], goalPrefix: () => null,
    composerPrompt: text => text, compactionLocked: () => false, rememberedNewChatModel: () => null,
    randomId: () => `send-${++nextId}`, goalComposerAction: () => false, updateComposer() {},
    recoveryStore: { activate: async () => {}, put: async () => {}, remove: async () => {}, clear: async () => {} },
    savePending: value => { context.pending = value; }, clearPendingFeedbackTimer() {},
    api: async (path, payload, requestId) => {
      const result = deferred(); actions.push({ path, payload, requestId, ...result }); return result.promise;
    },
    applyQueuedSendResult: () => false, sameSkills: () => true, persistSmallInputRecoveries() {},
    persistInputDraft: (key, remove) => {
      draftRemovals.push({ key, remove, scope: context.recoveryScope }); return Promise.resolve();
    },
    notice: message => notices.push(message), loadSidebar: async () => { reads.push("sidebar"); },
    reloadThread: async () => { reads.push("thread"); }, resetRelatedSessions() {}, clearReadViews: async () => {},
    transcriptCache: { clear() {} }, threadDeletionStorageKey: "deletions",
    sessionStorage: { getItem: () => null, removeItem() {} },
    clearComposerAttachments() {}, renderSelectedSkills() {}, $: () => prompt,
  };
  for (const name of ["drafts", "skillDrafts", "attachmentDrafts", "goalDrafts", "draftWrites",
    "failedInputRecoveries", "goalComposerRecoveries", "submittedDrafts", "queuedEdits", "questionDrafts",
    "questionReplyReceipts", "questionReceiptChecks", "compactions", "blockingRequestStates", "approvalReplies",
    "approvalDrafts", "threadDeletions"]) context[name] = new Map();
  context.drafts.set("main", originalText);
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function privateInputScope(", "function validRelatedThreadId("),
    sourceSection("function clearPrivateInputs(", "async function restorePrivateInputs("),
    sourceSection("function deleteDraft(", "function inputDraftEmpty("),
    sourceSection("function matchingDraft(", "function resetComposerOptions("),
    sourceSection("async function mutate(", "function acceptedSendItem("),
    sourceSection("async function sendMessage(", "async function stopGeneration("),
  ].join("\n"), context);
  const complete = index => actions[index].resolve({
    requestId: actions[index].requestId, state: "completed", result: { ok: true, threadId: "main" },
  });
  return { context, actions, draftRemovals, notices, reads, prompt, originalText, complete };
}

test("旧授权普通发送的迟到成功不得清理新授权同键同内容草稿或刷新新授权页面", async () => {
  for (const change of ["device", "created-at", "generation"]) {
    const { context, actions, draftRemovals, reads, originalText, complete } = sendRecoveryHarness();
    const oldSend = context.sendMessage();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(actions.length, 1);
    context.session = { device: { id: change === "device" ? "device-b" : "device-a",
      createdAtMs: change === "created-at" ? 2 : 1 }, capabilities: {} };
    if (change === "generation") ++context.recoveryGeneration;
    context.recoveryScope = context.privateInputScope(context.session);
    context.pending = null; ++context.selectionVersion;
    context.drafts = new Map([["main", originalText]]);
    complete(0);
    await oldSend;
    assert.equal(context.drafts.get("main"), originalText, change);
    assert.deepEqual(draftRemovals, [], change);
    assert.deepEqual(reads, [], change);
  }
});

test("清理旧授权后立即允许新发送，旧发送迟到成功不能释放新发送的busy或清理新稿", async () => {
  const { context, actions, notices, originalText, complete } = sendRecoveryHarness();
  const oldSend = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  await context.clearPrivateInputs();
  assert.equal(context.busy, false);
  assert.equal(context.sending, null);
  context.session = { device: { id: "device-b", createdAtMs: 2 }, capabilities: {} };
  context.recoveryScope = context.privateInputScope(context.session); ++context.selectionVersion;
  context.drafts.set("main", originalText);
  const newSend = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(actions.length, 2);
  const currentSending = context.sending;
  complete(0);
  await oldSend;
  assert.equal(context.busy, true);
  assert.equal(context.sending, currentSending);
  assert.equal(context.drafts.get("main"), originalText);
  assert.deepEqual(notices, []);
  complete(1);
  await newSend;
  assert.equal(context.busy, false);
  assert.equal(context.sending, null);
  assert.equal(context.drafts.has("main"), false);
});

test("同授权切到另一会话后原发送成功仍清理原稿，不清当前会话新稿", async () => {
  const { context, complete, prompt } = sendRecoveryHarness();
  const send = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  context.selectedId = "control"; context.thread = { id: "control" }; ++context.selectionVersion;
  context.drafts.set("control", "另一会话的新稿"); prompt.value = "另一会话的新稿";
  complete(0);
  await send;
  assert.equal(context.drafts.has("main"), false);
  assert.equal(context.drafts.get("control"), "另一会话的新稿");
  assert.equal(prompt.value, "另一会话的新稿");
});

function answeredQuestionReadHarness({ loading = false } = {}) {
  const { context, threadReads } = queueReloadHarness(), frames = [];
  const question = id => ({ id, method: "remote/requestUserInputAsync", params: { questions: [{ id: `${id}-field` }] } });
  const first = question("answered"), remaining = question("remaining");
  const blocking = { id: "blocking", method: "item/tool/requestUserInput", params: {} };
  const key = JSON.stringify(["device-a", "main", first.id]);
  Object.assign(context, {
    session: { device: { id: "device-a", createdAtMs: 1 } },
    thread: loading ? null : { id: "main", requests: [first, remaining, blocking] },
    questionReplyReceipts: new Map([[key, {
      id: "answer-receipt", deviceId: "device-a", threadId: "main", requestId: first.id, state: "unknown",
    }]]),
    questionDrafts: new Map([[key, new Map([["answered-field", "原回答"]])],
      [JSON.stringify(["device-a", "main", remaining.id]), new Map([["remaining-field", "保留未提交回答"]])]]),
    notifiedQuestions: null, sessionStorage: { setItem() {} }, renderSidebar() {}, approvalSignature: null,
    renderApprovals: () => frames.push({ ids: (context.thread?.requests || []).map(request => request.id),
      locked: context.questionReplyReceipts.has(key) }),
  });
  vm.runInContext(sourceSection("function replaceAsyncQuestions(", "async function recoverQuestionReceipts("), context);
  const completeRead = (index, requests) => threadReads[index].resolve({
    thread: { id: "main", title: "测试问答", cwd: "", requests },
  });
  return { context, threadReads, frames, key, first, remaining, blocking, completeRead };
}

for (const loading of [false, true]) test(`问答成功后迟到旧读不复活原题且保留其他题${loading ? "，当前详情仍在加载" : ""}`, async () => {
  const { context, threadReads, frames, key, first, remaining, blocking, completeRead } = answeredQuestionReadHarness({ loading });
  const oldRead = context.reloadThread();
  const success = context.applyQuestionReceipt(key, {
    requestId: "answer-receipt", state: "completed",
    result: { ok: true, threadId: "main", requestId: first.id },
  });
  completeRead(0, [first, remaining, blocking]);
  await new Promise(resolve => setImmediate(resolve));
  const beforeFresh = (context.thread?.requests || []).map(request => request.id);
  const readsBeforeFresh = threadReads.length;
  if (threadReads[1]) completeRead(1, [remaining, blocking]);
  await Promise.all([oldRead, success]);
  assert.equal(readsBeforeFresh, 2);
  assert.equal(beforeFresh.includes(first.id), false);
  assert.equal(frames.some(frame => frame.ids.includes(first.id) && !frame.locked), false);
  assert.equal(context.questionReplyReceipts.has(key), false);
  assert.deepEqual(context.thread.requests.map(request => request.id), [remaining.id, blocking.id]);
  assert.equal(context.questionDrafts.get(JSON.stringify(["device-a", "main", remaining.id])).get("remaining-field"),
    "保留未提交回答");
});

test("实际offline再online回调立即退役旧显示读取并启动新读，保留原写和草稿", async () => {
  const { context, refreshes, pendingWrite } = lifecycle(), listeners = new Map();
  Object.assign(context, {
    window: { addEventListener: (name, callback) => listeners.set(name, callback) },
    navigator: { onLine: true }, initialAuthenticationRequired: false, connecting: null, recoveryErrorSilenceDepth: 0,
    stream: null, connected: false,
    stopCatalogRefresh() {}, setConnection: value => { context.connected = value; },
    recoverQuestionReceipts() {}, refreshCachedCatalogs() {}, ensureStream() {},
    syncConnectionState: async source => {
      assert.equal(source, context.stream);
      context.connected = true;
      return true;
    },
    showConnectionRecovery() {},
    DOMException,
  });
  vm.runInContext([
    sourceSection("async function refreshConnectedSession(", "function savePending("),
    sourceSection("function withRecoveryErrorSilence(", "async function refreshAll("),
    sourceSection("function retryConnectionRecovery(", "async function copyText("),
    sourceSection('window.addEventListener("online",', 'document.addEventListener("visibilitychange", () => {\n  if (document.hidden) {\n    stopCatalogRefresh();'),
  ].join("\n"), context);
  const oldRefresh = context.refreshAll();
  listeners.get("offline")(); listeners.get("online")();
  await new Promise(resolve => setImmediate(resolve));
  const readsBeforeOldFinished = refreshes.length, generation = context.foregroundReadGeneration;
  for (const request of refreshes) request.resolve();
  await new Promise(resolve => setImmediate(resolve));
  for (const request of refreshes) request.resolve();
  await oldRefresh;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(readsBeforeOldFinished, 2);
  assert.equal(generation, 1);
  assert.equal(pendingWrite.signal.aborted, false);
  assert.equal(context.pending.id, "original-write");
  assert.equal(context.drafts.get("thread-a"), "保留原草稿");
});

function connectionRecoveryHarness() {
  const { context, threadReads, historyReads, notices } = queueReloadHarness();
  const probes = [], sidebarReads = [], frames = [], receiptChecks = [], writes = [], intervals = new Map();
  const listeners = new Map(), pendingWrite = new AbortController(), connectionIndicator = {};
  const question = {
    id: "existing-question", method: "item/tool/requestUserInput",
    params: { questions: [{ id: "existing-field", question: "断线期间已生成的问题" }] },
  };
  const snapshot = (overrides = {}) => ({
    device: { id: "device-a", createdAtMs: 1 }, nativeConnected: true, serviceConnected: false, ...overrides,
  });
  const makeStream = () => ({ readyState: 1, transport: "shared", close() { this.readyState = 2; } });
  let routeReady = false, intervalId = 0;
  const readThread = context.api, node = context.$;
  Object.assign(context, {
    DOMException, EventSource: { OPEN: 1, CLOSED: 2 },
    session: snapshot(), thread: { id: "main", title: "原会话", requests: [], queuedMessages: [] },
    connected: false, stream: makeStream(), streamRetry: null, streamBackoff: 1000,
    refreshRequest: null,
    creating: false, historyInitialized: true, transcriptChecking: false, historyError: "",
    threadReadFailed: false, latestHistoryReadFailed: false,
    selectionController: null, historyRequest: null, sideThreadExpired: () => false,
    isMissingThreadError: () => false,
    nativeConnectionVersion: 0, connectionCycleVersion: 0, connectionPollVersion: 0,
    connectionPollPending: null, recoveryErrorSilenceDepth: 0,
    navigator: { onLine: true }, initialAuthenticationRequired: false, connecting: null,
    pending: { id: "original-write", state: "unknown", controller: pendingWrite },
    drafts: new Map([["main", "保留普通草稿"]]),
    questionDrafts: new Map([["existing-question", "保留未提交回答"]]),
    window: { addEventListener: (name, callback) => listeners.set(name, callback) },
    $: selector => selector === "#connection-state" ? connectionIndicator : node(selector),
    setConnection: value => { context.connected = value; },
    readConnectionSession: async () => {
      const probe = deferred(); probes.push(probe);
      const latest = await probe.promise;
      routeReady = true;
      return latest;
    },
    api: async (...args) => {
      assert.equal(routeReady, true, "线路尚未探测恢复时不得提前消耗当前线程读取");
      return readThread(...args);
    },
    loadSidebar: async (_payload, options) => {
      sidebarReads.push(options);
      assert.equal(routeReady, true, "线路尚未探测恢复时不得提前读取侧栏");
    },
    renderApprovals: () => frames.push((context.thread?.requests || []).map(request => request.id)),
    reconnectChangedAuthorization: async () => { receiptChecks.push("authorization-changed"); },
    resolvePending: async () => { receiptChecks.push("original-receipt"); },
    mutate: async (...args) => { writes.push(args); assert.fail("恢复连接不得自动重新派发写操作"); },
    recoverQuestionReceipts: async () => {},
    refreshCachedCatalogs() {}, ensureStream() {}, stopCatalogRefresh() {}, showConnectionRecovery() {},
    syncGoalLiveTimer() {}, scheduleRelatedOperationCheck() {}, drainPendingNotificationOpen() {},
    clearPendingFeedbackTimer() {},
    syncActivityTimer() {}, stopActivityTimer() {}, stopGoalLiveTimer() {}, finishQueueDrag() {},
    closeContextUsageTip() {}, rememberDraft() {}, rememberTranscript() {}, rememberReadingPosition() {},
    stopRelatedSessionTimers() {}, sessionMenu: null, cancelConnectionProbe() {}, syncHistoryButtons() {},
    sidebarRequest: null, sidebarTimer: null, historyController: null,
    handleRemoteChannelEvent: () => false, scheduleRelatedMenuRefresh() {},
    createRemoteEventStream: makeStream, sharedEventsAvailable: () => true,
    loadModels: async () => {}, refreshSkillCatalog: async () => {},
    connect: { poll: null },
    setInterval(callback, delay) {
      assert.equal(delay, 15000, "沿用既有连接轮询周期");
      const id = ++intervalId; intervals.set(id, callback); return id;
    },
    clearInterval(id) { intervals.delete(id); },
  });
  context.document.addEventListener = (name, callback) => listeners.set(name, callback);
  vm.runInContext([
    sourceSection("async function syncConnectionState(", "function savePending("),
    sourceSection("function withRecoveryErrorSilence(", "async function refreshAll("),
    sourceSection("async function refreshAll(", "function openStream("),
    sourceSection("function openStream(", "function reconnectStream("),
    sourceSection("function stopConnectionPoll(", "function retireForegroundReads("),
    sourceSection("function retireForegroundReads(", "function resumePage()"),
    sourceSection("function resumePage()", "async function connect("),
    sourceSection("function retryConnectionRecovery(", "async function copyText("),
    `${sourceSection("function receive(event) {", "  const params = { ...event.params || {} };")}\n}`,
    sourceSection('  $("#connection-state").onclick =', "  const toolsTrigger ="),
    sourceSection('window.addEventListener("online",', 'document.addEventListener("visibilitychange", () => {\n  if (document.hidden) {\n    stopCatalogRefresh();'),
    sourceSection('document.addEventListener("visibilitychange", () => {\n  if (document.hidden) {\n    stopCatalogRefresh();',
      'window.addEventListener("popstate",'),
  ].join("\n"), context);
  const recover = source => {
    if (source === "open") { context.openStream(); return context.stream.onopen(); }
    if (source === "resync" || source === "reconnected")
      return context.receive({ method: `remote/${source}`, params: { nativeConnected: true } });
    if (source === "resume") { context.pageLeaving = true; return context.resumePage(); }
    if (source === "online") return listeners.get("online")();
    assert.equal(source, "manual");
    return connectionIndicator.onclick();
  };
  const completeRead = index => threadReads[index].resolve({
    thread: { id: "main", title: "原会话", cwd: "", requests: [question], queuedMessages: [] },
  });
  return {
    context, probes, sidebarReads, frames, receiptChecks, writes, threadReads, historyReads, notices,
    pendingWrite, question, snapshot, recover, completeRead, listeners,
    poll: () => {
      const callback = intervals.get(context.connect.poll);
      assert.equal(typeof callback, "function");
      return callback();
    },
  };
}

test("准备导航但仍停留原页面时，beforeunload 不关闭实时连接或连接轮询", () => {
  const h = connectionRecoveryHarness(), original = h.context.stream;
  h.context.connected = true;
  h.context.startConnectionPoll("device-a");
  const poll = h.context.connect.poll;
  h.listeners.get("beforeunload")();
  assert.equal(h.context.pageLeaving, false, "准备导航不等于原文档已经离开");
  assert.equal(h.context.stream, original);
  assert.equal(original.readyState, 1);
  assert.equal(h.context.connect.poll, poll);
  assert.equal(h.pendingWrite.signal.aborted, false);
});

test("发送状态诊断区分未读完整、回执和压缩锁定，只在状态变化时记录计数", () => {
  const events = [], receipt = { id: "original-write" };
  const context = {
    editingMessage: null, editingQueuedMessage: null, editing: false,
    deleting: false, busy: false, stopping: false, compacting: false,
    compactionSendReason: "", compaction: { state: "idle" },
    session: { device: { id: "device-a" } }, connected: true, pageLeaving: false,
    stream: { readyState: 1 }, threadReadOnly: () => false,
    unsupportedImages: false, unsupportedProjectless: false, goalReason: "",
    composerAttachmentsLoading: () => false, action: "send", pending: null,
    prompt: "不应进入日志的私人正文", hasAttachments: false, creating: false,
    selectedId: "main", thread: null, send: {}, composerDiagnosticState: "",
    privateInputScope: () => "device-a",
    composerAttachmentSnapshot: () => [],
    recordRuntimeDiagnostic: (name, fields) => events.push({ name, fields }),
  };
  vm.createContext(context);
  const update = vm.runInContext(`() => {${sourceSection(
    "  const blockedBy = editingMessage ?", '  $("#prompt").disabled =')}}`, context);
  update();
  assert.equal(context.send.disabled, true);
  assert.equal(events.at(-1).fields.blockedBy, "thread-not-loaded");
  context.thread = { id: "main" };
  update();
  assert.equal(context.send.disabled, false);
  assert.equal(events.at(-1).fields.blockedBy, "ready");
  context.prompt += "继续编辑";
  update();
  assert.equal(events.length, 2, "普通输入不得逐键刷日志");
  context.pending = receipt;
  update();
  assert.equal(context.send.disabled, true);
  assert.equal(events.at(-1).fields.blockedBy, "pending-receipt");
  assert.equal(context.pending, receipt, "记录诊断不能解除未知回执锁");
  context.pending = null;
  context.compacting = true; context.compaction = { state: "unknown" };
  context.compactionSendReason = "等待原压缩回执";
  update();
  assert.equal(context.send.disabled, true);
  assert.equal(events.at(-1).fields.blockedBy, "compaction-unknown");
  context.compaction = { state: "running", nativeConfirmed: true };
  context.compactionSendReason = "";
  update();
  assert.equal(context.send.disabled, false, "已确认压缩且可排队时不能因诊断改动禁用发送");
  assert.equal(events.at(-1).fields.blockedBy, "ready");
  assert.equal(JSON.stringify(events).includes("私人正文"), false);
  assert.ok(events.every(({ fields }) => Number.isInteger(fields.draftChars) && fields.attachmentCount === 0));
});

for (const source of ["visibility", "cancelled-navigation", "pagehide-visible", "pagehide-pageshow"])
  test(`${source}通过实际生命周期事件恢复读取，保留草稿与未知回执`, async () => {
    const h = connectionRecoveryHarness(), pending = h.context.pending;
    h.context.connected = true;
    h.context.startConnectionPoll("device-a");
    if (source === "cancelled-navigation") h.listeners.get("beforeunload")();
    if (source.startsWith("pagehide")) h.listeners.get("pagehide")();
    h.context.document.hidden = true;
    h.listeners.get("visibilitychange")();
    h.context.document.hidden = false;
    if (source === "pagehide-pageshow") h.listeners.get("pageshow")();
    else h.listeners.get("visibilitychange")();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.pageLeaving, false);
    assert.equal(h.probes.length, 1, "恢复后必须实际发起连接读取，不能只恢复界面可见性");
    h.probes[0].resolve(h.snapshot());
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.threadReads.length, 1);
    assert.equal(h.historyReads.length, 1);
    h.completeRead(0);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.thread.requests[0].id, h.question.id);
    assert.equal(h.context.pending, pending);
    assert.equal(h.pendingWrite.signal.aborted, false);
    assert.equal(h.context.drafts.get("main"), "保留普通草稿");
    assert.equal(h.context.questionDrafts.get(h.question.id), "保留未提交回答");
    assert.equal(h.writes.length, 0);
  });

for (const source of ["open", "resync", "reconnected", "resume", "online", "manual"])
  test(`${source}恢复先完成连接探测，再自动读出原问题，未决写和草稿保持`, async () => {
    const h = connectionRecoveryHarness(), pending = h.context.pending;
    const recovery = h.recover(source);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.probes.length, 1);
    assert.equal(h.threadReads.length, 0, "探测尚未完成时不得先派发当前会话读取");
    assert.equal(h.historyReads.length, 0, "探测尚未完成时不得先消耗历史刷新机会");
    assert.equal(h.sidebarReads.length, 0);
    assert.equal(h.context.refreshRequest, null);
    assert.deepEqual(h.context.thread.requests, []);
    h.probes[0].resolve(h.snapshot());
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.connected, true);
    assert.equal(h.threadReads.length, 1);
    assert.equal(h.historyReads.length, 1);
    h.completeRead(0);
    await recovery;
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.thread.requests[0].id, h.question.id);
    assert.equal(h.frames.some(ids => ids.includes(h.question.id)), true);
    assert.equal(h.context.pending, pending);
    assert.equal(h.context.pending.state, "unknown");
    assert.equal(h.pendingWrite.signal.aborted, false);
    assert.equal(h.context.drafts.get("main"), "保留普通草稿");
    assert.equal(h.context.questionDrafts.get(h.question.id), "保留未提交回答");
    assert.equal(h.writes.length, 0);
    assert.equal(h.notices.length, 0);
  });

for (const [label, invalidate, latest] of [
  ["旧事件流", context => { context.stream = { readyState: 1 }; }],
  ["旧原生连接代次", context => { ++context.nativeConnectionVersion; }],
  ["旧连接周期", context => { ++context.connectionCycleVersion; }],
  ["旧轮询代次", context => { ++context.connectionPollVersion; }],
  ["已离开页面", context => { context.pageLeaving = true; }],
  ["设备授权已更换", context => { context.session = { device: { id: "device-b", createdAtMs: 1 } }; }],
  ["同设备授权范围已更换", () => {}, { device: { id: "device-a", createdAtMs: 2 } }],
])
  test(`${label}的探测结果不得派生当前会话或历史读取`, async () => {
    const h = connectionRecoveryHarness(), pending = h.context.pending;
    h.context.connected = true;
    h.recover("online");
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.probes.length, 1);
    invalidate(h.context);
    h.probes[0].resolve(h.snapshot(latest));
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.threadReads.length, 0);
    assert.equal(h.historyReads.length, 0);
    assert.equal(h.sidebarReads.length, 0);
    assert.equal(h.context.pending, pending);
    assert.equal(h.context.drafts.get("main"), "保留普通草稿");
    assert.equal(h.writes.length, 0);
  });

for (const [label, source, prepare, latest] of [
  ["WS仍在连接时手动刷新", "manual", context => { context.stream.readyState = 0; }, {}],
  ["原生尚未就绪时open核对原回执", "open", () => {}, { nativeConnected: false, serviceConnected: false }],
])
  test(`${label}只要session探测已应用就允许补读原问题`, async () => {
    const h = connectionRecoveryHarness(), pending = h.context.pending;
    prepare(h.context);
    const recovery = h.recover(source);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.probes.length, 1);
    assert.equal(h.threadReads.length, 0);
    assert.equal(h.historyReads.length, 0);
    h.probes[0].resolve(h.snapshot(latest));
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.connected, false);
    assert.equal(h.threadReads.length, 1, "已应用session结果后不得额外用WS或原生状态封锁读取");
    assert.equal(h.historyReads.length, 1);
    if (source === "open")
      assert.deepEqual(h.receiptChecks, ["original-receipt"], "原回执核对不等待原生ready或全文读取完成");
    h.completeRead(0);
    await recovery;
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.context.thread.requests[0].id, h.question.id);
    assert.equal(h.context.pending, pending);
    assert.equal(h.context.drafts.get("main"), "保留普通草稿");
    assert.equal(h.context.questionDrafts.get(h.question.id), "保留未提交回答");
    assert.equal(h.writes.length, 0);
  });

test("open探测被冷却拒绝后，既有轮询恢复连接会补出原问题，连续在线轮询不重复读全文", async () => {
  const h = connectionRecoveryHarness(), pending = h.context.pending;
  const opened = h.recover("open");
  await new Promise(resolve => setImmediate(resolve));
  const unavailable = Object.assign(new Error("所有线路仍在冷却"), { notSubmitted: true });
  h.probes[0].reject(unavailable);
  await opened;
  assert.equal(h.context.connected, false);
  assert.equal(h.threadReads.length, 0);
  assert.equal(h.historyReads.length, 0);
  assert.equal(h.sidebarReads.length, 0);
  h.context.startConnectionPoll("device-a");
  const restored = h.poll();
  assert.equal(h.probes.length, 2);
  h.probes[1].resolve(h.snapshot());
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.context.connected, true);
  assert.equal(h.threadReads.length, 1);
  assert.equal(h.historyReads.length, 1);
  h.completeRead(0);
  await restored;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.context.thread.requests[0].id, h.question.id);
  assert.equal(h.frames.some(ids => ids.includes(h.question.id)), true);
  for (let index = 2; index < 4; index++) {
    const online = h.poll();
    assert.equal(h.probes.length, index + 1);
    h.probes[index].resolve(h.snapshot());
    await online;
  }
  assert.equal(h.threadReads.length, 1, "持续在线的轮询只更新连接与侧栏，不重复读取全文");
  assert.equal(h.historyReads.length, 1);
  assert.equal(h.context.pending, pending);
  assert.equal(h.context.pending.state, "unknown");
  assert.equal(h.pendingWrite.signal.aborted, false);
  assert.equal(h.context.drafts.get("main"), "保留普通草稿");
  assert.equal(h.context.questionDrafts.get(h.question.id), "保留未提交回答");
  assert.equal(h.writes.length, 0);
});

test("连接一直在线但只恢复旧消息、会话设置缺失时，既有轮询自动补读并保留草稿", async () => {
  const h = connectionRecoveryHarness();
  h.context.connected = true;
  h.context.thread = null;
  h.context.transcriptChecking = true;
  h.context.startConnectionPoll("device-a");
  const poll = h.poll();
  h.probes[0].resolve(h.snapshot());
  await poll;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.threadReads.length, 1, "连接成功不能掩盖当前会话设置仍未取得");
  assert.equal(h.historyReads.length, 1);
  h.completeRead(0);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.context.thread.id, "main");
  assert.equal(h.context.drafts.get("main"), "保留普通草稿");
  assert.equal(h.writes.length, 0);
});

test("恢复补读失败后下次在线探测重试读取，成功后停止补读而不重发消息", async () => {
  const h = connectionRecoveryHarness(), pending = h.context.pending;
  h.context.connected = true;
  h.context.startConnectionPoll("device-a");
  h.recover("online");
  await new Promise(resolve => setImmediate(resolve));
  h.probes[0].resolve(h.snapshot());
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.threadReads.length, 1);
  h.threadReads[0].reject(Object.assign(new Error("暂时无法读取会话设置"), { code: 503 }));
  await new Promise(resolve => setImmediate(resolve));
  const retry = h.poll();
  h.probes[1].resolve(h.snapshot());
  await retry;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.threadReads.length, 2, "首次恢复失败后不能永远只刷新连接和侧栏");
  h.completeRead(1);
  await new Promise(resolve => setImmediate(resolve));
  const settled = h.poll();
  h.probes[2].resolve(h.snapshot());
  await settled;
  assert.equal(h.threadReads.length, 2, "成功后不增加持续全文轮询");
  assert.equal(h.context.pending, pending);
  assert.equal(h.pendingWrite.signal.aborted, false);
  assert.equal(h.writes.length, 0);
});

function sideCreationNavigationHarness() {
  const creationReply = deferred(), writes = [], menuChanges = [], preparedStates = [], notices = [];
  const disk = storage(), listeners = [], location = new URL("https://remote.test/#thread=main");
  disk.removeItem = key => disk.records.delete(key);
  const node = (tag, className = "", text = "") => ({
    tag, className, textContent: text, children: [], inert: false,
    classList: { add() {}, remove() {} }, setAttribute() {}, focus() {}, getBoundingClientRect: () => ({}),
    append(...children) {
      for (const child of children) { this.children.push(child); child.parent = this; }
    },
    remove() { this.parent.children = this.parent.children.filter(child => child !== this); },
    replaceChildren(...children) { this.children = children; },
  });
  const body = node("body"), workspace = node("main"), prompt = { value: "M 的未发送草稿", disabled: false, focus() {} };
  const nodes = new Map([
    [".workspace", workspace], ["#prompt", prompt], ["#messages", node("section")],
    ["#project-select", { value: "" }],
    ["#project-select-slot", {}], ["#thread-title", {}], ["#thread-path", {}], ["#model-settings", { hidden: true }],
  ]);
  const historyEntries = [{ state: { keep: { source: "unrelated" }, position: 4 }, url: location.href }];
  let position = 0, backCalls = 0, sequence = 0, attachments = [{ name: "main.png" }], sidebarOpen = false;
  const history = {
    get state() { return historyEntries[position].state; },
    pushState(state, _title, url) {
      location.href = new URL(url, location.href).href;
      historyEntries.splice(++position, Infinity, { state: structuredClone(state), url: location.href });
    },
    replaceState(state, _title, url) {
      location.href = new URL(url, location.href).href;
      historyEntries[position] = { state: structuredClone(state), url: location.href };
    },
    back() {
      backCalls++;
      if (!position) return;
      location.href = historyEntries[--position].url;
      for (const listener of listeners) listener({ state: history.state });
    },
  };
  const context = {
    URLSearchParams, AbortController, clearTimeout, setTimeout, location, history, sessionStorage: disk,
    session: { device: { id: "creation-device", createdAtMs: 1 }, capabilities: {} },
    selectedId: "main", selectionVersion: 1, selectionToken: "main-selection",
    relatedSelectionVersion: 0, relatedNavigationOwner: "current-page", relatedSessionView: null,
    relatedThreadViews: new Map(), relatedThreadOperations: new Map(), sideCreationView: null,
    sideCreationHistoryKey: "creation", relatedHistoryKey: "related", conversationHistoryKey: "position",
    relatedOperationStorageKey: "operations", relatedReturnTranscript: null, relatedExitTransition: null,
    relatedBackPending: false, relatedMenuTimer: null, relatedOperationTimer: null,
    thread: { id: "main", title: "M" }, creating: false, connected: true, pageLeaving: false,
    historyInitialized: true, historyLoading: false, selectionController: null, historyCursor: null,
    threadRequest: null, historyRequest: null, threadReadFailed: false, latestHistoryReadFailed: false,
    transcriptChecking: false, historyError: "", sideThreadExpired: () => false,
    turns: new Map(), transcriptEvents: new Map(), threadReadVersion: 0,
    tokenUsageVersion: 0, statusVersion: 0, questionVersion: 0, goalVersion: 0, compactionVersion: 0, queueVersion: 0,
    drafts: new Map([["other", "C 的原有草稿"]]),
    skillDrafts: new Map([["other", [{ name: "C-skill", path: "/C/SKILL.md" }]]]),
    attachmentDrafts: new Map([["other", [{ name: "other.png" }]]]), goalDrafts: new Map(),
    selectedSkills: [{ name: "M-skill", path: "/M/SKILL.md" }], selectedGoal: null,
    editingQueuedMessage: null, editingMessage: null, pendingSteer: null,
    readScope: null, sidebar: null, sessionTools: null, recoveryErrorSilenceDepth: 0,
    initialAuthenticationRequired: false, pendingNotificationOpen: null,
    document: { body, hidden: false, querySelectorAll: () => [] },
    window: { addEventListener: (_name, listener) => listeners.push(listener) },
    matchMedia: () => ({ matches: true }), $: selector => nodes.get(selector),
    element: node, button: (_text, _label, action) => Object.assign(node("button"), { action }),
    randomId: () => `operation-${++sequence}`,
    composerAttachmentSnapshot: () => structuredClone(attachments),
    setComposerAttachments: value => { attachments = structuredClone(value); },
    syncSelectedSkillsFromPrompt() {}, renderSelectedSkills() {}, persistInputDraft: async () => {},
    sameSkills: (left, right) => JSON.stringify(left) === JSON.stringify(right),
    threadDeleted: () => false, finishRelatedSessionExit() {}, stopGoalLiveTimer() {}, cancelMessageEdit() {},
    closePopup() {}, closeComposerMenus() {}, closeMobileSidebar() { sidebarOpen = false; }, rememberTranscript() {},
    setSelectValue: (control, value) => { control.value = value; },
    draftKey: () => context.selectedId || "$new:",
    resetMessages() {}, resetComposerOptions() {}, restoreQueuedMessageEdit() {}, renderApprovals() {},
    restoreTranscript: () => null, historyPlaceholder: () => node("p"), currentThreadDisplay: () => context.thread,
    rememberReadingPosition() {}, renderSidebar() {}, rememberThreadDisplay() {},
    mergeBlockingRequestsRead() {}, applyQueueRead() {}, reconcileCompaction() {}, renderMessages() {},
    blockingRequestState: () => ({ generation: 0 }), loadHistory: async () => {},
    applyRelatedThreadMetadata() {}, replaceTranscriptEvents() {}, refreshTranscriptEvents() {},
    currentCompaction: () => null, syncEditableMessageMetadata() {}, scheduleMessages() {},
    isActiveThread: () => false, markVisibleThreadRead: async () => {},
    sessionMenuIsOpen: () => false, notice: message => notices.push(message),
    sessionMenu: {
      isOpen: () => false, isSuspended: () => false, close() {}, resume() {},
      updateSideThread: (parent, item) => menuChanges.push({ parent, item }),
    },
    syncRelatedSessionLayout() {
      if (context.relatedSessionView?.threadId === "created-side")
        preparedStates.push({ view: context.sideCreationView, inert: workspace.inert });
    },
    updateComposer() { prompt.disabled = context.threadReadOnly(); },
    mutate: async (action, payload, options) => {
      writes.push({ action, payload, options });
      return await creationReply.promise;
    },
    api: async (path, payload) => {
      assert.equal(path, "/api/read/thread");
      return { thread: { id: payload.threadId, title: payload.threadId, requests: [], transcriptEvents: [] } };
    },
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function snapshotDraft()", "function inputDraftEmpty("),
    sourceSection("function privateInputScope(", "function threadSentMessageEditAllowed("),
    sourceSection("function rememberRelatedView(", "function syncRelatedSessionLayout("),
    sourceSection("function conversationHistoryPosition(", "function rememberTransientHistory("),
    sourceSection("async function openRelatedThread(", "function updateSideCreationStatus("),
    sourceSection("function updateSideCreationStatus(", "function renderRelatedOperationStatus("),
    sourceSection("function rememberDraft()", "function persistQueuedEdits("),
    sourceSection("function filterSideThreadSidebar(", "function applyThreadDeletion("),
    sourceSection("async function selectThread(", "function syncNewChatHash("),
    sourceSection("function syncNewChatHash()", "function welcomeNode()"),
    sourceSection("function showWelcome(", "async function loadHistory("),
    sourceSection("function selectedThreadProjectId()", "async function loadSidebar("),
    sourceSection("function selectedThreadReadIncomplete()", "function retireForegroundReads("),
    sourceSection("function closeDialogsForNotification()", "function drainPendingNotificationOpen("),
    sourceSection('window.addEventListener("popstate", event => {\n  const target =',
      "const element ="),
    sourceSection('window.addEventListener("popstate", event => {\n  if (!session) return;',
      'postPlatformMessage("ready");'),
  ].join("\n"), context);
  const result = { ok: true, parentThreadId: "main", threadId: "created-side",
    thread: { id: "created-side", title: "S", viewKind: "side", sideHistoryVersion: 1, parentThreadId: "main" } };
  const start = () => {
    const promise = context.runRelatedOperation("createSideThread");
    const record = [...context.relatedThreadOperations.values()][0], view = context.sideCreationView;
    return { promise, record, view };
  };
  const settle = () => new Promise(resolve => setImmediate(resolve));
  return { context, workspace, prompt, body, historyEntries, writes, menuChanges, preparedStates, notices,
    disk, start, settle, get backCalls() { return backCalls; },
    openSidebar: () => { sidebarOpen = true; }, isSidebarOpen: () => sidebarOpen,
    async complete(operation) { creationReply.resolve(result); await operation.promise; await settle(); } };
}

test("自动进入新建页保留用户已经展开的侧栏，显式新建仍关闭侧栏", async () => {
  const h = sideCreationNavigationHarness();
  h.context.selectedId = null;
  h.context.thread = null;
  h.openSidebar();
  h.context.showWelcome({ startNew: true });
  await h.settle();
  assert.equal(h.context.creating, true);
  assert.equal(h.isSidebarOpen(), true, "自动初始化不能覆盖用户较新的展开操作");
  await h.context.startNewChat("", { focus: false });
  assert.equal(h.isSidebarOpen(), false, "用户明确点击新建时仍按原交互关闭侧栏");
});

test("切换会话等待缓存时重新展开侧栏，迟到缓存不得再关闭", async () => {
  const h = sideCreationNavigationHarness(), cache = deferred();
  Object.assign(h.context, {
    readScope: "cache-scope", threadEphemeralState: () => false,
    readCache: { get: () => cache.promise },
    optionalCachedRead: task => task, persistentThreadDisplayAllowed: () => true,
    notifiedQueue: null, notifiedQueueEdits: null, isMissingThreadError: () => false,
  });
  h.openSidebar();
  const navigation = h.context.selectThread("other");
  await h.settle();
  h.openSidebar();
  cache.resolve(null);
  await navigation;
  assert.equal(h.context.selectedId, "other");
  assert.equal(h.isSidebarOpen(), true);
  assert.equal(h.prompt.value, "C 的原有草稿");
  assert.equal(h.writes.length, 0);
});

test("显式新建等待项目读取时重新展开侧栏，迟到读取不得再关闭", async () => {
  const h = sideCreationNavigationHarness(), sidebarRead = deferred();
  h.context.loadSidebar = () => sidebarRead.promise;
  h.openSidebar();
  const creating = h.context.startNewChat(undefined, { focus: false });
  await h.settle();
  h.openSidebar();
  h.context.sidebar = { projects: [], recent: [], pinnedThreads: [] };
  sidebarRead.resolve();
  await creating;
  assert.equal(h.context.creating, true);
  assert.equal(h.isSidebarOpen(), true);
});

test("通知切换在侧边创建回执未到时立即解除面板，并保留两端草稿和原事务", async () => {
  const h = sideCreationNavigationHarness(), { context, workspace, prompt, historyEntries, disk } = h;
  const operation = h.start();
  assert.equal(workspace.inert, true);
  assert.equal(h.body.children.includes(operation.view.panel), true);
  context.openNotificationThread("other");
  await h.settle();
  assert.equal(context.selectedId, "other");
  assert.equal(prompt.value, "C 的原有草稿");
  assert.equal(prompt.disabled, false);
  assert.equal(workspace.inert, false, "创建回执未到时 C 就必须解除 inert，不能等收尾才关面板");
  assert.equal(context.sideCreationView, null);
  assert.equal(h.body.children.includes(operation.view.panel), false);
  assert.equal(operation.record.autoOpen, false);
  assert.equal(context.relatedThreadOperations.size, 1, "关闭面板不能取消或丢弃原创建事务");
  assert.equal(JSON.parse(disk.getItem("operations"))[0].autoOpen, false);
  assert.equal(context.drafts.get("main"), "M 的未发送草稿");
  assert.equal(context.skillDrafts.get("main")[0].name, "M-skill");
  assert.equal(context.attachmentDrafts.get("main")[0].name, "main.png");
  assert.equal(context.selectedSkills[0].name, "C-skill");
  assert.equal(context.composerAttachmentSnapshot()[0].name, "other.png");
  assert.equal(historyEntries.length, 3);
  for (const entry of historyEntries) {
    assert.equal(entry.state.creation, undefined, "不能把创建标记留在原条目或复制到 C");
    assert.deepEqual(entry.state.keep, { source: "unrelated" });
  }
  assert.equal(historyEntries[1].state.position, 4);
  assert.equal(historyEntries[2].state.position, 5);
  assert.equal(h.backCalls, 0);
  assert.equal(h.writes.length, 1);
  await h.complete(operation);
});

test("同一会话只有缓存历史但缺少设置时，再次选择会话会补读而非直接返回", async () => {
  const h = sideCreationNavigationHarness(), before = h.prompt.value;
  h.context.thread = null;
  h.context.transcriptChecking = true;
  let reads = 0;
  h.context.reloadThread = async () => { reads++; h.context.thread = { id: "main", title: "M" }; };
  await h.context.selectThread("main");
  assert.equal(reads, 1);
  assert.equal(h.prompt.value, before);
  assert.equal(h.writes.length, 0);
});

test("侧边创建迟到回执只登记原父关系，不回退通知会话或覆盖继续编辑的草稿", async () => {
  const h = sideCreationNavigationHarness(), { context, prompt } = h, operation = h.start();
  context.openNotificationThread("other");
  await h.settle();
  prompt.value = "C 在等待期间继续编辑";
  context.setComposerAttachments([{ name: "C-new.png" }]);
  context.rememberDraft();
  await h.complete(operation);
  assert.equal(h.backCalls, 0, "迟到成功不能借用创建页返回动作回退当前历史");
  assert.equal(context.location.hash, "#thread=other");
  assert.equal(context.selectedId, "other");
  assert.equal(prompt.value, "C 在等待期间继续编辑");
  assert.equal(context.drafts.get("other"), prompt.value);
  assert.equal(context.drafts.get("main"), "M 的未发送草稿");
  assert.equal(context.composerAttachmentSnapshot()[0].name, "C-new.png");
  assert.equal(context.attachmentDrafts.get("other")[0].name, "C-new.png");
  assert.equal(context.selectedSkills[0].name, "C-skill");
  assert.equal(context.sideCreationView, null);
  assert.equal(context.relatedSessionView, null);
  assert.equal(context.relatedThreadOperations.size, 0);
  assert.equal(context.relatedThreadViews.get("created-side").parentThreadId, "main");
  assert.equal(context.relatedThreadViews.get("created-side").kind, "side");
  assert.deepEqual(h.menuChanges.map(change => [change.parent, change.item.id]), [["main", "created-side"]]);
  assert.equal(h.writes.length, 1);
  assert.equal(h.writes[0].options.receiptId, operation.record.id);
});

test("正常侧边创建仍保留 preparedView 过渡并自动进入新侧边", async () => {
  const h = sideCreationNavigationHarness(), { context, workspace } = h, operation = h.start();
  await h.complete(operation);
  assert.equal(context.selectedId, "created-side");
  assert.equal(context.location.hash, "#thread=created-side");
  assert.equal(context.relatedSessionView.kind, "side");
  assert.equal(context.relatedSessionView.parentThreadId, "main");
  assert.equal(h.preparedStates[0].view, operation.view, "正常成功进入 S 时应保留面板到 onViewReady");
  assert.equal(h.preparedStates[0].inert, true);
  assert.equal(operation.record.autoOpen, true, "正常自动打开不能被误判为用户取消");
  assert.equal(context.sideCreationView, null);
  assert.equal(workspace.inert, false);
  assert.equal(h.historyEntries.length, 2, "成功使用创建页条目，不额外增加一层历史");
  assert.equal(context.history.state.creation, undefined);
  assert.equal(h.backCalls, 0);
  assert.equal(h.writes.length, 1);
});

test("侧边创建主动返回仍后退一次，迟到成功只登记关系且保留父会话草稿", async () => {
  const h = sideCreationNavigationHarness(), { context, workspace, prompt } = h, operation = h.start();
  context.requestSideCreationBack();
  assert.equal(h.backCalls, 1);
  assert.equal(context.sideCreationView, null);
  assert.equal(workspace.inert, false);
  assert.equal(operation.record.autoOpen, false);
  assert.equal(context.selectedId, "main");
  assert.equal(prompt.value, "M 的未发送草稿");
  await h.complete(operation);
  assert.equal(h.backCalls, 1);
  assert.equal(context.selectedId, "main");
  assert.equal(context.location.hash, "#thread=main");
  assert.equal(prompt.value, "M 的未发送草稿");
  assert.equal(context.relatedThreadViews.get("created-side").parentThreadId, "main");
  assert.equal(h.writes.length, 1);
});

test("离开创建父会话仅清理本页本记录的历史键，不删除其他记录或其他页面的标记", async () => {
  for (const mismatch of [{ owner: "other-page" }, { requestId: "other-operation" }]) {
    const h = sideCreationNavigationHarness(), { context } = h, operation = h.start();
    const foreign = { ...context.history.state.creation, ...mismatch };
    context.history.replaceState({ ...context.history.state, creation: foreign }, "", context.location.href);
    context.openNotificationThread("other");
    await h.settle();
    assert.equal(context.sideCreationView, null);
    assert.equal(h.workspace.inert, false);
    assert.deepEqual(context.history.state.creation, foreign);
    assert.deepEqual(context.history.state.keep, { source: "unrelated" });
    await h.complete(operation);
    assert.equal(h.backCalls, 0);
    assert.equal(context.selectedId, "other");
  }
});

function approvalAreaHarness(preceding = [], viewport = { offsetTop: 0, height: 900 }) {
  const styles = new Map(), anchor = { previousElementSibling: null,
    getBoundingClientRect: () => ({ top: 735 }) };
  for (const box of preceding) {
    anchor.previousElementSibling = {
      hidden: box.hidden === true, previousElementSibling: anchor.previousElementSibling,
      getBoundingClientRect: () => box,
    };
  }
  const root = { parentElement: anchor, childElementCount: 1,
    style: { setProperty: (key, value) => styles.set(key, value) } };
  const context = { $: () => root, window: { visualViewport: viewport, innerHeight: 900 } };
  vm.createContext(context);
  vm.runInContext(sourceSection("function fitApprovalArea()", "function refitComposerMenus("), context);
  return { context, root, anchor, styles, fit: () => context.fitApprovalArea(),
    bottom: () => anchor.getBoundingClientRect().top - 8 - parseFloat(styles.get("--approval-lift")) };
}

test("问答卡底部避让真实队列容器，不能覆盖队列菜单按钮", () => {
  // queue-dequeue-race/question-queue-overlap-before.json：队列 top=702、按钮中心 y=719、旧卡片 bottom=727。
  const queueContext = { top: 701, width: 760, height: 36 };
  const h = approvalAreaHarness([queueContext]);
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "34px");
  assert.equal(h.styles.get("--approval-available-height"), "685px");
  assert.equal(h.bottom(), 693);
  assert.ok(h.bottom() <= queueContext.top - 8, "卡片必须整体位于队列容器上方并保留间隔");
  assert.ok(h.bottom() < 719, "队列菜单按钮中心不能落入浮动问答卡");
});

test("问答卡同时避让目标、pending 条和项目选择，采用最上方可见内容", () => {
  for (const preceding of [
    [{ top: 560, width: 760, height: 100 }, { top: 660, width: 760, height: 30 }],
    [{ top: 640, width: 760, height: 40, childElementCount: 0 }],
    [{ top: 610, width: 760, height: 80 }],
  ]) {
    const h = approvalAreaHarness(preceding), firstTop = preceding[0].top;
    h.fit();
    assert.equal(h.bottom(), firstTop - 8);
    assert.equal(h.styles.get("--approval-available-height"), `${firstTop - 16}px`);
  }
});

test("无前置内容、空容器或隐藏元素保留原问答卡位置与视口高度限制", () => {
  const hidden = [
    { top: 400, width: 760, height: 60, hidden: true },
    { top: 500, width: 760, height: 0 },
    { top: 600, width: 0, height: 34 },
    { top: 0, width: 0, height: 0 },
  ];
  for (const preceding of [[], hidden]) {
    const h = approvalAreaHarness(preceding);
    h.fit();
    assert.equal(h.styles.get("--approval-lift"), "0px");
    assert.equal(h.styles.get("--approval-available-height"), "719px");
    h.context.window.visualViewport = { offsetTop: 120, height: 400 };
    h.fit();
    assert.equal(h.bottom(), 512);
    assert.equal(h.styles.get("--approval-available-height"), "384px");
    h.context.window.visualViewport = null;
    h.context.window.innerHeight = 600;
    h.fit();
    assert.equal(h.bottom(), 592);
    assert.equal(h.styles.get("--approval-available-height"), "584px");
  }
});

test("队列目标区域展开、收起或隐藏后重新定位问答卡，不留下旧抬升高度", () => {
  const content = { top: 701, width: 760, height: 36 }, h = approvalAreaHarness([content]);
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "34px");
  content.top = 560; content.height = 177;
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "175px");
  content.height = 0;
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "0px");
  h.anchor.previousElementSibling.hidden = true;
  content.height = 177;
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "0px");
  assert.equal(h.styles.get("--approval-available-height"), "719px");
});

test("问答卡无内容或没有定位锚点时不写布局，无可用视口空间时高度为零", () => {
  const h = approvalAreaHarness([{ top: 100, width: 760, height: 80 }], { offsetTop: 200, height: 300 });
  h.fit();
  assert.equal(h.styles.get("--approval-available-height"), "0px");
  assert.equal(h.bottom(), 92);
  h.styles.clear(); h.root.childElementCount = 0;
  h.fit();
  assert.equal(h.styles.size, 0);
  h.root.childElementCount = 1; h.root.parentElement = null;
  h.fit();
  assert.equal(h.styles.size, 0);
});

function sharedOrdinaryDraftHarness(initial = null) {
  const empty = () => ({ text: "", skills: [], attachments: [], goalMode: null });
  const disk = new Map(initial ? [["main", structuredClone(initial)]] : []), writes = [];
  const scope = '["shared-device",1]';
  const page = (name, loadedDraft = null) => {
    const loaded = structuredClone(loadedDraft || empty()), prompt = { value: loaded.text, querySelector: () => null };
    const context = {
      selectedId: "main", creating: false, creationProject: "", session: { device: { id: "shared-device", createdAtMs: 1 } },
      recoveryScope: scope, recoveryGeneration: 0, draftStorageError: "", draftWrites: new Map(),
      drafts: new Map(), skillDrafts: new Map(), attachmentDrafts: new Map(), goalDrafts: new Map(),
      selectedSkills: loaded.skills, selectedGoal: loaded.goalMode, currentAttachments: loaded.attachments,
      editingQueuedMessage: null, pageLeaving: false, stream: null, streamRetry: null, sessionMenu: null,
      $: () => prompt, clearTimeout, randomId: () => `${name}-changed-goal`,
      composerAttachmentSnapshot: () => context.currentAttachments.map(image => ({ ...image })),
      syncSelectedSkillsFromPrompt() {}, forgetPromptCanonicalText() {}, syncPromptTrailingBreak() {},
      updateComposer() {}, closeSkillMenu() {}, updateSkillMenu: async () => {}, skillIndex: 0,
      stopActivityTimer() {}, stopGoalLiveTimer() {}, finishQueueDrag() {}, stopRelatedSessionTimers() {},
      rememberTranscript() {}, rememberReadingPosition() {}, closeContextUsageTip() {}, retireForegroundReads() {},
      stopCatalogRefresh() {}, stopConnectionPoll() {}, cancelConnectionProbe() {},
      notice() { assert.fail("内存恢复存储不应报错"); },
      recoveryStorageMessage: error => error.message,
      recoveryStore: {
        async put(owner, records) {
          assert.equal(owner, scope);
          for (const record of records) {
            assert.equal(record.kind, "draft");
            const value = structuredClone(record.value);
            disk.set(record.id, value); writes.push({ page: name, kind: "put", value });
          }
        },
        async remove(owner, kind, key) {
          assert.equal(owner, scope); assert.equal(kind, "draft");
          disk.delete(key); writes.push({ page: name, kind: "remove" });
        },
      },
    };
    if (loadedDraft) {
      context.drafts.set("main", loaded.text); context.skillDrafts.set("main", loaded.skills);
      context.attachmentDrafts.set("main", loaded.attachments);
      if (loaded.goalMode) context.goalDrafts.set("main", loaded.goalMode);
    }
    vm.createContext(context);
    vm.runInContext([
      sourceSection("const skillIdentity =", "const skillNodeKey ="),
      sourceSection("function snapshotDraft()", "function privateInputScope("),
      sourceSection("function rememberDraft()", "function restoreDraft("),
      sourceSection("function handlePromptInput(", 'document.addEventListener("pointerdown",'),
      sourceSection("function suspendPage()", "function resumePage()"),
    ].join("\n"), context);
    const settled = () => Promise.all([...context.draftWrites.values()].map(write => write.promise));
    const input = async value => {
      prompt.value = value;
      context.handlePromptInput({ inputType: "insertText" });
      await settled();
    };
    return { context, prompt, input, settled };
  };
  return { disk, writes, page, empty };
}

test("同scope旧空稿页面卸载或切换不删除另一页已持久化的新稿", async () => {
  for (const leaving of ["unload", "switch"]) {
    const shared = sharedOrdinaryDraftHarness(), a = shared.page("A"), b = shared.page("B");
    await a.input("QUEUE_EDIT_UNSENT_AFTER_REFRESH_保留");
    assert.equal(b.context.drafts.has("main"), false);
    if (leaving === "unload") b.context.suspendPage();
    else b.context.rememberDraft();
    await b.settled();
    assert.equal(shared.disk.get("main")?.text, "QUEUE_EDIT_UNSENT_AFTER_REFRESH_保留", leaving);
    assert.equal(shared.writes.some(write => write.page === "B"), false, leaving);
    assert.equal(a.prompt.value, "QUEUE_EDIT_UNSENT_AFTER_REFRESH_保留");
  }
});

test("同scope旧非空稿页面未编辑时卸载或切换不覆盖另一页的新稿", async () => {
  const original = { text: "两页最初共同恢复的原稿", skills: [], attachments: [], goalMode: null };
  for (const leaving of ["unload", "switch"]) {
    const shared = sharedOrdinaryDraftHarness(original), a = shared.page("A", original), b = shared.page("B", original);
    await a.input("A后来编辑的新稿");
    if (leaving === "unload") b.context.suspendPage();
    else b.context.rememberDraft();
    await b.settled();
    assert.equal(shared.disk.get("main")?.text, "A后来编辑的新稿", leaving);
    assert.equal(shared.writes.some(write => write.page === "B"), false, leaving);
    assert.equal(b.prompt.value, original.text);
  }
});

test("同scope页面主动清空自己的普通草稿仍删除持久记录，卸载不重复保存", async () => {
  const original = { text: "用户准备主动清空的草稿", skills: [], attachments: [], goalMode: null };
  const shared = sharedOrdinaryDraftHarness(original), b = shared.page("B", original);
  await b.input("");
  assert.equal(shared.disk.has("main"), false);
  assert.deepEqual(shared.writes, [{ page: "B", kind: "remove" }]);
  b.context.suspendPage();
  await b.settled();
  assert.equal(shared.writes.length, 1);
});

test("普通输入及Skill位置、附件和目标模式的实际改动继续持久化，未改动离开不重写", async () => {
  const shared = sharedOrdinaryDraftHarness(), a = shared.page("A");
  await a.input("普通输入");
  assert.equal(shared.disk.get("main").text, "普通输入");
  a.context.selectedSkills = [{ name: "chosen", path: "/test/SKILL.md", _offset: 0 }];
  a.context.rememberDraft(); await a.settled();
  a.context.selectedSkills = [{ ...a.context.selectedSkills[0], _offset: 2 }];
  a.context.rememberDraft(); await a.settled();
  assert.equal(shared.disk.get("main").skills[0]._offset, 2);
  a.context.currentAttachments = [{ kind: "inline", name: "draft.png", mime: "image/png",
    dataUrl: "data:image/png;base64,ZHJhZnQ=", size: 5 }];
  a.context.rememberDraft(); await a.settled();
  assert.equal(shared.disk.get("main").attachments[0].dataUrl, a.context.currentAttachments[0].dataUrl);
  a.context.selectedGoal = "goal-draft";
  a.context.rememberDraft(); await a.settled();
  assert.equal(shared.disk.get("main").goalMode, "goal-draft");
  const count = shared.writes.length;
  a.context.rememberDraft(); a.context.suspendPage(); await a.settled();
  assert.equal(shared.writes.length, count);
});

function threadDisplayCacheHarness(existing = null) {
  const persisted = [];
  const context = existing || vm.createContext({
    session: { device: { id: "display-device", createdAtMs: 1 } },
    selectedId: "main", thread: null, relatedSessionView: null, recentHistoryPage: null,
    relatedThreadViews: new Map(), normalizeRelatedView: () => null,
    privateInputScope: value => JSON.stringify([value.device.id, value.device.createdAtMs]),
    validRelatedThreadId: value => typeof value === "string" && /^[A-Za-z0-9_-]+$/.test(value),
    threadDeleted: () => false,
  });
  Object.assign(context, {
    threadDisplays: new Map(), contextUsage: () => null, syncRelatedModelDisplay() {},
    readCache: { put: async (scope, kind, id, value) => {
      persisted.push({ scope, kind, id, value: structuredClone(value) });
    } },
  });
  context.recentHistoryPage ??= null;
  context.notifiedQueue ??= null;
  context.notifiedQueueEdits ??= null;
  context.discardPersistentThreadDisplay ||= () => {};
  if (!Object.hasOwn(context, "readScope")) context.readScope = context.privateInputScope(context.session);
  vm.runInContext([
    sourceSection("function threadEphemeralState(", "function discardPersistentThreadDisplay("),
    sourceSection("function persistentThreadDisplayAllowed(", "function syncRelatedModelDisplay("),
    sourceSection("function rememberThreadDisplay(", "function ownsWorkspaceCache("),
    sourceSection("function queueSnapshotIsVersioned(", "function queueMessageIdentity("),
  ].join("\n"), context);
  return { context, persisted };
}

function cachedControlsHarness() {
  const { context, persisted } = threadDisplayCacheHarness();
  const element = (tag, className = "", text = "") => ({
    tag, className, text, dataset: {}, attributes: {}, children: [], hidden: false,
    append(...children) { this.children.push(...children); },
    replaceChildren(...children) { this.children = children; },
    setAttribute(key, value) { this.attributes[key] = value; },
    get firstElementChild() { return this.children[0] || null; },
    querySelectorAll(selector) {
      const className = selector.slice(1);
      return this.children.flatMap(child => [
        ...(child.className?.split(" ").includes(className) ? [child] : []),
        ...(child.querySelectorAll?.(selector) || []),
      ]);
    },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
  });
  const goal = element("section"), queue = element("section");
  Object.assign(context, {
    element, icon: name => element("svg", name),
    button: (text, label, action, className) => Object.assign(element("button", className, text), { action }),
    $: (selector, root) => root ? root.querySelector(selector)
      : selector === "#goal-area" ? goal : selector === "#queued-area" ? queue : null,
    threadReadOnly: () => false, connected: true, busy: false, stopping: false, pending: null,
    queuedActionBusy: new Set(), editingQueuedMessage: null, editingMessage: null, queueReorder: null,
    queueDrag: null, queuedEditCheckVersion: 0, compactionLocked: () => false,
    goalEditPending: null, goalEditDraft: null, renderGoalEditor() {}, updateGoalLiveView() {}, stopGoalLiveTimer() {},
    syncPendingFeedbackTimer() {}, pendingFeedbackReady: () => true,
    editableGoalStatuses: ["active", "paused", "blocked", "budgetLimited", "usageLimited"],
    queueMessageText: message => message.text || message.context?.prompt || "",
  });
  vm.runInContext([
    sourceSection("function queueReorderAvailable()", "function applyQueueReorderResult("),
    sourceSection("function renderQueuedMessages(", "async function queuedEditFingerprint("),
    sourceSection("const goalStatusLabels =", "function formatGoalTokens("),
    sourceSection("function renderGoalStatus()", "function blockingRequestState("),
  ].join("\n"), context);
  const display = { id: "main", model: "test-model", ephemeral: false,
    goal: { objective: "已有目标立即显示", status: "active", createdAt: 1 },
    queuedMessages: [{ id: "queued-a", text: "已有排队消息", clientUserMessageId: "client-a", createdAt: 1 }] };
  const render = () => { context.renderGoalStatus(); context.renderQueueArea(); };
  return { context, persisted, goal, queue, display, render };
}

test("会话尚未返回时立即渲染缓存目标和队列，相同内容不重建区域", () => {
  const { context, goal, queue, display, render } = cachedControlsHarness();
  context.rememberThreadDisplay(display);
  render();
  assert.equal(context.thread, null, "展示缓存不能伪装为已读取的实时会话");
  assert.equal(goal.hidden, false);
  assert.equal(goal.querySelector(".goal-preview").text, display.goal.objective);
  assert.equal(queue.querySelector(".queued-message-text").text, display.queuedMessages[0].text);
  assert.equal(queue.querySelector(".queued-steer-button").disabled, true);
  assert.equal(goal.querySelector(".goal-edit-button").disabled, true);
  const previousGoal = goal.firstElementChild, previousQueue = queue.firstElementChild;
  render();
  assert.equal(goal.firstElementChild, previousGoal);
  assert.equal(queue.firstElementChild, previousQueue);
  context.thread = structuredClone(display);
  render();
  assert.equal(queue.querySelector(".queued-steer-button").disabled, false);
  assert.equal(goal.querySelector(".goal-edit-button").disabled, false);
});

test("详情缺少队列字段时保留展示，确认空队列和空目标后不能复活旧缓存", () => {
  const { context, goal, queue, display, render } = cachedControlsHarness();
  context.rememberThreadDisplay(display);
  context.thread = { id: "main", model: "fresh-model" };
  context.rememberThreadDisplay();
  render();
  assert.equal(context.currentThreadDisplay().model, "fresh-model");
  assert.equal(goal.hidden, false);
  assert.equal(queue.hidden, false);
  context.thread = { id: "main", goal: null, queuedMessages: [] };
  context.rememberThreadDisplay();
  render();
  assert.equal(goal.hidden, true);
  assert.equal(queue.hidden, true);
  context.thread = null;
  render();
  assert.equal(goal.hidden, true);
  assert.equal(queue.hidden, true);
});

test("查看侧边期间更新父会话缓存，返回时显示最新目标与队列", () => {
  const { context, goal, queue, display, render } = cachedControlsHarness();
  context.rememberThreadDisplay(display);
  context.selectedId = "side";
  context.thread = { id: "side", ephemeral: true };
  context.rememberBackgroundThreadDisplay({ method: "thread/queue/changed" }, {
    threadId: "main", queuedMessages: [{ id: "queued-b", text: "后台新增排队消息" }],
  });
  context.rememberBackgroundThreadDisplay({ method: "thread/goal/updated" }, {
    threadId: "main", goal: { ...display.goal, objective: "后台修改后的目标", status: "paused" },
  });
  assert.equal(context.currentThreadDisplay().id, "side");
  context.selectedId = "main"; context.thread = null;
  render();
  assert.equal(goal.querySelector(".goal-preview").text, "后台修改后的目标");
  assert.equal(queue.querySelector(".queued-message-text").text, "后台新增排队消息");
  context.selectedId = "side";
  context.rememberBackgroundThreadDisplay({ method: "thread/goal/cleared" }, { threadId: "main" });
  context.rememberBackgroundThreadDisplay({ method: "thread/queue/changed" }, { threadId: "main", queuedMessages: [] });
  context.selectedId = "main";
  render();
  assert.equal(goal.hidden, true);
  assert.equal(queue.hidden, true);
});

test("后台迟到的旧队列事件不能覆盖已清空的较新展示快照", () => {
  const { context, display } = cachedControlsHarness();
  context.rememberThreadDisplay({ ...display, queuedMessages: [],
    queueSnapshotVersion: { queueEpoch: "native-a", queueRevision: 20 } });
  context.selectedId = "side";
  for (const version of [{ queueEpoch: "native-a", queueRevision: 19 }, {}])
    context.rememberBackgroundThreadDisplay({ method: "thread/queue/changed" }, {
      threadId: "main", queuedMessages: display.queuedMessages, ...version,
    });
  context.selectedId = "main";
  assert.equal(context.currentThreadDisplay().queuedMessages.length, 0);
});

// 操作反馈延迟：只验证 UI 提示时机，不调用真实会话或新增查询轮询。
function delayFeedbackHarness() {
  let now = 100000, nextTimer = 0;
  const timers = new Map(), notices = [], disk = storage(), area = {
    children: [], replaceChildren() { this.children = []; }, append(node) { this.children.push(node); },
  };
  disk.removeItem = key => disk.records.delete(key);
  const element = (tag, className = "", text = "") => ({
    tag, className, text, children: [], attributes: {},
    get textContent() { return this.text; }, set textContent(value) { this.text = value; },
    append(...nodes) { for (const node of nodes) { node.parent = this; this.children.push(node); } },
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(node => node !== this); },
    setAttribute(key, value) { this.attributes[key] = value; },
    querySelectorAll(selector) {
      const matches = node => selector.startsWith(".") ? node.className?.split(" ").includes(selector.slice(1))
        : selector.split(",").includes(node.tag);
      return this.children.flatMap(node => [...(matches(node) ? [node] : []), ...(node.querySelectorAll?.(selector) || [])]);
    },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
  });
  const context = {
    Date: class extends Date { static now() { return now; } },
    setTimeout(callback, delay) { const id = ++nextTimer; timers.set(id, { callback, at: now + delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
    session: { device: { id: "device-a", createdAtMs: 1 }, capabilities: { compactionQueueSend: true }, allowApprovals: true },
    selectedId: "main", selectionVersion: 1, pageLeaving: false, document: { hidden: false },
    pending: null, pendingTimer: null, sending: null, connected: true, busy: false,
    threadDeletions: new Map(), threadDeletionChecks: new Set(), goalEditPending: null, relatedThreadOperations: new Map(),
    goalComposerRecoveries: new Map(), approvalReplies: new Map(), questionReplyReceipts: new Map(),
    questionDrafts: new Map(), compactions: new Map(),
    sideCreationView: null, relatedSessionView: null, unsupportedProjectless: false, goalMode: false, goalReason: "",
    recoveryScope: null, sessionStorage: disk, approvalSignature: "previous",
    queuedEditActions: new Set(["beginQueuedEdit", "editQueued", "cancelQueuedEdit", "dismissQueuedEdit"]),
    privateInputScope: value => JSON.stringify([value?.device.id, value?.device.createdAtMs]),
    threadDeletionKey: id => JSON.stringify(["device-a", id]),
    currentCompaction: id => context.compactions.get(id || context.selectedId) || { state: "idle" },
    compactionLocked: id => ["pending", "running", "unknown"].includes(context.currentCompaction(id).state),
    ownsRelatedOperation: record => record.scope === context.privateInputScope(context.session),
    goalRecordThreadId: record => record.payload.threadId,
    goalComposerAction: (_action, payload) => payload?.goalMode === true,
    goalComposerStorageRecord: record => record,
    renderQueuedEditRecoveryControls: node => node.append({ label: "查回执" }),
    compactionQueueSendReason: () => "", threadReadOnly: () => false,
    element, button: (label, aria, action) => Object.assign(element("button", "", label), { label, aria, action }), $: () => area,
    notice: (message, error = false) => notices.push({ message, error }),
    renderApprovals() {}, reloadThread: async () => {}, reconcileCompaction() {},
    resolvePending: () => assert.fail("显示提示不能查询回执"),
    resolveRelatedOperation: () => assert.fail("显示提示不能查询侧边会话回执"),
    resolveCompactionReceipt: () => assert.fail("显示提示不能查询压缩回执"),
    setCompactionState: (id, value) => context.compactions.set(id, value),
  };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("const pendingFeedbackDelayMs =", "function updateComposer("),
    sourceSection("function savePending(", "function updateGoalComposerPending("),
    sourceSection("function renderRelatedOperationStatus(", "function threadEphemeralState("),
    sourceSection("function renderAsyncQuestion(", "function commandApprovalPresentation("),
    sourceSection("function updateBlockingApprovalControls(", "function renderApprovals("),
    sourceSection("function renderCompactionBanner(", "function dismissGoalComposerRecovery("),
    sourceSection("async function applyCompactionReceipt(", "async function resolveCompactionReceipt("),
    `function renderFeedback(){${sourceSection("  const deletion = threadDeletions.get(threadDeletionKey(selectedId));",
      "  renderFailedInputRecovery(")}}`,
  ].join("\n"), context);
  context.updateComposer = () => {
    area.replaceChildren();
    context.renderFeedback();
    context.renderRelatedOperationStatus(area);
    context.renderCompactionBanner(area, context.selectedId, context.currentCompaction());
    context.syncPendingFeedbackTimer();
  };
  const advance = async milliseconds => {
    const target = now + milliseconds;
    while (true) {
      const next = [...timers].filter(([, timer]) => timer.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) break;
      now = next[1].at; timers.delete(next[0]); next[1].callback();
      await Promise.resolve();
    }
    now = target;
  };
  const pending = action => {
    const record = { id: `receipt-${action}`, action, deviceId: "device-a", time: now,
      payload: { threadId: "main", messageId: "q", editId: "edit", ...(action === "setGoal" ? { goalMode: true } : {}) },
      draft: { text: "保留草稿" } };
    context.savePending(record);
    context.updateComposer();
    return record;
  };
  return { context, timers, notices, area, advance, pending, now: () => now };
}

test("快速操作完成不出现额外回执提示，也不留下延时刷新", async () => {
  const h = delayFeedbackHarness();
  const record = h.pending("steerQueued");
  assert.equal(h.area.children.length, 0);
  assert.equal(h.timers.size, 1);
  await h.advance(100);
  h.context.savePending(null);
  h.context.updateComposer();
  assert.equal(h.timers.size, 0);
  await h.advance(6000);
  assert.equal(h.area.children.length, 0);
  assert.equal(record.draft.text, "保留草稿");
  assert.deepEqual(h.notices, []);
});

test("各类未确认操作到五秒才出现简短提示，原草稿与回执锁保持", async () => {
  for (const action of ["send", "steerQueued", "removeQueued", "reorderQueued",
    "beginQueuedEdit", "editQueued", "cancelQueuedEdit", "dismissQueuedEdit", "setGoal"]) {
    const h = delayFeedbackHarness(), record = h.pending(action);
    await h.advance(4999);
    assert.equal(h.area.children.length, 0, action);
    await h.advance(1);
    assert.equal(h.area.children.length, 1, action);
    assert.match(h.area.children[0].text, /^正在确认.+结果。$/);
    assert.equal(h.context.pending, record, "延时反馈不能解除未知回执锁");
    assert.equal(record.draft.text, "保留草稿");
    assert.equal(h.timers.size, 0, "只重绘一次，没有新增轮询");
    assert.deepEqual(h.notices, []);
  }
});

test("侧边操作、删除和压缩复用同一个五秒 UI 提示定时器", async () => {
  for (const kind of ["side-create", "side-close", "delete", "compact"]) {
    const h = delayFeedbackHarness(), record = { id: kind, time: h.now(), state: "pending",
      deviceId: "device-a", threadId: "main" };
    if (kind.startsWith("side")) h.context.relatedThreadOperations.set(kind, {
      ...record, parentThreadId: "main", scope: h.context.privateInputScope(h.context.session),
      action: kind === "side-create" ? "createSideThread" : "closeSideThread",
    });
    else if (kind === "delete") h.context.threadDeletions.set(h.context.threadDeletionKey("main"), record);
    else h.context.compactions.set("main", { ...record, receiptId: "compact-receipt" });
    h.context.updateComposer();
    assert.equal(h.area.children.length, 0, kind);
    assert.equal(h.timers.size, 1, kind);
    await h.advance(5000);
    assert.equal(h.area.children.length, 1, kind);
    assert.match(h.area.children[0].text, /^正在确认.+结果。$/);
    assert.equal(h.timers.size, 0, kind);
  }
});

test("切换、离开页面或更换授权后旧定时器不能显示迟到提示", async () => {
  for (const change of ["selection", "authorization", "page", "hidden"]) {
    const h = delayFeedbackHarness(), record = h.pending("steerQueued");
    if (change === "selection") ++h.context.selectionVersion;
    else if (change === "authorization") h.context.session.device.createdAtMs = 2;
    else if (change === "page") h.context.pageLeaving = true;
    else h.context.document.hidden = true;
    await h.advance(5000);
    assert.equal(h.area.children.length, 0, change);
    assert.equal(h.timers.size, 0, change);
    assert.equal(h.context.pending, record);
  }
});

test("明确失败在五秒内立即提示，已确认压缩不继续显示未确认横幅", async () => {
  const h = delayFeedbackHarness();
  h.context.compactions.set("main", { state: "pending", receiptId: "compact-receipt", time: h.now() });
  h.context.updateComposer();
  await h.context.applyCompactionReceipt("main", "compact-receipt", {
    state: "failed", result: { message: "压缩请求明确失败" },
  });
  assert.deepEqual(h.notices, [{ message: "压缩请求明确失败", error: true }]);
  assert.equal(h.timers.size, 0);
  h.context.compactions.set("main", { state: "pending", receiptId: "other", time: h.now(), acknowledged: true });
  h.context.updateComposer();
  await h.advance(5000);
  assert.equal(h.area.children.length, 0);
});

test("问答先显示提交按钮状态，五秒后才增加中性说明且保留原回答回执", async () => {
  for (const skip of [false, true]) {
    const h = delayFeedbackHarness(), key = JSON.stringify(["device-a", "main", "question"]);
    const request = { id: "question", params: { questions: [{ id: "field", question: "问题" }] } };
    const record = { id: "answer-receipt", deviceId: "device-a", threadId: "main",
      requestId: "question", state: "unknown", time: h.now(), skip };
    h.context.questionReplyReceipts.set(key, record);
    let card;
    h.context.renderApprovals = () => {
      card = h.context.element("section");
      h.context.renderAsyncQuestion(card, request, "main");
    };
    h.context.renderApprovals(); h.context.updateComposer();
    assert.equal(card.querySelector(".async-question-status").textContent, "");
    assert.match(card.querySelectorAll("button").find(node => /中…$/.test(node.textContent)).textContent,
      skip ? /^跳过中/ : /^提交中/);
    await h.advance(5000);
    assert.match(card.querySelector(".async-question-status").textContent, /^正在确认.+结果。$/);
    assert.equal(h.context.questionReplyReceipts.get(key), record);
  }
});

test("审批等待期间保留控件锁，五秒后增加核对入口，不解除原授权状态", async () => {
  const h = delayFeedbackHarness(), key = "approval", card = h.context.element("section");
  card.append(h.context.button("允许一次", "允许一次", () => assert.fail("不能重新提交")));
  const record = { deviceId: "device-a", threadId: "main", state: "unknown", time: h.now() };
  h.context.approvalReplies.set(key, record);
  h.context.renderApprovals = () => h.context.updateBlockingApprovalControls(card, key);
  h.context.renderApprovals(); h.context.updateComposer();
  assert.equal(card.querySelector("button").disabled, true);
  assert.equal(card.querySelector(".remote-approval-reply-status"), null);
  await h.advance(5000);
  assert.match(card.querySelector(".remote-approval-reply-status").children[0].textContent, /^正在确认操作结果。$/);
  assert.equal(card.querySelector("button").disabled, true);
  assert.equal(h.context.approvalReplies.get(key), record);
});

test("目标队列缓存按授权隔离，临时侧边仅留内存，缓存不携带审批和权限", () => {
  const { context, persisted } = threadDisplayCacheHarness();
  context.rememberThreadDisplay({ id: "main", goal: { objective: "目标", status: "paused" },
    queuedMessages: [{ id: "q", text: "排队" }], requests: [{ id: "approval" }],
    readOnly: false, status: { type: "active" } });
  const saved = context.currentThreadDisplay();
  assert.equal(saved.requests, undefined);
  assert.equal(saved.readOnly, undefined);
  assert.equal(saved.status, undefined);
  assert.equal(persisted[0].value.queuedMessages[0].text, "排队");
  context.rememberThreadDisplay({ id: "side", ephemeral: true, goal: null, queuedMessages: [] });
  assert.equal(persisted.length, 1);
  context.session = { device: { id: "other-device", createdAtMs: 2 } };
  assert.equal(context.currentThreadDisplay(), null);
});

test("从侧边返回时不等待父会话网络响应，迟到侧边详情不能覆盖父会话缓存", async () => {
  const h = sideCreationNavigationHarness(), { context } = threadDisplayCacheHarness(h.context);
  const reads = [], frames = [];
  context.thread = { id: "main", title: "M", goal: { objective: "主目标", status: "paused" },
    queuedMessages: [{ id: "main-q", text: "主会话排队" }] };
  context.rememberTranscript = () => context.rememberThreadDisplay();
  context.prepareRelatedSessionExit = () => null;
  context.updateComposer = () => frames.push(structuredClone(context.currentThreadDisplay()));
  context.api = (path, payload) => {
    const pending = deferred(); reads.push({ path, payload, ...pending }); return pending.promise;
  };
  const view = context.rememberRelatedView("side", "side", "main", "侧边", { ephemeral: true });
  const opening = context.selectThread("side", { relatedView: view });
  await h.settle();
  const returning = context.returnToParentSession();
  await h.settle();
  assert.equal(context.selectedId, "main");
  assert.equal(context.thread, null);
  assert.equal(frames.at(-1).goal.objective, "主目标");
  assert.equal(frames.at(-1).queuedMessages[0].text, "主会话排队");
  assert.deepEqual(reads.map(read => read.payload.threadId), ["side", "main"]);
  reads[1].resolve({ thread: { id: "main", title: "M", goal: null, queuedMessages: [] } });
  await returning;
  reads[0].resolve({ thread: { id: "side", title: "迟到侧边", goal: { objective: "不能覆盖", status: "active" },
    queuedMessages: [{ id: "side-q", text: "不能覆盖" }] } });
  await opening;
  assert.equal(context.selectedId, "main");
  assert.equal(context.currentThreadDisplay().goal, null);
  assert.equal(context.currentThreadDisplay().queuedMessages.length, 0);
});

test("没有内存记录时先恢复本地展示缓存，再等待会话网络刷新", async () => {
  const h = sideCreationNavigationHarness(), { context } = threadDisplayCacheHarness(h.context);
  const response = deferred(), frames = [];
  context.selectedId = "other"; context.thread = { id: "other" };
  context.readScope = context.privateInputScope(context.session);
  context.optionalCachedRead = task => task;
  context.readCache.get = async (_scope, kind, id) => kind === "thread" && id === "main"
    ? { id: "main", ephemeral: false, model: "cached-model",
      goal: { objective: "本地保存的目标", status: "paused" },
      queuedMessages: [{ id: "disk-q", text: "本地保存的排队消息" }] } : null;
  context.updateComposer = () => frames.push(structuredClone(context.currentThreadDisplay()));
  context.api = () => response.promise;
  const opening = context.selectThread("main");
  await h.settle();
  assert.equal(context.thread, null);
  assert.equal(frames.at(-1).goal.objective, "本地保存的目标");
  assert.equal(frames.at(-1).queuedMessages[0].text, "本地保存的排队消息");
  response.resolve({ thread: { id: "main", title: "M", goal: null, queuedMessages: [] } });
  await opening;
  assert.equal(context.currentThreadDisplay().goal, null);
  assert.equal(context.currentThreadDisplay().queuedMessages.length, 0);
});
