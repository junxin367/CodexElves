// 审批与提问的共享状态：草稿、回执与阻塞请求快照，以及命令、MCP 审批的展示与渲染辅助；审批面板与会话读取共用。

import { button, element } from "../ui/dom.js";
import { icon } from "../ui/icons.js";
import { notice } from "../ui/notice.js";
import { reloadThread } from "../core/actions.js";
import { connected, session } from "../core/state.js";
import { pendingFeedbackReady } from "./composer/send-state.js";

export let approvalSignature = null;
export function setApprovalSignature(value) { return approvalSignature = value; }
export let approvalResizeObserver = null;
export function setApprovalResizeObserver(value) { return approvalResizeObserver = value; }
export const blockingRequestStates = new Map();
export const approvalReplies = new Map();
export const approvalDrafts = new Map();
export const questionDrafts = new Map();
export const questionReplyReceipts = new Map();
export const questionReceiptChecks = new Map();
export let questionVersion = 0, notifiedQuestions = null;
export function setQuestionVersion(value) { return questionVersion = value; }
export function setNotifiedQuestions(value) { return notifiedQuestions = value; }
// 从会话存储恢复异步问题回复的待核对回执。
export function restoreQuestionReceipts() {
  try {
    const stored = JSON.parse(sessionStorage.getItem("codex-elves-question-receipts") || "[]");
    if (Array.isArray(stored)) for (const record of stored) {
      if (typeof record?.deviceId !== "string" || typeof record.threadId !== "string"
          || typeof record.requestId !== "string" || typeof record.id !== "string" || !/^[a-zA-Z0-9-]{1,100}$/.test(record.id)
          || !["pending", "unknown"].includes(record.state)) continue;
      questionReplyReceipts.set(JSON.stringify([record.deviceId, record.threadId, record.requestId]), record);
    }
  } catch {}
}
export function blockingRequestState(id) {
  const key = JSON.stringify([session?.device.id, id]);
  let state = blockingRequestStates.get(key);
  if (!state) {
    state = { generation: 0, sourceVersion: 0, requests: null, epoch: null, revision: -1,
      retiredEpochs: new Set(), completed: new Set() };
    blockingRequestStates.set(key, state);
  }
  return state;
}
export function requestsSnapshotIsVersioned(snapshot) {
  return typeof snapshot?.requestsEpoch === "string" && snapshot.requestsEpoch.length > 0
    && Number.isSafeInteger(snapshot.requestsRevision) && snapshot.requestsRevision >= 0;
}
export function replaceBlockingRequests(target, requests) {
  target.requests = [...requests.filter(request => request.method !== "remote/requestUserInputAsync"),
    ...(target.requests || []).filter(request => request.method === "remote/requestUserInputAsync")];
}
export function approvalReplyKey(id, request, deviceId = session?.device.id) {
  return JSON.stringify([deviceId, id, blockingRequestState(id).sourceVersion, request.id, request.method]);
}
export function blockingApprovalIdentity(id, request) {
  let params = request.params;
  // Claude 问题身份已绑定原生内容与等待代次；补齐多选显示属性时保留已填写答案，原生问题变更仍会更换 ID。
  if (params?.nativeQuestion && Array.isArray(params.questions))
    params = { ...params, questions: params.questions.map(({ multiSelect, ...question }) => question) };
  return JSON.stringify([approvalReplyKey(id, request), params]);
}
export function blockingApprovalDraft(id, request) {
  const key = blockingApprovalIdentity(id, request);
  let record = approvalDrafts.get(key);
  if (!record) {
    record = { deviceId: session?.device.id, threadId: id, values: new Map() };
    approvalDrafts.set(key, record);
  }
  return record.values;
}
export function replaceAsyncQuestions(target, requests) {
  target.requests = [...(target.requests || []).filter(request => request.method !== "remote/requestUserInputAsync"),
    ...requests.filter(request => request.method === "remote/requestUserInputAsync")];
}
export function setQuestionReceipt(key, record) {
  const previous = questionReplyReceipts.get(key);
  if (record && !Number.isFinite(record.time)) record.time = Date.now();
  if (record) questionReplyReceipts.set(key, record);
  else questionReplyReceipts.delete(key);
  try { sessionStorage.setItem("codex-elves-question-receipts", JSON.stringify([...questionReplyReceipts.values()])); }
  catch {
    if (previous) questionReplyReceipts.set(key, previous);
    else questionReplyReceipts.delete(key);
    throw new Error("无法保存回答回执，请检查浏览器存储后重试");
  }
}
function commandApprovalPresentation(params = {}) {
  const network = params.networkApprovalContext;
  if (network) {
    const host = String(network.host || "");
    const authority = `${host}${network.port != null ? `:${network.port}` : ""}`;
    const destination = ["http", "https"].includes(network.protocol) ? `${network.protocol}://${authority}` : authority;
    return {
      label: "互联网访问", icon: "globe",
      title: `允许 ChatGPT 与 ${destination} 建立连接？`,
      description: `${host} 不在当前网络允许列表中`, command: ""
    };
  }
  const commands = (Array.isArray(params.commandActions) ? params.commandActions : [])
    .map(action => action?.command).filter(command => typeof command === "string" && command.trim());
  const command = commands.length ? commands.join(" ")
    : typeof params.command === "string" && params.command.trim() ? params.command
    : Array.isArray(params.proposedExecpolicyAmendment) ? params.proposedExecpolicyAmendment.join(" ") : "";
  return {
    label: "终端", icon: "terminal",
    title: typeof params.reason === "string" && params.reason.trim() || "允许 ChatGPT 运行此命令？",
    description: "", command
  };
}
export function renderCommandApproval(card, params) {
  const view = commandApprovalPresentation(params);
  card.classList.add("command-approval-card");
  const identity = element("div", "approval-identity");
  identity.append(icon(view.icon), element("span", "", view.label));
  card.append(identity, element("strong", "approval-title", view.title));
  if (view.description) card.append(element("p", "approval-reason muted", view.description));
  if (!view.command) return;
  const preview = element("div", "approval-command-preview collapsed");
  const code = element("pre", "approval-command-code", view.command);
  const toggle = button("展开", "展开完整命令", () => {
    const collapsed = preview.classList.toggle("collapsed");
    toggle.textContent = collapsed ? "展开" : "收起";
    toggle.setAttribute("aria-label", collapsed ? "展开完整命令" : "收起完整命令");
    toggle.setAttribute("aria-expanded", String(!collapsed));
    toggle.title = collapsed ? "展开完整命令" : "收起完整命令";
  }, "text-button approval-command-toggle");
  toggle.hidden = true; toggle.setAttribute("aria-expanded", "false");
  preview.append(code, toggle); card.append(preview);
}
export function observeApprovalCommands(root) {
  const previews = root.querySelectorAll(".approval-command-preview");
  if (!previews.length) return;
  approvalResizeObserver = new ResizeObserver(entries => {
    for (const { target: code } of entries) {
      const preview = code.parentElement;
      if (preview.classList.contains("collapsed"))
        preview.querySelector(".approval-command-toggle").hidden = code.scrollHeight <= code.clientHeight + 1;
    }
  });
  for (const preview of previews) approvalResizeObserver.observe(preview.querySelector(".approval-command-code"));
}
export function mcpToolApprovalPresentation(params = {}) {
  const record = value => value && typeof value === "object" && !Array.isArray(value);
  const metadata = params._meta;
  if (params.mode !== "form" || !record(metadata)) return null;
  const toolCall = metadata.codex_approval_kind === "mcp_tool_call";
  if (["browser_auth", "browser_email_otp", "tool_suggestion"].includes(metadata.codex_approval_kind)) return null;
  const persist = metadata.persist === undefined ? [] : Array.isArray(metadata.persist) ? metadata.persist : [metadata.persist];
  const validPersist = (metadata.persist === undefined || !Array.isArray(metadata.persist)
    || metadata.persist.length >= 1 && metadata.persist.length <= 2)
    && persist.every(value => ["session", "always"].includes(value));
  const validToolCall = toolCall && typeof metadata.connector_id === "string" && record(metadata.tool_params)
    && ["connector_name", "tool_name", "tool_title"].every(key => metadata[key] === undefined || typeof metadata[key] === "string")
    && (metadata.codex_request_type === undefined || metadata.codex_request_type === "approval_request") && validPersist;
  const browser = ["browser", "browser-use"].includes(params.serverName)
    && record(params.requestedSchema?.properties) && Object.keys(params.requestedSchema.properties).length === 0
    && metadata.persist !== undefined && validPersist && typeof metadata.origin === "string" && metadata.origin.trim();
  if (!toolCall && !browser) return null;
  const executionBound = params.serverName === "codex_apps"
    && metadata.connector_id === "connector_20205bf7d4e99a89d7154bb849718324";
  let parameterEntries = [];
  if (Object.prototype.hasOwnProperty.call(metadata, "tool_params_display")) {
    const display = metadata.tool_params_display;
    if (Array.isArray(display) && display.every(entry => record(entry)
      && typeof entry.name === "string" && entry.name.trim()
      && Object.prototype.hasOwnProperty.call(entry, "value")
      && (entry.display_name === undefined || typeof entry.display_name === "string" && entry.display_name.trim()))) {
      parameterEntries = display.map(entry => ({
        name: entry.name.trim(), value: entry.value, displayName: entry.display_name?.trim() || entry.name.trim()
      }));
    }
  } else if (record(metadata.tool_params)) {
    parameterEntries = Object.entries(metadata.tool_params).map(([name, value]) => ({ name, value, displayName: name }));
  }
  return {
    unsupported: toolCall && !validToolCall || executionBound,
    title: typeof params.message === "string" && params.message.trim() || "允许此工具执行所列操作？",
    parameterEntries,
    persist, elevatedRisk: metadata.riskLevel === "high",
    subtitle: typeof metadata.subtitle === "string" ? metadata.subtitle : ""
  };
}
export function renderMcpToolApproval(card, view) {
  card.classList.add("tool-approval-card");
  if (view.elevatedRisk) card.append(element("div", "approval-identity approval-risk-label", "较高风险"));
  card.append(element("strong", "approval-title", view.title));
  if (view.subtitle) card.append(element("p", "approval-reason muted", view.subtitle));
  if (view.parameterEntries.length) {
    const details = element("details", "approval-tool-details");
    details.append(element("summary", "", "查看工具参数"));
    for (const entry of view.parameterEntries) {
      const field = element("div", "approval-tool-parameter");
      field.append(element("strong", "", entry.displayName),
        element("pre", "approval-description", typeof entry.value === "string" ? entry.value : JSON.stringify(entry.value, null, 2)));
      details.append(field);
    }
    card.append(details);
  }
}
export function updateBlockingApprovalControls(card, key) {
  card.remoteReplyStatus?.remove(); card.remoteReplyStatus = null;
  const reply = approvalReplies.get(key), replyState = reply?.state;
  if (card.remoteLockedControls) {
    for (const [control, disabled] of card.remoteLockedControls) control.disabled = disabled;
    card.remoteLockedControls = null;
  }
  if (!replyState) return;
  card.remoteLockedControls = new Map([...card.querySelectorAll("button,input,select,textarea")]
    .map(control => [control, control.disabled]));
  for (const control of card.remoteLockedControls.keys()) control.disabled = true;
  if (!pendingFeedbackReady(reply)) return;
  const status = element("div", "remote-approval-reply-status");
  status.append(element("small", "muted", "正在确认操作结果。"));
  if (replyState !== "submitting") {
    const check = button("核对状态", "重新读取原生待处理状态",
      () => void reloadThread().catch(error => notice(error.message, true)), "text-button");
    check.disabled = !connected; status.append(check);
  }
  card.remoteReplyStatus = status; card.append(status);
}
