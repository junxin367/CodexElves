// 模型目录与选择：按实例/协议加载模型、思考深度、Fast 档位及其偏好记忆。

import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { composerTooltips } from "../../ui/tooltip.js";
import {
  busy, connected, creating, nativeConnectionVersion, selectedId, selectionVersion, session,
  stopping, thread
} from "../../core/state.js";
import { api } from "../api.js";
import { renderModelMenu } from "./composer-menus.js";
import { updateComposer } from "./composer.js";
import { draftKey } from "./drafts.js";
import {
  codexCreationDefaults, creationProject, rememberedNewChatModel, rememberedNewChatSettings
} from "./new-chat.js";
import {
  catalogFallbackMs, catalogNeedsRefresh, catalogRetryMs, catalogUsable, scheduleCatalogRefresh
} from "./skills.js";
import { reloadThread } from "../conversation/history.js";
import { mutate, pending } from "../conversation/pending.js";
import {
  activeBackend, isClaude, privateInputScope, threadReadOnly, validRelatedThreadId
} from "../session/capabilities.js";
import { activeInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import {
  cacheDisplayData, currentThreadDisplay, rememberThreadDisplay, threadDisplayKey, threadDisplays
} from "../session/thread-cache.js";
import { threadDeleted, threadDeletionLocked } from "../sidebar/actions.js";

export let modelsRetryAt = 0;
export function setModelsRetryAt(value) { return modelsRetryAt = value; }
export let selectedModel = "", selectedEffort = "";
export function setSelectedModel(value) { return selectedModel = value; }
export function setSelectedEffort(value) { return selectedEffort = value; }
export let modelChoiceVersion = 0;
export function setModelChoiceVersion(value) { return modelChoiceVersion = value; }
export let selectedDraftFast = false, remoteFastState = null, remoteFastWriting = null, fastModeVersion = 0;
export function setSelectedDraftFast(value) { return selectedDraftFast = value; }
export function setRemoteFastState(value) { return remoteFastState = value; }
export function setRemoteFastWriting(value) { return remoteFastWriting = value; }
export function setFastModeVersion(value) { return fastModeVersion = value; }
// 用户要求 Fast 复用模型目录和会话详情；这里只协调尚未完成的保存，不另建模型能力或会话开关缓存。
export const remoteFastWrites = new Map();
export let models = [], modelsPromise = null, modelsLoadedAt = 0;
export function setModels(value) { return models = value; }
export function setModelsPromise(value) { return modelsPromise = value; }
export function setModelsLoadedAt(value) { return modelsLoadedAt = value; }
// 服务模式改按实际 GPT 模型开放 Fast；旧缓存中的能力否定和缺失档位需重新读取。
export const modelCatalogCacheVersion = 4;
export let modelDefaults = { model: "", reasoningEffort: "" };
export function setModelDefaults(value) { return modelDefaults = value; }
export let modelsScope = "codex", modelLoadError = "";
function modelContext() {
  return { backend: activeBackend(), instanceId: activeInstanceId(),
    ...(selectedId ? { threadId: selectedId } : { projectId: creationProject }) };
}
export function modelContextKey() {
  // 用户要求同一会话实例共用模型目录；项目、历史会话和新建入口不能各建一份模型缓存。
  return activeInstanceId() === "codex:local" ? "codex" : activeInstanceId();
}
export function syncBackendModels() {
  if (!sessionInstanceEnabled(activeInstanceId())) return;
  const key = modelContextKey();
  if (key === modelsScope) return;
  modelsScope = key; models = []; modelsLoadedAt = 0; modelsPromise = null; modelsRetryAt = 0;
  modelDefaults = { model: "", reasoningEffort: "" }; modelLoadError = "";
  void loadModels().catch(error => {
    if (modelsScope === key) { modelLoadError = error.message; updateComposer(); }
  });
}
export async function loadModels(force = false, { includeAvailable = false } = {}) {
  if (!sessionInstanceEnabled(activeInstanceId())) return [];
  const scope = privateInputScope(session);
  const context = modelContext(), contextKey = modelContextKey();
  const connectionGeneration = nativeConnectionVersion;
  const current = () => scope && privateInputScope(session) === scope
    && (nativeConnectionVersion) === connectionGeneration && modelContextKey() === contextKey;
  // 缓存只保存实例的完整目录；打开选择器复用新鲜目录，过期时才显式读取可用模型。
  if (!force && catalogUsable({ loadedAt: modelsLoadedAt })
      && (!includeAvailable || !catalogNeedsRefresh({ loadedAt: modelsLoadedAt }))
      && !(modelDefaults.fastModeAvailability?.available === null && Date.now() >= modelsRetryAt)) {
    if (catalogNeedsRefresh({ loadedAt: modelsLoadedAt }) && Date.now() >= modelsRetryAt)
      void loadModels(true).catch(() => {});
    scheduleCatalogRefresh();
    return models;
  }
  if (modelsPromise) {
    if (!includeAvailable || modelsPromise.includeAvailable) return modelsPromise;
    // 自动历史读取尚未结束时补读完整目录；切换实例后不能替另一环境发起探测。
    await modelsPromise.catch(() => {});
    return current() ? loadModels(force, { includeAvailable: true }) : [];
  }
  const previousModels = models;
  const request = (async () => {
    const result = await api("/api/read/models", { ...context, forceReload: force, includeAvailable });
    if (!current()) return [];
    if (!Array.isArray(result.data) || !result.data.every(model => model && typeof model.model === "string"
        && (model.supportedReasoningEfforts == null || Array.isArray(model.supportedReasoningEfforts))))
      throw new Error("模型列表返回格式无效");
    // 历史当前模型属于会话展示，不是可用模型目录；不能缓存它或覆盖实例的全局默认值。
    if (result.catalogComplete === false) {
      modelsRetryAt = Date.now() + catalogRetryMs;
      return models;
    }
    // 用户要求两条路径各自决定 Ultra 可见性，网页不能绕过注入路径返回的关闭状态。
    const nextModels = result.data.map(model => ({
      ...model,
      supportedReasoningEfforts: (model.supportedReasoningEfforts || [])
        .filter(effort => result.ultraReasoningEnabled !== false || effort.reasoningEffort !== "ultra")
    }));
    models = nextModels;
    modelDefaults = { model: result.defaultModel || "", reasoningEffort: result.defaultReasoningEffort || "",
      nativeCapabilities: result.nativeCapabilities,
      // 模型能力、实例限制共用同一目录及刷新周期；身份标记防止旧授权或运行方式的展示结果串用。
      fastModeAvailability: result.fastModeAvailability, fastModelAliases: result.fastModelAliases,
      fastModeOwner: JSON.stringify([scope, context.instanceId, connectionGeneration]) };
    modelsLoadedAt = Date.now();
    modelsRetryAt = result.fastModeAvailability?.available === null ? modelsLoadedAt + catalogRetryMs : 0;
    modelLoadError = "";
    cacheDisplayData("models", contextKey === "codex" ? "global" : contextKey, { version: modelCatalogCacheVersion, models, defaults: modelDefaults, loadedAt: modelsLoadedAt },
      { maxAge: catalogFallbackMs });
    return models;
  })();
  request.includeAvailable = includeAvailable;
  modelsPromise = request;
  try { return await request; }
  catch (error) {
    if (current()) { modelsRetryAt = Date.now() + catalogRetryMs; modelLoadError = error.message; }
    throw error;
  }
  finally {
    if (modelsPromise === request) {
      modelsPromise = null; updateComposer();
      if (models !== previousModels && $("#model-settings") && !$("#model-settings").hidden) renderModelMenu();
      scheduleCatalogRefresh();
    }
  }
}
export function defaultModelEffort(model, preferred = "") {
  const efforts = (model?.supportedReasoningEfforts || []).filter(effort => effort.reasoningEffort !== "minimal");
  return efforts.find(value => value.reasoningEffort === preferred)?.reasoningEffort
    || efforts.find(value => value.reasoningEffort === model?.defaultReasoningEffort)?.reasoningEffort
    || efforts.find(value => value.reasoningEffort === "medium")?.reasoningEffort || efforts[0]?.reasoningEffort || "";
}
export function currentModelId() {
  return selectedModel || (selectedId ? claudePendingSetting("model")?.requested || currentThreadDisplay()?.model
    : creating && rememberedNewChatModel()?.model || codexCreationDefaults()?.model || modelDefaults.model) || "";
}
export function modelForId(id) {
  if (typeof id !== "string" || !id) return null;
  // 用户要求 Claude 完整模型 ID 与原生目录别名共用推理等级；只采用显式 resolvedModel，Codex 仍按原值精确匹配。
  return models.find(model => model.model === id)
    || (isClaude() && models.find(model => model.resolvedModel === id)) || null;
}
function modelIdentityMatches(left, right) {
  if (!left || !right) return false;
  if (left === right) return true;
  const model = modelForId(left);
  return !!model && model === modelForId(right);
}
export function claudePendingSetting(name) {
  if (!selectedId || !isClaude()) return null;
  const change = currentThreadDisplay()?.nativeSettings?.changes?.[name];
  return change && ["pending", "confirmation", "confirming"].includes(change.state) ? change : null;
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
function remoteFastContext() {
  const key = remoteFastKey();
  if (!key) return null;
  const instanceId = activeInstanceId();
  const owner = JSON.stringify([privateInputScope(session), instanceId, nativeConnectionVersion]);
  return { key, owner, instanceId,
    preferenceKey: creating ? null : JSON.stringify([owner, selectedId]) };
}
export function rememberRemoteFastPreference(id, enabled) {
  if (typeof enabled !== "boolean" || !validRelatedThreadId(id) || threadDeleted(id)) return;
  // 读取只会回填当前选中会话；后台其他会话的变更不能阻止它接收自己的新快照。
  if (id === selectedId) ++fastModeVersion;
  if (thread?.id === id) thread.fastMode = enabled;
  rememberThreadDisplay({ id, fastMode: enabled });
}
export function reconcileRemoteFastPreference(value, readVersion) {
  if (readVersion === fastModeVersion) return value;
  // 保存或实时事件比读取更新时，沿用同一会话已确认的开关，不能被迟到的会话快照覆盖。
  const display = threadDisplays.get(threadDisplayKey(value?.id));
  return typeof display?.fastMode === "boolean" ? { ...value, fastMode: display.fastMode } : value;
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
  const state = remoteFastState = { key, supported, canDisable, enabled: supported && preference,
    reason: !directoryReady ? modelLoadError || "正在读取模型目录"
      : !modelSupported ? "当前请求模型不是 GPT，不能开启 Fast"
        : availability?.available !== true ? availability?.reason || "当前实例暂未提供 Fast 配置"
          : !preferenceKnown ? "正在读取会话设置" : "" };
  const operation = context.preferenceKey && remoteFastWrites.get(context.preferenceKey);
  if (operation) { operation.key = key; remoteFastWriting = operation; }
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
export async function toggleRemoteFast() {
  const control = $("#fast-button"), key = remoteFastKey();
  if (!key || control?.hidden || control?.disabled || remoteFastState?.key !== key
      || !remoteFastState.supported && !remoteFastState.canDisable) return;
  if (creating) {
    selectedDraftFast = !selectedDraftFast;
    updateComposer();
    return;
  }
  const id = selectedId, model = currentModelId(), enabled = currentThreadDisplay()?.fastMode !== true;
  const context = remoteFastContext();
  if (remoteFastWrites.has(context.preferenceKey)) return;
  const operation = { key };
  remoteFastWrites.set(context.preferenceKey, operation);
  remoteFastWriting = operation;
  updateComposer();
  try {
    const result = await mutate("setFast", { ...modelContext(), threadId: id, model, enabled });
    if (remoteFastWrites.get(context.preferenceKey) !== operation
        || JSON.stringify([privateInputScope(session), context.instanceId, nativeConnectionVersion]) !== context.owner) return;
    if (result.threadId !== id || result.model !== model || result.supported !== true || result.enabled !== enabled)
      throw new Error("Fast 设置尚未确认，请重新读取会话");
    rememberRemoteFastPreference(id, enabled);
  } catch (error) {
    if (remoteFastContext()?.preferenceKey === context.preferenceKey) {
      notice(error.message || "Fast 设置保存失败", true);
      // 写入失败只核对现有目录和会话详情，不清空已确认状态，不恢复独立的 Fast 探测。
      void Promise.allSettled([loadModels(true), reloadThread()]);
    }
  } finally {
    if (remoteFastWrites.get(context.preferenceKey) === operation) remoteFastWrites.delete(context.preferenceKey);
    if (remoteFastWriting === operation) remoteFastWriting = null;
    updateComposer();
  }
}
