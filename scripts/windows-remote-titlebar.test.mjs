import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { setImmediate as nextTurn } from "node:timers/promises";

const base = new URL("../apps/codex-elves-remote/", import.meta.url);
const titlebar = await readFile(new URL("web/titlebar.js", base), "utf8");
const bridge = await readFile(new URL("web/theme-bridge.js", base), "utf8");

test("标题栏读取和实时配色乱序到达时，较旧快照不能覆盖新颜色", () => {
  const values = new Map();
  const root = { style: { setProperty: (key, value) => values.set(key, value) } };
  const context = vm.createContext({ document: { documentElement: root } });
  vm.runInContext(titlebar.slice(titlebar.indexOf("let themeVersion ="),
    titlebar.indexOf("// 标题栏启动读取")), context);
  const theme = { background: [24, 24, 24], foreground: [237, 237, 237], border: [54, 54, 54], dark: true };
  context.applyThemeSnapshot({ version: 3, theme });
  context.applyThemeSnapshot({ version: 2, theme: { ...theme, background: [255, 255, 255], dark: false } });
  assert.equal(values.get("--titlebar-background"), "rgb(24,24,24)");
  assert.equal(root.style.colorScheme, "dark");
  context.applyThemeSnapshot({ version: 4, theme: { ...theme, foreground: [256, 0, 0] } });
  assert.equal(values.get("--titlebar-foreground"), "rgb(237,237,237)");
  context.applyThemeSnapshot({ version: 4, theme: { ...theme, background: [230, 242, 255], dark: false } });
  assert.equal(values.get("--titlebar-background"), "rgb(230,242,255)");
  assert.equal(root.style.colorScheme, "light");
});

test("外观连续变化合并为最新颜色并串行同步，模式来自实际页面", async () => {
  const frames = [], requests = [], completions = [], variables = new Map([
    ["--bg", "#181818"], ["--text", "#ededed"], ["--line", "#343434"],
  ]);
  const root = { dataset: { theme: "dark" } };
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
    window: host, document: { readyState: "complete", documentElement: root },
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

test("配色同步权限与连接设置、窗口控制按 WebView 分开", async () => {
  const capability = async file => JSON.parse(await readFile(new URL(`src-tauri/capabilities/${file}.json`, base), "utf8"));
  const [appearance, connection, titlebar] = await Promise.all([
    capability("content-appearance"), capability("connection-page"), capability("titlebar"),
  ]);
  assert.deepEqual(appearance.webviews, ["remote-content"]);
  assert.deepEqual(appearance.permissions, ["remote-appearance"]);
  assert.ok(appearance.remote.urls.every(url => /^https?:/.test(url)));
  assert.equal(connection.local, true);
  assert.equal(connection.remote, undefined);
  assert.deepEqual(connection.webviews, ["remote-content"]);
  assert.deepEqual(connection.permissions, ["remote-connection"]);
  assert.equal(titlebar.local, true);
  assert.equal(titlebar.remote, undefined);
  assert.deepEqual(titlebar.webviews, ["remote-titlebar"]);
  assert.ok(titlebar.permissions.includes("core:window:allow-minimize"));
  assert.ok(titlebar.permissions.includes("core:window:allow-close"));
});
