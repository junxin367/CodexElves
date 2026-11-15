// Claude 提问的回答气泡：已回答的原生 AskUserQuestion 在原位置显示为“问题 + 回答”用户气泡，样式与 Codex 问答回复一致。

import { element } from "../../ui/dom.js";
import { reconcileChildren } from "./item-presentation.js";
import { messageImage } from "./media.js";

// 用户要求经远程会话发出的回复在正文中可见（2026-11-15）：提问等待时从输入框发出的文字和截图会被原生记为
// AskUserQuestion 的回答，原先只能在折叠的“使用了工具”分组里展开看到。服务端带回 questionAnswers 后改为独立气泡，
// 不进入工作分组；未作答的题目显示“未回答”。拒答、中断或旧服务没有结构化回答时，仍按原工具卡片显示。
export function claudeQuestionReplies(item) {
  if (item?.type !== "dynamicToolCall" || item.source !== "claude" || item.tool !== "AskUserQuestion"
      || item.status !== "completed" || !Array.isArray(item.questionAnswers)) return null;
  const questions = Array.isArray(item.arguments?.questions) ? item.arguments.questions : [];
  const replies = item.questionAnswers.filter(reply => typeof reply?.question === "string").map(reply => {
    const header = questions.find(question => question?.question === reply.question)?.header;
    // 截图与用户消息相同：服务端把 data URL 登记为 remoteImages，url 改成 inline:<id>，按 source 取受鉴权地址。
    const images = (Array.isArray(reply.images) ? reply.images : []).filter(image => typeof image?.url === "string")
      .map(image => ({ source: image.url, asset: item.remoteImages?.find(remote => remote.source === image.url) }));
    return {
      question: reply.question.trim(), header: typeof header === "string" ? header.trim() : "",
      answer: typeof reply.answer === "string" ? reply.answer : null, images
    };
  });
  return replies.some(reply => reply.answer !== null) ? replies : null;
}
export function renderClaudeQuestionReply(replies, previous) {
  const node = previous?.classList.contains("claude-question-reply") ? previous : element("article");
  node.className = "message user-message claude-question-reply";
  node.setAttribute("aria-label", "你对 Claude 提问的回答");
  const attachments = node.querySelector(":scope > .user-attachments") || element("div", "user-attachments");
  const previousImages = [...attachments.children];
  // 截图沿用用户消息的缩略图与点击放大；连接端只转发本用户上传目录内、符合远程输入大小限制的图片。
  const images = [];
  for (const image of replies.flatMap(reply => reply.images)) {
    const node = messageImage(image.asset, "图片附件", image.source, previousImages[images.length]);
    if (node) images.push(node);
  }
  reconcileChildren(attachments, images);
  const bubble = element("div", "user-bubble");
  for (const reply of replies) {
    const pair = element("div", "user-question-reply");
    const question = element("strong", "user-reply-question", [reply.header, reply.question].filter(Boolean).join(" · "));
    question.title = reply.question;
    const answer = reply.answer === null ? element("div", "user-reply-answer unanswered", "未回答")
      : element("div", "user-reply-answer", reply.answer);
    pair.append(question, document.createTextNode("\n"), answer);
    bubble.append(pair);
  }
  reconcileChildren(node, [...(images.length ? [attachments] : []), bubble]);
  return node;
}
