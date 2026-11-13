/**
 * 在 Chrome 插件 CUA runtime 中传入已有 tab 后显式调用；导入本模块没有执行副作用。
 * 所有页面 evaluate 仅读取 DOM；所有变更均通过真实页面控件完成。
 */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, join, dirname } from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import {
  sourceFingerprint, createEvidenceReference, verifyEvidenceReference, screenshotFileType,
  verifyNativeEvidence, verifyRemoteAccessReport, verifyRuntimeEvidence,
  verifyQuestionReceiptRecoveryStage,
  verifyQueueRecoveryStage, classifyQueueRecoveryTransition, verifyQueueRecoveryTerminal,
  verifyQueueEditCancelEditingFacts,
  verifyQueueDequeueRaceConsumption, verifyQueueDequeueRaceFacts,
} from "./remote-access-e2e-evidence.mjs";
import { withRemoteAccessEnvironmentLease, restoreRemoteAccessEnvironmentLease,
  validateRemoteAccessEnvironmentScope } from "./remote-access-e2e-environment.mjs";

export const REMOTE_ACCESS_E2E_VERSION = "2.5.12";

export const REMOTE_ACCESS_E2E_CASES = Object.freeze([
  { id: "read.ready", suite: "read", title: "真实页面与连接可用" },
  { id: "read.runtime_version", suite: "read", title: "安装版本、服务协议与网页版本证据" },
  { id: "session.create", suite: "session", title: "普通创建两个独立测试会话" },
  { id: "read.select_refresh", suite: "read", title: "主会话选择、切换与刷新" },
  { id: "session.model", suite: "session", title: "模型选择、协议归属及发送后的实际模型" },
  { id: "session.send_stream", suite: "session", title: "真实发送、执行中流式变化和最终回复" },
  { id: "queue.automatic", suite: "queue", title: "两条原生队列消息按 FIFO 自然承接且各消费一次" },
  { id: "queue.edit_draft", suite: "queue", title: "取回排队消息作为未发送草稿，重新发送恢复原位置" },
  { id: "queue.edit_save", suite: "queue", title: "修改排队草稿后重新发送并刷新核对" },
  { id: "queue.reorder", suite: "queue", title: "队列键盘排序并刷新核对" },
  { id: "queue.remove", suite: "queue", title: "删除指定排队消息" },
  { id: "queue.steer", suite: "queue", title: "引导指定排队消息进入实际执行" },
  { id: "session.stop", suite: "session", title: "停止实际执行并恢复输入" },
  { id: "read.main_read_state", suite: "read", title: "主会话新回复未读、打开已读、刷新保持及再次未读" },
  { id: "lifecycle.side_create", suite: "lifecycle", title: "创建并标记侧边会话" },
  { id: "side.multiple", suite: "side", title: "多个侧边会话独立切换及父子身份" },
  { id: "lifecycle.side_switch", suite: "lifecycle", title: "侧边会话与主会话往返切换" },
  { id: "lifecycle.side_refresh", suite: "lifecycle", title: "侧边会话刷新及主会话归属" },
  { id: "read.side_aggregate", suite: "read", title: "多侧边未读聚合、逐个阅读及再次未读" },
  { id: "read.cross_client", suite: "read", title: "Codex 阅读同步远程及新回复再次未读" },
  { id: "read.side_read_state", suite: "read", title: "侧边与父会话的未读、已读持久化及再次未读" },
  { id: "project.pin_unpin", suite: "project", title: "专用项目置顶取消及刷新保持" },
  { id: "project.search_preferences", suite: "project", title: "项目搜索与客户端独立视图偏好持久化" },
  { id: "session.model_effort_isolation", suite: "session", title: "模型与思考深度实际生效且跨会话不串" },
  { id: "composer.multiline", suite: "composer", title: "多行草稿切换刷新及真实发送" },
  { id: "composer.image", suite: "composer", title: "图片选择、预览、真实发送与原生附件" },
  { id: "composer.skill", suite: "composer", title: "Skill 选择与原生实际输入" },
  { id: "composer.edit_latest", suite: "composer", title: "最新已发消息取消编辑、修改和真实重发" },
  { id: "optimizer.restore", suite: "optimizer", title: "提示词优化和恢复原文" },
  { id: "optimizer.cancel", suite: "optimizer", title: "提示词优化取消后原文保持" },
  { id: "optimizer.late_result", suite: "optimizer", title: "编辑与跨会话切换后迟到结果不覆盖" },
  { id: "optimizer.no_injection", suite: "optimizer", title: "会话服务不可用时独立优化、取消、恢复与只读授权" },
  { id: "context.switch_update", suite: "context", title: "上下文圆环跨会话与新轮次更新" },
  { id: "context.zero_missing", suite: "context", title: "上下文零值和缺失字段实际显示" },
  { id: "transport.service", suite: "transport", title: "正常发送使用服务通道" },
  { id: "recovery.reconnect", suite: "recovery", title: "断线重连恢复真实会话及草稿" },
  { id: "recovery.native_restart", suite: "recovery", title: "原生重启后恢复会话、活动轮和草稿" },
  { id: "recovery.unknown_receipt", suite: "recovery", title: "未知回执只核对原请求且没有重复写入" },
  { id: "recovery.question_receipt", suite: "recovery", title: "回答回包丢失后刷新查询原回执、解除原锁且不重答不丢草稿" },
  { id: "recovery.draft", suite: "recovery", title: "主侧会话草稿切换、刷新与附件保持" },
  { id: "side.late_creation", suite: "side", title: "创建结果迟到不抢占后来选中的会话" },
  { id: "queue.after_stop", suite: "queue", title: "停止后按原生策略自动承接或保留原队列" },
  { id: "queue.after_failure", suite: "queue", title: "失败后队列不丢失且不自动重发失败请求" },
  { id: "queue.approval_wait", suite: "queue", title: "审批等待中队列保持且审批后单次承接" },
  { id: "queue.compaction", suite: "queue", title: "压缩期间队列保持且完成后单次承接" },
  { id: "queue.dequeue_edit_race", suite: "queue", title: "原生出队与编辑并发不重复不丢消息" },
  { id: "queue.dequeue_remove_race", suite: "queue", title: "原生出队与删除并发准确反映原结果" },
  { id: "queue.cross_session", suite: "queue", title: "排队后跨会话和刷新恢复同一队列" },
  { id: "side.close_running", suite: "side", title: "运行中关闭侧边后终止且不影响父会话" },
  { id: "side.delete_parent", suite: "side", title: "删除带侧边的专用父会话后归属和执行清理" },
  { id: "lifecycle.side_close", suite: "lifecycle", title: "关闭本轮创建的侧边会话" },
  { id: "lifecycle.pin", suite: "lifecycle", title: "置顶本轮测试会话并刷新核对" },
  { id: "lifecycle.rename", suite: "lifecycle", title: "改名本轮测试会话并刷新核对" },
  { id: "scenario.approval", suite: "scenario", title: "真实审批及原生回执" },
  { id: "scenario.question", suite: "scenario", title: "真实问答及原生回执" },
  { id: "scenario.question_completed", suite: "scenario", title: "原轮已完成的未答异步问题仍可刷新、回答并确认" },
  { id: "scenario.compaction", suite: "scenario", title: "真实压缩及完成后的可用性" },
  { id: "scenario.goal", suite: "scenario", title: "真实目标创建、执行与原生状态" },
  { id: "android.installed_version", suite: "android", title: "实际安装包版本、签名和内置网页资源" },
  { id: "android.background_resume", suite: "android", title: "Android 后台返回保留会话和草稿" },
  { id: "android.back_navigation", suite: "android", title: "Android 返回键关闭浮层并返回父会话" },
  { id: "android.notification", suite: "android", title: "Android 通知点击按 ID 打开目标会话" },
  { id: "android.resource_update", suite: "android", title: "Android 资源更新、重载及失败恢复" },
  { id: "lifecycle.delete_preview", suite: "lifecycle", title: "永久删除确认前的对象与警告预览" },
  { id: "lifecycle.delete_confirm", suite: "lifecycle", title: "动作当时获得批准后永久删除并核对消失" },
].map(value => Object.freeze({ ...value, required: true })));

const contextTag = Symbol("remote-access-e2e-context");
const statuses = new Set(["passed", "failed", "blocked", "not_run"]);
const suites = new Set(REMOTE_ACCESS_E2E_CASES.map(value => value.suite));

export async function verifyRemoteAccessE2EReport(path) {
  return verifyRemoteAccessReport(path, {
    requiredCases: REMOTE_ACCESS_E2E_CASES, scriptVersion: REMOTE_ACCESS_E2E_VERSION,
    caseContracts: Object.fromEntries(REMOTE_ACCESS_E2E_CASES.filter(value =>
      REMOTE_ACCESS_E2E_CONTRACTS[value.id] || REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS[value.id]?.length).map(value => [value.id, {
      ...REMOTE_ACCESS_E2E_CONTRACTS[value.id],
      requiredDriverSteps: REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS[value.id] || [],
    }])),
  });
}

class Blocked extends Error {
  constructor(message) { super(message); this.name = "E2EBlocked"; }
}

function invariant(value, message) {
  if (!value) throw new Error(message);
}

function block(message) { throw new Blocked(message); }
function iso() { return new Date().toISOString(); }
function copy(value) { return JSON.parse(JSON.stringify(value)); }
function css(value) { return String(value).replace(/["\\]/g, "\\$&"); }
function marker(ctx, key) { return `[remote-e2e:${ctx.report.runId}:${key}]`; }
function ack(ctx, key) { return `REMOTE_E2E_${ctx.report.runId}_${key}`; }

function safeUrl(value) {
  const url = new URL(value);
  url.username = ""; url.password = "";
  for (const key of [...url.searchParams.keys()])
    if (/key|token|secret|auth|password/i.test(key)) url.searchParams.set(key, "[redacted]");
  return url.href;
}

function overall(report) {
  const required = REMOTE_ACCESS_E2E_CASES.filter(value => value.required).map(definition =>
    report.cases.find(value => value.id === definition.id) || { status: "not_run" });
  if (required.some(value => value.status === "failed")) return "failed";
  if (required.some(value => value.status === "blocked")) return "blocked";
  if (required.some(value => value.status === "not_run")) return "not_run";
  return required.length && required.every(value => value.status === "passed") ? "passed" : "not_run";
}

async function checkpoint(ctx) {
  ctx.report.overall = overall(ctx.report);
  ctx.report.updatedAt = iso();
  await writeFile(join(ctx.outputDir, "report.json"), `${JSON.stringify(ctx.report, null, 2)}\n`, "utf8");
  if (ctx.options.onCheckpoint) await ctx.options.onCheckpoint(copy(ctx.report));
}

async function note(ctx, kind, details = {}) {
  const entry = { at: iso(), kind, ...details };
  ctx.current?.evidence.push(entry);
  await checkpoint(ctx);
  return entry;
}

async function state(ctx) {
  const rawUrl = await ctx.tab.url();
  invariant(rawUrl, "无法读取实际页面 URL");
  const actual = new URL(rawUrl);
  if (actual.origin !== ctx.origin) block(`页面已离开授权 origin：${actual.origin}`);
  // 只读取已呈现 DOM；不读取应用变量、存储、网络或调用页面函数。
  const dom = await ctx.tab.playwright.evaluate(() => {
    const text = selector => document.querySelector(selector)?.textContent?.trim() || "";
    const visible = node => !!node && !node.closest("[hidden]") && node.getClientRects().length > 0;
    const send = document.querySelector("#send");
    const prompt = document.querySelector("#prompt");
    const rows = [...document.querySelectorAll(".thread-row[data-thread-id]")];
    const turns = [...document.querySelectorAll("#messages .turn[data-turn-id]")].map(node => ({
      id: node.dataset.turnId,
      users: [...node.querySelectorAll(".user-message")].map(message => ({
        key: message.dataset.key || "", text: message.textContent || "",
      })),
      agents: [...node.querySelectorAll(".agent-message")].map(message => ({
        key: message.dataset.key || "", text: message.textContent || "",
      })),
      stopped: !!node.querySelector(".turn-end"),
      error: node.querySelector(".turn-error")?.textContent || "",
      processing: [...node.querySelectorAll(".activity-summary")].some(summary =>
        visible(summary) && summary.getAttribute("aria-label")?.endsWith("，仍在处理")),
    }));
    return {
      title: text("#thread-title"), path: text("#thread-path"),
      loading: visible(document.querySelector(".initial-loading"))
        || visible(document.querySelector('#messages .empty-conversation[role="status"]'))
        || text("#thread-path").includes("正在读取会话设置"),
      connection: text("#connection-state"), notice: text("#notice"),
      prompt: prompt?.innerText ?? prompt?.textContent ?? "",
      promptEditable: !!prompt && prompt.getAttribute("contenteditable") === "true" && prompt.getAttribute("aria-disabled") !== "true",
      send: { action: send?.dataset.action, label: send?.getAttribute("aria-label"), enabled: !!send && !send.disabled },
      model: text("#model-button"), relatedModel: text("#related-session-model"),
      optimizer: {
        state: document.querySelector("#prompt-optimize")?.dataset.state,
        label: document.querySelector("#prompt-optimize")?.getAttribute("aria-label"),
        enabled: !!document.querySelector("#prompt-optimize") && !document.querySelector("#prompt-optimize").disabled,
      },
      context: {
        ring: text("#context-info .context-usage-ring"),
        label: document.querySelector("#context-info")?.getAttribute("aria-label") || "",
        tooltip: text("#context-usage-tooltip"),
      },
      search: document.querySelector("#sidebar-search")?.value || "",
      sidebarMode: document.querySelector("#sidebar-view-toggle")?.dataset.mode || "",
      projectGroups: [...document.querySelectorAll(".project-group")].map(node => ({
        title: node.querySelector(".project-label")?.textContent || "",
        menuLabel: node.querySelector(".row-menu-button")?.getAttribute("aria-label") || "",
        pinned: node.closest(".sidebar-section-body")?.previousElementSibling
          ?.querySelector(".section-label-text")?.textContent?.trim() === "置顶",
        threadIds: [...node.querySelectorAll("[data-thread-id]")].map(row => row.dataset.threadId),
      })),
      selectedSkills: [...document.querySelectorAll("#prompt .inline-skill")].map(node => ({
        text: node.textContent || "", title: node.getAttribute("title") || "",
      })),
      goalComposerTag: visible(document.querySelector("#prompt .inline-goal")),
      attachments: [...document.querySelectorAll("#composer-attachments img")].map(node => ({
        alt: node.getAttribute("alt") || "", loaded: node.complete && node.naturalWidth > 0,
      })),
      messageImages: [...document.querySelectorAll("#messages .user-message img")].map(node => ({
        alt: node.getAttribute("alt") || "", loaded: node.complete && node.naturalWidth > 0,
      })),
      approvalText: text("#approval-area"), pendingText: text("#pending-area"), goalText: text("#goal-area"),
      expired: visible(document.querySelector("#side-thread-expired")),
      editor: document.querySelector(".message-editor-input")?.value ?? null,
      canCreate: !!document.querySelector("#new-chat") && !document.querySelector("#new-chat").disabled,
      related: visible(document.querySelector("#related-session-back")),
      parentMenuUnread: document.querySelector("#open-session-tools")?.classList.contains("has-unread-side-thread") || false,
      editing: document.querySelector("#send")?.dataset.queuedEditState === "editing",
      queueEditBanner: visible(document.querySelector("#edit-message-area")),
      queueEditCancel: [...document.querySelectorAll("#composer button")].some(node =>
        visible(node) && node.textContent?.trim() === "取消编辑"),
      confirmation: visible(document.querySelector("#confirmation-dialog"))
        ? { title: text("#confirmation-title"), message: text("#confirmation-message") } : null,
      menuItems: [...document.querySelectorAll('[role="menuitem"]')].filter(visible).map(node => ({
        name: node.getAttribute("aria-label") || node.textContent?.trim() || "", visible: true,
        enabled: !node.disabled && node.getAttribute("aria-disabled") !== "true",
      })),
      sidebar: rows.map(node => ({
        id: node.dataset.threadId, title: node.querySelector(".thread-label")?.textContent || "",
        selected: node.classList.contains("selected"), visible: visible(node),
        unread: !!node.querySelector('.thread-state[aria-label="有未读消息"]'),
        active: !!node.querySelector('.thread-state[aria-label="正在运行"]'),
      })),
      sideEntries: [...document.querySelectorAll('[data-menu-key^="side:"]')].map(node => ({
        id: node.dataset.menuKey.slice(5), title: node.textContent || "", visible: visible(node),
        unread: !!node.querySelector('[aria-label="有未读消息"]'),
        active: !!node.querySelector('[aria-label="正在执行"]'),
      })),
      queue: [...document.querySelectorAll("#queued-area .queued-message")].map(node => ({
        id: node.dataset.messageId, text: node.querySelector(".queued-message-text")?.textContent || "",
        steerEnabled: !node.querySelector(".queued-steer-button")?.disabled,
      })),
      turns,
    };
  });
  // 浏览器插件返回值可能来自另一 VM；证据按 JSON 值比较，不保留运行时原型。
  return copy({
    ...dom, url: safeUrl(rawUrl),
    threadId: new URLSearchParams(actual.hash.slice(1)).get("thread") || null,
    at: iso(),
  });
}

async function capture(ctx, label) {
  const sequence = ++ctx.report.evidenceSequence;
  const prefix = `${String(sequence).padStart(4, "0")}-${ctx.current?.id || "run"}-${label}`.replace(/[^a-zA-Z0-9._-]/g, "_");
  const entry = { kind: "ui", at: iso(), label, files: {} };
  try {
    const snapshot = await ctx.tab.playwright.domSnapshot();
    const file = `${prefix}.dom.txt`;
    await writeFile(join(ctx.outputDir, file), snapshot, "utf8");
    entry.files.dom = await createEvidenceReference(join(ctx.outputDir, file));
  } catch (error) { entry.domError = error.message; }
  try {
    const bytes = await ctx.tab.screenshot({ fullPage: false });
    const format = screenshotFileType(bytes);
    const file = `${prefix}.${format.extension}`;
    await writeFile(join(ctx.outputDir, file), bytes);
    entry.files.screenshot = { ...await createEvidenceReference(join(ctx.outputDir, file)), mimeType: format.mimeType };
  } catch (error) { entry.screenshotError = error.message; }
  try {
    entry.observation = await state(ctx);
    entry.url = entry.observation.url; entry.threadId = entry.observation.threadId;
  } catch (error) { entry.stateError = error.message; }
  ctx.current?.evidence.push(entry);
  await checkpoint(ctx);
  return entry;
}

async function wait(ctx, description, predicate, timeoutMs = ctx.options.timeoutMs ?? 45_000) {
  const started = Date.now();
  let last, nextProgress = started + 10_000;
  while (Date.now() - started < timeoutMs) {
    last = await state(ctx);
    if (await predicate(last)) return last;
    if (Date.now() >= nextProgress) {
      await note(ctx, "waiting", { description, elapsedMs: Date.now() - started, threadId: last.threadId, url: last.url });
      if (ctx.options.onProgress) await ctx.options.onProgress({ caseId: ctx.current.id, description, elapsedMs: Date.now() - started });
      nextProgress = Date.now() + 10_000;
    }
    await new Promise(done => setTimeout(done, ctx.options.pollMs ?? 250));
  }
  await note(ctx, "timeout", { description, elapsedMs: Date.now() - started, last });
  throw new Error(`等待超时：${description}`);
}

/** reload 命令返回不代表新文档已装载；先登记导航等待，避免读到旧页面而误判刷新成功。 */
export async function reloadRemoteAccessPage(tab, { timeoutMs = 30_000 } = {}) {
  if (typeof tab?.reload !== "function" || typeof tab?.playwright?.expectNavigation !== "function")
    block("当前浏览器工具缺少新文档导航等待能力，不能验证整页刷新");
  await tab.playwright.expectNavigation(() => tab.reload(), {
    timeoutMs, waitUntil: "domcontentloaded",
  });
}

async function reloadPage(ctx) {
  await reloadRemoteAccessPage(ctx.tab, { timeoutMs: ctx.options.timeoutMs ?? 45_000 });
  await wait(ctx, "新文档的会话与历史加载结束", value => !value.loading);
}

async function click(ctx, locator) {
  if (ctx.actionContext) assertOwnedState(ctx, await state(ctx), ctx.actionContext);
  await locator.click({ timeoutMs: ctx.options.actionTimeoutMs ?? 10_000 });
}

function permit(ctx, name) {
  if (ctx.options.authorization?.[name] !== true) block(`缺少显式运行授权：authorization.${name}`);
}

async function action(ctx, label, details, perform) {
  const id = `${ctx.report.runId}:${ctx.current.id}:${label}`;
  const existing = ctx.report.actions[id];
  if (existing?.status === "confirmed") return existing.result;
  if (existing) block(`操作 ${id} 已进入 ${existing.status}，需人工核对原结果，不能在恢复时重发`);
  const record = { id, caseId: ctx.current.id, status: "unknown", startedAt: iso(), ...details };
  const contextThreadId = details.contextThreadId || details.threadId;
  const actionContext = contextThreadId
    ? Object.values(ctx.report.threads).find(value => value.id === contextThreadId && !value.deleted) : null;
  if (contextThreadId) {
    if (!actionContext) block("动作目标不属于本轮脚本创建的测试数据");
    assertOwnedState(ctx, await state(ctx), actionContext);
  }
  ctx.report.actions[id] = record;
  // 在发出 UI 动作之前落盘；进程或页面丢失后保留 unknown，禁止盲目重试。
  await checkpoint(ctx);
  ctx.actionContext = actionContext;
  try {
    const result = await perform();
    record.status = "confirmed"; record.finishedAt = iso(); record.result = result ?? null;
    await note(ctx, "action", { actionId: id, ...details, result: record.result });
    return record.result;
  } catch (error) {
    record.error = error.message;
    await checkpoint(ctx);
    throw error;
  } finally { ctx.actionContext = null; }
}

async function verifyNative(ctx, expectations = {}) {
  const actions = Object.values(ctx.report.actions).filter(value => value.caseId === ctx.current.id
    && (!expectations.actionsForMarker || value.marker === expectations.actionsForMarker)
    && (!expectations.actionIds || expectations.actionIds.includes(value.id)));
  const request = {
    runId: ctx.report.runId, caseId: ctx.current.id, origin: ctx.origin,
    runStartedAt: ctx.report.runStartedAt, sourceFingerprint: ctx.report.sourceFingerprint,
    actions: copy(actions), ...expectations,
  };
  ctx.current.nativeExpectations = copy(expectations);
  ctx.current.pendingNativeChecks ||= [];
  const signature = createHash("sha256").update(JSON.stringify(request)).digest("hex");
  if (!ctx.current.pendingNativeChecks.some(value => value.signature === signature))
    ctx.current.pendingNativeChecks.push({ signature, request, verified: false });
  await checkpoint(ctx);
}

async function verifyNativeActionsNow(ctx, {
  actions, threadIds = [], turnIds = [], expectedFacts = {}, expectations = [],
} = {}) {
  if (typeof ctx.options.nativeEvidence !== "function")
    block("停止／失败队列验收需要 nativeEvidence 只读核对原 queue-send 回执和原生轮次");
  const request = {
    runId: ctx.report.runId, caseId: ctx.current.id, origin: ctx.origin,
    runStartedAt: ctx.report.runStartedAt, sourceFingerprint: ctx.report.sourceFingerprint,
    actions: copy(actions), threadIds, turnIds, expectedFacts, expectations,
  };
  const records = await ctx.options.nativeEvidence(copy(request));
  if (records?.status === "blocked") block(records.reason || "原生回执尚未确认");
  invariant(Array.isArray(records) && records.length > 0, "nativeEvidence 未返回原始操作回执");
  const verified = await verifyNativeEvidence(records, request, { baseDir: ctx.outputDir });
  await note(ctx, "native", { records, request, expectations: request });
  return verified;
}

async function flushNativeChecks(ctx) {
  const checks = ctx.current.pendingNativeChecks || [];
  for (const check of checks.filter(value => !value.verified)) {
    const request = check.request;
    if (!ctx.options.nativeEvidence) block("UI 步骤已留证，但缺少 nativeEvidence 只读核验回调，原生执行及回执尚未确认");
    const records = await ctx.options.nativeEvidence(copy(request));
    if (records?.status === "blocked") block(records.reason || "原生回执尚未确认");
    invariant(Array.isArray(records) && records.length > 0, "原生核验没有返回证据记录");
    await verifyNativeEvidence(records, request, { baseDir: ctx.outputDir });
    check.verified = true;
    await note(ctx, "native", { records, request, expectations: request });
  }
  if (checks.length) ctx.current.nativeVerified = true;
  await checkpoint(ctx);
}

function owned(ctx, key) {
  const record = ctx.report.threads[key];
  if (!record || record.runId !== ctx.report.runId
    || !["remote-access-e2e", "external-created-this-run"].includes(record.createdBy) || record.deleted)
    block(`缺少本轮脚本创建的有效测试会话：${key}`);
  const creation = ctx.report.actions[record.creationActionId];
  invariant(creation?.status === "confirmed", `测试会话 ${key} 的创建操作未确认`);
  return record;
}

function jsonPointer(value, pointer) {
  invariant(typeof pointer === "string" && pointer.startsWith("/"), "原始回执字段必须使用 JSON Pointer");
  return pointer.slice(1).split("/").reduce((current, key) => current?.[key.replaceAll("~1", "/").replaceAll("~0", "~")], value);
}

/**
 * 显式接入同一轮由外层真实 UI 创建的测试会话。读取原始 JSON 回执并核对 DOM，
 * 不接受 created=true / verified=true 等信任布尔值，也不发送标记消息。
 */
export async function adoptRemoteAccessE2EThread(ctx, input) {
  invariant(ctx?.[contextTag] && !ctx.current, "接入测试对象必须在上下文的案例执行之间显式调用");
  invariant(["main", "control"].includes(input?.key), "只支持接入本轮普通 main/control 测试会话");
  invariant(typeof input.threadId === "string" && input.threadId.length > 0, "缺少明确 thread ID");
  invariant(typeof input.marker === "string" && input.marker.includes(ctx.report.runId),
    "外部测试消息标记必须包含当前唯一 runId");
  if (ctx.report.threads[input.key]) {
    const existing = owned(ctx, input.key);
    invariant(existing.id === input.threadId && existing.marker === input.marker, "不能替换已登记的测试对象");
    return existing;
  }
  invariant(!Object.values(ctx.report.threads).some(value => value.id === input.threadId), "同一会话不能登记为两个测试对象");
  const proof = input.creationEvidence;
  invariant(proof?.path && ["rpc", "bridge", "native-log"].includes(proof.source),
    "必须提供原始创建回执的本机 JSON 文件和来源");
  invariant(typeof proof.outcomePointer === "string" && proof.rawFacts,
    "v2 接入需显式 outcomePointer（平铺回执为 \"\"）和 rawFacts 原始来源/源码指纹等字段指针");
  const rawText = await readFile(proof.path, "utf8");
  const receipt = JSON.parse(rawText.replace(/^\uFEFF/, ""));
  const actionName = proof.actionPointer ? jsonPointer(receipt, proof.actionPointer) : null;
  const receiptId = jsonPointer(receipt, proof.receiptIdPointer);
  const actualThreadId = jsonPointer(receipt, proof.threadIdPointer);
  const originalPrompt = proof.promptPointer ? jsonPointer(receipt, proof.promptPointer) : null;
  const rawSubmittedAt = jsonPointer(receipt, proof.submittedAtPointer);
  const submittedAt = typeof rawSubmittedAt === "number"
    ? new Date(rawSubmittedAt).toISOString() : rawSubmittedAt;
  const requestThreadId = proof.requestThreadIdPointer ? jsonPointer(receipt, proof.requestThreadIdPointer) : null;
  const creationResult = receipt.result ?? receipt;
  invariant(["create", "createThread"].includes(actionName)
    || creationResult.status === "created" && creationResult.firstTurn?.status === "accepted",
  "原始动作必须创建普通会话，不能用已有会话的后续发送回执");
  invariant((typeof receiptId === "string" && receiptId.length > 0 || typeof receiptId === "number" && Number.isFinite(receiptId))
    && actualThreadId === input.threadId,
    "原始创建回执的 receipt ID 或结果 thread ID 不匹配");
  invariant(requestThreadId == null || requestThreadId === "", "原请求指定了已有 thread ID，不能视为本轮创建证明");
  if (proof.promptPointer) invariant(typeof originalPrompt === "string" && originalPrompt.includes(input.marker),
    "原始创建请求未包含本轮完整唯一标记");
  const runStartedAt = Date.parse(ctx.report.runStartedAt || ctx.report.startedAt);
  invariant(Number.isFinite(runStartedAt) && Number.isFinite(Date.parse(submittedAt))
    && Date.parse(submittedAt) >= runStartedAt && Date.parse(submittedAt) <= Date.now() + 5_000,
  "创建时间不属于明确的本轮时间范围；外部先创建时需传入实际 runStartedAt");
  const observation = await state(ctx);
  invariant(observation.threadId === input.threadId && observation.turns.some(turn =>
    turn.users.some(user => user.text.includes(input.marker))),
  "当前真实页面没有同时呈现准确 thread ID 和原始测试消息标记");
  if (input.title) invariant(observation.title === input.title, "当前可见会话标题与显式接入标题不一致");
  const id = `${ctx.report.runId}:adopt:${input.key}`;
  const nativeRecord = {
    source: proof.source, evidencePath: resolve(proof.path),
    ...(await createEvidenceReference(proof.path)),
    actionId: id, threadId: input.threadId, receiptId, outcomePointer: proof.outcomePointer,
    rawFacts: {
      ...proof.rawFacts, threadId: proof.threadIdPointer, receiptId: proof.receiptIdPointer,
      capturedAt: proof.submittedAtPointer,
      ...(proof.actionPointer ? { operation: proof.actionPointer } : {}),
      ...(proof.promptPointer ? { prompt: proof.promptPointer } : {}),
      ...(proof.requestThreadIdPointer ? { requestThreadId: proof.requestThreadIdPointer } : {}),
    },
  };
  const creationAction = {
    id, caseId: "external.adopt", kind: "external-created-this-run", threadId: input.threadId,
    status: "confirmed", startedAt: submittedAt, finishedAt: iso(),
    result: { threadId: actualThreadId, receiptId, source: proof.source, evidencePath: proof.path },
  };
  await verifyNativeEvidence([nativeRecord], {
    runId: ctx.report.runId, caseId: "external.adopt", origin: ctx.origin,
    sourceFingerprint: ctx.report.sourceFingerprint, runStartedAt: ctx.report.runStartedAt,
    actions: [creationAction], threadIds: [input.threadId],
    ...(proof.promptPointer ? { expectations: [fact("prompt", input.marker, "includes")] } : {}),
  }, { baseDir: ctx.outputDir });
  const record = {
    id: input.threadId, key: input.key, marker: input.marker, title: observation.title, kind: "main",
    runId: ctx.report.runId, createdBy: "external-created-this-run", createdAt: submittedAt,
    creationActionId: id, creationEvidencePath: proof.path, originalReceiptId: receiptId,
  };
  ctx.report.actions[id] = creationAction;
  ctx.report.threads[input.key] = record;
  await checkpoint(ctx);
  const evidence = await capture(ctx, `adopt-${input.key}`);
  if (!evidence.files.dom || !evidence.files.screenshot || !evidence.observation) {
    delete ctx.report.threads[input.key]; delete ctx.report.actions[id];
    await checkpoint(ctx);
    block("外部测试对象的身份截图或 DOM 留证失败，未取得后续修改权限");
  }
  ctx.report.adoptions ||= [];
  ctx.report.adoptions.push({
    at: iso(), key: input.key, threadId: input.threadId, marker: input.marker, source: proof.source,
    receiptId, evidencePath: proof.path, record: nativeRecord, actionId: id, uiEvidence: evidence,
  });
  await checkpoint(ctx);
  return record;
}

function assertOwnedState(ctx, observation, record) {
  if (observation.threadId !== record.id) block(`页面当前会话与测试对象不一致，预期 ${record.id}，实际 ${observation.threadId}`);
  if (!observation.turns.some(turn => turn.users.some(user => user.text.includes(record.marker))))
    block(`当前会话尚未呈现本轮唯一标记 ${record.marker}，停止变更`);
}

async function ensureOwnedMarker(ctx, record) {
  let observation = await state(ctx);
  for (let page = 0; page < 20; page++) {
    if (observation.threadId !== record.id) block("读取测试标记时会话已切换");
    if (observation.turns.some(turn => turn.users.some(user => user.text.includes(record.marker))))
      return observation;
    const earlier = ctx.tab.playwright.getByRole("button", { name: "加载更早的消息", exact: true });
    if (!await earlier.isVisible()) {
      observation = await state(ctx);
      if (observation.threadId === record.id && observation.turns.some(turn =>
        turn.users.some(user => user.text.includes(record.marker)))) return observation;
      block("当前已加载历史没有本轮测试标记");
    }
    const count = observation.turns.length;
    try { await earlier.click(); }
    catch (error) {
      observation = await state(ctx);
      if (observation.threadId === record.id && observation.turns.some(turn =>
        turn.users.some(user => user.text.includes(record.marker)))) return observation;
      throw error;
    }
    observation = await wait(ctx, "加载更早的测试消息", value => value.threadId === record.id
      && (value.turns.length > count || value.turns.some(turn => turn.users.some(user => user.text.includes(record.marker)))));
  }
  block("超过本轮测试历史分页上限");
}

async function openSidebar(ctx) {
  const control = ctx.tab.playwright.getByRole("button", { name: "打开侧边栏", exact: true });
  if (await control.isVisible()) await click(ctx, control);
  await ctx.tab.playwright.domSnapshot();
}

async function selectOwned(ctx, key) {
  const record = owned(ctx, key);
  let observation = await state(ctx);
  if (record.kind === "side" && observation.threadId !== record.id) return openSide(ctx, key);
  if (observation.threadId !== record.id) {
    if (observation.confirmation) block("页面仍停留在确认界面，先核对该操作后再切换");
    if (observation.prompt.trim() && !observation.prompt.includes(`[remote-e2e:${ctx.report.runId}:`))
      block("输入框有非本轮草稿，停止切换以保留用户输入");
    await openSidebar(ctx);
    observation = await wait(ctx, `侧栏同步显示本轮测试会话 ${key}`,
      value => value.sidebar.some(row => row.id === record.id && row.visible));
    if (observation.confirmation) block("等待侧栏期间出现确认界面，停止切换");
    if (observation.prompt.trim() && !observation.prompt.includes(`[remote-e2e:${ctx.report.runId}:`))
      block("等待侧栏期间出现非本轮草稿，停止切换以保留用户输入");
    const row = ctx.tab.playwright.locator(`.thread-row[data-thread-id="${css(record.id)}"]`).filter({ visible: true }).first();
    if (!await row.count()) block(`侧栏未显示测试会话 ${record.id}；请通过 UI 展开其项目或移除搜索条件后恢复`);
    await click(ctx, row.locator(".thread-link", {}));
    observation = await wait(ctx, `选中测试会话 ${key}`, value => value.threadId === record.id && value.promptEditable);
  }
  observation = await ensureOwnedMarker(ctx, record);
  assertOwnedState(ctx, observation, record);
  return observation;
}

async function idle(ctx, key = "main") {
  await selectOwned(ctx, key);
  return wait(ctx, "实际轮次结束且没有待发送队列", value => value.send.action !== "stop" && value.send.action !== "sending"
    && value.queue.length === 0 && value.promptEditable, ctx.options.turnTimeoutMs ?? 180_000);
}

function message(ctx, key, template) {
  const expected = ack(ctx, key);
  const content = (template
    ? template.replaceAll("{ack}", expected).replaceAll("{runId}", ctx.report.runId)
    : `你正在参加远程会话端到端回归。仅处理本条回归消息；不得读取或修改文件、联网、调用工具或创建子任务。只回复以下校验词，不添加解释：${expected}`).replace(/\r\n?/g, "\n");
  return { key, marker: marker(ctx, key), expected, text: `${content}\n\n回归标识：${marker(ctx, key)}` };
}

async function send(ctx, spec, { key = "main", creating = false, expectedThreadId = null, queued = false,
  prepared = false, compactionQueue = false } = {}) {
  permit(ctx, "allowMessages");
  const limit = ctx.options.authorization?.maxMessageSubmissions;
  if (!Number.isInteger(limit) || limit < 1) block("必须显式设置正整数 maxMessageSubmissions");
  const label = `send-${spec.key}`;
  const id = `${ctx.report.runId}:${ctx.current.id}:${label}`;
  if (!ctx.report.actions[id] && ctx.report.messageSubmissions >= limit) block(`已达到本轮消息提交上限 ${limit}`);
  const record = creating ? null : owned(ctx, key);
  if (!creating) assertOwnedState(ctx, await state(ctx), record);
  const initial = await state(ctx);
  if (creating) invariant(expectedThreadId
    ? initial.threadId === expectedThreadId
    : !initial.threadId && initial.title === "新建会话", "创建或新对象标记发送时页面身份不匹配");
  if (initial.prompt.trim() && initial.prompt.trim() !== spec.text.trim())
    block("输入框有其他草稿，停止覆盖");
  if (queued && (compactionQueue ? initial.send.label !== "发送到原生消息队列" : initial.send.action !== "stop"))
    block(compactionQueue ? "当前并非真实压缩队列发送状态" : "真实活动轮已结束，不能把空闲发送算作排队");
  const baseline = initial.turns.map(turn => turn.id);
  const result = await action(ctx, label, {
    kind: queued ? "queue-send" : creating && !expectedThreadId ? "create-send" : "send",
    threadId: record?.id || null, marker: spec.marker, expected: spec.expected, prompt: spec.text,
  }, async () => {
    ctx.report.messageSubmissions++;
    await checkpoint(ctx);
    if (!prepared) await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill(spec.text);
    const filled = await state(ctx);
    if (record) assertOwnedState(ctx, filled, record);
    if (creating) invariant(expectedThreadId
      ? filled.threadId === expectedThreadId
      : !filled.threadId && filled.title === "新建会话", "填写消息后页面已离开本轮新对象，停止发送");
    invariant(filled.prompt.includes(spec.marker), "真实输入框未包含回归标记");
    const target = ctx.tab.playwright.getByRole("button", { name: compactionQueue ? "发送到原生消息队列" : "发送消息", exact: true });
    invariant(await target.isEnabled(), "发送按钮不可用");
    await click(ctx, target);
    const after = await wait(ctx, queued ? "原生队列中出现唯一回归消息" : "实际会话出现唯一用户消息", value => queued
      ? value.threadId === record.id && value.queue.some(row => row.text.includes(spec.marker))
      : !!value.threadId && value.turns.some(turn => turn.users.some(user => user.text.includes(spec.marker))));
    const queue = queued ? after.queue.filter(row => row.text.includes(spec.marker)) : [];
    invariant(!expectedThreadId || after.threadId === expectedThreadId, "首条侧边消息实际发送结果属于其他 thread ID");
    invariant(!queued || queue.length === 1, "同一标记出现多条队列消息");
    return { threadId: after.threadId, queueId: queue[0]?.id || null, baselineTurns: baseline, at: after.at, url: after.url };
  });
  return { ...spec, ...result, actionId: id };
}

function userTurn(value, spec) {
  return value.turns.find(turn => turn.users.some(user => user.text.includes(spec.marker)));
}

export function findQueueSteeringTurn(observation) {
  const running = (observation?.turns || []).filter(turn => turn.processing === true
    && typeof turn.id === "string" && turn.id && !turn.id.startsWith("pending-"));
  return running.length === 1 ? running[0] : null;
}

function replied(value, spec) {
  const turn = userTurn(value, spec);
  return turn && !turn.id.startsWith("pending-") && !turn.error
    && turn.agents.some(agent => agent.text.includes(spec.expected));
}

async function finalReply(ctx, spec) {
  return wait(ctx, `原生轮次最终回复 ${spec.key}`, value => replied(value, spec)
    && value.send.action !== "stop" && value.send.action !== "sending", ctx.options.turnTimeoutMs ?? 180_000);
}

// 仅定位 UI 中的回答轮；接受事实仍须由相同问题/轮次的原生证据确认。
export function findQuestionAnswerTurn(value, spec, { originatingTurnId, beforeTurnIds, completedOrigin }) {
  if (value.threadId !== spec.threadId || value.loading
      || ["stop", "sending"].includes(value.send?.action)) return null;
  const before = new Set(beforeTurnIds);
  const matches = value.turns.filter(turn => !turn.id.startsWith("pending-") && !turn.error
    && (!before.has(turn.id) || !completedOrigin && turn.id === originatingTurnId)
    && turn.agents.some(agent => agent.text.includes(spec.expected)));
  return matches.length === 1 ? matches[0] : null;
}

async function chooseModel(ctx) {
  if (!ctx.options.model?.name) block("需传入从真实模型菜单观察得到的 model.name，避免默认使用未经确认的模型");
  const before = await state(ctx);
  const owner = before.threadId ? Object.values(ctx.report.threads).find(value => value.id === before.threadId && !value.deleted) : null;
  if (before.threadId) {
    if (!owner) block("当前模型菜单不属于本轮测试会话");
    assertOwnedState(ctx, before, owner);
  } else invariant(before.title === "新建会话", "模型选择时没有处于新建测试会话界面");
  await click(ctx, ctx.tab.playwright.getByRole("button", { name: "选择模型与思考深度", exact: true }));
  await ctx.tab.playwright.domSnapshot();
  const search = ctx.tab.playwright.getByPlaceholder("搜索模型", { exact: true });
  if (await search.count()) await search.fill(ctx.options.model.name);
  const item = ctx.tab.playwright.getByRole("menuitemradio", { name: ctx.options.model.name, exact: true });
  if (await item.count() !== 1 || !await item.isEnabled()) block("真实模型菜单没有唯一可用的目标模型");
  const current = await state(ctx);
  if (owner) assertOwnedState(ctx, current, owner);
  else invariant(!current.threadId && current.title === "新建会话", "选择模型前新建页面身份已变化");
  await click(ctx, item);
  await wait(ctx, "模型按钮显示所选模型", value => value.model.includes(ctx.options.model.name));
}

async function createOne(ctx, key) {
  permit(ctx, "allowCreate");
  if (ctx.report.threads[key]) {
    const record = owned(ctx, key);
    if (record.createdBy === "remote-access-e2e") {
      await selectOwned(ctx, key);
      const original = ctx.report.actions[record.creationActionId];
      await finalReply(ctx, { marker: record.marker, expected: original.expected, key });
    }
    return record;
  }
  const before = await state(ctx);
  if (before.related) block("先通过 UI 返回主会话，再创建普通测试会话");
  if (before.prompt.trim()) block("输入框有已有草稿，不能开始创建测试会话");
  await openSidebar(ctx);
  await click(ctx, ctx.tab.playwright.locator("#new-chat"));
  await wait(ctx, "进入新建会话界面", value => !value.threadId && value.title === "新建会话");
  if (ctx.options.projectName) {
    await click(ctx, ctx.tab.playwright.locator("#project-select"));
    await ctx.tab.playwright.domSnapshot();
    await click(ctx, ctx.tab.playwright.getByRole("option", { name: ctx.options.projectName, exact: true }));
  }
  await chooseModel(ctx);
  const spec = await send(ctx, message(ctx, key), { creating: true });
  invariant(!ctx.report.initialThreadIds.includes(spec.threadId), "创建操作选中了运行前已存在的会话");
  invariant(!Object.values(ctx.report.threads).some(value => value.id === spec.threadId), "两个测试对象指向同一个会话");
  const observation = await state(ctx);
  const record = {
    id: spec.threadId, key, marker: spec.marker, runId: ctx.report.runId, createdBy: "remote-access-e2e",
    creationActionId: spec.actionId, createdAt: iso(), title: observation.title, kind: "main",
  };
  ctx.report.threads[key] = record;
  await checkpoint(ctx);
  await finalReply(ctx, spec);
  record.title = (await state(ctx)).title;
  await checkpoint(ctx);
  return record;
}

async function refreshOwned(ctx, key) {
  const record = owned(ctx, key);
  assertOwnedState(ctx, await state(ctx), record);
  await click(ctx, ctx.tab.playwright.locator("#connection-state"));
  const observation = await wait(ctx, "刷新后仍显示相同会话及本轮标记", value => value.threadId === record.id
    && value.promptEditable);
  await ensureOwnedMarker(ctx, record);
  await note(ctx, "refresh", { threadId: record.id, url: observation.url, model: observation.model });
  return observation;
}

async function activeWindow(ctx, key) {
  const saved = ctx.report.windows[key];
  if (saved) {
    await selectOwned(ctx, "main");
    const observation = await state(ctx);
    if (observation.send.action !== "stop" || observation.turns.at(-1)?.id !== saved.turnId)
      block(`活动轮窗口 ${key} 已结束或切换轮次；不能重新创建或手动暂停自动轮次冒充原场景`);
    return saved;
  }
  const template = ctx.options.messages?.active;
  if (!template) block("缺少经用户批准的 messages.active；需产生足够长的真实活动轮，脚本不会设置 deferAutomaticTurns 或暂停自动轮次");
  await idle(ctx);
  const spec = await send(ctx, message(ctx, `active-${key}`, template));
  const observation = await wait(ctx, "真实活动轮仍在执行", value => value.send.action === "stop"
    && !!userTurn(value, spec) && !userTurn(value, spec).id.startsWith("pending-"));
  const record = { ...spec, turnId: userTurn(observation, spec).id };
  ctx.report.windows[key] = record;
  await note(ctx, "active-window", record);
  return record;
}

async function queueRef(ctx, name) {
  permit(ctx, "allowQueueChanges");
  const spec = ctx.report.queue[name];
  if (!spec) block(`没有本轮创建的队列对象 ${name}，请先运行 queue.edit_draft`);
  await selectOwned(ctx, "main");
  const observation = await state(ctx);
  const matches = observation.queue.filter(row => row.text.includes(spec.marker));
  if (!matches.length && observation.send.action !== "stop") block(`真实活动轮已结束，队列对象 ${name} 可能已自动执行，需另开 runId 重建窗口`);
  invariant(matches.length === 1, `无法唯一定位本轮队列对象 ${name}`);
  spec.queueId = matches[0].id;
  return { spec, row: matches[0], observation };
}

function queueLocator(ctx, id) {
  return ctx.tab.playwright.locator(`.queued-message[data-message-id="${css(id)}"]`);
}

async function queueMenu(ctx, id) {
  await click(ctx, queueLocator(ctx, id).getByRole("button", { name: "排队消息操作", exact: true }));
  await ctx.tab.playwright.domSnapshot();
}

async function ensureMenu(ctx) {
  const button = ctx.tab.playwright.locator("#open-session-tools");
  if (await button.getAttribute("aria-expanded") !== "true") await click(ctx, button);
  await ctx.tab.playwright.domSnapshot();
}

async function openSide(ctx, key = "side") {
  const side = owned(ctx, key);
  const main = Object.values(ctx.report.threads).find(value => value.id === side.parentThreadId && !value.deleted);
  invariant(main, "侧边会话没有本轮已登记父会话");
  let observation = await state(ctx);
  if (observation.threadId === side.id) { assertOwnedState(ctx, observation, side); return observation; }
  await selectOwned(ctx, main.key);
  await ensureMenu(ctx);
  await click(ctx, ctx.tab.playwright.locator(`[data-menu-key="side:${css(side.id)}"]`));
  observation = await wait(ctx, "切换到本轮侧边会话", value => value.threadId === side.id && value.related
    && value.turns.some(turn => turn.users.some(user => user.text.includes(side.marker))));
  assertOwnedState(ctx, observation, side);
  invariant(main.id === side.parentThreadId, "侧边会话登记的主会话不一致");
  return observation;
}

async function backToMain(ctx, key = "main") {
  const main = owned(ctx, key);
  await click(ctx, ctx.tab.playwright.getByRole("button", { name: "返回主会话", exact: true }));
  return wait(ctx, "返回本轮主会话", value => value.threadId === main.id && !value.related);
}

async function rowMenu(ctx, key) {
  const record = owned(ctx, key);
  await openSidebar(ctx);
  const row = ctx.tab.playwright.locator(`.thread-row[data-thread-id="${css(record.id)}"]`).filter({ visible: true }).first();
  if (!await row.count()) block(`侧栏中未显示 ${key}，请展开该项目后恢复`);
  await click(ctx, row.locator(".row-menu-button", {}));
  await ctx.tab.playwright.domSnapshot();
}

async function closeRowMenu(ctx, key) {
  const record = owned(ctx, key);
  const toggle = ctx.tab.playwright.locator(`.thread-row[data-thread-id="${css(record.id)}"] .row-menu-button[aria-expanded="true"]`)
    .filter({ visible: true });
  invariant(await toggle.count() === 1, "缺少准确会话已展开的菜单按钮");
  await click(ctx, toggle);
  await wait(ctx, "已打开的会话菜单关闭", value => value.menuItems.length === 0);
}

async function deletionDialog(ctx, key = ctx.current?.deletionKey || "control") {
  const record = owned(ctx, key);
  const observation = await state(ctx);
  if (!observation.confirmation) {
    await selectOwned(ctx, key);
    record.title = (await state(ctx)).title;
    await rowMenu(ctx, key);
    await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "删除会话", exact: true }));
  }
  const result = await wait(ctx, "永久删除确认界面", value => !!value.confirmation);
  invariant(result.confirmation.title === "删除会话"
    && result.confirmation.message.includes(record.title)
    && result.confirmation.message.includes("永久删除")
    && result.confirmation.message.includes("不可恢复"), "删除确认内容没有明确标识目标及不可恢复风险");
  invariant(result.threadId === record.id, "删除确认时选中会话已变化");
  return { record, observation: result };
}

function readStateReady(ctx) {
  permit(ctx, "allowMessages");
  if (!ctx.options.messages?.unread) block("需提供已批准、能留出离开页面时间的 messages.unread；不会自行提交长轮次");
  if (typeof ctx.options.nativeEvidence !== "function") block("已读回归需 nativeEvidence 提供原生状态文件与 readStatePointers");
  const remaining = ctx.options.authorization?.maxMessageSubmissions - ctx.report.messageSubmissions;
  if (!Number.isFinite(remaining) || remaining < 2) block("已读案例至少需要两次显式批准的消息提交预算");
}

function rowReadState(observation, id, unread) {
  const rows = observation.sidebar.filter(row => row.id === id);
  return rows.length > 0 && rows.every(row => row.unread === unread && !row.active);
}

async function nativeReadState(ctx, expected) {
  const records = await ctx.options.nativeEvidence(copy({
    runId: ctx.report.runId, caseId: ctx.current.id, origin: ctx.origin,
    runStartedAt: ctx.report.runStartedAt, sourceFingerprint: ctx.report.sourceFingerprint,
    operation: "read-state", actions: [], expectedReadState: expected,
    timeoutMs: ctx.options.timeoutMs ?? 45_000,
  }));
  if (records?.status === "blocked") block(records.reason || "原生已读状态暂未确认");
  let proof = Array.isArray(records) && records.find(value => value.threadId === expected.threadId
    && ["rpc", "bridge", "native-log"].includes(value.source) && value.evidencePath
    && (value.readId || value.receiptId || value.eventId) && value.readStatePointers);
  if (!proof) block("缺少原生已读状态文件、读取标识或 readStatePointers，不能只用 verified 布尔值判定");
  const normalized = await verifyNativeEvidence(records, {
    runId: ctx.report.runId, caseId: ctx.current.id, origin: ctx.origin,
    sourceFingerprint: ctx.report.sourceFingerprint, runStartedAt: ctx.report.runStartedAt,
    operation: "read-state", actions: [], expectedReadState: expected,
  }, { baseDir: ctx.outputDir });
  proof = normalized.find(value => value.evidencePath === resolve(ctx.outputDir, proof.evidencePath)) || normalized.find(value =>
    value.threadId === expected.threadId && value.readStatePointers);
  const fields = ["threadId", "hasUnreadTurn", "turnId", "turnStatus",
    ...(expected.parentThreadId ? ["parentThreadId", "parentSideUnread"] : [])];
  if (fields.some(field => typeof proof.readStatePointers[field] !== "string"))
    block("原生已读证据缺少准确字段的 JSON Pointer");
  const original = await readFile(proof.evidencePath, "utf8");
  const raw = JSON.parse(original.replace(/^\uFEFF/, ""));
  const actual = Object.fromEntries(fields.map(field => [field, jsonPointer(raw, proof.readStatePointers[field])]));
  if (fields.some(field => actual[field] === undefined)) block("原始原生证据中没有所指的已读/轮次字段");
  if (Array.isArray(actual.parentSideUnread)) actual.parentSideUnread = actual.parentSideUnread
    .some(item => item?.id === expected.threadId && item.parentThreadId === expected.parentThreadId);
  invariant(actual.threadId === expected.threadId && actual.turnId === expected.turnId,
    "原生已读证据对应其他会话或其他轮次");
  invariant(actual.turnStatus === "completed", "原生轮次尚未正常完成，不能据未读徽标认定完成");
  invariant(actual.hasUnreadTurn === expected.hasUnreadTurn, "真实原生 hasUnreadTurn 与页面阶段不一致");
  if (expected.parentThreadId) invariant(actual.parentThreadId === expected.parentThreadId
    && actual.parentSideUnread === expected.parentSideUnread, "父会话的原生侧边未读状态与页面不一致");
  await note(ctx, "native-read-state", { expected, actual, evidencePath: proof.evidencePath, record: proof,
    source: proof.source, readId: proof.readId || proof.receiptId || proof.eventId,
    sha256: createHash("sha256").update(original).digest("hex") });
}

async function startUnreadTurn(ctx, key, cycle) {
  if (key === "side") await openSide(ctx);
  await idle(ctx, key);
  const spec = await send(ctx, message(ctx, `read-${key}-${cycle}`, ctx.options.messages.unread), { key });
  const observation = await wait(ctx, "等待可离开的真实活动轮", value => {
    const turn = userTurn(value, spec);
    return turn && !turn.id.startsWith("pending-") && (value.send.action === "stop" || replied(value, spec));
  });
  if (observation.send.action !== "stop")
    block("真实回复在离开前已完成；messages.unread 未提供足够时间，不能把已阅读回复当作未读场景");
  return { ...spec, turnId: userTurn(observation, spec).id };
}

async function mainReadState(ctx) {
  readStateReady(ctx);
  const main = owned(ctx, "main");
  await idle(ctx);
  if ((await state(ctx)).parentMenuUnread) block("主会话已有未读侧边消息，会混淆主会话自身未读标识");
  for (let cycle = 1; cycle <= 2; cycle++) {
    const spec = await startUnreadTurn(ctx, "main", cycle);
    const leaving = await state(ctx);
    assertOwnedState(ctx, leaving, main);
    if (leaving.prompt.trim()) block("离开活动轮前有其他草稿，停止切换");
    await openSidebar(ctx);
    await click(ctx, ctx.tab.playwright.locator("#new-chat"));
    await wait(ctx, "只进入新建空白页面而不创建实际会话", value => {
      if (!value.threadId && value.title === "新建会话" && value.prompt.trim())
        block("新建空白页面恢复了已有草稿；保留该草稿，停止本轮未读场景");
      return !value.threadId && value.title === "新建会话" && !value.prompt.trim();
    });
    await openSidebar(ctx);
    await wait(ctx, "离开后新回复产生主会话未读标识", value => {
      if (value.threadId || value.title !== "新建会话") block("等待未读时页面已离开新建空白页");
      return rowReadState(value, main.id, true);
    }, ctx.options.turnTimeoutMs ?? 180_000);
    await nativeReadState(ctx, { phase: `main-${cycle}-unread`, threadId: main.id,
      turnId: spec.turnId, hasUnreadTurn: true });
    await capture(ctx, `main-${cycle}-unread`);
    await selectOwned(ctx, "main");
    await wait(ctx, "打开主会话后未读徽标清除且回复真实存在", value => replied(value, spec)
      && rowReadState(value, main.id, false));
    await nativeReadState(ctx, { phase: `main-${cycle}-opened`, threadId: main.id,
      turnId: spec.turnId, hasUnreadTurn: false });
    await reloadPage(ctx);
    await wait(ctx, "整页刷新后主会话已读保持", value => value.threadId === main.id
      && replied(value, spec) && rowReadState(value, main.id, false));
    await ensureOwnedMarker(ctx, main);
    await nativeReadState(ctx, { phase: `main-${cycle}-reloaded`, threadId: main.id,
      turnId: spec.turnId, hasUnreadTurn: false });
    await capture(ctx, `main-${cycle}-read-after-reload`);
  }
  await verifyNative(ctx, { threadIds: [main.id], requirement: "两轮真实回复均经历未读、markRead 后已读和刷新保持；记录原始发送回执" });
}

async function sideReadState(ctx) {
  readStateReady(ctx);
  const main = owned(ctx, "main"), side = owned(ctx, "side");
  if (side.closed) block("本轮侧边会话已关闭，需在关闭案例之前执行已读回归");
  invariant(side.parentThreadId === main.id, "侧边会话不属于本轮主会话");
  for (let cycle = 1; cycle <= 2; cycle++) {
    const spec = await startUnreadTurn(ctx, "side", cycle);
    await backToMain(ctx);
    await ensureMenu(ctx);
    await wait(ctx, "侧边回复完成后条目与父会话同时出现未读", value => value.threadId === main.id
      && value.parentMenuUnread && rowReadState(value, main.id, true)
      && value.sideEntries.some(entry => entry.id === side.id && entry.visible && entry.unread && !entry.active),
    ctx.options.turnTimeoutMs ?? 180_000);
    await nativeReadState(ctx, { phase: `side-${cycle}-unread`, threadId: side.id, turnId: spec.turnId,
      hasUnreadTurn: true, parentThreadId: main.id, parentSideUnread: true });
    await capture(ctx, `side-${cycle}-parent-unread`);
    await openSide(ctx);
    await wait(ctx, "打开侧边会话后看到实际完成回复", value => value.threadId === side.id && replied(value, spec));
    await nativeReadState(ctx, { phase: `side-${cycle}-opened`, threadId: side.id, turnId: spec.turnId,
      hasUnreadTurn: false, parentThreadId: main.id, parentSideUnread: false });
    await backToMain(ctx);
    await ensureMenu(ctx);
    await wait(ctx, "阅读侧边后父会话与侧边条目徽标清除", value => !value.parentMenuUnread
      && rowReadState(value, main.id, false)
      && value.sideEntries.some(entry => entry.id === side.id && entry.visible && !entry.unread && !entry.active));
    await reloadPage(ctx);
    await wait(ctx, "刷新后保持原主会话", value => value.threadId === main.id && value.promptEditable);
    await ensureOwnedMarker(ctx, main);
    await ensureMenu(ctx);
    await wait(ctx, "刷新后侧边和父会话保持已读", value => !value.parentMenuUnread
      && rowReadState(value, main.id, false)
      && value.sideEntries.some(entry => entry.id === side.id && entry.visible && !entry.unread && !entry.active));
    await nativeReadState(ctx, { phase: `side-${cycle}-reloaded`, threadId: side.id, turnId: spec.turnId,
      hasUnreadTurn: false, parentThreadId: main.id, parentSideUnread: false });
    await capture(ctx, `side-${cycle}-read-after-reload`);
  }
  await verifyNative(ctx, { threadIds: [side.id], parentThreadId: main.id,
    requirement: "两轮侧边回复的原生未读与父会话聚合标识一致，markRead 后刷新保持已读" });
}

/**
 * 外部驱动只负责当前真实环境的能力。不得返回 passed/verified、发 API 代替 UI、
 * mock 响应、改页面存储或生成预期数据。perform/observe 均返回原始 records；
 * records 的 rawFacts 是原始 JSON Pointer，由本模块和离线门禁读取并核对。
 *
 * @typedef {Object} RemoteE2EDriver
 * @property {string} contractVersion 必须为 remote-e2e-driver/2
 * @property {string[]} operations 实际支持的明确操作，未声明的操作阻塞
 * @property {(request: Object) => Promise<{records: Object[], uiEvidence?: Object[]}>} perform
 * @property {(request: Object) => Promise<{records: Object[], uiEvidence?: Object[]}>} observe
 */
export const REMOTE_ACCESS_DRIVER_VERSION = "remote-e2e-driver/2";
const fact = (name, value, op = "equals") => ({ fact: name, op, value });

// 这些契约来自脚本源码，离线验证不能采信报告自填的成功状态或预期。
export const REMOTE_ACCESS_E2E_CONTRACTS = Object.freeze({
  "queue.automatic": { requiresNaturalQueue: true },
  "transport.service": { expectedFacts: { transport: "service", submissionCount: 1, duplicateCount: 0 } },
  "recovery.reconnect": { expectedFacts: { connected: true, draftPreserved: true, duplicateCount: 0 } },
  "recovery.native_restart": { expectedFacts: { connected: true, nativeRestarted: true, draftPreserved: true, duplicateCount: 0 } },
  "recovery.unknown_receipt": { expectedFacts: { receiptState: "completed", originalRequestCount: 1, replayedSubmissionCount: 0 } },
  "recovery.question_receipt": { requiresQuestionReceiptRecovery: true },
  "side.late_creation": { expectedFacts: { createdSideCount: 1, selectedControlPreserved: true, duplicateCount: 0 } },
  "queue.after_stop": { requiresQueueRecovery: true },
  "queue.after_failure": { requiresQueueRecovery: true },
  "queue.approval_wait": { expectedFacts: { pendingKind: "approval", consumedBeforeResolution: 0, consumedCount: 1, duplicateCount: 0 } },
  "queue.compaction": { expectedFacts: { compactionStatus: "completed", consumedDuringCompaction: 0, consumedCount: 1, duplicateCount: 0 } },
  "queue.dequeue_edit_race": { requiresQueueDequeueRace: true },
  "queue.dequeue_remove_race": { requiresQueueDequeueRace: true },
  "side.close_running": { expectedFacts: { closed: true, activeAfterClose: false, parentUsable: true } },
  "side.delete_parent": { expectedFacts: { deleted: true, orphanSideCount: 0, activeSideCount: 0 } },
  "scenario.approval": { expectedFacts: { requestKind: "approval", resolved: true, duplicateCount: 0 } },
  "scenario.question": { expectedFacts: { requestKind: "question", resolved: true, answerAccepted: true } },
  "scenario.question_completed": { expectedFacts: {
    requestKind: "asyncQuestion", originatingTurnStatus: "completed", resolved: true,
    answerAccepted: true, submissionCount: 1, duplicateCount: 0,
  } },
  "scenario.compaction": { expectedFacts: { compactionStatus: "completed", usableAfterCompaction: true } },
  "scenario.goal": { expectedFacts: { goalCreated: true, goalExecuted: true } },
  "optimizer.restore": { requiredUiRestores: 1 },
  "optimizer.no_injection": { requiredUiRestores: 1, expectedFacts: { serviceConnected: false,
    optimizationState: "completed", threadSubmissionCount: 0 } },
  "android.installed_version": { expectedFacts: { packageName: "com.junxin367.codexelves", signatureMatchesRelease: true, installedAssetsMatch: true } },
  "android.background_resume": { expectedFacts: { resumed: true, draftPreserved: true, duplicateCount: 0 } },
  "android.back_navigation": { expectedFacts: { popupClosedFirst: true, returnedToParent: true, exitedUnexpectedly: false } },
  "android.notification": { expectedFacts: { notificationOpenedTarget: true, wrongThreadOpened: false } },
  "android.resource_update": { expectedFacts: { resourceUpdateApplied: true, rollbackAfterFailure: true, draftPreserved: true } },
});

const restoreResources = Object.freeze({
  "restore-service": "service", "restore-network": "tab-network",
  "restore-upstream": "upstream", "restore-receipts": "receipts", "restore-side-create-delivery": "side-create-delivery",
});
const faultResources = Object.freeze({
  "optimizer.no_injection": ["service"],
  "recovery.reconnect": ["tab-network"], "recovery.native_restart": ["native-process"],
  "recovery.unknown_receipt": ["receipts"], "recovery.question_receipt": ["receipts"],
  "side.late_creation": ["side-create-delivery"],
  "queue.after_failure": ["upstream"],
});

function environmentScopeFacts(ctx) {
  const lease = ctx.environmentLease;
  if (!lease) return [];
  return [fact("leaseKey", lease.leaseKey), fact("leaseOwnerRunId", ctx.report.runId),
    fact("instanceId", lease.scope.instanceId), fact("scopeKind", lease.scope.kind), fact("unownedStateChanged", false),
    ...["processId", "workspace", "servicePort", "nativeProcessId", "tabId"].filter(name =>
      lease.scope[name] != null).map(name => fact(name, lease.scope[name]))];
}

async function environmentLeaseOperation(ctx, operation, payload = {}, observe = false) {
  const lease = ctx.environmentLease;
  if (!lease) block("缺少本轮环境租约，不能尝试断开或恢复用户环境");
  const adapter = driver(ctx, "environment", [operation]);
  const label = `lease-${operation}-${payload.resource || "all"}`;
  const id = `${ctx.report.runId}:${ctx.current.id}:${label}`;
  const existing = ctx.report.actions[id];
  const restoring = operation === "restore-environment-resource";
  if (!observe && !restoring && existing) {
    if (existing.status === "confirmed") return existing.result;
    block("租约获取结果未知，只能按原 leaseKey 查询和恢复，不能重放获取请求");
  }
  const record = {
    id, caseId: ctx.current.id, kind: operation, threadId: lease.threadId,
    status: "unknown", startedAt: iso(), environmentLeaseKey: lease.leaseKey,
  };
  const facts = environmentScopeFacts(ctx);
  if (operation === "acquire-environment-lease") facts.push(fact("leaseAcquired", true), fact("scopeKind", lease.scope.kind));
  if (restoring) facts.push(fact("resource", payload.resource),
    fact("restoreDisposition", ["restored", "already-original", "lease-absent-no-mutation"], "oneOf"));
  if (observe) {
    facts.push(fact("stateMatchesOriginal", true));
    if (lease.originalState) facts.push(fact("currentState", lease.originalState));
  }
  const request = {
    contractVersion: REMOTE_ACCESS_DRIVER_VERSION, operation, runId: ctx.report.runId, caseId: ctx.current.id,
    origin: ctx.origin, sourceFingerprint: ctx.report.sourceFingerprint, runStartedAt: ctx.report.runStartedAt,
    capturedAfter: iso(), threadId: lease.threadId, threadIds: [lease.threadId], outputDir: ctx.outputDir,
    ownedThreadIds: Object.values(ctx.report.threads).map(value => value.id),
    payload: { ...payload, leaseKey: lease.leaseKey, scope: lease.scope,
      originalState: lease.originalState || null, restoreOnlyOwnedChanges: true },
    actions: observe ? [] : [{ ...record, status: "confirmed" }], expectations: facts,
  };
  if (!observe) {
    ctx.report.actions[id] = record;
    // 恢复即使无法落盘仍要尝试执行；租约获取/真实故障必须先成功落盘。
    try { await checkpoint(ctx); } catch (error) { if (!restoring) throw error; record.checkpointError = error.message; }
  }
  try {
    const result = await strictDriverEvidence(ctx, await adapter[observe ? "observe" : "perform"](copy(request)), request, "environment");
    if (observe) invariant(result.records.some(value => value.facts?.originalState
      && isDeepStrictEqual(value.facts.currentState, lease.originalState || value.facts.originalState)),
    "租约恢复后的实际 currentState 与驱动保存的原始 originalState 不一致或缺失");
    if (!observe) {
      record.status = "confirmed"; record.finishedAt = iso(); record.result = result;
      await checkpoint(ctx);
    }
    return result;
  } catch (error) {
    if (!observe) record.error = error.message;
    throw error;
  }
}

async function withEnvironment(ctx, resources, run) {
  permit(ctx, "allowFaults");
  const scope = ctx.options.environmentTarget;
  if (!scope) block("故障案例缺少 environmentTarget：需要独立测试服务或授权测试 tab 的原始实例 identity；45222 用户服务禁止故障演练");
  try { validateRemoteAccessEnvironmentScope(scope, { runId: ctx.report.runId, origin: ctx.origin, tabId: ctx.tab.id, resources }); }
  catch (error) { block(error.message); }
  driver(ctx, "environment", ["acquire-environment-lease", "restore-environment-resource", "inspect-environment-lease"]);
  const leaseKey = `${ctx.report.runId}:${ctx.current.id}:environment`;
  const saved = ctx.environmentLease;
  ctx.environmentLease = { leaseKey, scope, threadId: owned(ctx, "main").id };
  try {
    return await withRemoteAccessEnvironmentLease({
      leaseKey, runId: ctx.report.runId, origin: ctx.origin, tabId: ctx.tab.id, scope, resources,
      onEvent: async event => {
        ctx.report.environmentLeases ||= {};
        ctx.report.environmentLeases[leaseKey] = { ...ctx.report.environmentLeases[leaseKey], ...event, at: iso() };
        await note(ctx, "environment-lease", event);
      },
      acquire: async () => {
        const proof = await environmentLeaseOperation(ctx, "acquire-environment-lease", { resources });
        const originalState = await rawDriverFact(proof, "originalState");
        ctx.environmentLease.originalState = originalState;
        return { leaseKey, runId: ctx.report.runId, originalState };
      },
      run,
      restoreResource: request => environmentLeaseOperation(ctx, "restore-environment-resource", {
        resource: request.resource, acquisitionUnknown: request.acquisitionUnknown,
      }),
      verifyRestored: () => environmentLeaseOperation(ctx, "inspect-environment-lease", { resources }, true),
    });
  } finally { ctx.environmentLease = saved; }
}

async function restoreLeasedResources(ctx, resources) {
  const failures = [];
  for (const resource of resources) {
    try { await environmentLeaseOperation(ctx, "restore-environment-resource", { resource }); }
    catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, `租约资源恢复未全部确认：${failures.map(value => value.message).join("；")}`);
}

/**
 * 进程被终止后的独立恢复入口：即使源码指纹已变，也只恢复原租约，不获取新租约、不执行测试、
 * 不确认原 unknown 动作。调用方必须明确提供原 scope 与 allowFaults 授权。
 */
export async function recoverRemoteAccessEnvironmentLease({ reportPath, caseId, environmentTarget, drivers,
  authorization, authorizedTabId } = {}) {
  if (authorization?.allowFaults !== true) block("原环境租约恢复需要明确 authorization.allowFaults");
  invariant(typeof reportPath === "string" && typeof caseId === "string", "恢复必须指定原 reportPath 与 caseId");
  const report = JSON.parse((await readFile(reportPath, "utf8")).replace(/^\uFEFF/, ""));
  const leaseKey = `${report.runId}:${caseId}:environment`, previous = report.environmentLeases?.[leaseKey];
  invariant(previous?.scope && previous.resources?.length && isDeepStrictEqual(environmentTarget, previous.scope),
    "恢复目标必须与原报告登记的明确环境 scope 完全一致");
  const entry = report.cases.find(value => value.id === caseId);
  invariant(entry, "原报告没有对应故障案例");
  const ctx = { options: { drivers, authorization }, report, origin: report.origin, outputDir: dirname(resolve(reportPath)),
    tab: { id: authorizedTabId }, current: entry,
    environmentLease: { leaseKey, scope: previous.scope, threadId: report.threads.main.id,
      originalState: previous.originalState || null } };
  driver(ctx, "environment", ["restore-environment-resource", "inspect-environment-lease"]);
  const result = await restoreRemoteAccessEnvironmentLease({
    leaseKey, runId: report.runId, origin: report.origin, tabId: authorizedTabId,
    scope: previous.scope, resources: previous.resources, originalState: previous.originalState || null,
    restoreResource: request => environmentLeaseOperation(ctx, "restore-environment-resource", {
      resource: request.resource, acquisitionUnknown: request.acquisitionUnknown,
    }),
    verifyRestored: () => environmentLeaseOperation(ctx, "inspect-environment-lease", { resources: previous.resources }, true),
    onEvent: async event => {
      report.environmentLeases[leaseKey] = { ...previous, ...event, at: iso(), recoveryOnly: true };
      await note(ctx, "environment-lease-recovery-only", event);
    },
  });
  return { ...result, originalUnknownActionIds: Object.values(report.actions).filter(value =>
    value.status === "unknown").map(value => value.id) };
}

function driver(ctx, name, operations) {
  const value = ctx.options.drivers?.[name];
  if (!value || value.contractVersion !== REMOTE_ACCESS_DRIVER_VERSION
    || typeof value.perform !== "function" || typeof value.observe !== "function")
    block(`缺少真实 ${name} 驱动：drivers.${name} 须实现 ${REMOTE_ACCESS_DRIVER_VERSION} perform/observe`);
  const missing = operations.map(operation => name === "environment" && restoreResources[operation]
    ? "restore-environment-resource" : operation).filter(operation => !value.operations?.includes(operation));
  if (missing.length) block(`${name} 驱动缺少明确能力：${missing.join(", ")}`);
  return value;
}

async function strictDriverEvidence(ctx, result, request, name) {
  if (result?.status === "blocked") block(result.reason || `${name} 实际环境尚未就绪`);
  invariant(result && !("status" in result) && !("verified" in result)
    && Array.isArray(result.records) && result.records.length > 0,
  "驱动必须返回原始 records，不能把 passed/verified 当作执行证据");
  result = { ...result, records: await verifyNativeEvidence(result.records, request, { baseDir: ctx.outputDir }) };
  for (const item of result.uiEvidence || []) {
    await verifyEvidenceReference(item.dom, { baseDir: ctx.outputDir });
    await verifyEvidenceReference(item.screenshot, { baseDir: ctx.outputDir });
  }
  if (["desktop", "android", "files"].includes(name)) invariant(result.uiEvidence?.length,
    `${name} 操作必须保存真实设备或 Codex 桌面的 DOM/可访问性树与截图`);
  await note(ctx, "driver", { driver: name, operation: request.operation, records: result.records,
    uiEvidence: result.uiEvidence || [], request });
  return result;
}

async function driverStep(ctx, name, operation, { key = "main", mutation = false,
  facts = [], payload = {}, messageSpec = null, approval = null } = {}) {
  if (name === "environment" && mutation && restoreResources[operation])
    return environmentLeaseOperation(ctx, "restore-environment-resource", {
      resource: restoreResources[operation], sequence: restoreResources[operation], restoreOnlyOwnedChanges: true,
    });
  if (name === "environment" && mutation && !ctx.environmentLease)
    block("故障动作缺少独立测试实例的已核验环境租约，禁止影响用户服务或进程");
  const adapter = driver(ctx, name, [operation]);
  const contextRecord = owned(ctx, key);
  const record = payload.targetThreadId
    ? Object.values(ctx.report.threads).find(value => value.id === payload.targetThreadId && !value.deleted && !value.closed)
    : contextRecord;
  if (!record) block("驱动目标 thread ID 不属于本轮仍有效的测试对象");
  const request = {
    contractVersion: REMOTE_ACCESS_DRIVER_VERSION, runId: ctx.report.runId, caseId: ctx.current.id,
    operation, origin: ctx.origin, sourceFingerprint: ctx.report.sourceFingerprint,
    runStartedAt: ctx.report.runStartedAt, capturedAfter: iso(), threadId: record.id,
    parentThreadId: record.parentThreadId, threadIds: [record.id],
    ownedThreadIds: Object.values(ctx.report.threads).filter(value => !value.deleted).map(value => value.id),
    marker: record.marker, outputDir: ctx.outputDir,
    payload: name === "environment" ? { ...payload, environmentLease: ctx.environmentLease } : payload, approval,
    expectations: [fact("threadId", record.id),
      ...(["desktop", "android"].includes(name) ? [fact("surface", name === "desktop" ? "codex-desktop" : "android")] : []),
      ...(name === "environment" ? environmentScopeFacts(ctx) : []),
      ...facts], actions: [],
  };
  const perform = async () => {
    if (messageSpec) {
      permit(ctx, "allowMessages");
      const limit = ctx.options.authorization?.maxMessageSubmissions;
      if (!Number.isInteger(limit) || ctx.report.messageSubmissions >= limit)
        block("外部驱动消息缺少本轮剩余显式预算");
      invariant(messageSpec.text.includes(marker(ctx, messageSpec.key)), "外部驱动消息缺少完整本轮唯一标记");
      ctx.report.messageSubmissions++;
      request.message = messageSpec;
      await checkpoint(ctx);
    }
    const result = await strictDriverEvidence(ctx, await adapter[mutation ? "perform" : "observe"](copy(request)), request, name);
    await capture(ctx, `driver-${operation}`);
    return { records: result.records, uiEvidence: result.uiEvidence || [] };
  };
  if (!mutation) return perform();
  permit(ctx, ({ desktop: "allowDesktop", android: "allowAndroid", files: "allowAttachments",
    text: "allowDrafts", native: "allowFaults", environment: "allowFaults" })[name] || "allowFaults");
  const label = `driver-${name}-${operation}-${payload.sequence || "once"}`;
  return action(ctx, label, { kind: operation, threadId: record.id, contextThreadId: contextRecord.id }, async () => {
    const actionId = `${ctx.report.runId}:${ctx.current.id}:${label}`;
    request.actions = [{ id: actionId, kind: operation, threadId: record.id, status: "confirmed", startedAt: iso() }];
    request.actionId = actionId;
    return perform();
  });
}

async function createSideBlank(ctx, key = "side", parentKey = "main") {
  permit(ctx, "allowSideSessions");
  if (ctx.report.threads[key]) return owned(ctx, key);
  await idle(ctx, parentKey);
  const parent = owned(ctx, parentKey);
  await ensureMenu(ctx);
  const before = (await state(ctx)).sideEntries.map(value => value.id);
  const label = `create-${key}`;
  const result = await action(ctx, label, { kind: "side-create", threadId: parent.id }, async () => {
    await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "新建侧边会话", exact: true }));
    const observation = await wait(ctx, "真实新建空侧边会话", value => value.related && !!value.threadId && value.threadId !== parent.id);
    invariant(!before.includes(observation.threadId) && !ctx.report.initialThreadIds.includes(observation.threadId)
      && !Object.values(ctx.report.threads).some(value => value.id === observation.threadId), "侧边创建返回已存在的 ID");
    invariant(observation.turns.length === 0 && observation.prompt.trim() === "", "新侧边会话已有消息或草稿");
    return { threadId: observation.threadId, title: observation.title };
  });
  const record = {
    id: result.threadId, key, marker: marker(ctx, key), runId: ctx.report.runId,
    createdBy: "remote-access-e2e", creationActionId: `${ctx.report.runId}:${ctx.current.id}:${label}`,
    parentThreadId: parent.id, createdAt: iso(), title: result.title, defaultTitle: result.title, kind: "side",
  };
  ctx.report.threads[key] = record;
  await checkpoint(ctx);
  return record;
}

async function markSide(ctx, key = "side") {
  const record = owned(ctx, key);
  const existing = Object.values(ctx.report.actions).find(value => value.marker === record.marker && value.status === "confirmed");
  if (existing) {
    await openSide(ctx, key);
    return { key, marker: record.marker, expected: existing.expected, ...existing.result };
  }
  const spec = await send(ctx, message(ctx, key), { creating: true, expectedThreadId: record.id });
  await finalReply(ctx, spec);
  record.firstMessageText = spec.text;
  await checkpoint(ctx);
  return spec;
}

async function chooseEffort(ctx, model) {
  invariant(model?.name && model.id && model.protocol && model.effort && model.effortLabel,
    "模型深度案例需两个已观察且获准的 name/id/protocol/effort/effortLabel");
  const original = ctx.options.model;
  ctx.options.model = model;
  try { await chooseModel(ctx); } finally { ctx.options.model = original; }
  await click(ctx, ctx.tab.playwright.locator("#model-button"));
  const effort = ctx.tab.playwright.locator(".effort-options")
    .getByRole("menuitemradio", { name: model.effortLabel, exact: true });
  if (await effort.count() !== 1 || !await effort.isEnabled()) block(`当前模型没有唯一可用思考深度 ${model.effortLabel}`);
  await click(ctx, effort);
  await click(ctx, ctx.tab.playwright.locator("#model-button"));
  invariant(await ctx.tab.playwright.locator(`[data-choice-key="effort:${css(model.effort)}"]`)
    .getAttribute("aria-checked") === "true", "思考深度选择未生效");
  await ctx.tab.playwright.locator("#model-button").press("Escape");
}

async function fillOwnedDraft(ctx, key, draft) {
  permit(ctx, "allowDrafts");
  await selectOwned(ctx, key);
  const initial = await state(ctx);
  if (initial.prompt.trim() && !initial.prompt.includes(`[remote-e2e:${ctx.report.runId}:`)) block("当前有非本轮草稿，停止覆盖");
  await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill(draft);
  await wait(ctx, "本轮草稿完整进入输入框", value => value.prompt === draft && value.promptEditable);
}

async function captureObservation(ctx, label) {
  const evidence = await capture(ctx, label);
  invariant(evidence.observation && evidence.files.dom && evidence.files.screenshot, "缺少完整远程界面证据");
  const path = join(ctx.outputDir, `${ctx.report.evidenceSequence}-${label}.observation.json`);
  await writeFile(path, `${JSON.stringify(evidence.observation, null, 2)}\n`, "utf8");
  evidence.files.observation = await createEvidenceReference(path);
  await checkpoint(ctx);
  return evidence;
}

async function modelEffortIsolation(ctx) {
  const models = ctx.options.models;
  if (!Array.isArray(models) || models.length !== 2 || models[0].id === models[1].id && models[0].effort === models[1].effort)
    block("需提供明确获准且不同的 models[0..1] 及思考深度");
  const outcomes = [];
  for (const [index, key] of ["main", "control", "main"].entries()) {
    await idle(ctx, key);
    const target = models[index === 1 ? 1 : 0];
    if (index < 2) await chooseEffort(ctx, target);
    else {
      invariant((await state(ctx)).model.includes(target.name), "切回主会话后串用了另一会话模型");
      await click(ctx, ctx.tab.playwright.locator("#model-button"));
      invariant(await ctx.tab.playwright.locator(`[data-choice-key="effort:${css(target.effort)}"]`)
        .getAttribute("aria-checked") === "true", "切回主会话后思考深度串用");
      await ctx.tab.playwright.locator("#model-button").press("Escape");
    }
    const spec = await send(ctx, message(ctx, `model-effort-${index}`), { key });
    const final = await finalReply(ctx, spec);
    const turnId = userTurn(final, spec).id;
    await verifyNative(ctx, { threadIds: [spec.threadId], turnIds: [turnId], model: target,
      expectations: [fact("reasoningEffort", target.effort)], actionsForMarker: spec.marker });
    outcomes.push({ threadId: spec.threadId, turnId, model: target.id, effort: target.effort });
  }
  await note(ctx, "model-effort-isolation", { outcomes });
  ctx.current.nativeExpectations = { threadIds: [owned(ctx, "main").id, owned(ctx, "control").id],
    requirement: "每轮独立核对实际模型/思考深度，回切主会话后第三轮仍使用主会话设置" };
}

async function contextCheck(ctx, key, phase, expected = null) {
  await selectOwned(ctx, key);
  await click(ctx, ctx.tab.playwright.locator("#context-info"));
  const observation = await wait(ctx, "上下文圆环和提示框实际显示", value => !!value.context.tooltip);
  const result = await driverStep(ctx, "native", "observe-context", { key, payload: { phase },
    facts: expected ? Object.entries(expected).map(([name, value]) => fact(name, value)) : [] });
  const record = result.records.find(value => value.rawFacts?.usedTokens && value.rawFacts?.modelContextWindow);
  if (!record) block("原生上下文证据缺少 usedTokens/modelContextWindow 的准确原始指针");
  const original = JSON.parse((await readFile(record.evidencePath, "utf8")).replace(/^\uFEFF/, ""));
  const used = jsonPointer(original, record.rawFacts.usedTokens), total = jsonPointer(original, record.rawFacts.modelContextWindow);
  if (typeof used === "number" && used >= 0 && typeof total === "number" && total > 0) {
    const percent = Math.round(Math.min(used, total) / total * 100);
    invariant(observation.context.ring === `${percent}%` && observation.context.label.includes(`${used.toLocaleString()} / ${total.toLocaleString()} Token`),
      "上下文圆环或说明与当前原生最后请求用量不一致");
  } else invariant(observation.context.ring === "—" && observation.context.tooltip === "上下文信息尚未提供",
    "缺少有效用量时没有显示明确缺失态");
  await note(ctx, "context-observation", { phase, threadId: observation.threadId, used, total, ui: observation.context });
  await click(ctx, ctx.tab.playwright.locator("#context-info"));
  return { used, total };
}

const handlers = {
  "read.main_read_state": mainReadState,
  "read.side_read_state": sideReadState,
  "session.model_effort_isolation": modelEffortIsolation,
  async "read.ready"(ctx) {
    const observation = await wait(ctx, "真实远程页面具备连接与编辑控件", value => value.canCreate
      && !!value.connection && !!value.send.label);
    await note(ctx, "connected", { url: observation.url, threadId: observation.threadId, connection: observation.connection });
  },
  async "read.runtime_version"(ctx) {
    const evidence = ctx.options.versionEvidence;
    if (!evidence?.managerVersion || !evidence.managerExecutableSha256 || !evidence.codexVersion
      || !evidence.remoteAssetSha256 || evidence.serviceProtocolVersion == null || !evidence.verifiedAt
      || !evidence.records?.length || !evidence.managerExecutable || !evidence.remoteAsset
      || !evidence.browserAsset || !evidence.browserAssetEvidence)
      block("缺少当前安装程序、服务协议或浏览器实际加载脚本的证据；源码和另行下载的资源不能代替运行版本");
    invariant(/^[a-fA-F0-9]{64}$/.test(evidence.managerExecutableSha256)
      && /^[a-fA-F0-9]{64}$/.test(evidence.remoteAssetSha256), "运行资产 SHA-256 格式无效");
    await verifyRuntimeEvidence(evidence, {
      origin: ctx.origin, sourceFingerprint: ctx.report.sourceFingerprint, runStartedAt: ctx.report.runStartedAt,
    }, { baseDir: ctx.outputDir });
    await note(ctx, "runtime-version", { evidence });
  },
  async "session.create"(ctx) {
    await createOne(ctx, "main"); await createOne(ctx, "control");
    await selectOwned(ctx, "main");
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id, owned(ctx, "control").id], operation: "create-and-send" });
  },
  async "read.select_refresh"(ctx) {
    await selectOwned(ctx, "main"); await selectOwned(ctx, "control"); await selectOwned(ctx, "main");
    await refreshOwned(ctx, "main");
    await reloadPage(ctx);
    await wait(ctx, "整页刷新恢复主会话及真实历史", value => value.threadId === owned(ctx, "main").id && value.promptEditable);
    const observation = await ensureOwnedMarker(ctx, owned(ctx, "main"));
    assertOwnedState(ctx, observation, owned(ctx, "main"));
  },
  async "session.model"(ctx) {
    if (!ctx.options.model?.id || !ctx.options.model?.protocol)
      block("实际模型核验需 model.id 与 model.protocol，不能只凭显示名称判定通过");
    await idle(ctx);
    await chooseModel(ctx);
    await capture(ctx, "selected-model-before-actual-send");
    const spec = await send(ctx, message(ctx, "model"));
    const observation = await finalReply(ctx, spec);
    await refreshOwned(ctx, "main");
    await verifyNative(ctx, {
      threadIds: [spec.threadId], turnIds: [userTurn(observation, spec).id], model: ctx.options.model,
      requirement: "原生实际 turn 的模型与协议须匹配所选 model.id / protocol，不能只确认按钮文字",
    });
  },
  async "session.send_stream"(ctx) {
    await idle(ctx);
    const spec = await send(ctx, message(ctx, "stream", ctx.options.messages?.stream));
    const samples = [];
    const observation = await wait(ctx, "观察真实流式回复并等待最终回复", value => {
      const turn = userTurn(value, spec);
      const text = turn?.agents.map(agent => agent.text).join("\n") || "";
      if (text && text !== samples.at(-1)?.text) samples.push({
        at: value.at, text, active: value.send.action === "stop", turnId: turn.id,
      });
      return replied(value, spec) && value.send.action !== "stop" && value.send.action !== "sending";
    }, ctx.options.turnTimeoutMs ?? 180_000);
    await note(ctx, "stream-samples", { samples, turnId: userTurn(observation, spec).id });
    if (samples.length < 2 || !samples.some(sample => sample.active))
      block("真实最终回复已出现，但未观察到至少两次内容变化及执行中样本；不能据此宣告流式通过");
    await refreshOwned(ctx, "main");
    invariant(replied(await state(ctx), spec), "刷新后真实回复缺失");
    await verifyNative(ctx, { threadIds: [spec.threadId], turnIds: [userTurn(observation, spec).id] });
  },
  async "queue.automatic"(ctx) {
    permit(ctx, "allowQueueChanges");
    const window = await activeWindow(ctx, "automatic");
    const specs = [];
    for (const suffix of ["A", "B"])
      specs.push(await send(ctx, message(ctx, `automatic-next-${suffix}`), { queued: true }));
    const queued = await state(ctx);
    invariant(JSON.stringify(queued.queue.map(row => row.id)) === JSON.stringify(specs.map(spec => spec.queueId)),
      "两条原消息未同时按提交顺序处于原生队列");
    await capture(ctx, "queued-during-active-turn");
    // 从此只读等待；不能引导、停止、刷新、解除暂停或设置 deferAutomaticTurns。
    const observation = await wait(ctx, "活动轮自然结束后按顺序自动承接两条队列", value => value.queue.length === 0
      && specs.every(spec => replied(value, spec))
      && !["stop", "sending"].includes(value.send.action), ctx.options.turnTimeoutMs ?? 180_000);
    const turns = specs.map(spec => userTurn(observation, spec));
    invariant(new Set([window.turnId, ...turns.map(turn => turn.id)]).size === 3,
      "两条队列消息没有各自进入独立后续轮次");
    for (const spec of specs)
      invariant(observation.turns.filter(value => value.users.some(user => user.text.includes(spec.marker))).length === 1,
        "自动承接产生重复用户轮次");
    invariant(observation.turns.indexOf(turns[0]) < observation.turns.indexOf(turns[1]), "两条队列执行顺序颠倒");
    await note(ctx, "automatic-handoff", { queuedMessageIds: specs.map(spec => spec.queueId),
      previousTurnId: window.turnId, nextTurnIds: turns.map(turn => turn.id) });
    await verifyNative(ctx, { threadIds: [specs[0].threadId], naturalQueue: {
      threadId: specs[0].threadId, previousTurnId: window.turnId,
      entries: specs.map(spec => ({ actionId: spec.actionId, queueId: spec.queueId,
        prompt: spec.text, expectedReply: spec.expected })),
    }, requirement: "逐条将原排队回执的 clientUserMessageId 关联至持久 userMessage.clientId；核对 FIFO 起止时间、准确正文、唯一回复及最终空队列" });
  },
  async "queue.edit_draft"(ctx) {
    permit(ctx, "allowQueueChanges"); permit(ctx, "allowDrafts");
    // 复用原生取回观察合同的 before/editing 两阶段，保留完整原输入及位置证据。
    driver(ctx, "native", ["observe-queue-edit-cancel"]);
    const main = owned(ctx, "main");
    await activeWindow(ctx, "editing");
    for (const key of ["A", "B", "C"]) {
      if (!ctx.report.queue[key]) {
        ctx.report.queue[key] = await send(ctx, message(ctx, `queue-${key}`), { queued: true });
        await checkpoint(ctx);
      }
    }
    const { spec, observation: initial } = await queueRef(ctx, "A");
    if (initial.editing || initial.pendingText) block("已有取回草稿或待确认写入，不能开始排队草稿测试");
    if (initial.prompt && !initial.prompt.includes(`[remote-e2e:${ctx.report.runId}:`))
      block("普通草稿不是本轮输入，停止排队草稿测试");
    const stage = { kind: "queue-edit-draft", threadId: main.id, queueId: spec.queueId,
      queueActionId: spec.actionId, beforeOrder: initial.queue.map(row => row.id) };
    const observe = async phase => {
      const result = await driverStep(ctx, "native", "observe-queue-edit-cancel", {
        payload: { phase, queueId: stage.queueId, editId: stage.editId },
        facts: [fact("phase", phase)],
      });
      const records = result.records.filter(record => record.facts.phase === phase && record.threadId === main.id);
      invariant(records.length === 1, "队列取回每阶段必须提供唯一原始观察");
      stage[phase] = records[0];
      return records[0].facts;
    };
    const before = await observe("before");
    if (!before.queuedMessages?.some(value => value.id === stage.queueId))
      block("原队列对象在取出前已离开，保留窗口错过结果，不重建或重发");
    await action(ctx, "edit-draft-A", { kind: "queue-race-edit", threadId: spec.threadId, queueId: spec.queueId }, async () => {
      await queueMenu(ctx, spec.queueId);
      await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "编辑消息", exact: true }));
      await wait(ctx, "真实队列编辑进入输入框", value => value.editing && value.prompt.includes(spec.marker));
      const editing = await observe("editing");
      invariant(typeof editing.editId === "string" && editing.editId.length > 0 && editing.editState === "editing",
        "缺少准确原生ED，不继续修改");
      stage.editId = editing.editId;
      verifyQueueEditCancelEditingFacts({ before: stage.before.facts, editing }, {
        threadId: main.id, queueId: stage.queueId, editId: stage.editId,
      });
      const changed = `${spec.text}\n取回后尚未发送的草稿`;
      await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill(changed);
      await wait(ctx, "真实编辑器已改成与原文不同的正文", value =>
        value.threadId === main.id && value.editing && value.prompt === changed);
      stage.draftUi = await captureObservation(ctx, "queue-unsent-draft");
      const draft = stage.draftUi.observation;
      invariant(!draft.queueEditBanner && !draft.queueEditCancel && draft.send.action === "send",
        "排队草稿仍显示专用编辑栏、取消按钮或保存入口");
      invariant(!draft.queue.some(row => row.id === stage.queueId) && !userTurn(draft, spec),
        "未点击发送，原消息已经恢复队列或进入执行");
      return { queueId: stage.queueId, editId: stage.editId };
    });
    const changed = stage.draftUi.observation.prompt;
    const reloaded = await refreshOwned(ctx, "main");
    invariant(reloaded.editing && reloaded.prompt === changed
      && !reloaded.queue.some(row => row.id === stage.queueId) && !userTurn(reloaded, spec),
    "刷新后未发送草稿丢失，或原消息被自动恢复、执行");
    await action(ctx, "resend-draft-A", { kind: "queue-edit-save", threadId: main.id, queueId: stage.queueId }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "发送消息", exact: true }));
      const after = await wait(ctx, "重新发送后原位置出现唯一修改消息", value => !value.editing
        && value.queue.filter(row => row.text.trim() === changed.trim()).length === 1);
      const restored = after.queue.find(row => row.text.trim() === changed.trim());
      stage.afterOrder = after.queue.map(row => row.id);
      invariant(isDeepStrictEqual(stage.afterOrder,
        stage.beforeOrder.map(id => id === stage.queueId ? restored.id : id)),
      "重新发送改变了原队列位置");
      invariant(after.prompt === initial.prompt, "重新发送后原普通草稿没有恢复");
      spec.text = changed; spec.queueId = restored.id;
      return { queueId: restored.id, editId: stage.editId };
    });
    await note(ctx, "queue-edit-draft", stage);
    await verifyNative(ctx, { threadIds: [main.id], queueOrder: stage.afterOrder,
      requirement: "取回时保留完整原输入和位置；未再次发送不自动入队，重新发送后只保留一份修改且顺序不变" });
  },
  async "queue.edit_save"(ctx) {
    const { spec } = await queueRef(ctx, "A");
    const edited = `${spec.text}\n已保存的回归修改`;
    await action(ctx, "edit-save-A", { kind: "queue-edit-save", threadId: spec.threadId, queueId: spec.queueId }, async () => {
      await queueMenu(ctx, spec.queueId);
      await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "编辑消息", exact: true }));
      await wait(ctx, "进入队列编辑", value => value.editing && value.prompt.includes(spec.marker));
      await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill(edited);
      await wait(ctx, "真实编辑器已改成待发送正文", value => value.editing && value.prompt === edited);
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "发送消息", exact: true }));
      await wait(ctx, "发送后队列出现唯一修改消息", value => !value.editing
        && value.queue.filter(row => row.text.trim() === edited.trim()).length === 1);
      const observation = await refreshOwned(ctx, "main");
      const row = observation.queue.find(value => value.text.trim() === edited.trim());
      invariant(row, "刷新后队列修改丢失");
      spec.text = edited; spec.queueId = row.id; return { queueId: row.id, text: edited };
    });
    await verifyNative(ctx, { threadIds: [spec.threadId], requirement: "同一队列逻辑对象保存后只保留一份修改" });
  },
  async "queue.reorder"(ctx) {
    const { spec, observation } = await queueRef(ctx, "B");
    const before = observation.queue.map(row => row.id), index = before.indexOf(spec.queueId);
    if (index < 1) block("目标队列消息没有可交换的前一项，无法建立排序场景");
    const expected = [...before];
    [expected[index - 1], expected[index]] = [expected[index], expected[index - 1]];
    await action(ctx, "move-B-up", { kind: "queue-reorder", threadId: spec.threadId, queueId: spec.queueId }, async () => {
      await queueLocator(ctx, spec.queueId).getByRole("button", { name: "拖动调整排队顺序", exact: true }).press("ArrowUp");
      await wait(ctx, "队列顺序与键盘排序结果一致", value => JSON.stringify(value.queue.map(row => row.id)) === JSON.stringify(expected));
      const after = await refreshOwned(ctx, "main");
      invariant(JSON.stringify(after.queue.map(row => row.id)) === JSON.stringify(expected), "刷新后队列排序丢失");
      return { before, after: expected };
    });
    await verifyNative(ctx, { threadIds: [spec.threadId], queueOrder: expected });
  },
  async "queue.remove"(ctx) {
    const { spec } = await queueRef(ctx, "C");
    await action(ctx, "remove-C", { kind: "queue-remove", threadId: spec.threadId, queueId: spec.queueId }, async () => {
      await queueMenu(ctx, spec.queueId);
      await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "删除排队消息", exact: true }));
      await wait(ctx, "指定队列消息消失", value => !value.queue.some(row => row.text.includes(spec.marker)));
      const after = await refreshOwned(ctx, "main");
      invariant(!after.queue.some(row => row.text.includes(spec.marker)) && !userTurn(after, spec), "已删队列消息仍存在或已被执行");
      return { removedQueueId: spec.queueId };
    });
    await verifyNative(ctx, { threadIds: [spec.threadId], removedQueueId: spec.queueId });
  },
  async "queue.steer"(ctx) {
    const { spec } = await queueRef(ctx, "B");
    const before = await captureObservation(ctx, "queue-steer-before");
    const observation = before.observation;
    const row = observation.queue.find(value => value.id === spec.queueId);
    if (observation.threadId !== spec.threadId || observation.loading || !row?.steerEnabled)
      block("当前原生队列对象不能引导；需真实可引导的活动轮场景");
    const active = findQueueSteeringTurn(observation);
    if (!active) block("点击前没有唯一可确认的真实活动轮，不能以进入新轮代替引导验收");
    const targetTurnId = active.id;
    await action(ctx, "steer-B", { kind: "queue-steer", threadId: spec.threadId,
      queueId: spec.queueId, turnId: targetTurnId }, async () => {
      await click(ctx, queueLocator(ctx, spec.queueId).getByRole("button", { name: "引导此队列消息", exact: true }));
      const after = await wait(ctx, "引导消息进入真实轮次", value => !value.queue.some(row => row.text.includes(spec.marker))
        && !!userTurn(value, spec) && !userTurn(value, spec).id.startsWith("pending-"));
      invariant(userTurn(after, spec).id === targetTurnId, "引导消息没有进入点击前的活动轮");
      invariant(after.turns.filter(turn => turn.users.some(user => user.text.includes(spec.marker))).length === 1,
        "引导产生重复用户轮次");
      return { turnId: targetTurnId };
    });
    await verifyNative(ctx, { threadIds: [spec.threadId], turnIds: [targetTurnId],
      expectedFacts: { turnId: targetTurnId },
      queueSteerBefore: before.files.observation,
      requirement: "原始引导回执必须绑定点击前的活动轮，原队列只消费一次" });
  },
  async "session.stop"(ctx) {
    const template = ctx.options.messages?.stop;
    if (!template) block("缺少经用户批准的 messages.stop，不能保证发送后仍存在可停止的真实活动轮");
    await idle(ctx);
    const spec = await send(ctx, message(ctx, "stop", template));
    await wait(ctx, "出现真实停止按钮", value => value.send.action === "stop" && value.send.enabled
      && !!userTurn(value, spec) && !userTurn(value, spec).id.startsWith("pending-"));
    await action(ctx, "stop-active", { kind: "stop", threadId: spec.threadId }, async () => {
      assertOwnedState(ctx, await state(ctx), owned(ctx, "main"));
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "停止生成", exact: true }));
      const after = await wait(ctx, "原生轮次停止并恢复编辑", value => userTurn(value, spec)?.stopped
        && value.send.action !== "stop" && value.promptEditable);
      const draft = marker(ctx, "unsent-after-stop");
      await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill(draft);
      const editable = await state(ctx);
      invariant(editable.send.enabled && editable.prompt === draft, "停止后输入或发送按钮仍被锁定");
      await ctx.tab.playwright.getByRole("textbox", { name: "消息输入", exact: true }).fill("");
      return { turnId: userTurn(after, spec).id };
    });
    await verifyNative(ctx, { threadIds: [spec.threadId], requirement: "原生轮次确认为 interrupted / cancelled，未把超时当停止成功" });
  },
  async "lifecycle.side_create"(ctx) {
    const record = await createSideBlank(ctx);
    await markSide(ctx);
    await verifyNative(ctx, { threadIds: [record.id], parentThreadId: record.parentThreadId });
  },
  async "lifecycle.side_switch"(ctx) {
    await openSide(ctx); await backToMain(ctx); await openSide(ctx); await backToMain(ctx);
    assertOwnedState(ctx, await state(ctx), owned(ctx, "main"));
  },
  async "lifecycle.side_refresh"(ctx) {
    await openSide(ctx);
    await reloadPage(ctx);
    const side = owned(ctx, "side");
    const observation = await wait(ctx, "刷新后保持侧边身份与消息", value => value.threadId === side.id && value.related
      && value.turns.some(turn => turn.users.some(user => user.text.includes(side.marker))));
    invariant(observation.model.trim(), "刷新后侧边会话模型显示为空");
    await backToMain(ctx);
    assertOwnedState(ctx, await state(ctx), owned(ctx, "main"));
  },
  async "lifecycle.side_close"(ctx) {
    permit(ctx, "allowSideSessions");
    const sideKey = ctx.current.sideCloseKey || "side", parentKey = ctx.current.sideCloseParentKey || "main";
    const side = owned(ctx, sideKey), main = owned(ctx, parentKey);
    invariant(!side.closed && side.parentThreadId === main.id, "待关闭对象不是本轮主会话的有效侧边会话");
    let observation = await state(ctx), pendingPreview = ctx.report.pendingSideClosePreview;
    let sourceLabel = pendingPreview?.sourceLabel, sourceEvidence = pendingPreview?.sourceEvidence;
    if (!observation.confirmation) {
      if (observation.related) await backToMain(ctx);
      await selectOwned(ctx, parentKey); await ensureMenu(ctx);
      const source = ctx.tab.playwright.locator(`[data-menu-key="side:${css(side.id)}"]`);
      invariant(await source.count() === 1, "无法唯一定位待关闭的本轮侧边对象");
      sourceLabel = await source.innerText();
      sourceEvidence = await capture(ctx, "side-close-target");
      invariant(sourceEvidence.files.dom && sourceEvidence.files.screenshot, "待关闭侧边对象的菜单证据未保存");
      await source.press("Shift+F10");
      await ctx.tab.playwright.domSnapshot();
      await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "关闭会话", exact: true }));
      observation = await wait(ctx, "关闭侧边会话确认界面", value => !!value.confirmation);
    } else if (!pendingPreview || pendingPreview.threadId !== side.id || pendingPreview.parentThreadId !== main.id) {
      block("当前关闭确认框没有本轮已记录的对象来源，不能猜测目标并确认");
    }
    assertOwnedState(ctx, observation, main);
    invariant(!observation.related && observation.confirmation?.title === "关闭侧边会话"
      && observation.confirmation.message.includes("临时侧边会话")
      && observation.confirmation.message.includes("无法恢复"), "关闭确认框没有明确临时会话及不可恢复风险");
    const preview = await capture(ctx, "side-close-awaiting-approval");
    invariant(preview.files.screenshot && preview.observation, "关闭确认截图或对象证据未保存，不能执行关闭");
    const id = `${ctx.report.runId}:${ctx.current.id}:close-side`;
    if (!pendingPreview || pendingPreview.actionId !== id || pendingPreview.threadId !== side.id
      || pendingPreview.parentThreadId !== main.id || pendingPreview.confirmationMessage !== observation.confirmation.message) {
      pendingPreview = {
        runId: ctx.report.runId, actionId: id, threadId: side.id, parentThreadId: main.id,
        title: side.title, marker: side.marker, sourceMenuKey: `side:${side.id}`, sourceLabel, sourceEvidence,
        url: preview.url, preparedAt: iso(), screenshotPath: preview.files.screenshot.path,
        confirmationMessage: observation.confirmation.message,
      };
      ctx.report.pendingSideClosePreview = pendingPreview;
      await checkpoint(ctx);
    }
    if (!ctx.options.authorizeSideClose)
      block("已停留在不可恢复的侧边关闭确认界面；缺少动作当时批准回调 authorizeSideClose");
    const approval = await ctx.options.authorizeSideClose({
      ...pendingPreview, warning: "关闭后此临时侧边会话及内容无法恢复",
    });
    if (!approval?.approved || approval.runId !== ctx.report.runId || approval.actionId !== id
      || approval.threadId !== side.id || approval.parentThreadId !== main.id || !approval.evidencePath
      || !Number.isFinite(Date.parse(approval.approvedAt))
      || Date.parse(approval.approvedAt) < Date.parse(pendingPreview.preparedAt)
      || Date.parse(approval.approvedAt) > Date.now() + 5_000)
      block("未获得本次预览后、匹配侧边与父会话对象的有效批准；保持确认界面");
    const current = await state(ctx);
    assertOwnedState(ctx, current, main);
    invariant(!current.related && current.confirmation?.title === "关闭侧边会话"
      && current.confirmation.message === pendingPreview.confirmationMessage, "批准后关闭对象或确认框发生变化，需重新核对");
    await note(ctx, "action-time-side-close-approval", { approval });
    await action(ctx, "close-side", {
      kind: "side-close", threadId: side.id, contextThreadId: main.id,
    }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "关闭会话", exact: true }));
      await wait(ctx, "确认关闭后停留主会话", value => !value.confirmation && value.threadId === main.id && !value.related);
      await reloadPage(ctx);
      await wait(ctx, "刷新后主会话恢复", value => value.threadId === main.id && value.promptEditable);
      await ensureOwnedMarker(ctx, main);
      await ensureMenu(ctx);
      invariant(await ctx.tab.playwright.locator(`[data-menu-key="side:${css(side.id)}"]`).count() === 0, "已关闭侧边会话刷新后仍在列表");
      side.closed = true; delete ctx.report.pendingSideClosePreview;
      return { closedThreadId: side.id, parentThreadId: main.id };
    });
    await verifyNative(ctx, { threadIds: [side.id], parentThreadId: main.id,
      requirement: "原生关闭回执 closed=true 且准确侧边/父会话身份匹配，而非菜单暂时隐藏" });
  },
  async "lifecycle.pin"(ctx) {
    permit(ctx, "allowSessionMetadata");
    await idle(ctx); await rowMenu(ctx, "main");
    await action(ctx, "pin-main", { kind: "pin", threadId: owned(ctx, "main").id }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "置顶会话", exact: true }));
      await reloadPage(ctx);
      await wait(ctx, "置顶后刷新恢复同一会话", value => value.threadId === owned(ctx, "main").id && value.promptEditable);
      await rowMenu(ctx, "main");
      invariant(await ctx.tab.playwright.getByRole("menuitem", { name: "取消置顶会话", exact: true }).count() === 1,
        "刷新后菜单没有呈现已置顶状态");
      await ctx.tab.playwright.getByRole("menuitem", { name: "取消置顶会话", exact: true }).press("Escape");
      return { pinned: true };
    });
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], pinned: true });
  },
  async "lifecycle.rename"(ctx) {
    permit(ctx, "allowSessionMetadata");
    driver(ctx, "desktop", ["rename-thread", "observe-thread"]);
    await idle(ctx);
    const title = `远程回归 ${ctx.report.runId}`;
    await driverStep(ctx, "desktop", "rename-thread", { mutation: true, payload: { title },
      facts: [fact("title", title), fact("visibleTitle", title), fact("visibleThreadId", owned(ctx, "main").id)] });
    await wait(ctx, "Codex 改名同步到远程同一主会话", value => value.threadId === owned(ctx, "main").id && value.title === title);
    owned(ctx, "main").title = title;
    await reloadPage(ctx);
    await wait(ctx, "刷新后名称持久化", value => value.threadId === owned(ctx, "main").id && value.title === title);
    await driverStep(ctx, "desktop", "observe-thread", {
      facts: [fact("title", title), fact("visibleTitle", title), fact("visibleThreadId", owned(ctx, "main").id)],
    });
  },
  async "lifecycle.delete_preview"(ctx) {
    const { record } = await deletionDialog(ctx);
    const preview = await capture(ctx, "permanent-delete-preview");
    invariant(preview.files.screenshot, "删除确认前预览截图未保存");
    await note(ctx, "deletion-preview", { threadId: record.id, title: record.title, marker: record.marker });
    await click(ctx, ctx.tab.playwright.getByRole("button", { name: "取消操作", exact: true }));
    const after = await wait(ctx, "取消后会话保持存在", value => !value.confirmation && value.threadId === record.id);
    assertOwnedState(ctx, after, record);
  },
  async "lifecycle.delete_confirm"(ctx) {
    const { record, observation } = await deletionDialog(ctx);
    const preview = await capture(ctx, "permanent-delete-awaiting-approval");
    invariant(preview.files.screenshot, "无法保存删除确认截图，不能请求或执行删除");
    const id = `${ctx.report.runId}:${ctx.current.id}:delete-control`;
    let pendingPreview = ctx.report.pendingDeletionPreview;
    if (!pendingPreview || pendingPreview.actionId !== id || pendingPreview.threadId !== record.id
      || pendingPreview.confirmationMessage !== observation.confirmation.message) {
      pendingPreview = {
        runId: ctx.report.runId, actionId: id, threadId: record.id, title: record.title, marker: record.marker,
        url: preview.url, preparedAt: iso(), screenshotPath: preview.files.screenshot.path,
        confirmationMessage: observation.confirmation.message,
      };
      ctx.report.pendingDeletionPreview = pendingPreview;
      await checkpoint(ctx);
    }
    if (!ctx.options.authorizePermanentDelete)
      block("已停留在永久删除确认界面；缺少动作当时的外部授权回调 authorizePermanentDelete");
    const approval = await ctx.options.authorizePermanentDelete({
      ...pendingPreview,
      warning: "永久删除测试会话及记录，产品正常流程无法恢复",
    });
    if (!approval?.approved || approval.runId !== ctx.report.runId || approval.threadId !== record.id
      || approval.actionId !== id || !approval.evidencePath
      || !Number.isFinite(Date.parse(approval.approvedAt))
      || Date.parse(approval.approvedAt) < Date.parse(pendingPreview.preparedAt)
      || Date.parse(approval.approvedAt) > Date.now() + 5_000)
      block("未获得本次预览之后、匹配具体对象与操作的有效批准；保持确认界面");
    const current = await state(ctx);
    invariant(current.confirmation?.message === preview.observation.confirmation.message && current.threadId === record.id,
      "取得批准后删除目标或确认文字发生变化，必须重新批准");
    await note(ctx, "action-time-approval", { approval });
    await action(ctx, "delete-control", { kind: "permanent-delete", threadId: record.id }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "删除会话", exact: true }));
      await wait(ctx, "确认后会话退出且侧栏删除", value => !value.confirmation && value.threadId !== record.id
        && !value.sidebar.some(row => row.id === record.id));
      await reloadPage(ctx);
      const after = await wait(ctx, "刷新后目标会话保持消失", value => value.canCreate
        && value.threadId !== record.id && !value.sidebar.some(row => row.id === record.id));
      record.deleted = true;
      delete ctx.report.pendingDeletionPreview;
      return { deletedThreadId: record.id, url: after.url };
    });
    await verifyNative(ctx, { threadIds: [record.id], deleted: true,
      actionIds: Object.values(ctx.report.actions).filter(value => value.caseId === ctx.current.id
        && value.kind === "permanent-delete").map(value => value.id),
      requirement: "原生删除回执明确 deleted=true 且 threadId 匹配；只看侧栏隐藏不足以证明永久删除" });
  },
};

Object.assign(handlers, {
  async "side.multiple"(ctx) {
    const second = await createSideBlank(ctx, "side2"); await markSide(ctx, "side2");
    const side = owned(ctx, "side");
    invariant(side.id !== second.id && side.parentThreadId === second.parentThreadId, "多侧边 ID 或父子身份错误");
    for (const key of ["side", "side2", "side", "side2"]) {
      const observation = await openSide(ctx, key);
      invariant(observation.threadId === owned(ctx, key).id, "多个侧边切换到了错误 ID");
      await backToMain(ctx);
    }
    await reloadPage(ctx);
    await wait(ctx, "刷新后原父会话恢复", value => value.threadId === owned(ctx, "main").id && value.promptEditable);
    await ensureOwnedMarker(ctx, owned(ctx, "main")); await ensureMenu(ctx);
    const observation = await state(ctx);
    invariant([side.id, second.id].every(id => observation.sideEntries.filter(value => value.id === id).length === 1),
      "刷新后多侧边条目丢失或重复");
    await verifyNative(ctx, { threadIds: [side.id, second.id], parentThreadId: side.parentThreadId });
  },
  async "composer.multiline"(ctx) {
    await idle(ctx);
    const spec = message(ctx, "multiline", ctx.options.messages?.multiline);
    invariant(spec.text.includes("\n"), "多行测试消息没有换行");
    await fillOwnedDraft(ctx, "main", spec.text);
    await selectOwned(ctx, "control"); await selectOwned(ctx, "main");
    invariant((await state(ctx)).prompt === spec.text, "切换会话后多行草稿改变");
    await reloadPage(ctx);
    await wait(ctx, "刷新后多行草稿逐字保持", value => value.threadId === owned(ctx, "main").id && value.prompt === spec.text);
    const sent = await send(ctx, spec, { prepared: true });
    const final = await finalReply(ctx, sent);
    invariant(userTurn(final, sent).users.some(value => value.text.includes(spec.marker)), "发送后的多行消息缺失");
    await verifyNative(ctx, { threadIds: [sent.threadId], turnIds: [userTurn(final, sent).id],
      expectedFacts: { prompt: spec.text } });
  },
  async "composer.image"(ctx) {
    driver(ctx, "files", ["choose-image"]);
    const fixture = ctx.options.imageFixture;
    if (!fixture?.path || !fixture.sha256 || !fixture.name) block("需提供获准的本机真实 imageFixture path/sha256/name");
    await verifyEvidenceReference(fixture, { baseDir: ctx.outputDir });
    await idle(ctx);
    await click(ctx, ctx.tab.playwright.locator("#composer-add"));
    await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "添加图片", exact: true }));
    await driverStep(ctx, "files", "choose-image", { mutation: true, payload: { file: fixture },
      facts: [fact("fileSha256", fixture.sha256), fact("fileName", fixture.name), fact("pickerUsed", true)] });
    await wait(ctx, "真实图片选择器返回后待发送图片完成加载", value => value.attachments.length === 1
      && value.attachments[0].loaded && value.attachments[0].alt.includes(fixture.name));
    await click(ctx, ctx.tab.playwright.locator("#composer-attachments .composer-attachment-preview"));
    const preview = ctx.tab.playwright.getByRole("dialog", { name: new RegExp(fixture.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) });
    invariant(await preview.count() === 1, "图片预览弹层没有实际显示");
    await preview.press("Escape");
    const spec = await send(ctx, message(ctx, "image", ctx.options.messages?.image));
    const final = await finalReply(ctx, spec);
    invariant(final.messageImages.some(value => value.loaded), "真实发送后的图片没有渲染");
    await verifyNative(ctx, { threadIds: [spec.threadId], turnIds: [userTurn(final, spec).id],
      expectedFacts: { imageCount: 1, imageSha256: fixture.sha256 } });
  },
  async "composer.skill"(ctx) {
    const skill = ctx.options.skill;
    if (!skill?.name || !skill.path || !skill.displayName || !ctx.options.messages?.skill)
      block("需明确批准当前项目可用 skill name/path/displayName 和 messages.skill");
    await idle(ctx);
    const spec = message(ctx, "skill", ctx.options.messages.skill);
    await fillOwnedDraft(ctx, "main", `${spec.text}\n/${skill.name}`);
    const option = ctx.tab.playwright.getByRole("option", { name: skill.displayName, exact: true });
    await wait(ctx, "真实项目 Skill 菜单出现", async () => await option.count() === 1 && await option.isEnabled());
    await click(ctx, option);
    invariant((await state(ctx)).selectedSkills.some(value => value.text.includes(skill.displayName)), "选择后输入框没有 Skill 标记");
    const prepared = await state(ctx);
    invariant(prepared.prompt.includes(spec.marker), "选择 Skill 意外覆盖原多行消息");
    const sent = await send(ctx, { ...spec, text: prepared.prompt }, { prepared: true });
    const final = await finalReply(ctx, sent);
    await verifyNative(ctx, { threadIds: [sent.threadId], turnIds: [userTurn(final, sent).id],
      expectedFacts: { skillName: skill.name, skillPath: skill.path } });
  },
  async "composer.edit_latest"(ctx) {
    await idle(ctx);
    const original = await send(ctx, message(ctx, "edit-original"));
    const before = await finalReply(ctx, original);
    const turnId = userTurn(before, original).id;
    const button = ctx.tab.playwright.locator(`.turn[data-turn-id="${css(turnId)}"]`)
      .getByRole("button", { name: "编辑并再次发送", exact: true });
    if (await button.count() !== 1) block("最新真实已发消息没有唯一可用编辑入口");
    await click(ctx, button);
    await wait(ctx, "最新已发消息进入真实编辑器", value => value.editor?.includes(original.marker));
    const changed = message(ctx, "edit-replacement");
    await ctx.tab.playwright.getByRole("textbox", { name: "编辑最后一条消息", exact: true }).fill(changed.text);
    await click(ctx, ctx.tab.playwright.getByRole("button", { name: "取消编辑消息", exact: true }));
    invariant(userTurn(await state(ctx), original) && !(await state(ctx)).editor, "取消已发消息编辑改变了原历史");
    await click(ctx, button);
    await ctx.tab.playwright.getByRole("textbox", { name: "编辑最后一条消息", exact: true }).fill(changed.text);
    permit(ctx, "allowMessages");
    if (ctx.report.messageSubmissions >= ctx.options.authorization.maxMessageSubmissions) block("已发消息修改重发缺少剩余预算");
    await action(ctx, "edit-latest", { kind: "edit-message", threadId: original.threadId, turnId, marker: changed.marker }, async () => {
      ctx.report.messageSubmissions++; await checkpoint(ctx);
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "发送修改后的消息", exact: true }));
      const final = await finalReply(ctx, changed);
      invariant(!userTurn(final, original) && final.turns.filter(turn => turn.users.some(value =>
        value.text.includes(changed.marker))).length === 1, "重发后原消息未回滚或修改消息重复");
      return { turnId: userTurn(final, changed).id, originalTurnId: turnId };
    });
    await reloadPage(ctx);
    await wait(ctx, "刷新保持修改后的唯一消息", value => value.threadId === original.threadId && replied(value, changed) && !userTurn(value, original));
    await verifyNative(ctx, { threadIds: [original.threadId], expectedFacts: { editedPrompt: changed.text, editSubmissionCount: 1 } });
  },
  async "context.switch_update"(ctx) {
    driver(ctx, "native", ["observe-context"]);
    const first = await contextCheck(ctx, "main", "main-before");
    await contextCheck(ctx, "control", "control");
    const returned = await contextCheck(ctx, "main", "main-return");
    invariant(first.used === returned.used && first.total === returned.total, "无新轮次时切回主会话的上下文值串用");
    await idle(ctx);
    const spec = await send(ctx, message(ctx, "context-update"));
    await finalReply(ctx, spec);
    await contextCheck(ctx, "main", "main-new-turn");
    await verifyNative(ctx, { threadIds: [spec.threadId], expectedFacts: { tokenUsageTurnId: userTurn(await state(ctx), spec).id } });
  },
  async "context.zero_missing"(ctx) {
    driver(ctx, "native", ["observe-context"]);
    const fixtures = ctx.options.contextFixtures;
    if (!fixtures?.zeroKey || !fixtures.missingKey || fixtures.zeroKey === fixtures.missingKey)
      block("需用本轮已登记的两个真实会话提供 contextFixtures.zeroKey/missingKey；不得改写原生 tokenUsage 制造场景");
    for (const kind of ["zero", "missing"]) {
      await contextCheck(ctx, fixtures[`${kind}Key`], kind, kind === "zero" ? { usedTokens: 0 } : { usageAvailable: false });
    }
  },
});

async function optimizationStart(ctx, key, suffix) {
  permit(ctx, "allowOptimizations");
  const limit = ctx.options.authorization?.maxOptimizations;
  if (!Number.isInteger(limit) || (ctx.report.optimizationSubmissions || 0) >= limit)
    block("提示词优化需要明确 maxOptimizations 剩余额度，独立于消息提交预算");
  if (!ctx.options.messages?.optimization) block("缺少明确批准的 messages.optimization");
  await idle(ctx, key);
  const spec = message(ctx, `optimization-${suffix}`, ctx.options.messages.optimization);
  await fillOwnedDraft(ctx, key, spec.text);
  if (!(await state(ctx)).optimizer.enabled) block("当前真实服务没有可用提示词优化能力");
  await action(ctx, `optimize-${suffix}`, { kind: "optimizePrompt", threadId: owned(ctx, key).id, marker: spec.marker }, async () => {
    ctx.report.optimizationSubmissions = (ctx.report.optimizationSubmissions || 0) + 1;
    await checkpoint(ctx);
    await click(ctx, ctx.tab.playwright.getByRole("button", { name: "优化提示词", exact: true }));
    const after = await wait(ctx, "真实优化开始或完成", value => ["loading", "optimized"].includes(value.optimizer.state));
    return { initialPrompt: spec.text, initialState: after.optimizer.state };
  });
  return spec;
}

Object.assign(handlers, {
  async "optimizer.restore"(ctx) {
    const spec = await optimizationStart(ctx, "main", "restore");
    const optimized = await wait(ctx, "真实优化返回修改后的文本", value => value.optimizer.state === "optimized"
      && value.prompt.trim() && value.prompt !== spec.text, ctx.options.optimizationTimeoutMs ?? 180_000);
    const beforeUi = await capture(ctx, "optimized-before-restore");
    await click(ctx, ctx.tab.playwright.getByRole("button", { name: "恢复原提示词", exact: true }));
    await wait(ctx, "恢复逐字匹配的优化前原文", value => value.prompt === spec.text && value.optimizer.state === "idle");
    const afterUi = await capture(ctx, "original-prompt-restored");
    await note(ctx, "local-ui-restore", { threadId: owned(ctx, "main").id, originalInput: spec.text,
      optimizedPrompt: optimized.prompt, beforeUi, afterUi });
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id],
      expectedFacts: { optimizationState: "completed", originalInput: spec.text,
        optimizedPrompt: optimized.prompt, threadSubmissionCount: 0 } });
    await ctx.tab.playwright.locator("#prompt").fill("");
  },
  async "optimizer.cancel"(ctx) {
    driver(ctx, "native", ["wait-optimization-terminal"]);
    const spec = await optimizationStart(ctx, "main", "cancel");
    if ((await state(ctx)).optimizer.state !== "loading")
      block("实际优化过快，未出现可取消窗口；不自动追加调用");
    await action(ctx, "cancel-optimization", { kind: "cancelPromptOptimize", threadId: owned(ctx, "main").id }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "取消提示词优化", exact: true }));
      await wait(ctx, "取消后原文保持且取消按钮退出", value => value.optimizer.state === "idle" && value.prompt === spec.text);
      return { originalInput: spec.text };
    });
    await driverStep(ctx, "native", "wait-optimization-terminal", {
      facts: [fact("originalInput", spec.text), fact("optimizationState", ["cancelled", "completed"], "oneOf"), fact("threadSubmissionCount", 0)] });
    invariant((await state(ctx)).prompt === spec.text, "取消后的迟到结果覆盖原文");
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], expectedFacts: { originalInput: spec.text, threadSubmissionCount: 0 } });
    await ctx.tab.playwright.locator("#prompt").fill("");
  },
  async "optimizer.late_result"(ctx) {
    driver(ctx, "native", ["wait-optimization-terminal"]);
    const spec = await optimizationStart(ctx, "main", "late");
    if ((await state(ctx)).optimizer.state !== "loading") block("实际优化已完成，无法证明迟到隔离；不重发优化");
    const edited = `${spec.text}\n${marker(ctx, "edited-while-optimizing")}`;
    await ctx.tab.playwright.locator("#prompt").fill(edited);
    const controlDraft = marker(ctx, "optimization-control-draft");
    await fillOwnedDraft(ctx, "control", controlDraft);
    await driverStep(ctx, "native", "wait-optimization-terminal", {
      key: "control", payload: { optimizationThreadId: owned(ctx, "main").id, originalInput: spec.text },
      facts: [fact("optimizationThreadId", owned(ctx, "main").id),
        fact("optimizationState", ["cancelled", "completed"], "oneOf"), fact("originalInput", spec.text), fact("threadSubmissionCount", 0)] });
    invariant((await state(ctx)).prompt === controlDraft, "跨会话迟到结果覆盖 control 草稿");
    await selectOwned(ctx, "main");
    invariant((await state(ctx)).prompt === edited, "迟到结果覆盖主会话后来编辑的草稿");
    await capture(ctx, "late-optimization-preserved-new-draft");
    await ctx.tab.playwright.locator("#prompt").fill("");
    await selectOwned(ctx, "control"); await ctx.tab.playwright.locator("#prompt").fill("");
    await selectOwned(ctx, "main");
  },
  async "optimizer.no_injection"(ctx) {
    driver(ctx, "environment", ["disconnect-service", "restore-service",
      "observe-optimization", "observe-authorization"]);
    const readOnlyTab = ctx.options.readOnlyTab;
    if (!readOnlyTab?.playwright || !readOnlyTab.url || !readOnlyTab.screenshot)
      block("需显式绑定同一专用测试会话的 readOnlyTab，以真实只读授权检查独立优化权限");
    permit(ctx, "allowFaults");
    if (ctx.options.authorization?.maxOptimizations - (ctx.report.optimizationSubmissions || 0) < 2)
      block("独立优化案例至少需要两次独立优化额度（完成/恢复一次、取消一次）");
    await idle(ctx);
    const originalTab = ctx.tab;
    try {
      await driverStep(ctx, "environment", "disconnect-service", { mutation: true,
        facts: [fact("serviceConnected", false)], payload: { scope: "cli-service-only" } });
      const spec = await optimizationStart(ctx, "main", "without-native");
      const optimized = await wait(ctx, "会话服务不可用时独立优化实际完成", value =>
        value.optimizer.state === "optimized" && value.prompt.trim() && value.prompt !== spec.text,
      ctx.options.optimizationTimeoutMs ?? 180_000);
      await finalDriverContract(ctx, "environment", "observe-optimization", {
        originalInput: spec.text, optimizedPrompt: optimized.prompt });
      const beforeUi = await capture(ctx, "without-native-optimized");
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "恢复原提示词", exact: true }));
      await wait(ctx, "会话服务不可用时仍能恢复原文", value => value.prompt === spec.text && value.optimizer.state === "idle");
      const afterUi = await capture(ctx, "without-native-original-restored");
      await note(ctx, "local-ui-restore", { threadId: owned(ctx, "main").id, originalInput: spec.text,
        optimizedPrompt: optimized.prompt, beforeUi, afterUi });
      await ctx.tab.playwright.locator("#prompt").fill("");
      const cancelSpec = await optimizationStart(ctx, "main", "without-native-cancel");
      if ((await state(ctx)).optimizer.state !== "loading")
        block("会话服务不可用的第二次优化已完成，未建立真实取消窗口，不自动增加调用");
      await action(ctx, "cancel-without-native", { kind: "cancelPromptOptimize", threadId: owned(ctx, "main").id }, async () => {
        await click(ctx, ctx.tab.playwright.getByRole("button", { name: "取消提示词优化", exact: true }));
        await wait(ctx, "会话服务不可用取消后原文保持", value => value.optimizer.state === "idle" && value.prompt === cancelSpec.text);
        return { originalInput: cancelSpec.text };
      });
      await driverStep(ctx, "environment", "observe-optimization", { payload: { originalInput: cancelSpec.text, waitForTerminal: true },
        facts: [fact("serviceConnected", false),
          fact("optimizationState", ["cancelled", "completed"], "oneOf"), fact("originalInput", cancelSpec.text),
          fact("threadSubmissionCount", 0)] });
      invariant((await state(ctx)).prompt === cancelSpec.text, "会话服务不可用取消后迟到结果覆盖原文");
      await ctx.tab.playwright.locator("#prompt").fill("");
      // 只读授权页由调用方明确绑定；这里只读取可见控件，不修改授权或发送请求。
      ctx.tab = readOnlyTab;
      const readOnly = await state(ctx);
      invariant(readOnly.threadId === owned(ctx, "main").id, "只读测试页没有显示本轮同一 thread ID");
      invariant(!readOnly.optimizer.enabled && !readOnly.promptEditable && !readOnly.canCreate,
        "显式只读授权没有限制独立优化或写入控件");
      await capture(ctx, "explicit-readonly-without-native");
      await driverStep(ctx, "environment", "observe-authorization", { payload: { tabId: readOnlyTab.id, scope: "explicit-read-only" },
        facts: [fact("serviceConnected", false),
          fact("readOnly", true), fact("promptOptimizationAllowed", false)] });
    } finally {
      ctx.tab = originalTab;
      await restoreLeasedResources(ctx, ["service"]);
    }
    await wait(ctx, "优化独立性测试结束后会话服务恢复", value => value.threadId === owned(ctx, "main").id && value.promptEditable);
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], ...REMOTE_ACCESS_E2E_CONTRACTS["optimizer.no_injection"] });
  },
  async "recovery.draft"(ctx) {
    const drafts = Object.fromEntries(["main", "control", "side"].map(key => [key, `${marker(ctx, `draft-${key}`)}\n第二行 ${key}`]));
    for (const key of ["main", "control", "side"]) await fillOwnedDraft(ctx, key, drafts[key]);
    for (const key of ["main", "side", "control", "main"]) {
      await selectOwned(ctx, key);
      invariant((await state(ctx)).prompt === drafts[key], `会话 ${key} 恢复了其他会话草稿`);
      await reloadPage(ctx);
      await wait(ctx, `刷新后 ${key} 草稿逐字保持`, value => value.threadId === owned(ctx, key).id && value.prompt === drafts[key]);
      await ensureOwnedMarker(ctx, owned(ctx, key));
      await capture(ctx, `draft-restored-${key}`);
    }
    // 附件覆盖通过真实选择器和原始附件hash完成，不用 localStorage 模拟。
    if (!ctx.options.imageFixture) block("文字草稿切换及刷新已完成，附件草稿保持仍缺少 imageFixture 与 files.choose-image");
    driver(ctx, "files", ["choose-image"]);
    await click(ctx, ctx.tab.playwright.locator("#composer-add"));
    await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "添加图片", exact: true }));
    await driverStep(ctx, "files", "choose-image", { mutation: true, payload: { file: ctx.options.imageFixture },
      facts: [fact("fileSha256", ctx.options.imageFixture.sha256), fact("pickerUsed", true)] });
    await wait(ctx, "图片草稿已加载", value => value.attachments.length === 1 && value.attachments[0].loaded);
    await selectOwned(ctx, "control"); await selectOwned(ctx, "main"); await reloadPage(ctx);
    await wait(ctx, "图片和文字草稿跨会话刷新保持", value => value.threadId === owned(ctx, "main").id
      && value.prompt === drafts.main && value.attachments.length === 1 && value.attachments[0].loaded);
    await driverStep(ctx, "native", "observe-draft", { facts: [fact("draftText", drafts.main), fact("imageSha256", ctx.options.imageFixture.sha256)] });
    await click(ctx, ctx.tab.playwright.getByRole("button", { name: `移除图片 ${ctx.options.imageFixture.name}`, exact: true }));
    for (const key of ["main", "control", "side"]) {
      await selectOwned(ctx, key); await ctx.tab.playwright.locator("#prompt").fill("");
    }
    await selectOwned(ctx, "main");
  },
  async "queue.cross_session"(ctx) {
    permit(ctx, "allowQueueChanges");
    const active = await activeWindow(ctx, "cross-session");
    const queued = await send(ctx, message(ctx, "queue-cross-session"), { queued: true });
    await selectOwned(ctx, "control"); await selectOwned(ctx, "main");
    await reloadPage(ctx);
    const restored = await wait(ctx, "跨会话刷新仍为同一队列对象或已真实单次消费", value => value.threadId === queued.threadId
      && (value.queue.some(row => row.id === queued.queueId && row.text.includes(queued.marker)) || replied(value, queued)));
    await capture(ctx, "queue-cross-session-restored");
    const final = replied(restored, queued) ? restored : await finalReply(ctx, queued);
    invariant(final.turns.filter(turn => turn.users.some(user => user.text.includes(queued.marker))).length === 1,
      "跨会话恢复导致重复消费队列");
    await verifyNative(ctx, { threadIds: [queued.threadId], previousTurnId: active.turnId,
      nextTurnId: userTurn(final, queued).id, expectedFacts: { queuedMessageId: queued.queueId, consumedCount: 1 } });
  },
});

function rawDriverFact(result, name) {
  const proof = result.records.find(value => value.rawFacts?.[name]);
  if (!proof) block(`驱动缺少 ${name} 的原始字段指针`);
  return readFile(proof.evidencePath, "utf8").then(text =>
    jsonPointer(JSON.parse(text.replace(/^\uFEFF/, "")), proof.rawFacts[name]));
}

async function finalDriverContract(ctx, name, operation, payload = {}, key = "main") {
  const contract = REMOTE_ACCESS_E2E_CONTRACTS[ctx.current.id];
  invariant(contract, "环境案例缺少源码固定语义契约");
  const result = await driverStep(ctx, name, operation, { key, payload,
    facts: [...Object.entries(contract.expectedFacts || {}).map(([name, value]) => fact(name, value)),
      ...Object.entries(payload).filter(([name]) => /Id$|Ids$|^marker$|^draft$|^originalInput$|^optimizedPrompt$/.test(name))
        .map(([name, value]) => fact(name, value)), ...(contract.expectations || [])] });
  ctx.current.nativeExpectations = { threadIds: [owned(ctx, key).id], ...contract };
  ctx.current.nativeVerified = true;
  await checkpoint(ctx);
  return result;
}

Object.assign(handlers, {
  async "transport.service"(ctx) {
    await idle(ctx);
    const spec = await send(ctx, message(ctx, "transport-native"));
    const final = await finalReply(ctx, spec);
    await verifyNative(ctx, { threadIds: [spec.threadId], turnIds: [userTurn(final, spec).id],
      ...REMOTE_ACCESS_E2E_CONTRACTS["transport.service"] });
  },
  async "recovery.reconnect"(ctx) {
    driver(ctx, "environment", ["disconnect-network", "restore-network", "observe-recovery"]);
    await idle(ctx);
    const draft = `${marker(ctx, "reconnect-draft")}\n断线前第二行`;
    await fillOwnedDraft(ctx, "main", draft);
    try {
      await driverStep(ctx, "environment", "disconnect-network", { mutation: true,
        facts: [fact("networkConnected", false)], payload: { scope: "authorized-tab" } });
      await wait(ctx, "真实远程页面呈现断线状态", value => /离线|断开|重连|连接中/.test(value.connection));
      invariant((await state(ctx)).prompt === draft, "断线时草稿丢失");
      await capture(ctx, "actual-offline-draft");
    } finally {
      await restoreLeasedResources(ctx, ["tab-network"]);
    }
    await wait(ctx, "真实重新连接恢复相同会话和草稿", value => value.threadId === owned(ctx, "main").id
      && value.promptEditable && value.prompt === draft && !/离线|断开|重连|连接中/.test(value.connection));
    await reloadPage(ctx);
    await wait(ctx, "重连后刷新仍保持草稿", value => value.threadId === owned(ctx, "main").id && value.prompt === draft && value.promptEditable);
    await finalDriverContract(ctx, "environment", "observe-recovery", { draft });
    await ctx.tab.playwright.locator("#prompt").fill("");
  },
  async "recovery.native_restart"(ctx) {
    driver(ctx, "environment", ["restart-native", "observe-recovery"]);
    const active = await activeWindow(ctx, "native-restart");
    const draft = marker(ctx, "native-restart-draft");
    await fillOwnedDraft(ctx, "main", draft);
    await driverStep(ctx, "environment", "restart-native", { mutation: true,
      payload: { originalTurnId: active.turnId }, facts: [fact("nativeRestarted", true), fact("originalTurnId", active.turnId)] });
    const after = await wait(ctx, "原生重启后当前会话与活动轮恢复", value => value.threadId === active.threadId
      && value.promptEditable && value.prompt === draft && !!userTurn(value, active),
    ctx.options.turnTimeoutMs ?? 180_000);
    invariant(after.turns.filter(turn => turn.users.some(value => value.text.includes(active.marker))).length === 1,
      "重启恢复产生重复原消息");
    await finalDriverContract(ctx, "environment", "observe-recovery", { draft, originalTurnId: active.turnId });
    await ctx.tab.playwright.locator("#prompt").fill("");
  },
  async "recovery.unknown_receipt"(ctx) {
    driver(ctx, "environment", ["drop-next-action-receipt", "restore-receipts", "observe-original-receipt"]);
    await idle(ctx);
    await driverStep(ctx, "environment", "drop-next-action-receipt", { mutation: true,
      facts: [fact("receiptFaultArmed", true)], payload: { onlyNextAction: "send", preserveServerExecution: true } });
    const spec = message(ctx, "unknown-original");
    let originalActionId;
    try {
      // 首次点击后允许保留 unknown；本案例接下来只点击查原回执，不调用 send 第二次。
      await send(ctx, spec);
    } catch (error) {
      originalActionId = `${ctx.report.runId}:${ctx.current.id}:send-${spec.key}`;
      if (!ctx.report.actions[originalActionId]) throw error;
      await note(ctx, "expected-unknown-receipt", { originalActionId, reason: error.message });
    } finally {
      await driverStep(ctx, "environment", "restore-receipts", { mutation: true,
        facts: [fact("receiptFaultArmed", false)] });
    }
    originalActionId ||= `${ctx.report.runId}:${ctx.current.id}:send-${spec.key}`;
    const pending = await wait(ctx, "真实页面显示原送达待确认状态", value => /回执|送达|待确认/.test(value.pendingText));
    invariant(!pending.send.enabled || pending.send.action === "sending", "原操作未知时发送按钮没有防重复锁定");
    const check = ctx.tab.playwright.getByRole("button", { name: /查原回执|核对.*回执|核对发送结果|查询此消息的送达回执/ });
    if (await check.count() !== 1) block("真实待确认界面没有唯一只读核对原回执入口");
    await click(ctx, check);
    const proof = await finalDriverContract(ctx, "environment", "observe-original-receipt", { originalActionId, marker: spec.marker });
    const original = ctx.report.actions[originalActionId];
    const receiptId = await rawDriverFact(proof, "originalReceiptId");
    invariant(typeof receiptId === "string" && receiptId, "缺少最初请求真实回执 ID");
    const final = await finalReply(ctx, spec);
    invariant(final.turns.filter(turn => turn.users.some(value => value.text.includes(spec.marker))).length === 1,
      "原回执恢复产生重复消息");
    original.status = "confirmed"; original.finishedAt = iso();
    original.result = { threadId: final.threadId, turnId: userTurn(final, spec).id, receiptId, reconciledFromUnknown: true };
    await note(ctx, "unknown-reconciled", { actionId: originalActionId, receiptId, records: proof.records });
    await verifyNative(ctx, { threadIds: [final.threadId], ...REMOTE_ACCESS_E2E_CONTRACTS["recovery.unknown_receipt"] });
  },
  async "recovery.question_receipt"(ctx) {
    await questionReceiptRecovery(ctx);
  },
});

async function observeQueue(ctx, operation, spec, facts = [], payload = {}) {
  return driverStep(ctx, "native", operation, { payload: { marker: spec.marker, queueId: spec.queueId, ...payload }, facts });
}

async function observeQueueRecoveryPhase(ctx, spec, phase, activeTurnId, kind) {
  const result = await observeQueue(ctx, "observe-queue-boundary", spec, [
    fact("phase", phase), fact("activeTurnId", activeTurnId),
  ], {
    phase, activeTurnId, queueActionId: spec.actionId, transitionKind: kind,
  });
  invariant(result.records.length === 1, `原生 ${phase} 队列读取必须唯一`);
  return result.records[0];
}

async function queueSendReceipt(ctx, queued) {
  const actionRecord = ctx.report.actions[queued.actionId];
  if (!actionRecord || actionRecord.kind !== "queue-send" || actionRecord.status !== "confirmed")
    block("原 queue-send 动作未确认；停止/失败后不能猜测消息是否已排队");
  const receipts = await verifyNativeActionsNow(ctx, {
    actions: [actionRecord],
    threadIds: [queued.threadId],
    expectedFacts: { queueId: queued.queueId, deliveryState: "queued" },
  });
  const matches = receipts.filter(record => record.actionId === actionRecord.id
    && record.facts.threadId === queued.threadId && record.facts.queueId === queued.queueId
    && record.facts.deliveryState === "queued" && typeof record.facts.clientUserMessageId === "string"
    && record.facts.clientUserMessageId.trim());
  if (matches.length !== 1) block("原 queue-send 没有唯一正式回执及 clientUserMessageId");
  return matches[0];
}

async function finishQueueRecovery(ctx, {
  queued, activeTurnId, trigger, triggerActionId = null, before, transition, receipt,
}) {
  const expected = {
    kind: trigger, threadId: queued.threadId, activeTurnId,
    queueId: queued.queueId, clientUserMessageId: receipt.facts.clientUserMessageId,
    queueActionId: queued.actionId, prompt: queued.text, expectedReply: queued.expected,
  };
  let decision;
  try {
    decision = classifyQueueRecoveryTransition({
      before: before.facts, transition: transition.facts, receipt,
    }, expected);
  } catch (error) {
    block(`原生队列转场证据不足，保持未知且不执行恢复动作：${error.message}`);
  }
  if (decision.outcome === "unknown")
    block("原生队列转场状态未知；不重发同文、不执行引导，保留原 Q/T/client 供只读核对");

  let recoveryActionId = null;
  if (decision.outcome === "automatically-accepted") {
    await finalReply(ctx, queued);
  } else {
    const recovery = await steerRetained(ctx, queued);
    recoveryActionId = recovery.actionId;
    const actionRecord = ctx.report.actions[recoveryActionId];
    const recoveryProof = await verifyNativeActionsNow(ctx, {
      actions: [actionRecord],
      threadIds: [queued.threadId],
      turnIds: [recovery.turnId],
      expectedFacts: { queueId: queued.queueId, turnId: recovery.turnId },
    });
    if (!recoveryProof.some(record => record.actionId === recoveryActionId
      && record.facts.queueId === queued.queueId && record.facts.turnId === recovery.turnId))
      block("原 Q 的显式恢复缺少同 queue/client 绑定的原生回执");
  }

  const terminal = await observeQueueRecoveryPhase(ctx, queued, "terminal", activeTurnId, trigger);
  const stage = {
    kind: "queue-recovery", caseId: ctx.current.id, trigger, threadId: queued.threadId,
    activeTurnId, queueId: queued.queueId, clientUserMessageId: expected.clientUserMessageId,
    queueActionId: queued.actionId, triggerActionId, recoveryActionId,
    prompt: queued.text, expectedReply: queued.expected, outcome: decision.outcome,
  };
  await note(ctx, "queue-recovery", stage);
  try {
    return verifyQueueRecoveryTerminal({
      before: before.facts, transition: transition.facts, terminal: terminal.facts, receipt,
    }, { ...expected, outcome: decision.outcome });
  } catch (error) {
    block(`原 client/完整 input/最终轮证据不闭合；保留现状且不补发：${error.message}`);
  }
}

async function steerRetained(ctx, spec) {
  const row = (await state(ctx)).queue.find(value => value.id === spec.queueId);
  if (!row?.steerEnabled) block("真实保留队列没有可用引导入口；不能通过重新发送冒充原队列恢复");
  const actionId = `${ctx.report.runId}:${ctx.current.id}:resume-${spec.key}`;
  const result = await action(ctx, `resume-${spec.key}`, { kind: "queue-steer", threadId: spec.threadId, queueId: spec.queueId }, async () => {
    await click(ctx, queueLocator(ctx, spec.queueId).getByRole("button", { name: "引导此队列消息", exact: true }));
    const final = await finalReply(ctx, spec);
    invariant(final.turns.filter(turn => turn.users.some(user => user.text.includes(spec.marker))).length === 1,
      "保留队列恢复后产生重复执行");
    return { turnId: userTurn(final, spec).id };
  });
  return { ...result, actionId };
}

async function approvedRequest(ctx) {
  permit(ctx, "allowApprovals");
  const ui = ctx.options.approvalUI;
  if (!ui?.requestIncludes || !ui.buttonName) block("需提供已批准的 approvalUI.requestIncludes/buttonName，不能批准任意出现的请求");
  const observation = await wait(ctx, "真实原生审批请求出现", value => value.approvalText.includes(ui.requestIncludes));
  await capture(ctx, "actual-approval-request");
  await driverStep(ctx, "native", "observe-request", {
    facts: [fact("requestKind", "approval"), fact("requestText", ui.requestIncludes, "includes"), fact("pending", true)] });
  invariant(observation.approvalText.includes(ui.requestIncludes), "实际审批范围与批准内容不一致");
  await action(ctx, "resolve-approval", { kind: "approve", threadId: owned(ctx, "main").id }, async () => {
    const control = ctx.tab.playwright.locator("#approval-area").getByRole("button", { name: ui.buttonName, exact: true });
    invariant(await control.count() === 1 && await control.isEnabled(), "实际批准按钮不唯一或不可用");
    await click(ctx, control);
    await wait(ctx, "原审批卡片退出", value => !value.approvalText.includes(ui.requestIncludes));
    return { decisionLabel: ui.buttonName, approvedRequestText: ui.requestIncludes };
  });
}

Object.assign(handlers, {
  async "queue.after_stop"(ctx) {
    driver(ctx, "native", ["observe-queue-boundary"]);
    permit(ctx, "allowQueueChanges");
    const active = await activeWindow(ctx, "queue-stop");
    const queued = await send(ctx, message(ctx, "queue-after-stop"), { queued: true });
    const receipt = await queueSendReceipt(ctx, queued);
    const before = await observeQueueRecoveryPhase(ctx, queued, "before", active.turnId, "stop");
    const stopActionId = `${ctx.report.runId}:${ctx.current.id}:stop-with-queue`;
    await action(ctx, "stop-with-queue", { kind: "stop", threadId: active.threadId, turnId: active.turnId }, async () => {
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "停止生成", exact: true }));
      await wait(ctx, "停止后原轮次中断", value => userTurn(value, active)?.stopped);
      return { turnId: active.turnId };
    });
    await verifyNativeActionsNow(ctx, {
      actions: [ctx.report.actions[stopActionId]], threadIds: [queued.threadId], turnIds: [active.turnId],
    });
    const transition = await observeQueueRecoveryPhase(ctx, queued, "transition", active.turnId, "stop");
    await finishQueueRecovery(ctx, {
      queued, activeTurnId: active.turnId, trigger: "stop", triggerActionId: stopActionId,
      before, transition, receipt,
    });
  },
  async "queue.after_failure"(ctx) {
    driver(ctx, "environment", ["fail-active-upstream", "restore-upstream"]);
    driver(ctx, "native", ["observe-queue-boundary"]);
    permit(ctx, "allowQueueChanges");
    const active = await activeWindow(ctx, "queue-failure");
    const queued = await send(ctx, message(ctx, "queue-after-failure"), { queued: true });
    const receipt = await queueSendReceipt(ctx, queued);
    const before = await observeQueueRecoveryPhase(ctx, queued, "before", active.turnId, "failure");
    try {
      await driverStep(ctx, "environment", "fail-active-upstream", { mutation: true,
        payload: { turnId: active.turnId, scope: "original-request-only" },
        facts: [fact("affectedTurnId", active.turnId), fact("realUpstreamFailure", true)] });
      await wait(ctx, "实际失败轮次报错", value => !!userTurn(value, active)?.error);
    } finally {
      await restoreLeasedResources(ctx, ["upstream"]);
    }
    const transition = await observeQueueRecoveryPhase(ctx, queued, "transition", active.turnId, "failure");
    await finishQueueRecovery(ctx, {
      queued, activeTurnId: active.turnId, trigger: "failure", before, transition, receipt,
    });
  },
  async "queue.approval_wait"(ctx) {
    driver(ctx, "native", ["observe-request", "observe-queue-boundary"]);
    if (!ctx.options.messages?.approval || !ctx.options.approvalUI) block("需预先批准能触发真实审批的 messages.approval 和具体审批范围");
    permit(ctx, "allowApprovals"); permit(ctx, "allowQueueChanges");
    await idle(ctx);
    const active = await send(ctx, message(ctx, "queue-approval-source", ctx.options.messages.approval));
    await wait(ctx, "真实审批卡片出现且活动轮仍未完成", value => !!value.approvalText && value.send.action === "stop");
    const queued = await send(ctx, message(ctx, "queue-during-approval"), { queued: true });
    await observeQueue(ctx, "observe-queue-boundary", queued,
      [fact("pendingKind", "approval"), fact("consumedBeforeResolution", 0), fact("queueContainsMarker", queued.marker)], { phase: "approval-wait" });
    await approvedRequest(ctx);
    await finalReply(ctx, active);
    const final = await finalReply(ctx, queued);
    invariant(final.turns.filter(turn => turn.users.some(user => user.text.includes(queued.marker))).length === 1,
      "审批后队列重复承接");
    await finalDriverContract(ctx, "native", "observe-queue-boundary", { marker: queued.marker, queueId: queued.queueId, phase: "approval-resolved" });
    await verifyNative(ctx, { threadIds: [queued.threadId], ...REMOTE_ACCESS_E2E_CONTRACTS["queue.approval_wait"] });
  },
  async "queue.compaction"(ctx) {
    driver(ctx, "native", ["observe-compaction", "observe-queue-boundary"]);
    permit(ctx, "allowCompaction"); permit(ctx, "allowQueueChanges");
    await idle(ctx);
    await action(ctx, "compact-with-queue", { kind: "compact", threadId: owned(ctx, "main").id }, async () => {
      await click(ctx, ctx.tab.playwright.locator("#composer-add"));
      const compact = ctx.tab.playwright.getByRole("menuitem", { name: "压缩", exact: true });
      if (await compact.count() !== 1 || !await compact.isEnabled()) block("当前原生会话尚不满足压缩条件");
      await click(ctx, compact);
      await wait(ctx, "实际压缩进入允许排队状态", value => value.send.label === "发送到原生消息队列");
      return { compactionRequested: true };
    });
    const queued = await send(ctx, message(ctx, "queue-during-compaction"), { queued: true, compactionQueue: true });
    await observeQueue(ctx, "observe-queue-boundary", queued,
      [fact("compactionStatus", ["pending", "running"], "oneOf"), fact("consumedDuringCompaction", 0)], { phase: "compacting" });
    const final = await finalReply(ctx, queued);
    invariant(final.turns.filter(turn => turn.users.some(user => user.text.includes(queued.marker))).length === 1,
      "压缩完成后队列重复执行");
    await finalDriverContract(ctx, "native", "observe-queue-boundary", { queueId: queued.queueId, marker: queued.marker, phase: "compaction-completed" });
    await verifyNative(ctx, { threadIds: [queued.threadId], ...REMOTE_ACCESS_E2E_CONTRACTS["queue.compaction"] });
  },
  async "queue.dequeue_edit_race"(ctx) { await queueRace(ctx, "edit"); },
  async "queue.dequeue_remove_race"(ctx) { await queueRace(ctx, "remove"); },
  async "read.side_aggregate"(ctx) {
    driver(ctx, "native", ["observe-side-aggregate"]);
    readStateReady(ctx);
    const first = owned(ctx, "side"), second = owned(ctx, "side2"), parent = owned(ctx, "main");
    const specs = [];
    for (const key of ["side", "side2"]) {
      const spec = await startUnreadTurn(ctx, key, `aggregate-${key}`);
      specs.push(spec); await backToMain(ctx);
    }
    await ensureMenu(ctx);
    await wait(ctx, "两个侧边真实完成且父会话聚合未读", value => value.threadId === parent.id && value.parentMenuUnread
      && [first.id, second.id].every(id => value.sideEntries.some(entry => entry.id === id && entry.unread && !entry.active)),
    ctx.options.turnTimeoutMs ?? 180_000);
    await driverStep(ctx, "native", "observe-side-aggregate", { facts: [fact("unreadSideIds", [first.id, second.id]), fact("parentSideUnread", true)] });
    await openSide(ctx, "side"); await finalReply(ctx, specs[0]); await backToMain(ctx); await ensureMenu(ctx);
    await wait(ctx, "只读一个侧边后另一个仍未读且父标记保持", value => value.parentMenuUnread
      && value.sideEntries.some(entry => entry.id === first.id && !entry.unread)
      && value.sideEntries.some(entry => entry.id === second.id && entry.unread));
    await driverStep(ctx, "native", "observe-side-aggregate", { facts: [fact("unreadSideIds", [second.id]), fact("parentSideUnread", true)] });
    await openSide(ctx, "side2"); await finalReply(ctx, specs[1]); await backToMain(ctx); await ensureMenu(ctx);
    await wait(ctx, "两个侧边都阅读后父标识才清除", value => !value.parentMenuUnread
      && [first.id, second.id].every(id => value.sideEntries.some(entry => entry.id === id && !entry.unread)));
    await driverStep(ctx, "native", "observe-side-aggregate", { facts: [fact("unreadSideIds", []), fact("parentSideUnread", false)] });
    await reloadPage(ctx); await wait(ctx, "刷新原父会话", value => value.threadId === parent.id && value.promptEditable);
    await ensureMenu(ctx);
    invariant(!(await state(ctx)).parentMenuUnread, "多侧边已读刷新后聚合标记回退");
    const again = await startUnreadTurn(ctx, "side", "aggregate-again"); await backToMain(ctx); await ensureMenu(ctx);
    await wait(ctx, "已读侧边新回复再次产生父未读", value => value.parentMenuUnread
      && value.sideEntries.some(entry => entry.id === first.id && entry.unread && !entry.active), ctx.options.turnTimeoutMs ?? 180_000);
    await driverStep(ctx, "native", "observe-side-aggregate", { facts: [fact("unreadSideIds", [first.id]), fact("parentSideUnread", true)] });
    await openSide(ctx); await finalReply(ctx, again); await backToMain(ctx);
    await verifyNative(ctx, { threadIds: [first.id, second.id], parentThreadId: parent.id });
  },
  async "read.cross_client"(ctx) {
    driver(ctx, "desktop", ["open-thread", "observe-thread"]);
    readStateReady(ctx);
    const main = owned(ctx, "main");
    for (const cycle of [1, 2]) {
      await selectOwned(ctx, "main");
      await driverStep(ctx, "desktop", "open-thread", { mutation: true,
        payload: { sequence: `away-${cycle}`, targetThreadId: owned(ctx, "control").id, targetMarker: owned(ctx, "control").marker },
        facts: [fact("visibleThreadId", owned(ctx, "control").id)] });
      const spec = await startUnreadTurn(ctx, "main", `cross-${cycle}`);
      await selectOwned(ctx, "control");
      await wait(ctx, "远程离开后目标实际未读", value => rowReadState(value, main.id, true), ctx.options.turnTimeoutMs ?? 180_000);
      await nativeReadState(ctx, { phase: `cross-${cycle}-unread`, threadId: main.id, turnId: spec.turnId, hasUnreadTurn: true });
      await driverStep(ctx, "desktop", "open-thread", { key: "control", mutation: true,
        payload: { sequence: `read-${cycle}`, targetThreadId: main.id, targetMarker: main.marker },
        facts: [fact("visibleThreadId", main.id), fact("readThreadId", main.id), fact("hasUnreadTurn", false)] });
      await wait(ctx, "Codex 端阅读后远程未读同步清除", value => value.threadId === owned(ctx, "control").id && rowReadState(value, main.id, false));
      await nativeReadState(ctx, { phase: `cross-${cycle}-desktop-read`, threadId: main.id, turnId: spec.turnId, hasUnreadTurn: false });
      await selectOwned(ctx, "main"); await finalReply(ctx, spec);
    }
    await verifyNative(ctx, { threadIds: [main.id, owned(ctx, "control").id] });
  },
  async "side.close_running"(ctx) {
    driver(ctx, "native", ["observe-side-lifecycle"]);
    if (!ctx.options.messages?.stop) block("运行中关闭侧边需已批准的 messages.stop");
    const side = await createSideBlank(ctx, "runningSide"); await markSide(ctx, "runningSide");
    const active = await send(ctx, message(ctx, "running-side-close", ctx.options.messages.stop), { key: "runningSide" });
    await wait(ctx, "新侧边真实执行尚未完成", value => value.threadId === side.id && value.send.action === "stop" && !!userTurn(value, active));
    await backToMain(ctx);
    ctx.current.sideCloseKey = "runningSide";
    await handlers["lifecycle.side_close"](ctx);
    await finalDriverContract(ctx, "native", "observe-side-lifecycle", { sideThreadId: side.id, parentThreadId: side.parentThreadId, originalMarker: active.marker });
    invariant((await state(ctx)).threadId === owned(ctx, "main").id && (await state(ctx)).promptEditable, "关闭执行中侧边影响了父会话");
  },
  async "side.delete_parent"(ctx) {
    driver(ctx, "native", ["observe-side-lifecycle"]);
    await createOne(ctx, "deleteParent");
    const side = await createSideBlank(ctx, "parentDeleteSide", "deleteParent");
    await markSide(ctx, "parentDeleteSide");
    await backToMain(ctx, "deleteParent");
    await verifyNative(ctx, { threadIds: [owned(ctx, "deleteParent").id, side.id] });
    ctx.current.deletionKey = "deleteParent";
    await handlers["lifecycle.delete_confirm"](ctx);
    await selectOwned(ctx, "main");
    await finalDriverContract(ctx, "native", "observe-side-lifecycle", {
      deletedParentThreadId: ctx.report.threads.deleteParent.id, sideThreadId: side.id,
    });
    side.closed = true;
  },
});

async function queueRace(ctx, kind) {
  driver(ctx, "native", ["observe-dequeue-window", "observe-queue-race"]);
  driver(ctx, "environment", ["arm-queue-race-request", "observe-queue-race-request", "release-queue-race-request",
    "acquire-environment-lease", "restore-environment-resource", "inspect-environment-lease"]);
  if (typeof ctx.options.nativeEvidence !== "function")
    block("自然出队竞争缺少 nativeEvidence 原 queued 回执和旧 POST 原回执的只读核对能力");
  permit(ctx, "allowQueueChanges");
  await withEnvironment(ctx, ["receipts"], async () => {
    const active = await activeWindow(ctx, `race-${kind}`);
    const queued = await send(ctx, message(ctx, `queue-race-${kind}`), { queued: true });
    const receipt = await queueSendReceipt(ctx, queued);
    const expected = {
      origin: ctx.origin, tabId: ctx.tab.id, threadId: queued.threadId, activeTurnId: active.turnId,
      queueId: queued.queueId, queueActionId: queued.actionId, attemptedKind: kind, prompt: queued.text,
      clientUserMessageId: receipt.facts.clientUserMessageId,
    };
    const one = result => {
      if (result.records.length !== 1) block("自然出队竞争每阶段必须只有一份准确原始记录");
      return result.records[0].facts;
    };
    // queueRace.serverQueue 取服务端队列；nativeQueue 取原生组合显示投影，不推算独立本地物理队列。
    const before = one(await observeQueue(ctx, "observe-dequeue-window", queued, [fact("phase", "before")],
      { phase: "before", ...expected }));
    await driverStep(ctx, "environment", "arm-queue-race-request", { mutation: true,
      payload: { phase: "armed", ...expected,
        requestPath: `/api/action/${kind === "edit" ? "beginQueuedEdit" : "removeQueued"}`,
        requestStage: "Request" } });
    await queueMenu(ctx, queued.queueId);
    const item = ctx.tab.playwright.getByRole("menuitem", {
      name: kind === "edit" ? "编辑消息" : "删除排队消息", exact: true,
    });
    if (await item.count() !== 1 || !await item.isEnabled())
      block("原 Q 已不再可操作，未派发旧写；不暂停调度或重新入队制造窗口");
    const raceActionId = `${ctx.report.runId}:${ctx.current.id}:race-${kind}`;
    const result = await action(ctx, `race-${kind}`, { kind: `queue-race-${kind}`,
      threadId: queued.threadId, queueId: queued.queueId }, async () => {
      await click(ctx, item);
      const held = one(await driverStep(ctx, "environment", "observe-queue-race-request", {
        payload: { phase: "held", ...expected }, facts: [fact("phase", "held")],
      }));
      // 原请求仍在浏览器 Request 阶段；仅等待原 T 的有限任务自然结束，不设置调度暂停。
      await wait(ctx, "旧写暂缓期间原 Q 自然得到真实回复", value =>
        value.threadId === queued.threadId && replied(value, queued), ctx.options.turnTimeoutMs ?? 180_000);
      const consumed = one(await observeQueue(ctx, "observe-queue-race", queued, [fact("phase", "consumed")],
        { phase: "consumed", ...expected }));
      try { verifyQueueDequeueRaceConsumption({ before, held, consumed, receipt }, expected); }
      catch (error) { block(`放行前原 Q 自然消费证据不足：${error.message}`); }
      const released = one(await driverStep(ctx, "environment", "release-queue-race-request", {
        mutation: true, payload: { phase: "released", ...expected }, facts: [fact("phase", "released")],
      }));
      const after = one(await observeQueue(ctx, "observe-queue-race", queued, [fact("phase", "after")],
        { phase: "after", ...expected }));
      const data = { before, held, consumed, released, after, receipt };
      try { return verifyQueueDequeueRaceFacts(data, expected); }
      catch (error) { block(`同一旧 POST 与过期回执尚未闭环：${error.message}`); }
    });
    await note(ctx, "queue-dequeue-race", {
      ...expected, caseId: ctx.current.id, raceActionId, receiptId: result.receiptId,
    });
    await verifyNative(ctx, {
      threadIds: [queued.threadId], actionIds: [raceActionId], requiresQueueDequeueRace: true,
    });
  });
}

async function projectFixture(ctx) {
  driver(ctx, "desktop", ["create-project", "observe-project"]);
  permit(ctx, "allowProjects");
  const fixture = ctx.options.projectFixture;
  if (!fixture?.name?.includes(ctx.report.runId) || !fixture.path?.includes(ctx.report.runId))
    block("需为本轮提供带 runId 的 projectFixture.name/path；只能置顶本轮新建专用项目");
  if (!ctx.report.project) {
    const result = await driverStep(ctx, "desktop", "create-project", {
      mutation: true, payload: { projectName: fixture.name, projectPath: fixture.path },
      facts: [fact("projectName", fixture.name), fact("projectPath", fixture.path), fact("createdProjectCount", 1)] });
    const id = await rawDriverFact(result, "projectId");
    invariant(typeof id === "string" && id, "真实项目创建回执缺少项目 ID");
    ctx.report.project = { id, name: fixture.name, path: fixture.path, runId: ctx.report.runId };
    await checkpoint(ctx);
  }
  await driverStep(ctx, "desktop", "observe-project", { facts: [fact("projectId", ctx.report.project.id),
    fact("projectName", ctx.report.project.name), fact("projectPath", ctx.report.project.path)] });
  return ctx.report.project;
}

async function showProjectView(ctx) {
  await openSidebar(ctx);
  const toggle = ctx.tab.playwright.locator("#sidebar-view-toggle");
  if (await toggle.getAttribute("data-mode") !== "project") await click(ctx, toggle);
  await wait(ctx, "真实侧栏切换到项目视图", async () => await toggle.getAttribute("data-mode") === "project");
}

Object.assign(handlers, {
  async "project.pin_unpin"(ctx) {
    const project = await projectFixture(ctx);
    await idle(ctx);
    const originalMode = (await state(ctx)).sidebarMode;
    try {
    await showProjectView(ctx);
    for (const pinned of [true, false]) {
      const menu = ctx.tab.playwright.getByRole("button", { name: `${project.name} 的项目菜单`, exact: true });
      if (await menu.count() !== 1) block("新专用项目尚未出现在当前真实侧栏");
      await click(ctx, menu);
      await action(ctx, `project-${pinned ? "pin" : "unpin"}`, { kind: "project-pin",
        threadId: owned(ctx, "main").id, projectId: project.id }, async () => {
        await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: pinned ? "置顶项目" : "取消置顶项目", exact: true }));
        await wait(ctx, "项目移动到准确分组", value => value.projectGroups.some(value => value.title === project.name && value.pinned === pinned));
        await reloadPage(ctx);
        await wait(ctx, "刷新恢复原测试会话", value => value.threadId === owned(ctx, "main").id && value.promptEditable);
        await showProjectView(ctx);
        invariant((await state(ctx)).projectGroups.filter(value => value.title === project.name && value.pinned === pinned).length === 1,
          "刷新后的专用项目置顶状态不一致或重复");
        return { projectId: project.id, pinned };
      });
      await driverStep(ctx, "desktop", "observe-project", {
        facts: [fact("projectId", project.id), fact("projectName", project.name), fact("pinned", pinned)] });
    }
    } finally {
      if (["project", "list"].includes(originalMode) && (await state(ctx)).sidebarMode !== originalMode) {
        await click(ctx, ctx.tab.playwright.locator("#sidebar-view-toggle"));
        await wait(ctx, "项目置顶回归后恢复原会话视图", value => value.sidebarMode === originalMode);
      }
    }
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], expectedFacts: { projectId: project.id, pinned: false } });
  },
  async "project.search_preferences"(ctx) {
    const project = await projectFixture(ctx);
    await idle(ctx);
    const original = await state(ctx);
    try {
      await showProjectView(ctx);
      const toggle = ctx.tab.playwright.locator("#sidebar-search-toggle");
      if (await toggle.getAttribute("aria-expanded") !== "true") await click(ctx, toggle);
      await ctx.tab.playwright.getByRole("searchbox", { name: "搜索会话与项目", exact: true }).fill(project.name);
      await wait(ctx, "按专用项目名称搜索只显示对应项目", value => value.search === project.name
        && value.projectGroups.length === 1 && value.projectGroups[0].title === project.name);
      await capture(ctx, "project-search-result");
      await ctx.tab.playwright.getByRole("searchbox", { name: "搜索会话与项目", exact: true }).fill("");
      for (const expectedMode of ["project", "list"]) {
        if ((await state(ctx)).sidebarMode !== expectedMode)
          await click(ctx, ctx.tab.playwright.locator("#sidebar-view-toggle"));
        await wait(ctx, "本客户端切换到所选视图", value => value.sidebarMode === expectedMode);
        await reloadPage(ctx);
        await wait(ctx, "本客户端偏好刷新后保持", value => value.threadId === owned(ctx, "main").id
          && value.sidebarMode === expectedMode && value.promptEditable);
        await capture(ctx, `client-preference-${expectedMode}-reloaded`);
      }
    } finally {
      if (["project", "list"].includes(original.sidebarMode) && (await state(ctx)).sidebarMode !== original.sidebarMode)
        await click(ctx, ctx.tab.playwright.locator("#sidebar-view-toggle"));
      const searchToggle = ctx.tab.playwright.locator("#sidebar-search-toggle");
      if (original.search && await searchToggle.getAttribute("aria-expanded") !== "true") await click(ctx, searchToggle);
      if (await searchToggle.getAttribute("aria-expanded") === "true")
        await ctx.tab.playwright.getByRole("searchbox", { name: "搜索会话与项目", exact: true }).fill(original.search);
    }
    // 这里只核对搜索夹具确实是该原生项目；视图保存本身没有原生写入回执。
    await verifyNative(ctx, { actions: [], threadIds: [owned(ctx, "main").id],
      expectedFacts: { projectId: project.id, projectName: project.name, projectPath: project.path } });
  },
  async "scenario.approval"(ctx) {
    driver(ctx, "native", ["observe-request", "observe-scenario"]);
    if (!ctx.options.messages?.approval || !ctx.options.approvalUI) block("缺少获准的真实审批请求消息与明确请求范围");
    permit(ctx, "allowApprovals");
    await idle(ctx);
    const spec = await send(ctx, message(ctx, "approval-scenario", ctx.options.messages.approval));
    await approvedRequest(ctx);
    await finalReply(ctx, spec);
    await finalDriverContract(ctx, "native", "observe-scenario", { marker: spec.marker });
    await verifyNative(ctx, { threadIds: [spec.threadId], ...REMOTE_ACCESS_E2E_CONTRACTS["scenario.approval"] });
  },
  async "scenario.question"(ctx) {
    await questionScenario(ctx, false);
  },
  async "scenario.question_completed"(ctx) {
    await questionScenario(ctx, true);
  },
  async "scenario.compaction"(ctx) {
    driver(ctx, "native", ["observe-compaction", "observe-scenario"]);
    permit(ctx, "allowCompaction");
    await idle(ctx);
    await action(ctx, "compact-real-context", { kind: "compact", threadId: owned(ctx, "main").id }, async () => {
      await click(ctx, ctx.tab.playwright.locator("#composer-add"));
      const compact = ctx.tab.playwright.getByRole("menuitem", { name: "压缩", exact: true });
      if (await compact.count() !== 1 || !await compact.isEnabled()) block("当前真实上下文不足或能力不可用，无法执行压缩");
      await click(ctx, compact);
      await driverStep(ctx, "native", "observe-compaction", {
        facts: [fact("compactionStatus", "completed")], payload: { waitForCompletion: true } });
      await wait(ctx, "压缩完成后恢复正常输入", value => value.promptEditable && value.send.label !== "发送到原生消息队列");
      return { compactionStatus: "completed" };
    });
    const draft = marker(ctx, "after-compaction-unsent");
    await fillOwnedDraft(ctx, "main", draft);
    invariant((await state(ctx)).send.enabled, "压缩完成后仍无法发送");
    await ctx.tab.playwright.locator("#prompt").fill("");
    await finalDriverContract(ctx, "native", "observe-scenario");
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], ...REMOTE_ACCESS_E2E_CONTRACTS["scenario.compaction"] });
  },
  async "scenario.goal"(ctx) {
    driver(ctx, "native", ["observe-goal", "observe-scenario"]);
    permit(ctx, "allowGoals"); permit(ctx, "allowMessages");
    if (!ctx.options.messages?.goal) block("缺少明确批准的 messages.goal；不能自动创建任意持续目标");
    const limit = ctx.options.authorization?.maxMessageSubmissions;
    if (!Number.isInteger(limit) || ctx.report.messageSubmissions >= limit) block("创建目标缺少显式剩余模型调用预算");
    await idle(ctx);
    const spec = message(ctx, "real-goal", ctx.options.messages.goal);
    await fillOwnedDraft(ctx, "main", spec.text);
    await click(ctx, ctx.tab.playwright.locator("#composer-add"));
    const goal = ctx.tab.playwright.getByRole("menuitem", { name: "目标", exact: true });
    if (await goal.count() !== 1 || !await goal.isEnabled()) block("当前真实会话不支持创建目标");
    await click(ctx, goal);
    const prepared = await state(ctx);
    invariant(prepared.goalComposerTag && prepared.prompt.includes(spec.marker)
      && prepared.send.label === "创建目标", "目标标签或本轮正文未保留，不能按普通发送冒充目标创建");
    await action(ctx, "create-real-goal", { kind: "create-goal", threadId: owned(ctx, "main").id, marker: spec.marker }, async () => {
      ctx.report.messageSubmissions++; await checkpoint(ctx);
      await click(ctx, ctx.tab.playwright.getByRole("button", { name: "创建目标", exact: true }));
      await wait(ctx, "目标实际创建并显示本轮内容", value => value.goalText.includes(spec.marker));
      return { goalMarker: spec.marker };
    });
    await driverStep(ctx, "native", "observe-goal", {
      facts: [fact("goalMarker", spec.marker), fact("goalStatus", "complete"), fact("goalExecuted", true)],
      payload: { waitForCompletion: true } });
    await wait(ctx, "真实目标执行产生校验回复", value => value.turns.some(turn => turn.agents.some(agent => agent.text.includes(spec.expected))),
      ctx.options.turnTimeoutMs ?? 180_000);
    await finalDriverContract(ctx, "native", "observe-scenario", { marker: spec.marker });
    await verifyNative(ctx, { threadIds: [owned(ctx, "main").id], ...REMOTE_ACCESS_E2E_CONTRACTS["scenario.goal"] });
  },
});

async function questionScenario(ctx, completedOrigin) {
  driver(ctx, "native", ["observe-request", "observe-scenario"]);
  const question = completedOrigin ? ctx.options.completedQuestionUI : ctx.options.questionUI;
  const prompt = completedOrigin ? ctx.options.messages?.completedQuestion : ctx.options.messages?.question;
  if (!prompt || !question?.fields?.length || !question.submitName)
    block(completedOrigin
      ? "缺少 messages.completedQuestion、completedQuestionUI.fields(label/value) 和提交按钮；该消息必须提出异步题后结束原轮"
      : "缺少获准的真实问答消息、questionUI.fields(label/value) 和提交按钮名称");
  permit(ctx, "allowAnswers");
  await idle(ctx);
  const spec = await send(ctx, message(ctx, completedOrigin ? "completed-question-scenario" : "question-scenario", prompt));
  if (completedOrigin) {
    // 先以原生完成状态建立前提，不能抢在模型结束前回答而错过卡片消失的回归。
    await driverStep(ctx, "native", "observe-request", {
      payload: { threadId: spec.threadId, marker: spec.marker, waitForTurnCompletion: true },
      facts: [fact("requestKind", "asyncQuestion"), fact("originatingTurnStatus", "completed"),
        fact("lastSubmission", null), fact("isSkipped", false), fact("pending", true)],
    });
    await wait(ctx, "原轮完成后未答异步卡片仍在", value => !!value.approvalText
      && !value.sidebar.some(row => row.id === spec.threadId && row.active));
    await capture(ctx, "completed-turn-question-still-pending");
    await reloadPage(ctx);
    await wait(ctx, "刷新仍显示同一会话的未答异步卡片", value => value.threadId === spec.threadId && !!value.approvalText);
    await driverStep(ctx, "native", "observe-request", {
      payload: { threadId: spec.threadId, marker: spec.marker, sameQuestionAsPrevious: true },
      facts: [fact("requestKind", "asyncQuestion"), fact("originatingTurnStatus", "completed"),
        fact("lastSubmission", null), fact("isSkipped", false), fact("pending", true)],
    });
  } else {
    await wait(ctx, "真实问答卡片出现", value => !!value.approvalText);
    await driverStep(ctx, "native", "observe-request", { facts: [fact("requestKind", "question"), fact("pending", true)] });
  }
  for (const field of question.fields) {
    invariant(typeof field.label === "string" && typeof field.value === "string", "问答输入必须是获准的精确 label/value");
    const input = ctx.tab.playwright.locator("#approval-area").getByRole("textbox", { name: field.label, exact: true });
    invariant(await input.count() === 1, "实际问答输入与已批准问题不匹配");
    await input.fill(field.value);
  }
  const beforeAnswer = await state(ctx);
  const originatingTurnId = userTurn(beforeAnswer, spec)?.id;
  invariant(originatingTurnId && !originatingTurnId.startsWith("pending-"), "无法确认原问题所在的真实轮次");
  const answerScope = { originatingTurnId, beforeTurnIds: beforeAnswer.turns.map(turn => turn.id), completedOrigin };
  await action(ctx, "answer-real-question", { kind: "answer", threadId: spec.threadId }, async () => {
    await click(ctx, ctx.tab.playwright.locator("#approval-area").getByRole("button", { name: question.submitName, exact: true }));
    await wait(ctx, "问答提交后原卡片退出", value => !value.approvalText);
    return { answers: question.fields };
  });
  invariant(!(await state(ctx)).notice.includes("回答已提交"), "问答成功后仍显示用户明确要求移除的“回答已提交”提示");
  await capture(ctx, "question-accepted-without-success-toast");
  const answered = await wait(ctx, "准确问答的后续回复出现并结束", value => !!findQuestionAnswerTurn(value, spec, answerScope),
    ctx.options.turnTimeoutMs ?? 180_000);
  const answerTurnId = findQuestionAnswerTurn(answered, spec, answerScope).id;
  if (completedOrigin) {
    await reloadPage(ctx);
    invariant(!(await state(ctx)).approvalText, "已回答异步题在刷新后重新出现");
    await capture(ctx, "completed-question-answer-stays-resolved");
  }
  await finalDriverContract(ctx, "native", "observe-scenario", { marker: spec.marker, originatingTurnId, answerTurnId });
  const contract = REMOTE_ACCESS_E2E_CONTRACTS[ctx.current.id];
  await verifyNative(ctx, { threadIds: [spec.threadId], ...contract,
    expectedFacts: { ...contract.expectedFacts, originatingTurnId, answerTurnId, answers: question.fields } });
}

async function questionReceiptRecovery(ctx) {
  permit(ctx, "allowAnswers"); permit(ctx, "allowDrafts");
  driver(ctx, "native", ["observe-question-recovery"]);
  driver(ctx, "environment", [
    "start-question-recovery-observation", "drop-question-answer-receipt",
    "release-question-receipt-on-reload", "observe-question-recovery",
  ]);
  const question = ctx.options.completedQuestionUI, prompt = ctx.options.messages?.completedQuestion;
  if (!prompt || question?.fields?.length < 2 || !question?.submitName)
    block("问答回执恢复需要获准的 completedQuestion、至少两项精确 question/label/value 和提交按钮");
  invariant(question.fields.every(field => typeof field.question === "string" && field.question.trim()
    && typeof field.label === "string" && field.label.trim() && typeof field.value === "string" && field.value.trim())
    && new Set(question.fields.map(field => field.question)).size === question.fields.length,
  "问答恢复的每项获准回答需独立的原题文本、真实输入标签及答案");
  const remaining = ctx.options.authorization?.maxMessageSubmissions - ctx.report.messageSubmissions;
  if (!Number.isFinite(remaining) || remaining < 2)
    block("问答回执恢复需预留两次消息额度：一次提问和一次真实回答；不会自动追加额度");
  const main = owned(ctx, "main"), control = owned(ctx, "control");
  const stage = { kind: "question-receipt-recovery", threadId: main.id, controlThreadId: control.id,
    mainDraft: marker(ctx, "question-receipt-main-draft"), controlDraft: marker(ctx, "question-receipt-control-draft") };
  const observe = async (phase, payload = {}) => {
    const name = ["unknown", "delivery"].includes(phase) ? "environment" : "native";
    const result = await driverStep(ctx, name, "observe-question-recovery", {
      facts: [fact("phase", phase)],
      payload: { phase, controlThreadId: control.id, ...payload },
    });
    const records = result.records.filter(record => record.facts.phase === phase && record.threadId === main.id);
    invariant(records.length === 1, "问答恢复阶段必须提供唯一完整的原始观察");
    stage[phase] = records[0];
    return records[0].facts;
  };
  await idle(ctx);
  invariant(!(await state(ctx)).prompt, "主会话有现存草稿，不能覆盖后发起问答恢复测试");
  await driverStep(ctx, "environment", "start-question-recovery-observation", { mutation: true,
    facts: [fact("observationStarted", true)], payload: { onlyCurrentTab: true, readOnly: true } });
  await observe("before");
  const spec = await send(ctx, message(ctx, "question-receipt", prompt));
  stage.askActionId = spec.actionId;
  await observe("pendingInitial", { marker: spec.marker, waitForTurnCompletion: true });
  stage.questionReloadStartedAt = iso();
  await reloadPage(ctx);
  const pending = await observe("pending", { marker: spec.marker, waitForTurnCompletion: true });
  invariant(pending.requestId && pending.originatingTurnId && Array.isArray(pending.nativeQuestionStates),
    "缺少准确原请求和逐题身份，不提交回答");
  const answers = pending.nativeQuestionStates.map(value => {
    const original = value.state?.question;
    const fields = question.fields.filter(field => field.question === original?.title);
    invariant(fields.length === 1 && original?.id && value.key?.threadId === main.id && value.key.entityKey
      && value.key.itemId === original.id && value.sourceTurnId === pending.originatingTurnId
      && value.sourceTurnStatus === "completed" && value.state.turnId === pending.originatingTurnId
      && value.state.lastSubmission === null && value.state.isSkipped === false && value.state.isSubmitting === false
      && pending.requestId === JSON.stringify(["async-question", pending.originatingTurnId, original.sourceItemId]),
    "真实问题并非准确的 completed 未答题，或与批准回答无法逐题匹配");
    return { questionItemId: original.id, question: original.title, answer: fields[0].value };
  });
  invariant(answers.length === question.fields.length && new Set(answers.map(value => value.questionItemId)).size === answers.length,
    "真实多题缺失、重复或多于批准范围");
  invariant(isDeepStrictEqual(stage.pendingInitial.facts.nativeQuestionStates, pending.nativeQuestionStates)
    && stage.pendingInitial.facts.requestId === pending.requestId, "刷新后原题身份改变，不提交回答");
  await fillOwnedDraft(ctx, "control", stage.controlDraft);
  stage.controlBeforeUi = await captureObservation(ctx, "question-receipt-control-before");
  await fillOwnedDraft(ctx, "main", stage.mainDraft);
  for (const field of question.fields) {
    const input = ctx.tab.playwright.locator("#approval-area").getByRole("textbox", { name: field.label, exact: true });
    await wait(ctx, "切回主会话后原待答题的输入框已就绪", async value =>
      value.threadId === main.id && !value.loading && value.approvalText.includes(field.question)
      && await input.count() === 1 && await input.isVisible() && await input.isEnabled());
    await input.fill(field.value);
  }
  stage.beforeAnswerUi = await captureObservation(ctx, "question-receipt-before-answer");
  await driverStep(ctx, "environment", "drop-question-answer-receipt", { mutation: true,
    facts: [fact("receiptFaultArmed", true)],
    payload: { threadId: main.id, requestId: pending.requestId, onlyAction: "answer",
      preserveServerExecution: true, holdOnlyOriginalReceiptReadsUntilNewDocument: true,
      receiptReadFault: "pause" } });
  const answerLabel = "answer-question-with-lost-receipt";
  stage.answerActionId = `${ctx.report.runId}:${ctx.current.id}:${answerLabel}`;
  const awaitingOriginal = new Error("原回答结果未知，仅允许核对原回执");
  try {
    await action(ctx, answerLabel, { kind: "answer", threadId: main.id,
      questionRequestId: pending.requestId, originatingTurnId: pending.originatingTurnId, answers }, async () => {
      ctx.report.messageSubmissions++;
      await checkpoint(ctx);
      await click(ctx, ctx.tab.playwright.locator("#approval-area")
        .getByRole("button", { name: question.submitName, exact: true }));
      const unknown = await observe("unknown", { questionRequestId: pending.requestId });
      invariant(unknown.clientReceipts?.length === 1 && unknown.clientReceipts[0].state === "unknown"
        && unknown.clientReceipts[0].requestId === pending.requestId, "未捕获真实未知回答状态；不得补造或重复回答");
      // 已点击必须保留 unknown；原生已完成也不能代替浏览器取得回执。
      throw awaitingOriginal;
    });
  } catch (error) {
    if (error !== awaitingOriginal) throw error;
    await note(ctx, "question-answer-awaiting-original-receipt", { actionId: stage.answerActionId });
  }
  const answerScope = { originatingTurnId: pending.originatingTurnId,
    beforeTurnIds: stage.beforeAnswerUi.observation.turns.map(turn => turn.id), completedOrigin: true };
  await openSidebar(ctx);
  await wait(ctx, "实际回答完成且连接恢复，才能区分未知回执锁与运行中禁用", async value => {
    if (!findQuestionAnswerTurn(value, spec, answerScope)) return false;
    const trigger = ctx.tab.playwright.locator(`.thread-row[data-thread-id="${css(main.id)}"] .row-menu-button`)
      .filter({ visible: true }).first();
    return await trigger.count() === 1 && await trigger.isEnabled();
  }, ctx.options.turnTimeoutMs ?? 180_000);
  await observe("answered", { questionRequestId: pending.requestId });
  await rowMenu(ctx, "main");
  await wait(ctx, "未知回答的删除入口保持锁定", value => value.menuItems.some(item => item.name === "删除会话" && !item.enabled));
  stage.unknownUi = await captureObservation(ctx, "question-receipt-unknown-delete-locked");
  await closeRowMenu(ctx, "main");
  await driverStep(ctx, "environment", "release-question-receipt-on-reload", { mutation: true,
    facts: [fact("releaseOnNewDocument", true)],
    payload: { originalReceiptId: stage.unknown.facts.clientReceipts[0].id,
      questionRequestId: pending.requestId, onlyCurrentTab: true } });
  stage.reloadStartedAt = iso();
  await reloadPage(ctx);
  const completed = await wait(ctx, "刷新后原回答完成且普通草稿保留", value => value.prompt === stage.mainDraft
    && !value.approvalText && !!findQuestionAnswerTurn(value, spec, answerScope), ctx.options.turnTimeoutMs ?? 180_000);
  stage.reloadedUi = await captureObservation(ctx, "question-receipt-reloaded");
  await rowMenu(ctx, "main");
  await wait(ctx, "查询原回执后删除入口解除", value => value.menuItems.some(item => item.name === "删除会话" && item.enabled));
  stage.unlockedUi = await captureObservation(ctx, "question-receipt-delete-unlocked");
  await closeRowMenu(ctx, "main");
  await selectOwned(ctx, "control");
  stage.controlAfterUi = await captureObservation(ctx, "question-receipt-control-after");
  await selectOwned(ctx, "main");
  await observe("history", { questionRequestId: pending.requestId });
  await observe("delivery", { originalReceiptId: stage.unknown.facts.clientReceipts[0].id });
  const original = ctx.report.actions[stage.answerActionId];
  const candidate = { ...original, status: "confirmed", result: {
    threadId: main.id, receiptId: stage.unknown.facts.clientReceipts[0].id,
    answerTurnId: findQuestionAnswerTurn(completed, spec, answerScope).id, reconciledFromUnknown: true,
  } };
  await verifyQuestionReceiptRecoveryStage(stage, {
    runId: ctx.report.runId, caseId: ctx.current.id, origin: ctx.origin,
    sourceFingerprint: ctx.report.sourceFingerprint, runStartedAt: ctx.report.runStartedAt,
    threadId: main.id, controlThreadId: control.id, ask: ctx.report.actions[spec.actionId], answer: candidate,
  }, { baseDir: ctx.outputDir });
  Object.assign(original, candidate, { finishedAt: iso() });
  delete original.error;
  await note(ctx, "question-receipt-recovery", stage);
  await verifyNative(ctx, { threadIds: [main.id],
    ...REMOTE_ACCESS_E2E_CONTRACTS["recovery.question_receipt"] });
}

async function scenario(ctx) {
  block(`案例 ${ctx.current.id} 尚无执行器；禁止使用返回 passed 的自定义回调补齐`);
}

async function androidInstalled(ctx) {
  driver(ctx, "android", ["observe-installed"]);
  const target = ctx.options.androidExpected;
  if (!target?.versionName || !Number.isInteger(target.versionCode) || target.versionCode < 1
    || !/^[a-f0-9]{64}$/i.test(target.apkSha256 || "")
    || !/^[a-f0-9]{64}$/i.test(target.certificateSha256 || "") || !target.apk || !target.manifest)
    block("缺少 Android 实际发布基准：androidExpected 需 versionName/versionCode/apkSha256/certificateSha256/apk/manifest");
  await verifyEvidenceReference(target.apk, { baseDir: ctx.outputDir });
  const manifestRef = await verifyEvidenceReference(target.manifest, { baseDir: ctx.outputDir });
  const manifest = JSON.parse(manifestRef.bytes.toString("utf8").replace(/^\uFEFF/, ""));
  const version = manifest.versionName ?? manifest.version;
  invariant(version === target.versionName && manifest.versionCode === target.versionCode
    && (manifest.sha256 ?? manifest.apkSha256) === target.apkSha256
    && target.apk.sha256 === target.apkSha256, "Android 发布安装包、更新清单与指定版本基准不一致");
  const result = await driverStep(ctx, "android", "observe-installed", { facts: [
    fact("packageName", "com.junxin367.codexelves"), fact("versionName", target.versionName),
    fact("versionCode", target.versionCode), fact("installedApkSha256", target.apkSha256),
    fact("certificateSha256", target.certificateSha256),
    fact("signatureMatchesRelease", true), fact("installedAssetsMatch", true),
  ], payload: { releaseApk: target.apk, releaseManifest: target.manifest } });
  await note(ctx, "android-installation", { target, records: result.records });
  return target;
}

Object.assign(handlers, {
  async "side.late_creation"(ctx) {
    driver(ctx, "environment", ["delay-side-create-delivery", "restore-side-create-delivery"]);
    driver(ctx, "native", ["observe-late-side-create"]);
    permit(ctx, "allowSideSessions"); permit(ctx, "allowFaults");
    await idle(ctx);
    const parent = owned(ctx, "main"), control = owned(ctx, "control");
    await driverStep(ctx, "environment", "delay-side-create-delivery", { mutation: true,
      payload: { parentThreadId: parent.id, onlyNextCreation: true }, facts: [fact("sideCreateDeliveryDelayed", true)] });
    await ensureMenu(ctx);
    let result;
    try {
      result = await action(ctx, "create-side-late", { kind: "side-create", threadId: parent.id }, async () => {
        await click(ctx, ctx.tab.playwright.getByRole("menuitem", { name: "新建侧边会话", exact: true }));
        await selectOwned(ctx, "control");
        await driverStep(ctx, "environment", "restore-side-create-delivery", { key: "control", mutation: true,
          facts: [fact("sideCreateDeliveryDelayed", false)] });
        const proof = await driverStep(ctx, "native", "observe-late-side-create", { key: "control",
          payload: { parentThreadId: parent.id },
          facts: [fact("createdSideCount", 1), fact("selectedControlPreserved", true), fact("duplicateCount", 0)] });
        const id = await rawDriverFact(proof, "createdThreadId");
        invariant(typeof id === "string" && id && !ctx.report.initialThreadIds.includes(id)
          && !Object.values(ctx.report.threads).some(value => value.id === id), "迟到创建没有产生准确全新侧边 ID");
        invariant((await state(ctx)).threadId === control.id, "迟到创建抢占了后来选中的控制会话");
        return { threadId: id, parentThreadId: parent.id };
      });
    } finally {
      await restoreLeasedResources(ctx, ["side-create-delivery"]);
    }
    ctx.report.threads.lateSide = { id: result.threadId, key: "lateSide", marker: marker(ctx, "lateSide"),
      parentThreadId: parent.id, runId: ctx.report.runId, createdBy: "remote-access-e2e", createdAt: iso(), kind: "side",
      creationActionId: `${ctx.report.runId}:${ctx.current.id}:create-side-late` };
    await checkpoint(ctx);
    await selectOwned(ctx, "main"); await ensureMenu(ctx);
    await wait(ctx, "迟到侧边归入原父会话且只出现一次", value => value.sideEntries.filter(entry => entry.id === result.threadId).length === 1);
    await click(ctx, ctx.tab.playwright.locator(`[data-menu-key="side:${css(result.threadId)}"]`));
    await wait(ctx, "打开同一迟到侧边空会话", value => value.threadId === result.threadId && value.related && value.turns.length === 0);
    await markSide(ctx, "lateSide");
    await backToMain(ctx); await selectOwned(ctx, "control");
    await finalDriverContract(ctx, "native", "observe-late-side-create", { parentThreadId: parent.id, createdThreadId: result.threadId }, "control");
    await verifyNative(ctx, { threadIds: [parent.id, control.id, result.threadId], ...REMOTE_ACCESS_E2E_CONTRACTS["side.late_creation"] });
    await selectOwned(ctx, "main");
  },
  async "android.installed_version"(ctx) {
    await selectOwned(ctx, "main");
    await androidInstalled(ctx);
    await finalDriverContract(ctx, "android", "observe-installed");
  },
  async "android.background_resume"(ctx) {
    driver(ctx, "android", ["observe-installed", "open-thread", "fill-draft", "background-app", "resume-app", "observe-resume"]);
    permit(ctx, "allowAndroid");
    await androidInstalled(ctx); await selectOwned(ctx, "main");
    const draft = `${marker(ctx, "android-background-draft")}\n后台前第二行`;
    await driverStep(ctx, "android", "open-thread", { mutation: true, payload: { targetThreadId: owned(ctx, "main").id },
      facts: [fact("visibleThreadId", owned(ctx, "main").id)] });
    await driverStep(ctx, "android", "fill-draft", { mutation: true, payload: { draft },
      facts: [fact("visiblePrompt", draft)] });
    await driverStep(ctx, "android", "background-app", { mutation: true,
      facts: [fact("appForeground", false)] });
    await driverStep(ctx, "android", "resume-app", { mutation: true,
      facts: [fact("appForeground", true), fact("visibleThreadId", owned(ctx, "main").id), fact("visiblePrompt", draft)] });
    await finalDriverContract(ctx, "android", "observe-resume", { draft, targetThreadId: owned(ctx, "main").id });
    await driverStep(ctx, "android", "fill-draft", { mutation: true, payload: { sequence: 2, draft: "" }, facts: [fact("visiblePrompt", "")] });
  },
  async "android.back_navigation"(ctx) {
    driver(ctx, "android", ["observe-installed", "open-thread", "open-model-menu", "press-back", "observe-navigation"]);
    permit(ctx, "allowAndroid");
    await androidInstalled(ctx); await selectOwned(ctx, "main");
    const side = owned(ctx, "side2"), parent = owned(ctx, "main");
    await driverStep(ctx, "android", "open-thread", { mutation: true, payload: { targetThreadId: parent.id },
      facts: [fact("visibleThreadId", parent.id)] });
    await driverStep(ctx, "android", "open-model-menu", { mutation: true,
      payload: { targetThreadId: parent.id }, facts: [fact("modelMenuVisible", true), fact("visibleThreadId", parent.id)] });
    await driverStep(ctx, "android", "press-back", { mutation: true, payload: { sequence: 1 },
      facts: [fact("modelMenuVisible", false), fact("visibleThreadId", parent.id), fact("appForeground", true)] });
    await driverStep(ctx, "android", "open-thread", { mutation: true, payload: { sequence: 2, targetThreadId: side.id, parentThreadId: parent.id },
      facts: [fact("visibleThreadId", side.id)] });
    await driverStep(ctx, "android", "press-back", { mutation: true, payload: { sequence: 2 },
      facts: [fact("visibleThreadId", parent.id), fact("appForeground", true)] });
    await finalDriverContract(ctx, "android", "observe-navigation", { sideThreadId: side.id, parentThreadId: parent.id });
  },
  async "android.notification"(ctx) {
    driver(ctx, "android", ["observe-installed", "open-thread", "background-app", "observe-notification", "tap-notification", "observe-notification-navigation"]);
    permit(ctx, "allowAndroid"); readStateReady(ctx);
    await androidInstalled(ctx); await idle(ctx);
    const main = owned(ctx, "main"), control = owned(ctx, "control");
    await driverStep(ctx, "android", "open-thread", { mutation: true, payload: { targetThreadId: control.id },
      facts: [fact("visibleThreadId", control.id), fact("notificationsEnabled", true)] });
    await driverStep(ctx, "android", "background-app", { mutation: true, payload: { targetThreadId: control.id },
      facts: [fact("appForeground", false)] });
    const spec = await startUnreadTurn(ctx, "main", "android-notification");
    await selectOwned(ctx, "control");
    await wait(ctx, "通知目标真实完成并未读", value => rowReadState(value, main.id, true), ctx.options.turnTimeoutMs ?? 180_000);
    await driverStep(ctx, "android", "observe-notification", { key: "control", payload: { targetThreadId: main.id, marker: spec.marker },
      facts: [fact("notificationThreadId", main.id), fact("notificationVisible", true)] });
    await driverStep(ctx, "android", "tap-notification", { key: "control", mutation: true,
      payload: { targetThreadId: main.id, marker: spec.marker },
      facts: [fact("visibleThreadId", main.id), fact("appForeground", true), fact("visibleReply", spec.expected, "includes")] });
    await finalDriverContract(ctx, "android", "observe-notification-navigation", { targetThreadId: main.id, marker: spec.marker }, "control");
    await wait(ctx, "Android 打开目标后桌面远程侧栏同步已读", value => rowReadState(value, main.id, false));
    await nativeReadState(ctx, { phase: "android-notification-opened", threadId: main.id, turnId: spec.turnId, hasUnreadTurn: false });
    await selectOwned(ctx, "main");
    await verifyNative(ctx, { threadIds: [main.id, control.id], ...REMOTE_ACCESS_E2E_CONTRACTS["android.notification"] });
  },
  async "android.resource_update"(ctx) {
    driver(ctx, "android", ["observe-installed", "open-thread", "fill-draft", "check-resource-update", "apply-resource-update",
      "reload-webview", "attempt-unavailable-resource-update", "observe-resource-update"]);
    permit(ctx, "allowAndroid"); permit(ctx, "allowResourceUpdates");
    await androidInstalled(ctx); await selectOwned(ctx, "main");
    const update = ctx.options.androidResourceUpdate;
    if (!update?.manifest || !update.resourceArchive || !update.version || !update.previousVersion
      || !update.unavailableUpdateUrl || !/^[a-f0-9]{64}$/i.test(update.assetSha256 || ""))
      block("资源更新需明确真实测试更新服务：manifest/resourceArchive/version/previousVersion/assetSha256/unavailableUpdateUrl；不发布或伪造更新");
    await verifyEvidenceReference(update.manifest, { baseDir: ctx.outputDir });
    await verifyEvidenceReference(update.resourceArchive, { baseDir: ctx.outputDir });
    const draft = marker(ctx, "android-resource-update-draft");
    await driverStep(ctx, "android", "open-thread", { mutation: true, payload: { targetThreadId: owned(ctx, "main").id },
      facts: [fact("visibleThreadId", owned(ctx, "main").id)] });
    await driverStep(ctx, "android", "fill-draft", { mutation: true, payload: { draft }, facts: [fact("visiblePrompt", draft)] });
    await driverStep(ctx, "android", "check-resource-update", { mutation: true, payload: { update },
      facts: [fact("availableResourceVersion", update.version), fact("currentResourceVersion", update.previousVersion)] });
    await driverStep(ctx, "android", "apply-resource-update", { mutation: true, payload: { update },
      facts: [fact("currentResourceVersion", update.version), fact("installedAssetSha256", update.assetSha256)] });
    await driverStep(ctx, "android", "reload-webview", { mutation: true,
      facts: [fact("visibleThreadId", owned(ctx, "main").id), fact("visiblePrompt", draft)] });
    await driverStep(ctx, "android", "attempt-unavailable-resource-update", { mutation: true,
      payload: { updateUrl: update.unavailableUpdateUrl, expectedUnchangedVersion: update.version },
      facts: [fact("resourceUpdateFailed", true), fact("currentResourceVersion", update.version),
        fact("installedAssetSha256", update.assetSha256), fact("visiblePrompt", draft)] });
    await finalDriverContract(ctx, "android", "observe-resource-update", { draft, targetThreadId: owned(ctx, "main").id });
    await driverStep(ctx, "android", "fill-draft", { mutation: true, payload: { sequence: 2, draft: "" }, facts: [fact("visiblePrompt", "")] });
  },
});

// 每个依赖环境的案例必须留下这些实际步骤；离线验收不能只保留最后一条成功观察。
const requiredSteps = value => Object.freeze(value.split(" ").map(entry => {
  const [name, count = "1"] = entry.split("*"), [driver, operation] = name.split(":");
  return Object.freeze({ driver, operation, count: Number(count) });
}));
export const REMOTE_ACCESS_E2E_REQUIRED_DRIVER_STEPS = Object.freeze(Object.fromEntries(Object.entries({
  "queue.edit_draft": "native:observe-queue-edit-cancel*2",
  "read.side_aggregate": "native:observe-side-aggregate*4",
  "read.cross_client": "desktop:open-thread*4",
  "project.pin_unpin": "desktop:create-project desktop:observe-project*3",
  "project.search_preferences": "desktop:observe-project",
  "composer.image": "files:choose-image",
  "optimizer.cancel": "native:wait-optimization-terminal",
  "optimizer.late_result": "native:wait-optimization-terminal",
  "optimizer.no_injection": "environment:disconnect-service environment:observe-optimization*2 environment:observe-authorization environment:restore-service",
  "context.switch_update": "native:observe-context*4",
  "context.zero_missing": "native:observe-context*2",
  "recovery.reconnect": "environment:disconnect-network environment:restore-network environment:observe-recovery",
  "recovery.native_restart": "environment:restart-native environment:observe-recovery",
  "recovery.unknown_receipt": "environment:drop-next-action-receipt environment:restore-receipts environment:observe-original-receipt",
  "recovery.question_receipt": "native:observe-question-recovery*5 environment:observe-question-recovery*2 environment:start-question-recovery-observation environment:drop-question-answer-receipt environment:release-question-receipt-on-reload",
  "recovery.draft": "files:choose-image native:observe-draft",
  "side.late_creation": "environment:delay-side-create-delivery environment:restore-side-create-delivery native:observe-late-side-create*2",
  "queue.after_stop": "native:observe-queue-boundary*3",
  "queue.after_failure": "environment:fail-active-upstream environment:restore-upstream native:observe-queue-boundary*3",
  "queue.approval_wait": "native:observe-queue-boundary*2 native:observe-request",
  "queue.compaction": "native:observe-queue-boundary*2",
  "queue.dequeue_edit_race": "native:observe-dequeue-window native:observe-queue-race*2 environment:arm-queue-race-request environment:observe-queue-race-request environment:release-queue-race-request environment:acquire-environment-lease environment:restore-environment-resource environment:inspect-environment-lease",
  "queue.dequeue_remove_race": "native:observe-dequeue-window native:observe-queue-race*2 environment:arm-queue-race-request environment:observe-queue-race-request environment:release-queue-race-request environment:acquire-environment-lease environment:restore-environment-resource environment:inspect-environment-lease",
  "side.close_running": "native:observe-side-lifecycle",
  "side.delete_parent": "native:observe-side-lifecycle",
  "lifecycle.rename": "desktop:rename-thread desktop:observe-thread",
  "scenario.approval": "native:observe-request native:observe-scenario",
  "scenario.question": "native:observe-request native:observe-scenario",
  "scenario.question_completed": "native:observe-request*2 native:observe-scenario",
  "scenario.compaction": "native:observe-compaction native:observe-scenario",
  "scenario.goal": "native:observe-goal native:observe-scenario",
  "android.installed_version": "android:observe-installed*2",
  "android.background_resume": "android:observe-installed android:open-thread android:fill-draft*2 android:background-app android:resume-app android:observe-resume",
  "android.back_navigation": "android:observe-installed android:open-thread*2 android:open-model-menu android:press-back*2 android:observe-navigation",
  "android.notification": "android:observe-installed android:open-thread android:background-app android:observe-notification android:tap-notification android:observe-notification-navigation",
  "android.resource_update": "android:observe-installed android:open-thread android:fill-draft*2 android:check-resource-update android:apply-resource-update android:reload-webview android:attempt-unavailable-resource-update android:observe-resource-update",
}).map(([id, value]) => {
  const steps = requiredSteps(value).filter(value => !(value.driver === "environment" && restoreResources[value.operation]));
  const resources = faultResources[id];
  if (resources) steps.push(
    { driver: "environment", operation: "acquire-environment-lease", count: 1 },
    { driver: "environment", operation: "restore-environment-resource", count: resources.length },
    { driver: "environment", operation: "inspect-environment-lease", count: 1 },
  );
  return [id, Object.freeze(steps)];
})));

export const REMOTE_ACCESS_E2E_EXECUTABLE_CASE_IDS = Object.freeze(Object.keys(handlers));
invariant(REMOTE_ACCESS_E2E_CASES.every(value => typeof handlers[value.id] === "function"),
  "存在没有具体执行器的必测案例");

/**
 * 创建可供多次 CUA 调用共享的上下文。默认只允许读取，所有副作用需 authorization 显式开启。
 * resumeReport 可以是 report.json 的绝对路径或已解析对象，不接受任意已有 threadId 作为测试数据。
 */
export async function createRemoteAccessE2EContext(options) {
  invariant(options?.tab?.playwright && options.tab.url && options.tab.reload && options.tab.screenshot,
    "必须传入支持 playwright / url / reload / screenshot 的 Chrome 插件 tab");
  invariant(typeof options.runId === "string" && /^[a-zA-Z0-9][a-zA-Z0-9_-]{5,63}$/.test(options.runId),
    "runId 需为 6 至 64 位字母、数字、下划线或连字符，并对每轮使用唯一值");
  invariant(options.targetOrigin && new URL(options.targetOrigin).origin === options.targetOrigin,
    "targetOrigin 必须是明确授权的 origin，不能包含路径、查询或访问密钥");
  const outputDir = resolve(options.outputDir || join("temp", "remote-access-e2e", options.runId));
  let previous = options.resumeReport;
  if (typeof previous === "string") previous = JSON.parse(await readFile(previous, "utf8"));
  if (!previous) {
    try {
      await readFile(join(outputDir, "report.json"), "utf8");
      block("输出目录已有 report.json；请显式传入 resumeReport，或换唯一 runId，不能覆盖原操作记录");
    } catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  const environment = JSON.stringify(options.versionEvidence || null);
  const fingerprint = await sourceFingerprint();
  if (previous) {
    invariant(previous.schemaVersion === 2 && previous.scriptVersion === REMOTE_ACCESS_E2E_VERSION
      && previous.runId === options.runId && previous.origin === options.targetOrigin,
    "恢复报告的版本、runId 或 origin 不匹配");
    invariant(previous.environment === environment, "运行版本证据已变化，请使用新 runId，不能沿用旧通过结果");
    if (previous.sourceFingerprint) invariant(previous.sourceFingerprint === fingerprint,
      "源代码已变化，请使用新 runId，不能沿用旧通过结果");
    invariant(previous.cases.every(value => statuses.has(value.status)), "恢复报告存在未知案例状态");
  }
  const report = previous || {
    schemaVersion: 2, scriptVersion: REMOTE_ACCESS_E2E_VERSION, sourceFingerprint: fingerprint, runId: options.runId,
    startedAt: iso(), updatedAt: iso(), origin: options.targetOrigin, environment,
    versionEvidence: copy(options.versionEvidence || null), tabId: options.tab.id,
    runStartedAt: options.runStartedAt || iso(),
    overall: "not_run", initialThreadIds: [], evidenceSequence: 0, messageSubmissions: 0,
    threads: {}, queue: {}, windows: {}, actions: {},
    cases: REMOTE_ACCESS_E2E_CASES.map(value => ({ ...value, status: "not_run", elapsedMs: 0, attempts: [], evidence: [] })),
  };
  // 新增的必测案例在旧报告中显式补为未测，保留已有动作和证据，不能沿用旧 overall。
  report.cases = REMOTE_ACCESS_E2E_CASES.map(definition => ({
    ...(report.cases.find(value => value.id === definition.id)
      || { status: "not_run", elapsedMs: 0, attempts: [], evidence: [] }),
    ...definition,
  }));
  const ctx = { [contextTag]: true, options, tab: options.tab, origin: options.targetOrigin, outputDir, report, current: null };
  const observation = await state(ctx);
  if (!previous) report.initialThreadIds = [...new Set(observation.sidebar.map(value => value.id).concat(observation.threadId || []))];
  report.latestUrl = observation.url;
  await mkdir(outputDir, { recursive: true });
  // 上次进程可能在点击后中断；只恢复报告，不把未完成操作变成可重发操作。
  for (const value of report.cases) {
    if (value.inProgress) {
      value.status = "blocked"; value.reason = "上次执行中断；核对原动作和原生回执后再恢复";
      value.inProgress = false;
    }
  }
  await checkpoint(ctx);
  return ctx;
}

export async function runRemoteAccessCase(caseId, optionsOrContext) {
  const ctx = optionsOrContext?.[contextTag] ? optionsOrContext : await createRemoteAccessE2EContext(optionsOrContext);
  const entry = ctx.report.cases.find(value => value.id === caseId);
  invariant(entry, `未知案例 ${caseId}`);
  invariant(!ctx.current, "同一上下文不允许并发操作浏览器");
  if (entry.status === "passed") return ctx.report;
  if (entry.status !== "not_run" && !ctx.options.retryCases?.includes(caseId)) return ctx.report;
  const started = Date.now();
  const attempt = { startedAt: iso(), status: "not_run" };
  entry.attempts.push(attempt); entry.inProgress = true; ctx.current = entry;
  await checkpoint(ctx);
  try {
    await capture(ctx, "before");
    if (!entry.stepsCompleted && (entry.suite !== "read" || ["read.main_read_state", "read.side_read_state"].includes(caseId))
      && Object.values(ctx.report.actions).some(value => value.status === "unknown"))
      block("存在结果不明的原操作；先只读核对原生回执，不能执行后续变更");
    if (!entry.stepsCompleted) {
      // 只有整个处理器确实走完，才允许恢复时仅补读证据；中间阶段取证不能跳过剩余 UI。
      if (!entry.uiStepsCompleted) {
        const execute = () => (handlers[caseId] || scenario)(ctx);
        if (faultResources[caseId]) await withEnvironment(ctx, faultResources[caseId], execute);
        else await execute();
        entry.uiStepsCompleted = true;
        await checkpoint(ctx);
      }
      await flushNativeChecks(ctx);
      entry.stepsCompleted = true;
    }
    entry.status = "passed"; delete entry.reason;
  } catch (error) {
    entry.status = error instanceof Blocked ? "blocked" : "failed";
    entry.reason = error.message;
  } finally {
    entry.elapsedMs += Date.now() - started;
    entry.inProgress = false;
    Object.assign(attempt, { status: entry.status, finishedAt: iso(), elapsedMs: Date.now() - started, reason: entry.reason });
    const evidence = await capture(ctx, entry.status);
    // 缺少执行证据时不能保留通过状态。
    if (entry.status === "passed" && (!evidence.files.dom || !evidence.files.screenshot || !evidence.observation)) {
      entry.status = "blocked"; entry.reason = "UI 步骤完成，但 DOM 或截图执行证据保存失败";
      attempt.status = "blocked"; attempt.reason = entry.reason;
    }
    const fullElapsedMs = Date.now() - started;
    entry.elapsedMs += fullElapsedMs - attempt.elapsedMs;
    attempt.elapsedMs = fullElapsedMs;
    ctx.current = null;
    await checkpoint(ctx);
    if (ctx.options.onProgress) await ctx.options.onProgress({
      caseId, status: entry.status, elapsedMs: attempt.elapsedMs, reason: entry.reason,
    });
  }
  return ctx.report;
}

export async function runRemoteAccessE2E(optionsOrContext) {
  const ctx = optionsOrContext?.[contextTag] ? optionsOrContext : await createRemoteAccessE2EContext(optionsOrContext);
  const selectedSuites = ctx.options.suites || [...suites];
  invariant(Array.isArray(selectedSuites) && selectedSuites.every(value => suites.has(value)), "包含未知 suite");
  const selectedCases = ctx.options.caseIds;
  invariant(!selectedCases || selectedCases.every(value => REMOTE_ACCESS_E2E_CASES.some(item => item.id === value)), "包含未知 caseId");
  for (const entry of ctx.report.cases) {
    if (!selectedSuites.includes(entry.suite) || selectedCases && !selectedCases.includes(entry.id)) continue;
    await runRemoteAccessCase(entry.id, ctx);
    // 页面丢失、操作结果不明或删除确认界面仍打开时停止，余项保留 not_run。
    const unknown = Object.values(ctx.report.actions).some(value => value.caseId === entry.id && value.status === "unknown");
    let observation;
    try { observation = await state(ctx); } catch { break; }
    if (unknown || observation.confirmation || ctx.options.stopOnFailure !== false && entry.status === "failed") break;
  }
  ctx.report.finishedAt = iso();
  await checkpoint(ctx);
  return ctx.report;
}

// 供 CUA 按案例分次执行；均接受相同 options 或已有 context。
export const testMainSessionSelection = options => runRemoteAccessCase("read.select_refresh", options);
export const testMainSessionReadState = options => runRemoteAccessCase("read.main_read_state", options);
export const testSideSessionReadState = options => runRemoteAccessCase("read.side_read_state", options);
export const testModelSelection = options => runRemoteAccessCase("session.model", options);
export const testOrdinaryCreation = options => runRemoteAccessCase("session.create", options);
export const testSendAndRealtimeReply = options => runRemoteAccessCase("session.send_stream", options);
export const testAutomaticQueueHandoff = options => runRemoteAccessCase("queue.automatic", options);
export const testQueueEditDraft = options => runRemoteAccessCase("queue.edit_draft", options);
export const testQueueEditSave = options => runRemoteAccessCase("queue.edit_save", options);
export const testQueueReorder = options => runRemoteAccessCase("queue.reorder", options);
export const testQueueRemoval = options => runRemoteAccessCase("queue.remove", options);
export const testQueueSteering = options => runRemoteAccessCase("queue.steer", options);
export const testStopGeneration = options => runRemoteAccessCase("session.stop", options);
export const testSideSessionCreation = options => runRemoteAccessCase("lifecycle.side_create", options);
export const testSideSessionSwitching = options => runRemoteAccessCase("lifecycle.side_switch", options);
export const testSideSessionRefresh = options => runRemoteAccessCase("lifecycle.side_refresh", options);
export const testSideSessionClose = options => runRemoteAccessCase("lifecycle.side_close", options);
export const testPinSession = options => runRemoteAccessCase("lifecycle.pin", options);
export const testRenameSession = options => runRemoteAccessCase("lifecycle.rename", options);
export const testPermanentDeletePreview = options => runRemoteAccessCase("lifecycle.delete_preview", options);
export const testPermanentDeleteConfirmation = options => runRemoteAccessCase("lifecycle.delete_confirm", options);

if (typeof process !== "undefined" && process.argv[1]
    && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== "--verify-report" || !process.argv[3]) {
    console.error("真实浏览器案例请按 docs/remote-access-e2e.md 在 Chrome 插件运行。报告门禁：node scripts/remote-access-e2e.mjs --verify-report <report.json>");
    process.exitCode = 1;
  } else {
    try {
      const result = await verifyRemoteAccessE2EReport(process.argv[3]);
      console.log(JSON.stringify(result, null, 2));
      process.exitCode = result.passed ? 0 : 1;
    } catch (error) {
      console.error(error.message);
      process.exitCode = 1;
    }
  }
}
