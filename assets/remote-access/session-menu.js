let menuSequence = 0;

export function menuHorizontalLeft(anchor, menuWidth, viewport, padding = 8) {
  // 用户要求按钮菜单按可视区域中线选择方向：按钮中心在左半边时左对齐，否则右对齐（中线归右）。
  // 可视区域包含缩放/平移后的 left；两种方向均保留原来的 8px 边缘留白，不改变菜单宽度或上下定位。
  const alignLeft = (anchor.left + anchor.right) / 2 < viewport.left + viewport.width / 2;
  const preferred = alignLeft ? anchor.left : anchor.right - menuWidth;
  return Math.max(viewport.left + padding,
    Math.min(preferred, viewport.left + viewport.width - menuWidth - padding));
}

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
      || value.completedSubagents != null && (!Array.isArray(value.completedSubagents)
        || !value.completedSubagents.every(validItem)
        || value.completedSubagents.length !== value.completedSubagentCount)
      || !Array.isArray(value.sideThreads) || !value.sideThreads.every(validItem)
      || value.subagentsComplete != null && typeof value.subagentsComplete !== "boolean"
      || value.subagentsLoading != null && typeof value.subagentsLoading !== "boolean"
      || value.subagentsError != null && typeof value.subagentsError !== "string"
      || !Number.isInteger(value.completedSubagentCount) || value.completedSubagentCount < 0) {
    throw new Error("会话菜单数据格式无效，请重试。");
  }
  return { ...value, subagents: [...value.subagents],
    completedSubagents: [...(value.completedSubagents || [])], sideThreads: [...value.sideThreads] };
}
function menuSignature(data) {
  return JSON.stringify({ ...data,
    subagents: data.subagents.map(({ elapsedMs, ...item }) => item) });
}

export function mountSessionMenu({
  trigger, getContext, load, openChanges, openBrowser, openFiles, openSubagent, createSideThread, openSideThread,
  closeSideThread, beforeOpen, onError, onData, cache, renderSubagentIcon
}) {
  let disposed = false, opened = false, owner = "", authorization = "", context = null, cached = null;
  let suspended = false;
  let ticker = null, press = null, suppressedClick = null, blockedContext = null;
  let positionFrame = null, positionSignature = "";
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
  trigger.setAttribute("aria-haspopup", "menu"); trigger.setAttribute("aria-expanded", "false");
  trigger.setAttribute("aria-controls", id);
  const desktop = () => !globalThis.matchMedia?.("(max-width: 760px)").matches;
  // 用户要求手机和 PC 的菜单功能点击后均保持展开；手机仅允许点普通页面隐藏，侧边会话页面除外。
  const pinned = () => true;

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
      relatedEnabled: value.relatedEnabled !== false,
      subagentsEnabled: value.subagentsEnabled ?? value.relatedEnabled !== false,
      sideThreadsEnabled: value.sideThreadsEnabled ?? value.relatedEnabled !== false,
      sideThreadView: value.sideThreadView === true,
      serviceMode: value.serviceMode === true,
      createPending: value.createPending === true,
      authorization: JSON.stringify([deviceId, cacheScope]),
      connected: value.connected === true, owner: JSON.stringify([deviceId, cacheScope, threadId,
        value.subagentsEnabled ?? value.relatedEnabled !== false,
        value.sideThreadsEnabled ?? value.relatedEnabled !== false, value.relatedScope || ""]) };
  }
  function syncContext() {
    if (suspended) return context;
    const next = readContext();
    const changed = next.owner !== owner;
    const authorizationChanged = next.authorization !== authorization;
    if (authorizationChanged) {
      authorization = next.authorization;
      entries.clear();
    }
    if (next.owner !== owner) {
      owner = next.owner; cached = entries.get(owner)?.snapshot || null; message = "";
      times.length = 0; mutationButtons.clear();
      if (opened) close(false, authorizationChanged);
      menu.replaceChildren();
    }
    context = next;
    if (changed && opened && pinned()) render();
    if (changed && cached) onData?.(cached.data);
    return next;
  }
  const canRead = value => !!(value?.deviceId && value.cacheScope && value.threadId
    && (value.subagentsEnabled || value.sideThreadsEnabled));
  const canMutate = value => canRead(value) && value.sideThreadsEnabled && value.connected && !value.readOnly;
  function entryFor(value = syncContext()) {
    if (!canRead(value)) return null;
    let entry = entries.get(value.owner);
    if (!entry) entry = { context: value, snapshot: null, revision: 0, subagentsExpanded: true,
      completedSubagentsExpanded: false, dirty: true, request: null, restoring: null, restored: false, again: false };
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
    const data = entry.snapshot.data;
    const completedSubagents = data.completedSubagents.filter(item => item.ephemeral === false);
    try {
      Promise.resolve(cache?.put?.(entry.context.cacheScope, entry.context.threadId,
        { version: 3, ...entry.snapshot,
          data: { ...data, subagents: data.subagents.filter(item => item.ephemeral === false),
            completedSubagents, completedSubagentCount: completedSubagents.length, sideThreads: [],
            subagentsComplete: false, subagentsLoading: false, subagentsError: null, subagentsStale: true } })).catch(() => {});
    } catch {}
  }
  function applySnapshot(entry, snapshot, persist = false) {
    const previous = entry.snapshot;
    if (snapshot.data.subagentsError && previous
        && !snapshot.data.subagents.length && !snapshot.data.completedSubagentCount) {
      snapshot = { ...snapshot, data: { ...snapshot.data,
        subagents: previous.data.subagents, completedSubagents: previous.data.completedSubagents,
        completedSubagentCount: previous.data.completedSubagentCount, subagentsStale: true } };
    }
    const changed = !previous || menuSignature(previous.data) !== menuSignature(snapshot.data);
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
        if (!accepts(entry, revision) || entry.snapshot || saved?.version !== 3
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
  function close(restoreFocus = false, force = false) {
    // 两端普通工具操作、切换会话和返回不自动关闭；图标、手机普通页面点击及生命周期清理显式关闭。
    if (!force && pinned()) return;
    stopPositionTracking();
    if (!opened) { cancelPress(); return; }
    opened = false; suspended = false; clearInterval(ticker); ticker = null; cancelPress(); hideContext();
    trigger.setAttribute("aria-expanded", "false");
    if (layer.hasAttribute("popover")) {
      try { layer.hidePopover(); } catch {}
    }
    layer.classList.remove("session-related-menu-suspended");
    layer.hidden = true;
    if (restoreFocus && trigger.isConnected && !trigger.disabled) trigger.focus();
  }
  function dismissForPageClick(event) {
    // 只处理手机菜单外的真实点击，不拦截页面操作；侧边会话页面仍保持菜单展开。
    if (!opened || suspended || desktop() || readContext().sideThreadView
        || layer.contains(event.target) || trigger.contains(event.target)) return;
    close(false, true);
  }
  function suspend() {
    if (!opened || suspended) return;
    stopPositionTracking();
    suspended = true; cancelPress(); hideContext();
    trigger.setAttribute("aria-expanded", "false");
    if (layer.hasAttribute("popover")) {
      try { layer.hidePopover(); } catch {}
    }
    layer.classList.add("session-related-menu-suspended");
    layer.hidden = true;
  }
  function resume() {
    if (!opened || !suspended) return;
    suspended = false;
    layer.classList.remove("session-related-menu-suspended");
    layer.hidden = false;
    (trigger.closest("dialog") || document.body).append(layer);
    if (layer.hasAttribute("popover")) {
      try { layer.showPopover(); } catch { layer.removeAttribute("popover"); }
    }
    trigger.setAttribute("aria-expanded", "true");
    startPositionTracking();
    (window.requestAnimationFrame || (cb => setTimeout(cb, 16)))(() => {
      if (opened && !suspended) {
        fit();
        if (opened && !suspended) focusEdge(false);
      }
    });
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
      const text = item.status === "unknown" ? "状态暂不可用"
        : item.status === "waiting" ? `等待 · ${duration(elapsed)}` : duration(elapsed);
      if (node.textContent !== text) node.textContent = text;
    }
  }
  function invoke(callback, args, expected, mutation = false, onSuccess, { keepMenu = false } = {}) {
    if (!owns(expected) || mutation && !canMutate(context) || typeof callback !== "function") return;
    if (keepMenu && !pinned()) suspend(); else close();
    try {
      Promise.resolve(callback(...args)).then(() => {
        if (onSuccess && owns(expected)) onSuccess();
      }).catch(report);
    } catch (error) { report(error); }
  }
  function itemButton(label, callback, args, kind = "", item = null, { keepMenu = false } = {}) {
    const node = element("button", "session-related-menu-item");
    node.type = "button"; node.tabIndex = -1; node.setAttribute("role", "menuitem");
    node.append(element("span", "session-related-menu-name", label));
    node.disabled = typeof callback !== "function";
    const expected = owner;
    node.addEventListener("click", event => {
      event.preventDefault(); event.stopPropagation();
      if (node.disabled) return;
      if (kind === "create" && (context.createPending || cached?.data.canCreateSideThread === false)) return;
      invoke(callback, args, expected, !!kind, undefined, { keepMenu });
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
  function menuButtons(root = menu) {
    return [...root.querySelectorAll("button:not(:disabled)")].filter(node => !node.closest("[hidden]"));
  }
  function render() {
    const previousKey = menu.contains(document.activeElement) ? document.activeElement.dataset.menuKey : null;
    // 手机转为 PC 布局时，浏览器入口会移除；原焦点回到相邻的文件入口，避免键盘焦点落到页面外。
    const focusedKey = previousKey === "browser" && desktop() ? "files" : previousKey;
    cancelPress(); hideContext(); times.length = 0; mutationButtons.clear(); menu.replaceChildren();
    const tools = element("div", "session-related-menu-group");
    menu.append(tools);
    layer.classList.toggle("session-menu-pinned", pinned());
    const changes = itemButton("变更", openChanges, []);
    changes.dataset.menuKey = "changes"; tools.append(changes);
    const files = itemButton("文件", openFiles, []);
    files.dataset.menuKey = "files"; tools.append(files);
    // 用户要求 PC 布局（宽于 760px）不显示浏览器；手机保留，跨断点缩放时由现有 resize 重渲染同步。
    if (!desktop()) {
      const browser = itemButton("浏览器", openBrowser, []);
      browser.dataset.menuKey = "browser"; tools.append(browser);
    }
    // 旧后台未声明独立能力时继续置灰；新后台的子智能体和侧边会话分别受控，不展示禁用能力的旧缓存。
    if (context?.subagentsEnabled === false && context?.sideThreadsEnabled === false) {
      const subagents = section("子智能体");
      subagents.classList.add("session-related-menu-disabled");
      subagents.setAttribute("aria-disabled", "true");
      const create = itemButton("新建侧边会话", null, []);
      create.dataset.menuKey = "create";
      create.classList.add("session-related-menu-heading-action");
      create.setAttribute("aria-label", "新建侧边会话");
      create.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
      const sides = section("侧边会话", create);
      sides.classList.add("session-related-menu-disabled");
      sides.setAttribute("aria-disabled", "true");
      if (focusedKey) menuButtons().find(node => node.dataset.menuKey === focusedKey)?.focus();
      if (opened) fit();
      return;
    }
    const data = cached?.data, entry = entries.get(owner), expected = owner;
    if (context?.subagentsEnabled !== false) {
    const toggleSubagents = element("button", "session-related-menu-item session-related-menu-heading-action session-related-menu-heading-toggle");
    toggleSubagents.type = "button"; toggleSubagents.tabIndex = -1;
    toggleSubagents.setAttribute("role", "menuitem");
    toggleSubagents.dataset.menuKey = "toggle-subagents";
    toggleSubagents.disabled = !entry;
    toggleSubagents.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 8.5 7 7 7-7"/></svg>';
    const subagents = section("子智能体", toggleSubagents);
    const subagentHeader = subagents.querySelector(".session-related-menu-group-header");
    subagentHeader.classList.add("session-related-menu-collapsible-header");
    const subagentContents = element("div", "session-related-menu-subagents");
    subagentContents.id = `${id}-subagents`;
    toggleSubagents.setAttribute("aria-controls", subagentContents.id);
    const updateExpanded = () => {
      const expanded = entry?.subagentsExpanded !== false;
      subagentContents.hidden = !expanded;
      toggleSubagents.setAttribute("aria-expanded", String(expanded));
      const label = `${expanded ? "收起" : "展开"}子智能体`;
      toggleSubagents.setAttribute("aria-label", label); toggleSubagents.title = label;
      if (!expanded && subagentContents.contains(document.activeElement)) toggleSubagents.focus();
    };
    subagentHeader.addEventListener("click", event => {
      event.preventDefault(); event.stopPropagation();
      if (!entry || !owns(expected)) return;
      entry.subagentsExpanded = !entry.subagentsExpanded;
      updateExpanded(); fit();
    });
    updateExpanded(); subagents.append(subagentContents);
    for (const item of data?.subagents || []) {
      const button = itemButton(nonempty(item.name) ? item.name : "子智能体", openSubagent, [item]);
      button.classList.add("session-related-menu-thread");
      button.dataset.menuKey = `subagent:${item.id}`;
      const time = element("span", "session-related-menu-time");
      time.setAttribute("title", "执行时间");
      button.append(time); subagentContents.append(button);
      times.push({ node: time, item, loadedAtMs: cached.loadedAtMs });
    }
    if (data?.completedSubagentCount > 0) {
      const completed = element("button", "session-related-menu-item session-related-menu-completed");
      completed.type = "button"; completed.tabIndex = -1; completed.setAttribute("role", "menuitem");
      completed.dataset.menuKey = "toggle-completed-subagents";
      completed.setAttribute("aria-label", `${entry?.completedSubagentsExpanded ? "收起" : "展开"}已完成子智能体`);
      completed.setAttribute("aria-expanded", String(entry?.completedSubagentsExpanded === true));
      const icons = element("span", "session-related-menu-completed-icons");
      icons.setAttribute("aria-hidden", "true");
      for (const item of data.completedSubagents.slice(0, 4)) {
        const icon = renderSubagentIcon?.(item);
        if (icon) {
          icon.classList.add("session-related-menu-agent-icon");
          icons.append(icon);
        }
      }
      icons.hidden = !icons.childElementCount;
      completed.append(icons, element("span", "session-related-menu-completed-count",
        `${data.completedSubagentCount} 完成`));
      subagentContents.append(completed);
      completed.onclick = event => {
        event.preventDefault(); event.stopPropagation();
        if (!entry || !owns(expected)) return;
        entry.completedSubagentsExpanded = !entry.completedSubagentsExpanded;
        render();
      };
      if (entry?.completedSubagentsExpanded) for (const item of data.completedSubagents) {
        const button = itemButton(nonempty(item.name) ? item.name : "子智能体", openSubagent, [item]);
        button.classList.add("session-related-menu-thread");
        button.dataset.menuKey = `subagent:${item.id}`;
        button.append(element("span", "session-related-menu-time",
          item.outcome === "failed" ? "失败" : ["interrupted", "cancelled", "canceled"].includes(item.outcome) ? "已停止" : "已完成"));
        subagentContents.append(button);
      }
    }
    if (data?.subagentsError || data?.subagentsStale || data?.subagentsLoading || data?.subagentsComplete === false) {
      const status = element("div", "session-related-menu-status",
        data.subagentsError ? `子智能体读取失败${data.subagentsStale ? "，当前显示上次结果" : ""}：${data.subagentsError}`
          : data.subagentsStale ? "当前显示上次子智能体记录，等待同步…"
          : data.subagentsLoading ? "正在读取子智能体…" : "子智能体列表尚未完整，正在同步…");
      status.setAttribute("role", "status");
      subagentContents.append(status);
    }
    } else {
      const disabled = section("子智能体");
      disabled.classList.add("session-related-menu-disabled");
      disabled.setAttribute("aria-disabled", "true");
    }
    const create = itemButton("新建侧边会话", createSideThread, [], "create", null, { keepMenu: true });
    create.dataset.menuKey = "create";
    create.classList.add("session-related-menu-heading-action");
    create.setAttribute("aria-label", "新建侧边会话"); create.title = "新建侧边会话";
    if (desktop()) {
      // PC 新建侧边会话沿用 Codex 快捷键；手机不新增键盘提示，按钮与快捷键共用创建权限。
      const mac = /Mac/i.test(globalThis.navigator?.userAgentData?.platform || globalThis.navigator?.platform || "");
      create.title += mac ? "（⌘⌥S）" : "（Ctrl+Alt+S）";
      create.setAttribute("aria-keyshortcuts", mac ? "Meta+Alt+S" : "Control+Alt+S");
    }
    create.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
    const sides = section("侧边会话", create);
    // 只展示当前运行方式明确支持的侧边会话；旧后台或切换模式后的旧缓存不能开放写操作。
    if (context?.sideThreadsEnabled === false) {
      sides.classList.add("session-related-menu-disabled");
      sides.setAttribute("aria-disabled", "true");
    }
    for (const item of context?.sideThreadsEnabled === false ? [] : data?.sideThreads || []) {
      const button = itemButton(nonempty(item.title) ? item.title : "新建侧边会话", openSideThread, [item], "", item, { keepMenu: true });
      button.classList.add("session-related-menu-thread");
      if (item.running === true) {
        const running = element("span", "spinner session-related-menu-running");
        running.setAttribute("role", "img"); running.setAttribute("aria-label", "正在执行");
        button.append(running);
      } else if (item.hasUnreadTurn === true) {
        const unread = element("span", "session-related-menu-unread");
        unread.setAttribute("role", "img"); unread.setAttribute("aria-label", "有未读消息");
        button.append(unread);
      }
      button.dataset.menuKey = `side:${item.id}`; sides.append(button);
    }
    let status = message;
    // 用户要求新建会话时不显示选会话提示；已有会话仍保留授权异常说明。
    if (!canRead(context)) status = context?.threadId ? "当前授权不可用" : "";
    else if (!context.connected) status = data ? "当前离线，显示上次记录" : "当前离线，连接后读取相关会话";
    if (status) {
      const note = element("p", "session-related-menu-status", status);
      note.setAttribute("role", "status"); menu.append(note);
    }
    updatePermissions(); updateTimes();
    if (focusedKey) {
      menuButtons().find(node => node.dataset.menuKey === focusedKey)?.focus();
    }
    if (opened) fit();
  }
  function stopPositionTracking() {
    if (positionFrame != null) {
      if (window.cancelAnimationFrame) window.cancelAnimationFrame(positionFrame);
      else clearTimeout(positionFrame);
    }
    positionFrame = null; positionSignature = "";
  }
  function anchorPositionSignature(box, view) {
    return [box.left, box.top, box.right, box.bottom, box.width, box.height,
      view.left, view.top, view.width, view.height].join(",");
  }
  function startPositionTracking() {
    if (positionFrame != null || disposed || !opened || suspended || document.hidden) return;
    // 用户要求会话菜单跟随触发按钮：侧栏和右侧面板移动、关联页动画不一定产生窗口 resize。
    // 只在菜单可见时比较锚点坐标；位置不变不写样式，关闭、挂起和后台页面均取消逐帧跟踪。
    const requestFrame = callback => window.requestAnimationFrame
      ? window.requestAnimationFrame(callback) : setTimeout(callback, 16);
    const follow = () => {
      positionFrame = null;
      if (disposed || !opened || suspended || document.hidden) return;
      if (!trigger.isConnected || trigger.disabled || trigger.hidden
          || anchorPositionSignature(trigger.getBoundingClientRect(), viewport()) !== positionSignature) fit();
      if (!disposed && opened && !suspended && !document.hidden) positionFrame = requestFrame(follow);
    };
    positionFrame = requestFrame(follow);
  }
  function viewport() {
    const view = window.visualViewport;
    return { top: view?.offsetTop || 0, left: view?.offsetLeft || 0,
      width: view?.width || window.innerWidth, height: view?.height || window.innerHeight };
  }
  function fit() {
    if (!opened || suspended) return;
    if (!trigger.isConnected || trigger.disabled || trigger.hidden) { close(false, true); return; }
    const view = viewport(), box = trigger.getBoundingClientRect(), padding = 8;
    if (box.width === 0 && box.height === 0) {
      close(false, true); return;
    }
    if (box.bottom <= view.top || box.top >= view.top + view.height
        || box.right <= view.left || box.left >= view.left + view.width) { close(false, true); return; }
    positionSignature = anchorPositionSignature(box, view);
    // PC 沿用 300px 宽度，不能因窗口高度低于 600px 而改用手机比例；
    // 手机短边低于 600px 时仍取视口宽度的 65%，两侧至少保留 8px。
    const phoneViewport = !desktop() && Math.min(view.width, view.height) < 600;
    const width = phoneViewport
      ? Math.max(0, Math.min(view.width * .65, view.width - padding * 2))
      : Math.max(0, Math.min(300, view.width - padding * 2));
    // 用户要求弹层与图标统一间隔 10px；PC 和手机共用，视口边缘仍保留 8px 安全留白。
    const anchorGap = 10;
    const top = Math.max(view.top + padding, Math.min(box.bottom + anchorGap, view.top + view.height - padding));
    menu.style.width = `${width}px`;
    menu.style.left = `${menuHorizontalLeft(box, width, view, padding)}px`;
    menu.style.top = `${top}px`;
    menu.style.maxHeight = `${Math.max(0, Math.min(480, view.height * .8, view.top + view.height - top - padding))}px`;
    if (!actionMenu.hidden) {
      const source = contextSource.getBoundingClientRect();
      const x = contextPosition ? source.left + contextPosition.x : source.right;
      const y = contextPosition ? source.top + contextPosition.y : source.bottom;
      actionMenu.style.width = `${Math.min(180, view.width - padding * 2)}px`;
      const rect = actionMenu.getBoundingClientRect();
      // 右键/长按继续以实际点击坐标定位；键盘打开的按钮菜单沿用统一的左右展开规则。
      actionMenu.style.left = `${contextPosition
        ? Math.max(view.left + padding, Math.min(x, view.left + view.width - rect.width - padding))
        : menuHorizontalLeft(source, rect.width, view, padding)}px`;
      actionMenu.style.top = `${Math.max(view.top + padding, Math.min(y, view.top + view.height - rect.height - padding))}px`;
    }
  }
  function showContext(button, item, expected, position = null) {
    if (!opened || !owns(expected) || !canMutate(context) || typeof closeSideThread !== "function") return;
    hideContext(); contextSource = button; contextItem = item;
    const box = button.getBoundingClientRect();
    // 操作子菜单也按行内偏移跟随，不能在主菜单移动后停留于旧的屏幕坐标。
    contextPosition = position ? { x: position.x - box.left, y: position.y - box.top } : null;
    button.setAttribute("aria-expanded", "true");
    actionMenu.hidden = false; closeAction.disabled = false; fit();
    if (opened && !actionMenu.hidden) closeAction.focus();
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
    const buttons = menuButtons();
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
    // 服务模式每秒检查一次，快照最多复用 2 秒；与正文的 2 秒补读一起覆盖桌面持有会话的状态变化。
    if (entry.snapshot && !entry.dirty && Date.now() - entry.snapshot.loadedAtMs < (value.serviceMode ? 2000 : refreshAgeMs))
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
          if (value.serviceMode && cached) {
            // 子智能体查询失败时保留可查看的旧结果，但必须标明未同步，不能继续显示为完整的最新列表。
            applySnapshot(entry, { ...cached, data: { ...cached.data,
              subagentsComplete: false, subagentsLoading: false, subagentsStale: true,
              subagentsError: error?.message || "暂时无法读取，请稍后重试" } });
          } else {
            message = cached ? "" : "相关会话读取失败，请重新打开菜单";
            if (opened && !cached) render();
          }
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
    if (suspended) { resume(); return Promise.resolve(cached?.data || null); }
    if (opened) return refresh();
    try { beforeOpen?.(); } catch (error) { report(error); return Promise.resolve(null); }
    const value = syncContext(); restore(entryFor(value)); opened = true; layer.hidden = false;
    (trigger.closest("dialog") || document.body).append(layer);
    render();
    if (!opened) return Promise.resolve(null);
    if (layer.hasAttribute("popover")) {
      try { layer.showPopover(); } catch { layer.removeAttribute("popover"); }
    }
    trigger.setAttribute("aria-expanded", "true");
    fit(); if (opened && options?.focus !== false) focusEdge(options?.edge === "last");
    if (!opened) return Promise.resolve(null);
    startPositionTracking();
    ticker = setInterval(() => {
      syncContext();
      if (opened) {
        updatePermissions(); updateTimes();
        if (context?.serviceMode && context.connected && !document.hidden && !suspended)
          void refresh({ background: true });
      }
    }, 1000);
    return refresh();
  }
  function toggle() {
    if (opened) {
      close(true, true); return Promise.resolve(null);
    }
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
    if (trigger.closest?.(".related-session-view")) return;
    if (event.key === "Escape") {
      if (pinned() && !layer.contains(event.target) && event.target !== trigger) return;
      event.preventDefault(); event.stopImmediatePropagation();
      if (!actionMenu.hidden) hideContext(true); else close(true);
      return;
    }
    if (!layer.contains(event.target) && event.target !== trigger) return;
    if (event.key === "Tab") { close(); return; }
    const currentMenu = actionMenu.hidden ? menu : actionMenu;
    const buttons = menuButtons(currentMenu);
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
  listen(window, "click", dismissForPageClick, true);
  listen(window, "scroll", event => {
    cancelPress({ suppress: true, blockContext: true });
    if (opened && !layer.contains(event.target)) fit();
  }, true);
  listen(window, "resize", () => {
    // 已由图标关闭的菜单在窗口缩放后继续关闭，不能因 PC 固定显示规则自动重开。
    if (opened) render();
    fit();
  });
  if (window.visualViewport) {
    listen(window.visualViewport, "resize", fit); listen(window.visualViewport, "scroll", fit);
  }
  listen(window, "blur", () => cancelPress({ suppress: true, blockContext: true }));
  listen(document, "visibilitychange", () => {
    if (document.hidden) { stopPositionTracking(); close(); }
    else if (opened && !suspended) { fit(); startPositionTracking(); }
  });
  syncContext();
  return { toggle, open, close, suspend, resume, refresh, reposition: fit,
    isOpen: () => opened && !suspended, isSuspended: () => opened && suspended,
    canCreateSideThread() {
      return !disposed && canMutate(syncContext()) && cached?.data.canCreateSideThread !== false;
    },
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
            && !data?.completedSubagents.some(item => item.id === threadId)
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
      close(false, true); disposed = true; entries.clear(); cached = null; suppressedClick = null; blockedContext = null;
      for (const remove of listeners) remove();
      for (const [name, value] of originalAria) {
        if (value == null) trigger.removeAttribute(name); else trigger.setAttribute(name, value);
      }
      times.length = 0; mutationButtons.clear(); layer.remove();
    }
  };
}
