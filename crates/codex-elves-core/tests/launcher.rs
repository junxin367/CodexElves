mod support;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use codex_elves_core::app_paths::{
    build_codex_executable, codex_app_version, find_latest_codex_app_dir,
    find_latest_codex_app_dir_from_roots, find_macos_codex_app, find_standalone_codex_app_dir_from,
    latest_appx_install_location_from_output, normalize_codex_app_path, packaged_app_user_model_id,
    resolve_codex_app_dir_with_saved, user_data_candidates_from,
};
use codex_elves_core::launcher::{
    CodexLaunch, DefaultLaunchHooks, LaunchHooks, LaunchOptions, MacosCleanupPolicy,
    build_codex_arguments, build_codex_command, build_macos_cleanup_command,
    build_macos_open_command, build_packaged_activation, launch_and_inject_with_hooks,
};
#[cfg(windows)]
use codex_elves_core::launcher::{WindowsProcessControlStrategy, windows_process_control_strategy};
use codex_elves_core::ports::{
    select_packaged_codex_debug_port_with, select_platform_loopback_port_with,
};
use codex_elves_core::proxy_log::{ProxyRequestRecord, ProxyRequestState, ProxyRequestTransport};
use codex_elves_core::request_headers::{RequestContext, UpstreamHeaderRoute};
use codex_elves_core::settings::{BackendSettings, RelayProfile, RelayProtocol};
use codex_elves_core::status::StatusStore;
use support::DiagnosticLogCapture;

#[test]
fn request_context_rebuilds_only_native_responses_semantic_headers() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.append(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("remote_compaction_v2"),
    );
    headers.append(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("future_native_feature"),
    );
    headers.insert(
        "x-codex-turn-state",
        reqwest::header::HeaderValue::from_static("turn-state"),
    );
    headers.insert(
        "openai-beta",
        reqwest::header::HeaderValue::from_static("responses_websockets=2026-02-06"),
    );
    headers.insert(
        "connection",
        reqwest::header::HeaderValue::from_static("x-codex-turn-state, x-unrelated"),
    );
    headers.insert(
        "authorization",
        reqwest::header::HeaderValue::from_static("Bearer local-client-token"),
    );
    headers.insert(
        "x-forwarded-for",
        reqwest::header::HeaderValue::from_static("203.0.113.42"),
    );

    let context = RequestContext::from_headers(headers);
    let native = context.headers_for(UpstreamHeaderRoute::NativeResponsesHttp);
    let beta_features = native
        .get_all("x-codex-beta-features")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        beta_features,
        ["remote_compaction_v2", "future_native_feature"]
    );
    assert_eq!(
        native
            .get("openai-beta")
            .and_then(|value| value.to_str().ok()),
        Some("responses_websockets=2026-02-06")
    );
    assert!(
        native.get("x-codex-turn-state").is_none(),
        "Connection-declared headers are hop-by-hop and must not be forwarded"
    );
    assert!(native.get("authorization").is_none());
    assert!(native.get("x-forwarded-for").is_none());
    assert!(
        context
            .headers_for(UpstreamHeaderRoute::ConvertedProtocol)
            .is_empty()
    );
}

#[test]
fn app_paths_find_latest_windows_package_prefers_highest_version_app_dir() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_1.2.3.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(temp.path().join("OpenAI.Codex_not-a-version_x64__abc")).unwrap();

    let latest = find_latest_codex_app_dir(temp.path()).unwrap();

    assert_eq!(
        latest,
        temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc/app")
    );
}

#[test]
fn app_paths_find_latest_windows_package_detects_beta_package() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join("OpenAI.CodexBeta_26.527.7698.0_x64__2p2nqsd0c76g0/app"),
    )
    .unwrap();

    let latest = find_latest_codex_app_dir(temp.path()).unwrap();

    assert_eq!(
        latest,
        temp.path()
            .join("OpenAI.CodexBeta_26.527.7698.0_x64__2p2nqsd0c76g0/app")
    );
    assert_eq!(codex_app_version(&latest).as_deref(), Some("26.527.7698.0"));
    assert_eq!(
        packaged_app_user_model_id(&latest).as_deref(),
        Some("OpenAI.CodexBeta_2p2nqsd0c76g0!App")
    );
}

#[test]
fn app_paths_find_latest_windows_package_returns_package_when_app_dir_missing() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("OpenAI.Codex_26.429.8261.0_x64__abc");
    std::fs::create_dir_all(&package).unwrap();

    assert_eq!(find_latest_codex_app_dir(temp.path()).unwrap(), package);
}

#[test]
fn app_paths_find_latest_windows_package_checks_roots_before_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("WindowsApps");
    std::fs::create_dir_all(root.join("OpenAI.Codex_1.0.0.0_x64__abc/app")).unwrap();
    std::fs::create_dir_all(root.join("OpenAI.Codex_26.513.3673.0_x64__abc/app")).unwrap();

    let latest = find_latest_codex_app_dir_from_roots(&[root]).unwrap();

    assert!(latest.ends_with("OpenAI.Codex_26.513.3673.0_x64__abc/app"));
}

#[test]
fn app_paths_extracts_codex_version_from_windows_package_app_dir() {
    let app_dir =
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex_26.513.3673.0_x64__abc\app");

    assert_eq!(
        codex_app_version(&app_dir).as_deref(),
        Some("26.513.3673.0")
    );
}

#[test]
fn app_paths_extracts_codex_version_from_macos_bundle_plist() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("OpenAI Codex.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleVersion</key>
  <string>26.500.0</string>
  <key>CFBundleShortVersionString</key>
  <string>26.513.3673</string>
</dict>
</plist>
"#,
    )
    .unwrap();

    assert_eq!(codex_app_version(&app).as_deref(), Some("26.513.3673"));
}

#[test]
fn app_paths_user_data_candidates_include_local_and_roaming_variants() {
    let local = PathBuf::from(r"C:\Users\me\AppData\Local");
    let roaming = PathBuf::from(r"C:\Users\me\AppData\Roaming");

    let candidates = user_data_candidates_from(Some(&local), Some(&roaming));

    assert_eq!(
        candidates,
        vec![
            local.join("OpenAI").join("Codex"),
            local.join("OpenAI.Codex"),
            local.join("Codex"),
            roaming.join("OpenAI").join("Codex"),
            roaming.join("OpenAI.Codex"),
            roaming.join("Codex"),
        ]
    );
}

#[test]
fn app_paths_find_macos_codex_app_prefers_first_search_root_and_known_names() {
    let temp = tempfile::tempdir().unwrap();
    let system_root = temp.path().join("Applications");
    let user_root = temp.path().join("Users/me/Applications");
    let system_app = system_root.join("OpenAI Codex.app");
    let user_app = user_root.join("Codex.app");
    std::fs::create_dir_all(&system_app).unwrap();
    std::fs::create_dir_all(&user_app).unwrap();

    assert_eq!(
        find_macos_codex_app(&[system_root, user_root]).unwrap(),
        system_app
    );
}

#[test]
fn app_paths_find_macos_codex_app_accepts_chatgpt_bundle_name() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Applications");
    let app = root.join("ChatGPT.app");
    std::fs::create_dir_all(&app).unwrap();

    assert_eq!(find_macos_codex_app(&[root]).unwrap(), app);
}

#[test]
fn app_paths_find_macos_codex_app_prefers_chatgpt_across_search_roots() {
    let temp = tempfile::tempdir().unwrap();
    let system_root = temp.path().join("Applications");
    let user_root = temp.path().join("Users/me/Applications");
    let legacy_app = system_root.join("Codex.app");
    let chatgpt_app = user_root.join("OpenAI ChatGPT.app");
    std::fs::create_dir_all(&legacy_app).unwrap();
    std::fs::create_dir_all(&chatgpt_app).unwrap();

    assert_eq!(
        find_macos_codex_app(&[system_root, user_root]).unwrap(),
        chatgpt_app
    );
}

#[test]
fn app_paths_build_macos_bundle_executable() {
    let app = PathBuf::from("/Applications/OpenAI Codex.app");

    assert_eq!(
        build_codex_executable(&app),
        PathBuf::from("/Applications/OpenAI Codex.app/Contents/MacOS/Codex")
    );
}

#[test]
fn app_paths_build_macos_chatgpt_bundle_uses_plist_executable() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("ChatGPT.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::write(
        contents.join("Info.plist"),
        r#"<plist><dict>
<key>CFBundleExecutable</key>
<string>ChatGPT</string>
</dict></plist>"#,
    )
    .unwrap();

    assert_eq!(
        build_codex_executable(&app),
        app.join("Contents").join("MacOS").join("ChatGPT")
    );
}

#[test]
fn app_paths_normalizes_executable_and_package_paths() {
    let temp = tempfile::tempdir().unwrap();
    let portable = temp.path().join("CodexPortable");
    let app = portable.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("Codex.exe"), "").unwrap();

    assert_eq!(
        normalize_codex_app_path(&app.join("Codex.exe")).as_deref(),
        Some(app.as_path())
    );
    assert_eq!(
        normalize_codex_app_path(&portable).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn app_paths_prefers_chatgpt_desktop_executable_and_avoids_bundled_cli() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    let resources = app.join("resources");
    std::fs::create_dir_all(&resources).unwrap();
    std::fs::write(app.join("ChatGPT.exe"), "").unwrap();
    std::fs::write(app.join("Codex.exe"), "").unwrap();
    let bundled_cli = resources.join("codex.exe");
    std::fs::write(&bundled_cli, "").unwrap();

    assert_eq!(build_codex_executable(&app), app.join("ChatGPT.exe"));
    assert_eq!(
        normalize_codex_app_path(&app.join("ChatGPT.exe")).as_deref(),
        Some(app.as_path())
    );
    assert_eq!(
        normalize_codex_app_path(&bundled_cli).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn app_paths_migrates_saved_legacy_executable_to_chatgpt_sibling() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("ChatGPT.exe"), "").unwrap();

    assert_eq!(
        normalize_codex_app_path(&app.join("Codex.exe")).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn app_paths_migrates_saved_legacy_macos_bundle_to_chatgpt_sibling() {
    let temp = tempfile::tempdir().unwrap();
    let chatgpt_app = temp.path().join("ChatGPT.app");
    std::fs::create_dir_all(&chatgpt_app).unwrap();

    assert_eq!(
        normalize_codex_app_path(&temp.path().join("Codex.app")).as_deref(),
        Some(chatgpt_app.as_path())
    );
}

#[test]
fn app_paths_detects_chatgpt_standalone_only_with_codex_runtime_marker() {
    let temp = tempfile::tempdir().unwrap();
    let local = temp.path().join("Local");
    let classic = local.join("Programs").join("ChatGPT");
    std::fs::create_dir_all(&classic).unwrap();
    std::fs::write(classic.join("ChatGPT.exe"), "").unwrap();
    assert_eq!(find_standalone_codex_app_dir_from(&local), None);

    let app = local.join("Programs").join("OpenAI").join("ChatGPT");
    std::fs::create_dir_all(app.join("resources")).unwrap();
    std::fs::write(app.join("ChatGPT.exe"), "").unwrap();
    std::fs::write(app.join("resources").join("codex.exe"), "").unwrap();

    assert_eq!(
        find_standalone_codex_app_dir_from(&local).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn app_paths_saved_path_is_used_when_no_explicit_path_is_provided() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app).unwrap();

    assert_eq!(
        resolve_codex_app_dir_with_saved(None, Some(&app.to_string_lossy())).as_deref(),
        Some(app.as_path())
    );
}

#[test]
fn launcher_builds_debug_arguments_and_commands() {
    let app_dir = PathBuf::from(r"C:\Codex\app");

    assert_eq!(
        build_codex_arguments(9229, &[]),
        vec![
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
        ]
    );
    let command = build_codex_command(&app_dir, 9229, &[]);
    assert!(command[0].ends_with("ChatGPT.exe"));
    assert_eq!(command[1], "--remote-debugging-port=9229");
    assert_eq!(command[2], "--remote-allow-origins=http://127.0.0.1:9229");
}

#[test]
fn launcher_does_not_override_codex_app_environment() {
    let source = include_str!("../src/launcher.rs");

    assert!(!source.contains(".envs(codex_process_environment())"));
    assert!(!source.contains("activate_packaged_app_with_environment"));
    assert!(!source.contains("with_temporary_proxy_environment"));
}

#[test]
fn launcher_windows_process_wait_uses_platform_cfg_guards() {
    let source = include_str!("../src/launcher.rs").replace("\r\n", "\n");

    assert!(source.contains(
        "#[cfg(windows)]\nasync fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()>"
    ));
    assert!(source.contains(
        "#[cfg(not(windows))]\nasync fn wait_for_windows_process_id(process_id: u32) -> anyhow::Result<()>"
    ));
    assert!(source.contains(
        "#[cfg(windows)]\nfn wait_for_windows_process_id_blocking(process_id: u32) -> anyhow::Result<()>"
    ));
    assert!(
        source.contains("#[cfg(windows)]\n        {\n            let mut empty_streak = 0u32;")
    );
}

#[test]
fn launcher_appends_extra_codex_arguments_after_debug_arguments() {
    let app_dir = PathBuf::from(r"C:\Codex\app");
    let extra_args = vec![
        "--force_high_performance_gpu".to_string(),
        "  ".to_string(),
        "--enable-features=UseOzonePlatform".to_string(),
    ];

    assert_eq!(
        build_codex_arguments(9229, &extra_args),
        vec![
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--force_high_performance_gpu".to_string(),
            "--enable-features=UseOzonePlatform".to_string(),
        ]
    );
    let command = build_codex_command(&app_dir, 9229, &extra_args);
    assert_eq!(command[1], "--remote-debugging-port=9229");
    assert_eq!(command[2], "--remote-allow-origins=http://127.0.0.1:9229");
    assert_eq!(command[3], "--force_high_performance_gpu");
    assert_eq!(command[4], "--enable-features=UseOzonePlatform");
}

#[test]
fn launcher_constructs_windows_packaged_activation_without_real_app() {
    let app_dir = PathBuf::from(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.506.2212.0_x64__2p2nqsd0c76g0\app",
    );

    assert_eq!(
        packaged_app_user_model_id(&app_dir).unwrap(),
        "OpenAI.Codex_2p2nqsd0c76g0!App"
    );
    assert_eq!(
        build_packaged_activation(&app_dir, 9229, &[]).unwrap(),
        CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229"
                .to_string(),
            process_id: None,
        }
    );
}

#[test]
fn launcher_packaged_activation_appends_extra_codex_arguments() {
    let app_dir = PathBuf::from(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.506.2212.0_x64__2p2nqsd0c76g0\app",
    );
    let extra_args = vec!["--force_high_performance_gpu".to_string()];

    assert_eq!(
        build_packaged_activation(&app_dir, 9229, &extra_args).unwrap(),
        CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments:
                "--remote-debugging-port=9229 --remote-allow-origins=http://127.0.0.1:9229 --force_high_performance_gpu"
                    .to_string(),
            process_id: None,
        }
    );
}

#[test]
fn launcher_packaged_activation_can_preserve_process_id() {
    let launch = CodexLaunch::PackagedActivation {
        app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
        arguments: "--remote-debugging-port=9229".to_string(),
        process_id: Some(4242),
    };

    assert_eq!(launch.process_id(), Some(4242));
}

#[test]
fn app_paths_parse_appx_install_location_from_powershell_output() {
    let output =
        "\r\nC:\\Program Files\\WindowsApps\\OpenAI.Codex_26.611.7849.0_x64__2p2nqsd0c76g0\r\n";

    assert_eq!(
        latest_appx_install_location_from_output(output).as_deref(),
        Some(r"C:\Program Files\WindowsApps\OpenAI.Codex_26.611.7849.0_x64__2p2nqsd0c76g0")
    );
}

#[test]
fn launcher_packaged_activation_does_not_directly_fallback_to_windowsapps_exe() {
    let source = include_str!("../src/launcher.rs");

    assert!(!source.contains("launcher.packaged_activation_cdp_unready_direct_fallback"));
    assert!(!source.contains("terminate_windows_process_id(process_id).await"));
}

#[cfg(windows)]
#[test]
fn launcher_windows_packaged_process_management_uses_native_api() {
    assert_eq!(
        windows_process_control_strategy(),
        WindowsProcessControlStrategy::NativeWindowsApi
    );
}

#[test]
fn launcher_macos_open_command_waits_for_app_exit() {
    let command = build_macos_open_command(Path::new("/Applications/Codex.app"), 9229, &[]);

    assert_eq!(command[0], "open");
    assert!(command.contains(&"-W".to_string()));
    assert!(command.contains(&"-a".to_string()));
    assert!(command.contains(&"--args".to_string()));
    assert!(command.contains(&"--remote-debugging-port=9229".to_string()));
}

#[test]
fn launcher_macos_open_command_appends_extra_codex_arguments_after_args() {
    let extra_args = vec!["--force_high_performance_gpu".to_string()];
    let command = build_macos_open_command(Path::new("/Applications/Codex.app"), 9229, &extra_args);
    let args_index = command
        .iter()
        .position(|part| part == "--args")
        .expect("macOS command should contain --args");

    assert_eq!(
        &command[args_index + 1..],
        &[
            "--remote-debugging-port=9229".to_string(),
            "--remote-allow-origins=http://127.0.0.1:9229".to_string(),
            "--force_high_performance_gpu".to_string(),
        ]
    );
}

#[test]
fn ports_windows_falls_back_to_ephemeral_when_requested_is_busy() {
    let selected = select_platform_loopback_port_with(9229, true, |_| false, || 43001);

    assert_eq!(selected, 43001);
}

#[test]
fn ports_windows_packaged_debug_falls_back_to_ephemeral_when_requested_is_busy() {
    let selected = select_packaged_codex_debug_port_with(9229, true, |_| false, || 43001);

    assert_eq!(selected, 43001);
}

#[test]
fn ports_non_windows_keeps_requested_even_when_busy() {
    let selected = select_platform_loopback_port_with(9229, false, |_| false, || 43001);

    assert_eq!(selected, 9229);
}

#[tokio::test]
async fn default_helper_serves_backend_status_over_http() {
    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    hooks.start_helper(port).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .post(format!("http://127.0.0.1:{port}/backend/status"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let payload: serde_json::Value = response.json().await.unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["transport"], "http-helper");

    let repair_response = client
        .post(format!("http://127.0.0.1:{port}/backend/repair"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(repair_response.status().is_success());
    let repair_payload: serde_json::Value = repair_response.json().await.unwrap();
    assert_eq!(repair_payload["status"], "ok");
    assert_eq!(repair_payload["transport"], "http-helper");

    hooks.shutdown_helper(port).await;
}

#[tokio::test]
async fn default_helper_accepts_diagnostic_log_events_over_http() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("codex-elves.log");
    let diagnostic_log = DiagnosticLogCapture::new(log_path);
    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    hooks.start_helper(port).await.unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/diagnostics/log"))
        .json(&serde_json::json!({
            "event": "backend_check_failed",
            "message": "fetch failed",
            "helperBase": format!("http://127.0.0.1:{port}")
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let payload: serde_json::Value = response.json().await.unwrap();
    assert_eq!(payload["status"], "ok");
    hooks.shutdown_helper(port).await;

    let contents = diagnostic_log.read();
    assert!(contents.contains("renderer.backend_check_failed"));
    assert!(contents.contains("fetch failed"));
}

#[test]
fn helper_exposes_user_script_bundle_for_bootstrap_fallback() {
    let source = include_str!("../src/launcher.rs");

    assert!(source.contains("\"/inject/user-scripts.js\""));
    assert!(source.contains("default_user_script_manager().build_enabled_bundle()"));
    assert!(source.contains("helper.user_scripts_ok"));
}

#[tokio::test]
async fn default_helper_streams_translated_tool_call_to_completion() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_response(
        "text/event-stream",
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_tool_search","type":"message","role":"assistant","model":"claude-sonnet-4","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_search","name":"tool_search","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"local_shell\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}

"#
        .to_string(),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model": "claude-sonnet-4",
            "input": "find shell tools",
            "stream": true,
            "tools": [
                { "type": "tool_search" },
                { "type": "local_shell" }
            ]
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/messages");
    let first_upstream_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(first_upstream_body["stream"], true);
    assert!(
        first_upstream_body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "tool_search")
    );
    assert!(content_type.contains("text/event-stream"));
    assert!(body.contains("\"type\":\"tool_search_call\""));
    assert!(body.contains("\"call_id\":\"toolu_search\""));
    assert!(body.contains(r#""arguments":{"query":"local_shell"}"#));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn helper_does_not_defer_high_reasoning_stream_for_responses_models() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_delayed_response(
        "text/event-stream",
r#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_slow","object":"response","status":"completed","model":"gpt-responses","output":[],"usage":{"input_tokens":10,"output_tokens":1}}}

data: [DONE]

"#
        .to_string(),
        std::time::Duration::from_secs(2),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let started = std::time::Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": "think deeply",
            "stream": true,
            "reasoning": { "effort": "high" }
        }))
        .send()
        .await
        .unwrap();
    let header_elapsed = started.elapsed();

    assert!(response.status().is_success());
    assert!(
        header_elapsed >= std::time::Duration::from_millis(1500),
        "responses streams should wait for upstream headers instead of entering deferred path: {header_elapsed:?}"
    );
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/responses");
    let first_upstream_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(first_upstream_body["stream"], true);
    assert_eq!(
        first_upstream_body["reasoning"],
        serde_json::json!({ "effort": "high" })
    );
    assert!(content_type.contains("text/event-stream"));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn helper_prompt_only_compaction_does_not_continue_after_prompt_replacement() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let _guard = LauncherSettingsPathGuard::set(settings_path.clone());
    let diagnostic_log = DiagnosticLogCapture::new(temp.path().join("compaction-diagnostic.log"));
    let model = "gpt-qa-compaction-prompt-only";
    let upstream = spawn_launcher_upstream_with_response(
        "text/event-stream",
        format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({
                "type":"response.completed",
                "response":{
                    "id":"resp_compaction_summary",
                    "status":"completed",
                    "model":model,
                    "output":[{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":"<summary>SUMMARY</summary>"}]}],
                    "usage":{"output_tokens_details":{"reasoning_tokens":516}}
                }
            })
        ),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    settings["layeredCompactionEnabled"] = serde_json::json!(true);
    settings["layeredCompactionRetainRecentRoundEnabled"] = serde_json::json!(false);
    settings["layeredCompactionPromptOverride"] = serde_json::json!("CUSTOM SUMMARY PROMPT");
    settings["gptReasoningContinuation"] = serde_json::json!(true);
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model":model,
            "stream":true,
            "reasoning":{"effort":"high"},
            "input":[
                {"type":"message","role":"user","content":"recent request"},
                {"type":"message","role":"user",
                 "content":"You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    let forwarded: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(forwarded["input"][0]["content"], "recent request");
    assert_eq!(
        forwarded["input"][1]["content"][0]["text"],
        codex_elves_core::layered_compaction::compaction_instruction("CUSTOM SUMMARY PROMPT")
    );
    assert!(body.contains("SUMMARY"));
    assert!(!body.contains("codex-elves-compaction-v3:"));
    let continued = diagnostic_log.read().lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line).is_ok_and(|entry| {
            entry["event"] == "continue_thinking.round_start" && entry["detail"]["model"] == model
        })
    });
    assert!(!continued, "压缩摘要不得触发自动续思考请求");
}

#[tokio::test]
async fn helper_logs_independent_http_compaction_model_roles() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let proxy_log_path = temp.path().join("proxy-requests.jsonl");
    let _settings_guard = LauncherSettingsPathGuard::set(settings_path.clone());
    let _proxy_log_guard = LauncherProxyLogPathGuard::set(proxy_log_path);
    let original_model = "claude-opus-5-5";
    let compaction_model = "deepseek-v4.1-flash";
    let upstream = spawn_launcher_upstream_with_response(
        "text/event-stream",
        format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({
                "type":"response.completed",
                "response":{
                    "id":"resp_independent_compaction",
                    "object":"response",
                    "status":"completed",
                    "model":compaction_model,
                    "output":[{
                        "type":"message",
                        "role":"assistant",
                        "content":[{"type":"output_text","text":"<summary>SUMMARY</summary>"}]
                    }],
                    "usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}
                }
            })
        ),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    settings["relayProfiles"][0]["modelMappings"] = serde_json::json!([
        {
            "requestModel": original_model,
            "protocol": "responses",
            "contextWindow": "200000"
        },
        {
            "requestModel": compaction_model,
            "protocol": "responses",
            "contextWindow": "200000"
        }
    ]);
    settings["layeredCompactionEnabled"] = serde_json::json!(true);
    settings["layeredCompactionModelOverrideEnabled"] = serde_json::json!(true);
    settings["layeredCompactionModelUsage"] = serde_json::json!("default");
    settings["layeredCompactionModels"] = serde_json::json!({ "claude": compaction_model });
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model": original_model,
            "stream": true,
            "input": [
                {"type":"message","role":"user","content":"recent request"},
                {
                    "type":"message",
                    "role":"user",
                    "content":"You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let downstream = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    let forwarded: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(forwarded["model"], compaction_model);
    assert_eq!(forwarded["stream"], true);
    assert!(downstream.contains(&format!(r#""model":"{original_model}""#)));

    let summaries = codex_elves_core::proxy_log::read_summaries(10).unwrap();
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.model.as_deref(), Some(original_model));
    assert_eq!(
        summary.upstream_request_model.as_deref(),
        Some(compaction_model)
    );
    assert_eq!(
        summary.upstream_response_model.as_deref(),
        Some(compaction_model)
    );
    assert_eq!(
        summary.independent_compaction_model.as_deref(),
        Some(compaction_model)
    );
    assert_eq!(
        summary.independent_compaction_usage,
        Some(codex_elves_core::settings::LayeredCompactionModelUsage::Default)
    );

    let detail = codex_elves_core::proxy_log::find_record(&summary.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&detail.request_body).unwrap()["model"],
        compaction_model
    );
    assert_eq!(
        detail
            .response_body
            .contains(&format!(r#""model":"{original_model}""#)),
        true
    );
}

#[tokio::test]
async fn helper_preserves_codex_semantic_headers_for_native_responses_upstream() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_response(
        "application/json",
        r#"{"id":"resp_headers","object":"response","status":"completed","model":"gpt-responses","output":[]}"#
            .to_string(),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let mut semantic_headers = reqwest::header::HeaderMap::new();
    semantic_headers.append(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("remote_compaction_v2"),
    );
    semantic_headers.append(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("future_native_feature"),
    );
    semantic_headers.insert(
        "x-codex-turn-state",
        reqwest::header::HeaderValue::from_static("turn-state-from-client"),
    );
    semantic_headers.insert(
        "openai-beta",
        reqwest::header::HeaderValue::from_static("responses_websockets=2026-02-06"),
    );
    semantic_headers.insert(
        "authorization",
        reqwest::header::HeaderValue::from_static("Bearer local-client-token"),
    );
    semantic_headers.insert(
        "x-forwarded-for",
        reqwest::header::HeaderValue::from_static("203.0.113.42"),
    );

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .headers(semantic_headers)
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": "preserve the native responses context"
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    assert_eq!(
        request.header_values("x-codex-beta-features"),
        Some(
            &[
                "remote_compaction_v2".to_string(),
                "future_native_feature".to_string()
            ][..]
        )
    );
    assert_eq!(
        request.header_values("x-codex-turn-state"),
        Some(&["turn-state-from-client".to_string()][..])
    );
    assert_eq!(
        request.header_values("openai-beta"),
        Some(&["responses_websockets=2026-02-06".to_string()][..])
    );
    assert_eq!(
        request.header_value("authorization"),
        Some("Bearer sk-test"),
        "the relay credential must replace the local client credential"
    );
    assert_eq!(request.header_value("x-forwarded-for"), None);
}

#[tokio::test]
async fn native_remote_compaction_preserves_upstream_non_sse_status_and_content_type() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_status_response(
        "429 Too Many Requests",
        "application/json; charset=utf-8",
        r#"{"error":{"message":"upstream is busy"}}"#.to_string(),
        std::time::Duration::from_secs(2),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let started = std::time::Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": [
                { "role": "user", "content": "compact this context" },
                { "type": "compaction_trigger" }
            ],
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    let header_elapsed = started.elapsed();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let status = response.status();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert!(
        header_elapsed >= std::time::Duration::from_millis(1500),
        "native remote compaction must wait for upstream response semantics: {header_elapsed:?}"
    );
    assert_eq!(status, reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert!(content_type.contains("application/json"));
    assert!(body.contains("upstream is busy"));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/responses");
}

#[tokio::test]
async fn helper_retries_same_responses_request_when_model_is_overloaded() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let overloaded = r#"event: error
data: {"type":"error","error":{"type":"service_unavailable_error","code":"server_is_overloaded","message":"Our servers are currently overloaded. Please try again later.","param":null},"sequence_number":2}

event: response.failed
data: {"type":"response.failed","response":{"id":"resp_overloaded","object":"response","status":"failed","error":{"code":"server_is_overloaded","message":"Our servers are currently overloaded. Please try again later."}}}

"#;
    let recovered = r#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_recovered","object":"response","status":"completed","model":"gpt-responses","output":[],"usage":{"input_tokens":10,"output_tokens":1}}}

data: [DONE]

"#;
    let upstream = spawn_launcher_upstream_with_response_specs(vec![
        (
            "text/event-stream".to_string(),
            overloaded.to_string(),
            std::time::Duration::ZERO,
        ),
        (
            "text/event-stream".to_string(),
            recovered.to_string(),
            std::time::Duration::ZERO,
        ),
    ]);
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .header("x-codex-beta-features", "remote_compaction_v2")
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": "retry the same model",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/v1/responses");
    assert_eq!(requests[1].path, "/v1/responses");
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(
        requests[0].header_values("x-codex-beta-features"),
        Some(&["remote_compaction_v2".to_string()][..])
    );
    assert_eq!(
        requests[1].header_values("x-codex-beta-features"),
        Some(&["remote_compaction_v2".to_string()][..])
    );
    assert!(body.contains("resp_recovered"));
    assert!(!body.contains("server_is_overloaded"));
}

#[tokio::test]
async fn helper_retries_after_http_503_capacity_body() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let overloaded = r#"event: response.failed
data: {"type":"response.failed","response":{"id":"resp_overloaded","object":"response","status":"failed","error":{"code":"server_is_overloaded","message":"Our servers are currently overloaded. Please try again later."}}}

"#;
    let capacity_503 = r#"{"error":{"type":"server_error","code":"server_is_overloaded","message":"Selected model is at capacity. Please try a different model."}}"#;
    let recovered = r#"event: response.completed
data: {"type":"response.completed","response":{"id":"resp_recovered_after_503","object":"response","status":"completed","model":"gpt-responses","output":[],"usage":{"input_tokens":10,"output_tokens":1}}}

data: [DONE]

"#;
    let upstream = spawn_launcher_upstream_with_status_response_specs(vec![
        (
            "200 OK".to_string(),
            "text/event-stream".to_string(),
            overloaded.to_string(),
            std::time::Duration::ZERO,
        ),
        (
            "503 Service Unavailable".to_string(),
            "application/json".to_string(),
            capacity_503.to_string(),
            std::time::Duration::ZERO,
        ),
        (
            "200 OK".to_string(),
            "text/event-stream".to_string(),
            recovered.to_string(),
            std::time::Duration::ZERO,
        ),
    ]);
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .header("x-codex-beta-features", "remote_compaction_v2")
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": "retry after an HTTP 503 capacity response",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].body, requests[1].body);
    assert_eq!(requests[1].body, requests[2].body);
    assert!(body.contains("resp_recovered_after_503"));
    assert!(!body.contains("Selected model is at capacity"));
}

#[tokio::test]
async fn helper_defers_high_reasoning_stream_header_wait_for_chat_completions_models() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_delayed_response(
        "text/event-stream",
        r#"data: {"id":"chatcmpl_slow","object":"chat.completion.chunk","created":0,"model":"gpt-chat","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}

data: {"id":"chatcmpl_slow","object":"chat.completion.chunk","created":0,"model":"gpt-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":1,"total_tokens":11}}

data: [DONE]

"#
        .to_string(),
        std::time::Duration::from_secs(2),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let started = std::time::Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .header("x-codex-beta-features", "remote_compaction_v2")
        .json(&serde_json::json!({
            "model": "gpt-chat",
            "input": "think deeply",
            "stream": true,
            "reasoning": { "effort": "high" }
        }))
        .send()
        .await
        .unwrap();
    let header_elapsed = started.elapsed();

    assert!(response.status().is_success());
    assert!(
        header_elapsed < std::time::Duration::from_secs(1),
        "chat completions streams should receive local deferred SSE headers quickly: {header_elapsed:?}"
    );
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert_eq!(requests[0].header_value("x-codex-beta-features"), None);
    let first_upstream_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(first_upstream_body["stream"], true);
    assert!(content_type.contains("text/event-stream"));
    assert!(body.contains("event: response.output_text.delta"));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn chat_remote_compaction_stream_establishes_local_sse_before_upstream_headers() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_delayed_response(
        "text/event-stream",
        r#"data: {"id":"chatcmpl_compact","object":"chat.completion.chunk","created":0,"model":"gpt-chat","choices":[{"index":0,"delta":{"content":"<summary>summary</summary>"},"finish_reason":null}]}

data: {"id":"chatcmpl_compact","object":"chat.completion.chunk","created":0,"model":"gpt-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#
        .to_string(),
        std::time::Duration::from_secs(2),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);
    let settings_path = temp.path().join("settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    settings["layeredCompactionEnabled"] = serde_json::json!(true);
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let started = std::time::Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .json(&serde_json::json!({
            "model": "gpt-chat",
            "input": [
                { "role": "user", "content": "compact this context" },
                { "type": "compaction_trigger" }
            ],
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    let header_elapsed = started.elapsed();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert!(
        header_elapsed < std::time::Duration::from_secs(1),
        "chat remote compaction should establish local SSE promptly: {header_elapsed:?}"
    );
    assert!(content_type.contains("text/event-stream"));
    assert!(body.contains("event: response.completed"));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/chat/completions");
}

#[tokio::test]
async fn helper_defers_stream_header_wait_for_anthropic_models() {
    let _lock = launcher_settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream = spawn_launcher_upstream_with_delayed_response(
        "text/event-stream",
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_slow","type":"message","role":"assistant","model":"claude-sonnet-4","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}

event: message_stop
data: {"type":"message_stop"}

"#
        .to_string(),
        std::time::Duration::from_secs(2),
    );
    write_launcher_mixed_relay_settings(temp.path(), &upstream.base_url);

    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();

    let started = std::time::Instant::now();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses"))
        .header("x-codex-beta-features", "remote_compaction_v2")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4",
            "input": "think deeply",
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    let header_elapsed = started.elapsed();

    assert!(response.status().is_success());
    assert!(
        header_elapsed < std::time::Duration::from_secs(1),
        "anthropic streams should receive local deferred SSE headers quickly: {header_elapsed:?}"
    );
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.finish_all();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(requests[0].header_value("x-codex-beta-features"), None);
    let first_upstream_body: serde_json::Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(first_upstream_body["stream"], true);
    assert!(content_type.contains("text/event-stream"));
    assert!(body.contains("event: response.output_text.delta"));
    assert!(body.contains("event: response.completed"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn launch_lifecycle_runs_sync_before_launch_writes_success_and_shutdowns_on_exit() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_settings(BackendSettings {
            provider_sync_enabled: true,
            computer_use_guard_enabled: false,
            computer_use_api_key_browser_compat_enabled: false,
            ..BackendSettings::default()
        })
        .with_launch_result(CodexLaunch::Process {
            command: vec!["codex".to_string()],
            wait_strategy: codex_elves_core::launcher::ProcessWaitStrategy::TrackedChild,
            macos_cleanup_policy: None,
        });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir.clone()),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "provider-sync",
            "start-helper:45221",
            "launch:9229",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "status:running",
            "wait-codex",
            "shutdown-helper:45221",
        ]
    );
    let latest = handle.status_store.load_latest().unwrap().unwrap();
    assert_eq!(
        latest.codex_app.as_deref(),
        Some(app_dir.to_string_lossy().as_ref())
    );
    assert!(!latest.lan_proxy_listening);
}

#[tokio::test]
async fn launch_lifecycle_passes_configured_extra_args_to_codex_launch() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        codex_extra_args: vec!["--force_high_performance_gpu".to_string()],
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert!(
        events
            .lock()
            .unwrap()
            .contains(&"launch:9229:--force_high_performance_gpu".to_string())
    );
}

#[tokio::test]
async fn launch_lifecycle_keeps_js_injection_in_relay_mode() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        launch_mode: codex_elves_core::settings::LaunchMode::Relay,
        computer_use_guard_enabled: false,
        computer_use_api_key_browser_compat_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "start-helper:45221",
            "launch:9229",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "status:running",
            "wait-codex",
            "shutdown-helper:45221",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_skips_helper_and_injection_when_enhancements_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        enhancements_enabled: false,
        computer_use_guard_enabled: false,
        computer_use_api_key_browser_compat_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "launch:9229",
            "status:running",
            "wait-codex",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_runs_computer_use_guard_when_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        computer_use_guard_enabled: true,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "computer-use-guard",
            "start-helper:45221",
            "launch:9229",
            "computer-use-guard-watchdog",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "status:running",
            "wait-codex",
            "shutdown-helper:45221",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_skips_computer_use_guard_when_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone());

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(!events.contains(&"computer-use-guard-watchdog".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_runs_browser_compat_without_full_computer_use_guard() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        computer_use_guard_enabled: false,
        computer_use_api_key_browser_compat_enabled: true,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(events.contains(&"browser-request-header-compat".to_string()));
    assert!(events.contains(&"computer-use-guard-watchdog".to_string()));
    assert!(!events.contains(&"computer-use-guard".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_does_not_apply_relay_profile_while_launching_codex() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_profiles_enabled: true,
        relay_profiles: vec![RelayProfile {
            id: "relay-a".to_string(),
            base_url: "https://relay.example/v1".to_string(),
            local_proxy_enabled: Some(true),
            ..RelayProfile::default()
        }],
        active_relay_id: "relay-a".to_string(),
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(events.contains(&"ensure-stream-timeout".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_skips_active_relay_profile_when_supplier_config_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_profiles_enabled: false,
        computer_use_guard_enabled: false,
        computer_use_api_key_browser_compat_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(!events.contains(&"ensure-stream-timeout".to_string()));
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_tolerates_duplicate_context_parent_tables_without_applying_relay() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_settings(BackendSettings {
        relay_common_config_contents: "[mcp_servers]\n".to_string(),
        relay_context_config_contents: "[mcp_servers]\n\n[mcp_servers.ida]\ncommand = \"python\"\n"
            .to_string(),
        relay_profiles: vec![RelayProfile {
            id: "relay-a".to_string(),
            name: "Relay A".to_string(),
            relay_mode: codex_elves_core::settings::RelayMode::PureApi,
            config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-test"
"#
            .to_string(),
            auth_contents: r#"{"OPENAI_API_KEY":"sk-test"}"#.to_string(),
            ..RelayProfile::default()
        }],
        active_relay_id: "relay-a".to_string(),
        computer_use_guard_enabled: false,
        computer_use_api_key_browser_compat_enabled: false,
        ..BackendSettings::default()
    });

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();
    handle.wait_for_codex_exit().await.unwrap();

    let events = events.lock().unwrap().clone();
    assert!(!events.contains(&"apply-relay".to_string()));
    assert!(!events.contains(&"computer-use-guard".to_string()));
    assert!(events.contains(&"launch:9229".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_enters_degraded_mode_and_retries_when_injection_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_inject_error("inject failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store: status_store.clone(),
        },
        &hooks,
    )
    .await
    .unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "start-helper:45221",
            "launch:9229",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "status:running_degraded",
        ]
    );
    let status = status_store.load_latest().unwrap().unwrap();
    assert_eq!(status.status, "running_degraded");
    assert!(status.message.contains("Codex launched"));

    handle.wait_for_codex_exit().await.unwrap();
    let events = events.lock().unwrap().clone();
    assert!(events.contains(&"wait-codex".to_string()));
    assert!(events.contains(&"shutdown-helper:45221".to_string()));
    assert!(!events.contains(&"terminate-codex".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_cleans_helper_when_launch_fails_after_helper_started() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone()).with_launch_error("launch failed");

    let error = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store: status_store.clone(),
        },
        &hooks,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("launch failed"));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "start-helper:45221",
            "launch:9229",
            "shutdown-helper:45221",
            "status:failed",
        ]
    );
}

#[tokio::test]
async fn launch_starts_helper_when_chat_protocol_proxy_is_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let proxy_log_path = temp.path().join("proxy-requests.jsonl");
    let _proxy_log_guard = LauncherProxyLogPathGuard::set(proxy_log_path.clone());
    for index in 0..12 {
        codex_elves_core::proxy_log::append_record(&launcher_proxy_request_record(
            &format!("stale-request-{index}"),
            index,
        ))
        .unwrap();
    }
    assert_eq!(
        codex_elves_core::proxy_log::read_summaries(20)
            .unwrap()
            .len(),
        12
    );
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let settings = BackendSettings {
        enhancements_enabled: false,
        lan_proxy_enabled: true,
        relay_profiles: vec![RelayProfile {
            id: "relay-chat".to_string(),
            name: "Chat".to_string(),
            model: String::new(),
            base_url: "https://chat-only.example.test/v1".to_string(),
            upstream_base_url: "https://chat-only.example.test/v1".to_string(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::ChatCompletions,
            local_proxy_enabled: Some(true),
            relay_mode: codex_elves_core::settings::RelayMode::MixedApi,
            official_mix_api_key: false,
            test_model: String::new(),
            config_contents: String::new(),
            auth_contents: String::new(),
            use_common_config: true,
            context_selection: codex_elves_core::settings::RelayContextSelection::default(),
            context_selection_initialized: false,
            context_window: String::new(),
            auto_compact_limit: String::new(),
            model_insert_mode: codex_elves_core::settings::RelayModelInsertMode::default(),
            model_mappings: Vec::new(),
            model_list: String::new(),
            responses_model_list: String::new(),
            chat_completions_model_list: String::new(),
            anthropic_model_list: String::new(),
            responses_websocket: Default::default(),
            responses_websocket_enabled: None,
            user_agent: String::new(),
            system_prompt_override: String::new(),
        }],
        active_relay_id: "relay-chat".to_string(),
        ..BackendSettings::default()
    };
    let hooks = FakeHooks::new(events.clone()).with_settings(settings);

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 58000,
            status_store: status_store.clone(),
        },
        &hooks,
    )
    .await
    .unwrap();

    let before_stop = events.lock().unwrap().clone();
    assert!(before_stop.contains(&"select-helper:58000".to_string()));
    assert!(before_stop.contains(&"start-helper:45221".to_string()));
    assert!(!before_stop.contains(&"inject:9229:45221".to_string()));
    assert!(
        status_store
            .load_latest()
            .unwrap()
            .unwrap()
            .lan_proxy_listening
    );
    let proxy_log = std::fs::read_to_string(&proxy_log_path).unwrap();
    assert_eq!(
        proxy_log.lines().next(),
        Some(r#"{"format":"codex-elves-proxy-index","version":1}"#)
    );
    assert!(
        codex_elves_core::proxy_log::read_summaries(20)
            .unwrap()
            .is_empty()
    );
    assert!(
        codex_elves_core::proxy_log::find_record("stale-request-11")
            .unwrap()
            .is_none()
    );

    handle.wait_for_codex_exit().await.unwrap();

    let after_stop = events.lock().unwrap().clone();
    assert!(after_stop.contains(&"wait-codex".to_string()));
    assert!(after_stop.contains(&"shutdown-helper:45221".to_string()));
}

#[tokio::test]
async fn launch_lifecycle_cleans_helper_and_codex_when_status_save_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(temp.path().join("status-parent-file"), "not a directory").unwrap();
    let status_store = StatusStore::new(
        temp.path()
            .join("status-parent-file")
            .join("latest-status.json"),
    );
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks =
        FakeHooks::new(events.clone()).with_launch_result(CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229".to_string(),
            process_id: Some(4242),
        });

    let error = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("failed to create directory"));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "start-helper:45221",
            "launch:9229",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "shutdown-helper:45221",
            "terminate-packaged:4242",
            "status:failed",
        ]
    );
}

#[tokio::test]
async fn launch_lifecycle_keeps_packaged_process_id_running_and_retries_when_injection_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let status_store = StatusStore::new(temp.path().join("latest-status.json"));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_launch_result(CodexLaunch::PackagedActivation {
            app_user_model_id: "OpenAI.Codex_2p2nqsd0c76g0!App".to_string(),
            arguments: "--remote-debugging-port=9229".to_string(),
            process_id: Some(4242),
        })
        .with_inject_error("inject failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store,
        },
        &hooks,
    )
    .await
    .unwrap();

    assert!(
        !events
            .lock()
            .unwrap()
            .contains(&"terminate-packaged:4242".to_string())
    );
    handle.wait_for_codex_exit().await.unwrap();
}

#[tokio::test]
async fn default_provider_sync_enabled_fails_instead_of_silently_skipping() {
    let hooks = FakeHooks::new(Arc::new(Mutex::new(Vec::new()))).with_provider_sync_unsupported();

    let error = hooks
        .run_provider_sync()
        .await
        .expect_err("default-style provider sync should be explicit");

    assert!(
        error
            .to_string()
            .contains("provider sync requires launcher hooks")
    );
}

#[tokio::test]
async fn launch_continues_when_plugin_marketplace_config_fails() {
    let temp = tempfile::tempdir().unwrap();
    let app_dir = temp.path().join("Codex.app");
    std::fs::create_dir_all(&app_dir).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let hooks = FakeHooks::new(events.clone())
        .with_plugin_marketplace_error("config.toml TOML parse failed");

    let handle = launch_and_inject_with_hooks(
        LaunchOptions {
            app_dir: Some(app_dir),
            debug_port: 9229,
            helper_port: 45221,
            status_store: StatusStore::new(temp.path().join("status.json")),
        },
        &hooks,
    )
    .await
    .unwrap();

    assert_eq!(handle.debug_port, 9229);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "select-debug:9229",
            "select-helper:45221",
            "load-settings",
            "plugin-marketplace",
            "start-helper:45221",
            "launch:9229",
            "inject:9229:45221",
            "bridge-watchdog:9229:45221",
            "status:running"
        ]
    );
}

#[test]
fn launcher_macos_cleanup_command_targets_specific_app_bundle() {
    let command = build_macos_cleanup_command(
        Path::new("/Applications/OpenAI Codex.app"),
        MacosCleanupPolicy::QuitIfNotPreviouslyRunning,
    )
    .expect("cleanup command should be allowed");

    assert_eq!(command[0], "osascript");
    assert!(command.iter().any(|part| part.contains("OpenAI Codex")));
    assert!(!command.iter().any(|part| part == "Codex"));
}

#[test]
fn launcher_macos_cleanup_is_skipped_when_app_was_already_running() {
    let command = build_macos_cleanup_command(
        Path::new("/Applications/OpenAI Codex.app"),
        MacosCleanupPolicy::SkipQuitBecauseAlreadyRunning,
    );

    assert_eq!(command, None);
}

#[tokio::test]
async fn default_launch_hooks_provider_sync_enabled_returns_explicit_error() {
    let error = DefaultLaunchHooks::default()
        .run_provider_sync()
        .await
        .expect_err("default provider sync should not silently skip");

    assert!(
        error
            .to_string()
            .contains("provider sync requires launcher hooks")
    );
}

#[derive(Clone)]
struct FakeHooks {
    events: Arc<Mutex<Vec<String>>>,
    settings: BackendSettings,
    launch_result: CodexLaunch,
    launch_error: Option<String>,
    inject_error: Option<String>,
    provider_sync_unsupported: bool,
    plugin_marketplace_error: Option<String>,
}

impl FakeHooks {
    fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            events,
            settings: BackendSettings {
                computer_use_guard_enabled: false,
                computer_use_api_key_browser_compat_enabled: false,
                ..BackendSettings::default()
            },
            launch_result: CodexLaunch::Process {
                command: vec!["codex".to_string()],
                wait_strategy: codex_elves_core::launcher::ProcessWaitStrategy::TrackedChild,
                macos_cleanup_policy: None,
            },
            launch_error: None,
            inject_error: None,
            provider_sync_unsupported: false,
            plugin_marketplace_error: None,
        }
    }

    fn with_settings(mut self, settings: BackendSettings) -> Self {
        self.settings = settings;
        self
    }

    fn with_launch_result(mut self, launch_result: CodexLaunch) -> Self {
        self.launch_result = launch_result;
        self
    }

    fn with_inject_error(mut self, message: &str) -> Self {
        self.inject_error = Some(message.to_string());
        self
    }

    fn with_launch_error(mut self, message: &str) -> Self {
        self.launch_error = Some(message.to_string());
        self
    }

    fn with_provider_sync_unsupported(mut self) -> Self {
        self.provider_sync_unsupported = true;
        self
    }

    fn with_plugin_marketplace_error(mut self, message: &str) -> Self {
        self.plugin_marketplace_error = Some(message.to_string());
        self
    }

    fn event(&self, event: impl Into<String>) {
        self.events.lock().unwrap().push(event.into());
    }
}

#[async_trait::async_trait(?Send)]
impl LaunchHooks for FakeHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&Path>,
        _settings: &BackendSettings,
    ) -> anyhow::Result<PathBuf> {
        app_dir
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("missing app dir"))
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        self.event(format!("select-debug:{requested}"));
        requested
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        self.event(format!("select-helper:{requested}"));
        requested
    }

    async fn load_settings(&self) -> anyhow::Result<BackendSettings> {
        self.event("load-settings");
        Ok(self.settings.clone())
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        self.event("provider-sync");
        if self.provider_sync_unsupported {
            anyhow::bail!("provider sync requires launcher hooks");
        }
        Ok(())
    }

    async fn ensure_active_relay_stream_idle_timeout(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        self.event("ensure-stream-timeout");
        Ok(())
    }

    async fn apply_active_relay_profile(&self, _settings: &BackendSettings) -> anyhow::Result<()> {
        self.event("apply-relay");
        Ok(())
    }

    async fn ensure_computer_use_config(&self, settings: &BackendSettings) -> anyhow::Result<()> {
        if settings.computer_use_guard_enabled {
            self.event("computer-use-guard");
        } else if settings.computer_use_api_key_browser_compat_enabled {
            self.event("browser-request-header-compat");
        }
        Ok(())
    }

    async fn ensure_plugin_marketplace_config(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        if let Some(message) = &self.plugin_marketplace_error {
            self.event("plugin-marketplace");
            anyhow::bail!(message.clone());
        }
        Ok(())
    }

    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("start-helper:{helper_port}"));
        Ok(())
    }

    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        extra_args: &[String],
    ) -> anyhow::Result<CodexLaunch> {
        assert!(app_dir.ends_with("Codex.app"));
        if extra_args.is_empty() {
            self.event(format!("launch:{debug_port}"));
        } else {
            self.event(format!("launch:{debug_port}:{}", extra_args.join(",")));
        }
        if let Some(message) = &self.launch_error {
            anyhow::bail!(message.clone());
        }
        Ok(self.launch_result.clone())
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("inject:{debug_port}:{helper_port}"));
        if let Some(message) = &self.inject_error {
            anyhow::bail!(message.clone());
        }
        Ok(())
    }

    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.event(format!("bridge-watchdog:{debug_port}:{helper_port}"));
        Ok(())
    }

    async fn start_computer_use_guard_watchdog(
        &self,
        _settings: &BackendSettings,
    ) -> anyhow::Result<()> {
        self.event("computer-use-guard-watchdog");
        Ok(())
    }

    async fn write_status(&self, status: &str) {
        self.event(format!("status:{status}"));
    }

    async fn wait_for_codex_exit(&self, _launch: &CodexLaunch) -> anyhow::Result<()> {
        self.event("wait-codex");
        Ok(())
    }

    async fn shutdown_helper(&self, helper_port: u16) {
        self.event(format!("shutdown-helper:{helper_port}"));
    }

    async fn terminate_codex(&self, launch: &CodexLaunch) {
        if let Some(process_id) = launch.process_id() {
            self.event(format!("terminate-packaged:{process_id}"));
        } else {
            self.event("terminate-codex");
        }
    }
}

struct LauncherSettingsPathGuard {
    previous: Option<PathBuf>,
}

impl LauncherSettingsPathGuard {
    fn set(path: PathBuf) -> Self {
        let previous = codex_elves_core::paths::set_settings_path_for_tests(Some(path));
        Self { previous }
    }
}

impl Drop for LauncherSettingsPathGuard {
    fn drop(&mut self) {
        codex_elves_core::paths::set_settings_path_for_tests(self.previous.take());
    }
}

struct LauncherProxyLogPathGuard {
    previous: Option<PathBuf>,
}

impl LauncherProxyLogPathGuard {
    fn set(path: PathBuf) -> Self {
        let previous = codex_elves_core::paths::set_proxy_log_path_for_tests(Some(path));
        Self { previous }
    }
}

impl Drop for LauncherProxyLogPathGuard {
    fn drop(&mut self) {
        codex_elves_core::paths::set_proxy_log_path_for_tests(self.previous.take());
    }
}

fn launcher_proxy_request_record(id: &str, timestamp_ms: u64) -> ProxyRequestRecord {
    ProxyRequestRecord {
        id: id.to_string(),
        state: ProxyRequestState::Completed,
        transport: ProxyRequestTransport::Http,
        timestamp_ms,
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        remote_addr: Some("127.0.0.1:1".to_string()),
        model: Some("gpt-5.4".to_string()),
        upstream_request_model: None,
        upstream_response_model: None,
        independent_compaction_model: None,
        independent_compaction_usage: None,
        reasoning_tokens: None,
        reasoning_effort: None,
        reasoning_source: None,
        continue_thinking_triggered: false,
        continue_thinking_rounds: 0,
        continue_thinking_request_body: None,
        continue_thinking_before_response_body: None,
        continue_thinking_after_response_body: None,
        remote_compaction_triggered: false,
        layered_compaction_triggered: false,
        compaction_requested: false,
        layered_compaction_retain_tokens: None,
        layered_compaction_retained_items: None,
        layered_compaction_retained_chars: None,
        layered_compaction_before_response_body: None,
        service_tier: None,
        relay_id: Some("relay-chat".to_string()),
        relay_name: Some("Chat".to_string()),
        endpoint: Some("https://chat-only.example.test/v1/responses".to_string()),
        response_protocol: Some("responses".to_string()),
        status_code: Some(200),
        first_token_ms: Some(1),
        duration_ms: Some(2),
        stream: false,
        request_bytes: 2,
        response_bytes: Some(2),
        response_captured_bytes: Some(2),
        response_truncated: false,
        request_body: "{}".to_string(),
        response_body: "{}".to_string(),
        error: None,
    }
}

fn launcher_settings_path_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct LauncherUpstream {
    base_url: String,
    handle: std::thread::JoinHandle<Vec<LauncherUpstreamRequest>>,
}

impl LauncherUpstream {
    fn finish_all(self) -> Vec<LauncherUpstreamRequest> {
        self.handle.join().unwrap()
    }
}

struct LauncherUpstreamRequest {
    path: String,
    headers: BTreeMap<String, Vec<String>>,
    body: String,
}

impl LauncherUpstreamRequest {
    fn header_values(&self, name: &str) -> Option<&[String]> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    fn header_value(&self, name: &str) -> Option<&str> {
        self.header_values(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }
}

fn spawn_launcher_upstream_with_response(
    content_type: &str,
    response_body: String,
) -> LauncherUpstream {
    spawn_launcher_upstream_with_response_specs(vec![(
        content_type.to_string(),
        response_body,
        std::time::Duration::ZERO,
    )])
}

fn spawn_launcher_upstream_with_delayed_response(
    content_type: &str,
    response_body: String,
    response_delay: std::time::Duration,
) -> LauncherUpstream {
    spawn_launcher_upstream_with_response_specs(vec![(
        content_type.to_string(),
        response_body,
        response_delay,
    )])
}

fn spawn_launcher_upstream_with_status_response(
    status: &str,
    content_type: &str,
    response_body: String,
    response_delay: std::time::Duration,
) -> LauncherUpstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();
    let status = status.to_string();
    let content_type = content_type.to_string();

    let handle = std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        started.elapsed() < std::time::Duration::from_secs(10),
                        "test upstream did not receive a request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept test request: {error}"),
            }
        };
        let request = read_launcher_upstream_request(&mut stream);
        let request = launcher_upstream_request_from_raw(&request);
        if !response_delay.is_zero() {
            std::thread::sleep(response_delay);
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).unwrap();
        vec![request]
    });

    LauncherUpstream { base_url, handle }
}

fn spawn_launcher_upstream_with_response_specs(
    response_specs: Vec<(String, String, std::time::Duration)>,
) -> LauncherUpstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();

    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (content_type, response_body, response_delay) in response_specs {
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < std::time::Duration::from_secs(10),
                            "test upstream did not receive a request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept test request: {error}"),
                }
            };

            let request = read_launcher_upstream_request(&mut stream);
            let request = launcher_upstream_request_from_raw(&request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                content_type,
                response_body.len(),
                response_body
            );
            if !response_delay.is_zero() {
                std::thread::sleep(response_delay);
            }
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(request);
        }
        requests
    });

    LauncherUpstream { base_url, handle }
}

fn spawn_launcher_upstream_with_status_response_specs(
    response_specs: Vec<(String, String, String, std::time::Duration)>,
) -> LauncherUpstream {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();

    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, content_type, response_body, response_delay) in response_specs {
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < std::time::Duration::from_secs(10),
                            "test upstream did not receive a request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept test request: {error}"),
                }
            };

            let request = read_launcher_upstream_request(&mut stream);
            let request = launcher_upstream_request_from_raw(&request);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            if !response_delay.is_zero() {
                std::thread::sleep(response_delay);
            }
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(request);
        }
        requests
    });

    LauncherUpstream { base_url, handle }
}

fn launcher_upstream_request_from_raw(request: &str) -> LauncherUpstreamRequest {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for line in request
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers)
        .unwrap_or(request)
        .lines()
        .skip(1)
    {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers
            .entry(name.trim().to_ascii_lowercase())
            .or_default()
            .push(value.trim().to_string());
    }
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    LauncherUpstreamRequest {
        path,
        headers,
        body,
    }
}

fn read_launcher_upstream_request(stream: &mut std::net::TcpStream) -> String {
    let mut request_bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let bytes = match stream.read(&mut buffer) {
            Ok(0) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(error) => panic!("failed to read test upstream request: {error}"),
        };
        request_bytes.extend_from_slice(&buffer[..bytes]);
        let request = String::from_utf8_lossy(&request_bytes);
        if let Some(header_end) = request.find("\r\n\r\n") {
            let content_length = request
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            if request_bytes.len() >= header_end + 4 + content_length {
                return request.to_string();
            }
        }
    }
}

async fn audit_transport_mock(
    responses: Vec<(&'static str, &'static str, String)>,
) -> (
    String,
    tokio::task::JoinHandle<Vec<LauncherUpstreamRequest>>,
) {
    audit_transport_mock_with_followup_check(responses, false).await
}

async fn audit_transport_mock_with_followup_check(
    responses: Vec<(&'static str, &'static str, String)>,
    check_no_followup: bool,
) -> (
    String,
    tokio::task::JoinHandle<Vec<LauncherUpstreamRequest>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, content_type, body) in responses {
            let (mut stream, _) =
                tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                    .await
                    .expect("测试上游未收到请求")
                    .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0, "请求体读取完成前连接关闭");
                bytes.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&bytes);
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    continue;
                };
                let content_length = head
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if body.len() >= content_length {
                    requests.push(launcher_upstream_request_from_raw(&text));
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        if check_no_followup {
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept(),)
                    .await
                    .is_err(),
                "完成拒绝答复后仍发出了额外请求"
            );
        }
        requests
    });
    (base_url, task)
}

fn audit_transport_settings(
    base_url: &str,
    relay_id: &str,
    protocol: &str,
    model: &str,
) -> BackendSettings {
    serde_json::from_value(audit_transport_settings_json(
        base_url, relay_id, protocol, model,
    ))
    .unwrap()
}

fn audit_transport_settings_json(
    base_url: &str,
    relay_id: &str,
    protocol: &str,
    model: &str,
) -> serde_json::Value {
    serde_json::json!({
        "relayProfiles": [{
            "id": relay_id,
            "name": relay_id,
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [{
                "requestModel": model,
                "protocol": protocol,
                "contextWindow": "200000"
            }]
        }],
        "activeRelayId": relay_id
    })
}

fn audit_transport_completed_sse(id: &str, reasoning_tokens: u64) -> String {
    format!(
        "event: response.completed\ndata: {}\n\n",
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": id,
                "object": "response",
                "status": "completed",
                "model": "gpt-responses",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{"type": "output_text", "text": "首轮完整答案"}]
                }],
                "usage": {"output_tokens_details": {"reasoning_tokens": reasoning_tokens}}
            }
        })
    )
}

#[tokio::test]
async fn audit_transport_compact_preserves_the_native_operation() {
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let (base_url, upstream) = audit_transport_mock(vec![(
        "200 OK",
        "application/json",
        serde_json::json!({
            "id": "cmp_native",
            "object": "response.compaction",
            "output": [{"type": "compaction", "encrypted_content": "opaque"}]
        })
        .to_string(),
    )])
    .await;
    write_launcher_mixed_relay_settings(temp.path(), &base_url);
    let hooks = DefaultLaunchHooks::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    hooks.start_helper(port).await.unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap()
        .post(format!("http://127.0.0.1:{port}/v1/responses/compact"))
        .json(&serde_json::json!({
            "model": "gpt-responses",
            "input": [{"role": "user", "content": "压缩已有历史"}]
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    hooks.shutdown_helper(port).await;
    let requests = upstream.await.unwrap();
    assert_eq!(requests[0].path, "/v1/responses/compact");
    assert_eq!(body["object"], "response.compaction");
}

#[tokio::test]
async fn audit_transport_compact_rejects_converted_protocol_before_sending() {
    use codex_elves_core::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let settings =
        audit_transport_settings(&base_url, "compact-chat", "chatCompletions", "gpt-chat");
    let context = RequestContext::default().with_responses_path("/v1/responses/compact");
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        open_responses_proxy_request_with_settings_and_request_context(
            r#"{"model":"gpt-chat","input":"压缩历史"}"#,
            settings,
            &context,
        ),
    )
    .await;
    let sent = tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
        .await
        .is_ok();
    assert!(!sent, "compact 请求不得转换成 Chat 并发往上游");
    let response = result.expect("本地拒绝不得等待上游").unwrap();
    assert_eq!(response.status_code, 400);
}

#[tokio::test]
async fn audit_transport_chat_selects_an_aggregate_member() {
    use codex_elves_core::protocol_proxy::open_chat_completions_proxy_request;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let (base_url, upstream) = audit_transport_mock(vec![(
        "200 OK",
        "application/json",
        r#"{"id":"chatcmpl_member","choices":[]}"#.to_string(),
    )])
    .await;
    let mut settings =
        audit_transport_settings_json(&base_url, "chat-member", "chatCompletions", "gpt-chat");
    settings["relayProfiles"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "chat-aggregate",
            "name": "chat aggregate",
            "relayMode": "aggregate",
            "localProxyEnabled": false
        }));
    settings["activeRelayId"] = serde_json::json!("chat-aggregate");
    settings["activeAggregateRelayId"] = serde_json::json!("chat-aggregate");
    settings["aggregateRelayProfiles"] = serde_json::json!([{
        "id": "chat-aggregate",
        "name": "chat aggregate",
        "strategy": "requestRoundRobin",
        "members": [{"relayId": "chat-member", "weight": 1}]
    }]);
    std::fs::write(temp.path().join("settings.json"), settings.to_string()).unwrap();
    let result = open_chat_completions_proxy_request(
        r#"{"model":"gpt-chat","messages":[{"role":"user","content":"hello"}]}"#,
        None,
    )
    .await;
    if result.is_err() {
        upstream.abort();
    }
    let response = result.expect("聚合本身不开代理也应转发到真实成员");
    assert_eq!(response.relay_id.as_deref(), Some("chat-member"));
    response.into_body_bytes().await.unwrap();
    assert_eq!(upstream.await.unwrap()[0].path, "/v1/chat/completions");
}

#[tokio::test]
async fn audit_transport_compact_skips_converted_aggregate_members() {
    use codex_elves_core::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let chat = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let chat_url = format!("http://{}/v1", chat.local_addr().unwrap());
    let (base_url, upstream) = audit_transport_mock(vec![(
        "200 OK",
        "application/json",
        r#"{"id":"cmp_aggregate","object":"response.compaction","output":[]}"#.to_string(),
    )])
    .await;
    let mut settings = audit_transport_settings_json(
        &chat_url,
        "compact-chat",
        "chatCompletions",
        "gpt-responses",
    );
    let native =
        audit_transport_settings_json(&base_url, "compact-native", "responses", "gpt-responses");
    settings["relayProfiles"]
        .as_array_mut()
        .unwrap()
        .push(native["relayProfiles"][0].clone());
    settings["relayProfiles"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id":"compact-aggregate","name":"compact aggregate","relayMode":"aggregate"
        }));
    settings["activeRelayId"] = serde_json::json!("compact-aggregate");
    settings["activeAggregateRelayId"] = serde_json::json!("compact-aggregate");
    settings["aggregateRelayProfiles"] = serde_json::json!([{
        "id":"compact-aggregate","name":"compact aggregate","strategy":"requestRoundRobin",
        "members":[{"relayId":"compact-chat","weight":1},{"relayId":"compact-native","weight":1}]
    }]);
    let settings = serde_json::from_value(settings).unwrap();
    let context = RequestContext::default().with_responses_path("/v1/responses/compact");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        open_responses_proxy_request_with_settings_and_request_context(
            r#"{"model":"gpt-responses","input":"压缩上下文"}"#,
            settings,
            &context,
        ),
    )
    .await;
    if result.is_err() {
        upstream.abort();
    }
    let response = result.expect("应跳过不支持compact的成员").unwrap();
    assert_eq!(response.relay_id.as_deref(), Some("compact-native"));
    response.into_body_bytes().await.unwrap();
    assert_eq!(upstream.await.unwrap()[0].path, "/v1/responses/compact");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), chat.accept())
            .await
            .is_err(),
        "compact 请求不应发送给 Chat 成员"
    );
}

#[tokio::test]
async fn audit_transport_anthropic_error_body_obeys_the_request_timeout() {
    use codex_elves_core::protocol_proxy::open_responses_proxy_request_with_stream_header_timeout;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let settings =
        audit_transport_settings_json(&base_url, "body-timeout", "anthropic", "deepseek-v4-flash");
    std::fs::write(
        temp.path().join("settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let upstream = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0_u8; 8192];
        stream.read(&mut buffer).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 999\r\n\r\n{")
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(600),
        open_responses_proxy_request_with_stream_header_timeout(
            r#"{"model":"deepseek-v4-flash","input":"hello","stream":true,"reasoning":{"effort":"max"}}"#,
            None,
            std::time::Duration::from_millis(100),
        ),
    )
    .await;
    upstream.abort();
    assert!(result.is_ok(), "400 响应体无限等待，已绕过请求超时");
    let error = result.unwrap().err().expect("不完整的错误体必须失败");
    assert!(codex_elves_core::protocol_proxy::upstream_error_is_timeout(
        &error
    ));
}

#[tokio::test]
async fn audit_transport_anthropic_effort_cache_isolated_by_provider() {
    use codex_elves_core::protocol_proxy::{
        clear_anthropic_reasoning_compatibility_cache_for_tests,
        open_responses_proxy_request_with_settings,
    };
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let failure =
        r#"{"error":{"message":"max is not supported; valid levels are low, medium, high"}}"#;
    let request = r#"{"model":"deepseek-v4-flash","input":"hello","reasoning":{"effort":"max"}}"#;
    let mut failures = Vec::new();
    for same_endpoint in [false, true] {
        clear_anthropic_reasoning_compatibility_cache_for_tests();
        let mut first_responses = vec![
            ("400 Bad Request", "application/json", failure.to_string()),
            ("200 OK", "application/json", "{}".to_string()),
        ];
        if same_endpoint {
            first_responses.push(("200 OK", "application/json", "{}".to_string()));
        }
        let (first_url, first) = audit_transport_mock(first_responses).await;
        let (second_url, second) = if same_endpoint {
            (first_url.clone(), None)
        } else {
            let (url, task) =
                audit_transport_mock(vec![("200 OK", "application/json", "{}".to_string())]).await;
            (url, Some(task))
        };
        let first_settings =
            audit_transport_settings(&first_url, "provider-a", "anthropic", "deepseek-v4-flash");
        open_responses_proxy_request_with_settings(request, first_settings)
            .await
            .unwrap()
            .into_body_bytes()
            .await
            .unwrap();
        let second_id = if same_endpoint {
            "provider-b"
        } else {
            "provider-a"
        };
        let second_settings =
            audit_transport_settings(&second_url, second_id, "anthropic", "deepseek-v4-flash");
        open_responses_proxy_request_with_settings(request, second_settings)
            .await
            .unwrap()
            .into_body_bytes()
            .await
            .unwrap();
        let first_requests = first.await.unwrap();
        let first_retry: serde_json::Value = serde_json::from_str(&first_requests[1].body).unwrap();
        assert_eq!(first_retry["output_config"]["effort"], "high");
        let second_requests = match second {
            Some(task) => task.await.unwrap(),
            None => first_requests,
        };
        let received: serde_json::Value =
            serde_json::from_str(&second_requests.last().unwrap().body).unwrap();
        if received["output_config"]["effort"] != "max" {
            failures.push(if same_endpoint {
                "相同端点不同供应商"
            } else {
                "相同供应商切换端点"
            });
        }
    }
    clear_anthropic_reasoning_compatibility_cache_for_tests();
    assert_eq!(failures, Vec::<&str>::new(), "兼容缓存未按供应商和端点隔离");
}

#[tokio::test]
async fn audit_transport_non_streaming_header_wait_obeys_request_budget() {
    use codex_elves_core::protocol_proxy::open_responses_proxy_request_with_stream_header_timeout;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let _guard = LauncherSettingsPathGuard::set(temp.path().join("settings.json"));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    write_launcher_mixed_relay_settings(temp.path(), &base_url);
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(600),
        open_responses_proxy_request_with_stream_header_timeout(
            r#"{"model":"gpt-responses","input":"hello","stream":false}"#,
            None,
            std::time::Duration::from_millis(100),
        ),
    )
    .await;
    assert!(result.is_ok(), "非流式请求忽略了头等待预算");
    let error = result.unwrap().err().expect("上游没有响应头时必须超时");
    assert!(codex_elves_core::protocol_proxy::upstream_error_is_timeout(
        &error
    ));
}

#[tokio::test]
async fn audit_transport_capacity_retry_preserves_converted_success() {
    use codex_elves_core::protocol_proxy::apply_continue_thinking_to_responses_stream;
    use codex_elves_core::relay_rotation::{RotationContext, select_relay_for_request};
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut failures = Vec::new();
    for (case, protocol, content_type, body, expected_path) in [
        (
            "chat-sse",
            "chatCompletions",
            "text/event-stream",
            concat!(
                "data: {\"id\":\"chatcmpl_capacity_recovered\",\"model\":\"gpt-responses\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"容量恢复回答\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_capacity_recovered\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ),
            "/v1/chat/completions",
        ),
        (
            "chat-json",
            "chatCompletions",
            "application/json",
            r#"{"id":"chatcmpl_recovered","model":"gpt-responses","choices":[{"index":0,"message":{"role":"assistant","content":"容量恢复回答"},"finish_reason":"stop"}]}"#,
            "/v1/chat/completions",
        ),
        (
            "anthropic-json",
            "anthropic",
            "application/json",
            r#"{"id":"msg_recovered","type":"message","model":"gpt-responses","role":"assistant","content":[{"type":"text","text":"容量恢复回答"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}"#,
            "/v1/messages",
        ),
        (
            "anthropic-sse",
            "anthropic",
            "text/event-stream",
            concat!(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_recovered\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"gpt-responses\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"容量恢复回答\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            ),
            "/v1/messages",
        ),
    ] {
        let (base_url, upstream) =
            audit_transport_mock(vec![("200 OK", content_type, body.to_string())]).await;
        let mut settings = audit_transport_settings_json(
            "http://127.0.0.1:9/v1",
            "capacity-a",
            "responses",
            "gpt-responses",
        );
        let second =
            audit_transport_settings_json(&base_url, "capacity-b", protocol, "gpt-responses");
        settings["relayProfiles"]
            .as_array_mut()
            .unwrap()
            .push(second["relayProfiles"][0].clone());
        settings["relayProfiles"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "capacity-aggregate", "name": "capacity aggregate", "relayMode": "aggregate"
            }));
        settings["activeRelayId"] = serde_json::json!("capacity-aggregate");
        settings["activeAggregateRelayId"] = serde_json::json!("capacity-aggregate");
        settings["aggregateRelayProfiles"] = serde_json::json!([{
            "id": "capacity-aggregate",
            "name": "capacity aggregate",
            "strategy": "requestRoundRobin",
            "members": [{"relayId": "capacity-a", "weight": 1}, {"relayId": "capacity-b", "weight": 1}]
        }]);
        let settings: BackendSettings = serde_json::from_value(settings).unwrap();
        assert_eq!(
            select_relay_for_request(&settings, RotationContext::default())
                .unwrap()
                .id,
            "capacity-a"
        );
        let first = "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_busy\",\"status\":\"failed\",\"error\":{\"code\":\"server_is_overloaded\",\"message\":\"Our servers are currently overloaded.\"}}}\n\n";
        let result = apply_continue_thinking_to_responses_stream(
            &serde_json::json!({"model":"gpt-responses","input":"hello","stream":true}),
            settings,
            None,
            first.to_string(),
        )
        .await;
        let requests = upstream.await.unwrap();
        assert_eq!(requests[0].path, expected_path);
        if !result.sse_text.contains("容量恢复回答")
            || !result.sse_text.contains("response.completed")
            || result.sse_text.contains("server_is_overloaded")
        {
            failures.push(case);
        }
    }
    assert!(
        failures.is_empty(),
        "成功的跨协议/不同响应格式重试结果被丢弃: {failures:?}"
    );
}

#[tokio::test]
async fn audit_transport_failed_continuation_keeps_completed_answer() {
    use codex_elves_core::protocol_proxy::apply_continue_thinking_to_responses_stream;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut failures = Vec::new();
    for (name, next) in [
        (
            "failed",
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",\"status\":\"failed\",\"error\":{\"message\":\"upstream failed\"},\"output\":[]}}\n\n",
        ),
        (
            "unterminated",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"unfinished\"}\n\n",
        ),
    ] {
        let (base_url, upstream) =
            audit_transport_mock(vec![("200 OK", "text/event-stream", next.to_string())]).await;
        let mut settings = audit_transport_settings(&base_url, name, "responses", "gpt-responses");
        settings.gpt_reasoning_continuation = true;
        settings.gpt_reasoning_continuation_max_rounds = 1;
        let first = audit_transport_completed_sse("resp_first_complete", 516);
        let result = apply_continue_thinking_to_responses_stream(
            &serde_json::json!({"model":"gpt-responses","input":"hello","stream":true}),
            settings,
            None,
            first.clone(),
        )
        .await;
        assert_eq!(upstream.await.unwrap().len(), 1);
        if result.sse_text != first || result.rounds != 0 {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "失败续写覆盖了首轮答案: {failures:?}");
}

#[tokio::test]
async fn audit_transport_ineligible_response_never_starts_http_continuation() {
    use codex_elves_core::protocol_proxy::apply_continue_thinking_to_responses_stream;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut failures = Vec::new();
    for response in [
        serde_json::json!({
            "id": "resp_failed_initial",
            "status": "failed",
            "error": {"message":"generation failed"},
            "output": [],
            "usage": {"output_tokens_details":{"reasoning_tokens":516}}
        }),
        serde_json::json!({
            "id": "resp_refusal_initial",
            "status": "completed",
            "output": [{"type":"message","content":[{"type":"refusal","refusal":"I cannot help"}]}],
            "usage": {"output_tokens_details":{"reasoning_tokens":516}}
        }),
    ] {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let mut settings =
            audit_transport_settings(&base_url, "ineligible", "responses", "gpt-responses");
        settings.gpt_reasoning_continuation = true;
        let event = if response["status"] == "failed" {
            "response.failed"
        } else {
            "response.completed"
        };
        let first = format!(
            "event: {event}\ndata: {}\n\n",
            serde_json::json!({"type":event,"response":response})
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            apply_continue_thinking_to_responses_stream(
                &serde_json::json!({"model":"gpt-responses","input":"hello","stream":true}),
                settings,
                None,
                first.clone(),
            ),
        )
        .await;
        if !result.is_ok_and(|result| !result.triggered && result.sse_text == first) {
            failures.push(response["id"].as_str().unwrap().to_string());
        }
    }
    assert!(
        failures.is_empty(),
        "失败或拒绝响应仍然触发了HTTP续写: {failures:?}"
    );
}

#[tokio::test]
async fn audit_transport_completed_refusal_is_accepted_without_another_continuation() {
    use codex_elves_core::protocol_proxy::apply_continue_thinking_to_responses_stream;
    let _lock = launcher_settings_path_test_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let refusal = format!(
        "event: response.completed\ndata: {}\n\n",
        serde_json::json!({
            "type":"response.completed",
            "response": {
                "id":"resp_final_refusal",
                "status":"completed",
                "output":[{"type":"message","content":[{"type":"refusal","refusal":"合法拒绝答复"}]}],
                "usage":{"output_tokens_details":{"reasoning_tokens":516}}
            }
        })
    );
    let (base_url, upstream) = audit_transport_mock_with_followup_check(
        vec![("200 OK", "text/event-stream", refusal.clone())],
        true,
    )
    .await;
    let mut settings =
        audit_transport_settings(&base_url, "refusal-final", "responses", "gpt-responses");
    settings.gpt_reasoning_continuation = true;
    settings.gpt_reasoning_continuation_max_rounds = 3;
    let result = apply_continue_thinking_to_responses_stream(
        &serde_json::json!({"model":"gpt-responses","input":"hello","stream":true}),
        settings,
        None,
        audit_transport_completed_sse("resp_first_complete", 516),
    )
    .await;
    assert_eq!(upstream.await.unwrap().len(), 1);
    assert_eq!(
        result.sse_text, refusal,
        "合法完成的拒绝答复应该成为最终结果"
    );
    assert_eq!(result.rounds, 1);
}

fn write_launcher_mixed_relay_settings(settings_dir: &Path, base_url: &str) {
    let settings = serde_json::json!({
        "relayProfiles": [{
            "id": "mixed",
            "name": "Mixed",
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "protocol": "responses",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [
                {
                    "requestModel": "gpt-responses",
                    "protocol": "responses",
                    "contextWindow": "200000"
                },
                {
                    "requestModel": "gpt-chat",
                    "protocol": "chatCompletions",
                    "contextWindow": "200000"
                },
                {
                    "requestModel": "claude-sonnet-4",
                    "protocol": "anthropic",
                    "contextWindow": "200000"
                }
            ]
        }],
        "activeRelayId": "mixed"
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
}
