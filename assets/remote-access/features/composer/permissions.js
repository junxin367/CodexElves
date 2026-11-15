// 输入区权限选择：按实例读取可用权限模式、更新权限控件并提交修改；权限目录与每个会话、新建草稿的选择记录在 options-state.js。

import { permissionLabel, permissionOptions } from "../../permissions.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { setSelectOptions } from "../../ui/select.js";
import { mutate, updateComposer } from "../../core/actions.js";
import { busy, connected, creating, selectedId, session, stopping } from "../../core/state.js";
import { api } from "../api.js";
import { draftKey, snapshotDraft, storeDraft } from "./drafts.js";
import {
  codexCreationDefaults, newClaudePermissionMode, newCodexPermissionMode, rememberNewChatSettings,
  syncCodexCreationDefaults
} from "./new-chat-settings.js";
import {
  draftPermissionChoices, instancePermissionCatalogs, instancePermissionReads, permissionSelections,
  permissionWrites, rememberInstancePermissionCatalog, rememberPermissionSelection
} from "./options-state.js";
import { pending } from "../conversation/pending-state.js";
import { editingMessage } from "../messages/message-edit.js";
import { editingQueuedMessage } from "../queue/state.js";
import { activeBackend, isActiveThread, isClaude, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { activeInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import { relatedSessionView } from "../session/related-state.js";
import { currentThreadDisplay } from "../session/thread-cache.js";
import { threadDeletionLocked } from "../sidebar/deletion-state.js";

export async function loadInstancePermissionCatalog(id) {
  if (!id || !id.startsWith("claude:")) return;
  const scope = privateInputScope(session);
  const pending = instancePermissionReads.get(id);
  if (pending?.scope === scope) return pending;
  const cached = instancePermissionCatalogs.get(id);
  // 与 permissions.rs 的 CLI 帮助缓存保持同样的 60 秒上限；授权变化及读取错误不能复用旧能力。
  if (cached?.scope === scope && !cached.error && Date.now() - cached.loadedAt < 60000) return cached;
  const operation = api("/api/settings/permissions", { instanceId: id }).then(result => {
    if (scope !== privateInputScope(session)) return;
    if (result?.instanceId !== id || result.backend !== "claude"
        || !Array.isArray(result.supportedModes) || result.supportedModes.some(mode => typeof mode !== "string")
        || !result.settings || typeof result.settings !== "object" || Array.isArray(result.settings))
      throw new Error("实例权限读取结果不匹配，请重新读取。");
    rememberInstancePermissionCatalog(id, result);
  }).catch(error => {
    if (scope === privateInputScope(session)) {
      instancePermissionCatalogs.set(id, { scope, modes: [], defaultMode: "", error: error.message });
      notice(error.message, true);
    }
  }).finally(() => {
    if (instancePermissionReads.get(id) === operation) instancePermissionReads.delete(id);
    if (scope === privateInputScope(session) && activeInstanceId() === id) updateComposerPermissions();
  });
  operation.scope = scope;
  instancePermissionReads.set(id, operation);
  await operation;
}
export function updateComposerPermissions() {
  const control = $("#permission-button");
  if (!control) return;
  const display = currentThreadDisplay();
  // 用户要求侧边继承创建时的主会话权限且不显示权限按钮；加载中也按关联视图隐藏，避免短暂闪出入口。
  if (!creating && (relatedSessionView?.kind === "side" || display?.viewKind === "side" || display?.serviceSide === true)) {
    control.hidden = true;
    control.disabled = true;
    control.dataset.saving = "false";
    return;
  }
  if (!creating && display?.permissionMode)
    rememberPermissionSelection(selectedId, display.permissionMode, display.permissionRevision || 0);
  const saved = permissionSelections.get(selectedId);
  const codex = !isClaude();
  const nativeDefaultsEnabled = codex && creating && session?.capabilities?.codexSessionDefaults === true;
  if (nativeDefaultsEnabled) syncCodexCreationDefaults();
  const choice = creating ? draftPermissionChoices.get(draftKey())
      ?? (nativeDefaultsEnabled ? newCodexPermissionMode() : !codex ? newClaudePermissionMode() : undefined)
    : saved?.scope === privateInputScope(session) ? saved.mode : display?.permissionMode;
  const mode = choice || (creating && !nativeDefaultsEnabled ? "configured" : "");
  const catalogRecord = instancePermissionCatalogs.get(activeInstanceId());
  const catalog = catalogRecord?.scope === privateInputScope(session) ? catalogRecord : null;
  // 用户要求 Claude 新建沿用当前实例记住的权限；没有有效记忆时读取实例配置，已有会话仍显示自身权限。
  const effectiveMode = !codex && (!mode || mode === "configured")
    ? (!creating && display?.nativeSettings?.permissionMode) || catalog?.defaultMode || "" : mode;
  control.dataset.permissionBackend = codex ? "codex" : "claude";
  control.popupClassName = `composer-permissions-popup composer-${codex ? "codex" : "claude"}-permissions-popup`;
  // 两类菜单均以 340px 排列图标、说明和勾选；fitPopup 按视口两侧各 8px 收窄，手机入口仍为 28×36px。
  control.popupWidth = 340;
  // Claude 五行模式连同标题、说明约需 400px；预留到 440px，矮屏仍由 fitPopup 限高并滚动列表。
  control.popupMaxHeight = codex ? 360 : 440;
  control.popupTitle = codex ? "应如何批准 Codex 操作？" : "模式";
  control.popupHelp = codex ? { label: "了解更多", href: "https://developers.openai.com/codex/agent-approvals-security" } : null;
  control.popupNote = codex ? "" : "“绕过权限”仅在该实例 CLI 支持且已允许时显示";
  if (!codex && !catalog && connected && session?.capabilities?.sessionPermissions === true
      && sessionInstanceEnabled(activeInstanceId())) void loadInstancePermissionCatalog(activeInstanceId());
  let supported = codex ? nativeDefaultsEnabled ? codexCreationDefaults()?.permissionModes || null : null : catalog?.modes || [];
  if (isClaude() && !creating && supported) {
    // 共享 CLI 只能切换原生已开放的模式；启动参数或实例全局配置均未允许时不能由网页补开。
    const runtimeModes = Array.isArray(display?.permissionModes) ? display.permissionModes
      : ["default", "manual", "acceptEdits", "plan"];
    // 用户要求实例全局已将默认权限设为 bypassPermissions 时，视为该实例已明确允许绕过权限；
    // 这样已有会话也能显示该选项，后端会用同一实例配置再次校验，不能只靠前端放开。
    const instanceAllowsBypass = catalog?.defaultMode === "bypassPermissions";
    supported = supported.filter(mode => runtimeModes.includes(mode)
      || instanceAllowsBypass && mode === "bypassPermissions"
      || ["default", "manual"].includes(mode) && runtimeModes.some(value => ["default", "manual"].includes(value)));
  }
  const options = permissionOptions(activeBackend(), supported);
  // 旧只读 Codex 仍按原权限执行；Claude 的 manual/default 只选中同一项，不增加第六种模式。
  const displayMode = codex ? mode === "read-only" ? "auto" : mode || (nativeDefaultsEnabled ? "" : "configured")
    : options.find(option => option.value === effectiveMode
      || ["default", "manual"].includes(effectiveMode) && ["default", "manual"].includes(option.value))?.value || "";
  const signature = JSON.stringify([activeBackend(), displayMode, options]);
  if (control.dataset.permissionSignature !== signature) {
    control.dataset.permissionSignature = signature;
    setSelectOptions(control, options, displayMode);
  }
  control.dataset.permissionMode = displayMode;
  control.dataset.explicitPermission = String(creating && draftPermissionChoices.has(draftKey()));
  const saving = permissionWrites.get(selectedId)?.scope === privateInputScope(session);
  control.dataset.saving = String(saving);
  control.setAttribute("aria-busy", String(saving));
  control.hidden = session?.capabilities?.sessionPermissions !== true;
  const waiting = isActiveThread(), queued = (display?.queuedMessages || []).length > 0;
  control.disabled = !sessionInstanceEnabled(activeInstanceId()) || threadReadOnly() || !connected || busy
    || stopping || !!pending || !!editingMessage || !!editingQueuedMessage || threadDeletionLocked()
    || saving || !creating && !selectedId;
  const label = nativeDefaultsEnabled && !mode ? "权限"
    : permissionLabel(activeBackend(), codex ? choice : effectiveMode, creating);
  // 用户要求切换不能影响正常会话：选中值与 CLI 实际权限分别展示，待生效不冒充已切换，也不锁住发送。
  const permissionRecord = display?.nativeSettings?.changes?.permissionMode;
  const permissionChange = permissionRecord?.requested === mode ? permissionRecord : null;
  const nativePermission = display?.nativeSettings?.permissionMode;
  const desiredPermission = mode === "configured" ? catalog?.defaultMode : effectiveMode;
  const samePermission = nativePermission === desiredPermission
    || ["default", "manual"].includes(nativePermission) && ["default", "manual"].includes(desiredPermission);
  const permissionPending = !codex && !creating && (permissionChange?.state === "pending"
    || !!mode && !!desiredPermission && !!nativePermission && !samePermission);
  const selectedLabel = permissionLabel(activeBackend(), mode || effectiveMode, creating);
  const pendingDescription = `已选${selectedLabel}，待生效；当前原生权限：${nativePermission
    ? permissionLabel(activeBackend(), nativePermission, false) : "尚未确认"}。${permissionChange?.reason
    || "原生允许时自动切换，不中断任务或阻断消息。"}`;
  control.dataset.permissionPending = String(permissionPending);
  if (permissionPending) control.popupNote = pendingDescription;
  control.setAttribute("aria-label", permissionPending ? `选择会话权限，已选${selectedLabel}，待生效` : `选择会话权限，当前为${label}`);
  control.title = saving ? "正在保存会话权限"
    : permissionPending ? pendingDescription
    : queued ? `当前权限：${label}；已有待发消息按原选择执行，仅后续新轮次使用新选择`
    : waiting ? codex ? `当前权限：${label}；仅后续新轮次生效`
      : `当前权限：${label}；原生允许时切换，无法安全切换时保存待生效，不中断当前任务`
    : nativeDefaultsEnabled && !mode ? "正在读取 Codex 服务默认权限"
    : catalog?.error ? `实例权限读取失败：${catalog.error}；点击重新读取`
    : !codex && !supported.length ? "暂无法确认该 Claude CLI 支持的权限模式，点击重新读取"
    : `当前权限：${label}；仅此会话的后续新轮次生效`;
}
export async function changeComposerPermissions() {
  const control = $("#permission-button");
  if (!control || control.hidden || control.disabled) return;
  const mode = control.value;
  if (creating) {
    storeDraft(draftKey(), { ...snapshotDraft(), permissionMode: mode });
    // 用户要求两类实例都记住新建选择；按远程授权及实例保存，不修改原生全局配置。
    rememberNewChatSettings({ permissionMode: mode });
    updateComposerPermissions();
    return;
  }
  const id = selectedId, instanceId = activeInstanceId(), scope = privateInputScope(session);
  if (!id || permissionWrites.has(id)) return;
  const writing = { scope, mode };
  let finishPermissionWrite, saved = false;
  writing.completion = new Promise(resolve => { finishPermissionWrite = resolve; });
  permissionWrites.set(id, writing);
  updateComposer();
  try {
    // 权限保存断线时保留原回执并阻止后续发送，避免界面仍显示旧权限却按新权限执行。
    const result = await mutate("setPermissions", { threadId: id, instanceId, permissionMode: mode }, { reliable: true });
    if (scope !== privateInputScope(session)) return;
    if (result?.threadId !== id || result.permissionMode !== mode || result.instanceId !== instanceId)
      throw new Error("会话权限保存结果尚未确认，请重新读取会话后核对。");
    rememberPermissionSelection(id, mode, result.permissionRevision || 0);
    saved = true;
    notice(result.appliesTo === "immediate" ? "会话权限已生效。"
      : result.permissionPending === true ? "权限选择已保存，等待原生可安全切换时生效；当前会话可继续使用。"
      : "已保存，仅本会话的后续新轮次使用所选权限。");
  } catch (error) {
    if (scope === privateInputScope(session)) notice(error.message, true);
  } finally {
    if (permissionWrites.get(id) === writing) permissionWrites.delete(id);
    finishPermissionWrite(saved);
    if (scope === privateInputScope(session) && selectedId === id) updateComposer();
  }
}
