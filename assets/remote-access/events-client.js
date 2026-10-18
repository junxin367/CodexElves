let sharedWorkerFailed = false;

const READY_STATE = {
  CONNECTING: 0,
  OPEN: 1,
  CLOSED: 2
};
const RETRY_DELAYS = [500, 1000, 2000, 5000, 10000];
const HANDSHAKE_TIMEOUT = 45000;
const ACTIVITY_TIMEOUT = 75000;
const WORKER_START_TIMEOUT = 20000;
const WORKER_RESPONSE_TIMEOUT = 90000;
const WATCH_INTERVAL = 5000;
const TIMER_SUSPENSION = 30000;
const RESUME_GRACE = 20000;

export function sharedEventsAvailable() {
  return !sharedWorkerFailed && typeof SharedWorker === "function";
}

function eventsSocketUrl() {
  const url = new URL("/api/events/ws", location.href);
  url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

function retryDelay(attempt) {
  const base = RETRY_DELAYS[Math.min(attempt, RETRY_DELAYS.length - 1)];
  return Math.round(base * (0.8 + Math.random() * 0.4));
}

function fallbackStream() {
  let readyState = READY_STATE.CONNECTING;
  let socket = null;
  let retryTimer = null;
  let watchTimer = null;
  let retryAttempt = 0;
  let openedAt = null;
  let lastActivityAt = 0;
  let connectingAt = 0;
  let lastWatchAt = 0;
  let graceUntil = 0;
  let errorReported = false;
  let closed = false;
  let online = navigator.onLine !== false;
  let hidden = document.hidden;

  const stream = {
    transport: "fallback",
    onopen: null,
    onmessage: null,
    onerror: null,
    get readyState() { return readyState; },
    close() {
      if (closed) return;
      closed = true;
      readyState = READY_STATE.CLOSED;
      clearTimeout(retryTimer);
      clearTimeout(watchTimer);
      retryTimer = null;
      watchTimer = null;
      closeSocket();
      window.removeEventListener("offline", offline);
      window.removeEventListener("online", restore);
      window.removeEventListener("pagehide", stream.close);
      document.removeEventListener("visibilitychange", visibility);
    }
  };

  const closeSocket = () => {
    const current = socket;
    socket = null;
    if (!current) return;
    current.onopen = null;
    current.onmessage = null;
    current.onerror = null;
    current.onclose = null;
    try { current.close(); } catch {}
  };

  const reportError = event => {
    if (errorReported || closed) return;
    errorReported = true;
    stream.onerror?.(event);
  };

  const scheduleRetry = () => {
    if (closed || hidden || !online || retryTimer !== null) return;
    readyState = READY_STATE.CONNECTING;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      connect();
    }, retryDelay(retryAttempt));
    retryAttempt = Math.min(retryAttempt + 1, RETRY_DELAYS.length - 1);
  };

  const fail = event => {
    if (closed) return;
    if (openedAt !== null && Date.now() - openedAt >= 30000) retryAttempt = 0;
    openedAt = null;
    clearTimeout(watchTimer);
    watchTimer = null;
    closeSocket();
    readyState = READY_STATE.CONNECTING;
    reportError(event);
    scheduleRetry();
  };

  const watch = () => {
    watchTimer = null;
    if (closed || !socket || hidden || !online) return;
    const now = Date.now();
    // A suspended tab may run its timer before queued heartbeat messages.
    if (now - lastWatchAt > TIMER_SUSPENSION) graceUntil = now + RESUME_GRACE;
    lastWatchAt = now;
    const deadline = openedAt === null
      ? connectingAt + HANDSHAKE_TIMEOUT
      : lastActivityAt + ACTIVITY_TIMEOUT;
    if (now >= deadline && now >= graceUntil) {
      fail({ type: "error", reason: openedAt === null ? "handshake-timeout" : "activity-timeout" });
      return;
    }
    watchTimer = setTimeout(watch, WATCH_INTERVAL);
  };

  const connect = () => {
    if (closed || hidden || !online || socket || retryTimer !== null) return;
    readyState = READY_STATE.CONNECTING;
    errorReported = false;
    openedAt = null;
    connectingAt = lastWatchAt = Date.now();
    graceUntil = 0;

    let current;
    try {
      current = new WebSocket(eventsSocketUrl());
    } catch (error) {
      reportError(error);
      scheduleRetry();
      return;
    }

    socket = current;
    watchTimer = setTimeout(watch, WATCH_INTERVAL);
    current.onopen = event => {
      if (closed || socket !== current) return;
      readyState = READY_STATE.OPEN;
      openedAt = lastActivityAt = Date.now();
      stream.onopen?.(event);
    };
    current.onmessage = event => {
      if (closed || socket !== current || typeof event.data !== "string") return;
      lastActivityAt = Date.now();
      stream.onmessage?.({ data: event.data });
    };
    current.onerror = event => {
      if (closed || socket !== current) return;
      fail(event);
    };
    current.onclose = event => {
      if (closed || socket !== current) return;
      fail(event);
    };
  };

  const pause = reason => {
    clearTimeout(retryTimer);
    clearTimeout(watchTimer);
    retryTimer = watchTimer = null;
    closeSocket();
    readyState = READY_STATE.CONNECTING;
    if (reason) reportError({ type: "error", reason });
  };
  const offline = () => {
    online = false;
    pause("offline");
  };
  const restore = () => {
    if (closed) return;
    online = navigator.onLine !== false;
    if (!online || hidden) return;
    if (!socket) {
      connect();
      return;
    }
    const now = Date.now();
    lastWatchAt = now;
    graceUntil = Math.max(graceUntil, now + RESUME_GRACE);
    if (watchTimer === null) watchTimer = setTimeout(watch, WATCH_INTERVAL);
  };
  const visibility = () => {
    hidden = document.hidden;
    if (hidden) {
      clearTimeout(retryTimer);
      clearTimeout(watchTimer);
      retryTimer = watchTimer = null;
    } else restore();
  };
  window.addEventListener("offline", offline);
  window.addEventListener("online", restore);
  window.addEventListener("pagehide", stream.close);
  document.addEventListener("visibilitychange", visibility);
  connect();
  return stream;
}

function sharedStream() {
  const worker = new SharedWorker("/events-worker.js", "codex-elves-remote-events");
  const port = worker.port;
  let readyState = READY_STATE.CONNECTING;
  let closed = false;
  let fallback = null;
  let watchTimer = null;
  let workerReady = false;
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
    fallback = fallbackStream();
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
  return fallbackStream();
}
