import { createChannelEventStream, observeChannelConfiguration } from "./channels.js";

let sharedWorkerFailed = false;

const READY_STATE = {
  CONNECTING: 0,
  OPEN: 1,
  CLOSED: 2
};
const WORKER_START_TIMEOUT = 20000;
const WORKER_RESPONSE_TIMEOUT = 90000;
const WATCH_INTERVAL = 5000;
const TIMER_SUSPENSION = 30000;
const RESUME_GRACE = 20000;

export function sharedEventsAvailable() {
  return !sharedWorkerFailed && typeof SharedWorker === "function";
}

function sharedStream() {
  const worker = new SharedWorker("/events-worker.js", { name: "codex-elves-remote-events-v2", type: "module" });
  const port = worker.port;
  let readyState = READY_STATE.CONNECTING;
  let closed = false;
  let fallback = null;
  let watchTimer = null;
  let workerReady = false;
  const stopConfiguration = observeChannelConfiguration(channels => {
    try { port.postMessage({ type: "channels", channels }); } catch {}
  });
  let startedAt = Date.now();
  let lastActivityAt = startedAt;
  let lastWatchAt = startedAt;
  let lastPingAt = startedAt;
  let graceUntil = 0;
  let online = navigator.onLine !== false;
  let errorReported = false;
  const stream = {
    get transport() { return fallback ? "fallback" : "shared-worker"; },
    onopen: null,
    onmessage: null,
    onerror: null,
    get readyState() { return fallback ? fallback.readyState : readyState; },
    close() {
      if (closed) return;
      closed = true;
      readyState = READY_STATE.CLOSED;
      detachWorker();
      fallback?.close();
    }
  };
  const detachWorker = () => {
    stopConfiguration();
    clearTimeout(watchTimer);
    watchTimer = null;
    try { port.postMessage({ type: "unsubscribe" }); } catch {}
    worker.onerror = null;
    port.onmessage = null;
    port.onmessageerror = null;
    try { port.close(); } catch {}
    window.removeEventListener("offline", offline);
    window.removeEventListener("online", restore);
    window.removeEventListener("pagehide", stream.close);
    document.removeEventListener("visibilitychange", visibility);
  };
  const reportError = event => {
    if (closed || errorReported) return;
    errorReported = true;
    stream.onerror?.(event);
  };
  const fail = event => {
    if (closed || fallback) return;
    sharedWorkerFailed = true;
    detachWorker();
    fallback = createChannelEventStream();
    fallback.onopen = event => {
      if (closed) return;
      errorReported = false;
      stream.onopen?.(event);
    };
    fallback.onmessage = event => { if (!closed) stream.onmessage?.(event); };
    fallback.onerror = event => { if (!closed) stream.onerror?.(event); };
    reportError(event);
  };
  const post = message => {
    try { port.postMessage(message); }
    catch (error) { fail(error); }
  };
  const watch = () => {
    watchTimer = null;
    if (closed || fallback || document.hidden || !online) return;
    const now = Date.now();
    if (now - lastWatchAt > TIMER_SUSPENSION) graceUntil = now + RESUME_GRACE;
    lastWatchAt = now;
    const deadline = workerReady ? lastActivityAt + WORKER_RESPONSE_TIMEOUT : startedAt + WORKER_START_TIMEOUT;
    if (now >= deadline && now >= graceUntil) {
      fail({ type: "error", reason: workerReady ? "worker-response-timeout" : "worker-timeout" });
      return;
    }
    if (now - lastPingAt >= 30000) {
      lastPingAt = now;
      post({ type: "ping", online });
    }
    if (!fallback && !closed) watchTimer = setTimeout(watch, WATCH_INTERVAL);
  };
  const offline = () => {
    online = false;
    clearTimeout(watchTimer);
    watchTimer = null;
    readyState = READY_STATE.CONNECTING;
    post({ type: "network", online: false });
    reportError({ type: "error", reason: "offline" });
  };
  const restore = () => {
    if (closed || fallback) return;
    online = navigator.onLine !== false;
    const now = Date.now();
    graceUntil = now + RESUME_GRACE;
    startedAt = lastWatchAt = now;
    post({ type: "resume", online });
    if (!closed && !fallback && !document.hidden && online && watchTimer === null) watchTimer = setTimeout(watch, WATCH_INTERVAL);
  };
  const visibility = () => {
    if (document.hidden) {
      clearTimeout(watchTimer);
      watchTimer = null;
    } else restore();
  };
  worker.onerror = fail;
  port.onmessageerror = fail;
  port.onmessage = event => {
    if (closed || fallback) return;
    workerReady = true;
    lastActivityAt = Date.now();
    if (event.data?.type === "open") {
      if (!online || readyState === READY_STATE.OPEN) return;
      readyState = READY_STATE.OPEN;
      errorReported = false;
      stream.onopen?.(event);
    } else if (event.data?.type === "event") {
      if (!online) return;
      stream.onmessage?.({ data: event.data.data });
    } else if (event.data?.type === "error") {
      readyState = READY_STATE.CONNECTING;
      reportError({ type: "error", reason: event.data.reason, originalEvent: event });
    } else if (event.data?.type === "closed") {
      fail(event);
    }
  };
  try {
    port.start();
    port.postMessage({ type: "subscribe", online });
  } catch (error) {
    detachWorker();
    throw error;
  }
  window.addEventListener("offline", offline);
  window.addEventListener("online", restore);
  window.addEventListener("pagehide", stream.close);
  document.addEventListener("visibilitychange", visibility);
  if (!document.hidden && online) watchTimer = setTimeout(watch, WATCH_INTERVAL);
  return stream;
}

export function createRemoteEventStream() {
  if (sharedEventsAvailable()) {
    try { return sharedStream(); }
    catch { sharedWorkerFailed = true; }
  }
  return createChannelEventStream();
}
