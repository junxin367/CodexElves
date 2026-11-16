// 同一服务的偏好由服务端共享；事件即时刷新，既有 15 秒会话轮询补齐漏事件，外观和草稿继续留在客户端。
import { createSidebarPreferenceStore } from "../../recovery-store.js";
import { permissionModeKnown } from "../../permissions.js";
import { getAndroidNotificationSettings, isAndroidContainer, setAndroidNotificationSettings } from "../../platform.js";
import { $ } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { loadModels, refreshSkillCatalog, renderSidebar, updateComposer } from "../../core/actions.js";
import { connected, session } from "../../core/state.js";
import { readCache } from "../../core/stores.js";
import { api } from "../api.js";
import { applyAppearance } from "../appearance/apply.js";
import { appearance, legacySharedAppearance, syncAppearanceThemeControls } from "../appearance/theme.js";
import { newChatModels } from "../composer/creation-state.js";
import { instancePermissionCatalogs, invalidateModelSettings, models } from "../composer/options-state.js";
import { privateInputScope } from "../session/capabilities.js";
import { applySessionPreferences, loadSessionInstances } from "../session/instance-catalog.js";
import { activeInstanceId, availableSessionInstances } from "../session/instances.js";
import { projectVisibleThreadCounts } from "../sidebar/list-state.js";
import { defaultSharedSettings, notifySharedSettings, sharedSettingsState as state } from "./shared-state.js";

let reading = null, nativeNotifications = Promise.resolve(), notificationsReady = false;
let writeQueue = Promise.resolve(), writing = null;
const generalKeys = ["conversationWidth", "projectThreadLimit", "sessionTypes", "sessionInstances", "sessionFilter"];
const idPattern = /^[a-z][a-z0-9:_-]{0,159}$/;
const object = value => !!value && typeof value === "object" && !Array.isArray(value);
function checkedSnapshot(value) {
  const settings = value?.settings;
  if (!Number.isSafeInteger(value?.revision) || value.revision < 0 || typeof value.epoch !== "string"
      || !value.epoch || !Number.isSafeInteger(value.instanceRevision) || value.instanceRevision < 0
      || !object(value.stored) || !object(settings)
      || !Number.isInteger(settings.conversationWidth) || settings.conversationWidth < 320 || settings.conversationWidth > 3840
      || ![5, 10, 20, 50].includes(settings.projectThreadLimit) || !object(settings.sessionTypes)
      || settings.sessionInstances !== null && !object(settings.sessionInstances)
      || settings.sessionFilter !== "all" && (!Array.isArray(settings.sessionFilter)
        || settings.sessionFilter.some(id => typeof id !== "string" || !idPattern.test(id)))
      || !object(settings.sidebar) || !object(settings.newChat) || !object(settings.notifications)
      || !["enabled", "approval", "question", "completed", "failed"].every(key => typeof settings.notifications[key] === "boolean")
      || settings.notifications.failed !== false)
    throw new Error("共享设置返回格式无效，请重新连接后核对。");
  return value;
}
function synchronizeNativeNotifications(scope) {
  if (!notificationsReady || !isAndroidContainer()) return Promise.resolve();
  const settings = { ...state.settings.notifications };
  nativeNotifications = nativeNotifications.catch(() => {}).then(async () => {
    if (scope !== privateInputScope(session) || JSON.stringify(state.settings.notifications) !== JSON.stringify(settings)) return;
    const previous = await getAndroidNotificationSettings();
    if (scope !== privateInputScope(session) || !previous?.supported
        || JSON.stringify(state.settings.notifications) !== JSON.stringify(settings)) return;
    if (JSON.stringify(previous.settings) !== JSON.stringify(settings)) await setAndroidNotificationSettings(settings);
  });
  return nativeNotifications;
}
function applySnapshot(snapshot, scope, detail = {}, expectedEpoch = state.epoch) {
  checkedSnapshot(snapshot);
  if (scope !== privateInputScope(session)) return false;
  if (state.scope === scope && state.epoch !== expectedEpoch) return false;
  const reset = state.scope !== scope || state.epoch !== snapshot.epoch;
  if (!reset && snapshot.revision < state.revision) return false;
  const recovered = !state.ready;
  const changed = reset || snapshot.revision !== state.revision;
  const nativeChanged = reset || snapshot.instanceRevision > state.instanceRevision;
  const oldInstances = JSON.stringify(state.settings.sessionInstances);
  const oldChoice = JSON.stringify(state.settings.newChat[activeInstanceId()]);
  const changedInstance = !reset && detail.instanceRevision === snapshot.instanceRevision
    && snapshot.instanceRevision === state.instanceRevision + 1 ? detail.instanceId : undefined;
  Object.assign(state, { scope, ready: true, revision: snapshot.revision, epoch: snapshot.epoch,
    instanceRevision: Math.max(reset ? -1 : state.instanceRevision, snapshot.instanceRevision),
    settings: snapshot.settings, stored: snapshot.stored });
  if (recovered) state.error = "";
  if (changed) {
    for (const key of generalKeys) appearance[key] = structuredClone(state.settings[key]);
    newChatModels.clear();
    projectVisibleThreadCounts.clear();
    applyAppearance(false);
    applySessionPreferences();
    if (oldInstances !== JSON.stringify(state.settings.sessionInstances))
      void loadSessionInstances().catch(() => {});
    syncAppearanceThemeControls?.();
    void synchronizeNativeNotifications(scope).catch(error => {
      if (scope === privateInputScope(session)) notice(`共享通知设置未能应用到当前设备：${error.message}`, true);
    });
  }
  if (nativeChanged) {
    const id = changedInstance;
    const instances = id ? [id] : availableSessionInstances().map(instance => instance.id);
    for (const target of instances) {
      instancePermissionCatalogs.delete(target);
      void readCache.remove(scope, "models", target === "codex:local" ? "global" : target).catch(() => {});
    }
    if (!id || id === activeInstanceId()) {
      invalidateModelSettings();
      if ($("#composer")) {
        updateComposer();
        void loadModels(true).catch(() => {});
        void refreshSkillCatalog().catch(() => {});
      }
    }
  }
  if (changed && $("#composer") && oldChoice !== JSON.stringify(state.settings.newChat[activeInstanceId()])) {
    const choice = state.settings.newChat[activeInstanceId()];
    const model = choice?.model && models.find(model => model.model === choice.model || model.resolvedModel === choice.model);
    if (choice?.model && (!model || choice.reasoningEffort && !(model.supportedReasoningEfforts || [])
        .some(effort => effort.reasoningEffort === choice.reasoningEffort)))
      void loadModels(true, { includeAvailable: true }).catch(() => {});
  }
  if (changed || nativeChanged || recovered) {
    notifySharedSettings({ ...detail, preferences: changed, instance: nativeChanged, instanceId: changedInstance });
    if ($("#sidebar-content")) renderSidebar();
    if ($("#composer")) updateComposer();
  }
  return true;
}
function legacyChoices(scope) {
  const result = {};
  try {
    for (let index = 0; index < Math.min(localStorage.length, 2048); index++) {
      const key = localStorage.key(index), prefix = `codex-elves-new-chat-settings:${scope}`;
      if (!key?.startsWith(prefix)) continue;
      const suffix = key.slice(prefix.length), id = suffix ? suffix.slice(1) : "codex:local";
      if (suffix && !suffix.startsWith(":") || !idPattern.test(id)) continue;
      let saved;
      try { saved = JSON.parse(localStorage.getItem(key) || "null"); } catch { continue; }
      if (saved?.version !== 1 || typeof saved.model !== "string" || saved.model.length > 200
          || /[\u0000-\u001f\u007f-\u009f]/.test(saved.model)) continue;
      const effort = typeof saved.reasoningEffort === "string" && saved.reasoningEffort.length <= 32
        && !/[\u0000-\u001f\u007f-\u009f]/.test(saved.reasoningEffort) ? saved.reasoningEffort : "";
      result[id] = { model: saved.model, reasoningEffort: effort,
        permissionMode: permissionModeKnown(id.split(":")[0], saved.permissionMode) ? saved.permissionMode : "" };
    }
    if (!result["codex:local"]) {
      const model = localStorage.getItem(`codex-elves-new-chat-model:${scope}`);
      if (model) result["codex:local"] = { model };
    }
  } catch {}
  return result;
}
async function initializeLegacy(scope) {
  const patch = legacySharedAppearance();
  try {
    const sidebar = createSidebarPreferenceStore().read(scope);
    if (Object.keys(sidebar).length) patch.sidebar = sidebar;
  } catch {}
  const newChat = legacyChoices(scope);
  if (Object.keys(newChat).length) patch.newChat = newChat;
  if (isAndroidContainer()) {
    try {
      const legacy = await getAndroidNotificationSettings();
      if (legacy?.supported) patch.notifications = legacy.settings;
    } catch (error) {
      if (scope === privateInputScope(session)) notice(`无法迁移当前设备的通知设置：${error.message}`, true);
    }
  }
  if (scope !== privateInputScope(session)) return;
  if (Object.keys(patch).length && !session.readOnly) {
    try { await saveSharedSettings(patch, { initialize: true }); }
    catch (error) { if (scope === privateInputScope(session)) notice(`旧设置迁移未完成：${error.message}`, true); }
  }
  if (scope === privateInputScope(session)) {
    notificationsReady = true; state.notificationsReady = true; notifySharedSettings();
    void synchronizeNativeNotifications(scope).catch(error => notice(`共享通知设置未能应用：${error.message}`, true));
  }
}
export function syncSharedSettings(value = session) {
  const scope = privateInputScope(value);
  if (!scope || value?.capabilities?.sharedSettings !== true) {
    Object.assign(state, { scope, ready: false, saving: false, notificationsReady: false, revision: -1, epoch: "", instanceRevision: -1,
      settings: defaultSharedSettings(), stored: {}, error: scope ? "当前服务不支持共享设置，请更新服务后修改。" : "" });
    notificationsReady = false;
    return;
  }
  const first = scope !== state.scope;
  try {
    if (first || state.epoch !== value.sharedSettings?.epoch) {
      reading = null; writing = null; writeQueue = Promise.resolve(); state.saving = false;
    }
    if (first) { notificationsReady = false; state.notificationsReady = false; state.error = ""; }
    applySnapshot(value.sharedSettings, scope);
    if (first) void initializeLegacy(scope);
  } catch (error) {
    state.ready = false; state.error = error.message;
    notifySharedSettings();
  }
}
export async function refreshSharedSettings(detail = {}) {
  const scope = privateInputScope(session);
  if (!scope || session?.capabilities?.sharedSettings !== true) return;
  // 一个页面只合并自己的读取；跨授权不复用，迟到快照按服务 epoch 与版本丢弃。
  if (reading?.scope === scope) {
    await reading.promise;
    if (detail.instanceRevision > state.instanceRevision || detail.preferenceRevision > state.revision)
      return refreshSharedSettings(detail);
    return;
  }
  const request = { scope, epoch: state.epoch, promise: null };
  request.promise = api("/api/settings/shared", undefined, undefined, undefined, { resourcePriority: "background" })
    .then(result => {
      if (result?.ok !== true) throw new Error("共享设置读取失败。");
      applySnapshot(result.snapshot, scope, detail, request.epoch);
    }).finally(() => { if (reading === request) reading = null; });
  reading = request;
  return request.promise;
}
export function receiveSharedSettings(params = {}) {
  const scope = privateInputScope(session);
  void refreshSharedSettings(params.kind === "instance"
    ? { instanceId: params.instanceId, instanceRevision: params.instanceRevision }
    : { preferenceRevision: params.revision })
    .catch(error => { if (scope === privateInputScope(session)) { state.error = error.message; notifySharedSettings(); } });
}
export function saveSharedSettings(patch, options = {}) {
  const scope = privateInputScope(session), epoch = state.epoch, value = structuredClone(patch);
  // 快速选择模型、深度或不同开关时按用户顺序保存，不丢弃保存期间的后续明确修改。
  const request = writeQueue.catch(() => {}).then(() => saveSharedSettingsNow(value, options, scope, epoch));
  writeQueue = request;
  return request;
}
async function saveSharedSettingsNow(patch, { initialize = false, base: suppliedBase, expectedRevision }, requestedScope, requestedEpoch) {
  const scope = privateInputScope(session);
  if (!scope || scope !== requestedScope || state.epoch !== requestedEpoch
      || state.scope !== scope || !state.ready || session?.readOnly || !initialize && !connected)
    throw new Error(state.error || "共享设置尚未就绪，或当前访问为只读、未连接。");
  // 含草稿的设置页保留开始编辑时的基线，避免其他端更新后用新版本号静默覆盖旧草稿。
  const base = structuredClone(suppliedBase ?? state.settings),
    revision = expectedRevision ?? state.revision, epoch = state.epoch;
  const operation = { scope, epoch }; writing = operation;
  let confirmed = false;
  state.saving = true; state.error = ""; notifySharedSettings();
  try {
    const result = await api("/api/settings/shared", { operation: initialize ? "initialize" : "save",
      patch, expectedRevision: revision, base });
    if (scope !== privateInputScope(session) || writing !== operation) return;
    if (result?.ok !== true) throw new Error("共享设置保存结果未确认，请重新读取核对。");
    applySnapshot(result.snapshot, scope, {}, epoch);
    confirmed = true;
    if (patch.notifications && notificationsReady) await nativeNotifications;
    return result.snapshot;
  } catch (error) {
    if (confirmed) error.sharedSaved = true;
    if (scope === privateInputScope(session) && writing === operation) {
      state.error = `${error.message} 未重复提交，正在核对服务端设置。`;
      try { await refreshSharedSettings(); } catch {}
    }
    throw error;
  } finally {
    if (scope === privateInputScope(session) && writing === operation) {
      writing = null; state.saving = false; notifySharedSettings(); syncAppearanceThemeControls?.();
    }
  }
}
