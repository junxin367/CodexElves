// 模型目录查询：按 ID 查找与比对模型、模型上下文键、同步后端模型目录、Claude 待生效设置，以及 Fast 偏好的记录与核对。

import { loadModels, updateComposer } from "../../core/actions.js";
import { selectedId, thread } from "../../core/state.js";
import { creationProject } from "./creation-state.js";
import {
  fastModeVersion, models, modelsScope, setFastModeVersion, setModelDefaults, setModelLoadError,
  setModels, setModelsLoadedAt, setModelsPromise, setModelsRetryAt, setModelsScope
} from "./options-state.js";
import { activeBackend, isClaude, validRelatedThreadId } from "../session/capabilities.js";
import { activeInstanceId, sessionInstanceEnabled } from "../session/instances.js";
import { currentThreadDisplay, threadDisplayKey, threadDisplays } from "../session/thread-cache.js";
import { rememberThreadDisplay } from "../session/thread-display.js";
import { threadDeleted } from "../sidebar/deletion-state.js";
export { modelForId, modelIdentityMatches } from "./model-identity.js";

export function modelContext() {
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
  setModelsScope(key); setModels([]); setModelsLoadedAt(0); setModelsPromise(null); setModelsRetryAt(0);
  setModelDefaults({ model: "", reasoningEffort: "" }); setModelLoadError("");
  void loadModels().catch(error => {
    if (modelsScope === key) { setModelLoadError(error.message); updateComposer(); }
  });
}
export function claudePendingSetting(name) {
  if (!selectedId || !isClaude()) return null;
  const change = currentThreadDisplay()?.nativeSettings?.changes?.[name];
  return change && ["pending", "confirmation", "confirming"].includes(change.state) ? change : null;
}
export function rememberRemoteFastPreference(id, enabled) {
  if (typeof enabled !== "boolean" || !validRelatedThreadId(id) || threadDeleted(id)) return;
  // 读取只会回填当前选中会话；后台其他会话的变更不能阻止它接收自己的新快照。
  if (id === selectedId) setFastModeVersion(fastModeVersion + 1);
  if (thread?.id === id) thread.fastMode = enabled;
  rememberThreadDisplay({ id, fastMode: enabled });
}
export function reconcileRemoteFastPreference(value, readVersion) {
  if (readVersion === fastModeVersion) return value;
  // 保存或实时事件比读取更新时，沿用同一会话已确认的开关，不能被迟到的会话快照覆盖。
  const display = threadDisplays.get(threadDisplayKey(value?.id));
  return typeof display?.fastMode === "boolean" ? { ...value, fastMode: display.fastMode } : value;
}
