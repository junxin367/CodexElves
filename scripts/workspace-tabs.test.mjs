import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../assets/remote-access/session-tools.js", import.meta.url), "utf8");
const wrapper = source.slice(source.indexOf("export function mountSessionTools("),
  source.indexOf("function mountSessionToolPages(")).replace(/^export /gm, "");

function harness({ desktop = true, android = false, sidePanel = false, createSideThread, openImagePreview, openVideoPreview } = {}) {
  const renderers = [], disk = new Map(), observers = [], messageListeners = new Set();
  const document = { activeElement: null, querySelector: () => null };
  class Element {
    constructor(tag, className = "", text = "") {
      this.tag = tag; this.className = className; this.textContent = text;
      this.children = []; this.attributes = new Map(); this.events = new Map();
      this.hidden = false; this.inert = false; this.isConnected = true; this.tabIndex = 0;
      this.scrollLeft = 0;
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
    get parentElement() { return this.parent || null; }
    append(...children) {
      for (const child of children) {
        if (child.parent) child.parent.children = child.parent.children.filter(value => value !== child);
        child.parent = this; this.children.push(child);
      }
    }
    after(child) {
      if (child.parent) child.parent.children = child.parent.children.filter(value => value !== child);
      child.parent = this.parent;
      this.parent.children.splice(this.parent.children.indexOf(this) + 1, 0, child);
    }
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
  const header = node("header", "conversation-header");
  header.append(trigger); conversation.append(header);
  class Observer {
    constructor(action) { this.action = action; observers.push(this); }
    observe() {}
    disconnect() { this.disconnected = true; }
  }
  const context = vm.createContext({
    node, control, document, URL, URLSearchParams,
    window: { matchMedia: () => media, location: { href: "http://127.0.0.1:45222/" },
      addEventListener: (name, callback) => { if (name === "message") messageListeners.add(callback); },
      removeEventListener: (name, callback) => { if (name === "message") messageListeners.delete(callback); } },
    ResizeObserver: Observer,
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
  const api = context.mountSessionTools({ workspace, trigger, getContext: () => state, sidePanel,
    createSideThread, openImagePreview, openVideoPreview });
  const dock = workspace.children.at(-1), [separator, toolbar, views, empty] = dock.children;
  const tablist = toolbar.children[0], toggle = header.children[1];
  const visibleTabs = () => tablist.children.filter(tab => !tab.hidden);
  const visibleViews = () => views.children.filter(view => !view.hidden);
  const selected = () => visibleTabs().find(tab => tab.children[0].getAttribute("aria-selected") === "true");
  return { api, state, workspace, sidebar, conversation, trigger, dock, separator, views,
    tablist, header, toolbar, toggle, empty, renderers, document, media, disk, observers, selected, visibleTabs, visibleViews, messageListeners,
    message: event => { for (const callback of messageListeners) callback(event); } };
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
  const calls = h.renderers.map(value => value.calls.length);
  h.toggle.emit("click");
  assert.equal(h.dock.hidden, true);
  assert.ok(h.renderers.every(value => !value.disposed));
  assert.equal(h.document.activeElement, h.toggle);
  assert.equal(h.tablist.children.length, 7);
  h.toggle.emit("click");
  assert.equal(h.dock.hidden, false);
  assert.equal(h.renderers[0].options.workspace, originalView);
  assert.equal(originalView.readingPosition, 125);
  assert.deepEqual(h.renderers.map(value => value.calls.length), calls, "重新展开不能重新读取或初始化页面");
  h.api.dispose();
  assert.ok(h.renderers.every(value => value.disposed));
});

test("关闭当前标签选择相邻项，键盘与中键关闭不触发聊天导航", () => {
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
  h.tablist.children[1].children[1].emit("click");
  h.tablist.children[0].children[1].emit("click");
  assert.equal(h.dock.hidden, true);
  assert.equal(h.workspace.classList.contains("workspace-reader-open"), false);
  h.api.openFile("one.md"); h.api.openFile("two.md"); h.api.openFile("three.md");
  const last = h.selected(), first = h.tablist.children[0];
  assert.equal(first.emit("mousedown", { button: 1 }).prevented, true, "中键按下阻止浏览器自动滚屏");
  assert.equal(h.tablist.children.length, 3, "按下时不提前关闭，等待完整中键点击");
  first.emit("auxclick", { button: 0 }); first.emit("auxclick", { button: 2 });
  assert.equal(h.tablist.children.length, 3, "左键和右键不触发中键关闭");
  assert.equal(first.emit("auxclick", { button: 1 }).prevented, true);
  assert.equal(h.tablist.children.length, 2);
  assert.equal(h.selected(), last, "关闭非当前标签不改变选中项");
  assert.equal(h.renderers[3].disposed, true);
  last.emit("auxclick", { button: 1 });
  assert.equal(h.selected(), h.tablist.children[0], "关闭当前标签后选择相邻项");
  h.selected().emit("auxclick", { button: 1 });
  assert.equal(h.tablist.children.length, 0);
  assert.equal(h.dock.hidden, true, "中键关闭最后一个标签后收起面板");
  h.api.dispose();
});

test("标签栏将鼠标竖向滚轮和触控板横向滚动映射到标签，保留浏览器缩放", () => {
  const h = harness(), bar = h.tablist;
  h.api.openFile("one.md");
  bar.clientWidth = 300; bar.scrollWidth = 1000;
  const wheel = values => bar.emit("wheel", { deltaX: 0, deltaY: 0, deltaMode: 0, ...values });
  assert.equal(wheel({ deltaY: 120 }).prevented, true);
  assert.equal(bar.scrollLeft, 120);
  wheel({ deltaX: 80, deltaY: 5 });
  assert.equal(bar.scrollLeft, 200);
  wheel({ deltaY: -3, deltaMode: 1 });
  assert.equal(bar.scrollLeft, 152);
  wheel({ deltaY: 1, deltaMode: 2 });
  assert.equal(bar.scrollLeft, 452);
  wheel({ deltaY: 1000 });
  assert.equal(bar.scrollLeft, 700);
  assert.equal(wheel({ deltaY: 100 }).prevented, true, "到达边缘后也不让滚轮误滚主聊天");
  assert.equal(wheel({ deltaY: -100, ctrlKey: true }).prevented, undefined);
  assert.equal(bar.scrollLeft, 700);
  wheel({ deltaY: -1000 });
  assert.equal(bar.scrollLeft, 0);
  bar.scrollWidth = bar.clientWidth;
  assert.equal(wheel({ deltaY: 50 }).prevented, undefined);
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

test("切换会话保留各自标签与内容，后台回调不跨目标，授权变化释放全部页面", () => {
  const h = harness();
  h.api.open("files"); h.api.openFile("old.md");
  const original = h.renderers[1], stale = h.renderers[0].options.embedded;
  const oldTab = h.selected();
  original.options.workspace.readingPosition = 125;
  h.state.threadId = "thread-b";
  h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0);
  assert.equal(h.visibleViews().length, 0);
  assert.equal(h.dock.hidden, true);
  assert.ok(h.renderers.every(value => !value.disposed));
  assert.equal(original.options.getContext().threadId, "thread-a", "后台读取固定在原会话");
  stale.onOpenFile("late.md");
  stale.onOpenChange({ path: "late.md", section: "unstaged" }, {});
  assert.equal(h.visibleTabs().length, 0);
  h.api.openFile("old.md");
  assert.equal(h.visibleTabs().length, 1, "不同会话的同名文件分别创建标签");
  assert.equal(h.renderers[2].options.getContext().threadId, "thread-b");
  oldTab.children[0].emit("click"); oldTab.children[1].emit("click"); oldTab.emit("auxclick", { button: 1 });
  assert.equal(h.visibleTabs().length, 1, "隐藏旧标签的按钮不能选中或关闭新会话的同名标签");
  const calls = original.calls.length;
  h.state.threadId = "thread-a"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 2);
  assert.equal(h.selected(), oldTab);
  assert.equal(h.visibleViews()[0], original.options.workspace);
  assert.equal(original.options.workspace.readingPosition, 125);
  assert.equal(original.calls.length, calls, "切回不能重新读取、初始化或关闭已打开内容");
  stale.onOpenFile("added.md");
  assert.equal(h.visibleTabs().length, 3, "恢复后的原页面可以继续打开所属文件");
  h.state.cacheScope = "scope-b"; h.api.updateContext();
  assert.equal(h.tablist.children.length, 0);
  assert.ok(h.renderers.every(value => value.disposed));
  assert.equal(original.options.getContext().threadId, undefined, "旧授权不能继续提供读取身份");
  stale.onOpenFile("unauthorized.md");
  assert.equal(h.tablist.children.length, 0);
  h.state.cacheScope = "scope-a"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0, "切回旧授权不能恢复已清理的标签");
  h.api.openFile("device.md");
  h.state.deviceId = "other-device"; h.api.updateContext();
  assert.equal(h.tablist.children.length, 0);
  assert.ok(h.renderers.every(value => value.disposed));
  h.api.dispose();
});

test("各会话分别恢复面板显隐、选中项、调整宽度及标签滚动，新建项目也彼此独立", () => {
  const h = harness(), width = () => Number(h.separator.getAttribute("aria-valuenow"));
  h.api.openFile("one.md"); h.api.openFile("two.md");
  const first = h.tablist.children[0];
  first.children[0].emit("click");
  h.separator.emit("keydown", { key: "End" });
  const firstWidth = width();
  h.tablist.scrollLeft = 130;
  h.toggle.emit("click");
  h.state.threadId = "thread-b"; h.api.updateContext();
  h.api.openFile("other.md");
  h.separator.emit("keydown", { key: "Home" });
  const otherWidth = width();
  h.tablist.scrollLeft = 45;
  h.state.threadId = "thread-a"; h.api.updateContext();
  assert.equal(h.dock.hidden, true, "会话 A 保留关闭状态，不能被会话 B 的展开状态覆盖");
  assert.equal(h.selected(), first);
  h.toggle.emit("click");
  assert.equal(width(), firstWidth);
  assert.equal(h.tablist.scrollLeft, 130);
  h.state.threadId = "thread-b"; h.api.updateContext();
  assert.equal(h.dock.hidden, false);
  assert.equal(width(), otherWidth);
  assert.equal(h.tablist.scrollLeft, 45);
  h.state.threadId = null; h.state.creating = true; h.state.projectId = "project-a";
  h.api.updateContext(); h.api.openFile("draft.md");
  const projectView = h.visibleViews()[0];
  h.state.projectId = "project-b"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0);
  h.api.open("changes");
  h.state.projectId = "project-a"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 1);
  assert.equal(h.visibleViews()[0], projectView);
  h.state.threadId = "thread-a"; h.state.creating = false; h.api.updateContext();
  assert.equal(h.selected(), first);
  assert.equal(width(), firstWidth);
  h.api.dispose();
  assert.ok(h.renderers.every(value => value.disposed));
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
  assert.equal(android.toggle.hidden, true);
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
  assert.equal(h.tablist.children.length, 3);
  assert.equal(frame.isConnected, true);
  h.toggle.emit("click");
  assert.equal(h.views.children[1].children[0], frame);
  assert.equal(frame.savedDraft, "侧边独立输入");
  h.state.threadId = "thread-b"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0);
  assert.equal(sideView.hidden, true);
  assert.equal(frame.isConnected, true, "切换主会话不能卸载或移动侧边 iframe");
  h.api.openFile("other.md");
  h.state.threadId = "thread-a"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 3);
  assert.equal(sideView.children[0], frame);
  assert.equal(frame.savedDraft, "侧边独立输入");
  assert.equal(h.selected().children[0].getAttribute("aria-label"), "侧边 B");
  h.api.dispose();
  assert.equal(frame.isConnected, false);
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
  h.toggle.emit("click");
  assert.equal(visibility.at(-1).visible, false);
  h.toggle.emit("click");
  assert.equal(visibility.at(-1).visible, true);
  h.media.matches = false; h.media.change();
  assert.equal(visibility.at(-1).visible, false);
  h.api.dispose();
  assert.equal(frame.events.has("load"), false);
});

test("侧边资源交给主页面预览，拒绝其他来源及隐藏标签，切换或释放时关闭所属预览", () => {
  const opened = [], closed = [];
  const record = kind => (trigger, ...args) => {
    const value = { kind, trigger, args };
    opened.push(value);
    return { close: () => closed.push(value) };
  };
  const h = harness({ openImagePreview: record("image"), openVideoPreview: record("video") });
  h.api.openSideThread({ id: "side-a" });
  const frame = h.views.children[0].children[0];
  frame.contentWindow = { postMessage() {} };
  const resource = { type: "codex-elves-workspace-media-preview", kind: "image",
    source: "/api/images/abcdefghijklmnopqrst", name: "图片.png" };
  const event = { source: frame.contentWindow, origin: "http://127.0.0.1:45222", data: resource };
  h.message({ ...event, source: {} });
  h.message({ ...event, origin: "https://other.example" });
  h.message({ ...event, data: { ...resource, kind: "file" } });
  h.message({ ...event, data: { ...resource, source: {} } });
  assert.equal(opened.length, 0);
  h.message(event);
  assert.equal(opened.length, 1);
  assert.equal(opened[0].trigger, frame, "整页预览关闭后将焦点交还原 iframe");
  assert.equal(opened[0].args[0].originalImageSource, resource.source);
  assert.equal(opened[0].args[0].alt, resource.name);
  h.api.open("files");
  assert.deepEqual(closed, [opened[0]]);
  h.message(event);
  assert.equal(opened.length, 1, "保留的后台 iframe 不能弹出资源");
  h.api.openSideThread({ id: "side-a" });
  h.message({ ...event, data: { ...resource, kind: "video", poster: 42 } });
  assert.equal(opened.length, 1);
  const source = `/api/videos/${"a".repeat(64)}`;
  h.message({ ...event, data: { ...resource, kind: "video", source, poster: `${source}/poster` } });
  assert.equal(opened[1].kind, "video");
  assert.deepEqual(opened[1].args, [source, `${source}/poster`, resource.name]);
  frame.emit("load");
  assert.deepEqual(closed, [opened[0], opened[1]], "来源页面重新加载时清理旧预览");
  h.message(event);
  h.state.threadId = "thread-b"; h.api.updateContext();
  assert.deepEqual(closed, opened, "切换主会话时关闭旧侧边资源预览，但保留 iframe");
  assert.equal(frame.isConnected, true);
  h.message(event);
  assert.equal(opened.length, 3, "后台会话不能打开整页资源");
  h.state.threadId = "thread-a"; h.api.updateContext();
  h.message(event);
  assert.equal(opened.length, 4, "切回后原 iframe 可以继续打开资源");
  h.state.cacheScope = "scope-b";
  h.api.updateContext();
  assert.deepEqual(closed, opened, "授权切换时关闭预览并移除消息监听");
  h.message(event);
  assert.equal(opened.length, 4);
  assert.equal(h.messageListeners.size, 0);
  h.api.dispose();
});

test("侧边标签只接受自身原生执行状态，隐藏后仍同步旋转图标并在结束时恢复", () => {
  const h = harness();
  h.api.openSideThread({ id: "side-a", title: "侧边 A", running: true });
  const button = h.tablist.children[0].children[0], symbol = button.children[0];
  const frame = h.views.children[0].children[0];
  frame.contentWindow = { postMessage() {} };
  assert.equal(symbol.classList.contains("spinner"), true);
  assert.match(button.getAttribute("aria-label"), /正在执行/);
  const event = { source: frame.contentWindow, origin: "http://127.0.0.1:45222",
    data: { type: "codex-elves-side-panel-state", threadId: "side-a", running: false } };
  h.message({ ...event, source: {} });
  h.message({ ...event, origin: "https://other.example" });
  h.message({ ...event, data: { ...event.data, threadId: "side-b" } });
  h.message({ ...event, data: { ...event.data, running: "false" } });
  assert.equal(symbol.classList.contains("spinner"), true);
  h.api.openSideThread({ id: "side-b", title: "侧边 B" });
  h.message(event);
  assert.equal(symbol.classList.contains("spinner"), false);
  assert.equal(button.getAttribute("aria-label"), "侧边 A");
  h.api.close();
  h.message({ ...event, data: { ...event.data, running: true } });
  assert.equal(symbol.classList.contains("spinner"), true);
  const otherSymbol = h.tablist.children[1].children[0].children[0];
  assert.equal(otherSymbol.classList.contains("spinner"), false);
  h.message(event);
  assert.equal(symbol.classList.contains("spinner"), false);
  h.api.dispose();
  assert.equal(h.messageListeners.size, 0);
});

test("手机、Android 和侧边页面本身不嵌套会话标签，非法目标不创建页面", () => {
  for (const options of [{ desktop: false }, { android: true }, { sidePanel: true }]) {
    const h = harness(options);
    assert.equal(h.api.openSideThread({ id: "side-a" }), false);
    assert.equal(h.api.openSideCreation({ requestId: "request-a" }), null);
    assert.equal(h.tablist.children.length, 0);
    assert.equal(h.toggle.hidden, true);
    h.api.dispose();
  }
  const h = harness();
  assert.equal(h.api.openSideThread({ id: "thread-a" }), false);
  assert.equal(h.api.openSideThread({ id: "../outside" }), false);
  assert.equal(h.tablist.children.length, 0);
  h.api.dispose();
});

test("PC 标签开关位于会话菜单右侧，展开移到右侧栏同一窗口边缘，空面板不发读取", () => {
  const h = harness();
  assert.equal(h.header.children[1], h.toggle);
  assert.equal(h.toggle.getAttribute("aria-expanded"), "false");
  h.toggle.emit("click");
  assert.equal(h.dock.hidden, false);
  assert.equal(h.toolbar.children.at(-1), h.toggle);
  assert.equal(h.toggle.getAttribute("aria-label"), "隐藏标签页");
  assert.equal(h.empty.hidden, false);
  assert.equal(h.renderers.length, 0);
  assert.equal(h.toolbar.children.some(child =>
    child.classList.contains("workspace-reader-add") || child.classList.contains("workspace-reader-close")), false);
  h.toggle.emit("click");
  assert.equal(h.header.children[1], h.toggle);
  assert.equal(h.toggle.getAttribute("aria-label"), "打开标签页");
  h.api.dispose();
  assert.equal(h.toggle.isConnected, false);
});

test("隐藏创建等待面板保留原操作，迟到会话标签可在隐藏状态接替", () => {
  const h = harness(), closed = [];
  const pending = h.api.openSideCreation({ requestId: "request-a", onClose: () => closed.push(true) });
  h.toggle.emit("click");
  assert.equal(pending.isVisible(), false);
  assert.equal(h.tablist.children.length, 1);
  assert.equal(closed.length, 0, "隐藏不能当成用户关闭等待标签");
  h.api.openSideThread({ id: "side-a" }, { reveal: pending.isVisible() });
  pending.close();
  assert.equal(h.dock.hidden, true);
  assert.equal(h.tablist.children.length, 1);
  h.toggle.emit("click");
  assert.equal(h.dock.hidden, false);
  assert.equal(h.views.children[0].children[0].tag, "iframe");
  h.api.dispose();
});

test("PC 新建等待在右侧标签显示，重复操作复用标签，关闭只结束展示", () => {
  const h = harness(), closed = [];
  h.api.openFile("keep.md");
  const pending = h.api.openSideCreation({ requestId: "request-a", onClose: () => closed.push("a") });
  assert.equal(h.dock.hidden, false);
  assert.equal(h.tablist.children.length, 2);
  assert.equal(h.conversation.inert, false);
  const status = h.views.children[1].children[0];
  assert.equal(status.getAttribute("role"), "status");
  assert.equal(status.children[1].textContent, "正在创建侧边会话…");
  h.api.openSideCreation({ requestId: "request-a" });
  assert.equal(h.tablist.children.length, 2);
  pending.update(true);
  assert.equal(status.children[0].hidden, true);
  assert.match(status.children[1].textContent, /确认创建结果/);
  h.api.openSideThread({ id: "side-a" });
  pending.close();
  assert.equal(h.tablist.children.length, 2);
  assert.equal(h.selected(), h.tablist.children[1]);
  assert.deepEqual(closed, ["a"]);
  pending.close();
  assert.deepEqual(closed, ["a"]);
  h.api.dispose();
});

test("后台创建等待句柄只更新或关闭所属会话，不操作当前会话的同名等待标签", () => {
  const h = harness();
  const first = h.api.openSideCreation({ requestId: "request-a" });
  h.state.threadId = "thread-b"; h.api.updateContext();
  const second = h.api.openSideCreation({ requestId: "request-a" });
  const current = h.visibleViews()[0];
  assert.equal(first.isVisible(), false);
  assert.equal(second.isVisible(), true);
  first.update(true);
  assert.equal(current.children[0].children[1].textContent, "正在创建侧边会话…");
  first.close();
  assert.equal(h.visibleTabs().length, 1);
  assert.equal(h.visibleViews()[0], current);
  assert.equal(second.isVisible(), true);
  h.state.threadId = "thread-a"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0);
  second.close();
  h.state.threadId = "thread-b"; h.api.updateContext();
  assert.equal(h.visibleTabs().length, 0);
  assert.equal(h.dock.hidden, true);
  h.api.dispose();
});

test("侧边快捷键请求只接受同源可见 iframe，切换后暂停，切回恢复，授权变化释放", () => {
  const created = [], h = harness({ createSideThread: () => created.push(true) });
  h.api.openSideThread({ id: "side-a" });
  const first = h.views.children[0].children[0];
  first.contentWindow = { postMessage() {} };
  const event = { source: first.contentWindow, origin: "http://127.0.0.1:45222",
    data: { type: "codex-elves-create-side-thread" } };
  h.message({ ...event, source: {} });
  h.message({ ...event, origin: "https://other.example" });
  assert.equal(created.length, 0);
  h.message(event);
  assert.equal(created.length, 1);
  h.api.openFile("notes.md");
  h.message(event);
  assert.equal(created.length, 1);
  h.api.openSideThread({ id: "side-a" });
  h.message(event);
  assert.equal(created.length, 2);
  h.state.threadId = "thread-b"; h.api.updateContext();
  h.message(event);
  assert.equal(created.length, 2);
  assert.equal(h.messageListeners.size, 1);
  h.state.threadId = "thread-a"; h.api.updateContext();
  h.message(event);
  assert.equal(created.length, 3);
  h.state.cacheScope = "scope-b"; h.api.updateContext();
  h.message(event);
  assert.equal(created.length, 3);
  assert.equal(h.messageListeners.size, 0);
  h.api.dispose();
});
