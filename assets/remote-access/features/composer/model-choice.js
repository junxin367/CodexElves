// 当前模型选择：推导当前模型与思考深度，读写会话的 Fast 偏好；目录查询在 model-catalog.js。

import { $ } from "../../ui/dom.js";
import { composerTooltips } from "../../ui/tooltip.js";
import {
  busy, connected, creating, nativeConnectionVersion, selectedId, selectionVersion, session,
  stopping
} from "../../core/state.js";
import { draftKey } from "./drafts.js";
import { claudePendingSetting, modelContextKey, modelForId, modelIdentityMatches } from "./model-catalog.js";
import { codexCreationDefaults, rememberedNewChatModel, rememberedNewChatSettings } from "./new-chat-settings.js";
import {
  defaultModelEffort, modelDefaults, modelLoadError, models, modelsLoadedAt, modelsScope,
  remoteFastWrites, remoteFastWriting, selectedDraftFast, selectedEffort, selectedModel,
  setRemoteFastState, setRemoteFastWriting
} from "./options-state.js";
import { catalogUsable } from "./skill-catalog.js";
import { pending } from "../conversation/pending-state.js";
import { isClaude, privateInputScope, threadReadOnly } from "../session/capabilities.js";
import { activeInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import { currentThreadDisplay } from "../session/thread-cache.js";
import { threadDeletionLocked } from "../sidebar/deletion-state.js";

export function currentModelId() {
  return selectedModel || (selectedId ? claudePendingSetting("model")?.requested || currentThreadDisplay()?.model
    : creating && rememberedNewChatModel()?.model || codexCreationDefaults()?.model || modelDefaults.model) || "";
}
export function currentModel() { return modelForId(currentModelId()); }
export function currentEffort() {
  if (selectedEffort) return selectedEffort;
  const model = currentModelId();
  const display = currentThreadDisplay();
  if (selectedId) {
    const pending = claudePendingSetting("effort");
    if (pending && (!selectedModel || modelIdentityMatches(selectedModel, display?.model)))
      return pending.requested || "";
    return modelIdentityMatches(model, display?.model) ? display.reasoningEffort || "" : "";
  }
  if (creating) {
    const remembered = rememberedNewChatSettings(), catalog = currentModel();
    // 用户要求深度和模型成对记忆；旧模型失效、能力变化或切换模型后不能套用另一模型的等级。
    if (modelIdentityMatches(model, remembered.model) && (catalog?.supportedReasoningEfforts || [])
        .some(effort => effort.reasoningEffort === remembered.reasoningEffort)) return remembered.reasoningEffort;
    if (isClaude()) {
      if (remembered.reasoningEffort && modelIdentityMatches(model, remembered.model))
        return defaultModelEffort(catalog, modelIdentityMatches(model, modelDefaults.model) ? modelDefaults.reasoningEffort : "");
    } else {
      const defaults = codexCreationDefaults();
      if (defaults && modelIdentityMatches(model, defaults.model)) return defaults.reasoningEffort || defaultModelEffort(catalog);
      if (session?.capabilities?.codexSessionDefaults === true)
        return defaults || rememberedNewChatModel() ? defaultModelEffort(catalog) : "";
    }
  }
  return modelIdentityMatches(model, modelDefaults.model) ? modelDefaults.reasoningEffort
    : creating && !selectedModel ? defaultModelEffort(currentModel()) : "";
}
export function remoteFastKey() {
  // 用户要求 Codex 仅使用服务模式；Fast 必须由当前服务明确声明能力，并按会话保存。
  if (!session?.capabilities?.fastMode
      || session.capabilities.serviceFastMode !== true
      || isClaude() || !currentModelId() || !creating && !selectedId) return "";
  return JSON.stringify([privateInputScope(session), activeInstanceId(), draftKey(), currentModelId(),
    selectionVersion, nativeConnectionVersion]);
}
export function remoteFastContext() {
  const key = remoteFastKey();
  if (!key) return null;
  const instanceId = activeInstanceId();
  const owner = JSON.stringify([privateInputScope(session), instanceId, nativeConnectionVersion]);
  return { key, owner, instanceId,
    preferenceKey: creating ? null : JSON.stringify([owner, selectedId]) };
}
export function updateRemoteFast() {
  const control = $("#fast-button");
  if (!control) return;
  const key = remoteFastKey();
  control.hidden = !key;
  if (!key) return;
  const context = remoteFastContext();
  const model = currentModel() || models.find(model => model.id === currentModelId()
    || model.requestModel === currentModelId());
  const directoryReady = modelsScope === modelContextKey() && modelDefaults.fastModeOwner === context.owner
    && catalogUsable({ loadedAt: modelsLoadedAt });
  const availability = directoryReady ? modelDefaults.fastModeAvailability : null;
  // 用户要求服务模式与注入的 GPT 名称判定一致，不再依赖 API 模型目录声明 Fast；
  // requestModel 保留别名实际目标，不能因显示名像 GPT 就为 Claude 等模型开放。
  const alias = Object.entries(modelDefaults.fastModelAliases || {})
    .find(([name]) => name.trim().toLowerCase() === currentModelId().trim().toLowerCase());
  const requestModel = (typeof alias?.[1] === "string" ? alias[1] : null)
    ?? model?.requestModel ?? model?.request_model ?? model?.model ?? currentModelId();
  const serviceModelSupported = typeof requestModel === "string"
    && /^gpt-\d/i.test(requestModel.trim().split("/").at(-1) || "");
  const modelSupported = serviceModelSupported;
  // 用户要求选择不支持 Fast 的模型时隐藏按钮；沿用实际请求模型及目录能力判断，切回支持模型后恢复原偏好。
  control.hidden = !modelSupported;
  const preference = creating ? selectedDraftFast : currentThreadDisplay()?.fastMode;
  const preferenceKnown = typeof preference === "boolean";
  const supported = directoryReady && availability?.available === true && modelSupported && preferenceKnown;
  // 服务已保存的 Fast 在管理员随后禁用时仍允许关闭，避免后续发送因旧偏好一直受阻；不能借此开启。
  const canDisable = directoryReady && session.capabilities.serviceFastMode === true && preference === true;
  const state = setRemoteFastState({ key, supported, canDisable, enabled: supported && preference,
    reason: !directoryReady ? modelLoadError || "正在读取模型目录"
      : !modelSupported ? "当前请求模型不是 GPT，不能开启 Fast"
        : availability?.available !== true ? availability?.reason || "当前实例暂未提供 Fast 配置"
          : !preferenceKnown ? "正在读取会话设置" : "" });
  const operation = context.preferenceKey && remoteFastWrites.get(context.preferenceKey);
  if (operation) { operation.key = key; setRemoteFastWriting(operation); }
  const writing = !!operation || remoteFastWriting?.key === key;
  const enabled = state.enabled;
  control.setAttribute("aria-pressed", String(enabled));
  control.setAttribute("aria-busy", String(writing || !directoryReady
    || modelSupported && (availability?.available === null || availability?.available === true && !preferenceKnown)));
  control.setAttribute("aria-label", enabled ? "关闭本会话 Fast" : "开启本会话 Fast");
  // 用户要求编辑消息或队列草稿时也可选择 Fast；只锁实际保存、提交待确认及不可写状态，不以编辑标记禁用。
  control.disabled = (!state?.supported && !state?.canDisable) || !!writing || !connected || busy || stopping || !!pending
    || threadReadOnly() || threadDeletionLocked()
    || !sessionInstanceEnabled(activeInstanceId());
  // 用户要求 Fast 保持现有即时切换流程，不得套用模型/思考深度等待任务结束的规则；这里只展示会话开关状态。
  control.title = writing ? "正在保存本会话的 Fast 设置"
    : !supported && canDisable ? `${state.reason}；点击关闭本会话 Fast`
    : state.reason || (enabled ? "本会话 Fast 已开启，点击关闭"
      : "本会话 Fast 已关闭，点击开启");
  composerTooltips?.update?.(control, control.title);
}
