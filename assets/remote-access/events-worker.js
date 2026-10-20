import { applyChannelConfiguration, createChannelEventStream } from "./channels.js";

const subscribers = new Map();
let stream = null;

function broadcast(message) {
  for (const port of subscribers.keys()) {
    try { port.postMessage(message); }
    catch { unsubscribe(port); }
  }
}

function update() {
  if (![...subscribers.values()].some(Boolean)) {
    stream?.close();
    stream = null;
    return;
  }
  if (stream) return;
  stream = createChannelEventStream();
  stream.onopen = () => broadcast({ type: "open" });
  stream.onmessage = event => broadcast({ type: "event", data: event.data });
  stream.onerror = event => broadcast({ type: "error", reason: event?.reason });
}

function unsubscribe(port) {
  subscribers.delete(port);
  port.onmessage = port.onmessageerror = null;
  try { port.close(); } catch {}
  update();
}

self.onconnect = event => {
  const port = event.ports[0];
  port.onmessage = message => {
    const data = message.data;
    if (data?.type === "channels") {
      applyChannelConfiguration(data.channels);
    } else if (data?.type === "subscribe") {
      subscribers.set(port, data.online !== false);
      port.postMessage({ type: "ready" });
      if (stream?.readyState === 1) port.postMessage({ type: "open" });
      update();
    } else if (data?.type === "unsubscribe") unsubscribe(port);
    else if (["network", "resume", "ping"].includes(data?.type) && subscribers.has(port)) {
      subscribers.set(port, data.online !== false);
      port.postMessage({ type: "pong" });
      update();
      if (stream?.readyState === 1) port.postMessage({ type: "open" });
    }
  };
  port.onmessageerror = () => unsubscribe(port);
  port.start();
};
