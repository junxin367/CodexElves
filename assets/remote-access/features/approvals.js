// 阻塞请求面板：渲染命令/MCP 审批与异步问题；请求的提交、合并与回执恢复在 approval-requests.js，共享状态在 approval-state.js。

import { confirmAction } from "../ui/dialog.js";
import { $, button, element } from "../ui/dom.js";
import { notice } from "../ui/notice.js";
import { connected, session, thread } from "../core/state.js";
import { submitAsyncQuestion, submitBlockingRequest } from "./approval-requests.js";
import {
  approvalDrafts, approvalReplies, approvalReplyKey, approvalResizeObserver, approvalSignature,
  blockingApprovalDraft, blockingApprovalIdentity, blockingRequestState,
  mcpToolApprovalPresentation, observeApprovalCommands, questionDrafts, questionReplyReceipts,
  renderCommandApproval, renderMcpToolApproval, setApprovalResizeObserver, setApprovalSignature,
  updateBlockingApprovalControls
} from "./approval-state.js";
import { claudePlanOptionLabel, claudeRewindDialogText } from "./claude-notices.js";
import { fitApprovalArea } from "./composer/composer-menus.js";
import { syncPendingFeedbackTimer } from "./composer/composer.js";
import { pendingFeedbackReady } from "./composer/send-state.js";
import { reconcileChildren } from "./messages/item-presentation.js";
import { setMarkdown } from "./messages/media.js";
import { approvalsAllowed, isClaude, threadReadOnly } from "./session/capabilities.js";

function renderAsyncQuestion(card, request, requestThreadId) {
  const key = JSON.stringify([session.device.id, requestThreadId, request.id]);
  const receipt = questionReplyReceipts.get(key);
  const submissionState = receipt?.state || request.params?.submissionState;
  const allowed = !threadReadOnly(requestThreadId) && approvalsAllowed() && connected;
  const draft = questionDrafts.get(key) || new Map();
  questionDrafts.set(key, draft);
  card.append(element("strong", "", "Codex 需要你回答"));
  const form = element("form", "async-question-form"), fields = [];
  for (const [index, question] of (request.params?.questions || []).entries()) {
    const field = element("fieldset", "async-question-field");
    field.append(element("legend", "", question.question || question.header));
    const options = (question.options || []).map(option => option.label);
    if (!draft.has(question.id)) draft.set(question.id, options[0] || "");
    const choices = element("div", "async-question-options");
    const input = element("textarea", "async-question-input");
    // 用户要求问答菜单紧凑：自由回答默认一行，与 CSS 的 34px 最小高度配合，仍可输入多行或手动拉高。
    input.rows = 1; input.placeholder = options.length ? "或输入其他回答…" : "输入你的回答…";
    input.setAttribute("aria-label", `${question.question || question.header}：其他回答`);
    input.value = options.includes(draft.get(question.id)) ? "" : draft.get(question.id);
    for (const [optionIndex, option] of (question.options || []).entries()) {
      const label = element("label", "async-question-option");
      const radio = element("input"); radio.type = "radio";
      radio.name = `question-${index}`; radio.value = option.label;
      radio.checked = !input.value && draft.get(question.id) === option.label;
      radio.disabled = !allowed || !!submissionState;
      const text = element("span", "async-question-option-copy");
      text.append(element("span", "", option.label));
      if (option.description) text.append(element("small", "muted", option.description));
      label.append(radio, element("span", "async-question-option-number", String(optionIndex + 1)), text);
      radio.onchange = () => { input.value = ""; draft.set(question.id, radio.value); update(); };
      choices.append(label);
    }
    input.disabled = !allowed || !!submissionState;
    input.oninput = () => {
      for (const radio of choices.querySelectorAll("input")) radio.checked = false;
      draft.set(question.id, input.value); update();
    };
    field.append(choices, input); form.append(field); fields.push(question);
  }
  const status = element("p", "async-question-status muted");
  status.setAttribute("role", "status");
  const footer = element("div", "async-question-footer");
  const actions = element("div", "approval-actions");
  actions.hidden = threadReadOnly(requestThreadId);
  const skip = button("跳过", "跳过此次问答", () => {
    if (!skip.disabled) void submitAsyncQuestion(requestThreadId, request, null, { skip: true });
  });
  const submit = button("提交回答", "提交问答选择", () => {}, "primary");
  submit.type = "submit"; actions.append(skip, submit);
  const update = () => {
    skip.disabled = !allowed || !!submissionState || !fields.length;
    submit.disabled = !allowed || !!submissionState || !fields.length
      || fields.some(question => !draft.get(question.id)?.trim());
  };
  if (submissionState) {
    if (pendingFeedbackReady(receipt))
      status.textContent = receipt?.skip ? "正在确认问题跳过结果。" : "正在确认回答结果。";
    if (receipt?.skip) skip.textContent = "跳过中…";
    else submit.textContent = "提交中…";
  } else if (!allowed) status.textContent = threadReadOnly(requestThreadId) || !approvalsAllowed()
    ? "此设备没有问答回复权限，请在 Codex 桌面处理。" : "连接恢复后可以提交回答。";
  form.onsubmit = event => {
    event.preventDefault();
    if (submit.disabled) return;
    void submitAsyncQuestion(requestThreadId, request, Object.fromEntries(fields.map(question =>
      [question.id, { answers: [draft.get(question.id).trim()] }])));
  };
  update(); footer.append(status, actions); form.append(footer); card.append(form);
}
export function renderApprovals() {
  syncPendingFeedbackTimer();
  const root = $("#approval-area"); if (!root) return;
  const requests = thread?.requests || [];
  const signature = JSON.stringify([session?.device.id, thread?.id, requests, threadReadOnly(), approvalsAllowed(), connected,
    thread?.id ? [blockingRequestState(thread.id).epoch, blockingRequestState(thread.id).sourceVersion] : null,
    [...approvalReplies].filter(([, reply]) => reply.deviceId === session?.device.id && reply.threadId === thread?.id)
      .map(([key, reply]) => [key, reply.state, pendingFeedbackReady(reply)]),
    [...questionReplyReceipts].filter(([, reply]) => reply.deviceId === session?.device.id && reply.threadId === thread?.id)
      .map(([key, reply]) => [key, reply.state, pendingFeedbackReady(reply)])]);
  if (signature === approvalSignature) return;
  setApprovalSignature(signature);
  approvalResizeObserver?.disconnect(); setApprovalResizeObserver(null);
  const previousCards = new Map([...root.children].filter(card => card.remoteRequestSignature)
    .map(card => [card.remoteRequestSignature, card]));
  const cards = [], identities = new Set(requests.map(request => blockingApprovalIdentity(thread.id, request)));
  for (const [key, record] of approvalDrafts) {
    if (record.deviceId === session?.device.id && record.threadId === thread?.id && !identities.has(key))
      approvalDrafts.delete(key);
  }
  root.classList.toggle("multiple-requests", requests.length > 1);
  if (requests.length > 1) {
    const summary = root.querySelector(".approval-queue-summary") || element("div", "approval-queue-summary muted");
    summary.textContent = `${requests.length} 项待处理，请逐项处理`;
    cards.push(summary);
  }
  for (const [index, request] of requests.entries()) {
    const requestThreadId = thread.id;
    const asyncQuestion = request.method === "remote/requestUserInputAsync";
    const questionKey = asyncQuestion ? JSON.stringify([session.device.id, requestThreadId, request.id]) : null;
    const requestSignature = JSON.stringify([blockingApprovalIdentity(requestThreadId, request),
      request.params?.nativeQuestion ? request.params.questions?.map(question => question.multiSelect === true) : null,
      threadReadOnly(), approvalsAllowed(), connected,
      asyncQuestion ? [questionReplyReceipts.get(questionKey)?.state,
        pendingFeedbackReady(questionReplyReceipts.get(questionKey))] : null]);
    const previous = previousCards.get(requestSignature);
    if (previous) {
      if (!asyncQuestion) updateBlockingApprovalControls(previous, approvalReplyKey(requestThreadId, request));
      previous.setAttribute("aria-label", `第 ${index + 1} 项待处理请求，共 ${requests.length} 项`);
      cards.push(previous); continue;
    }
    const questionCard = asyncQuestion || request.method === "item/tool/requestUserInput";
    const card = element("section", `approval-card${questionCard ? " question-card" : ""}`);
    card.remoteRequestSignature = requestSignature;
    card.setAttribute("aria-label", `第 ${index + 1} 项待处理请求，共 ${requests.length} 项`);
    if (asyncQuestion) { renderAsyncQuestion(card, request, requestThreadId); cards.push(card); continue; }
    const toolApproval = request.method === "mcpServer/elicitation/request" ? mcpToolApprovalPresentation(request.params) : null;
    const nativeDialog = request.params?.nativeDialog;
    if (nativeDialog) {
      const kind = nativeDialog.kind === "model" ? "模型" : "思考深度";
      const planDialog = nativeDialog.kind === "plan";
      const rewindDialog = nativeDialog.kind === "rewind";
      const title = nativeDialog.kind === "unknown" ? "Claude 正在等待本机确认"
        : nativeDialog.state === "submitted" ? (rewindDialog ? "正在关闭终端回退菜单" : "正在确认原生操作结果")
        : planDialog ? "Claude 已写好计划，等待你确认"
        : rewindDialog ? "原版终端打开了回退菜单（/rewind）" : `切换${kind}需要确认`;
      card.append(element("strong", "approval-title", title));
      if (nativeDialog.target) card.append(element("p", "approval-reason", `目标${kind}：${nativeDialog.target}`));
      if (rewindDialog) {
        for (const line of claudeRewindDialogText(nativeDialog)) card.append(element("p", "approval-reason", line));
      } else if (planDialog && nativeDialog.description) {
        // 计划正文取自原生计划文件，按 Markdown 展示，便于在网页上审阅后再批准。
        const plan = element("div", "approval-description markdown approval-plan");
        setMarkdown(plan, nativeDialog.description);
        card.append(plan);
      } else if (nativeDialog.description)
        card.append(element("pre", "approval-description", nativeDialog.description));
      if (!nativeDialog.remoteSupported || nativeDialog.kind === "unknown")
        card.append(element("small", "muted", "请在该会话的原版 Claude CLI 中处理，完成后这里会自动同步。"));
    } else if (toolApproval) renderMcpToolApproval(card, toolApproval);
    else if (request.method === "item/commandExecution/requestApproval") renderCommandApproval(card, request.params);
    else {
      card.append(element("strong", "", request.method === "item/tool/requestUserInput" ? `${isClaude() ? "Claude" : "Codex"} 需要你回答` : "等待你的授权"));
      const description = request.params?.command || request.params?.reason || request.params?.message || request.method;
      card.append(element("pre", "approval-description", typeof description === "string" ? description : JSON.stringify(description, null, 2)));
    }
    if (threadReadOnly() || !approvalsAllowed()) { card.append(element("small", "muted", isClaude() ? "此设备没有远程审批权限。" : "请在 Codex 桌面处理，此设备没有审批权限。")); cards.push(card); continue; }
    // Claude 实时问答复用既有问答卡；原生最终提交后仅核对结果，多端或重连不能再次发送同一组回答。
    if (request.params?.nativeQuestion?.state === "submitted") {
      card.append(element("small", "muted", "回答已提交，等待 Claude 确认结果。"));
      cards.push(card); continue;
    }
    const requestIdentity = blockingApprovalIdentity(requestThreadId, request);
    const reply = (action, value) => submitBlockingRequest(requestThreadId, request, action, value, requestIdentity);
    const actions = element("div", "approval-actions");
    if (nativeDialog) {
      // 仅使用原生确认框提供的两项决策；未知弹窗和已提交确认不提供猜测按键或重复提交。
      if (nativeDialog.remoteSupported && nativeDialog.state === "waiting"
          && ["model", "effort"].includes(nativeDialog.kind)) {
        const keep = nativeDialog.kind === "model" ? "保留原模型" : "保留原等级";
        actions.append(button("确认切换", "确认原生模型或等级切换，并继续已经收到的消息",
          () => reply("approve", { decision: "accept" }), "primary"),
        button(keep, keep, () => reply("approve", { decision: "decline" })));
      } else if (nativeDialog.remoteSupported && nativeDialog.state === "waiting" && nativeDialog.kind === "plan") {
        // 用户要求网页处理计划审批：每个批准项对应原生编号（会同时决定后续权限模式，按钮提示保留原生原文）；
        // 修改意见对应原生 “Tell Claude what to change”，提交后 Claude 继续规划。
        const options = Array.isArray(nativeDialog.options) ? nativeDialog.options : [];
        for (const option of options.filter(option => option.action === "approve"))
          actions.append(button(claudePlanOptionLabel(option.label), `原生选项：${option.label}`,
            () => reply("approve", { decision: "accept", option: option.index }), option.index === 1 ? "primary" : ""));
        if (options.some(option => option.action === "feedback")) {
          const draft = blockingApprovalDraft(requestThreadId, request);
          const feedback = element("textarea", "plan-feedback");
          feedback.placeholder = "告诉 Claude 需要修改什么";
          feedback.setAttribute("aria-label", "计划修改意见");
          feedback.value = draft.get("feedback") || "";
          feedback.oninput = () => draft.set("feedback", feedback.value);
          card.append(feedback);
          actions.append(button("提交修改意见", "把修改意见交给 Claude，继续规划", () => {
            const text = feedback.value.trim();
            if (!text) { notice("请先填写修改意见", true); return; }
            void reply("approve", { decision: "decline", feedback: text });
          }));
        }
      } else if (nativeDialog.remoteSupported && nativeDialog.state === "waiting" && nativeDialog.kind === "rewind") {
        // 用户要求网页可关闭原版终端的回退菜单：只取消、不回退，确认回退只能在终端操作（连接端 answer_rewind）。
        // 终端那边有人正在选择时会被打断，所以先二次确认。
        actions.append(button("关闭终端回退菜单", "等同在终端取消回退，不会回退任何内容", async () => {
          const deviceId = session?.device.id;
          if (!await confirmAction("关闭终端回退菜单",
            "将取消原版终端里打开的回退菜单，不会回退任何内容。如果终端那边有人正在操作，会被打断。", "关闭菜单")) return;
          if (!card.isConnected || session?.device.id !== deviceId || threadReadOnly() || !approvalsAllowed()) return;
          void reply("approve", { decision: "decline" });
        }));
      }
    } else if (["item/commandExecution/requestApproval", "item/fileChange/requestApproval"].includes(request.method)) {
      const allowed = request.params?.availableDecisions;
      for (const [label, decision] of [["允许一次", "accept"], ["本会话允许", "acceptForSession"], ["拒绝", "decline"], ["取消本轮", "cancel"]]) {
        if (Array.isArray(allowed) && !allowed.includes(decision)) continue;
        actions.append(button(label, label, async () => {
          const deviceId = session?.device.id;
          if (decision === "acceptForSession" && !await confirmAction("本会话允许", "允许此会话后续同类操作，不再逐次询问。请确认已了解该操作范围。", "允许本会话")) return;
          if (!card.isConnected || session?.device.id !== deviceId || threadReadOnly() || !approvalsAllowed()) return;
          void reply("approve", { decision });
        }, decision === "accept" ? "primary" : ""));
      }
      if (!actions.children.length) card.append(element("small", "muted", "此请求的审批方式请在 Codex 桌面处理。"));
    } else if (request.method === "item/tool/requestUserInput") {
      const fields = [], draft = blockingApprovalDraft(requestThreadId, request);
      for (const question of request.params?.questions || []) {
        const label = element("label", "question-label", question.question || question.header);
        const input = element("input"); input.setAttribute("aria-label", question.question || question.header);
        if (question.isSecret) input.type = "password";
        input.value = draft.get(question.id) || "";
        input.oninput = () => draft.set(question.id, input.value);
        const choices = element("div", "question-options");
        const selected = new Set((draft.get(question.id) || "").split(", "));
        for (const option of question.options || []) {
          const choice = button(option.label, option.description || option.label, () => {
            if (question.multiSelect) {
              selected.has(option.label) ? selected.delete(option.label) : selected.add(option.label);
              input.value = [...selected].join(", ");
              for (const node of choices.children) node.setAttribute("aria-pressed", String(selected.has(node.textContent)));
            } else input.value = option.label;
            draft.set(question.id, input.value);
          });
          // Claude 原生多选问题允许同时选中多项，保留自由输入并按原生 answers 字符串回传。
          if (question.multiSelect) choice.setAttribute("aria-pressed", String(selected.has(option.label)));
          choices.append(choice);
        }
        fields.push([question.id, input]); label.append(choices, input); card.append(label);
      }
      actions.append(button("提交回答", "提交所有问题的回答", () => {
        if (isClaude() && fields.some(([, input]) => !input.value.trim())) {
          notice("请回答所有问题", true); return;
        }
        return reply("answer", {
          response: { answers: Object.fromEntries(fields.map(([id, input]) => [id, { answers: [input.value] }])) }
        });
      }, "primary"));
    } else if (request.method === "item/permissions/requestApproval") {
      card.append(element("pre", "approval-description", JSON.stringify(request.params?.permissions, null, 2)));
      actions.append(button("仅本轮授予", "仅本轮授予所列权限", () => reply("answer", { response: { grant: true, scope: "turn" } }), "primary"),
        button("拒绝", "不授予权限", () => reply("answer", { response: { grant: false, scope: "turn" } })));
    } else if (toolApproval) {
      if (toolApproval.unsupported) card.append(element("small", "muted", "此工具授权需要在 Codex 桌面核对并处理。"));
      else {
        for (const [scope, label] of [["session", "本会话允许"], ["always", "始终允许"]]) {
          if (toolApproval.persist.includes(scope)) actions.append(button(label, `按本次请求指定范围${label}`,
            () => reply("answer", { response: { action: "accept", content: {}, _meta: { persist: scope } } })));
        }
        actions.append(button("拒绝", "拒绝此工具操作",
          () => reply("answer", { response: { action: "decline", content: null, _meta: null } })),
        button("允许一次", "仅允许本次工具操作",
          () => reply("answer", { response: { action: "accept", content: {}, _meta: null } }), "primary"));
      }
    } else if (request.method === "mcpServer/elicitation/request" && ["form", "openai/form"].includes(request.params?.mode)) {
      const draft = blockingApprovalDraft(requestThreadId, request);
      const input = element("textarea"); input.placeholder = "按请求的 JSON 格式填写回答"; input.setAttribute("aria-label", "工具询问回答");
      input.value = draft.get("content") || "";
      input.oninput = () => draft.set("content", input.value);
      card.append(input);
      actions.append(button("提交", "提交工具询问", () => {
        try { void reply("answer", { response: { action: "accept", content: JSON.parse(input.value) } }); }
        catch { notice("请输入有效 JSON", true); }
      }, "primary"), button("拒绝", "拒绝工具询问", () => reply("answer", { response: { action: "decline" } })));
    } else card.append(element("small", "muted", "此类请求请在 Codex 桌面处理。"));
    card.append(actions);
    if (!connected) {
      for (const control of card.querySelectorAll("button,input,select,textarea")) control.disabled = true;
      card.append(element("small", "muted", "连接恢复后可以处理此请求。"));
    }
    updateBlockingApprovalControls(card, approvalReplyKey(requestThreadId, request));
    cards.push(card);
  }
  const scrollTop = root.scrollTop;
  reconcileChildren(root, cards);
  root.scrollTop = scrollTop;
  observeApprovalCommands(root);
  fitApprovalArea();
}
