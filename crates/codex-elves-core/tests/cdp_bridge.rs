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
fn renderer_features_delete_is_permanent_without_undo_ui() {
    let script = assets::renderer_features_script();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let manager_source =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/App.tsx")).unwrap();

    assert!(!script.contains("postJson(\"/undo\""));
    assert!(!script.contains("result.undo_token"));
    assert!(!script.contains("undo.textContent = \"撤销\""));
    assert!(script.contains("result.status === \"partial\""));
    assert!(manager_source.contains("deleteStatus === \"partial\""));
}

#[test]
fn obsolete_session_backup_cleanup_only_spawns_before_migration_completes() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let launcher =
        std::fs::read_to_string(repo.join("apps/codex-elves-launcher/src/main.rs")).unwrap();
    let manager =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src-tauri/src/lib.rs"))
            .unwrap();
    let guard = "if codex_elves_core::paths::obsolete_session_backup_cleanup_needed()";

    assert!(launcher.contains(guard));
    assert!(manager.contains(guard));
}

#[test]
fn renderer_features_supports_current_app_shell_header_layout() {
    let script = assets::renderer_features_script();

    assert!(script.contains("applicationMenuTopBar: '[class*=\"_ApplicationMenuTopBar_\"]'"));
    assert!(script.contains("function findApplicationMenuTopBar()"));
    assert!(script.contains("menuClassName: codexElvesMenuTitlebarClass"));
    assert!(script.contains("margin-inline-start: auto"));
    assert!(script.contains(
        "#${codexElvesMenuId} .codex-elves-backend-indicator + [data-codex-elves-trigger-label] { margin-inline-start: 4px; }"
    ));
    assert!(script.contains(
        "const applicationMenuTopBar = findApplicationMenuTopBar();\n    const headerRoot"
    ));
    assert!(
        script.contains("appHeader: \"[data-app-shell-application-menu-bar], .app-header-tint\"")
    );
    assert!(script.contains(
        "const contextSurface = header?.querySelector(selectors.headerContextMenuSurface)"
    ));
    assert!(script.contains("Array.from(contextSurface?.children || [])"));
    assert!(script.contains("node.matches?.(selectors.nativeMenuBar)"));
    assert!(script.contains("\"rounded-s-none\""));
    assert!(script.contains("\"border-s-0\""));
    assert!(script.contains("\"ps-0.5\""));
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
fn renderer_task_board_lifecycle_keeps_the_native_main_surface_recoverable() {
    let script = assets::renderer_features_script();

    assert!(script.contains("data-codex-task-board-entry=\"true\""));
    assert!(script.contains("function installTaskBoardEntry()"));
    assert!(script.contains("function activateTaskBoard()"));
    assert!(script.contains("const pluginNavigationControlSelector = ["));
    assert!(script.contains("function pluginEntryControlMatches(control)"));
    assert!(script.contains("globalSemanticMatches.length === 1"));
    assert!(script.contains("main[data-app-shell-main-surface]"));
    assert!(script.contains("data-codex-task-board-root=\"true\""));
    assert!(script.contains("codex-task-board-main-host"));
    assert!(
        script.contains("function destroyTaskBoardRuntime({ preserveNativeCreate = false } = {})")
    );
    assert!(script.contains("__codexElvesTaskBoardNativeOperationLease"));
    assert!(script.contains("preserveNativeCreate: !!taskBoardNativeOperationLease()"));
    assert!(script.contains("taskBoard: true"));
    assert!(script.contains("taskBoard: \"codexAppTaskBoard\""));
    assert!(script.contains("data-codex-elves-setting=\"taskBoard\""));
    assert!(script.contains("function taskBoardFeatureEnabled()"));
    assert!(script.contains("function disableTaskBoardRuntime()"));
    assert!(script.contains("if (!taskBoardFeatureEnabled())"));
    assert!(script.contains("window.__codexElvesTaskBoardCleanup"));
    assert!(script.contains("new ResizeObserver"));
    assert!(script.contains("new MutationObserver"));
}

#[test]
fn renderer_task_board_view_projects_read_only_bridge_snapshots_responsively() {
    let script = assets::renderer_features_script();

    assert!(script.contains("\"/task-board/snapshot\""));
    assert!(script.contains("\"/task-board/session-catalog\""));
    assert!(script.contains("window.__codexElvesTaskBoardMock"));
    assert!(script.contains("新任务"));
    assert!(script.contains("规划中"));
    assert!(script.contains("执行中"));
    assert!(script.contains("验收中"));
    assert!(script.contains("已完成"));
    assert!(script.contains("search.setAttribute(\"aria-label\", \"搜索任务、项目或关联会话\")"));
    assert!(
        script.contains("taskBoardConfigureDropdownTrigger(filter, \"全部项目\", \"筛选项目\")")
    );
    assert!(script.contains(
        "const filter = taskBoardElement(\"button\", \"codex-task-board-project-filter\")"
    ));
    assert!(!script.contains("taskBoardElement(\"select\", \"codex-task-board-project-filter\")"));
    assert!(script.contains("function openTaskBoardProjectMenu(trigger)"));
    assert!(script.contains("function openTaskBoardDropdownMenu({"));
    assert!(script.contains("function openTaskBoardCreateProjectMenu(trigger)"));
    assert!(script.contains("function openTaskBoardCreateStatusMenu(trigger)"));
    assert!(script.contains("const taskBoardProjectDropdownWidth = 320;"));
    assert_eq!(
        script
            .matches("fixedWidth: taskBoardProjectDropdownWidth")
            .count(),
        2
    );
    assert_eq!(script.matches("align: \"start\"").count(), 2);
    assert!(script.contains("const preferredLeft = align === \"start\""));
    assert!(!script.contains("taskBoardElement(\"select\", \"codex-task-board-create-select\")"));
    assert!(script.contains("codex-task-board-dropdown-trigger"));
    assert!(script.contains("codex-task-board-dropdown-menu"));
    assert!(script.contains("const gap = 6"));
    assert!(script.contains("border-radius: 10px"));
    assert!(script.contains("--task-board-action-background"));
    assert!(
        script.contains("--task-board-action-background: var(--color-background-button-secondary,")
    );
    assert!(script.contains("--task-board-action-foreground: var(--color-token-text-primary,"));
    assert!(script.contains(
        "--task-board-action-background-hover: var(--color-background-button-secondary-hover,"
    ));
    assert!(script.contains(
        "--task-board-action-background-active: var(--color-background-button-secondary-active,"
    ));
    assert!(!script.contains("--task-board-action-background: var(--color-token-primary,"));
    assert_eq!(
        script
            .matches("background: var(--task-board-action-background);")
            .count(),
        2
    );
    assert!(!script.contains("background: #ececec;"));
    assert!(script.contains("menuRole = \"listbox\""));
    assert!(script.contains("menu.setAttribute(\"role\", menuRole)"));
    assert!(script.contains("grid-template-rows: auto minmax(180px, 1fr)"));
    assert!(script.contains("min-height: 180px"));
    assert!(script.contains("codex-task-board-empty-column"));
    assert!(script.contains("overflow: auto"));
    assert!(script.contains("@container"));
    assert!(script.contains("taskBoardNativeAdapter.openSession"));
}

#[test]
fn renderer_task_board_review_fixes_keep_reinjection_navigation_and_cleanup_boundaries() {
    let script = assets::renderer_features_script();

    assert!(script.contains("const taskBoardRuntimeVersion ="));
    assert!(
        script.contains(r#"const codexDeleteStyleVersion = "55";"#),
        "task-board layout changes should invalidate the installed renderer stylesheet"
    );
    assert!(script.contains("--codex-confirm-surface: var("));
    assert!(script.contains("--color-background-elevated-primary-opaque,"));
    assert!(script.contains("--color-background-button-secondary-hover,"));
    assert!(script.contains("--color-background-danger-soft,"));
    assert!(script.contains("--color-text-danger-soft,"));
    assert!(script.contains("background: var(--codex-confirm-surface);"));
    assert!(script.contains("color: var(--codex-confirm-muted);"));
    assert!(!script.contains("html.dark .codex-delete-confirm-content"));
    assert!(
        !script
            .contains("html:not(.light):not([data-theme=\"light\"]) .codex-delete-confirm-content")
    );
    assert!(script.contains("taskBoardRuntimeCanRefresh()"));
    assert!(script.contains("window.__codexElvesTaskBoardRuntimeVersion"));
    assert!(script.contains("window.__codexElvesTaskBoardRefreshRuntime"));
    assert!(script.contains("function taskBoardEntryButtons()"));
    assert!(!script.contains("function taskBoardEntryButton()"));
    assert!(script.contains("function reconcileTaskBoardEntry()"));
    assert!(script.contains("entry.remove()"));
    assert!(script.contains("pluginButton.cloneNode(true)"));
    assert!(script.contains("pluginButton.insertAdjacentElement(\"afterend\", entry)"));
    assert!(script.contains("function clearTaskBoardNativeSelection()"));
    assert!(script.contains("function restoreTaskBoardNativeSelection("));
    assert!(script.contains("[data-app-action-sidebar-thread-row]"));
    assert!(script.contains("[data-app-action-sidebar-project-row]"));
    assert!(script.contains("data-app-action-sidebar-thread-selected"));
    assert!(script.contains("data-app-action-sidebar-thread-active"));
    assert!(script.contains("queueMicrotask(() =>"));
    assert!(script.contains("restoreNativeSelection: false"));
    assert!(script.contains("const taskBoardCatalogSessionMapCache = new WeakMap();"));
    assert!(script.contains("const taskBoardTaskSearchTextCache = new WeakMap();"));
    assert!(script.contains("const taskBoardLinkedConversationsCache = new WeakMap();"));
    assert!(!script.contains("function taskBoardCatalogSessionMap()"));
    assert!(script.contains("function taskBoardConversationProjection("));
    assert!(script.contains("function scheduleTaskBoardCardsRender()"));
    assert!(script.contains("const linkedEntries = Array.from(linked.entries());"));
    assert!(script.contains("const taskBoardConversationStatusMaxConcurrency = 4;"));
    assert!(script.contains("function taskBoardMapSettledWithConcurrency("));
    assert!(!script.contains("Array.from(linked.keys())[index]"));
    assert!(script.contains("if (resultsChanged) renderTaskBoardCards();"));
    assert!(script.contains("const tasksByStatus = new Map("));
    assert!(script.contains("目录部分不可用"));
    assert!(script.contains("function taskBoardApplyReadOutcome("));
    assert!(
        !script.contains("const [snapshotOutcome, catalogOutcome] = await Promise.allSettled([")
    );
    assert!(script.contains("position: relative"));
    assert!(script.contains("position: absolute"));
    assert!(script.contains("inset: 0"));
    assert!(script.contains("[data-low-height=\"true\"]"));
    assert!(script.contains("::-webkit-scrollbar-thumb"));
    assert!(script.contains("linked.forEach((conversation) =>"));
    assert!(!script.contains("function taskBoardConversationSummary("));
    assert!(!script.contains("function openTaskBoardConversationPopover("));
    assert!(!script.contains("codex-task-board-conversation-popover"));
    assert!(!script.contains("Debug 原型"));
    assert!(script.contains("拖动任务卡片可切换状态"));
    assert!(script.contains("min-width: 1580px"));
    assert!(script.contains("codex-task-board-card-footer"));
    assert!(script.contains("function taskBoardStatusPresentation("));
    assert!(script.contains(r#"const hint = root.querySelector(".codex-task-board-hint")"#));
    assert!(script.contains(r#"hint.setAttribute("aria-live", "polite")"#));
    assert!(!script.contains(r#"taskBoardElement("p", "codex-task-board-state")"#));
    assert!(!script.contains(r#"root.querySelector(".codex-task-board-state")"#));

    let sidebar_relevance = script
        .split("if (domain === \"sidebar\")")
        .nth(1)
        .and_then(|section| section.split("if (domain === \"header\")").next())
        .expect("sidebar relevance selector should be present");
    assert!(sidebar_relevance.contains("pluginNavigationControlSelector"));
    assert!(sidebar_relevance.contains("taskBoardEntrySelector"));

    let runtime_refresh = script
        .split("function refreshTaskBoardRuntime()")
        .nth(1)
        .and_then(|section| section.split("function destroyTaskBoardRuntime()").next())
        .expect("task board runtime refresh should be present");
    assert!(
        runtime_refresh
            .contains("closeTaskBoardCreateModal();\n    closeTaskBoardDetachDialog({ restoreFocus: false });\n    reconcileTaskBoardRuntime();")
    );
}

#[test]
fn renderer_task_board_preserves_debug_spike_column_and_card_surface_hierarchy() {
    let script = assets::renderer_features_script();
    let board_styles = script
        .split(".codex-task-board-columns {")
        .nth(1)
        .and_then(|section| section.split(".codex-task-board-dropdown-menu {").next())
        .expect("task board column and card styles should be present");
    let conversation_styles = script
        .split(".codex-task-board-conversation {")
        .nth(1)
        .and_then(|section| {
            section
                .split(".codex-task-board-conversation:hover {")
                .next()
        })
        .expect("task board conversation styles should be present");

    assert!(board_styles.contains("border-radius: 10px"));
    assert!(board_styles.contains(
        "background: color-mix(in srgb, var(--task-board-panel-background) 78%, transparent);"
    ));
    assert!(board_styles.contains(".codex-task-board-card {\n        display: grid;"));
    assert!(board_styles.contains("gap: 10px;"));
    assert!(board_styles.contains("border-radius: 9px"));
    assert!(board_styles.contains("background: var(--task-board-card-background);"));
    assert!(board_styles.contains(".codex-task-board-card:hover {"));
    assert!(script.contains(".codex-task-board-conversations {\n        display: grid;"));
    assert!(conversation_styles.contains("min-height: 24px;"));
    assert!(conversation_styles.contains("padding: 0 4px 0 0;"));
}

#[test]
fn renderer_task_board_exposes_conversation_statuses_and_card_level_attach_flow() {
    let script = assets::renderer_features_script();

    assert!(script.contains("\"/task-board/task-conversations-attach\""));
    assert!(script.contains("\"/task-board/task-conversations-detach\""));
    assert!(script.contains("\"/thread-usage-summary\""));
    assert!(script.contains("function taskBoardConversationStatus("));
    assert!(script.contains("function refreshTaskBoardConversationStatuses("));
    assert!(script.contains("已完成 · 未读"));
    assert!(script.contains("data-conversation-status"));
    assert!(script.contains("codex-task-board-conversation-status-indicator"));
    assert!(script.contains("codex-task-board-card-add"));
    assert!(script.contains("codex-task-board-conversation-remove"));
    assert!(script.contains("function openTaskBoardAttachModal("));
    assert!(script.contains("function openTaskBoardDetachDialog("));
    assert!(script.contains("不会删除 Codex 中的原始会话"));
    assert!(script.contains("创建并添加"));
}

#[test]
fn renderer_task_board_dynamic_contracts_apply_latest_catalog_and_independent_reads() {
    let cases = run_task_board_contract_harness();

    assert!(
        !cases["runtimeGate"]["oldRuntimeAccepted"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["runtimeGate"]["currentRuntimeAccepted"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["featureSwitch"]["defaultEnabled"].as_bool().unwrap());
    assert!(
        cases["featureSwitch"]["disabledBySwitch"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["featureSwitch"]["disabledByMaster"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["featureSwitch"]["activeViewClosedWhenDisabled"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["featureSwitch"]["restoredEnabled"].as_bool().unwrap());
    assert_eq!(cases["statusSlot"]["normal"]["status"], "ok");
    assert_eq!(
        cases["statusSlot"]["normal"]["text"],
        "拖动任务卡片可切换状态"
    );
    assert_eq!(cases["statusSlot"]["loading"]["status"], "loading");
    assert_eq!(
        cases["statusSlot"]["loading"]["text"],
        "正在加载任务与会话目录…"
    );
    assert_eq!(cases["statusSlot"]["failed"]["status"], "failed");
    assert_eq!(cases["statusSlot"]["failed"]["text"], "任务快照加载失败");
    assert_eq!(cases["statusSlot"]["warning"]["status"], "warning");
    assert_eq!(cases["statusSlot"]["warning"]["text"], "目录部分不可用");
    assert!(
        cases["entryDiscovery"]["currentSidebarClassIndependent"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["accessibleNameOnly"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["legacyIconFallback"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["uniqueGlobalFallback"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["ambiguousGlobalRejected"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["settingsNavigationExcluded"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["entryDiscovery"]["settingsOnlyRejected"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["catalog"]["latestTitle"], "目录最新标题");
    assert!(
        cases["catalog"]["latestTitleMatchesSearch"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["catalog"]["partialMissingAvailable"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["catalog"]["partialMissingLabel"], "目录部分不可用");
    assert!(
        !cases["catalog"]["completeMissingAvailable"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["conversationStatuses"]["running"], "running");
    assert_eq!(
        cases["conversationStatuses"]["completedUnread"],
        "completed-unread"
    );
    assert_eq!(cases["conversationStatuses"]["completed"], "completed");
    assert_eq!(cases["conversationStatuses"]["unknown"], "unknown");
    assert_eq!(cases["conversationStatuses"]["unavailable"], "unavailable");
    assert!(
        cases["conversationStatuses"]["usageRouteAndProjection"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["conversationStatuses"]["boundedConcurrency"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["conversationStatuses"]["idleRefreshSkipped"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["runtimeRefresh"]["activeAfterRefresh"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["read"]["snapshotTitleBeforeCatalog"], "先到的快照");
    assert_eq!(cases["read"]["catalogCountBeforeCatalog"], 0);
    assert!(cases["read"]["loadingBeforeCatalog"].as_bool().unwrap());
    assert_eq!(cases["read"]["catalogCountAfterCatalog"], 1);
    assert!(!cases["read"]["loadingAfterCatalog"].as_bool().unwrap());
}

#[test]
fn renderer_task_board_create_modal_preserves_accessibility_payload_and_recovery_contracts() {
    let script = assets::renderer_features_script();
    let cases = run_task_board_create_contract_harness();

    assert!(script.contains("\"/task-board/task-create\""));
    assert!(script.contains("toolbar.append(searchControl, filter, create, hint)"));
    assert!(script.contains("create.title = \"新建任务\""));
    assert!(script.contains("role\", \"dialog\""));
    assert!(script.contains("aria-modal\", \"true\""));
    assert!(script.contains("height: min(650px, calc(100vh - 32px))"));
    assert!(script.contains("width: 650px"));
    assert!(script.contains("max-width: calc(100vw - 32px)"));
    assert!(script.contains("overflow: hidden;\n        padding: 17px 20px 14px;"));
    assert!(script.contains("@media (max-height: 620px)"));
    assert!(script.contains("将 Codex 会话组织到跨项目任务看板中"));
    assert!(script.contains("codex-task-board-create-field-row"));
    assert!(script.contains("codex-task-board-create-modal-footer"));
    assert!(script.contains("codex-task-board-create-mode-content"));
    assert!(script.contains("codex-task-board-create-instruction-field"));
    assert!(script.contains("codex-task-board-create-composer"));
    assert!(script.contains("codex-task-board-create-model-trigger"));
    assert!(script.contains("codex-task-board-create-settings-menu"));
    assert!(script.contains("codex-task-board-create-model-menu"));
    assert!(script.contains("codex-task-board-create-effort-menu"));
    assert!(script.contains("function openTaskBoardCreateSettingsMenu("));
    assert!(script.contains("function taskBoardOpenCreateSettingsSubmenu("));
    assert!(script.contains("推理强度"));
    assert!(script.contains(r#"{ id: "low", label: "轻度" }"#));
    assert!(script.contains(
        r#"const taskBoardDefaultReasoningEffortIds = ["low", "medium", "high", "xhigh", "max"];"#
    ));
    assert!(script.contains(r#"placement: "top""#));
    assert!(script.contains("const opensAbove ="));
    assert!(script.contains("menuitemradio"));
    assert!(!script.contains("taskBoardConfigureDropdownTrigger(model"));
    assert!(script.contains("新会话模型"));
    assert!(script.contains("创建新会话"));
    assert!(!script.contains("将立即创建新会话、发送这条指令，并追加到当前任务"));
    assert!(script.contains("codex-task-board-empty-column"));
    assert!(script.contains("function taskBoardCreateModalKeydown("));
    assert!(script.contains("event.shiftKey"));
    assert!(script.contains("probe(project)"));
    assert!(
        script.contains(
            "startConversation(project, firstInstruction, modelId = \"\", effortId = \"\")"
        )
    );
    assert!(script.contains("function taskBoardNativeProbe(project)"));
    assert!(script.contains("function taskBoardNativeStartConversation("));
    assert!(script.contains("effortId = \"\","));
    assert!(script.contains("function taskBoardNativeSelectModel("));
    assert!(script.contains("function taskBoardNativeSelectReasoningEffort("));
    assert!(script.contains("modal.effortId,"));
    assert!(script.contains("actions.append(submitButton, cancelButton)"));
    assert!(
        script
            .contains("backdropPressCompleted = backdropPressStarted && event.target === backdrop")
    );
    assert!(script.contains("description: project.cwd"));
    assert!(script.contains("taskBoardSettingsNavigationLabelPattern"));
    assert!(script.contains("installTaskBoardNavigationObserver"));
    assert!(script.contains("classList?.toggle?.(\"bg-primary-ghost-hover\", !!active)"));
    assert!(!script.contains("任务将保存到本地看板，关联会话限制在同一项目内。"));
    assert!(!script.contains("仅展示当前目录中属于所选项目的会话；可同时选择多个。"));
    assert!(!script.contains("创建任务后将立即创建新会话，并发送这条首条指令。"));
    assert!(script.contains("\"native_create_unavailable\""));
    assert!(script.contains("function taskBoardConflictSnapshotResult("));
    assert!(script.contains("function reconcileTaskBoardRuntime()"));
    assert!(script.contains("taskBoardReconcileCreateSelectedSessions"));
    assert!(script.contains("function taskBoardToolbarLayout("));
    let create_submit = script
        .split("async function submitTaskBoardCreate()")
        .nth(1)
        .and_then(|section| {
            section
                .split("function taskBoardConversationButton(")
                .next()
        })
        .expect("task board create submit should be present");
    assert!(create_submit.contains("taskBoardNativeAdapter.probe(project)"));
    assert!(!create_submit.contains("startConversation("));
    assert!(script.contains("const initialStatus = taskBoardStatusId(modal.initialStatus);"));
    assert!(script.contains("await taskBoardApplyInitialStatus(taskId, initialStatus);"));

    assert_eq!(cases["modal"]["role"], "dialog");
    assert!(cases["modal"]["ariaModal"].as_bool().unwrap());
    assert!(cases["modal"]["initialFocus"].as_bool().unwrap());
    assert!(cases["modal"]["bodyMounted"].as_bool().unwrap());
    assert!(cases["modal"]["outsideMain"].as_bool().unwrap());
    assert!(cases["modal"]["tabForwardWraps"].as_bool().unwrap());
    assert!(
        cases["modal"]["tabBackwardWraps"].as_bool().unwrap(),
        "backward focus target: {}, order: {}",
        cases["modal"]["tabBackwardActive"],
        cases["modal"]["tabFocusableControls"]
    );
    assert!(cases["modal"]["busyControlsStayOpen"].as_bool().unwrap());
    assert!(cases["modal"]["dragReleaseStaysOpen"].as_bool().unwrap());
    assert!(cases["modal"]["backdropClickCloses"].as_bool().unwrap());
    assert!(
        cases["modal"]["routineReconcilePreservesDraft"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["modal"]["keydownListenersBeforeRefresh"], 1);
    assert!(cases["modal"]["removedAfterRefresh"].as_bool().unwrap());
    assert_eq!(cases["modal"]["keydownListenersAfterRefresh"], 0);
    assert!(cases["modal"]["focusRestored"].as_bool().unwrap());
    assert!(!cases["modal"]["busyAfterRefresh"].as_bool().unwrap());
    assert!(
        cases["dropdowns"]["sharedListbox"].as_bool().unwrap(),
        "dropdown state: {}",
        cases["dropdowns"]
    );
    assert!(
        cases["dropdowns"]["projectMenusConsistent"]
            .as_bool()
            .unwrap(),
        "project dropdown state: {}",
        cases["dropdowns"]
    );
    assert!(
        cases["dropdowns"]["nativeSettingsMenu"].as_bool().unwrap(),
        "settings menu state: {}",
        cases["dropdowns"]
    );
    assert!(
        cases["dropdowns"]["keyboardAndFocus"].as_bool().unwrap(),
        "dropdown state: {}",
        cases["dropdowns"]
    );

    assert!(
        cases["projectSelection"]["onlyMatchingSessions"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["projectSelection"]["clearedAfterProjectChange"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["projectSelection"]["catalogOutcomeReconcilesAndRenders"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["validation"]["trimmedTitleRejected"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["validation"]["catalogFailureBlockedExistingOnly"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["validation"]["tasksPreservedOnCatalogFailure"]
            .as_bool()
            .unwrap()
    );

    assert!(cases["success"]["exactPayload"].as_bool().unwrap());
    assert!(cases["success"]["closed"].as_bool().unwrap());
    assert!(!cases["success"]["busy"].as_bool().unwrap());
    assert!(
        cases["attach"]["existing"]["excludesAlreadyLinked"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["attach"]["existing"]["exactPayloadAndSnapshot"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["attach"]["existing"]["closed"].as_bool().unwrap());
    assert!(
        cases["attach"]["native"]["createsThenAttaches"]
            .as_bool()
            .unwrap(),
        "attach native cases: {cases}"
    );
    assert!(
        cases["attach"]["native"]["modelForwarded"]
            .as_bool()
            .unwrap(),
        "attach native model cases: {cases}"
    );
    assert!(
        cases["attach"]["native"]["effortForwarded"]
            .as_bool()
            .unwrap(),
        "attach native effort cases: {cases}"
    );
    assert!(cases["attach"]["native"]["closed"].as_bool().unwrap());
    assert!(cases["detach"]["confirmation"].as_bool().unwrap());
    assert!(
        cases["detach"]["cancelledWithoutRequest"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["detach"]["exactPayloadAndSnapshot"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["detach"]["revisionConflictRetriesOnce"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["initialStatus"]["createThenMove"].as_bool().unwrap());
    assert!(
        cases["initialStatus"]["moveUsesCreatedRevision"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(cases["initialStatus"]["finalStatus"], "planning");

    for code in [
        "invalid_input",
        "project_mismatch",
        "task_id_conflict",
        "bridge_unavailable",
        "task_board_busy",
        "task_file_invalid",
        "task_board_unavailable",
    ] {
        assert!(
            cases["stableErrors"][code]["feedback"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{code} should provide explicit feedback"
        );
        assert!(cases["stableErrors"][code]["modalOpen"].as_bool().unwrap());
        assert!(!cases["stableErrors"][code]["busy"].as_bool().unwrap());
        assert!(
            cases["stableErrors"][code]["inputsPreserved"]
                .as_bool()
                .unwrap()
        );
    }
    assert!(
        cases["stableErrors"]["task_id_conflict"]["nextManualRetryRotatesUuid"]
            .as_bool()
            .unwrap()
    );

    assert!(
        cases["sessionNotFound"]["catalogRefreshed"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["sessionNotFound"]["modalOpen"].as_bool().unwrap());
    assert!(!cases["sessionNotFound"]["busy"].as_bool().unwrap());
    assert!(
        cases["sessionNotFound"]["staleSelectionCleared"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["sessionNotFound"]["nextSubmitRequiresSelection"]
            .as_bool()
            .unwrap()
    );

    assert!(cases["revisionConflict"]["retriedOnce"].as_bool().unwrap());
    assert!(cases["revisionConflict"]["sameTaskId"].as_bool().unwrap());
    assert_eq!(
        cases["revisionConflict"]["expectedRevisions"],
        serde_json::json!([3, 4])
    );
    assert!(
        cases["revisionConflict"]["closedAfterRetry"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["revisionConflict"]["secondConflictStops"]
            .as_bool()
            .unwrap()
    );
    assert!(
        !cases["revisionConflict"]["secondConflictBusy"]
            .as_bool()
            .unwrap()
    );

    assert!(
        cases["nativeMode"]["instructionRequiredStaysOpen"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["nativeMode"]["neverStartsConversation"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["idempotency"]["uuidIsValid"].as_bool().unwrap());
    assert!(
        cases["idempotency"]["manualRetryReusesUuid"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["idempotency"]["semanticChangeRotatesUuid"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["idempotency"]["labelOnlyChangeReusesUuid"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["toolbar"]["wideInlineAdjacent"].as_bool().unwrap());
    assert!(
        cases["toolbar"]["narrowWrapsWith36pxCreate"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["lifecycle"]["deferredClosePreventsLateWrite"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn renderer_task_board_open_session_uses_native_rows_and_bounded_project_expansion() {
    let script = assets::renderer_features_script();
    let cases = run_task_board_open_session_contract_harness();
    let open_session = script
        .split("async function taskBoardNativeOpenSession(")
        .nth(1)
        .and_then(|section| section.split("function taskBoardNativeProjectRow(").next())
        .expect("native open session implementation should exist");

    assert!(script.contains("[data-app-action-sidebar-thread-id]"));
    assert!(open_session.contains("taskBoardNativeProjectTarget(location.cwd)"));
    assert!(open_session.contains("taskBoardNativeOpenSessionTimeoutMs"));
    assert!(!open_session.contains("dispatcher"));
    assert!(!open_session.contains("window.location"));
    assert!(!open_session.contains("sqlite"));

    assert!(cases["mounted"]["rawIdClickedOnce"].as_bool().unwrap());
    assert!(cases["mounted"]["localIdClickedOnce"].as_bool().unwrap());
    assert!(
        cases["expanded"]["projectThenThreadClickedOnce"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["deadline"]["fiveSecondsAndStableFailure"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["errors"]["missingSessionStable"].as_bool().unwrap());
    assert!(cases["errors"]["missingProjectStable"].as_bool().unwrap());
    assert!(
        cases["runtimeReplacement"]["stableFailure"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["repeat"]["safeAndDataPreserved"].as_bool().unwrap(),
        "open session cases: {cases}"
    );
    assert!(cases["seam"]["injectedAdapterStillUsed"].as_bool().unwrap());
}

#[test]
fn renderer_task_board_native_create_uses_host_adapter_and_recovers_without_instruction() {
    let script = assets::renderer_features_script();
    let cases = run_task_board_native_create_contract_harness();

    assert!(script.contains("function taskBoardNativeProjectRow(project)"));
    assert!(script.contains("[data-app-action-sidebar-project-row]"));
    assert!(script.contains("memoizedProps?.composerController"));
    assert!(script.contains("owner = owner.parentElement"));
    assert!(script.contains("function taskBoardNativePermanentSessionId()"));
    assert!(script.contains("taskBoardNativeCreatePermanentIdTimeoutMs = 15 * 1000"));
    assert!(script.contains("taskBoardNativeModelSelectionTimeoutMs = 5 * 1000"));
    assert!(script.contains("function taskBoardNativeSelectModel("));
    assert!(script.contains("function taskBoardNativeSelectReasoningEffort("));
    assert!(script.contains("taskBoardNativeCreateSessionRetryDelaysMs"));
    assert!(script.contains("taskBoardNativeCreateRecoveryTtlMs = 24 * 60 * 60 * 1000"));

    assert!(
        cases["supported"]["structuralButtonOnly"]
            .as_bool()
            .unwrap(),
        "native create cases: {cases}"
    );
    assert!(
        cases["supported"]["controllerAndNativeSubmit"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["supported"]["selectedSettingsBeforeSubmit"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["navigationRace"]["waitsForReplacementBeforeSettings"]
            .as_bool()
            .unwrap(),
        "native create navigation race cases: {cases}"
    );
    assert!(cases["supported"]["temporaryIdIgnored"].as_bool().unwrap());
    assert!(
        cases["supported"]["createAfterPermanentId"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["unsupported"]["newDisabledExistingWorks"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["timeout"]["boundedAt15Seconds"].as_bool().unwrap());
    assert!(
        cases["retry"]["sessionNotFoundWithin10Seconds"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["retry"]["revisionRetriesOnceWithSameTaskId"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["recovery"]["bridgeFailurePersistsAllowedFields"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["recovery"]["nextActivationRetriesOnceAndClears"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["recovery"]["retryFailureKeepsRecordAndWarns"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["recovery"]["expiredRecordDiscarded"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["recovery"]["malformedRecordDiscarded"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["routineRefresh"]["keepsCreateAlive"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["runtimeReplacement"]["keepsRuntimeAndCreateAlive"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["privacy"]["payloadStorageAndOutputExcludeInstruction"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn renderer_task_board_move_drag_menu_and_recovery_contracts() {
    let script = assets::renderer_features_script();
    let cases = run_task_board_move_contract_harness();

    assert!(script.contains("\"/task-board/task-move\""));
    assert!(script.contains("function taskBoardMoveTargetIndex("));
    assert!(script.contains("function taskBoardMoveTask("));
    assert!(script.contains("function openTaskBoardStatusMenu("));
    assert!(script.contains("menuRole = \"listbox\""));
    assert!(script.contains("taskBoardMoveTargetIndex(taskId, status.id)"));

    assert!(cases["payloads"]["crossColumn"].as_bool().unwrap());
    assert!(cases["payloads"]["sameColumn"].as_bool().unwrap());
    assert!(cases["payloads"]["filteredIndex"].as_bool().unwrap());
    assert!(cases["payloads"]["zeroAndEnd"].as_bool().unwrap());
    assert!(cases["payloads"]["selfDropNoOp"].as_bool().unwrap());
    assert!(cases["menu"]["fiveStatuses"].as_bool().unwrap());
    assert!(cases["menu"]["keyboardAndFocus"].as_bool().unwrap());
    assert!(
        cases["success"]["serverSnapshotCorrectsOptimistic"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["failure"]["rollbackAndBusyRelease"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["conflict"]["adoptsLatestWithoutRetry"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["conflict"]["malformedRollsBack"].as_bool().unwrap());
    assert!(
        cases["reads"]["beforeMoveCannotOverwrite"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["reads"]["refreshDuringMoveSkipped"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["reads"]["moveFailureKeepsReadOut"].as_bool().unwrap());
    assert!(
        cases["lifecycle"]["staleDeferredAndCleanup"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["lifecycle"]["duplicateMoveBlocked"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["lifecycle"]["dragEndKeepsMoveAlive"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["dom"]["dragPathExactPayload"].as_bool().unwrap(),
        "DOM move cases: {cases}"
    );
    assert!(cases["dom"]["dragEndKeepsMoveAlive"].as_bool().unwrap());
    assert!(
        cases["dom"]["optimisticOrdersContinuous"]
            .as_bool()
            .unwrap()
    );
    assert!(cases["dom"]["sameColumnDownward"].as_bool().unwrap());
    assert!(cases["dom"]["selfDropNoRequest"].as_bool().unwrap());
    assert!(
        cases["dom"]["allConversationsRenderedInline"]
            .as_bool()
            .unwrap(),
        "DOM move cases: {cases}"
    );
    assert!(cases["dom"]["cardStructureMatchesDebug"].as_bool().unwrap());
    assert!(cases["dom"]["menuOutsideMain"].as_bool().unwrap());
    assert!(
        cases["dom"]["enterMovesAndRestoresFocus"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["dom"]["escapeRestoresOriginalFocus"]
            .as_bool()
            .unwrap()
    );
    assert!(
        cases["dom"]["mainReplacementRollsBackAndIgnoresLateResult"]
            .as_bool()
            .unwrap()
    );
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
    assert!(script.contains("element.src = source"));
    assert!(script.contains("image_overlay_installed"));
}

#[test]
fn injection_script_switches_skin_appearance_through_codex_native_action() {
    let script = assets::injection_script(45221);

    assert!(script.contains("register-app-actions-"));
    assert!(script.contains("app.appearance.set_mode"));
    assert!(script.contains("__codexElvesApplySkinAppearance"));
    assert!(!script.contains("data-codex-elves-skin-appearance"));
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
    let timeout_value = |name: &str| -> u64 {
        let marker = format!("const {name} = ");
        let start = script
            .find(&marker)
            .map(|offset| offset + marker.len())
            .unwrap_or_else(|| panic!("missing timeout constant {name}"));
        let end = script[start..]
            .find(';')
            .map(|offset| start + offset)
            .unwrap_or_else(|| panic!("unterminated timeout constant {name}"));
        script[start..end]
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("invalid timeout constant {name}"))
    };

    assert!(script.contains("bridgeWithBackendTimeout"));
    assert!(script.contains("backend_bridge_timeout"));
    assert!(script.contains("/backend/repair"));
    assert!(script.contains("waitForBackendBridgeRecovery"));
    assert!(script.contains("location.protocol === \"app:\""));
    assert!(script.contains("bridgeMissing: true"));
    assert!(script.contains("backend_status_bridge_failed_http_fallback_ok"));
    assert!(script.contains("backend_status_bridge_and_http_failed"));
    assert!(
        timeout_value("codexBackendStatusTimeoutMs")
            > timeout_value("codexBackendBridgeReadyTimeoutMs")
                + timeout_value("codexBackendBridgeTimeoutMs")
    );
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
fn injection_script_omits_removed_plugin_list_auto_expand_feature() {
    let script = assets::injection_script(45221);

    for removed_marker in [
        "codexPluginAutoExpand",
        "pluginAutoExpand",
        "codexAppPluginAutoExpand",
        "plugin_auto_expand_finished",
        "插件列表全量展示",
        "__CODEX_ELVES_TEST_PLUGIN_AUTO_EXPAND__",
        "__codexElvesPluginAutoExpandTest",
    ] {
        assert!(
            !script.contains(removed_marker),
            "removed plugin auto-expand marker remains: {removed_marker}"
        );
    }
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
    assert!(script.contains("/thread-usage-summary"));
    assert!(!script.contains("postJson(\"/thread-usage-history\""));
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
    assert!(script.contains("codexTokenUsageRetryDelaysMs = [1000, 2500, 5000]"));
    assert!(!script.contains("codexTokenUsageHiddenRefreshIntervalMs"));
    assert!(script.contains("function scheduleCodexTokenUsageRefresh(delayMs = 0)"));
    assert!(script.contains("installCodexTokenUsageVisibilityListener"));
    assert!(script.contains("window.__codexTokenUsageRefreshPending = true"));
    assert!(script.contains("summary.isRunning && document.visibilityState !== \"hidden\""));
    assert!(!script.contains("执行中 · 已结算至最近模型响应"));
    assert!(script.contains("descendantCount"));
    assert!(script.contains("window.__codexTokenUsageRequestSeq"));
    assert!(script.contains("backendTimedOut"));
    assert!(script.contains("processCodexTokenUsageResult"));
    assert!(script.contains("if (!backendTimedOut || result?.status !== \"ok\") return"));
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
fn injection_script_resolves_temporary_thread_id_and_marks_stale_token_usage() {
    let script = assets::injection_script(45221);

    // 新建会话侧边栏 id 会长期停在临时形态，必须用 composer 上的真实 conversation id 校正。
    assert!(script.contains("function isTemporaryThreadId(sessionId)"));
    assert!(script.contains("(client-)?new-thread:"));
    assert!(script.contains("function activeConversationIdFromDom()"));
    assert!(script.contains("[data-above-composer-conversation-id]"));
    assert!(script.contains("function resolveTemporarySessionRef(ref)"));
    assert!(script.contains("return resolveTemporarySessionRef(ref);"));
    assert!(script.contains(
        "return resolveTemporarySessionRef({ session_id: locationThreadId(), title: \"\" });"
    ));

    // 读取失败时卡片必须标记过期，不能静默保留旧数值。
    assert!(script.contains("function markCodexTokenUsageCardStale(card, sessionSignature)"));
    assert!(script.contains("markCodexTokenUsageCardStale(activeCard, sessionSignature);"));
    assert!(script.contains("function renderCodexTokenUsageSummary(card, summary, stale = false)"));
    assert!(script.contains("card.dataset.stale = String(stale === true)"));
    assert!(script.contains("codex-token-usage-stale"));
    assert!(script.contains("可能已过期"));
}

#[test]
fn injection_script_keeps_session_ref_stable_and_refreshes_after_turn_completion() {
    let script = assets::injection_script(45221);

    // 侧边栏瞬时拿不到会话时，composer id 作为独立主来源兼宽容期内沍用上一次结果。
    assert!(script.contains("function currentSessionRefFromDom()"));
    assert!(
        script.contains("if (conversationId) return { session_id: conversationId, title: \"\" };")
    );
    assert!(script.contains("codexSessionRefGraceMs = 15000"));
    assert!(script.contains("window.__codexElvesLastSessionRef"));

    // 运行结束那一刻的最后一笔用量靠收尾刷新补齐，不能随轮询停止而丢失。
    assert!(script.contains("codexTokenUsageCompletionRefreshDelayMs = 3000"));
    assert!(script.contains("window.__codexTokenUsageWasRunning"));
    assert!(
        script.contains("const needsCompletionRefresh = wasRunning && summary.isRunning !== true;")
    );
    assert!(
        script.contains("scheduleCodexTokenUsageRefresh(codexTokenUsageCompletionRefreshDelayMs);")
    );
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
    assert!(script.contains("const requestedGap = Number(button.dataset.codexTooltipGap);"));
    assert!(script.contains(": 3;"));
    assert!(script.contains("const aboveTop = buttonRect.top - tooltipRect.height - gap;"));
    assert!(script.contains("tooltip.dataset.side = aboveTop >= 8"));
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
    assert!(script.contains("app-initial-"));
    assert!(script.contains("thread-context-inputs-"));
    assert!(script.contains("findCodexServiceTierDispatcher"));
    assert!(script.contains("codexServiceTierDispatcherFromModule"));
    assert!(script.contains("codexServiceTierSettingReaderFromModule"));
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
    assert!(script.contains(
        "syncCodexServiceTierBadgeLayoutListener();\n      installCodexServiceTierBadge();"
    ));
    assert!(script.contains("toggleCodexServiceTierFromBadge"));
    assert!(script.contains("wireCodexServiceTierBadge"));
    assert!(script.contains("codexServiceTierBadgePlacement"));
    assert!(script.contains("codexServiceTierNativeServiceTierSlot"));
    assert!(script.contains("[class*=\"_footer_\"]"));
    assert!(script.contains("codexServiceTierComposerFooterSelector"));
    assert!(script.contains("ComposerLayoutFooter"));
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
    assert_eq!(cases["unicodeAliasMatches"]["primary"], true);
    assert_eq!(cases["unicodeAliasMatches"]["backup"], true);
    assert_eq!(cases["unicodeAliasMatches"]["asciiLocaleIndependent"], true);
    assert_eq!(cases["aliasCatalogResolution"]["primary"], "gpt-5.6-sol");
    assert_eq!(
        cases["aliasCatalogResolution"]["ambiguous"],
        serde_json::Value::Null
    );
    assert_eq!(
        cases["aliasCatalogResolution"]["aliasSlugConflict"],
        serde_json::Value::Null
    );

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
    assert_eq!(cases["modernServiceTierModule"]["setting"], "priority");
    assert_eq!(
        cases["modernServiceTierModule"]["turnMessage"]["type"],
        "start-turn-for-host"
    );
    assert_eq!(
        cases["modernServiceTierModule"]["turnMessage"]["payload"]["params"]["serviceTier"],
        "priority"
    );
    assert_eq!(
        cases["modernServiceTierModule"]["dispatcherInstalled"],
        true
    );
    assert_eq!(
        cases["modernServiceTierModule"]["requestClientInstalled"],
        true
    );
    assert_eq!(
        cases["modernServiceTierModule"]["dispatcherRetryPending"],
        false
    );
    assert_eq!(
        cases["modernServiceTierModule"]["requestClientRetryPending"],
        false
    );
}

#[test]
fn injection_script_does_not_patch_app_server_model_requests() {
    let script = assets::injection_script(45221);
    assert!(script.contains("const codexAppServerManagerDiscoveryVersion = \"11\";"));
    assert!(!script.contains("__codexElvesModelOriginalSendRequest"));
    assert!(!script.contains("__codexElvesModelRequestPatch"));
    assert!(!script.contains("codexElvesModelPatchedSendRequest"));
}

#[test]
fn injection_script_adds_safe_app_server_restart_recovery() {
    let script = assets::renderer_features_script();
    let cases = run_service_tier_contract_harness();

    assert!(script.contains("failed to start turn: internal error; agent loop died unexpectedly"));
    assert!(script.contains("button.dataset.codexAppServerRestart = \"true\""));
    assert!(script.contains("button.dataset.codexAppServerRestartVersion"));
    assert!(script.contains("function installCodexAppServerRestartButtons()"));
    assert!(script.contains("function codexAppServerRestartErrorElements()"));
    assert!(script.contains("function codexAppServerRestartMutationRelevant(mutation)"));
    assert!(script.contains("mutations.some(codexAppServerRestartMutationRelevant)"));
    assert!(script.contains("const root = document.body || document.documentElement;"));
    assert!(script.contains("document.body.appendChild(button);"));
    assert!(script.contains("positionCodexAppServerRestartButton(button);"));
    assert!(script.contains("function codexAppServerRestartVisibleErrorElement()"));
    assert!(script.contains("function resolveCodexAppServerRestartPlacement("));
    assert!(script.contains("placement: \"after\""));
    assert!(script.contains("placement: \"before\""));
    assert!(script.contains("placement: \"notice\""));
    assert!(script.contains("检测到 app-server 异常，点击重启"));
    assert!(!script.contains("banner.appendChild(button);"));
    assert!(script.contains("CodexElves 提供热重启修复问题"));
    assert!(script.contains("function codexAppServerRunningConversations("));
    assert!(script.contains("threadRuntimeStatus?.type === \"active\""));
    assert!(script.contains("runtimeThreadStatusEvidenceByThreadId"));
    assert!(script.contains("status === \"inProgress\""));
    assert!(script.contains("当前有 ${count} 个会话正在执行"));
    assert!(script.contains(
        "button.addEventListener(\"pointerenter\", () => showActionButtonTooltip(button));"
    ));
    assert!(script.contains("button.addEventListener(\"pointerleave\", hideActionButtonTooltip);"));
    assert!(script.contains("button.dataset.codexTooltipPlacement = \"top\""));
    assert!(script.contains("button.dataset.codexTooltipGap = \"10\""));
    assert!(!script.contains("button.dataset.codexTooltip = \"CodexElves 提供热重启修复问题\""));
    assert!(script.contains("dispatcher.dispatchMessage(\"codex-app-server-restart\""));
    assert!(script.contains("killCodexProcess: false"));
    assert!(!script.contains("data-codex-app-server-restart-force"));

    assert_eq!(cases["appServerRestart"]["transientMatched"], true);
    assert_eq!(cases["appServerRestart"]["persistedMatched"], false);
    assert_eq!(cases["appServerRestart"]["exactErrorTextMatched"], true);
    assert_eq!(cases["appServerRestart"]["decoratedErrorTextMatched"], true);
    assert_eq!(
        cases["appServerRestart"]["unrelatedErrorTextMatched"],
        false
    );
    assert_eq!(cases["appServerRestart"]["errorMutationMatched"], true);
    assert_eq!(cases["appServerRestart"]["unrelatedMutationMatched"], false);
    assert_eq!(cases["appServerRestart"]["afterPlacement"], "after");
    assert_eq!(cases["appServerRestart"]["beforePlacement"], "before");
    assert_eq!(cases["appServerRestart"]["noticePlacement"], "notice");
    assert_eq!(cases["appServerRestart"]["noticeRight"], 18);
    assert_eq!(cases["appServerRestart"]["noticeBottom"], 18);
    assert_eq!(
        cases["appServerRestart"]["runningIds"],
        json!(["running-main", "running-turn", "running-subagent"])
    );
    assert_eq!(
        cases["appServerRestart"]["failedConversationBlocked"],
        false
    );
    assert_eq!(cases["appServerRestart"]["removedTurnCount"], 1);
    assert_eq!(
        cases["appServerRestart"]["remainingEntityKeys"],
        json!(["persisted"])
    );
    assert_eq!(
        cases["appServerRestart"]["remainingIslandKeys"],
        json!(["persisted"])
    );
    assert_eq!(
        cases["appServerRestartDispatch"]["dispatched"][0]["type"],
        "codex-app-server-restart"
    );
    assert_eq!(
        cases["appServerRestartDispatch"]["dispatched"][0]["payload"]["killCodexProcess"],
        false
    );
    assert_eq!(cases["appServerRestartDispatch"]["remainingFailure"], false);
    assert_eq!(
        cases["appServerRestartDispatch"]["toasts"],
        json!(["app-server 已重启，失败状态已清理"])
    );
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
window.__CODEX_ELVES_TEST_APP_SERVER_RESTART__ = true;
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
const appServerRestartApi = window.__codexElvesAppServerRestartTest;
const appServerRestartError = "failed to start turn: internal error; agent loop died unexpectedly";
const transientFailedTurn = {{
  turnId: null,
  status: "failed",
  error: {{ message: appServerRestartError }},
  items: [{{ type: "error", message: appServerRestartError }}],
}};
const persistedFailedTurn = {{
  ...transientFailedTurn,
  turnId: "turn-persisted",
}};
function restartConversation(id, turns, runtimeType = "idle") {{
  const entitiesByKey = Object.fromEntries(turns.map((turn, index) => [
    index === 0 ? "failed" : `entity-${{index}}`,
    turn,
  ]));
  return {{
    id,
    title: id,
    turns: [],
    threadRuntimeStatus: {{ type: runtimeType }},
    turnHistory: {{
      kind: "canonical",
      history: {{
        entitiesByKey,
        islands: [{{
          id: "tail",
          entries: Object.keys(entitiesByKey).map((key) => ({{ key, value: key }})),
        }}],
      }},
    }},
  }};
}}
function restartManager(conversations, runtimeEvidence = new Map()) {{
  return {{
    hostId: "local",
    threadStore: {{ runtimeThreadStatusEvidenceByThreadId: runtimeEvidence }},
    getCachedConversations: () => conversations,
    getConversation: (id) => conversations.find((conversation) => conversation.id === id) || null,
    updateConversationState(id, updater) {{
      const conversation = this.getConversation(id);
      if (conversation) updater(conversation);
    }},
  }};
}}
const failedConversation = restartConversation("failed-current", [transientFailedTurn], "active");
const runningMain = restartConversation("running-main", [{{ turnId: "done", status: "completed", items: [] }}], "active");
const runningTurn = restartConversation("running-turn", [{{ turnId: "turn-running", status: "inProgress", items: [] }}], "idle");
const restartRunningManager = restartManager(
  [failedConversation, runningMain, runningTurn],
  new Map([["running-subagent", {{ type: "active", activeFlags: [] }}]])
);
const runningState = appServerRestartApi.runningConversations(
  restartRunningManager,
  failedConversation.id
);
const cleanupConversation = restartConversation(
  "cleanup-current",
  [transientFailedTurn, persistedFailedTurn],
  "idle"
);
cleanupConversation.turnHistory.history.entitiesByKey = {{
  failed: transientFailedTurn,
  persisted: persistedFailedTurn,
}};
cleanupConversation.turnHistory.history.islands = [{{
  id: "tail",
  entries: [
    {{ key: "failed", value: "failed" }},
    {{ key: "persisted", value: "persisted" }},
  ],
}}];
const cleanupManager = restartManager([cleanupConversation]);
const removedTurnCount = appServerRestartApi.cleanupTransientFailedTurns(
  cleanupManager,
  cleanupConversation.id
);
const appServerRestart = {{
  transientMatched: appServerRestartApi.isTransientFailedTurn(transientFailedTurn),
  persistedMatched: appServerRestartApi.isTransientFailedTurn(persistedFailedTurn),
  exactErrorTextMatched: appServerRestartApi.matchesErrorText(appServerRestartError),
  decoratedErrorTextMatched: appServerRestartApi.matchesErrorText(
    `prefix ${{appServerRestartError}} suffix`
  ),
  unrelatedErrorTextMatched: appServerRestartApi.matchesErrorText(
    "failed to start turn: request cancelled"
  ),
  errorMutationMatched: appServerRestartApi.mutationRelevant({{
    type: "childList",
    addedNodes: [{{
      nodeType: 1,
      innerText: appServerRestartError,
      textContent: appServerRestartError,
      closest() {{ return null; }},
    }}],
    removedNodes: [],
  }}),
  unrelatedMutationMatched: appServerRestartApi.mutationRelevant({{
    type: "childList",
    addedNodes: [{{
      nodeType: 1,
      innerText: "completed",
      textContent: "completed",
      closest() {{ return null; }},
    }}],
    removedNodes: [],
  }}),
  afterPlacement: appServerRestartApi.resolvePlacement(
    {{ left: 100, right: 300, top: 100, height: 20 }},
    {{ width: 58, height: 26 }},
    1000,
    800
  ).placement,
  beforePlacement: appServerRestartApi.resolvePlacement(
    {{ left: 930, right: 990, top: 100, height: 20 }},
    {{ width: 58, height: 26 }},
    1000,
    800
  ).placement,
  noticePlacement: appServerRestartApi.resolvePlacement(
    {{ left: 20, right: 990, top: 100, height: 20 }},
    {{ width: 58, height: 26 }},
    1000,
    800
  ).placement,
  noticeRight: appServerRestartApi.resolvePlacement(
    {{ left: 20, right: 990, top: 100, height: 20 }},
    {{ width: 58, height: 26 }},
    1000,
    800
  ).right,
  noticeBottom: appServerRestartApi.resolvePlacement(
    {{ left: 20, right: 990, top: 100, height: 20 }},
    {{ width: 58, height: 26 }},
    1000,
    800
  ).bottom,
  runningIds: runningState.conversations.map((conversation) => conversation.id),
  failedConversationBlocked: runningState.conversations.some(
    (conversation) => conversation.id === failedConversation.id
  ),
  removedTurnCount,
  remainingEntityKeys: Object.keys(cleanupConversation.turnHistory.history.entitiesByKey),
  remainingIslandKeys: cleanupConversation.turnHistory.history.islands[0].entries.map(
    (entry) => entry.value
  ),
}};
api.setServiceTierState({{ serviceTier: "priority", fastTierValue: "priority" }});
api.setModelCatalog({{ status: "ok", model: "gpt-5.4", default_model: "gpt-5.4", models: ["gpt-5.4", "gpt-5.5"] }});
const displayNameMatches = {{
  gpt56Sol: api.modelMatchesText("gpt-5.6-sol", "5.6 Sol"),
  gpt56Terra: api.modelMatchesText("gpt-5.6-terra", "5.6 Terra"),
  gpt55: api.modelMatchesText("gpt-5.5", "5.5 超高"),
}};
const unicodeAliasMatches = {{
  primary: api.modelMatchesText("主力编程模型", "主力编程模型"),
  backup: api.modelMatchesText("备用编程模型", "备用编程模型"),
  asciiLocaleIndependent: api.modelMatchesText("KIMI", "kimi"),
}};
api.setModelCatalog({{
  status: "ok",
  model: "gpt-5.6-sol",
  default_model: "gpt-5.6-sol",
  model_entries: [
    {{ slug: "gpt-5.6-sol", display_name: "主力编程模型" }},
    {{ slug: "gpt-fast", display_name: "主力" }},
    {{ slug: "claude-slow", display_name: "主力" }},
    {{ slug: "model-a", display_name: "model-b" }},
    {{ slug: "model-b", display_name: "备用模型 B" }},
  ],
}});
const aliasCatalogResolution = {{
  primary: api.catalogSlugForText("主力编程模型"),
  ambiguous: api.catalogSlugForText("主力"),
  aliasSlugConflict: api.catalogSlugForText("model-b"),
}};
api.setModelCatalog({{ status: "ok", model: "gpt-5.4", default_model: "gpt-5.4", models: ["gpt-5.4", "gpt-5.5"] }});

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

async function runModernServiceTierModuleCase() {{
  api.resetServiceTierInstallState();
  api.setModelCatalog({{ status: "ok", model: "gpt-5.4", default_model: "gpt-5.4", models: ["gpt-5.4"] }});
  api.setThreadState({{ mode: "global-fast", defaultMode: "fast", entries: {{}} }});
  const dispatched = [];
  const dispatcher = {{
    handlers: new Map(),
    dispatchMessage(type, payload) {{
      dispatched.push({{ type, payload }});
    }},
    handleMessage() {{}},
  }};
  async function modernSettingReader(setting) {{
    const request = {{ method: "get-setting", params: {{ key: setting.key }} }};
    const result = {{
      value: request.params.key === "default-service-tier" ? "priority" : setting.default,
    }};
    return result.value ?? setting.default;
  }}
  api.setModuleLoader(async (namePart) => {{
    if (namePart === "app-initial-") {{
      return {{ modernSettingReader, dispatcher }};
    }}
    throw new Error(`legacy module unavailable: ${{namePart}}`);
  }});
  const setting = await api.readServiceTierSetting();
  await Promise.all([
    api.installDispatcherPatch(),
    api.installRequestClientPatch(),
  ]);
  dispatcher.dispatchMessage("start-turn-for-host", {{
    conversationId: "thread-12345678",
    params: {{
      model: "gpt-5.4",
      serviceTier: null,
    }},
  }});
  const state = api.serviceTierInstallState();
  const turnMessage = dispatched.find((message) => message.type === "start-turn-for-host");
  api.setModuleLoader(null);
  return {{
    setting,
    dispatched,
    turnMessage,
    ...state,
  }};
}}

async function runAppServerRestartDispatchCase() {{
  const conversation = restartConversation(
    "restart-safe",
    [transientFailedTurn],
    "active",
  );
  const manager = restartManager([conversation]);
  const dispatched = [];
  const dispatcher = {{
    dispatchMessage(type, payload) {{
      dispatched.push({{ type, payload }});
    }},
  }};
  const button = node();
  button.isConnected = true;
  appServerRestartApi.setConversationManager(manager);
  appServerRestartApi.setDispatcher(dispatcher);
  await appServerRestartApi.restartFromFailure(button, conversation.id);
  return {{
    dispatched,
    remainingFailure: appServerRestartApi.conversationHasTransientFailure(conversation),
    buttonText: button.textContent,
    buttonDisabled: button.disabled === true,
    toasts: window.__codexElvesAppServerRestartTestToasts || [],
  }};
}}

(async () => {{
  const serviceTierRetry = await runServiceTierRetryCase();
  const modernServiceTierModule = await runModernServiceTierModuleCase();
  const appServerRestartDispatch = await runAppServerRestartDispatchCase();
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
    unicodeAliasMatches,
    aliasCatalogResolution,
    catalogDrivenFast,
    catalogDrivenBlocked,
    patchedCreateRequest,
    relayModelNames,
    modelVisibilityConfig,
    pluginMarketplaceRequestParams,
    pluginMarketplaceRequestClient: pluginMarketplaceRequestClientCase,
    pluginScopedFilters,
    badgeTooltip,
    appServerRestart,
    appServerRestartDispatch,
    serviceTierRetry,
    modernServiceTierModule,
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

fn run_task_board_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("task-board-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    std::fs::write(
        &harness_path,
        r#"
const scriptPath = process.argv[2];
function node() {
  return {
    children: [],
    dataset: {},
    style: { setProperty() {}, removeProperty() {} },
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
    appendChild(child) {
      child.parentElement?.remove?.();
      this.children.push(child);
      child.parentElement = this;
      child.isConnected = true;
      return child;
    },
    append(...children) { children.forEach((child) => this.appendChild(child)); },
    prepend(...children) {
      children.slice().reverse().forEach((child) => {
        child.parentElement?.remove?.();
        this.children.unshift(child);
        child.parentElement = this;
        child.isConnected = true;
      });
    },
    remove() {
      this.removed = true;
      if (this.parentElement) {
        const index = this.parentElement.children.indexOf(this);
        if (index >= 0) this.parentElement.children.splice(index, 1);
      }
      this.parentElement = null;
      this.isConnected = false;
    },
    replaceChildren(...children) {
      this.children.slice().forEach((child) => child.remove());
      this.append(...children);
    },
    setAttribute() {},
    getAttribute() { return null; },
    removeAttribute() {},
    toggleAttribute() {},
    addEventListener() {},
    removeEventListener() {},
    querySelector() { return null; },
    querySelectorAll() { return []; },
    closest() { return null; },
    matches() { return false; },
    contains() { return false; },
    insertAdjacentElement() {},
    parentElement: null,
    isConnected: true,
    removed: false,
    textContent: "",
    innerHTML: "",
    clientWidth: 0,
    clientHeight: 0,
  };
}
globalThis.window = globalThis;
globalThis.Element = class Element {};
globalThis.HTMLElement = class HTMLElement extends Element {};
globalThis.HTMLButtonElement = class HTMLButtonElement extends HTMLElement {};
globalThis.MutationObserver = class MutationObserver {
  observe() {}
  disconnect() {}
};
globalThis.ResizeObserver = class ResizeObserver {
  observe() {}
  disconnect() {}
};
globalThis.requestAnimationFrame = (callback) => { callback(); return 1; };
globalThis.cancelAnimationFrame = () => {};
window.addEventListener = () => {};
window.removeEventListener = () => {};
window.dispatchEvent = () => true;
const documentListeners = new Map();
function documentListenerSet(type) {
  let listeners = documentListeners.get(type);
  if (!listeners) {
    listeners = new Set();
    documentListeners.set(type, listeners);
  }
  return listeners;
}
globalThis.document = {
  readyState: "complete",
  scripts: [],
  visibilityState: "visible",
  documentElement: node(),
  body: node(),
  createElement: () => node(),
  getElementById: () => null,
  querySelector: () => null,
  querySelectorAll: () => [],
  addEventListener(type, listener) { documentListenerSet(type).add(listener); },
  removeEventListener(type, listener) { documentListenerSet(type).delete(listener); },
  listenerCount(type) { return documentListeners.get(type)?.size || 0; },
};
globalThis.getComputedStyle = () => ({
  display: "block",
  visibility: "visible",
  pointerEvents: "auto",
});
globalThis.localStorage = {
  getItem: () => null,
  setItem() {},
  removeItem() {},
};
globalThis.location = {
  href: "https://codex.test/local/thread-12345678",
  pathname: "/local/thread-12345678",
  search: "",
  hash: "",
  protocol: "https:",
};
globalThis.navigator = { userAgent: "node-test" };
globalThis.performance = { getEntriesByType: () => [] };
globalThis.CustomEvent = class CustomEvent {
  constructor(type, options = {}) {
    this.type = type;
    this.detail = options.detail;
  }
};
globalThis.Event = class Event {
  constructor(type) {
    this.type = type;
  }
};
window.__CODEX_ELVES_TEST_TASK_BOARD__ = true;
window.__codexSessionDeleteBridge = async (path) => {
  if (path === "/settings/get") {
    return { launchMode: "direct", enhancementsEnabled: true, providerSyncEnabled: true };
  }
  if (path === "/session/suppressed") return { ids: [] };
  return { status: "ok" };
};
require(scriptPath);
const api = window.__codexElvesTaskBoardTest;
if (!api) throw new Error("task board test api unavailable");
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}
async function tick() {
  await Promise.resolve();
  await Promise.resolve();
}
(async () => {
  const runtimeGate = {
    oldRuntimeAccepted: api.runtimeCanRefresh("old-runtime", () => {}),
    currentRuntimeAccepted: api.runtimeCanRefresh(api.runtimeVersion(), () => {}),
  };
  function pluginControl({
    text = "",
    ariaLabel = "",
    title = "",
    legacyIcon = false,
    navigationLabel = "",
    sidebarContent = false,
  } = {}) {
    const navigation = {
      getAttribute(name) {
        return name === "aria-label" ? navigationLabel || null : null;
      },
      querySelector() {
        return sidebarContent ? {} : null;
      },
    };
    return {
      textContent: text,
      getAttribute(name) {
        if (name === "aria-label") return ariaLabel || null;
        if (name === "title") return title || null;
        return null;
      },
      querySelector() { return legacyIcon ? {} : null; },
      closest(selector) {
        return selector.includes("nav") || selector.includes("navigation")
          ? navigation
          : null;
      },
    };
  }
  const originalQuerySelectorAll = document.querySelectorAll;
  function resolvePluginEntry(
    navigationControls,
    globalControls = navigationControls,
    resolve = api.pluginEntryButtonForTest,
  ) {
    document.querySelectorAll = (selector) => {
      if (selector === 'button, [role="button"], a[href]') return globalControls;
      if (selector.includes("navigation") || selector.includes("nav button")) {
        return navigationControls;
      }
      return [];
    };
    return resolve();
  }
  const unrelatedControl = pluginControl({ text: "拉取请求" });
  const currentSidebarControl = pluginControl({ text: "插件" });
  const accessibleOnlyControl = pluginControl({ ariaLabel: "Plugins" });
  const legacyIconControl = pluginControl({ legacyIcon: true });
  const globalFallbackControl = pluginControl({ title: "插件" });
  const duplicateGlobalControl = pluginControl({ text: "Plugins" });
  const settingsPluginControl = pluginControl({
    text: "插件",
    navigationLabel: "设置",
  });
  const mainPluginControl = pluginControl({
    text: "插件",
    sidebarContent: true,
  });
  const entryDiscovery = {
    currentSidebarClassIndependent:
      resolvePluginEntry([unrelatedControl, currentSidebarControl]) === currentSidebarControl,
    accessibleNameOnly:
      resolvePluginEntry([unrelatedControl, accessibleOnlyControl]) === accessibleOnlyControl,
    legacyIconFallback:
      resolvePluginEntry([unrelatedControl, legacyIconControl]) === legacyIconControl,
    uniqueGlobalFallback:
      resolvePluginEntry([], [unrelatedControl, globalFallbackControl]) === globalFallbackControl,
    ambiguousGlobalRejected:
      resolvePluginEntry([], [globalFallbackControl, duplicateGlobalControl]) === null,
    settingsNavigationExcluded:
      resolvePluginEntry(
        [settingsPluginControl, mainPluginControl],
        [settingsPluginControl, mainPluginControl],
        api.taskBoardPluginEntryButtonForTest,
      ) === mainPluginControl,
    settingsOnlyRejected:
      resolvePluginEntry(
        [settingsPluginControl],
        [settingsPluginControl],
        api.taskBoardPluginEntryButtonForTest,
      ) === null,
  };
  document.querySelectorAll = originalQuerySelectorAll;
  const defaultTaskBoardEnabled = api.taskBoardFeatureEnabledForTest();
  api.setBackendSettingsForTest({ enhancementsEnabled: true, codexAppTaskBoard: false });
  const disabledBySwitch = !api.taskBoardFeatureEnabledForTest();
  api.resetReadState();
  api.reconcileRuntimeForTest();
  const activeViewClosedWhenDisabled = !api.activeForTest();
  api.setBackendSettingsForTest({ enhancementsEnabled: false, codexAppTaskBoard: true });
  const disabledByMaster = !api.taskBoardFeatureEnabledForTest();
  api.setBackendSettingsForTest({ enhancementsEnabled: true, codexAppTaskBoard: true });
  const restoredEnabled = api.taskBoardFeatureEnabledForTest();
  const statusSlot = {
    normal: api.statusPresentationForTest({
      loading: false,
      snapshotError: "",
      catalogError: "",
      moveFeedback: "",
      catalog: { warnings: [] },
    }),
    loading: api.statusPresentationForTest({
      loading: true,
      snapshotError: "任务快照加载失败",
      catalog: { warnings: [] },
    }),
    failed: api.statusPresentationForTest({
      loading: false,
      snapshotError: "任务快照加载失败",
      catalogError: "",
      moveFeedback: "",
      catalog: { warnings: [] },
    }),
    warning: api.statusPresentationForTest({
      loading: false,
      snapshotError: "",
      catalogError: "",
      moveFeedback: "",
      catalog: { warnings: [{ code: "codex_db_read_failed", count: 1 }] },
    }),
  };
  const latestCatalog = {
    warnings: [],
    projects: [],
    sessions: [{ sessionId: "session-1", title: "目录最新标题", cwd: "/repo", updatedAtMs: 20 }],
  };
  const linkedConversation = { sessionId: "session-1", title: "快照旧标题", cwd: "/repo", updatedAtMs: 10 };
  const latest = api.conversationProjection(linkedConversation, latestCatalog);
  const latestTitleMatchesSearch = api.taskMatchesQuery({
    title: "任务 A",
    project: { cwd: "/repo", label: "repo" },
    conversations: [linkedConversation],
  }, latestCatalog, "目录最新标题");
  const partial = api.conversationProjection(
    { sessionId: "session-missing", title: "保留标题", cwd: "/repo", updatedAtMs: 1 },
    { warnings: [{ code: "codex_db_read_failed", count: 1 }], projects: [], sessions: [] },
  );
  const complete = api.conversationProjection(
    { sessionId: "session-missing", title: "保留标题", cwd: "/repo", updatedAtMs: 1 },
    { warnings: [], projects: [], sessions: [] },
  );
  const conversationStatuses = {
    running: api.conversationStatusForTest({
      available: true,
      usageKnown: true,
      isRunning: true,
      unread: true,
    }).id,
    completedUnread: api.conversationStatusForTest({
      available: true,
      usageKnown: true,
      unread: true,
    }).id,
    completed: api.conversationStatusForTest({
      available: true,
      usageKnown: true,
    }).id,
    unknown: api.conversationStatusForTest({
      available: true,
      usageKnown: false,
      checking: false,
    }).id,
    unavailable: api.conversationStatusForTest({ available: false }).id,
  };
  const usageRequests = [];
  api.resetCreateStateForTest({
    snapshot: {
      status: "ok",
      schemaVersion: 1,
      revision: 1,
      tasks: [{
        id: "task-status",
        title: "状态任务",
        project: { cwd: "/repo", label: "repo" },
        status: "executing",
        order: 0,
        conversations: [linkedConversation],
      }],
    },
    catalog: { status: "ok", ...latestCatalog },
  });
  window.__codexElvesTaskBoardMock = {
    request(route, payload) {
      usageRequests.push({ route, payload });
      return { status: "ok", summary: { isRunning: true } };
    },
  };
  await api.refreshConversationStatusesForTest();
  const runningProjection = api.conversationProjection(linkedConversation, latestCatalog);
  conversationStatuses.usageRouteAndProjection =
    usageRequests.length === 1 &&
    usageRequests[0]?.route === "/thread-usage-summary" &&
    usageRequests[0]?.payload?.session_id === "session-1" &&
    runningProjection.status?.id === "running";
  const boundedConversations = Array.from({ length: 6 }, (_, index) => ({
    sessionId: `bounded-${index}`,
    title: `并发会话 ${index}`,
    cwd: "/repo",
    updatedAtMs: index,
  }));
  api.resetCreateStateForTest({
    snapshot: {
      status: "ok",
      schemaVersion: 1,
      revision: 2,
      tasks: [{
        id: "task-bounded-status",
        title: "并发状态任务",
        project: { cwd: "/repo", label: "repo" },
        status: "executing",
        order: 0,
        conversations: boundedConversations,
      }],
    },
    catalog: {
      status: "ok",
      warnings: [],
      projects: [],
      sessions: boundedConversations,
    },
  });
  let boundedActive = 0;
  let boundedMaxActive = 0;
  let boundedRequestCount = 0;
  const boundedPending = [];
  window.__codexElvesTaskBoardMock = {
    request(route) {
      if (route !== "/thread-usage-summary") throw new Error(`unexpected route ${route}`);
      boundedRequestCount += 1;
      boundedActive += 1;
      boundedMaxActive = Math.max(boundedMaxActive, boundedActive);
      return new Promise((resolve) => {
        boundedPending.push(() => {
          boundedActive -= 1;
          resolve({ status: "ok", summary: { isRunning: false } });
        });
      });
    },
  };
  const boundedRefresh = api.refreshConversationStatusesForTest();
  await Promise.resolve();
  const boundedFirstWave = boundedPending.length;
  while (boundedRequestCount < boundedConversations.length || boundedPending.length) {
    const batch = boundedPending.splice(0);
    if (!batch.length) {
      await Promise.resolve();
      continue;
    }
    batch.forEach((release) => release());
    await Promise.resolve();
  }
  await boundedRefresh;
  const boundedBeforeIdleRefresh = boundedRequestCount;
  await api.refreshConversationStatusesForTest();
  conversationStatuses.boundedConcurrency =
    boundedFirstWave === 4 &&
    boundedMaxActive === 4 &&
    boundedRequestCount === boundedConversations.length;
  conversationStatuses.idleRefreshSkipped =
    boundedRequestCount === boundedBeforeIdleRefresh;
  api.resetReadState();
  api.refreshRuntimeForTest();
  const activeAfterRefresh = api.activeForTest();
  api.resetReadState();
  const snapshot = deferred();
  const catalog = deferred();
  window.__codexElvesTaskBoardMock = {
    snapshot: () => snapshot.promise,
    catalog: () => catalog.promise,
  };
  const refresh = api.refresh();
  snapshot.resolve({
    status: "ok",
    schemaVersion: 1,
    revision: 3,
    tasks: [{
      id: "task-1",
      title: "先到的快照",
      project: { cwd: "/repo", label: "repo" },
      status: "new",
      order: 0,
      conversations: [],
    }],
  });
  await tick();
  const beforeCatalog = api.readState();
  catalog.resolve({
    status: "ok",
    projects: [],
    sessions: [{ sessionId: "session-1", title: "目录最新标题", cwd: "/repo", updatedAtMs: 20 }],
    warnings: [],
  });
  await refresh;
  const afterCatalog = api.readState();
  process.stdout.write(JSON.stringify({
    runtimeGate,
    entryDiscovery,
    featureSwitch: {
      defaultEnabled: defaultTaskBoardEnabled,
      disabledBySwitch,
      disabledByMaster,
      activeViewClosedWhenDisabled,
      restoredEnabled,
    },
    statusSlot,
    catalog: {
      latestTitle: latest.title,
      latestTitleMatchesSearch,
      partialMissingAvailable: partial.available,
      partialMissingLabel: partial.label,
      completeMissingAvailable: complete.available,
    },
    conversationStatuses,
    runtimeRefresh: {
      activeAfterRefresh,
    },
    read: {
      snapshotTitleBeforeCatalog: beforeCatalog.snapshot.tasks[0]?.title || "",
      catalogCountBeforeCatalog: beforeCatalog.catalog.sessions.length,
      loadingBeforeCatalog: beforeCatalog.loading,
      catalogCountAfterCatalog: afterCatalog.catalog.sessions.length,
      loadingAfterCatalog: afterCatalog.loading,
    },
  }));
  process.exit(0);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
"#,
    )
    .expect("task board harness should be written");

    let output = Command::new("node")
        .arg(&harness_path)
        .arg(&script_path)
        .output()
        .expect("node should run task board harness");
    assert!(
        output.status.success(),
        "node task board harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("task board harness stdout should be JSON")
}

fn run_task_board_move_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("task-board-move-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    std::fs::write(
        &harness_path,
        r##"
const scriptPath = process.argv[2];
globalThis.window = globalThis;
globalThis.Element = class Element {};
globalThis.HTMLElement = class HTMLElement extends Element {};
globalThis.HTMLButtonElement = class HTMLButtonElement extends HTMLElement {};
function selectorMatches(element, selector) {
  if (!element || element.nodeType !== 1) return false;
  const attributeMatches = [...selector.matchAll(/\[([^=\]]+)(?:="([^"]*)")?\]/g)];
  const classNames = [...selector.matchAll(/\.([a-zA-Z0-9_-]+)/g)].map((match) => match[1]);
  const tagName = selector.replace(/\[[^\]]+\]/g, "").replace(/\.[a-zA-Z0-9_-]+/g, "").trim();
  if (tagName && element.tagName !== tagName.toUpperCase()) return false;
  if (classNames.some((className) => !element.classList.contains(className))) return false;
  return attributeMatches.every((match) => {
    const value = element.getAttribute(match[1]);
    return match[2] === undefined ? value !== null : value === match[2];
  });
}
function descendantsOf(element) {
  const descendants = [];
  for (const child of element.children || []) descendants.push(child, ...descendantsOf(child));
  return descendants;
}
function node(tagName = "div") {
  const attributes = new Map();
  const listeners = new Map();
  const prototype = tagName === "button" ? HTMLButtonElement.prototype : HTMLElement.prototype;
  const value = Object.create(prototype);
  let classes = new Set();
  const get = (type) => listeners.get(type) || (listeners.set(type, new Set()), listeners.get(type));
  Object.assign(value, {
    nodeType: 1, tagName: String(tagName).toUpperCase(), children: [], dataset: {}, style: { setProperty() {}, removeProperty() {} },
    disabled: false, draggable: false, parentElement: null, textContent: "", innerHTML: "", value: "", tabIndex: 0,
    classList: {
      add(...names) { names.forEach((name) => classes.add(name)); },
      remove(...names) { names.forEach((name) => classes.delete(name)); },
      contains(name) { return classes.has(name); },
      toggle(name, force) { const next = force === undefined ? !classes.has(name) : Boolean(force); if (next) classes.add(name); else classes.delete(name); return next; }
    },
    appendChild(child) {
      child.parentElement?.remove?.();
      this.children.push(child);
      child.parentElement = this;
      return child;
    },
    append(...children) { children.forEach((child) => this.appendChild(child)); },
    replaceChildren(...children) { this.children.slice().forEach((child) => child.remove?.()); this.append(...children); },
    remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((child) => child !== this); this.parentElement = null; this.removed = true; },
    setAttribute(name, attributeValue) {
      const stringValue = String(attributeValue);
      attributes.set(String(name), stringValue);
      if (String(name).startsWith("data-")) this.dataset[String(name).slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = stringValue;
    },
    getAttribute(name) { return attributes.get(String(name)) || null; },
    removeAttribute(name) {
      attributes.delete(String(name));
      if (String(name).startsWith("data-")) delete this.dataset[String(name).slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())];
    },
    addEventListener(type, listener) { get(type).add(listener); },
    removeEventListener(type, listener) { get(type).delete(listener); },
    dispatchEvent(event) {
      event.target ||= this;
      event.currentTarget = this;
      event.preventDefault ||= function() { this.defaultPrevented = true; };
      event.stopPropagation ||= function() { this.cancelBubble = true; };
      get(event.type).forEach((listener) => listener.call(this, event));
      if (!event.cancelBubble && this.parentElement) this.parentElement.dispatchEvent(event);
      return !event.defaultPrevented;
    },
    click() { this.dispatchEvent({ type: "click" }); },
    focus() { document.activeElement = this; },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
    querySelectorAll(selector) { return descendantsOf(this).filter((child) => selectorMatches(child, selector)); },
    closest(selector) { let current = this; while (current) { if (selectorMatches(current, selector)) return current; current = current.parentElement; } return null; },
    matches(selector) { return selectorMatches(this, selector); },
    contains(other) { return other === this || descendantsOf(this).includes(other); },
  });
  Object.defineProperty(value, "className", {
    get() { return [...classes].join(" "); },
    set(next) { classes = new Set(String(next || "").split(/\s+/).filter(Boolean)); }
  });
  Object.defineProperty(value, "isConnected", { get() { return document.documentElement.contains(this); } });
  return value;
}
globalThis.MutationObserver = class MutationObserver { observe() {} disconnect() {} };
globalThis.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} };
globalThis.requestAnimationFrame = (callback) => { callback(); return 1; };
globalThis.cancelAnimationFrame = () => {};
window.addEventListener = () => {};
window.removeEventListener = () => {};
window.dispatchEvent = () => true;
const documentListeners = new Map();
const documentSet = (type) => documentListeners.get(type) || (documentListeners.set(type, new Set()), documentListeners.get(type));
globalThis.document = {
  readyState: "complete", scripts: [], visibilityState: "visible", documentElement: node("html"), body: node("body"), activeElement: null,
  createElement: (tag) => node(tag), createTextNode: (text) => { const value = node("#text"); value.nodeType = 3; value.textContent = String(text); return value; },
  querySelector(selector) { return this.documentElement.querySelector(selector); },
  querySelectorAll(selector) { return this.documentElement.querySelectorAll(selector); },
  addEventListener(type, listener) { documentSet(type).add(listener); },
  removeEventListener(type, listener) { documentSet(type).delete(listener); },
  dispatchEvent(event) { event.target ||= this; documentSet(event.type).forEach((listener) => listener(event)); return true; },
  listenerCount(type) { return documentListeners.get(type)?.size || 0; },
};
document.documentElement.appendChild(document.body);
let mainSurface = node("main");
mainSurface.setAttribute("data-app-shell-main-surface", "true");
document.body.appendChild(mainSurface);
globalThis.getComputedStyle = () => ({ display: "block", visibility: "visible", pointerEvents: "auto" });
globalThis.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.location = { href: "https://codex.test/local/thread-12345678", pathname: "/local/thread-12345678", search: "", hash: "", protocol: "https:" };
globalThis.navigator = { userAgent: "node-test" };
globalThis.performance = { getEntriesByType: () => [] };
globalThis.CustomEvent = class CustomEvent { constructor(type, options = {}) { this.type = type; this.detail = options.detail; } };
globalThis.Event = class Event { constructor(type) { this.type = type; } };
window.__CODEX_ELVES_TEST_TASK_BOARD__ = true;
window.__codexSessionDeleteBridge = async (path) => path === "/settings/get"
  ? { launchMode: "direct", enhancementsEnabled: true, providerSyncEnabled: true }
  : { status: "ok", ids: [] };
require(scriptPath);
const api = window.__codexElvesTaskBoardTest;
if (!api) throw new Error("task board test api unavailable");
function task(id, status, order, title = id) {
  return { id, title, project: { cwd: "/repo", label: "repo" }, status, order, conversations: [], createdAtMs: 1, updatedAtMs: 1 };
}
function snapshot(revision, tasks) { return { status: "ok", schemaVersion: 1, revision, tasks }; }
function base() { return snapshot(7, [task("a", "new", 0, "alpha"), task("b", "new", 1, "beta"), task("c", "new", 2, "gamma"), task("d", "planning", 0)]); }
function reset(mock = {}, nextSnapshot = base()) {
  window.__codexElvesTaskBoardMock = mock;
  api.resetMoveStateForTest(nextSnapshot);
}
function deferred() { let resolve; const promise = new Promise((next) => { resolve = next; }); return { promise, resolve }; }
function settle() { return new Promise((resolve) => setTimeout(resolve, 0)); }
(async () => {
  reset();
  const crossIndex = api.moveTargetIndexForTest("a", "planning", "d");
  const sameBefore = api.moveTargetIndexForTest("c", "new", "a");
  const sameEnd = api.moveTargetIndexForTest("a", "new");
  const selfIndex = api.moveTargetIndexForTest("b", "new", "b");
  api.setMoveFiltersForTest("alpha", "/repo");
  const filteredIndex = api.moveTargetIndexForTest("c", "new", "a");
  const payloadLog = [];
  reset({ request(route, payload) {
    if (route !== "/task-board/task-move") throw new Error(`unexpected ${route}`);
    payloadLog.push(payload);
    return snapshot(8, [task("b", "new", 0), task("c", "new", 1), task("a", "planning", 0), task("d", "planning", 1)]);
  }});
  await api.moveTaskForTest("a", "planning", crossIndex);
  const payloads = {
    crossColumn: JSON.stringify(payloadLog[0]) === JSON.stringify({ taskId: "a", toStatus: "planning", targetIndex: 0, expectedRevision: 7 }),
    sameColumn: sameBefore === 0,
    filteredIndex: filteredIndex === 0,
    zeroAndEnd: crossIndex === 0 && sameEnd === 2,
    selfDropNoOp: selfIndex === 1,
  };
  const success = {
    serverSnapshotCorrectsOptimistic: api.moveStateForTest().revision === 8 &&
      api.moveStateForTest().tasks.find((item) => item.id === "a")?.order === 0,
  };
  const menuPayloads = [];
  reset({ request(route, payload) { menuPayloads.push(payload); return snapshot(8, base().tasks); } });
  api.openStatusMenuForTest("a");
  const menuInitial = api.statusMenuStateForTest();
  api.dispatchStatusMenuKeyForTest("ArrowDown");
  const menuDown = api.statusMenuStateForTest();
  api.dispatchStatusMenuKeyForTest("ArrowUp");
  const menuUp = api.statusMenuStateForTest();
  api.dispatchStatusMenuKeyForTest("Home");
  const menuHome = api.statusMenuStateForTest();
  api.dispatchStatusMenuKeyForTest("End");
  const menuEnd = api.statusMenuStateForTest();
  api.dispatchStatusMenuKeyForTest("Enter");
  await Promise.resolve();
  api.openStatusMenuForTest("a");
  api.dispatchStatusMenuKeyForTest("Escape");
  const menu = {
    fiveStatuses: menuInitial.itemCount === 5,
    keyboardAndFocus: menuInitial.focusedIndex === 0 && menuDown.focusedIndex === 1 &&
      menuUp.focusedIndex === 0 && menuHome.focusedIndex === 0 && menuEnd.focusedIndex === 4 &&
      menuPayloads[0]?.toStatus === "done" && menuPayloads[0]?.targetIndex === 0 &&
      !api.statusMenuStateForTest().open,
  };
  reset({ request() { return { status: "failed", code: "task_board_unavailable", message: "down" }; } });
  await api.moveTaskForTest("a", "planning", 0);
  const failure = {
    rollbackAndBusyRelease: api.moveStateForTest().revision === 7 &&
      api.moveStateForTest().tasks.find((item) => item.id === "a")?.status === "new" &&
      !api.moveStateForTest().busy && api.moveStateForTest().feedback.includes("恢复"),
  };
  let conflictCalls = 0;
  reset({ request() { conflictCalls += 1; return { status: "conflict", code: "revision_conflict", message: "changed", schemaVersion: 1, revision: 9, tasks: [task("a", "review", 0)] }; } });
  await api.moveTaskForTest("a", "planning", 0);
  const conflict = {
    adoptsLatestWithoutRetry: conflictCalls === 1 && api.moveStateForTest().revision === 9 &&
      api.moveStateForTest().tasks[0]?.status === "review" && api.moveStateForTest().feedback.includes("重试"),
  };
  reset({ request() { return { status: "conflict", code: "revision_conflict", message: "bad" }; } });
  await api.moveTaskForTest("a", "planning", 0);
  conflict.malformedRollsBack =
    api.moveStateForTest().revision === 7 && api.moveStateForTest().tasks.find((item) => item.id === "a")?.status === "new";
  const pending = deferred();
  let deferredCalls = 0;
  reset({ request() { deferredCalls += 1; return pending.promise; } });
  const first = api.moveTaskForTest("a", "planning", 0);
  const second = api.moveTaskForTest("b", "planning", 0);
  const blocked = await second;
  api.refreshRuntimeForTest();
  pending.resolve(snapshot(10, [task("a", "planning", 0)]));
  await first;
  const lifecycle = {
    staleDeferredAndCleanup: api.moveStateForTest().revision === 7 && !api.moveStateForTest().busy && !api.moveStateForTest().menuOpen,
    duplicateMoveBlocked: deferredCalls === 1 && blocked.status === "blocked",
  };
  const dragPending = deferred();
  reset({ request() { return dragPending.promise; } });
  const dragMove = api.moveTaskForTest("a", "planning", 0);
  api.dragEndForTest();
  const dragStillBusy = api.moveStateForTest().busy;
  dragPending.resolve(snapshot(8, [task("a", "planning", 0)]));
  await dragMove;
  lifecycle.dragEndKeepsMoveAlive = dragStillBusy && api.moveStateForTest().revision === 8;

  const readSnapshot = deferred();
  const moveSnapshot = deferred();
  let readCalls = 0;
  reset({ request(route) {
    if (route === "/task-board/snapshot") { readCalls += 1; return readSnapshot.promise; }
    if (route === "/task-board/session-catalog") return { status: "ok", projects: [], sessions: [], warnings: [] };
    if (route === "/task-board/task-move") return moveSnapshot.promise;
    throw new Error(`unexpected ${route}`);
  }});
  const readBeforeMove = api.refresh();
  const pendingMove = api.moveTaskForTest("a", "planning", 0);
  readSnapshot.resolve(snapshot(99, [task("a", "done", 0)]));
  const skippedRefresh = await api.refresh();
  moveSnapshot.resolve(snapshot(8, [task("a", "planning", 0)]));
  await Promise.all([readBeforeMove, pendingMove]);
  const reads = {
    beforeMoveCannotOverwrite: api.moveStateForTest().revision === 8 && api.moveStateForTest().tasks[0]?.status === "planning",
    refreshDuringMoveSkipped: readCalls === 1 && Array.isArray(skippedRefresh) && skippedRefresh.length === 0,
  };
  reset({ request(route) {
    if (route === "/task-board/snapshot") return snapshot(99, [task("a", "done", 0)]);
    if (route === "/task-board/session-catalog") return { status: "ok", projects: [], sessions: [], warnings: [] };
    if (route === "/task-board/task-move") return { status: "failed", code: "task_board_unavailable", message: "down" };
    throw new Error(`unexpected ${route}`);
  }});
  const ignoredRead = api.refresh();
  await api.moveTaskForTest("a", "planning", 0);
  await ignoredRead;
  reads.moveFailureKeepsReadOut = api.moveStateForTest().revision === 7 && api.moveStateForTest().tasks[0]?.status === "new";

  function mountDom(mock = {}, nextSnapshot = base()) {
    reset(mock, nextSnapshot);
    api.setMoveFiltersForTest();
    api.reconcileRuntimeForTest();
    return mainSurface.querySelector('[data-codex-task-board-root="true"]');
  }
  function event(type, target) {
    return {
      type,
      target,
      dataTransfer: { setData() {} },
      preventDefault() { this.defaultPrevented = true; },
      stopPropagation() { this.cancelBubble = true; },
    };
  }
  const multiConversationTask = task("multi", "new", 0, "多会话任务");
  multiConversationTask.conversations = [
    { sessionId: "session-inline-1", title: "第一条关联会话" },
    { sessionId: "session-inline-2", title: "第二条关联会话" },
  ];
  mountDom({}, snapshot(7, [multiConversationTask]));
  const multiConversationCard = mainSurface.querySelector(
    '.codex-task-board-card[data-task-board-id="multi"]',
  );
  const multiConversationRows = Array.from(
    multiConversationCard?.querySelectorAll?.(".codex-task-board-conversation-row") || [],
  );
  const inlineConversationTitles = multiConversationRows.map(
    (row) => row.querySelector?.(".codex-task-board-conversation-title")?.textContent || "",
  );
  const allConversationsRenderedInline =
    multiConversationRows.length === 2 &&
    JSON.stringify(inlineConversationTitles) ===
      JSON.stringify(["第一条关联会话", "第二条关联会话"]) &&
    multiConversationCard?.querySelectorAll?.(".codex-task-board-conversation-state").length === 2 &&
    multiConversationCard?.querySelectorAll?.(".codex-task-board-conversation-remove").length === 2 &&
    !multiConversationCard?.querySelector?.(".codex-task-board-conversation-summary") &&
    !document.body.querySelector?.(".codex-task-board-conversation-popover");

  const domPayloads = [];
  const domPending = deferred();
  mountDom({ request(route, payload) {
    domPayloads.push({ route, payload });
    return domPending.promise;
  }});
  const dragCard = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]');
  const planningList = mainSurface.querySelector('.codex-task-board-card-list[data-task-board-status="planning"]');
  const planningCard = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="d"]');
  if (!dragCard || !planningList || !planningCard) {
    throw new Error(`drag DOM unavailable: root=${!!mainSurface.querySelector('[data-codex-task-board-root="true"]')} cards=${mainSurface.querySelectorAll(".codex-task-board-card").length} lists=${mainSurface.querySelectorAll(".codex-task-board-card-list").length}`);
  }
  const conversationRow = dragCard.querySelector(".codex-task-board-conversations");
  const cardFooter = dragCard.querySelector(".codex-task-board-card-footer");
  const cardStructureMatchesDebug =
    conversationRow?.parentElement === dragCard &&
    cardFooter?.parentElement === dragCard &&
    dragCard.children.indexOf(conversationRow) < dragCard.children.indexOf(cardFooter) &&
    cardFooter.children.length === 2 &&
    cardFooter.children[0]?.classList.contains("codex-task-board-card-add") &&
    cardFooter.children[1]?.classList.contains("codex-task-board-card-move");
  dragCard.dispatchEvent(event("dragstart", dragCard));
  planningList.dispatchEvent(event("dragover", planningList));
  const activeBeforeDrop = planningList.getAttribute("data-drop-active") === "true";
  planningCard.dispatchEvent(event("drop", planningCard));
  dragCard.dispatchEvent(event("dragend", dragCard));
  const optimistic = api.moveStateForTest();
  const optimisticOrdersContinuous =
    optimistic.tasks.filter((item) => item.status === "new").sort((left, right) => left.order - right.order).map((item) => item.order).join(",") === "0,1" &&
    optimistic.tasks.filter((item) => item.status === "planning").sort((left, right) => left.order - right.order).map((item) => item.order).join(",") === "0,1";
  const dragEndKeepsDomMoveAlive = api.moveStateForTest().busy;
  domPending.resolve(snapshot(8, [task("b", "new", 0), task("c", "new", 1), task("a", "planning", 0), task("d", "planning", 1)]));
  await settle();

  const sameColumnPayloads = [];
  mountDom({ request(route, payload) { sameColumnPayloads.push(payload); return snapshot(8, base().tasks); }});
  const sameDragCard = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]');
  const newList = mainSurface.querySelector('.codex-task-board-card-list[data-task-board-status="new"]');
  sameDragCard.dispatchEvent(event("dragstart", sameDragCard));
  newList.dispatchEvent(event("drop", newList));
  await settle();
  const selfRoot = mountDom({ request(route, payload) { sameColumnPayloads.push(payload); return snapshot(8, base().tasks); }});
  const selfCard = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="b"]');
  if (!selfCard) throw new Error(`self-drop card unavailable: ${mainSurface.querySelectorAll(".codex-task-board-card").map((card) => card.getAttribute("data-task-board-id")).join(",")}`);
  selfCard.dispatchEvent(event("dragstart", selfCard));
  selfCard.dispatchEvent(event("drop", selfCard));
  await settle();

  const menuPayloadsDom = [];
  mountDom({ request(route, payload) { menuPayloadsDom.push(payload); return snapshot(8, [task("a", "done", 0)]); }});
  const menuTrigger = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]').querySelector(".codex-task-board-card-move");
  menuTrigger.click();
  const menuNode = document.body.querySelector(".codex-task-board-status-menu");
  const menuOutsideMain = !!menuNode && document.body.contains(menuNode) && !mainSurface.contains(menuNode);
  document.dispatchEvent({ type: "keydown", key: "End", preventDefault() {}, stopPropagation() {} });
  document.dispatchEvent({ type: "keydown", key: "Enter", preventDefault() {}, stopPropagation() {} });
  await settle();
  const focusReturned = document.activeElement === mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]').querySelector(".codex-task-board-card-move");
  const escapeTrigger = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]').querySelector(".codex-task-board-card-move");
  escapeTrigger.click();
  document.dispatchEvent({ type: "keydown", key: "Escape", preventDefault() {}, stopPropagation() {} });
  const escapeReturnsOriginal = document.activeElement === escapeTrigger;

  const replacementPending = deferred();
  mountDom({ request() { return replacementPending.promise; }});
  const replacementCard = mainSurface.querySelector('.codex-task-board-card[data-task-board-id="a"]');
  const replacementList = mainSurface.querySelector('.codex-task-board-card-list[data-task-board-status="planning"]');
  replacementCard.dispatchEvent(event("dragstart", replacementCard));
  replacementList.dispatchEvent(event("drop", replacementList));
  const replacementHost = node("main");
  replacementHost.setAttribute("data-app-shell-main-surface", "true");
  mainSurface.remove();
  document.body.appendChild(replacementHost);
  mainSurface = replacementHost;
  api.reconcileRuntimeForTest();
  replacementPending.resolve(snapshot(10, [task("a", "planning", 0)]));
  await settle();
  const replacementCleanup =
    api.moveStateForTest().revision === 7 &&
    api.moveStateForTest().tasks.find((item) => item.id === "a")?.status === "new" &&
    !api.moveStateForTest().busy;

  const dom = {
    dragPathExactPayload: activeBeforeDrop &&
      JSON.stringify(domPayloads[0]) === JSON.stringify({ route: "/task-board/task-move", payload: { taskId: "a", toStatus: "planning", targetIndex: 0, expectedRevision: 7 } }),
    dragEndKeepsMoveAlive: dragEndKeepsDomMoveAlive,
    optimisticOrdersContinuous,
    sameColumnDownward: sameColumnPayloads[0]?.taskId === "a" && sameColumnPayloads[0]?.toStatus === "new" && sameColumnPayloads[0]?.targetIndex === 2,
    selfDropNoRequest: sameColumnPayloads.length === 1,
    allConversationsRenderedInline,
    cardStructureMatchesDebug,
    menuOutsideMain,
    enterMovesAndRestoresFocus: menuPayloadsDom[0]?.toStatus === "done" && focusReturned,
    escapeRestoresOriginalFocus: escapeReturnsOriginal,
    mainReplacementRollsBackAndIgnoresLateResult: replacementCleanup,
  };
  process.stdout.write(JSON.stringify({ payloads, menu, success, failure, conflict, lifecycle, reads, dom }));
  process.exit(0);
})().catch((error) => { console.error(error); process.exit(1); });
"##,
    )
    .expect("task board move harness should be written");
    let output = Command::new("node")
        .arg(&harness_path)
        .arg(&script_path)
        .output()
        .expect("node should run task board move harness");
    assert!(
        output.status.success(),
        "node task board move harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("task board move harness stdout should be JSON")
}

fn run_task_board_open_session_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("task-board-open-session-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    std::fs::write(
        &harness_path,
        r##"
const scriptPath = process.argv[2];
globalThis.window = globalThis;
globalThis.Element = class Element {};
globalThis.HTMLElement = class HTMLElement extends Element {};
globalThis.HTMLButtonElement = class HTMLButtonElement extends HTMLElement {};
function matches(element, selector) {
  return selector.split(",").some((raw) => {
    const value = raw.trim();
    if (!value || !element || element.nodeType !== 1) return false;
    const attributes = [...value.matchAll(/\[([^=\]]+)(?:=(?:"([^"]*)"|'([^']*)'))?\]/g)];
    const classes = [...value.matchAll(/\.([a-zA-Z0-9_-]+)/g)].map((match) => match[1]);
    const tag = value.replace(/\[[^\]]+\]/g, "").replace(/\.[a-zA-Z0-9_-]+/g, "").trim();
    if (tag && element.tagName !== tag.toUpperCase()) return false;
    if (classes.some((className) => !element.classList.contains(className))) return false;
    return attributes.every((match) => {
      const actual = element.getAttribute(match[1]);
      const expected = match[2] ?? match[3];
      return expected === undefined ? actual !== null : actual === expected;
    });
  });
}
function descendants(root) {
  const result = [];
  for (const child of root.children || []) result.push(child, ...descendants(child));
  return result;
}
function node(tagName = "div") {
  const attributes = new Map();
  const listeners = new Map();
  const prototype = tagName === "button" ? HTMLButtonElement.prototype : HTMLElement.prototype;
  const element = Object.create(prototype);
  let classes = new Set();
  const listenerSet = (type) => listeners.get(type) || (listeners.set(type, new Set()), listeners.get(type));
  Object.assign(element, {
    nodeType: 1,
    tagName: String(tagName).toUpperCase(),
    children: [],
    parentElement: null,
    dataset: {},
    style: { setProperty() {}, removeProperty() {} },
    disabled: false,
    textContent: "",
    innerHTML: "",
    classList: {
      add(...names) { names.forEach((name) => classes.add(name)); },
      remove(...names) { names.forEach((name) => classes.delete(name)); },
      contains(name) { return classes.has(name); },
      toggle(name, force) {
        const next = force === undefined ? !classes.has(name) : Boolean(force);
        if (next) classes.add(name); else classes.delete(name);
        return next;
      },
    },
    appendChild(child) {
      child.parentElement?.remove?.();
      this.children.push(child);
      child.parentElement = this;
      return child;
    },
    append(...children) { children.forEach((child) => this.appendChild(child)); },
    replaceChildren(...children) { this.children.slice().forEach((child) => child.remove?.()); this.append(...children); },
    remove() {
      if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
      this.parentElement = null;
      this.removed = true;
    },
    setAttribute(name, value) {
      const stringValue = String(value);
      attributes.set(String(name), stringValue);
      if (String(name).startsWith("data-")) this.dataset[String(name).slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = stringValue;
    },
    getAttribute(name) { return attributes.has(String(name)) ? attributes.get(String(name)) : null; },
    hasAttribute(name) { return attributes.has(String(name)); },
    removeAttribute(name) { attributes.delete(String(name)); },
    addEventListener(type, listener) { listenerSet(type).add(listener); },
    removeEventListener(type, listener) { listenerSet(type).delete(listener); },
    dispatchEvent(event) {
      event.target ||= this;
      event.currentTarget = this;
      event.preventDefault ||= function() { this.defaultPrevented = true; };
      for (const listener of [...listenerSet(event.type)]) listener.call(this, event);
      if (!event.cancelBubble && this.parentElement) this.parentElement.dispatchEvent(event);
      return !event.defaultPrevented;
    },
    click() { if (!this.disabled) this.dispatchEvent({ type: "click" }); },
    focus() { document.activeElement = this; },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
    querySelectorAll(selector) { return descendants(this).filter((child) => matches(child, selector)); },
    matches(selector) { return matches(this, selector); },
    closest(selector) {
      let current = this;
      while (current) {
        if (matches(current, selector)) return current;
        current = current.parentElement;
      }
      return null;
    },
    contains(target) { return target === this || descendants(this).includes(target); },
    getBoundingClientRect() { return { left: 0, top: 0, right: 100, bottom: 30, width: 100, height: 30 }; },
  });
  Object.defineProperty(element, "className", {
    get() { return [...classes].join(" "); },
    set(value) { classes = new Set(String(value || "").split(/\s+/).filter(Boolean)); },
  });
  Object.defineProperty(element, "isConnected", { get() { return document.documentElement.contains(this); } });
  return element;
}
const documentListeners = new Map();
const documentListenerSet = (type) => documentListeners.get(type) || (documentListeners.set(type, new Set()), documentListeners.get(type));
globalThis.document = {
  readyState: "complete",
  scripts: [],
  visibilityState: "visible",
  documentElement: node("html"),
  body: node("body"),
  activeElement: null,
  createElement(tagName) { return node(tagName); },
  createTextNode(text) { const value = node("span"); value.nodeType = 3; value.textContent = String(text); return value; },
  querySelector(selector) { return this.documentElement.querySelector(selector); },
  querySelectorAll(selector) { return this.documentElement.querySelectorAll(selector); },
  addEventListener(type, listener) { documentListenerSet(type).add(listener); },
  removeEventListener(type, listener) { documentListenerSet(type).delete(listener); },
  listenerCount(type) { return documentListeners.get(type)?.size || 0; },
  dispatchEvent(event) {
    event.target ||= this;
    event.preventDefault ||= function() { this.defaultPrevented = true; };
    for (const listener of [...documentListenerSet(event.type)]) listener.call(this, event);
    return !event.defaultPrevented;
  },
};
document.documentElement.appendChild(document.body);
globalThis.MutationObserver = class MutationObserver { observe() {} disconnect() {} };
globalThis.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} };
globalThis.requestAnimationFrame = (callback) => { callback(); return 1; };
globalThis.cancelAnimationFrame = () => {};
window.addEventListener = () => {};
window.removeEventListener = () => {};
window.dispatchEvent = () => true;
globalThis.getComputedStyle = () => ({ display: "block", visibility: "visible", pointerEvents: "auto" });
globalThis.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.sessionStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.location = { href: "https://codex.test/local/thread", pathname: "/local/thread", search: "", hash: "", protocol: "https:" };
globalThis.navigator = { userAgent: "node-test" };
globalThis.performance = { getEntriesByType: () => [] };
globalThis.CustomEvent = class CustomEvent { constructor(type, options = {}) { this.type = type; this.detail = options.detail; } };
globalThis.Event = class Event { constructor(type, options = {}) { this.type = type; Object.assign(this, options); } };
let now = 0;
const waits = [];
let mountThreadOnWait = false;
let replaceRuntimeOnWait = false;
window.__codexElvesTaskBoardNativeClock = {
  now: () => now,
  wait(delay) {
    waits.push(delay);
    now += delay;
    if (replaceRuntimeOnWait) window.__codexElvesTaskBoardNativeRuntimeId += 1;
    if (mountThreadOnWait) {
      mountThreadOnWait = false;
      addThread("session-expand");
    }
  },
};
window.__CODEX_ELVES_TEST_TASK_BOARD__ = true;
window.__codexSessionDeleteBridge = async (path) => path === "/settings/get"
  ? { launchMode: "direct", enhancementsEnabled: true, providerSyncEnabled: true }
  : { status: "ok", ids: [] };

let projectClicks = 0;
const projectRow = node("button");
projectRow.setAttribute("data-app-action-sidebar-project-row", "true");
projectRow.setAttribute("data-app-action-sidebar-project-id", "c:/repo-a");
projectRow.setAttribute("aria-expanded", "true");
projectRow.addEventListener("click", () => {
  projectClicks += 1;
  projectRow.setAttribute("aria-expanded", "true");
  projectRow.removeAttribute("data-app-action-sidebar-project-collapsed");
});
document.body.appendChild(projectRow);
const threadClicks = new Map();
function removeThreads() {
  document.querySelectorAll("[data-app-action-sidebar-thread-id]").forEach((row) => row.remove());
}
function addThread(id) {
  const row = node("button");
  row.setAttribute("data-app-action-sidebar-thread-id", id);
  row.addEventListener("click", () => threadClicks.set(id, (threadClicks.get(id) || 0) + 1));
  document.body.appendChild(row);
  return row;
}
function snapshot() {
  return {
    status: "ok",
    schemaVersion: 1,
    revision: 7,
    tasks: [{
      id: "task-open",
      title: "打开关联会话",
      project: { cwd: "c:/repo-a", label: "项目 A" },
      status: "new",
      order: 0,
      conversations: [
        { sessionId: "session-raw", title: "原始 ID" },
        { sessionId: "session-local", title: "本地 ID" },
        { sessionId: "session-expand", title: "折叠项目" },
      ],
    }],
  };
}
function catalog() {
  return {
    status: "ok",
    projects: [{ cwd: "c:/repo-a", label: "项目 A" }],
    sessions: [
      { sessionId: "session-raw", title: "原始 ID", cwd: "c:/repo-a", updatedAtMs: 3 },
      { sessionId: "session-local", title: "本地 ID", cwd: "c:/repo-a", updatedAtMs: 2 },
      { sessionId: "session-expand", title: "折叠项目", cwd: "c:/repo-a", updatedAtMs: 1 },
    ],
    warnings: [],
  };
}
require(scriptPath);
const api = window.__codexElvesTaskBoardTest;
if (!api) throw new Error("task board test api unavailable");
function reset(options = {}) {
  now = 0;
  waits.length = 0;
  mountThreadOnWait = false;
  replaceRuntimeOnWait = false;
  projectClicks = 0;
  threadClicks.clear();
  projectRow.setAttribute("aria-expanded", "true");
  projectRow.removeAttribute("data-app-action-sidebar-project-collapsed");
  removeThreads();
  window.__codexElvesTaskBoardNativeAdapter = options.adapter || null;
  api.resetCreateStateForTest({
    snapshot: options.snapshot || snapshot(),
    catalog: options.catalog || catalog(),
  });
}
(async () => {
  reset();
  addThread("session-raw");
  const raw = await api.nativeOpenSessionForTest("session-raw");
  const mounted = {
    rawIdClickedOnce: raw.status === "ok" && threadClicks.get("session-raw") === 1 && projectClicks === 0,
    localIdClickedOnce: false,
  };
  removeThreads();
  addThread("local:session-local");
  const local = await api.nativeOpenSessionForTest("session-local");
  mounted.localIdClickedOnce = local.status === "ok" && threadClicks.get("local:session-local") === 1 && projectClicks === 0;

  reset({ catalog: { status: "ok", projects: [{ cwd: "c:/repo-a", label: "项目 A" }], sessions: [], warnings: [] } });
  projectRow.setAttribute("aria-expanded", "false");
  projectRow.setAttribute("data-app-action-sidebar-project-collapsed", "true");
  mountThreadOnWait = true;
  const expandedResult = await api.nativeOpenSessionForTest("session-expand");
  const expanded = {
    projectThenThreadClickedOnce:
      expandedResult.status === "ok" && projectClicks === 1 && threadClicks.get("session-expand") === 1 &&
      waits.reduce((sum, delay) => sum + delay, 0) <= 5000,
  };

  reset();
  const beforeFailure = JSON.stringify(api.createSnapshotForTest());
  const deadlineResult = await api.nativeOpenSessionForTest("session-expand");
  const deadline = {
    fiveSecondsAndStableFailure:
      deadlineResult.code === "session_unavailable" &&
      waits.reduce((sum, delay) => sum + delay, 0) === 5000 &&
      JSON.stringify(api.createSnapshotForTest()) === beforeFailure,
  };

  reset({ catalog: { status: "ok", projects: [], sessions: [], warnings: [] } });
  const missingSession = await api.nativeOpenSessionForTest("does-not-exist");
  const errors = {
    missingSessionStable: missingSession.code === "session_unavailable",
    missingProjectStable: false,
  };
  reset();
  projectRow.remove();
  const missingProject = await api.nativeOpenSessionForTest("session-expand");
  errors.missingProjectStable = missingProject.code === "native_navigation_unavailable";
  document.body.appendChild(projectRow);

  reset();
  projectRow.setAttribute("aria-expanded", "false");
  projectRow.setAttribute("data-app-action-sidebar-project-collapsed", "true");
  replaceRuntimeOnWait = true;
  const runtimeResult = await api.nativeOpenSessionForTest("session-expand");
  const runtimeReplacement = { stableFailure: runtimeResult.code === "runtime_replaced" };
  window.__codexElvesTaskBoardNativeRuntimeId -= 1;

  reset();
  addThread("session-raw");
  const beforeRepeat = JSON.stringify(api.createSnapshotForTest());
  const first = await api.nativeOpenSessionForTest("session-raw");
  const second = await api.nativeOpenSessionForTest("session-raw");
  await api.openConversationForTest({ sessionId: "session-raw", title: "原始 ID" });
  const repeat = {
    safeAndDataPreserved:
      first.status === "ok" && second.status === "ok" &&
      threadClicks.get("session-raw") === 3 &&
      JSON.stringify(api.createSnapshotForTest()) === beforeRepeat,
  };

  let seamCalls = 0;
  reset({ adapter: { openSession(sessionId) { seamCalls += 1; return { status: "ok", sessionId }; } } });
  await api.openConversationForTest({ sessionId: "session-raw", title: "原始 ID" });
  const seam = { injectedAdapterStillUsed: seamCalls === 1 };
  process.stdout.write(JSON.stringify({ mounted, expanded, deadline, errors, runtimeReplacement, repeat, seam }));
  process.exit(0);
})().catch((error) => { process.stderr.write(String(error?.stack || error)); process.exit(1); });
"##,
    )
    .expect("task board open session harness should be written");
    let output = Command::new("node")
        .arg(&harness_path)
        .arg(&script_path)
        .output()
        .expect("node should run task board open session harness");
    assert!(
        output.status.success(),
        "node task board open session harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("open session harness stdout should be JSON")
}

fn run_task_board_native_create_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("task-board-native-create-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    std::fs::write(
        &harness_path,
        r##"
const scriptPath = process.argv[2];
globalThis.window = globalThis;
globalThis.Element = class Element {};
globalThis.HTMLElement = class HTMLElement extends Element {};
globalThis.HTMLButtonElement = class HTMLButtonElement extends HTMLElement {};
globalThis.Event = class Event {
  constructor(type, options = {}) { this.type = type; Object.assign(this, options); }
};
globalThis.KeyboardEvent = class KeyboardEvent extends Event {};
function matches(element, selector) {
  return selector.split(",").some((part) => {
    const value = part.trim();
    if (!value || !element || element.nodeType !== 1) return false;
    const attributes = [...value.matchAll(/\[([^=\]]+)(?:=(?:"([^"]*)"|'([^']*)'))?\]/g)];
    const classes = [...value.matchAll(/\.([a-zA-Z0-9_-]+)/g)].map((match) => match[1]);
    const tag = value.replace(/\[[^\]]+\]/g, "").replace(/\.[a-zA-Z0-9_-]+/g, "").trim();
    if (tag && element.tagName !== tag.toUpperCase()) return false;
    if (classes.some((className) => !element.classList.contains(className))) return false;
    return attributes.every((match) => {
      const actual = element.getAttribute(match[1]);
      const expected = match[2] ?? match[3];
      return expected === undefined ? actual !== null : actual === expected;
    });
  });
}
function descendants(root) {
  const result = [];
  for (const child of root.children || []) result.push(child, ...descendants(child));
  return result;
}
function node(tagName = "div") {
  const attributes = new Map();
  const listeners = new Map();
  const prototype = tagName === "button" ? HTMLButtonElement.prototype : HTMLElement.prototype;
  const element = Object.create(prototype);
  let classNames = new Set();
  const listenerSet = (type) => listeners.get(type) || (listeners.set(type, new Set()), listeners.get(type));
  Object.assign(element, {
    nodeType: 1,
    tagName: String(tagName).toUpperCase(),
    children: [],
    parentElement: null,
    dataset: {},
    style: { setProperty() {}, removeProperty() {} },
    disabled: false,
    hidden: false,
    value: "",
    checked: false,
    textContent: "",
    innerHTML: "",
    tabIndex: 0,
    classList: {
      add(...names) { names.forEach((name) => classNames.add(name)); },
      remove(...names) { names.forEach((name) => classNames.delete(name)); },
      contains(name) { return classNames.has(name); },
      toggle(name, force) {
        const next = force === undefined ? !classNames.has(name) : Boolean(force);
        if (next) classNames.add(name); else classNames.delete(name);
        return next;
      },
    },
    appendChild(child) {
      child.parentElement?.remove?.();
      this.children.push(child);
      child.parentElement = this;
      return child;
    },
    append(...children) { children.forEach((child) => this.appendChild(child)); },
    replaceChildren(...children) { this.children.slice().forEach((child) => child.remove?.()); this.append(...children); },
    remove() {
      if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
      this.parentElement = null;
      this.removed = true;
    },
    setAttribute(name, value) {
      const stringValue = String(value);
      attributes.set(String(name), stringValue);
      if (String(name).startsWith("data-")) this.dataset[String(name).slice(5).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = stringValue;
    },
    getAttribute(name) { return attributes.has(String(name)) ? attributes.get(String(name)) : null; },
    hasAttribute(name) { return attributes.has(String(name)); },
    removeAttribute(name) { attributes.delete(String(name)); },
    toggleAttribute(name, force) {
      if (force === false) attributes.delete(String(name));
      else attributes.set(String(name), "");
    },
    addEventListener(type, listener) { listenerSet(type).add(listener); },
    removeEventListener(type, listener) { listenerSet(type).delete(listener); },
    dispatchEvent(event) {
      event.target ||= this;
      event.currentTarget = this;
      event.preventDefault ||= function() { this.defaultPrevented = true; };
      event.stopPropagation ||= function() { this.cancelBubble = true; };
      for (const listener of [...listenerSet(event.type)]) listener.call(this, event);
      if (!event.cancelBubble && this.parentElement) this.parentElement.dispatchEvent(event);
      return !event.defaultPrevented;
    },
    click() { if (!this.disabled) this.dispatchEvent({ type: "click" }); },
    focus() { document.activeElement = this; },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
    querySelectorAll(selector) { return descendants(this).filter((child) => matches(child, selector)); },
    matches(selector) { return matches(this, selector); },
    closest(selector) {
      let current = this;
      while (current) {
        if (matches(current, selector)) return current;
        current = current.parentElement;
      }
      return null;
    },
    contains(target) { return target === this || descendants(this).includes(target); },
    getBoundingClientRect() { return { left: 0, top: 0, right: 100, bottom: 32, width: 100, height: 32 }; },
  });
  Object.defineProperty(element, "className", {
    get() { return [...classNames].join(" "); },
    set(value) { classNames = new Set(String(value || "").split(/\s+/).filter(Boolean)); },
  });
  Object.defineProperty(element, "isConnected", { get() { return document.documentElement.contains(this); } });
  return element;
}
const documentListeners = new Map();
const documentListenerSet = (type) => documentListeners.get(type) || (documentListeners.set(type, new Set()), documentListeners.get(type));
globalThis.document = {
  readyState: "complete",
  scripts: [],
  visibilityState: "visible",
  documentElement: node("html"),
  body: node("body"),
  activeElement: null,
  createElement(tagName) { return node(tagName); },
  createTextNode(text) { const textNode = node("span"); textNode.nodeType = 3; textNode.textContent = String(text); return textNode; },
  querySelector(selector) { return this.documentElement.querySelector(selector); },
  querySelectorAll(selector) { return this.documentElement.querySelectorAll(selector); },
  addEventListener(type, listener) { documentListenerSet(type).add(listener); },
  removeEventListener(type, listener) { documentListenerSet(type).delete(listener); },
  dispatchEvent(event) {
    event.target ||= this;
    event.preventDefault ||= function() { this.defaultPrevented = true; };
    for (const listener of [...documentListenerSet(event.type)]) listener.call(this, event);
    return !event.defaultPrevented;
  },
};
document.documentElement.appendChild(document.body);
globalThis.MutationObserver = class MutationObserver { observe() {} disconnect() {} };
globalThis.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} };
globalThis.requestAnimationFrame = (callback) => { callback(); return 1; };
globalThis.cancelAnimationFrame = () => {};
window.addEventListener = () => {};
window.removeEventListener = () => {};
window.dispatchEvent = () => true;
globalThis.getComputedStyle = () => ({ display: "block", visibility: "visible", pointerEvents: "auto" });
const storage = new Map();
const storageWrites = [];
globalThis.sessionStorage = {
  getItem(key) { return storage.has(key) ? storage.get(key) : null; },
  setItem(key, value) { storage.set(key, String(value)); storageWrites.push(String(value)); },
  removeItem(key) { storage.delete(key); },
  clear() { storage.clear(); },
};
globalThis.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.location = { href: "https://codex.test/local/thread", pathname: "/local/thread", search: "", hash: "", protocol: "https:" };
globalThis.navigator = { userAgent: "node-test" };
globalThis.performance = { getEntriesByType: () => [] };
globalThis.CustomEvent = class CustomEvent extends Event { constructor(type, options = {}) { super(type, options); this.detail = options.detail; } };
let now = 1000;
const waits = [];
let nativeClockWaitHook = null;
window.__codexElvesTaskBoardNativeClock = {
  now: () => now,
  wait: (delay) => {
    waits.push(delay);
    now += delay;
    nativeClockWaitHook?.(delay);
  },
};
const capturedLogs = [];
globalThis.console = {
  log(...args) { capturedLogs.push(args.join(" ")); },
  warn(...args) { capturedLogs.push(args.join(" ")); },
  error(...args) { capturedLogs.push(args.join(" ")); },
};
window.__CODEX_ELVES_TEST_TASK_BOARD__ = true;
window.__codexSessionDeleteBridge = async (path) => path === "/settings/get"
  ? { launchMode: "direct", enhancementsEnabled: true, providerSyncEnabled: true }
  : { status: "ok", ids: [] };

let nativeStartClicks = 0;
let menuClicks = 0;
let selectClicks = 0;
let submitEvents = 0;
let setTextCalls = 0;
let modelTriggerClicks = 0;
let modelSubmenuClicks = 0;
let modelOptionClicks = 0;
let effortSubmenuClicks = 0;
let effortOptionClicks = 0;
const nativeSequence = [];
let permanentOnSubmit = true;
let nativeStartHook = null;
const projectRow = node("div");
projectRow.setAttribute("data-app-action-sidebar-project-row", "true");
projectRow.setAttribute("data-app-action-sidebar-project-id", "c:\\repo-a\\");
const projectMenu = node("button");
projectMenu.setAttribute("aria-haspopup", "menu");
projectMenu.addEventListener("click", () => { menuClicks += 1; });
const nativeStartButton = node("button");
nativeStartButton.addEventListener("click", () => {
  nativeStartClicks += 1;
  nativeStartHook?.();
});
const projectSelectButton = node("button");
projectSelectButton.setAttribute("data-app-action-sidebar-select-project", "true");
projectSelectButton.addEventListener("click", () => { selectClicks += 1; });
projectRow.append(projectMenu, nativeStartButton, projectSelectButton);
const composer = node("div");
composer.setAttribute("data-codex-composer", "true");
composer.setAttribute("contenteditable", "true");
composer.setAttribute("role", "textbox");
const conversationSignal = node("div");
conversationSignal.setAttribute("data-above-composer-conversation-id", "local:client-new-thread:temporary");
const controller = {
  text: "",
  focus() {},
  setText(value) {
    setTextCalls += 1;
    nativeSequence.push("instruction");
    this.text = String(value);
  },
  getText() { return this.text; },
  getPersistedText() { return this.text; },
  view: { dispatchEvent() { return true; } },
};
const composerOwner = node("div");
composerOwner.setAttribute("data-composer-footer-responsive", "true");
composerOwner.__reactFiber$test = { memoizedProps: { composerController: controller }, return: null };
composerOwner.appendChild(composer);
composer.addEventListener("keydown", (event) => {
  if (event.key !== "Enter") return;
  submitEvents += 1;
  nativeSequence.push("submit");
  if (permanentOnSubmit) conversationSignal.setAttribute("data-above-composer-conversation-id", "session-permanent-1");
});
let activeComposerOwner = composerOwner;
function mountFreshNativeComposer() {
  const nextComposer = node("div");
  nextComposer.setAttribute("data-codex-composer", "true");
  nextComposer.setAttribute("contenteditable", "true");
  nextComposer.setAttribute("role", "textbox");
  const nextController = {
    text: "",
    focus() {},
    setText(value) {
      setTextCalls += 1;
      nativeSequence.push("instruction");
      this.text = String(value);
    },
    getText() { return this.text; },
    getPersistedText() { return this.text; },
    view: { dispatchEvent() { return true; } },
  };
  const nextOwner = node("div");
  nextOwner.setAttribute("data-composer-footer-responsive", "true");
  nextOwner.__reactFiber$test = {
    memoizedProps: { composerController: nextController },
    return: null,
  };
  nextOwner.append(nextComposer, modelTrigger);
  nextComposer.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    submitEvents += 1;
    nativeSequence.push("submit");
    if (permanentOnSubmit) {
      conversationSignal.setAttribute(
        "data-above-composer-conversation-id",
        "session-permanent-1",
      );
    }
  });
  activeComposerOwner.remove();
  activeComposerOwner = nextOwner;
  document.body.appendChild(nextOwner);
}
const modelTrigger = node("button");
modelTrigger.setAttribute("data-codex-intelligence-trigger", "true");
modelTrigger.setAttribute("data-composer-navigation-target", "reasoning");
modelTrigger.setAttribute("aria-expanded", "false");
modelTrigger.setAttribute("aria-haspopup", "menu");
modelTrigger.setAttribute("data-selected-reasoning-effort", "low");
const modelTriggerLabel = node("span");
modelTriggerLabel.setAttribute("data-tooltip-overflow-target", "true");
modelTriggerLabel.textContent = "5.6 Sol";
modelTrigger.appendChild(modelTriggerLabel);
modelTrigger.addEventListener("click", () => {
  modelTriggerClicks += 1;
  modelTrigger.setAttribute(
    "aria-expanded",
    modelTrigger.getAttribute("aria-expanded") === "true" ? "false" : "true",
  );
});
composerOwner.appendChild(modelTrigger);
const modelSubmenu = node("div");
modelSubmenu.setAttribute("role", "menuitem");
modelSubmenu.setAttribute("aria-haspopup", "menu");
modelSubmenu.setAttribute("aria-expanded", "false");
modelSubmenu.setAttribute("aria-label", "模型 5.6 Sol");
modelSubmenu.addEventListener("click", () => {
  modelSubmenuClicks += 1;
  modelSubmenu.setAttribute("aria-expanded", "true");
});
const modelOption = node("div");
modelOption.setAttribute("role", "menuitem");
modelOption.textContent = "Claude Sonnet 4.6";
modelOption.addEventListener("click", () => {
  modelOptionClicks += 1;
  nativeSequence.push("model");
  modelTriggerLabel.textContent = "Claude Sonnet 4.6";
  modelTrigger.setAttribute("aria-expanded", "false");
});
const effortSubmenu = node("div");
effortSubmenu.setAttribute("role", "menuitem");
effortSubmenu.setAttribute("aria-haspopup", "menu");
effortSubmenu.setAttribute("aria-expanded", "false");
effortSubmenu.setAttribute("aria-label", "推理强度 低");
effortSubmenu.addEventListener("click", () => {
  effortSubmenuClicks += 1;
  effortSubmenu.setAttribute("aria-expanded", "true");
});
const effortOption = node("div");
effortOption.setAttribute("role", "menuitemradio");
effortOption.setAttribute("data-value", "high");
effortOption.textContent = "高";
effortOption.addEventListener("click", () => {
  effortOptionClicks += 1;
  nativeSequence.push("effort");
  modelTrigger.setAttribute("data-selected-reasoning-effort", "high");
  modelTrigger.setAttribute("aria-expanded", "false");
});
document.body.append(
  projectRow,
  composerOwner,
  conversationSignal,
  modelSubmenu,
  modelOption,
  effortSubmenu,
  effortOption,
);
nativeStartHook = mountFreshNativeComposer;

require(scriptPath);
const api = window.__codexElvesTaskBoardTest;
if (!api) throw new Error("task board test api unavailable");
api.setModelCatalogForTest({
  status: "ok",
  model: "gpt-5.6-sol",
    default_model: "gpt-5.6-sol",
    models: ["gpt-5.6-sol", "claude-sonnet-4-6"],
    model_entries: [
    {
      slug: "gpt-5.6-sol",
      display_name: "5.6 Sol",
      default_reasoning_level: "medium",
      supported_reasoning_levels: [
        { effort: "low" },
        { effort: "medium" },
        { effort: "high" },
        { effort: "xhigh" },
      ],
    },
    {
      slug: "claude-sonnet-4-6",
      display_name: "Claude Sonnet 4.6",
      default_reasoning_level: "high",
      supported_reasoning_levels: [{ effort: "low" }, { effort: "high" }],
    },
  ],
});
const instruction = "do not persist this native first instruction";
function snapshot(revision, title = "任务") {
  return {
    status: "ok",
    schemaVersion: 1,
    revision,
    tasks: [{ id: `task-${revision}`, title, project: { cwd: "c:/repo-a", label: "项目 A" }, status: "new", order: 0, conversations: [] }],
  };
}
function catalog() {
  return {
    status: "ok",
    projects: [{ cwd: "c:/repo-a", label: "项目 A" }],
    sessions: [{ sessionId: "existing-1", title: "已有会话", cwd: "c:/repo-a", updatedAtMs: 1 }],
    warnings: [],
  };
}
function reset(options = {}) {
  window.__codexElvesTaskBoardMock = options.mock || {};
  window.__codexElvesTaskBoardNativeAdapter = options.nativeAdapter || null;
  api.resetCreateStateForTest({ snapshot: options.snapshot || snapshot(3), catalog: catalog() });
  api.openCreateModalForTest();
}
function setNew(title = "原生任务", modelId = "claude-sonnet-4-6", effortId = "high") {
  api.setCreateDraftForTest({
    mode: "new",
    title,
    projectCwd: "c:/repo-a",
    modelId,
    effortId,
    firstInstruction: instruction,
    sessionIds: [],
  });
}
function setExisting(title = "已有任务") {
  api.setCreateDraftForTest({ mode: "existing", title, projectCwd: "c:/repo-a", sessionIds: ["existing-1"] });
}
function clearRecovery() {
  storage.clear();
  storageWrites.length = 0;
}
function recoveryKey() { return "codexElvesTaskBoardNativeCreateRecoveryV1"; }
(async () => {
  clearRecovery();
  nativeStartButton.disabled = false;
  permanentOnSubmit = true;
  now = 1000;
  waits.length = 0;
  const supportedPayloads = [];
  reset({ mock: { request(route, payload) {
    if (route !== "/task-board/task-create") throw new Error("unexpected route");
    supportedPayloads.push(payload);
    return snapshot(4, "原生创建成功");
  }}});
  setNew();
  await api.submitCreateForTest();
  const supported = {
    structuralButtonOnly: nativeStartClicks === 1 && menuClicks === 0 && selectClicks === 0,
    controllerAndNativeSubmit: setTextCalls === 1 && submitEvents === 1,
    selectedSettingsBeforeSubmit:
      modelTriggerClicks === 2 &&
      modelSubmenuClicks === 1 &&
      modelOptionClicks === 1 &&
      effortSubmenuClicks === 1 &&
      effortOptionClicks === 1 &&
      JSON.stringify(nativeSequence.slice(0, 4)) ===
        JSON.stringify(["model", "effort", "instruction", "submit"]),
    temporaryIdIgnored: supportedPayloads[0]?.sessionIds?.[0] === "session-permanent-1",
    createAfterPermanentId: supportedPayloads.length === 1 && supportedPayloads[0]?.taskId && supportedPayloads[0]?.expectedRevision === 3,
  };

  clearRecovery();
  nativeStartButton.disabled = true;
  let unsupportedCreateCalls = 0;
  reset({ mock: { request(route) {
    if (route === "/task-board/task-create") unsupportedCreateCalls += 1;
    return snapshot(4);
  }}});
  api.setCreateProjectForTest("c:/repo-a");
  await Promise.resolve();
  const newDisabled = api.createModalContractForTest().newButton.disabled;
  setNew("不支持的新会话");
  await api.submitCreateForTest();
  const unsupportedStaysOpen = api.createModalStateForTest().open && unsupportedCreateCalls === 0;
  reset({ mock: { request(route) {
    if (route === "/task-board/task-create") { unsupportedCreateCalls += 1; return snapshot(4); }
    throw new Error("unexpected route");
  }}});
  setExisting();
  await api.submitCreateForTest();
  const unsupported = { newDisabledExistingWorks: newDisabled && unsupportedStaysOpen && unsupportedCreateCalls === 1 };

  nativeStartButton.disabled = false;
  permanentOnSubmit = false;
  conversationSignal.setAttribute("data-above-composer-conversation-id", "local:client-new-thread:temporary");
  now = 0;
  waits.length = 0;
  const timeoutResult = await api.nativeStartForTest({ cwd: "c:/repo-a", label: "项目 A" }, instruction);
  const timeout = {
    boundedAt15Seconds: timeoutResult.code === "native_create_timeout" && waits.reduce((sum, delay) => sum + delay, 0) === 15000,
  };
  permanentOnSubmit = true;

  clearRecovery();
  now = 0;
  waits.length = 0;
  let sessionNotFoundCalls = 0;
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => ({ status: "ok", sessionId: "session-retry-1" }),
    },
    mock: { request(route) {
      if (route !== "/task-board/task-create") throw new Error("unexpected route");
      sessionNotFoundCalls += 1;
      if (sessionNotFoundCalls <= 5) return { status: "failed", code: "session_not_found", message: "missing" };
      return snapshot(4);
    }},
  });
  setNew("会话重试");
  await api.submitCreateForTest();
  const sessionNotFoundWithin10Seconds = sessionNotFoundCalls === 6 && waits.reduce((sum, delay) => sum + delay, 0) === 10000;

  clearRecovery();
  const revisionPayloads = [];
  let revisionCalls = 0;
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => ({ status: "ok", sessionId: "session-revision-1" }),
    },
    mock: { request(route, payload) {
      if (route !== "/task-board/task-create") throw new Error("unexpected route");
      revisionPayloads.push(payload);
      revisionCalls += 1;
      if (revisionCalls === 1) return { status: "conflict", code: "revision_conflict", schemaVersion: 1, revision: 4, tasks: [] };
      return snapshot(5);
    }},
  });
  setNew("修订重试");
  await api.submitCreateForTest();
  const retry = {
    sessionNotFoundWithin10Seconds,
    revisionRetriesOnceWithSameTaskId:
      revisionPayloads.length === 2 &&
      revisionPayloads[0]?.taskId === revisionPayloads[1]?.taskId &&
      JSON.stringify(revisionPayloads.map((payload) => payload.expectedRevision)) === JSON.stringify([3, 4]),
  };

  clearRecovery();
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => ({ status: "ok", sessionId: "session-recover-1" }),
    },
    mock: { request() { throw new Error("bridge lost"); } },
  });
  setNew("恢复任务");
  await api.submitCreateForTest();
  const persisted = api.nativeRecoveryForTest();
  const persistedString = storage.get(recoveryKey()) || "";
  const bridgeFailurePersistsAllowedFields =
    persisted?.taskId && persisted?.title === "恢复任务" && persisted?.project?.cwd === "c:/repo-a" &&
    persisted?.sessionId === "session-recover-1" && persisted?.createdAtMs > 0 &&
    persisted?.kind === "create-task" &&
    persisted?.initialStatus === "new" &&
    JSON.stringify(Object.keys(persisted).sort()) === JSON.stringify(["createdAtMs", "initialStatus", "kind", "project", "sessionId", "taskId", "title"]);
  let recoveryCalls = 0;
  window.__codexElvesTaskBoardMock = { request(route, payload) {
    if (route !== "/task-board/task-create") throw new Error("unexpected route");
    recoveryCalls += 1;
    return snapshot(5);
  }};
  await api.retryNativeCreateRecoveryForTest();
  const nextActivationRetriesOnceAndClears = recoveryCalls === 1 && !storage.has(recoveryKey());

  const retryFailureRecord = {
    taskId: "11111111-1111-4111-8111-111111111112",
    title: "恢复仍失败",
    project: { cwd: "c:/repo-a", label: "项目 A" },
    sessionId: "session-retry-failed",
    createdAtMs: now,
  };
  storage.set(recoveryKey(), JSON.stringify(retryFailureRecord));
  api.resetCreateStateForTest({ snapshot: snapshot(5), catalog: catalog() });
  window.__codexElvesTaskBoardMock = {
    request() { return { status: "failed", code: "task_board_unavailable", message: "down" }; },
  };
  await api.retryNativeCreateRecoveryForTest();
  const retryFailureToast = document.body.querySelector(".codex-delete-toast")?.textContent || "";
  const retryFailureKeepsRecordAndWarns =
    storage.has(recoveryKey()) && retryFailureToast.includes("会话已创建，但任务尚未保存");

  const expired = {
    taskId: "11111111-1111-4111-8111-111111111111",
    title: "过期恢复",
    project: { cwd: "c:/repo-a", label: "项目 A" },
    sessionId: "session-expired",
    createdAtMs: now - 24 * 60 * 60 * 1000 - 1,
  };
  storage.set(recoveryKey(), JSON.stringify(expired));
  const expiredRecordDiscarded = api.nativeRecoveryForTest() === null && !storage.has(recoveryKey());
  storage.set(recoveryKey(), "{");
  const malformedRecordDiscarded = api.nativeRecoveryForTest() === null && !storage.has(recoveryKey());

  clearRecovery();
  let routineRefreshCalls = 0;
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => ({ status: "ok", sessionId: "session-runtime-1" }),
    },
    mock: { request(route) {
      if (route !== "/task-board/task-create") throw new Error("unexpected route");
      routineRefreshCalls += 1;
      api.refreshRuntimeForTest();
      return snapshot(9);
    }},
  });
  setNew("刷新期间继续创建");
  await api.submitCreateForTest();
  const routineRefreshRecord = api.nativeRecoveryForTest();
  const routineRefresh = {
    keepsCreateAlive:
      routineRefreshCalls === 1 &&
      routineRefreshRecord === null &&
      api.createSnapshotForTest().revision === 9 &&
      !window.__codexElvesTaskBoardNativeOperationLease,
  };

  clearRecovery();
  const replacementRuntimeId = window.__codexElvesTaskBoardNativeRuntimeId;
  let replacementCreateCalls = 0;
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => {
        window.__codexElvesTaskBoardRuntimeVersion = "force-full-replacement";
        delete require.cache[require.resolve(scriptPath)];
        require(scriptPath);
        return { status: "ok", sessionId: "session-replacement-1" };
      },
    },
    mock: { request(route) {
      if (route !== "/task-board/task-create") throw new Error("unexpected route");
      replacementCreateCalls += 1;
      return snapshot(10);
    }},
  });
  setNew("完整替换期间继续创建");
  await api.submitCreateForTest();
  const runtimeReplacement = {
    keepsRuntimeAndCreateAlive:
      replacementCreateCalls === 1 &&
      window.__codexElvesTaskBoardNativeRuntimeId === replacementRuntimeId &&
      api.nativeRecoveryForTest() === null &&
      api.createSnapshotForTest().revision === 10 &&
      !window.__codexElvesTaskBoardNativeOperationLease,
  };

  now = 0;
  waits.length = 0;
  conversationSignal.setAttribute(
    "data-above-composer-conversation-id",
    "local:client-new-thread:navigation-race",
  );
  nativeStartHook = null;
  const oldTriggerClicksBeforeRace = modelTriggerClicks;
  let oldReadyTriggerClickCalls = 0;
  modelTriggerLabel.textContent = "5.6 Sol";
  modelTrigger.setAttribute("aria-expanded", "false");
  modelTrigger.setAttribute("data-selected-reasoning-effort", "low");
  modelTrigger.disabled = false;
  modelTrigger.click = () => {
    oldReadyTriggerClickCalls += 1;
  };
  modelTrigger.getBoundingClientRect = () => ({
    left: 0,
    top: 32,
    right: 100,
    bottom: 64,
    width: 100,
    height: 32,
  });
  const hiddenRect = () => ({
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
    width: 0,
    height: 0,
  });
  modelSubmenu.getBoundingClientRect = hiddenRect;
  modelOption.getBoundingClientRect = hiddenRect;
  effortSubmenu.getBoundingClientRect = hiddenRect;
  effortOption.getBoundingClientRect = hiddenRect;

  let raceSetTextCalls = 0;
  let raceSubmitEvents = 0;
  let raceModelTriggerClicks = 0;
  let raceModelSubmenuClicks = 0;
  let raceModelOptionClicks = 0;
  let raceEffortSubmenuClicks = 0;
  let raceEffortOptionClicks = 0;
  const raceSequence = [];
  const raceComposer = node("div");
  raceComposer.setAttribute("data-codex-composer", "true");
  raceComposer.setAttribute("contenteditable", "true");
  raceComposer.setAttribute("role", "textbox");
  const raceController = {
    text: "",
    focus() {},
    setText(value) {
      raceSetTextCalls += 1;
      raceSequence.push("instruction");
      this.text = String(value);
    },
    getText() { return this.text; },
    getPersistedText() { return this.text; },
    view: { dispatchEvent() { return true; } },
  };
  const raceComposerOwner = node("div");
  raceComposerOwner.setAttribute("data-composer-footer-responsive", "true");
  raceComposerOwner.__reactFiber$test = {
    memoizedProps: { composerController: raceController },
    return: null,
  };
  raceComposerOwner.appendChild(raceComposer);
  raceComposer.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    raceSubmitEvents += 1;
    raceSequence.push("submit");
    conversationSignal.setAttribute(
      "data-above-composer-conversation-id",
      "session-navigation-race",
    );
  });

  const raceModelTrigger = node("button");
  raceModelTrigger.setAttribute("data-codex-intelligence-trigger", "true");
  raceModelTrigger.setAttribute("data-composer-navigation-target", "reasoning");
  raceModelTrigger.setAttribute("aria-expanded", "false");
  raceModelTrigger.setAttribute("aria-haspopup", "menu");
  raceModelTrigger.setAttribute("data-selected-reasoning-effort", "low");
  const raceModelTriggerLabel = node("span");
  raceModelTriggerLabel.setAttribute("data-tooltip-overflow-target", "true");
  raceModelTriggerLabel.textContent = "5.6 Sol";
  raceModelTrigger.appendChild(raceModelTriggerLabel);
  raceModelTrigger.addEventListener("click", () => {
    raceModelTriggerClicks += 1;
    raceModelTrigger.setAttribute(
      "aria-expanded",
      raceModelTrigger.getAttribute("aria-expanded") === "true"
        ? "false"
      : "true",
    );
  });
  raceComposerOwner.appendChild(raceModelTrigger);

  const raceModelSubmenu = node("div");
  raceModelSubmenu.setAttribute("role", "menuitem");
  raceModelSubmenu.setAttribute("aria-haspopup", "menu");
  raceModelSubmenu.setAttribute("aria-expanded", "false");
  raceModelSubmenu.setAttribute("aria-label", "模型 5.6 Sol");
  raceModelSubmenu.addEventListener("click", () => {
    raceModelSubmenuClicks += 1;
    raceModelSubmenu.setAttribute("aria-expanded", "true");
  });
  const raceModelOption = node("div");
  raceModelOption.setAttribute("role", "menuitem");
  raceModelOption.textContent = "Claude Sonnet 4.6";
  raceModelOption.addEventListener("click", () => {
    raceModelOptionClicks += 1;
    raceSequence.push("model");
    raceModelTriggerLabel.textContent = "Claude Sonnet 4.6";
    raceModelSubmenu.setAttribute("aria-expanded", "false");
    raceModelTrigger.setAttribute("aria-expanded", "false");
  });
  const raceEffortSubmenu = node("div");
  raceEffortSubmenu.setAttribute("role", "menuitem");
  raceEffortSubmenu.setAttribute("aria-haspopup", "menu");
  raceEffortSubmenu.setAttribute("aria-expanded", "false");
  raceEffortSubmenu.setAttribute("aria-label", "推理强度 低");
  raceEffortSubmenu.addEventListener("click", () => {
    raceEffortSubmenuClicks += 1;
    raceEffortSubmenu.setAttribute("aria-expanded", "true");
  });
  const raceEffortOption = node("div");
  raceEffortOption.setAttribute("role", "menuitemradio");
  raceEffortOption.setAttribute("data-value", "high");
  raceEffortOption.textContent = "高";
  raceEffortOption.addEventListener("click", () => {
    raceEffortOptionClicks += 1;
    raceSequence.push("effort");
    raceModelTrigger.setAttribute("data-selected-reasoning-effort", "high");
    raceEffortSubmenu.setAttribute("aria-expanded", "false");
    raceModelTrigger.setAttribute("aria-expanded", "false");
  });
  const visibleRect = () => ({
    left: 0,
    top: 0,
    right: 100,
    bottom: 32,
    width: 100,
    height: 32,
  });
  raceModelSubmenu.getBoundingClientRect = () =>
    raceModelTrigger.getAttribute("aria-expanded") === "true"
      ? visibleRect()
      : hiddenRect();
  raceModelOption.getBoundingClientRect = () =>
    raceModelSubmenu.getAttribute("aria-expanded") === "true"
      ? visibleRect()
      : hiddenRect();
  raceEffortSubmenu.getBoundingClientRect = () =>
    raceModelTrigger.getAttribute("aria-expanded") === "true"
      ? visibleRect()
      : hiddenRect();
  raceEffortOption.getBoundingClientRect = () =>
    raceEffortSubmenu.getAttribute("aria-expanded") === "true"
      ? visibleRect()
      : hiddenRect();

  let raceReplacementInstalled = false;
  nativeClockWaitHook = () => {
    if (raceReplacementInstalled || now < 100) return;
    raceReplacementInstalled = true;
    activeComposerOwner.querySelector(
      '[data-codex-composer][contenteditable="true"][role="textbox"]',
    )?.remove();
    document.body.append(
      raceComposerOwner,
      raceModelSubmenu,
      raceModelOption,
      raceEffortSubmenu,
      raceEffortOption,
    );
    activeComposerOwner = raceComposerOwner;
  };
  const navigationRaceResult = await api.nativeStartForTest(
    { cwd: "c:/repo-a", label: "项目 A" },
    instruction,
    "claude-sonnet-4-6",
    "high",
  );
  nativeClockWaitHook = null;
  const navigationRace = {
    waitsForReplacementBeforeSettings:
      navigationRaceResult.status === "ok" &&
      navigationRaceResult.sessionId === "session-navigation-race" &&
      raceReplacementInstalled &&
      oldReadyTriggerClickCalls === 0 &&
      modelTriggerClicks === oldTriggerClicksBeforeRace &&
      raceModelTriggerClicks === 2 &&
      raceModelSubmenuClicks === 1 &&
      raceModelOptionClicks === 1 &&
      raceEffortSubmenuClicks === 1 &&
      raceEffortOptionClicks === 1 &&
      raceSetTextCalls === 1 &&
      raceSubmitEvents === 1 &&
      JSON.stringify(raceSequence) ===
        JSON.stringify(["model", "effort", "instruction", "submit"]),
  };

  const privacy = {
    payloadStorageAndOutputExcludeInstruction:
      !JSON.stringify(supportedPayloads).includes(instruction) &&
      !persistedString.includes(instruction) &&
      !storageWrites.join("\n").includes(instruction) &&
      !capturedLogs.join("\n").includes(instruction),
  };
  process.stdout.write(JSON.stringify({ supported, navigationRace, unsupported, timeout, retry, recovery: {
    bridgeFailurePersistsAllowedFields,
    nextActivationRetriesOnceAndClears,
    retryFailureKeepsRecordAndWarns,
    expiredRecordDiscarded,
    malformedRecordDiscarded,
  }, routineRefresh, runtimeReplacement, privacy }));
  process.exit(0);
})().catch((error) => { process.stderr.write(String(error?.stack || error)); process.exit(1); });
"##,
    )
    .expect("task board native create harness should be written");
    let output = Command::new("node")
        .arg(&harness_path)
        .arg(&script_path)
        .output()
        .expect("node should run task board native create harness");
    assert!(
        output.status.success(),
        "node task board native create harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("native create harness stdout should be JSON")
}

fn run_task_board_create_contract_harness() -> serde_json::Value {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let script_path = temp.path().join("renderer-inject.js");
    let harness_path = temp.path().join("task-board-create-harness.cjs");
    std::fs::write(&script_path, assets::injection_script(45221))
        .expect("injection script should be written");
    std::fs::write(
        &harness_path,
        r#"
const scriptPath = process.argv[2];
function node(tagName = "div") {
  const attributes = new Map();
  const listeners = new Map();
  const listenerSet = (type) => {
    let entries = listeners.get(type);
    if (!entries) {
      entries = new Set();
      listeners.set(type, entries);
    }
    return entries;
  };
  const descendants = (root) => root.children.flatMap((child) => [child, ...descendants(child)]);
  const matches = (candidate, selector) => {
    if (selector.includes("button") && candidate.tagName === "BUTTON") return !candidate.disabled;
    if (selector.includes("input") && candidate.tagName === "INPUT") return !candidate.disabled;
    if (selector.includes("select") && candidate.tagName === "SELECT") return !candidate.disabled;
    const attribute = selector.match(/^\[([^=]+)="([^"]+)"\]$/);
    return !!attribute && candidate.getAttribute(attribute[1]) === attribute[2];
  };
  return {
    tagName: String(tagName).toUpperCase(),
    children: [],
    dataset: {},
    style: { setProperty() {}, removeProperty() {} },
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
    appendChild(child) {
      child.parentElement?.remove?.();
      this.children.push(child);
      child.parentElement = this;
      child.isConnected = true;
      return child;
    },
    append(...children) { children.forEach((child) => this.appendChild(child)); },
    prepend(...children) {
      children.slice().reverse().forEach((child) => {
        child.parentElement?.remove?.();
        this.children.unshift(child);
        child.parentElement = this;
        child.isConnected = true;
      });
    },
    remove() {
      this.removed = true;
      if (this.parentElement) {
        const index = this.parentElement.children.indexOf(this);
        if (index >= 0) this.parentElement.children.splice(index, 1);
      }
      this.parentElement = null;
      this.isConnected = false;
    },
    replaceChildren(...children) {
      this.children.slice().forEach((child) => child.remove());
      this.append(...children);
    },
    setAttribute(name, value) { attributes.set(String(name), String(value)); },
    getAttribute(name) { return attributes.get(String(name)) ?? null; },
    removeAttribute(name) { attributes.delete(String(name)); },
    toggleAttribute(name, force) {
      if (force === false) attributes.delete(String(name));
      else attributes.set(String(name), "");
    },
    addEventListener(type, listener) { listenerSet(type).add(listener); },
    removeEventListener(type, listener) { listenerSet(type).delete(listener); },
    dispatchEvent(event) {
      event.target ||= this;
      listenerSet(event.type).forEach((listener) => listener(event));
      return !event.defaultPrevented;
    },
    click() {
      return this.dispatchEvent({
        type: "click",
        target: this,
        defaultPrevented: false,
        preventDefault() { this.defaultPrevented = true; },
      });
    },
    querySelector(selector) { return this.querySelectorAll(selector)[0] || null; },
    querySelectorAll(selector) {
      return descendants(this).filter((candidate) => matches(candidate, selector));
    },
    closest() { return null; },
    matches(selector) { return matches(this, selector); },
    contains(target) {
      return this === target || this.children.some((child) => child.contains?.(target));
    },
    insertAdjacentElement() {},
    focus() { document.activeElement = this; },
    parentElement: null,
    isConnected: true,
    removed: false,
    disabled: false,
    value: "",
    checked: false,
    textContent: "",
    innerHTML: "",
    clientWidth: 0,
    clientHeight: 0,
  };
}
globalThis.window = globalThis;
globalThis.Element = class Element {};
globalThis.HTMLElement = class HTMLElement extends Element {};
globalThis.HTMLButtonElement = class HTMLButtonElement extends HTMLElement {};
globalThis.MutationObserver = class MutationObserver {
  observe() {}
  disconnect() {}
};
globalThis.ResizeObserver = class ResizeObserver {
  observe() {}
  disconnect() {}
};
globalThis.requestAnimationFrame = (callback) => { callback(); return 1; };
globalThis.cancelAnimationFrame = () => {};
window.addEventListener = () => {};
window.removeEventListener = () => {};
window.dispatchEvent = () => true;
const documentListeners = new Map();
function documentListenerSet(type) {
  let listeners = documentListeners.get(type);
  if (!listeners) {
    listeners = new Set();
    documentListeners.set(type, listeners);
  }
  return listeners;
}
const mainSurface = node("main");
mainSurface.setAttribute("data-app-shell-main-surface", "true");
globalThis.document = {
  readyState: "complete",
  scripts: [],
  visibilityState: "visible",
  documentElement: node("html"),
  body: node("body"),
  activeElement: null,
  createElement: (tagName) => node(tagName),
  createTextNode: (text) => ({ textContent: String(text), remove() {} }),
  getElementById: () => null,
  querySelector(selector) {
    if (selector === "main[data-app-shell-main-surface]" || selector === "main" || selector === "[role='main']") return mainSurface;
    return null;
  },
  querySelectorAll: () => [],
  addEventListener(type, listener) { documentListenerSet(type).add(listener); },
  removeEventListener(type, listener) { documentListenerSet(type).delete(listener); },
  listenerCount(type) { return documentListeners.get(type)?.size || 0; },
  dispatchEvent(event) {
    event.target ||= this;
    event.preventDefault ||= () => { event.defaultPrevented = true; };
    event.stopImmediatePropagation ||= () => { event.immediatePropagationStopped = true; };
    for (const listener of documentListenerSet(event.type)) {
      listener(event);
      if (event.immediatePropagationStopped) break;
    }
    return !event.defaultPrevented;
  },
};
document.body.appendChild(mainSurface);
globalThis.getComputedStyle = () => ({
  display: "block",
  visibility: "visible",
  pointerEvents: "auto",
});
globalThis.localStorage = {
  getItem: () => null,
  setItem() {},
  removeItem() {},
};
globalThis.location = {
  href: "https://codex.test/local/thread-12345678",
  pathname: "/local/thread-12345678",
  search: "",
  hash: "",
  protocol: "https:",
};
globalThis.navigator = { userAgent: "node-test" };
globalThis.performance = { getEntriesByType: () => [] };
globalThis.CustomEvent = class CustomEvent {
  constructor(type, options = {}) {
    this.type = type;
    this.detail = options.detail;
  }
};
globalThis.Event = class Event {
  constructor(type) {
    this.type = type;
  }
};
window.__CODEX_ELVES_TEST_TASK_BOARD__ = true;
window.__codexSessionDeleteBridge = async (path) => {
  if (path === "/settings/get") {
    return { launchMode: "direct", enhancementsEnabled: true, providerSyncEnabled: true };
  }
  if (path === "/session/suppressed") return { ids: [] };
  return { status: "ok" };
};
require(scriptPath);
const api = window.__codexElvesTaskBoardTest;
if (!api) throw new Error("task board test api unavailable");
function snapshot(revision, title = "已有任务") {
  return {
    status: "ok",
    schemaVersion: 1,
    revision,
    tasks: [{
      id: `task-${revision}`,
      title,
      project: { cwd: "/repo-a", label: "项目 A" },
      status: "new",
      order: 0,
      conversations: [],
    }],
  };
}
function catalog() {
  return {
    status: "ok",
    projects: [
      { cwd: "/repo-a", label: "项目 A" },
      { cwd: "/repo-b", label: "项目 B" },
    ],
    sessions: [
      { sessionId: "session-a1", title: "A 一号", cwd: "/repo-a", updatedAtMs: 3 },
      { sessionId: "session-a2", title: "A 二号", cwd: "/repo-a", updatedAtMs: 2 },
      { sessionId: "session-b1", title: "B 一号", cwd: "/repo-b", updatedAtMs: 1 },
    ],
    warnings: [],
  };
}
function reset(options = {}) {
  window.__codexElvesTaskBoardMock = options.mock || {};
  window.__codexElvesTaskBoardNativeAdapter = options.nativeAdapter || null;
  api.resetCreateStateForTest({
    snapshot: options.snapshot || snapshot(3),
    catalog: options.catalog || catalog(),
    catalogError: options.catalogError || "",
  });
  api.openCreateModalForTest();
}
function setExisting(title = "创建成功") {
  api.setCreateDraftForTest({
    mode: "existing",
    title,
    projectCwd: "/repo-a",
    sessionIds: ["session-a1", "session-a2"],
  });
}
function createState() {
  return api.createModalStateForTest();
}
(async () => {
  const priorFocus = document.createElement("button");
  document.body.appendChild(priorFocus);
  priorFocus.focus();
  reset();
  const modalBeforeRefresh = api.createModalContractForTest();
  api.focusCreateModalControlForTest("submitButton");
  api.dispatchCreateModalKeyForTest("Tab");
  const tabForwardWraps = document.activeElement === modalBeforeRefresh.closeButton;
  api.focusCreateModalControlForTest("closeButton");
  const backwardPrevented = api.dispatchCreateModalKeyForTest("Tab", true);
  const tabBackwardActive = api.activeCreateModalControlForTest();
  const tabBackwardWraps = tabBackwardActive === "submitButton";
  const tabFocusableControls = api.createModalFocusableControlsForTest();
  setExisting("保留草稿");
  api.reconcileRuntimeForTest();
  const routineReconcilePreservesDraft = createState().open && createState().title === "保留草稿";
  api.setCreateBusyForTest(true);
  api.dispatchCreateModalKeyForTest("Escape");
  api.clickCreateModalControlForTest("closeButton");
  api.clickCreateModalControlForTest("cancelButton");
  api.clickCreateModalControlForTest("backdrop");
  const busyControlsStayOpen = createState().open;
  api.refreshRuntimeForTest();
  const modal = {
    role: modalBeforeRefresh.role,
    ariaModal: modalBeforeRefresh.ariaModal,
    initialFocus: modalBeforeRefresh.initialFocus,
    bodyMounted: modalBeforeRefresh.bodyMounted,
    outsideMain: modalBeforeRefresh.outsideMain,
    tabForwardWraps,
    tabBackwardWraps,
    tabBackwardActive,
    tabFocusableControls,
    busyControlsStayOpen,
    routineReconcilePreservesDraft,
    keydownListenersBeforeRefresh: modalBeforeRefresh.keydownListeners,
    removedAfterRefresh: modalBeforeRefresh.node.removed === true,
    keydownListenersAfterRefresh: document.listenerCount("keydown"),
    focusRestored: document.activeElement === priorFocus,
    busyAfterRefresh: createState().busy,
  };

  reset();
  api.releaseCreateModalOnBackdropForTest();
  modal.dragReleaseStaysOpen = createState().open;
  api.clickCreateModalControlForTest("backdrop");
  modal.backdropClickCloses = !createState().open;

  api.setModelCatalogForTest({
    status: "ok",
    model: "gpt-5.6-sol",
    default_model: "gpt-5.6-sol",
    models: ["gpt-5.6-sol", "claude-sonnet-4-6"],
    model_entries: [
      {
        slug: "gpt-5.6-sol",
        display_name: "5.6 Sol",
        default_reasoning_level: "medium",
        supported_reasoning_levels: [
          { effort: "low" },
          { effort: "medium" },
          { effort: "high" },
          { effort: "xhigh" },
          { effort: "max" },
        ],
      },
      {
        slug: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6",
        default_reasoning_level: "high",
        supported_reasoning_levels: [
          { effort: "low" },
          { effort: "high" },
        ],
      },
    ],
  });
  reset();
  const dropdownContract = api.createModalContractForTest();
  dropdownContract.projectSelect.getBoundingClientRect = () => ({
    left: 260,
    right: 560,
    top: 100,
    bottom: 136,
    width: 300,
    height: 36,
  });
  dropdownContract.modelTrigger.getBoundingClientRect = () => ({
    left: 500,
    right: 580,
    top: 500,
    bottom: 530,
    width: 80,
    height: 30,
  });
  api.openBoardProjectMenuForTest({ left: 40, top: 40, width: 132, height: 36 });
  const boardProjectDropdownOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Enter");
  api.openCreateDropdownForTest("project");
  const projectDropdownOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Escape");
  const projectEscapeReturnsFocus = document.activeElement === dropdownContract.projectSelect;
  const projectExpandedAfterEscape = dropdownContract.projectSelect.getAttribute("aria-expanded");
  api.openCreateDropdownForTest("status");
  const statusDropdownOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("ArrowDown");
  const statusDropdownDown = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Enter");
  const statusAfterEnter = createState().initialStatus;
  const dropdownOpenAfterEnter = api.dropdownMenuStateForTest().open;
  api.setCreateDraftForTest({ mode: "new", modelId: "gpt-5.6-sol", effortId: "xhigh" });
  api.openCreateSettingsMenuForTest();
  const settingsMenuOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Escape");
  const settingsEscapeReturnsFocus = document.activeElement === dropdownContract.modelTrigger;
  api.openCreateEffortMenuForTest();
  const fullEffortMenuOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("End");
  api.dispatchDropdownMenuKeyForTest("Enter");
  const afterMaxEnter = createState();
  api.setCreateDraftForTest({ effortId: "xhigh" });
  api.openCreateModelMenuForTest();
  const modelMenuOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("End");
  api.dispatchDropdownMenuKeyForTest("Enter");
  const afterModelEnter = createState();
  api.openCreateEffortMenuForTest();
  const effortMenuOpen = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Home");
  api.dispatchDropdownMenuKeyForTest("Enter");
  const afterEffortEnter = createState();
  api.openCreateEffortMenuForTest();
  api.dispatchDropdownMenuKeyForTest("Escape");
  const afterSubmenuEscape = api.dropdownMenuStateForTest();
  api.dispatchDropdownMenuKeyForTest("Escape");
  const afterSettingsEscape = api.dropdownMenuStateForTest();
  const settingsLayeredEscape =
    afterSubmenuEscape.open &&
    !afterSubmenuEscape.submenuOpen &&
    afterSubmenuEscape.focusedIndex === 1 &&
    !afterSettingsEscape.open &&
    document.activeElement === dropdownContract.modelTrigger;
  const dropdowns = {
    projectEscapeReturnsFocus,
    projectExpandedAfterEscape,
    settingsEscapeReturnsFocus,
    statusFocusedAfterDown: statusDropdownDown.focusedIndex,
    statusAfterEnter,
    dropdownOpenAfterEnter,
    settingsLayeredEscape,
    projectMenusConsistent:
      boardProjectDropdownOpen.kind === "project" &&
      boardProjectDropdownOpen.role === "listbox" &&
      boardProjectDropdownOpen.itemCount === 3 &&
      boardProjectDropdownOpen.width === "320px" &&
      boardProjectDropdownOpen.minWidth === "320px" &&
      boardProjectDropdownOpen.left === "40px" &&
      projectDropdownOpen.width === "320px" &&
      projectDropdownOpen.minWidth === "320px" &&
      projectDropdownOpen.left === "260px",
    sharedListbox:
      projectDropdownOpen.kind === "create-project" &&
      projectDropdownOpen.role === "listbox" &&
      projectDropdownOpen.itemCount === 2 &&
      projectDropdownOpen.selectedIndex === 0 &&
      JSON.stringify(projectDropdownOpen.optionDescriptions) ===
        JSON.stringify(["/repo-a", "/repo-b"]) &&
      projectDropdownOpen.triggerExpanded === "true" &&
      statusDropdownOpen.kind === "create-status" &&
      statusDropdownOpen.role === "listbox" &&
      statusDropdownOpen.itemCount === 5 &&
      statusDropdownOpen.selectedIndex === 0,
    nativeSettingsMenu:
      settingsMenuOpen.kind === "create-settings" &&
      settingsMenuOpen.role === "menu" &&
      settingsMenuOpen.itemCount === 2 &&
      settingsMenuOpen.top === "494px" &&
      JSON.stringify(settingsMenuOpen.buttonTexts) ===
        JSON.stringify(["模型 5.6 Sol", "推理强度 极高"]) &&
      settingsMenuOpen.triggerExpanded === "true" &&
      fullEffortMenuOpen.submenuOpen &&
      fullEffortMenuOpen.submenuKind === "effort" &&
      fullEffortMenuOpen.submenuItemCount === 5 &&
      fullEffortMenuOpen.submenuSelectedIndex === 3 &&
      JSON.stringify(fullEffortMenuOpen.submenuTexts) ===
        JSON.stringify(["轻度", "中", "高", "极高", "最高"]) &&
      modelMenuOpen.submenuOpen &&
      modelMenuOpen.submenuKind === "model" &&
      modelMenuOpen.submenuRole === "menu" &&
      modelMenuOpen.submenuItemCount === 2 &&
      modelMenuOpen.submenuSelectedIndex === 0 &&
      JSON.stringify(modelMenuOpen.submenuTexts) ===
        JSON.stringify(["5.6 Sol", "Claude Sonnet 4.6"]) &&
      effortMenuOpen.submenuOpen &&
      effortMenuOpen.submenuKind === "effort" &&
      effortMenuOpen.submenuRole === "menu" &&
      effortMenuOpen.submenuItemCount === 2 &&
      effortMenuOpen.submenuSelectedIndex === 1 &&
      JSON.stringify(effortMenuOpen.submenuTexts) === JSON.stringify(["轻度", "高"]) &&
      dropdownContract.modelTrigger.parentElement === dropdownContract.firstInstructionComposer &&
      dropdownContract.modelTrigger.getAttribute("aria-haspopup") === "menu" &&
      dropdownContract.modelTrigger.getAttribute("data-reasoning-effort") === "low",
    keyboardAndFocus:
      projectEscapeReturnsFocus &&
      projectExpandedAfterEscape === "false" &&
      settingsEscapeReturnsFocus &&
      settingsLayeredEscape &&
      statusDropdownDown.focusedIndex === 1 &&
      statusAfterEnter === "planning" &&
      !dropdownOpenAfterEnter &&
      afterMaxEnter.effortId === "max" &&
      afterModelEnter.modelId === "claude-sonnet-4-6" &&
      afterModelEnter.effortId === "high" &&
      afterEffortEnter.effortId === "low",
  };

  reset();
  api.setCreateProjectForTest("/repo-a");
  const matching = createState().availableSessionIds;
  api.setCreateSessionsForTest(["session-a1", "session-a2"]);
  api.setCreateProjectForTest("/repo-b");
  const afterProjectChange = createState();
  const projectSelection = {
    onlyMatchingSessions: JSON.stringify(matching) === JSON.stringify(["session-a1", "session-a2"]),
    clearedAfterProjectChange: afterProjectChange.selectedSessionIds.length === 0,
  };
  reset({
    snapshot: { status: "ok", schemaVersion: 1, revision: 3, tasks: [] },
  });
  api.setCreateDraftForTest({ mode: "existing", title: "目录更新", projectCwd: "/repo-a", sessionIds: ["session-a1"] });
  api.applyCatalogForTest({
    status: "ok",
    projects: [{ cwd: "/repo-b", label: "项目 B" }],
    sessions: [{ sessionId: "session-b1", title: "B 一号", cwd: "/repo-b", updatedAtMs: 1 }],
    warnings: [],
  });
  const catalogOutcomeState = createState();
  projectSelection.catalogOutcomeReconcilesAndRenders =
    catalogOutcomeState.selectedSessionIds.length === 0 &&
    JSON.stringify(catalogOutcomeState.availableSessionIds) === JSON.stringify([]) &&
    JSON.stringify(catalogOutcomeState.projectOptionCwds) === JSON.stringify(["/repo-b"]);

  reset();
  api.setCreateDraftForTest({ mode: "existing", title: "   ", projectCwd: "/repo-a", sessionIds: ["session-a1"] });
  await api.submitCreateForTest();
  const trimmedTitleRejected = createState().feedback.includes("标题");
  let catalogCreateCalls = 0;
  reset({
    snapshot: snapshot(3, "保留任务"),
    catalogError: "会话目录加载失败",
    mock: {
      request(route) {
        if (route === "/task-board/task-create") catalogCreateCalls += 1;
        return { status: "failed", code: "bridge_unavailable", message: "bridge unavailable" };
      },
    },
  });
  setExisting("目录失败");
  await api.submitCreateForTest();
  const catalogFailureState = createState();
  const validation = {
    trimmedTitleRejected,
    catalogFailureBlockedExistingOnly: catalogCreateCalls === 0 && catalogFailureState.feedback.includes("目录"),
    tasksPreservedOnCatalogFailure: api.createSnapshotForTest().tasks[0]?.title === "保留任务",
  };

  const successPayloads = [];
  reset({
    mock: {
      request(route, payload) {
        if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
        successPayloads.push(payload);
        return snapshot(4, "创建成功");
      },
    },
  });
  setExisting("  创建成功  ");
  await api.submitCreateForTest();
  const successState = createState();
  const successPayload = successPayloads[0] || {};
  const success = {
    exactPayload:
      JSON.stringify(Object.keys(successPayload).sort()) === JSON.stringify(["expectedRevision", "project", "sessionIds", "taskId", "title"]) &&
      typeof successPayload.taskId === "string" &&
      successPayload.taskId.length > 0 &&
      successPayload.expectedRevision === 3 &&
      successPayload.title === "创建成功" &&
      JSON.stringify(successPayload.project) === JSON.stringify({ cwd: "/repo-a", label: "项目 A" }) &&
      JSON.stringify(successPayload.sessionIds) === JSON.stringify(["session-a1", "session-a2"]) &&
      !("sessionTitle" in successPayload) &&
      !("instruction" in successPayload),
    closed: !successState.open,
    busy: successState.busy,
  };

  const attachTaskId = "11111111-1111-4111-8111-111111111111";
  function attachSnapshot(revision, sessionIds) {
    const sessions = catalog().sessions;
    return {
      status: "ok",
      schemaVersion: 1,
      revision,
      tasks: [{
        id: attachTaskId,
        title: "追加会话任务",
        project: { cwd: "/repo-a", label: "项目 A" },
        status: "executing",
        order: 0,
        conversations: sessionIds.map((sessionId) => {
          const session = sessions.find((candidate) => candidate.sessionId === sessionId);
          return {
            sessionId,
            title: session?.title || sessionId,
            cwd: "/repo-a",
            updatedAtMs: session?.updatedAtMs || 0,
          };
        }),
      }],
    };
  }
  const attachExistingRequests = [];
  window.__codexElvesTaskBoardMock = {
    request(route, payload) {
      if (route === "/thread-usage-summary") {
        return { status: "ok", summary: { isRunning: false } };
      }
      if (route !== "/task-board/task-conversations-attach") {
        throw new Error(`unexpected route ${route}`);
      }
      attachExistingRequests.push(payload);
      return attachSnapshot(4, ["session-a1", "session-a2"]);
    },
  };
  window.__codexElvesTaskBoardNativeAdapter = null;
  api.resetCreateStateForTest({
    snapshot: attachSnapshot(3, ["session-a1"]),
    catalog: catalog(),
  });
  api.openAttachModalForTest(attachTaskId);
  const attachExistingBefore = createState();
  api.setCreateDraftForTest({ mode: "existing", sessionIds: ["session-a2"] });
  await api.submitCreateForTest();
  const attachExistingPayload = attachExistingRequests[0] || {};
  const attachExistingAfter = createState();
  const attachExistingRevision = api.createSnapshotForTest().revision;

  let attachNativeStarts = 0;
  let attachNativeModel = "";
  let attachNativeEffort = "";
  const attachNativeRequests = [];
  window.__codexElvesTaskBoardMock = {
    request(route, payload) {
      if (route === "/thread-usage-summary") {
        return { status: "ok", summary: { isRunning: false } };
      }
      if (route !== "/task-board/task-conversations-attach") {
        throw new Error(`unexpected route ${route}`);
      }
      attachNativeRequests.push(payload);
      return attachSnapshot(5, ["session-a1", "session-native"]);
    },
  };
  window.__codexElvesTaskBoardNativeAdapter = {
    probe: () => ({ status: "ok", canStart: true, canOpen: false }),
    startConversation: (_project, _instruction, modelId, effortId) => {
      attachNativeStarts += 1;
      attachNativeModel = modelId;
      attachNativeEffort = effortId;
      return { status: "ok", sessionId: "session-native" };
    },
  };
  api.setModelCatalogForTest({
    status: "ok",
    model: "gpt-5.6-sol",
    default_model: "gpt-5.6-sol",
    models: ["gpt-5.6-sol", "claude-sonnet-4-6"],
    model_entries: [
      {
        slug: "gpt-5.6-sol",
        display_name: "5.6 Sol",
        default_reasoning_level: "medium",
        supported_reasoning_levels: [{ effort: "low" }, { effort: "medium" }],
      },
      {
        slug: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6",
        default_reasoning_level: "high",
        supported_reasoning_levels: [{ effort: "low" }, { effort: "high" }],
      },
    ],
  });
  api.resetCreateStateForTest({
    snapshot: attachSnapshot(4, ["session-a1"]),
    catalog: catalog(),
  });
  api.openAttachModalForTest(attachTaskId);
  api.setCreateDraftForTest({
    mode: "new",
    modelId: "claude-sonnet-4-6",
    effortId: "high",
    firstInstruction: "为当前任务继续实现状态展示",
    sessionIds: [],
  });
  await api.submitCreateForTest();
  const attachNativeAfter = createState();
  const attach = {
    existing: {
      excludesAlreadyLinked:
        attachExistingBefore.purpose === "attach" &&
        JSON.stringify(attachExistingBefore.availableSessionIds) === JSON.stringify(["session-a2"]),
      exactPayloadAndSnapshot:
        JSON.stringify(Object.keys(attachExistingPayload).sort()) ===
          JSON.stringify(["expectedRevision", "sessionIds", "taskId"]) &&
        attachExistingPayload.taskId === attachTaskId &&
        attachExistingPayload.expectedRevision === 3 &&
        JSON.stringify(attachExistingPayload.sessionIds) === JSON.stringify(["session-a2"]) &&
        attachExistingRevision === 4,
      closed: !attachExistingAfter.open,
    },
    native: {
      createsThenAttaches:
        attachNativeStarts === 1 &&
        attachNativeRequests.length === 1 &&
        attachNativeRequests[0]?.taskId === attachTaskId &&
        attachNativeRequests[0]?.expectedRevision === 4 &&
        JSON.stringify(attachNativeRequests[0]?.sessionIds) === JSON.stringify(["session-native"]),
      modelForwarded: attachNativeModel === "claude-sonnet-4-6",
      effortForwarded: attachNativeEffort === "high",
      closed: !attachNativeAfter.open,
    },
  };

  const detachRequests = [];
  const detachStart = attachSnapshot(4, ["session-a1", "session-a2"]);
  window.__codexElvesTaskBoardMock = {
    request(route, payload) {
      if (route === "/thread-usage-summary") {
        return { status: "ok", summary: { isRunning: false } };
      }
      if (route !== "/task-board/task-conversations-detach") {
        throw new Error(`unexpected route ${route}`);
      }
      detachRequests.push(payload);
      return attachSnapshot(5, ["session-a2"]);
    },
  };
  api.resetCreateStateForTest({
    snapshot: detachStart,
    catalog: catalog(),
  });
  const detachTask = detachStart.tasks[0];
  const detachConversation = detachTask.conversations[0];
  const detachTrigger = document.createElement("button");
  document.body.appendChild(detachTrigger);
  detachTrigger.focus();
  api.openDetachDialogForTest(detachTask, detachConversation, detachTrigger);
  const detachContract = api.detachDialogContractForTest();
  api.closeDetachForTest();
  const cancelledWithoutRequest =
    detachRequests.length === 0 &&
    !api.detachDialogStateForTest().open &&
    document.activeElement === detachTrigger;

  api.openDetachDialogForTest(detachTask, detachConversation, detachTrigger);
  await api.submitDetachForTest();
  const detachPayload = detachRequests[0] || {};
  const detachedSnapshot = api.createSnapshotForTest();

  const detachConflictPayloads = [];
  let detachConflictCalls = 0;
  const detachConflictStart = attachSnapshot(5, ["session-a2"]);
  window.__codexElvesTaskBoardMock = {
    request(route, payload) {
      if (route === "/thread-usage-summary") {
        return { status: "ok", summary: { isRunning: false } };
      }
      if (route !== "/task-board/task-conversations-detach") {
        throw new Error(`unexpected route ${route}`);
      }
      detachConflictPayloads.push(payload);
      detachConflictCalls += 1;
      if (detachConflictCalls === 1) {
        const latest = attachSnapshot(6, ["session-a2"]);
        return {
          status: "conflict",
          code: "revision_conflict",
          schemaVersion: latest.schemaVersion,
          revision: latest.revision,
          tasks: latest.tasks,
        };
      }
      return attachSnapshot(7, []);
    },
  };
  api.resetCreateStateForTest({
    snapshot: detachConflictStart,
    catalog: catalog(),
  });
  api.openDetachDialogForTest(
    detachConflictStart.tasks[0],
    detachConflictStart.tasks[0].conversations[0],
    detachTrigger,
  );
  await api.submitDetachForTest();
  const detach = {
    confirmation:
      detachContract.role === "dialog" &&
      detachContract.ariaModal &&
      detachContract.initialFocus &&
      detachContract.title === "移除关联会话？" &&
      detachContract.message ===
        "仅解除与任务“追加会话任务”的关联，不会删除 Codex 中的原始会话。",
    cancelledWithoutRequest,
    exactPayloadAndSnapshot:
      JSON.stringify(Object.keys(detachPayload).sort()) ===
        JSON.stringify(["expectedRevision", "sessionIds", "taskId"]) &&
      detachPayload.taskId === attachTaskId &&
      detachPayload.expectedRevision === 4 &&
      JSON.stringify(detachPayload.sessionIds) === JSON.stringify(["session-a1"]) &&
      detachedSnapshot.revision === 5 &&
      JSON.stringify(
        detachedSnapshot.tasks[0].conversations.map((conversation) => conversation.sessionId),
      ) === JSON.stringify(["session-a2"]) &&
      !api.detachDialogStateForTest().open,
    revisionConflictRetriesOnce:
      detachConflictPayloads.length === 2 &&
      JSON.stringify(
        detachConflictPayloads.map((payload) => payload.expectedRevision),
      ) === JSON.stringify([5, 6]) &&
      api.createSnapshotForTest().revision === 7 &&
      !api.detachDialogStateForTest().open,
  };

  const initialStatusRequests = [];
  reset({
    snapshot: { status: "ok", schemaVersion: 1, revision: 3, tasks: [] },
    mock: {
      request(route, payload) {
        initialStatusRequests.push({ route, payload });
        if (route === "/task-board/task-create") {
          return {
            status: "ok",
            schemaVersion: 1,
            revision: 4,
            tasks: [{
              id: payload.taskId,
              title: payload.title,
              project: payload.project,
              status: "new",
              order: 0,
              conversations: [],
            }],
          };
        }
        if (route === "/task-board/task-move") {
          return {
            status: "ok",
            schemaVersion: 1,
            revision: 5,
            tasks: [{
              id: payload.taskId,
              title: "带初始状态",
              project: { cwd: "/repo-a", label: "项目 A" },
              status: payload.toStatus,
              order: 0,
              conversations: [],
            }],
          };
        }
        throw new Error(`unexpected route ${route}`);
      },
    },
  });
  api.setCreateDraftForTest({
    mode: "existing",
    title: "带初始状态",
    projectCwd: "/repo-a",
    initialStatus: "planning",
    sessionIds: ["session-a1"],
  });
  await api.submitCreateForTest();
  const initialStatus = {
    createThenMove:
      initialStatusRequests.length === 2 &&
      initialStatusRequests[0]?.route === "/task-board/task-create" &&
      initialStatusRequests[1]?.route === "/task-board/task-move",
    moveUsesCreatedRevision:
      initialStatusRequests[1]?.payload?.expectedRevision === 4 &&
      initialStatusRequests[1]?.payload?.toStatus === "planning",
    finalStatus: api.createSnapshotForTest().tasks[0]?.status,
  };

  const stableErrors = {};
  const conflictIds = [];
  for (const code of [
    "invalid_input",
    "project_mismatch",
    "task_id_conflict",
    "bridge_unavailable",
    "task_board_busy",
    "task_file_invalid",
    "task_board_unavailable",
  ]) {
    reset({
      mock: {
        request(route, payload) {
          if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
          if (code === "task_id_conflict") conflictIds.push(payload.taskId);
          return { status: "failed", code, message: `${code} message` };
        },
      },
    });
    setExisting(`错误 ${code}`);
    await api.submitCreateForTest();
    if (code === "task_id_conflict") await api.submitCreateForTest();
    const state = createState();
    stableErrors[code] = {
      feedback: state.feedback,
      modalOpen: state.open,
      busy: state.busy,
      inputsPreserved:
        state.title === `错误 ${code}` &&
        state.projectCwd === "/repo-a" &&
        JSON.stringify(state.selectedSessionIds) === JSON.stringify(["session-a1", "session-a2"]),
      nextManualRetryRotatesUuid:
        code !== "task_id_conflict" || (conflictIds.length === 2 && conflictIds[0] !== conflictIds[1]),
    };
  }

  let catalogRefreshes = 0;
  const refreshedCatalog = catalog();
  refreshedCatalog.sessions = refreshedCatalog.sessions.filter((session) => session.sessionId !== "session-a1");
  reset({
    mock: {
      request(route) {
        if (route === "/task-board/task-create") {
          return { status: "failed", code: "session_not_found", message: "session missing" };
        }
        if (route === "/task-board/session-catalog") {
          catalogRefreshes += 1;
          return refreshedCatalog;
        }
        throw new Error(`unexpected route ${route}`);
      },
    },
  });
  api.setCreateDraftForTest({ mode: "existing", title: "会话丢失", projectCwd: "/repo-a", sessionIds: ["session-a1"] });
  await api.submitCreateForTest();
  const sessionNotFoundState = createState();
  await api.submitCreateForTest();
  const sessionNotFoundRetryState = createState();
  const sessionNotFound = {
    catalogRefreshed: catalogRefreshes === 1,
    modalOpen: sessionNotFoundState.open,
    busy: sessionNotFoundState.busy,
    staleSelectionCleared: sessionNotFoundState.selectedSessionIds.length === 0,
    nextSubmitRequiresSelection: sessionNotFoundRetryState.feedback.includes("至少选择"),
  };

  const retryPayloads = [];
  let retryCalls = 0;
  reset({
    mock: {
      request(route, payload) {
        if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
        retryPayloads.push(payload);
        retryCalls += 1;
        if (retryCalls === 1) {
          const latest = snapshot(4, "冲突后快照");
          return { status: "conflict", schemaVersion: latest.schemaVersion, revision: latest.revision, tasks: latest.tasks };
        }
        return snapshot(5, "重试成功");
      },
    },
  });
  setExisting("重试任务");
  await api.submitCreateForTest();
  const revisionConflict = {
    retriedOnce: retryPayloads.length === 2,
    sameTaskId: retryPayloads[0]?.taskId === retryPayloads[1]?.taskId,
    expectedRevisions: retryPayloads.map((payload) => payload.expectedRevision),
    closedAfterRetry: !createState().open,
  };

  let secondConflictCalls = 0;
  reset({
    mock: {
      request(route) {
        if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
        secondConflictCalls += 1;
        const latest = snapshot(3 + secondConflictCalls, "仍然冲突");
        return { status: "conflict", schemaVersion: latest.schemaVersion, revision: latest.revision, tasks: latest.tasks };
      },
    },
  });
  setExisting("二次冲突");
  await api.submitCreateForTest();
  const secondConflictState = createState();
  revisionConflict.secondConflictStops =
    secondConflictCalls === 2 && secondConflictState.open && secondConflictState.feedback.includes("修订");
  revisionConflict.secondConflictBusy = secondConflictState.busy;

  let nativeStartCalls = 0;
  reset({
    nativeAdapter: {
      probe: () => ({ status: "ok", canStart: true, canOpen: false }),
      startConversation: () => { nativeStartCalls += 1; return { status: "ok", sessionId: "must-not-start" }; },
    },
    mock: {
      request(route) {
        if (route === "/task-board/task-create") throw new Error("T-010 must not create from native mode");
        return catalog();
      },
    },
  });
  api.setCreateDraftForTest({ mode: "new", title: "原生流程未启用", projectCwd: "/repo-a", sessionIds: [] });
  await api.submitCreateForTest();
  const nativeState = createState();
  const nativeMode = {
    instructionRequiredStaysOpen:
      nativeState.open && !nativeState.busy && nativeState.feedback.includes("首条指令"),
    neverStartsConversation: nativeStartCalls === 0,
  };

  const retryIds = [];
  let manualCalls = 0;
  reset({
    mock: {
      request(route, payload) {
        if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
        retryIds.push(payload.taskId);
        manualCalls += 1;
        if (manualCalls === 1) throw new Error("lost response");
        return { status: "failed", code: "bridge_unavailable", message: "bridge unavailable" };
      },
    },
  });
  setExisting("幂等重试");
  await api.submitCreateForTest();
  await api.submitCreateForTest();
  const renamedCatalog = catalog();
  renamedCatalog.projects[0].label = "项目 A（改名）";
  api.applyCatalogForTest(renamedCatalog);
  await api.submitCreateForTest();
  const taskIdBeforeSemanticChange = retryIds[retryIds.length - 1];
  api.setCreateDraftForTest({ title: "幂等重试已修改" });
  await api.submitCreateForTest();
  const idempotency = {
    uuidIsValid: /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(retryIds[0] || ""),
    manualRetryReusesUuid: retryIds.length >= 2 && retryIds[0] === retryIds[1],
    labelOnlyChangeReusesUuid: retryIds.length >= 3 && retryIds[1] === retryIds[2],
    semanticChangeRotatesUuid: retryIds.length >= 4 && taskIdBeforeSemanticChange !== retryIds[3],
  };

  let resolveDeferredCreate;
  const deferredCreate = new Promise((resolve) => { resolveDeferredCreate = resolve; });
  reset({
    mock: {
      request(route) {
        if (route !== "/task-board/task-create") throw new Error(`unexpected route ${route}`);
        return deferredCreate;
      },
    },
  });
  setExisting("延迟关闭");
  const pendingCreate = api.submitCreateForTest();
  api.refreshRuntimeForTest();
  resolveDeferredCreate(snapshot(4, "不应写入"));
  await pendingCreate;
  const lifecycle = {
    deferredClosePreventsLateWrite: !createState().open && api.createSnapshotForTest().revision === 3,
  };
  const wideToolbar = api.toolbarLayoutForTest(996, 785);
  const narrowToolbar = api.toolbarLayoutForTest(780, 400);
  const toolbar = {
    wideInlineAdjacent:
      wideToolbar.mode === "inline" &&
      JSON.stringify(wideToolbar.controls) === JSON.stringify(["search", "filter", "create"]),
    narrowWrapsWith36pxCreate:
      narrowToolbar.mode === "wrapped" &&
      narrowToolbar.createMinHeight === 36 &&
      JSON.stringify(narrowToolbar.controls) === JSON.stringify(["search", "filter", "create"]),
  };

  process.stdout.write(JSON.stringify({
    modal,
    dropdowns,
    projectSelection,
    validation,
    success,
    attach,
    detach,
    initialStatus,
    stableErrors,
    sessionNotFound,
    revisionConflict,
    nativeMode,
    idempotency,
    lifecycle,
    toolbar,
  }));
  process.exit(0);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
"#,
    )
    .expect("task board create harness should be written");

    let output = Command::new("node")
        .arg(&harness_path)
        .arg(&script_path)
        .output()
        .expect("node should run task board create harness");
    assert!(
        output.status.success(),
        "node task board create harness failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("task board create harness stdout should be JSON")
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
fn manager_ui_exposes_default_enabled_task_board_page_enhancement() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core crate should live under crates/codex-elves-core");
    let source =
        std::fs::read_to_string(repo.join("apps/codex-elves-manager/src/App.tsx")).unwrap();

    assert!(source.contains("codexAppTaskBoard: boolean"));
    assert!(source.contains("codexAppTaskBoard: true"));
    assert!(source.contains("title=\"任务看板\""));
    assert!(source.contains("checked={form.codexAppTaskBoard}"));
    assert!(source.contains("setEnhanceFlag(\"codexAppTaskBoard\", value)"));
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
    assert!(!source.contains("codexAppPluginAutoExpand"));
    assert!(!source.contains("插件列表全量展示"));
    assert!(!source.contains("界面背景主题已升级为"));
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
fn pick_injectable_codex_page_target_ignores_avatar_overlay_window() {
    let targets = vec![
        target(
            "avatar-overlay",
            "page",
            "Codex",
            "app://-/index.html?initialRoute=%2Favatar-overlay",
            Some("ws://avatar-overlay"),
        ),
        target(
            "workspace",
            "page",
            "Codex",
            "app://-/index.html",
            Some("ws://workspace"),
        ),
    ];

    let picked = pick_injectable_codex_page_target(&targets)
        .expect("main Codex window should be selected instead of avatar overlay");

    assert_eq!(picked.id, "workspace");
}

#[test]
fn pick_injectable_codex_page_target_rejects_avatar_overlay_only() {
    let targets = vec![target(
        "avatar-overlay",
        "page",
        "Codex",
        "app://-/index.html?initialRoute=/avatar-overlay",
        Some("ws://avatar-overlay"),
    )];

    let error = pick_injectable_codex_page_target(&targets)
        .expect_err("avatar overlay must not receive the Codex bridge");

    assert!(
        error
            .to_string()
            .contains("No injectable ChatGPT/Codex page target found")
    );
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
