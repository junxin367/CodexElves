// 条目展示：工具名称与展示文本、推理预览、Claude 事件文案、子节点复用与过程事件渲染。

import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { models } from "../composer/options-state.js";
import { effortLabels } from "../composer/prompt-editor.js";
import { compactionQueueSendReason } from "../conversation/compaction.js";

export function transcriptModelName(id) {
  if (typeof id !== "string" || !id) return "未知模型";
  return models.find(model => model.model === id || model.id === id)?.displayName || id;
}
// 用户要求 Claude 原生任务通知触发的自动续跑显示为“自动继续”，不显示成用户消息；原生摘要放在信息提示里。
function claudeTaskNotificationText(item) {
  const status = String(item.status || "");
  const subject = { command: "后台命令", agent: "后台子代理", monitor: "监控", schedule: "定时任务" }[item.kind] || "后台任务";
  // 监控每个事件都是无状态通知；监控结束时的通知带状态（2.1.323 实测）。定时任务触发时摘要为其提示词。
  const state = item.kind === "schedule" ? "已触发" : !status && item.kind === "monitor" ? "收到事件"
    : status === "completed" ? item.kind === "monitor" ? "已结束" : "已完成"
    : status === "failed" ? "失败" : ["killed", "stopped"].includes(status) ? "已停止" : "发来通知";
  const detail = [item.summary, item.event].filter(value => typeof value === "string" && value.trim()).join("\n");
  return {
    label: `${subject}${state} · Claude 自动继续`,
    description: detail || "原生任务通知没有提供摘要。"
  };
}
// 用户要求终端本地命令与原生系统提示不显示成用户消息、也不静默丢弃：以紧凑事件行展示，原文放在信息提示中。
function claudeEventText(item) {
  const clip = (value, size) => {
    const chars = Array.from(String(value || "").replace(/\s+/g, " ").trim());
    return chars.length > size ? `${chars.slice(0, size).join("")}…` : chars.join("");
  };
  if (item.type === "claudeLocalCommand") {
    const command = [item.name, item.args].filter(value => typeof value === "string" && value.trim()).join(" ");
    const output = typeof item.output === "string" ? item.output.trim() : "";
    return { label: `终端执行了 ${clip(command, 80) || "本地命令"}`,
      description: output || (typeof item.output === "string" ? "命令没有输出。" : "尚未收到命令输出。") };
  }
  const text = typeof item.text === "string" ? item.text.trim() : "";
  // 用户要求回顾消息沿用 Claude CLI 原生的“※ recap:”前缀，不再显示中文名称。
  if (item.kind === "awaySummary") return { label: `※ recap: ${clip(text, 160)}`, description: text || "原生回顾没有内容。" };
  if (item.kind === "stopHook") {
    const details = [item.reason, ...(Array.isArray(item.errors) ? item.errors : [])]
      .filter(value => typeof value === "string" && value.trim());
    return { label: item.prevented ? "Stop Hook 阻止了继续执行" : "Stop Hook 执行出错",
      description: details.join("\n") || "原生记录没有提供原因。" };
  }
  return { label: `Claude 提示：${clip(text, 120)}`, description: text || "原生提示没有内容。" };
}
export function renderTranscriptEvent(item, previous) {
  const node = previous || element("div");
  const notification = item.type === "claudeTaskNotification";
  const claudeEvent = ["claudeLocalCommand", "claudeNotice"].includes(item.type);
  node.className = `transcript-event muted ${item.type === "contextCompaction" ? "context-compaction"
    : notification ? "task-notification" : claudeEvent ? "claude-event" : "model-change"}`;
  const symbol = icon(item.type === "contextCompaction" ? "compaction" : notification ? "play"
    : item.type === "claudeLocalCommand" ? "terminal" : claudeEvent ? "info" : "model");
  symbol.classList.add("transcript-event-icon");
  // 用户要求切回会话仍显示压缩状态；服务历史可能只有起止时间，不能等下一条实时消息补 completed。
  const status = typeof item.status === "string" ? item.status : item.status?.type;
  const ended = item.completed === true || Number.isFinite(item.completedAtMs)
    || ["completed", "failed", "interrupted", "cancelled", "canceled"].includes(status);
  const compacting = item.type === "contextCompaction" && !ended && status !== "unknown"
    && (item.completed === false || Number.isFinite(item.startedAtMs) || ["inProgress", "running"].includes(status));
  let label, description;
  if (item.type === "contextCompaction") {
    const failed = status === "failed", interrupted = ["interrupted", "cancelled", "canceled"].includes(status);
    label = failed ? "上下文压缩失败" : interrupted ? "上下文压缩已中断"
      : item.outcome === "noop" ? "当前历史无需压缩"
      : status === "unknown" ? "压缩结果待确认"
      : compacting && item.phase === "stopping" ? "正在停止压缩"
      : compacting ? "正在压缩上下文 · 可能需要几分钟"
      : ended && item.source === "automatic" ? "上下文已自动压缩"
      : ended ? "上下文已压缩" : "上下文压缩状态已更新";
    description = failed ? "上下文压缩未完成，请查看原生会话中的错误。"
      : interrupted ? "上下文压缩已中断，未确认成功完成。"
      : item.outcome === "noop" ? "原生会话确认当前历史无需压缩。"
      : status === "unknown" ? "尚未确认压缩结果，请重新读取原生状态；不会自动重发压缩请求。"
      : compacting && item.phase === "stopping" ? "已请求停止压缩，等待原生执行结束。"
      : compacting ? `正在缩短较早的会话上下文。${compactionQueueSendReason()
        || "可继续编辑并发送普通消息，消息会加入原生队列，在压缩完成后执行。"}`
      : ended ? "已缩短较早的会话上下文，以便继续处理后续消息。"
      : "尚未收到明确的压缩完成状态，请以原生会话为准。";
    // 沿用现有信息提示展示原生统计，不额外增加正文卡片或改动间距。
    const validCount = value => Number.isSafeInteger(value) && value >= 0;
    if (validCount(item.preTokens) && validCount(item.postTokens))
      description += ` 压缩前 ${item.preTokens.toLocaleString("zh-CN")} Token，压缩后 ${item.postTokens.toLocaleString("zh-CN")} Token。`;
    if (validCount(item.durationMs)) description += ` 用时 ${(item.durationMs / 1000).toLocaleString("zh-CN")} 秒。`;
  } else if (notification) {
    ({ label, description } = claudeTaskNotificationText(item));
  } else if (claudeEvent) {
    ({ label, description } = claudeEventText(item));
  } else {
    const from = transcriptModelName(item.fromModel), to = transcriptModelName(item.toModel);
    const effortChanged = typeof item.fromEffort === "string" && typeof item.toEffort === "string"
      && item.fromEffort !== item.toEffort;
    const modelChanged = typeof item.fromModel === "string" && typeof item.toModel === "string"
      && item.fromModel !== item.toModel;
    const effort = value => effortLabels[value] || value;
    // 用户要求沿用 Codex 的紧凑变更行；只改等级也显示，恢复时同配置重设不冒充新的模型切换。
    label = effortChanged && !modelChanged
      ? `思考深度已从 ${effort(item.fromEffort)} 更改为 ${effort(item.toEffort)}`
      : (item.type === "modelRerouted" ? `模型已从 ${from} 重新路由到 ${to}` : `模型已从 ${from} 更改为 ${to}`)
        + (effortChanged ? `，思考深度从 ${effort(item.fromEffort)} 更改为 ${effort(item.toEffort)}` : "");
    description = item.source === "claude"
      ? `已根据 Claude 原生实际请求确认${item.remoteInlineEvent ? "，从此处开始使用此配置" : "，此配置用于下方消息"}。`
      : `模型切换可能触发上下文压缩。${item.reason ? ` 原因：${item.reason}` : ""}`;
  }
  const text = element("span", `transcript-event-label${compacting && item.status !== "failed" ? " working-label" : ""}`, label);
  const info = button("", description, event => event.stopPropagation(), "transcript-event-info");
  info.append(icon("info"));
  node.replaceChildren(symbol, text, info);
  node.setAttribute("role", compacting && item.status !== "failed" ? "status" : "note");
  node.setAttribute("aria-label", `${label}。${description}`);
  return node;
}
function readableToolName(item) {
  const words = value => String(value || "").replace(/^mcp__/, "").replace(/([a-z\d])([A-Z])/g, "$1 $2")
    .replace(/[._/-]+/g, " ").replace(/\s+/g, " ").trim();
  return [words(item.server), words(item.tool)].filter(Boolean).join(" · ")
    || (item.type === "dynamicToolCall" ? "动态工具" : "MCP 工具");
}
// 用户要求 Claude 实例按原生工具语义展示：内置工具显示动作和关键参数，不显示“动态工具”或英文工具名。
// 与 Rust claude_tools.rs 对应：命令、文件修改、网页搜索、MCP 已转为专用条目，这里只处理其余内置工具；
// 文件修改与任务清单工具仅在失败（改为通用卡片保留原生报错）时出现在这里。
export function claudeToolPresentation(item) {
  const input = item.arguments && typeof item.arguments === "object" ? item.arguments : {};
  const clip = value => Array.from(String(value ?? "").replace(/\s+/g, " ").trim()).slice(0, 80).join("");
  const file = value => clip(String(value || "").split(/[\\/]/).filter(Boolean).at(-1) || "");
  const join = (...parts) => parts.filter(Boolean).join(" · ");
  const task = value => typeof value === "string" && value.trim() || Number.isSafeInteger(value) ? `#${clip(value)}` : "";
  const host = value => {
    try { return new URL(value).host; } catch { return clip(value); }
  };
  const tool = String(item.tool || "");
  switch (tool) {
    case "Read": return { icon: "eye", description: join("读取文件", file(input.file_path)), explore: true };
    case "NotebookRead": return { icon: "eye", description: join("读取笔记本", file(input.notebook_path)), explore: true };
    case "Grep": return { icon: "search", description: join("搜索", input.pattern ? `“${clip(input.pattern)}”` : "", file(input.path)), explore: true };
    case "Glob": return { icon: "search", description: join("查找文件", clip(input.pattern), file(input.path)), explore: true };
    case "LS": return { icon: "folderClosed", description: join("列出目录", file(input.path)), explore: true };
    case "WebFetch": return { icon: "globe", description: join("读取网页", input.url ? host(input.url) : "") };
    case "Edit": case "MultiEdit": return { icon: "edit", description: join("修改文件", file(input.file_path)) };
    case "Write": return { icon: "edit", description: join("写入文件", file(input.file_path)) };
    case "NotebookEdit": return { icon: "edit", description: join("修改笔记本", file(input.notebook_path)) };
    case "Skill": return { icon: "wrench", description: join("使用技能", clip(input.skill)) };
    case "ToolSearch": return { icon: "search", description: join("加载工具", clip(input.query).replace(/^select:/, "")) };
    case "TaskCreate": return { icon: "queue", description: join("创建任务", clip(input.subject)) };
    case "TaskUpdate": return { icon: "queue", description: join("更新任务", task(input.taskId)) };
    case "TodoWrite": return { icon: "queue", description: "更新任务清单" };
    case "TaskList": return { icon: "queue", description: "查看任务清单" };
    case "TaskGet": return { icon: "queue", description: join("查看任务", task(input.taskId)) };
    case "TaskOutput": case "BashOutput":
      return { icon: "terminal", description: join("查看后台任务输出", clip(input.task_id || input.bash_id)) };
    case "TaskStop": case "KillShell": case "KillBash":
      return { icon: "stop", description: join("停止后台任务", clip(input.task_id || input.shell_id)) };
    case "Monitor": return { icon: "eye", description: join("监控", clip(input.description || input.command)) };
    case "AskUserQuestion":
      return { icon: "chat", description: join("提问", clip(Array.isArray(input.questions) ? input.questions[0]?.question : "")) };
    case "ExitPlanMode": return { icon: "target", description: "提交计划" };
    case "EnterPlanMode": return { icon: "target", description: "进入计划模式" };
    case "ScheduleWakeup":
      return { icon: "recent", description: join("定时唤醒", Number.isFinite(input.delaySeconds) ? `${input.delaySeconds} 秒后` : "") };
    case "CronCreate": return { icon: "recent", description: join("创建定时任务", clip(input.cron || input.schedule)) };
    case "CronDelete": return { icon: "recent", description: join("删除定时任务", clip(input.id)) };
    case "CronList": return { icon: "recent", description: "查看定时任务" };
    case "SendMessage": return { icon: "chat", description: join("发送消息", clip(input.to)) };
    case "EnterWorktree": return { icon: "folderClosed", description: "进入工作树" };
    case "ExitWorktree": return { icon: "folderClosed", description: "离开工作树" };
    default: return { icon: "tools", description: readableToolName(item) };
  }
}
export function genericToolPresentation(item) {
  if (item.source === "claude" && item.type === "dynamicToolCall") return claudeToolPresentation(item);
  const surfaces = [
    item.toolSurface,
    item._meta?.["codex/toolSurface"],
    item.result?._meta?.["codex/toolSurface"]
  ];
  const browser = surfaces.some(surface => surface?.kind === "browserUse")
    || item.pluginId === "unified-computer-use@openai-bundled"
    || /(?:^|[\s_-])(?:browser|chrome|cua)(?:$|[\s_-])/i.test(`${item.server || ""} ${item.tool || ""}`);
  const title = typeof item.arguments?.title === "string"
    ? Array.from(item.arguments.title.replace(/\s+/g, " ").trim()).slice(0, 80).join("") : "";
  return { icon: browser ? "globe" : "tools", description: title || readableToolName(item) };
}
export function reasoningPreviewText(item) {
  return [item.summary, item.content].flatMap(value => Array.isArray(value) ? value : [value])
    .map(part => typeof part === "string" ? part : part?.text || "").filter(text => text.trim()).at(-1) || "";
}
export function reconcileChildren(root, children) {
  let cursor = root.firstChild;
  for (const child of children) {
    if (cursor === child) cursor = cursor.nextSibling;
    else root.insertBefore(child, cursor);
  }
  while (cursor) { const next = cursor.nextSibling; cursor.remove(); cursor = next; }
}
