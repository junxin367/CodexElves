// 外观设置模型：主题预设与默认值、读取外观、自动明暗主题判定、远程背景图存储与主题诊断；把外观应用到页面在 apply.js。

import { recordDiagnostic } from "../../diagnostics.js";
import { isAndroidContainer } from "../../platform.js";

const appearancePresets = {
  // 用户要求独立调节对比度：每套主题的明暗模式分别保存 0～100，默认沿用 Codex 浅色 45、深色 60。
  light: { background: "#ffffff", foreground: "#1f1f1f", accent: "#171717", contrast: 45 },
  dark: { background: "#171717", foreground: "#ededed", accent: "#ededed", contrast: 60 }
};
// 用户要求主题与明暗模式独立；两套主题分别保存浅色/深色调色，切换主题不覆盖另一套自定义颜色。
export const appearanceThemes = {
  chatgpt: { label: "ChatGPT", colors: appearancePresets },
  "vscode-plus": { label: "VS Code Plus", colors: {
    light: { background: "#ffffff", foreground: "#3b3b3b", accent: "#0078d4", contrast: 45 },
    dark: { background: "#1f1f1f", foreground: "#cccccc", accent: "#0078d4", contrast: 60 }
  } }
};
export const defaultUiFont = '-apple-system,BlinkMacSystemFont,"Segoe UI","Microsoft YaHei",sans-serif';
export const defaultCodeFont = 'ui-monospace,"SFMono-Regular","SF Mono",Menlo,Consolas,"Liberation Mono",monospace';
// 用户要求先保留正文宽度上限，仅在右侧实际空间不足时收窄；有足够留白才对齐整个可视窗口中心，与 style.css 同步。
export const conversationWidthDefault = 950, conversationWidthMin = 320, conversationWidthMax = 3840;
export const validFontFamily = value => typeof value === "string" && value.length <= 100
  && !/[{};<>\u0000-\u001f]/u.test(value);
export const fontStack = (family, fallback) => family ? `${JSON.stringify(family)},${fallback}` : fallback;
export const projectThreadLimits = [5, 10, 20, 50];
// 用户要求文件阅读字号统一控制变更预览和文件查看，范围 6～14px、默认 12px、步长 1px；文件列表继续使用原字号。
export const fileReadingFontMin = 6, fileReadingFontMax = 14, fileReadingFontDefault = 12;
// 用户要求光照阈值范围为 5～40 lux、默认 10 lux、步长 1 lux；存储校验和滑块共用此范围，已保存的有效选择继续保留。
export const automaticThemeLightLuxMin = 5, automaticThemeLightLuxMax = 40, automaticThemeLightLuxDefault = 10;
function normalizeAutomaticThemeLightLux(value) {
  return Number.isInteger(value)
    ? Math.min(automaticThemeLightLuxMax, Math.max(automaticThemeLightLuxMin, value)) : automaticThemeLightLuxDefault;
}
export function defaultAppearance(theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") {
  const palettes = Object.fromEntries(Object.entries(appearanceThemes).map(([key, preset]) =>
    [key, { light: { ...preset.colors.light }, dark: { ...preset.colors.dark } }]));
  return { theme, themeMode: "manual", automaticThemeLightLux: automaticThemeLightLuxDefault,
    // 用户要求字体默认参考 Codex：系统界面字体/系统等宽字体，均为 14px；保留已保存字号和独立文件阅读字号。
    fontSize: 14, codeFontSize: 14, uiFont: "", codeFont: "", uiFontWeight: 400, codeFontWeight: 400,
    conversationWidth: conversationWidthDefault, sessionTypes: { codex: true, claude: false }, sessionInstances: null, sessionFilter: "all",
    themePreset: "chatgpt", palettes, colors: palettes.chatgpt,
    fileReadingFontSize: fileReadingFontDefault, projectThreadLimit: 5, backgroundOpacity: 35, backgroundFit: "cover" };
}
export function readAppearance() {
  const value = defaultAppearance();
  try {
    const saved = JSON.parse(localStorage.getItem("codex-elves-web-appearance") || "null");
    if (!saved || typeof saved !== "object") return value;
    if (["light", "dark"].includes(saved.theme)) value.theme = saved.theme;
    if (["auto", "manual"].includes(saved.themeMode)) value.themeMode = saved.themeMode;
    if (Object.hasOwn(appearanceThemes, saved.themePreset)) value.themePreset = saved.themePreset;
    // 用户要求除外观外跨端共用；旧字段仅由迁移流程读取，不能在连接新服务前恢复成设备独有配置。
    for (const key of ["uiFont", "codeFont"]) if (validFontFamily(saved[key])) value[key] = saved[key].trim();
    for (const key of ["uiFontWeight", "codeFontWeight"])
      if ([400, 500, 600].includes(saved[key])) value[key] = saved[key];
    if (Number.isInteger(saved.codeFontSize)) value.codeFontSize = Math.min(24, Math.max(8, saved.codeFontSize));
    value.automaticThemeLightLux = normalizeAutomaticThemeLightLux(saved.automaticThemeLightLux);
    if (Number.isInteger(saved.backgroundOpacity)) value.backgroundOpacity = Math.min(100, Math.max(1, saved.backgroundOpacity));
    if (["cover", "contain"].includes(saved.backgroundFit)) value.backgroundFit = saved.backgroundFit;
    if (Number.isInteger(saved.fontSize)) value.fontSize = Math.min(20, Math.max(10, saved.fontSize));
    if (Number.isInteger(saved.fileReadingFontSize))
      value.fileReadingFontSize = Math.min(fileReadingFontMax, Math.max(fileReadingFontMin, saved.fileReadingFontSize));
    for (const preset of Object.keys(appearanceThemes)) for (const theme of ["light", "dark"]) {
      for (const name of ["background", "foreground", "accent"]) {
        const color = saved.palettes?.[preset]?.[theme]?.[name]
          ?? (preset === value.themePreset ? saved.colors?.[theme]?.[name] : null);
        if (/^#[0-9a-f]{6}$/i.test(color)) value.palettes[preset][theme][name] = color;
      }
      const contrast = saved.palettes?.[preset]?.[theme]?.contrast
        ?? (preset === value.themePreset ? saved.colors?.[theme]?.contrast : null);
      // 旧设置缺失此项时保留默认值；零值有效，损坏或小数值不能改变当前调色。
      if (Number.isInteger(contrast)) value.palettes[preset][theme].contrast = Math.min(100, Math.max(0, contrast));
    }
    value.colors = value.palettes[value.themePreset];
  } catch {}
  return value;
}
export function localAppearance(value = appearance) {
  const keys = ["theme", "themeMode", "automaticThemeLightLux", "fontSize", "codeFontSize",
    "uiFont", "codeFont", "uiFontWeight", "codeFontWeight", "themePreset", "palettes", "colors",
    "fileReadingFontSize", "backgroundOpacity", "backgroundFit"];
  return Object.fromEntries(keys.map(key => [key, value[key]]));
}
export function legacySharedAppearance() {
  try {
    const saved = JSON.parse(localStorage.getItem("codex-elves-web-appearance") || "null"), patch = {};
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return patch;
    if (Number.isInteger(saved.conversationWidth) && saved.conversationWidth >= 320 && saved.conversationWidth <= 3840)
      patch.conversationWidth = saved.conversationWidth;
    if (projectThreadLimits.includes(saved.projectThreadLimit)) patch.projectThreadLimit = saved.projectThreadLimit;
    for (const key of ["sessionTypes", "sessionInstances"]) {
      if (saved[key] && typeof saved[key] === "object" && !Array.isArray(saved[key]))
        patch[key] = Object.fromEntries(Object.entries(saved[key]).filter(([id, enabled]) =>
          /^[a-z][a-z0-9:_-]{0,159}$/.test(id) && typeof enabled === "boolean"));
    }
    const filter = saved.sessionFilter;
    const filters = Array.isArray(filter) ? filter : typeof filter === "string" ? [filter] : [];
    const ids = [...new Set(filters.filter(id => typeof id === "string" && /^[a-z][a-z0-9:_-]{0,159}$/.test(id)))];
    if (filters.includes("all")) patch.sessionFilter = "all";
    else if (ids.length) patch.sessionFilter = ids;
    return patch;
  } catch { return {}; }
}
export function currentFileReadingFontSize() {
  // 用户已将默认值改为固定 12px；兼容旧版保存的 null，旧范围内的 15/16px 则收敛到 14px。
  return Math.min(fileReadingFontMax, Math.max(fileReadingFontMin, appearance.fileReadingFontSize ?? fileReadingFontDefault));
}
export function currentAutomaticThemeLightLux() {
  return normalizeAutomaticThemeLightLux(appearance.automaticThemeLightLux);
}
export let ambientLightStatus = { supported: false, lux: null };
export function setAmbientLightStatus(value) { return ambientLightStatus = value; }
export let syncAppearanceThemeControls = null;
export function setSyncAppearanceThemeControls(value) { return syncAppearanceThemeControls = value; }
export let appearance = readAppearance();
export function setAppearance(value) { return appearance = value; }
export let appearanceTransitionTimer = null;
export function setAppearanceTransitionTimer(value) { return appearanceTransitionTimer = value; }
const remoteBackgroundKey = "remote-session";
const remoteBackgroundMimes = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
export const remoteBackgroundDataPattern = /^data:image\/(?:png|jpeg|webp|gif);base64,[a-z0-9+/=]+$/i;
export let remoteBackground = { dataUrl: "", name: "" };
export function setRemoteBackground(value) { return remoteBackground = value; }
export let remoteBackgroundOperation = 0;
export function setRemoteBackgroundOperation(value) { return remoteBackgroundOperation = value; }
export let remoteBackgroundBusy = false;
export function setRemoteBackgroundBusy(value) { return remoteBackgroundBusy = value; }
export let remoteBackgroundError = "";
export function setRemoteBackgroundError(value) { return remoteBackgroundError = value; }
function openRemoteBackgroundDatabase() {
  return new Promise((resolve, reject) => {
    if (!globalThis.indexedDB) { reject(new Error("当前浏览器无法保存背景图片。")); return; }
    const request = indexedDB.open("codex-elves-appearance-v1", 1);
    request.onupgradeneeded = () => request.result.createObjectStore("images");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("无法打开背景图片存储。"));
    request.onblocked = () => reject(new Error("背景图片存储被其他页面占用，请关闭旧页面后重试。"));
  });
}
async function remoteBackgroundStore(mode, operation) {
  const database = await openRemoteBackgroundDatabase();
  try {
    return await new Promise((resolve, reject) => {
      const transaction = database.transaction("images", mode);
      const request = operation(transaction.objectStore("images"));
      let result;
      request.onsuccess = () => { result = request.result; };
      request.onerror = () => reject(request.error || new Error("背景图片存储操作失败。"));
      transaction.oncomplete = () => resolve(result);
      transaction.onerror = () => reject(transaction.error || new Error("背景图片存储操作失败。"));
      transaction.onabort = () => reject(transaction.error || new Error("背景图片存储操作已取消。"));
    });
  } finally { database.close(); }
}
export function readRemoteBackground() {
  return remoteBackgroundStore("readonly", store => store.get(remoteBackgroundKey));
}
export function writeRemoteBackground(record) {
  return remoteBackgroundStore("readwrite", store => store.put(record, remoteBackgroundKey));
}
export function deleteRemoteBackground() {
  return remoteBackgroundStore("readwrite", store => store.delete(remoteBackgroundKey));
}
export function readRemoteBackgroundFile(file) {
  if (!remoteBackgroundMimes.has(file.type)) return Promise.reject(new Error("请选择 PNG、JPG、WebP 或 GIF 图片。"));
  if (!file.size || file.size > 15 * 1024 * 1024) return Promise.reject(new Error("背景图片不能超过 15 MB。"));
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("无法读取所选图片。"));
    reader.onload = () => {
      const dataUrl = String(reader.result || "");
      if (!remoteBackgroundDataPattern.test(dataUrl)) { reject(new Error("图片内容无法识别。")); return; }
      const image = new Image();
      image.onerror = () => reject(new Error("图片内容损坏或无法解码。"));
      image.onload = () => {
        if (!image.naturalWidth || !image.naturalHeight || image.naturalWidth * image.naturalHeight > 100_000_000) {
          reject(new Error("图片尺寸无效或超过 1 亿像素。"));
        } else resolve(dataUrl);
      };
      image.src = dataUrl;
    };
    reader.readAsDataURL(file);
  });
}
export const automaticThemeStableMs = 5000;
const automaticThemeDarkLux = 10;
// 用户要求按切换前后光照变化补偿，从第一次切深色就处理，不等待往返次数、不改写用户阈值。
// 原生光照节流为 2 秒；切深色后只在 3 秒内估计界面引起的偏移，之后真实环境变化仍稳定 5 秒判断。
const automaticThemeShiftWindowMs = 3000;
export let automaticThemeCandidate = null;
export function setAutomaticThemeCandidate(value) { return automaticThemeCandidate = value; }
export let automaticThemeLuxShift = 0;
export function setAutomaticThemeLuxShift(value) { return automaticThemeLuxShift = value; }
export let automaticThemeShiftProbe = null;
export function setAutomaticThemeShiftProbe(value) { return automaticThemeShiftProbe = value; }
function automaticThemeThresholds() {
  const lightLux = currentAutomaticThemeLightLux();
  // 深色采样按实测偏移还原到切换前的光照基准；浅色仍用原读数，阈值设置保持 5～40 lux。
  return { lightLux, darkLux: Math.min(automaticThemeDarkLux, lightLux - 1),
    effectiveLightLux: lightLux + (appearance.theme === "dark" ? automaticThemeLuxShift : 0) };
}
export function resetAutomaticThemeCompensation() {
  automaticThemeCandidate = null;
  automaticThemeLuxShift = 0;
  automaticThemeShiftProbe = null;
}
export function automaticThemeAtNight(date = new Date()) {
  // 用户要求夜间按设备本地时间从 20:00（含）到次日 08:00（不含）；白天不保留夜间采样补偿。
  const hour = date.getHours();
  return hour < 8 || hour >= 20;
}
export function observeAutomaticThemeShift(now, reason) {
  const probe = automaticThemeShiftProbe;
  if (!probe) return;
  const durationMs = Math.max(0, now - probe.since);
  if (appearance.theme !== "dark" || durationMs > automaticThemeShiftWindowMs) {
    automaticThemeShiftProbe = null;
    return;
  }
  if (reason !== "sensor" || !ambientLightStatus.supported || !Number.isFinite(ambientLightStatus.lux)) return;
  // 有采样年龄时排除切换前的缓存；旧 APK 未提供年龄时沿用真实传感器回调，不伪造采样时间。
  if (Number.isFinite(ambientLightStatus.sampleAgeMs) && ambientLightStatus.sampleAgeMs > durationMs) return;
  const shift = ambientLightStatus.lux - probe.lux;
  if (probe.observed && shift === automaticThemeLuxShift) return;
  probe.observed = true;
  automaticThemeLuxShift = shift;
  recordThemeDiagnostic("theme.calibration", { reason, phase: "after", previousLux: probe.lux, durationMs });
}
// 切换诊断只观测、不推进主题判断。最多留 8 个观测点，回看最近 30 秒；无近期点时标注旧读数年龄。
const themeLightSamples = [];
let themeDiagnosticSequence = 0;
const themeDiagnosticTimers = new Set();
function themeDiagnosticNow() {
  return globalThis.performance?.now?.() ?? Date.now();
}
function themeDiagnosticFields(extra = {}, sample = themeLightSamples.at(-1)) {
  const date = new Date();
  return {
    theme: appearance.theme, themeMode: appearance.themeMode, themeSequence: themeDiagnosticSequence,
    ...automaticThemeThresholds(), themeLuxShift: automaticThemeLuxShift,
    supported: sample?.supported ?? ambientLightStatus.supported,
    lux: sample ? sample.lux : ambientLightStatus.lux,
    previousLux: sample?.previousLux,
    sampleAgeMs: sample?.sampledAtMs != null ? Math.max(0, Math.round(themeDiagnosticNow() - sample.sampledAtMs)) : undefined,
    localHour: date.getHours(), timezoneOffsetMinutes: date.getTimezoneOffset(), visible: !document.hidden,
    ...extra
  };
}
export function recordThemeDiagnostic(name, extra = {}, sample) {
  if (isAndroidContainer()) recordDiagnostic(name, themeDiagnosticFields(extra, sample));
}
export function observeThemeLight(status) {
  if (!isAndroidContainer()) return;
  const now = themeDiagnosticNow(), previous = themeLightSamples.at(-1);
  themeLightSamples.push({
    supported: status.supported, lux: status.lux, previousLux: previous?.lux, observedAtMs: now,
    // 老 APK 没有真实采样年龄时保持未知；不能把收到缓存回调的时间当成传感器采样时间。
    sampledAtMs: status.supported && Number.isSafeInteger(status.sampleAgeMs) && status.sampleAgeMs >= 0
      ? now - status.sampleAgeMs : null
  });
  if (themeLightSamples.length > 8) themeLightSamples.shift();
}
export function clearThemeDiagnosticTimers() {
  for (const timer of themeDiagnosticTimers) clearTimeout(timer);
  themeDiagnosticTimers.clear();
}
export function beginThemeDiagnosticTransition(targetTheme, reason,
  { previousTheme = appearance.theme, previousMode = appearance.themeMode } = {}) {
  if (!isAndroidContainer()) return null;
  clearThemeDiagnosticTimers();
  const transition = {
    previousTheme, targetTheme, reason, themeSequence: ++themeDiagnosticSequence
  };
  const now = themeDiagnosticNow(), recent = themeLightSamples.filter(sample => now - sample.observedAtMs <= 30_000);
  for (const sample of recent.length ? recent : themeLightSamples.slice(-1))
    recordThemeDiagnostic("theme.sample", { ...transition, phase: "before", theme: previousTheme, themeMode: previousMode }, sample);
  recordThemeDiagnostic("theme.transition", { ...transition, phase: "before", theme: previousTheme, themeMode: previousMode });
  return { ...transition, startedAtMs: now };
}
export function finishThemeDiagnosticTransition(transition) {
  if (!transition) return;
  const { startedAtMs, ...fields } = transition;
  recordThemeDiagnostic("theme.transition", { ...fields, phase: "after" });
  // 2/5/10 秒回调只记录最新可用读数及实际间隔，不调用主题判断、不推进 5 秒稳定候选。
  for (const delayMs of [2000, 5000, 10_000]) {
    const timer = setTimeout(() => {
      themeDiagnosticTimers.delete(timer);
      if (fields.themeSequence === themeDiagnosticSequence)
        recordThemeDiagnostic("theme.sample", { ...fields, phase: "after", delayMs,
          durationMs: Math.max(0, themeDiagnosticNow() - startedAtMs) });
    }, delayMs);
    themeDiagnosticTimers.add(timer);
  }
}
export function automaticAppearanceTheme(date = new Date(), currentTheme = appearance.theme) {
  if (automaticThemeAtNight(date)) {
    if (!ambientLightStatus.supported || ambientLightStatus.lux === null) return "dark";
    const { lightLux, darkLux } = automaticThemeThresholds();
    const lux = Math.max(0, ambientLightStatus.lux - (currentTheme === "dark" ? automaticThemeLuxShift : 0));
    if (lux <= darkLux) return "dark";
    if (lux >= lightLux) return "light";
  } else return "light";
  return currentTheme;
}
export function mixColor(left, right, weight) {
  return "#" + [1, 3, 5].map(index => Math.round(
    parseInt(left.slice(index, index + 2), 16) * (1 - weight) + parseInt(right.slice(index, index + 2), 16) * weight
  ).toString(16).padStart(2, "0")).join("");
}
function appearanceContrast(theme, value) {
  // 对齐 Codex 原生对比度曲线：45/60 是浅/深色默认值，60 是变化灵敏度的分母；
  // 高于默认值后加倍增强，保持默认表面颜色不变；与两套 appearanceThemes 默认值同步核对。
  const baseline = theme === "dark" ? 60 : 45;
  const setting = Number.isInteger(value) ? Math.min(100, Math.max(0, value)) : baseline;
  const scaled = setting / 100 + (setting - baseline) / 60 * .7;
  return setting <= baseline ? scaled : baseline / 100 + (scaled - baseline / 100) * 2;
}
function appearanceAlphaColor(color, alpha) {
  const channels = [1, 3, 5].map(index => parseInt(color.slice(index, index + 2), 16));
  return `rgba(${channels.join(", ")}, ${Math.min(1, Math.max(0, alpha)).toFixed(3)})`;
}
export function appearanceTextColors(theme, { foreground, contrast: setting }) {
  const dark = theme === "dark", contrast = appearanceContrast(theme, setting);
  // 用户要求会话栏参考 Codex：主标题仍由前景色控制，项目名、分组等辅助文字按对比度派生；
  // 使用原生三位 alpha，使背景图片与各层表面自然透出，style.css 的 thread-project 同步使用 muted。
  // 侧栏高亮与 appearanceSurfaceColors 的 hover 比例保持一致，但不让透明文字覆盖实色表面。
  return {
    muted: appearanceAlphaColor(foreground, dark ? .42 + .13 * contrast : .45 + .1 * contrast),
    "sidebar-row-highlight": appearanceAlphaColor(foreground, dark ? .06 + .03 * contrast : .04 + .03 * contrast),
  };
}
export function appearanceSurfaceColors(theme, { background, foreground, contrast: setting }) {
  // 用户要求排队、菜单和 hover 按 Codex 外观派生，不能把某个主题的 HEX 值作为其他主题的固定背景。
  // 对比度来自当前主题/模式；缺项兼容原来的深色 60、浅色 45，两种表面保留各自的原生公式。
  const dark = theme === "dark", contrast = appearanceContrast(theme, setting);
  const control = mixColor(background, dark ? foreground : "#ffffff", dark ? .06 + .05 * contrast : .09 + .04 * contrast);
  const composer = mixColor(background, dark ? foreground : "#ffffff", dark ? .08 + .08 * contrast : .16 + .12 * contrast);
  const hoverStrength = dark ? .06 + .03 * contrast : .04 + .03 * contrast;
  return {
    "composer-bg": composer,
    // 用户要求模型按钮 hover 参考 Codex：在输入框表面叠加随对比度变化的文字基色，不能使用更暗的页面 hover。
    "composer-hover": mixColor(composer, foreground, hoverStrength),
    "control-bg": control,
    "control-hover": mixColor(control, foreground, hoverStrength),
    // 用户要求深色模型／推理分组线更清晰：原生重边框比例随对比度变化，默认 .12 + .06 * .6 = .156；浅色保留原页面线色。
    "menu-divider": dark ? mixColor(control, foreground, .12 + .06 * contrast) : mixColor(background, foreground, .14),
    // 普通文字提示在浅色下反色；上下文等详细提示沿用控件表面，不使用这组普通 tooltip 颜色。
    "tooltip-bg": dark ? control : foreground,
    "tooltip-text": dark ? foreground : background,
  };
}
