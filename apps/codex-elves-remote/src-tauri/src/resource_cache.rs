use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

mod bundled {
    include!(concat!(env!("OUT_DIR"), "/bundled_resources.rs"));
}

// 用户要求与 Android 一样校验并缓存公开网页资源；单文件 12MiB、整包 32MiB、最多 512 项，与 Android 清单限制同步。
const MAX_FILE: usize = 12 * 1024 * 1024;
const MAX_TOTAL: u64 = 32 * 1024 * 1024;
const MAX_MANIFEST: usize = 1024 * 1024;
const BUILTIN: &str = "builtin";
// 内置回退沿用服务端公开页面的 CSP，保留双线路媒体与请求白名单；须与 remote_access.rs 的安全响应头同步核对。
const DEFAULT_HEADERS: &str = concat!(
    "X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n",
    "Content-Security-Policy: default-src 'self'; script-src 'self'; worker-src 'self'; style-src 'self'; ",
    "img-src 'self' data: https://*.ngrok-free.app https://*.ngrok-free.dev https://*.ngrok.app https://*.ngrok.dev https://*.ngrok.io https://*.shares.zrok.io; ",
    "media-src 'self' https://*.ngrok-free.app https://*.ngrok-free.dev https://*.ngrok.app https://*.ngrok.dev https://*.ngrok.io https://*.shares.zrok.io; ",
    "connect-src 'self' https://*.ngrok-free.app wss://*.ngrok-free.app https://*.ngrok-free.dev wss://*.ngrok-free.dev https://*.ngrok.app wss://*.ngrok.app https://*.ngrok.dev wss://*.ngrok.dev https://*.ngrok.io wss://*.ngrok.io https://*.shares.zrok.io wss://*.shares.zrok.io; ",
    "object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'\r\n"
);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub min_shell_version: u32,
    pub revision: u64,
    pub version: String,
    pub files: Vec<Resource>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub path: String,
    pub content_type: String,
    pub size: u64,
    pub sha256: String,
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn valid_path(path: &str) -> bool {
    path == "/"
        || path.starts_with('/')
            && !path.ends_with('/')
            && path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
            && path
                .split('/')
                .skip(1)
                .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
            && path != "/api"
            && !path.starts_with("/api/")
            && !matches!(
                path,
                "/native.js"
                    | "/app-resources.json"
                    | "/cache-recovery.html"
                    | "/cache-recovery.js"
            )
}
fn manifest_version(files: &[Resource]) -> String {
    let mut digest = Sha256::new();
    for file in files {
        for field in [file.path.as_bytes(), file.content_type.as_bytes()] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field);
        }
        digest.update(file.size.to_be_bytes());
        digest.update((file.sha256.len() as u64).to_be_bytes());
        digest.update(file.sha256.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        ensure!(bytes.len() <= MAX_MANIFEST, "网页资源清单过大");
        let value: Self = serde_json::from_slice(bytes).context("网页资源清单格式无效")?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.schema_version == 1 && self.min_shell_version == 1 && self.revision > 0,
            "网页资源协议不兼容，请更新客户端"
        );
        ensure!(
            valid_hash(&self.version) && !self.files.is_empty() && self.files.len() <= 512,
            "网页资源清单版本或数量无效"
        );
        let mut previous = "";
        let mut total = 0_u64;
        for file in &self.files {
            ensure!(
                valid_path(&file.path)
                    && file.path.as_str() > previous
                    && valid_hash(&file.sha256)
                    && file.size <= MAX_FILE as u64
                    && matches!(
                        file.content_type.as_str(),
                        "text/html; charset=utf-8"
                            | "text/css; charset=utf-8"
                            | "application/javascript; charset=utf-8"
                            | "application/manifest+json; charset=utf-8"
                            | "image/png"
                    ),
                "网页资源清单包含无效或保留条目"
            );
            previous = &file.path;
            total = total.checked_add(file.size).context("网页资源总大小无效")?;
        }
        ensure!(
            total <= MAX_TOTAL && self.files[0].path == "/",
            "网页资源缺少首页或超过大小限制"
        );
        ensure!(
            manifest_version(&self.files) == self.version,
            "网页资源清单摘要不匹配"
        );
        Ok(())
    }
}

#[derive(Clone)]
struct Snapshot {
    manifest: Arc<Manifest>,
    files: BTreeMap<String, Arc<[u8]>>,
    headers: String,
}

impl Snapshot {
    fn matches(&self, resource: &Resource) -> Option<Arc<[u8]>> {
        self.manifest
            .files
            .iter()
            .find(|entry| *entry == resource)?;
        self.files.get(&resource.path).cloned()
    }
    fn bundled() -> Self {
        let mut files = BTreeMap::new();
        let entries: Vec<_> = bundled::bundled_files()
            .into_iter()
            .map(|(path, mime, bytes)| {
                files.insert(path.to_owned(), Arc::from(bytes));
                Resource {
                    path: path.into(),
                    content_type: mime.into(),
                    size: bytes.len() as u64,
                    sha256: hex(bytes),
                }
            })
            .collect();
        let manifest = Manifest {
            schema_version: 1,
            min_shell_version: 1,
            revision: bundled::BUNDLED_REVISION,
            version: manifest_version(&entries),
            files: entries,
        };
        manifest.validate().expect("invalid built-in web resources");
        Self {
            manifest: Arc::new(manifest),
            files,
            headers: DEFAULT_HEADERS.into(),
        }
    }
    fn with_headers(&self, headers: &str) -> Arc<Self> {
        Arc::new(Self {
            manifest: self.manifest.clone(),
            files: self.files.clone(),
            headers: headers.into(),
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Index {
    schema: u8,
    origin: String,
    active: String,
    previous: Option<String>,
    headers: String,
}

struct Session {
    origin: String,
    epoch: u64,
    current: Arc<Snapshot>,
    next: Arc<Snapshot>,
}

pub struct CachedResponse {
    pub bytes: Arc<[u8]>,
    pub headers: String,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub status: &'static str,
    pub version: Option<String>,
    pub message: &'static str,
}

pub struct ResourceCache {
    directory: PathBuf,
    builtin: Arc<Snapshot>,
    session: RwLock<Option<Session>>,
    epoch: AtomicU64,
    enabled: AtomicBool,
    busy: AtomicBool,
    update_failed: AtomicBool,
    update_lock: tokio::sync::Mutex<()>,
    network: reqwest::Client,
    loopback: reqwest::Client,
}

fn client(no_proxy: bool) -> anyhow::Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("CodexElvesRemote/", env!("CARGO_PKG_VERSION")));
    Ok(if no_proxy {
        builder.no_proxy()
    } else {
        builder
    }
    .build()?)
}

fn loopback(url: &Url) -> bool {
    matches!(url.host(), Some(url::Host::Domain("localhost")))
        || matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback())
}

fn response_headers(response: &reqwest::Response) -> String {
    let mut headers = String::new();
    for name in [
        "content-security-policy",
        "x-frame-options",
        "x-content-type-options",
        "referrer-policy",
    ] {
        if let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 8192 && !value.contains(['\r', '\n']))
        {
            headers.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    if headers.is_empty() {
        DEFAULT_HEADERS.into()
    } else {
        headers
    }
}

async fn request(
    client: &reqwest::Client,
    url: Url,
    limit: usize,
) -> anyhow::Result<(Vec<u8>, String)> {
    let mut response = client
        .get(url)
        .header("skip_zrok_interstitial", "1")
        .header("ngrok-skip-browser-warning", "true")
        .header("Accept-Encoding", "gzip")
        .send()
        .await?;
    ensure!(response.status().is_success(), "无法取得网页资源");
    let encoded_limit = limit.saturating_add(64 * 1024);
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= encoded_limit as u64),
        "网页资源响应过大"
    );
    let gzip = match response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
    {
        None | Some("identity") => false,
        Some("gzip") => true,
        _ => anyhow::bail!("网页资源编码不受支持"),
    };
    let headers = response_headers(&response);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= encoded_limit,
            "网页资源响应过大"
        );
        bytes.extend_from_slice(&chunk);
    }
    if gzip {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(limit as u64 + 1)
            .read_to_end(&mut decoded)?;
        bytes = decoded;
    }
    ensure!(bytes.len() <= limit, "解压后的网页资源响应过大");
    Ok((bytes, headers))
}

pub async fn fetch_manifest(
    client: &reqwest::Client,
    origin: &Url,
) -> anyhow::Result<(Manifest, String)> {
    let (bytes, headers) =
        request(client, origin.join("app-resources.json")?, MAX_MANIFEST).await?;
    Ok((Manifest::parse(&bytes)?, headers))
}

fn write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let parent = path.parent().context("网页缓存目录无效")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temporary, value)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn read_limited(path: &Path, limit: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "网页缓存文件超过限制");
    Ok(bytes)
}

fn valid_headers(headers: &str) -> bool {
    headers.len() <= 40 * 1024
        && headers
            .split("\r\n")
            .filter(|line| !line.is_empty())
            .all(|line| {
                !line.contains('\n')
                    && !line.contains('\r')
                    && line.split_once(": ").is_some_and(|(name, _)| {
                        matches!(
                            name.to_ascii_lowercase().as_str(),
                            "content-security-policy"
                                | "x-frame-options"
                                | "x-content-type-options"
                                | "referrer-policy"
                        )
                    })
            })
}

fn remove_version(root: &Path, version: &str) -> anyhow::Result<()> {
    ensure!(valid_hash(version), "缓存清理目标无效");
    let target = root.join(version);
    let info = std::fs::symlink_metadata(&target)?;
    ensure!(!info.file_type().is_symlink(), "缓存目录不能是链接");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            info.file_attributes() & 0x400 == 0,
            "缓存目录不能是重解析点"
        );
    }
    // Windows 递归清理前核对最终绝对路径，只删除此服务缓存目录内的摘要版本，不接触设置、登录与其他目录。
    ensure!(
        target.canonicalize()?.parent() == Some(root.canonicalize()?.as_path()),
        "缓存清理目标越界"
    );
    std::fs::remove_dir_all(target)?;
    Ok(())
}

impl ResourceCache {
    pub fn new(directory: PathBuf) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            directory,
            builtin: Arc::new(Snapshot::bundled()),
            session: RwLock::new(None),
            epoch: AtomicU64::new(0),
            enabled: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            update_failed: AtomicBool::new(false),
            update_lock: tokio::sync::Mutex::new(()),
            network: client(false)?,
            loopback: client(true)?,
        }))
    }
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }
    pub fn set_busy(&self, value: bool) {
        self.busy.store(value, Ordering::SeqCst);
    }
    fn origin_root(&self, origin: &str) -> PathBuf {
        self.directory.join(hex(origin.as_bytes()))
    }
    fn http(&self, url: &Url) -> &reqwest::Client {
        if loopback(url) {
            &self.loopback
        } else {
            &self.network
        }
    }
    fn index(&self, origin: &str) -> Option<Index> {
        let bytes = read_limited(&self.origin_root(origin).join("index.json"), 128 * 1024).ok()?;
        let value: Index = serde_json::from_slice(&bytes).ok()?;
        (value.schema == 1
            && value.origin == origin
            && valid_headers(&value.headers)
            && (value.active == BUILTIN || valid_hash(&value.active))
            && value
                .previous
                .as_deref()
                .is_none_or(|id| id == BUILTIN || valid_hash(id)))
        .then_some(value)
    }
    fn load(&self, origin: &str, version: &str, headers: &str) -> anyhow::Result<Arc<Snapshot>> {
        if version == BUILTIN {
            return Ok(self.builtin.with_headers(headers));
        }
        ensure!(valid_hash(version), "网页缓存版本无效");
        let root = self.origin_root(origin).join(version);
        let manifest = Manifest::parse(&read_limited(&root.join("manifest.json"), MAX_MANIFEST)?)?;
        ensure!(manifest.version == version, "网页缓存清单身份不匹配");
        let mut files = BTreeMap::new();
        for (position, entry) in manifest.files.iter().enumerate() {
            // 文件使用短序号名，完整 SHA-256 留在清单中校验，避免 Windows 用户目录叠加三层摘要后超长。
            let bytes = read_limited(
                &root.join(format!("{position:03}.bin")),
                entry.size as usize,
            )?;
            ensure!(
                bytes.len() as u64 == entry.size && hex(&bytes) == entry.sha256,
                "网页缓存内容损坏"
            );
            files.insert(entry.path.clone(), Arc::from(bytes));
        }
        Ok(Arc::new(Snapshot {
            manifest: Arc::new(manifest),
            files,
            headers: headers.into(),
        }))
    }
    pub async fn prepare(
        self: &Arc<Self>,
        url: &Url,
        previously_connected: bool,
    ) -> anyhow::Result<()> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }
        crate::settings::normalize_server_url(url.as_str())?;
        let origin = url.origin().ascii_serialization();
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.busy.store(false, Ordering::SeqCst);
        let mut fresh = None;
        let (index, snapshot) = if let Some(index) = self.index(&origin) {
            let cached = self
                .load(&origin, &index.active, &index.headers)
                .ok()
                .or_else(|| {
                    index
                        .previous
                        .as_deref()
                        .and_then(|id| self.load(&origin, id, &index.headers).ok())
                });
            let snapshot = cached
                .filter(|value| value.manifest.revision >= self.builtin.manifest.revision)
                .unwrap_or_else(|| self.builtin.with_headers(&index.headers));
            (index, snapshot)
        } else {
            // 首次地址必须经过公开清单验证，不能把输错地址或其他网站伪装成已连接；之后优先本地打开，不重复点连接。
            let fetched = if previously_connected {
                None
            } else {
                Some(
                    fetch_manifest(self.http(url), url)
                        .await
                        .context("无法确认远程服务地址，请检查服务是否启动")?,
                )
            };
            let headers = fetched
                .as_ref()
                .map(|(_, headers)| headers.clone())
                .unwrap_or_else(|| DEFAULT_HEADERS.into());
            let index = Index {
                schema: 1,
                origin: origin.clone(),
                active: BUILTIN.into(),
                previous: None,
                headers,
            };
            write_json(&self.origin_root(&origin).join("index.json"), &index)?;
            fresh = fetched;
            let snapshot = self.builtin.with_headers(&index.headers);
            (index, snapshot)
        };
        let _ = index;
        ensure!(self.epoch.load(Ordering::SeqCst) == epoch, "连接已切换");
        *self.session.write().unwrap() = Some(Session {
            origin: origin.clone(),
            epoch,
            current: snapshot.clone(),
            next: snapshot,
        });
        let cache = self.clone();
        let url = url.clone();
        tokio::spawn(async move {
            let mut first = fresh;
            loop {
                let _ = cache.update(&url, epoch, first.take()).await;
                tokio::time::sleep(Duration::from_secs(5 * 60)).await;
                if cache.epoch.load(Ordering::SeqCst) != epoch {
                    break;
                }
            }
        });
        Ok(())
    }
    async fn idle(&self, epoch: u64) -> anyhow::Result<()> {
        while self.busy.load(Ordering::SeqCst) {
            ensure!(self.epoch.load(Ordering::SeqCst) == epoch, "连接已切换");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        ensure!(self.epoch.load(Ordering::SeqCst) == epoch, "连接已切换");
        Ok(())
    }
    async fn update(
        &self,
        url: &Url,
        epoch: u64,
        fetched: Option<(Manifest, String)>,
    ) -> anyhow::Result<()> {
        let Ok(_guard) = self.update_lock.try_lock() else {
            return Ok(());
        };
        let result = self.update_inner(url, epoch, fetched).await;
        if self.epoch.load(Ordering::SeqCst) == epoch {
            self.update_failed.store(result.is_err(), Ordering::SeqCst);
        }
        result
    }
    async fn update_inner(
        &self,
        url: &Url,
        epoch: u64,
        fetched: Option<(Manifest, String)>,
    ) -> anyhow::Result<()> {
        self.idle(epoch).await?;
        let (manifest, headers) = match fetched {
            Some(value) => value,
            None => fetch_manifest(self.http(url), url).await?,
        };
        let origin = url.origin().ascii_serialization();
        let old = self
            .session
            .read()
            .unwrap()
            .as_ref()
            .filter(|session| session.epoch == epoch && session.origin == origin)
            .map(|session| session.next.clone())
            .context("连接已切换")?;
        if manifest.revision < old.manifest.revision {
            return Ok(());
        }
        if manifest.version == old.manifest.version {
            if old.headers != headers || old.manifest.revision < manifest.revision {
                let snapshot = Arc::new(Snapshot {
                    manifest: Arc::new(manifest),
                    files: old.files.clone(),
                    headers,
                });
                self.persist(&origin, &snapshot)?;
                if let Some(session) = self
                    .session
                    .write()
                    .unwrap()
                    .as_mut()
                    .filter(|value| value.epoch == epoch)
                {
                    session.next = snapshot;
                }
            }
            return Ok(());
        }
        let mut files = BTreeMap::new();
        for entry in &manifest.files {
            self.idle(epoch).await?;
            let bytes = if let Some(bytes) =
                old.matches(entry).or_else(|| self.builtin.matches(entry))
            {
                bytes
            } else {
                let (bytes, _) =
                    request(self.http(url), url.join(&entry.path)?, entry.size as usize).await?;
                ensure!(
                    bytes.len() as u64 == entry.size && hex(&bytes) == entry.sha256,
                    "网页资源摘要不匹配"
                );
                Arc::from(bytes)
            };
            files.insert(entry.path.clone(), bytes);
        }
        self.idle(epoch).await?;
        let snapshot = Arc::new(Snapshot {
            manifest: Arc::new(manifest),
            files,
            headers,
        });
        self.persist(&origin, &snapshot)?;
        if let Some(session) = self
            .session
            .write()
            .unwrap()
            .as_mut()
            .filter(|session| session.epoch == epoch)
        {
            // 完整候选仅供下次导航使用；正在阅读/发送的页面不热切换脚本，不自动刷新或重发任何请求。
            session.next = snapshot;
        }
        Ok(())
    }
    fn persist(&self, origin: &str, snapshot: &Snapshot) -> anyhow::Result<()> {
        let root = self.origin_root(origin);
        std::fs::create_dir_all(&root)?;
        let staging = tempfile::Builder::new()
            .prefix(".candidate-")
            .tempdir_in(&root)?;
        for (position, entry) in snapshot.manifest.files.iter().enumerate() {
            std::fs::write(
                staging.path().join(format!("{position:03}.bin")),
                &snapshot.files[&entry.path],
            )?;
        }
        write_json(
            &staging.path().join("manifest.json"),
            snapshot.manifest.as_ref(),
        )?;
        let version = &snapshot.manifest.version;
        if root.join(version).exists() {
            remove_version(&root, version)?;
        }
        std::fs::rename(staging.path(), root.join(version))?;
        let old = self.index(origin);
        let previous = old
            .as_ref()
            .and_then(|index| {
                if index.active == *version {
                    index.previous.clone()
                } else {
                    Some(index.active.clone())
                }
            })
            .filter(|id| id != version);
        let index = Index {
            schema: 1,
            origin: origin.into(),
            active: version.clone(),
            previous,
            headers: snapshot.headers.clone(),
        };
        write_json(&root.join("index.json"), &index)?;
        // 只保留当前和上一份完整服务资源；内置包留在 EXE 中，更新失败仍可回退。
        for entry in std::fs::read_dir(&root)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_hash(&name) && name != index.active && index.previous.as_deref() != Some(&name)
            {
                let _ = remove_version(&root, &name);
            }
        }
        Ok(())
    }
    pub fn begin_navigation(&self, url: &Url) {
        if let Some(session) = self
            .session
            .write()
            .unwrap()
            .as_mut()
            .filter(|value| value.origin == url.origin().ascii_serialization())
        {
            session.current = session.next.clone();
            self.busy.store(false, Ordering::SeqCst);
        }
    }
    pub fn response(&self, url: &Url, method: &str) -> Option<CachedResponse> {
        if !matches!(method, "GET" | "HEAD") || !valid_path(url.path()) {
            return None;
        }
        let session = self.session.read().unwrap();
        let session = session
            .as_ref()
            .filter(|value| value.origin == url.origin().ascii_serialization())?;
        let entry = session
            .current
            .manifest
            .files
            .iter()
            .find(|entry| entry.path == url.path())?;
        let bytes = session.current.files.get(url.path())?.clone();
        Some(CachedResponse {
            headers: format!(
                "Content-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n{}",
                entry.content_type,
                bytes.len(),
                session.current.headers
            ),
            bytes: if method == "HEAD" {
                Arc::from([])
            } else {
                bytes
            },
        })
    }
    pub fn tunnel_header(&self, url: &Url) -> Option<&'static str> {
        if !matches!(url.scheme(), "http" | "https") || self.session.read().unwrap().is_none() {
            return None;
        }
        let host = url.host_str()?;
        if [
            "ngrok-free.app",
            "ngrok-free.dev",
            "ngrok.app",
            "ngrok.dev",
            "ngrok.io",
        ]
        .iter()
        .any(|domain| host.ends_with(&format!(".{domain}")))
        {
            Some("ngrok-skip-browser-warning")
        } else if host.ends_with(".shares.zrok.io") {
            Some("skip_zrok_interstitial")
        } else {
            None
        }
    }
    pub fn info(&self) -> CacheInfo {
        let session = self.session.read().unwrap();
        let Some(session) = session.as_ref() else {
            return CacheInfo {
                status: if self.enabled.load(Ordering::SeqCst) {
                    "deferred"
                } else {
                    "unsupported"
                },
                version: None,
                message: "",
            };
        };
        let waiting = session.current.manifest.version != session.next.manifest.version;
        CacheInfo {
            status: if waiting { "waiting" } else { "ready" },
            version: Some(session.next.manifest.version.clone()),
            message: if waiting {
                "新版网页资源已缓存，重新打开页面后启用"
            } else if self.update_failed.load(Ordering::SeqCst) {
                "正在使用本地网页资源，更新检查暂未完成"
            } else {
                "网页资源已在 Windows 客户端缓存"
            },
        }
    }
    pub async fn refresh(&self, url: &Url) -> anyhow::Result<CacheInfo> {
        let epoch = self
            .session
            .read()
            .unwrap()
            .as_ref()
            .filter(|value| value.origin == url.origin().ascii_serialization())
            .map(|value| value.epoch)
            .context("尚未连接此服务")?;
        self.update(url, epoch, None).await?;
        Ok(self.info())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    fn sample(revision: u64, text: &str) -> Snapshot {
        let bytes: Arc<[u8]> = Arc::from(text.as_bytes());
        let files = vec![Resource {
            path: "/".into(),
            content_type: "text/html; charset=utf-8".into(),
            size: bytes.len() as u64,
            sha256: hex(&bytes),
        }];
        Snapshot {
            manifest: Arc::new(Manifest {
                schema_version: 1,
                min_shell_version: 1,
                revision,
                version: manifest_version(&files),
                files,
            }),
            files: BTreeMap::from([("/".into(), bytes)]),
            headers: DEFAULT_HEADERS.into(),
        }
    }

    #[tokio::test]
    async fn gzip_update_is_atomic_and_an_existing_connection_reopens_offline() {
        use std::net::TcpListener;
        let directory = tempfile::tempdir().unwrap();
        let cache = ResourceCache::new(directory.path().into()).unwrap();
        cache.enable();
        let updated = sample(bundled::BUNDLED_REVISION + 1, "complete-new-page");
        let manifest = serde_json::to_vec(updated.manifest.as_ref()).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&manifest).unwrap();
        let encoded = encoder.finish().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            for (route, bytes, extra) in [
                ("/app-resources.json", encoded, "Content-Encoding: gzip\r\n"),
                ("/", b"complete-new-page".to_vec(), ""),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut received = Vec::new();
                while !received.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0 && received.len() < 8192);
                    received.extend_from_slice(&buffer[..count]);
                }
                let headers = String::from_utf8(received).unwrap().to_ascii_lowercase();
                assert!(headers.starts_with(&format!("get {route} ")));
                assert!(
                    !headers.contains("\r\ncookie:") && !headers.contains("\r\nauthorization:")
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        cache.prepare(&url, false).await.unwrap();
        assert_ne!(
            &*cache.response(&url, "GET").unwrap().bytes,
            b"complete-new-page"
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while cache.info().status != "waiting" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_ne!(
            &*cache.response(&url, "GET").unwrap().bytes,
            b"complete-new-page"
        );
        cache.begin_navigation(&url);
        assert_eq!(
            &*cache.response(&url, "GET").unwrap().bytes,
            b"complete-new-page"
        );
        server.join().unwrap();
        let reopened = ResourceCache::new(directory.path().into()).unwrap();
        reopened.enable();
        reopened.prepare(&url, true).await.unwrap();
        assert_eq!(
            &*reopened.response(&url, "GET").unwrap().bytes,
            b"complete-new-page"
        );
    }

    #[tokio::test]
    async fn previously_saved_origin_uses_the_built_in_bundle_without_a_new_handshake() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let directory = tempfile::tempdir().unwrap();
        let cache = ResourceCache::new(directory.path().into()).unwrap();
        cache.enable();
        cache.prepare(&url, true).await.unwrap();
        assert!(cache.response(&url, "GET").is_some());
        assert!(cache.info().version.is_some());
        assert_eq!(
            cache
                .index(&url.origin().ascii_serialization())
                .unwrap()
                .active,
            BUILTIN
        );
    }
    #[test]
    fn manifest_rejects_reserved_routes_tampering_duplicates_and_oversize() {
        let original = sample(1, "hello");
        let valid = serde_json::to_vec(original.manifest.as_ref()).unwrap();
        assert!(Manifest::parse(&valid).is_ok());
        for path in [
            "/api",
            "/api/session",
            "/native.js",
            "/app-resources.json",
            "/../x.js",
            "/x%2f.js",
            "//evil/x",
            "/cache-recovery.js",
        ] {
            let mut changed = original.manifest.as_ref().clone();
            changed.files[0].path = path.into();
            changed.version = manifest_version(&changed.files);
            assert!(changed.validate().is_err(), "{path}");
        }
        let mut changed = original.manifest.as_ref().clone();
        changed.files.push(changed.files[0].clone());
        changed.version = manifest_version(&changed.files);
        assert!(changed.validate().is_err());
        changed = original.manifest.as_ref().clone();
        changed.files[0].size = MAX_FILE as u64 + 1;
        changed.version = manifest_version(&changed.files);
        assert!(changed.validate().is_err());
        changed = original.manifest.as_ref().clone();
        changed.version = "0".repeat(64);
        assert!(changed.validate().is_err());
    }
    #[test]
    fn snapshots_switch_only_on_navigation_and_never_cache_api_or_other_origins() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ResourceCache::new(directory.path().into()).unwrap();
        let old = Arc::new(sample(1, "old"));
        let next = Arc::new(sample(2, "next"));
        let url = Url::parse("http://127.0.0.1:47200/").unwrap();
        *cache.session.write().unwrap() = Some(Session {
            origin: url.origin().ascii_serialization(),
            epoch: 1,
            current: old,
            next,
        });
        assert_eq!(&*cache.response(&url, "GET").unwrap().bytes, b"old");
        assert!(
            cache
                .response(&url.join("/api/session").unwrap(), "GET")
                .is_none()
        );
        assert!(cache.response(&url, "POST").is_none());
        assert!(
            cache
                .response(&Url::parse("http://127.0.0.1:47201/").unwrap(), "GET")
                .is_none()
        );
        cache.begin_navigation(&url);
        assert_eq!(&*cache.response(&url, "GET").unwrap().bytes, b"next");
        assert!(cache.response(&url, "HEAD").unwrap().bytes.is_empty());
        assert_eq!(
            cache.tunnel_header(&Url::parse("https://demo.ngrok-free.app/api/images/1").unwrap()),
            Some("ngrok-skip-browser-warning")
        );
        assert_eq!(
            cache.tunnel_header(&Url::parse("https://demo.shares.zrok.io/api/session").unwrap()),
            Some("skip_zrok_interstitial")
        );
        assert!(
            cache
                .tunnel_header(&Url::parse("https://demo.ngrok-free.app.evil.example/x").unwrap())
                .is_none()
        );
        assert!(
            cache
                .response(
                    &Url::parse("https://demo.ngrok-free.app/api/images/1").unwrap(),
                    "GET"
                )
                .is_none()
        );
    }
    #[test]
    fn disk_cache_checks_hashes_keeps_previous_and_does_not_cross_service_scopes() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ResourceCache::new(directory.path().into()).unwrap();
        let origin = "http://127.0.0.1:47200";
        let first = sample(1, "first");
        let second = sample(2, "second");
        let third = sample(3, "third");
        cache.persist(origin, &first).unwrap();
        cache.persist(origin, &second).unwrap();
        let index = cache.index(origin).unwrap();
        assert_eq!(index.previous.as_ref().unwrap(), &first.manifest.version);
        assert!(
            cache
                .load("http://127.0.0.1:47201", &index.active, &index.headers)
                .is_err()
        );
        let file = cache
            .origin_root(origin)
            .join(&index.active)
            .join("000.bin");
        std::fs::write(file, b"damaged").unwrap();
        assert!(cache.load(origin, &index.active, &index.headers).is_err());
        assert!(
            cache
                .load(origin, index.previous.as_ref().unwrap(), &index.headers)
                .is_ok()
        );
        cache.persist(origin, &third).unwrap();
        assert!(
            !cache
                .origin_root(origin)
                .join(&first.manifest.version)
                .exists()
        );
        assert_eq!(cache.index(origin).unwrap().active, third.manifest.version);
    }
    #[test]
    fn built_in_resources_match_public_entry_points_and_header_injection_is_rejected() {
        let snapshot = Snapshot::bundled();
        let paths: HashSet<_> = snapshot.files.keys().map(String::as_str).collect();
        assert!(paths.contains("/") && paths.contains("/app.js") && paths.contains("/style.css"));
        assert!(
            !paths.contains("/native.js") && !paths.iter().any(|path| path.starts_with("/api/"))
        );
        assert!(!valid_headers("Set-Cookie: session=not-allowed\r\n"));
        assert!(!valid_headers("X-Frame-Options: DENY\nSet-Cookie: x\r\n"));
        assert!(valid_headers(DEFAULT_HEADERS));
    }
}
