/**
 * 真实环境故障的租约控制流。该模块不连接服务、不执行故障，也不把布尔值当作取证。
 * 调用方必须用真实驱动获取租约、读取原始证据并验证；此处保证异常路径仍逐项尝试恢复。
 */
export const REMOTE_ACCESS_ENVIRONMENT_VERSION = "1.0.0";

function invariant(value, message) {
  if (!value) throw new Error(message);
}

export function validateRemoteAccessEnvironmentScope(scope, { runId, origin, tabId, resources = [] } = {}) {
  invariant(typeof runId === "string" && runId.length >= 6, "环境租约缺少本轮 runId");
  invariant(scope && ["dedicated-test-service", "authorized-tab"].includes(scope.kind), "故障仅允许独立测试服务或明确授权的测试 tab");
  invariant(typeof scope.instanceId === "string" && scope.instanceId.includes(runId), "测试实例 identity 必须包含本轮 runId");
  invariant(scope.origin === origin && new URL(scope.origin).origin === scope.origin, "环境租约的 origin 不匹配");
  const servicePort = Number(scope.servicePort || new URL(scope.origin).port);
  invariant(Number.isSafeInteger(servicePort) && servicePort > 0 && servicePort <= 65535, "缺少独立测试服务实际监听端口");
  invariant(servicePort !== 45222 && Number(new URL(scope.origin).port) !== 45222,
    "45222 是受保护的用户服务，禁止作为故障演练对象");
  if (scope.kind === "authorized-tab") {
    invariant(scope.tabId === tabId && tabId != null, "故障租约未绑定当前明确授权的测试 tab");
    invariant(resources.every(value => ["tab-network", "receipts", "side-create-delivery"].includes(value)),
      "tab 租约不能断开全局 CDP、CLI、上游或重启 Codex 进程");
  } else {
    invariant(typeof scope.workspace === "string" && scope.workspace.includes(runId), "独立测试工作区路径必须包含本轮 runId");
    invariant(Number.isSafeInteger(scope.processId) && scope.processId > 0, "独立测试服务缺少准确进程 identity");
    if (resources.includes("native-process"))
      invariant(Number.isSafeInteger(scope.nativeProcessId) && scope.nativeProcessId > 0,
        "原生重启必须指向独立测试实例的准确 nativeProcessId");
  }
  return scope;
}

/**
 * restoreResource 必须按 leaseKey 查询驱动持有的原状态，只恢复本租约拥有的变化。
 * acquire 已产生租约但证据校验失败时 lease 为 null，仍执行所有资源的租约恢复检查；
 * 不能把 null 解释为“把服务开启”，也不能用新的故障请求重试原断开。
 */
export async function withRemoteAccessEnvironmentLease({
  leaseKey, runId, origin, tabId, scope, resources,
  acquire, run, restoreResource, verifyRestored, onEvent = async () => {},
}) {
  validateRemoteAccessEnvironmentScope(scope, { runId, origin, tabId, resources });
  invariant(typeof leaseKey === "string" && leaseKey.includes(runId), "环境租约键不属于本轮");
  invariant(Array.isArray(resources) && resources.length && new Set(resources).size === resources.length,
    "环境租约资源必须明确且不重复");
  invariant([acquire, run, restoreResource, verifyRestored].every(value => typeof value === "function"), "环境租约缺少获取、执行或恢复函数");
  let lease = null, result, primaryError;
  const recoveryErrors = [], recoveryResults = [];
  // 在任何真实副作用之前登记恢复责任；后续 acquire 自身失败也会进入 finally。
  await onEvent({ phase: "registered", leaseKey, scope, resources });
  try {
    const original = await acquire({ leaseKey, runId, scope, resources });
    invariant(original?.leaseKey === leaseKey && original.runId === runId
      && original.originalState && typeof original.originalState === "object"
      && !Array.isArray(original.originalState), "真实环境租约缺少匹配的所有权与原状态");
    invariant(resources.every(resource => Object.hasOwn(original.originalState, resource)),
      "环境租约没有保存全部待故障资源的原状态");
    lease = original;
    await onEvent({ phase: "acquired", leaseKey, originalState: lease.originalState });
    result = await run(lease);
  } catch (error) {
    primaryError = error;
  } finally {
    // 第一项恢复失败也继续后续项。每一项按驱动记录的同一租约恢复，不按 true/false 猜原状态。
    for (const resource of [...resources].reverse()) {
      const request = { leaseKey, runId, scope, resource, lease, acquisitionUnknown: lease === null, restoreOnlyOwnedChanges: true };
      try {
        const value = await restoreResource(request);
        recoveryResults.push({ resource, status: "restored", value });
      } catch (error) {
        recoveryErrors.push(error);
        recoveryResults.push({ resource, status: "failed", reason: error.message });
      }
    }
    try {
      await verifyRestored({ leaseKey, runId, scope, resources, lease, recoveryResults });
    } catch (error) { recoveryErrors.push(error); }
    try {
      await onEvent({ phase: recoveryErrors.length ? "recovery-failed" : "restored", leaseKey, recoveryResults });
    } catch (error) { recoveryErrors.push(error); }
  }
  if (recoveryErrors.length) {
    throw new AggregateError([...(primaryError ? [primaryError] : []), ...recoveryErrors],
      `环境租约 ${leaseKey} 恢复未全部确认：${recoveryResults.filter(value => value.status === "failed").map(value =>
        `${value.resource}: ${value.reason}`).join("；") || recoveryErrors.map(value => value.message).join("；")}`);
  }
  if (primaryError) throw primaryError;
  return result;
}

/** 冷恢复专用：只恢复原 leaseKey，接口没有 acquire 或故障执行入口。 */
export async function restoreRemoteAccessEnvironmentLease({
  leaseKey, runId, origin, tabId, scope, resources, originalState = null,
  restoreResource, verifyRestored, onEvent = async () => {},
}) {
  validateRemoteAccessEnvironmentScope(scope, { runId, origin, tabId, resources });
  invariant(typeof leaseKey === "string" && leaseKey.includes(runId), "恢复租约键不属于本轮");
  invariant(Array.isArray(resources) && resources.length && new Set(resources).size === resources.length, "恢复资源缺失或重复");
  invariant(typeof restoreResource === "function" && typeof verifyRestored === "function", "恢复能力不完整");
  const lease = originalState ? { leaseKey, runId, originalState } : null;
  const recoveryResults = [], errors = [];
  for (const resource of [...resources].reverse()) {
    try {
      const value = await restoreResource({ leaseKey, runId, scope, resource, lease,
        acquisitionUnknown: !lease, restoreOnlyOwnedChanges: true });
      recoveryResults.push({ resource, status: "restored", value });
    } catch (error) {
      errors.push(error); recoveryResults.push({ resource, status: "failed", reason: error.message });
    }
  }
  try { await verifyRestored({ leaseKey, runId, scope, resources, lease, recoveryResults }); }
  catch (error) { errors.push(error); }
  try { await onEvent({ phase: errors.length ? "recovery-failed" : "restored", leaseKey, recoveryResults }); }
  catch (error) { errors.push(error); }
  if (errors.length) throw new AggregateError(errors, `原环境租约恢复未全部确认：${errors.map(value => value.message).join("；")}`);
  return { leaseKey, restored: true, recoveryResults };
}
