#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result};
use codex_elves_core::launcher::{
    DefaultLaunchHooks, LaunchHooks, LaunchOptions, launch_and_inject_with_hooks,
};
use codex_elves_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_elves_core::routes::{BridgeContext, BridgeDataService, BridgeRuntimeService};
use codex_elves_core::user_scripts::UserScriptManager;
use serde_json::{Value, json};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct LauncherHooks {
    core: Arc<DefaultLaunchHooks>,
    data: Arc<LauncherDataService>,
    runtime: Arc<LauncherRuntimeService>,
    app_dir: Arc<Mutex<Option<PathBuf>>>,
    bridge_watchdog: Arc<tokio::sync::Mutex<Option<LauncherBridgeWatchdogRuntime>>>,
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
            bridge_watchdog: Arc::new(tokio::sync::Mutex::new(None)),
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
    let Some(_guard) = acquire_single_instance_guard(options.debug_port)? else {
        activate_existing_codex_app(&options).await?;
        return Ok(());
    };
    tokio::spawn(async {
        let _ = notify_manager_when_update_available().await;
    });
    let hooks = LauncherHooks::default();
    let handle = launch_and_inject_with_hooks(options, &hooks).await?;
    handle.wait_for_codex_exit().await?;
    Ok(())
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
    if settings.enhancements_enabled {
        hooks.start_helper(options.helper_port).await?;
    }
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
    let injection_ready = if settings.enhancements_enabled {
        hooks
            .ensure_injection(options.debug_port, options.helper_port, &app_dir)
            .await
    } else {
        false
    };
    if injection_ready {
        hooks
            .start_bridge_watchdog(options.debug_port, options.helper_port)
            .await?;
        hooks.write_status("running").await;
    } else if settings.enhancements_enabled {
        hooks.write_status("running_degraded").await;
    }
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "launcher.activate_existing_codex",
        json!({
            "app_dir": app_dir.to_string_lossy(),
            "debug_port": options.debug_port,
            "helper_port": options.helper_port,
            "process_ids": process_ids,
            "activated": activated,
            "injection_ready": injection_ready,
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
    let update =
        codex_elves_core::update::check_for_update(codex_elves_core::version::VERSION).await?;
    if !update.update_available {
        return Ok(false);
    }
    open_manager_with_update_prompt()?;
    Ok(true)
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
        let _ =
            tokio::task::spawn_blocking(move || codex_elves_data::run_provider_sync(Some(&home)))
                .await
                .map_err(|error| anyhow::anyhow!("provider sync task failed: {error}"))?;
        Ok(())
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
        inject_with_context(debug_port, helper_port, ctx, self.runtime.clone()).await
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.core.inject(debug_port, helper_port).await
    }

    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let runtime = self.runtime.clone();
        let data = self.data.clone();
        let app_dir = self.app_dir.clone();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = interval.tick() => {
                        let runtime = runtime.clone();
                        let data = data.clone();
                        let app_dir = app_dir.clone();
                        let _ = codex_elves_core::launcher::check_and_reinject_bridge_with(
                            debug_port,
                            helper_port,
                            move || {
                                let runtime = runtime.clone();
                                let data = data.clone();
                                let app_dir = app_dir.clone();
                                async move {
                                    let app_dir = app_dir
                                        .lock()
                                        .unwrap()
                                        .clone()
                                        .ok_or_else(|| anyhow::anyhow!("launcher app dir is not configured"))?;
                                    runtime.set_debug_port(debug_port);
                                    let ctx = BridgeContext::core_with_data_and_app_dir(
                                        runtime.clone(),
                                        data.clone(),
                                        app_dir,
                                    );
                                    inject_with_context(debug_port, helper_port, ctx, runtime).await
                                }
                            },
                        )
                        .await;
                    }
                }
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
        self.core.shutdown_helper(helper_port).await;
    }

    async fn terminate_codex(&self, launch: &codex_elves_core::launcher::CodexLaunch) {
        self.core.terminate_codex(launch).await;
    }
}

#[derive(Debug, Clone)]
struct LauncherDataService {
    db_path: PathBuf,
    backup_dir: PathBuf,
}

impl Default for LauncherDataService {
    fn default() -> Self {
        Self {
            db_path: default_codex_db_path(),
            backup_dir: codex_elves_core::paths::default_app_state_dir().join("backups"),
        }
    }
}

#[async_trait::async_trait]
impl BridgeDataService for LauncherDataService {
    async fn delete(&self, session: SessionRef) -> anyhow::Result<DeleteResult> {
        let db_paths = self.candidate_db_paths();
        let backup_store = codex_elves_data::BackupStore::new(self.backup_dir.clone());
        tokio::task::spawn_blocking(move || {
            codex_elves_data::delete_local_from_paths(db_paths, backup_store, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("delete task failed: {error}"))
    }

    async fn undo(&self, undo_token: String) -> anyhow::Result<DeleteResult> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.undo(&undo_token))
            .await
            .map_err(|error| anyhow::anyhow!("undo task failed: {error}"))
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
        let backup_store = codex_elves_data::BackupStore::new(self.backup_dir.clone());
        tokio::task::spawn_blocking(move || {
            codex_elves_data::codex_thread_usage_history_from_paths(
                db_paths,
                backup_store,
                &session,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("thread usage history task failed: {error}"))
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
        let backup_store = codex_elves_data::BackupStore::new(self.backup_dir.clone());
        tokio::task::spawn_blocking(move || {
            codex_elves_data::move_codex_thread_workspace_from_paths(
                db_paths,
                backup_store,
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
}

impl LauncherDataService {
    fn candidate_db_paths(&self) -> Vec<PathBuf> {
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
        codex_elves_data::SQLiteStorageAdapter::new(
            self.current_db_path(),
            codex_elves_data::BackupStore::new(self.backup_dir.clone()),
        )
    }
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
) -> anyhow::Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match try_inject_with_context(debug_port, helper_port, ctx.clone(), runtime.clone()).await {
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
) -> anyhow::Result<()> {
    let targets = codex_elves_core::cdp::list_targets(debug_port).await?;
    let target = codex_elves_core::cdp::pick_injectable_codex_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("selected CDP target has no websocket URL"))?;
    runtime.set_websocket_url(websocket_url);
    let settings = codex_elves_core::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    let script =
        codex_elves_core::assets::bootstrap_injection_script_with_settings(helper_port, &settings);
    codex_elves_core::bridge::install_bridge(
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
    .await
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

        assert!(
            source.contains(
                "async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16)"
            )
        );
        assert!(source.contains("check_and_reinject_bridge_with"));
        assert!(source.contains("BridgeContext::core_with_data_and_app_dir"));
        assert!(
            source.contains("inject_with_context(debug_port, helper_port, ctx, runtime).await")
        );
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
}
