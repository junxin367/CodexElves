//! 远程会话服务：统一维护执行引擎、会话归属、队列、编辑事务、审批和事件。
//! 用户要求 Codex 仅保留服务模式；本模块不包含页面脚本或桌面注入回退。
mod catalog;
mod commands;
mod defaults;
mod desktop;
mod fast;
mod pins;
mod projection;
mod projects;
mod queue_gate;
mod read_cache;
mod reads;
mod runtime;
mod sides;
mod store;
mod subagents;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};

use super::execution;
use super::{ApiError, RemoteAccessController, RemoteAuthorization};
use crate::codex_service::ServiceDispatchGuard;
use catalog::Catalog;
use projection::Projection;
use runtime::Runtime;
use store::{Queued, Store};

pub(super) struct Harness {
    pub audit: Arc<execution::Audit>,
    runtime: Arc<Runtime>,
    catalog: Arc<Catalog>,
    projection: Arc<Mutex<Projection>>,
    store: Mutex<Store>,
    #[cfg(test)]
    project_codex_home: std::path::PathBuf,
    controller: OnceLock<Weak<RemoteAccessController>>,
    receiver: Mutex<Option<mpsc::Receiver<Value>>>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    queue_worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    desktop: desktop::DesktopObservers,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    queue_recovery_after: Mutex<HashMap<String, std::time::Instant>>,
    loaded: Mutex<HashMap<String, String>>,
    pin_capability: Mutex<Option<(String, bool)>>,
    pub enabled: AtomicBool,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.get_mut().take() {
            worker.abort();
        }
        if let Some(worker) = self.queue_worker.get_mut().take() {
            worker.abort();
        }
    }
}

impl Harness {
    #[cfg(test)]
    pub async fn install_test_process(&self, process: crate::codex_service::RunningService) {
        self.runtime.install_test_process(process).await;
    }
    #[cfg(test)]
    pub async fn seed_test_history(&self, payload: Value) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.runtime.has_test_process(),
            "只允许向隔离测试引擎写入历史样例"
        );
        self.runtime
            .request("test/seedHistory", payload, None)
            .await?;
        Ok(())
    }
    #[cfg(test)]
    pub async fn seed_test_subagents(&self, payload: Value) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.runtime.has_test_process(),
            "只允许配置隔离子智能体样例"
        );
        self.runtime
            .request("test/seedSubagents", payload, None)
            .await?;
        self.runtime.invalidate_read("thread/list").await;
        self.runtime.invalidate_read("thread/loaded/list").await;
        Ok(())
    }
    pub fn new(directory: &Path) -> anyhow::Result<Arc<Self>> {
        let (events, receiver) = mpsc::channel(512);
        let audit = Arc::new(execution::Audit::default());
        let projection = Arc::new(Mutex::new(Projection::default()));
        Ok(Arc::new(Self {
            audit: audit.clone(),
            runtime: Arc::new(Runtime::new(events, audit.clone())),
            catalog: Arc::new(Catalog::default()),
            projection: projection.clone(),
            store: Mutex::new(Store::open(directory)?),
            #[cfg(test)]
            project_codex_home: directory.to_path_buf(),
            controller: OnceLock::new(),
            receiver: Mutex::new(Some(receiver)),
            worker: Mutex::new(None),
            queue_worker: Mutex::new(None),
            desktop: desktop::DesktopObservers::new(projection, audit),
            locks: Mutex::new(HashMap::new()),
            queue_recovery_after: Mutex::new(HashMap::new()),
            loaded: Mutex::new(HashMap::new()),
            pin_capability: Mutex::new(None),
            enabled: AtomicBool::new(false),
        }))
    }

    pub fn attach(&self, controller: Weak<RemoteAccessController>) {
        let _ = self.controller.set(controller);
    }

    pub async fn configured_permissions(&self) -> anyhow::Result<Value> {
        // 只读实例的生效配置，不创建或恢复会话；配置保存后不能复用模型目录的 30 秒缓存。
        let response =
            execution::Scope::service(self.audit.clone(), "permission-config", &json!({}), None)
                .run(self.runtime.request(
                    "config/read",
                    json!({"cwd":null,"includeLayers":false}),
                    None,
                ))
                .await?;
        Ok(super::permissions::codex_config_preset(&response)?)
    }

    async fn start(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut worker = self.worker.lock().await;
        if worker.is_none() {
            let mut receiver = self
                .receiver
                .lock()
                .await
                .take()
                .context("会话事件接收器已关闭")?;
            let weak = Arc::downgrade(self);
            *worker = Some(execution::spawn_service(
                self.audit.clone(),
                "engine-events",
                async move {
                    while let Some(frame) = receiver.recv().await {
                        let Some(service) = weak.upgrade() else { break };
                        // 执行权随引擎加载生命周期变化；已释放的旧标记不能永久屏蔽桌面观察。
                        let generation = frame["_generation"].as_str().unwrap_or("");
                        if frame["method"] == "thread/closed"
                            || frame["method"] == "thread/status/changed"
                                && frame["params"]["status"]["type"] == "notLoaded"
                        {
                            if let Some(id) = frame["params"]["threadId"].as_str() {
                                let mut loaded = service.loaded.lock().await;
                                if loaded.get(id).is_some_and(|value| value == generation) {
                                    loaded.remove(id);
                                }
                            }
                        }
                        let paused = match service.observe_queue_lifecycle(&frame).await {
                            Ok(paused) => paused,
                            Err(failure) => {
                                let _ = crate::diagnostic_log::append_diagnostic_log(
                                    "remote_access.queue_completion_check_failed",
                                    json!({"error":format!("{failure:#}")}),
                                );
                                false
                            }
                        };
                        let affected = frame["params"]["threadId"].as_str().map(str::to_owned);
                        service.catalog.observe(&frame).await;
                        service.projection.lock().await.ingest(frame);
                        if paused && let Some(id) = affected {
                            service.queue_changed(&id).await;
                        }
                    }
                },
            ));
            let weak = Arc::downgrade(self);
            *self.queue_worker.lock().await = Some(execution::spawn_service(
                self.audit.clone(),
                "queue-worker",
                async move {
                    let mut tick = tokio::time::interval(Duration::from_millis(750));
                    loop {
                        tick.tick().await;
                        let Some(service) = weak.upgrade() else { break };
                        // 派发与接收独立运行，长请求不会堵住流式输出或审批事件。
                        if service.enabled.load(Ordering::Acquire) {
                            service.drain_queues().await;
                        }
                    }
                },
            ));
        }
        drop(worker);
        let connection = self.runtime.connection().await?;
        self.projection
            .lock()
            .await
            .ingest(json!({"method":"harness/connected",
            "_generation":connection.generation,"_generationIndex":connection.generation_index}));
        Ok(())
    }

    #[cfg(test)]
    pub async fn prepare(self: &Arc<Self>) -> anyhow::Result<()> {
        execution::Scope::service(self.audit.clone(), "prepare", &json!({}), None)
            .run(self.start())
            .await
    }

    pub async fn prewarm(self: &Arc<Self>) -> anyhow::Result<()> {
        // 用户要求 Codex 仅使用服务模式；预热只读目录，不改变会话归属或启动任务。
        execution::Scope::service(self.audit.clone(), "prewarm", &json!({}), None)
            .run(async {
                self.start().await?;
                let model_params = json!({});
                let (sidebar, models) = tokio::join!(self.sidebar(), self.models(&model_params));
                sidebar?;
                models?;
                Ok(())
            })
            .await
    }

    pub async fn prewarm_for_listener(self: &Arc<Self>) -> anyhow::Result<()> {
        // 普通监听器单测不能因自动预热访问本机真实会话；启动预热测试显式注入隔离引擎。
        #[cfg(test)]
        if !self.runtime.has_test_process() {
            return Ok(());
        }
        self.prewarm().await
    }

    pub async fn stop_preparation(&self) {
        // 停止远程服务时取消目录预热，释放仅用于预热的引擎；
        // 已启用的服务任务保持原生命周期，不能因预热清理中断已有工作。
        if !self.enabled.load(Ordering::Acquire) && self.idle().await {
            self.release().await;
        } else {
            self.catalog.reset().await;
        }
    }

    pub async fn idle(&self) -> bool {
        let queues_empty = self
            .store
            .lock()
            .await
            .data
            .queues
            .values()
            .all(Vec::is_empty);
        let projection = self.projection.lock().await;
        queues_empty
            && projection.active.is_empty()
            && projection.sides.threads.values().all(|side| side.closed)
    }

    pub async fn release(&self) {
        self.catalog.reset().await;
        self.desktop.reset().await;
        self.runtime.shutdown().await;
        self.loaded.lock().await.clear();
    }

    async fn thread_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .await
            .entry(id.to_owned())
            .or_default()
            .clone()
    }

    async fn ensure_loaded(
        &self,
        id: &str,
        guard: &dyn ServiceDispatchGuard,
    ) -> anyhow::Result<()> {
        let connection = self.runtime.connection().await?;
        if self.loaded.lock().await.get(id) == Some(&connection.generation) {
            return Ok(());
        }
        // 仅写操作才 resume；读取不抢占桌面会话，底层 writer lock 决定最终归属。
        let response = self
            .runtime
            .request(
                "thread/resume",
                json!({"threadId":id,"excludeTurns":true}),
                Some(guard),
            )
            .await?;
        let mut thread = crate::codex_service::normalize_thread_response(&response)?;
        for key in ["model", "reasoningEffort", "serviceTier"] {
            if let Some(value) = response.get(key) {
                thread[key] = value.clone();
            }
        }
        self.remember_settings(id, &response).await?;
        // 与列表观察的登记共用归属锁：取消旧订阅到登记服务执行权之间不能插入新桌面订阅。
        let mut loaded = self.loaded.lock().await;
        self.desktop.forget(id).await;
        self.projection
            .lock()
            .await
            .threads
            .insert(id.to_owned(), thread);
        loaded.insert(id.to_owned(), connection.generation.clone());
        Ok(())
    }

    async fn remember_settings(&self, id: &str, settings: &Value) -> anyhow::Result<()> {
        let saved = self.store.lock().await.commit(|data| {
            let current = data
                .settings
                .entry(id.to_owned())
                .or_insert_with(|| json!({}));
            for key in ["model", "reasoningEffort", "serviceTier", "fastMode"] {
                if let Some(value) = settings.get(key) {
                    current[key] = value.clone();
                }
            }
            Ok(current.clone())
        })?;
        let mut projection = self.projection.lock().await;
        let thread = projection
            .threads
            .entry(id.to_owned())
            .or_insert_with(|| json!({}));
        for (key, value) in saved.as_object().into_iter().flatten() {
            thread[key] = value.clone();
        }
        Ok(())
    }

    async fn locally_loaded(&self, id: &str) -> anyhow::Result<bool> {
        let generation = self.runtime.connection().await?.generation.clone();
        Ok(self.loaded.lock().await.get(id) == Some(&generation))
    }

    async fn owner_snapshot(&self, id: &str) -> anyhow::Result<Option<Value>> {
        self.owner_snapshot_kind(id, false).await
    }

    async fn owner_view_snapshot(&self, id: &str) -> anyhow::Result<Option<Value>> {
        self.owner_snapshot_kind(id, true).await
    }

    async fn owner_snapshot_kind(
        &self,
        id: &str,
        allow_subagents: bool,
    ) -> anyhow::Result<Option<Value>> {
        if self.locally_loaded(id).await? {
            return Ok(None);
        }
        let snapshot = if allow_subagents {
            crate::desktop_owner_ipc::read_view_snapshot(id).await
        } else {
            crate::desktop_owner_ipc::read_snapshot(id).await
        }
        .map_err(|error| {
            if error.downcast_ref::<ApiError>().is_some() {
                error
            } else {
                super::ApiError::new(
                    503,
                    "当前会话的桌面连接暂时不可用，请确认桌面应用可响应后重试。",
                )
                .into()
            }
        })?;
        if snapshot.is_some() {
            execution::transport("desktop-ipc", "thread-snapshot", false);
        }
        Ok(snapshot)
    }

    async fn owner_request(
        &self,
        id: &str,
        method: &str,
        params: Value,
        version: u64,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Option<Value>> {
        if self.locally_loaded(id).await? {
            return Ok(None);
        }
        crate::desktop_owner_ipc::request_owner(id, method, params, version, guard)
            .await
            .map_err(|error| {
                if error.downcast_ref::<ApiError>().is_some() {
                    return error;
                }
                // 用户要求 IPC 故障不能冒充无持有者；未发送和已发送待核对必须分别保留，均不回退到其他执行通道。
                let dispatched = error
                    .downcast_ref::<crate::desktop_owner_ipc::DispatchFailure>()
                    .is_some_and(|failure| failure.dispatched);
                ApiError {
                    code: 503,
                    message: if dispatched {
                        "当前会话的桌面连接中断，操作结果待核对，请刷新会话确认，勿重复提交。"
                    } else {
                        "当前会话的桌面连接暂时不可用，操作未发送，请确认桌面应用可响应后重试。"
                    }
                    .into(),
                    outcome_unknown: dispatched,
                }
                .into()
            })
    }

    pub async fn call(
        self: &Arc<Self>,
        action: &str,
        payload: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        execution::Scope::service(self.audit.clone(), action, &payload, authorization)
            .run(self.call_inner(action, payload, authorization))
            .await
    }

    async fn call_inner(
        self: &Arc<Self>,
        action: &str,
        payload: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        if let Some(authorization) = authorization {
            authorization.check().await?;
        }
        // 用户要求过期后仍能查看和确认已读；已保留的侧边只处理本地内存，不等待失效引擎重连。
        let retained_side = if matches!(
            action,
            "thread" | "history" | "markRead" | "closeSideThread"
        ) {
            let projection = self.projection.lock().await;
            payload["threadId"]
                .as_str()
                .and_then(|id| projection.sides.threads.get(id))
                .is_some_and(|side| side.expired || side.closed)
        } else {
            false
        };
        if !retained_side {
            self.start().await?;
        }
        let result = match action {
            "events" => {
                self.runtime.check_health().await?;
                // 服务模式有可见页面时沿用 3 秒目录缓存补齐桌面新会话；后台发现不阻塞事件推送。
                if self.desktop.catalog_refresh_due() {
                    self.catalog
                        .refresh_in_background(&self.runtime, &self.projection);
                }
                Ok(self.projection.lock().await.events(&payload))
            }
            "observeThreads" => {
                self.desktop.renew_catalog_interest();
                let generation = self.runtime.connection().await?.generation.clone();
                let loaded = self.loaded.lock().await;
                for id in payload["threadIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if loaded.get(id) != Some(&generation) {
                        self.desktop.observe(id, None).await;
                    }
                }
                Ok(json!({"ok":true}))
            }
            "notifications" => Ok(self.projection.lock().await.notifications(&payload)),
            "sidebar" => self.sidebar().await,
            "models" => self.models(&payload).await,
            "newSessionDefaults" => self
                .creation_defaults(&payload)
                .await
                .map(|(defaults, _)| defaults),
            "fast" => self.fast_state(&payload).await,
            "skills" => self.skills(&payload).await,
            "thread" => self.thread(&payload).await,
            "history" => self.history(&payload).await,
            "workspaceContext" => self.workspace_context(&payload).await,
            "queue" => Ok(self.queue(required(&payload, "threadId")?).await),
            "queueEdit" => self.queue_edit(&payload).await,
            "sendState" => self.send_state(&payload).await,
            "relatedThreads" => self.related_sides(&payload).await,
            _ => {
                let authorization = authorization.context("会话写操作缺少设备授权")?;
                let id = payload["threadId"].as_str().unwrap_or("$workspace");
                let lock = self.thread_lock(id).await;
                let _serial = lock.lock().await;
                authorization.check().await?;
                self.command(action, payload, authorization).await
            }
        }?;
        if let Some(authorization) = authorization {
            authorization.check().await?;
        }
        Ok(result)
    }

    async fn queue(&self, id: &str) -> Value {
        let projection = self.projection.lock().await;
        if let Some(side) = projection.sides.threads.get(id) {
            return side.queue_snapshot();
        }
        let epoch = projection.epoch.clone();
        drop(projection);
        self.store.lock().await.queue(id, &epoch)
    }

    async fn queue_changed(&self, id: &str) {
        let snapshot = self.queue(id).await;
        if snapshot["queuedMessages"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            self.queue_recovery_after.lock().await.remove(id);
        }
        self.projection
            .lock()
            .await
            .push(json!({"method":"thread/queue/changed","params":snapshot}));
    }

    async fn drain_queues(self: &Arc<Self>) {
        // 用户要求取回编辑后等同未发送草稿，不阻塞后续队列；原槽位仅供再次发送时恢复顺序。
        // 这里只跳过 editing，暂停、发送中或结果未知的消息仍保持原有等待边界。
        let mut candidates: Vec<_> = self
            .store
            .lock()
            .await
            .data
            .queues
            .iter()
            .filter(|(_, q)| {
                q.iter()
                    .find(|m| m.state != "editing")
                    .is_some_and(|m| m.state == "queued")
            })
            .map(|(id, _)| id.clone())
            .collect();
        candidates.extend(
            self.projection
                .lock()
                .await
                .sides
                .threads
                .iter()
                .filter(|(id, side)| {
                    !side.closed
                        && !side.expired
                        && !side.closing
                        && side
                            .queue
                            .queue_gates
                            .get(*id)
                            .is_some_and(|gate| gate.phase == "ready")
                        && side.queue.queues.get(*id).is_some_and(|queue| {
                            queue
                                .iter()
                                .find(|message| message.state != "editing")
                                .is_some_and(|message| message.state == "queued")
                        })
                })
                .map(|(id, _)| id.clone()),
        );
        for id in candidates {
            let lock = self.thread_lock(&id).await;
            let Ok(_serial) = lock.try_lock() else {
                continue;
            };
            if self.projection.lock().await.active.contains_key(&id) {
                continue;
            }
            if self.projection.lock().await.compactions.contains_key(&id) {
                continue;
            }
            let side_message = {
                let projection = self.projection.lock().await;
                projection.sides.threads.get(&id).and_then(|side| {
                    side.queue
                        .queues
                        .get(&id)
                        .and_then(|queue| queue.iter().find(|message| message.state != "editing"))
                        .cloned()
                })
            };
            let message = if side_message.is_some() {
                side_message
            } else {
                self.store
                    .lock()
                    .await
                    .data
                    .queues
                    .get(&id)
                    .and_then(|q| q.iter().find(|m| m.state != "editing"))
                    .cloned()
            };
            let Some(message) = message.filter(|q| q.state == "queued") else {
                continue;
            };
            let result = self.dispatch_queued(&id, &message).await;
            if let Err(error) = result {
                let mut projection = self.projection.lock().await;
                if let Some(side) = projection.sides.threads.get_mut(&id)
                    && side
                        .queue
                        .pause_queue(&id, "侧边队列发送失败，消息已暂停；请核对后手动重试")
                {
                    side.queue_revision += 1;
                    projection.side_changed(&id);
                }
                drop(projection);
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "remote_access.harness_queue_paused",
                    json!({"threadId":id,"messageId":message.id,"error":format!("{error:#}")}),
                );
            }
        }
    }

    async fn dispatch_queued(&self, id: &str, message: &Queued) -> anyhow::Result<()> {
        let controller = self
            .controller
            .get()
            .and_then(Weak::upgrade)
            .context("远程会话服务已停止")?;
        let device = controller
            .auth
            .lock()
            .await
            .devices
            .iter()
            .find(|d| d.id == message.device_id && d.key_id == message.key_id)
            .cloned()
            .context("队列消息的设备授权已撤销")?;
        let mut authorization = controller.authorization(device, Some("send"));
        authorization.request_id = message.request_id.clone();
        execution::Scope::new(
            self.audit.clone(),
            "queue-dispatch",
            &json!({"threadId":id}),
            Some(&authorization),
        )
        .recorded(async {
            self.dispatch_queued_inner(id, message, &authorization)
                .await?;
            Ok(json!({"ok":true}))
        })
        .await
        .map(|_| ())
    }

    async fn dispatch_queued_inner(
        &self,
        id: &str,
        message: &Queued,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<()> {
        authorization.check().await?;
        if self.known_side(id).await {
            return self
                .dispatch_side_queued(id, message, authorization, false)
                .await;
        }
        // 自动续发必须核对真实执行状态；重连后内存中没有 active 不代表上一轮正常完成。
        let identity = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":id,"includeTurns":false}),
                None,
            )
            .await?;
        crate::codex_service::normalize_thread_response(&identity)?;
        if self.send_is_running(id, &identity["thread"]).await? {
            let (turn, compacting) = self.queue_anchor(id).await?;
            self.remember_queue_wait(id, turn, compacting).await?;
            return Ok(());
        }
        if let Some(snapshot) = self.owner_snapshot(id).await? {
            // 桌面已明确执行中时，历史尚未载入当前轮次也必须等待，不能提前派发队列。
            let active = desktop::executing(&snapshot)
                || crate::desktop_owner_ipc::snapshot_turns(&snapshot)
                    .iter()
                    .any(|t| t["status"] == "inProgress");
            if active {
                let turn = crate::desktop_owner_ipc::snapshot_turns(&snapshot)
                    .iter()
                    .rev()
                    .find(|turn| turn["status"] == "inProgress")
                    .and_then(|turn| turn["turnId"].as_str())
                    .map(str::to_owned);
                self.remember_queue_wait(id, turn, false).await?;
                self.follow_owner(id, snapshot).await;
                return Ok(());
            }
            // 桌面已有待发内容时继续等待其持有者处理，不能让服务队列抢先发送。
            let settings = crate::settings::SettingsStore::default().load()?;
            let home = crate::codex_home::codex_home_dir_for_settings(&settings);
            if !crate::codex_service::local_queue_is_empty(&home, id, &snapshot).await? {
                return Ok(());
            }
            let native_queue = self
                .runtime
                .request(
                    "thread/queue/list",
                    json!({"threadId":id,"cursor":null,"limit":1}),
                    None,
                )
                .await?;
            let queued = native_queue["data"]
                .as_array()
                .context("桌面服务端队列状态未知")?;
            if !queued.is_empty() || !native_queue["nextCursor"].is_null() {
                return Ok(());
            }
        }
        if !self
            .queue_completion_confirmed(id, &identity, message.created_at)
            .await?
        {
            return Ok(());
        }
        {
            let projection = self.projection.lock().await;
            if projection.active.contains_key(id) || projection.compactions.contains_key(id) {
                return Ok(());
            }
        }
        self.store.lock().await.commit(|data| {
            anyhow::ensure!(
                data.queue_gates
                    .get(id)
                    .is_some_and(|gate| gate.phase == "ready"),
                "上一轮完成状态已变化，队列继续等待"
            );
            let queued = data
                .queues
                .get_mut(id)
                .and_then(|q| {
                    q.iter_mut()
                        .find(|q| q.id == message.id && q.state == "queued")
                })
                .context("队列消息已变化")?;
            queued.state = "sending".into();
            Ok(())
        })?;
        let result = self.start_turn(id, &message.payload, authorization).await;
        self.store.lock().await.commit(|data| {
            match &result {
                Ok(response) => {
                    data.sent
                        .insert(sent_key(id, &message.payload), response.clone());
                    data.sent_hashes.insert(
                        sent_key(id, &message.payload),
                        commands::payload_hash(&message.payload),
                    );
                    if let Some(queue) = data.queues.get_mut(id) {
                        queue.retain(|q| q.id != message.id);
                    }
                }
                Err(error) => {
                    if let Some(queued) = data
                        .queues
                        .get_mut(id)
                        .and_then(|q| q.iter_mut().find(|q| q.id == message.id))
                    {
                        queued.state = if error
                            .downcast_ref::<ApiError>()
                            .is_some_and(|error| !error.outcome_unknown)
                        {
                            "paused"
                        } else {
                            "unknown"
                        }
                        .into();
                    }
                }
            }
            Ok(())
        })?;
        self.queue_changed(id).await;
        result.map(|_| ())
    }

    async fn follow_owner(&self, id: &str, baseline: Value) {
        let Ok(connection) = self.runtime.connection().await else {
            return;
        };
        let loaded = self.loaded.lock().await;
        if loaded.get(id) != Some(&connection.generation) {
            self.desktop.observe(id, Some(baseline)).await;
        }
    }
}

fn error(code: u16, message: &str) -> anyhow::Error {
    ApiError::new(code, message).into()
}
fn required<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value[key]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| error(400, &format!("缺少 {key}")))
}
fn sent_key(id: &str, payload: &Value) -> String {
    format!(
        "{id}:{}",
        payload["clientUserMessageId"].as_str().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "只读验证已安装引擎，需显式指定会话和轮次，并隔离桌面 IPC"]
    async fn installed_engine_history_reentry_smoke() {
        assert!(
            std::env::var("CODEX_ELVES_TEST_IPC_PIPE")
                .unwrap_or_default()
                .starts_with(r"\\.\pipe\codex-elves-isolation-")
        );
        let id = std::env::var("CODEX_ELVES_HISTORY_CHECK_THREAD").expect("缺少待核对会话");
        let turn_id = std::env::var("CODEX_ELVES_HISTORY_CHECK_TURN").expect("缺少待核对轮次");
        let expected = std::env::var("CODEX_ELVES_HISTORY_CHECK_TEXT").expect("缺少待核对用户消息");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&root).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("history-reentry-check-")
            .tempdir_in(root)
            .unwrap();
        let service = Harness::new(directory.path()).unwrap();
        let mut cursor = Value::Null;
        let mut seen = std::collections::HashSet::new();
        let turn = loop {
            let history = service
                .call("history", json!({"threadId":id,"cursor":cursor}), None)
                .await
                .unwrap();
            if let Some(turn) = history["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|turn| turn["id"] == turn_id)
            {
                break turn.clone();
            }
            cursor = history["nextCursor"].clone();
            assert!(
                cursor.is_string() && seen.insert(cursor.to_string()),
                "未找到指定轮次"
            );
        };
        let first = turn["items"].as_array().unwrap();
        assert!(first.iter().any(|item| {
            item["type"] == "userMessage"
                && item["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["text"] == expected))
        }));
        let mut ids: std::collections::HashSet<_> = first
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect();
        let mut cursor = turn["itemsNextCursor"].clone();
        let mut pages = 1;
        while !cursor.is_null() {
            let history = service
                .call(
                    "history",
                    json!({"threadId":id,"turnId":turn_id,
                "itemCursor":cursor,"turnStatus":turn["status"]}),
                    None,
                )
                .await
                .unwrap();
            let before = ids.len();
            ids.extend(
                history["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item["id"].as_str().unwrap().to_owned()),
            );
            assert!(ids.len() > before, "补页没有新增记录");
            assert_ne!(history["itemsNextCursor"], cursor, "消息游标没有前进");
            cursor = history["itemsNextCursor"].clone();
            pages += 1;
        }
        assert!(
            service.loaded.lock().await.is_empty(),
            "只读核对不能接管或恢复用户会话"
        );
        println!(
            "HISTORY_REENTRY_VERIFIED {}",
            json!({"initialItems":first.len(),
            "uniqueItemsAfterPaging":ids.len(),"pages":pages,"inputVisibleOnReentry":true,
            "ownedThreads":0})
        );
        service.release().await;
    }

    #[tokio::test]
    #[ignore]
    async fn installed_engine_prewarm_smoke() {
        let directory = tempfile::tempdir().unwrap();
        let service = Harness::new(directory.path()).unwrap();
        let started = std::time::Instant::now();
        service.prewarm().await.unwrap();
        service
            .catalog
            .find(&service.runtime, &service.projection, |_| false)
            .await
            .unwrap();
        let preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
        let first_use = std::time::Instant::now();
        let sidebar = service.call("sidebar", json!({}), None).await.unwrap();
        println!(
            "HARNESS_PREWARM_READY {}",
            json!({
                "preparationMs":preparation_ms,"firstUseMs":first_use.elapsed().as_secs_f64()*1000.0,
                "complete":sidebar["complete"],"rows":sidebar["recent"].as_array().unwrap().len(),
                "dispatchEnabled":service.enabled.load(Ordering::Acquire),
                "ownedThreads":service.loaded.lock().await.len()
            })
        );
        assert_eq!(sidebar["complete"], true);
        assert!(!service.enabled.load(Ordering::Acquire));
        assert!(service.loaded.lock().await.is_empty());
        service.stop_preparation().await;
    }

    #[tokio::test]
    #[ignore]
    async fn installed_engine_performance_smoke() {
        let directory = tempfile::tempdir().unwrap();
        let service = Harness::new(directory.path()).unwrap();
        let started = std::time::Instant::now();
        let first = service.call("sidebar", json!({}), None).await.unwrap();
        let first_ms = started.elapsed().as_secs_f64() * 1000.0;
        let warm_started = std::time::Instant::now();
        let warm = service.call("sidebar", json!({}), None).await.unwrap();
        let warm_ms = warm_started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(first["recent"], warm["recent"]);
        println!(
            "HARNESS_CATALOG_FIRST {}",
            json!({
                "firstMs":first_ms,"firstRows":first["recent"].as_array().unwrap().len(),
                "firstComplete":first["complete"],"warmMs":warm_ms
            })
        );
        let model_started = std::time::Instant::now();
        service.call("models", json!({}), None).await.unwrap();
        let model_ms = model_started.elapsed().as_secs_f64() * 1000.0;
        let model_warm_started = std::time::Instant::now();
        service.call("models", json!({}), None).await.unwrap();
        println!(
            "HARNESS_MODEL_READ {}",
            json!({
                "firstMs":model_ms,"warmMs":model_warm_started.elapsed().as_secs_f64()*1000.0
            })
        );
        if let Some(project) = first["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["synthetic"] != true)
        {
            let context_started = std::time::Instant::now();
            service
                .workspace_context(&json!({"projectId":project["id"]}))
                .await
                .unwrap();
            println!(
                "HARNESS_PROJECT_CONTEXT {}",
                json!({
                    "milliseconds":context_started.elapsed().as_secs_f64()*1000.0
                })
            );
        }
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let sidebar = service.call("sidebar", json!({}), None).await.unwrap();
                if sidebar["complete"] == true {
                    println!(
                        "HARNESS_CATALOG_COMPLETE {}",
                        json!({
                            "totalMs":started.elapsed().as_secs_f64()*1000.0,
                            "rows":sidebar["recent"].as_array().unwrap().len()
                        })
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        service.release().await;
    }

    #[tokio::test]
    #[ignore]
    async fn installed_engine_native_pins_read_only_smoke() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = Harness::new(directory.path()).unwrap();
        Arc::get_mut(&mut service).unwrap().project_codex_home =
            crate::codex_home::default_codex_home_dir();
        service.prepare().await.unwrap();
        let expected = service
            .list_all(
                "thread/list",
                json!({
                    "limit":100,"archived":false,"sectionId":"01984de2-8f74-7c91-a3b2-5c5e937cf318",
                    "sortKey":"section_position","useStateDbOnly":true
                }),
            )
            .await
            .unwrap();
        let sidebar = service.call("sidebar", json!({}), None).await.unwrap();
        let ids = |rows: &[Value]| rows.iter().map(|row| row["id"].clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(sidebar["pinnedThreads"].as_array().unwrap()),
            ids(&expected)
        );
        println!(
            "HARNESS_NATIVE_PINS {}",
            json!({
                "nativePinnedCount":expected.len(),
                "displayedPinnedCount":sidebar["pinnedThreads"].as_array().unwrap().len(),
                "readOnly":true
            })
        );
        service.release().await;
    }

    #[tokio::test]
    #[ignore]
    async fn installed_engine_read_only_smoke() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = Harness::new(directory.path()).unwrap();
        Arc::get_mut(&mut service).unwrap().project_codex_home =
            crate::codex_home::default_codex_home_dir();
        service.prepare().await.unwrap();
        let threads = service
            .runtime
            .request("thread/list", json!({"limit":1,"archived":false}), None)
            .await
            .unwrap();
        let projects = service
            .list_all("project/list", json!({"limit":100}))
            .await
            .unwrap();
        assert!(threads["data"].is_array());
        let sidebar = service.sidebar().await.unwrap();
        let displayed_projects = sidebar["projects"].as_array().unwrap();
        let project_ids = projects
            .iter()
            .map(|project| &project["id"])
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(displayed_projects.len(), project_ids.len());
        assert!(
            displayed_projects
                .iter()
                .all(|project| project_ids.contains(&project["serverProjectId"]))
        );
        assert!(
            displayed_projects
                .iter()
                .all(|project| project["synthetic"] != true)
        );
        let desktop: Value =
            std::fs::read(service.project_codex_home.join(".codex-global-state.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or(Value::Null);
        let expected_order = ["pinned-project-ids", "project-order"]
            .into_iter()
            .flat_map(|key| desktop[key].as_array().into_iter().flatten())
            .filter(|id| {
                displayed_projects
                    .iter()
                    .any(|project| project["id"] == **id)
            })
            .collect::<Vec<_>>();
        if !expected_order.is_empty() {
            assert_eq!(
                displayed_projects
                    .iter()
                    .take(expected_order.len())
                    .map(|project| &project["id"])
                    .collect::<Vec<_>>(),
                expected_order,
                "真实列表顺序应与桌面置顶和项目顺序一致"
            );
        }
        let assigned = sidebar["recent"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|thread| thread["projectId"].is_string())
            .count();
        println!(
            "HARNESS_SAVED_PROJECTS {}",
            json!({
                "savedProjects":projects.len(),"displayedProjects":displayed_projects.len(),
                "firstPageThreads":sidebar["recent"].as_array().unwrap().len(),
                "assignedThreads":assigned,
                "projectlessThreads":sidebar["recent"].as_array().unwrap().iter()
                    .filter(|thread|thread["projectId"].is_null()).count()
            })
        );
        if let Some(id) = threads["data"]
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row["id"].as_str())
        {
            let goal = service
                .runtime
                .request("thread/goal/get", json!({"threadId":id}), None)
                .await;
            assert!(goal.is_ok(), "goal read failed: {goal:?}");
            let turns = service
                .runtime
                .request(
                    "thread/turns/list",
                    json!({"threadId":id,"limit":1,"itemsView":"full","sortDirection":"desc"}),
                    None,
                )
                .await
                .unwrap();
            assert!(turns["data"].is_array());
        }
        service.release().await;
    }
}
