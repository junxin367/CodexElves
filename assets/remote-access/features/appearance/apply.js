// 外观应用：把主题、字体与背景应用到页面，自动主题的光感切换与设置挂载，外观启动与侧边面板同步；外观状态、默认值与背景存储在 theme.js。

import { subscribeAndroidAmbientLight } from "../../platform.js";
import { $, element } from "../../ui/dom.js";
import { notice } from "../../ui/notice.js";
import {
  ambientLightStatus, appearance, appearanceSurfaceColors, appearanceTextColors,
  appearanceTransitionTimer, automaticAppearanceTheme, automaticThemeAtNight,
  automaticThemeCandidate, automaticThemeLightLuxMax, automaticThemeLightLuxMin,
  automaticThemeLuxShift, automaticThemeShiftProbe, automaticThemeStableMs,
  beginThemeDiagnosticTransition, clearThemeDiagnosticTimers, currentAutomaticThemeLightLux,
  currentFileReadingFontSize, defaultCodeFont, defaultUiFont, finishThemeDiagnosticTransition,
  fontStack, localAppearance, mixColor, observeAutomaticThemeShift, observeThemeLight, readAppearance,
  readRemoteBackground, recordThemeDiagnostic, remoteBackground, remoteBackgroundDataPattern,
  remoteBackgroundOperation, resetAutomaticThemeCompensation, setAmbientLightStatus, setAppearance,
  setAppearanceTransitionTimer, setAutomaticThemeCandidate, setAutomaticThemeLuxShift,
  setAutomaticThemeShiftProbe, setRemoteBackground, setRemoteBackgroundError,
  setRemoteBackgroundOperation, syncAppearanceThemeControls
} from "./theme.js";
import { refitComposerMenus } from "../composer/composer-menus.js";
import { resizeComposerInput, resizeEditorInput } from "../composer/prompt-editor.js";
import { scheduleMermaidBlocks } from "../messages/media-state.js";
import { editingMessage } from "../messages/message-edit.js";
import { workspaceSidePanel } from "../session/side-panel.js";

export function syncRemoteBackground() {
  const image = $("#remote-background");
  if (!image) return;
  // 用户要求侧边标签与主会话共用同一背景合成：图片只由父页面绘制一次，避免 iframe 重铺、错位或叠加透明度。
  if (workspaceSidePanel) {
    image.hidden = true;
    image.removeAttribute("src");
    return;
  }
  image.hidden = !remoteBackground.dataUrl;
  image.style.opacity = String(appearance.backgroundOpacity / 100);
  image.style.objectFit = appearance.backgroundFit;
  if (remoteBackground.dataUrl) {
    if (image.getAttribute("src") !== remoteBackground.dataUrl) image.src = remoteBackground.dataUrl;
  } else image.removeAttribute("src");
}
function applyAutomaticAppearance(save = false, { immediate = false, reason = "timer", now = Date.now() } = {}) {
  if (appearance.themeMode !== "auto") { resetAutomaticThemeCompensation(); return false; }
  const night = automaticThemeAtNight();
  if (!night && (automaticThemeShiftProbe || automaticThemeLuxShift)) resetAutomaticThemeCompensation();
  observeAutomaticThemeShift(now, reason);
  const next = automaticAppearanceTheme();
  if (next === appearance.theme) {
    if (automaticThemeCandidate) recordThemeDiagnostic("theme.decision", {
      targetTheme: automaticThemeCandidate.theme, decision: "cancelled", reason,
      durationMs: Math.max(0, now - automaticThemeCandidate.since)
    });
    setAutomaticThemeCandidate(null);
    return false;
  }
  if (!immediate) {
    if (automaticThemeCandidate?.theme !== next) {
      setAutomaticThemeCandidate({ theme: next, since: now, reason });
      recordThemeDiagnostic("theme.decision", { targetTheme: next, decision: "pending", reason, stableMs: automaticThemeStableMs });
      return false;
    }
    if (now - automaticThemeCandidate.since < automaticThemeStableMs) return false;
  }
  const stableDurationMs = automaticThemeCandidate ? Math.max(0, now - automaticThemeCandidate.since) : 0;
  const previousTheme = appearance.theme;
  const transition = beginThemeDiagnosticTransition(next, reason);
  setAutomaticThemeCandidate(null);
  appearance.theme = next;
  if (night && next === "dark" && previousTheme === "light") {
    setAutomaticThemeLuxShift(0);
    setAutomaticThemeShiftProbe(ambientLightStatus.supported && Number.isFinite(ambientLightStatus.lux)
      ? { lux: ambientLightStatus.lux, since: now, observed: false } : null);
  }
  applyAppearance(save);
  syncAppearanceThemeControls?.();
  recordThemeDiagnostic("theme.decision", { targetTheme: next, decision: "applied", reason,
    stableMs: automaticThemeStableMs, durationMs: stableDurationMs });
  finishThemeDiagnosticTransition(transition);
  return true;
}
export function setAppearanceThemeMode(mode, { theme = null } = {}) {
  if (!["auto", "manual"].includes(mode)) return;
  const previousTheme = appearance.theme, previousMode = appearance.themeMode;
  // 用户主动选择模式或修改阈值时重置采样补偿；估计偏移只保留在当前页面内存中。
  resetAutomaticThemeCompensation();
  appearance.themeMode = mode;
  if (mode === "auto") appearance.theme = automaticAppearanceTheme();
  // 用户要求自动/浅色/深色三个预览选项；浅色和深色仍沿用原有 manual 存储与自动模式切换约束。
  else if (["light", "dark"].includes(theme)) appearance.theme = theme;
  const transition = previousMode !== mode || previousTheme !== appearance.theme
    ? beginThemeDiagnosticTransition(appearance.theme, "manual", { previousTheme, previousMode }) : null;
  applyAppearance(true);
  syncAppearanceThemeControls?.();
  finishThemeDiagnosticTransition(transition);
  recordThemeDiagnostic("theme.snapshot", { reason: "manual" });
}
function setAutomaticThemeLightLux(value) {
  if (!Number.isInteger(value) || value < automaticThemeLightLuxMin || value > automaticThemeLightLuxMax) return false;
  if (appearance.automaticThemeLightLux === value) return true;
  appearance.automaticThemeLightLux = value;
  // 用户调整阈值后清除采样补偿并重新累计 5 秒，不能沿用旧候选；暗色阈值和昼夜范围继续沿用原规则。
  resetAutomaticThemeCompensation();
  applyAutomaticAppearance(false, { reason: "manual" });
  applyAppearance(true);
  syncAppearanceThemeControls?.();
  recordThemeDiagnostic("theme.snapshot", { reason: "manual" });
  return true;
}
export function mountAutomaticThemeLightSetting(host) {
  const row = element("label", "appearance-field"), controls = element("span", "appearance-font");
  const input = element("input"), output = element("output");
  input.id = "appearance-auto-light-lux"; input.type = "range";
  input.min = String(automaticThemeLightLuxMin); input.max = String(automaticThemeLightLuxMax); input.step = "1";
  input.setAttribute("aria-label", "切换浅色的光照阈值"); input.setAttribute("aria-describedby", "appearance-auto-light-hint");
  output.setAttribute("for", input.id);
  controls.append(input, output); row.append(element("span", "", "光照阈值"), controls);
  const hint = element("p", "appearance-reading-font-hint");
  hint.id = "appearance-auto-light-hint"; host.append(row, hint);
  const lightReading = () => {
    // 用户要求展示当前光照：保留最多两位小数，不套用阈值的 5～40 范围；缺少读数时不伪装成 0 lux。
    const lux = ambientLightStatus.lux;
    const current = ambientLightStatus.supported && Number.isFinite(lux) && lux >= 0 ? Number(lux.toFixed(2)) : "--";
    const text = `光线达到此数值后切换为浅色，当前 ${current} lux`;
    if (hint.textContent !== text) hint.textContent = text;
  };
  const sync = () => {
    const automatic = appearance.themeMode === "auto", value = currentAutomaticThemeLightLux();
    host.hidden = !automatic; input.disabled = !automatic;
    input.value = String(value); output.textContent = `${value} lux`;
    input.style.setProperty("--range-fill",
      `${(value - automaticThemeLightLuxMin) / (automaticThemeLightLuxMax - automaticThemeLightLuxMin) * 100}%`);
    lightReading();
  };
  sync.lightReading = lightReading;
  input.oninput = () => setAutomaticThemeLightLux(Number(input.value));
  sync();
  return sync;
}
export function applyAppearance(save = false) {
  const root = document.documentElement;
  const themeChanged = root.dataset.theme && root.dataset.theme !== appearance.theme;
  if (themeChanged) root.classList.add("theme-transition");
  const { background, foreground, accent, contrast } = appearance.colors[appearance.theme];
  root.dataset.theme = appearance.theme;
  root.style.colorScheme = appearance.theme;
  const brightness = [1, 3, 5].map(index => parseInt(accent.slice(index, index + 2), 16));
  const onAccent = brightness[0] * .299 + brightness[1] * .587 + brightness[2] * .114 > 150 ? "#171717" : "#ffffff";
  const values = {
    bg: background, text: foreground, accent, "on-accent": onAccent,
    ...appearanceTextColors(appearance.theme, { foreground, contrast }),
    ...appearanceSurfaceColors(appearance.theme, { background, foreground, contrast }),
    side: mixColor(background, foreground, .035), code: mixColor(background, foreground, .05),
    hover: mixColor(background, foreground, .075), line: mixColor(background, foreground, .14),
    "font-size": `${appearance.fontSize}px`,
    "ui-font-family": fontStack(appearance.uiFont, defaultUiFont),
    "code-font-family": fontStack(appearance.codeFont, defaultCodeFont),
    "ui-font-weight": String(appearance.uiFontWeight), "code-font-weight": String(appearance.codeFontWeight),
    "code-font-size": `${appearance.codeFontSize}px`,
    "conversation-max-width": `${appearance.conversationWidth}px`,
    "file-reading-font-size": `${currentFileReadingFontSize()}px`,
    danger: appearance.theme === "dark" ? "#ff9c97" : "#b42318", green: appearance.theme === "dark" ? "#77c7a0" : "#168350"
  };
  for (const [name, value] of Object.entries(values)) root.style.setProperty(`--${name}`, value);
  if (themeChanged) {
    clearTimeout(appearanceTransitionTimer);
    setAppearanceTransitionTimer(setTimeout(() => root.classList.remove("theme-transition"), 300));
  }
  $('meta[name="theme-color"]')?.setAttribute("content", background);
  syncRemoteBackground();
  if (save) {
    try { localStorage.setItem("codex-elves-web-appearance", JSON.stringify(localAppearance())); }
    catch { notice("外观已应用，但浏览器不允许保存；重新打开页面后将恢复默认。", true); }
  }
  resizeComposerInput();
  resizeEditorInput(editingMessage?.input);
  refitComposerMenus();
  if ($("#messages")) scheduleMermaidBlocks();
}
// 启动时应用外观、读取远程背景，并开始自动主题、主题诊断与环境光订阅。
export function startAppearance() {
  applyAutomaticAppearance(false, { immediate: true, reason: "startup" });
  applyAppearance();
  recordThemeDiagnostic("theme.snapshot", { reason: "startup" });
  // 侧边页面沿用父页面底图，不再为每个标签从 IndexedDB 重读整张背景图片。
  if (!workspaceSidePanel) {
    const initialRemoteBackgroundOperation = setRemoteBackgroundOperation(remoteBackgroundOperation + 1);
    void readRemoteBackground().then(record => {
      if (initialRemoteBackgroundOperation !== remoteBackgroundOperation) return;
      if (record && typeof record.dataUrl === "string" && remoteBackgroundDataPattern.test(record.dataUrl))
        setRemoteBackground({ dataUrl: record.dataUrl, name: String(record.name || "背景图片") });
      syncRemoteBackground();
      syncAppearanceThemeControls?.();
    }).catch(() => {
      if (initialRemoteBackgroundOperation !== remoteBackgroundOperation) return;
      setRemoteBackgroundError("无法读取已保存的背景图片。");
      syncAppearanceThemeControls?.();
    });
  }
  setInterval(() => {
    applyAutomaticAppearance(true);
    recordThemeDiagnostic("theme.snapshot", { reason: "timer" });
  }, 60_000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      applyAutomaticAppearance(true, { reason: "visibility" });
      recordThemeDiagnostic("theme.snapshot", { reason: "visibility" });
    }
  });
  globalThis.addEventListener("pagehide", clearThemeDiagnosticTimers);
  subscribeAndroidAmbientLight(status => {
    setAmbientLightStatus(status);
    observeThemeLight(status);
    recordThemeDiagnostic("theme.ambient", { reason: "sensor" });
    applyAutomaticAppearance(true, { reason: "sensor" });
    syncAppearanceThemeControls?.lightReading?.();
  });
}
// 侧边面板跟随主页面保存的外观设置。
export function installSidePanelAppearanceSync() {
  window.addEventListener("storage", event => {
    if (!workspaceSidePanel || event.key !== "codex-elves-web-appearance") return;
    const shared = Object.fromEntries(["conversationWidth", "projectThreadLimit", "sessionTypes", "sessionInstances", "sessionFilter"]
      .map(key => [key, appearance[key]]));
    setAppearance({ ...readAppearance(), ...shared }); applyAppearance();
  });
}
