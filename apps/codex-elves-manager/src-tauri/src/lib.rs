pub mod commands;
pub mod install;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size, WindowEvent};

static APP_EXITING: AtomicBool = AtomicBool::new(false);
const TRAY_MENU_SHOW: &str = "tray_show_main";
const TRAY_MENU_QUIT: &str = "tray_quit_app";
const MANAGER_WAKE_MESSAGE: &[u8] = b"codex-elves-manager:show-main-window\n";
const MANAGER_WINDOW_STATE_FILE: &str = "manager-window-state.json";
const DEV_MANAGER_WINDOW_STATE_FILE: &str = "manager-window-state-dev.json";
const DEFAULT_WINDOW_WIDTH: f64 = 1180.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 820.0;
const MIN_WINDOW_WIDTH: u32 = 960;
const MIN_WINDOW_HEIGHT: u32 = 720;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagerWindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn run() {
    install_panic_logger();
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "manager.start",
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION")
        }),
    );
    let Some(guard) = acquire_single_instance_guard() else {
        return;
    };
    let wake_listener = match guard.try_clone_listener() {
        Ok(listener) => listener,
        Err(error) => {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "manager.wake_listener_clone_failed",
                serde_json::json!({
                    "error": error.to_string()
                }),
            );
            None
        }
    };
    let show_update = commands::startup_should_show_update();
    let run_result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let url = if show_update {
                "index.html?showUpdate=1"
            } else {
                "index.html"
            };
            let restore_window_state = load_manager_window_state()
                .map(clamp_manager_window_state)
                .filter(|state| manager_window_state_is_visible(&app.handle(), state));
            let mut main_window_builder =
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App(url.into()))
                    .title(manager_window_title())
                    .inner_size(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
                    .min_inner_size(f64::from(MIN_WINDOW_WIDTH), f64::from(MIN_WINDOW_HEIGHT));
            if restore_window_state.is_some() {
                main_window_builder = main_window_builder.visible(false);
            }
            let main_window = main_window_builder.build()?;
            if let Some(state) = restore_window_state {
                apply_manager_window_state(&main_window, state);
                let _ = main_window.show();
                let _ = main_window.set_focus();
            }
            install_tray(app)?;
            register_main_window_events(main_window);
            if let Some(listener) = wake_listener {
                spawn_manager_wake_listener(listener, app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::backend_version,
            commands::startup_options,
            commands::load_overview,
            commands::launch_codex_elves,
            commands::restart_codex_elves,
            commands::load_settings,
            commands::save_settings,
            commands::load_ccs_providers,
            commands::import_ccs_providers,
            commands::list_local_sessions,
            commands::delete_local_session,
            commands::load_provider_sync_targets,
            commands::sync_providers_now,
            commands::refresh_script_market,
            commands::install_market_script,
            commands::set_user_script_enabled,
            commands::delete_user_script,
            commands::open_external_url,
            commands::install_entrypoints,
            commands::uninstall_entrypoints,
            commands::repair_shortcuts,
            commands::repair_backend,
            commands::plugin_marketplace_status,
            commands::repair_plugin_marketplace,
            commands::check_update,
            commands::perform_update,
            commands::load_watcher_state,
            commands::install_watcher,
            commands::uninstall_watcher,
            commands::enable_watcher,
            commands::disable_watcher,
            commands::read_latest_logs,
            commands::copy_diagnostics,
            commands::reset_settings,
            commands::reset_image_overlay_settings,
            commands::relay_status,
            commands::read_relay_files,
            commands::check_env_conflicts,
            commands::remove_env_conflicts,
            commands::save_relay_file,
            commands::write_diagnostic_event,
            commands::backfill_relay_profile_from_live,
            commands::list_context_entries,
            commands::read_live_context_entries,
            commands::sync_live_context_entries,
            commands::upsert_context_entry,
            commands::delete_context_entry,
            commands::test_relay_profile,
            commands::fetch_relay_profile_models,
            commands::switch_relay_profile,
            commands::apply_relay_injection,
            commands::apply_pure_api_injection,
            commands::clear_relay_injection
        ])
        .run(tauri::generate_context!());
    if let Err(error) = run_result {
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "manager.run_failed",
            serde_json::json!({
                "error": error.to_string()
            }),
        );
    }
}

fn install_tray<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, TRAY_MENU_SHOW, "显示主窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, TRAY_MENU_QUIT, "退出程序", true, None::<&str>)?;
    let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            TRAY_MENU_SHOW => {
                show_main_window(app);
            }
            TRAY_MENU_QUIT => {
                APP_EXITING.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                show_main_window(&tray.app_handle());
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let _ = tray_builder.build(app)?;
    Ok(())
}

fn register_main_window_events<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) {
    let event_window = window.clone();
    let minimized_window = event_window.clone();
    let moved_window = event_window.clone();
    let resized_window = event_window.clone();
    let close_window = event_window.clone();

    event_window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) => {
            persist_manager_window_state(&moved_window);
        }
        WindowEvent::Resized(_) => {
            if matches!(minimized_window.is_minimized(), Ok(true)) {
                let _ = minimized_window.hide();
            } else {
                persist_manager_window_state(&resized_window);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            if APP_EXITING.load(Ordering::SeqCst) {
                return;
            }

            api.prevent_close();
            persist_manager_window_state(&close_window);
            let _ = close_window.hide();
        }
        _ => {}
    });
}

fn manager_window_state_path() -> PathBuf {
    codex_elves_core::paths::default_app_state_dir().join(manager_window_state_file())
}

fn manager_dev_mode() -> bool {
    cfg!(debug_assertions)
        || std::env::var("CODEX_ELVES_MANAGER_DEV")
            .map(|value| value == "1")
            .unwrap_or(false)
}

fn manager_window_title() -> &'static str {
    if manager_dev_mode() {
        "CodexElves 管理工具 Dev"
    } else {
        "CodexElves 管理工具"
    }
}

fn manager_window_state_file() -> &'static str {
    if manager_dev_mode() {
        DEV_MANAGER_WINDOW_STATE_FILE
    } else {
        MANAGER_WINDOW_STATE_FILE
    }
}

fn load_manager_window_state() -> Option<ManagerWindowState> {
    let path = manager_window_state_path();
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_manager_window_state(state: ManagerWindowState) {
    let path = manager_window_state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
        let _ = std::fs::write(path, bytes);
    }
}

fn persist_manager_window_state<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if matches!(window.is_minimized(), Ok(true)) || matches!(window.is_fullscreen(), Ok(true)) {
        return;
    }
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    save_manager_window_state(clamp_manager_window_state(ManagerWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }));
}

fn clamp_manager_window_state(state: ManagerWindowState) -> ManagerWindowState {
    ManagerWindowState {
        x: state.x,
        y: state.y,
        width: state.width.max(MIN_WINDOW_WIDTH),
        height: state.height.max(MIN_WINDOW_HEIGHT),
    }
}

fn apply_manager_window_state<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    state: ManagerWindowState,
) {
    let state = clamp_manager_window_state(state);
    let _ = window.set_size(Size::Physical(PhysicalSize::new(state.width, state.height)));
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(state.x, state.y)));
}

fn manager_window_state_is_visible<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    state: &ManagerWindowState,
) -> bool {
    app_handle
        .available_monitors()
        .map(|monitors| {
            monitors
                .iter()
                .any(|monitor| manager_window_state_intersects_monitor(state, monitor))
        })
        .unwrap_or(false)
}

fn manager_window_state_intersects_monitor(
    state: &ManagerWindowState,
    monitor: &tauri::Monitor,
) -> bool {
    let area = monitor.work_area();
    let state_left = i64::from(state.x);
    let state_top = i64::from(state.y);
    let state_right = state_left + i64::from(state.width);
    let state_bottom = state_top + i64::from(state.height);
    let monitor_left = i64::from(area.position.x);
    let monitor_top = i64::from(area.position.y);
    let monitor_right = monitor_left + i64::from(area.size.width);
    let monitor_bottom = monitor_top + i64::from(area.size.height);

    state_left < monitor_right
        && state_right > monitor_left
        && state_top < monitor_bottom
        && state_bottom > monitor_top
}

fn show_main_window<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn spawn_manager_wake_listener<R: tauri::Runtime>(
    listener: std::net::TcpListener,
    app_handle: tauri::AppHandle<R>,
) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buffer = [0_u8; MANAGER_WAKE_MESSAGE.len()];
            if stream.read_exact(&mut buffer).is_err() {
                continue;
            }
            if buffer.as_slice() == MANAGER_WAKE_MESSAGE {
                show_main_window(&app_handle);
            }
        }
    });
}

fn request_existing_manager_to_show(manager_guard_port: u16) -> std::io::Result<()> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], manager_guard_port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(MANAGER_WAKE_MESSAGE)?;
    stream.flush()
}

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "非字符串 panic payload".to_string());
        let location = panic_info.location().map(|location| {
            serde_json::json!({
                "file": location.file(),
                "line": location.line(),
                "column": location.column()
            })
        });
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "manager.panic",
            serde_json::json!({
                "payload": payload,
                "location": location
            }),
        );
    }));
}

fn acquire_single_instance_guard() -> Option<codex_elves_core::ports::LoopbackPortGuard> {
    let manager_guard_port = codex_elves_core::ports::manager_guard_port();
    match codex_elves_core::ports::acquire_resilient_loopback_port_guard(manager_guard_port) {
        Ok(guard) => {
            if let Some(fallback_lock_path) = guard.fallback_path() {
                let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                    "manager.guard_fallback",
                    serde_json::json!({
                        "requested_guard_port": manager_guard_port,
                        "fallback_lock_path": fallback_lock_path
                    }),
                );
            }
            Some(guard)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            let wake_result = request_existing_manager_to_show(manager_guard_port);
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "manager.already_running",
                serde_json::json!({
                    "guard_port": manager_guard_port,
                    "wake_requested": wake_result.is_ok(),
                    "wake_error": wake_result.err().map(|error| error.to_string())
                }),
            );
            None
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            let wake_result = request_existing_manager_to_show(manager_guard_port);
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "manager.already_running",
                serde_json::json!({
                    "guard_port": manager_guard_port,
                    "wake_requested": wake_result.is_ok(),
                    "wake_error": wake_result.err().map(|error| error.to_string())
                }),
            );
            None
        }
        Err(error) => {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "manager.guard_failed",
                serde_json::json!({
                    "guard_port": manager_guard_port,
                    "error": error.to_string()
                }),
            );
            match std::net::TcpListener::bind(("127.0.0.1", 0)) {
                Ok(listener) => Some(codex_elves_core::ports::LoopbackPortGuard::listener(
                    listener,
                )),
                Err(fallback_error) => {
                    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                        "manager.guard_fallback_failed",
                        serde_json::json!({
                            "error": fallback_error.to_string()
                        }),
                    );
                    None
                }
            }
        }
    }
}
