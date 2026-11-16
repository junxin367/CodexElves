// 设置页中的远程会话设置：环境/指令设置与 Claude 回顾设置。

import { confirmAction } from "../../ui/dialog.js";
import { $ } from "../../ui/dom.js";
import { setSelectOptions } from "../../ui/select.js";
import { connected, session } from "../../core/state.js";
import { api } from "../api.js";
import { privateInputScope } from "../session/capabilities.js";
import { loadSessionInstances } from "../session/instance-catalog.js";

// textarea 将 CRLF 规范为 LF；比较草稿时统一文本，保存时恢复原文件换行，避免未编辑即提示“未保存”。
const normalizeInstructionText = value => value.replace(/\r\n?/g, "\n");
const instructionFileText = (value, lineEnding) => lineEnding === "\r\n" ? value.replace(/\n/g, "\r\n") : value;
const instructionDrafts = new Map();
export function mountRemoteSettings(dialog) {
  const find = id => $("#" + id, dialog);
  const instance = find("instruction-instance"), input = find("instruction-content");
  const status = find("instruction-status"), save = find("instruction-save"), reload = find("instruction-reload");
  const detectionButton = find("appearance-detect-environments"), detectionStatus = find("appearance-environments-status");
  let disposed = false, generation = 0, environments = null, instances = [], detection = null, current = null, saving = false;
  const deviceId = session?.device.id;
  const alive = () => !disposed && dialog.isConnected && session?.device.id === deviceId;
  // 用户要求只选择会话实例，编辑其用户级全局指令；实例身份同时隔离各环境的未保存草稿。
  const identity = () => ({ instanceId: instance.value });
  const keyFor = value => JSON.stringify([privateInputScope(session), value]);
  // 用户要求正常读取后不显示“已读取”；检测、读取及撤销编辑恢复原文时均留空。
  const dirty = () => current && input.value !== current.content;
  // 历史会话保留的已卸载实例不能冒充当前安装；本机 Codex 通过独立远程服务管理全局指令。
  const instanceAvailable = id => instances.some(value => value.id === id && environments?.some(environment =>
    environment.id === value.environmentId && environment.user === value.user && !environment.error
      && (value.id === "codex:local" || !!environment.executables?.[value.backend])));
  const update = () => {
    save.disabled = saving || !!detection || !current || !instanceAvailable(current.instanceId)
      || current.key !== keyFor(identity()) || !dirty() || session?.readOnly || !connected;
    reload.disabled = saving || !!detection || !instanceAvailable(instance.value); input.disabled = saving || !current;
    if (detectionButton) detectionButton.disabled = saving || !!detection;
    instance.disabled = saving || !!detection;
  };
  const renderInstances = () => {
    const old = instance.value;
    const options = instances.map(value => ({ value: value.id, label: value.label, disabled: !instanceAvailable(value.id) }));
    if (old && !instances.some(value => value.id === old)) {
      // 实例暂不可用时保留原目标，不能把旧指令和草稿悄悄配给另一个本机/WSL 文件。
      const label = instance.selectOptions.find(option => option.value === old)?.label || old;
      options.push({ value: old, label, description: "暂不可用", disabled: true });
    }
    setSelectOptions(instance, options, old || "codex:local");
  };
  async function detect(force = false) {
    if (detection) return detection;
    if (detectionButton) detectionButton.disabled = true;
    if (detectionStatus) detectionStatus.textContent = "正在检测本机与 WSL 中的会话实例…";
    detection = (async () => {
      try {
        const result = await loadSessionInstances(force);
        if (!alive()) return;
        if (!Array.isArray(result.environments) || !Array.isArray(result.instances)) throw new Error("实例检测数据无效。");
        environments = result.environments;
        instances = result.instances;
        renderInstances();
        if (detectionStatus) detectionStatus.textContent = "检测安装位置，未配置的 Claude 离开期间回顾默认关闭；实例配置与显示此实例开关由同一远程服务的各客户端共用。";
        if (current) status.textContent = instanceAvailable(current.instanceId)
          ? dirty() ? "未保存的草稿" : ""
          : "所选实例暂不可用，草稿已保留。";
      } catch (error) {
        if (alive()) {
          if (detectionStatus) detectionStatus.textContent = error.message;
          else status.textContent = error.message;
        }
      } finally {
        detection = null;
        if (alive()) update();
      }
    })();
    update();
    return detection;
  }
  async function read({ discard = false } = {}) {
    if (!alive() || saving) return;
    const version = ++generation, value = identity(), key = keyFor(value);
    current = null; input.value = ""; update();
    find("instruction-path").textContent = "";
    if (!instanceAvailable(value.instanceId)) {
      status.textContent = "请选择可用的会话实例。"; return;
    }
    status.textContent = "正在读取原生指令文件…";
    try {
      const result = await api("/api/settings/instructions", value);
      if (!alive() || generation !== version || keyFor(identity()) !== key) return;
      if (typeof result.content !== "string" || typeof result.revision !== "string")
        throw new Error("指令文件返回格式无效。");
      const content = normalizeInstructionText(result.content);
      current = { ...value, content, lineEnding: result.content.includes("\r\n") ? "\r\n" : "\n", revision: result.revision, key };
      let draft = !discard && instructionDrafts.get(key);
      if (draft?.content === content) { instructionDrafts.delete(key); draft = null; }
      input.value = draft?.content ?? content;
      if (draft && draft.revision !== result.revision) {
        current.revision = draft.revision;
        status.textContent = "有未保存的草稿，原文件也已变化。请复制草稿后重新读取并合并。";
      } else status.textContent = dirty() ? "未保存的草稿" : result.exists ? "" : "文件尚不存在，保存时创建。";
      find("instruction-path").textContent = result.path;
    } catch (error) {
      if (alive() && generation === version) status.textContent = error.message;
    } finally { if (alive() && generation === version) update(); }
  }
  input.oninput = () => {
    if (!current) return;
    if (dirty()) instructionDrafts.set(current.key, { content: input.value, revision: current.revision });
    else instructionDrafts.delete(current.key);
    status.textContent = dirty() ? "未保存的草稿" : ""; update();
  };
  instance.onchange = () => void read();
  reload.onclick = async () => {
    if (dirty() && !await confirmAction("重新读取指令", "当前未保存的草稿将被文件内容替换。", "重新读取")) return;
    if (current) instructionDrafts.delete(current.key);
    await read({ discard: true });
  };
  save.onclick = async () => {
    if (save.disabled || !current || current.key !== keyFor(identity()) || !instanceAvailable(current.instanceId)) return;
    const value = current, content = input.value;
    saving = true; update(); status.textContent = "正在保存…";
    try {
      const result = await api("/api/settings/instructions", { instanceId: value.instanceId,
        content: instructionFileText(content, value.lineEnding),
        expectedRevision: value.revision });
      if (typeof result.content !== "string" || typeof result.revision !== "string"
          || normalizeInstructionText(result.content) !== content) throw new Error("保存结果无法确认，请重新读取。");
      // 关闭后重开仍可继续编辑；迟到回包只清理自己提交的版本，不能删除新窗口的新草稿。
      const draft = instructionDrafts.get(value.key);
      if (draft?.content === content && draft.revision === value.revision) instructionDrafts.delete(value.key);
      if (!alive()) return;
      current = { ...value, content, revision: result.revision };
      status.textContent = "已保存至原生指令文件。";
    } catch (error) { if (alive()) status.textContent = error.message; }
    finally { saving = false; if (alive()) update(); }
  };
  if (detectionButton) detectionButton.onclick = () => void detect(true);
  update();
  return {
    refreshInstance(id) {
      if (!alive() || saving || id && id !== instance.value) return;
      if (dirty()) { status.textContent = "原生指令已在其他客户端更新，未保存的草稿已保留，请重新读取后合并。"; return; }
      void read();
    },
    async show(page) {
      if (!environments) await detect();
      if (page === "instructions" && alive() && !current) await read();
    },
    dispose() { disposed = true; generation++; }
  };
}
export function mountClaudeRecapSettings({
  page, element, selectControl, setSelectOptions, api, getInstances, loadInstances, readOnly, authorizationScope
}) {
  const scope = authorizationScope();
  let disposed = false, generation = 0, selected = "", current = null, loading = false, saving = false, needsRead = false;
  const alive = () => !disposed && page.isConnected && authorizationScope() === scope;
  const instances = selectControl("claude-recap-instance", "离开期间回顾的 Claude 实例", [], "");
  const heading = element("label", "appearance-field");
  const instanceSlot = element("div");
  instanceSlot.append(instances);
  heading.append(element("span", "", "Claude 实例"), instanceSlot);
  const row = element("label", "appearance-field appearance-notification-row");
  const copy = element("span", "appearance-environment-copy");
  copy.append(element("strong", "", "离开期间回顾"),
    element("small", "muted", "离开后返回时自动回顾会话进展；关闭后保留已有回顾。"));
  const control = element("input", "appearance-notification-switch");
  control.id = "claude-recap-enabled"; control.type = "checkbox";
  control.setAttribute("role", "switch"); control.setAttribute("aria-label", "离开期间回顾");
  const status = element("p", "appearance-hint claude-recap-status", "连接远程服务后可读取 Claude 配置。");
  status.id = "claude-recap-status"; status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  control.setAttribute("aria-describedby", status.id);
  const reload = element("button", "text-button", "重新读取");
  reload.type = "button"; reload.id = "claude-recap-reload";
  const actions = element("div", "appearance-action-buttons");
  actions.append(reload);
  row.append(copy, control);
  page.append(heading, row,
    element("p", "appearance-hint", "与所选实例的 Claude CLI 共用设置，CLI 重新加载配置后生效。项目或启动参数的同名设置优先。"),
    status, actions);
  function sync() {
    control.checked = current?.enabled === true;
    control.indeterminate = !current;
    control.disabled = !alive() || !current || loading || saving || needsRead || readOnly() || current.readOnly === true;
    control.setAttribute("aria-busy", String(loading || saving));
    instances.disabled = loading || saving || !selected;
    reload.disabled = !alive() || loading || saving;
  }
  function checkedResult(result, id) {
    if (result?.instanceId !== id || result.backend !== "claude" || typeof result.enabled !== "boolean"
        || typeof result.revision !== "string" || !result.revision)
      throw new Error("回顾配置返回结果无法确认，请重新读取。");
    return result;
  }
  async function read(preferred = selected) {
    if (!alive() || saving) return;
    const version = ++generation;
    loading = true; current = null; needsRead = false; sync();
    status.textContent = "正在读取 Claude 回顾配置…";
    try {
      const detection = await loadInstances();
      if (!alive() || generation !== version) return;
      const choices = getInstances().filter(instance => instance.backend === "claude")
        .map(instance => ({ value: instance.id, label: instance.label, disabled: !!instance.error || instance.ready === false }));
      // 用户要求按实际 Claude 实例控制；保留用户选中的实例，不把失败或迟到结果套用到另一个 WSL 用户。
      if (preferred && (preferred === selected || preferred.startsWith("claude:"))
          && !choices.some(option => option.value === preferred)) {
        choices.push({ value: preferred, label: instances.selectOptions?.find(option => option.value === preferred)?.label || preferred,
          disabled: true });
      }
      selected = choices.some(option => option.value === preferred) ? preferred
        : choices.find(option => !option.disabled)?.value || choices[0]?.value || "";
      setSelectOptions(instances, choices, selected);
      if (!selected || choices.find(option => option.value === selected)?.disabled) {
        status.textContent = selected ? "所选 Claude 实例暂不可用，请重新检测会话实例。" : "尚未检测到 Claude 会话实例。";
        return;
      }
      const id = selected, result = checkedResult(await api("/api/settings/claude-recap", { instanceId: id }), id);
      if (!alive() || generation !== version || selected !== id) return;
      current = result;
      const initializationError = detection?.claudeRecapDefaults?.find(value => value.instanceId === id)?.error;
      status.textContent = result.readOnly || readOnly() ? "当前为只读访问"
        : result.explicit === false ? `默认关闭配置尚未生效，当前仍沿用 Claude CLI 默认开启。${initializationError || "请重新读取后重试。"}`
          : "";
    } catch (error) {
      if (alive() && generation === version) status.textContent = `无法读取回顾配置：${error.message}`;
    } finally {
      if (alive() && generation === version) { loading = false; sync(); }
    }
  }
  control.onchange = async () => {
    if (control.disabled || !alive() || !current || readOnly()) { sync(); return; }
    const before = current, id = selected, enabled = control.checked;
    if (enabled === before.enabled) return;
    saving = true; sync(); status.textContent = "正在保存回顾配置…";
    try {
      // 开关写原生 awaySummaryEnabled，不能只隐藏网页事件，也不能删除已经存在的回顾或历史消息。
      const result = checkedResult(await api("/api/settings/claude-recap", {
        instanceId: id, operation: "save", enabled, expectedRevision: before.revision
      }), id);
      if (result.enabled !== enabled) throw new Error("保存结果与所选开关不一致");
      if (!alive() || selected !== id) return;
      current = result;
      status.textContent = `已${enabled ? "开启" : "关闭"}离开期间回顾，CLI 重新加载配置后生效。`;
    } catch (error) {
      if (alive() && selected === id) {
        needsRead = true;
        status.textContent = `${error.message} 请重新读取核对，未自动重复保存。`;
      }
    } finally { saving = false; if (alive()) sync(); }
  };
  instances.onchange = () => void read(instances.value);
  reload.onclick = () => void read();
  sync();
  return { show: read, dispose() { disposed = true; generation++; } };
}
