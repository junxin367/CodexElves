// 会话实例目录：实例加载、启用状态、新建会话的实例选择与 Claude 回顾默认值提示。

import { $, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { creating, selectedId, session, thread } from "../../core/state.js";
import { api } from "../api.js";
import { appearance, applyAppearance } from "../appearance/theme.js";
import { updateComposer } from "../composer/composer.js";
import {
  assignCreationBackend, creationInstanceId, setCreationBackend, setCreationInstanceId
} from "../composer/new-chat.js";
import { conversationBackend, newChatBackendOptions, privateInputScope } from "./capabilities.js";
import { currentThreadDisplay } from "./thread-cache.js";
import { sessionFilterValue } from "../sidebar/filters.js";
import { loadSidebar, projectVisibleThreadCounts, renderSidebar } from "../sidebar/list.js";

export let sessionInstances = [], sessionInstanceRequest = null;
export function setSessionInstances(value) { return sessionInstances = value; }
export function setSessionInstanceRequest(value) { return sessionInstanceRequest = value; }
let sessionTypes = [{ id: "codex", label: "Codex" }, { id: "claude", label: "Claude" }];
export function availableSessionInstances() {
  if (sessionInstances.length) return sessionInstances;
  // 用户要求实例显示名不拼接“本机”“WSL”；目录未返回时的默认名称也遵循同一规则。
  return [{ id: "codex:local", backend: "codex", label: "Codex", ready: true, legacyDefault: true },
    ...(session?.claudeInstanceId ? [{ id: session.claudeInstanceId, backend: "claude",
      label: ["Claude", session.claudeWslDistribution].filter(Boolean).join(" · "), ready: true, legacyDefault: true }] : [])];
}
export function conversationInstanceId(value) {
  if (value?.instanceId) return value.instanceId;
  const backend = conversationBackend(value);
  return availableSessionInstances().find(instance => instance.backend === backend
    && (value?.wslDistribution ? instance.distribution === value.wslDistribution
      && (!value.wslUser || instance.user === value.wslUser) : instance.legacyDefault))?.id
    || (backend === "codex" ? "codex:local" : "");
}
export function activeInstanceId() {
  const display = thread || (typeof currentThreadDisplay === "function" ? currentThreadDisplay() : null);
  return creating ? creationInstanceId : conversationInstanceId(display || { id: selectedId });
}
export function sessionInstanceEnabled(id, preferences = appearance) {
  if (preferences.sessionInstances && typeof preferences.sessionInstances === "object")
    return preferences.sessionInstances[id] === true;
  const instance = availableSessionInstances().find(value => value.id === id);
  return !!instance?.legacyDefault && preferences.sessionTypes?.[instance.backend] === true;
}
export function enabledInstanceIds() {
  // 所有环境统一按实例开关决定列表入口，不按发行版名称筛选。
  return availableSessionInstances().filter(instance => sessionInstanceEnabled(instance.id)).map(instance => instance.id);
}
function sessionBackendEnabled(backend) {
  return availableSessionInstances().some(instance => instance.backend === backend && sessionInstanceEnabled(instance.id));
}
function migrateSessionInstances(preferences, instances) {
  // 旧类型开关仅迁移到原默认实例；新发现的同类型实例保持关闭，各实例允许全部关闭。
  if (preferences.sessionInstances && typeof preferences.sessionInstances === "object") return false;
  preferences.sessionInstances = Object.fromEntries(instances.map(instance =>
    [instance.id, instance.legacyDefault === true && preferences.sessionTypes?.[instance.backend] === true]));
  return true;
}
function applySessionCatalog(result) {
  if (!Array.isArray(result.instances) || !Array.isArray(result.types)) return;
  sessionTypes = result.types.filter(kind => typeof kind.id === "string" && typeof kind.label === "string");
  sessionInstances = result.instances.filter(instance => typeof instance.id === "string"
    && typeof instance.label === "string" && sessionTypes.some(kind => kind.id === instance.backend));
  let changed = migrateSessionInstances(appearance, sessionInstances);
  const filter = sessionFilterValue();
  if (JSON.stringify(appearance.sessionFilter) !== JSON.stringify(filter)) {
    appearance.sessionFilter = filter; changed = true;
  }
  if (changed) applyAppearance(true);
  if (!sessionInstanceEnabled(creationInstanceId)) {
    const next = newChatBackendOptions()[0];
    if (next && creating) setCreationBackend(next.value);
    else if (next) { setCreationInstanceId(next.value); assignCreationBackend(next.backend); }
  }
  renderSessionInstances();
  if ($("#composer")) { renderSidebar(); updateComposer(); }
}
const claudeRecapDefaultWarnings = new Map();
function reportClaudeRecapDefaults(result, scope) {
  const warnings = [];
  for (const value of Array.isArray(result.claudeRecapDefaults) ? result.claudeRecapDefaults : []) {
    if (typeof value?.instanceId !== "string") continue;
    const key = JSON.stringify([scope, value.instanceId]);
    const message = typeof value.error === "string" ? value.error : "";
    if (!message) { claudeRecapDefaultWarnings.delete(key); continue; }
    if (claudeRecapDefaultWarnings.get(key) === message) continue;
    claudeRecapDefaultWarnings.set(key, message);
    const label = result.instances?.find(instance => instance.id === value.instanceId)?.label || value.instanceId;
    warnings.push(`${label} 的离开期间回顾未能默认关闭：${message}`);
  }
  if (warnings.length) notice(warnings.join("\n"), true);
}
export async function loadSessionInstances(force = false) {
  const scope = privateInputScope(session);
  if (sessionInstanceRequest && sessionInstanceRequest.deviceId === session?.device.id && sessionInstanceRequest.scope === scope)
    return sessionInstanceRequest;
  const deviceId = session?.device.id;
  const request = api("/api/settings/environments", { force }).then(result => {
    if (session?.device.id === deviceId && privateInputScope(session) === scope) {
      applySessionCatalog(result);
      reportClaudeRecapDefaults(result, scope);
    }
    return result;
  }).finally(() => { if (sessionInstanceRequest === request) sessionInstanceRequest = null; });
  request.deviceId = deviceId;
  request.scope = scope;
  sessionInstanceRequest = request;
  return request;
}
export function renderSessionInstances() {
  const root = $("#appearance-session-instances");
  if (!root) return;
  root.replaceChildren();
  for (const kind of sessionTypes) {
    const instances = availableSessionInstances().filter(instance => instance.backend === kind.id);
    if (!instances.length) continue;
    const group = element("section", "session-instance-group");
    group.append(element("h4", "", `${kind.label}（${instances.length}）`));
    for (const instance of instances) {
      const row = element("label", "appearance-notification-row session-instance-row");
      const copy = element("span", "appearance-environment-copy");
      copy.append(element("strong", "", instance.label), element("small", "muted", instance.status || "可接入"));
      if (instance.executable) copy.append(element("small", "muted session-instance-path", instance.executable));
      const control = element("input", "appearance-notification-switch");
      control.type = "checkbox"; control.setAttribute("role", "switch");
      control.dataset.instanceId = instance.id; control.setAttribute("aria-label", `启用 ${instance.label}`);
      control.checked = sessionInstanceEnabled(instance.id);
      control.onchange = () => {
        migrateSessionInstances(appearance, availableSessionInstances());
        appearance.sessionInstances = { ...appearance.sessionInstances, [instance.id]: control.checked };
        const filter = sessionFilterValue();
        if (filter !== "all") {
          // 关闭实例只移除该实例的筛选，保留其它多选项；全部移除后恢复“全部会话”。
          const selected = filter.filter(id => sessionInstanceEnabled(id));
          appearance.sessionFilter = selected.length ? selected : "all";
        }
        if (!sessionInstanceEnabled(creationInstanceId)) {
          const next = newChatBackendOptions()[0];
          if (next && creating) setCreationBackend(next.value);
          else if (next) { setCreationInstanceId(next.value); assignCreationBackend(next.backend); }
        }
        applyAppearance(true); projectVisibleThreadCounts.clear(); renderSidebar(); updateComposer();
        void loadSidebar(null, { fresh: true }).catch(error => notice(error.message, true));
      };
      row.append(copy, control); group.append(row);
    }
    root.append(group);
  }
}
