// 实例设置修改原生全局配置；编辑区菜单只选择当前会话的权限，两者不能互相代写。
// 用户要求 Codex 菜单与 App 同名同义，只保留这四项；不新增只读、实例设置或命名配置入口。
const codexModes = [
  { value: "auto", label: "请求批准", icon: "permissionAsk",
    description: "编辑外部文件和使用互联网时始终询问" },
  { value: "guardian-approvals", label: "帮我批准", icon: "shield",
    description: "仅对检测到的风险操作请求批准" },
  { value: "full-access", label: "完全访问权限", shortLabel: "完全访问", icon: "permissionFull",
    description: "可不受限制地访问互联网和你电脑上的任何文件" },
  { value: "configured", label: "自定义 (config.toml)", shortLabel: "自定义", icon: "settings",
    description: "使用 config.toml 中定义的权限" }
];
// Claude 菜单只提供 App 的五种模式；manual/default 共用一项，配置继承和旧 dontAsk 值仅兼容既有记录。
const claudeModes = [
  { value: "default", label: "手动", icon: "claudeManual", description: "每次更改前都先询问" },
  { value: "acceptEdits", label: "接受编辑", icon: "claudeEdits", description: "自动接受所有文件编辑" },
  { value: "plan", label: "计划", icon: "claudePlan", description: "更改前先制定计划" },
  { value: "auto", label: "自动", icon: "claudeAuto", description: "由 Claude 处理权限决策" },
  { value: "bypassPermissions", label: "绕过权限", icon: "claudeBypass", description: "接受所有权限请求" }
];

export function permissionOptions(backend, supportedModes = null) {
  const options = backend === "claude" ? claudeModes : codexModes;
  return options.filter(option => !supportedModes || backend !== "claude" && option.value === "configured"
      || supportedModes.includes(option.value)
      || backend === "claude" && option.value === "default" && supportedModes.includes("manual"))
    .map(option => ({ ...option, value: backend === "claude" && option.value === "default"
        && supportedModes && !supportedModes.includes("default") && supportedModes.includes("manual")
      ? "manual" : option.value }));
}

export function permissionModeKnown(backend, mode) {
  return permissionOptions(backend).some(option => option.value === mode)
    || (backend === "claude" ? ["manual", "configured", "dontAsk"].includes(mode) : mode === "read-only");
}

export function permissionLabel(backend, mode, creating = false) {
  // 旧会话的只读预设仍按原权限执行；与 App 一致归入“请求批准”，只改变展示，不自动迁移权限。
  const value = backend === "claude" ? mode === "manual" ? "default" : mode
    : mode === "read-only" ? "auto" : mode || "configured";
  const option = permissionOptions(backend).find(option => option.value === value);
  return option?.shortLabel || option?.label || (backend !== "claude" && creating ? "自定义" : "权限");
}

export function mountInstancePermissions({
  page, element, selectControl, setSelectOptions, setSelectValue, api, getInstances,
  readOnly, authorizationScope, onLoaded = () => {}, onSaved = () => {}
}) {
  const scope = authorizationScope();
  const current = () => page.isConnected && scope === authorizationScope();
  const instanceSlot = element("div", "permission-instance-slot");
  const heading = element("label", "appearance-field permission-instance-field");
  heading.append(element("span", "", "会话实例"), instanceSlot);
  const card = element("div", "appearance-card instance-permissions-card");
  const path = element("p", "appearance-hint instruction-path");
  const fields = element("div", "instance-permission-fields");
  const status = element("p", "appearance-hint permission-settings-status");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const actions = element("div", "appearance-action-buttons permission-settings-actions");
  const reload = element("button", "", "重新读取"), save = element("button", "primary", "保存权限");
  reload.type = save.type = "button";
  actions.append(reload, save);
  card.append(heading, path, fields, status, actions);
  page.append(element("p", "appearance-hint",
    "修改所选实例的全局权限，与原生客户端设置共用。已有会话的单独权限选择保持不变。"), card);
  const instances = selectControl("permission-instance", "权限设置的会话实例", [], "");
  instanceSlot.append(instances);
  const records = new Map(), reading = new Map(), writing = new Set();
  let disposed = false, selected = "";
  const alive = () => !disposed && current();
  const instance = () => getInstances().find(instance => instance.id === selected);
  const allowed = () => alive() && !readOnly() && records.get(selected)?.readOnly !== true;
  const changes = record => Object.fromEntries(Object.entries(record?.draft || {})
    .filter(([key, value]) => value !== record.original[key]));
  const syncButtons = () => {
    const record = records.get(selected), loading = reading.has(selected), busy = writing.has(selected);
    save.disabled = !allowed() || loading || busy || !record || !Object.keys(changes(record)).length;
    save.textContent = busy ? "保存中…" : "保存权限";
    save.setAttribute("aria-busy", String(busy));
    reload.disabled = loading || busy;
    instances.disabled = !getInstances().length;
  };
  function addField(label, name, options, value, disabled = false) {
    const row = element("label", "appearance-field instance-permission-field");
    row.append(element("span", "", label));
    if (name === "permissionMode" && ["default", "manual"].includes(value))
      value = options.find(option => ["default", "manual"].includes(option.value))?.value || value;
    if (!options.some(option => option.value === value))
      options = [...options, { value, label: "当前自定义配置（保留）", disabled: true }];
    const control = selectControl(`permission-global-${name}`, label, options, value);
    control.disabled = disabled || !allowed() || writing.has(selected);
    control.onchange = () => {
      const record = records.get(selected);
      if (!allowed() || !record || writing.has(selected)) return;
      record.draft[name] = control.value;
      record.message = "";
      status.textContent = "尚未保存";
      syncButtons();
    };
    row.append(control);
    fields.append(row);
  }
  function render() {
    if (!alive()) return;
    const record = records.get(selected);
    fields.replaceChildren();
    path.textContent = record?.path || "";
    status.textContent = reading.has(selected) ? "正在读取实例权限…" : record?.message || "";
    if (record) {
      const defaults = { value: "", label: "沿用原生默认" };
      if (record.backend === "codex") {
        addField("批准策略", "approvalPolicy",
          [defaults, { value: "on-request", label: "按请求" }, { value: "never", label: "从不请求批准" }],
          record.draft.approvalPolicy);
        addField("沙盒设置", "sandboxMode", [defaults,
          { value: "read-only", label: "只读" }, { value: "workspace-write", label: "工作区写入" },
          { value: "danger-full-access", label: "完整访问权限" }],
        record.permissionProfile ? "__profile__" : record.draft.sandboxMode, !!record.permissionProfile);
        const hint = element("p", "appearance-hint",
          record.permissionProfile
            ? "当前访问范围包含本界面不支持的配置，请在原生 config.toml 中调整。"
            : "沙盒控制可访问范围，批准策略控制是否请求确认；完整访问范围与“从不请求批准”分别设置。");
        fields.append(hint);
      } else {
        const modes = permissionOptions("claude", record.supportedModes).filter(option => option.value !== "configured");
        addField("默认权限模式", "permissionMode", [defaults, ...modes], record.draft.permissionMode,
          record.supportedModes?.length === 0);
        if (record.supportedModes?.length === 0)
          fields.append(element("p", "appearance-hint", "暂时无法确认该 CLI 支持的权限模式，请重新读取后再修改。"));
      }
      if (record.readOnly || readOnly()) status.textContent ||= "当前为只读访问";
    }
    syncButtons();
  }
  async function load(id, force = false) {
    if (!id || !alive() || writing.has(id)) return;
    if (!force && records.has(id)) { render(); return; }
    if (reading.has(id)) return reading.get(id);
    const operation = api("/api/settings/permissions", { instanceId: id }).then(result => {
      if (!alive()) return;
      const owner = getInstances().find(instance => instance.id === id);
      if (result?.instanceId !== id || result.backend !== owner?.backend
          || typeof result.revision !== "string" || !result.settings || typeof result.settings !== "object")
        throw new Error("实例权限读取结果不匹配，请重新读取。");
      const original = owner.backend === "codex" ? {
        approvalPolicy: result.settings.customApproval ? "__custom__" : result.settings.approvalPolicy || "",
        sandboxMode: result.settings.sandboxMode || ""
      } : { permissionMode: result.settings.permissionMode || "" };
      records.set(id, { ...result, ...result.settings, original, draft: { ...original }, message: "" });
      onLoaded(id, result);
    }).catch(error => {
      if (!alive()) return;
      const record = records.get(id);
      if (record) record.message = error.message;
      if (selected === id) status.textContent = error.message;
    }).finally(() => {
      reading.delete(id);
      if (alive() && selected === id) {
        // 没有已确认配置时保留读取错误，不能用空默认值开放保存。
        if (records.has(id)) render();
        else syncButtons();
      }
    });
    reading.set(id, operation);
    render();
    return operation;
  }
  instances.onchange = () => {
    selected = instances.value;
    render();
    void load(selected);
  };
  reload.onclick = () => void load(selected, true);
  save.onclick = async () => {
    const id = selected, record = records.get(id), settings = changes(record);
    if (!allowed() || !record || !Object.keys(settings).length || writing.has(id)) return;
    writing.add(id);
    record.message = "";
    render();
    try {
      const result = await api("/api/settings/permissions", {
        operation: "save", instanceId: id, expectedRevision: record.revision, settings
      });
      if (!alive()) return;
      if (result?.instanceId !== id || typeof result.revision !== "string"
          || !result.settings || result.backend !== record.backend)
        throw new Error("保存结果尚未确认，请重新读取实例权限。");
      const original = record.backend === "codex" ? {
        approvalPolicy: result.settings.customApproval ? "__custom__" : result.settings.approvalPolicy || "",
        sandboxMode: result.settings.sandboxMode || ""
      } : { permissionMode: result.settings.permissionMode || "" };
      records.set(id, { ...record, ...result, ...result.settings, original, draft: { ...original },
        message: "已保存至实例全局配置。新会话及后续按实例配置加载的操作使用新设置。" });
      onSaved(id, result);
    } catch (error) {
      if (alive()) record.message = `${error.message} 未自动重复保存，可重新读取核对。`;
    } finally {
      writing.delete(id);
      if (alive() && selected === id) render();
    }
  };
  return {
    async show(id = null) {
      if (!alive()) return;
      const choices = getInstances().map(instance => ({ value: instance.id, label: instance.label,
        disabled: !!instance.error }));
      selected = choices.some(option => option.value === id) ? id
        : choices.some(option => option.value === selected) ? selected : choices[0]?.value || "";
      setSelectOptions(instances, choices, selected);
      setSelectValue(instances, selected);
      render();
      if (!selected) status.textContent = "尚未检测到可配置的实例";
      else await load(selected);
    },
    dispose() { disposed = true; }
  };
}
