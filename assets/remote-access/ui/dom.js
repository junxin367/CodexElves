// 通用 DOM 辅助：查询、创建元素与按钮、HTML 转义。不依赖任何会话业务状态。

export const $ = (selector, root = document) => root.querySelector(selector);
const escapes = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const escape = value => String(value ?? "").replace(/[&<>"']/g, character => escapes[character]);
export const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
};
export function button(text, label, action, className = "") {
  const node = element("button", className, text);
  node.type = "button";
  node.title = label;
  node.setAttribute("aria-label", label);
  node.addEventListener("click", action);
  return node;
}
