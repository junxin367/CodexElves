(() => {
  const helperBase = window.__CODEX_SESSION_DELETE_HELPER__ || "http://127.0.0.1:45221";
  const buttonClass = "codex-delete-button";
  const exportButtonClass = "codex-export-button";
  const projectMoveButtonClass = "codex-project-move-button";
  const projectMoveOverlayClass = "codex-project-move-overlay";
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
  const codexDeleteStyleVersion = "27";
  const codexElvesMenuId = "codex-elves-menu";
  const codexElvesMenuFloatingClass = "codex-elves-menu-floating";
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
  const codexServiceTierRequestOverrideVersion = "3";
  const codexServiceTierRequestClientPatchRetryBaseMs = 1000;
  const codexServiceTierRequestClientPatchRetryMaxMs = 30000;
  const codexAppServerManagerDiscoveryVersion = "1";
  const codexStatsigModelVisibilityConfigId = "107580212";
  const codexStatsigModelVisibilityPatchVersion = "1";
  const codexStatsigModelVisibilityRetryDelayMs = 50;
  const codexStatsigModelVisibilityMaxWaitMs = 60000;
  const codexSessionPrewarmVersion = "3";
  const codexSessionPrewarmDefaultConcurrency = 4;
  const codexSessionPrewarmStartupDelayMs = 200;
  const codexSessionPrewarmInteractionPauseMs = 1200;
  const codexSessionPrewarmRecentRefreshTimeoutMs = 5000;
  const codexSessionPrewarmMaxAgeMs = 24 * 60 * 60 * 1000;
  const codexSessionPrewarmMaxRetries = 2;
  const codexSessionPrewarmRetryBaseDelayMs = 1500;
  const codexAppServerManagerDiscoveryMaxFailures = 12;
  const codexPluginMarketplaceUnlockVersion = "19";
  const codexPluginApiKeyUnsupportedMarketplaceKinds = new Set(["created-by-me-remote"]);
  const codexPluginAutoExpandVersion = "1";
  const codexPluginAutoExpandMaxClicks = 24;
  const codexPluginAutoExpandClickDelayMs = 180;
  const codexBackendHeartbeatIntervalMs = 30000;
  const codexElvesImageOverlayId = "codex-elves-image-overlay";
  const codexTokenUsageCardClass = "codex-token-usage-card";
  const codexTokenUsageHostClass = "codex-token-usage-host";
  const codexTokenUsageRefreshIntervalMs = 2500;
  const codexTokenUsageDurationTickIntervalMs = 1000;
  const codexTokenUsageSettleDelayMs = 500;
  const codexTokenUsageCompletionSettleDelayMs = 2500;
  const codexTokenUsageRetryDelaysMs = [1000, 2500, 5000];
  const codexTokenUsageRequestTimeoutMs = 5000;
  const codexTokenUsageLifecycleTimeoutMs = 30000;
  const codexPluginRequestIdTtlMs = 2 * 60 * 1000;
  const codexPluginRequestIdMaxEntries = 256;
  const codexFailureHistoryMaxEntries = 64;
  const codexManagerReactDiscoveryCooldownMs = 15000;
  const codexTokenUsageNotificationMethods = [
    "thread/tokenUsage/updated",
    "turn/started",
    "turn/completed",
  ];
  window.__codexProjectMoveRuntimeId = (window.__codexProjectMoveRuntimeId || 0) + 1;
  const codexProjectMoveRuntimeId = window.__codexProjectMoveRuntimeId;
  clearTimeout(window.__codexProjectMoveProjectionTimer);
  clearTimeout(window.__codexProjectMoveChatsSortTimer);
  clearTimeout(window.__codexProjectMoveChatsSortFallbackTimer);
  window.__codexProjectMoveProjectionTimer = null;
  window.__codexProjectMoveChatsSortTimer = null;
  window.__codexProjectMoveChatsSortFallbackTimer = null;
  clearTimeout(window.__codexAppServerManagerDiscoveryRetryTimer);
  window.__codexAppServerManagerDiscoveryRetryTimer = null;
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
  clearTimeout(window.__codexTokenUsageSettleTimer);
  window.__codexTokenUsageSettleTimer = null;
  clearTimeout(window.__codexTokenUsageCompletionSettleTimer);
  window.__codexTokenUsageCompletionSettleTimer = null;
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
  if (!(window.__codexTokenUsageSummaryCache instanceof Map)) {
    window.__codexTokenUsageSummaryCache = new Map();
  }
  window.__codexTokenUsageRequestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
  try {
    window.__codexTokenUsageNotificationUnsubscribe?.();
  } catch {
  }
  window.__codexTokenUsageNotificationUnsubscribe = null;
  window.__codexTokenUsageNotificationManager = null;
  window.__codexPluginAutoExpandContainer = null;
  window.__codexPluginAutoExpandCandidates = [];
  window.__codexPluginAutoExpandIdleUntil = 0;
  window.__codexPluginAutoExpandLastRouteSignature = "";
  window.__codexPluginAutoExpandLastContainerSignature = "";
  function cleanupLegacyForcePluginInstallRuntime() {
    window.__codexForcePluginInstallObserver?.disconnect?.();
    window.__codexForcePluginInstallObserver = null;
    window.__codexForcePluginInstallObserverRoot = null;
    clearTimeout(window.__codexForcePluginInstallSettleTimer);
    window.__codexForcePluginInstallSettleTimer = null;
  }
  cleanupLegacyForcePluginInstallRuntime();
  const codexElvesInjectedLaunchCycle = String(window.__CODEX_ELVES_LAUNCH_CYCLE__ || "").trim();
  if (
    codexElvesInjectedLaunchCycle &&
    window.__codexSessionPrewarmLaunchCycle !== codexElvesInjectedLaunchCycle
  ) {
    window.__codexSessionPrewarmCompletedSignature = "";
  }
  if (
    window.__codexElvesRuntimeBuild === codexElvesBuild &&
    window.__codexElvesRuntimeHelperBase === helperBase &&
    window.__codexElvesRuntimeManagerDiscoveryVersion === codexAppServerManagerDiscoveryVersion &&
    typeof window.__codexElvesRefreshRuntime === "function"
  ) {
    window.__codexElvesRefreshRuntime();
    return;
  }
  window.__codexSessionPrewarmRuntimeId = (window.__codexSessionPrewarmRuntimeId || 0) + 1;
  const codexSessionPrewarmRuntimeId = window.__codexSessionPrewarmRuntimeId;
  clearTimeout(window.__codexSessionPrewarmTimer);
  window.__codexSessionPrewarmTimer = null;

  function installCodexElvesImageOverlay() {
    const config = window.__CODEX_ELVES_IMAGE_OVERLAY__ || {};
    const canQueryById = typeof document?.getElementById === "function";
    const existing = canQueryById ? document.getElementById(codexElvesImageOverlayId) : null;
    const source = config.dataUrl || "";
    if (!config.enabled || !source) {
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
    const image = existing || document.createElement("img");
    image.id = codexElvesImageOverlayId;
    image.src = source;
    image.alt = "";
    image.setAttribute("aria-hidden", "true");
    Object.assign(image.style, {
      position: "fixed",
      inset: "0",
      width: "100vw",
      height: "100vh",
      objectFit: "contain",
      objectPosition: "center center",
      opacity: String(opacity),
      pointerEvents: "none",
      zIndex: "2147483646",
      userSelect: "none",
    });
    if (!existing) root.appendChild(image);
    sendCodexElvesDiagnostic("image_overlay_installed", {
      opacity,
      sourceKind: source.startsWith("data:") ? "data-uri" : "unknown",
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
    appHeader: ".app-header-tint",
    nativeMenuBar: "[class*=\"ms-auto\"][class*=\"flex\"][class*=\"items-center\"]",
    headerContextMenuSurface: '[data-testid="app-shell-header-context-menu-surface"]',
    pinnedSummaryPanel: '[data-pip-obstacle="thread-summary-panel"]',
    pinnedSummaryToggle: 'button[aria-label="切换置顶摘要"], button[title="切换置顶摘要"], button[aria-label="Toggle Pinned Summary"], button[title="Toggle Pinned Summary"]',
    archiveNav: 'button[aria-label="已归档对话"], button[aria-label="Archived conversations"]',
    pluginNavButton: 'nav[role="navigation"] button.h-token-nav-row.w-full',
    pluginSvgPath: 'svg path[d^="M7.94562 14.0277"]',
  };
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
        position: fixed;
        inset: 0;
        z-index: 2147483200;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(15,23,42,.28);
      }
      .codex-delete-confirm-content {
        width: min(420px, calc(100vw - 48px));
        border: 1px solid rgba(15,23,42,.12);
        border-radius: 12px;
        background: #ffffff;
        color: #111827;
        font: 14px system-ui, sans-serif;
        box-shadow: 0 24px 80px rgba(15,23,42,.22);
        padding: 18px;
      }
      .codex-delete-confirm-title { font-size: 16px; font-weight: 650; }
      .codex-delete-confirm-message { margin-top: 8px; color: #4b5563; line-height: 1.45; }
      .codex-delete-confirm-actions {
        display: flex;
        justify-content: flex-end;
        gap: 10px;
        margin-top: 18px;
      }
      .codex-delete-confirm-actions button {
        border: 1px solid #d1d5db;
        border-radius: 7px;
        padding: 6px 12px;
        background: #ffffff;
        color: #111827;
        font: 13px system-ui, sans-serif;
        cursor: pointer;
      }
      .codex-delete-confirm-actions [data-codex-delete-confirm="true"] {
        border-color: #ef4444;
        background: #dc2626;
        color: #ffffff;
      }
      /* Dark theme overrides for delete-confirm and project-move dialogs.
         Triggered either by Codex applying a "dark" class / data-theme="dark"
         on its document root, or by the OS-level prefers-color-scheme hint.
         Palette matches the existing CodexElves dark modal (.codex-elves-modal-content). */
      html.dark .codex-delete-confirm-overlay,
      html[data-theme="dark"] .codex-delete-confirm-overlay,
      :root[data-theme="dark"] .codex-delete-confirm-overlay {
        background: rgba(0,0,0,.55);
      }
      html.dark .codex-delete-confirm-content,
      html[data-theme="dark"] .codex-delete-confirm-content,
      :root[data-theme="dark"] .codex-delete-confirm-content {
        border-color: rgba(255,255,255,.12);
        background: #2b2b2b;
        color: #f3f4f6;
        box-shadow: 0 24px 80px rgba(0,0,0,.55);
      }
      html.dark .codex-delete-confirm-message,
      html[data-theme="dark"] .codex-delete-confirm-message,
      :root[data-theme="dark"] .codex-delete-confirm-message {
        color: #d1d5db;
      }
      html.dark .codex-delete-confirm-actions button,
      html[data-theme="dark"] .codex-delete-confirm-actions button,
      :root[data-theme="dark"] .codex-delete-confirm-actions button {
        border-color: rgba(255,255,255,.18);
        background: #3f3f46;
        color: #f3f4f6;
      }
      html.dark .codex-delete-confirm-actions [data-codex-delete-confirm="true"],
      html[data-theme="dark"] .codex-delete-confirm-actions [data-codex-delete-confirm="true"],
      :root[data-theme="dark"] .codex-delete-confirm-actions [data-codex-delete-confirm="true"] {
        border-color: #ef4444;
        background: #dc2626;
        color: #ffffff;
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
        html:not(.light):not([data-theme="light"]) .codex-delete-confirm-overlay {
          background: rgba(0,0,0,.55);
        }
        html:not(.light):not([data-theme="light"]) .codex-delete-confirm-content {
          border-color: rgba(255,255,255,.12);
          background: #2b2b2b;
          color: #f3f4f6;
          box-shadow: 0 24px 80px rgba(0,0,0,.55);
        }
        html:not(.light):not([data-theme="light"]) .codex-delete-confirm-message {
          color: #d1d5db;
        }
        html:not(.light):not([data-theme="light"]) .codex-delete-confirm-actions button {
          border-color: rgba(255,255,255,.18);
          background: #3f3f46;
          color: #f3f4f6;
        }
        html:not(.light):not([data-theme="light"]) .codex-delete-confirm-actions [data-codex-delete-confirm="true"] {
          border-color: #ef4444;
          background: #dc2626;
          color: #ffffff;
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
    `;
    document.documentElement.appendChild(style);
  }

  function defaultCodexElvesSettings() {
    return {
      pluginEntryUnlock: true,
      pluginMarketplaceUnlock: true,
      pluginAutoExpand: true,
      sessionDelete: true,

      sessionPrewarmEnabled: false,
      sessionPrewarmFullCount: 3,
      sessionPrewarmContentCount: 3,
      sessionPrewarmConcurrency: codexSessionPrewarmDefaultConcurrency,
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
    pluginAutoExpand: "codexAppPluginAutoExpand",
    sessionDelete: "codexAppSessionDelete",

    sessionPrewarmEnabled: "codexAppSessionPrewarmEnabled",
    sessionPrewarmFullCount: "codexAppSessionPrewarmFullCount",
    sessionPrewarmContentCount: "codexAppSessionPrewarmContentCount",
    sessionPrewarmConcurrency: "codexAppSessionPrewarmConcurrency",
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

  function clampSessionPrewarmCount(value, fallback, max) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return fallback;
    return Math.min(max, Math.max(0, Math.round(numeric)));
  }

  function clampSessionPrewarmConcurrency(value) {
    const numeric = Number(value);
    if (!Number.isFinite(numeric)) return codexSessionPrewarmDefaultConcurrency;
    return Math.min(4, Math.max(1, Math.round(numeric)));
  }

  function normalizeSessionPrewarmSettings(settings) {
    settings.sessionPrewarmEnabled = settings.sessionPrewarmEnabled === true;
    settings.sessionPrewarmFullCount = clampSessionPrewarmCount(settings.sessionPrewarmFullCount, 3, 4);
    settings.sessionPrewarmContentCount = clampSessionPrewarmCount(settings.sessionPrewarmContentCount, 3, 6);
    settings.sessionPrewarmConcurrency = clampSessionPrewarmConcurrency(
      settings.sessionPrewarmConcurrency
    );
    return settings;
  }

  let codexElvesSettingsCache = null;
  let codexThreadServiceTierStateCache = null;

  function invalidateCodexElvesSettingsCache() {
    codexElvesSettingsCache = null;
  }

  function disabledCodexElvesSettings() {
    return {
      pluginEntryUnlock: false,
      pluginMarketplaceUnlock: false,
      pluginAutoExpand: false,
      sessionDelete: false,
      sessionPrewarmEnabled: false,
      sessionPrewarmFullCount: 0,
      sessionPrewarmContentCount: 0,
      sessionPrewarmConcurrency: 0,
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
        settings.pluginAutoExpand = false;
      }
      codexElvesSettingsCache = normalizeSessionPrewarmSettings(settings);
    } catch {
      const settings = { ...defaultCodexElvesSettings(), ...backendCodexElvesSettings() };
      if (relayPatchDisabled) {
        settings.pluginEntryUnlock = false;
        settings.pluginMarketplaceUnlock = false;
        settings.pluginAutoExpand = false;
      }
      codexElvesSettingsCache = normalizeSessionPrewarmSettings(settings);
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
    if (key === "pluginAutoExpand" && !value) {
      clearTimeout(window.__codexPluginAutoExpandTimer);
      window.__codexPluginAutoExpandTimer = null;
      window.__codexPluginAutoExpandRunning = false;
      window.__codexPluginAutoExpandLastSignature = "";
    }
    renderCodexElvesMenu();
    scan(scanDirtyForSetting(key));
  }

  function scanDirtyForSetting(key) {
    const dirty = emptyScanDirty();
    if (["pluginEntryUnlock", "pluginMarketplaceUnlock", "pluginAutoExpand"].includes(key)) {
      dirty.plugins = true;
      return dirty;
    }
    if ([
      "sessionDelete",
      "sessionPrewarmEnabled",
      "sessionPrewarmFullCount",
      "sessionPrewarmContentCount",
      "sessionPrewarmConcurrency",
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

  // Codex App 升级后会重排 chunk：dispatcher 类的归属模块与导出名都可能变化。
  // 历史上它在 setting-storage-*.js 的 module.v；新版迁移到 vscode-api-*.js 的 module.d。
  // 这里不写死模块名/导出名，按特征（含 dispatchMessage + getInstance）在候选模块里嗅探。
  const codexServiceTierDispatcherModuleParts = ["vscode-api-", "setting-storage-"];
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

  async function codexSettingStorageModule() {
    const module = await loadCodexAppModule("setting-storage-");
    if (typeof module.n !== "function" || typeof module.s !== "function") {
      throw new Error("Codex setting-storage 接口不可用");
    }
    return module;
  }

  async function getCodexServiceTierSetting() {
    try {
      const settingStorage = await codexSettingStorageModule();
      return await settingStorage.n(codexDefaultServiceTierSetting);
    } catch (error) {
      if (typeof codexStateCall === "function") {
        const result = await codexStateCall("get-setting", { params: { key: codexDefaultServiceTierSetting.key } });
        return result && Object.prototype.hasOwnProperty.call(result, "value") ? result.value : codexDefaultServiceTierSetting.default;
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

  // 规范化模型名/文本用于匹配：小写、去除所有非字母数字字符
  function codexServiceTierModelMatchKey(value) {
    return String(value || "").toLowerCase().replace(/[^a-z0-9]+/g, "");
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
      const slugs = [];
      const entries = Array.isArray(codexModelCatalog.model_entries) ? codexModelCatalog.model_entries : [];
      for (const entry of entries) {
        if (entry && entry.slug) slugs.push(String(entry.slug));
        if (entry && entry.display_name) slugs.push(String(entry.display_name));
      }
      if (Array.isArray(codexModelCatalog.models)) {
        for (const m of codexModelCatalog.models) if (m) slugs.push(String(m));
      }
      if (!slugs.length) return "";
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
      // 优先精确匹配，再包含，再片段；同时优先更长的 slug，避免短片段误命中
      const sortedSlugs = slugs.slice().sort((a, b) => b.length - a.length);
      for (const text of texts) {
        for (const slug of sortedSlugs) {
          if (codexServiceTierModelMatchKey(text) === codexServiceTierModelMatchKey(slug)) {
            return findCatalogSlug(slug) || slug;
          }
        }
      }
      for (const text of texts) {
        for (const slug of sortedSlugs) {
          if (codexServiceTierModelMatchesText(slug, text)) {
            return findCatalogSlug(slug) || slug;
          }
        }
      }
    } catch (error) {
      void error;
    }
    return "";
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
    const key = codexServiceTierModelMatchKey(value);
    const entries = Array.isArray(codexModelCatalog.model_entries) ? codexModelCatalog.model_entries : [];
    for (const entry of entries) {
      if (entry && entry.slug && codexServiceTierModelMatchKey(entry.slug) === key) return String(entry.slug);
      if (entry && entry.display_name && codexServiceTierModelMatchKey(entry.display_name) === key && entry.slug) return String(entry.slug);
    }
    if (Array.isArray(codexModelCatalog.models)) {
      for (const m of codexModelCatalog.models) if (m && codexServiceTierModelMatchKey(m) === key) return String(m);
    }
    return "";
  }

  function codexServiceTierCurrentModelName() {
    // 优先使用会话 composer 中实际选中的模型；回退到后端配置的激活/默认模型
    return codexServiceTierComposerSelectedModel()
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
      showToast("后端未连接，无法切换服务模式", null);
      refreshCodexServiceTierControls();
      return;
    }
    const normalizedMode = normalizeCodexServiceTierControlMode(mode);
    if (normalizedMode === "global-fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName), null);
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
    showToast(`服务模式：${labels[normalizedMode] || normalizedMode}`, null);
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
      showToast("后端未连接，无法切换服务模式", null);
      refreshCodexServiceTierControls();
      return;
    }
    const normalizedMode = normalizeCodexThreadServiceTierMode(mode);
    if (normalizedMode === "fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName), null);
        refreshCodexServiceTierControls();
        return;
      }
    }
    const threadId = validThreadSessionKey(currentSessionRef().session_id);
    setCodexThreadServiceTierOverride(threadId, normalizedMode);
    refreshCodexServiceTierControls();
    const target = threadId ? "当前 thread" : "新 thread 草稿";
    showToast(`${target}服务模式：${normalizedMode === "inherit" ? "继承" : normalizedMode}`, null);
  }

  function toggleCodexServiceTierFromBadge() {
    if (codexElvesBackendStatus.status !== "ok") {
      showToast("后端未连接，无法切换服务模式", null);
      refreshCodexServiceTierControls();
      return;
    }
    syncCodexServiceTierEffectiveState();
    const nextMode = codexServiceTierState.effectiveMode === "fast" ? "standard" : "fast";
    if (nextMode === "fast") {
      const fastAvailability = codexServiceTierFastAvailability();
      if (!fastAvailability.supported) {
        codexServiceTierMaybeLoadModelCatalog(true);
        showToast(codexServiceTierFastUnsupportedMessage(fastAvailability.modelName), null);
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
        const module = await loadCodexAppModule("thread-context-inputs-");
        const requestClientClass = codexServiceTierRequestClientClassFromModule(module);
        if (!requestClientClass) throw new Error("Codex AppServerRequestClient unavailable");
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
    return refreshCodexSessionPrewarmFeatureState(reason);
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
      if (localKey === "projectMove" && !codexElvesSettings().projectMove) stopChatsSortRuntime();
      if (localKey === "projectMove") syncChatsSortVisibilityListener();
      if (localKey === "upstreamWorktreeCreate") refreshUpstreamBranchDropdownAdapter();
      if ([
        "sessionPrewarmEnabled",
        "sessionPrewarmFullCount",
        "sessionPrewarmContentCount",
        "sessionPrewarmConcurrency",
      ].includes(localKey)) {
        void refreshCodexSessionPrewarmFeatureState(`setting-${localKey}`);
      }
      scan(scanDirtyForSetting(localKey));
    }
  }

  function refreshCodexServiceTierFeatureState() {
    if (codexElvesSettings().serviceTierControls) {
      syncCodexServiceTierBadgeLayoutListener();
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
      new Promise((resolve) => setTimeout(() => resolve({ status: "failed", message: "后端检查超时", timeout: true }), 2000)),
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
      codexElvesBackendStatus = await postJson("/backend/repair", {});
    } catch (error) {
      codexElvesBackendStatus = { status: "failed", message: "后端修复失败" };
    }
    renderBackendStatus();
  }

  async function openManagerFromCodex() {
    const result = await postJson("/manager/open", {});
    if (result.status === "ok") {
      showToast("管理工具已打开", null);
    } else {
      showToast(result.message || "打开管理工具失败", null);
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
              <div><div class="codex-elves-row-title">页面功能增强</div><div class="codex-elves-row-description">关闭后停用删除、导出、移动、Fast 按钮、插件相关和菜单位置增强。</div></div>
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
              <div><div class="codex-elves-row-title">插件列表全量展示</div><div class="codex-elves-row-description">进入插件页后自动连续展开“更多”，尽量一次显示完整插件列表。</div></div>
              <button type="button" class="codex-elves-toggle" data-codex-elves-setting="pluginAutoExpand"><span></span></button>
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
              <div><div class="codex-elves-row-title">会话删除</div><div class="codex-elves-row-description">在会话列表悬停显示删除按钮，并支持撤销。</div></div>
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
          showToast("Upstream worktree enhancement is disabled", null);
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

  function findNativeMenuInsertionPoint() {
    if (!codexElvesSettings().nativeMenuPlacement) return null;
    const header = document.querySelector(selectors.appHeader);
    const isIconOnlyButton = (button) => String(button.className || "").includes("aspect-square");
    const menuBar = Array.from(header?.querySelectorAll?.(selectors.nativeMenuBar) || [])
      .find((node) => {
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
    const contextSurface = header?.querySelector(selectors.headerContextMenuSurface);
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
    const incompatibleNativeGroupClasses = new Set(["gap-0", "rounded-l-none", "border-l-0", "pl-0.5", "pr-1.5"]);
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
    if (existing && existing.dataset.codexElvesMenuVersion !== "6") {
      existing.remove();
      insertionPoint = findNativeMenuInsertionPoint();
    } else if (existing && insertionPoint && existing.parentElement === insertionPoint.parent) {
      configureCodexElvesTrigger(existing, existing.querySelector("button"), insertionPoint.nativeButtonClass);
      const safeBefore = insertionPoint.before?.parentElement === insertionPoint.parent ? insertionPoint.before : null;
      if (existing.nextSibling !== safeBefore) insertionPoint.parent.insertBefore(existing, safeBefore);
      removeDuplicateCodexElvesMenus(existing);
      return;
    } else if (existing && insertionPoint) {
      configureCodexElvesTrigger(existing, existing.querySelector("button"), insertionPoint.nativeButtonClass);
      existing.className = "";
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
    menu.dataset.codexElvesMenuVersion = "6";
    const trigger = document.createElement("button");
    trigger.type = "button";
    const indicator = ensureCodexElvesTriggerIndicator(trigger);
    if (indicator) indicator.dataset.status = codexElvesBackendStatus.status || "checking";
    setCodexElvesTriggerLabel(trigger);
    const nativeButtonClass = insertionPoint?.nativeButtonClass || headerIconTextButtonClass;
    configureCodexElvesTrigger(menu, trigger, nativeButtonClass);
    menu.appendChild(trigger);
    if (insertionPoint) {
      menu.className = "";
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

  function pluginAutoExpandVisibleElement(el) {
    if (!(el instanceof HTMLElement) || !el.isConnected) return false;
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden" || style.pointerEvents === "none") return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function pluginAutoExpandPageActive() {
    const pluginButton = pluginEntryButton();
    return pluginButton?.getAttribute("aria-current") === "page";
  }

  function pluginAutoExpandPageLooksRelevant() {
    if (pluginAutoExpandPageActive()) return true;
    const routeText = `${location.pathname || ""} ${location.hash || ""} ${document.title || ""}`;
    if (/插件|Plugins?|Marketplace|市场/i.test(routeText)) return true;
    return !!document.querySelector('[data-testid*="plugin" i], [class*="plugin" i], [class*="marketplace" i]');
  }

  function pluginAutoExpandContainer() {
    const pageActive = pluginAutoExpandPageActive();
    const routeSignature = `${location.pathname || ""}\n${location.hash || ""}\n${pageActive}`;
    const cached = window.__codexPluginAutoExpandContainer;
    if (
      cached?.isConnected
      && window.__codexPluginAutoExpandLastRouteSignature === routeSignature
    ) {
      return cached;
    }
    const selectorsForContainer = [
      '[data-testid*="plugin" i]',
      '[class*="plugin" i]',
      '[class*="marketplace" i]',
    ];
    const seed = selectorsForContainer
      .map((selector) => document.querySelector(selector))
      .find(Boolean);
    const latestPluginPage = pageActive
      ? document.querySelector("main, [role='main']")
      : null;
    const container = latestPluginPage || seed?.closest?.(
      '[role="main"], main, [role="dialog"], [data-radix-popper-content-wrapper], [class*="panel" i]'
    ) || seed || null;
    window.__codexPluginAutoExpandContainer = container;
    window.__codexPluginAutoExpandLastRouteSignature = routeSignature;
    window.__codexPluginAutoExpandCandidates = [];
    window.__codexPluginAutoExpandLastContainerSignature = "";
    return container;
  }

  function pluginAutoExpandButtonLooksScoped(button) {
    let node = button;
    for (let depth = 0; node instanceof HTMLElement && node !== document.body && depth < 8; depth += 1, node = node.parentElement) {
      const text = String(node.innerText || "");
      if (text.length > 16000) continue;
      if (/插件|Plugins?|Marketplace|市场/i.test(text)) return true;
    }
    return false;
  }

  function pluginAutoExpandButtonText(button) {
    return String(button?.textContent || button?.getAttribute?.("aria-label") || button?.getAttribute?.("title") || "")
      .replace(/\s+/g, " ")
      .trim();
  }

  function pluginAutoExpandButtonLooksLikeMore(button) {
    const text = pluginAutoExpandButtonText(button);
    if (!text || text.length > 120) return false;
    if (/^(更多|显示更多|查看更多|加载更多|Show more|Load more|More)$/i.test(text)) return true;
    if (/^查看\s+.+以及另外\s*\d+\s*个$/i.test(text)) return true;
    if (/^View\s+.+\s+and\s+\d+\s+more$/i.test(text)) return true;
    if (/^Show\s+.+\s+and\s+\d+\s+more$/i.test(text)) return true;
    return false;
  }

  function pluginAutoExpandButtonCandidates() {
    if (!codexElvesSettings().pluginAutoExpand || !pluginAutoExpandPageLooksRelevant()) return [];
    return Array.from(document.querySelectorAll('button, [role="button"]'))
      .filter(pluginAutoExpandVisibleElement)
      .filter((button) => !button.disabled && button.getAttribute("aria-disabled") !== "true")
      .filter(pluginAutoExpandButtonLooksLikeMore)
      .filter(pluginAutoExpandButtonLooksScoped)
      .filter((button) => !button.closest?.(`.${moreMenuClass}, #${codexElvesMenuId}, .codex-elves-modal-overlay`));
  }

  function pluginAutoExpandSignature() {
    return pluginAutoExpandButtonCandidates()
      .map((button) => {
        const rect = button.getBoundingClientRect();
        return `${pluginAutoExpandButtonText(button)}:${Math.round(rect.top)}:${Math.round(rect.left)}`;
      })
      .join("|");
  }

  function schedulePluginAutoExpand(force = false) {
    if (!codexElvesSettings().pluginAutoExpand) return;
    if (window.__codexPluginAutoExpandRunning && !force) return;
    clearTimeout(window.__codexPluginAutoExpandTimer);
    window.__codexPluginAutoExpandTimer = setTimeout(() => runPluginAutoExpand(force), force ? 30 : 180);
  }

  function runPluginAutoExpand(force = false) {
    if (!codexElvesSettings().pluginAutoExpand) return;
    const currentSignature = pluginAutoExpandSignature();
    if (!force && currentSignature && currentSignature === window.__codexPluginAutoExpandLastSignature) return;
    window.__codexPluginAutoExpandLastSignature = currentSignature;
    window.__codexPluginAutoExpandRunning = true;
    window.__codexPluginAutoExpandClicks = 0;
    const clickNext = () => {
      if (!codexElvesSettings().pluginAutoExpand) {
        window.__codexPluginAutoExpandRunning = false;
        return;
      }
      const button = pluginAutoExpandButtonCandidates()[0];
      if (!button || window.__codexPluginAutoExpandClicks >= codexPluginAutoExpandMaxClicks) {
        window.__codexPluginAutoExpandRunning = false;
        sendCodexElvesDiagnostic("plugin_auto_expand_finished", {
          version: codexPluginAutoExpandVersion,
          clicks: window.__codexPluginAutoExpandClicks || 0,
          exhausted: !!button,
        });
        return;
      }
      window.__codexPluginAutoExpandClicks = (window.__codexPluginAutoExpandClicks || 0) + 1;
      button.dataset.codexPluginAutoExpandClicked = String(Date.now());
      button.click();
      setTimeout(clickNext, codexPluginAutoExpandClickDelayMs);
    };
    clickNext();
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

  function pluginEntryButton() {
    const byIcon = document.querySelector(`${selectors.pluginNavButton} ${selectors.pluginSvgPath}`)?.closest("button");
    if (byIcon) return byIcon;
    return Array.from(document.querySelectorAll(selectors.pluginNavButton))
      .find((button) => /^(插件|Plugins)(\s+-\s+.*)?$/i.test((button.textContent || "").trim())) || null;
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

  function currentSessionRef() {
    const rows = sessionRows();
    for (const row of rows) {
      const ref = sessionRefFromRow(row);
      if (ref.session_id && isCurrentSessionRow(row, ref)) return ref;
    }
    return { session_id: locationThreadId(), title: "" };
  }

  async function postJson(path, payload) {
    if (!window.__codexSessionDeleteBridge) {
      if (path === "/backend/status" || path === "/backend/repair") {
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
    function bridgeWithBackendTimeout(path, payload) {
      return Promise.race([
        window.__codexSessionDeleteBridge(path, payload),
        new Promise((resolve) => setTimeout(() => resolve({ status: "failed", message: "后端检查超时", timeout: true }), 2000)),
      ]);
    }
    async function fetchBackendStatusFromHelper(path, payload) {
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
    const panel = document.querySelector(selectors.pinnedSummaryPanel);
    if (!panel?.parentElement) return null;
    const rect = panel.getBoundingClientRect();
    if (rect.width < 240 || rect.width > 420 || rect.height <= 0) return null;
    return { panel, host: panel.parentElement };
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
    clearTimeout(window.__codexTokenUsageSettleTimer);
    window.__codexTokenUsageSettleTimer = null;
    clearTimeout(window.__codexTokenUsageCompletionSettleTimer);
    window.__codexTokenUsageCompletionSettleTimer = null;
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
    clearTimeout(window.__codexTokenUsageSettleTimer);
    window.__codexTokenUsageSettleTimer = null;
    clearTimeout(window.__codexTokenUsageCompletionSettleTimer);
    window.__codexTokenUsageCompletionSettleTimer = null;
    clearTimeout(window.__codexTokenUsageRetryTimer);
    window.__codexTokenUsageRetryTimer = null;
    window.__codexTokenUsageRetryCount = 0;
    window.__codexTokenUsageRefreshPending = false;
    window.__codexTokenUsageRequestSeq = (window.__codexTokenUsageRequestSeq || 0) + 1;
    window.__codexTokenUsageRequestSession = "";
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

  function scheduleCodexTokenUsageCalibration(delayMs, timerKey) {
    clearTimeout(window[timerKey]);
    window[timerKey] = null;
    if (!codexElvesSettings().tokenUsage) return;
    window[timerKey] = setTimeout(() => {
      window[timerKey] = null;
      scheduleCodexTokenUsageRefresh(0);
    }, Math.max(0, delayMs));
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

  function removeCodexTokenUsageNotificationListener() {
    try {
      window.__codexTokenUsageNotificationUnsubscribe?.();
    } catch {
    }
    window.__codexTokenUsageNotificationUnsubscribe = null;
    window.__codexTokenUsageNotificationManager = null;
  }

  function installCodexTokenUsageNotificationListener(
    manager = codexSessionPrewarmManager || window.__codexElvesSessionPrewarmManager || null
  ) {
    if (!codexElvesSettings().tokenUsage) {
      removeCodexTokenUsageNotificationListener();
      return false;
    }
    if (!manager || typeof manager.addNotificationCallback !== "function") return false;
    if (
      window.__codexTokenUsageNotificationManager === manager
      && typeof window.__codexTokenUsageNotificationUnsubscribe === "function"
    ) {
      return true;
    }
    removeCodexTokenUsageNotificationListener();
    try {
      const callback = (notification) => {
        const method = String(notification?.method || "");
        window.__codexTokenUsageLastNotificationAt = Date.now();
        window.__codexTokenUsageLastNotificationMethod = method;
        resetCodexTokenUsageRetry();
        if (method === "turn/started") {
          clearTimeout(window.__codexTokenUsageCompletionSettleTimer);
          window.__codexTokenUsageCompletionSettleTimer = null;
          scheduleCodexTokenUsageRefresh(0);
          scheduleCodexTokenUsageCalibration(
            codexTokenUsageSettleDelayMs,
            "__codexTokenUsageSettleTimer"
          );
          scheduleCodexTokenUsageCalibration(
            codexTokenUsageCompletionSettleDelayMs,
            "__codexTokenUsageCompletionSettleTimer"
          );
          return;
        }
        if (method === "turn/completed") {
          scheduleCodexTokenUsageRefresh(0);
          scheduleCodexTokenUsageCalibration(
            codexTokenUsageCompletionSettleDelayMs,
            "__codexTokenUsageCompletionSettleTimer"
          );
          return;
        }
        scheduleCodexTokenUsageRefresh(0);
        scheduleCodexTokenUsageCalibration(
          codexTokenUsageSettleDelayMs,
          "__codexTokenUsageSettleTimer"
        );
      };
      const unsubscribe = manager.addNotificationCallback(
        codexTokenUsageNotificationMethods,
        callback
      );
      if (typeof unsubscribe !== "function") return false;
      window.__codexTokenUsageNotificationManager = manager;
      window.__codexTokenUsageNotificationUnsubscribe = unsubscribe;
      sendCodexElvesDiagnostic("token_usage_notification_listener_installed", {
        methods: codexTokenUsageNotificationMethods,
      });
      scheduleCodexTokenUsageRefresh(0);
      return true;
    } catch (error) {
      sendCodexElvesDiagnostic("token_usage_notification_listener_failed", {
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      return false;
    }
  }

  function refreshCodexTokenUsageFeatureState() {
    if (!codexElvesSettings().tokenUsage) {
      removeCodexTokenUsageNotificationListener();
      removeCodexTokenUsagePinnedSummaryObservers();
      installCodexTokenUsageVisibilityListener();
      stopCodexTokenUsageRuntime();
      return;
    }
    installCodexTokenUsageVisibilityListener();
    installCodexTokenUsagePinnedSummaryObserver();
    resetCodexTokenUsageRetry();
    scheduleCodexTokenUsageRefresh(0);
    if (installCodexTokenUsageNotificationListener()) return;
    resetCodexAppServerManagerDiscovery();
    void installAppServerManagerDiscovery(true, true);
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

  function renderCodexTokenUsageSummary(card, summary) {
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
    card.dataset.status = "ready";
    card.dataset.running = String(summary.isRunning === true);
    card.removeAttribute("title");
    card.hidden = false;
    card.innerHTML = `
      <div class="codex-token-usage-header">
        <span class="codex-token-usage-title">Token 用量</span>
        ${descendantLabel}
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
      if (sessionChanged) resetCodexTokenUsageRetry();
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
    const backendRequest = postJson("/thread-usage-history", {
      session_id: sessionId,
      title: sessionTitle,
    });
    const requestPromise = Promise.race([
      backendRequest,
      new Promise((resolve) => {
        timeoutId = setTimeout(
          () => resolve({ status: "failed", message: "读取超时", timeout: true }),
          codexTokenUsageRequestTimeoutMs
        );
      }),
    ]).then((result) => {
      if (requestSeq !== window.__codexTokenUsageRequestSeq) return;
      const activeRef = currentSessionRef();
      if (`${activeRef?.session_id || ""}\n${activeRef?.title || ""}` !== sessionSignature) return;
      const activeMount = codexPinnedSummaryMount();
      const activeCard = activeMount ? ensureCodexTokenUsageCard(activeMount) : null;
      if (activeCard) activeCard.dataset.codexTokenUsageSession = sessionSignature;
      if (result?.status !== "ok") {
        scheduleCodexTokenUsageRetry();
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
      if (!codexTokenUsageHasData(summary.totalUsage)) {
        if (activeCard) renderCodexTokenUsageSummary(activeCard, summary);
        if (activeCard && summary.isRunning && document.visibilityState !== "hidden") {
          scheduleCodexTokenUsageRefresh(codexTokenUsageRefreshIntervalMs);
        }
        return;
      }
      if (activeCard) {
        activeCard.dataset.codexTokenUsageResolvedSession = String(result.session_id || sessionId);
        renderCodexTokenUsageSummary(activeCard, summary);
      }
      if (activeCard && summary.isRunning && document.visibilityState !== "hidden") {
        scheduleCodexTokenUsageRefresh(codexTokenUsageRefreshIntervalMs);
      }
    }).catch(() => {
      if (requestSeq !== window.__codexTokenUsageRequestSeq) return;
      scheduleCodexTokenUsageRetry();
      const activeCard = document.querySelector(`.${codexTokenUsageCardClass}`);
      if (activeCard && activeCard.dataset.status !== "ready") {
        renderCodexTokenUsageStatus(activeCard, "failed", "Token 统计暂不可用。");
      }
    }).finally(() => {
      if (timeoutId) clearTimeout(timeoutId);
    });
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
  let codexAppServerManagerDiscoveryPromise = null;
  let codexAppServerManagerDiscoveryFailureCount = 0;
  let codexAppServerManagerDiscoveryNextAttemptAt = 0;
  let codexAppServerManagerDiscoveryFailureSignature = "";
  let codexAppServerManagerDiscoveryRetryExhausted = false;
  let codexSessionPrewarmManager = window.__codexElvesSessionPrewarmManager || null;
  let codexSessionPrewarmRunPromise = null;
  let codexSessionPrewarmRerunPending = false;
  let codexSessionPrewarmFeatureRefreshPromise = null;
  let codexSessionPrewarmManagerSequence = window.__codexSessionPrewarmManagerSequence || 0;
  const codexSessionPrewarmTaskPromises = new Map();
  const codexSessionPrewarmForegroundIds = new Set();
  const codexSessionPrewarmActiveIds = new Set();
  const codexSessionPrewarmRetryCounts = new Map();




  function codexSessionPrewarmSettingsSnapshot() {
    const settings = codexElvesSettings();
    return {
      enabled: settings.sessionPrewarmEnabled === true,
      fullCount: clampSessionPrewarmCount(settings.sessionPrewarmFullCount, 3, 4),
      contentCount: clampSessionPrewarmCount(settings.sessionPrewarmContentCount, 3, 6),
      concurrency: clampSessionPrewarmConcurrency(settings.sessionPrewarmConcurrency),
    };
  }

  function resetCodexSessionPrewarmForLaunchCycle() {
    const launchCycle = String(window.__CODEX_ELVES_LAUNCH_CYCLE__ || "").trim();
    if (!launchCycle || window.__codexSessionPrewarmLaunchCycle === launchCycle) return false;
    const previousLaunchCycle = String(window.__codexSessionPrewarmLaunchCycle || "");
    window.__codexSessionPrewarmLaunchCycle = launchCycle;
    window.__codexSessionPrewarmCompletedSignature = "";
    codexSessionPrewarmRetryCounts.clear();
    codexSessionPrewarmRerunPending = true;
    sendCodexElvesDiagnostic("session_prewarm_launch_cycle_reset", {
      previousLaunchCycle,
      launchCycle,
    });
    return true;
  }

  resetCodexSessionPrewarmForLaunchCycle();

  function codexSessionPrewarmManagerId(manager) {
    if (!manager || (typeof manager !== "object" && typeof manager !== "function")) return "";
    if (!manager.__codexElvesSessionPrewarmManagerId) {
      codexSessionPrewarmManagerSequence += 1;
      window.__codexSessionPrewarmManagerSequence = codexSessionPrewarmManagerSequence;
      try {
        Object.defineProperty(manager, "__codexElvesSessionPrewarmManagerId", {
          configurable: true,
          value: `${codexElvesBuild}:${codexSessionPrewarmManagerSequence}`,
        });
      } catch {
        manager.__codexElvesSessionPrewarmManagerId = `${codexElvesBuild}:${codexSessionPrewarmManagerSequence}`;
      }
    }
    return String(manager.__codexElvesSessionPrewarmManagerId || "");
  }

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

  function captureCodexSessionPrewarmManager(candidate) {
    if (!isCodexSessionPrewarmManager(candidate)) return false;
    let hostId = "";
    try {
      hostId = String(candidate.getHostId() || "");
    } catch {
      return false;
    }
    if (hostId !== "local") return false;
    const changed = codexSessionPrewarmManager !== candidate;
    codexSessionPrewarmManager = candidate;
    window.__codexElvesSessionPrewarmManager = candidate;
    window.__codexSessionPrewarmReactManagerDiscovery = null;
    codexSessionPrewarmManagerId(candidate);
    if (changed) {
      sendCodexElvesDiagnostic("session_prewarm_manager_captured", {
        version: codexSessionPrewarmVersion,
        hasResume: typeof candidate.resumeConversationForUnavailableOwner === "function",
        hasUnsubscribe: typeof candidate.unsubscribeInactiveConversation === "function",
        hasBackgroundHydration: typeof candidate.hydrateBackgroundThreads === "function",
      });
    }
    installCodexTokenUsageNotificationListener(candidate);
    scheduleCodexSessionPrewarm(codexSessionPrewarmStartupDelayMs, "manager-captured");
    return true;
  }

  function invalidateCodexSessionPrewarmManager(manager, reason) {
    if (!manager || codexSessionPrewarmManager !== manager) return false;
    codexSessionPrewarmManager = null;
    window.__codexElvesSessionPrewarmManager = null;
    if (window.__codexTokenUsageNotificationManager === manager) {
      removeCodexTokenUsageNotificationListener();
    }
    window.__codexSessionPrewarmCompletedSignature = "";
    window.__codexSessionPrewarmReactManagerDiscovery = null;
    sendCodexElvesDiagnostic("session_prewarm_manager_invalidated", { reason });
    resetCodexAppServerManagerDiscovery();
    void installAppServerManagerDiscovery(true, true);
    return true;
  }

  function codexSessionPrewarmConversationId(conversation) {
    return validThreadSessionKey(
      conversation?.id ||
      conversation?.threadId ||
      conversation?.conversationId ||
      conversation?.thread?.id ||
      ""
    );
  }

  function codexSessionPrewarmConversationIsSubagent(conversation) {
    return Boolean(
      conversation?.parentThreadId ||
      conversation?.source?.parentThreadId ||
      conversation?.subagentParentThreadId ||
      conversation?.isSubagentSource === true
    );
  }

  function codexSessionPrewarmConversationIsBusy(conversation) {
    if (
      conversation?.ephemeral === true ||
      conversation?.sideConversation === true ||
      conversation?.archived === true ||
      codexSessionPrewarmConversationIsSubagent(conversation)
    ) return true;
    if (conversation?.threadRuntimeStatus?.type === "active") return true;
    const turns = Array.isArray(conversation?.turns) ? conversation.turns : [];
    return turns.some((turn) => turn?.status === "inProgress");
  }

  function codexSessionPrewarmConversationUpdatedAtMs(conversation) {
    const candidates = [
      conversation?.updatedAt,
      conversation?.updatedAtMs,
      conversation?.updated_at_ms,
      conversation?.updated_at,
      conversation?.thread?.updatedAt,
      conversation?.thread?.updatedAtMs,
      conversation?.thread?.updated_at_ms,
      conversation?.thread?.updated_at,
    ];
    for (const candidate of candidates) {
      const timestampMs = timestampValueToMs(candidate);
      if (timestampMs) return timestampMs;
    }
    return 0;
  }

  function codexSessionPrewarmConversationIsRecent(conversation, nowMs = Date.now()) {
    const updatedAtMs = codexSessionPrewarmConversationUpdatedAtMs(conversation);
    if (!updatedAtMs) return false;
    return Math.max(0, numericTimestamp(nowMs) - updatedAtMs) <= codexSessionPrewarmMaxAgeMs;
  }

  function codexSessionPrewarmTitleNode(row) {
    return row?.querySelector?.(`${selectors.threadTitle}, .truncate.select-none, .truncate.text-base`) || null;
  }

  function clearCodexSessionPrewarmIndicator(titleNode) {
    if (!titleNode) return;
    titleNode.removeAttribute?.("data-codex-session-prewarming");
    titleNode.removeAttribute?.("data-codex-session-prewarm-title");
  }

  function syncCodexSessionPrewarmIndicators(rows = sessionRows(true)) {
    const matchedTitles = new Set();
    for (const row of Array.from(rows || [])) {
      const titleNode = codexSessionPrewarmTitleNode(row);
      if (!titleNode) continue;
      matchedTitles.add(titleNode);
      const threadId = validThreadSessionKey(sessionRefFromRow(row).session_id);
      if (!threadId || !codexSessionPrewarmActiveIds.has(threadId)) {
        clearCodexSessionPrewarmIndicator(titleNode);
        continue;
      }
      titleNode.setAttribute?.("data-codex-session-prewarming", "true");
      titleNode.setAttribute?.(
        "data-codex-session-prewarm-title",
        String(titleNode.textContent || "").trim()
      );
    }
    document.querySelectorAll?.(
      '[data-codex-session-prewarming], [data-codex-session-prewarm-title]'
    ).forEach((titleNode) => {
      if (matchedTitles.has(titleNode)) return;
      const row = titleNode.closest?.(selectors.sidebarThread);
      const threadId = row ? validThreadSessionKey(sessionRefFromRow(row).session_id) : "";
      if (!threadId || !codexSessionPrewarmActiveIds.has(threadId)) {
        clearCodexSessionPrewarmIndicator(titleNode);
      }
    });
  }

  function setCodexSessionPrewarmIndicatorActive(threadId, active) {
    const id = validThreadSessionKey(threadId);
    if (!id) return;
    if (active) codexSessionPrewarmActiveIds.add(id);
    else codexSessionPrewarmActiveIds.delete(id);
    syncCodexSessionPrewarmIndicators();
  }

  syncCodexSessionPrewarmIndicators();

  function buildCodexSessionPrewarmTasks(
    conversations,
    settings,
    activeThreadId = "",
    nowMs = Date.now()
  ) {
    const activeId = validThreadSessionKey(activeThreadId);
    const seen = new Set();
    const eligible = [];
    for (const conversation of Array.isArray(conversations) ? conversations : []) {
      const threadId = codexSessionPrewarmConversationId(conversation);
      if (
        !threadId ||
        threadId === activeId ||
        seen.has(threadId) ||
        codexSessionPrewarmConversationIsBusy(conversation) ||
        !codexSessionPrewarmConversationIsRecent(conversation, nowMs)
      ) continue;
      seen.add(threadId);
      eligible.push({ conversation, threadId });
    }
    const full = eligible.slice(0, settings.fullCount).map((entry) => ({ ...entry, type: "full" }));
    const content = eligible
      .slice(settings.fullCount, settings.fullCount + settings.contentCount)
      .map((entry) => ({ ...entry, type: "content" }));
    return [...full, ...content];
  }

  function codexSessionPrewarmResumeParams(task) {
    const cwd = String(task.conversation?.cwd || "").trim();
    return {
      conversationId: task.threadId,
      model: null,
      serviceTier: null,
      reasoningEffort: null,
      workspaceRoots: cwd ? [cwd] : [],
      permissions: null,
      collaborationMode: null,
      showThreadGoalResumeConfirmation: false,
    };
  }

  async function hydrateCodexSessionPrewarmContent(manager, task) {
    if (typeof manager.hydrateBackgroundThreads === "function") {
      await manager.hydrateBackgroundThreads([task.threadId], { includeTurns: true });
      return "content-hydrated";
    }
    if (typeof manager.readThread === "function") {
      await manager.readThread(task.threadId, { includeTurns: true });
      return "content-read";
    }
    return "content-unavailable";
  }

  async function ensureCodexSessionPrewarmResumed(manager, task) {
    if (typeof manager.needsResume === "function") {
      try {
        if (!manager.needsResume(task.threadId)) return "already-resumed";
      } catch {
      }
    }
    const canResume = typeof manager.resumeConversationForUnavailableOwner === "function";
    if (!canResume) {
      return hydrateCodexSessionPrewarmContent(manager, task);
    }
    await manager.resumeConversationForUnavailableOwner(codexSessionPrewarmResumeParams(task));
    return "resumed";
  }

  async function runCodexSessionPrewarmTask(manager, task) {
    const existing = codexSessionPrewarmTaskPromises.get(task.threadId);
    if (existing) return existing.promise;
    const startedAt = Date.now();
    const entry = { type: task.type, promise: null };
    setCodexSessionPrewarmIndicatorActive(task.threadId, true);
    const promise = Promise.resolve().then(async () => {
      const currentThreadId = validThreadSessionKey(currentSessionRef().session_id);
      const promoted = codexSessionPrewarmForegroundIds.has(task.threadId) || currentThreadId === task.threadId;
      const effectiveType = promoted ? "full" : task.type;
      const phase = effectiveType === "content" ? "content" : "owner";
      const phaseStartedAt = Date.now();
      const result = effectiveType === "content"
        ? await hydrateCodexSessionPrewarmContent(manager, task)
        : await ensureCodexSessionPrewarmResumed(manager, { ...task, type: "full" });
      const phaseDurationMs = Date.now() - phaseStartedAt;
      sendCodexElvesDiagnostic("session_prewarm_task_phase_completed", {
        type: effectiveType,
        phase,
        result,
        durationMs: phaseDurationMs,
      });
      sendCodexElvesDiagnostic("session_prewarm_task_completed", {
        type: effectiveType,
        phase,
        result,
        durationMs: Date.now() - startedAt,
      });
      return { type: effectiveType, phase, result, durationMs: phaseDurationMs };
    }).catch((error) => {
      sendCodexElvesDiagnostic("session_prewarm_task_failed", {
        type: task.type,
        durationMs: Date.now() - startedAt,
        errorName: error?.name || "",
        errorMessage: error?.message || String(error),
      });
      return { type: task.type, result: "failed" };
    }).finally(() => {
      setCodexSessionPrewarmIndicatorActive(task.threadId, false);
      codexSessionPrewarmForegroundIds.delete(task.threadId);
      if (codexSessionPrewarmTaskPromises.get(task.threadId) === entry) {
        codexSessionPrewarmTaskPromises.delete(task.threadId);
      }
    });
    entry.promise = promise;
    codexSessionPrewarmTaskPromises.set(task.threadId, entry);
    return promise;
  }

  async function waitForCodexSessionPrewarmIdle() {
    while (codexSessionPrewarmRuntimeId === window.__codexSessionPrewarmRuntimeId) {
      const visible = !document.visibilityState || document.visibilityState === "visible";
      if (!visible) return false;
      const lastInteractionAt = Number(window.__codexSessionPrewarmLastInteractionAt || 0);
      const remainingPause = codexSessionPrewarmInteractionPauseMs - (Date.now() - lastInteractionAt);
      if (remainingPause <= 0) return true;
      await new Promise((resolve) => setTimeout(resolve, Math.min(500, Math.max(100, remainingPause))));
    }
    return false;
  }

  async function runCodexSessionPrewarmQueue(
    manager,
    tasks,
    concurrency = codexSessionPrewarmDefaultConcurrency,
    taskRunner = runCodexSessionPrewarmTask
  ) {
    let nextIndex = 0;
    const results = new Array(tasks.length);
    const workerCount = Math.min(
      clampSessionPrewarmConcurrency(concurrency),
      tasks.length
    );
    const workers = Array.from({ length: workerCount }, async () => {
      while (nextIndex < tasks.length) {
        if (!await waitForCodexSessionPrewarmIdle()) return;
        const index = nextIndex;
        nextIndex += 1;
        const task = tasks[index];
        if (!task) return;
        if (validThreadSessionKey(currentSessionRef().session_id) === task.threadId) {
          results[index] = { type: "full", result: "foreground" };
          continue;
        }
        results[index] = await taskRunner(manager, task);
      }
    });
    await Promise.all(workers);
    const completedResults = results.filter(Boolean);
    return {
      completed: completedResults.length,
      failed: completedResults.filter((result) => result.result === "failed").length,
      interrupted: completedResults.length < tasks.length,
      results: completedResults,
      indexedResults: results,
    };
  }

  async function runCodexSessionPrewarmPhasedQueue(
    manager,
    tasks,
    concurrency = codexSessionPrewarmDefaultConcurrency
  ) {
    const contentTasks = tasks.map((task) => ({ ...task, type: "content" }));
    const contentSummary = await runCodexSessionPrewarmQueue(
      manager,
      contentTasks,
      concurrency
    );
    const contentResultsByThreadId = new Map();
    for (let index = 0; index < contentTasks.length; index += 1) {
      const result = contentSummary.indexedResults[index];
      if (result) contentResultsByThreadId.set(contentTasks[index].threadId, result);
    }
    const fullTasks = tasks.filter((task) => {
      if (task.type !== "full") return false;
      const contentResult = contentResultsByThreadId.get(task.threadId);
      return contentResult && contentResult.result !== "failed";
    });
    const ownerSummary = contentSummary.interrupted
      ? {
          completed: 0,
          failed: 0,
          interrupted: fullTasks.length > 0,
          results: [],
          indexedResults: [],
        }
      : await runCodexSessionPrewarmQueue(manager, fullTasks, concurrency);
    const ownerResultsByThreadId = new Map();
    for (let index = 0; index < fullTasks.length; index += 1) {
      const result = ownerSummary.indexedResults[index];
      if (result) ownerResultsByThreadId.set(fullTasks[index].threadId, result);
    }
    const finalResults = [];
    for (const task of tasks) {
      const contentResult = contentResultsByThreadId.get(task.threadId);
      if (!contentResult) continue;
      if (contentResult.result === "failed" || task.type === "content") {
        finalResults.push(contentResult);
        continue;
      }
      const ownerResult = ownerResultsByThreadId.get(task.threadId);
      if (ownerResult) finalResults.push(ownerResult);
    }
    return {
      completed: finalResults.length,
      failed: finalResults.filter((result) => result.result === "failed").length,
      interrupted:
        contentSummary.interrupted ||
        ownerSummary.interrupted ||
        finalResults.length < tasks.length,
      results: finalResults,
      contentSummary,
      ownerSummary,
    };
  }

  function scheduleCodexSessionPrewarmRetry(signature, reason, detail = {}) {
    const retryCount = (codexSessionPrewarmRetryCounts.get(signature) || 0) + 1;
    if (retryCount > codexSessionPrewarmMaxRetries) {
      codexSessionPrewarmRetryCounts.delete(signature);
      sendCodexElvesDiagnostic("session_prewarm_retry_exhausted", {
        reason,
        ...detail,
      });
      return false;
    }
    codexSessionPrewarmRetryCounts.set(signature, retryCount);
    const delayMs = codexSessionPrewarmRetryBaseDelayMs * (2 ** (retryCount - 1));
    sendCodexElvesDiagnostic("session_prewarm_retry_scheduled", {
      reason,
      retryCount,
      delayMs,
      ...detail,
    });
    scheduleCodexSessionPrewarm(delayMs, `retry-${reason}-${retryCount}`);
    return true;
  }

  async function refreshCodexSessionPrewarmRecentConversations(
    manager,
    timeoutMs = codexSessionPrewarmRecentRefreshTimeoutMs
  ) {
    if (typeof manager?.refreshRecentConversations !== "function") {
      return { status: "unavailable", durationMs: 0 };
    }
    const startedAt = Date.now();
    const boundedTimeoutMs = Math.max(0, Number(timeoutMs) || 0);
    let timeoutId = 0;
    sendCodexElvesDiagnostic("session_prewarm_recent_refresh_started", {
      timeoutMs: boundedTimeoutMs,
    });
    const refresh = Promise.resolve()
      .then(() => manager.refreshRecentConversations({ mode: "routine" }))
      .then(
        () => ({ status: "completed" }),
        (error) => ({
          status: "failed",
          errorName: error?.name || "",
          errorMessage: error?.message || String(error),
        })
      );
    const timeout = new Promise((resolve) => {
      timeoutId = setTimeout(
        () => resolve({ status: "timeout" }),
        boundedTimeoutMs
      );
    });
    const result = await Promise.race([refresh, timeout]);
    clearTimeout(timeoutId);
    const detail = {
      ...result,
      timeoutMs: boundedTimeoutMs,
      durationMs: Date.now() - startedAt,
    };
    if (result.status === "completed") {
      sendCodexElvesDiagnostic("session_prewarm_recent_refresh_completed", detail);
    } else if (result.status === "timeout") {
      sendCodexElvesDiagnostic("session_prewarm_recent_refresh_timeout", detail);
    } else {
      sendCodexElvesDiagnostic("session_prewarm_recent_refresh_failed", detail);
    }
    return detail;
  }

  async function runCodexSessionPrewarm(
    recentRefreshTimeoutMs = codexSessionPrewarmRecentRefreshTimeoutMs
  ) {
    const manager = codexSessionPrewarmManager;
    const settings = codexSessionPrewarmSettingsSnapshot();
    if (!manager) {
      sendCodexElvesDiagnostic("session_prewarm_skipped", { reason: "manager-unavailable" });
      return;
    }
    if (!settings.enabled) {
      sendCodexElvesDiagnostic("session_prewarm_skipped", { reason: "disabled" });
      return;
    }
    if (settings.fullCount + settings.contentCount === 0) {
      sendCodexElvesDiagnostic("session_prewarm_skipped", { reason: "empty-range" });
      return;
    }
    if (codexSessionPrewarmRunPromise) {
      codexSessionPrewarmRerunPending = true;
      sendCodexElvesDiagnostic("session_prewarm_skipped", { reason: "run-in-progress" });
      return codexSessionPrewarmRunPromise;
    }
    const signature = [
      codexSessionPrewarmVersion,
      codexSessionPrewarmManagerId(manager),
      settings.fullCount,
      settings.contentCount,
      settings.concurrency,
    ].join(":");
    if (window.__codexSessionPrewarmCompletedSignature === signature) {
      sendCodexElvesDiagnostic("session_prewarm_skipped", { reason: "already-completed" });
      return;
    }
    codexSessionPrewarmRerunPending = false;
    const run = Promise.resolve().then(async () => {
      const conversations = manager.getRecentConversations();
      const prewarmNow = Date.now();
      const recentConversationCount = Array.isArray(conversations)
        ? conversations.filter((conversation) =>
            codexSessionPrewarmConversationIsRecent(conversation, prewarmNow)
          ).length
        : 0;
      const tasks = buildCodexSessionPrewarmTasks(
        conversations,
        settings,
        currentSessionRef().session_id,
        prewarmNow
      );
      if (!tasks.length) {
        sendCodexElvesDiagnostic("session_prewarm_no_tasks", {
          recentCount: Array.isArray(conversations) ? conversations.length : 0,
          recentWithinWindowCount: recentConversationCount,
        });
        if (Array.isArray(conversations) && conversations.length > 0 && recentConversationCount === 0) {
          window.__codexSessionPrewarmCompletedSignature = signature;
          codexSessionPrewarmRetryCounts.delete(signature);
          return;
        }
        void refreshCodexSessionPrewarmRecentConversations(
          manager,
          recentRefreshTimeoutMs
        ).then(() => {
          scheduleCodexSessionPrewarmRetry(signature, "no-tasks", {
            recentCount: Array.isArray(conversations) ? conversations.length : 0,
          });
        });
        return;
      }
      sendCodexElvesDiagnostic("session_prewarm_started", {
        fullCount: tasks.filter((task) => task.type === "full").length,
        contentCount: tasks.filter((task) => task.type === "content").length,
        concurrency: settings.concurrency,
        strategy: "content-first",
      });
      const summary = await runCodexSessionPrewarmPhasedQueue(
        manager,
        tasks,
        settings.concurrency
      );
      void refreshCodexSessionPrewarmRecentConversations(
        manager,
        recentRefreshTimeoutMs
      );
      const fullyCompleted = !summary.interrupted && summary.completed === tasks.length;
      if (fullyCompleted && summary.failed === 0 && codexSessionPrewarmRuntimeId === window.__codexSessionPrewarmRuntimeId) {
        window.__codexSessionPrewarmCompletedSignature = signature;
        codexSessionPrewarmRetryCounts.delete(signature);
      } else if (summary.failed > 0) {
        const retryScheduled = scheduleCodexSessionPrewarmRetry(signature, "task-failed", {
          failed: summary.failed,
          taskCount: tasks.length,
        });
        if (!retryScheduled && summary.failed === tasks.length) {
          invalidateCodexSessionPrewarmManager(manager, "all-tasks-failed");
        }
      } else if (summary.interrupted) {
        sendCodexElvesDiagnostic("session_prewarm_interrupted", {
          completed: summary.completed,
          taskCount: tasks.length,
        });
      }
      sendCodexElvesDiagnostic("session_prewarm_completed", {
        completed: summary.completed,
        failed: summary.failed,
        taskCount: tasks.length,
      });
    }).catch((error) => {
      const errorName = error?.name || "";
      const errorMessage = error?.message || String(error);
      sendCodexElvesDiagnostic("session_prewarm_run_failed", {
        errorName,
        errorMessage,
      });
      const retryScheduled = scheduleCodexSessionPrewarmRetry(signature, "run-failed", {
        errorName,
        errorMessage,
      });
      if (!retryScheduled) invalidateCodexSessionPrewarmManager(manager, "run-failed");
    }).finally(() => {
      if (codexSessionPrewarmRunPromise !== run) return;
      codexSessionPrewarmRunPromise = null;
      if (codexSessionPrewarmRerunPending && codexSessionPrewarmRuntimeId === window.__codexSessionPrewarmRuntimeId) {
        codexSessionPrewarmRerunPending = false;
        scheduleCodexSessionPrewarm(0, "pending-rerun");
      }
    });
    codexSessionPrewarmRunPromise = run;
    return run;
  }

  function scheduleCodexSessionPrewarm(delayMs = codexSessionPrewarmStartupDelayMs, reason = "scheduled") {
    clearTimeout(window.__codexSessionPrewarmTimer);
    window.__codexSessionPrewarmTimer = null;
    const settings = codexSessionPrewarmSettingsSnapshot();
    if (
      !settings.enabled ||
      settings.fullCount + settings.contentCount === 0 ||
      !codexSessionPrewarmManager
    ) return;
    const runtimeId = codexSessionPrewarmRuntimeId;
    window.__codexSessionPrewarmTimer = setTimeout(() => {
      window.__codexSessionPrewarmTimer = null;
      if (runtimeId !== window.__codexSessionPrewarmRuntimeId) return;
      sendCodexElvesDiagnostic("session_prewarm_scheduled_run", { reason, delayMs });
      void runCodexSessionPrewarm();
    }, Math.max(0, delayMs));
  }

  function markCodexSessionPrewarmInteraction(event) {
    window.__codexSessionPrewarmLastInteractionAt = Date.now();
    const target =
      typeof Element !== "undefined" && event?.target instanceof Element
        ? event.target
        : event?.target?.parentElement;
    const row = target?.closest?.(selectors.sidebarThread);
    if (!row) return;
    const threadId = validThreadSessionKey(sessionRefFromRow(row).session_id);
    if (!threadId) return;
    const active = codexSessionPrewarmTaskPromises.get(threadId);
    if (active?.promise && active.type === "content" && codexSessionPrewarmManager) {
      codexSessionPrewarmForegroundIds.add(threadId);
      void active.promise.finally(() => {
        const conversation = codexSessionPrewarmManager?.getConversation?.(threadId) || { id: threadId, cwd: "" };
        return runCodexSessionPrewarmTask(codexSessionPrewarmManager, {
          type: "full",
          threadId,
          conversation,
        });
      });
    }
  }

  function installCodexSessionPrewarmInteractionHooks() {
    document.removeEventListener("pointerdown", window.__codexSessionPrewarmPointerHandler, true);
    document.removeEventListener("keydown", window.__codexSessionPrewarmKeyboardHandler, true);
    document.removeEventListener("visibilitychange", window.__codexSessionPrewarmVisibilityHandler, true);
    window.__codexSessionPrewarmPointerHandler = markCodexSessionPrewarmInteraction;
    window.__codexSessionPrewarmKeyboardHandler = markCodexSessionPrewarmInteraction;
    window.__codexSessionPrewarmVisibilityHandler = () => {
      if (!document.visibilityState || document.visibilityState === "visible") {
        scheduleCodexSessionPrewarm(0, "visibility-restored");
      }
    };
    document.addEventListener("pointerdown", window.__codexSessionPrewarmPointerHandler, true);
    document.addEventListener("keydown", window.__codexSessionPrewarmKeyboardHandler, true);
    document.addEventListener("visibilitychange", window.__codexSessionPrewarmVisibilityHandler, true);
  }

  if (window.__CODEX_ELVES_TEST_SESSION_PREWARM__) {
    window.__codexElvesSessionPrewarmTest = {
      activeIndicatorIds: () => [...codexSessionPrewarmActiveIds],
      buildTasks: (conversations, settings, activeThreadId = "", nowMs = Date.now()) =>
        buildCodexSessionPrewarmTasks(conversations, settings, activeThreadId, nowMs),
      captureManager: (manager) => captureCodexSessionPrewarmManager(manager),
      clearScheduledRun: () => {
        clearTimeout(window.__codexSessionPrewarmTimer);
        window.__codexSessionPrewarmTimer = null;
      },
      clearRetryCounts: () => codexSessionPrewarmRetryCounts.clear(),
      completedSignature: () => window.__codexSessionPrewarmCompletedSignature || "",
      managerReady: () => !!codexSessionPrewarmManager,
      findManagerFromRoots: (roots, maxNodes) =>
        findCodexSessionPrewarmManagerInObjectGraph(roots, maxNodes),
      isForeground: (threadId) => codexSessionPrewarmForegroundIds.has(validThreadSessionKey(threadId)),
      markForeground: (threadId) => codexSessionPrewarmForegroundIds.add(validThreadSessionKey(threadId)),
      managerDiscoveryVersion: codexAppServerManagerDiscoveryVersion,
      managerDiscoveryNeeded: () => codexAppServerManagerDiscoveryNeeded(),
      resumeParams: (task) => codexSessionPrewarmResumeParams(task),
      retryCounts: () => [...codexSessionPrewarmRetryCounts.values()],
      refreshRecent: (manager, timeoutMs) =>
        refreshCodexSessionPrewarmRecentConversations(manager, timeoutMs),
      resetLaunchCycle: () => resetCodexSessionPrewarmForLaunchCycle(),
      run: (recentRefreshTimeoutMs) =>
        runCodexSessionPrewarm(recentRefreshTimeoutMs),
      runQueue: (manager, tasks, concurrency) =>
        runCodexSessionPrewarmQueue(manager, tasks, concurrency),
      runPhasedQueue: (manager, tasks, concurrency) =>
        runCodexSessionPrewarmPhasedQueue(manager, tasks, concurrency),
      runTask: (manager, task) => runCodexSessionPrewarmTask(manager, task),
      setIndicatorActive: (threadId, active) =>
        setCodexSessionPrewarmIndicatorActive(threadId, active),
      setManager: (manager) => {
        codexSessionPrewarmManager = manager || null;
        window.__codexElvesSessionPrewarmManager = codexSessionPrewarmManager;
      },
      settingsSnapshot: () => codexSessionPrewarmSettingsSnapshot(),
      syncIndicators: (rows) => syncCodexSessionPrewarmIndicators(rows),
    };
  }

  if (window.__CODEX_ELVES_TEST_PLUGIN_AUTO_EXPAND__) {
    window.__codexElvesPluginAutoExpandTest = {
      matchesText: (text) =>
        pluginAutoExpandButtonLooksLikeMore({
          textContent: String(text || ""),
          getAttribute: () => "",
        }),
    };
  }

  if (window.__CODEX_ELVES_TEST_SERVICE_TIER__) {
    window.__codexElvesServiceTierTest = {
      applyServiceTierOverride: (method, params, threadIdHint = "") => applyCodexServiceTierRequestOverride(method, params, threadIdHint),
      requestOverride: (message) => codexServiceTierRequestOverride(message),
      patchRequestClientPrototype: (klass) => patchCodexServiceTierRequestClientPrototype(klass),
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
      patchStatsigModelVisibilityConfig: (config) => patchStatsigModelVisibilityConfig(config),
      patchPluginMarketplaceRequestParams: (method, params) => patchPluginMarketplaceRequestParams(method, params),
      patchPluginMarketplaceRequestClient: (client) => patchPluginMarketplaceRequestClient(client),
      patchPluginMarketplaceResult: (method, result) => patchPluginMarketplaceResult(method, result),
      appServerManagerDiscoveryBackoffMs: (failureCount) => {
        const previousFailureCount = codexAppServerManagerDiscoveryFailureCount;
        codexAppServerManagerDiscoveryFailureCount = failureCount;
        try {
          return codexAppServerManagerDiscoveryBackoffMs();
        } finally {
          codexAppServerManagerDiscoveryFailureCount = previousFailureCount;
        }
      },
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

  function codexAppServerManagerDiscoveryBackoffMs() {
    return Math.min(30000, 1000 * (2 ** Math.min(Math.max(codexAppServerManagerDiscoveryFailureCount - 1, 0), 5)));
  }

  function resetCodexAppServerManagerDiscovery() {
    clearTimeout(window.__codexAppServerManagerDiscoveryRetryTimer);
    window.__codexAppServerManagerDiscoveryRetryTimer = null;
    codexAppServerManagerDiscoveryFailureCount = 0;
    codexAppServerManagerDiscoveryNextAttemptAt = 0;
    codexAppServerManagerDiscoveryFailureSignature = "";
    codexAppServerManagerDiscoveryRetryExhausted = false;
  }

  function scheduleCodexAppServerManagerDiscoveryRetry(delayMs) {
    clearTimeout(window.__codexAppServerManagerDiscoveryRetryTimer);
    window.__codexAppServerManagerDiscoveryRetryTimer = null;
    if (codexAppServerManagerDiscoveryFailureCount >= codexAppServerManagerDiscoveryMaxFailures) {
      if (!codexAppServerManagerDiscoveryRetryExhausted) {
        codexAppServerManagerDiscoveryRetryExhausted = true;
        sendCodexElvesDiagnostic("app_server_manager_discovery_retry_exhausted", {
          failureCount: codexAppServerManagerDiscoveryFailureCount,
        });
      }
      return false;
    }
    const runtimeId = codexSessionPrewarmRuntimeId;
    window.__codexAppServerManagerDiscoveryRetryTimer = setTimeout(() => {
      window.__codexAppServerManagerDiscoveryRetryTimer = null;
      if (runtimeId !== window.__codexSessionPrewarmRuntimeId) return;
      void installAppServerManagerDiscovery(true);
    }, Math.max(0, delayMs));
    return true;
  }

  function codexAppServerManagerDiscoveryNeeded(rediscoverManager = false) {
    const prewarmSettings = codexSessionPrewarmSettingsSnapshot();
    const tokenUsageEnabled = codexElvesSettings().tokenUsage;
    return (
      prewarmSettings.enabled &&
      prewarmSettings.fullCount + prewarmSettings.contentCount > 0
      || tokenUsageEnabled
    ) && (rediscoverManager || !codexSessionPrewarmManager);
  }

  function installAppServerManagerDiscovery(force = false, rediscoverManager = false) {
    if (!codexAppServerManagerDiscoveryNeeded(rediscoverManager)) return;
    if (codexAppServerManagerDiscoveryPromise) return codexAppServerManagerDiscoveryPromise;
    if (!force && codexAppServerManagerDiscoveryFailureCount >= codexAppServerManagerDiscoveryMaxFailures) return null;
    if (!force && Date.now() < codexAppServerManagerDiscoveryNextAttemptAt) return null;
    const discovery = async () => {
      try {
        const module = await loadCodexAppModule("app-server-manager-signals-");
        const candidates = Object.values(module).filter((value) => value && (typeof value === "object" || typeof value === "function"));
        let managerCount = 0;
        let reactScannedCount = 0;
        let reactManagerFound = false;
        for (const candidate of candidates) {
          if (captureCodexSessionPrewarmManager(candidate)) managerCount += 1;
          if (typeof candidate.get === "function") {
            try {
              const resolved = candidate.get();
              if (captureCodexSessionPrewarmManager(resolved)) managerCount += 1;
            } catch {
            }
          }
        }
        if (!codexSessionPrewarmManager) {
          const reactResult = findCodexSessionPrewarmManagerInReactTree(rediscoverManager);
          reactScannedCount = reactResult.scanned;
          const reactManager = reactResult.manager;
          if (reactManager) {
            reactManagerFound = true;
            if (captureCodexSessionPrewarmManager(reactManager)) managerCount += 1;
          }
        }
        const managerReady = !!codexSessionPrewarmManager;
        if (managerReady) {
          clearTimeout(window.__codexAppServerManagerDiscoveryRetryTimer);
          window.__codexAppServerManagerDiscoveryRetryTimer = null;
          codexAppServerManagerDiscoveryFailureCount = 0;
          codexAppServerManagerDiscoveryNextAttemptAt = 0;
          codexAppServerManagerDiscoveryFailureSignature = "";
          codexAppServerManagerDiscoveryRetryExhausted = false;
          sendCodexElvesDiagnostic("app_server_manager_discovery_completed", {
            candidateCount: candidates.length,
            managerCount,
            managerReady,
            reactScannedCount,
            reactManagerFound,
          });
        } else {
          codexAppServerManagerDiscoveryFailureCount += 1;
          const retryAfterMs = codexAppServerManagerDiscoveryBackoffMs();
          codexAppServerManagerDiscoveryNextAttemptAt = Date.now() + retryAfterMs;
          scheduleCodexAppServerManagerDiscoveryRetry(retryAfterMs);
          const exportCount = Object.keys(module || {}).length;
          const failureSignature = `${exportCount}:${candidates.length}`;
          if (codexAppServerManagerDiscoveryFailureSignature !== failureSignature) {
            codexAppServerManagerDiscoveryFailureSignature = failureSignature;
            sendCodexElvesDiagnostic("app_server_manager_discovery_not_found", {
              exportCount,
              candidateCount: candidates.length,
              retryAfterMs,
              reactScannedCount,
              reactManagerFound,
            });
          }
        }
      } catch (error) {
        codexAppServerManagerDiscoveryFailureCount += 1;
        const retryAfterMs = codexAppServerManagerDiscoveryBackoffMs();
        codexAppServerManagerDiscoveryNextAttemptAt = Date.now() + retryAfterMs;
        scheduleCodexAppServerManagerDiscoveryRetry(retryAfterMs);
        const errorName = error?.name || "";
        const errorMessage = error?.message || String(error);
        const failureSignature = `error:${errorName}:${errorMessage}`;
        if (codexAppServerManagerDiscoveryFailureSignature !== failureSignature) {
          codexAppServerManagerDiscoveryFailureSignature = failureSignature;
          sendCodexElvesDiagnostic("app_server_manager_discovery_failed", {
            errorName,
            errorMessage,
            retryAfterMs,
          });
        }
      }
    };
    codexAppServerManagerDiscoveryPromise = discovery().finally(() => {
      codexAppServerManagerDiscoveryPromise = null;
    });
    void codexAppServerManagerDiscoveryPromise;
    return codexAppServerManagerDiscoveryPromise;
  }

  function refreshCodexSessionPrewarmFeatureState(reason = "settings-changed") {
    const settings = codexSessionPrewarmSettingsSnapshot();
    if (!settings.enabled || settings.fullCount + settings.contentCount === 0) {
      clearTimeout(window.__codexSessionPrewarmTimer);
      window.__codexSessionPrewarmTimer = null;
      return Promise.resolve(false);
    }
    if (reason === "setting-sessionPrewarmEnabled") {
      window.__codexSessionPrewarmCompletedSignature = "";
    }
    if (codexSessionPrewarmManager) {
      scheduleCodexSessionPrewarm(codexSessionPrewarmStartupDelayMs, reason);
      return Promise.resolve(true);
    }
    if (codexSessionPrewarmFeatureRefreshPromise) {
      return codexSessionPrewarmFeatureRefreshPromise;
    }
    const refresh = Promise.resolve().then(async () => {
      if (codexAppServerManagerDiscoveryPromise) {
        await codexAppServerManagerDiscoveryPromise;
      }
      if (!codexSessionPrewarmManager) {
        resetCodexAppServerManagerDiscovery();
        await installAppServerManagerDiscovery(true, true);
      }
      if (!codexSessionPrewarmManager) return false;
      scheduleCodexSessionPrewarm(codexSessionPrewarmStartupDelayMs, reason);
      return true;
    }).finally(() => {
      if (codexSessionPrewarmFeatureRefreshPromise === refresh) {
        codexSessionPrewarmFeatureRefreshPromise = null;
      }
    });
    codexSessionPrewarmFeatureRefreshPromise = refresh;
    return refresh;
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

  function showToast(message, undoToken, undoRef) {
    document.querySelectorAll(".codex-delete-toast").forEach((node) => node.remove());
    const toast = document.createElement("div");
    toast.className = "codex-delete-toast";
    toast.textContent = message;
    if (undoToken) {
      const undo = document.createElement("button");
      undo.textContent = "撤销";
      undo.addEventListener("click", async () => {
        const result = await postJson("/undo", { undo_token: undoToken });
        if (result.status === "undone" && undoRef) restoreSessionToCodexAppStore(undoRef);
        toast.textContent = result.message || "撤销完成";
        setTimeout(() => toast.remove(), 5000);
      });
      toast.appendChild(undo);
    }
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
      showToast(`该分支已在另一个 worktree 使用：${usedWorktreePath}`, null);
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
      showToast(`将从 ${upstreamBranchOptionLabel(option) || "upstream/main"} 创建新 worktree`, null);
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
      showToast("无法安全识别 Codex 原生 worktree 表单，请使用 CodexElves 菜单创建。", null);
      return false;
    }
    event.preventDefault();
    event.stopPropagation();
    try {
      const result = await postJson("/upstream-worktree/create", payload);
      if (result?.status === "ok") {
        writeUpstreamBranchSelection(null);
        syncUpstreamBranchTriggerLabel();
        showToast(`已从 ${result.sourceRef} 创建 worktree`, null);
      } else {
        showToast(result?.message || "创建 upstream worktree 失败", null);
      }
    } catch (error) {
      showToast(error?.message || "创建 upstream worktree 失败", null);
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
        showToast(`已创建 upstream worktree：${result.branchName}`, null);
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

  function codexAppStoreManager() {
    const manager = codexSessionPrewarmManager || window.__codexElvesSessionPrewarmManager || null;
    return manager && typeof manager === "object" ? manager : null;
  }

  // 走 Codex App 原生归档：archiveConversation 会把会话真正归档（持久化到 app-server）
  // 并加入 App 的抑制集，从根本上避免折叠/展开项目重渲染时恢复已删除行。
  // 返回 { ok, reason }：ok 为 true 表示原生归档成功；失败时由调用方降级并提示。
  async function archiveSessionViaCodexApp(ref) {
    const threadId = validThreadSessionKey(ref?.session_id);
    if (!threadId) return { ok: false, reason: "invalid_session_id" };
    const manager = codexAppStoreManager();
    if (!manager) return { ok: false, reason: "manager_unavailable" };
    if (typeof manager.archiveConversation !== "function") {
      return { ok: false, reason: "archive_api_missing" };
    }
    const conversationId = projectMoveSessionKey(threadId);
    try {
      await manager.archiveConversation(conversationId, { cleanupWorktree: false });
      sendCodexElvesDiagnostic("session_delete_native_archived", { threadId, ok: true });
      return { ok: true, reason: "archived" };
    } catch (error) {
      appendCodexElvesFailure("__codexSessionDeleteArchiveFailures", error);
      sendCodexElvesDiagnostic("session_delete_native_archived", { threadId, ok: false, error: String(error?.message || error) });
      return { ok: false, reason: "archive_failed" };
    }
  }

  // 让 Codex App 走它自己的删除内存态清理：把会话加入抑制集、从 recentConversations
  // 缓存与 thread summary 中移除，并触发侧边栏重渲染。否则 App 内存态仍保留该会话，
  // 折叠/展开项目重渲染时会把已删除的行恢复出来。
  function evictSessionFromCodexAppStore(ref) {
    const threadId = validThreadSessionKey(ref?.session_id);
    if (!threadId) return false;
    const manager = codexAppStoreManager();
    if (!manager) return false;
    const ids = uniqueValues(threadIdVariants(threadId));
    let evicted = false;
    try {
      if (typeof manager.handleThreadDeletion === "function") {
        manager.handleThreadDeletion(ids);
        evicted = true;
      }
    } catch (error) {
      appendCodexElvesFailure("__codexSessionDeleteEvictFailures", error);
    }
    sendCodexElvesDiagnostic("session_delete_store_evicted", { threadId, evicted });
    return evicted;
  }

  // 撤销删除时抵消 evict 的副作用：把会话从 App 抑制集中移除并刷新
  // 最近会话列表，否则恢复本地记录后会话仍被 App 抑制集过滤、不显示。
  function restoreSessionToCodexAppStore(ref) {
    const threadId = validThreadSessionKey(ref?.session_id);
    if (!threadId) return false;
    const manager = codexAppStoreManager();
    if (!manager) return false;
    const ids = uniqueValues(threadIdVariants(threadId));
    let restored = false;
    try {
      if (typeof manager.handleThreadUnarchived === "function") {
        ids.forEach((id) => manager.handleThreadUnarchived(id));
        restored = true;
      }
      if (typeof manager.refreshRecentConversations === "function") {
        Promise.resolve(manager.refreshRecentConversations({ mode: "expanded" })).catch(() => {});
      }
    } catch (error) {
      appendCodexElvesFailure("__codexSessionDeleteRestoreFailures", error);
    }
    sendCodexElvesDiagnostic("session_delete_store_restored", { threadId, restored });
    return restored;
  }

  function removeDeletedRow(row, button, ref, archived = false) {
    releaseDeleteFocus(row, button);
    const shouldReload = isCurrentSessionRow(row, ref);
    // 原生归档成功时，App 已把会话加入抑制集并持久化归档，无需再走不可靠的
    // handleThreadDeletion 降级；仅归档失败时才回退到内存态抑制。
    if (!archived) {
      evictSessionFromCodexAppStore(ref);
    }
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
      // A1：先走 Codex 原生归档（把会话加入抑制集并持久化），再删数据。
      // 归档失败不中断删除（降级为内存态抑制），但在右下角明确提示，便于后续定位根因。
      const archiveResult = await archiveSessionViaCodexApp(ref);
      const result = await postJson("/delete", ref);
      if (result.status === "server_deleted" || result.status === "local_deleted") {
        removeDeletedRow(row, button, ref, archiveResult.ok);
        if (archiveResult.ok) {
          showToast(result.message || "删除成功", result.undo_token, ref);
        } else {
          showToast(`已删除，但原生归档未生效（${archiveResult.reason}），若会话重现请反馈`, result.undo_token, ref);
        }
      } else if (result.status === "not_found") {
        // 会话在本地存储中已不存在，目标（会话不存在）已达成，直接移除残留的列表行
        removeDeletedRow(row, button, ref, archiveResult.ok);
        showToast(result.message || "会话已不存在，已从列表移除", null);
      } else {
        showToast(result.message || "删除失败", null);
      }
    });
  }

  async function exportMarkdown(ref) {
    const result = await postJson("/export-markdown", ref);
    if (result.status === "exported" && result.filename && typeof result.markdown === "string") {
      const saveResult = await saveMarkdown(result.filename, result.markdown);
      if (saveResult?.status === "cancelled") {
        showToast(saveResult.message || "导出已取消", null);
      } else {
        showToast(result.message || "导出成功", null);
      }
      return;
    }
    showToast(result.message || "导出失败", null);
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
    showToast(message, null);
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
      showToast(`移动失败：${error?.message || error}`, null);
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
      showToast(`加载项目失败：${error?.message || error}`, null);
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
    const gap = 3;
    const left = Math.min(
      window.innerWidth - tooltipRect.width - 8,
      Math.max(8, buttonRect.left + buttonRect.width / 2 - tooltipRect.width / 2),
    );
    const aboveTop = buttonRect.top - tooltipRect.height - gap;
    const top = aboveTop >= 8
      ? aboveTop
      : Math.min(window.innerHeight - tooltipRect.height - 8, buttonRect.bottom + gap);
    tooltip.dataset.side = aboveTop >= 8 ? "top" : "bottom";
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
          showToast("导出失败：未找到归档会话 ID", null);
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

  function codexServiceTierVisibleComposerFooters(root = document) {
    const footers = [
      ...(root?.matches?.(".composer-footer") ? [root] : []),
      ...(root?.matches?.('[class*="_footer_"]') ? [root] : []),
      ...Array.from(root?.querySelectorAll?.('.composer-footer, [class*="_footer_"]') || []),
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
    if (!className.includes("_footer_")) return false;
    if (!className.includes("items-center")) return false;
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
    if (composer.matches?.(".composer-footer")) score += 4;
    if (composer.querySelector?.(".composer-footer")) score += 8;
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
    if (composer?.matches?.(".composer-footer")) return composer;
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
    const footer = parent?.closest?.('.composer-footer, [class*="_footer_"]');
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
      plugins: pluginAutoExpandPageLooksRelevant(),
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
    if (window.__codexElvesRuntimeOnceInstalled === codexElvesBuild) return;
    installStyle();
    cleanupLegacyCodexComposerOverflowGuards();
    void loadCodexModelCatalog();
    installCodexServiceTierDispatcherPatch();
    installCodexServiceTierRequestClientPatch();
    installAppServerManagerDiscovery();
    installCodexSessionPrewarmInteractionHooks();
    scheduleBackendHeartbeat();
    installDeleteButtonEventDelegation();
    installConversationViewRouteHooks();
    installCodexRouteFeatureRefreshEvents();
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
      schedulePluginAutoExpand();
    }
    if (sidebarDirty) {
      const pending = takePendingSessionRows();
      pending.rows.forEach(tryAttachButton);
      updateDeleteButtonOffsets(pending.rows);
      scheduleSessionRowLayout(pending.rows);
      syncCodexSessionPrewarmIndicators(pending.rows);
      scheduleProjectMoveProjection();
      scheduleChatsSortCorrection(chatsSortEventDelayMs, { refreshKeys: true });
      if (!chatsSortFallbackArmed) armChatsSortVisibleFallback();
    }
    if (sidebarDirty || conversationDirty) {
      archivedPageRows().forEach(attachArchivedPageDeleteButton);
    }
    if (conversationDirty) {
      refreshConversationView();
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
    return !!node?.closest?.(`.codex-delete-toast, .codex-delete-confirm-overlay, .codex-elves-modal-overlay, .${projectMoveOverlayClass}, .codex-conversation-timeline, .${codexServiceTierBadgeClass}, .${codexTokenUsageCardClass}, #codex-elves-menu`);
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
      ].join(", ");
    }
    if (domain === "header") {
      return [
        selectors.appHeader,
        selectors.pinnedSummaryPanel,
        selectors.pinnedSummaryToggle,
      ].join(", ");
    }
    if (domain === "conversation") {
      return [
        '[data-codex-archive-page-row="true"]',
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

  function pluginAutoExpandMutationRelevant(mutation) {
    const container = pluginAutoExpandContainer();
    if (!container) return false;
    const changedNodes = [
      ...Array.from(mutation.addedNodes || []),
      ...Array.from(mutation.removedNodes || []),
    ];
    const relevant = mutation.target === container
      || container.contains?.(mutation.target)
      || changedNodes.some((node) => node === container || container.contains?.(node));
    if (relevant) {
      window.__codexPluginAutoExpandCandidates = [];
      window.__codexPluginAutoExpandIdleUntil = 0;
    }
    return relevant;
  }

  function shouldScheduleScan(mutations, domain) {
    if (!mutations) return true;
    const pluginMutationRelevant = (
      (domain === "conversation" || domain === "shell")
      && codexElvesSettings().pluginAutoExpand
      && pluginAutoExpandPageLooksRelevant()
    ) && mutations.some((mutation) =>
        !isExtensionUiNode(mutation.target)
        && (mutation.type === "childList" || mutation.type === "attributes")
        && pluginAutoExpandMutationRelevant(mutation)
      );
    return pluginMutationRelevant || mutations.some((mutation) => {
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
    if (pluginAutoExpandPageLooksRelevant()) dirty.plugins = true;
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
    const headerRoot = document.querySelector(selectors.appHeader)?.closest?.("header, [role='banner']") ||
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
    cleanupLegacyForcePluginInstallRuntime();
    const launchCycleChanged = resetCodexSessionPrewarmForLaunchCycle();
    void loadBackendSettingsForStartup();
    void loadCodexServiceTierState();
    void loadCodexModelCatalog();
    scan();
    refreshCodexTokenUsageFeatureState();
    refreshCodexServiceTierFeatureState();
    refreshUpstreamBranchDropdownAdapter();
    syncChatsSortVisibilityListener();
    installScanObservers();
    resetCodexAppServerManagerDiscovery();
    void installAppServerManagerDiscovery(true, true);
    scheduleCodexSessionPrewarm(
      codexSessionPrewarmStartupDelayMs,
      launchCycleChanged ? "launch-cycle-refresh" : "runtime-refresh"
    );
  };
  window.__codexElvesRuntimeBuild = codexElvesBuild;
  window.__codexElvesRuntimeHelperBase = helperBase;
  window.__codexElvesRuntimeManagerDiscoveryVersion = codexAppServerManagerDiscoveryVersion;
})();
