import { getAndroidAppInfo, isAndroidContainer } from "./platform.js";

const pending = new Map();
const earlyEvents = [];
const handledRequests = new Set();
const requestPattern = /^[A-Za-z0-9_-]{1,80}$/;
const tokenPattern = /^[A-Za-z][A-Za-z0-9_.-]{0,63}$/;
const numericFields = new Set(["status", "durationMs", "bytes", "sdk", "generation", "readyState",
  "expectedBytes", "loadedBytes", "line", "width", "height", "lux", "stableMs"]);
const tokenFields = new Set(["method", "errorType", "reason", "routeName", "phase", "imageKind", "network", "theme", "decision"]);
const errorTypes = new Set(["Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError",
  "URIError", "EvalError", "DOMException", "AbortError", "TimeoutError", "NetworkError", "SecurityError"]);
const fieldTokens = {
  method: new Set(["GET", "POST", "HEAD", "OPTIONS", "PUT", "PATCH", "DELETE"]),
  reason: new Set(["success", "cancelled", "timeout", "response-read-failed", "http-error", "network-error",
    "manual", "visibility", "online", "offline", "pagehide", "upload-failed", "unsupported", "native-failed",
    "sensor", "timer", "startup"]),
  phase: new Set(["request", "body", "complete", "ready", "failure", "pending"]),
  imageKind: new Set(["preview", "thumbnail", "image"]),
  routeName: new Set(["zrok", "ngrok"]),
  network: new Set(["wifi", "cellular", "ethernet", "vpn", "other", "none", "unknown"]),
  theme: new Set(["light", "dark"]),
  decision: new Set(["pending", "cancelled", "applied"])
};
const knownMimes = new Set(["application/json", "application/javascript", "application/octet-stream",
  "application/manifest+json", "text/javascript", "text/html", "text/css", "text/plain", "text/event-stream",
  "image/jpeg", "image/png", "image/webp", "image/gif", "image/avif", "image/svg+xml",
  "video/mp4", "video/webm", "video/ogg", "audio/mpeg", "audio/mp4", "audio/webm"]);
const staticPaths = new Set(["/", "/app-resources.json", "/app.js", "/channels.js", "/diagnostics.js",
  "/attachments.js", "/platform.js", "/pwa.js", "/sw.js", "/events-client.js", "/events-worker.js",
  "/session-tools.js", "/session-menu.js", "/markdown.js", "/markdown-renderer.js", "/recovery-store.js",
  "/message-actions.js", "/style.css", "/manifest.webmanifest", "/vendor/marked.esm.js",
  "/vendor/dompurify.esm.js", "/vendor/mermaid.esm.js"]);
let supported = null;
let sequence = 0;
let listening = false;
let getSession = () => null;
let upload = null;
let uploading = null;
const support = isAndroidContainer() ? getAndroidAppInfo().then(info => {
  supported = info?.diagnosticsVersion === 1;
  if (supported) for (const event of earlyEvents.splice(0)) postEvent(event);
  else earlyEvents.length = 0;
  return supported;
}).catch(() => { supported = false; earlyEvents.length = 0; return false; }) : Promise.resolve(false);

// 不记录查询参数、片段、请求正文或任意本地路径。
export function diagnosticLocation(value) {
  try {
    const url = new URL(value, globalThis.location?.origin);
    if (!["http:", "https:", "ws:", "wss:"].includes(url.protocol)) return {};
    let path = url.pathname;
    if (!staticPaths.has(path)
        && !/^\/api\/(?:session|channels|channel-events|events(?:\/ws)?|diagnostics|login-status|connect|logout)$/.test(path)
        && !/^\/api\/(?:read|action)\/[A-Za-z]{1,64}$/.test(path)
        && !/^\/api\/images\/[A-Za-z0-9_-]{20,128}(?:\/thumbnail)?$/.test(path)
        && !/^\/api\/videos\/[a-f0-9]{64}(?:\/poster)?$/.test(path)) {
      path = path.startsWith("/api/receipts/") ? "/api/receipts" : "/other";
    }
    return { host: url.hostname.slice(0, 160), path };
  } catch { return {}; }
}

function safeFields(value) {
  const result = {};
  for (const [key, field] of Object.entries(value || {})) {
    if (key === "errorCode" && Number.isSafeInteger(field) && field >= -1000 && field <= 1000) result[key] = field;
    else if (numericFields.has(key) && Number.isFinite(field) && field >= 0)
      result[key] = Math.min(Number.MAX_SAFE_INTEGER, Math.round(field));
    else if (["visible", "online", "supported"].includes(key) && typeof field === "boolean") result[key] = field;
    else if (key === "errorType" && typeof field === "string") result[key] = errorTypes.has(field) ? field : "unknown";
    else if (tokenFields.has(key) && typeof field === "string" && fieldTokens[key]?.has(field)) result[key] = field;
    else if (key === "mime" && typeof field === "string" && /^[a-zA-Z0-9.+-]+\/[a-zA-Z0-9.+-]+$/.test(field))
      result[key] = knownMimes.has(field) ? field : "other";
    else if (key === "host" && typeof field === "string" && /^[a-zA-Z0-9.[\]:-]{1,160}$/.test(field)) result[key] = field;
    else if (key === "path" && typeof field === "string") result[key] = diagnosticLocation(field).path || "/other";
    else if (["version", "webViewVersion"].includes(key) && typeof field === "string"
        && /^[a-zA-Z0-9._-]{1,80}$/.test(field)) result[key] = field;
    else if (key === "resourceVersion" && typeof field === "string" && /^[a-f0-9]{64}$/.test(field)) result[key] = field;
    else if (key === "requestId" && typeof field === "string" && requestPattern.test(field)) result[key] = field;
  }
  return result;
}

function postEvent(event) {
  try {
    const message = JSON.stringify({ type: "diagnostics-log", event });
    if (message.length <= 4096) globalThis.CodexElvesAndroid.postMessage(message);
  } catch { /* 诊断失败不能改变业务请求的结果。 */ }
}

export function recordDiagnostic(name, fields = {}) {
  try {
    if (!isAndroidContainer() || supported === false || !tokenPattern.test(name)) return;
    const event = { name, fields: safeFields(fields) };
    if (supported) postEvent(event);
    else if (earlyEvents.length < 32) earlyEvents.push(event);
  } catch {}
}

export function traceDiagnosticRequest(path, options, origin) {
  const started = globalThis.performance?.now?.() ?? Date.now();
  const fields = { ...diagnosticLocation(new URL(path, origin).href), method: options.method || "GET" };
  let status;
  let phase = "request";
  recordDiagnostic("http.start", fields);
  return {
    response(response) {
      status = response.status;
      phase = "body";
      recordDiagnostic("http.headers", {
        ...fields, status, phase,
        durationMs: (globalThis.performance?.now?.() ?? Date.now()) - started,
        mime: response.headers.get("content-type")?.split(";")[0]?.trim()
      });
    },
    finish(error = null, reason = null) {
      const signal = options.signal;
      recordDiagnostic(error ? "http.failed" : "http.complete", {
        ...fields, status, phase: error ? phase : "complete",
        durationMs: (globalThis.performance?.now?.() ?? Date.now()) - started,
        errorType: error?.name,
        reason: reason || (!error ? "success" : signal?.aborted
          ? signal.reason?.name === "TimeoutError" ? "timeout" : "cancelled"
          : error.responseReadFailed ? "response-read-failed" : Number.isFinite(error.code) ? "http-error" : "network-error")
      });
    }
  };
}

function finishBridgeRequest(requestId, value, error) {
  const request = pending.get(requestId);
  if (!request) return;
  pending.delete(requestId);
  clearTimeout(request.timer);
  if (error) request.reject(error);
  else request.resolve(value);
}

function ensureBridgeListener() {
  if (listening) return true;
  const bridge = globalThis.CodexElvesAndroid;
  if (typeof bridge?.addEventListener !== "function") return false;
  bridge.addEventListener("message", event => {
    if (typeof event.data !== "string" || event.data.length > 384 * 1024) return;
    let data;
    try { data = JSON.parse(event.data); } catch { return; }
    const request = pending.get(data?.requestId);
    if (!request || data.type !== request.type) return;
    if (data.error || data.ok === false) {
      finishBridgeRequest(data.requestId, null, Object.assign(new Error("诊断日志操作失败，请重试。"), { code: "native-failed" }));
    } else if (data.type === "diagnostics-export" && data.ok === true) {
      finishBridgeRequest(data.requestId, true);
    } else if (data.type === "diagnostics-snapshot" && data.bundle?.schemaVersion === 1
        && Array.isArray(data.bundle.events) && data.bundle.events.length <= 400) {
      finishBridgeRequest(data.requestId, data.bundle);
    }
  });
  listening = true;
  return true;
}

async function nativeRequest(type) {
  if (!await support || !ensureBridgeListener())
    throw Object.assign(new Error("当前 App 不支持诊断日志，请安装新版 APK。"), { code: "unsupported" });
  if (pending.size >= 2) throw new Error("诊断操作正在进行，请稍候。");
  const requestId = `diagnostics-${Date.now()}-${++sequence}`;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => finishBridgeRequest(requestId, null,
      Object.assign(new Error("读取诊断日志超时，请重试。"), { code: "timeout" })), 15000);
    pending.set(requestId, { type, resolve, reject, timer });
    try {
      globalThis.CodexElvesAndroid.postMessage(JSON.stringify({ type, requestId }));
    } catch {
      finishBridgeRequest(requestId, null, new Error("无法连接 App 诊断服务。"));
    }
  });
}

export async function exportAndroidDiagnostics() {
  recordDiagnostic("diagnostics.export", { reason: "manual" });
  return nativeRequest("diagnostics-export");
}

export async function sendAndroidDiagnostics(requestId = null) {
  if (uploading) return uploading;
  const device = getSession()?.device?.id;
  const origin = globalThis.location.origin;
  if (!device || typeof upload !== "function") throw new Error("连接恢复后才能发送日志，当前可以先导出。");
  const id = requestId || `manual-${Date.now()}-${++sequence}`;
  if (!requestPattern.test(id)) throw new Error("诊断请求无效。");
  const operation = (async () => {
    recordDiagnostic("diagnostics.snapshot", { requestId: id });
    const bundle = await nativeRequest("diagnostics-snapshot");
    if (getSession()?.device?.id !== device || globalThis.location.origin !== origin)
      throw new Error("连接身份已变化，请重新发送日志。");
    const result = await upload(id, bundle);
    if (result?.ok !== true || result.requestId !== id) throw new Error("电脑端未确认收到日志，请稍后核对。");
    recordDiagnostic("diagnostics.uploaded", { requestId: id });
    return result;
  })();
  uploading = operation;
  try { return await operation; }
  finally { if (uploading === operation) uploading = null; }
}

export function receiveDiagnosticRequest(params) {
  if (!params || params.deviceId !== getSession()?.device?.id
      || !requestPattern.test(params.requestId || "") || !Number.isSafeInteger(params.expiresAtMs)
      || params.expiresAtMs <= Date.now() || params.expiresAtMs > Date.now() + 5 * 60000
      || handledRequests.has(params.requestId)) return;
  handledRequests.add(params.requestId);
  if (handledRequests.size > 32) handledRequests.delete(handledRequests.values().next().value);
  void (async () => {
    if (uploading) { try { await uploading; } catch {} }
    if (params.deviceId !== getSession()?.device?.id || params.expiresAtMs <= Date.now()) return;
    await sendAndroidDiagnostics(params.requestId);
  })().catch(error => {
    handledRequests.delete(params.requestId);
    recordDiagnostic("diagnostics.failed", { errorType: error.name, reason: error.code || "upload-failed" });
  });
}

export function configureAndroidDiagnostics(options) {
  getSession = options.getSession;
  upload = options.upload;
  if (!isAndroidContainer()) return;
  recordDiagnostic("page.started", { visible: !document.hidden, online: navigator.onLine !== false });
  const lifecycle = reason => recordDiagnostic("page.lifecycle", {
    reason, visible: !document.hidden, online: navigator.onLine !== false
  });
  document.addEventListener("visibilitychange", () => lifecycle("visibility"));
  globalThis.addEventListener("online", () => lifecycle("online"));
  globalThis.addEventListener("offline", () => lifecycle("offline"));
  globalThis.addEventListener("pagehide", () => {
    lifecycle("pagehide");
    for (const requestId of pending.keys())
      finishBridgeRequest(requestId, null, new Error("页面已关闭。"));
  });
  for (const type of ["load", "error"]) document.addEventListener(type, event => {
    const image = event.target;
    if (image?.tagName !== "IMG") return;
    const source = image.currentSrc || image.getAttribute("src");
    if (!source || source.startsWith("data:")) return;
    recordDiagnostic(type === "load" ? "image.loaded" : "image.failed", {
      ...diagnosticLocation(source),
      width: image.naturalWidth, height: image.naturalHeight,
      imageKind: image.classList.contains("image-preview-image") ? "preview"
        : image.originalImageSource ? "thumbnail" : "image"
    });
  }, true);
  globalThis.addEventListener("error", event => {
    if (event.error) recordDiagnostic("javascript.error", {
      ...diagnosticLocation(event.filename), errorType: event.error.name || "Error", line: event.lineno
    });
  });
  globalThis.addEventListener("unhandledrejection", event =>
    recordDiagnostic("javascript.rejection", { errorType: event.reason?.name || "UnknownError" }));
}
