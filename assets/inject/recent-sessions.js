(() => {
  const version = 14;
  if (window.__codexElvesRecentSessions?.version === version) return;
  window.__codexElvesRecentSessions?.dispose?.();

  const entrySelector = "[data-codex-recent-sessions-entry]";
  const rootSelector = "[data-codex-recent-sessions-root]";
  const hostClass = "codex-recent-sessions-host";
  const styleId = "codex-recent-sessions-style";
  const cachePrefix = "codexElves.recentSessions.v1:";
  const cacheMaxAgeMs = 7 * 24 * 60 * 60 * 1000;
  const cacheMaxChars = 2 * 1024 * 1024;
  let options = { enabled: false };
  let entry = null, root = null, host = null, list = null, feedback = null;
  let menu = null, hostScrollTop = 0, selectedId = "";
  let active = false, opening = false, disposed = false, timer = null, request = null, revision = 0;
  let listening = false, deferredRender = false;
  let snapshot = null, signature = "", limit = 40;
  let cacheScope = "", cacheRestored = false, cachedSnapshot = false, snapshotComplete = false, lastCacheWriteAt = 0;
  let snapshotSavedAt = 0, cacheWriteTimer = null, cacheDirty = false;
  let paused = true, hostObserver = null, subscribedAdapter = null, unsubscribeSidebar = null;
  let renderedDay = "", renderedSelection = null, lastReadAt = 0, warmupFrame = 0;
  let entryObserver = null, entryObserverRoots = [], entryObserverSubtree = false, entrySyncTask = null;
  const deletedIds = new Set();
  let priorityIds = null, priorityOpenedAt = 0, priorityKnownIds = new Set();
  const prioritySelectionSuppressions = new Set();
  const collapsed = new Set(), busy = new Set(), uncertainDeletions = new Set();
  const updatedAt = item => item.updatedAt || item.recencyAt || item.createdAt || 0;
  const normalizedId = id => String(id || "").replace(/^local:/, "").toLowerCase();
  function localDay() {
    const date = new Date();
    return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
  }
  function resolvedScope() {
    return String(options.cacheScope || window.__CODEX_ELVES_SESSION_CACHE_SCOPE__
      || `launch:${window.__CODEX_ELVES_LAUNCH_CYCLE__ || "current"}:${window.__CODEX_SESSION_DELETE_HELPER__ || ""}`);
  }
  function persistentCacheKey() {
    return options.cacheScope || window.__CODEX_ELVES_SESSION_CACHE_SCOPE__ ? cachePrefix + cacheScope : null;
  }
  function compactSnapshot(value, includeSideAttention = true) {
    const sides = new Set(value.sideThreadIds || []);
    const suppressed = window.__codexElvesSuppressedThreadIds
      || new Set((window.__CODEX_ELVES_SUPPRESSED_THREADS__ || []).map(normalizedId));
    const threads = items => (items || []).filter(item => item && typeof item.id === "string"
      && mainThread(item) && !sides.has(item.id) && !deletedIds.has(normalizedId(item.id))
      && !suppressed.has(normalizedId(item.id))).map(item => ({
        id: item.id, title: String(item.title || "新会话"), cwd: String(item.cwd || ""),
        createdAt: Number(item.createdAt) || 0, updatedAt: Number(item.updatedAt) || 0,
        recencyAt: Number(item.recencyAt) || 0, hasUnreadTurn: item.hasUnreadTurn === true,
        status: item.status, priority: item.priority, projectId: item.projectId ?? null,
        pinned: item.pinned === true,
        ...(includeSideAttention && item.sideThreadAttention ? { sideThreadAttention: {
          active: item.sideThreadAttention.active === true,
          waiting: item.sideThreadAttention.waiting === true,
          unread: item.sideThreadAttention.unread === true
        } } : {})
      }));
    return {
      ok: true, recent: threads(value.recent), pinnedThreads: threads(value.pinnedThreads),
      projects: (value.projects || []).map(project => ({ id: project.id, name: String(project.name || "") })),
      sideThreadIds: [...sides]
    };
  }
  function validCache(record) {
    const age = Date.now() - record?.savedAt;
    return record?.version === 1 && record.scope === cacheScope && age >= 0 && age <= cacheMaxAgeMs
      && Array.isArray(record.snapshot?.recent) && Array.isArray(record.snapshot?.pinnedThreads)
      && Array.isArray(record.snapshot?.projects);
  }
  function restoreSnapshot() {
    if (cacheRestored) return;
    cacheRestored = true;
    let record = window.__codexElvesRecentSessionsSnapshotCache;
    if (!validCache(record)) {
      record = null;
      const key = persistentCacheKey();
      if (key) {
        try {
          const raw = window.localStorage.getItem(key);
          if (raw && raw.length <= cacheMaxChars) record = JSON.parse(raw);
          if (raw && !validCache(record)) window.localStorage.removeItem(key);
        } catch { record = null; }
      }
    }
    if (!validCache(record)) return;
    try {
      snapshot = compactSnapshot(record.snapshot, false);
      signature = JSON.stringify(snapshot);
      cachedSnapshot = true;
      snapshotComplete = true;
      snapshotSavedAt = record.savedAt;
    } catch { snapshot = null; signature = ""; }
  }
  function flushCache() {
    clearTimeout(cacheWriteTimer); cacheWriteTimer = null;
    if (!cacheDirty) return;
    cacheDirty = false;
    const record = window.__codexElvesRecentSessionsSnapshotCache;
    const key = persistentCacheKey();
    if (!key || !validCache(record)) return;
    lastCacheWriteAt = Date.now();
    try {
      const raw = JSON.stringify(record);
      if (raw.length <= cacheMaxChars) window.localStorage.setItem(key, raw);
      else window.localStorage.removeItem(key);
    } catch {}
  }
  function rememberSnapshot(changed = false, force = false) {
    if (!snapshot || cachedSnapshot || !snapshotComplete) return;
    window.__codexElvesRecentSessionsSnapshotCache = {
      version: 1, scope: cacheScope, savedAt: snapshotSavedAt,
      snapshot: compactSnapshot(snapshot, false)
    };
    const elapsed = Date.now() - lastCacheWriteAt;
    if (!persistentCacheKey() || !changed && !force && elapsed < 60000) return;
    cacheDirty = true;
    if (force || elapsed >= 5000) flushCache();
    else if (!cacheWriteTimer) cacheWriteTimer = setTimeout(flushCache, 5000 - elapsed);
  }
  function removeThread(id) {
    if (!id) return;
    deletedIds.add(normalizedId(id));
    if (!snapshot) return;
    snapshot = compactSnapshot(snapshot);
    signature = JSON.stringify(snapshot);
    priorityIds?.delete(id); priorityKnownIds.delete(id);
    prioritySelectionSuppressions.delete(id);
    // 删除回执已确认，即使列表目前关闭，也立即更新之前的成功快照。
    const wasCached = cachedSnapshot; cachedSnapshot = false;
    rememberSnapshot(true, true); cachedSnapshot = wasCached;
    if (active) render();
  }
  function attention(item) {
    const side = item.sideThreadAttention;
    const own = item.priority?.attentionState;
    const status = item.status?.status || item.status;
    const type = typeof status === "string" ? status : status?.type;
    if (side?.waiting || own === "waiting" || !own && (type === "waiting" || (status?.activeFlags || []).some(flag =>
      flag === "waitingOnApproval" || flag === "waitingOnUserInput"))) return "waiting";
    if (side?.unread || own === "unread" || item.hasUnreadTurn) return "unread";
    return side?.active || own === "active" || !own && (type === "active" || type === "inProgress") ? "active" : "idle";
  }
  const eligible = item => item.priority?.eligible !== false;
  const needsPriority = item => eligible(item) && attention(item) !== "idle";
  const mainThread = item => item.threadSource !== "subagent"
    && !item.source?.subAgent && !item.source?.subagent;
  function recentItems() {
    const ids = new Set();
    const sideIds = new Set(snapshot?.sideThreadIds || []);
    return [...(snapshot?.recent || []), ...(snapshot?.pinnedThreads || [])].filter(item => {
      if (!item.id || ids.has(item.id) || sideIds.has(item.id) || !mainThread(item)) return false;
      ids.add(item.id);
      return true;
    });
  }
  function updatePriority(items) {
    const order = { waiting: 0, unread: 1, active: 2, idle: 3 };
    const candidates = items.filter(item => needsPriority(item)
      && !(item.id === selectedId && prioritySelectionSuppressions.has(item.id))).sort((left, right) =>
      order[attention(left)] - order[attention(right)] || updatedAt(right) - updatedAt(left));
    if (priorityIds === null) {
      priorityOpenedAt = Date.now();
      priorityKnownIds = new Set(items.map(item => item.id));
      priorityIds = new Set(candidates.map(item => item.id));
    } else {
      const available = new Set(items.filter(eligible).map(item => item.id));
      const created = items.filter(item => eligible(item) && !priorityKnownIds.has(item.id)
        && item.createdAt >= Math.floor(priorityOpenedAt / 1000) * 1000);
      priorityIds = new Set([
        ...created.map(item => item.id), ...[...priorityIds].filter(id => available.has(id)),
        ...candidates.map(item => item.id)
      ]);
      for (const item of items) priorityKnownIds.add(item.id);
    }
  }
  function projectName(item) {
    return snapshot?.projects?.find(project => project.id === item.projectId)?.name
      || String(item.cwd || "").replace(/[\\/]+$/, "").split(/[\\/]/).pop()
      || "未分配项目";
  }
  function groups() {
    const items = recentItems();
    updatePriority(items);
    const recent = items.sort((left, right) => updatedAt(right) - updatedAt(left)
        || (right.createdAt || 0) - (left.createdAt || 0) || left.id.localeCompare(right.id));
    const byId = new Map(recent.map(item => [item.id, item]));
    const priority = [...priorityIds].flatMap(id => byId.has(id) ? [byId.get(id)] : []);
    const dated = recent.filter(item => !priorityIds.has(item.id));
    const visible = dated.slice(0, limit);
    const today = new Date(); today.setHours(0, 0, 0, 0);
    const yesterday = new Date(today); yesterday.setDate(yesterday.getDate() - 1);
    const result = [
      { key: "priority", label: "优先级", items: priority },
      { key: "today", label: "今天", items: [] },
      { key: "yesterday", label: "昨天", items: [] },
      { key: "history", label: "历史", items: [], more: visible.length < dated.length }
    ];
    for (const item of visible) {
      const index = updatedAt(item) >= today.getTime() ? 1 : updatedAt(item) >= yesterday.getTime() ? 2 : 3;
      result[index].items.push(item);
    }
    return result;
  }
  function element(tag, className = "", text = "") {
    const node = document.createElement(tag);
    node.className = className;
    node.textContent = text;
    return node;
  }
  function icon(kind) {
    const paths = {
      recent: ["M8 3a5 5 0 1 1-4.4 2.6", "M1.5 3.5v3h3", "M8 5v3l2 1.5"],
      chevron: ["m5 6 3 3 3-3"],
      more: ["M3 8h.01M8 8h.01M13 8h.01"],
      clear: ["m6 3 7 7-3 3H6L2 9l6-6", "m5 6 6 6", "M9 13h5"]
    };
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    for (const [key, value] of Object.entries({
      viewBox: "0 0 16 16", width: "16", height: "16", fill: "none",
      stroke: "currentColor", "stroke-width": "1.3", "stroke-linecap": "round",
      "stroke-linejoin": "round", "aria-hidden": "true", focusable: "false"
    })) svg.setAttribute(key, value);
    for (const d of paths[kind] || paths.recent) {
      const path = document.createElementNS(svg.namespaceURI, "path");
      path.setAttribute("d", d); svg.append(path);
    }
    return svg;
  }
  function button(label, action, kind = null, className = "") {
    const node = element("button", className);
    node.type = "button"; node.title = label; node.setAttribute("aria-label", label);
    if (kind) node.append(icon(kind)); else node.textContent = label;
    node.addEventListener("click", action);
    return node;
  }
  function installStyle() {
    if (document.getElementById(styleId)) return;
    const style = document.createElement("style");
    style.id = styleId;
    style.textContent = `
      .${hostClass}{position:relative!important;min-height:0!important;overflow:hidden!important}
      .${hostClass}>:not(${rootSelector}){display:none!important}
      ${rootSelector}{
        --recent-text:var(--color-text-primary,#dedede);
        --recent-muted:var(--color-text-tertiary,#888);
        --recent-hover:var(--color-background-primary-soft-hover,#ffffff10);
        position:absolute;inset:0;z-index:2;display:flex;flex-direction:column;
        min-width:0;min-height:0;color:var(--recent-text);
        background:transparent;font:inherit;
      }
      ${rootSelector} *{box-sizing:border-box}
      ${rootSelector} button{cursor:pointer;font:inherit;color:inherit}
      ${rootSelector} button:disabled{cursor:default;opacity:.5}
      ${rootSelector} button:focus-visible{
        outline:2px solid var(--color-text-info-ghost,#58a6ff);outline-offset:2px
      }
      [data-codex-recent-sessions-entry]{-webkit-app-region:no-drag;cursor:pointer;flex:none}
      [data-codex-recent-sessions-entry][aria-pressed=true]{
        color:var(--color-text-info-ghost,#58a6ff)!important;
        background:var(--color-background-info-soft-alpha,#3291ff26)!important
      }
      .codex-recent-icon-button{display:grid;place-items:center;flex:none;width:28px;height:28px;
        padding:0;border:0;border-radius:7px;background:transparent;color:var(--recent-muted)!important}
      .codex-recent-icon-button:hover{background:var(--recent-hover);color:var(--recent-text)!important}
      .codex-recent-feedback{flex:none;font-size:12px;line-height:18px;padding:6px 16px;color:var(--recent-muted)}
      .codex-recent-feedback:empty{display:none}
      .codex-recent-feedback[data-error=true]{color:var(--color-text-danger-ghost,#fa746f)}
      .codex-recent-scroll{flex:1;min-height:0;overflow:auto;overscroll-behavior:contain;padding:0 8px 12px;
        scrollbar-width:thin}
      .codex-recent-list{width:100%}
      .codex-recent-section{margin:0 0 8px}
      .codex-recent-section-heading{display:flex;align-items:center;gap:8px;min-height:36px;padding:0 8px}
      .codex-recent-section-toggle{flex:1;display:flex;align-items:center;gap:8px;min-width:0;
        border:0;padding:6px 0;background:transparent;color:var(--recent-muted)!important;
        text-align:left}
      .codex-recent-section-toggle svg{width:12px;height:12px}
      .codex-recent-section-toggle[aria-expanded=false] svg{transform:rotate(-90deg)}
      .codex-recent-row{display:grid;grid-template-columns:minmax(0,1fr) 32px;align-items:center;
        min-height:48px;border-radius:8px}
      .codex-recent-row:hover,.codex-recent-row:focus-within,.codex-recent-row[data-selected=true]{background:var(--recent-hover)}
      .codex-recent-link{display:flex;flex-direction:column;justify-content:center;align-items:stretch;
        min-width:0;width:100%;min-height:48px;border:0;background:transparent;text-align:left;padding:6px 8px}
      .codex-recent-title,.codex-recent-project{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
      .codex-recent-title{font-size:13px;line-height:20px}
      .codex-recent-project{font-size:11px;line-height:16px;color:var(--recent-muted)}
      .codex-recent-row-tail{display:grid;place-items:center;grid-area:1/2}
      .codex-recent-state{grid-area:1/1;pointer-events:none;width:6px;height:6px;
        border-radius:50%;background:var(--color-text-info-ghost,#3894ff)}
      .codex-recent-state[data-active=true]{width:10px;height:10px;border:1.5px solid var(--recent-muted);
        border-top-color:transparent;background:transparent;animation:codex-recent-spin 1s linear infinite}
      .codex-recent-row-menu{grid-area:1/1;opacity:0}
      .codex-recent-row:is(:hover,:focus-within) .codex-recent-row-menu,
      .codex-recent-row-menu[aria-expanded=true]{opacity:1}
      .codex-recent-row .codex-recent-row-menu:disabled{opacity:0}
      .codex-recent-row:has(.codex-recent-row-menu:not(:disabled)):is(:hover,:focus-within) .codex-recent-state,
      .codex-recent-row-tail:has([aria-expanded=true]) .codex-recent-state{opacity:0}
      .codex-recent-more{border:0;background:transparent;padding:8px;font-size:12px!important;
        color:var(--recent-muted)!important;text-align:left;border-radius:6px}
      .codex-recent-more:hover{background:var(--recent-hover)}
      .codex-recent-empty{padding:36px 8px;color:var(--recent-muted);font-size:13px}
      .codex-recent-menu{position:fixed;z-index:100000;display:flex;flex-direction:column;gap:2px;
        min-width:160px;padding:5px;border:1px solid var(--color-border-primary-outline,#555);
        border-radius:10px;background:var(--color-surface-secondary,#303030);
        color:var(--color-text-primary,#dedede);box-shadow:0 8px 28px #0003}
      .codex-recent-menu button{font:inherit;font-size:13px;text-align:left;padding:8px 10px;
        border:0;border-radius:6px;background:transparent;color:inherit;cursor:pointer}
      .codex-recent-menu button:hover,.codex-recent-menu button:focus-visible{
        background:var(--color-background-primary-soft-hover,#ffffff10)}
      .codex-recent-menu button[data-danger=true]{color:var(--color-text-danger-ghost,#fa746f)}
      .codex-recent-menu button:disabled{opacity:.5;cursor:default}
      @keyframes codex-recent-spin{to{transform:rotate(360deg)}}
      @media(prefers-reduced-motion:reduce){.codex-recent-state[data-active=true]{animation:none}}
    `;
    document.head.append(style);
  }
  function activityButton() {
    return [...document.querySelectorAll("button[aria-pressed]")].find(node =>
      !node.matches(entrySelector) && /^(?:查看活动|打开活动视图|关闭活动视图|View activity|Open activity view|Close activity view)(?:[，,].*)?$/i
        .test(node.getAttribute("aria-label") || node.title || ""));
  }
  function syncEntry() {
    if (!options.enabled) { close(); entry?.remove(); entry = null; return; }
    const anchor = activityButton();
    if (!anchor) { entry?.remove(); entry = null; return; }
    if (!entry?.isConnected) {
      entry = anchor.cloneNode(false);
      entry.removeAttribute("id");
      entry.removeAttribute("aria-controls");
      entry.setAttribute("data-codex-recent-sessions-entry", "true");
      entry.setAttribute("aria-label", "最近会话");
      entry.title = "最近会话";
      entry.setAttribute("data-color", "secondary");
      entry.setAttribute("data-variant", "transparent");
      const inner = anchor.firstElementChild?.cloneNode(false) || element("span");
      inner.append(icon("recent")); entry.replaceChildren(inner);
      entry.addEventListener("click", () => active ? close() : void open());
    }
    if (entry.getAttribute("aria-pressed") !== String(active))
      entry.setAttribute("aria-pressed", String(active));
    const wrapper = anchor.parentElement?.classList.contains("contents") ? anchor.parentElement : anchor;
    if (entry.parentElement !== wrapper.parentElement || entry.nextElementSibling !== wrapper)
      wrapper.before(entry);
  }
  function stopEntryObserver() {
    entryObserver?.disconnect(); entryObserver = null; entryObserverRoots = [];
    entrySyncTask = null;
  }
  function observeEntryLifecycle() {
    if (!options.enabled || disposed) { stopEntryObserver(); return; }
    const anchor = activityButton(), roots = [];
    // 只监听工具栏到 body 的直接子节点变化，覆盖页面外层替换，避开消息流。
    for (let node = anchor?.parentElement; node; node = node.parentElement) {
      roots.push(node);
      if (node === document.body) break;
    }
    const subtree = !anchor;
    // 原生侧栏尚未出现时等待其挂载，找到工具栏后立即切回浅层监听。
    if (subtree) roots.push(document.body || document.documentElement);
    if (entryObserver && entryObserverSubtree === subtree && roots.length === entryObserverRoots.length
      && roots.every((node, index) => node === entryObserverRoots[index])) return;
    if (!entryObserver) entryObserver = new MutationObserver(mutations => {
      if (!options.enabled || disposed || !mutations.some(mutation =>
        mutation.removedNodes.length || [...mutation.addedNodes].some(node => node !== entry))) return;
      if (active && (!host?.isConnected || root?.parentElement !== host)) pauseReads();
      if (entrySyncTask) return;
      const task = entrySyncTask = {};
      // DOM 挂载不依赖绘制帧，后台窗口返回页面时也能先恢复入口。
      queueMicrotask(() => {
        if (entrySyncTask !== task || !options.enabled || disposed) return;
        entrySyncTask = null;
        syncEntry();
        if (active) {
          reconcileHost();
          if (!canRead()) closeMenu();
          syncLifecycle(true);
          if (canRead()) syncSelection();
        }
        observeEntryLifecycle();
      });
    });
    entryObserver.disconnect();
    entryObserverRoots = roots; entryObserverSubtree = subtree;
    for (const node of roots) entryObserver.observe(node, { childList: true, subtree });
  }
  function onSidebarEvent(event) {
    if (typeof event.threadId === "string") {
      const runtimeStatus = event.status?.status ?? event.status;
      const runtimeType = typeof runtimeStatus === "string" ? runtimeStatus : runtimeStatus?.type;
      if (["thread/started", "turn/started", "remote/requestsChanged"].includes(event.method)
          || event.method === "thread/status/changed"
            && ["active", "inProgress", "running", "pending"].includes(runtimeType))
        prioritySelectionSuppressions.delete(event.threadId);
    }
    if (["thread/deleted", "thread/archived"].includes(event.method)) removeThread(event.threadId);
    if (event.method === "thread/unarchived") deletedIds.delete(normalizedId(event.threadId));
    queueRefresh();
  }
  async function adapter() {
    window.__codexElvesInstallRecentSessionsAdapter?.();
    const runtime = window.__codexElvesRemoteAccess;
    if (typeof runtime?.recentSidebar !== "function" || typeof runtime.openThread !== "function")
      throw new Error("原生会话服务尚未就绪，请重试");
    if (canRead() && subscribedAdapter !== runtime) {
      releaseAdapter();
      subscribedAdapter = runtime;
      unsubscribeSidebar = runtime.subscribeSidebar?.(onSidebarEvent) || null;
    }
    return runtime;
  }
  async function call(action, payload = {}) {
    const runtime = await adapter();
    const result = await runtime.recentAction(action, payload);
    return checkedResult(result);
  }
  function checkedResult(result) {
    if (!result?.ok) {
      const error = new Error(result?.message || "原生会话操作失败");
      error.code = result?.code;
      error.outcomeUnknown = result?.outcomeUnknown === true;
      throw error;
    }
    return result;
  }
  function releaseAdapter() {
    unsubscribeSidebar?.(); unsubscribeSidebar = null;
    const runtime = subscribedAdapter; subscribedAdapter = null;
    runtime?.releaseSidebar?.();
  }
  function report(message, error = false) {
    if (!feedback) return;
    feedback.textContent = message;
    feedback.dataset.error = String(error);
    if (error) feedback.append(button("重试", () => void refresh(), null, "codex-recent-more"));
  }
  function canRead() {
    return active && !disposed && options.enabled && !document.hidden && host?.isConnected
      && root?.parentElement === host && host.getClientRects().length > 0 && host.clientHeight > 0;
  }
  function pauseReads() {
    clearTimeout(timer); timer = null;
    flushCache();
    if (warmupFrame) cancelAnimationFrame(warmupFrame);
    warmupFrame = 0;
    request?.paint?.cancel();
    if (!paused) { revision++; request = null; }
    paused = true;
    releaseAdapter();
  }
  function scheduleRefresh(delay = 5000) {
    clearTimeout(timer); timer = null;
    if (canRead()) timer = setTimeout(() => { timer = null; void refresh(); }, delay);
    else pauseReads();
  }
  function queueRefresh() {
    if (!canRead()) return;
    if (request?.revision === revision) { request.again = true; return; }
    scheduleRefresh(Math.max(200, 1000 - (Date.now() - lastReadAt)));
  }
  function syncLifecycle(defer = false) {
    if (!canRead()) { pauseReads(); return; }
    if (snapshot && renderedDay !== localDay()) render();
    if (!paused) return;
    paused = false; revision++; request = null;
    if (defer && snapshot) {
      const token = revision;
      warmupFrame = requestAnimationFrame(() => {
        warmupFrame = 0;
        if (token === revision && canRead()) scheduleRefresh(0);
      });
    } else void refresh({ seed: !snapshot });
  }
  function applySnapshot(result) {
    const value = compactSnapshot(result);
    const nextSignature = JSON.stringify(value);
    const changed = nextSignature !== signature;
    const redraw = changed || cachedSnapshot || renderedDay !== localDay();
    snapshot = value; signature = nextSignature;
    cachedSnapshot = false; snapshotComplete = result.complete !== false;
    if (snapshotComplete) snapshotSavedAt = Date.now();
    report("");
    if (redraw) render();
    rememberSnapshot(changed);
  }
  function yieldToPaint(current) {
    return new Promise(resolve => {
      const paint = { frame: 0, timer: null, cancel() {
        if (paint.frame) cancelAnimationFrame(paint.frame);
        clearTimeout(paint.timer); current.paint = null; resolve();
      }};
      current.paint = paint;
      paint.frame = requestAnimationFrame(() => {
        paint.frame = 0;
        paint.timer = setTimeout(() => { current.paint = null; resolve(); }, 0);
      });
    });
  }
  async function refresh({ seed = false } = {}) {
    if (!canRead()) { pauseReads(); return; }
    if (snapshot && renderedDay !== localDay()) render();
    if (request?.revision === revision) return request.promise;
    const token = revision, scope = cacheScope;
    const current = { revision: token, again: false, promise: null };
    const currentRead = () => token === revision && scope === cacheScope && canRead();
    current.promise = (async () => {
      if (!snapshot) report("正在读取最近会话…");
      try {
        const runtime = await adapter();
        if (!currentRead()) return;
        lastReadAt = Date.now();
        if (seed && !snapshot) {
          const initial = checkedResult(await runtime.recentSidebar({ cachedOnly: true }));
          if (!currentRead()) return;
          if (initial.complete || initial.recent?.length || initial.pinnedThreads?.length) applySnapshot(initial);
          if (snapshot) await yieldToPaint(current);
          if (!currentRead()) return;
        }
        const result = checkedResult(await runtime.recentSidebar());
        if (currentRead()) applySnapshot(result);
      } catch (error) {
        if (currentRead()) {
          if (error.code === 409) current.again = true;
          else report(error.message, true);
        }
      } finally {
        if (request === current) request = null;
        if (currentRead()) scheduleRefresh(current.again ? Math.max(200, 1000 - (Date.now() - lastReadAt)) : 5000);
        else if (token === revision) pauseReads();
      }
    })();
    request = current;
    return current.promise;
  }
  async function openThread(item) {
    try {
      const native = await adapter();
      if (typeof native.openThread !== "function") throw new Error("原生会话导航尚未就绪，请重试");
      options.beforeNavigate?.();
      prioritySelectionSuppressions.add(item.id);
      const result = await native.openThread(item.id);
      if (!result?.ok) throw new Error(result?.message || "无法打开此会话");
      selectedId = item.id;
      syncSelection(false);
      void refresh();
    } catch (error) {
      prioritySelectionSuppressions.delete(item.id);
      report(error.message, true);
    }
  }
  function closeMenu(restoreFocus = false) {
    if (!menu) return;
    const previous = menu; menu = null;
    previous.node.remove();
    previous.trigger.setAttribute("aria-expanded", "false");
    if (restoreFocus && previous.trigger.isConnected) previous.trigger.focus();
    if (deferredRender && active && !disposed) { deferredRender = false; render(); }
  }
  async function pinThread(item) {
    const scope = cacheScope;
    busy.add(item.id); render();
    try {
      await call("pinThread", { threadId: item.id, pinned: !isPinned(item) });
      await request?.promise;
      if (scope === cacheScope) await refresh();
    } catch (error) { report(error.message, true); }
    finally { busy.delete(item.id); if (active) render(); }
  }
  async function deleteThread(item) {
    closeMenu();
    if (typeof options.confirmDelete !== "function" || !await options.confirmDelete(item.title || "新会话")) return;
    if (!active || disposed || busy.has(item.id) || uncertainDeletions.has(item.id)) return;
    const scope = cacheScope;
    busy.add(item.id); render();
    try {
      const result = await call("deleteThread", { threadId: item.id });
      if (scope !== cacheScope) return;
      removeThread(item.id);
      await request?.promise;
      if (result.message) options.notify?.(result.message);
      await refresh();
    } catch (error) {
      if (error.outcomeUnknown) uncertainDeletions.add(item.id);
      report(error.message, true);
    } finally { busy.delete(item.id); if (active) render(); }
  }
  function isPinned(item) {
    return item.pinned || snapshot?.pinnedThreads?.some(thread => thread.id === item.id);
  }
  function openMenu(item, trigger) {
    if (menu?.trigger === trigger) { closeMenu(true); return; }
    closeMenu();
    const node = element("div", "codex-recent-menu");
    node.setAttribute("role", "menu"); node.setAttribute("aria-label", `${item.title || "新会话"} 的会话菜单`);
    const choices = [
      button("打开会话", () => { closeMenu(); void openThread(item); }),
      button(isPinned(item) ? "取消置顶会话" : "置顶会话", () => { closeMenu(); void pinThread(item); }),
      button(uncertainDeletions.has(item.id) ? "删除结果待核对" : "删除会话", () => void deleteThread(item))
    ];
    choices[2].dataset.danger = "true";
    choices[2].disabled = uncertainDeletions.has(item.id) || busy.has(item.id);
    for (const choice of choices) choice.setAttribute("role", "menuitem");
    node.append(...choices); document.body.append(node);
    trigger.setAttribute("aria-expanded", "true");
    menu = { node, trigger };
    const rect = trigger.getBoundingClientRect(), bounds = node.getBoundingClientRect();
    node.style.left = `${Math.max(8, Math.min(rect.right - bounds.width, innerWidth - bounds.width - 8))}px`;
    node.style.top = `${Math.max(8, Math.min(rect.bottom + 4, innerHeight - bounds.height - 8))}px`;
    node.addEventListener("keydown", event => {
      const enabled = choices.filter(choice => !choice.disabled);
      const index = enabled.indexOf(document.activeElement);
      const next = { ArrowDown: (index + 1) % enabled.length,
        ArrowUp: (index + enabled.length - 1) % enabled.length, Home: 0, End: enabled.length - 1 }[event.key];
      if (next === undefined) return;
      event.preventDefault(); enabled[next]?.focus();
    });
    choices[0].focus();
  }
  function row(item) {
    const node = element("div", "codex-recent-row");
    node.dataset.threadId = item.id;
    const link = button(item.title || "新会话", () => void openThread(item), null, "codex-recent-link");
    link.dataset.recentFocus = `thread:${item.id}`;
    link.replaceChildren(element("span", "codex-recent-title", item.title || "新会话"),
      element("span", "codex-recent-project", projectName(item)));
    const tail = element("div", "codex-recent-row-tail");
    const state = attention(item);
    const running = state !== "waiting" && (item.sideThreadAttention?.active === true
      || (typeof item.priority?.active === "boolean" ? item.priority.active
        : ["active", "inProgress"].includes(item.status?.type || item.status)));
    if (running || state === "unread" || state === "waiting") {
      const indicator = element("span", "codex-recent-state");
      indicator.dataset.active = String(running);
      indicator.title = state === "waiting" ? "等待你的回复" : running ? "正在运行" : "有未读消息";
      indicator.setAttribute("role", "img"); indicator.setAttribute("aria-label", indicator.title);
      tail.append(indicator);
    }
    const more = button(`${item.title || "新会话"} 的会话菜单`, () => openMenu(item, more),
      "more", "codex-recent-icon-button codex-recent-row-menu");
    more.dataset.recentFocus = `menu:${item.id}`;
    more.setAttribute("aria-haspopup", "menu"); more.setAttribute("aria-expanded", "false");
    more.disabled = busy.has(item.id) || cachedSnapshot;
    node.addEventListener("contextmenu", event => {
      event.preventDefault();
      if (!more.disabled) openMenu(item, more);
    });
    tail.append(more); node.append(link, tail);
    return node;
  }
  function syncSelection(readNative = true) {
    if (readNative) selectedId = String(options.currentSessionId?.() || "").replace(/^local:/, "");
    if (renderedSelection === selectedId) return;
    renderedSelection = selectedId;
    for (const node of list?.querySelectorAll(".codex-recent-row") || []) {
      const selected = node.dataset.threadId === selectedId;
      if (node.dataset.selected !== String(selected)) node.dataset.selected = String(selected);
      const link = node.querySelector(".codex-recent-link");
      if (selected && link.getAttribute("aria-current") !== "page") link.setAttribute("aria-current", "page");
      else if (!selected && link.hasAttribute("aria-current")) link.removeAttribute("aria-current");
    }
  }
  function render() {
    if (!list || !snapshot) return;
    const scroll = list.parentElement, top = scroll.scrollTop;
    const focused = list.contains(document.activeElement) ? document.activeElement.dataset.recentFocus : null;
    // 菜单操作期间保留原触发按钮，避免状态刷新打断键盘和鼠标操作。
    if (menu) { deferredRender = true; return; }
    deferredRender = false;
    renderedSelection = null;
    list.replaceChildren();
    for (const group of groups()) {
      if (!group.items.length && !group.more) continue;
      const section = element("section", "codex-recent-section");
      section.dataset.group = group.key;
      const heading = element("div", "codex-recent-section-heading");
      const expanded = !collapsed.has(group.key);
      const toggle = button(`${group.label}分组`, () => {
        collapsed.has(group.key) ? collapsed.delete(group.key) : collapsed.add(group.key);
        render();
      }, null, "codex-recent-section-toggle");
      toggle.replaceChildren(element("span", "text-base font-medium browser:leading-4.5", group.label), icon("chevron"));
      toggle.dataset.recentFocus = `group:${group.key}`;
      toggle.setAttribute("aria-expanded", String(expanded));
      const body = element("div");
      body.id = `codex-recent-section-${group.key}`;
      toggle.setAttribute("aria-controls", body.id);
      heading.append(toggle);
      if (group.key === "priority" && group.items.some(item => attention(item) === "idle")) {
        heading.append(button("清除已读会话", () => {
          for (const item of recentItems()) if (!needsPriority(item)) priorityIds.delete(item.id);
          render();
        }, "clear", "codex-recent-icon-button"));
      }
      body.hidden = !expanded;
      body.append(...group.items.map(row));
      if (group.more) body.append(button("显示更多会话", () => { limit += 40; render(); }, null, "codex-recent-more"));
      section.append(heading, body); list.append(section);
    }
    if (!list.children.length) list.append(element("p", "codex-recent-empty", "暂无最近会话"));
    renderedDay = localDay();
    syncSelection();
    scroll.scrollTop = top;
    if (focused) [...list.querySelectorAll("[data-recent-focus]")]
      .find(node => node.dataset.recentFocus === focused)?.focus({ preventScroll: true });
  }
  async function open() {
    if (!options.enabled || disposed || active || opening) return;
    opening = true;
    const token = ++revision;
    const activity = activityButton();
    if (activity?.getAttribute("aria-pressed") === "true") {
      activity.click();
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    if (disposed || !options.enabled || token !== revision) return;
    opening = false;
    host = sidebarHost();
    if (!host) { options.notify?.("未找到 Codex 侧边会话栏，最近会话暂不可用"); return; }
    hostScrollTop = host.scrollTop;
    active = true; revision += 1;
    priorityIds = null; priorityKnownIds = new Set(); limit = 40; collapsed.clear();
    restoreSnapshot();
    if (snapshot) {
      snapshot = compactSnapshot(snapshot, false);
      signature = JSON.stringify(snapshot);
      cachedSnapshot = true;
    }
    root = element("div"); root.setAttribute("data-codex-recent-sessions-root", "true");
    root.setAttribute("role", "region"); root.setAttribute("aria-label", "最近会话");
    feedback = element("div", "codex-recent-feedback"); feedback.setAttribute("role", "status");
    const scroll = element("div", "codex-recent-scroll");
    scroll.addEventListener("scroll", () => closeMenu(), { passive: true });
    list = element("div", "codex-recent-list"); scroll.append(list);
    root.append(feedback, scroll);
    host.classList.add(hostClass); host.scrollTop = 0; host.append(root); syncEntry();
    observeHost();
    if (snapshot) render();
    syncLifecycle(!!snapshot);
  }
  function close(restoreFocus = false) {
    active = false; opening = false; revision += 1;
    pauseReads();
    hostObserver?.disconnect(); hostObserver = null;
    closeMenu();
    root?.remove();
    if (host) { host.classList.remove(hostClass); host.scrollTop = hostScrollTop; }
    root = null; host = null; list = null; feedback = null;
    renderedSelection = null;
    entry?.setAttribute("aria-pressed", "false");
    if (restoreFocus) entry?.focus({ preventScroll: true });
  }
  function onClick(event) {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    if (!target) return;
    if (menu && !menu.node.contains(target) && !menu.trigger.contains(target)) closeMenu();
    if (!active || root?.contains(target) || target.closest(entrySelector) || menu?.node.contains(target)) return;
    if (activityButton()?.contains(target)) close();
  }
  function onKey(event) {
    if (event.key !== "Escape") return;
    if (menu) { event.preventDefault(); event.stopImmediatePropagation(); closeMenu(true); render(); }
  }
  function onVisibility() {
    syncLifecycle(true);
  }
  function onResize() { closeMenu(); syncLifecycle(true); }
  function sidebarHost() {
    const navigation = activityButton()?.closest("nav, [role='navigation'], [data-sidebar-navigation-root]");
    return navigation?.querySelector("[data-app-action-sidebar-scroll]") || null;
  }
  function onNavigation() {
    if (!active) return;
    reconcileHost();
    syncLifecycle(true);
    if (canRead()) { syncSelection(); queueRefresh(); }
  }
  function observeHost() {
    hostObserver?.disconnect(); hostObserver = null;
    if (host && typeof ResizeObserver === "function") {
      hostObserver = new ResizeObserver(() => syncLifecycle(true));
      hostObserver.observe(host);
    }
  }
  function reconcileHost() {
    const nextHost = sidebarHost();
    if (!nextHost || host === nextHost && root?.parentElement === nextHost) return;
    pauseReads();
    if (host) { host.classList.remove(hostClass); host.scrollTop = hostScrollTop; }
    host = nextHost; hostScrollTop = host.scrollTop;
    host.classList.add(hostClass); host.scrollTop = 0; host.append(root);
    observeHost();
  }
  function onCacheStorage(event) {
    if (event.key !== persistentCacheKey()) return;
    if (active) queueRefresh();
    else { snapshot = null; signature = ""; cacheRestored = false; cachedSnapshot = false; snapshotComplete = false; }
  }
  function listen() {
    if (listening) return;
    listening = true;
    document.addEventListener("click", onClick, true);
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("resize", onResize);
    window.addEventListener("popstate", onNavigation);
    window.addEventListener("hashchange", onNavigation);
    window.addEventListener("storage", onCacheStorage);
  }
  window.__codexElvesRecentSessions = {
    version,
    configure(next) {
      options = next;
      const scope = resolvedScope();
      if (scope !== cacheScope) {
        pauseReads();
        cacheScope = scope; cacheRestored = false; cachedSnapshot = false; snapshotComplete = false;
        snapshot = null; signature = ""; renderedDay = ""; lastCacheWriteAt = 0; snapshotSavedAt = 0;
        priorityIds = null; priorityKnownIds = new Set(); prioritySelectionSuppressions.clear();
        deletedIds.clear(); uncertainDeletions.clear();
        if (active) {
          list?.replaceChildren();
          restoreSnapshot();
          if (snapshot) render();
        }
      }
      if (options.enabled) { installStyle(); listen(); }
      syncEntry();
      observeEntryLifecycle();
      if (active) {
        reconcileHost();
        syncLifecycle(true);
        if (canRead()) syncSelection();
      }
    },
    open, close,
    dispose() {
      disposed = true; stopEntryObserver(); close(); entry?.remove(); entry = null;
      if (listening) {
        document.removeEventListener("click", onClick, true);
        document.removeEventListener("keydown", onKey, true);
        document.removeEventListener("visibilitychange", onVisibility);
        window.removeEventListener("resize", onResize);
        window.removeEventListener("popstate", onNavigation);
        window.removeEventListener("hashchange", onNavigation);
        window.removeEventListener("storage", onCacheStorage);
      }
      document.getElementById(styleId)?.remove();
    }
  };
})();
