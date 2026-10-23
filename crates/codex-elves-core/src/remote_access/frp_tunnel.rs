use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use axum::Extension;
use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use super::{RemoteAccessController, RemoteAccessSettings};

const VERSION: &str = "0.71.1";
const ARCHIVE_HASH: &str = "b90074b2b6a1d792a7a40f085ac6d276bcab2d1bc60201f8a86ca53ce65ce821";
const PROGRAM_HASH: &str = "23d11640bee87207c7a99d006b84c554df997feeba7ac7243db20a88a9cb5e01";
const MAX_PEM_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct DownloadProgress(Mutex<DownloadSnapshot>);

#[derive(Default)]
struct DownloadSnapshot {
    active: bool,
    downloaded: u64,
    total: Option<u64>,
}

impl DownloadProgress {
    fn begin(&self) -> DownloadGuard<'_> {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = DownloadSnapshot {
            active: true,
            ..Default::default()
        };
        DownloadGuard(self)
    }

    fn update(&self, downloaded: u64, total: Option<u64>) {
        let mut progress = self.0.lock().unwrap_or_else(|error| error.into_inner());
        progress.downloaded = downloaded;
        progress.total = total;
    }

    pub fn status(&self) -> Value {
        let progress = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let percent = progress
            .total
            .filter(|total| *total > 0)
            .map(|total| (progress.downloaded.saturating_mul(100) / total).min(100));
        json!({
            "downloading": progress.active,
            "downloadedBytes": progress.downloaded,
            "downloadTotalBytes": progress.total,
            "downloadPercent": percent,
            "version": VERSION
        })
    }
}

struct DownloadGuard<'a>(&'a DownloadProgress);
impl Drop for DownloadGuard<'_> {
    fn drop(&mut self) {
        self.0
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active = false;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum HttpsMode {
    #[default]
    Provider,
    LocalCertificate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Config {
    pub server_addr: String,
    pub server_port: u16,
    pub remote_port: u16,
    pub public_origin: String,
    pub https_mode: HttpsMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_addr: String::new(),
            server_port: 7000,
            remote_port: 0,
            public_origin: String::new(),
            https_mode: HttpsMode::Provider,
        }
    }
}

impl Config {
    pub fn normalize(&mut self) -> anyhow::Result<()> {
        let address = self.server_addr.trim();
        if !address.is_empty() {
            let url = reqwest::Url::parse(
                if address.contains("://") {
                    address.to_owned()
                } else {
                    format!("http://{address}")
                }
                .as_str(),
            )
            .context("FRP 服务器地址格式无效")?;
            anyhow::ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none(),
                "FRP 服务器地址只支持域名、IP 或 HTTP/HTTPS 地址"
            );
            self.server_addr = url.host_str().context("FRP 服务器地址缺少域名")?.to_owned();
        }
        self.public_origin = self.public_origin.trim().to_owned();
        if self.public_origin.is_empty() && !self.server_addr.is_empty() && self.remote_port > 0 {
            self.public_origin =
                reqwest::Url::parse(&format!("http://{}:{}", self.server_addr, self.remote_port))?
                    .origin()
                    .ascii_serialization();
        }
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.server_addr.is_empty()
                && self.server_addr.len() <= 253
                && self
                    .server_addr
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                && !self.server_addr.starts_with('-')
                && self.server_port > 0
                && self.remote_port > 0,
            "请填写有效的 FRP 服务器地址、服务端口和公网映射端口"
        );
        if !self.public_origin.is_empty() {
            anyhow::ensure!(
                valid_origin(&self.public_origin),
                "FRP 公网地址必须是 HTTP 或 HTTPS 根地址，可包含端口，不能包含路径、账号或参数"
            );
        }
        Ok(())
    }
}

pub(super) fn valid_origin(origin: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.origin().ascii_serialization() == origin
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/"
        && url.host_str().is_some()
        && url.port() != Some(0)
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Saved {
    pub config: Config,
    pub token: String,
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub certificate_name: String,
    pub private_key_name: String,
}

impl Saved {
    pub fn load(directory: &Path, active: bool) -> anyhow::Result<Self> {
        let mut saved: Self = super::read_json_or_default(&config_path(directory, active))?;
        saved.config.normalize()?;
        Ok(saved)
    }

    pub fn store(&self, directory: &Path, active: bool) -> anyhow::Result<()> {
        let path = config_path(directory, active);
        std::fs::create_dir_all(path.parent().unwrap())?;
        crate::settings::atomic_write(&path, &serde_json::to_vec(self)?)
    }

    pub fn public(&self) -> Value {
        json!({
            "config":self.config,
            "tokenConfigured":!self.token.is_empty(),
            "certificateName":self.certificate_name,
            "privateKeyName":self.private_key_name,
            "httpsConfigured":self.https_configured()
        })
    }

    pub fn https_configured(&self) -> bool {
        self.config.public_origin.starts_with("https://")
            && (self.config.https_mode == HttpsMode::Provider
                || (!self.certificate_pem.is_empty() && !self.private_key_pem.is_empty()))
    }

    pub fn from_payload(directory: &Path, payload: Value) -> anyhow::Result<Self> {
        let previous = Self::load(directory, false)?;
        let mut config: Config =
            serde_json::from_value(payload["config"].clone()).context("FRP 配置格式无效")?;
        config.normalize()?;
        config.validate()?;
        let token = payload["token"]
            .as_str()
            .unwrap_or(&previous.token)
            .trim()
            .to_owned();
        anyhow::ensure!(
            !token.is_empty() && token.len() <= 512 && token.bytes().all(|b| b.is_ascii_graphic()),
            "请填写有效的 FRP Token"
        );
        let certificate_pem = payload["certificatePem"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&previous.certificate_pem)
            .to_owned();
        let private_key_pem = payload["privateKeyPem"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&previous.private_key_pem)
            .to_owned();
        anyhow::ensure!(
            certificate_pem.len() <= MAX_PEM_BYTES && private_key_pem.len() <= MAX_PEM_BYTES,
            "证书和私钥分别不能超过 64 KiB"
        );
        let value = Self {
            config,
            token,
            certificate_pem,
            private_key_pem,
            certificate_name: payload["certificateName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&previous.certificate_name)
                .chars()
                .take(160)
                .collect(),
            private_key_name: payload["privateKeyName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&previous.private_key_name)
                .chars()
                .take(160)
                .collect(),
        };
        if value.config.https_mode == HttpsMode::LocalCertificate
            && !value.certificate_pem.is_empty()
            && !value.private_key_pem.is_empty()
        {
            value.tls_config()?;
        }
        Ok(value)
    }

    fn tls_config(&self) -> anyhow::Result<RustlsConfig> {
        let certs =
            rustls::pki_types::CertificateDer::pem_slice_iter(self.certificate_pem.as_bytes())
                .collect::<Result<Vec<_>, _>>()
                .context("无法解析 HTTPS 证书")?;
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(self.private_key_pem.as_bytes())
            .context("无法解析 HTTPS 私钥")?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("HTTPS 证书与私钥无效或不匹配")?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(RustlsConfig::from_config(Arc::new(config)))
    }
}

fn config_path(directory: &Path, active: bool) -> PathBuf {
    directory
        .join("remote-access-frp")
        .join(if active { "active.json" } else { "draft.json" })
}

#[derive(Clone)]
pub(super) struct Ingress(pub String);

pub(super) struct Running {
    pub id: String,
    pub saved: Saved,
    child: Option<Child>,
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
    reader: Option<tokio::task::JoinHandle<()>>,
    server: tokio::task::JoinHandle<()>,
    handle: axum_server::Handle<std::net::SocketAddr>,
    config_path: PathBuf,
}

impl Running {
    pub async fn stop(mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill().await;
        }
    }
    pub fn alive(&mut self) -> bool {
        !self.server.is_finished()
            && self
                .child
                .as_mut()
                .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(reader) = &self.reader {
            reader.abort();
        }
        self.handle.shutdown();
        self.server.abort();
        let _ = std::fs::remove_file(&self.config_path);
    }
}

pub(super) async fn start(
    controller: &Arc<RemoteAccessController>,
    settings: &RemoteAccessSettings,
    saved: Saved,
    https: bool,
) -> anyhow::Result<Running> {
    match start_once(controller, settings, saved.clone(), https).await {
        Ok(running) => Ok(running),
        Err(error) if error.downcast_ref::<StartFailure>() == Some(&StartFailure::PortInUse) => {
            let program =
                ensure_program(&controller.state_dir, &controller.primary.frp_download).await?;
            super::frp_recovery::release_local_mapping(&controller.state_dir, &program, &saved)
                .await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
            start_once(controller, settings, saved, https)
                .await
                .context("FRP 映射自动恢复后仍未启动；请确认该端口未被其他设备占用")
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StartFailure {
    PortInUse,
}
impl std::fmt::Display for StartFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FRP 公网端口被占用，正在核对同一份本机映射")
    }
}
impl std::error::Error for StartFailure {}

fn startup_failure(line: &str) -> Option<anyhow::Error> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("port already used") || lower.contains("port already in use") {
        return Some(StartFailure::PortInUse.into());
    }
    if lower.contains("port not allowed") || lower.contains("port is not allowed") {
        return Some(anyhow::anyhow!(
            "FRP 服务商未授权此公网映射端口；请填写购买范围内的端口或联系服务商，重启不能解除端口限制"
        ));
    }
    if lower.contains("token")
        && (lower.contains("not match")
            || lower.contains("doesn't match")
            || lower.contains("invalid"))
    {
        return Some(anyhow::anyhow!("FRP Token 验证失败，请核对商家提供的令牌"));
    }
    if lower.contains("login to server failed") || lower.contains("start error") {
        return Some(anyhow::anyhow!(
            "FRP 连接失败，请检查服务器和服务端口后重试"
        ));
    }
    None
}

async fn start_once(
    controller: &Arc<RemoteAccessController>,
    settings: &RemoteAccessSettings,
    saved: Saved,
    https: bool,
) -> anyhow::Result<Running> {
    saved.config.validate()?;
    anyhow::ensure!(!saved.token.is_empty(), "请先保存 FRP Token");
    let program = ensure_program(&controller.state_dir, &controller.primary.frp_download).await?;
    let tls = if https && saved.config.https_mode == HttpsMode::LocalCertificate {
        Some(saved.tls_config()?)
    } else {
        None
    };
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let id = uuid::Uuid::new_v4().to_string();
    let router = super::web_router(controller.clone(), settings.clone())
        .layer(Extension(Ingress(id.clone())));
    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();
    let server = if let Some(tls) = tls {
        let server = axum_server::from_tcp_rustls(listener, tls)?.handle(server_handle);
        tokio::spawn(async move {
            let _ = server
                .serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await;
        })
    } else {
        let server = axum_server::from_tcp(listener)?.handle(server_handle);
        tokio::spawn(async move {
            let _ = server
                .serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await;
        })
    };
    let path = controller
        .state_dir
        .join("remote-access-frp")
        .join(format!("run-{id}.toml"));
    let mut running = Running {
        id,
        saved,
        child: None,
        #[cfg(windows)]
        job: None,
        reader: None,
        server,
        handle,
        config_path: path,
    };
    std::fs::create_dir_all(running.config_path.parent().unwrap())?;
    crate::settings::atomic_write(
        &running.config_path,
        client_config(&running.saved, &running.id, port)?.as_bytes(),
    )?;
    let mut command = Command::new(program);
    command
        .arg("-c")
        .arg(&running.config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "HTTP_PROXY_ENV",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "http_proxy_env",
    ] {
        command.env_remove(key);
    }
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let child = command.spawn().context("无法启动 FRP 客户端")?;
    running.child = Some(child);
    #[cfg(windows)]
    {
        running.job = Some(super::zrok_tunnel::child_job(
            running.child.as_ref().unwrap(),
            "frp",
        )?);
    }
    let mut lines = BufReader::new(
        running
            .child
            .as_mut()
            .unwrap()
            .stdout
            .take()
            .context("无法读取 FRP 启动状态")?,
    )
    .lines();
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(line) = lines.next_line().await? {
            if line.contains("start proxy success") {
                return Ok(());
            }
            if let Some(error) = startup_failure(&line) {
                return Err(error);
            }
        }
        anyhow::bail!("FRP 客户端未建立映射")
    })
    .await
    .context("FRP 建立映射超时")??;
    running.reader = Some(tokio::spawn(async move {
        while matches!(lines.next_line().await, Ok(Some(_))) {}
    }));
    Ok(running)
}

fn client_config(saved: &Saved, id: &str, port: u16) -> anyhow::Result<String> {
    let value = json!({
        "serverAddr":saved.config.server_addr, "serverPort":saved.config.server_port,
        "auth":{"method":"token","token":saved.token},
        "transport":{"tls":{"enable":true}},
        "loginFailExit":true,
        "log":{"to":"console","level":"info","disablePrintColor":true},
        "proxies":[{"name":format!("codex-elves-{id}"),"type":"tcp","localIP":"127.0.0.1","localPort":port,"remotePort":saved.config.remote_port}]
    });
    let config: toml::Value = serde_json::from_value(value)?;
    Ok(toml::to_string(&config)?)
}

pub fn program_directory(directory: &Path) -> PathBuf {
    directory.join("tools").join("frp").join(VERSION)
}

async fn ensure_program(directory: &Path, progress: &DownloadProgress) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        cfg!(all(windows, target_arch = "x86_64")),
        "FRP 托管客户端当前支持 Windows x64"
    );
    let path = program_directory(directory).join("frpc.exe");
    if std::fs::read(&path).ok().is_some_and(|bytes| {
        format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(PROGRAM_HASH)
    }) {
        return Ok(path);
    }
    let _download = progress.begin();
    let mut response = reqwest::Client::builder().https_only(true).connect_timeout(Duration::from_secs(20)).timeout(Duration::from_secs(300)).build()?
        .get(format!("https://github.com/fatedier/frp/releases/download/v{VERSION}/frp_{VERSION}_windows_amd64.zip"))
        .send().await?.error_for_status()?;
    let total = response.content_length();
    progress.update(0, total);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 32 * 1024 * 1024,
            "FRP 下载文件过大"
        );
        bytes.extend_from_slice(&chunk);
        progress.update(bytes.len() as u64, total);
    }
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == ARCHIVE_HASH,
        "FRP 下载摘要不匹配"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut entry = archive.by_name(&format!("frp_{VERSION}_windows_amd64/frpc.exe"))?;
    anyhow::ensure!(entry.size() <= 64 * 1024 * 1024, "FRP 程序过大");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(PROGRAM_HASH),
        "FRP 程序摘要不匹配"
    );
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::settings::atomic_write(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frp_download_progress_reports_bytes_and_clears_on_completion_or_failure() {
        let progress = DownloadProgress::default();
        assert_eq!(progress.status()["downloading"], false);
        {
            let _guard = progress.begin();
            assert_eq!(progress.status()["downloading"], true);
            assert!(progress.status()["downloadPercent"].is_null());
            progress.update(25, Some(100));
            assert_eq!(progress.status()["downloadedBytes"], 25);
            assert_eq!(progress.status()["downloadPercent"], 25);
            progress.update(110, Some(100));
            assert_eq!(progress.status()["downloadPercent"], 100);
        }
        assert_eq!(progress.status()["downloading"], false);
        {
            let _guard = progress.begin();
            assert_eq!(progress.status()["downloadedBytes"], 0);
            assert!(progress.status()["downloadTotalBytes"].is_null());
        }
        assert_eq!(progress.status()["downloading"], false);
    }

    #[test]
    fn frp_client_config_preserves_quoted_tokens_and_binds_only_loopback() {
        let saved = Saved {
            config: Config {
                server_addr: "relay.example.com".into(),
                server_port: 39397,
                remote_port: 39399,
                ..Default::default()
            },
            token: "test\"quoted\\token".into(),
            ..Default::default()
        };
        let text = client_config(&saved, "test-proxy", 45232).unwrap();
        let decoded: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            decoded["auth"]["token"].as_str(),
            Some(saved.token.as_str())
        );
        assert_eq!(decoded["transport"]["tls"]["enable"].as_bool(), Some(true));
        assert_eq!(decoded["proxies"][0]["localIP"].as_str(), Some("127.0.0.1"));
        assert_eq!(
            decoded["proxies"][0]["remotePort"].as_integer(),
            Some(39399)
        );
    }

    #[test]
    fn frp_public_origin_requires_an_exact_http_authority() {
        for valid in [
            "http://remote.example.com:39399",
            "https://remote.example.com",
            "https://remote.example.com:39399",
        ] {
            assert!(valid_origin(valid), "{valid}");
        }
        for invalid in [
            "ftp://remote.example.com:39399",
            "https://remote.example.com/",
            "https://remote.example.com/path",
            "https://user:secret@remote.example.com",
            "https://remote.example.com?token=test",
            "https://remote.example.com#x",
            "https://remote.example.com:0",
        ] {
            assert!(!valid_origin(invalid), "{invalid}");
        }
    }

    #[test]
    fn frp_normalizes_pasted_server_urls_and_generates_the_public_address() {
        for input in [
            " relay.example.com ",
            "http://relay.example.com",
            "https://relay.example.com/",
            "http://relay.example.com:39398/static/#/proxies/tcp",
        ] {
            let mut config = Config {
                server_addr: input.into(),
                server_port: 39397,
                remote_port: 39399,
                ..Default::default()
            };
            config.normalize().unwrap();
            config.validate().unwrap();
            assert_eq!(config.server_addr, "relay.example.com");
            assert_eq!(config.server_port, 39397);
            assert_eq!(config.public_origin, "http://relay.example.com:39399");
        }
        for address in [
            "ftp://relay.example.com",
            "https://user:password@relay.example.com",
        ] {
            let mut config = Config {
                server_addr: address.into(),
                ..Default::default()
            };
            assert!(config.normalize().is_err());
        }
    }

    #[test]
    fn frp_start_errors_distinguish_port_conflicts_from_authorization_failures() {
        let conflict = startup_failure("start error: port already used").unwrap();
        assert_eq!(
            conflict.downcast_ref::<StartFailure>(),
            Some(&StartFailure::PortInUse)
        );
        let denied = startup_failure("start error: port not allowed").unwrap();
        assert!(denied.to_string().contains("未授权"));
        assert!(denied.downcast_ref::<StartFailure>().is_none());
        let token =
            startup_failure("login to server failed: token in login doesn't match").unwrap();
        assert!(token.to_string().contains("Token 验证失败"));
        assert!(startup_failure("start proxy success").is_none());
    }

    #[test]
    fn frp_draft_does_not_publish_secrets_or_overwrite_active_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let payload = json!({"config":{"serverAddr":"relay.example.com","serverPort":7000,"remotePort":39401,
            "publicOrigin":"https://entry.example.com:39401","httpsMode":"provider"},"token":"test-frp-secret"});
        let active = Saved::from_payload(directory.path(), payload.clone()).unwrap();
        assert!(active.https_configured()); // Provider HTTPS needs no local private key.
        active.store(directory.path(), true).unwrap();
        let mut changed = payload;
        changed["config"]["remotePort"] = json!(39402);
        changed["token"] = json!("new-draft-secret");
        let draft = Saved::from_payload(directory.path(), changed).unwrap();
        draft.store(directory.path(), false).unwrap();
        assert_eq!(
            Saved::load(directory.path(), true).unwrap().token,
            "test-frp-secret"
        );
        assert_eq!(
            Saved::load(directory.path(), true)
                .unwrap()
                .config
                .remote_port,
            39401
        );
        assert_eq!(
            Saved::load(directory.path(), false).unwrap().token,
            "new-draft-secret"
        );
        let public = draft.public().to_string();
        assert!(!public.contains("new-draft-secret"));
        assert!(!public.contains("privateKeyPem"));
        let mut incomplete = draft;
        incomplete.config.public_origin.clear();
        assert!(!incomplete.https_configured());
        incomplete.config.public_origin = "https://entry.example.com:39402".into();
        incomplete.config.https_mode = HttpsMode::LocalCertificate;
        assert!(!incomplete.https_configured());
    }
}
