// Claude 控制提示：目标、计划与后台任务等原生控制提示。

import { button, element } from "../ui/dom.js";
import { notice } from "../ui/notice.js";
import { selectedId, session, thread } from "../core/state.js";
import { reloadThread } from "./conversation/history.js";
import { isClaude, privateInputScope } from "./session/capabilities.js";

let expandedClaudeGoalNotice = "";
// 用户要求网页能看出原版终端打开了回退菜单（/rewind，2.1.323 实测）：菜单打开期间网页消息只进入原生队列，
// 终端完成或取消回退后才执行；终端确认回退时，这些消息接在回退点之后执行。
export function claudeRewindDialogText(dialog) {
  const message = String(dialog?.description || "").split(/\s+/).filter(Boolean).join(" ").slice(0, 200);
  return [dialog?.step === "confirm"
    ? `终端正在确认回退到这条消息之前：${message || "（未读到消息原文）"}`
    : "终端正在选择要回退到哪条消息之前。",
  "此时发送的消息会先排队，等终端完成或取消回退后再执行；如果终端确认回退，这些消息会接在回退点之后执行。"];
}
// 计划审批批准项的中文按钮；原生措辞随权限配置变化（2.1.323 实测 auto mode / manually approve edits），未知措辞保留原文。
export function claudePlanOptionLabel(label) {
  const text = String(label || "").trim();
  if (/auto mode/i.test(text)) return "批准并使用自动模式";
  if (/auto-accept edits/i.test(text)) return "批准并自动接受编辑";
  if (/bypass permissions/i.test(text)) return "批准并跳过权限确认";
  if (/manually approve edits/i.test(text)) return "批准，编辑逐项确认";
  return `批准：${text}`;
}
// 原生底栏摘要（如 “1 shell”、“2 shells”）翻译为中文；未知写法保留原文，不猜测数量或类型。
function claudeBackgroundLabel(value) {
  const text = String(value || "").trim();
  const names = { shell: "后台命令", monitor: "监控", agent: "后台子代理", task: "后台任务" };
  const parts = text.split(/\s*,\s*/).map(part => {
    const match = /^(\d+)\s+(?:local\s+|background\s+)?(shell|monitor|agent|task)s?$/i.exec(part);
    return match ? `${match[1]} 个${names[match[2].toLowerCase()]}` : null;
  });
  return parts.length && parts.every(Boolean) ? parts.join("、") : text === "background" ? "后台任务" : text;
}
export function renderClaudeControlNotices(root) {
  if (!isClaude()) return;
  // 用户要求网页能看到原版 Claude 仍在运行的后台任务（后台命令、监控、后台子代理）；数据取自原生底栏，
  // 仅在会话在线且画面可读时由 Rust 视图 backgroundTasks 提供。任务结束后原版会以新一轮自动继续回复。
  const background = typeof thread?.backgroundTasks === "string" ? thread.backgroundTasks.trim() : "";
  if (background) root.append(element("div", "pending-banner",
    `后台任务运行中：${claudeBackgroundLabel(background)}。任务结束后 Claude 会自动继续回复。`));
  // 压缩运行及终态统一进入正文；等待/待确认只走 renderCompactionBanner，避免输入区双重提示。
  for (const command of thread?.nativeCommands || []) {
    if (command.name !== "goal" || !["pending", "unknown", "failed"].includes(command.state)) continue;
    const message = command.state === "pending" ? "目标命令已受理，等待执行"
      : command.state === "unknown" ? "目标命令结果待确认，不会自动重发" : "目标命令未完成";
    const banner = element("div", "pending-banner", message);
    if (command.state !== "pending")
      banner.append(button("重新读取", "读取原生状态", () => void reloadThread().catch(error => notice(error.message, true)), "text-button"));
    root.append(banner);
  }
  const text = thread?.goalNotice;
  if (typeof text !== "string" || !text) return;
  const label = text.startsWith("Goal paused") ? "目标已暂停"
    : text.startsWith("Goal still active") ? "目标评估暂未完成"
      : text.startsWith("Goal cleared after an unrecoverable error") ? "目标因错误已结束" : "目标状态已更新";
  const details = element("details", "pending-banner");
  const key = JSON.stringify([privateInputScope(session), selectedId, text]);
  details.open = expandedClaudeGoalNotice === key;
  details.ontoggle = () => {
    if (details.isConnected === false) return;
    expandedClaudeGoalNotice = details.open ? key : "";
  };
  details.append(element("summary", "", `${label} · 查看原因`), element("div", "approval-description", text));
  root.append(details);
}
