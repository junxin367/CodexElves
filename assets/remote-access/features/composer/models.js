// 模型目录加载与 Fast 切换：按实例/协议读取模型目录，切换当前会话的 Fast 档位；选项状态在 options-state.js，当前选择在 model-choice.js，目录查询与 Fast 偏好在 model-catalog.js。

import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { mutate, reloadThread, updateComposer } from "../../core/actions.js";
import { creating, nativeConnectionVersion, selectedId, session } from "../../core/state.js";
import { api } from "../api.js";
import { renderModelMenu } from "./composer-menus.js";
import { modelContext, modelContextKey, rememberRemoteFastPreference } from "./model-catalog.js";
import { currentModelId, remoteFastContext, remoteFastKey } from "./model-choice.js";
import {
  modelCatalogCacheVersion, modelDefaults, modelSettingsVersion, models, modelsLoadedAt, modelsPromise, modelsRetryAt,
  remoteFastState, remoteFastWrites, remoteFastWriting, selectedDraftFast, selectedEffort, setModelDefaults,
  setModelLoadError, setModels, setModelsLoadedAt, setModelsPromise, setModelsRetryAt,
  setRemoteFastWriting, setSelectedDraftFast, setSelectedEffort
} from "./options-state.js";
import { catalogFallbackMs, catalogNeedsRefresh, catalogRetryMs, catalogUsable } from "./skill-catalog.js";
import { scheduleCatalogRefresh } from "./skills.js";
import { privateInputScope } from "../session/capabilities.js";
import { activeInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import { cacheDisplayData, currentThreadDisplay } from "../session/thread-cache.js";

export async function loadModels(force = false, { includeAvailable = false } = {}) {
  if (!sessionInstanceEnabled(activeInstanceId())) return [];
  const scope = privateInputScope(session);
  const context = modelContext(), contextKey = modelContextKey();
  const connectionGeneration = nativeConnectionVersion;
  const settingsVersion = modelSettingsVersion;
  const current = () => scope && privateInputScope(session) === scope
    && (nativeConnectionVersion) === connectionGeneration && modelContextKey() === contextKey
    && modelSettingsVersion === settingsVersion;
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
      setModelsRetryAt(Date.now() + catalogRetryMs);
      return models;
    }
    // 用户要求由独立服务按实例决定 Ultra 可见性；网页不能绕过服务返回的关闭状态。
    const nextModels = result.data.map(model => ({
      ...model,
      supportedReasoningEfforts: (model.supportedReasoningEfforts || [])
        .filter(effort => result.ultraReasoningEnabled !== false || effort.reasoningEffort !== "ultra")
    }));
    // 其他客户端关闭 Ultra 后也清理未提交选择；历史会话中的原生等级仍按原快照展示。
    if (context.backend !== "claude" && result.ultraReasoningEnabled === false && selectedEffort === "ultra")
      setSelectedEffort("");
    setModels(nextModels);
    setModelDefaults({ model: result.defaultModel || "", reasoningEffort: result.defaultReasoningEffort || "",
      nativeCapabilities: result.nativeCapabilities,
      // 模型能力、实例限制共用同一目录及刷新周期；身份标记防止旧授权或运行方式的展示结果串用。
      fastModeAvailability: result.fastModeAvailability, fastModelAliases: result.fastModelAliases,
      fastModeOwner: JSON.stringify([scope, context.instanceId, connectionGeneration]) });
    setModelsLoadedAt(Date.now());
    setModelsRetryAt(result.fastModeAvailability?.available === null ? modelsLoadedAt + catalogRetryMs : 0);
    setModelLoadError("");
    cacheDisplayData("models", contextKey === "codex" ? "global" : contextKey, { version: modelCatalogCacheVersion, models, defaults: modelDefaults, loadedAt: modelsLoadedAt },
      { maxAge: catalogFallbackMs });
    return models;
  })();
  request.includeAvailable = includeAvailable;
  setModelsPromise(request);
  try { return await request; }
  catch (error) {
    if (current()) { setModelsRetryAt(Date.now() + catalogRetryMs); setModelLoadError(error.message); }
    throw error;
  }
  finally {
    if (modelsPromise === request) {
      setModelsPromise(null); updateComposer();
      if (models !== previousModels && $("#model-settings") && !$("#model-settings").hidden) renderModelMenu();
      scheduleCatalogRefresh();
    }
  }
}
export async function toggleRemoteFast() {
  const control = $("#fast-button"), key = remoteFastKey();
  if (!key || control?.hidden || control?.disabled || remoteFastState?.key !== key
      || !remoteFastState.supported && !remoteFastState.canDisable) return;
  if (creating) {
    setSelectedDraftFast(!selectedDraftFast);
    updateComposer();
    return;
  }
  const id = selectedId, model = currentModelId(), enabled = currentThreadDisplay()?.fastMode !== true;
  const context = remoteFastContext();
  if (remoteFastWrites.has(context.preferenceKey)) return;
  const operation = { key };
  remoteFastWrites.set(context.preferenceKey, operation);
  setRemoteFastWriting(operation);
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
    if (remoteFastWriting === operation) setRemoteFastWriting(null);
    updateComposer();
  }
}
