// 连接共享状态：线路延迟与连接指示、实时流与刷新计时、前台读取代次，以及登录状态。

import { remoteChannelRoutes, setRemoteChannelMeasurements } from "../../channels.js";
import { recordDiagnostic } from "../../diagnostics.js";
import { $ } from "../../ui/dom.js";
import { loadSidebar, reloadThread, selectThread } from "../../core/actions.js";
import { connected, creating, nativeConnectionVersion, pageLeaving, selectedId, session } from "../../core/state.js";
import { privateInputScope } from "./capabilities.js";

export let initialAuthenticationRequired = false;
// 从会话存储恢复“需要重新登录”标记。
export function restoreAuthenticationRequirement() {
  try { initialAuthenticationRequired = sessionStorage.getItem("codex-elves-auth-required") === "1"; } catch {}
}
export function setAuthenticationRequired(required) {
  initialAuthenticationRequired = required;
  try {
    if (required) sessionStorage.setItem("codex-elves-auth-required", "1");
    else sessionStorage.removeItem("codex-elves-auth-required");
  } catch {}
}
let loginClientId = "";
export function remoteLoginClientId() {
  // 用户要求同一 App/浏览器重复登录不累积设备；随机标识只做去重，不能替代访问密钥，也不采集硬件指纹。
  if (loginClientId) return loginClientId;
  try { loginClientId = localStorage.getItem("codex-elves-login-client-id") || ""; } catch {}
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(loginClientId)) {
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    bytes[6] = bytes[6] & 15 | 64; bytes[8] = bytes[8] & 63 | 128;
    const hex = [...bytes].map(value => value.toString(16).padStart(2, "0")).join("");
    loginClientId = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }
  try { localStorage.setItem("codex-elves-login-client-id", loginClientId); } catch {}
  return loginClientId;
}
export let loginPageCleanup = null;
export function setLoginPageCleanup(value) { return loginPageCleanup = value; }
export function connectionErrorMessage(error) {
  return error instanceof TypeError
    ? "无法连接服务，请检查网络和远程访问地址后重试。"
    : error?.message || "连接失败，请检查网络后重试。";
}

export let sessionRequest = null, connectionPollVersion = 0;
export function setSessionRequest(value) { return sessionRequest = value; }
export function setConnectionPollVersion(value) { return connectionPollVersion = value; }
export const sessionRecoveryReads = new WeakMap();
export let connectionCycleVersion = 0, connectionRtt = null;
export function setConnectionCycleVersion(value) { return connectionCycleVersion = value; }
export function setConnectionRtt(value) { return connectionRtt = value; }
export let connectionLatencyVersion = 0, connectionLatencyFloor = 0, connectionRouteSignature = "", connectionLatencyRefresh = null;
export function setConnectionLatencyVersion(value) { return connectionLatencyVersion = value; }
export function setConnectionLatencyFloor(value) { return connectionLatencyFloor = value; }
export function setConnectionRouteSignature(value) { return connectionRouteSignature = value; }
export function setConnectionLatencyRefresh(value) { return connectionLatencyRefresh = value; }
export const channelRtts = new Map();
export const connectionRttDescription = "点击探测连接并刷新会话。网页到接入服务的 /api/session HTTP 完整往返时长；存在两条已授权线路时分别测量，不代表模型响应耗时或端到端原生执行延迟。消息使用此处可用线路中延迟最低的一路发送，发出后不自动换路重发。";
export function cancelConnectionProbe() {
  connectionLatencyFloor = ++connectionLatencyVersion;
  channelRtts.clear();
  sessionRequest?.controller.abort();
  sessionRequest = null;
  connectionRtt = null;
}
export function currentChannelMeasurements() {
  return remoteChannelRoutes().map(route => {
    const measurement = channelRtts.get(route.origin);
    const current = connected && measurement?.deviceId === session?.device.id
      && measurement.nativeVersion === nativeConnectionVersion
      && measurement.connectionVersion === connectionCycleVersion
      && measurement.latencyVersion >= connectionLatencyFloor
      && route.origin === measurement.origin;
    return current ? measurement : { ...route, status: "unknown", deviceId: session?.device.id,
      nativeVersion: nativeConnectionVersion, connectionVersion: connectionCycleVersion, latencyVersion: connectionLatencyVersion };
  });
}
export function markMessageChannelUnavailable(origin, observed) {
  if (!connected || pageLeaving) return;
  const previous = observed.find(measurement => measurement.origin === origin);
  if (!previous || previous.deviceId !== session?.device.id
      || previous.nativeVersion !== nativeConnectionVersion || previous.connectionVersion !== connectionCycleVersion
      || previous.latencyVersion < connectionLatencyFloor) return;
  const latest = currentChannelMeasurements().find(measurement => measurement.origin === origin);
  // 旧发送结果不能覆盖后续已完成的连接测量，也不能污染新入口。
  if (!latest || latest.status !== "unknown" && latest !== previous) return;
  channelRtts.set(previous.origin, { ...previous, status: "unavailable", latencyVersion: ++connectionLatencyVersion });
  renderConnectionIndicator();
}
export function renderConnectionIndicator() {
  setRemoteChannelMeasurements(currentChannelMeasurements());
  const node = $("#connection-state"); if (!node) return;
  const measured = connected && connectionRtt?.deviceId === session?.device.id
    && connectionRtt.nativeVersion === nativeConnectionVersion
    && connectionRtt.connectionVersion === connectionCycleVersion ? connectionRtt : null;
  const activeRoutes = remoteChannelRoutes();
  const dual = activeRoutes.length > 1;
  const currentMeasurements = currentChannelMeasurements();
  const measurements = dual ? activeRoutes.map(route => {
    const measurement = currentMeasurements.find(measurement => measurement.origin === route.origin);
    return { name: route.name, measurement: measurement?.status === "unknown" ? null : measurement };
  }) : [];
  const probing = connectionRecoveryRequest?.scope === privateInputScope(session);
  node.disabled = probing;
  node.setAttribute("aria-busy", String(probing));
  renderConnectionLatency(node, { connected, measured, measurements, dual, probing });
}
function renderConnectionLatency(node, { connected, measured, measurements, dual, probing = false }) {
  const ready = measurement => measurement?.status === "ready"
    && Number.isFinite(measurement.milliseconds) && measurement.milliseconds >= 0;
  if (dual) {
    const rows = measurements.map(({ name, measurement }) => {
      const row = document.createElement("span");
      const status = !connected ? probing ? "正在探测…" : "连接中断" : ready(measurement) ? `${Math.round(measurement.milliseconds)} ms`
        : measurement?.status === "unavailable" ? "暂不可用" : "测量中…";
      row.className = `connection-route ${ready(measurement) ? "online" : measurement?.status === "unavailable" || !connected ? "unavailable" : "pending"}`;
      row.textContent = `${name} ${status}`;
      return row;
    });
    node.replaceChildren(...rows);
  } else {
    node.textContent = !connected ? probing ? "正在探测…" : "连接中断 · 自动重连"
      : measured ? `${Math.round(measured.milliseconds)} ms` : "连接中…";
  }
  const online = dual ? connected && measurements.some(({ measurement }) => ready(measurement)) : !!measured;
  node.classList.toggle("dual-channel", dual);
  node.title = connectionRttDescription;
  node.setAttribute("aria-label", `${dual ? measurements.map((_, index) => node.children[index].textContent).join("；") : node.textContent}。${connectionRttDescription}`);
  node.classList.toggle("online", online);
  node.classList.toggle("pending", connected && !online);
}

export let stream = null;
export function setStream(value) { return stream = value; }
export let sidebarTimer = null, refreshTimer = null;
export function setSidebarTimer(value) { return sidebarTimer = value; }
export function setRefreshTimer(value) { return refreshTimer = value; }
export let streamRetry = null, streamBackoff = 1000;
export function setStreamRetry(value) { return streamRetry = value; }
export function setStreamBackoff(value) { return streamBackoff = value; }
export let connectionPollPending = null;
export function setConnectionPollPending(value) { return connectionPollPending = value; }
export let connectionRecoveryRequest = null;
export function setConnectionRecoveryRequest(value) { return connectionRecoveryRequest = value; }
export let recoveryErrorSilenceDepth = 0;
export let connecting = null;
export function setConnecting(value) { return connecting = value; }
let refreshRequest = null;
export function setRefreshRequest(value) { return refreshRequest = value; }
export let foregroundReadGeneration = 0;
export function setForegroundReadGeneration(value) { return foregroundReadGeneration = value; }
export function withRecoveryErrorSilence(task) {
  recoveryErrorSilenceDepth++;
  return Promise.resolve().then(task).finally(() => { recoveryErrorSilenceDepth--; });
}
export async function refreshAll({ fresh = false } = {}) {
  if (!session || pageLeaving || document.hidden) return;
  const generation = foregroundReadGeneration;
  if (refreshRequest?.deviceId === session.device.id && refreshRequest.generation === generation) {
    if (fresh) refreshRequest.again = true;
    return refreshRequest.promise;
  }
  const deviceId = session?.device.id;
  const request = { deviceId, generation, again: false, promise: null };
  const promise = (async () => {
    do {
      if (generation !== foregroundReadGeneration) return;
      request.again = false;
      const sidebarRead = loadSidebar(null, { fresh }).catch(() => null);
      const id = selectedId || (!creating ? new URLSearchParams(location.hash.slice(1)).get("thread") : null);
      try {
        await (id ? selectedId
          ? reloadThread() : selectThread(id, { closeSidebar: false }) : Promise.resolve());
      } catch (error) {
        if (generation !== foregroundReadGeneration) return;
        if (!request.again || deviceId !== session?.device.id) throw error;
      }
      await sidebarRead;
      fresh = true;
    } while (request.again && generation === foregroundReadGeneration && deviceId === session?.device.id);
  })();
  request.promise = promise; refreshRequest = request;
  try { await promise; }
  finally { if (refreshRequest?.promise === promise) refreshRequest = null; }
}
export function recordRuntimeDiagnostic(name, fields = {}) {
  recordDiagnostic(name, {
    threadId: selectedId, visible: !document.hidden, online: navigator.onLine !== false,
    pageLeaving, connected, readyState: stream?.readyState ?? EventSource.CLOSED,
    generation: foregroundReadGeneration, ...fields
  });
}
