use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::settings::RemoteAccessSettings;

use super::now_ms;

const FUNNEL_PORT: u16 = 443;
const READY_RECHECK_MS: u64 = 30_000;
const RETRY_MS: u64 = 30_000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const INSTALLER_PAGE: &str = "https://pkgs.tailscale.com/stable/";
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_INSTALLER_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunnelState {
    Waiting,
    NotInstalled,
    NotReady,
    ActionRequired,
    Ready,
    Error,
}

impl FunnelState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::NotInstalled => "not_installed",
            Self::NotReady => "not_ready",
            Self::ActionRequired => "action_required",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

enum Attempt {
    NotInstalled,
    NotReady { backend_state: String },
    ActionRequired { url: String },
    Ready { origin: String },
}

pub(super) struct FunnelService {
    origin: Arc<RwLock<Option<String>>>,
    target: Option<String>,
    cleanup_pending: bool,
    state: FunnelState,
    installed: bool,
    backend_state: Option<String>,
    action_url: Option<String>,
    error: Option<String>,
    checked_at_ms: u64,
    retry_at_ms: u64,
}

impl FunnelService {
    pub(super) fn new(origin: Arc<RwLock<Option<String>>>) -> Self {
        Self {
            origin,
            target: None,
            cleanup_pending: true,
            state: FunnelState::Waiting,
            installed: false,
            backend_state: None,
            action_url: None,
            error: None,
            checked_at_ms: 0,
            retry_at_ms: 0,
        }
    }

    pub(super) async fn restart(&mut self, target: &str) {
        if let Err(error) = self.release(target).await {
            self.error = Some(format!("{error:#}"));
        }
        self.checked_at_ms = 0;
        self.retry_at_ms = 0;
    }

    pub(super) async fn reconcile(&mut self, settings: &RemoteAccessSettings, web_running: bool) {
        let target = local_target(settings);
        if !(settings.enabled && settings.funnel_enabled && web_running) {
            let released = if self.target.is_some() || self.cleanup_pending {
                self.release(&target).await.err()
            } else {
                None
            };
            self.set(
                FunnelState::Waiting,
                None,
                released.map(|error| format!("关闭 Funnel 失败：{error:#}")),
            );
            return;
        }
        let now = now_ms();
        if self.state == FunnelState::Ready
            && self.target.as_deref() == Some(target.as_str())
            && now < self.checked_at_ms + READY_RECHECK_MS
        {
            return;
        }
        if self.state != FunnelState::Ready && now < self.retry_at_ms {
            return;
        }
        self.enable(&target).await;
    }

    pub(super) fn invalidate(&mut self) {
        self.retry_at_ms = 0;
    }

    pub(super) fn status(&self, settings: &RemoteAccessSettings) -> Value {
        let state = if settings.funnel_enabled {
            self.state.as_str()
        } else {
            "stopped"
        };
        json!({
            "enabled":settings.funnel_enabled,
            "state":state,
            "installed":self.installed,
            "backendState":self.backend_state,
            "publicUrl":self.origin.read().unwrap_or_else(|error| error.into_inner()).clone().unwrap_or_default(),
            "actionUrl":self.action_url,
            "error":self.error,
        })
    }

    async fn enable(&mut self, target: &str) {
        let now = now_ms();
        self.checked_at_ms = now;
        self.retry_at_ms = now + RETRY_MS;
        if self
            .target
            .as_deref()
            .is_some_and(|recorded| recorded != target)
        {
            if let Err(error) = self.release(target).await {
                self.set(
                    FunnelState::Error,
                    None,
                    Some(format!("关闭旧 Funnel 配置失败：{error:#}")),
                );
                return;
            }
        }
        self.cleanup_pending = true;
        match attempt(target).await {
            Ok(Attempt::NotInstalled) => {
                self.installed = false;
                self.backend_state = None;
                self.set(
                    FunnelState::NotInstalled,
                    None,
                    Some("未检测到 Tailscale，请先点击「安装程序」。".into()),
                );
            }
            Ok(Attempt::NotReady { backend_state }) => {
                self.installed = true;
                let message = backend_message(&backend_state);
                self.backend_state = Some(backend_state);
                self.set(FunnelState::NotReady, None, Some(message));
            }
            Ok(Attempt::ActionRequired { url }) => {
                self.installed = true;
                self.backend_state = Some("Running".into());
                self.set(
                    FunnelState::ActionRequired,
                    Some(url),
                    Some("请在打开的 Tailscale 页面完成 HTTPS 与 Funnel 授权，然后点击「保存并应用」。".into()),
                );
            }
            Ok(Attempt::Ready { origin }) => {
                self.installed = true;
                self.backend_state = Some("Running".into());
                self.target = Some(target.to_owned());
                *self
                    .origin
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = Some(origin);
                self.retry_at_ms = 0;
                self.set(FunnelState::Ready, None, None);
            }
            Err(error) => self.set(FunnelState::Error, None, Some(format!("{error:#}"))),
        }
    }

    async fn release(&mut self, target: &str) -> anyhow::Result<()> {
        let recorded = self.target.take();
        self.cleanup_pending = false;
        self.clear_origin();
        let Some(program) = locate_tailscale().await else {
            return Ok(());
        };
        let Ok(status) = tailnet_status(&program).await else {
            return Ok(());
        };
        let Some(dns) = status.dns_name else {
            return Ok(());
        };
        let host_port = format!("{dns}:{FUNNEL_PORT}");
        let Ok(config) = funnel_config(&program).await else {
            return Ok(());
        };
        let proxy = web_proxy(&config, &host_port);
        for candidate in recorded.iter().map(String::as_str).chain([target]) {
            if proxy.as_deref() == Some(candidate) {
                return funnel_off(&program, candidate).await;
            }
        }
        Ok(())
    }

    fn set(&mut self, state: FunnelState, action_url: Option<String>, error: Option<String>) {
        if state != FunnelState::Ready {
            self.clear_origin();
        }
        self.state = state;
        self.action_url = action_url;
        self.error = error;
    }

    fn clear_origin(&self) {
        *self
            .origin
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

pub(super) fn local_target(settings: &RemoteAccessSettings) -> String {
    match settings.bind_host.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => format!("http://[{address}]:{}", settings.port),
        _ => format!("http://{}:{}", settings.bind_host, settings.port),
    }
}

async fn attempt(target: &str) -> anyhow::Result<Attempt> {
    let Some(program) = locate_tailscale().await else {
        return Ok(Attempt::NotInstalled);
    };
    let status = tailnet_status(&program).await?;
    if status.backend_state != "Running" {
        return Ok(Attempt::NotReady {
            backend_state: status.backend_state,
        });
    }
    let dns = status
        .dns_name
        .context("Tailscale 未返回本机域名，请在 Tailscale 管理后台开启 MagicDNS")?;
    let origin = public_origin(&dns)?;
    let host_port = format!("{dns}:{FUNNEL_PORT}");
    let config = funnel_config(&program).await?;
    ensure_port_free(&config, &host_port, target)?;
    if !is_serving(&config, &host_port, target) {
        let output = run(
            &program,
            &["funnel", "--bg", "--yes", "--https=443", target],
        )
        .await?;
        if !output.status.success() {
            let text = combined_output(&output);
            return match consent_url(&text) {
                Some(url) => Ok(Attempt::ActionRequired { url }),
                None => anyhow::bail!("Funnel 启用失败：{}", first_line(&text)),
            };
        }
        let config = funnel_config(&program).await?;
        anyhow::ensure!(
            is_serving(&config, &host_port, target),
            "Funnel 配置未生效，请检查 Tailscale 授权后重新应用"
        );
    }
    Ok(Attempt::Ready { origin })
}

async fn funnel_off(program: &Path, target: &str) -> anyhow::Result<()> {
    let output = run(program, &["funnel", "--yes", "--https=443", target, "off"]).await?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        first_line(&combined_output(&output))
    );
    Ok(())
}

pub(super) async fn probe_tailscale() -> Value {
    let Some(program) = locate_tailscale().await else {
        return json!({"status":"ok","installed":false,"message":"未检测到 Tailscale，请先安装官方客户端。"});
    };
    match tailnet_status(&program).await {
        Ok(status) => {
            let message =
                (status.backend_state != "Running").then(|| backend_message(&status.backend_state));
            json!({"status":"ok","installed":true,"backendState":status.backend_state,"message":message})
        }
        Err(error) => {
            json!({"status":"ok","installed":true,"backendState":null,"message":format!("{error:#}")})
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InstallPhase {
    Idle,
    Downloading,
    Verifying,
    Launched,
    Failed,
}

impl InstallPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Launched => "launched",
            Self::Failed => "failed",
        }
    }
}

struct InstallProgress {
    phase: InstallPhase,
    downloaded: u64,
    total: u64,
    message: Option<String>,
}

static INSTALL_PROGRESS: std::sync::Mutex<InstallProgress> =
    std::sync::Mutex::new(InstallProgress {
        phase: InstallPhase::Idle,
        downloaded: 0,
        total: 0,
        message: None,
    });

fn update_install_progress(update: impl FnOnce(&mut InstallProgress)) {
    update(
        &mut INSTALL_PROGRESS
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
    );
}

pub async fn install_tailscale(state_dir: &Path) -> anyhow::Result<Value> {
    anyhow::ensure!(
        cfg!(all(target_os = "windows", target_arch = "x86_64")),
        "自动安装仅支持 Windows x64，请从 Tailscale 官网安装"
    );
    if locate_tailscale().await.is_some() {
        return Ok(
            json!({"status":"ok","installed":true,"message":"Tailscale 已安装，请点击「检查状态」。"}),
        );
    }
    {
        let mut progress = INSTALL_PROGRESS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        anyhow::ensure!(
            !matches!(
                progress.phase,
                InstallPhase::Downloading | InstallPhase::Verifying
            ),
            "Tailscale 安装包正在下载，请稍候"
        );
        *progress = InstallProgress {
            phase: InstallPhase::Downloading,
            downloaded: 0,
            total: 0,
            message: None,
        };
    }
    let directory = state_dir.to_path_buf();
    tokio::spawn(async move {
        let outcome = fetch_and_launch_installer(&directory).await;
        update_install_progress(|progress| match outcome {
            Ok(message) => {
                progress.phase = InstallPhase::Launched;
                progress.message = Some(message);
            }
            Err(error) => {
                progress.phase = InstallPhase::Failed;
                progress.message = Some(format!("{error:#}"));
            }
        });
    });
    Ok(json!({"status":"ok","installed":false,"started":true}))
}

pub(super) fn installer_progress() -> Value {
    let progress = INSTALL_PROGRESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    json!({
        "status":"ok",
        "phase":progress.phase.as_str(),
        "percent":percent(progress.downloaded, progress.total),
        "message":progress.message,
    })
}

fn percent(downloaded: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (downloaded.saturating_mul(100) / total).min(100)
    }
}

async fn fetch_and_launch_installer(state_dir: &Path) -> anyhow::Result<String> {
    let client = installer_client()?;
    let (page_url, page) = download(&client, INSTALLER_PAGE, MAX_PAGE_BYTES, |_, _| {}).await?;
    ensure_official_source(&page_url)?;
    let page = String::from_utf8(page).context("Tailscale 官方发布页编码无效")?;
    let version = installer_version(&page)?;
    let file_name = format!("tailscale-setup-{version}-amd64.msi");
    let (installer_url, bytes) = download(
        &client,
        &format!("{INSTALLER_PAGE}{file_name}"),
        MAX_INSTALLER_BYTES,
        |downloaded, total| {
            update_install_progress(|progress| {
                progress.downloaded = downloaded;
                progress.total = total;
            });
        },
    )
    .await?;
    ensure_official_source(&installer_url)?;
    let path = installer_directory(state_dir).join(&file_name);
    crate::settings::atomic_write(&path, &bytes).context("无法保存 Tailscale 安装包")?;
    update_install_progress(|progress| progress.phase = InstallPhase::Verifying);
    verify_installer(&path).await?;
    Command::new("msiexec.exe")
        .arg("/i")
        .arg(&path)
        .spawn()
        .context("无法启动 Tailscale 安装向导")?;
    Ok("已启动 Tailscale 官方安装向导，完成后点击「检查状态」。".into())
}

fn ensure_official_source(url: &reqwest::Url) -> anyhow::Result<()> {
    anyhow::ensure!(
        url.host_str() == Some("pkgs.tailscale.com"),
        "Tailscale 下载地址不是官方发布源，已拒绝使用"
    );
    Ok(())
}
fn installer_directory(state_dir: &Path) -> PathBuf {
    state_dir.join("tools").join("tailscale")
}

fn installer_client() -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .user_agent("CodexElves-tailscale")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300));
    if let Some(proxy_url) = crate::proxy::detect_system_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            builder = builder.proxy(proxy);
        }
    }
    Ok(builder.build()?)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
    mut on_progress: impl FnMut(u64, u64),
) -> anyhow::Result<(reqwest::Url, Vec<u8>)> {
    let mut attempt = 1;
    let mut response = loop {
        match client.get(url).send().await {
            Ok(response) => break response,
            Err(error) if attempt < 3 && (error.is_connect() || error.is_timeout()) => {
                tokio::time::sleep(Duration::from_secs(attempt)).await;
                attempt += 1;
            }
            Err(error) => {
                return Err(error).context(
                    "无法连接 Tailscale 官方发布源（请检查网络连接或代理，也可手动安装 Tailscale）",
                );
            }
        }
    }
    .error_for_status()
    .context("Tailscale 官方发布源暂不可用，请稍后重试")?;
    let total = response.content_length().unwrap_or(0);
    anyhow::ensure!(total <= limit as u64, "官方下载内容过大，已取消下载");
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    on_progress(0, total);
    while let Some(chunk) = response
        .chunk()
        .await
        .context("官方安装包下载中断，请稍后重试")?
    {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "官方下载内容过大，已取消下载"
        );
        bytes.extend_from_slice(&chunk);
        on_progress(bytes.len() as u64, total);
    }
    Ok((final_url, bytes))
}
async fn verify_installer(path: &Path) -> anyhow::Result<()> {
    const SCRIPT: &str = "$signature = Get-AuthenticodeSignature -LiteralPath $env:CODEX_ELVES_TAILSCALE_INSTALLER; Write-Output \"$($signature.Status)|$($signature.SignerCertificate.Subject)\"";
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("CODEX_ELVES_TAILSCALE_INSTALLER", path)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = output_with_timeout(command)
        .await
        .context("无法检查 Tailscale 安装包签名")?;
    let text = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(
        signature_is_tailscale(text.trim()),
        "Tailscale 安装包签名校验失败，已拒绝运行"
    );
    Ok(())
}

fn installer_version(page: &str) -> anyhow::Result<String> {
    const PREFIX: &str = "tailscale-setup-";
    const SUFFIX: &str = "-amd64.msi";
    for (index, _) in page.match_indices(PREFIX) {
        let rest = &page[index + PREFIX.len()..];
        let version: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit() || *character == '.')
            .collect();
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() == 3
            && parts.iter().all(|part| !part.is_empty() && part.len() <= 4)
            && rest[version.len()..].starts_with(SUFFIX)
        {
            return Ok(version);
        }
    }
    anyhow::bail!("无法从 Tailscale 官方发布页获取 Windows x64 安装包版本，请稍后重试")
}

fn signature_is_tailscale(line: &str) -> bool {
    line.split_once('|')
        .is_some_and(|(status, subject)| status == "Valid" && subject.contains("O=Tailscale Inc."))
}

fn public_origin(dns: &str) -> anyhow::Result<String> {
    let valid = dns.strip_suffix(".ts.net").is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
    });
    anyhow::ensure!(valid, "Tailscale 域名无效，请确认 MagicDNS 已开启");
    Ok(format!("https://{dns}"))
}

fn ensure_port_free(config: &Value, host_port: &str, target: &str) -> anyhow::Result<()> {
    let web = &config["Web"][host_port];
    if !web.is_object() {
        return Ok(());
    }
    let proxy = web_proxy(config, host_port);
    anyhow::ensure!(
        proxy.as_deref() == Some(target),
        "{FUNNEL_PORT} 端口已被其他 Serve/Funnel 配置占用（{}）。为避免覆盖，请先在 Tailscale 中移除该配置。",
        proxy.as_deref().unwrap_or("非代理配置")
    );
    Ok(())
}

fn is_serving(config: &Value, host_port: &str, target: &str) -> bool {
    web_proxy(config, host_port).as_deref() == Some(target)
        && config["AllowFunnel"][host_port].as_bool() == Some(true)
}

fn web_proxy(config: &Value, host_port: &str) -> Option<String> {
    config["Web"][host_port]["Handlers"]["/"]["Proxy"]
        .as_str()
        .map(str::to_owned)
}

fn consent_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                matches!(character, '"' | '\'' | ',' | '.' | ';' | '(' | ')')
            })
        })
        .find(|token| token.starts_with("https://login.tailscale.com/"))
        .map(str::to_owned)
}

fn backend_message(backend_state: &str) -> String {
    match backend_state {
        "NeedsLogin" => "Tailscale 尚未登录，请先在 Tailscale 客户端中登录账号。".into(),
        "Stopped" => "Tailscale 已停止，请在客户端中开启连接。".into(),
        "NeedsMachineAuth" => "设备等待管理员批准，请在 Tailscale 管理后台批准该设备。".into(),
        "Starting" | "NoState" => "Tailscale 正在启动，请稍后点击「检查状态」。".into(),
        other => format!("Tailscale 当前状态为 {other}，暂不可用。"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TailnetStatus {
    backend_state: String,
    dns_name: Option<String>,
}

fn parse_tailnet_status(value: &Value) -> TailnetStatus {
    TailnetStatus {
        backend_state: value["BackendState"]
            .as_str()
            .unwrap_or("Unknown")
            .to_owned(),
        dns_name: value["Self"]["DNSName"]
            .as_str()
            .map(|name| name.trim_end_matches('.').to_owned())
            .filter(|name| !name.is_empty()),
    }
}

async fn tailnet_status(program: &Path) -> anyhow::Result<TailnetStatus> {
    let output = run(program, &["status", "--json"]).await?;
    anyhow::ensure!(
        output.status.success(),
        "无法连接 Tailscale 服务：{}",
        first_line(&combined_output(&output))
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).context("Tailscale 状态输出无法解析")?;
    Ok(parse_tailnet_status(&value))
}

async fn funnel_config(program: &Path) -> anyhow::Result<Value> {
    let output = run(program, &["funnel", "status", "--json"]).await?;
    anyhow::ensure!(
        output.status.success(),
        "无法读取 Funnel 状态：{}",
        first_line(&combined_output(&output))
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() || text.contains("No serve config") {
        return Ok(json!({}));
    }
    serde_json::from_str(text).context("Funnel 状态输出无法解析")
}

async fn locate_tailscale() -> Option<PathBuf> {
    let installed = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join("Tailscale")
        .join("tailscale.exe");
    if installed.is_file() {
        return Some(installed);
    }
    let on_path = PathBuf::from("tailscale");
    run(&on_path, &["version"])
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|_| on_path)
}

async fn run(program: &Path, args: &[&str]) -> anyhow::Result<Output> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    output_with_timeout(command)
        .await
        .context("无法运行 Tailscale 命令")
}

async fn output_with_timeout(mut command: Command) -> anyhow::Result<Output> {
    tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .context("Tailscale 命令执行超时")?
        .map_err(anyhow::Error::from)
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_owned()
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("无输出")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "http://127.0.0.1:45222";

    fn serving_config(proxy: &str, allowed: bool) -> Value {
        json!({
            "TCP": {"443": {"HTTPS": true}},
            "Web": {"codex.tailnet.ts.net:443": {"Handlers": {"/": {"Proxy": proxy}}}},
            "AllowFunnel": {"codex.tailnet.ts.net:443": allowed},
        })
    }

    #[test]
    fn parses_running_status_and_strips_dns_dot() {
        let value = json!({"BackendState":"Running","Self":{"DNSName":"codex.tailnet.ts.net."}});
        let status = parse_tailnet_status(&value);
        assert_eq!(status.backend_state, "Running");
        assert_eq!(status.dns_name.as_deref(), Some("codex.tailnet.ts.net"));
    }

    #[test]
    fn missing_dns_name_is_reported_as_absent() {
        let value = json!({"BackendState":"Running","Self":{"DNSName":""}});
        assert_eq!(parse_tailnet_status(&value).dns_name, None);
    }

    #[test]
    fn public_origin_accepts_only_tailnet_names() {
        assert_eq!(
            public_origin("codex.tailnet.ts.net").unwrap(),
            "https://codex.tailnet.ts.net"
        );
        assert!(public_origin("codex.example.com").is_err());
        assert!(public_origin(".ts.net").is_err());
        assert!(public_origin("codex.tailnet.ts.net/evil").is_err());
    }

    #[test]
    fn serving_requires_matching_proxy_and_allowed_funnel() {
        let host_port = "codex.tailnet.ts.net:443";
        assert!(is_serving(&serving_config(TARGET, true), host_port, TARGET));
        assert!(!is_serving(
            &serving_config(TARGET, false),
            host_port,
            TARGET
        ));
        assert!(!is_serving(
            &serving_config("http://127.0.0.1:9999", true),
            host_port,
            TARGET
        ));
        assert!(!is_serving(&json!({}), host_port, TARGET));
    }

    #[test]
    fn foreign_443_handler_is_not_overwritten() {
        let host_port = "codex.tailnet.ts.net:443";
        assert!(ensure_port_free(&json!({}), host_port, TARGET).is_ok());
        assert!(ensure_port_free(&serving_config(TARGET, true), host_port, TARGET).is_ok());
        let error = ensure_port_free(
            &serving_config("http://127.0.0.1:9999", true),
            host_port,
            TARGET,
        )
        .unwrap_err();
        assert!(error.to_string().contains("http://127.0.0.1:9999"));
    }

    #[test]
    fn consent_url_is_extracted_from_cli_output() {
        let text =
            "Funnel is not enabled.\nVisit https://login.tailscale.com/f/funnel?node=abc123.";
        assert_eq!(
            consent_url(text).as_deref(),
            Some("https://login.tailscale.com/f/funnel?node=abc123")
        );
        assert_eq!(consent_url("permission denied"), None);
    }

    #[test]
    fn installer_version_uses_amd64_msi_only() {
        let page = r#"<a href="tailscale-setup-1.102.2-arm64.msi">arm</a>
            <a href="tailscale-setup-1.102.3-amd64.msi">x64</a>"#;
        assert_eq!(installer_version(page).unwrap(), "1.102.3");
        assert!(installer_version("tailscale-setup-x-amd64.msi").is_err());
    }

    #[test]
    fn installer_signature_must_be_valid_and_from_tailscale() {
        assert!(signature_is_tailscale(
            "Valid|CN=Tailscale Inc., O=Tailscale Inc., L=Toronto, C=CA"
        ));
        assert!(!signature_is_tailscale("NotSigned|"));
        assert!(!signature_is_tailscale(
            "Valid|CN=Someone Else, O=Someone Else"
        ));
        assert!(!signature_is_tailscale(""));
    }

    #[test]
    fn local_target_brackets_ipv6_loopback() {
        let mut settings = RemoteAccessSettings::default();
        assert_eq!(local_target(&settings), "http://127.0.0.1:45222");
        settings.bind_host = "::1".into();
        assert_eq!(local_target(&settings), "http://[::1]:45222");
    }

    #[test]
    fn funnel_requires_loopback_http_listener() {
        let mut settings = RemoteAccessSettings {
            enabled: true,
            funnel_enabled: true,
            ..RemoteAccessSettings::default()
        };
        assert!(settings.validate().is_ok());
        settings.bind_host = "0.0.0.0".into();
        assert!(settings.validate().is_err());
        settings.bind_host = "127.0.0.1".into();
        settings.tls_certificate_path = "C:\\cert.pem".into();
        settings.tls_private_key_path = "C:\\key.pem".into();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn legacy_cloudflare_settings_do_not_enable_funnel() {
        let settings: RemoteAccessSettings = serde_json::from_str(
            r#"{"enabled":true,"cloudflareTunnelEnabled":true,"cloudflaredPath":"cloudflared"}"#,
        )
        .unwrap();
        assert!(!settings.funnel_enabled);
    }

    #[test]
    fn download_progress_percent_is_bounded() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(300, 200), 100);
    }

    #[tokio::test]
    async fn download_reports_progress_until_content_length() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body: Vec<u8> = (0..96 * 1024).map(|index| (index % 251) as u8).collect();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let payload = body.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            for chunk in payload.chunks(16 * 1024) {
                socket.write_all(chunk).await.unwrap();
            }
            socket.shutdown().await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut seen = Vec::new();
        let (_, bytes) = download(
            &client,
            &format!("http://{address}/tailscale-setup.msi"),
            1024 * 1024,
            |downloaded, total| seen.push((downloaded, total)),
        )
        .await
        .unwrap();
        let total = body.len() as u64;
        assert_eq!(bytes, body);
        assert_eq!(seen.first(), Some(&(0, total)));
        assert_eq!(seen.last(), Some(&(total, total)));
        assert!(seen.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    }
}
