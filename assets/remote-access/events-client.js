let sharedWorkerFailed = false;

const READY_STATE = {
  CONNECTING: 0,
  OPEN: 1,
  CLOSED: 2
};
const RETRY_DELAYS = [500, 1000, 2000, 5000, 10000];

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
  let retryAttempt = 0;
  let openedAt = 0;
  let errorReported = false;
  let closed = false;

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
      retryTimer = null;
      const current = socket;
      socket = null;
      if (current) {
        current.onopen = null;
        current.onmessage = null;
        current.onerror = null;
        current.onclose = null;
        try { current.close(); } catch {}
      }
    }
  };

  const reportError = event => {
    if (errorReported || closed) return;
    errorReported = true;
    stream.onerror?.(event);
  };

  const scheduleRetry = () => {
    if (closed || retryTimer !== null) return;
    readyState = READY_STATE.CONNECTING;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      connect();
    }, retryDelay(retryAttempt));
    retryAttempt = Math.min(retryAttempt + 1, RETRY_DELAYS.length - 1);
  };

  const connect = () => {
    if (closed || socket || retryTimer !== null) return;
    readyState = READY_STATE.CONNECTING;
    errorReported = false;

    let current;
    try {
      current = new WebSocket(eventsSocketUrl());
    } catch (error) {
      reportError(error);
      scheduleRetry();
      return;
    }

    socket = current;
    current.onopen = event => {
      if (closed || socket !== current) return;
      readyState = READY_STATE.OPEN;
      openedAt = Date.now();
      stream.onopen?.(event);
    };
    current.onmessage = event => {
      if (closed || socket !== current || typeof event.data !== "string") return;
      stream.onmessage?.({ data: event.data });
    };
    current.onerror = event => {
      if (closed || socket !== current) return;
      readyState = READY_STATE.CONNECTING;
      reportError(event);
    };
    current.onclose = event => {
      if (closed || socket !== current) return;
      socket = null;
      if (openedAt && Date.now() - openedAt >= 30000) retryAttempt = 0;
      openedAt = 0;
      readyState = READY_STATE.CONNECTING;
      reportError(event);
      scheduleRetry();
    };
  };

  connect();
  return stream;
}

function sharedStream() {
  const worker = new SharedWorker("/events-worker.js", "codex-elves-remote-events");
  const port = worker.port;
  let readyState = READY_STATE.CONNECTING;
  let closed = false;
  const stream = {
    transport: "shared-worker",
    onopen: null,
    onmessage: null,
    onerror: null,
    get readyState() { return readyState; },
    close() {
      if (closed) return;
      closed = true;
      readyState = READY_STATE.CLOSED;
      try { port.postMessage({ type: "unsubscribe" }); } catch {}
      worker.onerror = null;
      port.onmessage = null;
      port.onmessageerror = null;
      port.close();
    }
  };
  const fail = event => {
    if (closed) return;
    closed = true;
    sharedWorkerFailed = true;
    readyState = READY_STATE.CLOSED;
    try { port.postMessage({ type: "unsubscribe" }); } catch {}
    worker.onerror = null;
    port.onmessage = null;
    port.onmessageerror = null;
    port.close();
    stream.onerror?.(event);
  };
  worker.onerror = fail;
  port.onmessageerror = fail;
  port.onmessage = event => {
    if (closed) return;
    if (event.data?.type === "open") {
      readyState = READY_STATE.OPEN;
      stream.onopen?.(event);
    } else if (event.data?.type === "event") {
      stream.onmessage?.({ data: event.data.data });
    } else if (event.data?.type === "error") {
      readyState = READY_STATE.CONNECTING;
      stream.onerror?.(event);
    } else if (event.data?.type === "closed") {
      readyState = READY_STATE.CLOSED;
      stream.onerror?.(event);
    }
  };
  try {
    port.start();
    port.postMessage({ type: "subscribe" });
  } catch (error) {
    worker.onerror = null;
    port.onmessage = null;
    port.onmessageerror = null;
    try { port.close(); } catch {}
    throw error;
  }
  return stream;
}

export function createRemoteEventStream() {
  if (sharedEventsAvailable()) {
    try { return sharedStream(); }
    catch { sharedWorkerFailed = true; }
  }
  return fallbackStream();
}
