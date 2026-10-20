let menuSequence = 0;

function element(tag, className = "", text = "") {
  const node = document.createElement(tag);
  node.className = className;
  if (text) node.textContent = text;
  return node;
}
const nonempty = value => typeof value === "string" && value.trim().length > 0;
const finiteTime = value => Number.isFinite(value) && value >= 0;
function duration(ms) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}秒`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}分 ${String(seconds % 60).padStart(2, "0")}秒`
    : `${Math.floor(minutes / 60)}时 ${String(minutes % 60).padStart(2, "0")}分`;
}
function menuResponse(value, threadId) {
  const validItem = item => item && nonempty(item.id);
  if (!value || value.ok !== true || value.threadId !== threadId
      || !Array.isArray(value.subagents) || !value.subagents.every(validItem)
      || !Array.isArray(value.sideThreads) || !value.sideThreads.every(validItem)
      || !Number.isInteger(value.completedSubagentCount) || value.completedSubagentCount < 0) {
    throw new Error("会话菜单数据格式无效，请重试。");
  }
  return { ...value, subagents: [...value.subagents], sideThreads: [...value.sideThreads] };
}
function menuSignature(data) {
  return JSON.stringify({ ...data,
    subagents: data.subagents.map(({ elapsedMs, ...item }) => item) });
}

export function mountSessionMenu({
  trigger, getContext, load, openChanges, openSubagent, createSideThread, openSideThread,
  closeSideThread, beforeOpen, onError, onData, cache
}) {
  let disposed = false, opened = false, owner = "", authorization = "", context = null, cached = null;
  let ticker = null, press = null, suppressedClick = null, blockedContext = null;
  const entries = new Map(), maxEntries = 32, refreshAgeMs = 30000;
  let contextSource = null, contextItem = null, contextPosition = null, message = "";
  const times = [], mutationButtons = new Map(), listeners = [];
  const originalAria = new Map(["aria-haspopup", "aria-expanded", "aria-controls"]
    .map(name => [name, trigger.getAttribute(name)]));
  const layer = element("div", "session-related-menu-layer");
  const menu = element("div", "session-related-menu");
  const actionMenu = element("div", "session-related-menu session-related-menu-context");
  const closeAction = element("button", "session-related-menu-item session-related-menu-danger", "关闭会话");
  const id = `session-related-menu-${++menuSequence}`;
  layer.hidden = true;
  menu.id = id; menu.tabIndex = -1;
  menu.setAttribute("role", "menu"); menu.setAttribute("aria-label", "会话菜单");
  actionMenu.hidden = true; actionMenu.setAttribute("role", "menu");
  actionMenu.setAttribute("aria-label", "侧边会话操作");
  closeAction.type = "button"; closeAction.tabIndex = -1; closeAction.setAttribute("role", "menuitem");
  actionMenu.append(closeAction); layer.append(menu, actionMenu);
  if (typeof layer.showPopover === "function") layer.setAttribute("popover", "manual");
  trigger.setAttribute("aria-haspopup", "menu"); trigger.setAttribute("aria-expanded", "false");
  trigger.setAttribute("aria-controls", id);

  function listen(target, name, handler, options) {
    target.addEventListener(name, handler, options);
    listeners.push(() => target.removeEventListener(name, handler, options));
  }
  function report(error) {
    if (typeof onError === "function") {
      try { onError(error instanceof Error ? error : new Error(String(error))); } catch {}
    }
  }
  function readContext() {
    let value;
    try { value = getContext?.() || {}; } catch (error) { report(error); value = {}; }
    const deviceId = nonempty(value.deviceId) ? value.deviceId : "";
    const cacheScope = nonempty(value.cacheScope) ? value.cacheScope : "";
    const threadId = nonempty(value.threadId) ? value.threadId : "";
    return { deviceId, cacheScope, threadId, readOnly: value.readOnly === true,
      createPending: value.createPending === true,
      authorization: JSON.stringify([deviceId, cacheScope]),
      connected: value.connected === true, owner: JSON.stringify([deviceId, cacheScope, threadId]) };
  }
  function syncContext() {
    const next = readContext();
    const changed = next.owner !== owner;
    if (next.authorization !== authorization) {
      authorization = next.authorization;
      entries.clear();
    }
    if (next.owner !== owner) {
      owner = next.owner; cached = entries.get(owner)?.snapshot || null; message = "";
      times.length = 0; mutationButtons.clear();
      if (opened) close();
      menu.replaceChildren();
    }
    context = next;
    if (changed && cached) onData?.(cached.data);
    return next;
  }
  const canRead = value => !!(value?.deviceId && value.cacheScope && value.threadId);
  const canMutate = value => canRead(value) && value.connected && !value.readOnly;
  function entryFor(value = syncContext()) {
    if (!canRead(value)) return null;
    let entry = entries.get(value.owner);
    if (!entry) entry = { context: value, snapshot: null, revision: 0,
      dirty: true, request: null, restoring: null, restored: false, again: false };
    entries.delete(value.owner); entries.set(value.owner, entry);
    while (entries.size > maxEntries) {
      const oldest = entries.keys().next().value;
      entries.delete(oldest);
    }
    return entry;
  }
  function accepts(entry, revision = entry.revision) {
    return !disposed && entry.revision === revision
      && readContext().authorization === entry.context.authorization
      && entries.get(entry.context.owner) === entry;
  }
  function save(entry) {
    try {
      Promise.resolve(cache?.put?.(entry.context.cacheScope, entry.context.threadId,
        { version: 1, ...entry.snapshot })).catch(() => {});
    } catch {}
  }
  function applySnapshot(entry, snapshot, persist = false) {
    const previous = entry.snapshot, changed = !previous || menuSignature(previous.data) !== menuSignature(snapshot.data);
    entry.snapshot = snapshot;
    if (persist) save(entry);
    if (!owns(entry.context.owner)) return;
    cached = snapshot; message = "";
    if (changed) onData?.(snapshot.data);
    if (opened && (changed || menu.querySelector(".session-related-menu-status"))) render();
    else if (opened) {
      for (const time of times) {
        time.item = snapshot.data.subagents.find(item => item.id === time.item.id) || time.item;
        time.loadedAtMs = snapshot.loadedAtMs;
      }
      updatePermissions(); updateTimes();
    }
  }
  function restore(entry) {
    if (!entry || entry.snapshot || entry.restored || typeof cache?.get !== "function") return null;
    if (entry.restoring) return entry.restoring;
    const revision = entry.revision;
    entry.restoring = Promise.resolve().then(() => cache.get(entry.context.cacheScope, entry.context.threadId))
      .then(saved => {
        if (!accepts(entry, revision) || entry.snapshot || saved?.version !== 1
            || !finiteTime(saved.loadedAtMs)) return;
        applySnapshot(entry, { data: menuResponse(saved.data, entry.context.threadId), loadedAtMs: saved.loadedAtMs });
      }).catch(() => {}).finally(() => {
        entry.restoring = null; entry.restored = true;
      });
    return entry.restoring;
  }
  function owns(expected) {
    return !disposed && syncContext().owner === expected;
  }
  function cancelPress({ suppress = false, blockContext = false } = {}) {
    if (!press) return;
    if (suppress) suppressedClick = { button: press.button, until: Date.now() + 2000 };
    if (blockContext) blockedContext = { button: press.button, until: Date.now() + 2000 };
    clearTimeout(press.timer); press = null;
  }
  function hideContext(restoreFocus = false) {
    const source = contextSource;
    actionMenu.hidden = true; contextSource = null; contextItem = null; contextPosition = null;
    source?.setAttribute("aria-expanded", "false");
    if (restoreFocus && source?.isConnected && opened) source.focus();
  }
  function close(restoreFocus = false) {
    if (!opened) { cancelPress(); return; }
    opened = false; clearInterval(ticker); ticker = null; cancelPress(); hideContext();
    trigger.setAttribute("aria-expanded", "false");
    if (layer.hasAttribute("popover")) {
      try { layer.hidePopover(); } catch {}
    }
    layer.hidden = true;
    if (restoreFocus && trigger.isConnected && !trigger.disabled) trigger.focus();
  }
  function updatePermissions() {
    for (const [button, kind] of mutationButtons) {
      button.disabled = !canMutate(context) || (kind === "create"
        && (context.createPending || cached?.data.canCreateSideThread === false || typeof createSideThread !== "function"));
    }
    closeAction.disabled = !canMutate(context) || typeof closeSideThread !== "function";
    if (closeAction.disabled) hideContext();
  }
  function updateTimes() {
    const now = Date.now();
    for (const { node, item, loadedAtMs } of times) {
      const elapsed = finiteTime(item.elapsedMs) ? item.elapsedMs + Math.max(0, now - loadedAtMs)
        : finiteTime(item.startedAtMs) ? Math.max(0, now - item.startedAtMs) : 0;
      const text = duration(elapsed);
      if (node.textContent !== text) node.textContent = text;
    }
  }
  function invoke(callback, args, expected, mutation = false, onSuccess) {
    if (!owns(expected) || mutation && !canMutate(context) || typeof callback !== "function") return;
    close();
    try {
      Promise.resolve(callback(...args)).then(() => {
        if (onSuccess && owns(expected)) onSuccess();
      }).catch(report);
    } catch (error) { report(error); }
  }
  function itemButton(label, callback, args, kind = "", item = null) {
    const node = element("button", "session-related-menu-item");
    node.type = "button"; node.tabIndex = -1; node.setAttribute("role", "menuitem");
    node.append(element("span", "session-related-menu-name", label));
    node.disabled = typeof callback !== "function";
    const expected = owner;
    node.addEventListener("click", event => {
      event.preventDefault(); event.stopPropagation();
      if (node.disabled) return;
      if (kind === "create" && (context.createPending || cached?.data.canCreateSideThread === false)) return;
      invoke(callback, args, expected, !!kind);
    });
    if (kind) mutationButtons.set(node, kind);
    if (item) bindSideThread(node, item, expected);
    return node;
  }
  function section(label, action = null) {
    const group = element("section", "session-related-menu-group");
    group.setAttribute("role", "group"); group.setAttribute("aria-label", label);
    const heading = element("div", "session-related-menu-heading", label);
    heading.setAttribute("role", "presentation");
    const header = element("div", "session-related-menu-group-header");
    header.append(heading);
    if (action) header.append(action);
    group.append(header);
    menu.append(group);
    return group;
  }
  function render() {
    const focusedKey = menu.contains(document.activeElement) ? document.activeElement.dataset.menuKey : null;
    cancelPress(); hideContext(); times.length = 0; mutationButtons.clear(); menu.replaceChildren();
    const tools = element("div", "session-related-menu-group");
    menu.append(tools);
    const changes = itemButton("变更", openChanges, []);
    changes.dataset.menuKey = "changes"; tools.append(changes);
    const subagents = section("子代理"), data = cached?.data;
    for (const item of data?.subagents || []) {
      const button = itemButton(nonempty(item.name) ? item.name : "子代理", openSubagent, [item]);
      button.classList.add("session-related-menu-thread");
      button.dataset.menuKey = `subagent:${item.id}`;
      const time = element("span", "session-related-menu-time");
      time.setAttribute("title", "执行时间");
      button.append(time); subagents.append(button);
      times.push({ node: time, item, loadedAtMs: cached.loadedAtMs });
    }
    if (data?.completedSubagentCount > 0) {
      const completed = element("div", "session-related-menu-completed");
      const icons = element("span", "session-related-menu-completed-icons");
      icons.setAttribute("aria-hidden", "true");
      for (let index = 0; index < Math.min(4, data.completedSubagentCount); index++) {
        const icon = element("span", "session-related-menu-agent-icon");
        icon.innerHTML = '<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M10 2v2M6 13c-2 .7-3 2-3 4m11-4c2 .7 3 2 3 4"/><rect x="4" y="4" width="12" height="8" rx="2.5"/><path d="M7 7.5v1M13 7.5v1"/></svg>';
        icons.append(icon);
      }
      completed.append(icons, element("span", "session-related-menu-completed-count",
        `已完成 ${data.completedSubagentCount} 个`));
      subagents.append(completed);
    }
    const create = itemButton("新建侧边会话", createSideThread, [], "create");
    create.dataset.menuKey = "create";
    create.classList.add("session-related-menu-heading-action");
    create.setAttribute("aria-label", "新建侧边会话"); create.title = "新建侧边会话";
    create.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
    const sides = section("侧边会话", create);
    for (const item of data?.sideThreads || []) {
      const button = itemButton(nonempty(item.title) ? item.title : "侧边会话", openSideThread, [item], "", item);
      button.classList.add("session-related-menu-thread");
      if (item.hasUnreadTurn === true) {
        const unread = element("span", "session-related-menu-unread");
        unread.setAttribute("role", "img"); unread.setAttribute("aria-label", "有未读消息");
        button.append(unread);
      }
      button.dataset.menuKey = `side:${item.id}`; sides.append(button);
    }
    let status = message;
    if (!canRead(context)) status = context?.threadId ? "当前授权不可用" : "请选择会话后查看相关会话";
    else if (!context.connected) status = data ? "当前离线，显示上次记录" : "当前离线，连接后读取相关会话";
    if (status) {
      const note = element("p", "session-related-menu-status", status);
      note.setAttribute("role", "status"); menu.append(note);
    }
    updatePermissions(); updateTimes();
    if (focusedKey) {
      [...menu.querySelectorAll("button")].find(node => node.dataset.menuKey === focusedKey && !node.disabled)?.focus();
    }
    if (opened) fit();
  }
  function viewport() {
    const view = window.visualViewport;
    return { top: view?.offsetTop || 0, left: view?.offsetLeft || 0,
      width: view?.width || window.innerWidth, height: view?.height || window.innerHeight };
  }
  function fit() {
    if (!opened) return;
    if (!trigger.isConnected || trigger.disabled || trigger.hidden) { close(); return; }
    const view = viewport(), box = trigger.getBoundingClientRect(), padding = 8;
    if (box.bottom <= view.top || box.top >= view.top + view.height
        || box.right <= view.left || box.left >= view.left + view.width) { close(); return; }
    const width = Math.max(0, Math.min(view.width * .65, view.width - padding * 2));
    const top = Math.max(view.top + padding, Math.min(box.bottom + 5, view.top + view.height - padding));
    menu.style.width = `${width}px`;
    menu.style.left = `${Math.max(view.left + padding, Math.min(box.right - width, view.left + view.width - width - padding))}px`;
    menu.style.top = `${top}px`;
    menu.style.maxHeight = `${Math.max(0, Math.min(480, view.top + view.height - top - padding))}px`;
    if (!actionMenu.hidden) {
      const source = contextSource.getBoundingClientRect();
      const x = contextPosition?.x ?? source.right, y = contextPosition?.y ?? source.bottom;
      actionMenu.style.width = `${Math.min(180, view.width - padding * 2)}px`;
      const rect = actionMenu.getBoundingClientRect();
      actionMenu.style.left = `${Math.max(view.left + padding, Math.min(x, view.left + view.width - rect.width - padding))}px`;
      actionMenu.style.top = `${Math.max(view.top + padding, Math.min(y, view.top + view.height - rect.height - padding))}px`;
    }
  }
  function showContext(button, item, expected, position = null) {
    if (!opened || !owns(expected) || !canMutate(context) || typeof closeSideThread !== "function") return;
    hideContext(); contextSource = button; contextItem = item; contextPosition = position;
    button.setAttribute("aria-expanded", "true");
    actionMenu.hidden = false; closeAction.disabled = false; fit(); closeAction.focus();
  }
  function bindSideThread(button, item, expected) {
    button.setAttribute("aria-haspopup", "menu"); button.setAttribute("aria-expanded", "false");
    button.addEventListener("pointerdown", event => {
      if (event.isPrimary === false || event.button !== 0 || !canMutate(syncContext())) return;
      cancelPress(); blockedContext = null;
      const current = press = { button, pointerId: event.pointerId, x: event.clientX, y: event.clientY,
        opened: false, timer: null };
      current.timer = setTimeout(() => {
        if (press !== current || !opened || !button.isConnected) return;
        current.opened = true;
        suppressedClick = { button, until: Date.now() + 2000 };
        showContext(button, item, expected, { x: current.x, y: current.y });
      }, 500);
    });
    button.addEventListener("contextmenu", event => {
      event.preventDefault(); event.stopPropagation();
      if (blockedContext?.button === button && Date.now() < blockedContext.until) return;
      if (press && !press.opened) {
        clearTimeout(press.timer);
        suppressedClick = { button, until: Date.now() + 2000 };
      }
      cancelPress();
      showContext(button, item, expected, event.clientX || event.clientY
        ? { x: event.clientX, y: event.clientY } : null);
    });
    button.addEventListener("keydown", event => {
      if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
      event.preventDefault(); event.stopPropagation(); cancelPress(); showContext(button, item, expected);
    });
  }
  function focusEdge(last = false) {
    const buttons = [...menu.querySelectorAll("button:not(:disabled)")];
    (last ? buttons.at(-1) : buttons[0] || menu)?.focus();
  }
  function refresh({ background = false } = {}) {
    if (disposed) return Promise.resolve(null);
    const value = syncContext();
    if (!opened && !background) return Promise.resolve(null);
    const entry = entryFor(value);
    restore(entry);
    updatePermissions();
    if (!entry || !value.connected) {
      message = ""; if (opened) render();
      return Promise.resolve(cached?.data || null);
    }
    if (entry.request) return entry.request.promise;
    if (entry.snapshot && !entry.dirty && Date.now() - entry.snapshot.loadedAtMs < refreshAgeMs)
      return Promise.resolve(entry.snapshot.data);
    const version = entry.revision, job = { promise: null };
    entry.dirty = false; entry.again = false;
    message = "";
    if (opened && !cached) render();
    job.promise = Promise.resolve().then(() => accepts(entry, version) ? load(value.threadId) : null).then(result => {
      if (!accepts(entry, version)) return null;
      const data = menuResponse(result, value.threadId);
      applySnapshot(entry, { data, loadedAtMs: Date.now() }, true);
      return data;
    }).catch(error => {
      if (accepts(entry, version)) {
        entry.dirty = true;
        if (owns(value.owner)) {
          message = cached ? "" : "相关会话读取失败，请重新打开菜单";
          if (opened && !cached) render();
          if (!background && !cached) report(error);
        }
      }
      return null;
    }).finally(() => {
      if (entry.request !== job) return;
      entry.request = null;
      if (accepts(entry) && entry.again && owns(value.owner) && context.connected)
        void refresh({ background: true });
    });
    entry.request = job;
    return job.promise;
  }
  function open(options = {}) {
    if (disposed || trigger.disabled || !trigger.isConnected) return Promise.resolve(null);
    if (opened) return refresh();
    try { beforeOpen?.(); } catch (error) { report(error); return Promise.resolve(null); }
    const value = syncContext(); restore(entryFor(value)); opened = true; layer.hidden = false;
    (trigger.closest("dialog") || document.body).append(layer);
    render();
    if (layer.hasAttribute("popover")) {
      try { layer.showPopover(); } catch { layer.removeAttribute("popover"); }
    }
    trigger.setAttribute("aria-expanded", "true");
    fit(); focusEdge(options?.edge === "last");
    if (!opened) return Promise.resolve(null);
    ticker = setInterval(() => {
      syncContext();
      if (opened) { updatePermissions(); updateTimes(); }
    }, 1000);
    return refresh();
  }
  function toggle() {
    if (opened) { close(true); return Promise.resolve(null); }
    return open();
  }
  listen(trigger, "click", toggle);
  listen(trigger, "keydown", event => {
    if (!["ArrowDown", "ArrowUp"].includes(event.key) || event.isComposing) return;
    event.preventDefault(); void open({ edge: event.key === "ArrowUp" ? "last" : "first" });
  });
  listen(layer, "pointerdown", event => {
    if (event.target !== layer) return;
    event.preventDefault(); event.stopPropagation();
  });
  listen(layer, "click", event => {
    if (event.target !== layer) return;
    event.preventDefault(); event.stopPropagation(); close(true);
  });
  listen(layer, "contextmenu", event => { if (event.target === layer) event.preventDefault(); });
  listen(closeAction, "click", event => {
    event.preventDefault(); event.stopPropagation();
    const item = contextItem, expected = owner;
    if (!item || closeAction.disabled) return;
    invoke(closeSideThread, [item], expected, true);
  });
  listen(window, "keydown", event => {
    if (!opened || event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault(); event.stopImmediatePropagation();
      if (!actionMenu.hidden) hideContext(true); else close(true);
      return;
    }
    if (!layer.contains(event.target) && event.target !== trigger) return;
    if (event.key === "Tab") { close(); return; }
    const currentMenu = actionMenu.hidden ? menu : actionMenu;
    const buttons = [...currentMenu.querySelectorAll("button:not(:disabled)")];
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      const index = buttons.indexOf(document.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
        : index < 0 ? (event.key === "ArrowDown" ? 0 : buttons.length - 1)
        : (index + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length;
      buttons[next]?.focus(); buttons[next]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter" && buttons.includes(document.activeElement)) {
      event.preventDefault(); event.stopPropagation(); document.activeElement.click();
    }
  }, true);
  listen(window, "pointerdown", event => {
    if (!press && event.isPrimary !== false) { suppressedClick = null; blockedContext = null; }
    if (press && event.pointerId !== press.pointerId) cancelPress({ suppress: true, blockContext: true });
    if (opened && !actionMenu.hidden && !actionMenu.contains(event.target)) hideContext();
  }, true);
  listen(window, "pointermove", event => {
    if (press && event.pointerId === press.pointerId && Math.hypot(event.clientX - press.x, event.clientY - press.y) > 10) {
      cancelPress({ suppress: true, blockContext: true });
    }
  }, true);
  const finishPress = event => {
    if (!press || event.pointerId !== press.pointerId) return;
    if (press.opened) { event.preventDefault(); event.stopImmediatePropagation(); }
    cancelPress({ suppress: press.opened || event.type === "pointercancel" });
  };
  listen(window, "pointerup", finishPress, true);
  listen(window, "pointercancel", finishPress, true);
  listen(window, "click", event => {
    if (!suppressedClick) return;
    const suppressed = suppressedClick;
    if (Date.now() > suppressed.until) { suppressedClick = null; return; }
    if (suppressed.button.contains(event.target)) {
      suppressedClick = null; event.preventDefault(); event.stopImmediatePropagation();
    }
  }, true);
  listen(window, "scroll", event => {
    cancelPress({ suppress: true, blockContext: true });
    if (opened && !layer.contains(event.target)) fit();
  }, true);
  listen(window, "resize", fit);
  if (window.visualViewport) {
    listen(window.visualViewport, "resize", fit); listen(window.visualViewport, "scroll", fit);
  }
  listen(window, "blur", () => cancelPress({ suppress: true, blockContext: true }));
  listen(document, "visibilitychange", () => { if (document.hidden) close(); });
  syncContext();
  return { toggle, open, close, refresh, isOpen: () => opened,
    updateContext() {
      const value = syncContext(), entry = entryFor(value);
      restore(entry);
      if (entry && value.connected && (!entry.snapshot || entry.dirty
          || Date.now() - entry.snapshot.loadedAtMs >= refreshAgeMs))
        void refresh({ background: true });
    },
    invalidate(threadId = null, parentThreadId = null) {
      const value = syncContext();
      for (const entry of entries.values()) {
        const data = entry.snapshot?.data;
        if (threadId && entry.context.threadId !== threadId && entry.context.threadId !== parentThreadId
            && !data?.subagents.some(item => item.id === threadId)
            && !data?.sideThreads.some(item => item.id === threadId)) continue;
        entry.dirty = true;
        if (entry.request) entry.again = true;
      }
      return entries.get(value.owner)?.dirty === true;
    },
    updateSideThread(parentThreadId, item, { remove = false } = {}) {
      for (const entry of entries.values()) {
        if (entry.context.threadId !== parentThreadId || !entry.snapshot || !accepts(entry)) continue;
        ++entry.revision; entry.request = null; entry.dirty = true;
        const sideThreads = entry.snapshot.data.sideThreads.filter(value => value.id !== item.id);
        if (!remove) sideThreads.push(item);
        applySnapshot(entry, { data: { ...entry.snapshot.data, sideThreads }, loadedAtMs: entry.snapshot.loadedAtMs }, true);
      }
    },
    markSideThreadRead(threadId) {
      for (const entry of entries.values()) {
        if (!accepts(entry) || !entry.snapshot?.data.sideThreads.some(item => item.id === threadId && item.hasUnreadTurn === true)) continue;
        ++entry.revision; entry.request = null; entry.dirty = true;
        applySnapshot(entry, { ...entry.snapshot, data: { ...entry.snapshot.data,
          sideThreads: entry.snapshot.data.sideThreads.map(item =>
            item.id === threadId ? { ...item, hasUnreadTurn: false } : item) } }, true);
      }
    },
    removeThread(threadId) {
      for (const [key, entry] of entries) {
        if (entry.context.threadId !== threadId) continue;
        entries.delete(key);
        try { Promise.resolve(cache?.remove?.(entry.context.cacheScope, threadId)).catch(() => {}); } catch {}
      }
      for (const entry of entries.values())
        if (entry.snapshot?.data.sideThreads.some(item => item.id === threadId))
          this.updateSideThread(entry.context.threadId, { id: threadId }, { remove: true });
    },
    dispose() {
      if (disposed) return;
      close(); disposed = true; entries.clear(); cached = null; suppressedClick = null; blockedContext = null;
      for (const remove of listeners) remove();
      for (const [name, value] of originalAria) {
        if (value == null) trigger.removeAttribute(name); else trigger.setAttribute(name, value);
      }
      times.length = 0; mutationButtons.clear(); layer.remove();
    }
  };
}
