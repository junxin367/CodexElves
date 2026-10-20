use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

// Pinned official distribution; its ngrok, Inc. Authenticode signature was verified.
const DOWNLOAD_URL: &str =
    "https://bin.equinox.io/c/bNyj1mQVY4c/ngrok-v3-3.39.11-windows-amd64.zip";
const ARCHIVE_HASH: &str = "699bbf1932ec43a573b764bd03e6568efa2c4e45955eb3cc2089c19bb4be4464";
const PROGRAM_HASH: &str = "d339bcbd0713233337e860163f5249eea679cf26750a5700510dbc241d201748";
const CONFIG_FILE: &str = "remote-access-ngrok.json";

struct Running {
    child: Child,
    #[cfg(windows)]
    _job: std::os::windows::io::OwnedHandle,
    reader: tokio::task::JoinHandle<()>,
    origin: String,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.reader.abort();
    }
}

pub(super) struct NgrokTunnelService {
    directory: PathBuf,
    origin: Arc<RwLock<Option<String>>>,
    running: Option<Running>,
    starting: Option<tokio::task::JoinHandle<anyhow::Result<Running>>>,
    target: String,
    error: Option<String>,
    retry_at: u64,
}

impl NgrokTunnelService {
    pub(super) fn new(directory: PathBuf, origin: Arc<RwLock<Option<String>>>) -> Self {
        Self {
            directory,
            origin,
            running: None,
            starting: None,
            target: String::new(),
            error: None,
            retry_at: 0,
        }
    }

    pub(super) fn status(&self) -> Value {
        json!({
            "configured": self.directory.join(CONFIG_FILE).is_file(),
            "state": if self.starting.is_some() { "starting" } else if self.running.is_some() { "ready" }
                else if self.error.is_some() { "error" } else { "stopped" },
            "error": self.error,
        })
    }

    pub(super) async fn stop(&mut self) {
        if let Some(task) = self.starting.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(mut process) = self.running.take() {
            let _ = process.child.kill().await;
            let _ = process.child.wait().await;
        }
        *self.origin.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub(super) async fn reconcile(&mut self, enabled: bool, target: &str) {
        if !enabled || self.target != target {
            self.stop().await;
            self.target = target.to_owned();
            self.retry_at = 0;
        }
        if !enabled {
            return;
        }
        if self
            .running
            .as_mut()
            .is_some_and(|process| !matches!(process.child.try_wait(), Ok(None)))
        {
            self.stop().await;
            self.error = Some("ngrok 连接已结束，正在等待重试".into());
            self.retry_at = super::now_ms() + 30_000;
        }
        if self
            .starting
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            let result = self.starting.take().unwrap().await;
            match result {
                Ok(Ok(process)) => {
                    *self.origin.write().unwrap_or_else(|e| e.into_inner()) =
                        Some(process.origin.clone());
                    self.running = Some(process);
                    self.error = None;
                }
                _ => {
                    self.error =
                        Some("ngrok 启动失败，请检查 Token、账号额度及网络后重新验证".into());
                    self.retry_at = super::now_ms() + 30_000;
                }
            }
        }
        if self.running.is_none()
            && self.starting.is_none()
            && super::now_ms() >= self.retry_at
            && self.directory.join(CONFIG_FILE).is_file()
        {
            let directory = self.directory.clone();
            let target = target.to_owned();
            self.starting = Some(tokio::spawn(async move {
                tokio::time::timeout(Duration::from_secs(100), start(&directory, &target))
                    .await
                    .context("ngrok 启动超时")?
            }));
        }
    }

    pub(super) async fn save_and_test(
        &mut self,
        token: &str,
        target: &str,
    ) -> anyhow::Result<Value> {
        let token = token.trim();
        anyhow::ensure!(
            (20..=512).contains(&token.len())
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "ngrok Token 格式无效"
        );
        // Separate local configuration; never returned by status or discovery.
        crate::settings::atomic_write(
            &self.directory.join(CONFIG_FILE),
            &serde_json::to_vec(&json!({"version":"3","agent":{
                "authtoken":token, "web_addr":false, "console_ui":false,
                "update_check":false, "remote_management":false
            }}))?,
        )?;
        self.stop().await;
        self.retry_at = 0;
        self.reconcile(true, target).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(105);
        while self.starting.is_some() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            self.reconcile(true, target).await;
        }
        let origin = self
            .running
            .as_ref()
            .map(|p| p.origin.clone())
            .context("Token 已保存，但 ngrok 连接失败，请检查账号与网络后重试")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()?;
        let response = client
            .get(format!("{origin}/api/login-status"))
            .header("ngrok-skip-browser-warning", "1")
            .send()
            .await
            .context("Token 已保存，但 ngrok 公网连接测试失败")?;
        anyhow::ensure!(
            response.status().is_success() && response.json::<Value>().await?["ok"] == true,
            "ngrok 未返回本机远程服务"
        );
        let protected = client
            .get(format!("{origin}/api/session"))
            .header("ngrok-skip-browser-warning", "1")
            .send()
            .await?;
        anyhow::ensure!(
            protected.status() == reqwest::StatusCode::UNAUTHORIZED,
            "ngrok 授权边界验证失败"
        );
        Ok(json!({"status":"ok","message":"ngrok Token 已保存，公网连接及未授权访问测试通过。"}))
    }
}

pub(super) fn valid_origin(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/"
        && url.host_str().is_some_and(|host| {
            [
                ".ngrok-free.app",
                ".ngrok-free.dev",
                ".ngrok.app",
                ".ngrok.dev",
                ".ngrok.io",
            ]
            .iter()
            .any(|suffix| {
                host.strip_suffix(suffix).is_some_and(|name| {
                    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            })
        })
}

async fn ensure_program(directory: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        cfg!(all(windows, target_arch = "x86_64")),
        "ngrok 托管下载当前支持 Windows x64"
    );
    let path = directory.join("tools/ngrok/3.39.11/ngrok.exe");
    if let Ok(bytes) = std::fs::read(&path)
        && format!("{:x}", Sha256::digest(&bytes)) == PROGRAM_HASH
    {
        return Ok(path);
    }
    let mut response = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(75))
        .build()?
        .get(DOWNLOAD_URL)
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 64 * 1024 * 1024,
            "ngrok 下载文件过大"
        );
        bytes.extend_from_slice(&chunk);
    }
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == ARCHIVE_HASH,
        "ngrok 下载摘要不匹配"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut entry = archive.by_name("ngrok.exe")?;
    anyhow::ensure!(entry.size() <= 128 * 1024 * 1024, "ngrok 程序过大");
    let mut program = Vec::new();
    entry.read_to_end(&mut program)?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&program)) == PROGRAM_HASH,
        "ngrok 程序摘要不匹配"
    );
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::settings::atomic_write(&path, &program)?;
    Ok(path)
}

async fn start(directory: &Path, target: &str) -> anyhow::Result<Running> {
    let program = ensure_program(directory).await?;
    let mut command = Command::new(program);
    command
        .args(["http", target, "--config"])
        .arg(directory.join(CONFIG_FILE))
        .args(["--inspect=false", "--log=stdout", "--log-format=json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let mut child = command.spawn().context("无法启动 ngrok")?;
    #[cfg(windows)]
    let job = super::zrok_tunnel::child_job(&child, "ngrok")?;
    let mut lines = BufReader::new(child.stdout.take().context("无法读取 ngrok 状态")?).lines();
    while let Some(line) = lines.next_line().await? {
        // Parse only structured endpoint readiness. No raw agent output is retained.
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && let Some(origin) = value["url"].as_str().filter(|url| valid_origin(url))
        {
            let origin = origin.trim_end_matches('/').to_owned();
            let reader =
                tokio::spawn(
                    async move { while matches!(lines.next_line().await, Ok(Some(_))) {} },
                );
            return Ok(Running {
                child,
                #[cfg(windows)]
                _job: job,
                reader,
                origin,
            });
        }
    }
    anyhow::bail!("ngrok 未建立 HTTPS 入口")
}
