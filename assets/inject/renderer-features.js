(() => {
  const helperBase = window.__CODEX_SESSION_DELETE_HELPER__ || "http://127.0.0.1:45221";
  const buttonClass = "codex-delete-button";
  const exportButtonClass = "codex-export-button";
  const projectMoveButtonClass = "codex-project-move-button";
  const projectMoveOverlayClass = "codex-project-move-overlay";
  const codexAppServerRestartButtonClass = "codex-app-server-restart-button";
  const codexAppServerRestartDialogClass = "codex-app-server-restart-dialog";
  const actionButtonClass = "codex-session-action-button";
  const actionGroupClass = "codex-session-actions";
  const moreButtonClass = "codex-session-more-button";
  const moreMenuClass = "codex-session-more-menu";
  const actionTooltipClass = "codex-session-action-tooltip";
  const conversationViewMinWidth = 320;
  const conversationViewMaxAllowedWidth = 4000;
  const conversationViewDefaultWidth = 900;
  const conversationViewLegacyWidthKey = "codexElves.threadCenter.maxWidth";
  const upstreamWorktreeDialogClass = "codex-upstream-worktree-dialog";
  const upstreamBranchOptionAttribute = "data-codex-upstream-branch-option";
  const upstreamBranchSelectionKey = "codexUpstreamBranchSelection";
  const upstreamProjectContextKey = "codexUpstreamProjectContext";
  const projectMoveProjectionKey = "codexProjectMoveProjection";
  const legacyProjectMoveOverridesKey = "codexProjectMoveOverrides";
  const projectMoveProjectionTtlMs = 24 * 60 * 60 * 1000;
  const projectMoveProjectionSettleMs = 5 * 60 * 1000;
  const projectMoveRefreshDelaysMs = [50, 250, 750, 1500];
  const chatsSortEventDelayMs = 80;
  const chatsSortVisibleFallbackMs = 30000;
  const chatsSortRequestTimeoutMs = 10000;
  const styleId = "codex-delete-style";
  const codexDeleteStyleVersion = "60";
  const codexElvesMenuId = "codex-elves-menu";
  const codexElvesMenuVersion = "8";
  const codexElvesMenuFloatingClass = "codex-elves-menu-floating";
  const codexElvesMenuTitlebarClass = "codex-elves-menu-titlebar";
  const codexDeleteVersion = "7";
  const codexActionGroupVersion = "6";
  const codexArchiveRowActionsVersion = "1";
  const codexConversationViewRouteHooksVersion = "2";
  const codexConversationViewRouteRefreshDelaysMs = [0, 80, 220, 500, 1000, 1800, 3000];
  const codexRouteFeatureRefreshDelaysMs = [0, 360];
  const codexThreadServiceTierVersion = "1";
  const codexServiceTierBadgeClass = "codex-service-tier-badge";
  const codexLegacyServiceTierComposerSurfaceClass = "codex-elves-service-tier-composer-surface";
  const codexServiceTierBadgeVersion = "6";
  const codexServiceTierBadgePlacementGraceMs = 1200;
  const codexServiceTierBadgeRetryMaxAttempts = 8;
  const codexServiceTierBadgeRetryMaxDelayMs = 1000;
  let codexElvesVersion = window.__CODEX_ELVES_VERSION__ || "unknown";
  const codexElvesBuild = window.__CODEX_ELVES_BUILD__ || "unknown";
  const codexElvesSettingsKey = "codexElvesSettings";
  const codexThreadServiceTierKey = "codexThreadServiceTierOverrides";
  const codexThreadServiceTierMaxEntries = 120;
  const codexThreadServiceTierDraftBindWindowMs = 60 * 1000;
  const codexServiceTierRequestOverrideVersion = "4";
  const codexServiceTierRequestClientPatchRetryBaseMs = 1000;
  const codexServiceTierRequestClientPatchRetryMaxMs = 30000;
  const codexAppServerManagerDiscoveryVersion = "11";
  const codexAppServerRestartErrorText = "failed to start turn: internal error; agent loop died unexpectedly";
  const codexAppServerRestartRecoveryDelaysMs = [120, 280, 520, 900, 1500, 2400];
  const codexStatsigModelVisibilityConfigId = "107580212";
  const codexStatsigModelVisibilityPatchVersion = "1";
  const codexStatsigModelVisibilityRetryDelayMs = 50;
  const codexStatsigModelVisibilityMaxWaitMs = 60000;
  const codexPluginMarketplaceUnlockVersion = "19";
  const codexPluginApiKeyUnsupportedMarketplaceKinds = new Set(["created-by-me-remote"]);
  const codexBackendHeartbeatIntervalMs = 30000;
  const codexBackendBridgeReadyTimeoutMs = 2000;
  const codexBackendBridgeTimeoutMs = 2000;
  const codexBackendStatusTimeoutMs = 5000;
  const codexElvesImageOverlayId = "codex-elves-image-overlay";
  const codexTokenUsageCardClass = "codex-token-usage-card";
  const codexTokenUsageHostClass = "codex-token-usage-host";
  const codexTokenUsageRefreshIntervalMs = 2500;
  const codexTokenUsageDurationTickIntervalMs = 1000;
  const codexTokenUsageCompletionRefreshDelayMs = 3000;
  const codexSessionRefGraceMs = 15000;
  const codexTokenUsageRetryDelaysMs = [1000, 2500, 5000];
  const codexTokenUsageRequestTimeoutMs = 5000;
  const codexTokenUsageLifecycleTimeoutMs = 30000;
  const codexPluginRequestIdTtlMs = 2 * 60 * 1000;
  const codexPluginRequestIdMaxEntries = 256;
  const codexFailureHistoryMaxEntries = 64;
  const codexManagerReactDiscoveryCooldownMs = 15000;
  const taskBoardRuntimeVersion = "39";
  const taskBoardNativeOperationLeaseTtlMs = 2 * 60 * 1000;
  const taskBoardNativeCreateBusyMessage = "另一个窗口正在创建原生会话，请稍后重试";
  const taskBoardEntryAttribute = "data-codex-task-board-entry";
  const taskBoardEntryContextMenuClass = "codex-task-board-entry-context-menu";
  const taskBoardRootAttribute = "data-codex-task-board-root";
  const taskBoardNativeSelectionAttribute = "data-codex-task-board-native-selection-suppressed";
  const taskBoardEntrySelector = '[data-codex-task-board-entry="true"]';
  const taskBoardRootSelector = '[data-codex-task-board-root="true"]';
  const taskBoardMainHostClass = "codex-task-board-main-host";
  const taskBoardNativeNavigationSelector = [
    "[data-app-action-sidebar-thread-row]",
    "[data-app-action-sidebar-project-row]",
    'nav button',
    'nav [role="button"]',
    'nav a[href]',
    '[role="navigation"] button',
    '[role="navigation"] [role="button"]',
    '[role="navigation"] a[href]',
  ].join(", ");
  const taskBoardNativeSelectionAttributes = [
    "aria-current",
    "aria-selected",
    "data-state",
    "data-selected",
    "data-active",
    "data-app-action-sidebar-thread-selected",
    "data-app-action-sidebar-thread-active",
  ];
  const taskBoardStatusDefinitions = [
    { id: "new", label: "新任务", color: "#94a3b8" },
    { id: "planning", label: "规划中", color: "#60a5fa" },
    { id: "executing", label: "执行中", color: "#c084fc" },
    { id: "review", label: "验收中", color: "#fbbf24" },
    { id: "done", label: "已完成", color: "#34d399" },
  ];
  const taskBoardReasoningEffortDefinitions = [
    { id: "none", label: "无" },
    { id: "minimal", label: "最低" },
    { id: "low", label: "轻度" },
    { id: "medium", label: "中" },
    { id: "high", label: "高" },
    { id: "xhigh", label: "极高" },
    { id: "max", label: "最高" },
    { id: "ultra", label: "超高" },
  ];
  const taskBoardDefaultReasoningEffortIds = ["low", "medium", "high", "xhigh", "max"];
  const taskBoardProjectDropdownWidth = 320;
  const taskBoardBridgeRoutes = {
    snapshot: "/task-board/snapshot",
    catalog: "/task-board/session-catalog",
    createTask: "/task-board/task-create",
    attachConversations: "/task-board/task-conversations-attach",
    detachConversations: "/task-board/task-conversations-detach",
    moveTask: "/task-board/task-move",
    conversationStatus: "/thread-usage-summary",
  };
  window.__codexProjectMoveRuntimeId = (window.__codexProjectMoveRuntimeId || 0) + 1;
  const codexProjectMoveRuntimeId = window.__codexProjectMoveRuntimeId;
  clearTimeout(window.__codexProjectMoveProjectionTimer);
  clearTimeout(window.__codexProjectMoveChatsSortTimer);
  clearTimeout(window.__codexProjectMoveChatsSortFallbackTimer);
  window.__codexProjectMoveProjectionTimer = null;
  window.__codexProjectMoveChatsSortTimer = null;
  window.__codexProjectMoveChatsSortFallbackTimer = null;
  clearTimeout(window.__codexServiceTierDispatcherPatchRetryTimer);
  window.__codexServiceTierDispatcherPatchRetryTimer = null;
  clearTimeout(window.__codexServiceTierRequestClientPatchRetryTimer);
  window.__codexServiceTierRequestClientPatchRetryTimer = null;
  (window.__codexConversationViewRouteTimers || []).forEach((timer) => clearTimeout(timer));
  window.__codexConversationViewRouteTimers = [];
  (window.__codexRouteFeatureRefreshTimers || []).forEach((timer) => clearTimeout(timer));
  window.__codexRouteFeatureRefreshTimers = [];
  (window.__codexSessionDeleteObservers || []).forEach((observer) => observer.disconnect());
  window.__codexSessionDeleteObservers = [];
  window.__codexSessionDeleteObserverConfigs = [];
  if (typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(window.__codexServiceTierBadgeLayoutRafId);
  } else {
    clearTimeout(window.__codexServiceTierBadgeLayoutRafId);
  }
  window.__codexServiceTierBadgeLayoutRafId = 0;
  clearTimeout(window.__codexServiceTierBadgeRetryTimer);
  window.__codexServiceTierBadgeRetryTimer = null;
  window.__codexServiceTierBadgeRetryAttempt = 0;
  clearTimeout(window.__codexTokenUsageRefreshTimer);
  window.__codexTokenUsageRefreshTimer = null;
  clearInterval(window.__codexTokenUsageDurationTimer);
  window.__codexTokenUsageDurationTimer = null;
  clearTimeout(window.__codexTokenUsageRetryTimer);
  window.__codexTokenUsageRetryTimer = null;
  if (typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(window.__codexTokenUsagePinnedSummarySyncRafId);
  }
  window.__codexTokenUsagePinnedSummarySyncRafId = 0;
  window.__codexTokenUsagePinnedSummaryObserver?.disconnect?.();
  window.__codexTokenUsagePinnedSummaryObserver = null;
  window.__codexTokenUsagePinnedSummaryObserverTarget = null;
  window.__codexTokenUsagePinnedSummaryLifecycleObserver?.disconnect?.();
  window.__codexTokenUsagePinnedSummaryLifecycleObserver = null;
  window.__codexTokenUsagePinnedSummaryLifecycleObserverRoot = null;
  if (typeof document !== "undefined") {
    document.removeEventListener(
      "visibilitychange",
      window.__codexTokenUsageVisibilityHandler,
      true
    );
  }
  window.__codexTokenUsageVisibilityHandler = null;
  window.__codexTokenUsageRetryCount = 0;
  window.__codexTokenUsageRefreshPending = false;
  window.__codexTokenUsageWasRunning = false;
  if (!(window.__codexTokenUsageSummaryCache instanceof Map)) {
    window.__codexTokenUsageSummaryCache = new Map();
  }
  window.__codexTokenUsageRequestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
  function cleanupLegacyForcePluginInstallRuntime() {
    window.__codexForcePluginInstallObserver?.disconnect?.();
    window.__codexForcePluginInstallObserver = null;
    window.__codexForcePluginInstallObserverRoot = null;
    clearTimeout(window.__codexForcePluginInstallSettleTimer);
    window.__codexForcePluginInstallSettleTimer = null;
  }
  cleanupLegacyForcePluginInstallRuntime();
  function taskBoardRuntimeCanRefresh(
    installedVersion = window.__codexElvesTaskBoardRuntimeVersion,
    refreshRuntime = window.__codexElvesTaskBoardRefreshRuntime,
  ) {
    return installedVersion === taskBoardRuntimeVersion && typeof refreshRuntime === "function";
  }
  function taskBoardNativeOperationLease() {
    const lease = window.__codexElvesTaskBoardNativeOperationLease;
    const runtimeId = Number(lease?.runtimeId);
    const createdAtMs = Number(lease?.createdAtMs);
    const operationId = String(lease?.operationId || "").trim();
    const active = operationId &&
      Number.isSafeInteger(runtimeId) &&
      runtimeId > 0 &&
      Number.isFinite(createdAtMs) &&
      createdAtMs > 0 &&
      Date.now() - createdAtMs <= taskBoardNativeOperationLeaseTtlMs;
    if (active) return { operationId, runtimeId, createdAtMs };
    if (lease) delete window.__codexElvesTaskBoardNativeOperationLease;
    return null;
  }
  const codexElvesInjectedLaunchCycle = String(window.__CODEX_ELVES_LAUNCH_CYCLE__ || "").trim();
  if (
    window.__codexElvesRuntimeBuild === codexElvesBuild &&
    window.__codexElvesRuntimeHelperBase === helperBase &&
    window.__codexElvesRuntimeManagerDiscoveryVersion === codexAppServerManagerDiscoveryVersion &&
    typeof window.__codexElvesRefreshRuntime === "function" &&
    taskBoardRuntimeCanRefresh()
  ) {
    window.__codexElvesRefreshRuntime();
    return;
  }
  try {
    window.__codexElvesTaskBoardCleanup?.({
      preserveNativeCreate: !!taskBoardNativeOperationLease(),
    });
  } catch (_) {}
  window.__codexElvesTaskBoardCleanup = null;
  window.__codexSessionPrewarmRuntimeId = (window.__codexSessionPrewarmRuntimeId || 0) + 1;
  const codexSessionPrewarmRuntimeId = window.__codexSessionPrewarmRuntimeId;
  let codexElvesAppearanceRegistryPromise = null;
  let codexElvesDesiredAppearanceMode = "";
  let codexElvesAppliedAppearanceMode = "";
  let codexElvesAppearanceApplyRunning = false;
  let codexElvesAppearanceRetryCount = 0;
  let codexElvesAppearanceRetryTimer = null;
  const codexElvesAppearanceRetryDelaysMs = [500, 1500, 5000, 15000];

  async function readCodexSourcePrefix(url, maxBytes = 256 * 1024) {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`无法读取 Codex 脚本：${response.status}`);
    }
    if (!response.body?.getReader) {
      return (await response.text()).slice(0, maxBytes);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let source = "";
    try {
      while (source.length < maxBytes) {
        const { done, value } = await reader.read();
        if (done) break;
        source += decoder.decode(value, { stream: true });
      }
      source += decoder.decode();
      return source;
    } finally {
      try {
        await reader.cancel();
      } catch {
      }
    }
  }

  async function resolveCodexAppearanceRegistry() {
    if (codexElvesAppearanceRegistryPromise) {
      return codexElvesAppearanceRegistryPromise;
    }
    codexElvesAppearanceRegistryPromise = (async () => {
      if (location.protocol !== "app:") {
        throw new Error("当前页面不是 Codex App");
      }
      let entryUrl = "";
      for (let attempt = 0; attempt < 20 && !entryUrl; attempt += 1) {
        entryUrl = Array.from(document.scripts)
          .map((script) => script.src)
          .find((url) => /\/assets\/index-[^/]+\.js(?:\?|$)/.test(url)) || "";
        if (!entryUrl) {
          await new Promise((resolve) => setTimeout(resolve, 250));
        }
      }
      if (!entryUrl) {
        throw new Error("未找到 Codex 入口脚本");
      }
      const entrySource = await readCodexSourcePrefix(entryUrl, 64 * 1024);
      const initialMatch = entrySource.match(/["'`](\.\/app-initial-[^"'`]+\.js)["'`]/);
      if (!initialMatch) {
        throw new Error("未找到 Codex 初始化脚本");
      }
      const initialUrl = new URL(initialMatch[1], entryUrl).href;
      let initialSource = await readCodexSourcePrefix(initialUrl);
      let actionsMatch = initialSource.match(/["'`](\.\/register-app-actions-[^"'`]+\.js)["'`]/);
      if (!actionsMatch) {
        initialSource = await fetch(initialUrl).then((response) => response.text());
        actionsMatch = initialSource.match(/["'`](\.\/register-app-actions-[^"'`]+\.js)["'`]/);
      }
      if (!actionsMatch) {
        throw new Error("当前 Codex 版本未暴露外观动作");
      }
      const actionsUrl = new URL(actionsMatch[1], initialUrl).href;
      const module = await import(actionsUrl);
      const registry = module?.appActionRegistry;
      if (!(registry instanceof Map)) {
        throw new Error("Codex 外观动作注册表不可用");
      }
      return registry;
    })().catch((error) => {
      codexElvesAppearanceRegistryPromise = null;
      throw error;
    });
    return codexElvesAppearanceRegistryPromise;
  }

  function applyCodexElvesSkinAppearance(appearance, enabled) {
    const hadManagedAppearance = Boolean(
      codexElvesDesiredAppearanceMode ||
      codexElvesAppliedAppearanceMode ||
      codexElvesAppearanceApplyRunning
    );
    const mode = enabled
      ? appearance === "light" || appearance === "dark" ? appearance : "system"
      : hadManagedAppearance ? "system" : "";
    if (codexElvesDesiredAppearanceMode !== mode) {
      codexElvesAppearanceRetryCount = 0;
      clearTimeout(codexElvesAppearanceRetryTimer);
      codexElvesAppearanceRetryTimer = null;
    }
    codexElvesDesiredAppearanceMode = mode;
    if (!mode || codexElvesAppearanceApplyRunning) {
      return;
    }
    codexElvesAppearanceApplyRunning = true;
    void (async () => {
      try {
        const registry = await resolveCodexAppearanceRegistry();
        const handler = registry.get("app.appearance.set_mode");
        if (typeof handler !== "function") {
          throw new Error("Codex 外观切换动作不可用");
        }
        while (
          codexElvesDesiredAppearanceMode &&
          codexElvesAppliedAppearanceMode !== codexElvesDesiredAppearanceMode
        ) {
          const nextMode = codexElvesDesiredAppearanceMode;
          await handler(
            { type: "app.appearance.set_mode", mode: nextMode },
            {}
          );
          codexElvesAppliedAppearanceMode = nextMode;
          codexElvesAppearanceRetryCount = 0;
          sendCodexElvesDiagnostic("skin_appearance_applied", { mode: nextMode });
        }
      } catch (error) {
        codexElvesAppearanceRetryCount += 1;
        sendCodexElvesDiagnostic("skin_appearance_failed", {
          message: error instanceof Error ? error.message : String(error),
        });
      } finally {
        codexElvesAppearanceApplyRunning = false;
        if (
          codexElvesDesiredAppearanceMode &&
          codexElvesAppliedAppearanceMode !== codexElvesDesiredAppearanceMode &&
          codexElvesAppearanceRetryCount <= codexElvesAppearanceRetryDelaysMs.length
        ) {
          const retryDelay = codexElvesAppearanceRetryDelaysMs[
            Math.max(0, codexElvesAppearanceRetryCount - 1)
          ];
          codexElvesAppearanceRetryTimer = setTimeout(
            () => applyCodexElvesSkinAppearance(
              codexElvesDesiredAppearanceMode,
              true
            ),
            retryDelay
          );
        }
      }
    })();
  }

  function installCodexElvesImageOverlay() {
    const config = window.__CODEX_ELVES_IMAGE_OVERLAY__ || {};
    const canQueryById = typeof document?.getElementById === "function";
    const existing = canQueryById ? document.getElementById(codexElvesImageOverlayId) : null;
    const source = config.dataUrl || "";
    const kind = ["image", "color", "gradient"].includes(config.kind) ? config.kind : "image";
    const appearance = ["light", "dark", "auto"].includes(config.appearance) ? config.appearance : "auto";
    applyCodexElvesSkinAppearance(appearance, config.appearanceEnabled === true);
    const hasVisual = kind === "image" ? !!source : true;
    if (!config.enabled || !hasVisual) {
      if (window.__codexElvesImageOverlayBlobUrl) {
        URL.revokeObjectURL(window.__codexElvesImageOverlayBlobUrl);
        window.__codexElvesImageOverlayBlobUrl = "";
      }
      if (existing) existing.remove();
      return;
    }
    const root = document?.documentElement;
    if (!root || typeof document?.createElement !== "function") {
      return;
    }
    const opacity = Math.min(1, Math.max(0.01, Number(config.opacity) || 0.35));
    // 皮肤字段：铺法(fit) / 外观(appearance)。
    const fit = config.fit === "cover" ? "cover" : config.fit === "contain" ? "contain" : "contain";
    // 非图片背景(纯色/渐变)用 div 直接绘制，无需加载图片；图片背景用 img 展示并支持铺法。
    const wantedTag = kind === "image" ? "IMG" : "DIV";
    if (existing && existing.tagName !== wantedTag) {
      existing.remove();
    }
    const reused = canQueryById ? document.getElementById(codexElvesImageOverlayId) : null;
    const element = reused || document.createElement(wantedTag === "IMG" ? "img" : "div");
    element.id = codexElvesImageOverlayId;
    element.setAttribute("aria-hidden", "true");
    const baseStyle = {
      position: "fixed",
      inset: "0",
      width: "100vw",
      height: "100vh",
      opacity: String(opacity),
      pointerEvents: "none",
      zIndex: "2147483646",
      userSelect: "none",
    };
    let sourceKind = "unknown";
    if (kind === "image") {
      element.alt = "";
      element.src = source;
      element.style.background = "";
      Object.assign(element.style, {
        ...baseStyle,
        objectFit: fit,
        objectPosition: "50% 50%",
      });
      sourceKind = source.startsWith("data:") ? "data-uri" : "unknown";
    } else if (kind === "color") {
      const color = typeof config.backgroundColor === "string" && config.backgroundColor.trim() ? config.backgroundColor.trim() : "#1e293b";
      Object.assign(element.style, { ...baseStyle, background: color, objectFit: "", objectPosition: "" });
      sourceKind = "color";
    } else {
      const from = typeof config.gradientFrom === "string" && config.gradientFrom.trim() ? config.gradientFrom.trim() : "#4338ca";
      const to = typeof config.gradientTo === "string" && config.gradientTo.trim() ? config.gradientTo.trim() : "#0ea5e9";
      const angle = Number.isFinite(Number(config.gradientAngle)) ? Number(config.gradientAngle) : 135;
      Object.assign(element.style, {
        ...baseStyle,
        background: `linear-gradient(${angle}deg, ${from}, ${to})`,
        objectFit: "",
        objectPosition: "",
      });
      sourceKind = "gradient";
    }
    if (!reused) root.appendChild(element);
    sendCodexElvesDiagnostic("image_overlay_installed", {
      opacity,
      fit,
      appearance,
      kind,
      sourceKind,
    });
  }

  function scheduleCodexElvesImageOverlay() {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", installCodexElvesImageOverlay, { once: true });
      return;
    }
    installCodexElvesImageOverlay();
    setTimeout(installCodexElvesImageOverlay, 250);
  }

  window.__codexElvesApplySkinAppearance = applyCodexElvesSkinAppearance;
  window.__codexElvesApplyImageOverlay = installCodexElvesImageOverlay;

  scheduleCodexElvesImageOverlay();
  let upstreamBranchDefaultsCache = new Map();
  const upstreamBranchDefaultsCacheTtlMs = 5000;
  const upstreamRemoteBranchDefaultsCacheTtlMs = 30000;
  let upstreamBranchDefaultsInflight = new Map();
  const upstreamProjectContextTtlMs = 10 * 60 * 1000;
  const branchWorktreePathAttribute = "data-codex-branch-worktree-path";
  ["__codexElvesHtmlCenteredThreadWidth", "__codexElvesViewportCenteredThreadWidth", "__codexElvesBoundedThreadCenter"].forEach((key) => {
    try {
      window[key]?.cleanup?.();
    } catch (_) {}
  });
  try {
    window.__codexElvesConversationViewCleanup?.();
  } catch (_) {}
  window.__codexElvesConversationViewCleanup = null;

  function cleanupRemovedConversationHelpers(root = document) {
    root.querySelectorAll?.(".codex-conversation-timeline, .codex-thread-id-badge").forEach((node) => node.remove());
    root.querySelectorAll?.('[data-codex-thread-id-badge-wrap="true"]').forEach((wrapper) => {
      const parent = wrapper.parentElement;
      if (!parent) return;
      while (wrapper.firstChild) parent.insertBefore(wrapper.firstChild, wrapper);
      wrapper.remove();
    });
    root.querySelectorAll?.(".codex-conversation-timeline-target").forEach((node) => {
      node.classList.remove("codex-conversation-timeline-target");
    });
  }

  cleanupRemovedConversationHelpers();
  const selectors = {
    sidebarThread: "[data-app-action-sidebar-thread-id]",
    threadTitle: "[data-thread-title]",
    applicationMenuTopBar: '[class*="_ApplicationMenuTopBar_"]',
    applicationMenu: '[aria-label="应用程序菜单"], [aria-label="Application menu"]',
    appHeader: "[data-app-shell-application-menu-bar], .app-header-tint",
    nativeMenuBar: "[class*=\"ms-auto\"][class*=\"flex\"][class*=\"items-center\"]",
    headerContextMenuSurface: '[data-testid="app-shell-header-context-menu-surface"]',
    pinnedSummaryPanel: '[data-pip-obstacle="thread-summary-panel"]',
    pinnedSummaryToggle: 'button[aria-label="切换置顶摘要"], button[title="切换置顶摘要"], button[aria-label="Toggle Pinned Summary"], button[title="Toggle Pinned Summary"]',
    archiveNav: 'button[aria-label="已归档对话"], button[aria-label="Archived conversations"]',
    pluginSvgPath: 'svg path[d^="M7.94562 14.0277"]',
  };
  const pluginNavigationControlSelector = [
    'nav[role="navigation"] button',
    'nav[role="navigation"] [role="button"]',
    '[role="navigation"] button',
    '[role="navigation"] [role="button"]',
    "nav button",
    'nav [role="button"]',
  ].join(", ");
  const pluginEntryLabelPattern = /^(插件|Plugins)(?:\s*-\s*.*)?$/i;
  const taskBoardSettingsNavigationLabelPattern = /^(设置|Settings)$/i;
  const headerIconTextButtonClass = "border-token-border no-drag cursor-interaction flex items-center gap-1 border whitespace-nowrap select-none focus:outline-none disabled:cursor-not-allowed disabled:opacity-40 rounded-lg text-token-text-tertiary enabled:hover:bg-token-list-hover-background data-[state=open]:bg-token-list-hover-background border-transparent h-token-button-composer px-2 py-0 text-base leading-[18px]";

  function installStyle() {
    const existingStyle = document.getElementById(styleId);
    if (existingStyle?.dataset.codexDeleteStyleVersion === codexDeleteStyleVersion) return;
    existingStyle?.remove();
    const style = document.createElement("style");
    style.id = styleId;
    style.dataset.codexDeleteStyleVersion = codexDeleteStyleVersion;
    style.textContent = `
      .${actionGroupClass} {
        position: absolute;
        right: var(--codex-session-actions-right, 28px);
        top: 50%;
        transform: translateY(-50%);
        z-index: 20;
        opacity: 0;
        pointer-events: none;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        background: transparent;
      }
      .${actionGroupClass}[data-codex-action-placement="native"] {
        position: static;
        inset: auto;
        transform: none;
        z-index: auto;
        opacity: 1;
        pointer-events: auto;
        flex: 0 0 auto;
        gap: 8px;
      }
      [data-codex-session-action-host="true"] {
        width: auto !important;
        min-width: 52px !important;
        padding-left: 14px !important;
        background: transparent !important;
      }
      .${actionGroupClass}:not([data-codex-action-placement="native"]) .${actionButtonClass} {
        width: 26px;
        height: 26px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: 0;
        border-radius: 6px;
        background: transparent;
        color: #d1d5db;
        font: 14px/1 system-ui, sans-serif;
        padding: 0;
        cursor: default;
        text-align: center;
      }
      .${actionGroupClass}:not([data-codex-action-placement="native"]) .${actionButtonClass} svg {
        display: block;
        width: 16px;
        height: 16px;
      }
      .${actionGroupClass}:not([data-codex-action-placement="native"]) .${actionButtonClass}:hover,
      .${actionGroupClass}:not([data-codex-action-placement="native"]) .${actionButtonClass}:focus-visible {
        background: #363839;
        color: #f4f4f5;
        outline: none;
      }
      .${moreMenuClass} {
        position: fixed;
        z-index: 2147483201;
        min-width: 104px;
        border: 1px solid rgba(255,255,255,.1);
        border-radius: 10px;
        background: #242628;
        color: #f4f4f5;
        box-shadow: 0 14px 40px rgba(0,0,0,.28);
        padding: 5px;
      }
      .${moreMenuClass}[hidden] { display: none !important; }
      .${moreMenuClass}.codex-session-more-menu-open-up {
        transform: translateY(calc(-100% - 34px));
      }
      .codex-session-more-menu-item {
        width: 100%;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: inherit;
        cursor: default;
        display: flex;
        align-items: center;
        gap: 8px;
        font: 13px/18px system-ui, sans-serif;
        padding: 6px 8px;
        text-align: left;
      }
      .codex-session-more-menu-item:hover,
      .codex-session-more-menu-item:focus-visible {
        background: #363839;
        outline: none;
      }
      .codex-session-more-menu-icon {
        width: 16px;
        text-align: center;
      }
      [${taskBoardEntryAttribute}="true"][data-codex-task-board-context-menu-open="true"] {
        background: var(
          --color-background-primary-soft-hover,
          color-mix(in srgb, currentColor 8%, transparent)
        ) !important;
      }
      .${taskBoardEntryContextMenuClass} {
        position: fixed;
        z-index: 2147483301;
        min-width: 188px;
        border: 1px solid var(
          --color-border-primary-outline,
          color-mix(in srgb, currentColor 16%, transparent)
        );
        border-radius: 10px;
        background: var(--color-token-dropdown-background, #242628);
        color: var(--color-token-text-primary, #f4f4f5);
        box-shadow: 0 14px 40px rgba(0,0,0,.3);
        padding: 5px;
      }
      .${taskBoardEntryContextMenuClass} button {
        display: flex;
        width: 100%;
        min-height: 36px;
        align-items: center;
        gap: 9px;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: inherit;
        cursor: default;
        font: 13px/18px system-ui, sans-serif;
        padding: 0 9px;
        text-align: left;
      }
      .${taskBoardEntryContextMenuClass} button:hover,
      .${taskBoardEntryContextMenuClass} button:focus-visible {
        background: var(
          --color-background-primary-soft-hover,
          color-mix(in srgb, currentColor 9%, transparent)
        );
        outline: none;
      }
      .${taskBoardEntryContextMenuClass} button:active {
        background: var(
          --color-background-primary-soft-active,
          color-mix(in srgb, currentColor 13%, transparent)
        );
      }
      .codex-task-board-entry-context-menu-icon {
        display: inline-flex;
        width: 16px;
        height: 16px;
        flex: 0 0 auto;
        align-items: center;
        justify-content: center;
        color: color-mix(in srgb, currentColor 72%, transparent);
      }
      .codex-task-board-entry-context-menu-icon svg {
        display: block;
        width: 16px;
        height: 16px;
      }
      .codex-archive-row-button {
        border: 1px solid #ef4444;
        border-radius: 7px;
        background: #f3f4f6;
        color: #374151;
        font: 12px system-ui, sans-serif;
        line-height: 16px;
        padding: 3px 8px;
        cursor: pointer;
      }
      .codex-archive-row-button.${exportButtonClass} {
        border-color: #93c5fd;
        background: #dbeafe;
        color: #1d4ed8;
      }
      [data-codex-delete-row="true"]:hover .${actionGroupClass} {
        opacity: 1;
        pointer-events: auto;
      }
      [data-codex-delete-row="true"].codex-session-more-open .${actionGroupClass} {
        opacity: 1;
        pointer-events: auto;
        z-index: 2147483201;
      }
      [data-codex-delete-row="true"]:hover [data-thread-title],
      [data-codex-delete-row="true"]:focus-within [data-thread-title],
      [data-codex-delete-row="true"].codex-session-more-open [data-thread-title] {
        max-width: var(--codex-session-title-max-width) !important;
        flex: 0 1 auto !important;
      }
      @keyframes codex-session-prewarm-shimmer {
        0% {
          -webkit-mask-position: -70% 0;
          mask-position: -70% 0;
        }
        100% {
          -webkit-mask-position: 170% 0;
          mask-position: 170% 0;
        }
      }
      [data-codex-session-prewarming="true"] {
        position: relative !important;
      }
      [data-codex-session-prewarming="true"]::after {
        content: attr(data-codex-session-prewarm-title);
        position: absolute;
        inset: 0;
        overflow: hidden;
        color: #60a5fa;
        white-space: nowrap;
        text-overflow: ellipsis;
        pointer-events: none;
        -webkit-text-fill-color: #60a5fa;
        -webkit-mask-image: linear-gradient(90deg, transparent 0%, #000 35%, #000 65%, transparent 100%);
        mask-image: linear-gradient(90deg, transparent 0%, #000 35%, #000 65%, transparent 100%);
        -webkit-mask-size: 42% 100%;
        mask-size: 42% 100%;
        -webkit-mask-repeat: no-repeat;
        mask-repeat: no-repeat;
        -webkit-mask-position: -70% 0;
        mask-position: -70% 0;
        animation: codex-session-prewarm-shimmer 1.4s linear infinite;
        will-change: -webkit-mask-position, mask-position;
      }
      [data-codex-delete-row="true"].codex-archive-confirm-visible .${actionGroupClass} {
        right: max(66px, var(--codex-session-actions-right, 28px));
      }
      .${actionTooltipClass} {
        position: fixed !important;
        z-index: 2147483201;
        pointer-events: none;
      }
      .${projectMoveOverlayClass} {
        position: fixed;
        inset: 0;
        z-index: 2147483200;
        background: rgba(15,23,42,.28);
      }
      .codex-project-move-panel {
        position: fixed;
        width: min(360px, calc(100vw - 32px));
        max-height: min(520px, calc(100vh - 32px));
        overflow: hidden;
        border: 1px solid rgba(15,23,42,.14);
        border-radius: 10px;
        background: #ffffff;
        color: #111827;
        font: 13px system-ui, sans-serif;
        box-shadow: 0 18px 60px rgba(15,23,42,.25);
      }
      .codex-project-move-header { border-bottom: 1px solid #e5e7eb; padding: 10px 12px; }
      .codex-project-move-title { font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .codex-project-move-list { max-height: min(440px, calc(100vh - 110px)); overflow-y: auto; padding: 6px; }
      .codex-project-move-item {
        display: block;
        width: 100%;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: #111827;
        padding: 8px 9px;
        text-align: left;
        cursor: pointer;
      }
      .codex-project-move-item:hover,
      .codex-project-move-item:focus-visible { background: #f3f4f6; outline: none; }
      .codex-project-move-item-title { font-weight: 550; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .codex-project-move-item-path { margin-top: 2px; color: #6b7280; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .codex-project-move-empty { padding: 18px 12px; color: #6b7280; text-align: center; }
      .codex-project-move-hidden { display: none !important; }
      [data-codex-project-move-injected-list="true"] { display: flex; flex-direction: column; }
      .codex-archive-delete-all {
        border: 1px solid #ef4444;
        border-radius: 7px;
        background: #fee2e2;
        color: #991b1b;
        font: 12px system-ui, sans-serif;
        line-height: 16px;
        padding: 3px 8px;
        cursor: pointer;
      }
      .${codexAppServerRestartButtonClass} {
        position: fixed;
        right: 24px;
        bottom: 96px;
        transform: none;
        z-index: 2147482900;
        min-width: 58px;
        margin: 0;
        border: 1px solid rgba(148,163,184,.55);
        border-radius: 7px;
        background: rgba(255,255,255,.96);
        color: #111827;
        font: 12px/18px system-ui, sans-serif;
        padding: 3px 9px;
        cursor: pointer;
        white-space: nowrap;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        vertical-align: middle;
        -webkit-app-region: no-drag;
      }
      .${codexAppServerRestartButtonClass}:hover,
      .${codexAppServerRestartButtonClass}:focus-visible {
        border-color: rgba(148,163,184,.85);
        background: #ffffff;
        outline: none;
      }
      .${codexAppServerRestartButtonClass}:disabled {
        cursor: wait;
        opacity: .65;
      }
      .${codexAppServerRestartButtonClass}[data-codex-app-server-restart-placement="notice"] {
        min-width: 0;
        border-color: transparent !important;
        border-radius: 8px;
        background: #111827 !important;
        color: #ffffff !important;
        font: 13px/18px system-ui, sans-serif;
        padding: 10px 12px;
        box-shadow: 0 8px 30px rgba(0,0,0,.25);
      }
      .${codexAppServerRestartButtonClass}[data-codex-app-server-restart-placement="notice"]:hover,
      .${codexAppServerRestartButtonClass}[data-codex-app-server-restart-placement="notice"]:focus-visible {
        border-color: transparent !important;
        background: #1f2937 !important;
      }
      .codex-delete-toast {
        position: fixed;
        right: 18px;
        bottom: 18px;
        z-index: 2147483000;
        padding: 10px 12px;
        border-radius: 8px;
        background: #111827;
        color: white;
        font: 13px system-ui, sans-serif;
        box-shadow: 0 8px 30px rgba(0,0,0,.25);
        pointer-events: none;
      }
      .codex-delete-toast button { margin-left: 10px; pointer-events: auto; }
      .codex-delete-confirm-overlay {
        --codex-confirm-surface: var(
          --color-background-elevated-primary-opaque,
          var(
            --color-background-elevated-primary,
            var(--color-token-dropdown-background, Canvas)
          )
        );
        --codex-confirm-foreground: var(
          --color-text-primary,
          var(--color-token-text-primary, CanvasText)
        );
        --codex-confirm-muted: var(
          --color-text-secondary,
          var(
            --color-token-text-secondary,
            color-mix(in srgb, var(--codex-confirm-foreground) 72%, transparent)
          )
        );
        --codex-confirm-border: var(
          --color-border-primary-outline,
          color-mix(in srgb, var(--codex-confirm-foreground) 16%, transparent)
        );
        --codex-confirm-action-background: var(
          --color-background-button-secondary,
          color-mix(in srgb, var(--codex-confirm-foreground) 6%, transparent)
        );
        --codex-confirm-action-background-hover: var(
          --color-background-button-secondary-hover,
          color-mix(in srgb, var(--codex-confirm-foreground) 10%, transparent)
        );
        --codex-confirm-action-background-active: var(
          --color-background-button-secondary-active,
          color-mix(in srgb, var(--codex-confirm-foreground) 14%, transparent)
        );
        --codex-confirm-danger-background: var(
          --color-background-danger-soft,
          color-mix(in srgb, #ef4444 14%, transparent)
        );
        --codex-confirm-danger-background-hover: var(
          --color-background-danger-soft-hover,
          color-mix(in srgb, #ef4444 19%, transparent)
        );
        --codex-confirm-danger-background-active: var(
          --color-background-danger-soft-active,
          color-mix(in srgb, #ef4444 24%, transparent)
        );
        --codex-confirm-danger-border: var(
          --color-border-danger-surface,
          color-mix(in srgb, #ef4444 22%, transparent)
        );
        --codex-confirm-danger-foreground: var(
          --color-text-danger-soft,
          var(--color-text-danger, #ef4444)
        );
        position: fixed;
        inset: 0;
        z-index: 2147483200;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(0,0,0,.58);
        backdrop-filter: blur(4px);
      }
      .codex-delete-confirm-content {
        width: min(420px, calc(100vw - 48px));
        border: 1px solid var(--codex-confirm-border);
        border-radius: 12px;
        background: var(--codex-confirm-surface);
        color: var(--codex-confirm-foreground);
        font: 14px system-ui, sans-serif;
        box-shadow: 0 24px 80px rgba(0,0,0,.42);
        padding: 18px;
      }
      .codex-delete-confirm-title { font-size: 16px; font-weight: 650; }
      .codex-delete-confirm-message {
        margin-top: 8px;
        color: var(--codex-confirm-muted);
        line-height: 1.45;
      }
      .codex-delete-confirm-actions {
        display: flex;
        justify-content: flex-end;
        gap: 10px;
        margin-top: 18px;
      }
      .codex-delete-confirm-actions button {
        min-height: 32px;
        border: 1px solid var(--codex-confirm-border);
        border-radius: 7px;
        padding: 6px 12px;
        background: var(--codex-confirm-action-background);
        color: var(--codex-confirm-foreground);
        font: 13px system-ui, sans-serif;
        cursor: pointer;
        transition:
          background-color 160ms ease,
          border-color 160ms ease,
          color 160ms ease;
      }
      .codex-delete-confirm-actions button:hover {
        background: var(--codex-confirm-action-background-hover);
      }
      .codex-delete-confirm-actions button:active {
        background: var(--codex-confirm-action-background-active);
      }
      .codex-delete-confirm-actions button:focus-visible {
        outline: 2px solid var(--color-border-primary-outline-hover, #38bdf8);
        outline-offset: 2px;
      }
      .codex-delete-confirm-actions button:disabled {
        cursor: wait;
        opacity: .5;
      }
      .codex-delete-confirm-actions [data-codex-delete-confirm="true"] {
        border-color: var(--codex-confirm-danger-border);
        background: var(--codex-confirm-danger-background);
        color: var(--codex-confirm-danger-foreground);
      }
      .codex-delete-confirm-actions [data-codex-delete-confirm="true"]:hover {
        background: var(--codex-confirm-danger-background-hover);
      }
      .codex-delete-confirm-actions [data-codex-delete-confirm="true"]:active {
        background: var(--codex-confirm-danger-background-active);
      }
      .codex-delete-confirm-actions [data-codex-delete-confirm="true"]:focus-visible {
        outline-color: var(--color-ring-danger-soft, var(--codex-confirm-danger-foreground));
      }
      /* Dark theme fallbacks for project-move surfaces and restart controls.
         Triggered either by Codex applying a "dark" class / data-theme="dark"
         on its document root, or by the OS-level prefers-color-scheme hint.
         Confirmation dialogs above use Codex semantic tokens directly. */
      html.dark .${codexAppServerRestartButtonClass},
      html[data-theme="dark"] .${codexAppServerRestartButtonClass},
      :root[data-theme="dark"] .${codexAppServerRestartButtonClass} {
        border-color: rgba(255,255,255,.22);
        background: rgba(255,255,255,.08);
        color: #f3f4f6;
      }
      html.dark .${projectMoveOverlayClass},
      html[data-theme="dark"] .${projectMoveOverlayClass},
      :root[data-theme="dark"] .${projectMoveOverlayClass} {
        background: rgba(0,0,0,.55);
      }
      html.dark .codex-project-move-panel,
      html[data-theme="dark"] .codex-project-move-panel,
      :root[data-theme="dark"] .codex-project-move-panel {
        border-color: rgba(255,255,255,.12);
        background: #2b2b2b;
        color: #f3f4f6;
        box-shadow: 0 18px 60px rgba(0,0,0,.55);
      }
      html.dark .codex-project-move-header,
      html[data-theme="dark"] .codex-project-move-header,
      :root[data-theme="dark"] .codex-project-move-header {
        border-bottom-color: rgba(255,255,255,.1);
      }
      html.dark .codex-project-move-item,
      html[data-theme="dark"] .codex-project-move-item,
      :root[data-theme="dark"] .codex-project-move-item {
        color: #f3f4f6;
      }
      html.dark .codex-project-move-item:hover,
      html.dark .codex-project-move-item:focus-visible,
      html[data-theme="dark"] .codex-project-move-item:hover,
      html[data-theme="dark"] .codex-project-move-item:focus-visible,
      :root[data-theme="dark"] .codex-project-move-item:hover,
      :root[data-theme="dark"] .codex-project-move-item:focus-visible {
        background: rgba(255,255,255,.08);
      }
      html.dark .codex-project-move-item-path,
      html[data-theme="dark"] .codex-project-move-item-path,
      :root[data-theme="dark"] .codex-project-move-item-path,
      html.dark .codex-project-move-empty,
      html[data-theme="dark"] .codex-project-move-empty,
      :root[data-theme="dark"] .codex-project-move-empty {
        color: #9ca3af;
      }
      @media (prefers-color-scheme: dark) {
        html:not(.light):not([data-theme="light"]) .${codexAppServerRestartButtonClass} {
          border-color: rgba(255,255,255,.22);
          background: rgba(255,255,255,.08);
          color: #f3f4f6;
        }
        html:not(.light):not([data-theme="light"]) .${projectMoveOverlayClass} {
          background: rgba(0,0,0,.55);
        }
        html:not(.light):not([data-theme="light"]) .codex-project-move-panel {
          border-color: rgba(255,255,255,.12);
          background: #2b2b2b;
          color: #f3f4f6;
          box-shadow: 0 18px 60px rgba(0,0,0,.55);
        }
        html:not(.light):not([data-theme="light"]) .codex-project-move-header {
          border-bottom-color: rgba(255,255,255,.1);
        }
        html:not(.light):not([data-theme="light"]) .codex-project-move-item {
          color: #f3f4f6;
        }
        html:not(.light):not([data-theme="light"]) .codex-project-move-item:hover,
        html:not(.light):not([data-theme="light"]) .codex-project-move-item:focus-visible {
          background: rgba(255,255,255,.08);
        }
        html:not(.light):not([data-theme="light"]) .codex-project-move-item-path,
        html:not(.light):not([data-theme="light"]) .codex-project-move-empty {
          color: #9ca3af;
        }
      }
      #${codexElvesMenuId}.${codexElvesMenuFloatingClass} {
        position: fixed;
        top: var(--codex-elves-menu-top, 0);
        right: var(--codex-elves-menu-right, 140px);
        left: auto;
        z-index: 40;
        height: var(--codex-elves-menu-height, 30px);
        color: #d1d5db;
        font: 13px system-ui, sans-serif;
        text-align: right;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      #${codexElvesMenuId} {
        display: inline-flex;
        align-items: center;
        height: 100%;
        flex: 0 0 auto;
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      #${codexElvesMenuId}.${codexElvesMenuTitlebarClass} {
        margin-inline-start: auto;
        margin-inline-end: 4px;
      }
      .codex-elves-trigger {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 4px;
        border: 0;
        background: transparent;
        color: inherit;
        font: inherit;
        height: 100%;
        line-height: 1;
        padding: 0 8px;
        cursor: pointer;
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      .codex-elves-modal-overlay {
        position: fixed;
        inset: 0;
        z-index: 2147483646;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(0,0,0,.45);
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      .codex-elves-modal-content {
        width: min(600px, calc(100vw - 48px));
        min-width: 600px;
        max-height: min(680px, calc(100vh - 40px));
        display: flex;
        flex-direction: column;
        overflow: hidden;
        border: 1px solid rgba(255,255,255,.12);
        border-radius: 18px;
        background: #2b2b2b;
        color: #f3f4f6;
        font: 14px system-ui, sans-serif;
        box-shadow: 0 24px 80px rgba(0,0,0,.45);
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      .codex-elves-modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 16px 20px 8px;
        flex: 0 0 auto;
        -webkit-app-region: no-drag;
      }
      .codex-elves-modal-title { display: flex; align-items: center; gap: 8px; font-size: 18px; font-weight: 650; }
      .codex-elves-backend-indicator { width: 9px; height: 9px; border-radius: 999px; background: #a1a1aa; display: inline-block; }
      #${codexElvesMenuId} .codex-elves-backend-indicator + [data-codex-elves-trigger-label] { margin-inline-start: 4px; }
      .codex-elves-backend-indicator[data-status="ok"] { background: #34d399; box-shadow: 0 0 8px rgba(52,211,153,.75); }
      .codex-elves-backend-indicator[data-status="failed"] { background: #ef4444; box-shadow: 0 0 8px rgba(239,68,68,.75); }
      .codex-elves-backend-indicator[data-status="checking"] { background: #fbbf24; }
      .codex-elves-modal-close {
        border: 0;
        background: transparent;
        color: #d1d5db;
        font-size: 20px;
        cursor: pointer;
        pointer-events: auto;
        -webkit-app-region: no-drag;
      }
      .codex-elves-modal-body {
        flex: 1 1 auto;
        min-height: 0;
        overflow-y: auto;
        overscroll-behavior: contain;
        scrollbar-gutter: stable;
        padding: 4px 20px 16px;
        scrollbar-width: thin;
        scrollbar-color: rgba(255,255,255,.28) transparent;
      }
      .codex-elves-modal-body::-webkit-scrollbar { width: 10px; }
      .codex-elves-modal-body::-webkit-scrollbar-track { background: transparent; }
      .codex-elves-modal-body::-webkit-scrollbar-thumb {
        border: 2px solid transparent;
        border-radius: 999px;
        background: rgba(255,255,255,.28);
        background-clip: padding-box;
      }
      .codex-elves-modal-body::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,.38); background-clip: padding-box; }
      .codex-elves-row {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 0;
        border-top: 1px solid rgba(255,255,255,.1);
      }
      .codex-elves-row:first-child { border-top: 0; }
      .codex-elves-row-title { font-weight: 550; line-height: 1.35; }
      .codex-elves-row-description { margin-top: 2px; color: #a1a1aa; font-size: 12px; line-height: 1.4; }
      .codex-elves-model-compat-warning { margin-top: 6px; color: #fbbf24; font-size: 12px; line-height: 1.45; }
      .codex-elves-toggle {
        width: 42px;
        height: 24px;
        border: 0;
        border-radius: 999px;
        background: #52525b;
        padding: 2px;
      }
      .codex-elves-toggle span {
        display: block;
        width: 20px;
        height: 20px;
        border-radius: 999px;
        background: white;
        transition: transform .12s ease;
      }
      .codex-elves-toggle,
      .codex-elves-action-button,
      .codex-elves-issue-button,
      .codex-elves-backend-status {
        flex-shrink: 0;
        align-self: center;
      }
      .codex-elves-toggle[data-enabled="true"] { background: #10a37f; }
      .codex-elves-toggle[data-enabled="true"] span { transform: translateX(18px); }
      .codex-elves-toggle[data-relay-unneeded="true"] { width: 72px; cursor: default; background: rgba(16,163,127,.16); color: #6ee7b7; }
      .codex-elves-toggle[data-relay-unneeded="true"] span { display: none; }
      .codex-elves-toggle[data-relay-unneeded="true"]::after { content: "无需开启"; font-size: 12px; font-weight: 650; line-height: 1; }
      .codex-elves-width-control { display: flex; align-items: center; justify-content: flex-end; gap: 8px; min-width: 176px; align-self: center; }
      .codex-elves-width-input {
        width: 78px;
        height: 26px;
        box-sizing: border-box;
        border: 1px solid rgba(255,255,255,.18);
        border-radius: 7px;
        background: rgba(255,255,255,.08);
        color: #f3f4f6;
        font: 12px system-ui, sans-serif;
        padding: 0 8px;
      }
      .codex-elves-width-input:disabled { opacity: .55; cursor: not-allowed; }
      .codex-elves-service-tier-control { display: grid; gap: 6px; min-width: 316px; justify-items: end; align-self: center; }
      .codex-elves-service-tier-status { color: #a1a1aa; font-size: 12px; line-height: 1.3; text-align: right; }
      .codex-elves-service-tier-status[data-status="ok"] { color: #34d399; }
      .codex-elves-service-tier-status[data-status="failed"] { color: #f87171; }
      .codex-elves-service-tier-status[data-status="unsupported"] { color: #fbbf24; }
      .codex-elves-service-tier-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 6px; }
      .codex-elves-service-tier-thread-actions { opacity: .88; align-items: center; }
      .codex-elves-service-tier-thread-label { color: #a1a1aa; font: 12px/1.2 system-ui, sans-serif; white-space: nowrap; }
      .codex-elves-service-tier-button { border: 1px solid rgba(255,255,255,.18); border-radius: 7px; background: #3f3f46; color: #f3f4f6; font: 12px system-ui, sans-serif; padding: 5px 8px; white-space: nowrap; }
      .codex-elves-service-tier-button[data-active="true"] { border-color: #10a37f; background: rgba(16,163,127,.22); color: #6ee7b7; }
      .codex-elves-service-tier-button:disabled { opacity: .55; cursor: not-allowed; }
      [data-codex-tooltip] { position: relative; }
      [data-codex-tooltip]::before,
      [data-codex-tooltip]::after {
        display: none;
        position: absolute;
        left: 50%;
        z-index: 2147483647;
        opacity: 0;
        pointer-events: none;
        transform: translate(-50%, -2px);
        transition: opacity .12s ease, transform .12s ease;
      }
      [data-codex-tooltip]::before {
        top: calc(100% + 3px);
        width: 8px;
        height: 8px;
        border-left: 1px solid rgba(255,255,255,.12);
        border-top: 1px solid rgba(255,255,255,.12);
        background: #242628;
        content: "";
        transform: translate(-50%, -2px) rotate(45deg);
      }
      [data-codex-tooltip]::after {
        top: calc(100% + 7px);
        width: max-content;
        max-width: min(360px, calc(100vw - 32px));
        border: 1px solid rgba(255,255,255,.12);
        border-radius: 10px;
        background: #242628;
        color: #f4f4f5;
        content: attr(data-codex-tooltip);
        font: 12px/18px system-ui, sans-serif;
        padding: 8px 10px;
        text-align: left;
        white-space: pre-line;
        box-shadow: 0 14px 40px rgba(0,0,0,.28);
      }
      [data-codex-tooltip]:hover::before,
      [data-codex-tooltip]:hover::after,
      [data-codex-tooltip]:focus-visible::before,
      [data-codex-tooltip]:focus-visible::after {
        display: block;
        opacity: 1;
        transform: translate(-50%, 0);
      }
      [data-codex-tooltip]:hover::before,
      [data-codex-tooltip]:focus-visible::before {
        transform: translate(-50%, 0) rotate(45deg);
      }
      .${codexServiceTierBadgeClass} {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: 0 0 auto;
        height: 24px;
        min-width: 54px;
        box-sizing: border-box;
        border: 1px solid rgba(148,163,184,.28);
        border-radius: 999px;
        background: rgba(148,163,184,.12);
        color: #d4d4d8;
        font: 600 12px/1 system-ui, sans-serif;
        padding: 0 8px;
        white-space: nowrap;
        cursor: pointer;
      }
      .${codexServiceTierBadgeClass}:hover { border-color: rgba(16,163,127,.44); background: rgba(16,163,127,.13); }
      .${codexServiceTierBadgeClass}[data-tier="fast"] { border-color: rgba(16,163,127,.55); background: rgba(16,163,127,.18); color: #6ee7b7; }
      .${codexServiceTierBadgeClass}[data-tier="loading"] { color: #a1a1aa; }
      .${codexServiceTierBadgeClass}[data-tier="failed"] { border-color: rgba(248,113,113,.42); background: rgba(248,113,113,.12); color: #fca5a5; }
      .${codexServiceTierBadgeClass}[data-tier="unsupported"] { border-color: rgba(251,191,36,.48); background: rgba(251,191,36,.13); color: #fbbf24; }
      .${codexServiceTierBadgeClass}[data-disabled="true"] { cursor: not-allowed; opacity: .78; }
      .${codexServiceTierBadgeClass}[data-codex-service-tier-portal="true"] {
        position: fixed;
        z-index: 2147483000;
        margin: 0;
        pointer-events: auto;
      }
      .composer-surface-chrome {
        scrollbar-width: none !important;
        -ms-overflow-style: none !important;
      }
      .composer-surface-chrome::-webkit-scrollbar {
        width: 0 !important;
        height: 0 !important;
        display: none !important;
      }
      .composer-surface-chrome [class*="_WorkTriggerMeasurement_"][aria-hidden="true"],
      [class*="_multilineSurface_"] [class*="_WorkTriggerMeasurement_"][aria-hidden="true"],
      .composer-surface-chrome [class*="_ModelPickerTriggerMeasurement_"][aria-hidden="true"],
      [class*="_multilineSurface_"] [class*="_ModelPickerTriggerMeasurement_"][aria-hidden="true"] {
        block-size: 0 !important;
        max-block-size: 0 !important;
        overflow: clip !important;
      }
      .codex-elves-about { color: #a1a1aa; line-height: 1.5; }
      .codex-elves-tabs { display: flex; gap: 8px; padding: 0 20px 6px; flex: 0 0 auto; }
      .codex-elves-tab-button { border: 1px solid rgba(255,255,255,.14); border-radius: 999px; background: transparent; color: #d1d5db; font: 12px system-ui, sans-serif; padding: 5px 10px; }
      .codex-elves-tab-button[data-active="true"] { background: #10a37f; color: white; border-color: #10a37f; }
      .codex-elves-panel[hidden] { display: none; }
      .codex-elves-action-button,
      .codex-elves-issue-button { border: 1px solid rgba(255,255,255,.18); border-radius: 7px; background: #3f3f46; color: #f3f4f6; font: 12px system-ui, sans-serif; padding: 6px 8px; }
      .codex-elves-worktree-actions {
        display: inline-flex;
        align-items: center;
        gap: 8px;
      }
      .codex-elves-form-field {
        display: grid;
        gap: 4px;
        margin-top: 10px;
        color: #d4d4d8;
        font: 12px system-ui, sans-serif;
        text-align: left;
      }
      .codex-elves-form-field input {
        width: min(520px, 72vw);
        border: 1px solid rgba(255,255,255,.18);
        border-radius: 8px;
        background: #18181b;
        color: #f4f4f5;
        padding: 8px 10px;
        font: 13px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      }
      .codex-elves-form-message {
        min-height: 18px;
        margin-top: 10px;
        color: #a1a1aa;
        font: 12px system-ui, sans-serif;
        text-align: left;
      }
      .codex-elves-form-message[data-status="ok"] { color: #34d399; }
      .codex-elves-form-message[data-status="failed"] { color: #f87171; }
      .codex-elves-form-message[data-status="loading"] { color: #fbbf24; }
      .codex-elves-backend-status { display: grid; gap: 4px; min-width: 132px; justify-items: end; }
      .codex-elves-backend-label { color: #a1a1aa; font-size: 12px; }
      .codex-elves-backend-label[data-status="ok"] { color: #34d399; }
      .codex-elves-backend-label[data-status="failed"] { color: #f87171; }
      .codex-elves-backend-repair { border: 1px solid rgba(255,255,255,.18); border-radius: 7px; background: #3f3f46; color: #f3f4f6; font: 12px system-ui, sans-serif; padding: 6px 8px; }
      .codex-elves-backend-repair[hidden] { display: none; }
      .codex-elves-user-script-warning { margin-top: 4px; color: #fbbf24; font-size: 12px; }
      .codex-elves-user-script-dirs { margin-top: 6px; color: #a1a1aa; font-size: 11px; line-height: 1.4; word-break: break-all; }
      .codex-elves-user-script-list { margin-top: 8px; display: grid; gap: 6px; }
      .codex-elves-user-script-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; border: 1px solid rgba(255,255,255,.08); border-radius: 8px; padding: 6px 8px; }
      .codex-elves-user-script-name { font-size: 12px; }
      .codex-elves-user-script-meta { margin-top: 2px; color: #a1a1aa; font-size: 11px; }
      .codex-elves-user-script-error { margin-top: 2px; color: #f87171; font-size: 11px; word-break: break-all; }
      .codex-elves-user-script-actions { display: grid; justify-items: end; gap: 8px; min-width: 120px; }
      .codex-elves-user-script-reload { border: 1px solid rgba(255,255,255,.18); border-radius: 7px; background: #3f3f46; color: #f3f4f6; font: 12px system-ui, sans-serif; padding: 6px 8px; }
      .${codexTokenUsageCardClass} {
        box-sizing: border-box;
        display: block;
        width: 100%;
        margin-top: 10px;
        padding: 11px 14px;
        overflow: hidden;
        border: 0;
        border-radius: 18px;
        background: var(--color-token-dropdown-background, rgb(47,47,47));
        box-shadow: none;
        color: inherit;
        font-family: system-ui, sans-serif;
        pointer-events: none;
        cursor: default;
      }
      .${codexTokenUsageHostClass} {
        flex-direction: column !important;
        align-items: flex-start !important;
      }
      .${codexTokenUsageHostClass} > .${codexTokenUsageCardClass} {
        width: calc(100% - var(--codex-token-usage-panel-end-gap, 0px));
        min-height: 0;
        height: auto;
        flex: 0 0 auto;
        align-self: flex-start;
      }
      .codex-token-usage-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-width: 0;
        padding-bottom: 9px;
      }
      .codex-token-usage-title {
        color: currentColor;
        font-size: 14px;
        font-weight: 445;
        line-height: 21px;
        opacity: .66;
      }
      .codex-token-usage-agent-count {
        flex: 0 0 auto;
        padding: 1px 6px;
        border-radius: 999px;
        background: color-mix(in srgb, currentColor 8%, transparent);
        color: currentColor;
        font-size: 10px;
        font-weight: 445;
        line-height: 16px;
        opacity: .62;
      }
      .codex-token-usage-stale {
        flex: 0 0 auto;
        margin-left: auto;
        padding: 1px 6px;
        border-radius: 999px;
        background: color-mix(in srgb, currentColor 8%, transparent);
        color: currentColor;
        font-size: 10px;
        font-weight: 445;
        line-height: 16px;
        opacity: .5;
      }
      .codex-token-usage-section {
        display: grid;
        gap: 6px;
      }
      .codex-token-usage-section + .codex-token-usage-section {
        margin-top: 9px;
        padding-top: 9px;
        border-top: 1px solid color-mix(in srgb, currentColor 10%, transparent);
      }
      .codex-token-usage-section-head {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 8px;
        min-width: 0;
      }
      .codex-token-usage-label {
        min-width: 0;
        color: currentColor;
        font-size: 12px;
        font-weight: 445;
        line-height: 18px;
        opacity: .58;
      }
      .codex-token-usage-last-turn-label {
        display: inline-flex;
        align-items: baseline;
        gap: 5px;
      }
      .codex-token-usage-duration {
        color: currentColor;
        font-size: 12px;
        font-weight: 520;
        font-variant-numeric: tabular-nums;
        opacity: .8;
      }
      .codex-token-usage-value {
        color: currentColor;
        font-size: 15px;
        font-weight: 600;
        line-height: 18px;
        letter-spacing: -.01em;
        font-variant-numeric: tabular-nums;
      }
      .codex-token-usage-section:last-child .codex-token-usage-value {
        font-size: 13px;
        font-weight: 560;
        opacity: .88;
      }
      .codex-token-usage-metrics {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        align-items: center;
        gap: 6px;
        min-width: 0;
      }
      .codex-token-usage-metric {
        display: inline-flex;
        align-items: baseline;
        gap: 3px;
        min-width: 0;
        white-space: nowrap;
      }
      .codex-token-usage-metric:nth-child(2) {
        justify-content: center;
      }
      .codex-token-usage-metric:nth-child(3) {
        justify-content: flex-end;
      }
      .codex-token-usage-metric-label {
        color: currentColor;
        font-size: 10.5px;
        line-height: 16px;
        opacity: .48;
      }
      .codex-token-usage-metric-value {
        color: currentColor;
        font-size: 11.5px;
        font-weight: 520;
        line-height: 16px;
        font-variant-numeric: tabular-nums;
        opacity: .82;
      }
      .codex-token-usage-status {
        margin-top: 6px;
        color: currentColor;
        font-size: 12px;
        line-height: 18px;
        opacity: .58;
      }
      .${taskBoardMainHostClass} {
        position: relative;
        container-type: inline-size;
        isolation: isolate;
        min-height: 0 !important;
        overflow: hidden !important;
      }
      .${taskBoardMainHostClass} > :not([${taskBoardRootAttribute}="true"]) {
        display: none !important;
      }
      [${taskBoardNativeSelectionAttribute}="true"] {
        background-color: transparent !important;
        box-shadow: none !important;
      }
      [${taskBoardRootAttribute}="true"],
      .codex-task-board-create-modal {
        --task-board-action-background: var(--color-background-button-secondary, color-mix(in srgb, currentColor 5%, transparent));
        --task-board-action-background-hover: var(--color-background-button-secondary-hover, color-mix(in srgb, currentColor 8%, transparent));
        --task-board-action-background-active: var(--color-background-button-secondary-active, color-mix(in srgb, currentColor 12%, transparent));
        --task-board-action-foreground: var(--color-token-text-primary, var(--color-text-primary, currentColor));
        --task-board-action-border: var(--color-border-primary-outline, color-mix(in srgb, currentColor 16%, transparent));
      }
      [${taskBoardRootAttribute}="true"] {
        --task-board-panel-background: var(
          --color-background-secondary,
          var(--color-token-main-surface-secondary, #282828)
        );
        --task-board-card-background: var(
          --color-background-elevated-primary,
          var(--color-background-secondary, var(--color-token-dropdown-background, #303030))
        );
        --task-board-card-background-hover: var(
          --color-background-primary-soft-active,
          color-mix(in srgb, var(--task-board-card-background) 88%, currentColor 8%)
        );
        --task-board-border: var(
          --color-border-primary-outline,
          color-mix(in srgb, currentColor 16%, transparent)
        );
        --task-board-border-soft: var(
          --color-border-primary-soft,
          color-mix(in srgb, currentColor 9%, transparent)
        );
        --task-board-text-secondary: var(
          --color-text-secondary,
          color-mix(in srgb, currentColor 72%, transparent)
        );
        --task-board-text-tertiary: var(
          --color-text-tertiary,
          color-mix(in srgb, currentColor 54%, transparent)
        );
        --task-board-accent: var(
          --color-text-accent,
          var(--color-token-text-accent, #63aee0)
        );
        position: absolute;
        inset: 0;
        display: flex;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        height: 100%;
        overflow: hidden;
        color: var(--color-token-text-primary, #f4f4f5);
        background: var(--color-token-main-surface-primary, #1f1f1f);
        font: 14px/1.45 system-ui, sans-serif;
      }
      .codex-task-board-page {
        display: flex;
        flex: 1 1 auto;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        gap: 16px;
        padding: 24px 28px;
      }
      .codex-task-board-heading {
        display: flex;
        align-items: baseline;
        gap: 10px;
        min-width: 0;
      }
      .codex-task-board-heading h1 {
        margin: 0;
        color: inherit;
        font-size: 24px;
        font-weight: 680;
        letter-spacing: -.02em;
        line-height: 1.2;
      }
      .codex-task-board-description {
        margin: -8px 0 0;
        color: color-mix(in srgb, currentColor 62%, transparent);
        font-size: 13px;
      }
      .codex-task-board-toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
      }
      .codex-task-board-search-control,
      .codex-task-board-project-filter,
      .codex-task-board-create {
        min-height: 36px;
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        border-radius: 8px;
        background: color-mix(in srgb, currentColor 5%, transparent);
        color: inherit;
        font: inherit;
      }
      .codex-task-board-search-control {
        display: flex;
        align-items: center;
        gap: 8px;
        width: min(320px, 100%);
        min-width: 160px;
        padding: 0 10px;
      }
      .codex-task-board-search-control svg {
        flex: 0 0 auto;
        color: color-mix(in srgb, currentColor 58%, transparent);
      }
      .codex-task-board-search {
        flex: 1 1 auto;
        min-width: 0;
        min-height: 34px;
        border: 0;
        outline: 0;
        background: transparent;
        color: inherit;
        font: inherit;
        padding: 0;
      }
      .codex-task-board-project-filter {
        flex: 0 0 132px;
        width: 132px;
        max-width: 132px;
      }
      .codex-task-board-dropdown-trigger {
        box-sizing: border-box;
        display: inline-flex;
        min-width: 0;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 0 10px;
        cursor: pointer;
        text-align: left;
      }
      .codex-task-board-dropdown-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-dropdown-chevron {
        display: block;
        width: 14px;
        height: 14px;
        flex: 0 0 auto;
        color: color-mix(in srgb, currentColor 58%, transparent);
      }
      .codex-task-board-dropdown-trigger[aria-expanded="true"] {
        border-color: color-mix(in srgb, currentColor 30%, #63aee0);
        background: color-mix(in srgb, currentColor 7%, transparent);
      }
      .codex-task-board-create {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        flex: 0 0 auto;
        min-width: 98px;
        border-color: var(--task-board-action-border);
        background: var(--task-board-action-background);
        color: var(--task-board-action-foreground);
        padding: 0 12px;
        cursor: pointer;
      }
      .codex-task-board-hint {
        flex: 1 1 180px;
        min-width: 0;
        max-width: 420px;
        margin-left: auto;
        overflow: hidden;
        color: color-mix(in srgb, currentColor 54%, transparent);
        font-size: 12px;
        text-align: right;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-hint[data-status="failed"] { color: #fca5a5; }
      .codex-task-board-hint[data-status="warning"] { color: #fbbf24; }
      .codex-task-board-hint[data-status="loading"] { color: #cbd5e1; }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="wrapped"] .codex-task-board-toolbar {
        flex-wrap: wrap;
      }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="wrapped"] .codex-task-board-search-control {
        flex: 1 1 100%;
        width: 100%;
      }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="wrapped"] .codex-task-board-project-filter {
        flex: 1 1 auto;
        width: auto;
        max-width: none;
      }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="wrapped"] .codex-task-board-hint {
        display: block;
        flex: 1 1 180px;
        max-width: none;
      }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="collapsed"] .codex-task-board-create {
        min-width: 36px;
        padding: 0 9px;
      }
      [${taskBoardRootAttribute}="true"][data-toolbar-layout="collapsed"] .codex-task-board-create span {
        display: none;
      }
      .codex-task-board-search-control:focus-within,
      .codex-task-board-dropdown-trigger:focus-visible,
      .codex-task-board-conversation:focus-visible {
        outline: 2px solid #38bdf8;
        outline-offset: 2px;
      }
      .codex-task-board-create:hover {
        background: var(--task-board-action-background-hover);
      }
      .codex-task-board-create:active {
        background: var(--task-board-action-background-active);
      }
      .codex-task-board-create-modal-backdrop {
        position: fixed;
        z-index: 2147483300;
        inset: 0;
        display: grid;
        place-items: center;
        background: rgba(0, 0, 0, .58);
        backdrop-filter: blur(4px);
        padding: 16px;
      }
      .codex-task-board-create-modal {
        display: flex;
        height: min(650px, calc(100vh - 32px));
        width: 650px;
        flex-direction: column;
        max-width: calc(100vw - 32px);
        max-height: calc(100vh - 32px);
        overflow: hidden;
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        border-radius: 14px;
        background: color-mix(in srgb, var(--color-token-dropdown-background, #363636) 96%, transparent);
        color: var(--color-token-text-primary, #f4f4f5);
        box-shadow: 0 24px 70px rgba(0,0,0,.48);
      }
      .codex-task-board-create-modal-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
        padding: 20px 20px 15px;
        border-bottom: 1px solid color-mix(in srgb, currentColor 10%, transparent);
      }
      .codex-task-board-create-modal-head h2 {
        margin: 0;
        font-size: 18px;
        font-weight: 650;
      }
      .codex-task-board-create-modal-head p {
        margin: 5px 0 0;
        color: color-mix(in srgb, currentColor 52%, transparent);
        font-size: 11px;
      }
      .codex-task-board-create-close {
        display: inline-grid;
        width: 30px;
        height: 30px;
        flex: 0 0 auto;
        place-items: center;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: color-mix(in srgb, currentColor 52%, transparent);
        cursor: pointer;
        padding: 0;
      }
      .codex-task-board-create-close:hover {
        background: color-mix(in srgb, currentColor 7%, transparent);
        color: inherit;
      }
      .codex-task-board-create-fields {
        display: flex;
        flex: 1 1 auto;
        min-height: 0;
        flex-direction: column;
        gap: 14px;
        overflow: hidden;
        padding: 17px 20px 14px;
      }
      .codex-task-board-create-field {
        display: grid;
        gap: 7px;
      }
      .codex-task-board-create-mode-content {
        display: grid;
        flex: 1 1 auto;
        min-height: 0;
        overflow: hidden;
      }
      .codex-task-board-create-mode-panel {
        min-height: 0;
      }
      .codex-task-board-create-mode-panel[hidden] {
        display: none !important;
      }
      .codex-task-board-create-session-panel {
        display: flex;
        flex-direction: column;
        gap: 7px;
      }
      .codex-task-board-create-new-session {
        display: grid;
        grid-template-rows: minmax(0, 1fr);
      }
      .codex-task-board-create-instruction-field {
        min-height: 0;
        grid-template-rows: auto minmax(0, 1fr);
      }
      .codex-task-board-create-composer {
        position: relative;
        min-height: 0;
      }
      .codex-task-board-create-field-row {
        display: grid;
        grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
        gap: 12px;
      }
      .codex-task-board-create-field-label,
      .codex-task-board-create-picker-head {
        color: color-mix(in srgb, currentColor 72%, transparent);
        font-size: 11px;
        font-weight: 600;
      }
      .codex-task-board-create-input,
      .codex-task-board-create-select,
      .codex-task-board-create-textarea {
        width: 100%;
        min-height: 38px;
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        border-radius: 8px;
        outline: none;
        background: color-mix(
          in srgb,
          var(--color-background-secondary, var(--color-token-main-surface-secondary, #2b2b2b)) 86%,
          transparent
        );
        color: inherit;
        font: inherit;
        font-size: 12px;
        padding: 0 10px;
      }
      .codex-task-board-create-select {
        min-height: 36px;
      }
      .codex-task-board-create-textarea {
        height: 100%;
        min-height: 110px;
        resize: none;
        line-height: 1.5;
        padding: 10px 10px 44px;
      }
      .codex-task-board-create-model-trigger {
        position: absolute;
        right: 8px;
        bottom: 8px;
        z-index: 1;
        display: inline-flex;
        max-width: calc(100% - 16px);
        min-height: 30px;
        align-items: center;
        justify-content: center;
        gap: 6px;
        overflow: hidden;
        border: 0;
        border-radius: 7px;
        outline: none;
        background: transparent;
        color: color-mix(in srgb, currentColor 58%, transparent);
        cursor: pointer;
        font: 11px/1 system-ui, sans-serif;
        padding: 0 8px;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-create-model-trigger:hover,
      .codex-task-board-create-model-trigger[aria-expanded="true"] {
        background: color-mix(in srgb, currentColor 7%, transparent);
        color: color-mix(in srgb, currentColor 78%, transparent);
      }
      .codex-task-board-create-model-trigger:focus-visible {
        box-shadow: 0 0 0 2px color-mix(in srgb, #63aee0 34%, transparent);
        color: inherit;
      }
      .codex-task-board-create-model-trigger-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
      }
      .codex-task-board-create-effort-trigger-label {
        flex: 0 0 auto;
        color: color-mix(in srgb, currentColor 72%, transparent);
      }
      .codex-task-board-create-input::placeholder,
      .codex-task-board-create-textarea::placeholder {
        color: color-mix(in srgb, currentColor 46%, transparent);
      }
      .codex-task-board-create-input:focus,
      .codex-task-board-create-select:focus,
      .codex-task-board-create-textarea:focus {
        border-color: #63aee0;
        box-shadow: 0 0 0 2px color-mix(in srgb, #63aee0 20%, transparent);
      }
      .codex-task-board-create-mode-row {
        display: grid;
        grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
        overflow: hidden;
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        border-radius: 9px;
        background: color-mix(in srgb, var(--color-token-main-surface-secondary, #2b2b2b) 80%, transparent);
      }
      .codex-task-board-create-mode {
        display: inline-flex;
        min-width: 0;
        min-height: 42px;
        align-items: center;
        justify-content: flex-start;
        gap: 8px;
        margin: 0;
        padding: 0 12px;
        border: 0;
        outline: none;
        background: transparent;
        color: color-mix(in srgb, currentColor 48%, transparent);
        cursor: pointer;
        font: 12px/1 system-ui, sans-serif;
        text-align: left;
      }
      .codex-task-board-create-mode + .codex-task-board-create-mode {
        border-left: 1px solid color-mix(in srgb, currentColor 18%, transparent);
      }
      .codex-task-board-create-mode > svg {
        display: block;
        flex: 0 0 auto;
      }
      .codex-task-board-create-mode[aria-pressed="true"] {
        background: color-mix(in srgb, currentColor 7%, transparent);
        color: inherit;
      }
      .codex-task-board-create-picker-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
      }
      .codex-task-board-create-picker-count {
        color: color-mix(in srgb, currentColor 48%, transparent);
        font-size: 10px;
        font-weight: 400;
        font-variant-numeric: tabular-nums;
      }
      .codex-task-board-create-session-list {
        display: grid;
        flex: 1 1 auto;
        min-height: 0;
        align-content: start;
        grid-auto-rows: max-content;
        overflow-y: auto;
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        border-radius: 9px;
        background: color-mix(in srgb, var(--color-token-main-surface-secondary, #2b2b2b) 82%, transparent);
        scrollbar-gutter: stable;
      }
      .codex-task-board-create-session-list:has(.codex-task-board-create-session-hint) {
        align-content: stretch;
        grid-auto-rows: minmax(76px, 1fr);
      }
      .codex-task-board-create-session-option {
        display: grid !important;
        min-height: 52px;
        grid-template-columns: 18px 16px minmax(0, 1fr);
        align-items: center;
        gap: 8px;
        padding: 7px 10px;
        border-bottom: 1px solid color-mix(in srgb, currentColor 10%, transparent);
        color: color-mix(in srgb, currentColor 72%, transparent);
        cursor: pointer;
      }
      .codex-task-board-create-session-option:last-child {
        border-bottom: 0;
      }
      .codex-task-board-create-session-option:hover,
      .codex-task-board-create-session-option:has(input:checked) {
        background: color-mix(in srgb, currentColor 5%, transparent);
        color: inherit;
      }
      .codex-task-board-create-session-option input {
        width: 15px;
        height: 15px;
        margin: 0;
        accent-color: #63aee0;
        cursor: pointer;
      }
      .codex-task-board-create-session-icon {
        color: color-mix(in srgb, currentColor 48%, transparent);
      }
      .codex-task-board-create-session-copy {
        display: grid;
        min-width: 0;
        gap: 2px;
      }
      .codex-task-board-create-session-title {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 11px;
        line-height: 1.35;
      }
      .codex-task-board-create-session-time {
        min-width: 0;
        overflow: hidden;
        color: color-mix(in srgb, currentColor 44%, transparent);
        font-size: 10px;
        font-variant-numeric: tabular-nums;
        line-height: 1.3;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-create-session-hint,
      .codex-task-board-create-feedback {
        font-size: 10px;
        line-height: 1.4;
      }
      .codex-task-board-create-session-hint {
        display: grid;
        min-height: 76px;
        place-items: center;
        margin: 0;
        padding: 12px;
        color: color-mix(in srgb, currentColor 48%, transparent);
        text-align: center;
      }
      .codex-task-board-create-feedback {
        flex: 0 0 auto;
        margin: 0;
        color: #ff8582;
      }
      .codex-task-board-create-feedback:empty {
        display: none;
      }
      .codex-task-board-create-modal-footer {
        display: flex;
        flex: 0 0 auto;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        margin-top: 2px;
        padding: 14px 20px 18px;
        border-top: 1px solid color-mix(in srgb, currentColor 10%, transparent);
      }
      .codex-task-board-create-modal-footer-actions-only {
        justify-content: flex-end;
      }
      @media (max-height: 620px) {
        .codex-task-board-create-fields {
          overflow-y: auto;
        }
      }
      .codex-task-board-create-note {
        display: flex;
        min-width: 0;
        align-items: center;
        gap: 6px;
        color: color-mix(in srgb, currentColor 48%, transparent);
        font-size: 10px;
      }
      .codex-task-board-create-modal-actions {
        display: flex;
        flex: 0 0 auto;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
      }
      .codex-task-board-create-submit,
      .codex-task-board-create-cancel {
        display: inline-flex;
        min-height: 34px;
        align-items: center;
        justify-content: center;
        gap: 7px;
        border-radius: 8px;
        cursor: pointer;
        font: 12px/1 system-ui, sans-serif;
        font-weight: 600;
        padding: 0 13px;
      }
      .codex-task-board-create-submit {
        border: 1px solid var(--task-board-action-border);
        background: var(--task-board-action-background);
        color: var(--task-board-action-foreground);
      }
      .codex-task-board-create-submit:hover {
        background: var(--task-board-action-background-hover);
      }
      .codex-task-board-create-submit:active {
        background: var(--task-board-action-background-active);
      }
      .codex-task-board-create-cancel {
        border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
        background: transparent;
        color: color-mix(in srgb, currentColor 72%, transparent);
      }
      .codex-task-board-create-cancel:hover {
        background: color-mix(in srgb, currentColor 6%, transparent);
        color: inherit;
      }
      .codex-task-board-create-close:disabled,
      .codex-task-board-create-mode:disabled,
      .codex-task-board-create-model-trigger:disabled,
      .codex-task-board-create-submit:disabled,
      .codex-task-board-create-cancel:disabled {
        cursor: not-allowed;
        opacity: .45;
      }
      .codex-task-board-scroll {
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: auto;
        overscroll-behavior: contain;
        padding-bottom: 4px;
        scrollbar-color: rgba(148,163,184,.78) rgba(148,163,184,.12);
        scrollbar-width: thin;
      }
      .codex-task-board-scroll::-webkit-scrollbar {
        width: 12px;
        height: 12px;
      }
      .codex-task-board-scroll::-webkit-scrollbar-track {
        background: rgba(148,163,184,.12);
        border-radius: 999px;
      }
      .codex-task-board-scroll::-webkit-scrollbar-thumb {
        min-height: 38px;
        border: 3px solid transparent;
        border-radius: 999px;
        background: rgba(148,163,184,.78);
        background-clip: padding-box;
      }
      .codex-task-board-scroll::-webkit-scrollbar-corner {
        background: rgba(148,163,184,.12);
      }
      .codex-task-board-columns {
        display: grid;
        grid-template-columns: repeat(5, minmax(0, 1fr));
        gap: 12px;
        min-width: 1580px;
        min-height: 100%;
      }
      .codex-task-board-column {
        display: grid;
        min-width: 0;
        grid-template-rows: auto minmax(180px, 1fr);
        align-self: stretch;
        border: 1px solid var(--task-board-border-soft);
        border-radius: 10px;
        background: color-mix(in srgb, var(--task-board-panel-background) 78%, transparent);
        overflow: hidden;
      }
      .codex-task-board-column-head {
        display: flex;
        min-height: 46px;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 0 12px;
        border-bottom: 1px solid var(--task-board-border-soft);
      }
      .codex-task-board-column-title {
        display: inline-flex;
        min-width: 0;
        align-items: center;
        gap: 8px;
        color: var(--task-board-text-secondary);
        font-size: 12px;
        font-weight: 600;
      }
      .codex-task-board-status-dot {
        width: 7px;
        height: 7px;
        flex: 0 0 auto;
        border-radius: 999px;
        background: var(--task-board-status-color);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--task-board-status-color) 14%, transparent);
      }
      .codex-task-board-count {
        display: inline-flex;
        min-width: 21px;
        height: 21px;
        align-items: center;
        justify-content: center;
        padding: 0 6px;
        border-radius: 999px;
        background: color-mix(in srgb, currentColor 7%, transparent);
        color: var(--task-board-text-tertiary);
        font-size: 10px;
        font-variant-numeric: tabular-nums;
        line-height: 1;
        text-align: center;
      }
      .codex-task-board-card-list {
        display: flex;
        min-height: 180px;
        flex-direction: column;
        gap: 9px;
        padding: 9px;
        transition: background .15s ease, box-shadow .15s ease;
      }
      .codex-task-board-card {
        display: grid;
        box-sizing: border-box;
        min-width: 0;
        gap: 10px;
        border: 1px solid var(--task-board-border-soft);
        border-radius: 9px;
        background: var(--task-board-card-background);
        box-shadow: 0 1px 1px rgba(0,0,0,.12);
        padding: 11px;
        transition: border-color .15s ease, background .15s ease, transform .15s ease;
      }
      .codex-task-board-card[draggable="true"] { cursor: grab; }
      .codex-task-board-card[data-dragging="true"] { opacity: .48; }
      .codex-task-board-card:hover {
        border-color: var(--task-board-border);
        background: var(--task-board-card-background-hover);
        transform: translateY(-1px);
      }
      .codex-task-board-card-list[data-drop-active="true"] {
        background: color-mix(in srgb, var(--task-board-accent) 8%, transparent);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--task-board-accent) 50%, transparent);
      }
      .codex-task-board-dropdown-menu {
        position: fixed;
        z-index: 2147483300;
        box-sizing: border-box;
        display: grid;
        gap: 3px;
        max-height: min(340px, calc(100vh - 16px));
        overflow-y: auto;
        border: 1px solid var(
          --color-border-primary-outline,
          var(--color-token-border-primary, rgba(212, 212, 212, .16))
        );
        border-radius: 10px;
        background: var(
          --color-background-elevated-primary-opaque,
          var(--color-token-dropdown-background, #363636)
        );
        color: var(--color-text-primary, var(--color-token-text-primary, #f4f4f5));
        box-shadow: 0 14px 38px rgba(0,0,0,.42);
        color-scheme: dark;
        padding: 5px;
      }
      .codex-task-board-dropdown-menu button {
        display: flex;
        min-height: 34px;
        align-items: center;
        justify-content: space-between;
        gap: 14px;
        border: 0;
        border-radius: 6px;
        background: transparent;
        color: inherit;
        cursor: pointer;
        font: 12px/1.3 system-ui, sans-serif;
        padding: 6px 9px;
        text-align: left;
      }
      .codex-task-board-dropdown-option-copy {
        display: grid;
        flex: 1 1 auto;
        min-width: 0;
        align-content: center;
        gap: 2px;
      }
      .codex-task-board-dropdown-trigger-copy,
      .codex-task-board-dropdown-option-title-row {
        display: inline-flex;
        min-width: 0;
        align-items: center;
        gap: 7px;
      }
      .codex-task-board-dropdown-trigger-copy {
        flex: 1 1 auto;
      }
      .codex-task-board-dropdown-option-title-row {
        max-width: 100%;
      }
      .codex-task-board-dropdown-status-dot {
        width: 7px;
        height: 7px;
        flex: 0 0 auto;
        border-radius: 999px;
        background: var(--task-board-status-color);
        box-shadow: 0 0 0 2px color-mix(
          in srgb,
          var(--task-board-status-color) 14%,
          transparent
        );
      }
      .codex-task-board-dropdown-option-title,
      .codex-task-board-dropdown-option-description {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-dropdown-option-description {
        color: color-mix(in srgb, currentColor 48%, transparent);
        font-size: 10px;
        line-height: 1.35;
      }
      .codex-task-board-dropdown-option-marker {
        display: inline-flex;
        flex: 0 0 14px;
        min-height: 14px;
        align-items: center;
        justify-content: center;
        margin-top: 0;
      }
      .codex-task-board-dropdown-option-marker svg,
      .codex-task-board-create-settings-chevron svg {
        display: block;
      }
      .codex-task-board-dropdown-menu button:hover,
      .codex-task-board-dropdown-menu button:focus-visible {
        background: rgba(148,163,184,.18);
        outline: none;
      }
      .codex-task-board-dropdown-menu button[aria-selected="true"] {
        background: color-mix(
          in srgb,
          var(--color-text-accent, var(--color-token-text-accent, #63aee0)) 14%,
          transparent
        );
        color: var(--color-text-accent, var(--color-token-text-accent, #63aee0));
      }
      .codex-task-board-dropdown-menu button:disabled {
        cursor: not-allowed;
        opacity: .48;
      }
      .codex-task-board-create-settings-menu,
      .codex-task-board-create-model-menu,
      .codex-task-board-create-effort-menu {
        min-width: 220px;
        border-radius: 12px;
        padding: 5px;
      }
      .codex-task-board-create-settings-menu button,
      .codex-task-board-create-model-menu button,
      .codex-task-board-create-effort-menu button {
        min-height: 31px;
        align-items: center;
        border-radius: 7px;
        font-size: 12px;
        padding: 6px 9px;
      }
      .codex-task-board-create-settings-menu button {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto 14px;
        gap: 8px;
      }
      .codex-task-board-create-settings-label {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-create-settings-value {
        color: color-mix(in srgb, currentColor 52%, transparent);
        white-space: nowrap;
      }
      .codex-task-board-create-settings-chevron {
        display: inline-flex;
        width: 14px;
        align-items: center;
        justify-content: center;
        color: color-mix(in srgb, currentColor 52%, transparent);
      }
      .codex-task-board-create-model-menu button[aria-checked="true"],
      .codex-task-board-create-effort-menu button[aria-checked="true"] {
        background: transparent;
        color: inherit;
      }
      .codex-task-board-create-model-menu button[aria-checked="true"]:hover,
      .codex-task-board-create-model-menu button[aria-checked="true"]:focus-visible,
      .codex-task-board-create-effort-menu button[aria-checked="true"]:hover,
      .codex-task-board-create-effort-menu button[aria-checked="true"]:focus-visible {
        background: rgba(148,163,184,.18);
      }
      .codex-task-board-card-move {
        flex: 0 0 auto;
        width: 92px;
        min-height: 29px;
        border: 1px solid var(--task-board-border);
        border-radius: 7px;
        background: color-mix(in srgb, var(--task-board-card-background) 82%, currentColor 4%);
        color: inherit;
        font: 11px/1.3 system-ui, sans-serif;
        padding: 0 8px 0 9px;
      }
      .codex-task-board-card-move:hover {
        background: var(--task-board-card-background-hover);
      }
      .codex-task-board-project {
        min-width: 0;
        overflow: hidden;
        color: var(--task-board-text-tertiary);
        font-size: 10px;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-card-title {
        display: -webkit-box;
        margin: 0;
        overflow: hidden;
        color: inherit;
        font-size: 13px;
        font-weight: 600;
        line-height: 1.45;
        -webkit-box-orient: vertical;
        -webkit-line-clamp: 2;
      }
      .codex-task-board-card-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        min-width: 0;
      }
      .codex-task-board-card-add {
        display: inline-flex;
        min-height: 29px;
        align-items: center;
        justify-content: center;
        gap: 5px;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: var(--task-board-text-tertiary);
        cursor: pointer;
        font: 10px/1.3 system-ui, sans-serif;
        padding: 0 7px;
      }
      .codex-task-board-card-add:hover {
        background: color-mix(in srgb, currentColor 8%, transparent);
        color: inherit;
      }
      .codex-task-board-card-add:disabled {
        cursor: not-allowed;
        opacity: .52;
      }
      .codex-task-board-conversations {
        display: grid;
        align-items: stretch;
        gap: 3px;
        flex: 1 1 auto;
        min-width: 0;
      }
      .codex-task-board-conversation-row {
        display: flex;
        align-items: center;
        gap: 3px;
        flex: 1 1 auto;
        width: 100%;
        min-width: 0;
        border-radius: 7px;
        padding: 2px 0 2px 4px;
        transition: background .15s ease;
      }
      .codex-task-board-conversation-row:hover {
        background: color-mix(in srgb, currentColor 7%, transparent);
      }
      .codex-task-board-conversation {
        display: flex;
        align-items: center;
        gap: 7px;
        flex: 1 1 auto;
        width: auto;
        min-width: 0;
        min-height: 24px;
        border: 0;
        border-radius: 7px;
        background: transparent;
        color: var(--task-board-text-tertiary);
        cursor: pointer;
        font: 10px/1.3 system-ui, sans-serif;
        padding: 0 4px 0 0;
        text-align: left;
      }
      .codex-task-board-conversation:hover {
        color: inherit;
      }
      .codex-task-board-conversation:disabled {
        cursor: not-allowed;
        opacity: .52;
      }
      .codex-task-board-conversation-remove {
        display: inline-flex;
        width: 24px;
        height: 24px;
        align-items: center;
        justify-content: center;
        flex: 0 0 auto;
        border: 0;
        border-radius: 6px;
        background: transparent;
        color: color-mix(in srgb, var(--task-board-text-tertiary) 72%, transparent);
        cursor: pointer;
        padding: 0;
      }
      .codex-task-board-conversation-remove:hover,
      .codex-task-board-conversation-remove:focus-visible {
        background: color-mix(in srgb, #ef4444 12%, transparent);
        color: #ef6b6b;
        outline: none;
      }
      .codex-task-board-conversation-remove:disabled {
        cursor: wait;
        opacity: .52;
      }
      .codex-task-board-conversation-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: 0 0 auto;
        width: 14px;
        height: 14px;
        color: color-mix(in srgb, currentColor 64%, transparent);
      }
      .codex-task-board-conversation-title {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .codex-task-board-conversation-state {
        display: inline-flex;
        min-height: 18px;
        align-items: center;
        gap: 4px;
        flex: 0 0 auto;
        margin-left: auto;
        border-radius: 999px;
        background: color-mix(in srgb, currentColor 7%, transparent);
        color: color-mix(in srgb, currentColor 58%, transparent);
        font-size: 9px;
        line-height: 1;
        padding: 0 6px;
        white-space: nowrap;
      }
      .codex-task-board-conversation-status-indicator {
        box-sizing: border-box;
        width: 6px;
        height: 6px;
        flex: 0 0 auto;
        border-radius: 999px;
        background: currentColor;
      }
      .codex-task-board-conversation-state[data-conversation-status="running"] {
        background: color-mix(in srgb, var(--task-board-accent) 12%, transparent);
        color: var(--task-board-accent);
      }
      .codex-task-board-conversation-state[data-conversation-status="running"]
      .codex-task-board-conversation-status-indicator {
        width: 8px;
        height: 8px;
        border: 1.5px solid currentColor;
        border-right-color: transparent;
        background: transparent;
        animation: codex-task-board-status-spin .8s linear infinite;
      }
      .codex-task-board-conversation-state[data-conversation-status="completed-unread"] {
        background: color-mix(in srgb, var(--task-board-accent) 12%, transparent);
        color: var(--task-board-accent);
      }
      .codex-task-board-conversation-state[data-conversation-status="completed-unread"]
      .codex-task-board-conversation-status-indicator {
        box-shadow: 0 0 0 2px color-mix(in srgb, currentColor 18%, transparent);
      }
      .codex-task-board-conversation-state[data-conversation-status="completed"]
      .codex-task-board-conversation-status-indicator {
        background: #72a987;
      }
      .codex-task-board-conversation-state[data-conversation-status="checking"]
      .codex-task-board-conversation-status-indicator,
      .codex-task-board-conversation-state[data-conversation-status="unknown"]
      .codex-task-board-conversation-status-indicator {
        border: 1px solid currentColor;
        background: transparent;
      }
      .codex-task-board-conversation-state[data-conversation-status="unavailable"] {
        color: #d59a91;
      }
      .codex-task-board-conversations .codex-task-board-empty {
        padding: 0;
        text-align: left;
      }
      .codex-task-board-detach-feedback {
        min-height: 18px;
        margin-top: 10px;
        color: #dc2626;
        font-size: 12px;
        line-height: 1.45;
      }
      @keyframes codex-task-board-status-spin {
        to { transform: rotate(360deg); }
      }
      .codex-task-board-empty-column {
        display: grid;
        min-height: 110px;
        place-items: center;
        color: color-mix(in srgb, currentColor 48%, transparent);
        font-size: 11px;
        text-align: center;
      }
      .codex-task-board-empty {
        padding: 18px 10px;
        color: color-mix(in srgb, currentColor 54%, transparent);
        font-size: 12px;
        text-align: center;
      }
      @container (max-width: 860px) {
        .codex-task-board-page { gap: 12px; padding: 18px 20px; }
        .codex-task-board-toolbar { flex-wrap: wrap; }
        .codex-task-board-search-control { flex: 1 1 100%; width: 100%; }
        .codex-task-board-project-filter { flex: 1 1 auto; width: auto; max-width: none; }
        .codex-task-board-hint { display: block; flex: 1 1 180px; max-width: none; }
        .codex-task-board-columns { min-width: 1450px; }
      }
      @container (max-width: 540px) {
        .codex-task-board-page { padding: 14px 12px; }
        .codex-task-board-heading h1 { font-size: 20px; }
        .codex-task-board-description { font-size: 12px; }
        .codex-task-board-project-filter { max-width: none; }
        .codex-task-board-columns { min-width: 1320px; gap: 8px; }
        .codex-task-board-column-head { padding: 10px; }
        .codex-task-board-card-list { gap: 8px; padding: 8px; }
        .codex-task-board-card { padding: 10px; }
        .codex-task-board-create { min-width: 36px; padding: 0 9px; }
        .codex-task-board-create span { display: none; }
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-page {
        gap: 8px;
        padding-block: 10px;
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-description,
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-hint {
        font-size: 12px;
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-description {
        margin-top: 0;
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-column-head {
        padding-block: 8px;
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-card-list {
        gap: 7px;
        padding: 7px;
      }
      [${taskBoardRootAttribute}="true"][data-low-height="true"] .codex-task-board-card {
        padding: 9px;
      }
      @media (prefers-reduced-motion: reduce) {
        .codex-task-board-card,
        .codex-task-board-card-list,
        .codex-task-board-conversation { transition: none !important; }
        .codex-task-board-card:hover { transform: none; }
        .codex-task-board-conversation-state[data-conversation-status="running"]
        .codex-task-board-conversation-status-indicator { animation: none !important; }
      }
    `;
    document.documentElement.appendChild(style);
  }

  function defaultCodexElvesSettings() {
    return {
      pluginEntryUnlock: true,
      pluginMarketplaceUnlock: true,
      taskBoard: true,
      sessionDelete: true,
      markdownExport: true,
      projectMove: true,
      conversationView: false,
      tokenUsage: false,
      conversationViewMaxWidth: conversationViewDefaultWidth,
      upstreamWorktreeCreate: true,
      nativeMenuPlacement: true,
      serviceTierControls: false,
    };
  }

  const codexElvesBackendSettingMap = {
    pluginEntryUnlock: "codexAppPluginEntryUnlock",
    pluginMarketplaceUnlock: "codexAppPluginMarketplaceUnlock",
    taskBoard: "codexAppTaskBoard",
    sessionDelete: "codexAppSessionDelete",
    markdownExport: "codexAppMarkdownExport",
    projectMove: "codexAppProjectMove",
    conversationView: "codexAppConversationView",
    tokenUsage: "codexAppTokenUsage",

    upstreamWorktreeCreate: "codexAppUpstreamWorktreeCreate",
    nativeMenuPlacement: "codexAppNativeMenuPlacement",
    serviceTierControls: "codexAppServiceTierControls",
  };

  function backendCodexElvesSettings() {
    const settings = {};
    Object.entries(codexElvesBackendSettingMap).forEach(([localKey, backendKey]) => {
      const value = codexElvesBackendSettings[backendKey];
      if (typeof value === "boolean" || (typeof value === "number" && Number.isFinite(value))) settings[localKey] = value;
    });
    return settings;
  }

  function invalidateCodexElvesSettingsCache() {
    codexElvesSettingsCache = null;
  }

  function disabledCodexElvesSettings() {
    return {
      pluginEntryUnlock: false,
      pluginMarketplaceUnlock: false,
      taskBoard: false,
      sessionDelete: false,
      markdownExport: false,
      projectMove: false,
      conversationView: false,
      tokenUsage: false,
      conversationViewMaxWidth: conversationViewDefaultWidth,
      upstreamWorktreeCreate: false,
      nativeMenuPlacement: false,
      serviceTierControls: false,
    };
  }

  let codexElvesSettingsCache = null;
  let codexThreadServiceTierStateCache = null;

  function codexElvesSettings() {
    if (codexElvesSettingsCache) return codexElvesSettingsCache;
    const relayPatchDisabled = codexElvesBackendSettings.launchMode === "relay";
    if (codexElvesBackendSettings.enhancementsEnabled === false) {
      codexElvesSettingsCache = disabledCodexElvesSettings();
      return codexElvesSettingsCache;
    }
    try {
      const settings = { ...defaultCodexElvesSettings(), ...JSON.parse(localStorage.getItem(codexElvesSettingsKey) || "{}"), ...backendCodexElvesSettings() };
      if (relayPatchDisabled) {
        settings.pluginEntryUnlock = false;
        settings.pluginMarketplaceUnlock = false;
      }
      codexElvesSettingsCache = settings;
    } catch {
      const settings = { ...defaultCodexElvesSettings(), ...backendCodexElvesSettings() };
      if (relayPatchDisabled) {
        settings.pluginEntryUnlock = false;
        settings.pluginMarketplaceUnlock = false;
      }
      codexElvesSettingsCache = settings;
    }
    return codexElvesSettingsCache;
  }

  function setCodexElvesSetting(key, value) {
    const backendKey = codexElvesBackendSettingMap[key];
    if (backendKey) {
      setBackendSetting(backendKey, value);
      return;
    }
    let stored = {};
    try {
      stored = JSON.parse(localStorage.getItem(codexElvesSettingsKey) || "{}");
    } catch {
      stored = {};
    }
    const next = { ...stored, [key]: value };
    localStorage.setItem(codexElvesSettingsKey, JSON.stringify(next));
    invalidateCodexElvesSettingsCache();
    if (key === "serviceTierControls") {
      if (value) {
        void loadCodexServiceTierState();
      } else {
        removeCodexServiceTierBadges();
        refreshCodexServiceTierControls();
      }
    }
    renderCodexElvesMenu();
    scan(scanDirtyForSetting(key));
  }

  function scanDirtyForSetting(key) {
    const dirty = emptyScanDirty();
    if (["pluginEntryUnlock", "pluginMarketplaceUnlock"].includes(key)) {
      dirty.plugins = true;
      return dirty;
    }
    if (key === "taskBoard") {
      dirty.sidebar = true;
      return dirty;
    }
    if ([
      "sessionDelete",
      "markdownExport",
      "projectMove",
    ].includes(key)) {
      dirty.sidebar = true;
      return dirty;
    }
    if (key === "conversationView" || key === "conversationViewMaxWidth") {
      dirty.conversation = true;
      return dirty;
    }
    if (key === "tokenUsage" || key === "serviceTierControls") {
      dirty.header = true;
      dirty.conversation = true;
      return dirty;
    }
    if (key === "nativeMenuPlacement") {
      dirty.header = true;
      return dirty;
    }
    if (key === "upstreamWorktreeCreate") {
      dirty.conversation = true;
      return dirty;
    }
    return {
      sidebar: true,
      conversation: true,
      header: true,
      plugins: true,
      shell: false,
    };
  }

  function normalizeConversationViewWidth(value) {
    if (value === null || value === undefined || String(value).trim() === "") return null;
    const number = Number(value);
    if (!Number.isFinite(number)) return null;
    return Math.max(conversationViewMinWidth, Math.min(conversationViewMaxAllowedWidth, Math.round(number)));
  }

  function conversationViewWidth() {
    const settingsWidth = normalizeConversationViewWidth(codexElvesSettings().conversationViewMaxWidth);
    if (settingsWidth) return settingsWidth;
    const legacyWidth = normalizeConversationViewWidth(localStorage.getItem(conversationViewLegacyWidthKey));
    return legacyWidth || conversationViewDefaultWidth;
  }

  function refreshConversationViewControls() {
    const enabled = !!codexElvesSettings().conversationView;
    const width = conversationViewWidth();
    document.querySelectorAll("[data-codex-elves-conversation-view-width]").forEach((input) => {
      input.value = String(width);
      input.disabled = !enabled;
    });
  }

  function setConversationViewWidth(value) {
    const width = normalizeConversationViewWidth(value);
    if (!width) return;
    setCodexElvesSetting("conversationViewMaxWidth", width);
  }

  function renderCodexElvesMenu() {
    document.querySelectorAll(".codex-elves-toggle[data-codex-elves-setting]").forEach((button) => {
      const key = button.getAttribute("data-codex-elves-setting");
      button.dataset.enabled = String(!!codexElvesSettings()[key]);
    });
    refreshConversationViewControls();
    refreshCodexServiceTierControls();
  }

  let codexElvesBackendSettings = { providerSyncEnabled: false, enhancementsEnabled: true, launchMode: "patch", codexAppVersion: "" };
  const codexPluginLegacyEntryUnlockBeforeVersion = "26.601.2237";

  function parseCodexVersionParts(version) {
    const raw = String(version || "").trim();
    if (!raw) return null;
    const match = raw.match(/\d+(?:\.\d+)*/);
    if (!match) return null;
    const parts = match[0].split(".").map((part) => Number(part));
    if (!parts.length || parts.some((part) => !Number.isInteger(part) || part < 0)) return null;
    return parts;
  }

  function compareCodexVersions(left, right) {
    const leftParts = parseCodexVersionParts(left);
    const rightParts = parseCodexVersionParts(right);
    if (!leftParts || !rightParts) return null;
    const length = Math.max(leftParts.length, rightParts.length);
    for (let index = 0; index < length; index += 1) {
      const leftPart = leftParts[index] || 0;
      const rightPart = rightParts[index] || 0;
      if (leftPart !== rightPart) return leftPart < rightPart ? -1 : 1;
    }
    return 0;
  }

  function codexPluginUnlockStrategy() {
    const version = String(codexElvesBackendSettings.codexAppVersion || "").trim();
    const comparison = compareCodexVersions(version, codexPluginLegacyEntryUnlockBeforeVersion);
    if (comparison == null) return "unknown";
    return comparison < 0 ? "legacy" : "modern";
  }

  function logCodexPluginUnlockStrategy(strategy) {
    const codexAppVersion = String(codexElvesBackendSettings.codexAppVersion || "").trim();
    const signature = `${strategy}:${codexAppVersion || "unknown"}`;
    if (window.__codexPluginUnlockStrategyLogged === signature) return;
    window.__codexPluginUnlockStrategyLogged = signature;
    sendCodexElvesDiagnostic("plugin_unlock_strategy_selected", {
      strategy,
      codexAppVersion,
      cutoff: codexPluginLegacyEntryUnlockBeforeVersion,
    });
  }

  function codexPluginMarketplaceRequestPatchStrategy() {
    const pluginStrategy = codexPluginUnlockStrategy();
    if (pluginStrategy === "legacy") return "none";
    return "client";
  }

  let codexElvesBackendSettingsLoaded = false;
  let codexServiceTierState = {
    status: "loading",
    serviceTier: null,
    message: "正在读取…",
    fastTierValue: "priority",
    controlMode: "inherit",
    defaultMode: "inherit",
    activeThreadId: "",
    threadMode: "inherit",
    effectiveServiceTier: null,
    effectiveMode: "standard",
    fastModelName: "",
    fastSupported: false,
  };
  const codexDefaultServiceTierSetting = { key: "default-service-tier", default: null };
  const codexServiceTierFallbackFastValue = "priority";
  const codexServiceTierModulePromises = new Map();
  let codexAppModuleLoaderForTest = null;
  const codexServiceTierModernModulePart = "app-initial-";
  const codexServiceTierSettingModuleParts = ["setting-storage-", codexServiceTierModernModulePart];
  const codexServiceTierRequestClientModuleParts = ["thread-context-inputs-", codexServiceTierModernModulePart];
  const codexServiceTierSupportedFastModels = new Set([
    "gpt-5.4",
    "gpt-5.5",
    "gpt-5.6",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
  ]);
  const codexServiceTierSupportedFastModelPrefixes = [
    "gpt-5.6-sol-",
    "gpt-5.6-terra-",
    "gpt-5.6-luna-",
  ];
  const codexThreadServiceTierModes = new Set(["inherit", "standard", "fast"]);
  const codexServiceTierControlModes = new Set(["inherit", "global-standard", "global-fast", "custom"]);

  function codexAppAssetUrl(namePart) {
    const urls = [
      ...Array.from(document.scripts || []).map((script) => script.src),
      ...Array.from(document.querySelectorAll("link[href]") || []).map((link) => link.href),
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
    ].filter(Boolean);
    return urls.find((url) => url.includes("/assets/") && url.includes(namePart) && url.split("?")[0].endsWith(".js")) || "";
  }

  async function codexAppAssetUrlFromScriptText(namePart) {
    const scripts = Array.from(document.scripts || []).map((script) => script.src).filter(Boolean);
    for (const src of scripts) {
      if (!src.includes("/assets/") || !src.split("?")[0].endsWith(".js")) continue;
      try {
        const text = await fetch(src).then((response) => response.ok ? response.text() : "");
        const escaped = namePart.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        const match = text.match(new RegExp(`["'](\\./assets/${escaped}[^"']+\\.js)["']`));
        if (!match) continue;
        return new URL(match[1], src).href;
      } catch {
      }
    }
    return "";
  }

  async function loadCodexAppModule(namePart) {
    if (typeof codexAppModuleLoaderForTest === "function") {
      return await codexAppModuleLoaderForTest(namePart);
    }
    if (!codexServiceTierModulePromises.has(namePart)) {
      const promise = Promise.resolve().then(async () => {
        const url = codexAppAssetUrl(namePart) || await codexAppAssetUrlFromScriptText(namePart);
        if (!url) throw new Error(`未找到 ChatGPT/Codex 桌面应用 asset: ${namePart}`);
        return await import(url);
      }).catch((error) => {
        codexServiceTierModulePromises.delete(namePart);
        throw error;
      });
      codexServiceTierModulePromises.set(namePart, promise);
    }
    return await codexServiceTierModulePromises.get(namePart);
  }

  // Codex App 升级后会重排 chunk：dispatcher 的归属模块与导出形态都可能变化。
  // 历史版本导出 getInstance 类；新版 app-initial 直接导出带 handlers/dispatchMessage 的对象。
  const codexServiceTierDispatcherModuleParts = ["vscode-api-", "setting-storage-", codexServiceTierModernModulePart];
  let codexServiceTierDispatcher = null;
  let codexServiceTierNativeThreadSyncKey = "";

  function codexServiceTierRequestClientClassFromModule(module) {
    if (!module || typeof module !== "object") return null;
    for (const value of Object.values(module)) {
      if (typeof value !== "function") continue;
      const prototype = value.prototype;
      if (!prototype || typeof prototype !== "object") continue;
      if (
        typeof prototype.createRequest === "function" &&
        typeof prototype.sendRequest === "function" &&
        typeof prototype.prewarmThreadStart === "function"
      ) {
        return value;
      }
    }
    return null;
  }

  function patchCodexServiceTierRequestClientPrototype(requestClientClass) {
    const prototype = requestClientClass?.prototype;
    if (!prototype || typeof prototype.createRequest !== "function") return false;
    if (prototype.__codexServiceTierOriginalCreateRequest) return true;
    prototype.__codexServiceTierOriginalCreateRequest = prototype.createRequest;
    prototype.createRequest = function codexServiceTierPatchedCreateRequest(method, params, options) {
      const methodName = String(method || "");
      const nextParams = applyCodexServiceTierRequestOverride(methodName, params);
      return prototype.__codexServiceTierOriginalCreateRequest.call(this, method, nextParams, options);
    };
    return true;
  }

  function codexServiceTierDispatcherFromModule(module) {
    if (!module || typeof module !== "object") return null;
    for (const value of Object.values(module)) {
      if (!value || (typeof value !== "object" && typeof value !== "function")) continue;
      try {
        if (
          value.handlers instanceof Map &&
          typeof value.dispatchMessage === "function" &&
          typeof value.handleMessage === "function"
        ) {
          return value;
        }
      } catch {
      }
    }
    for (const value of Object.values(module)) {
      if (typeof value !== "function" || typeof value.getInstance !== "function") continue;
      let source = "";
      try {
        source = String(value);
      } catch {
        continue;
      }
      if (!source.includes("dispatchMessage")) continue;
      let instance = null;
      try {
        instance = value.getInstance();
      } catch {
        continue;
      }
      if (instance && typeof instance.dispatchMessage === "function") return instance;
    }
    return null;
  }

  async function findCodexServiceTierDispatcher() {
    let lastError = null;
    for (const namePart of codexServiceTierDispatcherModuleParts) {
      let module;
      try {
        module = await loadCodexAppModule(namePart);
      } catch (error) {
        lastError = error;
        continue;
      }
      const dispatcher = codexServiceTierDispatcherFromModule(module);
      if (dispatcher) return dispatcher;
    }
    if (lastError) throw lastError;
    return null;
  }

  function syncCodexNativeThreadServiceTier(threadId, serviceTier, source = "state") {
    const key = validThreadSessionKey(threadId);
    if (!key || !codexServiceTierDispatcher || typeof codexServiceTierDispatcher.dispatchMessage !== "function") return;
    const normalizedServiceTier = serviceTier || null;
    const syncKey = `${key}:${normalizedServiceTier || "default"}:${source}`;
    if (codexServiceTierNativeThreadSyncKey === syncKey) return;
    try {
      codexServiceTierNativeThreadSyncKey = syncKey;
      codexServiceTierDispatcher.dispatchMessage("update-thread-settings-for-next-turn", {
        conversationId: key,
        threadSettings: { serviceTier: normalizedServiceTier },
      });
      sendCodexElvesDiagnostic("service_tier_native_thread_setting_synced", {
        threadId: key,
        serviceTier: normalizedServiceTier || "standard",
        source,
      });
    } catch (error) {
      codexServiceTierNativeThreadSyncKey = "";
      sendCodexElvesDiagnostic("service_tier_native_thread_setting_sync_failed", {
        threadId: key,
        serviceTier: normalizedServiceTier || "standard",
        source,
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
    }
  }

  function codexServiceTierSettingReaderFromModule(module) {
    if (!module || typeof module !== "object") return null;
    if (typeof module.n === "function" && typeof module.s === "function") return module.n;
    for (const value of Object.values(module)) {
      if (typeof value !== "function") continue;
      let source = "";
      try {
        source = String(value);
      } catch {
        continue;
      }
      if (
        source.includes("get-setting") &&
        source.includes("key") &&
        source.includes(".value")
      ) {
        return value;
      }
    }
    return null;
  }

  async function codexServiceTierSettingReader() {
    let lastError = null;
    for (const namePart of codexServiceTierSettingModuleParts) {
      try {
        const module = await loadCodexAppModule(namePart);
        const reader = codexServiceTierSettingReaderFromModule(module);
        if (reader) return reader;
        lastError = new Error(`Codex 设置读取接口不可用: ${namePart}`);
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError || new Error("Codex 设置读取接口不可用");
  }

  async function getCodexServiceTierSetting() {
    try {
      const readSetting = await codexServiceTierSettingReader();
      return await readSetting(codexDefaultServiceTierSetting);
    } catch (error) {
      if (typeof codexStateCall === "function") {
        try {
          const result = await codexStateCall("get-setting", { params: { key: codexDefaultServiceTierSetting.key } });
          return result && Object.prototype.hasOwnProperty.call(result, "value") ? result.value : codexDefaultServiceTierSetting.default;
        } catch {
        }
      }
      throw error;
    }
  }

  function isFastServiceTierValue(value) {
    const normalized = String(value || "").trim().toLowerCase();
    return normalized === "fast" || normalized === "priority";
  }

  function codexFastServiceTierValue() {
    return codexServiceTierState.fastTierValue || codexServiceTierFallbackFastValue;
  }

  function codexServiceTierFastModelListLabel() {
    return "gpt-5.4+";
  }

  function normalizeCodexServiceTierModelName(model) {
    return String(model || "").trim().toLowerCase();
  }

  function codexServiceTierBuiltInFastSupported(modelName) {
    const normalized = normalizeCodexServiceTierModelName(modelName);
    const model = normalized.split("/").filter(Boolean).pop() || normalized;
    return codexServiceTierSupportedFastModels.has(model)
      || codexServiceTierSupportedFastModelPrefixes.some((prefix) => model.startsWith(prefix));
  }

  function codexServiceTierModelFromValue(value, visited = new WeakSet(), depth = 0) {
    if (typeof value === "string") return value.trim();
    if (!value || typeof value !== "object" || visited.has(value) || depth > 3) return "";
    visited.add(value);
    for (const key of ["model", "modelId", "model_id", "selectedModel", "selected_model", "defaultModel", "default_model"]) {
      const model = codexServiceTierModelFromValue(value[key], visited, depth + 1);
      if (model) return model;
    }
    for (const key of ["params", "request", "payload", "body", "config", "options"]) {
      const model = codexServiceTierModelFromValue(value[key], visited, depth + 1);
      if (model) return model;
    }
    return "";
  }

  // 规范化模型名/文本用于匹配：保留 Unicode 字母和数字，支持中文等展示别名。
  function codexServiceTierModelMatchKey(value) {
    return String(value || "")
      .normalize("NFKC")
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, "");
  }

  // slug 去掉常见厂商前缀后的“核心版本片段”（如 gpt-5.5 -> 5.5），用于与 UI 简写文本匹配
  function codexServiceTierModelCoreFragment(slug) {
    const lower = String(slug || "").toLowerCase().trim();
    if (!lower) return "";
    const stripped = lower.replace(/^(gpt|gpt-|o|claude|claude-|gemini|gemini-|deepseek|deepseek-|qwen|qwen-|kimi|moonshot|mistral|llama)[-_]?/, "");
    const frag = stripped.replace(/[^a-z0-9.]+/g, "");
    return frag;
  }

  // 判断某个 catalog slug 是否与 composer 按钮文本匹配。优先精确/包含，其次版本号片段。
  function codexServiceTierModelMatchesText(slug, text) {
    const slugKey = codexServiceTierModelMatchKey(slug);
    const textKey = codexServiceTierModelMatchKey(text);
    if (!slugKey || !textKey) return false;
    if (textKey === slugKey) return true;
    if (textKey.includes(slugKey) && slugKey.length >= 3) return true;
    const frag = codexServiceTierModelCoreFragment(slug);
    const fragText = String(text || "").toLowerCase().replace(/[^a-z0-9.]+/g, "");
    if (frag && frag.length >= 3 && /[0-9]/.test(frag)) {
      if (fragText.includes(frag)) return true;
    }
    return false;
  }

  // 从 composer footer 读取用户当前实际选中的模型，并匹配到 catalog 的 slug。
  // 解决：fast 能力判断不应使用后端配置的默认模型，而应使用会话里实际选中的模型。
  function codexServiceTierComposerSelectedModel() {
    try {
      const entries = codexServiceTierCatalogEntries();
      if (!entries.length) return "";
      if (typeof codexServiceTierBestComposerFooter !== "function") return "";
      const footer = codexServiceTierBestComposerFooter();
      if (!footer) return "";
      const buttons = Array.from(footer.querySelectorAll("button, [role='button']"));
      const modelButtons = buttons.filter((button) =>
        button.matches?.('[data-codex-intelligence-trigger="true"], [data-composer-navigation-target="reasoning"]')
      );
      const texts = (modelButtons.length ? modelButtons : buttons)
        .flatMap(codexServiceTierSelectedModelTexts)
        .filter(Boolean);
      for (const text of texts) {
        const match = codexServiceTierCatalogModelMatch(text, false, entries);
        if (match.slug) return match.slug;
        if (match.ambiguous) return null;
      }
      for (const text of texts) {
        const match = codexServiceTierCatalogModelMatch(text, true, entries);
        if (match.slug) return match.slug;
        if (match.ambiguous) return null;
      }
    } catch (error) {
      void error;
    }
    return "";
  }

  function codexServiceTierCatalogEntries() {
    const entries = [];
    const seen = new Set();
    const modelEntries = Array.isArray(codexModelCatalog.model_entries) ? codexModelCatalog.model_entries : [];
    for (const entry of modelEntries) {
      const slug = String(entry?.slug || "").trim();
      if (!slug || seen.has(slug)) continue;
      seen.add(slug);
      entries.push({
        slug,
        displayName: String(entry?.display_name || "").trim(),
        defaultReasoningEffort: String(
          entry?.default_reasoning_level ||
          entry?.defaultReasoningEffort ||
          "",
        ).trim(),
        supportedReasoningEfforts: (
          Array.isArray(entry?.supported_reasoning_levels)
            ? entry.supported_reasoning_levels
            : Array.isArray(entry?.supportedReasoningEfforts)
              ? entry.supportedReasoningEfforts
              : []
        )
          .map((level) => String(
            level?.effort ||
            level?.reasoningEffort ||
            level ||
            "",
          ).trim())
          .filter(Boolean),
      });
    }
    if (Array.isArray(codexModelCatalog.models)) {
      for (const model of codexModelCatalog.models) {
        const slug = String(model || "").trim();
        if (!slug || seen.has(slug)) continue;
        seen.add(slug);
        entries.push({
          slug,
          displayName: "",
          defaultReasoningEffort: "",
          supportedReasoningEfforts: [],
        });
      }
    }
    return entries;
  }

  function codexServiceTierUniqueCatalogMatch(entries, predicate) {
    const slugs = new Set(
      entries
        .filter(predicate)
        .map((entry) => entry.slug)
        .filter(Boolean)
    );
    if (slugs.size === 1) return { slug: Array.from(slugs)[0], ambiguous: false };
    return { slug: "", ambiguous: slugs.size > 1 };
  }

  // slug 和 display_name 的任一交叉冲突都拒绝猜测，避免可见别名解析到另一模型。
  function codexServiceTierCatalogModelMatch(value, allowPartial = false, entries = codexServiceTierCatalogEntries()) {
    const key = codexServiceTierModelMatchKey(value);
    if (!key || !entries.length) return { slug: "", ambiguous: false };

    let match = codexServiceTierUniqueCatalogMatch(
      entries,
      (entry) => codexServiceTierModelMatchKey(entry.slug) === key
        || (entry.displayName && codexServiceTierModelMatchKey(entry.displayName) === key)
    );
    if (match.slug || match.ambiguous || !allowPartial) return match;

    return codexServiceTierUniqueCatalogMatch(
      entries,
      (entry) => codexServiceTierModelMatchesText(entry.slug, value)
        || (entry.displayName && codexServiceTierModelMatchesText(entry.displayName, value))
    );
  }

  function codexServiceTierSelectedModelTexts(button) {
    if (!(button instanceof HTMLElement)) return [];
    const selectors = [
      '[class*="_WorkTriggerModelText_"]',
      '[class*="_WorkTriggerModelLabel_"]',
      '[data-tooltip-overflow-target="true"]',
    ];
    const visibleTexts = uniqueValues(selectors.flatMap((selector) =>
      Array.from(button.querySelectorAll(selector))
        .filter((node) => !node.closest?.('[aria-hidden="true"]'))
        .filter(codexServiceTierBadgeVisibleElement)
        .map(codexServiceTierBadgeText)
    ));
    if (visibleTexts.length) return visibleTexts;
    const fallback = button.cloneNode(true);
    fallback.querySelectorAll?.('[aria-hidden="true"]').forEach((node) => node.remove());
    const fallbackText = String(fallback.textContent || "").replace(/\s+/g, " ").trim();
    return fallbackText ? [fallbackText] : [];
  }

  // 将匹配到的 slug/display_name 归一回 catalog 真实 slug
  function findCatalogSlug(value) {
    return codexServiceTierCatalogModelMatch(value).slug;
  }

  function codexServiceTierCurrentModelName() {
    // 优先使用会话 composer 中实际选中的模型；回退到后端配置的激活/默认模型
    const selectedModel = codexServiceTierComposerSelectedModel();
    if (selectedModel === null) return "";
    return selectedModel
      || codexServiceTierModelFromValue(codexModelCatalog.model)
      || codexServiceTierModelFromValue(codexModelCatalog.default_model);
  }

  function codexServiceTierModelForRequest(params, modelHint = "") {
    return codexServiceTierModelFromValue(params) || codexServiceTierModelFromValue(modelHint) || codexServiceTierCurrentModelName();
  }

  function codexServiceTierFastSupportedForModel(modelName) {
    const catalogSupport = codexServiceTierCatalogFastSupport(modelName);
    if (catalogSupport !== null) return catalogSupport;
    return codexServiceTierBuiltInFastSupported(modelName);
  }

  function codexServiceTierCatalogFastSupport(modelName) {
    // 优先以后端 catalog 的模型能力为准（service_tiers 含 priority 或 supports_fast=true）；
    // catalog 未提供该模型条目时返回 null，交由内置白名单兜底。
    const normalized = normalizeCodexServiceTierModelName(modelName);
    if (!normalized) return null;
    const entries = Array.isArray(codexModelCatalog.model_entries) ? codexModelCatalog.model_entries : [];
    const entry = entries.find(
      (item) => normalizeCodexServiceTierModelName(item && item.slug) === normalized
    );
    if (!entry) return null;
    if (typeof entry.supports_fast === "boolean") return entry.supports_fast;
    if (Array.isArray(entry.service_tiers) && entry.service_tiers.length > 0) {
      return entry.service_tiers.some((tier) => isFastServiceTierValue(tier && tier.id));
    }
    return null;
  }

  function codexServiceTierFastUnsupportedMessage(modelName = codexServiceTierCurrentModelName()) {
    const modelText = modelName ? `当前模型 ${modelName} 不支持` : "当前模型未读取";
    return `Fast 仅 支持 gpt-5.4及以上模型， ${modelText}`;
  }

  function codexServiceTierMaybeLoadModelCatalog(force = false) {
    if (codexModelCatalogPromise) return;
    if (!force && codexModelCatalog.status === "failed") return;
    if (!force && codexModelCatalogLoadedAt && Date.now() - codexModelCatalogLoadedAt < 10000) return;
    loadCodexModelCatalog(force).then(() => {
      refreshCodexServiceTierControls();
    }).catch(() => {
      refreshCodexServiceTierControls();
    });
  }

  function codexServiceTierFastAvailability(modelName = codexServiceTierCurrentModelName()) {
    const normalizedModel = normalizeCodexServiceTierModelName(modelName);
    return {
      modelName: modelName || "",
      supported: !!normalizedModel && codexServiceTierFastSupportedForModel(modelName),
    };
  }

  function codexServiceTierValueForMode(mode) {
    if (mode === "fast") return codexFastServiceTierValue();
    if (mode === "standard") return null;
    return codexServiceTierState.serviceTier || null;
  }

  function codexServiceTierDefaultModeForControlMode(controlMode, fallback = "inherit") {
    if (controlMode === "global-fast") return "fast";
    if (controlMode === "global-standard") return "standard";
    if (controlMode === "inherit") return "inherit";
    return normalizeCodexThreadServiceTierMode(fallback);
  }

  function codexServiceTierEffectiveThreadMode(threadMode = "inherit", defaultMode = "inherit") {
    const normalizedThreadMode = normalizeCodexThreadServiceTierMode(threadMode);
    if (normalizedThreadMode !== "inherit") return normalizedThreadMode;
    return normalizeCodexThreadServiceTierMode(defaultMode);
  }

  function codexServiceTierValueForControlMode(controlMode, threadMode = "inherit", defaultMode = "inherit") {
    if (controlMode === "global-fast") return codexFastServiceTierValue();
    if (controlMode === "global-standard") return null;
    if (controlMode === "custom") return codexServiceTierValueForMode(codexServiceTierEffectiveThreadMode(threadMode, defaultMode));
    return codexServiceTierState.serviceTier || null;
  }

  function codexServiceTierEffectiveMode(value) {
    return isFastServiceTierValue(value) ? "fast" : "standard";
  }

  function normalizeCodexThreadServiceTierMode(mode) {
    const normalized = String(mode || "").trim().toLowerCase();
    return codexThreadServiceTierModes.has(normalized) ? normalized : "inherit";
  }

  function normalizeCodexServiceTierControlMode(mode) {
    const normalized = String(mode || "").trim().toLowerCase();
    return codexServiceTierControlModes.has(normalized) ? normalized : "inherit";
  }

  function serviceTierGlobalStatusMessage(serviceTier) {
    if (isFastServiceTierValue(serviceTier)) return "Fast 已开启";
    if (!serviceTier) return "默认服务模式";
    return `当前：${serviceTier}`;
  }

  function serviceTierStatusMessage(
    controlMode = codexServiceTierState.controlMode || "inherit",
    threadMode = codexServiceTierState.threadMode || "inherit",
    effectiveMode = codexServiceTierState.effectiveMode || "standard",
    defaultMode = codexServiceTierState.defaultMode || "inherit"
  ) {
    if (codexServiceTierState.status === "loading") return "正在读取…";
    if (codexServiceTierState.status === "failed") return "读取失败";
    if (controlMode === "inherit") return `继承 config.toml：${effectiveMode}`;
    if (controlMode === "global-standard") return "全局 Standard";
    if (controlMode === "global-fast") return "全局 Fast";
    if (threadMode === "inherit") return `自定义：默认 ${defaultMode}`;
    return `自定义：当前 thread ${threadMode}`;
  }

  function readThreadServiceTierState() {
    if (codexThreadServiceTierStateCache) return codexThreadServiceTierStateCache;
    try {
      const parsed = JSON.parse(localStorage.getItem(codexThreadServiceTierKey) || "{}");
      const rawEntries = parsed?.version === codexThreadServiceTierVersion && parsed?.entries && typeof parsed.entries === "object"
        ? parsed.entries
        : {};
      const entries = Object.create(null);
      Object.entries(rawEntries).forEach(([key, value]) => {
        const safeKey = typeof validThreadSessionKey === "function" ? validThreadSessionKey(key) : String(key || "");
        const mode = normalizeCodexThreadServiceTierMode(value?.mode);
        if (safeKey && mode !== "inherit") entries[safeKey] = { mode, at: finiteNonNegativeNumber(value?.at) || Date.now() };
      });
      const draft = normalizeThreadServiceTierDraft(parsed?.draft);
      const hasCustomState = !!draft || Object.keys(entries).length > 0;
      const mode = parsed?.mode ? normalizeCodexServiceTierControlMode(parsed.mode) : (hasCustomState ? "custom" : "inherit");
      codexThreadServiceTierStateCache = {
        mode,
        defaultMode: normalizeCodexThreadServiceTierMode(parsed?.defaultMode || codexServiceTierDefaultModeForControlMode(mode)),
        entries,
        draft,
      };
    } catch (_) {
      codexThreadServiceTierStateCache = { mode: "inherit", defaultMode: "inherit", entries: Object.create(null), draft: null };
    }
    return codexThreadServiceTierStateCache;
  }

  function writeThreadServiceTierState(state) {
    const mode = normalizeCodexServiceTierControlMode(state?.mode);
    const defaultMode = normalizeCodexThreadServiceTierMode(state?.defaultMode || codexServiceTierDefaultModeForControlMode(mode));
    const rawEntries = state?.entries && typeof state.entries === "object" ? state.entries : {};
    const entries = Object.create(null);
    Object.entries(rawEntries)
      .map(([key, value]) => {
        const safeKey = validThreadSessionKey(key);
        const mode = normalizeCodexThreadServiceTierMode(value?.mode);
        return safeKey && mode !== "inherit" ? [safeKey, { mode, at: finiteNonNegativeNumber(value?.at) || Date.now() }] : null;
      })
      .filter(Boolean)
      .sort((left, right) => right[1].at - left[1].at)
      .slice(0, codexThreadServiceTierMaxEntries)
      .forEach(([key, value]) => {
        entries[key] = value;
      });
    const draft = normalizeThreadServiceTierDraft(state?.draft);
    try {
      localStorage.setItem(codexThreadServiceTierKey, JSON.stringify({
        version: codexThreadServiceTierVersion,
        mode,
        defaultMode,
        entries,
        ...(draft ? { draft } : {}),
      }));
    } catch (_) {}
    codexThreadServiceTierStateCache = { mode, defaultMode, entries, draft };
  }

  function normalizeThreadServiceTierDraft(value) {
    if (!value || typeof value !== "object") return null;
    const mode = normalizeCodexThreadServiceTierMode(value.mode);
    if (mode === "inherit") return null;
    const at = finiteNonNegativeNumber(value.at) || Date.now();
    return { mode, at };
  }

  function codexThreadServiceTierOverride(threadId) {
    const key = validThreadSessionKey(threadId);
    if (!key) return null;
    const entry = readThreadServiceTierState().entries[key];
    const mode = normalizeCodexThreadServiceTierMode(entry?.mode);
    return mode === "inherit" ? null : { mode, at: finiteNonNegativeNumber(entry?.at) || 0 };
  }

  function codexThreadServiceTierDraft() {
    const draft = readThreadServiceTierState().draft;
    if (!draft) return null;
    if (Date.now() - draft.at > codexThreadServiceTierDraftBindWindowMs) return null;
    return draft;
  }

  function setCodexThreadServiceTierOverride(threadId, mode) {
    const normalizedMode = normalizeCodexThreadServiceTierMode(mode);
    const state = readThreadServiceTierState();
    state.mode = "custom";
    const key = validThreadSessionKey(threadId);
    if (key) {
      if (normalizedMode === "inherit") {
        delete state.entries[key];
      } else {
        state.entries[key] = { mode: normalizedMode, at: Date.now() };
      }
    } else if (normalizedMode === "inherit") {
      state.draft = null;
    } else {
      state.draft = { mode: normalizedMode, at: Date.now() };
    }
    writeThreadServiceTierState(state);
  }

  function bindDraftServiceTierToThread(threadId) {
    const key = validThreadSessionKey(threadId);
    const draft = codexThreadServiceTierDraft();
    if (!key || !draft) return false;
    const state = readThreadServiceTierState();
    if (normalizeCodexServiceTierControlMode(state.mode) !== "custom") {
      state.draft = null;
      writeThreadServiceTierState(state);
      return false;
    }
    if (!state.entries[key]) state.entries[key] = { mode: draft.mode, at: Date.now() };
    state.draft = null;
    writeThreadServiceTierState(state);
    return true;
  }

  function setCodexServiceTierControlMode(mode) {
    if (codexElvesBackendStatus.status !== "ok") {
      showToast("后端未连接，无法切换服务模式");
      refreshCodexServiceTierControls();
      return;
    }
    const normalizedMode = normalizeCodexServiceTierControlMode(mode);
    if (normalizedMode === "global-fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName));
        refreshCodexServiceTierControls();
        return;
      }
    }
    const state = readThreadServiceTierState();
    state.mode = normalizedMode;
    if (normalizedMode !== "custom") {
      state.defaultMode = codexServiceTierDefaultModeForControlMode(normalizedMode);
      state.entries = Object.create(null);
      state.draft = null;
    } else {
      state.defaultMode = normalizeCodexThreadServiceTierMode(state.defaultMode);
    }
    writeThreadServiceTierState(state);
    refreshCodexServiceTierControls();
    const labels = {
      inherit: "继承 config.toml",
      "global-standard": "全局 Standard",
      "global-fast": "全局 Fast",
      custom: "自定义",
    };
    showToast(`服务模式：${labels[normalizedMode] || normalizedMode}`);
  }

  function syncCodexServiceTierEffectiveState() {
    if (!codexElvesSettings().serviceTierControls) {
      codexServiceTierState = {
        ...codexServiceTierState,
        activeThreadId: "",
        threadMode: "inherit",
        effectiveServiceTier: codexServiceTierState.serviceTier || null,
        effectiveMode: codexServiceTierEffectiveMode(codexServiceTierState.serviceTier),
        message: "未启用",
      };
      return;
    }
    const activeThreadId = validThreadSessionKey(currentSessionRef().session_id);
    if (activeThreadId) bindDraftServiceTierToThread(activeThreadId);
    const storedState = readThreadServiceTierState();
    const controlMode = normalizeCodexServiceTierControlMode(storedState.mode);
    const defaultMode = normalizeCodexThreadServiceTierMode(storedState.defaultMode);
    const override = activeThreadId ? codexThreadServiceTierOverride(activeThreadId) : codexThreadServiceTierDraft();
    const threadMode = normalizeCodexThreadServiceTierMode(override?.mode);
    const effectiveServiceTier = codexServiceTierValueForControlMode(controlMode, threadMode, defaultMode);
    const effectiveMode = codexServiceTierEffectiveMode(effectiveServiceTier);
    const fastAvailability = codexServiceTierFastAvailability();
    const message = effectiveMode === "fast" && !fastAvailability.supported
      ? codexServiceTierFastUnsupportedMessage(fastAvailability.modelName)
      : serviceTierStatusMessage(controlMode, threadMode, effectiveMode, defaultMode);
    const canSyncNativeThreadServiceTier = effectiveMode !== "fast" || fastAvailability.supported;
    if (controlMode !== "inherit" && activeThreadId && canSyncNativeThreadServiceTier) {
      syncCodexNativeThreadServiceTier(activeThreadId, effectiveServiceTier, "state");
    }
    codexServiceTierState = {
      ...codexServiceTierState,
      controlMode,
      defaultMode,
      activeThreadId,
      threadMode,
      effectiveServiceTier,
      effectiveMode,
      fastModelName: fastAvailability.modelName,
      fastSupported: fastAvailability.supported,
      message,
    };
  }

  function codexServiceTierBadgeState() {
    if (codexElvesBackendStatus.status === "checking") return { tier: "loading", label: "...", disabled: true, title: "服务模式：正在检查后端连接" };
    if (codexElvesBackendStatus.status && codexElvesBackendStatus.status !== "ok") return { tier: "failed", label: "未连接", disabled: true, title: "服务模式：后端未连接，无法切换" };
    if (codexServiceTierState.status === "loading") return { tier: "loading", label: "...", title: "服务模式：正在读取" };
    if (codexServiceTierState.status === "failed") return { tier: "failed", label: "?", title: "服务模式：读取失败" };
    const fastAvailability = codexServiceTierFastAvailability();
    const effectiveMode = codexServiceTierState.effectiveMode || "standard";
    const scope = codexServiceTierState.controlMode === "custom" && codexServiceTierState.threadMode !== "inherit"
      ? `当前 thread：${codexServiceTierState.threadMode}`
      : serviceTierStatusMessage(codexServiceTierState.controlMode, codexServiceTierState.threadMode, effectiveMode, codexServiceTierState.defaultMode);
    const title = [
      `服务模式：${scope}`,
      "Standard：使用标准处理；不在请求上设置 priority。",
      `Fast：仅支持 ${codexServiceTierFastModelListLabel()}；对支持模型使用 service_tier=\"priority\"，官方说明其延迟更低且更一致，但会按更高价格计费；rate limit 与 Standard 共享，流量快速上涨时可能回落到 Standard。`,
    ].join("\n");
    if (effectiveMode === "fast" && !fastAvailability.supported) {
      return { tier: "unsupported", label: "不支持", title: `${title}\n${codexServiceTierFastUnsupportedMessage(fastAvailability.modelName)}；当前请求会按 Standard 发送。` };
    }
    if (effectiveMode === "fast") return { tier: "fast", label: "fast", title };
    return { tier: "standard", label: "standard", title };
  }

  function refreshCodexServiceTierBadges() {
    const state = codexServiceTierBadgeState();
    document.querySelectorAll(`[data-codex-service-tier-badge="true"]`).forEach((node) => {
      node.dataset.tier = state.tier;
      node.dataset.disabled = String(!!state.disabled);
      node.textContent = state.label;
      node.removeAttribute("data-codex-tooltip");
      node.setAttribute("title", state.title);
      node.setAttribute("aria-label", state.title);
    });
  }

  function refreshCodexServiceTierControls() {
    syncCodexServiceTierEffectiveState();
    const featureEnabled = !!codexElvesSettings().serviceTierControls;
    const backendConnected = codexElvesBackendStatus.status === "ok";
    const backendChecking = codexElvesBackendStatus.status === "checking";
    if (featureEnabled && backendConnected) codexServiceTierMaybeLoadModelCatalog();
    const fastAvailability = codexServiceTierFastAvailability();
    const fastDisabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading" || !fastAvailability.supported;
    const fastTitle = fastAvailability.supported
      ? "Fast：使用 service_tier=\"priority\""
      : codexServiceTierFastUnsupportedMessage(fastAvailability.modelName);
    const fastUnsupportedActive = codexServiceTierState.effectiveMode === "fast" && !fastAvailability.supported;
    document.querySelectorAll("[data-codex-service-tier-controls]").forEach((node) => {
      node.hidden = !featureEnabled;
    });
    document.querySelectorAll("[data-codex-service-tier-status]").forEach((node) => {
      node.dataset.status = fastUnsupportedActive ? "unsupported" : (featureEnabled && backendConnected ? (codexServiceTierState.status || "loading") : (backendChecking ? "loading" : "failed"));
      node.textContent = featureEnabled
        ? (backendConnected ? (codexServiceTierState.message || "未读取") : (backendChecking ? "正在检查后端…" : "未连接"))
        : "未启用";
    });
    document.querySelectorAll("[data-codex-service-tier-inherit]").forEach((button) => {
      button.disabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading";
      button.dataset.active = String(codexServiceTierState.controlMode === "inherit");
    });
    document.querySelectorAll("[data-codex-service-tier-standard]").forEach((button) => {
      button.disabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading";
      button.dataset.active = String(codexServiceTierState.controlMode === "global-standard");
    });
    document.querySelectorAll("[data-codex-service-tier-fast]").forEach((button) => {
      button.disabled = fastDisabled;
      button.dataset.active = String(codexServiceTierState.controlMode === "global-fast");
      button.dataset.codexTooltip = fastTitle;
      button.removeAttribute("title");
    });
    document.querySelectorAll("[data-codex-service-tier-custom]").forEach((button) => {
      button.disabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading";
      button.dataset.active = String(codexServiceTierState.controlMode === "custom");
    });
    document.querySelectorAll("[data-codex-service-tier-thread-inherit]").forEach((button) => {
      button.disabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading";
      button.dataset.active = String(codexServiceTierState.controlMode === "custom" && codexServiceTierState.threadMode === "inherit");
      button.dataset.codexTooltip = `当前 thread 不单独覆盖，继承自定义默认 ${codexServiceTierState.defaultMode || "inherit"}`;
      button.removeAttribute("title");
    });
    document.querySelectorAll("[data-codex-service-tier-thread-standard]").forEach((button) => {
      button.disabled = !featureEnabled || !backendConnected || codexServiceTierState.status === "loading";
      button.dataset.active = String(codexServiceTierState.controlMode === "custom" && codexServiceTierState.threadMode === "standard");
    });
    document.querySelectorAll("[data-codex-service-tier-thread-fast]").forEach((button) => {
      button.disabled = fastDisabled;
      button.dataset.active = String(codexServiceTierState.controlMode === "custom" && codexServiceTierState.threadMode === "fast");
      button.dataset.codexTooltip = fastTitle;
      button.removeAttribute("title");
    });
    refreshCodexServiceTierBadges();
  }

  async function loadCodexServiceTierState() {
    if (!codexElvesSettings().serviceTierControls) {
      codexServiceTierState = { ...codexServiceTierState, status: "idle", message: "未启用" };
      refreshCodexServiceTierControls();
      return;
    }
    codexServiceTierState = { ...codexServiceTierState, status: "loading", message: "正在读取…" };
    refreshCodexServiceTierControls();
    try {
      const serviceTier = await getCodexServiceTierSetting();
      codexServiceTierState = {
        ...codexServiceTierState,
        status: "ok",
        serviceTier,
        message: serviceTierGlobalStatusMessage(serviceTier),
      };
    } catch (error) {
      codexServiceTierState = {
        ...codexServiceTierState,
        status: "failed",
        message: "读取失败",
      };
      sendCodexElvesDiagnostic("service_tier_read_failed", {
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
    } finally {
      refreshCodexServiceTierControls();
    }
  }

  function setCodexThreadServiceTierMode(mode) {
    if (codexElvesBackendStatus.status !== "ok") {
      showToast("后端未连接，无法切换服务模式");
      refreshCodexServiceTierControls();
      return;
    }
    const normalizedMode = normalizeCodexThreadServiceTierMode(mode);
    if (normalizedMode === "fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName));
        refreshCodexServiceTierControls();
        return;
      }
    }
    const threadId = validThreadSessionKey(currentSessionRef().session_id);
    setCodexThreadServiceTierOverride(threadId, normalizedMode);
    refreshCodexServiceTierControls();
    const target = threadId ? "当前 thread" : "新 thread 草稿";
    showToast(`${target}服务模式：${normalizedMode === "inherit" ? "继承" : normalizedMode}`);
  }

  function toggleCodexServiceTierFromBadge() {
    if (codexElvesBackendStatus.status !== "ok") {
      showToast("后端未连接，无法切换服务模式");
      refreshCodexServiceTierControls();
      return;
    }
    syncCodexServiceTierEffectiveState();
    const nextMode = codexServiceTierState.effectiveMode === "fast" ? "standard" : "fast";
    if (nextMode === "fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName));
        refreshCodexServiceTierControls();
        return;
      }
    }
    setCodexThreadServiceTierMode(nextMode);
  }

  function codexServiceTierRequestMethods() {
    return new Set(["thread/start", "thread/resume", "turn/start"]);
  }

  function codexServiceTierThreadIdForRequest(method, params, threadIdHint = "") {
    if (method === "thread/start") return validThreadSessionKey(params?.threadId || threadIdHint);
    return validThreadSessionKey(params?.threadId || params?.conversationId || threadIdHint || currentSessionRef().session_id);
  }

  function codexServiceTierOverrideResult(method, params, threadIdHint, mode, requestedServiceTier, modelHint = "") {
    const threadId = codexServiceTierThreadIdForRequest(method, params, threadIdHint);
    const requestedFast = isFastServiceTierValue(requestedServiceTier);
    const modelName = codexServiceTierModelForRequest(params, modelHint);
    const fastSupported = !requestedFast || codexServiceTierFastSupportedForModel(modelName);
    return {
      threadId,
      mode,
      serviceTier: requestedFast && fastSupported ? codexFastServiceTierValue() : null,
      requestedServiceTier: requestedServiceTier || null,
      modelName,
      fastSupported,
      fastBlocked: requestedFast && !fastSupported,
    };
  }

  function codexServiceTierOverrideForRequest(method, params, threadIdHint = "") {
    if (!codexElvesSettings().serviceTierControls) return null;
    if (!codexServiceTierRequestMethods().has(method) || !params || typeof params !== "object") return null;
    const state = readThreadServiceTierState();
    const controlMode = normalizeCodexServiceTierControlMode(state.mode);
    const defaultMode = normalizeCodexThreadServiceTierMode(state.defaultMode);
    if (controlMode === "inherit") {
      const inheritedServiceTier = params.serviceTier ?? params.service_tier ?? codexServiceTierState.serviceTier;
      const override = codexServiceTierOverrideResult(method, params, threadIdHint, "inherit", inheritedServiceTier);
      return override.fastBlocked ? override : null;
    }
    if (controlMode === "global-standard" || controlMode === "global-fast") {
      return codexServiceTierOverrideResult(
        method,
        params,
        threadIdHint,
        controlMode,
        controlMode === "global-fast" ? codexFastServiceTierValue() : null
      );
    }
    const threadId = codexServiceTierThreadIdForRequest(method, params, threadIdHint);
    const override = threadId ? codexThreadServiceTierOverride(threadId) : codexThreadServiceTierDraft();
    const mode = codexServiceTierEffectiveThreadMode(override?.mode, defaultMode);
    if (mode === "inherit") {
      const inheritedServiceTier = params.serviceTier ?? params.service_tier ?? codexServiceTierState.serviceTier;
      const inheritedOverride = codexServiceTierOverrideResult(method, params, threadIdHint, "inherit", inheritedServiceTier);
      return inheritedOverride.fastBlocked ? { ...inheritedOverride, threadId, mode } : null;
    }
    return {
      ...codexServiceTierOverrideResult(method, params, threadIdHint, mode, mode === "fast" ? codexFastServiceTierValue() : null),
      threadId,
      mode,
    };
  }

  function applyCodexServiceTierRequestOverride(method, params, threadIdHint = "") {
    const override = codexServiceTierOverrideForRequest(method, params, threadIdHint);
    if (!override) return params;
    const nextParams = { ...(params || {}), serviceTier: override.serviceTier };
    if (Object.prototype.hasOwnProperty.call(nextParams, "service_tier") || override.fastBlocked) {
      nextParams.service_tier = override.serviceTier;
    }
    if (override.threadId && !override.fastBlocked) {
      syncCodexNativeThreadServiceTier(override.threadId, override.serviceTier, "request");
    }
    sendCodexElvesDiagnostic("service_tier_request_override_applied", {
      method,
      threadId: override.threadId || "",
      mode: override.mode,
      serviceTier: override.serviceTier || "standard",
      model: override.modelName || "",
      fastSupported: override.fastSupported !== false,
      fastBlocked: !!override.fastBlocked,
    });
    return nextParams;
  }

  function codexServiceTierRequestOverride(message) {
    if (!codexElvesSettings().serviceTierControls) return message;
    if (!message || typeof message !== "object") return message;
    if (message.type === "send-cli-request-for-host") {
      const method = String(message.method || "");
      const params = applyCodexServiceTierRequestOverride(method, message.params);
      return params === message.params ? message : { ...message, params };
    }
    if (message.type === "mcp-request" && message.request && typeof message.request === "object") {
      const method = String(message.request.method || "");
      const params = applyCodexServiceTierRequestOverride(method, message.request.params);
      if (params === message.request.params) return message;
      return { ...message, request: { ...message.request, params } };
    }
    if (message.type === "worker-request" && message.request && typeof message.request === "object") {
      const method = String(message.request.method || "");
      const params = applyCodexServiceTierRequestOverride(method, message.request.params);
      if (params === message.request.params) return message;
      return { ...message, request: { ...message.request, params } };
    }
    if (message.type === "thread-prewarm-start" && message.request && typeof message.request === "object") {
      const params = applyCodexServiceTierRequestOverride("thread/start", message.request.params);
      if (params === message.request.params) return message;
      return { ...message, request: { ...message.request, params } };
    }
    if (message.type === "start-conversation") {
      const nextMessage = applyCodexServiceTierRequestOverride("thread/start", message);
      return nextMessage === message ? message : nextMessage;
    }
    if (message.type === "prewarm-thread-start-for-host" && message.params && typeof message.params === "object") {
      const params = applyCodexServiceTierRequestOverride("thread/start", message.params);
      return params === message.params ? message : { ...message, params };
    }
    if (message.type === "start-thread-for-host") {
      const params = applyCodexServiceTierRequestOverride("thread/start", message);
      return params === message ? message : params;
    }
    if (message.type === "start-turn-for-host" && message.params && typeof message.params === "object") {
      const params = applyCodexServiceTierRequestOverride("turn/start", message.params, message.conversationId);
      return params === message.params ? message : { ...message, params };
    }
    return message;
  }

  function codexServiceTierPatchRetryDelay(failureCount) {
    return Math.min(
      codexServiceTierRequestClientPatchRetryMaxMs,
      codexServiceTierRequestClientPatchRetryBaseMs * (2 ** Math.min(Math.max(failureCount - 1, 0), 5))
    );
  }

  function clearCodexServiceTierDispatcherPatchRetry(resetFailure = false) {
    clearTimeout(window.__codexServiceTierDispatcherPatchRetryTimer);
    window.__codexServiceTierDispatcherPatchRetryTimer = null;
    if (resetFailure) window.__codexServiceTierDispatcherPatchFailureCount = 0;
  }

  function clearCodexServiceTierRequestClientPatchRetry(resetFailure = false) {
    clearTimeout(window.__codexServiceTierRequestClientPatchRetryTimer);
    window.__codexServiceTierRequestClientPatchRetryTimer = null;
    if (resetFailure) {
      window.__codexServiceTierRequestClientPatchFailureCount = 0;
      window.__codexServiceTierRequestClientPatchNextAttemptAt = 0;
    }
  }

  function scheduleCodexServiceTierDispatcherPatchRetry(failureCount) {
    clearCodexServiceTierDispatcherPatchRetry();
    if (!codexElvesSettings().serviceTierControls) return false;
    const delayMs = codexServiceTierPatchRetryDelay(failureCount);
    const runtimeId = codexSessionPrewarmRuntimeId;
    window.__codexServiceTierDispatcherPatchRetryTimer = setTimeout(() => {
      window.__codexServiceTierDispatcherPatchRetryTimer = null;
      if (runtimeId !== window.__codexSessionPrewarmRuntimeId) return;
      void installCodexServiceTierDispatcherPatch();
    }, delayMs);
    return true;
  }

  function scheduleCodexServiceTierRequestClientPatchRetry(failureCount) {
    clearCodexServiceTierRequestClientPatchRetry();
    if (!codexElvesSettings().serviceTierControls) return false;
    const delayMs = codexServiceTierPatchRetryDelay(failureCount);
    const runtimeId = codexSessionPrewarmRuntimeId;
    window.__codexServiceTierRequestClientPatchRetryTimer = setTimeout(() => {
      window.__codexServiceTierRequestClientPatchRetryTimer = null;
      if (runtimeId !== window.__codexSessionPrewarmRuntimeId) return;
      void installCodexServiceTierRequestClientPatch();
    }, delayMs);
    return true;
  }

  function installCodexServiceTierDispatcherPatch() {
    if (window.__codexServiceTierRequestOverrideInstalled === codexServiceTierRequestOverrideVersion) {
      clearCodexServiceTierDispatcherPatchRetry(true);
      return Promise.resolve(true);
    }
    if (window.__codexServiceTierDispatcherPatchPromise) {
      return window.__codexServiceTierDispatcherPatchPromise;
    }
    const patch = async () => {
      try {
        const dispatcher = await findCodexServiceTierDispatcher();
        if (!dispatcher || typeof dispatcher.dispatchMessage !== "function") throw new Error("Codex dispatcher unavailable");
        codexServiceTierDispatcher = dispatcher;
        if (dispatcher.__codexServiceTierOriginalDispatchMessage) {
          window.__codexServiceTierRequestOverrideInstalled = codexServiceTierRequestOverrideVersion;
          clearCodexServiceTierDispatcherPatchRetry(true);
          refreshCodexServiceTierControls();
          return true;
        }
        dispatcher.__codexServiceTierOriginalDispatchMessage = dispatcher.dispatchMessage.bind(dispatcher);
        dispatcher.dispatchMessage = (type, payload) => {
          const message = codexServiceTierRequestOverride({ ...(payload || {}), type });
          const nextType = message?.type || type;
          const { type: _type, ...nextPayload } = message || {};
          return dispatcher.__codexServiceTierOriginalDispatchMessage(nextType, nextPayload);
        };
        window.__codexServiceTierRequestOverrideInstalled = codexServiceTierRequestOverrideVersion;
        clearCodexServiceTierDispatcherPatchRetry(true);
        sendCodexElvesDiagnostic("service_tier_dispatcher_patch_installed", {});
        refreshCodexServiceTierControls();
        return true;
      } catch (error) {
        const failureCount = Number(window.__codexServiceTierDispatcherPatchFailureCount || 0) + 1;
        const retryAfterMs = codexServiceTierPatchRetryDelay(failureCount);
        window.__codexServiceTierDispatcherPatchFailureCount = failureCount;
        scheduleCodexServiceTierDispatcherPatchRetry(failureCount);
        sendCodexElvesDiagnostic("service_tier_dispatcher_patch_failed", {
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
          failureCount,
          retryAfterMs,
        });
        return false;
      } finally {
        if (window.__codexServiceTierDispatcherPatchPromise === patchPromise) {
          window.__codexServiceTierDispatcherPatchPromise = null;
        }
      }
    };
    const patchPromise = patch();
    window.__codexServiceTierDispatcherPatchPromise = patchPromise;
    return patchPromise;
  }

  function installCodexServiceTierRequestClientPatch() {
    if (window.__codexServiceTierRequestClientPatchInstalled === codexServiceTierRequestOverrideVersion) {
      clearCodexServiceTierRequestClientPatchRetry(true);
      return Promise.resolve(true);
    }
    if (window.__codexServiceTierRequestClientPatchPromise) {
      return window.__codexServiceTierRequestClientPatchPromise;
    }
    const now = Date.now();
    const nextAttemptAt = Number(window.__codexServiceTierRequestClientPatchNextAttemptAt || 0);
    if (now < nextAttemptAt) return;
    const patch = async () => {
      try {
        let requestClientClass = null;
        let modernModuleLoaded = false;
        let lastError = null;
        for (const namePart of codexServiceTierRequestClientModuleParts) {
          try {
            const module = await loadCodexAppModule(namePart);
            requestClientClass = codexServiceTierRequestClientClassFromModule(module);
            modernModuleLoaded ||= namePart === codexServiceTierModernModulePart;
            if (requestClientClass) break;
            lastError = new Error(`Codex AppServerRequestClient unavailable: ${namePart}`);
          } catch (error) {
            lastError = error;
          }
        }
        if (!requestClientClass && modernModuleLoaded) {
          const dispatcherInstalled = await installCodexServiceTierDispatcherPatch();
          if (!dispatcherInstalled) throw lastError || new Error("Codex dispatcher fallback unavailable");
          window.__codexServiceTierRequestClientPatchInstalled = codexServiceTierRequestOverrideVersion;
          window.__codexServiceTierRequestClientPatchFailureCount = 0;
          window.__codexServiceTierRequestClientPatchNextAttemptAt = 0;
          window.__codexServiceTierRequestClientPatchFailureSignature = "";
          clearCodexServiceTierRequestClientPatchRetry(true);
          sendCodexElvesDiagnostic("service_tier_request_client_patch_skipped", {
            reason: "modern_dispatcher_covers_host_requests",
          });
          return true;
        }
        if (!requestClientClass) throw lastError || new Error("Codex AppServerRequestClient unavailable");
        if (!patchCodexServiceTierRequestClientPrototype(requestClientClass)) {
          throw new Error("Codex AppServerRequestClient patch rejected");
        }
        window.__codexServiceTierRequestClientPatchInstalled = codexServiceTierRequestOverrideVersion;
        window.__codexServiceTierRequestClientPatchFailureCount = 0;
        window.__codexServiceTierRequestClientPatchNextAttemptAt = 0;
        window.__codexServiceTierRequestClientPatchFailureSignature = "";
        clearCodexServiceTierRequestClientPatchRetry(true);
        sendCodexElvesDiagnostic("service_tier_request_client_patch_installed", {});
        return true;
      } catch (error) {
        const failureCount = Number(window.__codexServiceTierRequestClientPatchFailureCount || 0) + 1;
        const retryAfterMs = codexServiceTierPatchRetryDelay(failureCount);
        const errorName = error?.name || "";
        const errorMessage = error?.message || String(error);
        const failureSignature = `${errorName}:${errorMessage}`;
        window.__codexServiceTierRequestClientPatchFailureCount = failureCount;
        window.__codexServiceTierRequestClientPatchNextAttemptAt = Date.now() + retryAfterMs;
        scheduleCodexServiceTierRequestClientPatchRetry(failureCount);
        if (window.__codexServiceTierRequestClientPatchFailureSignature !== failureSignature) {
          window.__codexServiceTierRequestClientPatchFailureSignature = failureSignature;
          sendCodexElvesDiagnostic("service_tier_request_client_patch_failed", {
            errorName,
            errorMessage,
            failureCount,
            retryAfterMs,
          });
        }
        return false;
      } finally {
        if (window.__codexServiceTierRequestClientPatchPromise === patchPromise) {
          window.__codexServiceTierRequestClientPatchPromise = null;
        }
      }
    };
    const patchPromise = patch();
    window.__codexServiceTierRequestClientPatchPromise = patchPromise;
    return patchPromise;
  }

  function applyLoadedBackendSettings(settings, reason = "settings-loaded") {
    codexElvesBackendSettings = { ...codexElvesBackendSettings, ...settings };
    invalidateCodexElvesSettingsCache();
    codexElvesBackendSettingsLoaded = true;
    refreshCodexElvesBackendToggles();
    refreshCodexServiceTierFeatureState();
    refreshCodexTokenUsageFeatureState();
    refreshUpstreamBranchDropdownAdapter();
    syncChatsSortVisibilityListener();
    if (!codexElvesSettings().projectMove) stopChatsSortRuntime();
  }

  async function loadBackendSettings() {
    try {
      const settings = await postJson("/settings/get", {});
      if (!settings || typeof settings !== "object" || (!("launchMode" in settings) && !("enhancementsEnabled" in settings) && !("providerSyncEnabled" in settings))) {
        throw new Error("invalid backend settings response");
      }
      void applyLoadedBackendSettings(settings, "settings-loaded");
      return true;
    } catch (_) {
      refreshCodexElvesBackendToggles();
      return false;
    }
  }

  function loadBackendSettingsForStartup(attempt = 0) {
    loadBackendSettings().then((loaded) => {
      if (loaded) {
        scan(scanDirtyForSetting(""));
        return;
      }
      if (attempt < 60) {
        setTimeout(() => loadBackendSettingsForStartup(attempt + 1), 250);
      }
    });
  }

  async function setBackendSetting(key, value) {
    codexElvesBackendSettings = { ...codexElvesBackendSettings, [key]: value };
    invalidateCodexElvesSettingsCache();
    refreshCodexElvesBackendToggles();
    if (key === codexElvesBackendSettingMap.taskBoard) refreshTaskBoardRuntime();
    try {
      const settings = await postJson("/settings/set", { [key]: value });
      codexElvesBackendSettings = { ...codexElvesBackendSettings, ...settings };
      invalidateCodexElvesSettingsCache();
    } finally {
      refreshCodexElvesBackendToggles();
      if (key === codexElvesBackendSettingMap.serviceTierControls) {
        refreshCodexServiceTierFeatureState();
      }
      if (key === codexElvesBackendSettingMap.tokenUsage) {
        refreshCodexTokenUsageFeatureState();
      }
      const localKey = Object.entries(codexElvesBackendSettingMap)
        .find(([, backendKey]) => backendKey === key)?.[0] || "";
      if (localKey === "taskBoard") refreshTaskBoardRuntime();
      if (localKey === "projectMove" && !codexElvesSettings().projectMove) stopChatsSortRuntime();
      if (localKey === "projectMove") syncChatsSortVisibilityListener();
      if (localKey === "upstreamWorktreeCreate") refreshUpstreamBranchDropdownAdapter();
      scan(scanDirtyForSetting(localKey));
    }
  }

  function refreshCodexServiceTierFeatureState() {
    if (codexElvesSettings().serviceTierControls) {
      syncCodexServiceTierBadgeLayoutListener();
      installCodexServiceTierBadge();
      void installCodexServiceTierDispatcherPatch();
      void installCodexServiceTierRequestClientPatch();
      void loadCodexServiceTierState();
    } else {
      clearCodexServiceTierDispatcherPatchRetry(true);
      clearCodexServiceTierRequestClientPatchRetry(true);
      syncCodexServiceTierBadgeLayoutListener();
      refreshCodexServiceTierControls();
    }
  }

  function refreshCodexElvesBackendToggles() {
    document.querySelectorAll(".codex-elves-toggle[data-codex-backend-setting]").forEach((button) => {
      const key = button.getAttribute("data-codex-backend-setting");
      button.dataset.enabled = String(!!codexElvesBackendSettings[key]);
    });
    renderCodexElvesMenu();
  }

  let codexElvesUserScripts = { enabled: true, builtin_dir: "", user_dir: "", scripts: [] };
  let codexElvesBackendStatus = { status: "checking", message: "正在检查后端…" };
  let codexElvesBackendCheckSeq = 0;

  function setCodexElvesTriggerLabel(trigger) {
    if (!trigger) return;
    let label = trigger.querySelector("[data-codex-elves-trigger-label]");
    if (!label) {
      label = document.createElement("span");
      label.dataset.codexElvesTriggerLabel = "true";
      trigger.appendChild(label);
    }
    label.textContent = `CodexElves ${codexElvesVersion}`;
  }

  function ensureCodexElvesTriggerIndicator(trigger) {
    if (!trigger) return null;
    let indicator = trigger.querySelector("[data-codex-backend-indicator]");
    if (!indicator) {
      indicator = document.createElement("span");
      indicator.className = "codex-elves-backend-indicator";
      indicator.dataset.codexBackendIndicator = "true";
      trigger.prepend(indicator);
    }
    return indicator;
  }

  function renderBackendStatus() {
    const status = codexElvesBackendStatus.status || "failed";
    if (codexElvesBackendStatus.version) {
      codexElvesVersion = codexElvesBackendStatus.version;
      document.querySelectorAll("[data-codex-elves-version]").forEach((node) => {
        node.textContent = `CodexElves ${codexElvesVersion}`;
      });
      document.querySelectorAll(`#${codexElvesMenuId} button`).forEach(setCodexElvesTriggerLabel);
    }
    const label = document.querySelector("[data-codex-backend-status]");
    if (label) {
      label.dataset.status = status;
      label.textContent = codexElvesBackendStatus.message || (status === "ok" ? "后端已连接" : "未连接");
    }
    document.querySelectorAll("[data-codex-backend-indicator]").forEach((indicator) => {
      indicator.dataset.status = status;
      indicator.dataset.codexTooltip = status === "ok" ? "后端已连接" : status === "checking" ? "正在检查后端" : "未连接";
      indicator.removeAttribute("title");
    });
    const repair = document.querySelector("[data-codex-backend-repair]");
    if (repair) repair.hidden = status === "ok" || status === "checking";
    refreshCodexServiceTierControls();
  }

  function withBackendTimeout(request) {
    return Promise.race([
      request,
      new Promise((resolve) => setTimeout(() => resolve({ status: "failed", message: "后端检查超时", timeout: true }), codexBackendStatusTimeoutMs)),
    ]);
  }

  async function checkBackendStatus() {
    const seq = ++codexElvesBackendCheckSeq;
    const nextStatus = await withBackendTimeout(postJson("/backend/status", {}));
    if (seq !== codexElvesBackendCheckSeq) return;
    codexElvesBackendStatus = nextStatus;
    if (nextStatus?.status !== "ok") {
      sendCodexElvesDiagnostic("backend_check_failed", {
        status: nextStatus?.status || "unknown",
        message: nextStatus?.message || "",
        timeout: !!nextStatus?.timeout,
      });
    }
    renderBackendStatus();
  }

  async function repairBackend() {
    codexElvesBackendStatus = { status: "checking", message: "正在修复后端…" };
    renderBackendStatus();
    try {
      const recoveredStatus = await waitForBackendBridgeRecovery();
      if (!recoveredStatus) {
        codexElvesBackendStatus = { status: "failed", message: "自动修复超时，请重启启动器" };
        renderBackendStatus();
        return;
      }
      codexElvesBackendStatus = recoveredStatus;
    } catch (error) {
      codexElvesBackendStatus = { status: "failed", message: "后端修复失败" };
    }
    renderBackendStatus();
  }

  async function waitForBackendBridgeRecovery(timeoutMs = 10000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (typeof window.__codexSessionDeleteBridge === "function") {
        const result = await Promise.race([
          Promise.resolve(window.__codexSessionDeleteBridge("/backend/status", {})).catch(() => null),
          new Promise((resolve) => setTimeout(() => resolve(null), 1000)),
        ]);
        if (result?.status === "ok") return result;
      }
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    return null;
  }

  async function openManagerFromCodex() {
    const result = await postJson("/manager/open", {});
    if (result.status === "ok") {
      showToast("管理工具已打开");
    } else {
      showToast(result.message || "打开管理工具失败");
    }
  }

  function scheduleBackendHeartbeat() {
    if (window.__codexElvesBackendHeartbeat) return;
    window.__codexElvesBackendHeartbeat = setInterval(() => {
      if (document.visibilityState === "hidden") return;
      checkBackendStatus();
    }, codexBackendHeartbeatIntervalMs);
    checkBackendStatus();
  }

  function userScriptStatusLabel(status) {
    return { loaded: "已加载", failed: "失败", disabled: "已禁用", not_loaded: "未加载", loading: "加载中" }[status] || status || "未知";
  }

  function renderUserScripts() {
    const enabledToggle = document.querySelector("[data-codex-user-scripts-enabled]");
    if (enabledToggle) enabledToggle.dataset.enabled = String(!!codexElvesUserScripts.enabled);
    const dirs = document.querySelector("[data-codex-user-script-dirs]");
    if (dirs) dirs.textContent = `内置：${codexElvesUserScripts.builtin_dir || "未找到"}  用户：${codexElvesUserScripts.user_dir || "未找到"}`;
    const list = document.querySelector("[data-codex-user-script-list]");
    if (!list) return;
    if (!codexElvesUserScripts.scripts?.length) {
      list.textContent = "未发现用户脚本。";
      return;
    }
    list.innerHTML = codexElvesUserScripts.scripts.map((script) => `
      <div class="codex-elves-user-script-item">
        <div>
          <div class="codex-elves-user-script-name">${escapeHtml(script.name || script.key)}</div>
          <div class="codex-elves-user-script-meta">${script.source === "builtin" ? "内置" : "用户"} · ${userScriptStatusLabel(script.status)}</div>
          ${script.error ? `<div class="codex-elves-user-script-error">${escapeHtml(script.error)}</div>` : ""}
        </div>
        <button type="button" class="codex-elves-toggle" data-codex-user-script-key="${escapeHtml(script.key)}" data-enabled="${String(!!script.enabled)}"><span></span></button>
      </div>
    `).join("");
  }

  async function loadUserScripts(path = "/user-scripts/list", payload = {}) {
    const result = await postJson(path, payload);
    if (result?.scripts) {
      codexElvesUserScripts = result;
      renderUserScripts();
    }
  }

  function selectCodexElvesTab(tab) {
    document.querySelectorAll(".codex-elves-modal-content").forEach((modal) => {
      modal.dataset.codexElvesActiveTab = tab;
    });
    document.querySelectorAll("[data-codex-elves-tab]").forEach((button) => {
      button.dataset.active = String(button.getAttribute("data-codex-elves-tab") === tab);
    });
    document.querySelectorAll("[data-codex-elves-panel]").forEach((panel) => {
      panel.hidden = panel.getAttribute("data-codex-elves-panel") !== tab;
    });
    if (tab === "userScripts") loadUserScripts();
  }

  function openCodexElvesModal() {
    document.querySelectorAll(".codex-elves-modal-overlay").forEach((node) => node.remove());
    document.querySelectorAll('[data-codex-elves-dialog="true"]').forEach((node) => node.remove());
    const overlay = document.createElement("div");
    overlay.className = "codex-elves-modal-overlay";
    overlay.innerHTML = `
      <div class="codex-elves-modal-content" role="dialog" aria-modal="true" aria-label="CodexElves">
        <div class="codex-elves-modal-header">
          <div class="codex-elves-modal-title"><span class="codex-elves-backend-indicator" data-codex-backend-indicator="true" data-status="checking"></span><span data-codex-elves-version="true">CodexElves ${codexElvesVersion}</span></div>
          <button type="button" class="codex-elves-modal-close" aria-label="关闭">×</button>
        </div>
        <div class="codex-elves-tabs" role="tablist" aria-label="CodexElves">
          <button type="button" class="codex-elves-tab-button" data-codex-elves-tab="home" data-active="true">主页</button>
          <button type="button" class="codex-elves-tab-button" data-codex-elves-tab="userScripts" data-active="false">用户脚本</button>
        </div>
        <div class="codex-elves-modal-body">
          <div class="codex-elves-panel" data-codex-elves-panel="home">
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">后端连接</div><div class="codex-elves-row-description">每 5 秒检查一次 launcher 后端状态；断开时可尝试修复后端运行。</div></div>
              <div class="codex-elves-backend-status">
                <div class="codex-elves-backend-label" data-codex-backend-status="true" data-status="checking">正在检查后端…</div>
                <button type="button" class="codex-elves-backend-repair" data-codex-backend-repair="true" hidden>修复后端运行</button>
              </div>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">页面功能增强</div><div class="codex-elves-row-description">关闭后停用任务看板、删除、导出、移动、Fast 按钮、插件相关和菜单位置增强。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-backend-setting="enhancementsEnabled"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">插件市场解锁</div><div class="codex-elves-row-description">${codexElvesBackendSettings.launchMode === "relay" ? "兼容增强模式下无需开启；ChatGPT 登录态会保留官方插件市场。" : "API Key 模式下扩展插件市场请求，尽量显示完整插件列表。"}</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="pluginMarketplaceUnlock" ${codexElvesBackendSettings.launchMode === "relay" ? 'disabled data-relay-unneeded="true"' : ""}><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">强制解锁入口</div><div class="codex-elves-row-description">${codexElvesBackendSettings.launchMode === "relay" ? "兼容增强模式下无需开启；官方登录态会保留插件入口。" : "恢复 1.1.9 的入口解锁方式，强制显示并启用插件入口。"}</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="pluginEntryUnlock" ${codexElvesBackendSettings.launchMode === "relay" ? 'disabled data-relay-unneeded="true"' : ""}><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">任务看板</div><div class="codex-elves-row-description">在左侧导航的“插件”下方显示内置任务看板入口；关闭时退出看板并恢复原生页面。默认开启。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="taskBoard"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">Fast 按钮</div><div class="codex-elves-row-description">显示服务模式切换按钮；Fast 仅支持 ${codexServiceTierFastModelListLabel()}，其他模型按 Standard 发送。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="serviceTierControls"><span></span></button>
            </div>
            <div class="codex-elves-row" data-codex-service-tier-controls="true">
              <div><div class="codex-elves-row-title">服务模式</div><div class="codex-elves-row-description">继承使用 config.toml 的 service tier；全局模式覆盖全部 thread；自定义允许按 thread 覆盖。</div></div>
              <div class="codex-elves-service-tier-control">
                <div class="codex-elves-service-tier-status" data-codex-service-tier-status="true" data-status="loading">正在读取…</div>
                <div class="codex-elves-service-tier-actions">
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-inherit="true">继承</button>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-standard="true">全局 Standard</button>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-fast="true">全局 Fast</button>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-custom="true">自定义</button>
                </div>
                <div class="codex-elves-service-tier-actions codex-elves-service-tier-thread-actions">
                  <span class="codex-elves-service-tier-thread-label">当前 thread 覆盖</span>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-thread-inherit="true" data-codex-tooltip="当前 thread 不单独覆盖，继承 config.toml">继承</button>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-thread-standard="true" data-codex-tooltip="仅当前 thread 使用 Standard，并切到自定义模式">Standard</button>
                  <button type="button" class="codex-elves-service-tier-button" data-codex-service-tier-thread-fast="true" data-codex-tooltip="仅当前 thread 使用 Fast，并切到自定义模式">Fast</button>
                </div>
              </div>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">会话删除</div><div class="codex-elves-row-description">在会话列表悬停显示删除按钮；删除后不可恢复。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="sessionDelete"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">Markdown 导出</div><div class="codex-elves-row-description">在会话列表显示导出按钮，按本地 rollout 导出带时间戳的 Markdown。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="markdownExport"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">会话项目移动</div><div class="codex-elves-row-description">在会话列表悬停显示移动按钮，可移动到普通对话或其他本地项目。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="projectMove"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">对话居中宽度</div><div class="codex-elves-row-description">开启后把主对话和输入框限制到固定最大宽度，适合大屏阅读。</div></div>
              <div class="codex-elves-width-control">
                <input class="codex-elves-width-input" data-codex-elves-conversation-view-width="true" min="${conversationViewMinWidth}" max="${conversationViewMaxAllowedWidth}" step="10" type="number" value="${conversationViewWidth()}">
                <button type="button" class="codex-elves-toggle" data-codex-elves-setting="conversationView"><span></span></button>
              </div>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">会话 Token 统计</div><div class="codex-elves-row-description">在右上角置顶摘要底部紧凑显示当前会话（含递归子代理）的总消耗和最近一轮输入、输出、缓存；默认关闭。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="tokenUsage"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">Upstream worktree</div><div class="codex-elves-row-description">Create a Git worktree from a fresh upstream branch, equivalent to git worktree add -b branch path upstream/base.</div></div>
              <div class="codex-elves-worktree-actions">
                <button type="button" class="codex-elves-action-button" data-codex-upstream-worktree-open="true">创建</button>
                <button type="button" class="codex-elves-toggle" data-codex-elves-setting="upstreamWorktreeCreate"><span></span></button>
              </div>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">历史会话修复</div><div class="codex-elves-row-description">切换官方登录、混合 API 或纯 API 后，让旧对话重新显示在当前模式下。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-backend-setting="providerSyncEnabled"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">页面增强模式</div><div class="codex-elves-row-description">${codexElvesBackendSettings.launchMode === "relay" ? "兼容增强：保留会话删除、导出、项目移动和用户脚本，仅关闭插件入口相关增强。" : "完整增强：加载插件入口、项目路径移动等页面能力。"}</div></div>
              <button type="button" class="codex-elves-action-button" data-codex-open-manager="true">打开管理工具</button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">原生菜单栏位置</div><div class="codex-elves-row-description">把 CodexElves 菜单插入顶部原生菜单栏；默认关闭以避免页面重渲染冲突。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="nativeMenuPlacement"><span></span></button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">打开 DevTools</div><div class="codex-elves-row-description">打开当前 ChatGPT/Codex 页面开发者工具，方便查看用户脚本报错。</div></div>
              <button type="button" class="codex-elves-action-button" data-codex-open-devtools="true">打开 DevTools</button>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">关于 CodexElves</div><div class="codex-elves-about">CodexElves 是通过外部 launcher 注入的增强菜单，不修改 ChatGPT/Codex 桌面应用原始安装文件。<br>Build: <span data-codex-elves-build="true">${codexElvesBuild}</span><br>GitHub: <a href="https://github.com/junxin367/CodexElves" target="_blank" rel="noreferrer">https://github.com/junxin367/CodexElves</a></div></div>
            </div>
            <div class="codex-elves-row">
              <div><div class="codex-elves-row-title">提出问题</div><div class="codex-elves-row-description">打开 GitHub Issues 反馈问题或建议。</div></div>
              <button type="button" class="codex-elves-issue-button" data-codex-elves-issue="true">提出问题</button>
            </div>
          </div>
          <div class="codex-elves-panel" data-codex-elves-panel="userScripts" hidden>
            <div class="codex-elves-row" data-codex-user-scripts-section="true">
              <div>
                <div class="codex-elves-row-title">用户脚本</div>
                <div class="codex-elves-row-description">启用用户脚本：自动加载内置目录和用户配置目录中的 .js 文件。</div>
                <div class="codex-elves-user-script-warning">禁用后需重载页面或重启 Codex 才能完全移除已执行效果。</div>
                <div class="codex-elves-user-script-dirs" data-codex-user-script-dirs="true">正在读取脚本目录…</div>
                <div class="codex-elves-user-script-list" data-codex-user-script-list="true">正在读取用户脚本…</div>
              </div>
              <div class="codex-elves-user-script-actions">
                <button type="button" class="codex-elves-toggle" data-codex-user-scripts-enabled="true"><span></span></button>
                <button type="button" class="codex-elves-user-script-reload" data-codex-user-scripts-reload="true">重新加载用户脚本</button>
              </div>
            </div>
          </div>
        </div>
      </div>
    `;
    const closeButton = overlay.querySelector(".codex-elves-modal-close");
    closeButton?.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      overlay.remove();
    }, true);
    overlay.addEventListener("input", (event) => {
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      const widthInput = target?.closest("[data-codex-elves-conversation-view-width]");
      if (widthInput) setConversationViewWidth(widthInput.value);
    }, true);
    overlay.addEventListener("change", (event) => {
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      const widthInput = target?.closest("[data-codex-elves-conversation-view-width]");
      if (widthInput) {
        const width = normalizeConversationViewWidth(widthInput.value);
        widthInput.value = String(width || conversationViewWidth());
        setConversationViewWidth(widthInput.value);
      }
    }, true);
    overlay.addEventListener("click", (event) => {
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (event.target === overlay || target?.closest(".codex-elves-modal-close")) {
        overlay.remove();
        return;
      }
      const tabButton = target?.closest("[data-codex-elves-tab]");
      if (tabButton) {
        selectCodexElvesTab(tabButton.getAttribute("data-codex-elves-tab"));
        return;
      }
      if (target?.closest("[data-codex-open-devtools]")) {
        postJson("/devtools/open", {});
        return;
      }
      if (target?.closest("[data-codex-open-manager]")) {
        openManagerFromCodex();
        return;
      }
      if (target?.closest("[data-codex-backend-repair]")) {
        repairBackend();
        return;
      }
      const issueButton = target?.closest("[data-codex-elves-issue]");
      if (issueButton) {
        const issueUrl = "https://github.com/junxin367/CodexElves/issues";
        window.open(issueUrl, "_blank");
        return;
      }
      const userScriptsEnabled = target?.closest("[data-codex-user-scripts-enabled]");
      if (userScriptsEnabled) {
        loadUserScripts("/user-scripts/set-enabled", { enabled: userScriptsEnabled.dataset.enabled !== "true" });
        return;
      }
      if (target?.closest("[data-codex-service-tier-inherit]")) {
        setCodexServiceTierControlMode("inherit");
        return;
      }
      if (target?.closest("[data-codex-service-tier-standard]")) {
        setCodexServiceTierControlMode("global-standard");
        return;
      }
      if (target?.closest("[data-codex-service-tier-fast]")) {
        setCodexServiceTierControlMode("global-fast");
        return;
      }
      if (target?.closest("[data-codex-service-tier-custom]")) {
        setCodexServiceTierControlMode("custom");
        return;
      }
      if (target?.closest("[data-codex-service-tier-thread-inherit]")) {
        setCodexThreadServiceTierMode("inherit");
        return;
      }
      if (target?.closest("[data-codex-service-tier-thread-standard]")) {
        setCodexThreadServiceTierMode("standard");
        return;
      }
      if (target?.closest("[data-codex-service-tier-thread-fast]")) {
        setCodexThreadServiceTierMode("fast");
        return;
      }
      const userScriptToggle = target?.closest("[data-codex-user-script-key]");
      if (userScriptToggle) {
        loadUserScripts("/user-scripts/set-script-enabled", { key: userScriptToggle.getAttribute("data-codex-user-script-key"), enabled: userScriptToggle.dataset.enabled !== "true" });
        return;
      }
      if (target?.closest("[data-codex-user-scripts-reload]")) {
        loadUserScripts("/user-scripts/reload", {});
        return;
      }
      if (target?.closest("[data-codex-upstream-worktree-open]")) {
        if (!codexElvesSettings().upstreamWorktreeCreate) {
          showToast("Upstream worktree enhancement is disabled");
          return;
        }
        openUpstreamWorktreeDialog();
        return;
      }
      const toggle = target?.closest("[data-codex-elves-setting]");
      if (toggle) {
        if (toggle.disabled) return;
        const key = toggle.getAttribute("data-codex-elves-setting");
        setCodexElvesSetting(key, !codexElvesSettings()[key]);
        return;
      }
      const backendToggle = target?.closest("[data-codex-backend-setting]");
      if (backendToggle) {
        const key = backendToggle.getAttribute("data-codex-backend-setting");
        setBackendSetting(key, !codexElvesBackendSettings[key]);
        return;
      }
    }, true);
    document.body.appendChild(overlay);
    selectCodexElvesTab("home");
    renderCodexElvesMenu();
    refreshCodexElvesBackendToggles();
    renderBackendStatus();
    void loadCodexServiceTierState();
    loadUserScripts();
  }

  function findApplicationMenuTopBar() {
    const applicationMenu = document.querySelector(selectors.applicationMenu);
    const candidates = [
      document.querySelector(selectors.applicationMenuTopBar),
      applicationMenu?.parentElement,
    ];
    return candidates.find((node) => {
      if (!node) return false;
      const rect = node.getBoundingClientRect();
      const styles = getComputedStyle(node);
      return rect.width > 0 &&
        rect.height > 0 &&
        rect.height <= 48 &&
        rect.top <= 2 &&
        styles.display === "flex";
    }) || null;
  }

  function findNativeMenuInsertionPoint() {
    if (!codexElvesSettings().nativeMenuPlacement) return null;
    const applicationMenuTopBar = findApplicationMenuTopBar();
    if (applicationMenuTopBar) {
      const applicationMenu = applicationMenuTopBar.querySelector(selectors.applicationMenu);
      const nativeButtonClass = applicationMenu?.querySelector("button")?.className || headerIconTextButtonClass;
      return {
        parent: applicationMenuTopBar,
        before: null,
        nativeButtonClass,
        menuClassName: codexElvesMenuTitlebarClass,
      };
    }
    const header = document.querySelector(selectors.appHeader);
    const isIconOnlyButton = (button) => String(button.className || "").includes("aspect-square");
    const contextSurface = header?.querySelector(selectors.headerContextMenuSurface);
    const directMenuBar = Array.from(contextSurface?.children || [])
      .find((node) => node.matches?.(selectors.nativeMenuBar));
    const menuBar = [directMenuBar, ...Array.from(header?.querySelectorAll?.(selectors.nativeMenuBar) || [])]
      .find((node) => {
        if (!node) return false;
        const rect = node.getBoundingClientRect();
        return !node.closest(".invisible") && rect.width > 0 && rect.height > 0;
      });
    if (menuBar) {
      const buttons = Array.from(menuBar.querySelectorAll("button")).filter((button) => !button.closest(`#${codexElvesMenuId}`));
      if (buttons.length && buttons.every(isIconOnlyButton)) return null;
      const openLocationButton = buttons.find((button) => /^(打开位置|Open location)$/i.test(button.getAttribute("aria-label") || ""));
      const openLocationGroup = openLocationButton?.closest?.(".inline-flex.self-start.items-stretch.overflow-hidden.rounded-lg");
      const openLocationIndex = buttons.indexOf(openLocationButton);
      const nativeButtonClass = openLocationButton
        ? buttons[openLocationIndex + 1]?.className || openLocationButton.className || ""
        : buttons[buttons.length - 1]?.className || "";
      if (openLocationGroup?.parentElement === menuBar) return { parent: menuBar, before: openLocationGroup, nativeButtonClass };
      if (openLocationGroup?.parentElement?.parentElement === menuBar) return { parent: menuBar, before: openLocationGroup.parentElement, nativeButtonClass };
      return { parent: menuBar, before: buttons[buttons.length - 1]?.nextSibling || null, nativeButtonClass: buttons[buttons.length - 1]?.className || "" };
    }
    const buttons = Array.from(contextSurface?.querySelectorAll?.("button") || [])
      .filter((button) => !button.closest(`#${codexElvesMenuId}`) && button.getBoundingClientRect().width > 0 && button.getBoundingClientRect().height > 0);
    if (buttons.length && buttons.every(isIconOnlyButton)) return null;
    const nativeButton = buttons.find((button) => !button.parentElement?.classList?.contains("inline-flex")) || buttons[0];
    const parent = nativeButton?.parentElement;
    if (!parent) {
      const emptyButtonGroup = Array.from(contextSurface?.querySelectorAll?.("div") || [])
        .find((node) => {
          const className = String(node.className || "");
          return className.includes("items-center") && className.includes("gap-2");
        });
      return emptyButtonGroup ? { parent: emptyButtonGroup, before: emptyButtonGroup.firstChild, nativeButtonClass: headerIconTextButtonClass } : null;
    }
    return { parent, before: nativeButton, nativeButtonClass: nativeButton.className || "" };
  }

  function applyCodexElvesMenuPlacement(menu, insertionPoint) {
    menu.className = insertionPoint?.menuClassName || "";
  }

  function removeDuplicateCodexElvesMenus(keep) {
    document.querySelectorAll(`#${codexElvesMenuId}, [data-codex-elves-menu="true"]`).forEach((node) => {
      if (node !== keep) node.remove();
    });
    Array.from(document.querySelectorAll("button")).forEach((button) => {
      if ((button.textContent || "").trim() === `CodexElves ${codexElvesVersion}` && !button.closest(`#${codexElvesMenuId}`)) {
        button.remove();
      }
    });
  }

  function normalizeCodexElvesTriggerClassName(className) {
    const classes = String(className || "").split(/\s+/).filter(Boolean);
    const incompatibleNativeGroupClasses = new Set([
      "gap-0",
      "rounded-l-none",
      "rounded-r-none",
      "rounded-s-none",
      "rounded-e-none",
      "border-l-0",
      "border-r-0",
      "border-s-0",
      "border-e-0",
      "pl-0.5",
      "pr-0.5",
      "ps-0.5",
      "pe-0.5",
      "pl-1.5",
      "pr-1.5",
      "ps-1.5",
      "pe-1.5",
    ]);
    const hasIncompatibleNativeGroupClass = classes.some((name) => incompatibleNativeGroupClasses.has(name));
    const normalized = classes.filter((name) => !incompatibleNativeGroupClasses.has(name));
    if (hasIncompatibleNativeGroupClass) {
      ["gap-1", "rounded-lg", "border-l", "px-2"].forEach((name) => {
        if (!normalized.includes(name)) normalized.push(name);
      });
    }
    return normalized.join(" ");
  }

  function configureCodexElvesTrigger(menu, trigger, nativeButtonClass) {
    if (!trigger) return;
    if (nativeButtonClass) trigger.className = normalizeCodexElvesTriggerClassName(nativeButtonClass);
    if (!trigger.querySelector(".codex-elves-backend-indicator")) {
      const indicator = document.createElement("span");
      indicator.className = "codex-elves-backend-indicator";
      indicator.dataset.codexBackendIndicator = "true";
      indicator.dataset.status = codexElvesBackendStatus.status || "checking";
      trigger.prepend(indicator);
    }
    if (trigger.dataset.codexElvesTriggerInstalled === "5") return;
    trigger.dataset.codexElvesTriggerInstalled = "5";
    trigger.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openCodexElvesModal();
    }, true);
  }

  function numericCssValue(value) {
    const parsed = Number.parseFloat(value || "");
    return Number.isFinite(parsed) ? parsed : 0;
  }

  function setCssPropIfChanged(menu, prop, value) {
    if (menu.style.getPropertyValue(prop) !== value) {
      menu.style.setProperty(prop, value);
    }
  }

  function headerTitleRegion(header) {
    const candidates = Array.from(header?.querySelectorAll?.('[data-state], [class*="truncate"], [class*="text-base"]') || []);
    return candidates.find((node) => {
      if (!node?.querySelector?.('[data-state], button')) return false;
      if (!node.textContent?.trim()) return false;
      return node.closest?.(".draggable") || node.closest?.('[class*="grid-cols-[minmax(0,1fr)]"]');
    }) || null;
  }

  function isHeaderToolbarButton(button, header, rect) {
    if (!button || button.closest?.(`#${codexElvesMenuId}`)) return false;
    if (!(rect.width > 0 && rect.height > 0 && rect.left > window.innerWidth / 2)) return false;
    const buttonCluster = button.closest(".ms-auto.flex.shrink-0.items-center");
    if (buttonCluster && header?.contains(buttonCluster)) return true;
    const titleRegion = headerTitleRegion(header);
    if (titleRegion?.contains?.(button)) return false;
    return !!button.closest?.('[class*="ms-auto"][class*="shrink-0"][class*="items-center"]');
  }

  function updateFloatingCodexElvesMenuPosition(menu) {
    if (!menu?.classList?.contains(codexElvesMenuFloatingClass)) return;
    const header = document.querySelector(selectors.appHeader) || document.querySelector("header");
    if (!header) return;
    const toolbarButtons = Array.from(header.querySelectorAll("button"))
      .map((button) => ({ button, rect: button.getBoundingClientRect() }))
      .filter(({ button, rect }) => isHeaderToolbarButton(button, header, rect))
      .sort((left, right) => left.rect.left - right.rect.left);
    const anchor = toolbarButtons[0];
    if (anchor) {
      const measuredGap = toolbarButtons[1] ? toolbarButtons[1].rect.left - toolbarButtons[0].rect.right : 0;
      const styles = anchor.button.parentElement ? getComputedStyle(anchor.button.parentElement) : null;
      const gap = Math.max(numericCssValue(styles?.columnGap || styles?.gap), measuredGap, 0);
      setCssPropIfChanged(menu, "--codex-elves-menu-top", `${anchor.rect.top}px`);
      setCssPropIfChanged(menu, "--codex-elves-menu-height", `${anchor.rect.height}px`);
      setCssPropIfChanged(menu, "--codex-elves-menu-right", `${Math.max(0, window.innerWidth - anchor.rect.left + gap)}px`);
      return;
    }

    const headerRect = header.getBoundingClientRect();
    if (headerRect.height) {
      setCssPropIfChanged(menu, "--codex-elves-menu-top", `${headerRect.top}px`);
      setCssPropIfChanged(menu, "--codex-elves-menu-height", `${headerRect.height}px`);
    }
    menu.style.removeProperty("--codex-elves-menu-right");
  }

  function installCodexElvesMenu() {
    const existing = document.getElementById(codexElvesMenuId);
    removeDuplicateCodexElvesMenus(existing);
    let insertionPoint = findNativeMenuInsertionPoint();
    if (existing && existing.dataset.codexElvesMenuVersion !== codexElvesMenuVersion) {
      existing.remove();
      insertionPoint = findNativeMenuInsertionPoint();
    } else if (existing && insertionPoint && existing.parentElement === insertionPoint.parent) {
      configureCodexElvesTrigger(existing, existing.querySelector("button"), insertionPoint.nativeButtonClass);
      applyCodexElvesMenuPlacement(existing, insertionPoint);
      const safeBefore = insertionPoint.before?.parentElement === insertionPoint.parent ? insertionPoint.before : null;
      if (existing.nextSibling !== safeBefore) insertionPoint.parent.insertBefore(existing, safeBefore);
      removeDuplicateCodexElvesMenus(existing);
      return;
    } else if (existing && insertionPoint) {
      configureCodexElvesTrigger(existing, existing.querySelector("button"), insertionPoint.nativeButtonClass);
      applyCodexElvesMenuPlacement(existing, insertionPoint);
      const safeBefore = insertionPoint.before?.parentElement === insertionPoint.parent ? insertionPoint.before : null;
      insertionPoint.parent.insertBefore(existing, safeBefore);
      removeDuplicateCodexElvesMenus(existing);
      return;
    } else if (existing) {
      configureCodexElvesTrigger(existing, existing.querySelector("button"), headerIconTextButtonClass);
      existing.className = codexElvesMenuFloatingClass;
      document.documentElement.appendChild(existing);
      updateFloatingCodexElvesMenuPosition(existing);
      removeDuplicateCodexElvesMenus(existing);
      return;
    }
    const menu = document.createElement("div");
    menu.id = codexElvesMenuId;
    menu.dataset.codexElvesMenu = "true";
    menu.dataset.codexElvesMenuVersion = codexElvesMenuVersion;
    const trigger = document.createElement("button");
    trigger.type = "button";
    const indicator = ensureCodexElvesTriggerIndicator(trigger);
    if (indicator) indicator.dataset.status = codexElvesBackendStatus.status || "checking";
    setCodexElvesTriggerLabel(trigger);
    const nativeButtonClass = insertionPoint?.nativeButtonClass || headerIconTextButtonClass;
    configureCodexElvesTrigger(menu, trigger, nativeButtonClass);
    menu.appendChild(trigger);
    if (insertionPoint) {
      applyCodexElvesMenuPlacement(menu, insertionPoint);
      const safeBefore = insertionPoint.before?.parentElement === insertionPoint.parent ? insertionPoint.before : null;
      insertionPoint.parent.insertBefore(menu, safeBefore);
    } else {
      menu.className = codexElvesMenuFloatingClass;
      document.documentElement.appendChild(menu);
      updateFloatingCodexElvesMenuPosition(menu);
    }
    removeDuplicateCodexElvesMenus(menu);
  }

  function patchPluginMarketplaceRequestParams(method, params) {
    if (method === "list-plugins") {
      if (!params || typeof params !== "object") return params;
    } else {
      return params;
    }
    const next = { ...params };
    const hadMarketplaceKinds = Object.prototype.hasOwnProperty.call(next, "marketplaceKinds");
    const unsupportedMarketplaceKinds = [];
    if (hadMarketplaceKinds && Array.isArray(next.marketplaceKinds)) {
      const nextKinds = next.marketplaceKinds.map((kind) => restorePluginMarketplaceName(kind));
      nextKinds.forEach((kind) => {
        if (codexPluginApiKeyUnsupportedMarketplaceKinds.has(kind)) {
          unsupportedMarketplaceKinds.push(kind);
        }
      });
      if (unsupportedMarketplaceKinds.length === 0 && !nextKinds.includes("vertical")) {
        nextKinds.push("vertical");
      }
      next.marketplaceKinds = Array.from(new Set(nextKinds));
    }
    sendCodexElvesDiagnostic("plugin_marketplace_request_expanded", {
      hadMarketplaceKinds,
      marketplaceKinds: Array.isArray(next.marketplaceKinds) ? next.marketplaceKinds : null,
      unsupportedMarketplaceKinds,
      cwdCount: Array.isArray(next.cwds) ? next.cwds.length : 0,
    });
    return next;
  }

  function unsupportedPluginMarketplaceKinds(method, params) {
    if (method !== "list-plugins" || !Array.isArray(params?.marketplaceKinds)) return [];
    return Array.from(new Set(
      params.marketplaceKinds
        .map((kind) => restorePluginMarketplaceName(kind))
        .filter((kind) => codexPluginApiKeyUnsupportedMarketplaceKinds.has(kind))
    ));
  }

  function emptyPluginMarketplaceResult() {
    return {
      marketplaces: [],
      marketplaceLoadErrors: [],
      featuredPluginIds: [],
    };
  }

  function cloneCodexPluginMarketplace(value) {
    if (!value || typeof value !== "object") return null;
    try {
      return JSON.parse(JSON.stringify(value));
    } catch {
      return null;
    }
  }

  function pluginMarketplacePluginKey(plugin) {
    if (!plugin || typeof plugin !== "object") return "";
    return String(plugin.name || plugin.id || plugin.pluginName || "").trim();
  }

  function normalizeLocalPluginMarketplacePlugin(plugin, marketplaceName) {
    const cloned = cloneCodexPluginMarketplace(plugin);
    if (!cloned || typeof cloned !== "object") return null;
    const name = String(cloned.name || cloned.id || cloned.pluginName || "").trim();
    if (!name) return null;
    if (!cloned.name) cloned.name = name;
    if (!cloned.id) cloned.id = `${name}@${marketplaceName}`;
    if (!cloned.marketplaceName) cloned.marketplaceName = marketplaceName;
    if (!cloned.marketplacePath) cloned.marketplacePath = `remote:${marketplaceName}`;
    if (!cloned.interface || typeof cloned.interface !== "object") cloned.interface = {};
    if (!cloned.interface.displayName) cloned.interface.displayName = name;
    if (!Array.isArray(cloned.keywords)) cloned.keywords = [];
    return cloned;
  }

  function mergePluginMarketplacePlugins(target, source) {
    if (!target || !source || !Array.isArray(source.plugins)) return 0;
    if (!Array.isArray(target.plugins)) target.plugins = [];
    const marketplaceName = restorePluginMarketplaceName(target.name || source.name || "");
    const existing = new Set(target.plugins.map(pluginMarketplacePluginKey).filter(Boolean));
    let added = 0;
    source.plugins.forEach((plugin) => {
      const key = pluginMarketplacePluginKey(plugin);
      if (!key || existing.has(key)) return;
      const cloned = normalizeLocalPluginMarketplacePlugin(plugin, marketplaceName);
      if (!cloned) return;
      target.plugins.push(cloned);
      existing.add(key);
      added += 1;
    });
    return added;
  }

  function mergeLocalPluginMarketplaces(result) {
    if (!result || typeof result !== "object" || !Array.isArray(result.marketplaces)) {
      return { addedMarketplaces: 0, addedPlugins: 0 };
    }
    const localMarketplaces = Array.isArray(window.__CODEX_ELVES_PLUGIN_MARKETPLACES__)
      ? window.__CODEX_ELVES_PLUGIN_MARKETPLACES__
      : [];
    if (!localMarketplaces.length) return { addedMarketplaces: 0, addedPlugins: 0 };
    const byName = new Map();
    result.marketplaces.forEach((marketplace) => {
      const name = restorePluginMarketplaceName(marketplace?.name || "");
      if (name) byName.set(name, marketplace);
    });
    let addedMarketplaces = 0;
    let addedPlugins = 0;
    localMarketplaces.forEach((marketplace) => {
      const name = restorePluginMarketplaceName(marketplace?.name || "");
      if (!name) return;
      const existing = byName.get(name);
      if (existing) {
        addedPlugins += mergePluginMarketplacePlugins(existing, marketplace);
        return;
      }
      const cloned = cloneCodexPluginMarketplace(marketplace);
      if (!cloned) return;
      cloned.plugins = Array.isArray(cloned.plugins)
        ? cloned.plugins.map((plugin) => normalizeLocalPluginMarketplacePlugin(plugin, name)).filter(Boolean)
        : [];
      result.marketplaces.push(cloned);
      byName.set(name, cloned);
      addedMarketplaces += 1;
      addedPlugins += Array.isArray(cloned.plugins) ? cloned.plugins.length : 0;
    });
    if (addedMarketplaces > 0 || addedPlugins > 0) {
      sendCodexElvesDiagnostic("plugin_marketplace_local_merged", { addedMarketplaces, addedPlugins });
    }
    return { addedMarketplaces, addedPlugins };
  }

  function restorePluginMarketplaceName(name) {
    if (name === "codex-elves-openai-bundled") return "openai-bundled";
    if (name === "codex-elves-openai-curated") return "openai-curated";
    if (name === "codex-elves-openai-primary-runtime") return "openai-primary-runtime";
    if (name === "codex-elves-openai-api-curated") return "openai-api-curated";
    if (name === "codex-elves-openai-curated-remote") return "openai-curated-remote";
    return name;
  }

  function restorePluginMarketplaceRequestParams(params, method = "") {
    if (!params || typeof params !== "object") return params;
    let next = params;
    if (Array.isArray(params.marketplaceKinds)) {
      const nextKinds = params.marketplaceKinds.map((kind) => {
        if (kind === "remote:openai-curated") return "openai-curated";
        return restorePluginMarketplaceName(kind);
      });
      next = { ...next, marketplaceKinds: Array.from(new Set(nextKinds)) };
    }
    if (method === "install-plugin") {
      next = next === params ? { ...params } : { ...next };
      if (next.remoteMarketplaceName) next.remoteMarketplaceName = restorePluginMarketplaceName(next.remoteMarketplaceName);
      if (typeof next.marketplacePath === "string" && next.marketplacePath.startsWith("remote:")) {
        const remoteMarketplaceName = next.marketplacePath.slice("remote:".length);
        delete next.marketplacePath;
        next.remoteMarketplaceName = restorePluginMarketplaceName(remoteMarketplaceName);
      }
    }
    return next;
  }

  function patchPluginMarketplaceResult(method, result) {
    if (method !== "list-plugins") return result;
    try {
      const pluginMarketplaceCounts = {};
      if (Array.isArray(result?.marketplaces)) {
        mergeLocalPluginMarketplaces(result);
        result.marketplaces.forEach((marketplace) => {
          if (Array.isArray(marketplace?.plugins)) {
            marketplace.plugins.forEach((plugin) => {
              const name = plugin?.marketplaceName || marketplace?.name || "";
              if (name) pluginMarketplaceCounts[name] = (pluginMarketplaceCounts[name] || 0) + 1;
            });
          }
        });
        sendCodexElvesDiagnostic("plugin_marketplace_response_debug", {
          marketplaces: result.marketplaces.map((marketplace) => ({
            name: marketplace?.name || "",
            path: marketplace?.path || null,
            displayName: marketplace?.displayName || marketplace?.interface?.displayName || null,
            pluginCount: Array.isArray(marketplace?.plugins) ? marketplace.plugins.length : null,
            remoteMarketplaceName: marketplace?.remoteMarketplaceName || null,
          })),
          pluginMarketplaceCounts,
        });
      }
    } catch (error) {
      sendCodexElvesDiagnostic("plugin_marketplace_response_patch_failed", {
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
    }
    return result;
  }

  function patchPluginMarketplaceRequestClient(client) {
    if (!client || typeof client.sendRequest !== "function") return false;
    if (client.__codexPluginMarketplaceUnlockPatch === codexPluginMarketplaceUnlockVersion) return true;
    const originalSendRequest = client.__codexPluginMarketplaceOriginalSendRequest || client.sendRequest.bind(client);
    client.__codexPluginMarketplaceOriginalSendRequest = originalSendRequest;
    client.sendRequest = async function codexPluginMarketplacePatchedSendRequest(method, params, options) {
      const requestMethod = appServerRequestMethod(String(method || ""), params);
      const restoredRequestParams = restorePluginMarketplaceRequestParams(params, requestMethod);
      const unsupportedKinds = unsupportedPluginMarketplaceKinds(requestMethod, restoredRequestParams);
      if (unsupportedKinds.length > 0) {
        sendCodexElvesDiagnostic("plugin_marketplace_request_skipped_unsupported_auth", {
          method: String(method || ""),
          requestMethod,
          unsupportedKinds,
        });
        return emptyPluginMarketplaceResult();
      }
      const requestParams = patchPluginMarketplaceRequestParams(requestMethod, restoredRequestParams);
      if (requestMethod === "install-plugin") {
        sendCodexElvesDiagnostic("plugin_install_request_debug", {
          method: String(method || ""),
          requestMethod,
          originalMarketplacePath: params?.marketplacePath || null,
          originalRemoteMarketplaceName: params?.remoteMarketplaceName || null,
          originalPluginName: params?.pluginName || null,
          requestMarketplacePath: requestParams?.marketplacePath || null,
          requestRemoteMarketplaceName: requestParams?.remoteMarketplaceName || null,
          requestPluginName: requestParams?.pluginName || null,
        });
      }
      try {
        const result = await originalSendRequest(method, requestParams, options);
        return patchPluginMarketplaceResult(requestMethod, result);
      } catch (error) {
        if (requestMethod === "install-plugin") {
          sendCodexElvesDiagnostic("plugin_install_request_failed", {
            method: String(method || ""),
            requestMethod,
            requestMarketplacePath: requestParams?.marketplacePath || null,
            requestRemoteMarketplaceName: requestParams?.remoteMarketplaceName || null,
            requestPluginName: requestParams?.pluginName || null,
            errorName: error?.name || "",
            errorMessage: error?.message || String(error),
          });
        }
        throw error;
      }
    };
    client.__codexPluginMarketplaceUnlockPatch = codexPluginMarketplaceUnlockVersion;
    return true;
  }

  function patchPluginMarketplaceRequestMessage(message) {
    if (!message || typeof message !== "object") return message;
    if (message.type === "fetch" && typeof message.url === "string") {
      const requestMethod = appServerRequestMethod(message.url, message.body);
      if (requestMethod !== "list-plugins" && requestMethod !== "install-plugin") return message;
      let requestBody = message.body;
      let params = null;
      if (typeof requestBody === "string" && requestBody.trim()) {
        try {
          params = JSON.parse(requestBody);
        } catch {
          params = null;
        }
      } else if (requestBody && typeof requestBody === "object") {
        params = requestBody;
      }
      const requestParams = patchPluginMarketplaceRequestParams(
        requestMethod,
        restorePluginMarketplaceRequestParams(params, requestMethod)
      );
      if (requestMethod === "list-plugins" && message.requestId != null) {
        rememberCodexPluginRequestId("__codexPluginMarketplaceFetchRequestIds", message.requestId);
      }
      if (requestParams === params) return message;
      if (requestMethod === "install-plugin") {
        sendCodexElvesDiagnostic("plugin_install_request_debug", {
          method: message.url,
          requestMethod,
          originalMarketplacePath: params?.marketplacePath || null,
          originalRemoteMarketplaceName: params?.remoteMarketplaceName || null,
          originalPluginName: params?.pluginName || null,
          requestMarketplacePath: requestParams?.marketplacePath || null,
          requestRemoteMarketplaceName: requestParams?.remoteMarketplaceName || null,
          requestPluginName: requestParams?.pluginName || null,
        });
      }
      return {
        ...message,
        body: typeof requestBody === "string" ? JSON.stringify(requestParams) : requestParams,
      };
    }
    if (message.type === "mcp-request" && message.request && typeof message.request === "object") {
      const requestMethod = appServerRequestMethod(String(message.request.method || ""), message.request.params);
      if (requestMethod !== "list-plugins" && requestMethod !== "install-plugin") return message;
      const requestParams = patchPluginMarketplaceRequestParams(
        requestMethod,
        restorePluginMarketplaceRequestParams(message.request.params, requestMethod)
      );
      if (requestMethod === "list-plugins" && message.request.id != null) {
        rememberCodexPluginRequestId("__codexPluginMarketplaceRequestIds", message.request.id);
      }
      if (requestParams === message.request.params) return message;
      if (requestMethod === "install-plugin") {
        sendCodexElvesDiagnostic("plugin_install_request_debug", {
          method: String(message.request.method || ""),
          requestMethod,
          originalMarketplacePath: message.request.params?.marketplacePath || null,
          originalRemoteMarketplaceName: message.request.params?.remoteMarketplaceName || null,
          originalPluginName: message.request.params?.pluginName || null,
          requestMarketplacePath: requestParams?.marketplacePath || null,
          requestRemoteMarketplaceName: requestParams?.remoteMarketplaceName || null,
          requestPluginName: requestParams?.pluginName || null,
        });
      }
      return { ...message, request: { ...message.request, params: requestParams } };
    }
    return message;
  }

  function patchPluginMarketplaceResponseData(data) {
    if (data?.type === "fetch-response") {
      const requestId = data.requestId != null ? String(data.requestId) : "";
      if (!consumeCodexPluginRequestId("__codexPluginMarketplaceFetchRequestIds", requestId)) return false;
      if (typeof data.bodyJsonString !== "string" || !data.bodyJsonString.trim()) return false;
      try {
        const result = JSON.parse(data.bodyJsonString);
        if (result && typeof result === "object") {
          patchPluginMarketplaceResult("list-plugins", result);
          patchPluginMarketplaceResult("list-plugins", result.data);
        }
        data.bodyJsonString = JSON.stringify(result);
        return true;
      } catch (error) {
        sendCodexElvesDiagnostic("plugin_marketplace_fetch_response_patch_failed", {
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
        });
      }
      return false;
    }
    if (data?.type !== "mcp-response") return false;
    const message = data.message || data.response;
    const method = String(message?.method || data.method || "");
    if (appServerRequestMethod(method) === "install-plugin") {
      clearPluginMarketplaceQueryCache();
    }
    const requestId = message?.id != null ? String(message.id) : "";
    if (!consumeCodexPluginRequestId("__codexPluginMarketplaceRequestIds", requestId)) return false;
    const result = message?.result;
    if (!result || typeof result !== "object") return false;
    patchPluginMarketplaceResult("list-plugins", result);
    patchPluginMarketplaceResult("list-plugins", result.data);
    return true;
  }

  function clearPluginMarketplaceQueryCache() {
    try {
      const queryClient = window.__REACT_QUERY_CLIENT__ || window.__codexQueryClient;
      if (queryClient && typeof queryClient.invalidateQueries === "function") {
        queryClient.invalidateQueries({ queryKey: ["plugins"] });
      }
    } catch {
    }
  }

  function installPluginMarketplaceBridgePatch() {
    if (window.__codexPluginMarketplaceBridgePatch === codexPluginMarketplaceUnlockVersion) return;
    if (pluginPatchDisabledInRelayMode()) return;
    if (!codexElvesSettings().pluginMarketplaceUnlock) return;
    installPluginMarketplaceWindowEventPatchOnly();
    const bridge = window.electronBridge;
    if (!bridge || typeof bridge.sendMessageFromView !== "function") {
      sendCodexElvesDiagnostic("plugin_marketplace_bridge_patch_not_found", {});
      return;
    }
    if (!bridge.__codexPluginMarketplaceOriginalSendMessageFromView) {
      bridge.__codexPluginMarketplaceOriginalSendMessageFromView = bridge.sendMessageFromView.bind(bridge);
      const patchedSendMessageFromView = function codexPluginMarketplacePatchedSendMessageFromView(message) {
        let nextMessage = message;
        try {
          nextMessage = patchPluginMarketplaceRequestMessage(message);
        } catch (error) {
          sendCodexElvesDiagnostic("plugin_marketplace_bridge_request_patch_failed", {
            errorName: error?.name || "",
            errorMessage: error?.message || String(error),
          });
        }
        return bridge.__codexPluginMarketplaceOriginalSendMessageFromView(nextMessage);
      };
      bridge.sendMessageFromView = patchedSendMessageFromView;
      if (bridge.sendMessageFromView !== patchedSendMessageFromView) {
        delete bridge.__codexPluginMarketplaceOriginalSendMessageFromView;
        sendCodexElvesDiagnostic("plugin_marketplace_bridge_patch_not_writable", {});
        return;
      }
    }
    bridge.__codexPluginMarketplaceBridgePatch = codexPluginMarketplaceUnlockVersion;
    window.__codexPluginMarketplaceBridgePatch = codexPluginMarketplaceUnlockVersion;
    sendCodexElvesDiagnostic("plugin_marketplace_bridge_patch_installed", {});
  }

  function installPluginMarketplaceWindowEventPatchOnly() {
    if (window.__codexPluginMarketplaceWindowEventPatch === codexPluginMarketplaceUnlockVersion) return;
    if (pluginPatchDisabledInRelayMode()) return;
    if (!codexElvesSettings().pluginMarketplaceUnlock) return;
    const originalDispatchEvent = window.__codexPluginMarketplaceOriginalDispatchEvent || window.dispatchEvent;
    if (!window.__codexPluginMarketplaceOriginalDispatchEvent) {
      window.__codexPluginMarketplaceOriginalDispatchEvent = originalDispatchEvent;
      window.dispatchEvent = function patchedCodexPluginMarketplaceDispatchEvent(event) {
        try {
          const detail = event?.detail;
          if (event?.type === "codex-message-from-view" && detail?.type === "mcp-request") {
            const patched = patchPluginMarketplaceRequestMessage(detail);
            if (patched !== detail) {
              Object.keys(detail).forEach((key) => delete detail[key]);
              Object.assign(detail, patched);
            }
          }
          if (event?.type === "message") patchPluginMarketplaceResponseData(event.data);
        } catch (error) {
          sendCodexElvesDiagnostic("plugin_marketplace_dispatch_event_patch_failed", {
            errorName: error?.name || "",
            errorMessage: error?.message || String(error),
          });
        }
        return originalDispatchEvent.call(this, event);
      };
    }
    if (!window.__codexPluginMarketplaceResponseListenerInstalled) {
      window.__codexPluginMarketplaceResponseListenerInstalled = true;
      window.addEventListener("message", (event) => {
        try {
          patchPluginMarketplaceResponseData(event?.data);
        } catch (error) {
          sendCodexElvesDiagnostic("plugin_marketplace_response_message_patch_failed", {
            errorName: error?.name || "",
            errorMessage: error?.message || String(error),
          });
        }
      }, true);
    }
    window.__codexPluginMarketplaceWindowEventPatch = codexPluginMarketplaceUnlockVersion;
  }

  function installPluginMarketplaceRequestPatch() {
    if (window.__codexPluginMarketplaceUnlockInstalled === codexPluginMarketplaceUnlockVersion) return;
    if (pluginPatchDisabledInRelayMode()) return;
    if (!codexElvesSettings().pluginMarketplaceUnlock) return;
    const patch = async () => {
      try {
        let patchedCount = 0;
        let manager = codexSessionPrewarmManager || window.__codexElvesSessionPrewarmManager || null;
        if (!manager) {
          try {
            manager = findCodexSessionPrewarmManagerInReactTree(true).manager;
          } catch {
          }
        }
        if (patchPluginMarketplaceRequestClient(manager?.requestClient)) patchedCount += 1;
        let module = null;
        let candidates = [];
        if (patchedCount === 0) {
          module = await loadCodexAppModule("app-server-manager-signals-");
          candidates = Object.values(module).filter((value) => value && typeof value === "object");
          for (const candidate of candidates) {
            if (patchPluginMarketplaceRequestClient(candidate)) patchedCount += 1;
            if (typeof candidate.sendRequest !== "function" && typeof candidate.get === "function") {
              try {
                if (patchPluginMarketplaceRequestClient(candidate.get())) patchedCount += 1;
              } catch {
              }
            }
          }
        }
        if (patchedCount > 0) {
          window.__codexPluginMarketplaceUnlockInstalled = codexPluginMarketplaceUnlockVersion;
          sendCodexElvesDiagnostic("plugin_marketplace_request_patch_installed", {
            managerFound: !!manager,
            candidateCount: candidates.length,
            patchedCount,
          });
        } else {
          sendCodexElvesDiagnostic("plugin_marketplace_request_patch_not_found", {
            exportCount: Object.keys(module || {}).length,
            candidateCount: candidates.length,
          });
        }
      } catch (error) {
        sendCodexElvesDiagnostic("plugin_marketplace_request_patch_failed", {
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
        });
      }
    };
    void patch();
  }

  function reactFiberFrom(element) {
    const fiberKey = Object.keys(element).find((key) => key.startsWith("__reactFiber"));
    return fiberKey ? element[fiberKey] : null;
  }

  function authContextValueFrom(element) {
    for (let fiber = reactFiberFrom(element); fiber; fiber = fiber.return) {
      for (const value of [fiber.memoizedProps?.value, fiber.pendingProps?.value]) {
        if (value && typeof value === "object" && typeof value.setAuthMethod === "function" && "authMethod" in value) {
          return value;
        }
      }
    }
    return null;
  }

  function spoofChatGPTAuthMethod(element) {
    const auth = authContextValueFrom(element);
    if (!auth || auth.authMethod === "chatgpt") return false;
    auth.setAuthMethod("chatgpt");
    return true;
  }

  function normalizedPluginEntryLabel(value) {
    return String(value || "").replace(/\s+/g, " ").trim();
  }

  function pluginEntryControlMatches(control) {
    return [
      control?.getAttribute?.("aria-label"),
      control?.getAttribute?.("title"),
      control?.textContent,
    ].some((value) => pluginEntryLabelPattern.test(normalizedPluginEntryLabel(value)));
  }

  function pluginNavigationControls() {
    return Array.from(new Set(document.querySelectorAll(pluginNavigationControlSelector)));
  }

  function pluginEntryButton() {
    const navigationControls = pluginNavigationControls();
    const bySemanticLabel = navigationControls.find(pluginEntryControlMatches);
    if (bySemanticLabel) return bySemanticLabel;

    const byLegacyIcon = navigationControls.find((control) => {
      return !!control?.querySelector?.(selectors.pluginSvgPath);
    });
    if (byLegacyIcon) return byLegacyIcon;

    const globalSemanticMatches = Array.from(
      document.querySelectorAll('button, [role="button"], a[href]'),
    ).filter(pluginEntryControlMatches);
    return globalSemanticMatches.length === 1 ? globalSemanticMatches[0] : null;
  }

  function labelUnlockedPluginEntry(button) {
    const labelTextNode = Array.from(button.querySelectorAll("span, div")).reverse()
      .flatMap((node) => Array.from(node.childNodes))
      .find((node) => node.nodeType === 3 && /^(插件|Plugins)( - 已解锁| - Unlocked)?$/i.test((node.nodeValue || "").trim()));
    if (!labelTextNode) return;
    const current = (labelTextNode.nodeValue || "").trim();
    labelTextNode.nodeValue = /^Plugins/i.test(current) ? "Plugins - Unlocked" : "插件 - 已解锁";
  }

  function clearPluginEntryUnlockLabel(button) {
    const labelTextNode = Array.from(button.querySelectorAll("span, div")).reverse()
      .flatMap((node) => Array.from(node.childNodes))
      .find((node) => node.nodeType === 3 && /^(插件 - 已解锁|Plugins - Unlocked)$/i.test((node.nodeValue || "").trim()));
    if (!labelTextNode) return;
    labelTextNode.nodeValue = /^Plugins/i.test((labelTextNode.nodeValue || "").trim()) ? "Plugins" : "插件";
  }

  function enablePluginEntry() {
    if (pluginPatchDisabledInRelayMode()) return;
    if (!codexElvesSettings().pluginEntryUnlock) return;
    const pluginButton = pluginEntryButton();
    if (!pluginButton) return;
    const spoofed = spoofChatGPTAuthMethod(pluginButton);
    pluginButton.disabled = false;
    pluginButton.removeAttribute("disabled");
    pluginButton.style.display = "";
    pluginButton.querySelectorAll("*").forEach((node) => {
      node.style.display = "";
    });
    labelUnlockedPluginEntry(pluginButton);
    const reactPropsKey = Object.keys(pluginButton).find((key) => key.startsWith("__reactProps"));
    if (reactPropsKey) {
      pluginButton[reactPropsKey].disabled = false;
    }
    if (pluginButton.dataset.codexPluginEnabled !== "true") {
      pluginButton.dataset.codexPluginEnabled = "true";
      pluginButton.addEventListener("click", () => {
        spoofChatGPTAuthMethod(pluginButton);
      }, true);
    }
    sendCodexElvesDiagnostic("plugin_entry_unlock_applied", { spoofed });
  }

  function pluginPatchDisabledInRelayMode() {
    return !codexElvesBackendSettingsLoaded || codexElvesBackendSettings.launchMode === "relay";
  }

  function clearPluginPatchArtifacts() {
    const pluginButton = pluginEntryButton();
    if (pluginButton) {
      delete pluginButton.dataset.codexPluginEnabled;
      clearPluginEntryUnlockLabel(pluginButton);
    }
  }

  let cachedSessionRows = [];
  let cachedSessionRowsDirty = true;
  const pendingSessionRows = new Set();
  const pendingSessionRowLayouts = new Set();
  let pendingSessionRowsMutationScoped = false;
  let pendingSessionRowLayoutRafId = 0;

  function invalidateSessionRowsCache() {
    cachedSessionRowsDirty = true;
  }

  function sessionRows(forceRefresh = false) {
    if (!forceRefresh && !cachedSessionRowsDirty) {
      cachedSessionRows = cachedSessionRows.filter((row) => row.isConnected);
      if (cachedSessionRows.length > 0) return cachedSessionRows;
    }

    cachedSessionRows = Array.from(document.querySelectorAll(selectors.sidebarThread));
    cachedSessionRowsDirty = false;
    return cachedSessionRows;
  }

  function sessionRowsFromNode(node) {
    if (!(node instanceof Element)) return [];
    const rows = new Set();
    if (node.matches?.(selectors.sidebarThread)) rows.add(node);
    const closest = node.closest?.(selectors.sidebarThread);
    if (closest) rows.add(closest);
    node.querySelectorAll?.(selectors.sidebarThread).forEach((row) => rows.add(row));
    return Array.from(rows);
  }

  function cleanupDisconnectedSessionRow(row) {
    pendingSessionRows.delete(row);
    pendingSessionRowLayouts.delete(row);
    document.querySelectorAll(`.${moreMenuClass}`).forEach((menu) => {
      if (menu.__codexSessionMoreRow === row) menu.remove();
    });
  }

  function cleanupDisconnectedSessionArtifacts() {
    document.querySelectorAll(`.${moreMenuClass}`).forEach((menu) => {
      const row = menu.__codexSessionMoreRow;
      if (row && !row.isConnected) {
        cleanupDisconnectedSessionRow(row);
      }
    });
    for (const row of pendingSessionRows) {
      if (!row?.isConnected) pendingSessionRows.delete(row);
    }
    for (const row of pendingSessionRowLayouts) {
      if (!row?.isConnected) pendingSessionRowLayouts.delete(row);
    }
  }

  function collectPendingSessionRows(mutations) {
    pendingSessionRowsMutationScoped = true;
    for (const mutation of Array.from(mutations || [])) {
      sessionRowsFromNode(mutation.target).forEach((row) => {
        if (row.isConnected) pendingSessionRows.add(row);
      });
      Array.from(mutation.addedNodes || []).forEach((node) => {
        sessionRowsFromNode(node).forEach((row) => {
          if (row.isConnected) pendingSessionRows.add(row);
        });
      });
      Array.from(mutation.removedNodes || []).forEach((node) => {
        sessionRowsFromNode(node).forEach(cleanupDisconnectedSessionRow);
      });
    }
    invalidateSessionRowsCache();
  }

  function takePendingSessionRows() {
    const scoped = pendingSessionRowsMutationScoped;
    pendingSessionRowsMutationScoped = false;
    const rows = scoped
      ? Array.from(pendingSessionRows)
      : sessionRows();
    pendingSessionRows.clear();
    return {
      rows: rows.filter((row) => row?.isConnected),
      scoped,
    };
  }

  function archivePageHintVisible() {
    if (window.location.href.includes("archive")) return true;
    if (document.querySelector('[data-codex-archive-page-row="true"]')) return true;
    const archiveNav = document.querySelector(selectors.archiveNav);
    if (archiveNav?.className?.includes?.("bg-token-list-hover-background")) return true;
    return !!Array.from(document.querySelectorAll("h1, h2, h3")).find((element) => (element.textContent || "").trim() === "已归档对话");
  }

  function archiveRowFromUnarchiveButton(button) {
    return button.closest('[data-codex-archive-page-row="true"]')
      || button.closest('[role="listitem"], [role="row"]')
      || button.closest(".flex.w-full.items-center.justify-between")
      || button.parentElement;
  }

  function archivedPageRows() {
    if (!archivePageHintVisible()) return [];
    const rows = Array.from(document.querySelectorAll("button")).filter((button) => (button.textContent || "").trim() === "取消归档").map(archiveRowFromUnarchiveButton).filter(Boolean);
    rows.forEach((row) => {
      row.dataset.codexArchivePageRow = "true";
      row.setAttribute("data-codex-archive-page-row", "true");
    });
    return rows;
  }

  function sessionRefFromRow(row) {
    const href = row.getAttribute("href") || row.querySelector("a")?.getAttribute("href") || "";
    const idMatch = href.match(/(?:session|conversation|thread)[=/:-]([A-Za-z0-9_.-]+)/i) || href.match(/([A-Za-z0-9_-]{8,})$/);
    const codexThreadId = row.getAttribute("data-app-action-sidebar-thread-id") || "";
    const fallbackId = row.getAttribute("data-session-id") || row.getAttribute("data-testid") || "";
    const sessionId = codexThreadId || (idMatch && idMatch[1]) || fallbackId;
    const titleNode = row.querySelector(`${selectors.threadTitle}, .truncate.select-none, .truncate.text-base`);
    const rawTitle = (titleNode?.textContent || (titleNode ? "" : (row.textContent || "Untitled session")));
    const title = (titleNode ? rawTitle : rawTitle.replace(/\s*(导出|删除|移动|移出项目)(\s*(导出|删除|移动|移出项目))*$/g, "")).trim().slice(0, 160);
    return { session_id: sessionId, title };
  }

  function codexElvesDiagnosticPayload(event, detail) {
    return {
      event,
      detail: detail || {},
      helperBase,
      hasBridge: !!window.__codexSessionDeleteBridge,
      location: window.location?.href || "",
      userAgent: navigator.userAgent || "",
      timestamp: new Date().toISOString(),
    };
  }

  function sendCodexElvesDiagnostic(event, detail) {
    const payload = codexElvesDiagnosticPayload(event, detail);
    if (window.__CODEX_ELVES_TEST_SERVICE_TIER__) {
      window.__codexElvesServiceTierTestDiagnostics = window.__codexElvesServiceTierTestDiagnostics || [];
      window.__codexElvesServiceTierTestDiagnostics.push(payload);
      return;
    }
    if (window.__codexSessionDeleteBridge) {
      try {
        Promise.resolve(window.__codexSessionDeleteBridge("/diagnostics/log", payload))
          .catch(() => sendCodexElvesDiagnosticOverHttp(payload));
        return;
      } catch (_) {}
    }
    sendCodexElvesDiagnosticOverHttp(payload);
  }

  function sendCodexElvesDiagnosticOverHttp(payload) {
    const body = JSON.stringify(payload);
    try {
      if (navigator.sendBeacon) {
        const blob = new Blob([body], { type: "application/json" });
        if (navigator.sendBeacon(`${helperBase}/diagnostics/log`, blob)) return;
      }
    } catch (_) {}
    fetch(`${helperBase}/diagnostics/log`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body,
      keepalive: true,
    }).catch(() => {});
  }

  sendCodexElvesDiagnostic("script_loaded", {
    version: codexElvesVersion,
    build: codexElvesBuild,
  });

  // 记录首次注入时间（跨 refresh 保留），作为“启动早期窗口”基准。
  window.__codexElvesScriptLoadedAt = window.__codexElvesScriptLoadedAt || Date.now();

  function locationThreadId() {
    const source = `${window.location.pathname}${window.location.search}${window.location.hash}`;
    const match = source.match(/\/local\/([A-Za-z0-9_.-]{8,128})(?:[/?#]|$)/i)
      || source.match(/(?:session|conversation|thread)(?:\/|=|:|-)([A-Za-z0-9_.-]+)/i)
      || source.match(/\/([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})(?:[/?#]|$)/)
      || source.match(/\/([A-Za-z0-9_-]{24,})(?:[/?#]|$)/);
    return match ? decodeURIComponent(match[1]) : "";
  }

  function finiteNonNegativeNumber(value) {
    const numeric = Number(value);
    return Number.isFinite(numeric) && numeric >= 0 ? numeric : 0;
  }

  function validThreadSessionKey(sessionId) {
    const key = projectMoveSessionKey(sessionId);
    if (!key || key === "__proto__" || key === "prototype" || key === "constructor") return "";
    return /^[A-Za-z0-9_.-]{8,128}$/.test(key) ? key : "";
  }

  // 侧边栏 thread id 对新建会话会长期停留在临时形态（`local:client-new-thread:<uuid>`），
  // 这种 id 在本地存储里没有对应 thread 记录。
  function isTemporaryThreadId(sessionId) {
    return /(^|:)(client-)?new-thread:/.test(String(sessionId || ""));
  }

  // composer 上方节点携带 Codex 分配的真实 conversation id，
  // 用它校正临时 id，避免后端按临时 id 查不到会话。
  function activeConversationIdFromDom() {
    const raw = document
      .querySelector("[data-above-composer-conversation-id]")
      ?.getAttribute("data-above-composer-conversation-id");
    const id = String(raw || "").trim();
    if (!id || isTemporaryThreadId(id)) return "";
    return validThreadSessionKey(id) ? id : "";
  }

  function resolveTemporarySessionRef(ref) {
    if (!isTemporaryThreadId(ref?.session_id)) return ref;
    const conversationId = activeConversationIdFromDom();
    if (!conversationId) return ref;
    return { ...ref, session_id: conversationId };
  }

  // 侧边栏折叠、虚拟滚动移除当前行、路由切换瞬间 aria-current 缺失时，
  // 侧边栏都拿不到会话 id；app:// 下 URL 也不携带 id。
  // 这种情况下 composer 上方节点通常仍在，因此作为独立主来源使用。
  function currentSessionRefFromDom() {
    const rows = sessionRows();
    for (const row of rows) {
      const ref = sessionRefFromRow(row);
      if (ref.session_id && isCurrentSessionRow(row, ref)) return resolveTemporarySessionRef(ref);
    }
    const conversationId = activeConversationIdFromDom();
    if (conversationId) return { session_id: conversationId, title: "" };
    return resolveTemporarySessionRef({ session_id: locationThreadId(), title: "" });
  }

  function currentSessionRef() {
    const ref = currentSessionRefFromDom();
    if (ref.session_id) {
      window.__codexElvesLastSessionRef = { ...ref, at: Date.now() };
      return ref;
    }
    // 解析失败往往是 DOM 重建造成的瞬时空窗口，宽容期内沍用上一次成功结果，
    // 避免已正常展示的会话被误判为“未识别到会话”。
    const last = window.__codexElvesLastSessionRef;
    if (last?.session_id && Date.now() - finiteNonNegativeNumber(last.at) <= codexSessionRefGraceMs) {
      return { session_id: last.session_id, title: last.title || "" };
    }
    return ref;
  }

  // 启动早期 bridge binding 可能尚未就绪。在启动窗口（首次注入后 8s）内，
  // 短暂轮询等 bridge 就绪（最多 ~2s）；超出启动窗口则不再等待，直接视为故障。
  async function waitForCodexSessionDeleteBridgeReady() {
    if (typeof window.__codexSessionDeleteBridge === "function") return true;
    const startupWindowMs = 8000;
    const loadedAt = window.__codexElvesScriptLoadedAt || Date.now();
    if (Date.now() - loadedAt > startupWindowMs) return false;
    const deadline = Date.now() + codexBackendBridgeReadyTimeoutMs;
    while (Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      if (typeof window.__codexSessionDeleteBridge === "function") return true;
    }
    return typeof window.__codexSessionDeleteBridge === "function";
  }

  async function postJson(path, payload) {
    if (!window.__codexSessionDeleteBridge) {
      // 启动早期存在“CDP 注入已成功但渲染端 bridge binding 尚未就绪”的瞬态窗口。
      // 在启动窗口内先短暂轮询等 bridge 就绪，避免误弹“桥接不可用”；
      // 超过启动窗口仍未就绪才视为真正故障，返回明确提示。
      const bridgeReady = await waitForCodexSessionDeleteBridgeReady();
      if (!bridgeReady) {
        if (path === "/backend/status" || path === "/backend/repair") {
          if (location.protocol === "app:") {
            return { status: "failed", message: "桥接不可用，等待自动修复", bridgeMissing: true };
          }
          try {
            const response = await fetch(`${helperBase}${path}`, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(payload || {}),
            });
            return await response.json();
          } catch (error) {
            return { status: "failed", message: "未连接" };
          }
        }
        sendCodexElvesDiagnostic("bridge_missing_for_route", { path });
        return { status: "failed", message: "桥接不可用，请重启启动器" };
      }
    }
    function bridgeWithBackendTimeout(path, payload) {
      return Promise.race([
        window.__codexSessionDeleteBridge(path, payload),
        new Promise((resolve) => setTimeout(() => resolve({ status: "failed", message: "后端检查超时", timeout: true }), codexBackendBridgeTimeoutMs)),
      ]);
    }
    async function fetchBackendStatusFromHelper(path, payload) {
      if (location.protocol === "app:") {
        return { status: "failed", message: "桥接不可用，等待自动修复", bridgeMissing: true };
      }
      try {
        const response = await fetch(`${helperBase}${path}`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload || {}),
        });
        return await response.json();
      } catch (error) {
        return { status: "failed", message: "未连接" };
      }
    }
    try {
      if (path === "/backend/status" || path === "/backend/repair") {
        const result = await bridgeWithBackendTimeout(path, payload);
        if (result?.status === "ok") return result;
        if (result?.timeout) sendCodexElvesDiagnostic("backend_bridge_timeout", { path });
        const fallback = await fetchBackendStatusFromHelper(path, payload);
        if (fallback?.status === "ok") {
          sendCodexElvesDiagnostic("backend_status_bridge_failed_http_fallback_ok", {
            path,
            httpStatus: 200,
            responseStatus: fallback.status || "",
          });
          return fallback;
        }
        sendCodexElvesDiagnostic("backend_status_bridge_and_http_failed", {
          path,
          errorName: "",
          errorMessage: "",
        });
        return fallback;
      }
      return await window.__codexSessionDeleteBridge(path, payload);
    } catch (error) {
      sendCodexElvesDiagnostic("bridge_call_failed", {
        path,
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      if (path === "/backend/status" || path === "/backend/repair") {
        const fallback = await fetchBackendStatusFromHelper(path, payload);
        if (fallback?.status === "ok") {
          sendCodexElvesDiagnostic("backend_status_bridge_failed_http_fallback_ok", {
            path,
            httpStatus: 200,
            responseStatus: fallback.status || "",
          });
          return fallback;
        }
        sendCodexElvesDiagnostic("backend_status_bridge_and_http_failed", {
          path,
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
        });
        return fallback;
      }
      throw error;
    }
  }

  function normalizeCodexTokenUsage(value) {
    const inputTokens = finiteNonNegativeNumber(value?.inputTokens);
    const outputTokens = finiteNonNegativeNumber(value?.outputTokens);
    const cachedTokens = finiteNonNegativeNumber(value?.cachedTokens);
    const cacheCreationTokens = finiteNonNegativeNumber(value?.cacheCreationTokens);
    return {
      inputTokens,
      outputTokens,
      totalTokens: Math.max(
        finiteNonNegativeNumber(value?.totalTokens),
        inputTokens + outputTokens
      ),
      cachedTokens,
      cacheCreationTokens,
      cacheTokens: Math.max(
        finiteNonNegativeNumber(value?.cacheTokens),
        cachedTokens + cacheCreationTokens
      ),
    };
  }

  function resetPendingSessionRowsForFullRefresh() {
    pendingSessionRowsMutationScoped = false;
    pendingSessionRows.clear();
    invalidateSessionRowsCache();
  }

  function addCodexTokenUsage(left, right) {
    const next = normalizeCodexTokenUsage(left);
    const addition = normalizeCodexTokenUsage(right);
    next.inputTokens += addition.inputTokens;
    next.outputTokens += addition.outputTokens;
    next.totalTokens += addition.totalTokens;
    next.cachedTokens += addition.cachedTokens;
    next.cacheCreationTokens += addition.cacheCreationTokens;
    next.cacheTokens += addition.cacheTokens;
    return next;
  }

  function codexTokenUsageSummaryFromResult(result) {
    const provided = result?.summary;
    if (provided?.totalUsage && provided?.lastTurnUsage) {
      return {
        totalUsage: normalizeCodexTokenUsage(provided.totalUsage),
        lastTurnUsage: normalizeCodexTokenUsage(provided.lastTurnUsage),
        lastTurnId: String(provided.lastTurnId || ""),
        lastTurnStartedAt: String(provided.lastTurnStartedAt || ""),
        lastTurnCompletedAt: String(provided.lastTurnCompletedAt || ""),
        observedAt: String(provided.observedAt || ""),
        turnCount: finiteNonNegativeNumber(provided.turnCount),
        descendantCount: finiteNonNegativeNumber(provided.descendantCount),
        lastTurnDescendantCount: finiteNonNegativeNumber(provided.lastTurnDescendantCount),
        unassociatedDescendantCount: finiteNonNegativeNumber(provided.unassociatedDescendantCount),
        isRunning: provided.isRunning === true,
        activeThreadCount: finiteNonNegativeNumber(provided.activeThreadCount),
        lastTurnRunning: provided.lastTurnRunning === true,
      };
    }
    const history = Array.isArray(result?.history) ? result.history : [];
    const latestTurnId = String(history[history.length - 1]?.turn_id || "");
    let totalUsage = normalizeCodexTokenUsage(null);
    let lastTurnUsage = normalizeCodexTokenUsage(null);
    const turnIds = new Set();
    let observedAt = "";
    history.forEach((entry) => {
      const turnId = String(entry?.turn_id || "");
      const usage = normalizeCodexTokenUsage(entry?.usage);
      totalUsage = addCodexTokenUsage(totalUsage, usage);
      if (turnId === latestTurnId) lastTurnUsage = addCodexTokenUsage(lastTurnUsage, usage);
      if (turnId) turnIds.add(turnId);
      observedAt = String(entry?.observed_at || observedAt);
    });
    return {
      totalUsage,
      lastTurnUsage,
      lastTurnId: latestTurnId,
      lastTurnStartedAt: "",
      lastTurnCompletedAt: "",
      observedAt,
      turnCount: turnIds.size,
      descendantCount: 0,
      lastTurnDescendantCount: 0,
      unassociatedDescendantCount: 0,
      isRunning: false,
      activeThreadCount: 0,
      lastTurnRunning: false,
    };
  }

  function codexTokenUsageHasData(usage) {
    const normalized = normalizeCodexTokenUsage(usage);
    return normalized.totalTokens > 0
      || normalized.inputTokens > 0
      || normalized.outputTokens > 0
      || normalized.cacheTokens > 0;
  }

  function formatCodexTokenCount(value) {
    const numeric = finiteNonNegativeNumber(value);
    const billion = 1000 * 1000 * 1000;
    const million = 1000 * 1000;
    const thousand = 1000;
    let divisor = thousand;
    let unit = "K";
    if (numeric >= billion) {
      divisor = billion;
      unit = "B";
    } else if (numeric >= million) {
      divisor = million;
      unit = "M";
    }
    const scaled = numeric / divisor;
    const decimals = scaled >= 100 ? 0 : scaled >= 10 ? 1 : 2;
    const compact = scaled
      .toFixed(decimals)
      .replace(/\.0+$/, "")
      .replace(/(\.\d*[1-9])0+$/, "$1");
    return `${compact}${unit}`;
  }

  function formatCodexTurnDuration(summary) {
    const startedAt = Date.parse(String(summary?.lastTurnStartedAt || ""));
    if (!Number.isFinite(startedAt)) return "";
    const completedAt = Date.parse(String(summary?.lastTurnCompletedAt || ""));
    const endedAt = Number.isFinite(completedAt)
      ? completedAt
      : summary?.lastTurnRunning === true
        ? Date.now()
        : NaN;
    if (!Number.isFinite(endedAt) || endedAt < startedAt) return "";
    const seconds = Math.max(0, Math.floor((endedAt - startedAt) / 1000));
    const hours = Math.floor(seconds / (60 * 60));
    const minutes = Math.floor((seconds % (60 * 60)) / 60);
    const secondsPart = seconds % 60;
    if (hours > 0) return `${hours}h ${minutes}m ${secondsPart}s`;
    if (minutes > 0) return `${minutes}m ${secondsPart}s`;
    return `${secondsPart}s`;
  }

  function stopCodexTokenUsageDurationTicker() {
    clearInterval(window.__codexTokenUsageDurationTimer);
    window.__codexTokenUsageDurationTimer = null;
  }

  function syncCodexTokenUsageDurationTicker(card, summary) {
    stopCodexTokenUsageDurationTicker();
    if (
      !card
      || card.hidden
      || document.visibilityState === "hidden"
      || summary?.lastTurnRunning !== true
      || !formatCodexTurnDuration(summary)
    ) {
      return;
    }
    const updateDuration = () => {
      const durationNode = card.querySelector("[data-codex-token-usage-duration]");
      if (
        !card.isConnected
        || card.hidden
        || card.dataset.status !== "ready"
        || document.visibilityState === "hidden"
        || !durationNode
      ) {
        stopCodexTokenUsageDurationTicker();
        return;
      }
      const duration = formatCodexTurnDuration(summary);
      if (!duration) {
        stopCodexTokenUsageDurationTicker();
        return;
      }
      durationNode.textContent = duration;
      durationNode.title = `最近一轮执行时长：${duration}`;
    };
    updateDuration();
    window.__codexTokenUsageDurationTimer = setInterval(
      updateDuration,
      codexTokenUsageDurationTickIntervalMs
    );
  }

  function codexTokenUsageMetrics(usage) {
    const normalized = normalizeCodexTokenUsage(usage);
    return `
      <div class="codex-token-usage-metrics">
        <span class="codex-token-usage-metric">
          <span class="codex-token-usage-metric-label">输入</span>
          <span class="codex-token-usage-metric-value">${formatCodexTokenCount(normalized.inputTokens)}</span>
        </span>
        <span class="codex-token-usage-metric">
          <span class="codex-token-usage-metric-label">输出</span>
          <span class="codex-token-usage-metric-value">${formatCodexTokenCount(normalized.outputTokens)}</span>
        </span>
        <span class="codex-token-usage-metric">
          <span class="codex-token-usage-metric-label">缓存</span>
          <span class="codex-token-usage-metric-value">${formatCodexTokenCount(normalized.cacheTokens)}</span>
        </span>
      </div>
    `;
  }

  function codexPinnedSummaryMount() {
    const toggle = document.querySelector(selectors.pinnedSummaryToggle);
    if (toggle && toggle.getAttribute("aria-pressed") !== "true") return null;
    const obstacle = document.querySelector(selectors.pinnedSummaryPanel);
    if (!obstacle?.parentElement) return null;
    const host = obstacle.parentElement;
    const panel = Array.from(host.children).find((node) =>
      node !== obstacle && !node.classList?.contains(codexTokenUsageCardClass)
    );
    if (!panel) return null;
    const rect = panel.getBoundingClientRect();
    if (rect.width < 240 || rect.width > 420 || rect.height <= 0) return null;
    return { panel, host };
  }

  function removeCodexTokenUsageCards() {
    document.querySelectorAll(`.${codexTokenUsageCardClass}`).forEach((card) => {
      const host = card.parentElement;
      card.remove();
      host?.classList.remove(codexTokenUsageHostClass);
      host?.style.removeProperty("--codex-token-usage-panel-end-gap");
    });
  }

  function hideCodexTokenUsageCards() {
    document.querySelectorAll(`.${codexTokenUsageCardClass}`).forEach((card) => {
      card.hidden = true;
    });
  }

  function pauseCodexTokenUsageForHiddenPinnedSummary() {
    clearTimeout(window.__codexTokenUsageRefreshTimer);
    window.__codexTokenUsageRefreshTimer = null;
    stopCodexTokenUsageDurationTicker();
    clearTimeout(window.__codexTokenUsageRetryTimer);
    window.__codexTokenUsageRetryTimer = null;
    window.__codexTokenUsageRetryCount = 0;
    window.__codexTokenUsageRefreshPending = false;
    window.__codexTokenUsageRequestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
    window.__codexTokenUsageRequestSession = "";
    hideCodexTokenUsageCards();
  }

  function cachedCodexTokenUsageSummary(sessionSignature) {
    return window.__codexTokenUsageSummaryCache?.get?.(sessionSignature) || null;
  }

  function cacheCodexTokenUsageSummary(sessionSignature, summary, resolvedSessionId = "") {
    const cache = window.__codexTokenUsageSummaryCache;
    if (!(cache instanceof Map) || !sessionSignature || !summary) return;
    cache.delete(sessionSignature);
    cache.set(sessionSignature, {
      summary,
      resolvedSessionId: String(resolvedSessionId || ""),
      cachedAt: Date.now(),
    });
    while (cache.size > 20) {
      const oldestKey = cache.keys().next().value;
      if (oldestKey == null) break;
      cache.delete(oldestKey);
    }
  }

  function stopCodexTokenUsageRuntime() {
    clearTimeout(window.__codexTokenUsageRefreshTimer);
    window.__codexTokenUsageRefreshTimer = null;
    stopCodexTokenUsageDurationTicker();
    clearTimeout(window.__codexTokenUsageRetryTimer);
    window.__codexTokenUsageRetryTimer = null;
    window.__codexTokenUsageRetryCount = 0;
    window.__codexTokenUsageRefreshPending = false;
    window.__codexTokenUsageRequestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
    window.__codexTokenUsageRequestSession = "";
    window.__codexTokenUsageWasRunning = false;
    removeCodexTokenUsageCards();
  }

  function removeCodexTokenUsagePinnedSummaryObservers() {
    window.__codexTokenUsagePinnedSummaryObserver?.disconnect?.();
    window.__codexTokenUsagePinnedSummaryObserver = null;
    window.__codexTokenUsagePinnedSummaryObserverTarget = null;
    window.__codexTokenUsagePinnedSummaryLifecycleObserver?.disconnect?.();
    window.__codexTokenUsagePinnedSummaryLifecycleObserver = null;
    window.__codexTokenUsagePinnedSummaryLifecycleObserverRoot = null;
    if (typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(window.__codexTokenUsagePinnedSummarySyncRafId);
    }
    window.__codexTokenUsagePinnedSummarySyncRafId = 0;
  }

  function resetCodexTokenUsageRetry() {
    clearTimeout(window.__codexTokenUsageRetryTimer);
    window.__codexTokenUsageRetryTimer = null;
    window.__codexTokenUsageRetryCount = 0;
  }

  function scheduleCodexTokenUsageRetry() {
    if (document.visibilityState === "hidden") return false;
    const retryIndex = Math.max(0, Math.round(
      finiteNonNegativeNumber(window.__codexTokenUsageRetryCount)
    ));
    if (retryIndex >= codexTokenUsageRetryDelaysMs.length) return false;
    clearTimeout(window.__codexTokenUsageRetryTimer);
    window.__codexTokenUsageRetryCount = retryIndex + 1;
    window.__codexTokenUsageRetryTimer = setTimeout(() => {
      window.__codexTokenUsageRetryTimer = null;
      scheduleCodexTokenUsageRefresh(0);
    }, codexTokenUsageRetryDelaysMs[retryIndex]);
    return true;
  }

  function installCodexTokenUsageVisibilityListener() {
    document.removeEventListener(
      "visibilitychange",
      window.__codexTokenUsageVisibilityHandler,
      true
    );
    if (!codexElvesSettings().tokenUsage) {
      window.__codexTokenUsageVisibilityHandler = null;
      return;
    }
    window.__codexTokenUsageVisibilityHandler = () => {
      if (document.visibilityState === "hidden") {
        clearTimeout(window.__codexTokenUsageRefreshTimer);
        window.__codexTokenUsageRefreshTimer = null;
        stopCodexTokenUsageDurationTicker();
        return;
      }
      if (codexElvesSettings().tokenUsage) {
        resetCodexTokenUsageRetry();
        scheduleCodexTokenUsageRefresh(0);
      }
    };
    document.addEventListener(
      "visibilitychange",
      window.__codexTokenUsageVisibilityHandler,
      true
    );
  }

  function refreshCodexTokenUsageFeatureState() {
    if (!codexElvesSettings().tokenUsage) {
      removeCodexTokenUsagePinnedSummaryObservers();
      installCodexTokenUsageVisibilityListener();
      stopCodexTokenUsageRuntime();
      return;
    }
    installCodexTokenUsageVisibilityListener();
    installCodexTokenUsagePinnedSummaryObserver();
    resetCodexTokenUsageRetry();
    scheduleCodexTokenUsageRefresh(0);
  }

  function ensureCodexTokenUsageCard(mount) {
    const { panel, host } = mount;
    document.querySelectorAll(`.${codexTokenUsageCardClass}`).forEach((card) => {
      if (card.parentElement === host) return;
      const previousHost = card.parentElement;
      card.remove();
      previousHost?.classList.remove(codexTokenUsageHostClass);
      previousHost?.style.removeProperty("--codex-token-usage-panel-end-gap");
    });
    document.querySelectorAll(`.${codexTokenUsageHostClass}`).forEach((candidate) => {
      if (candidate === host) return;
      candidate.classList.remove(codexTokenUsageHostClass);
      candidate.style.removeProperty("--codex-token-usage-panel-end-gap");
    });
    const panelStyle = getComputedStyle(panel);
    const panelEndGap = panelStyle.paddingInlineEnd || panelStyle.paddingRight || "0px";
    host.classList.add(codexTokenUsageHostClass);
    host.style.setProperty("--codex-token-usage-panel-end-gap", panelEndGap);
    let card = Array.from(host.children).find((node) =>
      node.classList?.contains(codexTokenUsageCardClass)
    );
    if (card) {
      card.className = `${codexTokenUsageCardClass} bg-token-dropdown-background text-token-foreground`;
      card.hidden = false;
      if (panel.nextElementSibling !== card) {
        panel.insertAdjacentElement("afterend", card);
      }
      return card;
    }
    card = document.createElement("section");
    card.className = `${codexTokenUsageCardClass} bg-token-dropdown-background text-token-foreground`;
    card.dataset.codexTokenUsageCard = "true";
    card.setAttribute("aria-label", "会话 Token 统计");
    renderCodexTokenUsagePlaceholder(card);
    panel.insertAdjacentElement("afterend", card);
    return card;
  }

  function renderCodexTokenUsageStatus(card, status, text) {
    card.dataset.status = status;
    card.dataset.running = "false";
    card.removeAttribute("title");
    card.hidden = false;
    card.innerHTML = `
      <div class="codex-token-usage-header">
        <div class="codex-token-usage-title">Token 用量</div>
      </div>
      <div class="codex-token-usage-status">${text}</div>
    `;
  }

  function renderCodexTokenUsageSummary(card, summary, stale = false) {
    const totalUsage = normalizeCodexTokenUsage(summary.totalUsage);
    const lastTurnUsage = normalizeCodexTokenUsage(summary.lastTurnUsage);
    const lastTurnDuration = formatCodexTurnDuration(summary);
    const lastTurnLabel = lastTurnDuration
      ? `<span class="codex-token-usage-label codex-token-usage-last-turn-label">最近一轮 <span class="codex-token-usage-duration" data-codex-token-usage-duration="true" title="最近一轮执行时长：${lastTurnDuration}">${lastTurnDuration}</span></span>`
      : `<span class="codex-token-usage-label">最近一轮</span>`;
    const descendantCount = Math.round(finiteNonNegativeNumber(summary.descendantCount));
    const descendantLabel = descendantCount > 0
      ? `<span class="codex-token-usage-agent-count">子智能体 ${descendantCount}</span>`
      : "";
    const staleLabel = stale
      ? `<span class="codex-token-usage-stale" title="读取会话 Token 记录失败，当前数值可能已过期">可能已过期</span>`
      : "";
    card.dataset.status = "ready";
    card.dataset.stale = String(stale === true);
    card.dataset.running = String(summary.isRunning === true);
    card.removeAttribute("title");
    card.hidden = false;
    card.innerHTML = `
      <div class="codex-token-usage-header">
        <span class="codex-token-usage-title">Token 用量</span>
        ${staleLabel}${descendantLabel}
      </div>
      <div class="codex-token-usage-section">
        <div class="codex-token-usage-section-head">
          <span class="codex-token-usage-label">累计</span>
          <strong class="codex-token-usage-value">${formatCodexTokenCount(totalUsage.totalTokens)}</strong>
        </div>
        ${codexTokenUsageMetrics(totalUsage)}
      </div>
      <div class="codex-token-usage-section">
        <div class="codex-token-usage-section-head">
          ${lastTurnLabel}
          <strong class="codex-token-usage-value">${formatCodexTokenCount(lastTurnUsage.totalTokens)}</strong>
        </div>
        ${codexTokenUsageMetrics(lastTurnUsage)}
      </div>
    `;
    syncCodexTokenUsageDurationTicker(card, summary);
  }

  function emptyCodexTokenUsageSummary() {
    const usage = normalizeCodexTokenUsage(null);
    return {
      totalUsage: usage,
      lastTurnUsage: usage,
      lastTurnId: "",
      lastTurnStartedAt: "",
      lastTurnCompletedAt: "",
      observedAt: "",
      turnCount: 0,
      descendantCount: 0,
      lastTurnDescendantCount: 0,
      unassociatedDescendantCount: 0,
      isRunning: false,
      activeThreadCount: 0,
      lastTurnRunning: false,
    };
  }

  function renderCodexTokenUsagePlaceholder(card) {
    renderCodexTokenUsageSummary(card, emptyCodexTokenUsageSummary());
    card.dataset.status = "placeholder";
  }

  function renderCachedCodexTokenUsage(card, cacheEntry) {
    const summary = cacheEntry?.summary;
    if (!summary) return false;
    if (cacheEntry.resolvedSessionId) {
      card.dataset.codexTokenUsageResolvedSession = cacheEntry.resolvedSessionId;
    }
    renderCodexTokenUsageSummary(card, summary);
    return true;
  }

  // 读取失败时不能静默保留旧数值，否则用户无法区分“未变化”和“无法更新”。
  function markCodexTokenUsageCardStale(card, sessionSignature) {
    if (!card || card.dataset.status !== "ready") return false;
    const summary = cachedCodexTokenUsageSummary(sessionSignature)?.summary;
    if (!summary) return false;
    renderCodexTokenUsageSummary(card, summary, true);
    return true;
  }

  function scheduleCodexTokenUsageRefresh(delayMs = 0) {
    clearTimeout(window.__codexTokenUsageRefreshTimer);
    window.__codexTokenUsageRefreshTimer = null;
    if (!codexElvesSettings().tokenUsage) return;
    const toggle = document.querySelector(selectors.pinnedSummaryToggle);
    if (!toggle || toggle.getAttribute("aria-pressed") !== "true") return;
    window.__codexTokenUsageRefreshTimer = setTimeout(() => {
      window.__codexTokenUsageRefreshTimer = null;
      refreshCodexTokenUsageCard();
    }, Math.max(0, delayMs));
  }

  function syncCodexTokenUsageWithPinnedSummaryState() {
    if (!codexElvesSettings().tokenUsage) return true;
    const toggle = document.querySelector(selectors.pinnedSummaryToggle);
    if (!toggle || toggle.getAttribute("aria-pressed") !== "true") {
      pauseCodexTokenUsageForHiddenPinnedSummary();
      return true;
    }
    refreshCodexTokenUsageCard();
    return !!document.querySelector(`.${codexTokenUsageCardClass}`);
  }

  function scheduleCodexTokenUsagePinnedSummarySync(previousPressed = "") {
    if (typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(window.__codexTokenUsagePinnedSummarySyncRafId);
    }
    window.__codexTokenUsagePinnedSummarySyncRafId = 0;
    let remainingFrames = 16;
    const syncBeforePaint = () => {
      window.__codexTokenUsagePinnedSummarySyncRafId = 0;
      if (!codexElvesSettings().tokenUsage) return;
      const toggle = document.querySelector(selectors.pinnedSummaryToggle);
      const currentPressed = toggle?.getAttribute("aria-pressed") || "";
      if (currentPressed !== previousPressed) {
        if (!syncCodexTokenUsageWithPinnedSummaryState()) {
          remainingFrames -= 1;
          if (remainingFrames > 0 && typeof requestAnimationFrame === "function") {
            window.__codexTokenUsagePinnedSummarySyncRafId =
              requestAnimationFrame(syncBeforePaint);
          }
        }
        return;
      }
      remainingFrames -= 1;
      if (remainingFrames <= 0 || typeof requestAnimationFrame !== "function") return;
      window.__codexTokenUsagePinnedSummarySyncRafId = requestAnimationFrame(syncBeforePaint);
    };
    if (typeof requestAnimationFrame === "function") {
      window.__codexTokenUsagePinnedSummarySyncRafId = requestAnimationFrame(syncBeforePaint);
    } else {
      syncCodexTokenUsageWithPinnedSummaryState();
    }
  }

  function installCodexTokenUsagePinnedSummaryLifecycleObserver() {
    if (!codexElvesSettings().tokenUsage) {
      removeCodexTokenUsagePinnedSummaryObservers();
      return false;
    }
    const root = document.getElementById("root") || document.body;
    if (!root || typeof MutationObserver !== "function") return false;
    if (
      window.__codexTokenUsagePinnedSummaryLifecycleObserver
      && window.__codexTokenUsagePinnedSummaryLifecycleObserverRoot === root
    ) {
      return true;
    }
    window.__codexTokenUsagePinnedSummaryLifecycleObserver?.disconnect?.();
    const observer = new MutationObserver(() => {
      if (!codexElvesSettings().tokenUsage) return;
      const observedToggle = window.__codexTokenUsagePinnedSummaryObserverTarget;
      if (window.__codexTokenUsagePinnedSummaryObserverTarget?.isConnected) {
        if (observedToggle.getAttribute("aria-pressed") !== "true") return;
        const card = document.querySelector(`.${codexTokenUsageCardClass}`);
        if (card && !card.hidden) return;
        if (!document.querySelector(selectors.pinnedSummaryPanel)) return;
        syncCodexTokenUsageWithPinnedSummaryState();
        return;
      }
      installCodexTokenUsagePinnedSummaryObserver();
    });
    observer.observe(root, {
      childList: true,
      subtree: true,
    });
    window.__codexTokenUsagePinnedSummaryLifecycleObserver = observer;
    window.__codexTokenUsagePinnedSummaryLifecycleObserverRoot = root;
    return true;
  }

  function installCodexTokenUsagePinnedSummaryObserver() {
    if (!codexElvesSettings().tokenUsage) {
      removeCodexTokenUsagePinnedSummaryObservers();
      return false;
    }
    installCodexTokenUsagePinnedSummaryLifecycleObserver();
    const toggle = document.querySelector(selectors.pinnedSummaryToggle);
    if (window.__codexTokenUsagePinnedSummaryObserverTarget === toggle) {
      if (toggle && !syncCodexTokenUsageWithPinnedSummaryState()) {
        scheduleCodexTokenUsagePinnedSummarySync();
      }
      return;
    }
    window.__codexTokenUsagePinnedSummaryObserver?.disconnect?.();
    window.__codexTokenUsagePinnedSummaryObserver = null;
    window.__codexTokenUsagePinnedSummaryObserverTarget = toggle || null;
    if (!toggle || typeof MutationObserver !== "function") {
      pauseCodexTokenUsageForHiddenPinnedSummary();
      return;
    }
    const observer = new MutationObserver((mutations) => {
      if (!mutations.some((mutation) => mutation.attributeName === "aria-pressed")) return;
      if (!syncCodexTokenUsageWithPinnedSummaryState()) {
        scheduleCodexTokenUsagePinnedSummarySync();
      }
    });
    observer.observe(toggle, {
      attributes: true,
      attributeFilter: ["aria-pressed"],
    });
    window.__codexTokenUsagePinnedSummaryObserver = observer;
    if (!syncCodexTokenUsageWithPinnedSummaryState()) {
      scheduleCodexTokenUsagePinnedSummarySync();
    }
  }

  function refreshCodexTokenUsageCard() {
    if (!codexElvesSettings().tokenUsage) {
      stopCodexTokenUsageRuntime();
      return;
    }
    const ref = currentSessionRef();
    const sessionId = String(ref?.session_id || "").trim();
    const sessionTitle = String(ref?.title || "").trim();
    const sessionSignature = `${sessionId}\n${sessionTitle}`;
    const mount = codexPinnedSummaryMount();
    let card = null;
    let sessionChanged = false;
    if (mount) {
      card = ensureCodexTokenUsageCard(mount);
      sessionChanged = card.dataset.codexTokenUsageSession !== sessionSignature;
      card.dataset.codexTokenUsageSession = sessionSignature;
      if (sessionChanged) {
        resetCodexTokenUsageRetry();
        window.__codexTokenUsageWasRunning = false;
      }
      const cacheEntry = cachedCodexTokenUsageSummary(sessionSignature);
      if (cacheEntry) {
        renderCachedCodexTokenUsage(card, cacheEntry);
      } else if (sessionChanged || !card.dataset.status) {
        renderCodexTokenUsagePlaceholder(card);
      }
    } else {
      pauseCodexTokenUsageForHiddenPinnedSummary();
      return;
    }
    if (!sessionId) {
      if (card) renderCodexTokenUsageStatus(card, "empty", "当前页面尚未识别到会话。");
      return;
    }
    if (document.visibilityState === "hidden") {
      return;
    }
    if (
      window.__codexTokenUsageRequestPromise
      && window.__codexTokenUsageRequestSession === sessionSignature
    ) {
      window.__codexTokenUsageRefreshPending = true;
      return;
    }
    const requestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
    window.__codexTokenUsageRequestSeq = requestSeq;
    window.__codexTokenUsageRequestSession = sessionSignature;
    let timeoutId = null;
    let backendTimedOut = false;
    const backendRequest = postJson("/thread-usage-summary", {
      session_id: sessionId,
      title: sessionTitle,
    });
    const processCodexTokenUsageResult = (result) => {
      if (requestSeq !== window.__codexTokenUsageRequestSeq) return;
      const activeRef = currentSessionRef();
      if (`${activeRef?.session_id || ""}\n${activeRef?.title || ""}` !== sessionSignature) return;
      const activeMount = codexPinnedSummaryMount();
      const activeCard = activeMount ? ensureCodexTokenUsageCard(activeMount) : null;
      if (activeCard) activeCard.dataset.codexTokenUsageSession = sessionSignature;
      if (result?.status !== "ok") {
        scheduleCodexTokenUsageRetry();
        markCodexTokenUsageCardStale(activeCard, sessionSignature);
        if (
          activeCard
          && activeCard.dataset.status !== "ready"
          && activeCard.dataset.status !== "placeholder"
        ) {
          renderCodexTokenUsageStatus(activeCard, "empty", "当前会话暂无 Token 记录。");
        }
        return;
      }
      resetCodexTokenUsageRetry();
      const summary = codexTokenUsageSummaryFromResult(result);
      cacheCodexTokenUsageSummary(
        sessionSignature,
        summary,
        String(result.session_id || sessionId)
      );
      // 最后一笔 token_count 与 task_complete 几乎同时落盘，若本次已读到
      // isRunning=false，轮询会停止并漏掉那一笔，因此补一次收尾刷新。
      const wasRunning = window.__codexTokenUsageWasRunning === true;
      window.__codexTokenUsageWasRunning = summary.isRunning === true;
      const needsCompletionRefresh = wasRunning && summary.isRunning !== true;
      if (!codexTokenUsageHasData(summary.totalUsage)) {
        if (activeCard) renderCodexTokenUsageSummary(activeCard, summary);
        if (activeCard && summary.isRunning && document.visibilityState !== "hidden") {
          scheduleCodexTokenUsageRefresh(codexTokenUsageRefreshIntervalMs);
        } else if (activeCard && needsCompletionRefresh && document.visibilityState !== "hidden") {
          scheduleCodexTokenUsageRefresh(codexTokenUsageCompletionRefreshDelayMs);
        }
        return;
      }
      if (activeCard) {
        activeCard.dataset.codexTokenUsageResolvedSession = String(result.session_id || sessionId);
        renderCodexTokenUsageSummary(activeCard, summary);
      }
      if (activeCard && summary.isRunning && document.visibilityState !== "hidden") {
        scheduleCodexTokenUsageRefresh(codexTokenUsageRefreshIntervalMs);
      } else if (activeCard && needsCompletionRefresh && document.visibilityState !== "hidden") {
        scheduleCodexTokenUsageRefresh(codexTokenUsageCompletionRefreshDelayMs);
      }
    };
    const requestPromise = Promise.race([
      backendRequest,
      new Promise((resolve) => {
        timeoutId = setTimeout(
          () => resolve({ status: "failed", message: "读取超时", timeout: true }),
          codexTokenUsageRequestTimeoutMs
        );
      }),
    ]).then((result) => {
      backendTimedOut = result?.timeout === true;
      return processCodexTokenUsageResult(result);
    }).catch(() => {
      if (requestSeq !== window.__codexTokenUsageRequestSeq) return;
      scheduleCodexTokenUsageRetry();
      const activeCard = document.querySelector(`.${codexTokenUsageCardClass}`);
      markCodexTokenUsageCardStale(activeCard, sessionSignature);
      if (activeCard && activeCard.dataset.status !== "ready") {
        renderCodexTokenUsageStatus(activeCard, "failed", "Token 统计暂不可用。");
      }
    }).finally(() => {
      if (timeoutId) clearTimeout(timeoutId);
    });
    // 冷缓存或超大父会话可能超过前端等待预算。后端成功返回时仍应用结果，
    // 避免 5 秒超时把随后到达的有效摘要永久丢弃。
    void backendRequest.then((result) => {
      if (!backendTimedOut || result?.status !== "ok") return;
      processCodexTokenUsageResult(result);
    }).catch(() => {});
    const backendLifecycle = Promise.race([
      backendRequest.catch(() => null),
      new Promise((resolve) => setTimeout(resolve, codexTokenUsageLifecycleTimeoutMs)),
    ]);
    const lifecyclePromise = Promise.allSettled([requestPromise, backendLifecycle]).finally(() => {
      if (window.__codexTokenUsageRequestPromise === lifecyclePromise) {
        window.__codexTokenUsageRequestPromise = null;
        window.__codexTokenUsageRequestSession = "";
      }
      if (window.__codexTokenUsageRefreshPending) {
        window.__codexTokenUsageRefreshPending = false;
        scheduleCodexTokenUsageRefresh(0);
      }
    });
    window.__codexTokenUsageRequestPromise = lifecyclePromise;
  }

  function downloadMarkdownFallback(filename, markdown) {
    if (!filename || typeof markdown !== "string") {
      throw new Error("导出结果不完整");
    }
    const blob = new Blob([markdown], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = filename;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  async function saveMarkdown(filename, markdown) {
    if (!filename || typeof markdown !== "string") {
      throw new Error("导出结果不完整");
    }
    if (typeof window.showSaveFilePicker !== "function") {
      downloadMarkdownFallback(filename, markdown);
      return { status: "saved" };
    }
    try {
      const handle = await window.showSaveFilePicker({
        suggestedName: filename,
        types: [{
          description: "Markdown",
          accept: { "text/markdown": [".md", ".markdown"] },
        }],
      });
      const writable = await handle.createWritable();
      await writable.write(markdown);
      await writable.close();
      return { status: "saved" };
    } catch (error) {
      if (error?.name === "AbortError") {
        return { status: "cancelled", message: "导出已取消" };
      }
      throw error;
    }
  }

  let codexStateApiPromise = null;
  let chatsSortInFlight = false;
  let chatsSortSignature = "";
  let chatsSortFallbackArmed = false;

  async function codexStateApi() {
    codexStateApiPromise = codexStateApiPromise || loadCodexAppModule("vscode-api-");
    const api = await codexStateApiPromise;
    if (typeof api.n !== "function") throw new Error("Codex 状态 API 不可用");
    return api.n;
  }

  async function codexStateCall(method, params) {
    const call = await codexStateApi();
    return await call(method, params);
  }

  async function getCodexGlobalState(key) {
    const result = await codexStateCall("get-global-state", { params: { key } });
    return result && Object.prototype.hasOwnProperty.call(result, "value") ? result.value : result;
  }

  async function setCodexGlobalState(key, value) {
    return await codexStateCall("set-global-state", { params: { key, value } });
  }

  function objectGlobalState(value) {
    return value && typeof value === "object" && !Array.isArray(value) ? { ...value } : {};
  }

  function uniqueValues(values) {
    return Array.from(new Set(values.filter((value) => typeof value === "string" && value.trim().length > 0)));
  }

  let codexModelCatalog = { status: "loading", model: "", default_model: "", model_provider: "", provider_name: "", models: [], sources: [], responses_api: { status: "unknown", message: "" } };
  let codexModelCatalogLoadedAt = 0;
  let codexModelCatalogPromise = null;
  let codexStatsigModelVisibilityPatchPromise = null;
  let codexSessionPrewarmManager = window.__codexElvesSessionPrewarmManager || null;



  function isCodexSessionPrewarmManager(candidate) {
    if (!candidate || (typeof candidate !== "object" && typeof candidate !== "function")) return false;
    return typeof candidate.getHostId === "function" &&
      typeof candidate.getRecentConversations === "function" &&
      typeof candidate.sendRequest === "function";
  }

  function codexSessionPrewarmManagerScore(candidate) {
    if (!isCodexSessionPrewarmManager(candidate)) return 0;
    let hostId = "";
    try {
      hostId = String(candidate.getHostId() || "");
    } catch {
      return 0;
    }
    if (hostId !== "local") return 0;
    let score = 1;
    if (typeof candidate.readThread === "function") score += 1;
    if (typeof candidate.hydrateBackgroundThreads === "function") score += 2;
    if (typeof candidate.unsubscribeInactiveConversation === "function") score += 4;
    if (typeof candidate.resumeConversationForUnavailableOwner === "function") score += 8;
    return score;
  }

  function reactFiberKeys(element) {
    return Object.keys(element).filter((key) =>
      key.startsWith("__reactFiber")
      || key.startsWith("__reactInternalInstance")
      || key.startsWith("__reactProps")
    );
  }

  function codexSessionPrewarmReactRootsSignature(roots) {
    return Array.from(roots || []).map((root, index) => {
      const tag = root?.tagName || root?.constructor?.name || "unknown";
      return `${index}:${tag}:${Object.keys(root || {}).slice(0, 8).join(",")}`;
    }).join("|");
  }

  const codexSessionPrewarmPreferredObjectProperties = new Set([
    "child",
    "dependencies",
    "firstContext",
    "memoizedValue",
    "value",
    "familyBindings",
    "atom",
    "init",
    "store",
    "memoizedState",
    "memoizedProps",
    "updateQueue",
  ]);

  function findCodexSessionPrewarmManagerInObjectGraph(roots, maxNodes = 12000) {
    const priorityQueue = [];
    const queue = [];
    const visited = new WeakSet();
    let cursor = 0;
    let scanned = 0;
    let bestManager = null;
    let bestScore = 0;
    const enqueue = (value, depth, priority = false) => {
      if (!value || (typeof value !== "object" && typeof value !== "function") || visited.has(value)) return;
      (priority ? priorityQueue : queue).push({ value, depth });
    };
    const rootValues = Array.isArray(roots) ? roots : [];
    for (let index = rootValues.length - 1; index >= 0; index -= 1) {
      enqueue(rootValues[index], 0, true);
    }
    while ((priorityQueue.length > 0 || cursor < queue.length) && scanned < maxNodes) {
      const { value, depth } = priorityQueue.length > 0
        ? priorityQueue.pop()
        : queue[cursor++];
      if (!value || visited.has(value) || depth > 18) continue;
      visited.add(value);
      scanned += 1;
      const score = codexSessionPrewarmManagerScore(value);
      if (score > bestScore) {
        bestManager = value;
        bestScore = score;
        if (typeof value.resumeConversationForUnavailableOwner === "function") break;
      }
      if (
        value === window ||
        value === document ||
        (typeof Element !== "undefined" && value instanceof Element)
      ) {
        continue;
      }
      if (value instanceof Map) {
        const entries = Array.from(value.entries()).slice(0, 256);
        for (let index = entries.length - 1; index >= 0; index -= 1) {
          const [key, item] = entries[index];
          enqueue(key, depth + 1);
          enqueue(item, depth + 1, true);
        }
      } else if (value instanceof Set) {
        const entries = Array.from(value).slice(0, 256);
        for (let index = entries.length - 1; index >= 0; index -= 1) {
          enqueue(entries[index], depth + 1, true);
        }
      }
      let propertyNames = [];
      try {
        propertyNames = Object.getOwnPropertyNames(value);
      } catch {
      }
      const boundedPropertyNames = propertyNames.slice(0, 256);
      for (let index = boundedPropertyNames.length - 1; index >= 0; index -= 1) {
        const name = boundedPropertyNames[index];
        if (["ownerDocument", "parentElement", "parentNode", "children", "childNodes", "return"].includes(name)) continue;
        try {
          const descriptor = Object.getOwnPropertyDescriptor(value, name);
          if (descriptor && Object.prototype.hasOwnProperty.call(descriptor, "value")) {
            enqueue(
              descriptor.value,
              depth + 1,
              codexSessionPrewarmPreferredObjectProperties.has(name)
            );
          }
        } catch {
        }
      }
    }
    return {
      manager: bestManager,
      scanned,
      exhausted: priorityQueue.length > 0 || cursor < queue.length,
    };
  }

  function codexSessionPrewarmReactObjectRoots() {
    const nodes = [
      document.querySelector("aside"),
      document.querySelector("main"),
      document.body?.firstElementChild || null,
    ].filter(Boolean);
    if (!nodes.some((node) => reactFiberKeys(node).length > 0)) {
      nodes.push(...Array.from(document.querySelectorAll("aside, main, body > div, body > section")).slice(0, 24));
    }
    const roots = [];
    const seen = new Set();
    for (const node of nodes) {
      if (!node || seen.has(node)) continue;
      seen.add(node);
      for (const key of reactFiberKeys(node)) {
        try {
          if (node[key]) roots.push(node[key]);
        } catch {
        }
      }
    }
    return roots;
  }

  function findCodexSessionPrewarmManagerInReactTree(force = false) {
    const roots = codexSessionPrewarmReactObjectRoots();
    const signature = codexSessionPrewarmReactRootsSignature(roots);
    const cached = window.__codexSessionPrewarmReactManagerDiscovery;
    if (
      !force
      && cached
      && cached.signature === signature
      && Date.now() - cached.at < codexManagerReactDiscoveryCooldownMs
    ) {
      return { manager: null, scanned: 0, exhausted: cached.exhausted === true, cached: true };
    }
    const result = findCodexSessionPrewarmManagerInObjectGraph(roots);
    if (!result.manager) {
      window.__codexSessionPrewarmReactManagerDiscovery = {
        signature,
        at: Date.now(),
        exhausted: result.exhausted === true,
      };
    } else {
      window.__codexSessionPrewarmReactManagerDiscovery = null;
    }
    return result;
  }

  let codexConversationManager = window.__codexElvesConversationStateManager || null;

  function codexAppServerManagerHostId(candidate) {
    try {
      if (typeof candidate?.getHostId === "function") {
        return String(candidate.getHostId() || "");
      }
      return String(candidate?.hostId || "");
    } catch {
      return "";
    }
  }

  function isCodexConversationManager(candidate) {
    if (!candidate || (typeof candidate !== "object" && typeof candidate !== "function")) return false;
    return codexAppServerManagerHostId(candidate) === "local"
      && typeof candidate.getCachedConversations === "function"
      && typeof candidate.getConversation === "function"
      && typeof candidate.updateConversationState === "function"
      && !!candidate.threadStore;
  }

  function findCodexConversationManagerInObjectGraph(roots, maxNodes = 30000) {
    const priorityQueue = [];
    const queue = [];
    const visited = new WeakSet();
    let cursor = 0;
    let scanned = 0;
    const enqueue = (value, depth, priority = false) => {
      if (!value || (typeof value !== "object" && typeof value !== "function") || visited.has(value)) return;
      (priority ? priorityQueue : queue).push({ value, depth });
    };
    const rootValues = Array.isArray(roots) ? roots : [];
    for (let index = rootValues.length - 1; index >= 0; index -= 1) {
      enqueue(rootValues[index], 0, true);
    }
    while ((priorityQueue.length > 0 || cursor < queue.length) && scanned < maxNodes) {
      const { value, depth } = priorityQueue.length > 0
        ? priorityQueue.pop()
        : queue[cursor++];
      if (!value || visited.has(value) || depth > 20) continue;
      visited.add(value);
      scanned += 1;
      if (isCodexConversationManager(value)) {
        return { manager: value, scanned, exhausted: false };
      }
      if (
        value === window
        || value === document
        || (typeof Element !== "undefined" && value instanceof Element)
      ) {
        continue;
      }
      if (value instanceof Map) {
        const entries = Array.from(value.entries()).slice(0, 256);
        for (let index = entries.length - 1; index >= 0; index -= 1) {
          const [key, item] = entries[index];
          enqueue(key, depth + 1);
          enqueue(item, depth + 1, true);
        }
      } else if (value instanceof Set) {
        const entries = Array.from(value).slice(0, 256);
        for (let index = entries.length - 1; index >= 0; index -= 1) {
          enqueue(entries[index], depth + 1, true);
        }
      }
      let propertyNames = [];
      try {
        propertyNames = Object.getOwnPropertyNames(value).slice(0, 320);
      } catch {
      }
      for (let index = propertyNames.length - 1; index >= 0; index -= 1) {
        const name = propertyNames[index];
        if (["ownerDocument", "parentElement", "parentNode", "children", "childNodes", "return"].includes(name)) continue;
        try {
          const descriptor = Object.getOwnPropertyDescriptor(value, name);
          if (descriptor && Object.prototype.hasOwnProperty.call(descriptor, "value")) {
            enqueue(
              descriptor.value,
              depth + 1,
              codexSessionPrewarmPreferredObjectProperties.has(name) || name === "stateNode"
            );
          }
        } catch {
        }
      }
    }
    return {
      manager: null,
      scanned,
      exhausted: priorityQueue.length > 0 || cursor < queue.length,
    };
  }

  function findCodexConversationManagerInReactTree(force = false) {
    const testManager = window.__CODEX_ELVES_TEST_APP_SERVER_RESTART__
      ? window.__codexElvesAppServerRestartManagerOverride
      : null;
    if (isCodexConversationManager(testManager)) {
      return { manager: testManager, scanned: 0, exhausted: false, testOverride: true };
    }
    if (!force && isCodexConversationManager(codexConversationManager)) {
      return { manager: codexConversationManager, scanned: 0, exhausted: false, cached: true };
    }
    const result = findCodexConversationManagerInObjectGraph(
      codexSessionPrewarmReactObjectRoots()
    );
    if (result.manager) {
      codexConversationManager = result.manager;
      window.__codexElvesConversationStateManager = result.manager;
    } else if (force) {
      codexConversationManager = null;
      window.__codexElvesConversationStateManager = null;
    }
    return result;
  }

  function codexAppServerTurnErrorMessages(turn) {
    const messages = [];
    if (typeof turn?.error?.message === "string") messages.push(turn.error.message);
    for (const item of Array.isArray(turn?.items) ? turn.items : []) {
      if (item?.type === "error" && typeof item.message === "string") {
        messages.push(item.message);
      }
    }
    return messages;
  }

  function isCodexAppServerTransientFailedTurn(turn) {
    if (!turn || turn.turnId != null || turn.status !== "failed") return false;
    return codexAppServerRestartTextMatches(
      codexAppServerTurnErrorMessages(turn).join("\n")
    );
  }

  function codexAppServerConversationEntityEntries(conversation) {
    const entities = conversation?.turnHistory?.kind === "canonical"
      ? conversation.turnHistory.history?.entitiesByKey
      : null;
    if (entities instanceof Map) return Array.from(entities.entries());
    if (entities && typeof entities === "object") return Object.entries(entities);
    return [];
  }

  function codexAppServerConversationTurns(conversation) {
    const turns = [];
    const visited = new Set();
    for (const turn of Array.isArray(conversation?.turns) ? conversation.turns : []) {
      if (!turn || visited.has(turn)) continue;
      visited.add(turn);
      turns.push(turn);
    }
    for (const [, turn] of codexAppServerConversationEntityEntries(conversation)) {
      if (!turn || visited.has(turn)) continue;
      visited.add(turn);
      turns.push(turn);
    }
    return turns;
  }

  function codexAppServerConversationHasTransientFailure(conversation) {
    return codexAppServerConversationTurns(conversation)
      .some(isCodexAppServerTransientFailedTurn);
  }

  function codexAppServerCachedConversations(manager) {
    const raw = manager?.getCachedConversations?.();
    return Array.from(raw || []).map((entry) =>
      Array.isArray(entry) && entry.length === 2 && entry[1]?.id
        ? entry[1]
        : entry
    ).filter((conversation) => conversation?.id);
  }

  function codexAppServerRunningConversations(manager, failedConversationId = "") {
    if (!isCodexConversationManager(manager)) {
      return { known: false, conversations: [] };
    }
    try {
      const conversations = codexAppServerCachedConversations(manager);
      const byId = new Map();
      const evidence = manager.threadStore?.runtimeThreadStatusEvidenceByThreadId;
      for (const conversation of conversations) {
        const conversationId = String(conversation.id || "");
        if (!conversationId) continue;
        const turns = codexAppServerConversationTurns(conversation);
        const hasInProgressTurn = turns.some((turn) => turn?.status === "inProgress");
        const hasTransientFailure = turns.some(isCodexAppServerTransientFailedTurn);
        const runtimeActive = conversation.threadRuntimeStatus?.type === "active";
        const evidenceActive = evidence instanceof Map
          && evidence.get(conversationId)?.type === "active";
        const failedPlaceholderOnly = conversationId === failedConversationId
          && hasTransientFailure
          && !hasInProgressTurn;
        if (hasInProgressTurn || ((runtimeActive || evidenceActive) && !failedPlaceholderOnly)) {
          byId.set(conversationId, {
            id: conversationId,
            title: String(conversation.title || ""),
            source: hasInProgressTurn
              ? "turn"
              : evidenceActive
                ? "runtime-evidence"
                : "runtime-status",
          });
        }
      }
      if (evidence instanceof Map) {
        for (const [rawId, status] of evidence.entries()) {
          const conversationId = String(rawId || "");
          if (!conversationId || status?.type !== "active" || byId.has(conversationId)) continue;
          if (conversationId === failedConversationId) {
            const failedConversation = conversations.find((item) => String(item.id || "") === conversationId);
            if (failedConversation && codexAppServerConversationHasTransientFailure(failedConversation)) {
              continue;
            }
          }
          const conversation = conversations.find((item) => String(item.id || "") === conversationId);
          byId.set(conversationId, {
            id: conversationId,
            title: String(conversation?.title || ""),
            source: "runtime-evidence",
          });
        }
      }
      return { known: true, conversations: Array.from(byId.values()) };
    } catch (error) {
      sendCodexElvesDiagnostic("app_server_restart_running_check_failed", {
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      return { known: false, conversations: [] };
    }
  }

  function cleanupCodexAppServerTransientFailedTurns(manager, conversationId) {
    if (!isCodexConversationManager(manager) || !conversationId) return 0;
    let removed = 0;
    manager.updateConversationState(conversationId, (conversation) => {
      if (Array.isArray(conversation.turns)) {
        const previousLength = conversation.turns.length;
        conversation.turns = conversation.turns.filter(
          (turn) => !isCodexAppServerTransientFailedTurn(turn)
        );
        removed += previousLength - conversation.turns.length;
      }

      const history = conversation.turnHistory?.kind === "canonical"
        ? conversation.turnHistory.history
        : null;
      const entities = history?.entitiesByKey;
      if (!entities || typeof entities !== "object") return;

      const removedEntityKeys = new Set();
      if (entities instanceof Map) {
        for (const [key, turn] of Array.from(entities.entries())) {
          if (!isCodexAppServerTransientFailedTurn(turn)) continue;
          removedEntityKeys.add(String(key));
          entities.delete(key);
          removed += 1;
        }
      } else {
        for (const [key, turn] of Object.entries(entities)) {
          if (!isCodexAppServerTransientFailedTurn(turn)) continue;
          removedEntityKeys.add(String(key));
          delete entities[key];
          removed += 1;
        }
      }
      if (removedEntityKeys.size === 0 || !Array.isArray(history.islands)) return;

      for (const island of history.islands) {
        if (!Array.isArray(island?.entries)) continue;
        island.entries = island.entries.filter((entry) => {
          const entityKey = typeof entry === "string"
            ? entry
            : entry?.value ?? entry?.key ?? "";
          return !removedEntityKeys.has(String(entityKey));
        });
      }
    });
    return removed;
  }

  function codexAppServerNormalizedText(value) {
    return String(value || "").replace(/\s+/g, " ").trim().toLowerCase();
  }

  function codexAppServerRestartTextMatches(value) {
    const normalized = codexAppServerNormalizedText(value);
    return normalized.includes("failed to start turn")
      && normalized.includes("agent loop died unexpectedly");
  }

  function codexAppServerRestartTextElementEligible(element) {
    if (!(element instanceof Element)) return false;
    if (codexAppServerNormalizedText(element.innerText || element.textContent) !== codexAppServerRestartErrorText) {
      return false;
    }
    if (element.closest('[data-message-author-role="user"], [data-user-message-bubble]')) return false;
    if (element.closest(`.${codexAppServerRestartButtonClass}, .${codexAppServerRestartDialogClass}`)) return false;
    return true;
  }

  function codexAppServerRestartMutationRelevant(mutation) {
    if (mutation?.type !== "childList") return false;
    const changedNodes = [
      ...Array.from(mutation.addedNodes || []),
      ...Array.from(mutation.removedNodes || []),
    ];
    return changedNodes.some((node) => {
      const element = node?.nodeType === 1 ? node : node?.parentElement;
      if (element?.closest?.('[data-message-author-role="user"], [data-user-message-bubble]')) {
        return false;
      }
      const text = node?.nodeType === 3
        ? node.nodeValue
        : node?.innerText || node?.textContent;
      return codexAppServerRestartTextMatches(text);
    });
  }

  function codexAppServerRestartErrorElements() {
    const root = document.body || document.documentElement;
    if (!root) return [];
    const elements = new Set();
    if (typeof document.createTreeWalker === "function") {
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      let textNode = null;
      while ((textNode = walker.nextNode())) {
        if (codexAppServerNormalizedText(textNode.nodeValue) !== codexAppServerRestartErrorText) continue;
        const element = textNode.parentElement;
        if (!codexAppServerRestartTextElementEligible(element)) continue;
        elements.add(element);
      }
    }
    if (elements.size === 0) {
      for (const element of root.querySelectorAll("div, p, span, [role='alert']")) {
        if (!codexAppServerRestartTextElementEligible(element)) continue;
        const childContainsWholeError = Array.from(element.children).some((child) =>
          codexAppServerNormalizedText(child.innerText || child.textContent)
            === codexAppServerRestartErrorText
        );
        if (childContainsWholeError) continue;
        elements.add(element);
      }
    }
    return Array.from(elements);
  }

  function codexAppServerRestartElementRect(element) {
    if (!(element instanceof Element) || !element.isConnected) return null;
    if (typeof document.createRange === "function") {
      try {
        const range = document.createRange();
        range.selectNodeContents(element);
        const rect = range.getBoundingClientRect();
        if (rect && rect.width > 0 && rect.height > 0) return rect;
      } catch {
      }
    }
    try {
      const rect = element.getBoundingClientRect();
      return rect && rect.width > 0 && rect.height > 0 ? rect : null;
    } catch {
      return null;
    }
  }

  function codexAppServerRestartViewportSize() {
    return {
      width: Math.max(
        0,
        finiteNonNegativeNumber(window.innerWidth)
          || finiteNonNegativeNumber(document.documentElement?.clientWidth)
      ),
      height: Math.max(
        0,
        finiteNonNegativeNumber(window.innerHeight)
          || finiteNonNegativeNumber(document.documentElement?.clientHeight)
      ),
    };
  }

  function codexAppServerRestartRectVisible(rect, viewport) {
    return !!rect
      && rect.bottom > 0
      && rect.right > 0
      && (!viewport.width || rect.left < viewport.width)
      && (!viewport.height || rect.top < viewport.height);
  }

  function codexAppServerRestartVisibleErrorElement() {
    const viewport = codexAppServerRestartViewportSize();
    return codexAppServerRestartErrorElements().find((element) =>
      codexAppServerRestartRectVisible(
        codexAppServerRestartElementRect(element),
        viewport
      )
    ) || null;
  }

  function resolveCodexAppServerRestartPlacement(
    anchorRect,
    buttonRect,
    viewportWidth,
    viewportHeight
  ) {
    const margin = 8;
    const gap = 10;
    const buttonWidth = Math.max(58, finiteNonNegativeNumber(buttonRect?.width));
    const buttonHeight = Math.max(24, finiteNonNegativeNumber(buttonRect?.height));
    if (anchorRect && viewportWidth > 0 && viewportHeight > 0) {
      const maxTop = Math.max(margin, viewportHeight - buttonHeight - margin);
      const top = Math.min(
        maxTop,
        Math.max(margin, anchorRect.top + (anchorRect.height - buttonHeight) / 2)
      );
      const afterLeft = anchorRect.right + gap;
      if (afterLeft + buttonWidth <= viewportWidth - margin) {
        return { placement: "after", left: afterLeft, top };
      }
      const beforeLeft = anchorRect.left - gap - buttonWidth;
      if (beforeLeft >= margin) {
        return { placement: "before", left: beforeLeft, top };
      }
    }
    return { placement: "notice", right: 18, bottom: 18 };
  }

  function syncCodexAppServerRestartButtonLabel(button) {
    if (!button) return;
    const notice = button.dataset.codexAppServerRestartPlacement === "notice";
    if (button.disabled) {
      button.textContent = notice ? "app-server 重启中…" : "重启中…";
      return;
    }
    button.textContent = notice ? "检测到 app-server 异常，点击重启" : "重启";
  }

  function positionCodexAppServerRestartButton(button) {
    if (!button?.isConnected) return;
    const viewport = codexAppServerRestartViewportSize();
    let anchor = button.__codexAppServerRestartAnchor;
    let anchorRect = codexAppServerRestartElementRect(anchor);
    if (!codexAppServerRestartRectVisible(anchorRect, viewport)) {
      anchor = codexAppServerRestartVisibleErrorElement();
      anchorRect = codexAppServerRestartElementRect(anchor);
      button.__codexAppServerRestartAnchor = anchor;
    }

    const buttonRect = button.getBoundingClientRect();
    const placement = resolveCodexAppServerRestartPlacement(
      codexAppServerRestartRectVisible(anchorRect, viewport) ? anchorRect : null,
      buttonRect,
      viewport.width,
      viewport.height
    );
    button.dataset.codexAppServerRestartPlacement = placement.placement;
    if (placement.placement === "notice") {
      button.style.left = "auto";
      button.style.top = "auto";
      button.style.right = `${placement.right}px`;
      button.style.bottom = `${placement.bottom}px`;
      syncCodexAppServerRestartButtonLabel(button);
      return;
    }
    button.style.right = "auto";
    button.style.bottom = "auto";
    button.style.left = `${Math.round(placement.left)}px`;
    button.style.top = `${Math.round(placement.top)}px`;
    syncCodexAppServerRestartButtonLabel(button);
  }

  function scheduleCodexAppServerRestartButtonPosition() {
    if (window.__codexAppServerRestartPositionRaf) return;
    window.__codexAppServerRestartPositionRaf = requestAnimationFrame(() => {
      window.__codexAppServerRestartPositionRaf = 0;
      document.querySelectorAll(`.${codexAppServerRestartButtonClass}`)
        .forEach(positionCodexAppServerRestartButton);
    });
  }

  function installCodexAppServerRestartPositionTracking() {
    const previous = window.__codexAppServerRestartPositionHandler;
    if (previous) {
      window.removeEventListener("resize", previous, true);
      document.removeEventListener("scroll", previous, true);
    }
    const handler = () => scheduleCodexAppServerRestartButtonPosition();
    window.__codexAppServerRestartPositionHandler = handler;
    window.addEventListener("resize", handler, true);
    document.addEventListener("scroll", handler, true);
  }

  function removeCodexAppServerRestartButton(button) {
    if (button) button.__codexAppServerRestartAnchor = null;
    button?.remove?.();
  }

  function removeCodexAppServerRestartButtonsExcept(conversationId = "") {
    document.querySelectorAll(`.${codexAppServerRestartButtonClass}`).forEach((button) => {
      if (conversationId && button.dataset.codexAppServerRestartConversationId === conversationId) return;
      removeCodexAppServerRestartButton(button);
    });
  }

  function showCodexAppServerRestartNotice(title, message) {
    document.querySelectorAll(`.${codexAppServerRestartDialogClass}`).forEach((node) => node.remove());
    return new Promise((resolve) => {
      const overlay = document.createElement("div");
      overlay.className = `codex-delete-confirm-overlay ${codexAppServerRestartDialogClass}`;
      overlay.innerHTML = `
        <div class="codex-delete-confirm-content" role="dialog" aria-modal="true" aria-label="${escapeHtml(title)}">
          <div class="codex-delete-confirm-title">${escapeHtml(title)}</div>
          <div class="codex-delete-confirm-message">${escapeHtml(message)}</div>
          <div class="codex-delete-confirm-actions">
            <button type="button" data-codex-app-server-restart-ack="true">知道了</button>
          </div>
        </div>
      `;
      const finish = (event) => {
        event?.preventDefault();
        event?.stopPropagation();
        overlay.remove();
        resolve();
      };
      overlay.addEventListener("click", (event) => {
        if (event.target === overlay || event.target.closest("[data-codex-app-server-restart-ack]")) {
          finish(event);
        }
      }, true);
      overlay.addEventListener("keydown", (event) => {
        if (event.key === "Escape") finish(event);
      }, true);
      document.body.appendChild(overlay);
      overlay.querySelector("[data-codex-app-server-restart-ack]")?.focus();
    });
  }

  function showCodexAppServerRestartBlockedDialog(count) {
    return showCodexAppServerRestartNotice(
      "暂时无法重启",
      `当前有 ${count} 个会话正在执行，请等待执行完成后再重启，否则会话会被中断。`
    );
  }

  function setCodexAppServerRestartButtonBusy(button, busy) {
    if (!button) return;
    button.disabled = !!busy;
    button.setAttribute("aria-busy", busy ? "true" : "false");
    syncCodexAppServerRestartButtonLabel(button);
  }

  function waitForCodexAppServerRestartDelay(delayMs) {
    return new Promise((resolve) => setTimeout(resolve, delayMs));
  }

  function showCodexAppServerRestartToast(message) {
    if (window.__CODEX_ELVES_TEST_APP_SERVER_RESTART__) {
      window.__codexElvesAppServerRestartTestToasts =
        window.__codexElvesAppServerRestartTestToasts || [];
      window.__codexElvesAppServerRestartTestToasts.push(String(message || ""));
      return;
    }
    showToast(message);
  }

  async function restartCodexAppServerFromFailure(button, conversationId) {
    if (!button || !conversationId || window.__codexAppServerRestartInFlight === true) return;
    window.__codexAppServerRestartInFlight = true;
    setCodexAppServerRestartButtonBusy(button, true);
    let recovered = false;
    try {
      const discovery = findCodexConversationManagerInReactTree(true);
      const manager = discovery.manager;
      const running = codexAppServerRunningConversations(manager, conversationId);
      sendCodexElvesDiagnostic("app_server_restart_running_check", {
        conversationId,
        known: running.known,
        runningCount: running.conversations.length,
      });
      if (!running.known) {
        await showCodexAppServerRestartNotice(
          "暂时无法重启",
          "无法确认当前是否还有执行中的会话，请稍后重试。"
        );
        return;
      }
      if (running.conversations.length > 0) {
        await showCodexAppServerRestartBlockedDialog(running.conversations.length);
        return;
      }

      const dispatcher = codexServiceTierDispatcher || await findCodexServiceTierDispatcher();
      if (!dispatcher || typeof dispatcher.dispatchMessage !== "function") {
        throw new Error("Codex app-server 重启组件不可用");
      }
      codexServiceTierDispatcher = dispatcher;
      dispatcher.dispatchMessage("codex-app-server-restart", {
        hostId: "local",
        intent: "restart",
        killCodexProcess: false,
        remoteControlEnabled: false,
      });
      sendCodexElvesDiagnostic("app_server_restart_requested", {
        conversationId,
        killCodexProcess: false,
      });

      let lastManager = manager;
      let removed = 0;
      for (const delayMs of codexAppServerRestartRecoveryDelaysMs) {
        await waitForCodexAppServerRestartDelay(delayMs);
        const refreshed = findCodexConversationManagerInReactTree(true);
        if (refreshed.manager) lastManager = refreshed.manager;
        if (!lastManager) continue;
        removed += cleanupCodexAppServerTransientFailedTurns(lastManager, conversationId);
        let conversation = null;
        try {
          conversation = lastManager.getConversation(conversationId);
        } catch {
        }
        if (removed > 0 || !conversation || !codexAppServerConversationHasTransientFailure(conversation)) {
          recovered = true;
          break;
        }
      }
      if (!recovered) {
        throw new Error("app-server 已请求重启，但失败状态尚未清理，请稍后重试");
      }
      sendCodexElvesDiagnostic("app_server_restart_recovered", {
        conversationId,
        removedTurnCount: removed,
      });
      showCodexAppServerRestartToast("app-server 已重启，失败状态已清理");
    } catch (error) {
      sendCodexElvesDiagnostic("app_server_restart_failed", {
        conversationId,
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      showCodexAppServerRestartToast(error?.message || "app-server 重启失败，请稍后重试");
    } finally {
      window.__codexAppServerRestartInFlight = false;
      if (!recovered && button?.isConnected) {
        setCodexAppServerRestartButtonBusy(button, false);
      }
    }
  }

  function installCodexAppServerRestartButtons() {
    const activeRef = currentSessionRefFromDom();
    const conversationId = activeConversationIdFromDom()
      || validThreadSessionKey(activeRef?.session_id);
    removeCodexAppServerRestartButtonsExcept(conversationId);
    if (!conversationId) return;
    const anchor = codexAppServerRestartVisibleErrorElement();
    const discovery = findCodexConversationManagerInReactTree();
    const manager = discovery.manager;
    let conversation = null;
    try {
      conversation = manager?.getConversation?.(conversationId);
    } catch {
    }
    const stateHasFailure = codexAppServerConversationHasTransientFailure(conversation);
    if (!stateHasFailure && !anchor) {
      removeCodexAppServerRestartButtonsExcept();
      return;
    }
    const existing = document.querySelector(
      `.${codexAppServerRestartButtonClass}[data-codex-app-server-restart-conversation-id="${CSS.escape(conversationId)}"]`
    );
    if (existing?.isConnected) {
      if (
        existing.dataset.codexAppServerRestartVersion
        === codexAppServerManagerDiscoveryVersion
      ) {
        existing.__codexAppServerRestartAnchor = anchor;
        positionCodexAppServerRestartButton(existing);
        return;
      }
      removeCodexAppServerRestartButton(existing);
    }

    if (!document.body) return;
    const button = document.createElement("button");
    button.type = "button";
    button.className = codexAppServerRestartButtonClass;
    button.dataset.codexAppServerRestart = "true";
    button.dataset.codexAppServerRestartConversationId = conversationId;
    button.dataset.codexAppServerRestartVersion = codexAppServerManagerDiscoveryVersion;
    button.dataset.codexActionLabel = "CodexElves 提供热重启修复问题";
    button.dataset.codexTooltipPlacement = "top";
    button.dataset.codexTooltipGap = "10";
    button.__codexAppServerRestartAnchor = anchor;
    button.textContent = "重启";
    button.setAttribute("aria-label", "CodexElves 提供热重启修复问题");
    button.addEventListener("pointerenter", () => showActionButtonTooltip(button));
    button.addEventListener("pointerleave", hideActionButtonTooltip);
    button.addEventListener("focus", () => showActionButtonTooltip(button));
    button.addEventListener("blur", hideActionButtonTooltip);
    button.addEventListener("click", (event) => {
      hideActionButtonTooltip();
      event.preventDefault();
      event.stopPropagation();
      void restartCodexAppServerFromFailure(button, conversationId);
    }, true);
    document.body.appendChild(button);
    positionCodexAppServerRestartButton(button);
    sendCodexElvesDiagnostic("app_server_restart_button_installed", {
      conversationId,
      source: stateHasFailure ? "state" : "dom",
      placement: button.dataset.codexAppServerRestartPlacement || "",
    });
  }

  if (window.__CODEX_ELVES_TEST_APP_SERVER_RESTART__) {
    window.__codexElvesAppServerRestartTest = {
      isTransientFailedTurn: isCodexAppServerTransientFailedTurn,
      matchesErrorText: codexAppServerRestartTextMatches,
      mutationRelevant: codexAppServerRestartMutationRelevant,
      resolvePlacement: resolveCodexAppServerRestartPlacement,
      runningConversations: codexAppServerRunningConversations,
      cleanupTransientFailedTurns: cleanupCodexAppServerTransientFailedTurns,
      conversationHasTransientFailure: codexAppServerConversationHasTransientFailure,
      installButtons: installCodexAppServerRestartButtons,
      removeButtons: () => removeCodexAppServerRestartButtonsExcept(),
      restartFromFailure: restartCodexAppServerFromFailure,
      setConversationManager: (manager) => {
        codexConversationManager = manager;
        window.__codexElvesConversationStateManager = manager;
        window.__codexElvesAppServerRestartManagerOverride = manager;
      },
      setDispatcher: (dispatcher) => {
        codexServiceTierDispatcher = dispatcher;
      },
    };
  }

  if (window.__CODEX_ELVES_TEST_SERVICE_TIER__) {
    window.__codexElvesServiceTierTest = {
      applyServiceTierOverride: (method, params, threadIdHint = "") => applyCodexServiceTierRequestOverride(method, params, threadIdHint),
      requestOverride: (message) => codexServiceTierRequestOverride(message),
      patchRequestClientPrototype: (klass) => patchCodexServiceTierRequestClientPrototype(klass),
      readServiceTierSetting: () => getCodexServiceTierSetting(),
      installDispatcherPatch: () => installCodexServiceTierDispatcherPatch(),
      installRequestClientPatch: () => installCodexServiceTierRequestClientPatch(),
      resetServiceTierInstallState: () => {
        clearCodexServiceTierDispatcherPatchRetry(true);
        clearCodexServiceTierRequestClientPatchRetry(true);
        window.__codexServiceTierDispatcherPatchPromise = null;
        window.__codexServiceTierRequestClientPatchPromise = null;
        delete window.__codexServiceTierRequestOverrideInstalled;
        delete window.__codexServiceTierRequestClientPatchInstalled;
        codexServiceTierDispatcher = null;
      },
      serviceTierInstallState: () => ({
        dispatcherInstalled:
          window.__codexServiceTierRequestOverrideInstalled === codexServiceTierRequestOverrideVersion,
        requestClientInstalled:
          window.__codexServiceTierRequestClientPatchInstalled === codexServiceTierRequestOverrideVersion,
        dispatcherRetryPending: !!window.__codexServiceTierDispatcherPatchRetryTimer,
        requestClientRetryPending: !!window.__codexServiceTierRequestClientPatchRetryTimer,
      }),
      setModuleLoader: (loader) => {
        codexAppModuleLoaderForTest = typeof loader === "function" ? loader : null;
        codexServiceTierModulePromises.clear();
      },
      applyBackendSettings: (settings, reason = "settings-loaded") =>
        applyLoadedBackendSettings(settings, reason),
      diagnostics: () => [...(window.__codexElvesServiceTierTestDiagnostics || [])],
      setModelCatalog: (catalog = {}) => {
        codexModelCatalog = {
          status: "ok",
          model: "",
          default_model: "",
          model_provider: "",
          provider_name: "",
          models: [],
          sources: [],
          responses_api: { status: "unknown", message: "" },
          ...catalog,
        };
        codexModelCatalogLoadedAt = Date.now();
        codexModelCatalogPromise = null;
      },
      modelNames: () => codexElvesModelNames(),
      modelMatchesText: (slug, text) => codexServiceTierModelMatchesText(slug, text),
      catalogSlugForText: (text) => codexServiceTierCatalogModelMatch(text, true).slug || null,
      patchStatsigModelVisibilityConfig: (config) => patchStatsigModelVisibilityConfig(config),
      patchPluginMarketplaceRequestParams: (method, params) => patchPluginMarketplaceRequestParams(method, params),
      patchPluginMarketplaceRequestClient: (client) => patchPluginMarketplaceRequestClient(client),
      patchPluginMarketplaceResult: (method, result) => patchPluginMarketplaceResult(method, result),
      setServiceTierState: (state = {}) => {
        codexServiceTierState = { ...codexServiceTierState, ...state };
      },
      refreshBadgeNode: (node) => {
        const originalQuerySelectorAll = document.querySelectorAll;
        document.querySelectorAll = (selector) => selector === `[data-codex-service-tier-badge="true"]`
          ? [node]
          : originalQuerySelectorAll.call(document, selector);
        try {
          refreshCodexServiceTierBadges();
        } finally {
          document.querySelectorAll = originalQuerySelectorAll;
        }
        return node;
      },
      setThreadState: (state = {}) => {
        localStorage.setItem(codexThreadServiceTierKey, JSON.stringify({
          version: codexThreadServiceTierVersion,
          mode: "inherit",
          defaultMode: "inherit",
          entries: {},
          ...state,
        }));
      },
    };
    return;
  }

  function codexElvesModelNames() {
    return uniqueValues([
      ...(Array.isArray(codexModelCatalog.models) ? codexModelCatalog.models : []),
      codexModelCatalog.default_model,
      codexModelCatalog.model,
    ]);
  }

  function patchStatsigModelVisibilityConfig(config) {
    const value = config?.value;
    if (!value || typeof value !== "object" || value.use_hidden_models === false) return config;
    const nextValue = {
      ...value,
      use_hidden_models: false,
    };
    try {
      config.value = nextValue;
      return config;
    } catch {
      return { ...config, value: nextValue };
    }
  }

  function patchStatsigModelVisibilityTarget(target) {
    if (!target || typeof target.getDynamicConfig !== "function") return false;
    if (target.__codexElvesModelVisibilityPatch === codexStatsigModelVisibilityPatchVersion) {
      return true;
    }
    const originalGetDynamicConfig =
      target.__codexElvesModelVisibilityOriginalGetDynamicConfig ||
      target.getDynamicConfig;
    target.__codexElvesModelVisibilityOriginalGetDynamicConfig = originalGetDynamicConfig;
    target.getDynamicConfig = function codexElvesModelVisibilityGetDynamicConfig(name, ...args) {
      const config = originalGetDynamicConfig.call(this, name, ...args);
      return name === codexStatsigModelVisibilityConfigId
        ? patchStatsigModelVisibilityConfig(config)
        : config;
    };
    target.__codexElvesModelVisibilityPatch = codexStatsigModelVisibilityPatchVersion;
    return true;
  }

  function patchStatsigModelVisibilityClients() {
    const root = window.__STATSIG__ || globalThis.__STATSIG__;
    if (!root || typeof root !== "object") return 0;
    const targets = [
      root.StatsigClient?.prototype,
      root.firstInstance,
      typeof root.instance === "function" ? root.instance() : null,
      ...(root.instances && typeof root.instances === "object" ? Object.values(root.instances) : []),
    ].filter((target, index, all) => target && all.indexOf(target) === index);
    return targets.filter(patchStatsigModelVisibilityTarget).length;
  }

  async function invalidateCodexNativeModelList(source) {
    try {
      const dispatcher = codexServiceTierDispatcher || await findCodexServiceTierDispatcher();
      if (!dispatcher || typeof dispatcher.dispatchMessage !== "function") return false;
      codexServiceTierDispatcher = dispatcher;
      dispatcher.dispatchMessage("query-cache-invalidate", {
        queryKey: ["models", "list"],
      });
      window.dispatchEvent(new Event("resize"));
      sendCodexElvesDiagnostic("model_visibility_query_invalidated", { source });
      return true;
    } catch (error) {
      sendCodexElvesDiagnostic("model_visibility_query_invalidate_failed", {
        source,
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      return false;
    }
  }

  function installStatsigModelVisibilityPatch() {
    if (!codexModelCatalog.model_provider || codexElvesModelNames().length === 0) {
      return Promise.resolve(false);
    }
    if (codexStatsigModelVisibilityPatchPromise) {
      return codexStatsigModelVisibilityPatchPromise;
    }
    const runtimeId = codexSessionPrewarmRuntimeId;
    const install = async () => {
      const startedAt = Date.now();
      while (
        runtimeId === window.__codexSessionPrewarmRuntimeId &&
        Date.now() - startedAt < codexStatsigModelVisibilityMaxWaitMs
      ) {
        const patchedTargetCount = patchStatsigModelVisibilityClients();
        if (patchedTargetCount > 0) {
          window.__codexElvesStatsigModelVisibilityPatchInstalled =
            codexStatsigModelVisibilityPatchVersion;
          await invalidateCodexNativeModelList("statsig-visibility");
          sendCodexElvesDiagnostic("model_visibility_patch_installed", {
            patchedTargetCount,
            modelCount: codexElvesModelNames().length,
          });
          return true;
        }
        await new Promise((resolve) =>
          setTimeout(resolve, codexStatsigModelVisibilityRetryDelayMs)
        );
      }
      sendCodexElvesDiagnostic("model_visibility_patch_timeout", {
        maxWaitMs: codexStatsigModelVisibilityMaxWaitMs,
      });
      return false;
    };
    const promise = install().finally(() => {
      if (codexStatsigModelVisibilityPatchPromise === promise) {
        codexStatsigModelVisibilityPatchPromise = null;
      }
    });
    codexStatsigModelVisibilityPatchPromise = promise;
    return promise;
  }

  async function loadCodexModelCatalog(force = false) {
    if (!force && codexModelCatalogPromise) return codexModelCatalogPromise;
    if (!force && codexModelCatalogLoadedAt && Date.now() - codexModelCatalogLoadedAt < 10000) return codexModelCatalog;
    codexModelCatalogPromise = postJson("/codex-model-catalog", {})
      .then((result) => {
        codexModelCatalog = result && typeof result === "object" ? result : { status: "failed", model: "", default_model: "", model_provider: "", provider_name: "", models: [], sources: [], responses_api: { status: "unknown", message: "" } };
        codexModelCatalogLoadedAt = Date.now();
        renderCodexElvesMenu();
        void installStatsigModelVisibilityPatch();
        return codexModelCatalog;
      })
      .catch((error) => {
        codexModelCatalog = { status: "failed", message: String(error?.message || error), model: "", default_model: "", model_provider: "", provider_name: "", models: [], sources: [], responses_api: { status: "unknown", message: "" } };
        codexModelCatalogLoadedAt = Date.now();
        return codexModelCatalog;
      })
      .finally(() => {
        codexModelCatalogPromise = null;
      });
    return codexModelCatalogPromise;
  }

  function appServerRequestMethod(method, params) {
    if (method === "send-cli-request-for-host" && params?.method) return String(params.method);
    if (method === "vscode://codex/list-plugins") return "list-plugins";
    if (method === "vscode://codex/plugin/install") return "install-plugin";
    if (method === "vscode://codex/plugin/uninstall") return "uninstall-plugin";
    if (method === "plugin/list") return "list-plugins";
    if (method === "plugin/install") return "install-plugin";
    if (method === "plugin/uninstall") return "uninstall-plugin";
    return String(method || "");
  }

  function threadIdVariants(sessionId) {
    if (typeof sessionId !== "string" || !sessionId.trim()) return [];
    const id = sessionId.trim();
    const bareId = id.startsWith("local:") ? id.slice("local:".length) : id;
    return uniqueValues([id, bareId, `local:${bareId}`]);
  }

  function projectMoveSessionKey(sessionId) {
    const variants = threadIdVariants(sessionId);
    const bareId = variants.find((id) => !id.startsWith("local:"));
    return bareId || variants[0] || "";
  }

  // ── 已删除会话的持久抑制（不依赖 Codex 内部 manager）──
  // Codex App 升级后原生归档 manager 可能不可用，导致删除后展开项目会话复现。
  // 这里维护一份持久化的“已删除 thread ID 集合”，在 DOM 层拦截并移除任何复现的会话行，
  // 无论 App 如何重渲染或后续更新都能兵底。
  function normalizeSuppressedThreadId(raw) {
    if (typeof raw !== "string") return "";
    const trimmed = raw.trim();
    const bare = trimmed.startsWith("local:") ? trimmed.slice("local:".length) : trimmed;
    return bare.trim().toLowerCase();
  }

  if (!(window.__codexElvesSuppressedThreadIds instanceof Set)) {
    const initial = Array.isArray(window.__CODEX_ELVES_SUPPRESSED_THREADS__)
      ? window.__CODEX_ELVES_SUPPRESSED_THREADS__
      : [];
    window.__codexElvesSuppressedThreadIds = new Set(initial.map(normalizeSuppressedThreadId).filter(Boolean));
  }

  function suppressedThreadIds() {
    return window.__codexElvesSuppressedThreadIds;
  }

  function isThreadSuppressed(rawId) {
    const id = normalizeSuppressedThreadId(rawId);
    return !!id && suppressedThreadIds().has(id);
  }

  // 移除所有命中抑制集的侧边栏会话行。只在非归档页生效，避免误伤归档列表。
  function removeSuppressedThreadRows(root) {
    if (suppressedThreadIds().size === 0) return;
    if (typeof archivePageHintVisible === "function" && archivePageHintVisible()) return;
    const scope = root && typeof root.querySelectorAll === "function" ? root : document;
    let nodes;
    try {
      nodes = scope.querySelectorAll(selectors.sidebarThread);
    } catch {
      return;
    }
    nodes.forEach((node) => {
      const tid = node.getAttribute && node.getAttribute("data-app-action-sidebar-thread-id");
      if (tid && isThreadSuppressed(tid) && node.isConnected) {
        node.remove();
      }
    });
  }

  function installSuppressedThreadObserver() {
    removeSuppressedThreadRows();
    // 从后端拉一次最新抑制集并合并（启动注入前缀只是快照，这里补齐增量）。
    void postJson("/session/suppressed", {}).then((result) => {
      if (!result || !Array.isArray(result.ids)) return;
      let changed = false;
      result.ids.forEach((raw) => {
        const id = normalizeSuppressedThreadId(raw);
        if (id && !suppressedThreadIds().has(id)) {
          suppressedThreadIds().add(id);
          changed = true;
        }
      });
    if (changed) removeSuppressedThreadRows();
    }).catch(() => {});
    // 监听根优先收窄到侧边栏容器，避免主对话区流式输出时的大量 DOM 变更白白触发回调；
    // 侧边栏尚未渲染时回退到 documentElement 兵底。根变化时重连。
    // 依次尝试包含会话行的侧边栏根（必须真正包住会话行，否则会漏拦截）。
    const sidebarRoot = [
      document.querySelector("nav[role='navigation']"),
      document.querySelector("aside"),
      document.querySelector("nav"),
      document.querySelector(selectors.sidebarThread)?.closest?.("nav, aside, [role='navigation']"),
    ].find((candidate) => {
      try {
        return candidate && candidate.querySelector(selectors.sidebarThread);
      } catch {
        return false;
      }
    });
    const observerRoot = sidebarRoot || document.documentElement;
    if (window.__codexElvesSuppressedThreadObserver) {
      if (window.__codexElvesSuppressedThreadObserverRoot === observerRoot) return;
      window.__codexElvesSuppressedThreadObserver.disconnect();
    }
    const observer = new MutationObserver((mutations) => {
      if (suppressedThreadIds().size === 0) return;
      for (const mutation of mutations) {
        if (mutation.type === "attributes" && mutation.target instanceof Element) {
          const tid = mutation.target.getAttribute("data-app-action-sidebar-thread-id");
          if (tid && isThreadSuppressed(tid) && mutation.target.isConnected
            && !(typeof archivePageHintVisible === "function" && archivePageHintVisible())) {
            mutation.target.remove();
          }
        }
        for (const node of mutation.addedNodes || []) {
          if (node instanceof Element) removeSuppressedThreadRows(node);
        }
      }
    });
    observer.observe(observerRoot, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["data-app-action-sidebar-thread-id"],
    });
    window.__codexElvesSuppressedThreadObserver = observer;
    window.__codexElvesSuppressedThreadObserverRoot = observerRoot;
  }

  // 把 thread 加入抑制集（内存立即生效 + 后端持久化），并立即清除现有行。
  function suppressThreadEverywhere(rawId) {
    threadIdVariants(rawId).forEach((variant) => {
      const id = normalizeSuppressedThreadId(variant);
      if (id) suppressedThreadIds().add(id);
    });
    removeSuppressedThreadRows();
    installSuppressedThreadObserver();
    void postJson("/session/suppress", { thread_id: rawId }).catch(() => {});
  }

  function uuidV7TimestampMs(sessionId) {
    const id = projectMoveSessionKey(sessionId).replaceAll("-", "");
    if (!/^[0-9a-fA-F]{12}/.test(id)) return 0;
    const timestamp = Number.parseInt(id.slice(0, 12), 16);
    return Number.isFinite(timestamp) ? timestamp : 0;
  }

  function numericTimestamp(value) {
    const timestamp = Number(value);
    return Number.isFinite(timestamp) && timestamp > 0 ? timestamp : 0;
  }

  function timestampValueToMs(value) {
    const timestamp = numericTimestamp(value);
    if (!timestamp) return 0;
    return timestamp < 1000000000000 ? timestamp * 1000 : timestamp;
  }

  function sortMsForSession(sessionId, preferredValue) {
    return numericTimestamp(preferredValue) || uuidV7TimestampMs(sessionId);
  }

  function timestampMsFromPayload(payload) {
    return numericTimestamp(payload?.updated_at_ms) || timestampValueToMs(payload?.updated_at) || numericTimestamp(payload?.created_at_ms);
  }

  function relativeTimeLabel(timestampMs, nowMs = Date.now()) {
    const timestamp = numericTimestamp(timestampMs);
    if (!timestamp) return "";
    const elapsedSeconds = Math.max(0, Math.floor((nowMs - timestamp) / 1000));
    if (elapsedSeconds < 60) return "刚刚";
    const elapsedMinutes = Math.floor(elapsedSeconds / 60);
    if (elapsedMinutes < 60) return `${elapsedMinutes} 分`;
    const elapsedHours = Math.floor(elapsedMinutes / 60);
    if (elapsedHours < 24) return `${elapsedHours} 小时`;
    const elapsedDays = Math.floor(elapsedHours / 24);
    if (elapsedDays < 7) return `${elapsedDays} 天`;
    const elapsedWeeks = Math.floor(elapsedDays / 7);
    if (elapsedWeeks < 5) return `${elapsedWeeks} 周`;
    const elapsedMonths = Math.floor(elapsedDays / 30);
    if (elapsedMonths < 12) return `${Math.max(1, elapsedMonths)} 月`;
    return `${Math.floor(elapsedDays / 365)} 年`;
  }

  function taskBoardSessionTimeLabel(value) {
    const timestamp = numericTimestamp(value);
    if (!timestamp) return "时间未知";
    const date = new Date(timestamp);
    if (!Number.isFinite(date.getTime())) return "时间未知";
    const pad = (part) => String(part).padStart(2, "0");
    return [
      `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())}`,
      `${pad(date.getHours())}:${pad(date.getMinutes())}`,
    ].join(" ");
  }

  function normalizeWorkspacePath(path) {
    const normalized = String(path || "").trim().replace(/\\/g, "/").replace(/\/+$/, "");
    return normalized || String(path || "").trim();
  }

  function sameWorkspacePath(left, right) {
    const leftPath = normalizeWorkspacePath(left);
    const rightPath = normalizeWorkspacePath(right);
    return !!leftPath && !!rightPath && leftPath === rightPath;
  }

  function displayProjectName(path) {
    const trimmed = String(path || "").replace(/\/+$/, "");
    return trimmed.split(/[\\/]+/).filter(Boolean).pop() || trimmed || "未命名项目";
  }

  function normalizeProjectLabel(value) {
    return String(value || "").replace(/\s+/g, " ").trim();
  }

  function projectsSection() {
    return document.querySelector('[data-app-action-sidebar-section-heading="Projects"]');
  }

  function chatsSection() {
    return document.querySelector('[data-app-action-sidebar-section-heading="Chats"]');
  }

  function projectRowListItem(projectRow) {
    return projectRow.closest?.('[role="listitem"][aria-label]') || projectRow.closest?.('[role="listitem"]') || projectRow;
  }

  function nativeProjectTargets() {
    const section = projectsSection();
    const seen = new Set();
    const targets = [];
    Array.from(document.querySelectorAll('[data-app-action-sidebar-project-row]')).forEach((row) => {
      if (section && !section.contains(row)) return;
      const path = row.getAttribute("data-app-action-sidebar-project-id") || "";
      const normalizedPath = normalizeWorkspacePath(path);
      if (!normalizedPath || seen.has(normalizedPath)) return;
      const label = row.getAttribute("data-app-action-sidebar-project-label") || row.getAttribute("aria-label") || displayProjectName(path);
      seen.add(normalizedPath);
      targets.push({ kind: "project", label: String(label || displayProjectName(path)), description: path, path, normalizedPath, row, listItem: projectRowListItem(row) });
    });
    return targets;
  }

  const taskBoardState = {
    active: false,
    root: null,
    host: null,
    entry: null,
    hostObserver: null,
    resizeObserver: null,
    navigationHandler: null,
    navigationObserver: null,
    navigationObserverRoot: null,
    navigationVersion: 0,
    navigationReleaseId: 0,
    entryTemplateSignature: "",
    entryContextMenu: null,
    requestRevision: 0,
    snapshot: { schemaVersion: 1, revision: 0, tasks: [] },
    catalog: { projects: [], sessions: [], warnings: [] },
    snapshotError: "",
    catalogError: "",
    loading: false,
    pendingReadCount: 0,
    conversationStatuses: new Map(),
    conversationStatusRequestId: 0,
    conversationStatusTimer: null,
    conversationStatusRefreshPromise: null,
    query: "",
    projectCwd: "",
    nativeSelection: null,
    detachDialog: null,
    detachDialogKeydownHandler: null,
    detachDialogPreviousFocus: null,
    detachRequestId: 0,
    detachBusy: false,
    createModal: null,
    createModalKeydownHandler: null,
    createModalPreviousFocus: null,
    createModalRequestId: 0,
    nativeCreateOperation: null,
    nativeCreateRecoveryAttempted: false,
    moveBusy: false,
    moveRequestId: 0,
    moveServerSnapshot: null,
    moveFeedback: "",
    dragTaskId: "",
    moveFocusTaskId: "",
    dropdownMenu: null,
    dropdownMenuKeydownHandler: null,
    dropdownMenuDismissHandler: null,
    dropdownMenuViewportHandler: null,
    dropdownMenuPreviousFocus: null,
    statusMenu: null,
    projectMenu: null,
    projectOptionsCache: null,
    cardsRenderFrame: 0,
  };
  const taskBoardCatalogSessionMapCache = new WeakMap();
  const taskBoardCatalogSessionsByProjectCache = new WeakMap();
  const taskBoardTaskSearchTextCache = new WeakMap();
  const taskBoardLinkedConversationsCache = new WeakMap();

  const taskBoardNativeCreateRecoveryKey = "codexElvesTaskBoardNativeCreateRecoveryV1";
  const taskBoardNativeCreateRecoveryTtlMs = 24 * 60 * 60 * 1000;
  const taskBoardNativeCreatePermanentIdTimeoutMs = 15 * 1000;
  const taskBoardNativeSubmitReadyTimeoutMs = 15 * 1000;
  const taskBoardNativeSubmitTransitionGraceMs = 2 * 1000;
  const taskBoardNativeModelSelectionTimeoutMs = 5 * 1000;
  const taskBoardNativeCreateSessionRetryDelaysMs = [250, 750, 1500, 2500, 5000];
  const taskBoardNativeOpenSessionTimeoutMs = 5 * 1000;
  const taskBoardConversationStatusRefreshIntervalMs = 2500;
  const taskBoardConversationStatusIdleRefreshIntervalMs = 10000;
  const taskBoardConversationStatusMaxConcurrency = 4;
  const taskBoardNativeRuntimeLease = taskBoardNativeOperationLease();
  const taskBoardNativeRuntimeId = taskBoardNativeRuntimeLease?.runtimeId ||
    (Number(window.__codexElvesTaskBoardNativeRuntimeId || 0) + 1);
  window.__codexElvesTaskBoardNativeRuntimeId = taskBoardNativeRuntimeId;

  function taskBoardForeignNativeCreateLease() {
    const lease = taskBoardNativeOperationLease();
    if (!lease) return null;
    const localOperationId = String(
      taskBoardState.nativeCreateOperation?.operationId || "",
    ).trim();
    return lease.operationId === localOperationId ? null : lease;
  }

  function taskBoardNativeCreateBusyProbe() {
    return {
      status: "ok",
      canStart: false,
      canOpen: false,
      code: "native_create_busy",
      message: taskBoardNativeCreateBusyMessage,
    };
  }

  function taskBoardNativeCreateLeaseFailure() {
    return taskBoardForeignNativeCreateLease()
      ? taskBoardNativeFailure("native_create_busy", taskBoardNativeCreateBusyMessage)
      : null;
  }

  const taskBoardNativeAdapter = {
    openSession(sessionId, conversation) {
      const adapter = window.__codexElvesTaskBoardNativeAdapter;
      if (adapter && typeof adapter.openSession === "function") {
        return adapter.openSession(sessionId, conversation);
      }
      return taskBoardNativeOpenSession(sessionId, conversation);
    },
    probe(project) {
      if (taskBoardForeignNativeCreateLease()) {
        return Promise.resolve(taskBoardNativeCreateBusyProbe());
      }
      const adapter = window.__codexElvesTaskBoardNativeAdapter;
      if (adapter && typeof adapter.probe === "function") return adapter.probe(project);
      return taskBoardNativeProbe(project);
    },
    startConversation(project, firstInstruction, modelId = "", effortId = "") {
      const adapter = window.__codexElvesTaskBoardNativeAdapter;
      if (adapter && typeof adapter.startConversation === "function") {
        return adapter.startConversation(project, firstInstruction, modelId, effortId);
      }
      return taskBoardNativeStartConversation(project, firstInstruction, modelId, effortId);
    },
  };

  function taskBoardHostAppearanceHash(value) {
    const text = String(value || "");
    let hash = 2166136261;
    for (let index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 16777619);
    }
    return (hash >>> 0).toString(36);
  }

  const taskBoardHostAppearanceSourceSignatureCache = new WeakMap();

  function taskBoardHostAppearanceSourceSignature(config) {
    const source = String(config?.dataUrl || "");
    if (!config || typeof config !== "object") {
      return taskBoardHostAppearanceHash(source);
    }
    const cached = taskBoardHostAppearanceSourceSignatureCache.get(config);
    if (cached?.source === source) return cached.signature;
    const signature = taskBoardHostAppearanceHash(source);
    taskBoardHostAppearanceSourceSignatureCache.set(config, {
      source,
      signature,
    });
    return signature;
  }

  function taskBoardHostAppearanceOverlay() {
    const config = window.__CODEX_ELVES_IMAGE_OVERLAY__ || {};
    const kind = ["image", "color", "gradient"].includes(config.kind)
      ? config.kind
      : "image";
    const rawImageUrl = typeof config.imageUrl === "string"
      ? config.imageUrl.trim()
      : "";
    const imageUrl = /^data:/i.test(rawImageUrl) ? "" : rawImageUrl;
    const enabled = config.enabled === true && (kind !== "image" || !!imageUrl);
    const opacity = Math.min(1, Math.max(0.01, Number(config.opacity) || 0.35));
    const fit = config.fit === "cover" ? "cover" : "contain";
    const backgroundColor = typeof config.backgroundColor === "string" &&
      config.backgroundColor.trim()
      ? config.backgroundColor.trim()
      : "#1e293b";
    const gradientFrom = typeof config.gradientFrom === "string" &&
      config.gradientFrom.trim()
      ? config.gradientFrom.trim()
      : "#4338ca";
    const gradientTo = typeof config.gradientTo === "string" &&
      config.gradientTo.trim()
      ? config.gradientTo.trim()
      : "#0ea5e9";
    const gradientAngle = Number.isFinite(Number(config.gradientAngle))
      ? Number(config.gradientAngle)
      : 135;
    return {
      overlay: {
        enabled,
        kind,
        imageUrl,
        opacity,
        fit,
        backgroundColor,
        gradientFrom,
        gradientTo,
        gradientAngle,
      },
      sourceSignature: taskBoardHostAppearanceSourceSignature(config),
    };
  }

  function taskBoardHostAppearanceSignature(appearance, overlaySourceSignature = "") {
    const overlay = appearance?.overlay || {};
    return `task-board-appearance-v${appearance?.version || 1}-${taskBoardHostAppearanceHash([
      appearance?.background,
      appearance?.foreground,
      appearance?.panelBackground,
      appearance?.cardBackground,
      appearance?.cardBackgroundHover,
      appearance?.border,
      appearance?.borderSoft,
      appearance?.textSecondary,
      appearance?.textTertiary,
      appearance?.accent,
      appearance?.actionBackground,
      appearance?.actionBackgroundHover,
      appearance?.actionBackgroundActive,
      appearance?.actionForeground,
      appearance?.actionBorder,
      appearance?.modalBackground,
      appearance?.modalForeground,
      appearance?.modalBorder,
      appearance?.fieldBackground,
      appearance?.menuBackground,
      appearance?.rootFontFamily,
      appearance?.modalFontFamily,
      overlay.enabled ? "1" : "0",
      overlay.kind,
      overlay.imageUrl,
      overlay.opacity,
      overlay.fit,
      overlay.backgroundColor,
      overlay.gradientFrom,
      overlay.gradientTo,
      overlay.gradientAngle,
      overlaySourceSignature,
    ].join("\u001f"))}`;
  }

  function taskBoardHostAppearance() {
    installStyle();
    let root = document.querySelector(taskBoardRootSelector);
    let removeRoot = false;
    if (!root) {
      root = document.createElement("div");
      root.setAttribute(taskBoardRootAttribute, "true");
      root.style.cssText = "position:fixed!important;left:-10000px!important;top:-10000px!important;width:1px!important;height:1px!important;visibility:hidden!important;pointer-events:none!important;";
      document.documentElement.appendChild(root);
      removeRoot = true;
    }
    let modal = document.querySelector(".codex-task-board-create-modal");
    let removeModal = false;
    if (!modal) {
      modal = taskBoardElement("section", "codex-task-board-create-modal");
      modal.style.cssText = "position:fixed!important;left:-10000px!important;top:-10000px!important;width:1px!important;height:1px!important;visibility:hidden!important;pointer-events:none!important;";
      document.documentElement.appendChild(modal);
      removeModal = true;
    }
    const field = taskBoardElement("input", "codex-task-board-create-input");
    const menu = taskBoardElement("div", "codex-task-board-dropdown-menu");
    menu.style.cssText = "position:fixed!important;left:-10000px!important;top:-10000px!important;width:1px!important;height:1px!important;visibility:hidden!important;pointer-events:none!important;";
    modal.appendChild(field);
    document.documentElement.appendChild(menu);
    const rootStyle = getComputedStyle(root);
    const modalStyle = getComputedStyle(modal);
    const fieldStyle = getComputedStyle(field);
    const menuStyle = getComputedStyle(menu);
    const custom = (name, fallback) =>
      String(rootStyle.getPropertyValue(name) || "").trim() || fallback;
    const overlayContract = taskBoardHostAppearanceOverlay();
    const appearance = {
      version: 2,
      background: rootStyle.backgroundColor || "#1f1f1f",
      foreground: rootStyle.color || "#d4d4d4",
      panelBackground: custom("--task-board-panel-background", "#282828"),
      cardBackground: custom("--task-board-card-background", "#303030"),
      cardBackgroundHover: custom(
        "--task-board-card-background-hover",
        "color-mix(in srgb, #303030 88%, currentColor 8%)",
      ),
      border: custom(
        "--task-board-border",
        "color-mix(in srgb, currentColor 16%, transparent)",
      ),
      borderSoft: custom(
        "--task-board-border-soft",
        "color-mix(in srgb, currentColor 9%, transparent)",
      ),
      textSecondary: custom(
        "--task-board-text-secondary",
        "color-mix(in srgb, currentColor 72%, transparent)",
      ),
      textTertiary: custom(
        "--task-board-text-tertiary",
        "color-mix(in srgb, currentColor 54%, transparent)",
      ),
      accent: custom("--task-board-accent", "#63aee0"),
      actionBackground: custom(
        "--task-board-action-background",
        "color-mix(in srgb, currentColor 5%, transparent)",
      ),
      actionBackgroundHover: custom(
        "--task-board-action-background-hover",
        "color-mix(in srgb, currentColor 8%, transparent)",
      ),
      actionBackgroundActive: custom(
        "--task-board-action-background-active",
        "color-mix(in srgb, currentColor 12%, transparent)",
      ),
      actionForeground: custom(
        "--task-board-action-foreground",
        rootStyle.color || "#d4d4d4",
      ),
      actionBorder: custom(
        "--task-board-action-border",
        "color-mix(in srgb, currentColor 16%, transparent)",
      ),
      modalBackground: modalStyle.backgroundColor || "#2f2f2f",
      modalForeground: modalStyle.color || rootStyle.color || "#d4d4d4",
      modalBorder: modalStyle.borderTopColor || "rgba(212, 212, 212, .18)",
      fieldBackground: fieldStyle.backgroundColor || "#2b2b2b",
      menuBackground: menuStyle.backgroundColor || "#363636",
      rootFontFamily: rootStyle.fontFamily || "system-ui, sans-serif",
      modalFontFamily:
        modalStyle.fontFamily ||
        '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      overlay: overlayContract.overlay,
    };
    appearance.signature = taskBoardHostAppearanceSignature(
      appearance,
      overlayContract.sourceSignature,
    );
    field.remove();
    menu.remove();
    if (removeModal) modal.remove();
    if (removeRoot) root.remove();
    return { status: "ok", appearance };
  }

  async function taskBoardHostConversationStatuses(conversations = []) {
    const values = Array.isArray(conversations) ? conversations.slice(0, 256) : [];
    const unreadBySession = taskBoardNativeThreadUnreadIndex();
    const outcomes = await taskBoardMapSettledWithConcurrency(
      values,
      taskBoardConversationStatusMaxConcurrency,
      async (conversation) => {
        const sessionId = String(conversation?.sessionId || "").trim();
        let result = null;
        try {
          result = await postJson(taskBoardBridgeRoutes.conversationStatus, {
            session_id: sessionId,
            title: String(conversation?.title || "").trim(),
          });
        } catch {
          result = null;
        }
        const summary = result?.status === "ok" &&
          result?.summary &&
          typeof result.summary === "object"
          ? result.summary
          : null;
        return {
          sessionId,
          known: !!summary,
          checking: false,
          isRunning:
            summary?.isRunning === true ||
            summary?.lastTurnRunning === true,
          unread: taskBoardNativeThreadUnread(sessionId, unreadBySession),
        };
      },
    );
    return {
      status: "ok",
      statuses: outcomes.map((outcome, index) => {
        if (outcome?.status === "fulfilled") return outcome.value;
        const conversation = values[index];
        const sessionId = String(conversation?.sessionId || "").trim();
        return {
          sessionId,
          known: false,
          checking: false,
          isRunning: false,
          unread: taskBoardNativeThreadUnread(sessionId, unreadBySession),
        };
      }),
    };
  }

  const taskBoardHostCapabilities = Object.freeze({
    nativeCreateLease: true,
    nativeCreateRuntime: Number(taskBoardRuntimeVersion),
  });
  window.__codexElvesTaskBoardHost = {
    version: 3,
    capabilities: taskBoardHostCapabilities,
    openSession(sessionId, conversation = null) {
      return taskBoardNativeAdapter.openSession(sessionId, conversation);
    },
    probe(project) {
      return taskBoardNativeAdapter.probe(project);
    },
    async createOptions() {
      try {
        await loadCodexModelCatalog();
      } catch {
        // The default-model option remains valid when the live catalog is unavailable.
      }
      const modelId = taskBoardCreateDefaultModelId();
      return {
        status: "ok",
        modelId,
        effortId: taskBoardCreateDefaultEffortId(modelId),
        models: taskBoardCreateModelOptions().map((option) => ({
          ...option,
          efforts: taskBoardCreateEffortOptions(option.value),
        })),
      };
    },
    appearance() {
      return taskBoardHostAppearance();
    },
    conversationStatuses(conversations = []) {
      return taskBoardHostConversationStatuses(conversations);
    },
    startConversation(project, firstInstruction, modelId = "", effortId = "") {
      return taskBoardNativeAdapter.startConversation(
        project,
        firstInstruction,
        modelId,
        effortId,
      );
    },
  };

  function taskBoardNormalizedCwd(value) {
    let normalized = normalizeWorkspacePath(value);
    normalized = normalized
      .replace(/^\/\/\?\/unc\//i, "//")
      .replace(/^\/\/\?\//, "");
    if (/^[a-z]:\//i.test(normalized)) normalized = normalized.toLowerCase();
    return normalized;
  }

  function taskBoardNativeFailure(code, message) {
    return { status: "failed", code, message };
  }

  function taskBoardNativeSessionId(value) {
    return String(value || "").trim().replace(/^local:/i, "");
  }

  function taskBoardNativeSessionIdVariants(value) {
    const sessionId = taskBoardNativeSessionId(value);
    return sessionId ? new Set([sessionId, `local:${sessionId}`]) : new Set();
  }

  function taskBoardNativeThreadRow(sessionId) {
    const variants = taskBoardNativeSessionIdVariants(sessionId);
    if (!variants.size) return null;
    return Array.from(document.querySelectorAll("[data-app-action-sidebar-thread-id]")).find((row) => {
      return variants.has(String(row.getAttribute?.("data-app-action-sidebar-thread-id") || "").trim());
    }) || null;
  }

  function taskBoardConversationStatusKey(sessionId) {
    return taskBoardNativeSessionId(sessionId).toLocaleLowerCase();
  }

  function taskBoardNativeThreadUnreadIndex() {
    const unreadBySession = new Map();
    document.querySelectorAll("[data-app-action-sidebar-thread-id]").forEach((row) => {
      const key = taskBoardConversationStatusKey(
        row.getAttribute?.("data-app-action-sidebar-thread-id"),
      );
      if (!key) return;
      const unread = Array.from(row.querySelectorAll?.(".sr-only") || []).some((node) => {
        const text = String(node?.textContent || "").replace(/\s+/g, " ").trim();
        return text === "未读" || /^unread$/i.test(text);
      });
      unreadBySession.set(key, unread);
    });
    return unreadBySession;
  }

  function taskBoardNativeThreadUnread(sessionId, unreadBySession = null) {
    const key = taskBoardConversationStatusKey(sessionId);
    if (!key) return false;
    if (unreadBySession instanceof Map) return unreadBySession.get(key) === true;
    const row = taskBoardNativeThreadRow(sessionId);
    if (!row) return false;
    return Array.from(row.querySelectorAll?.(".sr-only") || []).some((node) => {
      const text = String(node?.textContent || "").replace(/\s+/g, " ").trim();
      return text === "未读" || /^unread$/i.test(text);
    });
  }

  function taskBoardNativeConversationLocation(sessionId, fallbackConversation = null) {
    const normalizedSessionId = taskBoardNativeSessionId(sessionId);
    if (!normalizedSessionId) return null;
    const variants = taskBoardNativeSessionIdVariants(normalizedSessionId);
    const catalogSession = (taskBoardState.catalog?.sessions || []).find((session) => {
      return variants.has(String(session?.sessionId || "").trim());
    }) || null;
    const fallback = fallbackConversation && variants.has(String(fallbackConversation?.sessionId || "").trim())
      ? fallbackConversation
      : null;
    let taskConversation = null;
    let taskProject = null;
    for (const task of taskBoardState.snapshot?.tasks || []) {
      const conversation = (task?.conversations || []).find((item) => {
        return variants.has(String(item?.sessionId || "").trim());
      });
      if (conversation) {
        taskConversation = conversation;
        taskProject = task?.project || null;
        break;
      }
    }
    const source = catalogSession || fallback || taskConversation;
    const cwd = taskBoardNormalizedCwd(
      catalogSession?.cwd || fallback?.cwd || taskConversation?.cwd || taskProject?.cwd,
    );
    return { sessionId: normalizedSessionId, cwd };
  }

  function taskBoardNativeProjectTarget(cwd) {
    const normalizedCwd = taskBoardNormalizedCwd(cwd);
    if (!normalizedCwd) return null;
    return nativeProjectTargets().find((target) => {
      return taskBoardNormalizedCwd(target?.path) === normalizedCwd;
    }) || null;
  }

  function taskBoardNativeProjectCollapsed(row) {
    const collapsed = row?.getAttribute?.("data-app-action-sidebar-project-collapsed");
    return (collapsed !== null && collapsed !== "false") || row?.getAttribute?.("aria-expanded") === "false";
  }

  async function taskBoardNativeOpenSession(sessionId, fallbackConversation = null) {
    const runtimeId = taskBoardNativeRuntimeId;
    if (!taskBoardNativeRuntimeCurrent(runtimeId)) {
      return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    }
    const location = taskBoardNativeConversationLocation(sessionId, fallbackConversation);
    if (!location) return taskBoardNativeFailure("session_unavailable", "关联会话不可用");
    const directRow = taskBoardNativeThreadRow(location.sessionId);
    if (directRow) {
      try {
        directRow.click?.();
        return { status: "ok" };
      } catch {
        return taskBoardNativeFailure("native_navigation_unavailable", "无法打开关联会话");
      }
    }
    const project = taskBoardNativeProjectTarget(location.cwd);
    if (!project?.row) {
      return taskBoardNativeFailure(
        location.cwd ? "native_navigation_unavailable" : "session_unavailable",
        location.cwd ? "关联会话所在项目不可用" : "关联会话不可用",
      );
    }
    if (taskBoardNativeProjectCollapsed(project.row)) {
      try {
        project.row.click?.();
      } catch {
        return taskBoardNativeFailure("native_navigation_unavailable", "无法展开关联会话所在项目");
      }
    }
    const deadlineMs = taskBoardNativeNow() + taskBoardNativeOpenSessionTimeoutMs;
    while (taskBoardNativeRuntimeCurrent(runtimeId) && taskBoardNativeNow() <= deadlineMs) {
      const row = taskBoardNativeThreadRow(location.sessionId);
      if (row) {
        try {
          row.click?.();
          return { status: "ok" };
        } catch {
          return taskBoardNativeFailure("native_navigation_unavailable", "无法打开关联会话");
        }
      }
      const remainingMs = deadlineMs - taskBoardNativeNow();
      if (remainingMs <= 0) break;
      await taskBoardNativeWait(Math.min(100, remainingMs));
    }
    return taskBoardNativeRuntimeCurrent(runtimeId)
      ? taskBoardNativeFailure("session_unavailable", "关联会话当前不可用")
      : taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
  }

  function taskBoardNativeProjectRow(project) {
    const cwd = taskBoardNormalizedCwd(project?.cwd);
    const rows = Array.from(document.querySelectorAll("[data-app-action-sidebar-project-row]"));
    const exact = cwd ? rows.find((row) => {
      return taskBoardNormalizedCwd(row.getAttribute?.("data-app-action-sidebar-project-id")) === cwd;
    }) : null;
    if (exact) return exact;
    const label = String(project?.label || "").trim().toLocaleLowerCase("zh-Hans-CN");
    if (!label) return null;
    const labelMatches = rows.filter((row) => {
      const rowLabel = String(
        row.getAttribute?.("data-app-action-sidebar-project-label") ||
        row.getAttribute?.("aria-label") ||
        row.textContent ||
        "",
      ).trim().toLocaleLowerCase("zh-Hans-CN");
      return rowLabel === label;
    });
    return labelMatches.length === 1 ? labelMatches[0] : null;
  }

  function taskBoardNativeStartButton(row) {
    if (!row) return null;
    return Array.from(row.querySelectorAll?.("button") || []).find((button) => {
      return !button.disabled &&
        !button.hasAttribute?.("aria-haspopup") &&
        !button.hasAttribute?.("data-app-action-sidebar-select-project");
    }) || null;
  }

  function taskBoardNativeComposer() {
    return document.querySelector('[data-codex-composer][contenteditable="true"][role="textbox"]');
  }

  function taskBoardNativeComposerController(composer) {
    if (!composer) return null;
    let owner = composer;
    let fiber = null;
    while (owner && !fiber) {
      const fiberKey = Object.keys(owner).find((key) => {
        return key.startsWith("__reactFiber$") || key.startsWith("__reactInternalInstance$");
      });
      fiber = fiberKey ? owner[fiberKey] : null;
      owner = owner.parentElement;
    }
    const seen = new Set();
    while (fiber && !seen.has(fiber)) {
      seen.add(fiber);
      const controller = fiber.memoizedProps?.composerController || fiber.pendingProps?.composerController;
      if (controller && typeof controller.setText === "function") return controller;
      fiber = fiber.return || fiber._debugOwner || null;
    }
    return null;
  }

  function taskBoardNativeReusableComposerProject(composer) {
    if (!(composer instanceof HTMLElement)) return "";
    const root = composer.closest?.(
      '[data-codex-composer-root][data-composer-placement="home"]',
    );
    if (!root) return "";
    const trigger = root.querySelector?.(
      '[data-composer-navigation-target="workspace-project"]',
    );
    const ariaLabel = String(trigger?.getAttribute?.("aria-label") || "")
      .replace(/^(?:切换项目|Switch project)\s*[:：]\s*/i, "")
      .trim();
    return ariaLabel || String(trigger?.textContent || "").replace(/\s+/g, " ").trim();
  }

  function taskBoardNativeComposerCanReuse(composer, project) {
    const expected = String(project?.label || "").trim().toLocaleLowerCase("zh-Hans-CN");
    const actual = taskBoardNativeReusableComposerProject(composer)
      .toLocaleLowerCase("zh-Hans-CN");
    return !!expected && actual === expected;
  }

  function taskBoardNativeSessionSignalId() {
    return String(
      document.querySelector("[data-above-composer-conversation-id]")
        ?.getAttribute?.("data-above-composer-conversation-id") ||
      "",
    ).trim();
  }

  function taskBoardNativePermanentSessionId() {
    const value = taskBoardNativeSessionSignalId();
    if (!value || isTemporaryThreadId(value)) return "";
    return validThreadSessionKey(value);
  }

  function taskBoardNativeNow() {
    const clock = window.__codexElvesTaskBoardNativeClock;
    return typeof clock?.now === "function" ? Number(clock.now()) : Date.now();
  }

  function taskBoardNativeWait(delayMs) {
    const clock = window.__codexElvesTaskBoardNativeClock;
    if (typeof clock?.wait === "function") return Promise.resolve(clock.wait(delayMs));
    return new Promise((resolve) => setTimeout(resolve, delayMs));
  }

  function taskBoardNativeRuntimeCurrent(runtimeId) {
    return window.__codexElvesTaskBoardNativeRuntimeId === runtimeId;
  }

  async function taskBoardNativeWaitForComposer(
    runtimeId,
    deadlineMs,
    previousComposer = null,
    project = null,
  ) {
    while (taskBoardNativeRuntimeCurrent(runtimeId) && taskBoardNativeNow() <= deadlineMs) {
      const composer = taskBoardNativeComposer();
      const controller = taskBoardNativeComposerController(composer);
      const ready =
        composer &&
        controller &&
        (
          composer !== previousComposer ||
          taskBoardNativeComposerCanReuse(composer, project)
        );
      if (ready) {
        return { composer, controller };
      }
      const remainingMs = deadlineMs - taskBoardNativeNow();
      if (remainingMs <= 0) break;
      await taskBoardNativeWait(Math.min(100, remainingMs));
    }
    return null;
  }

  async function taskBoardNativeWaitForValue(runtimeId, deadlineMs, resolveValue) {
    while (taskBoardNativeRuntimeCurrent(runtimeId) && taskBoardNativeNow() <= deadlineMs) {
      const value = resolveValue();
      if (value) return value;
      const remainingMs = deadlineMs - taskBoardNativeNow();
      if (remainingMs <= 0) break;
      await taskBoardNativeWait(Math.min(50, remainingMs));
    }
    return null;
  }

  const taskBoardNativeComposerControlSurfaceSelector = [
    "[data-composer-footer-responsive]",
    "[data-codex-composer-root]",
    ".composer-footer",
    '[class*="ComposerLayoutFooter"]',
    '[class*="_footer_"]',
  ].join(", ");

  function taskBoardNativeComposerControlSurface(composer) {
    if (!(composer instanceof HTMLElement)) return null;
    return composer.closest?.(taskBoardNativeComposerControlSurfaceSelector) || null;
  }

  function taskBoardNativeSubmitButtonLabelMatches(button) {
    const label = String(button?.getAttribute?.("aria-label") || "")
      .replace(/\s+/g, " ")
      .trim();
    if (!label) return false;
    return /^(?:发送|提交)(?:消息|指令|提示词)?(?:\s*(?:[（(][^）)]*[）)]|[：:].*))?\s*$/i.test(label) ||
      /^(?:send|submit)(?:\s+(?:message|prompt|instruction))?(?:\s*(?:\([^)]*\)|[:.].*))?\s*$/i.test(label);
  }

  function taskBoardNativeSubmitButtonInteractive(button) {
    if (!(button instanceof HTMLElement) || button.tagName !== "BUTTON") return false;
    if (
      button.isConnected === false ||
      button.hidden ||
      button.disabled ||
      button.getAttribute?.("aria-disabled") === "true" ||
      button.getAttribute?.("aria-hidden") === "true"
    ) {
      return false;
    }
    let style;
    try {
      style = getComputedStyle(button);
    } catch {
      return false;
    }
    if (
      style?.display === "none" ||
      style?.visibility === "hidden" ||
      style?.visibility === "collapse" ||
      style?.pointerEvents === "none" ||
      style?.opacity === "0"
    ) {
      return false;
    }
    const rect = button.getBoundingClientRect?.();
    const width = Number(rect?.width);
    const height = Number(rect?.height);
    return Number.isFinite(width) &&
      Number.isFinite(height) &&
      width >= 16 &&
      height >= 16 &&
      Number(rect?.right) > Number(rect?.left) &&
      Number(rect?.bottom) > Number(rect?.top);
  }

  function taskBoardNativeSubmitButton(composer) {
    const surface = taskBoardNativeComposerControlSurface(composer);
    if (!surface) return null;
    return Array.from(surface.querySelectorAll?.("button[aria-label]") || [])
      .find((button) => {
        return button.closest?.(taskBoardNativeComposerControlSurfaceSelector) === surface &&
          taskBoardNativeSubmitButtonLabelMatches(button) &&
          taskBoardNativeSubmitButtonInteractive(button);
      }) || null;
  }

  function taskBoardNativeComposerTexts(composer, controller) {
    const values = [];
    try {
      values.push(controller?.getText?.());
    } catch {}
    try {
      values.push(controller?.getPersistedText?.());
    } catch {}
    values.push(composer?.textContent, composer?.innerText);
    return uniqueValues(values)
      .map((value) => String(value || "").replace(/\r\n/g, "\n").trim())
      .filter(Boolean);
  }

  function taskBoardNativeComposerHasInstruction(
    composer,
    controller,
    instruction,
  ) {
    const expected = String(instruction || "").replace(/\r\n/g, "\n").trim();
    return !!expected && taskBoardNativeComposerTexts(composer, controller)
      .some((value) => value === expected);
  }

  function taskBoardNativeComposerStateCurrent(runtimeId, composer, controller) {
    return taskBoardNativeRuntimeCurrent(runtimeId) &&
      composer?.isConnected !== false &&
      taskBoardNativeComposer() === composer &&
      taskBoardNativeComposerController(composer) === controller;
  }

  async function taskBoardNativeWaitForSubmitControl(
    runtimeId,
    composer,
    controller,
    instruction,
  ) {
    const deadlineMs = taskBoardNativeNow() + taskBoardNativeSubmitReadyTimeoutMs;
    let instructionReady = false;
    while (taskBoardNativeRuntimeCurrent(runtimeId) && taskBoardNativeNow() <= deadlineMs) {
      if (!taskBoardNativeComposerStateCurrent(runtimeId, composer, controller)) {
        return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
      }
      if (taskBoardNativeComposerHasInstruction(composer, controller, instruction)) {
        instructionReady = true;
        const submitButton = taskBoardNativeSubmitButton(composer);
        if (submitButton) return { status: "ok", submitButton };
      }
      const remainingMs = deadlineMs - taskBoardNativeNow();
      if (remainingMs <= 0) break;
      await taskBoardNativeWait(Math.min(50, remainingMs));
    }
    if (!taskBoardNativeRuntimeCurrent(runtimeId)) {
      return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    }
    return instructionReady
      ? taskBoardNativeFailure(
        "composer_submit_unavailable",
        "原生发送按钮尚未就绪，请稍后重试",
      )
      : taskBoardNativeFailure(
        "composer_submit_failed",
        "原生编辑器未保留首条指令",
      );
  }

  function taskBoardNativeModelTrigger(composer = taskBoardNativeComposer()) {
    const selector = '[data-codex-intelligence-trigger="true"], [data-composer-navigation-target="reasoning"]';
    const composerSurface = taskBoardNativeComposerControlSurface(composer);
    if (!composerSurface) return null;
    const candidates = Array.from(composerSurface.querySelectorAll?.(selector) || [])
      .filter(codexServiceTierBadgeVisibleElement);
    return candidates.sort((left, right) => {
      const leftRect = left.getBoundingClientRect?.() || { bottom: 0 };
      const rightRect = right.getBoundingClientRect?.() || { bottom: 0 };
      return Number(rightRect.bottom || 0) - Number(leftRect.bottom || 0);
    })[0] || null;
  }

  function taskBoardNativeReadyModelTrigger(composer = taskBoardNativeComposer()) {
    const trigger = taskBoardNativeModelTrigger(composer);
    if (!trigger?.isConnected || trigger.disabled) return null;
    if (trigger.getAttribute?.("aria-haspopup") !== "menu") return null;
    if (trigger.getAttribute?.("aria-expanded") === null) return null;
    return codexServiceTierSelectedModelTexts(trigger).some((text) => {
      return !!String(text || "").replace(/\s+/g, " ").trim();
    })
      ? trigger
      : null;
  }

  function taskBoardNativeModelTextMatches(modelId, value) {
    const desired = normalizeCodexServiceTierModelName(modelId);
    const text = String(value || "").replace(/\s+/g, " ").trim();
    if (!desired || !text) return false;
    const entry = taskBoardCreateSelectedModelCatalogEntry(desired);
    const aliases = uniqueValues([
      desired,
      entry?.displayName,
      taskBoardCreateModelDisplayLabel({
        slug: entry?.slug || desired,
        displayName: "",
      }),
    ])
      .map(codexServiceTierModelMatchKey)
      .filter(Boolean);
    const textKey = codexServiceTierModelMatchKey(text);
    if (!textKey || !aliases.length) return false;
    const reasoningSuffixes = new Set(
      taskBoardReasoningEffortDefinitions.flatMap((effort) => [
        codexServiceTierModelMatchKey(effort.id),
        codexServiceTierModelMatchKey(effort.label),
      ]).filter(Boolean),
    );
    return aliases.some((alias) => {
      if (textKey === alias) return true;
      if (!textKey.startsWith(alias)) return false;
      return reasoningSuffixes.has(textKey.slice(alias.length));
    });
  }

  function taskBoardNativeCurrentModelMatches(modelId, knownTrigger = null) {
    const desired = normalizeCodexServiceTierModelName(modelId);
    if (!desired) return true;
    if (knownTrigger) {
      return codexServiceTierSelectedModelTexts(knownTrigger)
        .some((text) => taskBoardNativeModelTextMatches(modelId, text));
    }
    const selected = codexServiceTierComposerSelectedModel();
    if (selected && normalizeCodexServiceTierModelName(selected) === desired) return true;
    const trigger = taskBoardNativeModelTrigger();
    return codexServiceTierSelectedModelTexts(trigger)
      .some((text) => taskBoardNativeModelTextMatches(modelId, text));
  }

  function taskBoardNativeVisibleMenuItems() {
    return Array.from(document.querySelectorAll('[role="menuitem"], [role="menuitemradio"]'))
      .filter(codexServiceTierBadgeVisibleElement);
  }

  function taskBoardNativeModelSubmenuTrigger() {
    return taskBoardNativeVisibleMenuItems().find((item) => {
      if (item.getAttribute?.("aria-haspopup") !== "menu") return false;
      const label = String(
        item.getAttribute?.("aria-label") ||
        item.textContent ||
        "",
      ).replace(/\s+/g, " ").trim();
      return /^(模型|Model)(?:\s|$)/i.test(label);
    }) || null;
  }

  function taskBoardNativeModelOption(modelId) {
    return taskBoardNativeVisibleMenuItems().find((item) => {
      if (item.getAttribute?.("aria-haspopup") === "menu") return false;
      return [
        item.getAttribute?.("aria-label"),
        item.textContent,
      ].some((text) => taskBoardNativeModelTextMatches(modelId, text));
    }) || null;
  }

  function taskBoardNativeReasoningSubmenuTrigger() {
    return taskBoardNativeVisibleMenuItems().find((item) => {
      if (item.getAttribute?.("aria-haspopup") !== "menu") return false;
      const label = String(
        item.getAttribute?.("aria-label") ||
        item.textContent ||
        "",
      ).replace(/\s+/g, " ").trim();
      return /^(推理强度|Reasoning effort|Reasoning)(?:\s|$)/i.test(label);
    }) || null;
  }

  function taskBoardNativeReasoningOption(effortId) {
    const desired = String(effortId || "").trim().toLowerCase();
    const desiredLabel = taskBoardReasoningEffortLabel(desired);
    if (!desired) return null;
    return taskBoardNativeVisibleMenuItems().find((item) => {
      if (item.getAttribute?.("aria-haspopup") === "menu") return false;
      const value = String(
        item.getAttribute?.("data-value") ||
        item.getAttribute?.("data-reasoning-effort") ||
        item.getAttribute?.("data-effort") ||
        "",
      ).trim().toLowerCase();
      if (value === desired) return true;
      return [
        item.getAttribute?.("aria-label"),
        item.textContent,
      ].some((text) => {
        const label = String(text || "").replace(/\s+/g, " ").trim();
        return label === desired || label === desiredLabel;
      });
    }) || null;
  }

  function taskBoardNativeCurrentEffortMatches(effortId, knownTrigger = null) {
    const desired = String(effortId || "").trim().toLowerCase();
    if (!desired) return true;
    if (knownTrigger) {
      return String(
        knownTrigger.getAttribute?.("data-selected-reasoning-effort") ||
        "",
      ).trim().toLowerCase() === desired;
    }
    return taskBoardCurrentComposerReasoningEffort() === desired;
  }

  function taskBoardNativeActivateControl(control) {
    if (!control || typeof control.click !== "function") return false;
    try {
      control.click();
      if (
        control.getAttribute?.("aria-haspopup") === "menu" &&
        control.getAttribute?.("aria-expanded") !== "true" &&
        typeof control.dispatchEvent === "function"
      ) {
        const options = {
          bubbles: true,
          cancelable: true,
          button: 0,
          buttons: 1,
          pointerId: 1,
          pointerType: "mouse",
          isPrimary: true,
        };
        const event = typeof PointerEvent === "function"
          ? new PointerEvent("pointerdown", options)
          : Object.assign(new Event("pointerdown", options), options);
        control.dispatchEvent(event);
      }
      return true;
    } catch {
      return false;
    }
  }

  function taskBoardNativeCloseModelMenu(trigger) {
    if (trigger?.getAttribute?.("aria-expanded") !== "true") return;
    taskBoardNativeActivateControl(trigger);
  }

  async function taskBoardNativeSelectModel(runtimeId, modelId, composer) {
    const desired = String(modelId || "").trim();
    if (!desired) return { status: "ok", modelId: desired };
    const deadlineMs = taskBoardNativeNow() + taskBoardNativeModelSelectionTimeoutMs;
    const trigger = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => taskBoardNativeReadyModelTrigger(composer),
    );
    if (!trigger) {
      return taskBoardNativeFailure("native_model_unavailable", "未找到原生模型选择控件");
    }
    if (taskBoardNativeCurrentModelMatches(desired, trigger)) {
      return { status: "ok", modelId: desired };
    }
    if (trigger.getAttribute?.("aria-expanded") !== "true" &&
      !taskBoardNativeActivateControl(trigger)) {
      return taskBoardNativeFailure("native_model_unavailable", "无法打开原生模型选择控件");
    }
    const submenuTrigger = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      taskBoardNativeModelSubmenuTrigger,
    );
    if (!submenuTrigger) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_model_unavailable", "原生模型列表暂不可用");
    }
    if (submenuTrigger.getAttribute?.("aria-expanded") !== "true" &&
      !taskBoardNativeActivateControl(submenuTrigger)) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_model_unavailable", "无法打开原生模型列表");
    }
    const option = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => taskBoardNativeModelOption(desired),
    );
    if (!option) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_model_not_found", "所选模型在当前 Codex 会话中不可用");
    }
    if (!taskBoardNativeActivateControl(option)) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_model_select_failed", "无法切换到所选模型");
    }
    const selected = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => {
        const currentTrigger = taskBoardNativeReadyModelTrigger(composer);
        return currentTrigger &&
          taskBoardNativeCurrentModelMatches(desired, currentTrigger);
      },
    );
    if (!selected) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_model_select_failed", "切换模型后未读取到所选模型");
    }
    return { status: "ok", modelId: desired };
  }

  async function taskBoardNativeSelectReasoningEffort(
    runtimeId,
    effortId,
    composer,
  ) {
    const desired = String(effortId || "").trim().toLowerCase();
    if (!desired) return { status: "ok", effortId: desired };
    const deadlineMs = taskBoardNativeNow() + taskBoardNativeModelSelectionTimeoutMs;
    const trigger = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => taskBoardNativeReadyModelTrigger(composer),
    );
    if (!trigger) {
      return taskBoardNativeFailure("native_effort_unavailable", "未找到原生推理强度控件");
    }
    if (taskBoardNativeCurrentEffortMatches(desired, trigger)) {
      return { status: "ok", effortId: desired };
    }
    if (trigger.getAttribute?.("aria-expanded") !== "true" &&
      !taskBoardNativeActivateControl(trigger)) {
      return taskBoardNativeFailure("native_effort_unavailable", "无法打开原生推理强度控件");
    }
    const submenuTrigger = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      taskBoardNativeReasoningSubmenuTrigger,
    );
    if (!submenuTrigger) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_effort_unavailable", "原生推理强度列表暂不可用");
    }
    if (submenuTrigger.getAttribute?.("aria-expanded") !== "true" &&
      !taskBoardNativeActivateControl(submenuTrigger)) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_effort_unavailable", "无法打开原生推理强度列表");
    }
    const option = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => taskBoardNativeReasoningOption(desired),
    );
    if (!option) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_effort_not_found", "所选推理强度在当前模型中不可用");
    }
    if (!taskBoardNativeActivateControl(option)) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_effort_select_failed", "无法切换到所选推理强度");
    }
    const selected = await taskBoardNativeWaitForValue(
      runtimeId,
      deadlineMs,
      () => {
        const currentTrigger = taskBoardNativeReadyModelTrigger(composer);
        return currentTrigger &&
          taskBoardNativeCurrentEffortMatches(desired, currentTrigger);
      },
    );
    if (!selected) {
      taskBoardNativeCloseModelMenu(trigger);
      return taskBoardNativeFailure("native_effort_select_failed", "切换强度后未读取到所选值");
    }
    return { status: "ok", effortId: desired };
  }

  function taskBoardNativeSubmitComposer(
    runtimeId,
    composer,
    controller,
    submitButton,
    instruction,
  ) {
    if (!taskBoardNativeComposerStateCurrent(runtimeId, composer, controller)) {
      return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    }
    if (!taskBoardNativeComposerHasInstruction(composer, controller, instruction)) {
      return taskBoardNativeFailure("composer_submit_failed", "首条指令在提交前发生变化");
    }
    if (
      taskBoardNativeSubmitButton(composer) !== submitButton ||
      !taskBoardNativeSubmitButtonInteractive(submitButton)
    ) {
      return taskBoardNativeFailure(
        "composer_submit_unavailable",
        "原生发送按钮在提交前不可用",
      );
    }
    try {
      submitButton.click();
      return { status: "ok" };
    } catch {
      return taskBoardNativeFailure("composer_submit_failed", "无法点击原生发送按钮");
    }
  }

  function taskBoardNativeProbe(project) {
    if (taskBoardForeignNativeCreateLease()) {
      return Promise.resolve(taskBoardNativeCreateBusyProbe());
    }
    const row = taskBoardNativeProjectRow(project);
    const button = taskBoardNativeStartButton(row);
    const composer = taskBoardNativeComposer();
    const controller = taskBoardNativeComposerController(composer);
    const canStart = !!row && !!button && !!composer && !!controller;
    return Promise.resolve({
      status: "ok",
      canStart,
      canOpen: false,
      code: canStart ? "" : "native_create_unavailable",
      message: canStart ? "" : "当前项目暂不支持新建关联会话",
    });
  }

  async function taskBoardNativeStartConversation(
    project,
    firstInstruction,
    modelId = "",
    effortId = "",
  ) {
    const instruction = String(firstInstruction || "").trim();
    if (!instruction) return taskBoardNativeFailure("invalid_input", "请输入首条指令");
    const runtimeId = taskBoardNativeRuntimeId;
    const row = taskBoardNativeProjectRow(project);
    const button = taskBoardNativeStartButton(row);
    if (!row || !button) return taskBoardNativeFailure("native_create_unavailable", "当前项目暂不支持新建关联会话");
    const previousSessionId = taskBoardNativePermanentSessionId();
    const previousSessionSignalId = taskBoardNativeSessionId(
      taskBoardNativeSessionSignalId(),
    );
    const previousComposer = taskBoardNativeComposer();
    try {
      button.click?.();
    } catch {
      return taskBoardNativeFailure("native_create_unavailable", "无法启动当前项目的新会话");
    }
    const composerDeadlineMs = taskBoardNativeNow() + taskBoardNativeCreatePermanentIdTimeoutMs;
    const composerState = await taskBoardNativeWaitForComposer(
      runtimeId,
      composerDeadlineMs,
      previousComposer,
      project,
    );
    if (!taskBoardNativeRuntimeCurrent(runtimeId)) return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    if (!composerState) return taskBoardNativeFailure("composer_unavailable", "未找到原生会话编辑器");
    const { composer, controller } = composerState;
    const modelSelection = await taskBoardNativeSelectModel(
      runtimeId,
      modelId,
      composer,
    );
    if (modelSelection?.status !== "ok") return modelSelection;
    const effortSelection = await taskBoardNativeSelectReasoningEffort(
      runtimeId,
      effortId,
      composer,
    );
    if (effortSelection?.status !== "ok") return effortSelection;
    try {
      controller.focus?.();
      controller.setText(instruction);
    } catch {
      return taskBoardNativeFailure("composer_submit_failed", "无法写入首条指令");
    }
    const submitState = await taskBoardNativeWaitForSubmitControl(
      runtimeId,
      composer,
      controller,
      instruction,
    );
    if (submitState?.status !== "ok") return submitState;
    const submitted = taskBoardNativeSubmitComposer(
      runtimeId,
      composer,
      controller,
      submitState.submitButton,
      instruction,
    );
    if (submitted?.status !== "ok") return submitted;
    const submittedAtMs = taskBoardNativeNow();
    let submitTransitionObserved = false;
    const deadlineMs = taskBoardNativeNow() + taskBoardNativeCreatePermanentIdTimeoutMs;
    while (taskBoardNativeNow() <= deadlineMs) {
      const sessionSignalId = taskBoardNativeSessionId(
        taskBoardNativeSessionSignalId(),
      );
      if (sessionSignalId && sessionSignalId !== previousSessionSignalId) {
        submitTransitionObserved = true;
      }
      const sessionId = taskBoardNativePermanentSessionId();
      if (sessionId && sessionId !== previousSessionId) {
        const ref = currentSessionRef();
        return {
          status: "ok",
          sessionId,
          title: ref?.session_id === sessionId ? String(ref?.title || "") : "",
          cwd: taskBoardNormalizedCwd(project?.cwd),
        };
      }
      if (!taskBoardNativeRuntimeCurrent(runtimeId)) {
        return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
      }
      if (
        !submitTransitionObserved &&
        !taskBoardNativeComposerStateCurrent(runtimeId, composer, controller) &&
        taskBoardNativeNow() - submittedAtMs >= taskBoardNativeSubmitTransitionGraceMs
      ) {
        return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
      }
      const remainingMs = deadlineMs - taskBoardNativeNow();
      if (remainingMs <= 0) break;
      await taskBoardNativeWait(Math.min(100, remainingMs));
    }
    return taskBoardNativeRuntimeCurrent(runtimeId)
      ? taskBoardNativeFailure("native_create_timeout", "等待新会话就绪超时")
      : taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
  }

  function taskBoardStatusId(value) {
    const status = String(value || "").trim();
    return taskBoardStatusDefinitions.some((item) => item.id === status) ? status : "new";
  }

  function taskBoardElement(tag, className, text = "") {
    const element = document.createElement(tag);
    if (className) element.className = className;
    if (text) element.textContent = text;
    return element;
  }

  function taskBoardDropdownChevron() {
    const icon = taskBoardElement("span", "codex-task-board-dropdown-chevron");
    icon.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4"><path d="m5 6 3 3 3-3" stroke-linecap="round" stroke-linejoin="round"></path></svg>`;
    return icon;
  }

  function taskBoardDropdownCheck() {
    const marker = taskBoardElement("span", "codex-task-board-dropdown-option-marker");
    marker.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4"><path d="m3.5 8.2 2.8 2.8 6.2-6.2" stroke-linecap="round" stroke-linejoin="round"></path></svg>`;
    return marker;
  }

  function taskBoardDropdownStatusDot(color = "") {
    const normalized = String(color || "").trim();
    if (!normalized) return null;
    const dot = taskBoardElement("span", "codex-task-board-dropdown-status-dot");
    dot.style.setProperty("--task-board-status-color", normalized);
    dot.setAttribute("aria-hidden", "true");
    return dot;
  }

  function taskBoardConfigureDropdownTrigger(trigger, label, ariaLabel, color = "") {
    if (!trigger) return trigger;
    trigger.classList.add("codex-task-board-dropdown-trigger");
    trigger.setAttribute("aria-label", ariaLabel);
    trigger.setAttribute("aria-haspopup", "listbox");
    trigger.setAttribute("aria-expanded", "false");
    const labelNode = taskBoardElement("span", "codex-task-board-dropdown-label", label);
    const copy = taskBoardElement("span", "codex-task-board-dropdown-trigger-copy");
    const dot = taskBoardDropdownStatusDot(color);
    if (dot) copy.appendChild(dot);
    copy.appendChild(labelNode);
    trigger.replaceChildren(copy, taskBoardDropdownChevron());
    return trigger;
  }

  function taskBoardSetDropdownTriggerLabel(trigger, label, color = "") {
    const labelNode = trigger?.querySelector?.(".codex-task-board-dropdown-label");
    if (labelNode) labelNode.textContent = String(label || "");
    const copy = trigger?.querySelector?.(".codex-task-board-dropdown-trigger-copy");
    const currentDot = copy?.querySelector?.(".codex-task-board-dropdown-status-dot");
    const nextColor = String(color || "").trim();
    if (nextColor) {
      const dot = currentDot || taskBoardDropdownStatusDot(nextColor);
      dot?.style?.setProperty?.("--task-board-status-color", nextColor);
      if (!currentDot && dot) copy?.prepend?.(dot);
    } else {
      currentDot?.remove?.();
    }
    if (trigger) trigger.title = String(label || "");
  }

  function taskBoardConfigureCreateModelTrigger(
    trigger,
    modelLabel = "默认模型",
    effortLabel = "",
  ) {
    if (!trigger) return trigger;
    trigger.setAttribute("aria-haspopup", "menu");
    trigger.setAttribute("aria-expanded", "false");
    trigger.replaceChildren(
      taskBoardElement(
        "span",
        "codex-task-board-create-model-trigger-label",
        modelLabel,
      ),
      taskBoardElement(
        "span",
        "codex-task-board-create-effort-trigger-label",
        effortLabel,
      ),
    );
    return trigger;
  }

  function taskBoardSetCreateModelTriggerLabel(trigger, modelLabel, effortLabel) {
    const modelLabelNode = trigger?.querySelector?.(".codex-task-board-create-model-trigger-label");
    const effortLabelNode = trigger?.querySelector?.(".codex-task-board-create-effort-trigger-label");
    if (modelLabelNode) modelLabelNode.textContent = String(modelLabel || "");
    if (effortLabelNode) effortLabelNode.textContent = String(effortLabel || "");
  }

  function taskBoardMessageFromResult(result, fallback) {
    return String(result?.message || fallback || "读取任务看板失败").trim();
  }

  function taskBoardSnapshotResult(result) {
    if (!result || (result.status && result.status !== "ok") || !Array.isArray(result.tasks)) return null;
    return {
      schemaVersion: Number.isSafeInteger(result.schemaVersion) ? result.schemaVersion : 1,
      revision: Number.isSafeInteger(result.revision) && result.revision >= 0 ? result.revision : 0,
      tasks: result.tasks,
    };
  }

  function taskBoardConflictSnapshotResult(result) {
    if (!result || result.status !== "conflict" || !Array.isArray(result.tasks)) return null;
    return {
      schemaVersion: Number.isSafeInteger(result.schemaVersion) ? result.schemaVersion : 1,
      revision: Number.isSafeInteger(result.revision) && result.revision >= 0 ? result.revision : 0,
      tasks: result.tasks,
    };
  }

  function taskBoardCatalogResult(result) {
    if (!result || (result.status && result.status !== "ok")) return null;
    return {
      projects: Array.isArray(result.projects) ? result.projects : [],
      sessions: Array.isArray(result.sessions) ? result.sessions : [],
      warnings: Array.isArray(result.warnings) ? result.warnings : [],
    };
  }

  async function taskBoardMockOrBridgeResult(kind, payload = {}) {
    const route = taskBoardBridgeRoutes[kind];
    const mock = window.__codexElvesTaskBoardMock;
    if (mock && typeof mock.request === "function") {
      return mock.request(route, payload);
    }
    const mockValue = mock?.[kind];
    if (typeof mockValue === "function") return mockValue(payload);
    if (mockValue && typeof mockValue === "object") return mockValue;
    return postJson(route, payload);
  }

  function taskBoardFeatureEnabled() {
    return !!codexElvesSettings().taskBoard;
  }

  function taskBoardNavigationForControl(control) {
    return control?.closest?.("nav, [role='navigation']") || null;
  }

  function taskBoardControlInSettingsNavigation(control) {
    const navigation = taskBoardNavigationForControl(control);
    const label = normalizedPluginEntryLabel(navigation?.getAttribute?.("aria-label"));
    return !!label && taskBoardSettingsNavigationLabelPattern.test(label);
  }

  function taskBoardPrimaryNavigationHasSidebarContent(control) {
    const navigation = taskBoardNavigationForControl(control);
    return !!navigation?.querySelector?.(
      "[data-app-action-sidebar-section-heading], " +
      "[data-app-action-sidebar-thread-id], " +
      "[data-app-action-sidebar-project-row]",
    );
  }

  function taskBoardPluginEntryButton() {
    const navigationControls = pluginNavigationControls()
      .filter((control) => !taskBoardControlInSettingsNavigation(control));
    const semanticMatches = navigationControls.filter(pluginEntryControlMatches);
    const semanticSidebarMatch = semanticMatches.find(taskBoardPrimaryNavigationHasSidebarContent);
    if (semanticSidebarMatch) return semanticSidebarMatch;
    if (semanticMatches.length) return semanticMatches[0];

    const legacyMatches = navigationControls.filter((control) => {
      return !!control?.querySelector?.(selectors.pluginSvgPath);
    });
    const legacySidebarMatch = legacyMatches.find(taskBoardPrimaryNavigationHasSidebarContent);
    if (legacySidebarMatch) return legacySidebarMatch;
    if (legacyMatches.length) return legacyMatches[0];

    const globalSemanticMatches = Array.from(
      document.querySelectorAll('button, [role="button"], a[href]'),
    )
      .filter(pluginEntryControlMatches)
      .filter((control) => !taskBoardControlInSettingsNavigation(control));
    return globalSemanticMatches.length === 1 ? globalSemanticMatches[0] : null;
  }

  function taskBoardMainHost() {
    return document.querySelector("main[data-app-shell-main-surface]") ||
      document.querySelector("main") ||
      document.querySelector("[role='main']");
  }

  function taskBoardEntryButtons() {
    return Array.from(document.querySelectorAll(taskBoardEntrySelector))
      .filter((entry) => entry instanceof HTMLButtonElement);
  }

  function taskBoardEntryTemplateSignature(pluginButton) {
    if (!pluginButton) return "";
    return [pluginButton, ...Array.from(pluginButton.querySelectorAll?.("*") || [])]
      .map((node) => `${node.tagName || ""}:${node.children?.length || 0}`)
      .join(">");
  }

  function taskBoardEntryLabelNode(entry) {
    return Array.from(entry?.querySelectorAll?.("span, div") || [])
      .reverse()
      .find((node) => {
        const text = String(node.textContent || "").replace(/\s+/g, " ").trim();
        return pluginEntryLabelPattern.test(text) && !node.querySelector?.("svg");
      }) || null;
  }

  function taskBoardNavigationIcon(templateIcon) {
    const namespace = "http://www.w3.org/2000/svg";
    const icon = document.createElementNS(namespace, "svg");
    icon.setAttribute("aria-hidden", "true");
    icon.setAttribute("width", "16");
    icon.setAttribute("height", "16");
    icon.setAttribute("viewBox", "0 0 16 16");
    icon.setAttribute("fill", "none");
    const templateClass = templateIcon?.getAttribute?.("class");
    if (templateClass) icon.setAttribute("class", templateClass);
    [
      ["1.75", "2.25", "4.5", "11.5"],
      ["9.75", "2.25", "4.5", "7.5"],
    ].forEach(([x, y, width, height]) => {
      const column = document.createElementNS(namespace, "rect");
      column.setAttribute("x", x);
      column.setAttribute("y", y);
      column.setAttribute("width", width);
      column.setAttribute("height", height);
      column.setAttribute("rx", "1.25");
      column.setAttribute("stroke", "currentColor");
      column.setAttribute("stroke-width", "1.25");
      icon.appendChild(column);
    });
    return icon;
  }

  function closeTaskBoardEntryContextMenu({ restoreFocus = true } = {}) {
    const state = taskBoardState.entryContextMenu;
    if (!state) return;
    document.removeEventListener("keydown", state.keydownHandler, true);
    document.removeEventListener("pointerdown", state.dismissHandler, true);
    window.removeEventListener("resize", state.viewportHandler);
    window.removeEventListener("scroll", state.viewportHandler, true);
    window.removeEventListener("blur", state.blurHandler);
    state.entry?.removeAttribute?.("data-codex-task-board-context-menu-open");
    state.element?.remove?.();
    taskBoardState.entryContextMenu = null;
    if (restoreFocus && state.entry?.isConnected !== false) state.entry?.focus?.();
  }

  function taskBoardEntryContextMenuAnchor(entry, event) {
    const entryRect = entry?.getBoundingClientRect?.() || {
      left: 8,
      right: 220,
      top: 8,
      bottom: 44,
    };
    const clientX = Number(event?.clientX);
    const clientY = Number(event?.clientY);
    const pointerInvocation =
      event?.type === "contextmenu" &&
      Number.isFinite(clientX) &&
      Number.isFinite(clientY) &&
      (clientX !== 0 || clientY !== 0);
    return pointerInvocation
      ? { left: clientX, top: clientY }
      : {
          left: Number(entryRect.right || entryRect.left || 8) + 4,
          top: Number(entryRect.top || 8),
        };
  }

  function positionTaskBoardEntryContextMenu(menu, entry, event) {
    const anchor = taskBoardEntryContextMenuAnchor(entry, event);
    const viewportWidth = Number(window.innerWidth || 1024);
    const viewportHeight = Number(window.innerHeight || 768);
    const menuRect = menu.getBoundingClientRect?.() || { width: 188, height: 46 };
    const menuWidth = Math.max(188, Number(menuRect.width || 188));
    const menuHeight = Math.max(46, Number(menuRect.height || 46));
    const left = Math.min(
      viewportWidth - menuWidth - 8,
      Math.max(8, Number(anchor.left || 8)),
    );
    const top = Number(anchor.top || 8) + menuHeight <= viewportHeight - 8
      ? Number(anchor.top || 8)
      : Math.max(8, Number(anchor.top || 8) - menuHeight);
    menu.style.left = `${Math.max(8, left)}px`;
    menu.style.top = `${Math.max(8, top)}px`;
  }

  function taskBoardEntryContextMenuIcon() {
    const icon = taskBoardElement("span", "codex-task-board-entry-context-menu-icon");
    icon.setAttribute("aria-hidden", "true");
    icon.innerHTML = `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"><rect x="2.25" y="3.75" width="8.5" height="9" rx="1.4"></rect><path d="M8.5 2.5h5v5M13.5 2.5 7.75 8.25"></path></svg>`;
    return icon;
  }

  function openTaskBoardEntryContextMenu(entry, event) {
    if (!entry) return null;
    event?.preventDefault?.();
    event?.stopPropagation?.();
    event?.stopImmediatePropagation?.();
    closeTaskBoardEntryContextMenu({ restoreFocus: false });
    closeTaskBoardDropdownMenu({ restoreFocus: false });
    closeSessionMoreMenus();
    const menu = taskBoardElement("div", taskBoardEntryContextMenuClass);
    menu.setAttribute("role", "menu");
    menu.setAttribute("aria-label", "任务看板操作");
    const openWindow = taskBoardElement("button");
    openWindow.type = "button";
    openWindow.setAttribute("role", "menuitem");
    openWindow.append(
      taskBoardEntryContextMenuIcon(),
      taskBoardElement("span", "", "在新窗口中打开"),
    );
    openWindow.addEventListener("click", (activateEvent) => {
      activateEvent.preventDefault?.();
      activateEvent.stopPropagation?.();
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
      void openStandaloneTaskBoard();
    }, true);
    menu.appendChild(openWindow);
    document.body.appendChild(menu);
    entry.setAttribute("data-codex-task-board-context-menu-open", "true");
    positionTaskBoardEntryContextMenu(menu, entry, event);
    const state = {
      element: menu,
      entry,
      openWindow,
      keydownHandler: null,
      dismissHandler: null,
      viewportHandler: null,
      blurHandler: null,
    };
    state.keydownHandler = (keyEvent) => {
      if (keyEvent.key === "Escape") {
        keyEvent.preventDefault?.();
        closeTaskBoardEntryContextMenu();
      } else if (keyEvent.key === "Tab") {
        closeTaskBoardEntryContextMenu({ restoreFocus: false });
      } else if (
        keyEvent.key === "ArrowDown" ||
        keyEvent.key === "ArrowUp" ||
        keyEvent.key === "Home" ||
        keyEvent.key === "End"
      ) {
        keyEvent.preventDefault?.();
        openWindow.focus?.();
      } else if (keyEvent.key === "Enter" || keyEvent.key === " ") {
        if (document.activeElement !== openWindow) return;
        keyEvent.preventDefault?.();
        openWindow.click?.();
      }
    };
    state.dismissHandler = (dismissEvent) => {
      const target = dismissEvent.target;
      if (menu.contains?.(target) || entry.contains?.(target)) return;
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
    };
    state.viewportHandler = () => {
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
    };
    state.blurHandler = () => {
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
    };
    taskBoardState.entryContextMenu = state;
    document.addEventListener("keydown", state.keydownHandler, true);
    document.addEventListener("pointerdown", state.dismissHandler, true);
    window.addEventListener("resize", state.viewportHandler);
    window.addEventListener("scroll", state.viewportHandler, true);
    window.addEventListener("blur", state.blurHandler);
    requestAnimationFrame(() => openWindow.focus?.());
    return state;
  }

  function taskBoardCreateEntry(pluginButton) {
    const entry = pluginButton.cloneNode(true);
    entry.type = "button";
    entry.disabled = false;
    entry.removeAttribute("disabled");
    entry.removeAttribute("id");
    entry.removeAttribute("aria-current");
    entry.removeAttribute("aria-selected");
    entry.removeAttribute("aria-describedby");
    entry.removeAttribute("data-state");
    entry.removeAttribute("data-active");
    entry.removeAttribute("data-selected");
    entry.removeAttribute("data-app-action-sidebar-thread-active");
    entry.removeAttribute("data-app-action-sidebar-thread-selected");
    entry.querySelectorAll?.("[id]").forEach((node) => node.removeAttribute("id"));
    entry.classList?.remove?.("bg-primary-ghost-hover");
    const templateIcon = entry.querySelector?.("svg");
    templateIcon?.replaceWith?.(taskBoardNavigationIcon(templateIcon));
    const label = taskBoardEntryLabelNode(entry);
    if (label) label.textContent = "任务看板";
    else entry.appendChild(taskBoardElement("span", "", "任务看板"));
    entry.setAttribute(taskBoardEntryAttribute, "true");
    entry.setAttribute("aria-label", "任务看板");
    entry.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
      activateTaskBoard();
    });
    entry.addEventListener("contextmenu", (event) => {
      openTaskBoardEntryContextMenu(entry, event);
    });
    entry.addEventListener("keydown", (event) => {
      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
        openTaskBoardEntryContextMenu(entry, event);
      }
    });
    return entry;
  }

  async function openStandaloneTaskBoard() {
    let result;
    try {
      const bridge = window.__codexSessionDeleteBridge;
      result = typeof bridge === "function"
        ? await bridge("/task-board/open-window", {})
        : taskBoardNativeFailure("bridge_unavailable", "任务看板启动接口不可用");
    } catch (error) {
      result = taskBoardNativeFailure(
        "bridge_unavailable",
        String(error?.message || error || "任务看板启动接口不可用"),
      );
    }
    if (result?.status === "ok") return result;
    showToast(result?.message || "独立任务看板暂不可用，请稍后重试");
    return result;
  }

  function reconcileTaskBoardEntry() {
    const entries = taskBoardEntryButtons();
    if (!taskBoardFeatureEnabled()) {
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
      entries.forEach((entry) => entry.remove());
      taskBoardState.entry = null;
      return null;
    }
    const primaryEntries = entries.filter((entry) => {
      if (!taskBoardControlInSettingsNavigation(entry)) return true;
      entry.remove();
      return false;
    });
    const pluginButton = taskBoardPluginEntryButton();
    if (!pluginButton) {
      const entry = primaryEntries.shift() || null;
      primaryEntries.forEach((duplicate) => duplicate.remove());
      taskBoardState.entry = entry;
      return entry;
    }
    const templateSignature = taskBoardEntryTemplateSignature(pluginButton);
    let entry = primaryEntries.shift() || null;
    primaryEntries.forEach((duplicate) => duplicate.remove());
    if (!entry || taskBoardState.entryTemplateSignature !== templateSignature) {
      closeTaskBoardEntryContextMenu({ restoreFocus: false });
      entry?.remove?.();
      entry = taskBoardCreateEntry(pluginButton);
    }
    if (entry.previousElementSibling !== pluginButton) {
      pluginButton.insertAdjacentElement("afterend", entry);
    }
    taskBoardState.entryTemplateSignature = templateSignature;
    taskBoardState.entry = entry;
    return entry;
  }

  function taskBoardApplyEntryActiveState(entry, active) {
    if (!entry) return;
    entry.classList?.toggle?.("bg-primary-ghost-hover", !!active);
    if (active) {
      entry.setAttribute("aria-current", "page");
      entry.setAttribute("aria-selected", "true");
      entry.setAttribute("data-active", "true");
      entry.setAttribute("data-selected", "true");
      entry.setAttribute("data-state", "active");
    } else {
      entry.removeAttribute("aria-current");
      entry.removeAttribute("aria-selected");
      entry.removeAttribute("data-active");
      entry.removeAttribute("data-selected");
      entry.removeAttribute("data-state");
    }
  }

  function taskBoardSetEntryActive(active) {
    const entry = taskBoardState.entry?.isConnected
      ? taskBoardState.entry
      : reconcileTaskBoardEntry();
    taskBoardState.entry = entry;
    if (!entry) return;
    taskBoardApplyEntryActiveState(entry, active);
  }

  function taskBoardNodeContainsPluginControl(node) {
    if (!(node instanceof Element)) return false;
    const controls = [];
    if (node.matches?.(pluginNavigationControlSelector)) controls.push(node);
    node.querySelectorAll?.(pluginNavigationControlSelector).forEach((control) => controls.push(control));
    return controls.some(pluginEntryControlMatches);
  }

  function taskBoardNavigationMutationRelevant(mutations) {
    return Array.from(mutations || []).some((mutation) => {
      return [
        ...Array.from(mutation.addedNodes || []),
        ...Array.from(mutation.removedNodes || []),
      ].some(taskBoardNodeContainsPluginControl);
    });
  }

  function taskBoardProjectNavigationMutationRelevant(mutations) {
    return Array.from(mutations || []).some((mutation) => {
      return [
        ...Array.from(mutation.addedNodes || []),
        ...Array.from(mutation.removedNodes || []),
      ].some((node) => {
        if (!(node instanceof Element)) return false;
        return node.matches?.("[data-app-action-sidebar-project-row]") ||
          !!node.querySelector?.("[data-app-action-sidebar-project-row]");
      });
    });
  }

  function installTaskBoardNavigationObserver() {
    const root = document.body || document.documentElement;
    if (!root || typeof MutationObserver !== "function") return;
    if (taskBoardState.navigationObserver && taskBoardState.navigationObserverRoot === root) return;
    taskBoardState.navigationObserver?.disconnect?.();
    taskBoardState.navigationObserver = new MutationObserver((mutations) => {
      if (!taskBoardFeatureEnabled()) return;
      if (taskBoardProjectNavigationMutationRelevant(mutations)) {
        taskBoardState.navigationVersion += 1;
        taskBoardState.projectOptionsCache = null;
      }
      if (!taskBoardNavigationMutationRelevant(mutations)) return;
      const entry = reconcileTaskBoardEntry();
      if (!entry) return;
      taskBoardApplyEntryActiveState(entry, taskBoardState.active);
      if (!taskBoardState.navigationHandler) installTaskBoardEntry();
    });
    taskBoardState.navigationObserver.observe(root, { childList: true, subtree: true });
    taskBoardState.navigationObserverRoot = root;
  }

  function taskBoardNativeNavigationControl(target) {
    if (!(target instanceof Element)) return null;
    return target.closest?.("[data-app-action-sidebar-thread-row], [data-app-action-sidebar-project-row]") ||
      target.closest?.(taskBoardNativeNavigationSelector) ||
      null;
  }

  function taskBoardNativeNavigationControls() {
    return Array.from(document.querySelectorAll(taskBoardNativeNavigationSelector))
      .filter((control) => control instanceof Element && !control.matches?.(taskBoardEntrySelector));
  }

  function taskBoardNativeSelectionKey(control) {
    const stableId = control.getAttribute("data-app-action-sidebar-project-id") ||
      control.getAttribute("data-app-action-sidebar-thread-id") ||
      control.getAttribute("href") ||
      control.getAttribute("aria-label") ||
      String(control.textContent || "").replace(/\s+/g, " ").trim();
    return stableId ? `${control.tagName || "control"}:${stableId}` : "";
  }

  function taskBoardNativeControlSelected(control) {
    return taskBoardNativeSelectionAttributes.some((name) => {
      const value = control.getAttribute(name);
      return value === "page" || value === "true" || value === "active";
    });
  }

  function taskBoardSuppressNativeSelection(control) {
    control.setAttribute(taskBoardNativeSelectionAttribute, "true");
    control.removeAttribute("aria-current");
    control.removeAttribute("aria-selected");
    control.removeAttribute("data-selected");
    control.removeAttribute("data-active");
    if (control.hasAttribute("data-app-action-sidebar-thread-selected")) {
      control.setAttribute("data-app-action-sidebar-thread-selected", "false");
    }
    if (control.hasAttribute("data-app-action-sidebar-thread-active")) {
      control.setAttribute("data-app-action-sidebar-thread-active", "false");
    }
    if (control.getAttribute("data-state") === "active") control.setAttribute("data-state", "inactive");
    control.classList?.remove?.("bg-primary-ghost-hover");
  }

  function taskBoardRestoreNativeSelectionSnapshot(control, snapshot) {
    if (!control || !snapshot) return;
    snapshot.attributes.forEach(([name, value]) => {
      if (value == null) control.removeAttribute(name);
      else control.setAttribute(name, value);
    });
    snapshot.activeClasses.forEach((className) => control.classList?.add?.(className));
  }

  function clearTaskBoardNativeSelection() {
    const state = taskBoardState.nativeSelection instanceof Map
      ? taskBoardState.nativeSelection
      : new Map();
    taskBoardNativeNavigationControls().forEach((control) => {
      if (!taskBoardNativeControlSelected(control)) return;
      const key = taskBoardNativeSelectionKey(control);
      if (!key) return;
      if (!state.has(key)) {
        state.set(key, {
          attributes: taskBoardNativeSelectionAttributes.map((name) => [name, control.getAttribute(name)]),
          activeClasses: ["bg-primary-ghost-hover"].filter((className) => control.classList?.contains?.(className)),
        });
      }
      taskBoardSuppressNativeSelection(control);
    });
    taskBoardState.nativeSelection = state;
  }

  function restoreTaskBoardNativeSelection(nativeNavigationTarget = null) {
    const state = taskBoardState.nativeSelection;
    if (!(state instanceof Map)) return;
    const targetKey = nativeNavigationTarget
      ? taskBoardNativeSelectionKey(nativeNavigationTarget)
      : "";
    taskBoardNativeNavigationControls().forEach((control) => {
      control.removeAttribute(taskBoardNativeSelectionAttribute);
      const key = taskBoardNativeSelectionKey(control);
      if (targetKey && key !== targetKey) return;
      taskBoardRestoreNativeSelectionSnapshot(control, state.get(key));
    });
    taskBoardState.nativeSelection = null;
  }

  function taskBoardDisconnectHostResources() {
    taskBoardState.hostObserver?.disconnect?.();
    taskBoardState.resizeObserver?.disconnect?.();
    taskBoardState.hostObserver = null;
    taskBoardState.resizeObserver = null;
  }

  function taskBoardUnmountHost() {
    taskBoardDisconnectHostResources();
    taskBoardState.root?.remove?.();
    taskBoardState.root = null;
    taskBoardState.host?.classList?.remove(taskBoardMainHostClass);
    taskBoardState.host = null;
  }

  function taskBoardSyncDensity() {
    const root = taskBoardState.root;
    const width = taskBoardState.host?.clientWidth || 0;
    const height = taskBoardState.host?.clientHeight || 0;
    if (!root) return;
    const compact = width > 0 && width < 860 ? "true" : "false";
    const lowHeight = height > 0 && height < 520 ? "true" : "false";
    const toolbarLayout = taskBoardToolbarLayout(width, height).mode;
    if (root.dataset.compact !== compact) root.dataset.compact = compact;
    if (root.dataset.lowHeight !== lowHeight) root.dataset.lowHeight = lowHeight;
    if (root.dataset.toolbarLayout !== toolbarLayout) {
      root.dataset.toolbarLayout = toolbarLayout;
    }
  }

  function cancelScheduledTaskBoardCardsRender() {
    if (!taskBoardState.cardsRenderFrame) return;
    cancelAnimationFrame(taskBoardState.cardsRenderFrame);
    taskBoardState.cardsRenderFrame = 0;
  }

  function scheduleTaskBoardCardsRender() {
    if (taskBoardState.cardsRenderFrame || !taskBoardState.active) return;
    taskBoardState.cardsRenderFrame = requestAnimationFrame(() => {
      taskBoardState.cardsRenderFrame = 0;
      if (taskBoardState.active) renderTaskBoardCards();
    });
  }

  function taskBoardToolbarLayout(width, height = 0) {
    const normalizedWidth = Number(width) || 0;
    return {
      mode: normalizedWidth > 0 && normalizedWidth <= 540
        ? "collapsed"
        : normalizedWidth > 0 && normalizedWidth < 860
          ? "wrapped"
          : "inline",
      controls: ["search", "filter", "create"],
      createMinHeight: 36,
    };
  }

  function taskBoardObserveHost(host) {
    taskBoardDisconnectHostResources();
    if (typeof ResizeObserver === "function") {
      taskBoardState.resizeObserver = new ResizeObserver(() => taskBoardSyncDensity());
      taskBoardState.resizeObserver.observe(host);
    }
    if (typeof MutationObserver === "function") {
      taskBoardState.hostObserver = new MutationObserver(() => {
        if (!taskBoardState.active) return;
        if (taskBoardState.root?.parentElement === host) return;
        requestAnimationFrame(() => {
          if (taskBoardState.active) mountTaskBoardRoot();
        });
      });
      taskBoardState.hostObserver.observe(host, { childList: true });
    }
  }

  function taskBoardEnsureScaffold(root) {
    if (root.dataset.taskBoardScaffold === "true") return;
    root.dataset.taskBoardScaffold = "true";
    const page = taskBoardElement("section", "codex-task-board-page");
    page.setAttribute("aria-label", "任务看板");
    const heading = taskBoardElement("div", "codex-task-board-heading");
    const title = taskBoardElement("h1", "", "任务看板");
    heading.appendChild(title);
    const description = taskBoardElement(
      "p",
      "codex-task-board-description",
      "跨项目观察任务状态，并集中关联项目下的多个会话",
    );
    const toolbar = taskBoardElement("div", "codex-task-board-toolbar");
    const searchControl = taskBoardElement("div", "codex-task-board-search-control");
    searchControl.innerHTML = `<svg aria-hidden="true" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="11" cy="11" r="6.5"></circle><path d="m16 16 4 4"></path></svg>`;
    const search = taskBoardElement("input", "codex-task-board-search");
    search.type = "search";
    search.placeholder = "搜索任务、项目或关联会话";
    search.setAttribute("aria-label", "搜索任务、项目或关联会话");
    search.addEventListener("input", () => {
      taskBoardState.query = search.value;
      scheduleTaskBoardCardsRender();
    });
    searchControl.appendChild(search);
    const filter = taskBoardElement("button", "codex-task-board-project-filter");
    filter.type = "button";
    taskBoardConfigureDropdownTrigger(filter, "全部项目", "筛选项目");
    filter.addEventListener("click", () => openTaskBoardProjectMenu(filter));
    const create = taskBoardElement("button", "codex-task-board-create", "新建任务");
    create.type = "button";
    create.title = "新建任务";
    create.setAttribute("aria-label", "新建任务");
    create.innerHTML = `<svg aria-hidden="true" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"></path></svg><span>新建任务</span>`;
    create.addEventListener("click", () => openTaskBoardCreateModal());
    const hint = taskBoardElement("span", "codex-task-board-hint", "拖动任务卡片可切换状态");
    hint.dataset.status = "ok";
    hint.setAttribute("aria-live", "polite");
    hint.setAttribute("aria-atomic", "true");
    toolbar.append(searchControl, filter, create, hint);
    const scroll = taskBoardElement("div", "codex-task-board-scroll");
    scroll.tabIndex = 0;
    scroll.setAttribute("aria-label", "任务看板列，可横向和纵向滚动");
    const columns = taskBoardElement("div", "codex-task-board-columns");
    scroll.appendChild(columns);
    page.append(heading, description, toolbar, scroll);
    root.appendChild(page);
  }

  function mountTaskBoardRoot() {
    const host = taskBoardMainHost();
    if (!host) return false;
    if (taskBoardState.host && taskBoardState.host !== host) {
      cancelTaskBoardMoveInteraction({ restoreFocus: false });
      closeTaskBoardCreateModal();
      taskBoardUnmountHost();
    }
    taskBoardState.host = host;
    let root = Array.from(host.children).find((node) => node.matches?.(taskBoardRootSelector)) || null;
    if (!root) {
      root = document.createElement("div");
      root.setAttribute(taskBoardRootAttribute, "true");
      host.appendChild(root);
    }
    taskBoardState.root = root;
    host.classList.add(taskBoardMainHostClass);
    taskBoardEnsureScaffold(root);
    taskBoardObserveHost(host);
    taskBoardSyncDensity();
    renderTaskBoard();
    return true;
  }

  function taskBoardProjectOptions() {
    const cached = taskBoardState.projectOptionsCache;
    if (
      cached?.snapshot === taskBoardState.snapshot &&
      cached?.catalog === taskBoardState.catalog &&
      cached?.navigationVersion === taskBoardState.navigationVersion
    ) {
      return cached.options;
    }
    const projects = new Map();
    const concreteLabels = new Set();
    const opaqueProjectId = (cwd) => {
      return /^local-[0-9a-f]+$/i.test(cwd) ||
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(cwd);
    };
    const add = (project) => {
      const cwd = taskBoardNormalizedCwd(project?.cwd || project?.path || "");
      if (!cwd || projects.has(cwd)) return;
      const label = String(project?.label || displayProjectName(project?.cwd || project?.path || cwd));
      const labelKey = label.trim().toLocaleLowerCase("zh-Hans-CN");
      if (opaqueProjectId(cwd) && labelKey && concreteLabels.has(labelKey)) return;
      projects.set(cwd, {
        cwd,
        label,
      });
      if (!opaqueProjectId(cwd) && labelKey) concreteLabels.add(labelKey);
    };
    taskBoardState.catalog.projects.forEach(add);
    nativeProjectTargets().forEach((target) => add({ cwd: target.path, label: target.label }));
    taskBoardState.snapshot.tasks.forEach((task) => add(task?.project));
    const options = Array.from(projects.values())
      .sort((left, right) => left.label.localeCompare(right.label, "zh-Hans-CN"));
    taskBoardState.projectOptionsCache = {
      snapshot: taskBoardState.snapshot,
      catalog: taskBoardState.catalog,
      navigationVersion: taskBoardState.navigationVersion,
      options,
    };
    return options;
  }

  function taskBoardVisibleTasks() {
    const query = taskBoardState.query.trim().toLocaleLowerCase();
    const projectCwd = taskBoardState.projectCwd;
    return taskBoardState.snapshot.tasks.filter((task) => {
      const taskCwd = taskBoardNormalizedCwd(task?.project?.cwd);
      if (projectCwd && taskCwd !== projectCwd) return false;
      return !query || taskBoardTaskSearchText(task).includes(query);
    });
  }

  function taskBoardCatalogSessionMapFor(catalog) {
    if (catalog && typeof catalog === "object") {
      const cached = taskBoardCatalogSessionMapCache.get(catalog);
      if (cached?.sessions === catalog.sessions) return cached.map;
    }
    const sessions = Array.isArray(catalog?.sessions) ? catalog.sessions : [];
    const result = new Map();
    sessions.forEach((session) => {
      const sessionId = String(session?.sessionId || "").trim();
      if (!sessionId) return;
      const current = result.get(sessionId);
      const currentUpdatedAt = Number(current?.updatedAtMs || -1);
      const nextUpdatedAt = Number(session?.updatedAtMs || -1);
      if (!current || nextUpdatedAt >= currentUpdatedAt) result.set(sessionId, session);
    });
    if (catalog && typeof catalog === "object") {
      taskBoardCatalogSessionMapCache.set(catalog, {
        sessions: catalog.sessions,
        map: result,
      });
    }
    return result;
  }

  function taskBoardCatalogPartiallyUnavailable(catalog = taskBoardState.catalog) {
    return Array.isArray(catalog?.warnings) && catalog.warnings.length > 0;
  }

  function taskBoardConversationProjectionForCatalog(conversation, catalog, catalogError = "") {
    const sessionId = String(conversation?.sessionId || "").trim();
    if (!sessionId) {
      return {
        available: false,
        label: "会话不可用",
        status: taskBoardConversationStatus({ available: false }),
      };
    }
    const catalogSession = taskBoardCatalogSessionMapFor(catalog).get(sessionId);
    const fallbackTitle = String(conversation?.title || "未命名会话");
    const runtimeStatus = taskBoardState.conversationStatuses.get(
      taskBoardConversationStatusKey(sessionId),
    );
    const status = taskBoardConversationStatus({
      available: !!catalogSession || !!catalogError || taskBoardCatalogPartiallyUnavailable(catalog),
      usageKnown: runtimeStatus?.known === true,
      checking: runtimeStatus?.checking === true || !runtimeStatus,
      isRunning: runtimeStatus?.isRunning === true,
      unread: typeof runtimeStatus?.unread === "boolean"
        ? runtimeStatus.unread
        : taskBoardNativeThreadUnread(sessionId),
    });
    if (catalogSession) {
      return {
        available: true,
        label: "打开会话",
        title: String(catalogSession.title || fallbackTitle),
        status,
      };
    }
    if (catalogError) {
      return {
        available: true,
        label: "目录暂不可用",
        title: fallbackTitle,
        status,
      };
    }
    if (taskBoardCatalogPartiallyUnavailable(catalog)) {
      return {
        available: true,
        label: "目录部分不可用",
        title: fallbackTitle,
        status,
      };
    }
    return {
      available: false,
      label: "会话不可用",
      title: fallbackTitle,
      status: taskBoardConversationStatus({ available: false }),
    };
  }

  function taskBoardConversationStatus({
    available = true,
    usageKnown = false,
    checking = false,
    isRunning = false,
    unread = false,
  } = {}) {
    if (!available) return { id: "unavailable", label: "不可用" };
    if (isRunning) return { id: "running", label: "运行中" };
    if (!usageKnown) {
      return checking
        ? { id: "checking", label: "检查中" }
        : { id: "unknown", label: "状态未知" };
    }
    if (unread) return { id: "completed-unread", label: "已完成 · 未读" };
    return { id: "completed", label: "已完成" };
  }

  function taskBoardConversationProjection(conversation) {
    return taskBoardConversationProjectionForCatalog(
      conversation,
      taskBoardState.catalog,
      taskBoardState.catalogError,
    );
  }

  function taskBoardLinkedConversations() {
    const snapshot = taskBoardState.snapshot;
    if (snapshot && typeof snapshot === "object") {
      const cached = taskBoardLinkedConversationsCache.get(snapshot);
      if (cached) return cached;
    }
    const conversations = new Map();
    (snapshot?.tasks || []).forEach((task) => {
      (Array.isArray(task?.conversations) ? task.conversations : []).forEach((conversation) => {
        const key = taskBoardConversationStatusKey(conversation?.sessionId);
        if (key && !conversations.has(key)) conversations.set(key, conversation);
      });
    });
    if (snapshot && typeof snapshot === "object") {
      taskBoardLinkedConversationsCache.set(snapshot, conversations);
    }
    return conversations;
  }

  function taskBoardConversationRuntimeStatusChanged(current, next) {
    return !current ||
      current.known !== next.known ||
      current.checking !== next.checking ||
      current.isRunning !== next.isRunning ||
      current.unread !== next.unread;
  }

  function taskBoardSetConversationRuntimeStatus(key, next) {
    const current = taskBoardState.conversationStatuses.get(key);
    const changed = taskBoardConversationRuntimeStatusChanged(current, next);
    taskBoardState.conversationStatuses.set(key, next);
    return changed;
  }

  function stopTaskBoardConversationStatusRefresh() {
    if (taskBoardState.conversationStatusTimer !== null) {
      clearTimeout(taskBoardState.conversationStatusTimer);
      taskBoardState.conversationStatusTimer = null;
    }
    taskBoardState.conversationStatusRequestId += 1;
    taskBoardState.conversationStatusRefreshPromise = null;
  }

  function scheduleTaskBoardConversationStatusRefresh(delayMs) {
    if (!taskBoardState.active) return;
    if (taskBoardState.conversationStatusTimer !== null) {
      clearTimeout(taskBoardState.conversationStatusTimer);
    }
    taskBoardState.conversationStatusTimer = setTimeout(() => {
      taskBoardState.conversationStatusTimer = null;
      void refreshTaskBoardConversationStatuses();
    }, Math.max(0, Number(delayMs || 0)));
  }

  async function taskBoardMapSettledWithConcurrency(values, maxConcurrency, mapper) {
    const entries = Array.isArray(values) ? values : [];
    const outcomes = new Array(entries.length);
    let nextIndex = 0;
    const worker = async () => {
      while (nextIndex < entries.length) {
        const index = nextIndex;
        nextIndex += 1;
        try {
          outcomes[index] = {
            status: "fulfilled",
            value: await mapper(entries[index], index),
          };
        } catch (reason) {
          outcomes[index] = { status: "rejected", reason };
        }
      }
    };
    const workerCount = Math.min(
      entries.length,
      Math.max(1, Math.trunc(Number(maxConcurrency) || 1)),
    );
    await Promise.all(Array.from({ length: workerCount }, () => worker()));
    return outcomes;
  }

  async function refreshTaskBoardConversationStatuses({ schedule = true } = {}) {
    if (!taskBoardState.active) return [];
    if (taskBoardState.conversationStatusRefreshPromise) {
      return taskBoardState.conversationStatusRefreshPromise;
    }
    if (taskBoardState.conversationStatusTimer !== null) {
      clearTimeout(taskBoardState.conversationStatusTimer);
      taskBoardState.conversationStatusTimer = null;
    }
    const linked = taskBoardLinkedConversations();
    const linkedEntries = Array.from(linked.entries());
    const activeKeys = new Set(linked.keys());
    let statusesChanged = false;
    Array.from(taskBoardState.conversationStatuses.keys()).forEach((key) => {
      if (activeKeys.has(key)) return;
      taskBoardState.conversationStatuses.delete(key);
      statusesChanged = true;
    });
    if (!linked.size) {
      if (statusesChanged) renderTaskBoardCards();
      if (schedule) {
        scheduleTaskBoardConversationStatusRefresh(
          taskBoardConversationStatusIdleRefreshIntervalMs,
        );
      }
      return [];
    }

    const now = taskBoardNativeNow();
    const refreshEntries = linkedEntries.filter(([key]) => {
      const current = taskBoardState.conversationStatuses.get(key);
      const checkedAtMs = Number(current?.checkedAtMs || 0);
      return !current ||
        current.isRunning === true ||
        checkedAtMs <= 0 ||
        now < checkedAtMs ||
        now - checkedAtMs >= taskBoardConversationStatusIdleRefreshIntervalMs;
    });
    if (!refreshEntries.length) {
      if (statusesChanged) renderTaskBoardCards();
      if (schedule) {
        const anyRunning = Array.from(taskBoardState.conversationStatuses.values())
          .some((status) => status?.isRunning === true);
        scheduleTaskBoardConversationStatusRefresh(
          anyRunning
            ? taskBoardConversationStatusRefreshIntervalMs
            : taskBoardConversationStatusIdleRefreshIntervalMs,
        );
      }
      return [];
    }

    const unreadBySession = taskBoardNativeThreadUnreadIndex();
    refreshEntries.forEach(([key, conversation]) => {
      const current = taskBoardState.conversationStatuses.get(key);
      const next = current
        ? {
          ...current,
          unread: taskBoardNativeThreadUnread(conversation?.sessionId, unreadBySession),
        }
        : {
          known: false,
          checking: true,
          isRunning: false,
          unread: taskBoardNativeThreadUnread(conversation?.sessionId, unreadBySession),
          checkedAtMs: 0,
        };
      statusesChanged = taskBoardSetConversationRuntimeStatus(key, next) || statusesChanged;
    });
    if (statusesChanged) renderTaskBoardCards();
    const requestId = ++taskBoardState.conversationStatusRequestId;
    const refreshPromise = taskBoardMapSettledWithConcurrency(
      refreshEntries,
      taskBoardConversationStatusMaxConcurrency,
      async ([key, conversation]) => {
        const result = await taskBoardMockOrBridgeResult("conversationStatus", {
          session_id: String(conversation?.sessionId || "").trim(),
          title: String(conversation?.title || "").trim(),
        });
        const summary = result?.status === "ok" && result?.summary && typeof result.summary === "object"
          ? result.summary
          : null;
        const current = taskBoardState.conversationStatuses.get(key);
        return {
          key,
          known: !!summary,
          checking: false,
          isRunning: summary?.isRunning === true || summary?.lastTurnRunning === true,
          unread: current?.unread === true,
          checkedAtMs: taskBoardNativeNow(),
        };
      },
    ).then((outcomes) => {
      if (!taskBoardState.active || requestId !== taskBoardState.conversationStatusRequestId) {
        return outcomes;
      }
      let resultsChanged = false;
      outcomes.forEach((outcome, index) => {
        const key = refreshEntries[index]?.[0];
        if (!key) return;
        if (outcome.status === "fulfilled") {
          resultsChanged =
            taskBoardSetConversationRuntimeStatus(key, outcome.value) || resultsChanged;
        } else {
          const current = taskBoardState.conversationStatuses.get(key);
          resultsChanged = taskBoardSetConversationRuntimeStatus(key, {
            known: false,
            checking: false,
            isRunning: false,
            unread: current?.unread === true,
            checkedAtMs: taskBoardNativeNow(),
          }) || resultsChanged;
        }
      });
      if (resultsChanged) renderTaskBoardCards();
      if (schedule) {
        const anyRunning = Array.from(taskBoardState.conversationStatuses.values())
          .some((status) => status?.isRunning === true);
        scheduleTaskBoardConversationStatusRefresh(
          anyRunning
            ? taskBoardConversationStatusRefreshIntervalMs
            : taskBoardConversationStatusIdleRefreshIntervalMs,
        );
      }
      return outcomes;
    }).finally(() => {
      if (taskBoardState.conversationStatusRefreshPromise === refreshPromise) {
        taskBoardState.conversationStatusRefreshPromise = null;
      }
    });
    taskBoardState.conversationStatusRefreshPromise = refreshPromise;
    return refreshPromise;
  }

  function taskBoardTaskSearchText(task, catalog = taskBoardState.catalog) {
    if (task && typeof task === "object") {
      const cached = taskBoardTaskSearchTextCache.get(task);
      if (cached?.catalog === catalog) return cached.text;
    }
    const catalogSessions = taskBoardCatalogSessionMapFor(catalog);
    const text = [
      task?.title,
      task?.project?.label,
      task?.project?.cwd,
      ...(Array.isArray(task?.conversations)
        ? task.conversations.flatMap((conversation) => {
          const sessionId = String(conversation?.sessionId || "").trim();
          const catalogSession = catalogSessions.get(sessionId);
          return [
            String(catalogSession?.title || conversation?.title || "未命名会话"),
            sessionId,
          ];
        })
        : []),
    ].join("\n").toLocaleLowerCase();
    if (task && typeof task === "object") {
      taskBoardTaskSearchTextCache.set(task, { catalog, text });
    }
    return text;
  }

  function taskBoardTaskMatchesQuery(task, query, catalog = taskBoardState.catalog) {
    const normalizedQuery = String(query || "").trim().toLocaleLowerCase();
    return !normalizedQuery || taskBoardTaskSearchText(task, catalog).includes(normalizedQuery);
  }

  async function openTaskBoardConversation(conversation) {
    const result = await taskBoardNativeAdapter.openSession(String(conversation?.sessionId || ""), conversation);
    if (result?.status === "ok") return;
    showToast(result?.message || "无法打开关联会话");
  }

  function taskBoardCreateModalKeydown(event) {
    const modal = taskBoardState.createModal;
    if (!modal) return;
    const dropdown = taskBoardState.dropdownMenu;
    if (dropdown?.kind?.startsWith?.("create-")) {
      if (event.key === "Escape") {
        event.preventDefault?.();
        event.stopImmediatePropagation?.();
        if (dropdown.kind === "create-settings" && dropdown.submenuElement) {
          taskBoardCloseCreateSettingsSubmenu(dropdown, { restoreFocus: true });
        } else {
          closeTaskBoardDropdownMenu();
        }
        return;
      }
      if (event.key === "Tab") {
        const focusable = taskBoardCreateModalFocusableElements(modal);
        const triggerIndex = focusable.indexOf(dropdown.trigger);
        const nextIndex = event.shiftKey
          ? (triggerIndex <= 0 ? focusable.length - 1 : triggerIndex - 1)
          : (triggerIndex === focusable.length - 1 ? 0 : triggerIndex + 1);
        event.preventDefault?.();
        event.stopImmediatePropagation?.();
        closeTaskBoardDropdownMenu({ restoreFocus: false });
        focusable[nextIndex]?.focus?.();
        return;
      }
    }
    if (event.key === "Escape") {
      event.preventDefault?.();
      if (modal.busy) return;
      closeTaskBoardCreateModal();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = taskBoardCreateModalFocusableElements(modal);
    if (!focusable.length) {
      event.preventDefault?.();
      modal.dialog.focus?.();
      return;
    }
    const currentIndex = focusable.indexOf(document.activeElement);
    const nextIndex = event.shiftKey
      ? (currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1)
      : (currentIndex === focusable.length - 1 ? 0 : currentIndex + 1);
    event.preventDefault?.();
    focusable[nextIndex].focus?.();
  }

  function taskBoardCreateModalFocusableElements(modal) {
    const sessionInputs = Array.from(modal?.sessionField?.querySelectorAll?.("input") || []);
    return [
      modal?.closeButton,
      modal?.existingButton,
      modal?.newButton,
      modal?.titleInput,
      modal?.projectSelect,
      modal?.statusSelect,
      ...(modal?.mode === "new" ? [modal?.firstInstructionInput, modal?.modelTrigger] : []),
      ...sessionInputs,
      modal?.cancelButton,
      modal?.submitButton,
    ].filter((element) => element && !element.disabled);
  }

  function closeTaskBoardCreateModal({ restoreFocus = true } = {}) {
    const modal = taskBoardState.createModal;
    taskBoardState.createModalRequestId += 1;
    if (!modal) return;
    taskBoardCancelCreateSessionHydration(modal);
    if (taskBoardState.dropdownMenu?.kind?.startsWith?.("create-")) {
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    }
    document.removeEventListener("keydown", taskBoardState.createModalKeydownHandler, true);
    taskBoardState.createModalKeydownHandler = null;
    taskBoardState.createModal = null;
    modal.busy = false;
    modal.backdrop.remove?.();
    const previousFocus = taskBoardState.createModalPreviousFocus;
    taskBoardState.createModalPreviousFocus = null;
    if (restoreFocus && previousFocus?.isConnected !== false) previousFocus?.focus?.();
  }

  function taskBoardCreateModalProjects() {
    return taskBoardProjectOptions();
  }

  function taskBoardCreateModelDisplayLabel(entry) {
    const displayName = String(entry?.displayName || "").trim();
    if (displayName) return displayName;
    const slug = String(entry?.slug || "").trim();
    const codexModel = slug.match(/^gpt-(\d+(?:\.\d+)?)-(sol|terra|luna)(.*)$/i);
    if (!codexModel) return slug;
    const variant = codexModel[2].slice(0, 1).toUpperCase() + codexModel[2].slice(1).toLowerCase();
    return `${codexModel[1]} ${variant}${codexModel[3] || ""}`.trim();
  }

  function taskBoardCreateModelOptions() {
    return [
      {
        value: "",
        label: "使用当前默认模型",
        description: "沿用 Codex 当前会话的默认模型",
      },
      ...codexServiceTierCatalogEntries().map((entry) => ({
        value: entry.slug,
        label: taskBoardCreateModelDisplayLabel(entry),
        description: entry.displayName ? entry.slug : "",
      })),
    ];
  }

  function taskBoardReasoningEffortLabel(effortId) {
    const id = String(effortId || "").trim().toLowerCase();
    return taskBoardReasoningEffortDefinitions.find((effort) => effort.id === id)?.label || id;
  }

  function taskBoardCreateSelectedModelCatalogEntry(modelId) {
    const normalized = normalizeCodexServiceTierModelName(modelId);
    if (!normalized) return null;
    return codexServiceTierCatalogEntries().find(
      (entry) => normalizeCodexServiceTierModelName(entry.slug) === normalized,
    ) || null;
  }

  function taskBoardCreateEffortOptions(modelId) {
    const entry = taskBoardCreateSelectedModelCatalogEntry(modelId);
    const supportedIds = uniqueValues(
      (entry?.supportedReasoningEfforts || [])
        .map((effort) => String(effort || "").trim().toLowerCase())
        .filter(Boolean),
    );
    const preferredIds = supportedIds.filter(
      (effort) => taskBoardDefaultReasoningEffortIds.includes(effort),
    );
    const optionIds = preferredIds.length
      ? preferredIds
      : supportedIds.length
        ? supportedIds
        : taskBoardDefaultReasoningEffortIds;
    return optionIds.map((id) => ({
      value: id,
      label: taskBoardReasoningEffortLabel(id),
    }));
  }

  function taskBoardCurrentComposerReasoningEffort() {
    const trigger = taskBoardNativeModelTrigger();
    return String(
      trigger?.getAttribute?.("data-selected-reasoning-effort") ||
      "",
    ).trim().toLowerCase();
  }

  function taskBoardCreateDefaultEffortId(modelId) {
    const options = taskBoardCreateEffortOptions(modelId);
    const optionIds = new Set(options.map((option) => option.value));
    const currentModel = codexServiceTierComposerSelectedModel();
    const currentEffort = taskBoardCurrentComposerReasoningEffort();
    const sameModel = !modelId ||
      !currentModel ||
      normalizeCodexServiceTierModelName(currentModel) ===
        normalizeCodexServiceTierModelName(modelId);
    if (sameModel && optionIds.has(currentEffort)) return currentEffort;
    const catalogDefault = String(
      taskBoardCreateSelectedModelCatalogEntry(modelId)?.defaultReasoningEffort ||
      "",
    ).trim().toLowerCase();
    if (optionIds.has(catalogDefault)) return catalogDefault;
    if (optionIds.has("medium")) return "medium";
    return options[0]?.value || "";
  }

  function taskBoardReconcileCreateEffort(modal) {
    if (!modal) return "";
    const options = taskBoardCreateEffortOptions(modal.modelId);
    if (!options.some((option) => option.value === modal.effortId)) {
      modal.effortId = taskBoardCreateDefaultEffortId(modal.modelId);
    }
    return modal.effortId;
  }

  function taskBoardCreateDefaultModelId() {
    const modelName = codexServiceTierCurrentModelName();
    if (!modelName) return "";
    const match = codexServiceTierCatalogModelMatch(modelName, true);
    const modelId = String(match.slug || modelName || "").trim();
    return taskBoardCreateModelOptions().some((option) => option.value === modelId)
      ? modelId
      : "";
  }

  async function taskBoardRefreshCreateModels(modal) {
    if (!modal || taskBoardState.createModal !== modal) return;
    modal.modelsLoading = codexServiceTierCatalogEntries().length === 0;
    renderTaskBoardCreateModal();
    let result;
    try {
      result = await loadCodexModelCatalog();
    } catch (error) {
      result = { status: "failed", message: String(error?.message || error || "") };
    }
    if (taskBoardState.createModal !== modal) return;
    modal.modelsLoading = false;
    modal.modelCatalogError = result?.status === "failed"
      ? String(result?.message || "模型目录加载失败")
      : "";
    const options = taskBoardCreateModelOptions();
    if (result?.status !== "failed" &&
      !options.some((option) => option.value === modal.modelId)) {
      modal.modelId = "";
    }
    if (!modal.modelSelectionTouched && !modal.modelId) {
      modal.modelId = taskBoardCreateDefaultModelId();
    }
    taskBoardReconcileCreateEffort(modal);
    renderTaskBoardCreateModal();
  }

  function taskBoardCreateSessionsForProject(cwd, excludedSessionIds = null) {
    const normalizedCwd = taskBoardNormalizedCwd(cwd);
    const excluded = excludedSessionIds instanceof Set
      ? new Set(Array.from(excludedSessionIds).map(taskBoardConversationStatusKey))
      : new Set();
    const catalog = taskBoardState.catalog;
    const sessions = Array.isArray(catalog?.sessions) ? catalog.sessions : [];
    let cached = catalog && typeof catalog === "object"
      ? taskBoardCatalogSessionsByProjectCache.get(catalog)
      : null;
    if (!cached || cached.sessions !== sessions) {
      const byProject = new Map();
      sessions.forEach((session) => {
        const sessionId = String(session?.sessionId || "").trim();
        const projectCwd = taskBoardNormalizedCwd(session?.cwd);
        if (!sessionId || !projectCwd) return;
        const projectSessions = byProject.get(projectCwd) || [];
        projectSessions.push(session);
        byProject.set(projectCwd, projectSessions);
      });
      byProject.forEach((projectSessions) => {
        projectSessions.sort(
          (left, right) =>
            Number(right?.updatedAtMs || 0) - Number(left?.updatedAtMs || 0),
        );
      });
      cached = { sessions, byProject };
      if (catalog && typeof catalog === "object") {
        taskBoardCatalogSessionsByProjectCache.set(catalog, cached);
      }
    }
    const projectSessions = cached.byProject.get(normalizedCwd) || [];
    if (!excluded.size) return projectSessions;
    return projectSessions.filter((session) => {
      return !excluded.has(taskBoardConversationStatusKey(session?.sessionId));
    });
  }

  function taskBoardSetCreateModalFeedback(modal, message = "") {
    if (taskBoardState.createModal !== modal) return;
    modal.feedback = String(message || "");
    const feedback = modal.feedbackNode;
    if (feedback) feedback.textContent = modal.feedback;
  }

  function taskBoardSetCreateModalBusy(modal, busy) {
    if (taskBoardState.createModal !== modal) return;
    modal.busy = !!busy;
    renderTaskBoardCreateModal();
  }

  function taskBoardSetCreateMode(mode) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy || !["existing", "new"].includes(mode)) return;
    if (mode === "new" && modal.nativeCreateAvailable === false) {
      taskBoardSetCreateModalFeedback(
        modal,
        modal.nativeCreateMessage || "当前项目暂不支持新建关联会话",
      );
      return;
    }
    if (mode !== "new" && taskBoardState.dropdownMenu?.kind === "create-settings") {
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    }
    if (modal.mode !== mode) taskBoardClearCreateIdentity(modal);
    modal.mode = mode;
    modal.feedback = "";
    renderTaskBoardCreateModal();
  }

  function taskBoardSetCreateProject(cwd) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy) return;
    if (modal.purpose === "attach") return;
    const normalizedCwd = taskBoardNormalizedCwd(cwd);
    if (modal.projectCwd !== normalizedCwd) {
      modal.selectedSessionIds.clear();
      taskBoardClearCreateIdentity(modal);
    }
    modal.projectCwd = normalizedCwd;
    modal.feedback = "";
    renderTaskBoardCreateModal();
    void taskBoardRefreshNativeCreateAvailability(modal);
  }

  function taskBoardSetCreateModel(modelId) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy) return;
    const value = String(modelId || "").trim();
    if (!taskBoardCreateModelOptions().some((option) => option.value === value)) return;
    modal.modelId = value;
    modal.modelSelectionTouched = true;
    taskBoardReconcileCreateEffort(modal);
    modal.feedback = "";
    renderTaskBoardCreateModal();
  }

  function taskBoardSetCreateEffort(effortId) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy) return;
    const value = String(effortId || "").trim().toLowerCase();
    if (!taskBoardCreateEffortOptions(modal.modelId).some((option) => option.value === value)) {
      return;
    }
    modal.effortId = value;
    modal.feedback = "";
    renderTaskBoardCreateModal();
  }

  function openTaskBoardCreateProjectMenu(trigger) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy || modal.purpose === "attach") return null;
    const projects = taskBoardCreateModalProjects();
    return openTaskBoardDropdownMenu({
      kind: "create-project",
      trigger,
      options: projects.length
        ? projects.map((project) => ({
          value: project.cwd,
          label: project.label,
          description: project.cwd,
        }))
        : [{ value: "", label: "暂无可用项目", disabled: true }],
      currentValue: modal.projectCwd,
      ariaLabel: "选择所属项目",
      fixedWidth: taskBoardProjectDropdownWidth,
      onSelect: (cwd) => taskBoardSetCreateProject(cwd),
    });
  }

  function taskBoardCreateSettingsChevron() {
    const chevron = taskBoardElement("span", "codex-task-board-create-settings-chevron");
    chevron.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4"><path d="m6 4 4 4-4 4" stroke-linecap="round" stroke-linejoin="round"></path></svg>`;
    return chevron;
  }

  function taskBoardCloseCreateSettingsSubmenu(state, { restoreFocus = false } = {}) {
    if (!state?.submenuElement) return;
    state.submenuParent?.setAttribute?.("aria-expanded", "false");
    state.submenuElement.remove?.();
    const previousParent = state.submenuParent;
    state.submenuElement = null;
    state.submenuButtons = [];
    state.submenuKind = "";
    state.submenuParent = null;
    if (restoreFocus) {
      previousParent?.focus?.({ preventScroll: true });
      requestAnimationFrame(() => {
        if (
          taskBoardState.dropdownMenu === state &&
          previousParent?.isConnected !== false
        ) {
          previousParent?.focus?.({ preventScroll: true });
        }
      });
    }
  }

  function taskBoardPositionCreateSettingsSubmenu(
    submenu,
    menu,
    parentButton,
    trigger,
  ) {
    const fallbackMenuLeft = Number.parseFloat(menu?.style?.left || "") || 8;
    const fallbackMenuTop = Number.parseFloat(menu?.style?.top || "") || 8;
    const menuRect = menu?.getBoundingClientRect?.() || {
      left: fallbackMenuLeft,
      right: fallbackMenuLeft + 220,
      top: fallbackMenuTop,
      bottom: fallbackMenuTop + 77,
    };
    const parentRect = parentButton?.getBoundingClientRect?.() || {
      left: Number(menuRect.left || 8),
      top: 8,
    };
    const submenuRect = submenu.getBoundingClientRect?.() || {
      width: 220,
      height: 0,
    };
    const viewportWidth = Number(window.innerWidth || 1024);
    const viewportHeight = Number(window.innerHeight || 768);
    const width = Number(submenuRect.width || 220);
    const height = Number(submenuRect.height || 0);
    const gap = 6;
    const triggerRect = trigger?.getBoundingClientRect?.() || {
      top: Number(menuRect.bottom || 85) + gap,
    };
    const opensAbove = Number(menuRect.bottom || 85) <= Number(triggerRect.top || 91);
    const spaceAbove = Math.max(0, Number(menuRect.top || 8) - gap - 8);
    const spaceBelow = Math.max(
      0,
      viewportHeight - Number(menuRect.bottom || 85) - gap - 8,
    );
    const placeAbove = opensAbove
      ? spaceAbove >= height || spaceAbove >= spaceBelow
      : spaceBelow < height && spaceAbove > spaceBelow;
    const preferredTop = placeAbove
      ? Number(menuRect.top || 8) - gap - height
      : Number(menuRect.bottom || 85) + gap;
    const top = Math.min(
      viewportHeight - height - 8,
      Math.max(8, preferredTop),
    );
    submenu.style.left = `${taskBoardDropdownLeft(
      Number(parentRect.left || menuRect.left || 8),
      width,
      viewportWidth,
    )}px`;
    submenu.style.top = `${Math.max(8, top)}px`;
  }

  function taskBoardOpenCreateSettingsSubmenu(state, kind, { focus = true } = {}) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy || modal.mode !== "new" || taskBoardState.dropdownMenu !== state) {
      return null;
    }
    const parentButton = state.buttons.find(
      (button) => button.getAttribute?.("data-settings-kind") === kind,
    );
    if (!parentButton) return null;
    if (state.submenuKind === kind && state.submenuElement?.isConnected !== false) {
      if (focus) {
        const selected = state.submenuButtons.find(
          (button) => button.getAttribute?.("aria-checked") === "true" && !button.disabled,
        );
        (selected || state.submenuButtons.find((button) => !button.disabled))?.focus?.({
          preventScroll: true,
        });
      }
      return state.submenuElement;
    }
    taskBoardCloseCreateSettingsSubmenu(state);
    const isModel = kind === "model";
    const options = isModel
      ? taskBoardCreateModelOptions().filter((option) => String(option?.value || "").trim())
      : taskBoardCreateEffortOptions(modal.modelId);
    const normalizedOptions = options.length
      ? options
      : [{
        value: "",
        label: isModel ? "暂无可用模型" : "暂无可用强度",
        disabled: true,
      }];
    const currentValue = isModel ? modal.modelId : modal.effortId;
    const submenu = taskBoardElement(
      "div",
      `codex-task-board-dropdown-menu codex-task-board-create-${kind}-menu`,
    );
    submenu.setAttribute("role", "menu");
    submenu.setAttribute("aria-label", isModel ? "选择新会话模型" : "选择新会话推理强度");
    const buttons = normalizedOptions.map((option) => {
      const value = String(option?.value || "");
      const selected = value === String(currentValue || "");
      const button = taskBoardElement("button");
      button.type = "button";
      button.disabled = option?.disabled === true;
      button.setAttribute("role", "menuitemradio");
      button.setAttribute("data-value", value);
      button.setAttribute("aria-checked", String(selected));
      button.setAttribute("aria-label", String(option?.label || ""));
      button.append(
        taskBoardElement(
          "span",
          "codex-task-board-dropdown-option-title",
          String(option?.label || ""),
        ),
        selected
          ? taskBoardDropdownCheck()
          : taskBoardElement("span", "codex-task-board-dropdown-option-marker"),
      );
      button.addEventListener("click", () => {
        if (button.disabled) return;
        closeTaskBoardDropdownMenu();
        if (isModel) taskBoardSetCreateModel(value);
        else taskBoardSetCreateEffort(value);
      });
      submenu.appendChild(button);
      return button;
    });
    document.body.appendChild(submenu);
    parentButton.setAttribute("aria-expanded", "true");
    state.submenuElement = submenu;
    state.submenuButtons = buttons;
    state.submenuKind = kind;
    state.submenuParent = parentButton;
    taskBoardPositionCreateSettingsSubmenu(
      submenu,
      state.element,
      parentButton,
      state.trigger,
    );
    if (focus) {
      requestAnimationFrame(() => {
        const selected = buttons.find(
          (button) => button.getAttribute("aria-checked") === "true" && !button.disabled,
        );
        (selected || buttons.find((button) => !button.disabled))?.focus?.({
          preventScroll: true,
        });
      });
    }
    return submenu;
  }

  function openTaskBoardCreateSettingsMenu(trigger) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy || modal.mode !== "new" || !trigger) return null;
    if (
      taskBoardState.dropdownMenu?.kind === "create-settings" &&
      taskBoardState.dropdownMenu?.trigger === trigger
    ) {
      closeTaskBoardDropdownMenu();
      return null;
    }
    closeTaskBoardDropdownMenu({ restoreFocus: false });
    taskBoardReconcileCreateEffort(modal);
    const modelOption = taskBoardCreateModelOptions().find(
      (option) => option.value === modal.modelId,
    );
    const effortOption = taskBoardCreateEffortOptions(modal.modelId).find(
      (option) => option.value === modal.effortId,
    );
    const menu = taskBoardElement(
      "div",
      "codex-task-board-dropdown-menu codex-task-board-create-settings-menu",
    );
    menu.setAttribute("role", "menu");
    menu.setAttribute("aria-label", "新会话模型设置");
    const definitions = [
      {
        kind: "model",
        label: "模型",
        value: modelOption?.label || "默认模型",
      },
      {
        kind: "effort",
        label: "推理强度",
        value: effortOption?.label || taskBoardReasoningEffortLabel(modal.effortId),
      },
    ];
    const buttons = definitions.map((definition) => {
      const button = taskBoardElement("button");
      button.type = "button";
      button.setAttribute("role", "menuitem");
      button.setAttribute("aria-haspopup", "menu");
      button.setAttribute("aria-expanded", "false");
      button.setAttribute("data-settings-kind", definition.kind);
      button.setAttribute("aria-label", `${definition.label} ${definition.value}`);
      button.append(
        taskBoardElement(
          "span",
          "codex-task-board-create-settings-label",
          definition.label,
        ),
        taskBoardElement(
          "span",
          "codex-task-board-create-settings-value",
          definition.value,
        ),
        taskBoardCreateSettingsChevron(),
      );
      button.addEventListener("click", () => {
        taskBoardOpenCreateSettingsSubmenu(state, definition.kind);
      });
      button.addEventListener("pointerenter", () => {
        taskBoardOpenCreateSettingsSubmenu(state, definition.kind, { focus: false });
      });
      menu.appendChild(button);
      return button;
    });
    document.body.appendChild(menu);
    trigger.setAttribute("aria-expanded", "true");
    taskBoardPositionDropdownMenu(menu, trigger, {
      minWidth: 220,
      placement: "top",
    });
    const state = {
      kind: "create-settings",
      element: menu,
      buttons,
      trigger,
      options: definitions,
      selectionAttribute: "",
      submenuElement: null,
      submenuButtons: [],
      submenuKind: "",
      submenuParent: null,
    };
    taskBoardState.dropdownMenu = state;
    taskBoardState.dropdownMenuPreviousFocus = trigger;
    taskBoardState.dropdownMenuKeydownHandler = (event) => {
      const submenuButtons = state.submenuButtons.filter((button) => !button.disabled);
      if (state.submenuElement) {
        const current = submenuButtons.indexOf(document.activeElement);
        if (event.key === "Escape" || event.key === "ArrowLeft") {
          event.preventDefault?.();
          taskBoardCloseCreateSettingsSubmenu(state, { restoreFocus: true });
          return;
        }
        if (
          event.key === "ArrowDown" ||
          event.key === "ArrowUp" ||
          event.key === "Home" ||
          event.key === "End"
        ) {
          if (!submenuButtons.length) return;
          event.preventDefault?.();
          const next = event.key === "Home" ? 0 : event.key === "End" ? submenuButtons.length - 1 :
            event.key === "ArrowDown" ? (current + 1 + submenuButtons.length) % submenuButtons.length :
              (current - 1 + submenuButtons.length) % submenuButtons.length;
          submenuButtons[next]?.focus?.({ preventScroll: true });
          return;
        }
        if (event.key === "Enter" || event.key === " ") {
          const target = submenuButtons[current >= 0 ? current : 0];
          if (!target) return;
          event.preventDefault?.();
          target.click?.();
        }
        return;
      }
      const current = buttons.indexOf(document.activeElement);
      if (event.key === "Escape") {
        event.preventDefault?.();
        closeTaskBoardDropdownMenu();
      } else if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Home" ||
        event.key === "End"
      ) {
        event.preventDefault?.();
        const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 :
          event.key === "ArrowDown" ? (current + 1 + buttons.length) % buttons.length :
            (current - 1 + buttons.length) % buttons.length;
        buttons[next]?.focus?.({ preventScroll: true });
      } else if (
        event.key === "Enter" ||
        event.key === " " ||
        event.key === "ArrowRight"
      ) {
        const target = buttons[current >= 0 ? current : 0];
        if (!target) return;
        event.preventDefault?.();
        taskBoardOpenCreateSettingsSubmenu(
          state,
          target.getAttribute("data-settings-kind"),
        );
      }
    };
    taskBoardState.dropdownMenuDismissHandler = (event) => {
      const target = event.target;
      if (
        menu.contains?.(target) ||
        state.submenuElement?.contains?.(target) ||
        trigger.contains?.(target)
      ) {
        return;
      }
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    };
    taskBoardState.dropdownMenuViewportHandler = (event) => {
      if (
        event?.target &&
        (menu.contains?.(event.target) || state.submenuElement?.contains?.(event.target))
      ) {
        return;
      }
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    };
    document.addEventListener("keydown", taskBoardState.dropdownMenuKeydownHandler, true);
    document.addEventListener("pointerdown", taskBoardState.dropdownMenuDismissHandler, true);
    window.addEventListener("resize", taskBoardState.dropdownMenuViewportHandler);
    window.addEventListener("scroll", taskBoardState.dropdownMenuViewportHandler, true);
    requestAnimationFrame(() => buttons[0]?.focus?.({ preventScroll: true }));
    return state;
  }

  function openTaskBoardCreateStatusMenu(trigger) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy || modal.purpose === "attach") return null;
    return openTaskBoardDropdownMenu({
      kind: "create-status",
      trigger,
      options: taskBoardStatusDefinitions.map((status) => ({
        value: status.id,
        label: status.label,
        color: status.color,
      })),
      currentValue: taskBoardStatusId(modal.initialStatus),
      ariaLabel: "选择初始状态",
      minWidth: 160,
      matchTriggerWidth: true,
      onSelect: (statusId) => {
        const activeModal = taskBoardState.createModal;
        if (!activeModal || activeModal.busy) return;
        activeModal.initialStatus = taskBoardStatusId(statusId);
        activeModal.feedback = "";
        renderTaskBoardCreateModal();
      },
    });
  }

  function taskBoardSetCreateSessions(sessionIds) {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy) return;
    modal.selectedSessionIds = new Set(
      (Array.isArray(sessionIds) ? sessionIds : []).map((value) => String(value || "").trim()).filter(Boolean),
    );
    taskBoardReconcileCreateSelectedSessions(modal);
    taskBoardClearCreateIdentity(modal);
    modal.feedback = "";
    renderTaskBoardCreateModal();
  }

  function taskBoardCreateModalSelectedProject(modal) {
    if (modal?.purpose === "attach" && modal.targetProject?.cwd) {
      return {
        cwd: taskBoardNormalizedCwd(modal.targetProject.cwd),
        label: String(
          modal.targetProject.label ||
          displayProjectName(modal.targetProject.cwd),
        ),
      };
    }
    return taskBoardCreateModalProjects().find((project) => project.cwd === modal.projectCwd) || null;
  }

  async function taskBoardRefreshNativeCreateAvailability(modal) {
    if (!modal || taskBoardState.createModal !== modal) return;
    const project = taskBoardCreateModalSelectedProject(modal);
    const probeId = ++modal.nativeProbeId;
    if (!project) {
      modal.nativeCreateAvailable = false;
      modal.nativeCreateCode = "native_create_unavailable";
      modal.nativeCreateMessage = "请选择项目";
      renderTaskBoardCreateModal();
      return;
    }
    modal.nativeCreateAvailable = null;
    modal.nativeCreateCode = "";
    modal.nativeCreateMessage = "";
    renderTaskBoardCreateModal();
    let probe;
    try {
      probe = await Promise.resolve(taskBoardNativeAdapter.probe(project));
    } catch {
      probe = null;
    }
    if (taskBoardState.createModal !== modal || probeId !== modal.nativeProbeId || modal.busy) return;
    modal.nativeCreateAvailable = probe?.status === "ok" && probe?.canStart === true;
    modal.nativeCreateCode = modal.nativeCreateAvailable
      ? ""
      : String(probe?.code || "native_create_unavailable").trim();
    modal.nativeCreateMessage = modal.nativeCreateAvailable
      ? ""
      : taskBoardMessageFromResult(probe, "当前项目暂不支持新建关联会话");
    if (!modal.nativeCreateAvailable && modal.mode === "new") {
      modal.mode = "existing";
      if (modal.nativeCreateCode === "native_create_busy") {
        modal.feedback = modal.nativeCreateMessage;
      }
    }
    renderTaskBoardCreateModal();
  }

  function taskBoardReconcileCreateSelectedSessions(modal = taskBoardState.createModal) {
    if (!modal) return false;
    const available = new Set(taskBoardCreateSessionsForProject(
      modal.projectCwd,
      modal.attachedSessionIds,
    )
      .map((session) => String(session?.sessionId || "").trim())
      .filter(Boolean));
    const next = new Set(Array.from(modal.selectedSessionIds).filter((sessionId) => available.has(sessionId)));
    const changed = next.size !== modal.selectedSessionIds.size ||
      Array.from(next).some((sessionId) => !modal.selectedSessionIds.has(sessionId));
    if (changed) {
      modal.selectedSessionIds = next;
      taskBoardClearCreateIdentity(modal);
    }
    return changed;
  }

  function taskBoardCreateSemanticKey(title, project, sessionIds) {
    return JSON.stringify({
      title: String(title || "").trim(),
      cwd: taskBoardNormalizedCwd(project?.cwd),
      sessionIds: Array.from(new Set(Array.from(sessionIds || [])
        .map((sessionId) => String(sessionId || "").trim())
        .filter(Boolean))).sort(),
    });
  }

  function taskBoardClearCreateIdentity(modal) {
    if (!modal) return;
    if (modal.purpose === "attach") {
      modal.semanticKey = "";
      return;
    }
    modal.taskId = "";
    modal.semanticKey = "";
  }

  function taskBoardCreateTaskIdIsValid(taskId) {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(String(taskId || ""));
  }

  function taskBoardCancelCreateSessionHydration(modal) {
    if (!modal) return;
    if (modal.sessionHydrationFrame) {
      cancelAnimationFrame(modal.sessionHydrationFrame);
      modal.sessionHydrationFrame = 0;
    }
    if (modal.sessionHydrationTimer) {
      clearTimeout(modal.sessionHydrationTimer);
      modal.sessionHydrationTimer = 0;
    }
    modal.sessionHydrationScheduled = false;
  }

  function taskBoardHydrateCreateModalSessions(modal = taskBoardState.createModal) {
    if (!modal || taskBoardState.createModal !== modal) return false;
    taskBoardCancelCreateSessionHydration(modal);
    if (modal.sessionsHydrated) return true;
    modal.sessionsHydrated = true;
    modal.sessionRenderKey = "";
    modal.sessionRenderCatalog = null;
    renderTaskBoardCreateModal();
    return true;
  }

  function taskBoardScheduleCreateSessionHydration(modal) {
    if (
      !modal ||
      taskBoardState.createModal !== modal ||
      modal.sessionsHydrated ||
      modal.sessionHydrationScheduled
    ) {
      return;
    }
    modal.sessionHydrationScheduled = true;
    const hydrateAfterPaint = () => {
      if (taskBoardState.createModal !== modal) return;
      modal.sessionHydrationFrame = 0;
      modal.sessionHydrationTimer = setTimeout(() => {
        modal.sessionHydrationTimer = 0;
        taskBoardHydrateCreateModalSessions(modal);
      }, 0);
    };
    if (
      document.visibilityState === "hidden" ||
      typeof requestAnimationFrame !== "function"
    ) {
      hydrateAfterPaint();
      return;
    }
    modal.sessionHydrationFrame = requestAnimationFrame(hydrateAfterPaint);
  }

  function renderTaskBoardCreateModal() {
    const modal = taskBoardState.createModal;
    if (!modal) return;
    modal.titleInput.value = modal.title;
    const selectedProject = taskBoardCreateModalSelectedProject(modal);
    modal.projectSelect.value = modal.projectCwd;
    taskBoardSetDropdownTriggerLabel(modal.projectSelect, selectedProject?.label || "请选择项目");
    modal.projectSelect.title = selectedProject
      ? `${selectedProject.label}\n${selectedProject.cwd}`
      : "请选择项目";
    modal.statusSelect.value = taskBoardStatusId(modal.initialStatus);
    const selectedStatus = taskBoardStatusDefinitions.find(
      (status) => status.id === modal.statusSelect.value,
    );
    taskBoardSetDropdownTriggerLabel(
      modal.statusSelect,
      selectedStatus?.label || "新任务",
      selectedStatus?.color || "",
    );
    const modelOptions = taskBoardCreateModelOptions();
    const selectedModel = modelOptions.find((option) => option.value === modal.modelId) ||
      (modal.modelId ? { label: modal.modelId, description: "" } : modelOptions[0]);
    taskBoardReconcileCreateEffort(modal);
    const selectedEffort = taskBoardCreateEffortOptions(modal.modelId).find(
      (option) => option.value === modal.effortId,
    );
    const modelLabel = modal.modelsLoading && modelOptions.length === 1
      ? "正在加载模型…"
      : selectedModel?.label || "默认模型";
    const effortLabel = selectedEffort?.label || taskBoardReasoningEffortLabel(modal.effortId);
    modal.modelTrigger.value = modal.modelId;
    modal.modelTrigger.setAttribute("data-model-id", modal.modelId);
    modal.modelTrigger.setAttribute("data-reasoning-effort", modal.effortId);
    taskBoardSetCreateModelTriggerLabel(modal.modelTrigger, modelLabel, effortLabel);
    modal.modelTrigger.setAttribute(
      "aria-label",
      `选择新会话模型与推理强度，当前 ${modelLabel}，${effortLabel}`,
    );
    modal.modelTrigger.title = modal.modelCatalogError ||
      `模型 ${modelLabel}；推理强度 ${effortLabel}`;
    modal.modelTrigger.setAttribute("aria-busy", String(!!modal.modelsLoading));
    modal.existingButton.setAttribute("aria-pressed", String(modal.mode === "existing"));
    modal.newButton.setAttribute("aria-pressed", String(modal.mode === "new"));
    modal.newButton.setAttribute(
      "data-native-create-code",
      modal.nativeCreateAvailable === false ? modal.nativeCreateCode : "",
    );
    modal.newButton.title = modal.nativeCreateAvailable === false
      ? modal.nativeCreateMessage
      : "";
    modal.sessionSection.hidden = modal.mode !== "existing";
    modal.newSessionSection.hidden = modal.mode !== "new";
    modal.firstInstructionInput.value = modal.firstInstruction;
    modal.sessionCount.textContent = `已选 ${modal.selectedSessionIds.size} 个`;
    const sessionRenderKey = [
      modal.projectCwd,
      Array.from(modal.attachedSessionIds || []).sort().join("\u0000"),
      Array.from(modal.selectedSessionIds || []).sort().join("\u0000"),
      String(!!modal.busy),
      taskBoardState.catalogError,
    ].join("\u0001");
    if (!modal.sessionsHydrated) {
      if (modal.sessionField.dataset.taskBoardRenderState !== "pending") {
        modal.sessionField.replaceChildren();
        modal.sessionField.appendChild(taskBoardElement(
          "p",
          "codex-task-board-create-session-hint",
          "正在准备会话列表…",
        ));
        modal.sessionField.dataset.taskBoardRenderState = "pending";
      }
    } else if (
      modal.sessionRenderCatalog !== taskBoardState.catalog ||
      modal.sessionRenderKey !== sessionRenderKey
    ) {
      modal.sessionField.replaceChildren();
      modal.sessionField.dataset.taskBoardRenderState = "ready";
      if (taskBoardState.catalogError) {
        modal.sessionField.appendChild(taskBoardElement(
          "p",
          "codex-task-board-create-session-hint",
          "会话目录暂不可用，暂时无法绑定已有会话。",
        ));
      } else if (!modal.projectCwd) {
        modal.sessionField.appendChild(taskBoardElement(
          "p",
          "codex-task-board-create-session-hint",
          "请先选择项目。",
        ));
      } else {
        const sessions = taskBoardCreateSessionsForProject(
          modal.projectCwd,
          modal.attachedSessionIds,
        );
        if (!sessions.length) {
          modal.sessionField.appendChild(taskBoardElement(
            "p",
            "codex-task-board-create-session-hint",
            "该项目暂无可关联会话。",
          ));
        }
        sessions.forEach((session) => {
          const option = taskBoardElement("label", "codex-task-board-create-session-option");
          const checkbox = taskBoardElement("input");
          checkbox.type = "checkbox";
          checkbox.checked = modal.selectedSessionIds.has(session.sessionId);
          checkbox.disabled = modal.busy;
          checkbox.addEventListener("change", () => {
            if (checkbox.checked) modal.selectedSessionIds.add(session.sessionId);
            else modal.selectedSessionIds.delete(session.sessionId);
            modal.feedback = "";
            renderTaskBoardCreateModal();
          });
          const sessionIcon = taskBoardElement("span", "codex-task-board-create-session-icon");
          sessionIcon.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M3.25 3.25h9.5v7H7.4l-3.15 2.5v-2.5h-1z" stroke-linejoin="round"></path></svg>`;
          const sessionTitle = taskBoardElement(
            "span",
            "codex-task-board-create-session-title",
            String(session.title || "未命名会话"),
          );
          sessionTitle.title = String(session.title || "未命名会话");
          const sessionTimeLabel = taskBoardSessionTimeLabel(session.updatedAtMs);
          const sessionTime = taskBoardElement(
            "span",
            "codex-task-board-create-session-time",
            sessionTimeLabel,
          );
          sessionTime.title = `更新时间：${sessionTimeLabel}`;
          const sessionCopy = taskBoardElement(
            "span",
            "codex-task-board-create-session-copy",
          );
          sessionCopy.append(sessionTitle, sessionTime);
          option.append(
            checkbox,
            sessionIcon,
            sessionCopy,
          );
          modal.sessionField.appendChild(option);
        });
      }
      modal.sessionRenderCatalog = taskBoardState.catalog;
      modal.sessionRenderKey = sessionRenderKey;
    }
    modal.feedbackNode.textContent = modal.feedback;
    const submitLabel = modal.purpose === "attach"
      ? (modal.mode === "new" ? "创建并添加" : "添加会话")
      : "创建任务";
    const submitLabelNode = modal.submitButton.querySelector?.("span");
    if (submitLabelNode) submitLabelNode.textContent = submitLabel;
    modal.submitButton.disabled = modal.busy;
    modal.cancelButton.disabled = modal.busy;
    modal.closeButton.disabled = modal.busy;
    modal.existingButton.disabled = modal.busy;
    modal.newButton.disabled = modal.busy || !modal.projectCwd || modal.nativeCreateAvailable === false;
    modal.titleInput.disabled = modal.busy || modal.purpose === "attach";
    modal.projectSelect.disabled = modal.busy || modal.purpose === "attach";
    modal.statusSelect.disabled = modal.busy || modal.purpose === "attach";
    modal.modelTrigger.disabled = modal.busy || modal.mode !== "new";
    modal.firstInstructionInput.disabled = modal.busy || modal.nativeCreateAvailable === false;
  }

  function taskBoardCurrentSidebarProjectLabel() {
    const selectedThread = document.querySelector(
      `[${taskBoardNativeSelectionAttribute}="true"][data-app-action-sidebar-thread-id], ` +
      '[aria-current="page"][data-app-action-sidebar-thread-id]',
    );
    const projectList = selectedThread?.closest?.("[data-app-action-sidebar-project-list-id]");
    const projectItem = projectList?.closest?.('[role="listitem"][aria-label]');
    const projectRow = projectItem?.querySelector?.("[data-app-action-sidebar-project-row]");
    return String(
      projectRow?.getAttribute?.("data-app-action-sidebar-project-label") ||
      projectItem?.getAttribute?.("aria-label") ||
      "",
    ).trim();
  }

  function openTaskBoardAttachModal(taskOrId) {
    const task = typeof taskOrId === "object" && taskOrId
      ? taskOrId
      : (taskBoardState.snapshot?.tasks || []).find(
        (candidate) => String(candidate?.id || "") === String(taskOrId || ""),
      );
    if (!task) {
      showToast("未找到要添加会话的任务");
      return null;
    }
    return openTaskBoardCreateModal(task);
  }

  function openTaskBoardCreateModal(targetTask = null) {
    closeTaskBoardDropdownMenu({ restoreFocus: false });
    closeTaskBoardCreateModal();
    const attaching = !!targetTask;
    const attachedSessionIds = new Set(
      (Array.isArray(targetTask?.conversations) ? targetTask.conversations : [])
        .map((conversation) => String(conversation?.sessionId || "").trim())
        .filter(Boolean),
    );
    const previousFocus = document.activeElement;
    const backdrop = taskBoardElement("div", "codex-task-board-create-modal-backdrop");
    const dialog = taskBoardElement("section", "codex-task-board-create-modal");
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-modal", "true");
    dialog.setAttribute("aria-labelledby", "codex-task-board-create-modal-title");
    dialog.tabIndex = -1;
    const heading = taskBoardElement("div", "codex-task-board-create-modal-head");
    const headingCopy = taskBoardElement("div");
    const title = taskBoardElement("h2", "", attaching ? "添加会话" : "新建任务");
    title.id = "codex-task-board-create-modal-title";
    const subtitle = taskBoardElement(
      "p",
      "",
      attaching
        ? `为“${String(targetTask?.title || "未命名任务")}”关联已有会话，或创建一个新会话`
        : "将 Codex 会话组织到跨项目任务看板中",
    );
    headingCopy.append(title, subtitle);
    const closeButton = taskBoardElement("button", "codex-task-board-create-close");
    closeButton.type = "button";
    closeButton.setAttribute("aria-label", attaching ? "关闭添加会话弹窗" : "关闭新建任务弹窗");
    closeButton.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="m4 4 8 8M12 4l-8 8" stroke-linecap="round"></path></svg>`;
    closeButton.addEventListener("click", () => {
      if (!taskBoardState.createModal?.busy) closeTaskBoardCreateModal();
    });
    heading.append(headingCopy, closeButton);
    const fields = taskBoardElement("div", "codex-task-board-create-fields");
    const titleLabel = taskBoardElement("label", "codex-task-board-create-field");
    titleLabel.appendChild(taskBoardElement("span", "codex-task-board-create-field-label", "任务名称"));
    const titleInput = taskBoardElement("input", "codex-task-board-create-input");
    titleInput.type = "text";
    titleInput.maxLength = 120;
    titleInput.placeholder = "输入一个清晰、可跟进的任务名称";
    titleInput.setAttribute("aria-label", "任务名称");
    titleInput.setAttribute("data-task-board-modal-autofocus", "true");
    titleInput.addEventListener("input", () => {
      const modal = taskBoardState.createModal;
      if (modal && !modal.busy) {
        modal.title = titleInput.value;
        taskBoardClearCreateIdentity(modal);
      }
    });
    titleLabel.appendChild(titleInput);
    titleLabel.hidden = attaching;
    const fieldRow = taskBoardElement("div", "codex-task-board-create-field-row");
    const projectLabel = taskBoardElement("label", "codex-task-board-create-field");
    projectLabel.appendChild(taskBoardElement("span", "codex-task-board-create-field-label", "所属项目"));
    const projectSelect = taskBoardElement("button", "codex-task-board-create-select");
    projectSelect.type = "button";
    taskBoardConfigureDropdownTrigger(projectSelect, "请选择项目", "选择所属项目");
    projectSelect.addEventListener("click", () => openTaskBoardCreateProjectMenu(projectSelect));
    projectLabel.appendChild(projectSelect);
    const statusLabel = taskBoardElement("label", "codex-task-board-create-field");
    statusLabel.appendChild(taskBoardElement("span", "codex-task-board-create-field-label", "初始状态"));
    const statusSelect = taskBoardElement("button", "codex-task-board-create-select");
    statusSelect.type = "button";
    taskBoardConfigureDropdownTrigger(
      statusSelect,
      taskBoardStatusDefinitions[0].label,
      "选择初始状态",
      taskBoardStatusDefinitions[0].color,
    );
    statusSelect.addEventListener("click", () => openTaskBoardCreateStatusMenu(statusSelect));
    statusLabel.appendChild(statusSelect);
    fieldRow.append(projectLabel, statusLabel);
    fieldRow.hidden = attaching;
    const modeField = taskBoardElement("div", "codex-task-board-create-field");
    modeField.appendChild(taskBoardElement("span", "codex-task-board-create-field-label", "会话关联方式"));
    const modeRow = taskBoardElement("div", "codex-task-board-create-mode-row");
    modeRow.setAttribute("role", "group");
    modeRow.setAttribute("aria-label", "会话关联方式");
    const existingButton = taskBoardElement("button", "codex-task-board-create-mode");
    existingButton.type = "button";
    existingButton.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M6.1 9.9 9.9 6.1M5.2 11.7l-1 .9a2.55 2.55 0 0 1-3.6-3.6l2.1-2.1a2.55 2.55 0 0 1 3.6 0M10.8 4.3l1-.9a2.55 2.55 0 1 1 3.6 3.6l-2.1 2.1a2.55 2.55 0 0 1-3.6 0" stroke-linecap="round"></path></svg><span>绑定已有会话</span>`;
    existingButton.addEventListener("click", () => taskBoardSetCreateMode("existing"));
    const newButton = taskBoardElement("button", "codex-task-board-create-mode");
    newButton.type = "button";
    newButton.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.25"><path d="M8 1.8c.35 2.6 1.6 3.85 4.2 4.2C9.6 6.35 8.35 7.6 8 10.2 7.65 7.6 6.4 6.35 3.8 6 6.4 5.65 7.65 4.4 8 1.8ZM12.2 10c.2 1.45.9 2.15 2.35 2.35-1.45.2-2.15.9-2.35 2.35-.2-1.45-.9-2.15-2.35-2.35 1.45-.2 2.15-.9 2.35-2.35Z" stroke-linejoin="round"></path></svg><span>创建新会话</span>`;
    newButton.addEventListener("click", () => taskBoardSetCreateMode("new"));
    modeRow.append(existingButton, newButton);
    modeField.appendChild(modeRow);
    const modeContent = taskBoardElement("div", "codex-task-board-create-mode-content");
    const sessionSection = taskBoardElement(
      "div",
      "codex-task-board-create-mode-panel codex-task-board-create-session-panel",
    );
    const sessionHead = taskBoardElement("div", "codex-task-board-create-picker-head");
    sessionHead.appendChild(taskBoardElement("span", "", "选择已有会话"));
    const sessionCount = taskBoardElement("span", "codex-task-board-create-picker-count", "已选 0 个");
    sessionHead.appendChild(sessionCount);
    const sessionField = taskBoardElement("div", "codex-task-board-create-session-list");
    sessionField.setAttribute("role", "group");
    sessionField.setAttribute("aria-label", "选择同项目下的已有会话");
    sessionSection.append(sessionHead, sessionField);
    const newSessionSection = taskBoardElement(
      "div",
      "codex-task-board-create-mode-panel codex-task-board-create-new-session",
    );
    const firstInstructionField = taskBoardElement(
      "div",
      "codex-task-board-create-field codex-task-board-create-instruction-field",
    );
    firstInstructionField.appendChild(taskBoardElement(
      "span",
      "codex-task-board-create-field-label",
      "新会话首条指令",
    ));
    const firstInstructionInput = taskBoardElement("textarea", "codex-task-board-create-textarea");
    firstInstructionInput.rows = 4;
    firstInstructionInput.maxLength = 4000;
    firstInstructionInput.placeholder = "例如：梳理任务看板的数据模型，并输出可执行方案";
    firstInstructionInput.setAttribute("aria-label", "新会话首条指令");
    firstInstructionInput.addEventListener("input", () => {
      const modal = taskBoardState.createModal;
      if (modal && !modal.busy) modal.firstInstruction = firstInstructionInput.value;
    });
    const firstInstructionComposer = taskBoardElement(
      "div",
      "codex-task-board-create-composer",
    );
    const modelTrigger = taskBoardElement(
      "button",
      "codex-task-board-create-model-trigger",
    );
    modelTrigger.type = "button";
    taskBoardConfigureCreateModelTrigger(modelTrigger);
    modelTrigger.addEventListener("click", () => openTaskBoardCreateSettingsMenu(modelTrigger));
    firstInstructionComposer.append(firstInstructionInput, modelTrigger);
    firstInstructionField.appendChild(firstInstructionComposer);
    newSessionSection.appendChild(firstInstructionField);
    modeContent.append(sessionSection, newSessionSection);
    const feedbackNode = taskBoardElement("p", "codex-task-board-create-feedback");
    feedbackNode.setAttribute("aria-live", "polite");
    fields.append(titleLabel, fieldRow, modeField, modeContent, feedbackNode);
    const footer = taskBoardElement("footer", "codex-task-board-create-modal-footer");
    footer.classList.toggle("codex-task-board-create-modal-footer-actions-only", !attaching);
    const actions = taskBoardElement("div", "codex-task-board-create-modal-actions");
    const cancelButton = taskBoardElement("button", "codex-task-board-create-cancel", "取消");
    cancelButton.type = "button";
    cancelButton.addEventListener("click", () => {
      if (!taskBoardState.createModal?.busy) closeTaskBoardCreateModal();
    });
    const submitButton = taskBoardElement("button", "codex-task-board-create-submit");
    submitButton.type = "button";
    submitButton.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.3"><path d="M8 3v10M3 8h10" stroke-linecap="round"></path></svg><span>创建任务</span>`;
    submitButton.addEventListener("click", () => void submitTaskBoardCreate());
    actions.append(submitButton, cancelButton);
    if (attaching) {
      const note = taskBoardElement("span", "codex-task-board-create-note");
      note.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M8 2c.3 2.2 1.4 3.3 3.6 3.6C9.4 5.9 8.3 7 8 9.2 7.7 7 6.6 5.9 4.4 5.6 6.6 5.3 7.7 4.2 8 2Z" stroke-linejoin="round"></path></svg><span>只可追加当前任务所属项目中的会话。</span>`;
      footer.appendChild(note);
    }
    footer.appendChild(actions);
    dialog.append(heading, fields, footer);
    backdrop.appendChild(dialog);
    let backdropPressStarted = false;
    let backdropPressCompleted = false;
    backdrop.addEventListener("pointerdown", (event) => {
      backdropPressStarted = event.target === backdrop;
      backdropPressCompleted = false;
    });
    backdrop.addEventListener("pointerup", (event) => {
      backdropPressCompleted = backdropPressStarted && event.target === backdrop;
    });
    backdrop.addEventListener("pointercancel", () => {
      backdropPressStarted = false;
      backdropPressCompleted = false;
    });
    backdrop.addEventListener("click", (event) => {
      const shouldClose = event.target === backdrop && backdropPressCompleted;
      backdropPressStarted = false;
      backdropPressCompleted = false;
      if (shouldClose && !taskBoardState.createModal?.busy) closeTaskBoardCreateModal();
    });
    document.body.appendChild(backdrop);
    const projects = taskBoardCreateModalProjects();
    const nativeProjects = projects.filter((project) => taskBoardNativeProjectRow(project));
    const currentProjectLabel = taskBoardCurrentSidebarProjectLabel().toLocaleLowerCase("zh-Hans-CN");
    const currentProject = currentProjectLabel
      ? projects.find((project) => {
        return project.label.trim().toLocaleLowerCase("zh-Hans-CN") === currentProjectLabel &&
          taskBoardCreateSessionsForProject(project.cwd).length > 0;
      }) || projects.find((project) => {
        return project.label.trim().toLocaleLowerCase("zh-Hans-CN") === currentProjectLabel;
      })
      : null;
    const targetProject = attaching
      ? {
        cwd: taskBoardNormalizedCwd(targetTask?.project?.cwd),
        label: String(
          targetTask?.project?.label ||
          displayProjectName(targetTask?.project?.cwd || ""),
        ),
      }
      : null;
    const defaultProject = targetProject || currentProject || nativeProjects.find(
      (project) => taskBoardCreateSessionsForProject(project.cwd).length >= 2,
    ) || nativeProjects[0] || projects.find(
      (project) => taskBoardCreateSessionsForProject(project.cwd).length >= 2,
    ) || projects[0] || null;
    const defaultSessions = defaultProject
      ? taskBoardCreateSessionsForProject(defaultProject.cwd, attachedSessionIds)
      : [];
    const defaultModelId = taskBoardCreateDefaultModelId();
    const defaultEffortId = taskBoardCreateDefaultEffortId(defaultModelId);
    const modal = {
      backdrop,
      dialog,
      closeButton,
      existingButton,
      newButton,
      titleInput,
      projectSelect,
      statusSelect,
      modelTrigger,
      firstInstructionField,
      firstInstructionComposer,
      firstInstructionInput,
      sessionSection,
      newSessionSection,
      sessionCount,
      sessionField,
      feedbackNode,
      cancelButton,
      submitButton,
      purpose: attaching ? "attach" : "create",
      targetProject,
      attachedSessionIds,
      mode: "existing",
      title: attaching ? String(targetTask?.title || "未命名任务") : "",
      projectCwd: defaultProject?.cwd || "",
      initialStatus: "new",
      modelId: defaultModelId,
      effortId: defaultEffortId,
      modelSelectionTouched: false,
      modelsLoading: false,
      modelCatalogError: "",
      sessionsHydrated: false,
      sessionHydrationScheduled: false,
      sessionHydrationFrame: 0,
      sessionHydrationTimer: 0,
      sessionRenderCatalog: null,
      sessionRenderKey: "",
      selectedSessionIds: new Set(defaultSessions[0]?.sessionId ? [defaultSessions[0].sessionId] : []),
      firstInstruction: "",
      nativeCreateAvailable: false,
      nativeCreateCode: "",
      nativeCreateMessage: "",
      nativeProbeId: 0,
      taskId: attaching ? String(targetTask?.id || "") : "",
      semanticKey: "",
      feedback: "",
      busy: false,
    };
    taskBoardState.createModal = modal;
    taskBoardState.createModalPreviousFocus = previousFocus;
    taskBoardState.createModalKeydownHandler = taskBoardCreateModalKeydown;
    document.addEventListener("keydown", taskBoardState.createModalKeydownHandler, true);
    renderTaskBoardCreateModal();
    requestAnimationFrame(() => {
      if (taskBoardState.createModal === modal) {
        (attaching ? modal.existingButton : modal.titleInput).focus?.();
      }
    });
    taskBoardScheduleCreateSessionHydration(modal);
    void taskBoardRefreshNativeCreateAvailability(modal);
    void taskBoardRefreshCreateModels(modal);
    return modal;
  }

  function taskBoardCreateTaskId() {
    if (typeof globalThis.crypto?.randomUUID === "function") return globalThis.crypto.randomUUID();
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (letter) => {
      const random = Math.floor(Math.random() * 16);
      return (letter === "x" ? random : (random & 0x3) | 0x8).toString(16);
    });
  }

  function taskBoardCreateFailureMessage(result, fallback = "创建任务失败，请检查目录或桥接连接") {
    const code = String(result?.code || "").trim();
    if (code === "invalid_input") return "任务信息无效，请检查标题、项目和关联会话";
    if (code === "project_mismatch") return "所选会话不属于当前项目，请重新选择";
    if (code === "task_id_conflict") return "任务 ID 冲突，请重试";
    if (code === "task_not_found") return "任务不存在或已被移除";
    if (code === "session_not_found") return "关联会话不存在，已刷新会话目录";
    if (code === "revision_conflict") return "任务修订已变化，请检查后重试";
    if (code === "bridge_unavailable") return "任务看板桥接暂不可用，请稍后重试";
    if (code === "task_board_busy") return "任务看板正忙，请稍后重试";
    if (code === "task_file_invalid") return "任务文件无效，请检查后重试";
    if (code === "task_board_unavailable") return "任务看板暂不可用，请稍后重试";
    return taskBoardMessageFromResult(result, fallback);
  }

  function taskBoardNativeCreateFailureMessage(result) {
    const code = String(result?.code || "").trim();
    if (code === "native_create_busy") return taskBoardNativeCreateBusyMessage;
    if (code === "native_create_unavailable") return "当前项目暂不支持新建关联会话";
    if (code === "native_create_timeout") return "等待新会话就绪超时";
    if (code === "composer_unavailable") return "未找到原生会话编辑器";
    if (code === "composer_submit_unavailable") return "原生发送按钮尚未就绪，请稍后重试";
    if (code === "composer_submit_failed") return "无法提交首条指令";
    if (code === "native_model_unavailable") return "原生模型列表暂不可用，请稍后重试";
    if (code === "native_model_not_found") return "所选模型在当前 Codex 会话中不可用";
    if (code === "native_model_select_failed") return "无法切换到所选模型，请稍后重试";
    if (code === "native_effort_unavailable") return "原生推理强度列表暂不可用，请稍后重试";
    if (code === "native_effort_not_found") return "所选推理强度在当前模型中不可用";
    if (code === "native_effort_select_failed") return "无法切换到所选推理强度，请稍后重试";
    if (code === "runtime_replaced") return "Codex 页面已更新，请重试";
    if (code === "session_not_found") return "关联会话尚未就绪，将在下次打开任务看板时重试";
    if (code === "revision_conflict") return "任务修订已变化，请检查后重试";
    return "创建原生关联会话失败，将在下次打开任务看板时重试";
  }

  function taskBoardCreatePayload(taskId, expectedRevision, title, project, sessionIds) {
    return {
      taskId,
      expectedRevision,
      title,
      project: { cwd: project.cwd, label: project.label },
      sessionIds: Array.from(sessionIds),
    };
  }

  function taskBoardAttachPayload(taskId, expectedRevision, sessionIds) {
    return {
      taskId,
      expectedRevision,
      sessionIds: Array.from(sessionIds),
    };
  }

  function taskBoardDetachPayload(taskId, expectedRevision, sessionIds) {
    return {
      taskId,
      expectedRevision,
      sessionIds: Array.from(sessionIds),
    };
  }

  async function taskBoardApplyInitialStatus(taskId, initialStatus) {
    const targetStatus = taskBoardStatusId(initialStatus);
    if (targetStatus === "new") return { status: "ok" };
    const targetIndex = taskBoardMoveTargetIndex(taskId, targetStatus);
    const result = await taskBoardMoveTask(taskId, targetStatus, targetIndex);
    if (result?.status !== "ok") {
      showToast("任务已创建，但初始状态设置失败，可在看板中手动移动");
    }
    return result;
  }

  function taskBoardCreateRequestIsCurrent(modal, requestId) {
    return taskBoardState.createModal === modal && requestId === taskBoardState.createModalRequestId;
  }

  function taskBoardCreateNormalizedFailure(result) {
    const code = String(result?.code || "").trim() ||
      (result?.status === "conflict" ? "revision_conflict" : "bridge_unavailable");
    return {
      ...result,
      status: String(result?.status || "failed"),
      code,
      message: String(result?.message || "").trim(),
    };
  }

  function taskBoardNativeCreateRecoveryRecord(value) {
    if (!value || typeof value !== "object") return null;
    const kind = value.kind === "attach-conversation"
      ? "attach-conversation"
      : "create-task";
    const taskId = String(value.taskId || "").trim();
    const title = String(value.title || "").trim();
    const projectCwd = taskBoardNormalizedCwd(value.project?.cwd);
    const projectLabel = String(value.project?.label || "").trim();
    const sessionId = String(value.sessionId || "").trim();
    const initialStatus = taskBoardStatusId(value.initialStatus);
    const createdAtMs = Number(value.createdAtMs || 0);
    if (!taskBoardCreateTaskIdIsValid(taskId) || !title || !projectCwd || !sessionId ||
      !Number.isFinite(createdAtMs) || createdAtMs <= 0 ||
      taskBoardNativeNow() - createdAtMs > taskBoardNativeCreateRecoveryTtlMs) {
      return null;
    }
    return {
      kind,
      taskId,
      title,
      project: { cwd: projectCwd, label: projectLabel || displayProjectName(projectCwd) },
      sessionId,
      initialStatus,
      createdAtMs,
    };
  }

  function taskBoardReadNativeCreateRecovery() {
    try {
      const parsed = JSON.parse(sessionStorage.getItem(taskBoardNativeCreateRecoveryKey) || "null");
      const record = taskBoardNativeCreateRecoveryRecord(parsed);
      if (!record) sessionStorage.removeItem(taskBoardNativeCreateRecoveryKey);
      return record;
    } catch {
      try {
        sessionStorage.removeItem(taskBoardNativeCreateRecoveryKey);
      } catch {}
      return null;
    }
  }

  function taskBoardSaveNativeCreateRecovery(operation) {
    const record = taskBoardNativeCreateRecoveryRecord({
      kind: operation?.kind,
      taskId: operation?.taskId,
      title: operation?.title,
      project: operation?.project,
      sessionId: operation?.sessionId,
      initialStatus: operation?.initialStatus,
      createdAtMs: operation?.createdAtMs,
    });
    if (!record) return;
    try {
      sessionStorage.setItem(taskBoardNativeCreateRecoveryKey, JSON.stringify(record));
    } catch {}
  }

  function taskBoardClearNativeCreateRecovery() {
    try {
      sessionStorage.removeItem(taskBoardNativeCreateRecoveryKey);
    } catch {}
  }

  function taskBoardNativeCreateOperationCurrent(operation) {
    return taskBoardState.nativeCreateOperation === operation &&
      !operation?.cancelled &&
      taskBoardNativeRuntimeCurrent(operation.runtimeId);
  }

  function taskBoardLeaseNativeCreateOperation(operation) {
    if (!operation?.operationId) return;
    window.__codexElvesTaskBoardNativeOperationLease = {
      operationId: operation.operationId,
      runtimeId: operation.runtimeId,
      createdAtMs: Date.now(),
    };
  }

  function taskBoardReleaseNativeCreateOperation(operation) {
    if (taskBoardState.nativeCreateOperation === operation) {
      taskBoardState.nativeCreateOperation = null;
    }
    const lease = window.__codexElvesTaskBoardNativeOperationLease;
    if (
      operation?.operationId &&
      String(lease?.operationId || "") === operation.operationId
    ) {
      delete window.__codexElvesTaskBoardNativeOperationLease;
    }
  }

  function taskBoardCreateNativeOperation(
    taskId,
    title,
    project,
    expectedRevision,
    sessionId = "",
    initialStatus = "new",
    kind = "create-task",
  ) {
    if (taskBoardForeignNativeCreateLease()) return null;
    const operation = {
      kind: kind === "attach-conversation" ? "attach-conversation" : "create-task",
      taskId,
      title,
      project: { cwd: taskBoardNormalizedCwd(project?.cwd), label: String(project?.label || "") },
      expectedRevision,
      sessionId: String(sessionId || "").trim(),
      initialStatus: taskBoardStatusId(initialStatus),
      createdAtMs: taskBoardNativeNow(),
      runtimeId: taskBoardNativeRuntimeId,
      operationId: `${taskBoardNativeRuntimeId}:${Date.now()}:${Math.random().toString(36).slice(2)}`,
      cancelled: false,
    };
    taskBoardState.nativeCreateOperation = operation;
    taskBoardLeaseNativeCreateOperation(operation);
    return operation;
  }

  function taskBoardCancelNativeCreateOperation() {
    const operation = taskBoardState.nativeCreateOperation;
    if (!operation) return;
    operation.cancelled = true;
    if (operation.sessionId) taskBoardSaveNativeCreateRecovery(operation);
    taskBoardReleaseNativeCreateOperation(operation);
  }

  async function taskBoardCreateNativeTask(operation) {
    let expectedRevision = Number.isSafeInteger(operation.expectedRevision)
      ? operation.expectedRevision
      : taskBoardState.snapshot.revision;
    let revisionRetries = 0;
    let sessionRetryIndex = 0;
    while (taskBoardNativeCreateOperationCurrent(operation)) {
      const attaching = operation.kind === "attach-conversation";
      const payload = attaching
        ? taskBoardAttachPayload(
          operation.taskId,
          expectedRevision,
          [operation.sessionId],
        )
        : taskBoardCreatePayload(
          operation.taskId,
          expectedRevision,
          operation.title,
          operation.project,
          [operation.sessionId],
        );
      let result;
      try {
        result = await taskBoardMockOrBridgeResult(
          attaching ? "attachConversations" : "createTask",
          payload,
        );
      } catch (error) {
        result = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
      }
      if (!taskBoardNativeCreateOperationCurrent(operation)) {
        taskBoardReleaseNativeCreateOperation(operation);
        return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
      }
      result = taskBoardCreateNormalizedFailure(result);
      const snapshot = taskBoardSnapshotResult(result);
      if (result?.status === "ok" && snapshot) {
        taskBoardState.snapshot = snapshot;
        taskBoardState.snapshotError = "";
        taskBoardClearNativeCreateRecovery();
        taskBoardReleaseNativeCreateOperation(operation);
        renderTaskBoard();
        void refreshTaskBoardConversationStatuses();
        return { status: "ok", snapshot };
      }
      if ((result?.status === "conflict" || result?.code === "revision_conflict") && revisionRetries < 1) {
        const conflictSnapshot = taskBoardConflictSnapshotResult(result);
        if (conflictSnapshot) {
          revisionRetries += 1;
          expectedRevision = conflictSnapshot.revision;
          taskBoardState.snapshot = conflictSnapshot;
          taskBoardState.snapshotError = "";
          renderTaskBoard();
          continue;
        }
      }
      if (result?.code === "session_not_found" && sessionRetryIndex < taskBoardNativeCreateSessionRetryDelaysMs.length) {
        const delay = taskBoardNativeCreateSessionRetryDelaysMs[sessionRetryIndex++];
        await taskBoardNativeWait(delay);
        continue;
      }
      taskBoardSaveNativeCreateRecovery(operation);
      taskBoardReleaseNativeCreateOperation(operation);
      return result;
    }
    taskBoardReleaseNativeCreateOperation(operation);
    return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
  }

  async function taskBoardStartNativeTaskCreate(modal, requestId, title, project) {
    const leaseFailure = taskBoardNativeCreateLeaseFailure();
    if (leaseFailure) {
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(
          modal,
          taskBoardNativeCreateFailureMessage(leaseFailure),
        );
      } else {
        showToast(taskBoardNativeCreateFailureMessage(leaseFailure));
      }
      return leaseFailure;
    }
    const semanticKey = taskBoardCreateSemanticKey(title, project, []);
    if (modal.semanticKey !== semanticKey || !taskBoardCreateTaskIdIsValid(modal.taskId)) {
      modal.taskId = taskBoardCreateTaskId();
      modal.semanticKey = semanticKey;
    }
    const operation = taskBoardCreateNativeOperation(
      modal.taskId,
      title,
      project,
      taskBoardState.snapshot.revision,
      "",
      modal.initialStatus,
      "create-task",
    );
    if (!operation) {
      const failure = taskBoardNativeCreateLeaseFailure() ||
        taskBoardNativeFailure("native_create_busy", taskBoardNativeCreateBusyMessage);
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(
          modal,
          taskBoardNativeCreateFailureMessage(failure),
        );
      } else {
        showToast(taskBoardNativeCreateFailureMessage(failure));
      }
      return failure;
    }
    let started;
    try {
      started = await Promise.resolve(
        taskBoardNativeAdapter.startConversation(
          project,
          modal.firstInstruction,
          modal.modelId,
          modal.effortId,
        ),
      );
    } catch (error) {
      started = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
    }
    if (!taskBoardNativeCreateOperationCurrent(operation)) {
      taskBoardReleaseNativeCreateOperation(operation);
      return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    }
    if (started?.status !== "ok" || !String(started?.sessionId || "").trim() ||
      isTemporaryThreadId(String(started?.sessionId || ""))) {
      taskBoardReleaseNativeCreateOperation(operation);
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(modal, taskBoardNativeCreateFailureMessage(started));
      } else {
        showToast(taskBoardNativeCreateFailureMessage(started));
      }
      return started;
    }
    operation.sessionId = String(started.sessionId).trim();
    operation.createdAtMs = taskBoardNativeNow();
    taskBoardSaveNativeCreateRecovery(operation);
    const result = await taskBoardCreateNativeTask(operation);
    if (result?.status === "ok") {
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) closeTaskBoardCreateModal();
      await taskBoardApplyInitialStatus(operation.taskId, operation.initialStatus);
      return result;
    }
    if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
      taskBoardSetCreateModalFeedback(modal, taskBoardNativeCreateFailureMessage(result));
    } else {
      showToast("会话已创建，但任务尚未保存；下次打开任务看板时将自动重试");
    }
    return result;
  }

  async function taskBoardStartNativeConversationAttach(modal, requestId, project) {
    const leaseFailure = taskBoardNativeCreateLeaseFailure();
    if (leaseFailure) {
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(
          modal,
          taskBoardNativeCreateFailureMessage(leaseFailure),
        );
      } else {
        showToast(taskBoardNativeCreateFailureMessage(leaseFailure));
      }
      return leaseFailure;
    }
    const operation = taskBoardCreateNativeOperation(
      modal.taskId,
      modal.title,
      project,
      taskBoardState.snapshot.revision,
      "",
      "new",
      "attach-conversation",
    );
    if (!operation) {
      const failure = taskBoardNativeCreateLeaseFailure() ||
        taskBoardNativeFailure("native_create_busy", taskBoardNativeCreateBusyMessage);
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(
          modal,
          taskBoardNativeCreateFailureMessage(failure),
        );
      } else {
        showToast(taskBoardNativeCreateFailureMessage(failure));
      }
      return failure;
    }
    let started;
    try {
      started = await Promise.resolve(
        taskBoardNativeAdapter.startConversation(
          project,
          modal.firstInstruction,
          modal.modelId,
          modal.effortId,
        ),
      );
    } catch (error) {
      started = {
        status: "failed",
        code: "bridge_unavailable",
        message: taskBoardMessageFromResult(error, ""),
      };
    }
    if (!taskBoardNativeCreateOperationCurrent(operation)) {
      taskBoardReleaseNativeCreateOperation(operation);
      return taskBoardNativeFailure("runtime_replaced", "Codex 页面已更新，请重试");
    }
    if (
      started?.status !== "ok" ||
      !String(started?.sessionId || "").trim() ||
      isTemporaryThreadId(String(started?.sessionId || ""))
    ) {
      taskBoardReleaseNativeCreateOperation(operation);
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
        taskBoardSetCreateModalFeedback(modal, taskBoardNativeCreateFailureMessage(started));
      } else {
        showToast(taskBoardNativeCreateFailureMessage(started));
      }
      return started;
    }
    operation.sessionId = String(started.sessionId).trim();
    operation.createdAtMs = taskBoardNativeNow();
    taskBoardSaveNativeCreateRecovery(operation);
    const result = await taskBoardCreateNativeTask(operation);
    if (result?.status === "ok") {
      if (taskBoardCreateRequestIsCurrent(modal, requestId)) closeTaskBoardCreateModal();
      return result;
    }
    if (taskBoardCreateRequestIsCurrent(modal, requestId)) {
      taskBoardSetCreateModalFeedback(modal, taskBoardNativeCreateFailureMessage(result));
    } else {
      showToast("会话已创建，但尚未添加到任务；下次打开任务看板时将自动重试");
    }
    return result;
  }

  async function taskBoardRetryNativeCreateRecovery() {
    if (taskBoardState.nativeCreateRecoveryAttempted || taskBoardState.nativeCreateOperation) return;
    const record = taskBoardReadNativeCreateRecovery();
    if (!record) {
      taskBoardState.nativeCreateRecoveryAttempted = true;
      return;
    }
    const leaseFailure = taskBoardNativeCreateLeaseFailure();
    if (leaseFailure) {
      showToast(taskBoardNativeCreateFailureMessage(leaseFailure));
      return leaseFailure;
    }
    taskBoardState.nativeCreateRecoveryAttempted = true;
    const operation = taskBoardCreateNativeOperation(
      record.taskId,
      record.title,
      record.project,
      taskBoardState.snapshot.revision,
      record.sessionId,
      record.initialStatus,
      record.kind,
    );
    if (!operation) {
      taskBoardState.nativeCreateRecoveryAttempted = false;
      const failure = taskBoardNativeCreateLeaseFailure() ||
        taskBoardNativeFailure("native_create_busy", taskBoardNativeCreateBusyMessage);
      showToast(taskBoardNativeCreateFailureMessage(failure));
      return failure;
    }
    operation.createdAtMs = record.createdAtMs;
    const result = await taskBoardCreateNativeTask(operation);
    if (result?.status === "ok") {
      if (operation.kind === "create-task") {
        await taskBoardApplyInitialStatus(operation.taskId, operation.initialStatus);
      }
    } else if (result?.code === "task_not_found") {
      taskBoardClearNativeCreateRecovery();
    } else {
      showToast(
        operation.kind === "attach-conversation"
          ? "会话已创建，但尚未添加到任务，请稍后重试"
          : "会话已创建，但任务尚未保存，请稍后重试",
      );
    }
    return result;
  }

  async function taskBoardRefreshCatalogForCreate(modal, requestId) {
    let result;
    try {
      result = await taskBoardMockOrBridgeResult("catalog");
    } catch (error) {
      result = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
    }
    if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return false;
    const catalog = taskBoardCatalogResult(result);
    if (catalog) {
      taskBoardState.catalog = catalog;
      taskBoardState.catalogError = "";
      taskBoardReconcileCreateSelectedSessions(modal);
    } else {
      taskBoardState.catalogError = taskBoardMessageFromResult(result, "会话目录刷新失败");
    }
    renderTaskBoard();
    renderTaskBoardCreateModal();
    return true;
  }

  async function taskBoardAttachSelectedConversations(modal, requestId, sessionIds) {
    let expectedRevision = taskBoardState.snapshot.revision;
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const payload = taskBoardAttachPayload(modal.taskId, expectedRevision, sessionIds);
      let result;
      try {
        result = await taskBoardMockOrBridgeResult("attachConversations", payload);
      } catch (error) {
        result = {
          status: "failed",
          code: "bridge_unavailable",
          message: taskBoardMessageFromResult(error, ""),
        };
      }
      if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return;
      result = taskBoardCreateNormalizedFailure(result);
      const snapshot = taskBoardSnapshotResult(result);
      if (result?.status === "ok" && snapshot) {
        taskBoardState.snapshot = snapshot;
        taskBoardState.snapshotError = "";
        closeTaskBoardCreateModal();
        renderTaskBoard();
        void refreshTaskBoardConversationStatuses();
        return;
      }
      if (result?.status === "conflict" || result?.code === "revision_conflict") {
        const conflictSnapshot = taskBoardConflictSnapshotResult(result);
        if (conflictSnapshot) {
          taskBoardState.snapshot = conflictSnapshot;
          taskBoardState.snapshotError = "";
          renderTaskBoard();
        }
        if (attempt === 0 && conflictSnapshot) {
          expectedRevision = conflictSnapshot.revision;
          continue;
        }
        taskBoardSetCreateModalFeedback(modal, taskBoardCreateFailureMessage(result));
        return;
      }
      if (result?.code === "session_not_found") {
        if (!await taskBoardRefreshCatalogForCreate(modal, requestId)) return;
        if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return;
      }
      taskBoardSetCreateModalFeedback(modal, taskBoardCreateFailureMessage(
        result,
        "添加会话失败，请稍后重试",
      ));
      return;
    }
  }

  async function submitTaskBoardCreate() {
    const modal = taskBoardState.createModal;
    if (!modal || modal.busy) return;
    const title = String(modal.title || "").trim();
    const titleLength = Array.from(title).length;
    const project = taskBoardCreateModalSelectedProject(modal);
    if (modal.purpose !== "attach" && (!title || titleLength > 120)) {
      taskBoardSetCreateModalFeedback(modal, "任务标题必须为 1 到 120 个字符");
      return;
    }
    if (!project) {
      taskBoardSetCreateModalFeedback(modal, "请选择项目");
      return;
    }
    if (modal.mode === "existing" && taskBoardState.catalogError) {
      taskBoardSetCreateModalFeedback(modal, "会话目录暂不可用，无法绑定已有会话");
      return;
    }
    taskBoardReconcileCreateSelectedSessions(modal);
    const sessionIds = Array.from(modal.selectedSessionIds);
    if (modal.mode === "existing" && !sessionIds.length) {
      taskBoardSetCreateModalFeedback(modal, "请至少选择一个关联会话");
      return;
    }
    taskBoardSetCreateModalBusy(modal, true);
    const requestId = ++taskBoardState.createModalRequestId;
    try {
      if (modal.mode === "new") {
        let probe;
        try {
          probe = await Promise.resolve(taskBoardNativeAdapter.probe(project));
        } catch (error) {
          probe = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
        }
        if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return;
        if (probe?.status !== "ok" || probe?.canStart !== true) {
          taskBoardSetCreateModalFeedback(modal, taskBoardMessageFromResult(probe, "当前版本暂不支持新建关联会话"));
          return;
        }
        if (!String(modal.firstInstruction || "").trim()) {
          taskBoardSetCreateModalFeedback(modal, "请输入首条指令");
          return;
        }
        if (modal.purpose === "attach") {
          await taskBoardStartNativeConversationAttach(modal, requestId, project);
        } else {
          await taskBoardStartNativeTaskCreate(modal, requestId, title, project);
        }
        return;
      }
      if (modal.purpose === "attach") {
        await taskBoardAttachSelectedConversations(modal, requestId, sessionIds);
        return;
      }
      const semanticKey = taskBoardCreateSemanticKey(title, project, sessionIds);
      if (modal.semanticKey !== semanticKey || !taskBoardCreateTaskIdIsValid(modal.taskId)) {
        modal.taskId = taskBoardCreateTaskId();
        modal.semanticKey = semanticKey;
      }
      const taskId = modal.taskId;
      let expectedRevision = taskBoardState.snapshot.revision;
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const payload = taskBoardCreatePayload(taskId, expectedRevision, title, project, sessionIds);
        let result;
        try {
          result = await taskBoardMockOrBridgeResult("createTask", payload);
        } catch (error) {
          result = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
        }
        if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return;
        result = taskBoardCreateNormalizedFailure(result);
        const snapshot = taskBoardSnapshotResult(result);
        if (result?.status === "ok" && snapshot) {
          const initialStatus = taskBoardStatusId(modal.initialStatus);
          taskBoardState.snapshot = snapshot;
          taskBoardState.snapshotError = "";
          closeTaskBoardCreateModal();
          renderTaskBoard();
          await taskBoardApplyInitialStatus(taskId, initialStatus);
          return;
        }
        if (result?.status === "conflict" || result?.code === "revision_conflict") {
          const conflictSnapshot = taskBoardConflictSnapshotResult(result);
          if (conflictSnapshot) {
            taskBoardState.snapshot = conflictSnapshot;
            taskBoardState.snapshotError = "";
            renderTaskBoard();
          }
          if (attempt === 0 && conflictSnapshot) {
            expectedRevision = conflictSnapshot.revision;
            continue;
          }
          taskBoardSetCreateModalFeedback(modal, taskBoardCreateFailureMessage(result));
          return;
        }
        if (result?.code === "session_not_found") {
          if (!await taskBoardRefreshCatalogForCreate(modal, requestId)) return;
          if (!taskBoardCreateRequestIsCurrent(modal, requestId)) return;
          taskBoardSetCreateModalFeedback(modal, taskBoardCreateFailureMessage(result));
          return;
        }
        if (result?.code === "task_id_conflict") taskBoardClearCreateIdentity(modal);
        taskBoardSetCreateModalFeedback(modal, taskBoardCreateFailureMessage(result));
        return;
      }
    } finally {
      if (taskBoardState.createModal === modal && requestId === taskBoardState.createModalRequestId) {
        taskBoardSetCreateModalBusy(modal, false);
      }
    }
  }

  function taskBoardConversationIcon() {
    const icon = taskBoardElement("span", "codex-task-board-conversation-icon");
    icon.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M3.25 3.25h9.5v7H7.4l-3.15 2.5v-2.5h-1z" stroke-linejoin="round"></path></svg>`;
    return icon;
  }

  function taskBoardConversationStatusNode(status) {
    const normalized = status && typeof status === "object"
      ? status
      : { id: "unknown", label: "状态未知" };
    const node = taskBoardElement(
      "span",
      "codex-task-board-conversation-state",
    );
    node.setAttribute("data-conversation-status", String(normalized.id || "unknown"));
    const indicator = taskBoardElement(
      "span",
      "codex-task-board-conversation-status-indicator",
    );
    indicator.setAttribute("aria-hidden", "true");
    node.append(indicator, document.createTextNode(String(normalized.label || "状态未知")));
    return node;
  }

  function taskBoardConversationButton(
    conversation,
    className = "codex-task-board-conversation",
    { showState = true, projection = null } = {},
  ) {
    const resolvedProjection = projection || taskBoardConversationProjection(conversation);
    const button = taskBoardElement("button", className);
    button.type = "button";
    button.disabled = !resolvedProjection.available;
    button.setAttribute(
      "aria-label",
      `${resolvedProjection.title}，${resolvedProjection.status?.label || "状态未知"}，${resolvedProjection.label}`,
    );
    const title = taskBoardElement(
      "span",
      "codex-task-board-conversation-title",
      resolvedProjection.title,
    );
    button.append(taskBoardConversationIcon(), title);
    if (showState || !resolvedProjection.available) button.appendChild(
      taskBoardConversationStatusNode(resolvedProjection.status),
    );
    if (resolvedProjection.available) {
      button.addEventListener("click", () => void openTaskBoardConversation(conversation));
    }
    return button;
  }

  function taskBoardConversationRemoveIcon() {
    const icon = taskBoardElement("span");
    icon.setAttribute("aria-hidden", "true");
    icon.innerHTML = `<svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.35"><path d="M4 4l8 8M12 4l-8 8" stroke-linecap="round"></path></svg>`;
    return icon;
  }

  function taskBoardConversationRow(task, conversation, {
    className = "codex-task-board-conversation",
    showState = true,
  } = {}) {
    const projection = taskBoardConversationProjection(conversation);
    const row = taskBoardElement("div", "codex-task-board-conversation-row");
    const conversationButton = taskBoardConversationButton(
      conversation,
      className,
      { showState, projection },
    );
    const removeButton = taskBoardElement(
      "button",
      "codex-task-board-conversation-remove",
    );
    removeButton.type = "button";
    removeButton.disabled = taskBoardState.detachBusy;
    removeButton.title = "从任务中移除会话";
    removeButton.setAttribute(
      "aria-label",
      `从任务 ${String(task?.title || "未命名任务")} 中移除会话 ${projection.title}`,
    );
    removeButton.setAttribute(
      "data-task-board-session-id",
      String(conversation?.sessionId || ""),
    );
    removeButton.appendChild(taskBoardConversationRemoveIcon());
    removeButton.addEventListener("click", (event) => {
      event.preventDefault?.();
      event.stopPropagation?.();
      openTaskBoardDetachDialog(task, conversation, removeButton);
    });
    row.append(conversationButton, removeButton);
    return row;
  }

  function taskBoardDetachFailureMessage(result) {
    const code = String(result?.code || "").trim();
    if (code === "invalid_input") return "关联信息无效，请关闭弹窗后刷新任务看板";
    if (code === "task_not_found") return "任务不存在或已被移除";
    if (code === "revision_conflict") return "任务已被其他更改更新，请确认最新状态后重试";
    if (code === "bridge_unavailable") return "任务看板桥接暂不可用，请稍后重试";
    if (code === "task_board_busy") return "任务看板正忙，请稍后重试";
    if (code === "task_file_invalid") return "任务文件无效，请检查后重试";
    if (code === "task_board_unavailable") return "任务看板暂不可用，请稍后重试";
    return taskBoardMessageFromResult(result, "移除关联会话失败，请稍后重试");
  }

  function taskBoardDetachDialogLabel(value, maxCharacters) {
    const normalized = String(value || "").replace(/\s+/g, " ").trim();
    const characters = Array.from(normalized);
    if (characters.length <= maxCharacters) return normalized;
    return `${characters.slice(0, maxCharacters).join("")}…`;
  }

  function taskBoardDetachDialogFocusable(dialog) {
    return Array.from(
      dialog?.overlay?.querySelectorAll?.("button:not([disabled])") || [],
    );
  }

  function taskBoardDetachRequestIsCurrent(dialog, requestId) {
    return taskBoardState.detachDialog === dialog &&
      requestId === taskBoardState.detachRequestId;
  }

  function taskBoardSetDetachDialogBusy(dialog, busy) {
    if (!dialog || taskBoardState.detachDialog !== dialog) return;
    taskBoardState.detachBusy = !!busy;
    dialog.overlay.setAttribute("aria-busy", String(!!busy));
    dialog.cancelButton.disabled = !!busy;
    dialog.confirmButton.disabled = !!busy;
    dialog.confirmButton.textContent = busy ? "正在移除…" : "移除";
  }

  function taskBoardSetDetachDialogFeedback(dialog, message = "") {
    if (!dialog || taskBoardState.detachDialog !== dialog) return;
    dialog.feedback.textContent = String(message || "");
  }

  function closeTaskBoardDetachDialog({ restoreFocus = true } = {}) {
    const dialog = taskBoardState.detachDialog;
    if (!dialog) return;
    taskBoardState.detachRequestId += 1;
    taskBoardState.detachBusy = false;
    window.removeEventListener("keydown", taskBoardState.detachDialogKeydownHandler, true);
    window.removeEventListener("keyup", taskBoardState.detachDialogKeydownHandler, true);
    taskBoardState.detachDialogKeydownHandler = null;
    taskBoardState.detachDialog = null;
    dialog.overlay.remove?.();
    const previousFocus = taskBoardState.detachDialogPreviousFocus;
    taskBoardState.detachDialogPreviousFocus = null;
    if (restoreFocus) {
      requestAnimationFrame(() => {
        if (previousFocus?.isConnected) {
          previousFocus.focus?.();
          return;
        }
        const card = Array.from(document.querySelectorAll?.(".codex-task-board-card") || [])
          .find((node) => node.getAttribute?.("data-task-board-id") === dialog.taskId);
        const matchingRemoveButton = Array.from(
          card?.querySelectorAll?.(".codex-task-board-conversation-remove") || [],
        ).find(
          (button) =>
            button.getAttribute?.("data-task-board-session-id") === dialog.sessionId,
        );
        (
          matchingRemoveButton ||
          card?.querySelector?.(
            ".codex-task-board-conversation, .codex-task-board-card-add, .codex-task-board-card-move",
          )
        )?.focus?.();
      });
    }
  }

  function taskBoardDetachDialogKeydown(event) {
    const dialog = taskBoardState.detachDialog;
    if (!dialog) return;
    if (event.key === "Escape") {
      if (taskBoardState.detachBusy) return;
      event.preventDefault?.();
      event.stopImmediatePropagation?.();
      closeTaskBoardDetachDialog();
      return;
    }
    if (event.type === "keyup") return;
    if (event.key !== "Tab") return;
    const focusable = taskBoardDetachDialogFocusable(dialog);
    if (!focusable.length) return;
    const current = focusable.indexOf(document.activeElement);
    const next = event.shiftKey
      ? (current <= 0 ? focusable.length - 1 : current - 1)
      : (current < 0 || current === focusable.length - 1 ? 0 : current + 1);
    event.preventDefault?.();
    focusable[next]?.focus?.();
  }

  function taskBoardRestoreAfterDetach(taskId) {
    requestAnimationFrame(() => {
      const card = Array.from(document.querySelectorAll?.(".codex-task-board-card") || [])
        .find((node) => node.getAttribute?.("data-task-board-id") === taskId);
      card?.querySelector?.(".codex-task-board-card-add, .codex-task-board-card-move")?.focus?.();
    });
  }

  async function taskBoardDetachConversation(dialog) {
    if (!dialog || taskBoardState.detachBusy) return { status: "blocked" };
    const requestId = ++taskBoardState.detachRequestId;
    taskBoardSetDetachDialogFeedback(dialog, "");
    taskBoardSetDetachDialogBusy(dialog, true);
    let expectedRevision = taskBoardState.snapshot.revision;
    try {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        let result;
        try {
          result = await taskBoardMockOrBridgeResult(
            "detachConversations",
            taskBoardDetachPayload(
              dialog.taskId,
              expectedRevision,
              [dialog.sessionId],
            ),
          );
        } catch (error) {
          result = {
            status: "failed",
            code: "bridge_unavailable",
            message: taskBoardMessageFromResult(error, ""),
          };
        }
        if (!taskBoardDetachRequestIsCurrent(dialog, requestId)) {
          return { status: "stale" };
        }
        const snapshot = taskBoardSnapshotResult(result);
        if (result?.status === "ok" && snapshot) {
          taskBoardState.snapshot = snapshot;
          taskBoardState.snapshotError = "";
          const taskId = dialog.taskId;
          closeTaskBoardDetachDialog({ restoreFocus: false });
          renderTaskBoard();
          void refreshTaskBoardConversationStatuses();
          showToast("已从任务中移除会话");
          taskBoardRestoreAfterDetach(taskId);
          return { status: "ok" };
        }
        if (result?.status === "conflict" || result?.code === "revision_conflict") {
          const conflictSnapshot = taskBoardConflictSnapshotResult(result);
          if (conflictSnapshot) {
            taskBoardState.snapshot = conflictSnapshot;
            taskBoardState.snapshotError = "";
          }
          if (attempt === 0 && conflictSnapshot) {
            expectedRevision = conflictSnapshot.revision;
            continue;
          }
        }
        taskBoardSetDetachDialogFeedback(dialog, taskBoardDetachFailureMessage(result));
        return { status: "failed" };
      }
      return { status: "failed" };
    } finally {
      if (taskBoardDetachRequestIsCurrent(dialog, requestId)) {
        taskBoardSetDetachDialogBusy(dialog, false);
      }
    }
  }

  function openTaskBoardDetachDialog(task, conversation, trigger) {
    closeTaskBoardDetachDialog({ restoreFocus: false });
    const taskId = String(task?.id || "");
    const sessionId = String(conversation?.sessionId || "").trim();
    if (!taskId || !sessionId) {
      showToast("缺少任务或会话标识，无法移除关联");
      return null;
    }
    const taskLabel = taskBoardDetachDialogLabel(
      task?.title || "未命名任务",
      60,
    );
    const overlay = taskBoardElement(
      "div",
      "codex-delete-confirm-overlay",
    );
    const content = taskBoardElement("div", "codex-delete-confirm-content");
    const titleId = "codex-task-board-detach-title";
    const messageId = "codex-task-board-detach-message";
    content.setAttribute("role", "dialog");
    content.setAttribute("aria-modal", "true");
    content.setAttribute("aria-labelledby", titleId);
    content.setAttribute("aria-describedby", messageId);
    const title = taskBoardElement(
      "div",
      "codex-delete-confirm-title",
      "移除关联会话？",
    );
    title.id = titleId;
    const message = taskBoardElement(
      "div",
      "codex-delete-confirm-message",
      `仅解除与任务“${taskLabel}”的关联，不会删除 Codex 中的原始会话。`,
    );
    message.id = messageId;
    const feedback = taskBoardElement("div", "codex-task-board-detach-feedback");
    feedback.setAttribute("role", "alert");
    feedback.setAttribute("aria-live", "polite");
    const actions = taskBoardElement("div", "codex-delete-confirm-actions");
    const cancelButton = taskBoardElement("button", "", "取消");
    cancelButton.type = "button";
    cancelButton.setAttribute("data-codex-task-board-detach-cancel", "true");
    const confirmButton = taskBoardElement("button", "", "移除");
    confirmButton.type = "button";
    confirmButton.setAttribute("data-codex-delete-confirm", "true");
    confirmButton.setAttribute("data-codex-task-board-detach-confirm", "true");
    actions.append(cancelButton, confirmButton);
    content.append(title, message, feedback, actions);
    overlay.appendChild(content);
    const dialog = {
      overlay,
      content,
      cancelButton,
      confirmButton,
      feedback,
      title,
      message,
      taskId,
      sessionId,
    };
    taskBoardState.detachDialog = dialog;
    taskBoardState.detachDialogPreviousFocus = trigger || document.activeElement;
    taskBoardState.detachDialogKeydownHandler = taskBoardDetachDialogKeydown;
    overlay.addEventListener("click", (event) => {
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (
        !taskBoardState.detachBusy &&
        (event.target === overlay ||
          target?.closest?.("[data-codex-task-board-detach-cancel]"))
      ) {
        event.preventDefault?.();
        event.stopPropagation?.();
        closeTaskBoardDetachDialog();
        return;
      }
      if (target?.closest?.("[data-codex-task-board-detach-confirm]")) {
        event.preventDefault?.();
        event.stopPropagation?.();
        void taskBoardDetachConversation(dialog);
      }
    }, true);
    window.addEventListener("keydown", taskBoardState.detachDialogKeydownHandler, true);
    window.addEventListener("keyup", taskBoardState.detachDialogKeydownHandler, true);
    document.body.appendChild(overlay);
    requestAnimationFrame(() => cancelButton.focus?.());
    return dialog;
  }

  function taskBoardFullColumnTasks(status, snapshot = taskBoardState.snapshot) {
    return (snapshot?.tasks || [])
      .filter((task) => taskBoardStatusId(task?.status) === status)
      .slice()
      .sort((left, right) => Number(left?.order || 0) - Number(right?.order || 0));
  }

  function taskBoardMoveTargetIndex(taskId, toStatus, beforeTaskId = "") {
    const source = taskBoardState.snapshot.tasks.find((task) => String(task?.id || "") === String(taskId || ""));
    if (beforeTaskId === taskId && taskBoardStatusId(source?.status) === toStatus) {
      const sourceIndex = taskBoardFullColumnTasks(toStatus).findIndex((task) => String(task?.id || "") === String(taskId || ""));
      return Math.max(0, sourceIndex);
    }
    const target = taskBoardFullColumnTasks(toStatus)
      .filter((task) => String(task?.id || "") !== String(taskId || ""));
    const beforeIndex = target.findIndex((task) => String(task?.id || "") === String(beforeTaskId || ""));
    return beforeIndex >= 0 ? beforeIndex : target.length;
  }

  function taskBoardOptimisticMoveSnapshot(snapshot, taskId, toStatus, targetIndex) {
    const tasks = (snapshot?.tasks || []).map((task) => ({
      ...task,
      project: { ...task.project },
    }));
    const source = tasks.find((task) => String(task?.id || "") === String(taskId || ""));
    if (!source) return null;
    source.status = toStatus;
    const tasksByStatus = new Map(
      taskBoardStatusDefinitions.map((status) => [status.id, []]),
    );
    tasks.forEach((task) => {
      tasksByStatus.get(taskBoardStatusId(task?.status))?.push(task);
    });
    tasksByStatus.forEach((statusTasks) => {
      statusTasks.sort(
        (left, right) => Number(left?.order || 0) - Number(right?.order || 0),
      );
    });
    const target = tasksByStatus.get(toStatus) || [];
    const sourceIndex = target.indexOf(source);
    if (sourceIndex >= 0) target.splice(sourceIndex, 1);
    const insertionIndex = Math.max(
      0,
      Math.min(Math.trunc(Number(targetIndex) || 0), target.length),
    );
    target.splice(insertionIndex, 0, source);
    tasksByStatus.forEach((statusTasks) => {
      statusTasks.forEach((task, index) => {
        task.order = index;
      });
    });
    return {
      ...snapshot,
      tasks,
    };
  }

  function closeTaskBoardDropdownMenu({ restoreFocus = true } = {}) {
    const menu = taskBoardState.dropdownMenu;
    if (!menu) return;
    taskBoardCloseCreateSettingsSubmenu(menu);
    document.removeEventListener("keydown", taskBoardState.dropdownMenuKeydownHandler, true);
    document.removeEventListener("pointerdown", taskBoardState.dropdownMenuDismissHandler, true);
    window.removeEventListener("resize", taskBoardState.dropdownMenuViewportHandler);
    window.removeEventListener("scroll", taskBoardState.dropdownMenuViewportHandler, true);
    taskBoardState.dropdownMenuKeydownHandler = null;
    taskBoardState.dropdownMenuDismissHandler = null;
    taskBoardState.dropdownMenuViewportHandler = null;
    taskBoardState.dropdownMenu = null;
    taskBoardState.projectMenu = null;
    taskBoardState.statusMenu = null;
    menu.trigger?.setAttribute?.("aria-expanded", "false");
    menu.element.remove?.();
    const previousFocus = taskBoardState.dropdownMenuPreviousFocus;
    taskBoardState.dropdownMenuPreviousFocus = null;
    if (restoreFocus && previousFocus?.isConnected !== false) previousFocus?.focus?.();
  }

  function taskBoardDropdownLeft(triggerLeft, menuWidth, viewportWidth) {
    const viewportRight = Math.max(8, viewportWidth - menuWidth - 8);
    return Math.max(8, Math.min(viewportRight, triggerLeft));
  }

  function taskBoardPositionDropdownMenu(
    menu,
    trigger,
    {
      minWidth = 180,
      matchTriggerWidth = false,
      fixedWidth = 0,
      placement = "auto",
    } = {},
  ) {
    const triggerRect = trigger?.getBoundingClientRect?.() || {
      left: 8,
      right: 188,
      top: 8,
      bottom: 44,
      width: 180,
    };
    const viewportWidth = Number(window.innerWidth || 1024);
    const viewportHeight = Number(window.innerHeight || 768);
    const triggerWidth = Math.max(0, Math.round(Number(triggerRect.width || 0)));
    const requestedFixedWidth = Math.max(0, Math.round(Number(fixedWidth || 0)));
    const constrainedFixedWidth = requestedFixedWidth
      ? Math.min(requestedFixedWidth, Math.max(0, viewportWidth - 16))
      : 0;
    const menuWidth = constrainedFixedWidth || Math.max(minWidth, triggerWidth);
    menu.style.minWidth = `${menuWidth}px`;
    if (constrainedFixedWidth) {
      menu.style.width = `${constrainedFixedWidth}px`;
    } else if (matchTriggerWidth && triggerWidth) {
      menu.style.width = `${triggerWidth}px`;
    }
    const menuRect = menu.getBoundingClientRect?.() || { width: menuWidth, height: 0 };
    const renderedWidth = Number(menuRect.width || menuWidth);
    const renderedHeight = Number(menuRect.height || 0);
    const gap = 6;
    const left = taskBoardDropdownLeft(
      Number(triggerRect.left || 8),
      renderedWidth,
      viewportWidth,
    );
    const fitsBelow =
      Number(triggerRect.bottom || 44) + gap + renderedHeight <= viewportHeight - 8;
    const top = placement === "top"
      ? Math.max(8, Number(triggerRect.top || 8) - gap - renderedHeight)
      : placement === "bottom" || fitsBelow
        ? Number(triggerRect.bottom || 44) + gap
        : Math.max(8, Number(triggerRect.top || 8) - gap - renderedHeight);
    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
  }

  function openTaskBoardDropdownMenu({
    kind,
    trigger,
    options,
    currentValue,
    ariaLabel,
    minWidth = 180,
    matchTriggerWidth = false,
    fixedWidth = 0,
    placement = "auto",
    surfaceClass = "",
    menuRole = "listbox",
    itemRole = "option",
    selectionAttribute = "aria-selected",
    showDescriptions = true,
    onSelect,
  }) {
    if (!trigger) return null;
    if (taskBoardState.dropdownMenu?.kind === kind && taskBoardState.dropdownMenu?.trigger === trigger) {
      closeTaskBoardDropdownMenu();
      return null;
    }
    closeTaskBoardDropdownMenu({ restoreFocus: false });
    const menu = taskBoardElement(
      "div",
      `codex-task-board-dropdown-menu codex-task-board-${kind}-menu ${surfaceClass}`.trim(),
    );
    menu.setAttribute("role", menuRole);
    menu.setAttribute("aria-label", ariaLabel);
    const normalizedOptions = Array.isArray(options) ? options : [];
    const buttons = normalizedOptions.map((option) => {
      const value = String(option?.value || "");
      const selected = value === String(currentValue || "");
      const button = taskBoardElement("button");
      button.type = "button";
      button.disabled = option?.disabled === true;
      button.setAttribute("role", itemRole);
      button.setAttribute("data-value", value);
      button.setAttribute(selectionAttribute, String(selected));
      const label = String(option?.label || "");
      const description = String(option?.description || "").trim();
      const copy = taskBoardElement("span", "codex-task-board-dropdown-option-copy");
      const titleRow = taskBoardElement(
        "span",
        "codex-task-board-dropdown-option-title-row",
      );
      const dot = taskBoardDropdownStatusDot(option?.color);
      if (dot) titleRow.appendChild(dot);
      titleRow.appendChild(taskBoardElement(
        "span",
        "codex-task-board-dropdown-option-title",
        label,
      ));
      copy.appendChild(titleRow);
      if (showDescriptions && description) {
        copy.appendChild(taskBoardElement(
          "span",
          "codex-task-board-dropdown-option-description",
          description,
        ));
      }
      button.title = description ? `${label}\n${description}` : label;
      button.append(
        copy,
        selected
          ? taskBoardDropdownCheck()
          : taskBoardElement("span", "codex-task-board-dropdown-option-marker"),
      );
      button.addEventListener("click", () => {
        if (button.disabled) return;
        closeTaskBoardDropdownMenu();
        onSelect?.(value, option);
      });
      menu.appendChild(button);
      return button;
    });
    document.body.appendChild(menu);
    trigger.setAttribute("aria-expanded", "true");
    taskBoardPositionDropdownMenu(menu, trigger, {
      minWidth,
      matchTriggerWidth,
      fixedWidth,
      placement,
    });
    const state = {
      kind,
      element: menu,
      buttons,
      trigger,
      options: normalizedOptions,
      selectionAttribute,
    };
    taskBoardState.dropdownMenu = state;
    taskBoardState.projectMenu = kind === "project" ? state : null;
    taskBoardState.statusMenu = kind === "status" ? state : null;
    taskBoardState.dropdownMenuPreviousFocus = trigger;
    taskBoardState.dropdownMenuKeydownHandler = (event) => {
      const enabledButtons = buttons.filter((button) => !button.disabled);
      const current = enabledButtons.indexOf(document.activeElement);
      if (event.key === "Escape") {
        event.preventDefault?.();
        closeTaskBoardDropdownMenu();
      } else if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Home" ||
        event.key === "End"
      ) {
        if (!enabledButtons.length) return;
        event.preventDefault?.();
        const next = event.key === "Home" ? 0 : event.key === "End" ? enabledButtons.length - 1 :
          event.key === "ArrowDown" ? (current + 1 + enabledButtons.length) % enabledButtons.length :
            (current - 1 + enabledButtons.length) % enabledButtons.length;
        enabledButtons[next]?.focus?.();
      } else if (event.key === "Enter" || event.key === " ") {
        const target = enabledButtons[current >= 0 ? current : 0];
        if (!target) return;
        event.preventDefault?.();
        target.click?.();
      }
    };
    taskBoardState.dropdownMenuDismissHandler = (event) => {
      const target = event.target;
      if (menu.contains?.(target) || trigger.contains?.(target)) return;
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    };
    taskBoardState.dropdownMenuViewportHandler = (event) => {
      if (event?.target && menu.contains?.(event.target)) return;
      closeTaskBoardDropdownMenu({ restoreFocus: false });
    };
    document.addEventListener("keydown", taskBoardState.dropdownMenuKeydownHandler, true);
    document.addEventListener("pointerdown", taskBoardState.dropdownMenuDismissHandler, true);
    window.addEventListener("resize", taskBoardState.dropdownMenuViewportHandler);
    window.addEventListener("scroll", taskBoardState.dropdownMenuViewportHandler, true);
    requestAnimationFrame(() => {
      const selected = buttons.find(
        (button) => button.getAttribute(selectionAttribute) === "true" && !button.disabled,
      );
      (selected || buttons.find((button) => !button.disabled))?.focus?.();
    });
    return state;
  }

  function closeTaskBoardProjectMenu({ restoreFocus = true } = {}) {
    if (!taskBoardState.projectMenu) return;
    closeTaskBoardDropdownMenu({ restoreFocus });
  }

  function openTaskBoardProjectMenu(trigger) {
    return openTaskBoardDropdownMenu({
      kind: "project",
      trigger,
      options: [
        { value: "", label: "全部项目" },
        ...taskBoardProjectOptions().map((project) => ({
          value: project.cwd,
          label: project.label,
          description: project.cwd,
        })),
      ],
      currentValue: taskBoardState.projectCwd,
      ariaLabel: "筛选项目",
      fixedWidth: taskBoardProjectDropdownWidth,
      onSelect: (cwd) => {
        taskBoardState.projectCwd = cwd;
        renderTaskBoardCards();
      },
    });
  }

  function closeTaskBoardStatusMenu({ restoreFocus = true } = {}) {
    if (!taskBoardState.statusMenu) return;
    closeTaskBoardDropdownMenu({ restoreFocus });
  }

  function clearTaskBoardDragVisuals() {
    taskBoardState.dragTaskId = "";
    document.querySelectorAll?.(".codex-task-board-card[data-dragging=\"true\"]")
      .forEach((card) => card.removeAttribute?.("data-dragging"));
    document.querySelectorAll?.(".codex-task-board-card-list[data-drop-active=\"true\"]")
      .forEach((list) => list.removeAttribute?.("data-drop-active"));
  }

  function cancelTaskBoardMoveInteraction({ restoreFocus = false } = {}) {
    const hadOptimisticMove = taskBoardState.moveBusy && !!taskBoardState.moveServerSnapshot;
    if (taskBoardState.moveBusy && taskBoardState.moveServerSnapshot) {
      taskBoardState.snapshot = taskBoardState.moveServerSnapshot;
    }
    taskBoardState.moveRequestId += 1;
    taskBoardState.moveBusy = false;
    taskBoardState.moveServerSnapshot = null;
    taskBoardState.moveFocusTaskId = "";
    clearTaskBoardDragVisuals();
    closeTaskBoardProjectMenu({ restoreFocus });
    closeTaskBoardStatusMenu({ restoreFocus });
    if (hadOptimisticMove) renderTaskBoard();
  }

  function taskBoardRestoreMoveButtonFocus(taskId) {
    if (!taskId) return;
    requestAnimationFrame(() => {
      const card = Array.from(document.querySelectorAll?.(".codex-task-board-card") || [])
        .find((node) => node.getAttribute?.("data-task-board-id") === taskId);
      card?.querySelector?.(".codex-task-board-card-move")?.focus?.();
    });
  }

  function taskBoardMoveFeedback(message = "") {
    taskBoardState.moveFeedback = String(message || "");
    renderTaskBoard();
  }

  function taskBoardMoveFailureMessage(result) {
    const code = String(result?.code || "").trim();
    if (code === "task_not_found") return "任务不存在，已恢复到最近快照";
    if (code === "task_board_busy") return "任务看板正忙，已恢复到最近快照";
    if (code === "task_file_invalid") return "任务文件无效，已恢复到最近快照";
    if (code === "task_board_unavailable") return "任务看板暂不可用，已恢复到最近快照";
    if (code === "bridge_unavailable") return "任务看板桥接暂不可用，已恢复到最近快照";
    return taskBoardMessageFromResult(result, "移动失败，已恢复到最近快照");
  }

  function taskBoardMoveRequestIsCurrent(requestId) {
    return taskBoardState.moveBusy && requestId === taskBoardState.moveRequestId;
  }

  async function taskBoardMoveTask(taskId, toStatus, targetIndex) {
    if (taskBoardState.moveBusy) return { status: "blocked" };
    const serverSnapshot = taskBoardState.snapshot;
    const optimistic = taskBoardOptimisticMoveSnapshot(serverSnapshot, taskId, toStatus, targetIndex);
    if (!optimistic) {
      taskBoardMoveFeedback("任务不存在");
      return { status: "failed" };
    }
    const requestId = ++taskBoardState.moveRequestId;
    taskBoardState.requestRevision += 1;
    taskBoardState.pendingReadCount = 0;
    taskBoardState.loading = false;
    taskBoardState.moveBusy = true;
    taskBoardState.moveServerSnapshot = serverSnapshot;
    taskBoardState.moveFeedback = "";
    taskBoardState.snapshot = optimistic;
    renderTaskBoard();
    const payload = {
      taskId: String(taskId || ""),
      toStatus,
      targetIndex: Math.max(0, Number(targetIndex) || 0),
      expectedRevision: serverSnapshot.revision,
    };
    try {
      let result;
      try {
        result = await taskBoardMockOrBridgeResult("moveTask", payload);
      } catch (error) {
        result = { status: "failed", code: "bridge_unavailable", message: taskBoardMessageFromResult(error, "") };
      }
      if (!taskBoardMoveRequestIsCurrent(requestId)) return { status: "stale" };
      const successSnapshot = taskBoardSnapshotResult(result);
      if (result?.status === "ok" && successSnapshot) {
        taskBoardState.snapshot = successSnapshot;
        taskBoardState.snapshotError = "";
        return { status: "ok" };
      }
      if (result?.status === "conflict") {
        const conflictSnapshot = taskBoardConflictSnapshotResult(result);
        taskBoardState.snapshot = conflictSnapshot || serverSnapshot;
        taskBoardMoveFeedback("任务已被其他更改更新，请查看最新状态后重试");
        return { status: "conflict" };
      }
      taskBoardState.snapshot = serverSnapshot;
      taskBoardMoveFeedback(taskBoardMoveFailureMessage(result?.code ? result : { ...result, code: "bridge_unavailable" }));
      return { status: "failed" };
    } finally {
      if (taskBoardMoveRequestIsCurrent(requestId)) {
        taskBoardState.moveBusy = false;
        taskBoardState.moveServerSnapshot = null;
        clearTaskBoardDragVisuals();
        closeTaskBoardStatusMenu({ restoreFocus: false });
        renderTaskBoard();
        taskBoardRestoreMoveButtonFocus(taskBoardState.moveFocusTaskId);
        taskBoardState.moveFocusTaskId = "";
      }
    }
  }

  function openTaskBoardStatusMenu(trigger, taskId) {
    if (taskBoardState.moveBusy) return;
    const task = (taskBoardState.snapshot.tasks || []).find((item) => String(item?.id || "") === taskId);
    const state = openTaskBoardDropdownMenu({
      kind: "status",
      trigger,
      options: taskBoardStatusDefinitions.map((status) => ({
        value: status.id,
        label: status.label,
        color: status.color,
      })),
      currentValue: taskBoardStatusId(task?.status),
      ariaLabel: "移动任务状态",
      minWidth: 150,
      onSelect: (statusId) => {
        const status = taskBoardStatusDefinitions.find((item) => item.id === statusId);
        if (!status) return;
        const targetIndex = taskBoardMoveTargetIndex(taskId, status.id);
        taskBoardState.moveFocusTaskId = taskId;
        void taskBoardMoveTask(taskId, status.id, targetIndex);
      },
    });
    if (state) state.taskId = taskId;
    return state;
  }

  function renderTaskBoardCards() {
    cancelScheduledTaskBoardCardsRender();
    const root = taskBoardState.root;
    const columns = root?.querySelector(".codex-task-board-columns");
    const filter = root?.querySelector(".codex-task-board-project-filter");
    if (!root || !columns || !filter) return;
    const projects = taskBoardProjectOptions();
    if (taskBoardState.projectCwd && !projects.some((project) => project.cwd === taskBoardState.projectCwd)) {
      taskBoardState.projectCwd = "";
    }
    const selectedProject = projects.find((project) => project.cwd === taskBoardState.projectCwd);
    const filterText = selectedProject?.label || "全部项目";
    taskBoardSetDropdownTriggerLabel(filter, filterText);
    const tasks = taskBoardVisibleTasks();
    const tasksByStatus = new Map(
      taskBoardStatusDefinitions.map((status) => [status.id, []]),
    );
    tasks.forEach((task) => {
      tasksByStatus.get(taskBoardStatusId(task?.status))?.push(task);
    });
    tasksByStatus.forEach((statusTasks) => {
      statusTasks.sort(
        (left, right) => Number(left?.order || 0) - Number(right?.order || 0),
      );
    });
    columns.replaceChildren();
    taskBoardStatusDefinitions.forEach((status) => {
      const statusTasks = tasksByStatus.get(status.id) || [];
      const column = taskBoardElement("section", "codex-task-board-column");
      column.style.setProperty("--task-board-status-color", status.color);
      const header = taskBoardElement("header", "codex-task-board-column-head");
      const title = taskBoardElement("div", "codex-task-board-column-title");
      title.append(taskBoardElement("span", "codex-task-board-status-dot"), document.createTextNode(status.label));
      const count = taskBoardElement("span", "codex-task-board-count", String(statusTasks.length));
      header.append(title, count);
      const list = taskBoardElement("div", "codex-task-board-card-list");
      list.setAttribute("data-task-board-status", status.id);
      list.addEventListener("dragover", (event) => {
        if (!taskBoardState.dragTaskId || taskBoardState.moveBusy) return;
        event.preventDefault?.();
        list.setAttribute("data-drop-active", "true");
      });
      list.addEventListener("dragleave", () => list.removeAttribute("data-drop-active"));
      list.addEventListener("drop", (event) => {
        event.preventDefault?.();
        const taskId = taskBoardState.dragTaskId;
        list.removeAttribute("data-drop-active");
        if (!taskId || taskBoardState.moveBusy) return;
        const target = event.target instanceof Element ? event.target : event.target?.parentElement;
        const beforeTaskId = target?.closest?.(".codex-task-board-card")?.getAttribute?.("data-task-board-id") || "";
        const sourceTask = taskBoardState.snapshot.tasks.find((task) => String(task?.id || "") === taskId);
        if (beforeTaskId === taskId && sourceTask?.status === status.id) {
          clearTaskBoardDragVisuals();
          return;
        }
        const targetIndex = taskBoardMoveTargetIndex(taskId, status.id, beforeTaskId);
        void taskBoardMoveTask(taskId, status.id, targetIndex);
      });
      if (!statusTasks.length) {
        list.appendChild(taskBoardElement("div", "codex-task-board-empty-column", "暂无任务"));
      }
      statusTasks.forEach((task) => {
        const card = taskBoardElement("article", "codex-task-board-card");
        card.setAttribute("data-task-board-id", String(task?.id || ""));
        card.draggable = !taskBoardState.moveBusy;
        card.addEventListener("dragstart", (event) => {
          if (taskBoardState.moveBusy) {
            event.preventDefault?.();
            return;
          }
          taskBoardState.dragTaskId = String(task?.id || "");
          card.setAttribute("data-dragging", "true");
          event.dataTransfer?.setData?.("text/plain", taskBoardState.dragTaskId);
        });
        card.addEventListener("dragend", () => clearTaskBoardDragVisuals());
        card.appendChild(taskBoardElement("div", "codex-task-board-project", String(task?.project?.label || displayProjectName(task?.project?.cwd || ""))));
        card.appendChild(taskBoardElement("div", "codex-task-board-card-title", String(task?.title || "未命名任务")));
        const moveButton = taskBoardElement("button", "codex-task-board-card-move");
        moveButton.type = "button";
        taskBoardConfigureDropdownTrigger(
          moveButton,
          status.label,
          `移动任务 ${String(task?.title || "未命名任务")} 的状态`,
          status.color,
        );
        moveButton.disabled = taskBoardState.moveBusy;
        moveButton.addEventListener("click", () => openTaskBoardStatusMenu(moveButton, String(task?.id || "")));
        const addConversationButton = taskBoardElement(
          "button",
          "codex-task-board-card-add",
        );
        addConversationButton.type = "button";
        addConversationButton.disabled = taskBoardState.moveBusy;
        addConversationButton.setAttribute(
          "aria-label",
          `为任务 ${String(task?.title || "未命名任务")} 添加会话`,
        );
        addConversationButton.innerHTML = `<svg aria-hidden="true" viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.35"><path d="M8 3v10M3 8h10" stroke-linecap="round"></path></svg><span>添加会话</span>`;
        addConversationButton.addEventListener(
          "click",
          () => openTaskBoardAttachModal(task),
        );
        const conversations = taskBoardElement("div", "codex-task-board-conversations");
        const linked = Array.isArray(task?.conversations) ? task.conversations : [];
        if (!linked.length) {
          conversations.appendChild(taskBoardElement("div", "codex-task-board-empty", "未关联会话"));
        } else {
          linked.forEach((conversation) => {
            conversations.appendChild(taskBoardConversationRow(task, conversation));
          });
        }
        const footer = taskBoardElement("div", "codex-task-board-card-footer");
        footer.append(addConversationButton, moveButton);
        card.append(conversations, footer);
        list.appendChild(card);
      });
      column.append(header, list);
      columns.appendChild(column);
    });
  }

  function taskBoardStatusPresentation(state = taskBoardState) {
    const failures = [
      state?.snapshotError,
      state?.catalogError,
      state?.moveFeedback,
    ].filter(Boolean);
    const warning = taskBoardCatalogPartiallyUnavailable(state?.catalog)
      ? "目录部分不可用"
      : "";
    if (state?.loading) {
      return {
        status: "loading",
        text: "正在加载任务与会话目录…",
      };
    }
    if (failures.length) {
      return {
        status: "failed",
        text: failures.join("；"),
      };
    }
    if (warning) {
      return {
        status: "warning",
        text: warning,
      };
    }
    return {
      status: "ok",
      text: "拖动任务卡片可切换状态",
    };
  }

  function renderTaskBoardStatus(root = taskBoardState.root) {
    if (!root) return;
    const hint = root.querySelector(".codex-task-board-hint");
    if (hint) {
      const presentation = taskBoardStatusPresentation();
      hint.dataset.status = presentation.status;
      hint.textContent = presentation.text;
      hint.title = presentation.text;
    }
  }

  function renderTaskBoard() {
    const root = taskBoardState.root;
    if (!root) return;
    taskBoardEnsureScaffold(root);
    const search = root.querySelector(".codex-task-board-search");
    if (search && search.value !== taskBoardState.query) search.value = taskBoardState.query;
    renderTaskBoardStatus(root);
    renderTaskBoardCards();
  }

  function taskBoardApplyReadOutcome(requestRevision, kind, outcome) {
    if (!taskBoardState.active || taskBoardState.moveBusy || requestRevision !== taskBoardState.requestRevision) return;
    const fulfilled = outcome.status === "fulfilled";
    let cardsChanged = false;
    if (kind === "snapshot") {
      const snapshot = fulfilled ? taskBoardSnapshotResult(outcome.value) : null;
      if (snapshot) {
        taskBoardState.snapshot = snapshot;
        taskBoardState.snapshotError = "";
        cardsChanged = true;
      } else {
        taskBoardState.snapshotError = fulfilled
          ? taskBoardMessageFromResult(outcome.value, "任务快照加载失败")
          : taskBoardMessageFromResult(outcome.reason, "任务快照加载失败");
      }
    } else {
      const catalog = fulfilled ? taskBoardCatalogResult(outcome.value) : null;
      if (catalog) {
        taskBoardState.catalog = catalog;
        taskBoardState.catalogError = "";
        cardsChanged = true;
        taskBoardReconcileCreateSelectedSessions();
        if (taskBoardState.createModal) renderTaskBoardCreateModal();
      } else {
        taskBoardState.catalogError = fulfilled
          ? taskBoardMessageFromResult(outcome.value, "会话目录加载失败")
          : taskBoardMessageFromResult(outcome.reason, "会话目录加载失败");
      }
    }
    taskBoardState.pendingReadCount = Math.max(0, taskBoardState.pendingReadCount - 1);
    taskBoardState.loading = taskBoardState.pendingReadCount > 0;
    if (cardsChanged) renderTaskBoard();
    else renderTaskBoardStatus();
  }

  function refreshTaskBoardData() {
    if (taskBoardState.moveBusy) return Promise.resolve([]);
    const requestRevision = ++taskBoardState.requestRevision;
    taskBoardState.pendingReadCount = 2;
    taskBoardState.loading = true;
    renderTaskBoardStatus();
    const read = (kind) => Promise.resolve(taskBoardMockOrBridgeResult(kind)).then(
      (value) => taskBoardApplyReadOutcome(requestRevision, kind, { status: "fulfilled", value }),
      (reason) => taskBoardApplyReadOutcome(requestRevision, kind, { status: "rejected", reason }),
    );
    return Promise.all([read("snapshot"), read("catalog")]).then((outcomes) => {
      void refreshTaskBoardConversationStatuses();
      return outcomes;
    });
  }

  function deactivateTaskBoard({
    restoreNativeSelection = true,
    nativeNavigationTarget = null,
  } = {}) {
    cancelTaskBoardMoveInteraction({ restoreFocus: false });
    closeTaskBoardCreateModal();
    closeTaskBoardDetachDialog({ restoreFocus: false });
    stopTaskBoardConversationStatusRefresh();
    cancelScheduledTaskBoardCardsRender();
    if (!taskBoardState.active && !taskBoardState.root) return;
    taskBoardState.active = false;
    taskBoardState.requestRevision += 1;
    taskBoardState.pendingReadCount = 0;
    taskBoardState.loading = false;
    taskBoardUnmountHost();
    taskBoardSetEntryActive(false);
    if (restoreNativeSelection) restoreTaskBoardNativeSelection();
    else restoreTaskBoardNativeSelection(nativeNavigationTarget);
  }

  function activateTaskBoard() {
    if (!taskBoardFeatureEnabled()) {
      reconcileTaskBoardRuntime();
      return;
    }
    taskBoardState.active = true;
    taskBoardState.nativeCreateRecoveryAttempted = false;
    clearTaskBoardNativeSelection();
    taskBoardSetEntryActive(true);
    if (!mountTaskBoardRoot()) {
      taskBoardState.active = false;
      taskBoardSetEntryActive(false);
      restoreTaskBoardNativeSelection();
      showToast("未找到 Codex 内容区，任务看板暂不可用");
      return;
    }
    void refreshTaskBoardData().finally(() => void taskBoardRetryNativeCreateRecovery());
  }

  function installTaskBoardEntry() {
    installTaskBoardNavigationObserver();
    const entry = reconcileTaskBoardEntry();
    if (!entry) return;
    taskBoardState.entry = entry;
    taskBoardSetEntryActive(taskBoardState.active);
    if (taskBoardState.navigationHandler) return;
    taskBoardState.navigationHandler = (event) => {
      if (!taskBoardState.active) return;
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (!target || target.closest?.(`[${taskBoardEntryAttribute}="true"]`)) return;
      const nativeNavigationTarget = taskBoardNativeNavigationControl(target);
      if (!nativeNavigationTarget) return;
      const releaseId = ++taskBoardState.navigationReleaseId;
      queueMicrotask(() => {
        if (!taskBoardState.active || releaseId !== taskBoardState.navigationReleaseId) return;
        deactivateTaskBoard({
          restoreNativeSelection: false,
          nativeNavigationTarget,
        });
      });
    };
    document.addEventListener("click", taskBoardState.navigationHandler, true);
  }

  function disableTaskBoardRuntime() {
    taskBoardState.navigationReleaseId += 1;
    closeTaskBoardEntryContextMenu({ restoreFocus: false });
    deactivateTaskBoard({ restoreNativeSelection: true });
    document.removeEventListener("click", taskBoardState.navigationHandler, true);
    taskBoardState.navigationHandler = null;
    taskBoardState.navigationObserver?.disconnect?.();
    taskBoardState.navigationObserver = null;
    taskBoardState.navigationObserverRoot = null;
    taskBoardEntryButtons().forEach((entry) => entry.remove());
    taskBoardState.entry = null;
    taskBoardState.entryTemplateSignature = "";
  }

  function reconcileTaskBoardRuntime() {
    if (!taskBoardFeatureEnabled()) {
      disableTaskBoardRuntime();
      return;
    }
    installTaskBoardEntry();
    if (!taskBoardState.active) return;
    clearTaskBoardNativeSelection();
    if (!mountTaskBoardRoot()) return;
    taskBoardSetEntryActive(true);
  }

  function refreshTaskBoardRuntime() {
    const preserveNativeCreate =
      taskBoardFeatureEnabled() &&
      (!!taskBoardState.nativeCreateOperation || !!taskBoardNativeOperationLease());
    if (!preserveNativeCreate) taskBoardCancelNativeCreateOperation();
    cancelTaskBoardMoveInteraction({ restoreFocus: false });
    closeTaskBoardEntryContextMenu({ restoreFocus: false });
    closeTaskBoardCreateModal();
    closeTaskBoardDetachDialog({ restoreFocus: false });
    reconcileTaskBoardRuntime();
  }

  function destroyTaskBoardRuntime({ preserveNativeCreate = false } = {}) {
    if (!preserveNativeCreate) taskBoardCancelNativeCreateOperation();
    disableTaskBoardRuntime();
  }

  window.__codexElvesTaskBoardCleanup = destroyTaskBoardRuntime;
  window.__codexElvesTaskBoardRefreshRuntime = refreshTaskBoardRuntime;

  function resetTaskBoardReadStateForTests() {
    cancelTaskBoardMoveInteraction({ restoreFocus: false });
    closeTaskBoardCreateModal({ restoreFocus: false });
    closeTaskBoardDetachDialog({ restoreFocus: false });
    stopTaskBoardConversationStatusRefresh();
    taskBoardState.active = true;
    taskBoardState.root = null;
    taskBoardState.host = null;
    taskBoardState.requestRevision += 1;
    taskBoardState.snapshot = { schemaVersion: 1, revision: 0, tasks: [] };
    taskBoardState.catalog = { projects: [], sessions: [], warnings: [] };
    taskBoardState.snapshotError = "";
    taskBoardState.catalogError = "";
    taskBoardState.loading = false;
    taskBoardState.pendingReadCount = 0;
    taskBoardState.conversationStatuses = new Map();
  }

  function resetTaskBoardCreateStateForTests(options = {}) {
    taskBoardCancelNativeCreateOperation();
    cancelTaskBoardMoveInteraction({ restoreFocus: false });
    closeTaskBoardCreateModal({ restoreFocus: false });
    closeTaskBoardDetachDialog({ restoreFocus: false });
    stopTaskBoardConversationStatusRefresh();
    taskBoardState.active = true;
    taskBoardState.root = null;
    taskBoardState.host = null;
    taskBoardState.requestRevision += 1;
    taskBoardState.snapshot = taskBoardSnapshotResult(options.snapshot) || { schemaVersion: 1, revision: 0, tasks: [] };
    taskBoardState.catalog = taskBoardCatalogResult(options.catalog) || { projects: [], sessions: [], warnings: [] };
    taskBoardState.snapshotError = "";
    taskBoardState.catalogError = String(options.catalogError || "");
    taskBoardState.loading = false;
    taskBoardState.pendingReadCount = 0;
    taskBoardState.conversationStatuses = new Map();
    taskBoardState.nativeCreateRecoveryAttempted = false;
  }

  function resetTaskBoardMoveStateForTests(snapshot) {
    cancelTaskBoardMoveInteraction({ restoreFocus: false });
    closeTaskBoardDetachDialog({ restoreFocus: false });
    stopTaskBoardConversationStatusRefresh();
    taskBoardState.active = true;
    taskBoardState.root = null;
    taskBoardState.host = null;
    taskBoardState.requestRevision += 1;
    taskBoardState.snapshot = taskBoardSnapshotResult(snapshot) || { schemaVersion: 1, revision: 0, tasks: [] };
    taskBoardState.snapshotError = "";
    taskBoardState.catalogError = "";
    taskBoardState.moveFeedback = "";
    taskBoardState.conversationStatuses = new Map();
  }

  if (window.__CODEX_ELVES_TEST_TASK_BOARD__ === true) {
    window.__codexElvesTaskBoardTest = {
      runtimeVersion: () => taskBoardRuntimeVersion,
      runtimeCanRefresh: (version, refresh) => taskBoardRuntimeCanRefresh(version, refresh),
      taskBoardFeatureEnabledForTest: taskBoardFeatureEnabled,
      setBackendSettingsForTest: (settings = {}) => {
        codexElvesBackendSettings = { ...codexElvesBackendSettings, ...settings };
        codexElvesBackendSettingsLoaded = true;
        invalidateCodexElvesSettingsCache();
        return codexElvesSettings();
      },
      pluginEntryButtonForTest: pluginEntryButton,
      taskBoardPluginEntryButtonForTest: taskBoardPluginEntryButton,
      createEntryForTest: taskBoardCreateEntry,
      openEntryContextMenuForTest: openTaskBoardEntryContextMenu,
      closeEntryContextMenuForTest: closeTaskBoardEntryContextMenu,
      entryContextMenuStateForTest: () => ({
        open: !!taskBoardState.entryContextMenu,
        role: taskBoardState.entryContextMenu?.element?.getAttribute?.("role") || "",
        label: taskBoardState.entryContextMenu?.openWindow?.textContent || "",
        itemRole:
          taskBoardState.entryContextMenu?.openWindow?.getAttribute?.("role") || "",
        focused:
          document.activeElement === taskBoardState.entryContextMenu?.openWindow,
      }),
      conversationProjection: (conversation, catalog) =>
        taskBoardConversationProjectionForCatalog(conversation, catalog),
      conversationStatusForTest: taskBoardConversationStatus,
      refreshConversationStatusesForTest: (options = {}) =>
        refreshTaskBoardConversationStatuses({ schedule: false, ...options }),
      taskMatchesQuery: (task, catalog, query) => taskBoardTaskMatchesQuery(task, query, catalog),
      statusPresentationForTest: (overrides = {}) => taskBoardStatusPresentation({
        ...taskBoardState,
        ...overrides,
        catalog: overrides.catalog || taskBoardState.catalog,
      }),
      resetReadState: resetTaskBoardReadStateForTests,
      refresh: refreshTaskBoardData,
      openDetachDialogForTest: (task, conversation, trigger = null) =>
        openTaskBoardDetachDialog(task, conversation, trigger),
      detachDialogStateForTest: () => ({
        open: !!taskBoardState.detachDialog,
        busy: taskBoardState.detachBusy,
        feedback: taskBoardState.detachDialog?.feedback?.textContent || "",
      }),
      detachDialogContractForTest: () => ({
        role: taskBoardState.detachDialog?.content?.getAttribute?.("role") || "",
        ariaModal:
          taskBoardState.detachDialog?.content?.getAttribute?.("aria-modal") === "true",
        title: taskBoardState.detachDialog?.title?.textContent || "",
        message: taskBoardState.detachDialog?.message?.textContent || "",
        initialFocus:
          document.activeElement === taskBoardState.detachDialog?.cancelButton,
      }),
      submitDetachForTest: () => taskBoardDetachConversation(taskBoardState.detachDialog),
      closeDetachForTest: () => closeTaskBoardDetachDialog(),
      refreshRuntimeForTest: refreshTaskBoardRuntime,
      reconcileRuntimeForTest: reconcileTaskBoardRuntime,
      activeForTest: () => taskBoardState.active,
      resetCreateStateForTest: resetTaskBoardCreateStateForTests,
      resetMoveStateForTest: resetTaskBoardMoveStateForTests,
      setMoveFiltersForTest: (query = "", projectCwd = "") => {
        taskBoardState.query = String(query || "");
        taskBoardState.projectCwd = taskBoardNormalizedCwd(projectCwd);
      },
      moveTargetIndexForTest: taskBoardMoveTargetIndex,
      moveTaskForTest: taskBoardMoveTask,
      dragEndForTest: clearTaskBoardDragVisuals,
      openStatusMenuForTest: (taskId, rect = {}) => {
        const trigger = document.createElement("button");
        const left = Number(rect.left ?? 40);
        const top = Number(rect.top ?? 40);
        const width = Number(rect.width ?? 92);
        const height = Number(rect.height ?? 29);
        trigger.getBoundingClientRect = () => ({
          left,
          right: left + width,
          top,
          bottom: top + height,
          width,
          height,
        });
        return openTaskBoardStatusMenu(trigger, taskId);
      },
      openBoardProjectMenuForTest: (rect = {}) => {
        const trigger = document.createElement("button");
        const left = Number(rect.left ?? 40);
        const top = Number(rect.top ?? 40);
        const width = Number(rect.width ?? 132);
        const height = Number(rect.height ?? 36);
        trigger.getBoundingClientRect = () => ({
          left,
          right: left + width,
          top,
          bottom: top + height,
          width,
          height,
        });
        return openTaskBoardProjectMenu(trigger);
      },
      statusMenuStateForTest: () => ({
        open: !!taskBoardState.statusMenu,
        itemCount: taskBoardState.statusMenu?.buttons?.length || 0,
        focusedIndex: taskBoardState.statusMenu?.buttons?.indexOf(document.activeElement) ?? -1,
      }),
      dispatchStatusMenuKeyForTest: (key) => {
        const event = { type: "keydown", key, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
        document.dispatchEvent?.(event);
        return event.defaultPrevented;
      },
      moveStateForTest: () => ({
        busy: taskBoardState.moveBusy,
        feedback: taskBoardState.moveFeedback,
        revision: taskBoardState.snapshot.revision,
        tasks: taskBoardState.snapshot.tasks,
        menuOpen: !!taskBoardState.statusMenu,
      }),
      openCreateModalForTest: openTaskBoardCreateModal,
      openAttachModalForTest: openTaskBoardAttachModal,
      openCreateDropdownForTest: (kind) => {
        const modal = taskBoardState.createModal || openTaskBoardCreateModal();
        if (kind === "project") return openTaskBoardCreateProjectMenu(modal.projectSelect);
        if (kind === "status") return openTaskBoardCreateStatusMenu(modal.statusSelect);
        return null;
      },
      openCreateSettingsMenuForTest: () => {
        const modal = taskBoardState.createModal || openTaskBoardCreateModal();
        return openTaskBoardCreateSettingsMenu(modal.modelTrigger);
      },
      openCreateModelMenuForTest: () => {
        const modal = taskBoardState.createModal || openTaskBoardCreateModal();
        const state = openTaskBoardCreateSettingsMenu(modal.modelTrigger);
        if (!state) return null;
        taskBoardOpenCreateSettingsSubmenu(state, "model");
        return state;
      },
      openCreateEffortMenuForTest: () => {
        const modal = taskBoardState.createModal || openTaskBoardCreateModal();
        const state = openTaskBoardCreateSettingsMenu(modal.modelTrigger);
        if (!state) return null;
        taskBoardOpenCreateSettingsSubmenu(state, "effort");
        return state;
      },
      dropdownMenuStateForTest: () => ({
        open: !!taskBoardState.dropdownMenu,
        kind: taskBoardState.dropdownMenu?.kind || "",
        role: taskBoardState.dropdownMenu?.element?.getAttribute?.("role") || "",
        width: taskBoardState.dropdownMenu?.element?.style?.width || "",
        minWidth: taskBoardState.dropdownMenu?.element?.style?.minWidth || "",
        left: taskBoardState.dropdownMenu?.element?.style?.left || "",
        top: taskBoardState.dropdownMenu?.element?.style?.top || "",
        itemCount: taskBoardState.dropdownMenu?.buttons?.length || 0,
        focusedIndex: taskBoardState.dropdownMenu?.buttons?.indexOf(document.activeElement) ?? -1,
        buttonTexts: Array.from(taskBoardState.dropdownMenu?.buttons || [])
          .map((button) => String(
            button?.getAttribute?.("aria-label") ||
            button?.textContent ||
            "",
          ).replace(/\s+/g, " ").trim()),
        selectedIndex: taskBoardState.dropdownMenu?.buttons?.findIndex?.(
          (button) => {
            const selectionAttribute = taskBoardState.dropdownMenu?.selectionAttribute || "aria-selected";
            return button.getAttribute?.(selectionAttribute) === "true";
          },
        ) ?? -1,
        submenuOpen: !!taskBoardState.dropdownMenu?.submenuElement,
        submenuKind: taskBoardState.dropdownMenu?.submenuKind || "",
        submenuRole: taskBoardState.dropdownMenu?.submenuElement?.getAttribute?.("role") || "",
        submenuLeft: taskBoardState.dropdownMenu?.submenuElement?.style?.left || "",
        submenuTop: taskBoardState.dropdownMenu?.submenuElement?.style?.top || "",
        submenuItemCount: taskBoardState.dropdownMenu?.submenuButtons?.length || 0,
        submenuFocusedIndex:
          taskBoardState.dropdownMenu?.submenuButtons?.indexOf(document.activeElement) ?? -1,
        submenuSelectedIndex: taskBoardState.dropdownMenu?.submenuButtons?.findIndex?.(
          (button) => button.getAttribute?.("aria-checked") === "true",
        ) ?? -1,
        submenuTexts: Array.from(taskBoardState.dropdownMenu?.submenuButtons || [])
          .map((button) => String(
            button?.getAttribute?.("aria-label") ||
            button?.textContent ||
            "",
          ).replace(/\s+/g, " ").trim()),
        optionDescriptions: Array.from(taskBoardState.dropdownMenu?.options || [])
          .map((option) => String(option?.description || "")),
        triggerExpanded: taskBoardState.dropdownMenu?.trigger?.getAttribute?.("aria-expanded") || "false",
      }),
      dispatchDropdownMenuKeyForTest: (key, shiftKey = false) => {
        const event = {
          type: "keydown",
          key,
          shiftKey,
          defaultPrevented: false,
          preventDefault() { this.defaultPrevented = true; },
          stopImmediatePropagation() { this.immediatePropagationStopped = true; },
        };
        document.dispatchEvent?.(event);
        return event.defaultPrevented;
      },
      setCreateDraftForTest: (draft = {}) => {
        const modal = taskBoardState.createModal || openTaskBoardCreateModal();
        const semanticChange =
          (["existing", "new"].includes(draft.mode) && modal.mode !== draft.mode) ||
          (Object.prototype.hasOwnProperty.call(draft, "title") && modal.title !== String(draft.title || "")) ||
          (Object.prototype.hasOwnProperty.call(draft, "projectCwd") &&
            modal.projectCwd !== taskBoardNormalizedCwd(draft.projectCwd)) ||
          (Object.prototype.hasOwnProperty.call(draft, "initialStatus") &&
            modal.initialStatus !== taskBoardStatusId(draft.initialStatus)) ||
          Array.isArray(draft.sessionIds);
        if (["existing", "new"].includes(draft.mode)) modal.mode = draft.mode;
        if (Object.prototype.hasOwnProperty.call(draft, "title")) modal.title = String(draft.title || "");
        if (Object.prototype.hasOwnProperty.call(draft, "projectCwd")) {
          modal.projectCwd = taskBoardNormalizedCwd(draft.projectCwd);
        }
        if (Object.prototype.hasOwnProperty.call(draft, "initialStatus")) {
          modal.initialStatus = taskBoardStatusId(draft.initialStatus);
        }
        if (Object.prototype.hasOwnProperty.call(draft, "modelId")) {
          modal.modelId = String(draft.modelId || "").trim();
          modal.modelSelectionTouched = true;
        }
        if (Object.prototype.hasOwnProperty.call(draft, "effortId")) {
          modal.effortId = String(draft.effortId || "").trim().toLowerCase();
        }
        if (Array.isArray(draft.sessionIds)) {
          modal.selectedSessionIds = new Set(draft.sessionIds.map((value) => String(value || "").trim()).filter(Boolean));
        }
        if (Object.prototype.hasOwnProperty.call(draft, "firstInstruction")) {
          modal.firstInstruction = String(draft.firstInstruction || "");
        }
        if (semanticChange) taskBoardClearCreateIdentity(modal);
        taskBoardReconcileCreateSelectedSessions(modal);
        taskBoardReconcileCreateEffort(modal);
        modal.feedback = "";
        renderTaskBoardCreateModal();
      },
      setCreateProjectForTest: taskBoardSetCreateProject,
      setCreateSessionsForTest: taskBoardSetCreateSessions,
      setModelCatalogForTest: (catalog = {}) => {
        codexModelCatalog = {
          status: "ok",
          model: "",
          default_model: "",
          model_provider: "",
          provider_name: "",
          models: [],
          model_entries: [],
          sources: [],
          responses_api: { status: "unknown", message: "" },
          ...catalog,
        };
        codexModelCatalogLoadedAt = Date.now();
        codexModelCatalogPromise = null;
        if (taskBoardState.createModal) {
          taskBoardReconcileCreateEffort(taskBoardState.createModal);
          renderTaskBoardCreateModal();
        }
        return taskBoardCreateModelOptions();
      },
      applyCatalogForTest: (catalog) => {
        taskBoardApplyReadOutcome(taskBoardState.requestRevision, "catalog", {
          status: "fulfilled",
          value: catalog,
        });
      },
      toolbarLayoutForTest: taskBoardToolbarLayout,
      setCreateBusyForTest: (busy) => {
        const modal = taskBoardState.createModal;
        if (modal) taskBoardSetCreateModalBusy(modal, busy);
      },
      hydrateCreateSessionsForTest: () =>
        taskBoardHydrateCreateModalSessions(taskBoardState.createModal),
      submitCreateForTest: submitTaskBoardCreate,
      nativeProbeForTest: taskBoardNativeProbe,
      nativeStartForTest: taskBoardNativeStartConversation,
      nativeOpenSessionForTest: taskBoardNativeOpenSession,
      openConversationForTest: openTaskBoardConversation,
      retryNativeCreateRecoveryForTest: taskBoardRetryNativeCreateRecovery,
      nativeRecoveryForTest: taskBoardReadNativeCreateRecovery,
      nativeCreateLeaseStateForTest: () => ({
        lease: taskBoardNativeOperationLease(),
        foreignLease: taskBoardForeignNativeCreateLease(),
        operationId: String(
          taskBoardState.nativeCreateOperation?.operationId || "",
        ),
        recoveryAttempted: taskBoardState.nativeCreateRecoveryAttempted,
      }),
      createModalContractForTest: () => {
        const modal = taskBoardState.createModal;
        return {
          node: modal?.backdrop || null,
          dialog: modal?.dialog || null,
          titleInput: modal?.titleInput || null,
          projectSelect: modal?.projectSelect || null,
          statusSelect: modal?.statusSelect || null,
          modelTrigger: modal?.modelTrigger || null,
          firstInstructionComposer: modal?.firstInstructionComposer || null,
          firstInstructionInput: modal?.firstInstructionInput || null,
          closeButton: modal?.closeButton || null,
          cancelButton: modal?.cancelButton || null,
          existingButton: modal?.existingButton || null,
          newButton: modal?.newButton || null,
          role: modal?.dialog?.getAttribute?.("role") || "",
          ariaModal: modal?.dialog?.getAttribute?.("aria-modal") === "true",
          initialFocus: document.activeElement === modal?.titleInput,
          keydownListeners: document.listenerCount?.("keydown") || 0,
          bodyMounted: modal?.backdrop?.parentElement === document.body,
          outsideMain: modal?.backdrop?.parentElement !== taskBoardState.host,
        };
      },
      dispatchCreateModalKeyForTest: (key, shiftKey = false) => {
        const event = {
          type: "keydown",
          key,
          shiftKey,
          defaultPrevented: false,
          preventDefault() { this.defaultPrevented = true; },
        };
        document.dispatchEvent?.(event);
        return event.defaultPrevented;
      },
      clickCreateModalControlForTest: (control) => {
        const modal = taskBoardState.createModal;
        const target = control === "backdrop" ? modal?.backdrop : modal?.[control];
        if (control === "backdrop") {
          target?.dispatchEvent?.({ type: "pointerdown", target });
          target?.dispatchEvent?.({ type: "pointerup", target });
        }
        target?.dispatchEvent?.({ type: "click", target, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } });
      },
      releaseCreateModalOnBackdropForTest: () => {
        const modal = taskBoardState.createModal;
        modal?.dialog?.dispatchEvent?.({ type: "pointerdown", target: modal.dialog });
        modal?.backdrop?.dispatchEvent?.({ type: "pointerup", target: modal.backdrop });
        modal?.backdrop?.dispatchEvent?.({
          type: "click",
          target: modal.backdrop,
          defaultPrevented: false,
          preventDefault() { this.defaultPrevented = true; },
        });
      },
      focusCreateModalControlForTest: (control) => {
        taskBoardState.createModal?.[control]?.focus?.();
      },
      activeCreateModalControlForTest: () => {
        const modal = taskBoardState.createModal;
        return ["closeButton", "existingButton", "newButton", "titleInput", "projectSelect", "statusSelect", "firstInstructionInput", "modelTrigger", "cancelButton", "submitButton"]
          .find((control) => document.activeElement === modal?.[control]) || "";
      },
      createModalFocusableControlsForTest: () => {
        const modal = taskBoardState.createModal;
        return taskBoardCreateModalFocusableElements(modal).map((element) => {
          return ["closeButton", "existingButton", "newButton", "titleInput", "projectSelect", "statusSelect", "firstInstructionInput", "modelTrigger", "cancelButton", "submitButton"]
            .find((control) => element === modal?.[control]) || element?.tagName || "";
        });
      },
      createModalStateForTest: () => {
        const modal = taskBoardState.createModal;
        return {
          open: !!modal,
          purpose: modal?.purpose || "",
          busy: !!modal?.busy,
          mode: modal?.mode || "",
          title: modal?.title || "",
          projectCwd: modal?.projectCwd || "",
          initialStatus: modal?.initialStatus || "new",
          modelId: modal?.modelId || "",
          modelOptionIds: taskBoardCreateModelOptions().map((option) => option.value),
          effortId: modal?.effortId || "",
          effortOptionIds: taskBoardCreateEffortOptions(modal?.modelId || "")
            .map((option) => option.value),
          nativeCreateAvailable: modal?.nativeCreateAvailable ?? null,
          nativeCreateCode: modal?.nativeCreateCode || "",
          nativeCreateMessage: modal?.nativeCreateMessage || "",
          selectedSessionIds: Array.from(modal?.selectedSessionIds || []),
          sessionsHydrated: !!modal?.sessionsHydrated,
          sessionRenderState:
            modal?.sessionField?.dataset?.taskBoardRenderState || "",
          sessionInputCount:
            modal?.sessionField?.querySelectorAll?.("input")?.length || 0,
          availableSessionIds: modal
            ? taskBoardCreateSessionsForProject(
              modal.projectCwd,
              modal.attachedSessionIds,
            ).map((session) => session.sessionId)
            : [],
          projectOptionCwds: modal ? taskBoardCreateModalProjects().map((project) => project.cwd) : [],
          feedback: modal?.feedback || "",
        };
      },
      createSnapshotForTest: () => taskBoardState.snapshot,
      readState: () => ({
        snapshot: taskBoardState.snapshot,
        catalog: taskBoardState.catalog,
        loading: taskBoardState.loading,
        requestRevision: taskBoardState.requestRevision,
      }),
    };
  }

  function serializableProjectTarget(target) {
    return { kind: target.kind, label: target.label, description: target.description, path: target.path, normalizedPath: target.normalizedPath || normalizeWorkspacePath(target.path) };
  }

  function projectMoveTargets() {
    return [
      { kind: "projectless", label: "普通对话", description: "不属于任何项目", path: "", normalizedPath: "" },
      ...nativeProjectTargets().map(serializableProjectTarget),
    ];
  }

  function readLegacyProjectMoveProjection() {
    try {
      const parsed = JSON.parse(localStorage.getItem(legacyProjectMoveOverridesKey) || "{}");
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
      const now = Date.now();
      const next = {};
      for (const [key, value] of Object.entries(parsed)) {
        if (!value || typeof value !== "object" || !value.targetCwd) continue;
        const sessionId = projectMoveSessionKey(value.sessionId || key);
        if (!sessionId) continue;
        next[sessionId] = {
          sessionId,
          targetKind: "project",
          targetCwd: String(value.targetCwd),
          targetLabel: String(value.targetLabel || displayProjectName(value.targetCwd)),
          title: String(value.title || ""),
          sortMs: sortMsForSession(sessionId, value.sortMs || value.updatedAtMs || value.updated_at_ms),
          sortMsTrusted: false,
          at: typeof value.at === "number" ? value.at : now,
        };
      }
      return next;
    } catch {
      return {};
    }
  }

  function readProjectMoveProjection() {
    try {
      const parsed = JSON.parse(localStorage.getItem(projectMoveProjectionKey) || "{}");
      const raw = parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {};
      const merged = { ...readLegacyProjectMoveProjection(), ...raw };
      const now = Date.now();
      const projection = {};
      for (const [key, value] of Object.entries(merged)) {
        if (!value || typeof value !== "object") continue;
        const sessionId = projectMoveSessionKey(value.sessionId || key);
        if (!sessionId) continue;
        if (typeof value.at === "number" && now - value.at > projectMoveProjectionTtlMs) continue;
        const targetKind = value.targetKind === "projectless" ? "projectless" : "project";
        const targetCwd = String(value.targetCwd || value.path || "");
        if (targetKind === "project" && !targetCwd) continue;
        projection[sessionId] = {
          sessionId,
          targetKind,
          targetCwd,
          targetLabel: String(value.targetLabel || value.label || (targetKind === "projectless" ? "普通对话" : displayProjectName(targetCwd))),
          title: String(value.title || ""),
          sortMs: sortMsForSession(sessionId, value.sortMs || value.updatedAtMs || value.updated_at_ms),
          sortMsTrusted: value.sortMsTrusted === true,
          at: typeof value.at === "number" ? value.at : now,
        };
      }
      return projection;
    } catch {
      return readLegacyProjectMoveProjection();
    }
  }

  function writeProjectMoveProjection(projection) {
    try {
      localStorage.setItem(projectMoveProjectionKey, JSON.stringify(projection || {}));
      localStorage.removeItem(legacyProjectMoveOverridesKey);
    } catch (error) {
      appendCodexElvesFailure("__codexProjectMoveProjectionFailures", error);
    }
  }

  function saveProjectMoveProjection(ref, target, sortMs) {
    const id = projectMoveSessionKey(ref.session_id);
    if (!id || !target) return;
    const projection = readProjectMoveProjection();
    projection[id] = {
      sessionId: id,
      targetKind: target.kind === "projectless" ? "projectless" : "project",
      targetCwd: target.path || "",
      targetLabel: target.label || (target.kind === "projectless" ? "普通对话" : displayProjectName(target.path)),
      title: ref.title || "",
      sortMs: sortMsForSession(ref.session_id, sortMs || target.sortMs),
      sortMsTrusted: target.sortMsTrusted === true,
      at: Date.now(),
    };
    writeProjectMoveProjection(projection);
  }

  function clearProjectMoveProjection(ref) {
    const projection = readProjectMoveProjection();
    const keys = threadIdVariants(ref.session_id).map(projectMoveSessionKey).filter(Boolean);
    let changed = false;
    keys.forEach((key) => {
      if (Object.prototype.hasOwnProperty.call(projection, key)) {
        delete projection[key];
        changed = true;
      }
    });
    if (changed) writeProjectMoveProjection(projection);
  }

  function projectionForSessionId(sessionId, projection = readProjectMoveProjection()) {
    const key = projectMoveSessionKey(sessionId);
    return key ? projection[key] || null : null;
  }

  function projectRowFromListItem(projectItem) {
    if (!projectItem) return null;
    if (projectItem.matches?.("[data-app-action-sidebar-project-row]")) return projectItem;
    return projectItem.querySelector?.("[data-app-action-sidebar-project-row]") || null;
  }

  function targetPath(target) {
    return target?.path || target?.targetCwd || "";
  }

  function targetLabel(target) {
    return target?.label || target?.targetLabel || displayProjectName(targetPath(target));
  }

  function projectItemMatchesTarget(projectItem, target) {
    const projectRow = projectRowFromListItem(projectItem);
    const projectPath = projectRow?.getAttribute?.("data-app-action-sidebar-project-id") || "";
    if (projectPath && sameWorkspacePath(projectPath, targetPath(target))) return true;
    const actual = normalizeProjectLabel(projectRow?.getAttribute?.("data-app-action-sidebar-project-label") || projectItem?.getAttribute?.("aria-label"));
    const labels = uniqueValues([targetLabel(target), displayProjectName(targetPath(target))]).map(normalizeProjectLabel).filter(Boolean);
    return !!actual && labels.includes(actual);
  }

  function findProjectListItem(target) {
    const nativeTarget = nativeProjectTargets().find((project) => sameWorkspacePath(project.path, targetPath(target)));
    if (nativeTarget?.listItem) return nativeTarget.listItem;
    const section = projectsSection();
    if (!section) return null;
    return Array.from(section.querySelectorAll('[role="listitem"][aria-label]')).find((item) => projectItemMatchesTarget(item, target)) || null;
  }

  function closestProjectListItem(row) {
    const item = row.closest?.('[role="listitem"][aria-label]');
    return item?.closest?.('[data-app-action-sidebar-section-heading="Projects"]') ? item : null;
  }

  function rowIsInChats(row) {
    return !!row.closest?.('[data-app-action-sidebar-section-heading="Chats"]');
  }

  function chatsThreadList() {
    return chatsSection()?.querySelector?.('[role="list"][aria-label="对话"], [role="list"]') || null;
  }

  function rowIsUnderTargetProject(row, target) {
    const item = closestProjectListItem(row);
    return !!item && projectItemMatchesTarget(item, target);
  }

  function rowIsUnderTarget(row, target) {
    return target?.targetKind === "projectless" || target?.kind === "projectless" ? rowIsInChats(row) : rowIsUnderTargetProject(row, target);
  }

  function rowListItem(row) {
    return row.closest?.('[role="listitem"]') || row;
  }

  function rowContentRoot(row) {
    return Array.from(row?.children || []).find((child) => String(child.className || "").includes("h-full w-full items-center")) || null;
  }

  function normalizedText(node) {
    return String(node?.textContent || "").replace(/\s+/g, " ").trim();
  }

  function classNameText(node) {
    return String(node?.className || "");
  }

  function isRelativeTimeText(text) {
    const value = String(text || "").replace(/\s+/g, " ").trim();
    return /^(刚刚|just now|\d+\s*(秒|秒钟|分|分钟|小时|天|日|周|星期|个月|月|年|sec|secs|second|seconds|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days|w|wk|wks|week|weeks|mo|mos|month|months|y|yr|yrs|year|years))$/i.test(value);
  }

  function nodeIsThreadTitle(row, node) {
    return Array.from(row?.querySelectorAll?.('[data-thread-title], .truncate.select-none, .truncate.text-base') || [])
      .some((titleNode) => titleNode === node || titleNode.contains(node));
  }

  function closestTimeWrapper(row, node) {
    const root = rowContentRoot(row) || row;
    let current = node?.parentElement || null;
    while (current && current !== root && current !== row) {
      const className = classNameText(current);
      if (current.dataset?.codexProjectMoveTimeWrapper === "true" || (className.includes("ml-[3px]") && className.includes("min-w-[26px]"))) return current;
      current = current.parentElement;
    }
    return null;
  }

  function nodeInsideStatusIcon(row, node) {
    const stop = closestTimeWrapper(row, node) || rowContentRoot(row) || row;
    let current = node || null;
    while (current && current !== stop && current !== row) {
      const className = classNameText(current);
      if (className.includes("animate-spin")) return true;
      if (className.includes("size-5") && className.includes("shrink-0")) return true;
      if (className.includes("contain-paint") && className.includes("contain-layout")) return true;
      current = current.parentElement;
    }
    return false;
  }

  function cleanupManagedStatusIconTimeNodes(row) {
    Array.from(row?.querySelectorAll?.('[data-codex-project-move-time="true"]') || []).forEach((node) => {
      if (!nodeInsideStatusIcon(row, node)) return;
      const text = normalizedText(node);
      delete node.dataset.codexProjectMoveTime;
      delete node.dataset.codexProjectMoveTimeMs;
      if (node.children.length === 0 && isRelativeTimeText(text)) node.textContent = "";
    });
  }

  function nodeLooksLikeTimeLabel(row, node) {
    if (nodeInsideStatusIcon(row, node)) return false;
    if (node?.dataset?.codexProjectMoveTime === "true") return true;
    if (node.children.length > 0) return false;
    const text = normalizedText(node);
    const className = classNameText(node);
    if ((className.includes("tabular-nums") || className.includes("text-token-description-foreground")) && text.length <= 24) return true;
    if (!isRelativeTimeText(text)) return false;
    const rowRect = row?.getBoundingClientRect?.();
    const nodeRect = node?.getBoundingClientRect?.();
    if (!rowRect || !nodeRect || rowRect.width <= 0 || nodeRect.width <= 0) return false;
    return nodeRect.left >= rowRect.left + rowRect.width * 0.45 || nodeRect.right >= rowRect.right - 96;
  }

  function rowTimeLabelCandidates(row) {
    cleanupManagedStatusIconTimeNodes(row);
    const root = rowContentRoot(row) || row;
    const raw = Array.from(root?.querySelectorAll?.("div, span, time, small") || []).filter((node) => {
      if (nodeIsThreadTitle(row, node)) return false;
      return nodeLooksLikeTimeLabel(row, node);
    });
    return raw.filter((node) => !raw.some((other) => other !== node && node.contains(other)));
  }

  function rowTimeLabelNode(row) {
    const candidates = rowTimeLabelCandidates(row);
    return candidates.find((node) => node.dataset?.codexProjectMoveTime !== "true" && !node.closest?.('[data-codex-project-move-time-wrapper="true"]')) || candidates[0] || null;
  }

  function removeTimeLabelNode(row, node) {
    if (!node || !row?.contains?.(node)) return;
    const wrapper = node.closest?.('[data-codex-project-move-time-wrapper="true"]') || closestTimeWrapper(row, node);
    if (wrapper && wrapper !== row && row.contains(wrapper)) {
      wrapper.remove();
      return;
    }
    node.remove();
  }

  function cleanupRowTimeLabels(row, keepNode) {
    if (!keepNode) return;
    rowTimeLabelCandidates(row).forEach((node) => {
      if (node === keepNode) return;
      if (node.dataset?.codexProjectMoveTime === "true" || node.closest?.('[data-codex-project-move-time-wrapper="true"]')) removeTimeLabelNode(row, node);
    });
  }

  function ensureRowTimeLabelNode(row) {
    const existing = rowTimeLabelNode(row);
    if (existing) {
      cleanupRowTimeLabels(row, existing);
      return existing;
    }
    const root = rowContentRoot(row);
    if (!root) return null;
    const wrapper = document.createElement("div");
    wrapper.className = "ml-[3px] flex items-center justify-end gap-1 min-w-[26px]";
    wrapper.dataset.codexProjectMoveTimeWrapper = "true";
    const inner = document.createElement("div");
    const label = document.createElement("div");
    label.className = "text-token-description-foreground text-sm leading-4 empty:hidden tabular-nums overflow-visible truncate text-right group-focus-within:opacity-0 group-hover:opacity-0";
    label.dataset.codexProjectMoveTime = "true";
    inner.appendChild(label);
    wrapper.appendChild(inner);
    root.appendChild(wrapper);
    return label;
  }

  function updateRowTimeLabel(row, sortMs) {
    const label = ensureRowTimeLabelNode(row);
    if (!label) return;
    const timestamp = numericTimestamp(sortMs);
    const text = relativeTimeLabel(timestamp);
    label.dataset.codexProjectMoveTime = "true";
    label.dataset.codexProjectMoveTimeMs = String(timestamp || 0);
    if (text && label.textContent !== text) label.textContent = text;
    cleanupRowTimeLabels(row, label);
  }

  function rowProjectionKind(row) {
    return row?.dataset?.codexProjectMoveTargetKind || rowListItem(row)?.dataset?.codexProjectMoveTargetKind || "";
  }

  function rowSortMs(row, ref = sessionRefFromRow(row), target = null) {
    return sortMsForSession(ref.session_id, target?.sortMs || row?.dataset?.codexProjectMoveSortMs || rowListItem(row)?.dataset?.codexProjectMoveSortMs);
  }

  function threadRowFromListItem(item) {
    if (!item) return null;
    if (item.matches?.("[data-app-action-sidebar-thread-id]")) return item;
    return item.querySelector?.("[data-app-action-sidebar-thread-id]") || null;
  }

  function rowPinned(row) {
    return row?.getAttribute?.("data-app-action-sidebar-thread-pinned") === "true" || rowListItem(row)?.getAttribute?.("data-app-action-sidebar-thread-pinned") === "true";
  }

  function insertRowItemByTime(list, item, row, target) {
    const ref = sessionRefFromRow(row);
    const sortMs = rowSortMs(row, ref, target);
    item.dataset.codexProjectMoveSortMs = String(sortMs || 0);
    row.dataset.codexProjectMoveSortMs = String(sortMs || 0);
    if (target?.sortMsTrusted) updateRowTimeLabel(row, sortMs);
    const pinned = rowPinned(row);
    const sessionKey = projectMoveSessionKey(ref.session_id);
    const existingItems = Array.from(list.children).filter((child) => child !== item);
    let firstNonThreadItem = null;
    for (const child of existingItems) {
      const childRow = threadRowFromListItem(child);
      if (!childRow) {
        firstNonThreadItem = firstNonThreadItem || child;
        continue;
      }
      const childPinned = rowPinned(childRow);
      if (childPinned && !pinned) continue;
      if (!childPinned && pinned) {
        list.insertBefore(item, child);
        return;
      }
      const childRef = sessionRefFromRow(childRow);
      const childSortMs = rowSortMs(childRow, childRef);
      const childKey = projectMoveSessionKey(childRef.session_id);
      if (sortMs > childSortMs || (sortMs === childSortMs && sessionKey > childKey)) {
        list.insertBefore(item, child);
        return;
      }
    }
    if (firstNonThreadItem) {
      list.insertBefore(item, firstNonThreadItem);
      return;
    }
    list.appendChild(item);
  }

  function projectMoveInjectedList(projectItem) {
    let list = projectItem.querySelector('[data-codex-project-move-injected-list="true"]');
    if (!list) {
      const body = Array.from(projectItem.children).find((child) => child.classList?.contains("overflow-hidden")) || projectItem;
      list = document.createElement("div");
      list.setAttribute("role", "list");
      list.setAttribute("data-codex-project-move-injected-list", "true");
      list.className = "flex flex-col";
      body.appendChild(list);
    }
    return list;
  }

  function projectThreadList(projectItem, target) {
    const targetCwd = targetPath(target);
    const projectLists = Array.from(projectItem.querySelectorAll("[data-app-action-sidebar-project-list-id]"));
    return projectLists.find((list) => sameWorkspacePath(list.getAttribute("data-app-action-sidebar-project-list-id"), targetCwd))
      || projectLists[0]
      || projectMoveInjectedList(projectItem);
  }

  function projectEmptyStateNodes(projectItem) {
    const emptyLabels = new Set(["暂无对话", "No conversations"]);
    return Array.from(projectItem.querySelectorAll("div, span")).filter((node) => {
      if (node.classList?.contains("overflow-hidden")) return false;
      if (node.closest('[data-app-action-sidebar-thread-id], [data-codex-project-move-injected-list="true"]')) return false;
      return emptyLabels.has(normalizeProjectLabel(node.textContent));
    });
  }

  function setProjectEmptyStateHidden(projectItem, hidden) {
    projectEmptyStateNodes(projectItem).forEach((node) => {
      if (hidden) {
        node.dataset.codexProjectMoveEmptyHidden = "true";
        node.classList.add("codex-project-move-hidden");
      } else if (node.dataset.codexProjectMoveEmptyHidden === "true") {
        delete node.dataset.codexProjectMoveEmptyHidden;
        node.classList.remove("codex-project-move-hidden");
      }
    });
  }

  function updateProjectMoveEmptyStates() {
    document.querySelectorAll('[data-codex-project-move-injected-list="true"]').forEach((list) => {
      const projectItem = list.closest('[role="listitem"][aria-label]');
      const hasRows = Array.from(list.children).some((child) => child.querySelector?.("[data-app-action-sidebar-thread-id]") || child.matches?.("[data-app-action-sidebar-thread-id]"));
      if (!hasRows) list.remove();
      if (projectItem) setProjectEmptyStateHidden(projectItem, hasRows);
    });
    document.querySelectorAll('[data-codex-project-move-empty-hidden="true"]').forEach((node) => {
      const projectItem = node.closest('[role="listitem"][aria-label]');
      const list = projectItem?.querySelector?.('[data-codex-project-move-injected-list="true"]');
      if (!list || list.children.length === 0) {
        delete node.dataset.codexProjectMoveEmptyHidden;
        node.classList.remove("codex-project-move-hidden");
      }
    });
  }

  function moveRowToProjectList(row, target) {
    const projectItem = findProjectListItem(target);
    if (!projectItem) return false;
    const list = projectThreadList(projectItem, target);
    const item = rowListItem(row);
    if (!list) return false;
    insertRowItemByTime(list, item, row, target);
    invalidateSessionRowsCache();
    item.dataset.codexProjectMoveTargetKind = "project";
    item.dataset.codexProjectMoveTargetCwd = targetPath(target);
    row.dataset.codexProjectMoveTargetKind = "project";
    row.dataset.codexProjectMoveTargetCwd = targetPath(target);
    setProjectEmptyStateHidden(projectItem, true);
    return true;
  }

  function moveRowToChats(row, target = null) {
    const list = chatsThreadList();
    if (!list) return false;
    const item = rowListItem(row);
    insertRowItemByTime(list, item, row, target);
    invalidateSessionRowsCache();
    item.dataset.codexProjectMoveTargetKind = "projectless";
    row.dataset.codexProjectMoveTargetKind = "projectless";
    delete item.dataset.codexProjectMoveTargetCwd;
    delete row.dataset.codexProjectMoveTargetCwd;
    updateProjectMoveEmptyStates();
    return true;
  }

  function applyProjectMoveProjection() {
    if (!codexElvesSettings().projectMove) return;
    const projection = readProjectMoveProjection();
    const targetRowsById = new Map();
    const settledRefs = [];
    const now = Date.now();
    const rows = sessionRows(true);
    rows.forEach((row) => {
      const ref = sessionRefFromRow(row);
      const target = projectionForSessionId(ref.session_id, projection);
      if (target && rowIsUnderTarget(row, target)) {
        const rowId = projectMoveSessionKey(ref.session_id);
        const hadProjectionKind = !!rowProjectionKind(row);
        const existingRow = targetRowsById.get(rowId);
        if (existingRow && existingRow !== row) {
          const existingIsProjection = !!rowProjectionKind(existingRow);
          const currentIsProjection = !!rowProjectionKind(row);
          const rowToRemove = existingIsProjection && !currentIsProjection ? existingRow : row;
          rowListItem(rowToRemove).remove();
          if (rowToRemove === existingRow) targetRowsById.set(rowId, row);
          if (rowToRemove === row) return;
        } else {
          targetRowsById.set(rowId, row);
        }
        if (!hadProjectionKind && typeof target.at === "number" && now - target.at > projectMoveProjectionSettleMs) settledRefs.push(ref);
        const projectItem = closestProjectListItem(row);
        if (projectItem) setProjectEmptyStateHidden(projectItem, true);
      }
    });
    rows.forEach((row) => {
      const ref = sessionRefFromRow(row);
      const rowId = projectMoveSessionKey(ref.session_id);
      const target = projectionForSessionId(ref.session_id, projection);
      if (!target) {
        const item = rowListItem(row);
        delete row.dataset.codexProjectMoveTargetKind;
        delete row.dataset.codexProjectMoveTargetCwd;
        delete item.dataset.codexProjectMoveTargetKind;
        delete item.dataset.codexProjectMoveTargetCwd;
        return;
      }
      if (rowIsUnderTarget(row, target)) return;
      if (targetRowsById.has(rowId)) {
        rowListItem(row).remove();
        return;
      }
      const moved = target.targetKind === "projectless" ? moveRowToChats(row, target) : moveRowToProjectList(row, target);
      if (moved) targetRowsById.set(rowId, row);
    });
    settledRefs.forEach(clearProjectMoveProjection);
    updateProjectMoveEmptyStates();
  }

  function scheduleProjectMoveProjection() {
    if (!codexElvesSettings().projectMove || window.__codexProjectMoveProjectionTimer) return;
    window.__codexProjectMoveProjectionTimer = setTimeout(() => {
      if (window.__codexProjectMoveRuntimeId !== codexProjectMoveRuntimeId) return;
      window.__codexProjectMoveProjectionTimer = null;
      applyProjectMoveProjection();
    }, 80);
  }

  async function refreshRecentConversationsForHost() {
    try {
      const signals = await import("./assets/app-server-manager-signals-C1h8B-R-.js");
      if (typeof signals.rn === "function") await signals.rn("refresh-recent-conversations-for-host", { hostId: "local", sortKey: "updated_at" });
    } catch (error) {
      appendCodexElvesFailure("__codexProjectMoveRefreshFailures", error);
    }
  }

  function refreshAfterProjectMove() {
    const refreshVisibleSidebar = () => {
      applyProjectMoveProjection();
      scheduleChatsSortCorrection(0, { refreshKeys: true });
    };
    refreshVisibleSidebar();
    refreshRecentConversationsForHost().finally(() => {
      projectMoveRefreshDelaysMs.forEach((delay) => setTimeout(refreshVisibleSidebar, delay));
    });
  }

  function visibleChatsRows() {
    const list = chatsThreadList();
    if (!list) return [];
    return Array.from(list.children).map(threadRowFromListItem).filter(Boolean).filter((row) => rowIsInChats(row));
  }

  function chatsSortNeedsCorrection(rows) {
    let previousPinned = true;
    let previousSortMs = Infinity;
    let previousKey = "\uffff";
    for (const row of rows) {
      const pinned = rowPinned(row);
      const ref = sessionRefFromRow(row);
      const sortMs = rowSortMs(row, ref);
      const key = projectMoveSessionKey(ref.session_id);
      if (previousPinned && !pinned) {
        previousPinned = false;
        previousSortMs = sortMs;
        previousKey = key;
        continue;
      }
      if (!previousPinned && pinned) return true;
      if (sortMs > previousSortMs || (sortMs === previousSortMs && key > previousKey)) return true;
      previousSortMs = sortMs;
      previousKey = key;
    }
    return false;
  }

  function reorderChatsRows(rows) {
    const list = chatsThreadList();
    if (!list || rows.length < 2) return;
    const rowItems = new Set(rows.map(rowListItem));
    const firstNonThreadItem = Array.from(list.children).find((child) => !rowItems.has(child) && !threadRowFromListItem(child));
    const orderedRows = [...rows].sort((left, right) => {
      const leftPinned = rowPinned(left);
      const rightPinned = rowPinned(right);
      if (leftPinned !== rightPinned) return leftPinned ? -1 : 1;
      const leftRef = sessionRefFromRow(left);
      const rightRef = sessionRefFromRow(right);
      const leftSortMs = rowSortMs(left, leftRef);
      const rightSortMs = rowSortMs(right, rightRef);
      if (leftSortMs !== rightSortMs) return rightSortMs - leftSortMs;
      return projectMoveSessionKey(rightRef.session_id).localeCompare(projectMoveSessionKey(leftRef.session_id));
    });
    orderedRows.forEach((row) => list.insertBefore(rowListItem(row), firstNonThreadItem || null));
    invalidateSessionRowsCache();
  }

  async function applyChatsSortCorrection({ refreshKeys = false } = {}) {
    if (!codexElvesSettings().projectMove || document.visibilityState === "hidden") return;
    if (chatsSortInFlight) {
      window.__codexProjectMoveChatsSortPending = true;
      if (refreshKeys) window.__codexProjectMoveChatsSortRefreshKeys = true;
      return;
    }
    const rows = visibleChatsRows();
    if (rows.length < 2) return;
    const refs = rows.map(sessionRefFromRow).filter((ref) => ref.session_id);
    const signature = refs.map((ref) => projectMoveSessionKey(ref.session_id)).join("|");
    const allRowsHaveSortMs = rows.every((row) => numericTimestamp(row.dataset.codexProjectMoveSortMs || rowListItem(row).dataset.codexProjectMoveSortMs));
    const shouldRefreshSortKeys = refreshKeys || signature !== chatsSortSignature || !allRowsHaveSortMs;
    if (!shouldRefreshSortKeys && !chatsSortNeedsCorrection(rows)) return;
    chatsSortInFlight = true;
    try {
      if (shouldRefreshSortKeys) {
        const result = await Promise.race([
          postJson("/thread-sort-keys", { sessions: refs }),
          new Promise((resolve) => setTimeout(
            () => resolve({ status: "failed", timeout: true, sort_keys: [] }),
            chatsSortRequestTimeoutMs,
          )),
        ]).catch(() => ({ status: "failed", sort_keys: [] }));
        const currentRows = visibleChatsRows();
        const currentSignature = currentRows
          .map((row) => projectMoveSessionKey(sessionRefFromRow(row).session_id))
          .join("|");
        if (currentSignature !== signature || currentRows.some((row) => !row.isConnected)) {
          window.__codexProjectMoveChatsSortPending = true;
          window.__codexProjectMoveChatsSortRefreshKeys = true;
          return;
        }
        const byId = new Map();
        if (result?.status === "ok" && Array.isArray(result?.sort_keys)) {
          result.sort_keys.forEach((item) => {
            const key = projectMoveSessionKey(String(item?.session_id || ""));
            if (key) byId.set(key, item);
          });
        }
        currentRows.forEach((row) => {
          const ref = sessionRefFromRow(row);
          const payload = byId.get(projectMoveSessionKey(ref.session_id));
          const trustedSortMs = timestampMsFromPayload(payload);
          const sortMs = trustedSortMs || sortMsForSession(ref.session_id, row.dataset.codexProjectMoveSortMs || rowListItem(row).dataset.codexProjectMoveSortMs);
          row.dataset.codexProjectMoveSortMs = String(sortMs || 0);
          rowListItem(row).dataset.codexProjectMoveSortMs = String(sortMs || 0);
          if (trustedSortMs) updateRowTimeLabel(row, trustedSortMs);
        });
      }
      const activeRows = visibleChatsRows();
      if (chatsSortNeedsCorrection(activeRows)) reorderChatsRows(activeRows);
      chatsSortSignature = visibleChatsRows().map((row) => projectMoveSessionKey(sessionRefFromRow(row).session_id)).join("|");
    } finally {
      chatsSortInFlight = false;
      if (window.__codexProjectMoveChatsSortPending) {
        window.__codexProjectMoveChatsSortPending = false;
        scheduleChatsSortCorrection(0, {
          refreshKeys: window.__codexProjectMoveChatsSortRefreshKeys === true,
        });
      }
    }
  }

  function scheduleChatsSortCorrection(delay = chatsSortEventDelayMs, options = {}) {
    if (!codexElvesSettings().projectMove || document.visibilityState === "hidden") return;
    if (options.refreshKeys) window.__codexProjectMoveChatsSortRefreshKeys = true;
    if (window.__codexProjectMoveChatsSortTimer) return;
    window.__codexProjectMoveChatsSortTimer = setTimeout(() => {
      if (window.__codexProjectMoveRuntimeId !== codexProjectMoveRuntimeId) return;
      window.__codexProjectMoveChatsSortTimer = null;
      const refreshKeys = window.__codexProjectMoveChatsSortRefreshKeys === true;
      window.__codexProjectMoveChatsSortRefreshKeys = false;
      applyChatsSortCorrection({ refreshKeys }).catch((error) => {
        appendCodexElvesFailure("__codexProjectMoveSortFailures", error);
      });
    }, delay);
  }

  function armChatsSortVisibleFallback() {
    clearTimeout(window.__codexProjectMoveChatsSortFallbackTimer);
    window.__codexProjectMoveChatsSortFallbackTimer = null;
    chatsSortFallbackArmed = false;
    if (!codexElvesSettings().projectMove || document.visibilityState === "hidden") return;
    chatsSortFallbackArmed = true;
    window.__codexProjectMoveChatsSortFallbackTimer = setTimeout(() => {
      window.__codexProjectMoveChatsSortFallbackTimer = null;
      chatsSortFallbackArmed = false;
      scheduleChatsSortCorrection(0, { refreshKeys: true });
      armChatsSortVisibleFallback();
    }, chatsSortVisibleFallbackMs);
  }

  function stopChatsSortRuntime() {
    clearTimeout(window.__codexProjectMoveChatsSortTimer);
    window.__codexProjectMoveChatsSortTimer = null;
    clearTimeout(window.__codexProjectMoveChatsSortFallbackTimer);
    window.__codexProjectMoveChatsSortFallbackTimer = null;
    window.__codexProjectMoveChatsSortRefreshKeys = false;
    window.__codexProjectMoveChatsSortPending = false;
    chatsSortFallbackArmed = false;
  }

  function syncChatsSortVisibilityListener() {
    document.removeEventListener("visibilitychange", window.__codexProjectMoveVisibilityHandler, true);
    window.__codexProjectMoveVisibilityHandler = null;
    if (!codexElvesSettings().projectMove) {
      stopChatsSortRuntime();
      return;
    }
    window.__codexProjectMoveVisibilityHandler = () => {
      if (document.visibilityState === "hidden") {
        stopChatsSortRuntime();
        return;
      }
      scheduleChatsSortCorrection(0, { refreshKeys: true });
      armChatsSortVisibleFallback();
    };
    document.addEventListener("visibilitychange", window.__codexProjectMoveVisibilityHandler, true);
    if (document.visibilityState !== "hidden") armChatsSortVisibleFallback();
  }

  async function setProjectlessThreadIds(ref, mode) {
    const variants = threadIdVariants(ref.session_id);
    if (variants.length === 0) throw new Error("未找到会话 ID");
    const existingIds = await getCodexGlobalState("projectless-thread-ids").catch(() => []);
    const ids = Array.isArray(existingIds) ? existingIds : [];
    const variantSet = new Set(variants);
    const nextIds = mode === "add" ? uniqueValues([...ids, ...variants]) : ids.filter((id) => !variantSet.has(id));
    if (nextIds.length !== ids.length || nextIds.some((id, index) => id !== ids[index])) await setCodexGlobalState("projectless-thread-ids", nextIds);
  }

  async function clearThreadWorkspaceHints(ref) {
    const variants = threadIdVariants(ref.session_id);
    if (variants.length === 0) return;
    const hints = objectGlobalState(await getCodexGlobalState("thread-workspace-root-hints").catch(() => ({})));
    const hintKeys = variants.filter((id) => Object.prototype.hasOwnProperty.call(hints, id));
    if (hintKeys.length > 0) {
      hintKeys.forEach((id) => delete hints[id]);
      await setCodexGlobalState("thread-workspace-root-hints", hints);
    }
  }

  async function clearThreadWritableRoots(ref) {
    const variants = threadIdVariants(ref.session_id);
    if (variants.length === 0) return;
    const roots = objectGlobalState(await getCodexGlobalState("thread-writable-roots").catch(() => ({})));
    const rootKeys = variants.filter((id) => Object.prototype.hasOwnProperty.call(roots, id));
    if (rootKeys.length > 0) {
      rootKeys.forEach((id) => delete roots[id]);
      await setCodexGlobalState("thread-writable-roots", roots);
    }
  }

  async function clearThreadProjectlessOutputDirectories(ref) {
    const variants = threadIdVariants(ref.session_id);
    if (variants.length === 0) return;
    const dirs = objectGlobalState(await getCodexGlobalState("thread-projectless-output-directories").catch(() => ({})));
    const dirKeys = variants.filter((id) => Object.prototype.hasOwnProperty.call(dirs, id));
    if (dirKeys.length > 0) {
      dirKeys.forEach((id) => delete dirs[id]);
      await setCodexGlobalState("thread-projectless-output-directories", dirs);
    }
  }

  async function moveSessionToProjectless(ref) {
    if (!ref.session_id) throw new Error("未找到会话 ID");
    await setProjectlessThreadIds(ref, "add");
    await clearThreadWorkspaceHints(ref);
    await clearThreadWritableRoots(ref);
    await clearThreadProjectlessOutputDirectories(ref);
    const sortKey = await postJson("/thread-sort-key", ref).catch(() => ({}));
    return { status: "moved", session_id: ref.session_id, updated_at: sortKey?.updated_at, updated_at_ms: sortKey?.updated_at_ms, created_at_ms: sortKey?.created_at_ms };
  }

  function isNativeProjectTarget(target) {
    return target?.kind === "project" && nativeProjectTargets().some((project) => sameWorkspacePath(project.path, target.path));
  }

  async function moveSessionToProject(ref, target) {
    if (!ref.session_id) throw new Error("未找到会话 ID");
    if (!target?.path) throw new Error("目标项目路径为空");
    if (!isNativeProjectTarget(target)) throw new Error("目标项目不在 Codex 项目列表中");
    const result = await postJson("/move-thread-workspace", { ...ref, target_cwd: target.path });
    if (result.status !== "moved") throw new Error(result.message || "移动项目失败");
    await setProjectlessThreadIds(ref, "remove");
    await clearThreadWorkspaceHints(ref);
    return result;
  }

  function showToast(message) {
    document.querySelectorAll(".codex-delete-toast").forEach((node) => node.remove());
    const toast = document.createElement("div");
    toast.className = "codex-delete-toast";
    toast.textContent = message;
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 10000);
  }

  function upstreamWorktreeField(dialog, name) {
    return dialog.querySelector(`[data-codex-upstream-worktree-field="${name}"]`);
  }

  function upstreamWorktreePayload(dialog) {
    return {
      repoPath: upstreamWorktreeField(dialog, "repoPath")?.value || "",
      branchName: upstreamWorktreeField(dialog, "branchName")?.value || "",
      worktreePath: upstreamWorktreeField(dialog, "worktreePath")?.value || "",
      remote: upstreamWorktreeField(dialog, "remote")?.value || "upstream",
      baseBranch: upstreamWorktreeField(dialog, "baseBranch")?.value || "main",
      fetch: true,
    };
  }

  function readUpstreamBranchSelection() {
    try {
      return JSON.parse(sessionStorage.getItem(upstreamBranchSelectionKey) || "null");
    } catch {
      return null;
    }
  }

  function writeUpstreamBranchSelection(selection) {
    if (!selection) {
      sessionStorage.removeItem(upstreamBranchSelectionKey);
      return;
    }
    sessionStorage.setItem(upstreamBranchSelectionKey, JSON.stringify(selection));
  }

  function nativeBranchMenuCandidates() {
    return [...document.querySelectorAll('[role="menu"], [data-radix-menu-content], [cmdk-list]')];
  }

  function looksLikeBranchMenu(menu, trigger = branchMenuTriggerFromMenu(menu)) {
    const text = (menu.innerText || menu.textContent || "").toLowerCase();
    if (!branchMenuTriggerIsBranchControl(trigger)) return false;
    if (/^start in\b/.test(text) || /\bwork locally\b.*\bnew worktree\b.*\bcloud\b/s.test(text)) return false;
    return /\bbranches?\b|\bbranche\b|create and checkout new branch|create branch/.test(text);
  }

  function visibleElement(node) {
    if (!(node instanceof Element)) return false;
    const rect = node.getBoundingClientRect?.();
    return !!rect && rect.width > 0 && rect.height > 0;
  }

  function effectiveElementRect(node) {
    if (!(node instanceof Element)) return null;
    const rect = node.getBoundingClientRect?.();
    if (rect && rect.width > 0 && rect.height > 0) return rect;
    const controls = [...node.closest?.(".composer-footer")?.querySelectorAll?.("button, [role='button']") || []]
      .filter((candidate) => candidate !== node && visibleElement(candidate));
    const matching = controls.find((candidate) => normalizedElementText(candidate) === normalizedElementText(node));
    return matching?.getBoundingClientRect?.() || rect || null;
  }

  function sidebarProjectRows() {
    const section = projectsSection?.();
    return [...document.querySelectorAll('[data-app-action-sidebar-project-row][data-app-action-sidebar-project-id]')]
      .filter((row) => !section || section.contains(row));
  }

  function projectRowPath(row) {
    return row?.getAttribute?.("data-app-action-sidebar-project-id") || "";
  }

  function projectContextFromRow(row) {
    const path = projectRowPath(row);
    if (!path) return null;
    const label = row.getAttribute("data-app-action-sidebar-project-label")
      || row.getAttribute("aria-label")
      || displayProjectName(path);
    return {
      repoPath: path.startsWith("/") ? path : "",
      projectId: path.startsWith("/") ? "" : path,
      label: normalizeProjectLabel(label),
      at: Date.now(),
    };
  }

  function remoteProjectContextFromGlobalState(projectId) {
    const normalizedProjectId = String(projectId || "").trim();
    if (!normalizedProjectId) return null;
    return { projectId: normalizedProjectId, repoPath: "", label: "", at: Date.now() };
  }

  function readUpstreamProjectContext() {
    try {
      const context = JSON.parse(sessionStorage.getItem(upstreamProjectContextKey) || "null");
      if (!context || typeof context !== "object") return null;
      if (typeof context.at === "number" && Date.now() - context.at > upstreamProjectContextTtlMs) return null;
      if (!context.repoPath && !context.projectId) return null;
      return context;
    } catch {
      return null;
    }
  }

  function writeUpstreamProjectContext(context) {
    if (!context?.repoPath && !context?.projectId) return;
    try {
      sessionStorage.setItem(upstreamProjectContextKey, JSON.stringify({
        repoPath: context.repoPath || "",
        projectId: context.projectId || "",
        label: context.label || "",
        at: Date.now(),
      }));
    } catch {
    }
  }

  function projectContextFromStartButton(button) {
    const row = button?.closest?.('[data-app-action-sidebar-project-row][data-app-action-sidebar-project-id]');
    return projectContextFromRow(row);
  }

  function rememberStartNewChatProjectContext(event) {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const button = target?.closest?.('button[aria-label^="Start new chat in "]');
    const context = projectContextFromStartButton(button);
    if (context) writeUpstreamProjectContext(context);
  }

  function visibleProjectRows() {
    return sidebarProjectRows().filter((row) => visibleElement(row));
  }

  function currentProjectContextFromStartButton() {
    const startButtons = [...document.querySelectorAll('button[aria-label^="Start new chat in "]')]
      .filter((button) => visibleElement(button));
    const bottomHalf = window.innerHeight * 0.5;
    startButtons.sort((left, right) => {
      const leftRect = left.getBoundingClientRect();
      const rightRect = right.getBoundingClientRect();
      const leftScore = Math.abs(leftRect.y - bottomHalf) + Math.max(0, bottomHalf - leftRect.y) * 0.5;
      const rightScore = Math.abs(rightRect.y - bottomHalf) + Math.max(0, bottomHalf - rightRect.y) * 0.5;
      return leftScore - rightScore;
    });
    for (const button of startButtons) {
      const context = projectContextFromStartButton(button);
      if (context) return context;
    }
    return null;
  }

  function currentProjectRepoPathFromSelectedProjectButton() {
    const projectButtons = [...document.querySelectorAll('button[aria-haspopup="menu"]')]
      .filter((button) => visibleElement(button))
      .filter((button) => button.getBoundingClientRect().x > 300)
      .map((button) => (button.innerText || button.textContent || "").trim())
      .filter(Boolean);
    for (const label of projectButtons) {
      const match = visibleProjectRows().find((row) => {
        const rowLabel = row.getAttribute("data-app-action-sidebar-project-label") || row.getAttribute("aria-label") || "";
        return rowLabel.trim() === label;
      });
      const path = projectRowPath(match);
      if (path?.startsWith?.("/")) return path;
    }
    return "";
  }

  function projectContextFromProjectLabel(label) {
    const normalizedLabel = normalizeProjectLabel(label);
    if (!normalizedLabel) return null;
    const row = visibleProjectRows().find((candidate) => {
      const rowPath = projectRowPath(candidate);
      const rowLabels = [
        candidate.getAttribute("data-app-action-sidebar-project-label"),
        candidate.getAttribute("aria-label"),
        displayProjectName(rowPath),
      ].map(normalizeProjectLabel).filter(Boolean);
      return rowLabels.includes(normalizedLabel);
    });
    const context = projectContextFromRow(row);
    if (!context) return null;
    return context.projectId ? { ...remoteProjectContextFromGlobalState(context.projectId), label: context.label } : context;
  }

  function contextMatchesProjectLabel(context, label) {
    const expected = normalizeProjectLabel(label);
    if (!expected) return true;
    const actual = normalizeProjectLabel(context?.label);
    return !actual || actual === expected;
  }

  function currentProjectContextFromStoredSelection(label = "") {
    const context = readUpstreamProjectContext();
    return contextMatchesProjectLabel(context, label) ? context : null;
  }

  function currentProjectContextForBranchMenu(menu, trigger = branchMenuTriggerFromMenu(menu)) {
    const footer = trigger?.closest?.(".composer-footer");
    const projectButton = footer ? [...footer.querySelectorAll('button, [role="button"]')]
      .filter((node) => node !== trigger && visibleElement(node))
      .filter((node) => {
        const rect = effectiveElementRect(node);
        const triggerRect = effectiveElementRect(trigger);
        return rect && triggerRect && rect.x < triggerRect.x;
      })
      .sort((left, right) => effectiveElementRect(left).x - effectiveElementRect(right).x)
      .find((node) => projectContextFromProjectLabel(normalizedElementText(node))) : null;
    const projectLabel = normalizedElementText(projectButton);
    return currentProjectContextFromStoredSelection(projectLabel)
      || projectContextFromProjectLabel(projectLabel)
      || currentProjectContextFromStoredSelection()
      || currentProjectContext();
  }

  function currentProjectRepoPathFromExpandedRows() {
    const expandedRows = visibleProjectRows().filter((row) => row.getAttribute("data-app-action-sidebar-project-collapsed") === "false");
    const pathRows = expandedRows.filter((row) => projectRowPath(row).startsWith("/"));
    if (pathRows.length === 1) return projectRowPath(pathRows[0]);
    return "";
  }

  function currentProjectContext() {
    const stored = currentProjectContextFromStoredSelection();
    if (stored) return stored;
    const selectedPath = currentProjectRepoPathFromSelectedProjectButton();
    if (selectedPath) return { repoPath: selectedPath, projectId: "", label: displayProjectName(selectedPath), at: Date.now() };
    const startContext = currentProjectContextFromStartButton();
    if (startContext) return startContext;
    const expandedPath = currentProjectRepoPathFromExpandedRows();
    if (expandedPath) return { repoPath: expandedPath, projectId: "", label: displayProjectName(expandedPath), at: Date.now() };
    return null;
  }

  function newWorktreeModeActive() {
    return [...document.querySelectorAll('button, [role="button"]')]
      .filter((node) => visibleElement(node))
      .some((node) => {
        return normalizedElementText(node) === "New worktree";
      });
  }

  function normalizedElementText(node) {
    return (node?.innerText || node?.textContent || "").replace(/\s+/g, " ").trim();
  }

  async function loadUpstreamBranchDefaults(context) {
    const repoPath = typeof context === "string" ? context : context?.repoPath || "";
    const projectId = typeof context === "string" ? "" : context?.projectId || "";
    if (!repoPath && !projectId) return null;
    const cacheKey = projectId ? `project:${projectId}` : `repo:${repoPath}`;
    const cacheTtlMs = projectId ? upstreamRemoteBranchDefaultsCacheTtlMs : upstreamBranchDefaultsCacheTtlMs;
    const cached = upstreamBranchDefaultsCache.get(cacheKey);
    if (cached && Date.now() - cached.loadedAt < cacheTtlMs) return cached;
    const inflight = upstreamBranchDefaultsInflight.get(cacheKey);
    if (inflight) return inflight;
    const request = postJson("/upstream-worktree/defaults", { repoPath, projectId })
      .then((result) => {
        const entry = { repoPath, projectId, result, loadedAt: Date.now() };
        if (result?.status === "ok") upstreamBranchDefaultsCache.set(cacheKey, entry);
        return entry;
      })
      .finally(() => upstreamBranchDefaultsInflight.delete(cacheKey));
    upstreamBranchDefaultsInflight.set(cacheKey, request);
    return request;
  }

  function renderUpstreamBranchOption(menu, context, ref) {
    const repoPath = context?.repoPath || "";
    const label = ref.label || `${ref.remote || "upstream"}/${ref.branch || "main"}`;
    const item = document.createElement("div");
    item.setAttribute("role", "menuitem");
    item.setAttribute("aria-checked", "false");
    item.setAttribute(upstreamBranchOptionAttribute, "true");
    item.setAttribute("data-repo-path", repoPath);
    item.setAttribute("data-project-id", context?.projectId || "");
    item.setAttribute("data-remote", ref.remote || "upstream");
    item.setAttribute("data-base-branch", ref.branch || "main");
    item.setAttribute("data-label", label);
    item.className = "codex-upstream-branch-option cursor-interaction flex items-center gap-2 rounded-sm px-2 py-1.5 text-sm text-token-foreground hover:bg-token-list-hover-background";
    item.innerHTML = `${branchIconSvg()}<span class="min-w-0 flex-1 truncate">${escapeHtml(label)}</span>${checkmarkSvg()}`;
    menu.appendChild(item);
  }

  function branchIconSvg() {
    return '<svg aria-hidden="true" data-codex-upstream-branch-icon="true" viewBox="0 0 24 24" class="h-4 w-4 shrink-0 text-token-text-tertiary" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" x2="6" y1="3" y2="15"></line><circle cx="18" cy="6" r="3"></circle><circle cx="6" cy="18" r="3"></circle><path d="M18 9a9 9 0 0 1-9 9"></path></svg>';
  }

  function checkmarkSvg() {
    return '<svg hidden aria-hidden="true" data-codex-upstream-branch-check="true" viewBox="0 0 24 24" class="h-4 w-4 shrink-0 text-token-text-secondary" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"></path></svg>';
  }

  function branchMenuItems(menu) {
    return [...menu.querySelectorAll('[role="menuitem"], [data-radix-collection-item]')]
      .filter((item) => !item.closest?.(`[${upstreamBranchOptionAttribute}]`));
  }

  function branchMenuItemLabel(menuItem) {
    return normalizedElementText(menuItem);
  }

  function upstreamBranchOptionLabel(option) {
    return option?.getAttribute?.("data-label") || normalizedElementText(option);
  }

  function worktreeBranchMap(defaultsResult) {
    const repoRoot = defaultsResult?.repoRoot || "";
    const entries = Array.isArray(defaultsResult?.worktreeBranches) ? defaultsResult.worktreeBranches : [];
    return new Map(entries
      .filter((entry) => entry?.branch && entry?.path && entry.path !== repoRoot)
      .map((entry) => [entry.branch, entry.path]));
  }

  function annotateBranchMenuWorktreeUsage(menu, defaultsResult) {
    const usedBranches = worktreeBranchMap(defaultsResult);
    for (const item of branchMenuItems(menu)) {
      item.removeAttribute(branchWorktreePathAttribute);
      item.removeAttribute("data-codex-tooltip");
      item.removeAttribute("title");
      const worktreePath = usedBranches.get(branchMenuItemLabel(item));
      if (!worktreePath) continue;
      item.setAttribute(branchWorktreePathAttribute, worktreePath);
      item.setAttribute("data-codex-tooltip", `该分支已在另一个 worktree 使用：${worktreePath}`);
    }
  }

  function branchWorktreePathFromMenuItem(menuItem) {
    const annotatedPath = menuItem?.getAttribute?.(branchWorktreePathAttribute) || "";
    if (annotatedPath) return annotatedPath;
    const menu = menuItem?.closest?.('[role="menu"], [data-radix-menu-content]');
    const context = currentProjectContextForBranchMenu(menu);
    const cacheKey = context?.projectId ? `project:${context.projectId}` : `repo:${context?.repoPath || ""}`;
    const usedBranches = worktreeBranchMap(upstreamBranchDefaultsCache.get(cacheKey)?.result);
    return usedBranches.get(branchMenuItemLabel(menuItem)) || "";
  }

  function upstreamBranchOptionsMatchRefs(menu, context, refs) {
    const repoPath = context?.repoPath || "";
    const projectId = context?.projectId || "";
    const options = [...menu.querySelectorAll(`[${upstreamBranchOptionAttribute}]`)];
    if (options.length !== refs.length) return false;
    return options.every((option, index) => {
      const ref = refs[index];
      return option.getAttribute("data-repo-path") === repoPath
        && option.getAttribute("data-project-id") === projectId
        && option.getAttribute("data-remote") === (ref.remote || "upstream")
        && option.getAttribute("data-base-branch") === (ref.branch || "main")
        && upstreamBranchOptionLabel(option) === (ref.label || `${ref.remote || "upstream"}/${ref.branch || "main"}`);
    });
  }

  function syncUpstreamBranchMenuSelection(menu) {
    if (!menu) return;
    const selection = readUpstreamBranchSelection();
    for (const option of menu.querySelectorAll(`[${upstreamBranchOptionAttribute}]`)) {
      const selected = !!selection
        && option.getAttribute("data-repo-path") === (selection.repoPath || "")
        && option.getAttribute("data-project-id") === (selection.projectId || "")
        && option.getAttribute("data-remote") === (selection.remote || "upstream")
        && option.getAttribute("data-base-branch") === (selection.baseBranch || "main");
      option.setAttribute("aria-checked", selected ? "true" : "false");
      option.toggleAttribute("data-selected", selected);
      const check = option.querySelector('[data-codex-upstream-branch-check="true"]');
      if (check && selected) check.removeAttribute("hidden");
      if (check && !selected) check.setAttribute("hidden", "");
    }
  }

  function removeUpstreamBranchOptions(scope = document) {
    scope.querySelectorAll(`[${upstreamBranchOptionAttribute}], .codex-upstream-branch-group`)
      .forEach((node) => node.remove());
  }

  function cleanupInvalidUpstreamBranchOptions() {
    for (const menu of nativeBranchMenuCandidates()) {
      if (!menu.querySelector(`[${upstreamBranchOptionAttribute}], .codex-upstream-branch-group`)) continue;
      const trigger = branchMenuTriggerFromMenu(menu);
      if (!looksLikeBranchMenu(menu, trigger) || !branchMenuInNewWorktreeMode(trigger)) {
        removeUpstreamBranchOptions(menu);
      }
    }
  }

  function branchMenuTriggerFromMenu(menu) {
    const labelledBy = menu?.getAttribute?.("aria-labelledby") || "";
    if (labelledBy) {
      const trigger = document.getElementById(labelledBy);
      if (trigger instanceof Element) return trigger;
    }
    return [...document.querySelectorAll('button')]
      .filter((button) => (button.innerText || button.textContent || "").trim() === "main")
      .sort((left, right) => right.getBoundingClientRect().x - left.getBoundingClientRect().x)[0] || null;
  }

  function branchMenuTriggerIsBranchControl(trigger) {
    const text = normalizedElementText(trigger);
    if (!text || /^(work locally|new worktree|cloud|no environment)$/i.test(text)) return false;
    const rect = effectiveElementRect(trigger);
    const footer = trigger?.closest?.(".composer-footer");
    if (!rect || !footer) return /branch|main|create branch/i.test(text);
    const modeTrigger = [...footer.querySelectorAll('button, [role="button"]')]
      .filter((node) => node !== trigger && visibleElement(node))
      .filter((node) => node.getBoundingClientRect().x < rect.x)
      .sort((left, right) => right.getBoundingClientRect().x - left.getBoundingClientRect().x)
      .find((node) => /^(work locally|new worktree|cloud)$/i.test(normalizedElementText(node)));
    return !!modeTrigger;
  }

  function branchMenuInNewWorktreeMode(trigger) {
    if (!trigger) return newWorktreeModeActive();
    const footer = trigger.closest?.(".composer-footer");
    const scope = footer || trigger.parentElement || document;
    const triggerRect = effectiveElementRect(trigger);
    if (!triggerRect) return false;
    const modeTrigger = [...scope.querySelectorAll('button, [role="button"]')]
      .filter((node) => node !== trigger && visibleElement(node))
      .filter((node) => node.getBoundingClientRect().x < triggerRect.x)
      .sort((left, right) => right.getBoundingClientRect().x - left.getBoundingClientRect().x)
      .find((node) => /worktree|work locally/i.test(normalizedElementText(node)));
    return normalizedElementText(modeTrigger) === "New worktree";
  }

  function branchTriggerLabelNode(trigger) {
    if (!trigger) return null;
    const nodes = [...trigger.querySelectorAll("span, div")]
      .filter((node) => (node.innerText || node.textContent || "").trim());
    return nodes.find((node) => node.classList?.contains("composer-footer__label--sm")) || nodes[0] || trigger;
  }

  function ensureNativeBranchTriggerLabel(trigger) {
    if (!trigger || trigger.querySelector?.('[data-codex-upstream-branch-selection-label="true"]')) return;
    const labelNode = branchTriggerLabelNode(trigger);
    if (!labelNode) return;
    trigger.setAttribute("data-codex-upstream-branch-trigger", "true");
    labelNode.setAttribute("data-codex-native-branch-label", "true");
    const selectionLabel = document.createElement("span");
    selectionLabel.setAttribute("data-codex-upstream-branch-selection-label", "true");
    selectionLabel.className = labelNode.className || "composer-footer__label--sm composer-footer__secondary-label max-w-40 truncate";
    selectionLabel.hidden = true;
    labelNode.insertAdjacentElement("afterend", selectionLabel);
  }

  function clearUpstreamBranchTriggerLabel() {
    document.querySelectorAll('[data-codex-upstream-branch-trigger="true"]').forEach((trigger) => {
      const nativeLabel = trigger.querySelector('[data-codex-native-branch-label="true"]');
      const selectionLabel = trigger.querySelector('[data-codex-upstream-branch-selection-label="true"]');
      if (nativeLabel) nativeLabel.hidden = false;
      if (selectionLabel) selectionLabel.hidden = true;
      trigger.removeAttribute("aria-label");
      trigger.removeAttribute("data-codex-tooltip");
      trigger.removeAttribute("title");
    });
  }

  function syncUpstreamBranchTriggerLabel() {
    const selection = readUpstreamBranchSelection();
    if (!selection?.label) {
      clearUpstreamBranchTriggerLabel();
      return;
    }
    document.querySelectorAll('[data-codex-upstream-branch-trigger="true"]').forEach((trigger) => {
      const nativeLabel = trigger.querySelector('[data-codex-native-branch-label="true"]');
      const selectionLabel = trigger.querySelector('[data-codex-upstream-branch-selection-label="true"]');
      if (!selectionLabel) return;
      if (nativeLabel) nativeLabel.hidden = true;
      selectionLabel.hidden = false;
      selectionLabel.textContent = selection.label;
      trigger.setAttribute("aria-label", selection.label);
      trigger.setAttribute("data-codex-tooltip", selection.label);
      trigger.removeAttribute("title");
    });
  }

  function handleNativeBranchSelection(event) {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const menuItem = target?.closest?.('[role="menuitem"], [data-radix-collection-item]');
    if (!menuItem || menuItem.closest?.(`[${upstreamBranchOptionAttribute}]`)) return;
    const menu = menuItem.closest?.('[role="menu"], [data-radix-menu-content]');
    if (!menu || !looksLikeBranchMenu(menu)) return;
    const text = (menuItem.innerText || menuItem.textContent || "").replace(/\s+/g, " ").trim();
    if (!text || /^branches$/i.test(text) || /^upstream$/i.test(text) || text === readUpstreamBranchSelection()?.label) return;
    const usedWorktreePath = branchWorktreePathFromMenuItem(menuItem);
    writeUpstreamBranchSelection(null);
    clearUpstreamBranchTriggerLabel();
    syncUpstreamBranchMenuSelection(menu);
    if (usedWorktreePath) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation?.();
      showToast(`该分支已在另一个 worktree 使用：${usedWorktreePath}`);
    }
  }

  async function injectUpstreamBranchOptions() {
    if (!codexElvesSettings().upstreamWorktreeCreate) {
      removeUpstreamBranchOptions();
      return;
    }
    cleanupInvalidUpstreamBranchOptions();
    for (const menu of nativeBranchMenuCandidates()) {
      const trigger = branchMenuTriggerFromMenu(menu);
      if (!looksLikeBranchMenu(menu, trigger)) continue;
      const context = currentProjectContextForBranchMenu(menu, trigger);
      if (!context?.repoPath && !context?.projectId) {
        removeUpstreamBranchOptions(menu);
        continue;
      }
      const defaults = await loadUpstreamBranchDefaults(context);
      const defaultsResult = defaults?.result;
      const refs = defaults?.result?.upstreamRefs || [];
      annotateBranchMenuWorktreeUsage(menu, defaultsResult);
      if (!branchMenuInNewWorktreeMode(trigger)) {
        removeUpstreamBranchOptions(menu);
        writeUpstreamBranchSelection(null);
        clearUpstreamBranchTriggerLabel();
        continue;
      }
      if (!refs.length) {
        removeUpstreamBranchOptions(menu);
        continue;
      }
      const resolvedContext = {
        repoPath: defaults?.repoPath || context.repoPath || defaultsResult?.repoRoot || "",
        projectId: defaults?.projectId || context.projectId || "",
      };
      if (upstreamBranchOptionsMatchRefs(menu, resolvedContext, refs)) {
        syncUpstreamBranchTriggerLabel();
        syncUpstreamBranchMenuSelection(menu);
        continue;
      }
      removeUpstreamBranchOptions(menu);
      ensureNativeBranchTriggerLabel(trigger);
      const group = document.createElement("div");
      group.className = "codex-upstream-branch-group px-2 py-1 text-xs text-token-text-tertiary";
      group.textContent = "Upstream";
      menu.appendChild(group);
      refs.forEach((ref) => renderUpstreamBranchOption(menu, resolvedContext, ref));
      syncUpstreamBranchTriggerLabel();
      syncUpstreamBranchMenuSelection(menu);
    }
  }

  function installUpstreamBranchDropdownAdapter() {
    const adapterVersion = "actual-upstream-refs-v16";
    window.__codexUpstreamBranchDropdownAdapterVersion = adapterVersion;
    if (!codexElvesSettings().upstreamWorktreeCreate) {
      clearTimeout(window.__codexUpstreamBranchDropdownInjectTimer);
      window.__codexUpstreamBranchDropdownInjectTimer = null;
      window.__codexUpstreamBranchDropdownObserver?.disconnect?.();
      window.__codexUpstreamBranchDropdownObserver = null;
      document.removeEventListener("click", window.__codexUpstreamBranchDropdownClickHandler, true);
      nativeBranchMenuCandidates().forEach(removeUpstreamBranchOptions);
      cleanupInvalidUpstreamBranchOptions();
      writeUpstreamBranchSelection(null);
      clearUpstreamBranchTriggerLabel();
      window.__codexUpstreamBranchDropdownAdapterInstalled = null;
      return;
    }
    if (window.__codexUpstreamBranchDropdownAdapterInstalled === adapterVersion) return;
    window.__codexUpstreamBranchDropdownAdapterInstalled = adapterVersion;
    document.removeEventListener("click", window.__codexUpstreamBranchDropdownClickHandler, true);
    window.__codexUpstreamBranchDropdownClickHandler = (event) => {
      rememberStartNewChatProjectContext(event);
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      const option = target?.closest?.(`[${upstreamBranchOptionAttribute}]`);
      if (!option) {
        handleNativeBranchSelection(event);
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const selection = {
        repoPath: option.getAttribute("data-repo-path") || "",
        projectId: option.getAttribute("data-project-id") || "",
        remote: option.getAttribute("data-remote") || "upstream",
        baseBranch: option.getAttribute("data-base-branch") || "main",
        label: upstreamBranchOptionLabel(option) || "upstream/main",
      };
      writeUpstreamBranchSelection(selection);
      prepareUpstreamBranchSelection(selection);
      syncUpstreamBranchTriggerLabel();
      syncUpstreamBranchMenuSelection(option.closest?.('[role="menu"], [data-radix-menu-content], [cmdk-list]'));
      showToast(`将从 ${upstreamBranchOptionLabel(option) || "upstream/main"} 创建新 worktree`);
    };
    document.addEventListener("click", window.__codexUpstreamBranchDropdownClickHandler, true);
    const schedule = () => {
      clearTimeout(window.__codexUpstreamBranchDropdownInjectTimer);
      window.__codexUpstreamBranchDropdownInjectTimer = setTimeout(() => {
        if (!codexElvesSettings().upstreamWorktreeCreate) return;
        injectUpstreamBranchOptions().catch((error) => reportDiagnostic("upstream_branch_inject_failed", { error: error?.message || String(error) }));
      }, 80);
    };
    window.__codexUpstreamBranchDropdownObserver?.disconnect?.();
    window.__codexUpstreamBranchDropdownObserver = new MutationObserver(schedule);
    window.__codexUpstreamBranchDropdownObserver.observe(document.body || document.documentElement, { childList: true, subtree: true });
    schedule();
  }

  function refreshUpstreamBranchDropdownAdapter() {
    installUpstreamBranchDropdownAdapter();
  }

  function upstreamQualifiedSourceRef(selection) {
    if (selection?.qualifiedSourceRef) return selection.qualifiedSourceRef;
    const remote = (selection?.remote || "upstream").trim();
    const baseBranch = (selection?.baseBranch || "main").trim();
    return remote && baseBranch ? `refs/remotes/${remote}/${baseBranch}` : "";
  }

  function prepareUpstreamBranchSelection(selection) {
    if ((!selection?.repoPath && !selection?.projectId) || !selection.remote || !selection.baseBranch) return;
    void postJson("/upstream-worktree/prepare", {
      repoPath: selection.repoPath || "",
      projectId: selection.projectId || "",
      remote: selection.remote,
      baseBranch: selection.baseBranch,
      fetch: true,
    }).then((result) => {
      if (result?.status !== "ok") throw new Error(result?.message || "prepare failed");
      writePreparedUpstreamBranchSelection(selection, result);
    }).catch((error) => {
      sendCodexElvesDiagnostic("upstream_branch_prepare_failed", {
        label: selection.label || "",
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
    });
  }

  function writePreparedUpstreamBranchSelection(selection, result) {
    const current = readUpstreamBranchSelection();
    if (!upstreamSelectionMatches(current, selection)) return;
    writeUpstreamBranchSelection({
      ...current,
      qualifiedSourceRef: result.qualifiedSourceRef || upstreamQualifiedSourceRef(selection),
      sourceHead: result.sourceHead || "",
      preparedAt: Date.now(),
    });
  }

  function upstreamSelectionMatches(left, right) {
    return !!left && !!right
      && (left.repoPath || "") === (right.repoPath || "")
      && (left.projectId || "") === (right.projectId || "")
      && (left.remote || "upstream") === (right.remote || "upstream")
      && (left.baseBranch || "main") === (right.baseBranch || "main");
  }

  function pendingWorktreeRequestMatchesSelection(request, selection) {
    if (!selection || !request || request.launchMode !== "start-conversation") return false;
    const sourceRoot = request.sourceWorkspaceRoot || "";
    if (selection.repoPath && sourceRoot) return sameWorkspacePath(sourceRoot, selection.repoPath);
    if (selection.projectId) return true;
    return !selection.repoPath || sameWorkspacePath(sourceRoot, selection.repoPath);
  }

  function applyUpstreamPendingWorktreeOverride(payload) {
    const selection = readUpstreamBranchSelection();
    const request = payload?.request;
    const sourceRef = upstreamQualifiedSourceRef(selection);
    if (!codexElvesSettings().upstreamWorktreeCreate || !sourceRef) return payload;
    if (!pendingWorktreeRequestMatchesSelection(request, selection)) return payload;
    if (request?.startingState?.type !== "branch") return payload;
    if (request.startingState.branchName === sourceRef) return payload;
    const nextRequest = {
      ...request,
      startingState: { ...request.startingState, branchName: sourceRef },
    };
    prepareUpstreamBranchSelection(selection);
    sendCodexElvesDiagnostic("upstream_pending_worktree_override_applied", {
      label: selection.label || "",
      sourceRef,
      sourceWorkspaceRoot: request.sourceWorkspaceRoot || "",
    });
    return { ...(payload || {}), request: nextRequest };
  }

  function installUpstreamPendingWorktreeDispatcherPatch() {
    const patchVersion = "1";
    if (window.__codexUpstreamPendingWorktreeDispatcherPatch === patchVersion) return;
    const patch = async () => {
      try {
        const module = await loadCodexAppModule("setting-storage-");
        const dispatcherClass = typeof module.v === "function" && String(module.v).includes("dispatchMessage") ? module.v : null;
        const dispatcher = dispatcherClass?.getInstance?.();
        if (!dispatcher || typeof dispatcher.dispatchMessage !== "function") throw new Error("Codex dispatcher unavailable");
        if (!dispatcher.__codexUpstreamWorktreeOriginalDispatchMessage) {
          dispatcher.__codexUpstreamWorktreeOriginalDispatchMessage = dispatcher.dispatchMessage.bind(dispatcher);
          dispatcher.dispatchMessage = (type, payload) => {
            const nextPayload = type === "pending-worktree-create"
              ? applyUpstreamPendingWorktreeOverride(payload)
              : payload;
            return dispatcher.__codexUpstreamWorktreeOriginalDispatchMessage(type, nextPayload);
          };
        }
        window.__codexUpstreamPendingWorktreeDispatcherPatch = patchVersion;
      } catch (error) {
        sendCodexElvesDiagnostic("upstream_pending_worktree_patch_failed", {
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
        });
      }
    };
    void patch();
  }

  function upstreamWorktreeNativePayloadFromElement(element) {
    const trigger = element?.closest?.("[data-codex-worktree-create], [data-worktree-create]") || element;
    const scopes = [
      trigger,
      trigger?.closest?.("form"),
      trigger?.closest?.("dialog, [role='dialog']"),
    ].filter((scope, index, all) => scope?.querySelector && all.indexOf(scope) === index);
    if (!scopes.length) return null;
    const valueFrom = (selectors) => {
      for (const scope of scopes) {
        for (const selector of selectors) {
          const node = scope.matches?.(selector) ? scope : scope.querySelector(selector);
          const dataAttribute = selector.match(/^\[([a-z0-9-]+)\]$/i)?.[1] || "";
          const value = node?.value || node?.getAttribute?.(dataAttribute) || node?.getAttribute?.("data-value") || node?.textContent || "";
          if (String(value).trim()) return String(value).trim();
        }
      }
      return "";
    };
    const repoPath = valueFrom(["[data-repo-path]", "[name='repoPath']", "[name='repo']"]);
    const branchName = valueFrom(["[data-branch-name]", "[name='branchName']", "[name='branch']"]);
    const worktreePath = valueFrom(["[data-worktree-path]", "[name='worktreePath']", "[name='path']"]);
    const remote = valueFrom(["[data-remote]", "[name='remote']"]) || "upstream";
    const baseBranch = valueFrom(["[data-base-branch]", "[name='baseBranch']", "[name='base']"]) || "main";
    if (!repoPath || !branchName || !worktreePath || !remote || !baseBranch) return null;
    return { repoPath, branchName, worktreePath, remote, baseBranch, fetch: true };
  }

  function upstreamWorktreePayloadFromSelection(trigger) {
    const selection = readUpstreamBranchSelection();
    if ((!selection?.repoPath && !selection?.projectId) || !selection?.remote || !selection?.baseBranch) return null;
    const nativePayload = upstreamWorktreeNativePayloadFromElement(trigger);
    if (!nativePayload?.branchName || !nativePayload?.worktreePath) return null;
    return {
      ...nativePayload,
      repoPath: selection.repoPath,
      projectId: selection.projectId || "",
      remote: selection.remote,
      baseBranch: selection.baseBranch,
      fetch: true,
    };
  }

  async function handleUpstreamWorktreeNativeCreate(event) {
    if (!codexElvesSettings().upstreamWorktreeCreate) return false;
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const trigger = target?.closest?.("[data-codex-worktree-create], [data-worktree-create]");
    if (!trigger) return false;
    const payload = upstreamWorktreePayloadFromSelection(trigger) || upstreamWorktreeNativePayloadFromElement(trigger);
    if (!payload) {
      showToast("无法安全识别 Codex 原生 worktree 表单，请使用 CodexElves 菜单创建。");
      return false;
    }
    event.preventDefault();
    event.stopPropagation();
    try {
      const result = await postJson("/upstream-worktree/create", payload);
      if (result?.status === "ok") {
        writeUpstreamBranchSelection(null);
        syncUpstreamBranchTriggerLabel();
        showToast(`已从 ${result.sourceRef} 创建 worktree`);
      } else {
        showToast(result?.message || "创建 upstream worktree 失败");
      }
    } catch (error) {
      showToast(error?.message || "创建 upstream worktree 失败");
    }
    return true;
  }

  function installUpstreamWorktreeNativeAdapter() {
    const adapterVersion = "2";
    installUpstreamPendingWorktreeDispatcherPatch();
    if (window.__codexUpstreamWorktreeNativeAdapterInstalled === adapterVersion) return;
    window.__codexUpstreamWorktreeNativeAdapterInstalled = adapterVersion;
    document.addEventListener("click", (event) => {
      handleUpstreamWorktreeNativeCreate(event);
    }, true);
  }

  function setUpstreamWorktreeMessage(dialog, message, status = "idle") {
    const messageNode = dialog.querySelector("[data-codex-upstream-worktree-message]");
    if (!messageNode) return;
    messageNode.dataset.status = status;
    messageNode.textContent = message || "";
  }

  async function loadUpstreamWorktreeDefaults(dialog) {
    const repoPath = upstreamWorktreeField(dialog, "repoPath")?.value?.trim() || "";
    if (!repoPath) {
      setUpstreamWorktreeMessage(dialog, "填写仓库路径后会自动读取 remote 和当前分支。", "idle");
      return;
    }
    setUpstreamWorktreeMessage(dialog, "正在读取仓库默认值…", "loading");
    try {
      const result = await postJson("/upstream-worktree/defaults", { repoPath });
      if (result?.status !== "ok") {
        setUpstreamWorktreeMessage(dialog, result?.message || "读取仓库默认值失败", "failed");
        return;
      }
      const remote = upstreamWorktreeField(dialog, "remote");
      const baseBranch = upstreamWorktreeField(dialog, "baseBranch");
      if (remote && !remote.value) remote.value = result.defaultRemote || "upstream";
      if (baseBranch && (!baseBranch.value || baseBranch.value === "main")) baseBranch.value = result.defaultBaseBranch || "main";
      setUpstreamWorktreeMessage(dialog, `将从 ${remote?.value || "upstream"}/${baseBranch?.value || "main"} 创建 worktree。`, "ok");
    } catch (error) {
      setUpstreamWorktreeMessage(dialog, error?.message || "读取仓库默认值失败", "failed");
    }
  }

  async function submitUpstreamWorktree(dialog) {
    const payload = upstreamWorktreePayload(dialog);
    if (!payload.repoPath || !payload.branchName || !payload.worktreePath || !payload.remote || !payload.baseBranch) {
      setUpstreamWorktreeMessage(dialog, "仓库路径、分支名、worktree 路径、remote 和 base branch 都必须填写。", "failed");
      return;
    }
    setUpstreamWorktreeMessage(dialog, "正在 fetch 并创建 worktree…", "loading");
    try {
      const result = await postJson("/upstream-worktree/create", payload);
      if (result?.status === "ok") {
        setUpstreamWorktreeMessage(dialog, `已从 ${result.sourceRef} 创建：${result.worktreePath}`, "ok");
        showToast(`已创建 upstream worktree：${result.branchName}`);
      } else {
        setUpstreamWorktreeMessage(dialog, result?.message || "创建 upstream worktree 失败", "failed");
      }
    } catch (error) {
      setUpstreamWorktreeMessage(dialog, error?.message || "创建 upstream worktree 失败", "failed");
    }
  }

  function openUpstreamWorktreeDialog() {
    document.querySelectorAll(`.${upstreamWorktreeDialogClass}`).forEach((node) => node.remove());
    const overlay = document.createElement("div");
    overlay.className = `codex-delete-confirm-overlay ${upstreamWorktreeDialogClass}`;
    overlay.innerHTML = `
      <div class="codex-delete-confirm-content" role="dialog" aria-modal="true" aria-label="Create upstream worktree">
        <div class="codex-delete-confirm-title">Create from upstream</div>
        <div class="codex-delete-confirm-message">等价于 git worktree add -b branch path upstream/base。创建前会先 fetch 远端分支。</div>
        <label class="codex-elves-form-field">仓库路径<input data-codex-upstream-worktree-field="repoPath" type="text" placeholder="/path/to/repo"></label>
        <label class="codex-elves-form-field">新分支名<input data-codex-upstream-worktree-field="branchName" type="text" placeholder="feature/my-task"></label>
        <label class="codex-elves-form-field">Worktree 路径<input data-codex-upstream-worktree-field="worktreePath" type="text" placeholder="/path/to/worktrees/my-task"></label>
        <label class="codex-elves-form-field">Remote<input data-codex-upstream-worktree-field="remote" type="text" value="upstream"></label>
        <label class="codex-elves-form-field">Base branch<input data-codex-upstream-worktree-field="baseBranch" type="text" value="main"></label>
        <div class="codex-elves-form-message" data-codex-upstream-worktree-message>填写仓库路径后会自动读取 remote 和当前分支。</div>
        <div class="codex-delete-confirm-actions">
          <button type="button" data-codex-upstream-worktree-cancel="true">取消</button>
          <button type="button" data-codex-upstream-worktree-defaults="true">读取默认值</button>
          <button type="button" data-codex-upstream-worktree-submit="true">Create from upstream</button>
        </div>
      </div>
    `;
    overlay.addEventListener("click", (event) => {
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (event.target === overlay || target?.closest("[data-codex-upstream-worktree-cancel]")) {
        overlay.remove();
        return;
      }
      if (target?.closest("[data-codex-upstream-worktree-defaults]")) {
        loadUpstreamWorktreeDefaults(overlay);
        return;
      }
      if (target?.closest("[data-codex-upstream-worktree-submit]")) {
        submitUpstreamWorktree(overlay);
      }
    }, true);
    upstreamWorktreeField(overlay, "repoPath")?.addEventListener("change", () => loadUpstreamWorktreeDefaults(overlay));
    document.body.appendChild(overlay);
    upstreamWorktreeField(overlay, "repoPath")?.focus();
  }

  function escapeHtml(value) {
    return String(value)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#39;");
  }

  function confirmDelete(title) {
    document.querySelectorAll(".codex-delete-confirm-overlay").forEach((node) => node.remove());
    return new Promise((resolve) => {
      const overlay = document.createElement("div");
      overlay.className = "codex-delete-confirm-overlay";
      overlay.innerHTML = `
        <div class="codex-delete-confirm-content" role="dialog" aria-modal="true" aria-label="删除会话">
          <div class="codex-delete-confirm-title">删除会话</div>
          <div class="codex-delete-confirm-message">删除“${escapeHtml(title)}”？</div>
          <div class="codex-delete-confirm-actions">
            <button type="button" data-codex-delete-cancel="true">取消</button>
            <button type="button" data-codex-delete-confirm="true">删除</button>
          </div>
        </div>
      `;
      const finish = (value, event) => {
        event?.preventDefault();
        event?.stopPropagation();
        event?.target?.blur?.();
        overlay.remove();
        resolve(value);
      };
      overlay.addEventListener("click", (event) => {
        if (event.target === overlay || event.target.closest("[data-codex-delete-cancel]")) {
          finish(false, event);
          return;
        }
        if (event.target.closest("[data-codex-delete-confirm]")) {
          finish(true, event);
        }
      }, true);
      overlay.addEventListener("keydown", (event) => {
        if (event.key === "Escape") finish(false, event);
      }, true);
      document.body.appendChild(overlay);
      overlay.querySelector("[data-codex-delete-cancel]")?.focus();
    });
  }

  function rowHref(row) {
    return row.getAttribute("href") || row.querySelector("a")?.getAttribute("href") || "";
  }

  function isCurrentSessionRow(row, ref) {
    if (row.getAttribute("aria-current") === "page" || row.getAttribute("aria-current") === "true") return true;
    const href = rowHref(row);
    if (href) {
      try {
        const url = new URL(href, window.location.href);
        if (url.href === window.location.href || url.pathname === window.location.pathname) return true;
      } catch {
        if (window.location.href.includes(href)) return true;
      }
    }
    return !!ref.session_id && window.location.href.includes(ref.session_id);
  }

  function releaseDeleteFocus(row, button) {
    button.blur();
    if (row.contains(document.activeElement)) {
      document.activeElement.blur();
    }
  }

  function removeDeletedRow(row, button, ref, archived = false) {
    releaseDeleteFocus(row, button);
    const shouldReload = isCurrentSessionRow(row, ref);
    // 把会话加入持久抑制集（不依赖 Codex 内部 manager），
    // 确保展开项目、折叠/展开重渲染时不复现已删除会话。
    if (ref && ref.session_id) suppressThreadEverywhere(ref.session_id);
    row.remove();
    if (shouldReload) {
      window.location.reload();
    }
  }

  function updateDeleteButtonOffsets(rows = sessionRows()) {
    const measurements = Array.from(rows || []).filter((row) => row?.isConnected).map((row) => {
      const rowRect = row.getBoundingClientRect();
      const hasArchiveConfirm = Array.from(row.querySelectorAll("button")).some((button) => {
        const rect = button.getBoundingClientRect();
        const label = button.getAttribute("aria-label") || "";
        const text = (button.textContent || "").trim();
        if (button.classList.contains(buttonClass) || button.classList.contains(exportButtonClass) || label === "归档对话" || label === "置顶对话") return false;
        return text === "确认" || (text.length > 0 && rect.width > 0 && rect.width <= 36 && rect.x > rowRect.right - 50);
      });
      return { row, hasArchiveConfirm };
    });
    measurements.forEach(({ row, hasArchiveConfirm }) => {
      row.classList.toggle("codex-archive-confirm-visible", hasArchiveConfirm);
    });
  }

  function openDeleteConfirmForRow(row, button, ref, event) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
    releaseDeleteFocus(row, button);
    confirmDelete(ref.title).then(async (confirmed) => {
      if (!confirmed) return;
      releaseDeleteFocus(row, button);
      // 先删数据，再由 removeDeletedRow 加入持久抑制集（不依赖 Codex 内部 manager）。
      const result = await postJson("/delete", ref);
      if (result.status === "server_deleted" || result.status === "local_deleted" || result.status === "partial") {
        removeDeletedRow(row, button, ref);
        // partial 表示数据库记录已经删除，仅有 rollout 文件清理失败；仍需抑制残留列表行。
        showToast(result.message || (result.status === "partial" ? "数据库已删除，但部分文件清理失败" : "删除成功"));
      } else if (result.status === "not_found") {
        // 会话在本地存储中已不存在，目标（会话不存在）已达成，直接移除残留的列表行
        removeDeletedRow(row, button, ref);
        showToast(result.message || "会话已不存在，已从列表移除");
      } else {
        showToast(result.message || "删除失败");
      }
    });
  }

  async function exportMarkdown(ref) {
    const result = await postJson("/export-markdown", ref);
    if (result.status === "exported" && result.filename && typeof result.markdown === "string") {
      const saveResult = await saveMarkdown(result.filename, result.markdown);
      if (saveResult?.status === "cancelled") {
        showToast(saveResult.message || "导出已取消");
      } else {
        showToast(result.message || "导出成功");
      }
      return;
    }
    showToast(result.message || "导出失败");
  }

  function sortStateFromMoveResult(result, ref, row) {
    const trustedSortMs = timestampMsFromPayload(result);
    return { sortMs: trustedSortMs || rowSortMs(row, ref), sortMsTrusted: !!trustedSortMs };
  }

  function finishProjectMove(row, button, ref, target, message) {
    releaseDeleteFocus(row, button);
    button.disabled = false;
    button.textContent = "移动";
    saveProjectMoveProjection(ref, target, target.sortMs || rowSortMs(row, ref, target));
    if (target.kind === "projectless") moveRowToChats(row, target);
    refreshAfterProjectMove();
    showToast(message);
  }

  async function applyProjectMove(row, button, ref, target) {
    button.disabled = true;
    button.textContent = "移动中";
    try {
      if (target.kind === "projectless") {
        const result = await moveSessionToProjectless(ref);
        finishProjectMove(row, button, ref, { ...target, ...sortStateFromMoveResult(result, ref, row) }, `已移动到普通对话：“${ref.title || ref.session_id}”`);
      } else {
        const result = await moveSessionToProject(ref, target);
        finishProjectMove(row, button, ref, { ...target, ...sortStateFromMoveResult(result, ref, row) }, `已移动到“${target.label}”：“${ref.title || ref.session_id}”`);
      }
    } catch (error) {
      button.disabled = false;
      button.textContent = "移动";
      showToast(`移动失败：${error?.message || error}`);
    }
  }

  async function openProjectMoveMenuForRow(row, button, ref, event) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
    releaseDeleteFocus(row, button);
    document.querySelectorAll(`.${projectMoveOverlayClass}`).forEach((node) => node.remove());
    const overlay = document.createElement("div");
    overlay.className = projectMoveOverlayClass;
    overlay.innerHTML = `
      <div class="codex-project-move-panel" role="dialog" aria-modal="true" aria-label="移动对话">
        <div class="codex-project-move-header">
          <div class="codex-project-move-title">移动“${escapeHtml(ref.title || ref.session_id)}”</div>
        </div>
        <div class="codex-project-move-list"><div class="codex-project-move-empty">加载项目中...</div></div>
      </div>
    `;
    const panel = overlay.querySelector(".codex-project-move-panel");
    const rect = button.getBoundingClientRect();
    const panelWidth = Math.min(360, Math.max(240, window.innerWidth - 32));
    panel.style.left = `${Math.max(16, Math.min(window.innerWidth - panelWidth - 16, rect.right - panelWidth))}px`;
    panel.style.top = `${Math.max(16, Math.min(window.innerHeight - 120, rect.bottom + 6))}px`;
    const close = () => overlay.remove();
    overlay.addEventListener("click", (clickEvent) => {
      if (clickEvent.target === overlay) close();
    }, true);
    overlay.addEventListener("keydown", (keyEvent) => {
      if (keyEvent.key === "Escape") {
        keyEvent.preventDefault();
        close();
      }
    }, true);
    document.body.appendChild(overlay);
    try {
      const targets = projectMoveTargets();
      const list = overlay.querySelector(".codex-project-move-list");
      if (!list) return;
      list.innerHTML = "";
      if (targets.length === 0) {
        list.innerHTML = `<div class="codex-project-move-empty">没有可用目标</div>`;
        return;
      }
      for (const target of targets) {
        const item = document.createElement("button");
        item.type = "button";
        item.className = "codex-project-move-item";
        item.innerHTML = `
          <div class="codex-project-move-item-title">${escapeHtml(target.label)}</div>
          <div class="codex-project-move-item-path">${escapeHtml(target.description)}</div>
        `;
        item.addEventListener("click", async (selectEvent) => {
          selectEvent.preventDefault();
          selectEvent.stopPropagation();
          close();
          await applyProjectMove(row, button, ref, target);
        }, true);
        list.appendChild(item);
      }
      list.querySelector("button")?.focus();
    } catch (error) {
      close();
      showToast(`加载项目失败：${error?.message || error}`);
    }
  }

  function installDeleteButtonEventDelegation() {
    document.removeEventListener("pointerup", window.__codexSessionDeleteDocumentDeleteHandler, true);
    document.removeEventListener("click", window.__codexSessionDeleteDocumentDeleteHandler, true);
    const handler = (event) => {
      const button = event.target?.closest?.(`.${buttonClass}`);
      const row = button?.closest?.("[data-app-action-sidebar-thread-id]");
      if (!button || !row) return;
      const ref = sessionRefFromRow(row);
      if (!ref.session_id) return;
      openDeleteConfirmForRow(row, button, ref, event);
    };
    window.__codexSessionDeleteDocumentDeleteHandler = handler;
    document.addEventListener("pointerup", handler, true);
    document.addEventListener("click", handler, true);
  }

  function actionGroupFromRow(row) {
    return row.querySelector(`.${actionGroupClass}`);
  }

  function nativeActionContentsFromRow(row) {
    return Array.from(row?.children || []).find((node) =>
      node.matches?.('div.contents[data-hover-card-open-immediately="true"]')
    ) || null;
  }

  function nativeActionHostFromRow(row) {
    const contents = nativeActionContentsFromRow(row);
    if (!contents) return null;
    return Array.from(contents.children).find((node) => {
      if (!(node instanceof HTMLElement) || !node.querySelector("button")) return false;
      const style = getComputedStyle(node);
      return style.position === "absolute" && (style.right === "0px" || classNameText(node).includes("right-0"));
    }) || null;
  }

  function nativeActionButtonClassFromHost(host) {
    const nativeButton = Array.from(host?.querySelectorAll?.("button") || [])
      .find((button) => !button.closest(`.${actionGroupClass}`));
    return String(nativeButton?.className || "").trim();
  }

  function sessionActionButtonClassName(nativeHost, featureClass) {
    return [
      nativeActionButtonClassFromHost(nativeHost),
      actionButtonClass,
      featureClass,
    ].filter(Boolean).join(" ");
  }

  function nativeActionButtonsFromRow(row, rowRect) {
    return [...row.querySelectorAll('button,[role="button"],a')]
      .filter((node) => !node.closest(`.${actionGroupClass}`))
      .filter((node) => {
        const rect = node.getBoundingClientRect();
        if (rect.width < 12 || rect.height < 12) return false;
        const label = [
          node.getAttribute("aria-label"),
          node.getAttribute("title"),
          node.dataset?.state,
          node.textContent,
        ]
          .filter(Boolean)
          .join(" ")
          .toLowerCase();
        if (/(pin|archive|置顶|归档)/i.test(label)) return true;
        return rect.left > rowRect.left + rowRect.width * 0.68;
      });
  }

  function measureActionGroupLayout(row, group) {
    if (!row?.isConnected || !group?.isConnected) return null;
    const rowRect = row.getBoundingClientRect();
    const nativePlacement = group.dataset.codexActionPlacement === "native";
    const nativeHost = nativePlacement ? group.parentElement : null;
    const layoutKey = nativePlacement
      ? `native:${nativeHost?.children?.length || 0}:${Math.round(rowRect.width)}`
      : `fallback:${Math.round(rowRect.width)}`;
    if (
      group.dataset.codexActionLayoutStable === "true" &&
      group.dataset.codexActionLayoutKey === layoutKey
    ) return null;

    const titleNode = row.querySelector(selectors.threadTitle);
    const titleRect = titleNode?.getBoundingClientRect();
    if (nativePlacement && nativeHost) {
      const hostRect = nativeHost.getBoundingClientRect();
      return {
        row,
        group,
        layoutKey,
        nativePlacement: true,
        maxTitleWidth: titleRect && hostRect.width > 0
          ? Math.max(24, Math.floor(hostRect.left - titleRect.left))
          : null,
      };
    }

    const nativeButtons = nativeActionButtonsFromRow(row, rowRect);
    const leftmostNative = nativeButtons
      .map((button) => button.getBoundingClientRect())
      .filter((rect) => rect.width > 0 && rect.height > 0)
      .sort((a, b) => a.left - b.left)[0];
    const gap = 8;
    const fallbackRight = 28;
    const right = leftmostNative
      ? Math.max(fallbackRight, Math.round(rowRect.right - leftmostNative.left + gap))
      : fallbackRight;
    const groupWidth = Math.ceil(group.getBoundingClientRect().width || 96);
    const titleLeft = titleRect?.left || rowRect.left + 40;
    const maxTitleWidth = Math.max(24, Math.round(rowRect.width - (titleLeft - rowRect.left) - right - groupWidth - 14));
    return {
      row,
      group,
      layoutKey,
      nativePlacement: false,
      right,
      groupWidth,
      maxTitleWidth,
    };
  }

  function applyActionGroupLayout(measurement) {
    if (!measurement?.row?.isConnected || !measurement?.group?.isConnected) return;
    const { row, group, layoutKey } = measurement;
    if (measurement.nativePlacement) {
      if (measurement.maxTitleWidth == null) {
        row.style.removeProperty("--codex-session-title-max-width");
      } else {
        row.style.setProperty("--codex-session-title-max-width", `${measurement.maxTitleWidth}px`);
      }
      row.style.removeProperty("--codex-session-title-mask");
      group.style.removeProperty("--codex-session-actions-right");
    } else {
      group.style.setProperty("--codex-session-actions-right", `${measurement.right}px`);
      row.style.setProperty("--codex-session-title-mask", `${measurement.right + measurement.groupWidth + 12}px`);
      row.style.setProperty("--codex-session-title-max-width", `${measurement.maxTitleWidth}px`);
    }
    group.dataset.codexActionLayoutKey = layoutKey;
    group.dataset.codexActionLayoutStable = "true";
  }

  function syncActionGroupsLayout(rows = sessionRows()) {
    const measurements = Array.from(rows || []).map((row) => {
      const group = actionGroupFromRow(row);
      return group ? measureActionGroupLayout(row, group) : null;
    }).filter(Boolean);
    measurements.forEach(applyActionGroupLayout);
  }

  function scheduleSessionRowLayout(rows) {
    Array.from(rows || []).forEach((row) => {
      if (row?.isConnected) pendingSessionRowLayouts.add(row);
    });
    if (pendingSessionRowLayoutRafId) return;
    pendingSessionRowLayoutRafId = requestAnimationFrame(() => {
      pendingSessionRowLayoutRafId = 0;
      const rowsToLayout = Array.from(pendingSessionRowLayouts);
      pendingSessionRowLayouts.clear();
      syncActionGroupsLayout(rowsToLayout);
    });
  }

  function removeActionGroups(row) {
    document.querySelectorAll(`.${moreMenuClass}`).forEach((menu) => {
      if (menu.__codexSessionMoreRow === row) menu.remove();
    });
    row.querySelectorAll(`.${actionGroupClass}`).forEach((group) => {
      const host = group.parentElement;
      if (host?.dataset?.codexSessionActionHost === "true") {
        delete host.dataset.codexSessionActionHost;
      }
      group.remove();
    });
    row.style.removeProperty("--codex-session-title-mask");
    row.style.removeProperty("--codex-session-title-max-width");
  }

  function stopActionButtonEvent(row, button, event) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
    releaseDeleteFocus(row, button);
  }

  function installActionButtonEvents(row, button, onActivate) {
    ["pointerdown", "mousedown", "mouseup", "touchstart"].forEach((eventName) => {
      button.addEventListener(eventName, (event) => stopActionButtonEvent(row, button, event), true);
    });
    button.addEventListener("pointerenter", () => showActionButtonTooltip(button));
    button.addEventListener("pointerleave", hideActionButtonTooltip);
    button.addEventListener("focus", () => showActionButtonTooltip(button));
    button.addEventListener("blur", hideActionButtonTooltip);
    button.addEventListener("pointerup", onActivate, true);
    button.addEventListener("click", (event) => {
      hideActionButtonTooltip();
      onActivate(event);
    }, true);
  }

  function installMoreButtonEvents(row, button, onActivate) {
    ["pointerdown", "mousedown", "mouseup", "touchstart"].forEach((eventName) => {
      button.addEventListener(eventName, (event) => stopActionButtonEvent(row, button, event), true);
    });
    button.addEventListener("pointerenter", () => showActionButtonTooltip(button));
    button.addEventListener("pointerleave", hideActionButtonTooltip);
    button.addEventListener("focus", () => showActionButtonTooltip(button));
    button.addEventListener("blur", hideActionButtonTooltip);
    button.addEventListener("pointerup", onActivate, true);
    button.addEventListener("click", (event) => {
      hideActionButtonTooltip();
      stopActionButtonEvent(row, button, event);
    }, true);
  }

  function hideActionButtonTooltip() {
    document.querySelectorAll(`.${actionTooltipClass}`).forEach((node) => node.remove());
  }

  function closeSessionMoreMenus(exceptMenu = null) {
    document.querySelectorAll(`.${moreMenuClass}`).forEach((menu) => {
      if (menu !== exceptMenu) {
        menu.hidden = true;
        menu.closest?.("[data-codex-delete-row]")?.classList.remove("codex-session-more-open");
        menu.__codexSessionMoreRow?.classList?.remove("codex-session-more-open");
      }
    });
  }

  function toggleSessionMoreMenu(row, button, menu) {
    const nextHidden = !menu.hidden;
    closeSessionMoreMenus(menu);
    menu.hidden = nextHidden;
    row.classList.toggle("codex-session-more-open", !menu.hidden);
    button.setAttribute("aria-expanded", String(!menu.hidden));
  }

  function installSessionMoreMenuAutoClose(row, menu) {
    const group = menu.__codexSessionMoreGroup || menu.closest?.(`.${actionGroupClass}`);
    const closeIfOutside = () => {
      window.setTimeout(() => {
        if (menu.hidden) return;
        const active = document.activeElement;
        if (group?.matches?.(":hover") || menu.matches?.(":hover") || menu.contains(active)) return;
        menu.hidden = true;
        row.classList.remove("codex-session-more-open");
        group?.querySelector?.(`.${moreButtonClass}`)?.setAttribute("aria-expanded", "false");
      }, 80);
    };
    group?.addEventListener("pointerleave", closeIfOutside, true);
    menu.addEventListener("pointerleave", closeIfOutside, true);
    menu.addEventListener("focusout", closeIfOutside, true);
  }

  function updateSessionMoreMenuDirection(button, menu) {
    menu.classList.remove("codex-session-more-menu-open-up");
    const buttonRect = button.getBoundingClientRect();
    const estimatedMenuHeight = Math.max(80, menu.getBoundingClientRect().height || 76);
    if (buttonRect.bottom + 30 + estimatedMenuHeight > window.innerHeight - 8) {
      menu.classList.add("codex-session-more-menu-open-up");
    }
  }

  function positionSessionMoreMenu(button, menu) {
    const rect = button.getBoundingClientRect();
    const menuWidth = Math.max(104, menu.getBoundingClientRect().width || 104);
    const left = Math.min(window.innerWidth - menuWidth - 8, Math.max(8, rect.right - menuWidth));
    menu.style.left = `${left}px`;
    menu.style.top = `${Math.max(8, rect.bottom + 4)}px`;
  }

  function createSessionMoreMenuItem(label, icon, onActivate) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "codex-session-more-menu-item";
    item.innerHTML = `<span class="codex-session-more-menu-icon">${icon}</span><span>${label}</span>`;
    item.addEventListener("click", onActivate, true);
    return item;
  }

  function showActionButtonTooltip(button) {
    const label = button.dataset.codexActionLabel || button.getAttribute("aria-label") || "";
    if (!label) return;
    hideActionButtonTooltip();
    const tooltip = document.createElement("div");
    tooltip.className = `${actionTooltipClass} z-50 w-fit select-none text-sm whitespace-normal break-words bg-token-dropdown-background text-token-foreground border-token-border rounded-lg border px-2 py-1`;
    tooltip.setAttribute("role", "tooltip");
    const content = document.createElement("div");
    content.className = "flex items-center gap-2";
    const text = document.createElement("div");
    text.className = "min-w-0";
    text.textContent = label;
    content.appendChild(text);
    tooltip.appendChild(content);
    document.body.appendChild(tooltip);
    const buttonRect = button.getBoundingClientRect();
    const tooltipRect = tooltip.getBoundingClientRect();
    const requestedGap = Number(button.dataset.codexTooltipGap);
    const gap = Number.isFinite(requestedGap) && requestedGap >= 0
      ? requestedGap
      : 3;
    const placement = button.dataset.codexTooltipPlacement || "";
    let left = 0;
    if (placement === "top-right") {
      const preferredLeft = buttonRect.right + gap;
      left = preferredLeft + tooltipRect.width <= window.innerWidth - 8
        ? preferredLeft
        : Math.max(8, buttonRect.left - tooltipRect.width - gap);
    } else {
      left = Math.min(
        window.innerWidth - tooltipRect.width - 8,
        Math.max(8, buttonRect.left + buttonRect.width / 2 - tooltipRect.width / 2),
      );
    }
    const aboveTop = buttonRect.top - tooltipRect.height - gap;
    const top = aboveTop >= 8
      ? aboveTop
      : Math.min(window.innerHeight - tooltipRect.height - 8, buttonRect.bottom + gap);
    tooltip.dataset.side = aboveTop >= 8
      ? placement === "top-right" ? "top-right" : "top"
      : placement === "top-right" ? "bottom-right" : "bottom";
    tooltip.style.left = `${left}px`;
    tooltip.style.top = `${Math.max(8, top)}px`;
  }

  function refreshActionButton(originalButton, row, onActivate) {
    if (!originalButton.isConnected) return;
    const replacement = originalButton.cloneNode(true);
    installActionButtonEvents(row, replacement, onActivate);
    originalButton.replaceWith(replacement);
    return replacement;
  }

  function configureActionButton(button, label, icon) {
    button.setAttribute("aria-label", label);
    button.dataset.codexActionLabel = label;
    button.removeAttribute("title");
    button.textContent = icon;
  }

  function trashIconSvg() {
    return `
      <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 6h18"></path>
        <path d="M8 6V4h8v2"></path>
        <path d="M19 6l-1 14H6L5 6"></path>
        <path d="M10 11v5"></path>
        <path d="M14 11v5"></path>
      </svg>
    `;
  }

  function configureSvgActionButton(button, label, svg) {
    button.setAttribute("aria-label", label);
    button.dataset.codexActionLabel = label;
    button.removeAttribute("title");
    button.innerHTML = svg;
  }

  function attachButton(row) {
    const settings = codexElvesSettings();
    if (!settings.sessionDelete && !settings.markdownExport && !settings.projectMove) {
      removeActionGroups(row);
      row.dataset.codexDeleteRow = "false";
      row.dataset.codexProjectMoveRow = "false";
      return;
    }
    const nativeActionHost = nativeActionHostFromRow(row);
    const existingGroup = actionGroupFromRow(row);
    const existingDeleteButton = existingGroup?.querySelector(`.${buttonClass}`);
    const existingMoreButton = existingGroup?.querySelector(`.${moreButtonClass}`);
    const existingExportButton = existingGroup?.querySelector(`.${exportButtonClass}`);
    const existingMoveButton = existingGroup?.querySelector(`.${projectMoveButtonClass}`);
    const needsMoreMenu = settings.markdownExport || settings.projectMove;
    const hasUnexpectedDelete = !settings.sessionDelete && !!existingDeleteButton;
    const hasUnexpectedMore = !needsMoreMenu && !!existingMoreButton;
    const hasUnexpectedExport = !!existingExportButton;
    const hasUnexpectedMove = !!existingMoveButton;
    const missingDelete = settings.sessionDelete && !existingDeleteButton;
    const missingMore = needsMoreMenu && !existingMoreButton;
    const deleteReady = !settings.sessionDelete || existingDeleteButton?.dataset.codexDeleteVersion === codexDeleteVersion;
    const groupReady = existingGroup?.dataset.codexActionGroupVersion === codexActionGroupVersion;
    const expectedPlacement = nativeActionHost ? "native" : "fallback";
    const placementReady = existingGroup?.dataset.codexActionPlacement === expectedPlacement &&
      (expectedPlacement === "native" ? existingGroup?.parentElement === nativeActionHost : existingGroup?.parentElement === row);
    if (groupReady && placementReady && deleteReady && !hasUnexpectedDelete && !hasUnexpectedMore && !hasUnexpectedExport && !hasUnexpectedMove && !missingDelete && !missingMore) {
      scheduleSessionRowLayout([row]);
      return;
    }
    removeActionGroups(row);
    row.dataset.codexDeleteRow = "false";
    row.dataset.codexProjectMoveRow = "false";
    const ref = sessionRefFromRow(row);
    if (!ref.session_id) return;
    row.dataset.codexDeleteRow = "true";
    row.dataset.codexProjectMoveRow = String(!!settings.projectMove);
    const group = document.createElement("div");
    group.className = actionGroupClass;
    group.dataset.codexActionGroupVersion = codexActionGroupVersion;
    group.dataset.codexActionPlacement = expectedPlacement;
    if (settings.markdownExport || settings.projectMove) {
      const moreButton = document.createElement("button");
      moreButton.type = "button";
      moreButton.className = sessionActionButtonClassName(nativeActionHost, moreButtonClass);
      moreButton.setAttribute("aria-haspopup", "menu");
      moreButton.setAttribute("aria-expanded", "false");
      configureActionButton(moreButton, "更多操作", "…");
      const moreMenu = document.createElement("div");
      moreMenu.className = moreMenuClass;
      moreMenu.setAttribute("role", "menu");
      moreMenu.hidden = true;
      if (settings.markdownExport) {
        moreMenu.appendChild(createSessionMoreMenuItem("导出", "⇩", (event) => {
          stopActionButtonEvent(row, moreButton, event);
          closeSessionMoreMenus();
          exportMarkdown(ref);
        }));
      }
      if (settings.projectMove) {
        moreMenu.appendChild(createSessionMoreMenuItem("移动", "↗", (event) => {
          stopActionButtonEvent(row, moreButton, event);
          closeSessionMoreMenus();
          openProjectMoveMenuForRow(row, moreButton, ref, event);
        }));
      }
      const openMoreMenu = (event) => {
        stopActionButtonEvent(row, moreButton, event);
        hideActionButtonTooltip();
        toggleSessionMoreMenu(row, moreButton, moreMenu);
        if (!moreMenu.hidden) {
          positionSessionMoreMenu(moreButton, moreMenu);
          updateSessionMoreMenuDirection(moreButton, moreMenu);
        }
      };
      installMoreButtonEvents(row, moreButton, openMoreMenu);
      group.appendChild(moreButton);
      moreMenu.__codexSessionMoreRow = row;
      moreMenu.__codexSessionMoreGroup = group;
      document.body.appendChild(moreMenu);
      installSessionMoreMenuAutoClose(row, moreMenu);
    }
    if (settings.sessionDelete) {
      const deleteButton = document.createElement("button");
      deleteButton.type = "button";
      deleteButton.className = sessionActionButtonClassName(nativeActionHost, buttonClass);
      deleteButton.dataset.codexDeleteVersion = codexDeleteVersion;
      configureSvgActionButton(deleteButton, "删除", trashIconSvg());
      const openDeleteConfirm = (event) => openDeleteConfirmForRow(row, deleteButton, ref, event);
      installActionButtonEvents(row, deleteButton, openDeleteConfirm);
      group.appendChild(deleteButton);
      setTimeout(() => refreshActionButton(deleteButton, row, openDeleteConfirm), 0);
    }
    if (nativeActionHost) {
      nativeActionHost.dataset.codexSessionActionHost = "true";
      nativeActionHost.prepend(group);
    } else {
      row.appendChild(group);
    }
    scheduleSessionRowLayout([row]);
  }

  function tryAttachButton(row) {
    try {
      attachButton(row);
    } catch (error) {
      appendCodexElvesFailure("__codexSessionDeleteAttachButtonFailures", error);
    }
  }

  function reactArchivedThreadFromNode(node) {
    const reactKey = Object.keys(node).find((key) => key.startsWith("__reactFiber$") || key.startsWith("__reactInternalInstance$"));
    let fiber = reactKey ? node[reactKey] : null;
    for (let depth = 0; fiber && depth < 20; depth += 1, fiber = fiber.return) {
      const props = fiber.memoizedProps || fiber.pendingProps || {};
      if (props.archivedThread?.id) return props.archivedThread;
      const childThread = props.children?.props?.archivedThread;
      if (childThread?.id) return childThread;
    }
    return null;
  }

  function archivedThreadFromRow(row) {
    for (const node of [row, ...row.querySelectorAll("*")]) {
      const thread = reactArchivedThreadFromNode(node);
      if (thread?.id || thread?.sessionId) return thread;
    }
    return null;
  }

  function archivedRefFromRow(row) {
    const archivedThread = archivedThreadFromRow(row);
    if (archivedThread?.id || archivedThread?.sessionId) {
      return { session_id: archivedThread.id || archivedThread.sessionId, title: archivedThread.title || row.querySelector(".truncate.text-base")?.textContent?.trim() || "Untitled session" };
    }
    const sidebarRef = sessionRefFromRow(row);
    if (sidebarRef.session_id) return sidebarRef;
    const titleNode = row.querySelector(".truncate.text-base, [data-thread-title], a, div");
    const title = ((titleNode || row).textContent || "Untitled session")
      .replace("取消归档", "")
      .replace("删除", "")
      .replace(/\d{4}年\d{1,2}月\d{1,2}日.*$/, "")
      .replace(/\s+·\s+.*$/, "")
      .trim()
      .slice(0, 160);
    return { session_id: "", title };
  }

  async function resolveArchivedThread(row) {
    const ref = archivedRefFromRow(row);
    if (ref.session_id) return ref;
    const resolved = await postJson("/archived-thread", { title: ref.title });
    return resolved?.session_id ? resolved : ref;
  }

  function stopArchivedButtonEvent(event) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation?.();
  }

  function attachArchivedPageDeleteButton(row) {
    const settings = codexElvesSettings();
    row.querySelectorAll("[data-codex-archive-row-action]").forEach((button) => button.remove());
    row.dataset.codexArchiveDeleteRow = "false";
    if (!settings.sessionDelete && !settings.markdownExport) return;
    const unarchiveButton = Array.from(row.querySelectorAll("button")).find((button) => (button.textContent || "").trim() === "取消归档");
    if (!unarchiveButton) return;
    row.dataset.codexArchiveDeleteRow = "true";
    row.dataset.codexArchiveRowActionsVersion = codexArchiveRowActionsVersion;
    let insertionPoint = unarchiveButton;
    if (settings.markdownExport) {
      const exportButton = document.createElement("button");
      exportButton.type = "button";
      exportButton.className = `codex-archive-delete-all codex-archive-row-button ${exportButtonClass}`;
      exportButton.dataset.codexArchiveRowAction = "export";
      exportButton.textContent = "导出";
      ["pointerdown", "mousedown", "mouseup", "touchstart"].forEach((eventName) => {
        exportButton.addEventListener(eventName, stopArchivedButtonEvent, true);
      });
      exportButton.addEventListener("click", async (event) => {
        stopArchivedButtonEvent(event);
        const ref = await resolveArchivedThread(row);
        if (!ref.session_id) {
          showToast("导出失败：未找到归档会话 ID");
          return;
        }
        await exportMarkdown(ref);
      }, true);
      insertionPoint.insertAdjacentElement("afterend", exportButton);
      insertionPoint = exportButton;
    }
  }

  const conversationViewContentClasses = [
    "mx-auto",
    "w-full",
    "max-w-(--thread-content-max-width)",
    "px-toolbar",
    "relative",
    "flex",
    "shrink-0",
    "flex-col",
    "pb-8",
  ];
  const conversationViewComposerClasses = [
    "relative",
    "z-10",
    "flex",
    "flex-col",
    "mx-auto",
    "w-full",
    "max-w-(--thread-content-max-width)",
    "px-toolbar",
  ];
  const conversationViewState = {
    contentEl: null,
    composerEl: null,
    rafId: 0,
    settleFramesLeft: 0,
    settleTimer: 0,
    mo: null,
    ro: null,
    observedRoot: null,
    observed: new WeakSet(),
    elements: new Set(),
  };

  function conversationViewTokenSet(el) {
    return new Set(String(el?.className || "").split(/\s+/).filter(Boolean));
  }

  function conversationViewHasAllClasses(el, classes) {
    const set = conversationViewTokenSet(el);
    return classes.every((cls) => set.has(cls));
  }

  function conversationViewElementIsActive(el) {
    if (!el?.isConnected) return false;
    if (el.closest?.("[hidden], [aria-hidden='true'], [inert], .invisible")) return false;
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const rect = el.getBoundingClientRect?.();
    return !!rect && rect.width > 0 && rect.height > 0;
  }

  function conversationViewFindByClasses(classes) {
    const matches = Array.from(document.querySelectorAll("div")).filter((el) => conversationViewHasAllClasses(el, classes));
    return matches.find(conversationViewElementIsActive) || matches.find((el) => el?.isConnected) || null;
  }

  function conversationViewFindContentEl() {
    return conversationViewFindByClasses(conversationViewContentClasses);
  }

  function conversationViewFindComposerEl() {
    return conversationViewFindByClasses(conversationViewComposerClasses);
  }

  function codexServiceTierBadgeVisibleElement(element) {
    if (!(element instanceof HTMLElement) || !element.isConnected) return false;
    const style = getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function codexServiceTierBadgeText(element) {
    return String(element?.textContent || "").replace(/\s+/g, " ").trim();
  }

  function codexServiceTierComposerInputs(root) {
    return Array.from(root?.querySelectorAll?.('.ProseMirror, textarea, [contenteditable="true"]') || [])
      .filter(codexServiceTierBadgeVisibleElement);
  }

  function cleanupLegacyCodexComposerOverflowGuards() {
    document.querySelectorAll(`.codex-elves-composer-overflow-guard, .${codexLegacyServiceTierComposerSurfaceClass}`)
      .forEach((surface) => {
        surface.classList.remove("codex-elves-composer-overflow-guard");
        surface.classList.remove(codexLegacyServiceTierComposerSurfaceClass);
      });
  }

  function codexServiceTierRectHorizontalOverlap(left, right) {
    return Math.max(0, Math.min(left.right, right.right) - Math.max(left.left, right.left));
  }

  function codexServiceTierFooterHasNearbyComposerInput(footer) {
    if (!(footer instanceof HTMLElement)) return false;
    const footerRect = footer.getBoundingClientRect();
    for (let node = footer.parentElement, depth = 0; node instanceof HTMLElement && depth < 7; depth += 1, node = node.parentElement) {
      const inputs = codexServiceTierComposerInputs(node);
      if (!inputs.length) continue;
      if (!inputs.some((input) => {
        const inputRect = input.getBoundingClientRect();
        const overlap = codexServiceTierRectHorizontalOverlap(inputRect, footerRect);
        return overlap >= Math.min(160, footerRect.width * 0.45)
          && inputRect.bottom <= footerRect.bottom + 12
          && inputRect.bottom >= footerRect.top - 220;
      })) continue;
      return true;
    }
    return false;
  }

  function codexServiceTierKnownProviderNames() {
    return uniqueValues([
      codexModelCatalog.provider_name,
      codexModelCatalog.model_provider,
    ]).map((value) => value.toLowerCase());
  }

  function codexServiceTierLooksLikeProviderButton(button, providerNames) {
    const text = codexServiceTierBadgeText(button);
    if (!text || text.length > 32) return false;
    const lower = text.toLowerCase();
    if (providerNames.includes(lower)) return true;
    if (/\s/.test(text)) return false;
    if (!/[a-z]/i.test(text)) return false;
    if (!/^[a-z0-9][a-z0-9._-]{1,31}$/i.test(text)) return false;
    if (/^(local|remote|cloud|standard|default|fast|worktree|new|send|stop|codex)$/i.test(text)) return false;
    if (/^(gpt|o[1-9]|claude|gemini|deepseek|qwen|kimi|moonshot|mistral|llama|sonnet|opus|haiku)[a-z0-9._-]*$/i.test(text)) return false;
    return true;
  }

  function codexServiceTierBadgeButtonCandidates(composer) {
    const composerRect = composer.getBoundingClientRect();
    return Array.from(composer.querySelectorAll("button, [role='button']"))
      .filter((button) => !button.closest?.(`[data-codex-service-tier-badge="true"]`))
      .filter(codexServiceTierBadgeVisibleElement)
      .filter((button) => {
        const rect = button.getBoundingClientRect();
        return rect.bottom >= composerRect.top + composerRect.height * 0.35;
      })
      .sort((left, right) => {
        const leftRect = left.getBoundingClientRect();
        const rightRect = right.getBoundingClientRect();
        return (rightRect.bottom - leftRect.bottom) || (leftRect.left - rightRect.left);
      });
  }

  const codexServiceTierComposerFooterSelector = '.composer-footer, [class*="_footer_"], [class*="ComposerLayoutFooter"]';

  function codexServiceTierIsComposerFooter(element) {
    return element instanceof HTMLElement && element.matches?.(codexServiceTierComposerFooterSelector);
  }

  function codexServiceTierVisibleComposerFooters(root = document) {
    const footers = [
      ...(codexServiceTierIsComposerFooter(root) ? [root] : []),
      ...Array.from(root?.querySelectorAll?.(codexServiceTierComposerFooterSelector) || []),
    ];
    return footers
      .filter(codexServiceTierLooksLikeComposerFooter)
      .filter(codexServiceTierBadgeVisibleElement)
      .sort((left, right) => {
        const leftRect = left.getBoundingClientRect();
        const rightRect = right.getBoundingClientRect();
        return (rightRect.bottom - leftRect.bottom) || (rightRect.width - leftRect.width);
      });
  }

  function codexServiceTierLooksLikeComposerFooter(footer) {
    if (!(footer instanceof HTMLElement)) return false;
    if (footer.matches?.(".composer-footer")) return codexServiceTierFooterHasNearbyComposerInput(footer);
    const className = String(footer.className || "");
    const legacyModuleFooter = className.includes("_footer_");
    const composerLayoutFooter = className.includes("ComposerLayoutFooter");
    if (!legacyModuleFooter && !composerLayoutFooter) return false;
    if (legacyModuleFooter && !className.includes("items-center")) return false;
    const rect = footer.getBoundingClientRect();
    if (rect.width < 220 || rect.height > 90) return false;
    if (!codexServiceTierFooterHasNearbyComposerInput(footer)) return false;
    const buttons = Array.from(footer.querySelectorAll("button, [role='button']")).filter(codexServiceTierBadgeVisibleElement);
    if (buttons.length < 2) return false;
    const text = codexServiceTierBadgeText(footer);
    return /model|完全访问|full access|high|超高|gpt|claude|gemini|deepseek|qwen|kimi|sonnet|opus|haiku/i.test(text)
      || buttons.some((button) => codexServiceTierBadgeText(button));
  }

  function codexServiceTierComposerScore(composer) {
    const text = codexServiceTierBadgeText(composer).toLowerCase();
    const providerNames = codexServiceTierKnownProviderNames();
    let score = 0;
    if (providerNames.some((name) => name && text.includes(name))) score += 40;
    if (/完全访问权限|full access|model|超高|high|sub2api|provider/i.test(text)) score += 20;
    if (/本地模式|local mode|worktree|branch|codex\//i.test(text)) score -= 30;
    if (codexServiceTierIsComposerFooter(composer)) score += 4;
    if (composer.querySelector?.(codexServiceTierComposerFooterSelector)) score += 8;
    const buttons = Array.from(composer.querySelectorAll?.("button, [role='button']") || []).filter(codexServiceTierBadgeVisibleElement);
    if (buttons.some((button) => codexServiceTierLooksLikeProviderButton(button, providerNames))) score += 30;
    score += Math.min(10, buttons.length);
    return score;
  }

  function codexServiceTierComposerCandidates() {
    const candidates = new Set();
    const threadComposer = conversationViewFindComposerEl();
    if (threadComposer && codexServiceTierBadgeVisibleElement(threadComposer)) candidates.add(threadComposer);
    codexServiceTierVisibleComposerFooters().forEach((footer) => {
      candidates.add(footer);
      let node = footer.parentElement;
      for (let depth = 0; node instanceof HTMLElement && depth < 6; depth += 1, node = node.parentElement) {
        if (codexServiceTierBadgeVisibleElement(node)) candidates.add(node);
      }
    });
    return Array.from(candidates);
  }

  function codexServiceTierBestComposerFooter(root = document) {
    return codexServiceTierVisibleComposerFooters(root)
      .map((footer, index) => ({ footer, index, score: codexServiceTierComposerScore(footer) }))
      .sort((left, right) => (right.score - left.score) || (left.index - right.index))[0]?.footer || null;
  }

  function codexServiceTierFindComposerEl() {
    return codexServiceTierComposerCandidates()
      .map((composer, index) => ({ composer, index, score: codexServiceTierComposerScore(composer) }))
      .sort((left, right) => (right.score - left.score) || (left.index - right.index))[0]?.composer || null;
  }

  function codexServiceTierBadgeAnchor(composer) {
    const providerNames = codexServiceTierKnownProviderNames();
    const buttons = codexServiceTierBadgeButtonCandidates(composer);
    const exact = buttons.find((button) => providerNames.includes(codexServiceTierBadgeText(button).toLowerCase()));
    if (exact) return exact;
    const composerRect = composer.getBoundingClientRect();
    return buttons.find((button) => {
      const rect = button.getBoundingClientRect();
      return rect.left >= composerRect.left + composerRect.width * 0.42 && codexServiceTierLooksLikeProviderButton(button, providerNames);
    }) || null;
  }

  function codexServiceTierComposerFooter(composer) {
    if (codexServiceTierIsComposerFooter(composer)) return composer;
    return codexServiceTierBestComposerFooter(composer) || codexServiceTierBestComposerFooter() || null;
  }

  function codexServiceTierBadgeFooterGroup(composer) {
    const footer = codexServiceTierComposerFooter(composer);
    if (!footer) return null;
    const children = Array.from(footer.children).filter(codexServiceTierBadgeVisibleElement);
    if (!children.length) return footer;
    const providerNames = codexServiceTierKnownProviderNames();
    const providerGroup = children.find((child) => {
      const text = codexServiceTierBadgeText(child).toLowerCase();
      return providerNames.some((name) => name && text.includes(name));
    });
    return providerGroup || children[children.length - 1] || footer;
  }

  function codexServiceTierNativeServiceTierSlot(composer) {
    const footer = codexServiceTierComposerFooter(composer);
    if (!footer) return null;
    const children = Array.from(footer.children).filter((child) => child instanceof HTMLElement);
    if (children.length >= 3 && String(footer.className || "").includes("grid-cols")) {
      const middle = children[Math.floor(children.length / 2)];
      const middleText = codexServiceTierBadgeText(middle);
      const onlyBadge = middle.children.length === 1 && middle.firstElementChild?.matches?.('[data-codex-service-tier-badge="true"]');
      if (middleText.length <= 32 && (middle.children.length === 0 || onlyBadge)) return middle;
    }
    return children.find((child) => {
      const text = codexServiceTierBadgeText(child);
      const className = String(child.className || "");
      const onlyBadge = child.children.length === 1 && child.firstElementChild?.matches?.('[data-codex-service-tier-badge="true"]');
      return className.includes("items-center") && text.length <= 32 && (child.children.length === 0 || onlyBadge);
    }) || null;
  }

  function codexServiceTierBadgePlacement(composer) {
    const nativeSlot = codexServiceTierNativeServiceTierSlot(composer);
    if (nativeSlot) return { parent: nativeSlot, before: null };
    const anchor = composer ? codexServiceTierBadgeAnchor(composer) : null;
    if (anchor?.parentElement) return { parent: anchor.parentElement, before: anchor };
    const group = composer ? codexServiceTierBadgeFooterGroup(composer) : null;
    if (group) return { parent: group, before: group.firstChild };
    return null;
  }

  function codexServiceTierPlacementFooter(placement) {
    const parent = placement?.parent;
    const footer = parent?.closest?.(codexServiceTierComposerFooterSelector);
    return codexServiceTierLooksLikeComposerFooter(footer) ? footer : null;
  }

  function codexServiceTierPlacementRowRect(placement, footer, beforeRect = null) {
    if (beforeRect) return beforeRect;
    const footerRect = footer.getBoundingClientRect();
    const parent = placement?.parent;
    if (parent instanceof HTMLElement) {
      const parentRect = parent.getBoundingClientRect();
      const overlapsFooter = parentRect.bottom > footerRect.top && parentRect.top < footerRect.bottom;
      if (overlapsFooter && parentRect.height > 0 && parentRect.height <= 48) return parentRect;
    }
    const bottomControl = Array.from(footer.querySelectorAll("button, [role='button']"))
      .filter(codexServiceTierBadgeVisibleElement)
      .sort((left, right) => {
        const leftRect = left.getBoundingClientRect();
        const rightRect = right.getBoundingClientRect();
        return (rightRect.bottom - leftRect.bottom) || (leftRect.left - rightRect.left);
      })[0];
    return bottomControl?.getBoundingClientRect() || footerRect;
  }

  function codexServiceTierPortalBadgeLeft(footer, rowRect, badgeWidth, desiredLeft) {
    const footerRect = footer.getBoundingClientRect();
    const contentLeft = footerRect.left + 4;
    const contentRight = footerRect.right - 4;
    const maxLeft = Math.max(contentLeft, contentRight - badgeWidth);
    const preferredLeft = Number.isFinite(desiredLeft)
      ? Math.min(maxLeft, Math.max(contentLeft, desiredLeft))
      : contentLeft;
    const rowCenter = rowRect.top + rowRect.height / 2;
    const controlPadding = 6;
    const occupied = [];
    Array.from(footer.querySelectorAll("button, [role='button']"))
      .filter(codexServiceTierBadgeVisibleElement)
      .map((control) => control.getBoundingClientRect())
      .filter((rect) => rowCenter >= rect.top - 2 && rowCenter <= rect.bottom + 2)
      .map((rect) => ({
        left: Math.max(contentLeft, rect.left - controlPadding),
        right: Math.min(contentRight, rect.right + controlPadding),
      }))
      .filter((rect) => rect.right > rect.left)
      .sort((left, right) => left.left - right.left)
      .forEach((rect) => {
        const previous = occupied[occupied.length - 1];
        if (previous && rect.left <= previous.right) {
          previous.right = Math.max(previous.right, rect.right);
        } else {
          occupied.push(rect);
        }
      });
    const gaps = [];
    let cursor = contentLeft;
    occupied.forEach((rect) => {
      if (rect.left - cursor >= badgeWidth) gaps.push({ left: cursor, right: rect.left });
      cursor = Math.max(cursor, rect.right);
    });
    if (contentRight - cursor >= badgeWidth) gaps.push({ left: cursor, right: contentRight });
    if (!gaps.length) return preferredLeft;
    return gaps
      .map((gap) => {
        const left = Math.min(gap.right - badgeWidth, Math.max(gap.left, preferredLeft));
        return { left, distance: Math.abs(left - preferredLeft) };
      })
      .sort((left, right) => (left.distance - right.distance) || (left.left - right.left))[0].left;
  }

  function codexServiceTierClearBadgeRetry(resetAttempt = false) {
    clearTimeout(window.__codexServiceTierBadgeRetryTimer);
    window.__codexServiceTierBadgeRetryTimer = null;
    if (resetAttempt) window.__codexServiceTierBadgeRetryAttempt = 0;
  }

  function scheduleCodexServiceTierBadgeLayout() {
    if (!codexElvesSettings().serviceTierControls) return;
    if (typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(window.__codexServiceTierBadgeLayoutRafId);
    } else {
      clearTimeout(window.__codexServiceTierBadgeLayoutRafId);
    }
    const scheduleFrame = typeof requestAnimationFrame === "function"
      ? requestAnimationFrame
      : (callback) => setTimeout(callback, 16);
    window.__codexServiceTierBadgeLayoutRafId = scheduleFrame(() => {
      window.__codexServiceTierBadgeLayoutRafId = 0;
      installCodexServiceTierBadge();
    });
  }

  function scheduleCodexServiceTierBadgeRetry(delayMs = 80) {
    codexServiceTierClearBadgeRetry();
    const attempt = Number(window.__codexServiceTierBadgeRetryAttempt || 0) + 1;
    window.__codexServiceTierBadgeRetryAttempt = attempt;
    if (attempt > codexServiceTierBadgeRetryMaxAttempts) return;
    const retryDelayMs = Math.min(
      codexServiceTierBadgeRetryMaxDelayMs,
      Math.max(delayMs, 80 * (2 ** Math.min(attempt - 1, 4)))
    );
    window.__codexServiceTierBadgeRetryTimer = setTimeout(() => {
      window.__codexServiceTierBadgeRetryTimer = null;
      installCodexServiceTierBadge();
    }, retryDelayMs);
  }

  function codexServiceTierHasVisibleComposerInput() {
    return codexServiceTierComposerInputs(document).length > 0;
  }

  function codexServiceTierPositionPortalBadge(badge, placement) {
    const footer = codexServiceTierPlacementFooter(placement);
    const portalRoot = document.body || document.documentElement;
    if (!badge || !footer || !portalRoot) return false;
    badge.dataset.codexServiceTierPortal = "true";
    badge.style.visibility = "hidden";
    if (badge.parentElement !== portalRoot) portalRoot.appendChild(badge);
    const parentRect = placement.parent.getBoundingClientRect();
    const before = placement.before?.parentElement === placement.parent ? placement.before : null;
    const beforeRect = before && codexServiceTierBadgeVisibleElement(before)
      ? before.getBoundingClientRect()
      : null;
    const badgeRect = badge.getBoundingClientRect();
    const badgeWidth = badgeRect.width || 54;
    const badgeHeight = badgeRect.height || 24;
    let desiredLeft;
    if (beforeRect) {
      desiredLeft = beforeRect.left - badgeWidth - 6;
    } else if (parentRect.width >= badgeWidth) {
      desiredLeft = parentRect.left + (parentRect.width - badgeWidth) / 2;
    } else {
      const previous = placement.parent.previousElementSibling;
      const previousRect = previous instanceof HTMLElement && codexServiceTierBadgeVisibleElement(previous)
        ? previous.getBoundingClientRect()
        : null;
      desiredLeft = previousRect ? previousRect.right + 6 : parentRect.left;
    }
    const verticalAnchorRect = codexServiceTierPlacementRowRect(placement, footer, beforeRect);
    const left = codexServiceTierPortalBadgeLeft(footer, verticalAnchorRect, badgeWidth, desiredLeft);
    const top = verticalAnchorRect.top + (verticalAnchorRect.height - badgeHeight) / 2;
    badge.style.left = `${Math.round(left)}px`;
    badge.style.top = `${Math.round(top)}px`;
    badge.style.visibility = "visible";
    badge.dataset.codexServiceTierPlacementValidAt = String(Date.now());
    return true;
  }

  function codexServiceTierKeepPortalBadgeDuringTransientLayout(existingBadges) {
    const badge = existingBadges.find((node) => node.dataset.codexServiceTierPortal === "true");
    const lastValidAt = Number(badge?.dataset.codexServiceTierPlacementValidAt || 0);
    if (
      badge &&
      codexServiceTierHasVisibleComposerInput() &&
      lastValidAt > 0 &&
      Date.now() - lastValidAt <= codexServiceTierBadgePlacementGraceMs
    ) {
      scheduleCodexServiceTierBadgeRetry();
      return true;
    }
    existingBadges.forEach((node) => {
      node.style.visibility = "hidden";
    });
    if (codexServiceTierHasVisibleComposerInput()) scheduleCodexServiceTierBadgeRetry(160);
    return false;
  }

  function wireCodexServiceTierBadge(badge) {
    if (!badge || badge.dataset.codexServiceTierBadgeWired === codexServiceTierBadgeVersion) return;
    badge.dataset.codexServiceTierBadgeWired = codexServiceTierBadgeVersion;
    badge.setAttribute("role", "button");
    badge.setAttribute("tabindex", "0");
    badge.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (codexServiceTierState.status === "loading") return;
      toggleCodexServiceTierFromBadge();
    });
    badge.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      event.stopPropagation();
      if (codexServiceTierState.status === "loading") return;
      toggleCodexServiceTierFromBadge();
    });
  }

  function installCodexServiceTierBadge() {
    if (!codexElvesSettings().serviceTierControls) {
      removeCodexServiceTierBadges();
      return;
    }
    const composer = codexServiceTierFindComposerEl();
    const placement = composer ? codexServiceTierBadgePlacement(composer) : null;
    const existingBadges = Array.from(document.querySelectorAll(`[data-codex-service-tier-badge="true"]`));
    if (!composer || !placement?.parent || !codexServiceTierPlacementFooter(placement)) {
      codexServiceTierKeepPortalBadgeDuringTransientLayout(existingBadges);
      return;
    }
    codexServiceTierClearBadgeRetry(true);
    let badge = existingBadges.find((node) => node.dataset.codexServiceTierPortal === "true") || existingBadges[0];
    existingBadges.forEach((node) => {
      if (node !== badge) node.remove();
    });
    if (!badge || badge.dataset.codexServiceTierBadgeVersion !== codexServiceTierBadgeVersion) {
      badge?.remove();
      badge = document.createElement("span");
      badge.className = codexServiceTierBadgeClass;
      badge.dataset.codexServiceTierBadge = "true";
      badge.dataset.codexServiceTierBadgeVersion = codexServiceTierBadgeVersion;
    }
    wireCodexServiceTierBadge(badge);
    codexServiceTierPositionPortalBadge(badge, placement);
    refreshCodexServiceTierBadges();
  }

  function removeCodexServiceTierBadges() {
    codexServiceTierClearBadgeRetry(true);
    if (typeof cancelAnimationFrame === "function") {
      cancelAnimationFrame(window.__codexServiceTierBadgeLayoutRafId);
    } else {
      clearTimeout(window.__codexServiceTierBadgeLayoutRafId);
    }
    window.__codexServiceTierBadgeLayoutRafId = 0;
    document.querySelectorAll(`[data-codex-service-tier-badge="true"]`).forEach((badge) => badge.remove());
  }

  function syncCodexServiceTierBadgeLayoutListener() {
    document.removeEventListener("scroll", window.__codexServiceTierBadgeScrollHandler, true);
    window.__codexServiceTierBadgeScrollHandler = null;
    if (!codexElvesSettings().serviceTierControls) {
      removeCodexServiceTierBadges();
      return;
    }
    window.__codexServiceTierBadgeScrollHandler = scheduleCodexServiceTierBadgeLayout;
    document.addEventListener("scroll", window.__codexServiceTierBadgeScrollHandler, true);
  }

  function conversationViewRememberOriginals(el) {
    if (!el) return;
    conversationViewState.elements.add(el);
    const original = {
      width: el.style.width || "",
      maxWidth: el.style.maxWidth || "",
      marginLeft: el.style.marginLeft || "",
      marginRight: el.style.marginRight || "",
      left: el.style.left || "",
      transform: el.style.transform || "",
      boxSizing: el.style.boxSizing || "",
    };
    if (!("codexElvesConversationViewOriginalWidth" in el.dataset)) el.dataset.codexElvesConversationViewOriginalWidth = original.width;
    if (!("codexElvesConversationViewOriginalMaxWidth" in el.dataset)) el.dataset.codexElvesConversationViewOriginalMaxWidth = original.maxWidth;
    if (!("codexElvesConversationViewOriginalMarginLeft" in el.dataset)) el.dataset.codexElvesConversationViewOriginalMarginLeft = original.marginLeft;
    if (!("codexElvesConversationViewOriginalMarginRight" in el.dataset)) el.dataset.codexElvesConversationViewOriginalMarginRight = original.marginRight;
    if (!("codexElvesConversationViewOriginalLeft" in el.dataset)) el.dataset.codexElvesConversationViewOriginalLeft = original.left;
    if (!("codexElvesConversationViewOriginalTransform" in el.dataset)) el.dataset.codexElvesConversationViewOriginalTransform = original.transform;
    if (!("codexElvesConversationViewOriginalBoxSizing" in el.dataset)) el.dataset.codexElvesConversationViewOriginalBoxSizing = original.boxSizing;
  }

  function conversationViewRestoreElement(el) {
    if (!el) return;
    if ("codexElvesConversationViewOriginalWidth" in el.dataset) {
      el.style.width = el.dataset.codexElvesConversationViewOriginalWidth;
      delete el.dataset.codexElvesConversationViewOriginalWidth;
    }
    if ("codexElvesConversationViewOriginalMaxWidth" in el.dataset) {
      el.style.maxWidth = el.dataset.codexElvesConversationViewOriginalMaxWidth;
      delete el.dataset.codexElvesConversationViewOriginalMaxWidth;
    }
    if ("codexElvesConversationViewOriginalMarginLeft" in el.dataset) {
      el.style.marginLeft = el.dataset.codexElvesConversationViewOriginalMarginLeft;
      delete el.dataset.codexElvesConversationViewOriginalMarginLeft;
    }
    if ("codexElvesConversationViewOriginalMarginRight" in el.dataset) {
      el.style.marginRight = el.dataset.codexElvesConversationViewOriginalMarginRight;
      delete el.dataset.codexElvesConversationViewOriginalMarginRight;
    }
    if ("codexElvesConversationViewOriginalLeft" in el.dataset) {
      el.style.left = el.dataset.codexElvesConversationViewOriginalLeft;
      delete el.dataset.codexElvesConversationViewOriginalLeft;
    }
    if ("codexElvesConversationViewOriginalTransform" in el.dataset) {
      el.style.transform = el.dataset.codexElvesConversationViewOriginalTransform;
      delete el.dataset.codexElvesConversationViewOriginalTransform;
    }
    if ("codexElvesConversationViewOriginalBoxSizing" in el.dataset) {
      el.style.boxSizing = el.dataset.codexElvesConversationViewOriginalBoxSizing;
      delete el.dataset.codexElvesConversationViewOriginalBoxSizing;
    }
    delete el.dataset.codexElvesConversationViewAppliedLeft;
  }

  function conversationViewNativeRect(el) {
    if (!el) return null;
    const originalTransform = el.dataset.codexElvesConversationViewOriginalTransform || "";
    const originalLeft = el.dataset.codexElvesConversationViewOriginalLeft || "";
    const appliedLeft = el.dataset.codexElvesConversationViewAppliedLeft || "";
    if (!appliedLeft || el.style.left !== appliedLeft) {
      if (el.style.left !== originalLeft) el.style.left = originalLeft;
      delete el.dataset.codexElvesConversationViewAppliedLeft;
    }
    if (el.style.transform !== originalTransform) el.style.transform = originalTransform;
    const transform = String(el.style.transform || "").trim();
    if (/^(translateX\([^)]*\)\s*)+$/i.test(transform)) {
      el.style.transform = "";
    }
    const rect = el.getBoundingClientRect();
    if (!appliedLeft || el.style.left !== appliedLeft) return rect;
    const appliedPx = Number.parseFloat(appliedLeft);
    if (!Number.isFinite(appliedPx)) return rect;
    return {
      left: rect.left - appliedPx,
      right: rect.right - appliedPx,
      x: rect.x - appliedPx,
      top: rect.top,
      bottom: rect.bottom,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    };
  }

  function conversationViewApplyNativeWidth(el) {
    conversationViewRememberOriginals(el);
    const maxWidth = `${conversationViewWidth()}px`;
    if (el.style.boxSizing !== "border-box") el.style.boxSizing = "border-box";
    if (el.style.width !== "100%") el.style.width = "100%";
    if (el.style.maxWidth !== maxWidth) el.style.maxWidth = maxWidth;
    if (el.style.marginLeft !== "auto") el.style.marginLeft = "auto";
    if (el.style.marginRight !== "auto") el.style.marginRight = "auto";
  }

  function conversationViewSessionRectFor(el) {
    return el?.parentElement?.getBoundingClientRect() || null;
  }

  function conversationViewHtmlCenter() {
    const rect = document.documentElement.getBoundingClientRect();
    return rect.left + rect.width / 2;
  }

  function conversationViewHasRoomForHtmlCenter(nativeRect, bounds) {
    if (!nativeRect || !bounds) return false;
    const targetLeft = conversationViewHtmlCenter() - nativeRect.width / 2;
    const targetRight = targetLeft + nativeRect.width;
    return targetLeft >= bounds.left - 0.5 && targetRight <= bounds.right + 0.5;
  }

  function conversationViewAlignElement(el) {
    if (!conversationViewElementIsActive(el)) return;
    conversationViewApplyNativeWidth(el);
    const nativeRect = conversationViewNativeRect(el);
    const bounds = conversationViewSessionRectFor(el);
    if (!conversationViewHasRoomForHtmlCenter(nativeRect, bounds)) {
      const originalLeft = el.dataset.codexElvesConversationViewOriginalLeft || "";
      if (el.style.left !== originalLeft) el.style.left = originalLeft;
      delete el.dataset.codexElvesConversationViewAppliedLeft;
      return;
    }
    const targetLeft = conversationViewHtmlCenter() - nativeRect.width / 2;
    const delta = targetLeft - nativeRect.left;
    if (Math.abs(delta) > 0.5) {
      const nextLeft = `${delta.toFixed(2)}px`;
      if (el.style.left !== nextLeft) el.style.left = nextLeft;
      el.dataset.codexElvesConversationViewAppliedLeft = nextLeft;
    } else {
      const originalLeft = el.dataset.codexElvesConversationViewOriginalLeft || "";
      if (el.style.left !== originalLeft) el.style.left = originalLeft;
      delete el.dataset.codexElvesConversationViewAppliedLeft;
    }
  }

  function conversationViewObserveIfNeeded(el) {
    if (!el || !conversationViewState.ro || conversationViewState.observed.has(el)) return;
    conversationViewState.observed.add(el);
    conversationViewState.ro.observe(el);
  }

  function conversationViewResolveTargets() {
    if (!conversationViewElementIsActive(conversationViewState.contentEl)) conversationViewState.contentEl = conversationViewFindContentEl();
    if (!conversationViewElementIsActive(conversationViewState.composerEl)) conversationViewState.composerEl = conversationViewFindComposerEl();
    [
      document.documentElement,
      document.body,
      conversationViewState.contentEl,
      conversationViewState.contentEl?.parentElement,
      conversationViewState.contentEl?.parentElement?.parentElement,
      conversationViewState.composerEl,
      conversationViewState.composerEl?.parentElement,
      conversationViewState.composerEl?.parentElement?.parentElement,
    ].forEach(conversationViewObserveIfNeeded);
  }

  function conversationViewObserverRoot() {
    const content = conversationViewElementIsActive(conversationViewState.contentEl) ? conversationViewState.contentEl : conversationViewFindContentEl();
    const composer = conversationViewElementIsActive(conversationViewState.composerEl) ? conversationViewState.composerEl : conversationViewFindComposerEl();
    const contentRoot = content?.parentElement?.parentElement || content?.parentElement || content;
    const composerRoot = composer?.parentElement?.parentElement || composer?.parentElement || composer;
    return document.querySelector("main, [role='main']") || contentRoot?.parentElement || composerRoot?.parentElement || contentRoot || composerRoot || null;
  }

  function conversationViewAlignNow() {
    if (!codexElvesSettings().conversationView) return;
    conversationViewResolveTargets();
    conversationViewAlignElement(conversationViewState.contentEl);
    conversationViewAlignElement(conversationViewState.composerEl);
  }

  function scheduleConversationViewAlign(frames = 16) {
    conversationViewState.settleFramesLeft = Math.max(conversationViewState.settleFramesLeft, frames);
    if (conversationViewState.rafId) return;
    const tick = () => {
      conversationViewState.rafId = 0;
      conversationViewAlignNow();
      conversationViewState.settleFramesLeft -= 1;
      if (conversationViewState.settleFramesLeft > 0) {
        conversationViewState.rafId = requestAnimationFrame(tick);
      }
    };
    conversationViewState.rafId = requestAnimationFrame(tick);
  }

  function conversationViewForgetTargets() {
    conversationViewState.contentEl = null;
    conversationViewState.composerEl = null;
  }

  function startConversationViewSettleWindow() {
    if (conversationViewState.settleTimer) clearTimeout(conversationViewState.settleTimer);
    scheduleConversationViewAlign(180);
    conversationViewState.settleTimer = window.setTimeout(() => {
      conversationViewState.settleTimer = 0;
    }, 3000);
  }

  function cleanupConversationView() {
    if (conversationViewState.rafId) cancelAnimationFrame(conversationViewState.rafId);
    if (conversationViewState.settleTimer) clearTimeout(conversationViewState.settleTimer);
    conversationViewState.rafId = 0;
    conversationViewState.settleTimer = 0;
    conversationViewState.mo?.disconnect();
    conversationViewState.ro?.disconnect();
    conversationViewState.mo = null;
    conversationViewState.ro = null;
    conversationViewState.observedRoot = null;
    conversationViewState.observed = new WeakSet();
    conversationViewState.elements.forEach(conversationViewRestoreElement);
    conversationViewState.elements.clear();
    conversationViewState.contentEl = null;
    conversationViewState.composerEl = null;
  }

  window.__codexElvesConversationViewCleanup = cleanupConversationView;

  function ensureConversationViewRuntime() {
    conversationViewState.ro = conversationViewState.ro || new ResizeObserver(() => scheduleConversationViewAlign());
    conversationViewState.mo = conversationViewState.mo || new MutationObserver(() => scheduleConversationViewAlign());
    const root = conversationViewObserverRoot();
    if (root && conversationViewState.observedRoot !== root) {
      conversationViewState.mo.disconnect();
      conversationViewState.mo.observe(root, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ["class", "hidden", "data-state", "aria-hidden"],
      });
      conversationViewState.observedRoot = root;
    }
  }

  function refreshConversationView(forceResolve = false) {
    if (!codexElvesSettings().conversationView) {
      cleanupConversationView();
      return;
    }
    if (forceResolve) {
      conversationViewForgetTargets();
      conversationViewState.observedRoot = null;
    }
    ensureConversationViewRuntime();
    startConversationViewSettleWindow();
  }

  function scheduleConversationViewRouteRefresh() {
    (window.__codexConversationViewRouteTimers || []).forEach((timer) => clearTimeout(timer));
    window.__codexConversationViewRouteTimers = [];
    if (!codexElvesSettings().conversationView) return;
    const revision = (window.__codexConversationViewRouteRevision || 0) + 1;
    window.__codexConversationViewRouteRevision = revision;
    window.__codexConversationViewRouteTimers = codexConversationViewRouteRefreshDelaysMs.map((delay) => setTimeout(() => {
      if (window.__codexConversationViewRouteRevision !== revision) return;
      refreshConversationView(true);
    }, delay));
  }

  function routeFeatureScanDirty() {
    return {
      sidebar: true,
      conversation: true,
      header: true,
      plugins: false,
      shell: false,
    };
  }

  function runRouteFeatureRefresh() {
    invalidateSessionRowsCache();
    scan(routeFeatureScanDirty());
    requestAnimationFrame(() => runScanStep(installScanObservers));
  }

  function scheduleCodexRouteFeatureRefresh() {
    scheduleConversationViewRouteRefresh();
    refreshCodexServiceTierFeatureState();
    (window.__codexRouteFeatureRefreshTimers || []).forEach((timer) => clearTimeout(timer));
    const revision = (window.__codexRouteFeatureRefreshRevision || 0) + 1;
    window.__codexRouteFeatureRefreshRevision = revision;
    window.__codexRouteFeatureRefreshTimers = codexRouteFeatureRefreshDelaysMs.map((delay) => setTimeout(() => {
      if (window.__codexRouteFeatureRefreshRevision !== revision) return;
      runRouteFeatureRefresh();
    }, delay));
  }

  function installCodexRouteFeatureRefreshEvents() {
    document.removeEventListener("pointerup", window.__codexRouteFeaturePointerHandler, true);
    document.removeEventListener("click", window.__codexRouteFeatureClickHandler, true);
    document.removeEventListener("keydown", window.__codexRouteFeatureKeyboardHandler, true);
    const shouldRefreshConversationViewForControl = (event) => {
      if (!codexElvesSettings().conversationView) return false;
      const target = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (!target || isExtensionUiNode(target)) return false;
      const control = target.closest("button, a, [role='button'], [role='link']");
      if (!control || isExtensionUiNode(control)) return false;
      return true;
    };
    const clickHandler = (event) => {
      const toggle = event.target?.closest?.(selectors.pinnedSummaryToggle);
      if (toggle) {
        scheduleCodexTokenUsagePinnedSummarySync(
          toggle.getAttribute("aria-pressed") || ""
        );
      } else if (event.target?.closest?.(selectors.sidebarThread)) scheduleCodexRouteFeatureRefresh();
      else if (shouldRefreshConversationViewForControl(event)) scheduleConversationViewRouteRefresh();
    };
    const keyboardHandler = (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      if (event.target?.closest?.(selectors.sidebarThread)) scheduleCodexRouteFeatureRefresh();
      else if (shouldRefreshConversationViewForControl(event)) scheduleConversationViewRouteRefresh();
    };
    window.__codexRouteFeaturePointerHandler = null;
    window.__codexRouteFeatureClickHandler = clickHandler;
    window.__codexRouteFeatureKeyboardHandler = keyboardHandler;
    document.addEventListener("click", clickHandler, true);
    document.addEventListener("keydown", keyboardHandler, true);
  }

  function installConversationViewRouteHooks() {
    if (window.__codexConversationViewRouteHooksInstalled === codexConversationViewRouteHooksVersion) return;
    window.__codexConversationViewRouteHooksInstalled = codexConversationViewRouteHooksVersion;
    window.__codexConversationViewOriginals = window.__codexConversationViewOriginals || {};
    const originals = window.__codexConversationViewOriginals;
    ["pushState", "replaceState"].forEach((method) => {
      const currentMethod = history[method];
      const original = originals[`history_${method}`] || currentMethod;
      originals[`history_${method}`] = original;
      if (typeof original !== "function") return;
      history[method] = function codexConversationViewPatchedHistory(...args) {
        const result = original.apply(this, args);
        scheduleCodexRouteFeatureRefresh();
        return result;
      };
    });
    window.removeEventListener("popstate", window.__codexConversationViewPopStateHandler, true);
    window.removeEventListener("hashchange", window.__codexConversationViewHashChangeHandler, true);
    window.__codexConversationViewPopStateHandler = () => scheduleCodexRouteFeatureRefresh();
    window.__codexConversationViewHashChangeHandler = () => scheduleCodexRouteFeatureRefresh();
    window.addEventListener("popstate", window.__codexConversationViewPopStateHandler, true);
    window.addEventListener("hashchange", window.__codexConversationViewHashChangeHandler, true);
  }

  function installCodexElvesRuntimeOnce() {
    installStyle();
    if (window.__codexElvesRuntimeOnceInstalled === codexElvesBuild) return;
    cleanupLegacyCodexComposerOverflowGuards();
    void loadCodexModelCatalog();
    installCodexServiceTierDispatcherPatch();
    installCodexServiceTierRequestClientPatch();
    installSuppressedThreadObserver();
    scheduleBackendHeartbeat();
    installDeleteButtonEventDelegation();
    installConversationViewRouteHooks();
    installCodexRouteFeatureRefreshEvents();
    installCodexAppServerRestartPositionTracking();
    installCodexTokenUsagePinnedSummaryObserver();
    refreshCodexServiceTierControls();
    window.__codexElvesRuntimeOnceInstalled = codexElvesBuild;
  }

  function scanDeferred(dirty = allScanDirty()) {
    const shellDirty = !!dirty.shell;
    const sidebarDirty = !!dirty.sidebar || shellDirty;
    const conversationDirty = !!dirty.conversation || shellDirty;
    const headerDirty = !!dirty.header || shellDirty;
    const pluginsDirty = !!dirty.plugins || shellDirty;

    if (shellDirty) cleanupDisconnectedSessionArtifacts();
    if (pluginsDirty) {
      if (pluginPatchDisabledInRelayMode()) {
        clearPluginPatchArtifacts();
      } else {
        const pluginUnlockStrategy = codexPluginUnlockStrategy();
        const settings = codexElvesSettings();
        logCodexPluginUnlockStrategy(pluginUnlockStrategy);
        if ((pluginUnlockStrategy === "legacy" || pluginUnlockStrategy === "unknown") && settings.pluginEntryUnlock) {
          enablePluginEntry();
        }
        if ((pluginUnlockStrategy === "modern" || pluginUnlockStrategy === "unknown") && settings.pluginMarketplaceUnlock) {
          const marketplaceRequestPatchStrategy = codexPluginMarketplaceRequestPatchStrategy();
          if (marketplaceRequestPatchStrategy === "bridge") {
            installPluginMarketplaceBridgePatch();
            installPluginMarketplaceRequestPatch();
          } else if (marketplaceRequestPatchStrategy === "client") {
            installPluginMarketplaceRequestPatch();
          } else {
            installPluginMarketplaceWindowEventPatchOnly();
            installPluginMarketplaceBridgePatch();
            installPluginMarketplaceRequestPatch();
          }
        }
      }
    }
    if (sidebarDirty || shellDirty) {
      reconcileTaskBoardRuntime();
    }
    if (sidebarDirty) {
      const pending = takePendingSessionRows();
      pending.rows.forEach(tryAttachButton);
      updateDeleteButtonOffsets(pending.rows);
      scheduleSessionRowLayout(pending.rows);
      scheduleProjectMoveProjection();
      scheduleChatsSortCorrection(chatsSortEventDelayMs, { refreshKeys: true });
      if (!chatsSortFallbackArmed) armChatsSortVisibleFallback();
    }
    if (sidebarDirty || conversationDirty) {
      archivedPageRows().forEach(attachArchivedPageDeleteButton);
    }
    if (conversationDirty) {
      refreshConversationView();
      installCodexAppServerRestartButtons();
    }
    if (headerDirty || conversationDirty) {
      if (headerDirty) installCodexElvesMenu();
      installCodexServiceTierBadge();
      refreshCodexTokenUsageCard();
    }
  }

  function emptyScanDirty() {
    return {
      sidebar: false,
      conversation: false,
      header: false,
      plugins: false,
      shell: false,
    };
  }

  function allScanDirty() {
    return {
      sidebar: true,
      conversation: true,
      header: true,
      plugins: true,
      shell: true,
    };
  }

  function mergeScanDirty(target, source) {
    const next = source || allScanDirty();
    Object.keys(next).forEach((key) => {
      target[key] = target[key] || !!next[key];
    });
    return target;
  }

  function dirtyForScanDomain(domain) {
    const dirty = emptyScanDirty();
    if (domain && domain in dirty) {
      dirty[domain] = true;
      return dirty;
    }
    return allScanDirty();
  }

  function runScanStep(step) {
    try {
      step();
    } catch (error) {
      appendCodexElvesFailure("__codexSessionDeleteScanFailures", error);
    }
  }

  function appendCodexElvesFailure(key, error) {
    const failures = Array.isArray(window[key]) ? window[key] : [];
    failures.push(String(error?.stack || error));
    if (failures.length > codexFailureHistoryMaxEntries) {
      failures.splice(0, failures.length - codexFailureHistoryMaxEntries);
    }
    window[key] = failures;
  }

  function codexPluginRequestIds(key) {
    const existing = window[key];
    const ids = existing instanceof Map ? existing : new Map();
    if (existing instanceof Set) {
      existing.forEach((id) => ids.set(String(id), Date.now()));
    }
    const expiresAt = Date.now() - codexPluginRequestIdTtlMs;
    for (const [id, at] of ids) {
      if (!Number.isFinite(at) || at < expiresAt) ids.delete(id);
    }
    while (ids.size > codexPluginRequestIdMaxEntries) {
      const oldest = ids.keys().next().value;
      if (oldest == null) break;
      ids.delete(oldest);
    }
    window[key] = ids;
    return ids;
  }

  function rememberCodexPluginRequestId(key, requestId) {
    if (requestId == null) return;
    const ids = codexPluginRequestIds(key);
    ids.set(String(requestId), Date.now());
    while (ids.size > codexPluginRequestIdMaxEntries) {
      const oldest = ids.keys().next().value;
      if (oldest == null) break;
      ids.delete(oldest);
    }
  }

  function consumeCodexPluginRequestId(key, requestId) {
    const ids = codexPluginRequestIds(key);
    if (ids.size === 0) return true;
    const normalizedId = String(requestId || "");
    if (!ids.has(normalizedId)) return false;
    ids.delete(normalizedId);
    return true;
  }

  function scan(dirty = allScanDirty(), options = {}) {
    if (dirty.sidebar && options.sidebarIncremental !== true) {
      resetPendingSessionRowsForFullRefresh();
    }
    requestAnimationFrame(() => runScanStep(() => scanDeferred(dirty)));
  }

  function isExtensionUiNode(node) {
    return !!node?.closest?.(`.codex-delete-toast, .codex-delete-confirm-overlay, .codex-elves-modal-overlay, .${projectMoveOverlayClass}, .codex-conversation-timeline, .${codexServiceTierBadgeClass}, .${codexTokenUsageCardClass}, .${codexAppServerRestartButtonClass}, .${codexAppServerRestartDialogClass}, .${taskBoardMainHostClass}, .${taskBoardEntryContextMenuClass}, [${taskBoardEntryAttribute}="true"], #codex-elves-menu`);
  }

  function scanRelevantSelectorForDomain(domain) {
    if (domain === "sidebar") {
      return [
        selectors.sidebarThread,
        '[data-app-action-sidebar-section-heading="Chats"]',
        '[data-app-action-sidebar-section-heading="Projects"]',
        '[data-app-action-sidebar-project-row]',
        '[data-app-action-sidebar-project-id]',
        '[data-codex-project-move-row="true"]',
        pluginNavigationControlSelector,
        taskBoardEntrySelector,
      ].join(", ");
    }
    if (domain === "header") {
      return [
        selectors.applicationMenuTopBar,
        selectors.appHeader,
        selectors.pinnedSummaryPanel,
        selectors.pinnedSummaryToggle,
      ].join(", ");
    }
    if (domain === "conversation") {
      return [
        '[data-codex-archive-page-row="true"]',
        '[data-content-search-unit-key]',
        '[data-message-author-role]',
        '[data-testid="conversation-turn"]',
        '[class*="user-message"]',
        '[class*="UserMessage"]',
        ".composer-footer",
        '[class*="_footer_"]',
        ".ProseMirror",
        selectors.pinnedSummaryPanel,
        selectors.pinnedSummaryToggle,
        selectors.archiveNav,
      ].join(", ");
    }
    return [
      scanRelevantSelectorForDomain("sidebar"),
      scanRelevantSelectorForDomain("header"),
      scanRelevantSelectorForDomain("conversation"),
      "main",
      "aside",
      "header",
      "[role='main']",
      "[role='navigation']",
      "[role='banner']",
    ].join(", ");
  }

  function nodeSelfOrAncestorMatchesScanRelevance(node, domain) {
    if (node.nodeType !== 1) return false;
    if (isExtensionUiNode(node)) return false;
    const relevantSelector = scanRelevantSelectorForDomain(domain);
    return !!node.matches?.(relevantSelector) ||
      !!node.closest?.(relevantSelector);
  }

  function isScanRelevantNode(node, domain) {
    if (node.nodeType !== 1) return false;
    if (isExtensionUiNode(node)) return false;
    return nodeSelfOrAncestorMatchesScanRelevance(node, domain) ||
      !!node.querySelector?.(scanRelevantSelectorForDomain(domain));
  }

  function isChatContentMutation(mutation) {
    const target = mutation.target;
    if (!target?.closest?.('[data-message-author-role], [data-testid="conversation-turn"], main .prose')) return false;
    return !Array.from(mutation.addedNodes).some((node) => node.nodeType === 1 && isScanRelevantNode(node, "conversation")) &&
      !Array.from(mutation.removedNodes).some((node) => node.nodeType === 1 && isScanRelevantNode(node, "conversation"));
  }

  function shouldScheduleScan(mutations, domain) {
    if (!mutations) return true;
    const appServerRestartDirty = (
      domain === "conversation" || domain === "shell"
    ) && mutations.some(codexAppServerRestartMutationRelevant);
    return appServerRestartDirty || mutations.some((mutation) => {
      if (domain === "conversation" && isChatContentMutation(mutation)) return false;
      const target = mutation.target;
      if (isExtensionUiNode(target)) return false;
      if (target?.nodeType === 1 && nodeSelfOrAncestorMatchesScanRelevance(target, domain)) return true;
      const changedNodes = [...Array.from(mutation.addedNodes), ...Array.from(mutation.removedNodes)];
      return changedNodes.some((node) => node.nodeType === 1 && isScanRelevantNode(node, domain));
    });
  }

  function runScheduledScan() {
    const dirty = window.__codexSessionDeleteScanDirty || allScanDirty();
    window.__codexSessionDeleteScanPending = false;
    window.__codexSessionDeleteScanDirty = emptyScanDirty();
    clearTimeout(window.__codexSessionDeleteScanTimer);
    window.__codexSessionDeleteScanTimer = null;
    if (dirty.shell) invalidateSessionRowsCache();
    scan(dirty, { sidebarIncremental: !dirty.shell });
    if (dirty.shell) requestAnimationFrame(() => runScanStep(installScanObservers));
  }

  function scheduleScan(mutations, domain) {
    if (!shouldScheduleScan(mutations, domain)) return;
    if (domain === "sidebar") collectPendingSessionRows(mutations);
    window.__codexSessionDeleteScanDirty = mergeScanDirty(
      window.__codexSessionDeleteScanDirty || emptyScanDirty(),
      dirtyForScanDomain(domain),
    );
    if (window.__codexSessionDeleteScanPending) return;
    window.__codexSessionDeleteScanPending = true;
    window.__codexSessionDeleteScanTimer = setTimeout(runScheduledScan, 200);
  }

  function scanObserverRoots() {
    const roots = [];
    const push = (domain, root, options = { childList: true, subtree: true }) => {
      if (!root || roots.some((entry) => entry.root === root && entry.domain === domain)) return;
      roots.push({ domain, root, options });
    };
    const sidebarRoot = document.querySelector(selectors.sidebarThread)?.closest?.("nav, aside, [role='navigation'], [class*='sidebar']") ||
      document.querySelector("nav, aside, [role='navigation']");
    const conversationRoot = conversationViewFindContentEl()?.closest?.("main, [role='main']") ||
      document.querySelector("main, [role='main']");
    const applicationMenuTopBar = findApplicationMenuTopBar();
    const headerRoot = applicationMenuTopBar?.closest?.("header, [role='banner']") ||
      applicationMenuTopBar?.parentElement ||
      document.querySelector(selectors.appHeader)?.closest?.("header, [role='banner']") ||
      document.querySelector("header, [role='banner']");
    const scopedRootsReady = !!sidebarRoot && !!conversationRoot && !!headerRoot;
    push("shell", document.body || document.documentElement, {
      childList: true,
      subtree: !scopedRootsReady,
    });
    if (scopedRootsReady) {
      [sidebarRoot, conversationRoot, headerRoot].forEach((root) => {
        push("shell", root.parentElement, {
          childList: true,
          subtree: false,
        });
      });
    }
    push("sidebar", sidebarRoot, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["aria-current", "data-state", "data-selected", "data-active"],
    });
    push("conversation", conversationRoot);
    push("header", headerRoot, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style", "hidden", "aria-expanded", "aria-pressed", "data-state"] });
    return roots;
  }

  function scanObserverOptionsKey(options) {
    const attributeFilter = Array.isArray(options?.attributeFilter) ? options.attributeFilter.join(",") : "";
    return [
      options?.childList ? "childList" : "",
      options?.subtree ? "subtree" : "",
      options?.attributes ? "attributes" : "",
      attributeFilter,
    ].join("|");
  }

  function sameScanObserverRoots(nextRoots) {
    const previous = window.__codexSessionDeleteObserverConfigs || [];
    if (previous.length !== nextRoots.length) return false;
    return nextRoots.every((entry, index) => {
      const current = previous[index];
      return current?.domain === entry.domain &&
        current?.root === entry.root &&
        current?.optionsKey === scanObserverOptionsKey(entry.options);
    });
  }

  function installScanObservers() {
    const roots = scanObserverRoots();
    if (sameScanObserverRoots(roots)) return;
    (window.__codexSessionDeleteObservers || []).forEach((observer) => observer.disconnect());
    window.__codexSessionDeleteObservers = [];
    window.__codexSessionDeleteObserverConfigs = roots.map(({ domain, root, options }) => ({
      domain,
      root,
      optionsKey: scanObserverOptionsKey(options),
    }));
    roots.forEach(({ domain, root, options }) => {
      const observer = new MutationObserver((mutations) => scheduleScan(mutations, domain));
      observer.observe(root, options);
      window.__codexSessionDeleteObservers.push(observer);
    });
  }

  void loadBackendSettingsForStartup();
  void loadCodexServiceTierState();
  refreshUpstreamBranchDropdownAdapter();
  installUpstreamWorktreeNativeAdapter();
  runScanStep(installCodexElvesRuntimeOnce);
  refreshTaskBoardRuntime();
  scan();
  syncChatsSortVisibilityListener();
  window.__codexProjectMoveApplyProjection = applyProjectMoveProjection;
  window.__codexProjectMoveReadProjection = readProjectMoveProjection;
  window.__codexProjectMoveTargets = projectMoveTargets;
  window.__codexProjectMoveSortChats = applyChatsSortCorrection;
  window.__codexTokenUsageRefresh = refreshCodexTokenUsageCard;
  window.removeEventListener("resize", window.__codexElvesResizeHandler);
  let codexElvesResizeRafId = 0;
  window.__codexElvesResizeHandler = () => {
    cancelAnimationFrame(codexElvesResizeRafId);
    codexElvesResizeRafId = requestAnimationFrame(() => {
      const rows = sessionRows();
      rows.forEach((row) => {
        const group = actionGroupFromRow(row);
        if (group) delete group.dataset.codexActionLayoutStable;
      });
      scheduleSessionRowLayout(rows);
      updateFloatingCodexElvesMenuPosition(document.getElementById(codexElvesMenuId));
      runScanStep(refreshConversationView);
      scheduleCodexServiceTierBadgeLayout();
    });
  };
  window.addEventListener("resize", window.__codexElvesResizeHandler);
  syncCodexServiceTierBadgeLayoutListener();
  window.removeEventListener("storage", window.__codexElvesStorageHandler, true);
  window.__codexElvesStorageHandler = (event) => {
    if (!event || (event.key !== codexElvesSettingsKey && event.key !== codexThreadServiceTierKey)) return;
    invalidateCodexElvesSettingsCache();
    if (event.key === codexThreadServiceTierKey) codexThreadServiceTierStateCache = null;
    if (event.key === codexElvesSettingsKey) {
      refreshCodexTokenUsageFeatureState();
      refreshCodexServiceTierFeatureState();
      refreshUpstreamBranchDropdownAdapter();
      syncChatsSortVisibilityListener();
    }
    scan(scanDirtyForSetting(""));
  };
  window.addEventListener("storage", window.__codexElvesStorageHandler, true);
  window.__codexSessionDeleteObserver?.disconnect();
  window.__codexSessionDeleteObserver = null;
  installScanObservers();
  window.__codexElvesRefreshRuntime = () => {
    installStyle();
    cleanupLegacyForcePluginInstallRuntime();
    void loadBackendSettingsForStartup();
    installCodexElvesImageOverlay();
    void loadCodexServiceTierState();
    void loadCodexModelCatalog();
    scan();
    refreshCodexTokenUsageFeatureState();
    refreshCodexServiceTierFeatureState();
    refreshUpstreamBranchDropdownAdapter();
    syncChatsSortVisibilityListener();
    refreshTaskBoardRuntime();
    installScanObservers();
    installSuppressedThreadObserver();
    installCodexAppServerRestartPositionTracking();
  };
  window.__codexElvesRuntimeBuild = codexElvesBuild;
  window.__codexElvesRuntimeHelperBase = helperBase;
  window.__codexElvesRuntimeManagerDiscoveryVersion = codexAppServerManagerDiscoveryVersion;
  window.__codexElvesTaskBoardRuntimeVersion = taskBoardRuntimeVersion;
})();
