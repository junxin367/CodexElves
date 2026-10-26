import { requestRemote } from "./channels.js";
import { markdown } from "./markdown.js";

const node = (tag, className = "", text = "") => {
  const result = document.createElement(tag);
  result.className = className; result.textContent = text;
  return result;
};
const control = (label, action, className = "text-button") => {
  const result = node("button", className, label);
  result.type = "button"; result.addEventListener("click", action);
  return result;
};
const parentPath = path => path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
const fileName = path => path.replaceAll("\\", "/").split("/").at(-1) || "文件";
const revisionValid = value => typeof value === "string" && /^[a-f0-9]{64}$/u.test(value);
const imageValid = value => typeof value === "string"
  && /^data:image\/(?:png|jpeg|gif|webp);base64,[a-zA-Z0-9+/=]+$/u.test(value);
const bytesLabel = size => size < 1024 ? `${size} B`
  : size < 1024 * 1024 ? `${(size / 1024).toFixed(1)} KiB` : `${(size / 1024 / 1024).toFixed(1)} MiB`;
const fileSort = (left, right) => {
  const leftDirectory = left.kind === "directory" ? 0 : 1;
  const rightDirectory = right.kind === "directory" ? 0 : 1;
  return leftDirectory - rightDirectory
    || left.name.localeCompare(right.name, "zh-CN", { numeric: true, sensitivity: "base" })
    || left.path.localeCompare(right.path, "zh-CN", { numeric: true, sensitivity: "base" });
};
const sortedFiles = files => [...(files || [])].sort(fileSort);
const fileIconKind = path => {
  const name = fileName(path).toLowerCase(), lower = path.toLowerCase();
  if (name === "package.json" || name === "tsconfig.json" || name === "cargo.toml") return "package";
  if (name === "package-lock.json" || name === "pnpm-lock.yaml" || name === "yarn.lock"
      || name === "cargo.lock") return "lock";
  if (name === "dockerfile" || name.startsWith(".dockerfile")) return "docker";
  if (name === ".gitignore" || name === ".gitattributes" || name === ".editorconfig") return "git";
  if (/\.(?:md|mdx|rst|txt|log)$/u.test(lower)) return "document";
  if (/\.(?:png|jpe?g|gif|webp|avif|bmp|ico|svg)$/u.test(lower)) return "image";
  if (/\.(?:css|scss|sass|less|styl)$/u.test(lower)) return "style";
  if (/\.(?:html?|xhtml)$/u.test(lower)) return "html";
  if (/\.(?:js|jsx|mjs|cjs)$/u.test(lower)) return "javascript";
  if (/\.(?:ts|tsx)$/u.test(lower)) return "typescript";
  if (/\.(?:vue|svelte|astro)$/u.test(lower)) return "component";
  if (/\.json$/u.test(lower)) return "json";
  if (/\.(?:ya?ml|toml|ini|conf|config|properties|env)$/u.test(lower)) return "config";
  if (/\.(?:rs)$/u.test(lower)) return "rust";
  if (/\.(?:py)$/u.test(lower)) return "python";
  if (/\.(?:java|kt|kts)$/u.test(lower)) return "java";
  if (/\.(?:go)$/u.test(lower)) return "go";
  if (/\.(?:c|h|cc|cpp|hpp|cs|swift)$/u.test(lower)) return "native";
  if (/\.(?:sh|bash|zsh|ps1|bat|cmd)$/u.test(lower)) return "shell";
  if (/\.(?:sql|graphql|gql|proto)$/u.test(lower)) return "data";
  return "file";
};
const fileIconPaths = {
  folder: '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h4.5A1.5 1.5 0 0 1 17 8.5v7A1.5 1.5 0 0 1 15.5 17h-11A1.5 1.5 0 0 1 3 15.5Z"/>',
  document: '<path d="M5 2.5h6l4 4v11H5z"/><path d="M11 2.5v4h4M7.5 11h5M7.5 14h4"/>',
  file: '<path d="M5 2.5h6l4 4v11H5z"/><path d="M11 2.5v4h4"/>',
  image: '<path d="M4 3h12v14H4z"/><circle cx="8" cy="8" r="1.2"/><path d="m5.5 14 3.5-3 2.2 2 1.5-1.5 2.3 2.5"/>',
  javascript: '<path d="M4 3h12v14H4z"/><path d="M8 13.5c.5 1 2.5 1 2.5-.2 0-1.6-2.5-.8-2.5-2.4 0-1.2 2-1.5 2.8-.5M12.5 10.5h2M13.5 10.5v4"/>',
  typescript: '<path d="M4 3h12v14H4z"/><path d="M7 11h6M10 11v4M13 13c0 1.6-2.5 1.6-2.5 0"/>',
  style: '<path d="M4 3h12v14H4z"/><path d="m7 7 3 3-3 3M11.5 13H14"/>',
  html: '<path d="M4 3h12v14H4z"/><path d="m7 8-2 2 2 2M13 8l2 2-2 2M11.5 6l-3 8"/>',
  config: '<path d="M4 3h12v14H4z"/><path d="M7 7h6M7 10h6M7 13h4"/>',
  package: '<path d="m4 6 6-3 6 3-6 3zM4 6v8l6 3 6-3V6M10 9v8"/>',
  lock: '<path d="M5 8h10v9H5zM7 8V6a3 3 0 0 1 6 0v2"/>',
  git: '<path d="M10 3a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM5 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4Zm10 0a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM7 10h6M10 7v6"/>',
  docker: '<path d="M3 12h13a3 3 0 0 1-3 3H7a4 4 0 0 1-4-3ZM6 8h2v2H6zM9 8h2v2H9zM12 8h2v2h-2zM8 5h2v2H8zM11 5h2v2h-2z"/>',
  rust: '<path d="m10 3 1 2 2-.3.5 2 2 .7-1 1.8 1 1.7-2 .7-.5 2-2-.3-1 2-1-2-2 .3-.5-2-2-.7 1-1.8-1-1.7 2-.7.5-2 2 .3z"/><circle cx="10" cy="10" r="2.2"/>',
  python: '<path d="M10 3H7a2 2 0 0 0-2 2v3h5V6H7.5M10 3v6h3a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2zM10 17h3a2 2 0 0 0 2-2v-3h-5v2h2.5M10 17v-6H7a2 2 0 0 0-2 2v2a2 2 0 0 0 2 2z"/>',
  java: '<path d="M8 4c3 2-1 2 1 4 2 2 4 0 2-2M6 10c-1 2 1 3 4 3s5-1 4-3M7 15c2 1 6 1 8 0M9 17h2"/>',
  go: '<path d="M3 10c2-4 12-4 14 0-2 4-12 4-14 0Z"/><path d="M7 10h6M10 7v6"/>',
  native: '<path d="M4 3h12v14H4z"/><path d="m7 8 2 2-2 2M10.5 12H13"/>',
  shell: '<path d="M4 3h12v14H4z"/><path d="m7 8 2 2-2 2M10 13h3"/>',
  data: '<path d="M5 4c0-1 2-2 5-2s5 1 5 2-2 2-5 2-5-1-5-2ZM5 4v5c0 1 2 2 5 2s5-1 5-2V4M5 9v5c0 1 2 2 5 2s5-1 5-2V9"/>',
  component: '<path d="m10 2 7 4v8l-7 4-7-4V6z"/><path d="M10 2v8m7-4-7 4-7-4"/>'
};
function fileIcon(kind) {
  const result = node("span", `session-files-icon session-files-icon-${kind === "folder" ? "folder" : kind}`);
  result.dataset.icon = kind;
  result.innerHTML = `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${kind === "folder" ? fileIconPaths.folder : fileIconPaths[kind] || fileIconPaths.file}</svg>`;
  return result;
}
function fileTwisty(isFolder, expanded = false) {
  const result = node("span", "session-files-twist");
  result.setAttribute("aria-hidden", "true");
  if (isFolder) {
    result.dataset.expanded = String(expanded);
    result.append(node("span", "session-files-folder-chevron", "›"));
  }
  return result;
}

export function mountSessionFiles({
  workspace, trigger, getContext, beforeOpen, onUnauthorized, openImagePreview, copyText, request = requestRemote
}) {
  const historyKey = "codexElvesFiles";
  let navigationOwner = Math.random().toString(36).slice(2);
  const screens = [], controllers = new Set(), inertState = new Map();
  const responses = new Map(), views = new Map();
  const pendingDirectories = new Set();
  let disposed = false, contextKey = "", authorizationKey = "", target = null, route = null;
  let listEpoch = 0, fileEpoch = 0, listController = null, fileController = null;
  let listResponse = null, contentResponse = null, currentDirectory = "", renderedFile = "";
  let expanded = new Set(), searchMatches = null, searchEpoch = 0, searchController = null;
  let sourceMode = false, wrap = false, returnView = null, exitTimer = null;
  const createScreen = (id, title) => {
    const screen = node("section", "session-tools-screen session-files-screen");
    screen.id = id; screen.hidden = true; screen.inert = true;
    const heading = node("header", "session-tools-heading");
    const copy = node("div", "session-tools-heading-copy");
    const label = node("h1", "", title), path = node("span", "session-tools-heading-path");
    label.id = `${id}-title`; screen.setAttribute("aria-labelledby", label.id);
    copy.append(label, path);
    heading.append(copy, control("返回", back, "text-button session-tools-back"));
    screen.append(heading); workspace.append(screen); screens.push(screen);
    return { screen, heading, label, path };
  };
  const listing = createScreen("session-files-list", "文件");
  const content = createScreen("session-files-content", "文件");
  content.screen.classList.add("session-tools-file-screen");
  const search = node("input", "session-tools-file-search");
  search.type = "search"; search.placeholder = "搜索项目文件"; search.autocomplete = "off";
  search.setAttribute("aria-label", search.placeholder);
  const parentButton = control("上级目录", back);
  const listStatus = node("p", "session-tools-status"); listStatus.setAttribute("role", "status");
  const list = node("div", "session-tools-file-list session-files-list");
  list.setAttribute("role", "tree");
  list.setAttribute("aria-label", "项目文件树");
  const listingBody = node("div", "session-tools-page session-tools-changes");
  listingBody.append(search, parentButton, listStatus, list); listing.screen.append(listingBody);
  const contentControls = node("div", "session-tools-detail-controls");
  const metadata = node("span", "session-files-metadata");
  const modeButton = control("源码", () => { sourceMode = !sourceMode; renderContent(); });
  modeButton.setAttribute("aria-label", "切换源码和预览");
  const wrapButton = control("自动换行", () => { wrap = !wrap; renderContent(); });
  contentControls.append(metadata, wrapButton, modeButton);
  const fileStatus = node("p", "session-tools-status"); fileStatus.setAttribute("role", "status");
  const body = node("div", "session-tools-detail session-files-content");
  content.screen.append(contentControls, fileStatus, body);
  function message(element, text = "", failed = false) {
    element.textContent = text; element.hidden = !text; element.classList.toggle("error-text", failed);
  }
  function saveView() {
    if (route?.kind === "list") views.set("list:root", {
      query: search.value, scroll: list.scrollTop, expanded: [...expanded]
    });
    if (route?.kind === "file") views.set(`file:${route.path}`, { scroll: body.scrollTop, sourceMode, wrap });
    while (views.size > 64) views.delete(views.keys().next().value);
  }
  function updateContext() {
    if (disposed) return;
    const value = getContext() || {};
    const nextTarget = value.threadId ? { threadId: value.threadId }
      : value.creating && value.projectId ? { projectId: value.projectId } : null;
    const nextAuthorization = JSON.stringify([value.deviceId || "", value.cacheScope || ""]);
    const nextKey = JSON.stringify([nextAuthorization, nextTarget]);
    if (contextKey === nextKey) return;
    // The conversation may already show another thread when this runs. Do not
    // restore the previous thread's scroll position or reactivate its pages.
    returnView = null;
    hideAll(true); ++listEpoch; ++fileEpoch;
    navigationOwner = Math.random().toString(36).slice(2);
    for (const controller of controllers) controller.abort();
    target = nextTarget; contextKey = nextKey; authorizationKey = nextAuthorization;
    responses.clear(); views.clear(); listResponse = contentResponse = null;
    expanded = new Set(); pendingDirectories.clear(); searchMatches = null; search.value = "";
    currentDirectory = renderedFile = ""; list.replaceChildren(); body.replaceChildren();
  }
  function retain(key, value) {
    responses.delete(key); responses.set(key, value);
    let size = 0, count = 0;
    for (const [id, result] of [...responses].reverse()) {
      size += JSON.stringify(result).length * 2;
      if (size > 16 * 1024 * 1024 || ++count > 24) responses.delete(id);
    }
  }
  async function read(path, kind, controller, query = "") {
    if (!target) throw new Error("请先选择已有会话，或为新建会话选择项目。");
    const normalizedQuery = query.trim();
    const owner = contextKey, authorization = authorizationKey;
    const key = `${kind}:${path}:${normalizedQuery}`;
    const cached = responses.get(key);
    controllers.add(controller);
    const timer = setTimeout(() => controller.abort(new DOMException("读取超时", "TimeoutError")), 45000);
    const current = () => !disposed && owner === contextKey && authorization === authorizationKey && !controller.signal.aborted;
    try {
    const value = await request(`/api/workspace/${kind}`, {
        method: "POST", credentials: "same-origin", redirect: "error", signal: controller.signal,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...target, path,
          ...(kind === "files" && normalizedQuery ? { query: normalizedQuery } : {}),
          ...(cached ? { ifRevision: cached.revision } : {}) })
      }, async response => {
        const result = await response.json();
        if (!response.ok || result?.ok !== true)
          throw Object.assign(new Error(result?.message || result?.error || `读取失败（HTTP ${response.status}）`), { code: response.status });
        return result;
      });
      if (!current()) throw new DOMException("已取消", "AbortError");
      if (!revisionValid(value.revision) || typeof value.root !== "string") throw new Error("文件响应格式无效");
      if (value.notModified === true) {
        if (!cached || value.revision !== cached.revision || value.root !== cached.root) throw new Error("文件缓存已失效，请重新打开");
        return cached;
      }
      if (typeof value.path !== "string" || typeof value.truncated !== "boolean"
          || kind === "files" && (!Array.isArray(value.files) || value.files.length > 2000
            || value.files.some(file => !file || typeof file.name !== "string" || typeof file.path !== "string"
              || !["directory", "file", "unsupported"].includes(file.kind)))
          || kind === "file" && (!["text", "image", "binary", "unsupported"].includes(value.kind)
            || value.kind === "text" && (typeof value.text !== "string" || value.text.length > 4 * 1024 * 1024)
            || value.kind === "image" && !imageValid(value.dataUrl))) throw new Error("文件响应格式无效");
      retain(key, value); return value;
    } catch (error) {
      if (current() && error.code === 401 && !error.secondaryChannel) {
        responses.clear(); views.clear(); body.replaceChildren(); list.replaceChildren(); onUnauthorized?.();
      }
      if (current() && [403, 404, 409, 422].includes(error.code)) responses.delete(key);
      if (controller.signal.reason?.name === "TimeoutError") throw new Error("读取超时，请检查远程连接后重试。");
      if (error instanceof TypeError) throw new Error("无法连接电脑端服务，请重试。");
      throw error;
    } finally { clearTimeout(timer); controllers.delete(controller); }
  }
  function prepare(next, recordHistory) {
    updateContext(); beforeOpen?.(); saveView(); clearTimeout(exitTimer);
    if (!route) {
      const messages = workspace.querySelector("#messages");
      returnView = { messages, scroll: messages?.scrollTop || 0, key: contextKey };
    }
    route = next;
    for (const element of workspace.querySelectorAll(":scope > .conversation, :scope > .sidebar")) {
      if (!inertState.has(element)) inertState.set(element, element.inert);
      element.inert = true;
    }
    if (recordHistory) history.pushState({ ...history.state,
      [historyKey]: { owner: navigationOwner, context: contextKey, ...next } }, "", location.href);
  }
  function show(screen) {
    screen.hidden = false; screen.inert = false;
    screen.getBoundingClientRect(); screen.classList.add("session-tools-visible");
    screen.querySelector(".session-tools-back").focus({ preventScroll: true });
  }
  function hide(screen) {
    screen.inert = true; screen.classList.remove("session-tools-visible");
  }
  async function openDirectory(path = "", { recordHistory = true } = {}) {
    if (disposed) return;
    prepare({ kind: "list", path }, recordHistory);
    ++fileEpoch; fileController?.abort(); hide(content.screen);
    show(listing.screen); currentDirectory = path;
    parentButton.hidden = true;
    const listView = views.get("list:root") || {};
    search.value = listView.query || "";
    expanded = new Set(Array.isArray(listView.expanded) ? listView.expanded : []);
    searchMatches = null;
    if (search.value.trim()) {
      listResponse = responses.get("files::") || null;
      renderDirectory(); list.scrollTop = listView.scroll || 0;
      void searchProject(search.value);
      return;
    }
    const cached = responses.get(`files:${path}:`);
    listResponse = cached || null; renderDirectory(); list.scrollTop = listView.scroll || 0;
    if (!cached) message(listStatus, "正在读取目录…");
    listing.path.textContent = cached?.root ? `${cached.root}${path ? `/${path}` : ""}` : path;
    const epoch = ++listEpoch; listController?.abort(); listController = new AbortController();
    try {
      const value = await read(path, "files", listController, "");
      if (disposed || epoch !== listEpoch || route?.kind !== "list" || currentDirectory !== path) return;
      const scroll = list.scrollTop;
      if (value !== listResponse) { listResponse = value; renderDirectory(); list.scrollTop = scroll; }
      listing.path.textContent = `${value.root}${path ? `/${path}` : ""}`; listing.path.title = listing.path.textContent;
      message(listStatus, value.truncated ? "目录较大，仅显示前 2000 项。" : value.files.length ? "" : "此目录为空。");
    } catch (error) {
      if (epoch !== listEpoch || error.name === "AbortError") return;
      if ([401, 403, 404, 409, 422].includes(error.code)) { listResponse = null; list.replaceChildren(); }
      message(listStatus, `${listResponse ? "正在显示上次内容：" : ""}${error.message}`, true);
      listStatus.append(control("重试", () => void openDirectory(path, { recordHistory: false })));
    }
  }
  function renderDirectory() {
    list.replaceChildren();
    const query = search.value.trim().toLocaleLowerCase();
    if (query) {
      for (const file of searchMatches || []) renderTreeRow(file, 0, true);
    } else {
      for (const file of sortedFiles(listResponse?.files)) renderTreeRow(file, 0, false);
    }
    if (query && searchMatches && !searchMatches.length) list.append(node("p", "session-tools-empty", "没有匹配的文件。"));
    if (!query && listResponse?.files.length && !list.childElementCount) list.append(node("p", "session-tools-empty", "此目录为空。"));
  }
  function renderTreeRow(file, depth, searchResult) {
    const isFolder = file.kind === "directory";
    const isExpanded = expanded.has(file.path);
    const button = control("", () => isFolder ? toggleDirectory(file.path) : void openFile(file.path), "session-tools-file");
    button.title = file.path; button.disabled = file.kind === "unsupported";
    button.style.setProperty("--file-depth", String(depth));
    button.dataset.path = file.path;
    button.dataset.depth = String(depth);
    button.setAttribute("role", "treeitem");
    button.setAttribute("aria-level", String(depth + 1));
    button.setAttribute("aria-selected", String(!isFolder && renderedFile === file.path));
    if (isFolder) button.setAttribute("aria-expanded", String(isExpanded));
    button.classList.toggle("session-files-selected", !isFolder && renderedFile === file.path);
    button.append(fileTwisty(isFolder, isExpanded), fileIcon(isFolder ? "folder" : fileIconKind(file.path)));
    const copy = node("span", "session-tools-file-copy");
    copy.append(node("strong", "session-tools-file-name", file.name));
    if (searchResult) copy.append(node("span", "session-tools-file-directory", parentPath(file.path) || "项目根目录"));
    button.append(copy, node("span", "session-files-metadata",
      isFolder ? "文件夹" : file.kind === "unsupported" ? "不支持预览" : bytesLabel(file.size)));
    list.append(button);
    if (isFolder && isExpanded && !searchResult) {
      const children = sortedFiles(responses.get(`files:${file.path}:`)?.files);
      for (const child of children) renderTreeRow(child, depth + 1, false);
      if (!responses.has(`files:${file.path}:`) && !pendingDirectories.has(file.path)) {
        const loading = node("p", "session-files-tree-status", "正在读取目录…");
        loading.style.setProperty("--file-depth", String(depth + 1)); list.append(loading);
        void loadDirectoryChildren(file.path);
      }
    }
  }
  function toggleDirectory(path) {
    if (expanded.has(path)) expanded.delete(path); else expanded.add(path);
    renderDirectory();
    if (expanded.has(path) && !responses.has(`files:${path}:`)) void loadDirectoryChildren(path);
  }
  async function loadDirectoryChildren(path) {
    if (pendingDirectories.has(path)) return;
    pendingDirectories.add(path);
    const controller = new AbortController();
    try {
      const value = await read(path, "files", controller, "");
      if (value && route?.kind === "list" && !search.value.trim()) renderDirectory();
    } catch (error) {
      if (error.name !== "AbortError") {
        const failure = node("p", "session-files-tree-status error-text", error.message);
        failure.style.setProperty("--file-depth", String(path.split("/").length + 1));
        list.append(failure);
      }
    } finally {
      pendingDirectories.delete(path);
    }
  }
  async function searchProject(query) {
    const normalized = query.trim();
    if (!normalized) { searchMatches = null; renderDirectory(); return; }
    const epoch = ++searchEpoch; searchController?.abort(); searchController = new AbortController();
    message(listStatus, "正在搜索项目文件…");
    try {
      const value = await read("", "files", searchController, normalized);
      if (epoch !== searchEpoch || route?.kind !== "list") return;
      searchMatches = value.files || []; renderDirectory();
      message(listStatus, value.truncated ? "搜索结果较多，仅显示前 2000 项。" : `${searchMatches.length} 个匹配文件`);
    } catch (error) {
      if (epoch !== searchEpoch || error.name === "AbortError") return;
      message(listStatus, error.message, true);
    }
  }
  async function openFile(path, line = null, { recordHistory = true } = {}) {
    if (disposed || typeof path !== "string") return;
    prepare({ kind: "file", path, line: Number.isSafeInteger(line) && line > 0 ? line : null }, recordHistory);
    ++listEpoch; listController?.abort(); listing.screen.inert = true;
    show(content.screen); content.label.textContent = fileName(path); content.path.textContent = path;
    const saved = views.get(`file:${path}`);
    sourceMode = line ? true : saved?.sourceMode ?? !/\.mdx?$/iu.test(path); wrap = saved?.wrap || false;
    contentResponse = responses.get(`file:${path}`) || null; renderedFile = path;
    renderContent(); body.scrollTop = saved?.scroll || 0;
    if (!contentResponse) message(fileStatus, "正在读取文件…");
    const epoch = ++fileEpoch; fileController?.abort(); fileController = new AbortController();
    try {
      const value = await read(path, "file", fileController);
      if (disposed || epoch !== fileEpoch || route?.kind !== "file" || route.path !== path) return;
      const scroll = body.scrollTop;
      if (value !== contentResponse) { contentResponse = value; renderContent(); body.scrollTop = scroll; }
      content.path.textContent = `${value.root}/${value.path}`; content.path.title = content.path.textContent;
      message(fileStatus, value.message);
      if (line) body.querySelector(`[data-line="${line}"]`)?.scrollIntoView({ block: "center" });
    } catch (error) {
      if (epoch !== fileEpoch || error.name === "AbortError") return;
      if ([401, 403, 404, 409, 422].includes(error.code)) { contentResponse = null; body.replaceChildren(); }
      message(fileStatus, `${contentResponse ? "正在显示上次内容：" : ""}${error.message}`, true);
      fileStatus.append(control("重试", () => void openFile(path, line, { recordHistory: false })));
    }
  }
  function resolveDocumentReference(path) {
    if (/^(?:[a-z]:\/|\/)/iu.test(path)) return path;
    const parts = parentPath(contentResponse?.path || renderedFile).split("/").filter(Boolean);
    for (const part of path.replaceAll("\\", "/").split("/")) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (!parts.length) return null;
        parts.pop();
      } else parts.push(part);
    }
    return parts.join("/");
  }
  function renderContent() {
    body.replaceChildren(); body.classList.toggle("session-files-wrap", wrap);
    const value = contentResponse;
    contentControls.hidden = !value;
    modeButton.hidden = value?.kind !== "text" || !/\.mdx?$/iu.test(value.path);
    wrapButton.hidden = value?.kind !== "text" || !sourceMode;
    modeButton.textContent = sourceMode ? "预览" : "源码";
    modeButton.title = sourceMode ? "切换到预览" : "切换到源码";
    wrapButton.setAttribute("aria-pressed", String(wrap));
    if (!value) return;
    metadata.textContent = bytesLabel(value.size);
    message(fileStatus, value.message);
    if (value.kind === "text" && !sourceMode) {
      const document = node("div", "markdown session-files-markdown");
      document.innerHTML = markdown(value.text);
      for (const copy of document.querySelectorAll(".copy-code")) {
        if (copyText) copy.onclick = () => void copyText(copy.closest(".code-block").querySelector("code").textContent);
        else copy.remove();
      }
      document.addEventListener("click", event => {
        const link = event.target.closest("[data-local-file-path]");
        if (!link) return;
        event.preventDefault(); event.stopPropagation();
        const path = resolveDocumentReference(link.dataset.localFilePath);
        if (path) void openFile(path, Number(link.dataset.localFileLine) || null);
        else message(fileStatus, "该路径超出了当前项目。", true);
      });
      body.append(document);
    } else if (value.kind === "text") {
      const code = node("div", "session-files-source");
      code.tabIndex = 0; code.setAttribute("aria-label", "文件源码，可横向滚动");
      const fragment = document.createDocumentFragment();
      value.text.split("\n").forEach((line, index) => {
        const row = node("div", "session-files-source-line");
        row.dataset.line = index + 1;
        row.classList.toggle("session-files-selected-line", route?.line === index + 1);
        row.append(node("span", "session-tools-line-number", String(index + 1)), node("code", "", line || "\u200b"));
        fragment.append(row);
      });
      code.append(fragment); body.append(code);
    } else if (value.kind === "image") {
      const link = node("a", "session-files-image"); link.href = value.dataUrl;
      const image = node("img"); image.src = value.dataUrl; image.alt = fileName(value.path);
      link.append(image); link.onclick = event => { event.preventDefault(); openImagePreview?.(link, image); };
      body.append(link);
    }
  }
  function finishExit() {
    if (route || disposed) return;
    const returning = !!returnView;
    for (const { screen } of [listing, content]) screen.hidden = true;
    for (const [element, previous] of inertState) element.inert = previous;
    inertState.clear();
    if (returnView?.key === contextKey && returnView.messages?.isConnected) returnView.messages.scrollTop = returnView.scroll;
    returnView = null;
    if (returning && trigger.isConnected) trigger.focus({ preventScroll: true });
  }
  function hideAll(immediate = false) {
    saveView(); route = null; ++fileEpoch; ++listEpoch;
    fileController?.abort(); listController?.abort();
    for (const screen of screens) hide(screen);
    clearTimeout(exitTimer);
    if (immediate) finishExit(); else exitTimer = setTimeout(finishExit, 250);
  }
  function back() {
    if (!route || disposed) return;
    const image = document.querySelector("#image-preview-dialog[open]");
    if (image) { image.close(); return; }
    saveView();
    if (history.state?.[historyKey]?.owner === navigationOwner) history.back();
    else hideAll();
  }
  function pop(event) {
    const next = event.state?.[historyKey];
    if (next?.owner !== navigationOwner || next.context !== contextKey) { if (route) hideAll(); return; }
    if (next.kind === "file") void openFile(next.path, next.line, { recordHistory: false });
    else if (next.kind === "list") void openDirectory(next.path, { recordHistory: false });
  }
  function key(event) {
    if (!route || disposed || event.isComposing || document.querySelector("dialog[open]")) return;
    if (event.key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); back(); }
    if (event.key === "Tab") {
      const screen = route.kind === "file" ? content.screen : listing.screen;
      const controls = [...screen.querySelectorAll("button:not(:disabled),input,[tabindex='0'],a[href]")]
        .filter(element => element.getClientRects().length && !element.closest("[hidden]"));
      const first = controls[0], last = controls.at(-1);
      if (event.shiftKey && (document.activeElement === first || !screen.contains(document.activeElement))) {
        event.preventDefault(); last?.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !screen.contains(document.activeElement))) {
        event.preventDefault(); first?.focus();
      }
    }
  }
  function nativeBack(event) {
    if (!route || disposed || event.defaultPrevented || document.querySelector("dialog[open]")) return;
    event.preventDefault(); back();
  }
  function markdownReady() {
    if (route?.kind === "file" && contentResponse?.kind === "text" && !sourceMode) renderContent();
  }
  let searchTimer = null;
  search.addEventListener("input", () => {
    clearTimeout(searchTimer);
    searchEpoch++;
    searchController?.abort();
    searchTimer = setTimeout(() => void searchProject(search.value), 180);
    if (!search.value.trim()) {
      searchMatches = null; message(listStatus, "");
      renderDirectory();
    } else {
      message(listStatus, "正在搜索项目文件…");
      renderDirectory();
    }
  });
  document.addEventListener("keydown", key, true);
  window.addEventListener("popstate", pop, true);
  window.addEventListener("codex-elves-native-back", nativeBack);
  window.addEventListener("codex-elves-markdown-ready", markdownReady);
  updateContext();
  return {
    open: () => openDirectory(currentDirectory), openFile, updateContext,
    dispose() {
      if (disposed) return;
      hideAll(true); disposed = true;
      for (const controller of controllers) controller.abort();
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("popstate", pop, true);
      window.removeEventListener("codex-elves-native-back", nativeBack);
      window.removeEventListener("codex-elves-markdown-ready", markdownReady);
      if (history.state?.[historyKey]?.owner === navigationOwner) {
        const state = { ...history.state }; delete state[historyKey]; history.replaceState(state, "", location.href);
      }
      for (const screen of screens) screen.remove();
      responses.clear(); views.clear();
    }
  };
}
