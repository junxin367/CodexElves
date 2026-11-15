// 会话条目渲染：按条目类型渲染单条消息；工具名称、Claude 事件文案与过程事件渲染在 item-presentation.js。

import { bindMessageVideos, videoPreviewMarkup } from "../../attachments.js";
import { prepareRemoteTunnelAccess } from "../../channels.js";
import { $, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { messageEditor } from "../../core/actions.js";
import { selectedId } from "../../core/state.js";
import { transcriptEventTypes } from "../conversation/transcript-events.js";
import { genericToolPresentation, reconcileChildren, renderTranscriptEvent } from "./item-presentation.js";
import { messageImage, renderImageViewItem, setMarkdown } from "./media.js";
import { editingMessage } from "./message-edit.js";
import { claudeQuestionReplies, renderClaudeQuestionReply } from "./question-reply.js";
import { isSubagentItem, renderSubagentItem } from "./subagents.js";
import { commandText, renderSkillReadItem, skillReadInfo } from "./tool-info.js";
import { renderToolOutput } from "./tool-output.js";
import { textContent, userDisplayContent, userMessageSkillOptions } from "./user-content.js";

export function renderItem(item, previous, inActivity = false, running = false) {
  if (item.type === "userMessage") {
    const node = previous || element("article", "message user-message");
    const editing = editingMessage?.threadId === selectedId && editingMessage.messageId === item.id;
    node.className = `message user-message${editing ? " editing-message" : ""}`;
    if (editing && editingMessage.form && node.contains(editingMessage.form)) return node;
    const display = userDisplayContent(item);
    const bubble = $(".user-bubble", node) || element("div", "user-bubble");
    if (display.questionReplies) {
      bubble.classList.remove("markdown");
      bubble.markdownHtml = null;
      bubble.replaceChildren();
      for (const reply of display.questionReplies) {
        const pair = element("div", "user-question-reply");
        const question = element("strong", "user-reply-question", reply.question.trim());
        question.title = reply.question.trim();
        pair.append(question, document.createTextNode("\n"), element("div", "user-reply-answer", reply.answer));
        bubble.append(pair);
      }
    } else {
      bubble.classList.add("markdown");
      setMarkdown(bubble, display.text, item.remoteImages, { userMessage: true, ...userMessageSkillOptions(item), remoteVideos: item.remoteVideos });
    }
    const attachments = $(".user-attachments", node) || element("div", "user-attachments");
    const previousImages = [...attachments.children], images = [];
    for (const content of item.content || []) if (["image", "localImage"].includes(content.type)) {
      const source = content.path || content.url;
      // Claude 图片沿用现有附件预览；内联资源 ID 不作为面向用户的文件名。
      const label = /^(?:data|inline):/iu.test(source || "") ? "图片附件" : String(source || "图片附件").split(/[\\/]/).at(-1);
      const image = messageImage(item.remoteImages?.find(image => image.source === source), label, source, previousImages[images.length]);
      images.push(image || element("span", "attachment-placeholder", "图片附件 · 请在 Codex 桌面查看"));
    }
    for (const content of item.content || []) if (["video", "localVideo"].includes(content.type)) {
      const asset = item.remoteVideos?.find(video => video.source === (content.path || content.url));
      const markup = videoPreviewMarkup(asset);
      if (!markup) continue;
      const previous = previousImages[images.length];
      if (previous?.videoSource === asset.url) { images.push(previous); continue; }
      const template = document.createElement("template");
      template.innerHTML = markup;
      const preview = template.content.firstElementChild;
      preview.videoSource = asset.url;
      images.push(preview);
    }
    reconcileChildren(attachments, images);
    bindMessageVideos(attachments, prepareRemoteTunnelAccess);
    reconcileChildren(node, [...(images.length ? [attachments] : []),
      ...(editing ? [messageEditor()] : display.text.trim() || display.questionReplies ? [bubble] : [])]);
    return node;
  }
  if (item.type === "agentMessage" || item.type === "plan") {
    const node = previous || element("article");
    node.className = `message agent-message${inActivity || item.phase === "commentary" ? " commentary" : ""}`;
    const body = $(".markdown", node) || element("div", "markdown");
    setMarkdown(body, textContent(item), item.remoteImages, { remoteVideos: item.remoteVideos }); reconcileChildren(node, [body]);
    return node;
  }
  if (isSubagentItem(item)) return renderSubagentItem(item, previous, running);
  const questionReplies = claudeQuestionReplies(item);
  if (questionReplies) return renderClaudeQuestionReply(questionReplies, previous);
  if (transcriptEventTypes.has(item.type)) return renderTranscriptEvent(item, previous);
  const reconnect = item.type === "error" && typeof item.message === "string"
    ? item.message.trim().match(/^Reconnecting(?:\.{3}|…)?\s+(\d+)\s*\/\s*(\d+)$/i) : null;
  if (reconnect) {
    const node = previous?.tagName === "DETAILS" && previous.classList.contains("reconnecting-status")
      ? previous : element("details");
    node.className = "reconnecting-status muted";
    const summary = element("summary", "", `正在重新连接 ${reconnect[1]}/${reconnect[2]}`);
    const chevron = icon("chevron");
    chevron.classList.add("reconnecting-chevron");
    summary.append(chevron);
    const description = typeof item.additionalDetails === "string" ? item.additionalDetails.trim() : "";
    node.replaceChildren(summary, ...(description ? [element("pre", "reconnecting-error", description)] : []));
    return node;
  }
  if (item.type === "imageView") return renderImageViewItem(item, previous);
  const skillRead = skillReadInfo(item);
  if (skillRead) return renderSkillReadItem(item, previous, skillRead);
  const titles = {
    reasoning: "思考", commandExecution: "执行命令", fileChange: "修改文件", mcpToolCall: "调用工具",
    dynamicToolCall: "调用工具", webSearch: "搜索网页", imageGeneration: "生成图片",
    collabAgentToolCall: "协作任务", error: "执行出错"
  };
  const details = previous?.tagName === "DETAILS" ? previous : element("details");
  const unknown = item.status === "unknown";
  const active = running && !unknown;
  const genericTool = ["mcpToolCall", "dynamicToolCall"].includes(item.type);
  const failed = item.status === "failed" || genericTool && (item.success === false || !!item.error || item.result?.isError === true);
  const interrupted = ["interrupted", "cancelled", "canceled"].includes(item.status);
  details.className = `tool-item${failed ? " failed" : ""}${interrupted ? " interrupted" : ""}${active ? " running" : ""}${item.type === "commandExecution" ? " command-item" : ""}${item.type === "reasoning" ? " reasoning-item" : ""}${genericTool ? " generic-tool-item" : ""}`;
  const summary = element("summary");
  const name = genericTool ? genericToolPresentation(item).description
    : item.type === "commandExecution" ? commandText(item)
    : item.type === "webSearch" ? item.action?.query || ""
    : item.type === "fileChange" ? (item.changes || []).map(change => String(change.path || "").split(/[\\/]/).at(-1)).join("、") : "";
  if (genericTool) {
    const presentation = genericToolPresentation(item);
    const state = failed ? "失败" : interrupted ? "已中断" : unknown ? "状态未知" : "";
    const visual = icon(failed ? "warning" : interrupted ? "stop" : presentation.icon);
    const chevron = icon("chevron");
    chevron.classList.add("tool-chevron");
    summary.append(visual, element("span", `tool-description${active ? " working-label" : ""}`, presentation.description),
      element("small", "muted tool-status-label", state), chevron);
    summary.title = presentation.description;
    summary.setAttribute("aria-label", active ? `${presentation.description}，执行中`
      : state ? `${presentation.description}，${state}` : presentation.description);
    details.setAttribute("aria-busy", String(active));
  } else {
    // Claude 后台命令：启动即返回，结束状态由原生任务通知写回；“已转入后台”只陈述启动事实，不声称仍在运行。
    const background = item.type === "commandExecution" && typeof item.backgroundStatus === "string" ? item.backgroundStatus : "";
    const backgroundLabel = { running: "已转入后台", completed: "后台已完成", failed: "后台失败",
      killed: "后台已停止", stopped: "后台已停止", unknown: "后台结果未知" }[background] || "";
    const label = element("strong", "", item.type === "reasoning" ? active ? "正在思考" : unknown || failed || interrupted ? "思考" : "已完成思考"
      : item.type === "commandExecution" ? active && !background ? "正在运行" : backgroundLabel || (unknown || failed || interrupted ? "执行命令" : "已运行") : titles[item.type] || item.type);
    summary.append(!active && !failed && !interrupted && !unknown && ["commandExecution", "fileChange"].includes(item.type)
      ? icon(item.type === "commandExecution" ? "terminal" : "fileChange")
      : element("span", `tool-state${active ? " spinner" : ""}`, active ? "" : failed ? "!" : interrupted ? "■" : unknown ? "?" : "✓"),
      label, element("span", "tool-description", name),
      element("small", "muted", active && !background ? "进行中" : item.exitCode != null && item.exitCode !== 0 ? `退出码 ${item.exitCode}`
        : failed ? "失败" : interrupted ? "已中断" : unknown ? "状态未知" : ""));
    if (["commandExecution", "reasoning"].includes(item.type)) {
      summary.title = item.type === "reasoning" ? "查看思考内容" : name;
      const chevron = icon("chevron");
      chevron.classList.add(item.type === "reasoning" ? "tool-chevron" : "command-chevron");
      summary.append(chevron);
    }
  }
  details.replaceChildren(summary);
  details.ontoggle = () => { if (details.isConnected) renderToolOutput(details, item); };
  renderToolOutput(details, item);
  return details;
}
