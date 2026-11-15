// 远程接口调用：只读/删除保护、设备授权校验、线路选择与读取优先级。

import { activateRemoteChannels, raceRemoteRead, requestRemote, validateRemoteReceipt } from "../channels.js";
import { receiveDiagnosticRequest } from "../diagnostics.js";
import { beginPwaForegroundWork } from "../pwa.js";
import { requireAuthentication } from "../core/actions.js";
import { session } from "../core/state.js";
import { threadActionReadOnly } from "./session/capabilities.js";
import { currentChannelMeasurements, markMessageChannelUnavailable } from "./session/connection-state.js";
import { threadDeleted, threadDeletionLocked } from "./sidebar/deletion-state.js";

export async function api(path, body, id, signal, { resourcePriority = "foreground", onChannelLatency = null, recoveryProbe = false } = {}) {
  if (path.startsWith("/api/action/") && body?.threadId
      && threadActionReadOnly(path.slice("/api/action/".length), body.threadId))
    throw Object.assign(new Error("此会话仅供查看，不能提交操作。"), { code: 403, notSubmitted: true });
  if (path.startsWith("/api/action/") && path !== "/api/action/deleteThread" && body?.threadId
      && (threadDeletionLocked(body.threadId) || threadDeleted(body.threadId)))
    throw Object.assign(new Error("此会话正在删除或已删除，请先核对删除结果。"), { code: 409 });
  const deviceId = session?.device.id, deviceCreatedAtMs = session?.device.createdAtMs ?? null;
  const encodedBody = body === undefined ? undefined : JSON.stringify(body);
  // 16 KiB/s 包含 base64 上传；12 MiB 图片可等待约 18 分钟，仍有有限上限。
  const actionTimeout = path.startsWith("/api/action/")
    ? Math.min(20 * 60000, 90000 + Math.ceil(new TextEncoder().encode(encodedBody || "").length / 16384) * 1000) : 0;
  const timeout = ["/api/diagnostics", "/api/workspace/context", "/api/workspace/open-in-targets"].includes(path) || path.startsWith("/api/settings/") ? 45000 : path.startsWith("/api/read/") ? 60000
    : path === "/api/session" || path.startsWith("/api/receipts/") ? 15000 : actionTimeout;
  const controller = timeout ? new AbortController() : null;
  const abort = () => controller?.abort(signal?.reason);
  let timedOut = false;
  if (signal?.aborted) abort();
  else if (controller) signal?.addEventListener("abort", abort, { once: true });
  const timer = timeout ? setTimeout(() => {
    timedOut = true;
    controller.abort(new DOMException("远程请求超时", "TimeoutError"));
  }, timeout) : null;
  let releaseForeground = null;
  try {
    if (!["/api/read/models", "/api/read/skills"].includes(path))
      releaseForeground = beginPwaForegroundWork({ priority: resourcePriority });
    const connectionProbe = path === "/api/session" && typeof onChannelLatency === "function";
    const request = connectionProbe ? raceRemoteRead : requestRemote;
    const measurements = currentChannelMeasurements();
    const result = await request(path, {
      method: body === undefined ? "GET" : "POST", credentials: "same-origin",
      headers: { ...(body !== undefined ? { "Content-Type": "application/json" } : {}), ...(id ? { "Idempotency-Key": id } : {}) },
      body: encodedBody, signal: controller?.signal || signal
    }, async response => {
    const result = await response.json().catch(error => {
      if (controller?.signal.aborted || signal?.aborted) throw error;
      if (response.ok) throw Object.assign(new Error(actionTimeout
        ? "未能读取操作回执，请核对原请求结果，勿重复提交。"
        : "响应读取失败，请检查网络后重试"), {
        responseReadFailed: true, ...(actionTimeout ? { outcomeUnknown: true } : {})
      });
      return { message: `HTTP ${response.status}` };
    });
    if (!response.ok) {
      throw Object.assign(new Error(typeof result?.message === "string" && result.message || `HTTP ${response.status}`), {
        code: response.status,
        ...(["/api/read/thread", "/api/read/history"].includes(path) && typeof body?.threadId === "string"
          ? { readThreadId: body.threadId,
            readThreadScope: typeof deviceId === "string" ? JSON.stringify([deviceId, deviceCreatedAtMs]) : null } : {}),
        remoteApplicationError: result?.ok === false && typeof result.message === "string"
          && /(?:^|\/|\+)json(?:\s*;|$)/i.test(response.headers.get("Content-Type") || ""),
        ...(path.startsWith("/api/action/") && result?.notSent === true && result?.outcomeUnknown !== true
          ? { notSubmitted: true } : {}),
        ...(result?.outcomeUnknown === true ? { outcomeUnknown: true } : {})
      });
    }
      if (path.startsWith("/api/receipts/"))
        return validateRemoteReceipt(result, path.slice("/api/receipts/".length));
      return result;
    }, connectionProbe ? onChannelLatency : measurements,
    connectionProbe ? { recoveryProbe } : origin => markMessageChannelUnavailable(origin, measurements));
    if (path === "/api/session" && result?.device?.id) {
      activateRemoteChannels(result.device.id);
      // 冷启动会建立新的事件基线，尚未完成的采集请求由原有状态轮询补达。
      if (result.diagnosticsRequest) setTimeout(() => {
        if (session?.device.id === result.device.id) receiveDiagnosticRequest(result.diagnosticsRequest);
      }, 0);
    }
    return result;
  } catch (error) {
    if (error.code === 401 && !error.secondaryChannel && !path.endsWith("/connect") && session && deviceId === session.device.id)
      requireAuthentication();
    if (error.notSubmitted) throw error;
    if (timedOut && actionTimeout) throw Object.assign(
      new Error("等待上传或回包已超时，正在核对原请求回执；不会自动重发。"), { outcomeUnknown: true, actionTimedOut: true });
    if (timedOut) throw new Error(path.startsWith("/api/read/")
      ? "会话数据读取超时，请检查线路状态后重试；若刚重启 Codex，请等待主界面就绪。"
      : "读取超时，请检查网络后重试");
    throw error;
  } finally {
    clearTimeout(timer);
    if (controller) signal?.removeEventListener("abort", abort);
    releaseForeground?.();
  }
}
export function promoteReadPriority(request, resourcePriority) {
  if (resourcePriority === "background" || request.resourcePriority !== "background") return;
  request.resourcePriority = "foreground";
  if (request.apiStarted) request.releasePriority = beginPwaForegroundWork();
}
