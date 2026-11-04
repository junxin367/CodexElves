import test from "node:test";
import assert from "node:assert/strict";
import { withRemoteAccessEnvironmentLease, restoreRemoteAccessEnvironmentLease } from "./remote-access-e2e-environment.mjs";
import { reloadRemoteAccessPage } from "./remote-access-e2e.mjs";

test("页面重载先监听导航，命令返回后仍等待新文档", async () => {
  const calls = [];
  let releaseNavigation;
  const navigated = new Promise(resolve => { releaseNavigation = resolve; });
  const tab = {
    reload: async () => { calls.push("reload-command-returned"); },
    playwright: { expectNavigation: async (action, options) => {
      calls.push("navigation-registered");
      assert.equal(options.waitUntil, "domcontentloaded");
      await action();
      await navigated;
      calls.push("new-document-ready");
    } },
  };
  let complete = false;
  const operation = reloadRemoteAccessPage(tab).then(() => { complete = true; });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ["navigation-registered", "reload-command-returned"]);
  assert.equal(complete, false);
  releaseNavigation();
  await operation;
  assert.equal(complete, true);
  assert.equal(calls.at(-1), "new-document-ready");
});

test("页面导航失败不能只凭 reload 命令成功通过，也不自动重复导航", async () => {
  let reloads = 0;
  const tab = {
    reload: async () => { reloads++; },
    playwright: { expectNavigation: async action => {
      await action();
      throw new Error("new document did not load");
    } },
  };
  await assert.rejects(reloadRemoteAccessPage(tab), /new document did not load/);
  assert.equal(reloads, 1);
});

test("没有导航观察能力时在 reload 前明确阻塞", async () => {
  let reloads = 0;
  await assert.rejects(reloadRemoteAccessPage({ reload: async () => { reloads++; }, playwright: {} }),
    /缺少新文档导航等待能力/);
  assert.equal(reloads, 0);
});

const runId = "lease-regression";
function fixture(overrides = {}) {
  const state = { cdp: true, service: false };
  const originalState = { ...state }, calls = [];
  const scope = { kind: "dedicated-test-service", instanceId: `service-${runId}`, origin: "http://127.0.0.1:46171",
    servicePort: 46171, processId: 1234, workspace: `E:/temp/${runId}` };
  const lease = { leaseKey: `${runId}:case`, runId, originalState };
  return {
    state, calls, lease,
    options: {
      leaseKey: lease.leaseKey, runId, origin: scope.origin, scope, resources: ["cdp", "service"],
      acquire: async () => { calls.push("acquire"); return lease; },
      run: async () => { state.cdp = false; throw new Error("真实断开完成，但原始证据校验失败"); },
      restoreResource: async ({ leaseKey, resource, restoreOnlyOwnedChanges }) => {
        assert.equal(leaseKey, lease.leaseKey); assert.equal(restoreOnlyOwnedChanges, true);
        calls.push(`restore:${resource}`); state[resource] = originalState[resource];
      },
      verifyRestored: async () => { calls.push("verify"); assert.deepEqual(state, originalState); },
      ...overrides,
    },
  };
}

test("perform 已改变状态而证据失败时，仍恢复全部资源且保留原失败", async () => {
  const value = fixture();
  await assert.rejects(withRemoteAccessEnvironmentLease(value.options), /原始证据校验失败/);
  assert.deepEqual(value.state, value.lease.originalState);
  assert.deepEqual(value.calls, ["acquire", "restore:service", "restore:cdp", "verify"]);
});

test("第一项恢复失败不会跳过其他资源，最终仍验证并报告恢复失败", async () => {
  const value = fixture();
  value.options.restoreResource = async ({ resource }) => {
    value.calls.push(`restore:${resource}`);
    if (resource === "service") throw new Error("service 的恢复取证失败");
    value.state[resource] = value.lease.originalState[resource];
  };
  await assert.rejects(withRemoteAccessEnvironmentLease(value.options), error =>
    error instanceof AggregateError && error.errors.length === 2 && /service/.test(error.message));
  assert.equal(value.state.cdp, true);
  assert.deepEqual(value.calls, ["acquire", "restore:service", "restore:cdp", "verify"]);
});

test("acquire 已登记驱动租约但取证失败，也按稳定 leaseKey 查询恢复，不能重放 acquire", async () => {
  const value = fixture();
  value.options.acquire = async () => { value.calls.push("acquire"); value.state.cdp = false; throw new Error("lease 原始证据失败"); };
  const restore = value.options.restoreResource;
  value.options.restoreResource = async request => {
    assert.equal(request.lease, null); assert.equal(request.acquisitionUnknown, true);
    return restore(request);
  };
  await assert.rejects(withRemoteAccessEnvironmentLease(value.options), /lease 原始证据失败/);
  assert.deepEqual(value.state, value.lease.originalState);
  assert.equal(value.calls.filter(value => value === "acquire").length, 1);
});

test("原来关闭的 CLI 服务保持关闭，恢复不会盲目设为 true", async () => {
  const value = fixture({ run: async () => "完成" });
  assert.equal(await withRemoteAccessEnvironmentLease(value.options), "完成");
  assert.equal(value.state.service, false);
});

test("非隔离实例或 45222 用户服务在调用 acquire 之前阻塞", async () => {
  for (const patch of [{ instanceId: "user-service" }, { servicePort: 45222 }, { workspace: "E:/user-workspace" }]) {
    const value = fixture();
    value.options.scope = { ...value.options.scope, ...patch };
    await assert.rejects(withRemoteAccessEnvironmentLease(value.options));
    assert.deepEqual(value.calls, []);
  }
});

test("tab 租约不能重启原生或断开全局 CDP", async () => {
  const value = fixture();
  value.options.tabId = "authorized-tab";
  value.options.scope = { ...value.options.scope, kind: "authorized-tab", tabId: "authorized-tab" };
  await assert.rejects(withRemoteAccessEnvironmentLease(value.options), /tab 租约不能/);
  assert.deepEqual(value.calls, []);
});

test("冷恢复只查询原租约并恢复，未知获取也不会执行 acquire 或原故障", async () => {
  const value = fixture();
  value.state.cdp = false;
  const result = await restoreRemoteAccessEnvironmentLease({
    ...value.options, originalState: null,
    restoreResource: async request => {
      assert.equal(request.acquisitionUnknown, true);
      return value.options.restoreResource(request);
    },
  });
  assert.equal(result.restored, true);
  assert.deepEqual(value.state, value.lease.originalState);
  assert.deepEqual(value.calls, ["restore:service", "restore:cdp", "verify"]);
});
