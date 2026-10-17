const subscribers = new Map();
const RETRY_DELAYS = [500, 1000, 2000, 5000, 10000];
const HANDSHAKE_TIMEOUT = 45000;
const ACTIVITY_TIMEOUT = 75000;
const WATCH_INTERVAL = 5000;
const TIMER_SUSPENSION = 30000;
const RESUME_GRACE = 20000;
let socket = null;
let retryTimer = null;
let watchTimer = null;
let retryAttempt = 0;
let openedAt = null;
let connectingAt = 0;
let lastActivityAt = 0;
let lastWatchAt = 0;
let graceUntil = 0;
let errorReported = false;

function online() {
  return [...subscribers.values()].some(value => value);
}

function eventsSocketUrl() {
  const url = new URL("/api/events/ws", self.location.href);
  url.protocol = self.location.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

function retryDelay(attempt) {
  const base = RETRY_DELAYS[Math.min(attempt, RETRY_DELAYS.length - 1)];
  return Math.round(base * (0.8 + Math.random() * 0.4));
}

function broadcast(message) {
  for (const port of subscribers.keys()) {
    try { port.postMessage(message); }
    catch { unsubscribe(port); }
  }
}

function closeSocket() {
  clearTimeout(watchTimer);
  watchTimer = null;
  const current = socket;
  socket = null;
  if (!current) return;
  current.onopen = null;
  current.onmessage = null;
  current.onerror = null;
  current.onclose = null;
  try { current.close(); } catch {}
}

function unsubscribe(port) {
  subscribers.delete(port);
  port.onmessage = null;
  port.onmessageerror = null;
  try { port.close(); } catch {}
  if (subscribers.size && online()) return;
  clearTimeout(retryTimer);
  retryTimer = null;
  retryAttempt = 0;
  closeSocket();
}

function scheduleRetry() {
  if (!online() || retryTimer !== null) return;
  const delay = retryDelay(retryAttempt);
  retryAttempt = Math.min(retryAttempt + 1, RETRY_DELAYS.length - 1);
  retryTimer = setTimeout(() => {
    retryTimer = null;
    openSocket();
  }, delay);
}

function fail(reason) {
  if (openedAt !== null && Date.now() - openedAt >= 30000) retryAttempt = 0;
  openedAt = null;
  closeSocket();
  if (!errorReported) {
    errorReported = true;
    broadcast({ type: "error", reason });
  }
  scheduleRetry();
}

function watch() {
  watchTimer = null;
  if (!socket || !online()) return;
  const now = Date.now();
  // Let queued heartbeats run after browser/worker timer suspension.
  if (now - lastWatchAt > TIMER_SUSPENSION) graceUntil = now + RESUME_GRACE;
  lastWatchAt = now;
  const deadline = openedAt === null
    ? connectingAt + HANDSHAKE_TIMEOUT
    : lastActivityAt + ACTIVITY_TIMEOUT;
  if (now >= deadline && now >= graceUntil) {
    fail(openedAt === null ? "handshake-timeout" : "activity-timeout");
    return;
  }
  watchTimer = setTimeout(watch, WATCH_INTERVAL);
}

function openSocket() {
  if (socket || retryTimer !== null || !online()) return;
  errorReported = false;
  openedAt = null;
  connectingAt = lastWatchAt = Date.now();
  graceUntil = 0;

  let current;
  try {
    current = new WebSocket(eventsSocketUrl());
  } catch {
    fail("socket-error");
    return;
  }

  socket = current;
  watchTimer = setTimeout(watch, WATCH_INTERVAL);
  current.onopen = () => {
    if (socket !== current) return;
    openedAt = lastActivityAt = Date.now();
    broadcast({ type: "open" });
  };
  current.onmessage = event => {
    if (socket !== current || typeof event.data !== "string") return;
    lastActivityAt = Date.now();
    broadcast({ type: "event", data: event.data });
  };
  current.onerror = () => {
    if (socket !== current) return;
    fail("socket-error");
  };
  current.onclose = () => {
    if (socket !== current) return;
    fail("socket-closed");
  };
}

function subscribe(port, isOnline) {
  if (subscribers.has(port)) return;
  subscribers.set(port, isOnline !== false);
  // Acknowledgement is independent of the slower WebSocket handshake.
  try { port.postMessage({ type: "ready" }); }
  catch { unsubscribe(port); return; }
  if (!online()) {
    try { port.postMessage({ type: "error", reason: "offline" }); }
    catch { unsubscribe(port); }
    return;
  }
  if (socket?.readyState === WebSocket.OPEN) {
    try { port.postMessage({ type: "open" }); }
    catch { unsubscribe(port); }
    return;
  }
  openSocket();
}

function updateNetwork(port, isOnline, resume) {
  if (!subscribers.has(port)) return;
  subscribers.set(port, isOnline !== false);
  try { port.postMessage({ type: "pong" }); }
  catch { unsubscribe(port); return; }
  if (!online()) {
    clearTimeout(retryTimer);
    retryTimer = null;
    closeSocket();
    broadcast({ type: "error", reason: "offline" });
    return;
  }
  if (resume) graceUntil = Math.max(graceUntil, Date.now() + RESUME_GRACE);
  if (socket?.readyState === WebSocket.OPEN) {
    try { port.postMessage({ type: "open" }); }
    catch { unsubscribe(port); }
    return;
  }
  openSocket();
}

self.onconnect = event => {
  const port = event.ports[0];
  port.onmessage = message => {
    if (message.data?.type === "subscribe") subscribe(port, message.data.online);
    else if (message.data?.type === "unsubscribe") unsubscribe(port);
    else if (["network", "resume", "ping"].includes(message.data?.type)) {
      updateNetwork(port, message.data.online, message.data.type === "resume");
    }
  };
  port.onmessageerror = () => unsubscribe(port);
  port.start();
};
