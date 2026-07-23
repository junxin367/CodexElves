use base64::Engine;
use codex_elves_core::assets;
use codex_elves_core::bridge::{self, BRIDGE_BINDING_NAME};
use codex_elves_core::cdp::{
    CdpTarget, list_targets, pick_injectable_codex_page_target, pick_page_target,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

fn target(id: &str, kind: &str, title: &str, url: &str, websocket_url: Option<&str>) -> CdpTarget {
    CdpTarget {
        id: id.to_string(),
        target_type: kind.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        web_socket_debugger_url: websocket_url.map(str::to_string),
    }
}

#[test]
fn bridge_script_defines_expected_globals_and_binding() {
    let script = bridge::build_bridge_script(BRIDGE_BINDING_NAME);

    assert!(script.contains("window.__codexSessionDeleteBridge"));
    assert!(script.contains("window.__codexSessionDeleteResolve"));
    assert!(script.contains("window.__codexSessionDeleteReject"));
    assert!(script.contains("__codexSessionDeleteBridgeGeneration"));
    assert!(script.contains("path, payload, generation"));
    assert!(script.contains("codexSessionDeleteV2"));
}

#[test]
fn bridge_binding_generation_ignores_stale_message_pumps() {
    assert!(bridge::bridge_payload_matches_generation(
        &json!({"id": "1", "generation": "active"}),
        "active"
    ));
    assert!(!bridge::bridge_payload_matches_generation(
        &json!({"id": "1", "generation": "stale"}),
        "active"
    ));
    assert!(bridge::bridge_payload_matches_generation(
        &json!({"id": "1"}),
        "active"
    ));
}

#[test]
fn injection_script_prefixes_helper_url_and_version() {
    let script = assets::injection_script(45221);

    assert!(script.contains("window.__CODEX_SESSION_DELETE_HELPER__"));
    assert!(script.contains("http://127.0.0.1:45221"));
    assert!(script.contains("window.__CODEX_ELVES_VERSION__"));
    assert!(script.contains(codex_elves_core::version::VERSION));
    assert!(script.contains("window.__CODEX_ELVES_LAUNCH_CYCLE__"));
}

#[test]
fn bootstrap_injection_script_loads_features_without_inlining_full_runtime() {
    let script = assets::bootstrap_injection_script(45221);

    assert!(script.contains("/runtime/install-renderer-features"));
    assert!(script.contains("/inject/renderer-features.js"));
    assert!(script.contains("/inject/user-scripts.js"));
    assert!(script.contains("ready_fallback"));
    assert!(script.contains("ready_fallback_degraded"));
    assert!(script.contains("window.__CODEX_SESSION_DELETE_HELPER__"));
    assert!(script.contains("http://127.0.0.1:45221"));
    assert!(!script.contains("function installCodexElvesMenu"));
    assert!(assets::renderer_features_script().contains("function installCodexElvesMenu"));
}

#[test]
fn renderer_features_diagnostics_prefer_bridge_before_http_fallback() {
    let script = assets::renderer_features_script();

    assert!(script.contains("Promise.resolve(window.__codexSessionDeleteBridge"));
    assert!(script.contains(".catch(() => sendCodexElvesDiagnosticOverHttp(payload))"));
    assert!(script.contains("function sendCodexElvesDiagnosticOverHttp(payload)"));
}

#[test]
fn renderer_features_reuses_scan_observers_when_roots_are_unchanged() {
    let script = assets::renderer_features_script();

    assert!(script.contains("function installCodexElvesRuntimeOnce()"));
    assert!(script.contains("window.__codexElvesRuntimeOnceInstalled === codexElvesBuild"));
    assert!(!script.contains("function scanLightweight()"));
    assert!(script.contains("function sameScanObserverRoots"));
    assert!(script.contains("if (sameScanObserverRoots(roots)) return;"));
    assert!(script.contains("window.__codexSessionDeleteObserverConfigs"));
    assert!(
        script.contains(
            "const scopedRootsReady = !!sidebarRoot && !!conversationRoot && !!headerRoot;"
        )
    );
    assert!(script.contains("subtree: !scopedRootsReady"));
    assert!(script.contains("[sidebarRoot, conversationRoot, headerRoot].forEach((root) =>"));
    assert!(script.contains("push(\"shell\", root.parentElement"));
    assert!(script.contains("function scanRelevantSelectorForDomain(domain)"));
    assert!(script.contains("function shouldScheduleScan(mutations, domain)"));
    assert!(script.contains(
        "if (!shouldScheduleScan(mutations, domain)) return;\n    if (domain === \"sidebar\") collectPendingSessionRows(mutations)"
    ));
    assert!(script.contains(
        "if (dirty.shell) requestAnimationFrame(() => runScanStep(installScanObservers))"
    ));
    assert!(script.contains("if (headerDirty) installCodexElvesMenu()"));
    assert!(script.contains("if (shellDirty) cleanupDisconnectedSessionArtifacts()"));
}

#[test]
fn injection_script_batches_session_row_refresh_and_layout() {
    let script = assets::renderer_features_script();

    assert!(script.contains("const pendingSessionRows = new Set()"));
    assert!(script.contains("const pendingSessionRowLayouts = new Set()"));
    assert!(script.contains("function collectPendingSessionRows(mutations)"));
    assert!(script.contains("function takePendingSessionRows()"));
    assert!(script.contains("function resetPendingSessionRowsForFullRefresh()"));
    assert!(script.contains("scan(dirty, { sidebarIncremental: !dirty.shell })"));
    assert!(script.contains("pending.rows.forEach(tryAttachButton)"));
    assert!(!script.contains("sessionRows().forEach(tryAttachButton)"));
    assert!(script.contains("function measureActionGroupLayout(row, group)"));
    assert!(script.contains("function applyActionGroupLayout(measurement)"));
    assert!(script.contains("measurements.forEach(applyActionGroupLayout)"));
    assert!(script.contains("function scheduleSessionRowLayout(rows)"));
    assert!(script.contains("pendingSessionRowLayoutRafId = requestAnimationFrame"));
    assert!(script.contains("updateDeleteButtonOffsets(pending.rows)"));
}

#[test]
fn injection_script_exposes_image_overlay_config() {
    let temp = tempfile::tempdir().unwrap();
    let image_path = temp.path().join("overlay.png");
    std::fs::write(
        &image_path,
        base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=")
            .unwrap(),
    )
    .unwrap();
    let settings = codex_elves_core::settings::BackendSettings {
        codex_app_image_overlay_enabled: true,
        codex_app_image_overlay_path: image_path.to_string_lossy().to_string(),
        codex_app_image_overlay_opacity: 42,
        ..Default::default()
    };
    let script = assets::injection_script_with_settings(45221, &settings);

    assert!(script.contains("window.__CODEX_ELVES_IMAGE_OVERLAY__"));
    assert!(script.contains("\"enabled\":true"));
    assert!(script.contains("\"opacity\":0.42"));
    assert!(script.contains("\"dataUrl\":\"data:image/png;base64,"));
    assert!(script.contains("http://127.0.0.1:45221/overlay/image"));
}

#[test]
fn injection_script_installs_image_overlay_from_data_uri() {
    let script = assets::injection_script(45221);

    assert!(script.contains("const source = config.dataUrl || \"\""));
    assert!(script.contains("image.src = source"));
    assert!(script.contains("image_overlay_installed"));
}

#[test]
fn injection_script_marks_diagnostic_build_and_reports_script_loaded() {
    let script = assets::injection_script(45221);

    assert!(script.contains("window.__CODEX_ELVES_BUILD__"));
    assert!(script.contains(codex_elves_core::assets::DIAGNOSTIC_BUILD_ID));
    assert!(script.contains("script_loaded"));
    assert!(script.contains("data-codex-elves-build"));
}

#[test]
fn injection_script_times_out_backend_bridge_calls_and_falls_back_to_helper() {
    let script = assets::injection_script(45221);

    assert!(script.contains("bridgeWithBackendTimeout"));
    assert!(script.contains("backend_bridge_timeout"));
    assert!(script.contains("/backend/repair"));
    assert!(script.contains("backend_status_bridge_failed_http_fallback_ok"));
    assert!(script.contains("backend_status_bridge_and_http_failed"));
}

#[test]
fn injection_script_explains_plugin_patch_is_unneeded_in_relay_mode() {
    let script = assets::injection_script(45221);

    assert!(script.contains("兼容增强模式下无需开启"));
}

#[test]
fn injection_script_menu_exposes_plugin_entry_and_marketplace_switches() {
    let script = assets::injection_script(45221);

    assert!(script.contains("插件市场解锁"));
    assert!(script.contains("data-codex-elves-setting=\"pluginMarketplaceUnlock\""));
    assert!(script.contains("强制解锁入口"));
    assert!(script.contains("data-codex-elves-setting=\"pluginEntryUnlock\""));
    assert!(!script.contains("特殊插件强制安装"));
    assert!(!script.contains("data-codex-elves-setting=\"forcePluginInstall\""));
    assert!(!script.contains("forcePluginInstall"));
    assert!(script.contains("恢复 1.1.9 的入口解锁方式"));
}

#[test]
fn injection_script_exposes_plugin_list_auto_expand_switch() {
    let script = assets::injection_script(45221);

    assert!(script.contains("codexPluginAutoExpandVersion = \"1\""));
    assert!(script.contains("pluginAutoExpand: true"));
    assert!(script.contains("pluginAutoExpand: \"codexAppPluginAutoExpand\""));
    assert!(script.contains("function pluginAutoExpandPageActive()"));
    assert!(script.contains("pluginButton?.getAttribute(\"aria-current\") === \"page\""));
    assert!(script.contains("document.querySelector(\"main, [role='main']\")"));
    assert!(script.contains("function pluginAutoExpandButtonLooksLikeMore"));
    assert!(!script.contains(r#"/^(另有|还有)\s*\d+\s*个(?:插件)?$/i"#));
    assert!(script.contains("function schedulePluginAutoExpand"));
    assert!(script.contains("plugins: pluginAutoExpandPageLooksRelevant()"));
    assert!(script.contains("codexElvesSettings().pluginAutoExpand"));
    assert!(script.contains("&& pluginAutoExpandPageLooksRelevant()"));
    assert!(script.contains("if (pluginAutoExpandPageLooksRelevant()) dirty.plugins = true"));
    assert!(script.contains("plugin_auto_expand_finished"));
    assert!(script.contains("插件列表全量展示"));
    assert!(script.contains("data-codex-elves-setting=\"pluginAutoExpand\""));
}

#[test]
fn injection_script_skips_plugin_patch_work_in_relay_mode() {
    let script = assets::injection_script(45221);

    assert!(script.contains("function pluginPatchDisabledInRelayMode()"));
    assert!(script.contains("!codexElvesBackendSettingsLoaded"));
    assert!(script.contains("if (pluginPatchDisabledInRelayMode()) return"));
    assert!(script.contains("clearPluginPatchArtifacts()"));
}

#[test]
fn injection_script_disables_plugin_auto_expand_in_relay_mode() {
    let script = assets::injection_script(45221);

    assert!(script.contains("settings.pluginAutoExpand = false"));
    assert!(script.contains("if (pluginPatchDisabledInRelayMode()) return"));
    assert!(script.contains("if (!codexElvesSettings().pluginAutoExpand) return"));
}

#[test]
fn injection_script_defines_version_gated_plugin_unlock_strategy() {
    let script = assets::injection_script(45221);

    assert!(script.contains("codexPluginLegacyEntryUnlockBeforeVersion = \"26.601.2237\""));
    assert!(script.contains("function parseCodexVersionParts(version)"));
    assert!(script.contains("function compareCodexVersions(left, right)"));
    assert!(script.contains("function codexPluginUnlockStrategy()"));
    assert!(script.contains("const comparison = compareCodexVersions(version, codexPluginLegacyEntryUnlockBeforeVersion)"));
    assert!(script.contains("return comparison < 0 ? \"legacy\" : \"modern\""));
}

#[test]
fn injection_script_gates_legacy_and_modern_plugin_unlock_by_codex_version() {
    let script = assets::injection_script(45221);

    assert!(script.contains("const pluginUnlockStrategy = codexPluginUnlockStrategy()"));
    assert!(script.contains("if ((pluginUnlockStrategy === \"legacy\" || pluginUnlockStrategy === \"unknown\") && settings.pluginEntryUnlock)"));
    assert!(script.contains("if ((pluginUnlockStrategy === \"modern\" || pluginUnlockStrategy === \"unknown\") && settings.pluginMarketplaceUnlock)"));
    assert!(script.contains("plugin_unlock_strategy_selected"));
    assert!(script.contains("window.__codexPluginUnlockStrategyLogged"));
}

#[test]
fn injection_script_restores_legacy_plugin_sidebar_entry_unlock() {
    let script = assets::injection_script(45221);

    assert!(script.contains("pluginEntryUnlock: true"));
    assert!(script.contains("pluginEntryUnlock: \"codexAppPluginEntryUnlock\""));
    assert!(script.contains("function reactFiberFrom(element)"));
    assert!(script.contains("function authContextValueFrom(element)"));
    assert!(script.contains("function spoofChatGPTAuthMethod(element)"));
    assert!(script.contains("auth.setAuthMethod(\"chatgpt\")"));
    assert!(script.contains("function pluginEntryButton()"));
    assert!(script.contains("function enablePluginEntry()"));
    assert!(script.contains("if (!codexElvesSettings().pluginEntryUnlock) return"));
    assert!(script.contains("pluginButton.addEventListener(\"click\", () => {"));
    assert!(script.contains("spoofChatGPTAuthMethod(pluginButton);"));
    assert!(script.contains("插件 - 已解锁"));
    assert!(script.contains("Plugins - Unlocked"));
}

#[test]
fn injection_script_keeps_plugin_marketplace_unlock_separate_from_entry_unlock() {
    let script = assets::injection_script(45221);

    assert!(script.contains("pluginMarketplaceUnlock: true"));
    assert!(script.contains("pluginMarketplaceUnlock: \"codexAppPluginMarketplaceUnlock\""));
    assert!(script.contains("if (!codexElvesSettings().pluginMarketplaceUnlock) return"));
    assert!(script.contains("installPluginMarketplaceRequestPatch"));
}

#[test]
fn injection_script_does_not_unlock_disabled_plugin_install_buttons() {
    let script = assets::injection_script(45221);

    assert!(!script.contains("installButtonUnlockNodes"));
    assert!(!script.contains("patchReactDisabledProps"));
    assert!(!script.contains("props[\"data-disabled\"] = undefined"));
    assert!(!script.contains("button.querySelectorAll?.(\"button, [role='button'], [disabled], [aria-disabled], [data-disabled]"));
    assert!(!script.contains("button.dataset.codexForceInstallUnlocked"));
}

#[test]
fn injection_script_preserves_official_marketplace_literal_names() {
    let script = assets::injection_script(45221);

    assert!(script.contains("codexPluginMarketplaceUnlockVersion = \"19\""));
    // 不再重命名官方 marketplace，保留字面名以恢复原生浏览器 / 电脑操控面板。
    assert!(!script.contains("codexPluginMarketplaceAliasForName"));
    assert!(!script.contains("marketplace.name = alias"));
    assert!(!script.contains("OpenAI插件1(CodexElves)"));
}

#[test]
fn injection_script_does_not_bypass_plugin_marketplace_search_filters() {
    let script = assets::injection_script(45221);

    assert!(script.contains("codexPluginMarketplaceUnlockVersion = \"19\""));
    assert!(!script.contains("Array.prototype.filter = patchedFilter"));
    assert!(!script.contains("Object.defineProperty(items, \"filter\""));
}

#[test]
fn injection_script_expands_api_key_plugin_marketplace_requests() {
    let script = assets::injection_script(45221);
    let cases = run_service_tier_contract_harness();

    assert!(script.contains("codexPluginMarketplaceUnlockVersion = \"19\""));
    assert!(script.contains("installPluginMarketplaceRequestPatch"));
    assert!(script.contains("installPluginMarketplaceBridgePatch"));
    assert!(script.contains("return \"client\";"));
    assert!(script.contains("manager = findCodexSessionPrewarmManagerInReactTree(true).manager"));
    assert!(script.contains("patchPluginMarketplaceRequestClient(manager?.requestClient)"));
    assert!(script.contains("plugin_marketplace_bridge_patch_not_writable"));
    assert!(script.contains("plugin_marketplace_request_skipped_unsupported_auth"));
    assert!(script.contains("return emptyPluginMarketplaceResult();"));
    assert!(!script.contains("Array.prototype.filter = patchedFilter"));
    assert!(!script.contains("installPluginBuildFlavorFilterPatch"));
    assert!(!script.contains("codexPluginMarketplaceAliasForName"));
    assert!(!script.contains("marketplace.name = alias"));
    assert!(script.contains("method === \"list-plugins\""));
    assert!(script.contains("method === \"vscode://codex/list-plugins\""));
    assert!(script.contains("message.type === \"fetch\""));
    assert!(script.contains("data?.type === \"fetch-response\""));
    assert!(script.contains("__codexPluginMarketplaceFetchRequestIds"));
    assert!(script.contains("if (hadMarketplaceKinds && Array.isArray(next.marketplaceKinds))"));
    assert!(script.contains("codexPluginApiKeyUnsupportedMarketplaceKinds.has(kind)"));
    assert!(script.contains(
        "if (unsupportedMarketplaceKinds.length === 0 && !nextKinds.includes(\"vertical\"))"
    ));
    assert!(script.contains("next.marketplaceKinds = Array.from(new Set(nextKinds));"));
    assert!(script.contains("patchPluginMarketplaceResult"));
    assert!(script.contains("__CODEX_ELVES_PLUGIN_MARKETPLACES__"));
    assert!(script.contains("mergeLocalPluginMarketplaces(result)"));
    assert!(script.contains("plugin_marketplace_local_merged"));
    assert!(script.contains("cloned.marketplaceName = marketplaceName"));
    assert!(script.contains("cloned.marketplacePath = `remote:${marketplaceName}`"));
    assert!(script.contains("restorePluginMarketplaceName"));
    assert!(script.contains(
        "next.remoteMarketplaceName = restorePluginMarketplaceName(next.remoteMarketplaceName)"
    ));
    assert!(!script.contains("OpenAI插件1(CodexElves)"));
    assert!(script.contains("method === \"install-plugin\""));
    assert!(script.contains("plugin_install_request_debug"));
    assert!(script.contains("plugin_install_request_failed"));
    assert!(!script.contains("marketplace.path ="));
    assert!(!script.contains("codexPluginMarketplacePathAliasForName"));
    assert!(!script.contains("spoofAnyCodexAuthContext"));
    assert_eq!(
        cases["pluginScopedFilters"]["pluginCount"],
        cases["pluginScopedFilters"]["pluginTotal"]
    );
    // 保留字面 openai-bundled 后，Codex 原生“隐藏 marketplace”过滤会把 bundled
    // 从插件市场列表隐藏（原生默认行为）；bundled 插件由原生面板承载。
    let marketplace_count = cases["pluginScopedFilters"]["marketplaceCount"]
        .as_i64()
        .unwrap();
    let marketplace_total = cases["pluginScopedFilters"]["marketplaceTotal"]
        .as_i64()
        .unwrap();
    assert_eq!(marketplace_count + 1, marketplace_total);
    assert_eq!(
        cases["pluginScopedFilters"]["officialMarketplaceName"],
        "openai-bundled"
    );
    assert_eq!(
        cases["pluginScopedFilters"]["curatedRemoteMarketplaceName"],
        "openai-curated-remote"
    );
    assert_eq!(cases["pluginScopedFilters"]["catalogReady"], true);
    assert_eq!(cases["pluginScopedFilters"]["pluginFilterIsOwn"], false);
    assert_eq!(
        cases["pluginScopedFilters"]["marketplaceFilterIsOwn"],
        false
    );
    assert_eq!(
        cases["pluginScopedFilters"]["ordinaryFilter"],
        json!([2, 3])
    );
    assert_eq!(
        cases["pluginMarketplaceRequestParams"]["personal"]["marketplaceKinds"],
        json!(["created-by-me-remote"])
    );
    assert_eq!(
        cases["pluginMarketplaceRequestParams"]["mixed"]["marketplaceKinds"],
        json!(["created-by-me-remote", "workspace"])
    );
    assert_eq!(
        cases["pluginMarketplaceRequestParams"]["original"]["marketplaceKinds"],
        json!(["created-by-me-remote"])
    );
    assert_eq!(
        cases["pluginMarketplaceRequestClient"]["calls"][0]["method"],
        "plugin/list"
    );
    assert_eq!(
        cases["pluginMarketplaceRequestClient"]["calls"][0]["params"]["marketplaceKinds"],
        json!(["workspace", "vertical"])
    );
    assert_eq!(
        cases["pluginMarketplaceRequestClient"]["unsupportedCount"],
        0
    );
    assert_eq!(
        cases["pluginMarketplaceRequestClient"]["calls"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn injection_script_skips_api_key_incompatible_marketplace_queries_and_expands_supported_catalogs()
{
    let script = assets::injection_script(45221);

    assert!(script.contains("const hadMarketplaceKinds = Object.prototype.hasOwnProperty.call(next, \"marketplaceKinds\")"));
    assert!(script.contains("if (hadMarketplaceKinds && Array.isArray(next.marketplaceKinds))"));
    assert!(script.contains(".map((kind) => restorePluginMarketplaceName(kind))"));
    assert!(script.contains(
        "const codexPluginApiKeyUnsupportedMarketplaceKinds = new Set([\"created-by-me-remote\"]);"
    ));
    assert!(script.contains("codexPluginApiKeyUnsupportedMarketplaceKinds.has(kind)"));
    assert!(script.contains("unsupportedMarketplaceKinds.push(kind)"));
    assert!(script.contains("function unsupportedPluginMarketplaceKinds(method, params)"));
    assert!(script.contains("function emptyPluginMarketplaceResult()"));
    assert!(script.contains(
        "if (unsupportedMarketplaceKinds.length === 0 && !nextKinds.includes(\"vertical\"))"
    ));
    assert!(script.contains("next.marketplaceKinds = Array.from(new Set(nextKinds));"));
    assert!(script.contains("plugin_marketplace_request_expanded"));
    assert!(script.contains(
        "marketplaceKinds: Array.isArray(next.marketplaceKinds) ? next.marketplaceKinds : null"
    ));
    assert!(script.contains("unsupportedMarketplaceKinds"));
    assert!(!script.contains("delete next.marketplaceKinds"));
    assert!(!script.contains("codexPluginAllowedMarketplaceKinds"));
    assert!(!script.contains("codexPluginExpandedMarketplaceKinds"));
}

#[test]
fn injection_script_logs_marketplace_grouping_diagnostics() {
    let script = assets::injection_script(45221);

    assert!(script.contains("plugin_marketplace_response_debug"));
    assert!(script.contains("marketplaces: result.marketplaces.map"));
    assert!(script.contains("pluginMarketplaceCounts"));
    assert!(script.contains("remoteMarketplaceName"));
}

#[test]
fn injection_script_omits_force_install_unlock_loop() {
    let script = assets::injection_script(45221);

    assert!(!script.contains("codex-force-install-unlocked"));
    assert!(!script.contains("codexForcePluginInstallSettleWindowMs"));
    assert!(!script.contains("refreshForcePluginInstallUnlockLoop"));
    assert!(script.contains("cleanupLegacyForcePluginInstallRuntime"));
    assert!(script.contains("__codexForcePluginInstallObserver?.disconnect?.()"));
    assert!(!script.contains("__codexForcePluginInstallObserver = new MutationObserver"));
    assert!(!script.contains("codexForcePluginInstallRefreshIntervalMs"));
}

#[test]
fn injection_script_loads_backend_settings_before_initial_scan() {
    let script = assets::injection_script(45221);
    let startup_call = script
        .find("void loadBackendSettingsForStartup();")
        .expect("script should load backend settings on startup");
    let footer = &script[startup_call..];
    let initial_scan = footer
        .find("scan();")
        .expect("script should perform an initial scan");
    let footer_marker = footer
        .find("window.__codexProjectMoveApplyProjection")
        .expect("script should continue bootstrapping after the initial scan");

    assert!(initial_scan < footer_marker);
    assert!(script.contains("if (attempt < 60)"));
}

#[test]
fn injection_script_exposes_conversation_view_width_control() {
    let script = assets::injection_script(45221);

    assert!(script.contains("conversationView: false"));
    assert!(script.contains("conversationView"));
    assert!(script.contains("conversationViewMaxWidth"));
    assert!(script.contains("对话居中宽度"));
    assert!(script.contains("data-codex-elves-conversation-view-width"));
    assert!(script.contains("conversationViewWidth()"));
    assert!(script.contains("normalizeConversationViewWidth"));
    assert!(script.contains("installConversationViewRouteHooks"));
    assert!(script.contains("scheduleConversationViewRouteRefresh"));
    assert!(script.contains("scheduleCodexRouteFeatureRefresh"));
    assert!(script.contains("installCodexRouteFeatureRefreshEvents"));
}

#[test]
fn injection_script_exposes_compact_per_thread_token_usage_summary() {
    let script = assets::injection_script(45221);

    assert!(script.contains("tokenUsage: false"));
    assert!(script.contains("tokenUsage: \"codexAppTokenUsage\""));
    assert!(script.contains("data-codex-elves-setting=\"tokenUsage\""));
    assert!(script.contains("会话 Token 统计"));
    assert!(script.contains("[data-pip-obstacle=\"thread-summary-panel\"]"));
    assert!(script.contains("aria-pressed"));
    assert!(script.contains("/thread-usage-history"));
    assert!(script.contains("turnId === latestTurnId"));
    assert!(script.contains("最近一轮"));
    assert!(script.contains("formatCodexTokenCount"));
    assert!(script.contains("formatCodexTurnDuration"));
    assert!(script.contains("lastTurnStartedAt"));
    assert!(script.contains("lastTurnCompletedAt"));
    assert!(script.contains("最近一轮执行时长"));
    assert!(script.contains("data-codex-token-usage-duration"));
    assert!(script.contains("syncCodexTokenUsageDurationTicker"));
    assert!(script.contains("codexTokenUsageDurationTickIntervalMs = 1000"));
    assert!(script.contains("setInterval("));
    assert!(script.contains("const thousand = 1000"));
    assert!(script.contains("let unit = \"K\""));
    assert!(script.contains("codex-token-usage-metrics"));
    assert!(script.contains("codex-token-usage-agent-count"));
    assert!(script.contains("codex-token-usage-host"));
    assert!(script.contains("flex-direction: column !important"));
    assert!(script.contains("--codex-token-usage-panel-end-gap"));
    assert!(script.contains("var(--color-token-dropdown-background"));
    assert!(script.contains("pointer-events: none"));
    assert!(script.contains("子智能体 ${descendantCount}"));
    assert!(script.contains("card.removeAttribute(\"title\")"));
    assert!(script.contains("window.__codexTokenUsageSummaryCache instanceof Map"));
    assert!(script.contains("cacheCodexTokenUsageSummary"));
    assert!(script.contains("renderCachedCodexTokenUsage"));
    assert!(script.contains("emptyCodexTokenUsageSummary"));
    assert!(script.contains("renderCodexTokenUsagePlaceholder"));
    assert!(script.contains("card.dataset.status = \"placeholder\""));
    assert!(script.contains("panel.insertAdjacentElement(\"afterend\", card)"));
    assert!(script.contains("codexTokenUsageRefreshIntervalMs = 2500"));
    assert!(script.contains("codexTokenUsageSettleDelayMs = 500"));
    assert!(script.contains("codexTokenUsageCompletionSettleDelayMs = 2500"));
    assert!(script.contains("codexTokenUsageRetryDelaysMs = [1000, 2500, 5000]"));
    assert!(!script.contains("codexTokenUsageHiddenRefreshIntervalMs"));
    assert!(script.contains("function scheduleCodexTokenUsageRefresh(delayMs = 0)"));
    assert!(script.contains("\"thread/tokenUsage/updated\""));
    assert!(script.contains("method === \"turn/started\""));
    assert!(script.contains("method === \"turn/completed\""));
    assert!(script.contains("addNotificationCallback"));
    assert!(script.contains("installCodexTokenUsageVisibilityListener"));
    assert!(script.contains("window.__codexTokenUsageRefreshPending = true"));
    assert!(script.contains("summary.isRunning && document.visibilityState !== \"hidden\""));
    assert!(!script.contains("执行中 · 已结算至最近模型响应"));
    assert!(script.contains("descendantCount"));
    assert!(script.contains("window.__codexTokenUsageRequestSeq"));
    assert!(script.contains("refreshCodexTokenUsageCard"));
    assert!(script.contains("function syncCodexTokenUsageWithPinnedSummaryState()"));
    assert!(script.contains("function installCodexTokenUsagePinnedSummaryObserver()"));
    assert!(script.contains("function installCodexTokenUsagePinnedSummaryLifecycleObserver()"));
    assert!(script.contains("window.__codexTokenUsagePinnedSummaryObserverTarget?.isConnected"));
    assert!(script.contains("document.getElementById(\"root\") || document.body"));
    assert!(script.contains("attributeFilter: [\"aria-pressed\"]"));
    assert!(script.contains("function hideCodexTokenUsageCards()"));
    assert!(script.contains("function pauseCodexTokenUsageForHiddenPinnedSummary()"));
    assert!(script.contains("pauseCodexTokenUsageForHiddenPinnedSummary();"));
    assert!(!script.contains("syncCodexTokenUsageWithPinnedSummaryToggle"));
    assert!(!script.contains("scheduleCodexTokenUsageRefresh(120)"));
}

#[test]
fn injection_script_removes_timeline_and_sidebar_thread_id_badge_controls() {
    let script = assets::injection_script(45221);

    assert!(!script.contains("data-codex-elves-setting=\"threadIdBadge\""));
    assert!(!script.contains("data-codex-elves-setting=\"conversationTimeline\""));
    assert!(!script.contains("会话 ID 标识"));
    assert!(!script.contains("对话 Timeline"));
    assert!(!script.contains("function refreshThreadIdBadges()"));
    assert!(!script.contains("function refreshConversationTimeline()"));
    assert!(script.contains("cleanupRemovedConversationHelpers"));
    assert!(script.contains("codex-conversation-timeline"));
    assert!(script.contains("codex-thread-id-badge"));
}

#[test]
fn injection_script_reuses_native_session_action_button_style_with_fallback() {
    let script = assets::injection_script(45221);

    assert!(script.contains("actionButtonClass = \"codex-session-action-button\""));
    assert!(script.contains("nativeActionButtonClassFromHost"));
    assert!(script.contains("sessionActionButtonClassName"));
    assert!(script.contains(
        ".${actionGroupClass}:not([data-codex-action-placement=\"native\"]) .${actionButtonClass}"
    ));
    assert!(script.contains("background: transparent;"));
    assert!(script.contains("background: #363839;"));
    assert!(script.contains("cursor: default;"));
    assert!(script.contains(
        "bg-token-dropdown-background text-token-foreground border-token-border rounded-lg border px-2 py-1"
    ));
    assert!(script.contains("tooltip.setAttribute(\"role\", \"tooltip\")"));
    assert!(script.contains("content.className = \"flex items-center gap-2\""));
    assert!(script.contains("text.className = \"min-w-0\""));
    assert!(script.contains("const gap = 3;"));
    assert!(script.contains("const aboveTop = buttonRect.top - tooltipRect.height - gap;"));
    assert!(script.contains("tooltip.dataset.side = aboveTop >= 8 ? \"top\" : \"bottom\""));
}

#[test]
fn injection_script_moves_export_and_project_move_into_more_menu() {
    let script = assets::injection_script(45221).replace("\r\n", "\n");

    assert!(script.contains("moreButtonClass = \"codex-session-more-button\""));
    assert!(script.contains("moreMenuClass = \"codex-session-more-menu\""));
    assert!(script.contains("configureActionButton(moreButton, \"更多操作\", \"…\")"));
    assert!(script.contains("createSessionMoreMenuItem(\"导出\""));
    assert!(script.contains("createSessionMoreMenuItem(\"移动\""));
    assert!(script.contains("group.appendChild(moreButton)"));
    assert!(script.contains("installMoreButtonEvents(row, moreButton, openMoreMenu)"));
    assert!(script.contains("installSessionMoreMenuAutoClose(row, moreMenu)"));
    assert!(script.contains("updateSessionMoreMenuDirection(moreButton, moreMenu)"));
    assert!(script.contains("positionSessionMoreMenu(moreButton, moreMenu)"));
    assert!(script.contains("document.body.appendChild(moreMenu)"));
    assert!(script.contains("position: fixed;"));
    assert!(script.contains("codex-session-more-menu-open-up"));
    assert!(script.contains("transform: translateY(calc(-100% - 34px));"));
    assert!(script.contains("positionSessionMoreMenu(moreButton, moreMenu);"));
    assert!(script.contains("row.classList.toggle(\"codex-session-more-open\""));
    assert!(script.contains(".${actionGroupClass} {"));
    assert!(script.contains("position: absolute;"));
    assert!(script.contains("pointer-events: none;"));
    assert!(
        script
            .contains("node.matches?.('div.contents[data-hover-card-open-immediately=\"true\"]')")
    );
    assert!(script.contains("function nativeActionHostFromRow(row)"));
    assert!(script.contains("group.dataset.codexActionPlacement = expectedPlacement"));
    assert!(script.contains("nativeActionHost.dataset.codexSessionActionHost = \"true\""));
    assert!(script.contains("nativeActionHost.prepend(group)"));
    assert!(script.contains("row.appendChild(group)"));
    assert!(script.contains("width: auto !important;"));
    assert!(script.contains("maxTitleWidth: titleRect && hostRect.width > 0"));
    assert!(script.contains("Math.max(24, Math.floor(hostRect.left - titleRect.left))"));
    assert!(script.contains("max-width: var(--codex-session-title-max-width) !important;"));
    assert!(script.contains("[data-codex-delete-row=\"true\"]:focus-within [data-thread-title]"));
    assert!(script.contains("[data-codex-delete-row=\"true\"]:hover .${actionGroupClass} {\n        opacity: 1;\n        pointer-events: auto;\n      }"));
    assert!(script.contains("[data-codex-delete-row=\"true\"].codex-session-more-open .${actionGroupClass} {\n        opacity: 1;\n        pointer-events: auto;\n        z-index: 2147483201;"));
    assert!(!script.contains("installActionButtonEvents(row, moreButton, openMoreMenu)"));
    assert!(!script.contains("group.appendChild(exportButton)"));
    assert!(!script.contains("group.appendChild(moveButton)"));
}

#[test]
fn injection_script_does_not_add_delete_controls_on_archived_page() {
    let script = assets::injection_script(45221);

    assert!(script.contains("attachArchivedPageDeleteButton"));
    assert!(script.contains("data-codex-archive-row-action"));
    assert!(script.contains("dataset.codexArchiveRowAction = \"export\""));
    assert!(!script.contains("dataset.codexArchiveRowAction = \"delete\""));
    assert!(!script.contains("installArchivedDeleteAllButton"));
    assert!(!script.contains("删除全部归档"));
}

#[test]
fn injection_script_uses_codex_native_model_catalog_without_model_list_patching() {
    let script = assets::injection_script(45221);

    assert!(script.contains("/codex-model-catalog"));
    assert!(script.contains("codexModelCatalog"));
    assert!(script.contains("codexElvesModelNames"));
    assert!(script.contains("installAppServerManagerDiscovery"));
    assert!(script.contains("installStatsigModelVisibilityPatch"));
    assert!(script.contains("use_hidden_models: false"));
    assert!(script.contains("appServerRequestMethod"));
    assert!(!script.contains("patchModelArray"));
    assert!(!script.contains("patchModelContainer"));
    assert!(!script.contains("patchAppServerModelResult"));
    assert!(!script.contains("patchAppServerModelRequestClient"));
    assert!(!script.contains("patchStatsigModelDynamicConfig"));
    assert!(!script.contains("installStatsigModelConfigPatch"));
    assert!(!script.contains("available_models: availableModels"));
    assert!(!script.contains("ensureCodexModelIntegration"));
    assert!(!script.contains("model/list"));
    assert!(!script.contains("list-models-for-host"));
    assert!(script.contains(r#"queryKey: ["models", "list"]"#));
    assert!(!script.contains("model_unlock_path_applied"));
    assert!(!script.contains("Response.prototype.json"));
    assert!(!script.contains("patchObjectGraphForModels"));
    assert!(!script.contains("patchReactModelState"));
    assert!(!script.contains("shouldScheduleReactModelStatePatch"));
    assert!(!script.contains("scheduleCodexModelWhitelistRefresh"));
    assert!(!script.contains("model_whitelist_refresh_scheduled"));
    assert!(!script.contains("model_statsig_wait_started"));
    assert!(!script.contains("modelWhitelistUnlock"));
    assert!(!script.contains("codexAppModelWhitelistUnlock"));
    assert!(!script.contains("模型白名单解锁"));
    assert!(!script.contains("querySelectorAll(\"button, [role='menu']"));
}

#[test]
fn injection_script_exposes_fast_service_tier_control() {
    let script = assets::injection_script(45221).replace("\r\n", "\n");

    assert!(script.contains("default-service-tier"));
    assert!(script.contains("setting-storage-"));
    assert!(script.contains("vscode-api-"));
    assert!(script.contains("thread-context-inputs-"));
    assert!(script.contains("findCodexServiceTierDispatcher"));
    assert!(script.contains("codexServiceTierDispatcherFromModule"));
    assert!(script.contains("codexServiceTierRequestClientClassFromModule"));
    assert!(script.contains("patchCodexServiceTierRequestClientPrototype"));
    assert!(script.contains("update-thread-settings-for-next-turn"));
    assert!(script.contains("service_tier_native_thread_setting_synced"));
    assert!(script.contains("service_tier_request_client_patch_installed"));
    assert!(script.contains("installCodexServiceTierRequestClientPatch"));
    assert!(script.contains("__codexServiceTierRequestClientPatchPromise"));
    assert!(script.contains("__codexServiceTierRequestClientPatchNextAttemptAt"));
    assert!(script.contains("codexServiceTierRequestClientPatchRetryMaxMs"));
    assert!(script.contains("codexAppAssetUrl"));
    assert!(script.contains("codexThreadServiceTierOverrides"));
    assert!(script.contains("setCodexThreadServiceTierMode"));
    assert!(script.contains("codexServiceTierRequestOverride"));
    assert!(script.contains("codexServiceTierSupportedFastModels"));
    assert!(script.contains("codexServiceTierSupportedFastModelPrefixes"));
    assert!(script.contains("\"gpt-5.4\""));
    assert!(script.contains("\"gpt-5.5\""));
    assert!(script.contains("\"gpt-5.6\""));
    assert!(script.contains("\"gpt-5.6-sol\""));
    assert!(script.contains("\"gpt-5.6-terra\""));
    assert!(script.contains("\"gpt-5.6-luna\""));
    assert!(script.contains("codexServiceTierBuiltInFastSupported"));
    assert!(script.contains("codexServiceTierFastSupportedForModel"));
    assert!(script.contains("codexServiceTierModelForRequest"));
    assert!(script.contains("codexServiceTierMaybeLoadModelCatalog"));
    assert!(script.contains("fastBlocked"));
    assert!(script.contains("data-tier=\"unsupported\""));
    assert!(script.contains("nextParams.service_tier = override.serviceTier"));
    assert!(script.contains("serviceTierControls: false"));
    assert!(script.contains("data-codex-elves-setting=\"serviceTierControls\""));
    assert!(script.contains("data-codex-service-tier-controls"));
    assert!(script.contains("[data-codex-tooltip]::before"));
    assert!(script.contains("[data-codex-tooltip]::after"));
    assert!(script.contains("display: none;\n        position: absolute;"));
    assert!(script.contains("display: block;\n        opacity: 1;"));
    assert!(script.contains("removeCodexServiceTierBadges"));
    assert!(script.contains("installCodexServiceTierDispatcherPatch"));
    assert!(script.contains("服务模式"));
    assert!(script.contains("data-codex-service-tier-status"));
    assert!(script.contains("data-codex-service-tier-inherit"));
    assert!(script.contains("data-codex-service-tier-standard"));
    assert!(script.contains("data-codex-service-tier-fast"));
    assert!(script.contains("data-codex-service-tier-custom"));
    assert!(script.contains("data-codex-service-tier-thread-inherit"));
    assert!(script.contains("data-codex-service-tier-thread-standard"));
    assert!(script.contains("data-codex-service-tier-thread-fast"));
    assert!(script.contains("global-standard"));
    assert!(script.contains("global-fast"));
    assert!(script.contains("defaultMode"));
    assert!(script.contains("codexServiceTierEffectiveThreadMode"));
    assert!(script.contains("codexServiceTierDefaultModeForControlMode"));
    assert!(script.contains("normalizeCodexServiceTierControlMode(state.mode) !== \"custom\""));
    assert!(script.contains("state.draft = null"));
    assert!(script.contains("后端未连接，无法切换服务模式"));
    assert!(script.contains("未连接"));
    assert!(script.contains("thread/start"));
    assert!(script.contains("thread/resume"));
    assert!(script.contains("turn/start"));
    assert!(script.contains("send-cli-request-for-host"));
    assert!(script.contains("start-conversation"));
    assert!(script.contains("applyCodexServiceTierRequestOverride(\"thread/start\", message)"));
    assert!(script.contains("codex-service-tier-badge"));
    assert!(script.contains("installCodexServiceTierBadge"));
    assert!(script.contains("toggleCodexServiceTierFromBadge"));
    assert!(script.contains("wireCodexServiceTierBadge"));
    assert!(script.contains("codexServiceTierBadgePlacement"));
    assert!(script.contains("codexServiceTierNativeServiceTierSlot"));
    assert!(script.contains("[class*=\"_footer_\"]"));
    assert!(script.contains("codexServiceTierBadgeFooterGroup"));
    assert!(script.contains("codexServiceTierFindComposerEl"));
    assert!(script.contains("codexServiceTierVisibleComposerFooters"));
    assert!(script.contains("codexServiceTierBestComposerFooter"));
    assert!(script.contains("codexServiceTierComposerCandidates"));
    assert!(script.contains("codexServiceTierComposerScore"));
    assert!(script.contains("codexServiceTierSelectedModelTexts"));
    assert!(script.contains("data-codex-intelligence-trigger"));
    assert!(script.contains("data-composer-navigation-target=\"reasoning\""));
    assert!(script.contains("!node.closest?.('[aria-hidden=\"true\"]')"));
    assert!(script.contains("data-codex-service-tier-badge"));
    assert!(script.contains("codexServiceTierBadgeWired"));
    assert!(script.contains("setAttribute(\"role\", \"button\")"));
    assert!(script.contains("setAttribute(\"tabindex\", \"0\")"));
    assert!(script.contains("继承 config.toml"));
    assert!(script.contains("service_tier=\\\"priority\\\""));
    assert!(script.contains("Fast 仅支持"));
    assert!(script.contains("当前 thread"));
    assert!(script.contains("standard"));
    assert!(script.contains("fast"));
}

#[test]
fn injection_script_constrains_native_composer_measurement_without_clipping_surface() {
    let script = assets::injection_script(45221);

    assert!(script.contains("codex-elves-service-tier-composer-surface"));
    assert!(script.contains(".composer-surface-chrome {"));
    assert!(script.contains("scrollbar-width: none !important;"));
    assert!(script.contains(".composer-surface-chrome::-webkit-scrollbar"));
    assert!(script.contains("[class*=\"_WorkTriggerMeasurement_\"][aria-hidden=\"true\"]"));
    assert!(script.contains("[class*=\"_ModelPickerTriggerMeasurement_\"][aria-hidden=\"true\"]"));
    assert!(script.contains("block-size: 0 !important;"));
    assert!(script.contains("max-block-size: 0 !important;"));
    assert!(script.contains("overflow: clip !important;"));
    assert!(script.contains("cleanupLegacyCodexComposerOverflowGuards"));
    assert!(script.contains("cleanupLegacyCodexComposerOverflowGuards();"));
    assert!(!script.contains("codexComposerOverflowSurfaces"));
    assert!(!script.contains("codexComposerHiddenMeasurementOverflows"));
    assert!(!script.contains("syncCodexComposerOverflowGuard"));
    assert!(!script.contains(
        "syncCodexServiceTierComposerOverflowGuard(enabled = codexElvesSettings().serviceTierControls)"
    ));
}

#[test]
fn injection_script_portals_fast_badge_outside_react_owned_composer() {
    let script = assets::injection_script(45221);

    assert!(script.contains("data-codex-service-tier-portal"));
    assert!(script.contains("codexServiceTierPositionPortalBadge"));
    assert!(script.contains("codexServiceTierPlacementRowRect"));
    assert!(script.contains("codexServiceTierPortalBadgeLeft"));
    assert!(script.contains("const controlPadding = 6"));
    assert!(script.contains("rect.left - cursor >= badgeWidth"));
    assert!(script.contains(
        "const left = codexServiceTierPortalBadgeLeft(footer, verticalAnchorRect, badgeWidth, desiredLeft)"
    ));
    assert!(script.contains(
        "const verticalAnchorRect = codexServiceTierPlacementRowRect(placement, footer, beforeRect)"
    ));
    assert!(
        script.contains("verticalAnchorRect.top + (verticalAnchorRect.height - badgeHeight) / 2")
    );
    assert!(!script.contains("footerRect.top + (footerRect.height - badgeHeight) / 2"));
    assert!(script.contains("portalRoot.appendChild(badge)"));
    assert!(script.contains("codexServiceTierKeepPortalBadgeDuringTransientLayout"));
    assert!(script.contains("codexServiceTierBadgePlacementGraceMs"));
    assert!(script.contains("codexServiceTierBadgeRetryMaxAttempts"));
    assert!(script.contains("codexServiceTierBadgeRetryMaxDelayMs"));
    assert!(script.contains("scheduleCodexServiceTierBadgeLayout"));
    assert!(!script.contains("placement.parent.insertBefore(badge, before)"));
}

#[test]
fn injection_script_refreshes_fast_state_after_backend_load_and_route_entry() {
    let script = assets::injection_script(45221).replace("\r\n", "\n");

    assert!(script.contains("refreshCodexServiceTierFeatureState"));
    assert!(script.contains("if (key === codexElvesBackendSettingMap.serviceTierControls)"));
    assert!(script.contains("refreshCodexServiceTierFeatureState();"));
    assert!(script.contains("refreshCodexTokenUsageFeatureState();"));
    assert!(script.contains("void applyLoadedBackendSettings(settings, \"settings-loaded\")"));
    assert!(script.contains("return refreshCodexSessionPrewarmFeatureState(reason);"));
    assert!(script.contains("refreshCodexSessionPrewarmFeatureState(`setting-${localKey}`)"));
    assert!(script.contains("installCodexServiceTierDispatcherPatch();"));
    assert!(script.contains("installCodexServiceTierRequestClientPatch();"));
    assert!(script.contains("refreshUpstreamBranchDropdownAdapter();"));
    assert!(script.contains("syncChatsSortVisibilityListener();"));
    assert!(script.contains(
        "scheduleConversationViewRouteRefresh();\n    refreshCodexServiceTierFeatureState();"
    ));
}

#[test]
fn injection_script_prompts_for_markdown_export_path_when_supported() {
    let script = assets::injection_script(45221);

    assert!(script.contains("showSaveFilePicker"));
    assert!(script.contains("suggestedName: filename"));
    assert!(script.contains("createWritable()"));
    assert!(script.contains("await writable.write(markdown)"));
    assert!(script.contains("status: \"cancelled\""));
    assert!(script.contains("导出已取消"));
}

#[test]
fn injection_script_applies_fast_service_tier_contract() {
    let cases = run_service_tier_contract_harness();

    assert_eq!(cases["supportedFast"]["serviceTier"], "priority");
    assert_eq!(cases["supportedFast"]["service_tier"], "priority");

    assert_eq!(
        cases["unsupportedModel"]["serviceTier"],
        serde_json::Value::Null
    );
    assert_eq!(
        cases["unsupportedModel"]["service_tier"],
        serde_json::Value::Null
    );

    assert_eq!(cases["turnWithoutModel"]["serviceTier"], "priority");
    assert_eq!(cases["turnWithoutModelDiagnosticModel"], "gpt-5.4");

    assert_eq!(
        cases["customInheritUnsupported"]["serviceTier"],
        serde_json::Value::Null
    );
    assert_eq!(
        cases["customInheritUnsupported"]["service_tier"],
        serde_json::Value::Null
    );

    assert_eq!(cases["startConversation"]["serviceTier"], "priority");

    for model in [
        "gpt-5.6",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "openai/gpt-5.6-terra",
        "gpt-5.6-sol-2026-07-09",
    ] {
        assert_eq!(
            cases["gpt56Fast"][model]["service_tier"], "priority",
            "{model} 应启用 Fast"
        );
        assert_eq!(
            cases["gpt56Fast"][model]["serviceTier"], "priority",
            "{model} 应同步 serviceTier"
        );
    }
    assert_eq!(cases["gpt56EmptyCatalogFast"]["service_tier"], "priority");
    assert_eq!(cases["displayNameMatches"]["gpt56Sol"], true);
    assert_eq!(cases["displayNameMatches"]["gpt56Terra"], true);
    assert_eq!(cases["displayNameMatches"]["gpt55"], true);

    // catalog 驱动：白名单之外但 catalog 标记 supports_fast 的模型也能注入 priority
    assert_eq!(cases["catalogDrivenFast"]["service_tier"], "priority");
    assert_eq!(cases["catalogDrivenFast"]["serviceTier"], "priority");
    // catalog 明确 supports_fast=false 时，名字像 gpt-5.4 也被阻断
    assert_eq!(
        cases["catalogDrivenBlocked"]["service_tier"],
        serde_json::Value::Null
    );
    assert_eq!(
        cases["patchedCreateRequest"]["params"]["serviceTier"],
        "priority"
    );
    assert_eq!(
        cases["patchedCreateRequest"]["params"]["service_tier"],
        "priority"
    );
    assert_eq!(cases["patchedCreateRequest"]["options"]["timeoutMs"], 123);
    assert_eq!(
        cases["relayModelNames"],
        json!(["first-model", "second-model", "current-model"])
    );
    assert_eq!(
        cases["modelVisibilityConfig"],
        json!({
            "default_model": "gpt-5.4",
            "use_hidden_models": false,
            "available_models": ["gpt-5.4"]
        })
    );
    assert_eq!(
        cases["managerDiscoveryBackoffMs"],
        json!([1000, 2000, 30000])
    );
    assert_eq!(cases["pluginAutoExpandLabels"]["latestChinese"], false);
    assert_eq!(
        cases["pluginAutoExpandLabels"]["latestChineseCompact"],
        false
    );
    assert_eq!(cases["pluginAutoExpandLabels"]["legacyChinese"], true);
    assert_eq!(cases["pluginAutoExpandLabels"]["unrelatedCount"], false);
    assert_eq!(
        cases["badgeTooltip"]["dataCodexTooltip"],
        serde_json::Value::Null
    );
    assert_eq!(
        cases["badgeTooltip"]["title"],
        cases["badgeTooltip"]["ariaLabel"]
    );
    assert!(
        cases["badgeTooltip"]["title"]
            .as_str()
            .is_some_and(|value| value.contains("服务模式"))
    );
    assert_eq!(cases["serviceTierRetry"]["dispatcherAttempts"], 3);
    assert_eq!(cases["serviceTierRetry"]["requestClientAttempts"], 2);
    assert_eq!(cases["serviceTierRetry"]["dispatcherInstalled"], true);
    assert_eq!(cases["serviceTierRetry"]["requestClientInstalled"], true);
    assert_eq!(cases["serviceTierRetry"]["dispatcherRetryPending"], false);
    assert_eq!(
        cases["serviceTierRetry"]["requestClientRetryPending"],
        false
    );
}

#[test]
fn injection_script_does_not_patch_app_server_model_requests() {
    let script = assets::injection_script(45221);
    assert!(script.contains("const codexAppServerManagerDiscoveryVersion = \"1\";"));
    assert!(!script.contains("__codexElvesModelOriginalSendRequest"));
    assert!(!script.contains("__codexElvesModelRequestPatch"));
    assert!(!script.contains("codexElvesModelPatchedSendRequest"));
}

#[test]
fn injection_script_prewarms_sessions_with_bounded_concurrency_and_deduplication() {
    let script = assets::injection_script(45221);
    let cases = run_service_tier_contract_harness();
    let prewarm = &cases["sessionPrewarm"];
    let runtime_generation = run_runtime_manager_discovery_generation_harness();

    assert!(script.contains("@keyframes codex-session-prewarm-shimmer"));
    assert!(script.contains("data-codex-session-prewarming"));
    assert!(script.contains("-webkit-mask-size: 42% 100%"));
    assert!(script.contains("mask-position: 170% 0"));
    assert!(!script.contains("@media (prefers-reduced-motion: reduce)"));
    assert!(script.contains("session_prewarm_launch_cycle_reset"));
    assert!(script.contains("\"launch-cycle-refresh\""));
    assert!(script.contains("session_prewarm_recent_refresh_timeout"));
    assert!(script.contains("session_prewarm_skipped"));
    assert!(script.contains("const codexSessionPrewarmVersion = \"3\";"));
    assert!(script.contains("const codexSessionPrewarmStartupDelayMs = 200;"));
    assert!(script.contains("const codexSessionPrewarmMaxAgeMs = 24 * 60 * 60 * 1000;"));
    assert!(script.contains(
        "window.__codexElvesRuntimeManagerDiscoveryVersion === codexAppServerManagerDiscoveryVersion"
    ));
    assert!(script.contains(
        "window.__codexElvesRuntimeManagerDiscoveryVersion = codexAppServerManagerDiscoveryVersion;"
    ));
    let runtime_refresh = script
        .find("window.__codexElvesRefreshRuntime();")
        .expect("same-build reinjection should refresh the existing runtime");
    let runtime_increment = script
        .find("window.__codexSessionPrewarmRuntimeId =")
        .expect("new runtime installation should allocate a prewarm runtime id");
    assert!(
        runtime_refresh < runtime_increment,
        "same-build reinjection must return before invalidating the existing prewarm runtime"
    );
    assert_eq!(
        runtime_generation["current"],
        json!({
            "refreshCalls": 1,
            "runtimeId": 10,
            "managerDiscoveryVersion": "1"
        })
    );
    assert_eq!(
        runtime_generation["old"],
        json!({
            "refreshCalls": 0,
            "runtimeId": 11,
            "managerDiscoveryVersion": "0"
        })
    );
    assert_eq!(
        runtime_generation["missing"],
        json!({
            "refreshCalls": 0,
            "runtimeId": 11,
            "managerDiscoveryVersion": null
        })
    );
    assert_eq!(
        prewarm["defaultPrewarmSettings"],
        json!({
            "enabled": true,
            "fullCount": 3,
            "contentCount": 3,
            "concurrency": 4
        })
    );
    assert_eq!(
        prewarm["taskTypes"],
        json!([
            "full", "full", "full", "full", "content", "content", "content", "content", "content",
            "content"
        ])
    );
    assert_eq!(
        prewarm["taskIds"],
        json!([
            "prewarm-thread-01",
            "prewarm-thread-02",
            "prewarm-thread-03",
            "prewarm-thread-04",
            "prewarm-thread-05",
            "prewarm-thread-06",
            "prewarm-thread-07",
            "prewarm-thread-08",
            "prewarm-thread-09",
            "prewarm-thread-10"
        ])
    );
    assert_eq!(
        prewarm["subagentFilterTaskIds"],
        json!(["prewarm-thread-normal", "prewarm-thread-fork"])
    );
    assert_eq!(
        prewarm["ageFilterTaskIds"],
        json!(["prewarm-thread-recent", "prewarm-thread-boundary"])
    );
    assert_eq!(prewarm["completed"], 10);
    assert_eq!(prewarm["failed"], 0);
    assert_eq!(prewarm["maxActiveResumes"], 4);
    assert_eq!(prewarm["maxActiveIndicators"], 4);
    assert_eq!(prewarm["activeIndicatorsAfterQueue"], json!([]));
    assert_eq!(prewarm["limitedMaxActiveResumes"], 2);
    assert_eq!(prewarm["limitedConcurrencySummary"]["completed"], 5);
    assert_eq!(prewarm["limitedConcurrencySummary"]["failed"], 0);
    assert_eq!(prewarm["zeroConcurrencyResumeCalls"], 1);
    assert_eq!(prewarm["zeroConcurrencySummary"]["completed"], 1);
    assert_eq!(
        prewarm["resumeCalls"]
            .as_array()
            .expect("resumeCalls should be an array")
            .len(),
        4
    );
    assert_eq!(
        prewarm["unsubscribeCalls"]
            .as_array()
            .expect("unsubscribeCalls should be an array")
            .len(),
        0
    );
    assert_eq!(prewarm["duplicateResumeCalls"], 1);
    assert_eq!(prewarm["promotedResumeCalls"], 0);
    assert_eq!(prewarm["promotedHydrationCalls"], 1);
    assert_eq!(prewarm["promotedUnsubscribeCalls"], 0);
    assert_eq!(prewarm["promotedIndicatorActiveDuring"], true);
    assert_eq!(prewarm["promotedIndicatorActiveAfter"], false);
    assert_eq!(prewarm["promotedForegroundRetained"], false);
    assert_eq!(
        prewarm["indicatorActiveAttributes"],
        json!({
            "prewarming": "true",
            "title": "正在预热的会话"
        })
    );
    assert_eq!(
        prewarm["indicatorReusedAttributes"],
        json!({
            "prewarming": null,
            "title": null
        })
    );
    assert_eq!(
        prewarm["indicatorClearedAttributes"],
        json!({
            "prewarming": null,
            "title": null
        })
    );
    assert_eq!(prewarm["fallbackResumeCalls"], 0);
    assert_eq!(
        prewarm["fallbackHydrationCalls"],
        json!([{
            "threadIds": ["prewarm-thread-fallback"],
            "options": {"includeTurns": true}
        }])
    );
    assert_eq!(prewarm["fallbackResult"]["result"], "content-hydrated");
    assert_eq!(prewarm["phasedSummary"]["completed"], 3);
    assert_eq!(prewarm["phasedSummary"]["failed"], 0);
    assert_eq!(prewarm["phasedHydrationOptions"], json!([true, true, true]));
    assert_eq!(
        prewarm["phasedResumeIds"],
        json!(["prewarm-thread-phase-full-a", "prewarm-thread-phase-full-b"])
    );
    assert_eq!(prewarm["phasedContentCompletedBeforeOwner"], true);
    assert_eq!(prewarm["failureSummary"]["completed"], 2);
    assert_eq!(prewarm["failureSummary"]["failed"], 1);
    assert_eq!(
        prewarm["failureCalls"],
        json!(["prewarm-thread-failure", "prewarm-thread-after-failure"])
    );
    assert_eq!(prewarm["currentThreadResumeCalls"], 0);
    assert_eq!(prewarm["currentThreadSummary"]["completed"], 1);
    assert_eq!(
        prewarm["currentThreadSummary"]["results"][0]["result"],
        "foreground"
    );
    assert_eq!(prewarm["hiddenResumeCalls"], 0);
    assert_eq!(prewarm["hiddenQueueSummary"]["completed"], 0);
    assert_eq!(prewarm["hiddenQueueSummary"]["interrupted"], true);
    assert_eq!(prewarm["managerDiscoveryNeededWithManager"], json!(false));
    assert_eq!(prewarm["managerDiscoveryNeededWithoutManager"], json!(true));
    assert_eq!(prewarm["failedRunCompletedSignature"], "");
    assert_eq!(prewarm["failedRunRetryCounts"], json!([1]));
    assert_eq!(prewarm["noTasksRetryCounts"], json!([1]));
    assert_eq!(prewarm["expiredResumeCalls"], 0);
    assert!(
        prewarm["expiredCompletedSignature"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(prewarm["expiredRetryCounts"], json!([]));
    assert_eq!(prewarm["firstManagerResumeCalls"], 1);
    assert_eq!(prewarm["secondManagerResumeCalls"], 1);
    assert_eq!(prewarm["nestedManagerFound"], true);
    assert_eq!(prewarm["nestedManagerHasResume"], true);
    assert_eq!(prewarm["latestManagerFound"], true);
    assert!(
        prewarm["latestManagerScanned"]
            .as_u64()
            .is_some_and(|value| value < 1000)
    );
    assert!(
        prewarm["nestedManagerScanned"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert_eq!(prewarm["coldStartManagerReady"], true);
    assert_eq!(prewarm["coldStartManagerLoadCalls"], 1);
    assert_eq!(prewarm["coldStartResumeCalls"], 1);
    assert_eq!(prewarm["emptyWorkspaceRoots"], json!([]));
    assert_eq!(prewarm["launchCycleReset"], true);
    assert_eq!(prewarm["launchCycleResetRepeated"], false);
    assert_eq!(prewarm["completedSignatureAfterLaunchCycleReset"], "");
    assert_eq!(prewarm["refreshTimeoutStatus"], "timeout");
    assert_eq!(prewarm["refreshTimeoutResumeCalls"], 1);
    assert_eq!(prewarm["refreshStartedAfterResumeCompleted"], true);
    assert!(
        prewarm["refreshTimeoutDurationMs"]
            .as_u64()
            .is_some_and(|value| value < 1000)
    );
}

#[test]
fn injection_script_defaults_session_prewarm_to_disabled() {
    let script = assets::injection_script(45221);

    assert!(script.contains("sessionPrewarmEnabled: false"));
    assert!(script.contains("sessionPrewarmFullCount: 3"));
    assert!(script.contains("sessionPrewarmContentCount: 3"));
    assert!(script.contains("sessionPrewarmConcurrency: codexSessionPrewarmDefaultConcurrency"));
    assert!(script.contains("sessionPrewarmConcurrency: \"codexAppSessionPrewarmConcurrency\""));
    assert!(
        script
            .contains("settings.sessionPrewarmEnabled = settings.sessionPrewarmEnabled === true;")
    );
    assert!(script.contains("enabled: settings.sessionPrewarmEnabled === true"));
}

#[test]
fn injection_script_visible_sort_fallback_refreshes_backend_sort_keys() {
    let script = assets::injection_script(45221).replace("\r\n", "\n");
    let fallback = script
        .split("function armChatsSortVisibleFallback()")
        .nth(1)
        .and_then(|tail| tail.split("function stopChatsSortRuntime()").next())
        .expect("visible sort fallback function should exist");

    assert!(fallback.contains("scheduleChatsSortCorrection(0, { refreshKeys: true });"));
}

fn run_runtime_manager_discovery_generation_harness() -> serde_json::Value {
    let script = assets::injection_script(45221);
    let bootstrap_end = script
        .find("  function installCodexElvesImageOverlay()")
        .expect("runtime bootstrap should precede image overlay installation");
    let bootstrap = format!("{} }})();", &script[..bootstrap_end]);
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let harness_path = temp.path().join("runtime-generation-harness.cjs");
    let harness = format!(
        r#"
const vm = require("node:vm");
const bootstrap = {bootstrap};
globalThis.window = globalThis;
const expectedBuild = JSON.parse(
  bootstrap.match(/window\.__CODEX_ELVES_BUILD__ = (".*?");/)?.[1] || '""'
);
const expectedHelperBase = JSON.parse(
  bootstrap.match(/window\.__CODEX_SESSION_DELETE_HELPER__ = (".*?");/)?.[1] || '""'
);

function runScenario(managerDiscoveryVersion) {{
  window.__codexElvesRuntimeBuild = expectedBuild;
  window.__codexElvesRuntimeHelperBase = expectedHelperBase;
  window.__codexSessionPrewarmRuntimeId = 10;
  window.__codexConversationViewRouteTimers = [];
  window.__codexRouteFeatureRefreshTimers = [];
  window.__codexSessionDeleteObservers = [];
  window.__codexSessionDeleteObserverConfigs = [];
  let refreshCalls = 0;
  window.__codexElvesRefreshRuntime = () => {{
    refreshCalls += 1;
  }};
  if (managerDiscoveryVersion == null) {{
    delete window.__codexElvesRuntimeManagerDiscoveryVersion;
  }} else {{
    window.__codexElvesRuntimeManagerDiscoveryVersion = managerDiscoveryVersion;
  }}
  vm.runInThisContext(bootstrap);
  return {{
    refreshCalls,
    runtimeId: window.__codexSessionPrewarmRuntimeId,
    managerDiscoveryVersion: window.__codexElvesRuntimeManagerDiscoveryVersion || null,
  }};
}}

process.stdout.write(JSON.stringify({{
  current: runScenario("1"),
  old: runScenario("0"),
  missing: runScenario(null),
}}));
"#,
        bootstrap = serde_json::to_string(&bootstrap).expect("bootstrap should serialize")
    );
    std::fs::write(&harness_path, harness).expect("runtime generation harness should be written");
    let output = Command::new("node")
        .arg(&harness_path)
        .output()
        .expect("node should run runtime generation harness");
    assert!(
        output.status.success(),
        "runtime generation harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("runtime generation stdout should be JSON")
}

fn run_service_tier_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("service-tier-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    let mut harness = std::fs::File::create(&harness_path).expect("harness should be created");
    write!(
        harness,
        r#"
const scriptPath = {script_path};
const store = new Map();
store.set("codexElvesSettings", JSON.stringify({{
  serviceTierControls: true,
  sessionPrewarmEnabled: true,
}}));
function node() {{
  return {{
    appendChild() {{}},
    prepend() {{}},
    remove() {{}},
    setAttribute() {{}},
    removeAttribute() {{}},
    addEventListener() {{}},
    querySelector() {{ return null; }},
    querySelectorAll() {{ return []; }},
    closest() {{ return null; }},
    classList: {{ add() {{}}, remove() {{}}, toggle() {{}}, contains() {{ return false; }} }},
    dataset: {{}},
    style: {{}},
    children: [],
    isConnected: true,
    textContent: "",
    innerHTML: "",
  }};
}}
globalThis.HTMLElement = Object;
globalThis.MutationObserver = class MutationObserver {{
  constructor(callback) {{
    this.callback = callback;
  }}
  observe() {{}}
  disconnect() {{}}
}};
globalThis.getComputedStyle = () => ({{
  display: "block",
  visibility: "visible",
  pointerEvents: "auto",
}});
globalThis.window = globalThis;
window.__CODEX_ELVES_TEST_SERVICE_TIER__ = true;
window.__CODEX_ELVES_TEST_SESSION_PREWARM__ = true;
window.__CODEX_ELVES_TEST_PLUGIN_AUTO_EXPAND__ = true;
window.dispatchEvent = () => true;
globalThis.CustomEvent = class CustomEvent {{
  constructor(type, options = {{}}) {{
    this.type = type;
    this.detail = options.detail;
  }}
}};
globalThis.Event = class Event {{
  constructor(type) {{
    this.type = type;
  }}
}};
globalThis.document = {{
  scripts: [],
  visibilityState: "visible",
  documentElement: node(),
  body: node(),
  createElement: () => node(),
  getElementById: () => null,
  querySelector: () => null,
  querySelectorAll: () => [],
  addEventListener() {{}},
  removeEventListener() {{}},
}};
globalThis.localStorage = {{
  getItem: (key) => store.has(key) ? store.get(key) : null,
  setItem: (key, value) => store.set(key, String(value)),
  removeItem: (key) => store.delete(key),
}};
globalThis.location = {{ href: "https://codex.test/local/thread-12345678", pathname: "/local/thread-12345678", search: "", hash: "" }};
window.location = globalThis.location;
globalThis.navigator = {{ userAgent: "node-test" }};
globalThis.performance = {{ getEntriesByType: () => [] }};
require(scriptPath);
const api = window.__codexElvesServiceTierTest;
const pluginAutoExpandApi = window.__codexElvesPluginAutoExpandTest;
const pluginAutoExpandLabels = {{
  latestChinese: pluginAutoExpandApi.matchesText("另有 4 个"),
  latestChineseCompact: pluginAutoExpandApi.matchesText("另有4个插件"),
  legacyChinese: pluginAutoExpandApi.matchesText("显示更多"),
  unrelatedCount: pluginAutoExpandApi.matchesText("已安装 4 个"),
}};
api.setServiceTierState({{ serviceTier: "priority", fastTierValue: "priority" }});
api.setModelCatalog({{ status: "ok", model: "gpt-5.4", default_model: "gpt-5.4", models: ["gpt-5.4", "gpt-5.5"] }});
const displayNameMatches = {{
  gpt56Sol: api.modelMatchesText("gpt-5.6-sol", "5.6 Sol"),
  gpt56Terra: api.modelMatchesText("gpt-5.6-terra", "5.6 Terra"),
  gpt55: api.modelMatchesText("gpt-5.5", "5.5 超高"),
}};

api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const supportedFast = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-5.4",
  service_tier: null,
}}, "conv-should-not-be-model");

const unsupportedModel = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-4.1",
  service_tier: "priority",
}}, "conv-should-not-be-model");

const turnWithoutModel = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  service_tier: null,
}}, "conversation-should-not-be-model");
const turnWithoutModelDiagnosticModel = api.diagnostics().at(-1)?.detail?.model;

api.setModelCatalog({{ status: "ok", model: "gpt-4.1", default_model: "gpt-4.1", models: ["gpt-4.1"] }});
api.setThreadState({{ mode: "custom", defaultMode: "inherit", entries: {{}}, draft: {{ mode: "inherit", at: Date.now() }} }});
api.setServiceTierState({{ serviceTier: "priority" }});
const customInheritUnsupported = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  service_tier: "priority",
}}, "");

api.setModelCatalog({{ status: "ok", model: "gpt-5.5", default_model: "gpt-5.5", models: ["gpt-5.5"] }});
api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const startConversation = api.requestOverride({{
  type: "start-conversation",
  threadId: "thread-12345678",
  model: "gpt-5.5",
}});

const gpt56Fast = {{}};
for (const model of [
  "gpt-5.6",
  "gpt-5.6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "openai/gpt-5.6-terra",
  "gpt-5.6-sol-2026-07-09",
]) {{
  api.setModelCatalog({{ status: "ok", model, default_model: model, models: [model] }});
  api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
  gpt56Fast[model] = api.applyServiceTierOverride("turn/start", {{
    threadId: "thread-12345678",
    model,
    service_tier: null,
  }}, "");
}}

api.setModelCatalog({{
  status: "ok",
  model: "gpt-5.6-luna",
  default_model: "gpt-5.6-luna",
  models: ["gpt-5.6-luna"],
  model_entries: [{{ slug: "gpt-5.6-luna", service_tiers: [] }}],
}});
api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const gpt56EmptyCatalogFast = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-5.6-luna",
  service_tier: null,
}}, "");

// catalog 驱动：内置白名单之外的模型，但 catalog 标记 supports_fast=true 也应支持
api.setModelCatalog({{
  status: "ok",
  model: "gpt-5.6-custom",
  default_model: "gpt-5.6-custom",
  models: ["gpt-5.6-custom"],
  model_entries: [{{ slug: "gpt-5.6-custom", supports_fast: true, service_tiers: [{{ id: "priority", name: "Fast" }}] }}],
}});
api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const catalogDrivenFast = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-5.6-custom",
  service_tier: null,
}}, "");

// catalog 明确标记不支持（supports_fast=false）时，即使属于 GPT-5.6 系列也应被阻断
api.setModelCatalog({{
  status: "ok",
  model: "gpt-5.6-terra",
  default_model: "gpt-5.6-terra",
  models: ["gpt-5.6-terra"],
  model_entries: [{{ slug: "gpt-5.6-terra", supports_fast: false, service_tiers: [] }}],
}});
api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const catalogDrivenBlocked = api.applyServiceTierOverride("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-5.6-terra",
  service_tier: "priority",
}}, "");

class RequestClient {{
  createRequest(method, params, options) {{
    return {{ request: {{ method, params, options }}, promise: Promise.resolve(null) }};
  }}
  sendRequest() {{}}
  prewarmThreadStart() {{}}
}}
api.patchRequestClientPrototype(RequestClient);
api.setModelCatalog({{ status: "ok", model: "gpt-5.4", default_model: "gpt-5.4", models: ["gpt-5.4"] }});
api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
const patchedCreateRequest = new RequestClient().createRequest("turn/start", {{
  threadId: "thread-12345678",
  model: "gpt-5.4",
  service_tier: null,
}}, {{ timeoutMs: 123 }}).request;

api.setModelCatalog({{
  status: "ok",
  model_provider: "relay",
  model: "current-model",
  default_model: "first-model",
  models: ["first-model", "second-model", "current-model"],
}});
const relayModelNames = api.modelNames();
const modelVisibilityConfig = api.patchStatsigModelVisibilityConfig({{
  value: {{
    default_model: "gpt-5.4",
    use_hidden_models: true,
    available_models: ["gpt-5.4"],
  }},
}}).value;
const managerDiscoveryBackoffMs = [
  api.appServerManagerDiscoveryBackoffMs(1),
  api.appServerManagerDiscoveryBackoffMs(2),
  api.appServerManagerDiscoveryBackoffMs(10),
];

const pluginMarketplaceResult = {{
  marketplaces: [
    {{
      name: "openai-bundled",
      plugins: [
        {{ name: "official-plugin", marketplaceName: "openai-bundled" }},
        {{ name: "local-plugin", marketplaceName: "local-marketplace" }},
      ],
    }},
    {{
      name: "openai-curated-remote",
      interface: {{ displayName: "OpenAI Curated Remote" }},
      plugins: [
        {{ name: "remote-plugin", marketplaceName: "openai-curated-remote" }},
      ],
    }},
    {{ name: "local-marketplace", plugins: [] }},
  ],
}};
api.patchPluginMarketplaceResult("list-plugins", pluginMarketplaceResult);
const originalPluginMarketplaceRequestParams = {{
  marketplaceKinds: ["created-by-me-remote"],
}};
const pluginMarketplaceRequestParams = {{
  personal: api.patchPluginMarketplaceRequestParams(
    "list-plugins",
    originalPluginMarketplaceRequestParams,
  ),
  mixed: api.patchPluginMarketplaceRequestParams("list-plugins", {{
    marketplaceKinds: ["created-by-me-remote", "workspace"],
  }}),
  original: originalPluginMarketplaceRequestParams,
}};
async function runPluginMarketplaceRequestClientCase() {{
  const calls = [];
  const client = {{
    async sendRequest(method, params, options) {{
      calls.push({{ method, params, options }});
      return {{ marketplaces: [{{ name: "local-marketplace", plugins: [] }}] }};
    }},
  }};
  api.patchPluginMarketplaceRequestClient(client);
  const unsupportedResult = await client.sendRequest(
    "plugin/list",
    {{ marketplaceKinds: ["created-by-me-remote"] }},
    {{ timeoutMs: 123 }},
  );
  const supportedResult = await client.sendRequest(
    "plugin/list",
    {{ marketplaceKinds: ["workspace"] }},
    {{ timeoutMs: 123 }},
  );
  return {{
    calls,
    unsupportedCount: unsupportedResult.marketplaces.length,
    supportedCount: supportedResult.marketplaces.length,
  }};
}}
const u = (name) => String(name || "") === "openai-bundled";
const r = "openai-bundled";
const t = ["openai-bundled"];
const buildFlavorFilter=(e)=>!u(e.marketplaceName)||e.marketplaceName===r;
const hiddenMarketplaceFilter=(e)=>!t.includes(e.name);
const visibleMarketplaces = pluginMarketplaceResult.marketplaces.filter(hiddenMarketplaceFilter);
const derivedPlugins = [];
for (const marketplace of visibleMarketplaces) {{
  for (const plugin of marketplace.plugins) {{
    derivedPlugins.push({{ ...plugin, marketplaceName: marketplace.name }});
  }}
}}
const visiblePlugins = derivedPlugins.filter(buildFlavorFilter);
const pluginScopedFilters = {{
  pluginCount: visiblePlugins.length,
  pluginTotal: derivedPlugins.length,
  marketplaceCount: visibleMarketplaces.length,
  marketplaceTotal: pluginMarketplaceResult.marketplaces.length,
  officialMarketplaceName: pluginMarketplaceResult.marketplaces[0].name,
  curatedRemoteMarketplaceName: pluginMarketplaceResult.marketplaces[1].name,
  catalogReady: derivedPlugins.some((plugin) => {{
    const normalize = (value) => String(value || "")
      .trim()
      .toLowerCase()
      .replace(/[_-]+/g, " ");
    const recognized = new Set([
      "codex official",
      "openai curated",
      "openai curated remote",
    ]);
    return recognized.has(normalize(plugin.marketplaceName))
      || recognized.has(normalize(plugin.marketplaceDisplayName));
  }}),
  pluginFilterIsOwn: Object.prototype.hasOwnProperty.call(
    pluginMarketplaceResult.marketplaces[0].plugins,
    "filter",
  ),
  marketplaceFilterIsOwn: Object.prototype.hasOwnProperty.call(
    pluginMarketplaceResult.marketplaces,
    "filter",
  ),
  ordinaryFilter: [1, 2, 3].filter((value) => value > 1),
}};

const badgeNode = {{
  dataset: {{ codexTooltip: "stale custom tooltip" }},
  textContent: "",
  attributes: {{}},
  removeAttribute(name) {{
    delete this.attributes[name];
    if (name === "data-codex-tooltip") delete this.dataset.codexTooltip;
    if (name === "title") delete this.title;
  }},
  setAttribute(name, value) {{
    this.attributes[name] = String(value);
    if (name === "title") this.title = String(value);
  }},
}};
api.refreshBadgeNode(badgeNode);
const badgeTooltip = {{
  dataCodexTooltip: Object.prototype.hasOwnProperty.call(badgeNode.dataset, "codexTooltip") ? badgeNode.dataset.codexTooltip : null,
  title: badgeNode.title || "",
  ariaLabel: badgeNode.attributes["aria-label"] || "",
}};

async function waitForCondition(predicate, timeoutMs = 6000) {{
  const startedAt = Date.now();
  while (!predicate()) {{
    if (Date.now() - startedAt > timeoutMs) throw new Error("condition wait timed out");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }}
}}

async function runServiceTierRetryCase() {{
  api.resetServiceTierInstallState();
  let dispatcherAttempts = 0;
  let requestClientAttempts = 0;
  const dispatcher = {{
    dispatchMessage() {{}},
  }};
  function DispatcherFactory() {{
    return "dispatchMessage";
  }}
  DispatcherFactory.getInstance = () => dispatcher;
  class RetryRequestClient {{
    createRequest(method, params, options) {{
      return {{ request: {{ method, params, options }}, promise: Promise.resolve(null) }};
    }}
    sendRequest() {{}}
    prewarmThreadStart() {{}}
  }}
  api.setModuleLoader(async (namePart) => {{
    if (namePart === "thread-context-inputs-") {{
      requestClientAttempts += 1;
      if (requestClientAttempts === 1) throw new Error("request client module not ready");
      return {{ RetryRequestClient }};
    }}
    if (namePart === "vscode-api-" || namePart === "setting-storage-") {{
      dispatcherAttempts += 1;
      if (dispatcherAttempts <= 2) throw new Error("dispatcher module not ready");
      return {{ DispatcherFactory }};
    }}
    throw new Error(`unexpected module: ${{namePart}}`);
  }});
  await Promise.allSettled([
    api.installDispatcherPatch(),
    api.installRequestClientPatch(),
  ]);
  await waitForCondition(() => {{
    const state = api.serviceTierInstallState();
    return state.dispatcherInstalled && state.requestClientInstalled;
  }});
  const state = api.serviceTierInstallState();
  api.setModuleLoader(null);
  return {{
    dispatcherAttempts,
    requestClientAttempts,
    ...state,
  }};
}}

async function runSessionPrewarmCases() {{
  const prewarmApi = window.__codexElvesSessionPrewarmTest;
  const defaultPrewarmSettings = prewarmApi.settingsSnapshot();
  const nestedReadOnlyManager = {{
    getHostId() {{
      return "local";
    }},
    getRecentConversations() {{
      return [];
    }},
    async sendRequest() {{}},
  }};
  const nestedFullManager = {{
    getHostId() {{
      return "local";
    }},
    getRecentConversations() {{
      return [];
    }},
    async sendRequest() {{}},
    async resumeConversationForUnavailableOwner() {{}},
    async unsubscribeInactiveConversation() {{}},
  }};
  const nestedManagerGraph = {{
    node: {{
      familyBindings: new Map([
        ["read-only", {{ atom: {{ init: nestedReadOnlyManager }} }}],
        ["full", {{ atom: {{ init: nestedFullManager }} }}],
      ]),
    }},
  }};
  const nestedManagerResult = prewarmApi.findManagerFromRoots([nestedManagerGraph]);
  const nestedManagerFound = nestedManagerResult.manager === nestedFullManager;
  const nestedManagerHasResume =
    typeof nestedManagerResult.manager?.resumeConversationForUnavailableOwner === "function";
  const nestedManagerScanned = nestedManagerResult.scanned;
  const latestManagerScope = {{
    familyBindings: new Map([
      ["manager", {{ atom: {{ init: nestedFullManager }} }}],
    ]),
  }};
  const latestManagerGraph = {{}};
  for (let index = 0; index < 255; index += 1) {{
    const branch = {{}};
    for (let childIndex = 0; childIndex < 64; childIndex += 1) {{
      branch[`child-${{childIndex}}`] = {{}};
    }}
    latestManagerGraph[`noise-${{index}}`] = branch;
  }}
  latestManagerGraph.child = {{
    dependencies: {{
      firstContext: {{
        memoizedValue: new Map([["scope", latestManagerScope]]),
      }},
    }},
  }};
  const latestManagerResult = prewarmApi.findManagerFromRoots([latestManagerGraph]);
  const latestManagerFound = latestManagerResult.manager === nestedFullManager;
  const latestManagerScanned = latestManagerResult.scanned;
  const prewarmNow = Date.now();
  const conversations = Array.from({{ length: 12 }}, (_, index) => ({{
    id: `prewarm-thread-${{String(index + 1).padStart(2, "0")}}`,
    cwd: `C:/workspace/${{index + 1}}`,
    updatedAt: prewarmNow - (index * 60 * 60 * 1000),
  }}));
  conversations.splice(2, 0, {{
    id: "thread-12345678",
    cwd: "C:/workspace/active",
    updatedAt: prewarmNow,
  }});
  conversations.splice(5, 0, {{
    id: "prewarm-thread-busy",
    cwd: "C:/workspace/busy",
    updatedAt: prewarmNow,
    threadRuntimeStatus: {{ type: "active" }},
  }});
  const tasks = prewarmApi.buildTasks(
    conversations,
    {{ fullCount: 4, contentCount: 6 }},
    "thread-12345678",
    prewarmNow,
  );
  const subagentFilterTasks = prewarmApi.buildTasks([
    {{ id: "prewarm-thread-normal", updatedAt: prewarmNow }},
    {{ id: "prewarm-thread-parent", updatedAt: prewarmNow, parentThreadId: "parent-thread" }},
    {{ id: "prewarm-thread-source-parent", updatedAt: prewarmNow, source: {{ parentThreadId: "parent-thread" }} }},
    {{ id: "prewarm-thread-subagent-parent", updatedAt: prewarmNow, subagentParentThreadId: "parent-thread" }},
    {{ id: "prewarm-thread-subagent-source", updatedAt: prewarmNow, isSubagentSource: true }},
    {{ id: "prewarm-thread-fork", updatedAt: prewarmNow, forkedFromId: "source-thread" }},
  ], {{ fullCount: 10, contentCount: 0 }}, "", prewarmNow);
  const ageFilterTasks = prewarmApi.buildTasks([
    {{ id: "prewarm-thread-recent", updatedAt: prewarmNow - ((24 * 60 * 60 * 1000) - 1) }},
    {{ id: "prewarm-thread-boundary", updatedAt: prewarmNow - (24 * 60 * 60 * 1000) }},
    {{ id: "prewarm-thread-expired", updatedAt: prewarmNow - ((24 * 60 * 60 * 1000) + 1) }},
    {{ id: "prewarm-thread-missing-time" }},
  ], {{ fullCount: 10, contentCount: 0 }}, "", prewarmNow);
  const indicatorAttributes = {{}};
  const indicatorTitle = {{
    textContent: "正在预热的会话",
    setAttribute(name, value) {{
      indicatorAttributes[name] = String(value);
    }},
    removeAttribute(name) {{
      delete indicatorAttributes[name];
    }},
    closest() {{
      return indicatorRow;
    }},
  }};
  let indicatorRowThreadId = "prewarm-thread-indicator";
  const indicatorRow = {{
    getAttribute(name) {{
      if (name === "data-app-action-sidebar-thread-id") return indicatorRowThreadId;
      return "";
    }},
    querySelector(selector) {{
      if (selector === "a") return null;
      return indicatorTitle;
    }},
  }};
  const indicatorSnapshot = () => ({{
    prewarming: indicatorAttributes["data-codex-session-prewarming"] ?? null,
    title: indicatorAttributes["data-codex-session-prewarm-title"] ?? null,
  }});
  prewarmApi.setIndicatorActive("prewarm-thread-indicator", true);
  prewarmApi.syncIndicators([indicatorRow]);
  const indicatorActiveAttributes = indicatorSnapshot();
  indicatorRowThreadId = "prewarm-thread-reused";
  prewarmApi.syncIndicators([indicatorRow]);
  const indicatorReusedAttributes = indicatorSnapshot();
  indicatorRowThreadId = "prewarm-thread-indicator";
  prewarmApi.setIndicatorActive("prewarm-thread-indicator", false);
  prewarmApi.syncIndicators([indicatorRow]);
  const indicatorClearedAttributes = indicatorSnapshot();

  let activeResumes = 0;
  let maxActiveResumes = 0;
  let maxActiveIndicators = 0;
  const resumed = new Set();
  const resumeCalls = [];
  const unsubscribeCalls = [];
  const queueManager = {{
    needsResume(threadId) {{
      return !resumed.has(threadId);
    }},
    async resumeConversationForUnavailableOwner(params) {{
      activeResumes += 1;
      maxActiveResumes = Math.max(maxActiveResumes, activeResumes);
      maxActiveIndicators = Math.max(
        maxActiveIndicators,
        prewarmApi.activeIndicatorIds().length,
      );
      resumeCalls.push(params.conversationId);
      await new Promise((resolve) => setTimeout(resolve, 5));
      resumed.add(params.conversationId);
      activeResumes -= 1;
    }},
    async unsubscribeInactiveConversation(threadId) {{
      unsubscribeCalls.push(threadId);
      resumed.delete(threadId);
    }},
  }};
  const queueSummary = await prewarmApi.runQueue(queueManager, tasks);
  const activeIndicatorsAfterQueue = prewarmApi.activeIndicatorIds();
  let limitedActiveResumes = 0;
  let limitedMaxActiveResumes = 0;
  const limitedConcurrencySummary = await prewarmApi.runQueue({{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      limitedActiveResumes += 1;
      limitedMaxActiveResumes = Math.max(limitedMaxActiveResumes, limitedActiveResumes);
      await new Promise((resolve) => setTimeout(resolve, 5));
      limitedActiveResumes -= 1;
    }},
  }}, Array.from({{ length: 5 }}, (_, index) => ({{
    type: "full",
    threadId: `prewarm-thread-limited-${{index + 1}}`,
    conversation: {{ id: `prewarm-thread-limited-${{index + 1}}` }},
  }})), 2);
  let zeroConcurrencyResumeCalls = 0;
  const zeroConcurrencySummary = await prewarmApi.runQueue({{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      zeroConcurrencyResumeCalls += 1;
    }},
  }}, [{{
    type: "full",
    threadId: "prewarm-thread-paused",
    conversation: {{ id: "prewarm-thread-paused" }},
  }}], 0);

  let duplicateResumeCalls = 0;
  const duplicateManager = {{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      duplicateResumeCalls += 1;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }},
  }};
  const duplicateTask = {{
    type: "full",
    threadId: "prewarm-thread-duplicate",
    conversation: {{ id: "prewarm-thread-duplicate", cwd: "C:/workspace/duplicate" }},
  }};
  await Promise.all([
    prewarmApi.runTask(duplicateManager, duplicateTask),
    prewarmApi.runTask(duplicateManager, duplicateTask),
  ]);

  let promotedResumeCalls = 0;
  let promotedHydrationCalls = 0;
  let promotedUnsubscribeCalls = 0;
  let releasePromotedResume;
  const promotedResumeGate = new Promise((resolve) => {{
    releasePromotedResume = resolve;
  }});
  const promotedManager = {{
    needsResume() {{
      return promotedResumeCalls === 0;
    }},
    async resumeConversationForUnavailableOwner() {{
      promotedResumeCalls += 1;
    }},
    async hydrateBackgroundThreads() {{
      promotedHydrationCalls += 1;
      await promotedResumeGate;
    }},
    async unsubscribeInactiveConversation() {{
      promotedUnsubscribeCalls += 1;
    }},
  }};
  const promotedTask = {{
    type: "content",
    threadId: "prewarm-thread-promoted",
    conversation: {{ id: "prewarm-thread-promoted", cwd: "C:/workspace/promoted" }},
  }};
  const promotedPromise = prewarmApi.runTask(promotedManager, promotedTask);
  await Promise.resolve();
  const promotedIndicatorActiveDuring =
    prewarmApi.activeIndicatorIds().includes(promotedTask.threadId);
  prewarmApi.markForeground(promotedTask.threadId);
  releasePromotedResume();
  const promotedResult = await promotedPromise;
  const promotedIndicatorActiveAfter =
    prewarmApi.activeIndicatorIds().includes(promotedTask.threadId);
  const promotedForegroundRetained = prewarmApi.isForeground(promotedTask.threadId);

  let fallbackResumeCalls = 0;
  const fallbackHydrationCalls = [];
  const fallbackManager = {{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      fallbackResumeCalls += 1;
    }},
    async hydrateBackgroundThreads(threadIds, options) {{
      fallbackHydrationCalls.push({{ threadIds, options }});
    }},
  }};
  const fallbackResult = await prewarmApi.runTask(fallbackManager, {{
    type: "content",
    threadId: "prewarm-thread-fallback",
    conversation: {{ id: "prewarm-thread-fallback", cwd: "C:/workspace/fallback" }},
  }});

  const phasedEvents = [];
  const phasedHydrationOptions = [];
  const phasedResumeIds = [];
  const phasedManager = {{
    async hydrateBackgroundThreads(threadIds, options) {{
      phasedHydrationOptions.push(options.includeTurns);
      phasedEvents.push(`hydrate:${{threadIds[0]}}`);
      await new Promise((resolve) => setTimeout(resolve, 1));
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner(params) {{
      phasedResumeIds.push(params.conversationId);
      phasedEvents.push(`resume:${{params.conversationId}}`);
    }},
  }};
  const phasedSummary = await prewarmApi.runPhasedQueue(phasedManager, [
    {{
      type: "full",
      threadId: "prewarm-thread-phase-full-a",
      conversation: {{ id: "prewarm-thread-phase-full-a" }},
    }},
    {{
      type: "content",
      threadId: "prewarm-thread-phase-content",
      conversation: {{ id: "prewarm-thread-phase-content" }},
    }},
    {{
      type: "full",
      threadId: "prewarm-thread-phase-full-b",
      conversation: {{ id: "prewarm-thread-phase-full-b" }},
    }},
  ], 2);
  const phasedLastHydrationIndex = Math.max(
    ...phasedEvents.map((event, index) => event.startsWith("hydrate:") ? index : -1),
  );
  const phasedFirstResumeIndex = phasedEvents.findIndex((event) => event.startsWith("resume:"));
  const phasedContentCompletedBeforeOwner =
    phasedFirstResumeIndex > phasedLastHydrationIndex;

  const failureCalls = [];
  const failureManager = {{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner(params) {{
      failureCalls.push(params.conversationId);
      if (params.conversationId === "prewarm-thread-failure") throw new Error("transient resume failure");
    }},
  }};
  const failureSummary = await prewarmApi.runQueue(failureManager, [
    {{
      type: "full",
      threadId: "prewarm-thread-failure",
      conversation: {{ id: "prewarm-thread-failure", cwd: "C:/workspace/failure" }},
    }},
    {{
      type: "full",
      threadId: "prewarm-thread-after-failure",
      conversation: {{ id: "prewarm-thread-after-failure", cwd: "C:/workspace/after-failure" }},
    }},
  ]);

  const savedLocation = {{ href: location.href, pathname: location.pathname }};
  location.href = "https://codex.test/thread/prewarm-thread-current";
  location.pathname = "/thread/prewarm-thread-current";
  let currentThreadResumeCalls = 0;
  const currentThreadSummary = await prewarmApi.runQueue({{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      currentThreadResumeCalls += 1;
    }},
  }}, [{{
    type: "full",
    threadId: "prewarm-thread-current",
    conversation: {{ id: "prewarm-thread-current", cwd: "C:/workspace/current" }},
  }}]);
  location.href = savedLocation.href;
  location.pathname = savedLocation.pathname;

  document.visibilityState = "hidden";
  let hiddenResumeCalls = 0;
  const hiddenQueueSummary = await prewarmApi.runQueue({{
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      hiddenResumeCalls += 1;
    }},
  }}, [{{
    type: "full",
    threadId: "prewarm-thread-hidden",
    conversation: {{ id: "prewarm-thread-hidden", cwd: "C:/workspace/hidden" }},
  }}]);
  document.visibilityState = "visible";

  prewarmApi.setManager({{}});
  const managerDiscoveryNeededWithManager = prewarmApi.managerDiscoveryNeeded();
  prewarmApi.setManager(null);
  const managerDiscoveryNeededWithoutManager = prewarmApi.managerDiscoveryNeeded();

  delete window.__codexSessionPrewarmCompletedSignature;
  prewarmApi.clearRetryCounts();
  const retryManager = {{
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-run-failure",
        cwd: "C:/workspace/run-failure",
        updatedAt: prewarmNow,
      }}];
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      throw new Error("retryable run failure");
    }},
  }};
  prewarmApi.setManager(retryManager);
  await prewarmApi.run();
  const failedRunCompletedSignature = prewarmApi.completedSignature();
  const failedRunRetryCounts = prewarmApi.retryCounts();
  prewarmApi.clearScheduledRun();
  prewarmApi.clearRetryCounts();
  prewarmApi.setManager(null);

  delete window.__codexSessionPrewarmCompletedSignature;
  const noTasksManager = {{
    getRecentConversations() {{
      return [];
    }},
  }};
  prewarmApi.setManager(noTasksManager);
  await prewarmApi.run();
  const noTasksRetryCounts = prewarmApi.retryCounts();
  prewarmApi.clearScheduledRun();
  prewarmApi.clearRetryCounts();
  prewarmApi.setManager(null);

  delete window.__codexSessionPrewarmCompletedSignature;
  let expiredResumeCalls = 0;
  const expiredOnlyManager = {{
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-expired-only",
        cwd: "C:/workspace/expired-only",
        updatedAt: prewarmNow - ((24 * 60 * 60 * 1000) + 1),
      }}];
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      expiredResumeCalls += 1;
    }},
  }};
  prewarmApi.setManager(expiredOnlyManager);
  await prewarmApi.run();
  const expiredCompletedSignature = prewarmApi.completedSignature();
  const expiredRetryCounts = prewarmApi.retryCounts();
  prewarmApi.clearScheduledRun();
  prewarmApi.clearRetryCounts();
  prewarmApi.setManager(null);

  delete window.__codexSessionPrewarmCompletedSignature;
  let refreshTimeoutResumeCalls = 0;
  let refreshStartedAfterResumeCompleted = false;
  let refreshResumeCompleted = false;
  const refreshTimeoutManager = {{
    async refreshRecentConversations() {{
      refreshStartedAfterResumeCompleted = refreshResumeCompleted;
      await new Promise(() => {{}});
    }},
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-refresh-timeout",
        cwd: "C:/workspace/refresh-timeout",
        updatedAt: prewarmNow,
      }}];
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      refreshTimeoutResumeCalls += 1;
      refreshResumeCompleted = true;
    }},
  }};
  prewarmApi.setManager(refreshTimeoutManager);
  const refreshTimeoutStartedAt = Date.now();
  await prewarmApi.run(5);
  const refreshTimeoutDurationMs = Date.now() - refreshTimeoutStartedAt;
  const refreshTimeoutStatus = (
    await prewarmApi.refreshRecent(refreshTimeoutManager, 5)
  ).status;
  prewarmApi.clearScheduledRun();
  prewarmApi.setManager(null);

  delete window.__codexSessionPrewarmCompletedSignature;
  let releasePendingManagerRun;
  const pendingManagerGate = new Promise((resolve) => {{
    releasePendingManagerRun = resolve;
  }});
  let firstManagerResumeCalls = 0;
  const firstPendingManager = {{
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-manager-a",
        cwd: "C:/workspace/manager-a",
        updatedAt: prewarmNow,
      }}];
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      firstManagerResumeCalls += 1;
      await pendingManagerGate;
    }},
  }};
  let secondManagerResumeCalls = 0;
  const secondPendingManager = {{
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-manager-b",
        cwd: "C:/workspace/manager-b",
        updatedAt: prewarmNow,
      }}];
    }},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      secondManagerResumeCalls += 1;
    }},
  }};
  prewarmApi.setManager(firstPendingManager);
  const firstManagerRun = prewarmApi.run();
  while (firstManagerResumeCalls === 0) {{
    await new Promise((resolve) => setTimeout(resolve, 0));
  }}
  prewarmApi.setManager(secondPendingManager);
  const joinedManagerRun = prewarmApi.run();
  releasePendingManagerRun();
  await Promise.all([firstManagerRun, joinedManagerRun]);
  await new Promise((resolve) => setTimeout(resolve, 20));
  prewarmApi.clearScheduledRun();
  prewarmApi.setManager(null);

  const emptyWorkspaceRoots = prewarmApi.resumeParams({{
    type: "full",
    threadId: "prewarm-thread-no-cwd",
    conversation: {{ id: "prewarm-thread-no-cwd", cwd: "" }},
  }}).workspaceRoots;
  window.__codexSessionPrewarmCompletedSignature = "completed-before-restart";
  const previousLaunchCycle = String(window.__CODEX_ELVES_LAUNCH_CYCLE__ || "launch-cycle");
  window.__CODEX_ELVES_LAUNCH_CYCLE__ = `${{previousLaunchCycle}}-restart`;
  const launchCycleReset = prewarmApi.resetLaunchCycle();
  const completedSignatureAfterLaunchCycleReset = prewarmApi.completedSignature();
  const launchCycleResetRepeated = prewarmApi.resetLaunchCycle();

  prewarmApi.clearScheduledRun();
  prewarmApi.setManager(null);
  let coldStartManagerLoadCalls = 0;
  let coldStartResumeCalls = 0;
  const coldStartManager = {{
    getHostId() {{
      return "local";
    }},
    getRecentConversations() {{
      return [{{
        id: "prewarm-thread-cold-start",
        cwd: "C:/workspace/cold-start",
        updatedAt: Date.now(),
      }}];
    }},
    async sendRequest() {{}},
    async hydrateBackgroundThreads() {{}},
    needsResume() {{
      return true;
    }},
    async resumeConversationForUnavailableOwner() {{
      coldStartResumeCalls += 1;
    }},
  }};
  api.setModuleLoader(async (namePart) => {{
    if (namePart !== "app-server-manager-signals-") {{
      throw new Error(`unexpected prewarm module: ${{namePart}}`);
    }}
    coldStartManagerLoadCalls += 1;
    return {{ coldStartManager }};
  }});
  await api.applyBackendSettings({{
    launchMode: "patch",
    enhancementsEnabled: true,
    providerSyncEnabled: false,
    codexAppSessionPrewarmEnabled: true,
    codexAppSessionPrewarmFullCount: 1,
    codexAppSessionPrewarmContentCount: 0,
    codexAppSessionPrewarmConcurrency: 1,
    codexAppServiceTierControls: false,
    codexAppTokenUsage: false,
    codexAppProjectMove: false,
  }}, "settings-loaded");
  await waitForCondition(() => coldStartResumeCalls === 1);
  const coldStartManagerReady = prewarmApi.managerReady();
  prewarmApi.clearScheduledRun();
  prewarmApi.setManager(null);
  api.setModuleLoader(null);

  return {{
    defaultPrewarmSettings,
    nestedManagerFound,
    nestedManagerHasResume,
    nestedManagerScanned,
    latestManagerFound,
    latestManagerScanned,
    taskTypes: tasks.map((task) => task.type),
    taskIds: tasks.map((task) => task.threadId),
    subagentFilterTaskIds: subagentFilterTasks.map((task) => task.threadId),
    ageFilterTaskIds: ageFilterTasks.map((task) => task.threadId),
    indicatorActiveAttributes,
    indicatorReusedAttributes,
    indicatorClearedAttributes,
    completed: queueSummary.completed,
    failed: queueSummary.failed,
    maxActiveResumes,
    maxActiveIndicators,
    activeIndicatorsAfterQueue,
    limitedMaxActiveResumes,
    limitedConcurrencySummary,
    zeroConcurrencyResumeCalls,
    zeroConcurrencySummary,
    resumeCalls,
    unsubscribeCalls,
    duplicateResumeCalls,
    promotedResumeCalls,
    promotedHydrationCalls,
    promotedUnsubscribeCalls,
    promotedResult,
    promotedIndicatorActiveDuring,
    promotedIndicatorActiveAfter,
    promotedForegroundRetained,
    fallbackResumeCalls,
    fallbackHydrationCalls,
    fallbackResult,
    phasedSummary,
    phasedHydrationOptions,
    phasedResumeIds,
    phasedContentCompletedBeforeOwner,
    failureCalls,
    failureSummary,
    currentThreadResumeCalls,
    currentThreadSummary,
    hiddenResumeCalls,
    hiddenQueueSummary,
    managerDiscoveryNeededWithManager,
    managerDiscoveryNeededWithoutManager,
    failedRunCompletedSignature,
    failedRunRetryCounts,
    noTasksRetryCounts,
    expiredResumeCalls,
    expiredCompletedSignature,
    expiredRetryCounts,
    firstManagerResumeCalls,
    secondManagerResumeCalls,
    emptyWorkspaceRoots,
    launchCycleReset,
    launchCycleResetRepeated,
    completedSignatureAfterLaunchCycleReset,
    refreshTimeoutStatus,
    refreshTimeoutResumeCalls,
    refreshStartedAfterResumeCompleted,
    refreshTimeoutDurationMs,
    coldStartManagerReady,
    coldStartManagerLoadCalls,
    coldStartResumeCalls,
  }};
}}

(async () => {{
  const serviceTierRetry = await runServiceTierRetryCase();
  const sessionPrewarm = await runSessionPrewarmCases();
  const pluginMarketplaceRequestClientCase = await runPluginMarketplaceRequestClientCase();
  process.stdout.write(JSON.stringify({{
    supportedFast,
    unsupportedModel,
    turnWithoutModel,
    turnWithoutModelDiagnosticModel,
    customInheritUnsupported,
    startConversation,
    gpt56Fast,
    gpt56EmptyCatalogFast,
    displayNameMatches,
    catalogDrivenFast,
    catalogDrivenBlocked,
    patchedCreateRequest,
    relayModelNames,
    modelVisibilityConfig,
    managerDiscoveryBackoffMs,
    pluginMarketplaceRequestParams,
    pluginMarketplaceRequestClient: pluginMarketplaceRequestClientCase,
    pluginScopedFilters,
    pluginAutoExpandLabels,
    badgeTooltip,
    sessionPrewarm,
    serviceTierRetry,
  }}));
}})().catch((error) => {{
  console.error(error);
  process.exitCode = 1;
}});
"#,
        script_path = serde_json::to_string(&script_path.to_string_lossy().to_string())
            .expect("script path should serialize")
    )
    .expect("harness should be written");
    drop(harness);

    let output = Command::new("node")
        .arg(&harness_path)
        .output()
        .expect("node should run service-tier harness");
    assert!(
        output.status.success(),
        "node harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("harness stdout should be JSON")
}

#[test]
fn injection_script_installs_upstream_branch_dropdown_adapter() {
    let script = assets::injection_script(45221);

    assert!(script.contains("installUpstreamBranchDropdownAdapter"));
    assert!(script.contains("installUpstreamPendingWorktreeDispatcherPatch"));
    assert!(script.contains("data-codex-upstream-branch-option"));
    assert!(script.contains("codexUpstreamBranchSelection"));
    assert!(script.contains("/upstream-worktree/defaults"));
    assert!(script.contains("/upstream-worktree/prepare"));
    assert!(script.contains("injectUpstreamBranchOptions"));
    assert!(script.contains("Upstream"));
    assert!(script.contains("data-base-branch"));
    assert!(script.contains("data-project-id"));
    assert!(script.contains("MutationObserver"));
    assert!(script.contains("upstreamWorktreePayloadFromSelection"));
    assert!(script.contains("readUpstreamBranchSelection"));
    assert!(script.contains("writeUpstreamBranchSelection(null)"));
    assert!(script.contains("currentProjectRepoPathFromSelectedProjectButton"));
    assert!(script.contains("Start new chat in"));
    assert!(script.contains("codexUpstreamProjectContext"));
    assert!(script.contains("rememberStartNewChatProjectContext"));
    assert!(script.contains("currentProjectContextForBranchMenu"));
    assert!(script.contains("remoteProjectContextFromGlobalState"));
    assert!(script.contains("upstreamBranchDefaultsInflight = new Map()"));
    assert!(script.contains("upstreamRemoteBranchDefaultsCacheTtlMs"));
    assert!(script.contains("upstreamBranchDefaultsInflight.delete(cacheKey)"));
    assert!(script.contains("projectId:"));
    assert!(script.contains("data-codex-upstream-branch-selection-label"));
    assert!(script.contains("syncUpstreamBranchTriggerLabel"));
    assert!(script.contains("syncUpstreamBranchMenuSelection"));
    assert!(script.contains("applyUpstreamPendingWorktreeOverride"));
    assert!(script.contains("pending-worktree-create"));
    assert!(script.contains("qualifiedSourceRef"));
    assert!(script.contains("refs/remotes/${remote}/${baseBranch}"));
    assert!(script.contains("startingState: { ...request.startingState, branchName: sourceRef }"));
    assert!(script.contains("data-codex-upstream-branch-check"));
    assert!(script.contains("data-codex-upstream-branch-icon"));
    assert!(script.contains("branchIconSvg"));
    assert!(script.contains("checkmarkSvg"));
    assert!(script.contains("aria-checked"));
    assert!(script.contains("check.removeAttribute(\"hidden\")"));
    assert!(script.contains("check.setAttribute(\"hidden\", \"\")"));
    assert!(script.contains("handleNativeBranchSelection"));
    assert!(script.contains("clearUpstreamBranchTriggerLabel"));
    assert!(!script.contains(r#"text.includes("/")"#));
    assert!(script.contains("newWorktreeModeActive"));
    assert!(script.contains("effectiveElementRect"));
    assert!(script.contains("removeUpstreamBranchOptions"));
    assert!(script.contains("cleanupInvalidUpstreamBranchOptions"));
    assert!(script.contains("branchMenuInNewWorktreeMode"));
    assert!(script.contains("branchMenuTriggerIsBranchControl"));
    assert!(script.contains("actual-upstream-refs-v16"));
    assert!(script.contains("create and checkout new branch"));
    assert!(script.contains("if (/^start in"));
    assert!(script.contains("if (!branchMenuInNewWorktreeMode(trigger))"));
}

#[test]
fn injection_script_prevents_switching_to_branches_used_by_other_worktrees() {
    let script = assets::injection_script(45221);

    assert!(script.contains("data-codex-branch-worktree-path"));
    assert!(script.contains("annotateBranchMenuWorktreeUsage"));
    assert!(script.contains("branchWorktreePathFromMenuItem"));
    assert!(script.contains("该分支已在另一个 worktree 使用"));
    assert!(script.contains("event.stopImmediatePropagation?.()"));
}

#[test]
fn injection_script_rebuilds_upstream_options_for_each_project_branch_menu() {
    let script = assets::injection_script(45221);

    assert!(script.contains("projectContextFromProjectLabel"));
    assert!(script.contains("upstreamBranchOptionsMatchRefs"));
    assert!(script.contains("upstreamBranchDefaultsCache = new Map()"));
    assert!(script.contains("actual-upstream-refs-v16"));
}

#[test]
fn manager_ui_exposes_pure_api_relay_mode_button() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let source =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/App.tsx")).unwrap();
    let commands =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src-tauri/src/lib.rs"))
            .unwrap();

    assert!(source.contains("官方混入 API Key"));
    assert!(source.contains("纯 API"));
    assert!(source.contains("apply_pure_api_injection"));
    assert!(commands.contains("commands::apply_pure_api_injection"));
}

#[test]
fn manager_ui_exposes_session_prewarm_performance_controls() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let source =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/App.tsx")).unwrap();
    let styles =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/styles.css")).unwrap();

    assert!(source.contains("codexAppSessionPrewarmFullCount: 3"));
    assert!(source.contains("codexAppSessionPrewarmContentCount: 3"));
    assert!(source.contains("codexAppSessionPrewarmConcurrency: 4"));
    assert!(source.contains("className=\"session-runtime-settings\""));
    assert!(
        source.contains("className=\"session-runtime-setting session-context-compaction-setting\"")
    );
    assert!(styles.contains(".session-runtime-setting"));
    assert!(source.contains("<Field label=\"并发数\">"));
    assert!(source.contains("1-4，数值越高同时预热的会话越多。"));
    assert!(source.contains("0-6，仅加载会话内容，不获取 Owner。"));
    assert!(source.contains("可能出现短暂卡顿"));
    assert!(styles.contains("grid-template-columns: repeat(3, minmax(0, 1fr));"));
    let full_field = source
        .split("<Field label=\"完整恢复数量\">")
        .nth(1)
        .and_then(|value| value.split("</Field>").next())
        .expect("full prewarm field should exist");
    assert!(full_field.contains("min={0}"));
    assert!(full_field.contains("codexAppSessionPrewarmFullCount: clampNumber("));
    let concurrency_field = source
        .split("<Field label=\"并发数\">")
        .nth(1)
        .and_then(|value| value.split("</Field>").next())
        .expect("prewarm concurrency field should exist");
    assert!(concurrency_field.contains("min={1}"));
    assert!(concurrency_field.contains("codexAppSessionPrewarmConcurrency: clampNumber("));
}

#[test]
fn manager_ui_exposes_remote_plugin_marketplace_controls() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let source =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/App.tsx")).unwrap();
    let commands =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src-tauri/src/lib.rs"))
            .unwrap();
    let permissions = std::fs::read_to_string(
        repo.join("apps/codex-elves-manager/src-tauri/permissions/default.toml"),
    )
    .unwrap();

    assert!(source.contains("官方远端插件缓存"));
    assert!(source.contains("释放并注册内置缓存"));
    assert!(source.contains("官方远端插件缓存未释放"));
    assert!(source.contains("官方远端插件缓存候选项"));
    assert!(source.contains("read_remote_context_options"));
    assert!(source.contains("checkRemotePluginMarketplacePrompt"));
    assert!(source.contains("refreshRemoteContextOptions"));
    assert!(source.contains("RemotePluginMarketplacePromptDialog"));
    assert!(source.contains("repair_remote_plugin_marketplace"));
    assert!(source.contains(
        "checked={form.codexAppPluginAutoExpand} disabled={!masterEnabled || !patchMode}"
    ));
    assert!(commands.contains("commands::remote_plugin_marketplace_status"));
    assert!(commands.contains("commands::repair_remote_plugin_marketplace"));
    assert!(commands.contains("commands::read_remote_context_options"));
    assert!(permissions.contains("\"remote_plugin_marketplace_status\""));
    assert!(permissions.contains("\"repair_remote_plugin_marketplace\""));
    assert!(permissions.contains("\"read_remote_context_options\""));
}

#[test]
fn cdp_target_deserializes_websocket_field() {
    let target: CdpTarget = serde_json::from_value(json!({
        "id": "page-1",
        "type": "page",
        "title": "Codex",
        "url": "https://codex.test",
        "webSocketDebuggerUrl": "ws://debug",
    }))
    .expect("target should deserialize");

    assert_eq!(target.target_type, "page");
    assert_eq!(
        target.web_socket_debugger_url.as_deref(),
        Some("ws://debug")
    );
}

#[test]
fn runtime_evaluate_params_sets_expected_flags() {
    let params = bridge::runtime_evaluate_params("1 + 1");

    assert_eq!(params["expression"], "1 + 1");
    assert_eq!(params["awaitPromise"], false);
    assert_eq!(params["allowUnsafeEvalBlockedByCSP"], true);
}

#[test]
fn runtime_evaluate_params_can_await_promise_for_bridge_health_checks() {
    let params = bridge::runtime_evaluate_params_with_await_promise("Promise.resolve(true)", true);

    assert_eq!(params["expression"], "Promise.resolve(true)");
    assert_eq!(params["awaitPromise"], true);
    assert_eq!(params["allowUnsafeEvalBlockedByCSP"], true);
}

#[test]
fn bridge_health_check_script_uses_real_backend_round_trip() {
    let script = bridge::bridge_health_check_script();

    assert!(script.contains("__codexSessionDeleteBridge"));
    assert!(script.contains("/backend/status"));
    assert!(script.contains("Promise.race"));
    assert!(script.contains("setTimeout"));
}

#[test]
fn bridge_result_expressions_json_escape_inputs() {
    let resolve = bridge::resolve_bridge_expression("request\"1", &json!({"status": "ok"}))
        .expect("resolve expression should build");
    let reject = bridge::reject_bridge_expression("request\"1", "bad \"value\"")
        .expect("reject expression should build");

    assert_eq!(
        resolve,
        r#"window.__codexSessionDeleteResolve("request\"1", {"status":"ok"})"#
    );
    assert_eq!(
        reject,
        r#"window.__codexSessionDeleteReject("request\"1", "bad \"value\"")"#
    );
}

#[test]
fn pick_page_target_prefers_codex_title_or_url() {
    let targets = vec![
        target(
            "first",
            "page",
            "Other",
            "https://example.test",
            Some("ws://first"),
        ),
        target(
            "second",
            "page",
            "Codex",
            "https://example.test",
            Some("ws://second"),
        ),
        target(
            "third",
            "page",
            "Other",
            "https://codex.test",
            Some("ws://third"),
        ),
    ];

    let picked = pick_page_target(&targets).expect("target should be selected");

    assert_eq!(picked.id, "second");
}

#[test]
fn pick_page_target_accepts_renamed_chatgpt_shell() {
    let targets = vec![
        target(
            "first",
            "page",
            "Other",
            "https://example.test",
            Some("ws://first"),
        ),
        target(
            "chatgpt",
            "page",
            "ChatGPT",
            "app://-/index.html",
            Some("ws://chatgpt"),
        ),
    ];

    let picked = pick_injectable_codex_page_target(&targets)
        .expect("renamed ChatGPT shell should be selected");

    assert_eq!(picked.id, "chatgpt");
}

#[test]
fn pick_page_target_accepts_app_shell_when_title_changes() {
    let targets = vec![target(
        "app-shell",
        "page",
        "OpenAI",
        "app://-/index.html",
        Some("ws://app-shell"),
    )];

    let picked = pick_injectable_codex_page_target(&targets).expect("app shell should be selected");

    assert_eq!(picked.id, "app-shell");
}

#[test]
fn pick_page_target_prefers_explicit_workspace_over_generic_app_shell() {
    let targets = vec![
        target(
            "generic-shell",
            "page",
            "OpenAI",
            "app://-/background.html",
            Some("ws://generic-shell"),
        ),
        target(
            "workspace",
            "page",
            "Codex",
            "app://-/index.html",
            Some("ws://workspace"),
        ),
    ];

    let picked =
        pick_injectable_codex_page_target(&targets).expect("explicit workspace target should win");

    assert_eq!(picked.id, "workspace");
}

#[test]
fn pick_page_target_leniently_falls_back_to_first_injectable_page() {
    let targets = vec![
        target(
            "browser",
            "browser",
            "Codex",
            "https://codex.test",
            Some("ws://browser"),
        ),
        target(
            "first",
            "page",
            "Other",
            "https://example.test",
            Some("ws://first"),
        ),
        target(
            "second",
            "page",
            "Other 2",
            "https://example.test/2",
            Some("ws://second"),
        ),
    ];

    let picked = pick_page_target(&targets).expect("target should be selected");

    assert_eq!(picked.id, "first");
}

#[test]
fn pick_page_target_rejects_non_pages_and_pages_without_websocket() {
    let targets = vec![
        target(
            "browser",
            "browser",
            "Codex",
            "https://codex.test",
            Some("ws://browser"),
        ),
        target("page-no-ws", "page", "Codex", "https://codex.test", None),
    ];

    let error = pick_page_target(&targets).expect_err("no injectable page should be selected");

    assert!(
        error
            .to_string()
            .contains("No injectable page target found")
    );
}

#[test]
fn pick_injectable_codex_page_target_rejects_non_codex_pages() {
    let targets = vec![
        target(
            "browser",
            "browser",
            "Codex",
            "https://codex.test",
            Some("ws://browser"),
        ),
        target(
            "other-page",
            "page",
            "Other App",
            "https://example.test",
            Some("ws://other"),
        ),
    ];

    let error = pick_injectable_codex_page_target(&targets)
        .expect_err("non-Codex page must not be selected for injection");

    assert!(
        error
            .to_string()
            .contains("No injectable ChatGPT/Codex page target found")
    );
}

#[test]
fn pick_injectable_codex_page_target_requires_websocket() {
    let targets = vec![target("codex", "page", "Codex", "https://codex.test", None)];

    let error = pick_injectable_codex_page_target(&targets)
        .expect_err("Codex page without websocket must not be selected for injection");

    assert!(
        error
            .to_string()
            .contains("No injectable ChatGPT/Codex page target found")
    );
}

#[tokio::test]
async fn list_targets_can_query_ipv6_loopback_cdp_endpoint() {
    let listener = TcpListener::bind("[::1]:0")
        .await
        .expect("IPv6 loopback listener should bind");
    let port = listener.local_addr().unwrap().port();
    let body = serde_json::to_vec(&json!([
        {
            "id": "page-1",
            "type": "page",
            "title": "Codex",
            "url": "app://-/index.html",
            "webSocketDebuggerUrl": format!("ws://[::1]:{port}/devtools/page/page-1"),
        }
    ]))
    .unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("request should arrive");
        let mut request = [0_u8; 1024];
        let _ = stream.readable().await;
        let _ = stream.try_read(&mut request);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .try_write(response.as_bytes())
            .expect("response headers should write");
        stream.try_write(&body).expect("response body should write");
    });

    let targets = list_targets(port)
        .await
        .expect("CDP target query should fall back to IPv6 loopback");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "page-1");
    server.await.expect("server task should complete");
}

#[tokio::test]
async fn install_bridge_routes_binding_while_waiting_for_command_response() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("codex-elves.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=4 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        let evaluate = recv_json(&mut socket).await;
        assert_eq!(evaluate["id"], 5);
        assert_eq!(evaluate["method"], "Runtime.evaluate");
        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": {
                    "payload": serde_json::to_string(&json!({
                        "id": "request-1",
                        "path": "delete",
                        "payload": { "target": "session" },
                    })).unwrap(),
                },
            }),
        )
        .await;
        send_json(&mut socket, json!({ "id": 5, "result": {} })).await;

        let response = recv_json(&mut socket).await;
        assert_eq!(response["method"], "Runtime.evaluate");
        assert!(
            response["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("__codexSessionDeleteResolve")
        );
        send_json(&mut socket, json!({ "id": response["id"], "result": {} })).await;
        close_socket(&mut socket).await;
    })
    .await;

    let handled = Arc::new(AtomicBool::new(false));
    let handler = {
        let handled = Arc::clone(&handled);
        Arc::new(move |path: String, payload: serde_json::Value| {
            let handled = Arc::clone(&handled);
            Box::pin(async move {
                assert_eq!(path, "delete");
                assert_eq!(payload["target"], "session");
                handled.store(true, Ordering::SeqCst);
                Ok(json!({ "status": "ok" }))
            })
                as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
        })
    };

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(&url, BRIDGE_BINDING_NAME, handler, &[]),
    )
    .await
    .expect("bridge should not hang while processing interleaved binding call")
    .expect("bridge should keep processing interleaved binding call");
    request_rx
        .await
        .expect("server task should finish without panicking");
    assert!(handled.load(Ordering::SeqCst));
    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(contents.contains("bridge.resolve_start"));
    assert!(contents.contains("bridge.resolve_ok"));
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
}

#[tokio::test]
async fn install_bridge_immediately_evaluates_new_document_scripts() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        let add_main = recv_json(&mut socket).await;
        assert_eq!(add_main["method"], "Page.addScriptToEvaluateOnNewDocument");
        assert_eq!(add_main["params"]["source"], "window.mainInjected = true;");
        send_json(&mut socket, json!({ "id": add_main["id"], "result": {} })).await;

        let eval_main = recv_json(&mut socket).await;
        assert_eq!(eval_main["method"], "Runtime.evaluate");
        assert_eq!(
            eval_main["params"]["expression"],
            "window.mainInjected = true;"
        );
        send_json(&mut socket, json!({ "id": eval_main["id"], "result": {} })).await;

        let add_user = recv_json(&mut socket).await;
        assert_eq!(add_user["method"], "Page.addScriptToEvaluateOnNewDocument");
        assert_eq!(add_user["params"]["source"], "window.userInjected = true;");
        send_json(&mut socket, json!({ "id": add_user["id"], "result": {} })).await;

        let eval_user = recv_json(&mut socket).await;
        assert_eq!(eval_user["method"], "Runtime.evaluate");
        assert_eq!(
            eval_user["params"]["expression"],
            "window.userInjected = true;"
        );
        send_json(&mut socket, json!({ "id": eval_user["id"], "result": {} })).await;

        close_socket(&mut socket).await;
    })
    .await;

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(
            &url,
            BRIDGE_BINDING_NAME,
            noop_handler(),
            &[
                "window.mainInjected = true;".to_string(),
                "window.userInjected = true;".to_string(),
            ],
        ),
    )
    .await
    .expect("bridge should not hang while evaluating new document scripts")
    .expect("bridge should evaluate new document scripts immediately");
    request_rx
        .await
        .expect("server task should finish without panicking");
}

#[tokio::test]
async fn install_bridge_returns_after_installing_and_keeps_message_pump_alive() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        let add_script = recv_json(&mut socket).await;
        assert_eq!(
            add_script["method"],
            "Page.addScriptToEvaluateOnNewDocument"
        );
        send_json(&mut socket, json!({ "id": add_script["id"], "result": {} })).await;

        let eval_script = recv_json(&mut socket).await;
        assert_eq!(eval_script["method"], "Runtime.evaluate");
        send_json(
            &mut socket,
            json!({ "id": eval_script["id"], "result": {} }),
        )
        .await;

        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": {
                    "payload": serde_json::to_string(&json!({
                        "id": "after-return",
                        "path": "status",
                        "payload": {},
                    })).unwrap(),
                },
            }),
        )
        .await;

        let resolve = recv_json(&mut socket).await;
        assert!(
            resolve["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("after-return")
        );
        send_json(&mut socket, json!({ "id": resolve["id"], "result": {} })).await;
        close_socket(&mut socket).await;
    })
    .await;

    let handled = Arc::new(AtomicBool::new(false));
    let handler = {
        let handled = Arc::clone(&handled);
        Arc::new(move |_path: String, _payload: serde_json::Value| {
            let handled = Arc::clone(&handled);
            Box::pin(async move {
                handled.store(true, Ordering::SeqCst);
                Ok(json!({ "status": "ok" }))
            })
                as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
        })
    };

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(
            &url,
            BRIDGE_BINDING_NAME,
            handler,
            &["window.ready = true;".to_string()],
        ),
    )
    .await
    .expect("bridge install should return after setup")
    .expect("bridge install should succeed");

    request_rx
        .await
        .expect("server task should finish without panicking");
    assert!(handled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn install_bridge_command_error_mentions_method_and_id() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        let command = recv_json(&mut socket).await;
        assert_eq!(command["method"], "Runtime.enable");
        send_json(
            &mut socket,
            json!({
                "id": command["id"],
                "error": { "code": -32000, "message": "Runtime disabled" },
            }),
        )
        .await;
        close_socket(&mut socket).await;
    })
    .await;

    let handler = noop_handler();
    let error = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(&url, BRIDGE_BINDING_NAME, handler, &[]),
    )
    .await
    .expect("bridge should not hang on CDP error response")
    .expect_err("CDP error response should fail install");
    let message = error.to_string();

    request_rx
        .await
        .expect("server task should finish without panicking");
    assert!(message.contains("Runtime.enable"), "{message}");
    assert!(message.contains("id 1"), "{message}");
    assert!(message.contains("Runtime disabled"), "{message}");
}

#[tokio::test]
async fn install_bridge_rejects_bad_payload_with_id_and_continues_after_unparseable_payload() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": { "payload": "{\"id\":\"bad-1\",\"payload\":{}" },
            }),
        )
        .await;
        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": { "payload": "not json" },
            }),
        )
        .await;
        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": {
                    "payload": serde_json::to_string(&json!({
                        "id": "ok-1",
                        "path": "delete",
                        "payload": {},
                    })).unwrap(),
                },
            }),
        )
        .await;

        let reject = recv_json(&mut socket).await;
        assert!(
            reject["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("__codexSessionDeleteReject")
        );
        assert!(
            reject["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("bad-1")
        );
        send_json(&mut socket, json!({ "id": reject["id"], "result": {} })).await;

        let resolve = recv_json(&mut socket).await;
        assert!(
            resolve["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("__codexSessionDeleteResolve")
        );
        assert!(
            resolve["params"]["expression"]
                .as_str()
                .expect("expression should be string")
                .contains("ok-1")
        );
        send_json(&mut socket, json!({ "id": resolve["id"], "result": {} })).await;
        close_socket(&mut socket).await;
    })
    .await;

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(&url, BRIDGE_BINDING_NAME, noop_handler(), &[]),
    )
    .await
    .expect("bridge should not hang after bad payload")
    .expect("bad payloads should not terminate the bridge loop");
    request_rx
        .await
        .expect("server task should finish without panicking");
}

#[tokio::test]
async fn install_bridge_queues_consecutive_bindings_without_recursive_dispatch() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        for request_id in ["first", "second", "third"] {
            send_json(
                &mut socket,
                json!({
                    "method": "Runtime.bindingCalled",
                    "params": {
                        "payload": serde_json::to_string(&json!({
                            "id": request_id,
                            "path": "delete",
                            "payload": { "request": request_id },
                        })).unwrap(),
                    },
                }),
            )
            .await;
        }

        let first = recv_json(&mut socket).await;
        assert_eq!(first["method"], "Runtime.evaluate");
        assert_expression_contains_request(&first, "first");
        let second = recv_json(&mut socket).await;
        assert_eq!(second["method"], "Runtime.evaluate");
        assert_expression_contains_request(&second, "second");
        assert_ne!(second["id"], first["id"]);

        let third = recv_json(&mut socket).await;
        assert_eq!(third["method"], "Runtime.evaluate");
        assert_expression_contains_request(&third, "third");
        assert_ne!(third["id"], first["id"]);
        assert_ne!(third["id"], second["id"]);

        close_socket(&mut socket).await;
    })
    .await;

    let handler = Arc::new(|_path: String, payload: serde_json::Value| {
        Box::pin(async move { Ok(json!({ "status": "ok", "request": payload["request"] })) })
            as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
    });

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(&url, BRIDGE_BINDING_NAME, handler, &[]),
    )
    .await
    .expect("bridge should not hang while draining queued binding calls")
    .expect("bridge should process queued binding calls");
    request_rx
        .await
        .expect("server task should finish without panicking");
}

#[tokio::test]
async fn install_bridge_does_not_block_fast_request_behind_slow_handler() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        for (request_id, delay_ms) in [("slow", 250_u64), ("fast", 0_u64)] {
            send_json(
                &mut socket,
                json!({
                    "method": "Runtime.bindingCalled",
                    "params": {
                        "payload": serde_json::to_string(&json!({
                            "id": request_id,
                            "path": "/backend/test",
                            "payload": { "delayMs": delay_ms },
                        })).unwrap(),
                    },
                }),
            )
            .await;
        }

        let first = tokio::time::timeout(Duration::from_millis(150), recv_json(&mut socket))
            .await
            .expect("fast request should resolve before slow handler finishes");
        assert_expression_contains_request(&first, "fast");

        let second = tokio::time::timeout(Duration::from_millis(500), recv_json(&mut socket))
            .await
            .expect("slow request should eventually resolve");
        assert_expression_contains_request(&second, "slow");
        close_socket(&mut socket).await;
    })
    .await;

    let handler = Arc::new(|_path: String, payload: serde_json::Value| {
        Box::pin(async move {
            let delay_ms = payload["delayMs"].as_u64().unwrap_or_default();
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            Ok(json!({ "status": "ok" }))
        }) as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
    });

    let _runtime = bridge::install_bridge(&url, BRIDGE_BINDING_NAME, handler, &[])
        .await
        .expect("bridge should install");
    request_rx
        .await
        .expect("server task should finish without panicking");
}

#[tokio::test]
async fn install_bridge_does_not_wait_for_resolve_runtime_evaluate_ack() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=5 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": {
                    "payload": serde_json::to_string(&json!({
                        "id": "first",
                        "path": "/backend/status",
                        "payload": {},
                    })).unwrap(),
                },
            }),
        )
        .await;
        let first_resolve = recv_json(&mut socket).await;
        assert_eq!(first_resolve["method"], "Runtime.evaluate");
        assert_expression_contains_request(&first_resolve, "first");

        send_json(
            &mut socket,
            json!({
                "method": "Runtime.bindingCalled",
                "params": {
                    "payload": serde_json::to_string(&json!({
                        "id": "second",
                        "path": "/backend/status",
                        "payload": {},
                    })).unwrap(),
                },
            }),
        )
        .await;
        let second_resolve =
            tokio::time::timeout(Duration::from_millis(500), recv_json(&mut socket))
                .await
                .expect(
                    "second resolve should be sent without waiting for first Runtime.evaluate ack",
                );
        assert_eq!(second_resolve["method"], "Runtime.evaluate");
        assert_expression_contains_request(&second_resolve, "second");
        close_socket(&mut socket).await;
    })
    .await;

    let handler = Arc::new(|_path: String, _payload: serde_json::Value| {
        Box::pin(async { Ok(json!({ "status": "ok" })) })
            as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
    });

    let _runtime = tokio::time::timeout(
        Duration::from_secs(2),
        bridge::install_bridge(&url, BRIDGE_BINDING_NAME, handler, &[]),
    )
    .await
    .expect("bridge install should not wait for resolve ack")
    .expect("bridge install should survive missing resolve ack");
    request_rx
        .await
        .expect("server task should finish without panicking");
}

#[tokio::test]
async fn bridge_runtime_shutdown_removes_registered_scripts_and_binding() {
    let (url, request_rx) = spawn_cdp_server(|mut socket| async move {
        for expected_id in 1..=3 {
            let command = recv_json(&mut socket).await;
            assert_eq!(command["id"], expected_id);
            send_json(&mut socket, json!({ "id": expected_id, "result": {} })).await;
        }

        let add_bridge = recv_json(&mut socket).await;
        assert_eq!(
            add_bridge["method"],
            "Page.addScriptToEvaluateOnNewDocument"
        );
        send_json(
            &mut socket,
            json!({
                "id": add_bridge["id"],
                "result": { "identifier": "bridge-script" },
            }),
        )
        .await;

        let eval_bridge = recv_json(&mut socket).await;
        assert_eq!(eval_bridge["method"], "Runtime.evaluate");
        send_json(
            &mut socket,
            json!({ "id": eval_bridge["id"], "result": {} }),
        )
        .await;

        let add_feature = recv_json(&mut socket).await;
        assert_eq!(
            add_feature["method"],
            "Page.addScriptToEvaluateOnNewDocument"
        );
        send_json(
            &mut socket,
            json!({
                "id": add_feature["id"],
                "result": { "identifier": "feature-script" },
            }),
        )
        .await;

        let eval_feature = recv_json(&mut socket).await;
        assert_eq!(eval_feature["method"], "Runtime.evaluate");
        send_json(
            &mut socket,
            json!({ "id": eval_feature["id"], "result": {} }),
        )
        .await;

        let remove_bridge = recv_json(&mut socket).await;
        assert_eq!(
            remove_bridge["method"],
            "Page.removeScriptToEvaluateOnNewDocument"
        );
        assert_eq!(remove_bridge["params"]["identifier"], "bridge-script");

        let remove_feature = recv_json(&mut socket).await;
        assert_eq!(
            remove_feature["method"],
            "Page.removeScriptToEvaluateOnNewDocument"
        );
        assert_eq!(remove_feature["params"]["identifier"], "feature-script");

        let ownership_check = recv_json(&mut socket).await;
        assert_eq!(ownership_check["method"], "Runtime.evaluate");
        assert!(
            ownership_check["params"]["expression"]
                .as_str()
                .expect("ownership expression should be string")
                .contains("__codexSessionDeleteBridgeGeneration")
        );
        assert!(
            ownership_check["params"]["expression"]
                .as_str()
                .expect("ownership expression should be string")
                .contains("CodexElves Bridge 已重启，请重试")
        );
        send_json(
            &mut socket,
            json!({
                "id": ownership_check["id"],
                "result": { "result": { "value": true } },
            }),
        )
        .await;

        let remove_binding = recv_json(&mut socket).await;
        assert_eq!(remove_binding["method"], "Runtime.removeBinding");
        assert_eq!(remove_binding["params"]["name"], BRIDGE_BINDING_NAME);
        close_socket(&mut socket).await;
    })
    .await;

    let runtime = bridge::install_bridge(
        &url,
        BRIDGE_BINDING_NAME,
        noop_handler(),
        &["window.featureInjected = true;".to_string()],
    )
    .await
    .expect("bridge should install");
    runtime.shutdown().await;

    request_rx
        .await
        .expect("server task should observe bridge cleanup");
}

type TestSocket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn spawn_cdp_server<F, Fut>(handler: F) -> (String, oneshot::Receiver<()>)
where
    F: FnOnce(TestSocket) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener should bind");
    let address = listener.local_addr().expect("listener should have address");
    let (done_tx, done_rx) = oneshot::channel();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("client should connect");
        let socket = accept_async(stream)
            .await
            .expect("websocket should upgrade");
        handler(socket).await;
        let _ = done_tx.send(());
    });

    (websocket_url(address), done_rx)
}

fn websocket_url(address: SocketAddr) -> String {
    format!("ws://{address}")
}

async fn recv_json(socket: &mut TestSocket) -> serde_json::Value {
    let message = socket
        .next()
        .await
        .expect("client should send message")
        .expect("message should be readable");
    let Message::Text(text) = message else {
        panic!("expected text websocket message");
    };
    serde_json::from_str(&text).expect("message should be JSON")
}

async fn send_json(socket: &mut TestSocket, value: serde_json::Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .expect("message should send");
}

fn assert_expression_contains_request(command: &serde_json::Value, request_id: &str) {
    let expression = command["params"]["expression"]
        .as_str()
        .expect("expression should be string");
    assert!(
        expression.contains("__codexSessionDeleteResolve"),
        "{expression}"
    );
    assert!(expression.contains(request_id), "{expression}");
}

async fn close_socket(socket: &mut TestSocket) {
    socket.close(None).await.expect("websocket should close");
    let _ = tokio::time::timeout(Duration::from_millis(200), socket.next()).await;
}

fn noop_handler() -> bridge::BridgeHandler {
    Arc::new(|_, _| {
        Box::pin(async { Ok(json!({ "status": "ok" })) })
            as Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>
    })
}
