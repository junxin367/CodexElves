(() => {
  if (window.top !== window) return;
  // 用户要求同一程序打开独立会话窗口；仅传会话 ID，URL、窗口和来源校验均由原生处理。
  Object.defineProperty(window, "__CODEX_ELVES_SESSION_WINDOWS__", {
    value: Object.freeze({
      open: threadId => window.__TAURI__.core.invoke("remote_open_session_window", { threadId }),
      openPreview: preview => window.__TAURI__.core.invoke("remote_open_session_window", { preview })
    })
  });
  let scheduled = false, previous = "", pending = null, running = false, colorContext = null;
  const transitions = new Set(), colorProperties = new Set(["--bg", "--text", "--line"]);
  async function flush() {
    if (running) return;
    running = true;
    // 取色和渐变帧串行发送并合并为最新颜色，慢回执不积压旧帧或倒退原生应用顶栏。
    while (pending) {
      const next = pending; pending = null;
      try { await window.__TAURI__.core.invoke("remote_titlebar_theme", { theme: next.theme }); }
      catch { if (previous === next.signature) previous = ""; }
    }
    running = false;
  }
  function rgb(value, fallback) {
    value = String(value || "").trim();
    if (/^#[\da-f]{6}$/i.test(value)) return [1, 3, 5].map(index => parseInt(value.slice(index, index + 2), 16));
    if (/^#[\da-f]{3}$/i.test(value)) return [1, 2, 3].map(index => parseInt(value[index].repeat(2), 16));
    const match = value.match(/^rgba?\(\s*([\d.]+)\s*[, ]\s*([\d.]+)\s*[, ]\s*([\d.]+)(?:\s*[,/]\s*1(?:\.0+)?)?\s*\)$/i);
    if (match && match.slice(1).every(part => Number.isFinite(Number(part)) && Number(part) <= 255)) {
      return match.slice(1).map(part => Math.round(Number(part)));
    }
    // CSS 颜色渐变可能返回小数、oklab 或 color(srgb)；经浏览器转换为不透明 RGB 字节。
    if (!window.CSS?.supports?.("color", value)) return fallback;
    if (!colorContext) {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 1;
      colorContext = canvas.getContext("2d", { willReadFrequently: true });
    }
    if (!colorContext) return fallback;
    colorContext.clearRect(0, 0, 1, 1);
    colorContext.fillStyle = value;
    colorContext.fillRect(0, 0, 1, 1);
    const pixel = colorContext.getImageData(0, 0, 1, 1).data;
    return pixel[3] === 255 ? Array.from(pixel.slice(0, 3)) : fallback;
  }
  function publish() {
    scheduled = false;
    const root = document.documentElement;
    if (!root || !window.__TAURI__?.core?.invoke || document.visibilityState === "hidden") return;
    const style = getComputedStyle(root);
    const dark = root.dataset.theme === "dark" || root.dataset.theme !== "light"
      && (style.colorScheme === "dark" || style.colorScheme !== "light" && matchMedia("(prefers-color-scheme:dark)").matches);
    const background = rgb(style.getPropertyValue("--bg"), rgb(style.backgroundColor, dark ? [24, 24, 24] : [255, 255, 255]));
    const foreground = rgb(style.getPropertyValue("--text"), rgb(style.color, dark ? [237, 237, 237] : [23, 23, 23]));
    const border = rgb(style.getPropertyValue("--line"), background.map((part, index) => Math.round(part * .86 + foreground[index] * .14)));
    const theme = { background, foreground, border, dark }, signature = JSON.stringify(theme);
    if (signature !== previous) {
      previous = signature;
      // 用户要求跟随页面实际外观；只同步 RGB，不向页面授予窗口操作或连接设置权限。
      pending = { theme, signature }; void flush();
    }
    // 用户要求与网页渐变保持同一节奏：采样 CSS 当前帧，不在 Rust 端再启一个动画计时器。
    // 相邻帧颜色暂未变化也继续采样；仅在这三个颜色属性的原生 CSS 过渡期间持续读取。
    if (transitions.size) schedule();
  }
  function schedule() {
    if (scheduled) return;
    scheduled = true; requestAnimationFrame(publish);
  }
  function start() {
    const observer = new MutationObserver(schedule);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style", "class"] });
    for (const type of ["transitionrun", "transitionend", "transitioncancel"]) {
      document.documentElement.addEventListener(type, event => {
        if (event.target !== document.documentElement || !colorProperties.has(event.propertyName)) return;
        if (type === "transitionrun") transitions.add(event.propertyName);
        else transitions.delete(event.propertyName);
        schedule();
      });
    }
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") { transitions.clear(); pending = null; }
      else { previous = ""; schedule(); }
    });
    matchMedia("(prefers-color-scheme:dark)").addEventListener("change", schedule);
    window.addEventListener("load", schedule, { once: true });
    schedule();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
})();
