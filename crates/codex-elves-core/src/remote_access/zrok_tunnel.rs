use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tar::Archive;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use super::now_ms;

const ZROK_VERSION: &str = "2.0.8";
const ZROK_ARCHIVE_URL: &str =
    "https://github.com/openziti/zrok/releases/download/v2.0.8/zrok_2.0.8_windows_amd64.tar.gz";
const ZROK_ARCHIVE_SHA256: &str =
    "661a12c56c9610c71fc859fdf161873a5833c762b45c37ff5241e682b5017917";
const ZROK_PROGRAM_SHA256: &str =
    "771d63d500f64c2a0858ef44439d2dbe75db51d1d288bf77a69cdc56e535be3a";
const ZROK_API_ENDPOINT: &str = "https://api-v2.zrok.io";
const ZROK_CONFIG_FILE: &str = "remote-access-zrok.json";
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PROGRAM_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PUBLIC_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const RETRY_MS: u64 = 30_000;
const PUBLIC_RETRY_MS: u64 = 5_000;
const PUBLIC_RECHECK_MS: u64 = 30_000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const NAME_QUERY_RETRY_DELAY: Duration = Duration::from_millis(500);
const PUBLIC_TIMEOUT: Duration = Duration::from_secs(15);
const SHARE_RELEASE_TIMEOUT: Duration = Duration::from_secs(10);
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZrokState {
    Missing,
    Checking,
    Downloading,
    Unbound,
    NameRequired,
    Stopped,
    Starting,
    Connecting,
    Ready,
    Backoff,
    Error,
}

impl ZrokState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Checking => "checking",
            Self::Downloading => "downloading",
            Self::Unbound => "unbound",
            Self::NameRequired => "name_required",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Backoff => "retry_wait",
            Self::Error => "error",
        }
    }
}

enum ProcessEvent {
    Diagnostic(String),
    ReadError(String),
}

#[derive(Default)]
struct DownloadProgress {
    downloaded_bytes: AtomicU64,
    total_bytes: AtomicU64,
}

impl DownloadProgress {
    fn reset(&self) {
        self.downloaded_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (u64, u64) {
        (
            self.downloaded_bytes.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed),
        )
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ZrokConfig {
    fixed_name: String,
}

struct EnvironmentStatus {
    bound: bool,
    detected_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZrokName {
    name: String,
    namespace_token: String,
    reserved: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZrokShareList {
    #[serde(default)]
    shares: Vec<ZrokShare>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZrokShare {
    #[serde(default, rename = "envZId")]
    env_zid: String,
    #[serde(default)]
    frontend_endpoints: Vec<String>,
    #[serde(default)]
    share_mode: String,
    #[serde(default)]
    share_token: String,
}

#[derive(Deserialize)]
struct ZrokEnvironment {
    ziti_identity: String,
}

struct CommandOutput {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[async_trait::async_trait]
trait EnvironmentCommands: Sync {
    fn verify_program(&self, program: &Path) -> anyhow::Result<()> {
        verify_program_sha256(program)
    }

    async fn run(
        &self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> anyhow::Result<CommandOutput>;
}

struct SystemEnvironmentCommands;

#[async_trait::async_trait]
impl EnvironmentCommands for SystemEnvironmentCommands {
    async fn run(
        &self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> anyhow::Result<CommandOutput> {
        run_command(program, args, timeout).await
    }
}

enum EnableOutcome {
    Enabled,
    AlreadyEnabled,
}

pub(super) struct ZrokTunnelService {
    state_dir: PathBuf,
    origin: Arc<RwLock<Option<String>>>,
    child: Option<Child>,
    #[cfg(windows)]
    child_job: Option<std::os::windows::io::OwnedHandle>,
    download: Option<tokio::task::JoinHandle<anyhow::Result<PathBuf>>>,
    environment_check: Option<tokio::task::JoinHandle<anyhow::Result<EnvironmentStatus>>>,
    public_check: Option<tokio::task::JoinHandle<anyhow::Result<Value>>>,
    download_progress: Arc<DownloadProgress>,
    readers: Vec<tokio::task::JoinHandle<()>>,
    events: Option<mpsc::UnboundedReceiver<ProcessEvent>>,
    program_path: Option<PathBuf>,
    target: Option<String>,
    fixed_name: String,
    state: ZrokState,
    installed: bool,
    bound: bool,
    environment_checked: bool,
    error: Option<String>,
    last_diagnostic: Option<String>,
    retry_at_ms: u64,
    checked_at_ms: u64,
    next_public_check_at_ms: u64,
}

impl ZrokTunnelService {
    pub(super) fn new(state_dir: impl Into<PathBuf>, origin: Arc<RwLock<Option<String>>>) -> Self {
        let state_dir = state_dir.into();
        cleanup_obsolete_quick_tunnel_state(&state_dir);
        let fixed_name = load_config(&state_dir)
            .map(|config| config.fixed_name)
            .unwrap_or_default();
        let program_path = find_verified_program(&state_dir);
        let installed = program_path.is_some();
        Self {
            state_dir,
            origin,
            child: None,
            #[cfg(windows)]
            child_job: None,
            download: None,
            environment_check: None,
            public_check: None,
            download_progress: Arc::new(DownloadProgress::default()),
            readers: Vec::new(),
            events: None,
            program_path,
            target: None,
            fixed_name,
            state: if installed {
                ZrokState::Checking
            } else {
                ZrokState::Missing
            },
            installed,
            bound: false,
            environment_checked: false,
            error: None,
            last_diagnostic: None,
            retry_at_ms: 0,
            checked_at_ms: 0,
            next_public_check_at_ms: 0,
        }
    }

    pub(super) async fn reconcile(&mut self, web_running: bool, target: &str) {
        self.refresh_download().await;
        self.refresh_environment().await;
        self.refresh_process().await;

        if self.installed
            && !self.environment_checked
            && self.environment_check.is_none()
            && self.child.is_none()
            && self.state != ZrokState::Error
        {
            self.begin_environment_check();
        }
        if !web_running {
            self.stop(false).await;
            self.state = self.passive_state();
            return;
        }
        if let Err(error) = validate_target(target) {
            self.stop(true).await;
            self.error = Some(format!("{error:#}"));
            self.state = ZrokState::Error;
            return;
        }
        if self.child.is_some()
            && !self.environment_checked
            && !target_requires_restart(self.target.as_deref(), target)
        {
            self.refresh_public_access().await;
            return;
        }
        if target_requires_restart(self.target.as_deref(), target) {
            self.stop(false).await;
            self.target = Some(target.to_owned());
            self.retry_at_ms = 0;
            self.checked_at_ms = 0;
        } else if self.target.is_none() {
            self.target = Some(target.to_owned());
        }
        if !self.installed || !self.environment_checked || !self.bound || self.fixed_name.is_empty()
        {
            self.stop(false).await;
            self.state = self.passive_state();
            return;
        }
        if self.child.is_some() {
            self.refresh_public_access().await;
            return;
        }
        if self.state == ZrokState::Error && self.retry_at_ms == 0 {
            return;
        }
        if now_ms() < self.retry_at_ms {
            self.state = ZrokState::Backoff;
            return;
        }
        let Some(program) = self.program_path.clone() else {
            self.installed = false;
            self.state = ZrokState::Missing;
            return;
        };
        let fixed_name = self.fixed_name.clone();
        if let Err(error) = self.start(&program, target, &fixed_name).await {
            self.fail_with_backoff(error);
        }
    }

    pub(super) async fn begin_download(&mut self) -> anyhow::Result<()> {
        self.refresh_download().await;
        if self.installed {
            return Ok(());
        }
        if self
            .download
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            self.state = ZrokState::Downloading;
            return Ok(());
        }
        let directory = self.state_dir.clone();
        let progress = Arc::clone(&self.download_progress);
        progress.reset();
        self.download = Some(tokio::spawn(async move {
            ensure_managed_zrok(&directory, &progress).await
        }));
        self.error = None;
        self.state = ZrokState::Downloading;
        Ok(())
    }

    pub(super) async fn register(&self, email: &str) -> anyhow::Result<Value> {
        anyhow::ensure!(self.installed, "请先下载 zrok 程序");
        register_email(email).await
    }

    pub(super) async fn start_public(
        &mut self,
        token: Option<&str>,
        fixed_name: &str,
        web_running: bool,
        target: &str,
    ) -> anyhow::Result<()> {
        self.start_public_with_commands(
            token,
            fixed_name,
            web_running,
            target,
            &SystemEnvironmentCommands,
        )
        .await
    }

    pub(super) async fn start_for_primary(&mut self, target: &str) -> anyhow::Result<String> {
        self.start_for_primary_with_commands(target, &SystemEnvironmentCommands)
            .await
    }

    async fn start_for_primary_with_commands(
        &mut self,
        target: &str,
        commands: &impl EnvironmentCommands,
    ) -> anyhow::Result<String> {
        self.refresh_download().await;
        self.refresh_process().await;
        if self.child.is_none() || target_requires_restart(self.target.as_deref(), target) {
            // 主入口切换是显式启动，不能沿用后台巡检留下的错误或退避状态。
            let fixed_name = self.fixed_name.clone();
            self.start_public_with_commands(None, &fixed_name, true, target, commands)
                .await?;
        }
        self.current_origin().context("zrok 地址未就绪")
    }

    async fn start_public_with_commands(
        &mut self,
        token: Option<&str>,
        fixed_name: &str,
        web_running: bool,
        target: &str,
        commands: &impl EnvironmentCommands,
    ) -> anyhow::Result<()> {
        self.refresh_download().await;
        let result = self
            .prepare_and_start_public(token, fixed_name, web_running, target, commands)
            .await;
        if let Err(error) = &result {
            self.record_start_error(error);
        }
        result
    }

    async fn prepare_and_start_public(
        &mut self,
        token: Option<&str>,
        fixed_name: &str,
        web_running: bool,
        target: &str,
        commands: &impl EnvironmentCommands,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.installed, "请先下载 zrok 程序");
        anyhow::ensure!(web_running, "请先启用远程会话网页");
        validate_target(target)?;
        let fixed_name = normalize_fixed_name(fixed_name)?;
        let program = self
            .program_path
            .clone()
            .context("未找到已校验的 zrok 程序")?;

        commands.verify_program(&program)?;
        self.ensure_environment_bound(&program, token, commands)
            .await?;
        ensure_reserved_name(&program, &fixed_name, commands).await?;
        save_config(
            &self.state_dir,
            &ZrokConfig {
                fixed_name: fixed_name.clone(),
            },
        )?;
        self.stop(false).await;
        self.error = None;
        self.retry_at_ms = 0;
        self.state = ZrokState::Starting;
        self.fixed_name = fixed_name.clone();
        self.target = Some(target.to_owned());
        self.start(&program, target, &fixed_name).await
    }

    async fn ensure_environment_bound(
        &mut self,
        program: &Path,
        token: Option<&str>,
        commands: &impl EnvironmentCommands,
    ) -> anyhow::Result<()> {
        // 启动必须使用本次读取的结果，旧探测即使已完成也不能覆盖新的绑定。
        if let Some(task) = self.environment_check.take() {
            task.abort();
            let _ = task.await;
        }
        self.environment_checked = false;
        let bound = read_environment_bound(program, commands).await?;
        self.bound = bound;
        self.environment_checked = true;
        if bound {
            return Ok(());
        }
        let token = token
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .context("请输入 zrok token")?;
        match enable_environment(program, token, commands).await? {
            EnableOutcome::Enabled => self.bound = true,
            EnableOutcome::AlreadyEnabled => {
                self.environment_checked = false;
                let bound = read_environment_bound(program, commands)
                    .await
                    .context("zrok 环境已被启用，但无法确认当前绑定状态")?;
                self.bound = bound;
                self.environment_checked = true;
                anyhow::ensure!(
                    bound,
                    "zrok 提示环境已启用，但当前状态仍未绑定，请重新点击“启动公网”"
                );
            }
        }
        Ok(())
    }

    fn record_start_error(&mut self, error: &anyhow::Error) {
        self.error = Some(format!("{error:#}"));
        self.retry_at_ms = 0;
        if self.child.is_none() {
            self.state = ZrokState::Error;
        }
    }

    pub(super) async fn check(&mut self) -> anyhow::Result<Value> {
        self.refresh_process().await;
        self.test_public_access().await
    }

    pub(super) fn status(&self) -> Value {
        let public_url = self.configured_origin().unwrap_or_default();
        let running = self.child.is_some();
        let (downloaded_bytes, download_total_bytes) = self.download_progress.snapshot();
        let download_percent = if download_total_bytes > 0 {
            downloaded_bytes
                .min(download_total_bytes)
                .saturating_mul(100)
                / download_total_bytes
        } else {
            0
        };
        json!({
            "state":self.state.as_str(),
            "running":running,
            "connected":running && self.state == ZrokState::Ready,
            "processId":self.child.as_ref().and_then(Child::id),
            "installed":self.installed,
            "bound":self.bound,
            "fixedName":self.fixed_name,
            "version":self.installed.then(|| format!("v{ZROK_VERSION}")),
            "downloadedBytes":downloaded_bytes,
            "downloadTotalBytes":download_total_bytes,
            "downloadPercent":download_percent,
            "target":self.target,
            "publicUrl":public_url,
            "managedVersion":ZROK_VERSION,
            "error":self.error,
            "checkedAtMs":self.checked_at_ms,
            "retryAtMs":self.retry_at_ms,
        })
    }

    async fn refresh_download(&mut self) {
        if self
            .download
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            self.state = ZrokState::Downloading;
            return;
        }
        let Some(task) = self.download.take() else {
            return;
        };
        match task.await {
            Ok(Ok(program)) => {
                self.installed = true;
                self.program_path = Some(program);
                self.environment_checked = false;
                self.bound = false;
                self.error = None;
                self.state = ZrokState::Checking;
                self.begin_environment_check();
            }
            Ok(Err(error)) => {
                self.installed = false;
                self.program_path = None;
                self.error = Some(format!("{error:#}"));
                self.state = ZrokState::Error;
            }
            Err(error) => {
                self.installed = false;
                self.program_path = None;
                self.error = Some(format!("zrok 下载任务未完成：{error}"));
                self.state = ZrokState::Error;
            }
        }
    }

    fn begin_environment_check(&mut self) {
        let Some(program) = self.program_path.clone() else {
            return;
        };
        self.state = ZrokState::Checking;
        self.environment_check = Some(tokio::spawn(
            async move { inspect_environment(&program).await },
        ));
    }

    async fn refresh_environment(&mut self) {
        if self
            .environment_check
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            if self.child.is_none() && self.state != ZrokState::Error {
                self.state = ZrokState::Checking;
            }
            return;
        }
        let Some(task) = self.environment_check.take() else {
            return;
        };
        match task.await {
            Ok(Ok(status)) => {
                self.environment_checked = true;
                self.bound = status.bound;
                if self.fixed_name.is_empty()
                    && let Some(name) = status.detected_name
                {
                    self.fixed_name = name;
                    if let Err(error) = save_config(
                        &self.state_dir,
                        &ZrokConfig {
                            fixed_name: self.fixed_name.clone(),
                        },
                    ) {
                        self.error = Some(format!("{error:#}"));
                    }
                }
                if self.error.is_none() && self.child.is_none() {
                    self.state = self.passive_state();
                }
            }
            Ok(Err(error)) => {
                self.environment_checked = false;
                self.record_start_error(&error);
            }
            Err(error) => {
                self.environment_checked = false;
                self.record_start_error(
                    &anyhow::Error::new(error).context("无法确认 zrok 绑定状态"),
                );
            }
        }
    }

    async fn test_public_access(&mut self) -> anyhow::Result<Value> {
        let origin = self
            .current_origin()
            .context("zrok 公网尚未启动，请先点击“启动公网”")?;
        anyhow::ensure!(self.child.is_some(), "zrok 未运行，请重新启动公网");
        self.cancel_public_check().await;
        let result = probe_public_access(&origin).await;
        self.refresh_process().await;
        anyhow::ensure!(
            self.child.is_some() && self.current_origin().as_deref() == Some(origin.as_str()),
            "公网验证期间 zrok 已停止，当前连接不可用"
        );
        self.record_public_access_result(&result);
        result
    }

    async fn refresh_public_access(&mut self) {
        if self
            .public_check
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            let result = self
                .public_check
                .take()
                .expect("finished public check should exist")
                .await
                .context("zrok 公网验证任务未完成")
                .and_then(|result| result);
            self.record_public_access_result(&result);
        }
        if self.public_check.is_none()
            && now_ms() >= self.next_public_check_at_ms
            && let Some(origin) = self.current_origin()
        {
            self.public_check = Some(tokio::spawn(
                async move { probe_public_access(&origin).await },
            ));
        }
    }

    fn record_public_access_result(&mut self, result: &anyhow::Result<Value>) {
        self.checked_at_ms = now_ms();
        match result {
            Ok(_) => {
                self.error = None;
                self.state = ZrokState::Ready;
                self.next_public_check_at_ms = self.checked_at_ms + PUBLIC_RECHECK_MS;
            }
            Err(error) => {
                self.error = Some(format!("公网访问尚未验证通过，将自动重试：{error:#}"));
                self.state = ZrokState::Connecting;
                self.next_public_check_at_ms = self.checked_at_ms + PUBLIC_RETRY_MS;
            }
        }
    }

    async fn cancel_public_check(&mut self) {
        if let Some(task) = self.public_check.take() {
            task.abort();
            let _ = task.await;
        }
    }

    async fn start(
        &mut self,
        program: &Path,
        target: &str,
        fixed_name: &str,
    ) -> anyhow::Result<()> {
        verify_program_sha256(program)?;
        release_reclaimable_share(program, fixed_name).await?;
        let name_selection = format!("public:{fixed_name}");
        let mut command = Command::new(program);
        command
            .args([
                "share",
                "public",
                target,
                "--name-selection",
                &name_selection,
                "--open",
                "--headless",
                "--force-local",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        let mut child = command.spawn().context("无法启动 zrok 公网服务")?;
        #[cfg(windows)]
        let job = child_job(&child, "zrok")?;
        let stdout = child.stdout.take().context("无法读取 zrok 标准输出")?;
        let stderr = child.stderr.take().context("无法读取 zrok 错误输出")?;
        let (sender, receiver) = mpsc::unbounded_channel();
        self.readers = vec![
            spawn_output_reader(stdout, sender.clone()),
            spawn_output_reader(stderr, sender),
        ];
        self.events = Some(receiver);
        self.child = Some(child);
        #[cfg(windows)]
        {
            self.child_job = Some(job);
        }
        self.target = Some(target.to_owned());
        *self
            .origin
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(origin_for_name(fixed_name));
        self.state = ZrokState::Connecting;
        self.error = None;
        self.last_diagnostic = None;
        self.checked_at_ms = 0;
        self.next_public_check_at_ms = 0;
        Ok(())
    }

    async fn refresh_process(&mut self) {
        self.consume_events();
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                self.child = None;
                #[cfg(windows)]
                {
                    self.child_job = None;
                }
                self.cancel_public_check().await;
                for task in self.readers.drain(..) {
                    let _ = tokio::time::timeout(Duration::from_millis(250), task).await;
                }
                self.consume_events();
                self.events = None;
                self.clear_origin();
                let diagnostic = self.last_diagnostic.take();
                let message = if let Some(diagnostic) = diagnostic {
                    format!("zrok 已退出（{status}）：{diagnostic}")
                } else {
                    format!("zrok 已退出（{status}）")
                };
                if status.code() == Some(2) {
                    self.error = Some(format!("{message}。请核对 token 和固定公网名称。"));
                    self.state = ZrokState::Error;
                } else {
                    self.error = Some(format!("{message}。30 秒后自动重试。"));
                    self.retry_at_ms = now_ms() + RETRY_MS;
                    self.state = ZrokState::Backoff;
                }
            }
            Err(error) => {
                let message = anyhow::Error::new(error).context("无法读取 zrok 进程状态");
                self.stop(false).await;
                self.fail_with_backoff(message);
            }
        }
    }

    fn consume_events(&mut self) {
        let Some(receiver) = self.events.as_mut() else {
            return;
        };
        while let Ok(event) = receiver.try_recv() {
            match event {
                ProcessEvent::Diagnostic(diagnostic) => {
                    self.last_diagnostic = Some(diagnostic);
                }
                ProcessEvent::ReadError(error) => {
                    self.last_diagnostic = Some(error.clone());
                    self.error = Some(error);
                }
            }
        }
    }

    async fn stop(&mut self, clear_target: bool) {
        self.consume_events();
        self.cancel_public_check().await;
        self.next_public_check_at_ms = 0;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        #[cfg(windows)]
        {
            self.child_job = None;
        }
        for mut task in self.readers.drain(..) {
            if tokio::time::timeout(Duration::from_millis(250), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        self.consume_events();
        self.events = None;
        self.last_diagnostic = None;
        self.clear_origin();
        if clear_target {
            self.target = None;
        }
    }

    fn fail_with_backoff(&mut self, error: anyhow::Error) {
        self.clear_origin();
        self.error = Some(format!("{error:#}"));
        self.retry_at_ms = now_ms() + RETRY_MS;
        self.state = ZrokState::Backoff;
    }

    fn passive_state(&self) -> ZrokState {
        if self.error.is_some() && (self.state == ZrokState::Error || !self.environment_checked) {
            ZrokState::Error
        } else if self
            .download
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            ZrokState::Downloading
        } else if !self.installed {
            ZrokState::Missing
        } else if !self.environment_checked
            || self
                .environment_check
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            ZrokState::Checking
        } else if !self.bound {
            ZrokState::Unbound
        } else if self.fixed_name.is_empty() {
            ZrokState::NameRequired
        } else {
            ZrokState::Stopped
        }
    }

    fn configured_origin(&self) -> Option<String> {
        valid_fixed_name(&self.fixed_name).then(|| origin_for_name(&self.fixed_name))
    }

    fn current_origin(&self) -> Option<String> {
        self.origin
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn clear_origin(&self) {
        *self
            .origin
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

fn cleanup_obsolete_quick_tunnel_state(state_dir: &Path) {
    for file in [
        "remote-access-cloudflared-quick.yml",
        "remote-access-quick-tunnel-cooldown.json",
    ] {
        let path = state_dir.join(file);
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
    let directory = state_dir.join("tools/cloudflared");
    match std::fs::remove_dir_all(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn load_config(state_dir: &Path) -> anyhow::Result<ZrokConfig> {
    let path = state_dir.join(ZROK_CONFIG_FILE);
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("zrok 公网配置损坏"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ZrokConfig::default()),
        Err(error) => Err(error).context("无法读取 zrok 公网配置"),
    }
}

fn save_config(state_dir: &Path, config: &ZrokConfig) -> anyhow::Result<()> {
    crate::settings::atomic_write(
        &state_dir.join(ZROK_CONFIG_FILE),
        &serde_json::to_vec(config)?,
    )
    .context("无法保存 zrok 固定公网名称")
}

fn normalize_fixed_name(value: &str) -> anyhow::Result<String> {
    let value = value.trim().to_ascii_lowercase();
    anyhow::ensure!(
        valid_fixed_name(&value),
        "固定公网名称需为 3–63 位小写字母、数字或连字符，且首尾必须是字母或数字"
    );
    Ok(value)
}

fn valid_fixed_name(value: &str) -> bool {
    (3..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn origin_for_name(name: &str) -> String {
    format!("https://{name}.shares.zrok.io")
}

fn target_requires_restart(current: Option<&str>, requested: &str) -> bool {
    current.is_some_and(|target| target != requested)
}

fn validate_target(target: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(target).context("zrok 回源地址无效")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "zrok 回源地址必须是无凭据、无查询参数的 HTTP(S) 地址"
    );
    Ok(())
}

fn managed_program_path(state_dir: &Path) -> PathBuf {
    state_dir
        .join("tools/zrok")
        .join(format!("zrok2-{ZROK_VERSION}-{ZROK_PROGRAM_SHA256}.exe"))
}

fn find_verified_program(state_dir: &Path) -> Option<PathBuf> {
    let managed = managed_program_path(state_dir);
    if verify_program_sha256(&managed).is_ok() {
        return Some(managed);
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join("zrok2.exe"))
        .find(|candidate| verify_program_sha256(candidate).is_ok())
}

async fn ensure_managed_zrok(
    state_dir: &Path,
    progress: &DownloadProgress,
) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        cfg!(all(target_os = "windows", target_arch = "x86_64")),
        "zrok 自动下载仅支持 Windows x64"
    );
    let _guard = DOWNLOAD_LOCK.lock().await;
    if let Some(program) = find_verified_program(state_dir) {
        if let Ok(metadata) = std::fs::metadata(&program) {
            progress
                .downloaded_bytes
                .store(metadata.len(), Ordering::Relaxed);
            progress
                .total_bytes
                .store(metadata.len(), Ordering::Relaxed);
        }
        return Ok(program);
    }

    let path = managed_program_path(state_dir);
    let parent = path.parent().context("zrok 保存路径无效")?;
    std::fs::create_dir_all(parent).context("无法创建 zrok 工具目录")?;
    let temporary_path = parent.join(format!(
        ".zrok-{ZROK_VERSION}-{}.download",
        uuid::Uuid::new_v4().simple()
    ));
    let mut temporary = TemporaryDownload::create(temporary_path)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .user_agent("CodexElves-zrok")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()?;
    let mut response = client
        .get(ZROK_ARCHIVE_URL)
        .send()
        .await
        .context("无法下载官方 zrok，请检查网络后重试")?
        .error_for_status()
        .context("官方 zrok 下载暂不可用，请稍后重试")?;
    ensure_official_download(response.url())?;
    let content_length = response.content_length().unwrap_or(0);
    anyhow::ensure!(
        content_length <= MAX_ARCHIVE_BYTES,
        "官方 zrok 压缩包过大，已取消下载"
    );
    progress
        .total_bytes
        .store(content_length, Ordering::Relaxed);
    let mut size = 0u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("官方 zrok 下载中断，请稍后重试")?
    {
        size = size
            .checked_add(chunk.len() as u64)
            .context("官方 zrok 文件大小溢出")?;
        anyhow::ensure!(
            size <= MAX_ARCHIVE_BYTES,
            "官方 zrok 压缩包过大，已取消下载"
        );
        temporary
            .file
            .as_mut()
            .expect("temporary download file should be open")
            .write_all(&chunk)
            .context("无法写入 zrok 临时文件")?;
        hasher.update(&chunk);
        progress.downloaded_bytes.store(size, Ordering::Relaxed);
    }
    temporary
        .file
        .as_mut()
        .expect("temporary download file should be open")
        .sync_all()
        .context("无法完成 zrok 临时文件写入")?;
    anyhow::ensure!(size > 0, "官方 zrok 下载内容为空");
    if content_length == 0 {
        progress.total_bytes.store(size, Ordering::Relaxed);
    }
    anyhow::ensure!(
        format!("{:x}", hasher.finalize()) == ZROK_ARCHIVE_SHA256,
        "官方 zrok 压缩包的 SHA-256 不匹配，已拒绝保存和运行"
    );
    let archive_path = temporary.path.clone();
    let target_path = path.clone();
    tokio::task::spawn_blocking(move || extract_program(&archive_path, &target_path))
        .await
        .context("zrok 解压任务未完成")??;
    temporary.remove()?;
    verify_program_sha256(&path)?;
    Ok(path)
}

fn extract_program(archive_path: &Path, target_path: &Path) -> anyhow::Result<()> {
    let archive_file = File::open(archive_path).context("无法读取 zrok 压缩包")?;
    let mut archive = Archive::new(GzDecoder::new(archive_file));
    for entry in archive.entries().context("无法解析 zrok 压缩包")? {
        let entry = entry.context("无法读取 zrok 压缩包条目")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("zrok 压缩包条目路径无效")?;
        if path.file_name().and_then(|name| name.to_str()) != Some("zrok2.exe") {
            continue;
        }
        let size = entry.size();
        anyhow::ensure!(
            size > 0 && size <= MAX_PROGRAM_BYTES,
            "zrok2.exe 文件大小无效"
        );
        let mut bytes = Vec::with_capacity(size as usize);
        entry
            .take(MAX_PROGRAM_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("无法解压 zrok2.exe")?;
        anyhow::ensure!(
            !bytes.is_empty() && bytes.len() as u64 <= MAX_PROGRAM_BYTES,
            "zrok2.exe 文件大小无效"
        );
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == ZROK_PROGRAM_SHA256,
            "zrok2.exe 的 SHA-256 不匹配，已拒绝保存和运行"
        );
        crate::settings::atomic_write(target_path, &bytes).context("无法保存 zrok2.exe")?;
        return Ok(());
    }
    anyhow::bail!("官方 zrok 压缩包中缺少 zrok2.exe")
}

fn ensure_official_download(url: &reqwest::Url) -> anyhow::Result<()> {
    anyhow::ensure!(
        url.scheme() == "https"
            && matches!(
                url.host_str(),
                Some(
                    "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            ),
        "zrok 下载不是官方 HTTPS 资产地址，已拒绝使用"
    );
    Ok(())
}

fn verify_program_sha256(path: &Path) -> anyhow::Result<()> {
    let bytes = read_limited_file(path, MAX_PROGRAM_BYTES).context("受管 zrok 缺失或无法读取")?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == ZROK_PROGRAM_SHA256,
        "zrok 程序校验失败，已拒绝运行"
    );
    Ok(())
}

fn read_limited_file(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
        "zrok 程序文件大小无效"
    );
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= limit,
        "zrok 程序文件大小无效"
    );
    Ok(bytes)
}

async fn inspect_environment(program: &Path) -> anyhow::Result<EnvironmentStatus> {
    let bound = read_environment_bound(program, &SystemEnvironmentCommands).await?;
    let detected_name = if bound {
        detect_reserved_name(program).await.ok().flatten()
    } else {
        None
    };
    Ok(EnvironmentStatus {
        bound,
        detected_name,
    })
}

async fn read_environment_bound(
    program: &Path,
    commands: &impl EnvironmentCommands,
) -> anyhow::Result<bool> {
    let status = commands
        .run(program, &["status"], Duration::from_secs(10))
        .await?;
    anyhow::ensure!(
        status.success,
        "无法读取 zrok 本机环境状态：{}",
        command_diagnostic(&status)
    );
    status_output_is_bound(&format!("{}\n{}", status.stdout, status.stderr))
}

fn status_output_is_bound(output: &str) -> anyhow::Result<bool> {
    let output = status_without_ansi_styles(output);
    if output
        .lines()
        .any(|line| line.contains("Account Token") && line.contains("<<SET>>"))
    {
        return Ok(true);
    }
    if output.contains("To create a local environment use the zrok2 enable command.") {
        return Ok(false);
    }
    anyhow::bail!("无法确认 zrok 本机环境是否已启用，请重新点击“启动公网”")
}

fn status_without_ansi_styles(output: &str) -> String {
    let mut parts = output.split("\x1b[");
    let mut plain = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        match part.split_once('m') {
            Some((style, text))
                if style
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b';') =>
            {
                plain.push_str(text);
            }
            _ => {
                plain.push_str("\x1b[");
                plain.push_str(part);
            }
        }
    }
    plain
}

async fn detect_reserved_name(program: &Path) -> anyhow::Result<Option<String>> {
    let names = read_reserved_names(program, &SystemEnvironmentCommands).await?;
    Ok(select_reserved_name(&names))
}

async fn read_reserved_names(
    program: &Path,
    commands: &impl EnvironmentCommands,
) -> anyhow::Result<Vec<ZrokName>> {
    // Only this read can be retried; both attempts share the original command deadline.
    tokio::time::timeout(COMMAND_TIMEOUT, async {
        let mut output = commands
            .run(program, &["list", "names", "--json"], COMMAND_TIMEOUT)
            .await?;
        if !output.success && command_diagnostic(&output).contains("TLS handshake timeout") {
            tokio::time::sleep(NAME_QUERY_RETRY_DELAY).await;
            output = commands
                .run(program, &["list", "names", "--json"], COMMAND_TIMEOUT)
                .await?;
        }
        anyhow::ensure!(
            output.success,
            "无法读取 zrok 固定公网名称：{}",
            command_diagnostic(&output)
        );
        serde_json::from_str(&output.stdout).context("zrok 固定公网名称列表格式无效")
    })
    .await
    .context("读取 zrok 固定公网名称超时，请稍后重试")?
}

fn select_reserved_name(names: &[ZrokName]) -> Option<String> {
    names
        .iter()
        .find(|name| {
            name.reserved
                && name.namespace_token == "public"
                && name.name.starts_with("codex-elves-")
                && valid_fixed_name(&name.name)
        })
        .map(|name| name.name.clone())
}

async fn enable_environment(
    program: &Path,
    token: &str,
    commands: &impl EnvironmentCommands,
) -> anyhow::Result<EnableOutcome> {
    anyhow::ensure!(
        !token.chars().any(char::is_whitespace) && (6..=512).contains(&token.len()),
        "zrok token 格式无效"
    );
    let output = commands
        .run(
            program,
            &["enable", token, "--headless", "--description", "CodexElves"],
            COMMAND_TIMEOUT,
        )
        .await?;
    if output.success {
        return Ok(EnableOutcome::Enabled);
    }
    if [&output.stdout, &output.stderr].iter().any(|text| {
        status_without_ansi_styles(text).contains(
            "you already have an enabled environment, zrok2 disable first before you zrok2 enable",
        )
    }) {
        return Ok(EnableOutcome::AlreadyEnabled);
    }
    anyhow::bail!("zrok token 绑定失败：{}", command_diagnostic(&output))
}

async fn ensure_reserved_name(
    program: &Path,
    fixed_name: &str,
    commands: &impl EnvironmentCommands,
) -> anyhow::Result<()> {
    let names = read_reserved_names(program, commands).await?;
    if names
        .iter()
        .any(|name| name.name == fixed_name && name.namespace_token == "public" && name.reserved)
    {
        return Ok(());
    }
    anyhow::ensure!(
        !names
            .iter()
            .any(|name| name.name == fixed_name && name.namespace_token == "public"),
        "该固定公网名称已存在但不是可复用的保留名称"
    );
    let output = commands
        .run(
            program,
            &["create", "name", fixed_name, "--namespace-token", "public"],
            COMMAND_TIMEOUT,
        )
        .await?;
    anyhow::ensure!(
        output.success,
        "固定公网名称注册失败：{}",
        command_diagnostic(&output)
    );
    Ok(())
}

async fn release_reclaimable_share(program: &Path, fixed_name: &str) -> anyhow::Result<()> {
    let env_zid = load_zrok_environment_id()?;
    let shares = list_local_public_shares(program, &env_zid).await?;
    let Some(share_token) =
        reclaimable_share_token(&shares, &env_zid, fixed_name).map(str::to_owned)
    else {
        return Ok(());
    };
    let output = run_command(
        program,
        &["delete", "share", &share_token, "--envzid", &env_zid],
        COMMAND_TIMEOUT,
    )
    .await?;
    if !output.success {
        let remaining = list_local_public_shares(program, &env_zid).await?;
        anyhow::ensure!(
            reclaimable_share_token(&remaining, &env_zid, fixed_name).is_none(),
            "无法释放旧的 zrok 固定公网连接：{}",
            command_diagnostic(&output)
        );
        return Ok(());
    }

    let deadline = Instant::now() + SHARE_RELEASE_TIMEOUT;
    loop {
        let remaining = list_local_public_shares(program, &env_zid).await?;
        if reclaimable_share_token(&remaining, &env_zid, fixed_name).is_none() {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "旧的 zrok 固定公网连接仍未释放，请稍后重试"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn list_local_public_shares(program: &Path, env_zid: &str) -> anyhow::Result<Vec<ZrokShare>> {
    let output = run_command(
        program,
        &[
            "list",
            "shares",
            "--json",
            "--share-mode",
            "public",
            "--env-zid",
            env_zid,
        ],
        COMMAND_TIMEOUT,
    )
    .await?;
    anyhow::ensure!(
        output.success,
        "无法读取本机 zrok 公网连接：{}",
        command_diagnostic(&output)
    );
    let list: ZrokShareList =
        serde_json::from_str(&output.stdout).context("zrok 公网连接列表格式无效")?;
    Ok(list.shares)
}

fn reclaimable_share_token<'a>(
    shares: &'a [ZrokShare],
    env_zid: &str,
    fixed_name: &str,
) -> Option<&'a str> {
    let endpoint = format!("{fixed_name}.shares.zrok.io");
    shares
        .iter()
        .find(|share| {
            share.env_zid == env_zid
                && share.share_mode == "public"
                && !share.share_token.is_empty()
                && share
                    .frontend_endpoints
                    .iter()
                    .any(|candidate| candidate == &endpoint)
        })
        .map(|share| share.share_token.as_str())
}

fn load_zrok_environment_id() -> anyhow::Result<String> {
    let home = directories::BaseDirs::new().context("无法定位当前用户目录")?;
    let path = home.home_dir().join(".zrok2").join("environment.json");
    let bytes = std::fs::read(path).context("无法读取本机 zrok 环境")?;
    anyhow::ensure!(bytes.len() <= 64 * 1024, "本机 zrok 环境文件异常");
    let environment: ZrokEnvironment =
        serde_json::from_slice(&bytes).context("本机 zrok 环境文件格式无效")?;
    let env_zid = environment.ziti_identity.trim();
    anyhow::ensure!(!env_zid.is_empty(), "本机 zrok 环境标识为空");
    Ok(env_zid.to_owned())
}

async fn run_command(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> anyhow::Result<CommandOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .context("zrok 命令执行超时")?
        .context("无法执行 zrok 命令")?;
    Ok(CommandOutput {
        success: output.status.success(),
        code: output.status.code(),
        stdout: limited_text(&output.stdout),
        stderr: limited_text(&output.stderr),
    })
}

fn limited_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_COMMAND_OUTPUT_BYTES)])
        .trim()
        .to_string()
}

fn command_diagnostic(output: &CommandOutput) -> String {
    let text = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    let text = status_without_ansi_styles(text);
    if text.to_ascii_lowercase().contains("tls handshake timeout") {
        return "连接 zrok 控制服务超时（TLS handshake timeout），请稍后重试".into();
    }
    let message = text
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with("goroutine ") && !line.starts_with("runtime stack:"))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let message = message.strip_prefix("panic:").unwrap_or(&message).trim();
    if message.is_empty() {
        output
            .code
            .map_or_else(|| "未知错误".into(), |code| format!("退出码 {code}"))
    } else {
        message.chars().take(600).collect()
    }
}

async fn register_email(email: &str) -> anyhow::Result<Value> {
    let email = email.trim().to_ascii_lowercase();
    anyhow::ensure!(valid_email(&email), "请输入有效邮箱地址");
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .user_agent("CodexElves-zrok")
        .build()?;
    let response = client
        .post(format!("{ZROK_API_ENDPOINT}/api/v2/invite"))
        .header(reqwest::header::CONTENT_TYPE, "application/zrok.v1+json")
        .header(reqwest::header::ACCEPT, "application/zrok.v1+json")
        .json(&json!({"email":email,"inviteToken":""}))
        .send()
        .await
        .context("无法连接 zrok 注册服务")?;
    let status = response.status();
    let body = bounded_response_text(response).await.unwrap_or_default();
    match status.as_u16() {
        201 => Ok(json!({
            "status":"ok",
            "message":"注册邮件已发送，请检查收件箱和垃圾邮件，取得 token 后粘贴到输入框。"
        })),
        400 => anyhow::bail!("该邮箱可能已注册或已有待处理邀请，请检查邮箱后直接使用 token"),
        401 => anyhow::bail!("当前 zrok 服务需要额外邀请凭据，暂不能自动注册"),
        429 => anyhow::bail!("zrok 注册请求过于频繁，请稍后再试"),
        _ => anyhow::bail!(
            "zrok 注册服务返回 HTTP {}{}",
            status.as_u16(),
            if body.trim().is_empty() {
                String::new()
            } else {
                format!("：{}", body.chars().take(300).collect::<String>())
            }
        ),
    }
}

fn valid_email(value: &str) -> bool {
    if value.len() < 3
        || value.len() > 96
        || value.chars().any(char::is_whitespace)
        || value.matches('@').count() != 1
    {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn spawn_output_reader<R>(
    reader: R,
    sender: mpsc::UnboundedSender<ProcessEvent>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Some(diagnostic) = zrok_diagnostic(&line) {
                        let _ = sender.send(ProcessEvent::Diagnostic(diagnostic));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(ProcessEvent::ReadError(format!(
                        "无法读取 zrok 输出：{error}"
                    )));
                    break;
                }
            }
        }
    })
}

fn zrok_diagnostic(text: &str) -> Option<String> {
    let value = text.trim();
    if value.is_empty() {
        return None;
    }
    let lowercase = value.to_ascii_lowercase();
    if ![
        "error",
        "failed",
        "failure",
        "unable",
        "denied",
        "timeout",
        "status 4",
        "status 5",
        "rate limit",
        "\"level\":\"error\"",
    ]
    .iter()
    .any(|needle| lowercase.contains(needle))
    {
        return None;
    }
    Some(value.chars().take(600).collect())
}

fn strict_zrok_origin(url: &reqwest::Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    host.strip_suffix(".shares.zrok.io")
        .is_some_and(valid_fixed_name)
}

async fn probe_public_access(origin: &str) -> anyhow::Result<Value> {
    let url = reqwest::Url::parse(origin).context("zrok 公网地址无效")?;
    anyhow::ensure!(
        strict_zrok_origin(&url),
        "zrok 公网地址不属于严格的 shares.zrok.io Origin"
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PUBLIC_TIMEOUT)
        .build()?;
    let response = client
        .get(origin)
        .header("skip_zrok_interstitial", "1")
        .send()
        .await
        .context("zrok 公网 HTTPS 连接暂不可用")?;
    let http_status = response.status();
    let text = bounded_response_text(response).await?;
    anyhow::ensure!(
        http_status == reqwest::StatusCode::OK,
        "zrok 公网入口返回 HTTP {}",
        http_status.as_u16()
    );
    anyhow::ensure!(
        text.contains("CodexElves") && text.contains("/app.js"),
        "zrok 公网入口未返回 CodexElves 远程网页"
    );
    let protected = client
        .get(format!("{origin}/api/session"))
        .header("skip_zrok_interstitial", "1")
        .send()
        .await
        .context("无法检查 zrok 公网访问授权边界")?;
    anyhow::ensure!(
        protected.status() == reqwest::StatusCode::UNAUTHORIZED,
        "未登录的会话接口返回 HTTP {}，预期为 401",
        protected.status().as_u16()
    );
    Ok(json!({
        "status":"ok",
        "publicUrl":origin,
        "httpStatus":200,
        "authRequired":true,
        "message":"zrok 公网 HTTPS 与网页回源验证通过；未登录访问会话接口返回 401。"
    }))
}

async fn bounded_response_text(mut response: reqwest::Response) -> anyhow::Result<String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len().saturating_add(chunk.len()) <= MAX_PUBLIC_RESPONSE_BYTES,
            "远程响应过大"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(windows)]
pub(super) fn child_job(
    child: &Child,
    name: &str,
) -> anyhow::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::core::PCWSTR;

    let process = child
        .raw_handle()
        .with_context(|| format!("{name} 进程已退出"))?;
    let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .with_context(|| format!("无法创建 {name} 进程生命周期保护"))?;
    let job = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )?;
        AssignProcessToJobObject(handle, HANDLE(process))?;
    }
    Ok(job)
}

struct TemporaryDownload {
    path: PathBuf,
    file: Option<File>,
}

impl TemporaryDownload {
    fn create(path: PathBuf) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("无法创建 zrok 下载临时文件")?;
        Ok(Self {
            path,
            file: Some(file),
        })
    }

    fn remove(mut self) -> anyhow::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            drop(file);
        }
        std::fs::remove_file(&self.path).context("无法清理 zrok 下载临时文件")?;
        self.path = PathBuf::new();
        Ok(())
    }
}

impl Drop for TemporaryDownload {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    const BOUND_STATUS: &str = "Environment:\nAccount Token <<SET>>\nEnvZId <<SET>>";
    const UNBOUND_STATUS: &str = "To create a local environment use the zrok2 enable command.";
    const ALREADY_ENABLED: &str = "[ERROR] you already have an enabled environment, zrok2 disable first before you zrok2 enable";

    struct ScriptedCommands {
        replies: Mutex<VecDeque<(&'static str, anyhow::Result<CommandOutput>)>>,
        calls: Mutex<Vec<String>>,
    }

    impl ScriptedCommands {
        fn new(
            replies: impl IntoIterator<Item = (&'static str, anyhow::Result<CommandOutput>)>,
        ) -> Self {
            Self {
                replies: Mutex::new(replies.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn assert_calls(&self, expected: &[&str]) {
            assert_eq!(*self.calls.lock().unwrap(), expected);
            assert!(self.replies.lock().unwrap().is_empty());
        }
    }

    #[async_trait::async_trait]
    impl EnvironmentCommands for ScriptedCommands {
        fn verify_program(&self, _program: &Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn run(
            &self,
            _program: &Path,
            args: &[&str],
            _timeout: Duration,
        ) -> anyhow::Result<CommandOutput> {
            let (expected, result) = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected environment command");
            assert_eq!(args[0], expected);
            self.calls.lock().unwrap().push(args[0].to_owned());
            result
        }
    }

    fn command_success(stdout: &str) -> anyhow::Result<CommandOutput> {
        Ok(CommandOutput {
            success: true,
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    fn command_failure(stderr: &str) -> anyhow::Result<CommandOutput> {
        Ok(CommandOutput {
            success: false,
            code: Some(1),
            stdout: String::new(),
            stderr: stderr.into(),
        })
    }

    #[test]
    fn command_diagnostic_summarizes_tls_panics() {
        let output = command_failure(
            "panic: client version error accessing api endpoint 'https://api-v2.zrok.io': \
             Post \"https://api-v2.zrok.io/api/v2/clientVersionCheck\": net/http: TLS handshake timeout\n\
             \ngoroutine 1 [running]:\nmain.(*listNamesCommand).run()\n\t/home/runner/work/zrok/listNames.go:43",
        )
        .unwrap();

        let diagnostic = command_diagnostic(&output);
        assert_eq!(
            diagnostic,
            "连接 zrok 控制服务超时（TLS handshake timeout），请稍后重试"
        );
    }

    #[test]
    fn command_diagnostic_keeps_failure_reason_without_panic_stack() {
        let output = command_failure(
            "\x1b[31mpanic: permission denied: name is already reserved\x1b[0m\n\
             \ngoroutine 1 [running]:\nmain.(*listNamesCommand).run()\n\t/home/runner/work/zrok/listNames.go:43",
        )
        .unwrap();

        assert_eq!(
            command_diagnostic(&output),
            "permission denied: name is already reserved"
        );
    }

    #[tokio::test]
    async fn reserved_name_query_retries_tls_timeout_and_reuses_existing_name() {
        let commands = ScriptedCommands::new([
            (
                "list",
                command_failure("panic: net/http: TLS handshake timeout\n\ngoroutine 1 [running]:"),
            ),
            (
                "list",
                command_success(
                    r#"[{"name":"codex-elves-test","namespaceToken":"public","reserved":true}]"#,
                ),
            ),
        ]);

        ensure_reserved_name(Path::new("unused-zrok.exe"), "codex-elves-test", &commands)
            .await
            .unwrap();

        commands.assert_calls(&["list", "list"]);
    }

    #[tokio::test]
    async fn reserved_name_query_stops_after_second_tls_timeout() {
        let commands = ScriptedCommands::new([
            ("list", command_failure("panic: TLS handshake timeout")),
            ("list", command_failure("panic: TLS handshake timeout")),
        ]);

        let error =
            ensure_reserved_name(Path::new("unused-zrok.exe"), "codex-elves-test", &commands)
                .await
                .unwrap_err();

        assert_eq!(
            error.to_string(),
            "无法读取 zrok 固定公网名称：连接 zrok 控制服务超时（TLS handshake timeout），请稍后重试"
        );
        commands.assert_calls(&["list", "list"]);
    }

    #[tokio::test]
    async fn reserved_name_query_does_not_retry_credentials_or_invalid_json() {
        for reply in [
            command_failure("permission denied"),
            command_success("invalid json"),
            Err(anyhow::anyhow!("无法执行 zrok 命令")),
        ] {
            let commands = ScriptedCommands::new([("list", reply)]);

            assert!(
                ensure_reserved_name(Path::new("unused-zrok.exe"), "codex-elves-test", &commands)
                    .await
                    .is_err()
            );
            commands.assert_calls(&["list"]);
        }
    }

    #[tokio::test]
    async fn reserved_name_registration_is_not_retried_after_tls_timeout() {
        let commands = ScriptedCommands::new([
            ("list", command_success("[]")),
            ("create", command_failure("panic: TLS handshake timeout")),
        ]);

        let error =
            ensure_reserved_name(Path::new("unused-zrok.exe"), "codex-elves-test", &commands)
                .await
                .unwrap_err();

        assert!(error.to_string().starts_with("固定公网名称注册失败："));
        assert!(!error.to_string().contains("panic"));
        commands.assert_calls(&["list", "create"]);
    }

    fn test_service() -> ZrokTunnelService {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../temp/zrok-binding-fix-20261017")
            .join(uuid::Uuid::new_v4().simple().to_string());
        let mut service = ZrokTunnelService::new(&directory, Arc::new(RwLock::new(None)));
        service.installed = true;
        service.program_path = Some(directory.join("nonexistent-zrok-test.exe"));
        service.state = ZrokState::Checking;
        service
    }

    async fn confirm_environment(
        service: &mut ZrokTunnelService,
        token: Option<&str>,
        commands: &ScriptedCommands,
    ) -> anyhow::Result<()> {
        let program = service.program_path.clone().unwrap();
        service
            .ensure_environment_bound(&program, token, commands)
            .await
    }

    #[test]
    fn validates_fixed_public_names_and_origins() {
        for value in ["abc", "codex-elves-junxin367", "a1-b2-c3"] {
            assert!(valid_fixed_name(value), "{value}");
            let origin = reqwest::Url::parse(&origin_for_name(value)).unwrap();
            assert!(strict_zrok_origin(&origin), "{origin}");
        }
        for value in [
            "ab",
            "-abc",
            "abc-",
            "ABC",
            "a_b",
            "a.b",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(!valid_fixed_name(value), "{value}");
        }
        for value in [
            "https://alpha.shares.zrok.io.evil.test",
            "https://alpha.shares.zrok.io@evil.test",
            "https://alpha.beta.shares.zrok.io",
            "https://-alpha.shares.zrok.io",
            "https://alpha-.shares.zrok.io",
            "https://alpha.shares.zrok.io/path",
            "https://alpha.shares.zrok.io?next=https://evil.test",
            "http://alpha.shares.zrok.io",
        ] {
            assert!(!strict_zrok_origin(&reqwest::Url::parse(value).unwrap()));
        }
    }

    #[test]
    fn detects_bound_status_without_reading_secrets() {
        assert!(
            status_output_is_bound("Environment:\nAccount Token <<SET>>\nEnvZId <<SET>>").unwrap()
        );
        assert!(
            !status_output_is_bound("To create a local environment use the zrok2 enable command.")
                .unwrap()
        );
        assert!(status_output_is_bound("").is_err());
        assert!(status_output_is_bound("Account Token <<UNSET>>\nEnvZId <<SET>>").is_err());
        assert!(
            !status_output_is_bound(
                "To create a local environment use the \x1b[33;1mzrok2 enable\x1b[0m command."
            )
            .unwrap()
        );
        assert!(
            status_output_is_bound(
                "Environment:\n\x1b[1mAccount Token\x1b[0m <<SET>>\nEnvZId <<SET>>"
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn startup_rechecks_stale_unbound_cache_and_reuses_enabled_environment() {
        let mut service = test_service();
        service.bound = false;
        service.environment_checked = true;
        let commands = ScriptedCommands::new([("status", command_success(BOUND_STATUS))]);

        confirm_environment(&mut service, Some("unused invalid token"), &commands)
            .await
            .unwrap();

        assert!(service.bound && service.environment_checked);
        commands.assert_calls(&["status"]);
    }

    #[tokio::test]
    async fn startup_discards_late_environment_probe_before_binding() {
        let mut service = test_service();
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        service.environment_check = Some(tokio::spawn(async move {
            receiver.await?;
            Ok(EnvironmentStatus {
                bound: false,
                detected_name: None,
            })
        }));
        let commands = ScriptedCommands::new([
            ("status", command_success(UNBOUND_STATUS)),
            ("enable", command_success("environment enabled")),
        ]);

        confirm_environment(&mut service, Some("test-token"), &commands)
            .await
            .unwrap();
        assert!(sender.send(()).is_err(), "old probe must be cancelled");
        service.refresh_environment().await;

        assert!(service.bound && service.environment_checked);
        assert!(service.environment_check.is_none());
        commands.assert_calls(&["status", "enable"]);
    }

    #[tokio::test]
    async fn startup_discards_completed_environment_probe_before_fresh_status() {
        let mut service = test_service();
        service.environment_check = Some(tokio::spawn(async {
            Ok(EnvironmentStatus {
                bound: false,
                detected_name: None,
            })
        }));
        tokio::task::yield_now().await;
        assert!(service.environment_check.as_ref().unwrap().is_finished());
        let commands = ScriptedCommands::new([("status", command_success(BOUND_STATUS))]);

        confirm_environment(&mut service, None, &commands)
            .await
            .unwrap();
        service.refresh_environment().await;

        assert!(service.bound && service.environment_checked);
        commands.assert_calls(&["status"]);
    }

    #[tokio::test]
    async fn startup_enables_once_only_after_confirming_environment_is_unbound() {
        let mut service = test_service();
        service.bound = true;
        service.environment_checked = true;
        let commands = ScriptedCommands::new([
            ("status", command_success(UNBOUND_STATUS)),
            ("enable", command_success("environment enabled")),
        ]);

        confirm_environment(&mut service, Some("test-token"), &commands)
            .await
            .unwrap();

        assert!(service.bound && service.environment_checked);
        commands.assert_calls(&["status", "enable"]);
    }

    #[tokio::test]
    async fn startup_recovers_already_enabled_race_only_after_confirming_bound_status() {
        for message in [
            ALREADY_ENABLED,
            "[ERROR] you already have an enabled environment, \x1b[33;1mzrok2 disable\x1b[0m first before you \x1b[33;1mzrok2 enable\x1b[0m",
        ] {
            let mut service = test_service();
            let commands = ScriptedCommands::new([
                ("status", command_success(UNBOUND_STATUS)),
                ("enable", command_failure(message)),
                ("status", command_success(BOUND_STATUS)),
            ]);

            confirm_environment(&mut service, Some("test-token"), &commands)
                .await
                .unwrap();

            assert!(service.bound && service.environment_checked);
            commands.assert_calls(&["status", "enable", "status"]);
        }
    }

    #[tokio::test]
    async fn startup_rejects_already_enabled_race_if_recheck_is_still_unbound() {
        let mut service = test_service();
        let commands = ScriptedCommands::new([
            ("status", command_success(UNBOUND_STATUS)),
            ("enable", command_failure(ALREADY_ENABLED)),
            ("status", command_success(UNBOUND_STATUS)),
        ]);

        let error = service
            .start_public_with_commands(
                Some("test-token"),
                "codex-elves-test",
                true,
                "http://127.0.0.1:45222",
                &commands,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("当前状态仍未绑定"));
        assert!(!service.bound);
        assert_eq!(service.state.as_str(), "error");
        assert_eq!(service.retry_at_ms, 0);
        commands.assert_calls(&["status", "enable", "status"]);
    }

    #[tokio::test]
    async fn startup_rejects_unknown_status_without_enabling_or_discarding_bound_cache() {
        let mut service = test_service();
        service.bound = true;
        service.environment_checked = true;
        let commands = ScriptedCommands::new([("status", command_success("unexpected output"))]);

        let error = service
            .start_public_with_commands(
                Some("test-token"),
                "codex-elves-test",
                true,
                "http://127.0.0.1:45222",
                &commands,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("无法确认"));
        assert!(service.bound);
        assert!(!service.environment_checked);
        assert_eq!(service.state.as_str(), "error");
        service.reconcile(true, "http://127.0.0.1:45222").await;
        assert_eq!(service.state.as_str(), "error");
        assert!(service.environment_check.is_none());
        commands.assert_calls(&["status"]);
    }

    #[tokio::test]
    async fn startup_does_not_retry_ordinary_enable_errors() {
        for message in [
            "invalid token",
            "network timeout",
            "permission denied",
            "already enabled",
        ] {
            let mut service = test_service();
            let commands = ScriptedCommands::new([
                ("status", command_success(UNBOUND_STATUS)),
                ("enable", command_failure(message)),
            ]);

            let error = service
                .start_public_with_commands(
                    Some("test-token"),
                    "codex-elves-test",
                    true,
                    "http://127.0.0.1:45222",
                    &commands,
                )
                .await
                .unwrap_err();

            assert!(error.to_string().contains(message));
            assert!(!service.bound);
            assert_eq!(service.state.as_str(), "error");
            assert_eq!(service.retry_at_ms, 0);
            commands.assert_calls(&["status", "enable"]);
        }
    }

    #[tokio::test]
    async fn startup_does_not_accept_already_enabled_when_recheck_fails() {
        let mut service = test_service();
        let commands = ScriptedCommands::new([
            ("status", command_success(UNBOUND_STATUS)),
            ("enable", command_failure(ALREADY_ENABLED)),
            ("status", Err(anyhow::anyhow!("simulated status timeout"))),
        ]);

        let error = service
            .start_public_with_commands(
                Some("test-token"),
                "codex-elves-test",
                true,
                "http://127.0.0.1:45222",
                &commands,
            )
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("simulated status timeout"));
        assert!(!service.bound && !service.environment_checked);
        assert_eq!(service.state.as_str(), "error");
        commands.assert_calls(&["status", "enable", "status"]);
    }

    #[tokio::test]
    async fn failed_start_can_be_clicked_again_and_read_environment_again() {
        let mut service = test_service();
        let first = ScriptedCommands::new([(
            "status",
            Err(anyhow::anyhow!("simulated first status failure")),
        )]);
        assert!(
            service
                .start_public_with_commands(
                    Some("test-token"),
                    "codex-elves-test",
                    true,
                    "http://127.0.0.1:45222",
                    &first,
                )
                .await
                .is_err()
        );
        assert_eq!(service.state.as_str(), "error");
        first.assert_calls(&["status"]);
        let second = ScriptedCommands::new([
            ("status", command_success(UNBOUND_STATUS)),
            ("enable", command_success("environment enabled")),
            ("list", Err(anyhow::anyhow!("无法执行 zrok 命令"))),
        ]);

        // 模拟名称查询失败，使第二次启动在绑定完成后结束，不运行真实 zrok。
        let error = service
            .start_public_with_commands(
                Some("test-token"),
                "codex-elves-test",
                true,
                "http://127.0.0.1:45222",
                &second,
            )
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("无法执行 zrok 命令"));
        assert!(service.bound && service.environment_checked);
        assert_eq!(service.state.as_str(), "error");
        second.assert_calls(&["status", "enable", "list"]);
    }

    #[tokio::test]
    async fn background_probe_failure_keeps_last_confirmed_binding_and_exits_checking() {
        let mut service = test_service();
        service.bound = true;
        service.environment_checked = true;
        service.environment_check = Some(tokio::spawn(async {
            Err(anyhow::anyhow!("simulated background status failure"))
        }));
        tokio::task::yield_now().await;

        service.refresh_environment().await;
        service.reconcile(true, "http://127.0.0.1:45222").await;

        assert!(service.bound);
        assert!(!service.environment_checked);
        assert_eq!(service.state.as_str(), "error");
        assert!(service.environment_check.is_none());
    }

    #[tokio::test]
    async fn startup_token_validation_failure_exits_checking_without_enabling() {
        for token in [None, Some("invalid token")] {
            let mut service = test_service();
            let commands = ScriptedCommands::new([("status", command_success(UNBOUND_STATUS))]);

            assert!(
                service
                    .start_public_with_commands(
                        token,
                        "codex-elves-test",
                        true,
                        "http://127.0.0.1:45222",
                        &commands,
                    )
                    .await
                    .is_err()
            );

            assert_eq!(service.state.as_str(), "error");
            assert_eq!(service.retry_at_ms, 0);
            commands.assert_calls(&["status"]);
        }
    }

    #[tokio::test]
    async fn pending_probe_does_not_restore_checking_after_start_validation_error() {
        let mut service = test_service();
        let (_sender, receiver) = tokio::sync::oneshot::channel::<()>();
        service.environment_check = Some(tokio::spawn(async move {
            receiver.await?;
            Ok(EnvironmentStatus {
                bound: false,
                detected_name: None,
            })
        }));
        assert!(
            service
                .start_public(
                    Some("test-token"),
                    "invalid name",
                    true,
                    "http://127.0.0.1:45222",
                )
                .await
                .is_err()
        );

        service.refresh_environment().await;

        assert_eq!(service.state.as_str(), "error");
        let task = service.environment_check.take().unwrap();
        task.abort();
        let _ = task.await;
    }

    #[cfg(windows)]
    async fn test_child() -> Child {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "remote_access::zrok_tunnel::tests::job_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CODEX_ELVES_ZROK_JOB_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        let mut child = command.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if line == "job-child-ready" {
                    return;
                }
            }
            panic!("test child exited before becoming ready");
        })
        .await
        .unwrap();
        child
    }

    #[tokio::test]
    async fn primary_start_rechecks_binding_after_stale_error_or_backoff() {
        for state in [ZrokState::Error, ZrokState::Backoff] {
            let mut service = test_service();
            service.bound = true;
            service.environment_checked = true;
            service.fixed_name = "codex-elves-test".into();
            service.target = Some("http://127.0.0.1:45222".into());
            service.retry_at_ms = if state == ZrokState::Backoff {
                now_ms() + RETRY_MS
            } else {
                0
            };
            service.error = Some("previous TLS handshake timeout".into());
            service.state = state;
            service.reconcile(true, "http://127.0.0.1:45222").await;
            assert!(service.child.is_none());

            let commands = ScriptedCommands::new([("status", command_success(UNBOUND_STATUS))]);
            let error = service
                .start_for_primary_with_commands("http://127.0.0.1:45222", &commands)
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "请输入 zrok token");
            assert!(!service.bound);
            assert_eq!(service.retry_at_ms, 0);
            commands.assert_calls(&["status"]);
        }
    }

    #[tokio::test]
    async fn primary_start_preserves_actual_control_service_error() {
        let mut service = test_service();
        service.fixed_name = "codex-elves-test".into();
        let commands = ScriptedCommands::new([(
            "status",
            Err(anyhow::anyhow!(
                "clientVersionCheck: net/http: TLS handshake timeout"
            )),
        )]);
        let error = service
            .start_for_primary_with_commands("http://127.0.0.1:45222", &commands)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("TLS handshake timeout"));
        assert!(!error.to_string().contains("请先完成绑定"));
        assert_eq!(service.state.as_str(), "error");
        commands.assert_calls(&["status"]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn primary_start_reuses_live_process_and_preserves_it_on_failed_target_change() {
        let mut service = test_service();
        let target = "http://127.0.0.1:45222";
        service.bound = true;
        service.environment_checked = true;
        service.fixed_name = "codex-elves-test".into();
        service.target = Some(target.into());
        service.child = Some(test_child().await);
        service.state = ZrokState::Ready;
        let origin = origin_for_name(&service.fixed_name);
        *service.origin.write().unwrap() = Some(origin.clone());
        let process_id = service.child.as_ref().unwrap().id();

        let commands = ScriptedCommands::new([]);
        assert_eq!(
            service
                .start_for_primary_with_commands(target, &commands)
                .await
                .unwrap(),
            origin
        );
        assert_eq!(service.child.as_ref().unwrap().id(), process_id);
        commands.assert_calls(&[]);

        let commands =
            ScriptedCommands::new([("status", Err(anyhow::anyhow!("TLS handshake timeout")))]);
        assert!(
            service
                .start_for_primary_with_commands("http://127.0.0.1:45223", &commands)
                .await
                .is_err()
        );
        assert_eq!(service.child.as_ref().unwrap().id(), process_id);
        assert_eq!(service.current_origin(), Some(origin));
        commands.assert_calls(&["status"]);
        service.stop(true).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_start_preflight_keeps_current_process_and_origin() {
        let mut service = test_service();
        service.bound = true;
        service.environment_checked = true;
        service.fixed_name = "codex-elves-test".into();
        service.target = Some("http://127.0.0.1:45222".into());
        service.child = Some(test_child().await);
        service.state = ZrokState::Ready;
        service.next_public_check_at_ms = u64::MAX;
        let origin = origin_for_name(&service.fixed_name);
        *service.origin.write().unwrap() = Some(origin.clone());
        let process_id = service.child.as_ref().unwrap().id();

        for fixed_name in ["invalid name", "codex-elves-test"] {
            assert!(
                service
                    .start_public(None, fixed_name, true, "http://127.0.0.1:45222")
                    .await
                    .is_err()
            );
            assert_eq!(service.child.as_ref().unwrap().id(), process_id);
            assert_eq!(service.current_origin(), Some(origin.clone()));
            assert_eq!(service.state.as_str(), "ready");
        }
        let commands =
            ScriptedCommands::new([("status", Err(anyhow::anyhow!("simulated status failure")))]);
        assert!(
            service
                .start_public_with_commands(
                    None,
                    "codex-elves-test",
                    true,
                    "http://127.0.0.1:45222",
                    &commands,
                )
                .await
                .is_err()
        );
        service.reconcile(true, "http://127.0.0.1:45222").await;

        assert_eq!(service.child.as_ref().unwrap().id(), process_id);
        assert_eq!(service.current_origin(), Some(origin.clone()));
        assert_eq!(service.state.as_str(), "ready");
        assert!(service.environment_check.is_none());
        commands.assert_calls(&["status"]);
        for (replies, expected_calls, expected_error) in [
            (
                vec![
                    ("status", command_success(BOUND_STATUS)),
                    ("list", Err(anyhow::anyhow!("无法执行 zrok 命令"))),
                ],
                vec!["status", "list"],
                "无法执行 zrok 命令",
            ),
            (
                vec![
                    ("status", command_success(BOUND_STATUS)),
                    ("list", command_failure("panic: TLS handshake timeout")),
                    ("list", command_failure("panic: TLS handshake timeout")),
                ],
                vec!["status", "list", "list"],
                "TLS handshake timeout",
            ),
        ] {
            let commands = ScriptedCommands::new(replies);
            let error = service
                .start_public_with_commands(
                    None,
                    "codex-elves-test",
                    true,
                    "http://127.0.0.1:45222",
                    &commands,
                )
                .await
                .unwrap_err();
            assert!(format!("{error:#}").contains(expected_error));
            assert_eq!(service.child.as_ref().unwrap().id(), process_id);
            assert_eq!(service.current_origin(), Some(origin.clone()));
            assert_eq!(service.state.as_str(), "ready");
            commands.assert_calls(&expected_calls);
        }
        service.stop(true).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn changed_target_still_stops_previous_process_after_failed_environment_check() {
        let mut service = test_service();
        service.bound = true;
        service.environment_checked = false;
        service.fixed_name = "codex-elves-test".into();
        service.target = Some("http://127.0.0.1:45222".into());
        service.child = Some(test_child().await);
        service.state = ZrokState::Ready;
        let commands =
            ScriptedCommands::new([("status", Err(anyhow::anyhow!("simulated status failure")))]);
        assert!(
            service
                .start_public_with_commands(
                    None,
                    "codex-elves-test",
                    true,
                    "http://127.0.0.1:45222",
                    &commands,
                )
                .await
                .is_err()
        );

        service.reconcile(true, "http://127.0.0.1:45223").await;

        assert!(service.child.is_none());
        assert!(service.current_origin().is_none());
        assert_eq!(service.target.as_deref(), Some("http://127.0.0.1:45223"));
        assert_eq!(service.state.as_str(), "error");
        commands.assert_calls(&["status"]);
    }

    #[test]
    fn selects_only_reserved_codex_elves_public_name() {
        let names = vec![
            ZrokName {
                name: "unrelated".into(),
                namespace_token: "public".into(),
                reserved: true,
            },
            ZrokName {
                name: "codex-elves-preview".into(),
                namespace_token: "private".into(),
                reserved: true,
            },
            ZrokName {
                name: "codex-elves-junxin367".into(),
                namespace_token: "public".into(),
                reserved: true,
            },
        ];
        assert_eq!(
            select_reserved_name(&names).as_deref(),
            Some("codex-elves-junxin367")
        );
    }

    #[test]
    fn reclaims_only_the_current_environments_exact_fixed_public_share() {
        let shares: ZrokShareList = serde_json::from_value(json!({
            "shares":[
                {
                    "envZId":"other-environment",
                    "frontendEndpoints":["codex-elves-junxin367.shares.zrok.io"],
                    "shareMode":"public",
                    "shareToken":"other-environment-token",
                    "target":"http://127.0.0.1:45222"
                },
                {
                    "envZId":"current-environment",
                    "frontendEndpoints":["other-name.shares.zrok.io"],
                    "shareMode":"public",
                    "shareToken":"other-name-token",
                    "target":"http://127.0.0.1:45222"
                },
                {
                    "envZId":"current-environment",
                    "frontendEndpoints":["codex-elves-junxin367.shares.zrok.io"],
                    "shareMode":"public",
                    "shareToken":"reclaim-me",
                    "target":"http://127.0.0.1:45223"
                }
            ]
        }))
        .unwrap();

        assert_eq!(
            reclaimable_share_token(
                &shares.shares,
                "current-environment",
                "codex-elves-junxin367"
            ),
            Some("reclaim-me")
        );
        assert_eq!(
            reclaimable_share_token(
                &shares.shares,
                "missing-environment",
                "codex-elves-junxin367"
            ),
            None
        );
    }

    #[test]
    fn validates_registration_email() {
        assert!(valid_email("user@example.com"));
        assert!(!valid_email("user"));
        assert!(!valid_email("a@localhost"));
        assert!(!valid_email("a b@example.com"));
    }

    #[test]
    fn same_target_does_not_require_restart() {
        assert!(!target_requires_restart(
            Some("http://127.0.0.1:9527"),
            "http://127.0.0.1:9527"
        ));
        assert!(target_requires_restart(
            Some("http://127.0.0.1:9527"),
            "http://127.0.0.1:9528"
        ));
        assert!(!target_requires_restart(None, "http://127.0.0.1:9527"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "仅由进程生命周期测试调用"]
    fn job_child() {
        if std::env::var("CODEX_ELVES_ZROK_JOB_CHILD").as_deref() != Ok("1") {
            return;
        }
        println!("job-child-ready");
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn owned_zrok_job_terminates_child_when_closed() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "remote_access::zrok_tunnel::tests::job_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CODEX_ELVES_ZROK_JOB_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        let mut child = command.spawn().unwrap();
        let job = child_job(&child, "zrok").unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if line == "job-child-ready" {
                    return;
                }
            }
            panic!("test child exited before becoming ready");
        })
        .await
        .unwrap();
        drop(job);
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
