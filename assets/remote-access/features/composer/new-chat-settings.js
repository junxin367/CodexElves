// 新建会话的记忆设置与默认项：按授权记住后端、实例与模型，Codex 默认项的读取与同步，新会话权限模式，分支读取与选中新建的会话。

import { permissionModeKnown, permissionOptions } from "../../permissions.js";
import { setWorkspaceBranch } from "../../session-files.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { saveSharedSettings, selectThread, updateComposer } from "../../core/actions.js";
import {
  connected, creating, nativeConnectionVersion, selectionToken, selectionVersion, session
} from "../../core/state.js";
import { api } from "../api.js";
import {
  codexCreationDefaultsState, creationBranchRead, creationBranchSnapshot, creationProject,
  resetNewChatBranch, setCodexCreationDefaultsState, setCreationBranchRead,
  setCreationBranchSnapshot
} from "./creation-state.js";
import { deleteDraft, draftKey, snapshotDraft, storeDraft } from "./drafts.js";
import { modelForId } from "./model-catalog.js";
import { instancePermissionCatalogs, modelSettingsVersion } from "./options-state.js";
import { activeBackend, isClaude, privateInputScope } from "../session/capabilities.js";
import { activeInstanceId } from "../session/instances.js";
import { sharedSettingsState } from "../settings/shared-state.js";

export function refreshNewChatBranch() {
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
  setCreationBranchRead(record);
  setWorkspaceBranch(badge, creationBranchSnapshot?.key === key ? creationBranchSnapshot.git : null, "读取分支…");
  const current = () => creationBranchRead === record && !record.controller.signal.aborted
    && creating && session && $("#new-chat-branch") === badge
    && JSON.stringify([privateInputScope(session), nativeConnectionVersion ?? null, creationProject, selectionToken]) === key;
  record.promise = api("/api/workspace/context", { projectId: creationProject }, undefined, record.controller.signal)
    .then(result => {
      if (!current()) return;
      const git = result?.ok === true && result.git && typeof result.git === "object"
        ? result.git : { unavailable: true };
      setCreationBranchSnapshot({ key, git, loadedAt: Date.now() });
      setWorkspaceBranch(badge, git);
    })
    .catch(error => {
      if (!current() || error.name === "AbortError") return;
      setCreationBranchSnapshot({ key, git: { unavailable: true }, loadedAt: Date.now() });
      setWorkspaceBranch(badge, creationBranchSnapshot.git);
    })
    .finally(() => { if (creationBranchRead === record) setCreationBranchRead(null); });
  return record.promise;
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
  // 用户要求多端共用模型、思考与权限记忆；仅使用当前授权服务的快照，旧本地值由首次迁移单独处理。
  if (sharedSettingsState.ready && sharedSettingsState.scope === privateInputScope(session))
    return sharedSettingsState.settings.newChat[activeInstanceId()] || {};
  return {};
}
export function rememberNewChatSettings(patch) {
  if (!["codex", "claude"].includes(activeBackend())) return;
  const scope = newChatPreferenceScope();
  if (!scope) return;
  const owner = privateInputScope(session);
  void saveSharedSettings({ newChat: { [activeInstanceId()]: patch } }).catch(error => {
    if (owner === privateInputScope(session)) notice(`新会话偏好未能共享：${error.message}`, true);
  });
}
export function codexCreationDefaultsKey() {
  // 实例模型设置变更后，新建默认值和在途旧读取同时失效，不能继续显示旧模型或已关闭的 Ultra。
  return creating && session?.capabilities?.codexSessionDefaults === true && !isClaude()
    ? JSON.stringify([privateInputScope(session), nativeConnectionVersion, creationProject, selectionVersion, modelSettingsVersion]) : "";
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
  setCodexCreationDefaultsState(record);
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
