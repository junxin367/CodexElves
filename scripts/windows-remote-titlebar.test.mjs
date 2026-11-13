import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { setImmediate as nextTurn } from "node:timers/promises";

const base = new URL("../apps/codex-elves-remote/", import.meta.url);
const bridge = await readFile(new URL("web/theme-bridge.js", base), "utf8");

test("外观连续变化合并为最新颜色并串行同步，模式来自实际页面", async () => {
  const frames = [], requests = [], completions = [], variables = new Map([
    ["--bg", "#181818"], ["--text", "#ededed"], ["--line", "#343434"],
  ]);
  const root = { dataset: { theme: "dark" }, addEventListener() {} };
  let changed;
  const host = {
    __TAURI__: { core: { invoke: (command, payload) => {
      requests.push({ command, payload });
      return new Promise(resolve => completions.push(resolve));
    } } },
    addEventListener() {},
  };
  host.top = host;
  const context = vm.createContext({
    window: host, document: { readyState: "complete", documentElement: root, addEventListener() {} },
    MutationObserver: class { constructor(callback) { changed = callback; } observe() {} },
    getComputedStyle: () => ({ getPropertyValue: name => variables.get(name), colorScheme: "light" }),
    matchMedia: () => ({ matches: false, addEventListener() {} }),
    requestAnimationFrame: action => frames.push(action),
  });
  vm.runInContext(bridge, context);
  frames.shift()();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].command, "remote_titlebar_theme");
  assert.equal(requests[0].payload.theme.dark, true, "页面深色优先于系统浅色偏好");
  assert.deepEqual(Array.from(requests[0].payload.theme.background), [24, 24, 24]);
  variables.set("--bg", "#cde"); root.dataset.theme = "light";
  changed(); frames.shift()();
  variables.set("--bg", "#e6f2ff");
  changed(); frames.shift()();
  assert.equal(requests.length, 1, "旧同步完成前只保留待同步的最新颜色");
  completions.shift()();
  await nextTurn();
  assert.equal(requests.length, 2);
  assert.deepEqual(Array.from(requests[1].payload.theme.background), [230, 242, 255]);
  assert.equal(requests[1].payload.theme.dark, false);
  completions.shift()();
  await nextTurn();
  changed(); frames.shift()();
  assert.equal(requests.length, 2, "相同颜色不重复发起本机命令");
});

test("原生应用顶栏不向任何网页授予窗口控制权限，连接设置仍只允许内置连接页", async () => {
  const capability = async file => JSON.parse(await readFile(new URL(`src-tauri/capabilities/${file}.json`, base), "utf8"));
  const [appearance, connection, configuration] = await Promise.all([
    capability("content-appearance"), capability("connection-page"),
    readFile(new URL("src-tauri/tauri.conf.json", base), "utf8").then(JSON.parse),
  ]);
  assert.deepEqual(appearance.webviews, ["remote-content", "remote-content-session-*"]);
  assert.deepEqual(appearance.permissions, ["remote-appearance", "remote-resource-cache", "remote-session-window", "remote-taskbar-badge"]);
  assert.ok(appearance.remote.urls.every(url => /^https?:/.test(url)));
  assert.equal(connection.local, true);
  assert.equal(connection.remote, undefined);
  assert.deepEqual(connection.webviews, ["remote-content"]);
  assert.deepEqual(connection.permissions, ["remote-connection"]);
  assert.deepEqual(configuration.app.security.capabilities, ["connection-page", "content-appearance"]);
  for (const value of [appearance, connection]) {
    assert.ok(value.permissions.every(permission => !permission.startsWith("core:window:")));
  }
});

function simulatedBridge() {
  const frames = [], requests = [], rootEvents = new Map(), documentEvents = new Map();
  const variables = new Map([["--bg", "#181818"], ["--text", "#ededed"], ["--line", "#343434"]]);
  const root = { dataset: { theme: "dark" },
    addEventListener: (name, handler) => rootEvents.set(name, handler) };
  const document = { readyState: "complete", documentElement: root, visibilityState: "visible",
    addEventListener: (name, handler) => documentEvents.set(name, handler) };
  let changed;
  const host = { __TAURI__: { core: { invoke: async (command, payload) => requests.push({ command, payload }) } },
    addEventListener() {} };
  host.top = host;
  vm.runInContext(bridge, vm.createContext({
    window: host, document,
    MutationObserver: class { constructor(callback) { changed = callback; } observe() {} },
    getComputedStyle: () => ({ getPropertyValue: name => variables.get(name), colorScheme: "dark" }),
    matchMedia: () => ({ matches: false, addEventListener() {} }),
    requestAnimationFrame: callback => frames.push(callback),
  }));
  return { frames, requests, variables, document, documentEvents, changed: () => changed(),
    event: (name, propertyName = "--bg", target = root) => rootEvents.get(name)({ target, propertyName }),
    async step() {
      assert.ok(frames.length, "应有待执行的颜色采样帧");
      frames.shift()(); await nextTurn();
    } };
}

test("原生顶栏逐帧接收 CSS 渐变的当前颜色，结束时补齐最后一帧并停止采样", async () => {
  const page = simulatedBridge();
  await page.step();
  page.event("transitionrun");
  page.variables.set("--bg", "rgb(64.5, 80.4, 96.5)");
  await page.step();
  assert.deepEqual(Array.from(page.requests.at(-1).payload.theme.background), [65, 80, 97]);
  const count = page.requests.length;
  await page.step();
  assert.equal(page.requests.length, count, "同色帧不重复通信，但过渡期间仍继续采样");
  assert.equal(page.frames.length, 1);
  page.variables.set("--bg", "#fafafa");
  page.event("transitionend");
  await page.step();
  assert.deepEqual(Array.from(page.requests.at(-1).payload.theme.background), [250, 250, 250]);
  assert.equal(page.frames.length, 0, "渐变结束不遗留永久逐帧轮询");
});

test("连续改色取消旧过渡后继续新过渡，不相关动画和减少动画模式不启动持续采样", async () => {
  const page = simulatedBridge();
  await page.step();
  page.event("transitionrun", "--bg", {});
  page.event("transitionrun", "opacity");
  assert.equal(page.frames.length, 0);
  page.event("transitionrun");
  page.event("transitioncancel");
  page.event("transitionrun");
  await page.step();
  assert.equal(page.frames.length, 1);
  page.event("transitioncancel");
  await page.step();
  assert.equal(page.frames.length, 0);
  page.variables.set("--bg", "#ffffff");
  page.changed();
  await page.step();
  assert.deepEqual(Array.from(page.requests.at(-1).payload.theme.background), [255, 255, 255]);
  assert.equal(page.frames.length, 0, "没有 CSS 过渡时直接同步当前颜色，不伪造额外动画");
});

test("隐藏窗口停止过渡采样，恢复时重新同步实际颜色", async () => {
  const page = simulatedBridge();
  await page.step();
  page.event("transitionrun");
  page.document.visibilityState = "hidden";
  page.documentEvents.get("visibilitychange")();
  await page.step();
  assert.equal(page.frames.length, 0);
  page.variables.set("--bg", "#eeeeee");
  page.document.visibilityState = "visible";
  page.documentEvents.get("visibilitychange")();
  await page.step();
  assert.deepEqual(Array.from(page.requests.at(-1).payload.theme.background), [238, 238, 238]);
  assert.equal(page.frames.length, 0);
});

test("任务栏角标桥串行同步并合并连续数字，读完后能清零且拒绝非法计数", async () => {
  const script = await readFile(new URL("web/unread-bridge.js", base), "utf8");
  const requests = [], completions = [];
  const host = { __TAURI__: { core: { invoke: (command, payload) => {
    requests.push({ command, payload });
    return new Promise(resolve => completions.push(resolve));
  } } } };
  host.top = host;
  vm.runInContext(script, vm.createContext({ window: host }));
  const badge = host.__CODEX_ELVES_UNREAD_BADGE__;
  badge.setCount(1); badge.setCount(2); badge.setCount(3);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].command, "remote_unread_badge");
  completions.shift()(); await nextTurn();
  assert.equal(requests[1].payload.count, 3, "不积压过期的中间计数");
  badge.setCount(0);
  completions.shift()(); await nextTurn();
  assert.equal(requests[2].payload.count, 0);
  completions.shift()(); await nextTurn();
  for (const count of [0, -1, NaN, Infinity, 1.5, 0x100000000]) badge.setCount(count);
  assert.equal(requests.length, 3);
});

test("任务栏桥在一次失败后允许相同计数重试，子框架不能上报应用数字", async () => {
  const script = await readFile(new URL("web/unread-bridge.js", base), "utf8");
  let requests = 0;
  const host = { __TAURI__: { core: { invoke: async () => {
    if (++requests === 1) throw new Error("暂时不可用");
  } } } };
  host.top = host;
  vm.runInContext(script, vm.createContext({ window: host }));
  host.__CODEX_ELVES_UNREAD_BADGE__.setCount(2); await nextTurn();
  host.__CODEX_ELVES_UNREAD_BADGE__.setCount(2); await nextTurn();
  assert.equal(requests, 2);
  const child = { top: host };
  vm.runInContext(script, vm.createContext({ window: child }));
  assert.equal(child.__CODEX_ELVES_UNREAD_BADGE__, undefined);
});
