use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, broadcast};

use crate::settings::{RemoteAccessSettings, SettingsStore};

mod credentials;
mod funnel;
use credentials::{AccessKey, load_access_key};
pub use credentials::{access_credentials_status, configure_access_key};
use funnel::FunnelService;

const NATIVE_SCRIPT: &str = include_str!("../../../assets/remote-access/native.js");
const WEB_HTML: &str = include_str!("../../../assets/remote-access/index.html");
const WEB_JS: &str = include_str!("../../../assets/remote-access/app.js");
const WEB_CSS: &str = include_str!("../../../assets/remote-access/style.css");
const COOKIE_NAME: &str = "codex_elves_device";
const MAX_DEVICES: usize = 32;
const MAX_RECEIPTS: usize = 20_000;
static CONTROLLER: OnceLock<Arc<RemoteAccessController>> = OnceLock::new();
static STARTUP_ERROR: OnceLock<std::sync::Mutex<Option<String>>> = OnceLock::new();

#[async_trait]
pub trait NativeRemoteBridge: Send + Sync {
    async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value>;
}

pub struct NativeCdpBridge;

#[async_trait]
impl NativeRemoteBridge for NativeCdpBridge {
    async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        let status = crate::status::StatusStore::default()
            .load_latest()?
            .filter(|status| status.status == "running")
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
        let response = crate::bridge::evaluate_script_value_with_timeout(
            &websocket,
            &expression,
            Duration::from_secs(45),
        )
        .await?;
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
    login_attempts: HashMap<IpAddr, (u64, u8)>,
    receipts: HashMap<String, Receipt>,
}

struct LiveService {
    settings: Option<RemoteAccessSettings>,
    handle: Option<axum_server::Handle<std::net::SocketAddr>>,
    server: Option<tokio::task::JoinHandle<()>>,
    pump: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
    funnel: FunnelService,
}

pub struct RemoteAccessController {
    native: Arc<dyn NativeRemoteBridge>,
    state_dir: PathBuf,
    auth: Mutex<AuthState>,
    live: Mutex<LiveService>,
    running: AtomicBool,
    native_connected: AtomicBool,
    events: broadcast::Sender<Value>,
    key_checks: Arc<tokio::sync::Semaphore>,
    funnel_origin: Arc<RwLock<Option<String>>>,
}

impl RemoteAccessController {
    pub fn new(
        native: Arc<dyn NativeRemoteBridge>,
        state_dir: PathBuf,
    ) -> anyhow::Result<Arc<Self>> {
        let devices = read_json_or_default(&state_dir.join("remote-access-devices.json"))?;
        let access_key = load_access_key(&state_dir)?;
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
        let funnel_origin = Arc::new(RwLock::new(None));
        Ok(Arc::new(Self {
            native,
            state_dir,
            auth: Mutex::new(AuthState {
                devices,
                access_key,
                receipts,
                ..AuthState::default()
            }),
            live: Mutex::new(LiveService {
                settings: None,
                handle: None,
                server: None,
                pump: None,
                error: None,
                funnel: FunnelService::new(funnel_origin.clone()),
            }),
            running: AtomicBool::new(false),
            native_connected: AtomicBool::new(false),
            events,
            key_checks: Arc::new(tokio::sync::Semaphore::new(4)),
            funnel_origin,
        }))
    }

    fn funnel_origin(&self) -> Option<String> {
        self.funnel_origin
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    async fn save_devices(&self, state: &AuthState) -> anyhow::Result<()> {
        crate::settings::atomic_write(
            &self.state_dir.join("remote-access-devices.json"),
            &serde_json::to_vec(&state.devices)?,
        )
    }

    async fn save_receipts(&self, state: &AuthState) -> anyhow::Result<()> {
        crate::settings::atomic_write(
            &self.state_dir.join("remote-access-receipts.json"),
            &serde_json::to_vec(&state.receipts)?,
        )
    }

    async fn reload_access_key(&self) -> anyhow::Result<bool> {
        let key = load_access_key(&self.state_dir)?;
        let mut auth = self.auth.lock().await;
        if auth.access_key != key {
            auth.devices
                .retain(|device| key.as_ref().is_some_and(|key| device.key_id == key.id));
            auth.access_key = key;
            let _ = self
                .events
                .send(json!({"method":"remote/authorizationChanged"}));
        }
        Ok(auth.access_key.is_some())
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

    pub async fn reconcile(self: &Arc<Self>, settings: RemoteAccessSettings) {
        let key_error = match self.reload_access_key().await {
            Ok(false) if settings.enabled => {
                Some("请先在管理器设置访问密钥，再启用远程网页".to_string())
            }
            Err(error) => Some(error.to_string()),
            _ => None,
        };
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
            live.funnel
                .reconcile(&settings, self.running.load(Ordering::SeqCst))
                .await;
            return;
        }
        live.funnel.restart(&funnel::local_target(&settings)).await;
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
            live.funnel.reconcile(&settings, false).await;
            if finished || was_enabled {
                let _ = self.native.call("dispose", json!({})).await;
            }
            return;
        }
        if let Some(error) = key_error {
            live.error = Some(error);
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
        live.funnel
            .reconcile(&settings, self.running.load(Ordering::SeqCst))
            .await;
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
        let tls = if settings.tls_certificate_path.is_empty() {
            None
        } else {
            let _ = rustls::crypto::ring::default_provider().install_default();
            Some(
                axum_server::tls_rustls::RustlsConfig::from_pem_file(
                    &settings.tls_certificate_path,
                    &settings.tls_private_key_path,
                )
                .await
                .context("无法加载 HTTPS 证书或私钥")?,
            )
        };
        let router = web_router(self.clone(), settings);
        let handle = axum_server::Handle::new();
        let server_handle = handle.clone();
        let controller = self.clone();
        let server = if let Some(tls) = tls {
            let server = axum_server::from_tcp_rustls(listener, tls)?.handle(server_handle);
            tokio::spawn(async move {
                let _ = server
                    .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                    .await;
                controller.running.store(false, Ordering::SeqCst);
            })
        } else {
            let server = axum_server::from_tcp(listener)?.handle(server_handle);
            tokio::spawn(async move {
                let _ = server
                    .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                    .await;
                controller.running.store(false, Ordering::SeqCst);
            })
        };
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
                            let _ = controller.events.send(event.clone());
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

    pub async fn admin(self: &Arc<Self>, action: &str, payload: Value) -> anyhow::Result<Value> {
        match action {
            "status" | "apply" => {
                let settings = SettingsStore::default().load()?.remote_access;
                if action == "apply" {
                    self.live
                        .lock()
                        .await
                        .funnel
                        .restart(&funnel::local_target(&settings))
                        .await;
                }
                if payload["check"] == true {
                    self.live.lock().await.funnel.invalidate();
                }
                self.reconcile(settings.clone()).await;
                let live = self.live.lock().await;
                let error = live.error.clone();
                let funnel = live.funnel.status(&settings);
                drop(live);
                let auth = self.auth.lock().await;
                Ok(json!({
                    "status":"ok", "enabled":settings.enabled,
                    "running":self.running.load(Ordering::SeqCst),
                    "nativeConnected":self.native_connected.load(Ordering::SeqCst),
                    "error":error, "addresses":access_addresses(&settings),
                    "devices":auth.devices.iter().filter(|device|device.expires_at_ms>now_ms())
                        .filter(|device|auth.access_key.as_ref().is_some_and(|key|device.key_id==key.id))
                        .map(Device::public).collect::<Vec<_>>(),
                    "settings":settings, "funnel":funnel,
                    "accessKeyConfigured":auth.access_key.is_some()
                }))
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
        if !self.running.load(Ordering::SeqCst) || !self.reload_access_key().await.ok()? {
            return None;
        }
        let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
        let token = cookies.split(';').find_map(|cookie| {
            let (name, value) = cookie.trim().split_once('=')?;
            (name == COOKIE_NAME).then_some(value)
        })?;
        let digest = hash(token);
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
                return Ok(receipt_value(receipt));
            }
            if auth.receipts.len() >= MAX_RECEIPTS {
                return Err(ApiError::new(429, "消息回执已达上限"));
            }
            auth.receipts.insert(
                key.clone(),
                Receipt {
                    device_id: device.id,
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
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let operation_id = request_id.clone();
        tokio::spawn(async move {
            let result = controller.native.call(&action, payload).await;
            let mut auth = controller.auth.lock().await;
            let receipt = auth.receipts.get_mut(&key).unwrap();
            match result {
                Ok(value) => {
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
        match tokio::time::timeout(Duration::from_secs(40), receiver).await {
            Ok(Ok(value)) => Ok(value),
            _ => Ok(json!({"requestId":request_id,"state":"pending"})),
        }
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

pub async fn local_settings_command(action: &str, payload: Value) -> anyhow::Result<Value> {
    let directory = crate::paths::default_app_state_dir();
    if action == "tailscaleStatus" {
        return Ok(funnel::probe_tailscale().await);
    }
    if action == "installTailscale" {
        return funnel::install_tailscale(&directory).await;
    }
    if action == "tailscaleInstallProgress" {
        return Ok(funnel::installer_progress());
    }
    let action = action.to_string();
    tokio::task::spawn_blocking(move || {
        let directory = crate::paths::default_app_state_dir();
        match action.as_str() {
            "credentials" => access_credentials_status(&directory),
            "setKey" => {
                configure_access_key(&directory, payload["key"].as_str().context("缺少访问密钥")?)?;
                Ok(json!({"status":"ok","accessKeyConfigured":true}))
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
pub async fn request_local_admin(action: &str, payload: Value) -> anyhow::Result<Value> {
    anyhow::ensure!(
        matches!(action, "status" | "apply" | "revoke"),
        "不支持的远程管理操作"
    );
    let status = crate::status::StatusStore::default()
        .load_latest()?
        .filter(|status| status.status == "running")
        .context("请先通过 CodexElves 启动 Codex")?;
    let port = status.helper_port.context("本机管理连接不可用")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(12))
        .build()?;
    let response = client
        .post(format!("http://127.0.0.1:{port}/remote-access/{action}"))
        .json(&payload)
        .send()
        .await
        .context("本机后台未连接，请在保存设置后重启 CodexElves launcher")?;
    anyhow::ensure!(
        response.status().is_success(),
        "请启动包含远程会话功能的新版 CodexElves launcher"
    );
    response
        .json::<Value>()
        .await
        .context("远程访问服务未返回有效状态")
}

#[derive(Clone)]
struct WebState {
    controller: Arc<RemoteAccessController>,
    settings: RemoteAccessSettings,
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
        .route("/", get(|| async { Html(WEB_HTML) }))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    WEB_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], WEB_CSS) }),
        )
        .route("/api/connect", post(connect))
        .route("/api/session", get(session))
        .route("/api/logout", post(logout))
        .route("/api/read/{action}", post(read))
        .route("/api/action/{action}", post(action))
        .route("/api/receipts/{id}", get(receipt))
        .route("/api/events", get(events))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            access_boundary,
        ))
        .with_state(state)
}

#[derive(Debug)]
struct ApiError {
    code: u16,
    message: String,
}

impl ApiError {
    fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.code).unwrap_or(StatusCode::BAD_REQUEST),
            Json(json!({"ok":false,"message":self.message})),
        )
            .into_response()
    }
}

async fn access_boundary(State(state): State<WebState>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let external: Vec<String> = state.controller.funnel_origin().into_iter().collect();
    let mut origins = allowed_origins(&state.settings);
    origins.extend(external.iter().cloned());
    let scheme = if state.settings.tls_certificate_path.is_empty() {
        "http"
    } else {
        "https"
    };
    let local_origin = format!("{scheme}://{host}");
    let valid_host = origins
        .iter()
        .any(|origin| origin.ends_with(&format!("://{host}")));
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let bad_origin = origin.is_some_and(|origin| !origins.iter().any(|allowed| allowed == origin));
    let missing_origin = request.method() == axum::http::Method::POST && origin.is_none();
    let cross_site = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        == Some("cross-site");
    if !valid_host || bad_origin || missing_origin || cross_site {
        return ApiError::new(403, "只允许从已配置的访问地址操作").into_response();
    }
    if origin.is_some_and(|origin| {
        origin != local_origin && !external.iter().any(|allowed| allowed == origin)
    }) {
        return ApiError::new(403, "请求来源与访问地址不一致").into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("content-security-policy", HeaderValue::from_static(
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
    ));
    if !state.settings.tls_certificate_path.is_empty() {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

async fn authorized(state: &WebState, headers: &HeaderMap) -> Result<Device, ApiError> {
    if !state.controller.running.load(Ordering::SeqCst) {
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    if !state.controller.running.load(Ordering::SeqCst) {
        return Err(ApiError::new(503, "服务已关闭"));
    }
    let key_value = payload["key"].as_str().unwrap_or("").trim();
    if !(32..=256).contains(&key_value.len())
        || !key_value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError::new(
            400,
            "请填写 32–256 个非空格 ASCII 字符的访问密钥",
        ));
    }
    state
        .controller
        .reload_access_key()
        .await
        .map_err(|_| ApiError::new(503, "访问密钥无法读取，请在本机管理器检查"))?;
    let ip = peer.ip();
    let key = {
        let mut auth = state.controller.auth.lock().await;
        let now = now_ms();
        auth.login_attempts
            .retain(|_, (start, _)| now.saturating_sub(*start) < 60_000);
        if auth.login_attempts.len() >= 1024 && !auth.login_attempts.contains_key(&ip) {
            return Err(ApiError::new(429, "连接尝试过多，请稍后再试"));
        }
        let attempt = auth.login_attempts.entry(ip).or_insert((now, 0));
        if attempt.1 >= 8 {
            return Err(ApiError::new(429, "密钥尝试过多，请一分钟后再试"));
        }
        attempt.1 += 1;
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
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        checked_key.verify(&candidate)
    })
    .await
    .map_err(|_| ApiError::new(503, "无法验证访问密钥，请稍后再试"))?;
    if !valid {
        return Err(ApiError::new(401, "访问密钥不正确"));
    }
    if !state.controller.running.load(Ordering::SeqCst)
        || load_access_key(&state.controller.state_dir)
            .ok()
            .flatten()
            .is_none_or(|current| current != key)
    {
        return Err(ApiError::new(401, "访问密钥刚被更换，请使用新密钥"));
    }
    let mut auth = state.controller.auth.lock().await;
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
        key_id: key.id,
    };
    auth.devices.push(device.clone());
    if state.controller.save_devices(&auth).await.is_err() {
        auth.devices.pop();
        return Err(ApiError::new(503, "设备授权保存失败"));
    }
    let secure = !state.settings.tls_certificate_path.is_empty()
        || state.controller.funnel_origin().is_some();
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
    Ok(Json(
        json!({"ok":true,"device":device.public(),"nativeConnected":state.controller.native_connected.load(Ordering::SeqCst),
        "readOnly":state.settings.read_only,"allowApprovals":state.settings.allow_approvals}),
    ))
}

async fn logout(State(state): State<WebState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    state
        .controller
        .admin("revoke", json!({"deviceId":device.id}))
        .await
        .map_err(|_| ApiError::new(503, "撤销授权失败"))?;
    Ok((
        [(
            header::SET_COOKIE,
            format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}

async fn read(
    State(state): State<WebState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers).await?;
    if !matches!(
        action.as_str(),
        "sidebar" | "history" | "thread" | "models" | "skills"
    ) {
        return Err(ApiError::new(404, "不支持的读取操作"));
    }
    let value = state
        .controller
        .native
        .call(&action, payload)
        .await
        .map_err(|_| ApiError::new(503, "Codex 桌面未连接，请在本机检查"))?;
    native_result(value).map(Json)
}

async fn action(
    State(state): State<WebState>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    if state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    if !matches!(
        action.as_str(),
        "send"
            | "stop"
            | "create"
            | "pinProject"
            | "pinThread"
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
    if matches!(action.as_str(), "send" | "create")
        && payload["prompt"]
            .as_str()
            .is_none_or(|text| text.trim().is_empty() || text.len() > 128 * 1024)
    {
        return Err(ApiError::new(400, "消息不能为空或超过 128 KiB"));
    }
    if matches!(action.as_str(), "send" | "create")
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
    let id = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
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
    Ok(Json(receipt_value(receipt)))
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
            if !controller.running.load(Ordering::SeqCst) {
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
            let done = !authorized || !controller.running.load(Ordering::SeqCst);
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

pub fn access_addresses(settings: &RemoteAccessSettings) -> Vec<String> {
    let scheme = if settings.tls_certificate_path.is_empty() {
        "http"
    } else {
        "https"
    };
    let mut addresses = Vec::new();
    if settings.bind_host == "0.0.0.0" {
        addresses.extend(
            crate::lan_proxy::lan_ipv4_addresses()
                .iter()
                .map(|ip| format!("{scheme}://{ip}:{}", settings.port)),
        );
        addresses.push(format!("{scheme}://127.0.0.1:{}", settings.port));
    } else {
        let host = if settings.bind_host.contains(':') {
            format!("[{}]", settings.bind_host)
        } else {
            settings.bind_host.clone()
        };
        addresses.push(format!("{scheme}://{host}:{}", settings.port));
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
        let scheme = if settings.tls_certificate_path.is_empty() {
            "http"
        } else {
            "https"
        };
        addresses.push(format!("{scheme}://localhost:{}", settings.port));
    }
    addresses
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
