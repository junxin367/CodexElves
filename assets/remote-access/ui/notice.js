// 通用页面通知条：短时提示与错误提示。

import { $ } from "./dom.js";

export function notice(message, error = false) {
  // 线路状态由右上角显示，后台重试不再弹出线路不可用提示。
  if (message === "当前没有可用的请求线路，请等待连接恢复后重试。"
      || message === "线路正在等待重试，请稍后再试。") return;
  const node = $("#notice");
  node.textContent = message;
  node.className = `visible${error ? " error" : ""}`;
  if (typeof node.showPopover === "function") {
    node.setAttribute("popover", "manual");
    if (!node.matches(":popover-open")) node.showPopover();
  }
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => {
    node.className = "";
    if (node.hasAttribute("popover") && node.matches(":popover-open")) node.hidePopover();
  }, 6500);
}
