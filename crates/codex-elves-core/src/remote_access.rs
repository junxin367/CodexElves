use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::io::{Cursor, Read, Write};
use std::net::SocketAddr;
use std::path::{Component, Path as FsPath, PathBuf, Prefix};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message as WebSocketMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use flate2::Compression;
use flate2::write::GzEncoder;
use futures_util::{SinkExt, stream};
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, broadcast};

use crate::settings::{RemoteAccessSettings, SettingsStore};

mod credentials;
mod service;
mod zrok_tunnel;
use credentials::{AccessKey, AccessKeyStore, lock_access_key_store};
pub use credentials::{access_credentials_status, configure_access_key};
pub use service::{ensure_running as ensure_service_running, run as run_service};
use zrok_tunnel::ZrokTunnelService;

const NATIVE_SCRIPT: &str = include_str!("../../../assets/remote-access/native.js");
const WEB_HTML: &str = include_str!("../../../assets/remote-access/index.html");
const WEB_JS: &str = include_str!("../../../assets/remote-access/app.js");
const WEB_ATTACHMENTS_JS: &str = include_str!("../../../assets/remote-access/attachments.js");
const WEB_RECOVERY_STORE_JS: &str = include_str!("../../../assets/remote-access/recovery-store.js");
const WEB_EVENTS_CLIENT_JS: &str = include_str!("../../../assets/remote-access/events-client.js");
const WEB_EVENTS_WORKER_JS: &str = include_str!("../../../assets/remote-access/events-worker.js");
const WEB_CSS: &str = include_str!("../../../assets/remote-access/style.css");
const WEB_MARKDOWN_JS: &str = include_str!("../../../assets/remote-access/markdown.js");
const WEB_MARKDOWN_RENDERER_JS: &str =
    include_str!("../../../assets/remote-access/markdown-renderer.js");
const WEB_MESSAGE_ACTIONS_JS: &str =
    include_str!("../../../assets/remote-access/message-actions.js");
const WEB_VENDOR_MARKED_JS: &str =
    include_str!("../../../assets/remote-access/vendor/marked.esm.js");
const WEB_VENDOR_DOMPURIFY_JS: &str =
    include_str!("../../../assets/remote-access/vendor/dompurify.esm.js");
const WEB_VENDOR_MERMAID_JS: &str =
    include_str!("../../../assets/remote-access/vendor/mermaid.esm.js");
const WEB_PLATFORM_JS: &str = include_str!("../../../assets/remote-access/platform.js");
const WEB_PWA_JS: &str = include_str!("../../../assets/remote-access/pwa.js");
const WEB_SERVICE_WORKER_JS: &str = include_str!("../../../assets/remote-access/sw.js");
const WEB_CACHE_RECOVERY_HTML: &str =
    include_str!("../../../assets/remote-access/cache-recovery.html");
const WEB_CACHE_RECOVERY_JS: &str = include_str!("../../../assets/remote-access/cache-recovery.js");
const WEB_MANIFEST_WEBMANIFEST: &str =
    include_str!("../../../assets/remote-access/manifest.webmanifest");
const WEB_ICON_192_PNG: &[u8] = include_bytes!("../../../assets/remote-access/icons/icon-192.png");
const WEB_ICON_512_PNG: &[u8] = include_bytes!("../../../assets/remote-access/icons/icon-512.png");
const WEB_APPLE_TOUCH_ICON_PNG: &[u8] =
    include_bytes!("../../../assets/remote-access/icons/apple-touch-icon.png");
const COOKIE_NAME: &str = "codex_elves_device";
const DESKTOP_START_GUIDANCE: &str = "Codex 桌面尚未就绪。请在电脑端 CodexElves 管理器右上角点击「重启 Codex」，等待 Codex 主界面完全打开、能够正常查看会话后，再点击「启动公网」。";
const DESKTOP_READ_GUIDANCE: &str = "暂时无法读取 Codex 桌面数据。若刚启动或重启 Codex，请等待主界面完全打开、能够正常查看会话后，再刷新远程网页；尚未启动时，请在电脑端 CodexElves 管理器右上角点击「重启 Codex」。";
const MAX_DEVICES: usize = 32;
const MAX_RECEIPTS: usize = 20_000;
const MAX_REMOTE_IMAGES: usize = 1_024;
const MAX_REMOTE_INLINE_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_REMOTE_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_REMOTE_IMAGE_DIMENSION: u32 = 16_384;
const MAX_REMOTE_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_INPUT_IMAGE_COUNT: usize = 4;
const MAX_INPUT_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_INPUT_IMAGES_TOTAL_BYTES: usize = 12 * 1024 * 1024;
const MAX_ACTION_BODY_BYTES: usize = 18 * 1024 * 1024;
static CONTROLLER: OnceLock<Arc<RemoteAccessController>> = OnceLock::new();
static STARTUP_ERROR: OnceLock<std::sync::Mutex<Option<String>>> = OnceLock::new();
static WEB_ASSETS: OnceLock<HashMap<&'static str, StaticWebAsset>> = OnceLock::new();

#[async_trait]
pub trait NativeRemoteBridge: Send + Sync {
    async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value>;

    async fn call_authorized(
        &self,
        action: &str,
        payload: Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        authorization.check().await?;
        self.call(action, payload).await
    }
}

pub struct NativeCdpBridge;

#[async_trait]
impl NativeRemoteBridge for NativeCdpBridge {
    async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        Self::call_with_status(
            crate::status::StatusStore::default().load_latest()?,
            action,
            payload,
        )
        .await
    }

    async fn call_authorized(
        &self,
        action: &str,
        payload: Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        Self::call_guarded_with_status(
            crate::status::StatusStore::default().load_latest()?,
            action,
            payload,
            Some(authorization),
        )
        .await
    }
}

impl NativeCdpBridge {
    async fn call_with_status(
        status: Option<crate::status::LaunchStatus>,
        action: &str,
        payload: Value,
    ) -> anyhow::Result<Value> {
        Self::call_guarded_with_status(status, action, payload, None).await
    }

    async fn call_guarded_with_status(
        status: Option<crate::status::LaunchStatus>,
        action: &str,
        payload: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        let status = status
            .filter(|status| matches!(status.status.as_str(), "running" | "running_degraded"))
            .context("请先通过 CodexElves 启动 Codex 桌面应用")?;
        let port = status.debug_port.context("Codex 桌面连接不可用")?;
        let targets = crate::cdp::list_targets(port).await?;
        let target = crate::cdp::pick_injectable_codex_page_target(&targets)?;
        let websocket = target
            .web_socket_debugger_url
            .context("Codex 桌面连接不可用")?;
        let expression = format!(
            "{NATIVE_SCRIPT}\nwindow.__codexElvesRemoteAccess.call({}, {})",
            serde_json::to_string(action)?,
            serde_json::to_string(&payload)?
        );
        let response = if let Some(authorization) = authorization {
            crate::bridge::evaluate_script_value_with_authorization(
                &websocket,
                &expression,
                Duration::from_secs(45),
                || authorization.prepare_dispatch(),
            )
            .await?
        } else {
            crate::bridge::evaluate_script_value_with_timeout(
                &websocket,
                &expression,
                Duration::from_secs(45),
            )
            .await?
        };
        if response.pointer("/result/exceptionDetails").is_some() {
            anyhow::bail!("Codex 原生会话桥接执行失败");
        }
        response
            .pointer("/result/result/value")
            .cloned()
            .context("Codex 原生会话桥接未返回结果")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    token_hash: String,
    created_at_ms: u64,
    expires_at_ms: u64,
    last_seen_at_ms: u64,
    #[serde(default)]
    key_id: String,
    #[serde(default)]
    #[serde(alias = "quickTunnelOrigin")]
    public_origin: Option<String>,
}

impl Device {
    fn public(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "createdAtMs": self.created_at_ms,
            "expiresAtMs": self.expires_at_ms, "lastSeenAtMs": self.last_seen_at_ms
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    device_id: String,
    request_id: String,
    payload_hash: String,
    created_at_ms: u64,
    state: String,
    result: Option<Value>,
}

#[derive(Default)]
struct AuthState {
    devices: Vec<Device>,
    access_key: Option<AccessKey>,
    login_locked: bool,
    login_storage_failed_for: Option<String>,
    receipts: HashMap<String, Receipt>,
}

#[derive(Clone, Copy)]
struct AccessPolicy {
    enabled: bool,
    read_only: bool,
    allow_approvals: bool,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            read_only: true,
            allow_approvals: false,
        }
    }
}

#[derive(Clone)]
pub struct RemoteAuthorization {
    controller: Arc<RemoteAccessController>,
    device: Device,
    write_action: Option<String>,
}

impl RemoteAuthorization {
    pub async fn check(&self) -> anyhow::Result<()> {
        use crate::bridge::CdpDispatchPermit;
        self.prepare_dispatch().await?.commit(|| Ok(()))
    }

    async fn prepare_dispatch(&self) -> anyhow::Result<RemoteDispatchPermit> {
        let mut auth = self.controller.auth.clone().lock_owned().await;
        let (store, key, locked) = self
            .controller
            .access_key_snapshot()
            .await
            .map_err(|_| ApiError::new(401, "设备授权无法确认，请重新连接"))?;
        self.controller.sync_access_key(&mut auth, key, locked);
        let current_key = auth.access_key.as_ref().map(|key| key.id.as_str());
        let current = auth
            .devices
            .iter()
            .find(|device| {
                device.id == self.device.id
                    && device.key_id == self.device.key_id
                    && current_key == Some(device.key_id.as_str())
                    && constant_time_eq(&device.token_hash, &self.device.token_hash)
                    && device.expires_at_ms > now_ms()
            })
            .ok_or_else(|| ApiError::new(401, "设备授权已撤销或过期"))?;
        let expires_at_ms = current.expires_at_ms;
        Ok(RemoteDispatchPermit {
            authorization: self.clone(),
            expires_at_ms,
            _auth: auth,
            _key_store: store,
        })
    }
}

struct RemoteDispatchPermit {
    authorization: RemoteAuthorization,
    expires_at_ms: u64,
    _auth: tokio::sync::OwnedMutexGuard<AuthState>,
    _key_store: AccessKeyStore,
}

impl crate::bridge::CdpDispatchPermit for RemoteDispatchPermit {
    fn commit<T>(self, send: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
        let controller = &self.authorization.controller;
        let policy = controller
            .access_policy
            .read()
            .unwrap_or_else(|error| error.into_inner());
        if !policy.enabled || !controller.running.load(Ordering::SeqCst) {
            return Err(ApiError::new(503, "远程访问服务已停止").into());
        }
        if self.expires_at_ms <= now_ms() {
            return Err(ApiError::new(401, "设备授权已撤销或过期").into());
        }
        if let Some(action) = &self.authorization.write_action {
            if policy.read_only {
                return Err(ApiError::new(403, "当前为只读访问").into());
            }
            if matches!(action.as_str(), "approve" | "answer") && !policy.allow_approvals {
                return Err(ApiError::new(403, "远程审批与询问回答已关闭，请在桌面处理").into());
            }
        }
        send()
    }
}

#[derive(Clone)]
struct RegisteredImage {
    source: RegisteredImageSource,
}

#[derive(Clone)]
enum RegisteredImageSource {
    Path(PathBuf),
    Inline {
        bytes: Arc<[u8]>,
        mime: &'static str,
    },
}

struct LoadedImage {
    bytes: Vec<u8>,
    mime: &'static str,
}

#[derive(Default)]
struct ImageRegistry {
    images: HashMap<String, RegisteredImage>,
    paths: HashMap<PathBuf, String>,
    order: VecDeque<String>,
    inline_bytes: usize,
}

impl ImageRegistry {
    fn register(&mut self, path: PathBuf) -> String {
        if let Some(id) = self.paths.get(&path).cloned() {
            if let Some(position) = self.order.iter().position(|entry| entry == &id) {
                self.order.remove(position);
            }
            self.order.push_back(id.clone());
            return id;
        }
        while self.images.len() >= MAX_REMOTE_IMAGES {
            let Some(id) = self.order.pop_front() else {
                break;
            };
            if let Some(image) = self.images.remove(&id) {
                match image.source {
                    RegisteredImageSource::Path(path) => {
                        self.paths.remove(&path);
                    }
                    RegisteredImageSource::Inline { bytes, .. } => {
                        self.inline_bytes = self.inline_bytes.saturating_sub(bytes.len());
                    }
                }
            }
        }
        let id = loop {
            let id = random_token();
            if !self.images.contains_key(&id) {
                break id;
            }
        };
        self.paths.insert(path.clone(), id.clone());
        self.images.insert(
            id.clone(),
            RegisteredImage {
                source: RegisteredImageSource::Path(path),
            },
        );
        self.order.push_back(id.clone());
        id
    }

    fn register_inline(&mut self, bytes: Vec<u8>, mime: &'static str) -> String {
        let id = format!("{:x}", Sha256::digest(&bytes));
        if self.images.contains_key(&id) {
            if let Some(position) = self.order.iter().position(|entry| entry == &id) {
                self.order.remove(position);
            }
            self.order.push_back(id.clone());
            return id;
        }
        while self.images.len() >= MAX_REMOTE_IMAGES
            || self.inline_bytes.saturating_add(bytes.len()) > MAX_REMOTE_INLINE_IMAGE_BYTES
        {
            let Some(expired_id) = self.order.pop_front() else {
                break;
            };
            if let Some(image) = self.images.remove(&expired_id) {
                match image.source {
                    RegisteredImageSource::Path(path) => {
                        self.paths.remove(&path);
                    }
                    RegisteredImageSource::Inline { bytes, .. } => {
                        self.inline_bytes = self.inline_bytes.saturating_sub(bytes.len());
                    }
                }
            }
        }
        self.inline_bytes += bytes.len();
        self.images.insert(
            id.clone(),
            RegisteredImage {
                source: RegisteredImageSource::Inline {
                    bytes: bytes.into(),
                    mime,
                },
            },
        );
        self.order.push_back(id.clone());
        id
    }
}

struct LiveService {
    settings: Option<RemoteAccessSettings>,
    handle: Option<axum_server::Handle<std::net::SocketAddr>>,
    server: Option<tokio::task::JoinHandle<()>>,
    pump: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
    zrok: ZrokTunnelService,
}

pub struct RemoteAccessController {
    native: Arc<dyn NativeRemoteBridge>,
    state_dir: PathBuf,
    auth: Arc<Mutex<AuthState>>,
    access_policy: RwLock<AccessPolicy>,
    live: Mutex<LiveService>,
    running: AtomicBool,
    native_connected: AtomicBool,
    events: broadcast::Sender<Value>,
    key_checks: Arc<tokio::sync::Semaphore>,
    public_origin: Arc<RwLock<Option<String>>>,
    images: RwLock<ImageRegistry>,
}

impl RemoteAccessController {
    pub fn new(
        native: Arc<dyn NativeRemoteBridge>,
        state_dir: PathBuf,
    ) -> anyhow::Result<Arc<Self>> {
        let devices = read_json_or_default(&state_dir.join("remote-access-devices.json"))?;
        let (access_key, login_locked) = {
            let mut store = lock_access_key_store(&state_dir)?;
            let key = store.load()?;
            let locked = store.login_locked(key.as_ref())?;
            (key, locked)
        };
        let mut receipts: HashMap<String, Receipt> =
            read_json_or_default(&state_dir.join("remote-access-receipts.json"))?;
        let mut recovered = false;
        for receipt in receipts
            .values_mut()
            .filter(|receipt| receipt.state == "pending")
        {
            receipt.state = "unknown".into();
            receipt.result = Some(
                json!({"message":"服务曾重启，消息送达状态未知，请查看原生会话，不要重复发送"}),
            );
            recovered = true;
        }
        if recovered {
            crate::settings::atomic_write(
                &state_dir.join("remote-access-receipts.json"),
                &serde_json::to_vec(&receipts)?,
            )?;
        }
        let (events, _) = broadcast::channel(512);
        let public_origin = Arc::new(RwLock::new(None));
        let zrok = ZrokTunnelService::new(state_dir.clone(), public_origin.clone());
        Ok(Arc::new(Self {
            native,
            state_dir,
            auth: Arc::new(Mutex::new(AuthState {
                devices,
                access_key,
                login_locked,
                receipts,
                ..AuthState::default()
            })),
            access_policy: RwLock::new(AccessPolicy::default()),
            live: Mutex::new(LiveService {
                settings: None,
                handle: None,
                server: None,
                pump: None,
                error: None,
                zrok,
            }),
            running: AtomicBool::new(false),
            native_connected: AtomicBool::new(false),
            events,
            key_checks: Arc::new(tokio::sync::Semaphore::new(4)),
            public_origin,
            images: RwLock::new(ImageRegistry::default()),
        }))
    }

    fn public_origin(&self) -> Option<String> {
        self.public_origin
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn attach_item_images(&self, item: &mut Value) {
        let sources = canonical_item_image_sources(item);
        if let Some(object) = item.as_object_mut() {
            object.remove("remoteImages");
        }
        if sources.is_empty() {
            return;
        }
        let mut registry = self
            .images
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let mut replacements = HashMap::new();
        let images = sources
            .into_iter()
            .filter_map(|source| {
                let image = register_remote_image(&mut registry, &source)?;
                replacements.insert(source, image.source.clone());
                Some(json!({
                    "source": image.source,
                    "url": format!("/api/images/{}", image.id),
                    "mime": image.mime
                }))
            })
            .collect::<Vec<_>>();
        if !images.is_empty() {
            replace_item_inline_image_sources(item, &replacements);
            item["remoteImages"] = Value::Array(images);
        }
    }

    fn attach_queue_message_images(&self, message: &mut Value) {
        let sources = canonical_queue_image_sources(message);
        if let Some(object) = message.as_object_mut() {
            object.remove("remoteImages");
        }
        if sources.is_empty() {
            return;
        }
        let mut registry = self
            .images
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let mut replacements = HashMap::new();
        let images = sources
            .into_iter()
            .filter_map(|source| {
                let image = register_remote_image(&mut registry, &source)?;
                replacements.insert(source, image.source.clone());
                Some(json!({
                    "source": image.source,
                    "url": format!("/api/images/{}", image.id),
                    "mime": image.mime
                }))
            })
            .collect::<Vec<_>>();
        if !images.is_empty() {
            replace_queue_inline_image_sources(message, &replacements);
            message["remoteImages"] = Value::Array(images);
        }
    }

    fn attach_queue_images(&self, value: &mut Value) {
        for pointer in [
            "/queuedMessages",
            "/thread/queuedMessages",
            "/params/queuedMessages",
            "/result/queuedMessages",
        ] {
            for message in value
                .pointer_mut(pointer)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                self.attach_queue_message_images(message);
            }
        }
    }

    fn attach_history_images(&self, value: &mut Value) {
        if value.get("scope").and_then(Value::as_str) == Some("items") {
            for item in value["data"].as_array_mut().into_iter().flatten() {
                self.attach_item_images(item);
            }
            return;
        }
        for turn in value["data"].as_array_mut().into_iter().flatten() {
            for item in turn["items"].as_array_mut().into_iter().flatten() {
                self.attach_item_images(item);
            }
        }
    }

    fn attach_event_images(&self, event: &mut Value) {
        self.attach_queue_images(event);
        if !matches!(
            event["method"].as_str(),
            Some("item/started" | "item/completed")
        ) {
            return;
        }
        if let Some(item) = event.pointer_mut("/params/item") {
            self.attach_item_images(item);
        }
    }

    fn attach_response_images(&self, value: &mut Value) {
        self.attach_queue_images(value);
        if let Some(result) = value.get_mut("result") {
            self.attach_queue_images(result);
        }
    }

    fn registered_image(&self, id: &str) -> Option<RegisteredImage> {
        self.images
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .images
            .get(id)
            .cloned()
    }

    async fn save_devices(&self, state: &AuthState) -> anyhow::Result<()> {
        let path = self.state_dir.join("remote-access-devices.json");
        let devices = state.devices.clone();
        tokio::task::spawn_blocking(move || {
            crate::settings::atomic_write(&path, &serde_json::to_vec(&devices)?)
        })
        .await?
    }

    async fn save_receipts(&self, state: &AuthState) -> anyhow::Result<()> {
        crate::settings::atomic_write(
            &self.state_dir.join("remote-access-receipts.json"),
            &serde_json::to_vec(&state.receipts)?,
        )
    }

    async fn reload_access_key(&self) -> anyhow::Result<bool> {
        let mut auth = self.auth.lock().await;
        let (_store, key, locked) = self.access_key_snapshot().await?;
        self.sync_access_key(&mut auth, key, locked);
        Ok(auth.access_key.is_some())
    }

    fn publish_access_policy(&self, settings: &RemoteAccessSettings) {
        *self
            .access_policy
            .write()
            .unwrap_or_else(|error| error.into_inner()) = AccessPolicy {
            enabled: settings.enabled,
            read_only: settings.read_only,
            allow_approvals: settings.allow_approvals,
        };
    }

    fn accepting_requests(&self) -> bool {
        self.running.load(Ordering::SeqCst)
            && self
                .access_policy
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .enabled
    }

    fn authorization(
        self: &Arc<Self>,
        device: Device,
        write_action: Option<&str>,
    ) -> RemoteAuthorization {
        RemoteAuthorization {
            controller: self.clone(),
            device,
            write_action: write_action.map(str::to_owned),
        }
    }

    async fn access_key_snapshot(
        &self,
    ) -> anyhow::Result<(AccessKeyStore, Option<AccessKey>, bool)> {
        let state_dir = self.state_dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = lock_access_key_store(&state_dir)?;
            let key = store.load()?;
            let locked = store.login_locked(key.as_ref())?;
            Ok((store, key, locked))
        })
        .await?
    }

    fn sync_access_key(&self, auth: &mut AuthState, key: Option<AccessKey>, locked: bool) {
        let changed =
            auth.access_key.as_ref().map(|key| &key.id) != key.as_ref().map(|key| &key.id);
        if changed {
            auth.devices
                .retain(|device| key.as_ref().is_some_and(|key| device.key_id == key.id));
            auth.login_storage_failed_for = None;
            let _ = self
                .events
                .send(json!({"method":"remote/authorizationChanged"}));
        }
        auth.login_locked = locked
            || key
                .as_ref()
                .is_some_and(|key| auth.login_storage_failed_for.as_ref() == Some(&key.id));
        auth.access_key = key;
    }

    async fn device_active(&self, id: &str) -> bool {
        if self.reload_access_key().await.is_err() {
            return false;
        }
        let auth = self.auth.lock().await;
        auth.devices.iter().any(|device| {
            device.id == id
                && device.expires_at_ms > now_ms()
                && auth
                    .access_key
                    .as_ref()
                    .is_some_and(|key| device.key_id == key.id)
        })
    }

    async fn prune_stale_public_devices(&self) {
        let Some(current_origin) = self.public_origin() else {
            return;
        };
        let mut auth = self.auth.lock().await;
        let before = auth.devices.len();
        auth.devices.retain(|device| {
            device.public_origin.is_none()
                || device.public_origin.as_deref() == Some(current_origin.as_str())
        });
        if auth.devices.len() == before {
            return;
        }
        let _ = self.save_devices(&auth).await;
        let _ = self
            .events
            .send(json!({"method":"remote/authorizationChanged"}));
    }

    pub async fn reconcile(self: &Arc<Self>, settings: RemoteAccessSettings) {
        // Publish restrictions before any file lock, native call, or tunnel await.
        self.publish_access_policy(&settings);
        let zrok_target = zrok_target(&settings);
        let key_error = self
            .reload_access_key()
            .await
            .err()
            .map(|error| error.to_string());
        let mut live = self.live.lock().await;
        let finished = live.server.as_ref().is_some_and(|task| task.is_finished());
        let was_enabled = live
            .settings
            .as_ref()
            .is_some_and(|settings| settings.enabled);
        if live.settings.as_ref() == Some(&settings)
            && !finished
            && key_error.is_none()
            && (live.server.is_some() || !settings.enabled)
        {
            live.zrok
                .reconcile(self.running.load(Ordering::SeqCst), &zrok_target)
                .await;
            drop(live);
            self.prune_stale_public_devices().await;
            return;
        }
        self.running.store(false, Ordering::SeqCst);
        self.native_connected.store(false, Ordering::SeqCst);
        let _ = self.events.send(json!({"method":"remote/disconnected"}));
        if let Some(handle) = live.handle.take() {
            handle.shutdown();
        }
        if let Some(task) = live.server.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(task) = live.pump.take() {
            task.abort();
            let _ = task.await;
        }
        live.settings = Some(settings.clone());
        live.error = None;
        if !settings.enabled {
            live.zrok.reconcile(false, &zrok_target).await;
            if finished || was_enabled {
                let _ = self.native.call("dispose", json!({})).await;
            }
            drop(live);
            self.prune_stale_public_devices().await;
            return;
        }
        if let Some(error) = key_error {
            live.error = Some(error);
            live.zrok.reconcile(false, &zrok_target).await;
            drop(live);
            self.prune_stale_public_devices().await;
            return;
        }
        match self.start_server(settings.clone()).await {
            Ok((handle, server, pump)) => {
                live.handle = Some(handle);
                live.server = Some(server);
                live.pump = Some(pump);
                self.running.store(true, Ordering::SeqCst);
            }
            Err(error) => live.error = Some(error.to_string()),
        }
        live.zrok
            .reconcile(self.running.load(Ordering::SeqCst), &zrok_target)
            .await;
        drop(live);
        self.prune_stale_public_devices().await;
    }

    async fn start_server(
        self: &Arc<Self>,
        settings: RemoteAccessSettings,
    ) -> anyhow::Result<(
        axum_server::Handle<std::net::SocketAddr>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    )> {
        settings.validate()?;
        let listener = std::net::TcpListener::bind((settings.bind_host.as_str(), settings.port))
            .context("远程访问端口无法监听，请检查端口是否占用")?;
        listener.set_nonblocking(true)?;
        let router = web_router(self.clone(), settings);
        let handle = axum_server::Handle::new();
        let server_handle = handle.clone();
        let controller = self.clone();
        let server = axum_server::from_tcp(listener)?.handle(server_handle);
        let server = tokio::spawn(async move {
            let _ = server
                .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                .await;
            controller.running.store(false, Ordering::SeqCst);
        });
        let controller = self.clone();
        let pump = tokio::spawn(async move {
            let mut cursor = 0;
            let mut epoch = String::new();
            loop {
                match controller
                    .native
                    .call("events", json!({"cursor":cursor, "epoch":epoch}))
                    .await
                {
                    Ok(response) if response["ok"] == true => {
                        if !controller.native_connected.swap(true, Ordering::SeqCst) {
                            let _ = controller
                                .events
                                .send(json!({"method":"remote/reconnected"}));
                        }
                        epoch = response["epoch"].as_str().unwrap_or("").to_string();
                        cursor = response["cursor"].as_u64().unwrap_or(0);
                        if response["reset"] == true {
                            let _ = controller.events.send(json!({"method":"remote/resync"}));
                        }
                        for event in response["events"].as_array().into_iter().flatten() {
                            let mut event = event.clone();
                            controller.attach_event_images(&mut event);
                            let _ = controller.events.send(event);
                        }
                    }
                    _ => {
                        if controller.native_connected.swap(false, Ordering::SeqCst) {
                            let _ = controller
                                .events
                                .send(json!({"method":"remote/disconnected"}));
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(650)).await;
            }
        });
        Ok((handle, server, pump))
    }

    async fn admin_status(&self, settings: &RemoteAccessSettings) -> Value {
        let live = self.live.lock().await;
        let error = live.error.clone();
        let zrok = live.zrok.status();
        drop(live);
        let auth = self.auth.lock().await;
        json!({
            "status":"ok", "enabled":settings.enabled,
            "running":self.running.load(Ordering::SeqCst),
            "nativeConnected":self.native_connected.load(Ordering::SeqCst),
            "error":error, "addresses":access_addresses(settings),
            "devices":auth.devices.iter().filter(|device|device.expires_at_ms>now_ms())
                .filter(|device|auth.access_key.as_ref().is_some_and(|key|device.key_id==key.id))
                .map(Device::public).collect::<Vec<_>>(),
            "settings":settings, "zrok":zrok,
            "accessKeyConfigured":auth.access_key.is_some(),
            "loginLocked":auth.login_locked
        })
    }

    pub async fn admin(self: &Arc<Self>, action: &str, payload: Value) -> anyhow::Result<Value> {
        match action {
            "status" | "apply" => {
                let settings = SettingsStore::default().load()?.remote_access;
                self.reconcile(settings.clone()).await;
                Ok(self.admin_status(&settings).await)
            }
            "downloadZrok" => {
                let settings = SettingsStore::default().load()?.remote_access;
                self.reconcile(settings.clone()).await;
                self.live.lock().await.zrok.begin_download().await?;
                Ok(self.admin_status(&settings).await)
            }
            "registerZrok" => {
                let email = payload["email"].as_str().context("缺少注册邮箱")?;
                let settings = SettingsStore::default().load()?.remote_access;
                self.reconcile(settings).await;
                self.live.lock().await.zrok.register(email).await
            }
            "startZrok" => {
                let settings = SettingsStore::default().load()?.remote_access;
                self.reconcile(settings.clone()).await;
                anyhow::ensure!(
                    settings.enabled && self.running.load(Ordering::SeqCst),
                    "请先启用远程会话网页"
                );
                anyhow::ensure!(
                    self.native_connected.load(Ordering::SeqCst),
                    DESKTOP_START_GUIDANCE
                );
                let token = payload["token"].as_str();
                let fixed_name = payload["fixedName"].as_str().context("缺少固定公网名称")?;
                self.live
                    .lock()
                    .await
                    .zrok
                    .start_public(token, fixed_name, true, &zrok_target(&settings))
                    .await?;
                Ok(self.admin_status(&settings).await)
            }
            "testZrok" => {
                let settings = SettingsStore::default().load()?.remote_access;
                self.reconcile(settings).await;
                self.live.lock().await.zrok.check().await
            }
            "revoke" => {
                let id = payload["deviceId"].as_str().context("缺少设备 ID")?;
                let mut auth = self.auth.lock().await;
                auth.devices.retain(|device| device.id != id);
                self.save_devices(&auth).await?;
                let _ = self
                    .events
                    .send(json!({"method":"remote/authorizationChanged"}));
                Ok(json!({"status":"ok"}))
            }
            _ => anyhow::bail!("不支持的远程管理操作"),
        }
    }

    async fn authorize(&self, headers: &HeaderMap) -> Option<Device> {
        if !self.running.load(Ordering::SeqCst)
            || !self
                .access_policy
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .enabled
        {
            return None;
        }
        let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
        let token = cookies.split(';').find_map(|cookie| {
            let (name, value) = cookie.trim().split_once('=')?;
            (name == COOKIE_NAME).then_some(value)
        })?;
        if token.is_empty() || token.len() > 256 {
            return None;
        }
        let digest = hash(token);
        // Anonymous and random-cookie requests must not reach key-file/KDF work.
        {
            let auth = self.auth.lock().await;
            if !auth.devices.iter().any(|device| {
                device.expires_at_ms > now_ms() && constant_time_eq(&device.token_hash, &digest)
            }) {
                return None;
            }
        }
        if !self.reload_access_key().await.ok()? {
            return None;
        }
        let mut auth = self.auth.lock().await;
        let key_id = auth.access_key.as_ref()?.id.clone();
        let device = auth.devices.iter_mut().find(|device| {
            device.key_id == key_id
                && constant_time_eq(&device.token_hash, &digest)
                && device.expires_at_ms > now_ms()
        })?;
        device.last_seen_at_ms = now_ms();
        Some(device.clone())
    }

    async fn execute(
        self: &Arc<Self>,
        device: Device,
        request_id: String,
        action: String,
        payload: Value,
    ) -> Result<Value, ApiError> {
        let authorization = self.authorization(device.clone(), Some(&action));
        authorization.check().await.map_err(authorization_error)?;
        if request_id.is_empty()
            || request_id.len() > 100
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(ApiError::new(400, "需要稳定的客户端请求 ID"));
        }
        let key = format!("{}:{request_id}", device.id);
        let digest = hash(&serde_json::to_string(&json!([action, payload])).unwrap());
        {
            let mut auth = self.auth.lock().await;
            if !auth
                .devices
                .iter()
                .any(|current| current.id == device.id && current.expires_at_ms > now_ms())
            {
                return Err(ApiError::new(401, "设备授权已撤销或过期"));
            }
            let active_devices = auth
                .devices
                .iter()
                .filter(|device| device.expires_at_ms > now_ms())
                .map(|device| device.id.clone())
                .collect::<std::collections::HashSet<_>>();
            auth.receipts.retain(|_, receipt| {
                receipt.state == "pending" || active_devices.contains(&receipt.device_id)
            });
            if let Some(receipt) = auth.receipts.get(&key) {
                if receipt.payload_hash != digest {
                    return Err(ApiError::new(409, "请求 ID 已用于不同操作"));
                }
                let value = receipt_value(receipt);
                drop(auth);
                self.authorization(device, None)
                    .check()
                    .await
                    .map_err(|error| authorization_error(error).after_admission())?;
                return Ok(value);
            }
            if auth.receipts.len() >= MAX_RECEIPTS {
                return Err(ApiError::new(429, "消息回执已达上限"));
            }
            auth.receipts.insert(
                key.clone(),
                Receipt {
                    device_id: device.id.clone(),
                    request_id: request_id.clone(),
                    payload_hash: digest,
                    created_at_ms: now_ms(),
                    state: "pending".into(),
                    result: None,
                },
            );
            if self.save_receipts(&auth).await.is_err() {
                auth.receipts.remove(&key);
                return Err(ApiError::new(503, "无法保存回执，操作未发送"));
            }
        }
        let controller = self.clone();
        let dispatch_authorization = authorization.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let operation_id = request_id.clone();
        tokio::spawn(async move {
            let result = controller
                .native
                .call_authorized(&action, payload, &dispatch_authorization)
                .await;
            let mut auth = controller.auth.lock().await;
            let receipt = auth.receipts.get_mut(&key).unwrap();
            match result {
                Ok(mut value) => {
                    controller.attach_response_images(&mut value);
                    receipt.state = if value["outcomeUnknown"] == true {
                        "unknown"
                    } else if value["ok"] == true {
                        "completed"
                    } else {
                        "failed"
                    }
                    .into();
                    receipt.result = Some(value);
                }
                Err(error) if error.downcast_ref::<ApiError>().is_some() => {
                    let error = error.downcast_ref::<ApiError>().unwrap();
                    receipt.state = "failed".into();
                    receipt.result = Some(
                        json!({"ok":false,"notSent":true,"code":error.code,"message":error.message}),
                    );
                }
                Err(_) => {
                    // The native client may have accepted the operation before the CDP response was lost.
                    receipt.state = "unknown".into();
                    receipt.result =
                        Some(json!({"message":"送达状态未知，请查看原生会话，不要重复发送"}));
                }
            }
            let response = receipt_value(receipt);
            if controller.save_receipts(&auth).await.is_err() {
                let _ = sender.send(json!({"requestId":operation_id,"state":"unknown","result":{"message":"回执保存失败，请查看会话确认结果"}}));
            } else {
                let _ = sender.send(response);
            }
        });
        let value = match tokio::time::timeout(Duration::from_secs(40), receiver).await {
            Ok(Ok(value)) => value,
            _ => json!({"requestId":request_id,"state":"pending"}),
        };
        self.authorization(device, None)
            .check()
            .await
            .map_err(|error| authorization_error(error).after_admission())?;
        Ok(value)
    }
}

fn receipt_value(receipt: &Receipt) -> Value {
    json!({"requestId":receipt.request_id,"state":receipt.state,"result":receipt.result})
}

pub struct RemoteAccessMonitor {
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl RemoteAccessMonitor {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

pub fn start_monitor() -> anyhow::Result<RemoteAccessMonitor> {
    let controller = match CONTROLLER.get() {
        Some(controller) => controller.clone(),
        None => {
            let controller = RemoteAccessController::new(
                Arc::new(NativeCdpBridge),
                crate::paths::default_app_state_dir(),
            )
            .map_err(|error| {
                if let Ok(mut message) = STARTUP_ERROR
                    .get_or_init(|| std::sync::Mutex::new(None))
                    .lock()
                {
                    *message = Some(error.to_string());
                }
                error
            })?;
            let _ = CONTROLLER.set(controller);
            CONTROLLER.get().unwrap().clone()
        }
    };
    let (shutdown, mut receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        loop {
            if let Ok(settings) = SettingsStore::default().load() {
                controller.reconcile(settings.remote_access).await;
            }
            tokio::select! {
                _ = &mut receiver => break,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
        controller.reconcile(RemoteAccessSettings::default()).await;
    });
    Ok(RemoteAccessMonitor { shutdown, task })
}

pub async fn admin_request(action: &str, payload: Value) -> Value {
    let result = match CONTROLLER.get() {
        Some(controller) => controller.admin(action, payload).await,
        None => Err(anyhow::anyhow!(
            STARTUP_ERROR
                .get()
                .and_then(|error| error.lock().ok()?.clone())
                .unwrap_or_else(|| "请启动新版 CodexElves launcher 后再使用远程访问".into())
        )),
    };
    result.unwrap_or_else(|error| json!({"status":"failed","message":error.to_string()}))
}

pub async fn service_admin_request(action: &str, payload: Value) -> Value {
    service::request_or_start(action, payload)
        .await
        .unwrap_or_else(|error| json!({"status":"failed","message":format!("{error:#}")}))
}

pub async fn local_settings_command(action: &str, payload: Value) -> anyhow::Result<Value> {
    let action = action.to_string();
    tokio::task::spawn_blocking(move || {
        let directory = crate::paths::default_app_state_dir();
        match action.as_str() {
            "credentials" => access_credentials_status(&directory),
            "setKey" => {
                configure_access_key(&directory, payload["key"].as_str().context("缺少访问密钥")?)?;
                access_credentials_status(&directory)
            }
            "generateKey" => {
                let key = random_token();
                Ok(json!({"status":"ok","key":key}))
            }
            _ => anyhow::bail!("不支持的本机远程设置操作"),
        }
    })
    .await
    .context("无法保存本机远程访问设置")?
}
#[derive(Clone)]
struct WebState {
    controller: Arc<RemoteAccessController>,
    settings: RemoteAccessSettings,
}

#[derive(Clone, Copy, Default)]
struct RequestTrustContext {
    external_https: bool,
}

struct StaticWebAsset {
    content_type: &'static str,
    identity: &'static [u8],
    identity_etag: OnceLock<HeaderValue>,
    gzip: OnceLock<CompressedWebAsset>,
}

struct CompressedWebAsset {
    bytes: Bytes,
    etag: HeaderValue,
}

impl StaticWebAsset {
    fn new(content_type: &'static str, identity: &'static [u8]) -> Self {
        Self {
            content_type,
            identity,
            identity_etag: OnceLock::new(),
            gzip: OnceLock::new(),
        }
    }

    fn identity_etag(&self) -> &HeaderValue {
        self.identity_etag
            .get_or_init(|| strong_etag(self.identity))
    }

    fn compressed(&self) -> &CompressedWebAsset {
        self.gzip.get_or_init(|| {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(self.identity)
                .expect("in-memory gzip compression should not fail");
            let bytes = encoder
                .finish()
                .expect("in-memory gzip compression should not fail");
            CompressedWebAsset {
                etag: strong_etag(&bytes),
                bytes: Bytes::from(bytes),
            }
        })
    }
}

fn web_assets() -> &'static HashMap<&'static str, StaticWebAsset> {
    WEB_ASSETS.get_or_init(|| {
        HashMap::from([
            (
                "/",
                StaticWebAsset::new("text/html; charset=utf-8", WEB_HTML.as_bytes()),
            ),
            (
                "/app.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", WEB_JS.as_bytes()),
            ),
            (
                "/attachments.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_ATTACHMENTS_JS.as_bytes(),
                ),
            ),
            (
                "/recovery-store.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_RECOVERY_STORE_JS.as_bytes(),
                ),
            ),
            (
                "/events-client.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_EVENTS_CLIENT_JS.as_bytes(),
                ),
            ),
            (
                "/events-worker.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_EVENTS_WORKER_JS.as_bytes(),
                ),
            ),
            (
                "/markdown.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_MARKDOWN_JS.as_bytes(),
                ),
            ),
            (
                "/markdown-renderer.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_MARKDOWN_RENDERER_JS.as_bytes(),
                ),
            ),
            (
                "/vendor/marked.esm.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_VENDOR_MARKED_JS.as_bytes(),
                ),
            ),
            (
                "/vendor/dompurify.esm.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_VENDOR_DOMPURIFY_JS.as_bytes(),
                ),
            ),
            (
                "/vendor/mermaid.esm.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_VENDOR_MERMAID_JS.as_bytes(),
                ),
            ),
            (
                "/message-actions.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_MESSAGE_ACTIONS_JS.as_bytes(),
                ),
            ),
            (
                "/platform.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_PLATFORM_JS.as_bytes(),
                ),
            ),
            (
                "/pwa.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_PWA_JS.as_bytes(),
                ),
            ),
            (
                "/sw.js",
                StaticWebAsset::new(
                    "application/javascript; charset=utf-8",
                    WEB_SERVICE_WORKER_JS.as_bytes(),
                ),
            ),
            (
                "/style.css",
                StaticWebAsset::new("text/css; charset=utf-8", WEB_CSS.as_bytes()),
            ),
            (
                "/manifest.webmanifest",
                StaticWebAsset::new(
                    "application/manifest+json; charset=utf-8",
                    WEB_MANIFEST_WEBMANIFEST.as_bytes(),
                ),
            ),
            (
                "/icons/icon-192.png",
                StaticWebAsset::new("image/png", WEB_ICON_192_PNG),
            ),
            (
                "/icons/icon-512.png",
                StaticWebAsset::new("image/png", WEB_ICON_512_PNG),
            ),
            (
                "/icons/apple-touch-icon.png",
                StaticWebAsset::new("image/png", WEB_APPLE_TOUCH_ICON_PNG),
            ),
        ])
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppResourcesManifest {
    schema_version: u32,
    min_shell_version: u32,
    version: String,
    files: Vec<AppResourceFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppResourceFile {
    path: String,
    content_type: String,
    size: u64,
    sha256: String,
}

fn build_app_resources_manifest(
    assets: &HashMap<&'static str, StaticWebAsset>,
) -> AppResourcesManifest {
    let mut files: Vec<_> = assets
        .iter()
        .map(|(path, asset)| AppResourceFile {
            path: (*path).to_string(),
            content_type: asset.content_type.to_string(),
            size: asset.identity.len() as u64,
            sha256: format!("{:x}", Sha256::digest(asset.identity)),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut version = Sha256::new();
    for file in &files {
        update_manifest_version_field(&mut version, file.path.as_bytes());
        update_manifest_version_field(&mut version, file.content_type.as_bytes());
        version.update(file.size.to_be_bytes());
        update_manifest_version_field(&mut version, file.sha256.as_bytes());
    }

    AppResourcesManifest {
        schema_version: 1,
        min_shell_version: 1,
        version: format!("{:x}", version.finalize()),
        files,
    }
}

fn update_manifest_version_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn app_resources_json() -> &'static [u8] {
    static JSON: OnceLock<Box<[u8]>> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_vec(&build_app_resources_manifest(web_assets()))
            .expect("app resource manifest should serialize")
            .into_boxed_slice()
    })
}

fn app_resources_asset() -> &'static StaticWebAsset {
    static ASSET: OnceLock<StaticWebAsset> = OnceLock::new();
    ASSET.get_or_init(|| {
        StaticWebAsset::new("application/json; charset=utf-8", app_resources_json())
    })
}

fn strong_etag(bytes: &[u8]) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{:x}\"", Sha256::digest(bytes)))
        .expect("SHA-256 ETag should be a valid header value")
}

fn is_static_web_asset_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/app-resources.json"
            | "/app.js"
            | "/attachments.js"
            | "/recovery-store.js"
            | "/events-client.js"
            | "/events-worker.js"
            | "/markdown.js"
            | "/markdown-renderer.js"
            | "/vendor/marked.esm.js"
            | "/vendor/dompurify.esm.js"
            | "/vendor/mermaid.esm.js"
            | "/message-actions.js"
            | "/platform.js"
            | "/pwa.js"
            | "/sw.js"
            | "/style.css"
            | "/manifest.webmanifest"
            | "/icons/icon-192.png"
            | "/icons/icon-512.png"
            | "/icons/apple-touch-icon.png"
    )
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    let mut gzip_quality = None;
    let mut wildcard_quality = None;
    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for coding in value.split(',') {
            let mut parts = coding.trim().split(';');
            let name = parts.next().unwrap_or("").trim();
            let mut quality = 1000u16;
            for parameter in parts {
                let Some((key, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if key.trim().eq_ignore_ascii_case("q") {
                    quality = value
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|value| (0.0..=1.0).contains(value))
                        .map_or(0, |value| (value * 1000.0).round() as u16);
                }
            }
            let target = if name.eq_ignore_ascii_case("gzip") {
                &mut gzip_quality
            } else if name == "*" {
                &mut wildcard_quality
            } else {
                continue;
            };
            *target = Some(target.map_or(quality, |current: u16| current.max(quality)));
        }
    }
    gzip_quality
        .or(wildcard_quality)
        .is_some_and(|quality| quality > 0)
}

fn if_none_match_matches(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let Ok(etag) = etag.to_str() else {
        return false;
    };
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*"
                || candidate.strip_prefix("W/").unwrap_or(candidate)
                    == etag.strip_prefix("W/").unwrap_or(etag)
        })
}

fn apply_static_asset_headers(response: &mut Response, asset: &StaticWebAsset, gzip: bool) {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    headers.insert(
        header::ETAG,
        if gzip {
            asset.compressed().etag.clone()
        } else {
            asset.identity_etag().clone()
        },
    );
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    if gzip {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
}

async fn static_web_asset(uri: Uri, headers: HeaderMap) -> Response {
    // 摘要和压缩按资源首次访问生成，不让 Mermaid 等非首屏资源阻塞入口。
    // 首次计算或同资源并发等待均在阻塞线程池中完成，避免占住事件/API 工作线程。
    tokio::task::spawn_blocking(move || {
        let asset = if uri.path() == "/app-resources.json" {
            app_resources_asset()
        } else if let Some(asset) = web_assets().get(uri.path()) {
            asset
        } else {
            return StatusCode::NOT_FOUND.into_response();
        };
        web_asset_response(asset, &headers, true)
    })
    .await
    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn cache_recovery_asset(uri: Uri, headers: HeaderMap) -> Response {
    static HTML: OnceLock<StaticWebAsset> = OnceLock::new();
    static SCRIPT: OnceLock<StaticWebAsset> = OnceLock::new();
    let asset = match uri.path() {
        "/cache-recovery.html" => HTML.get_or_init(|| {
            StaticWebAsset::new(
                "text/html; charset=utf-8",
                WEB_CACHE_RECOVERY_HTML.as_bytes(),
            )
        }),
        "/cache-recovery.js" => SCRIPT.get_or_init(|| {
            StaticWebAsset::new(
                "application/javascript; charset=utf-8",
                WEB_CACHE_RECOVERY_JS.as_bytes(),
            )
        }),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    // 恢复入口不属于任一版本壳，损坏的缓存也不能挡住它。
    web_asset_response(asset, &headers, false)
}

fn web_asset_response(asset: &StaticWebAsset, headers: &HeaderMap, conditional: bool) -> Response {
    let gzip = accepts_gzip(headers);
    let etag = if gzip {
        &asset.compressed().etag
    } else {
        asset.identity_etag()
    };
    if conditional && if_none_match_matches(headers, etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        apply_static_asset_headers(&mut response, asset, gzip);
        return response;
    }
    let (body, length) = if gzip {
        let compressed = asset.compressed();
        (Body::from(compressed.bytes.clone()), compressed.bytes.len())
    } else {
        (Body::from(asset.identity), asset.identity.len())
    };
    let mut response = Response::new(body);
    apply_static_asset_headers(&mut response, asset, gzip);
    if !conditional {
        response.headers_mut().remove(header::ETAG);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string())
            .expect("static asset length should be a valid header value"),
    );
    response
}

pub fn web_router(
    controller: Arc<RemoteAccessController>,
    settings: RemoteAccessSettings,
) -> Router {
    let state = WebState {
        controller,
        settings,
    };
    Router::new()
        .route("/", get(static_web_asset))
        .route("/app-resources.json", get(static_web_asset))
        .route("/app.js", get(static_web_asset))
        .route("/attachments.js", get(static_web_asset))
        .route("/recovery-store.js", get(static_web_asset))
        .route("/events-client.js", get(static_web_asset))
        .route("/events-worker.js", get(static_web_asset))
        .route("/markdown.js", get(static_web_asset))
        .route("/markdown-renderer.js", get(static_web_asset))
        .route("/vendor/marked.esm.js", get(static_web_asset))
        .route("/vendor/dompurify.esm.js", get(static_web_asset))
        .route("/vendor/mermaid.esm.js", get(static_web_asset))
        .route("/message-actions.js", get(static_web_asset))
        .route("/platform.js", get(static_web_asset))
        .route("/pwa.js", get(static_web_asset))
        .route("/sw.js", get(static_web_asset))
        .route("/cache-recovery.html", get(cache_recovery_asset))
        .route("/cache-recovery.js", get(cache_recovery_asset))
        .route("/style.css", get(static_web_asset))
        .route("/manifest.webmanifest", get(static_web_asset))
        .route("/icons/icon-192.png", get(static_web_asset))
        .route("/icons/icon-512.png", get(static_web_asset))
        .route("/icons/apple-touch-icon.png", get(static_web_asset))
        .route("/api/login-status", get(login_status))
        .route("/api/connect", post(connect))
        .route("/api/session", get(session))
        .route("/api/logout", post(logout))
        .route("/api/read/{action}", post(read))
        .route(
            "/api/action/{action}",
            post(action).layer(DefaultBodyLimit::max(MAX_ACTION_BODY_BYTES)),
        )
        .route("/api/receipts/{id}", get(receipt))
        .route("/api/images/{id}", get(image))
        .route("/api/events", get(events))
        .route("/api/events/ws", get(events_ws))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            access_boundary,
        ))
        .with_state(state)
}

#[derive(Debug, Clone)]
struct ApiError {
    code: u16,
    message: String,
    outcome_unknown: bool,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

fn authorization_error(error: anyhow::Error) -> ApiError {
    error
        .downcast_ref::<ApiError>()
        .cloned()
        .unwrap_or_else(|| ApiError::new(503, "无法确认当前访问权限，请重新连接"))
}

impl ApiError {
    fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            outcome_unknown: false,
        }
    }

    fn after_admission(mut self) -> Self {
        self.outcome_unknown = true;
        self.message =
            "访问授权已失效，无法确认原操作结果。请勿重复发送，重新连接后请查看会话确认。".into();
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut payload = json!({"ok":false,"message":self.message});
        if self.outcome_unknown {
            payload["outcomeUnknown"] = json!(true);
        }
        (
            StatusCode::from_u16(self.code).unwrap_or(StatusCode::BAD_REQUEST),
            Json(payload),
        )
            .into_response()
    }
}

async fn access_boundary(
    State(state): State<WebState>,
    mut request: Request,
    next: Next,
) -> Response {
    let static_web_asset = is_static_web_asset_path(request.uri().path());
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let external_origin = state.controller.public_origin();
    let mut origins = allowed_origins(&state.settings);
    origins.extend(external_origin.iter().cloned());
    let matched_origin = origins
        .iter()
        .find(|origin| origin_matches_host(origin, host))
        .cloned();
    let valid_host = matched_origin.is_some();
    let external_https = external_origin
        .as_ref()
        .is_some_and(|origin| origin_matches_host(origin, host));
    let request_origin = if external_https {
        external_origin.as_deref().unwrap_or_default().to_string()
    } else {
        matched_origin.unwrap_or_else(|| format!("http://{host}"))
    };
    let origin_header = request.headers().get(header::ORIGIN);
    let origin = origin_header.and_then(|value| value.to_str().ok());
    let bad_origin = origin_header.is_some_and(|value| {
        value.to_str().map_or(true, |origin| {
            !origins.iter().any(|allowed| allowed == origin)
        })
    });
    let missing_origin = request.method() == axum::http::Method::POST && origin_header.is_none();
    let cross_site = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        == Some("cross-site");
    let root_document_navigation = matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && request.uri().path() == "/"
        && origin_header.is_none()
        && request
            .headers()
            .get("sec-fetch-mode")
            .and_then(|value| value.to_str().ok())
            == Some("navigate")
        && request
            .headers()
            .get("sec-fetch-dest")
            .and_then(|value| value.to_str().ok())
            == Some("document");
    if !valid_host || bad_origin || missing_origin || (cross_site && !root_document_navigation) {
        return ApiError::new(403, "只允许从已配置的访问地址操作").into_response();
    }
    if origin.is_some_and(|origin| origin != request_origin) {
        return ApiError::new(403, "请求来源与访问地址不一致").into_response();
    }
    request
        .extensions_mut()
        .insert(RequestTrustContext { external_https });
    let path = request.uri().path();
    let private_api = (path == "/api" || path.starts_with("/api/"))
        && !matches!(path, "/api/connect" | "/api/login-status");
    // Reject unauthorized uploads before JSON/body extraction and before routing.
    let mut response = if private_api {
        match authorized(&state, request.headers()).await {
            Ok(_) => next.run(request).await,
            Err(error) => error.into_response(),
        }
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if static_web_asset {
            "no-cache"
        } else {
            "no-store"
        }),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("content-security-policy", HeaderValue::from_static(
        "default-src 'self'; script-src 'self'; worker-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
    ));
    if external_https {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

async fn authorized(state: &WebState, headers: &HeaderMap) -> Result<Device, ApiError> {
    if !state.controller.accepting_requests() {
        return Err(ApiError::new(503, "远程访问服务已停止"));
    }
    state
        .controller
        .authorize(headers)
        .await
        .ok_or_else(|| ApiError::new(401, "设备未授权或授权已过期"))
}

async fn connect(
    State(state): State<WebState>,
    ConnectInfo(_peer): ConnectInfo<SocketAddr>,
    Extension(trust): Extension<RequestTrustContext>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    if !state.controller.accepting_requests() {
        return Err(ApiError::new(503, "服务已关闭"));
    }
    let key_value = payload["key"].as_str().unwrap_or("").trim();
    if !(8..=256).contains(&key_value.len())
        || !key_value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError::new(
            400,
            "请填写 8–256 个非空格 ASCII 字符的访问密钥",
        ));
    }
    let key = {
        let mut auth = state.controller.auth.lock().await;
        let (_store, key, locked) = state
            .controller
            .access_key_snapshot()
            .await
            .map_err(|_| ApiError::new(503, "访问密钥无法读取，请在本机管理器检查"))?;
        state.controller.sync_access_key(&mut auth, key, locked);
        if auth.login_locked {
            return Err(login_locked_error());
        }
        auth.access_key
            .clone()
            .ok_or_else(|| ApiError::new(503, "本机尚未设置访问密钥"))?
    };
    let candidate = key_value.to_string();
    let checked_key = key.clone();
    let permit = state
        .controller
        .key_checks
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::new(429, "连接验证繁忙，请稍后再试"))?;
    // Dropping an HTTP request must not discard an already running verification.
    tokio::spawn(async move {
        let valid = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            checked_key.verify(&candidate)
        })
        .await
        .map_err(|_| ApiError::new(503, "无法验证访问密钥，请稍后再试"))?;
        complete_login(state, key, valid, headers, trust).await
    })
    .await
    .map_err(|_| ApiError::new(503, "无法完成访问密钥验证，请稍后再试"))?
}

fn login_locked_error() -> ApiError {
    ApiError::new(
        423,
        "访问密钥连续输错超过 3 次，登录已锁定，请在本机管理器重新生成并保存新密钥",
    )
}

async fn login_status(State(state): State<WebState>) -> Result<Json<Value>, ApiError> {
    let mut auth = state.controller.auth.lock().await;
    let (_store, key, locked) = state
        .controller
        .access_key_snapshot()
        .await
        .map_err(|_| ApiError::new(503, "登录状态无法读取，请在本机管理器检查"))?;
    state.controller.sync_access_key(&mut auth, key, locked);
    Ok(Json(json!({"ok":true,"loginLocked":auth.login_locked})))
}

async fn persist_login_result(
    mut store: AccessKeyStore,
    mut key: AccessKey,
    valid: bool,
) -> anyhow::Result<(AccessKeyStore, AccessKey)> {
    tokio::task::spawn_blocking(move || {
        store.record_login_result(&mut key, valid)?;
        Ok((store, key))
    })
    .await?
}

async fn complete_login(
    state: WebState,
    verified_key: AccessKey,
    valid: bool,
    headers: HeaderMap,
    trust: RequestTrustContext,
) -> Result<Response, ApiError> {
    // Every controller path takes auth before the file lock. Keep both until the
    // outcome and any device authorization are committed, including key rotation.
    let mut auth = state.controller.auth.lock().await;
    let (store, key, locked) = state
        .controller
        .access_key_snapshot()
        .await
        .map_err(|_| ApiError::new(503, "访问密钥无法读取，请在本机管理器检查"))?;
    state.controller.sync_access_key(&mut auth, key, locked);
    let key = auth
        .access_key
        .clone()
        .filter(|key| key.id == verified_key.id)
        .ok_or_else(|| ApiError::new(401, "访问密钥刚被更换，请使用新密钥"))?;
    if !state.controller.accepting_requests() {
        return Err(ApiError::new(503, "服务已关闭"));
    }
    if auth.login_locked {
        return Err(login_locked_error());
    }
    if !valid {
        let key_id = key.id.clone();
        let (_store, key) = persist_login_result(store, key, false).await.map_err(|_| {
            auth.login_locked = true;
            auth.login_storage_failed_for = Some(key_id);
            ApiError::new(
                503,
                "登录状态保存失败，已拒绝新登录，请在本机管理器重新生成密钥",
            )
        })?;
        auth.login_locked = key.login_locked();
        auth.access_key = Some(key);
        return Err(if auth.login_locked {
            login_locked_error()
        } else {
            ApiError::new(401, "访问密钥不正确")
        });
    }
    auth.devices
        .retain(|device| device.expires_at_ms > now_ms() && device.key_id == key.id);
    if auth.devices.len() >= MAX_DEVICES {
        return Err(ApiError::new(409, "授权设备已达上限，请撤销旧设备"));
    }
    let token = random_token();
    let (expires_at_ms, max_age_seconds) = if state.settings.device_session_hours == 0 {
        (u64::MAX, 315_360_000u64)
    } else {
        (
            now_ms() + u64::from(state.settings.device_session_hours) * 3_600_000,
            u64::from(state.settings.device_session_hours) * 3600,
        )
    };
    let device = Device {
        id: uuid::Uuid::new_v4().to_string(),
        name: device_name(&headers).into(),
        token_hash: hash(&token),
        created_at_ms: now_ms(),
        expires_at_ms,
        last_seen_at_ms: now_ms(),
        key_id: key.id.clone(),
        public_origin: if trust.external_https {
            state.controller.public_origin()
        } else {
            None
        },
    };
    auth.devices.push(device.clone());
    if state.controller.save_devices(&auth).await.is_err() {
        auth.devices.pop();
        return Err(ApiError::new(503, "设备授权保存失败"));
    }
    let key_id = key.id.clone();
    let (_store, key) = match persist_login_result(store, key, true).await {
        Ok(result) => result,
        Err(_) => {
            auth.devices.pop();
            let _ = state.controller.save_devices(&auth).await;
            auth.login_locked = true;
            auth.login_storage_failed_for = Some(key_id);
            return Err(ApiError::new(
                503,
                "登录状态保存失败，已拒绝新登录，请在本机管理器重新生成密钥",
            ));
        }
    };
    auth.access_key = Some(key);
    auth.login_locked = false;
    let secure = trust.external_https;
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age_seconds}{}",
        if secure { "; Secure" } else { "" }
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({"ok":true,"device":device.public()})),
    )
        .into_response())
}

async fn session(
    State(state): State<WebState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    let policy = *state
        .controller
        .access_policy
        .read()
        .unwrap_or_else(|error| error.into_inner());
    Ok(Json(
        json!({"ok":true,"device":device.public(),"nativeConnected":state.controller.native_connected.load(Ordering::SeqCst),
        "readOnly":policy.read_only,"allowApprovals":policy.allow_approvals,
        "capabilities":{"imageInput":true,"queuedImageRemoval":true,"queuedImageEditing":true,"queueEditRecovery":true,"queueSendRecovery":true,"projectlessCreation":true,"promptOptimization":true,"messageEditModelSettings":true,"goalComposer":true}}),
    ))
}

async fn logout(
    State(state): State<WebState>,
    Extension(trust): Extension<RequestTrustContext>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    state
        .controller
        .admin("revoke", json!({"deviceId":device.id}))
        .await
        .map_err(|_| ApiError::new(503, "撤销授权失败"))?;
    Ok((
        [(
            header::SET_COOKIE,
            format!(
                "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
                if trust.external_https { "; Secure" } else { "" }
            ),
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}

async fn read(
    State(state): State<WebState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    if !matches!(
        action.as_str(),
        "sidebar"
            | "history"
            | "thread"
            | "models"
            | "skills"
            | "queue"
            | "queueEdit"
            | "sendState"
            | "promptOptimization"
    ) {
        return Err(ApiError::new(404, "不支持的读取操作"));
    }
    if matches!(action.as_str(), "queue" | "queueEdit" | "sendState") {
        validate_queued_action_payload(&action, &payload)?;
    }
    if action == "promptOptimization" {
        payload = json!({"requestId": prompt_optimization_request_id(&device.id, payload["requestId"].as_str().unwrap_or(""))?});
    }
    let authorization = state.controller.authorization(device, None);
    let value = state
        .controller
        .native
        .call_authorized(&action, payload, &authorization)
        .await
        .map_err(|error| {
            if let Some(error) = error.downcast_ref::<ApiError>() {
                return error.clone();
            }
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.native_read_failed",
                json!({
                    "action": action,
                    "error": format!("{error:#}")
                }),
            );
            ApiError::new(503, DESKTOP_READ_GUIDANCE)
        })?;
    let mut value = native_result(value)?;
    if action == "history" {
        state.controller.attach_history_images(&mut value);
    }
    if matches!(
        action.as_str(),
        "thread" | "queue" | "queueEdit" | "sendState"
    ) {
        state.controller.attach_queue_images(&mut value);
    }
    let gzip = accepts_gzip(&headers);
    let response = tokio::task::spawn_blocking(move || read_json_response(value, gzip))
        .await
        .map_err(|_| ApiError::new(503, "会话数据处理暂时失败，请重试读取"))??;
    authorization.check().await.map_err(authorization_error)?;
    Ok(response)
}

fn read_json_response(value: Value, accept_gzip: bool) -> Result<Response, ApiError> {
    let mut bytes =
        serde_json::to_vec(&value).map_err(|_| ApiError::new(500, "无法编码会话数据"))?;
    let mut compressed = false;
    if accept_gzip && bytes.len() >= 1024 {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        if encoder.write_all(&bytes).is_ok() {
            if let Ok(encoded) = encoder.finish() {
                if encoded.len() < bytes.len() {
                    bytes = encoded;
                    compressed = true;
                }
            }
        }
    }
    let length = bytes.len();
    let mut response = Body::from(bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string())
            .expect("serialized JSON length should be a valid header value"),
    );
    if compressed {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    Ok(response)
}

fn prompt_optimization_request_id(device_id: &str, request_id: &str) -> Result<String, ApiError> {
    if request_id.is_empty()
        || request_id.len() > 100
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ApiError::new(400, "提示词优化请求 ID 无效"));
    }
    Ok(hash(&format!("prompt-optimize:{device_id}:{request_id}")))
}

fn validate_prompt_optimization_payload(payload: &Value) -> Result<(), ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "提示词优化参数无效"))?;
    if object
        .keys()
        .any(|key| !["input", "model", "recentContext"].contains(&key.as_str()))
    {
        return Err(ApiError::new(400, "远程网页不能修改提示词优化设置"));
    }
    if payload["input"]
        .as_str()
        .is_none_or(|input| input.trim().is_empty() || input.chars().count() > 100_000)
        || payload.get("model").is_some_and(|model| {
            model
                .as_str()
                .is_none_or(|model| model.chars().count() > 256)
        })
    {
        return Err(ApiError::new(400, "提示词为空、过长或模型无效"));
    }
    if let Some(context) = payload.get("recentContext") {
        let context = context
            .as_object()
            .ok_or_else(|| ApiError::new(400, "最近一轮上下文无效"))?;
        if context
            .keys()
            .any(|key| !["user", "assistant"].contains(&key.as_str()))
            || ["user", "assistant"]
                .iter()
                .any(|key| context.get(*key).is_none_or(|value| !value.is_string()))
        {
            return Err(ApiError::new(400, "最近一轮上下文无效"));
        }
    }
    Ok(())
}

fn validate_goal_composer_payload(action: &str, payload: &Value) -> Result<(), ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "目标提交参数无效"))?;
    if object
        .get("goalMode")
        .is_some_and(|mode| !mode.is_boolean())
    {
        return Err(ApiError::new(400, "目标模式参数无效"));
    }
    let goal_mode = object.get("goalMode") == Some(&Value::Bool(true));
    if action == "send" && goal_mode {
        return Err(ApiError::new(400, "目标必须通过目标提交入口发送"));
    }
    if action != "setGoal" && !(action == "create" && goal_mode) {
        return Ok(());
    }
    if !goal_mode {
        return Err(ApiError::new(400, "目标提交需要启用目标模式"));
    }
    let target_field = if action == "setGoal" {
        "threadId"
    } else {
        "projectId"
    };
    if object.keys().any(|key| {
        ![
            target_field,
            "prompt",
            "goalMode",
            "skills",
            "images",
            "model",
            "reasoningEffort",
        ]
        .contains(&key.as_str())
    }) {
        return Err(ApiError::new(400, "目标提交包含不支持的字段"));
    }
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 120
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    };
    if action == "setGoal" {
        if object
            .get("threadId")
            .and_then(Value::as_str)
            .is_none_or(|id| !valid_id(id))
        {
            return Err(ApiError::new(400, "会话 ID 无效"));
        }
    } else if object.get("projectId").is_some_and(|value| {
        !value.is_null()
            && value
                .as_str()
                .is_none_or(|id| !id.is_empty() && !valid_id(id))
    }) {
        return Err(ApiError::new(400, "项目 ID 无效"));
    }
    validate_input_images(payload)?;
    let has_images = object
        .get("images")
        .and_then(Value::as_array)
        .is_some_and(|images| !images.is_empty());
    if object
        .get("prompt")
        .and_then(Value::as_str)
        .is_none_or(|prompt| prompt.len() > 128 * 1024 || prompt.trim().is_empty() && !has_images)
    {
        return Err(ApiError::new(400, "目标不能为空或超过 128 KiB"));
    }
    if object.get("skills").is_some_and(|skills| {
        skills.as_array().is_none_or(|skills| {
            skills.len() > 16
                || skills.iter().any(|skill| {
                    skill.as_object().is_none_or(|object| {
                        object
                            .keys()
                            .any(|key| !["name", "path"].contains(&key.as_str()))
                            || object
                                .get("name")
                                .and_then(Value::as_str)
                                .is_none_or(|name| name.is_empty() || name.len() > 256)
                            || object
                                .get("path")
                                .and_then(Value::as_str)
                                .is_none_or(|path| path.is_empty() || path.len() > 4096)
                    })
                })
        })
    }) {
        return Err(ApiError::new(400, "Skill 选择无效，最多选择 16 项"));
    }
    for (field, limit) in [("model", 256), ("reasoningEffort", 32)] {
        if object.get(field).is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|text| text.trim().is_empty() || text.len() > limit)
        }) {
            return Err(ApiError::new(400, "目标模型或思考深度无效"));
        }
    }
    Ok(())
}

fn validate_goal_update_payload(payload: &Value) -> Result<(), ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "目标编辑参数无效"))?;
    const ALLOWED_FIELDS: [&str; 5] = [
        "threadId",
        "objective",
        "expectedCreatedAt",
        "expectedObjective",
        "expectedStatus",
    ];
    if object
        .keys()
        .any(|key| !ALLOWED_FIELDS.contains(&key.as_str()))
    {
        return Err(ApiError::new(400, "目标编辑包含不支持的字段"));
    }
    let thread_id = object
        .get("threadId")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(400, "会话 ID 无效"))?;
    if thread_id.is_empty()
        || thread_id.len() > 120
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ApiError::new(400, "会话 ID 无效"));
    }
    let objective = object
        .get("objective")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(400, "目标内容不能为空或超过 16 KiB"))?;
    if objective.trim().is_empty() || objective.trim().len() > 16 * 1024 {
        return Err(ApiError::new(400, "目标内容不能为空或超过 16 KiB"));
    }
    let expected_objective = object
        .get("expectedObjective")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(400, "目标编辑基线无效"))?;
    if expected_objective.trim().is_empty() || expected_objective.trim().len() > 16 * 1024 {
        return Err(ApiError::new(400, "目标编辑基线无效"));
    }
    if object
        .get("expectedCreatedAt")
        .and_then(Value::as_u64)
        .is_none()
    {
        return Err(ApiError::new(400, "目标编辑基线无效"));
    }
    if !matches!(
        object.get("expectedStatus").and_then(Value::as_str),
        Some("active" | "paused" | "blocked" | "budgetLimited" | "usageLimited")
    ) {
        return Err(ApiError::new(400, "目标编辑基线无效"));
    }
    Ok(())
}

fn validate_delete_thread_payload(payload: &Value) -> Result<(), ApiError> {
    let object = payload
        .as_object()
        .filter(|object| object.len() == 1 && object.contains_key("threadId"))
        .ok_or_else(|| ApiError::new(400, "永久删除参数只允许包含 threadId"))?;
    let thread_id = object
        .get("threadId")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(400, "永久删除需要已创建完成的本机会话 ID"))?;
    if thread_id.len() != 36 || uuid::Uuid::parse_str(thread_id).is_err() {
        return Err(ApiError::new(400, "永久删除需要已创建完成的本机会话 ID"));
    }
    Ok(())
}

fn validate_input_images(payload: &Value) -> Result<(), ApiError> {
    let Some(images) = payload.get("images") else {
        return Ok(());
    };
    let images = images
        .as_array()
        .ok_or_else(|| ApiError::new(400, "图片附件参数无效"))?;
    if images.len() > MAX_INPUT_IMAGE_COUNT {
        return Err(ApiError::new(400, "每条消息最多添加 4 张图片"));
    }
    let mut total = 0usize;
    for image in images {
        let object = image
            .as_object()
            .ok_or_else(|| ApiError::new(400, "图片附件参数无效"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "mime" | "dataUrl" | "size"))
        {
            return Err(ApiError::new(400, "图片附件包含不支持的字段"));
        }
        let name = image["name"]
            .as_str()
            .ok_or_else(|| ApiError::new(400, "图片名称无效"))?;
        if name.is_empty()
            || name.len() > 255
            || name
                .chars()
                .any(|character| character.is_control() || character == '\u{7f}')
        {
            return Err(ApiError::new(400, "图片名称无效"));
        }
        let mime = image["mime"]
            .as_str()
            .ok_or_else(|| ApiError::new(400, "图片类型无效"))?;
        let source = image["dataUrl"]
            .as_str()
            .ok_or_else(|| ApiError::new(400, "图片内容无效"))?;
        let loaded = decode_data_image(source, MAX_INPUT_IMAGE_BYTES)?;
        if loaded.mime != mime {
            return Err(ApiError::new(400, "图片类型与实际内容不一致"));
        }
        if image["size"].as_u64() != Some(loaded.bytes.len() as u64) {
            return Err(ApiError::new(400, "图片大小与实际内容不一致"));
        }
        total = total.saturating_add(loaded.bytes.len());
        if total > MAX_INPUT_IMAGES_TOTAL_BYTES {
            return Err(ApiError::new(413, "图片总大小不能超过 12 MiB"));
        }
    }
    Ok(())
}

fn validate_queued_action_payload(action: &str, payload: &Value) -> Result<(), ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "队列消息参数无效"))?;
    let reading = action == "queue";
    let checking = action == "queueEdit";
    let confirming_send = action == "sendState";
    let reordering = action == "reorderQueued";
    let beginning = action == "beginQueuedEdit";
    let editing = action == "editQueued";
    let canceling = action == "cancelQueuedEdit";
    let dismissing = action == "dismissQueuedEdit";
    let session_editing = editing && object.contains_key("editId");
    if editing && !session_editing && object.contains_key("images") {
        return Err(ApiError::new(
            409,
            "旧版队列编辑不支持新增图片，请刷新后重试",
        ));
    }
    let fields: &[&str] = if confirming_send {
        &["threadId", "clientUserMessageId"]
    } else if checking {
        &["threadId", "editId"]
    } else if dismissing {
        &["threadId", "editId", "confirmed"]
    } else if reading {
        &["threadId"]
    } else if reordering {
        &["threadId", "messageIds", "expectedMessageIds"]
    } else if beginning {
        &[
            "threadId",
            "messageId",
            "originalText",
            "clientUserMessageId",
            "createdAt",
            "editId",
            "expectedImageSources",
        ]
    } else if session_editing {
        &[
            "threadId",
            "editId",
            "prompt",
            "retainedImageSources",
            "images",
        ]
    } else if editing {
        &[
            "threadId",
            "messageId",
            "prompt",
            "originalText",
            "clientUserMessageId",
            "createdAt",
        ]
    } else if canceling {
        &["threadId", "editId"]
    } else {
        &["threadId", "messageId"]
    };
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(ApiError::new(400, "队列消息包含不支持的字段"));
    }
    let id_fields: &[&str] = if confirming_send {
        &["threadId", "clientUserMessageId"]
    } else if checking || dismissing {
        &["threadId", "editId"]
    } else if reading {
        &["threadId"]
    } else if reordering {
        &["threadId"]
    } else if beginning {
        &["threadId", "messageId", "clientUserMessageId", "editId"]
    } else if session_editing || canceling {
        &["threadId", "editId"]
    } else if editing {
        &["threadId", "messageId", "clientUserMessageId"]
    } else {
        &["threadId", "messageId"]
    };
    for field in id_fields {
        let id = object
            .get(*field)
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::new(400, "队列消息身份无效"))?;
        if id.is_empty()
            || id.len() > 120
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(ApiError::new(400, "队列消息身份无效"));
        }
    }
    if dismissing && object.get("confirmed").and_then(Value::as_bool) != Some(true) {
        return Err(ApiError::new(400, "请先核对队列和会话，再结束待确认编辑"));
    }
    if reordering {
        let message_ids = object
            .get("messageIds")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::new(400, "队列排序参数无效"))?;
        let expected_message_ids = object
            .get("expectedMessageIds")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::new(400, "队列排序参数无效"))?;
        if message_ids.is_empty() || message_ids.len() != expected_message_ids.len() {
            return Err(ApiError::new(400, "队列排序必须包含相同的消息"));
        }
        let validate_ids = |ids: &[Value]| -> Result<HashSet<String>, ApiError> {
            let mut seen = HashSet::with_capacity(ids.len());
            for value in ids {
                let id = value
                    .as_str()
                    .ok_or_else(|| ApiError::new(400, "队列消息身份无效"))?;
                if id.is_empty()
                    || id.len() > 120
                    || !id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                    || !seen.insert(id.to_string())
                {
                    return Err(ApiError::new(400, "队列消息身份无效"));
                }
            }
            Ok(seen)
        };
        let message_ids = validate_ids(message_ids)?;
        let expected_message_ids = validate_ids(expected_message_ids)?;
        if message_ids != expected_message_ids {
            return Err(ApiError::new(400, "队列排序必须包含相同的消息"));
        }
    }
    if beginning || editing && !session_editing {
        if object.get("createdAt").and_then(Value::as_u64).is_none()
            || object
                .get("originalText")
                .and_then(Value::as_str)
                .is_none_or(|text| text.len() > 128 * 1024)
        {
            return Err(ApiError::new(400, "队列消息编辑基线无效"));
        }
    }
    for field in ["expectedImageSources", "retainedImageSources"] {
        let Some(sources) = object.get(field) else {
            continue;
        };
        let sources = sources
            .as_array()
            .ok_or_else(|| ApiError::new(400, "队列图片 source 参数无效"))?;
        if sources.len() > MAX_INPUT_IMAGE_COUNT {
            return Err(ApiError::new(400, "队列图片 source 参数无效"));
        }
        let mut seen = HashSet::with_capacity(sources.len());
        for source in sources {
            let source = source
                .as_str()
                .ok_or_else(|| ApiError::new(400, "队列图片 source 参数无效"))?;
            if source.is_empty()
                || source.len() > 32_767
                || source
                    .chars()
                    .any(|character| character.is_control() || character == '\u{7f}')
                || !seen.insert(source)
            {
                return Err(ApiError::new(400, "队列图片 source 参数无效"));
            }
        }
    }
    if editing {
        let prompt = object
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::new(400, "消息不能为空或超过 128 KiB"))?;
        if prompt.len() > 128 * 1024 {
            return Err(ApiError::new(400, "消息不能为空或超过 128 KiB"));
        }
        if session_editing {
            validate_input_images(payload)?;
            let retained_count = object
                .get("retainedImageSources")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let added_count = object
                .get("images")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if retained_count + added_count > MAX_INPUT_IMAGE_COUNT {
                return Err(ApiError::new(400, "每条消息最多保留或新增 4 张图片"));
            }
            if prompt.trim().is_empty() && retained_count + added_count == 0 {
                return Err(ApiError::new(400, "消息不能为空或超过 128 KiB"));
            }
        } else if prompt.trim().is_empty() {
            return Err(ApiError::new(400, "消息不能为空或超过 128 KiB"));
        }
    }
    Ok(())
}

async fn action(
    State(state): State<WebState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    if state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    if !matches!(
        action.as_str(),
        "send"
            | "stop"
            | "compact"
            | "optimizePrompt"
            | "cancelPromptOptimize"
            | "markRead"
            | "removeQueued"
            | "reorderQueued"
            | "beginQueuedEdit"
            | "editQueued"
            | "cancelQueuedEdit"
            | "dismissQueuedEdit"
            | "steerQueued"
            | "editMessage"
            | "updateGoal"
            | "setGoal"
            | "create"
            | "pinProject"
            | "pinThread"
            | "deleteThread"
            | "sort"
            | "rename"
            | "approve"
            | "answer"
    ) {
        return Err(ApiError::new(404, "不支持的会话操作"));
    }
    if matches!(action.as_str(), "approve" | "answer") && !state.settings.allow_approvals {
        return Err(ApiError::new(403, "远程审批与询问回答已关闭，请在桌面处理"));
    }
    if matches!(action.as_str(), "send" | "create" | "setGoal") {
        validate_goal_composer_payload(&action, &payload)?;
    }
    if action == "updateGoal" {
        validate_goal_update_payload(&payload)?;
    }
    if action == "deleteThread" {
        validate_delete_thread_payload(&payload)?;
    }
    if matches!(
        action.as_str(),
        "reorderQueued"
            | "beginQueuedEdit"
            | "editQueued"
            | "cancelQueuedEdit"
            | "dismissQueuedEdit"
            | "steerQueued"
    ) {
        validate_queued_action_payload(&action, &payload)?;
    }
    let has_input_images = matches!(action.as_str(), "send" | "create")
        && payload["images"]
            .as_array()
            .is_some_and(|images| !images.is_empty());
    if matches!(action.as_str(), "send" | "create" | "editMessage")
        && payload["prompt"].as_str().is_none_or(|text| {
            text.len() > 128 * 1024 || text.trim().is_empty() && !has_input_images
        })
    {
        return Err(ApiError::new(400, "消息不能为空或超过 128 KiB"));
    }
    if matches!(action.as_str(), "send" | "create")
        && payload["goalMode"] != true
        && payload.get("skills").is_some_and(|skills| {
            skills.as_array().is_none_or(|skills| {
                skills.len() > 16
                    || skills.iter().any(|skill| {
                        skill["name"]
                            .as_str()
                            .is_none_or(|name| name.is_empty() || name.len() > 256)
                            || skill["path"]
                                .as_str()
                                .is_none_or(|path| path.is_empty() || path.len() > 4096)
                    })
            })
        })
    {
        return Err(ApiError::new(400, "Skill 选择无效，最多选择 16 项"));
    }
    if matches!(action.as_str(), "send" | "create") && payload["goalMode"] != true {
        validate_input_images(&payload)?;
    }
    let id = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if action == "optimizePrompt" {
        validate_prompt_optimization_payload(&payload)?;
        payload["requestId"] = json!(prompt_optimization_request_id(&device.id, &id)?);
    } else if action == "cancelPromptOptimize" {
        payload = json!({"requestId": prompt_optimization_request_id(&device.id, payload["requestId"].as_str().unwrap_or(""))?});
    }
    state
        .controller
        .execute(device, id, action, payload)
        .await
        .map(Json)
}

async fn receipt(
    State(state): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    let auth = state.controller.auth.lock().await;
    let receipt = auth
        .receipts
        .get(&format!("{}:{id}", device.id))
        .ok_or_else(|| ApiError::new(404, "未找到此请求回执"))?;
    let value = receipt_value(receipt);
    drop(auth);
    state
        .controller
        .authorization(device, None)
        .check()
        .await
        .map_err(authorization_error)?;
    Ok(Json(value))
}

async fn image(
    State(state): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::new(404, "图片不存在或已过期"));
    }
    let image = state
        .controller
        .registered_image(&id)
        .ok_or_else(|| ApiError::new(404, "图片不存在或已过期"))?;
    let image = tokio::task::spawn_blocking(move || read_registered_image(&image))
        .await
        .map_err(|_| ApiError::new(503, "无法读取本机图片"))??;
    state
        .controller
        .authorization(device, None)
        .check()
        .await
        .map_err(authorization_error)?;
    let content_length = HeaderValue::from_str(&image.bytes.len().to_string())
        .map_err(|_| ApiError::new(500, "无法生成图片响应"))?;
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(image.mime)),
            (header::CONTENT_LENGTH, content_length),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("inline"),
            ),
            (
                header::HeaderName::from_static("cross-origin-resource-policy"),
                HeaderValue::from_static("same-origin"),
            ),
        ],
        image.bytes,
    )
        .into_response())
}

async fn events(
    State(state): State<WebState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let device = authorized(&state, &headers).await?;
    let receiver = state.controller.events.subscribe();
    let controller = state.controller.clone();
    let id = device.id.clone();
    let stream = stream::unfold(
        (receiver, controller, id, false),
        |(mut receiver, controller, id, done)| async move {
            if done {
                return None;
            }
            let active = controller.device_active(&id).await;
            if !active {
                return Some((
                    Ok::<_, Infallible>(
                        Event::default().data(r#"{"method":"remote/unauthorized"}"#),
                    ),
                    (receiver, controller, id, true),
                ));
            }
            if !controller.accepting_requests() {
                return Some((
                    Ok(Event::default().data(r#"{"method":"remote/disconnected"}"#)),
                    (receiver, controller, id, true),
                ));
            }
            let mut event =
                match tokio::time::timeout(Duration::from_secs(15), receiver.recv()).await {
                    Ok(Ok(event)) => event,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                        json!({"method":"remote/resync"})
                    }
                    Ok(Err(_)) => return None,
                    Err(_) => json!({"method":"remote/heartbeat"}),
                };
            let authorized = controller.device_active(&id).await;
            let done = !authorized || !controller.accepting_requests();
            if !authorized {
                event = json!({"method":"remote/unauthorized"});
            } else if done {
                event = json!({"method":"remote/disconnected"});
            }
            Some((
                Ok(Event::default().data(serde_json::to_string(&event).unwrap())),
                (receiver, controller, id, done),
            ))
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn events_ws(
    ws: WebSocketUpgrade,
    State(state): State<WebState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    let controller = state.controller.clone();
    let id = device.id;
    Ok(ws
        .on_upgrade(move |socket| stream_websocket_events(socket, controller, id))
        .into_response())
}

async fn stream_websocket_events(
    mut socket: WebSocket,
    controller: Arc<RemoteAccessController>,
    id: String,
) {
    let mut receiver = controller.events.subscribe();
    let heartbeat = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(heartbeat);
    loop {
        let mut event = if !controller.device_active(&id).await {
            json!({"method":"remote/unauthorized"})
        } else if !controller.accepting_requests() {
            json!({"method":"remote/disconnected"})
        } else {
            tokio::select! {
                incoming = socket.recv() => {
                    match incoming {
                        Some(Ok(WebSocketMessage::Close(_))) => {
                            let _ = socket.close().await;
                            break;
                        }
                        Some(Ok(WebSocketMessage::Ping(_))) => {
                            if socket.flush().await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(_)) | None => break,
                    }
                    continue;
                }
                received = receiver.recv() => match received {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        json!({"method":"remote/resync"})
                    }
                    Err(_) => break,
                },
                _ = &mut heartbeat => {
                    heartbeat.as_mut().reset(
                        tokio::time::Instant::now() + Duration::from_secs(15)
                    );
                    json!({"method":"remote/heartbeat"})
                }
            }
        };
        let authorized = controller.device_active(&id).await;
        let done = !authorized || !controller.accepting_requests();
        if !authorized {
            event = json!({"method":"remote/unauthorized"});
        } else if done {
            event = json!({"method":"remote/disconnected"});
        }
        let Ok(text) = serde_json::to_string(&event) else {
            break;
        };
        if socket
            .send(WebSocketMessage::Text(text.into()))
            .await
            .is_err()
        {
            break;
        }
        if done {
            let _ = socket.close().await;
            break;
        }
    }
}

struct RegisteredRemoteImage {
    id: String,
    source: String,
    mime: &'static str,
}

fn register_remote_image(
    registry: &mut ImageRegistry,
    source: &str,
) -> Option<RegisteredRemoteImage> {
    if let Some((path, mime)) = eligible_image_path(source) {
        let id = registry.register(path);
        return Some(RegisteredRemoteImage {
            id,
            source: source.to_string(),
            mime,
        });
    }
    let image = decode_data_image(source, MAX_REMOTE_IMAGE_BYTES as usize).ok()?;
    let id = registry.register_inline(image.bytes, image.mime);
    Some(RegisteredRemoteImage {
        source: format!("inline:{id}"),
        id,
        mime: image.mime,
    })
}

fn canonical_item_image_sources(item: &Value) -> Vec<String> {
    let mut sources = Vec::new();
    match item["type"].as_str() {
        Some("agentMessage") => {
            if let Some(text) = item["text"].as_str() {
                sources.extend(markdown_image_sources(text));
            }
        }
        Some("userMessage") => {
            for content in item["content"].as_array().into_iter().flatten() {
                match content["type"].as_str() {
                    Some("localImage") => {
                        if let Some(path) = content["path"].as_str() {
                            sources.push(path.to_string());
                        }
                    }
                    Some("image") => {
                        if let Some(url) = content["url"].as_str() {
                            sources.push(url.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("imageView") => {
            if let Some(path) = item["path"].as_str() {
                sources.push(path.to_string());
            }
        }
        _ => {}
    }
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.clone()));
    sources
}

fn canonical_queue_image_sources(message: &Value) -> Vec<String> {
    let mut sources = message
        .pointer("/context/imageAttachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|image| image["src"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.clone()));
    sources
}

fn replace_item_inline_image_sources(item: &mut Value, replacements: &HashMap<String, String>) {
    if item["type"] != "userMessage" {
        return;
    }
    for content in item["content"].as_array_mut().into_iter().flatten() {
        if content["type"] != "image" {
            continue;
        }
        let Some(source) = content["url"].as_str() else {
            continue;
        };
        if let Some(replacement) = replacements.get(source) {
            content["url"] = Value::String(replacement.clone());
        }
    }
}

fn replace_queue_inline_image_sources(message: &mut Value, replacements: &HashMap<String, String>) {
    for image in message
        .pointer_mut("/context/imageAttachments")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(source) = image["src"].as_str() else {
            continue;
        };
        if let Some(replacement) = replacements.get(source) {
            image["src"] = Value::String(replacement.clone());
            image.as_object_mut().map(|object| {
                object.remove("localPath");
            });
        }
    }
}

fn markdown_image_sources(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut sources = Vec::new();
    let mut index = 0;
    let mut line_start = true;
    let mut fence: Option<(u8, usize)> = None;
    let mut inline_code_ticks = 0;
    while index < bytes.len() {
        if line_start {
            let mut delimiter = index;
            while delimiter < bytes.len()
                && delimiter - index < 4
                && matches!(bytes[delimiter], b' ' | b'\t')
            {
                delimiter += 1;
            }
            if delimiter < bytes.len() && matches!(bytes[delimiter], b'`' | b'~') {
                let marker = bytes[delimiter];
                let run = marker_run(bytes, delimiter, marker);
                if run >= 3 {
                    match fence {
                        None => fence = Some((marker, run)),
                        Some((open_marker, open_run))
                            if open_marker == marker && run >= open_run =>
                        {
                            fence = None
                        }
                        _ => {}
                    }
                    index = delimiter + run;
                    line_start = false;
                    continue;
                }
            }
        }
        if bytes[index] == b'\n' {
            index += 1;
            line_start = true;
            continue;
        }
        if bytes[index] == b'\r' {
            index += 1;
            continue;
        }
        if fence.is_some() {
            index += 1;
            line_start = false;
            continue;
        }
        line_start = false;
        if bytes[index] == b'`' {
            let run = marker_run(bytes, index, b'`');
            if inline_code_ticks == 0 {
                inline_code_ticks = run;
            } else if inline_code_ticks == run {
                inline_code_ticks = 0;
            }
            index += run;
            continue;
        }
        if inline_code_ticks > 0 {
            index += 1;
            continue;
        }
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
            continue;
        }
        if bytes[index] != b'!' || bytes.get(index + 1) != Some(&b'[') {
            index += 1;
            continue;
        }
        let Some(alt_end) = matching_square_bracket(bytes, index + 1) else {
            index += 2;
            continue;
        };
        if bytes.get(alt_end + 1) != Some(&b'(') {
            index = alt_end + 1;
            continue;
        }
        let Some((raw_source, end)) = markdown_destination(text, alt_end + 1) else {
            index = alt_end + 2;
            continue;
        };
        let source = unescape_markdown_destination(raw_source);
        if !source.is_empty() {
            sources.push(source);
        }
        index = end;
    }
    sources
}

fn marker_run(bytes: &[u8], start: usize, marker: u8) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == marker)
        .count()
}

fn matching_square_bracket(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            b'[' => {
                depth += 1;
                index += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn markdown_destination(text: &str, open: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut start = open + 1;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if bytes.get(start) == Some(&b'<') {
        let mut index = start + 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                b'>' => {
                    let source = &text[start + 1..index];
                    let end = matching_parenthesis_end(bytes, index + 1)?;
                    return Some((source, end));
                }
                b'\r' | b'\n' => return None,
                _ => index += 1,
            }
        }
        return None;
    }
    let mut depth = 1usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let source = strip_markdown_title(text[start..index].trim());
                    return Some((source, index + 1));
                }
                index += 1;
            }
            b'\r' | b'\n' => return None,
            _ => index += 1,
        }
    }
    None
}

fn matching_parenthesis_end(bytes: &[u8], mut index: usize) -> Option<usize> {
    let mut quote = None;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = (index + 2).min(bytes.len()),
            b'\'' | b'"' if quote == Some(bytes[index]) => {
                quote = None;
                index += 1;
            }
            b'\'' | b'"' if quote.is_none() => {
                quote = Some(bytes[index]);
                index += 1;
            }
            b')' if quote.is_none() => return Some(index + 1),
            b'\r' | b'\n' => return None,
            _ => index += 1,
        }
    }
    None
}

fn strip_markdown_title(value: &str) -> &str {
    for (index, byte) in value.bytes().enumerate().rev() {
        if !byte.is_ascii_whitespace() {
            continue;
        }
        let title = value[index..].trim();
        let quoted = title.len() >= 2
            && matches!(
                (title.as_bytes()[0], title.as_bytes()[title.len() - 1]),
                (b'"', b'"') | (b'\'', b'\'')
            );
        if quoted {
            return value[..index].trim_end();
        }
    }
    value
}

fn unescape_markdown_destination(value: &str) -> String {
    let windows_backslash_path =
        value.as_bytes().get(1) == Some(&b':') && value.as_bytes().get(2) == Some(&b'\\');
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' || windows_backslash_path {
            result.push(character);
            continue;
        }
        let Some(next) = characters.peek().copied() else {
            result.push(character);
            break;
        };
        if next.is_ascii_punctuation() || next.is_ascii_whitespace() {
            result.push(characters.next().unwrap());
        } else {
            result.push(character);
        }
    }
    result
}

fn eligible_image_path(source: &str) -> Option<(PathBuf, &'static str)> {
    let source = source.trim();
    if source.is_empty()
        || source.len() > 32_767
        || source.contains('\0')
        || source.contains("://")
        || source.to_ascii_lowercase().starts_with("file:")
        || source.starts_with(r"\\")
        || source.starts_with("//")
    {
        return None;
    }
    let path = PathBuf::from(source);
    if !path.is_absolute() || !input_path_has_local_prefix(&path) {
        return None;
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    Some((path, mime))
}

#[cfg(windows)]
fn input_path_has_local_prefix(path: &FsPath) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
    )
}

#[cfg(not(windows))]
fn input_path_has_local_prefix(path: &FsPath) -> bool {
    matches!(path.components().next(), Some(Component::RootDir))
}

fn read_registered_image(image: &RegisteredImage) -> Result<LoadedImage, ApiError> {
    let RegisteredImageSource::Path(path) = &image.source else {
        let RegisteredImageSource::Inline { bytes, mime } = &image.source else {
            unreachable!();
        };
        return Ok(LoadedImage {
            bytes: bytes.to_vec(),
            mime,
        });
    };
    let path =
        std::fs::canonicalize(path).map_err(|_| ApiError::new(404, "图片文件不存在或无法读取"))?;
    if !canonical_path_is_local(&path) {
        return Err(ApiError::new(403, "图片不在本机固定磁盘"));
    }
    let metadata =
        std::fs::metadata(&path).map_err(|_| ApiError::new(404, "图片文件不存在或无法读取"))?;
    if !metadata.is_file() {
        return Err(ApiError::new(404, "图片文件不存在或无法读取"));
    }
    if metadata.len() > MAX_REMOTE_IMAGE_BYTES {
        return Err(ApiError::new(413, "图片文件过大"));
    }
    let file =
        std::fs::File::open(&path).map_err(|_| ApiError::new(404, "图片文件不存在或无法读取"))?;
    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_REMOTE_IMAGE_BYTES) as usize);
    file.take(MAX_REMOTE_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ApiError::new(503, "无法读取本机图片"))?;
    if bytes.len() as u64 > MAX_REMOTE_IMAGE_BYTES {
        return Err(ApiError::new(413, "图片文件过大"));
    }
    validate_image_bytes(bytes, None, MAX_REMOTE_IMAGE_BYTES as usize)
}

fn decode_data_image(source: &str, max_bytes: usize) -> Result<LoadedImage, ApiError> {
    let (header, encoded) = source
        .split_once(',')
        .ok_or_else(|| ApiError::new(400, "图片内容无效"))?;
    let declared_mime = match header.to_ascii_lowercase().as_str() {
        "data:image/png;base64" => "image/png",
        "data:image/jpeg;base64" => "image/jpeg",
        "data:image/webp;base64" => "image/webp",
        "data:image/gif;base64" => "image/gif",
        _ => return Err(ApiError::new(415, "仅支持 PNG、JPEG、WebP 和 GIF 图片")),
    };
    if encoded.len() > max_bytes.saturating_mul(4).div_ceil(3).saturating_add(4) {
        return Err(ApiError::new(413, "图片文件过大"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| ApiError::new(400, "图片 Base64 内容无效"))?;
    validate_image_bytes(bytes, Some(declared_mime), max_bytes)
}

fn validate_image_bytes(
    bytes: Vec<u8>,
    declared_mime: Option<&str>,
    max_bytes: usize,
) -> Result<LoadedImage, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::new(400, "图片文件为空"));
    }
    if bytes.len() > max_bytes {
        return Err(ApiError::new(413, "图片文件过大"));
    }
    let format =
        image::guess_format(&bytes).map_err(|_| ApiError::new(415, "文件不是支持的图片格式"))?;
    let mime =
        image_format_mime(format).ok_or_else(|| ApiError::new(415, "文件不是支持的图片格式"))?;
    if declared_mime.is_some_and(|declared| declared != mime) {
        return Err(ApiError::new(400, "图片类型与实际内容不一致"));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_REMOTE_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_REMOTE_IMAGE_DIMENSION);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    reader.limits(limits);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| ApiError::new(415, "图片内容损坏或尺寸无效"))?;
    if width == 0
        || height == 0
        || width > MAX_REMOTE_IMAGE_DIMENSION
        || height > MAX_REMOTE_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_REMOTE_IMAGE_PIXELS
    {
        return Err(ApiError::new(413, "图片尺寸过大"));
    }
    Ok(LoadedImage { bytes, mime })
}

fn image_format_mime(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

#[cfg(windows)]
fn canonical_path_is_local(path: &FsPath) -> bool {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;

    let drive = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => return false,
        },
        _ => return false,
    };
    let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
    unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == 3 }
}

#[cfg(not(windows))]
fn canonical_path_is_local(path: &FsPath) -> bool {
    matches!(path.components().next(), Some(Component::RootDir))
}

fn native_result(value: Value) -> Result<Value, ApiError> {
    if value["ok"] == true {
        Ok(value)
    } else {
        Err(ApiError::new(
            value["code"].as_u64().unwrap_or(503) as u16,
            value["message"].as_str().unwrap_or("原生会话操作失败"),
        ))
    }
}

fn device_name(headers: &HeaderMap) -> &'static str {
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if agent.contains("iPhone") || agent.contains("iPad") {
        "Apple 移动设备"
    } else if agent.contains("Android") {
        "Android 设备"
    } else if agent.contains("Windows") {
        "Windows 浏览器"
    } else if agent.contains("Macintosh") {
        "Mac 浏览器"
    } else {
        "网页设备"
    }
}

fn zrok_target(settings: &RemoteAccessSettings) -> String {
    let host = if settings.bind_host.contains(':') {
        format!("[{}]", settings.bind_host)
    } else {
        settings.bind_host.clone()
    };
    format!("http://{host}:{}", settings.port)
}

pub fn access_addresses(settings: &RemoteAccessSettings) -> Vec<String> {
    let mut addresses = Vec::new();
    if settings.bind_host == "0.0.0.0" {
        addresses.extend(
            crate::lan_proxy::lan_ipv4_addresses()
                .iter()
                .map(|ip| format!("http://{ip}:{}", settings.port)),
        );
        addresses.push(format!("http://127.0.0.1:{}", settings.port));
    } else {
        let host = if settings.bind_host.contains(':') {
            format!("[{}]", settings.bind_host)
        } else {
            settings.bind_host.clone()
        };
        addresses.push(format!("http://{host}:{}", settings.port));
    }
    addresses
        .into_iter()
        .filter_map(|address| reqwest::Url::parse(&address).ok())
        .map(|url| url.origin().ascii_serialization())
        .collect()
}

fn allowed_origins(settings: &RemoteAccessSettings) -> Vec<String> {
    let mut addresses = access_addresses(settings);
    if settings.bind_host == "127.0.0.1" || settings.bind_host == "0.0.0.0" {
        addresses.push(format!("http://localhost:{}", settings.port));
    }
    addresses
}

fn origin_matches_host(origin: &str, host: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let serialized = url.origin().ascii_serialization();
    let Some((_, authority)) = serialized.split_once("://") else {
        return false;
    };
    authority.eq_ignore_ascii_case(host)
}

fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(
    path: &std::path::Path,
) -> anyhow::Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("远程访问授权文件损坏，已拒绝启动"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn now_ms() -> u64 {
    crate::proxy_log::current_timestamp_ms()
}
fn constant_time_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovered_desktop_remains_readable_with_degraded_launch_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new()
            .route(
                "/json",
                get(move || async move {
                    Json(json!([{
                        "id":"desktop", "type":"page", "url":"app://-/index.html",
                        "webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/desktop")
                    }]))
                }),
            )
            .route(
                "/desktop",
                get(|upgrade: WebSocketUpgrade| async move {
                    upgrade.on_upgrade(|mut socket| async move {
                        let request = socket.recv().await.unwrap().unwrap();
                        let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                        assert_eq!(request["method"], "Runtime.evaluate");
                        assert!(request["params"]["expression"].as_str().unwrap().contains(
                            "window.__codexElvesRemoteAccess.call(\"models\", {})"
                        ));
                        socket.send(WebSocketMessage::Text(json!({
                            "id":request["id"],
                            "result":{"result":{"value":{"ok":true,"data":[{"id":"recovered-model"}]}}}
                        }).to_string().into())).await.unwrap();
                    })
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let status = crate::status::LaunchStatus {
            status: "running_degraded".to_owned(),
            message: "Waiting for injection".to_owned(),
            started_at_ms: 123,
            debug_port: Some(port),
            helper_port: Some(45221),
            lan_proxy_listening: false,
            codex_app: None,
        };
        let response = NativeCdpBridge::call_with_status(Some(status.clone()), "models", json!({}))
            .await
            .unwrap();
        assert_eq!(response["data"][0]["id"], "recovered-model");
        let failed = crate::status::LaunchStatus {
            status: "failed".to_owned(),
            ..status
        };
        assert!(
            NativeCdpBridge::call_with_status(Some(failed), "models", json!({}))
                .await
                .is_err()
        );
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn delete_thread_payload_requires_a_single_real_thread_uuid() {
        assert!(
            validate_delete_thread_payload(
                &json!({"threadId":"01234567-89ab-4cde-8123-456789abcdef"})
            )
            .is_ok()
        );
        for payload in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"threadId":42}),
            json!({"threadId":"client-new-thread-1"}),
            json!({"threadId":"local:01234567-89ab-4cde-8123-456789abcdef"}),
            json!({"threadId":"cloud:01234567-89ab-4cde-8123-456789abcdef"}),
            json!({"threadId":"0123456789ab4cde8123456789abcdef"}),
            json!({"threadId":"01234567-89ab-4cde-8123-456789abcdef","hostId":"local"}),
            json!({"threadId":"01234567-89ab-4cde-8123-456789abcdef","force":true}),
        ] {
            assert_eq!(
                validate_delete_thread_payload(&payload).unwrap_err().code,
                400
            );
        }
    }

    struct RecordingActionBridge {
        calls: Mutex<Vec<(String, Value)>>,
        result: Mutex<Value>,
    }

    #[async_trait::async_trait]
    impl NativeRemoteBridge for RecordingActionBridge {
        async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
            self.calls.lock().await.push((action.to_string(), payload));
            Ok(self.result.lock().await.clone())
        }
    }

    #[tokio::test]
    async fn read_response_compression_preserves_auth_and_private_payloads() {
        let state_dir = tempfile::tempdir().unwrap();
        configure_access_key(state_dir.path(), "compression-test-key").unwrap();
        let expected = json!({
            "ok": true,
            "data": [{
                "id": "isolated-turn",
                "items": [{
                    "id": "isolated-message",
                    "type": "agentMessage",
                    "text": "这是用于弱网传输验证的合成会话内容，不涉及真实会话。".repeat(1000)
                }]
            }]
        });
        let original_bytes = serde_json::to_vec(&expected).unwrap();
        let native = Arc::new(RecordingActionBridge {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(expected.clone()),
        });
        let controller =
            RemoteAccessController::new(native.clone(), state_dir.path().to_path_buf()).unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        {
            let mut auth = controller.auth.lock().await;
            let key_id = auth.access_key.as_ref().unwrap().id.clone();
            auth.devices.push(Device {
                id: "compression-device".into(),
                name: "隔离传输验证".into(),
                token_hash: hash("compression-cookie"),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 120_000,
                last_seen_at_ms: now_ms(),
                key_id,
                public_origin: None,
            });
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = web_router(
            controller,
            RemoteAccessSettings {
                enabled: true,
                port,
                ..RemoteAccessSettings::default()
            },
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let request = |encoding: &str| {
            client
                .post(format!("{base_url}/api/read/history"))
                .header(reqwest::header::ORIGIN, &base_url)
                .header(reqwest::header::ACCEPT_ENCODING, encoding)
                .header(reqwest::header::IF_NONE_MATCH, "*")
                .json(&json!({"threadId":"isolated-thread"}))
        };
        let denied = request("gzip").send().await.unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert!(native.calls.lock().await.is_empty());
        for (encoding, compressed) in [
            ("gzip", true),
            ("identity", false),
            ("gzip;q=0, *;q=1", false),
        ] {
            let response = request(encoding)
                .header(
                    reqwest::header::COOKIE,
                    "codex_elves_device=compression-cookie",
                )
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(
                response.headers()[reqwest::header::CACHE_CONTROL],
                "no-store"
            );
            assert_eq!(response.headers()[reqwest::header::VARY], "Accept-Encoding");
            assert!(!response.headers().contains_key(reqwest::header::ETAG));
            assert_eq!(
                response.headers().get(reqwest::header::CONTENT_ENCODING),
                compressed.then_some(&HeaderValue::from_static("gzip"))
            );
            let body = response.bytes().await.unwrap();
            let decoded = if compressed {
                assert!(body.len() < original_bytes.len() / 4);
                println!(
                    "合成历史正文：identity={} bytes，gzip={} bytes",
                    original_bytes.len(),
                    body.len()
                );
                let mut decoded = Vec::new();
                flate2::read::GzDecoder::new(&body[..])
                    .read_to_end(&mut decoded)
                    .unwrap();
                decoded
            } else {
                body.to_vec()
            };
            assert!(
                serde_json::from_slice::<Value>(&decoded).unwrap() == expected,
                "压缩与未压缩响应必须保留完整相同的 JSON 正文"
            );
        }
        *native.result.lock().await = json!({"ok":true,"data":[]});
        let small = request("gzip")
            .header(
                reqwest::header::COOKIE,
                "codex_elves_device=compression-cookie",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(small.status(), reqwest::StatusCode::OK);
        assert!(
            !small
                .headers()
                .contains_key(reqwest::header::CONTENT_ENCODING)
        );
        assert_eq!(
            small.json::<Value>().await.unwrap(),
            json!({"ok":true,"data":[]})
        );
        assert_eq!(native.calls.lock().await.len(), 4);
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn delete_thread_action_preserves_auth_read_only_and_idempotency() {
        let temp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&temp_root).unwrap();
        let state_dir = tempfile::Builder::new()
            .prefix("delete-route-")
            .tempdir_in(temp_root)
            .unwrap();
        configure_access_key(state_dir.path(), "delete-test-key").unwrap();
        let thread_id = "01234567-89ab-4cde-8123-456789abcdef";
        let native = Arc::new(RecordingActionBridge {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(
                json!({"ok":true,"threadId":thread_id,"deleted":true,"status":"local_deleted"}),
            ),
        });
        let controller =
            RemoteAccessController::new(native.clone(), state_dir.path().to_path_buf()).unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        {
            let mut auth = controller.auth.lock().await;
            let key_id = auth.access_key.as_ref().unwrap().id.clone();
            auth.devices.push(Device {
                id: "test-device".into(),
                name: "隔离删除测试".into(),
                token_hash: hash("test-cookie"),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 60_000,
                last_seen_at_ms: now_ms(),
                key_id,
                public_origin: None,
            });
        }
        let state = WebState {
            controller: controller.clone(),
            settings: RemoteAccessSettings::default(),
        };
        let payload = json!({"threadId":thread_id});
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("codex_elves_device=test-cookie"),
        );
        headers.insert("idempotency-key", HeaderValue::from_static("delete-1"));
        let invoke = |state, headers, payload| {
            action(
                State(state),
                Path("deleteThread".to_string()),
                headers,
                Json(payload),
            )
        };
        assert_eq!(
            invoke(state.clone(), HeaderMap::new(), payload.clone())
                .await
                .unwrap_err()
                .code,
            401
        );
        let mut read_only = state.clone();
        read_only.settings.read_only = true;
        assert_eq!(
            invoke(read_only, headers.clone(), payload.clone())
                .await
                .unwrap_err()
                .code,
            403
        );
        assert_eq!(
            invoke(
                state.clone(),
                headers.clone(),
                json!({"threadId":"client-new-thread-1"})
            )
            .await
            .unwrap_err()
            .code,
            400
        );
        let mut no_id = headers.clone();
        no_id.remove("idempotency-key");
        assert_eq!(
            invoke(state.clone(), no_id, payload.clone())
                .await
                .unwrap_err()
                .code,
            400
        );
        assert!(native.calls.lock().await.is_empty());
        let first = invoke(state.clone(), headers.clone(), payload.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(first["state"], "completed");
        assert_eq!(first["result"]["deleted"], true);
        assert_eq!(
            invoke(state.clone(), headers.clone(), payload.clone())
                .await
                .unwrap()
                .0,
            first
        );
        assert_eq!(
            invoke(
                state.clone(),
                headers.clone(),
                json!({"threadId":"fedcba98-7654-4321-8abc-0123456789ab"})
            )
            .await
            .unwrap_err()
            .code,
            409
        );
        assert_eq!(
            *native.calls.lock().await,
            vec![("deleteThread".to_string(), payload.clone())]
        );
        *native.result.lock().await =
            json!({"ok":false,"threadId":thread_id,"outcomeUnknown":true});
        headers.insert("idempotency-key", HeaderValue::from_static("delete-2"));
        let unknown = invoke(state.clone(), headers.clone(), payload.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(unknown["state"], "unknown");
        assert_eq!(
            invoke(state.clone(), headers.clone(), payload.clone())
                .await
                .unwrap()
                .0,
            unknown
        );
        assert_eq!(native.calls.lock().await.len(), 2);
        *native.result.lock().await = json!({"ok":true,"threadId":thread_id,"deleted":true,"status":"partial","message":"mock 最近列表更新失败"});
        headers.insert("idempotency-key", HeaderValue::from_static("delete-3"));
        let partial = invoke(state.clone(), headers.clone(), payload.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(partial["state"], "completed");
        assert_eq!(partial["result"]["status"], "partial");
        controller.auth.lock().await.devices.clear();
        assert_eq!(invoke(state, headers, payload).await.unwrap_err().code, 401);
        assert_eq!(native.calls.lock().await.len(), 3);
    }

    #[test]
    fn goal_composer_payload_requires_explicit_mode_and_validated_inputs() {
        let payload = json!({
            "threadId":"existing-thread","prompt":"前文 [$example](C:/skills/example/SKILL.md) 后文",
            "goalMode":true,"model":"test-model","reasoningEffort":"xhigh",
            "skills":[{"name":"example","path":"C:/skills/example/SKILL.md"}]
        });
        assert!(validate_goal_composer_payload("setGoal", &payload).is_ok());
        assert!(
            validate_goal_composer_payload(
                "create",
                &json!({"projectId":null,"prompt":"新目标","goalMode":true})
            )
            .is_ok()
        );
        assert!(
            validate_goal_composer_payload(
                "create",
                &json!({"prompt":"普通消息","goalMode":false})
            )
            .is_ok()
        );
        assert_eq!(
            validate_goal_composer_payload("send", &payload)
                .unwrap_err()
                .code,
            400
        );
        for (field, value) in [
            ("goalMode", json!("true")),
            ("goalMode", Value::Null),
            ("goalMode", json!(false)),
            ("threadId", json!("local:other-thread")),
            ("prompt", json!(" ")),
            ("status", json!("active")),
            ("tokenBudget", json!(600_000)),
            ("directoryPath", json!("C:/private")),
            ("model", json!(12)),
            ("reasoningEffort", json!("")),
            (
                "skills",
                json!([{"name":"example","path":"C:/skill.md","content":"unsafe"}]),
            ),
            (
                "images",
                json!([{"name":"sample.png","localPath":"C:/private.png"}]),
            ),
        ] {
            let mut invalid = payload.clone();
            invalid[field] = value;
            assert_eq!(
                validate_goal_composer_payload("setGoal", &invalid)
                    .unwrap_err()
                    .code,
                400,
                "field: {field}"
            );
        }
        let mut image_bytes = Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut image_bytes, ImageFormat::Png)
            .unwrap();
        let bytes = image_bytes.into_inner();
        let image = json!({"name":"sample.png","mime":"image/png","size":bytes.len(),
            "dataUrl":format!("data:image/png;base64,{}",STANDARD.encode(bytes))});
        let mut images_only =
            json!({"threadId":"existing-thread","goalMode":true,"prompt":"","images":[image]});
        assert!(validate_goal_composer_payload("setGoal", &images_only).is_ok());
        images_only["images"][0]["size"] = json!(1);
        assert_eq!(
            validate_goal_composer_payload("setGoal", &images_only)
                .unwrap_err()
                .code,
            400
        );
    }

    #[tokio::test]
    async fn goal_composer_actions_preserve_auth_read_only_and_idempotent_phase_receipts() {
        let temp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&temp_root).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("goal-composer-route-")
            .tempdir_in(temp_root)
            .unwrap();
        configure_access_key(directory.path(), "isolated-goal-key").unwrap();
        let native = Arc::new(RecordingActionBridge {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(json!({
                "ok":true,"threadId":"existing-thread",
                "goalSubmission":{"action":"setGoal","phase":"complete","threadId":"existing-thread",
                    "firstTurnAccepted":false,"goalSet":"succeeded"}
            })),
        });
        let controller =
            RemoteAccessController::new(native.clone(), directory.path().to_path_buf()).unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        {
            let mut auth = controller.auth.lock().await;
            let key_id = auth.access_key.as_ref().unwrap().id.clone();
            auth.devices.push(Device {
                id: "goal-test-device".into(),
                name: "隔离目标测试".into(),
                token_hash: hash("goal-test-cookie"),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 60_000,
                last_seen_at_ms: now_ms(),
                key_id,
                public_origin: None,
            });
        }
        let state = WebState {
            controller: controller.clone(),
            settings: RemoteAccessSettings::default(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("codex_elves_device=goal-test-cookie"),
        );
        headers.insert("idempotency-key", HeaderValue::from_static("goal-1"));
        let payload = json!({"threadId":"existing-thread","prompt":"完成目标","goalMode":true});
        let invoke = |state, action_name: &str, headers, payload| {
            action(
                State(state),
                Path(action_name.to_string()),
                headers,
                Json(payload),
            )
        };
        assert_eq!(
            invoke(state.clone(), "setGoal", HeaderMap::new(), payload.clone())
                .await
                .unwrap_err()
                .code,
            401
        );
        let mut read_only = state.clone();
        read_only.settings.read_only = true;
        assert_eq!(
            invoke(read_only, "setGoal", headers.clone(), payload.clone())
                .await
                .unwrap_err()
                .code,
            403
        );
        assert_eq!(
            invoke(state.clone(), "send", headers.clone(), payload.clone())
                .await
                .unwrap_err()
                .code,
            400
        );
        let mut no_id = headers.clone();
        no_id.remove("idempotency-key");
        assert_eq!(
            invoke(state.clone(), "setGoal", no_id, payload.clone())
                .await
                .unwrap_err()
                .code,
            400
        );
        assert!(native.calls.lock().await.is_empty());
        let capabilities = session(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(capabilities["capabilities"]["goalComposer"], true);
        let completed = invoke(state.clone(), "setGoal", headers.clone(), payload.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(completed["state"], "completed");
        assert_eq!(
            invoke(state.clone(), "setGoal", headers.clone(), payload.clone())
                .await
                .unwrap()
                .0,
            completed
        );
        let mut changed = payload.clone();
        changed["prompt"] = json!("另一个目标");
        assert_eq!(
            invoke(state.clone(), "setGoal", headers.clone(), changed)
                .await
                .unwrap_err()
                .code,
            409
        );
        assert_eq!(native.calls.lock().await.len(), 1);
        let unknown = json!({
            "ok":false,"outcomeUnknown":true,"threadId":"new-thread",
            "goalSubmission":{"action":"create","phase":"goalSet","threadId":"new-thread",
                "firstTurnAccepted":true,"goalSet":"unknown"}
        });
        *native.result.lock().await = unknown.clone();
        headers.insert("idempotency-key", HeaderValue::from_static("goal-2"));
        let creating = json!({"projectId":"project1","prompt":"新目标","goalMode":true});
        let first = invoke(state.clone(), "create", headers.clone(), creating.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(first["state"], "unknown");
        assert_eq!(first["result"], unknown);
        assert_eq!(
            invoke(state.clone(), "create", headers.clone(), creating)
                .await
                .unwrap()
                .0,
            first
        );
        let reread = receipt(State(state), Path("goal-2".to_owned()), headers)
            .await
            .unwrap()
            .0;
        assert_eq!(reread, first);
        assert_eq!(native.calls.lock().await.len(), 2);
    }

    struct OfflineNativeBridge;

    #[async_trait::async_trait]
    impl NativeRemoteBridge for OfflineNativeBridge {
        async fn call(&self, _action: &str, _payload: Value) -> anyhow::Result<Value> {
            anyhow::bail!("native bridge unavailable during test")
        }
    }

    const LOGIN_TEST_KEY: &str = "isolated-login-key";

    fn login_lock_test_state() -> (tempfile::TempDir, WebState) {
        let temp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&temp_root).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("remote-login-lock-")
            .tempdir_in(temp_root)
            .unwrap();
        configure_access_key(directory.path(), LOGIN_TEST_KEY).unwrap();
        let state = reopened_login_lock_state(directory.path());
        (directory, state)
    }

    fn reopened_login_lock_state(directory: &std::path::Path) -> WebState {
        let controller =
            RemoteAccessController::new(Arc::new(OfflineNativeBridge), directory.to_path_buf())
                .unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        WebState {
            controller,
            settings: RemoteAccessSettings::default(),
        }
    }

    async fn login_lock_attempt(
        state: &WebState,
        candidate: &str,
        peer: &str,
    ) -> Result<Response, ApiError> {
        connect(
            State(state.clone()),
            ConnectInfo(peer.parse().unwrap()),
            Extension(RequestTrustContext::default()),
            HeaderMap::new(),
            Json(json!({"key":candidate})),
        )
        .await
    }

    fn login_failure_count(directory: &std::path::Path) -> u64 {
        let record: Value = serde_json::from_slice(
            &std::fs::read(directory.join("remote-access-key.json")).unwrap(),
        )
        .unwrap();
        record["login_failures"].as_u64().unwrap()
    }

    fn login_cookie(response: &Response) -> String {
        response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn login_lock_is_global_at_four_failures_and_skips_non_key_errors() {
        let (directory, state) = login_lock_test_state();
        assert_eq!(
            login_lock_attempt(&state, "short", "192.0.2.1:1000")
                .await
                .unwrap_err()
                .code,
            400
        );
        state.controller.running.store(false, Ordering::SeqCst);
        assert_eq!(
            login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
                .await
                .unwrap_err()
                .code,
            503
        );
        state.controller.running.store(true, Ordering::SeqCst);
        let busy = state
            .controller
            .key_checks
            .clone()
            .acquire_many_owned(4)
            .await
            .unwrap();
        assert_eq!(
            login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000")
                .await
                .unwrap_err()
                .code,
            429
        );
        drop(busy);
        assert_eq!(login_failure_count(directory.path()), 0);
        for (index, peer) in [
            "192.0.2.1:1000",
            "192.0.2.2:2000",
            "[2001:db8::1]:3000",
            "198.51.100.1:4000",
        ]
        .into_iter()
        .enumerate()
        {
            let error = login_lock_attempt(&state, "wrong-login-key", peer)
                .await
                .unwrap_err();
            assert_eq!(error.code, if index == 3 { 423 } else { 401 });
            assert!(
                error
                    .into_response()
                    .headers()
                    .get(header::SET_COOKIE)
                    .is_none()
            );
            assert_eq!(login_failure_count(directory.path()), index as u64 + 1);
            assert_eq!(
                login_status(State(state.clone())).await.unwrap().0["loginLocked"],
                index == 3
            );
        }
        assert_eq!(
            login_lock_attempt(&state, LOGIN_TEST_KEY, "203.0.113.5:5000")
                .await
                .unwrap_err()
                .code,
            423
        );
        assert_eq!(login_failure_count(directory.path()), 4);
        assert!(state.controller.auth.lock().await.devices.is_empty());
    }

    #[tokio::test]
    async fn login_lock_success_resets_counter_and_preserves_existing_device() {
        let (directory, state) = login_lock_test_state();
        let mut authorization_events = state.controller.events.subscribe();
        for _ in 0..3 {
            assert_eq!(
                login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000")
                    .await
                    .unwrap_err()
                    .code,
                401
            );
        }
        let response = login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
            .await
            .unwrap();
        let cookie = login_cookie(&response);
        assert_eq!(login_failure_count(directory.path()), 0);
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
        for index in 0..4 {
            let error = login_lock_attempt(&state, "wrong-login-key", "192.0.2.2:2000")
                .await
                .unwrap_err();
            assert_eq!(error.code, if index == 3 { 423 } else { 401 });
            assert!(state.controller.authorize(&headers).await.is_some());
        }
        assert!(authorization_events.try_recv().is_err());
        let status = state.controller.admin_status(&state.settings).await;
        assert_eq!(status["loginLocked"], true);
        assert_eq!(status["devices"].as_array().unwrap().len(), 1);

        configure_access_key(directory.path(), "rotated-login-key").unwrap();
        assert!(state.controller.authorize(&headers).await.is_none());
        assert_eq!(
            login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
                .await
                .unwrap_err()
                .code,
            401
        );
        assert!(
            login_lock_attempt(&state, "rotated-login-key", "192.0.2.1:1000")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn login_lock_survives_restart_settings_reads_and_saving_same_key() {
        let (directory, state) = login_lock_test_state();
        for _ in 0..3 {
            let _ = login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000").await;
        }
        let restarted = reopened_login_lock_state(directory.path());
        assert_eq!(login_failure_count(directory.path()), 3);
        assert_eq!(
            login_lock_attempt(&restarted, "wrong-login-key", "192.0.2.2:2000")
                .await
                .unwrap_err()
                .code,
            423
        );
        let key_id = restarted
            .controller
            .auth
            .lock()
            .await
            .access_key
            .as_ref()
            .unwrap()
            .id
            .clone();
        let settings_store = SettingsStore::new(directory.path().join("settings.json"));
        let mut settings = settings_store.load().unwrap();
        settings.remote_access.read_only = true;
        settings_store.save(&settings).unwrap();
        restarted.controller.reconcile(settings.remote_access).await;
        restarted
            .controller
            .publish_access_policy(&RemoteAccessSettings {
                enabled: true,
                read_only: true,
                ..RemoteAccessSettings::default()
            });
        restarted.controller.running.store(true, Ordering::SeqCst);
        let _unsaved_candidate = random_token();
        configure_access_key(directory.path(), LOGIN_TEST_KEY).unwrap();
        for _ in 0..2 {
            let status = access_credentials_status(directory.path()).unwrap();
            assert_eq!(status["loginLocked"], true);
            assert_eq!(
                login_status(State(restarted.clone())).await.unwrap().0["loginLocked"],
                true
            );
        }
        let restarted_again = reopened_login_lock_state(directory.path());
        assert_eq!(
            restarted_again
                .controller
                .auth
                .lock()
                .await
                .access_key
                .as_ref()
                .unwrap()
                .id,
            key_id
        );
        assert_eq!(login_failure_count(directory.path()), 4);
        assert_eq!(
            login_lock_attempt(&restarted_again, LOGIN_TEST_KEY, "203.0.113.1:3000")
                .await
                .unwrap_err()
                .code,
            423
        );
        configure_access_key(directory.path(), "new-isolated-key").unwrap();
        assert_eq!(
            login_status(State(restarted_again.clone()))
                .await
                .unwrap()
                .0["loginLocked"],
            false
        );
        assert_ne!(
            restarted_again
                .controller
                .auth
                .lock()
                .await
                .access_key
                .as_ref()
                .unwrap()
                .id,
            key_id
        );
        assert!(
            login_lock_attempt(&restarted_again, "new-isolated-key", "192.0.2.1:1000")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn login_lock_write_failure_is_durable_and_does_not_revoke_devices() {
        let (directory, state) = login_lock_test_state();
        let response = login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&login_cookie(&response)).unwrap(),
        );
        for _ in 0..3 {
            let _ = login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000").await;
        }
        let blocked_temp = directory.path().join("remote-access-key.json.tmp");
        std::fs::create_dir(&blocked_temp).unwrap();
        let error = login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000")
            .await
            .unwrap_err();
        assert_eq!(error.code, 503);
        assert!(
            error
                .into_response()
                .headers()
                .get(header::SET_COOKIE)
                .is_none()
        );
        assert_eq!(login_failure_count(directory.path()), 3);
        assert!(state.controller.authorize(&headers).await.is_some());
        let restarted = reopened_login_lock_state(directory.path());
        assert_eq!(
            login_lock_attempt(&restarted, LOGIN_TEST_KEY, "192.0.2.2:2000")
                .await
                .unwrap_err()
                .code,
            423
        );
        assert!(restarted.controller.authorize(&headers).await.is_some());
        assert!(configure_access_key(directory.path(), "new-isolated-key").is_err());
        assert_eq!(
            access_credentials_status(directory.path()).unwrap()["loginLocked"],
            true
        );
        std::fs::remove_dir(blocked_temp).unwrap();
        configure_access_key(directory.path(), LOGIN_TEST_KEY).unwrap();
        assert_eq!(
            access_credentials_status(directory.path()).unwrap()["loginLocked"],
            true
        );
        configure_access_key(directory.path(), "new-isolated-key").unwrap();
        assert_eq!(
            login_status(State(restarted.clone())).await.unwrap().0["loginLocked"],
            false
        );
    }

    #[tokio::test]
    async fn login_lock_corrupt_counter_refuses_new_authorization_until_key_rotation() {
        let (directory, state) = login_lock_test_state();
        let response = login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&login_cookie(&response)).unwrap(),
        );
        let path = directory.path().join("remote-access-key.json");
        let mut stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored["login_failures"] = json!(5);
        crate::settings::atomic_write(&path, &serde_json::to_vec(&stored).unwrap()).unwrap();
        assert_eq!(
            login_lock_attempt(&state, LOGIN_TEST_KEY, "192.0.2.1:1000")
                .await
                .unwrap_err()
                .code,
            423
        );
        assert_eq!(
            login_status(State(state.clone())).await.unwrap().0["loginLocked"],
            true
        );
        let restarted = reopened_login_lock_state(directory.path());
        assert!(restarted.controller.authorize(&headers).await.is_some());
        configure_access_key(directory.path(), LOGIN_TEST_KEY).unwrap();
        assert_eq!(
            login_status(State(restarted)).await.unwrap().0["loginLocked"],
            true
        );
        configure_access_key(directory.path(), "new-isolated-key").unwrap();
        assert!(
            login_lock_attempt(&state, "new-isolated-key", "192.0.2.1:1000")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn login_lock_concurrent_failures_block_late_success_and_stale_rotation_results() {
        let (directory, state) = login_lock_test_state();
        let snapshot = state
            .controller
            .auth
            .lock()
            .await
            .access_key
            .clone()
            .unwrap();
        let verified = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let late_success = {
            let state = state.clone();
            let key = snapshot.clone();
            let verified = verified.clone();
            let release = release.clone();
            tokio::spawn(async move {
                let checked_key = key.clone();
                let valid = tokio::task::spawn_blocking(move || checked_key.verify(LOGIN_TEST_KEY))
                    .await
                    .unwrap();
                verified.notify_one();
                release.notified().await;
                complete_login(
                    state,
                    key,
                    valid,
                    HeaderMap::new(),
                    RequestTrustContext::default(),
                )
                .await
            })
        };
        verified.notified().await;
        let mut failures = tokio::task::JoinSet::new();
        for _ in 0..4 {
            let state = state.clone();
            failures.spawn(async move {
                login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000")
                    .await
                    .unwrap_err()
                    .code
            });
        }
        let mut codes = Vec::new();
        while let Some(result) = failures.join_next().await {
            codes.push(result.unwrap());
        }
        codes.sort_unstable();
        assert_eq!(codes, vec![401, 401, 401, 423]);
        release.notify_one();
        assert_eq!(late_success.await.unwrap().unwrap_err().code, 423);
        assert!(state.controller.auth.lock().await.devices.is_empty());
        configure_access_key(directory.path(), "new-isolated-key").unwrap();
        for valid in [false, true] {
            assert_eq!(
                complete_login(
                    state.clone(),
                    snapshot.clone(),
                    valid,
                    HeaderMap::new(),
                    RequestTrustContext::default()
                )
                .await
                .unwrap_err()
                .code,
                401
            );
            assert_eq!(login_failure_count(directory.path()), 0);
        }
        assert!(
            login_lock_attempt(&state, "new-isolated-key", "192.0.2.1:1000")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn login_lock_cancelled_request_still_records_completed_key_verification() {
        let (directory, state) = login_lock_test_state();
        let request = {
            let state = state.clone();
            tokio::spawn(async move {
                login_lock_attempt(&state, "wrong-login-key", "192.0.2.1:1000").await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.controller.key_checks.available_permits() == 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        request.abort();
        let _ = request.await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while login_failure_count(directory.path()) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(login_failure_count(directory.path()), 1);
    }

    #[tokio::test]
    async fn login_lock_public_routes_report_lock_without_secrets_or_new_cookie() {
        let (_directory, mut state) = login_lock_test_state();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        state.settings.port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{}", state.settings.port);
        let router = web_router(state.controller.clone(), state.settings);
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .get(format!("{base_url}/api/login-status"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.headers()[reqwest::header::CACHE_CONTROL],
            "no-store"
        );
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"ok":true,"loginLocked":false})
        );
        let authorized_response = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":LOGIN_TEST_KEY}))
            .send()
            .await
            .unwrap();
        assert_eq!(authorized_response.status(), reqwest::StatusCode::OK);
        let existing_cookie = authorized_response.headers()[reqwest::header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        for index in 0..5 {
            let response = client
                .post(format!("{base_url}/api/connect"))
                .header(reqwest::header::ORIGIN, &base_url)
                .json(&json!({"key":if index == 4 { LOGIN_TEST_KEY } else { "wrong-login-key" }}))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status().as_u16(),
                if index >= 3 { 423 } else { 401 }
            );
            assert!(
                response
                    .headers()
                    .get(reqwest::header::SET_COOKIE)
                    .is_none()
            );
        }
        let status: Value = client
            .get(format!("{base_url}/api/login-status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status, json!({"ok":true,"loginLocked":true}));
        assert_eq!(
            client
                .get(format!("{base_url}/api/session"))
                .header(reqwest::header::COOKIE, existing_cookie)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::OK
        );
        server.abort();
        let _ = server.await;
    }

    struct RestartingNativeBridge {
        online: AtomicBool,
    }

    #[async_trait::async_trait]
    impl NativeRemoteBridge for RestartingNativeBridge {
        async fn call(&self, _action: &str, _payload: Value) -> anyhow::Result<Value> {
            anyhow::ensure!(self.online.load(Ordering::SeqCst), "native restarting");
            Ok(json!({"ok":true,"epoch":"test","cursor":0,"events":[]}))
        }
    }

    #[tokio::test]
    async fn native_restart_preserves_web_listener_and_device_authorization() {
        let directory = tempfile::Builder::new()
            .prefix("remote-native-restart-")
            .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp"))
            .unwrap();
        configure_access_key(directory.path(), "restart-key").unwrap();
        let native = Arc::new(RestartingNativeBridge {
            online: AtomicBool::new(true),
        });
        let controller =
            RemoteAccessController::new(native.clone(), directory.path().to_path_buf()).unwrap();
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        controller
            .reconcile(RemoteAccessSettings {
                enabled: true,
                port,
                ..RemoteAccessSettings::default()
            })
            .await;
        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let login = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":"restart-key"}))
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), reqwest::StatusCode::OK);
        let cookie = login.headers()[reqwest::header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        for online in [true, false, true] {
            native.online.store(online, Ordering::SeqCst);
            tokio::time::timeout(Duration::from_secs(3), async {
                while controller.native_connected.load(Ordering::SeqCst) != online {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(controller.running.load(Ordering::SeqCst));
            let response = client
                .get(format!("{base_url}/api/session"))
                .header(reqwest::header::COOKIE, &cookie)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let session: Value = response.json().await.unwrap();
            assert_eq!(session["nativeConnected"], online);
        }
        controller.reconcile(RemoteAccessSettings::default()).await;
    }

    #[tokio::test]
    async fn missing_credentials_allow_listener_but_reject_session_access() {
        let state_dir = tempfile::tempdir().unwrap();
        let controller = RemoteAccessController::new(
            Arc::new(OfflineNativeBridge),
            state_dir.path().to_path_buf(),
        )
        .unwrap();
        let port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        let settings = RemoteAccessSettings {
            enabled: true,
            port,
            ..RemoteAccessSettings::default()
        };

        controller.reconcile(settings.clone()).await;
        let status = controller.admin_status(&settings).await;
        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::new();
        let root_status = client
            .get(format!("{base_url}/"))
            .send()
            .await
            .unwrap()
            .status();
        let connect_response = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":"a".repeat(32)}))
            .send()
            .await
            .unwrap();
        let connect_status = connect_response.status();
        let connect_body: Value = connect_response.json().await.unwrap();
        let session_status = client
            .get(format!("{base_url}/api/session"))
            .send()
            .await
            .unwrap()
            .status();
        controller.reconcile(RemoteAccessSettings::default()).await;

        assert_eq!(status["running"], true);
        assert_eq!(status["accessKeyConfigured"], false);
        assert!(status["error"].is_null());
        assert_eq!(root_status, reqwest::StatusCode::OK);
        assert_eq!(connect_status, reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(connect_body["message"], "本机尚未设置访问密钥");
        assert_eq!(session_status, reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn short_keys_saved_after_startup_take_effect_without_restart() {
        let temp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        let state_dir = tempfile::Builder::new()
            .prefix("remote-key-test-")
            .tempdir_in(temp_root)
            .unwrap();
        let controller = RemoteAccessController::new(
            Arc::new(OfflineNativeBridge),
            state_dir.path().to_path_buf(),
        )
        .unwrap();
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let settings = RemoteAccessSettings {
            enabled: true,
            port,
            ..RemoteAccessSettings::default()
        };
        controller.reconcile(settings).await;
        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let first_key = "firstkey";
        let second_key = "next-key9";

        assert!(configure_access_key(state_dir.path(), "short!!").is_err());
        assert!(!state_dir.path().join("remote-access-key.json").exists());
        let short_login = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":"short!!"}))
            .send()
            .await
            .unwrap();
        assert_eq!(short_login.status(), reqwest::StatusCode::BAD_REQUEST);

        configure_access_key(state_dir.path(), first_key).unwrap();
        let credentials = access_credentials_status(state_dir.path()).unwrap();
        assert_eq!(credentials["accessKeyConfigured"], true);
        assert_eq!(credentials["key"], first_key);
        assert!(credentials.get("hash").is_none());
        let stored_key: Value = serde_json::from_slice(
            &std::fs::read(state_dir.path().join("remote-access-key.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stored_key["value"], first_key);
        assert!(
            stored_key["hash"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        let first_login = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":first_key}))
            .send()
            .await
            .unwrap();
        assert_eq!(first_login.status(), reqwest::StatusCode::OK);
        let first_cookie = first_login.headers()[reqwest::header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let first_session = client
            .get(format!("{base_url}/api/session"))
            .header(reqwest::header::COOKIE, &first_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(first_session.status(), reqwest::StatusCode::OK);

        configure_access_key(state_dir.path(), second_key).unwrap();
        let old_session = client
            .get(format!("{base_url}/api/session"))
            .header(reqwest::header::COOKIE, &first_cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(old_session.status(), reqwest::StatusCode::UNAUTHORIZED);
        let old_login = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":first_key}))
            .send()
            .await
            .unwrap();
        assert_eq!(old_login.status(), reqwest::StatusCode::UNAUTHORIZED);
        let new_login = client
            .post(format!("{base_url}/api/connect"))
            .header(reqwest::header::ORIGIN, &base_url)
            .json(&json!({"key":second_key}))
            .send()
            .await
            .unwrap();
        assert_eq!(new_login.status(), reqwest::StatusCode::OK);
        controller.reconcile(RemoteAccessSettings::default()).await;
    }

    #[test]
    fn app_resource_version_is_stable_and_changes_with_resource_content() {
        let first_assets = HashMap::from([
            (
                "/b.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"second"),
            ),
            (
                "/a.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"first"),
            ),
        ]);
        let reordered_assets = HashMap::from([
            (
                "/a.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"first"),
            ),
            (
                "/b.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"second"),
            ),
        ]);
        let changed_assets = HashMap::from([
            (
                "/a.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"changed"),
            ),
            (
                "/b.js",
                StaticWebAsset::new("application/javascript; charset=utf-8", b"second"),
            ),
        ]);

        let first = build_app_resources_manifest(&first_assets);
        let reordered = build_app_resources_manifest(&reordered_assets);
        let changed = build_app_resources_manifest(&changed_assets);

        assert_eq!(first.version, reordered.version);
        assert_ne!(first.version, changed.version);
        assert_eq!(first.files[0].path, "/a.js");
        assert_eq!(first.files[1].path, "/b.js");
        assert_eq!(first.schema_version, 1);
        assert_eq!(first.min_shell_version, 1);
        assert_eq!(first.version.len(), 64);
        assert!(
            first
                .version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }

    #[tokio::test]
    async fn app_resource_manifest_matches_public_routes_and_preserves_api_boundary() {
        let state_dir = tempfile::tempdir().unwrap();
        let controller = RemoteAccessController::new(
            Arc::new(OfflineNativeBridge),
            state_dir.path().to_path_buf(),
        )
        .unwrap();
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let settings = RemoteAccessSettings {
            enabled: true,
            port,
            ..RemoteAccessSettings::default()
        };
        controller.reconcile(settings).await;

        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let manifest_response = client
            .get(format!("{base_url}/app-resources.json"))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .unwrap();
        assert_eq!(manifest_response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            manifest_response
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-cache")
        );
        let manifest_etag = manifest_response
            .headers()
            .get(reqwest::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let manifest: Value = manifest_response.json().await.unwrap();
        let manifest_version = manifest["version"].as_str().unwrap();
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["minShellVersion"], 1);
        assert_eq!(manifest_version.len(), 64);
        assert!(
            manifest_version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );

        let files = manifest["files"].as_array().unwrap();
        let paths: Vec<_> = files
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        let mut sorted_paths = paths.clone();
        sorted_paths.sort_unstable();
        assert_eq!(paths, sorted_paths);
        assert!(!paths.contains(&"/app-resources.json"));
        assert!(!paths.contains(&"/native.js"));
        assert!(!paths.contains(&"/cache-recovery.html"));
        assert!(!paths.contains(&"/cache-recovery.js"));
        assert!(paths.iter().all(|path| !path.starts_with("/api/")));

        for (path, content_type) in [
            ("/cache-recovery.html", "text/html; charset=utf-8"),
            (
                "/cache-recovery.js",
                "application/javascript; charset=utf-8",
            ),
        ] {
            let response = client
                .get(format!("{base_url}{path}"))
                .header(reqwest::header::IF_NONE_MATCH, "*")
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(
                response.headers()[reqwest::header::CONTENT_TYPE],
                content_type
            );
            assert_eq!(
                response.headers()[reqwest::header::CACHE_CONTROL],
                "no-store"
            );
            assert!(!response.headers().contains_key(reqwest::header::ETAG));
            assert!(!response.bytes().await.unwrap().is_empty());
        }

        for file in files {
            let path = file["path"].as_str().unwrap();
            let response = client
                .get(format!("{base_url}{path}"))
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK, "{path}");
            assert_eq!(
                response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok()),
                file["contentType"].as_str(),
                "{path}"
            );
            let bytes = response.bytes().await.unwrap();
            assert_eq!(bytes.len() as u64, file["size"].as_u64().unwrap(), "{path}");
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                file["sha256"].as_str().unwrap(),
                "{path}"
            );
        }

        let repeated_manifest: Value = client
            .get(format!("{base_url}/app-resources.json"))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(repeated_manifest["version"], manifest["version"]);
        let revalidated_manifest = client
            .get(format!("{base_url}/app-resources.json"))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .header(reqwest::header::IF_NONE_MATCH, manifest_etag)
            .send()
            .await
            .unwrap();
        assert_eq!(
            revalidated_manifest.status(),
            reqwest::StatusCode::NOT_MODIFIED
        );

        let compressed_asset = client
            .get(format!("{base_url}/app.js"))
            .header(reqwest::header::ACCEPT_ENCODING, "gzip")
            .send()
            .await
            .unwrap();
        assert_eq!(compressed_asset.status(), reqwest::StatusCode::OK);
        assert_eq!(
            compressed_asset
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip")
        );
        let compressed_etag = compressed_asset
            .headers()
            .get(reqwest::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let cached_compressed_asset = client
            .get(format!("{base_url}/app.js"))
            .header(reqwest::header::ACCEPT_ENCODING, "gzip")
            .header(reqwest::header::IF_NONE_MATCH, compressed_etag)
            .send()
            .await
            .unwrap();
        assert_eq!(
            cached_compressed_asset.status(),
            reqwest::StatusCode::NOT_MODIFIED
        );

        let unauthenticated_api = client
            .get(format!("{base_url}/api/session"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unauthenticated_api.status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            unauthenticated_api
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );

        let bad_host = client
            .get(format!("{base_url}/app-resources.json"))
            .header(reqwest::header::HOST, "untrusted.example")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_host.status(), reqwest::StatusCode::FORBIDDEN);
        let bad_origin = client
            .get(format!("{base_url}/app-resources.json"))
            .header(reqwest::header::ORIGIN, "https://untrusted.example")
            .send()
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), reqwest::StatusCode::FORBIDDEN);

        controller.reconcile(RemoteAccessSettings::default()).await;
    }
}
