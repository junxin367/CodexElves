// 输入区浮层：模型菜单渲染与新会话所选模型的记忆、浮层与审批区的视口适配，以及统一收起输入区菜单；模型菜单搜索词在 options-state.js。

import { $, button, element } from "../../ui/dom.js";
import { activePopup, fitPopup } from "../../ui/popup.js";
import { menuHorizontalLeft } from "../../ui/position.js";
import { updateComposer } from "../../core/actions.js";
import { creating } from "../../core/state.js";
import { fitClaudeUsageTip } from "./claude-usage.js";
import { closeContextUsageTip, fitContextUsageTip } from "./metrics.js";
import { currentEffort, currentModel, currentModelId } from "./model-choice.js";
import { rememberNewChatSettings } from "./new-chat-settings.js";
import {
  defaultModelEffort, modelChoiceVersion, modelQuery, models, selectedEffort, setModelChoiceVersion,
  setModelQuery, setSelectedEffort, setSelectedModel
} from "./options-state.js";
import { effortLabels } from "./prompt-editor.js";
import { fitGoalEditor } from "../goal/state.js";
import { isClaude } from "../session/capabilities.js";
import { scheduleDesktopObservation } from "../sidebar/list-state.js";

export function closeSkillMenu() {
  const menu = $("#skill-menu"); if (menu) menu.hidden = true;
  $("#prompt")?.setAttribute("aria-expanded", "false");
  $("#prompt")?.removeAttribute("aria-activedescendant");
}
export function closeModelMenu() {
  const menu = $("#model-settings"); if (menu) menu.hidden = true;
  $("#model-button")?.setAttribute("aria-expanded", "false");
}
export function closeComposerMenus({ keepContextUsage = false } = {}) {
  closeModelMenu();
  closeSkillMenu();
  if (!keepContextUsage) closeContextUsageTip();
}
export function renderModelMenu() {
  const menu = $("#model-settings");
  if (!menu) return;
  const focusIndex = [...menu.querySelectorAll("button")].indexOf(document.activeElement);
  const focusKey = menu.contains(document.activeElement) ? document.activeElement.dataset.choiceKey : null;
  const modelScroll = $(".model-options", menu)?.scrollTop || 0;
  const effortScroll = $(".effort-options", menu)?.scrollTop || 0;
  const searchFocused = document.activeElement === $("#model-search");
  const searchSelection = searchFocused ? [document.activeElement.selectionStart, document.activeElement.selectionEnd] : null;
  menu.replaceChildren();
  if (models.length > 8 || modelQuery || searchFocused) {
    const input = element("input", "model-search");
    input.id = "model-search"; input.type = "search"; input.value = modelQuery;
    input.placeholder = "搜索模型"; input.setAttribute("aria-label", "搜索模型"); input.autocomplete = "off";
    input.oninput = event => { if (!event.isComposing) { setModelQuery(input.value); renderModelMenu(); } };
    input.oncompositionend = () => { setModelQuery(input.value); renderModelMenu(); };
    menu.append(input);
  }
  const choice = (text, checked, action, description, key = text) => {
    const node = button("", text, action, "model-option");
    node.dataset.choiceKey = key;
    node.title = text;
    node.setAttribute("role", "menuitemradio"); node.setAttribute("aria-checked", String(checked));
    const labels = element("span", "model-option-label");
    labels.append(element("span", "", text));
    if (description) labels.append(element("small", "muted", description));
    node.append(labels, element("span", "choice-check", checked ? "✓" : ""));
    return node;
  };
  const applyModelMenuChoice = () => {
    setModelChoiceVersion(modelChoiceVersion + 1);
    renderModelMenu();
    updateComposer();
    closeComposerMenus();
    $("#prompt").focus();
  };
  const group = element("div", "model-options"); group.setAttribute("role", "group"); group.setAttribute("aria-label", "模型");
  const modelId = currentModelId(), matchedModel = currentModel(), selectedReasoning = currentEffort();
  const visibleModels = models.filter(model => `${model.displayName || ""} ${model.model}`.toLowerCase().includes(modelQuery.trim().toLowerCase()));
  for (const model of visibleModels) group.append(choice(model.displayName || model.model, matchedModel === model, () => {
    if (currentModel() !== model) {
      // 新建会话保留目标模型仍支持的记忆深度；已有 Claude 会话继续沿用原生切换规则。
      const previousEffort = isClaude() && !creating ? "" : currentEffort();
      setSelectedModel(model.model);
      setSelectedEffort(defaultModelEffort(model, previousEffort));
    }
    if (creating) rememberNewChatModel(model.model);
    applyModelMenuChoice();
  }, null, `model:${model.model}`));
  if (modelId && !matchedModel && modelId.toLowerCase().includes(modelQuery.trim().toLowerCase())) {
    const current = choice(modelId, true, () => {}, "当前会话模型，原生模型列表暂未提供", `model:${modelId}`);
    current.disabled = true; group.prepend(current);
  }
  if (!group.children.length) group.append(element("p", "popover-empty muted", models.length ? "没有匹配的模型，请修改搜索条件" : "暂无可选模型，请稍后重试"));
  menu.append(group);
  const efforts = element("div", "effort-options"); efforts.setAttribute("role", "group"); efforts.setAttribute("aria-label", "思考深度");
  const supportedEfforts = (currentModel()?.supportedReasoningEfforts || []).filter(effort => effort.reasoningEffort !== "minimal");
  for (const effort of supportedEfforts) {
    efforts.append(choice(effortLabels[effort.reasoningEffort] || effort.reasoningEffort,
      selectedReasoning === effort.reasoningEffort,
      () => {
        setSelectedEffort(effort.reasoningEffort);
        if (creating) rememberNewChatSettings({ model: currentModelId(), reasoningEffort: selectedEffort });
        applyModelMenuChoice();
      }, null, `effort:${effort.reasoningEffort}`));
  }
  if (!efforts.children.length) efforts.append(element("p", "popover-empty muted", "此模型暂无可选思考深度"));
  menu.append(efforts);
  fitComposerMenu(menu);
  group.scrollTop = modelScroll; efforts.scrollTop = effortScroll;
  if (searchFocused && $("#model-search")) {
    $("#model-search").focus({ preventScroll: true });
    $("#model-search").setSelectionRange(...searchSelection);
  } else if (focusIndex >= 0) {
    const buttons = [...menu.querySelectorAll("button")];
    (buttons.find(node => node.dataset.choiceKey === focusKey) || buttons[focusIndex])?.focus({ preventScroll: true });
  }
}
export function fitComposerMenu(menu) {
  if (menu.id === "model-settings") {
    const anchor = $("#model-button");
    if (!anchor || menu.hidden) return;
    const viewport = window.visualViewport;
    const top = viewport?.offsetTop || 0, left = viewport?.offsetLeft || 0;
    const width = viewport?.width || window.innerWidth, height = viewport?.height || window.innerHeight;
    const box = anchor.getBoundingClientRect(), gap = 4, padding = 8;
    const above = Math.max(0, box.top - top - gap - padding);
    const below = Math.max(0, top + height - box.bottom - gap - padding);
    const limit = Math.min(560, height * .8, Math.max(above, below));
    const compact = width <= 760 || window.matchMedia("(pointer:coarse)").matches;
    menu.style.width = "max-content";
    menu.style.minWidth = `${Math.min(compact ? 220 : 300, width - padding * 2)}px`;
    menu.style.maxWidth = `${Math.min(compact ? 280 : 350, width - padding * 2)}px`;
    menu.style.maxHeight = `${limit}px`;
    $(".effort-options", menu)?.style.setProperty("max-height", `${Math.max(0, (limit - 80) * .45)}px`);
    const upwards = below < menu.getBoundingClientRect().height && above > below;
    const available = Math.min(560, height * .8, upwards ? above : below);
    menu.style.maxHeight = `${available}px`;
    $(".effort-options", menu)?.style.setProperty("max-height", `${Math.max(0, (available - 80) * .45)}px`);
    const actual = menu.getBoundingClientRect();
    menu.style.left = `${menuHorizontalLeft(box, actual.width, { left, width }, padding)}px`;
    menu.style.top = `${upwards ? box.top - gap - actual.height : box.bottom + gap}px`;
    menu.dataset.direction = upwards ? "up" : "down";
    return;
  }
  // 命令/Skill 建议属于整个输入框的补全层，保留输入框对齐，不套用按钮菜单的左右展开规则。
  const top = $("#composer")?.getBoundingClientRect().top || 300;
  const viewport = window.visualViewport, height = viewport?.height || window.innerHeight;
  menu.style.maxHeight = `${Math.max(0, Math.min(480, height * .8, top - (viewport?.offsetTop || 0) - 16))}px`;
}
export function fitApprovalArea() {
  const root = $("#approval-area"), anchor = root?.parentElement;
  if (!root?.childElementCount || !anchor) return;
  const viewport = window.visualViewport, padding = 8, gap = 8;
  const top = viewport?.offsetTop || 0, height = viewport?.height || window.innerHeight;
  const anchorBottom = anchor.getBoundingClientRect().top - gap;
  let contentBottom = anchorBottom;
  // 用户要求问答卡贴在输入栏上方，可直接覆盖排队、目标等附属信息；普通权限审批仍避让这些内容。
  if (!root.querySelector(".question-card")) {
    for (let previous = anchor.previousElementSibling; previous; previous = previous.previousElementSibling) {
      if (previous.hidden) continue;
      const box = previous.getBoundingClientRect();
      if (box.width > 0 && box.height > 0) contentBottom = Math.min(contentBottom, box.top - gap);
    }
  }
  const bottom = Math.min(contentBottom, top + height - padding);
  root.style.setProperty("--approval-available-height", `${Math.max(0, bottom - top - padding)}px`);
  root.style.setProperty("--approval-lift", `${Math.max(0, anchorBottom - bottom)}px`);
}
export function refitComposerMenus() {
  fitPopup();
  fitContextUsageTip();
  fitClaudeUsageTip();
  fitApprovalArea();
  fitGoalEditor();
  for (const id of ["model-settings", "skill-menu"]) {
    const menu = $(`#${id}`); if (menu && !menu.hidden) fitComposerMenu(menu);
  }
}
// 视口尺寸或滚动变化时重新适配输入区浮层，并重新安排桌面可见性观测。
export function installComposerMenuViewportListeners() {
  window.addEventListener("resize", refitComposerMenus);
  window.addEventListener("resize", scheduleDesktopObservation);
  window.visualViewport?.addEventListener("resize", refitComposerMenus);
  window.visualViewport?.addEventListener("scroll", refitComposerMenus);
  document.addEventListener("scroll", event => {
    if (event.target?.id === "sidebar-content") scheduleDesktopObservation();
    fitApprovalArea();
    fitContextUsageTip();
    fitClaudeUsageTip();
    if (activePopup && !activePopup.node.contains(event.target)) fitPopup();
    const menu = $("#model-settings");
    if (menu && !menu.hidden && !menu.contains(event.target)) fitComposerMenu(menu);
  }, true);
}

export function rememberNewChatModel(model) {
  rememberNewChatSettings({ model, reasoningEffort: currentEffort() });
}
