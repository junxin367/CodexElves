// 用户确认的实例设置：共享的显示开关与原生常规配置分开；模型、思考和扩展页面共用实例选择及请求隔离。
import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { selectControl, setSelectOptions } from "../../ui/select.js";
import { connected, session } from "../../core/state.js";
import { readCache } from "../../core/stores.js";
import { loadModels, refreshSkillCatalog, updateComposer } from "../../core/actions.js";
import { api } from "../api.js";
import { privateInputScope } from "../session/capabilities.js";
import { activeInstanceId, availableSessionInstances, sessionInstanceEnabled } from "../session/instances.js";
import { loadSessionInstances, setInstanceDisplay } from "../session/instance-catalog.js";
import { invalidateModelSettings, selectedEffort, setSelectedEffort } from "../composer/options-state.js";
import { effortLabels } from "../composer/prompt-editor.js";
import { sharedSettingsWritable, subscribeSharedSettings } from "./shared-state.js";
import { notice } from "../../ui/notice.js";

const sections = {
  general: "会话实例", models: "模型与思考", skills: "Skills", mcp: "MCP", hooks: "钩子"
};
// 用户要求后续可扩展设置；导航分组、名称和图标集中登记，新增页面不再复制整段侧栏模板。
export const settingsNavigation = [
  { id: "personal", label: "个人", items: [
    { id: "general", label: "常规", icon: "settings" },
    { id: "customization", label: "外观", icon: "sun" },
    { id: "shortcuts", label: "快捷键", icon: "keyboard" }
  ] },
  { id: "agents", label: "智能体", items: [
    { id: "types", label: "会话实例", icon: "allSessions" },
    { id: "models", label: "模型与思考", icon: "model" },
    { id: "naming", label: "会话命名", icon: "edit" },
    { id: "permissions", label: "权限", icon: "shield" },
    { id: "instructions", label: "自定义指令", icon: "edit" }
  ] },
  { id: "extensions", label: "扩展", items: [
    { id: "skills", label: "Skills", icon: "puzzle" },
    { id: "mcp", label: "MCP", icon: "network" },
    { id: "hooks", label: "钩子", icon: "hook" }
  ] },
  { id: "tools", label: "设备工具", androidOnly: true, items: [
    { id: "notifications", label: "通知事件", icon: "globe" },
    { id: "diagnostics", label: "诊断日志", icon: "tools" }
  ] }
];
export const instanceSettingPages = [
  { id: "types", label: "会话实例", section: "general", description: "按实例管理常规能力和原生配置。" },
  { id: "models", label: "模型与思考", section: "models", description: "设置所选实例的默认模型与默认思考等级。" },
  { id: "skills", label: "Skills", section: "skills", description: "查看和管理所选实例的 Skills。" },
  { id: "mcp", label: "MCP", section: "mcp", description: "查看和管理所选实例的 MCP 服务。" },
  { id: "hooks", label: "钩子", section: "hooks", description: "查看和管理所选实例的原生钩子。" }
];
function brand(backend) {
  // 用户要求保留现有 Codex 结形、Claude 橙色星芒；实例选择器和页标题均直接复用现有 icon()。
  const node = icon(backend);
  node.classList.add("instance-settings-brand");
  node.dataset.backend = backend;
  return node;
}
function fieldCopy(label, description) {
  const copy = element("span", "appearance-field-copy");
  copy.append(element("span", "", label));
  if (description) copy.append(element("small", "", description));
  return copy;
}
function switchControl(label, value) {
  const control = element("input", "appearance-notification-switch");
  // 缺少显式值时使用原生默认；使用原生混合态及对应无障碍语义，不能读成“关闭”。
  control.type = "checkbox"; control.setAttribute("role", value == null ? "checkbox" : "switch");
  control.setAttribute("aria-label", value == null ? `${label}，使用原生默认` : label);
  control.checked = value === true; control.indeterminate = value == null;
  return control;
}
function section(parent, label, backend = null) {
  const node = element("section", "appearance-section");
  const heading = element("h4", "instance-settings-section-heading");
  if (backend) heading.append(brand(backend));
  heading.append(document.createTextNode(label));
  const card = element("div", "appearance-card");
  node.append(heading, card); parent.append(node);
  return card;
}
// 用户要求默认状态统一口径；实例原生配置的空值称“使用原生默认”，模型能力的默认值单独注明来源。
const inherit = { value: "", label: "使用原生默认" };
const booleanOptions = [inherit, { value: "true", label: "开启" }, { value: "false", label: "关闭" }];
const compactOptions = [inherit, { value: "auto", label: "自动" },
  ...[100000, 200000, 400000, 600000, 800000, 1000000].map(value =>
    ({ value: String(value), label: `${value / 1000}k Token` }))];
const boolValue = value => value == null ? "" : String(value);
const apiValue = value => value === "" ? null : value === "true" ? true : value === "false" ? false : value;

export function mountInstanceSettingsPage({ page, section: name, getSelectedInstance = () => "",
  setSelectedInstance = () => {} }) {
  const scope = privateInputScope(session);
  let disposed = false, selected = "", record = null, detecting = false, loading = false, saving = false, needsRead = false;
  let generation = 0, readController = null, message = "", externalRefresh = false;
  const alive = () => !disposed && page.isConnected && scope === privateInputScope(session);
  const writable = () => alive() && connected && session && !session.readOnly && !record?.readOnly
    && !detecting && !loading && !saving && !needsRead && !!record
    && !!currentInstance() && !currentInstance().error && record.backend === currentInstance().backend;
  const currentInstance = () => availableSessionInstances().find(instance => instance.id === selected);
  function retireRead() {
    generation++; readController?.abort(); readController = null; loading = false;
    return generation;
  }
  const head = element("div", "appearance-card instance-settings-target");
  const targetRow = element("div", "appearance-field");
  const instances = selectControl(`settings-${name}-instance`,
    name === "general" ? "当前会话实例" : `${sections[name]}的会话实例`, [], "", "",
    option => [option?.icon ? brand(option.icon) : icon("allSessions"),
      element("span", "select-label", option?.label || "请选择会话实例"), icon("chevron")]);
  targetRow.append(element("span", "", "会话实例"), instances); head.append(targetRow);
  const display = switchControl("显示此实例", false);
  if (name === "general") {
    const row = element("label", "appearance-field");
    row.append(fieldCopy("显示此实例", "所有客户端共用此开关，不会停止正在执行的任务。"), display);
    head.append(row);
  }
  const detect = button("重新检测实例", "重新检测会话实例", () => void show(true), "text-button");
  const content = element("div", "instance-settings-content");
  const status = element("p", "appearance-hint instance-settings-status");
  status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  const reload = button("重新读取", "重新读取当前实例设置", () => void read(), "text-button");
  const actions = element("div", "appearance-action-buttons instance-settings-actions");
  actions.append(reload);
  page.append(head, detect, content, status, actions);
  const controls = new Set();
  function sync() {
    instances.disabled = !alive() || detecting || saving || !availableSessionInstances().length;
    detect.disabled = !alive() || detecting || saving || !session || !connected;
    reload.disabled = detect.disabled || loading || !currentInstance() || !!currentInstance().error;
    display.checked = sessionInstanceEnabled(selected);
    display.indeterminate = !currentInstance();
    // 显示开关来自服务端共享偏好，不等待原生配置读取；实例检测或保存期间仍锁定目标。
    display.disabled = !alive() || detecting || saving || !currentInstance()
      || !connected || !sharedSettingsWritable(session);
    status.textContent = detecting ? "正在检测会话实例…" : loading ? "正在读取实例设置…" : saving ? "正在保存…" : message;
    for (const control of controls) control.disabled = !writable() || control.settingsUnsupported === true;
  }
  function addSwitch(card, key, label, description, value, unsupported = false) {
    const row = element("label", "appearance-field");
    const control = switchControl(label, value);
    control.id = `settings-${name}-${key}`; control.settingsUnsupported = unsupported;
    const suffix = value == null ? " 当前使用原生默认设置。" : "";
    row.append(fieldCopy(label, `${description || ""}${suffix}`), control);
    control.onchange = () => void save({ [key]: control.checked });
    controls.add(control); card.append(row);
  }
  function addSelect(card, key, label, description, options, value, transform = apiValue, unsupported = false) {
    const row = element("div", "appearance-field");
    if (!options.some(option => option.value === value))
      options = [...options, { value, label: `${value}（当前配置）`, disabled: true }];
    const control = selectControl(`settings-${name}-${key}`, label, options, value);
    control.settingsUnsupported = unsupported;
    control.onchange = () => {
      const settings = { [key]: transform(control.value) };
      if (name === "models" && key === "model" && record.backend === "codex") {
        // 切换 Codex 实例默认模型时恢复模型默认思考等级，避免把旧模型的专属等级带给新模型。
        settings.reasoningEffort = null;
      }
      void save(settings);
    };
    row.append(fieldCopy(label, description), control); controls.add(control); card.append(row);
  }
  function renderGeneral() {
    const backend = record.backend, values = record.settings;
    const card = section(content, `${backend === "claude" ? "Claude" : "Codex"} 常规设置`, backend);
    if (backend === "codex") {
      for (const [key, label, description] of [
        ["todo", "任务清单（Todo）", "允许 Codex 创建任务清单并更新任务进度。"],
        ["memories", "记忆", "在后续会话中使用实例已有的记忆。"],
        ["multiAgent", "子智能体", "允许 Codex 调用子智能体处理任务。"],
        ["goals", "目标模式", "为持续任务设置目标并跟踪进度。"]
      ]) addSwitch(card, key, label, description, values[key]);
      content.append(element("p", "appearance-hint",
        "仅修改所选实例的原生配置，新会话或重新加载后生效。"));
    } else {
      addSwitch(card, "awaySummaryEnabled", "离开期间回顾",
        "离开后返回时自动回顾会话进展；关闭后保留已有回顾。", values.awaySummaryEnabled);
      addSelect(card, "promptSuggestionEnabled", "输入建议", "根据会话进展提供后续输入建议。",
        booleanOptions, boolValue(values.promptSuggestionEnabled), apiValue,
        record.capabilities?.promptSuggestions !== true);
      addSelect(card, "autoCompactWindow", "自动压缩窗口", "达到窗口阈值时自动压缩上下文。",
        compactOptions, values.autoCompactWindow == null ? "" : String(values.autoCompactWindow),
        value => value === "" ? null : value === "auto" ? "auto" : Number(value),
        record.capabilities?.autoCompactWindow !== true);
      content.append(element("p", "appearance-hint",
        "回顾和输入建议保存在所选实例的原生配置；自动压缩窗口用于此实例后续新建的远程会话。"));
      if (record.capabilities?.promptSuggestions !== true || record.capabilities?.autoCompactWindow !== true)
        content.append(element("p", "appearance-hint",
          record.capabilityError
            ? `无法核对当前 Claude CLI 能力：${record.capabilityError} 输入建议与压缩窗口暂不可修改。`
            : "该实例的 Claude CLI 未提供部分设置参数，对应控件暂不可用。"));
    }
    content.append(element("p", "appearance-hint", "项目设置、启动参数和组织策略仍按原生优先级生效。"));
  }
  function renderModels() {
    const values = record.settings;
    const card = section(content, "实例默认值");
    const models = record.models || [];
    addSelect(card, "model", "默认模型", record.backend === "codex" ? "切换默认模型后，默认思考等级恢复为“使用模型默认”。" : "",
      [inherit,
        ...models.map(model => ({ value: model.model, label: model.displayName || model.model }))],
      values.model || "", value => value === "" ? null : value);
    const modelId = values.model || record.effectiveModel;
    // 已配置的模型退出目录时不能借用另一个模型的能力；仍允许用户重选模型或移除失效覆盖。
    const model = modelId ? models.find(model => model.model === modelId) : models.find(model => model.isDefault);
    const efforts = (model?.supportedReasoningEfforts || [])
      .filter(effort => effort.reasoningEffort !== "ultra" || values.ultraReasoningEnabled === true);
    if (record.backend === "codex") addSelect(card, "reasoningEffort",
        "默认思考等级", "可用等级由当前模型能力决定。",
        [{ value: "", label: "使用模型默认" }, ...efforts.map(effort =>
          ({ value: effort.reasoningEffort, label: effortLabels[effort.reasoningEffort] || effort.reasoningEffort }))],
        values.reasoningEffort || "", apiValue, !model);
    else content.append(element("p", "appearance-hint",
      "尚未确认当前 Claude CLI 的默认思考等级配置，请在会话输入区选择。"));
    if (!model) content.append(element("p", "appearance-hint",
      "当前模型未出现在实例目录中，请选择可用模型或选择“使用原生默认”。"));
    // 实例默认值与输入区记住的选择是不同来源；明确既有优先级，不能把此处描述为覆盖全部新会话选择。
    content.append(element("p", "appearance-hint",
      "新会话优先使用输入区已记住且仍可用的模型与思考等级，没有可用选择时使用实例默认值。已有会话保留各自设置。"));
    if (record.backend === "codex") {
      const reasoning = section(content, "思考等级");
      addSwitch(reasoning, "ultraReasoningEnabled", "开放 Ultra 思考等级",
        "默认关闭；开启后，仅在支持的 Codex 模型中显示 Ultra。", values.ultraReasoningEnabled);
    }
  }
  function renderInventory() {
    const inventory = record.inventory || {};
    if (name === "hooks") {
      const card = section(content, "钩子设置");
      addSwitch(card, "enabled", "开启钩子", "按所选实例的原生钩子配置执行。", inventory.enabled);
    }
    if (inventory.message) content.append(element("p", "appearance-hint", inventory.message));
    const rows = Array.isArray(inventory.data) ? inventory.data : [];
    if (!rows.length) {
      content.append(element("p", "appearance-hint", inventory.error || `所选实例尚未返回${sections[name]}。`));
      return;
    }
    const card = section(content, `所选实例的 ${sections[name]}`);
    for (const value of rows) {
      const row = element("div", "appearance-field");
      const inherited = value.enabled == null && (value.canToggle === true || Object.hasOwn(value, "enabled"));
      const description = [value.description || value.transport,
        inherited && value.canToggle === true ? "使用原生默认；切换后保存所选状态。" : ""].filter(Boolean).join(" · ");
      row.append(fieldCopy(value.name || value.id || value.path, description));
      if (value.canToggle === true) {
        const control = switchControl(value.name || value.id, value.enabled);
        control.onchange = () => void save({ [name === "skills" ? "skill" : "server"]:
          { id: value.id, enabled: control.checked } });
        controls.add(control); row.append(control);
      } else if (typeof value.enabled === "boolean") {
        row.append(element("span", "muted", value.enabled ? "已开启" : "已关闭"));
      } else if (inherited) {
        row.append(element("span", "muted", "使用原生默认"));
      }
      card.append(row);
    }
  }
  function render() {
    content.replaceChildren(); controls.clear();
    if (record) {
      if (name === "general") renderGeneral();
      else if (name === "models") renderModels();
      else renderInventory();
      if (record.readOnly || session?.readOnly) message ||= record.readOnlyReason || "当前为只读访问";
    }
    sync();
  }
  function checkedResult(result, id) {
    if (!result || result.ok === false || result.instanceId !== id || result.backend !== currentInstance()?.backend
        || typeof result.readOnly !== "boolean"
        || typeof result.revision !== "string" || typeof result.preferenceRevision !== "string"
        || !result.settings || Array.isArray(result.settings) || typeof result.settings !== "object")
      throw new Error("实例设置返回结果无法确认，请重新读取。");
    const values = result.settings;
    if (name === "general") {
      const keys = result.backend === "codex" ? ["todo", "memories", "multiAgent", "goals"]
        : ["awaySummaryEnabled", "promptSuggestionEnabled"];
      if (keys.some(key => !Object.hasOwn(values, key) || values[key] !== null && typeof values[key] !== "boolean"))
        throw new Error("实例常规设置字段不完整，请重新读取。");
      if (result.backend === "claude" && (!Object.hasOwn(values, "autoCompactWindow")
          || values.autoCompactWindow !== null && values.autoCompactWindow !== "auto"
            && !(Number.isInteger(values.autoCompactWindow) && values.autoCompactWindow >= 100000
              && values.autoCompactWindow <= 1000000)))
        throw new Error("实例常规设置字段不完整，请重新读取。");
    } else if (name === "models") {
      if (!Array.isArray(result.models) || result.models.some(model => !model || typeof model.model !== "string"
            || model.displayName != null && typeof model.displayName !== "string"
            || model.supportedReasoningEfforts != null && (!Array.isArray(model.supportedReasoningEfforts)
              || model.supportedReasoningEfforts.some(effort => !effort || typeof effort.reasoningEffort !== "string")))
          || ["model", "reasoningEffort"].some(key => !Object.hasOwn(values, key)
            || values[key] !== null && typeof values[key] !== "string")
          || result.effectiveModel != null && typeof result.effectiveModel !== "string"
          || result.backend === "codex" && typeof values.ultraReasoningEnabled !== "boolean")
        throw new Error("实例模型设置字段不完整，请重新读取。");
    } else if (!result.inventory || !Array.isArray(result.inventory.data)
        || result.inventory.data.some(row => !row || typeof row !== "object"
          || typeof (row.name || row.id || row.path) !== "string"
          || row.canToggle === true && typeof row.id !== "string"
          || row.enabled != null && typeof row.enabled !== "boolean")
        || name === "hooks" && (!Object.hasOwn(result.inventory, "enabled")
          || result.inventory.enabled !== null && typeof result.inventory.enabled !== "boolean")) {
      throw new Error("实例扩展列表字段不完整，请重新读取。");
    }
    return result;
  }
  async function read() {
    if (!alive() || saving || detecting) return;
    const id = selected, version = retireRead();
    record = null; needsRead = false;
    if (!id || !currentInstance() || currentInstance().error) {
      message = "所选实例暂不可用，请重新检测或明确选择其他实例。"; render(); return;
    }
    if (!session || !connected) {
      message = "连接远程服务后可读取实例设置。"; render(); return;
    }
    readController = new AbortController();
    loading = true; record = null; message = ""; needsRead = false; render();
    try {
      const result = await api("/api/settings/instance", { instanceId: id, section: name }, undefined, readController.signal);
      if (!alive() || selected !== id || generation !== version) return;
      record = checkedResult(result, id);
    } catch (error) {
      if (alive() && selected === id && generation === version) message = error.message;
    } finally {
      if (alive() && selected === id && generation === version) {
        loading = false; render();
        if (externalRefresh) { externalRefresh = false; void read(); }
      }
    }
  }
  async function save(settings) {
    if (!writable()) { render(); return; }
    const id = selected, before = record;
    saving = true; message = ""; render();
    try {
      const result = await api("/api/settings/instance", { instanceId: id, section: name,
        operation: "save", settings, expectedRevision: before.revision,
        expectedPreferenceRevision: before.preferenceRevision });
      if (scope !== privateInputScope(session) || selected !== id) return;
      record = checkedResult(result, id);
      for (const [key, value] of Object.entries(settings)) {
        const actual = key === "enabled" ? record.inventory?.enabled
          : ["skill", "server"].includes(key) ? record.inventory?.data?.find(row => row.id === value.id)?.enabled
            : record.settings[key === "effortLevel" ? "reasoningEffort" : key];
        if (actual !== (["skill", "server"].includes(key) ? value.enabled : value))
          throw new Error("保存结果与所选设置不一致，请重新读取核对。");
      }
      if (name === "models") {
        // 已确认的配置变更即使设置窗口已关闭也须失效缓存；其他实例的旧磁盘目录同样不能在下次启动恢复。
        const removed = readCache.remove(scope, "models", id === "codex:local" ? "global" : id).catch(() => {});
        if (id === activeInstanceId()) {
          // 关闭 Ultra 后清理尚未提交的 Ultra 选择，避免下一次发送继续携带禁用等级；已有原生会话深度保持原值。
          if (record.backend === "codex" && record.settings.ultraReasoningEnabled === false && selectedEffort === "ultra")
            setSelectedEffort("");
          invalidateModelSettings(); updateComposer();
          void removed.then(() => {
            if (scope === privateInputScope(session) && id === activeInstanceId()) return loadModels(true);
          }).catch(() => {});
        }
      } else if (name === "skills") {
        // 用户修改原生 Skill 后及时撤销旧补全缓存；只为当前实例主动重读，不借用其他会话的环境探测。
        void refreshSkillCatalog({ reload: id === activeInstanceId() }).catch(() => {});
      }
      if (alive()) message = "已保存，新会话或重新加载后生效。";
    } catch (error) {
      if (alive() && selected === id) {
        record = before; needsRead = true;
        message = `${error.message} 未自动重复保存，请重新读取核对。`;
      }
    } finally {
      if (alive() && selected === id) {
        saving = false; render();
        if (externalRefresh) { externalRefresh = false; void read(); }
      }
    }
  }
  async function show(force = false) {
    if (!alive() || saving) return;
    // 重新检测开始即撤销旧读取和写入口；迟到的旧回包不能盖掉“实例不可用”，也不能恢复旧控件。
    const version = retireRead();
    detecting = true; message = ""; sync();
    if (!session || !connected) {
      detecting = false; record = null; message = "连接远程服务后可读取实例设置。"; render(); return;
    }
    try {
      await loadSessionInstances(force);
      if (!alive() || generation !== version) return;
      const choices = availableSessionInstances().map(instance =>
        ({ value: instance.id, label: instance.label, icon: instance.backend, disabled: !!instance.error }));
      // 用户要求按真实实例配置；页面间保留同一选择，实例消失时保留目标，不能悄悄改写其他实例。
      const preferred = getSelectedInstance() || selected || activeInstanceId();
      const target = choices.find(option => option.value === preferred);
      detecting = false;
      if (preferred && (!target || target.disabled)) {
        if (!target) choices.push({ value: preferred,
          label: instances.selectOptions.find(option => option.value === preferred)?.label || preferred, disabled: true });
        selected = preferred; setSelectOptions(instances, choices, selected);
        record = null; message = "所选实例暂不可用，请重新检测或明确选择其他实例。"; render(); return;
      }
      selected = preferred || choices.find(option => !option.disabled)?.value || "";
      setSelectedInstance(selected);
      setSelectOptions(instances, choices, selected);
      if (selected) await read();
      else { record = null; message = "尚未检测到可配置的会话实例。"; render(); }
    } catch (error) {
      if (alive() && generation === version) { record = null; message = error.message; render(); }
    } finally {
      if (alive() && generation === version) { detecting = false; sync(); }
    }
  }
  instances.onchange = () => {
    if (!alive() || saving || detecting) { sync(); return; }
    selected = instances.value; setSelectedInstance(selected); void read();
  };
  display.onchange = () => {
    if (alive() && !detecting && !saving && sharedSettingsWritable(session))
      void setInstanceDisplay(selected, display.checked).catch(error => { notice(error.message, true); sync(); });
    sync();
  };
  const unsubscribe = subscribeSharedSettings(change => {
    if (!alive()) return;
    sync();
    if (change.instance && (!change.instanceId || change.instanceId === selected)
        && !page.closest?.(".appearance-settings-page")?.hidden) {
      if (saving || detecting || loading) externalRefresh = true;
      else void read();
    }
  });
  render();
  return { show, dispose() { disposed = true; retireRead(); unsubscribe(); } };
}

export function mountShortcuts(page) {
  const mac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  const modifier = mac ? "⌘" : "Ctrl";
  const card = section(page, "会话编辑区");
  // 用户要求只显示快捷键；列表逐项对应 shell.js 已有按键处理，不增加未实现的全局快捷键。
  for (const [label, keys] of [
    ["发送消息", `${modifier} + Enter`], ["换行", "Enter"],
    ["撤销", `${modifier} + Z`], ["重做", `${modifier} + Y / ${modifier} + Shift + Z`],
    ["选择命令或 Skill", "↑ / ↓"], ["确认命令或 Skill", "Enter / Tab"],
    ["关闭菜单或设置", "Esc"]
  ]) {
    const row = element("div", "appearance-field shortcut-setting-row");
    row.append(element("span", "", label), element("kbd", "", keys)); card.append(row);
  }
  page.append(element("p", "appearance-hint", "编辑快捷键在输入框获得焦点时生效；命令菜单中的 Enter / Tab 用于选择当前项。"));
}
