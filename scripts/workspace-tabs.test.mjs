import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../assets/remote-access/session-tools.js", import.meta.url), "utf8");
const wrapper = source.slice(source.indexOf("export function mountSessionTools("),
  source.indexOf("function mountSessionToolPages(")).replace(/^export /gm, "");

function harness({ desktop = true, android = false, sidePanel = false } = {}) {
  const renderers = [], disk = new Map(), observers = [];
  const document = { activeElement: null, querySelector: () => null };
  class Element {
    constructor(tag, className = "", text = "") {
      this.tag = tag; this.className = className; this.textContent = text;
      this.children = []; this.attributes = new Map(); this.events = new Map();
      this.hidden = false; this.inert = false; this.isConnected = true; this.tabIndex = 0;
      const values = new Map();
      this.style = { values, setProperty: (key, value) => values.set(key, value),
        removeProperty: key => values.delete(key) };
      this.classList = {
        toggle: (name, force) => {
          const names = new Set(this.className.split(" ").filter(Boolean));
          const include = force ?? !names.has(name);
          if (include) names.add(name); else names.delete(name);
          this.className = [...names].join(" "); return include;
        },
        add: (...names) => names.forEach(name => this.classList.toggle(name, true)),
        remove: (...names) => names.forEach(name => this.classList.toggle(name, false)),
        contains: name => this.className.split(" ").includes(name),
      };
    }
    append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
    remove() { this.isConnected = false; if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); }
    setAttribute(key, value) { this.attributes.set(key, String(value)); }
    getAttribute(key) { return this.attributes.get(key) ?? null; }
    addEventListener(name, action) { this.events.set(name, action); }
    removeEventListener(name) { this.events.delete(name); }
    querySelector(selector) {
      const name = selector.replace(":scope > ", "").slice(1);
      return this.children.find(child => child.classList.contains(name)) || null;
    }
    getBoundingClientRect() { return { width: this.width || 0 }; }
    scrollIntoView() {}
    focus() { document.activeElement = this; }
    setPointerCapture(pointer) { this.pointer = pointer; }
    hasPointerCapture(pointer) { return this.pointer === pointer; }
    releasePointerCapture() { this.pointer = null; }
    emit(name, input = {}) {
      const event = { target: this, preventDefault() { this.prevented = true; }, stopPropagation() {}, ...input };
      this.events.get(name)?.(event); return event;
    }
  }
  const node = (tag, className, text) => new Element(tag, className, text);
  const control = (text, label, action, className) => {
    const button = node("button", className, text);
    button.setAttribute("aria-label", label); button.addEventListener("click", action); return button;
  };
  function renderer(options, kind) {
    const value = { options, kind, calls: [], disposed: false,
      open: (...args) => value.calls.push(["open", ...args]),
      openFile: (...args) => value.calls.push(["file", ...args]),
      openChange: (...args) => value.calls.push(["change", ...args]),
      updateContext: () => value.calls.push(["context"]),
      hide: () => value.calls.push(["hide"]),
      close: () => value.calls.push(["close"]),
      dispose: () => { value.disposed = true; } };
    renderers.push(value); return value;
  }
  const media = { matches: desktop, addEventListener(_event, action) { this.change = action; },
    removeEventListener() { this.change = null; } };
  const workspace = node("div", "workspace"), sidebar = node("aside", "sidebar"), conversation = node("main", "conversation");
  workspace.clientWidth = 1400; sidebar.width = 280;
  workspace.append(sidebar, conversation);
  const trigger = node("button"), state = { deviceId: "test-device", cacheScope: "scope-a", threadId: "thread-a" };
  class Observer {
    constructor(action) { this.action = action; observers.push(this); }
    observe() {}
    disconnect() { this.disconnected = true; }
  }
  const context = vm.createContext({
    node, control, document, URL, URLSearchParams,
    window: { matchMedia: () => media, location: { href: "http://127.0.0.1:45222/" } }, ResizeObserver: Observer,
    localStorage: { getItem: key => disk.get(key), setItem: (key, value) => disk.set(key, value) },
    isAndroidContainer: () => android, displayWorkspaceName: value => value,
    displayWorkspacePath: value => value,
    detailKey: file => JSON.stringify([file.repository || "", file.section, file.path]),
    displayedPath: file => `${file.repository ? `${file.repository}/` : ""}${file.path}`,
    sections: [["staged", "已暂存"], ["unstaged", "未暂存"], ["untracked", "未跟踪"]],
    mountSessionToolPages: options => renderer(options, "changes"),
    mountSessionFiles: options => renderer(options, "files"),
  });
  vm.runInContext(wrapper, context);
  const api = context.mountSessionTools({ workspace, trigger, getContext: () => state, sidePanel });
  const dock = workspace.children.at(-1), [separator, toolbar, views] = dock.children;
  const [tablist, add, close] = toolbar.children;
  const selected = () => tablist.children.find(tab => tab.children[0].getAttribute("aria-selected") === "true");
  return { api, state, workspace, sidebar, conversation, trigger, dock, separator, views,
    tablist, add, close, renderers, document, media, disk, observers, selected };
}

test("桌面文件、变更与详情保留多个标签，重复路径复用，切换保留原视图", () => {
  const h = harness();
  h.api.open("files");
  const list = h.renderers[0], originalView = list.options.workspace;
  originalView.readingPosition = 125;
  list.options.embedded.onOpenFile("src/app.js");
  list.options.embedded.onOpenFile("docs/guide.md", 12);
  assert.equal(h.tablist.children.length, 3);
  assert.equal(h.views.children.length, 3);
  assert.equal(h.conversation.inert, false);
  assert.equal(h.sidebar.inert, false);
  assert.equal(h.dock.hidden, false);
  h.api.openFile("src\\app.js");
  assert.equal(h.tablist.children.length, 3, "重复地址不会建立第二份标签");
  assert.equal(h.renderers[1].options.workspace.hidden, false);
  h.tablist.children[0].children[0].emit("click");
  assert.equal(h.renderers[0].options.workspace, originalView);
  assert.equal(originalView.readingPosition, 125);
  assert.equal(originalView.hidden, false);
  h.api.open("changes");
  const changes = h.renderers.at(-1);
  changes.options.embedded.onOpenChange({ repository: "api", path: "src/app.js", section: "unstaged" }, {});
  changes.options.embedded.onOpenChange({ repository: "web", path: "src/app.js", section: "unstaged" }, {});
  changes.options.embedded.onOpenChange({ repository: "web", path: "src/app.js", section: "staged" }, {});
  assert.equal(h.tablist.children.length, 7, "仓库、分区和文件内容各自独立");
  h.close.emit("click");
  assert.equal(h.dock.hidden, true);
  assert.ok(h.renderers.every(value => value.disposed));
  assert.equal(h.document.activeElement, h.trigger);
  h.api.dispose();
});

test("关闭当前标签选择相邻项，键盘切换与删除不触发聊天导航", () => {
  const h = harness();
  h.api.openFile("one.md"); h.api.openFile("two.md"); h.api.openFile("three.md");
  const middle = h.tablist.children[1].children[0];
  h.tablist.emit("keydown", { target: middle, key: "Home" });
  assert.equal(h.selected(), h.tablist.children[0]);
  assert.equal(h.document.activeElement, h.selected().children[0]);
  h.tablist.emit("keydown", { target: h.selected().children[0], key: "ArrowRight" });
  assert.equal(h.selected(), h.tablist.children[1]);
  h.tablist.emit("keydown", { target: h.selected().children[0], key: "Delete" });
  assert.equal(h.tablist.children.length, 2);
  assert.equal(h.renderers[1].disposed, true);
  assert.equal(h.selected(), h.tablist.children[1]);
  h.api.close(); h.api.close();
  assert.equal(h.dock.hidden, true);
  assert.equal(h.workspace.classList.contains("workspace-reader-open"), false);
  h.api.dispose();
});

test("拖动及键盘调宽遵守窗口边界，取消恢复，临时窄窗不覆盖已选宽度", () => {
  const h = harness();
  h.api.open("files");
  const width = () => Number(h.separator.getAttribute("aria-valuenow"));
  assert.equal(width(), 480);
  h.separator.emit("pointerdown", { button: 0, isPrimary: true, pointerId: 1, clientX: 920 });
  h.separator.emit("pointermove", { pointerId: 1, clientX: 800 });
  assert.equal(width(), 600);
  h.separator.emit("pointerup", { pointerId: 1 });
  assert.equal([...h.disk.values()][0], "600");
  h.separator.emit("pointerdown", { button: 0, isPrimary: true, pointerId: 2, clientX: 800 });
  h.separator.emit("pointermove", { pointerId: 2, clientX: 10000 });
  assert.equal(width(), 320);
  h.separator.emit("pointercancel");
  assert.equal(width(), 600);
  h.separator.emit("keydown", { key: "ArrowLeft" });
  assert.equal(width(), 620);
  h.workspace.clientWidth = 900;
  h.observers[0].action();
  assert.equal(width(), 310);
  assert.equal([...h.disk.values()][0], "620");
  h.workspace.clientWidth = 1400;
  h.observers[0].action();
  assert.equal(width(), 620);
  h.api.dispose();
  assert.equal(h.workspace.classList.contains("workspace-reader-resizing"), false);
  assert.equal(h.observers[0].disconnected, true);
});

test("切换会话或授权释放所有阅读标签，旧标签回调不能在新目标打开文件", () => {
  const h = harness();
  h.api.open("files"); h.api.openFile("old.md");
  const stale = h.renderers[0].options.embedded;
  h.state.threadId = "thread-b";
  h.api.updateContext();
  assert.equal(h.tablist.children.length, 0);
  assert.ok(h.renderers.every(value => value.disposed));
  stale.onOpenFile("late.md");
  stale.onOpenChange({ path: "late.md", section: "unstaged" }, {});
  assert.equal(h.tablist.children.length, 0);
  h.api.openFile("new.md");
  h.state.cacheScope = "scope-b"; h.api.updateContext();
  assert.equal(h.tablist.children.length, 0);
  h.api.dispose();
});

test("窄屏和 Android 沿用原工具页，桌面标签缩窄后仍可恢复", () => {
  const h = harness();
  h.api.openFile("keep.md");
  h.media.matches = false; h.media.change();
  assert.equal(h.dock.hidden, true);
  h.api.open("files");
  assert.equal(h.renderers.at(-1).options.embedded, undefined);
  assert.equal(h.renderers.at(-1).options.isAvailable(), true);
  h.media.matches = true; h.media.change();
  assert.equal(h.dock.hidden, false);
  assert.equal(h.tablist.children.length, 1);
  assert.equal(h.renderers.at(-1).calls.at(-1)[0], "hide");
  h.api.dispose();
  const android = harness({ android: true });
  android.api.open("changes");
  assert.equal(android.dock.hidden, true);
  assert.equal(android.renderers[0].options.embedded, undefined);
  android.api.dispose();
});

test("PC 侧边会话与文件共用标签，重复打开不重载，主聊天和滚动状态独立", () => {
  const h = harness();
  h.api.openFile("notes.md");
  assert.equal(h.api.openSideThread({ id: "side-a", title: "侧边 A" }), true);
  const sideView = h.views.children[1], frame = sideView.children[0];
  assert.equal(frame.tag, "iframe");
  assert.equal(new URL(frame.src).pathname, "/side-thread");
  assert.equal(new URL(frame.src).hash, "#thread=side-a");
  assert.equal(h.state.threadId, "thread-a");
  assert.equal(h.conversation.inert, false);
  frame.savedDraft = "侧边独立输入";
  h.tablist.children[0].children[0].emit("click");
  assert.equal(sideView.hidden, true);
  h.api.openSideThread({ id: "side-a", title: "侧边 A" });
  assert.equal(h.tablist.children.length, 2);
  assert.equal(h.views.children[1].children[0], frame);
  assert.equal(frame.savedDraft, "侧边独立输入");
  assert.equal(sideView.hidden, false);
  h.api.openSideThread({ id: "side-b", title: "侧边 B" });
  assert.equal(h.tablist.children.length, 3);
  h.api.close();
  assert.equal(h.tablist.children.length, 2);
  h.state.threadId = "thread-b"; h.api.updateContext();
  assert.equal(h.tablist.children.length, 0);
  assert.equal(frame.isConnected, false);
  h.api.dispose();
});

test("侧边标签向独立页面同步显示状态，隐藏标签不会继续作为可见页面", () => {
  const h = harness();
  h.api.openSideThread({ id: "side-a" });
  const frame = h.views.children[0].children[0], visibility = [];
  frame.contentWindow = { postMessage: (message, origin) => visibility.push({ ...message, origin }) };
  frame.emit("load");
  assert.deepEqual(visibility.at(-1), { type: "codex-elves-side-panel-visibility", visible: true,
    origin: "http://127.0.0.1:45222" });
  h.api.open("files");
  assert.equal(visibility.at(-1).visible, false);
  h.api.openSideThread({ id: "side-a" });
  assert.equal(visibility.at(-1).visible, true);
  h.media.matches = false; h.media.change();
  assert.equal(visibility.at(-1).visible, false);
  h.api.dispose();
  assert.equal(frame.events.has("load"), false);
});

test("手机、Android 和侧边页面本身不嵌套会话标签，非法目标不创建页面", () => {
  for (const options of [{ desktop: false }, { android: true }, { sidePanel: true }]) {
    const h = harness(options);
    assert.equal(h.api.openSideThread({ id: "side-a" }), false);
    assert.equal(h.tablist.children.length, 0);
    h.api.dispose();
  }
  const h = harness();
  assert.equal(h.api.openSideThread({ id: "thread-a" }), false);
  assert.equal(h.api.openSideThread({ id: "../outside" }), false);
  assert.equal(h.tablist.children.length, 0);
  h.api.dispose();
});
