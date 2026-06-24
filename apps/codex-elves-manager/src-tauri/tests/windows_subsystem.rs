#[cfg(windows)]
#[test]
fn manager_binary_uses_windows_gui_subsystem_in_debug_and_release() {
    let main_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"))
        .expect("read manager main.rs");

    assert!(
        main_rs.contains("#![cfg_attr(windows, windows_subsystem = \"windows\")]"),
        "manager binary should not allocate a console window on Windows"
    );
}

#[test]
fn manager_release_binary_uses_embedded_frontend_assets() {
    let cargo_toml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read manager Cargo.toml");

    assert!(
        cargo_toml.contains("custom-protocol"),
        "release manager binary should use Tauri custom protocol instead of devUrl localhost"
    );
}

#[test]
fn manager_uses_single_instance_guard_before_starting_tauri() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("acquire_single_instance_guard()"));
    assert!(lib_rs.contains("manager_guard_port()"));
    assert!(lib_rs.contains("manager.already_running"));
}

#[test]
fn manager_dev_mode_has_separate_title_and_window_state() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("CODEX_ELVES_MANAGER_DEV"));
    assert!(lib_rs.contains("CodexElves 管理工具 Dev"));
    assert!(lib_rs.contains("manager-window-state-dev.json"));
    assert!(lib_rs.contains("manager_window_title()"));
    assert!(lib_rs.contains("manager_window_state_file()"));
}

#[test]
fn manager_dev_mode_loads_vite_dev_url_for_manual_window() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("manager_webview_url(show_update)?"));
    assert!(lib_rs.contains("tauri::WebviewUrl::External"));
    assert!(lib_rs.contains("http://localhost:1420/"));
    assert!(lib_rs.contains("tauri::WebviewUrl::App(url.into())"));
}

#[test]
fn manager_default_capability_allows_vite_dev_origin() {
    let capability = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/capabilities/default.json"
    ))
    .expect("read default capability");

    assert!(capability.contains("\"local\": true"));
    assert!(capability.contains("\"remote\""));
    assert!(capability.contains("\"urls\""));
    assert!(capability.contains("\"http://localhost:1420\""));
    assert!(capability.contains("\"http://localhost:1420/*\""));
}

#[test]
fn dev_manager_script_sets_isolated_dev_environment() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/dev-manager.ps1");
    let script = std::fs::read_to_string(&script).expect("read dev manager script");

    assert!(script.contains("CODEX_ELVES_MANAGER_DEV"));
    assert!(script.contains("CODEX_ELVES_MANAGER_GUARD_PORT"));
    assert!(script.contains("[int]$GuardPort = 45229"));
    assert!(script.contains("npm run dev"));
}

#[test]
fn manager_second_launch_requests_existing_window_to_show() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("spawn_manager_wake_listener"));
    assert!(lib_rs.contains("request_existing_manager_to_show"));
    assert!(lib_rs.contains("MANAGER_WAKE_MESSAGE"));
    assert!(lib_rs.contains("MANAGER_WAKE_ACK"));
    assert!(lib_rs.contains("stream.write_all(MANAGER_WAKE_ACK)"));
    assert!(lib_rs.contains("fallback_single_instance_guard()"));
    assert!(lib_rs.contains("wake_requested"));
    assert!(lib_rs.contains("show_main_window(&app_handle)"));
}

#[test]
fn launcher_binary_embeds_codex_icon_resource() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let launcher_build = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("codex-elves-launcher/build.rs");
    let build_rs = std::fs::read_to_string(&launcher_build).expect("read launcher build.rs");

    assert!(build_rs.contains("WindowsResource"));
    assert!(build_rs.contains("icons/icon.ico"));
}

#[test]
fn windows_binaries_request_administrator_privileges() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manager_build =
        std::fs::read_to_string(manifest_dir.join("build.rs")).expect("read manager build.rs");
    let windows_manifest = std::fs::read_to_string(manifest_dir.join("windows-app-manifest.xml"))
        .expect("read windows app manifest");
    let launcher_build = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("codex-elves-launcher/build.rs");
    let launcher_build = std::fs::read_to_string(&launcher_build).expect("read launcher build.rs");
    let windows_installer = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/installer/windows/CodexElves.nsi");
    let windows_installer =
        std::fs::read_to_string(&windows_installer).expect("read windows installer");

    assert!(manager_build.contains("windows-app-manifest.xml"));
    assert!(launcher_build.contains("windows-app-manifest.xml"));
    assert!(windows_manifest.contains("requireAdministrator"));
    assert!(windows_manifest.contains("Microsoft.Windows.Common-Controls"));
    assert!(windows_installer.contains("RequestExecutionLevel admin"));
}

#[test]
fn manager_launch_button_spawns_silent_launcher_binary() {
    let commands_rs =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands.rs"))
            .expect("read manager commands.rs");

    assert!(commands_rs.contains("SILENT_BINARY"));
    assert!(commands_rs.contains("std::process::Command::new"));
    assert!(!commands_rs.contains("launch_and_inject_with_hooks(options"));
}

#[test]
fn frontend_literal_tauri_commands_are_registered_for_invocation() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib_rs = std::fs::read_to_string(manifest_dir.join("src/lib.rs")).expect("read lib.rs");
    let commands_rs =
        std::fs::read_to_string(manifest_dir.join("src/commands.rs")).expect("read commands.rs");
    let app_tsx = std::fs::read_to_string(manifest_dir.parent().unwrap().join("src/App.tsx"))
        .expect("read App.tsx");
    let default_capability =
        std::fs::read_to_string(manifest_dir.join("capabilities/default.json"))
            .expect("read default capability");
    let default_permissions =
        std::fs::read_to_string(manifest_dir.join("permissions/default.toml"))
            .expect("read default permissions");

    let frontend_commands = literal_tauri_commands(&app_tsx);
    assert!(default_capability.contains("\"allow-manager-commands\""));
    for expected in [
        "load_ccs_providers",
        "import_ccs_providers",
        "plugin_marketplace_status",
        "repair_plugin_marketplace",
    ] {
        assert!(
            frontend_commands.contains(expected),
            "expected frontend command {expected} to be covered"
        );
    }

    for command in frontend_commands {
        let sync_fn = format!("pub fn {command}");
        let async_fn = format!("pub async fn {command}");
        assert!(
            commands_rs.contains(&sync_fn) || commands_rs.contains(&async_fn),
            "frontend command {command} should have a backend command implementation"
        );
        assert!(
            lib_rs.contains(&format!("commands::{command}")),
            "frontend command {command} should be registered in Tauri invoke_handler"
        );
        assert!(
            default_permissions.contains(&format!("\"{command}\"")),
            "frontend command {command} should be allowed by app permissions"
        );
    }
}

fn literal_tauri_commands(source: &str) -> std::collections::BTreeSet<String> {
    let mut commands = std::collections::BTreeSet::new();

    for marker in ["call", "invoke"] {
        let mut offset = 0;
        while let Some(relative_start) = source[offset..].find(marker) {
            let start = offset + relative_start;
            let after_marker = start + marker.len();
            if source[after_marker..]
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_')
            {
                offset = after_marker;
                continue;
            }

            let Some(relative_open_paren) = source[after_marker..].find('(') else {
                break;
            };
            let open_paren = after_marker + relative_open_paren;
            let argument = source[open_paren + 1..].trim_start();
            let Some(quote) = argument.chars().next() else {
                offset = open_paren + 1;
                continue;
            };
            if quote != '"' && quote != '\'' {
                offset = open_paren + 1;
                continue;
            }

            let rest = &argument[quote.len_utf8()..];
            if let Some(end) = rest.find(quote) {
                commands.insert(rest[..end].to_string());
            }
            offset = open_paren + 1;
        }
    }

    commands
}

#[test]
fn macos_packager_hides_silent_launcher_but_not_manager() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let packager = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/installer/macos/package-dmg.sh");
    let script = std::fs::read_to_string(&packager).expect("read macOS packager");

    assert!(script.contains("<key>LSUIElement</key>"));
    assert!(script.contains("ARCH=\"${2:-$(uname -m)}\""));
    assert!(script.contains("BINARY_DIR=\"${BINARY_DIR:-$ROOT/target/release}\""));
    assert!(script.contains("CodexElves-${VERSION}-macos-${ARCH}.dmg"));
    assert!(script.contains(
        "create_app \"CodexElves\" \"CodexElves\" \"$BINARY_DIR/codex-elves\" \"com.bigpizzav3.codexelves\" \"true\""
    ));
    assert!(script.contains(
        "create_app \"CodexElves 管理工具\" \"CodexElvesManager\" \"$BINARY_DIR/codex-elves-manager\" \"com.bigpizzav3.codexelves.manager\" \"false\""
    ));
}

#[test]
fn github_release_workflow_builds_separate_macos_x64_and_arm64_dmgs() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("macos-15-intel"));
    assert!(workflow.contains("x86_64-apple-darwin"));
    assert!(workflow.contains("macos-14"));
    assert!(workflow.contains("aarch64-apple-darwin"));
    assert!(workflow.contains("package-dmg.sh \"$VERSION\" \"${{ matrix.arch }}\""));
    assert!(workflow.contains("target/${{ matrix.target }}/release"));
}

#[test]
fn github_release_workflow_can_build_assets_from_tags_and_manual_dispatch() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("tags:"));
    assert!(workflow.contains("- \"v*\""));
    assert!(workflow.contains("ensure-release:"));
    assert!(workflow.contains("gh release create \"$TAG\""));
    assert!(workflow.contains("release-notes.md"));
    assert!(workflow.contains("gh release edit \"$TAG\""));
    assert!(workflow.contains("CodexElves $VERSION 发布版本。"));
    assert!(
        workflow
            .contains("gh release edit \"$TAG\" --repo \"$REPO\" --notes-file release-notes.md")
    );
    assert!(workflow.contains("ref: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("TAG: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("gh release upload $env:TAG @($files.FullName) --clobber"));
    assert!(workflow.contains("gh release upload \"$TAG\" dist/macos/*.dmg --clobber"));
    assert!(!workflow.contains("softprops/action-gh-release"));
}

#[test]
fn github_workflows_install_frontend_dependencies_from_lockfile() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows");

    for workflow_name in ["release-assets.yml", "pr-build.yml"] {
        let workflow = std::fs::read_to_string(workflow_root.join(workflow_name))
            .unwrap_or_else(|error| panic!("read {workflow_name}: {error}"));
        assert!(
            workflow.contains("run: npm ci"),
            "{workflow_name} should install frontend dependencies from package-lock.json"
        );
        assert!(
            !workflow.contains("npm install --package-lock=false"),
            "{workflow_name} should not ignore the committed package-lock.json"
        );
    }
}

#[test]
fn github_release_workflow_uploads_static_latest_json() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("latest-json:"));
    assert!(workflow.contains("latest.json"));
    assert!(workflow.contains("- ensure-release"));
    assert!(workflow.contains("TAG: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("gh release upload \"$TAG\" latest.json --clobber"));
}

#[test]
fn relay_settings_uses_structured_config_and_isolated_auth() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let commands_rs = manifest_dir.join("src/commands.rs");
    let commands_rs = std::fs::read_to_string(&commands_rs).expect("read manager commands.rs");

    assert!(app_tsx.contains("switch_relay_profile"));
    assert!(app_tsx.contains("previousActiveRelayId"));
    assert!(app_tsx.contains("relayProfileSwitchValidation(selectedBeforeSave)"));
    assert!(app_tsx.contains("RelayActivationPanel"));
    assert!(app_tsx.contains("启用后会修改"));
    assert!(app_tsx.contains("auth.json 存档"));
    assert!(app_tsx.contains("saveRelayAuthFile"));
    assert!(!app_tsx.contains("RelayFileEditors"));
    assert!(!app_tsx.contains("config.toml 预览"));
    assert!(!app_tsx.contains("提取当前供应商配置"));
    assert!(!app_tsx.contains("启用目标功能"));
    assert!(commands_rs.contains("供应商配置不再支持直接保存 config.toml"));
    assert!(commands_rs.contains("backfill_relay_profile_from_live"));
    assert!(commands_rs.contains("switch_relay_profile_in_home"));
}

#[test]
fn relay_context_management_is_global_not_supplier_scoped() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let styles = manifest_dir.parent().unwrap().join("src/styles.css");
    let styles = std::fs::read_to_string(&styles).expect("read manager styles.css");

    assert!(app_tsx.contains("作为全局配置独立管理"));
    assert!(app_tsx.contains("label: \"工具与插件\""));
    assert!(app_tsx.contains("title=\"Codex 工具与插件\""));
    assert!(!app_tsx.contains("label: \"上下文配置\""));
    assert!(!app_tsx.contains("title=\"上下文配置\""));
    assert!(!app_tsx.contains("<strong>Codex 上下文</strong>"));
    assert!(app_tsx.contains("id: \"context\""));
    assert!(app_tsx.contains("function ContextScreen"));
    assert!(app_tsx.contains("route === \"context\""));
    assert!(app_tsx.contains("if (next === \"context\")"));
    assert!(app_tsx.contains("contextConfigTextFromConfig(configContents, entries)"));
    assert!(app_tsx.contains("toggleContextEntryEnabled"));
    assert!(app_tsx.contains("relayFiles={relayFiles}"));
    assert!(app_tsx.contains("read_live_context_entries"));
    assert!(app_tsx.contains("sync_live_context_entries"));
    assert!(app_tsx.contains("refreshLiveContextEntries"));
    assert!(app_tsx.contains("syncLiveContextEntries(next, true, { kind"));
    assert!(app_tsx.contains("function contextEntriesWithLiveEntries"));
    assert!(app_tsx.contains("liveByKind"));
    assert!(app_tsx.contains("mergeLiveContextEntries"));
    assert!(app_tsx.contains("withLiveEntryState"));
    assert!(app_tsx.contains("contextEnabledSwitch"));
    assert!(!app_tsx.contains("entry.enabled ? \"已启用\" : \"已禁用\""));
    assert!(!app_tsx.contains("空配置体"));
    assert!(app_tsx.contains("relay-context-delete"));
    assert!(!app_tsx.contains("切换供应商时只合并勾选项"));
    assert!(!app_tsx.contains("未勾选的条目不会写入"));
    assert!(!app_tsx.contains("className=\"context-switch\""));
    assert!(!styles.contains(".context-switch {"));
    assert!(styles.contains(".context-enabled-switch"));
    assert!(styles.contains(".context-switch-track"));
    assert!(styles.contains(".context-switch-thumb"));
    assert!(!styles.contains(".relay-context-row code"));
    assert!(styles.contains(".relay-context-delete"));
}

#[test]
fn manager_window_and_relay_detail_header_stay_usable() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let styles = manifest_dir.parent().unwrap().join("src/styles.css");
    let styles = std::fs::read_to_string(&styles).expect("read manager styles.css");
    let lib_rs =
        std::fs::read_to_string(manifest_dir.join("src/lib.rs")).expect("read manager lib.rs");
    let tauri_conf =
        std::fs::read_to_string(manifest_dir.join("tauri.conf.json")).expect("read tauri config");

    assert!(app_tsx.contains("relay-detail-sticky"));
    assert!(!app_tsx.contains("CardHead title=\"供应商详情\""));
    assert!(styles.contains(".relay-detail-sticky"));
    assert!(styles.contains("position: sticky"));
    assert!(styles.contains("top: 0"));
    assert!(styles.contains("margin: 0"));
    assert!(lib_rs.contains("DEFAULT_WINDOW_WIDTH"));
    assert!(lib_rs.contains("DEFAULT_WINDOW_HEIGHT"));
    assert!(lib_rs.contains("MIN_WINDOW_WIDTH"));
    assert!(lib_rs.contains("MIN_WINDOW_HEIGHT"));
    assert!(lib_rs.contains("MANAGER_WINDOW_STATE_FILE"));
    assert!(lib_rs.contains("visible(false)"));
    assert!(lib_rs.contains("apply_manager_window_state"));
    assert!(lib_rs.contains("manager_window_state_is_visible"));
    assert!(lib_rs.contains("persist_manager_window_state"));
    assert!(tauri_conf.contains("\"width\": 1180"));
    assert!(tauri_conf.contains("\"height\": 820"));
    assert!(tauri_conf.contains("\"minWidth\": 960"));
    assert!(tauri_conf.contains("\"minHeight\": 720"));
}

#[test]
fn relay_preview_deduplicates_root_keys_when_merging_common_config() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");

    assert!(app_tsx.contains("dedupeTomlRootLines"));
    assert!(app_tsx.contains("rootSeen.add(key)"));
    assert!(app_tsx.contains("joinTomlSectionsRootFirst"));
}
