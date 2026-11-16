// 实例目录加载：读取并应用会话实例目录，渲染实例列表。

import { $, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { loadSidebar, renderSidebar, updateComposer } from "../../core/actions.js";
import { creating, session } from "../../core/state.js";
import { api } from "../api.js";
import { applyAppearance } from "../appearance/apply.js";
import { appearance } from "../appearance/theme.js";
import { assignCreationBackend, creationInstanceId, setCreationInstanceId } from "../composer/creation-state.js";
import { setCreationBackend } from "../composer/new-chat.js";
import { privateInputScope } from "./capabilities.js";
import {
  availableSessionInstances, migrateSessionInstances, newChatBackendOptions,
  reportClaudeRecapDefaults, sessionInstanceEnabled, sessionInstanceRequest, sessionInstances,
  sessionTypes, setSessionInstanceRequest, setSessionInstances, setSessionTypes
} from "./instances.js";
import { sessionFilterValue } from "../sidebar/filters.js";
import { projectVisibleThreadCounts } from "../sidebar/list-state.js";

function applySessionCatalog(result) {
  if (!Array.isArray(result.instances) || !Array.isArray(result.types)) return;
  setSessionTypes(result.types.filter(kind => typeof kind.id === "string" && typeof kind.label === "string"));
  setSessionInstances(result.instances.filter(instance => typeof instance.id === "string"
    && typeof instance.label === "string" && sessionTypes.some(kind => kind.id === instance.backend)));
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
  }).finally(() => { if (sessionInstanceRequest === request) setSessionInstanceRequest(null); });
  request.deviceId = deviceId;
  request.scope = scope;
  setSessionInstanceRequest(request);
  return request;
}
export function setInstanceDisplay(id, enabled) {
  if (typeof enabled !== "boolean" || !availableSessionInstances().some(instance => instance.id === id)) return;
  // 用户要求“在本设备显示”只管理当前设备入口；复用原有实例切换和筛选流程，不停止原生任务。
  migrateSessionInstances(appearance, availableSessionInstances());
  appearance.sessionInstances = { ...appearance.sessionInstances, [id]: enabled };
  const filter = sessionFilterValue();
  if (filter !== "all") {
    const selected = filter.filter(value => sessionInstanceEnabled(value));
    appearance.sessionFilter = selected.length ? selected : "all";
  }
  if (!sessionInstanceEnabled(creationInstanceId)) {
    const next = newChatBackendOptions()[0];
    if (next && creating) setCreationBackend(next.value);
    else if (next) { setCreationInstanceId(next.value); assignCreationBackend(next.backend); }
  }
  applyAppearance(true); projectVisibleThreadCounts.clear(); renderSidebar(); updateComposer();
  void loadSidebar(null, { fresh: true }).catch(error => notice(error.message, true));
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
        setInstanceDisplay(instance.id, control.checked);
      };
      row.append(copy, control); group.append(row);
    }
    root.append(group);
  }
}
