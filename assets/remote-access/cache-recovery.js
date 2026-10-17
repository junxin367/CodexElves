(() => {
  "use strict";

  const hexPattern = /^[a-f0-9]{64}$/;
  const pathPattern = /^\/(?:[A-Za-z0-9._-]+\/)*[A-Za-z0-9._-]+$/;
  const excludedPaths = new Set([
    "/api", "/native.js", "/app-resources.json", "/cache-recovery.html", "/cache-recovery.js"
  ]);
  const contentTypes = new Set([
    "application/javascript; charset=utf-8", "application/manifest+json; charset=utf-8",
    "image/png", "text/css; charset=utf-8", "text/html; charset=utf-8"
  ]);
  const timeoutMs = 30000;
  const status = document.getElementById("recovery-status");
  const nextStep = document.getElementById("recovery-next-step");
  const retry = document.getElementById("recovery-retry");
  const returnLink = document.getElementById("recovery-return");
  let busy = false;
  let lastAttempt = 0;
  document.getElementById("recovery-address").textContent = `${location.origin}/`;

  function prepareCookie() {
    if (location.protocol === "https:"
        && /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.shares\.zrok\.io$/i.test(location.hostname)) {
      try {
        document.cookie = "zrok_interstitial=1; Path=/; Secure; SameSite=Strict; Max-Age=604800";
      } catch {
        // cookie 不可用时清单检查会提供可重试错误。
      }
    }
  }

  async function withDeadline(task, description) {
    const controller = new AbortController();
    let timer;
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => {
        reject(new Error(`${description}超过30秒，可稍后重新检查`));
        controller.abort();
      }, timeoutMs);
    });
    try {
      return await Promise.race([task(controller.signal), timeout]);
    } finally {
      clearTimeout(timer);
    }
  }

  async function manifestDigest(files) {
    const parts = [];
    const encoder = new TextEncoder();
    const number = value => {
      const bytes = new Uint8Array(8);
      new DataView(bytes.buffer).setBigUint64(0, BigInt(value));
      parts.push(bytes);
    };
    const field = value => {
      const bytes = encoder.encode(value);
      number(bytes.length);
      parts.push(bytes);
    };
    for (const file of files) {
      field(file.path);
      field(file.contentType);
      number(file.size);
      field(file.sha256);
    }
    const bytes = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
    let offset = 0;
    for (const part of parts) {
      bytes.set(part, offset);
      offset += part.length;
    }
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return [...new Uint8Array(digest)].map(value => value.toString(16).padStart(2, "0")).join("");
  }

  async function readManifest(signal) {
    const response = await fetch("/app-resources.json", {
      cache: "no-store", credentials: "same-origin", redirect: "error", priority: "low", signal
    });
    if (!response.ok || response.redirected || new URL(response.url).origin !== location.origin)
      throw new Error("无法读取同源网页资源清单");
    const manifest = await response.json();
    if (manifest?.schemaVersion !== 1 || manifest.minShellVersion !== 1
        || !hexPattern.test(manifest.version) || !Array.isArray(manifest.files)
        || !manifest.files.length || manifest.files.length > 512)
      throw new Error("网页资源清单不受支持");
    let previous = "";
    let total = 0;
    for (const file of manifest.files) {
      if (!file || typeof file.path !== "string"
          || file.path !== "/" && !pathPattern.test(file.path)
          || excludedPaths.has(file.path) || file.path.startsWith("/api/")
          || previous && file.path <= previous || !contentTypes.has(file.contentType)
          || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > 24 * 1024 * 1024
          || !hexPattern.test(file.sha256))
        throw new Error("网页资源清单包含无效条目");
      const url = new URL(file.path, location.origin);
      if (url.origin !== location.origin || url.pathname !== file.path || url.search || url.hash)
        throw new Error("网页资源路径无效");
      previous = file.path;
      total += file.size;
      if (total > 64 * 1024 * 1024) throw new Error("网页资源清单超过大小限制");
    }
    if (manifest.files[0].path !== "/" || await manifestDigest(manifest.files) !== manifest.version)
      throw new Error("网页资源清单摘要无效");
    return manifest;
  }

  function workerVersion(worker) {
    if (!worker) return null;
    try {
      const url = new URL(worker.scriptURL);
      return url.origin === location.origin && url.pathname === "/sw.js"
        ? url.searchParams.get("resources") : null;
    } catch {
      return null;
    }
  }

  async function waitForVersion(registration, version, signal) {
    const subscriptions = [];
    const watched = new WeakSet();
    try {
      return await new Promise((resolve, reject) => {
        const inspect = event => {
          if (event?.target?.state === "redundant" && workerVersion(event.target) === version) {
            reject(new Error("网页资源安装失败，请稍后重新检查"));
            return;
          }
          if (workerVersion(registration.waiting) === version
              && registration.waiting.state === "installed") {
            resolve("waiting");
            return;
          }
          if (workerVersion(registration.active) === version
              && registration.active.state === "activated") {
            resolve("active");
            return;
          }
          for (const worker of [registration.installing, registration.waiting, registration.active]) {
            if (!worker || watched.has(worker)) continue;
            watched.add(worker);
            worker.addEventListener("statechange", inspect);
            subscriptions.push(() => worker.removeEventListener("statechange", inspect));
          }
          const candidate = registration.installing;
          if (candidate && workerVersion(candidate) === version && candidate.state === "redundant")
            reject(new Error("网页资源安装失败，请稍后重新检查"));
        };
        const abort = () => reject(new Error("网页资源仍在后台准备，可稍后重新检查"));
        signal.addEventListener("abort", abort, { once: true });
        subscriptions.push(() => signal.removeEventListener("abort", abort));
        registration.addEventListener("updatefound", inspect);
        subscriptions.push(() => registration.removeEventListener("updatefound", inspect));
        inspect();
      });
    } finally {
      for (const unsubscribe of subscriptions) unsubscribe();
    }
  }

  async function warmup(worker, version, signal) {
    const channel = new MessageChannel();
    let cancel;
    try {
      return await new Promise((resolve, reject) => {
        cancel = () => reject(new Error("网页资源校验仍未完成，可稍后重新检查"));
        signal.addEventListener("abort", cancel, { once: true });
        channel.port1.onmessage = event => {
          const result = event.data;
          if (result?.type !== "codex-elves-cache-warmup-result" || result.version !== version) return;
          if (result.ok === true) resolve();
          else reject(new Error(result.message || "当前版本网页资源无法补齐"));
        };
        worker.postMessage({ type: "codex-elves-cache-warmup", version }, [channel.port2]);
      });
    } finally {
      if (cancel) signal.removeEventListener("abort", cancel);
      channel.port1.close();
      channel.port2.close();
    }
  }

  async function recover() {
    if (busy) return;
    if (lastAttempt && Date.now() - lastAttempt < 10000) {
      status.textContent = "刚刚已检查，请稍候再试。";
      return;
    }
    busy = true;
    lastAttempt = Date.now();
    retry.disabled = true;
    returnLink.hidden = true;
    status.textContent = "正在检查网页资源…";
    nextStep.textContent = "现有会话页面不会自动刷新，授权和草稿保持原样。";
    try {
      if (!isSecureContext || !("serviceWorker" in navigator))
        throw new Error("网页缓存恢复需要 HTTPS 或本机安全访问环境");
      prepareCookie();
      const manifest = await withDeadline(readManifest, "读取网页资源清单");
      const registration = await withDeadline(() => navigator.serviceWorker.register(
        `/sw.js?resources=${encodeURIComponent(manifest.version)}`,
        { scope: "/", updateViaCache: "none" }
      ), "检查网页缓存服务");
      status.textContent = "正在等待完整网页资源准备好…";
      const state = await withDeadline(signal => waitForVersion(registration, manifest.version, signal),
        "等待网页资源状态");
      if (state === "waiting") {
        status.textContent = "新版网页资源已准备好。";
        nextStep.textContent = "请关闭此网站的所有旧页面（包括本恢复页），再从原地址重新打开；现有页面不会被强制刷新。";
      } else {
        status.textContent = "正在由当前版本缓存服务校验并补齐必需资源…";
        await withDeadline(signal => warmup(registration.active, manifest.version, signal), "校验网页资源");
        status.textContent = "网页缓存已修复，可以重新打开远程会话。";
        nextStep.textContent = "本浏览器的授权、草稿和待处理操作保持原样。";
        returnLink.hidden = false;
      }
    } catch (error) {
      status.textContent = error.message || "网页资源检查失败，可稍后重试。";
      nextStep.textContent = "现有页面和浏览器数据未清除。若资源仍在后台准备，请稍后重新检查。";
    } finally {
      busy = false;
      retry.disabled = false;
    }
  }

  retry.addEventListener("click", () => { void recover(); });
  window.addEventListener("pageshow", prepareCookie);
  void recover();
})();
