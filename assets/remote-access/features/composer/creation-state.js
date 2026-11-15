// 新建会话的选择状态：后端、实例、项目、Codex 默认项与分支读取状态，以及项目选择器同步。

import { $ } from "../../ui/dom.js";
import { session, sidebar } from "../../core/state.js";

export const newChatModels = new Map();
export let codexCreationDefaultsState = null;
export function setCodexCreationDefaultsState(value) { return codexCreationDefaultsState = value; }
export let creationProject = "";
export function setCreationProject(value) { return creationProject = value; }
export let creationBackend = "codex";
export function assignCreationBackend(value) { return creationBackend = value; }
export let creationInstanceId = "codex:local";
export function setCreationInstanceId(value) { return creationInstanceId = value; }
export let creationBranchRead = null, creationBranchSnapshot = null;
export function setCreationBranchRead(value) { return creationBranchRead = value; }
export function setCreationBranchSnapshot(value) { return creationBranchSnapshot = value; }
export function newChatProjectOptions(projects = []) {
  // 兼容旧服务及旧页面缓存：历史工作目录不能成为可选项目，也不能触发同名项目的路径提示。
  projects = projects.filter(project => !project.synthetic);
  const names = new Map();
  for (const project of projects) names.set(project.name, (names.get(project.name) || 0) + 1);
  return [
    ...projects.map(project => ({
      value: project.id, label: project.name,
      description: names.get(project.name) > 1
        ? (Array.isArray(project.roots) ? project.roots.join(" · ") : project.cwd || "")
        : "",
      searchText: Array.isArray(project.roots) ? project.roots.join(" ") : project.cwd || "",
      icon: "folder"
    })),
    { value: "", label: "不在项目中工作", icon: "close", footer: true }
  ];
}
export function syncNewChatProjectPicker() {
  const select = $("#project-select");
  if (!select || !sidebar) return;
  select.disabled = !!session?.readOnly;
  const control = select.closest(".new-chat-project-control"), clear = $("#clear-new-chat-project");
  if (control) control.dataset.selected = String(!!select.value);
  if (clear) { clear.hidden = !select.value; clear.disabled = select.disabled; }
}
export function resetNewChatBranch() {
  creationBranchRead?.controller.abort();
  creationBranchRead = creationBranchSnapshot = null;
}
