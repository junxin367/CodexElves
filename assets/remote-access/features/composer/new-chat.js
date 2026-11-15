// 新建会话：项目与分支选择、实例切换、记住新会话的模型/权限设置与 Codex 创建默认值。

import { permissionModeKnown, permissionOptions } from "../../permissions.js";
import { setWorkspaceBranch } from "../../session-files.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { closePopup } from "../../ui/popup.js";
import { setSelectOptions } from "../../ui/select.js";
import {
  connected, creating, nativeConnectionVersion, selectionToken, selectionVersion, session,
  setSelectionToken, setSelectionVersion, sidebar
} from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { renderApprovals } from "../approvals.js";
import { closeComposerMenus } from "./composer-menus.js";
import { updateComposer } from "./composer.js";
import {
  attachmentDrafts, deleteDraft, draftKey, drafts, rememberDraft, resetComposerOptions,
  restoreDraft, snapshotDraft, storeDraft
} from "./drafts.js";
import { currentEffort, modelForId } from "./models.js";
import { instancePermissionCatalogs } from "./permissions.js";
import { setSelectedSkills } from "./skills.js";
import { renderMessages } from "../conversation/history-view.js";
import { selectThread } from "../conversation/selection.js";
import { setSelectedGoal } from "../goal/composer.js";
import {
  activeBackend, backendAvailable, isClaude, newChatBackendOptions, privateInputScope
} from "../session/capabilities.js";
import { setConnection } from "../session/connection.js";
import { activeInstanceId, availableSessionInstances, sessionInstanceEnabled } from "../session/instances.js";
import { stream } from "../session/lifecycle.js";
import { sessionTools } from "../shell.js";

const newChatModels = new Map();
export let codexCreationDefaultsState = null;
export function setCodexCreationDefaultsState(value) { return codexCreationDefaultsState = value; }
export let creationProject = "";
export function setCreationProject(value) { return creationProject = value; }
export let creationBackend = "codex";
export function assignCreationBackend(value) { return creationBackend = value; }
export let creationInstanceId = "codex:local";
export function setCreationInstanceId(value) { return creationInstanceId = value; }
let creationBranchRead = null, creationBranchSnapshot = null;
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
function refreshNewChatBranch() {
  const badge = $("#new-chat-branch");
  if (!badge) return;
  if (!creating || !session || !creationProject) {
    resetNewChatBranch();
    // 用户要求“不在项目中工作”不显示分支标签；同时清空旧项目文字，避免切换时短暂残留。
    setWorkspaceBranch(badge, null);
    return;
  }
  // 用户要求新建页展示所选项目的当前分支；按授权、运行方式和本次选择隔离，晚到响应不能串到另一项目。
  const key = JSON.stringify([privateInputScope(session), nativeConnectionVersion ?? null, creationProject, selectionToken]);
  if (creationBranchRead?.key === key && creationBranchRead.badge === badge) return creationBranchRead.promise;
  // 与现有 15 秒连接/侧栏轮询同步刷新，不因输入、展开菜单或重复渲染而频繁启动 Git。
  if (creationBranchSnapshot?.key === key && Date.now() - creationBranchSnapshot.loadedAt < 15000) {
    setWorkspaceBranch(badge, creationBranchSnapshot.git);
    return;
  }
  creationBranchRead?.controller.abort();
  const record = { key, badge, controller: new AbortController(), promise: null };
  creationBranchRead = record;
  setWorkspaceBranch(badge, creationBranchSnapshot?.key === key ? creationBranchSnapshot.git : null, "读取分支…");
  const current = () => creationBranchRead === record && !record.controller.signal.aborted
    && creating && session && $("#new-chat-branch") === badge
    && JSON.stringify([privateInputScope(session), nativeConnectionVersion ?? null, creationProject, selectionToken]) === key;
  record.promise = api("/api/workspace/context", { projectId: creationProject }, undefined, record.controller.signal)
    .then(result => {
      if (!current()) return;
      const git = result?.ok === true && result.git && typeof result.git === "object"
        ? result.git : { unavailable: true };
      creationBranchSnapshot = { key, git, loadedAt: Date.now() };
      setWorkspaceBranch(badge, git);
    })
    .catch(error => {
      if (!current() || error.name === "AbortError") return;
      creationBranchSnapshot = { key, git: { unavailable: true }, loadedAt: Date.now() };
      setWorkspaceBranch(badge, creationBranchSnapshot.git);
    })
    .finally(() => { if (creationBranchRead === record) creationBranchRead = null; });
  return record.promise;
}
export function changeCreationProject() {
  syncNewChatProjectPicker();
  const project = $("#project-select").value;
  if (!creating || project === creationProject) { void refreshNewChatBranch(); return; }
  rememberDraft();
  creationProject = project; setSelectionVersion(selectionVersion + 1); setSelectionToken(randomId());
  void refreshNewChatBranch();
  sessionTools?.updateContext();
  restoreDraft(draftKey()); resetComposerOptions(); updateComposer();
}
export function changeCreationBackend() {
  const select = $("#new-chat-backend");
  if (!select || select.disabled) return;
  setCreationBackend(select.value);
}
export function setCreationBackend(instanceId) {
  const instance = availableSessionInstances().find(value => value.id === instanceId
    || value.backend === instanceId && value.legacyDefault);
  if (!creating || !instance?.ready || !sessionInstanceEnabled(instance.id) || instance.id === creationInstanceId) return;
  const draft = snapshotDraft();
  rememberDraft(); closePopup(); closeComposerMenus();
  creationBackend = instance.backend; creationInstanceId = instance.id;
  setSelectionVersion(selectionVersion + 1); setSelectionToken(randomId());
  // 实例按钮、设置开关和恢复默认共用切换流程；目标实例已有草稿优先，首次切换保留文字/图片，清理模型、目标和 Skill。
  setSelectedSkills([]); setSelectedGoal(null);
  if (!drafts.has(draftKey()) && !attachmentDrafts.has(draftKey()))
    storeDraft(draftKey(), { ...draft, skills: [], goalMode: null });
  restoreDraft(draftKey()); resetComposerOptions();
  const select = $("#new-chat-backend");
  if (select) setSelectOptions(select, newChatBackendOptions(), instance.id);
  renderMessages();
  setConnection(backendAvailable() && stream?.readyState === EventSource.OPEN);
  updateComposer(); renderApprovals();
}
export function selectCreatedThread(id) {
  const key = draftKey(), draft = snapshotDraft();
  storeDraft(id, draft);
  const selection = selectThread(id, { closeSidebar: false });
  deleteDraft(key);
  return selection;
}
export function rememberedNewChatModel() {
  return modelForId(rememberedNewChatSettings().model);
}
export function newChatPreferenceScope() {
  const owner = privateInputScope(session), scope = owner && `${owner}${activeInstanceId() === "codex:local" ? "" : `:${activeInstanceId()}`}`;
  return scope || "";
}
export function rememberedNewChatSettings() {
  const scope = newChatPreferenceScope();
  if (!scope) return {};
  if (!newChatModels.has(scope)) {
    let model = "";
    try { model = localStorage.getItem(`codex-elves-new-chat-model:${scope}`) || ""; } catch {}
    let settings = { model };
    try {
      const saved = JSON.parse(localStorage.getItem(`codex-elves-new-chat-settings:${scope}`) || "null");
      if (saved?.version === 1 && typeof saved.model === "string"
          && (saved.reasoningEffort == null || typeof saved.reasoningEffort === "string")
          && (saved.permissionMode == null || saved.permissionMode === "" || permissionModeKnown(activeBackend(), saved.permissionMode))) {
        settings = { model: saved.model, reasoningEffort: saved.reasoningEffort || "",
          permissionMode: saved.permissionMode || "" };
      }
    } catch {}
    newChatModels.set(scope, settings);
  }
  const settings = newChatModels.get(scope);
  // 用户要求 Claude 与 Codex 同样记住三项选择；旧版只保存模型的字符串记录仍可读取，实例之间不串用。
  return typeof settings === "string" ? { model: settings } : settings || {};
}
export function rememberNewChatSettings(patch) {
  if (!["codex", "claude"].includes(activeBackend())) return;
  const scope = newChatPreferenceScope();
  if (!scope) return;
  const settings = { ...rememberedNewChatSettings(), ...patch };
  newChatModels.set(scope, settings);
  try {
    localStorage.setItem(`codex-elves-new-chat-settings:${scope}`, JSON.stringify({ version: 1, ...settings, model: settings.model || "" }));
    if (settings.model) localStorage.setItem(`codex-elves-new-chat-model:${scope}`, settings.model);
  } catch {}
}
export function rememberNewChatModel(model) {
  rememberNewChatSettings({ model, reasoningEffort: currentEffort() });
}
export function codexCreationDefaultsKey() {
  return creating && session?.capabilities?.codexSessionDefaults === true && !isClaude()
    ? JSON.stringify([privateInputScope(session), nativeConnectionVersion, creationProject, selectionVersion]) : "";
}
export function codexCreationDefaults() {
  const key = codexCreationDefaultsKey();
  return key && codexCreationDefaultsState?.key === key ? codexCreationDefaultsState.value || null : null;
}
export function syncCodexCreationDefaults() {
  const key = codexCreationDefaultsKey();
  if (key && codexCreationDefaultsState?.key !== key && connected)
    void loadCodexCreationDefaults().catch(() => {});
}
export async function loadCodexCreationDefaults(force = false) {
  const key = codexCreationDefaultsKey();
  if (!key) return null;
  const previous = codexCreationDefaultsState;
  if (previous?.key === key) {
    if (previous.promise) return previous.promise;
    if (!force && previous.value) return previous.value;
    if (!force && previous.error) throw new Error(previous.error);
  }
  const record = { key, value: null, promise: null, error: "" };
  codexCreationDefaultsState = record;
  const current = () => codexCreationDefaultsState === record && codexCreationDefaultsKey() === key;
  const request = api("/api/read/newSessionDefaults", { backend: "codex", instanceId: activeInstanceId(), projectId: creationProject })
    .then(result => {
      if (!current()) return null;
      if (result?.ok !== true || typeof result.model !== "string" || !permissionModeKnown("codex", result.permissionMode)
          || !Array.isArray(result.permissionModes) || result.permissionModes.some(mode => !permissionModeKnown("codex", mode))
          || result.reasoningEffort != null && typeof result.reasoningEffort !== "string")
        throw new Error("Codex 新会话默认设置返回无效，请重新读取");
      record.value = result;
      return result;
    }).catch(error => {
      if (current()) { record.error = error.message; notice(`无法读取 Codex 新会话默认设置：${error.message}`, true); }
      throw error;
    }).finally(() => {
      record.promise = null;
      if (current()) updateComposer();
    });
  record.promise = request;
  return request;
}
export function newCodexPermissionMode() {
  const defaults = codexCreationDefaults();
  const remembered = rememberedNewChatSettings().permissionMode;
  return remembered && defaults?.permissionModes.includes(remembered) ? remembered : defaults?.permissionMode || "";
}
export function newClaudePermissionMode() {
  if (!creating || !isClaude()) return "";
  const catalog = instancePermissionCatalogs.get(activeInstanceId());
  if (!catalog || catalog.scope !== privateInputScope(session) || catalog.error || !Array.isArray(catalog.modes)) return "";
  const remembered = rememberedNewChatSettings().permissionMode;
  // 记忆不能越过目标 CLI 能力；只恢复当前菜单实际开放的模式，manual/default 视为同一选择。
  return permissionOptions("claude", catalog.modes).find(option => option.value === remembered
    || ["default", "manual"].includes(remembered) && ["default", "manual"].includes(option.value))?.value || "";
}
