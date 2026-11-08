//! 远程会话服务：统一维护执行引擎、会话归属、队列、编辑事务、审批和事件。
//! 用户要求关闭 CDP 后完整切换；本模块不包含 CDP、页面脚本或注入回退。
mod catalog;
mod commands;
mod projection;
mod projects;
mod read_cache;
mod reads;
mod runtime;
mod store;

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
    owners: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    loaded: Mutex<HashMap<String, String>>,
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
        for (_, task) in self.owners.get_mut().drain() {
            task.abort();
        }
    }
}

impl Harness {
    #[cfg(test)]
    pub async fn install_test_process(&self, process: crate::codex_service::RunningService) {
        self.runtime.install_test_process(process).await;
    }
    pub fn new(directory: &Path) -> anyhow::Result<Arc<Self>> {
        let (events, receiver) = mpsc::channel(512);
        let audit = Arc::new(execution::Audit::default());
        Ok(Arc::new(Self {
            audit: audit.clone(),
            runtime: Arc::new(Runtime::new(events, audit)),
            catalog: Arc::new(Catalog::default()),
            projection: Arc::new(Mutex::new(Projection::default())),
            store: Mutex::new(Store::open(directory)?),
            #[cfg(test)]
            project_codex_home: directory.to_path_buf(),
            controller: OnceLock::new(),
            receiver: Mutex::new(Some(receiver)),
            worker: Mutex::new(None),
            queue_worker: Mutex::new(None),
            owners: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            loaded: Mutex::new(HashMap::new()),
            enabled: AtomicBool::new(false),
        }))
    }

    pub fn attach(&self, controller: Weak<RemoteAccessController>) {
        let _ = self.controller.set(controller);
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
                        service.catalog.observe(&frame).await;
                        service.projection.lock().await.ingest(frame);
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

    pub async fn prepare(self: &Arc<Self>) -> anyhow::Result<()> {
        execution::Scope::service(self.audit.clone(), "prepare", &json!({}), None)
            .run(self.start())
            .await
    }

    pub async fn prewarm(self: &Arc<Self>) -> anyhow::Result<()> {
        // 用户要求即使开启 CDP 也异步提前准备服务模式；这里只读共享目录，
        // 不改设备开关、enabled 派发标志或会话归属，不调用 resume / turn/start。
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

    pub async fn can_leave(&self, device_id: &str) -> anyhow::Result<()> {
        let state = self.store.lock().await;
        if state
            .data
            .queues
            .values()
            .flatten()
            .any(|message| message.device_id == device_id)
            || self.projection.lock().await.active.keys().any(|id| {
                state
                    .data
                    .thread_devices
                    .get(id)
                    .is_some_and(|owner| owner == device_id)
            })
        {
            return Err(error(
                409,
                "请先完成或停止当前任务，并处理待发送消息后再切换",
            ));
        }
        Ok(())
    }

    pub async fn owns(&self, id: &str) -> bool {
        self.loaded.lock().await.contains_key(id)
    }

    pub async fn idle(&self) -> bool {
        self.store
            .lock()
            .await
            .data
            .queues
            .values()
            .all(Vec::is_empty)
            && self.projection.lock().await.active.is_empty()
    }

    pub async fn release(&self) {
        self.catalog.reset().await;
        for (_, task) in self.owners.lock().await.drain() {
            task.abort();
        }
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
        self.projection
            .lock()
            .await
            .threads
            .insert(id.to_owned(), thread);
        self.loaded
            .lock()
            .await
            .insert(id.to_owned(), connection.generation.clone());
        Ok(())
    }

    async fn remember_settings(&self, id: &str, settings: &Value) -> anyhow::Result<()> {
        let saved = self.store.lock().await.commit(|data| {
            let current = data
                .settings
                .entry(id.to_owned())
                .or_insert_with(|| json!({}));
            for key in ["model", "reasoningEffort", "serviceTier"] {
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
        if self.locally_loaded(id).await? {
            return Ok(None);
        }
        let snapshot = crate::desktop_owner_ipc::read_snapshot(id)
            .await
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
                // 用户要求 IPC 故障不能冒充无持有者；未发送和已发送待核对必须分别保留，均不回退 CDP。
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
        self.start().await?;
        if let Some(authorization) = authorization {
            authorization.check().await?;
        }
        let result = match action {
            "events" => {
                self.runtime.check_health().await?;
                Ok(self.projection.lock().await.events(&payload))
            }
            "notifications" => Ok(self.projection.lock().await.notifications(&payload)),
            "sidebar" => self.sidebar().await,
            "models" => self.models(&payload).await,
            "skills" => self.skills(&payload).await,
            "thread" => self.thread(&payload).await,
            "history" => self.history(&payload).await,
            "workspaceContext" => self.workspace_context(&payload).await,
            "queue" => Ok(self.queue(required(&payload, "threadId")?).await),
            "queueEdit" => self.queue_edit(&payload).await,
            "sendState" => self.send_state(&payload).await,
            "relatedThreads" => Ok(
                json!({"ok":true,"threadId":payload["threadId"],"subagents":[],
                "completedSubagents":[],"completedSubagentCount":0,"sideThreads":[],
                "canCreateSideThread":false,"subagentsComplete":true}),
            ),
            "createSideThread" | "closeSideThread" => Err(error(409, "不支持的会话操作")),
            "dispose" => Ok(json!({"ok":true})),
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
        let epoch = self.projection.lock().await.epoch.clone();
        self.store.lock().await.queue(id, &epoch)
    }

    async fn queue_changed(&self, id: &str) {
        let snapshot = self.queue(id).await;
        self.projection
            .lock()
            .await
            .push(json!({"method":"thread/queue/changed","params":snapshot}));
    }

    async fn drain_queues(self: &Arc<Self>) {
        let candidates: Vec<_> = self
            .store
            .lock()
            .await
            .data
            .queues
            .iter()
            .filter(|(_, q)| q.first().is_some_and(|m| m.state == "queued"))
            .map(|(id, _)| id.clone())
            .collect();
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
            let message = self
                .store
                .lock()
                .await
                .data
                .queues
                .get(&id)
                .and_then(|q| q.first())
                .cloned();
            let Some(message) = message.filter(|q| q.state == "queued") else {
                continue;
            };
            let result = self.dispatch_queued(&id, &message).await;
            if let Err(error) = result {
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
            true,
            "queue-dispatch",
            &json!({"threadId":id}),
            Some(&authorization),
            message.runtime_revision,
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
        if let Some(snapshot) = self.owner_snapshot(id).await? {
            let active = crate::desktop_owner_ipc::snapshot_turns(&snapshot)
                .iter()
                .any(|t| t["status"] == "inProgress");
            if active {
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
        self.store.lock().await.commit(|data| {
            let queued = data
                .queues
                .get_mut(id)
                .and_then(|q| q.iter_mut().find(|q| q.id == message.id))
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
        let mut owners = self.owners.lock().await;
        if owners.get(id).is_some_and(|task| !task.is_finished()) {
            return;
        }
        if owners.len() >= 8 {
            if let Some(key) = owners.keys().next().cloned() {
                if let Some(task) = owners.remove(&key) {
                    task.abort();
                }
            }
        }
        let projection = self.projection.clone();
        let id_owned = id.to_owned();
        owners.insert(id.to_owned(), execution::spawn_service(self.audit.clone(), "desktop-events", async move {
            let mut previous = baseline;
            let mut changes = crate::codex_service::ServiceProjection::new();
            let mut cursor = changes.events(&json!({}));
            let (sender, mut receiver) = mpsc::channel(8);
            let watch = crate::desktop_owner_ipc::watch_snapshots(id_owned.clone(), sender);
            tokio::pin!(watch);
            loop {
                tokio::select! {
                    Some(observation) = receiver.recv() => {
                        changes.project_changes(&id_owned, &previous, &observation.snapshot);
                        cursor = changes.events(&cursor);
                        let mut state = projection.lock().await;
                        for event in cursor["events"].as_array().into_iter().flatten() {
                    if event["method"]=="remote/notification" {continue;}
                            state.ingest(event.clone());
                        }
                        if let Ok(thread) = crate::codex_service::normalize_owner_thread(&observation.snapshot, None) {
                            state.threads.insert(id_owned.clone(), thread);
                        }
                        previous = observation.snapshot;
                    }
                    _ = &mut watch => break,
                }
            }
            projection.lock().await.push(json!({"method":"remote/resync"}));
        }));
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
                "真实列表顺序应与 CDP 的桌面置顶和项目顺序一致"
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
