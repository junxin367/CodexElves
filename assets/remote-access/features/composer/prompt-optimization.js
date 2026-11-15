// 提示词优化：调用设置的优化模型改写输入，支持恢复原文与设置对话框。

import { isAndroidContainer } from "../../platform.js";
import { $, button, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import { activePopup, closePopup } from "../../ui/popup.js";
import { selectControl, setSelectOptions } from "../../ui/select.js";
import { closeComposerMenus, updateComposer } from "../../core/actions.js";
import { busy, selectionVersion, session, stopping, turns } from "../../core/state.js";
import { randomId } from "../../core/util.js";
import { api } from "../api.js";
import { draftKey, rememberDraft } from "./drafts.js";
import { tokenCount } from "./metrics.js";
import { currentModelId } from "./model-choice.js";
import { setPromptSelection, writePromptContent } from "./prompt-editor.js";
import { selectedSkills, setSelectedSkills } from "./skill-catalog.js";
import { pending } from "../conversation/pending-state.js";
import { messageCopyText } from "../messages/user-content.js";
import { editingQueuedMessage } from "../queue/state.js";
import { privateInputScope, threadReadOnly } from "../session/capabilities.js";

export const promptOptimizations = new Map();
export let promptOptimizationSettingsDialog = null;
function promptOptimizationScope() {
  return JSON.stringify([draftKey(), editingQueuedMessage?.editId || null]);
}
function promptOptimizationSignature() {
  return JSON.stringify([$("#prompt").value, selectedSkills]);
}
export function cancelPromptOptimization(record) {
  if (record.state !== "pending") return;
  record.state = "cancelled";
  if (session?.device.id === record.deviceId)
    void api("/api/action/cancelPromptOptimize", { requestId: record.id }, randomId()).catch(() => {});
}
export function updatePromptOptimization() {
  const control = $("#prompt-optimize");
  if (!control) return;
  // 用户要求提示词优化独立于会话类型：Claude 实例也显示，复用独立优化服务和桌面设置，不依赖会话执行引擎能力。
  control.hidden = false;
  const scope = promptOptimizationScope(), signature = promptOptimizationSignature();
  for (const record of promptOptimizations.values()) {
    if (record.state === "pending" && (record.scope !== scope || record.version !== selectionVersion
        || record.deviceId !== session?.device.id || record.signature !== signature
        || $("#prompt").disabled || busy || stopping || pending))
      cancelPromptOptimization(record);
  }
  let record = promptOptimizations.get(scope);
  if (record?.state === "optimized" && record.signature !== signature) {
    promptOptimizations.delete(scope); record = null;
  }
  const loading = record?.state === "pending", optimized = record?.state === "optimized";
  control.dataset.state = loading ? "loading" : optimized ? "optimized" : "idle";
  control.setAttribute("aria-busy", String(loading));
  control.disabled = session?.capabilities?.promptOptimization !== true || threadReadOnly()
    || $("#prompt").disabled || busy || stopping || !!pending;
  const label = loading ? "取消提示词优化" : optimized ? "恢复原提示词" : "优化提示词";
  control.setAttribute("aria-label", label);
  control.title = session?.capabilities?.promptOptimization !== true
    ? "当前远程服务尚不支持提示词优化"
    : `${label}（使用 CodexElves 桌面端设置${session?.capabilities?.promptOptimizationSettings === true
      && !isAndroidContainer() && !matchMedia("(max-width: 760px)").matches ? "，右键设置" : ""}）`;
}
export async function openPromptOptimizationSettings() {
  // 用户要求仅 PC 网页和 Windows 客户端右键打开，手机/Android 的点击和长按保持原行为。
  if (isAndroidContainer() || matchMedia("(max-width: 760px)").matches || !session) return;
  if (session.readOnly) { notice("当前设备为只读模式"); return; }
  if (session.capabilities?.promptOptimizationSettings !== true) {
    notice("当前服务尚不支持远程优化设置，请更新远程服务后再试", true); return;
  }
  if (promptOptimizationSettingsDialog?.isConnected) { promptOptimizationSettingsDialog.focus(); return; }
  closePopup(); closeComposerMenus();
  const scope = privateInputScope(session), dialog = element("dialog", "prompt-optimization-settings");
  dialog.id = "prompt-optimization-settings";
  dialog.setAttribute("aria-label", "提示词优化设置");
  const header = element("header", "prompt-optimization-settings-header");
  const heading = element("div"), supplier = element("small", "muted", "正在读取设置…");
  heading.append(element("h2", "", "提示词优化"), supplier);
  const close = button("×", "关闭提示词优化设置", () => { if (!saving) dialog.close(); }, "icon-button");
  header.append(heading, close);
  const body = element("div", "prompt-optimization-settings-body");
  const status = element("p", "prompt-optimization-settings-status muted", "正在读取设置…");
  status.setAttribute("role", "status");
  const footer = element("footer", "prompt-optimization-settings-footer");
  const cancel = button("取消", "取消修改", () => { if (!saving) dialog.close(); });
  const save = button("保存", "保存提示词优化设置", () => void persist(), "primary");
  save.disabled = true; footer.append(cancel, save); dialog.append(header, body, status, footer);
  let saving = false, draft = null, revision = "", activeStyle = "", textarea = null;
  let modelControl = null, effortControl = null, styleControl = null, contextControl = null;
  const current = () => dialog.isConnected && promptOptimizationSettingsDialog === dialog
    && session && privateInputScope(session) === scope && !session.readOnly;
  dialog.oncancel = event => {
    event.preventDefault();
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup(true);
    else if (!saving) dialog.close();
  };
  let backdrop = false;
  const outside = event => {
    const box = dialog.getBoundingClientRect();
    return event.target === dialog && (event.clientX < box.left || event.clientX > box.right
      || event.clientY < box.top || event.clientY > box.bottom);
  };
  dialog.onpointerdown = event => { backdrop = outside(event); };
  dialog.onclick = event => { if (backdrop && outside(event) && !saving) dialog.close(); backdrop = false; };
  dialog.onclose = () => {
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup();
    if (promptOptimizationSettingsDialog === dialog) promptOptimizationSettingsDialog = null;
    dialog.remove();
  };
  document.body.append(dialog); promptOptimizationSettingsDialog = dialog; dialog.showModal();
  async function persist() {
    if (!current() || saving || !draft || save.disabled) return;
    draft.prompts[activeStyle] = textarea.value;
    const empty = Object.entries(draft.prompts).find(([, text]) => !text.trim());
    if (!modelControl.value.trim() || !effortControl.value || empty
        || Object.values(draft.prompts).some(text => Array.from(text).length > 20000)) {
      status.textContent = empty ? "各风格系统提示词不能为空"
        : !modelControl.value.trim() ? "请选择模型或输入模型名称" : "请检查思考深度及模板长度（最多 20000 字）";
      status.classList.add("error-text"); return;
    }
    const settings = { ...draft, model: modelControl.value, reasoningEffort: effortControl.value,
      style: styleControl.value, includeRecentContext: contextControl.checked };
    saving = true; save.disabled = cancel.disabled = close.disabled = true;
    status.classList.remove("error-text"); status.textContent = "正在保存…";
    try {
      await api("/api/settings/prompt-optimization", { settings, expectedRevision: revision });
      if (!current()) return;
      dialog.close(); notice("提示词优化设置已保存"); updatePromptOptimization();
    } catch (error) {
      if (current()) { status.textContent = error.message || "保存失败，修改内容已保留"; status.classList.add("error-text"); }
    } finally {
      saving = false; save.disabled = cancel.disabled = close.disabled = false;
    }
  }
  try {
    const data = await api("/api/settings/prompt-optimization", {});
    if (!current()) { dialog.close(); return; }
    revision = data.revision;
    draft = { ...data.defaults, ...data.settings,
      prompts: { ...data.defaults.prompts, ...data.settings.prompts } };
    supplier.textContent = `当前供应商：${data.supplier || "当前项目供应商"}`;
    status.textContent = "";
    const definitions = [{ value: "concise", label: "简洁" }, { value: "structured", label: "结构化" },
      { value: "coding", label: "编程" }];
    const grid = element("div", "prompt-optimization-settings-grid");
    const field = (label, control) => {
      const node = element("div", "prompt-optimization-settings-field");
      node.append(element("span", "", label), control); grid.append(node);
    };
    const available = Array.isArray(data.models) ? data.models : [];
    const options = available.map(model => ({ value: model.value,
      label: model.label && model.label !== model.value ? `${model.label} · ${model.value}` : model.value }));
    const selected = draft.model || (options.some(option => option.value === currentModelId()) ? currentModelId() : "")
      || data.defaultModel || options[0]?.value || "";
    if (selected && !options.some(option => option.value === selected)) options.unshift({ value: selected, label: selected });
    if (!options.length) options.push({ value: "", label: "选择模型或输入名称" });
    modelControl = selectControl("prompt-optimization-model", "优化模型", options, selected);
    modelControl.selectSearchable = true;
    modelControl.selectSearchPlaceholder = "搜索或输入模型名称";
    modelControl.selectSearchLabel = "搜索或输入优化模型";
    modelControl.selectCustomOption = value => {
      value = value.trim();
      return value && Array.from(value).length <= 256 && !value.includes("\0")
        ? { value, label: value, description: "使用此模型名称" } : null;
    };
    effortControl = selectControl("prompt-optimization-effort", "优化思考深度", [], "");
    const effortLabels = { low: "轻度", medium: "中", high: "高", xhigh: "极高", max: "最高" };
    const syncEffort = preferred => {
      const configured = available.find(model => model.value === modelControl.value)?.efforts;
      const name = modelControl.value.toLowerCase().split("/").at(-1);
      const values = configured?.length ? configured : /^(gpt(?:-|$)|o\d)/.test(name)
        ? ["low", "medium", "high", "xhigh"] : /claude|sonnet|opus|haiku/.test(name)
          ? ["low", "medium", "high", "xhigh", "max"] : ["medium"];
      const value = values.includes(preferred) ? preferred : values.includes("medium") ? "medium" : values[0];
      setSelectOptions(effortControl, values.map(value => ({ value, label: effortLabels[value] || value })), value);
    };
    syncEffort(draft.reasoningEffort);
    modelControl.onchange = () => syncEffort(effortControl.value);
    styleControl = selectControl("prompt-optimization-style", "优化风格", definitions, draft.style);
    contextControl = element("input", "appearance-notification-switch");
    contextControl.type = "checkbox"; contextControl.checked = draft.includeRecentContext;
    contextControl.setAttribute("role", "switch");
    contextControl.setAttribute("aria-label", "优化时附带最近一轮上下文");
    const context = element("label", "prompt-optimization-context");
    context.append(element("span", "", "优化时附带"), contextControl);
    field("模型", modelControl); field("思考深度", effortControl);
    field("优化风格", styleControl); field("最近一轮上下文", context);
    const promptArea = element("div", "prompt-optimization-settings-prompt");
    const promptHeader = element("div", "prompt-optimization-prompt-heading"), promptTitle = element("span");
    const reset = button("恢复默认", "恢复当前风格的默认系统提示词", () => {
      textarea.value = data.defaults.prompts[activeStyle]; textarea.focus();
    }, "text-button");
    promptHeader.append(promptTitle, reset);
    textarea = element("textarea"); textarea.spellcheck = false;
    textarea.maxLength = 40000;
    const switchStyle = () => {
      if (activeStyle) draft.prompts[activeStyle] = textarea.value;
      activeStyle = styleControl.value;
      textarea.value = draft.prompts[activeStyle];
      const title = `${definitions.find(style => style.value === activeStyle).label}系统提示词`;
      promptTitle.textContent = title; textarea.setAttribute("aria-label", title);
    };
    styleControl.onchange = switchStyle; switchStyle();
    promptArea.append(promptHeader, textarea); body.append(grid, promptArea);
    save.disabled = false; modelControl.focus();
  } catch (error) {
    if (current()) { status.textContent = error.message || "无法读取优化设置"; status.classList.add("error-text"); }
  }
}
function promptOptimizationRecentContext() {
  for (const turn of [...turns.values()].reverse()) {
    const user = turn.items?.find(item => item.type === "userMessage");
    const assistant = [...new Set((turn.items || []).filter(item => item.type === "agentMessage")
      .map(item => messageCopyText(item).trim()).filter(Boolean))].join("\n\n");
    const text = user && messageCopyText(user).trim();
    if (text && assistant) return { user: text, assistant };
  }
  return null;
}
export async function activatePromptOptimization() {
  if (threadReadOnly() || $("#prompt-optimize").disabled) return;
  closePopup(); closeComposerMenus();
  const scope = promptOptimizationScope(), current = promptOptimizations.get(scope), prompt = $("#prompt");
  if (current?.state === "pending") {
    cancelPromptOptimization(current); updatePromptOptimization(); return;
  }
  if (current?.state === "optimized" && current.signature === promptOptimizationSignature()) {
    setSelectedSkills(current.originalSkills.map(skill => ({ ...skill })));
    writePromptContent(prompt, current.originalText);
    promptOptimizations.delete(scope);
    rememberDraft(); updateComposer(); prompt.focus({ preventScroll: true });
    setPromptSelection(prompt, prompt.value.length); return;
  }
  const originalText = prompt.value;
  if (!originalText.trim()) { notice("请先输入要优化的提示词"); return; }
  if (Array.from(originalText).length > 100000) { notice("提示词过长，无法优化", true); return; }
  const record = {
    id: randomId(), scope, version: selectionVersion, deviceId: session.device.id,
    state: "pending", originalText, originalSkills: selectedSkills.map(skill => ({ ...skill })),
    signature: promptOptimizationSignature(),
  };
  promptOptimizations.set(scope, record); updatePromptOptimization();
  const isCurrent = () => record.state === "pending" && record.scope === promptOptimizationScope()
    && record.version === selectionVersion && session?.device.id === record.deviceId
    && record.signature === promptOptimizationSignature();
  try {
    const recentContext = promptOptimizationRecentContext();
    let receipt = await api("/api/action/optimizePrompt", {
      input: originalText, model: currentModelId(), ...(recentContext ? { recentContext } : {}),
    }, record.id);
    while (receipt.state === "pending" && isCurrent()) {
      await new Promise(resolve => setTimeout(resolve, 750));
      if (isCurrent()) receipt = await api(`/api/receipts/${record.id}`);
    }
    if (!isCurrent()) return;
    if (receipt.state !== "completed")
      throw new Error(receipt.result?.message || "未能确认提示词优化请求，原输入未改动");
    let result = receipt.result;
    while (result?.state === "pending" && isCurrent()) {
      await new Promise(resolve => setTimeout(resolve, 750));
      if (isCurrent()) result = await api("/api/read/promptOptimization", { requestId: record.id });
    }
    if (!isCurrent()) return;
    if (result?.state === "cancelled") { record.state = "cancelled"; return; }
    if (result?.state !== "completed" || !result.text?.trim())
      throw new Error(result?.message || "提示词优化失败，原输入未改动");
    writePromptContent(prompt, result.text);
    record.state = "optimized"; record.signature = promptOptimizationSignature();
    rememberDraft(); updateComposer(); prompt.focus({ preventScroll: true });
    setPromptSelection(prompt, prompt.value.length);
    notice(`提示词已优化，再次点击可恢复原文${result.totalTokens > 0 ? `。当前使用 ${tokenCount(result.totalTokens)} Token` : ""}`);
  } catch (error) {
    if (isCurrent()) {
      cancelPromptOptimization(record); record.state = "failed";
      notice(error.message || "提示词优化失败，原输入未改动", true);
    }
  } finally {
    updatePromptOptimization();
  }
}
