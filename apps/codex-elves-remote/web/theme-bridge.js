(() => {
  if (window.top !== window) return;
  // 用户要求同一程序打开独立会话窗口；仅传会话 ID，URL、窗口和来源校验均由原生处理。
  Object.defineProperty(window, "__CODEX_ELVES_SESSION_WINDOWS__", {
    value: Object.freeze({
      open: threadId => window.__TAURI__.core.invoke("remote_open_session_window", { threadId }),
      openPreview: preview => window.__TAURI__.core.invoke("remote_open_session_window", { preview })
    })
  });
  let scheduled = false, previous = "", pending = null, running = false;
  async function flush() {
    if (running) return;
    running = true;
    // 连续拖动取色时串行发送并合并为最新颜色，旧请求不能在较新颜色之后完成并改回标题栏。
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
    const match = value.match(/^rgba?\(\s*(\d+)\s*[, ]\s*(\d+)\s*[, ]\s*(\d+)(?:\s*[,/]\s*1(?:\.0+)?)?\s*\)$/i);
    return match && match.slice(1).every(part => Number(part) <= 255) ? match.slice(1).map(Number) : fallback;
  }
  function publish() {
    scheduled = false;
    const root = document.documentElement;
    if (!root || !window.__TAURI__?.core?.invoke) return;
    const style = getComputedStyle(root);
    const dark = root.dataset.theme === "dark" || root.dataset.theme !== "light"
      && (style.colorScheme === "dark" || style.colorScheme !== "light" && matchMedia("(prefers-color-scheme:dark)").matches);
    const background = rgb(style.getPropertyValue("--bg"), rgb(style.backgroundColor, dark ? [24, 24, 24] : [255, 255, 255]));
    const foreground = rgb(style.getPropertyValue("--text"), rgb(style.color, dark ? [237, 237, 237] : [23, 23, 23]));
    const border = rgb(style.getPropertyValue("--line"), background.map((part, index) => Math.round(part * .86 + foreground[index] * .14)));
    const theme = { background, foreground, border, dark }, signature = JSON.stringify(theme);
    if (signature === previous) return;
    previous = signature;
    // 用户要求跟随页面实际外观（含自动模式与自定义颜色）；只同步 RGB，不向页面授予窗口操作或连接设置权限。
    pending = { theme, signature }; void flush();
  }
  function schedule() {
    if (scheduled) return;
    scheduled = true; requestAnimationFrame(publish);
  }
  function start() {
    const observer = new MutationObserver(schedule);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style", "class"] });
    matchMedia("(prefers-color-scheme:dark)").addEventListener("change", schedule);
    window.addEventListener("load", schedule, { once: true });
    schedule();
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
})();
