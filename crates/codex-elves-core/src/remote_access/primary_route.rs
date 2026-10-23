use std::path::Path;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use anyhow::Context;
use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{
    LiveService, RemoteAccessController, RemoteAccessSettings, frp_tunnel, now_ms,
    origin_matches_host,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Provider {
    #[default]
    Zrok,
    Frp,
}

#[derive(Clone)]
struct Probe {
    origin: String,
    nonce: String,
    ingress: Option<String>,
    expires: u64,
}

#[derive(Default)]
struct Selection {
    provider: Provider,
    origin: Option<String>,
    ingress: Option<String>,
}

pub(super) struct Primary {
    selection: RwLock<Selection>,
    probe: RwLock<Option<Probe>>,
    operation: Mutex<()>,
    pub busy: AtomicBool,
    pub generation: AtomicU64,
    pub frp_download: frp_tunnel::DownloadProgress,
    last_error: RwLock<Option<String>>,
}

impl Primary {
    pub fn try_operation(&self) -> anyhow::Result<tokio::sync::MutexGuard<'_, ()>> {
        self.operation
            .try_lock()
            .map_err(|_| anyhow::anyhow!("主入口正在验证或切换，请稍后重试"))
    }

    pub fn new(directory: &Path) -> anyhow::Result<Self> {
        let provider = super::read_json_or_default(&directory.join("remote-access-primary.json"))?;
        let origin = if provider == Provider::Frp {
            let saved = frp_tunnel::Saved::load(directory, true)?;
            saved.config.validate()?;
            anyhow::ensure!(
                frp_tunnel::valid_origin(&saved.config.public_origin),
                "已选 FRP 主入口缺少有效公网地址"
            );
            Some(saved.config.public_origin)
        } else {
            None
        };
        Ok(Self {
            selection: RwLock::new(Selection {
                provider,
                origin,
                ingress: None,
            }),
            probe: RwLock::new(None),
            operation: Mutex::new(()),
            busy: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            frp_download: frp_tunnel::DownloadProgress::default(),
            last_error: RwLock::new(None),
        })
    }

    pub fn provider(&self) -> Provider {
        self.selection
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .provider
    }
    pub fn frp_origin(&self) -> Option<String> {
        self.selection
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .origin
            .clone()
    }

    pub fn permits_frp(&self, id: Option<&str>) -> bool {
        let selection = self.selection.read().unwrap_or_else(|e| e.into_inner());
        selection.provider == Provider::Frp
            && selection
                .ingress
                .as_deref()
                .is_some_and(|expected| Some(expected) == id)
    }

    fn set_ingress(&self, running: Option<&frp_tunnel::Running>) {
        let mut selection = self.selection.write().unwrap_or_else(|e| e.into_inner());
        selection.ingress = running.map(|r| r.id.clone());
    }

    pub fn probe_response(
        &self,
        host: &str,
        ingress: Option<&str>,
        request: &super::Request,
    ) -> Option<Response> {
        let permit = self
            .probe
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        if now_ms() > permit.expires
            || !origin_matches_host(&permit.origin, host)
            || permit.ingress.as_deref() != ingress
        {
            return None;
        }
        if request.method() != axum::http::Method::GET
            || request.headers().contains_key(header::AUTHORIZATION)
            || request.headers().contains_key(header::COOKIE)
        {
            return Some(StatusCode::FORBIDDEN.into_response());
        }
        if request.uri().path() == "/api/entry-check"
            && request.uri().query() == Some(format!("challenge={}", permit.nonce).as_str())
        {
            let mut response = Json(json!({"challenge":permit.nonce})).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            return Some(response);
        }
        if request.uri().path() == "/api/session" {
            return Some(StatusCode::UNAUTHORIZED.into_response());
        }
        // A trial origin never receives a login page or real session data.
        Some(StatusCode::FORBIDDEN.into_response())
    }

    fn commit(
        &self,
        directory: &Path,
        provider: Provider,
        running: Option<&frp_tunnel::Running>,
    ) -> anyhow::Result<()> {
        if let Some(running) = running {
            running.saved.store(directory, true)?;
        }
        crate::settings::atomic_write(
            &directory.join("remote-access-primary.json"),
            &serde_json::to_vec(&provider)?,
        )?;
        *self.selection.write().unwrap_or_else(|e| e.into_inner()) = Selection {
            provider,
            origin: running.map(|r| r.saved.config.public_origin.clone()),
            ingress: running.map(|r| r.id.clone()),
        };
        Ok(())
    }

    fn save_stopped(&self, directory: &Path, provider: Provider) -> anyhow::Result<()> {
        if self.provider() == provider {
            return Ok(());
        }
        let origin = if provider == Provider::Frp {
            let saved = frp_tunnel::Saved::load(directory, false)?;
            saved.config.validate()?;
            anyhow::ensure!(
                frp_tunnel::valid_origin(&saved.config.public_origin),
                "请先填写服务器地址和公网映射端口"
            );
            saved.store(directory, true)?;
            Some(saved.config.public_origin)
        } else {
            None
        };
        crate::settings::atomic_write(
            &directory.join("remote-access-primary.json"),
            &serde_json::to_vec(&provider)?,
        )?;
        *self.selection.write().unwrap_or_else(|e| e.into_inner()) = Selection {
            provider,
            origin,
            ingress: None,
        };
        Ok(())
    }
}

struct Trial<'a>(&'a Primary);
impl Drop for Trial<'_> {
    fn drop(&mut self) {
        *self.0.probe.write().unwrap_or_else(|e| e.into_inner()) = None;
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

impl RemoteAccessController {
    pub(super) fn primary_status(&self, live: &LiveService) -> Value {
        let saved = frp_tunnel::Saved::load(&self.state_dir, false).unwrap_or_default();
        let mut frp = saved.public();
        let download = self.primary.frp_download.status();
        for field in [
            "version",
            "downloadedBytes",
            "downloadTotalBytes",
            "downloadPercent",
        ] {
            frp[field] = download[field].clone();
        }
        frp["state"] = json!(if download["downloading"] == true {
            "downloading"
        } else if live.frp_start.is_some() {
            "starting"
        } else if live.frp.is_some() {
            "ready"
        } else if live.frp_error.is_some() {
            "error"
        } else {
            "stopped"
        });
        frp["running"] = json!(live.frp.is_some());
        frp["error"] = json!(live.frp_error);
        json!({
            "provider":self.primary.provider(),
            "publicUrl":self.public_origin(),
            "switching":self.primary.busy.load(Ordering::SeqCst),
            "error":self.primary.last_error.read().unwrap_or_else(|e|e.into_inner()).clone(),
            "frp":frp
        })
    }

    pub(super) async fn stop_frp(&self, live: &mut LiveService) {
        if let Some(task) = live.frp_start.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(running) = live.frp.take() {
            running.stop().await;
        }
        self.primary.set_ingress(None);
    }

    pub(super) async fn reconcile_primary(
        self: &Arc<Self>,
        live: &mut LiveService,
        settings: &RemoteAccessSettings,
    ) {
        let enabled = self.running.load(Ordering::SeqCst);
        let target = super::zrok_target(settings);
        if !enabled {
            self.stop_frp(live).await;
            live.zrok.reconcile(false, &target).await;
            live.ngrok.stop().await;
            return;
        }
        if self.primary.busy.load(Ordering::SeqCst) {
            return;
        }
        match self.primary.provider() {
            Provider::Zrok => {
                self.stop_frp(live).await;
                live.zrok.reconcile(true, &target).await;
                let status = live.zrok.status();
                *self
                    .channels
                    .primary
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = status["publicUrl"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
                let configured = status["bound"] == true
                    && status["publicUrl"].as_str().is_some_and(|s| !s.is_empty());
                live.ngrok.reconcile(configured, &target).await;
            }
            Provider::Frp => {
                live.zrok.reconcile(false, &target).await;
                // Preserve the existing ngrok process, credentials and routing.
                live.ngrok.reconcile(true, &target).await;
                if live.frp.as_mut().is_some_and(|running| !running.alive()) {
                    live.frp = None;
                    self.primary.set_ingress(None);
                    live.frp_error = Some("FRP 连接已结束，等待自动重试".into());
                    live.frp_retry_at = now_ms() + 30_000;
                }
                if live
                    .frp_start
                    .as_ref()
                    .is_some_and(|task| task.is_finished())
                {
                    let result = live.frp_start.take().unwrap().await;
                    match result {
                        Ok(Ok(running)) => {
                            self.primary.set_ingress(Some(&running));
                            live.frp = Some(running);
                            live.frp_error = None;
                        }
                        result => {
                            live.frp_error = Some(match result {
                                Ok(Err(error)) => error.to_string(),
                                _ => "FRP 主入口启动任务已结束，等待重试".into(),
                            });
                            live.frp_retry_at = now_ms() + 30_000;
                        }
                    }
                }
                if live.frp.is_none() && live.frp_start.is_none() && now_ms() >= live.frp_retry_at {
                    let controller = self.clone();
                    let settings = settings.clone();
                    live.frp_start = Some(tokio::spawn(async move {
                        let _operation = controller.primary.operation.lock().await;
                        let saved = frp_tunnel::Saved::load(&controller.state_dir, true)?;
                        let generation = controller.primary.generation.load(Ordering::SeqCst);
                        controller.primary.busy.store(true, Ordering::SeqCst);
                        let _trial = Trial(&controller.primary);
                        let https = saved.https_configured();
                        let running =
                            frp_tunnel::start(&controller, &settings, saved, https).await?;
                        controller
                            .probe_entry(
                                &running.saved.config.public_origin,
                                Some(running.id.clone()),
                            )
                            .await?;
                        anyhow::ensure!(
                            controller.running.load(Ordering::SeqCst)
                                && controller.primary.generation.load(Ordering::SeqCst)
                                    == generation,
                            "远程配置已变化"
                        );
                        Ok(running)
                    }));
                }
            }
        }
    }

    async fn probe_entry(&self, origin: &str, ingress: Option<String>) -> anyhow::Result<()> {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        *self
            .primary
            .probe
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Probe {
            origin: origin.into(),
            nonce: nonce.clone(),
            ingress,
            expires: now_ms() + 45_000,
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        loop {
            let checked = async {
                let response = client
                    .get(format!("{origin}/api/entry-check?challenge={nonce}"))
                    .header("skip_zrok_interstitial", "1")
                    .send()
                    .await?;
                anyhow::ensure!(response.status().is_success(), "入口未返回有效响应");
                anyhow::ensure!(
                    response.content_length().unwrap_or(0) <= 4096,
                    "入口验证响应过大"
                );
                let mut bytes = Vec::new();
                let mut response = response;
                while let Some(chunk) = response.chunk().await? {
                    anyhow::ensure!(bytes.len() + chunk.len() <= 4096, "入口验证响应过大");
                    bytes.extend_from_slice(&chunk);
                }
                let value: Value = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(value["challenge"] == nonce, "公网入口未回到当前服务");
                let protected = client
                    .get(format!("{origin}/api/session"))
                    .header("skip_zrok_interstitial", "1")
                    .send()
                    .await?;
                anyhow::ensure!(
                    protected.status() == StatusCode::UNAUTHORIZED,
                    "未登录会话访问边界异常"
                );
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if checked.is_ok() {
                return checked;
            }
            if tokio::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "公网入口验证失败，请检查访问地址、端口转发和服务商配置；原主入口未切换"
                );
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub(super) async fn primary_command(
        self: &Arc<Self>,
        action: &str,
        payload: Value,
    ) -> anyhow::Result<Value> {
        let _operation = self.primary.try_operation()?;
        if action == "saveFrp" {
            let saved = frp_tunnel::Saved::from_payload(&self.state_dir, payload)?;
            saved.store(&self.state_dir, false)?;
            return Ok(
                json!({"status":"ok","frp":saved.public(),"message":"FRP 候选配置已保存，当前主入口未改变"}),
            );
        }
        let provider: Provider =
            serde_json::from_value(payload["provider"].clone()).context("请选择 zrok 或 FRP")?;
        let apply = action == "applyPrimaryRoute";
        let settings = crate::settings::SettingsStore::default()
            .load()?
            .remote_access;
        if apply && !settings.enabled {
            let _live = self.live.lock().await;
            anyhow::ensure!(
                !self.running.load(Ordering::SeqCst),
                "远程服务尚未完成停止，请稍后保存主入口选择"
            );
            self.primary.save_stopped(&self.state_dir, provider)?;
            return Ok(
                json!({"status":"ok","message":"主链路选择已保存，启用远程会话网页后验证并启动。"}),
            );
        }
        anyhow::ensure!(
            settings.enabled && self.running.load(Ordering::SeqCst),
            "请先启用远程会话网页"
        );
        if apply {
            anyhow::ensure!(
                self.native_connected.load(Ordering::SeqCst),
                super::DESKTOP_START_GUIDANCE
            );
        }
        let original = self.primary.provider();
        let generation = self.primary.generation.load(Ordering::SeqCst);
        self.primary.busy.store(true, Ordering::SeqCst);
        let _trial = Trial(&self.primary);
        *self
            .primary
            .last_error
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        let result = self
            .run_primary_trial(provider, apply, original, generation, &settings)
            .await;
        if original == Provider::Frp && self.primary.provider() == Provider::Frp {
            self.live
                .lock()
                .await
                .zrok
                .reconcile(false, &super::zrok_target(&settings))
                .await;
        }
        if let Err(error) = &result {
            *self
                .primary
                .last_error
                .write()
                .unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
        }
        result
    }

    async fn run_primary_trial(
        self: &Arc<Self>,
        provider: Provider,
        apply: bool,
        original: Provider,
        generation: u64,
        settings: &RemoteAccessSettings,
    ) -> anyhow::Result<Value> {
        let mut candidate = None;
        let (origin, ingress, https) = match provider {
            Provider::Frp => {
                let saved = frp_tunnel::Saved::load(&self.state_dir, false)?;
                saved.config.validate()?;
                let https = saved.https_configured();
                let mut live = self.live.lock().await;
                if original == Provider::Frp {
                    let running = live
                        .frp
                        .as_mut()
                        .context("FRP 主入口未运行，请等待重连后测试")?;
                    anyhow::ensure!(
                        running.saved == saved,
                        "FRP 候选配置已改变；请先切回 zrok，再测试并应用新配置，以保留原入口"
                    );
                    anyhow::ensure!(running.alive(), "FRP 主入口已断开，请等待重连");
                    (
                        saved.config.public_origin.clone(),
                        Some(running.id.clone()),
                        https,
                    )
                } else {
                    drop(live);
                    let running = frp_tunnel::start(self, settings, saved, https).await?;
                    let origin = running.saved.config.public_origin.clone();
                    let id = running.id.clone();
                    candidate = Some(running);
                    (origin, Some(id), https)
                }
            }
            Provider::Zrok => {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                let origin = loop {
                    let mut live = self.live.lock().await;
                    live.zrok
                        .reconcile(true, &super::zrok_target(settings))
                        .await;
                    let status = live.zrok.status();
                    if status["running"] == true {
                        break status["publicUrl"]
                            .as_str()
                            .context("zrok 地址未就绪")?
                            .to_owned();
                    }
                    anyhow::ensure!(status["installed"] == true, "请先下载并绑定 zrok");
                    anyhow::ensure!(
                        tokio::time::Instant::now() < deadline,
                        "zrok 未启动，请先完成绑定和固定名称配置"
                    );
                    drop(live);
                    tokio::time::sleep(Duration::from_millis(400)).await;
                };
                (origin, None, true)
            }
        };
        self.probe_entry(&origin, ingress).await?;
        anyhow::ensure!(
            self.running.load(Ordering::SeqCst)
                && self.primary.generation.load(Ordering::SeqCst) == generation,
            "验证期间远程配置已变化，未应用切换"
        );
        if apply && provider != original {
            let mut live = self.live.lock().await;
            anyhow::ensure!(
                self.running.load(Ordering::SeqCst)
                    && self.primary.generation.load(Ordering::SeqCst) == generation,
                "远程配置已变化，未应用切换"
            );
            match provider {
                Provider::Frp => anyhow::ensure!(
                    candidate.as_mut().is_some_and(|running| running.alive()),
                    "FRP 在验证后已退出，原主入口未切换"
                ),
                Provider::Zrok => anyhow::ensure!(
                    live.zrok.status()["running"] == true,
                    "zrok 在验证后已退出，原主入口未切换"
                ),
            }
            self.primary
                .commit(&self.state_dir, provider, candidate.as_ref())?;
            match provider {
                Provider::Frp => {
                    live.frp = candidate.take();
                    live.frp_error = None;
                    live.zrok
                        .reconcile(false, &super::zrok_target(settings))
                        .await;
                }
                Provider::Zrok => {
                    self.stop_frp(&mut live).await;
                }
            }
            // No ngrok stop/rebind occurs when changing the primary provider.
            live.ngrok
                .reconcile(true, &super::zrok_target(settings))
                .await;
            drop(live);
            self.prune_stale_public_devices().await;
        }
        let message = if apply {
            if https {
                "主链路已应用，请使用新的公网地址重新登录。"
            } else {
                "frp HTTP 主链路已应用，请使用固定公网地址登录；公网传输不加密。"
            }
        } else {
            if https {
                "公网 HTTPS、回源身份及未登录访问边界验证通过，当前主入口未改变。"
            } else {
                "frp HTTP 入口、回源身份及未登录访问边界验证通过，可应用为主链路；公网传输不加密。"
            }
        };
        Ok(json!({"status":"ok","httpsReady":https,"publicUrl":origin,"message":message}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Extension;

    struct Bridge;
    #[async_trait::async_trait]
    impl super::super::NativeRemoteBridge for Bridge {
        async fn call(&self, _: &str, _: Value) -> anyhow::Result<Value> {
            Ok(json!({"ok":true}))
        }
    }

    async fn server(
        controller: Arc<RemoteAccessController>,
        ingress: Option<&str>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut router = super::super::web_router(
            controller,
            RemoteAccessSettings {
                enabled: true,
                port: address.port(),
                ..Default::default()
            },
        );
        if let Some(id) = ingress {
            router = router.layer(Extension(frp_tunnel::Ingress(id.into())));
        }
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn frp_candidate_only_serves_probes_and_leaves_zrok_online() {
        let directory = tempfile::tempdir().unwrap();
        let controller =
            RemoteAccessController::new(Arc::new(Bridge), directory.path().into()).unwrap();
        *controller.public_origin.write().unwrap() = Some("https://existing.shares.zrok.io".into());
        *controller.primary.probe.write().unwrap() = Some(Probe {
            origin: "http://trial.example.com:39400".into(),
            nonce: "unique-probe".into(),
            ingress: Some("trial".into()),
            expires: now_ms() + 60_000,
        });
        let (url, task) = server(controller.clone(), Some("trial")).await;
        let (old, old_task) = server(controller.clone(), None).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let probe = client
            .get(format!("{url}/api/entry-check?challenge=unique-probe"))
            .header("Host", "trial.example.com:39400")
            .send()
            .await
            .unwrap();
        assert_eq!(probe.status(), 200);
        assert_eq!(
            probe.json::<Value>().await.unwrap()["challenge"],
            "unique-probe"
        );
        for path in [
            "/",
            "/api/login-status",
            "/api/channels",
            "/api/entry-check?challenge=wrong",
        ] {
            assert_eq!(
                client
                    .get(format!("{url}{path}"))
                    .header("Host", "trial.example.com:39400")
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403,
                "{path}"
            );
        }
        assert_eq!(
            client
                .get(format!("{url}/api/session"))
                .header("Host", "trial.example.com:39400")
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            client
                .get(format!("{url}/api/session"))
                .header("Host", "trial.example.com:39400")
                .bearer_auth("test-ticket")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(&old)
                .header("Host", "existing.shares.zrok.io")
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(controller.primary.provider(), Provider::Zrok);
        *controller.primary.probe.write().unwrap() = None;
        assert_eq!(
            client
                .get(format!("{url}/api/entry-check?challenge=unique-probe"))
                .header("Host", "trial.example.com:39400")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        task.abort();
        old_task.abort();
    }

    #[tokio::test]
    async fn frp_active_host_requires_its_verified_ingress_and_keeps_ngrok_cors() {
        let directory = tempfile::tempdir().unwrap();
        let controller =
            RemoteAccessController::new(Arc::new(Bridge), directory.path().into()).unwrap();
        *controller.primary.selection.write().unwrap() = Selection {
            provider: Provider::Frp,
            origin: Some("https://entry.example.com:39399".into()),
            ingress: Some("verified".into()),
        };
        *controller.channels.secondary.write().unwrap() =
            Some("https://second.ngrok-free.app".into());
        let (plain, plain_task) = server(controller.clone(), None).await;
        let (verified, verified_task) = server(controller.clone(), Some("verified")).await;
        let (stale, stale_task) = server(controller.clone(), Some("old-ingress")).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for url in [&plain, &stale] {
            assert_eq!(
                client
                    .get(url.as_str())
                    .header("Host", "entry.example.com:39399")
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
            assert_eq!(
                client
                    .post(format!("{url}/api/connect"))
                    .header("Host", "entry.example.com:39399")
                    .header("Origin", "https://entry.example.com:39399")
                    .json(&json!({"key":"not-a-real-key"}))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
        }
        let root = client
            .get(&verified)
            .header("Host", "entry.example.com:39399")
            .send()
            .await
            .unwrap();
        assert_eq!(root.status(), 200);
        assert!(root.headers().contains_key("strict-transport-security"));
        let preflight = client
            .request(
                reqwest::Method::OPTIONS,
                format!("{plain}/api/read/history"),
            )
            .header("Host", "second.ngrok-free.app")
            .header("Origin", "https://entry.example.com:39399")
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization,content-type",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(preflight.status(), 204);
        assert_eq!(
            preflight.headers()["access-control-allow-origin"],
            "https://entry.example.com:39399"
        );
        assert_eq!(
            client
                .get(&verified)
                .header("Host", "unregistered.example.com")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        plain_task.abort();
        verified_task.abort();
        stale_task.abort();
    }

    #[test]
    fn frp_selection_defaults_to_zrok_and_restores_http_or_https_entry() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            Primary::new(directory.path()).unwrap().provider(),
            Provider::Zrok
        );
        let saved = frp_tunnel::Saved {
            config: frp_tunnel::Config {
                server_addr: "relay.example.com".into(),
                server_port: 7000,
                remote_port: 39400,
                public_origin: "http://entry.example.com:39400".into(),
                ..Default::default()
            },
            token: "test-only".into(),
            ..Default::default()
        };
        saved.store(directory.path(), true).unwrap();
        crate::settings::atomic_write(
            &directory.path().join("remote-access-primary.json"),
            b"\"frp\"",
        )
        .unwrap();
        let http = Primary::new(directory.path()).unwrap();
        assert_eq!(
            http.frp_origin().as_deref(),
            Some("http://entry.example.com:39400")
        );
        assert!(!http.permits_frp(Some("unverified")));
        let mut saved = saved;
        saved.config.public_origin = "https://entry.example.com:39400".into();
        saved.store(directory.path(), false).unwrap();
        let primary = Primary {
            selection: RwLock::new(Selection::default()),
            probe: RwLock::new(None),
            operation: Mutex::new(()),
            busy: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            frp_download: frp_tunnel::DownloadProgress::default(),
            last_error: RwLock::new(None),
        };
        primary
            .save_stopped(directory.path(), Provider::Frp)
            .unwrap();
        let restarted = Primary::new(directory.path()).unwrap();
        assert_eq!(restarted.provider(), Provider::Frp);
        assert_eq!(
            restarted.frp_origin().as_deref(),
            Some("https://entry.example.com:39400")
        );
        assert!(!restarted.permits_frp(Some("unverified")));
        saved.config.public_origin = "https://changed.example.com:39400".into();
        saved.store(directory.path(), false).unwrap();
        restarted
            .save_stopped(directory.path(), Provider::Frp)
            .unwrap();
        assert_eq!(
            frp_tunnel::Saved::load(directory.path(), true)
                .unwrap()
                .config
                .public_origin,
            "https://entry.example.com:39400"
        );
        let operation = restarted.try_operation().unwrap();
        assert!(restarted.try_operation().is_err());
        drop(operation);
        assert!(restarted.try_operation().is_ok());
        restarted
            .save_stopped(directory.path(), Provider::Zrok)
            .unwrap();
        assert_eq!(
            Primary::new(directory.path()).unwrap().provider(),
            Provider::Zrok
        );
    }

    #[tokio::test]
    async fn frp_http_login_media_and_logout_keep_authorization_and_ngrok_isolation() {
        let directory = tempfile::tempdir().unwrap();
        super::super::configure_access_key(directory.path(), "test-http-access-key").unwrap();
        let controller =
            RemoteAccessController::new(Arc::new(Bridge), directory.path().into()).unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..Default::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        let origin = "http://entry.example.com:39399";
        *controller.primary.selection.write().unwrap() = Selection {
            provider: Provider::Frp,
            origin: Some(origin.into()),
            ingress: Some("verified-http".into()),
        };
        *controller.channels.secondary.write().unwrap() =
            Some("https://second.ngrok-free.app".into());
        let (url, task) = server(controller.clone(), Some("verified-http")).await;
        let (plain, plain_task) = server(controller.clone(), None).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let root = client
            .get(&url)
            .header("Host", "entry.example.com:39399")
            .send()
            .await
            .unwrap();
        assert_eq!(root.status(), 200);
        assert!(!root.headers().contains_key("strict-transport-security"));
        assert_eq!(
            client
                .get(format!("{url}/api/session"))
                .header("Host", "entry.example.com:39399")
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let login = client
            .post(format!("{url}/api/connect"))
            .header("Host", "entry.example.com:39399")
            .header("Origin", origin)
            .json(&json!({"key":"test-http-access-key"}))
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), 200);
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(!cookie.contains("; Secure"));
        assert_eq!(
            controller.auth.lock().await.devices[0]
                .public_origin
                .as_deref(),
            Some(origin)
        );
        let discovery = client
            .get(format!("{url}/api/channels"))
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie.split(';').next().unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(discovery.status(), 200);
        let discovery: Value = discovery.json().await.unwrap();
        assert_eq!(
            discovery["origins"],
            json!([origin, "https://second.ngrok-free.app"])
        );
        let cookie_header = cookie.split(';').next().unwrap();
        let image_path = directory.path().join("http-preview.png");
        image::RgbaImage::from_pixel(64, 48, image::Rgba([40, 120, 200, 255]))
            .save(&image_path)
            .unwrap();
        let image_id = controller.images.write().unwrap().register(image_path);
        let mut image_etag = None;
        for suffix in ["", "/thumbnail"] {
            let media_url = format!("{url}/api/images/{image_id}{suffix}");
            let image = client
                .get(&media_url)
                .header("Host", "entry.example.com:39399")
                .header("Cookie", cookie_header)
                .send()
                .await
                .unwrap();
            assert_eq!(image.status(), 200);
            assert!(
                image.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("image/")
            );
            let etag = image.headers()[header::ETAG].to_str().unwrap().to_owned();
            let cached = client
                .get(&media_url)
                .header("Host", "entry.example.com:39399")
                .header("Cookie", cookie_header)
                .header(header::IF_NONE_MATCH, &etag)
                .send()
                .await
                .unwrap();
            assert_eq!(cached.status(), 304);
            image_etag = Some(etag);
        }
        let video_path = directory.path().join("http-video.mp4");
        let video_bytes: Vec<u8> = (0..2048).map(|value| (value % 251) as u8).collect();
        std::fs::write(&video_path, &video_bytes).unwrap();
        let mut video_item =
            json!({"type":"userMessage","content":[{"type":"localVideo","path":video_path}]});
        super::super::video::attach_item(&controller, &mut video_item);
        let video_path = video_item["remoteVideos"][0]["url"].as_str().unwrap();
        let video_url = format!("{url}{video_path}");
        let video = client
            .get(&video_url)
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie_header)
            .header(header::RANGE, "bytes=100-199")
            .send()
            .await
            .unwrap();
        assert_eq!(video.status(), 206);
        assert_eq!(video.headers()[header::CONTENT_RANGE], "bytes 100-199/2048");
        assert_eq!(
            video.bytes().await.unwrap().as_ref(),
            &video_bytes[100..200]
        );
        let head = client
            .head(&video_url)
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie_header)
            .send()
            .await
            .unwrap();
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "2048");
        assert!(head.bytes().await.unwrap().is_empty());
        let invalid_range = client
            .get(&video_url)
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie_header)
            .header(header::RANGE, "bytes=9999-")
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_range.status(), 416);
        let poster = client
            .get(format!("{video_url}/poster"))
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie_header)
            .send()
            .await
            .unwrap();
        assert_eq!(poster.status(), 200);
        let preflight = client
            .request(
                reqwest::Method::OPTIONS,
                format!("{plain}/api/read/history"),
            )
            .header("Host", "second.ngrok-free.app")
            .header("Origin", origin)
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization,content-type",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(preflight.status(), 204);
        let foreign = client
            .post(format!("{url}/api/connect"))
            .header("Host", "entry.example.com:39399")
            .header("Origin", "http://other.example.com")
            .json(&json!({"key":"test-http-access-key"}))
            .send()
            .await
            .unwrap();
        assert_eq!(foreign.status(), 403);
        let bypass = client
            .get(&plain)
            .header("Host", "entry.example.com:39399")
            .send()
            .await
            .unwrap();
        assert_eq!(bypass.status(), 403);
        let logout = client
            .post(format!("{url}/api/logout"))
            .header("Host", "entry.example.com:39399")
            .header("Origin", origin)
            .header("Cookie", cookie_header)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(logout.status(), 200);
        assert!(
            !logout.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("; Secure")
        );
        let after_logout = client
            .get(format!("{url}/api/images/{image_id}/thumbnail"))
            .header("Host", "entry.example.com:39399")
            .header("Cookie", cookie_header)
            .header(header::IF_NONE_MATCH, image_etag.unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(after_logout.status(), 401);
        assert_eq!(
            client
                .get(video_url)
                .header("Host", "entry.example.com:39399")
                .header("Cookie", cookie_header)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        task.abort();
        plain_task.abort();
    }
}
