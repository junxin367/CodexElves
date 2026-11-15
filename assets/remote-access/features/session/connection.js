// 连接状态：会话探测、线路延迟测量与连接指示器。

import { observeChannelConfiguration, remoteChannelRoutes, setRemoteChannelMeasurements } from "../../channels.js";
import { $ } from "../../ui/dom.js";
import {
  connected, nativeConnectionVersion, pageLeaving, session, setConnected, setSession, thread
} from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { api, promoteReadPriority } from "../api.js";
import { renderApprovals } from "../approvals.js";
import { updateComposer } from "../composer/composer.js";
import { recoveryGeneration } from "../composer/drafts.js";
import { messageSkillLoadScope, setMessageSkillLoadScope, skillLoading, skillScope } from "../composer/skills.js";
import { scheduleMessages } from "../conversation/history-view.js";
import { stopGoalLiveTimer } from "../goal/status.js";
import { backendAvailable, privateInputScope } from "./capabilities.js";
import {
  connectionRecoveryRequest, reconnectChangedAuthorization, recordRuntimeDiagnostic, refreshAll,
  stream
} from "./lifecycle.js";
import { scheduleRelatedOperationCheck, stopRelatedSessionTimers } from "./related.js";

export let sessionRequest = null, connectionPollVersion = 0;
export function setConnectionPollVersion(value) { return connectionPollVersion = value; }
export const sessionRecoveryReads = new WeakMap();
export let connectionCycleVersion = 0, connectionRtt = null;
let connectionLatencyVersion = 0, connectionLatencyFloor = 0, connectionRouteSignature = "", connectionLatencyRefresh = null;
const channelRtts = new Map();
export const connectionRttDescription = "点击探测连接并刷新会话。网页到接入服务的 /api/session HTTP 完整往返时长；存在两条已授权线路时分别测量，不代表模型响应耗时或端到端原生执行延迟。消息使用此处可用线路中延迟最低的一路发送，发出后不自动换路重发。";
export function readConnectionSession(signal, recoveryObservation = undefined, { resourcePriority = "foreground", recoveryProbe = false } = {}) {
  const deviceId = session?.device.id;
  const source = stream;
  const nativeVersion = nativeConnectionVersion;
  const connectionVersion = connectionCycleVersion;
  const pollVersion = connectionPollVersion;
  if (sessionRequest && sessionRequest.deviceId === deviceId && sessionRequest.nativeVersion === nativeVersion
      && sessionRequest.connectionVersion === connectionVersion && sessionRequest.source === source
      && sessionRequest.pollVersion === pollVersion) {
    promoteReadPriority(sessionRequest, resourcePriority);
    return sessionRequest.promise;
  }
  const controller = new AbortController();
  const latencyVersion = ++connectionLatencyVersion;
  const abort = () => controller.abort();
  if (signal?.aborted) abort();
  else signal?.addEventListener("abort", abort, { once: true });
  const request = { deviceId, source, nativeVersion, connectionVersion, pollVersion, controller,
    recoveryGeneration, resourcePriority, apiStarted: false, releasePriority: null, promise: null };
  request.promise = (async () => {
      let expected = recoveryObservation;
      if (expected === undefined) {
        try { expected = await recoveryStore.observe(); }
        catch { expected = null; }
      }
      if (controller.signal.aborted) throw new Error("连接已取消，请重试。");
      const startedAt = performance.now();
      request.apiStarted = true;
      const latest = await api("/api/session", undefined, undefined, controller.signal,
        {
          resourcePriority: request.resourcePriority,
          recoveryProbe,
          onChannelLatency: measurement => {
            if (latencyVersion < connectionLatencyFloor || controller.signal.aborted || pageLeaving
                || session?.device.id !== deviceId || stream !== source || pollVersion !== connectionPollVersion
                || nativeVersion !== nativeConnectionVersion || connectionVersion !== connectionCycleVersion) return;
            if ((channelRtts.get(measurement.origin)?.latencyVersion ?? 0) > latencyVersion) return;
            channelRtts.set(measurement.origin, { ...measurement, deviceId, nativeVersion, connectionVersion, latencyVersion });
            renderConnectionIndicator();
          }
        });
      const elapsed = performance.now() - startedAt;
      const sameDevice = deviceId === undefined ? !session : session?.device.id === deviceId;
      if (sessionRequest === request && !controller.signal.aborted && sameDevice
          && stream === source && pollVersion === connectionPollVersion
          && nativeVersion === nativeConnectionVersion && connectionVersion === connectionCycleVersion
          && typeof latest.device?.id === "string"
          && Number.isFinite(elapsed) && elapsed >= 0) {
        connectionRtt = { deviceId: latest.device.id, nativeVersion, connectionVersion, milliseconds: elapsed };
      }
      const result = session && session.device.id === deviceId && nativeVersion !== nativeConnectionVersion
        ? { ...latest, serviceConnected: session.serviceConnected } : latest;
      sessionRecoveryReads.set(result, { expected, requestGeneration: request.recoveryGeneration });
      return result;
    })()
    .finally(() => {
      signal?.removeEventListener("abort", abort);
      request.releasePriority?.();
      if (sessionRequest === request) sessionRequest = null;
    });
  sessionRequest = request;
  return request.promise;
}
export function cancelConnectionProbe() {
  connectionLatencyFloor = ++connectionLatencyVersion;
  channelRtts.clear();
  sessionRequest?.controller.abort();
  sessionRequest = null;
  connectionRtt = null;
}
export async function syncConnectionState(source = stream, { recoveryProbe = false } = {}) {
  const deviceId = session?.device.id;
  const nativeVersion = nativeConnectionVersion, connectionVersion = connectionCycleVersion;
  const pollVersion = connectionPollVersion;
  const isCurrent = () => !pageLeaving && stream === source && session?.device.id === deviceId
    && nativeVersion === nativeConnectionVersion && connectionVersion === connectionCycleVersion
    && pollVersion === connectionPollVersion;
  let latest;
  try {
    latest = await readConnectionSession(undefined, undefined, { recoveryProbe });
  } catch (error) {
    // 主动恢复已退役旧探测；其迟到错误不能再标记断线或让旧 onopen 弹出错误。
    if (!isCurrent()) return false;
    setConnection(false);
    throw error;
  }
  if (!isCurrent()) return false;
  if (privateInputScope(latest) !== privateInputScope(session)) {
    await reconnectChangedAuthorization();
    return false;
  }
  setSession(latest);
  setConnection(backendAvailable(latest) && source?.readyState === EventSource.OPEN);
  renderApprovals();
  return true;
}
export async function refreshConnectedSession(source = stream) {
  // 事件通道可能先于网页请求线路恢复；等待测速清除旧退避后再补读。
  const applied = await syncConnectionState(source);
  if (!applied) return false;
  await refreshAll({ fresh: true });
  return true;
}
export function setConnection(value) {
  const changed = connected !== value;
  const restored = !connected && value;
  if (connected && !value) ++connectionCycleVersion;
  setConnected(value);
  if (changed) recordRuntimeDiagnostic("connection.state");
  if (!value) stopRelatedSessionTimers();
  else scheduleRelatedOperationCheck();
  if (restored && messageSkillLoadScope && skillScope !== messageSkillLoadScope && !skillLoading) {
    setMessageSkillLoadScope(""); scheduleMessages();
  }
  if (!value) stopGoalLiveTimer();
  if (!value) { connectionRtt = null; channelRtts.clear(); }
  else if (connectionRtt && (connectionRtt.deviceId !== session?.device.id
      || connectionRtt.nativeVersion !== nativeConnectionVersion
      || connectionRtt.connectionVersion !== connectionCycleVersion)) connectionRtt = null;
  renderConnectionIndicator();
  updateComposer();
  if (thread?.requests?.some(request => request.method === "remote/requestUserInputAsync")) renderApprovals();
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
// 线路配置变化时重置延迟测量；存在多条线路时立即重新探测。
export function installChannelConfigurationObserver() {
  observeChannelConfiguration(configuration => {
    const routes = remoteChannelRoutes();
    const signature = [configuration?.serverId || "", ...routes.map(route => route.origin)].join("\n");
    if (signature === connectionRouteSignature) return;
    connectionRouteSignature = signature;
    connectionLatencyFloor = ++connectionLatencyVersion;
    channelRtts.clear();
    connectionRtt = null;
    renderConnectionIndicator();
    clearTimeout(connectionLatencyRefresh);
    if (routes.length > 1) connectionLatencyRefresh = setTimeout(() => {
      if (session && !pageLeaving && !document.hidden) void syncConnectionState().catch(() => {});
    }, 0);
  });
}
