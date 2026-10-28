const messageTypes = new Set(["ready", "authenticated", "auth-required", "logged-out", "app-info", "check-app-update"]);
const appInfoTimeoutMs = 3000;
let androidAppInfo = null;
let androidAppInfoPending = null;
let browserOpenPending = null;
let browserRequestSequence = 0;
const notificationRequestTypes = new Set([
  "notification-settings-get",
  "notification-settings-set",
  "notification-permission-request",
  "notification-open-system-settings"
]);
const notificationSettingsKeys = ["enabled", "approval", "question", "completed", "failed"];
const notificationPermissions = new Set(["prompt", "granted", "denied"]);
const notificationDeliveries = new Set([
  "disabled", "permission-required", "connecting", "connected", "disconnected", "unsupported", "auth-required"
]);
const notificationRequestTimeouts = {
  "notification-settings-get": 5000,
  "notification-settings-set": 15000,
  "notification-permission-request": 60000,
  "notification-open-system-settings": 15000
};
const notificationRequests = new Map();
const notificationSettingsListeners = new Set();
let notificationBridge = null;
let notificationBridgeListenerInstalled = false;
let notificationRequestSequence = 0;
const androidHapticTypes = Object.freeze({
  click: "haptic-click",
  "drag-start": "haptic-drag-start",
  "drag-tick": "haptic-drag-tick",
  "drag-end": "haptic-drag-end"
});
const androidClickableSelector = [
  "button",
  "a[href]",
  "area[href]",
  "input[type='button']",
  "input[type='submit']",
  "input[type='reset']",
  "input[type='image']",
  "input[type='file']",
  "input[type='checkbox']",
  "input[type='radio']",
  "select",
  "summary",
  "[role~='button']",
  "[role~='link']",
  "[role~='checkbox']",
  "[role~='radio']",
  "[role~='switch']",
  "[role~='menuitem']",
  "[role~='menuitemcheckbox']",
  "[role~='menuitemradio']",
  "[role~='tab']",
  "[role~='option']",
  "[role~='treeitem']"
].join(",");
let lastAndroidHapticAt = -Infinity;
let lastAndroidDragTickAt = -Infinity;
let androidClickHapticFeedbackInstalled = false;

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && Object.getPrototypeOf(value) === Object.prototype;
}

function hasExactKeys(value, expected) {
  if (!isPlainObject(value)) return false;
  const keys = Object.keys(value);
  return keys.length === expected.length && expected.every(key => Object.hasOwn(value, key));
}

function isValidNotificationSettings(value) {
  return hasExactKeys(value, notificationSettingsKeys)
    && notificationSettingsKeys.every(key => typeof value[key] === "boolean");
}

function notificationStatus(data, requestId = null) {
  const expected = ["type", "supported", "settings", "permission", "delivery", "message"];
  if (requestId !== null) expected.push("requestId");
  const allowed = new Set(expected);
  if (!isPlainObject(data) || Object.keys(data).some(key => !allowed.has(key))
      || expected.filter(key => key !== "message").some(key => !Object.hasOwn(data, key))
      || data.type !== "notification-settings"
      || requestId !== null && data.requestId !== requestId
      || typeof data.supported !== "boolean"
      || !isValidNotificationSettings(data.settings)
      || !notificationPermissions.has(data.permission)
      || !notificationDeliveries.has(data.delivery)
      || Object.hasOwn(data, "message") && (typeof data.message !== "string" || data.message.length > 200
        || /[\u0000-\u001f\u007f]/.test(data.message))) return null;
  return Object.freeze({
    supported: data.supported,
    settings: Object.freeze({ ...data.settings }),
    permission: data.permission,
    delivery: data.delivery,
    ...(Object.hasOwn(data, "message") ? { message: data.message } : {})
  });
}

function notificationBridgeError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

function notificationRequestId() {
  let requestId;
  do {
    notificationRequestSequence = (notificationRequestSequence + 1) % Number.MAX_SAFE_INTEGER;
    requestId = `notification-${Date.now().toString(36)}-${notificationRequestSequence.toString(36)}`;
  } while (notificationRequests.has(requestId));
  return requestId;
}

function stopNotificationBridgeListenerIfIdle() {
  if (notificationRequests.size || notificationSettingsListeners.size || !notificationBridgeListenerInstalled) return;
  try { notificationBridge.removeEventListener("message", onNotificationBridgeMessage); } catch {}
  notificationBridge = null;
  notificationBridgeListenerInstalled = false;
}

function finishNotificationRequest(requestId, result, error = null) {
  const pending = notificationRequests.get(requestId);
  if (!pending) return;
  notificationRequests.delete(requestId);
  clearTimeout(pending.timeout);
  stopNotificationBridgeListenerIfIdle();
  if (error) pending.reject(error);
  else pending.resolve(result);
}

function onNotificationBridgeMessage(event) {
  if (typeof event?.data !== "string" || event.data.length > 4096) return;
  let data;
  try { data = JSON.parse(event.data); } catch { return; }
  if (!isPlainObject(data)) return;

  if (data.type === "notification-settings-changed") {
    const status = notificationStatus({ ...data, type: "notification-settings" });
    if (!status || Object.hasOwn(data, "requestId")) return;
    for (const listener of notificationSettingsListeners) {
      try { listener(status); } catch {}
    }
    return;
  }

  if (data.type !== "notification-settings" || typeof data.requestId !== "string"
      || !/^[A-Za-z0-9_-]{1,80}$/.test(data.requestId)) return;
  const pending = notificationRequests.get(data.requestId);
  if (!pending) return;

  if (Object.hasOwn(data, "error")) {
    if (!hasExactKeys(data, ["type", "requestId", "error"])
        || typeof data.error !== "string" || !/^[A-Za-z0-9_.-]{1,128}$/.test(data.error)) return;
    finishNotificationRequest(data.requestId, null,
      notificationBridgeError(data.error, "Android 通知设置操作失败，请重试。"));
    return;
  }
  const status = notificationStatus(data, data.requestId);
  if (status) finishNotificationRequest(data.requestId, status);
}

function ensureNotificationBridgeListener() {
  if (!isAndroidContainer()) return false;
  const bridge = globalThis.CodexElvesAndroid;
  if (typeof bridge.addEventListener !== "function" || typeof bridge.removeEventListener !== "function") return false;
  if (notificationBridgeListenerInstalled && notificationBridge === bridge) return true;
  if (notificationBridgeListenerInstalled) {
    try { notificationBridge.removeEventListener("message", onNotificationBridgeMessage); } catch {}
  }
  notificationBridge = bridge;
  try {
    bridge.addEventListener("message", onNotificationBridgeMessage);
    notificationBridgeListenerInstalled = true;
    return true;
  } catch {
    notificationBridge = null;
    notificationBridgeListenerInstalled = false;
    return false;
  }
}

function requestAndroidNotificationBridge(type, fields = {}) {
  if (!isAndroidContainer()) return Promise.resolve(null);
  if (!notificationRequestTypes.has(type))
    return Promise.reject(notificationBridgeError("invalid-request", "不支持此通知设置操作。"));
  if (!ensureNotificationBridgeListener())
    return Promise.reject(notificationBridgeError("unsupported", "当前 Android App 版本不支持通知设置，请更新 App。"));

  const requestId = notificationRequestId();
  const payload = { type, requestId, ...fields };
  const serialized = JSON.stringify(payload);
  if (serialized.length > 4096)
    return Promise.reject(notificationBridgeError("invalid-request", "通知设置请求内容无效。"));

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => finishNotificationRequest(requestId, null,
      notificationBridgeError("timeout", "当前 Android App 未响应通知设置，请更新 App 后重试。")),
    notificationRequestTimeouts[type]);
    notificationRequests.set(requestId, { resolve, reject, timeout });
    try {
      const result = globalThis.CodexElvesAndroid.postMessage(serialized);
      if (result === false) finishNotificationRequest(requestId, null,
        notificationBridgeError("unsupported", "当前 Android App 版本不支持通知设置，请更新 App。"));
    } catch {
      finishNotificationRequest(requestId, null,
        notificationBridgeError("bridge-failed", "无法连接 Android 通知设置，请重试。"));
    }
  });
}

export function isAndroidContainer() {
  return typeof globalThis.CodexElvesAndroid?.postMessage === "function";
}

export function performAndroidHapticFeedback(kind) {
  if (typeof kind !== "string" || !Object.hasOwn(androidHapticTypes, kind)) return false;
  const type = androidHapticTypes[kind];
  try {
    const bridge = globalThis.CodexElvesAndroid;
    if (typeof bridge?.postMessage !== "function" || typeof document === "undefined"
        || document.visibilityState !== "visible") return false;
    const now = globalThis.performance.now();
    if (!Number.isFinite(now) || now - lastAndroidHapticAt < 50
        || kind === "drag-tick" && now - lastAndroidDragTickAt < 80) return false;
    bridge.postMessage(JSON.stringify({ type }));
    lastAndroidHapticAt = now;
    if (kind === "drag-tick") lastAndroidDragTickAt = now;
    return true;
  } catch {
    return false;
  }
}

function onAndroidClickableClick(event) {
  try {
    if (event.isTrusted !== true || event.button !== 0 && event.detail !== 0) return;
    const target = event.target?.nodeType === 1 ? event.target : event.target?.parentElement;
    if (!target || typeof target.closest !== "function" || target.closest(".queued-drag-handle")) return;
    const control = target.closest(androidClickableSelector);
    if (!control || control.disabled === true || control.hasAttribute("disabled")
        || control.matches(":disabled")) return;
    for (let ancestor = control; ancestor; ancestor = ancestor.parentElement) {
      if (ancestor.getAttribute("aria-disabled") === "true"
          || ancestor.inert === true || ancestor.hasAttribute("inert")) return;
    }
    performAndroidHapticFeedback("click");
  } catch {}
}

export function installAndroidClickHapticFeedback() {
  if (androidClickHapticFeedbackInstalled || typeof document === "undefined"
      || typeof document.addEventListener !== "function") return false;
  try {
    if (!isAndroidContainer()) return false;
    document.addEventListener("click", onAndroidClickableClick, true);
    androidClickHapticFeedbackInstalled = true;
    return true;
  } catch {
    return false;
  }
}

export function postPlatformMessage(type) {
  if (!messageTypes.has(type) || !isAndroidContainer()) return false;
  try {
    globalThis.CodexElvesAndroid.postMessage(JSON.stringify({ type }));
    return true;
  } catch {
    return false;
  }
}

export function getAndroidAppInfo() {
  if (!isAndroidContainer()) return Promise.resolve(null);
  if (androidAppInfo) return Promise.resolve(androidAppInfo);
  if (androidAppInfoPending) return androidAppInfoPending;

  const bridge = globalThis.CodexElvesAndroid;
  if (typeof bridge.addEventListener !== "function" || typeof bridge.removeEventListener !== "function") {
    return Promise.resolve(null);
  }

  let resolvePending;
  let settled = false;
  let timeout;
  const pending = new Promise(resolve => { resolvePending = resolve; });
  androidAppInfoPending = pending;
  const finish = info => {
    if (settled) return;
    settled = true;
    clearTimeout(timeout);
    try { bridge.removeEventListener("message", onMessage); } catch {}
    if (androidAppInfoPending === pending) androidAppInfoPending = null;
    if (info) androidAppInfo = info;
    resolvePending(info);
  };
  const onMessage = event => {
    let data;
    try { data = JSON.parse(event?.data); } catch { return; }
    if (!data || typeof data !== "object" || data.type !== "app-info") return;
    const versionName = typeof data.versionName === "string" ? data.versionName.trim() : "";
    if (!versionName || versionName.length > 64 || /[\u0000-\u001f\u007f]/.test(versionName)
        || !Number.isSafeInteger(data.versionCode) || data.versionCode < 1
        || typeof data.updateSupported !== "boolean") {
      finish(null);
      return;
    }
    finish({ versionName, versionCode: data.versionCode, updateSupported: data.updateSupported,
      requestRoutingVersion: data.requestRoutingVersion === 2 ? 2 : 1,
      diagnosticsVersion: data.diagnosticsVersion === 1 ? 1 : 0,
      browserVersion: data.browserVersion === 1 ? 1 : 0 });
  };

  try {
    bridge.addEventListener("message", onMessage);
  } catch {
    finish(null);
    return pending;
  }
  timeout = setTimeout(() => finish(null), appInfoTimeoutMs);
  if (!postPlatformMessage("app-info")) finish(null);
  return pending;
}

export function getAndroidNotificationSettings() {
  return requestAndroidNotificationBridge("notification-settings-get");
}

function browserAppearance() {
  const defaults = {
    background: "#ffffff", foreground: "#1f1f1f", muted: "#717171", line: "#e8e8e8", surface: "#f9f9f9"
  };
  const style = typeof document !== "undefined" && typeof getComputedStyle === "function"
    ? getComputedStyle(document.body || document.documentElement) : null;
  const colors = {};
  for (const [name, property] of Object.entries({
    background: "--bg", foreground: "--text", muted: "--muted", line: "--line", surface: "--side"
  })) {
    let value = style?.getPropertyValue(property)?.trim() || defaults[name];
    if (/^#[0-9a-f]{3}$/i.test(value)) value = "#" + [...value.slice(1)].map(x => x + x).join("");
    colors[name] = /^#[0-9a-f]{6}$/i.test(value) ? value : defaults[name];
  }
  return { colors, fontSize: Math.max(12, Math.min(24, parseFloat(style?.fontSize) || 14)) };
}

export function openAndroidBrowser(initialUrl = "") {
  if (!isAndroidContainer()) return Promise.reject(new Error("请在 Android App 中使用浏览器。"));
  if (browserOpenPending) return browserOpenPending;
  const pending = (async () => {
    const info = await getAndroidAppInfo();
    if (info?.browserVersion !== 1) throw new Error("当前 App 不支持浏览器，请更新 Android App 后重试。");
    let targetUrl = "";
    if (initialUrl) {
      try {
        const parsed = new URL(initialUrl, location.href);
        if (parsed.origin !== location.origin || !parsed.pathname.startsWith("/api/workspace/view/")
            || parsed.pathname.split("/").some(part => part === "." || part === "..")
            || parsed.search || parsed.username || parsed.password) {
          throw new Error("本地文件地址无效");
        }
        targetUrl = parsed.href;
      } catch (error) {
        throw error instanceof Error ? error : new Error("本地文件地址无效");
      }
    }
    const bridge = globalThis.CodexElvesAndroid;
    if (typeof bridge?.addEventListener !== "function" || typeof bridge?.removeEventListener !== "function")
      throw new Error("无法连接 Android 浏览器，请重新打开 App。");
    const requestId = `browser-${Date.now().toString(36)}-${++browserRequestSequence}`;
    return new Promise((resolve, reject) => {
      let settled = false;
      const finish = error => {
        if (settled) return;
        settled = true; clearTimeout(timer);
        try { bridge.removeEventListener("message", receive); } catch {}
        error ? reject(error) : resolve();
      };
      const receive = event => {
        if (typeof event?.data !== "string" || event.data.length > 2048) return;
        let value;
        try { value = JSON.parse(event.data); } catch { return; }
        if (!isPlainObject(value) || value.type !== "browser-open-result"
          || value.requestId !== requestId || typeof value.ok !== "boolean") return;
        finish(value.ok ? null : new Error(
          typeof value.message === "string" && value.message.length <= 256
            ? value.message : "无法打开浏览器，请重试。"));
      };
      const timer = setTimeout(() => finish(new Error("Android 浏览器未响应，请更新 App 后重试。")), 10000);
      try {
        bridge.addEventListener("message", receive);
        const message = { type: "browser-open", requestId, ...browserAppearance(),
          ...(targetUrl ? { initialUrl: targetUrl } : {}) };
        if (bridge.postMessage(JSON.stringify(message)) === false)
          finish(new Error("当前 Android App 无法打开浏览器，请更新后重试。"));
      } catch { finish(new Error("无法打开 Android 浏览器，请重试。")); }
    });
  })().finally(() => { if (browserOpenPending === pending) browserOpenPending = null; });
  browserOpenPending = pending;
  return pending;
}

export function setAndroidNotificationSettings(settings) {
  if (!isAndroidContainer()) return Promise.resolve(null);
  if (!isValidNotificationSettings(settings))
    return Promise.reject(notificationBridgeError("invalid-settings", "通知设置格式无效。"));
  return requestAndroidNotificationBridge("notification-settings-set", {
    settings: Object.fromEntries(notificationSettingsKeys.map(key => [key, key === "failed" ? false : settings[key]]))
  });
}

export function requestAndroidNotificationPermission() {
  return requestAndroidNotificationBridge("notification-permission-request");
}

export function openAndroidNotificationSystemSettings() {
  return requestAndroidNotificationBridge("notification-open-system-settings");
}

export function subscribeAndroidNotificationSettings(listener) {
  if (typeof listener !== "function") throw new TypeError("listener 必须是函数。");
  if (!isAndroidContainer() || !ensureNotificationBridgeListener()) return () => {};
  notificationSettingsListeners.add(listener);
  let subscribed = true;
  return () => {
    if (!subscribed) return;
    subscribed = false;
    notificationSettingsListeners.delete(listener);
    stopNotificationBridgeListenerIfIdle();
  };
}
