import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import nodeVm from "node:vm";
import { createSidebarPreferenceStore } from "../assets/remote-access/recovery-store.js";
import { createSelectComponent } from "../assets/remote-access/select.js";

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
const native = await readFile(new URL("../assets/remote-access/native.js", import.meta.url), "utf8");
const channels = await readFile(new URL("../assets/remote-access/channels.js", import.meta.url), "utf8");
const diagnostics = await readFile(new URL("../assets/remote-access/diagnostics.js", import.meta.url), "utf8");
const platform = await readFile(new URL("../assets/remote-access/platform.js", import.meta.url), "utf8");
const eventsClient = await readFile(new URL("../assets/remote-access/events-client.js", import.meta.url), "utf8");
const sessionTools = await readFile(new URL("../assets/remote-access/session-tools.js", import.meta.url), "utf8");
const sessionFiles = await readFile(new URL("../assets/remote-access/session-files.js", import.meta.url), "utf8");
const sessionMenuSource = await readFile(new URL("../assets/remote-access/session-menu.js", import.meta.url), "utf8");
const markdownRendererSource = await readFile(new URL("../assets/remote-access/markdown-renderer.js", import.meta.url), "utf8");
const devServerSource = await readFile(new URL("./dev-remote-access.mjs", import.meta.url), "utf8");

test("设置 Select 的调试代理只放行两个固定设置接口，不扩大到任意路径", () => {
  const context = nodeVm.createContext();
  nodeVm.runInContext(devServerSource.slice(devServerSource.indexOf("function allowedApi("),
    devServerSource.indexOf("function mappedCookie(")), context);
  for (const path of ["/api/settings/environments", "/api/settings/instructions"])
    assert.equal(context.allowedApi("POST", path), true);
  for (const [method, path] of [["GET", "/api/settings/instructions"], ["POST", "/api/settings/other"],
    ["POST", "/api/settings/instructions/extra"], ["DELETE", "/api/settings/instructions"], ["POST", "/settings/instructions"]])
    assert.equal(context.allowedApi(method, path), false);
});
// 旧回归用例只加载待测函数；补齐来自真实页面源码的后端上下文，默认仍使用 Codex。
const backendHelpers = app.slice(app.indexOf("function availableSessionInstances("), app.indexOf("function syncBackendModels("))
  + app.slice(app.indexOf("function conversationBackend("), app.indexOf("const projectThreadLimits ="));
const vm = {
  ...nodeVm,
  createContext(context = {}, options) {
    for (const [key, value] of Object.entries({
      creating: false, creationBackend: "codex", creationProject: "", selectedId: null, thread: null,
      creationInstanceId: "codex:local", sessionInstanceRequest: null,
      sessionTypes: [{ id: "codex", label: "Codex" }, { id: "claude", label: "Claude" }],
      sessionInstances: [{ id: "codex:local", backend: "codex", label: "Codex · 本机", ready: true, legacyDefault: true },
        { id: "claude:wsl:test", backend: "claude", label: "Claude · WSL · test", ready: true, legacyDefault: true }],
      drafts: new Map(), attachmentDrafts: new Map(),
      session: null, stream: null, EventSource: { OPEN: 1 }, modelsScope: "codex", modelLoadError: "",
      selectionVersion: 0,
      syncBackendModels() {}, setConnection() {},
      claudeUsageState: null, resetClaudeUsage() {}, syncClaudeUsage() {}, fitClaudeUsageTip() {},
      appearance: { sessionTypes: { codex: true, claude: false }, sessionFilter: "all" },
      transcriptEvents: new Map(), claudeHistoryOrder: null,
    })) if (!Object.hasOwn(context, key)) context[key] = value;
    context.appearance.sessionTypes ??= { codex: true, claude: false };
    context.appearance.sessionFilter ??= "all";
    const result = nodeVm.createContext(context, options);
    nodeVm.runInContext(backendHelpers, result);
    nodeVm.runInContext(app.slice(app.indexOf("function compactionInProgress("),
      app.indexOf("function compactionQueueSendReason(")), result);
    nodeVm.runInContext(app.slice(app.indexOf("function retireClaudeHistoryTurns("),
      app.indexOf("function pruneCachedHistory(")), result);
    return result;
  },
};

test("Codex/Claude 图片添加和草稿/队列恢复完整保留八张，第九张拒绝且不丢原图", async () => {
  const source = await readFile(new URL("../assets/remote-access/attachments.js", import.meta.url), "utf8");
  const notices = [];
  class Reader {
    readAsDataURL(blob) {
      void blob.arrayBuffer().then(bytes => {
        this.result = `data:${blob.type};base64,${Buffer.from(bytes).toString("base64")}`;
        this.onload();
      });
    }
  }
  const context = vm.createContext({ Blob, FileReader: Reader, Uint8Array,
    createImageBitmap: async () => ({ width: 1, height: 1, close() {} }),
    atob, sha256Hex: async () => "test-hash" });
  vm.runInContext(source.replace(/^import .*;\r?\n/gm, "").replace(/^export /gm, ""), context);
  context.report = message => notices.push(message);
  vm.runInContext("showNotice = report", context);
  const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==", "base64");
  for (const backend of ["codex", "claude"]) {
    context.backend = backend;
    context.files = Array.from({ length: 8 }, (_, i) => Object.assign(new Blob([png], { type: "image/png" }), { name: `${i}.png` }));
    vm.runInContext("clearComposerAttachments()", context);
    await vm.runInContext("addFiles(files)", context);
    const payload = vm.runInContext("composerAttachmentsPayload()", context);
    assert.equal(payload.length, 8, `${backend} 发送不能截为四张`);
    assert.equal(vm.runInContext("setComposerAttachments(composerAttachmentSnapshot()); composerAttachmentsPayload().length", context), 8);
    await vm.runInContext("addFiles([files[0]])", context);
    assert.match(notices.at(-1), /最多添加 8 张图片/);
    assert.deepEqual(vm.runInContext("composerAttachmentsPayload()", context), payload);
    context.queued = { context: { imageAttachments: payload.map(image => ({ src: image.dataUrl, filename: image.name })) } };
    await vm.runInContext("attachmentsFromQueuedMessage(queued).then(images => setComposerAttachments(images, {readOnly:true,removableRemote:true}))", context);
    assert.equal(vm.runInContext("composerAttachmentSnapshot().length", context), 8);
    assert.equal(vm.runInContext("composerRemoteAttachmentSources().length", context), 8);
  }
  assert.match(source, /attachmentInput\.onchange[^\n]*addFiles/);
  assert.match(source, /addEventListener\("drop"[\s\S]*?addFiles\(files\)/);
  assert.match(source, /function handleComposerImagePaste[\s\S]*?addFiles\(files\)/);
});

test("native 桥接对发送与队列来源都接收八张，并拒绝第九张", () => {
  let next = 0;
  const context = vm.createContext({ nativeImageAttachmentId: () => `image-${++next}`,
    failure: (code, message) => Object.assign(new Error(message), { code }) });
  vm.runInContext(native.slice(native.indexOf("  const remoteInputImageMaxCount"), native.indexOf("  function remoteAttachmentContext"))
    + native.slice(native.indexOf("  function validateQueuedImageSources("), native.indexOf("  function hexBytes(")), context);
  context.payload = { images: Array.from({ length: 8 }, (_, i) => ({ name: `${i}.png`, mime: "image/png", size: 3, dataUrl: "data:image/png;base64,AQID" })) };
  assert.equal(vm.runInContext('remoteImageAttachments(payload, "send-validation").length', context), 8);
  assert.equal(vm.runInContext('validateQueuedImageSources(payload.images.map((_, i) => `path-${i}`), "基线").length', context), 8);
  context.payload.images.push({ ...context.payload.images[0], name: "9.png" });
  assert.throws(() => vm.runInContext('remoteImageAttachments(payload, "send-validation")', context), /最多添加 8 张图片/);
  assert.throws(() => vm.runInContext('validateQueuedImageSources(payload.images.map((_, i) => `path-${i}`), "基线")', context), /基线无效/);
});

test("桥接版本升级清理旧实例，守卫与公布版本一致", () => {
  const version = Number(native.match(/__codexElvesRemoteAccess\?\.version === (\d+)/)[1]);
  assert.ok(version > 139);
  assert.match(native, new RegExp(`version: ${version}, call`));
  for (const installed of [135, 139, version]) {
    let disposed = 0;
    const context = vm.createContext({ window: { __codexElvesRemoteAccess: { version: installed, dispose() { ++disposed; } } } });
    vm.runInContext(native.slice(0, native.indexOf("  let manager = null;")) + "globalThis.upgraded = true; })();", context);
    assert.equal(disposed, installed === version ? 0 : 1);
    assert.equal(context.upgraded, installed === version ? undefined : true);
  }
});

test("Claude 完整历史同步移除旧摘要轮次，迟到的历史响应不能复活摘要", async () => {
  const id = "claude-00000000-0000-4000-8000-000000000001", summary = { id: "compact", _nativeUserId: "summary-native", items: [{ id: "summary-item" }] };
  const turns = new Map([[summary.id, summary], ["real", { id: "real", _nativeUserId: "native-real", items: [] }], ["pending", { id: "pending", items: [] }]]);
  const context = vm.createContext({ selectedId: id, turns, thread: { id }, messageVersion: 0, statusVersion: 0, queueVersion: 0,
    notifiedStatus: null, notifiedQueue: null, notifiedQueueEdits: [],
    turnVersions: new Map(), itemVersions: new Map(), partialItems: new Map(),
    normalizeDisplayedTurnItems: value => value, enrichTurnTranscriptEvents: value => value,
    reconcileSentMessageDisplays() {}, rememberThreadDisplay() {}, rememberTranscript() {}, updateComposer() {},
    scheduleMessages() {}, scheduleSidebarRefresh() {}, markVisibleThreadRead: async () => {} });
  let respond;
  Object.assign(context, { document: { hidden: false }, performance, AbortController,
    pageLeaving: false, foregroundReadGeneration: 0, selectionVersion: 1, historyCursor: null,
    threadDeleted: () => false, transcriptChecking: false, historyInitialized: true,
    syncHistoryButtons() {}, recordDiagnostic() {}, mergeTranscriptEvents() {},
    syncEditableMessageMetadata() {}, renderMessages() {}, sideThreadExpired: () => false,
    mergeHistoryTurn: value => turns.get(value.id) || value,
    threadEphemeralState: () => false, isMissingThreadError: () => false, recoveryErrorSilenceDepth: 0,
    api: () => new Promise(resolve => { respond = resolve; }), notice: message => assert.fail(message) });
  vm.runInContext(app.slice(app.indexOf("async function readHistoryPage("), app.indexOf("function pruneCachedHistory(")), context);
  const oldRead = context.readHistoryPage(false, 1);
  vm.runInContext(app.slice(app.indexOf("function receiveClaudeHistory("), app.indexOf("function receive(event)")), context);
  context.params = { threadId: id, thread: { id, queuedMessages: [] }, turnIds: ["real"],
    turns: [{ id: "real", _nativeUserId: "native-real", _nativeDone: true, items: [{ id: "reply", type: "agentMessage", text: "完成" }] }] };
  vm.runInContext("receiveClaudeHistory(params)", context);
  assert.equal(turns.has("compact"), false);
  assert.equal(turns.has("pending"), true);
  assert.equal(turns.get("real")._nativeDone, true);
  assert.equal(context.turnVersions.get("compact"), 1, "删除版本保留，防止较早的历史请求复活摘要");
  respond({ data: [summary, { id: "real", items: [] }], nextCursor: null });
  await oldRead;
  assert.equal(context.turns.has("compact"), false, "已删除摘要不能被通知之前发起的旧读取复活");
  assert.equal(context.recentHistoryPage.turns.some(turn => turn.id === "compact"), false);
});

function claudeUsageHarness() {
  const requests = [], timers = new Map(), nodes = [];
  let tick = Date.parse("2026-11-12T08:00:00Z"), timerId = 0;
  const makeElement = (tag, className = "", textContent = "") => ({
    tag, className, textContent, dataset: {}, style: {}, attributes: {}, children: [], hidden: false, isConnected: true,
    append(...values) { this.children.push(...values); },
    replaceChildren(...values) { this.children = values; },
    setAttribute(name, value) { this.attributes[name] = value; },
    removeAttribute(name) { delete this.attributes[name]; },
    hasAttribute(name) { return Object.hasOwn(this.attributes, name); },
    remove() { this.isConnected = false; },
  });
  const trigger = makeElement("button");
  const context = nodeVm.createContext({
    claudeUsageState: null, connected: true, creating: false, selectedId: "claude-session",
    thread: { id: "claude-session" }, session: { device: { id: "device-a", createdAtMs: 1 },
      capabilities: { claudeSubscriptionUsage: true } },
    instance: "claude:wsl:a", backend: "claude", enabled: true, pageLeaving: false,
    document: { hidden: false, activeElement: null, documentElement: { dataset: { inputModality: "pointer" } },
      body: { append: node => nodes.push(node) } },
    Date: class extends Date { static now() { return tick; } }, AbortController,
    privateInputScope: value => JSON.stringify([value?.device?.id, value?.device?.createdAtMs]),
    activeInstanceId: () => context.instance, isClaude: () => context.backend === "claude",
    sessionInstanceEnabled: () => context.enabled,
    $: selector => selector === "#claude-usage" ? trigger : null, element: makeElement,
    closePopup() {}, closeComposerMenus() {},
    setTimeout(callback, delay) { const id = ++timerId; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
    api(path, payload, id, signal, options) {
      let resolve;
      const promise = new Promise(done => { resolve = done; });
      requests.push({ path, payload, signal, options, resolve });
      return promise;
    },
  });
  nodeVm.runInContext(sourceSection("function claudeUsageContext(", "function tokenCount("), context);
  context.fitClaudeUsageTip = () => {};
  const usage = (percent = 24) => ({ source: "claude-native-usage", observedAtMs: tick,
    fiveHour: { usedPercentage: percent, resetsAt: new Date(tick + 2 * 3600000 + 18 * 60000).toISOString() },
    sevenDay: { usedPercentage: 52, resetsAt: (tick + 3 * 86400000 + 6 * 3600000) / 1000 } });
  return { context, trigger, timers, requests, nodes, usage, advance: milliseconds => { tick += milliseconds; } };
}

test("Claude 额度区分缺失、真实 0%、非法和过期窗口，秒数与 ISO 重置时间一致", () => {
  const { context, usage, advance } = claudeUsageHarness();
  assert.equal(context.claudeUsageWindows(null).length, 0);
  let windows = context.claudeUsageWindows(usage(0));
  assert.equal(windows.length, 2);
  assert.equal(windows[0].used, 0);
  assert.equal(context.claudeUsageResetText(windows[0].resetAt), "2 小时 18 分钟后重置");
  assert.equal(context.claudeUsageResetText(windows[1].resetAt), "3 天 6 小时后重置");
  for (const invalid of [-1, 101, NaN, Infinity, true, "24", null]) {
    const value = usage();
    value.fiveHour.usedPercentage = invalid;
    assert.equal(context.claudeUsageWindows(value).length, 1);
  }
  for (const invalid of ["not-a-date", true, 1]) {
    const value = usage(); value.fiveHour.resetsAt = invalid;
    assert.equal(context.claudeUsageWindows(value).length, 1);
  }
  const value = usage(); value.fiveHour.resetsAt = null;
  assert.equal(context.claudeUsageWindows(value).length, 2);
  assert.equal(context.claudeUsageResetText(null), "重置时间尚未提供");
  const stale = usage();
  advance(180001);
  assert.equal(context.claudeUsageWindows(stale).length, 0, "后台停留过久不能继续显示旧额度");
  const expired = usage();
  expired.fiveHour.resetsAt = "2026-11-12T08:00:00Z";
  assert.equal(context.claudeUsageWindows(expired).length, 1, "窗口到期不能自动推断已用 0%");
});

test("Claude 额度按授权和实例隔离，切换后拒绝迟到响应，停用或后台页面不读取", async () => {
  const { context, trigger, requests, timers, usage } = claudeUsageHarness();
  const flush = () => new Promise(resolve => setImmediate(resolve));
  context.syncClaudeUsage();
  for (let index = 0; index < 5; index++) context.syncClaudeUsage();
  assert.equal(requests.length, 1, "输入框反复更新不重复读取");
  assert.equal(requests[0].path, "/api/read/claudeUsage");
  assert.equal(requests[0].payload.instanceId, "claude:wsl:a");
  assert.equal(requests[0].options.resourcePriority, "background");
  context.instance = "claude:wsl:b";
  context.syncClaudeUsage();
  assert.equal(requests[0].signal.aborted, true);
  requests[1].resolve({ ok: true, backend: "claude", instanceId: "claude:wsl:b", usage: usage(52) });
  await flush();
  requests[0].resolve({ ok: true, backend: "claude", instanceId: "claude:wsl:a", usage: usage(99) });
  await flush();
  assert.equal(context.claudeUsageState.value.fiveHour.usedPercentage, 52);
  assert.equal(trigger.hidden, false);
  assert.equal(timers.size, 1);
  context.session.device.createdAtMs = 2;
  context.syncClaudeUsage();
  assert.equal(trigger.hidden, true, "同设备重新授权也不能沿用旧账号快照");
  assert.equal(requests.length, 3);
  context.document.hidden = true;
  context.syncClaudeUsage();
  assert.equal(requests[2].signal.aborted, true);
  assert.equal(timers.size, 0);
  assert.equal(context.claudeUsageState, null);
  context.document.hidden = false;
  context.backend = "codex";
  context.syncClaudeUsage();
  assert.equal(requests.length, 3);
  context.backend = "claude"; context.enabled = false;
  context.syncClaudeUsage();
  assert.equal(requests.length, 3);
});

test("Claude 额度详情支持鼠标与点击，数据失效时关闭浮层并保留未知状态", async () => {
  const { context, trigger, requests, nodes, usage, advance } = claudeUsageHarness();
  context.mountClaudeUsage();
  context.syncClaudeUsage();
  requests[0].resolve({ ok: true, backend: "claude", instanceId: "claude:wsl:a", usage: usage() });
  await new Promise(resolve => setImmediate(resolve));
  assert.match(trigger.attributes["aria-label"], /5 小时已用 24%.*7 天已用 52%/);
  trigger.onpointerenter({ pointerType: "touch" });
  assert.equal(nodes.length, 0);
  trigger.onpointerenter({ pointerType: "mouse" });
  assert.equal(nodes.length, 1);
  assert.equal(trigger.attributes["aria-expanded"], "true");
  assert.equal(nodes[0].children[0].textContent, "Claude 订阅额度");
  assert.equal(nodes[0].children[3].textContent, "2 小时 18 分钟后重置");
  trigger.onclick();
  assert.equal(context.claudeUsageState.tip.pinned, true);
  trigger.onclick();
  assert.equal(trigger.attributes["aria-expanded"], "false");
  assert.equal(nodes[0].isConnected, false);
  trigger.onclick();
  advance(180001);
  context.renderClaudeUsage();
  assert.equal(trigger.hidden, true);
  assert.equal(context.claudeUsageState.tip, null);
});

test("上下文容量缺失时显示真实最后请求用量，不伪造百分比", () => {
  let info = { modelContextWindow: null, last: { totalTokens: 1300 } };
  const context = vm.createContext({ currentThreadDisplay: () => ({ tokenUsage: info }) });
  vm.runInContext(app.slice(app.indexOf("function tokenCount("), app.indexOf("function closeContextUsageTip(")), context);
  let text = vm.runInContext("contextUsageText()", context);
  assert.match(text.value, /1\.3k Token，容量尚未提供/);
  assert.doesNotMatch(text.value, /%/);
  info = { modelContextWindow: 10000, last: { totalTokens: 1300 } };
  assert.match(vm.runInContext("contextUsageText().value", context), /13%/);
  info = null;
  assert.match(vm.runInContext("contextUsageText().value", context), /上下文信息尚未提供/);
  const cache = threadDisplayCacheHarness();
  cache.context.rememberThreadDisplay({ id: "main", tokenUsage: { modelContextWindow: null, last: { totalTokens: 1300 } } });
  assert.equal(cache.persisted.at(-1).value.tokenUsage.last.totalTokens, 1300);
  assert.equal(cache.context.currentThreadDisplay().tokenUsage.modelContextWindow, null);
});

test("模型容量优先于配置窗口，百分比和重连缓存沿用 Codex 原有规则", () => {
  const { context, persisted } = threadDisplayCacheHarness();
  vm.runInContext(app.slice(app.indexOf("function tokenCount("), app.indexOf("function closeContextUsageTip(")), context);
  const usage = { modelContextWindow: 1000000, usedPercentage: 21.6, model: "claude-opus-5-5",
    modelContextWindowSource: { kind: "native", key: "context_window.context_window_size", model: "claude-opus-5-5" },
    compaction: { windowTokens: 600000, triggerPercent: 90 }, last: { totalTokens: 215496 } };
  context.rememberThreadDisplay({ id: "main", model: "gpt-6.1-sol", tokenUsage: usage });
  const text = context.contextUsageText();
  assert.equal(text.value, "215.5k / 22%，1000k");
  assert.doesNotMatch(text.value, /600k|36%|配置容量/);
  assert.doesNotMatch(text.description, /原生报告|配置压缩|提前压缩/);
  assert.equal(Object.hasOwn(text, "compaction"), false);
  assert.deepEqual(persisted.at(-1).value.tokenUsage.modelContextWindowSource, usage.modelContextWindowSource);
  assert.equal(persisted.at(-1).value.tokenUsage.model, usage.model);
  assert.equal(Object.hasOwn(persisted.at(-1).value.tokenUsage, "usedPercentage"), false);
  assert.deepEqual(persisted.at(-1).value.tokenUsage.compaction, usage.compaction);
  context.rememberThreadDisplay({ id: "main", model: "other-model", tokenUsage: { modelContextWindow: null, model: "other-model", last: { totalTokens: 500 } } });
  assert.match(context.contextUsageText().value, /容量尚未提供/);
  assert.equal(context.currentThreadDisplay().tokenUsage.modelContextWindowSource, undefined, "新模型的明确未知值不能复用旧容量来源");
});

test("配置窗口补齐缺失容量，Codex原生475k保持优先，提前压缩比例不改变分母", () => {
  let info = { modelContextWindow: 600000, modelContextWindowSource: { kind: "configuration" },
    last: { totalTokens: 215496 }, compaction: { windowTokens: 600000, triggerPercent: 90 } };
  const context = vm.createContext({ currentThreadDisplay: () => ({ tokenUsage: info }) });
  vm.runInContext(app.slice(app.indexOf("function tokenCount("), app.indexOf("function closeContextUsageTip(")), context);
  assert.equal(context.contextUsageText().value, "215.5k / 36%，600k");
  assert.equal(Object.hasOwn(context.contextUsageText(), "compaction"), false);
  for (const modelWindow of [null, undefined, 0, -1, NaN, Infinity, "1000000"]) {
    info.modelContextWindow = modelWindow;
    info.usedPercentage = 95;
    for (const triggerPercent of [1, 50, 95, 100]) {
      info.compaction.triggerPercent = triggerPercent;
      assert.equal(context.contextUsageText().value, "215.5k / 36%，600k");
    }
  }
  for (const invalidWindow of [undefined, 0, -1, NaN, Infinity, "600000"]) {
    info.compaction.windowTokens = invalidWindow;
    assert.match(context.contextUsageText().value, /容量尚未提供/);
  }
  info = { modelContextWindow: 475000, last: { totalTokens: 194100 }, compaction: { windowTokens: 900000 } };
  assert.equal(context.contextUsageText().value, "194.1k / 41%，475k");
  for (const reported of [NaN, Infinity, -1, 101, "50", 50, 21.6, 95]) {
    info.usedPercentage = reported;
    assert.equal(context.contextUsageText().value, "194.1k / 41%，475k");
  }
  const cache = threadDisplayCacheHarness();
  vm.runInContext(app.slice(app.indexOf("function tokenCount("), app.indexOf("function closeContextUsageTip(")), cache.context);
  cache.context.rememberThreadDisplay({ id: "main", tokenUsage: {
    modelContextWindow: null, usedPercentage: 95, model: "claude-opus-5-5",
    compaction: { windowTokens: 600000, triggerPercent: 95 }, last: { totalTokens: 215496 },
  } });
  const stored = cache.persisted.at(-1).value.tokenUsage;
  assert.equal(stored.modelContextWindow, 600000);
  assert.equal(Object.hasOwn(stored, "usedPercentage"), false);
  cache.context.rememberThreadDisplay({ id: "main", tokenUsage: stored });
  assert.equal(cache.context.contextUsageText().value, "215.5k / 36%，600k", "重连后不能恢复旧百分比");
});

test("Claude 与 Codex 共用单行容量弹层，不显示配置、比例或来源说明", () => {
  let info = { modelContextWindow: null, last: { totalTokens: 215496 },
    compaction: { windowTokens: 600000, triggerPercent: 95 } };
  const attributes = {}, nodes = [];
  const ring = { textContent: "", style: { setProperty(name, data) { this[name] = data; } } };
  const trigger = { isConnected: true, setAttribute(name, data) { attributes[name] = data; } };
  const element = (tag, className = "", textContent = "") => ({
    tag, className, textContent, children: [], attributes: {},
    get firstElementChild() { return this.children[0]; },
    append(...children) { this.children.push(...children); },
    setAttribute(name, data) { this.attributes[name] = data; },
    hasAttribute(name) { return Object.hasOwn(this.attributes, name); },
  });
  const context = vm.createContext({ currentThreadDisplay: () => ({ tokenUsage: info }),
    $: selector => selector === "#context-info" ? trigger : ring,
    contextUsageTip: null, fitContextUsageTip() {}, element,
    closePopup() {}, closeComposerMenus() {}, closeContextUsageTip() {}, clearTimeout() {},
    scheduleContextUsageTipClose() {}, document: { body: { append: node => nodes.push(node) } } });
  vm.runInContext(sourceSection("function tokenCount(", "function closeContextUsageTip(")
    + sourceSection("function updateContextUsage()", "function scheduleContextUsageTipClose("), context);
  context.showContextUsageTip(true);
  const node = nodes[0];
  assert.equal(node.children.length, 1, "实际弹层只创建一行");
  assert.equal(node.firstElementChild.className, "context-usage-value", "用量使用正文色，不能再次套用 muted");
  assert.equal(node.firstElementChild.textContent, "215.5k / 36%，600k");
  assert.equal(ring.textContent, "36%");
  assert.doesNotMatch(attributes["aria-label"], /配置压缩|提前压缩|原生报告|实际触发/);
  info = { modelContextWindow: 475000, last: { totalTokens: 395300 } };
  context.updateContextUsage();
  assert.equal(node.firstElementChild.textContent, "395.3k / 83%，475k");
  assert.equal(ring.textContent, "83%");
  info = { modelContextWindow: 1000000, usedPercentage: 21.6, last: { totalTokens: 215496 } };
  context.updateContextUsage();
  assert.equal(ring.textContent, "22%");
  assert.equal(node.children.length, 1);
  assert.equal(node.firstElementChild.textContent, "215.5k / 22%，1000k");
  info = { modelContextWindow: null, last: { totalTokens: 215496 } };
  context.updateContextUsage();
  assert.equal(ring.textContent, "—");
  assert.match(node.firstElementChild.textContent, /容量尚未提供/);
  assert.equal(node.children.length, 1);
});

function richFileHarness() {
  const node = (tag, className = "", textContent = "") => ({
    tag, className, text: textContent, children: [], attributes: {},
    get textContent() { return this.text + this.children.map(child => child.textContent).join(""); },
    set textContent(value) { this.text = value; this.children = []; },
    style: { setProperty(name, value) { this[name] = value; } }, disabled: false,
    append(...children) { this.children.push(...children); },
    replaceChildren(...children) { this.text = ""; this.children = children; },
    setAttribute(name, value) { this.attributes[name] = value; },
  });
  const context = vm.createContext({
    node, displayWorkspaceName: value => value,
    control: (label, click) => Object.assign(node("button", "", label), { click }),
  });
  const from = sessionFiles.indexOf("export function validRichFilePreview(");
  const to = sessionFiles.indexOf("const bytesLabel =", from);
  vm.runInContext(sessionFiles.slice(from, to).replace(/^export /gm, ""), context);
  const all = root => [root, ...root.children.flatMap(all)];
  return { context, all };
}

test("表格文档预览验证结构与总量，拒绝错误行号、越界列和未知内容块", () => {
  const h = richFileHarness();
  const value = { kind: "spreadsheet", sheets: [{
    name: "中文表", columnCount: 2, truncated: false,
    rows: [{ number: 1, cells: ["号码", "说明"] }, { number: 1048576, cells: ["00123", "<script>"] }],
  }] };
  assert.equal(h.context.validRichFilePreview(value), true);
  for (const change of [
    value => value.sheets[0].rows[1].number = 1,
    value => value.sheets[0].rows[1].number = -1,
    value => value.sheets[0].columnCount = 101,
    value => value.sheets[0].rows[1].cells = [1],
    value => value.sheets[0].rows[1].cells = ["x".repeat(16385)],
    value => value.sheets = Array.from({ length: 21 }, () => value.sheets[0]),
  ]) {
    const invalid = structuredClone(value); change(invalid);
    assert.equal(h.context.validRichFilePreview(invalid), false);
  }
  const document = { kind: "document", sections: [{ title: "第 1 页", blocks: [
    { kind: "heading", text: "标题" }, { kind: "paragraph", text: "正文" },
    { kind: "table", rows: [["单元格"]] },
  ] }] };
  assert.equal(h.context.validRichFilePreview(document), true);
  document.sections[0].blocks.push({ kind: "html", text: "<script>" });
  assert.equal(h.context.validRichFilePreview(document), false);
});

test("CSV、TSV、ODS 的空布局数组可预览，缺省列宽与完整列宽兼容，残缺列宽仍拒绝", () => {
  const h = richFileHarness();
  // 对齐 SheetBuilder::finish 实际序列化：无原始列宽时返回 []，不能只测省略字段的手写响应。
  const value = { kind: "spreadsheet", sheets: [{
    name: "工作表", columnCount: 3, truncated: false, merges: [], columnWidths: [], styles: [],
    rows: [{ number: 1, cells: ["名称", "号码", "说明"] },
      { number: 2, cells: ["示例", "001234", "换行\n逗号,引号"] }, { number: 3, cells: ["短行"] }],
  }] };
  assert.equal(h.context.validRichFilePreview(value), true);
  const root = h.context.renderRichFilePreview(value, { layout: "original" }), nodes = h.all(root);
  assert.equal(nodes.find(node => node.tag === "table").style.width, "calc(360px + var(--sheet-row-number-width,44px))");
  assert.deepEqual(nodes.filter(node => node.tag === "col").map(node => node.style.width),
    ["var(--sheet-row-number-width,44px)", "120px", "120px", "120px"]);
  assert.equal(nodes.some(node => node.tag === "td" && node.textContent === "001234"), true);
  assert.equal(nodes.some(node => node.tag === "td" && node.textContent === "换行\n逗号,引号"), true);
  for (const columnWidths of [undefined, null, [], [1, 32, 480], [54, 120, 180]]) {
    const compatible = structuredClone(value);
    compatible.sheets[0].columnWidths = columnWidths;
    assert.equal(h.context.validRichFilePreview(compatible), true);
  }
  for (const columnWidths of [[120], [120, 120, 120, 120], [120, 0, 120],
    [120, 481, 120], [120, 120.5, 120], ["120", 120, 120], ""]) {
    const invalid = structuredClone(value);
    invalid.sheets[0].columnWidths = columnWidths;
    assert.equal(h.context.validRichFilePreview(invalid), false);
  }
});

test("Excel 多表切换和每页100行保留页码，空表与稀疏行号可以阅读", () => {
  const h = richFileHarness(), state = {};
  const value = { kind: "spreadsheet", sheets: [
    { name: "数据", columnCount: 2, truncated: false,
      rows: Array.from({ length: 215 }, (_, index) => ({ number: index + 1000, cells: [`row-${index}`, "00123"] })) },
    { name: "空表", columnCount: 0, truncated: false, rows: [] },
  ] };
  let changes = 0;
  const root = h.context.renderRichFilePreview(value, state, () => changes++);
  const find = predicate => h.all(root).filter(predicate);
  assert.equal(find(node => node.tag === "td").length, 200);
  assert.equal(find(node => node.tag === "th" && node.scope === "row")[0].textContent, "1000");
  find(node => node.tag === "button" && node.attributes["aria-label"] === "下一页")[0].click();
  assert.equal(state.page, 1);
  assert.equal(find(node => node.tag === "td")[0].textContent, "row-100");
  const restored = h.context.renderRichFilePreview(value, { ...state });
  assert.equal(h.all(restored).find(node => node.tag === "td").textContent, "row-100");
  find(node => node.attributes.role === "tab" && node.textContent === "空表")[0].click();
  assert.equal(state.sheet, 1);
  assert.equal(state.page, 0);
  assert.equal(find(node => node.tag === "p" && node.textContent === "此表格为空").length, 1);
  assert.equal(changes, 2);
  assert.equal(h.context.spreadsheetColumn(26), "AA");
});

test("文档正文和表格中的HTML只作为文字，不创建脚本、图片或外链", () => {
  const h = richFileHarness();
  const source = '<img src="https://example.invalid" onerror="alert(1)"><script>bad()</script>';
  const value = { kind: "document", sections: [{ title: "文档", blocks: [
    { kind: "paragraph", text: source }, { kind: "table", rows: [[source, "00123"]] },
  ] }] };
  const root = h.context.renderRichFilePreview(value), nodes = h.all(root);
  assert.equal(nodes.filter(node => ["img", "script", "iframe", "a"].includes(node.tag)).length, 0);
  assert.equal(nodes.filter(node => node.textContent === source).length, 2);
  assert.equal(nodes.some(node => node.tag === "td" && node.textContent === "00123"), true);
});

test("Excel 合并范围保留横纵跨度、固定列宽和跨页左上角内容，不复制被覆盖的空格", () => {
  const h = richFileHarness(), rows = Array.from({ length: 105 }, (_, index) => ({
    number: index + 1, cells: index === 0 ? ["横向标题", "", "第三列"]
      : index === 98 ? ["纵向合并", "", "值"] : ["", "", `row-${index + 1}`],
  }));
  const sheet = { name: "合并表", columnCount: 3, truncated: false,
    columnWidths: [54, 96, 130], rows, merges: [
      { startRow: 1, endRow: 1, startColumn: 0, endColumn: 1 },
      { startRow: 99, endRow: 103, startColumn: 0, endColumn: 1 },
    ] };
  const value = { kind: "spreadsheet", sheets: [sheet] };
  assert.equal(h.context.validRichFilePreview(value), true);
  const root = h.context.renderRichFilePreview(value, { layout: "original" });
  let cells = h.all(root).filter(node => node.tag === "td");
  const heading = cells.find(node => node.textContent === "横向标题");
  assert.equal(heading.colSpan, 2); assert.equal(heading.rowSpan, 1);
  assert.equal(cells.filter(node => node.attributes["data-row"] === "1").length, 2);
  const merged = cells.find(node => node.textContent === "纵向合并");
  assert.equal(merged.rowSpan, 2); assert.equal(merged.colSpan, 2);
  assert.equal(h.all(root).find(node => node.tag === "table").style.width, "calc(280px + var(--sheet-row-number-width,44px))");
  assert.deepEqual(h.all(root).filter(node => node.tag === "col").map(node => node.style.width),
    ["var(--sheet-row-number-width,44px)", "54px", "96px", "130px"]);
  h.all(root).find(node => node.tag === "button" && node.attributes["aria-label"] === "下一页").click();
  cells = h.all(root).filter(node => node.tag === "td");
  const continued = cells.find(node => node.textContent === "纵向合并");
  assert.equal(continued.rowSpan, 3);
  assert.equal(continued.attributes["data-row"], "101");
  assert.equal(cells.filter(node => node.attributes["data-row"] === "102").length, 1);
  assert.equal(sheet.rows[100].cells[0], "", "分页渲染不能改写原始数据");
  for (const change of [
    sheet => sheet.merges.push({ ...sheet.merges[0] }),
    sheet => sheet.merges[0].endColumn = 3,
    sheet => sheet.merges[0].endRow = 10000,
    sheet => sheet.columnWidths[0] = -1,
    sheet => sheet.rows = sheet.rows.filter(row => row.number !== 100),
  ]) {
    const invalid = structuredClone(value); change(invalid.sheets[0]);
    assert.equal(h.context.validRichFilePreview(invalid), false);
  }
});

test("Excel 原始背景字号和文字效果按样式索引应用，行数交给文件大小信息行", () => {
  const h = richFileHarness(), style = {
    background: "#17365D", color: "#FFFFFF", fontSize: 16,
    bold: true, italic: true, underline: true, strike: false,
    horizontal: "center", vertical: "middle", wrap: true,
  };
  const value = { kind: "spreadsheet", sheets: [{ name: "样式", columnCount: 2, truncated: false,
    rows: [{ number: 1, cells: ["标题", ""], styleIds: [0, 0] }],
    styles: [style], merges: [{ startRow: 1, endRow: 1, startColumn: 0, endColumn: 1 }],
  }] };
  assert.equal(h.context.validRichFilePreview(value), true);
  let range;
  const root = h.context.renderRichFilePreview(value, {}, null, { onRange: value => { range = value; } });
  const cell = h.all(root).find(node => node.tag === "td");
  assert.equal(cell.style.backgroundColor, "#17365D");
  assert.equal(cell.style.color, "#FFFFFF");
  assert.match(cell.style.fontSize, /file-reading-font-size/);
  assert.equal(cell.style.fontWeight, "700");
  assert.equal(cell.style.fontStyle, "italic");
  assert.equal(cell.style.textAlign, "center");
  assert.equal(cell.style.verticalAlign, "middle");
  assert.equal(h.all(range).some(node => node.textContent === "1–1 / 1 行"), true);
  assert.equal(h.all(root).some(node => node.textContent === "1–1 / 1 行"), false);
  for (const change of [
    value => value.sheets[0].styles[0].background = "url(https://example.invalid)",
    value => value.sheets[0].styles[0].fontSize = 1000,
    value => value.sheets[0].rows[0].styleIds = [10, 0],
  ]) {
    const invalid = structuredClone(value); change(invalid);
    assert.equal(h.context.validRichFilePreview(invalid), false);
  }
});

test("原始布局使用文件行高，自动换行允许增高，紧凑布局解除文件固定高度", () => {
  const h = richFileHarness(), value = { kind: "spreadsheet", sheets: [{
    name: "尺寸", columnCount: 2, columnWidths: [480, 200], defaultRowHeight: 24, truncated: false,
    rows: [{ number: 1, height: 80, cells: ["短", "一行\n第二行"] }, { number: 2, cells: ["末行", "值"] }],
  }] };
  assert.equal(h.context.validRichFilePreview(value), true);
  const fixed = h.context.renderRichFilePreview(value, { layout: "original", wrap: false });
  const fixedCells = h.all(fixed).filter(node => node.className === "session-files-cell-content");
  assert.equal(fixedCells[0].style.height, "79px");
  assert.equal(fixedCells[2].style.height, "23px");
  assert.equal(h.all(fixed).find(node => node.tag === "td").style.whiteSpace, "pre");
  const wrapped = h.context.renderRichFilePreview(value, { layout: "original", wrap: true });
  assert.equal(h.all(wrapped).find(node => node.className === "session-files-cell-content").style.minHeight, "79px");
  assert.equal(h.all(wrapped).find(node => node.tag === "td").style.whiteSpace, "pre-wrap");
  const compact = h.context.renderRichFilePreview(value, { layout: "compact", wrap: true });
  assert.equal(h.all(compact).find(node => node.className === "session-files-cell-content").style.height, undefined);
  assert.equal(h.all(compact).find(node => node.className === "session-files-cell-content").style.minHeight, undefined);
  assert.equal(h.all(compact).find(node => node.tag === "table").attributes["data-layout"], "compact");
  assert.equal(h.all(compact).filter(node => node.tag === "col")[1].style.width.startsWith("clamp(48px,"), true);
  for (const change of [
    value => value.sheets[0].rows[0].height = 0,
    value => value.sheets[0].rows[0].height = Infinity,
    value => value.sheets[0].rows[0].height = "80",
    value => value.sheets[0].defaultRowHeight = 2049,
    value => value.sheets[0].defaultRowHeight = -1,
  ]) {
    const invalid = structuredClone(value); change(invalid);
    assert.equal(h.context.validRichFilePreview(invalid), false);
  }
});

test("紧凑列宽测量整张表且有上限，合并标题分摊宽度，翻页不跳变", () => {
  const h = richFileHarness(), measure = text => text.length * 10;
  const rows = [{ number: 1, cells: ["合并标题".repeat(10), "", "短"] },
    { number: 2, cells: ["A", "B", "多".repeat(100)] }];
  const widths = h.context.compactSpreadsheetWidths(rows, 3, [],
    [{ startRow: 1, endRow: 1, startColumn: 0, endColumn: 1 }], measure);
  assert.equal(widths[0], widths[1], "合并标题不能只撑宽 A 列");
  assert.match(widths[0], /^clamp\(48px,calc\(/);
  assert.match(widths[2], /,480px\)$/);
  const value = { kind: "spreadsheet", sheets: [{
    name: "跨页", columnCount: 1, columnWidths: [480], truncated: false,
    rows: Array.from({ length: 105 }, (_, index) => ({ number: index + 1, cells: [index === 104 ? "很长的末页内容".repeat(30) : "短"] })),
  }] }, state = { layout: "compact", wrap: false };
  const root = h.context.renderRichFilePreview(value, state);
  const firstWidth = h.all(root).find(node => node.tag === "table").style.width;
  h.all(root).find(node => node.attributes["aria-label"] === "下一页").click();
  assert.equal(h.all(root).find(node => node.tag === "table").style.width, firstWidth);
  assert.equal(state.layout, "compact");
  assert.equal(state.wrap, false);
  assert.equal(state.page, 1);
});

test("服务子智能体按独立能力启用，旧后台保持禁用且子会话始终只读", () => {
  const context = { session: { readOnly: false, runtime: { cdpInjectionEnabled: false }, capabilities: {} },
    selectedId: "child", thread: { id: "child", viewKind: "subagent", readOnly: true },
    relatedThreadViews: new Map(), relatedSessionView: null, normalizeRelatedView: value => value };
  vm.createContext(context);
  vm.runInContext(app.slice(app.indexOf("function subagentsEnabled("), app.indexOf("function threadReadOnlyMessage(")), context);
  assert.equal(context.subagentsEnabled(), false);
  context.session.capabilities.serviceSubagents = true;
  assert.equal(context.subagentsEnabled(), true);
  assert.equal(context.sideThreadsEnabled(), false);
  context.session.capabilities.serviceSideThreads = true;
  assert.equal(context.sideThreadsEnabled(), true);
  assert.equal(context.threadReadOnly(), true);
  assert.equal(context.threadReadOnly("main"), false);
  context.session.runtime.cdpInjectionEnabled = true;
  delete context.session.capabilities.serviceSubagents;
  assert.equal(context.subagentsEnabled(), true);
});

test("会话菜单分离子智能体读取和侧边写入，并按运行方式退役旧请求", () => {
  let current = { deviceId: "device", cacheScope: "scope", threadId: "parent", connected: true,
    subagentsEnabled: true, sideThreadsEnabled: false, serviceMode: true, relatedScope: "service:1" };
  const context = vm.createContext({ getContext: () => current, nonempty: value => typeof value === "string" && !!value });
  vm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function readContext("),
    sessionMenuSource.indexOf("  function syncContext(")) + sessionMenuSource.slice(
      sessionMenuSource.indexOf("  const canRead ="), sessionMenuSource.indexOf("  function entryFor("))
      + "\nglobalThis.permissions = value => [canRead(value), canMutate(value)];", context);
  const service = context.readContext();
  assert.deepEqual(Array.from(context.permissions(service)), [true, false]);
  current = { ...current, sideThreadsEnabled: true, relatedScope: "cdp:2", serviceMode: false };
  const cdp = context.readContext();
  assert.deepEqual(Array.from(context.permissions(cdp)), [true, true]);
  assert.notEqual(cdp.owner, service.owner);
  current = { ...current, subagentsEnabled: false, sideThreadsEnabled: false };
  assert.deepEqual(Array.from(context.permissions(context.readContext())), [false, false]);
});

test("服务子智能体刷新失败保留旧列表并明确标记未同步", async () => {
  const previous = { loadedAtMs: 1, data: { threadId: "parent", subagentsComplete: true,
    subagents: [{ id: "child", status: "active" }], completedSubagents: [], completedSubagentCount: 0 } };
  const entry = { snapshot: previous, dirty: true, revision: 1, request: null, again: false };
  const value = { threadId: "parent", owner: "owner", connected: true, serviceMode: true };
  let applied;
  const context = vm.createContext({
    disposed: false, opened: true, cached: previous, context: value, message: "", refreshAgeMs: 30000,
    syncContext: () => value, entryFor: () => entry, restore() {}, updatePermissions() {}, render() {},
    accepts: () => true, owns: () => true,
    load: async () => { throw new Error("引擎暂时不可用"); },
    applySnapshot: (_entry, snapshot) => { applied = snapshot; },
    report: () => assert.fail("后台刷新不弹出重复通知"),
  });
  vm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function refresh("),
    sessionMenuSource.indexOf("  function open(")), context);
  await context.refresh({ background: true });
  assert.equal(applied.data.subagents[0].id, "child");
  assert.equal(applied.data.subagentsComplete, false);
  assert.equal(applied.data.subagentsStale, true);
  assert.equal(applied.data.subagentsError, "引擎暂时不可用");
  assert.equal(entry.dirty, true);
  assert.equal(entry.request, null);
});

test("服务子智能体补读只在当前可见只读页运行，返回主会话后旧定时器不再读取", async () => {
  const timers = [], reads = [];
  const context = vm.createContext({
    relatedSessionCheckTimer: null, relatedSessionView: { kind: "subagent", threadId: "child" },
    selectedId: "child", selectionVersion: 1, session: { runtime: { cdpInjectionEnabled: false } },
    document: { hidden: false }, connected: true, privateInputScope: () => "scope",
    subagentsEnabled: () => true, normalizeRelatedView: value => value,
    clearTimeout() {}, setTimeout: (callback, delay) => { timers.push({ callback, delay }); return timers.length; },
    reloadThread: async () => { reads.push(context.selectedId); },
  });
  vm.runInContext(app.slice(app.indexOf("function scheduleRelatedSessionCheck("), app.indexOf("function isMissingThreadError(")), context);
  context.scheduleRelatedSessionCheck();
  assert.equal(timers[0].delay, 2000);
  await timers[0].callback();
  assert.deepEqual(reads, ["child"]);
  context.selectedId = "main"; context.selectionVersion++;
  await timers[1].callback();
  assert.deepEqual(reads, ["child"]);
  context.selectedId = "child"; context.document.hidden = true;
  context.scheduleRelatedSessionCheck();
  assert.equal(timers.length, 2);
});

test("服务菜单实际展示子智能体并继续禁用侧边会话，旧侧边缓存不渲染", () => {
  class MenuNode {
    constructor(text = "") {
      this.textContent = text; this.children = []; this.dataset = {}; this.attributes = new Map();
      const classes = new Set();
      this.classList = { add: (...values) => values.forEach(value => classes.add(value)),
        toggle: (value, enabled) => enabled ? classes.add(value) : classes.delete(value) };
    }
    append(...nodes) { this.children.push(...nodes); }
    replaceChildren(...nodes) { this.children = [...nodes]; }
    contains() { return false; }
    setAttribute(name, value) { this.attributes.set(name, value); }
    addEventListener() {}
    querySelector() { return this.header; }
    get childElementCount() { return this.children.length; }
  }
  const groups = new Map(), buttons = [], mutationButtons = new Map(), menu = new MenuNode();
  const element = (_tag, _className, text = "") => new MenuNode(text);
  const itemButton = (label, callback, _args, kind) => {
    const button = new MenuNode(label); button.disabled = typeof callback !== "function";
    if (kind) mutationButtons.set(button, kind);
    buttons.push(button); return button;
  };
  const section = (label, action) => {
    const node = new MenuNode(label); node.header = new MenuNode(label);
    node.append(node.header); if (action) node.header.append(action);
    groups.set(label, node); menu.append(node); return node;
  };
  const context = vm.createContext({
    menu, groups, mutationButtons, element, itemButton, section, times: [], document: { activeElement: null },
    layer: { classList: { toggle() {} } }, pinned: () => false, desktop: () => false,
    context: { subagentsEnabled: true, sideThreadsEnabled: false, connected: true, threadId: "parent" },
    cached: { loadedAtMs: 1, data: { subagents: [{ id: "child", name: "工作中的子智能体", status: "active" }],
      completedSubagentCount: 1, completedSubagents: [{ id: "done", name: "已结束子智能体", outcome: "failed" }],
      sideThreads: [{ id: "old-side", title: "旧侧边缓存" }], canCreateSideThread: false, subagentsComplete: true } },
    entries: new Map([["owner", { subagentsExpanded: true, completedSubagentsExpanded: true }]]),
    owner: "owner", id: "menu", opened: false, message: "",
    closeAction: new MenuNode(), createSideThread() {}, closeSideThread() {}, openSideThread() {}, openSubagent() {},
    openChanges() {}, openFiles() {}, openBrowser() {}, renderSubagentIcon: () => null,
    nonempty: value => typeof value === "string" && !!value,
    cancelPress() {}, hideContext() {}, fit() {}, updateTimes() {}, canRead: () => true,
    canMutate: value => value.sideThreadsEnabled && value.connected && !value.readOnly,
  });
  vm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function updatePermissions("),
    sessionMenuSource.indexOf("  function updateTimes("))
    + sessionMenuSource.slice(sessionMenuSource.indexOf("  function render()"), sessionMenuSource.indexOf("  function viewport(")), context);
  context.render();
  assert.ok(buttons.some(button => button.textContent === "工作中的子智能体" && !button.disabled));
  assert.ok(buttons.some(button => button.textContent === "已结束子智能体" && !button.disabled));
  assert.equal(buttons.some(button => button.textContent === "旧侧边缓存"), false);
  assert.equal(groups.get("侧边会话").attributes.get("aria-disabled"), "true");
  assert.equal(groups.get("子智能体").attributes.has("aria-disabled"), false);
  assert.ok([...mutationButtons.keys()].every(button => button.disabled));
  context.context.sideThreadsEnabled = true;
  context.cached.data.canCreateSideThread = true;
  context.cached.data.sideThreads = [{ id: "service-side", title: "服务侧边会话" }];
  buttons.length = 0;
  context.render();
  assert.ok(buttons.some(button => button.textContent === "服务侧边会话" && !button.disabled));
  assert.ok([...mutationButtons.keys()].every(button => !button.disabled));
});

test("手机侧栏遮罩100ms后接受独立点击，早期触摸和跨区域手势仍不会误关", () => {
  const classes = new Set(), nodes = new Map();
  for (const selector of [".workspace", "#session-sidebar", ".sidebar-backdrop", ".conversation", "#open-sidebar"]) {
    nodes.set(selector, {
      inert: false, setAttribute() {}, focus() {}, querySelector() { return this; },
      classList: {
        contains: name => classes.has(name),
        remove: name => classes.delete(name),
        toggle: (name, enabled) => enabled ? classes.add(name) : classes.delete(name)
      }
    });
  }
  let now = 1000;
  const context = vm.createContext({
    $: selector => nodes.get(selector), performance: { now: () => now },
    matchMedia: () => ({ matches: true, addEventListener() {}, removeEventListener() {} }),
    desktopSidebarOpen: true, relatedSessionView: null,
    closePopup() {}, closeComposerMenus() {}, renderSidebar() {}
  });
  const start = app.indexOf('  const workspace = $(".workspace"), sidebarPanel = $("#session-sidebar");');
  const end = app.indexOf('  const searchInput = $("#sidebar-search")', start);
  assert.ok(start >= 0 && end > start);
  vm.runInContext(app.slice(start, end), context);
  const backdrop = nodes.get(".sidebar-backdrop");
  const event = (timeStamp, target = backdrop) => ({
    timeStamp, target, pointerId: 1, button: 0, isPrimary: true,
    preventDefault() {}, stopPropagation() {}
  });
  const open = () => nodes.get("#open-sidebar").onclick();
  const click = (down, up = down) => {
    backdrop.onpointerdown(down); backdrop.onpointerup(up); backdrop.onclick(up);
  };
  open();
  click(event(1099), event(1101));
  assert.equal(classes.has("sidebar-open"), true, "保护期内开始的触摸不能因晚松手而关闭");
  click(event(1100));
  assert.equal(classes.has("sidebar-open"), false, "100ms时独立点击即可关闭，不需等待300ms");
  now = 2000; open();
  backdrop.onclick(event(2200));
  assert.equal(classes.has("sidebar-open"), true, "没有独立按下和松开记录时不能关闭");
  click(event(2200), event(2201, nodes.get("#session-sidebar")));
  assert.equal(classes.has("sidebar-open"), true);
  backdrop.onpointerdown(event(2200)); backdrop.onpointercancel();
  backdrop.onpointerup(event(2201)); backdrop.onclick(event(2201));
  assert.equal(classes.has("sidebar-open"), true);
  click(event(2202));
  assert.equal(classes.has("sidebar-open"), false);
});

test("文件阅读字号默认12，旧配置和新设置都限制在6至14且独立于全局字号", () => {
  const disk = storage();
  const context = vm.createContext({ localStorage: disk, matchMedia: () => ({ matches: false }) });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  for (const [saved, expected] of [[undefined, 12], [null, 12], [6, 6], [12, 12], [14, 14], [16, 14], [5, 6], ["8", 12]]) {
    disk.setItem("codex-elves-web-appearance", JSON.stringify({ fontSize: 20, fileReadingFontSize: saved }));
    context.appearance = context.readAppearance();
    assert.equal(context.currentFileReadingFontSize(), expected);
    assert.equal(context.appearance.fontSize, 20);
  }
  assert.equal(context.defaultAppearance().fileReadingFontSize, 12);
});

test("新会话设置默认仅 Codex、正文950和系统14px字体，兼容旧颜色及隔离两套主题", () => {
  const disk = storage();
  const context = vm.createContext({ localStorage: disk, matchMedia: () => ({ matches: false }) });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  const defaults = context.readAppearance();
  assert.deepEqual(JSON.parse(JSON.stringify(defaults.sessionTypes)), { codex: true, claude: false });
  assert.equal(defaults.conversationWidth, 950);
  assert.equal(defaults.fontSize, 14); assert.equal(defaults.codeFontSize, 14);
  assert.equal(defaults.uiFont, ""); assert.equal(defaults.codeFont, "");
  disk.setItem("codex-elves-web-appearance", JSON.stringify({ fontSize: 17, conversationWidth: 100000,
    uiFont: "Consolas;url(invalid)", colors: { dark: { background: "#232323" } } }));
  const migrated = context.readAppearance();
  assert.equal(migrated.fontSize, 17); assert.equal(migrated.conversationWidth, 3840);
  assert.equal(migrated.uiFont, "");
  assert.equal(migrated.colors.dark.background, "#232323");
  assert.equal(migrated.palettes["vscode-plus"].dark.background, "#1f1f1f");
  migrated.palettes["vscode-plus"].dark.background = "#123456";
  assert.equal(migrated.palettes.chatgpt.dark.background, "#232323");
});

test("连续取色的 HSV 与 HEX 往返保留原色，灰阶和色相边界保持有效", () => {
  const context = vm.createContext();
  vm.runInContext(sourceSection("function appearanceHexToHsv(", "function openAppearanceColorPicker("), context);
  for (const color of ["#000000", "#ffffff", "#808080", "#ff0000", "#00ff00", "#0000ff", "#007acc", "#a561df", "#fe0010"]) {
    const hsv = context.appearanceHexToHsv(color);
    assert.ok(hsv.h >= 0 && hsv.h < 360 && hsv.s >= 0 && hsv.s <= 1 && hsv.v >= 0 && hsv.v <= 1);
    assert.equal(context.appearanceHsvToHex(hsv), color);
  }
  assert.equal(context.appearanceHexToHsv("#bad"), null);
  assert.equal(context.appearanceHexToHsv("not-a-color"), null);
  assert.equal(context.appearanceHsvToHex({ h: 360, s: 1, v: 1 }), "#ff0000");
});

test("字体 Select 提供系统选项及设备可用字体，并保留已保存的自定义字体", () => {
  const metrics = { font: "", measureText() {
    return { width: /Segoe UI|Consolas/.test(this.font) ? 120 : 100 };
  } };
  const context = vm.createContext({
    localStorage: storage(), matchMedia: () => ({ matches: false }),
    document: { createElement: () => ({ getContext: () => metrics }) },
  });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  vm.runInContext(sourceSection("const appearanceFontFamilies =", "function appearanceHexToHsv("), context);
  assert.deepEqual(Array.from(context.appearanceFontOptions("Custom Font"), option => option.value),
    ["", "Consolas", "Custom Font", "Segoe UI"]);
  assert.equal(context.appearanceFontOptions("")[0].label, "系统");
  assert.equal(context.appearanceFontOptions("bad;font").some(option => option.value === "bad;font"), false);
});

test("浅色与深色预览选择沿用 manual 存储，自动选择仍由自动主题决定", async () => {
  const h = await themeDiagnosticsHarness();
  h.context.setAppearanceThemeMode("manual", { theme: "dark" });
  assert.equal(h.appearance.themeMode, "manual");
  assert.equal(h.appearance.theme, "dark");
  h.context.setAppearanceThemeMode("manual", { theme: "light" });
  assert.equal(h.appearance.theme, "light");
  h.hour(21);
  h.context.setAppearanceThemeMode("auto");
  assert.equal(h.appearance.themeMode, "auto");
  assert.equal(h.appearance.theme, "dark");
});

test("会话类型筛选与启用开关取交集，原生 Claude ID 也按同一规则过滤", () => {
  const context = vm.createContext();
  vm.runInContext(app.slice(app.indexOf("function conversationBackend("), app.indexOf("const projectThreadLimits =")), context);
  const codex = { id: "codex-thread" }, claude = { id: "claude-12345678-1234-1234-1234-123456789abc" };
  for (const [enabled, filter, expected] of [
    [{ codex: true, claude: false }, "all", [true, false]],
    [{ codex: true, claude: true }, "all", [true, true]],
    [{ codex: true, claude: true }, "codex", [true, false]],
    [{ codex: true, claude: true }, "claude", [false, true]],
    [{ codex: true, claude: false }, "claude", [false, false]],
  ]) {
    const preferences = { sessionTypes: enabled, sessionFilter: filter };
    assert.deepEqual([context.sessionTypeVisible(codex, preferences), context.sessionTypeVisible(claude, preferences)], expected);
  }
});

test("会话实例筛选分别显示同类型 WSL 选项，精确匹配实例并遵循各自启用开关", () => {
  const instances = [
    { id: "codex:local", backend: "codex", label: "Codex", legacyDefault: true },
    { id: "claude:wsl:a", backend: "claude", label: "Claude · Ubuntu", distribution: "Ubuntu", legacyDefault: true },
    { id: "claude:wsl:b", backend: "claude", label: "Claude · Debian", distribution: "Debian" },
  ];
  const appearance = { sessionFilter: "claude:wsl:b",
    sessionInstances: { "codex:local": true, "claude:wsl:a": true, "claude:wsl:b": true } };
  const context = vm.createContext({ sessionInstances: instances, appearance });
  const rows = [{ id: "codex", instanceId: "codex:local" },
    { id: "a", backend: "claude", instanceId: "claude:wsl:a" },
    { id: "b", backend: "claude", instanceId: "claude:wsl:b" },
    { id: "legacy-b", backend: "claude", wslDistribution: "Debian" }];
  assert.deepEqual(Array.from(context.sessionFilterOptions(), option => [option.value, option.label]), [
    ["all", "全部会话"], ["codex:local", "Codex"],
    ["claude:wsl:a", "Claude · Ubuntu"], ["claude:wsl:b", "Claude · Debian"],
  ]);
  assert.deepEqual(rows.map(row => context.sessionTypeVisible(row)), [false, false, true, true]);
  appearance.sessionInstances["claude:wsl:b"] = false;
  assert.deepEqual(rows.map(row => context.sessionTypeVisible(row)), [false, false, false, false]);
  assert.equal(context.sessionFilterOptions().some(option => option.value === "claude:wsl:b"), false,
    "未开启的实例直接隐藏，不能只置灰");
  appearance.sessionFilter = ["codex:local", "claude:wsl:a"];
  assert.deepEqual(rows.map(row => context.sessionTypeVisible(row)), [true, true, false, false]);
  appearance.sessionFilter = "all";
  assert.deepEqual(rows.map(row => context.sessionTypeVisible(row)), [true, true, false, false]);
  assert.equal(rows.length, 4, "筛选不能改写原列表快照");
});

test("实例筛选可持久保存，旧类型选择迁到启用的默认实例且不扩大到其他类型", () => {
  const disk = storage(), instances = [
    { id: "codex:local", backend: "codex", label: "Codex", legacyDefault: true },
    { id: "claude:wsl:a", backend: "claude", label: "Claude · Ubuntu", legacyDefault: true },
    { id: "claude:wsl:b", backend: "claude", label: "Claude · Debian" },
  ];
  const context = vm.createContext({ localStorage: disk, sessionInstances: instances,
    matchMedia: () => ({ matches: false }) });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  disk.setItem("codex-elves-web-appearance", JSON.stringify({ sessionFilter: "claude:wsl:b",
    sessionInstances: { "codex:local": true, "claude:wsl:a": true, "claude:wsl:b": true } }));
  const preferences = context.readAppearance();
  assert.equal(preferences.sessionFilter, "claude:wsl:b");
  preferences.sessionFilter = "claude";
  assert.deepEqual(Array.from(context.sessionFilterValue(preferences)), ["claude:wsl:a"]);
  preferences.sessionInstances["claude:wsl:a"] = false;
  assert.deepEqual(Array.from(context.sessionFilterValue(preferences)), ["claude:wsl:b"]);
  preferences.sessionInstances["claude:wsl:b"] = false;
  assert.deepEqual(Array.from(context.sessionFilterValue(preferences)), ["claude:wsl:a"]);
  assert.equal(context.sessionTypeVisible({ instanceId: "codex:local" }, preferences), false);
});

function sessionFilterPopupHarness({ enabled = ["codex:local", "claude:wsl:a", "claude:wsl:b"], filter = "all" } = {}) {
  const listeners = new Map(), saved = [], nodes = new Map();
  let document;
  class PopupNode {
    constructor(tag, className = "", text = "") {
      this.tag = tag; this.className = className; this.textContent = text; this.children = [];
      this.dataset = {}; this.attributes = new Map(); this.disabled = false; this.isConnected = true;
      this.classList = { toggle() {} };
    }
    append(...children) { for (const child of children) { child.parentElement = this; this.children.push(child); } }
    replaceChildren(...children) {
      for (const child of this.children) child.parentElement = null;
      this.children = []; this.append(...children);
    }
    contains(node) { return this === node || this.children.some(child => child.contains(node)); }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    removeAttribute(name) { this.attributes.delete(name); }
    hasAttribute(name) { return this.attributes.has(name); }
    closest() { return null; }
    focus() { document.activeElement = this; this.onfocus?.(); }
    click() { if (!this.disabled) this.onclick?.(); }
    scrollIntoView() {}
    remove() {
      if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this);
      this.parentElement = null; this.isConnected = false;
    }
    querySelectorAll(selector) {
      const descendants = [];
      const visit = node => { for (const child of node.children) { descendants.push(child); visit(child); } };
      visit(this);
      return descendants.filter(node => selector.split(",").some(part => {
        if (part.startsWith("button") && node.tag !== "button") return false;
        if (part.includes(":not(:disabled)") && node.disabled) return false;
        const checked = part.match(/\[(aria-selected|aria-checked)="true"\]/);
        return !checked || node.getAttribute(checked[1]) === "true";
      }));
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
  }
  const element = (tag, className, text) => new PopupNode(tag, className, text);
  document = { activeElement: null, body: element("body"), documentElement: { dataset: {} },
    addEventListener: (name, callback) => listeners.set(name, callback) };
  const trigger = element("button"); nodes.set("#sidebar-type-filter", trigger);
  const instances = [
    { id: "codex:local", backend: "codex", label: "Codex", ready: true, legacyDefault: true },
    { id: "claude:wsl:a", backend: "claude", label: "Claude · A", ready: true, legacyDefault: true },
    { id: "claude:wsl:b", backend: "claude", label: "Claude · B", ready: true },
    { id: "claude:wsl:off", backend: "claude", label: "Claude · 未开启", ready: true },
  ];
  const context = vm.createContext({
    document, element, icon: name => element("span", "ui-icon", name),
    $: selector => nodes.get(selector) || null,
    button(text, label, callback, className) {
      const node = element("button", className, text); node.title = label; node.onclick = callback; return node;
    },
    sessionInstances: instances, appearance: { sessionFilter: filter,
      sessionInstances: Object.fromEntries(instances.map(instance => [instance.id, enabled.includes(instance.id)])) },
    activePopup: null, contextUsageTip: null, recentLimit: 100, projectVisibleThreadCounts: new Map(),
    fitPopup() {}, closeComposerMenus() {}, closeContextUsageTip() {}, closeModelMenu() {},
    applyAppearance(save) { assert.equal(save, true); saved.push(JSON.parse(JSON.stringify(context.appearance))); },
    renderSidebar: () => context.renderSessionFilter(), updateComposer() {}, notice() {}, loadSidebar: async () => {},
  });
  vm.runInContext(sourceSection("function closePopup(", "function fitPopup("), context);
  const filterStart = app.indexOf('  const typeFilter = $("#sidebar-type-filter");');
  const outsideStart = app.indexOf('document.addEventListener("pointerdown", event => {');
  vm.runInContext(app.slice(filterStart, app.indexOf("  const setSidebarSearchOpen =", filterStart))
    + app.slice(outsideStart, app.indexOf("function checkModelMenuFocus()", outsideStart)), context);
  const options = () => context.activePopup?.list.querySelectorAll("button") || [];
  const find = value => options().find(node => node.dataset.value === value);
  return { context, trigger, instances, saved, nodes, element, options, find,
    open: () => trigger.onclick(),
    click(value) { const node = find(value); assert.ok(node, `菜单缺少 ${value}`); node.focus(); node.click(); },
    checked: () => options().filter(node => node.getAttribute("aria-checked") === "true").map(node => node.dataset.value),
    outside: () => listeners.get("pointerdown")({ target: element("button") }),
    focusOutside: () => listeners.get("focusin")({ target: element("input") }),
  };
}

test("实例多选菜单连续选择并显示勾选，全部会话同步清除其它项且迟到旧节点不覆盖", () => {
  const h = sessionFilterPopupHarness(); h.open();
  assert.deepEqual(h.options().map(node => node.dataset.value),
    ["all", "codex:local", "claude:wsl:a", "claude:wsl:b"]);
  assert.ok(h.options().every(node => node.getAttribute("role") === "menuitemcheckbox"));
  assert.deepEqual(h.checked(), ["all"]);
  const popup = h.context.activePopup.node;
  const originalOption = h.find("codex:local");
  h.context.renderSidebar();
  assert.equal(h.find("codex:local"), originalOption, "普通会话刷新不能替换正被点击的菜单项");
  h.click("codex:local"); h.click("claude:wsl:a");
  assert.equal(h.context.activePopup.node, popup);
  assert.deepEqual(h.checked(), ["codex:local", "claude:wsl:a"]);
  assert.match(h.trigger.getAttribute("aria-label"), /已选 2 个会话实例/);
  const stale = h.find("claude:wsl:b"), writes = h.saved.length;
  h.click("all");
  assert.equal(h.context.appearance.sessionFilter, "all");
  assert.deepEqual(h.checked(), ["all"]);
  stale.onclick();
  assert.equal(h.saved.length, writes + 1, "重绘前节点的迟到点击必须忽略");
  assert.deepEqual(h.checked(), ["all"]);
  h.click("claude:wsl:b");
  assert.deepEqual(h.checked(), ["claude:wsl:b"], "从全部切换具体实例时不能恢复之前的多选");
  h.click("claude:wsl:b");
  assert.deepEqual(h.checked(), ["all"], "取消最后一个实例后恢复全部");
  h.outside(); assert.equal(h.context.activePopup, null);
});

test("只有一个已开启实例时点击关闭菜单，多个实例时留到点击或聚焦外部再关闭", () => {
  for (const enabled of [[], ["claude:wsl:a"], ["codex:local", "claude:wsl:a"]]) {
    for (const choice of ["all", enabled[0]].filter(Boolean)) {
      const h = sessionFilterPopupHarness({ enabled }); h.open(); h.click(choice);
      assert.equal(!!h.context.activePopup, enabled.length > 1);
      if (h.context.activePopup) { h.focusOutside(); assert.equal(h.context.activePopup, null); }
    }
  }
});

test("实例开关变化保留其余多选，菜单立即隐藏关闭的实例并更新选择后的收起规则", () => {
  const h = sessionFilterPopupHarness({ filter: ["claude:wsl:a", "claude:wsl:b"] });
  const root = h.element("div"); h.nodes.set("#appearance-session-instances", root);
  h.context.renderSessionInstances(); h.open();
  const controls = root.querySelectorAll("input");
  const disable = id => {
    const control = controls.find(node => node.dataset.instanceId === id);
    assert.ok(control); control.checked = false; control.onchange();
  };
  disable("claude:wsl:a");
  assert.deepEqual(h.checked(), ["claude:wsl:b"]);
  assert.equal(h.find("claude:wsl:a"), undefined);
  assert.equal(h.context.toggleSessionFilter("claude:wsl:a"), false, "已关闭的旧目标不能被迟到事件重新选中");
  disable("claude:wsl:b");
  assert.deepEqual(h.options().map(node => node.dataset.value), ["all", "codex:local"]);
  assert.deepEqual(h.checked(), ["all"]);
  h.click("codex:local"); assert.equal(h.context.activePopup, null);
});

test("实例目录的迟到读取使用当前多选状态，不能在选择全部后恢复旧筛选", () => {
  const h = sessionFilterPopupHarness(); h.open(); h.click("codex:local"); h.click("claude:wsl:a");
  const result = { instances: h.instances, types: [{ id: "codex", label: "Codex" }, { id: "claude", label: "Claude" }] };
  h.click("all"); h.context.applySessionCatalog(result); h.context.renderSessionFilter();
  assert.deepEqual(h.checked(), ["all"]);
  h.click("claude:wsl:b"); h.context.applySessionCatalog(result); h.context.renderSessionFilter();
  assert.deepEqual(h.checked(), ["claude:wsl:b"]);
  assert.equal(h.context.sessionTypeVisible({ instanceId: "codex:local" }), false);
  assert.equal(h.context.sessionTypeVisible({ instanceId: "claude:wsl:b" }), true);
});

test("多选筛选持久恢复时去重，混入全部项时以全部为准，损坏项不能进入筛选", () => {
  const disk = storage(), context = vm.createContext({ localStorage: disk, matchMedia: () => ({ matches: false }) });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  for (const [saved, expected] of [
    [["codex:local", "claude:wsl:test", "codex:local"], ["codex:local", "claude:wsl:test"]],
    [["codex:local", "all", "claude:wsl:test"], "all"],
    [[{}, null, 1, "../other", "claude:wsl:test"], ["claude:wsl:test"]],
    [[], "all"],
  ]) {
    disk.setItem("codex-elves-web-appearance", JSON.stringify({ sessionFilter: saved }));
    const filter = context.readAppearance().sessionFilter;
    assert.deepEqual(Array.isArray(filter) ? Array.from(filter) : filter, expected);
  }
});

test("PC 会话菜单仅图标切换或生命周期清理会关闭，手机普通关闭行为保留", async () => {
  for (const desktop of [true, false]) {
    const context = vm.createContext({
      opened: true, suspended: false, ticker: null, cached: null,
      pinned: () => desktop, clearInterval() {}, cancelPress() {}, hideContext() {}, stopPositionTracking() {},
      trigger: { isConnected: true, disabled: false, setAttribute() {}, focus() {} },
      layer: { hidden: false, hasAttribute: () => false, classList: { remove() {} } },
      open: async () => { context.opened = true; context.layer.hidden = false; },
    });
    vm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function close("),
      sessionMenuSource.indexOf("  function suspend(")) + sessionMenuSource.slice(
      sessionMenuSource.indexOf("  function toggle()"), sessionMenuSource.indexOf('  listen(trigger, "click"')), context);
    context.close(true);
    assert.equal(context.opened, desktop, "外部点击、Escape 和普通操作共用关闭入口");
    context.opened = true; context.layer.hidden = false;
    await context.toggle();
    assert.equal(context.opened, false, "再次点击图标必须关闭");
    await context.toggle();
    assert.equal(context.opened, true, "点击图标重新打开");
    context.close(false, true);
    assert.equal(context.opened, false, "授权变化或销毁必须清理菜单");
  }
});

function sessionMenuPositionHarness({ width = 1200, height = 800, related = false } = {}) {
  let anchor = { left: width - 62, top: 20, width: 32, height: 32 }, frameId = 0;
  const frames = new Map(), listeners = new Map(), writes = [], attributes = new Map();
  const rectangle = box => ({ ...box, right: box.left + box.width, bottom: box.top + box.height });
  const menu = { style: new Proxy({}, { set(target, name, value) {
    writes.push([name, value]); target[name] = value; return true;
  } }) };
  const context = nodeVm.createContext({
    opened: true, suspended: false, disposed: false, ticker: null, cached: null,
    positionFrame: null, positionSignature: "", contextSource: null, contextItem: null, contextPosition: null,
    menu, context: {}, owns: () => true, canMutate: () => true, closeSideThread() {},
    pinned: () => width > 760, desktop: () => width > 760,
    clearInterval() {}, cancelPress() {}, focusEdge() {},
    document: { hidden: false, body: { append() {} } },
    window: { innerWidth: width, innerHeight: height,
      requestAnimationFrame(callback) { frames.set(++frameId, callback); return frameId; },
      cancelAnimationFrame(id) { frames.delete(id); } },
    trigger: { isConnected: true, disabled: false, hidden: false,
      getBoundingClientRect: () => rectangle(anchor),
      closest: selector => related && selector === ".related-session-view" ? {} : null,
      setAttribute: (name, value) => attributes.set(name, value), focus() {} },
    layer: { hidden: false, hasAttribute: () => false, classList: { add() {}, remove() {} } },
    actionMenu: { hidden: true, style: {}, getBoundingClientRect: () => ({ width: 180, height: 40 }) },
    closeAction: { disabled: false, focus() {} },
    listen(_target, name, handler) { listeners.set(name, handler); },
  });
  const visibilityStart = sessionMenuSource.indexOf('  listen(document, "visibilitychange"');
  nodeVm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function hideContext("),
    sessionMenuSource.indexOf("  function updatePermissions("))
    + sessionMenuSource.slice(sessionMenuSource.indexOf("  function stopPositionTracking("),
      sessionMenuSource.indexOf("  function bindSideThread("))
    + sessionMenuSource.slice(visibilityStart, sessionMenuSource.indexOf("  syncContext();", visibilityStart)), context);
  return { context, frames, writes, attributes,
    move(box) { anchor = { ...anchor, ...box }; },
    frame() { const pending = [...frames.values()]; frames.clear(); for (const callback of pending) callback(); },
    visibility(hidden) { context.document.hidden = hidden; listeners.get("visibilitychange")(); },
  };
}

test("会话菜单跟随侧栏、右侧面板及关联页动画移动，静止时不重复写定位样式", () => {
  for (const related of [false, true]) {
    const h = sessionMenuPositionHarness({ related }), { context, frames, writes } = h;
    context.fit(); context.startPositionTracking(); context.startPositionTracking();
    assert.equal(context.menu.style.left, "870px");
    assert.equal(context.menu.style.top, "62px");
    assert.equal(frames.size, 1, "重复启动不能累积跟踪任务");
    const initialWrites = writes.length;
    h.frame();
    assert.equal(writes.length, initialWrites, "布局不变时只读锚点，不反复写样式");
    for (const [left, top] of [[1000, 24], [850, 28], [1138, 20]]) {
      h.move({ left, top }); h.frame();
      assert.equal(context.menu.style.left, `${left + 32 - 300}px`);
      assert.equal(context.menu.style.top, `${top + 32 + 10}px`);
      assert.equal(frames.size, 1, "没有 resize 或 scroll 事件也持续跟随");
    }
    context.close(false, true);
    assert.equal(frames.size, 0);
  }
});

test("会话菜单保持 PC 短窗口宽度、手机比例及可视视口边界", () => {
  const desktop = sessionMenuPositionHarness({ height: 500 });
  desktop.context.fit();
  assert.equal(desktop.context.menu.style.width, "300px");
  const phone = sessionMenuPositionHarness({ width: 390, height: 844 });
  phone.context.fit();
  assert.equal(phone.context.menu.style.width, "253.5px");
  const h = sessionMenuPositionHarness(), { context } = h;
  context.fit(); context.startPositionTracking();
  context.window.visualViewport = { offsetLeft: 100, offsetTop: 40, width: 700, height: 400 };
  h.move({ left: 780, top: 50 }); h.frame();
  assert.equal(context.menu.style.left, "492px", "右侧保留 8px");
  assert.equal(context.menu.style.top, "92px");
  assert.equal(context.menu.style.maxHeight, "320px");
  h.move({ left: 104, top: 400 }); h.frame();
  assert.equal(context.menu.style.left, "108px", "左侧保留 8px");
  assert.equal(context.menu.style.top, "432px");
  assert.equal(context.menu.style.maxHeight, "0px", "底边空间不足时不溢出视口");
  context.close(false, true);
});

test("会话菜单挂起、后台和强制关闭会取消跟踪，恢复后重新对齐", () => {
  const h = sessionMenuPositionHarness(), { context, frames } = h;
  context.fit(); context.startPositionTracking();
  context.close();
  assert.equal(frames.size, 1, "PC 普通关闭仍保留菜单及跟踪");
  context.suspend();
  assert.equal(frames.size, 0);
  h.move({ left: 900 });
  context.resume(); h.frame();
  assert.equal(context.menu.style.left, "632px");
  assert.equal(frames.size, 1);
  h.visibility(true);
  assert.equal(context.opened, true, "PC 菜单在后台保留打开状态");
  assert.equal(frames.size, 0);
  h.move({ left: 800 });
  h.visibility(false);
  assert.equal(context.menu.style.left, "532px");
  assert.equal(frames.size, 1);
  context.close(false, true);
  assert.equal(frames.size, 0);
  assert.equal(context.layer.hidden, true);
});

test("会话菜单锚点隐藏、移除、禁用或移出视口时，即使 PC 固定显示也会清理", () => {
  for (const invalidate of [
    h => { h.context.trigger.hidden = true; },
    h => { h.context.trigger.isConnected = false; },
    h => { h.context.trigger.disabled = true; },
    h => h.move({ left: 0, top: 0, width: 0, height: 0 }),
    h => h.move({ left: -40 }),
    h => h.move({ top: 800 }),
  ]) {
    const h = sessionMenuPositionHarness(), { context, frames } = h;
    context.fit(); context.startPositionTracking();
    invalidate(h); h.frame();
    assert.equal(context.opened, false);
    assert.equal(context.layer.hidden, true);
    assert.equal(h.attributes.get("aria-expanded"), "false");
    assert.equal(frames.size, 0, "失去锚点后不能留下重试帧");
  }
});

test("首次打开时锚点不可见，不会在定位清理后重新显示弹层", async () => {
  const h = sessionMenuPositionHarness(), { context } = h;
  context.opened = false; context.layer.hidden = true; context.trigger.hidden = true;
  Object.assign(context, { beforeOpen: null, syncContext: () => ({}), entryFor: () => ({}), restore() {},
    render: () => context.fit(), refresh: async () => null, focusEdge: () => assert.fail("隐藏菜单不能聚焦"),
    setInterval: () => assert.fail("隐藏菜单不能启动刷新"), report: error => { throw error; } });
  nodeVm.runInContext(sessionMenuSource.slice(sessionMenuSource.indexOf("  function open("),
    sessionMenuSource.indexOf("  function toggle()")), context);
  await context.open();
  assert.equal(context.opened, false);
  assert.equal(context.layer.hidden, true);
  assert.equal(h.attributes.get("aria-expanded"), "false");
  assert.equal(h.frames.size, 0);
});

test("侧边会话操作子菜单随所属菜单行移动，保留右键位置的行内偏移", () => {
  const h = sessionMenuPositionHarness(), { context } = h;
  context.fit(); context.startPositionTracking();
  const row = { setAttribute() {}, getBoundingClientRect: () => {
    const left = parseFloat(context.menu.style.left) + 16, top = parseFloat(context.menu.style.top) + 240;
    return { left, top, right: left + 268, bottom: top + 32 };
  } };
  const original = row.getBoundingClientRect();
  context.showContext(row, { id: "side" }, "owner", { x: original.left + 12, y: original.top + 8 });
  assert.equal(context.actionMenu.style.left, "898px");
  assert.equal(context.actionMenu.style.top, "310px");
  h.move({ left: 900, top: 30 }); h.frame();
  assert.equal(context.actionMenu.style.left, "660px");
  assert.equal(context.actionMenu.style.top, "320px");
  context.close(false, true);
  assert.equal(context.actionMenu.hidden, true);
});

test("三个 Claude 与两个 Codex 实例独立迁移、启用和筛选，新类型沿用同一逻辑", () => {
  const instances = [
    { id: "codex:local", backend: "codex", ready: true, legacyDefault: true },
    { id: "codex:wsl:3", backend: "codex", ready: false },
    { id: "claude:local", backend: "claude", ready: false },
    { id: "claude:wsl:1", backend: "claude", ready: true, legacyDefault: true },
    { id: "claude:wsl:2", backend: "claude", ready: true },
  ].map(instance => ({ ...instance, label: instance.id }));
  const preferences = { sessionTypes: { codex: true, claude: true }, sessionFilter: "all" };
  const context = vm.createContext({ sessionInstances: instances, appearance: preferences });
  assert.equal(context.migrateSessionInstances(preferences, instances), true);
  assert.deepEqual(Object.entries(preferences.sessionInstances).filter(([, enabled]) => enabled).map(([id]) => id),
    ["codex:local", "claude:wsl:1"]);
  preferences.sessionInstances["claude:wsl:1"] = false;
  preferences.sessionInstances["claude:wsl:2"] = true;
  preferences.sessionInstances["claude:local"] = true;
  assert.equal(context.migrateSessionInstances(preferences, instances), false);
  assert.equal(context.sessionTypeVisible({ backend: "claude", instanceId: "claude:wsl:1" }), false);
  assert.equal(context.sessionTypeVisible({ backend: "claude", instanceId: "claude:wsl:2" }), true);
  assert.deepEqual(Array.from(context.newChatBackendOptions(), option => option.value), ["codex:local", "claude:wsl:2"]);
  instances.push({ id: "future:wsl:4", backend: "future", ready: true, label: "Future · WSL4" });
  preferences.sessionInstances["future:wsl:4"] = true;
  assert.equal(context.newChatBackendOptions().at(-1).value, "future:wsl:4");
  assert.equal(context.sessionTypeVisible({ backend: "future", instanceId: "future:wsl:4" }), true);
  preferences.sessionFilter = "claude";
  assert.equal(context.sessionTypeVisible({ backend: "future", instanceId: "future:wsl:4" }), false);
  // 仅使用内存选项验证：环境名称不改变实例开关、列表和新建入口的规则。
  instances.push({ id: "claude:wsl:third", backend: "claude", ready: true,
    label: "Claude · WSL · claude-us", distribution: "claude-us" });
  preferences.sessionInstances["claude:wsl:third"] = true;
  assert.equal(context.enabledInstanceIds().includes("claude:wsl:third"), true);
  assert.equal(context.newChatBackendOptions().some(option => option.value === "claude:wsl:third"), true);
  for (const id of Object.keys(preferences.sessionInstances)) preferences.sessionInstances[id] = false;
  assert.equal(context.newChatBackendOptions().length, 0, "允许所有实例关闭，不暗中重新启用默认实例");
});

test("实例偏好重新打开后保留全部关闭，草稿及模型上下文不跨 WSL 实例", () => {
  const disk = storage();
  const context = vm.createContext({ localStorage: disk, matchMedia: () => ({ matches: false }),
    creating: true, creationBackend: "claude", creationInstanceId: "claude:wsl:1",
    session: { claudeInstanceId: "claude:wsl:1" } });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  disk.setItem("codex-elves-web-appearance", JSON.stringify({ sessionInstances: {}, sessionTypes: { codex: true, claude: true } }));
  context.appearance = context.readAppearance();
  assert.equal(context.sessionInstanceEnabled("codex:local"), false);
  assert.equal(context.newChatBackendOptions().length, 0);
  const a = context.newDraftKey("project", "claude", "claude:wsl:1");
  const b = context.newDraftKey("project", "claude", "claude:wsl:2");
  assert.notEqual(a, b);
  assert.equal(context.newDraftKey("project", "claude"), a, "旧失败恢复只归属原默认实例");
  const modelsA = context.modelContextKey();
  context.creationInstanceId = "claude:wsl:2";
  assert.notEqual(context.modelContextKey(), modelsA);
  assert.equal(context.modelContext().instanceId, "claude:wsl:2");
});

test("设置强制切换新建类型时保留文字图片，清理旧模型目标并隔离原草稿", () => {
  const selector = { value: "codex" }, saved = new Map();
  const initial = { text: "继续这项工作", attachments: [{ name: "proof.png" }],
    skills: [{ name: "codex-skill", path: "/skill" }], goalMode: "goal-id" };
  let current = structuredClone(initial);
  const context = vm.createContext({
    creating: true, creationBackend: "codex", creationProject: "original-project", selectionVersion: 2,
    selectedSkills: initial.skills, selectedGoal: initial.goalMode, selectedModel: "codex-model", selectedEffort: "xhigh",
    appearance: { sessionTypes: { codex: false, claude: true } },
    $: id => id === "#new-chat-backend" ? selector : null,
    snapshotDraft: () => structuredClone(current),
    rememberDraft: () => saved.set(context.draftKey(), structuredClone(current)),
    storeDraft: (key, value) => saved.set(key, structuredClone(value)),
    restoreDraft: key => { current = structuredClone(saved.get(key)); },
    resetComposerOptions: () => { context.selectedModel = context.selectedEffort = ""; },
    closePopup() {}, closeComposerMenus() {}, renderMessages() {}, updateComposer() {}, renderApprovals() {},
    setSelectOptions: (node, options, value) => { node.value = value; node.selectOptions = options; },
    randomId: () => "new-selection",
  });
  vm.runInContext(sourceSection("function draftKey()", "function storedDraft("), context);
  vm.runInContext(sourceSection("function changeCreationBackend(", "function selectCreatedThread("), context);
  context.setCreationBackend("claude");
  assert.equal(context.creationProject, "original-project");
  assert.equal(context.creationBackend, "claude");
  assert.equal(selector.value, "claude:wsl:test");
  assert.equal(context.selectionVersion, 3);
  assert.equal(context.selectedModel, "");
  assert.equal(context.selectedEffort, "");
  assert.equal(context.selectedGoal, null);
  assert.equal(context.selectedSkills.length, 0);
  assert.deepEqual(current, { ...initial, skills: [], goalMode: null });
  assert.deepEqual(saved.get("$new:original-project"), initial, "原 Codex 草稿仍可恢复");
  context.creating = false;
  context.setCreationBackend("codex");
  assert.equal(context.creationBackend, "claude", "已创建会话不能通过此入口切换类型");
});

test("Claude 队列编辑使用会话图片能力，桌面离线不禁用保存且不借用其他会话能力", () => {
  const id = "claude-01234567-89ab-4cde-8fab-0123456789ab";
  const context = vm.createContext({
    selectedId: id, thread: { id, backend: "claude", capabilities: { queuedImageEditing: true, queueEditRecovery: true } },
    session: { capabilities: { queuedImageEditing: false, queueEditRecovery: false } },
    editing: true, hasAttachments: true, editingQueuedMessage: { imageEditing: true },
    composerAttachmentsPayload: () => [{ name: "new.png" }],
  });
  const unsupported = vm.runInContext(`() => {${sourceSection("  const unsupportedImages =", "  const queueImageNote =")}
    return unsupportedImages; }`, context);
  assert.equal(unsupported(), false);
  assert.equal(context.queueCapability("queueEditRecovery"), true);
  assert.equal(context.queueCapability("queueEditRecovery", "claude-11111111-1111-4111-8111-111111111111"), false);
  context.thread.capabilities.queuedImageEditing = false;
  context.session.capabilities.queuedImageEditing = true;
  assert.equal(unsupported(), true, "Codex 的能力不能冒充 Claude 支持");
});

test("指令编辑将CRLF规范为textarea文本并恢复原换行，不制造未保存变更", () => {
  const context = vm.createContext();
  vm.runInContext(app.slice(app.indexOf("const normalizeInstructionText ="), app.indexOf("const instructionDrafts =")), context);
  context.original = "# 全局指令\r\n中文说明。\r\n";
  assert.equal(vm.runInContext('instructionFileText(normalizeInstructionText(original), "\\r\\n")', context), context.original);
  assert.equal(vm.runInContext('normalizeInstructionText(original)', context), "# 全局指令\n中文说明。\n");
});

function selectComponentHarness() {
  const opened = [], refreshed = [];
  function element(tag, _class = "", text = "") {
    const attributes = new Map();
    return { tag, textContent: text, value: "", disabled: false, children: [], events: [],
      setAttribute(name, value) { attributes.set(name, String(value)); },
      getAttribute(name) { return attributes.get(name) ?? null; },
      dispatchEvent(event) { this.events.push(event); this["on" + event.type]?.({ target: this, type: event.type }); return true; },
      append(...children) { this.children.push(...children); },
      replaceChildren(...children) { this.children = children; } };
  }
  const component = createSelectComponent({
    element, icon: name => element("span", "ui-icon", name),
    button(text, label, action) {
      const node = element("button", "", text); node.setAttribute("aria-label", label); node.onclick = action; return node;
    },
    togglePopup: node => opened.push({ node }),
    openPopup: (node, options) => opened.push({ node, ...options }),
    refreshPopup: node => refreshed.push(node),
  });
  return { element, ...component, opened, refreshed };
}

test("统一 Select 的程序同步不触发 change，用户选择只触发一次并拒绝禁用项", () => {
  const h = selectComponentHarness();
  const node = h.selectControl("weight", "程序字重", [
    { value: "400", label: "常规" }, { value: "500", label: "中等" }, { value: "600", label: "半粗", disabled: true },
  ], 400);
  assert.equal(node.value, "400");
  assert.equal(node.children[0].textContent, "常规");
  assert.equal(node.getAttribute("aria-haspopup"), "listbox");
  assert.equal(h.commitSelectValue(node, 500), true);
  assert.equal(node.value, "500"); assert.equal(node.events.length, 1);
  assert.equal(node.events[0].bubbles, true);
  assert.equal(h.commitSelectValue(node, "500"), false);
  assert.equal(h.commitSelectValue(node, "600"), false);
  assert.equal(h.commitSelectValue(node, "missing"), false);
  h.setSelectValue(node, "400");
  assert.equal(node.events.length, 1);
  node.disabled = true;
  assert.equal(h.commitSelectValue(node, "500"), false);
  assert.equal(node.events.length, 1);
});

test("统一 Select 保留失效目标及空状态，键盘打开不越过禁用或输入法状态", () => {
  const h = selectComponentHarness(), node = h.selectControl("environment", "指令环境", [], "");
  assert.equal(node.children[0].textContent, "暂无可用选项");
  h.setSelectOptions(node, [{ value: "old", label: "原环境（暂不可用）", disabled: true },
    { value: "local", label: "本机" }], "old");
  assert.equal(node.value, "old");
  assert.equal(node.children[0].textContent, "原环境（暂不可用）");
  assert.equal(h.commitSelectValue(node, "old"), false);
  let prevented = 0;
  for (const key of ["ArrowDown", "ArrowUp", "Home", "End"])
    node.onkeydown({ key, preventDefault() { ++prevented; } });
  assert.equal(prevented, 4);
  assert.deepEqual(h.opened.map(entry => entry.edge), [null, null, "first", "last"]);
  node.disabled = true;
  node.onkeydown({ key: "Home", preventDefault() { throw new Error("禁用时不能打开"); } });
  node.disabled = false;
  node.onkeydown({ key: "Home", isComposing: true, preventDefault() { throw new Error("输入法组合时不能打开"); } });
  assert.equal(h.opened.length, 4);
});

function instructionSettingsHarness(api) {
  const select = selectComponentHarness(), { element } = select;
  const context = vm.createContext({ api, element, setSelectOptions: select.setSelectOptions,
    connected: true, session: { device: { id: "instruction-device" } },
    privateInputScope: () => "instruction-device", confirmAction: async () => true,
    $: (id, dialog) => dialog.nodes.get(id), sidebar: { projects: [] } });
  vm.runInContext(sourceSection("const normalizeInstructionText =", "function openAppearance("), context);
  function mount({ withDetectionControls = false } = {}) {
    const nodes = new Map();
    for (const name of ["content", "status", "save", "reload", "path"])
      nodes.set("#instruction-" + name, element("input"));
    nodes.set("#instruction-instance", select.selectControl("instruction-instance", "会话实例", [], ""));
    if (withDetectionControls) for (const name of ["detect-environments", "environments-status", "environments"])
      nodes.set("#appearance-" + name, element("div"));
    const dialog = { isConnected: true, nodes };
    const settings = context.mountRemoteSettings(dialog);
    return { dialog, settings, get: id => nodes.get("#" + id),
      async chooseInstance(id) {
        select.commitSelectValue(nodes.get("#instruction-instance"), id);
        await new Promise(resolve => setImmediate(resolve));
      },
      edit(text) { const input = nodes.get("#instruction-content"); input.value = text; input.oninput(); } };
  }
  return { context, mount };
}

test("指令保存迟到不能清掉重新打开窗口后的新草稿", async () => {
  const pendingSave = deferred();
  let stored = { content: "旧指令\n", revision: "r1", exists: true, path: "/fixture/AGENTS.md" };
  const h = instructionSettingsHarness((path, body) => {
    if (path.endsWith("environments")) return Promise.resolve({ environments: [
      { id: "local", label: "本机" },
      { id: "wsl:claude-us", label: "WSL · claude-us", executables: { claude: "/fixture/bin/claude" } },
      { id: "wsl:unavailable", label: "不可用环境", error: "模拟环境不可用" },
    ], instances: [
      { id: "codex:local", label: "Codex · 本机", backend: "codex", environmentId: "local" },
      { id: "claude:wsl:fixture", label: "Claude · WSL · claude-us", backend: "claude", environmentId: "wsl:claude-us" },
      { id: "claude:wsl:unavailable", label: "Claude · 不可用环境", backend: "claude", environmentId: "wsl:unavailable" },
    ] });
    return Object.hasOwn(body, "content") ? pendingSave.promise : Promise.resolve(stored);
  });
  const a = h.mount();
  await a.settings.show("instructions");
  assert.deepEqual(Array.from(a.get("instruction-instance").selectOptions.filter(option => !option.disabled), option => option.value),
    ["codex:local", "claude:wsl:fixture"], "所有正常实例均可编辑全局指令，不按发行版名称限制");
  a.edit("第一次保存\n");
  const save = a.get("instruction-save").onclick();
  a.settings.dispose(); a.dialog.isConnected = false;
  const b = h.mount();
  await b.settings.show("instructions");
  b.edit("第二个窗口尚未保存的新草稿\n");
  stored = { ...stored, content: "第一次保存\n", revision: "r2" };
  pendingSave.resolve(stored);
  await save;
  b.settings.dispose(); b.dialog.isConnected = false;
  const c = h.mount();
  await c.settings.show("instructions");
  assert.equal(c.get("instruction-content").value, "第二个窗口尚未保存的新草稿\n");
  assert.match(c.get("instruction-status").textContent, /原文件也已变化/);
});

test("重新检测实例失效时保留明确的目标与草稿，并禁用保存", async () => {
  let available = true, writes = 0;
  const h = instructionSettingsHarness(async (path, body) => {
    if (path.endsWith("environments")) return { environments: available
      ? [{ id: "wsl:test", label: "测试 WSL", executables: { claude: "/fixture/bin/claude" } }, { id: "local", label: "本机" }]
      : [{ id: "local", label: "本机" }], instances: [
        ...(available ? [{ id: "claude:wsl:test", backend: "claude", label: "Claude · 测试 WSL", environmentId: "wsl:test" }] : []),
        { id: "codex:local", backend: "codex", label: "Codex · 本机", environmentId: "local" },
      ] };
    if (Object.hasOwn(body, "content")) ++writes;
    return { content: "WSL 指令", revision: "r1", exists: true, path: "/test/AGENTS.md" };
  });
  const page = h.mount({ withDetectionControls: true });
  await page.settings.show("instructions");
  await page.chooseInstance("claude:wsl:test");
  page.edit("保留 WSL 草稿");
  available = false;
  await page.get("appearance-detect-environments").onclick();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(page.get("instruction-instance").value, "claude:wsl:test", "不能自动跳成本机并保留旧实例的正文");
  assert.equal(page.get("instruction-instance").selectOptions.find(option => option.value === "claude:wsl:test").disabled, true);
  assert.equal(page.get("instruction-content").value, "保留 WSL 草稿");
  assert.equal(page.get("instruction-save").disabled, true);
  await page.get("instruction-save").onclick();
  assert.equal(writes, 0);
});

test("全局指令只按实例读写，实例开关不限制编辑，草稿和换行各自保留", async () => {
  const catalog = { environments: [
    { id: "local", user: "" }, { id: "wsl:fixture", user: "fixture-user", executables: { claude: "/fixture/bin/claude" } },
  ], instances: [
    { id: "codex:local", backend: "codex", label: "Codex · 本机", environmentId: "local", user: "" },
    { id: "claude:wsl:fixture", backend: "claude", label: "Claude · WSL · fixture", environmentId: "wsl:fixture", user: "fixture-user" },
  ] };
  const stored = new Map([
    ["codex:local", { content: "Codex 原文\r\n", revision: "c1", exists: true, path: "/fixture/.codex/AGENTS.md" }],
    ["claude:wsl:fixture", { content: "Claude 原文\r\n", revision: "a1", exists: true, path: "/fixture/.claude/CLAUDE.md" }],
  ]), requests = [];
  const h = instructionSettingsHarness(async (path, body) => {
    if (path.endsWith("environments")) return catalog;
    requests.push({ ...body });
    if (Object.hasOwn(body, "content")) stored.set(body.instanceId,
      { ...stored.get(body.instanceId), content: body.content, revision: "saved" });
    return stored.get(body.instanceId);
  });
  h.context.appearance.sessionInstances = { "codex:local": true, "claude:wsl:fixture": false };
  const page = h.mount();
  await page.settings.show("instructions");
  page.edit("Codex 未保存草稿\n");
  await page.chooseInstance("claude:wsl:fixture");
  assert.equal(page.get("instruction-content").value, "Claude 原文\n");
  page.edit("Claude 全局新指令\n");
  await page.get("instruction-save").onclick();
  assert.deepEqual(requests.at(-1), { instanceId: "claude:wsl:fixture",
    content: "Claude 全局新指令\r\n", expectedRevision: "a1" });
  assert.equal(page.get("instruction-save").disabled, true);
  await page.chooseInstance("codex:local");
  assert.equal(page.get("instruction-content").value, "Codex 未保存草稿\n");
  assert.equal(stored.get("codex:local").content, "Codex 原文\r\n");
  assert.ok(requests.filter(body => !Object.hasOwn(body, "content"))
    .every(body => Object.keys(body).join() === "instanceId"));
});

test("切换指令实例后忽略旧实例的迟到回包，不能覆盖当前文件", async () => {
  const pending = deferred();
  const h = instructionSettingsHarness(async (path, body) => {
    if (path.endsWith("environments")) return { environments: [{ id: "local" },
      { id: "wsl:fixture", executables: { claude: "/fixture/bin/claude" } }], instances: [
      { id: "codex:local", backend: "codex", label: "Codex · 本机", environmentId: "local" },
      { id: "claude:wsl:fixture", backend: "claude", label: "Claude · WSL · fixture", environmentId: "wsl:fixture" },
    ] };
    if (body.instanceId === "codex:local") return pending.promise;
    return { content: "当前 Claude 指令", revision: "a1", exists: true, path: "/fixture/CLAUDE.md" };
  });
  const page = h.mount(), initialRead = page.settings.show("instructions");
  await new Promise(resolve => setImmediate(resolve));
  await page.chooseInstance("claude:wsl:fixture");
  pending.resolve({ content: "迟到 Codex 指令", revision: "c1", exists: true, path: "/fixture/AGENTS.md" });
  await initialRead;
  assert.equal(page.get("instruction-content").value, "当前 Claude 指令");
  assert.equal(page.get("instruction-path").textContent, "/fixture/CLAUDE.md");
});

test("历史保留实例缺少安装或用户不匹配时不能读取全局指令", async () => {
  let reads = 0;
  const h = instructionSettingsHarness(async (path, body) => {
    if (path.endsWith("environments")) return { environments: [
      { id: "local", user: "" }, { id: "wsl:uninstalled", user: "fixture", executables: {} },
      { id: "wsl:other-user", user: "current", executables: { claude: "/fixture/bin/claude" } },
    ], instances: [
      { id: "codex:local", backend: "codex", label: "Codex · 本机", environmentId: "local", user: "" },
      { id: "claude:wsl:uninstalled", backend: "claude", label: "Claude · 已卸载", environmentId: "wsl:uninstalled", user: "fixture" },
      { id: "claude:wsl:other-user", backend: "claude", label: "Claude · 其他用户", environmentId: "wsl:other-user", user: "saved" },
    ] };
    ++reads;
    assert.equal(body.instanceId, "codex:local");
    return { content: "Codex 全局指令", revision: "c1", exists: true, path: "/fixture/AGENTS.md" };
  });
  const page = h.mount();
  await page.settings.show("instructions");
  for (const id of ["claude:wsl:uninstalled", "claude:wsl:other-user"]) {
    assert.equal(page.get("instruction-instance").selectOptions.find(option => option.value === id).disabled, true);
    await page.chooseInstance(id);
    assert.equal(page.get("instruction-instance").value, "codex:local");
  }
  assert.equal(reads, 1);
});

test("Claude 中断修复退役旧气泡并重算分页，同时保留待发送输入和较新的原生消息", () => {
  const context = vm.createContext({
    turns: new Map([
      ["real", { _nativeUserId: "real" }],
      ["interruption", { _nativeUserId: "interruption", items: [{ id: "control-item" }] }],
      ["pending", { items: [{ id: "pending-item", remotePendingInput: true }] }],
      ["new-live", { _nativeUserId: "new-live" }],
    ]),
    turnVersions: new Map([["new-live", 20]]), itemVersions: new Map(), partialItems: new Set(["interruption:control-item"]),
    transcriptEvents: new Map([["old-event", { turnId: "interruption" }]]), messageVersion: 20,
  });
  assert.equal(context.retireClaudeHistoryTurns(["older", "real"], 10, 21), true);
  assert.deepEqual([...context.turns.keys()], ["real", "pending", "new-live"]);
  assert.equal(context.turnVersions.get("interruption"), 21);
  assert.equal(context.itemVersions.get("interruption:control-item"), 21);
  assert.equal(context.partialItems.size, 0); assert.equal(context.transcriptEvents.size, 0);
  assert.equal(context.claudeHistoryCursor(["older", "real"]), "1");
  context.turns.set("older", { _nativeUserId: "older" });
  assert.equal(context.claudeHistoryCursor(["older", "real"]), null);
  assert.equal(context.claudeHistoryCursor(undefined), undefined);
});

test("自动主题浅色阈值默认10，旧设置兼容且保存值收敛至5至40", () => {
  const disk = storage();
  const context = vm.createContext({ localStorage: disk, matchMedia: () => ({ matches: false }) });
  vm.runInContext(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")), context);
  for (const [saved, expected] of [
    [undefined, 10], [null, 10], [5, 5], [10, 10], [25, 25], [35, 35], [40, 40],
    [4, 5], [24, 24], [41, 40], [45, 40], [50, 40], ["40", 10], [35.5, 10],
  ]) {
    disk.setItem("codex-elves-web-appearance", JSON.stringify({ themeMode: "auto", automaticThemeLightLux: saved }));
    context.appearance = context.readAppearance();
    assert.equal(context.currentAutomaticThemeLightLux(), expected);
    assert.equal(context.appearance.themeMode, "auto");
  }
  assert.equal(context.defaultAppearance().automaticThemeLightLux, 10);
});

function previewControlsScrollHarness() {
  class PreviewElement {
    constructor() {
      this.children = []; this.parentElement = null; this.hidden = false; this.inert = false;
      this.scrollTop = 0; this.scrollHeight = 40; this.clientHeight = 40; this.listeners = new Map();
      this.properties = new Map();
      this.style = {
        setProperty: (key, value) => this.properties.set(key, value),
        removeProperty: key => this.properties.delete(key)
      };
    }
    append(...children) {
      for (const child of children) {
        if (child.parentElement) child.parentElement.children.splice(child.parentElement.children.indexOf(child), 1);
        this.children.push(child); child.parentElement = this;
      }
    }
    before(child) {
      const parent = this.parentElement;
      parent.children.splice(parent.children.indexOf(this), 0, child); child.parentElement = parent;
    }
    replaceWith(child) {
      const parent = this.parentElement;
      if (child.parentElement) child.parentElement.children.splice(child.parentElement.children.indexOf(child), 1);
      parent.children.splice(parent.children.indexOf(this), 1, child);
      child.parentElement = parent; this.parentElement = null;
    }
    addEventListener(name, listener) { this.listeners.set(name, listener); }
    removeEventListener(name) { this.listeners.delete(name); }
  }
  const screen = new PreviewElement(), controls = new PreviewElement(), body = new PreviewElement();
  body.scrollHeight = 2000; body.clientHeight = 600; body.scrollTop = 100;
  screen.append(controls, body);
  let active = true;
  const context = vm.createContext({
    Element: PreviewElement, node: () => new PreviewElement(),
    getComputedStyle: element => ({ overflowY: element.overflowY || "visible" })
  });
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function mountPreviewControlsScroll"),
    sessionFiles.indexOf("export function mountSessionFiles")).replace(/^export /gm, ""), context);
  const controller = context.mountPreviewControlsScroll({ body, controls, isActive: () => active });
  const shell = screen.children[0];
  const wheel = options => body.listeners.get("wheel")({
    deltaY: 0, deltaX: 0, deltaMode: 0, target: body, ...options
  });
  return { controller, screen, shell, controls, body, wheel, PreviewElement,
    reveal: () => Number(shell.properties.get("--preview-controls-reveal") ?? 1),
    active: value => { active = value; } };
}

test("文件预览第二行在80px手势内收展，聚焦与重新打开恢复完整工具行", () => {
  const h = previewControlsScrollHarness();
  h.wheel({ deltaY: 40 });
  assert.equal(h.reveal(), .5);
  h.wheel({ deltaY: 120 });
  assert.equal(h.reveal(), 0); assert.equal(h.shell.inert, true);
  h.wheel({ deltaY: -80 });
  assert.equal(h.reveal(), 1); assert.equal(h.shell.inert, false);
  h.wheel({ deltaY: 80 });
  assert.equal(h.reveal(), 0);
  h.controls.listeners.get("focusin")();
  assert.equal(h.reveal(), 1);
  assert.equal(h.shell.properties.has("--preview-controls-rows"), false);
  h.wheel({ deltaY: 500 }); h.controller.reset();
  assert.equal(h.reveal(), 1);
  h.controller.dispose();
  assert.deepEqual(h.screen.children, [h.controls, h.body]);
  assert.equal(h.body.listeners.size, 0);
  assert.equal(h.controls.listeners.size, 0);
});

test("文件预览只响应正文纵向手势，短文件、横向滚动、缩放及内层滚动不会误收起", () => {
  const h = previewControlsScrollHarness();
  h.body.scrollHeight = 600; h.body.scrollTop = 0;
  h.wheel({ deltaY: 100 }); assert.equal(h.reveal(), 1);
  h.body.scrollHeight = 2000;
  h.wheel({ deltaX: 200, deltaY: 50 });
  h.wheel({ ctrlKey: true, deltaY: 200 });
  assert.equal(h.reveal(), 1);
  const nested = new h.PreviewElement();
  nested.scrollHeight = 800; nested.clientHeight = 100; nested.scrollTop = 100; nested.overflowY = "auto";
  h.body.append(nested);
  h.wheel({ deltaY: 200, target: nested }); assert.equal(h.reveal(), 1);
  h.active(false); h.wheel({ deltaY: 200 }); assert.equal(h.reveal(), 1);
  h.active(true);
  h.body.listeners.get("touchstart")({ touches: [{ clientX: 100, clientY: 300 }] });
  h.body.listeners.get("touchmove")({ touches: [{ clientX: 100, clientY: 260 }], target: h.body });
  assert.equal(h.reveal(), .5);
  h.body.listeners.get("touchmove")({ touches: [{ clientX: 200, clientY: 250 }], target: h.body });
  assert.equal(h.reveal(), .5);
  h.body.scrollTop = 800;
  assert.equal(h.reveal(), .5, "恢复滚动位置不应改变工具行展开比例");
  h.controller.dispose();
});

function nativeSourceSection(start, end) {
  const from = native.indexOf(start), to = native.indexOf(end, from);
  assert.ok(from >= 0 && to > from);
  return native.slice(from, to);
}

function nativeCompactionHarness(options = {}) {
  const resumes = [], dispatches = [], refreshes = [];
  let role = options.role ?? null, deleted = false;
  const thread = { id: "compact-thread", cwd: "E:/workspace", ...options.thread };
  let conversation = options.conversation ?? (role ? { ...thread, resumeState: "resumed" } : null);
  const context = {
    Error, setTimeout: callback => callback(),
    appScope: {}, unsubscribe: [() => {}], nativeNavigate() {}, nativeIntl: { formatMessage() {} },
    window: {}, compactionState: { state: "idle", turnId: null, itemId: null },
    nativeReadActions: new Set(["thread", "history"]), requestClientReadActions: new Set(["compact"]),
    validateCompactionQueueSendPayload() {}, resetStaleManager() {}, reportNativeCompatibilityDiagnostic() {},
    reportNativeCallbackFailure() {}, guardThreadAction: async () => null,
    isDeletedThread: () => deleted, readThread: async () => thread,
    runtimeCompaction: () => context.compactionState,
    refreshRequestClientConversation: async (id, settings) => { refreshes.push({ id, settings }); },
    requestClientNativeRpc: async () => rpc,
  };
  const rpc = {
    getStreamRole: () => role,
  };
  const manager = {
    __codexElvesRequestClientManager: options.requestClient !== false, disposed: false,
    getConversation: () => conversation,
    getConversationCwd: () => conversation?.cwd,
    requestClient: { sendRequest() { throw new Error("不能绕过原生所有权直接提交压缩"); } },
    async resumeConversation(params) {
      resumes.push(JSON.parse(JSON.stringify(params)));
      const result = typeof options.resumeResult === "function"
        ? await options.resumeResult(resumes.length) : options.resumeResult ?? { status: "ready" };
      if (result.status === "ready") {
        role = Object.hasOwn(options, "resumedRole") ? options.resumedRole : { role: "owner" };
        conversation = { ...thread, resumeState: "resumed" };
      }
      options.onResume?.(context);
      return result;
    },
    async compactThread(id) {
      if (!["owner", "follower"].includes(role?.role))
        throw new Error("Please continue this conversation on the window where it was started.");
      dispatches.push({ id, path: role.role === "follower" ? "forward" : "local" });
      if (options.compactError) throw options.compactError;
      context.compactionState = { state: "pending", turnId: null, itemId: "pending-manual-context-compaction" };
    },
  };
  if (options.requestClient === false) manager.getStreamRole = rpc.getStreamRole;
  context.manager = manager;
  vm.createContext(context);
  vm.runInContext([
    nativeSourceSection("function failure(", "const state ="),
    nativeSourceSection("async function compactThreadContext(", "function runtimeCompaction("),
    nativeSourceSection("async function call(action,", "window.__codexElvesRemoteAccess ="),
  ].join("\n"), context);
  return {
    context, manager, resumes, dispatches, refreshes,
    remove: () => { deleted = true; },
    compact: () => context.call("compact", { threadId: thread.id }),
  };
}

test("远程压缩在新版 RPC 和旧管理器中先恢复未加载会话，再调用原生压缩", async () => {
  for (const requestClient of [true, false]) {
    const h = nativeCompactionHarness({ requestClient });
    const result = await h.compact();
    assert.equal(result.ok, true);
    assert.equal(result.compaction.state, "pending");
    assert.deepEqual(h.resumes, [{
      conversationId: "compact-thread", model: null, reasoningEffort: null,
      workspaceRoots: ["E:/workspace"], collaborationMode: null,
    }]);
    assert.deepEqual(h.dispatches, [{ id: "compact-thread", path: "local" }]);
  }
});

test("远程压缩不把侧栏摘要当成已恢复会话，并沿用原来的模型配置", async () => {
  const collaborationMode = { mode: "default", settings: { model: "existing-model", reasoning_effort: "max" } };
  const h = nativeCompactionHarness({
    conversation: { id: "compact-thread", cwd: "E:/original", latestCollaborationMode: collaborationMode },
  });
  assert.equal((await h.compact()).ok, true);
  assert.equal(h.resumes.length, 1);
  assert.deepEqual(h.resumes[0], {
    conversationId: "compact-thread", model: null, reasoningEffort: null,
    workspaceRoots: ["E:/original"], collaborationMode,
  });
});

test("远程压缩保留已加载会话的所有权和跨宿主原生转发", async () => {
  for (const requestClient of [true, false]) {
    for (const role of ["owner", "follower"]) {
      const h = nativeCompactionHarness({ requestClient, role: { role, ownerClientId: "existing-owner" } });
      assert.equal((await h.compact()).ok, true);
      assert.equal(h.resumes.length, 0);
      assert.deepEqual(h.dispatches, [{ id: "compact-thread", path: role === "owner" ? "local" : "forward" }]);
    }
  }
});

test("远程压缩重新恢复 needs_resume 会话，但不抢占已有 follower", async () => {
  for (const role of ["owner", "follower"]) {
    const h = nativeCompactionHarness({
      role: { role }, conversation: { id: "compact-thread", cwd: "E:/workspace", resumeState: "needs_resume" },
    });
    assert.equal((await h.compact()).ok, true);
    assert.equal(h.resumes.length, role === "owner" ? 1 : 0);
  }
});

test("远程压缩等待原生执行配置加载，但不会重试真正的压缩提交", async () => {
  const h = nativeCompactionHarness({
    resumeResult: attempt => attempt === 1
      ? { status: "not-ready", reason: "execution-config-loading" } : { status: "ready" },
  });
  assert.equal((await h.compact()).ok, true);
  assert.equal(h.resumes.length, 2);
  assert.equal(h.dispatches.length, 1);
  const rejected = nativeCompactionHarness({
    role: { role: "owner" },
    compactError: new Error("App-server task is not ready: execution-config-loading"),
  });
  assert.equal((await rejected.compact()).ok, false);
  assert.equal(rejected.resumes.length, 0);
  assert.equal(rejected.dispatches.length, 1);
});

test("远程压缩在恢复被拒绝或所有权未就绪时停止提交", async () => {
  for (const options of [
    { resumeResult: { status: "not-ready", reason: "inactive-window" } },
    { resumedRole: null },
    { resumeResult: () => { throw new Error("恢复失败"); } },
    { resumeResult: { status: "not-ready", reason: "execution-config-loading" } },
  ]) {
    const h = nativeCompactionHarness(options), result = await h.compact();
    assert.equal(result.ok, false);
    assert.ok([409, 503].includes(result.code));
    assert.equal(h.dispatches.length, 0);
    assert.doesNotMatch(result.message, /Please continue this conversation/);
  }
});

test("远程压缩在恢复期间更换连接、作用域、删除或释放管理器时停止提交", async () => {
  for (const change of ["manager", "scope", "deleted", "disposed"]) {
    let h;
    h = nativeCompactionHarness({ onResume(context) {
      if (change === "manager") context.manager = { ...h.manager };
      else if (change === "scope") context.appScope = {};
      else if (change === "deleted") h.remove();
      else h.manager.disposed = true;
    } });
    const result = await h.compact();
    assert.equal(result.ok, false, change);
    assert.equal(result.code, change === "deleted" ? 404 : 409, change);
    assert.equal(h.dispatches.length, 0, change);
  }
});

test("远程压缩检查恢复前后的压缩状态，并拒绝在未知工作目录恢复会话", async () => {
  for (const alreadyRunning of [true, false]) {
    const h = nativeCompactionHarness({ onResume(context) {
      context.compactionState = { state: "running" };
    } });
    if (alreadyRunning) h.context.compactionState = { state: "running" };
    const result = await h.compact();
    assert.equal(result.ok, false);
    assert.equal(result.code, 409);
    assert.equal(h.resumes.length, alreadyRunning ? 0 : 1);
    assert.equal(h.dispatches.length, 0);
  }
  const h = nativeCompactionHarness({ thread: { cwd: null } });
  assert.equal((await h.compact()).code, 409);
  assert.equal(h.resumes.length, 0);
  assert.equal(h.dispatches.length, 0);
});

test("Windows 打包拒绝缺少分支信息、运行模式或压缩恢复的旧基线", async () => {
  const verifier = await readFile(new URL("./verify-windows-build-inputs.mjs", import.meta.url), "utf8");
  const start = verifier.indexOf("function verifyRemoteRuntimeSources(");
  const end = verifier.indexOf("const runtimeContract =", start);
  assert.ok(start >= 0 && end > start);
  const verify = new Function(`${verifier.slice(start, end)}; return verifyRemoteRuntimeSources;`)();
  const [backend, service, workspace] = await Promise.all([
    readFile(new URL("../crates/codex-elves-core/src/remote_access.rs", import.meta.url), "utf8"),
    readFile(new URL("../crates/codex-elves-core/src/remote_access/service.rs", import.meta.url), "utf8"),
    readFile(new URL("../crates/codex-elves-core/src/remote_access/workspace.rs", import.meta.url), "utf8"),
  ]);
  const current = { backend, service, workspace, app, native };
  // 普通发送入队边界要求桥接至少为 144；允许后续版本递增，仍核对当前声明并保留下方旧协议和能力缺失的拒绝断言。
  const currentBridge = Number(native.match(/window\.__codexElvesRemoteAccess\?\.version === (\d+)/)?.[1]);
  assert.ok(Number.isSafeInteger(currentBridge) && currentBridge >= 144);
  assert.deepEqual(verify(current), { protocol: 5, bridge: currentBridge });
  for (const obsolete of [2, 3, 4])
    assert.throws(() => verify({ ...current, service: service.replace("PROTOCOL_VERSION: u32 = 5", `PROTOCOL_VERSION: u32 = ${obsolete}`) }), /后台协议不能低于 5/);
  assert.throws(() => verify({ ...current, workspace: workspace.replace("async fn branch_metadata(", "async fn obsolete_metadata(") }), /缺少真实分支/);
  assert.throws(() => verify({ ...current, app: app.replace('api("/api/workspace/context"', 'api("/api/workspace/obsolete"') }), /缺少真实分支/);
  assert.throws(() => verify({ ...current, backend: backend.replace('| "runtime"', "") }), /后台缺少运行模式/);
  assert.throws(() => verify({ ...current, backend: backend.replaceAll('"setRuntime"', '"legacyRuntime"') }), /后台缺少运行模式/);
  assert.throws(() => verify({ ...current, app: app.replace('"/api/read/runtime"', '"/api/read/legacyRuntime"') }), /远程页面缺少运行模式/);
  assert.throws(() => verify({ ...current, native: native.replace(/version === \d+/, "version === 132") }), /原生桥缺少/);
  assert.throws(() => verify({ ...current, native: native.replace("async function compactThreadContext(", "async function obsoleteCompaction(") }), /原生桥缺少/);
});

function newChatBranchHarness() {
  const reads = [], label = { textContent: "" }, attributes = new Map();
  const badge = { hidden: false, title: "", querySelector: () => label, setAttribute: (name, value) => attributes.set(name, value) };
  let now = 1000;
  const context = vm.createContext({
    AbortController, Date: class extends Date { static now() { return now; } },
    creationBranchRead: null, creationBranchSnapshot: null,
    session: { device: { id: "branch-device", createdAtMs: 1 }, runtime: { revision: 1 } },
    creating: true, creationProject: "alpha", selectionToken: "selection-alpha",
    $: selector => selector === "#new-chat-branch" ? badge : null,
    privateInputScope: value => JSON.stringify([value.device.id, value.device.createdAtMs]),
    api: (path, payload, _id, signal) => {
      const response = deferred();
      reads.push({ path, payload, signal, response });
      return response.promise;
    },
  });
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function workspaceBranchLabel("),
    sessionFiles.indexOf("export function createWorkspaceBranch(")).replace(/^export /gm, "")
    + "\n" + sourceSection("function resetNewChatBranch()", "function changeCreationProject()"), context);
  return { context, reads, badge, label, attributes, advance: ms => { now += ms; } };
}

test("新建页切换项目或授权时不显示旧请求的分支，查询只携带项目身份", async () => {
  const h = newChatBranchHarness();
  const first = h.context.refreshNewChatBranch();
  assert.equal(h.label.textContent, "读取分支…");
  h.context.creationProject = "beta"; h.context.selectionToken = "selection-beta";
  const second = h.context.refreshNewChatBranch();
  assert.equal(h.reads[0].signal.aborted, true);
  h.reads[0].response.resolve({ ok: true, git: { branch: "old-alpha", detachedHead: null } });
  await first;
  assert.equal(h.label.textContent, "读取分支…");
  h.reads[1].response.resolve({ ok: true, git: { branch: "feature/界面优化", detachedHead: null } });
  await second;
  assert.equal(h.label.textContent, "feature/界面优化");
  assert.equal(h.badge.title, "当前分支：feature/界面优化");
  assert.equal(h.attributes.get("aria-label"), h.badge.title);
  assert.equal(h.reads.every(read => read.path === "/api/workspace/context"), true);
  assert.deepEqual(JSON.parse(JSON.stringify(h.reads[1].payload)), { projectId: "beta" });

  h.advance(15000);
  const previousAuthorization = h.context.refreshNewChatBranch();
  h.context.session.device.createdAtMs = 2;
  const newAuthorization = h.context.refreshNewChatBranch();
  h.reads[2].response.resolve({ ok: true, git: { branch: "old-authorization", detachedHead: null } });
  await previousAuthorization;
  assert.equal(h.label.textContent, "读取分支…");
  h.reads[3].response.resolve({ ok: true, git: { branch: "main", detachedHead: null } });
  await newAuthorization;
  assert.equal(h.label.textContent, "main");
});

test("分支随现有轮询刷新，未选项目、读取失败和重新打开不沿用错误状态", async () => {
  const h = newChatBranchHarness();
  const first = h.context.refreshNewChatBranch();
  h.context.refreshNewChatBranch();
  assert.equal(h.reads.length, 1, "并发渲染应共享当前读取");
  h.reads[0].response.resolve({ ok: true, git: { branch: "main", detachedHead: null } });
  await first;
  h.context.refreshNewChatBranch();
  assert.equal(h.reads.length, 1, "15 秒内重复渲染不重复启动 Git");
  h.advance(15000);
  const updated = h.context.refreshNewChatBranch();
  h.reads[1].response.resolve({ ok: true, git: { branch: "release/next", detachedHead: null } });
  await updated;
  assert.equal(h.label.textContent, "release/next");
  h.context.selectionToken = "reopened-new-chat";
  const reopened = h.context.refreshNewChatBranch();
  h.reads[2].response.reject(new Error("后台暂不可用"));
  await reopened;
  assert.equal(h.label.textContent, "分支不可用", "查询失败不能继续冒充当前分支");
  h.context.creationProject = "";
  h.context.refreshNewChatBranch();
  assert.equal(h.label.textContent, "");
  assert.equal(h.badge.hidden, true, "不在项目中工作时分支标签不占布局");
  assert.equal(h.reads.length, 3);
});

test("路径仅在展示时去掉长路径前缀，分离 HEAD 与无分支状态清楚区分", () => {
  const context = vm.createContext({});
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function displayWorkspacePath("),
    sessionFiles.indexOf("export function setWorkspaceBranch(")).replace(/^export /gm, ""), context);
  for (const [input, expected] of [
    [String.raw`\\?\E:\workspace\project`, String.raw`E:\workspace\project`],
    ["//?/E:/workspace/project", "E:/workspace/project"],
    [String.raw`\\?\UNC\server\share\project`, String.raw`\\server\share\project`],
    ["/home/workspace/project", "/home/workspace/project"],
    ["E\uf03a\uf05cAppData\uf05cLocal\uf05cnpm-cache", String.raw`E:\AppData\Local\npm-cache`],
    ["C:/项目/中文\uf03a缓存/报告\uf03f.md", "C:/项目/中文:缓存/报告?.md"],
    ["\\\\?\\C:\\项目\\E\uf03a\uf05ccache", String.raw`C:\项目\E:\cache`],
    ["图标\ue000\uf000\uf009\uf020\uf02f\uf8ff", "图标\ue000\uf000\uf009\uf020\uf02f\uf8ff"],
  ]) assert.equal(context.displayWorkspacePath(input), expected);
  assert.equal(context.displayWorkspaceName("\uf022\uf02a\uf03a\uf03c\uf03e\uf03f\uf05c\uf07c"), '"*:<>?\\|');
  assert.equal(context.workspaceBranchLabel({ branch: null, detachedHead: "a".repeat(40) }), "分离 HEAD · aaaaaaaa");
  assert.equal(context.workspaceBranchLabel({ branch: null, detachedHead: null }), "无 Git 分支");
  assert.equal(context.workspaceBranchLabel({ unavailable: true }), "分支不可用");
  assert.equal(context.workspaceBranchLabel(undefined), "", "旧后台没有分支字段时不能伪造分支");
  assert.equal(context.workspaceBranchLabel({ branch: "\nmain" }), "分支不可用");
});

test("WSL 映射文件名只在界面还原，展开目录与打开文件仍保留原始路径和目录层级", () => {
  const node = (tag, className = "", text = "") => ({
    tag, className, textContent: text, children: [], dataset: {}, attributes: new Map(),
    style: { setProperty() {} }, classList: { toggle() {} },
    append(...children) { this.children.push(...children); },
    setAttribute(name, value) { this.attributes.set(name, value); },
  });
  const list = node("div"), opened = [];
  const context = vm.createContext({
    node, list, expanded: new Set(), renderedFile: "",
    control: (_label, action, className) => Object.assign(node("button", className), { click: action }),
    fileTwisty: () => node("span"), fileIcon: () => node("span"), fileIconKind: () => "text",
    createWorkspaceRepository: () => null, bytesLabel: size => `${size} B`,
    toggleDirectory: path => opened.push(["directory", path]),
    openFile: path => opened.push(["file", path]),
    embedded: null,
  });
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function displayWorkspacePath("),
    sessionFiles.indexOf("export function workspaceBranchLabel(")).replace(/^export /gm, "")
    + "\n" + sessionFiles.slice(sessionFiles.indexOf("const parentPath ="), sessionFiles.indexOf("const revisionValid ="))
    + "\n" + sessionFiles.slice(sessionFiles.indexOf("  function renderTreeRow("), sessionFiles.indexOf("  function toggleDirectory("))
    + "\n" + sessionTools.slice(sessionTools.indexOf("  function fileParts("), sessionTools.indexOf("  function setDetailNotice(")), context);

  const rawDirectory = "E\uf03a\uf05cAppData\uf05cLocal\uf05cnpm-cache";
  const readableDirectory = String.raw`E:\AppData\Local\npm-cache`;
  context.renderTreeRow({ name: rawDirectory, path: rawDirectory, kind: "directory" }, 0, false);
  const directoryButton = list.children[0];
  assert.equal(directoryButton.children[2].children[0].textContent, readableDirectory);
  assert.equal(directoryButton.title, readableDirectory);
  assert.equal(directoryButton.dataset.path, rawDirectory);
  assert.equal(directoryButton.attributes.get("aria-level"), "1");
  directoryButton.click();
  assert.deepEqual(opened[0], ["directory", rawDirectory]);

  const rawName = "报告\uf05c第一版\uf03f.md", rawPath = `${rawDirectory}/${rawName}`;
  context.renderTreeRow({ name: rawName, path: rawPath, kind: "file", size: 128 }, 1, true);
  const fileButton = list.children[1];
  assert.equal(fileButton.children[2].children[0].textContent, String.raw`报告\第一版?.md`);
  assert.equal(fileButton.children[2].children[1].textContent, readableDirectory);
  assert.equal(fileButton.dataset.path, rawPath);
  assert.equal(fileButton.attributes.get("aria-level"), "2");
  fileButton.click();
  assert.deepEqual(opened[1], ["file", rawPath]);

  const parts = context.fileParts({ path: rawPath });
  assert.equal(parts.name, String.raw`报告\第一版?.md`);
  assert.equal(parts.directory, readableDirectory, "显示用的反斜杠不能被重新拆成真实目录");
  const changes = workspaceChangeContract();
  const change = { repository: "tools", path: rawPath, section: "unstaged" };
  assert.equal(changes.displayedPath(change), `tools/${readableDirectory}/报告\\第一版?.md`);
  assert.equal(JSON.parse(changes.detailKey(change))[2], rawPath, "变更详情的身份不能使用显示路径");
});

test("文件树右侧显示各仓库的分支和图标，缺失分支时不使用仓库名兜底", () => {
  const node = (tag, className = "", text = "") => {
    const element = {
      tag, className, textContent: text, children: [], attributes: new Map(),
      append(...children) { this.children.push(...children); },
      setAttribute(name, value) { this.attributes.set(name, value); },
      querySelector(selector) {
        const name = selector.slice(1);
        for (const child of this.children) {
          if (child.className.split(" ").includes(name)) return child;
          const match = child.querySelector(selector);
          if (match) return match;
        }
        return null;
      },
    };
    element.classList = { add: name => { element.className += ` ${name}`; } };
    return element;
  };
  const context = vm.createContext({ node });
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function workspaceBranchLabel("),
    sessionFiles.indexOf("export function createWorkspaceLocation(")).replace(/^export /gm, ""), context);
  for (const [name, branch] of [
    ["core", "pre-production"], ["sc-video-engine", "feature/render"],
    ["仓库-前端", "feature/界面优化"], ["<img src=x>", "<img src=branch>"],
  ]) {
    const badge = context.createWorkspaceRepository({ name, git: { branch, detachedHead: null } });
    assert.equal(badge.querySelector(".workspace-branch-label").textContent, branch);
    assert.equal(badge.querySelector(".workspace-branch-icon").attributes.get("aria-hidden"), "true");
    assert.ok(badge.querySelector(".workspace-branch-icon").innerHTML.includes("<svg"));
    assert.equal(badge.title, `当前分支：${branch}`);
    assert.equal(badge.attributes.get("aria-label"), badge.title);
    assert.equal(badge.hidden, false);
    assert.ok(badge.className.includes("session-files-repository"));
  }
  for (const [git, label] of [
    [undefined, "分支不可用"], [{ unavailable: true }, "分支不可用"],
    [{ branch: null, detachedHead: "a".repeat(40) }, "分离 HEAD · aaaaaaaa"],
    [{ branch: "\nmain" }, "分支不可用"],
  ]) {
    const badge = context.createWorkspaceRepository({ name: "不能冒充当前分支的仓库名", git });
    assert.equal(badge.querySelector(".workspace-branch-label").textContent, label);
    assert.equal(badge.title, label);
    assert.equal(badge.attributes.get("aria-label"), label);
    assert.equal(badge.hidden, false);
  }
  for (const input of [undefined, null, {}, { name: "" }, { name: "core\nmain" }])
    assert.equal(context.createWorkspaceRepository(input), null);
});

async function connectionDiagnosticsHarness({ android = true } = {}) {
  const logs = [], documentListeners = new Map(), windowListeners = new Map(), timers = new Map();
  let now = 1000, nextTimer = 0;
  const context = {
    URL, Headers, Response, AbortController, DOMException, TextEncoder, queueMicrotask,
    Date: class extends Date { static now() { return now; } },
    performance: { now: () => now },
    location: new URL("https://diagnostics.example/"),
    navigator: { onLine: true },
    document: {
      hidden: false, cookie: "",
      addEventListener: (type, callback) => documentListeners.set(type, callback),
      removeEventListener: type => documentListeners.delete(type)
    },
    addEventListener: (type, callback) => windowListeners.set(type, callback),
    removeEventListener: type => windowListeners.delete(type),
    setTimeout: (callback, delay) => {
      timers.set(++nextTimer, { callback, delay, interval: false }); return nextTimer;
    },
    setInterval: (callback, delay) => {
      timers.set(++nextTimer, { callback, delay, interval: true }); return nextTimer;
    },
    clearTimeout: id => timers.delete(id), clearInterval: id => timers.delete(id),
    isAndroidContainer: () => android,
    getAndroidAppInfo: async () => ({ diagnosticsVersion: 1, requestRoutingVersion: 2 }),
    CodexElvesAndroid: { postMessage: text => {
      const message = JSON.parse(text);
      if (message.type === "diagnostics-log") logs.push(message.event);
    } }
  };
  context.window = context;
  vm.createContext(context);
  const load = source => vm.runInContext(source.replace(/^import .*;\r?\n/gm, "").replace(/^export /gm, ""), context);
  load(diagnostics);
  await Promise.resolve();
  return { context, logs, timers, documentListeners, windowListeners, load,
    advance: milliseconds => { now += milliseconds; } };
}

test("连接诊断关联标识只附加到状态探测，保留业务请求和既有查询参数", async () => {
  const h = await connectionDiagnosticsHarness(), origin = h.context.location.origin;
  const options = { method: "GET" };
  const trace = h.context.traceDiagnosticRequest("/api/session?existing=1", options, origin);
  const url = new URL(trace.path, origin);
  const requestId = url.searchParams.get("diag");
  assert.match(requestId, /^http-[A-Za-z0-9_-]+$/);
  assert.equal(url.searchParams.get("existing"), "1");
  trace.response(jsonResponse({ ok: true }));
  trace.finish();
  assert.deepEqual(h.logs.map(event => event.fields.requestId), [requestId, requestId, requestId]);
  assert.equal(h.logs[0].fields.path, "/api/session");
  assert.equal(JSON.stringify(h.logs).includes("existing"), false);
  const action = h.context.traceDiagnosticRequest("/api/action/send", { method: "POST" }, origin);
  assert.equal(action.path, "/api/action/send");
  assert.deepEqual(options, { method: "GET" });
  const browser = await connectionDiagnosticsHarness({ android: false });
  assert.equal(browser.context.traceDiagnosticRequest("/api/session", options, origin).path, "/api/session");
});

test("连接诊断保留关闭码与恢复状态，剔除消息凭证、原始关闭文本和越界字段", async () => {
  const h = await connectionDiagnosticsHarness();
  h.context.recordDiagnostic("socket.closed", {
    path: "/api/events/ws?channel_token=secret", requestId: "ws-test", closeCode: 1006, wasClean: false,
    isTrusted: true, delayMs: 500, backgroundMs: 4 * 60 * 60 * 1000, attempt: 2,
    prompt: "private-message", token: "secret", cookie: "secret", closeReason: "private-server-text",
    reason: "private-server-text"
  });
  assert.deepEqual(h.logs[0].fields, {
    path: "/api/events/ws", requestId: "ws-test", closeCode: 1006, wasClean: false,
    isTrusted: true, delayMs: 500, backgroundMs: 14400000, attempt: 2
  });
  h.context.recordDiagnostic("socket.closed", {
    closeCode: 65536, wasClean: "false", isTrusted: 1, delayMs: 86400001, backgroundMs: -1, attempt: 1.5
  });
  assert.deepEqual(h.logs[1].fields, {});
  h.context.setDiagnosticSink(() => { throw new Error("diagnostic failure"); });
  assert.doesNotThrow(() => h.context.recordDiagnostic("socket.error", { reason: "network-error" }));
});

test("主题日志保留小数光照和固定判断字段，未知文本及非法年龄不会上传", async () => {
  const h = await connectionDiagnosticsHarness();
  h.context.recordDiagnostic("theme.transition", {
    lux: 44.99, previousLux: 10.01, theme: "dark", previousTheme: "light", targetTheme: "dark",
    themeMode: "auto", decision: "applied", reason: "sensor", phase: "after",
    sampleAgeMs: 2100, stableMs: 5000, themeSequence: 1, localHour: 21, timezoneOffsetMinutes: -480,
    supported: true, registered: false, token: "private", message: "private"
  });
  assert.deepEqual(h.logs[0].fields, {
    lux: 44.99, previousLux: 10.01, theme: "dark", previousTheme: "light", targetTheme: "dark",
    themeMode: "auto", decision: "applied", reason: "sensor", phase: "after",
    sampleAgeMs: 2100, stableMs: 5000, themeSequence: 1, localHour: 21, timezoneOffsetMinutes: -480,
    supported: true, registered: false
  });
  h.context.recordDiagnostic("theme.sample", {
    lux: Infinity, previousLux: -1, sampleAgeMs: -1, themeSequence: NaN, localHour: 24,
    timezoneOffsetMinutes: -841, theme: "private", themeMode: "private", phase: "private", reason: "private"
  });
  assert.deepEqual(h.logs[1].fields, {});
});

test("光照桥兼容旧APK，非法诊断年龄不改变原有光照回调", () => {
  const values = [], messages = [], listeners = new Map();
  const context = vm.createContext({
    values, CodexElvesAndroid: { postMessage: value => messages.push(JSON.parse(value)) },
    addEventListener: (name, callback) => listeners.set(name, callback),
    removeEventListener: name => listeners.delete(name)
  });
  vm.runInContext(platform.replace(/^export /gm, ""), context);
  vm.runInContext(`
    globalThis.unsubscribe = subscribeAndroidAmbientLight(status => values.push(status));
    globalThis.emit = json => onAmbientLight({ detail: JSON.parse(json) });
  `, context);
  for (const detail of [
    { type: "ambient-light", supported: true, lux: 9.99 },
    { type: "ambient-light", supported: true, lux: 45.125, sampleAgeMs: 90000 },
    { type: "ambient-light", supported: true, lux: 20.5, sampleAgeMs: -1 },
    { type: "ambient-light", supported: false }
  ]) context.emit(JSON.stringify(detail));
  assert.deepEqual(values.map(value => ({ ...value })), [
    { supported: true, lux: 9.99, sampleAgeMs: null },
    { supported: true, lux: 45.125, sampleAgeMs: 90000 },
    { supported: true, lux: 20.5, sampleAgeMs: null },
    { supported: false, lux: null, sampleAgeMs: null }
  ]);
  assert.ok(values.every(Object.isFrozen));
  context.emit(JSON.stringify({ type: "ambient-light", supported: true, lux: -1 }));
  assert.equal(values.length, 4);
  context.unsubscribe();
  assert.equal(listeners.size, 0);
  assert.deepEqual(messages.map(value => value.type), ["ambient-light-subscribe", "ambient-light-unsubscribe"]);
});

async function themeDiagnosticsHarness({ android = true } = {}) {
  const h = await connectionDiagnosticsHarness({ android });
  let hour = 21, ambient;
  const Clock = h.context.Date;
  h.context.Date = class extends Clock {
    getHours() { return hour; }
    getTimezoneOffset() { return -480; }
  };
  Object.assign(h.context, {
    appearance: { theme: "light", themeMode: "auto" },
    ambientLightStatus: { supported: false, lux: null },
    syncAppearanceThemeControls: null, applyAppearance() {},
    subscribeAndroidAmbientLight: callback => { ambient = callback; }
  });
  h.load(app.slice(app.indexOf("const appearancePresets ="), app.indexOf("let ambientLightStatus =")));
  h.load(app.slice(app.indexOf("const automaticThemeStableMs ="), app.indexOf("function mixColor(")));
  const startup = app.indexOf('applyAutomaticAppearance(false, { immediate: true, reason: "startup" })');
  const timers = app.indexOf("setInterval(() => {", startup);
  h.load(app.slice(timers, app.indexOf("function currentFileBrowserThreadId(", timers)));
  return {
    ...h, appearance: h.context.appearance, hour: value => { hour = value; },
    sensor: (lux, sampleAgeMs = 0) => ambient({ supported: lux !== null, lux, sampleAgeMs }),
    runAfter: delay => {
      const timer = [...h.timers.entries()].find(([, value]) => !value.interval && value.delay === delay);
      assert.ok(timer, `缺少 ${delay}ms 诊断回调`);
      h.timers.delete(timer[0]); timer[1].callback();
    }
  };
}

test("主题切换保留前后真实光照及年龄，延迟采证不会提前应用下一次切换", async () => {
  const h = await themeDiagnosticsHarness();
  h.sensor(60.25);
  h.advance(2000); h.sensor(8.5);
  assert.equal(h.appearance.theme, "light");
  assert.equal([...h.timers.values()].filter(timer => !timer.interval).length, 0);
  h.advance(4999); h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "light");
  h.advance(1); h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "dark");
  const before = h.logs.filter(event => event.name === "theme.sample" && event.fields.phase === "before");
  assert.deepEqual(before.map(event => event.fields.lux), [60.25, 8.5]);
  const after = h.logs.find(event => event.name === "theme.transition" && event.fields.phase === "after").fields;
  assert.deepEqual({ ...after }, {
    theme: "dark", themeMode: "auto", themeSequence: 1, supported: true, lux: 8.5, previousLux: 60.25,
    sampleAgeMs: 5000, localHour: 21, timezoneOffsetMinutes: -480, visible: true,
    previousTheme: "light", targetTheme: "dark", reason: "timer", phase: "after"
  });
  h.advance(2000); h.sensor(12.25); h.runAfter(2000);
  h.advance(3000); h.sensor(60.125); h.runAfter(5000);
  h.advance(5000); h.runAfter(10000);
  const samples = h.logs.filter(event => event.name === "theme.sample" && event.fields.phase === "after");
  assert.deepEqual(samples.map(event => event.fields.lux), [12.25, 60.125, 60.125]);
  assert.deepEqual(samples.map(event => event.fields.durationMs), [2000, 5000, 10000]);
  assert.deepEqual(samples.map(event => event.fields.sampleAgeMs), [0, 0, 5000]);
  assert.equal(h.appearance.theme, "dark", "诊断回调不能推进已满 5 秒的切换候选");
  h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "light", "原有业务触发仍可应用稳定候选");
  h.windowListeners.get("pagehide")();
  assert.equal([...h.timers.values()].filter(timer => !timer.interval).length, 0);
});

test("自动主题使用20:00至次日08:00夜间范围，默认9/10lux滞回并保留候选取消规则", async () => {
  const h = await themeDiagnosticsHarness();
  for (const hour of [7, 8, 19, 20, 21]) {
    h.hour(hour);
    for (const lux of [null, 0, 9, 9.01, 9.99, 10, 35, 100]) {
      h.context.ambientLightStatus = { supported: lux !== null, lux };
      for (const current of ["light", "dark"]) {
        const expected = hour >= 8 && hour < 20 ? "light"
          : lux === null || lux <= 9 ? "dark" : lux >= 10 ? "light" : current;
        assert.equal(h.context.automaticAppearanceTheme(undefined, current), expected);
      }
    }
  }
  h.hour(20); h.sensor(5); h.advance(3000); h.sensor(50);
  assert.equal(h.appearance.theme, "light");
  assert.equal(h.logs.find(event => event.fields.decision === "cancelled").fields.durationMs, 3000);
  const browser = await themeDiagnosticsHarness({ android: false });
  browser.sensor(0); browser.advance(5000); browser.context.applyAutomaticAppearance(true);
  assert.equal(browser.appearance.theme, "dark");
  assert.equal(browser.logs.length, 0);
  assert.equal([...browser.timers.values()].filter(timer => !timer.interval).length, 0);
});

test("自定义光照阈值按所选数值切回浅色，修改后重新累计五秒稳定时间", async () => {
  const h = await themeDiagnosticsHarness();
  for (const threshold of [5, 10, 25, 35, 40]) {
    h.appearance.automaticThemeLightLux = threshold;
    const darkLux = Math.min(10, threshold - 1);
    for (const [lux, current, expected] of [
      [darkLux, "light", "dark"], [darkLux + .01, "light", "light"],
      [threshold - .01, "dark", "dark"], [threshold - .01, "light", "light"],
      [threshold, "dark", "light"], [threshold + .01, "dark", "light"],
    ]) {
      h.context.ambientLightStatus = { supported: true, lux };
      assert.equal(h.context.automaticAppearanceTheme(undefined, current), expected, `${threshold} lux/${lux} lux`);
    }
  }
  h.appearance.theme = "dark"; h.appearance.automaticThemeLightLux = 35;
  h.sensor(37); h.advance(4000);
  assert.equal(h.context.setAutomaticThemeLightLux(40), true);
  h.advance(5000); h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "dark", "升高阈值后原有切亮候选必须失效");
  assert.equal(h.context.setAutomaticThemeLightLux(35), true);
  h.advance(4999); h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "dark");
  h.advance(1); h.context.applyAutomaticAppearance(true);
  assert.equal(h.appearance.theme, "light");
  for (const invalid of [4, 41, 35.5, NaN, "40", null]) {
    assert.equal(h.context.setAutomaticThemeLightLux(invalid), false);
    assert.equal(h.appearance.automaticThemeLightLux, 35);
  }
});

test("光照滑块仅在自动主题下显示，5至40可保存并在切换模式后保留", async () => {
  const h = await themeDiagnosticsHarness(), disk = storage();
  const element = (tag, className = "", textContent = "") => ({
    tag, className, textContent, children: [], attributes: new Map(), properties: new Map(),
    append(...children) { this.children.push(...children); },
    setAttribute(name, value) { this.attributes.set(name, value); },
    style: { values: new Map(), setProperty(name, value) { this.values.set(name, value); } },
  });
  Object.assign(h.context, {
    element, localStorage: disk, matchMedia: () => ({ matches: false }),
    applyAppearance: save => { if (save) disk.setItem("codex-elves-web-appearance", JSON.stringify(h.appearance)); },
  });
  h.appearance.themeMode = "manual";
  const host = element("div");
  const sync = h.context.mountAutomaticThemeLightSetting(host);
  h.context.syncAppearanceThemeControls = sync;
  const input = host.children[0].children[1].children[0];
  const output = host.children[0].children[1].children[1];
  const hint = host.children[1];
  assert.equal(host.hidden, true);
  assert.equal(input.disabled, true);
  assert.equal(input.min, "5"); assert.equal(input.max, "40"); assert.equal(input.step, "1");
  assert.equal(output.attributes.get("for"), input.id);
  h.context.setAppearanceThemeMode("auto");
  assert.equal(host.hidden, false); assert.equal(input.disabled, false);
  assert.equal(input.value, "10"); assert.equal(output.textContent, "10 lux");
  assert.equal(input.style.values.get("--range-fill"), `${5 / 35 * 100}%`);
  assert.equal(hint.textContent, "光线达到此数值后切换为浅色，当前 -- lux");
  for (const [lux, displayed] of [[26.544523, "26.54"], [79.308532, "79.31"], [0, "0"], [null, "--"]]) {
    h.sensor(lux);
    assert.equal(hint.textContent, `光线达到此数值后切换为浅色，当前 ${displayed} lux`);
    assert.equal(input.value, "10", "实时光照不能覆盖设置的切换阈值");
  }
  for (const value of [5, 40]) {
    input.value = String(value); input.oninput();
    assert.equal(output.textContent, `${value} lux`);
    assert.equal(h.context.readAppearance().automaticThemeLightLux, value);
    assert.equal(input.style.values.get("--range-fill"), value === 5 ? "0%" : "100%");
  }
  h.context.setAppearanceThemeMode("manual");
  assert.equal(host.hidden, true); assert.equal(input.disabled, true);
  assert.equal(h.context.readAppearance().automaticThemeLightLux, 40);
  h.context.setAppearanceThemeMode("auto");
  assert.equal(input.value, "40");
  Object.assign(h.appearance, h.context.defaultAppearance());
  sync();
  assert.equal(input.value, "10");
});

test("主题诊断限制前序采样数量，缓存及旧APK缺少的采样时间不会伪装成新读数", async () => {
  const h = await themeDiagnosticsHarness();
  for (let index = 0; index < 12; index++) { h.sensor(60 + index, 90_000); h.advance(1000); }
  h.context.setAppearanceThemeMode("manual");
  const samples = h.logs.filter(event => event.name === "theme.sample" && event.fields.phase === "before");
  assert.equal(samples.length, 8);
  assert.equal(samples[0].fields.sampleAgeMs, 98_000);
  const transitions = h.logs.filter(event => event.name === "theme.transition");
  assert.deepEqual(transitions.map(event => event.fields.themeMode), ["auto", "manual"]);
  h.advance(31_000); h.context.setAppearanceThemeMode("auto");
  const latest = h.logs.filter(event => event.name === "theme.sample" && event.fields.phase === "before").at(-1);
  assert.equal(latest.fields.sampleAgeMs, 122_000);
  assert.equal(h.logs.filter(event => event.name === "theme.sample" && event.fields.phase === "before").length, 9);
  h.sensor(80, null);
  assert.equal(h.logs.filter(event => event.name === "theme.ambient").at(-1).fields.sampleAgeMs, undefined);
});

test("前后台日志保留后台时长及网络事件来源，online值仍反映当前浏览器状态", async () => {
  const h = await connectionDiagnosticsHarness();
  h.context.configureAndroidDiagnostics({ getSession: () => null, upload: async () => {} });
  h.context.document.hidden = true;
  h.documentListeners.get("visibilitychange")({ isTrusted: true });
  h.advance(14400000);
  h.context.document.hidden = false;
  h.documentListeners.get("visibilitychange")({ isTrusted: true });
  h.windowListeners.get("offline")({ isTrusted: true });
  const events = h.logs.filter(event => event.name === "page.lifecycle");
  assert.equal(events[1].fields.backgroundMs, 14400000);
  assert.equal(events[1].fields.isTrusted, true);
  assert.deepEqual(events[2].fields, { reason: "offline", visible: true, online: true, isTrusted: true });
});

test("WebSocket异常关闭及重试记录同一握手标识，诊断不修改原有500毫秒退避", async () => {
  const h = await connectionDiagnosticsHarness(), sockets = [];
  class Socket {
    constructor(url) { this.url = new URL(url); this.listeners = new Map(); sockets.push(this); }
    addEventListener(type, callback) { this.listeners.set(type, callback); }
    close() { this.closed = true; }
  }
  h.context.WebSocket = Socket;
  h.load(channels);
  const stream = h.context.createChannelEventStream();
  let errors = 0;
  stream.onerror = () => { errors++; };
  await Promise.resolve();
  const socket = sockets[0], requestId = socket.url.searchParams.get("diag");
  assert.match(requestId, /^ws-[A-Za-z0-9_-]+$/);
  socket.onopen();
  assert.equal(stream.readyState, 1);
  const logsBeforeMessage = h.logs.length;
  socket.onmessage({ data: JSON.stringify({ method: "remote/heartbeat" }) });
  assert.equal(h.logs.length, logsBeforeMessage, "每条消息及心跳不刷诊断日志");
  socket.onerror();
  socket.listeners.get("close")({ code: 1006, wasClean: false, reason: "private text" });
  assert.equal(errors, 1);
  assert.equal(stream.readyState, 0);
  const close = h.logs.find(event => event.name === "socket.closed");
  assert.equal(close.fields.requestId, requestId);
  assert.equal(close.fields.closeCode, 1006);
  assert.equal(close.fields.wasClean, false);
  assert.equal(JSON.stringify(h.logs).includes("private text"), false);
  assert.equal(h.logs.find(event => event.name === "socket.retry").fields.delayMs, 500);
  const retry = [...h.timers.values()].find(timer => !timer.interval && timer.delay === 500);
  assert.ok(retry);
  h.advance(500); retry.callback();
  assert.equal(sockets.length, 2);
  assert.notEqual(sockets[1].url.searchParams.get("diag"), requestId);
  stream.close();
});

test("SharedWorker转交日志不算作心跳，也不推迟原有失联降级", async () => {
  const h = await connectionDiagnosticsHarness();
  let worker, fallbacks = 0;
  h.context.SharedWorker = class {
    constructor() {
      worker = this;
      this.port = { postMessage() {}, start() {}, close() {} };
    }
  };
  h.context.observeChannelConfiguration = () => () => {};
  h.context.createChannelEventStream = () => {
    fallbacks++;
    return { readyState: 0, close() {} };
  };
  h.load(eventsClient);
  const stream = h.context.createRemoteEventStream();
  worker.port.onmessage({ data: { type: "ready" } });
  for (let tick = 1; tick <= 18; tick++) {
    h.advance(5000);
    worker.port.onmessage({ data: {
      type: "diagnostic", event: { name: "socket.retry", fields: { requestId: "ws-test", delayMs: 500 } }
    } });
    const watcher = [...h.timers.entries()].find(([, timer]) => timer.delay === 5000);
    assert.ok(watcher);
    h.timers.delete(watcher[0]); watcher[1].callback();
  }
  assert.equal(fallbacks, 1);
  assert.equal(stream.transport, "fallback");
  assert.ok(h.logs.some(event => event.name === "worker.timeout" && event.fields.reason === "worker-response-timeout"));
  stream.close();
});

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
    confirmedSendDisplayFor: () => null, reconcileSentMessageDisplays: () => false,
    queueDisplayMessages: value => value, queueMessageText: value => value.text,
    applyQueueSnapshot: () => true, queueSnapshotIsVersioned: () => true,
    notice: value => notices.push(value),
  };
  vm.createContext(context);
  for (const [start, end] of [
    ["function queueReorderAvailable()", "function moveQueueItem("],
    ["function applyQueuedSendResult(", "function queueMessageText("],
    ["function renderQueuedMessages(", "function renderQueueArea("],
    ["function isActiveThread()", "function renderCompactionBanner("],
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

test("Claude 仅按原生能力提供一次整队立即执行，不开放逐条编辑、删除或排序", () => {
  const messages = ["a", "b"].map(id => ({ id, text: `待执行-${id}` }));
  const { context } = queueDeliveryHarness(messages), calls = [];
  context.thread.backend = "claude";
  context.thread.status = { type: "active" };
  context.thread.capabilities = { queueSendNow: true };
  context.steerQueuedMessage = (threadId, messageId) => calls.push({ threadId, messageId });
  context.openPopup = () => assert.fail("Claude 不应通过右键打开 Codex 队列菜单");
  const queue = context.renderQueuedMessages(messages);
  const actions = queue.children.flatMap(row =>
    row.children.filter(node => node.className === "queued-steer-button"));
  assert.equal(actions.length, 1);
  assert.equal(actions[0].text, "立即执行全部");
  assert.equal(actions[0].disabled, false);
  assert.equal(context.queueReorderAvailable(), false);
  for (const row of queue.children) {
    assert.equal(row.children.some(node => node.className === "queued-menu-button"), false);
    row.oncontextmenu({ preventDefault() {}, target: { closest: () => null } });
  }
  actions[0].action();
  assert.deepEqual(calls, [{ threadId: "main", messageId: "a" }]);
  context.thread.capabilities.queueSendNow = false;
  assert.equal(context.renderQueuedMessages(messages).children.some(row =>
    row.children.some(node => node.className === "queued-steer-button")), false);
});

test("Claude 远程待发送队列逐条引导并开放编辑删除排序，原生已接收记录保持只读", () => {
  const messages = ["B", "C", "D"].map((id,index) => ({
    id, clientUserMessageId:id, createdAt:index+1, text:`待发送-${id}`,
    remoteQueueSource:"remote", submission:{status:"queued"},
  }));
  const {context}=queueDeliveryHarness(messages), calls=[];
  context.thread.backend="claude";
  context.thread.status={type:"active"};
  context.thread.capabilities={remoteQueue:true,queueSendNow:false};
  context.steerQueuedMessage=(threadId,messageId)=>calls.push({threadId,messageId});
  const queue=context.renderQueuedMessages(messages);
  assert.equal(queue.attributes["aria-label"],"远程待发送队列，3 条待执行");
  assert.equal(context.queueReorderAvailable(),true);
  for (const row of queue.children) {
    const action=row.children.find(n=>n.className==="queued-steer-button");
    assert.equal(action.children.at(-1).text,"引导");
    assert.equal(action.disabled,false);
    const menu=row.children.find(n=>n.className==="queued-menu-button");
    assert.equal(menu.menuOptions[0].disabled,false);
    assert.equal(menu.menuOptions[1].label,"删除排队消息");
    assert.notEqual(row.children[0].hidden,true);
  }
  queue.children[1].children.find(n=>n.className==="queued-steer-button").action();
  assert.deepEqual(calls,[{threadId:"main",messageId:"C"}]);
  const accepted={...messages[0],remoteQueueSource:"native",remoteDeliveryState:"accepted",
    submission:{status:"accepted"},canSendNow:false};
  context.thread.queuedMessages=[accepted,...messages.slice(1)];
  const readonly=context.renderQueuedMessages(context.thread.queuedMessages).children[0];
  assert.equal(readonly.children.find(n=>n.className==="queued-steer-button").disabled,true);
  assert.equal(readonly.children.some(n=>n.className==="queued-menu-button"),false);
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

test("会话恢复执行后，暂停的队列消息显示引导并操作原消息", () => {
  for (const status of ["active", "inProgress", "running", "pending", { type: "active" }, { status: { type: "running" } }]) {
    const message = { id: "paused-queue", text: "保留的排队内容", pausedReason: "消息已暂停",
      submission: { status: "queued" }, canSendNow: true };
    const { context } = queueDeliveryHarness([message]), calls = [];
    context.thread.status = status;
    context.steerQueuedMessage = (threadId, messageId) => calls.push({ threadId, messageId });
    const row = context.renderQueuedMessages([message]).children[0];
    const action = row.children.find(node => node.className === "queued-steer-button");
    assert.equal(action.children[1].text, "引导");
    assert.equal(action.children[0].className, "steer");
    assert.equal(action.attributes["aria-label"], "引导此队列消息");
    assert.equal(action.disabled, false);
    assert.equal(message.pausedReason, "消息已暂停", "显示更新不能伪造队列状态或自动重发");
    assert.equal(calls.length, 0);
    action.action();
    assert.deepEqual(calls, [{ threadId: "main", messageId: message.id }]);
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
    sessionTypeEnabled: true,
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

function compactionComposerHarness() {
  const h = queuedEditFeedbackHarness(), { context } = h;
  Object.assign(context, {
    editingQueuedMessage: null, editing: false, prompt: "", active: false,
    compacting: true, compaction: { state: "running", nativeConfirmed: true },
    compactionSendReason: "当前服务不支持压缩期间排队发送", queueOnly: false,
    currentCompaction: () => context.compaction, isActiveThread: () => context.active,
    updateComposer: () => context.renderSendFeedback(),
  });
  vm.runInContext(sourceSection("async function stopGeneration(", "async function applyCompactionReceipt("), context);
  context.updateComposer();
  return h;
}

test("已确认压缩先于线程执行状态到达时，空输入显示可用停止按钮，有内容仍可排队发送", () => {
  const h = compactionComposerHarness();
  assert.equal(h.send.dataset.action, "stop");
  assert.equal(h.send.disabled, false, "排队发送能力缺失不能禁用停止压缩");
  assert.ok(h.send.querySelector(".stop"));
  assert.equal(h.send.attributes["aria-label"], "停止压缩");
  assert.equal(h.send.title, "停止压缩");
  h.context.compactionSendReason = ""; h.context.queueOnly = true;
  for (const [prompt, hasAttachments] of [["补充消息", false], ["", true]]) {
    Object.assign(h.context, { prompt, hasAttachments });
    h.context.updateComposer();
    assert.equal(h.send.dataset.action, "send");
    assert.equal(h.send.disabled, false);
    assert.ok(h.send.querySelector(".arrowUp"));
    assert.equal(h.send.attributes["aria-label"], "发送到原生消息队列");
  }
  h.context.prompt = ""; h.context.hasAttachments = false; h.context.compacting = false;
  h.context.compaction = { state: "idle" }; h.context.queueOnly = false;
  h.context.updateComposer();
  assert.equal(h.send.dataset.action, "send");
  assert.equal(h.send.disabled, true, "压缩结束后恢复空输入的发送禁用状态");
});

test("未确认或未知压缩保留发送锁定，已确认压缩仍受只读和离线限制", async () => {
  const h = compactionComposerHarness(), writes = [];
  h.context.mutate = async (...input) => { writes.push(input); };
  for (const state of ["pending", "unknown"]) {
    h.context.compaction = { state };
    h.context.compactionSendReason = "等待原压缩回执";
    h.context.updateComposer();
    assert.equal(h.send.dataset.action, "send");
    assert.equal(h.send.disabled, true);
    await h.context.stopGeneration();
  }
  assert.equal(writes.length, 0);
  h.context.compaction = { state: "running" };
  for (const lockedBy of ["read-only", "offline"]) {
    h.context.threadReadOnly = () => lockedBy === "read-only";
    h.context.connected = lockedBy !== "offline";
    h.context.updateComposer();
    assert.equal(h.send.dataset.action, "stop");
    assert.equal(h.send.disabled, true);
    await h.context.stopGeneration();
  }
  assert.equal(writes.length, 0);
});

test("停止压缩实际提交当前会话停止请求，在途重复点击不重复提交，回读后恢复发送状态", async () => {
  for (const compaction of [{ state: "running" }, { state: "pending", acknowledged: true },
    { state: "pending", nativeConfirmed: true }]) {
    const h = compactionComposerHarness(), response = deferred(), writes = [];
    h.context.compaction = compaction;
    h.context.mutate = async (action, payload) => { writes.push({ action, ...payload }); return response.promise; };
    let reloaded = 0;
    h.context.reloadThread = async () => {
      reloaded++; h.context.compaction = { state: "idle" }; h.context.compacting = false;
      h.context.compactionSendReason = "";
    };
    const operation = h.context.stopGeneration();
    assert.equal(h.context.stopping, true);
    assert.equal(h.send.disabled, true);
    await h.context.stopGeneration();
    assert.deepEqual(writes, [{ action: "stop", threadId: h.context.selectedId }]);
    response.resolve({ ok: true });
    await operation;
    assert.equal(reloaded, 1);
    assert.equal(h.context.stopping, false);
    assert.equal(h.send.dataset.action, "send");
    assert.equal(h.send.disabled, true);
  }
});

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

test("取消和保存未知回执停止在途spinner但保持锁定，八秒前无额外提示", async () => {
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
    assert.equal(h.context.pendingFeedbackReady(record), false);
    record.time -= 3000;
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

function compactionRenderer() {
  function element(tag, className = "", text = "") {
    return { tagName: tag, className, ownText: text, children: [], attributes: {},
      classList: { add() {} }, append(...nodes) { this.children.push(...nodes); },
      replaceChildren(...nodes) { this.children = nodes; },
      setAttribute(name, value) { this.attributes[name] = value; },
      get textContent() { return this.ownText + this.children.map(node => node.textContent || "").join(""); },
    };
  }
  const context = { element, icon: () => element("svg"), button: () => element("button"),
    compactionQueueSendReason: () => "", models: [],
    effortLabels: { high: "高", xhigh: "极高", max: "Max" } };
  vm.createContext(context);
  vm.runInContext(sourceSection("function transcriptModelName(", "function readableToolName("), context);
  return context.renderTranscriptEvent;
}

test("Claude 模型和等级变化沿用 Codex 变更行，纯等级调整不显示假模型切换", () => {
  const render = compactionRenderer();
  const effort = render({ id: "effort", type: "modelChanged", source: "claude",
    fromModel: "claude-opus-5-5", toModel: "claude-opus-5-5", fromEffort: "max", toEffort: "xhigh" });
  assert.match(effort.className, /transcript-event.*model-change/);
  assert.match(effort.textContent, /思考深度已从 Max 更改为 极高/);
  assert.doesNotMatch(effort.textContent, /模型已从|command-name|local-command-stdout/);
  assert.match(effort.attributes["aria-label"], /原生实际请求确认/);
  const partial = render({ id: "partial", type: "modelChanged", source: "claude",
    fromModel: null, toModel: "model-b", fromEffort: "high", toEffort: "max" });
  assert.match(partial.textContent, /思考深度已从 高 更改为 Max/);
  assert.doesNotMatch(partial.textContent, /模型已从|未知模型/, "缺失旧型号不能猜造型号切换");
  const both = render({ id: "both", type: "modelChanged", source: "claude",
    fromModel: "model-a", toModel: "model-b", fromEffort: "high", toEffort: "max" });
  assert.match(both.textContent, /模型已从 model-a 更改为 model-b，思考深度从 高 更改为 Max/);
  const codex = render({ id: "codex", type: "modelChanged", fromModel: "model-a", toModel: "model-b" });
  assert.equal(codex.textContent, "模型已从 model-a 更改为 model-b");
  assert.match(codex.attributes["aria-label"], /模型切换可能触发上下文压缩/);
});

test("Claude 首次生效提示位于对应问题前，中途变更保持原生请求位置且重绘不重复", () => {
  const context = { transcriptEvents: new Map(), itemVersions: new Map() };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("const transcriptEventTypes =", "async function mutate("),
    sourceSection("function completedItem(", "function mergeHistoryTurn("),
  ].join("\n"), context);
  const input = { id: "input", type: "userMessage" }, first = { id: "first", type: "agentMessage" },
    second = { id: "second", type: "agentMessage" };
  const initial = { id: "initial-change", type: "modelChanged", source: "claude", beforeItemId: "input" };
  const inline = { id: "inline-change", type: "modelChanged", source: "claude",
    remoteInlineEvent: true, beforeItemId: "second" };
  let turn = { id: "turn", status: "completed", items: [input, initial, first, inline, second] };
  for (let index = 0; index < 3; index++) turn = context.enrichTurnTranscriptEvents(turn);
  assert.deepEqual([...turn.items.map(item => item.id)], ["initial-change", "input", "first", "inline-change", "second"]);
  assert.equal(turn.items.filter(item => item.type === "modelChanged").length, 2);
});

test("压缩历史仅有时间字段时，切回会话即可显示进行中或已完成", () => {
  const render = compactionRenderer();
  // 真实服务历史没有 completed 字段，不能依赖下一次实时事件才补全显示状态。
  const item = { type: "contextCompaction", id: "compact", startedAtMs: 1794147063807 };
  assert.match(render(item).textContent, /正在压缩上下文/);
  assert.match(render({ ...item, completedAtMs: 1794147151527 }).textContent, /上下文已压缩/);
  assert.doesNotMatch(render({ ...item, completedAtMs: 1794147151527 }).textContent, /正在压缩|状态已更新/);
});

test("压缩失败、中断和未知历史不能显示为成功完成", () => {
  const render = compactionRenderer(), item = { type: "contextCompaction", id: "compact" };
  assert.match(render({ ...item, status: "failed", completedAtMs: 100 }).textContent, /失败/);
  for (const status of ["interrupted", "cancelled", "canceled"]) {
    assert.match(render({ ...item, status, completedAtMs: 100 }).textContent, /中断/);
  }
  assert.match(render(item).textContent, /状态已更新/);
});

test("切回会话的迟到快照不能抹掉压缩完成事件或把它恢复为进行中", () => {
  const context = { transcriptEvents: new Map(), turns: new Map(), itemVersions: new Map() };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("const transcriptEventTypes =", "async function mutate("),
    sourceSection("function completedItem(", "function mergeHistoryTurn("),
  ].join("\n"), context);
  const completed = { turnId: "compact-turn", item: {
    type: "contextCompaction", id: "compact", completed: true, completedAtMs: 2000,
  } };
  context.mergeTranscriptEvents([completed]);
  context.itemVersions.set("compact-turn:compact", 2);
  context.replaceTranscriptEvents([], 1);
  assert.equal(context.transcriptEvents.size, 1);
  context.mergeTranscriptEvents([{ ...completed, item: { ...completed.item, completed: false, completedAtMs: undefined } }]);
  const restored = context.enrichTurnTranscriptEvents({ id: "compact-turn", items: [] });
  assert.equal(restored.items.length, 1);
  assert.equal(restored.items[0].completed, true);
  assert.match(compactionRenderer()(restored.items[0]).textContent, /上下文已压缩/);
  // 新选择用另一会话的快照时仍必须隔离，不能把上一会话的事件带过去。
  context.replaceTranscriptEvents([]);
  assert.equal(context.transcriptEvents.size, 0);
  const interrupted = context.enrichTurnTranscriptEvents({ id: "stopped", status: "interrupted",
    items: [{ type: "contextCompaction", id: "unfinished", startedAtMs: 1000 }] });
  assert.match(compactionRenderer()(interrupted.items[0]).textContent, /已中断/);
  const finishedBeforeInterruption = context.enrichTurnTranscriptEvents({ id: "stopped", status: "interrupted",
    items: [{ type: "contextCompaction", id: "finished", startedAtMs: 1000, completedAtMs: 2000 }] });
  assert.match(compactionRenderer()(finishedBeforeInterruption.items[0]).textContent, /上下文已压缩/);
});

test("CDP 历史压缩投影保留完成时间与失败状态", () => {
  const context = { transcriptEventTypes: new Set(["contextCompaction"]) };
  vm.createContext(context);
  vm.runInContext(nativeSourceSection("function nativeTranscriptItem(", "function nativeTranscriptEvents("), context);
  const item = { type: "contextCompaction", id: "compact", startedAtMs: 1000, completedAtMs: 2000, status: "failed" };
  const projected = context.nativeTranscriptItem(item);
  assert.equal(projected.completedAtMs, 2000);
  assert.match(compactionRenderer()(projected).textContent, /失败/);
});

test("服务历史先补回继续消息，再翻页时保留顺序且不重复首条输入", () => {
  const context = { turns: new Map(), transcriptEvents: new Map(), itemVersions: new Map(),
    turnVersions: new Map(), turnFieldVersions: new Map(), partialItems: new Set() };
  vm.createContext(context);
  vm.runInContext([
    sourceSection("const transcriptEventTypes =", "async function mutate("),
    sourceSection("function mergeLiveText(", "async function reloadThread("),
  ].join("\n"), context);
  const items = Array.from({ length: 114 }, (_, index) => ({
    id: `item-${String(index).padStart(3, "0")}`,
    ...(index === 0 ? { type: "userMessage", content: [{ type: "text", text: "继续" }] }
      : { type: "commandExecution", command: "echo check", status: "completed" }),
  }));
  let turn = context.mergeHistoryTurn({ id: "turn", status: "completed",
    items: [{ ...items[0], remoteHistoryContext: true }, ...items.slice(74)],
    itemsPageComplete: true, itemsComplete: false, itemsNextCursor: "page-2" }, 0);
  context.turns.set(turn.id, turn);
  assert.equal(turn.items[0].content[0].text, "继续");
  assert.equal(turn.itemsCursorAnchor, "item-074");
  for (const [pageItems, next] of [[items.slice(34, 74), "page-3"], [items.slice(0, 34), null]]) {
    turn = context.mergeHistoryTurn({ id: "turn", items: pageItems,
      itemsPageComplete: true, itemsComplete: next == null, itemsNextCursor: next },
    0, { older: true, beforeId: turn.itemsCursorAnchor });
    context.turns.set(turn.id, turn);
  }
  assert.equal(turn.items.length, 114);
  assert.deepEqual(Array.from(turn.items, item => item.id), items.map(item => item.id));
  assert.equal(turn.items.filter(item => item.type === "userMessage").length, 1);
  assert.equal(turn.itemsComplete, true);
  assert.equal(turn.itemsNextCursor, null);
});

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
  vm.runInContext(sourceSection("function threadActionReadOnly(", "function threadReadOnlyMessage(")
    + app.slice(apiFrom, apiTo), context);
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

function independentPromptOptimizationHarness(backend = "claude") {
  const control = { hidden: true, dataset: {}, attributes: {},
    setAttribute(name, value) { this.attributes[name] = value; } };
  const prompt = { value: "请整理这段需求", disabled: false, focus() {} }, add = {};
  const calls = [], notices = [], attachments = [{ id: "fixture-image", src: "data:image/png;base64,AQID" }];
  const h = { control, prompt, add, calls, notices, attachments,
    respond: async () => ({ state: "completed", result: { state: "completed", text: "优化后的需求", totalTokens: 12 } }) };
  let sequence = 0;
  const context = {
    backend, scope: `new:project:${backend}:fixture-a`, readOnly: false,
    session: { device: { id: "device-a" }, nativeConnected: false, serviceConnected: false,
      capabilities: { promptOptimization: true } },
    connected: false, busy: false, stopping: false, pending: null, selectionVersion: 1,
    editingQueuedMessage: null, selectedSkills: [{ name: "fixture-skill", path: "/fixtures/skill" }],
    promptOptimizations: new Map(),
    turns: new Map([["turn", { items: [
      { type: "userMessage", text: "先前需求" }, { type: "agentMessage", text: "已有解释" },
    ] }]]),
    $: selector => selector === "#prompt" ? prompt : selector === "#composer-add" ? add : control,
    draftKey: () => context.scope, threadReadOnly: () => context.readOnly,
    isClaude: () => context.backend === "claude",
    currentThreadDisplay: () => ({ tokenUsage: null }), contextUsage: () => null,
    compactUsageDescription: () => "", compactCommandAvailable: () => false, compactContext() {},
    goalComposerReason: () => "当前原生会话不提供目标", selectGoalCommand() {},
    deleting: false, composerCanAddImages: () => true, chooseComposerImages() {},
    activePopup: null, renderPopup() {}, closePopup() {}, closeComposerMenus() {},
    currentModelId: () => context.backend === "claude" ? "claude-opus-5-5" : "gpt-fixture",
    randomId: () => `optimize-${++sequence}`, messageCopyText: item => item.text,
    writePromptContent: (node, text) => { node.value = text; }, setPromptSelection() {}, rememberDraft() {},
    notice: (message, error = false) => notices.push({ message, error }), tokenCount: String,
    api: async (path, payload, id) => {
      calls.push({ path, payload: JSON.parse(JSON.stringify(payload)), id });
      if (path === "/api/action/cancelPromptOptimize") return { state: "completed", result: { state: "cancelled" } };
      assert.equal(path, "/api/action/optimizePrompt", "独立优化不能请求会话、模型目录或原生 CLI");
      return h.respond();
    },
  };
  nodeVm.createContext(context);
  const composer = app.indexOf("function updateComposer(");
  const tools = app.indexOf('  const add = $("#composer-add");', composer);
  const end = app.indexOf("  const queuedEditAction = queuedEditSubmittingAction();", tools);
  assert.ok(composer >= 0 && tools > composer && end > tools);
  nodeVm.runInContext(sourceSection("function promptOptimizationScope()", "function newChatProjectOptions(")
    + `\nfunction renderComposerTools(){${app.slice(tools, end)}}`, context);
  context.updateComposer = () => { context.renderComposerTools(); context.updatePromptOptimization(); };
  context.updateComposer();
  h.context = context;
  return h;
}

test("Claude 与 Codex 均显示独立提示词优化，不依赖原生连接并保留恢复、附件和写入限制", async () => {
  for (const backend of ["claude", "codex"]) {
    const h = independentPromptOptimizationHarness(backend), { context, control, prompt } = h;
    const skills = JSON.stringify(context.selectedSkills), images = JSON.stringify(h.attachments);
    assert.equal(control.hidden, false);
    assert.equal(control.disabled, false, "未连接原生服务也可以使用独立优化");
    assert.deepEqual(Array.from(h.add.menuOptions, option => option.value || option.label),
      backend === "claude" ? ["添加图片"] : ["compact", "goal", "添加图片"]);
    await context.activatePromptOptimization();
    assert.equal(prompt.value, "优化后的需求");
    assert.equal(control.dataset.state, "optimized");
    assert.equal(h.calls.length, 1);
    assert.deepEqual(Object.keys(h.calls[0].payload).sort(), ["input", "model", "recentContext"]);
    assert.equal(h.calls[0].payload.model, backend === "claude" ? "claude-opus-5-5" : "gpt-fixture");
    assert.deepEqual(h.calls[0].payload.recentContext, { user: "先前需求", assistant: "已有解释" });
    await context.activatePromptOptimization();
    assert.equal(prompt.value, "请整理这段需求");
    assert.equal(h.calls.length, 1, "恢复原文不能再次调用优化服务");
    assert.equal(JSON.stringify(context.selectedSkills), skills);
    assert.equal(JSON.stringify(h.attachments), images);
    for (const lock of ["readOnly", "input", "busy", "stopping", "pending", "capability"]) {
      const blocked = independentPromptOptimizationHarness(backend);
      if (lock === "input") blocked.prompt.disabled = true;
      else if (lock === "capability") blocked.context.session.capabilities.promptOptimization = false;
      else blocked.context[lock] = lock === "pending" ? {} : true;
      blocked.context.updateComposer();
      assert.equal(blocked.control.hidden, false);
      assert.equal(blocked.control.disabled, true, lock);
      await blocked.context.activatePromptOptimization();
      assert.equal(blocked.calls.length, 0, lock);
    }
  }
});

test("Claude 独立优化取消、继续编辑或切换实例与授权后，迟到结果不能覆盖草稿", async () => {
  for (const change of ["cancel", "edit", "scope", "device", "skills"]) {
    const h = independentPromptOptimizationHarness(), { context, prompt } = h, response = deferred();
    h.respond = () => response.promise;
    const running = context.activatePromptOptimization();
    assert.equal(h.control.dataset.state, "loading");
    if (change === "cancel") await context.activatePromptOptimization();
    if (change === "edit") prompt.value = "用户继续编辑";
    if (change === "scope") {
      context.scope = "new:project:claude:fixture-b"; context.selectionVersion++;
      prompt.value = "另一个实例的草稿";
    }
    if (change === "device") { context.session.device = { id: "device-b" }; prompt.value = "新授权的草稿"; }
    if (change === "skills") context.selectedSkills.push({ name: "new-skill", path: "/fixtures/new-skill" });
    context.updateComposer();
    const currentText = prompt.value, currentImages = JSON.stringify(h.attachments);
    response.resolve({ state: "completed", result: { state: "completed", text: "已过期的优化结果" } });
    await running;
    assert.equal(prompt.value, currentText, change);
    assert.equal(JSON.stringify(h.attachments), currentImages);
    assert.equal(h.calls.filter(call => call.path === "/api/action/optimizePrompt").length, 1);
    assert.equal(h.calls.filter(call => call.path === "/api/action/cancelPromptOptimize").length,
      change === "device" ? 0 : 1, "新授权不能取消旧设备的请求");
  }
  const failed = independentPromptOptimizationHarness();
  failed.respond = async () => ({ state: "completed", result: { state: "failed", message: "优化配置不可用" } });
  await failed.context.activatePromptOptimization();
  assert.equal(failed.prompt.value, "请整理这段需求");
  assert.equal(failed.notices.at(-1).error, true);
  assert.equal(failed.control.hidden, false);
  assert.equal(failed.control.disabled, false);
});

function mermaidRootsHarness() {
  const observers = [], rendered = [];
  class Observer {
    constructor(callback, options) {
      this.callback = callback; this.options = options; this.blocks = new Set(); this.disconnected = false;
      observers.push(this);
    }
    observe(block) { this.blocks.add(block); }
    unobserve(block) { this.blocks.delete(block); }
    disconnect() { this.disconnected = true; this.blocks.clear(); }
  }
  const context = vm.createContext({
    IntersectionObserver: Observer, document: { documentElement: { dataset: { theme: "light" } } },
    renderMermaidBlock: (block, callback) => { rendered.push(block); callback?.(); },
  });
  vm.runInContext(markdownRendererSource.slice(markdownRendererSource.indexOf("  const diagramRoots ="),
    markdownRendererSource.indexOf("  function renderMermaidBlock(")), context);
  const root = name => {
    const block = { name, isConnected: true, querySelector: () => ({ textContent: "flowchart TD; A --> B" }) };
    return { isConnected: true, blocks: [block], querySelectorAll() { return this.blocks; },
      contains(value) { return this.blocks.includes(value); } };
  };
  return { context, observers, rendered, root };
}

test("消息与文件分别观察 Mermaid，打开文件不会取消消息里的离屏图表", () => {
  const h = mermaidRootsHarness(), messages = h.root("message"), files = h.root("file");
  h.context.renderMermaidBlocks(messages);
  h.context.renderMermaidBlocks(files);
  assert.equal(h.observers.length, 2);
  assert.equal(h.observers[0].disconnected, false);
  assert.equal(h.observers[0].options.root, messages);
  assert.equal(h.observers[1].options.root, files);
  for (const observer of h.observers)
    observer.callback([...observer.blocks].map(target => ({ target, isIntersecting: true })));
  assert.deepEqual(h.rendered.map(block => block.name), ["message", "file"]);
  h.context.renderMermaidBlocks(messages);
  assert.equal(h.observers.length, 2, "重新扫描复用当前容器的观察器");
});

test("文件关闭、容器移除或主题更新后，旧 Mermaid 任务不能写回新内容", () => {
  const h = mermaidRootsHarness(), messages = h.root("message"), files = h.root("file");
  h.context.renderMermaidBlocks(messages); h.context.renderMermaidBlocks(files);
  const message = messages.blocks[0], file = files.blocks[0];
  message.mermaidKey = "message-key"; file.mermaidKey = "file-key";
  h.context.releaseMermaidBlocks(files);
  assert.equal(h.observers[1].disconnected, true);
  assert.equal(h.context.currentDiagramBlock(file, "file-key"), false);
  assert.equal(h.context.currentDiagramBlock(message, "message-key"), true);
  message.mermaidKey = "new-theme";
  assert.equal(h.context.currentDiagramBlock(message, "old-theme"), false);
  assert.equal(message.mermaidKey, "new-theme", "旧任务不能清除新主题标识");
  h.context.renderMermaidBlocks(files);
  files.isConnected = false;
  h.context.renderMermaidBlocks(messages);
  assert.equal(h.observers[2].disconnected, true);
});

test("文件图表只在当前 Markdown 预览扫描，并使用文件自己的布局事务", () => {
  const calls = [], body = {};
  const context = vm.createContext({
    disposed: false, route: { kind: "file" }, contentResponse: { kind: "text" }, sourceMode: false, body,
    renderMermaidBlocks: (root, _onRender, withLayout) => { calls.push(root); withLayout(() => calls.push("change")); },
    withStableMessageLayout: (root, change) => { assert.equal(root, body); calls.push("layout"); change(); },
  });
  const source = awaitSource => awaitSource.slice(awaitSource.indexOf("  function refreshFileDiagrams()"),
    awaitSource.indexOf("  const diagramThemeObserver ="));
  vm.runInContext(source(sessionFiles), context);
  context.refreshFileDiagrams();
  assert.deepEqual(calls, [body, "layout", "change"]);
  for (const change of [
    () => { context.sourceMode = true; },
    () => { context.sourceMode = false; context.route = { kind: "list" }; },
    () => { context.route = { kind: "file" }; context.disposed = true; },
  ]) { change(); context.refreshFileDiagrams(); }
  assert.equal(calls.length, 3, "源码、目录和已销毁页面不继续渲染");
});

test("首次目录尚未返回时可以搜索文件，不把两侧空 root 当成缓存命中", async () => {
  for (const listResponse of [null, undefined]) {
    const results = [{ name: "diagram.md", path: "docs/diagram.md", kind: "file" }];
    const reads = [];
    const context = vm.createContext({
      responses: new Map(), listResponse, searchMatches: null, searchEpoch: 0, searchController: null,
      route: { kind: "list" }, AbortController, listStatus: {},
      listing: { location: { set() {} } }, renderDirectory() {}, message() {},
      read: async (...input) => { reads.push(input); return { root: "/project", files: results, truncated: false }; },
    });
    vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("  async function searchProject("),
      sessionFiles.indexOf("  async function openFile(")), context);
    await context.searchProject("diagram.md");
    assert.equal(reads.length, 1);
    assert.equal(reads[0][3], "diagram.md");
    assert.equal(context.searchMatches, results);
  }
});

function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function installSentMessageDisplayHarness(context) {
  context.sentMessageDisplays ??= new Map();
  context.turns ??= new Map();
  vm.runInContext([
    sourceSection("function rememberSendDisplay(", "function queueDisplayMessages("),
    sourceSection("function queueMessageIdentity(", "function applyQueueSnapshot("),
  ].join("\n"), context);
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
    tokenUsageVersion: 0, questionVersion: 0, statusVersion: 0, goalVersion: 0, compactionVersion: 0, messageVersion: 0,
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
    confirmedSendDisplayFor: () => null, reconcileSentMessageDisplays: () => false,
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
  installSentMessageDisplayHarness(context);
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
    claudeThreadId: id => typeof id === "string" && id.startsWith("claude-"),
    queuedActionBusy: new Set(), queuedEditActions: new Set(["beginQueuedEdit", "editQueued", "cancelQueuedEdit"]),
    pendingSteer: null, turns: new Map(), queueVersion: 0, goalVersion: 0, submittedDrafts: new Map(),
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
  installSentMessageDisplayHarness(context);
  vm.runInContext([
    sourceSection("function threadActionReadOnly(", "function threadReadOnlyMessage("),
    sourceSection("const pendingFeedbackDelayMs =", "function clearPendingFeedbackTimer("),
    sourceSection("function savePending(", "function updateGoalComposerPending("),
    sourceSection("function ordinaryInputRecord(", "function rememberDraft("),
    sourceSection("async function mutate(", "function acceptedSendItem("),
    sourceSection("async function resolvePending()", "let loginPageCleanup"),
    sourceSection("function confirmQueuedSteerDisplay(", "function renderQueuedMessages("),
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

test("Claude 引导确认前不展示正文，成功后保留一份消息", async () => {
  const h = queuedReceiptHarness();
  h.context.thread.backend = "claude";
  const operation = h.invoke("steerQueued");
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 0);
  h.receipt(0, "completed", { ok: true, threadId: "main", scope: "all",
    queuedMessages: h.context.thread.queuedMessages, queuedEdits: [] });
  await operation;
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 1);
  assert.equal(h.context.pendingSteer, null);
  assert.equal(h.context.pending, null);
  assert.equal(h.requests.length, 1);
  assert.equal(h.draft.text, "操作期间保留的普通草稿");
});

test("引导原生正文早于回执时按身份接收，确认前不凭同文问题展示消息", async () => {
  const h = queuedReceiptHarness();
  h.context.thread.queuedMessages[0].clientUserMessageId = "old-client";
  const operation = h.invoke("steerQueued"), preview = h.context.pendingSteer;
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 0);
  h.context.turns.set("active-turn", { id: "active-turn", status: "inProgress", items: [
    { id: "original", type: "userMessage", clientId: "initial-client",
      content: [{ type: "text", text: preview.text }] },
  ] });
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 1,
    "同文初始问题不能被误判为引导已送达");
  h.context.turns.get("active-turn").items.push({
    id: "server-steer", type: "userMessage", clientId: "restored-client",
    restoreMessage: { id: "queued-a" }, content: [{ type: "text", text: preview.text }],
  });
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 2);
  h.completed(0);
  await operation;
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 2, "成功回执不能重复插入引导正文");
  assert.equal(h.context.pendingSteer, null);
  assert.equal(h.requests.length, 1);
});

test("引导成功后历史空档和迟到读取不移除正文，也不影响后续引导", async () => {
  const reload = deferred(), h = queuedReceiptHarness({ refresh: () => reload.promise });
  const operation = h.invoke("steerQueued"), preview = h.context.pendingSteer;
  h.completed(0);
  await operation;
  assert.equal(h.context.pendingSteer, null);
  const displayed = h.context.withSentMessageDisplays().flatMap(turn => turn.items);
  assert.equal(displayed.length, 1);
  assert.equal(displayed[0].content[0].text, preview.text);
  reload.resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(h.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 1,
    "成功后的读取缺项不能清掉已确认内容");
  const laterReload = deferred(), later = queuedReceiptHarness({ refresh: () => laterReload.promise });
  const laterOperation = later.invoke("steerQueued");
  later.completed(0);
  await laterOperation;
  const nextPreview = { threadId: "main", messageId: "queued-b", text: "下一条消息" };
  later.context.pendingSteer = nextPreview;
  laterReload.resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(later.context.pendingSteer, nextPreview);
});

function nativeSteerHistoryHarness() {
  const context = vm.createContext({
    array: value => Array.isArray(value) ? value : [],
    failure: (code, message) => Object.assign(new Error(message), { code }),
    HISTORY_ITEM_PAGE_LIMIT: 40, timestamp: value => Number.isFinite(value) ? value : null,
    messageActionManager() {}, nativeConversationTurns: value => value.turns,
    nativeTurnId: turn => turn.turnId, nativeTurnStatus: turn => turn.status,
    initialUserMessage: turn => turn?.items.find(item => item.type === "userMessage"),
    subagentIdentity: () => null, editableMessageState: () => ({ metadata: null }),
    manager: { getConversation: () => context.conversation },
  });
  vm.runInContext(native.slice(native.indexOf("  function nativeDisplayMessageItem("),
    native.indexOf("  async function requestClientNativeRpc("))
    + native.slice(native.indexOf("  function nativeAssistantSentAtMs("),
      native.indexOf("  async function editLastMessage(")), context);
  return context;
}

test("侧边引导正文与确认共用 ID 时，执行中、结束后分页及重建读取都保留一份图文", () => {
  const h = nativeSteerHistoryHarness();
  const content = [{ type: "text", text: "同文引导" }, { type: "image", url: "inline:image" }];
  const accepted = id => ({ id: `local-${id}`, type: "steeringUserMessage", status: "accepted",
    serverUserMessageId: id, clientUserMessageId: `client-${id}`, input: structuredClone(content),
    restoreMessage: { id: `queue-${id}`, createdAt: 2000 } });
  const turn = { turnId: "active-turn", status: "completed", turnStartedAtMs: 1000, items: [
    { id: "original", type: "userMessage", clientId: "initial", content: [{ type: "text", text: "初始问题" }] },
    ...Array.from({ length: 45 }, (_, index) => ({ id: `tool-${index}`, type: "commandExecution" })),
    accepted("server-a"), { id: "server-a", type: "steered" },
    { id: "server-b", type: "steered" }, accepted("server-b"),
    { id: "orphan", type: "steered" },
  ] };
  const before = JSON.stringify(turn);
  for (const fresh of [h, nativeSteerHistoryHarness()]) {
    const full = fresh.nativeCompleteSideItemsPage(turn);
    assert.equal(full.nextCursor, null);
    assert.equal(full.items.filter(item => item.type === "userMessage").length, 3,
      "相同正文的两个原生逻辑消息均保留；确认状态不能覆盖正文");
    assert.equal(full.items.filter(item => item.id === "orphan").length, 1, "未关联正文的原生状态保留");
    const restored = new Map();
    let cursor = null, pageCount = 0;
    do {
      const page = fresh.nativeSideItemsPage(turn, cursor);
      for (const item of page.items) restored.set(item.id, item);
      cursor = page.nextCursor;
      assert.ok(++pageCount < 5, "分页游标必须前进");
    } while (cursor);
    assert.equal(pageCount, 2);
    for (const id of ["server-a", "server-b"]) {
      const message = restored.get(id);
      assert.equal(message.type, "userMessage");
      assert.deepEqual(structuredClone(message.content), content);
      assert.equal(message.clientId, `client-${id}`);
    }
  }
  assert.equal(JSON.stringify(turn), before, "读取不能改写原生历史");
});

test("侧边引导确认不能挤掉正文来源，恢复后的时间与逻辑 ID 保留原生值", () => {
  const h = nativeSteerHistoryHarness();
  const steer = { id: "local", type: "steeringUserMessage", status: "accepted",
    serverUserMessageId: "server", clientUserMessageId: "logical", input: [{ type: "text", text: "补充" }],
    restoreMessage: { id: "queue", createdAt: 2345 } };
  h.conversation = { turns: [{ turnId: "turn", status: "completed", turnStartedAtMs: 1000,
    items: [steer, { id: "server", type: "steered" }] }] };
  const items = h.nativeDisplayMessageItems(h.conversation.turns[0].items);
  const [turn] = h.enrichMessageMetadata("side", [{ id: "turn", status: "completed", items }], { readOnly: true });
  assert.equal(turn.items.length, 1);
  assert.equal(turn.items[0].sentAtMs, 2345);
  assert.equal(turn.items[0].messageMetadata.sentAtMs, 2345);
  assert.equal(turn.items[0].clientId, "logical");
});

test("侧边引导在实时轮次、重进历史与迟到旧状态中只显示一份正文", () => {
  const h = vm.createContext({
    turns: new Map(), itemVersions: new Map(), turnVersions: new Map(), turnFieldVersions: new Map(),
    partialItems: new Map(), enrichTurnTranscriptEvents: value => value,
  });
  vm.runInContext(sourceSection("function normalizeDisplayedUserMessage(", "async function reloadThread("), h);
  const steer = { id: "local", type: "steeringUserMessage", status: "accepted",
    serverUserMessageId: "server", clientUserMessageId: "logical", input: [{ type: "text", text: "补充正文" }] };
  const marker = { id: "server", type: "steered" };
  for (const items of [[steer, marker], [marker, steer]]) {
    const turn = h.mergeHistoryTurn({ id: "turn", status: "completed", items }, 0);
    assert.equal(turn.items.length, 1);
    assert.equal(turn.items[0].type, "userMessage");
    assert.equal(turn.items[0].content[0].text, "补充正文");
    h.turns.set(turn.id, turn);
    assert.equal(h.mergeItemProjection(turn.items[0], marker).type, "userMessage");
    const stale = h.mergeHistoryTurn({ id: "turn", status: "completed", items: [marker] }, 100);
    assert.equal(stale.items[0].type, "userMessage", "迟到旧分页不能再次抹掉正文");
    h.turns.clear();
  }
});

test("队列引导和删除unknown保留原回执，刷新只读恢复且不清普通草稿", async () => {
  for (const action of ["steerQueued", "removeQueued"]) {
    const first = queuedReceiptHarness(), original = structuredClone(first.draft);
    const operation = first.invoke(action);
    assert.equal(first.requests.length, 1);
    const id = first.requests[0].requestId;
    assert.equal(first.context.pending.id, id);
    first.receipt(0, "unknown", { ok: false, outcomeUnknown: true });
    await operation;
    assert.equal(first.context.withSentMessageDisplays().flatMap(turn => turn.items).length, 0,
      "未知引导回执不能展示正文");
    assert.equal(first.notices.length, 0, "快速返回未确认时不得抢先弹出错误提示");
    assert.equal(first.requests[1].path, `/api/receipts/${id}`);
    await first.invoke(action);
    await first.invoke(action === "steerQueued" ? "removeQueued" : "steerQueued");
    assert.equal(first.requests.length, 2, "未知期间不能再次引导、删除或普通发送补偿");
    first.receipt(1, "unknown", { ok: false, outcomeUnknown: true });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(first.context.pending.id, id);
    first.context.renderPending();
    assert.equal(first.area.children.length, 0, "正常等待的前八秒不额外显示回执横幅");
    first.context.pending.time -= 5000;
    first.context.renderPending();
    assert.equal(first.area.children.length, 0, "队列操作等待五秒时仍不显示回执横幅");
    first.context.pending.time -= 3000;
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
    assert.equal(reopened.context.withSentMessageDisplays().flatMap(turn => turn.items).length,
      action === "steerQueued" ? 1 : 0, "原回执恢复后才确认引导正文，删除不产生正文");
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
    assert.equal(fixture.context.pending.receiptMissing, true);
    assert.deepEqual(fixture.notices, [], "不存在的回执保持页内提示，不反复弹红色错误");
    const check = fixture.context.resolvePending();
    fixture.receipt(2, "completed", { ok: true, threadId: "other-thread", messageId: "queued-a" });
    await check;
    assert.equal(fixture.context.pending.id, id);
    await fixture.invoke(action);
    assert.equal(fixture.requests.filter(request => request.path.startsWith("/api/action/")).length, 1);
    assert.deepEqual(fixture.clearedDrafts, []);
  }
});

test("队列明确未派发的409立即结束等待，保留普通草稿且不查询不存在的回执", async () => {
  const h = queuedReceiptHarness(), original = structuredClone(h.draft);
  const operation = h.invoke("steerQueued");
  h.requests[0].reject(Object.assign(new Error("验收入口未开放此操作"),
    { code: 409, notSubmitted: true }));
  await operation;
  assert.equal(h.context.pending, null);
  assert.equal(h.requests.length, 1);
  assert.deepEqual(h.draft, original);
  assert.deepEqual(h.clearedDrafts, []);
});

test("Claude 旧队列回执缺失时，以原生接收记录和当前队列核对残留，不能凭404假定完成", async () => {
  for (const delivered of [true, false]) {
    const h = queuedReceiptHarness(), original = structuredClone(h.draft);
    const threadId = "claude-12345678-1234-1234-1234-123456789abc";
    h.context.selectedId = threadId;
    h.context.thread.id = threadId;
    h.context.thread.backend = "claude";
    const operation = h.context.steerQueuedMessage(threadId, "queued-a");
    h.receipt(0, "unknown", { ok: false, outcomeUnknown: true });
    await operation;
    h.requests[1].reject(Object.assign(new Error("missing"), { code: 404 }));
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.requests[2].path, "/api/read/queue");
    h.requests[2].resolve({ ok: true, threadId, queuedMessages: [{ id: "other-message" }], queuedEdits: [] });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.requests[3].path, "/api/read/history");
    h.requests[3].resolve({ ok: true, threadId, data: [{ items: [{
      type: "userMessage", clientId: delivered ? "queued-a" : "unrelated-message"
    }] }] });
    await new Promise(resolve => setImmediate(resolve));
    if (delivered) {
      assert.equal(h.context.pending, null);
      assert.deepEqual(h.context.thread.queuedMessages.map(message => message.id), ["other-message"]);
    } else {
      assert.equal(h.context.pending.receiptMissing, true);
      assert.equal(h.context.thread.queuedMessages[0].id, "queued-a");
    }
    assert.equal(h.requests.filter(request => request.path.startsWith("/api/action/")).length, 1);
    assert.deepEqual(h.notices, []);
    assert.deepEqual(h.draft, original);
    assert.deepEqual(h.clearedDrafts, []);
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
    sourceSection("function mergeIncompleteSidebar(", "async function loadSidebar("),
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
    loadHistory: async () => {}, markVisibleThreadRead: async () => {}, threadDeleted: () => false,
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

test("侧边过期后仍读取服务保留的最新历史和较早消息", async () => {
  const fixture = sideExpiryHarness(() => jsonResponse({
    ok: true, data: [{ id: "latest-turn", status: "completed",
      items: [{ id: "latest-answer", type: "agentMessage", text: "过期前的最终回复" }] }],
    nextCursor: null, ephemeral: true, sideHistoryVersion: 1
  }));
  const { context, id, calls } = fixture;
  await context.retireExpiredSideThread(id);
  Object.assign(context, {
    transcriptEvents: new Map(), mergeHistoryTurn: turn => turn, mergeTranscriptEvents() {},
    syncEditableMessageMetadata() {}, rememberTranscript() {}, discardPersistentThreadDisplay() {}
  });
  await context.readHistoryPage(false, context.selectionVersion);
  assert.equal(calls.length, 1, "执行过期不能阻断只读补读");
  assert.equal(context.turns.get("latest-turn").items[0].text, "过期前的最终回复");
  assert.equal(context.historyError, "");
  assert.equal(context.latestHistoryReadFailed, false);
  assert.equal(context.sideThreadExpired(id), true, "历史补读不恢复写入能力");
  context.historyCursor = "side-turn:latest-turn";
  await context.readHistoryPage(true, context.selectionVersion);
  assert.equal(calls.length, 2, "过期后仍可向前翻页");
  assert.equal(calls.every(call => call.path === "/api/read/history"), true);
});

test("过期侧边阅读后提交已读，清除父会话蓝点并保留其它侧边未读", async () => {
  const acknowledgements = [], timers = [];
  const transport = transportClient(({ path, options }) => {
    acknowledgements.push({ action: path.split("/").at(-1), payload: JSON.parse(options.body) });
    return jsonResponse({ requestId: options.headers.get("Idempotency-Key"),
      state: "completed", result: { ok: true } });
  });
  const parent = { id: "parent", hasUnreadTurn: false, sideThreadAttention: { unread: true } };
  const otherParent = { id: "other-parent", sideThreadAttention: { unread: true } };
  const context = Object.assign(transport.context, {
    session: { device: { id: "device" }, readOnly: false }, selectedId: "side",
    thread: { id: "side", viewKind: "side", readOnly: true, sideThreadExpired: true, hasUnreadTurn: true },
    relatedSessionView: { threadId: "side", kind: "side" }, relatedThreadViews: new Map(),
    normalizeRelatedView: value => value, connected: true, historyInitialized: true, historyError: "",
    transcriptChecking: false, document: { hidden: false }, followMessages: true,
    threadDeletionLocked: () => false, threadDeleted: () => false,
    readMarkRequests: new Map(), recentPrioritySelectionSuppressions: new Set(),
    sidebarReadRefreshSuppressions: new Set(), setTimeout: callback => timers.push(callback),
    selectionVersion: 1, messageVersion: 0, recoveryGeneration: 0, randomId: () => "read-receipt",
    privateInputScope: value => value.device.id, goalComposerAction: () => false,
    sidebar: { recent: [parent, otherParent], pinnedThreads: [], projects: [],
      unreadSideThreads: [{ id: "side", parentThreadId: "parent" },
        { id: "other-side", parentThreadId: "other-parent" }] },
    sessionMenu: { markSideThreadRead() {} }, syncSideThreadUnread() {}, renderSidebar() {}
  });
  vm.runInContext([
    sourceSection("function threadReadOnly(", "function threadReadOnlyMessage("),
    sourceSection("async function mutate(", "function reportMutationError("),
    sourceSection("function markSidebarThreadReadLocally(", "function sidebarAttentionState("),
    sourceSection("async function markVisibleThreadRead(", "function beginMessageEdit(")
  ].join("\n"), context);
  await context.markVisibleThreadRead();
  assert.equal(acknowledgements.length, 1, "侧边只读不应禁止已读回执");
  assert.equal(acknowledgements[0].action, "markRead");
  assert.equal(acknowledgements[0].payload.threadId, "side");
  assert.equal(transport.calls.length, 1, "已读须通过真实 api 和传输层，不得被中间的只读检查拦截");
  assert.equal(context.thread.hasUnreadTurn, false);
  assert.equal(parent.sideThreadAttention.unread, false);
  assert.equal(otherParent.sideThreadAttention.unread, true);
  assert.deepEqual(context.sidebar.unreadSideThreads.map(item => item.id), ["other-side"]);
  parent.sideThreadAttention.unread = true;
  context.sidebar.unreadSideThreads.push({ id: "side", parentThreadId: "parent" },
    { id: "same-parent-side", parentThreadId: "parent" });
  context.markSidebarThreadReadLocally("side");
  assert.equal(parent.sideThreadAttention.unread, true, "同父会话仍有未读侧边时保留蓝点");
  context.markSidebarThreadReadLocally("same-parent-side");
  assert.equal(parent.sideThreadAttention.unread, false);
  context.session.readOnly = true;
  await context.markVisibleThreadRead();
  assert.equal(acknowledgements.length, 1, "设备只读授权仍不允许提交已读");
  await assert.rejects(context.mutate("markRead", { threadId: "side" }), error => error.code === 403);
  context.session.readOnly = false;
  await assert.rejects(context.mutate("send", { threadId: "side" }), error => error.code === 403);
  context.thread.viewKind = "subagent";
  await assert.rejects(context.mutate("markRead", { threadId: "side" }), error => error.code === 403);
});

test("侧边过期且从未读到正文时显示读取失败，不伪装成空会话或提交已读", async () => {
  const h = sideExpiryHarness(() => jsonResponse({
    ok: false, message: `thread not loaded: ${expiredSideProbe.threadId}`
  }, 503));
  h.context.turns.clear();
  await h.context.readHistoryPage(false, h.context.selectionVersion);
  assert.match(h.context.historyError, /历史暂时不可读取/);
  assert.equal(h.calls.some(call => call.path.startsWith("/api/action/")), false);
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
    "questionReplyReceipts", "questionDrafts", "sentMessageDisplays"]) context[name] = new Map();
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function clearSentMessageDisplays(", "function sentMessageIdentities("),
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

test("服务模式返回明确删除回执后立即清理目标，不提示未确认", async () => {
  const h = sidebarDeletionHarness();
  let refreshed = 0;
  h.context.loadSidebar = async () => { refreshed++; };
  const deletion = h.context.deleteSidebarThread(h.target);
  await new Promise(resolve => setImmediate(resolve));
  h.reply.resolve({ ok: true, threadId: h.target.id, deleted: true, status: "local_deleted",
    executionTransport: "app-server-stdio" });
  await deletion;
  assert.equal(h.context.threadDeleted(h.target.id), true);
  assert.equal(h.context.sidebar.recent.some(item => item.id === h.target.id), false);
  assert.deepEqual(h.notices, [{ message: "会话已永久删除", error: false }]);
  assert.equal(refreshed, 1);
  assert.equal(h.calls.length, 1);
  h.assertControlRetained();
});

test("旧服务删除回执恢复后只核对原回执，不再次派发删除", async () => {
  const h = sidebarDeletionHarness(), reads = [];
  h.context.threadDeletionChecks = new Set();
  const record = { id: "original-delete-receipt", threadId: h.target.id, deviceId: "device-a", state: "unknown" };
  h.context.threadDeletions.set(h.context.threadDeletionKey(h.target.id), record);
  h.context.api = async path => {
    reads.push(path);
    return { state: "completed", result: {
      ok: true, threadId: h.target.id, deleted: true, status: "local_deleted", executionTransport: "app-server-stdio",
    } };
  };
  vm.runInContext(sourceSection("async function checkThreadDeletion(", "function restoreSidebarPreferences("), h.context);
  await h.context.checkThreadDeletion(record);
  assert.deepEqual(reads, ["/api/receipts/original-delete-receipt"]);
  assert.equal(h.calls.length, 0);
  assert.equal(h.context.threadDeleted(h.target.id), true);
  assert.deepEqual(h.notices, [{ message: "已确认会话永久删除", error: false }]);
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
    rememberSendDisplay: () => null, finishSendDisplay() {},
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
    "approvalDrafts", "threadDeletions", "sentMessageDisplays"]) context[name] = new Map();
  context.drafts.set("main", originalText);
  vm.createContext(context);
  vm.runInContext([
    sourceSection("function threadActionReadOnly(", "function threadReadOnlyMessage("),
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

test("Claude 目录信任检查只在明确确认后附加精确目录，取消和已信任均不写确认参数", async () => {
  for (const state of ["trusted", "confirm", "cancel"]) {
    const calls = [];
    const context = vm.createContext({
      api: async (path, payload) => {
        calls.push({ path, payload });
        return { ok: true, instanceId: "claude:fixture", instanceLabel: "Claude · 测试",
          trusted: state === "trusted", trustPath: "/project" };
      },
      confirmAction: async (title, message, action) => {
        calls.push({ title, message, action });
        return state === "confirm";
      },
    });
    vm.runInContext(sourceSection("async function prepareClaudeProjectCreation(", "async function sendMessage("), context);
    const payload = { backend: "claude", instanceId: "claude:fixture", projectId: "project", prompt: "原消息" };
    assert.equal(await context.prepareClaudeProjectCreation(payload, () => true), state !== "cancel");
    assert.equal(calls[0].path, "/api/read/projectTrust");
    assert.equal(payload.trustProject, state === "confirm" ? true : undefined);
    assert.equal(payload.expectedProjectPath, state === "confirm" ? "/project" : undefined);
    assert.equal(payload.prompt, "原消息");
    if (state === "trusted") assert.equal(calls.length, 1);
    else {
      assert.match(calls[1].message, /Claude · 测试/);
      assert.match(calls[1].message, /\/project/);
      assert.equal(calls[1].action, "信任并继续");
    }
  }
});

test("Claude 信任结果实例不匹配或确认期间切换选择时不能继续发送", async () => {
  let selected = true;
  const context = vm.createContext({
    api: async () => ({ ok: true, trusted: false, instanceId: "claude:fixture", trustPath: "/project" }),
    confirmAction: async () => { selected = false; return true; },
  });
  vm.runInContext(sourceSection("async function prepareClaudeProjectCreation(", "async function sendMessage("), context);
  const payload = { backend: "claude", instanceId: "claude:fixture", projectId: "project" };
  assert.equal(await context.prepareClaudeProjectCreation(payload, () => selected), false);
  assert.equal(payload.trustProject, undefined);
  await assert.rejects(context.prepareClaudeProjectCreation({ instanceId: "claude:other" }, () => true),
    /检查结果不匹配/);
});

test("Claude 新建入口先等待目录信任，确认才提交一次创建，取消保留输入且不创建空会话", async () => {
  for (const accepted of [false, true]) {
    const h = sendRecoveryHarness(), { context } = h, approval = deferred();
    context.creating = true;
    context.creationBackend = "claude";
    context.creationInstanceId = "claude:wsl:test";
    context.appearance.sessionTypes.claude = true;
    context.session.capabilities.claudeProjectTrust = true;
    context.confirmAction = () => approval.promise;
    context.selectCreatedThread = async () => {};
    const query = context.$;
    context.$ = (selector, root) => selector === "#project-select" ? { value: "project" } : query(selector, root);
    vm.runInContext(sourceSection("async function prepareClaudeProjectCreation(", "async function sendMessage("), context);
    const sending = context.sendMessage();
    assert.equal(h.actions.length, 1);
    assert.equal(h.actions[0].path, "/api/read/projectTrust");
    h.actions[0].resolve({ ok: true, instanceId: "claude:wsl:test", trusted: false, trustPath: "/project" });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(h.actions.length, 1, "确认前不得派发 create 或首条消息");
    approval.resolve(accepted);
    await new Promise(resolve => setImmediate(resolve));
    if (accepted) {
      assert.equal(h.actions.length, 2);
      const create = h.actions[1];
      assert.equal(create.path, "/api/action/create");
      assert.equal(create.payload.trustProject, true);
      assert.equal(create.payload.expectedProjectPath, "/project");
      create.resolve({ requestId: create.requestId, state: "completed", result: { ok: true, threadId: "created" } });
    }
    await sending;
    if (!accepted) {
      assert.equal(h.actions.length, 1);
      assert.equal(h.prompt.value, h.originalText);
      assert.equal(context.submittedDrafts.size, 0);
      assert.equal(context.busy, false);
    }
  }
});

function idleSendDisplayHarness(mode = "native") {
  const h = sendRecoveryHarness(), controls = cachedControlsHarness(h.context);
  const { context } = h, frames = [], reloads = [];
  const controlsQuery = context.$;
  Object.assign(context, {
    session: { ...context.session, runtime: { mode }, capabilities: { queueSendRecovery: true, compactionQueueSend: true } },
    relatedSessionView: null, recentHistoryPage: null, relatedThreadViews: new Map(), normalizeRelatedView: () => null,
    validRelatedThreadId: value => typeof value === "string" && /^[A-Za-z0-9_-]+$/.test(value),
    thread: { id: "main", status: { type: "idle" }, queuedMessages: [], queuedEdits: [] },
    turns: new Map([["previous", { id: "previous", status: "completed", items: [] }]]),
    notifiedQueue: [], notifiedQueueEdits: [], queueSnapshotVersion: null, retiredQueueEpochs: new Set(),
    recordDiagnostic() {}, compactionQueueSendReason: () => "",
    scheduleReload: () => reloads.push("thread"), reloadQueue: async () => reloads.push("queue"),
    renderMessages: () => frames.push(context.withSentMessageDisplays()),
    updateComposer: () => context.renderQueueArea(),
    $: (selector, root) => !root && selector === "#prompt" ? h.prompt : controlsQuery(selector, root),
  });
  vm.runInContext([
    sourceSection("function acceptedSendItem(", "async function confirmPendingSend("),
    sourceSection("function queueSnapshotIsVersioned(", "async function reloadQueue("),
    sourceSection("function applyQueuedSendResult(", "function queueMessageText("),
  ].join("\n"), context);
  const snapshot = (messages, revision) => ({
    ok: true, threadId: "main", queuedMessages: messages, queuedEdits: [],
    queueEpoch: "send-display-epoch", queueRevision: revision
  });
  const queued = action => ({
    id: "server-message", text: action.payload.prompt, createdAt: 1, submission: { status: "queued" },
    ...(mode === "native"
      ? { serverQueuedMessageId: "native-message", submissionOptions: { clientUserMessageId: action.payload.clientUserMessageId } }
      : { clientUserMessageId: action.payload.clientUserMessageId })
  });
  const completeQueued = (action, messages, revision) => action.resolve({
    requestId: action.requestId, state: "completed",
    result: { ...snapshot(messages, revision), deliveryState: "queued", confirmed: true }
  });
  return { ...h, queue: controls.queue, frames, reloads, snapshot, queued, completeQueued };
}

const displayedUserMessages = context => context.withSentMessageDisplays().flatMap(turn => turn.items)
  .filter(item => item.type === "userMessage");

test("CDP 与服务普通发送成功后进入正文，空队列和迟到历史不移除已确认消息", async () => {
  for (const mode of ["native", "service"]) {
    const h = idleSendDisplayHarness(mode), { context } = h;
    const send = context.sendMessage();
    await new Promise(resolve => setImmediate(resolve));
    const action = h.actions[0], queued = h.queued(action);
    assert.equal(displayedUserMessages(context).length, 0, "发出请求不能提前显示正文");
    action.resolve({ requestId: action.requestId, state: "completed", result: {
      ok: true, threadId: "main", clientUserMessageId: action.payload.clientUserMessageId,
      queueSyncPending: true, deliveryState: "accepted", result: { status: "sent", turnId: "started" }
    } });
    await send;
    const confirmed = displayedUserMessages(context)[0];
    assert.equal(confirmed.content[0].text, h.originalText);
    assert.equal(confirmed.remoteConfirmedInput, true);
    assert.equal(context.acceptedSendItem([confirmed], action.payload.clientUserMessageId), undefined,
      "成功展示投影不能冒充原生核对证据");
    context.applyQueueSnapshot("main", [], [], h.snapshot([], 1));
    assert.equal(displayedUserMessages(context)[0], confirmed, "队列先移除，历史尚未同步也要保持正文");
    context.applyQueueSnapshot("main", [queued], [], h.snapshot([queued], 2));
    assert.equal(h.queue.hidden, true, "迟到的未发送队列不能降级已确认成功的消息");
    assert.equal(displayedUserMessages(context).length, 1);
    const native = { id: "real-user", type: "userMessage", clientId: action.payload.clientUserMessageId,
      content: [{ type: "text", text: h.originalText }] };
    context.turns.set("started", { id: "started", status: "completed", items: [native] });
    const adopted = displayedUserMessages(context)[0];
    assert.equal(adopted.id, native.id, "原生消息接替展示投影");
    assert.equal(adopted.remoteConfirmedInput, true, "成功身份进入原有会话缓存以便重进恢复");
    assert.equal(displayedUserMessages(context).length, 1);
    context.turns.get("started").items = [];
    assert.equal(displayedUserMessages(context)[0], adopted, "较旧历史缺项不能让成功消息消失");
    context.turns.delete("started");
    assert.equal(displayedUserMessages(context)[0], adopted, "整轮暂缺也保持已显示的消息");
    assert.equal(h.prompt.value, "");
    assert.equal(h.actions.length, 1);
  }
});

test("仅确认入队的消息留在队列，运行状态变化不能提前展示正文", async () => {
  for (const mode of ["native", "service"]) {
    const h = idleSendDisplayHarness(mode), { context } = h;
    context.thread.status = { type: "active" };
    const send = context.sendMessage();
    await new Promise(resolve => setImmediate(resolve));
    const action = h.actions[0], queued = h.queued(action);
    context.applyQueueSnapshot("main", [queued], [], h.snapshot([queued], 1));
    h.completeQueued(action, [queued], 1);
    await send;
    assert.equal(h.queue.hidden, false);
    assert.equal(displayedUserMessages(context).length, 0);
    context.thread.status = { type: "idle" };
    context.renderMessages();
    assert.equal(displayedUserMessages(context).length, 0, "空闲本身不证明队列消息已发送");
    const accepted = { ...queued, remoteDeliveryState: "accepted" };
    context.applyQueueSnapshot("main", [accepted], [], h.snapshot([accepted], 2));
    assert.equal(displayedUserMessages(context).length, 1, "原生接受证据可确认后续自动发送");
    context.applyQueueSnapshot("main", [], [], h.snapshot([], 3));
    assert.equal(displayedUserMessages(context).length, 1);
    assert.equal(h.actions.length, 1);
  }
});

test("发送成功完整保留图片和技能，原生身份先到正文后到时不闪空", async () => {
  const h = idleSendDisplayHarness("service"), { context } = h;
  const image = { id: "image-1", name: "附件.png", dataUrl: "data:image/png;base64,AA==" };
  context.composerAttachmentsPayload = () => [image];
  context.session.capabilities.imageInput = true;
  context.snapshotDraft = () => ({ text: h.originalText, skills: [{ name: "示例", path: "/skills/example" }], attachments: [image] });
  const send = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  const action = h.actions[0];
  assert.equal(displayedUserMessages(context).length, 0);
  action.resolve({ requestId: action.requestId, state: "completed", result: {
    ok: true, threadId: "main", deliveryState: "accepted", result: { status: "sent", turnId: "native-turn" }
  } });
  await send;
  const confirmed = displayedUserMessages(context)[0];
  assert.equal(confirmed.content.find(part => part.type === "image").url, image.dataUrl);
  assert.equal(confirmed.content.find(part => part.type === "skill").name, "示例");
  context.turns.set("native-turn", { id: "native-turn", items: [{ id: "native-input", type: "userMessage",
    clientId: action.payload.clientUserMessageId, content: [] }] });
  const adopted = displayedUserMessages(context);
  assert.equal(adopted.length, 1);
  assert.equal(adopted[0].id, "native-input");
  assert.equal(adopted[0].content.find(part => part.type === "image").url, image.dataUrl);
  assert.equal(h.actions.length, 1);
});

test("重进成功消息缓存后原生跨轮接替，正文和附件保留且只显示一次", () => {
  const { context } = idleSendDisplayHarness();
  const cached = { id: "cached-input", type: "userMessage", clientId: "cache-client",
    remotePendingInput: true, remoteConfirmedInput: true, remoteDirectSend: true,
    content: [{ type: "text", text: "已成功消息" }, { type: "image", url: "data:image/png;base64,AA==" }] };
  context.turns.set("cached-turn", { id: "cached-turn", status: "completed", items: [cached] });
  context.turns.set("native-turn", { id: "native-turn", status: "completed", items: [
    { id: "native-input", type: "userMessage", clientId: "cache-client", content: [] }
  ] });
  const messages = displayedUserMessages(context);
  assert.equal(messages.length, 1);
  assert.equal(messages[0].id, "native-input");
  assert.equal(messages[0].content.length, 2);
  context.turns.clear();
  assert.equal(displayedUserMessages(context).length, 1, "原生接替后短暂空历史也不能删除已显示消息");
});

test("历史同步空档内连续成功的消息按确认顺序显示，队列登记顺序不能把正文倒排", () => {
  const { context } = idleSendDisplayHarness();
  const first = { threadId: "main", clientUserMessageId: "first", prompt: "先登记后发送" };
  const second = { threadId: "main", clientUserMessageId: "second", prompt: "后登记先发送" };
  context.rememberSendDisplay(first);
  context.rememberSendDisplay(second);
  context.confirmSendDisplay(second, { ok: true, threadId: "main", deliveryState: "accepted" });
  context.confirmSendDisplay(first, { ok: true, threadId: "main", deliveryState: "accepted" });
  assert.deepEqual(Array.from(displayedUserMessages(context), item => item.clientId), ["second", "first"]);
  context.applyQueueSnapshot("main", [], [], { queueEpoch: "order", queueRevision: 1 });
  assert.deepEqual(Array.from(displayedUserMessages(context), item => item.clientId), ["second", "first"]);
});

test("暂停、结果未知和其他设备抢先执行都不能在成功前产生正文气泡", async () => {
  for (const outcome of ["paused", "unknown", "peer-turn"]) {
    const h = idleSendDisplayHarness(), { context } = h;
    const send = context.sendMessage();
    await new Promise(resolve => setImmediate(resolve));
    const action = h.actions[0], queued = h.queued(action);
    if (outcome === "paused") queued.pausedReason = "原生尚未接收";
    if (outcome === "unknown") queued.submission = { status: "outcome-unknown" };
    if (outcome === "peer-turn") context.thread.status = { type: "active" };
    context.applyQueueSnapshot("main", [queued], [], h.snapshot([queued], 1));
    assert.equal(displayedUserMessages(context).length, 0);
    h.completeQueued(action, [queued], 1);
    await send;
    assert.equal(displayedUserMessages(context).length, 0);
    assert.equal(h.queue.hidden, false);
  }
});

test("授权清理后迟到发送回执不展示旧正文，也不删除新授权的展示记录", async () => {
  const h = idleSendDisplayHarness(), { context } = h;
  const send = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  const action = h.actions[0];
  context.clearPrivateInputs();
  assert.equal(context.sentMessageDisplays.size, 0);
  context.session = { ...context.session, device: { id: "device-b", createdAtMs: 2 } };
  const record = context.rememberSendDisplay({ threadId: "main", clientUserMessageId: "new-client", prompt: "新输入" });
  action.resolve({ requestId: action.requestId, state: "completed", result: {
    ok: true, threadId: "main", deliveryState: "accepted"
  } });
  await send;
  assert.equal(context.sentMessageDisplays.size, 1);
  assert.equal([...context.sentMessageDisplays.values()][0], record);
  assert.equal(displayedUserMessages(context).length, 0);
});

test("真实渲染入口过滤未确认占位，已确认正文在后续渲染保留", () => {
  const h = idleSendDisplayHarness(), { context } = h;
  context.turns.set("native-pending", { id: "native-pending", items: [{ id: "pending-user", type: "userMessage",
    remotePendingInput: true, clientId: "pending", content: [{ type: "text", text: "尚未成功" }] }] });
  const originalQuery = context.$, messagesRoot = { scrollHeight: 0, scrollTop: 0 };
  Object.assign(context, {
    messageFrame: null, followMessages: true, composerScroll: null,
    cancelAnimationFrame() {}, scheduleMermaidBlocks() {},
    $: (selector, root) => !root && selector === "#messages" ? messagesRoot : originalQuery(selector, root),
  });
  vm.runInContext(sourceSection("function renderMessages(", "  loadMessageSkillChoices(sorted.flatMap")
    .replace("function renderMessages(", "function projectDisplayTurns(") + "\nreturn sorted;\n}", context);
  assert.equal(context.projectDisplayTurns().flatMap(turn => turn.items).length, 0);
  context.confirmSendDisplay({ threadId: "main", clientUserMessageId: "success", prompt: "已经成功" },
    { ok: true, threadId: "main", deliveryState: "accepted" });
  assert.equal(context.projectDisplayTurns().flatMap(turn => turn.items).length, 1);
  context.clearSentMessageDisplays("main");
  assert.equal(context.projectDisplayTurns().flatMap(turn => turn.items).length, 0, "明确回退可清除成功展示缓存");
});

test("发送网络结果未知保留草稿和原回执，核对成功前没有正文也不重发", async () => {
  const h = idleSendDisplayHarness("service"), { context } = h;
  let receiptChecks = 0;
  context.resolvePending = async () => { receiptChecks++; };
  const send = context.sendMessage();
  await new Promise(resolve => setImmediate(resolve));
  const action = h.actions[0], queued = h.queued(action);
  context.applyQueueSnapshot("main", [queued], [], h.snapshot([queued], 1));
  action.reject(new TypeError("network unavailable"));
  await send;
  assert.equal(context.pending.id, action.requestId);
  assert.equal(context.pending.payload.clientUserMessageId, action.payload.clientUserMessageId);
  assert.equal(h.prompt.value, h.originalText);
  assert.equal(context.drafts.get("main"), h.originalText);
  assert.equal(displayedUserMessages(context).length, 0);
  assert.equal(h.queue.querySelector(".queued-steer-button").disabled, true);
  assert.equal(receiptChecks, 1);
  assert.equal(h.actions.length, 1);
});

test("成功消息沿用普通用户气泡，正文不附加发送中或等待同步标签", () => {
  const element = (tag, className = "", text = "") => ({
    tag, className, text, children: [], classList: { add() {}, remove() {} },
  });
  const context = vm.createContext({
    element, editingMessage: null, selectedId: "main", $: () => null,
    userDisplayContent: item => ({ text: item.content[0].text }),
    setMarkdown: (node, text) => { node.text = text; }, userMessageSkillOptions: () => ({}),
    reconcileChildren: (node, children) => { node.children = children; },
    bindMessageVideos() {}, prepareRemoteTunnelAccess() {},
  });
  vm.runInContext(sourceSection("function renderItem(", '  if (item.type === "agentMessage"')
    + "\nreturn null;\n}", context);
  for (const confirmed of [false, true]) {
    const node = context.renderItem({ id: "input", type: "userMessage", remoteDirectSend: confirmed,
      content: [{ type: "text", text: "成功输入" }] });
    assert.equal(node.className, "message user-message");
    assert.equal(node.children.length, 1);
    assert.equal(node.children[0].text, "成功输入");
  }
});

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
    connectionRecoveryRequest: null, privateInputScope: value => value?.device?.id,
    startConnectionPoll() {}, cancelConnectionProbe() {}, renderConnectionIndicator() {},
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
    sourceSection("function probeConnectionRecovery(", "async function copyText("),
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
    connectionPollPending: null, recoveryErrorSilenceDepth: 0, connectionRecoveryRequest: null,
    renderConnectionIndicator() {},
    navigator: { onLine: true }, initialAuthenticationRequired: false, connecting: null,
    pending: { id: "original-write", state: "unknown", controller: pendingWrite },
    drafts: new Map([["main", "保留普通草稿"]]),
    questionDrafts: new Map([["existing-question", "保留未提交回答"]]),
    window: { addEventListener: (name, callback) => listeners.set(name, callback) },
    $: selector => selector === "#connection-state" ? connectionIndicator : node(selector),
    setConnection: value => { context.connected = value; },
    readConnectionSession: async (_signal, _observation, options) => {
      const probe = { ...deferred(), options }; probes.push(probe);
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
    syncGoalLiveTimer() {}, scheduleRelatedOperationCheck() {}, scheduleRelatedSessionCheck() {}, drainPendingNotificationOpen() {},
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
    sourceSection("function connectionErrorMessage(", "function showConnectionRecovery("),
    sourceSection("async function syncConnectionState(", "function savePending("),
    sourceSection("function withRecoveryErrorSilence(", "async function refreshAll("),
    sourceSection("async function refreshAll(", "function openStream("),
    sourceSection("function openStream(", "function stopConnectionPoll("),
    sourceSection("function stopConnectionPoll(", "function retireForegroundReads("),
    sourceSection("function retireForegroundReads(", "function resumePage()"),
    sourceSection("function resumePage()", "async function connect("),
    sourceSection("function probeConnectionRecovery(", "async function copyText("),
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

test("设置初始化和运行模式刷新共用自动切换提示，未连接及两种运行模式都可打开", () => {
  const cases = [
    [null, "CDP 模式", "", true],
    [{ cdpInjectionEnabled: true, revision: 1 }, "CDP 模式", "", false],
    [{ cdpInjectionEnabled: false, revision: 2 }, "服务模式", "", false],
    [{ cdpInjectionEnabled: false, revision: 3, fallbackReason: "cdp-unavailable" }, "服务模式", "自动切换为服务模式", false],
    [{ cdpInjectionEnabled: true, revision: 4, fallbackReason: "cdp-recovered" }, "CDP 模式", "自动切回 CDP 模式", false],
  ];
  for (const [runtime, mode, message, disabled] of cases) {
    const runtimeSwitch = { setAttribute() {} }, runtimeMode = {};
    const runtimeStatus = { hidden: true, dataset: {} }, dialog = { isConnected: true };
    const context = vm.createContext({
      session: runtime ? { runtime, readOnly: false } : null,
      dialog, runtimeSwitch, runtimeMode, runtimeStatus, connected: true,
      busy: false, stopping: false, pending: null, editingMessage: null, editingQueuedMessage: null,
    });
    vm.runInContext([
      sourceSection("function runtimeAutomaticMessage(", "function applyRemoteRuntime("),
      sourceSection("  let runtimeSettings = session?.runtime || null", "  const readRuntimeSettings = async"),
    ].join("\n"), context);
    assert.equal(runtimeMode.textContent, mode);
    assert.equal(runtimeSwitch.disabled, disabled);
    assert.equal(runtimeStatus.hidden, !message);
    assert.ok(runtimeStatus.textContent.includes(message));
    if (!runtime) continue;
    context.session.runtime = { cdpInjectionEnabled: true, revision: 5, fallbackReason: "cdp-recovered" };
    dialog.updateRemoteRuntimeSwitch();
    assert.equal(runtimeMode.textContent, "CDP 模式");
    assert.match(runtimeStatus.textContent, /自动切回 CDP 模式/);
    context.session.readOnly = true;
    dialog.updateRemoteRuntimeSwitch();
    assert.equal(runtimeSwitch.disabled, true);
  }
});

test("CDP 自动降级和恢复都保留原回执、编辑内容和草稿，只提示一次并拒绝旧模式回填", () => {
  const h = connectionRecoveryHarness(), context = h.context;
  const pending = context.pending, draft = context.drafts.get("main");
  const edit = { threadId: "main", text: "正在编辑的内容" };
  const queuedEdit = { threadId: "main", editId: "native-edit", text: "排队编辑草稿" };
  Object.assign(context, {
    session: { ...context.session, readOnly: true, runtime: { cdpInjectionEnabled: true, revision: 7 },
      capabilities: { serviceSubagents: true, serviceSideThreads: true } },
    relatedSessionView: null, modelsLoadedAt: 1, modelsPromise: null, modelsRetryAt: 0,
    editingMessage: edit, editingQueuedMessage: queuedEdit, dismissSideCreation() {},
  });
  vm.runInContext(sourceSection("function runtimeAutomaticMessage(", "function receive(event)"), context);
  const next = { cdpInjectionEnabled: false, revision: 8, fallbackReason: "cdp-unavailable" };
  context.applyRemoteRuntime(next);
  context.applyRemoteRuntime(next);
  context.applyRemoteRuntime({ cdpInjectionEnabled: true, revision: 7 });
  assert.equal(context.session.runtime.cdpInjectionEnabled, false);
  assert.equal(context.session.runtime.revision, 8);
  assert.equal(context.session.readOnly, true);
  assert.equal(context.pending, pending);
  assert.equal(context.pending.state, "unknown");
  assert.equal(context.editingMessage, edit);
  assert.equal(context.editingQueuedMessage, queuedEdit);
  assert.equal(context.drafts.get("main"), draft);
  assert.equal(h.pendingWrite.signal.aborted, false);
  assert.equal(h.writes.length, 0);
  assert.equal(h.notices.filter(message => message.includes("自动切换为服务模式")).length, 1);
  const recovered = { cdpInjectionEnabled: true, revision: 9, fallbackReason: "cdp-recovered" };
  context.applyRemoteRuntime(recovered);
  context.applyRemoteRuntime(recovered);
  context.applyRemoteRuntime(next);
  assert.equal(context.session.runtime.cdpInjectionEnabled, true);
  assert.equal(context.session.runtime.revision, 9);
  assert.equal(context.pending, pending);
  assert.equal(context.editingMessage, edit);
  assert.equal(context.editingQueuedMessage, queuedEdit);
  assert.equal(context.drafts.get("main"), draft);
  assert.equal(h.pendingWrite.signal.aborted, false);
  assert.equal(h.writes.length, 0);
  assert.equal(h.notices.filter(message => message.includes("自动切回 CDP 模式")).length, 1);
  context.applyRemoteRuntime({ cdpInjectionEnabled: false, revision: 10 });
  assert.equal(context.session.runtime.cdpInjectionEnabled, false, "手动服务模式保持原选择");
  assert.equal(h.notices.length, 2, "手动模式和重复事件不产生自动恢复提示");
});

function transportRecoveryHarness() {
  const h = connectionRecoveryHarness(), context = h.context;
  const calls = [], sockets = [], pending = [], catalogReads = [], refreshes = [], timers = new Map();
  const listeners = new Map([...h.listeners].map(([name, callback]) => [name, new Set([callback])]));
  let now = 1000, sequence = 0, networkReady = false, sessionReply = null;
  const node = () => ({
    textContent: "", children: [], disabled: false, attributes: new Map(), classes: new Map(),
    setAttribute(key, value) { this.attributes.set(key, value); },
    replaceChildren(...children) { this.children = children; },
    classList: { toggle() {} },
  });
  const indicator = node();
  indicator.onclick = context.refreshConnectionFromIndicator;
  const timer = (callback, delay, interval = false) => {
    timers.set(++sequence, { callback, at: now + delay, delay, interval });
    return sequence;
  };
  const json = value => jsonResponse(value);
  Object.assign(context, {
    URL, Headers, Response, TextEncoder, queueMicrotask,
    Date: class extends Date { static now() { return now; } },
    performance: { now: () => now },
    location: new URL("http://recovery.example:39399/"),
    setTimeout: (callback, delay) => timer(callback, delay),
    clearTimeout: id => timers.delete(id),
    setInterval: (callback, delay) => timer(callback, delay, true),
    clearInterval: id => timers.delete(id),
    addEventListener(name, callback) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(callback);
    },
    removeEventListener(name, callback) { listeners.get(name)?.delete(callback); },
    isAndroidContainer: () => false, getAndroidAppInfo: async () => ({}),
    diagnosticLocation: () => ({}), newDiagnosticRequestId: () => `test-${++sequence}`,
    recordDiagnostic() {}, traceDiagnosticRequest: () => ({ response() {}, finish() {} }),
    beginPwaForegroundWork: () => () => {}, requireAuthentication() { assert.fail("网络恢复不能清除授权"); },
    threadDeletionLocked: () => false, threadDeleted: () => false,
    recoveryStore: { observe: async () => ({}) }, sessionRecoveryReads: new WeakMap(),
    sessionRequest: null, connectionRtt: null, connectionLatencyVersion: 0, connectionLatencyFloor: 0,
    channelRtts: new Map(), connectionRttDescription: "点击探测连接并刷新会话",
    $: selector => selector === "#connection-state" ? indicator : null,
    refreshAll: async () => { refreshes.push("read"); },
    loadSidebar: async () => {},
    loadModels: async () => { catalogReads.push("models"); },
    refreshSkillCatalog: async () => { catalogReads.push("skills"); },
    fetch(url, options) {
      const path = new URL(url).pathname;
      calls.push({ path, time: now, method: options.method });
      assert.equal(options.method, "GET", "恢复探测不能发送写请求");
      if (networkReady) {
        if (path === "/api/session") return sessionReply ? sessionReply.promise : Promise.resolve(json(h.snapshot()));
        return Promise.resolve(json({
          ok: true, schemaVersion: 1, deviceId: "device-a",
          serverId: "12345678-1234-1234-1234-123456789abc", token: "a".repeat(64),
          expiresAtMs: now + 3600000, origins: [context.location.origin],
        }));
      }
      const result = deferred(); pending.push({ path, ...result });
      options.signal?.addEventListener("abort", () => result.reject(options.signal.reason), { once: true });
      return result.promise;
    },
    WebSocket: class {
      constructor() { sockets.push({ socket: this, time: now }); }
      addEventListener() {}
      close() {}
    },
  });
  context.window = context;
  context.document.createElement = node;
  context.document.addEventListener = () => {};
  context.document.removeEventListener = () => {};
  vm.runInContext(channels.replace(/^import .*;\r?\n/gm, "").replace(/^export /gm, ""), context);
  vm.runInContext([
    sourceSection("function threadActionReadOnly(", "function threadReadOnlyMessage("),
    sourceSection("async function api(", "async function refreshConnectedSession("),
    sourceSection("function currentChannelMeasurements(", "function scheduleReload("),
  ].join("\n"), context);
  context.createRemoteEventStream = () => context.createChannelEventStream();
  context.sharedEventsAvailable = () => false;
  context.ensureStream = () => {};
  const flush = async () => { for (let turn = 0; turn < 30; turn++) await Promise.resolve(); };
  const advance = async target => {
    for (;;) {
      const next = [...timers].filter(([, item]) => item.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) break;
      const [id, item] = next;
      now = item.at; timers.delete(id);
      if (item.interval) timers.set(id, { ...item, at: now + item.delay });
      item.callback(); await flush();
    }
    now = target; await flush();
  };
  const establishCooldown = async () => {
    context.openStream(); await flush();
    const probe = context.readConnectionSession().catch(error => error);
    await flush();
    await advance(11000);
    const discovery = context.api("/api/channels").catch(error => error);
    await flush();
    await advance(16000); await probe;
    await advance(17000);
    pending.find(item => item.path === "/api/channels").reject(new TypeError("network unavailable"));
    await discovery; await flush();
    assert.equal(sockets.length, 1);
    networkReady = true;
  };
  return { ...h, context, indicator, calls, sockets, catalogReads, refreshes, flush, advance, establishCooldown,
    fire: name => { for (const callback of [...listeners.get(name) || []]) callback({ isTrusted: true }); },
    setNetworkReady: () => { networkReady = true; },
    holdSession: () => (sessionReply = deferred()),
    json,
    dispose: () => { context.stream?.close(); context.stopConnectionPoll(); context.clearRemoteChannels(); timers.clear(); },
  };
}

for (const source of ["online", "manual"])
  test(`${source}通过真实HTTP和事件线路绕过30秒网络退避，成功前仍禁止普通请求`, async () => {
    const h = transportRecoveryHarness(), pending = h.context.pending;
    try {
      await h.establishCooldown();
      const before = h.calls.length, reply = h.holdSession();
      if (source === "online") h.fire("online");
      else void h.indicator.onclick();
      await h.flush();
      assert.equal(h.calls.length, before + 1);
      assert.equal(h.calls.at(-1).path, "/api/session");
      assert.equal(h.calls.at(-1).time, 17000, "不能等到47000毫秒的旧退避截止时间");
      assert.equal(h.indicator.textContent, "正在探测…");
      assert.equal(h.indicator.disabled, true);
      assert.equal(h.indicator.attributes.get("aria-busy"), "true");
      assert.equal(h.catalogReads.length, 0);
      await assert.rejects(h.context.requestRemote("/api/read/thread", { method: "POST" }, r => r.json()),
        error => error.notSubmitted === true);
      const duplicate = h.indicator.onclick();
      h.fire("online"); await h.flush();
      assert.equal(h.calls.length, before + 1, "点击与在线事件合并为一次探测");
      reply.resolve(h.json(h.snapshot())); await duplicate; await h.flush();
      assert.equal(h.sockets.length, 2, "探测成功后事件通道立即重新握手");
      assert.equal(h.sockets[1].time, 17000);
      assert.equal(h.indicator.disabled, false);
      assert.equal(h.context.pending, pending);
      assert.equal(h.pendingWrite.signal.aborted, false);
      assert.equal(h.context.drafts.get("main"), "保留普通草稿");
      assert.equal(h.writes.length, 0);
    } finally { h.dispose(); }
  });

for (const [status, headers] of [[429, {}], [429, { "Retry-After": "60" }], [503, { "Retry-After": "60" }]])
  test(`手动探测保留HTTP ${status}及${headers["Retry-After"] || "默认"}限流等待`, async () => {
    const h = transportRecoveryHarness();
    try {
      h.context.fetch = async () => {
        h.calls.push("limited");
        return jsonResponse({ message: "limited" }, status, headers);
      };
      await assert.rejects(h.context.raceRemoteRead("/api/session", { method: "GET" }, response => {
        if (!response.ok) throw new Error("limited");
        return response.json();
      }, () => {}));
      await h.indicator.onclick();
      assert.equal(h.calls.length, 1, "手动点击不能越过服务端限流");
      assert.match(h.notices.at(-1), /服务暂时限制请求/);
      assert.equal(h.indicator.disabled, false);
    } finally { h.dispose(); }
  });

test("显式点击在浏览器offline标记尚未更新时仍执行只读探测", async () => {
  const h = transportRecoveryHarness();
  try {
    await h.establishCooldown();
    h.context.navigator.onLine = false;
    const before = h.calls.length, reply = h.holdSession(), probe = h.indicator.onclick();
    await h.flush();
    assert.equal(h.calls.length, before + 1);
    assert.equal(h.calls.at(-1).path, "/api/session");
    assert.equal(h.indicator.disabled, true);
    reply.reject(new TypeError("network unavailable"));
    await probe;
    assert.equal(h.indicator.disabled, false);
    assert.equal(h.writes.length, 0);
  } finally { h.dispose(); }
});

test("手动探测失败会释放按钮，下次点击仍能越过网络退避，迟到页面结果不弹提示", async () => {
  const h = transportRecoveryHarness();
  try {
    await h.establishCooldown();
    const first = h.holdSession(), failed = h.indicator.onclick();
    await h.flush();
    first.reject(new TypeError("network unavailable")); await failed;
    assert.equal(h.indicator.disabled, false);
    assert.equal(h.notices.length, 1);
    const count = h.calls.length, second = h.holdSession(), recovered = h.indicator.onclick();
    await h.flush();
    assert.equal(h.calls.length, count + 1);
    h.context.pageLeaving = true;
    second.reject(new TypeError("late failure")); await recovered;
    assert.equal(h.notices.length, 1, "已离开页面不得弹出旧请求错误");
    assert.equal(h.context.pending.id, "original-write");
    assert.equal(h.pendingWrite.signal.aborted, false);
  } finally { h.dispose(); }
});

for (const phase of ["connecting", "retrying", "open"])
  test(`成功探测处理${phase}事件通道，只重新建立过时握手或解除旧退避`, async () => {
    const h = transportRecoveryHarness();
    try {
      h.context.openStream(); await h.flush();
      const original = h.sockets[0].socket;
      if (phase === "retrying") original.onerror();
      if (phase === "open") {
        // 本例只验证传输层与手动探测，避免初次 onopen 同时派发独立会话读取。
        h.context.stream.onopen = () => {};
        original.onopen();
      }
      h.setNetworkReady();
      await h.indicator.onclick(); await h.flush();
      assert.equal(h.sockets.length, phase === "open" ? 1 : 2);
      assert.ok(h.sockets.every(entry => entry.time === 1000), "成功探测后不得等待旧重试或握手超时");
      if (phase === "retrying") {
        h.sockets.at(-1).socket.onopen(); await h.flush();
        await h.advance(2000);
        assert.equal(h.sockets.length, 2, "已恢复的连接不能被外层旧重试计时器再次关闭");
      }
      assert.equal(h.writes.length, 0);
    } finally { h.dispose(); }
  });

test("手动恢复退役旧onopen探测，旧取消错误不覆盖新连接或弹出提示", async () => {
  const h = transportRecoveryHarness();
  try {
    h.context.openStream(); await h.flush();
    h.sockets[0].socket.onopen(); await h.flush();
    assert.equal(h.calls.filter(call => call.path === "/api/session").length, 1);
    h.setNetworkReady();
    await h.indicator.onclick(); await h.flush();
    assert.equal(h.calls.filter(call => call.path === "/api/session").length, 2);
    assert.equal(h.context.connected, true);
    assert.deepEqual(h.notices, []);
    assert.equal(h.indicator.disabled, false);
  } finally { h.dispose(); }
});

test("HTTP探测成功但WebSocket构造失败时保留新退避，不递归重连", async () => {
  const h = transportRecoveryHarness();
  let attempts = 0;
  try {
    h.context.WebSocket = class { constructor() { attempts++; throw new Error("socket unavailable"); } };
    h.context.openStream(); await h.flush();
    assert.equal(attempts, 1);
    h.setNetworkReady();
    await h.indicator.onclick(); await h.flush();
    assert.equal(attempts, 2);
    assert.equal(h.indicator.disabled, false);
    assert.equal(h.context.connected, false);
  } finally { h.dispose(); }
});

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
    unsupportedImages: false, unsupportedProjectless: false, goalReason: "", sessionTypeEnabled: true,
    composerAttachmentsLoading: () => false, action: "send", pending: null,
    prompt: "不应进入日志的私人正文", hasAttachments: false, creating: false,
    selectedId: "main", thread: null, send: {}, composerDiagnosticState: "",
    privateInputScope: () => "device-a",
    composerAttachmentSnapshot: () => [],
    recordRuntimeDiagnostic: (name, fields) => events.push({ name, fields }),
  };
  vm.createContext(context);
  const update = vm.runInContext(`() => {${sourceSection(
    "  const blockedBy =", '  $("#prompt").disabled =')}}`, context);
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
  context.isClaude = () => true;
  context.active = false; context.modelsLoadedAt = 0;
  context.currentThreadDisplay = () => context.thread;
  update();
  assert.equal(events.at(-1).fields.blockedBy, "claude-models");
  context.thread.model = "history-model";
  update();
  assert.equal(context.send.disabled, false, "历史已知当前模型时，完整目录尚未加载也可续发");
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
    tokenUsageVersion: 0, statusVersion: 0, questionVersion: 0, goalVersion: 0, compactionVersion: 0, queueVersion: 0, messageVersion: 0,
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

test("PC 打开侧边会话交给右侧标签，不切换主会话、不改主草稿和历史", async () => {
  const h = sideCreationNavigationHarness(), opened = [];
  const beforeHistory = JSON.stringify(h.historyEntries), beforeDraft = h.prompt.value;
  h.context.sessionTools = { openSideThread: item => { opened.push(item); return true; } };
  let ready = false;
  await h.context.openRelatedThread({ id: "side-one", title: "侧边一" }, "side", "main",
    { onViewReady: () => { ready = true; } });
  assert.equal(opened.length, 1);
  assert.equal(opened[0].id, "side-one");
  assert.equal(h.context.selectedId, "main");
  assert.equal(h.context.relatedSessionView, null);
  assert.equal(h.prompt.value, beforeDraft);
  assert.equal(JSON.stringify(h.historyEntries), beforeHistory);
  assert.equal(ready, true);
});

test("隐藏的侧边标签即使请求打开确认也不标记已读，显示后沿用原确认流程", async () => {
  const calls = [], document = { hidden: false, documentElement: { dataset: { workspaceSidePanelVisible: "false" } } };
  const context = nodeVm.createContext({
    document, selectedId: "side-a", thread: { id: "side-a", hasUnreadTurn: true }, connected: true,
    historyInitialized: true, historyError: "", transcriptChecking: false, followMessages: true,
    threadActionReadOnly: () => false, threadDeletionLocked: () => false, threadDeleted: () => false,
    readMarkRequests: new Map(), recentPrioritySelectionSuppressions: new Set(), sidebarReadRefreshSuppressions: new Set(),
    session: { device: { id: "fixture-device" } }, messageVersion: 1, setTimeout() {},
    mutate: async (...args) => calls.push(args), sessionMenu: { markSideThreadRead() {} },
    markSidebarThreadReadLocally() {}, syncSideThreadUnread() {}, renderSidebar() {},
  });
  nodeVm.runInContext(sourceSection("async function markVisibleThreadRead(", "function beginMessageEdit("), context);
  await context.markVisibleThreadRead("side-a", { opened: true });
  assert.equal(calls.length, 0);
  assert.equal(context.thread.hasUnreadTurn, true);
  document.documentElement.dataset.workspaceSidePanelVisible = "true";
  await context.markVisibleThreadRead("side-a");
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "markRead");
  assert.equal(context.thread.hasUnreadTurn, false);
});

test("侧边页面只接收同源父页面的可见状态，其他来源不能触发已读", async () => {
  const parent = {}, handlers = new Map(), marked = [], document = { documentElement: { dataset: {} } };
  const context = nodeVm.createContext({
    workspaceSidePanel: true, window: { parent, addEventListener: (name, fn) => handlers.set(name, fn) },
    location: { origin: "http://127.0.0.1:45222" }, document,
    markVisibleThreadRead: async () => marked.push(true),
  });
  nodeVm.runInContext(sourceSection('window.addEventListener("message", event => {\n  if (!workspaceSidePanel',
    'window.addEventListener("storage", event => {'), context);
  const listener = handlers.get("message"), data = { type: "codex-elves-side-panel-visibility", visible: true };
  listener({ source: {}, origin: context.location.origin, data });
  listener({ source: parent, origin: "https://other.example", data });
  assert.equal(marked.length, 0);
  listener({ source: parent, origin: context.location.origin, data });
  assert.equal(marked.length, 1);
  assert.equal(document.documentElement.dataset.workspaceSidePanelVisible, "true");
  listener({ source: parent, origin: context.location.origin, data: { ...data, visible: false } });
  assert.equal(marked.length, 1);
  assert.equal(document.documentElement.dataset.workspaceSidePanelVisible, "false");
});

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

function approvalAreaHarness(preceding = [], viewport = { offsetTop: 0, height: 900 }, question = true) {
  const styles = new Map(), anchor = { previousElementSibling: null,
    getBoundingClientRect: () => ({ top: 735 }) };
  for (const box of preceding) {
    anchor.previousElementSibling = {
      hidden: box.hidden === true, previousElementSibling: anchor.previousElementSibling,
      getBoundingClientRect: () => box,
    };
  }
  const root = { parentElement: anchor, childElementCount: 1, questionCard: question,
    querySelector: selector => selector === ".question-card" && root.questionCard ? {} : null,
    style: { setProperty: (key, value) => styles.set(key, value) } };
  const context = { $: () => root, window: { visualViewport: viewport, innerHeight: 900 } };
  vm.createContext(context);
  vm.runInContext(sourceSection("function fitApprovalArea()", "function refitComposerMenus("), context);
  return { context, root, anchor, styles, fit: () => context.fitApprovalArea(),
    bottom: () => anchor.getBoundingClientRect().top - 8 - parseFloat(styles.get("--approval-lift")) };
}

test("问答卡紧贴输入栏上沿并覆盖队列，不再为队列菜单抬升", () => {
  // 用户新要求参考原生问答：输入栏 top=735，卡片保留 8px 间隔，允许叠在队列 top=701 上方。
  const queueContext = { top: 701, width: 760, height: 36 };
  const h = approvalAreaHarness([queueContext]);
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "0px");
  assert.equal(h.styles.get("--approval-available-height"), "719px");
  assert.equal(h.bottom(), 727);
  assert.ok(h.bottom() > queueContext.top, "问答弹层覆盖排队区域，不能抬到队列上方");
});

test("问答卡不因目标、pending 条或项目选择抬升，普通权限审批仍避让", () => {
  for (const preceding of [
    [{ top: 560, width: 760, height: 100 }, { top: 660, width: 760, height: 30 }],
    [{ top: 640, width: 760, height: 40, childElementCount: 0 }],
    [{ top: 610, width: 760, height: 80 }],
  ]) {
    const h = approvalAreaHarness(preceding);
    h.fit();
    assert.equal(h.bottom(), 727);
    assert.equal(h.styles.get("--approval-available-height"), "719px");
    h.root.questionCard = false; h.fit();
    assert.equal(h.bottom(), preceding[0].top - 8);
    assert.equal(h.styles.get("--approval-available-height"), `${preceding[0].top - 16}px`);
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

test("权限审批切换为问答后清理旧抬升，队列目标展开或收起都贴住输入栏", () => {
  const content = { top: 701, width: 760, height: 36 }, h = approvalAreaHarness([content], undefined, false);
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "34px");
  content.top = 560; content.height = 177;
  h.fit();
  assert.equal(h.styles.get("--approval-lift"), "175px");
  h.root.questionCard = true; h.fit();
  assert.equal(h.styles.get("--approval-lift"), "0px");
  assert.equal(h.bottom(), 727);
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
  h.anchor.getBoundingClientRect = () => ({ top: 208 });
  h.fit();
  assert.equal(h.styles.get("--approval-available-height"), "0px");
  assert.equal(h.bottom(), 200);
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
    sentMessageDisplays: new Map(), reconcileSentMessageDisplays: () => false,
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

function cachedControlsHarness(existing = null) {
  const { context, persisted } = threadDisplayCacheHarness(existing);
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
    sourceSection("function queueMessageIdentity(", "function applyQueueSnapshot("),
    sourceSection("function queueReorderAvailable()", "function applyQueueReorderResult("),
    sourceSection("function renderQueuedMessages(", "async function queuedEditFingerprint("),
    sourceSection("function isActiveThread()", "function renderCompactionBanner("),
    sourceSection("const goalStatusLabels =", "function formatGoalTokens("),
    sourceSection("function renderGoalStatus()", "function blockingRequestState("),
  ].join("\n"), context);
  const display = { id: "main", model: "test-model", ephemeral: false,
    goal: { objective: "已有目标立即显示", status: "active", createdAt: 1 },
    queuedMessages: [{ id: "queued-a", text: "已有排队消息", clientUserMessageId: "client-a", createdAt: 1 }] };
  const render = () => { context.renderGoalStatus(); context.renderQueueArea(); };
  return { context, persisted, goal, queue, display, render };
}

test("队列内容不变时，运行状态切换仍立即刷新重试和引导按钮", () => {
  const { context, queue, display, render } = cachedControlsHarness();
  display.queuedMessages[0].pausedReason = "消息已暂停";
  display.queuedMessages[0].submission = { status: "queued" };
  context.rememberThreadDisplay(display);
  context.thread = { ...display, status: { type: "idle" } };
  const action = () => queue.querySelector(".queued-steer-button");
  render();
  assert.equal(action().children[1].text, "重试");
  const initial = queue.firstElementChild;
  render();
  assert.equal(queue.firstElementChild, initial, "状态未变时仍复用已有队列区域");
  context.thread.status = { type: "active" };
  render();
  assert.notEqual(queue.firstElementChild, initial);
  assert.equal(action().children[1].text, "引导");
  assert.equal(action().children[0].className, "steer");
  context.thread.status = { type: "idle" };
  render();
  assert.equal(action().children[1].text, "重试");
  assert.equal(action().children[0].className, "retry");
  assert.equal(context.thread.queuedMessages[0].pausedReason, "消息已暂停");
});

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
  await h.advance(9000);
  assert.equal(h.area.children.length, 0);
  assert.equal(record.draft.text, "保留草稿");
  assert.deepEqual(h.notices, []);
});

test("队列未确认操作到八秒才提示，普通发送和目标仍为五秒，原草稿与回执锁保持", async () => {
  for (const [action, delayMs] of [
    ["send", 5000], ["setGoal", 5000],
    ["steerQueued", 8000], ["removeQueued", 8000], ["reorderQueued", 8000],
    ["beginQueuedEdit", 8000], ["editQueued", 8000], ["cancelQueuedEdit", 8000], ["dismissQueuedEdit", 8000],
  ]) {
    const h = delayFeedbackHarness(), record = h.pending(action);
    await h.advance(4999);
    assert.equal(h.area.children.length, 0, action);
    await h.advance(1);
    if (delayMs === 8000) {
      assert.equal(h.area.children.length, 0, `${action} 五秒时不显示提示`);
      await h.advance(2999);
      assert.equal(h.area.children.length, 0, `${action} 八秒前不显示提示`);
      await h.advance(1);
    }
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
    await h.advance(8000);
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

test("压缩完成的回执和实时事件静默解除锁定，失败仍给出错误提示", async () => {
  const h = delayFeedbackHarness();
  h.context.setCompactionState = (id, value) => { h.context.compactions.set(id, value); return value; };
  h.context.reconcileCompaction = (id, value) => h.context.setCompactionState(id, value);
  h.context.compactions.set("main", { state: "pending", receiptId: "compact-receipt", time: h.now() });
  await h.context.applyCompactionReceipt("main", "compact-receipt", {
    state: "completed", result: { compaction: { state: "idle", turnId: "turn", itemId: "compact" } },
  });
  assert.equal(h.context.compactionLocked("main"), false);
  assert.deepEqual(h.notices, []);
  vm.runInContext(sourceSection("function compactionEventMatches(", "function scheduleSidebarRefresh("), h.context);
  for (const source of ["manual", "automatic"]) {
    h.context.compactions.set("main", { state: "running", turnId: "turn", itemId: "compact" });
    h.context.receiveCompactionLifecycle({ method: "item/completed" }, {
      threadId: "main", turnId: "turn",
      item: { type: "contextCompaction", id: "compact", source, completed: true, status: "completed" },
    });
    assert.equal(h.context.compactionLocked("main"), false);
    assert.deepEqual(h.notices, []);
  }
  h.context.compactions.set("main", { state: "running", turnId: "turn", itemId: "compact" });
  h.context.receiveCompactionLifecycle({ method: "turn/completed" }, {
    threadId: "main", turn: { id: "turn", status: "completed", items: [] },
  });
  assert.equal(h.context.compactionLocked("main"), false);
  assert.deepEqual(h.notices, []);
  h.context.compactions.set("main", { state: "running", turnId: "turn", itemId: "compact" });
  h.context.receiveCompactionLifecycle({ method: "item/completed" }, {
    threadId: "main", turnId: "turn",
    item: { type: "contextCompaction", id: "compact", completed: true, status: "failed" },
  });
  assert.deepEqual(h.notices, [{ message: "上下文压缩失败，请查看原生会话。", error: true }]);
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

function nativeReasoningEffortHarness({ featureEnabled = true, visibleEfforts = ["low", "medium", "high", "xhigh", "max"],
  unavailable = false, rawUltra = true } = {}) {
  const feature = {}, visibility = {}, diagnostics = [];
  const state = { featureEnabled, visibleEfforts, unavailable };
  const levels = ["minimal", "low", "medium", "high", "xhigh", "max", ...(rawUltra ? ["ultra"] : [])];
  const raw = [{ id: "gpt-6.1-sol", model: "gpt-6.1-sol", defaultReasoningEffort: "xhigh",
    supportedReasoningEfforts: levels.map(reasoningEffort => ({ reasoningEffort })) },
  { model: "hidden-model", hidden: true, supportedReasoningEfforts: [] }];
  // 使用与当前 Codex 一致的查询结构，并覆盖打包器将导入符号压缩成含 $ 标识符的情况。
  const record = { source: `modelList(e,t,n){var $flags=n("flags"),$visibility=n("visibility");
    let query=({enabledReasoningEfforts:$efforts,includeUltraReasoningEffort:$ultra},{get:$get})=>{
      let enabled=null==$efforts?$get($visibility.enabled):new Set([...$efforts,"persistent"]),
        ultra=$ultra&&$get($flags.feature,"native-ultra-gate");
      return {refetchOnWindowFocus:!0,queryFn:()=>client.sendRequest("model/list",{})};};}` };
  const context = {
    Set, state, raw, diagnostics, appScope: { get(value, key) {
      if (value === visibility) return new Set(state.visibleEfforts);
      assert.equal(value, feature); assert.equal(key, "native-ultra-gate");
      return state.featureEnabled;
    } },
    nativeRspackRuntime: { runtime: { c: {
      flags: { exports: { feature } }, visibility: { exports: { enabled: visibility } },
    } } },
    nativeModuleSnapshot: async () => {
      if (state.unavailable) throw new Error("原生配置不可用");
      return { rspack: true, records: [record] };
    },
    manager: { requestClient: { sendRequest: async (method, payload) => {
      assert.equal(method, "model/list"); assert.equal(payload.limit, 100);
      return { data: raw, nextCursor: null };
    } } },
    array: value => Array.isArray(value) ? value : [],
    failure: (code, message) => Object.assign(new Error(message), { code }),
    reportNativeCompatibilityDiagnostic: (...args) => diagnostics.push(args),
    discover: () => assert.fail("已有原生配置上下文时不应重新发现"),
  };
  const section = (start, end) => {
    const from = native.indexOf(start), to = native.indexOf(end, from);
    assert.ok(from >= 0 && to > from); return native.slice(from, to);
  };
  vm.createContext(context);
  vm.runInContext([
    section("async function nativeReasoningEffortSettings(", "async function defaultModelSettings("),
    section("function nativeRuntimeRecord(", "function nativeRuntimeScope("),
  ].join("\n"), context);
  return { context, state, raw, diagnostics,
    efforts: model => Array.from(model.supportedReasoningEfforts, effort => effort.reasoningEffort) };
}

test("注入模型目录遵循 Codex 可见等级，原始目录中的 Ultra 不能绕过配置或发送校验", async () => {
  const h = nativeReasoningEffortHarness();
  const models = await h.context.nativeModelCatalog();
  assert.equal(models.length, 1);
  assert.deepEqual(h.efforts(models[0]), ["low", "medium", "high", "xhigh", "max"]);
  assert.equal(models[0].defaultReasoningEffort, "xhigh");
  assert.equal(h.efforts(h.raw[0]).includes("ultra"), true);
  assert.equal(h.diagnostics.length, 0);
  await assert.rejects(h.context.validateReasoningEffort("gpt-6.1-sol", "ultra"), { code: 400 });
  await h.context.validateReasoningEffort("gpt-6.1-sol", "xhigh");
});

test("注入的 Ultra 只有在 Codex 开关和可见等级同时允许时展示，并在配置变化后重新核对", async () => {
  const h = nativeReasoningEffortHarness({ featureEnabled: false, visibleEfforts: ["xhigh", "ultra"] });
  assert.deepEqual(h.efforts((await h.context.nativeModelCatalog())[0]), ["xhigh"]);
  h.state.featureEnabled = true;
  assert.deepEqual(h.efforts((await h.context.nativeModelCatalog())[0]), ["xhigh", "ultra"]);
  await h.context.validateReasoningEffort("gpt-6.1-sol", "ultra");
  h.state.visibleEfforts = ["xhigh"];
  assert.deepEqual(h.efforts((await h.context.nativeModelCatalog())[0]), ["xhigh"]);
  await assert.rejects(h.context.validateReasoningEffort("gpt-6.1-sol", "ultra"), { code: 400 });
});

test("注入配置无法读取时保留其它等级且不开放 Ultra，原生未声明的 Ultra 也不补齐", async () => {
  const unavailable = nativeReasoningEffortHarness({ unavailable: true });
  assert.equal(unavailable.efforts((await unavailable.context.nativeModelCatalog())[0]).includes("xhigh"), true);
  assert.equal(unavailable.efforts((await unavailable.context.nativeModelCatalog())[0]).includes("ultra"), false);
  assert.ok(unavailable.diagnostics.length > 0);
  const missing = nativeReasoningEffortHarness({ rawUltra: false, visibleEfforts: ["xhigh", "ultra"] });
  assert.deepEqual(missing.efforts((await missing.context.nativeModelCatalog())[0]), ["xhigh"]);
});

test("模型列表显式关闭 Ultra 时网页不展示该等级，并保存带版本的目录缓存", async () => {
  const writes = [];
  const context = {
    models: [], modelsPromise: null, modelsLoadedAt: 0, modelsRetryAt: 0,
    modelDefaults: {}, modelCatalogCacheVersion: 1, catalogFallbackMs: 10000,
    session: { device: { id: "device-a" } }, privateInputScope: () => "device-a",
    cacheDisplayData: (...args) => writes.push(args),
    catalogUsable: () => false, scheduleCatalogRefresh() {}, updateComposer() {}, $: () => ({ hidden: true }),
    api: async path => {
      assert.equal(path, "/api/read/models");
      return { defaultModel: "gpt-6.1-sol", defaultReasoningEffort: "xhigh", ultraReasoningEnabled: false,
        data: [{ model: "gpt-6.1-sol", supportedReasoningEfforts: [
          { reasoningEffort: "xhigh" }, { reasoningEffort: "max" }, { reasoningEffort: "ultra" },
        ] }] };
    },
  };
  vm.createContext(context);
  vm.runInContext(sourceSection("async function loadModels(", "function rememberedNewChatModel("), context);
  const models = await context.loadModels(true);
  assert.deepEqual(Array.from(models[0].supportedReasoningEfforts, effort => effort.reasoningEffort), ["xhigh", "max"]);
  assert.equal(writes[0][2].version, 1);
  assert.equal(context.modelDefaults.reasoningEffort, "xhigh");
});

test("升级后的远程页面拒绝恢复未按 Codex 配置过滤的旧模型目录缓存", async () => {
  for (const version of [undefined, 1]) {
    const modelCatalog = { version, loadedAt: Date.now(), models: [
      { model: "gpt-6.1-sol", supportedReasoningEfforts: [{ reasoningEffort: "xhigh" }] },
    ] };
    const context = {
      URLSearchParams, location: { hash: "" }, session: { device: { id: "device-a" } },
      readScope: null, readGeneration: 0, sidebar: null, privateInputScope: () => "device-a",
      models: [], modelsLoadedAt: 0, modelDefaults: {}, modelCatalogCacheVersion: 1,
      readCache: { activate: async () => {}, get: async (_scope, kind) => kind === "models" ? modelCatalog : null },
      optionalCachedRead: value => value, catalogUsable: () => true,
    };
    vm.createContext(context);
    vm.runInContext(sourceSection("async function restoreReadViews(", "function clearPrivateInputs("), context);
    await context.restoreReadViews(context.session);
    assert.equal(context.models.length, version === 1 ? 1 : 0);
  }
});

function initialPageHarness({ cdp = false, readOnly = false, suppress = false } = {}) {
  const h = sideCreationNavigationHarness(), sidebarRead = deferred();
  const latest = { device: { id: "initial-device", createdAtMs: 1 }, readOnly,
    capabilities: { projectlessCreation: true }, runtime: { cdpInjectionEnabled: cdp, revision: 1 } };
  Object.assign(h.context, {
    session: null, selectedId: null, thread: null, sidebar: null, creating: false,
    recoveryGeneration: 0, recoveryStore: { observe: async () => null },
    sessionRecoveryReads: new WeakMap(), restorePrivateInputs: async () => {},
    restoreReadViews: async () => {}, setAuthenticationRequired() {}, restoreSmallInputRecoveries() {},
    postPlatformMessage() {}, restoreGoalComposerDrafts() {}, setConnection() {}, openStream() {},
    startConnectionPoll() {}, recoverQuestionReceipts() {}, resolvePending() {}, drainPendingNotificationOpen() {},
    readConnectionSession: async () => latest, suppressAutomaticNewChat: suppress,
    refreshRequest: null, foregroundReadGeneration: 0, clearComposerAttachments() {},
    loadSidebar: () => sidebarRead.promise,
    shell() {
      h.context.$("#thread-title").textContent = "远程会话";
      h.context.$("#project-select-slot").hidden = true;
      h.context.updateComposer();
    },
    updateComposer() { h.prompt.disabled = readOnly || !h.context.creating && !h.context.selectedId; },
    welcomeNode: () => ({ textContent: "欢迎页" }),
  });
  h.context.location.hash = "";
  vm.runInContext([
    sourceSection("async function refreshAll(", "function openStream("),
    sourceSection("async function connectPage(", "function probeConnectionRecovery("),
  ].join("\n"), h.context);
  return { ...h, sidebarRead };
}

test("首次连接及恢复新建页时，慢侧栏不能阻塞标题、项目区、输入和已有草稿", async () => {
  for (const cdp of [false, true]) {
    const h = initialPageHarness({ cdp });
    h.context.drafts.set("$new:", "尚未发送的新建草稿");
    const connection = h.context.connectPage();
    await h.settle();
    assert.equal(h.context.creating, true);
    assert.equal(h.context.$("#thread-title").textContent, "新建会话");
    assert.equal(h.context.$("#project-select-slot").hidden, false);
    assert.equal(h.prompt.disabled, false);
    assert.equal(h.prompt.value, "尚未发送的新建草稿");
    h.context.selectedId = "newer-selection";
    h.context.creating = false;
    h.context.$("#thread-title").textContent = "用户后来选择的会话";
    h.sidebarRead.resolve();
    await connection;
    assert.equal(h.context.selectedId, "newer-selection");
    assert.equal(h.context.$("#thread-title").textContent, "用户后来选择的会话");
    assert.equal(h.writes.length, 0, "初始化只能恢复页面，不能实际新建或发送会话");
  }
});

test("初始只读设备及明确禁止自动新建时保留欢迎页约束", async () => {
  for (const options of [{ readOnly: true }, { suppress: true }]) {
    const h = initialPageHarness(options);
    const connection = h.context.connectPage();
    await h.settle();
    assert.equal(h.context.creating, false);
    assert.equal(h.context.$("#project-select-slot").hidden, true);
    assert.equal(h.prompt.disabled, true);
    h.sidebarRead.resolve();
    await connection;
    assert.equal(h.writes.length, 0);
  }
});

test("切换运行方式后，旧模型读取晚到不能覆盖新后端目录", async () => {
  const reads = [];
  const context = {
    session: { device: { id: "model-device" }, runtime: { cdpInjectionEnabled: true, revision: 1 } },
    models: [], modelsPromise: null, modelsLoadedAt: 0, modelsRetryAt: 0, modelDefaults: {},
    modelCatalogCacheVersion: 1, catalogFallbackMs: 1000, catalogRetryMs: 1000,
    privateInputScope: () => "model-device", catalogUsable: () => false,
    api: (_path, payload) => { const response = deferred(); reads.push({ response, payload }); return response.promise; },
    cacheDisplayData() {}, updateComposer() {}, scheduleCatalogRefresh() {}, $: () => null,
  };
  vm.createContext(context);
  vm.runInContext(sourceSection("async function loadModels(", "function rememberedNewChatModel("), context);
  const oldRead = context.loadModels(true);
  context.session.runtime = { cdpInjectionEnabled: false, revision: 2 };
  context.modelsPromise = null;
  const newRead = context.loadModels(true);
  reads[1].response.resolve({ data: [{ model: "service-model" }], defaultModel: "service-model" });
  await newRead;
  reads[0].response.resolve({ data: [{ model: "old-cdp-model" }], defaultModel: "old-cdp-model" });
  await oldRead;
  assert.equal(context.models[0].model, "service-model");
  assert.equal(context.modelDefaults.model, "service-model");
  assert.equal(reads[1].payload.forceReload, true);
});

test("Claude 会话图标使用品牌色，间距仅由列表 gap 提供", async () => {
  const css = await readFile(new URL("../assets/remote-access/style.css", import.meta.url), "utf8");
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter(([, selectors]) => selectors.split(",").some(selector => selector.trim() === ".thread-link>.claude-session-icon"))
    .map(([, , declarations]) => declarations).join(";");
  assert.match(rules, /(?:^|;)\s*color\s*:\s*#d97757\s*(?:;|$)/);
  assert.doesNotMatch(rules, /\bmargin(?:-right)?\s*:/);
});

test("Claude 类型跟随已有会话身份，新建草稿和能力与 Codex 分开", () => {
  const context = {
    creating: true, creationBackend: "claude", creationProject: "shared-project",
    session: { capabilities: { claudeSessions: true, goalComposer: true },
      claudeAllowApprovals: true, runtime: { cdpInjectionEnabled: true } },
  };
  vm.createContext(context);
  vm.runInContext(sourceSection("function subagentsEnabled(", "function threadReadOnly("), context);
  assert.equal(context.activeBackend(), "claude");
  assert.equal(context.modelContext().projectId, "shared-project");
  assert.equal(context.subagentsEnabled(), false);
  assert.equal(context.sideThreadsEnabled(), false);
  assert.notEqual(context.newDraftKey("shared-project", "claude"), context.newDraftKey("shared-project", "codex"));
  context.creating = false;
  context.creationBackend = "codex";
  context.selectedId = "claude-01234567-89ab-4cde-8fab-0123456789ab";
  assert.equal(context.activeBackend(), "claude");
  assert.equal(context.approvalsAllowed(), true);
  context.selectedId = "codex-existing";
  assert.equal(context.activeBackend(), "codex");
});

test("Claude 原生队列允许执行中继续发送，仅提交期间防重复", () => {
  const h = queuedEditFeedbackHarness(), context = h.context;
  Object.assign(context, {
    selectedId: "claude-01234567-89ab-4cde-8fab-0123456789ab",
    thread: { id: "claude-01234567-89ab-4cde-8fab-0123456789ab",
      backend: "claude", capabilities: { queue: true } },
    active: true, prompt: "下一条消息", editing: false, editingQueuedMessage: null,
    pending: null, busy: false, stopping: false, modelsLoadedAt: 1,
  });
  context.renderSendFeedback();
  assert.equal(h.send.dataset.action, "send");
  assert.equal(h.send.disabled, false);
  context.busy = true;
  context.renderSendFeedback();
  assert.equal(h.send.disabled, true);
  context.busy = false;
  context.renderSendFeedback();
  assert.equal(h.send.disabled, false);
});

test("Claude 实时历史投影接收 CLI 消息，重复快照不产生重复消息", () => {
  const context = vm.createContext({
    selectedId: "claude-live", thread: { id: "claude-live", backend: "claude" },
    turns: new Map(), turnVersions: new Map(), itemVersions: new Map(), partialItems: new Set(),
    messageVersion: 0, statusVersion: 0, queueVersion: 0,
    normalizeDisplayedTurnItems: value => value, enrichTurnTranscriptEvents: value => value,
    rememberThreadDisplay() {}, rememberTranscript() {}, reconcileSentMessageDisplays() {},
    updateComposer() {}, scheduleMessages() {}, scheduleSidebarRefresh() {},
    markVisibleThreadRead: async () => {},
  });
  vm.runInContext(sourceSection("function receiveClaudeHistory(", "function receive(event)"), context);
  const snapshot = { threadId: "claude-live",
    thread: { id: "claude-live", backend: "claude", status: { type: "active" },
      capabilities: { queue: true }, queuedMessages: [{ id: "web-next", text: "网页补充" }],
      queuedEdits: [{ editId: "edit-active", messageId: "web-edit", state: "editing" }] },
    turns: [{ id: "native-cli-user", status: "inProgress", items: [
      { id: "native-cli-user", type: "userMessage", content: [{ type: "text", text: "CLI 发出的消息" }] },
      { id: "native-reply", type: "agentMessage", text: "正在回复" },
    ] }] };
  context.receiveClaudeHistory(snapshot);
  context.receiveClaudeHistory(structuredClone(snapshot));
  assert.equal(context.turns.size, 1);
  assert.equal(context.turns.get("native-cli-user").items.length, 2);
  assert.equal(context.turns.get("native-cli-user").items[0].content[0].text, "CLI 发出的消息");
  assert.equal(context.thread.queuedMessages[0].id, "web-next");
  assert.equal(context.notifiedQueueEdits[0].editId, "edit-active", "实时快照必须保留队列编辑，供迟到读取合并");
  context.receiveClaudeHistory({ ...snapshot, threadId: "another" });
  assert.equal(context.turns.size, 1);
});

test("Claude 终端预览只更新当前临时气泡，迟到或缩短的片段不能覆盖正式正文", () => {
  let renders = 0, cached = 0, sidebarReads = 0;
  const context = vm.createContext({
    selectedId: "claude-preview", selectionVersion: 1, thread: { id: "claude-preview", backend: "claude" },
    turns: new Map(), turnVersions: new Map(), itemVersions: new Map(), partialItems: new Set(),
    messageVersion: 0, statusVersion: 0, queueVersion: 0,
    normalizeDisplayedTurnItems: value => value, enrichTurnTranscriptEvents: value => value,
    rememberThreadDisplay() {}, rememberTranscript() { cached++; }, reconcileSentMessageDisplays() {},
    updateComposer() {}, scheduleMessages() { renders++; }, scheduleSidebarRefresh() { sidebarReads++; },
    markVisibleThreadRead: async () => {},
  });
  vm.runInContext(sourceSection("function receiveClaudeHistory(", "function receive(event)"), context);
  const canonical = { id: "turn", _nativeUserId: "native-user", _process: "generation",
    _nativePreviewBasis: "b".repeat(64), status: "inProgress", items: [
      { id: "user", type: "userMessage", text: "question" },
      { id: "official", type: "agentMessage", text: "正式 **Markdown** 正文" },
    ] };
  const event = { threadId: "claude-preview", thread: { id: "claude-preview", status: { type: "active" },
    queuedMessages: [], queuedEdits: [] }, turns: [canonical], turnIds: ["turn"] };
  context.receiveClaudeHistory(event);
  const previews = (basis, text) => context.receiveClaudeHistory({ ...event, preview: true,
    turns: [{ ...canonical, _nativePreviewBasis: basis, items: [
      { id: "user", type: "userMessage", text: "迟到副本不能覆盖的旧内容" },
      { id: "native-live-native-user", type: "agentMessage", text, status: "inProgress", _nativePreview: true },
    ] }] });
  previews("a".repeat(64), "旧的终端片段");
  assert.equal(context.turns.get("turn").items.length, 2);
  assert.equal(context.turns.get("turn").items[1].id, "official", "迟到预览不能把正式回复替换掉");
  assert.equal(renders, 1, "无效预览不触发页面重绘");
  previews("b".repeat(64), "下一段回复");
  assert.equal(context.turns.get("turn").items[0].text, "question");
  assert.equal(context.turns.get("turn").items[1].text, "正式 **Markdown** 正文");
  assert.equal(context.turns.get("turn").items[2].text, "下一段回复");
  assert.equal(context.itemVersions.get("turn:official"), 1, "预览不能把旧正式正文标成比 HTTP 读取更新");
  previews("b".repeat(64), "下一段");
  assert.equal(context.turns.get("turn").items[2].text, "下一段回复");
  assert.equal(cached, 1, "终端预览不写正式正文缓存");
  context.receiveClaudeHistory({ ...event, turns: [{ ...canonical, _nativePreviewBasis: "c".repeat(64),
    items: [...canonical.items, { id: "official-next", type: "agentMessage", text: "下一段**回复**" }] }] });
  previews("b".repeat(64), "下一段回复的迟到片段");
  assert.deepEqual(Array.from(context.turns.get("turn").items, item => item.id),
    ["user", "official", "official-next"]);
  const before = sidebarReads;
  context.receiveClaudeHistory({ ...event, threadId: "another", preview: true });
  assert.equal(sidebarReads, before, "其他会话的预览不触发重复侧栏读取");
});

test("Claude HTTP 完整历史接替预览且迟到读取不能复活旧预览", () => {
  const context = vm.createContext({
    turns: new Map(), itemVersions: new Map(), turnVersions: new Map(), turnFieldVersions: new Map(),
    partialItems: new Set(), enrichTurnTranscriptEvents: value => value,
  });
  vm.runInContext(sourceSection("function mergeLiveText(", "async function reloadThread("), context);
  const base = { id: "turn", _nativeUserId: "native-user", _process: "generation",
    _nativePreviewBasis: "a".repeat(64), status: "inProgress", itemsComplete: true, itemsView: "full",
    items: [{ id: "user", type: "userMessage", text: "question" }] };
  const preview = { id: "native-live-native-user", type: "agentMessage", text: "partial reply",
    status: "inProgress", _nativePreview: true };
  context.turns.set("turn", { ...base, items: [...base.items, preview] });
  context.itemVersions.set("turn:native-live-native-user", 2);
  const metadata = context.mergeHistoryTurn(base, 1);
  assert.equal(metadata.items.at(-1).text, preview.text, "正文基线未变时保留已显示预览");
  const official = { ...base, _nativePreviewBasis: "b".repeat(64), items: [
    ...base.items, { id: "official", type: "agentMessage", text: "partial **reply**", status: "completed" },
  ] };
  const replaced = context.mergeHistoryTurn(official, 1);
  assert.deepEqual(Array.from(replaced.items, item => item.id), ["user", "official"],
    "即使预览晚于 HTTP 发起，正式正文基线变化也必须使其退役");
  context.turns.set("turn", replaced);
  context.turnVersions.set("turn", 3);
  const stale = context.mergeHistoryTurn({ ...base, items: [...base.items, preview] }, 1);
  assert.deepEqual(Array.from(stale.items, item => item.id), ["user", "official"]);
  assert.equal(stale._nativePreviewBasis, official._nativePreviewBasis);
  context.turns.set("turn", { ...base, items: [...base.items, preview] });
  context.turnVersions.set("turn", 0);
  assert.equal(context.mergeHistoryTurn({ ...base, status: "completed", _nativeDone: true }, 0).items.length, 1);
});

test("Claude 长历史按原生顺序补齐实时快照，零散缓存不能跳过中间分页", () => {
  const id = "claude-01234567-89ab-4cde-8fab-0123456789ab";
  const ids = Array.from({ length: 30 }, (_, index) => `turn-${index + 1}`);
  const turn = key => ({ id: key, _nativeUserId: key, items: [], status: "completed" });
  const context = vm.createContext({
    selectedId: id, selectionVersion: 1, thread: { id, backend: "claude" },
    turns: new Map(ids.slice(-10).map(key => [key, turn(key)])),
    turnVersions: new Map(), itemVersions: new Map(), partialItems: new Set(),
    messageVersion: 0, statusVersion: 0, queueVersion: 0,
    normalizeDisplayedTurnItems: value => value, enrichTurnTranscriptEvents: value => value,
    rememberThreadDisplay() {}, rememberTranscript() {}, reconcileSentMessageDisplays() {},
    updateComposer() {}, scheduleMessages() {}, scheduleSidebarRefresh() {},
    markVisibleThreadRead: async () => {},
  });
  vm.runInContext(sourceSection("function receiveClaudeHistory(", "function receive(event)"), context);
  context.receiveClaudeHistory({ threadId: id, thread: { id, queuedMessages: [], queuedEdits: [] },
    turnIds: ids, turns: ids.slice(-20).reverse().map(turn) });
  assert.deepEqual([...context.turns.keys()], ids.slice(-20), "实时补入的旧轮次必须在最新轮次前面");
  context.turns.set(ids[0], turn(ids[0]));
  assert.equal(context.claudeHistoryCursor(ids), "10", "第一条已缓存不代表中间九轮已加载");
  context.turns.set("live-new", turn("live-new"));
  context.turnVersions.set("live-new", 3);
  const newest = [...ids, "live-new"];
  context.receiveClaudeHistory({ threadId: id, thread: { id, queuedMessages: [] },
    turnIds: newest, turns: [turn("live-new")] });
  context.orderClaudeHistoryTurns([...ids].reverse(), 0);
  assert.deepEqual([...context.turns.keys()], [ids[0], ...ids.slice(-20), "live-new"],
    "迟到 HTTP 顺序不能覆盖较新的原生顺序或移走新轮次");
  assert.equal(context.claudeHistoryCursor(ids), "10");
  for (const key of ids.slice(1, 10)) context.turns.set(key, turn(key));
  context.orderClaudeHistoryTurns(newest);
  assert.deepEqual([...context.turns.keys()], newest);
  assert.equal(context.claudeHistoryCursor(newest), null);
});

test("新建类型切换后，迟到 Codex 模型不覆盖 Claude，项目 ID 保持共用", async () => {
  const reads = [];
  const context = {
    creating: true, creationBackend: "codex", creationProject: "shared-project",
    appearance: { sessionInstances: { "codex:local": true, "claude:wsl:test": true } },
    session: { device: { id: "backend-device" }, runtime: { revision: 1 } },
    models: [], modelsPromise: null, modelsLoadedAt: 0, modelsRetryAt: 0, modelDefaults: {},
    modelCatalogCacheVersion: 1, catalogFallbackMs: 1000, catalogRetryMs: 1000,
    privateInputScope: () => "backend-device", catalogUsable: () => false,
    api: (_path, payload) => { const response = deferred(); reads.push({ payload, response }); return response.promise; },
    cacheDisplayData() {}, scheduleCatalogRefresh() {}, updateComposer() {}, $: () => null,
  };
  vm.createContext(context);
  vm.runInContext(sourceSection("async function loadModels(", "function rememberedNewChatModel("), context);
  const codex = context.loadModels(true);
  context.creationBackend = "claude";
  context.creationInstanceId = "claude:wsl:test";
  context.modelsPromise = null;
  const claude = context.loadModels(true);
  assert.equal(reads[1].payload.backend, "claude");
  assert.equal(reads[1].payload.projectId, "shared-project");
  reads[1].response.resolve({ data: [{ model: "haiku" }], defaultModel: "haiku" });
  await claude;
  reads[0].response.resolve({ data: [{ model: "codex-model" }], defaultModel: "codex-model" });
  await codex;
  assert.equal(context.models[0].model, "haiku");
  assert.equal(context.modelDefaults.model, "haiku");
});

function claudeHistoryModelMenuHarness({ cached = false, stale = false } = {}) {
  const reads = [], cacheWrites = [];
  const catalog = { data: [
    { model: "history-haiku", supportedReasoningEfforts: [{ reasoningEffort: "high" }] },
    { model: "fixture-opus", supportedReasoningEfforts: [{ reasoningEffort: "max" }] },
    { model: "fixture-sonnet", supportedReasoningEfforts: [{ reasoningEffort: "medium" }] },
  ], defaultModel: "history-haiku", defaultReasoningEffort: "" };
  function element(tag, className = "", textContent = "") {
    return { tag, className, textContent, children: [], hidden: false,
      append(...nodes) { this.children.push(...nodes); },
      prepend(...nodes) { this.children.unshift(...nodes); },
      replaceChildren(...nodes) { this.children = nodes; },
      setAttribute() {} };
  }
  const menu = element("div"), trigger = element("button");
  menu.hidden = true;
  const context = vm.createContext({
    creating: false, selectedId: "claude-00000000-0000-4000-8000-000000000001",
    thread: { id: "claude-00000000-0000-4000-8000-000000000001", backend: "claude",
      instanceId: "claude:wsl:history", model: "history-haiku", reasoningEffort: "high" },
    appearance: { sessionInstances: { "claude:wsl:history": true } },
    session: { device: { id: "history-model-device" }, runtime: { revision: 1 } },
    models: cached ? catalog.data : [],
    modelsPromise: null, modelsLoadedAt: cached ? Date.now() - (stale ? 3600001 : 0) : 0, modelsRetryAt: 0,
    modelDefaults: cached ? { model: catalog.defaultModel, reasoningEffort: "" } : {},
    modelCatalogCacheVersion: 1, catalogFallbackMs: 1000, catalogRetryMs: 1000,
    selectedModel: "", selectedEffort: "", modelQuery: "",
    privateInputScope: value => value?.device?.id,
    catalogUsable: value => value.loadedAt > 0,
    catalogNeedsRefresh: value => Date.now() - value.loadedAt > 3600000,
    api(path, payload) {
      assert.equal(path, "/api/read/models", "模型菜单只允许模型读取，不能发送或恢复会话");
      const response = deferred(); reads.push({ payload, response }); return response.promise;
    },
    cacheDisplayData(...args) { cacheWrites.push(args); }, scheduleCatalogRefresh() {}, updateComposer() {}, element,
    $: selector => selector === "#model-settings" ? menu : selector === "#model-button" ? trigger : null,
    threadReadOnly: () => false, closePopup() {}, closeComposerMenus() { menu.hidden = true; },
    fitComposerMenu() {}, renderModelMenu() {
      menu.replaceChildren(...Array.from(context.models, model => element("button", "", model.model)));
    },
  });
  vm.runInContext(sourceSection("async function loadModels(", "function rememberedNewChatModel("), context);
  vm.runInContext(sourceSection('  $("#model-button").onclick = async () => {', '  $("#model-settings").onkeydown ='), context);
  return { context, reads, cacheWrites, menu, trigger, catalog, settle: () => new Promise(resolve => setImmediate(resolve)) };
}

function claudeAliasModelMenuHarness() {
  class MenuNode {
    constructor(tag, className = "", text = "") {
      this.tag = tag; this.className = className; this.text = text;
      this.children = []; this.dataset = {}; this.attributes = {}; this.hidden = false; this.scrollTop = 0;
    }
    get textContent() { return this.text + this.children.map(node => node.textContent).join(""); }
    append(...nodes) { this.children.push(...nodes); }
    prepend(...nodes) { this.children.unshift(...nodes); }
    replaceChildren(...nodes) { this.children = nodes; }
    setAttribute(name, value) { this.attributes[name] = value; }
    contains(value) { return !!value && (this === value || this.children.some(node => node.contains(value))); }
    querySelectorAll(selector) {
      return this.children.flatMap(node => [
        ...(selector === "button" ? node.tag === "button" : node.className.split(" ").includes(selector.slice(1))) ? [node] : [],
        ...node.querySelectorAll(selector),
      ]);
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
    focus() {}
  }
  const menu = new MenuNode("div"), prompt = new MenuNode("input");
  const context = vm.createContext({
    models: [
      { model: "opus", resolvedModel: "claude-opus-5-5", displayName: "Opus 5.5",
        supportedReasoningEfforts: ["low", "medium", "high", "xhigh", "max"].map(reasoningEffort => ({ reasoningEffort })) },
      { model: "sonnet", resolvedModel: "claude-sonnet-5-5", displayName: "Sonnet 5.5",
        supportedReasoningEfforts: [{ reasoningEffort: "medium" }, { reasoningEffort: "high" }] },
      { model: "legacy-haiku", resolvedModel: "claude-haiku-4-5", displayName: "Haiku 4.5",
        supportedReasoningEfforts: [] },
    ],
    selectedId: "claude-00000000-0000-4000-8000-000000000001", creating: false,
    thread: { backend: "claude", model: "claude-opus-5-5", reasoningEffort: "max" },
    selectedModel: "", selectedEffort: "", modelQuery: "",
    modelDefaults: { model: "opus", reasoningEffort: "high" },
    document: { activeElement: prompt }, newChatModels: new Map(),
    session: { device: { id: "model-device" } }, privateInputScope: () => "model-device",
    localStorage: { getItem: () => "", setItem() {} },
    currentThreadDisplay: () => context.thread,
    effortLabels: { low: "轻度", medium: "中", high: "高", xhigh: "极高", max: "Max" },
    element: (tag, className, text) => new MenuNode(tag, className, text),
    button: (text, _label, action, className) => Object.assign(new MenuNode("button", className, text), { onclick: action }),
    $: (selector, root) => root ? root.querySelector(selector)
      : selector === "#model-settings" ? menu : selector === "#prompt" ? prompt : null,
    updateComposer() {}, fitComposerMenu() {}, closeComposerMenus() { menu.hidden = true; },
  });
  vm.runInContext(sourceSection("function rememberedNewChatModel(", "function fitComposerMenu("), context);
  const choices = group => menu.querySelector(`.${group}`).children.filter(node => node.tag === "button");
  return { context, menu, choices };
}

test("Claude 完整模型 ID 匹配原生别名，显示已选模型和推理等级，重选同模型保留 Max", () => {
  const h = claudeAliasModelMenuHarness();
  h.context.renderModelMenu();
  assert.equal(h.context.currentModel(), h.context.models[0]);
  assert.equal(h.context.currentEffort(), "max");
  assert.deepEqual(h.choices("model-options").filter(node => node.attributes["aria-checked"] === "true")
    .map(node => node.dataset.choiceKey), ["model:opus"]);
  assert.deepEqual(h.choices("effort-options").map(node => node.dataset.choiceKey),
    ["effort:low", "effort:medium", "effort:high", "effort:xhigh", "effort:max"]);
  assert.equal(h.menu.textContent.includes("原生模型列表暂未提供"), false);
  h.choices("model-options")[0].onclick();
  assert.equal(h.context.selectedModel, "");
  assert.equal(h.context.currentEffort(), "max");
  h.choices("effort-options").find(node => node.dataset.choiceKey === "effort:high").onclick();
  assert.equal(h.context.selectedEffort, "high");
  h.choices("model-options").find(node => node.dataset.choiceKey === "model:sonnet").onclick();
  assert.equal(h.context.selectedModel, "sonnet");
  assert.equal(h.context.selectedEffort, "medium");
});

test("Claude 模型匹配优先精确值，未知或未报告等级不猜测，Codex 不扩展别名规则", () => {
  const h = claudeAliasModelMenuHarness();
  h.context.models.push({ model: "claude-opus-5-5", supportedReasoningEfforts: [{ reasoningEffort: "low" }] });
  assert.equal(h.context.currentModel(), h.context.models[3]);
  h.context.renderModelMenu();
  assert.deepEqual(h.choices("effort-options").map(node => node.dataset.choiceKey), ["effort:low"]);
  h.context.models.pop();
  h.context.thread.model = "claude-opus-unreported";
  h.context.renderModelMenu();
  assert.equal(h.context.currentModel(), null);
  assert.ok(h.menu.textContent.includes("原生模型列表暂未提供"));
  assert.equal(h.choices("effort-options").length, 0);
  h.context.thread.model = "claude-haiku-4-5";
  h.context.renderModelMenu();
  assert.equal(h.choices("effort-options").length, 0);
  h.context.thread = { backend: "codex", model: "claude-opus-5-5" };
  assert.equal(h.context.currentModel(), null);
  assert.equal(h.context.modelForId(""), null);
  assert.equal(h.context.modelForId(undefined), null);
});

test("Claude 新建默认等级与记住的完整模型 ID 复用同一别名目录", () => {
  const h = claudeAliasModelMenuHarness();
  Object.assign(h.context, { creating: true, selectedId: null, creationBackend: "claude",
    creationInstanceId: "claude:wsl:fixture", selectedModel: "claude-opus-5-5" });
  h.context.newChatModels.set("model-device:claude:wsl:fixture", "claude-opus-5-5");
  assert.equal(h.context.rememberedNewChatModel(), h.context.models[0]);
  assert.equal(h.context.currentEffort(), "high");
});

test("历史当前模型不进入共用目录，点击选择器时读取完整模型和思考深度", async () => {
  const h = claudeHistoryModelMenuHarness();
  const history = h.context.loadModels();
  assert.equal(h.reads[0].payload.includeAvailable, false, "单纯显示历史不能触发能力初始化");
  h.reads[0].response.resolve({ data: [h.catalog.data[0]], catalogComplete: false,
    defaultModel: "history-haiku", defaultReasoningEffort: "high" });
  await history;
  assert.equal(h.context.modelsLoadedAt, 0);
  assert.equal(h.context.models.length, 0);
  assert.equal(h.cacheWrites.length, 0, "单个历史模型不能污染实例的共用目录缓存");
  const opened = h.trigger.onclick();
  assert.equal(h.reads.length, 2, "当前模型展示不能阻止显式读取可用选项");
  assert.deepEqual({ ...h.reads[1].payload }, { backend: "claude", instanceId: "claude:wsl:history",
    threadId: h.context.selectedId, forceReload: false, includeAvailable: true });
  assert.ok(h.menu.children.some(node => /正在读取/.test(node.textContent)));
  h.reads[1].response.resolve(h.catalog);
  await opened;
  assert.deepEqual(Array.from(h.context.models, model => model.model), ["history-haiku", "fixture-opus", "fixture-sonnet"]);
  assert.equal(h.context.models[1].supportedReasoningEfforts[0].reasoningEffort, "max");
  assert.deepEqual(h.menu.children.map(node => node.textContent), ["history-haiku", "fixture-opus", "fixture-sonnet"]);
  assert.equal(h.context.thread.model, "history-haiku");
  assert.equal(h.context.selectedModel, "", "读取完整目录不能自行切换历史会话模型");
  assert.equal(h.cacheWrites[0][1], "claude:wsl:history");
});

test("同一实例的新建与多个历史会话共用完整模型缓存，不逐会话请求目录", async () => {
  const h = claudeHistoryModelMenuHarness({ cached: true });
  const key = h.context.modelContextKey();
  await h.trigger.onclick();
  h.context.selectedId = "claude-00000000-0000-4000-8000-000000000002";
  h.context.thread = { ...h.context.thread, id: h.context.selectedId, model: "fixture-opus", reasoningEffort: "max" };
  assert.equal(h.context.modelContextKey(), key);
  await h.context.loadModels();
  h.context.creating = true; h.context.selectedId = null; h.context.thread = null;
  h.context.creationBackend = "claude"; h.context.creationInstanceId = "claude:wsl:history";
  h.context.creationProject = "another-project";
  assert.equal(h.context.modelContextKey(), key);
  await h.context.loadModels();
  assert.equal(h.context.models.length, 3);
  assert.equal(h.reads.length, 0);
  assert.equal(h.context.modelDefaults.reasoningEffort, "", "历史会话的等级不能污染新建默认值");
});

test("点击模型菜单会在进行中的历史读取后补读完整目录，并合并重复选择请求", async () => {
  const h = claudeHistoryModelMenuHarness({ cached: false });
  const history = h.context.loadModels();
  const opened = h.trigger.onclick();
  assert.equal(h.reads.length, 1);
  assert.equal(h.reads[0].payload.includeAvailable, false);
  h.reads[0].response.resolve({ data: [h.catalog.data[0]], catalogComplete: false });
  await history; await h.settle();
  assert.equal(h.reads.length, 2);
  assert.equal(h.reads[1].payload.includeAvailable, true);
  const repeated = h.context.loadModels(false, { includeAvailable: true });
  assert.equal(h.reads.length, 2);
  h.reads[1].response.resolve(h.catalog);
  await Promise.all([opened, repeated]);
  assert.equal(h.context.models.length, 3);
});

test("历史模型读取期间切换会话后，旧菜单不能替新实例发起能力探测", async () => {
  const h = claudeHistoryModelMenuHarness({ cached: false });
  const history = h.context.loadModels(), opened = h.trigger.onclick();
  h.context.selectedId = "claude-00000000-0000-4000-8000-000000000002";
  h.context.thread = { ...h.context.thread, id: h.context.selectedId, instanceId: "claude:wsl:other" };
  h.context.appearance.sessionInstances["claude:wsl:other"] = true;
  h.context.models = [{ model: "other-instance-model" }];
  h.reads[0].response.resolve({ data: [h.catalog.data[0]] });
  await Promise.all([history, opened]);
  assert.equal(h.reads.length, 1);
  assert.equal(h.context.models[0].model, "other-instance-model");
});

test("历史模型目录读取失败时显示错误并保留旧目录，重新打开可重试", async () => {
  const h = claudeHistoryModelMenuHarness({ cached: true, stale: true });
  const opened = h.trigger.onclick();
  h.reads[0].response.reject(new Error("模拟原生模型目录读取失败"));
  await opened;
  assert.ok(h.menu.children.some(node => /模拟原生模型目录读取失败/.test(node.textContent)));
  assert.equal(h.context.models[0].model, "history-haiku");
  assert.equal(h.context.modelLoadError, "模拟原生模型目录读取失败");
  await h.trigger.onclick();
  const retried = h.trigger.onclick();
  assert.equal(h.reads.length, 2);
  h.reads[1].response.resolve(h.catalog);
  await retried;
  assert.equal(h.context.models.length, 3);
  assert.equal(h.context.modelLoadError, "");
  assert.ok(h.menu.children.every(node => !node.className.includes("error-text")));
});

test("服务部分侧栏保留旧会话展示，完整目录及其他后端不合并旧记录", () => {
  const previous = { nativeBridge: "harness", recent: [{ id: "old" }, { id: "known", title: "旧名" },
    { id: "loose", projectId: "workspace-old" }],
    pinnedThreads: [{ id: "unpin" }, { id: "old-pin" }],
    projects: [{ id: "p", threads: [{ id: "old" }, { id: "known", title: "旧名" }] },
      { id: "workspace-old", synthetic: true, threads: [{ id: "loose", projectId: "workspace-old" }] }] };
  const current = { nativeBridge: "harness", complete: false,
    recent: [{ id: "known", title: "新名" }, { id: "unpin", pinned: false }], pinnedThreads: [],
    projects: [{ id: "p", threads: [{ id: "known", title: "新名" }] }] };
  const context = {};
  vm.createContext(context);
  vm.runInContext(sourceSection("function mergeIncompleteSidebar(", "async function loadSidebar("), context);
  const merged = context.mergeIncompleteSidebar(previous, current);
  assert.deepEqual(Array.from(merged.recent, row => row.id), ["known", "unpin", "old", "loose"]);
  assert.equal(merged.recent[0].title, "新名");
  assert.equal(merged.recent[3].projectId, null);
  assert.deepEqual(Array.from(merged.pinnedThreads, row => row.id), ["old-pin"]);
  assert.deepEqual(Array.from(merged.projects, project => project.id), ["p"]);
  assert.deepEqual(Array.from(merged.projects[0].threads, row => row.id), ["known", "old"]);
  for (const input of [{ ...current, complete: true }, { ...current, nativeBridge: "cdp" }])
    assert.equal(context.mergeIncompleteSidebar(previous, input), input);
});

test("原生置顶完整时不被部分最近目录恢复，取消的旧会话仍保留在普通列表", () => {
  const previous = { nativeBridge: "harness",
    recent: [{ id: "unpin", projectId: "p", pinned: true }],
    pinnedThreads: [{ id: "unpin", projectId: "p", pinned: true }, { id: "pin-only", pinned: true }],
    projects: [{ id: "p", threads: [{ id: "unpin", projectId: "p", pinned: true }] }] };
  const current = { nativeBridge: "harness", complete: false, pinnedThreadsComplete: true,
    recent: [{ id: "new-pin", pinned: true }], pinnedThreads: [{ id: "new-pin", pinned: true }],
    projects: [{ id: "p", threads: [] }] };
  const context = {};
  vm.createContext(context);
  vm.runInContext(sourceSection("function mergeIncompleteSidebar(", "async function loadSidebar("), context);
  const merged = context.mergeIncompleteSidebar(previous, current);
  assert.deepEqual(Array.from(merged.pinnedThreads, row => row.id), ["new-pin"]);
  assert.equal(merged.recent.find(row => row.id === "unpin").pinned, false);
  assert.equal(merged.recent.find(row => row.id === "pin-only").pinned, false);
  assert.equal(merged.projects[0].threads[0].pinned, false);
  assert.equal(new Set(merged.recent.map(row => row.id)).size, merged.recent.length);
  const empty = context.mergeIncompleteSidebar(previous, { ...current, recent: [], pinnedThreads: [] });
  assert.equal(empty.pinnedThreads.length, 0, "原生取消全部置顶后不能复活旧置顶");
});

test("旧服务缓存去除合成项目后仍保留无项目会话及已有项目名称和顺序", () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(sourceSection("function sidebarSnapshot(", "function clearReadViews("), context);
  const snapshot = context.sidebarSnapshot({
    nativeBridge: "harness",
    recent: [{ id: "loose", projectId: "workspace-old" }, { id: "assigned", projectId: "saved-z" }],
    pinnedThreads: [{ id: "pinned-loose", projectId: "workspace-old" }],
    projects: [
      { id: "saved-z", name: "已保存项目", threads: [{ id: "assigned", projectId: "saved-z" }] },
      { id: "workspace-old", name: "new-chat", synthetic: true, threads: [{ id: "loose" }] },
      { id: "saved-a", name: "空项目", threads: [] }
    ]
  });
  assert.deepEqual(Array.from(snapshot.projects, project => project.id), ["saved-z", "saved-a"]);
  assert.equal(snapshot.recent[0].projectId, null);
  assert.equal(snapshot.recent[1].projectId, "saved-z");
  assert.equal(snapshot.pinnedThreads[0].projectId, null);
  assert.equal(snapshot.projects[0].name, "已保存项目");
});

test("项目选择只保留已保存项目，真实同名项目显示路径，合成项目不参与重名判断", () => {
  const context = {};
  vm.createContext(context);
  vm.runInContext(sourceSection("function newChatProjectOptions(", "function syncNewChatProjectPicker("), context);
  const projects = [
    { id: "saved-z", name: "同名项目", roots: ["C:\\repo\\one"] },
    { id: "saved-a", name: "同名项目", roots: ["D:\\repo\\two"] },
    { id: "saved-unique", name: "唯一项目", roots: ["E:\\repo\\unique"] },
    { id: "workspace-old", name: "唯一项目", roots: ["C:\\tmp\\unique"], synthetic: true }
  ];
  const options = context.newChatProjectOptions(projects);
  assert.deepEqual(Array.from(options, option => option.value), ["saved-z", "saved-a", "saved-unique", ""]);
  assert.deepEqual(Array.from(options.slice(0, 3), option => option.description),
    ["C:\\repo\\one", "D:\\repo\\two", ""]);
  assert.equal(options.at(-1).label, "不在项目中工作");
  assert.deepEqual(options, context.newChatProjectOptions(projects.filter(project => !project.synthetic)));
});

function workspaceChangeContract() {
  const context = vm.createContext({});
  vm.runInContext(sessionFiles.slice(sessionFiles.indexOf("export function displayWorkspacePath("),
    sessionFiles.indexOf("export function workspaceBranchLabel(")).replace(/^export /gm, "")
    + "\n" + sessionTools.slice(sessionTools.indexOf("const sections ="), sessionTools.indexOf("export function mountSessionTools"))
    + "\nglobalThis.contract = { detailKey, displayedPath, listResponse, detailResponse, uiState };", context);
  return context.contract;
}

test("多仓库变更按仓库隔离同名路径，保留分组信息和各仓库折叠状态", () => {
  const contract = workspaceChangeContract();
  const alpha = { repository: "web/alpha", path: "src/app.ts", section: "unstaged", status: "M" };
  const beta = { ...alpha, repository: "web/beta" };
  assert.notEqual(contract.detailKey(alpha), contract.detailKey(beta));
  assert.equal(contract.displayedPath(beta), "web/beta/src/app.ts");
  const response = contract.listResponse({
    ok: true, root: "D:/workspace", revision: "a".repeat(64), truncated: false,
    git: { branch: "main", detachedHead: null },
    repositories: ["web/alpha", "web/beta"].map(path => ({
      path, root: `D:/workspace/${path}`, fileCount: 1, truncated: false, error: null,
      git: { branch: path === "web/alpha" ? "feature/alpha" : "release/beta", detachedHead: null }
    })),
    files: [alpha, beta],
  });
  assert.equal(response.repositories.length, 2);
  assert.equal(response.files[1].repository, "web/beta");
  assert.equal(response.files[1].path, "src/app.ts");
  assert.equal(response.git.branch, "main");
  assert.equal(response.repositories[0].git.branch, "feature/alpha");
  assert.equal(response.repositories[1].git.branch, "release/beta");
  const alphaGroup = JSON.stringify(["web/alpha", "unstaged"]);
  const betaGroup = JSON.stringify(["web/beta", "unstaged"]);
  const ui = contract.uiState({ collapsed: { [alphaGroup]: true, [betaGroup]: false } });
  assert.equal(ui.collapsed[alphaGroup], true);
  assert.notEqual(ui.collapsed[betaGroup], true);
});

test("多仓库变更拒绝越界、重复仓库和错误仓库的详情，同时兼容单仓库旧响应", () => {
  const contract = workspaceChangeContract();
  const file = { repository: "alpha", path: "same.txt", section: "untracked", status: "A" };
  const repository = { path: "alpha", root: "D:/workspace/alpha", fileCount: 1, truncated: false, error: null };
  const response = { ok: true, root: "D:/workspace", revision: "b".repeat(64), truncated: false,
    repositories: [repository], files: [file] };
  for (const path of ["../outside", "/outside", "C:/outside", "alpha/.git/objects"]) {
    assert.equal(contract.listResponse({ ...response, files: [{ ...file, repository: path }] }), null);
  }
  assert.equal(contract.listResponse({ ...response, repositories: [repository, repository] }), null);
  const detail = { ...file, root: response.root, revision: response.revision, kind: "text", truncated: false, diff: "+alpha" };
  assert.ok(contract.detailResponse(detail, file));
  assert.equal(contract.detailResponse({ ...detail, repository: "beta" }, file), null);
  const { repository: _repository, ...legacyFile } = file;
  const legacy = contract.listResponse({ ok: true, root: response.root, revision: response.revision,
    truncated: false, files: [legacyFile] });
  assert.ok(legacy);
  assert.equal(legacy.repositories, undefined);
  assert.equal(legacy.files[0].repository, undefined);
});

test("新版多仓库页面连接旧后台时仅对旧白名单拒绝回退，并暂时记住旧能力", async () => {
  const calls = [];
  const context = vm.createContext({
    legacyChangesUntil: 0,
    request: async (path, payload) => {
      calls.push({ path, payload });
      if (payload.allRepositories) throw Object.assign(new Error("只能通过会话或项目标识读取变更"), { code: 400 });
      return { ok: true, legacy: true };
    },
  });
  vm.runInContext(sessionTools.slice(sessionTools.indexOf("  async function requestChanges("), sessionTools.indexOf("  function status(")), context);
  assert.equal((await context.requestChanges({ threadId: "thread" })).legacy, true);
  assert.equal(calls.length, 2);
  assert.equal(calls[0].payload.allRepositories, true);
  assert.equal(calls[1].payload.allRepositories, undefined);
  await context.requestChanges({ threadId: "thread" });
  assert.equal(calls.length, 3);
  assert.equal(calls[2].payload.allRepositories, undefined);
  context.legacyChangesUntil = 0;
  context.request = async () => { throw Object.assign(new Error("访问已撤销"), { code: 403 }); };
  await assert.rejects(context.requestChanges({ threadId: "thread" }), error => error.code === 403);
});

function promptUndoHarness() {
  const microtasks = [];
  let clock = 0, scope = "device-a:thread-a", readOnly = false;
  const prompt = { text: "", selection: { start: 0, end: 0 }, disabled: false, scrollTop: 0, scrollLeft: 0, focus() {} };
  const context = nodeVm.createContext({
    selectedSkills: [], selectedGoal: null,
    queueMicrotask: action => microtasks.push(action),
    skillIdentity: skill => `${skill.name}\0${skill.path}`,
    promptPlainText: () => prompt.text,
    promptSelection: () => prompt.selection,
    setPromptSelection: (_prompt, start, end, selection) => { prompt.selection = { ...selection, start, end }; },
    writePromptContent: (_prompt, text) => { prompt.editHistory.begin("replace"); prompt.text = text; },
    handlePromptInput: () => prompt.editHistory.commit(),
    randomId: () => "fresh-goal-id",
    threadReadOnly: () => readOnly,
    removeAdjacentPromptToken: () => false,
    $: selector => selector === "#prompt" ? prompt : { hidden: true },
  });
  nodeVm.runInContext(app.slice(app.indexOf("function createPromptEditHistory("),
    app.indexOf("function configurePromptEditor("))
    + "\n" + app.slice(app.indexOf('  $("#prompt").onkeydown ='), app.indexOf('  $("#prompt").onpaste =')), context);
  prompt.editHistory = context.createPromptEditHistory(prompt, () => scope, () => clock);
  const flush = () => { while (microtasks.length) microtasks.shift()(); };
  const key = (key, extra = {}) => {
    const event = { key, ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
      isComposing: false, preventDefault() { this.prevented = true; }, ...extra };
    prompt.onkeydown(event); return event;
  };
  const native = (text, type = "insertText") => {
    prompt.onbeforeinput({ inputType: type, data: text, isComposing: false, preventDefault() {} });
    // 浏览器可在 beforeinput 默认改写 DOM 前运行微任务；不能在这个时机丢弃原编辑快照。
    flush();
    prompt.text = text;
    prompt.selection = { start: text.length, end: text.length };
    prompt.editHistory.commit();
    flush();
  };
  const replace = text => {
    prompt.editHistory.begin("replace");
    prompt.text = text; prompt.selection = { start: text.length, end: text.length };
    flush();
  };
  return {
    prompt, context, flush, key, native, replace,
    tick: milliseconds => { clock += milliseconds; },
    scope: value => { scope = value; },
    readOnly: value => { readOnly = value; },
  };
}

test("编辑框连续输入按一段撤销，Ctrl+Y 与 Ctrl+Shift+Z 可重做", () => {
  const h = promptUndoHarness();
  for (const text of ["h", "he", "hel", "hell", "hello"]) { h.native(text); h.tick(30); }
  assert.equal(h.key("z").prevented, true);
  assert.equal(h.prompt.text, "");
  h.key("y"); assert.equal(h.prompt.text, "hello");
  h.key("z"); h.key("z", { shiftKey: true });
  assert.equal(h.prompt.text, "hello");
  h.tick(1100); h.native("hello world");
  h.key("z"); assert.equal(h.prompt.text, "hello", "停顿后的编辑独立撤销");
});

test("粘贴、换行和选区替换分别撤销，并恢复被替换的选区", () => {
  const h = promptUndoHarness();
  h.replace("第一行\n第二行");
  h.replace("第一行\n第二行\n");
  h.key("z"); assert.equal(h.prompt.text, "第一行\n第二行");
  h.key("z"); assert.equal(h.prompt.text, "");
  h.key("y"); assert.equal(h.prompt.text, "第一行\n第二行");
  h.prompt.selection = { start: 0, end: 3 };
  h.replace("修改后\n第二行");
  h.key("z");
  assert.equal(h.prompt.text, "第一行\n第二行");
  assert.equal(h.prompt.selection.start, 0);
  assert.equal(h.prompt.selection.end, 3);
  h.key("y"); assert.equal(h.prompt.text, "修改后\n第二行");
});

test("撤销恢复 Skill 位置和目标标签，目标恢复不复用旧请求标识", () => {
  const h = promptUndoHarness();
  h.context.selectedSkills = [{ name: "test-skill", path: "/fixture/SKILL.md", _offset: 1 }];
  h.context.selectedGoal = "old-goal-id";
  h.prompt.text = "abc"; h.prompt.selection = { start: 1, end: 1 };
  h.prompt.editHistory.begin("replace");
  h.context.selectedSkills = []; h.context.selectedGoal = null;
  h.prompt.text = "a新内容bc"; h.flush();
  h.key("z");
  assert.equal(h.prompt.text, "abc");
  assert.deepEqual(JSON.parse(JSON.stringify(h.context.selectedSkills)), [{ name: "test-skill", path: "/fixture/SKILL.md", _offset: 1 }]);
  assert.equal(h.context.selectedGoal, "fresh-goal-id");
  h.key("y");
  assert.equal(h.prompt.text, "a新内容bc");
  assert.equal(h.context.selectedSkills.length, 0);
  assert.equal(h.context.selectedGoal, null);
});

test("中文输入法候选过程不被撤销打断，提交后整次输入可以撤销", () => {
  const h = promptUndoHarness();
  h.prompt.text = "原稿"; h.prompt.selection = { start: 2, end: 2 };
  h.prompt.editHistory.startComposition();
  for (const text of ["原稿n", "原稿ni", "原稿你"]) {
    h.prompt.text = text; h.prompt.selection = { start: text.length, end: text.length };
    h.prompt.editHistory.commit(); h.flush();
    assert.equal(h.prompt.editHistory.undo(), false);
  }
  const candidateUndo = { inputType: "historyUndo", isComposing: false, preventDefault() { this.prevented = true; } };
  h.prompt.onbeforeinput(candidateUndo);
  assert.equal(candidateUndo.prevented, undefined, "候选阶段保留输入法原生处理，不拦截其撤销");
  h.prompt.editHistory.endComposition(); h.prompt.editHistory.commit();
  h.key("z"); assert.equal(h.prompt.text, "原稿");
  h.key("y"); assert.equal(h.prompt.text, "原稿你");
});

test("撤销后新编辑丢弃重做分支，切换会话和清空草稿均不恢复旧内容", () => {
  const h = promptUndoHarness();
  h.replace("原内容"); h.replace("第二次修改"); h.key("z"); h.replace("新分支");
  assert.equal(h.prompt.editHistory.redo(), false);
  assert.equal(h.prompt.text, "新分支");
  h.scope("device-a:thread-b"); h.prompt.text = "其他会话草稿";
  h.key("z"); assert.equal(h.prompt.text, "其他会话草稿");
  h.replace("等待提交的编辑");
  h.prompt.editHistory.begin("replace"); h.prompt.text = "";
  h.prompt.editHistory.reset(); h.flush();
  h.key("z"); assert.equal(h.prompt.text, "", "发送后清空建立新基线，待处理微任务不能复活原稿");
});

test("编辑框支持 beforeinput 的撤销与重做，只读和输入法组合期间不改内容", () => {
  const h = promptUndoHarness();
  h.replace("第一行\n第二行");
  const undo = { inputType: "historyUndo", isComposing: false, preventDefault() { this.prevented = true; } };
  h.prompt.onbeforeinput(undo);
  assert.equal(undo.prevented, true); assert.equal(h.prompt.text, "");
  h.prompt.onbeforeinput({ inputType: "historyRedo", isComposing: false, preventDefault() {} });
  assert.equal(h.prompt.text, "第一行\n第二行");
  h.readOnly(true); h.key("z"); assert.equal(h.prompt.text, "第一行\n第二行");
  h.readOnly(false); h.key("z", { isComposing: true });
  assert.equal(h.prompt.text, "第一行\n第二行");
});

test("长时间编辑仅保留最近一百次独立修改，淘汰旧记录后仍能撤销到正确位置", () => {
  const h = promptUndoHarness();
  for (let value = 1; value <= 150; value++) h.replace(String(value));
  let count = 0;
  while (h.prompt.editHistory.undo()) count++;
  assert.equal(count, 100);
  assert.equal(h.prompt.text, "50");
  while (h.prompt.editHistory.redo()) {}
  assert.equal(h.prompt.text, "150");
});

test("外观表面与原生 Codex 默认调色一致，并区分输入框、控件及浅色普通提示", () => {
  const context = nodeVm.createContext();
  nodeVm.runInContext(sourceSection("function mixColor(", "function applyAppearance("), context);
  const dark = context.appearanceSurfaceColors("dark", { background: "#181818", foreground: "#ffffff" });
  assert.equal(dark["composer-bg"], "#363636");
  assert.equal(dark["control-bg"], "#2d2d2d");
  assert.equal(dark["control-hover"], "#3d3d3d");
  assert.equal(dark["tooltip-bg"], dark["control-bg"]);
  assert.equal(dark["tooltip-text"], "#ffffff");
  const light = context.appearanceSurfaceColors("light", { background: "#ffffff", foreground: "#1a1c1f" });
  assert.equal(light["composer-bg"], "#ffffff");
  assert.equal(light["control-bg"], "#ffffff");
  assert.equal(light["tooltip-bg"], "#1a1c1f");
  assert.equal(light["tooltip-text"], "#ffffff");
});

test("自定义背景和前景分别参与明暗调色，不复用默认主题色或透明表面", () => {
  const context = nodeVm.createContext();
  nodeVm.runInContext(sourceSection("function mixColor(", "function applyAppearance("), context);
  const dark = context.appearanceSurfaceColors("dark", { background: "#102030", foreground: "#f0e0d0" });
  assert.equal(dark["control-bg"], "#24313e");
  assert.equal(dark["composer-bg"], "#2d3944");
  assert.equal(dark["control-hover"], "#343f49");
  const light = context.appearanceSurfaceColors("light", { background: "#f0e0d0", foreground: "#182838" });
  assert.equal(light["control-bg"], "#f2e3d5");
  assert.equal(light["composer-bg"], "#f3e7da");
  assert.equal(light["tooltip-bg"], "#182838");
  assert.equal(light["tooltip-text"], "#f0e0d0");
  for (const value of [...Object.values(dark), ...Object.values(light)]) assert.match(value, /^#[0-9a-f]{6}$/);
});

test("模型按钮 hover 在输入框上合成，深色分组线增强，浅色仍沿用原线色", () => {
  const context = nodeVm.createContext();
  nodeVm.runInContext(sourceSection("function mixColor(", "function applyAppearance("), context);
  const dark = context.appearanceSurfaceColors("dark", { background: "#1e1e1e", foreground: "#d4d4d4" });
  assert.equal(dark["composer-bg"], "#353535");
  assert.equal(dark["composer-hover"], "#414141", "模型按钮应高亮，不能变成更暗的页面 hover");
  assert.equal(dark["control-bg"], "#2e2e2e");
  assert.equal(dark["menu-divider"], "#484848");
  assert.notEqual(dark["composer-hover"], dark["control-hover"], "按钮和弹出菜单有不同的底色");
  const light = context.appearanceSurfaceColors("light", { background: "#ffffff", foreground: "#1f1f1f" });
  assert.equal(light["composer-hover"], "#f3f3f3");
  assert.equal(light["menu-divider"], "#e0e0e0", "浅色不附带加深分组线");
  const custom = context.appearanceSurfaceColors("dark", { background: "#102030", foreground: "#f0e0d0" });
  assert.notEqual(custom["composer-hover"], dark["composer-hover"]);
  assert.notEqual(custom["menu-divider"], dark["menu-divider"]);
});

test("设置 HEX 可独立编辑，非法草稿不保存，失焦或 Escape 恢复实际颜色", () => {
  let color = "#16a34a", saves = 0;
  const attributes = new Map(), document = { activeElement: null };
  const input = { value: "", setAttribute: (key, value) => attributes.set(key, value),
    getAttribute: key => attributes.get(key) ?? null,
    blur() { document.activeElement = null; this.onblur(); } };
  const context = nodeVm.createContext({ document });
  nodeVm.runInContext(sourceSection("function bindAppearanceColorValue(", "function appearanceHexToHsv("), context);
  const sync = context.bindAppearanceColorValue(input, () => color, value => { color = value; saves++; sync(); });
  sync();
  assert.equal(input.value, "#16A34A");
  document.activeElement = input; input.value = "#12"; input.oninput();
  assert.equal(input.getAttribute("aria-invalid"), "true");
  assert.equal(color, "#16a34a"); assert.equal(saves, 0);
  sync(); assert.equal(input.value, "#12", "同步其它设置不能覆盖正在编辑的颜色草稿");
  input.onblur(); assert.equal(input.value, "#16A34A");
  input.value = "#ABCDEF"; input.oninput();
  assert.equal(color, "#abcdef"); assert.equal(saves, 1);
  input.value = "#bad";
  const event = { key: "Escape", preventDefault() { this.prevented = true; }, stopPropagation() {} };
  input.onkeydown(event);
  assert.equal(event.prevented, true);
  assert.equal(input.value, "#ABCDEF");
  assert.equal(saves, 1);
  color = "#2563eb"; document.activeElement = null; sync();
  assert.equal(input.value, "#2563EB", "换主题或通过取色器选择后显示当前真实色值");
});

test("三类字号输入在确认后保存整数，空值、小数和越界值恢复，重复失焦不重复保存", () => {
  const context = nodeVm.createContext();
  nodeVm.runInContext(sourceSection("function bindAppearanceFontSize(", "function appearanceHexToHsv("), context);
  for (const [min, max, initial] of [[10, 20, 14], [8, 24, 14], [6, 14, 12]]) {
    let size = initial, saves = 0;
    const attributes = new Map(), input = { value: String(initial),
      setAttribute: (key, value) => attributes.set(key, value),
      getAttribute: key => attributes.get(key) ?? null, blur() { this.onblur(); } };
    context.bindAppearanceFontSize(input, min, max, () => size, value => { size = value; saves++; });
    for (const invalid of ["", "not-a-number", "12.5", String(min - 1), String(max + 1)]) {
      input.value = invalid; input.oninput();
      assert.equal(input.getAttribute("aria-invalid"), "true");
      assert.equal(size, initial);
      input.onblur();
      assert.equal(input.value, String(initial));
      assert.equal(saves, 0);
    }
    input.value = String(max); input.oninput();
    assert.equal(size, initial, "输入过程中不应用半成品数字");
    input.onchange(); input.onblur();
    assert.equal(size, max); assert.equal(saves, 1);
    input.value = String(min);
    input.onkeydown({ key: "Enter", preventDefault() {}, stopPropagation() {} });
    assert.equal(size, min); assert.equal(saves, 2);
    input.value = String(max);
    input.onkeydown({ key: "Escape", preventDefault() {}, stopPropagation() {} });
    assert.equal(input.value, String(min));
    assert.equal(size, min); assert.equal(saves, 2);
  }
});

test("外观切换会重新写入全部表面变量，保存时不修改用户选定的颜色", () => {
  const styles = new Map(), saved = [];
  const appearance = { theme: "dark", colors: {
    dark: { background: "#102030", foreground: "#f0e0d0", accent: "#339cff" },
    light: { background: "#f0e0d0", foreground: "#182838", accent: "#0078d4" },
  } };
  const initial = JSON.stringify(appearance.colors);
  const context = nodeVm.createContext({
    appearance, appearanceTransitionTimer: null, editingMessage: null, document: {
      documentElement: { dataset: {}, style: { setProperty: (name, value) => styles.set(name, value) },
        classList: { add() {}, remove() {} } },
    },
    $: () => null, fontStack: () => "fixture", defaultUiFont: "", defaultCodeFont: "",
    currentFileReadingFontSize: () => 14, syncRemoteBackground() {}, resizeComposerInput() {},
    resizeEditorInput() {}, refitComposerMenus() {}, clearTimeout() {}, setTimeout: () => 1,
    localStorage: { setItem: (_key, value) => saved.push(JSON.parse(value)) },
  });
  nodeVm.runInContext(sourceSection("function mixColor(", "\napplyAutomaticAppearance(false,"), context);
  context.applyAppearance(true);
  assert.equal(styles.get("--control-bg"), "#24313e");
  appearance.theme = "light";
  context.applyAppearance(true);
  assert.equal(styles.get("--control-bg"), "#f2e3d5");
  assert.equal(styles.get("--tooltip-bg"), "#182838");
  assert.equal(styles.get("--tooltip-text"), "#f0e0d0");
  assert.equal(JSON.stringify(appearance.colors), initial);
  assert.equal(saved.length, 2);
  assert.deepEqual(saved[1].colors, appearance.colors);
});

test("编辑区主题提示接管并恢复 title，保留无障碍说明，触屏不弹出，隐藏目标后清理", () => {
  const listeners = new Map(), timers = new Map(), nodes = [], observers = [];
  let timerId = 0;
  function element(_tag, className = "", textContent = "") {
    const attrs = new Map();
    return { id: "", className, textContent, style: {}, isConnected: true, inScope: true, hidden: false,
      setAttribute: (name, value) => attrs.set(name, value),
      getAttribute: name => attrs.get(name) ?? null,
      hasAttribute: name => attrs.has(name), removeAttribute: name => attrs.delete(name),
      contains(other) { return other === this; },
      closest(selector) { return selector === "[title]" ? attrs.has("title") ? this : null
        : selector === "[hidden],[inert]" ? this.hidden ? this : null : this.inScope ? this : null; },
      getBoundingClientRect: () => ({ left: 100, top: 200, bottom: 228, width: 120, height: 28 }),
      showPopover() {}, remove() { const index = nodes.indexOf(this); if (index >= 0) nodes.splice(index, 1); },
    };
  }
  const document = { documentElement: { dataset: { inputModality: "pointer" } },
    body: { append: node => nodes.push(node) },
    addEventListener: (name, handler) => listeners.set(name, handler) };
  const context = nodeVm.createContext({ document, element,
    window: { innerWidth: 800, innerHeight: 600, addEventListener() {} },
    setTimeout: fn => { timers.set(++timerId, fn); return timerId; }, clearTimeout: id => timers.delete(id),
    MutationObserver: class {
      constructor(callback) { this.callback = callback; observers.push(this); }
      observe() {} disconnect() {}
    },
  });
  nodeVm.runInContext(sourceSection("function installComposerTooltips(", "\nlet composerTooltips ="), context);
  context.installComposerTooltips();
  const trigger = element("button");
  trigger.setAttribute("title", "选择模型"); trigger.setAttribute("aria-describedby", "existing-help");
  listeners.get("pointerover")({ target: trigger, pointerType: "touch" });
  assert.equal(timers.size, 0);
  listeners.get("pointerover")({ target: trigger, pointerType: "mouse" });
  assert.equal(trigger.hasAttribute("title"), false, "等待期间也不出现浏览器原生提示");
  const flush = () => { const callbacks = [...timers.values()]; timers.clear(); for (const callback of callbacks) callback(); };
  flush();
  assert.equal(nodes.length, 1);
  assert.equal(nodes[0].textContent, "选择模型");
  assert.equal(trigger.getAttribute("aria-describedby"), "existing-help composer-hover-tooltip");
  listeners.get("pointerout")({ target: trigger, relatedTarget: null });
  assert.equal(nodes.length, 0);
  assert.equal(trigger.getAttribute("title"), "选择模型");
  assert.equal(trigger.getAttribute("aria-describedby"), "existing-help");
  document.documentElement.dataset.inputModality = "keyboard";
  listeners.get("focusin")({ target: trigger }); flush();
  assert.equal(nodes.length, 1);
  trigger.hidden = true; observers.at(-1).callback();
  assert.equal(nodes.length, 0);
  assert.equal(trigger.getAttribute("title"), "选择模型");
});
