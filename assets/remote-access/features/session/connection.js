// 连接状态：会话探测与连接状态切换、线路配置变化观测；线路延迟测量与连接指示器在 connection-state.js。

import { observeChannelConfiguration, remoteChannelRoutes } from "../../channels.js";
import {
  reconnectChangedAuthorization, renderApprovals, scheduleMessages, updateComposer
} from "../../core/actions.js";
import {
  connected, nativeConnectionVersion, pageLeaving, session, setConnected, setSession, thread
} from "../../core/state.js";
import { recoveryStore } from "../../core/stores.js";
import { api, promoteReadPriority } from "../api.js";
import { recoveryGeneration } from "../composer/drafts.js";
import {
  messageSkillLoadScope, setMessageSkillLoadScope, skillLoading, skillScope
} from "../composer/skill-catalog.js";
import { stopGoalLiveTimer } from "../goal/state.js";
import { backendAvailable, privateInputScope } from "./capabilities.js";
import {
  channelRtts, connectionCycleVersion, connectionLatencyFloor, connectionLatencyRefresh,
  connectionLatencyVersion, connectionPollVersion, connectionRouteSignature, connectionRtt,
  recordRuntimeDiagnostic, refreshAll, renderConnectionIndicator, sessionRecoveryReads,
  sessionRequest, setConnectionCycleVersion, setConnectionLatencyFloor, setConnectionLatencyRefresh,
  setConnectionLatencyVersion, setConnectionRouteSignature, setConnectionRtt, setSessionRequest,
  stream
} from "./connection-state.js";
import { stopRelatedSessionTimers } from "./related-state.js";
import { scheduleRelatedOperationCheck } from "./related.js";

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
  const latencyVersion = setConnectionLatencyVersion(connectionLatencyVersion + 1);
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
        setConnectionRtt({ deviceId: latest.device.id, nativeVersion, connectionVersion, milliseconds: elapsed });
      }
      const result = session && session.device.id === deviceId && nativeVersion !== nativeConnectionVersion
        ? { ...latest, serviceConnected: session.serviceConnected } : latest;
      sessionRecoveryReads.set(result, { expected, requestGeneration: request.recoveryGeneration });
      return result;
    })()
    .finally(() => {
      signal?.removeEventListener("abort", abort);
      request.releasePriority?.();
      if (sessionRequest === request) setSessionRequest(null);
    });
  setSessionRequest(request);
  return request.promise;
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
  if (connected && !value) setConnectionCycleVersion(connectionCycleVersion + 1);
  setConnected(value);
  if (changed) recordRuntimeDiagnostic("connection.state");
  if (!value) stopRelatedSessionTimers();
  else scheduleRelatedOperationCheck();
  if (restored && messageSkillLoadScope && skillScope !== messageSkillLoadScope && !skillLoading) {
    setMessageSkillLoadScope(""); scheduleMessages();
  }
  if (!value) stopGoalLiveTimer();
  if (!value) { setConnectionRtt(null); channelRtts.clear(); }
  else if (connectionRtt && (connectionRtt.deviceId !== session?.device.id
      || connectionRtt.nativeVersion !== nativeConnectionVersion
      || connectionRtt.connectionVersion !== connectionCycleVersion)) setConnectionRtt(null);
  renderConnectionIndicator();
  updateComposer();
  if (thread?.requests?.some(request => request.method === "remote/requestUserInputAsync")) renderApprovals();
}
// 线路配置变化时重置延迟测量；存在多条线路时立即重新探测。
export function installChannelConfigurationObserver() {
  observeChannelConfiguration(configuration => {
    const routes = remoteChannelRoutes();
    const signature = [configuration?.serverId || "", ...routes.map(route => route.origin)].join("\n");
    if (signature === connectionRouteSignature) return;
    setConnectionRouteSignature(signature);
    setConnectionLatencyFloor(setConnectionLatencyVersion(connectionLatencyVersion + 1));
    channelRtts.clear();
    setConnectionRtt(null);
    renderConnectionIndicator();
    clearTimeout(connectionLatencyRefresh);
    if (routes.length > 1) setConnectionLatencyRefresh(setTimeout(() => {
      if (session && !pageLeaving && !document.hidden) void syncConnectionState().catch(() => {});
    }, 0));
  });
}
