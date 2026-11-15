// 新建会话的选择操作：切换项目、后端与实例，并重置输入区选项；选择状态在 creation-state.js，记忆设置与 Codex 默认项在 new-chat-settings.js。

import { $ } from "../../ui/dom.js";
import { closePopup } from "../../ui/popup.js";
import { setSelectOptions } from "../../ui/select.js";
import { closeComposerMenus, renderApprovals, renderMessages, updateComposer } from "../../core/actions.js";
import { creating, selectionVersion, setSelectionToken, setSelectionVersion } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import {
  assignCreationBackend, creationInstanceId, creationProject, setCodexCreationDefaultsState,
  setCreationInstanceId, setCreationProject, syncNewChatProjectPicker
} from "./creation-state.js";
import { restoreDraft } from "./draft-restore.js";
import { attachmentDrafts, draftKey, drafts, rememberDraft, snapshotDraft, storeDraft } from "./drafts.js";
import { refreshNewChatBranch } from "./new-chat-settings.js";
import {
  modelChoiceVersion, setModelChoiceVersion, setModelQuery, setRemoteFastState,
  setSelectedDraftFast, setSelectedEffort, setSelectedModel
} from "./options-state.js";
import {
  globalSkillChoices, setAppliedGlobalSkills, setAppliedSkillCatalog, setMessageSkillLoadScope,
  setSelectedSkills, setSkillChoices, setSkillLoadVersion, setSkillLoading, setSkillRenderVersion,
  setSkillScope, skillLoadVersion, skillRenderVersion
} from "./skill-catalog.js";
import { scheduleCatalogRefresh } from "./skills.js";
import { setSelectedGoal } from "../goal/state.js";
import { backendAvailable, isClaude } from "../session/capabilities.js";
import { stream } from "../session/connection-state.js";
import { setConnection } from "../session/connection.js";
import { availableSessionInstances, newChatBackendOptions, sessionInstanceEnabled } from "../session/instances.js";
import { sessionTools } from "../shell-state.js";

export function changeCreationProject() {
  syncNewChatProjectPicker();
  const project = $("#project-select").value;
  if (!creating || project === creationProject) { void refreshNewChatBranch(); return; }
  rememberDraft();
  setCreationProject(project); setSelectionVersion(selectionVersion + 1); setSelectionToken(randomId());
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
  assignCreationBackend(instance.backend); setCreationInstanceId(instance.id);
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

export function resetComposerOptions() {
  setSelectedModel(setSelectedEffort(""));
  setCodexCreationDefaultsState(null);
  setModelChoiceVersion(modelChoiceVersion + 1);
  setSelectedDraftFast(false);
  setRemoteFastState(null);
  setModelQuery("");
  setSkillChoices(isClaude() ? [] : [...globalSkillChoices]); setSkillScope(""); setSkillLoading(null); setSkillLoadVersion(skillLoadVersion + 1);
  setMessageSkillLoadScope(""); setSkillRenderVersion(skillRenderVersion + 1);
  setAppliedSkillCatalog(setAppliedGlobalSkills(null));
  closeComposerMenus();
  scheduleCatalogRefresh();
}
