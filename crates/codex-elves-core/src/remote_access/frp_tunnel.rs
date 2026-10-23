use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
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

const VERSION: &str = "0.54.0";
const ARCHIVE_HASH: &str = "95f0d8c8f4781fc8e42b7d644024c647032e3f6cd0ffe425e8f7d5a46d601557";
const PROGRAM_HASH: &str = "15846490706281424a0f5b0b7d3e6ea92173dbeaf6a10a566f4dc8a6a1e977bb";
const MAX_PEM_BYTES: usize = 64 * 1024;

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
                "FRP 公网地址必须是 HTTPS 根地址，可包含端口，不能包含路径、账号或参数"
            );
        }
        Ok(())
    }
}

pub(super) fn valid_origin(origin: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    url.scheme() == "https"
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
        super::read_json_or_default(&config_path(directory, active))
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
        !self.config.public_origin.is_empty()
            && (self.config.https_mode == HttpsMode::Provider
                || (!self.certificate_pem.is_empty() && !self.private_key_pem.is_empty()))
    }

    pub fn from_payload(directory: &Path, payload: Value) -> anyhow::Result<Self> {
        let previous = Self::load(directory, false)?;
        let config: Config =
            serde_json::from_value(payload["config"].clone()).context("FRP 配置格式无效")?;
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
    saved.config.validate()?;
    anyhow::ensure!(!saved.token.is_empty(), "请先保存 FRP Token");
    let program = ensure_program(&controller.state_dir).await?;
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
            if line.contains("start proxy success") { return Ok(()); }
            if line.contains("port already used") || line.contains("port already in use") {
                anyhow::bail!("FRP 公网端口已被占用；请先在原客户端释放该映射或使用其他端口，程序不会停止外部客户端");
            }
            if line.contains("start error") || line.contains("login to server failed") {
                anyhow::bail!("FRP 连接失败，请检查服务器、Token、端口许可或已有映射");
            }
        }
        anyhow::bail!("FRP 客户端未建立映射")
    }).await.context("FRP 建立映射超时")??;
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

async fn ensure_program(directory: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        cfg!(all(windows, target_arch = "x86_64")),
        "FRP 托管客户端当前支持 Windows x64"
    );
    let path = directory.join(format!("tools/frp/{VERSION}/frpc.exe"));
    if std::fs::read(&path).ok().is_some_and(|bytes| {
        format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(PROGRAM_HASH)
    }) {
        return Ok(path);
    }
    let mut response = reqwest::Client::builder().https_only(true).timeout(Duration::from_secs(75)).build()?
        .get(format!("https://github.com/fatedier/frp/releases/download/v{VERSION}/frp_{VERSION}_windows_amd64.zip"))
        .send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 32 * 1024 * 1024,
            "FRP 下载文件过大"
        );
        bytes.extend_from_slice(&chunk);
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
    fn frp_https_origin_requires_an_exact_https_authority() {
        for valid in [
            "https://remote.example.com",
            "https://remote.example.com:39399",
        ] {
            assert!(valid_origin(valid), "{valid}");
        }
        for invalid in [
            "http://remote.example.com:39399",
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
