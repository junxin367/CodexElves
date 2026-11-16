// 输入区选项状态：模型目录与模型菜单搜索词、所选模型与思考等级、Fast 状态，以及按实例缓存的权限目录与选择。

import { permissionModeKnown } from "../../permissions.js";
import { session, thread } from "../../core/state.js";
import { claudeThreadId, privateInputScope } from "../session/capabilities.js";

// 会话权限按授权及真实会话保存；新建页的选择按实例/项目草稿隔离，不写入全局设置。
export const draftPermissionChoices = new Map(), permissionSelections = new Map(), permissionWrites = new Map();
export const instancePermissionCatalogs = new Map(), instancePermissionReads = new Map();
export function rememberPermissionSelection(id, mode, revision = 0) {
  const backend = claudeThreadId(id) ? "claude" : "codex";
  if (!id || !permissionModeKnown(backend, mode)) return;
  const scope = privateInputScope(session), previous = permissionSelections.get(id);
  if (previous?.scope === scope && previous.revision > revision) return;
  permissionSelections.set(id, { scope, mode, revision });
  if (thread?.id === id) {
    thread.permissionMode = mode;
    thread.permissionRevision = revision;
  }
}
export function rememberInstancePermissionCatalog(id, value) {
  if (value.backend !== "claude" || value.instanceId !== id || !Array.isArray(value.supportedModes)) return;
  instancePermissionCatalogs.set(id, { scope: privateInputScope(session), modes: value.supportedModes,
    defaultMode: typeof value.settings?.permissionMode === "string" ? value.settings.permissionMode : "",
    loadedAt: Date.now() });
}

export let modelQuery = "";
export function setModelQuery(value) { return modelQuery = value; }

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
// 用户要求默认隐藏 Ultra；旧目录缓存不能在设置页尚未读取时继续开放该等级。
export const modelCatalogCacheVersion = 5;
export let modelSettingsVersion = 0;
export function invalidateModelSettings() {
  modelSettingsVersion++;
  setModels([]); setModelsLoadedAt(0); setModelsPromise(null); setModelsRetryAt(0);
  setModelDefaults({ model: "", reasoningEffort: "" }); setModelLoadError("");
}
export let modelDefaults = { model: "", reasoningEffort: "" };
export function setModelDefaults(value) { return modelDefaults = value; }
export let modelsScope = "codex", modelLoadError = "";
export function setModelsScope(value) { return modelsScope = value; }
export function setModelLoadError(value) { return modelLoadError = value; }
export function defaultModelEffort(model, preferred = "") {
  const efforts = (model?.supportedReasoningEfforts || []).filter(effort => effort.reasoningEffort !== "minimal");
  return efforts.find(value => value.reasoningEffort === preferred)?.reasoningEffort
    || efforts.find(value => value.reasoningEffort === model?.defaultReasoningEffort)?.reasoningEffort
    || efforts.find(value => value.reasoningEffort === "medium")?.reasoningEffort || efforts[0]?.reasoningEffort || "";
}
