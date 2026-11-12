import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { setImmediate as nextTurn } from "node:timers/promises";

const client = await readFile(new URL("../apps/codex-elves-remote/web/app.js", import.meta.url), "utf8");
const pwa = await readFile(new URL("../assets/remote-access/pwa.js", import.meta.url), "utf8");

function connectionPage() {
  const calls = [], nodes = new Map();
  for (const id of ["connection-form", "server-url", "connect", "status", "version"]) {
    const classes = new Set();
    nodes.set(`#${id}`, {
      value: "", textContent: "", disabled: true, events: {}, focused: false, selected: false,
      addEventListener(name, callback) { this.events[name] = callback; },
      focus() { this.focused = true; }, select() { this.selected = true; },
      classList: {
        add: name => classes.add(name), remove: name => classes.delete(name),
        toggle: (name, active) => active ? classes.add(name) : classes.delete(name),
        contains: name => classes.has(name),
      },
    });
  }
  let resolveSettings;
  const settings = new Promise(resolve => { resolveSettings = resolve; });
  const context = vm.createContext({
    document: { querySelector: selector => nodes.get(selector) },
    window: { __TAURI__: { core: { invoke: (command, payload) => {
      if (command === "remote_client_state") return settings;
      calls.push({ command, payload }); return Promise.resolve();
    } } } },
  });
  const ready = vm.runInContext(`(async () => { ${client}\n })()`, context);
  return {
    nodes, calls,
    async load(values) { resolveSettings({ version: "0.7.0", error: null, autoConnect: false, ...values }); await ready; await nextTurn(); },
    input(value) { nodes.get("#server-url").value = value; nodes.get("#server-url").events.input(); },
  };
}

test("首次发现本机服务只预填，已有成功连接则自动打开", async () => {
  const detected = connectionPage();
  await detected.load({ serverUrl: "http://127.0.0.1:49123/", detected: true });
  assert.equal(detected.nodes.get("#server-url").value, "http://127.0.0.1:49123/");
  assert.match(detected.nodes.get("#status").textContent, /已发现本机/);
  assert.equal(detected.calls.length, 0);
  const saved = connectionPage();
  await saved.load({ serverUrl: "https://remote.example/", autoConnect: true });
  assert.equal(saved.calls.length, 1);
  assert.equal(saved.calls[0].command, "connect_remote");
  assert.equal(saved.calls[0].payload.serverUrl, "https://remote.example/");
});

test("服务探测迟到时不覆盖手动地址，也不自动连接旧地址", async () => {
  const page = connectionPage();
  page.input("http://127.0.0.1:49200/");
  await page.load({ serverUrl: "https://previous.example/", autoConnect: true, detected: true });
  assert.equal(page.nodes.get("#server-url").value, "http://127.0.0.1:49200/");
  assert.equal(page.calls.length, 0);
});

test("上次连接失败仍停留在可编辑地址页，不因自动恢复再次陷入错误页", async () => {
  const page = connectionPage();
  await page.load({ serverUrl: "https://bad.example.invalid/", autoConnect: true, error: "无法连接此服务" });
  assert.equal(page.calls.length, 0);
  assert.equal(page.nodes.get("#server-url").selected, true);
  assert.equal(page.nodes.get("#connect").disabled, false);
  assert.equal(page.nodes.get("#status").classList.contains("error"), true);
});

test("Windows 普通 HTTP 使用原生资源缓存，并在前台请求结束后恢复后台检查", async () => {
  const calls = [], windowEvents = new Map(), timers = [];
  let now = 1000;
  const document = {
    visibilityState: "visible", querySelector: () => null, getElementById: () => null,
    addEventListener() {},
  };
  const host = {
    addEventListener: (name, callback) => windowEvents.set(name, callback),
    dispatchEvent() {},
    __TAURI__: { core: { invoke: (command, payload) => {
      calls.push({ command, payload });
      return Promise.resolve({
        status: command === "remote_cache_refresh" ? "waiting" : "ready",
        version: "a".repeat(64), message: "客户端网页资源已缓存",
      });
    } } },
  };
  const context = vm.createContext({
    __CODEX_ELVES_NATIVE_RESOURCES__: { version: 1 }, window: host, document,
    navigator: { onLine: true }, isSecureContext: false,
    location: { protocol: "http:", origin: "http://127.0.0.1:49123", hostname: "127.0.0.1" },
    isAndroidContainer: () => false, Date: class extends Date { static now() { return now; } },
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options?.detail; } },
    setTimeout: (callback, delay) => { const timer = { callback, delay }; timers.push(timer); return timer; },
    clearTimeout: timer => { if (timer) timer.canceled = true; },
  });
  vm.runInContext(pwa.replace(/^import .*;\r?\n/gm, "").replace(/^export /gm, ""), context);
  assert.equal(context.getPwaCacheState().status, "ready");
  windowEvents.get("codex-elves-app-ready")();
  await nextTurn();
  assert.equal(calls[0].command, "remote_cache_status");
  const release = context.beginPwaForegroundWork();
  assert.equal(calls.at(-1).command, "remote_cache_activity");
  assert.equal(calls.at(-1).payload.busy, true);
  release();
  now += 2100;
  timers.findLast(timer => !timer.canceled).callback();
  assert.equal(calls.at(-1).payload.busy, false);
  await context.retryPwaCache();
  assert.equal(context.getPwaCacheState().status, "waiting");
  assert.ok(calls.some(call => call.command === "remote_cache_refresh"));
});
