// 用户要求所有实例共用一组命名实例与模型；配置由服务保存，页面草稿按授权隔离。
import { button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { selectControl, setSelectOptions } from "../../ui/select.js";
import { notice } from "../../ui/notice.js";
import { confirmAction } from "../../ui/dialog.js";
import { saveSharedSettings } from "../../core/actions.js";
import { connected, session } from "../../core/state.js";
import { api } from "../api.js";
import { privateInputScope } from "../session/capabilities.js";
import { loadSessionInstances } from "../session/instance-catalog.js";
import { availableSessionInstances } from "../session/instances.js";
import { sharedSettingsState, sharedSettingsWritable, subscribeSharedSettings } from "./shared-state.js";

const namingListeners = new Set();
// 用户要求同一功能只有一套口径；可见名称与无障碍名称共用，自动更新标题是自动命名的子设置。
const namingLabels = {
  enabled: "自动命名", refresh: "自动更新标题", instance: "命名实例", model: "命名模型",
  maxTitleChars: "标题长度上限", refreshEveryTurns: "标题更新间隔", systemPrompt: "命名系统提示词"
};
export function receiveConversationNaming(value) {
  for (const listener of namingListeners) listener(value);
  if (value?.state === "failed") notice(value.message || "自动命名未完成，请检查会话命名设置。", true);
}

export function mountConversationNaming(page) {
  page.classList.add("instance-settings-content");
  const scope = privateInputScope(session);
  let disposed = false, loading = false, saving = false, defaults = null, base = null, draft = null;
  let revision = -1, epoch = "", generation = 0, readController = null, loadedModels = [];
  const alive = () => !disposed && page.isConnected && privateInputScope(session) === scope;
  const supported = () => session?.capabilities?.conversationNaming === true && sharedSettingsState.ready;
  const dirty = () => draft && base && JSON.stringify(draft) !== JSON.stringify(base.conversationNaming);
  const writable = () => alive() && connected && supported() && sharedSettingsWritable(session) && !saving;
  const card = element("div", "appearance-card");
  const switchControl = label => {
    const input = element("input", "appearance-notification-switch");
    input.type = "checkbox"; input.setAttribute("role", "switch"); input.setAttribute("aria-label", label);
    return input;
  };
  const enabled = switchControl(namingLabels.enabled), refresh = switchControl(namingLabels.refresh);
  const instances = selectControl("conversation-naming-instance", namingLabels.instance, [], "", "instance-settings-select",
    option => {
      const glyph = icon(option?.icon || "allSessions");
      glyph.classList.add("instance-settings-brand"); glyph.dataset.backend = option?.icon || "";
      return [glyph, element("span", "select-label", option?.label || "请选择命名实例"), icon("chevron")];
    });
  const models = selectControl("conversation-naming-model", namingLabels.model, [], "", "instance-settings-select");
  models.selectSearchable = true; models.selectSearchPlaceholder = "搜索命名模型";
  const numberControl = (label, min, max) => {
    const input = element("input"); input.type = "number"; input.min = String(min);
    input.max = String(max); input.step = "1"; input.setAttribute("aria-label", label); return input;
  };
  const length = numberControl(namingLabels.maxTitleChars, 10, 120);
  const interval = numberControl(namingLabels.refreshEveryTurns, 1, 50);
  const row = (label, description, control, tag = "label") => {
    const field = element(tag, "appearance-field"), copy = element("span", "appearance-field-copy");
    copy.append(element("span", "", label), element("small", "", description));
    field.append(copy, control); card.append(field);
    return field;
  };
  row(namingLabels.enabled, "首条普通消息成功接收后生成标题，适用于所有 Codex、Claude 实例的主会话。", enabled);
  // 用户要求实例与模型是关联选择：同一行从左到右先实例、后模型；切换实例后清空旧模型并重新读取目录。
  const targetSelectors = element("div", "conversation-naming-target-selectors");
  targetSelectors.append(instances, models);
  row("命名实例与模型", "先选择命名实例，再选择其命名模型；所有主会话共用此组合，不改变原会话的模型。",
    targetSelectors, "div").classList.add("conversation-naming-target-row");
  const lengthBox = element("span", "appearance-number"); lengthBox.append(length);
  const intervalBox = element("span", "appearance-number"); intervalBox.append(interval);
  row(namingLabels.maxTitleChars, "默认 36 个字符，可设置为 10～120 个字符。", lengthBox);
  row(namingLabels.refresh, "仅在自动命名开启后生效；按标题更新间隔检查主题，主题未变时保留原标题。手动重命名后停止更新。", refresh);
  row(namingLabels.refreshEveryTurns, "标题生成后，默认每完成 5 个成功轮次检查一次；失败、中断和重复事件不计数。", intervalBox);
  const promptSection = element("section", "appearance-section");
  promptSection.append(element("h4", "", namingLabels.systemPrompt));
  const prompt = element("textarea", "instruction-content");
  prompt.id = "conversation-naming-prompt"; prompt.rows = 13; prompt.maxLength = 20000;
  prompt.setAttribute("aria-label", namingLabels.systemPrompt); prompt.spellcheck = false;
  const promptHint = element("p", "appearance-hint", "默认命名系统提示词参考 Codex App：使用用户语言概括任务，并保留工单及代码名称。最多 10000 字符。");
  const reset = button("恢复默认提示词", "恢复默认命名系统提示词", () => {
    if (!writable() || !defaults) return;
    draft.systemPrompt = defaults.systemPrompt; prompt.value = draft.systemPrompt; sync();
  }, "text-button");
  promptSection.append(promptHint, prompt, reset);
  const status = element("p", "appearance-hint");
  status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  const recent = element("p", "appearance-hint");
  const actions = element("div", "appearance-action-buttons");
  const reload = button("重新读取", "重新读取会话命名设置", () => void show(true), "text-button");
  const save = button("保存", "保存会话命名设置", () => void persist(), "primary");
  actions.append(reload, save);
  page.append(card, promptSection, element("p", "appearance-hint",
    "同一远程服务的各客户端共用会话命名设置。命名实例和命名模型根据会话文字生成标题，不改动正文；生成失败时保留原标题。"),
  status, recent, actions);

  function sync() {
    const canWrite = writable();
    for (const control of [enabled, instances, models, length, refresh, interval, prompt, reset])
      control.disabled = !canWrite || !draft;
    models.disabled ||= loading;
    // 主开关关闭时子设置不生效，禁用控件并保留原选择，避免呈现为两个互相独立的命名开关。
    refresh.disabled ||= !draft?.enabled;
    interval.disabled ||= !draft?.enabled || !draft?.refreshEnabled;
    save.disabled = !canWrite || loading || !dirty();
    reload.disabled = saving || loading || !alive();
  }
  function restore() {
    base = structuredClone(sharedSettingsState.settings); draft = structuredClone(base.conversationNaming);
    revision = sharedSettingsState.revision; epoch = sharedSettingsState.epoch;
    enabled.checked = draft.enabled; refresh.checked = draft.refreshEnabled;
    length.value = String(draft.maxTitleChars); interval.value = String(draft.refreshEveryTurns);
    prompt.value = draft.systemPrompt;
    renderInstances(); renderModels(); sync();
  }
  function renderInstances() {
    const choices = availableSessionInstances().map(instance => ({ value: instance.id,
      label: instance.label, icon: instance.backend, disabled: !instance.ready || !!instance.error }));
    if (draft?.instanceId && !choices.some(option => option.value === draft.instanceId))
      choices.push({ value: draft.instanceId, label: draft.instanceId, disabled: true });
    setSelectOptions(instances, choices, draft?.instanceId || "");
  }
  function renderModels() {
    const choices = loadedModels.map(model => ({ value: model.model, label: model.displayName || model.model }));
    if (draft?.model && !choices.some(option => option.value === draft.model))
      choices.unshift({ value: draft.model, label: `${draft.model}（请核对可用状态）`, disabled: true });
    if (!draft?.model || !choices.length) choices.unshift({ value: "", label: loading ? "正在读取命名模型…" : "请选择命名模型" });
    setSelectOptions(models, choices, draft?.model || "");
  }
  async function readNamingModels() {
    const id = draft?.instanceId, version = ++generation;
    readController?.abort(); readController = new AbortController();
    loadedModels = []; loading = true; renderModels(); sync();
    try {
      const instance = availableSessionInstances().find(instance => instance.id === id);
      if (!instance?.ready || instance.error) throw new Error("所选命名实例暂不可用，请重新选择命名实例。");
      const result = await api("/api/read/models", { backend: instance.backend, instanceId: id, includeAvailable: true },
        undefined, readController.signal);
      if (!alive() || generation !== version || draft.instanceId !== id) return;
      if (!Array.isArray(result.data) || result.data.some(model => typeof model.model !== "string"))
        throw new Error("命名模型目录返回无效。");
      loadedModels = result.data; renderModels();
      if (!loadedModels.length) status.textContent = "所选命名实例未返回可用模型，请检查该实例的原生 CLI。";
    } catch (error) {
      if (alive() && generation === version && error.name !== "AbortError") status.textContent = error.message;
    } finally { if (alive() && generation === version) { loading = false; renderModels(); sync(); } }
  }
  function renderRecent(value) {
    recent.textContent = value?.state === "failed" ? value.message
      : value?.state === "completed" ? `最近自动命名结果：${value.title}` : "";
  }
  async function show(force = false) {
    if (!alive() || saving) return;
    if (force && dirty() && !await confirmAction("重新读取会话命名设置", "当前未保存的修改将被服务端设置替换。", "重新读取")) return;
    if (!alive()) return;
    if (!supported()) { status.textContent = "当前服务尚未支持会话命名，请更新服务后设置。"; sync(); return; }
    status.textContent = "";
    try {
      const data = await api("/api/settings/conversation-naming");
      if (!alive()) return;
      defaults = data.defaults; renderRecent(data.status);
      await loadSessionInstances();
      if (!alive()) return;
      if (!draft || force || !dirty()) restore();
      await readNamingModels();
    } catch (error) { if (alive()) { status.textContent = error.message; sync(); } }
  }
  async function persist() {
    if (!writable() || save.disabled || !draft || !base) return;
    if (epoch !== sharedSettingsState.epoch) { status.textContent = "服务已重新连接，请重新打开设置后核对。"; return; }
    if (draft.enabled && (!availableSessionInstances().some(instance => instance.id === draft.instanceId && instance.ready)
        || !loadedModels.some(model => model.model === draft.model))) {
      status.textContent = "请选择可用的命名实例和命名模型后开启自动命名。"; return;
    }
    if (!Number.isInteger(draft.maxTitleChars) || draft.maxTitleChars < 10 || draft.maxTitleChars > 120
        || !Number.isInteger(draft.refreshEveryTurns) || draft.refreshEveryTurns < 1 || draft.refreshEveryTurns > 50
        || !draft.systemPrompt.trim() || Array.from(draft.systemPrompt).length > 10000) {
      status.textContent = "请核对标题长度上限、标题更新间隔和命名系统提示词。"; return;
    }
    const patch = Object.fromEntries(Object.entries(draft).filter(([key, value]) => value !== base.conversationNaming[key]));
    saving = true; status.textContent = "正在同步到所有客户端…"; sync();
    try {
      await saveSharedSettings({ conversationNaming: patch }, { base, expectedRevision: revision });
      if (!alive()) return;
      restore(); status.textContent = "会话命名设置已保存，同一远程服务的各客户端共用。";
    } catch (error) {
      if (alive()) status.textContent = `${error.message} 修改内容已保留，请核对其他客户端的设置。`;
    } finally { if (alive()) { saving = false; sync(); } }
  }
  enabled.onchange = () => { draft.enabled = enabled.checked; sync(); };
  refresh.onchange = () => { draft.refreshEnabled = refresh.checked; sync(); };
  instances.onchange = () => {
    draft.instanceId = instances.value; draft.model = ""; status.textContent = ""; void readNamingModels(); sync();
  };
  models.onchange = () => { draft.model = models.value; sync(); };
  length.oninput = () => { draft.maxTitleChars = Number(length.value); sync(); };
  interval.oninput = () => { draft.refreshEveryTurns = Number(interval.value); sync(); };
  prompt.oninput = () => { draft.systemPrompt = prompt.value; sync(); };
  const unsubscribe = subscribeSharedSettings(change => {
    if (!alive() || !supported()) { sync(); return; }
    if (draft && !saving) {
      const changed = JSON.stringify(base?.conversationNaming) !== JSON.stringify(sharedSettingsState.settings.conversationNaming);
      if (dirty() && changed) status.textContent = "其他客户端已修改会话命名设置，当前未保存内容已保留；保存时将核对冲突。";
      else if (!dirty() && changed) { restore(); if (!page.closest(".appearance-settings-page")?.hidden) void readNamingModels(); }
    }
    if (change.instance && (!change.instanceId || change.instanceId === draft?.instanceId)
        && !page.closest(".appearance-settings-page")?.hidden && !saving) void readNamingModels();
    sync();
  });
  const receive = value => { if (alive()) renderRecent(value); };
  namingListeners.add(receive); sync();
  return { show, dispose() { disposed = true; generation++; readController?.abort(); unsubscribe(); namingListeners.delete(receive); } };
}
