use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};

use crate::settings::RemoteAccessSettings;

use super::credentials::{TunnelToken, load_tunnel_token};
use super::now_ms;

const RELEASE_URL: &str = "https://github.com/cloudflare/cloudflared/releases/latest";
const MAX_PROGRAM_BYTES: usize = 100 * 1024 * 1024;
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
pub(super) struct TunnelService {
    child: Option<Child>,
    download: Option<tokio::task::JoinHandle<anyhow::Result<PathBuf>>>,
    token_id: String,
    metrics_port: Option<u16>,
    connected: bool,
    error: Option<String>,
    retry_at_ms: u64,
}

impl TunnelService {
    pub async fn restart(&mut self) {
        self.stop().await;
        self.retry_at_ms = 0;
        self.error = None;
    }

    pub async fn stop(&mut self) {
        if let Some(task) = self.download.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        self.connected = false;
        self.metrics_port = None;
    }

    pub fn status(&self, settings: &RemoteAccessSettings) -> Value {
        json!({
            "enabled":settings.cloudflare_tunnel_enabled,
            "running":self.child.is_some(),
            "connected":self.connected,
            "processId":self.child.as_ref().and_then(Child::id),
            "error":self.error,
            "publicUrl":settings.public_url,
            "state":if self.download.is_some() { "downloading" } else if self.error.is_some() { "error" } else if self.connected {
                "connected"
            } else if self.child.is_some() { "connecting" } else { "stopped" }
        })
    }

    pub async fn reconcile(
        &mut self,
        settings: &RemoteAccessSettings,
        state_dir: &Path,
        web_running: bool,
    ) {
        if !settings.enabled || !settings.cloudflare_tunnel_enabled || !web_running {
            self.stop().await;
            self.token_id.clear();
            self.error = None;
            self.retry_at_ms = 0;
            return;
        }
        let record = match load_tunnel_token(state_dir) {
            Ok(Some(record)) => record,
            Ok(None) => {
                self.stop().await;
                self.token_id.clear();
                self.error = Some("请先在管理器设置 Cloudflare 隧道令牌".into());
                return;
            }
            Err(_) => {
                self.stop().await;
                self.error = Some("Cloudflare 隧道令牌无法读取，请重新设置".into());
                return;
            }
        };
        if self.token_id != record.id {
            self.stop().await;
            self.token_id = record.id.clone();
            self.retry_at_ms = 0;
            self.error = None;
        }
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(code)) => {
                    self.child = None;
                    self.connected = false;
                    self.error = Some(format!(
                        "cloudflared 已退出（{code}），30 秒后重试。请检查令牌、程序版本和网络。"
                    ));
                    self.retry_at_ms = now_ms() + 30_000;
                }
                Err(_) => {
                    self.stop().await;
                    self.error = Some("无法读取 cloudflared 状态，30 秒后重试。".into());
                    self.retry_at_ms = now_ms() + 30_000;
                }
            }
        }
        if self.child.is_none() && now_ms() >= self.retry_at_ms {
            let program = if automatic_program(&settings.cloudflared_path) {
                if self.download.is_none() {
                    let directory = state_dir.to_path_buf();
                    self.error = None;
                    self.download =
                        Some(tokio::spawn(
                            async move { ensure_cloudflared(&directory).await },
                        ));
                }
                if !self
                    .download
                    .as_ref()
                    .is_some_and(|task| task.is_finished())
                {
                    return;
                }
                match self.download.take().unwrap().await {
                    Ok(Ok(path)) => path,
                    result => {
                        self.error = Some(match result {
                            Ok(Err(error)) => error.to_string(),
                            Err(_) => "cloudflared 下载任务未完成，请重新应用设置".into(),
                            _ => unreachable!(),
                        });
                        self.retry_at_ms = now_ms() + 30_000;
                        return;
                    }
                }
            } else {
                PathBuf::from(settings.cloudflared_path.trim())
            };
            match self.start(state_dir, record, &program).await {
                Ok(()) => self.error = None,
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.retry_at_ms = now_ms() + 30_000;
                }
            }
        }
        if let Some(port) = self.metrics_port.filter(|_| self.child.is_some()) {
            self.connected = match reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
            {
                Ok(client) => client
                    .get(format!("http://127.0.0.1:{port}/ready"))
                    .send()
                    .await
                    .is_ok_and(|response| response.status().is_success()),
                Err(_) => false,
            };
        }
    }

    async fn start(
        &mut self,
        state_dir: &Path,
        record: TunnelToken,
        program: &Path,
    ) -> anyhow::Result<()> {
        let token = record.reveal()?;
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").context("无法分配本机隧道状态端口")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let config = state_dir.join("remote-access-cloudflared.yml");
        crate::settings::atomic_write(&config, b"{}\n")?;
        let mut command = Command::new(program);
        command
            .arg("--no-autoupdate")
            .arg("tunnel")
            .arg("--config")
            .arg(&config)
            .arg("--metrics")
            .arg(format!("127.0.0.1:{port}"))
            .arg("run")
            .env("TUNNEL_TOKEN", token)
            .env_remove("TUNNEL_TOKEN_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        self.child = Some(command.spawn().context(
            "无法启动 cloudflared，请下载官方程序或核对自定义路径；本机网页服务仍可使用",
        )?);
        self.metrics_port = Some(port);
        self.connected = false;
        Ok(())
    }
}

pub(super) fn automatic_program(path: &str) -> bool {
    path.trim().is_empty() || path.trim() == "cloudflared"
}

#[derive(Serialize, Deserialize)]
pub(super) struct ProgramRelease {
    pub(super) version: String,
    pub(super) sha256: String,
}

impl ProgramRelease {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.version.is_empty()
                && self.version.len() < 64
                && self
                    .version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "官方 cloudflared 版本或 SHA-256 信息无效，已拒绝使用程序"
        );
        Ok(())
    }

    pub(super) fn path(&self, state_dir: &Path) -> PathBuf {
        state_dir.join("tools/cloudflared").join(format!(
            "cloudflared-{}-{}.exe",
            self.version,
            self.sha256.to_ascii_lowercase()
        ))
    }
}

pub(super) fn parse_release(
    url: &reqwest::Url,
    page: &str,
    asset: &str,
) -> anyhow::Result<ProgramRelease> {
    anyhow::ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.query().is_none()
            && url.fragment().is_none(),
        "cloudflared 发布信息不是官方地址，已拒绝下载"
    );
    let version = url
        .path()
        .strip_prefix("/cloudflare/cloudflared/releases/tag/")
        .context("无法获取 cloudflared 官方版本，请稍后重试")?;
    let marker = format!("{asset}:");
    let checksum = page
        .split_once(&marker)
        .context("官方发布页缺少 SHA-256 校验信息，已拒绝下载")?
        .1
        .trim_start();
    let sha256: String = checksum
        .chars()
        .take_while(|value| value.is_ascii_hexdigit())
        .collect();
    let release = ProgramRelease {
        version: version.into(),
        sha256,
    };
    release.validate()?;
    Ok(release)
}

pub(super) fn managed_program(
    state_dir: &Path,
) -> anyhow::Result<Option<(PathBuf, ProgramRelease)>> {
    let metadata = state_dir.join("tools/cloudflared/installed.json");
    let bytes = match std::fs::read(metadata) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("无法读取托管 cloudflared 信息"),
    };
    let release: ProgramRelease =
        serde_json::from_slice(&bytes).context("托管 cloudflared 信息损坏，请重新下载")?;
    release.validate()?;
    let path = release.path(state_dir);
    let bytes = std::fs::read(&path).context("托管 cloudflared 程序缺失，请重新下载")?;
    anyhow::ensure!(
        bytes.len() <= MAX_PROGRAM_BYTES
            && format!("{:x}", Sha256::digest(&bytes)) == release.sha256.to_ascii_lowercase(),
        "托管 cloudflared 校验失败，请重新下载官方程序"
    );
    Ok(Some((path, release)))
}

pub(super) fn program_status(state_dir: &Path) -> Value {
    match managed_program(state_dir) {
        Ok(Some((path, release))) => json!({"status":"ok","installed":true,
            "path":path,"version":format!("cloudflared version {}", release.version)}),
        Ok(None) => json!({"status":"ok","installed":false}),
        Err(error) => json!({"status":"ok","installed":false,"message":error.to_string()}),
    }
}

async fn response_bytes(mut response: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        response.content_length().unwrap_or(0) <= limit as u64,
        "官方下载内容过大，已取消下载"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("官方程序下载中断，请稍后重试")?
    {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "官方下载内容过大，已取消下载"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn ensure_cloudflared(state_dir: &Path) -> anyhow::Result<PathBuf> {
    let _guard = DOWNLOAD_LOCK.lock().await;
    if let Ok(Some((path, _))) = managed_program(state_dir) {
        return Ok(path);
    }
    let asset = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "cloudflared-windows-amd64.exe"
    } else if cfg!(all(target_os = "windows", target_arch = "x86")) {
        "cloudflared-windows-386.exe"
    } else {
        anyhow::bail!(
            "此平台请使用官方 cloudflared 并填写自定义程序路径；自动下载支持 Windows x64 / x86"
        );
    };
    let client = reqwest::Client::builder()
        .https_only(true)
        .user_agent("CodexElves-cloudflared")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()?;
    let response = client
        .get(RELEASE_URL)
        .send()
        .await
        .context("无法获取官方 cloudflared，请检查网络后重试下载或填写自定义程序路径")?
        .error_for_status()
        .context("官方 cloudflared 发布页暂不可用，请稍后重试")?;
    let url = response.url().clone();
    let page = String::from_utf8(response_bytes(response, 2 * 1024 * 1024).await?)
        .context("官方 cloudflared 发布信息编码无效")?;
    let release = parse_release(&url, &page, asset)?;
    let response = client
        .get(format!(
            "https://github.com/cloudflare/cloudflared/releases/download/{}/{asset}",
            release.version
        ))
        .send()
        .await
        .context("官方 cloudflared 下载失败，请检查网络后重试")?
        .error_for_status()
        .context("官方 cloudflared 下载暂不可用，请稍后重试")?;
    anyhow::ensure!(
        matches!(
            response.url().host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        ),
        "cloudflared 下载不是官方资产地址，已拒绝使用"
    );
    let bytes = response_bytes(response, MAX_PROGRAM_BYTES).await?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == release.sha256.to_ascii_lowercase(),
        "官方 cloudflared 的 SHA-256 不匹配，已拒绝保存和运行"
    );
    let path = release.path(state_dir);
    crate::settings::atomic_write(&path, &bytes).context("无法保存官方 cloudflared 程序")?;
    check_cloudflared(&path.to_string_lossy()).await?;
    crate::settings::atomic_write(
        &state_dir.join("tools/cloudflared/installed.json"),
        &serde_json::to_vec(&release)?,
    )?;
    Ok(path)
}

pub async fn check_cloudflared(path: &str) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !path.trim().is_empty() && !path.contains('\0'),
        "请填写 cloudflared 程序路径"
    );
    let mut command = Command::new(path.trim());
    command
        .arg("--version")
        .env_remove("TUNNEL_TOKEN")
        .env_remove("TUNNEL_TOKEN_FILE")
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .context("检查 cloudflared 超时")?
        .context("未找到 cloudflared，请安装后填写程序路径")?;
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.lines().next().unwrap_or("").trim();
    anyhow::ensure!(
        output.status.success()
            && version.starts_with("cloudflared version")
            && version.len() < 200,
        "该程序未返回有效的 cloudflared 版本"
    );
    Ok(json!({"status":"ok","version":version}))
}
