import { requestRemote } from "./channels.js";
import { createWorkspaceBranch, createWorkspaceLocation, displayWorkspaceName, displayWorkspacePath, mountPreviewControlsScroll, mountSessionFiles, setWorkspaceBranch } from "./session-files.js";
import { safeInlineImageSource } from "./attachments.js";
import { isAndroidContainer } from "./platform.js";

const sections = [
  ["staged", "已暂存"], ["unstaged", "未暂存"], ["untracked", "未跟踪"]
];
const statusLabels = {
  A: "新增", M: "修改", D: "删除", R: "重命名", C: "复制", U: "冲突", T: "类型变化", "?": "新增",
  added: "新增", modified: "修改", deleted: "删除", renamed: "重命名",
  copied: "复制", conflicted: "冲突", untracked: "新增"
};
const imageSource = value => typeof value === "string"
  && /^data:image\/(?:png|jpe?g|gif|webp|avif|bmp|x-icon|vnd\.microsoft\.icon|svg\+xml);base64,[a-zA-Z0-9+/=\s]+$/.test(value);

function node(tag, className = "", text = "") {
  const result = document.createElement(tag);
  result.className = className;
  if (text) result.textContent = text;
  return result;
}
function control(text, label, action, className = "") {
  const result = node("button", className, text);
  result.type = "button";
  result.title = label;
  result.setAttribute("aria-label", label);
  result.addEventListener("click", action);
  return result;
}
function byteSize(size) {
  if (!Number.isFinite(size)) return "大小未知";
  return size < 1024 ? `${size} B` : size < 1024 * 1024
    ? `${(size / 1024).toFixed(1)} KiB` : `${(size / 1024 / 1024).toFixed(1)} MiB`;
}
function statusLabel(value) {
  return statusLabels[value] || statusLabels[String(value).trim()] || String(value || "变更");
}
const cacheSchema = 1, responseVersion = 3, memoryLimit = 16 * 1024 * 1024;
const validRevision = value => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const objectValue = value => value !== null && typeof value === "object" && !Array.isArray(value);
const safeString = (value, maximum = 32768) => typeof value === "string" && value.length <= maximum;
const validRoot = value => safeString(value) && value.length > 0;
const validRepository = value => safeString(value, 16384) && (value === "" || !/[\\:\0]/.test(value)
  && value.split("/").every(part => part && part !== "." && part !== ".." && part.toLowerCase() !== ".git" && !/[ .]$/.test(part)));
// 用户要求多仓库全部可查：列表、详情缓存和历史导航都包含仓库身份，隔离各仓库的同名文件。
const detailKey = file => JSON.stringify([file.repository || "", file.section, file.path]);
const displayedPath = file => displayWorkspacePath(file.repository ? `${file.repository}/${file.path}` : file.path);
const groupKey = (repository, section) => repository ? JSON.stringify([repository, section]) : section;
function fileEntry(value) {
  return objectValue(value) && sections.some(([section]) => section === value.section)
    && safeString(value.path) && value.path.length > 0 && safeString(value.status, 64)
    && (value.repository == null || validRepository(value.repository))
    && (value.oldPath == null || safeString(value.oldPath));
}
function listResponse(value) {
  if (!objectValue(value) || value.ok === false || !validRoot(value.root) || !validRevision(value.revision)
      || !Array.isArray(value.files) || value.files.length > 10000 || !value.files.every(fileEntry)
      || typeof value.truncated !== "boolean" || value.message != null && !safeString(value.message, 10000)) return null;
  let repositories;
  if (value.repositories != null) {
    if (!Array.isArray(value.repositories) || value.repositories.length > 256) return null;
    repositories = value.repositories.map(repository => {
      if (!objectValue(repository) || !validRepository(repository.path) || !validRoot(repository.root)
          || !Number.isSafeInteger(repository.fileCount) || repository.fileCount < 0
          || typeof repository.truncated !== "boolean"
          || repository.error != null && !safeString(repository.error, 10000)) return null;
      return { path: repository.path, root: repository.root, fileCount: repository.fileCount,
        truncated: repository.truncated, error: repository.error || null,
        ...(objectValue(repository.git) ? { git: repository.git } : {}) };
    });
    if (repositories.includes(null)) return null;
    const paths = new Set(repositories.map(repository => repository.path));
    if (paths.size !== repositories.length
        || value.files.some(file => typeof file.repository !== "string" || !paths.has(file.repository))) return null;
  } else if (value.files.some(file => file.repository != null)) return null;
  return { ok: true, root: value.root, revision: value.revision, truncated: value.truncated,
    message: value.message || null,
    ...(objectValue(value.git) ? { git: value.git } : {}),
    ...(repositories ? { repositories } : {}),
    files: value.files.map(file => ({ section: file.section, path: file.path, status: file.status,
      ...(file.repository != null ? { repository: file.repository } : {}),
      ...(file.oldPath != null ? { oldPath: file.oldPath } : {}) })) };
}
function detailResponse(value, file) {
  const sideValid = side => side == null || objectValue(side) && typeof side.exists === "boolean"
    && Number.isFinite(side.size) && side.size >= 0 && (side.mime == null || safeString(side.mime, 256))
    && (side.dataUrl == null || imageSource(side.dataUrl));
  if (!objectValue(value) || value.ok === false || !validRoot(value.root) || !validRevision(value.revision)
      || value.path !== file.path || value.section !== file.section
      || value.repository != null && !validRepository(value.repository)
      || (value.repository || "") !== (file.repository || "")
      || !["text", "image", "binary", "unsupported"].includes(value.kind)
      || typeof value.truncated !== "boolean" || value.diff != null && !safeString(value.diff, 4 * 1024 * 1024)
      || value.message != null && !safeString(value.message, 10000)
      || !sideValid(value.before) || !sideValid(value.after)) return null;
  const side = value => value == null ? null : {
    exists: value.exists, size: value.size, mime: value.mime || null, dataUrl: value.dataUrl || null
  };
  return { ok: true, root: value.root, revision: value.revision, path: value.path, section: value.section,
    ...(objectValue(value.git) ? { git: value.git } : {}),
    ...(value.repository != null ? { repository: value.repository } : {}),
    kind: value.kind, diff: value.diff || "", truncated: value.truncated, message: value.message || null,
    before: side(value.before), after: side(value.after) };
}
function uiState(value = {}) {
  return {
    query: safeString(value?.query, 1000) ? value.query : "",
    collapsed: Object.fromEntries(Object.entries(objectValue(value?.collapsed) ? value.collapsed : {})
      .filter(([key, collapsed]) => safeString(key, 32768) && collapsed === true).slice(0, 768)),
    scrollTop: Number.isFinite(value?.scrollTop) ? Math.max(0, value.scrollTop) : 0,
    wrap: value?.wrap === true
  };
}
function responseBytes(value) {
  const serialized = JSON.stringify(value);
  return typeof TextEncoder === "function" ? new TextEncoder().encode(serialized).length : serialized.length * 2;
}
export function mountSessionTools(options) {
  const { workspace, trigger, getContext, beforeOpen } = options;
  const desktopMedia = window.matchMedia("(min-width:761px)");
  const desktop = () => desktopMedia.matches && !isAndroidContainer() && !options.sidePanel;
  const contexts = new Map();
  const owner = `workspace-reader-${Math.random().toString(36).slice(2)}`;
  const widthKey = "codexElvesWorkspacePanelWidth";
  let disposed = false, mobile = null, sequence = 0, contextKey = "", authorizationKey = "", drag = null;
  let currentContext = null, displayedContext = null;
  // 默认 480px 便于并排阅读；仅用户拖动或键盘调整才保存，窗口临时变窄不能覆盖偏好。
  let preferredWidth = 480;
  try {
    const saved = Number(localStorage.getItem(widthKey));
    if (Number.isFinite(saved) && saved >= 200 && saved <= 10000) preferredWidth = saved;
  } catch {}
  const dock = node("aside", "workspace-reader");
  dock.id = `${owner}-panel`;
  dock.hidden = true; dock.inert = true;
  dock.setAttribute("aria-label", "文件、变更与侧边会话");
  const separator = node("div", "workspace-reader-resize");
  separator.tabIndex = 0;
  separator.setAttribute("role", "separator");
  separator.setAttribute("aria-label", "调整右侧标签面板宽度");
  separator.setAttribute("aria-orientation", "vertical");
  const toolbar = node("header", "workspace-reader-toolbar");
  const tablist = node("div", "workspace-reader-tabs");
  tablist.setAttribute("role", "tablist");
  tablist.setAttribute("aria-label", "文件、变更与侧边会话标签页");
  const toggle = control("", "打开标签页", () => {
    if (disposed || !desktop()) return;
    updateContext(); beforeOpen?.();
    currentContext.expanded = !currentContext.expanded; syncLayout();
    toggle.focus({ preventScroll: true });
  }, "icon-button workspace-reader-toggle");
  toggle.hidden = true;
  toggle.setAttribute("aria-controls", dock.id);
  toggle.innerHTML = '<svg class="ui-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="3"/><path class="workspace-reader-toggle-pane" d="M14 4h3a3 3 0 0 1 3 3v10a3 3 0 0 1-3 3h-3z"/><path d="M14 4v16"/></svg>';
  const views = node("div", "workspace-reader-views");
  const empty = node("div", "workspace-reader-empty", "从会话菜单打开文件、变更或侧边会话");
  toolbar.append(tablist); dock.append(separator, toolbar, views, empty); workspace.append(dock);
  trigger.after(toggle);

  function identity(context = getContext() || {}) {
    return JSON.stringify([context.deviceId || "", context.cacheScope || "",
      context.threadId ? ["thread", context.threadId] : context.creating ? ["project", context.projectId || ""] : null]);
  }
  function authorizationIdentity(context = getContext() || {}) {
    return JSON.stringify([context.deviceId || "", context.cacheScope || ""]);
  }
  function mobilePages() {
    mobile ||= mountSessionToolPages({ ...options, isAvailable: () => !desktop() });
    return mobile;
  }
  function widthBounds() {
    const sidebar = workspace.querySelector(":scope > .sidebar");
    const available = Math.max(0, workspace.clientWidth - (sidebar?.getBoundingClientRect().width || 0));
    // 用户要求右侧可拖宽；通常至少 320px、给会话保留 360px，窄窗口则各占一半，不能撑出横向整页滚动。
    const minimum = Math.min(320, Math.floor(available / 2));
    return { minimum, maximum: Math.max(minimum, available - 360) };
  }
  function resize(width = currentContext?.width ?? preferredWidth, persist = false) {
    const { minimum, maximum } = widthBounds();
    const next = Math.round(Math.max(minimum, Math.min(maximum, width)));
    workspace.style.setProperty("--workspace-reader-width", `${next}px`);
    separator.setAttribute("aria-valuemin", String(minimum));
    separator.setAttribute("aria-valuemax", String(maximum));
    separator.setAttribute("aria-valuenow", String(next));
    separator.setAttribute("aria-valuetext", `${next} 像素`);
    if (persist) {
      preferredWidth = next;
      if (currentContext) currentContext.width = next;
      try { localStorage.setItem(widthKey, String(next)); } catch {}
    }
    return next;
  }
  function syncLayout() {
    // 用户要求 PC 按会话恢复标签及阅读状态；旧 DOM/iframe 留在原位置，只隐藏，避免移动 iframe 导致重新加载。
    if (!dock.hidden && displayedContext) displayedContext.scrollLeft = tablist.scrollLeft;
    const visible = desktop() && currentContext?.expanded === true;
    const tabs = currentContext?.tabs;
    dock.hidden = !visible; dock.inert = !visible;
    views.hidden = !tabs?.size; empty.hidden = !!tabs?.size;
    workspace.classList.toggle("workspace-reader-open", visible);
    toggle.hidden = !desktop();
    toggle.title = visible ? "隐藏标签页" : "打开标签页";
    toggle.setAttribute("aria-label", toggle.title);
    toggle.setAttribute("aria-expanded", String(visible));
    // 用户要求开关固定在窗口右上角：展开时移到标签栏最右侧，收起时回到会话菜单右侧。
    // 两处右内边距均为 24px，按钮均为 32px；需与 style.css 的标签栏和会话页头同步保持坐标。
    if (visible) {
      if (toggle.parentElement !== toolbar) toolbar.append(toggle);
    } else if (toggle.parentElement !== trigger.parentElement) trigger.after(toggle);
    for (const context of contexts.values()) for (const record of context.tabs.values()) {
      const selected = context === currentContext && record.key === context.active;
      record.tab.hidden = context !== currentContext;
      record.view.hidden = !selected; record.view.inert = !selected;
      record.renderer.setVisible?.(visible && selected);
    }
    displayedContext = currentContext;
    if (visible) { mobile?.hide(); resize(); tablist.scrollLeft = currentContext.scrollLeft; }
    else finishDrag(false);
  }
  function hidePanel() {
    // 用户要求隐藏后恢复原页面：只收起容器，保留标签 DOM、iframe、滚动和草稿，不调用 dispose 或重载。
    if (currentContext) currentContext.expanded = false;
    syncLayout();
    toggle.focus({ preventScroll: true });
  }
  function tabKey(item) {
    if (item.kind === "side") return JSON.stringify(["side", item.threadId]);
    if (item.kind === "side-creation") return JSON.stringify(["side-creation", item.requestId]);
    if (item.kind === "file") return JSON.stringify(["file", item.path.replaceAll("\\", "/")]);
    if (item.kind === "change") return JSON.stringify(["change", detailKey(item.file)]);
    return item.kind;
  }
  function selectTab(record, focus = false, context = currentContext) {
    if (!record || disposed || !context) return;
    context.active = record.key;
    for (const candidate of context.tabs.values()) {
      const selected = candidate === record;
      candidate.button.setAttribute("aria-selected", String(selected));
      candidate.button.tabIndex = selected ? 0 : -1;
      candidate.tab.classList.toggle("selected", selected);
    }
    syncLayout();
    if (context === currentContext) {
      if (!dock.hidden) record.tab.scrollIntoView?.({ block: "nearest", inline: "nearest" });
      if (focus) record.button.focus({ preventScroll: true });
    }
  }
  function closeTab(key, focus = true, context = currentContext) {
    const tabs = context?.tabs, record = tabs?.get(key);
    if (!record) return;
    const order = [...tabs.keys()], index = order.indexOf(key);
    record.renderer.dispose(); record.tab.remove(); record.view.remove(); tabs.delete(key);
    if (!tabs.size) context.expanded = false;
    if (context.active === key) {
      context.active = null;
      selectTab(tabs.get(order[index + 1]) || tabs.get(order[index - 1]), focus, context);
    }
    syncLayout();
    if (!tabs.size && focus && context === currentContext) trigger?.focus({ preventScroll: true });
    record.item.onClose?.();
  }
  function clearContexts() {
    finishDrag(false);
    for (const context of contexts.values())
      for (const key of [...context.tabs.keys()]) closeTab(key, false, context);
    contexts.clear(); currentContext = null;
    syncLayout();
  }
  function updateContext() {
    if (disposed) return;
    const value = { ...(getContext() || {}) }, nextAuthorization = authorizationIdentity(value);
    // 会话/新建项目分别保留标签、内容、选中项、显隐、宽度及标签滚动；注销或更换授权才释放全部页面与请求。
    if (nextAuthorization !== authorizationKey) {
      clearContexts(); authorizationKey = nextAuthorization;
    }
    const next = identity(value);
    if (next !== contextKey || !currentContext) {
      finishDrag(false);
      contextKey = next;
      if (!contexts.has(next)) contexts.set(next, {
        key: next, value, tabs: new Map(), active: null, expanded: false, width: preferredWidth, scrollLeft: 0,
      });
      currentContext = contexts.get(next);
      currentContext.value = value;
      syncLayout();
    } else currentContext.value = value;
    mobile?.updateContext();
  }
  function openTab(item, { reveal = true } = {}) {
    if (disposed) return;
    updateContext(); beforeOpen?.();
    const context = currentContext, tabs = context.tabs;
    if (reveal) context.expanded = true;
    const key = tabKey(item), existing = tabs.get(key);
    if (existing) {
      selectTab(existing);
      if (item.kind === "file") void existing.renderer.openFile(item.path, item.line);
      else if (item.kind === "change") void existing.renderer.openChange(item.file, item.list);
      else if (item.kind === "changes") existing.renderer.open("changes");
      else void existing.renderer.open();
      return existing;
    }
    const id = `${owner}-${++sequence}`;
    const side = item.kind === "side" || item.kind === "side-creation";
    const label = side ? item.title : item.kind === "changes" ? "变更" : item.kind === "files" ? "打开文件"
      : `${displayWorkspaceName((item.path || item.file.path).split(/[\\/]/).at(-1))}${item.kind === "change" ? " · 变更" : ""}`;
    const title = item.kind === "change" ? `${sections.find(([name]) => name === item.file.section)?.[1] || "变更"}：${displayedPath(item.file)}`
      : item.path ? displayWorkspacePath(item.path) : label;
    const tab = node("div", "workspace-reader-tab");
    tab.setAttribute("role", "presentation");
    const button = control("", title, () => { if (ownsTab()) selectTab(record); }, "workspace-reader-tab-select");
    button.id = `${id}-tab`; button.setAttribute("role", "tab");
    button.setAttribute("aria-controls", id);
    const symbol = node("span", "workspace-reader-tab-icon");
    symbol.innerHTML = `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">${
      side
        ? '<path d="M3 3.5h14v10H8l-4 3v-3H3z"/><path d="M6 7h8M6 10h5"/>'
        : item.kind === "changes" || item.kind === "change"
        ? '<rect x="4" y="2.5" width="12" height="15" rx="2"/><path d="M7 7h6M10 4v6M7 13h6"/>'
        : '<path d="M11.5 2.5H5a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V7z"/><path d="M11 2.5V7h5"/>'}</svg>`;
    symbol.setAttribute("aria-hidden", "true");
    const updateRunning = running => {
      // 用户要求侧边会话执行时原位换成已有旋转图标，完成/停止后恢复原 SVG；隐藏标签也继续同步原生状态。
      symbol.classList.toggle("spinner", running);
      button.title = running ? `${title}，正在执行` : title;
      button.setAttribute("aria-label", button.title);
    };
    if (item.kind === "side") updateRunning(item.running === true);
    button.append(symbol, node("span", "workspace-reader-tab-label", label));
    const remove = control("×", `关闭${label}`, () => { if (ownsTab()) closeTab(key); }, "workspace-reader-tab-close");
    tab.append(button, remove);
    const view = node("section", "workspace-reader-view");
    view.id = id; view.setAttribute("role", "tabpanel"); view.setAttribute("aria-labelledby", button.id);
    tablist.append(tab); views.append(view);
    const retainsTab = () => !disposed && tabs.get(key) === record && contexts.get(context.key) === context
      && authorizationKey === authorizationIdentity();
    const ownsTab = () => retainsTab() && currentContext === context && context.key === identity();
    // 用户要求 PC 中键点击关闭标签；按下时阻止浏览器自动滚屏，松开后复用正常关闭，不关闭服务端会话。
    tab.addEventListener("mousedown", event => {
      if (event.button === 1 && ownsTab()) event.preventDefault();
    });
    tab.addEventListener("auxclick", event => {
      if (event.button !== 1 || !ownsTab()) return;
      event.preventDefault(); event.stopPropagation(); closeTab(key);
    });
    const embedded = { id, onClose: () => { if (ownsTab()) closeTab(key); },
      onOpenFile: (path, line) => { if (ownsTab()) openTab({ kind: "file", path, line }); },
      onOpenChange: (file, list) => { if (ownsTab()) openTab({ kind: "change", file, list }); } };
    // 后台标签的迟到读取只更新所属会话，不能从当前主会话借用 threadId/projectId；授权变化立即失效。
    const rendererOptions = { ...options, workspace: view, beforeOpen: undefined, embedded,
      getContext: () => authorizationIdentity(context.value) === authorizationIdentity() ? context.value : {} };
    // 每个标签保留自己的阅读 DOM、滚动和请求；详情不重复写变更列表的持久缓存。
    const renderer = item.kind === "side" ? mountWorkspaceSideThread(view, item, () => {
      // 侧边 iframe 中的快捷键仍从当前主会话创建；只接受当前可见标签，不允许嵌套创建。
      if (ownsTab() && desktop() && context.active === key) options.createSideThread?.();
    }, running => {
      if (retainsTab()) updateRunning(running);
    }, (trigger, resource) => {
      if (!ownsTab() || !desktop() || context.active !== key) return;
      return resource.kind === "video"
        ? options.openVideoPreview?.(trigger, resource.source, resource.poster, resource.name)
        : options.openImagePreview?.(trigger, { originalImageSource: resource.source, alt: resource.name });
    })
      : item.kind === "side-creation" ? mountWorkspaceSideCreation(view)
      : item.kind === "changes" || item.kind === "change"
      ? mountSessionToolPages({ ...rendererOptions, cache: item.kind === "changes" ? options.cache : null })
      : mountSessionFiles(rendererOptions);
    const record = { key, item, tab, button, view, renderer };
    tabs.set(key, record); selectTab(record);
    if (side) renderer.open();
    else if (item.kind === "changes") renderer.open("changes");
    else if (item.kind === "change") void renderer.openChange(item.file, item.list);
    else if (item.kind === "files") void renderer.open();
    else void renderer.openFile(item.path, item.line);
    return record;
  }
  function open(name) {
    if (!["files", "changes"].includes(name)) return;
    updateContext();
    if (desktop()) openTab({ kind: name });
    else mobilePages().open(name);
  }
  function openFile(path, line) {
    if (typeof path !== "string" || !path) return;
    updateContext();
    if (desktop()) openTab({ kind: "file", path, line });
    else mobilePages().openFile(path, line);
  }
  function openSideThread(item, { reveal = true } = {}) {
    if (!desktop() || disposed || typeof item?.id !== "string" || !/^[A-Za-z0-9_-]{1,120}$/.test(item.id)) return false;
    const parentThreadId = getContext()?.threadId;
    if (!parentThreadId || item.id === parentThreadId) return false;
    // 用户要求 PC 侧边会话与主聊天并排，共用文件/变更标签栏；重复点击只选中已有标签，不重载或重发。
    openTab({ kind: "side", threadId: item.id, title: String(item.title || item.name || "侧边会话"),
      running: item.running }, { reveal });
    return true;
  }
  function openSideCreation({ requestId, onClose }) {
    if (!desktop() || disposed || !getContext()?.threadId) return null;
    // 用户要求 PC 新建也直接进入右侧标签：这里只展示原操作的等待状态，不另发创建请求、不禁用主聊天。
    const record = openTab({ kind: "side-creation", requestId, title: "新建侧边会话", onClose });
    const context = currentContext, tabs = context.tabs;
    return {
      isVisible: () => !disposed && currentContext === context && context.key === identity()
        && desktop() && context.expanded,
      update(unknown) {
        if (tabs.get(record.key) === record) record.renderer.update(unknown);
      },
      close() {
        if (tabs.get(record.key) === record) closeTab(record.key, false, context);
      }
    };
  }
  function finishDrag(persist) {
    if (!drag) return;
    const previous = drag; drag = null;
    workspace.classList.remove("workspace-reader-resizing");
    if (separator.hasPointerCapture?.(previous.pointer)) separator.releasePointerCapture(previous.pointer);
    resize(persist ? previous.current : currentContext?.width ?? preferredWidth, persist);
  }
  separator.addEventListener("pointerdown", event => {
    if (event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    const width = resize();
    drag = { pointer: event.pointerId, x: event.clientX, width, current: width };
    separator.setPointerCapture(event.pointerId);
    workspace.classList.add("workspace-reader-resizing");
  });
  separator.addEventListener("pointermove", event => {
    if (!drag || drag.pointer !== event.pointerId) return;
    drag.current = resize(drag.width + drag.x - event.clientX);
  });
  separator.addEventListener("pointerup", event => { if (drag?.pointer === event.pointerId) finishDrag(true); });
  separator.addEventListener("pointercancel", () => finishDrag(false));
  separator.addEventListener("lostpointercapture", () => finishDrag(false));
  separator.addEventListener("dblclick", () => resize(480, true));
  separator.addEventListener("keydown", event => {
    const width = Number(separator.getAttribute("aria-valuenow"));
    const bounds = widthBounds();
    // 分隔线也可用方向键每次调整 20px，Home/End 到两端，避免只能靠鼠标拖动。
    const next = { ArrowLeft: width + 20, ArrowRight: width - 20, Home: bounds.minimum, End: bounds.maximum }[event.key];
    if (next == null) return;
    event.preventDefault(); resize(next, true);
  });
  tablist.addEventListener("wheel", event => {
    if (event.ctrlKey || event.metaKey || tablist.scrollWidth <= tablist.clientWidth) return;
    // 用户要求标签栏滚轮横向浏览：保留触控板主滚动方向，普通竖向滚轮映射到横向。
    // deltaMode 为行时按 16px/行转换，为页时按标签栏可见宽度转换；缩放手势仍交给浏览器。
    const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
    if (!delta) return;
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? tablist.clientWidth : 1;
    event.preventDefault();
    tablist.scrollLeft = Math.max(0, Math.min(tablist.scrollWidth - tablist.clientWidth, tablist.scrollLeft + delta * unit));
  }, { passive: false });
  tablist.addEventListener("keydown", event => {
    const records = [...(currentContext?.tabs.values() || [])], index = records.findIndex(record => record.button === event.target);
    if (index < 0 || event.altKey || event.ctrlKey || event.metaKey) return;
    const next = { ArrowLeft: (index - 1 + records.length) % records.length,
      ArrowRight: (index + 1) % records.length, Home: 0, End: records.length - 1 }[event.key];
    if (next != null) { event.preventDefault(); selectTab(records[next], true); }
    else if (event.key === "Delete") { event.preventDefault(); closeTab(records[index].key); }
  });
  dock.addEventListener("keydown", event => {
    if (event.key !== "Escape" || event.isComposing || document.querySelector("dialog[open]")) return;
    event.preventDefault(); event.stopPropagation(); hidePanel();
  });
  const observer = typeof ResizeObserver === "function" ? new ResizeObserver(() => {
    if (desktop() && currentContext?.expanded && !drag) resize();
  }) : null;
  observer?.observe(workspace);
  const sidebar = workspace.querySelector(":scope > .sidebar");
  if (sidebar) observer?.observe(sidebar);
  desktopMedia.addEventListener("change", syncLayout);
  updateContext();
  return {
    open, openFile, openSideThread, openSideCreation, updateContext,
    close: () => desktop() ? hidePanel() : mobile?.close(),
    dispose() {
      if (disposed) return;
      clearContexts(); mobile?.dispose(); disposed = true;
      observer?.disconnect(); desktopMedia.removeEventListener("change", syncLayout);
      toggle.remove(); dock.remove(); workspace.classList.remove("workspace-reader-open", "workspace-reader-resizing");
      workspace.style.removeProperty("--workspace-reader-width");
    }
  };
}

function mountWorkspaceSideCreation(workspace) {
  const status = node("div", "side-creation-status");
  status.setAttribute("role", "status");
  const spinner = node("span", "spinner"), message = node("p", "", "正在创建侧边会话…");
  status.append(spinner, message); workspace.append(status);
  return {
    open() {},
    update(unknown) {
      message.textContent = unknown ? "正在确认创建结果，可继续使用主会话。" : "正在创建侧边会话…";
      spinner.hidden = unknown;
    },
    dispose() { status.remove(); }
  };
}

function mountWorkspaceSideThread(workspace, item, createSideThread, updateRunning, openMediaPreview) {
  // 每个侧边会话使用独立页面上下文，隔离输入、选中会话、滚动和实时连接，不能借用主聊天的全局发送状态。
  // 专用入口仅允许同源嵌入，不修改普通首页的禁止嵌入策略；关闭标签只卸载页面，不关闭服务端会话。
  const frame = node("iframe", "workspace-side-thread");
  const address = new URL("/side-thread", window.location.href);
  address.hash = new URLSearchParams({ thread: item.threadId }).toString();
  frame.src = address.href; frame.title = item.title;
  frame.referrerPolicy = "no-referrer";
  let visible = false, mediaPreview = null;
  const closePreview = () => { mediaPreview?.close(); mediaPreview = null; };
  const sync = () => frame.contentWindow?.postMessage({
    type: "codex-elves-side-panel-visibility", visible,
  }, address.origin);
  const loaded = () => { closePreview(); sync(); };
  const onMessage = event => {
    if (event.source !== frame.contentWindow || event.origin !== address.origin) return;
    if (event.data?.type === "codex-elves-side-panel-state" && event.data.threadId === item.threadId
        && typeof event.data.running === "boolean") updateRunning?.(event.data.running);
    else if (visible && event.data?.type === "codex-elves-create-side-thread") createSideThread?.();
    else if (visible && event.data?.type === "codex-elves-workspace-media-preview"
        && ["image", "video"].includes(event.data.kind) && typeof event.data.source === "string"
        && typeof event.data.name === "string"
        && (event.data.kind !== "video" || typeof event.data.poster === "string")) {
      // 用户要求资源覆盖整页而非 iframe；同源、来源窗口及标签归属均核验，原预览器继续校验实际资源。
      closePreview();
      mediaPreview = openMediaPreview?.(frame, event.data) || null;
    }
  };
  frame.addEventListener("load", loaded);
  window.addEventListener("message", onMessage);
  workspace.append(frame);
  return { open() {}, setVisible(value) {
    if (visible !== value) { visible = value; if (!visible) closePreview(); sync(); }
  },
    dispose() {
      frame.removeEventListener("load", loaded); window.removeEventListener("message", onMessage); frame.remove();
      // 切换会话/项目/授权或关闭来源标签时结束其整页预览，不让旧资源留在新会话之上。
      closePreview();
    } };
}

export function mountWorkspacePreview(options, target) {
  // 独立预览复用原有文件/差异渲染器，只接收固定来源；不加载聊天，不创建新的原生会话。
  const embedded = { id: "independent-preview", onClose() {} };
  const renderer = target.kind === "change"
    ? mountSessionToolPages({ ...options, embedded })
    : mountSessionFiles({ ...options, embedded });
  const open = () => target.kind === "change"
    ? renderer.openChange({ path: target.path, section: target.section, repository: target.repository || "" })
    : renderer.openFile(target.path, target.line);
  void open();
  return { dispose: () => renderer.dispose(), refresh: open };
}

function mountSessionToolPages({ workspace, trigger, getContext, beforeOpen, onUnauthorized, openImagePreview, copyText, cache, onFileContextMenu, embedded = null, isAvailable = () => true, request: transport = requestRemote }) {
  let projectFiles = null;
  let disposed = false, opened = false, activePage = "changes", detailVisible = false, contextKey = "";
  let changesEpoch = 0, detailEpoch = 0, changesController = null, detailController = null;
  let files = [], selectedFile = null, changesLoaded = false, changesBusy = false;
  let returnView = null, wrapDiff = false, pageExitTimer = null, detailExitTimer = null;
  let project = null, displayedProject = null, detailRecord = null, authorized = true, projectSequence = 0;
  let legacyChangesUntil = 0;
  const projects = new Map(), cacheReads = new Set(), cacheMutations = new Map();
  const controllers = new Set(), inertState = new Map();
  const navigationOwner = Math.random().toString(36).slice(2);
  const historyKey = "codexElvesTools";
  const pageTitles = { changes: "变更" };
  const fileTriggers = new Map();
  const panel = node("section", "session-tools-screen");
  panel.id = embedded ? `${embedded.id}-changes` : "session-tools-screen";
  panel.hidden = true;
  panel.setAttribute("aria-labelledby", embedded ? `${panel.id}-title` : "session-tools-title");
  panel.setAttribute("tabindex", "-1");
  const heading = node("header", "session-tools-heading");
  const headingCopy = node("div", "session-tools-heading-copy");
  const pageTitle = node("h1", "", "变更");
  pageTitle.id = embedded ? `${panel.id}-title` : "session-tools-title";
  const rootLocation = createWorkspaceLocation("当前会话工作目录");
  const backToConversation = control("返回", "返回会话", requestBack, "text-button session-tools-back");
  headingCopy.append(pageTitle, rootLocation.element);
  heading.append(headingCopy, backToConversation);
  const pageBodies = new Map();
  for (const [name, label] of [["changes", "变更"]]) {
    const body = node("section", `session-tools-page session-tools-${name}`);
    body.id = embedded ? `${embedded.id}-${name}-body` : `session-tools-${name}`;
    body.setAttribute("aria-label", label);
    body.hidden = name !== activePage;
    pageBodies.set(name, body);
  }
  panel.append(heading, ...pageBodies.values());
  const detailPanel = node("section", "session-tools-screen session-tools-file-screen");
  detailPanel.id = embedded ? `${embedded.id}-detail` : "session-tools-file-screen"; detailPanel.hidden = true; detailPanel.inert = true;
  detailPanel.setAttribute("aria-labelledby", embedded ? `${detailPanel.id}-title` : "session-tools-file-title");
  const detailHeading = node("header", "session-tools-heading");
  const detailHeadingCopy = node("div", "session-tools-heading-copy");
  const detailTitle = node("h1");
  detailTitle.id = embedded ? `${detailPanel.id}-title` : "session-tools-file-title";
  const detailLocation = createWorkspaceLocation();
  detailHeadingCopy.append(detailTitle, detailLocation.element);
  const backToFiles = control("返回", "返回变更列表", requestBack, "text-button session-tools-back");
  detailHeading.append(detailHeadingCopy, backToFiles);
  const detail = node("div", "session-tools-detail");
  detail.setAttribute("aria-label", "文件差异");
  const detailControls = node("div", "session-tools-detail-controls");
  detailControls.hidden = true;
  const diffStats = node("span", "session-tools-diff-stats");
  const wrapButton = control("自动换行", "切换差异自动换行", () => {
    wrapDiff = !wrapDiff;
    wrapButton.setAttribute("aria-pressed", String(wrapDiff));
    detail.classList.toggle("session-tools-diff-wrap", wrapDiff);
    if (project) { project.ui.wrap = wrapDiff; markViewChanged(project); }
  }, "text-button");
  wrapButton.setAttribute("aria-pressed", "false");
  detailControls.append(diffStats, wrapButton);
  const detailNotice = node("p", "session-tools-status session-tools-cache-note");
  detailNotice.setAttribute("role", "status"); detailNotice.hidden = true;
  detailPanel.append(detailHeading, detailControls, detailNotice, detail);
  const detailControlsScroll = mountPreviewControlsScroll({
    body: detail, controls: detailControls, isActive: () => !disposed && detailVisible && !detailPanel.inert
  });
  workspace.append(panel, detailPanel);

  const changesPage = pageBodies.get("changes");
  const changesStatus = node("p", "session-tools-status");
  changesStatus.setAttribute("role", "status");
  const fileSearch = node("input", "session-tools-file-search");
  fileSearch.type = "search"; fileSearch.placeholder = "搜索当前变更文件";
  fileSearch.setAttribute("aria-label", "搜索当前变更文件"); fileSearch.autocomplete = "off";
  fileSearch.addEventListener("input", searchFiles);
  const fileList = node("div", "session-tools-file-list");
  fileList.addEventListener("scroll", saveListScroll, { passive: true });
  changesPage.append(fileSearch, changesStatus, fileList);

  function currentContext() {
    const value = getContext() || {};
    return {
      deviceId: value.deviceId || "", threadId: value.threadId || "",
      projectId: value.creating ? value.projectId || "" : "", readOnly: !!value.readOnly,
      cacheScope: typeof value.cacheScope === "string" ? value.cacheScope : ""
    };
  }
  function changeTarget() {
    const value = currentContext();
    if (value.threadId) return { threadId: value.threadId };
    if (value.projectId) return { projectId: value.projectId };
    return null;
  }
  async function request(path, body, signal) {
    const owner = authorizationKey();
    const controller = new AbortController();
    controllers.add(controller);
    const abort = () => controller.abort(signal?.reason);
    if (signal?.aborted) abort();
    else signal?.addEventListener("abort", abort, { once: true });
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort(new DOMException("服务响应超时", "TimeoutError"));
    }, 45000);
    const ensureCurrent = () => {
      if (controller.signal.aborted || !ownsAuthorization(owner)) {
        controller.abort();
        const error = new Error("请求已取消。"); error.name = "AbortError"; throw error;
      }
    };
    try {
      ensureCurrent();
      return await transport(path, {
        method: "POST", credentials: "same-origin", redirect: "error",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body), signal: controller.signal
      }, async response => {
        ensureCurrent();
        let result;
        try { result = await response.json(); }
        catch (error) {
          if (error?.name === "AbortError") throw error;
          ensureCurrent();
          result = { message: `无法读取服务响应（HTTP ${response.status}）` };
        }
        ensureCurrent();
        if (!response.ok || result?.ok === false)
          throw Object.assign(new Error(result?.message || result?.error || `请求失败（HTTP ${response.status}）`), { code: response.status });
        return result;
      });
    } catch (error) {
      if (error.code === 401 && !error.secondaryChannel) { forgetAuthorization(); onUnauthorized?.(); }
      if (timedOut) throw new Error("服务响应超时，请检查电脑端服务后重试。");
      if (error instanceof TypeError) throw new Error("无法连接电脑端服务，请检查远程连接后重试。");
      throw error;
    } finally {
      clearTimeout(timeout); controllers.delete(controller);
      signal?.removeEventListener("abort", abort);
    }
  }
  async function requestChanges(payload, signal) {
    const path = "/api/workspace/changes";
    if (Date.now() < legacyChangesUntil) return request(path, payload, signal);
    try {
      return await request(path, { ...payload, allRepositories: true }, signal);
    } catch (error) {
      // 手机网页可能先于电脑后台更新；仅对旧版白名单拒绝回退单仓库读取，不吞掉授权及真实读取错误。
      if (error.code !== 400 || error.message !== "只能通过会话或项目标识读取变更") throw error;
      legacyChangesUntil = Date.now() + 30000;
      return request(path, payload, signal);
    }
  }
  function status(element, message, failed = false) {
    element.textContent = message;
    element.classList.toggle("error-text", failed);
  }
  function unavailableChanges() {
    return currentContext().projectId || currentContext().threadId
      ? "" : "请选择一个已有会话，或为新建会话选择项目后查看变更。";
  }
  function targetIdentity(value = currentContext()) {
    if (value.threadId) return ["thread", value.threadId];
    if (value.projectId) return ["project", value.projectId];
    return null;
  }
  function authorizationKey(value = currentContext()) {
    return JSON.stringify([value.deviceId, value.cacheScope]);
  }
  function ownsAuthorization(owner, value = currentContext()) {
    return !disposed && authorized && owner === authorizationKey(value);
  }
  function ownsProject(p) {
    const value = currentContext(), identity = targetIdentity(value);
    return ownsAuthorization(p.owner, value) && project === p && identity !== null
      && p.key === JSON.stringify([value.deviceId, value.cacheScope, ...identity]);
  }
  function mutation(p, kind, id, operation, value) {
    if (!cache || !p.scope || typeof cache[operation] !== "function" || disposed && operation === "put") return;
    const key = JSON.stringify([p.scope, kind, id]);
    let queue = cacheMutations.get(key);
    if (!queue) { queue = { next: null, running: false }; cacheMutations.set(key, queue); }
    queue.next = { p, kind, id, operation, value, generation: p.generation };
    if (queue.running) return;
    const drain = () => {
      queue.running = true;
      void Promise.resolve().then(async () => {
        while (queue.next) {
          const next = queue.next; queue.next = null;
          if (next.p.scope !== currentContext().cacheScope || next.operation === "put"
              && (!authorized || next.p.generation !== next.generation)) continue;
          try {
            if (next.operation === "put") await cache.put(next.p.scope, next.kind, next.id, next.value);
            else await cache.remove(next.p.scope, next.kind, next.id);
          } catch {}
        }
      }).finally(() => {
        queue.running = false;
        if (queue.next) drain();
        else if (cacheMutations.get(key) === queue) cacheMutations.delete(key);
      });
    };
    drain();
  }
  function readStore(p, kind, id, onResult) {
    if (!cache || !p.scope || !authorized || typeof cache.get !== "function") {
      onResult?.(null, false); return Promise.resolve(null);
    }
    return new Promise(resolve => {
      let settled = false;
      const finish = value => {
        if (settled) return;
        settled = true; clearTimeout(timeout); cacheReads.delete(finish); resolve(value);
      };
      const timeout = setTimeout(() => finish(null), 750);
      cacheReads.add(finish);
      const receive = (value, failed = false) => {
        // 读取预算只限制首屏等待；迟到值仍交由调用方按 owner/generation 判断是否可合并。
        if (!disposed && ownsAuthorization(p.owner)) {
          try { onResult?.(value, failed); } catch {}
        }
        finish(value);
      };
      Promise.resolve().then(() => cache.get(p.scope, kind, id)).then(receive, () => receive(null, true));
    });
  }
  function validStored(value, p, kind, id) {
    return objectValue(value) && value.schema === cacheSchema && value.responseVersion === responseVersion
      && value.kind === kind && value.target === p.id && (kind !== "workspace-detail" || value.id === id)
      && validRoot(value.root) && value.root === value.response?.root;
  }
  function persistList(p) {
    clearTimeout(p.saveTimer); p.saveTimer = null;
    if (!p.list || !p.uiReady && p.uiVersion === 0) return;
    mutation(p, "workspace-list", p.id, "put", {
      schema: cacheSchema, responseVersion, kind: "workspace-list", target: p.id, root: p.list.root,
      response: p.list, ui: uiState(p.ui), detailIds: [...p.detailIds]
    });
  }
  function markViewChanged(p) {
    ++p.uiVersion;
    scheduleViewSave(p);
  }
  function scheduleViewSave(p) {
    clearTimeout(p.saveTimer);
    p.saveTimer = setTimeout(() => persistList(p), 500);
  }
  function saveListScroll() {
    if (!project || displayedProject !== project) return;
    project.ui.scrollTop = fileList.scrollTop; markViewChanged(project);
  }
  function saveDetailScroll() {
    if (!detailRecord) return;
    detailRecord.scrollTop = detail.scrollTop;
    detailRecord.scrollLeft = detailRecord.view?.querySelector(".session-tools-diff")?.scrollLeft || 0;
  }
  function saveCurrentView(immediate = false) {
    if (!project) return;
    if (displayedProject === project) {
      project.ui.query = fileSearch.value; project.ui.scrollTop = fileList.scrollTop;
    }
    project.ui.wrap = wrapDiff; saveDetailScroll();
    if (immediate) persistList(project);
    else scheduleViewSave(project);
  }
  function createProject(value, identity) {
    const key = JSON.stringify([value.deviceId, value.cacheScope, ...identity]);
    let p = projects.get(key);
    if (!p) {
      p = { key, id: JSON.stringify(identity), scope: value.cacheScope, owner: authorizationKey(value),
        target: identity[0] === "thread" ? { threadId: identity[1] } : { projectId: identity[1] },
        number: ++projectSequence, generation: 0, networkVersion: 0, uiVersion: 0,
        uiReady: !cache || !value.cacheScope || typeof cache.get !== "function", uiRead: null,
        list: null, listBytes: 0, ui: uiState(), details: new Map(), detailIds: new Set(),
        listView: node("div", "session-tools-file-content"), buttons: new Map(), groups: new Map(),
        rendered: "", searchCollapsed: new Set(), allowRestore: true, saveTimer: null, used: Date.now() };
      projects.set(key, p);
    }
    p.used = Date.now();
    return p;
  }
  function evictProject(p) {
    persistList(p); clearTimeout(p.saveTimer); projects.delete(p.key);
    p.details.clear(); p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
    p.list = null; p.listBytes = 0;
  }
  function trimMemory() {
    while (projects.size > 5) {
      const candidate = [...projects.values()].filter(p => p !== project).sort((a, b) => a.used - b.used)[0];
      if (!candidate) break;
      evictProject(candidate);
    }
    const records = () => [...projects.values()].flatMap(p => [...p.details.entries()].map(([id, record]) => ({ p, id, record })));
    let entries = records();
    let bytes = [...projects.values()].reduce((sum, p) => sum + p.listBytes, 0)
      + entries.reduce((sum, entry) => sum + entry.record.bytes, 0);
    for (const entry of entries.sort((a, b) => a.record.used - b.record.used)) {
      if (entries.length <= 20 && bytes <= memoryLimit) break;
      if (entry.record === detailRecord) continue;
      entry.p.details.delete(entry.id); bytes -= entry.record.bytes;
      entries = entries.filter(value => value !== entry);
    }
    for (const candidate of [...projects.values()].filter(p => p !== project).sort((a, b) => a.used - b.used)) {
      if (bytes <= memoryLimit) break;
      bytes -= candidate.listBytes + [...candidate.details.values()].reduce((sum, record) => sum + record.bytes, 0);
      evictProject(candidate);
    }
    const views = records().filter(entry => entry.record.view).sort((a, b) => b.record.used - a.record.used);
    let kept = detailRecord?.view ? 1 : 0;
    for (const { record } of views) {
      if (record === detailRecord) continue;
      if (kept++ >= 3) { record.view = null; record.stats = null; }
    }
  }
  function summary(p) {
    if (p.list?.repositories) {
      if (!p.list.repositories.length) return p.list.message || "当前项目未发现 Git 仓库。";
      const count = `${p.list.repositories.length} 个仓库 · ${p.list.files.length} 项变更`;
      return p.list.truncated ? `${count}\n${p.list.message || "部分数据未完整读取。"}` : count;
    }
    return p.list?.truncated ? "变更较多，仅显示部分文件。"
      : p.list?.files.length ? `${p.list.files.length} 项变更` : "当前项目没有未提交变更。";
  }
  function attachList(p) {
    if (project !== p) return;
    if (p.list && !p.rendered) { renderFiles(p, true); return; }
    files = p.list?.files || []; changesLoaded = !!p.list;
    fileSearch.value = p.ui.query; wrapDiff = p.ui.wrap;
    wrapButton.setAttribute("aria-pressed", String(wrapDiff));
    detail.classList.toggle("session-tools-diff-wrap", wrapDiff);
    rootLocation.set(p.list?.root || "当前会话工作目录", p.list?.git);
    if (displayedProject !== p) {
      fileList.replaceChildren(p.listView); displayedProject = p;
    }
    fileTriggers.clear(); for (const [id, button] of p.buttons) fileTriggers.set(id, button);
    fileList.scrollTop = p.ui.scrollTop;
    status(changesStatus, p.list ? summary(p) : "正在读取当前项目变更…");
  }
  function forgetDetail(p, id, message = "") {
    const record = p.details.get(id);
    if (project === p && selectedFile && detailKey(selectedFile) === id) {
      ++detailEpoch; detailController?.abort(); detailController = null;
      detailRecord = null; detail.replaceChildren(); detailControls.hidden = true;
      setDetailNotice(message || "该文件已不在变更列表中，请返回列表。", true);
    } else if (detailRecord === record) detailRecord = null;
    p.details.delete(id); p.detailIds.delete(id);
    mutation(p, "workspace-detail", JSON.stringify([p.id, id]), "remove");
  }
  function forgetDetails(p, message) {
    for (const id of new Set([...p.detailIds, ...p.details.keys()])) forgetDetail(p, id, message);
    p.details.clear(); p.detailIds.clear();
    if (project === p && detailVisible) {
      ++detailEpoch; detailController?.abort(); detailController = null;
      detailRecord = null; detail.replaceChildren(); detailControls.hidden = true;
      setDetailNotice(message || "文件变更已失效，请返回列表。", true);
    }
  }
  function invalidateProject(p, message) {
    ++p.generation; p.allowRestore = false;
    p.uiReady = true; p.uiRead = null;
    forgetDetails(p, message); p.list = null; p.listBytes = 0; p.rendered = "";
    clearTimeout(p.saveTimer); p.saveTimer = null;
    p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
    mutation(p, "workspace-list", p.id, "remove");
    if (project === p) {
      files = []; changesLoaded = false; fileTriggers.clear();
      rootLocation.set("当前会话工作目录");
      status(changesStatus, message, true);
    }
  }
  function forgetAuthorization() {
    authorized = false; ++changesEpoch; ++detailEpoch;
    changesController?.abort(); detailController?.abort(); changesBusy = false;
    for (const finish of [...cacheReads]) finish(null);
    for (const p of projects.values()) invalidateProject(p, "登录已失效，请重新登录。");
    projects.clear(); files = []; detailRecord = null; selectedFile = null;
    fileList.replaceChildren(); detail.replaceChildren(); detailControls.hidden = true;
    setDetailNotice("登录已失效，请重新登录。", true);
  }
  function updateContext() {
    if (disposed) return;
    projectFiles?.updateContext();
    const value = currentContext(), identity = targetIdentity(value);
    const key = JSON.stringify([value.deviceId, value.cacheScope, identity]);
    if (key === contextKey) return;
    saveCurrentView(true);
    if (detailVisible) hideDetail({ focus: false });
    ++changesEpoch; ++detailEpoch; changesController?.abort(); detailController?.abort();
    changesController = null; detailController = null; changesBusy = false;
    const oldScope = project?.scope;
    if (oldScope !== undefined && oldScope !== value.cacheScope) {
      for (const p of projects.values()) {
        clearTimeout(p.saveTimer);
        p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
        for (const record of p.details.values()) { record.view = null; record.stats = null; }
        p.details.clear();
      }
      projects.clear();
    }
    contextKey = key; authorized = true;
    project = identity ? createProject(value, identity) : null;
    files = []; selectedFile = null; detailRecord = null; detail.replaceChildren(); detailControls.hidden = true;
    setDetailNotice("");
    if (project) attachList(project);
    else {
      fileSearch.value = ""; fileList.replaceChildren(); displayedProject = null; changesLoaded = false;
      rootLocation.set("当前会话工作目录");
      status(changesStatus, unavailableChanges());
    }
    trimMemory();
    if (opened && project) void refreshChanges();
  }
  async function restoreList(p, current) {
    if (!p.allowRestore || p.uiRead || p.list && p.uiReady) return;
    const token = { uiVersion: p.uiVersion, networkVersion: p.networkVersion, generation: p.generation };
    p.uiRead = token;
    await readStore(p, "workspace-list", p.id, (saved, failed) => {
      if (p.uiRead !== token) return;
      p.uiRead = null;
      if (failed || projects.get(p.key) !== p || p.generation !== token.generation || !p.allowRestore) return;
      p.uiReady = true;
      const response = saved && validStored(saved, p, "workspace-list") && listResponse(saved.response);
      if (!response) {
        if (saved && p.networkVersion === token.networkVersion) mutation(p, "workspace-list", p.id, "remove");
        persistList(p); return;
      }
      const restoreData = current() && p.networkVersion === token.networkVersion && !p.list;
      const restoreUi = p.uiVersion === token.uiVersion && (!p.list || p.list.root === response.root);
      if (restoreUi) p.ui = uiState(saved.ui);
      if (restoreData) {
        p.list = response; p.listBytes = responseBytes(response);
        if (Array.isArray(saved.detailIds)) for (const id of saved.detailIds.slice(0, 64)) {
          try {
            const [repository, section, path] = JSON.parse(id);
            if (detailKey({ repository, section, path }) === id) {
              if (response.files.some(file => detailKey(file) === id)) p.detailIds.add(id);
              else mutation(p, "workspace-detail", JSON.stringify([p.id, id]), "remove");
            }
          } catch {}
        }
      }
      if (restoreUi || restoreData) {
        if (ownsProject(p)) {
          const failure = changesStatus.classList.contains("error-text") ? changesStatus.textContent : "";
          renderFiles(p, true); attachList(p);
          if (failure) status(changesStatus, restoreData && !failure.includes("正在显示上次缓存")
            ? `${summary(p)}\n正在显示上次缓存：${failure}` : failure, true);
        } else if (project !== p) renderFiles(p, true);
        else p.rendered = "";
        trimMemory();
      }
      if (p.networkVersion !== token.networkVersion || p.uiVersion !== token.uiVersion) persistList(p);
    });
  }
  function acceptList(p, response) {
    const previous = p.list;
    ++p.networkVersion;
    if (previous?.root !== undefined && previous.root !== response.root) {
      ++p.generation;
      p.uiReady = true; p.uiRead = null;
      forgetDetails(p, "工作目录已变化，请返回列表后重新打开文件。");
    }
    const ids = new Set(response.files.map(detailKey));
    for (const id of new Set([...p.detailIds, ...p.details.keys()])) if (!ids.has(id)) forgetDetail(p, id);
    if (project === p && selectedFile && !ids.has(detailKey(selectedFile)))
      forgetDetail(p, detailKey(selectedFile));
    const unchanged = previous?.revision === response.revision && previous.root === response.root;
    p.list = response; p.listBytes = responseBytes(response); p.allowRestore = true;
    if (selectedFile && project === p) selectedFile = response.files.find(file => detailKey(file) === detailKey(selectedFile)) || selectedFile;
    if (!unchanged) renderFiles(p);
    attachList(p);
    if (!unchanged) persistList(p);
    trimMemory();
  }
  async function refreshChanges() {
    const p = project;
    if (disposed || !authorized || !p) { status(changesStatus, unavailableChanges()); return; }
    const epoch = ++changesEpoch, generation = p.generation;
    changesController?.abort();
    const controller = new AbortController(); changesController = controller; changesBusy = true;
    const current = () => ownsProject(p) && p.generation === generation
      && changesEpoch === epoch && !controller.signal.aborted;
    if (p.list) attachList(p);
    else status(changesStatus, "正在读取当前项目变更…");
    try {
      await restoreList(p, current);
      if (!current()) return;
      const cached = p.list;
      const payload = { ...p.target, ...(cached ? { ifRevision: cached.revision } : {}) };
      let result = await requestChanges(payload, controller.signal);
      if (!current()) return;
      if (result?.notModified === true) {
        if (cached && validRoot(result.root) && result.root !== cached.root) {
          invalidateProject(p, "工作目录已变化，正在重新读取变更。");
          void refreshChanges(); return;
        }
        if (cached && result.revision === cached.revision && result.root === cached.root && p.list === cached) {
          ++p.networkVersion; status(changesStatus, summary(p)); return;
        }
        result = await requestChanges(p.target, controller.signal);
        if (!current()) return;
      }
      const response = listResponse(result);
      if (!response) throw new Error("变更列表响应无效，请重新打开变更页。");
      acceptList(p, response);
    } catch (error) {
      if (error?.name === "AbortError" || !current()) return;
      if ([403, 404, 409, 422].includes(error.code)) invalidateProject(p, `读取变更失败：${error.message}`);
      else status(changesStatus, p.list ? `${summary(p)}
正在显示上次缓存：${error.message}` : `读取变更失败：${error.message}`, true);
    } finally {
      if (!disposed && changesEpoch === epoch) {
        changesBusy = false;
        if (changesController === controller) changesController = null;
      }
    }
  }
  function searchFiles() {
    if (!project) return;
    project.ui.query = fileSearch.value; project.ui.scrollTop = 0; project.searchCollapsed.clear();
    renderFiles(project); fileList.scrollTop = 0; markViewChanged(project);
  }
  function applyFold(p, section) {
    const group = p.groups.get(section);
    if (!group) return;
    const expanded = p.ui.query.trim() ? !p.searchCollapsed.has(section) : !p.ui.collapsed[section];
    group.toggle.setAttribute("aria-expanded", String(expanded)); group.body.hidden = !expanded;
  }
  function renderFiles(p = project, restoreScroll = false) {
    if (!p) return;
    const query = p.ui.query.trim().toLocaleLowerCase();
    const signature = JSON.stringify([p.list?.revision || "", query]);
    if (signature === p.rendered) {
      for (const key of p.groups.keys()) applyFold(p, key);
      return;
    }
    const scrollTop = !restoreScroll && project === p && displayedProject === p ? fileList.scrollTop : p.ui.scrollTop;
    p.rendered = signature; p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
    const source = p.list?.files || [];
    const visible = source.filter(file => !query
      || `${displayedPath(file)} ${displayWorkspacePath(file.oldPath)} ${file.path} ${file.oldPath || ""}`.toLocaleLowerCase().includes(query));
    if (source.length && !visible.length) p.listView.append(node("p", "session-tools-empty", "没有匹配的变更文件。"));
    const repositories = p.list?.repositories || [{ path: "", root: p.list?.root || "" }];
    for (const repository of repositories) {
      const repositoryFiles = visible.filter(file => (file.repository || "") === repository.path);
      if (!repositoryFiles.length && !repository.error) continue;
      let host = p.listView;
      if (repositories.length > 1 || repository.path || repository.error) {
        host = node("section", "session-tools-repository");
        const heading = node("h2", "session-tools-repository-heading");
        heading.append(node("span", "session-tools-repository-label",
          `${displayWorkspaceName(repository.path || repository.root.split(/[\\/]/).filter(Boolean).at(-1)) || "当前仓库"} · ${repositoryFiles.length}`),
        createWorkspaceBranch(repository.git));
        heading.title = displayWorkspacePath(repository.root);
        host.append(heading);
        if (repository.error) host.append(node("p", "session-tools-status error-text", repository.error));
        else if (repository.truncated) host.append(node("p", "session-tools-warning", "此仓库仅显示部分变更。"));
        p.listView.append(host);
      }
      for (const [section, label] of sections) {
        const key = groupKey(repository.path, section);
        const entries = repositoryFiles.filter(file => file.section === section);
        if (!entries.length) continue;
        const group = node("section", "session-tools-file-group"), title = node("h3");
        const body = node("div", "session-tools-file-group-body");
        body.id = `session-tools-group-${navigationOwner}-${p.number}-${p.groups.size}`;
        const toggle = control("", `${label} · ${entries.length}`, () => {
          if (project !== p || disposed) return;
          if (p.ui.query.trim()) {
            if (p.searchCollapsed.has(key)) p.searchCollapsed.delete(key);
            else p.searchCollapsed.add(key);
          } else p.ui.collapsed[key] = !p.ui.collapsed[key];
          applyFold(p, key); p.ui.scrollTop = fileList.scrollTop; markViewChanged(p);
        }, "session-tools-group-toggle");
        toggle.setAttribute("aria-controls", body.id);
        const chevron = node("span", "session-tools-group-chevron", "›"); chevron.setAttribute("aria-hidden", "true");
        toggle.append(chevron, node("span", "session-tools-group-label", `${label} · ${entries.length}`));
        title.append(toggle); group.append(title, body); p.groups.set(key, { toggle, body });
        applyFold(p, key);
        for (const file of entries) {
          const button = control("", `${label}：${displayedPath(file)}`, () => {
            if (embedded?.onOpenChange) embedded.onOpenChange(file, p.list);
            else void loadDetail(file);
          }, "session-tools-file");
          // 差异窗口须保留仓库及暂存分区，不能把多仓库同名文件或已删除文件替换成工作区文件预览。
          if (typeof onFileContextMenu === "function") button.addEventListener("contextmenu", event => {
            if (ownsProject(p)) onFileContextMenu(event, { kind: "change", path: file.path,
              section: file.section, repository: file.repository || "" }, { ...getContext() });
          });
          const parts = fileParts(file), copy = node("span", "session-tools-file-copy");
          const directory = file.oldPath ? `${parts.directory} · 原路径：${displayWorkspacePath(file.oldPath)}` : parts.directory;
          copy.append(node("strong", "session-tools-file-name", parts.name), node("span", "session-tools-file-directory", directory));
          copy.title = file.oldPath ? `${displayWorkspacePath(file.oldPath)} → ${displayedPath(file)}` : displayedPath(file);
          const badge = node("span", "session-tools-file-status", statusLabel(file.status)); badge.setAttribute("data-status", file.status);
          button.append(node("span", "session-tools-file-icon", "▤"), copy, badge, node("span", "session-tools-file-chevron", "›"));
          p.buttons.set(detailKey(file), button); body.append(button);
        }
        host.append(group);
      }
    }
    p.ui.scrollTop = scrollTop;
    if (project === p) attachList(p);
  }
  function fileParts(file) {
    // 与文件树保持一致：先按真实分隔符分层，再还原显示字符，不能把文件名中的映射反斜杠拆成目录。
    const parts = String(file.path).split(/[\\/]/);
    return { name: displayWorkspaceName(parts.pop() || file.path), directory: displayWorkspacePath(parts.join("/")) || "项目根目录" };
  }
  function setDetailNotice(message, failed = false) {
    status(detailNotice, message, failed); detailNotice.hidden = !message;
  }
  function rememberDetail(p, file, response, persist = true) {
    const id = detailKey(file), previous = p.details.get(id);
    if (previous?.response.revision === response.revision && previous.response.root === response.root) return previous;
    const record = { response, bytes: responseBytes(response), view: null, stats: null, used: Date.now(),
      scrollTop: previous?.scrollTop || 0, scrollLeft: previous?.scrollLeft || 0 };
    p.details.set(id, record); p.detailIds.add(id);
    if (persist) {
      mutation(p, "workspace-detail", JSON.stringify([p.id, id]), "put", {
        schema: cacheSchema, responseVersion, kind: "workspace-detail", target: p.id, id,
        root: response.root, response
      });
      scheduleViewSave(p);
    }
    while (p.detailIds.size > 64) {
      const oldest = p.detailIds.values().next().value;
      p.detailIds.delete(oldest);
      mutation(p, "workspace-detail", JSON.stringify([p.id, oldest]), "remove");
    }
    return record;
  }
  function paintDetail(record, file) {
    if (detailRecord !== record) saveDetailScroll();
    record.used = Date.now();
    if (record.response.git) setWorkspaceBranch(detailLocation.branch, record.response.git);
    if (!record.view) {
      const result = record.response, view = node("div", "session-tools-detail-content");
      if (result.truncated) view.append(node("p", "session-tools-warning", result.message || "文件较大，差异或预览已截断。"));
      else if (result.message && ["text", "image"].includes(result.kind))
        view.append(node("p", "session-tools-hint", result.message));
      if (result.kind === "text") record.stats = renderDiff(result.diff || "", view);
      else if (result.kind === "image") renderImages(result, file, view);
      else {
        view.append(node("p", "session-tools-status", result.message || (result.kind === "binary"
          ? "二进制文件，无法进行文本比较。" : "该文件类型暂不支持预览。")));
        renderMetadata(result, view);
      }
      record.view = view;
    }
    if (detail.firstElementChild !== record.view) {
      detail.replaceChildren(record.view);
      detail.scrollTop = record.scrollTop;
      const output = record.view.querySelector(".session-tools-diff");
      if (output) output.scrollLeft = record.scrollLeft;
      diffStats.replaceChildren(...(record.stats || []));
      detailControls.hidden = !record.stats;
    }
    detailRecord = record;
    trimMemory();
  }
  async function loadDetail(file, { recordHistory = true } = {}) {
    const p = project;
    if (disposed || !authorized || !p) return;
    file = p.list?.files.find(entry => detailKey(entry) === detailKey(file));
    if (!file) return;
    saveCurrentView();
    const epoch = ++detailEpoch, generation = p.generation, id = detailKey(file);
    const signature = JSON.stringify([file.status, file.oldPath || ""]);
    detailController?.abort();
    const controller = new AbortController(); detailController = controller;
    const current = () => !disposed && authorized && opened && detailVisible && ownsProject(p)
      && p.generation === generation && epoch === detailEpoch && !controller.signal.aborted;
    if (!detailVisible || !selectedFile || detailKey(selectedFile) !== id) detailControlsScroll.reset();
    selectedFile = file;
    const parts = fileParts(file);
    detailTitle.textContent = parts.name; detailTitle.title = displayedPath(file);
    const repository = p.list?.repositories?.find(repository => repository.path === (file.repository || ""));
    detailLocation.set(file.repository ? `${file.repository} / ${parts.directory}` : parts.directory,
      repository?.git || p.list?.git);
    let cached = p.details.get(id);
    if (cached && cached.response.root !== p.list.root) {
      p.details.delete(id); p.detailIds.delete(id);
      mutation(p, "workspace-detail", JSON.stringify([p.id, id]), "remove"); cached = null;
    }
    if (cached) { paintDetail(cached, file); setDetailNotice(""); }
    else {
      saveDetailScroll(); detailRecord = null;
      detailControls.hidden = true; detail.replaceChildren(); setDetailNotice("正在读取文件差异…");
    }
    showDetail({ recordHistory });
    try {
      if (!cached && p.allowRestore) {
        const saved = await readStore(p, "workspace-detail", JSON.stringify([p.id, id]));
        if (!current()) return;
        const response = validStored(saved, p, "workspace-detail", id) && detailResponse(saved.response, file);
        if (response && response.root === p.list?.root) {
          cached = rememberDetail(p, file, response, false); paintDetail(cached, file); setDetailNotice("");
        } else if (saved) mutation(p, "workspace-detail", JSON.stringify([p.id, id]), "remove");
      }
      if (!current()) return;
      const payload = { ...p.target, path: file.path, section: file.section,
        ...(file.repository != null ? { repository: file.repository } : {}) };
      let result = await request("/api/workspace/change",
        { ...payload, ...(cached ? { ifRevision: cached.response.revision } : {}) }, controller.signal);
      if (!current()) return;
      if (validRoot(result?.root) && result.root !== p.list?.root) {
        invalidateProject(p, "工作目录已变化，请返回列表后重新打开文件。");
        setDetailNotice("工作目录已变化，请返回列表后重新打开文件。", true);
        void refreshChanges(); return;
      }
      const latestFile = p.list?.files.find(entry => detailKey(entry) === id);
      if (!latestFile) { forgetDetail(p, id); return; }
      if (JSON.stringify([latestFile.status, latestFile.oldPath || ""]) !== signature) {
        void loadDetail(latestFile, { recordHistory: false }); return;
      }
      if (result?.notModified === true) {
        if (cached && result.revision === cached.response.revision && result.root === cached.response.root
            && result.root === p.list.root) { setDetailNotice(""); return; }
        result = await request("/api/workspace/change", payload, controller.signal);
        if (!current()) return;
      }
      const response = detailResponse(result, file);
      if (!response) throw new Error("文件差异响应无效，请重新打开文件。");
      if (response.root !== p.list?.root) {
        invalidateProject(p, "工作目录已变化，请返回列表后重新打开文件。");
        setDetailNotice("工作目录已变化，请返回列表后重新打开文件。", true);
        void refreshChanges(); return;
      }
      saveDetailScroll();
      const record = rememberDetail(p, file, response);
      paintDetail(record, file); setDetailNotice("");
    } catch (error) {
      if (error?.name === "AbortError" || !current()) return;
      if ([403, 404, 409, 422].includes(error.code)) {
        invalidateProject(p, `读取差异失败：${error.message}`); setDetailNotice(`读取差异失败：${error.message}`, true);
      } else if (cached && p.details.get(id) === cached) {
        setDetailNotice(`正在显示上次缓存：${error.message}`, true);
      } else {
        setDetailNotice(`读取差异失败：${error.message}`, true);
        detail.replaceChildren(
          control("重试", "重新读取该文件差异", () => void loadDetail(file), "text-button"));
      }
    } finally {
      if (detailController === controller) detailController = null;
    }
  }
  function renderDiff(diff, host) {
    if (!diff) { host.append(node("p", "session-tools-status", "没有可显示的文本差异。")); return null; }
    const output = node("div", "session-tools-diff");
    output.tabIndex = 0; output.setAttribute("aria-label", "文本差异，可横向滚动");
    let before = null, after = null, additions = 0, deletions = 0, lineNumberDigits = 2;
    const lines = String(diff).split("\n");
    if (lines.at(-1) === "") lines.pop();
    const fragment = document.createDocumentFragment();
    for (const line of lines) {
      const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      let kind = "meta", lineNumber = "";
      if (hunk) { before = Number(hunk[1]); after = Number(hunk[2]); kind = "hunk"; }
      else if (before !== null && line.startsWith("+")) { kind = "add"; lineNumber = String(after++); ++additions; }
      else if (before !== null && line.startsWith("-")) { kind = "delete"; lineNumber = String(before++); ++deletions; }
      else if (before !== null && line.startsWith(" ")) {
        kind = "context"; ++before; lineNumber = String(after++);
      }
      const row = node("div", `session-tools-diff-line ${kind}`);
      if (lineNumber) {
        lineNumberDigits = Math.max(lineNumberDigits, lineNumber.length);
        row.append(node("span", "session-tools-line-number", lineNumber));
      }
      row.append(node("code", "", line));
      fragment.append(row);
    }
    output.style.setProperty("--line-number-width", `${lineNumberDigits + 1.2}ch`);
    output.append(fragment); host.append(output);
    return [node("span", "session-tools-additions", `+${additions}`),
      node("span", "session-tools-deletions", `−${deletions}`)];
  }
  function renderImages(result, file, host) {
    const comparison = node("div", "session-tools-image-comparison");
    for (const [side, label] of [["before", "修改前"], ["after", "修改后"]]) {
      const value = result[side];
      const source = safeInlineImageSource(value?.dataUrl, { allowSvg: true });
      const figure = node("figure");
      figure.append(node("figcaption", "", label));
      if (value?.exists === false) figure.append(node("p", "session-tools-empty", "此侧不存在文件"));
      else if (imageSource(value?.dataUrl) && source) {
        const link = node("a", "session-tools-image-link");
        link.href = source;
        const image = node("img");
        image.alt = `${displayWorkspacePath(file.path)} · ${label}`; image.src = source;
        link.append(image);
        link.addEventListener("click", event => {
          event.preventDefault(); event.stopPropagation();
          openImagePreview?.(link, image);
        });
        image.addEventListener("error", () => {
          link.replaceWith(node("p", "session-tools-status error-text", "图片无法解码，请刷新后重试。"));
        }, { once: true });
        figure.append(link, node("small", "muted", `${value.mime || "图片"} · ${byteSize(value.size)}`));
      } else figure.append(node("p", "session-tools-status", "图片预览不可用，可能超出读取大小限制。"));
      comparison.append(figure);
    }
    host.append(comparison);
  }
  function renderMetadata(result, host) {
    for (const [side, label] of [["before", "修改前"], ["after", "修改后"]]) {
      const value = result[side];
      if (!value) continue;
      host.append(node("p", "session-tools-hint", value.exists === false ? `${label}：文件不存在`
        : `${label}：${value.mime || value.type || "二进制文件"} · ${byteSize(value.size)}`));
    }
  }
  function restoreInert() {
    for (const [element, value] of inertState) element.inert = value;
    inertState.clear();
  }
  function suspendConversation() {
    if (embedded) return;
    for (const element of workspace.querySelectorAll(":scope > .conversation, :scope > .sidebar")) {
      if (!inertState.has(element)) inertState.set(element, element.inert);
      element.inert = true;
    }
  }
  function recordRoute(route, file = null) {
    if (embedded) return;
    history.pushState({
      ...(history.state || {}),
      [historyKey]: { owner: navigationOwner, route, page: activePage, context: contextKey,
        ...(file ? { path: file.path, section: file.section, repository: file.repository || "" } : {}) }
    }, "", location.href);
  }
  function openPage(name, options = {}) {
    if (disposed || !pageTitles[name]) return;
    beforeOpen?.();
    showPage(name, options);
  }
  function showPage(name, { recordHistory = true, refresh = true } = {}) {
    if (disposed || !pageTitles[name]) return;
    const wasOpened = opened;
    const enteringChanges = name === "changes" && (!wasOpened || activePage !== "changes");
    if (!wasOpened && !returnView) {
      const messages = workspace.querySelector("#messages");
      returnView = { messages, scrollTop: messages?.scrollTop || 0, context: contextKey };
    }
    clearTimeout(pageExitTimer); pageExitTimer = null;
    opened = true; activePage = name;
    if (detailVisible) hideDetail({ focus: false });
    pageTitle.textContent = pageTitles[name];
    rootLocation.element.hidden = name !== "changes";
    for (const [key, body] of pageBodies) body.hidden = key !== name;
    panel.hidden = false; panel.inert = false;
    suspendConversation();
    if (!wasOpened) panel.getBoundingClientRect();
    panel.classList.add("session-tools-visible");
    if (recordHistory) recordRoute(name);
    updateContext();
    if (refresh && name === "changes" && !changesBusy && changeTarget() && (embedded || enteringChanges || !changesLoaded))
      void refreshChanges();
    if (!embedded) backToConversation.focus({ preventScroll: true });
  }
  function finishPageExit() {
    if (disposed || opened) return;
    clearTimeout(pageExitTimer); pageExitTimer = null;
    panel.hidden = true;
    detailPanel.hidden = true;
    restoreInert();
    if (returnView?.context === contextKey && returnView.messages?.isConnected)
      returnView.messages.scrollTop = returnView.scrollTop;
    returnView = null;
    if (!embedded && isAvailable() && trigger?.isConnected) trigger.focus({ preventScroll: true });
  }
  function hidePage() {
    if (disposed || !opened) return;
    saveCurrentView(true);
    opened = false;
    if (detailVisible) hideDetail({ focus: false });
    ++changesEpoch; changesBusy = false;
    changesController?.abort(); detailController?.abort();
    changesController = null; detailController = null;
    panel.inert = true;
    panel.classList.remove("session-tools-visible");
    clearTimeout(pageExitTimer);
    pageExitTimer = setTimeout(finishPageExit, 250);
  }
  function showDetail({ recordHistory = true } = {}) {
    const wasVisible = detailVisible;
    clearTimeout(detailExitTimer); detailExitTimer = null;
    detailVisible = true;
    detailPanel.hidden = false; detailPanel.inert = false; panel.inert = true;
    if (embedded) panel.hidden = true;
    if (!wasVisible) {
      detail.scrollTop = detailRecord?.scrollTop || 0;
      detailPanel.getBoundingClientRect();
    }
    detailPanel.classList.add("session-tools-visible");
    if (recordHistory && !wasVisible) recordRoute("detail", selectedFile);
    if (!embedded) backToFiles.focus({ preventScroll: true });
  }
  function finishDetailExit(focus) {
    if (disposed || detailVisible) return;
    clearTimeout(detailExitTimer); detailExitTimer = null;
    detailPanel.hidden = true;
    panel.inert = !opened;
    if (embedded) panel.hidden = !opened;
    if (focus && opened) {
      const selected = selectedFile && fileTriggers.get(detailKey(selectedFile));
      (selected || backToConversation).focus({ preventScroll: true });
    }
  }
  function hideDetail({ focus = true } = {}) {
    if (disposed || !detailVisible) return;
    saveCurrentView();
    detailVisible = false; ++detailEpoch; detailController?.abort(); detailController = null;
    document.querySelector("#image-preview-dialog")?.close();
    detailPanel.inert = true;
    detailPanel.classList.remove("session-tools-visible");
    clearTimeout(detailExitTimer);
    detailExitTimer = setTimeout(() => finishDetailExit(focus), 250);
  }
  function requestBack() {
    if (disposed || !opened) return;
    if (embedded) { embedded.onClose(); return; }
    const ownsEntry = history.state?.[historyKey]?.owner === navigationOwner;
    if (detailVisible) hideDetail();
    else hidePage();
    if (ownsEntry) history.back();
  }
  function handlePopState(event) {
    if (disposed || embedded || !isAvailable()) return;
    const route = event.state?.[historyKey];
    if (route?.owner !== navigationOwner || route.context !== contextKey) {
      if (opened) hidePage();
      return;
    }
    beforeOpen?.();
    if (route.route === "detail") {
      if (!opened) showPage(route.page, { recordHistory: false });
      const file = files.find(entry => detailKey(entry) === detailKey(route));
      if (file && (!detailVisible || selectedFile !== file)) void loadDetail(file, { recordHistory: false });
    } else if (pageTitles[route.route]) {
      if (detailVisible) hideDetail();
      if (!opened || activePage !== route.route) showPage(route.route, { recordHistory: false });
    }
  }
  function handleNativeBack(event) {
    if (disposed || embedded || !isAvailable() || !opened || event.defaultPrevented) return;
    const imageDialog = document.querySelector("#image-preview-dialog[open]");
    if (imageDialog) { event.preventDefault(); imageDialog.close(); return; }
    if (document.querySelector("dialog[open]")) return;
    event.preventDefault(); requestBack();
  }
  function handleKey(event) {
    if (!opened || disposed || embedded || !isAvailable() || document.querySelector("dialog[open]")) return;
    if (event.key === "Escape") {
      event.preventDefault(); event.stopImmediatePropagation(); requestBack();
    } else if (event.key === "Tab") {
      const currentPanel = detailVisible ? detailPanel : panel;
      const focusable = [...currentPanel.querySelectorAll("button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex='0']")]
        .filter(element => element.tabIndex >= 0 && !element.closest("[hidden],[inert]") && element.getClientRects().length);
      const first = focusable[0], last = focusable.at(-1), active = document.activeElement;
      if (event.shiftKey && (active === first || !currentPanel.contains(active))) {
        event.preventDefault(); last?.focus({ preventScroll: true });
      } else if (!event.shiftKey && (active === last || !currentPanel.contains(active))) {
        event.preventDefault(); first?.focus({ preventScroll: true });
      }
    }
  }
  panel.addEventListener("transitionend", event => {
    if (event.target === panel && event.propertyName === "transform") finishPageExit();
  });
  document.addEventListener("keydown", handleKey, true);
  window.addEventListener("popstate", handlePopState, true);
  window.addEventListener("codex-elves-native-back", handleNativeBack);
  if (!embedded) projectFiles = mountSessionFiles({
    workspace, trigger, getContext, beforeOpen, onUnauthorized, openImagePreview, copyText,
    request: transport, isAvailable, onFileContextMenu
  });
  updateContext();

  return {
    updateContext,
    open: name => name === "files" ? projectFiles?.open() : openPage(name),
    openFile: (path, line) => projectFiles?.openFile(path, line),
    async openChange(file, snapshot) {
      showPage("changes", { recordHistory: false, refresh: false });
      const list = listResponse(snapshot);
      if (list && project) acceptList(project, list);
      else await refreshChanges();
      if (disposed) return;
      if (files.some(entry => detailKey(entry) === detailKey(file))) await loadDetail(file);
      else {
        detailTitle.textContent = fileParts(file).name;
        showDetail({ recordHistory: false });
        setDetailNotice("该文件已不在变更列表中，请重新打开变更列表。", true);
      }
    },
    hide() {
      projectFiles?.close();
      if (opened) { hidePage(); finishPageExit(); }
    },
    close: requestBack,
    dispose() {
      if (disposed) return;
      detailControlsScroll.dispose();
      projectFiles?.dispose();
      saveCurrentView(true);
      disposed = true; opened = false; detailVisible = false; ++changesEpoch; ++detailEpoch;
      changesController?.abort(); detailController?.abort();
      for (const controller of controllers) controller.abort();
      clearTimeout(pageExitTimer); clearTimeout(detailExitTimer); restoreInert();
      for (const finish of [...cacheReads]) finish(null);
      for (const p of projects.values()) {
        clearTimeout(p.saveTimer);
        p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
        for (const record of p.details.values()) { record.view = null; record.stats = null; }
        p.details.clear();
      }
      document.removeEventListener("keydown", handleKey, true);
      window.removeEventListener("popstate", handlePopState, true);
      window.removeEventListener("codex-elves-native-back", handleNativeBack);
      if (history.state?.[historyKey]?.owner === navigationOwner) {
        const state = { ...history.state }; delete state[historyKey];
        history.replaceState(state, "", location.href);
      }
      panel.remove(); detailPanel.remove();
      fileList.replaceChildren(); detail.replaceChildren(); diffStats.replaceChildren();
      fileSearch.removeEventListener("input", searchFiles);
      fileList.removeEventListener("scroll", saveListScroll);
      projects.clear(); fileTriggers.clear(); files = [];
      project = null; displayedProject = null; detailRecord = null;
    }
  };
}
