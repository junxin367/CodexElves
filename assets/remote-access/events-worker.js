const subscribers = new Set();
const RETRY_DELAYS = [500, 1000, 2000, 5000, 10000];
let socket = null;
let retryTimer = null;
let retryAttempt = 0;

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
  for (const port of subscribers) {
    try { port.postMessage(message); }
    catch { unsubscribe(port); }
  }
}

function closeSocket() {
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
  if (subscribers.size) return;
  clearTimeout(retryTimer);
  retryTimer = null;
  retryAttempt = 0;
  closeSocket();
}

function scheduleRetry() {
  if (!subscribers.size || retryTimer !== null) return;
  const delay = retryDelay(retryAttempt);
  retryAttempt = Math.min(retryAttempt + 1, RETRY_DELAYS.length - 1);
  retryTimer = setTimeout(() => {
    retryTimer = null;
    openSocket();
  }, delay);
}

function openSocket() {
  if (socket || retryTimer !== null || !subscribers.size) return;

  let current;
  try {
    current = new WebSocket(eventsSocketUrl());
  } catch {
    broadcast({ type: "error" });
    scheduleRetry();
    return;
  }

  socket = current;
  let errorReported = false;
  let openedAt = 0;
  current.onopen = () => {
    if (socket !== current) return;
    openedAt = Date.now();
    broadcast({ type: "open" });
  };
  current.onmessage = event => {
    if (socket !== current || typeof event.data !== "string") return;
    broadcast({ type: "event", data: event.data });
  };
  current.onerror = () => {
    if (socket !== current || errorReported) return;
    errorReported = true;
    broadcast({ type: "error" });
  };
  current.onclose = () => {
    if (socket !== current) return;
    socket = null;
    if (openedAt && Date.now() - openedAt >= 30000) retryAttempt = 0;
    if (!errorReported) broadcast({ type: "error" });
    scheduleRetry();
  };
}

function subscribe(port) {
  if (subscribers.has(port)) return;
  subscribers.add(port);
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
    if (message.data?.type === "subscribe") subscribe(port);
    else if (message.data?.type === "unsubscribe") unsubscribe(port);
  };
  port.onmessageerror = () => unsubscribe(port);
  port.start();
};
