(() => {
  const root = typeof window === "object" ? window : {};
  const version = "8";
  if (root.__codexElvesComposerFast?.version === version) return;
  root.__codexElvesComposerFast?.configure?.({ enabled: false });
  const attribute = "data-codex-elves-fast";
  const statusAttribute = "data-codex-elves-fast-status";
  const modelAttribute = "data-codex-elves-fast-model";
  // 仅用于清理旧版列表闪电；用户要求会话列表不再增加 Fast 标识。
  const sidebarAttribute = "data-codex-elves-fast-session";
  const preferencePrefix = "codex-elves-fast-session-v1:";
  let adapters = null;
  let enabled = false;
  let dispatcher = null;
  let snapshot = null;
  let snapshotAt = 0;
  let loading = null;
  let refreshing = null;
  let writing = false;
  let revision = 0;
  let menuPlacement = null;
  let menuObserver = null;
  let menuRefreshFrame = null;
  const pending = new Map();
  const drafts = new Map();
  const boundDrafts = new WeakSet();

  function threadId(value) {
    const id = typeof value === "string" ? value.trim().replace(/^local:/, "") : "";
    return /^[A-Za-z0-9_.-]{8,128}$/.test(id)
      && !["__proto__", "prototype", "constructor"].includes(id) ? id : "";
  }

  function contextKey(context) {
    if (context?.hostId !== "local") return "";
    const id = threadId(context.threadId);
    return id ? `${preferencePrefix}local:${id}`
      : context.draftId ? `draft:${context.draftId}` : "";
  }

  function readPreference(context) {
    const key = contextKey(context);
    if (!key) return false;
    if (key.startsWith("draft:")) return drafts.get(key) === true;
    return root.localStorage?.getItem(key) === "true";
  }

  function writePreference(context, value) {
    const key = contextKey(context);
    if (!key) throw new Error("未识别到当前本机会话，Fast 设置未改变");
    if (key.startsWith("draft:")) {
      if (value === true) drafts.set(key, true);
      else drafts.delete(key);
      return;
    }
    if (!root.localStorage) throw new Error("会话设置存储不可用");
    if (value === true) root.localStorage.setItem(key, "true");
    else root.localStorage.removeItem(key);
    if (readPreference(context) !== (value === true)) throw new Error("Fast 会话设置保存失败");
  }

  function currentContext() {
    return adapters?.getContext?.() || null;
  }

  function configuredFast(context = currentContext()) {
    try { return readPreference(context); } catch { return false; }
  }

  function request(method, params) {
    return new Promise((resolve, reject) => {
      const id = `codex-elves-fast-${root.crypto.randomUUID()}`;
      const timer = root.setTimeout(() => {
        pending.delete(id);
        reject(new Error("Fast 配置读取超时"));
      }, 5000);
      pending.set(id, { resolve, reject, timer });
      try {
        // 使用公开的 app-server 消息形状；不导入压缩函数、不改写 Codex 安装文件。
        Promise.resolve(root.electronBridge.sendMessageFromView({
          type: "mcp-request", hostId: "local", request: { id, method, params },
          priority: "critical",
        })).catch((error) => finish(id, null, error));
      } catch (error) {
        finish(id, null, error);
      }
    });
  }

  function finish(id, result, error) {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    root.clearTimeout(entry.timer);
    if (error) entry.reject(new Error(error.message || String(error)));
    else entry.resolve(result);
  }

  function onMessage(event) {
    const data = event.data;
    if (data?.type !== "mcp-response" || (data.hostId && data.hostId !== "local")) return;
    const message = data.message || data.response;
    if (message?.id) finish(message.id, message.result, message.error);
  }

  async function readSnapshot(force = false) {
    if (!force && snapshot && Date.now() - snapshotAt < 10000) return snapshot;
    if (loading) return loading;
    loading = (async () => {
      const [account, requirements, configResponse, copilot] = await Promise.all([
        request("account/read", { refreshToken: false }),
        request("configRequirements/read"),
        request("config/read", { includeLayers: false, cwd: null }),
        adapters.isCopilotActive(),
      ]);
      const models = [];
      let cursor = null;
      const cursors = new Set();
      do {
        const page = await request("model/list", { includeHidden: true, cursor, limit: 100 });
        models.push(...(page?.data || []));
        cursor = page?.nextCursor || null;
        if (cursor && cursors.has(cursor)) throw new Error("模型列表分页异常");
        if (cursor) cursors.add(cursor);
      } while (cursor && models.length < 2000);
      const next = {
        account: account?.account?.type,
        requirements,
        config: {
          service_tier: configResponse?.config?.service_tier,
          features: { fast_mode: configResponse?.config?.features?.fast_mode },
        },
        models,
        copilot,
      };
      snapshot = next;
      snapshotAt = Date.now();
      return next;
    })().finally(() => { loading = null; });
    return loading;
  }

  function modelCapability(state, model) {
    if (!model || !adapters?.isGptModel(model)) return null;
    const entry = state.models.find((item) => item.model === model || item.id === model);
    return entry?.serviceTiers?.find((tier) => tier.id === "priority" || tier.id === "fast") || null;
  }

  function allowed(state) {
    // 独立入口仍遵守账号与管理员边界；未知身份或未知 requirements 不开放。
    return enabled && state?.copilot === false && ["apiKey", "apikey"].includes(state?.account)
      && state.requirements && Object.hasOwn(state.requirements, "requirements")
      && state.requirements.requirements?.featureRequirements?.fast_mode !== false
      && state.config.features?.fast_mode !== false;
  }

  function selectedModel() {
    return adapters?.getModel?.(snapshot?.models) || "";
  }

  async function patchMessage(type, payload, context = {}) {
    const params = payload?.request?.params;
    if (!enabled || type !== "mcp-request" || payload?.hostId !== "local"
      || payload.request?.method !== "turn/start" || !params) return payload;
    // 用户要求按会话设置 Fast：只按该请求的真实 threadId 查找，后台与排队发送不能读取当前页面。
    // 新建请求由原生 startConversation 显式传入草稿，禁止把全局 service_tier 当作会话默认值。
    const target = { hostId: "local", threadId: threadId(params.threadId) || threadId(context?.threadId),
      draftId: context?.draftId };
    const fast = target.threadId ? configuredFast(target)
      : contextKey(target) && context?.fast === true;
    const state = await readSnapshot(true);
    if (!allowed(state)) return payload;
    // 用户要求 Fast 不依赖原生入口：在最终发送边界补齐等级，覆盖普通发送和出队发送。
    // 只使用该请求自己的模型，禁止用当前输入框或全局默认模型替代其他会话的模型。
    const model = params.collaborationMode?.settings?.model || params.model;
    const tier = modelCapability(state, model);
    if (!tier) return payload;
    // flex/ultrafast 等其他等级仍由原生逻辑负责；显式开启本会话 Fast 时才选择 Fast。
    const selected = params.serviceTier ?? state.config.service_tier;
    if (!fast && selected != null && !["fast", "priority", "default"].includes(selected)) return payload;
    return { ...payload, request: { ...payload.request, params: {
      ...params, serviceTier: fast ? tier.id : "default",
    } } };
  }

  async function installDispatcher() {
    const found = await adapters.getDispatcher();
    if (!found || typeof found.dispatchMessage !== "function") throw new Error("未找到 Fast 请求通道");
    if (found.__codexElvesComposerFastPatch !== api) {
      const original = found.dispatchMessage;
      const patched = function (type, payload, ...rest) {
        if (!enabled || type !== "mcp-request" || payload?.hostId !== "local"
          || payload.request?.method !== "turn/start") {
          return original.call(this, type, payload, ...rest);
        }
        const receiver = this;
        return patchMessage(type, payload).catch((error) => {
          adapters?.notify?.(`${error.message}，本轮保留原生服务等级`);
          return payload;
        }).then((next) => original.call(receiver, type, next, ...rest));
      };
      found.dispatchMessage = patched;
      if (found.dispatchMessage !== patched) throw new Error("Fast 请求通道不可写");
      found.__codexElvesComposerFastPatch = api;
    }
    dispatcher = found;
  }

  function removeButton() {
    root.document?.querySelectorAll(`[${attribute}]`).forEach((button) => button.remove());
    if (menuPlacement?.restorePosition && menuPlacement.menu.style.position === "relative")
      menuPlacement.menu.style.position = menuPlacement.originalPosition;
    menuPlacement = null;
  }

  function removeIndicators() {
    root.document?.querySelectorAll(`[${statusAttribute}], [${sidebarAttribute}]`)
      .forEach((node) => node.remove());
    highlightModels();
  }

  function highlightModels(labels = []) {
    // 用户要求只在 Fast 开启时将模型名称标蓝；移除属性即可恢复原主题颜色，不改写原生行内样式。
    const selected = new Set(labels.filter((label) => label?.isConnected));
    root.document?.querySelectorAll(`[${modelAttribute}]`).forEach((node) => {
      if (!selected.has(node)) node.removeAttribute(modelAttribute);
    });
    for (const label of selected) {
      if (label.getAttribute(modelAttribute) !== "true") label.setAttribute(modelAttribute, "true");
    }
  }

  function renderIndicators(state, error) {
    // 用户要求去除会话列表闪电；同时清理旧文字标签，只通过模型名称标蓝提示当前会话状态。
    root.document?.querySelectorAll(`[${statusAttribute}], [${sidebarAttribute}]`)
      .forEach((node) => node.remove());
    const indicators = !error && state && allowed(state)
      ? adapters?.getIndicators?.(state.models) ?? [{
        context: currentContext(), model: selectedModel(), label: adapters?.getModelLabel?.(state.models),
      }] : [];
    // 用户要求侧边 Fast 与主会话互不影响；两个输入区同时可见时按各自会话设置更新名称颜色。
    highlightModels(indicators.filter(({ context, model }) =>
      contextKey(context) && modelCapability(state, model) && configuredFast(context)
    ).map(({ label }) => label));
  }

  function syncMenuObserver() {
    if (!enabled) {
      menuObserver?.disconnect();
      menuObserver = null;
      if (menuRefreshFrame != null) root.cancelAnimationFrame(menuRefreshFrame);
      menuRefreshFrame = null;
      return;
    }
    if (menuObserver || !root.MutationObserver || !root.document?.body) return;
    const selector = "[data-model-picker-view], [data-model-picker-power-slider]";
    menuObserver = new root.MutationObserver((mutations) => {
      const relevant = menuPlacement && !menuPlacement.menu.isConnected || mutations.some((mutation) =>
        mutation.target?.closest?.(selector)
        || [...mutation.addedNodes || [], ...mutation.removedNodes || []].some((node) =>
          node.matches?.(selector) || node.querySelector?.(selector)));
      if (!relevant || menuRefreshFrame != null) return;
      menuRefreshFrame = root.requestAnimationFrame(() => {
        menuRefreshFrame = null;
        void refresh();
      });
    });
    // 用户要求 Fast 跟随模型菜单：单独观察菜单开关和视图切换，不依赖原生 Fast 或输入区重绘。
    menuObserver.observe(root.document.body, {
      childList: true, subtree: true, attributes: true,
      attributeFilter: ["data-model-picker-view", "data-state", "aria-hidden"],
    });
  }

  function render(state, error = "") {
    if (!enabled) { removeButton(); removeIndicators(); return; }
    renderIndicators(state, error);
    const model = selectedModel();
    // 用户要求非 GPT 模型不显示 Fast；按实际模型及别名判断，未知模型也不显示灰色占位。
    if (!model || !adapters?.isGptModel(model)) { removeButton(); return; }
    const menu = adapters?.getMenu?.();
    if (!menu?.isConnected) { removeButton(); return; }
    if (menuPlacement?.menu !== menu) {
      removeButton();
      menuPlacement = { menu, originalPosition: menu.style.position,
        restorePosition: root.getComputedStyle(menu).position === "static" };
      if (menuPlacement.restorePosition) menu.style.position = "relative";
    }
    let button = root.document.querySelector(`[${attribute}]`);
    if (!button) {
      button = root.document.createElement("button");
      button.type = "button";
      button.setAttribute(attribute, "true");
      // 用户要求 Fast 固定在默认模型选择菜单左上角，不再跟随原生 Fast 或输入工具栏。
      // 28px 点击区、16px 图标和菜单内侧 8px 间距对应此紧凑菜单，不挤占中央模型标签和下方滑块。
      button.innerHTML = '<svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h8l-1 8 11-12h-8V2Z"/></svg>';
      button.style.cssText = "position:absolute;top:8px;left:8px;z-index:2;display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px;padding:0;margin:0;border:1px solid transparent;border-radius:999px;cursor:pointer";
      button.addEventListener("pointerdown", (event) => event.stopPropagation());
      button.addEventListener("click", toggle);
    }
    if (button.parentElement !== menu) menu.appendChild(button);
    const supported = state && allowed(state) && contextKey(currentContext()) && modelCapability(state, model);
    const active = supported && configuredFast();
    button.disabled = writing || !dispatcher || !supported || !!error;
    button.setAttribute("aria-pressed", String(!!active));
    button.setAttribute("aria-label", active ? "关闭本会话 Fast" : "开启本会话 Fast");
    // 用户要求开启时显示实心蓝色闪电，两种状态均无背景；关闭时使用灰色空心图标。
    button.style.color = active ? "#3b82f6" : "var(--color-text-tertiary,currentColor)";
    button.style.background = "transparent";
    button.querySelector("svg")?.setAttribute("fill", active ? "currentColor" : "none");
    button.style.opacity = button.disabled ? ".5" : "1";
    button.title = error || (writing ? "正在保存 Fast 设置" : !state ? "正在读取 Fast 能力"
      : !allowed(state) ? "Fast 入口仅适用于允许 Fast 的本机 API 模式"
        : !contextKey(currentContext()) ? "未识别到当前本机会话"
          : !supported ? "当前模型未声明 Fast 能力"
            : active ? "本会话 Fast 已开启，点击关闭" : "本会话 Fast 已关闭，点击开启");
  }

  async function toggle(event) {
    event.preventDefault();
    event.stopPropagation();
    if (writing || !enabled) return;
    const token = revision;
    const model = selectedModel();
    const context = currentContext();
    const key = contextKey(context);
    if (!key) return;
    writing = true;
    render(snapshot);
    try {
      const state = await readSnapshot(true);
      const capability = modelCapability(state, model);
      if (token !== revision || !allowed(state) || !capability || selectedModel() !== model
        || contextKey(currentContext()) !== key) return;
      // 用户要求现有会话互不影响、新会话默认关闭；不再写入 config.toml 或 profile 的全局服务等级。
      writePreference(context, !configuredFast(context));
    } catch (error) {
      adapters?.notify?.(error.message || "Fast 设置保存失败");
    } finally {
      writing = false;
      if (token === revision) render(snapshot);
    }
  }

  function refresh() {
    if (!enabled) { removeButton(); removeIndicators(); return Promise.resolve(); }
    render(snapshot);
    if (refreshing) return refreshing;
    refreshing = (async () => {
      try {
        if (!dispatcher) await installDispatcher();
        const state = await readSnapshot();
        render(state);
      } catch (error) {
        render(null, error.message || "Fast 暂不可用");
      }
    })().finally(() => { refreshing = null; });
    return refreshing;
  }

  const api = {
    version,
    configure(options) {
      adapters = options;
      const next = options.enabled === true;
      if (next !== enabled) { revision += 1; snapshotAt = 0; }
      enabled = next;
      syncMenuObserver();
      return refresh();
    },
    refresh,
    captureContext() {
      const context = currentContext();
      return context && { ...context, fast: configuredFast(context) };
    },
    bindConversation(context, id) {
      if (!enabled || !context?.draftId || !contextKey(context) || !threadId(id) || boundDrafts.has(context)) return;
      // 新建页的选择只交给本次创建的真实会话；草稿清空后，下一个新会话默认关闭。
      try {
        writePreference({ hostId: context.hostId, threadId: id }, context.fast === true);
        boundDrafts.add(context);
        drafts.delete(contextKey(context));
        void refresh();
      } catch (error) { adapters?.notify?.(error.message || "Fast 会话设置保存失败"); }
    },
    // 新版原生发送走 RPC，和旧 dispatcher 共用同一组账号、模型及管理员限制。
    async patchTurnRequest(params, context = {}) {
      try {
        return (await patchMessage("mcp-request", {
          hostId: "local", request: { method: "turn/start", params },
        }, context)).request.params;
      } catch (error) {
        adapters?.notify?.(`${error.message}，本轮保留原生服务等级`);
        return params;
      }
    },
    status: () => ({ enabled, ready: !!dispatcher, loaded: !!snapshot, model: selectedModel() || null,
      threadId: threadId(currentContext()?.threadId) || null, fast: configuredFast() }),
  };
  if (typeof module === "object" && module.exports) {
    module.exports = { api, modelCapability, allowed, patchMessage, onMessage };
  }
  root.addEventListener?.("message", onMessage);
  root.addEventListener?.("storage", (event) => {
    if (event.key?.startsWith(preferencePrefix)) void refresh();
  });
  root.__codexElvesComposerFast = api;
})();
