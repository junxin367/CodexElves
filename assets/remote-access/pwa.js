import { isAndroidContainer } from "./platform.js";

const shellVersion = 1;
const zrokShareHostnamePattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.shares\.zrok\.io$/i;
const stateEvent = "codex-elves-pwa-state";
const states = new Set([
  "unsupported", "deferred", "checking", "installing", "ready", "waiting", "failed", "recovery-required"
]);
const backgroundRetryInterval = 5 * 60 * 1000;
const manualRetryInterval = 10000;
const manifestTimeoutMs = 30000;
const foregroundQuietMs = 2000;
const activityRenewalMs = 10000;
let registrationPromise = null;
let lastAttempt = 0;
let registration = null;
let initialScheduled = false;
let initialStarted = false;
let fallbackTimer = null;
let backgroundTimer = null;
let activityTimer = null;
let foregroundWorkCount = 0;
let foregroundPreemptCount = 0;
let foregroundQuietUntil = 0;
let appReady = false;
let backgroundManifestController = null;
const watchedRegistrations = new WeakSet();
const watchedWorkers = new WeakSet();
const boundRecoveryButtons = new WeakSet();
let cacheState = Object.freeze({
  status: "deferred",
  version: null,
  message: "网页可用后在后台准备离线资源",
  retryable: false,
  updatedAt: Date.now(),
  path: null
});

export function getPwaCacheState() {
  return cacheState;
}

export function beginPwaForegroundWork({ priority = "foreground" } = {}) {
  const preempt = priority !== "background";
  foregroundWorkCount += 1;
  if (preempt) foregroundPreemptCount += 1;
  if (preempt && backgroundManifestController) {
    const error = new Error("网页资源检查已让出网络");
    error.code = "CODEX_ELVES_BACKGROUND_PAUSED";
    backgroundManifestController.abort(error);
  }
  broadcastResourceActivity();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    foregroundWorkCount = Math.max(0, foregroundWorkCount - 1);
    if (preempt) foregroundPreemptCount = Math.max(0, foregroundPreemptCount - 1);
    if (!foregroundWorkCount) foregroundQuietUntil = Date.now() + foregroundQuietMs;
    broadcastResourceActivity();
    scheduleBackgroundCheck();
  };
}

function renderLoadingRecovery() {
  const loading = document.querySelector("#app > .initial-loading");
  if (!loading) return;
  const status = loading.querySelector("#initial-loading-status");
  const recovery = loading.querySelector("#initial-loading-recovery");
  const retry = loading.querySelector("#initial-cache-retry");
  if (!status || !recovery || !retry) return;
  if (cacheState.status === "unsupported") {
    retry.hidden = true;
    return;
  }
  const offline = navigator.onLine === false;
  if (offline) {
    status.textContent = "网络暂不可用，请联网后检查网页更新。";
  } else if (cacheState.status === "ready") {
    status.textContent = "页面资源已准备好。若仍停留在此处，请关闭此网站所有页面后重新打开。";
  } else if (cacheState.status !== "deferred" && cacheState.status !== "unsupported") {
    status.textContent = cacheState.message;
  } else {
    status.textContent = "正在加载远程会话…";
  }
  if (["failed", "recovery-required", "waiting"].includes(cacheState.status)) recovery.open = true;
  retry.hidden = false;
  retry.disabled = offline || ["checking", "installing", "waiting"].includes(cacheState.status);
  retry.textContent = cacheState.status === "waiting" ? "更新已准备好"
    : cacheState.status === "checking" ? "正在检查更新…"
    : cacheState.status === "installing" ? "正在准备更新…" : "检查并准备更新";
  if (!boundRecoveryButtons.has(retry)) {
    boundRecoveryButtons.add(retry);
    retry.addEventListener("click", () => {
      void retryPwaCache().finally(renderLoadingRecovery);
    });
  }
}

function setCacheState(status, message, { version = cacheState.version, path = null } = {}) {
  if (!states.has(status)) return;
  const retryable = status === "failed" || status === "recovery-required";
  if (cacheState.status === status && cacheState.message === message
      && cacheState.version === version && cacheState.path === path) return;
  cacheState = Object.freeze({ status, version, message, retryable, updatedAt: Date.now(), path });
  renderLoadingRecovery();
  window.dispatchEvent(new CustomEvent(stateEvent, { detail: cacheState }));
}

export function prepareZrokInterstitialCookie() {
  if (isAndroidContainer() || globalThis.location?.protocol !== "https:"
      || !zrokShareHostnamePattern.test(globalThis.location.hostname)
      || !globalThis.document)
    return false;
  try {
    globalThis.document.cookie =
      "zrok_interstitial=1; Path=/; Secure; SameSite=Strict; Max-Age=604800";
    return true;
  } catch {
    return false;
  }
}

function prepareManifestLink() {
  prepareZrokInterstitialCookie();
  const link = document.getElementById("app-manifest");
  if (link && !link.hasAttribute("href")) link.setAttribute("href", "/manifest.webmanifest");
}

function workerVersion(worker) {
  try {
    return new URL(worker.scriptURL).searchParams.get("resources");
  } catch {
    return null;
  }
}

function broadcastResourceActivity() {
  const workers = new Set([
    registration?.installing, registration?.waiting, registration?.active,
    navigator.serviceWorker?.controller
  ].filter(Boolean));
  for (const worker of workers) {
    let url;
    try {
      url = new URL(worker.scriptURL);
    } catch {
      continue;
    }
    const version = url.searchParams.get("resources");
    if (url.origin !== location.origin || url.pathname !== "/sw.js"
        || !/^[a-f0-9]{64}$/.test(version)) continue;
    try {
      worker.postMessage({
        type: "codex-elves-resource-activity",
        version,
        busy: foregroundWorkCount > 0 || navigator.onLine === false,
        preempt: foregroundPreemptCount > 0 || navigator.onLine === false,
        ready: appReady,
        visible: document.visibilityState === "visible"
      });
    } catch {
      // worker 状态切换期间的消息由下一次续租补发。
    }
  }
  if (activityTimer !== null) clearTimeout(activityTimer);
  activityTimer = null;
  if (workers.size) activityTimer = setTimeout(() => {
    activityTimer = null;
    broadcastResourceActivity();
  }, activityRenewalMs);
}

function showRegistrationState() {
  if (registration?.waiting) {
    setCacheState("waiting", "新版网页资源已就绪，关闭现有页面后重新打开即可启用",
      { version: workerVersion(registration.waiting) });
  } else if (registration?.installing) {
    setCacheState("installing", "正在后台准备离线网页资源",
      { version: workerVersion(registration.installing) });
  } else if (registration?.active) {
    setCacheState("ready", "",
      { version: workerVersion(registration.active) });
  }
  broadcastResourceActivity();
}

function watchWorker(worker) {
  if (!worker || watchedWorkers.has(worker)) return;
  watchedWorkers.add(worker);
  const candidate = worker === registration?.installing || worker.state === "installing";
  worker.addEventListener("statechange", () => {
    if (worker.state === "redundant") {
      if (registration?.waiting || registration?.installing && registration.installing !== worker
          || registration?.active && registration.active !== worker) {
        showRegistrationState();
        return;
      }
      // 详细失败通知通常先由 worker 发出，避免以笼统信息覆盖它。
      if (candidate && cacheState.status !== "failed" && cacheState.status !== "recovery-required")
        setCacheState("failed", "离线网页资源准备中断，可稍后重试",
          { version: workerVersion(worker) });
    } else if (worker.state === "installed" && registration?.active
        && registration.active !== worker) {
      setCacheState("waiting", "新版网页资源已就绪，关闭现有页面后重新打开即可启用",
        { version: workerVersion(worker) });
    } else {
      showRegistrationState();
    }
  });
}

async function readResourceManifest(manual = false) {
  const controller = new AbortController();
  if (!manual) backgroundManifestController = controller;
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      controller.abort();
      reject(new Error("读取网页资源清单超时，请稍后重试"));
    }, manifestTimeoutMs);
  });
  try {
    return await Promise.race([(async () => {
      const response = await fetch("/app-resources.json", {
        cache: "no-store",
        credentials: "same-origin",
        redirect: "error",
        priority: manual ? "high" : "low",
        signal: controller.signal
      });
      if (!response.ok || response.redirected
          || response.url && new URL(response.url).origin !== location.origin)
        throw new Error(`HTTP ${response.status}`);
      return response.json();
    })(), timeout]);
  } catch (error) {
    if (controller.signal.reason?.code === "CODEX_ELVES_BACKGROUND_PAUSED")
      throw controller.signal.reason;
    throw error;
  } finally {
    clearTimeout(timer);
    if (backgroundManifestController === controller) backgroundManifestController = null;
  }
}

async function registerServiceWorker(manual = false) {
  if (isAndroidContainer() || !globalThis.isSecureContext || !("serviceWorker" in navigator)) {
    setCacheState("unsupported", "当前环境不使用浏览器离线网页缓存");
    return null;
  }
  if (registrationPromise) return registrationPromise;
  if (!manual && (foregroundWorkCount || document.visibilityState !== "visible")) return registration;
  if (!manual && Date.now() < foregroundQuietUntil) {
    scheduleBackgroundCheck();
    return registration;
  }
  const interval = manual ? manualRetryInterval : backgroundRetryInterval;
  if (lastAttempt && Date.now() - lastAttempt < interval) return registration;
  // 无网络时保持已有失败/就绪状态，避免 online 与前台事件造成反复提示。
  if (navigator.onLine === false) return registration;
  lastAttempt = Date.now();
  initialStarted = true;
  registrationPromise = (async () => {
    if (manual || !["ready", "waiting", "failed", "recovery-required"].includes(cacheState.status))
      setCacheState("checking", "正在检查离线网页资源");
    const manifest = await readResourceManifest(manual);
    if (manifest?.schemaVersion !== 1 || !Number.isInteger(manifest.minShellVersion)
        || manifest.minShellVersion < 1 || manifest.minShellVersion > shellVersion
        || !/^[a-f0-9]{64}$/.test(manifest.version)
        || !Array.isArray(manifest.files))
      throw new Error("网页资源清单不受支持");
    registration = await navigator.serviceWorker.register(
      `/sw.js?resources=${encodeURIComponent(manifest.version)}`,
      { scope: "/", updateViaCache: "none" }
    );
    if (!watchedRegistrations.has(registration)) {
      watchedRegistrations.add(registration);
      registration.addEventListener("updatefound", () => {
        watchWorker(registration.installing);
        showRegistrationState();
      });
    }
    watchWorker(registration.installing);
    watchWorker(registration.waiting);
    watchWorker(registration.active);
    showRegistrationState();
    return registration;
  })().catch(error => {
    if (error?.code === "CODEX_ELVES_BACKGROUND_PAUSED") {
      lastAttempt = 0;
      if (cacheState.status === "checking")
        setCacheState("deferred", "会话可用后在后台继续准备离线资源");
      return registration;
    }
    setCacheState("failed", `离线网页资源检查失败：${error.message}`);
    return null;
  }).finally(() => { registrationPromise = null; });
  return registrationPromise;
}

export function retryPwaCache() {
  prepareManifestLink();
  return registerServiceWorker(true);
}

function startInitialRegistration() {
  if (initialStarted) return;
  initialStarted = true;
  if (fallbackTimer !== null) clearTimeout(fallbackTimer);
  fallbackTimer = null;
  void registerServiceWorker();
}

function scheduleInitialRegistration() {
  if (initialScheduled || initialStarted) return;
  initialScheduled = true;
  if ("requestIdleCallback" in window) {
    window.requestIdleCallback(startInitialRegistration, { timeout: 5000 });
  } else {
    setTimeout(startInitialRegistration, 1000);
  }
}

function scheduleBackgroundCheck() {
  if (!initialStarted || backgroundTimer !== null || navigator.onLine === false
      || foregroundWorkCount || document.visibilityState !== "visible") return;
  const remaining = Math.max(1000, foregroundQuietUntil - Date.now(),
    lastAttempt ? lastAttempt + backgroundRetryInterval - Date.now() : 0);
  backgroundTimer = setTimeout(() => {
    backgroundTimer = null;
    if (document.visibilityState === "visible" && navigator.onLine !== false && !foregroundWorkCount)
      void registerServiceWorker();
  }, remaining);
}

prepareManifestLink();
renderLoadingRecovery();
if (isAndroidContainer() || !globalThis.isSecureContext || !("serviceWorker" in navigator)) {
  setCacheState("unsupported", "当前环境不使用浏览器离线网页缓存");
} else {
  window.addEventListener("codex-elves-app-ready", () => {
    appReady = true;
    broadcastResourceActivity();
    scheduleInitialRegistration();
  }, { once: true });
  const startFallback = () => {
    // load 只证明静态页面加载完成；通常由 app-ready 在首轮会话加载后提前调度。
    fallbackTimer = setTimeout(startInitialRegistration, 15000);
  };
  if (document.readyState === "complete") startFallback();
  else window.addEventListener("load", startFallback, { once: true });
  navigator.serviceWorker.addEventListener("message", event => {
    const worker = event.source;
    let source;
    try {
      source = new URL(worker?.scriptURL);
    } catch {
      return;
    }
    if (source.origin !== location.origin || source.pathname !== "/sw.js"
        || event.data?.type !== stateEvent) return;
    const state = event.data.state;
    if (!states.has(state?.status) || !/^[a-f0-9]{64}$/.test(state.version)) return;
    // 旧活动 worker 的恢复失败与新版 waiting 可以同时存在；恢复失败优先可见。
    if (state.status === "ready" && registration?.waiting) {
      showRegistrationState();
      return;
    }
    setCacheState(state.status, state.message, { version: state.version, path: state.path || null });
    if (state.status === "ready") broadcastResourceActivity();
  });
}
window.addEventListener("pageshow", () => {
  prepareManifestLink();
  broadcastResourceActivity();
});
window.addEventListener("visibilitychange", () => {
  broadcastResourceActivity();
  if (document.visibilityState !== "visible") return;
  prepareManifestLink();
  scheduleBackgroundCheck();
});
window.addEventListener("online", () => {
  foregroundQuietUntil = Date.now() + foregroundQuietMs;
  prepareManifestLink();
  broadcastResourceActivity();
  renderLoadingRecovery();
  scheduleBackgroundCheck();
});
window.addEventListener("offline", () => {
  renderLoadingRecovery();
  broadcastResourceActivity();
});
