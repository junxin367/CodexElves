use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use super::now_ms;

const CLOUDFLARED_VERSION: &str = "2026.10.0";
const CLOUDFLARED_URL: &str = "https://github.com/cloudflare/cloudflared/releases/download/2026.10.0/cloudflared-windows-amd64.exe";
const CLOUDFLARED_SHA256: &str = "86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c";
const MAX_PROGRAM_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PUBLIC_RESPONSE_BYTES: usize = 64 * 1024;
const RETRY_MS: u64 = 30_000;
const RATE_LIMIT_RETRY_MS: u64 = 10 * 60_000;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_TIMEOUT: Duration = Duration::from_secs(15);
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuickTunnelState {
    Stopped,
    Downloading,
    Connecting,
    Ready,
    Backoff,
    Error,
}

impl QuickTunnelState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Downloading => "downloading",
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::Backoff => "retry_wait",
            Self::Error => "error",
        }
    }
}

enum ProcessEvent {
    Origin(String),
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

pub(super) struct QuickTunnelService {
    state_dir: PathBuf,
    origin: Arc<RwLock<Option<String>>>,
    child: Option<Child>,
    download: Option<tokio::task::JoinHandle<anyhow::Result<PathBuf>>>,
    download_progress: Arc<DownloadProgress>,
    readers: Vec<tokio::task::JoinHandle<()>>,
    events: Option<mpsc::UnboundedReceiver<ProcessEvent>>,
    target: Option<String>,
    state: QuickTunnelState,
    installed: bool,
    version: Option<String>,
    error: Option<String>,
    last_diagnostic: Option<String>,
    retry_at_ms: u64,
    checked_at_ms: u64,
}

impl QuickTunnelService {
    pub(super) fn new(state_dir: impl Into<PathBuf>, origin: Arc<RwLock<Option<String>>>) -> Self {
        let state_dir = state_dir.into();
        let installed = managed_program_path(&state_dir).is_file();
        Self {
            state_dir,
            origin,
            child: None,
            download: None,
            download_progress: Arc::new(DownloadProgress::default()),
            readers: Vec::new(),
            events: None,
            target: None,
            state: QuickTunnelState::Stopped,
            installed,
            version: installed.then(|| format!("cloudflared version {CLOUDFLARED_VERSION}")),
            error: None,
            last_diagnostic: None,
            retry_at_ms: 0,
            checked_at_ms: 0,
        }
    }

    pub(super) async fn reconcile(&mut self, enabled: bool, web_running: bool, target: &str) {
        if !(enabled && web_running) {
            self.stop(true).await;
            self.error = None;
            self.retry_at_ms = 0;
            self.checked_at_ms = 0;
            self.state = QuickTunnelState::Stopped;
            return;
        }
        if let Err(error) = validate_target(target) {
            self.stop(true).await;
            self.error = Some(format!("{error:#}"));
            self.state = QuickTunnelState::Error;
            return;
        }
        if target_requires_restart(self.target.as_deref(), target) {
            self.stop(false).await;
            self.target = Some(target.to_owned());
            self.error = None;
            self.retry_at_ms = 0;
            self.checked_at_ms = 0;
        } else if self.target.is_none() {
            self.target = Some(target.to_owned());
        }

        self.refresh_process().await;
        if self.child.is_some() {
            self.state = if self.current_origin().is_some() {
                QuickTunnelState::Ready
            } else {
                QuickTunnelState::Connecting
            };
            return;
        }
        if now_ms() < self.retry_at_ms {
            self.state = QuickTunnelState::Backoff;
            return;
        }

        if let Some(task) = self.download.as_ref()
            && !task.is_finished()
        {
            self.state = QuickTunnelState::Downloading;
            return;
        }
        if let Some(task) = self.download.take() {
            match task.await {
                Ok(Ok(program)) => {
                    self.installed = true;
                    self.version = Some(format!("cloudflared version {CLOUDFLARED_VERSION}"));
                    if let Err(error) = self.start(&program, target).await {
                        self.fail_with_backoff(error);
                    }
                }
                Ok(Err(error)) => {
                    self.installed = false;
                    self.version = None;
                    self.fail_with_backoff(error);
                }
                Err(error) => {
                    self.installed = false;
                    self.version = None;
                    self.fail_with_backoff(anyhow::anyhow!("cloudflared 下载任务未完成：{error}"))
                }
            }
            return;
        }

        let directory = self.state_dir.clone();
        let progress = Arc::clone(&self.download_progress);
        self.installed = false;
        progress.reset();
        self.download = Some(tokio::spawn(async move {
            ensure_managed_cloudflared(&directory, &progress).await
        }));
        self.state = QuickTunnelState::Downloading;
    }

    pub(super) async fn restart(&mut self, target: &str) {
        self.stop(false).await;
        self.target = Some(target.to_owned());
        self.error = None;
        self.retry_at_ms = 0;
        self.checked_at_ms = 0;
        self.state = QuickTunnelState::Stopped;
    }

    pub(super) fn invalidate(&mut self) {
        self.retry_at_ms = 0;
        self.checked_at_ms = 0;
        if self.child.is_some() {
            self.error = None;
        }
    }

    pub(super) async fn check(&mut self) -> anyhow::Result<Value> {
        self.refresh_process().await;
        self.test_public_access().await
    }

    pub(super) fn status(&self, enabled: bool) -> Value {
        let public_url = self.current_origin().unwrap_or_default();
        let running = self.child.is_some();
        let state = if !enabled {
            QuickTunnelState::Stopped
        } else {
            self.state
        };
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
            "enabled":enabled,
            "state":state.as_str(),
            "running":running,
            "connected":running && !public_url.is_empty(),
            "processId":self.child.as_ref().and_then(Child::id),
            "installed":self.installed,
            "version":self.version,
            "downloadedBytes":downloaded_bytes,
            "downloadTotalBytes":download_total_bytes,
            "downloadPercent":download_percent,
            "target":self.target,
            "publicUrl":public_url,
            "managedVersion":CLOUDFLARED_VERSION,
            "error":self.error,
            "checkedAtMs":self.checked_at_ms,
            "retryAtMs":self.retry_at_ms,
        })
    }

    pub(super) async fn test_public_access(&mut self) -> anyhow::Result<Value> {
        let origin = self
            .current_origin()
            .context("Quick Tunnel 尚未返回公网地址，请稍后再试")?;
        anyhow::ensure!(
            self.child.is_some(),
            "cloudflared 未运行，请重新启用 Quick Tunnel"
        );
        self.checked_at_ms = now_ms();
        match probe_public_access(&origin).await {
            Ok(value) => {
                self.error = None;
                self.state = QuickTunnelState::Ready;
                Ok(value)
            }
            Err(error) => {
                self.error = Some(format!(
                    "Quick Tunnel 仍在运行，但公网就绪检查暂时失败：{error:#}"
                ));
                self.state = QuickTunnelState::Connecting;
                Err(error)
            }
        }
    }

    async fn start(&mut self, program: &Path, target: &str) -> anyhow::Result<()> {
        verify_managed_program(program).await?;
        let config = self.state_dir.join("remote-access-cloudflared-quick.yml");
        crate::settings::atomic_write(&config, b"{}\n")
            .context("无法创建 Quick Tunnel 隔离配置")?;
        let mut command = Command::new(program);
        command
            .arg("--no-autoupdate")
            .arg("tunnel")
            .arg("--config")
            .arg(&config)
            .arg("--url")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        remove_cloudflared_credentials(&mut command);
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .context("无法启动受管 cloudflared，Quick Tunnel 未启用")?;
        let stdout = child
            .stdout
            .take()
            .context("无法读取 cloudflared 标准输出")?;
        let stderr = child
            .stderr
            .take()
            .context("无法读取 cloudflared 错误输出")?;
        let (sender, receiver) = mpsc::unbounded_channel();
        self.readers = vec![
            spawn_output_reader(stdout, sender.clone()),
            spawn_output_reader(stderr, sender),
        ];
        self.events = Some(receiver);
        self.child = Some(child);
        self.target = Some(target.to_owned());
        self.state = QuickTunnelState::Connecting;
        self.error = None;
        self.last_diagnostic = None;
        self.checked_at_ms = 0;
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
                for task in self.readers.drain(..) {
                    let _ = tokio::time::timeout(Duration::from_millis(250), task).await;
                }
                self.consume_events();
                self.events = None;
                self.clear_origin();
                let diagnostic = self.last_diagnostic.take();
                let rate_limited = diagnostic.as_deref().is_some_and(rate_limited_diagnostic);
                let retry_ms = if rate_limited {
                    RATE_LIMIT_RETRY_MS
                } else {
                    RETRY_MS
                };
                self.error = Some(if rate_limited {
                    "Cloudflare 暂时限制生成临时地址（HTTP 429），10 分钟后自动重试；也可稍后手动重新生成。"
                        .into()
                } else if let Some(diagnostic) = diagnostic {
                    format!(
                        "cloudflared 已退出（{status}）：{diagnostic}。30 秒后重试，当前 Quick Tunnel 地址已失效。"
                    )
                } else {
                    format!(
                        "cloudflared 已退出（{status}），30 秒后重试。Quick Tunnel 地址已失效。"
                    )
                });
                self.retry_at_ms = now_ms() + retry_ms;
                self.state = QuickTunnelState::Backoff;
            }
            Err(error) => {
                let message = anyhow::Error::new(error).context("无法读取 cloudflared 进程状态");
                self.stop(false).await;
                self.fail_with_backoff(message);
            }
        }
    }

    fn consume_events(&mut self) {
        let mut origin_missing = self.current_origin().is_none();
        let Some(receiver) = self.events.as_mut() else {
            return;
        };
        while let Ok(event) = receiver.try_recv() {
            match event {
                ProcessEvent::Origin(origin) => {
                    let mut current = self
                        .origin
                        .write()
                        .unwrap_or_else(|error| error.into_inner());
                    if current.is_none() {
                        *current = Some(origin);
                    }
                    origin_missing = false;
                    self.error = None;
                    self.last_diagnostic = None;
                    self.state = QuickTunnelState::Ready;
                }
                ProcessEvent::Diagnostic(diagnostic) => {
                    self.last_diagnostic = Some(diagnostic);
                }
                ProcessEvent::ReadError(error) if origin_missing => {
                    self.last_diagnostic = Some(error.clone());
                    self.error = Some(error);
                }
                ProcessEvent::ReadError(error) => {
                    self.last_diagnostic = Some(error);
                }
            }
        }
    }

    async fn stop(&mut self, clear_target: bool) {
        if let Some(task) = self.download.take() {
            task.abort();
            let _ = task.await;
        }
        self.download_progress.reset();
        for task in self.readers.drain(..) {
            task.abort();
        }
        self.events = None;
        self.last_diagnostic = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        self.clear_origin();
        if clear_target {
            self.target = None;
        }
    }

    fn fail_with_backoff(&mut self, error: anyhow::Error) {
        self.clear_origin();
        self.error = Some(format!("{error:#}"));
        self.retry_at_ms = now_ms() + RETRY_MS;
        self.state = QuickTunnelState::Backoff;
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

fn target_requires_restart(current: Option<&str>, requested: &str) -> bool {
    current.is_some_and(|target| target != requested)
}

fn validate_target(target: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(target).context("Quick Tunnel 回源地址无效")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Quick Tunnel 回源地址必须是无凭据、无查询参数的 HTTP(S) 地址"
    );
    Ok(())
}

fn managed_program_path(state_dir: &Path) -> PathBuf {
    state_dir.join("tools/cloudflared").join(format!(
        "cloudflared-{CLOUDFLARED_VERSION}-{CLOUDFLARED_SHA256}.exe"
    ))
}

async fn ensure_managed_cloudflared(
    state_dir: &Path,
    progress: &DownloadProgress,
) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        cfg!(all(target_os = "windows", target_arch = "x86_64")),
        "Quick Tunnel 自动下载仅支持 Windows x64"
    );
    let _guard = DOWNLOAD_LOCK.lock().await;
    let path = managed_program_path(state_dir);
    if verify_managed_program(&path).await.is_ok() {
        if let Ok(metadata) = std::fs::metadata(&path) {
            progress
                .downloaded_bytes
                .store(metadata.len(), Ordering::Relaxed);
            progress
                .total_bytes
                .store(metadata.len(), Ordering::Relaxed);
        }
        return Ok(path);
    }

    let parent = path.parent().context("cloudflared 保存路径无效")?;
    std::fs::create_dir_all(parent).context("无法创建 cloudflared 工具目录")?;
    let temp = parent.join(format!(
        ".cloudflared-{CLOUDFLARED_VERSION}-{}.download",
        uuid::Uuid::new_v4().simple()
    ));
    let mut temporary = TemporaryDownload::create(temp)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .user_agent("CodexElves-cloudflared-quick-tunnel")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()?;
    let mut response = client
        .get(CLOUDFLARED_URL)
        .send()
        .await
        .context("无法下载官方 cloudflared，请检查网络后重试")?
        .error_for_status()
        .context("官方 cloudflared 下载暂不可用，请稍后重试")?;
    ensure_official_download(response.url())?;
    let content_length = response.content_length().unwrap_or(0);
    anyhow::ensure!(
        content_length <= MAX_PROGRAM_BYTES,
        "官方 cloudflared 文件过大，已取消下载"
    );
    progress
        .total_bytes
        .store(content_length, Ordering::Relaxed);
    let mut size = 0u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("官方 cloudflared 下载中断，请稍后重试")?
    {
        size = size
            .checked_add(chunk.len() as u64)
            .context("官方 cloudflared 文件大小溢出")?;
        anyhow::ensure!(
            size <= MAX_PROGRAM_BYTES,
            "官方 cloudflared 文件过大，已取消下载"
        );
        temporary
            .file
            .as_mut()
            .expect("temporary download file should be open")
            .write_all(&chunk)
            .context("无法写入 cloudflared 临时文件")?;
        hasher.update(&chunk);
        progress.downloaded_bytes.store(size, Ordering::Relaxed);
    }
    temporary
        .file
        .as_mut()
        .expect("temporary download file should be open")
        .sync_all()
        .context("无法完成 cloudflared 临时文件写入")?;
    anyhow::ensure!(size > 0, "官方 cloudflared 下载内容为空");
    if content_length == 0 {
        progress.total_bytes.store(size, Ordering::Relaxed);
    }
    anyhow::ensure!(
        format!("{:x}", hasher.finalize()) == CLOUDFLARED_SHA256,
        "官方 cloudflared 的 SHA-256 不匹配，已拒绝保存和运行"
    );
    let bytes = read_limited_file(&temporary.path, MAX_PROGRAM_BYTES)?;
    crate::settings::atomic_write(&path, &bytes).context("无法保存官方 cloudflared 程序")?;
    temporary.remove()?;
    verify_managed_program(&path).await?;
    Ok(path)
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
        "cloudflared 下载不是官方 HTTPS 资产地址，已拒绝使用"
    );
    Ok(())
}

async fn verify_managed_program(path: &Path) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    let check_path = path.clone();
    tokio::task::spawn_blocking(move || verify_program_sha256(&check_path))
        .await
        .context("cloudflared 校验任务未完成")??;
    check_cloudflared_version(&path).await
}

fn verify_program_sha256(path: &Path) -> anyhow::Result<()> {
    let bytes =
        read_limited_file(path, MAX_PROGRAM_BYTES).context("受管 cloudflared 缺失或无法读取")?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == CLOUDFLARED_SHA256,
        "受管 cloudflared 校验失败，已拒绝运行"
    );
    Ok(())
}

fn read_limited_file(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit,
        "cloudflared 文件大小无效"
    );
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= limit,
        "cloudflared 文件大小无效"
    );
    Ok(bytes)
}

async fn check_cloudflared_version(path: &Path) -> anyhow::Result<()> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    remove_cloudflared_credentials(&mut command);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = tokio::time::timeout(VERSION_TIMEOUT, command.output())
        .await
        .context("检查 cloudflared 版本超时")?
        .context("无法执行受管 cloudflared")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.lines().next().unwrap_or("").trim();
    anyhow::ensure!(
        output.status.success()
            && version.starts_with("cloudflared version ")
            && version.contains(CLOUDFLARED_VERSION)
            && version.len() < 200,
        "受管程序未返回预期的 cloudflared {CLOUDFLARED_VERSION} 版本"
    );
    Ok(())
}

fn remove_cloudflared_credentials(command: &mut Command) {
    for name in [
        "TUNNEL_TOKEN",
        "TUNNEL_TOKEN_FILE",
        "TUNNEL_ORIGIN_CERT",
        "TUNNEL_CERT",
        "TUNNEL_CRED_FILE",
        "TUNNEL_CREDENTIAL_FILE",
        "TUNNEL_CREDENTIALS_FILE",
        "CLOUDFLARED_TOKEN",
        "CLOUDFLARED_ORIGIN_CERT",
        "CF_TUNNEL_TOKEN",
    ] {
        command.env_remove(name);
    }
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
                    if let Some(origin) = quick_tunnel_origin(&line) {
                        let _ = sender.send(ProcessEvent::Origin(origin));
                    } else if let Some(diagnostic) = quick_tunnel_diagnostic(&line) {
                        let _ = sender.send(ProcessEvent::Diagnostic(diagnostic));
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(ProcessEvent::ReadError(format!(
                        "无法读取 cloudflared 输出：{error}"
                    )));
                    break;
                }
            }
        }
    })
}

fn quick_tunnel_diagnostic(text: &str) -> Option<String> {
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
    ]
    .iter()
    .any(|needle| lowercase.contains(needle))
    {
        return None;
    }
    Some(value.chars().take(600).collect())
}

fn rate_limited_diagnostic(text: &str) -> bool {
    let lowercase = text.to_ascii_lowercase();
    lowercase.contains("status 429")
        || lowercase.contains("http 429")
        || lowercase.contains("rate limit")
}

fn quick_tunnel_origin(text: &str) -> Option<String> {
    let mut offset = 0;
    while let Some(found) = text[offset..].find("https://") {
        let start = offset + found;
        let end = text[start..]
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'' | '<' | '>')
            })
            .map_or(text.len(), |length| start + length);
        let candidate =
            text[start..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
        if let Ok(url) = reqwest::Url::parse(candidate)
            && strict_quick_tunnel_origin(&url)
        {
            return Some(url.origin().ascii_serialization());
        }
        offset = start + "https://".len();
    }
    None
}

fn strict_quick_tunnel_origin(url: &reqwest::Url) -> bool {
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
    let Some(label) = host.strip_suffix(".trycloudflare.com") else {
        return false;
    };
    !label.is_empty()
        && label.len() <= 63
        && !label.contains('.')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

async fn probe_public_access(origin: &str) -> anyhow::Result<Value> {
    let url = reqwest::Url::parse(origin).context("Quick Tunnel 公网地址无效")?;
    anyhow::ensure!(
        strict_quick_tunnel_origin(&url),
        "Quick Tunnel 公网地址不属于严格的 trycloudflare.com Origin"
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PUBLIC_TIMEOUT)
        .build()?;
    let response = client
        .get(origin)
        .send()
        .await
        .context("Quick Tunnel 公网 HTTPS 连接暂不可用")?;
    let http_status = response.status();
    let text = bounded_response_text(response).await?;
    anyhow::ensure!(
        http_status == reqwest::StatusCode::OK,
        "Quick Tunnel 公网入口返回 HTTP {}",
        http_status.as_u16()
    );
    anyhow::ensure!(
        text.contains("CodexElves") && text.contains("/app.js"),
        "Quick Tunnel 未返回 CodexElves 远程网页"
    );
    let protected = client
        .get(format!("{origin}/api/session"))
        .send()
        .await
        .context("无法检查 Quick Tunnel 访问授权边界")?;
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
        "message":"Quick Tunnel 公网 HTTPS 与网页回源验证通过；未登录访问会话接口返回 401。"
    }))
}

async fn bounded_response_text(mut response: reqwest::Response) -> anyhow::Result<String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len().saturating_add(chunk.len()) <= MAX_PUBLIC_RESPONSE_BYTES,
            "Quick Tunnel 公网网页响应过大，无法确认服务"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
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
            .context("无法创建 cloudflared 下载临时文件")?;
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
        std::fs::remove_file(&self.path).context("无法清理 cloudflared 下载临时文件")?;
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

    #[test]
    fn parses_only_single_label_quick_tunnel_origin() {
        assert_eq!(
            quick_tunnel_origin(
                "INF Your quick Tunnel has been created! Visit it at https://random-name-123.trycloudflare.com"
            ),
            Some("https://random-name-123.trycloudflare.com".into())
        );
        assert_eq!(
            quick_tunnel_origin(
                r#"{"message":"https://alpha123.trycloudflare.com","level":"info"}"#
            ),
            Some("https://alpha123.trycloudflare.com".into())
        );
    }

    #[test]
    fn rejects_malicious_or_non_origin_urls() {
        for value in [
            "https://alpha.trycloudflare.com.evil.test",
            "https://alpha.trycloudflare.com@evil.test",
            "https://alpha.beta.trycloudflare.com",
            "https://-alpha.trycloudflare.com",
            "https://alpha-.trycloudflare.com",
            "https://alpha.trycloudflare.com/path",
            "https://alpha.trycloudflare.com?next=https://evil.test",
            "http://alpha.trycloudflare.com",
        ] {
            assert_eq!(quick_tunnel_origin(value), None, "{value}");
        }
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

    #[test]
    fn keeps_actionable_rate_limit_diagnostic() {
        let diagnostic =
            quick_tunnel_diagnostic("quick tunnel provisioning failed with status 429").unwrap();
        assert!(rate_limited_diagnostic(&diagnostic));
        assert_eq!(
            quick_tunnel_diagnostic("INF Registered tunnel connection connIndex=0"),
            None
        );
    }
}
