//! Codex 仅是执行引擎；连接、请求关联和事件接收由远程会话服务统一管理。
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::io::AsyncBufReadExt;
use tokio::sync::{Mutex, mpsc, oneshot};

use super::execution;
use super::read_cache::ReadCache;
use crate::codex_service::{ServiceDispatchGuard, spawn_service};
use crate::desktop_owner_ipc::{WriteFrame, guarded_write};

const TIMEOUT: Duration = Duration::from_secs(45);
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

#[derive(Debug)]
pub(super) struct UnsupportedMethod;

impl std::fmt::Display for UnsupportedMethod {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("执行引擎不支持此方法")
    }
}

pub(super) struct Runtime {
    audit: Arc<execution::Audit>,
    health: Mutex<Option<(Instant, String, Result<(), String>)>>,
    connection: Mutex<Option<Arc<Connection>>>,
    events: mpsc::Sender<Value>,
    next_generation: AtomicU64,
    retry_after: Mutex<Option<Instant>>,
    reads: Arc<ReadCache>,
    #[cfg(test)]
    test_process: Mutex<Option<crate::codex_service::RunningService>>,
    #[cfg(test)]
    test_only: AtomicBool,
}

pub(super) struct Connection {
    pub generation: String,
    pub generation_index: u64,
    alive: Arc<AtomicBool>,
    stdin: mpsc::Sender<WriteFrame>,
    write_gate: Mutex<()>,
    pending: Pending,
    next_id: AtomicU64,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.get_mut().take() {
            reader.abort();
        }
    }
}

impl Runtime {
    #[cfg(test)]
    pub fn has_test_process(&self) -> bool {
        self.test_only.load(Ordering::Acquire)
    }

    pub fn new(events: mpsc::Sender<Value>, audit: Arc<execution::Audit>) -> Self {
        Self {
            audit,
            health: Mutex::new(None),
            connection: Mutex::new(None),
            events,
            next_generation: AtomicU64::new(1),
            retry_after: Mutex::new(None),
            reads: Arc::new(ReadCache::default()),
            #[cfg(test)]
            test_process: Mutex::new(None),
            #[cfg(test)]
            test_only: AtomicBool::new(false),
        }
    }

    pub async fn connection(&self) -> anyhow::Result<Arc<Connection>> {
        let mut current = self.connection.lock().await;
        if let Some(connection) = current.as_ref().filter(|c| c.alive.load(Ordering::Acquire)) {
            return Ok(connection.clone());
        }
        if current.take().is_some() {
            *self.retry_after.lock().await = Some(Instant::now() + Duration::from_secs(3));
        }
        if self
            .retry_after
            .lock()
            .await
            .is_some_and(|until| Instant::now() < until)
        {
            bail!("远程会话引擎正在恢复，请稍后重试");
        }
        #[cfg(test)]
        let supplied = self.test_process.lock().await.take();
        #[cfg(not(test))]
        let supplied: Option<crate::codex_service::RunningService> = None;
        #[cfg(test)]
        if supplied.is_none() && self.test_only.load(Ordering::Acquire) {
            bail!("隔离测试引擎已结束，禁止回退到本机真实 Codex");
        }
        let service = match supplied {
            Some(service) => service,
            None => match spawn_service().await {
                Ok(service) => service,
                Err(error) => {
                    *self.retry_after.lock().await = Some(Instant::now() + Duration::from_secs(30));
                    return Err(error);
                }
            },
        };
        let generation = uuid::Uuid::new_v4().to_string();
        let generation_index = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let reader_pending = pending.clone();
        let reader_alive = alive.clone();
        let events = self.events.clone();
        let reader_generation = generation.clone();
        self.reads.clear().await;
        let reader_reads = self.reads.clone();
        self.events
            .send(json!({"method":"harness/connected","_generation":generation,"_generationIndex":generation_index}))
            .await
            .context("会话事件接收器已关闭")?;
        let reader = execution::spawn_service(self.audit.clone(), "engine-reader", async move {
            // 持续接收事件，不能像请求兜底通道那样在等待响应时丢弃通知和审批。
            let mut child = service.child;
            let mut lines = service.stdout.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(mut frame) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if frame.get("method").is_some() {
                    if let Some(id) = frame["params"]["threadId"]
                        .as_str()
                        .or_else(|| frame["params"]["thread"]["id"].as_str())
                    {
                        reader_reads.invalidate(id).await;
                    }
                    if frame["method"] == "skills/changed" {
                        reader_reads.invalidate("skills").await;
                    }
                    let method = frame["method"].as_str().unwrap_or("");
                    if method == "thread/section/changed" {
                        reader_reads.invalidate("thread/list").await;
                    }
                    if method.starts_with("project/") {
                        reader_reads.invalidate("project/list").await;
                    }
                    if method.starts_with("model/") || method.starts_with("config/") {
                        reader_reads.invalidate("model/list").await;
                        reader_reads.invalidate("config/read").await;
                    }
                    frame["_generation"] = json!(&reader_generation);
                    frame["_generationIndex"] = json!(generation_index);
                    if events.send(frame).await.is_err() {
                        break;
                    }
                } else if let Some(id) = frame["id"].as_u64() {
                    if let Some(reply) = reader_pending.lock().await.remove(&id) {
                        let _ = reply.send(frame);
                    }
                }
            }
            reader_alive.store(false, Ordering::Release);
            reader_pending.lock().await.clear();
            let _ = child.start_kill();
            let _ = child.wait().await;
            let _ = events
                .send(json!({"method":"harness/disconnected",
                "_generation":reader_generation,"_generationIndex":generation_index}))
                .await;
        });
        let connection = Arc::new(Connection {
            generation,
            generation_index,
            alive,
            stdin: service.stdin,
            write_gate: Mutex::new(()),
            pending,
            next_id: AtomicU64::new(1),
            reader: Mutex::new(Some(reader)),
        });
        let initialized = connection
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":"codex-elves-harness","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"experimentalApi":true}
                }),
                None,
            )
            .await
            .context("远程会话引擎初始化失败");
        if let Err(error) = initialized {
            *self.retry_after.lock().await = Some(Instant::now() + Duration::from_secs(30));
            return Err(error);
        }
        connection
            .write(json!({"method":"initialized","params":{}}), None)
            .await?;
        *current = Some(connection.clone());
        Ok(connection)
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Value> {
        execution::transport("app-server-stdio", method, guard.is_some());
        if guard.is_some()
            && let Some(id) = params["threadId"].as_str()
        {
            self.reads.invalidate(id).await;
        }
        let thread_id = params["threadId"].as_str().map(str::to_owned);
        let result = self
            .connection()
            .await?
            .request(method, params, guard)
            .await;
        if guard.is_some()
            && let Some(id) = thread_id
        {
            self.reads.invalidate(&id).await;
        }
        result
    }

    pub async fn read(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        execution::transport("app-server-stdio", method, false);
        let connection = self.connection().await?;
        let scope = params["threadId"].as_str().unwrap_or_else(|| {
            if method == "skills/list" {
                "skills"
            } else {
                method
            }
        });
        let max_age = match method {
            "project/list" => Duration::from_secs(30),
            "model/list" | "config/read" => Duration::from_secs(30),
            "skills/list" => Duration::from_secs(5),
            _ => Duration::from_millis(200),
        };
        let key = serde_json::to_string(&json!([connection.generation, method, params]))?;
        self.reads
            .read(
                scope,
                key,
                max_age,
                connection.request(method, params.clone(), None),
            )
            .await
    }

    pub async fn request_in_generation(
        &self,
        generation: &str,
        method: &str,
        params: Value,
        guard: &dyn ServiceDispatchGuard,
    ) -> anyhow::Result<Value> {
        execution::transport("app-server-stdio", method, true);
        let connection = self.connection().await?;
        // 临时侧边会话只属于创建它的引擎，断线后不能把旧写入交给新进程或自动 resume。
        if connection.generation != generation {
            return Err(super::error(410, "侧边会话所属的服务连接已结束"));
        }
        let id = params["threadId"].as_str().unwrap_or("");
        self.reads.invalidate(id).await;
        let result = connection
            .request(method, params.clone(), Some(guard))
            .await;
        self.reads.invalidate(id).await;
        result
    }

    pub async fn invalidate_read(&self, scope: &str) {
        self.reads.invalidate(scope).await;
    }

    pub async fn respond(
        &self,
        generation: &str,
        id: Value,
        result: Value,
        guard: &dyn ServiceDispatchGuard,
    ) -> anyhow::Result<()> {
        execution::transport("app-server-stdio", "approval-response", true);
        let connection = self
            .connection
            .lock()
            .await
            .clone()
            .filter(|c| c.generation == generation && c.alive.load(Ordering::Acquire))
            .context("审批所属的执行引擎已断开，请刷新会话")?;
        connection
            .write(json!({"id":id,"result":result}), Some(guard))
            .await
    }

    pub async fn shutdown(&self) {
        let connection = self.connection.lock().await.take();
        // 用户要求断开后能独立恢复；即使旧请求仍持有连接，也须先结束旧引擎，防止两个写入者并存。
        if let Some(connection) = connection {
            connection.alive.store(false, Ordering::Release);
            connection.pending.lock().await.clear();
            if let Some(reader) = connection.reader.lock().await.take() {
                reader.abort();
                let _ = reader.await;
            }
        }
        self.reads.clear().await;
        self.health.lock().await.take();
    }

    pub async fn check_health(&self) -> anyhow::Result<()> {
        let connection = self.connection().await?;
        let mut health = self.health.lock().await;
        // 用户要求服务健康不依赖 CDP；5 秒内复用探测结果，限制每分钟最多 12 次只读 RPC，
        // 单次最多等 3 秒；不 resume 会话或发送消息，也不以普通读取缓存冒充引擎可用。
        if let Some((at, generation, result)) = &*health
            && generation == &connection.generation
            && at.elapsed() < Duration::from_secs(5)
        {
            return result.clone().map_err(anyhow::Error::msg);
        }
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            connection.request("thread/list", json!({"limit":1,"archived":false}), None),
        )
        .await
        .map_err(|_| "远程会话引擎健康检查超时".to_owned())
        .and_then(|result| result.map(|_| ()).map_err(|error| error.to_string()));
        *health = Some((
            Instant::now(),
            connection.generation.clone(),
            result.clone(),
        ));
        result.map_err(anyhow::Error::msg)
    }

    #[cfg(test)]
    pub async fn install_test_process(&self, process: crate::codex_service::RunningService) {
        self.test_only.store(true, Ordering::Release);
        *self.test_process.lock().await = Some(process);
    }
}

impl Connection {
    async fn write(
        &self,
        frame: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<()> {
        let _gate = self.write_gate.lock().await;
        if !self.alive.load(Ordering::Acquire) {
            bail!("远程会话引擎已断开");
        }
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        guarded_write(&self.stdin, bytes, guard, TIMEOUT).await
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.retain(|_, sender| !sender.is_closed());
            anyhow::ensure!(pending.len() < 128, "远程会话请求过多，请稍后再试");
            pending.insert(id, sender);
        }
        if let Err(error) = self
            .write(json!({"id":id,"method":method,"params":params}), guard)
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        let response = tokio::time::timeout(TIMEOUT, receiver).await;
        self.pending.lock().await.remove(&id);
        let frame = response
            .context("请求已派发，等待执行引擎响应超时")?
            .context("请求已派发，执行引擎连接中断")?;
        if let Some(error) = frame.get("error") {
            let rejected = matches!(error["code"].as_i64(), Some(-32600 | -32601 | -32602));
            // 引擎内部错误可能出现在写入之后，不能当作“尚未发送”而允许重放。
            let failure = anyhow::Error::from(super::super::ApiError {
                code: if rejected { 409 } else { 503 },
                message: format!(
                    "Codex 操作失败：{}",
                    error["message"].as_str().unwrap_or("未知错误")
                ),
                outcome_unknown: guard.is_some() && !rejected,
            });
            // 仅 -32601 允许兼容旧引擎；权限、参数、内部错误或超时不能伪装成不支持置顶。
            return Err(if error["code"] == -32601 {
                failure.context(UnsupportedMethod)
            } else {
                failure
            });
        }
        Ok(frame.get("result").cloned().unwrap_or(Value::Null))
    }
}
