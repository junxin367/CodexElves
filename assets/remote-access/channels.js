let deviceId = null;
let generation = 0;
let discoveryTimer = null;
let discoveryPending = null;
let configuration = null;
const listeners = new Set();
const latencyObservations = new Set();
const primaryOrigin = () => globalThis.location.origin;
const messageActions = new Set(["send", "create", "setGoal"]);
const readPaths = new Set([
  "/api/session", "/api/channels", "/api/channel-events", "/api/read/sidebar", "/api/read/history", "/api/read/thread",
  "/api/read/queue", "/api/read/queueEdit", "/api/read/sendState", "/api/read/notifications"
]);

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
    return url.origin === value && url.protocol === "https:" && !url.username && !url.password
      && !url.port && /\.(?:shares\.zrok\.io|ngrok-free\.app|ngrok-free\.dev|ngrok\.app|ngrok\.dev|ngrok\.io)$/.test(url.hostname);
  } catch { return false; }
}

function changed() { for (const listener of listeners) listener(); }

export function clearRemoteChannels() {
  ++generation;
  deviceId = null;
  configuration = null;
  discoveryPending = null;
  clearInterval(discoveryTimer);
  discoveryTimer = null;
  for (const abort of latencyObservations) abort();
  changed();
}

function routes() {
  const primary = { origin: primaryOrigin(), token: null };
  if (!configuration || configuration.expiresAtMs <= Date.now()) return [primary];
  return [primary, ...configuration.origins.filter(origin => origin !== primary.origin)
    .slice(0, 1).map(origin => ({ origin, token: configuration.token }))];
}

export function remoteChannelRoutes() {
  return routes().map(({ origin }, index) => ({
    origin,
    name: new URL(origin).hostname.endsWith(".shares.zrok.io") ? "zrok"
      : /\.ngrok(?:-free)?\.(?:app|dev|io)$/.test(new URL(origin).hostname) ? "ngrok"
        : index === 0 ? "zrok" : "ngrok"
  }));
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

export async function sendRemoteMessage(path, options, consume, measurements = [], onUnavailable = null) {
  prepareRemoteTunnelAccess();
  const action = path.startsWith("/api/action/") ? path.slice("/api/action/".length) : "";
  const headers = new Headers(options.headers);
  const notSubmitted = message => Object.assign(new Error(message), { notSubmitted: true, outcomeUnknown: false });
  if (!messageActions.has(action) || options.method !== "POST"
      || !/^[a-zA-Z0-9-]{1,100}$/.test(headers.get("Idempotency-Key") || ""))
    throw notSubmitted("消息发送参数无效，操作未提交。");
  if (options.signal?.aborted) throw notSubmitted("消息发送已取消，操作未提交。");
  // 使用页面已展示的同一份线路状态；不探测、不竞速写入，也不在失败后换路重发。
  const candidates = routes().filter(route => !route.token || configuration?.sendActions?.includes(action))
    .map(route => ({ ...route, measurement: measurements.find(value => value.origin === route.origin) }))
    .filter(route => route.measurement?.status !== "unavailable");
  const measured = candidates.filter(route => route.measurement?.status === "ready"
    && Number.isFinite(route.measurement.milliseconds) && route.measurement.milliseconds >= 0);
  measured.sort((left, right) => left.measurement.milliseconds - right.measurement.milliseconds);
  // 尚无有效测量时，在未标记不可用的候选线路中优先原入口。
  const route = measured[0] || candidates[0];
  if (!route) throw notSubmitted("当前没有可用的消息发送线路，草稿已保留，请等待连接恢复。");
  const expectedServer = configuration?.serverId;
  headers.set("skip_zrok_interstitial", "1");
  headers.set("ngrok-skip-browser-warning", "1");
  if (route.token) headers.set("Authorization", `Bearer ${route.token}`);
  let receivedResponse = false;
  try {
    const response = await fetch(new URL(path, route.origin), {
      ...options, headers, credentials: route.token ? "omit" : options.credentials, redirect: "error"
    });
    receivedResponse = true;
    if (route.token && response.headers.get("x-codex-channel-server") !== expectedServer)
      throw Object.assign(new Error("发送线路服务身份不匹配，请核对原请求回执，勿重复发送。"), {
        responseReadFailed: true, outcomeUnknown: true
      });
    return validateRemoteReceipt(await consume(response), headers.get("Idempotency-Key"));
  } catch (error) {
    if (route.token) error.secondaryChannel = true;
    if (!receivedResponse || error.responseReadFailed || route.token && error.code === 401)
      onUnavailable?.(route.origin);
    throw error;
  }
}

export async function raceRemoteRead(path, options, consume, onLatency = null) {
  prepareRemoteTunnelAccess();
  const pathname = path.split("?")[0];
  const candidates = readPaths.has(pathname) || pathname.startsWith("/api/receipts/") ? routes() : routes().slice(0, 1);
  const controllers = candidates.map(() => new AbortController());
  const abort = () => controllers.forEach(controller => controller.abort());
  const measureBoth = pathname === "/api/session" && candidates.length > 1 && typeof onLatency === "function";
  const observedGeneration = generation;
  const names = remoteChannelRoutes();
  let attempts;
  const observationTimer = measureBoth ? setTimeout(abort, 15000) : null;
  if (measureBoth) latencyObservations.add(abort);
  const cleanup = () => {
    clearTimeout(observationTimer);
    latencyObservations.delete(abort);
    options.signal?.removeEventListener("abort", abort);
  };
  if (options.signal?.aborted) abort();
  else options.signal?.addEventListener("abort", abort, { once: true });
  try {
    attempts = candidates.map(async (route, index) => {
      const startedAt = performance.now();
      const report = value => {
        if (measureBoth && observedGeneration === generation)
          onLatency({ origin: route.origin, name: names[index].name, ...value });
      };
      try {
        const headers = new Headers(options.headers);
        headers.set("skip_zrok_interstitial", "1");
        headers.set("ngrok-skip-browser-warning", "1");
        if (route.token) {
          headers.set("Authorization", `Bearer ${route.token}`);
        }
        const response = await fetch(new URL(path, route.origin), {
          ...options, headers, credentials: route.token ? "omit" : options.credentials,
          redirect: "error", signal: controllers[index].signal
        });
        if (route.token && response.headers.get("x-codex-channel-server") !== configuration?.serverId)
          throw new Error("第二通道服务身份不匹配");
        const result = await consume(response);
        report({ status: "ready", milliseconds: performance.now() - startedAt });
        return result;
      } catch (error) {
        report({ status: "unavailable" });
        throw error;
      }
    });
    try { return await Promise.any(attempts); }
    catch (error) {
      // A transport failure on one route must not hide a successful response on the other.
      throw error.errors?.[0] || error;
    }
  } finally {
    if (measureBoth && attempts) {
      // The caller gets the first valid session immediately; only this small
      // status response keeps its slower route alive for an independent RTT.
      void Promise.allSettled(attempts).then(cleanup);
    } else {
      abort();
      cleanup();
    }
  }
}

async function discover() {
  if (!deviceId || discoveryPending) return discoveryPending;
  const observed = generation, expectedDevice = deviceId;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 12000);
  const pending = (async () => {
    const value = await raceRemoteRead("/api/channels", { credentials: "same-origin", signal: controller.signal }, async response => {
      if (!response.ok) throw new Error("第二通道发现暂不可用");
      const value = await response.json();
      if (value.ok !== true || value.schemaVersion !== 1 || value.deviceId !== expectedDevice
          || !/^[a-f0-9-]{36}$/.test(value.serverId) || !/^[a-f0-9]{64}$/.test(value.token)
          || !Number.isSafeInteger(value.expiresAtMs) || value.expiresAtMs <= Date.now()
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
  const update = () => listener(configuration);
  listeners.add(update);
  update();
  return () => listeners.delete(update);
}

// Called only by the same-origin SharedWorker's trusted MessagePorts.
export function applyChannelConfiguration(value) {
  if (value && (!Array.isArray(value.origins) || value.origins.some(origin => !validOrigin(origin))
      || !/^[a-f0-9]{64}$/.test(value.token))) return;
  configuration = value;
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
  let closed = false, gapTimer = null, recovering = false, recoveryController = null;
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
    const timer = setTimeout(() => controller.abort(), 10000);
    try {
      const value = await raceRemoteRead(`/api/channel-events${merger.query()}`,
        { credentials: "same-origin", signal: controller.signal }, async response => {
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
    if (!closed && ![...sockets.values()].some(entry => entry.active)) {
      readyState = 0; stream.onerror?.({ reason: "all-channels-disconnected" });
    }
    if (!closed && !globalThis.document?.hidden)
      entry.retry = setTimeout(() => open(entry), Math.min(10000, 500 * 2 ** Math.min(entry.failures++, 5)));
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
    const available = routes();
    for (const [origin, entry] of sockets) if (!available.some(route => route.origin === origin)) {
      dispose(entry); sockets.delete(origin);
    }
    if (readyState === 1 && ![...sockets.values()].some(entry => entry.active)) {
      readyState = 0; stream.onerror?.({ reason: "all-channels-disconnected" });
    }
    for (const route of available) {
      if (!sockets.has(route.origin)) sockets.set(route.origin, { origin: route.origin, failures: 0 });
      const entry = sockets.get(route.origin);
      if (!entry.socket) { clearTimeout(entry.retry); open(entry); }
    }
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
