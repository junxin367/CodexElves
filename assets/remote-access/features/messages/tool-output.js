// 工具输出渲染：通用工具与命令结果的展示；命令文本、Skill 读取识别与结果分段在 tool-info.js。

import { copyText } from "../../ui/clipboard.js";
import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { messageImage, setMarkdown } from "./media.js";
import { commandText, toolResultParts } from "./tool-info.js";

function renderGenericToolOutput(body, item) {
  // 用户要求回退 Claude 特殊渲染：工具结果、文件修改和工作分组保持原有通用展示。
  body.classList.add("generic-tool-body");
  const parts = toolResultParts(item);
  if (!parts.length) {
    body.append(element("p", "tool-result-empty muted",
      ["inProgress", "running"].includes(item.status) ? "工具尚未返回结果。" : "此工具没有返回可展示的内容。"));
    return;
  }
  const outputs = parts.map(part => {
    if (part.type === "image") {
      const image = messageImage({ url: part.source }, "工具返回的图片", part.source)
        || element("span", "attachment-placeholder", "工具返回的图片暂不可用，请在 Codex 桌面查看。");
      body.append(image);
      return { ...part, node: image };
    }
    const section = element("div", "tool-result-section");
    const output = element("pre", "tool-output");
    section.append(element("div", "tool-result-label", "纯文本"), output);
    body.append(section);
    return { ...part, node: section, output };
  });
  let shown = 20_000;
  const more = button("显示更多输出", "显示更多输出", () => { shown += 20_000; update(); }, "text-button");
  const update = () => {
    let remaining = shown, truncated = false;
    for (const entry of outputs) {
      entry.node.hidden = truncated;
      if (truncated || entry.type !== "text") continue;
      if (!remaining && entry.text.length) {
        entry.node.hidden = true; truncated = true; continue;
      }
      entry.output.textContent = entry.text.slice(0, remaining);
      truncated = entry.text.length > remaining;
      remaining = Math.max(0, remaining - entry.text.length);
    }
    more.hidden = !truncated;
  };
  update(); body.append(more);
}
export function renderToolOutput(details, item) {
  details.querySelector(".tool-body")?.remove();
  if (!details.open) return;
  const body = element("div", "tool-body");
  // Claude 网页搜索带原生结果正文，展开时按通用结果显示，不输出条目 JSON。
  if (["mcpToolCall", "dynamicToolCall"].includes(item.type) || item.type === "webSearch" && item.source === "claude") {
    renderGenericToolOutput(body, item); details.append(body); return;
  }
  const content = item.type === "commandExecution" ? item.aggregatedOutput || item.output || ""
    : item.type === "reasoning" ? [item.summary, item.content].flatMap(value => Array.isArray(value) ? value.map(entry => entry.text || entry) : [value || ""]).join("\n\n")
    : item.type === "fileChange" ? (item.changes || []).map(change => `${change.path || ""}\n${change.diff || ""}`).join("\n\n")
    : item.type === "error" ? item.message : JSON.stringify(item, null, 2);
  const text = typeof content === "string" ? content : JSON.stringify(content ?? "");
  let shown = 20_000;
  const output = element(item.type === "reasoning" ? "div" : "pre", `tool-output${item.type === "reasoning" ? " reasoning-output markdown" : ""}`);
  let result = output;
  if (item.type === "commandExecution") {
    body.classList.add("command-shell");
    const title = element("div", "command-shell-title");
    const copy = button("", "复制完整命令输出", () => void copyText(`$ ${commandText(item)}\n${text}`), "command-shell-copy");
    copy.append(icon("copy"));
    title.append(element("span", "", "Shell"), copy); body.append(title);
    output.classList.add("command-output");
    result = element("span", "command-result");
    output.append(element("span", "command-prompt", `$ ${commandText(item)}\n`), result);
  }
  const more = button("显示更多输出", "显示更多输出", () => { shown += 20_000; update(); }, "text-button");
  const update = () => {
    if (item.type === "reasoning") setMarkdown(output, text.slice(0, shown));
    else result.textContent = text.slice(0, shown);
    more.hidden = shown >= text.length;
  };
  update(); body.append(output, more); details.append(body);
}
