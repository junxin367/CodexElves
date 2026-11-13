//! Versioned desktop owner routing. It never starts a runtime or acquires a writer.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, bail, ensure};
#[cfg(test)]
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::codex_service::{ServiceDispatchGuard, ServiceDispatchPermit};

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_IGNORED_FRAMES: usize = 128;
const SNAPSHOT_VERSION: u64 = 11;
#[cfg(windows)]
const PIPE_PATH: &str = r"\\.\pipe\codex-ipc";

#[cfg(windows)]
fn pipe_path() -> String {
    // 无 CDP 验收使用本轮专属且不存在的管道，保证真实引擎不能借用用户正在运行的桌面。
    #[cfg(test)]
    if let Ok(path) = std::env::var("CODEX_ELVES_TEST_IPC_PIPE") {
        assert!(path.starts_with(r"\\.\pipe\codex-elves-isolation-"));
        return path;
    }
    PIPE_PATH.to_owned()
}

#[derive(Debug)]
pub(crate) struct DispatchFailure {
    pub dispatched: bool,
    message: String,
}

impl std::fmt::Display for DispatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DispatchFailure {}

pub(crate) struct WriteFrame {
    bytes: Vec<u8>,
    completion: oneshot::Sender<Result<(), String>>,
}

pub(crate) fn spawn_writer<W: AsyncWrite + Unpin + Send + 'static>(
    mut writer: W,
    timeout: Duration,
) -> (mpsc::Sender<WriteFrame>, JoinHandle<()>) {
    let (sender, mut receiver) = mpsc::channel::<WriteFrame>(1);
    let task = tokio::spawn(async move {
        while let Some(frame) = receiver.recv().await {
            let result = tokio::time::timeout(timeout, writer.write_all(&frame.bytes))
                .await
                .map_err(|_| "写入 Codex 请求超时".to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let failed = result.is_err();
            let _ = frame.completion.send(result);
            if failed {
                break;
            }
        }
    });
    (sender, task)
}

struct TrustedPermit;
impl ServiceDispatchPermit for TrustedPermit {
    fn commit(
        self: Box<Self>,
        dispatch: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()> {
        dispatch()
    }
}

/// The permit encloses only the synchronous, bounded queue insertion.
pub(crate) async fn guarded_write(
    sender: &mpsc::Sender<WriteFrame>,
    bytes: Vec<u8>,
    guard: Option<&dyn ServiceDispatchGuard>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let permit = match guard {
        Some(guard) => guard.prepare_dispatch().await.map_err(|error| {
            let message = format!("{error:#}");
            error.context(DispatchFailure {
                dispatched: false,
                message,
            })
        })?,
        None => Box::new(TrustedPermit) as Box<dyn ServiceDispatchPermit>,
    };
    let (completion, result) = oneshot::channel();
    let frame = WriteFrame { bytes, completion };
    let sender = sender.clone();
    let dispatched = Arc::new(AtomicBool::new(false));
    let queued = Arc::clone(&dispatched);
    let commit = permit.commit(Box::new(move || {
        sender
            .try_send(frame)
            .map_err(|_| anyhow::anyhow!("Codex 请求写入队列不可用"))?;
        queued.store(true, Ordering::Release);
        Ok(())
    }));
    if let Err(error) = commit {
        let message = format!("{error:#}");
        return Err(error.context(DispatchFailure {
            dispatched: dispatched.load(Ordering::Acquire),
            message,
        }));
    }
    ensure!(
        dispatched.load(Ordering::Acquire),
        "Codex 授权派发器未提交请求"
    );
    tokio::time::timeout(timeout, result)
        .await
        .map_err(|_| DispatchFailure {
            dispatched: true,
            message: "Codex 请求已派发，等待写入结果超时".to_string(),
        })?
        .map_err(|_| DispatchFailure {
            dispatched: true,
            message: "Codex 请求已派发，写入连接已关闭".to_string(),
        })?
        .map_err(|message| DispatchFailure {
            dispatched: true,
            message,
        })?;
    Ok(())
}

pub(crate) fn require_main_thread(thread: &Value, thread_id: &str) -> anyhow::Result<()> {
    ensure!(thread["id"] == thread_id, "会话身份已变化");
    ensure!(
        thread["ephemeral"] == false
            && thread.get("parentThreadId").is_none_or(Value::is_null)
            && thread.get("forkedFromId").is_some_and(Value::is_null)
            && thread
                .get("sideConversation")
                .is_none_or(|value| value == false),
        "此操作仅支持持久普通主会话"
    );
    ensure!(
        thread["threadSource"] == "user"
            && matches!(
                thread["source"].as_str(),
                Some("vscode" | "cli" | "exec" | "appServer")
            ),
        "无法确认会话是普通主会话，未发送"
    );
    ensure!(
        thread.get("hostId").is_none_or(|host| host == "local"),
        "此操作仅支持本机主会话"
    );
    ensure!(
        thread.get("canAcceptDirectInput") != Some(&Value::Bool(false)),
        "此会话不接受直接输入"
    );
    Ok(())
}

pub(crate) fn snapshot_turns(snapshot: &Value) -> Vec<Value> {
    let turns = if snapshot["turnHistory"]["kind"] == "canonical" {
        snapshot["turnHistory"]["history"]["entitiesByKey"]
            .as_object()
            .map(|turns| turns.values().cloned().collect())
            .unwrap_or_default()
    } else {
        snapshot["turns"].as_array().cloned().unwrap_or_default()
    };
    turns.into_iter().map(normalize_owner_turn).collect()
}

fn lifecycle_status(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value["type"].as_str())
        .or_else(|| value["status"].as_str())
}

fn normalize_owner_turn(mut turn: Value) -> Value {
    let running = matches!(
        lifecycle_status(&turn["status"]),
        Some("inProgress" | "active" | "running")
    );
    let completed_ids = turn["completedItemIds"].as_object().cloned();
    for item in turn["items"].as_array_mut().into_iter().flatten() {
        let Some(_) = item.as_object() else { continue };
        let raw_status = lifecycle_status(&item["status"]).map(str::to_string);
        let terminal_status = raw_status.as_deref().filter(|status| {
            matches!(
                *status,
                "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
            )
        });
        let completed_by_id = item["id"]
            .as_str()
            .and_then(|id| completed_ids.as_ref().and_then(|ids| ids.get(id)))
            == Some(&Value::Bool(true));
        let has_completed_time = item["completedAtMs"].as_u64().is_some();
        let has_error = item.get("error").is_some_and(|error| {
            !error.is_null() && error != false && error.as_str().is_none_or(|text| !text.is_empty())
        });
        let outcome_failed = item["success"] == false
            || item["result"]["isError"] == true
            || has_error
            || item["exitCode"].as_i64().is_some_and(|code| code != 0);
        let has_outcome = item["success"].is_boolean()
            || item["exitCode"].as_i64().is_some()
            || item["result"]["isError"].is_boolean()
            || has_error;
        let complete =
            terminal_status.is_some() || completed_by_id || has_completed_time || has_outcome;
        let known_pending = running
            && (item["startedAtMs"].as_u64().is_some()
                || completed_ids.is_some() && item["id"].as_str().is_some_and(|id| !id.is_empty()));
        // The owner stores completion membership in a dictionary. Tool duration,
        // elapsed wall time, and a terminal parent turn do not prove item success.
        let status = if let Some(status) = terminal_status.filter(|status| *status != "completed") {
            status
        } else if complete {
            if outcome_failed {
                "failed"
            } else {
                "completed"
            }
        } else if let Some(status) = raw_status.as_deref().filter(|status| !status.is_empty()) {
            status
        } else if known_pending {
            "inProgress"
        } else {
            "unknown"
        };
        item["status"] = json!(status);
        item["remoteItemCompleted"] = json!(complete);
    }
    turn
}

pub(crate) struct OwnerSnapshot {
    pub snapshot: Value,
}

/// 服务模式通过已有 owner 执行操作；不存在 owner 才允许由自己的运行时接管。
/// 此入口不连接 CDP，也不在页面中执行脚本。
#[cfg(windows)]
pub(crate) async fn request_owner(
    thread_id: &str,
    method: &str,
    params: Value,
    version: u64,
    guard: Option<&dyn ServiceDispatchGuard>,
) -> anyhow::Result<Option<Value>> {
    crate::remote_access::execution::transport("desktop-ipc", method, true);
    use tokio::net::windows::named_pipe::ClientOptions;
    let stream = match ClientOptions::new().open(pipe_path()) {
        Ok(stream) => stream,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut session = Session::new(stream, thread_id, RPC_TIMEOUT);
    session.initialize().await?;
    let Some(owner) = session.discover(None).await? else {
        return Ok(None);
    };
    session.owner = Some(owner.clone());
    let snapshot = session.fresh_snapshot().await?;
    ensure!(
        snapshot["ephemeral"] != true && snapshot.get("parentThreadId").is_none_or(Value::is_null),
        "服务模式仅支持普通主会话"
    );
    let request_id = uuid::Uuid::new_v4().to_string();
    session
        .write(
            &json!({
                "type":"request","requestId":request_id,"sourceClientId":session.client_id,
                "method":method,"version":version,"targetClientId":owner,"params":params,
                "timeoutMs":RPC_TIMEOUT.as_millis() as u64
            }),
            guard,
        )
        .await?;
    let response = session
        .wait_response(&request_id, method, Some(&owner))
        .await;
    let _ = session.following(false).await;
    match response {
        Ok(value) => Ok(Some(value["result"].clone())),
        Err(error) => {
            let message = format!("{error:#}");
            Err(error.context(DispatchFailure {
                dispatched: true,
                message,
            }))
        }
    }
}

#[cfg(not(windows))]
pub(crate) async fn request_owner(
    _thread_id: &str,
    _method: &str,
    _params: Value,
    _version: u64,
    _guard: Option<&dyn ServiceDispatchGuard>,
) -> anyhow::Result<Option<Value>> {
    Ok(None)
}

#[cfg(not(windows))]
pub(crate) async fn watch_snapshots(
    _thread_id: String,
    _changes: mpsc::Sender<OwnerSnapshot>,
) -> anyhow::Result<()> {
    bail!("当前平台没有桌面 owner IPC")
}

#[cfg(windows)]
pub(crate) async fn watch_snapshots(
    thread_id: String,
    changes: mpsc::Sender<OwnerSnapshot>,
) -> anyhow::Result<()> {
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_path())?;
    watch_with_stream(stream, &thread_id, changes).await
}

async fn watch_with_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    thread_id: &str,
    changes: mpsc::Sender<OwnerSnapshot>,
) -> anyhow::Result<()> {
    let mut session = Session::new(stream, thread_id, RPC_TIMEOUT);
    session.initialize().await?;
    let owner = session
        .discover(None)
        .await?
        .context("未发现此主会话的 owner")?;
    session.owner = Some(owner.clone());
    let snapshot = session.fresh_snapshot().await?;
    require_main_thread(&snapshot, thread_id)?;
    changes.send(OwnerSnapshot { snapshot }).await?;
    loop {
        let message = tokio::select! {
            _ = changes.closed() => return Ok(()),
            message = session.read_frame() => message?,
        };
        if message["type"] == "broadcast"
            && message["method"] == "client-status-changed"
            && message["params"]["clientId"] == owner
            && message["params"]["status"] == "disconnected"
        {
            bail!("主会话 owner 已断开");
        }
        let relevant = message["type"] == "broadcast"
            && message["method"] == "thread-stream-state-changed"
            && message["params"]["conversationId"] == thread_id;
        session.observe(&message).await?;
        if !relevant {
            continue;
        }
        let snapshot = match session.snapshot.take() {
            Some(snapshot) => snapshot,
            // Snapshot projection is exact; unsupported patch operations are
            // never guessed. Ask the same owner for its current full state.
            None => session.fresh_snapshot().await?,
        };
        require_main_thread(&snapshot, thread_id)?;
        changes.send(OwnerSnapshot { snapshot }).await?;
    }
}

#[cfg(not(windows))]
pub(crate) async fn read_snapshot(_thread_id: &str) -> anyhow::Result<Option<Value>> {
    Ok(None)
}

#[cfg(windows)]
pub(crate) async fn read_snapshot(thread_id: &str) -> anyhow::Result<Option<Value>> {
    read_snapshot_kind(thread_id, false).await
}

#[cfg(not(windows))]
pub(crate) async fn read_view_snapshot(_thread_id: &str) -> anyhow::Result<Option<Value>> {
    Ok(None)
}

#[cfg(windows)]
pub(crate) async fn read_view_snapshot(thread_id: &str) -> anyhow::Result<Option<Value>> {
    read_snapshot_kind(thread_id, true).await
}

fn require_view_snapshot(
    snapshot: &Value,
    thread_id: &str,
    allow_subagents: bool,
) -> anyhow::Result<()> {
    if allow_subagents && crate::codex_service::subagent_parent_id(snapshot).is_some() {
        // 用户只恢复子智能体查看：仅扩展此读取校验，发送、审批和 owner 监听仍使用 require_main_thread。
        ensure!(snapshot["id"] == thread_id, "子智能体快照身份已变化");
        ensure!(
            snapshot.get("hostId").is_none_or(|host| host == "local"),
            "只支持读取本机子智能体快照"
        );
        crate::codex_service::normalize_subagent_thread(snapshot)?;
        Ok(())
    } else {
        require_main_thread(snapshot, thread_id)
    }
}

#[cfg(windows)]
async fn read_snapshot_kind(
    thread_id: &str,
    allow_subagents: bool,
) -> anyhow::Result<Option<Value>> {
    let stream = match tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_path()) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    read_snapshot_with_stream(stream, thread_id, allow_subagents).await
}

async fn read_snapshot_with_stream<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    thread_id: &str,
    allow_subagents: bool,
) -> anyhow::Result<Option<Value>> {
    let mut session = Session::new(stream, thread_id, RPC_TIMEOUT);
    let result = async {
        session.initialize().await?;
        let Some(owner) = session.discover(None).await? else {
            return Ok(None);
        };
        session.owner = Some(owner);
        let snapshot = session.fresh_snapshot().await?;
        require_view_snapshot(&snapshot, thread_id, allow_subagents)?;
        Ok(Some(snapshot))
    }
    .await;
    if session.following {
        let _ = session.following(false).await;
    }
    result
}

struct Session<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> {
    reader: tokio::io::ReadHalf<S>,
    writer: mpsc::Sender<WriteFrame>,
    writer_task: JoinHandle<()>,
    client_id: Option<String>,
    thread_id: String,
    owner: Option<String>,
    snapshot: Option<Value>,
    following: bool,
    revision: Option<u64>,
    timeout: Duration,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Drop for Session<S> {
    fn drop(&mut self) {
        self.writer_task.abort();
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Session<S> {
    fn new(stream: S, thread_id: &str, timeout: Duration) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let (writer, writer_task) = spawn_writer(writer, timeout);
        Self {
            reader,
            writer,
            writer_task,
            client_id: None,
            thread_id: thread_id.to_string(),
            owner: None,
            snapshot: None,
            following: false,
            revision: None,
            timeout,
        }
    }

    async fn initialize(&mut self) -> anyhow::Result<()> {
        let response = self
            .request("initialize", json!({"clientType":"codexelves"}), 0, None)
            .await?;
        let client_id = response["result"]["clientId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("桌面 IPC 初始化未返回客户端身份")?;
        ensure!(
            response["handledByClientId"] == client_id,
            "桌面 IPC 初始化身份不匹配"
        );
        self.client_id = Some(client_id.to_string());
        Ok(())
    }

    async fn read_frame(&mut self) -> anyhow::Result<Value> {
        let length = self.reader.read_u32_le().await? as usize;
        ensure!(
            (1..=MAX_FRAME_BYTES).contains(&length),
            "桌面 IPC 消息大小不受支持"
        );
        let mut bytes = vec![0; length];
        self.reader.read_exact(&mut bytes).await?;
        serde_json::from_slice(&bytes).context("桌面 IPC 消息不是有效 JSON")
    }

    async fn write(
        &self,
        message: &Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(message)?;
        ensure!(bytes.len() <= MAX_FRAME_BYTES, "桌面 IPC 请求超过大小限制");
        let mut frame = Vec::with_capacity(bytes.len() + 4);
        frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        frame.extend_from_slice(&bytes);
        guarded_write(&self.writer, frame, guard, self.timeout).await
    }

    async fn observe(&mut self, message: &Value) -> anyhow::Result<()> {
        if message["type"] == "client-discovery-request" {
            self.write(
                &json!({
                    "type":"client-discovery-response",
                    "requestId":message["requestId"],
                    "response":{"canHandle":false}
                }),
                None,
            )
            .await?;
        }
        if message["type"] == "broadcast"
            && message["method"] == "thread-stream-state-changed"
            && message["params"]["conversationId"] == self.thread_id
        {
            ensure!(
                message["sourceClientId"].as_str() == self.owner.as_deref()
                    && message["params"]["hostId"] == "local"
                    && message["targetClientIds"].as_array().is_some_and(|ids| ids
                        .iter()
                        .any(|id| id.as_str() == self.client_id.as_deref())),
                "桌面 IPC 快照来源或目标已变化"
            );
            ensure!(
                message["version"] == SNAPSHOT_VERSION,
                "桌面 IPC 快照协议版本不受支持"
            );
            let revision = message["params"]["change"]["revision"]
                .as_u64()
                .context("桌面 IPC 快照缺少修订号")?;
            ensure!(
                self.revision.is_none_or(|previous| revision >= previous),
                "桌面 IPC 快照已过期"
            );
            self.revision = Some(revision);
            match message["params"]["change"]["type"].as_str() {
                Some("snapshot") => {
                    let snapshot = message["params"]["change"]["conversationState"].clone();
                    ensure!(snapshot["id"] == self.thread_id, "桌面 IPC 快照会话已变化");
                    self.snapshot = Some(snapshot);
                }
                // A new full snapshot is required before dispatch; patches never
                // silently advance an unknown local state.
                Some("patches") => self.snapshot = None,
                _ => bail!("桌面 IPC 快照格式不受支持"),
            }
        }
        Ok(())
    }

    async fn wait_response(
        &mut self,
        request_id: &str,
        method: &str,
        expected_owner: Option<&str>,
    ) -> anyhow::Result<Value> {
        let timeout = self.timeout;
        tokio::time::timeout(timeout, async {
            for _ in 0..MAX_IGNORED_FRAMES {
                let message = self.read_frame().await?;
                self.observe(&message).await?;
                if message["type"] != "response" || message["requestId"] != request_id {
                    continue;
                }
                if message["resultType"] == "error" {
                    if message["error"] == "no-client-found" && expected_owner.is_none() {
                        return Ok(message);
                    }
                    bail!("桌面 IPC 拒绝请求：{}", message["error"]);
                }
                ensure!(
                    message["resultType"] == "success" && message["method"] == method,
                    "桌面 IPC 响应方法或协议不匹配"
                );
                if let Some(owner) = expected_owner {
                    ensure!(
                        message["handledByClientId"] == owner,
                        "桌面 IPC 会话 owner 已变化"
                    );
                }
                return Ok(message);
            }
            bail!("桌面 IPC 返回了过多无关消息")
        })
        .await
        .context("等待桌面 IPC 响应超时")?
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        version: u64,
        target: Option<&str>,
    ) -> anyhow::Result<Value> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.write(
            &json!({
                "type":"request", "requestId":request_id,
                "sourceClientId":self.client_id,"method":method,"version":version,
                "params":params,"targetClientId":target,"timeoutMs":self.timeout.as_millis() as u64
            }),
            None,
        )
        .await?;
        self.wait_response(&request_id, method, target).await
    }

    async fn discover(&mut self, target: Option<&str>) -> anyhow::Result<Option<String>> {
        let response = self
            .request(
                "thread-owner-discovery",
                json!({"hostId":"local","conversationId":self.thread_id}),
                1,
                target,
            )
            .await?;
        if response["resultType"] == "error" && response["error"] == "no-client-found" {
            return Ok(None);
        }
        ensure!(
            response["result"]["supportsUntrustedAppInput"] == true,
            "桌面 owner 未提供当前发送协议"
        );
        let owner = response["handledByClientId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("桌面 IPC 缺少会话 owner 身份")?;
        Ok(Some(owner.to_string()))
    }

    async fn following(&mut self, following: bool) -> anyhow::Result<()> {
        self.write(
            &json!({
                "type":"broadcast","method":"thread-stream-following-changed","version":1,
                "sourceClientId":self.client_id,"targetClientIds":[self.owner],
                "params":{"hostId":"local","conversationId":self.thread_id,"following":following}
            }),
            None,
        )
        .await?;
        self.following = following;
        Ok(())
    }

    async fn fresh_snapshot(&mut self) -> anyhow::Result<Value> {
        self.snapshot = None;
        self.following(true).await?;
        let timeout = self.timeout;
        tokio::time::timeout(timeout, async {
            for _ in 0..MAX_IGNORED_FRAMES {
                let message = self.read_frame().await?;
                self.observe(&message).await?;
                if let Some(snapshot) = self.snapshot.take() {
                    return Ok(snapshot);
                }
            }
            bail!("桌面 IPC 没有返回当前会话快照")
        })
        .await
        .context("读取桌面会话快照超时")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "main-thread";
    const OWNER: &str = "owner-client";
    const CLIENT: &str = "remote-client";

    fn snapshot() -> Value {
        json!({
            "id":THREAD,"sessionId":THREAD,"hostId":"local","source":"vscode",
            "threadSource":"user","ephemeral":false,"sideConversation":false,
            "parentThreadId":null,"forkedFromId":null,"resumeState":"resumed",
            "threadRuntimeStatus":{"type":"idle"},"requests":[],"threadGoal":null,
            "turnHistory":{"kind":"canonical","history":{"isComplete":true,"entitiesByKey":{}}}
        })
    }

    async fn read_peer<S: AsyncRead + Unpin>(stream: &mut S) -> anyhow::Result<Value> {
        let length = stream.read_u32_le().await? as usize;
        ensure!(
            (1..=MAX_FRAME_BYTES).contains(&length),
            "invalid test frame"
        );
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn write_peer<S: AsyncWrite + Unpin>(stream: &mut S, value: Value) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(&value)?;
        stream.write_u32_le(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        Ok(())
    }

    #[test]
    fn owner_tool_status_uses_completion_dictionary_and_times_instead_of_requested_duration() {
        let sleep = json!({
            "id":"sleep-tool","type":"sleep","durationMs":15000,"startedAtMs":1793490014982u64
        });
        let turn = json!({
            "turnId":"turn","status":"inProgress","completedItemIds":{},"items":[sleep]
        });
        let active = normalize_owner_turn(turn.clone());
        assert_eq!(active["items"][0]["status"], "inProgress");
        assert_eq!(active["items"][0]["remoteItemCompleted"], false);
        assert_eq!(active["items"][0]["durationMs"], 15000);
        assert!(turn["items"][0].get("status").is_none());
        assert!(active["items"][0].get("completedAtMs").is_none());

        let mut by_id = turn.clone();
        by_id["completedItemIds"]["sleep-tool"] = json!(true);
        let completed = normalize_owner_turn(by_id);
        assert_eq!(completed["items"][0]["status"], "completed");
        assert_eq!(completed["items"][0]["remoteItemCompleted"], true);
        let mut by_time = turn;
        by_time["items"][0]["completedAtMs"] = json!(1793490029985u64);
        let completed = normalize_owner_turn(by_time);
        assert_eq!(completed["items"][0]["status"], "completed");
        assert_eq!(completed["items"][0]["startedAtMs"], 1793490014982u64);
        assert_eq!(completed["items"][0]["completedAtMs"], 1793490029985u64);
    }

    #[test]
    fn owner_tool_status_preserves_failed_and_interrupted_outcomes_and_rejects_unknown_success() {
        for item in [
            json!({"id":"tool","type":"commandExecution","status":"completed","exitCode":7}),
            json!({"id":"tool","type":"dynamicToolCall","success":false}),
            json!({"id":"tool","type":"mcpToolCall","result":{"isError":true,"content":[]}}),
            json!({"id":"tool","type":"customTool","error":{"message":"actual failure"}}),
        ] {
            let projected = normalize_owner_turn(json!({
                "status":"completed","completedItemIds":{"tool":true},"items":[item]
            }));
            assert_eq!(projected["items"][0]["status"], "failed");
            assert_eq!(projected["items"][0]["remoteItemCompleted"], true);
        }
        let interrupted = normalize_owner_turn(json!({
            "status":"interrupted","completedItemIds":{"tool":true},"items":[{
                "id":"tool","type":"commandExecution","status":"interrupted","exitCode":130
            }]
        }));
        assert_eq!(interrupted["items"][0]["status"], "interrupted");
        assert_eq!(interrupted["items"][0]["exitCode"], 130);
        for status in ["completed", "failed", "interrupted"] {
            let unknown = normalize_owner_turn(json!({
                "status":status,"completedItemIds":{},"items":[{
                    "id":"tool","type":"sleep","durationMs":1,"startedAtMs":1000
                }]
            }));
            assert_eq!(unknown["items"][0]["status"], "unknown");
            assert_eq!(unknown["items"][0]["remoteItemCompleted"], false);
        }
        let malformed_ids = normalize_owner_turn(json!({
            "status":"inProgress","completedItemIds":["tool"],"items":[{"id":"tool","type":"sleep"}]
        }));
        assert_eq!(malformed_ids["items"][0]["status"], "unknown");
        assert_eq!(malformed_ids["items"][0]["remoteItemCompleted"], false);
        let explicit = normalize_owner_turn(json!({
            "status":"inProgress","items":[{
                "id":"tool","type":"customTool","status":{"type":"inProgress"}
            }]
        }));
        assert_eq!(explicit["items"][0]["status"], "inProgress");
    }

    fn watch_broadcast(snapshot: Value, revision: u64) -> Value {
        json!({
            "type":"broadcast","method":"thread-stream-state-changed","version":11,
            "sourceClientId":OWNER,"targetClientIds":[CLIENT],
            "params":{"conversationId":THREAD,"hostId":"local",
                "change":{"type":"snapshot","revision":revision,"conversationState":snapshot}}
        })
    }

    async fn watch_peer_handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) {
        for method in ["initialize", "thread-owner-discovery"] {
            let message = read_peer(stream).await.unwrap();
            assert_eq!(message["method"], method);
            let (owner, result) = if method == "initialize" {
                (CLIENT, json!({"clientId":CLIENT}))
            } else {
                (OWNER, json!({"supportsUntrustedAppInput":true}))
            };
            write_peer(
                stream,
                json!({
                    "type":"response","method":method,"requestId":message["requestId"],
                    "resultType":"success","handledByClientId":owner,"result":result
                }),
            )
            .await
            .unwrap();
        }
        let following = read_peer(stream).await.unwrap();
        assert_eq!(following["method"], "thread-stream-following-changed");
        assert_eq!(following["params"]["following"], true);
        write_peer(stream, watch_broadcast(snapshot(), 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn owner_watch_projects_full_states_and_refreshes_patches_without_writing_turns() {
        let (client, mut peer) = tokio::io::duplex(128 * 1024);
        let peer_task = tokio::spawn(async move {
            watch_peer_handshake(&mut peer).await;
            let mut active = snapshot();
            active["threadRuntimeStatus"] = json!({"type":"active"});
            write_peer(&mut peer, watch_broadcast(active, 2))
                .await
                .unwrap();
            let mut patches = watch_broadcast(Value::Null, 3);
            patches["params"]["change"] = json!({
                "type":"patches","revision":3,
                "patches":[{"op":"unsupported-operation-is-never-guessed"}]
            });
            write_peer(&mut peer, patches).await.unwrap();
            let following = read_peer(&mut peer).await.unwrap();
            assert_eq!(following["method"], "thread-stream-following-changed");
            assert_eq!(following["params"]["following"], true);
            let mut completed = snapshot();
            completed["turns"] = json!([{
                "turnId":"observed","status":"completed","items":[
                    {"id":"answer","type":"agentMessage","text":"实际完整状态"}
                ]
            }]);
            completed["turnHistory"] = Value::Null;
            write_peer(&mut peer, watch_broadcast(completed, 4))
                .await
                .unwrap();
            // Consumer shutdown closes this owned stream. It never starts or
            // interrupts a turn, nor terminates the desktop process.
            assert!(read_peer(&mut peer).await.is_err());
        });
        let (changes, mut observations) = mpsc::channel(8);
        let watcher = tokio::spawn(async move { watch_with_stream(client, THREAD, changes).await });
        let first = observations.recv().await.unwrap();
        assert_eq!(first.snapshot["threadRuntimeStatus"]["type"], "idle");
        let second = observations.recv().await.unwrap();
        assert_eq!(second.snapshot["threadRuntimeStatus"]["type"], "active");
        let third = observations.recv().await.unwrap();
        assert_eq!(
            third.snapshot["turns"][0]["items"][0]["text"],
            "实际完整状态"
        );
        drop(observations);
        tokio::time::timeout(Duration::from_secs(1), watcher)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), peer_task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn owner_watch_rejects_foreign_or_version_mismatched_updates_and_owner_disconnect() {
        for invalid in [
            json!({"sourceClientId":"other-owner"}),
            json!({"targetClientIds":["other-client"]}),
            json!({"version":10}),
            json!({"type":"broadcast","method":"client-status-changed",
                "params":{"clientId":OWNER,"status":"disconnected"}}),
        ] {
            let (client, mut peer) = tokio::io::duplex(128 * 1024);
            let peer_task = tokio::spawn(async move {
                watch_peer_handshake(&mut peer).await;
                let mut message = watch_broadcast(snapshot(), 2);
                message
                    .as_object_mut()
                    .unwrap()
                    .extend(invalid.as_object().unwrap().clone());
                write_peer(&mut peer, message).await.unwrap();
                // Keep the stream alive until the watcher explicitly rejects it.
                assert!(read_peer(&mut peer).await.is_err());
            });
            let (changes, mut observations) = mpsc::channel(8);
            let watcher =
                tokio::spawn(async move { watch_with_stream(client, THREAD, changes).await });
            assert_eq!(observations.recv().await.unwrap().snapshot["id"], THREAD);
            assert!(
                tokio::time::timeout(Duration::from_secs(1), watcher)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
            assert!(observations.recv().await.is_none());
            peer_task.await.unwrap();
        }
    }

    #[derive(Debug)]
    struct AuthorizationMarker;
    impl std::fmt::Display for AuthorizationMarker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("authorization-marker")
        }
    }
    impl std::error::Error for AuthorizationMarker {}
    struct DeniedBeforePermit;
    #[async_trait]
    impl ServiceDispatchGuard for DeniedBeforePermit {
        async fn prepare_dispatch(&self) -> anyhow::Result<Box<dyn ServiceDispatchPermit>> {
            Err(AuthorizationMarker.into())
        }
    }

    #[tokio::test]
    async fn authorization_failure_keeps_its_original_error_type_for_http_mapping() {
        let (sender, _receiver) = mpsc::channel(1);
        let error = guarded_write(
            &sender,
            vec![1],
            Some(&DeniedBeforePermit),
            Duration::from_millis(20),
        )
        .await
        .unwrap_err();
        assert!(error.downcast_ref::<AuthorizationMarker>().is_some());
        let failure = error.downcast_ref::<DispatchFailure>().unwrap();
        assert!(!failure.dispatched);
    }

    #[test]
    fn persistent_main_qualification_rejects_nested_subagents_temporary_and_unknown_sources() {
        assert!(require_main_thread(&snapshot(), THREAD).is_ok());
        for (field, value) in [
            (
                "source",
                json!({"subAgent":{"thread_spawn":{"parent_thread_id":"parent"}}}),
            ),
            ("threadSource", Value::Null),
            ("threadSource", json!("subagent")),
            ("source", json!("unknown")),
            ("ephemeral", json!(true)),
            ("parentThreadId", json!("parent")),
            ("sideConversation", json!(true)),
            ("hostId", json!("another-host")),
        ] {
            let mut value_to_check = snapshot();
            value_to_check[field] = value;
            assert!(
                require_main_thread(&value_to_check, THREAD).is_err(),
                "{field}"
            );
        }
        let mut unloaded_main = snapshot();
        unloaded_main["canAcceptDirectInput"] = Value::Null;
        assert!(require_main_thread(&unloaded_main, THREAD).is_ok());
    }

    #[tokio::test]
    async fn harness_subagent_owner_snapshot_reads_without_enabling_write_qualification() {
        let mut child = snapshot();
        child["cwd"] = json!("test");
        child["parentThreadId"] = json!("root-thread");
        child["threadSource"] = json!("subagent");
        child["ephemeral"] = json!(true);
        child["canAcceptDirectInput"] = json!(false);
        child["turnHistory"] = Value::Null;
        child["turnsPagination"] = json!({"hasLoadedOldest":true});
        child["turns"] = json!([{"turnId":"child-turn","status":"completed","items":[
            {"id":"answer","type":"agentMessage","text":"临时子智能体的桌面快照"}]}]);
        assert!(require_view_snapshot(&child, THREAD, true).is_ok());
        assert!(require_view_snapshot(&child, THREAD, false).is_err());
        assert!(require_main_thread(&child, THREAD).is_err());
        for (field, value) in [
            ("id", json!("another-thread")),
            ("sideConversation", json!(true)),
            ("hostId", json!("remote-host")),
            ("parentThreadId", json!(THREAD)),
        ] {
            let mut invalid = child.clone();
            invalid[field] = value;
            assert!(
                require_view_snapshot(&invalid, THREAD, true).is_err(),
                "{field}"
            );
        }
        let (client, mut peer) = tokio::io::duplex(128 * 1024);
        let expected = child.clone();
        let peer_task = tokio::spawn(async move {
            for method in ["initialize", "thread-owner-discovery"] {
                let message = read_peer(&mut peer).await.unwrap();
                assert_eq!(message["method"], method);
                let (owner, result) = if method == "initialize" {
                    (CLIENT, json!({"clientId":CLIENT}))
                } else {
                    (OWNER, json!({"supportsUntrustedAppInput":true}))
                };
                write_peer(
                    &mut peer,
                    json!({
                        "type":"response","method":method,"requestId":message["requestId"],
                        "resultType":"success","handledByClientId":owner,"result":result
                    }),
                )
                .await
                .unwrap();
            }
            let following = read_peer(&mut peer).await.unwrap();
            assert_eq!(following["method"], "thread-stream-following-changed");
            assert_eq!(following["params"]["following"], true);
            write_peer(&mut peer, watch_broadcast(child, 1))
                .await
                .unwrap();
            let following = read_peer(&mut peer).await.unwrap();
            assert_eq!(following["method"], "thread-stream-following-changed");
            assert_eq!(following["params"]["following"], false);
        });
        let observed = read_snapshot_with_stream(client, THREAD, true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed, expected);
        let history = crate::codex_service::owner_snapshot_history(&observed, THREAD).unwrap();
        assert_eq!(
            history["data"][0]["items"][0]["text"],
            "临时子智能体的桌面快照"
        );
        peer_task.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_frame_length_is_rejected_before_allocating_payload() {
        let (client, mut server) = tokio::io::duplex(32);
        let mut session = Session::new(client, THREAD, RPC_TIMEOUT);
        server
            .write_u32_le((MAX_FRAME_BYTES + 1) as u32)
            .await
            .unwrap();
        assert!(
            session
                .read_frame()
                .await
                .unwrap_err()
                .to_string()
                .contains("大小")
        );
    }
}
