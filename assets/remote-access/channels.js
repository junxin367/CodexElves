import { getAndroidAppInfo, isAndroidContainer } from "./platform.js";
import { diagnosticLocation, recordDiagnostic, traceDiagnosticRequest } from "./diagnostics.js";

let deviceId = null;
let generation = 0;
let discoveryTimer = null;
let discoveryPending = null;
let configuration = null;
let channelMeasurements = [];
let measurementSignature = "";
let routeRequestSequence = 0;
const listeners = new Set();
const latencyObservations = new Set();
const connectionProbes = new Map();
const routeHealth = new Map();
const readRouteTimeout = 8000;
const probeInterval = 15000, maximumProbeBackoff = 60000;
const primaryOrigin = () => globalThis.location.origin;
const messageActions = new Set(["send", "create", "setGoal"]);
const mediaPath = /^\/api\/(?:images\/[A-Za-z0-9_-]{20,128}(?:\/thumbnail)?|videos\/[a-f0-9]{64}(?:\/poster)?)$/;
let fullRequestRouting = !isAndroidContainer();
if (!fullRequestRouting) void getAndroidAppInfo().then(info => {
  fullRequestRouting = info?.requestRoutingVersion === 2;
  changed();
});

export function prepareRemoteTunnelAccess() {
  if (!globalThis.document || globalThis.location?.protocol !== "https:"
      || !globalThis.location.hostname.endsWith(".shares.zrok.io")) return;
  try {
    if (document.cookie.split(";").some(value => value.trim() === "zrok_interstitial=1")) return;
    // <img> 无法添加跳过提示页的请求头；Android 内置页面也不会先打开该提示页。
    document.cookie = "zrok_interstitial=1; Path=/; Max-Age=604800; Secure; SameSite=Strict";
  } catch { /* Cookie 不可用时，保留原有登录和图片失败处理。 */ }
}
prepareRemoteTunnelAccess();

function validOrigin(value) {
  try {
    const url = new URL(value);
    return url.origin === value && !url.username && !url.password
      && (value === primaryOrigin() && ["http:", "https:"].includes(url.protocol)
        || url.protocol === "https:" && !url.port && /\.(?:shares\.zrok\.io|ngrok-free\.app|ngrok-free\.dev|ngrok\.app|ngrok\.dev|ngrok\.io)$/.test(url.hostname));
  } catch { return false; }
}

function changed() { for (const listener of listeners) listener(); }

export function clearRemoteChannels() {
  ++generation;
  deviceId = null;
  configuration = null;
  channelMeasurements = [];
  measurementSignature = "";
  discoveryPending = null;
  clearInterval(discoveryTimer);
  discoveryTimer = null;
  routeHealth.clear();
  for (const abort of latencyObservations) abort();
  changed();
}

const routeKey = route => JSON.stringify([generation, route.origin, route.token]);
function health(route) {
  if (!routeHealth.has(route.origin)) routeHealth.set(route.origin, {
    failures: 0, retryAt: 0, retryAfterUntil: 0, probeAt: 0, measurement: null, version: 0
  });
  return routeHealth.get(route.origin);
}
function retryDelay(response) {
  const value = response?.headers.get("Retry-After")?.trim();
  if (!value) return 0;
  const milliseconds = /^\d+$/.test(value) ? Number(value) * 1000 : Date.parse(value) - Date.now();
  return Number.isFinite(milliseconds) ? Math.max(0, Math.min(milliseconds, 2147483647)) : 0;
}
function failedRoute(route, response, sequence) {
  const state = health(route);
  const before = state.failures === 0 && state.retryAt <= Date.now();
  const delay = retryDelay(response);
  if (response?.status === 429 || delay > 0)
    state.retryAfterUntil = Math.max(state.retryAfterUntil, Date.now() + Math.max(probeInterval, delay));
  if (sequence < state.version) {
    state.retryAt = Math.max(state.retryAt, state.retryAfterUntil);
    if (state.retryAt > Date.now()) state.measurement = { status: "unavailable" };
    if (before && state.retryAt > Date.now()) changed();
    return false;
  }
  state.version = sequence;
  state.measurement = { status: "unavailable" };
  state.failures++;
  state.retryAt = Math.max(state.retryAt, state.retryAfterUntil, Date.now() + Math.max(delay,
    Math.min(maximumProbeBackoff, probeInterval * 2 ** Math.min(state.failures - 1, 2))));
  if (before) changed();
  return true;
}
function healthyRoute(route, sequence) {
  const state = health(route);
  const before = state.failures === 0 && state.retryAt <= Date.now();
  if (sequence < state.version) return false;
  state.version = sequence;
  state.failures = 0;
  state.retryAt = state.retryAfterUntil > Date.now() ? state.retryAfterUntil : 0;
  if (state.measurement?.status === "unavailable" && !state.retryAt) state.measurement = null;
  if (before !== (state.retryAt <= Date.now())) changed();
  return true;
}

function routes() {
  const primary = { origin: primaryOrigin(), token: null };
  if (!configuration || configuration.expiresAtMs <= Date.now()) return [primary];
  return [primary, ...configuration.origins.filter(origin => origin !== primary.origin)
    .slice(0, 1).map(origin => ({ origin, token: configuration.token }))];
}

export function remoteChannelRoutes() {
  return routes().map(({ origin }) => {
    const host = new URL(origin).hostname;
    const name = configuration?.primaryProvider === "frp" && configuration.primaryOrigin === origin ? "FRP"
      : host.endsWith(".shares.zrok.io") ? "zrok"
      : /\.(?:ngrok-free\.app|ngrok-free\.dev|ngrok\.app|ngrok\.dev|ngrok\.io)$/.test(host) ? "ngrok" : host;
    return { origin, name };
  });
}

export function setRemoteChannelMeasurements(values) {
  const next = (Array.isArray(values) ? values : []).filter(value =>
    typeof value?.origin === "string" && ["ready", "unknown", "unavailable"].includes(value.status))
    .map(({ origin, status, milliseconds }) => ({ origin, status, milliseconds }));
  const signature = JSON.stringify(next);
  if (signature === measurementSignature) return;
  measurementSignature = signature;
  channelMeasurements = next;
  for (const value of next) recordDiagnostic("channel.measurement", {
    ...diagnosticLocation(value.origin), phase: value.status === "ready" ? "ready"
      : value.status === "unavailable" ? "failure" : "pending", durationMs: value.milliseconds
  });
  changed();
}

function orderedRoutes(candidates = routes(), measurements = channelMeasurements) {
  return candidates.map((route, index) => ({ ...route, index, measurement: measurements.find(value => value.origin === route.origin) }))
    .filter(route => route.measurement?.status !== "unavailable"
      && health(route).failures === 0 && health(route).retryAt <= Date.now())
    .sort((left, right) => {
      const latency = route => route.measurement?.status === "ready"
        && Number.isFinite(route.measurement.milliseconds) && route.measurement.milliseconds >= 0
        ? route.measurement.milliseconds : Infinity;
      return latency(left) - latency(right) || left.index - right.index;
    });
}

function supportsRequest(route, path, method) {
  if (["/api/connect", "/api/login-status"].includes(path.split("?")[0])) return !route.token;
  if (!route.token) return true;
  if (fullRequestRouting && configuration?.requestRoutingVersion === 2) return true;
  if (method === "GET") return ["/api/session", "/api/channels", "/api/channel-events", "/api/events", "/api/events/ws"].includes(path.split("?")[0])
    || path.startsWith("/api/receipts/");
  if (path.startsWith("/api/read/")) return true;
  return configuration?.sendActions?.includes(path.slice("/api/action/".length)) === true;
}

// 媒体元素不能设置 Authorization；独立只读凭证仅可用于受限的媒体读取地址。
export function remoteMediaCandidates(path) {
  if (!mediaPath.test(path)) return [];
  return orderedRoutes(routes().filter(route => supportsRequest(route, path, "GET")))
    .flatMap(route => {
      if (!route.token) return [path];
      if (!/^[a-f0-9]{64}$/.test(configuration?.mediaToken || "")) return [];
      const url = new URL(path, route.origin);
      url.searchParams.set("channel_token", configuration.mediaToken);
      return [url.href];
    });
}

export function remoteMediaUrl(path) {
  return mediaPath.test(path) ? remoteMediaCandidates(path)[0] || "" : path;
}

export function validateRemoteReceipt(value, requestId) {
  if (!value || Array.isArray(value) || value.requestId !== requestId
      || !["pending", "completed", "failed", "unknown"].includes(value.state)
      || value.state === "completed" && value.result?.ok !== true)
    throw Object.assign(new Error("消息回执无效或与原请求不一致，请核对送达状态，勿重复发送。"), {
      responseReadFailed: true, outcomeUnknown: true
    });
  return value;
}

export async function requestRemote(path, options, consume, measurements = channelMeasurements, onUnavailable = null) {
  prepareRemoteTunnelAccess();
  const action = path.startsWith("/api/action/");
  const headers = new Headers(options.headers);
  const notSubmitted = message => Object.assign(new Error(message), { notSubmitted: true, outcomeUnknown: false });
  const target = new URL(path, primaryOrigin());
  if (!path.startsWith("/api/") || target.origin !== primaryOrigin() || target.hash
      || target.pathname !== path.split("?")[0] || !target.pathname.startsWith("/api/"))
    throw notSubmitted("请求地址无效，操作未提交。");
  if (action && (options.method !== "POST"
      || !/^[a-zA-Z0-9-]{1,100}$/.test(headers.get("Idempotency-Key") || "")))
    throw notSubmitted("请求参数无效，操作未提交。");
  if (options.signal?.aborted) throw notSubmitted("请求已取消，操作未提交。");
  const pathname = target.pathname;
  const read = options.method === "GET" && (["/api/session", "/api/channels", "/api/channel-events"].includes(pathname)
      || /^\/api\/receipts\/[a-zA-Z0-9-]{1,100}$/.test(pathname))
    || options.method === "POST" && (/^\/api\/read\/[a-zA-Z]+$/.test(pathname)
      || ["/api/workspace/changes", "/api/workspace/change"].includes(pathname));
  let candidates = orderedRoutes(routes().filter(route => supportsRequest(route, path, options.method)), measurements);
  if (!candidates.length) throw notSubmitted("当前没有可用的请求线路，请等待连接恢复后重试。");
  const observedGeneration = generation;
  const expectedServer = configuration?.serverId;
  headers.set("skip_zrok_interstitial", "1");
  headers.set("ngrok-skip-browser-warning", "1");
  let failure;
  const attempted = new Set();
  const remainingRoutes = () => attempted.size >= 2 ? [] : orderedRoutes(
    routes().filter(route => supportsRequest(route, path, options.method) && !attempted.has(route.origin))
  );
  // 动作仍只提交一次；只有这些重复安全的读取允许顺序尝试另一条授权线路。
  while (candidates.length && attempted.size < (read ? 2 : 1)) {
    const route = candidates[0];
    if (attempted.size && (observedGeneration !== generation || options.signal?.aborted)) break;
    attempted.add(route.origin);
    const requestSequence = ++routeRequestSequence;
    const controller = read ? new AbortController() : null;
    const abort = () => controller?.abort(options.signal?.reason);
    const startedAt = performance.now();
    let timer = null;
    const routeChanged = () => {
      if (observedGeneration !== generation) controller?.abort(new DOMException("授权已更新", "AbortError"));
      else if (remainingRoutes().length) {
        if (health(route).retryAt > Date.now()
            || channelMeasurements.some(value => value.origin === route.origin && value.status === "unavailable"))
          controller?.abort(new DOMException("读取线路已失效", "TimeoutError"));
        else if (timer === null)
          timer = setTimeout(() => controller?.abort(new DOMException("读取线路响应超时", "TimeoutError")),
            Math.max(0, readRouteTimeout - (performance.now() - startedAt)));
      } else {
        clearTimeout(timer);
        timer = null;
      }
    };
    if (options.signal?.aborted) abort();
    else if (controller) options.signal?.addEventListener("abort", abort, { once: true });
    if (controller) {
      listeners.add(routeChanged);
      routeChanged();
    }
    const attemptOptions = { ...options, signal: controller?.signal || options.signal };
    const attemptHeaders = new Headers(headers);
    if (route.token) attemptHeaders.set("Authorization", `Bearer ${route.token}`);
    let receivedResponse = false;
    let response;
    const trace = traceDiagnosticRequest(path, attemptOptions, route.origin);
    try {
      response = await fetch(new URL(path, route.origin), {
        ...attemptOptions, headers: attemptHeaders, credentials: route.token ? "omit" : options.credentials, redirect: "error"
      });
      receivedResponse = true;
      trace.response(response);
      if (route.token && response.headers.get("x-codex-channel-server") !== expectedServer)
        throw Object.assign(new Error("发送线路服务身份不匹配，请核对原请求回执，勿重复发送。"), {
          responseReadFailed: true, outcomeUnknown: true
        });
      const value = await consume(response);
      if (read && (observedGeneration !== generation || attemptOptions.signal?.aborted))
        throw attemptOptions.signal?.reason || new DOMException("读取已取消", "AbortError");
      const result = action ? validateRemoteReceipt(value, headers.get("Idempotency-Key")) : value;
      if (observedGeneration === generation) healthyRoute(route, requestSequence);
      trace.finish();
      return result;
    } catch (error) {
      trace.finish(error);
      if (route.token) error.secondaryChannel = true;
      failure = error;
      const cancelled = attemptOptions.signal?.aborted && attemptOptions.signal.reason?.name !== "TimeoutError";
      const attemptTimedOut = controller?.signal.aborted && controller.signal.reason?.name === "TimeoutError";
      const transportFailure = !receivedResponse || error.responseReadFailed || attemptTimedOut;
      let currentFailure = true;
      if (!cancelled && observedGeneration === generation && (transportFailure || [429, 502, 503, 504].includes(error.code)))
        currentFailure = failedRoute(route, response, requestSequence);
      if (!cancelled && currentFailure && (transportFailure || route.token && error.code === 401))
        onUnavailable?.(route.origin);
      if (!read || cancelled || options.signal?.aborted || observedGeneration !== generation || error.outcomeUnknown
          || receivedResponse && !attemptTimedOut && !error.responseReadFailed && ![429, 502, 503, 504].includes(error.code)) throw error;
    } finally {
      clearTimeout(timer);
      if (controller) {
        options.signal?.removeEventListener("abort", abort);
        listeners.delete(routeChanged);
      }
    }
    candidates = remainingRoutes();
  }
  throw failure || notSubmitted("当前没有可用的请求线路，请等待连接恢复后重试。");
}

export const sendRemoteMessage = requestRemote;

function connectionProbe(route, options) {
  const observedGeneration = generation;
  const key = routeKey(route);
  const pending = connectionProbes.get(key);
  if (pending && !pending.controller.signal.aborted) return pending;
  const controller = new AbortController();
  const abort = () => controller.abort(new DOMException("连接测速已取消", "AbortError"));
  const timer = setTimeout(() => controller.abort(new DOMException("连接测速超时", "TimeoutError")), 15000);
  const expectedServer = configuration?.serverId;
  const headers = new Headers(options.headers);
  headers.set("skip_zrok_interstitial", "1");
  headers.set("ngrok-skip-browser-warning", "1");
  if (route.token) headers.set("Authorization", `Bearer ${route.token}`);
  const startedAt = performance.now();
  const checkTakeover = () => {
    if (observedGeneration !== generation || performance.now() - startedAt < readRouteTimeout) return;
    if (orderedRoutes().some(other => other.origin !== route.origin
        && (other.measurement?.status === "ready" || health(other).version > 0)))
      controller.abort(new DOMException("已有健康备用线路，停止等待慢速测速", "TimeoutError"));
  };
  const takeoverTimer = setTimeout(checkTakeover, readRouteTimeout);
  listeners.add(checkTakeover);
  health(route).probeAt = Date.now();
  const entry = { controller, users: new Set(), promise: null, sequence: ++routeRequestSequence };
  const trace = traceDiagnosticRequest("/api/session", { ...options, signal: controller.signal }, route.origin);
  latencyObservations.add(abort);
  connectionProbes.set(key, entry);
  entry.promise = (async () => {
    try {
      const response = await fetch(new URL("/api/session", route.origin), {
        ...options, headers, credentials: route.token ? "omit" : options.credentials,
        redirect: "error", signal: controller.signal
      });
      trace.response(response);
      if (route.token && response.headers.get("x-codex-channel-server") !== expectedServer)
        throw new Error("第二通道服务身份不匹配");
      // 只缓存这个小型测速响应，使并发调用仍各自执行原有响应校验。
      const body = await response.arrayBuffer();
      const result = { body, status: response.status, statusText: response.statusText,
        headers: response.headers, milliseconds: performance.now() - startedAt };
      if (observedGeneration === generation) {
        const state = health(route);
        if (response.ok) {
          if (healthyRoute(route, entry.sequence)) {
            state.measurement = state.retryAt > Date.now() ? { status: "unavailable" }
              : { status: "ready", milliseconds: result.milliseconds };
            changed();
          }
        } else failedRoute(route, response, entry.sequence);
      }
      trace.finish(response.ok ? null : Object.assign(new Error("连接测速失败"), { code: response.status }));
      return result;
    } catch (error) {
      if (observedGeneration === generation && (!controller.signal.aborted || controller.signal.reason?.name === "TimeoutError")) {
        failedRoute(route, null, entry.sequence);
      }
      trace.finish(error);
      throw error;
    } finally {
      clearTimeout(timer);
      clearTimeout(takeoverTimer);
      listeners.delete(checkTakeover);
      latencyObservations.delete(abort);
      if (connectionProbes.get(key) === entry) connectionProbes.delete(key);
    }
  })();
  return entry;
}

export async function raceRemoteRead(path, options, consume, onLatency = null) {
  prepareRemoteTunnelAccess();
  if (path !== "/api/session" || options.method !== "GET" || typeof onLatency !== "function")
    return requestRemote(path, options, consume);
  if (options.signal?.aborted) throw options.signal.reason || new DOMException("连接已取消", "AbortError");
  if (globalThis.document?.hidden || globalThis.navigator?.onLine === false)
    throw new DOMException("当前不启动连接测速", "AbortError");
  const available = routes();
  const active = orderedRoutes(available)[0];
  const now = Date.now();
  const candidates = available.filter(route => {
    const state = health(route);
    const pending = connectionProbes.get(routeKey(route));
    if (pending && !pending.controller.signal.aborted) return true;
    const due = state.retryAt <= now && (!state.measurement || now - state.probeAt >= probeInterval);
    // 活动线路读取当前会话状态；备用线路只在到期时补测，不复用旧业务响应。
    if (due || active?.origin === route.origin) return true;
    if (state.measurement) onLatency({
      origin: route.origin, name: remoteChannelRoutes().find(value => value.origin === route.origin)?.name,
      ...state.measurement
    });
    return false;
  });
  if (!candidates.length) throw Object.assign(new Error("线路正在等待重试，请稍后再试。"), { notSubmitted: true });
  const observedGeneration = generation;
  const names = remoteChannelRoutes();
  const attemptRoute = async route => {
    const entry = connectionProbe(route, options);
    const user = {};
    entry.users.add(user);
    let abort;
    try {
      const result = await new Promise((resolve, reject) => {
        abort = () => {
          entry.users.delete(user);
          if (!entry.users.size) entry.controller.abort(options.signal?.reason);
          reject(options.signal.reason || new DOMException("连接已取消", "AbortError"));
        };
        options.signal?.addEventListener("abort", abort, { once: true });
        if (options.signal?.aborted) abort();
        entry.promise.then(resolve, reject);
      });
      if (observedGeneration !== generation || options.signal?.aborted)
        throw options.signal?.reason || new DOMException("授权已更新", "AbortError");
      const value = await consume(new Response(result.body, {
        status: result.status, statusText: result.statusText, headers: result.headers
      }));
      if (observedGeneration !== generation || options.signal?.aborted)
        throw options.signal?.reason || new DOMException("授权已更新", "AbortError");
      if (entry.sequence >= health(route).version)
        onLatency({ origin: route.origin, name: names.find(value => value.origin === route.origin)?.name,
          ...(health(route).retryAt > Date.now() ? { status: "unavailable" }
            : { status: "ready", milliseconds: result.milliseconds }) });
      return value;
    } catch (error) {
      if (route.token) error.secondaryChannel = true;
      const cancelled = options.signal?.aborted && options.signal.reason?.name !== "TimeoutError"
        || entry.controller.signal.aborted && entry.controller.signal.reason?.name !== "TimeoutError";
      if (!cancelled && observedGeneration === generation && entry.sequence >= health(route).version)
        onLatency({ origin: route.origin, name: names.find(value => value.origin === route.origin)?.name, status: "unavailable" });
      throw error;
    } finally {
      options.signal?.removeEventListener("abort", abort);
      entry.users.delete(user);
    }
  };
  const attempts = candidates.map(attemptRoute);
  const standby = available.filter(route => !candidates.some(candidate => candidate.origin === route.origin));
  let finished = false, recovering = false, timer = null;
  const recovery = standby.length ? new Promise((resolve, reject) => {
    const recover = () => {
      if (finished || recovering) return;
      recovering = true;
      if (observedGeneration !== generation || options.signal?.aborted) {
        reject(options.signal?.reason || new DOMException("授权已更新", "AbortError"));
        return;
      }
      const route = orderedRoutes(standby)[0];
      if (!route) { reject(new Error("备用线路暂不可用")); return; }
      // 这是活动线路阻塞后的真实状态读取，不使用备用线路的旧业务响应。
      void attemptRoute(route).then(resolve, reject);
    };
    timer = setTimeout(recover, readRouteTimeout);
    void Promise.allSettled(attempts).then(results => {
      if (results.every(result => result.status === "rejected")) recover();
    });
  }) : null;
  try { return await Promise.any(recovery ? [...attempts, recovery] : attempts); }
  catch (error) { throw error.errors?.[0] || error; }
  finally { finished = true; clearTimeout(timer); }
}

async function discover() {
  if (!deviceId || discoveryPending) return discoveryPending;
  const observed = generation, expectedDevice = deviceId;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(new DOMException("线路发现超时", "TimeoutError")), 12000);
  const pending = (async () => {
    const value = await requestRemote("/api/channels", { method: "GET", credentials: "same-origin", signal: controller.signal }, async response => {
      if (!response.ok) throw new Error("第二通道发现暂不可用");
      const value = await response.json();
      if (value.ok !== true || value.schemaVersion !== 1 || value.deviceId !== expectedDevice
          || !/^[a-f0-9-]{36}$/.test(value.serverId) || !/^[a-f0-9]{64}$/.test(value.token)
          || !Number.isSafeInteger(value.expiresAtMs) || value.expiresAtMs <= Date.now()
          || value.requestRoutingVersion !== undefined && value.requestRoutingVersion !== 2
          || value.requestRoutingVersion === 2 && !/^[a-f0-9]{64}$/.test(value.mediaToken || "")
          || value.sendActions !== undefined && (!Array.isArray(value.sendActions)
            || value.sendActions.length > messageActions.size || value.sendActions.some(action => !messageActions.has(action)))
          || !Array.isArray(value.origins) || value.origins.length > 2 || value.origins.some(origin => !validOrigin(origin)))
        throw new Error("第二通道发现数据无效");
      return value;
    });
    if (generation !== observed || deviceId !== expectedDevice) return;
    configuration = value;
    changed();
  })().catch(() => { /* The primary connection remains usable while discovery retries. */ })
    .finally(() => { clearTimeout(timeout); if (discoveryPending === pending) discoveryPending = null; });
  discoveryPending = pending;
  return pending;
}

export function activateRemoteChannels(id) {
  if (id !== deviceId) {
    clearRemoteChannels();
    deviceId = id;
  }
  if (!discoveryTimer) {
    discoveryTimer = setInterval(() => void discover(), 60000);
    void discover();
  }
}

// One ordered sequence for both sockets. The bounded journal repairs missing frames.
export function observeChannelConfiguration(listener) {
  const update = () => listener(configuration ? {
    ...configuration, measurements: channelMeasurements, channelRequestVersion: fullRequestRouting ? 2 : 1
  } : null);
  listeners.add(update);
  update();
  return () => listeners.delete(update);
}

// Called only by the same-origin SharedWorker's trusted MessagePorts.
export function applyChannelConfiguration(value) {
  if (value && (!Array.isArray(value.origins) || value.origins.some(origin => !validOrigin(origin))
      || !/^[a-f0-9]{64}$/.test(value.token))) return;
  configuration = value;
  if (value?.channelRequestVersion === 1) fullRequestRouting = false;
  else if (value?.channelRequestVersion === 2 && !isAndroidContainer()) fullRequestRouting = true;
  setRemoteChannelMeasurements(value?.measurements || []);
  changed();
}

export class ChannelEventMerger {
  constructor(deliver, gap) {
    this.deliver = deliver;
    this.gap = gap;
    this.epoch = null;
    this.cursor = 0;
    this.pending = new Map();
    this.retired = new Set();
  }
  accept(event) {
    if (event.method === "remote/streamReady") {
      if (typeof event.channelEpoch !== "string" || !Number.isSafeInteger(event.cursor) || event.cursor < 0) return;
      if (this.retired.has(event.channelEpoch)) return;
      if (event.channelEpoch !== this.epoch) {
        if (this.epoch) this.retired.add(this.epoch);
        this.epoch = event.channelEpoch;
        this.cursor = event.cursor;
        this.pending.clear();
        this.deliver({ method: "remote/resync" });
      } else if (event.reset && event.cursor > this.cursor) {
        this.cursor = event.cursor;
        this.pending.clear();
        this.deliver({ method: "remote/resync" });
      }
      for (const entry of event.events || []) this.accept(entry);
      return;
    }
    if (!Number.isSafeInteger(event.channelSequence)) {
      if (event.method === "remote/resync") this.gap();
      else this.deliver(event);
      return;
    }
    if (event.channelEpoch !== this.epoch || event.channelSequence <= this.cursor) return;
    this.pending.set(event.channelSequence, event);
    while (this.pending.has(this.cursor + 1)) {
      const next = this.pending.get(++this.cursor);
      this.pending.delete(this.cursor);
      this.deliver(next);
    }
    if (this.pending.size > 256) {
      this.cursor = Math.max(...this.pending.keys());
      this.pending.clear();
      this.deliver({ method: "remote/resync" });
    } else if (this.pending.size) this.gap();
  }
  query() {
    return this.epoch ? `?epoch=${encodeURIComponent(this.epoch)}&after=${this.cursor}` : "";
  }
}

export function createChannelEventStream() {
  const sockets = new Map();
  const retries = new Map();
  let closed = false, gapTimer = null, routeRetry = null, recovering = false, recoveryController = null;
  let readyState = 0;
  const merger = new ChannelEventMerger(
    event => stream.onmessage?.({ data: JSON.stringify(event) }),
    () => { if (!gapTimer) gapTimer = setTimeout(() => { gapTimer = null; void recover(); }, 200); }
  );
  const stream = {
    transport: "fallback", onopen: null, onmessage: null, onerror: null,
    get readyState() { return readyState; },
    close() {
      if (closed) return;
      closed = true; readyState = 2;
      clearTimeout(gapTimer);
      clearTimeout(routeRetry);
      recoveryController?.abort();
      listeners.delete(update);
      globalThis.removeEventListener?.("online", update);
      globalThis.removeEventListener?.("offline", offline);
      globalThis.removeEventListener?.("pagehide", stream.close);
      globalThis.document?.removeEventListener("visibilitychange", resume);
      for (const entry of sockets.values()) dispose(entry);
      sockets.clear();
    }
  };
  async function recover() {
    if (closed || recovering || !merger.epoch) return;
    recovering = true;
    const controller = recoveryController = new AbortController();
    const timer = setTimeout(() => controller.abort(new DOMException("事件补读超时", "TimeoutError")), 10000);
    try {
      const value = await requestRemote(`/api/channel-events${merger.query()}`,
        { method: "GET", credentials: "same-origin", signal: controller.signal }, async response => {
          if (!response.ok) throw new Error("事件补读失败");
          return response.json();
        });
      if (!closed) merger.accept(value);
    } catch {
      if (!closed) stream.onmessage?.({ data: '{"method":"remote/resync"}' });
    } finally {
      clearTimeout(timer); recovering = false;
      if (recoveryController === controller) recoveryController = null;
      if (!closed && merger.pending.size && !gapTimer)
        gapTimer = setTimeout(() => { gapTimer = null; void recover(); }, 1000);
    }
  }
  function dispose(entry) {
    clearTimeout(entry.retry); clearInterval(entry.watch);
    entry.active = false;
    if (entry.socket) {
      entry.socket.onopen = entry.socket.onmessage = entry.socket.onerror = entry.socket.onclose = null;
      try { entry.socket.close(); } catch {}
    }
  }
  function connected(entry) {
    entry.active = true;
    retries.delete(entry.origin);
    if (readyState !== 1) { readyState = 1; stream.onopen?.(); }
  }
  function open(entry) {
    if (closed || globalThis.navigator?.onLine === false || globalThis.document?.hidden || !sockets.has(entry.origin)) return;
    const route = routes().find(route => route.origin === entry.origin);
    if (!route) return;
    const expectedServer = route.token ? configuration.serverId : null;
    const url = new URL(`/api/events/ws${merger.query()}`, entry.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    let socket;
    try { socket = route.token ? new WebSocket(url, ["codex-elves-channel-v1", `codex-channel.${route.token}`]) : new WebSocket(url); }
    catch { fail(entry); return; }
    entry.socket = socket; entry.active = false; entry.verified = !route.token;
    entry.openedAt = null;
    entry.activity = entry.lastWatch = Date.now(); entry.graceUntil = 0;
    entry.watch = setInterval(() => {
      if (globalThis.document?.hidden) return;
      const now = Date.now();
      if (now - entry.lastWatch > 30000) entry.graceUntil = now + 20000;
      entry.lastWatch = now;
      if (now >= entry.graceUntil && now - entry.activity > (entry.active ? 75000 : 45000)) fail(entry);
    }, 5000);
    socket.onopen = () => {
      if (closed || entry.socket !== socket) return;
      entry.openedAt = entry.activity = Date.now();
      if (entry.verified) connected(entry);
    };
    socket.onmessage = message => {
      if (closed || entry.socket !== socket) return;
      entry.activity = Date.now();
      try {
        const event = JSON.parse(message.data);
        if (route.token && (!entry.verified || event?.method === "remote/streamReady")) {
          if (event?.method !== "remote/streamReady" || event.channelEpoch !== expectedServer
              || !Number.isSafeInteger(event.cursor) || event.cursor < 0) {
            fail(entry);
            return;
          }
          entry.verified = true;
          connected(entry);
        }
        merger.accept(event);
      } catch {}
    };
    socket.onerror = socket.onclose = () => { if (entry.socket === socket) fail(entry); };
  }
  function fail(entry) {
    if (entry.openedAt !== null && Date.now() - entry.openedAt >= 30000) entry.failures = 0;
    entry.openedAt = null;
    dispose(entry); entry.socket = null; entry.active = false;
    const failures = (retries.get(entry.origin)?.failures || 0) + 1;
    retries.set(entry.origin, { failures, at: Date.now() + Math.min(10000, 500 * 2 ** Math.min(failures - 1, 5)) });
    if (!closed && ![...sockets.values()].some(entry => entry.active)) {
      readyState = 0; stream.onerror?.({ reason: "all-channels-disconnected" });
    }
    update();
  }
  function offline() {
    for (const entry of sockets.values()) { dispose(entry); entry.socket = null; entry.active = false; }
    readyState = 0; stream.onerror?.({ reason: "offline" });
  }
  function resume() {
    if (closed) return;
    if (globalThis.document?.hidden) {
      for (const entry of sockets.values()) clearTimeout(entry.retry);
      return;
    }
    for (const entry of sockets.values()) entry.graceUntil = Date.now() + 20000;
    update();
  }
  function update() {
    if (closed) return;
    clearTimeout(routeRetry);
    const now = Date.now();
    const candidates = orderedRoutes(routes().filter(route => supportsRequest(route, "/api/events/ws", "GET")))
      .filter(route => (retries.get(route.origin)?.at || 0) <= now);
    const current = candidates.find(route => sockets.get(route.origin)?.active);
    let selected = candidates[0];
    if (current && selected && current.origin !== selected.origin) {
      const currentRtt = current.measurement?.milliseconds, nextRtt = selected.measurement?.milliseconds;
      if (Number.isFinite(currentRtt) && Number.isFinite(nextRtt)
          && currentRtt - nextRtt < Math.max(150, currentRtt * 0.2)) selected = current;
    }
    const available = selected ? [selected] : [];
    for (const [origin, entry] of sockets) if (!available.some(route => route.origin === origin)) {
      dispose(entry); sockets.delete(origin);
    }
    // 小幅延迟抖动保留已有连接；明确故障或明显更快的线路才接替。
    for (const route of available) {
      if (!sockets.has(route.origin)) sockets.set(route.origin, { origin: route.origin, failures: 0 });
      const entry = sockets.get(route.origin);
      if (!entry.socket) { clearTimeout(entry.retry); open(entry); }
    }
    const waiting = [...retries.values()].filter(value => value.at > now);
    if (waiting.length && !globalThis.document?.hidden)
      routeRetry = setTimeout(update, Math.max(100, Math.min(...waiting.map(value => value.at)) - now));
  }
  listeners.add(update);
  globalThis.addEventListener?.("online", update);
  globalThis.addEventListener?.("offline", offline);
  globalThis.addEventListener?.("pagehide", stream.close);
  globalThis.document?.addEventListener("visibilitychange", resume);
  // Let the caller install callbacks before any synchronous mocked socket events.
  queueMicrotask(update);
  return stream;
}
