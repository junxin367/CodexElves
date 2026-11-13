(() => {
  if (window.top !== window) return;
  let previous = -1, pending = null, running = false;
  async function flush() {
    if (running) return;
    running = true;
    while (pending !== null) {
      const count = pending; pending = null;
      try { await window.__TAURI__.core.invoke("remote_unread_badge", { count }); }
      catch { if (previous === count) previous = -1; }
    }
    running = false;
  }
  // 仅主窗口注入此桥；多窗口共享会话未读状态，不能把每个窗口的数字相加。
  Object.defineProperty(window, "__CODEX_ELVES_UNREAD_BADGE__", {
    value: Object.freeze({
      setCount(count) {
        if (!Number.isSafeInteger(count) || count < 0 || count > 0xffffffff || count === previous) return;
        previous = count; pending = count;
        void flush();
      }
    })
  });
})();
