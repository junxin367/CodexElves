//! Codex 仅是执行引擎；连接、请求关联和事件接收由远程会话服务统一管理。
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::io::AsyncBufReadExt;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::codex_service::{ServiceDispatchGuard, spawn_service};
use crate::desktop_owner_ipc::{WriteFrame, guarded_write};

const TIMEOUT: Duration = Duration::from_secs(45);
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub(super) struct Runtime {
    connection: Mutex<Option<Arc<Connection>>>,
    events: mpsc::Sender<Value>,
    next_generation: AtomicU64,
    retry_after: Mutex<Option<Instant>>,
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
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl Runtime {
    pub fn new(events: mpsc::Sender<Value>) -> Self {
        Self {
            connection: Mutex::new(None),
            events,
            next_generation: AtomicU64::new(1),
            retry_after: Mutex::new(None),
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
        self.events
            .send(json!({"method":"harness/connected","_generation":generation,"_generationIndex":generation_index}))
            .await
            .context("会话事件接收器已关闭")?;
        let reader = tokio::spawn(async move {
            // 持续接收事件，不能像请求兜底通道那样在等待响应时丢弃通知和审批。
            let mut child = service.child;
            let mut lines = service.stdout.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(mut frame) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if frame.get("method").is_some() {
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
            reader,
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
        self.connection()
            .await?
            .request(method, params, guard)
            .await
    }

    pub async fn respond(
        &self,
        generation: &str,
        id: Value,
        result: Value,
        guard: &dyn ServiceDispatchGuard,
    ) -> anyhow::Result<()> {
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
        self.connection.lock().await.take();
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
            return Err(super::super::ApiError {
                code: if rejected { 409 } else { 503 },
                message: format!(
                    "Codex 操作失败：{}",
                    error["message"].as_str().unwrap_or("未知错误")
                ),
                outcome_unknown: guard.is_some() && !rejected,
            }
            .into());
        }
        Ok(frame.get("result").cloned().unwrap_or(Value::Null))
    }
}
