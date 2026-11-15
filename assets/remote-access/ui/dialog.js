// 通用确认对话框：返回 Promise<boolean>，关闭后恢复触发前焦点。

import { $, button, element } from "./dom.js";
import { icon } from "./icons.js";
import { closeAppMenus, closePopup } from "./popup.js";

export function confirmAction(title, message, action, danger = false, className = "") {
  closePopup(); closeAppMenus();
  if ($("#confirmation-dialog")) return Promise.resolve(false);
  const trigger = document.activeElement;
  const dialog = element("dialog", `confirmation-dialog${className ? ` ${className}` : ""}`);
  dialog.id = "confirmation-dialog";
  dialog.setAttribute("aria-labelledby", "confirmation-title"); dialog.setAttribute("aria-describedby", "confirmation-message");
  const heading = element("h2", "", title); heading.id = "confirmation-title";
  const description = element("p", "muted", message); description.id = "confirmation-message";
  const actions = element("div", "confirmation-actions");
  actions.append(button("取消", "取消操作", () => dialog.close("cancel")),
    button(action, action, () => dialog.close("confirm"), danger ? "danger-button" : "primary"));
  const content = element("div", "confirmation-content"), text = element("div");
  text.append(heading, description); content.append(icon("warning"), text);
  dialog.append(content, actions); document.body.append(dialog);
  return new Promise(resolve => {
    dialog.onclose = () => {
      const confirmed = dialog.returnValue === "confirm";
      dialog.remove(); document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
      if (trigger?.isConnected && document.activeElement === document.body) trigger.focus();
      resolve(confirmed);
    };
    document.body.classList.add("modal-open");
    dialog.showModal(); $("button", actions).focus();
  });
}
