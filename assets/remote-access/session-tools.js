const sections = [
  ["staged", "已暂存"], ["unstaged", "未暂存"], ["untracked", "未跟踪"]
];
const statusLabels = {
  A: "新增", M: "修改", D: "删除", R: "重命名", C: "复制", U: "冲突", T: "类型变化", "?": "新增",
  added: "新增", modified: "修改", deleted: "删除", renamed: "重命名",
  copied: "复制", conflicted: "冲突", untracked: "新增"
};
const imageSource = value => typeof value === "string"
  && /^data:image\/(?:png|jpe?g|gif|webp|avif|bmp|x-icon|vnd\.microsoft\.icon);base64,[a-zA-Z0-9+/=\s]+$/.test(value);

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
const cacheSchema = 1, responseVersion = 1, memoryLimit = 16 * 1024 * 1024;
const validRevision = value => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const objectValue = value => value !== null && typeof value === "object" && !Array.isArray(value);
const safeString = (value, maximum = 32768) => typeof value === "string" && value.length <= maximum;
const validRoot = value => safeString(value) && value.length > 0;
const detailKey = file => JSON.stringify([file.section, file.path]);
function fileEntry(value) {
  return objectValue(value) && sections.some(([section]) => section === value.section)
    && safeString(value.path) && value.path.length > 0 && safeString(value.status, 64)
    && (value.oldPath == null || safeString(value.oldPath));
}
function listResponse(value) {
  if (!objectValue(value) || value.ok === false || !validRoot(value.root) || !validRevision(value.revision)
      || !Array.isArray(value.files) || value.files.length > 10000 || !value.files.every(fileEntry)
      || typeof value.truncated !== "boolean" || value.message != null && !safeString(value.message, 10000)) return null;
  return { ok: true, root: value.root, revision: value.revision, truncated: value.truncated,
    message: value.message || null,
    files: value.files.map(file => ({ section: file.section, path: file.path, status: file.status,
      ...(file.oldPath != null ? { oldPath: file.oldPath } : {}) })) };
}
function detailResponse(value, file) {
  const sideValid = side => side == null || objectValue(side) && typeof side.exists === "boolean"
    && Number.isFinite(side.size) && side.size >= 0 && (side.mime == null || safeString(side.mime, 256))
    && (side.dataUrl == null || imageSource(side.dataUrl));
  if (!objectValue(value) || value.ok === false || !validRoot(value.root) || !validRevision(value.revision)
      || value.path !== file.path || value.section !== file.section
      || !["text", "image", "binary", "unsupported"].includes(value.kind)
      || typeof value.truncated !== "boolean" || value.diff != null && !safeString(value.diff, 2 * 1024 * 1024)
      || value.message != null && !safeString(value.message, 10000)
      || !sideValid(value.before) || !sideValid(value.after)) return null;
  const side = value => value == null ? null : {
    exists: value.exists, size: value.size, mime: value.mime || null, dataUrl: value.dataUrl || null
  };
  return { ok: true, root: value.root, revision: value.revision, path: value.path, section: value.section,
    kind: value.kind, diff: value.diff || "", truncated: value.truncated, message: value.message || null,
    before: side(value.before), after: side(value.after) };
}
function uiState(value = {}) {
  return {
    query: safeString(value?.query, 1000) ? value.query : "",
    collapsed: Object.fromEntries(sections.map(([section]) => [section, value?.collapsed?.[section] === true])),
    scrollTop: Number.isFinite(value?.scrollTop) ? Math.max(0, value.scrollTop) : 0,
    wrap: value?.wrap === true
  };
}
function responseBytes(value) {
  const serialized = JSON.stringify(value);
  return typeof TextEncoder === "function" ? new TextEncoder().encode(serialized).length : serialized.length * 2;
}
export function mountSessionTools({ workspace, trigger, getContext, beforeOpen, onUnauthorized, openImagePreview, cache }) {
  let disposed = false, opened = false, activePage = "changes", detailVisible = false, contextKey = "";
  let changesEpoch = 0, detailEpoch = 0, changesController = null, detailController = null;
  let files = [], selectedFile = null, changesLoaded = false, changesBusy = false;
  let returnView = null, wrapDiff = false, pageExitTimer = null, detailExitTimer = null;
  let project = null, displayedProject = null, detailRecord = null, authorized = true, projectSequence = 0;
  const projects = new Map(), cacheReads = new Set(), cacheMutations = new Map();
  const controllers = new Set(), inertState = new Map();
  const navigationOwner = Math.random().toString(36).slice(2);
  const historyKey = "codexElvesTools";
  const pageTitles = { changes: "变更" };
  const fileTriggers = new Map();
  const panel = node("section", "session-tools-screen");
  panel.id = "session-tools-screen";
  panel.hidden = true;
  panel.setAttribute("aria-labelledby", "session-tools-title");
  panel.setAttribute("tabindex", "-1");
  const heading = node("header", "session-tools-heading");
  const headingCopy = node("div", "session-tools-heading-copy");
  const pageTitle = node("h1", "", "变更");
  pageTitle.id = "session-tools-title";
  const rootLabel = node("span", "session-tools-heading-path", "当前会话工作目录");
  const backToConversation = control("返回", "返回会话", requestBack, "text-button session-tools-back");
  headingCopy.append(pageTitle, rootLabel);
  heading.append(headingCopy, backToConversation);
  const pageBodies = new Map();
  for (const [name, label] of [["changes", "变更"]]) {
    const body = node("section", `session-tools-page session-tools-${name}`);
    body.id = `session-tools-${name}`;
    body.setAttribute("aria-label", label);
    body.hidden = name !== activePage;
    pageBodies.set(name, body);
  }
  panel.append(heading, ...pageBodies.values());
  const detailPanel = node("section", "session-tools-screen session-tools-file-screen");
  detailPanel.id = "session-tools-file-screen"; detailPanel.hidden = true; detailPanel.inert = true;
  detailPanel.setAttribute("aria-labelledby", "session-tools-file-title");
  const detailHeading = node("header", "session-tools-heading");
  const detailHeadingCopy = node("div", "session-tools-heading-copy");
  const detailTitle = node("h1");
  detailTitle.id = "session-tools-file-title";
  const detailPath = node("span", "session-tools-heading-path");
  detailHeadingCopy.append(detailTitle, detailPath);
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
    const abort = () => controller.abort();
    if (signal?.aborted) abort();
    else signal?.addEventListener("abort", abort, { once: true });
    let timedOut = false;
    const timeout = setTimeout(() => { timedOut = true; controller.abort(); }, 45000);
    const ensureCurrent = () => {
      if (controller.signal.aborted || !ownsAuthorization(owner)) {
        controller.abort();
        const error = new Error("请求已取消。"); error.name = "AbortError"; throw error;
      }
    };
    try {
      ensureCurrent();
      // 变更接口固定走当前登录入口。
      const response = await fetch(path, {
        method: "POST", credentials: "same-origin", redirect: "error",
        headers: { "Content-Type": "application/json", skip_zrok_interstitial: "1", "ngrok-skip-browser-warning": "1" },
        body: JSON.stringify(body), signal: controller.signal
      });
      ensureCurrent();
      let result;
      try { result = await response.json(); }
      catch (error) {
        if (error?.name === "AbortError") throw error;
        ensureCurrent();
        result = { message: `无法读取服务响应（HTTP ${response.status}）` };
      }
      ensureCurrent();
      if (!response.ok || result?.ok === false) {
        if (response.status === 401) { forgetAuthorization(); onUnauthorized?.(); }
        throw Object.assign(new Error(result?.message || result?.error || `请求失败（HTTP ${response.status}）`), { code: response.status });
      }
      return result;
    } catch (error) {
      if (timedOut) throw new Error("服务响应超时，请检查电脑端服务后重试。");
      if (error instanceof TypeError) throw new Error("无法连接电脑端服务，请检查远程连接后重试。");
      throw error;
    } finally {
      clearTimeout(timeout); controllers.delete(controller);
      signal?.removeEventListener("abort", abort);
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
    rootLabel.textContent = p.list?.root || "当前会话工作目录"; rootLabel.title = p.list?.root || "";
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
      rootLabel.textContent = "当前会话工作目录"; rootLabel.title = "";
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
      rootLabel.textContent = "当前会话工作目录"; rootLabel.title = "";
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
            const [section, path] = JSON.parse(id);
            if (detailKey({ section, path }) === id) {
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
      let result = await request("/api/workspace/changes", payload, controller.signal);
      if (!current()) return;
      if (result?.notModified === true) {
        if (cached && validRoot(result.root) && result.root !== cached.root) {
          invalidateProject(p, "工作目录已变化，正在重新读取变更。");
          void refreshChanges(); return;
        }
        if (cached && result.revision === cached.revision && result.root === cached.root && p.list === cached) {
          ++p.networkVersion; status(changesStatus, summary(p)); return;
        }
        result = await request("/api/workspace/changes", p.target, controller.signal);
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
      for (const [section] of sections) applyFold(p, section);
      return;
    }
    const scrollTop = !restoreScroll && project === p && displayedProject === p ? fileList.scrollTop : p.ui.scrollTop;
    p.rendered = signature; p.listView.replaceChildren(); p.buttons.clear(); p.groups.clear();
    const source = p.list?.files || [];
    const visible = source.filter(file => !query || `${file.path} ${file.oldPath || ""}`.toLocaleLowerCase().includes(query));
    if (source.length && !visible.length) p.listView.append(node("p", "session-tools-empty", "没有匹配的变更文件。"));
    for (const [section, label] of sections) {
      const entries = visible.filter(file => file.section === section);
      if (!entries.length) continue;
      const group = node("section", "session-tools-file-group"), title = node("h3");
      const body = node("div", "session-tools-file-group-body");
      body.id = `session-tools-group-${navigationOwner}-${p.number}-${section}`;
      const toggle = control("", `${label} · ${entries.length}`, () => {
        if (project !== p || disposed) return;
        if (p.ui.query.trim()) {
          if (p.searchCollapsed.has(section)) p.searchCollapsed.delete(section);
          else p.searchCollapsed.add(section);
        } else p.ui.collapsed[section] = !p.ui.collapsed[section];
        applyFold(p, section); p.ui.scrollTop = fileList.scrollTop; markViewChanged(p);
      }, "session-tools-group-toggle");
      toggle.setAttribute("aria-controls", body.id);
      const chevron = node("span", "session-tools-group-chevron", "›"); chevron.setAttribute("aria-hidden", "true");
      toggle.append(chevron, node("span", "session-tools-group-label", `${label} · ${entries.length}`));
      title.append(toggle); group.append(title, body); p.groups.set(section, { toggle, body });
      applyFold(p, section);
      for (const file of entries) {
        const button = control("", `${label}：${file.path}`, () => void loadDetail(file), "session-tools-file");
        const parts = fileParts(file), copy = node("span", "session-tools-file-copy");
        const directory = file.oldPath ? `${parts.directory} · 原路径：${file.oldPath}` : parts.directory;
        copy.append(node("strong", "session-tools-file-name", parts.name), node("span", "session-tools-file-directory", directory));
        copy.title = file.oldPath ? `${file.oldPath} → ${file.path}` : file.path;
        const badge = node("span", "session-tools-file-status", statusLabel(file.status)); badge.setAttribute("data-status", file.status);
        button.append(node("span", "session-tools-file-icon", "▤"), copy, badge, node("span", "session-tools-file-chevron", "›"));
        p.buttons.set(detailKey(file), button); body.append(button);
      }
      p.listView.append(group);
    }
    p.ui.scrollTop = scrollTop;
    if (project === p) attachList(p);
  }
  function fileParts(file) {
    const parts = String(file.path).split(/[\\/]/);
    return { name: parts.pop() || file.path, directory: parts.join("/") || "项目根目录" };
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
    selectedFile = file;
    const parts = fileParts(file);
    detailTitle.textContent = parts.name; detailTitle.title = file.path;
    detailPath.textContent = parts.directory; detailPath.title = file.path;
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
      const payload = { ...p.target, path: file.path, section: file.section };
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
      const figure = node("figure");
      figure.append(node("figcaption", "", label));
      if (value?.exists === false) figure.append(node("p", "session-tools-empty", "此侧不存在文件"));
      else if (imageSource(value?.dataUrl)) {
        const link = node("a", "session-tools-image-link");
        link.href = value.dataUrl;
        const image = node("img");
        image.alt = `${file.path} · ${label}`; image.src = value.dataUrl;
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
    for (const element of workspace.querySelectorAll(":scope > .conversation, :scope > .sidebar")) {
      if (!inertState.has(element)) inertState.set(element, element.inert);
      element.inert = true;
    }
  }
  function recordRoute(route, file = null) {
    history.pushState({
      ...(history.state || {}),
      [historyKey]: { owner: navigationOwner, route, page: activePage, context: contextKey,
        ...(file ? { path: file.path, section: file.section } : {}) }
    }, "", location.href);
  }
  function openPage(name, options = {}) {
    if (disposed || !pageTitles[name]) return;
    beforeOpen?.();
    showPage(name, options);
  }
  function showPage(name, { recordHistory = true } = {}) {
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
    rootLabel.hidden = name !== "changes";
    for (const [key, body] of pageBodies) body.hidden = key !== name;
    panel.hidden = false; panel.inert = false;
    suspendConversation();
    if (!wasOpened) panel.getBoundingClientRect();
    panel.classList.add("session-tools-visible");
    if (recordHistory) recordRoute(name);
    updateContext();
    if (name === "changes" && !changesBusy && changeTarget() && (enteringChanges || !changesLoaded))
      void refreshChanges();
    backToConversation.focus({ preventScroll: true });
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
    if (trigger.isConnected) trigger.focus({ preventScroll: true });
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
    if (!wasVisible) {
      detail.scrollTop = detailRecord?.scrollTop || 0;
      detailPanel.getBoundingClientRect();
    }
    detailPanel.classList.add("session-tools-visible");
    if (recordHistory && !wasVisible) recordRoute("detail", selectedFile);
    backToFiles.focus({ preventScroll: true });
  }
  function finishDetailExit(focus) {
    if (disposed || detailVisible) return;
    clearTimeout(detailExitTimer); detailExitTimer = null;
    detailPanel.hidden = true;
    panel.inert = !opened;
    if (focus && opened) {
      const selected = selectedFile && fileTriggers.get(JSON.stringify([selectedFile.section, selectedFile.path]));
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
    const ownsEntry = history.state?.[historyKey]?.owner === navigationOwner;
    if (detailVisible) hideDetail();
    else hidePage();
    if (ownsEntry) history.back();
  }
  function handlePopState(event) {
    if (disposed) return;
    const route = event.state?.[historyKey];
    if (route?.owner !== navigationOwner || route.context !== contextKey) {
      if (opened) hidePage();
      return;
    }
    beforeOpen?.();
    if (route.route === "detail") {
      if (!opened) showPage(route.page, { recordHistory: false });
      const file = files.find(entry => entry.path === route.path && entry.section === route.section);
      if (file && (!detailVisible || selectedFile !== file)) void loadDetail(file, { recordHistory: false });
    } else if (pageTitles[route.route]) {
      if (detailVisible) hideDetail();
      if (!opened || activePage !== route.route) showPage(route.route, { recordHistory: false });
    }
  }
  function handleNativeBack(event) {
    if (disposed || !opened || event.defaultPrevented) return;
    const imageDialog = document.querySelector("#image-preview-dialog[open]");
    if (imageDialog) { event.preventDefault(); imageDialog.close(); return; }
    if (document.querySelector("dialog[open]")) return;
    event.preventDefault(); requestBack();
  }
  function handleKey(event) {
    if (!opened || disposed || document.querySelector("dialog[open]")) return;
    if (event.key === "Escape") {
      event.preventDefault(); event.stopImmediatePropagation(); requestBack();
    } else if (event.key === "Tab") {
      const currentPanel = detailVisible ? detailPanel : panel;
      const focusable = [...currentPanel.querySelectorAll("button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex='0']")]
        .filter(element => element.tabIndex >= 0 && !element.hidden && element.getClientRects().length);
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
  updateContext();

  return {
    updateContext,
    open: openPage,
    close: requestBack,
    dispose() {
      if (disposed) return;
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
