#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result};
use codex_elves_core::launcher::{
    DefaultLaunchHooks, LaunchHooks, LaunchOptions, launch_and_inject_with_hooks,
};
use codex_elves_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_elves_core::routes::{BridgeContext, BridgeDataService, BridgeRuntimeService};
use codex_elves_core::task_board::{
    TaskBoardCatalogProject, TaskBoardCatalogSession, TaskBoardCatalogWarning,
    TaskBoardCatalogWarningCode, TaskBoardSessionCatalog, normalize_task_project_cwd,
    task_board_timestamp_from_bridge_i64,
};
use codex_elves_core::user_scripts::UserScriptManager;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LAUNCHER_REPAIR_COMMAND: &[u8] = b"codex-elves:repair-bridge\n";
const LAUNCHER_REPAIR_ACK: &[u8] = b"ok\n";
const LAUNCHER_CONTROL_TIMEOUT: Duration = Duration::from_millis(750);
const LAUNCHER_CONTROL_MAX_CONNECTIONS: usize = 8;

#[derive(Clone)]
struct LauncherHooks {
    core: Arc<DefaultLaunchHooks>,
    data: Arc<LauncherDataService>,
    runtime: Arc<LauncherRuntimeService>,
    app_dir: Arc<Mutex<Option<PathBuf>>>,
    bridge_runtime: Arc<tokio::sync::Mutex<Option<codex_elves_core::bridge::BridgeRuntime>>>,
    bridge_watchdog: Arc<tokio::sync::Mutex<Option<LauncherBridgeWatchdogRuntime>>>,
    bridge_repair_notify: Arc<tokio::sync::Notify>,
}

impl Default for LauncherHooks {
    fn default() -> Self {
        Self {
            core: Arc::new(DefaultLaunchHooks::default()),
            data: Arc::new(LauncherDataService::default()),
            runtime: Arc::new(LauncherRuntimeService::new(
                9229,
                default_user_script_manager(),
            )),
            app_dir: Arc::new(Mutex::new(None)),
            bridge_runtime: Arc::new(tokio::sync::Mutex::new(None)),
            bridge_watchdog: Arc::new(tokio::sync::Mutex::new(None)),
            bridge_repair_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

struct LauncherBridgeWatchdogRuntime {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = parse_launch_options(std::env::args().skip(1));
    let Some(guard) = acquire_single_instance_guard(options.debug_port)? else {
        request_existing_launcher_bridge_repair();
        activate_existing_codex_app(&options).await?;
        return Ok(());
    };
    if codex_elves_core::paths::obsolete_session_backup_cleanup_needed() {
        spawn_obsolete_session_backup_cleanup();
    }
    tokio::spawn(async {
        let _ = notify_manager_when_update_available().await;
    });
    let hooks = LauncherHooks::default();
    if let Err(error) = start_launcher_control_listener(&guard, hooks.bridge_repair_notify.clone())
    {
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "launcher.control_start_failed_nonfatal",
            json!({
                "message": error.to_string()
            }),
        );
    }
    let handle = launch_and_inject_with_hooks(options, &hooks).await?;
    handle.wait_for_codex_exit().await?;
    Ok(())
}

fn spawn_obsolete_session_backup_cleanup() {
    drop(tokio::task::spawn_blocking(|| {
        if let Err(error) = codex_elves_core::paths::cleanup_obsolete_session_backups() {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "launcher.obsolete_session_backups_cleanup_failed",
                json!({
                    "message": error.to_string()
                }),
            );
        }
    }));
}

fn start_launcher_control_listener(
    guard: &codex_elves_core::ports::LoopbackPortGuard,
    bridge_repair_notify: Arc<tokio::sync::Notify>,
) -> anyhow::Result<bool> {
    let Some(listener) = guard.try_clone_listener()? else {
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "launcher.control_unavailable",
            json!({
                "guard_port": codex_elves_core::ports::launcher_guard_port(),
                "reason": "fallback_lock"
            }),
        );
        return Ok(false);
    };
    listener.set_nonblocking(true)?;
    let weak_notify = Arc::downgrade(&bridge_repair_notify);
    let active_connections = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("codex-elves-launcher-control".to_string())
        .spawn(move || {
            loop {
                let Some(bridge_repair_notify) = weak_notify.upgrade() else {
                    break;
                };
                match listener.accept() {
                    Ok((stream, peer)) => {
                        if active_connections
                            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                                (count < LAUNCHER_CONTROL_MAX_CONNECTIONS).then_some(count + 1)
                            })
                            .is_err()
                        {
                            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                                "launcher.control_request_dropped",
                                json!({
                                    "peer": peer.to_string(),
                                    "reason": "connection_limit",
                                    "limit": LAUNCHER_CONTROL_MAX_CONNECTIONS
                                }),
                            );
                            continue;
                        }
                        let request_notify = bridge_repair_notify.clone();
                        let request_connections = active_connections.clone();
                        let peer_text = peer.to_string();
                        let request_peer = peer_text.clone();
                        if let Err(error) = std::thread::Builder::new()
                            .name("codex-elves-launcher-control-request".to_string())
                            .spawn(move || {
                                let handled =
                                    handle_launcher_control_connection(stream, &request_notify);
                                request_connections.fetch_sub(1, Ordering::AcqRel);
                                let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                                    "launcher.control_request",
                                    json!({
                                        "peer": request_peer,
                                        "handled": handled
                                    }),
                                );
                            })
                        {
                            active_connections.fetch_sub(1, Ordering::AcqRel);
                            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                                "launcher.control_request_thread_failed",
                                json!({
                                    "peer": peer_text,
                                    "message": error.to_string()
                                }),
                            );
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        drop(bridge_repair_notify);
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => {
                        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                            "launcher.control_accept_failed",
                            json!({
                                "message": error.to_string()
                            }),
                        );
                        break;
                    }
                }
            }
        })?;
    Ok(true)
}

fn handle_launcher_control_connection(
    mut stream: TcpStream,
    bridge_repair_notify: &tokio::sync::Notify,
) -> bool {
    let _ = stream.set_read_timeout(Some(LAUNCHER_CONTROL_TIMEOUT));
    let _ = stream.set_write_timeout(Some(LAUNCHER_CONTROL_TIMEOUT));
    let mut command = vec![0u8; LAUNCHER_REPAIR_COMMAND.len()];
    if stream.read_exact(&mut command).is_err() || command != LAUNCHER_REPAIR_COMMAND {
        return false;
    }
    bridge_repair_notify.notify_one();
    stream.write_all(LAUNCHER_REPAIR_ACK).is_ok()
}

fn request_existing_launcher_bridge_repair() -> bool {
    let guard_port = codex_elves_core::ports::launcher_guard_port();
    let repaired = send_launcher_bridge_repair(guard_port);
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.bridge_repair_requested",
        json!({
            "guard_port": guard_port,
            "delivered": repaired
        }),
    );
    repaired
}

fn send_launcher_bridge_repair(guard_port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], guard_port));
    TcpStream::connect_timeout(&address, LAUNCHER_CONTROL_TIMEOUT)
        .and_then(|mut stream| {
            stream.set_read_timeout(Some(LAUNCHER_CONTROL_TIMEOUT))?;
            stream.set_write_timeout(Some(LAUNCHER_CONTROL_TIMEOUT))?;
            stream.write_all(LAUNCHER_REPAIR_COMMAND)?;
            let mut ack = vec![0u8; LAUNCHER_REPAIR_ACK.len()];
            stream.read_exact(&mut ack)?;
            Ok(ack == LAUNCHER_REPAIR_ACK)
        })
        .unwrap_or(false)
}

fn acquire_single_instance_guard(
    debug_port: u16,
) -> anyhow::Result<Option<codex_elves_core::ports::LoopbackPortGuard>> {
    acquire_single_instance_guard_with_retry(debug_port, true)
}

fn acquire_single_instance_guard_with_retry(
    debug_port: u16,
    allow_stale_recovery: bool,
) -> anyhow::Result<Option<codex_elves_core::ports::LoopbackPortGuard>> {
    match try_acquire_single_instance_guard() {
        Ok(guard) => {
            if let Some(fallback_lock_path) = guard.fallback_path() {
                log_launcher_guard_fallback(fallback_lock_path);
            }
            Ok(Some(guard))
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            log_launcher_already_running(debug_port);
            Ok(None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            log_launcher_already_running(debug_port);
            if allow_stale_recovery && should_recover_stale_launcher(debug_port) {
                codex_elves_core::watcher::stop_launcher_processes();
                std::thread::sleep(std::time::Duration::from_millis(250));
                return acquire_single_instance_guard_with_retry(debug_port, false);
            }
            Ok(None)
        }
        Err(error) => Err(error)
            .with_context(|| {
                format!(
                    "failed to acquire launcher guard port {}",
                    codex_elves_core::ports::launcher_guard_port()
                )
            })
            .map(Some),
    }
}

fn try_acquire_single_instance_guard() -> std::io::Result<codex_elves_core::ports::LoopbackPortGuard>
{
    codex_elves_core::ports::acquire_resilient_loopback_port_guard(
        codex_elves_core::ports::launcher_guard_port(),
    )
}

fn log_launcher_guard_fallback(fallback_lock_path: &Path) {
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.guard_fallback",
        json!({
            "requested_guard_port": codex_elves_core::ports::launcher_guard_port(),
            "fallback_lock_path": fallback_lock_path
        }),
    );
}

fn should_recover_stale_launcher(debug_port: u16) -> bool {
    let codex_process_ids = codex_elves_core::watcher::find_codex_processes();
    let has_codex_process = !codex_process_ids.is_empty();
    let cdp_listening = codex_elves_core::watcher::cdp_listening(debug_port);
    let recover =
        codex_elves_core::watcher::should_recover_stale_launcher(has_codex_process, cdp_listening);
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.stale_recovery_check",
        json!({
            "debug_port": debug_port,
            "has_codex_process": has_codex_process,
            "process_ids": codex_process_ids,
            "cdp_listening": cdp_listening,
            "recover": recover
        }),
    );
    recover
}

async fn activate_existing_codex_app(options: &LaunchOptions) -> anyhow::Result<()> {
    let hooks = LauncherHooks::default();
    let settings = hooks.load_settings().await?;
    let app_dir = hooks.resolve_app_dir(options.app_dir.as_deref(), &settings)?;
    let launch_result = hooks
        .launch_codex(&app_dir, options.debug_port, &settings.codex_extra_args)
        .await;
    let process_ids = codex_elves_core::watcher::find_codex_processes();
    let mut activated = false;
    #[cfg(windows)]
    {
        for process_id in &process_ids {
            if codex_elves_core::windows_activate_process_window(*process_id) {
                activated = true;
                break;
            }
        }
    }
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.activate_existing_codex",
        json!({
            "app_dir": app_dir.to_string_lossy(),
            "debug_port": options.debug_port,
            "helper_port": options.helper_port,
            "process_ids": process_ids,
            "activated": activated,
            "launch_ok": launch_result.is_ok(),
            "launch_error": launch_result.as_ref().err().map(|error| error.to_string())
        }),
    );
    launch_result.map(|_| ())
}

fn log_launcher_already_running(debug_port: u16) {
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.already_running",
        json!({
            "guard_port": codex_elves_core::ports::launcher_guard_port(),
            "debug_port": debug_port
        }),
    );
}

async fn notify_manager_when_update_available() -> anyhow::Result<bool> {
    let settings = codex_elves_core::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    if !update_prompt_enabled(&settings) {
        return Ok(false);
    }
    let update =
        codex_elves_core::update::check_for_update(codex_elves_core::version::VERSION).await?;
    if !update.update_available {
        return Ok(false);
    }
    open_manager_with_update_prompt()?;
    Ok(true)
}

fn update_prompt_enabled(settings: &codex_elves_core::settings::BackendSettings) -> bool {
    settings.github_release_update_prompt_enabled
}

fn open_manager_with_update_prompt() -> anyhow::Result<()> {
    let manager_path = manager_exe_path();
    let mut command = std::process::Command::new(&manager_path);
    command.arg("--show-update");
    #[cfg(windows)]
    {
        command.creation_flags(codex_elves_core::windows_create_no_window());
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))
}

fn parse_launch_options<I, S>(args: I) -> LaunchOptions
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut options = LaunchOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_ref() {
            "--app-path" => {
                if let Some(value) = iter.next() {
                    let value = value.as_ref().trim();
                    if !value.is_empty() {
                        options.app_dir = Some(PathBuf::from(value));
                    }
                }
            }
            "--debug-port" => {
                if let Some(value) = iter.next() {
                    if let Ok(port) = value.as_ref().parse::<u16>() {
                        options.debug_port = port;
                    }
                }
            }
            "--helper-port" => {
                if let Some(value) = iter.next() {
                    if let Ok(port) = value.as_ref().parse::<u16>() {
                        options.helper_port = port;
                    }
                }
            }
            _ => {}
        }
    }
    options
}

#[async_trait::async_trait(?Send)]
impl LaunchHooks for LauncherHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&std::path::Path>,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<std::path::PathBuf> {
        self.core.resolve_app_dir(app_dir, settings)
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        self.core.select_debug_port(requested)
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        self.core.select_helper_port(requested)
    }

    async fn load_settings(&self) -> anyhow::Result<codex_elves_core::settings::BackendSettings> {
        self.core.load_settings().await
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        let home = codex_elves_core::codex_home::default_codex_home_dir();
        let sync = tokio::task::spawn_blocking(move || {
            codex_elves_data::run_provider_sync_with_target_guarded(Some(&home), None)
        })
        .await
        .map_err(|error| anyhow::anyhow!("provider sync task failed: {error}"))?;
        match sync.status {
            codex_elves_data::ProviderSyncStatus::Disabled
            | codex_elves_data::ProviderSyncStatus::Synced
            | codex_elves_data::ProviderSyncStatus::Partial => Ok(()),
            codex_elves_data::ProviderSyncStatus::Skipped
            | codex_elves_data::ProviderSyncStatus::Blocked
            | codex_elves_data::ProviderSyncStatus::RecoveryRequired
            | codex_elves_data::ProviderSyncStatus::Failed => {
                anyhow::bail!("{}", sync.message)
            }
        }
    }

    async fn ensure_active_relay_stream_idle_timeout(
        &self,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core
            .ensure_active_relay_stream_idle_timeout(settings)
            .await
    }

    async fn apply_active_relay_profile(
        &self,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.apply_active_relay_profile(settings).await
    }

    async fn ensure_computer_use_config(
        &self,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.ensure_computer_use_config(settings).await
    }

    async fn ensure_plugin_marketplace_config(
        &self,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.ensure_plugin_marketplace_config(settings).await
    }

    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()> {
        self.core.start_helper(helper_port).await
    }

    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        extra_args: &[String],
    ) -> anyhow::Result<codex_elves_core::launcher::CodexLaunch> {
        self.core
            .launch_codex(app_dir, debug_port, extra_args)
            .await
    }

    async fn bridge_context(
        &self,
        debug_port: u16,
        app_dir: &Path,
    ) -> anyhow::Result<Option<BridgeContext>> {
        self.runtime.set_debug_port(debug_port);
        *self.app_dir.lock().unwrap() = Some(app_dir.to_path_buf());
        Ok(Some(BridgeContext::core_with_data_and_app_dir(
            self.runtime.clone(),
            self.data.clone(),
            app_dir.to_path_buf(),
        )))
    }

    async fn inject_bridge(
        &self,
        debug_port: u16,
        helper_port: u16,
        ctx: BridgeContext,
    ) -> anyhow::Result<()> {
        inject_with_context(
            debug_port,
            helper_port,
            ctx,
            self.runtime.clone(),
            self.bridge_runtime.clone(),
        )
        .await
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.core.inject(debug_port, helper_port).await
    }

    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let runtime = self.runtime.clone();
        let data = self.data.clone();
        let app_dir = self.app_dir.clone();
        let bridge_runtime = self.bridge_runtime.clone();
        let bridge_repair_notify = self.bridge_repair_notify.clone();
        let task = tokio::spawn(async move {
            let mut delay = codex_elves_core::launcher::bridge_watchdog_delay(
                codex_elves_core::launcher::BridgeWatchdogStatus::Healthy,
            );
            loop {
                let trigger = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = tokio::time::sleep(delay) => "interval",
                    _ = bridge_repair_notify.notified() => "requested",
                };
                let runtime = runtime.clone();
                let data = data.clone();
                let app_dir = app_dir.clone();
                let bridge_runtime = bridge_runtime.clone();
                let outcome = codex_elves_core::launcher::check_and_reinject_bridge_status_with(
                    debug_port,
                    helper_port,
                    move || {
                        let runtime = runtime.clone();
                        let data = data.clone();
                        let app_dir = app_dir.clone();
                        let bridge_runtime = bridge_runtime.clone();
                        async move {
                            let app_dir = app_dir.lock().unwrap().clone().ok_or_else(|| {
                                anyhow::anyhow!("launcher app dir is not configured")
                            })?;
                            runtime.set_debug_port(debug_port);
                            let ctx = BridgeContext::core_with_data_and_app_dir(
                                runtime.clone(),
                                data.clone(),
                                app_dir,
                            );
                            inject_with_context(
                                debug_port,
                                helper_port,
                                ctx,
                                runtime,
                                bridge_runtime,
                            )
                            .await
                        }
                    },
                )
                .await;
                let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                    "bridge.watchdog_check",
                    json!({
                        "debug_port": debug_port,
                        "helper_port": helper_port,
                        "trigger": trigger,
                        "outcome": format!("{outcome:?}")
                    }),
                );
                delay = codex_elves_core::launcher::bridge_watchdog_delay(outcome);
            }
        });
        if let Some(runtime) = self
            .bridge_watchdog
            .lock()
            .await
            .replace(LauncherBridgeWatchdogRuntime { shutdown, task })
        {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
        Ok(())
    }

    async fn start_computer_use_guard_watchdog(
        &self,
        settings: &codex_elves_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.start_computer_use_guard_watchdog(settings).await
    }

    async fn write_status(&self, status: &str) {
        self.core.write_status(status).await;
    }

    async fn wait_for_codex_exit(
        &self,
        launch: &codex_elves_core::launcher::CodexLaunch,
    ) -> anyhow::Result<()> {
        self.core.wait_for_codex_exit(launch).await
    }

    async fn shutdown_helper(&self, helper_port: u16) {
        if let Some(runtime) = self.bridge_watchdog.lock().await.take() {
            let _ = runtime.shutdown.send(());
            let _ = runtime.task.await;
        }
        let bridge_runtime = { self.bridge_runtime.lock().await.take() };
        if let Some(runtime) = bridge_runtime {
            runtime.shutdown().await;
        }
        self.core.shutdown_helper(helper_port).await;
    }

    async fn terminate_codex(&self, launch: &codex_elves_core::launcher::CodexLaunch) {
        self.core.terminate_codex(launch).await;
    }
}

#[cfg(test)]
#[derive(Clone)]
struct TaskBoardCatalogTestSeam {
    candidate_db_paths: Vec<PathBuf>,
    reader: Arc<
        dyn Fn(Vec<PathBuf>) -> anyhow::Result<codex_elves_data::LocalSessionCatalog> + Send + Sync,
    >,
}

#[cfg(test)]
impl std::fmt::Debug for TaskBoardCatalogTestSeam {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskBoardCatalogTestSeam")
            .field("candidate_db_paths", &self.candidate_db_paths)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct LauncherDataService {
    db_path: PathBuf,
    #[cfg(test)]
    task_board_catalog_test_seam: Option<TaskBoardCatalogTestSeam>,
}

impl Default for LauncherDataService {
    fn default() -> Self {
        Self {
            db_path: default_codex_db_path(),
            #[cfg(test)]
            task_board_catalog_test_seam: None,
        }
    }
}

#[async_trait::async_trait]
impl BridgeDataService for LauncherDataService {
    async fn delete(&self, session: SessionRef) -> anyhow::Result<DeleteResult> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_elves_data::delete_local_from_paths(db_paths, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("delete task failed: {error}"))
    }

    async fn export_markdown(&self, session: SessionRef) -> anyhow::Result<ExportResult> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_elves_data::export_markdown_from_paths(db_paths, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("export markdown task failed: {error}"))
    }

    async fn thread_usage_history(&self, session: SessionRef) -> anyhow::Result<Value> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_elves_data::codex_thread_usage_history_from_paths(db_paths, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("thread usage history task failed: {error}"))
    }

    async fn thread_usage_summary(&self, session: SessionRef) -> anyhow::Result<Value> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_elves_data::codex_thread_usage_summary_from_paths(db_paths, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("thread usage summary task failed: {error}"))
    }

    async fn find_archived_thread_by_title(
        &self,
        title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.find_archived_thread_by_title(&title))
            .await
            .map_err(|error| anyhow::anyhow!("archived lookup task failed: {error}"))
    }

    async fn move_thread_workspace(
        &self,
        session: SessionRef,
        target_cwd: String,
    ) -> anyhow::Result<Value> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_elves_data::move_codex_thread_workspace_from_paths(
                db_paths,
                &session,
                &target_cwd,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("move thread workspace task failed: {error}"))
    }

    async fn thread_sort_key(&self, session: SessionRef) -> anyhow::Result<Value> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.codex_thread_sort_key(&session))
            .await
            .map_err(|error| anyhow::anyhow!("thread sort key task failed: {error}"))
    }

    async fn thread_sort_keys(&self, sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.codex_thread_sort_keys(&sessions))
            .await
            .map_err(|error| anyhow::anyhow!("thread sort keys task failed: {error}"))
    }

    async fn task_board_session_catalog(&self) -> anyhow::Result<TaskBoardSessionCatalog> {
        let db_paths = self.candidate_db_paths();
        #[cfg(test)]
        let test_reader = self
            .task_board_catalog_test_seam
            .as_ref()
            .map(|seam| seam.reader.clone());

        let local_catalog = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(reader) = test_reader {
                return reader(db_paths);
            }

            codex_elves_data::aggregate_local_session_catalog(&db_paths)
                .map_err(anyhow::Error::from)
        })
        .await
        .map_err(|_| anyhow::anyhow!("Task board session catalog worker failed"))?
        .map_err(|_| anyhow::anyhow!("Task board session catalog service is unavailable"))?;

        task_board_catalog_from_local_catalog(local_catalog)
    }
}

impl LauncherDataService {
    fn candidate_db_paths(&self) -> Vec<PathBuf> {
        #[cfg(test)]
        if let Some(seam) = &self.task_board_catalog_test_seam {
            return seam.candidate_db_paths.clone();
        }

        let mut paths = Vec::new();
        for path in codex_elves_core::codex_sqlite::codex_session_db_paths_from_home(
            &codex_elves_core::codex_sqlite::default_codex_home_dir(),
        ) {
            if !paths.iter().any(|candidate| candidate == &path) {
                paths.push(path);
            }
        }
        if !paths.iter().any(|candidate| candidate == &self.db_path) {
            paths.push(self.db_path.clone());
        }
        paths
    }

    fn current_db_path(&self) -> PathBuf {
        self.candidate_db_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.db_path.clone())
    }

    fn storage_adapter(&self) -> codex_elves_data::SQLiteStorageAdapter {
        codex_elves_data::SQLiteStorageAdapter::new(self.current_db_path())
    }
}

fn task_board_catalog_from_local_catalog(
    local_catalog: codex_elves_data::LocalSessionCatalog,
) -> anyhow::Result<TaskBoardSessionCatalog> {
    let mut projects: Vec<TaskBoardCatalogProject> = Vec::new();
    let mut project_indexes: HashMap<String, usize> = HashMap::new();
    let mut sessions: Vec<TaskBoardCatalogSession> = Vec::new();

    for session in local_catalog.sessions {
        let Ok(cwd) = normalize_task_project_cwd(&session.cwd) else {
            continue;
        };
        let updated_at_ms =
            task_board_timestamp_from_bridge_i64(session.updated_at_ms).map_err(|_| {
                anyhow::anyhow!("Task board session catalog contains an invalid timestamp")
            })?;

        if let Some(project_index) = project_indexes.get(&cwd).copied() {
            let project = &mut projects[project_index];
            project.session_count = project
                .session_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Task board project session count is too large"))?;
        } else {
            project_indexes.insert(cwd.clone(), projects.len());
            projects.push(TaskBoardCatalogProject {
                label: task_board_project_label(&cwd),
                cwd: cwd.clone(),
                session_count: 1,
            });
        }

        sessions.push(TaskBoardCatalogSession {
            session_id: session.id,
            title: session.title,
            cwd,
            updated_at_ms,
        });
    }

    let warnings = local_catalog
        .warnings
        .into_iter()
        .map(|warning| match warning {
            codex_elves_data::LocalSessionCatalogWarning::DatabaseReadFailed { count } => {
                Ok(TaskBoardCatalogWarning {
                    code: TaskBoardCatalogWarningCode::CodexDbReadFailed,
                    count: u32::try_from(count).map_err(|_| {
                        anyhow::anyhow!("Task board database failure count is too large")
                    })?,
                })
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(TaskBoardSessionCatalog {
        projects,
        sessions,
        warnings,
    })
}

fn task_board_project_label(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches(|character| character == '\\' || character == '/');
    trimmed
        .rsplit(|character| character == '\\' || character == '/')
        .find(|component| !component.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

struct LauncherRuntimeService {
    debug_port: Mutex<u16>,
    websocket_url: Mutex<Option<String>>,
    user_scripts: UserScriptManager,
}

impl LauncherRuntimeService {
    fn new(debug_port: u16, user_scripts: UserScriptManager) -> Self {
        Self {
            debug_port: Mutex::new(debug_port),
            websocket_url: Mutex::new(None),
            user_scripts,
        }
    }

    fn set_debug_port(&self, debug_port: u16) {
        *self.debug_port.lock().unwrap() = debug_port;
    }

    fn set_websocket_url(&self, websocket_url: &str) {
        *self.websocket_url.lock().unwrap() = Some(websocket_url.to_string());
    }
}

#[async_trait::async_trait]
impl BridgeRuntimeService for LauncherRuntimeService {
    async fn user_script_inventory(&self) -> anyhow::Result<Value> {
        self.user_scripts.inventory()
    }

    async fn set_user_scripts_enabled(&self, enabled: bool) -> anyhow::Result<Value> {
        self.user_scripts.set_global_enabled(enabled)?;
        self.user_scripts.inventory()
    }

    async fn set_user_script_enabled(&self, key: String, enabled: bool) -> anyhow::Result<Value> {
        self.user_scripts.set_script_enabled(&key, enabled)?;
        self.user_scripts.inventory()
    }

    async fn delete_user_script(&self, key: String) -> anyhow::Result<Value> {
        self.user_scripts.delete_user_script(&key)?;
        self.user_scripts.inventory()
    }

    async fn reload_user_scripts(&self) -> anyhow::Result<Value> {
        let bundle = self.user_scripts.build_enabled_bundle()?;
        let websocket_url = self.websocket_url.lock().unwrap().clone();
        if let Some(websocket_url) = websocket_url.filter(|_| !bundle.trim().is_empty()) {
            codex_elves_core::bridge::evaluate_script(&websocket_url, &bundle).await?;
        }
        self.user_scripts.inventory()
    }

    async fn open_devtools(&self) -> anyhow::Result<Value> {
        let debug_port = *self.debug_port.lock().unwrap();
        let targets = codex_elves_core::cdp::list_targets(debug_port).await?;
        let target = codex_elves_core::cdp::pick_page_target(&targets)?;
        let url = codex_elves_core::routes::devtools_url(debug_port, &target.id);
        open_url(&url)?;
        Ok(json!({
            "status": "ok",
            "target_id": target.id,
            "url": url
        }))
    }

    async fn open_manager(&self) -> anyhow::Result<Value> {
        let manager_path = manager_exe_path();
        #[cfg(windows)]
        {
            std::process::Command::new(&manager_path)
                .creation_flags(codex_elves_core::windows_create_no_window())
                .spawn()
                .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))?;
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new(&manager_path)
                .spawn()
                .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))?;
        }
        Ok(json!({
            "status": "ok",
            "path": manager_path.to_string_lossy()
        }))
    }

    async fn backend_status(&self) -> anyhow::Result<Value> {
        Ok(
            json!({"status": "ok", "message": "后端已连接", "version": codex_elves_core::version::VERSION}),
        )
    }

    async fn repair_backend(&self) -> anyhow::Result<Value> {
        self.backend_status().await
    }

    async fn install_renderer_features(&self) -> anyhow::Result<Value> {
        let websocket_url = self
            .websocket_url
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No renderer target configured"))?;
        codex_elves_core::bridge::evaluate_script(
            &websocket_url,
            codex_elves_core::assets::renderer_features_script(),
        )
        .await?;
        let user_bundle = self.user_scripts.build_enabled_bundle().unwrap_or_default();
        if !user_bundle.trim().is_empty() {
            codex_elves_core::bridge::evaluate_script(&websocket_url, &user_bundle).await?;
        }
        Ok(json!({
            "status": "ok",
            "build": codex_elves_core::assets::DIAGNOSTIC_BUILD_ID
        }))
    }

    async fn codex_model_catalog(&self) -> anyhow::Result<Value> {
        Ok(codex_elves_core::model_catalog::read_codex_model_catalog().await)
    }

    async fn upstream_worktree_status(&self) -> anyhow::Result<Value> {
        Ok(codex_elves_core::upstream_worktree::status_response())
    }

    async fn upstream_worktree_defaults(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_elves_core::upstream_worktree::defaults_response(
            &payload,
        ))
    }

    async fn upstream_worktree_prepare(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_elves_core::upstream_worktree::prepare_response(
            &payload,
        ))
    }

    async fn upstream_worktree_create(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_elves_core::upstream_worktree::create_response(
            &payload,
        ))
    }
}

async fn inject_with_context(
    debug_port: u16,
    helper_port: u16,
    ctx: BridgeContext,
    runtime: Arc<LauncherRuntimeService>,
    bridge_runtime: Arc<tokio::sync::Mutex<Option<codex_elves_core::bridge::BridgeRuntime>>>,
) -> anyhow::Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match try_inject_with_context(
            debug_port,
            helper_port,
            ctx.clone(),
            runtime.clone(),
            bridge_runtime.clone(),
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("ChatGPT/Codex injection failed")))
}

async fn try_inject_with_context(
    debug_port: u16,
    helper_port: u16,
    ctx: BridgeContext,
    runtime: Arc<LauncherRuntimeService>,
    bridge_runtime: Arc<tokio::sync::Mutex<Option<codex_elves_core::bridge::BridgeRuntime>>>,
) -> anyhow::Result<()> {
    let targets = codex_elves_core::cdp::list_targets(debug_port).await?;
    let target = codex_elves_core::cdp::pick_injectable_codex_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("selected CDP target has no websocket URL"))?;
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "bridge.inject_target_selected",
        json!({
            "debug_port": debug_port,
            "helper_port": helper_port,
            "target_id": target.id,
            "target_title": target.title,
            "target_url": target.url,
            "target_count": targets.len()
        }),
    );
    runtime.set_websocket_url(websocket_url);
    let settings = codex_elves_core::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    let script =
        codex_elves_core::assets::bootstrap_injection_script_with_settings(helper_port, &settings);
    let mut bridge_runtime = bridge_runtime.lock().await;
    if let Some(previous_runtime) = bridge_runtime.take() {
        previous_runtime.shutdown().await;
    }
    let installed_runtime = codex_elves_core::bridge::install_bridge(
        websocket_url,
        codex_elves_core::bridge::BRIDGE_BINDING_NAME,
        Arc::new(move |path, payload| {
            let ctx = ctx.clone();
            Box::pin(async move {
                Ok(codex_elves_core::routes::handle_bridge_request(ctx, &path, payload).await)
            })
        }),
        &[script],
    )
    .await?;
    *bridge_runtime = Some(installed_runtime);
    Ok(())
}

fn default_codex_db_path() -> PathBuf {
    codex_elves_core::codex_sqlite::codex_session_db_path()
}

fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        codex_elves_core::windows_open_url(url)
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(not(any(windows, target_os = "macos", unix)))]
    {
        let _ = url;
        anyhow::bail!("opening DevTools URL is not supported on this platform")
    }
}

fn manager_exe_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    dir.join(format!(
        "{}{}",
        codex_elves_core::install::MANAGER_BINARY,
        suffix
    ))
}

fn default_user_script_manager() -> UserScriptManager {
    codex_elves_core::user_scripts::default_user_script_manager()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_elves_data::{
        LocalSessionCatalog, LocalSessionCatalogEntry, LocalSessionCatalogWarning,
    };
    use std::sync::mpsc;

    fn task_board_catalog_test_service(
        candidate_db_paths: Vec<PathBuf>,
        reader: impl Fn(Vec<PathBuf>) -> anyhow::Result<LocalSessionCatalog> + Send + Sync + 'static,
    ) -> LauncherDataService {
        LauncherDataService {
            db_path: PathBuf::from("C:/unused/default-state.sqlite"),
            task_board_catalog_test_seam: Some(TaskBoardCatalogTestSeam {
                candidate_db_paths,
                reader: Arc::new(reader),
            }),
        }
    }

    #[tokio::test]
    async fn task_board_session_catalog_returns_empty_success_for_missing_candidates() {
        let service = task_board_catalog_test_service(
            vec![PathBuf::from("C:/test/missing-state.sqlite")],
            |_| {
                Ok(LocalSessionCatalog {
                    sessions: Vec::new(),
                    warnings: Vec::new(),
                })
            },
        );

        let catalog = service.task_board_session_catalog().await.unwrap();

        assert!(catalog.projects.is_empty());
        assert!(catalog.sessions.is_empty());
        assert!(catalog.warnings.is_empty());
    }

    #[tokio::test]
    async fn task_board_session_catalog_normalizes_projects_counts_sessions_and_labels() {
        let service = task_board_catalog_test_service(Vec::new(), |_| {
            Ok(LocalSessionCatalog {
                sessions: vec![
                    LocalSessionCatalogEntry {
                        id: "session-one".to_string(),
                        title: "First".to_string(),
                        cwd: " C:/Workspace/Project/../Project ".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(42),
                    },
                    LocalSessionCatalogEntry {
                        id: "session-two".to_string(),
                        title: "Second".to_string(),
                        cwd: "c:\\WORKSPACE\\project".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(11),
                    },
                    LocalSessionCatalogEntry {
                        id: "session-three".to_string(),
                        title: "Third".to_string(),
                        cwd: "D:/Another".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: None,
                    },
                ],
                warnings: Vec::new(),
            })
        });

        let catalog = service.task_board_session_catalog().await.unwrap();

        assert_eq!(
            catalog
                .sessions
                .iter()
                .map(|session| {
                    (
                        session.session_id.as_str(),
                        session.title.as_str(),
                        session.cwd.as_str(),
                        session.updated_at_ms,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("session-one", "First", "C:\\workspace\\project", Some(42)),
                ("session-two", "Second", "C:\\workspace\\project", Some(11)),
                ("session-three", "Third", "D:\\another", None),
            ]
        );
        assert_eq!(
            catalog
                .projects
                .iter()
                .map(|project| {
                    (
                        project.cwd.as_str(),
                        project.label.as_str(),
                        project.session_count,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("C:\\workspace\\project", "project", 2),
                ("D:\\another", "another", 1),
            ]
        );
    }

    #[tokio::test]
    async fn task_board_session_catalog_accepts_zero_and_js_safe_max_timestamps() {
        let max_safe_timestamp =
            i64::try_from(codex_elves_core::task_board::TASK_BOARD_MAX_SAFE_INTEGER).unwrap();
        let service = task_board_catalog_test_service(Vec::new(), move |_| {
            Ok(LocalSessionCatalog {
                sessions: vec![
                    LocalSessionCatalogEntry {
                        id: "zero-timestamp".to_string(),
                        title: "Zero".to_string(),
                        cwd: "C:/workspace/zero".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(0),
                    },
                    LocalSessionCatalogEntry {
                        id: "max-timestamp".to_string(),
                        title: "Max".to_string(),
                        cwd: "C:/workspace/max".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(max_safe_timestamp),
                    },
                ],
                warnings: Vec::new(),
            })
        });

        let catalog = service.task_board_session_catalog().await.unwrap();

        assert_eq!(catalog.sessions[0].updated_at_ms, Some(0));
        assert_eq!(
            catalog.sessions[1].updated_at_ms,
            Some(codex_elves_core::task_board::TASK_BOARD_MAX_SAFE_INTEGER)
        );
    }

    #[tokio::test]
    async fn task_board_session_catalog_rejects_negative_timestamp_without_sensitive_text() {
        let sensitive_db_path = "C:/Users/tester/.codex/state.sqlite";
        let sensitive_rollout_path = "C:/Users/tester/.codex/sessions/private.jsonl";
        let invalid_timestamp = -1_i64;
        let service =
            task_board_catalog_test_service(vec![PathBuf::from(sensitive_db_path)], move |_| {
                Ok(LocalSessionCatalog {
                    sessions: vec![LocalSessionCatalogEntry {
                        id: "negative-timestamp".to_string(),
                        title: "Negative".to_string(),
                        cwd: "C:/workspace/negative".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(invalid_timestamp),
                    }],
                    warnings: Vec::new(),
                })
            });

        let error = service.task_board_session_catalog().await.unwrap_err();
        let message = error.to_string();

        assert_eq!(
            message,
            "Task board session catalog contains an invalid timestamp"
        );
        assert!(!message.contains(sensitive_db_path));
        assert!(!message.contains(sensitive_rollout_path));
        assert!(!message.contains(&invalid_timestamp.to_string()));
    }

    #[tokio::test]
    async fn task_board_session_catalog_rejects_timestamp_above_js_safe_max_without_sensitive_text()
    {
        let sensitive_db_path = "C:/Users/tester/.codex/state.sqlite";
        let sensitive_rollout_path = "C:/Users/tester/.codex/sessions/private.jsonl";
        let invalid_timestamp =
            i64::try_from(codex_elves_core::task_board::TASK_BOARD_MAX_SAFE_INTEGER).unwrap() + 1;
        let service =
            task_board_catalog_test_service(vec![PathBuf::from(sensitive_db_path)], move |_| {
                Ok(LocalSessionCatalog {
                    sessions: vec![LocalSessionCatalogEntry {
                        id: "unsafe-timestamp".to_string(),
                        title: "Unsafe".to_string(),
                        cwd: "C:/workspace/unsafe".to_string(),
                        model_provider: "ignored".to_string(),
                        updated_at_ms: Some(invalid_timestamp),
                    }],
                    warnings: Vec::new(),
                })
            });

        let error = service.task_board_session_catalog().await.unwrap_err();
        let message = error.to_string();

        assert_eq!(
            message,
            "Task board session catalog contains an invalid timestamp"
        );
        assert!(!message.contains(sensitive_db_path));
        assert!(!message.contains(sensitive_rollout_path));
        assert!(!message.contains(&invalid_timestamp.to_string()));
    }

    #[tokio::test]
    async fn task_board_session_catalog_preserves_only_aggregate_database_warning_counts() {
        let sensitive_db_path = "C:/Users/tester/.codex/state.sqlite";
        let sensitive_rollout_path = "C:/Users/tester/.codex/sessions/secret.jsonl";
        let service =
            task_board_catalog_test_service(vec![PathBuf::from(sensitive_db_path)], |_| {
                Ok(LocalSessionCatalog {
                    sessions: Vec::new(),
                    warnings: vec![LocalSessionCatalogWarning::DatabaseReadFailed { count: 2 }],
                })
            });

        let catalog = service.task_board_session_catalog().await.unwrap();
        let serialized = serde_json::to_string(&catalog).unwrap();

        assert_eq!(
            serialized,
            r#"{"projects":[],"sessions":[],"warnings":[{"code":"codex_db_read_failed","count":2}]}"#
        );
        assert!(!serialized.contains(sensitive_db_path));
        assert!(!serialized.contains(sensitive_rollout_path));
    }

    #[tokio::test]
    async fn task_board_session_catalog_returns_a_path_free_error_when_all_reads_fail() {
        let sensitive_db_path = "C:/Users/tester/.codex/state.sqlite";
        let sensitive_rollout_path = "C:/Users/tester/.codex/sessions/secret.jsonl";
        let service = task_board_catalog_test_service(
            vec![PathBuf::from(sensitive_db_path)],
            move |_| {
                anyhow::bail!(
                    "failed to read database {sensitive_db_path} for rollout {sensitive_rollout_path}"
                )
            },
        );

        let error = service.task_board_session_catalog().await.unwrap_err();
        let message = error.to_string();

        assert_eq!(message, "Task board session catalog service is unavailable");
        assert!(!message.contains(sensitive_db_path));
        assert!(!message.contains(sensitive_rollout_path));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task_board_session_catalog_uses_blocking_worker_and_passes_every_candidate_path() {
        let caller_thread = std::thread::current().id();
        let candidate_db_paths = vec![
            PathBuf::from("C:/test/state.sqlite"),
            PathBuf::from("C:/test/automation.sqlite"),
        ];
        let expected_paths = candidate_db_paths.clone();
        let (sender, receiver) = mpsc::channel();
        let service = task_board_catalog_test_service(candidate_db_paths, move |paths| {
            sender
                .send((std::thread::current().id(), paths))
                .expect("test receiver should stay available");
            Ok(LocalSessionCatalog {
                sessions: Vec::new(),
                warnings: Vec::new(),
            })
        });

        service.task_board_session_catalog().await.unwrap();
        let (reader_thread, observed_paths) = receiver.recv().unwrap();

        assert_ne!(reader_thread, caller_thread);
        assert_eq!(observed_paths, expected_paths);
    }

    #[test]
    fn parse_launch_options_accepts_manager_forwarded_ports_and_app_path() {
        let options = parse_launch_options([
            "--app-path",
            "C:/Codex/App",
            "--debug-port",
            "9333",
            "--helper-port",
            "57322",
        ]);

        assert_eq!(options.app_dir, Some(PathBuf::from("C:/Codex/App")));
        assert_eq!(options.debug_port, 9333);
        assert_eq!(options.helper_port, 57322);
    }

    #[test]
    fn parse_launch_options_ignores_invalid_ports() {
        let options = parse_launch_options(["--debug-port", "nope", "--helper-port", "70000"]);

        assert_eq!(options.debug_port, LaunchOptions::default().debug_port);
        assert_eq!(options.helper_port, LaunchOptions::default().helper_port);
    }

    #[test]
    fn launcher_uses_single_instance_guard_before_launching() {
        let source = include_str!("main.rs");

        assert!(source.contains("acquire_single_instance_guard(options.debug_port)?"));
        assert!(source.contains("launcher_guard_port"));
        assert!(source.contains("launcher.already_running"));
        assert!(source.contains("request_existing_launcher_bridge_repair()"));
    }

    #[tokio::test]
    async fn launcher_control_request_wakes_existing_bridge_watchdog() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let guard = codex_elves_core::ports::LoopbackPortGuard::listener(listener);
        let notify = Arc::new(tokio::sync::Notify::new());

        assert!(start_launcher_control_listener(&guard, notify.clone()).unwrap());

        assert!(send_launcher_bridge_repair(port));
        tokio::time::timeout(Duration::from_secs(1), notify.notified())
            .await
            .expect("repair notification should reach watchdog");
    }

    #[test]
    fn launcher_hooks_forward_computer_use_guard_methods() {
        let source = include_str!("main.rs");

        assert!(source.contains("async fn ensure_computer_use_config"));
        assert!(source.contains("self.core.ensure_computer_use_config(settings).await"));
        assert!(source.contains("async fn start_computer_use_guard_watchdog"));
        assert!(source.contains("self.core"));
        assert!(source.contains(".start_computer_use_guard_watchdog(settings)"));
    }

    #[test]
    fn launcher_hooks_use_context_preserving_bridge_watchdog() {
        let source = include_str!("main.rs");
        let function_name = ["async fn start_bridge_", "watchdog"].concat();
        let start = source
            .find(&function_name)
            .expect("bridge watchdog function should exist");
        let next_function = ["\n    async fn start_computer_use_guard_", "watchdog"].concat();
        let end = source[start..]
            .find(&next_function)
            .map(|offset| start + offset)
            .expect("bridge watchdog function should have a following function");
        let watchdog = &source[start..end];
        let watchdog_status_call = ["check_and_reinject_bridge_status", "_with"].concat();
        let bridge_context = ["BridgeContext::core_with_data", "_and_app_dir"].concat();
        let contextual_injection = ["inject_with_", "context("].concat();

        assert!(watchdog.contains(&watchdog_status_call));
        assert!(watchdog.contains(&bridge_context));
        assert!(watchdog.contains(&contextual_injection));
        assert!(watchdog.contains("bridge_repair_notify.notified()"));
    }

    #[test]
    fn existing_launcher_activation_does_not_replace_active_bridge() {
        let source = include_str!("main.rs");
        let start = source
            .find("async fn activate_existing_codex_app")
            .expect("activation function should exist");
        let end = source[start..]
            .find("\nfn log_launcher_already_running")
            .map(|offset| start + offset)
            .expect("activation function should have a following function");
        let activation = &source[start..end];
        let helper_start = [".start_", "helper("].concat();
        let injection = [".ensure_", "injection("].concat();
        let watchdog = [".start_bridge_", "watchdog("].concat();

        assert!(!activation.contains(&helper_start));
        assert!(!activation.contains(&injection));
        assert!(!activation.contains(&watchdog));
    }

    #[test]
    fn manager_update_prompt_uses_sidecar_manager_binary_name() {
        let path = manager_exe_path();

        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(codex_elves_core::install::MANAGER_BINARY))
        );
    }

    #[test]
    fn update_prompt_setting_defaults_on_and_can_be_disabled() {
        let mut settings = codex_elves_core::settings::BackendSettings::default();
        assert!(update_prompt_enabled(&settings));

        settings.github_release_update_prompt_enabled = false;
        assert!(!update_prompt_enabled(&settings));
    }
}
