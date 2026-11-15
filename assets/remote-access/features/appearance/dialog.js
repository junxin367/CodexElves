// 设置对话框：外观、字体、颜色选择器、通知与缓存状态等设置页面。

import { exportAndroidDiagnostics, sendAndroidDiagnostics } from "../../diagnostics.js";
import { mountInstancePermissions } from "../../permissions.js";
import {
  getAndroidAppInfo, getAndroidNotificationSettings, isAndroidContainer,
  openAndroidNotificationSystemSettings, postPlatformMessage, requestAndroidNotificationPermission,
  setAndroidNotificationSettings, subscribeAndroidNotificationSettings
} from "../../platform.js";
import { getPwaCacheState, retryPwaCache } from "../../pwa.js";
import { confirmAction } from "../../ui/dialog.js";
import { $, button, element } from "../../ui/dom.js";
import { icon } from "../../ui/icons.js";
import { notice } from "../../ui/notice.js";
import { activePopup, closePopup, fitPopup, setActivePopup } from "../../ui/popup.js";
import { selectControl, setSelectOptions, setSelectValue } from "../../ui/select.js";
import { connected, creating, session } from "../../core/state.js";
import { api } from "../api.js";
import {
  appearance, appearanceThemes, applyAppearance, conversationWidthMax, conversationWidthMin,
  currentFileReadingFontSize, defaultAppearance, deleteRemoteBackground, fileReadingFontDefault,
  fileReadingFontMax, fileReadingFontMin, mountAutomaticThemeLightSetting, projectThreadLimits,
  readRemoteBackgroundFile, remoteBackground, remoteBackgroundBusy, remoteBackgroundError,
  remoteBackgroundOperation, setAppearance, setAppearanceThemeMode, setRemoteBackground,
  setRemoteBackgroundBusy, setRemoteBackgroundError, setRemoteBackgroundOperation,
  setSyncAppearanceThemeControls, syncAppearanceThemeControls, syncRemoteBackground,
  validFontFamily, writeRemoteBackground
} from "./theme.js";
import { closeComposerMenus } from "../composer/composer-menus.js";
import { updateComposer } from "../composer/composer.js";
import { setCreationBackend } from "../composer/new-chat.js";
import { rememberInstancePermissionCatalog } from "../composer/permissions.js";
import { privateInputScope } from "../session/capabilities.js";
import {
  activeInstanceId, availableSessionInstances, loadSessionInstances, renderSessionInstances
} from "../session/instances.js";
import { mountClaudeRecapSettings, mountRemoteSettings } from "../settings/remote-settings.js";
import { sessionMenu } from "../shell.js";
import { loadSidebar, projectVisibleThreadCounts, renderSidebar } from "../sidebar/list.js";

function renderPwaCacheState() {
  const root = $("#appearance-cache-state");
  if (!root) return;
  const state = getPwaCacheState();
  root.hidden = state.status === "ready" || state.status === "unsupported";
  if (root.hidden) {
    root.replaceChildren();
    return;
  }
  root.replaceChildren(element("span", "muted", state.message));
  if (state.retryable) root.append(button("重试", "重新检查离线页面缓存", async () => {
    try { await retryPwaCache(); } catch (error) { notice(error.message || "缓存检查失败，请稍后重试。", true); }
  }, "text-button"));
}
// 离线缓存状态变化时刷新设置页中的缓存说明。
export function installPwaCacheStateListener() {
  window.addEventListener("codex-elves-pwa-state", () => renderPwaCacheState());
}
const appearanceFontFamilies = [
  "Arial", "Arial Black", "Bahnschrift", "Calibri", "Cambria", "Cambria Math", "Candara",
  "Cascadia Code", "Cascadia Mono", "Comic Sans MS", "Consolas", "Constantia", "Corbel",
  "Courier New", "Fira Code", "Georgia", "Helvetica", "Helvetica Neue", "Inter", "JetBrains Mono",
  "Liberation Mono", "Menlo", "Microsoft JhengHei", "Microsoft YaHei", "Monaco", "Noto Sans",
  "Noto Sans SC", "Noto Serif SC", "PingFang SC", "Segoe UI", "SF Mono", "SimHei", "SimSun",
  "Source Code Pro", "Tahoma", "Times New Roman", "Trebuchet MS", "Verdana"
];
function appearanceFontOptions(current = "") {
  // 用户要求字体使用 Codex 风格 Select；在当前设备检测常用字体，不请求本地字体权限，也保留已保存的自定义字体。
  const sample = "AaBbWw0123456789汉字", bases = ["monospace", "sans-serif", "serif"];
  let fonts = appearanceFontFamilies;
  try {
    const canvas = document.createElement("canvas").getContext("2d");
    if (canvas) {
      const measure = family => { canvas.font = `16px ${family}`; return canvas.measureText(sample).width; };
      const widths = bases.map(measure);
      fonts = fonts.filter(family => bases.some((base, index) =>
        Math.abs(measure(`${JSON.stringify(family)},${base}`) - widths[index]) > .01));
    }
  } catch {}
  if (current && validFontFamily(current) && !fonts.includes(current)) fonts = [...fonts, current];
  return [{ value: "", label: "系统" },
    ...[...fonts].sort((left, right) => left.localeCompare(right)).map(family => ({ value: family, label: family }))];
}
// 用户要求恢复之前的预设颜色，保留原有 12 个值，放在取色器的色相条下方。
const appearanceColorPresets = ["#171717", "#3f3f46", "#71717a", "#d4d4d8", "#f4f4f5", "#ffffff",
  "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#16a34a"];
function bindAppearanceColorValue(input, getColor, onChange) {
  // 用户要求 HEX 可直接修改、只有色块打开取色器；不保存输入到一半或非法的颜色，失焦时恢复实际色值。
  const reset = () => { input.value = getColor().toUpperCase(); input.setAttribute("aria-invalid", "false"); };
  input.oninput = () => {
    const value = input.value.trim(), valid = /^#[0-9a-f]{6}$/i.test(value);
    input.setAttribute("aria-invalid", String(!valid));
    if (valid) onChange(value.toLowerCase());
  };
  input.onblur = reset;
  input.onkeydown = event => {
    if (!["Enter", "Escape"].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    if (event.key === "Escape") reset();
    if (input.getAttribute("aria-invalid") !== "true") input.blur();
  };
  return () => { if (document.activeElement !== input) reset(); };
}
function bindAppearanceFontSize(input, min, max, getValue, onChange) {
  // 用户要求所有字号统一为数字输入框；保留原滑条/输入框的范围，Enter 或失焦才应用，不能把空值或小数写入设置。
  bindAppearanceNumberValue(input, min, max, getValue, onChange, "px");
}
function bindAppearanceNumberValue(input, min, max, getValue, onChange, unit) {
  // 对比度与原字号共用数值确认交互，避免空值变成零；输入阶段只标记错误，不改已保存的外观。
  input.setAttribute("aria-description", `输入 ${min}～${max}${unit}，按 Enter 或离开输入框应用`);
  const valid = () => input.value.trim() !== "" && Number.isInteger(Number(input.value))
    && Number(input.value) >= min && Number(input.value) <= max;
  const reset = () => { input.value = String(getValue()); input.setAttribute("aria-invalid", "false"); };
  const commit = () => {
    if (valid() && Number(input.value) !== getValue()) onChange(Number(input.value));
    reset();
  };
  input.oninput = () => input.setAttribute("aria-invalid", String(!valid()));
  input.onchange = commit;
  input.onblur = commit;
  input.onkeydown = event => {
    if (!["Enter", "Escape"].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    if (event.key === "Escape") reset();
    else commit();
    input.blur();
  };
}
function appearanceHexToHsv(hex) {
  if (!/^#[0-9a-f]{6}$/i.test(hex)) return null;
  const [r, g, b] = [1, 3, 5].map(index => parseInt(hex.slice(index, index + 2), 16) / 255);
  const high = Math.max(r, g, b), low = Math.min(r, g, b), delta = high - low;
  const hue = !delta ? 0 : high === r ? ((g - b) / delta) % 6 : high === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
  return { h: (hue * 60 + 360) % 360, s: high ? delta / high : 0, v: high };
}
function appearanceHsvToHex({ h, s, v }) {
  const hue = ((h % 360) + 360) % 360 / 60, chroma = v * s;
  const part = chroma * (1 - Math.abs(hue % 2 - 1)), base = v - chroma;
  const rgb = [[chroma, part, 0], [part, chroma, 0], [0, chroma, part],
    [0, part, chroma], [part, 0, chroma], [chroma, 0, part]][Math.floor(hue)];
  return "#" + rgb.map(channel => Math.round((channel + base) * 255).toString(16).padStart(2, "0")).join("");
}
function openAppearanceColorPicker(trigger, label, color, onChange) {
  if (activePopup?.trigger === trigger) { closePopup(true); return; }
  closePopup(); closeComposerMenus();
  // 用户要求只点击色块触发浮层；连续取色、色相条与预设色共用保存逻辑，HEX 字段可独立编辑。
  const node = element("div", "ui-popup appearance-color-picker");
  node.id = "active-popup"; node.setAttribute("role", "dialog"); node.setAttribute("aria-label", `${label}颜色选择`);
  const plane = element("div", "appearance-color-plane"), cursor = element("span", "appearance-color-cursor");
  plane.tabIndex = 0; plane.setAttribute("role", "slider");
  plane.setAttribute("aria-label", `${label}饱和度与亮度`);
  plane.setAttribute("aria-description", "左右方向键调整饱和度，上下方向键调整亮度，按住 Shift 加快调整");
  plane.setAttribute("aria-valuemin", "0"); plane.setAttribute("aria-valuemax", "100");
  cursor.setAttribute("aria-hidden", "true"); plane.append(cursor);
  const hue = element("input", "appearance-color-hue"), hex = element("input", "color-hex");
  hue.type = "range"; hue.min = "0"; hue.max = "360"; hue.step = "1"; hue.setAttribute("aria-label", `${label}色相`);
  hex.type = "text"; hex.maxLength = 7; hex.spellcheck = false; hex.autocomplete = "off";
  hex.setAttribute("aria-label", `${label}色值`);
  const footer = element("div", "appearance-color-footer"), swatch = element("span", "appearance-color-dot");
  const presets = element("div", "appearance-color-presets");
  presets.setAttribute("role", "group"); presets.setAttribute("aria-label", `${label}预设颜色`);
  footer.append(swatch, hex); node.append(plane, hue, presets, footer);
  let hsv = appearanceHexToHsv(color), currentColor = color.toLowerCase();
  const presetButtons = appearanceColorPresets.map(value => {
    const preset = button("", `${label} ${value.toUpperCase()}`, () => {
      const next = appearanceHexToHsv(value);
      hsv = { ...next, h: next.s ? next.h : hsv.h }; commit();
    }, "appearance-color-preset");
    preset.dataset.color = value; preset.style.setProperty("--swatch", value);
    presets.append(preset);
    return preset;
  });
  const render = () => {
    plane.style.setProperty("--color-hue", hsv.h);
    cursor.style.left = `${hsv.s * 100}%`; cursor.style.top = `${(1 - hsv.v) * 100}%`;
    cursor.style.background = currentColor; swatch.style.background = currentColor;
    plane.setAttribute("aria-valuenow", String(Math.round(hsv.s * 100)));
    plane.setAttribute("aria-valuetext", `饱和度 ${Math.round(hsv.s * 100)}%，亮度 ${Math.round(hsv.v * 100)}%`);
    hue.value = String(hsv.h);
    for (const preset of presetButtons) preset.setAttribute("aria-pressed", String(preset.dataset.color === currentColor));
    if (document.activeElement !== hex) { hex.value = currentColor.toUpperCase(); hex.setAttribute("aria-invalid", "false"); }
  };
  const commit = () => {
    currentColor = appearanceHsvToHex(hsv); onChange(currentColor); render();
  };
  const move = event => {
    const bounds = plane.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    hsv.s = Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width));
    hsv.v = 1 - Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height));
    commit();
  };
  plane.onpointerdown = event => {
    if (!event.isPrimary || event.button !== 0) return;
    event.preventDefault(); plane.focus({ preventScroll: true }); plane.setPointerCapture(event.pointerId); move(event);
  };
  plane.onpointermove = event => { if (plane.hasPointerCapture(event.pointerId)) move(event); };
  plane.onpointerup = event => {
    if (plane.hasPointerCapture(event.pointerId)) { move(event); plane.releasePointerCapture(event.pointerId); }
  };
  plane.onkeydown = event => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const step = event.shiftKey ? .1 : .01;
    if (["ArrowLeft", "ArrowRight"].includes(event.key))
      hsv.s = Math.max(0, Math.min(1, hsv.s + (event.key === "ArrowRight" ? step : -step)));
    else hsv.v = Math.max(0, Math.min(1, hsv.v + (event.key === "ArrowUp" ? step : -step)));
    commit();
  };
  hue.oninput = () => { hsv.h = Number(hue.value); commit(); };
  hex.oninput = () => {
    const value = appearanceHexToHsv(hex.value);
    hex.setAttribute("aria-invalid", String(!value));
    if (value) { hsv = { ...value, h: value.s ? value.h : hsv.h }; commit(); }
  };
  trigger.popupWidth = 240; trigger.popupMaxHeight = 360;
  trigger.setAttribute("aria-expanded", "true"); trigger.setAttribute("aria-controls", node.id);
  setActivePopup({ node, trigger, syncColor(value) {
    if (value.toLowerCase() !== currentColor) {
      const next = appearanceHexToHsv(value);
      if (next) { hsv = { ...next, h: next.s ? next.h : hsv.h }; currentColor = value.toLowerCase(); }
    }
    render();
  } });
  if (typeof node.showPopover === "function") node.setAttribute("popover", "manual");
  (trigger.closest("dialog") || document.body).append(node);
  render();
  if (node.hasAttribute("popover")) node.showPopover();
  fitPopup(); plane.focus({ preventScroll: true });
}
export function openAppearance() {
  closeComposerMenus(); closePopup(); sessionMenu?.close();
  const trigger = document.activeElement;
  const androidContainer = isAndroidContainer();
  const previousDialog = $("#appearance-dialog");
  if (previousDialog?.open) previousDialog.close();
  previousDialog?.remove();
  const dialog = element("dialog", "appearance-dialog appearance-screen");
  dialog.id = "appearance-dialog";
  dialog.setAttribute("aria-labelledby", "appearance-title");
  dialog.innerHTML = `<div class="appearance-heading"><h2 id="appearance-title">设置</h2><button type="button" class="icon-button" title="关闭设置" aria-label="关闭设置"></button></div>
    <div class="appearance-layout">
    <nav class="appearance-nav" aria-label="设置导航">
      <!-- 用户确认设置导航按效果稿分组：“外观”仍紧随“会话”，保留全部入口的原相对顺序。 -->
      <div class="appearance-nav-group" role="group" aria-labelledby="appearance-nav-personal-title">
        <h3 id="appearance-nav-personal-title" class="appearance-nav-group-title">个人</h3>
        <button type="button" id="appearance-general-entry" class="appearance-nav-item" aria-controls="appearance-general-page" aria-selected="true"><span class="appearance-nav-icon"></span><span>会话</span></button>
        <button type="button" id="appearance-customization-entry" class="appearance-nav-item" title="打开外观" aria-label="打开外观" aria-controls="appearance-customization-page" aria-selected="false"><span class="appearance-nav-icon"></span><span>外观</span></button>
      </div>
      <div class="appearance-nav-group" role="group" aria-labelledby="appearance-nav-connections-title">
        <h3 id="appearance-nav-connections-title" class="appearance-nav-group-title">会话与连接</h3>
        <button type="button" id="appearance-types-entry" class="appearance-nav-item" aria-controls="appearance-types-page" aria-selected="false"><span class="appearance-nav-icon"></span><span>会话实例</span></button>
        <button type="button" id="appearance-permissions-entry" class="appearance-nav-item" aria-controls="appearance-permissions-page" aria-selected="false"><span class="appearance-nav-icon"></span><span>权限</span></button>
        <button type="button" id="appearance-instructions-entry" class="appearance-nav-item" aria-controls="appearance-instructions-page" aria-selected="false"><span class="appearance-nav-icon"></span><span>自定义指令</span></button>
      </div>
      <!-- 通知事件、诊断日志及其“工具”分组仅在 Android App 显示，PC 与手机浏览器不留空分组。 -->
      <div id="appearance-nav-tools" class="appearance-nav-group" role="group" aria-labelledby="appearance-nav-tools-title" hidden>
        <h3 id="appearance-nav-tools-title" class="appearance-nav-group-title">工具</h3>
        <button type="button" id="appearance-notifications-entry" class="appearance-nav-item" title="打开通知事件" aria-label="打开通知事件" aria-controls="appearance-notifications-page" aria-selected="false" hidden><span class="appearance-nav-icon"></span><span>通知事件</span></button>
        <button type="button" id="appearance-diagnostics-entry" class="appearance-nav-item" title="打开诊断日志" aria-label="打开诊断日志" aria-controls="appearance-diagnostics-page" aria-selected="false" hidden><span class="appearance-nav-icon"></span><span>诊断日志</span></button>
      </div>
    </nav>
    <div class="appearance-panels">
    <section id="appearance-general-page" class="appearance-body appearance-settings-page">
    <h3 class="appearance-panel-title">会话</h3>
    <section class="appearance-section"><h4>显示</h4><div class="appearance-card">
    <label class="appearance-field appearance-width-field"><span class="appearance-field-copy"><span>会话正文宽度上限</span><small>默认 950px，仅在右侧可用空间不足时自动收窄。</small></span><span class="appearance-number"><input id="appearance-conversation-width" type="number" min="${conversationWidthMin}" max="${conversationWidthMax}" step="10" value="${appearance.conversationWidth}" aria-label="会话正文宽度上限"><span>px</span></span></label>
    <div class="appearance-field appearance-thread-limit-field"><span>每个项目显示会话数</span><div id="appearance-project-thread-limit-slot"></div></div>
    <div id="appearance-cache-state" class="appearance-field" role="status"></div>
    </div></section>
    <section class="appearance-section"><h4>Claude</h4>
    <!-- 用户要求通过设置开关控制离开期间回顾；按实例修改原生配置，历史回顾继续保留。 -->
    <div id="appearance-claude-recap" class="appearance-card"></div></section>
    </section>
    <section id="appearance-types-page" class="appearance-body appearance-settings-page" hidden>
    <!-- 用户要求统一显示为“会话实例”：导航、页头与指令选择保持一致，逐实例开关与自动检测行为不变。 -->
    <h3 class="appearance-panel-title">会话实例</h3>
    <p class="appearance-hint">按实例启用会话，仅影响当前设备；关闭不会停止任务。</p>
    <div id="appearance-session-instances"></div>
    </section>
    <section id="appearance-permissions-page" class="appearance-body appearance-settings-page" hidden>
    <h3 class="appearance-panel-title">权限</h3>
    <div id="instance-permissions"></div>
    </section>
    <section id="appearance-instructions-page" class="appearance-body appearance-settings-page" hidden>
    <h3 class="appearance-panel-title">自定义指令</h3>
    <div class="appearance-card appearance-instructions-card">
    <!-- 用户要求只选择实例并编辑用户级全局指令，不再提供类型、环境、作用范围和项目选择。 -->
    <div class="instruction-selectors"><label>会话实例<div id="instruction-instance-slot"></div></label></div>
    <p id="instruction-path" class="appearance-hint instruction-path"></p>
    <textarea id="instruction-content" class="instruction-content" aria-label="自定义指令内容" spellcheck="false" disabled></textarea>
    <p id="instruction-status" class="appearance-hint" role="status"></p>
    <p class="appearance-hint">保存至所选实例的用户级全局指令文件，重新加载后生效。</p>
    </div></section>
    <section id="appearance-customization-page" class="appearance-body appearance-settings-page" hidden>
    <h3 class="appearance-panel-title">外观</h3>
    <!-- 用户要求单层设置页使用分组卡片，不再添加主题/字体/背景的内部导航。 -->
    <section class="appearance-section"><h4>视觉风格</h4>
    <div class="appearance-card appearance-mode-card">
      <div class="appearance-field"><span id="appearance-theme-label">模式</span><div id="appearance-theme-slot" class="appearance-theme-options" role="group" aria-labelledby="appearance-theme-label"></div></div>
      <div id="appearance-auto-light-setting" hidden></div>
    </div>
    <div class="appearance-card">
      <div class="appearance-field"><span>主题</span><div id="appearance-preset-slot"></div></div>
      <div id="appearance-colors"></div>
      <!-- 用户要求增加独立对比度；每套主题/明暗模式分别保存，复用现有数字输入样式，不改动颜色选择器。 -->
      <label class="appearance-field"><span>对比度</span><span class="appearance-number"><input id="appearance-contrast" type="number" inputmode="numeric" min="0" max="100" step="1" aria-label="外观对比度"><span>%</span></span></label>
      <div class="appearance-field"><span>字体</span><div id="appearance-ui-font-slot"></div></div>
    </div></section>
    <section class="appearance-section"><h4>背景图片</h4><div class="appearance-card">
    <div class="appearance-field appearance-background-field"><span>图片</span><div class="appearance-background-actions"><input id="appearance-background-input" type="file" accept="image/png,image/jpeg,image/webp,image/gif" hidden><button type="button" id="appearance-background-action"></button></div></div>
    <div id="appearance-background-preview" class="appearance-background-preview" hidden><img id="appearance-background-thumbnail" alt=""><span id="appearance-background-name"></span></div>
    <label class="appearance-field"><span>透明度</span><span class="appearance-font"><input id="appearance-background-opacity" type="range" min="1" max="100" step="1" aria-label="背景透明度"><output id="appearance-background-opacity-value"></output></span></label>
    <div class="appearance-field"><span>图片铺法</span><div id="appearance-background-fit-slot"></div></div>
    <p id="appearance-background-status" class="appearance-background-status" role="status" hidden></p>
    </div></section>
    <details class="appearance-advanced"><summary>高级<span aria-hidden="true">›</span></summary>
    <section class="appearance-section"><h4>字体与排版</h4><div class="appearance-card">
    <!-- 用户要求字号均改为数值输入；程序 10～20、代码 8～24、阅读 6～14 的原有范围与校验同步保留。 -->
    <label class="appearance-field"><span>程序字号</span><span class="appearance-number"><input id="appearance-font" type="number" inputmode="numeric" min="10" max="20" step="1" aria-label="程序字号"><span>px</span></span></label>
    <div class="appearance-field"><span>程序字重</span><div id="appearance-ui-font-weight-slot"></div></div>
    <p class="appearance-font-preview">CodexElves · 你好，开始新的会话 Aa 012345</p>
    <div class="appearance-field"><span>代码字体</span><div id="appearance-code-font-slot"></div></div>
    <label class="appearance-field"><span>代码字号</span><span class="appearance-number"><input id="appearance-code-font-size" type="number" inputmode="numeric" min="8" max="24" step="1" aria-label="代码字号"><span>px</span></span></label>
    <div class="appearance-field"><span>代码字重</span><div id="appearance-code-font-weight-slot"></div></div>
    <pre class="appearance-code-preview">const greeting = "Hello, 你好";\n0123456789  () [] {} =&gt;</pre>
    <label class="appearance-field"><span>文件阅读字号</span><span class="appearance-number"><input id="appearance-file-reading-font" type="number" inputmode="numeric" min="${fileReadingFontMin}" max="${fileReadingFontMax}" step="1" aria-label="文件阅读字号" aria-describedby="appearance-file-reading-font-hint"><span>px</span></span></label>
    <div id="appearance-file-reading-font-hint" class="appearance-reading-font-hint"><span>用于变更预览与文件查看。</span><button type="button" id="appearance-file-reading-font-reset" class="text-button" title="恢复为 ${fileReadingFontDefault}px">恢复默认</button></div>
    </div><p class="appearance-hint">选择“系统”使用当前设备默认字体；程序和代码默认均为 14px。</p></section>
    </details>
    </section>
    <section id="appearance-diagnostics-page" class="appearance-body appearance-settings-page" hidden>
    <h3 class="appearance-panel-title">诊断日志</h3>
    <div id="appearance-diagnostics-field" class="appearance-field" hidden><span>运行日志</span><div class="appearance-action-buttons"><button type="button" id="appearance-diagnostics-export" disabled>导出</button><button type="button" id="appearance-diagnostics-send" disabled>发送到电脑</button></div></div>
    <p id="appearance-diagnostics-status" class="appearance-notification-status" role="status" hidden></p>
    </section>
    </div></div>
    <div class="appearance-actions"><button type="button" class="appearance-version" id="appearance-app-version" hidden disabled></button><div class="appearance-action-buttons"><button type="button" id="appearance-reset">恢复默认</button><button id="instruction-reload" type="button" hidden>重新读取</button><button id="instruction-save" type="button" class="primary" hidden disabled>保存指令</button><button type="button" class="primary" id="appearance-close">完成</button></div></div>`;
  document.body.append(dialog);
  const wrapSettingsPage = page => {
    if (page.querySelector(":scope > .appearance-page-content")) return;
    const content = element("div", "appearance-page-content");
    content.append(...page.childNodes); page.append(content);
  };
  for (const page of dialog.querySelectorAll(".appearance-settings-page")) wrapSettingsPage(page);
  // 设置内容实际保留 1000px，不把滚动条算进正文；PC 与手机导航宽度不同，居中始终读取实际右边界。
  const syncSettingsOffset = () => {
    if (dialog.open) dialog.style.setProperty("--settings-content-offset",
      `${$(".appearance-nav", dialog).getBoundingClientRect().right}px`);
  };
  const settingsResize = typeof ResizeObserver === "function" ? new ResizeObserver(syncSettingsOffset) : null;
  settingsResize?.observe($(".appearance-nav", dialog));
  window.addEventListener("resize", syncSettingsOffset);
  // 用户要求设置复用主题 Select：指令实例与两种字重也使用统一组件。
  for (const [id, label, options, value] of [
    ["instruction-instance", "会话实例", [], ""],
    ...[["ui", "程序字体", appearance.uiFont], ["code", "代码字体", appearance.codeFont]]
      .map(([kind, label, family]) => [`appearance-${kind}-font`, label, appearanceFontOptions(family), family]),
    ...[["ui", "程序字重", appearance.uiFontWeight], ["code", "代码字重", appearance.codeFontWeight]]
      .map(([kind, label, weight]) => [`appearance-${kind}-font-weight`, label,
        [{ value: "400", label: "常规" }, { value: "500", label: "中等" }, { value: "600", label: "半粗" }], String(weight)])
  ]) $(`#${id}-slot`, dialog).append(selectControl(id, label, options, value,
    id.endsWith("-font") ? "appearance-font-family" : ""));
  const title = $("#appearance-title", dialog);
  const headingButton = $(".appearance-heading button", dialog);
  const primaryBody = $("#appearance-general-page", dialog);
  const panelHost = $(".appearance-panels", dialog);
  const generalEntry = $("#appearance-general-entry", dialog);
  const customizationEntry = $("#appearance-customization-entry", dialog);
  const customizationPage = $("#appearance-customization-page", dialog);
  const entryButton = $("#appearance-notifications-entry", dialog);
  const diagnosticsEntry = $("#appearance-diagnostics-entry", dialog);
  const diagnosticsPage = $("#appearance-diagnostics-page", dialog);
  const resetButton = $("#appearance-reset", dialog);
  const notificationVersionButton = $("#appearance-app-version", dialog);
  if (androidContainer) {
    const diagnosticsField = $("#appearance-diagnostics-field", dialog);
    const diagnosticsStatus = $("#appearance-diagnostics-status", dialog);
    const exportButton = $("#appearance-diagnostics-export", dialog);
    const sendButton = $("#appearance-diagnostics-send", dialog);
    diagnosticsField.hidden = diagnosticsStatus.hidden = false;
    let diagnosticsSupported = false, diagnosticsBusy = false;
    const update = () => {
      exportButton.disabled = !diagnosticsSupported || diagnosticsBusy;
      sendButton.disabled = !diagnosticsSupported || diagnosticsBusy || !session || !connected;
    };
    void getAndroidAppInfo().then(info => {
      if (!dialog.isConnected) return;
      diagnosticsSupported = info?.diagnosticsVersion === 1;
      diagnosticsStatus.textContent = diagnosticsSupported
        ? "记录运行日志，便于排查问题。"
        : "当前 App 未提供诊断采集，请安装新版 APK。";
      update();
    }).catch(() => {});
    const run = async sending => {
      if (diagnosticsBusy || !diagnosticsSupported) return;
      diagnosticsBusy = true;
      diagnosticsStatus.textContent = sending ? "正在发送诊断日志…" : "正在准备诊断日志…";
      update();
      try {
        if (sending) await sendAndroidDiagnostics();
        else await exportAndroidDiagnostics();
        if (dialog.isConnected) diagnosticsStatus.textContent = sending
          ? "电脑端已收到诊断日志。" : "已打开系统分享，可选择保存或发送日志文件。";
      } catch (error) {
        if (dialog.isConnected) diagnosticsStatus.textContent = error.message;
      } finally {
        diagnosticsBusy = false;
        update();
      }
    };
    exportButton.onclick = () => { void run(false); };
    sendButton.onclick = () => { void run(true); };
  }
  if (androidContainer) {
    $("#appearance-nav-tools", dialog).hidden = false;
    entryButton.hidden = false;
    diagnosticsEntry.hidden = false;
  }
  generalEntry.querySelector(".appearance-nav-icon").append(icon("settings"));
  customizationEntry.querySelector(".appearance-nav-icon").append(icon("sun"));
  entryButton.querySelector(".appearance-nav-icon").append(icon("globe"));
  diagnosticsEntry.querySelector(".appearance-nav-icon").append(icon("tools"));
  $("#appearance-types-entry", dialog).querySelector(".appearance-nav-icon").append(icon("allSessions"));
  $("#appearance-permissions-entry", dialog).querySelector(".appearance-nav-icon").append(icon("shield"));
  $("#appearance-instructions-entry", dialog).querySelector(".appearance-nav-icon").append(icon("edit"));
  headingButton.append(icon("close"));
  renderPwaCacheState();
  const versionButton = notificationVersionButton;
  let notificationPage = null;
  let notificationPageRevision = 0;
  let notificationPageVisible = false;
  let notificationPageUnsubscribe = null;
  let activeNotificationPage = null;
  let notificationOperations = 0;
  let androidVersionLoaded = false;
  let subpageEntry = null;
  const notificationEntry = entryButton;
  const updateNotificationOperationState = () => {
    notificationEntry.disabled = notificationOperations > 0;
    activeNotificationPage?.setBusy(notificationOperations > 0);
  };
  const notificationErrorMessage = error => {
    if (["unsupported", "timeout"].includes(error?.code))
      return "当前 Android App 版本不支持或未响应通知设置，请更新 App 后重试。";
    if (error?.code === "permission-denied") return "尚未获得系统通知权限，可前往系统通知设置开启。";
    return "请稍后重试。";
  };
  const beginNotificationOperation = () => {
    notificationOperations++;
    updateNotificationOperationState();
  };
  const endNotificationOperation = () => {
    notificationOperations = Math.max(0, notificationOperations - 1);
    updateNotificationOperationState();
  };
  const notificationPageIsCurrent = (page, revision) => androidContainer && notificationPageVisible
    && notificationPage === page && notificationPageRevision === revision && page.isConnected
    && dialog.isConnected && dialog.open;
  const setSettingsNav = entry => {
    for (const item of dialog.querySelectorAll(".appearance-nav-item"))
      item.setAttribute("aria-selected", String(item === entry));
  };
  const selectSettingsPage = (page, entry, { focus = false } = {}) => {
    if (!page?.isConnected) return;
    wrapSettingsPage(page);
    closePopup();
    for (const panel of panelHost.querySelectorAll(".appearance-settings-page"))
      panel.hidden = panel !== page;
    // 用户要求指令按钮使用底部“完成”的位置；该页只显示重新读取和保存，切换到其他页时恢复“完成”。
    const instructions = page.id === "appearance-instructions-page";
    for (const id of ["instruction-reload", "instruction-save"])
      $(`#${id}`, dialog).hidden = !instructions;
    $("#appearance-close", dialog).hidden = instructions;
    resetButton.hidden = page !== primaryBody && page !== customizationPage;
    setSettingsNav(entry);
    page.scrollTop = 0;
    if (focus) entry?.focus({ preventScroll: true });
  };
  const showAppearancePage = (page, pageTitle, entry) => {
    subpageEntry = entry;
    selectSettingsPage(page, entry);
    // 用户要求设置内容按分组显示，导航保持单层；标题始终为“设置”，不增加返回箭头或页内面包屑。
    page.setAttribute("aria-label", pageTitle);
    title.textContent = "设置";
    headingButton.title = "关闭设置";
    headingButton.setAttribute("aria-label", "关闭设置");
    headingButton.replaceChildren(icon("close"));
    versionButton.hidden = !androidVersionLoaded;
  };
  const restoreAppearanceRoot = () => {
    if (!subpageEntry) return;
    const returnEntry = subpageEntry;
    subpageEntry = null;
    notificationPageVisible = false;
    notificationPageRevision++;
    notificationPageUnsubscribe?.();
    notificationPageUnsubscribe = null;
    activeNotificationPage?.systemSettingsButton.remove();
    activeNotificationPage = null;
    notificationPage?.remove();
    notificationPage = null;
    selectSettingsPage(primaryBody, generalEntry);
    dialog.classList.remove("appearance-subpage-open");
    title.textContent = "设置";
    headingButton.title = "关闭设置";
    headingButton.setAttribute("aria-label", "关闭设置");
    headingButton.replaceChildren(icon("close"));
    resetButton.hidden = false;
    versionButton.hidden = !androidVersionLoaded;
    updateNotificationOperationState();
    returnEntry.focus({ preventScroll: true });
  };
  customizationEntry.onclick = () => {
    if (!dialog.isConnected || !dialog.open) return;
    showAppearancePage(customizationPage, "外观", customizationEntry);
  };
  for (const [name, label] of [["types", "会话实例"], ["instructions", "自定义指令"]]) {
    const entry = $(`#appearance-${name}-entry`, dialog);
    entry.onclick = () => {
      showAppearancePage($(`#appearance-${name}-page`, dialog), label, entry);
      void remoteSettings.show(name);
    };
  }
  $("#appearance-permissions-entry", dialog).onclick = async () => {
    showAppearancePage($("#appearance-permissions-page", dialog), "权限", $("#appearance-permissions-entry", dialog));
    try {
      await loadSessionInstances();
      await instancePermissions.show();
    } catch (error) {
      if (dialog.isConnected) notice(error.message, true);
    }
  };
  generalEntry.onclick = () => {
    if (!dialog.isConnected || !dialog.open) return;
    if (subpageEntry) restoreAppearanceRoot();
    else selectSettingsPage(primaryBody, generalEntry, { focus: true });
    if (session) void claudeRecap.show();
  };
  diagnosticsEntry.onclick = () => {
    if (!dialog.isConnected || !dialog.open || diagnosticsEntry.hidden) return;
    showAppearancePage(diagnosticsPage, "诊断日志", diagnosticsEntry);
  };
  if (androidContainer) entryButton.onclick = () => {
    if (!dialog.isConnected || !dialog.open) return;
    notificationPageVisible = false;
    notificationPageRevision++;
    notificationPageUnsubscribe?.();
    notificationPageUnsubscribe = null;
    activeNotificationPage?.systemSettingsButton.remove();
    activeNotificationPage = null;
    notificationPage?.remove();
    notificationPage = null;
    notificationPageVisible = true;
    const revision = ++notificationPageRevision;
    const page = element("div", "appearance-body appearance-settings-page appearance-subpage appearance-notifications-page");
    page.id = "appearance-notifications-page";
    page.hidden = true;
    notificationPage = page;
    const status = element("p", "appearance-notification-status");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    const rows = element("div", "appearance-notification-rows");
    rows.setAttribute("aria-busy", "true");
    const controls = new Map();
    const addSwitch = (key, label, destination) => {
      const row = element("label", "appearance-notification-row");
      const text = element("span", "", label);
      const control = document.createElement("input");
      control.type = "checkbox";
      control.className = "appearance-notification-switch";
      control.setAttribute("role", "switch");
      control.setAttribute("aria-label", label);
      control.dataset.notificationSetting = key;
      control.disabled = true;
      row.append(text, control);
      destination.append(row);
      controls.set(key, control);
    };
    addSwitch("enabled", "接收通知", rows);
    const categories = element("div", "appearance-notification-categories");
    for (const [key, label] of [
      ["approval", "需要授权"],
      ["question", "需要回答"],
      ["completed", "执行完成"]
    ]) addSwitch(key, label, categories);
    rows.append(categories);
    const systemSettingsButton = button("打开系统通知设置", "打开系统通知设置", async () => {
      if (!notificationPageIsCurrent(page, revision) || notificationOperations) return;
      beginNotificationOperation();
      try {
        const result = await openAndroidNotificationSystemSettings();
        if (!notificationPageIsCurrent(page, revision)) return;
        if (result?.supported) {
          queuedChangedStatus = null;
          renderStatus(result);
        }
        else status.textContent = "当前 Android App 版本暂不支持通知设置，请更新 App。";
      } catch (error) {
        if (notificationPageIsCurrent(page, revision))
          status.textContent = `打开系统通知设置失败：${notificationErrorMessage(error)}`;
      } finally {
        finishPageOperation();
      }
    }, "appearance-notification-action");
    systemSettingsButton.hidden = true;
    page.append(status, rows, systemSettingsButton);
    panelHost.append(page);
    showAppearancePage(page, "通知事件", notificationEntry);
    let state = null;
    const setBusy = busy => {
      for (const [key, control] of controls)
        control.disabled = busy || !state || key !== "enabled" && !state.settings.enabled;
      categories.setAttribute("aria-disabled", String(!state?.settings.enabled));
      systemSettingsButton.disabled = busy || !state;
    };
    const pageApi = { setBusy, systemSettingsButton };
    activeNotificationPage = pageApi;
    let changedVersion = 0;
    let queuedChangedStatus = null;
    const renderStatus = value => {
      if (!notificationPageIsCurrent(page, revision)) return;
      if (!value?.supported) {
        state = null;
        rows.hidden = true;
        rows.removeAttribute("aria-busy");
        systemSettingsButton.hidden = true;
        status.textContent = "当前 Android App 版本暂不支持通知设置，请更新 App。";
        return;
      }
      state = value;
      rows.hidden = false;
      rows.removeAttribute("aria-busy");
      for (const [key, control] of controls) {
        control.checked = value.settings[key];
      }
      setBusy(notificationOperations > 0);
      const permissionText = {
        prompt: "系统通知尚未授权",
        granted: "系统通知权限已允许",
        denied: "系统通知未开启"
      }[value.permission];
      const deliveryText = {
        disabled: value.settings.enabled ? "等待启动" : "已关闭",
        "permission-required": "需要系统通知权限",
        connecting: "正在连接",
        connected: "连接已建立",
        disconnected: "连接中断",
        unsupported: "当前版本不支持",
        "auth-required": "需要重新授权"
      }[value.delivery];
      let summary = `通知${value.settings.enabled ? "已开启" : "已关闭"}；${permissionText}；${deliveryText}。`;
      if (value.permission === "denied")
        summary = `系统通知尚未获准或已关闭，当前不会接收通知。`;
      if (value.message) summary += ` ${value.message}`;
      status.textContent = summary;
      systemSettingsButton.hidden = false;
    };
    const renderError = (error, action) => {
      if (!notificationPageIsCurrent(page, revision)) return;
      if (!state) rows.hidden = true;
      rows.removeAttribute("aria-busy");
      const message = notificationErrorMessage(error);
      status.textContent = action === "read"
        ? `无法读取通知设置：${message}`
        : action === "permission"
          ? `通知设置已保存；系统权限请求未完成。${message}`
          : `保存失败，页面保留上次读取的设置。${message}`;
      systemSettingsButton.hidden = !state?.supported;
    };
    const isCurrent = () => notificationPageIsCurrent(page, revision);
    const finishPageOperation = () => {
      endNotificationOperation();
      if (!notificationOperations && queuedChangedStatus) {
        const latest = queuedChangedStatus;
        queuedChangedStatus = null;
        if (isCurrent()) renderStatus(latest);
      }
    };
    const readSettings = async () => {
      const previousChangedVersion = changedVersion;
      beginNotificationOperation();
      try {
        const value = await getAndroidNotificationSettings();
        if (!isCurrent() || changedVersion !== previousChangedVersion) return;
        queuedChangedStatus = null;
        renderStatus(value);
      } catch (error) {
        renderError(error, "read");
      } finally {
        finishPageOperation();
      }
    };
    const saveSetting = async (key, enabled) => {
      if (!isCurrent() || !state || notificationOperations) return;
      const oldValue = state.settings[key];
      controls.get(key).checked = oldValue;
      const nextSettings = { ...state.settings, [key]: enabled };
      beginNotificationOperation();
      let saveCompleted = false;
      try {
        await setAndroidNotificationSettings(nextSettings);
        saveCompleted = true;
        const saved = await getAndroidNotificationSettings();
        if (!isCurrent()) return;
        if (!saved?.supported) throw Object.assign(new Error("当前 Android App 版本暂不支持通知设置，请更新 App。"), { code: "unsupported" });
        queuedChangedStatus = null;
        renderStatus(saved);
        if (key === "enabled" && enabled) {
          try {
            const permission = await requestAndroidNotificationPermission();
            if (!isCurrent()) return;
            if (permission?.supported) {
              queuedChangedStatus = null;
              renderStatus(permission);
            }
            else status.textContent = "通知已开启，但当前 Android App 版本无法申请系统权限，请更新 App。";
          } catch (error) {
            if (!isCurrent()) return;
            renderError(error, "permission");
          }
        }
      } catch (error) {
        if (saveCompleted) {
          if (isCurrent()) status.textContent = `通知设置已保存，但无法确认当前状态；页面保留上次读取的设置。${notificationErrorMessage(error)}`;
        } else {
          queuedChangedStatus = null;
          renderError(error, "save");
        }
      } finally {
        finishPageOperation();
      }
    };
    for (const [key, control] of controls) {
      control.addEventListener("change", () => {
        if (state) void saveSetting(key, control.checked);
      });
    }
    notificationPageUnsubscribe = subscribeAndroidNotificationSettings(value => {
      if (!isCurrent()) return;
      changedVersion++;
      if (notificationOperations) queuedChangedStatus = value;
      else renderStatus(value);
    });
    void readSettings();
    updateNotificationOperationState();
  };
  let closing = false;
  const closeAppearance = returnValue => {
    if (!dialog.open || closing) return;
    closing = true;
    // 用户要求 PC 设置立即开关；手机沿用 220ms 滑入/滑出及原生返回行为。
    if (matchMedia("(min-width: 761px)").matches) {
      dialog.close(returnValue); closing = false; return;
    }
    dialog.classList.remove("appearance-screen-visible");
    setTimeout(() => {
      if (dialog.open) dialog.close(returnValue);
      closing = false;
    }, 220);
  };
  const onNativeBack = event => {
    if (!dialog.open || !dialog.isConnected || [...document.querySelectorAll("dialog[open]")].at(-1) !== dialog) return;
    event.preventDefault();
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup(true);
    else closeAppearance();
  };
  const onAppearanceEscape = event => {
    // 只处理顶层设置窗口；指令重新读取等确认弹窗继续使用自己的取消行为。
    if (event.key !== "Escape" || event.isComposing || !dialog.open
        || [...document.querySelectorAll("dialog[open]")].at(-1) !== dialog) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    // 设置中的所有 Select 共用关闭顺序：先关闭弹层并恢复焦点，再响应下一次返回。
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup(true);
    else closeAppearance();
  };
  if (androidContainer) {
    window.addEventListener("codex-elves-native-back", onNativeBack);
  }
  window.addEventListener("keydown", onAppearanceEscape, true);
  if (androidContainer) {
    void getAndroidAppInfo().then(info => {
      if (!info || !dialog.isConnected || !dialog.open || !versionButton.isConnected) return;
      androidVersionLoaded = true;
      versionButton.textContent = `v${info.versionName}`;
      versionButton.disabled = !info.updateSupported;
      versionButton.title = info.updateSupported ? "点击检查更新" : "当前应用暂不支持在线更新";
      versionButton.setAttribute("aria-label", info.updateSupported
        ? `当前应用版本 v${info.versionName}，点击检查更新`
        : `当前应用版本 v${info.versionName}，暂不支持在线更新`);
      versionButton.hidden = false;
    }).catch(() => {});
  }
  versionButton.onclick = () => {
    if (dialog.isConnected && dialog.open && !versionButton.disabled) postPlatformMessage("check-app-update");
  };
  const modeButtons = new Map();
  for (const [mode, label] of [["auto", "自动"], ["light", "浅色"], ["dark", "深色"]]) {
    const control = button("", `${label}模式`, () =>
      setAppearanceThemeMode(mode === "auto" ? "auto" : "manual", { theme: mode }), "appearance-mode-option");
    control.dataset.themeMode = mode;
    const preview = element("span", `appearance-mode-preview appearance-mode-${mode}`);
    preview.setAttribute("aria-hidden", "true");
    preview.append(element("span", "appearance-mode-sidebar"), element("span", "appearance-mode-lines"));
    control.append(preview, element("span", "appearance-mode-label", label));
    modeButtons.set(mode, control); $("#appearance-theme-slot").append(control);
  }
  $("#appearance-preset-slot").append(selectControl("appearance-preset", "主题",
    Object.entries(appearanceThemes).map(([value, preset]) => ({ value, label: preset.label })), appearance.themePreset));
  const syncAutomaticLightSetting = mountAutomaticThemeLightSetting($("#appearance-auto-light-setting"));
  $("#appearance-project-thread-limit-slot").append(selectControl("appearance-project-thread-limit", "每个项目显示会话数",
    projectThreadLimits.map(value => ({ value: String(value), label: `${value}个` })), String(appearance.projectThreadLimit)));
  const backgroundAction = $("#appearance-background-action");
  $("#appearance-background-fit-slot").append(selectControl("appearance-background-fit", "图片铺法", [
    { value: "cover", label: "铺满裁切" }, { value: "contain", label: "完整不裁" }
  ], appearance.backgroundFit));
  const colorFields = [["accent", "强调色"], ["background", "背景"], ["foreground", "前景"]];
  const setColor = (name, color) => { appearance.colors[appearance.theme][name] = color; applyAppearance(true); sync(); };
  const colorValueSync = new Map();
  for (const [name, label] of colorFields) {
    const row = element("div", "appearance-field");
    const control = element("div", "appearance-color-control");
    const swatch = button("", `${label}颜色`, () => openAppearanceColorPicker(swatch, label,
      appearance.colors[appearance.theme][name], color => setColor(name, color)), "color-swatch");
    swatch.id = `appearance-${name}`;
    swatch.setAttribute("aria-haspopup", "dialog"); swatch.setAttribute("aria-expanded", "false");
    swatch.append(element("span", "appearance-color-dot"));
    const hex = element("input", "color-hex appearance-color-value");
    hex.id = `appearance-${name}-hex`; hex.type = "text"; hex.maxLength = 7;
    hex.spellcheck = false; hex.autocomplete = "off"; hex.setAttribute("aria-label", `${label}色值`);
    colorValueSync.set(name, bindAppearanceColorValue(hex, () => appearance.colors[appearance.theme][name], color => setColor(name, color)));
    control.append(swatch, hex);
    row.append(element("span", "", label), control); $("#appearance-colors").append(row);
  }
  const sync = () => {
    const selectedMode = appearance.themeMode === "auto" ? "auto" : appearance.theme;
    for (const [mode, control] of modeButtons) control.setAttribute("aria-pressed", String(mode === selectedMode));
    syncAutomaticLightSetting();
    setSelectValue($("#appearance-project-thread-limit"), String(appearance.projectThreadLimit));
    const hasBackground = !!remoteBackground.dataUrl;
    backgroundAction.disabled = remoteBackgroundBusy;
    backgroundAction.replaceChildren(icon(hasBackground ? "trash" : "image"),
      element("span", "", hasBackground ? "移除" : "上传"));
    resetButton.disabled = remoteBackgroundBusy;
    $("#appearance-background-preview").hidden = !hasBackground;
    $("#appearance-background-name").textContent = remoteBackground.name;
    if (hasBackground && $("#appearance-background-thumbnail").getAttribute("src") !== remoteBackground.dataUrl)
      $("#appearance-background-thumbnail").src = remoteBackground.dataUrl;
    if (!hasBackground) $("#appearance-background-thumbnail").removeAttribute("src");
    $("#appearance-background-opacity").disabled = remoteBackgroundBusy || !hasBackground;
    $("#appearance-background-opacity").value = appearance.backgroundOpacity;
    $("#appearance-background-opacity").style.setProperty("--range-fill", `${appearance.backgroundOpacity - 1}%`);
    $("#appearance-background-opacity-value").textContent = `${appearance.backgroundOpacity}%`;
    $("#appearance-background-fit").disabled = remoteBackgroundBusy || !hasBackground;
    setSelectValue($("#appearance-background-fit"), appearance.backgroundFit);
    $("#appearance-background-status").hidden = !remoteBackgroundError;
    $("#appearance-background-status").textContent = remoteBackgroundError;
    for (const [name] of colorFields) {
      const color = appearance.colors[appearance.theme][name], swatch = $(`#appearance-${name}`);
      swatch.style.setProperty("--swatch", color);
      colorValueSync.get(name)?.();
      swatch.setAttribute("aria-description", color.toUpperCase());
      if (activePopup?.trigger === swatch) activePopup.syncColor?.(color);
    }
    const contrastInput = $("#appearance-contrast", dialog);
    if (document.activeElement !== contrastInput) contrastInput.value = appearance.colors[appearance.theme].contrast;
    if (document.activeElement !== $("#appearance-font")) $("#appearance-font").value = appearance.fontSize;
    setSelectValue($("#appearance-preset"), appearance.themePreset);
    for (const [id, value] of [["ui-font", appearance.uiFont], ["code-font", appearance.codeFont]])
      setSelectValue($(`#appearance-${id}`, dialog), value);
    for (const [id, value] of [["code-font-size", appearance.codeFontSize], ["conversation-width", appearance.conversationWidth]]) {
      const control = $(`#appearance-${id}`, dialog);
      if (document.activeElement !== control) control.value = value;
    }
    setSelectValue($("#appearance-ui-font-weight", dialog), appearance.uiFontWeight);
    setSelectValue($("#appearance-code-font-weight", dialog), appearance.codeFontWeight);
    renderSessionInstances();
    const readingFontSize = currentFileReadingFontSize();
    if (document.activeElement !== $("#appearance-file-reading-font")) $("#appearance-file-reading-font").value = readingFontSize;
    $("#appearance-file-reading-font-reset").disabled = readingFontSize === fileReadingFontDefault;
  };
  sync.lightReading = syncAutomaticLightSetting.lightReading;
  setSyncAppearanceThemeControls(sync);
  const remoteSettings = mountRemoteSettings(dialog, sync);
  const instancePermissions = mountInstancePermissions({
    page: $("#instance-permissions", dialog), element, selectControl, setSelectOptions, setSelectValue, api,
    getInstances: availableSessionInstances, readOnly: () => !session || session.readOnly || !connected,
    authorizationScope: () => privateInputScope(session),
    onLoaded: rememberInstancePermissionCatalog,
    onSaved(id, result) { rememberInstancePermissionCatalog(id, result); updateComposer(); }
  });
  const claudeRecap = mountClaudeRecapSettings({
    page: $("#appearance-claude-recap", dialog), element, selectControl, setSelectOptions, api,
    getInstances: availableSessionInstances, loadInstances: loadSessionInstances,
    readOnly: () => !session || session.readOnly || !connected,
    authorizationScope: () => privateInputScope(session)
  });
  if (session) void claudeRecap.show(activeInstanceId());
  sync();
  bindAppearanceNumberValue($("#appearance-contrast", dialog), 0, 100,
    () => appearance.colors[appearance.theme].contrast,
    value => { appearance.colors[appearance.theme].contrast = value; applyAppearance(true); sync(); }, "%");
  $("#appearance-conversation-width", dialog).onchange = event => {
    const value = Number(event.target.value);
    if (!Number.isInteger(value) || value < conversationWidthMin || value > conversationWidthMax) {
      event.target.value = appearance.conversationWidth;
      notice(`正文宽度请填写 ${conversationWidthMin}～${conversationWidthMax}px。`, true); return;
    }
    appearance.conversationWidth = value; applyAppearance(true);
  };
  $("#appearance-preset", dialog).onchange = event => {
    appearance.themePreset = event.target.value;
    appearance.colors = appearance.palettes[appearance.themePreset];
    applyAppearance(true); sync();
  };
  for (const [id, key] of [["ui-font", "uiFont"], ["code-font", "codeFont"]])
    $(`#appearance-${id}`, dialog).onchange = event => {
      const value = event.target.value.trim();
      if (!validFontFamily(value)) { event.target.value = appearance[key]; return; }
      appearance[key] = value; applyAppearance(true);
    };
  for (const [id, key] of [["ui-font-weight", "uiFontWeight"], ["code-font-weight", "codeFontWeight"]])
    $(`#appearance-${id}`, dialog).onchange = event => {
    const value = Number(event.target.value);
    if (![400, 500, 600].includes(value)) {
      event.target.value = appearance[key]; return;
    }
    appearance[key] = value; applyAppearance(true);
  };
  $("#appearance-project-thread-limit").onchange = event => {
    appearance.projectThreadLimit = Number(event.target.value);
    applyAppearance(true);
    projectVisibleThreadCounts.clear();
    renderSidebar();
    sync();
  };
  $("#appearance-background-input").onchange = async event => {
    const file = event.target.files?.[0]; event.target.value = "";
    if (!file || remoteBackgroundBusy) return;
    const operation = setRemoteBackgroundOperation(remoteBackgroundOperation + 1);
    setRemoteBackgroundBusy(true); setRemoteBackgroundError("");
    syncAppearanceThemeControls?.();
    try {
      const dataUrl = await readRemoteBackgroundFile(file);
      const record = { dataUrl, name: file.name || "背景图片" };
      await writeRemoteBackground(record);
      if (operation !== remoteBackgroundOperation) return;
      setRemoteBackground(record);
      syncRemoteBackground();
    } catch (error) {
      if (operation === remoteBackgroundOperation) setRemoteBackgroundError(error.message || "背景图片保存失败。");
    } finally {
      if (operation === remoteBackgroundOperation) {
        setRemoteBackgroundBusy(false);
        syncAppearanceThemeControls?.();
      }
    }
  };
  backgroundAction.onclick = async () => {
    if (remoteBackgroundBusy) return;
    if (!remoteBackground.dataUrl) { $("#appearance-background-input").click(); return; }
    const operation = setRemoteBackgroundOperation(remoteBackgroundOperation + 1);
    setRemoteBackgroundBusy(true); setRemoteBackgroundError("");
    syncAppearanceThemeControls?.();
    try {
      await deleteRemoteBackground();
      if (operation !== remoteBackgroundOperation) return;
      setRemoteBackground({ dataUrl: "", name: "" });
      syncRemoteBackground();
    } catch (error) {
      if (operation === remoteBackgroundOperation) setRemoteBackgroundError(error.message || "背景图片移除失败。");
    } finally {
      if (operation === remoteBackgroundOperation) {
        setRemoteBackgroundBusy(false);
        syncAppearanceThemeControls?.();
      }
    }
  };
  $("#appearance-background-opacity").oninput = event => {
    appearance.backgroundOpacity = Number(event.target.value);
    applyAppearance(true); sync();
  };
  $("#appearance-background-fit").onchange = event => {
    appearance.backgroundFit = event.target.value;
    applyAppearance(true); sync();
  };
  for (const [id, key, min, max] of [["font", "fontSize", 10, 20], ["code-font-size", "codeFontSize", 8, 24],
    ["file-reading-font", "fileReadingFontSize", fileReadingFontMin, fileReadingFontMax]]) {
    bindAppearanceFontSize($(`#appearance-${id}`, dialog), min, max,
      () => key === "fileReadingFontSize" ? currentFileReadingFontSize() : appearance[key],
      value => { appearance[key] = value; applyAppearance(true); sync(); });
  }
  $("#appearance-file-reading-font-reset").onclick = () => {
    appearance.fileReadingFontSize = fileReadingFontDefault; applyAppearance(true); sync();
  };
  $("#appearance-reset").onclick = async () => {
    if (remoteBackgroundBusy || !dialog.isConnected || !dialog.open) return;
    const scope = privateInputScope(session);
    // 用户要求外观“恢复默认”先二次确认；取消、关闭设置或更换授权均不得删除背景或改写当前设置。
    if (!await confirmAction("恢复默认设置",
      "将恢复默认配色、字体、字号、会话宽度、项目会话数、会话筛选和实例开关，并移除背景图片。是否继续？",
      "恢复默认", true) || remoteBackgroundBusy || !dialog.isConnected || !dialog.open
        || scope !== privateInputScope(session)) return;
    const operation = setRemoteBackgroundOperation(remoteBackgroundOperation + 1);
    setRemoteBackgroundBusy(true); setRemoteBackgroundError(""); sync();
    try {
      await deleteRemoteBackground();
      if (operation !== remoteBackgroundOperation) return;
      setRemoteBackground({ dataUrl: "", name: "" });
      setAppearance(defaultAppearance(appearance.theme));
      if (creating) setCreationBackend("codex");
      applyAppearance(true);
      projectVisibleThreadCounts.clear();
      renderSidebar();
      updateComposer(); sessionMenu?.close();
      void loadSidebar(null, { fresh: true }).catch(error => notice(error.message, true));
    } catch (error) {
      if (operation === remoteBackgroundOperation) setRemoteBackgroundError(error.message || "恢复默认设置失败。");
    } finally {
      if (operation === remoteBackgroundOperation) {
        setRemoteBackgroundBusy(false);
        syncAppearanceThemeControls?.();
      }
    }
  };
  $("#appearance-close").onclick = () => closeAppearance();
  headingButton.onclick = () => closeAppearance();
  let backdropPressed = false;
  const onBackdrop = event => {
    const box = dialog.getBoundingClientRect();
    return event.target === dialog && (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom);
  };
  dialog.onpointerdown = event => { backdropPressed = onBackdrop(event); };
  dialog.onpointermove = event => { if (!onBackdrop(event)) backdropPressed = false; };
  dialog.onpointercancel = () => { backdropPressed = false; };
  dialog.onclick = event => {
    const close = backdropPressed && onBackdrop(event);
    backdropPressed = false;
    if (!close) return;
    // 触摸在 pointerup 后仍会派发点击，等它命中遮罩后再关闭，避免落到背景元素。
    event.preventDefault();
    event.stopPropagation();
    closeAppearance();
  };
  dialog.oncancel = event => {
    event.preventDefault();
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup(true);
    else closeAppearance();
  };
  dialog.onclose = () => {
    settingsResize?.disconnect();
    window.removeEventListener("resize", syncSettingsOffset);
    remoteSettings.dispose();
    instancePermissions.dispose();
    claudeRecap.dispose();
    if (syncAppearanceThemeControls === sync) setSyncAppearanceThemeControls(null);
    dialog.classList.remove("appearance-screen-visible");
    dialog.classList.remove("appearance-subpage-open");
    notificationPageRevision++;
    notificationPageVisible = false;
    subpageEntry = null;
    notificationPageUnsubscribe?.();
    notificationPageUnsubscribe = null;
    activeNotificationPage?.systemSettingsButton.remove();
    activeNotificationPage = null;
    notificationPage?.remove();
    notificationPage = null;
    window.removeEventListener("codex-elves-native-back", onNativeBack);
    window.removeEventListener("keydown", onAppearanceEscape, true);
    if (activePopup?.trigger.closest("dialog") === dialog) closePopup();
    document.body.classList.toggle("modal-open", !!document.querySelector("dialog[open]"));
    if (trigger?.isConnected && document.activeElement === document.body) trigger.focus();
  };
  document.body.classList.add("modal-open");
  dialog.showModal();
  syncSettingsOffset();
  if (matchMedia("(min-width: 761px)").matches) dialog.classList.add("appearance-screen-visible");
  else requestAnimationFrame(() => {
    if (dialog.isConnected && dialog.open) dialog.classList.add("appearance-screen-visible");
  });
}
