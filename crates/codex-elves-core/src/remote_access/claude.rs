//! Claude 会话只转交原版 CLI 的输入输出；账号、工具和上下文由指定 WSL 中的 Claude 管理。
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use anyhow::Context;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, mpsc, oneshot};

use super::{ApiError, RemoteAccessController, RemoteAuthorization, now_ms};
use crate::desktop_owner_ipc::{WriteFrame, guarded_write, spawn_writer};

#[path = "claude_queue.rs"]
mod remote_queue;
use remote_queue::RemoteQueue;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
// 同一会话类型实例的完整模型目录缓存五分钟；新建与历史会话共用此期限。
const MODEL_CATALOG_TTL: Duration = Duration::from_secs(300);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_SHARED_FRAME_BYTES: usize = 64 * 1024 * 1024;

fn error(code: u16, message: impl Into<String>) -> anyhow::Error {
    ApiError::new(code, message.into()).into()
}

pub(super) fn api_error(failure: anyhow::Error) -> anyhow::Error {
    if failure.downcast_ref::<ApiError>().is_some() {
        failure
    } else {
        let unknown = failure
            .downcast_ref::<crate::desktop_owner_ipc::DispatchFailure>()
            .is_some_and(|failure| failure.dispatched);
        let error = ApiError::new(503, format!("Claude 服务操作失败：{failure}"));
        if unknown {
            error.after_admission().into()
        } else {
            error.into()
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct WslConfig {
    pub distribution: String,
    pub user: String,
    pub executable: String,
}

impl Default for WslConfig {
    fn default() -> Self {
        // 缺少显式配置时使用既有默认实例；已选实例和已创建会话始终保留自己的环境绑定。
        Self {
            distribution: "codex-elves-claude-test".into(),
            user: "elves-test".into(),
            executable: "/home/elves-test/.local/bin/claude".into(),
        }
    }
}

impl WslConfig {
    pub fn instance_id(&self) -> String {
        super::instances::id("claude", &self.distribution, &self.user)
    }

    fn validate(&self) -> anyhow::Result<()> {
        // 所有发行版共用名称、用户和程序路径校验，不增加按名称区分的产品权限。
        anyhow::ensure!(
            super::preferences::valid_distribution_name(&self.distribution)
                && !self.user.is_empty()
                && !self.user.contains('\0')
                && self.executable.starts_with('/')
                && !self.executable.contains('\0'),
            "Claude WSL 配置无效"
        );
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Conversation {
    #[serde(default = "claude_backend")]
    backend: String,
    id: String,
    native_id: String,
    project_id: Value,
    cwd: String,
    linux_cwd: String,
    config: WslConfig,
    title: String,
    model: String,
    effort: String,
    created_at: u64,
    updated_at: u64,
    pinned: bool,
    unread: bool,
    deleted: bool,
    resumed: bool,
    turns: Vec<Value>,
    #[serde(default)]
    shared: bool,
    #[serde(default)]
    native_job_id: String,
    #[serde(default)]
    native_status: Value,
    // 用户要求重连与首次读取仍显示原生最后请求用量；旧存档缺字段时保持未知，不填固定容量。
    #[serde(default)]
    token_usage: Value,
    #[serde(default)]
    native_clients: BTreeMap<String, String>,
    #[serde(default)]
    queued_messages: Vec<Value>,
    #[serde(default)]
    remote_queue: RemoteQueue,
    // 用户要求原生发现与网页创建区分：发现项重启/读历史只读，首次用户发送才接入原任务；本地标题修改不能被扫描覆盖。
    #[serde(default)]
    discovered: bool,
    #[serde(default)]
    native_title: String,
}

fn claude_backend() -> String {
    "claude".into()
}

impl Conversation {
    fn active(&self) -> bool {
        // 原版已回复的澄清也可能标为 blocked；只按生成活动或未完成轮次显示执行中，保留真正的工具等待。
        self.shared && self.native_status["tempo"] == "active"
            || self
                .turns
                .last()
                .is_some_and(|t| t["status"] == "inProgress")
    }

    fn view(&self) -> Value {
        // 用户要求实例显示名统一简化；wslUser、实例 ID 和实际运行用户保持原用途。
        json!({"id":self.id,"backend":self.backend,"title":self.title,"cwd":self.cwd,
            "projectId":self.project_id,"model":self.model,"reasoningEffort":self.effort,
            "createdAt":self.created_at,"updatedAt":self.updated_at,"recencyAt":self.updated_at,
            "pinned":self.pinned,"hasUnreadTurn":self.unread,"hostId":"local","ephemeral":false,
            "status":{"type":if self.active() {"active"} else {"idle"}},
            "editableMessage":null,"queuedMessages":self.remote_queue.messages(&self.queued_messages),
            "queuedEdits":self.remote_queue.edits(),"goal":null,"tokenUsage":self.token_usage,
            "nativeId":self.native_id,"nativeJobId":self.native_job_id,
            "wslDistribution":self.config.distribution,
            "instanceId":self.config.instance_id(),
            "instanceLabel":super::instances::label("claude",&self.config.distribution,&self.config.user),
            "wslUser":self.config.user,
            "cliCommand":if self.shared && !self.native_job_id.is_empty() {
                format!("wsl.exe --distribution '{}' --user '{}' --exec '{}' attach {}",
                    self.config.distribution.replace('\'', "''"),
                    self.config.user.replace('\'', "''"),
                    self.config.executable.replace('\'', "''"), self.native_job_id)
            } else {String::new()},
            "capabilities":{"sideThreads":false,"subagents":false,"goalComposer":false,
                "messageEdit":false,"queue":self.shared,"queueSendNow":false,
                "remoteQueue":self.shared,"queueEditRecovery":self.shared,
                "queuedImageEditing":self.shared,"queuedImageRemoval":self.shared,"compact":false}})
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct Saved {
    conversations: BTreeMap<String, Conversation>,
}

struct Channel {
    live: AtomicBool,
    generation: String,
    pending: Mutex<HashMap<String, oneshot::Sender<anyhow::Result<Value>>>>,
}

struct Process {
    // 不让读取任务持有 Process，最后一个调用者释放后关闭 stdin 并回收自己启动的进程。
    _child: Mutex<Option<tokio::process::Child>>,
    writer: mpsc::Sender<WriteFrame>,
    channel: Arc<Channel>,
    initialization: Value,
    shared: bool,
}

impl Process {
    async fn write(
        &self,
        frame: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<()> {
        if !self.channel.live.load(Ordering::Acquire) {
            return Err(error(503, "Claude 进程已退出，消息尚未派发"));
        }
        let mut bytes = serde_json::to_vec(&frame)?;
        // 用户允许图片合计 12 MiB，Base64 已占 16 MiB，输入还需容纳正文与 JSON；
        // 普通输入沿用 HTTP 的 18 MiB 上限，共享初始化含已保存的待确认消息，与 64 MiB 存档/快照上限一致。
        let limit = if self.shared {
            MAX_SHARED_FRAME_BYTES
        } else {
            super::MAX_ACTION_BODY_BYTES
        };
        if bytes.len() > limit {
            return Err(error(413, "Claude 输入超过大小限制，消息尚未派发"));
        }
        bytes.push(b'\n');
        guarded_write(
            &self.writer,
            bytes,
            authorization.map(|a| a as &dyn crate::codex_service::ServiceDispatchGuard),
            WRITE_TIMEOUT,
        )
        .await
    }

    async fn control(
        &self,
        request: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.channel.pending.lock().await.insert(id.clone(), sender);
        let response = async {
            self.write(
                json!({"type":"control_request","request_id":id,"request":request}),
                authorization,
            )
            .await?;
            tokio::time::timeout(INITIALIZE_TIMEOUT, receiver)
                .await
                .map_err(|_| {
                    ApiError::new(503, "Claude 控制请求已派发，回执等待超时").after_admission()
                })?
                .map_err(|_| {
                    ApiError::new(503, "Claude 控制请求已派发，连接已关闭").after_admission()
                })?
        }
        .await;
        self.channel.pending.lock().await.remove(&id);
        response
    }
}

#[derive(Default)]
struct DiscoveryState {
    config: Option<WslConfig>,
    process: Option<Arc<Process>>,
    checked: Option<Instant>,
}

pub(super) struct Claude {
    directory: PathBuf,
    discovery: Mutex<HashMap<String, Arc<Mutex<DiscoveryState>>>>,
    saved: Mutex<Saved>,
    processes: Mutex<HashMap<String, Arc<Process>>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    metadata: Mutex<HashMap<String, (Instant, Value)>>,
    requests: Mutex<HashMap<String, BTreeMap<String, Value>>>,
    observed: Mutex<HashMap<String, String>>,
    controller: OnceLock<Weak<RemoteAccessController>>,
    epoch: String,
    request_revision: AtomicU64,
}

impl Claude {
    pub fn new(directory: &Path) -> anyhow::Result<Arc<Self>> {
        let path = directory.join("remote-claude-sessions.json");
        let mut saved: Saved = match std::fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Claude 会话记录损坏，未覆盖原文件")?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => return Err(e.into()),
        };
        // 服务重启后不自动重发、续跑或批准；原生历史仍由 Claude 的 --resume 读取。
        for conversation in saved.conversations.values_mut() {
            anyhow::ensure!(
                conversation.backend == "claude" && Self::owns_id(&conversation.id),
                "Claude 会话类型或身份无效，未改写记录"
            );
            conversation.remote_queue.recover();
            if !conversation.shared
                && let Some(turn) = conversation.turns.last_mut()
                && turn["status"] == "inProgress"
            {
                turn["status"] = json!("interrupted");
                turn["error"] = json!({"message":"服务曾重启，请核对历史后继续；未自动重发消息"});
            }
        }
        Ok(Arc::new(Self {
            directory: directory.to_owned(),
            discovery: Mutex::new(HashMap::new()),
            saved: Mutex::new(saved),
            processes: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            metadata: Mutex::new(HashMap::new()),
            requests: Mutex::new(HashMap::new()),
            observed: Mutex::new(HashMap::new()),
            controller: OnceLock::new(),
            epoch: format!("claude-{}", uuid::Uuid::new_v4()),
            request_revision: AtomicU64::new(0),
        }))
    }

    pub fn attach(self: &Arc<Self>, controller: Weak<RemoteAccessController>) {
        let _ = self.controller.set(controller);
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        // 共享会话由原版后台进程继续运行；网页服务恢复时只重新连接，不重发用户消息。
        let service = Arc::downgrade(self);
        runtime.spawn(async move {
            let Some(service) = service.upgrade() else {
                return;
            };
            let conversations: Vec<_> = service
                .saved
                .lock()
                .await
                .conversations
                .values()
                .filter(|c| c.shared && !c.deleted && c.resumed && !c.discovered)
                .cloned()
                .collect();
            for conversation in conversations {
                let lock = service.lock(&conversation.id).await;
                let _guard = lock.lock().await;
                if let Ok(process) = service.spawn(&conversation).await {
                    service
                        .processes
                        .lock()
                        .await
                        .insert(conversation.id, process.clone());
                    // 连接登记完成后再补齐一次，避免空闲会话的首个快照早于进程登记而丢失。
                    let _ = process
                        .control(json!({"subtype":"shared_sync"}), None)
                        .await;
                }
            }
        });
        let service = Arc::downgrade(self);
        runtime.spawn(async move {
            // 用户改为远程服务调度：750ms 只检查已确认空闲的会话，每次最多投递一条，接收流独立运行。
            let mut tick = tokio::time::interval(Duration::from_millis(750));
            loop {
                tick.tick().await;
                let Some(service) = service.upgrade() else {
                    break;
                };
                service.drain_remote_queues().await;
            }
        });
    }

    #[cfg(any(test, debug_assertions))]
    pub async fn release(&self) {
        self.processes.lock().await.clear();
        self.discovery.lock().await.clear();
        self.observed.lock().await.clear();
        // 等读取任务收到 stdin 关闭并回收本后端启动的 WSL 进程。
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    pub fn wsl_distribution(&self) -> Option<String> {
        // 用户要求显示实际 WSL 系统名称；只读显式配置，不通过启动发行版或 Claude 获取展示名称。
        self.config().ok().map(|config| config.distribution)
    }

    pub(super) fn config(&self) -> anyhow::Result<WslConfig> {
        let config: WslConfig = match std::fs::read(self.directory.join("remote-claude-wsl.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Claude WSL 配置格式无效")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => WslConfig::default(),
            Err(e) => return Err(e.into()),
        };
        config.validate()?;
        Ok(config)
    }

    pub async fn instance_id(&self, thread_id: &str) -> anyhow::Result<String> {
        Ok(self.conversation(thread_id).await?.config.instance_id())
    }

    pub async fn add_known_instances(&self, instances: &mut Vec<super::instances::Instance>) {
        // 检测失败或暂时卸载不能丢失旧实例开关；保留归属，禁用新建，已有会话仍使用保存的配置。
        let default = self.config().ok().map(|config| config.instance_id());
        for c in self
            .saved
            .lock()
            .await
            .conversations
            .values()
            .filter(|c| !c.deleted)
        {
            if c.config.validate().is_err()
                || instances
                    .iter()
                    .any(|value| value.id == c.config.instance_id())
            {
                continue;
            }
            instances.push(super::instances::Instance {
                id: c.config.instance_id(),
                backend: "claude".into(),
                // 历史保留实例与已检测实例共用简化显示名，用户信息仍用于执行路由。
                label: super::instances::label("claude", &c.config.distribution, &c.config.user),
                environment_id: format!("wsl:{}", c.config.distribution),
                distribution: c.config.distribution.clone(),
                user: c.config.user.clone(),
                executable: c.config.executable.clone(),
                ready: false,
                status: "已有会话，环境暂未检测到".into(),
                legacy_default: default.as_deref() == Some(c.config.instance_id().as_str()),
                connection_modes: vec![],
            });
        }
    }

    async fn discovery_state(
        &self,
        config: &WslConfig,
    ) -> anyhow::Result<Arc<Mutex<DiscoveryState>>> {
        config.validate()?;
        let key = serde_json::to_string(config)?;
        Ok(self.discovery.lock().await.entry(key).or_default().clone())
    }

    fn wsl(config: &WslConfig) -> anyhow::Result<Command> {
        anyhow::ensure!(cfg!(windows), "当前 Claude 接入需要 Windows WSL2");
        config.validate()?;
        let mut command = Command::new("wsl.exe");
        command.args([
            "--distribution",
            &config.distribution,
            "--user",
            &config.user,
        ]);
        #[cfg(windows)]
        command.creation_flags(crate::windows_create_hidden_process());
        command.kill_on_drop(true);
        Ok(command)
    }

    async fn linux_cwd(config: &WslConfig, cwd: &str) -> anyhow::Result<String> {
        if let Some(distribution) = super::preferences::wsl_distribution(Path::new(cwd)) {
            anyhow::ensure!(
                distribution.eq_ignore_ascii_case(&config.distribution),
                "所选项目属于其他 WSL 实例，请选择同一环境中的项目"
            );
        }
        let mut command = Self::wsl(config)?;
        if cwd.is_empty() {
            // 无项目会话也使用同一测试用户的目录，不继承管理器或 WSL 默认目录。
            command.args(["--exec", "printenv", "HOME"]);
        } else {
            command.args(["--exec", "wslpath", "-a", "-u", cwd]);
        }
        let output = tokio::time::timeout(WRITE_TIMEOUT, command.output())
            .await
            .context("解析 WSL 项目目录超时")??;
        anyhow::ensure!(output.status.success(), "无法访问配置的 Claude WSL 环境");
        let path = String::from_utf8(output.stdout)?.trim().to_owned();
        anyhow::ensure!(
            path.starts_with('/') && !path.contains('\n'),
            "WSL 返回无效项目目录"
        );
        Ok(path)
    }

    async fn lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .await
            .entry(id.to_owned())
            .or_default()
            .clone()
    }

    fn persist(&self, saved: &Saved) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(saved)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "Claude 会话记录已达 64 MiB，未截断历史"
        );
        crate::settings::atomic_write(&self.directory.join("remote-claude-sessions.json"), &bytes)
    }

    async fn change<T>(
        &self,
        change: impl FnOnce(&mut Saved) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut saved = self.saved.lock().await;
        let mut next = saved.clone();
        let result = change(&mut next)?;
        self.persist(&next)?;
        *saved = next;
        Ok(result)
    }

    fn publish(&self, event: Value) {
        if let Some(controller) = self.controller.get().and_then(Weak::upgrade) {
            controller.publish_backend_event("claude", event);
        }
    }

    async fn conversation(&self, id: &str) -> anyhow::Result<Conversation> {
        self.saved
            .lock()
            .await
            .conversations
            .get(id)
            .filter(|c| !c.deleted)
            .cloned()
            .ok_or_else(|| error(404, "Claude 会话不存在或已删除"))
    }

    pub fn owns_id(id: &str) -> bool {
        id.strip_prefix("claude-")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    }

    pub async fn workspace(&self, id: &str) -> anyhow::Result<Value> {
        let c = self.conversation(id).await?;
        Ok(
            json!({"ok":true,"threadId":id,"cwd":c.cwd,"linuxCwd":c.linux_cwd,
            "wslDistribution":c.config.distribution,"instanceId":c.config.instance_id(),"hostId":"local","backend":"claude"}),
        )
    }

    async fn native_reader(self: &Arc<Self>, config: &WslConfig) -> anyhow::Result<Arc<Process>> {
        // 配置仅来自服务端检测或已保存会话；按实例缓存读取器，不能复用另一发行版的进程。
        let state = self.discovery_state(config).await?;
        let mut state = state.lock().await;
        if state.config.as_ref() != Some(config) {
            state.config = Some(config.clone());
            state.process = None;
            state.checked = None;
        }
        if let Some(process) = state
            .process
            .as_ref()
            .filter(|process| process.channel.live.load(Ordering::Acquire))
        {
            return Ok(process.clone());
        }
        // 用户要求发现只读：这里只启动 Python 文件读取连接，不执行 Claude、探测模型、恢复任务或复制网页登记文件。
        let path = self.directory.join("claude-shared.py");
        crate::settings::atomic_write(&path, include_bytes!("claude_shared.py"))?;
        let linux_path =
            Self::linux_cwd(config, path.to_str().context("Claude 连接端路径编码无效")?).await?;
        let mut command = Self::wsl(config)?;
        command.args([
            "--exec",
            "python3",
            "-u",
            &linux_path,
            "--cli",
            &config.executable,
            "--discover",
        ]);
        let probe = Conversation {
            backend: claude_backend(),
            id: String::new(),
            native_id: String::new(),
            project_id: Value::Null,
            cwd: String::new(),
            linux_cwd: String::new(),
            config: config.clone(),
            title: String::new(),
            model: String::new(),
            effort: String::new(),
            created_at: 0,
            updated_at: 0,
            pinned: false,
            unread: false,
            deleted: false,
            resumed: false,
            turns: vec![],
            shared: true,
            native_job_id: String::new(),
            native_status: Value::Null,
            token_usage: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            discovered: true,
            native_title: String::new(),
        };
        let process = self.launch(&probe, command, Some(json!({}))).await?;
        state.process = Some(process.clone());
        Ok(process)
    }

    pub async fn discover(self: &Arc<Self>, sidebar: &Value) -> anyhow::Result<()> {
        let config = self.config()?;
        self.discover_in(&config, sidebar).await
    }

    pub async fn discover_in(
        self: &Arc<Self>,
        config: &WslConfig,
        sidebar: &Value,
    ) -> anyhow::Result<()> {
        let lock = self
            .lock(&format!("native-discovery:{}", config.instance_id()))
            .await;
        let _guard = lock.lock().await;
        {
            let state = self.discovery_state(config).await?;
            let mut state = state.lock().await;
            if state.config.as_ref() == Some(config)
                && state
                    .checked
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
            {
                return Ok(());
            }
            if state.config.as_ref() != Some(config) {
                state.process = None;
                state.config = Some(config.clone());
            }
            // 用户要求普通列表高频刷新不能反复扫描：同一显式 WSL 最多十秒发现一次，失败也按相同间隔退避。
            state.checked = Some(Instant::now());
        }
        let process = self.native_reader(config).await?;
        let result = process.control(json!({"subtype":"discover"}), None).await?;
        let rows = result["sessions"]
            .as_array()
            .context("Claude 原生发现返回格式无效")?;
        // 用户要求首次网页发送前也同步 CLI 后续内容；只在原生正文时间/状态变化时补读已打开会话，避免轮询重发旧快照。
        let mut opened: Vec<_> = self
            .saved
            .lock()
            .await
            .conversations
            .values()
            .filter(|c| {
                c.config.instance_id() == config.instance_id()
                    && c.discovered
                    && !c.deleted
                    && !c.turns.is_empty()
                    && rows.iter().any(|row| {
                        row["nativeId"] == c.native_id
                            && (row["updatedAt"].as_u64().unwrap_or(0) > c.updated_at
                                || row["nativeStatus"] != c.native_status)
                    })
            })
            .map(|c| c.id.clone())
            .collect();
        self.register_native_sessions(config, rows, sidebar).await?;
        opened.extend(
            self.saved
                .lock()
                .await
                .conversations
                .values()
                .filter(|c| {
                    c.config.instance_id() == config.instance_id()
                        && c.discovered
                        && !c.deleted
                        && c.native_title.is_empty()
                        && c.turns.is_empty()
                })
                .map(|c| c.id.clone()),
        );
        for id in opened {
            let _ = self.sync_discovered(&id).await;
        }
        Ok(())
    }

    async fn register_native_sessions(
        &self,
        config: &WslConfig,
        rows: &[Value],
        sidebar: &Value,
    ) -> anyhow::Result<()> {
        let connected: std::collections::HashSet<_> = self
            .processes
            .lock()
            .await
            .iter()
            .filter(|(_, process)| process.channel.live.load(Ordering::Acquire))
            .map(|(id, _)| id.clone())
            .collect();
        let mut saved = self.saved.lock().await;
        let mut next = saved.clone();
        let mut changed = false;
        for row in rows {
            let Some(native) = row["nativeId"]
                .as_str()
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            else {
                continue;
            };
            let Some(linux_cwd) = row["cwd"].as_str().filter(|cwd| cwd.starts_with('/')) else {
                continue;
            };
            let (project_id, cwd) = native_project(sidebar, config, linux_cwd);
            let title = row["title"]
                .as_str()
                .filter(|title| !title.is_empty())
                .unwrap_or("Claude 会话");
            // 发行版+Linux 用户+原生 UUID 是登记键；删除墓碑也参与去重，不能靠再次扫描复活旧会话。
            let existing = next.conversations.values_mut().find(|c| {
                c.native_id == native
                    && c.config
                        .distribution
                        .eq_ignore_ascii_case(&config.distribution)
                    && c.config.user == config.user
            });
            if let Some(c) = existing {
                if c.deleted {
                    continue;
                }
                // 旧网页明确选择的“无项目”和手动标题必须保留；只有原生自动命名来源随目录补归属。
                if c.discovered
                    && !c.native_title.is_empty()
                    && c.project_id.is_null()
                    && !project_id.is_null()
                {
                    c.project_id = project_id;
                    c.cwd = cwd;
                    changed = true;
                }
                // 用户要求早期 print 登记也读取完整原生历史，保留原 web ID；不接管仍连接或执行中的进程。
                if !c.shared && !c.active() && !connected.contains(&c.id) {
                    c.shared = true;
                    c.resumed = true;
                    c.discovered = true;
                    c.turns.clear();
                    changed = true;
                }
                if c.discovered {
                    let automatic_title = !c.native_title.is_empty();
                    if automatic_title && c.title == c.native_title && c.title != title {
                        c.title = title.into();
                        changed = true;
                    }
                    if automatic_title && c.native_title != title {
                        c.native_title = title.into();
                        changed = true;
                    }
                    let at = row["updatedAt"].as_u64().unwrap_or(0);
                    if at > c.updated_at {
                        c.updated_at = at;
                        changed = true;
                    }
                    for (target, key) in [
                        (&mut c.model, "model"),
                        (&mut c.effort, "effort"),
                        (&mut c.native_job_id, "jobId"),
                    ] {
                        if let Some(value) = row[key]
                            .as_str()
                            .filter(|value| key == "jobId" || !value.is_empty())
                            && target.as_str() != value
                        {
                            *target = value.into();
                            changed = true;
                        }
                    }
                    if row["nativeStatus"].is_object() && c.native_status != row["nativeStatus"] {
                        c.native_status = row["nativeStatus"].clone();
                        changed = true;
                    }
                }
                continue;
            }
            let c = Conversation {
                backend: claude_backend(),
                id: format!("claude-{}", uuid::Uuid::new_v4()),
                native_id: native.into(),
                project_id,
                cwd,
                linux_cwd: linux_cwd.into(),
                config: config.clone(),
                title: title.into(),
                model: row["model"].as_str().unwrap_or("").into(),
                effort: row["effort"].as_str().unwrap_or("").into(),
                created_at: row["createdAt"].as_u64().unwrap_or(0),
                updated_at: row["updatedAt"].as_u64().unwrap_or(0),
                pinned: false,
                unread: false,
                deleted: false,
                resumed: true,
                turns: vec![],
                shared: true,
                native_job_id: row["jobId"].as_str().unwrap_or("").into(),
                native_status: row["nativeStatus"].clone(),
                token_usage: Value::Null,
                native_clients: BTreeMap::new(),
                queued_messages: vec![],
                remote_queue: RemoteQueue::default(),
                discovered: true,
                native_title: title.into(),
            };
            next.conversations.insert(c.id.clone(), c);
            changed = true;
        }
        if changed {
            self.persist(&next)?;
            *saved = next;
        }
        Ok(())
    }

    async fn sync_discovered(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;
        let c = self.conversation(id).await?;
        if !c.discovered
            || self
                .processes
                .lock()
                .await
                .get(id)
                .is_some_and(|p| p.channel.live.load(Ordering::Acquire))
        {
            return Ok(());
        }
        let process = self.native_reader(&c.config).await?;
        let mut pending = c.queued_messages.clone();
        pending.extend(c.remote_queue.native_intents());
        let frame = process
            .control(
                json!({"subtype":"read_history","native_id":c.native_id,
            "clients":c.native_clients,"pending":pending,"model":c.model}),
                None,
            )
            .await?;
        // 初次导入的旧历史不置未读；之后真正新增正文仍沿用已有判断。稳定代次使重启/重复读取不会回弹未读。
        self.apply_shared_snapshot(
            id,
            &format!("native-history-{}", c.native_id),
            &frame,
            json!({}),
            !c.turns.is_empty(),
        )
        .await
    }

    pub async fn merge_sidebar(&self, sidebar: &mut Value) {
        let ids: Vec<_> = self
            .config()
            .ok()
            .map(|config| config.instance_id())
            .into_iter()
            .collect();
        self.merge_sidebar_instances(sidebar, Some(&ids)).await;
    }

    pub async fn merge_sidebar_instances(&self, sidebar: &mut Value, enabled: Option<&[String]>) {
        let saved = self.saved.lock().await;
        let mut rows: Vec<_> = saved
            .conversations
            .values()
            .filter(|c| {
                !c.deleted && enabled.is_none_or(|ids| ids.contains(&c.config.instance_id()))
            })
            .map(Conversation::view)
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row["updatedAt"].as_u64().unwrap_or(0)));
        for row in rows {
            if let Some(recent) = sidebar["recent"].as_array_mut() {
                recent.push(row.clone());
            }
            if row["pinned"] == true
                && let Some(pins) = sidebar["pinnedThreads"].as_array_mut()
            {
                pins.push(row.clone());
            }
            if let Some(projects) = sidebar["projects"].as_array_mut()
                && let Some(project) = projects.iter_mut().find(|p| p["id"] == row["projectId"])
                && let Some(threads) = project["threads"].as_array_mut()
            {
                // 用户要求共享 Codex 项目及其原 ID；不根据 WSL 路径生成另一份项目。
                threads.push(row);
            }
        }
    }

    pub async fn detail(self: &Arc<Self>, id: &str) -> anyhow::Result<Value> {
        self.sync_discovered(id).await?;
        let c = self.conversation(id).await?;
        let mut thread = c.view();
        for (key, value) in c.queue_snapshot(&self.epoch).as_object().unwrap() {
            if key != "ok" && key != "threadId" {
                thread[key] = value.clone();
            }
        }
        let requests = self.requests.lock().await;
        thread["requests"] = json!(
            requests
                .get(id)
                .map(|r| r.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        );
        thread["requestsKnown"] = json!(true);
        thread["requestsEpoch"] = json!(self.epoch);
        thread["requestsRevision"] = json!(self.request_revision.load(Ordering::Acquire));
        Ok(json!({"ok":true,"thread":thread,"nativeBridge":"claude"}))
    }

    pub async fn history(self: &Arc<Self>, payload: &Value) -> anyhow::Result<Value> {
        let id = payload["threadId"]
            .as_str()
            .context("缺少 Claude 会话 ID")?;
        self.sync_discovered(id).await?;
        let c = self.conversation(id).await?;
        let end = match &payload["cursor"] {
            Value::Null => c.turns.len(),
            Value::String(value) => value
                .parse::<usize>()
                .ok()
                .filter(|n| *n <= c.turns.len())
                .ok_or_else(|| error(400, "Claude 历史游标无效"))?,
            _ => return Err(error(400, "Claude 历史游标无效")),
        };
        let start = end.saturating_sub(10);
        let turns: Vec<_> = c.turns[start..end].iter().rev().cloned().collect();
        Ok(json!({"ok":true,"threadId":id,"data":turns,
            // 用户要求修复中断假消息；HTTP 分页也提供完整有效 ID，旧缓存无需等待事件流才能退役错误轮次。
            "turnIds":c.turns.iter().map(|turn| &turn["id"]).collect::<Vec<_>>(),
            "nextCursor":if start == 0 {Value::Null} else {json!(start.to_string())},
            "transcriptEvents":[],"backend":"claude"}))
    }

    pub async fn queue(&self, id: &str) -> anyhow::Result<Value> {
        let conversation = self.conversation(id).await?;
        Ok(conversation.queue_snapshot(&self.epoch))
    }

    async fn spawn(self: &Arc<Self>, c: &Conversation) -> anyhow::Result<Arc<Process>> {
        if !c.shared {
            return self.spawn_stdio(c).await;
        }
        let probe = Conversation {
            id: String::new(),
            shared: false,
            resumed: false,
            native_id: uuid::Uuid::new_v4().to_string(),
            ..c.clone()
        };
        let catalog = self.spawn_stdio(&probe).await?.initialization.clone();
        let path = self.directory.join("claude-shared.py");
        crate::settings::atomic_write(&path, include_bytes!("claude_shared.py"))?;
        let linux_path = Self::linux_cwd(
            &c.config,
            path.to_str().context("Claude 连接端路径编码无效")?,
        )
        .await?;
        let mut command = Self::wsl(&c.config)?;
        command.args([
            "--cd",
            &c.linux_cwd,
            "--exec",
            "python3",
            "-u",
            &linux_path,
            "--cli",
            &c.config.executable,
        ]);
        if c.resumed {
            command.args(["--native-id", &c.native_id]);
            if !c.native_job_id.is_empty() {
                command.args(["--job-id", &c.native_job_id]);
            }
        }
        self.launch(c, command, Some(catalog)).await
    }

    async fn spawn_stdio(self: &Arc<Self>, c: &Conversation) -> anyhow::Result<Arc<Process>> {
        let mut command = Self::wsl(&c.config)?;
        // 用户要求保持原生系统提示、MCP、Skills、Hooks 和权限；不传 bare、替代提示或跳过审批参数。
        command.args([
            "--cd",
            &c.linux_cwd,
            "--exec",
            &c.config.executable,
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-prompt-tool",
            "stdio",
        ]);
        if c.resumed {
            command.args(["--resume", &c.native_id]);
        } else {
            command.args(["--session-id", &c.native_id]);
        }
        self.launch(c, command, None).await
    }

    async fn launch(
        self: &Arc<Self>,
        c: &Conversation,
        mut command: Command,
        shared_catalog: Option<Value>,
    ) -> anyhow::Result<Arc<Process>> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .context("无法启动指定 WSL 中的 Claude CLI")?;
        let (writer, _) = spawn_writer(
            child.stdin.take().context("Claude stdin 不可用")?,
            WRITE_TIMEOUT,
        );
        let stdout = child.stdout.take().context("Claude stdout 不可用")?;
        let stderr = child.stderr.take().context("Claude stderr 不可用")?;
        let channel = Arc::new(Channel {
            live: AtomicBool::new(true),
            generation: uuid::Uuid::new_v4().to_string(),
            pending: Mutex::new(HashMap::new()),
        });
        let read_channel = channel.clone();
        // 共享帧包含整段原生历史投影，与已有 64 MiB 会话存储上限一致；stdio 单消息仍为 16 MiB。
        let frame_limit = if shared_catalog.is_some() {
            MAX_SHARED_FRAME_BYTES
        } else {
            MAX_FRAME_BYTES
        };
        let weak = Arc::downgrade(self);
        let id = c.id.clone();
        let errors = Arc::new(Mutex::new(String::new()));
        let error_output = errors.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // stderr 只用于本次启动失败说明，不写包含用户配置或凭据的诊断日志。
                let mut output = error_output.lock().await;
                if output.len() < 8192 {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
        });
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            let outcome: anyhow::Result<()> = async {
                loop {
                    line.clear();
                    // 每个 JSON 帧限长；不能让异常进程在换行前无限占用内存。
                    let size = tokio::io::AsyncReadExt::take(&mut reader, (frame_limit + 1) as u64)
                        .read_until(b'\n', &mut line)
                        .await?;
                    if size == 0 {
                        break;
                    }
                    anyhow::ensure!(size <= frame_limit, "Claude 输出帧超过大小限制");
                    let frame: Value =
                        serde_json::from_slice(&line).context("Claude 输出不是有效 JSON")?;
                    if frame["type"] == "control_response" {
                        let response = &frame["response"];
                        if let Some(id) = response["request_id"].as_str()
                            && let Some(sender) = read_channel.pending.lock().await.remove(id)
                        {
                            let result = if response["subtype"] == "success" {
                                Ok(response["response"].clone())
                            } else {
                                let message = format!("Claude 控制请求失败：{}", response["error"]);
                                let failure = ApiError::new(
                                    if response["code"] == "EBUSY" {
                                        409
                                    } else {
                                        503
                                    },
                                    message,
                                );
                                Err(if response["not_sent"] == true {
                                    failure.into()
                                } else {
                                    failure.after_admission().into()
                                })
                            };
                            let _ = sender.send(result);
                        }
                    } else if let Some(service) = weak.upgrade() {
                        service.ingest(&id, &read_channel.generation, frame).await?;
                    } else {
                        break;
                    }
                }
                Ok(())
            }
            .await;
            read_channel.live.store(false, Ordering::Release);
            let detail = outcome
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Claude 进程已退出，请检查新测试环境中的安装和登录".into());
            // CLI 的 stderr 可能包含配置内容；不把原文传到远程页面或写入通用读取失败日志。
            for (_, sender) in read_channel.pending.lock().await.drain() {
                let _ = sender.send(Err(ApiError::new(503, detail.clone())
                    .after_admission()
                    .into()));
            }
            if let Some(service) = weak.upgrade() {
                let mut processes = service.processes.lock().await;
                if processes
                    .get(&id)
                    .is_some_and(|p| p.channel.generation == read_channel.generation)
                {
                    processes.remove(&id);
                }
                drop(processes);
                if let Ok(conversation) = service.conversation(&id).await
                    && !conversation.shared
                {
                    let _ = service
                        .finish(&id, Some(&detail), true, Some(&read_channel.generation))
                        .await;
                }
            }
        });
        let mut process = Process {
            _child: Mutex::new(Some(child)),
            writer,
            channel,
            initialization: Value::Null,
            shared: shared_catalog.is_some(),
        };
        process.initialization = process
            .control(if let Some(catalog) = shared_catalog {
                let mut pending = c.queued_messages.clone();
                pending.extend(c.remote_queue.native_intents());
                json!({"subtype":"initialize","catalog":catalog,"model":c.model,"effort":c.effort,
                    "clients":c.native_clients,"pending":pending})
            } else {json!({"subtype":"initialize"})}, None)
            .await?;
        Ok(Arc::new(process))
    }

    pub async fn models(self: &Arc<Self>, cwd: &str, force: bool) -> anyhow::Result<Value> {
        let config = self.config()?;
        self.models_in(config, cwd, force).await
    }

    pub async fn models_in(
        self: &Arc<Self>,
        config: WslConfig,
        cwd: &str,
        force: bool,
    ) -> anyhow::Result<Value> {
        config.validate()?;
        // 用户要求同一类型实例共用模型目录；项目及会话不参与缓存键，刷新仍沿用五分钟有效期。
        let key = config.instance_id();
        let probe_lock = self.lock(&format!("models:{key}")).await;
        let _probe_guard = probe_lock.lock().await;
        if !force
            && let Some((at, data)) = self.metadata.lock().await.get(&key)
            && at.elapsed() < MODEL_CATALOG_TTL
        {
            return Ok(data.clone());
        }
        let c = Conversation {
            backend: claude_backend(),
            id: String::new(),
            native_id: uuid::Uuid::new_v4().to_string(),
            project_id: Value::Null,
            cwd: cwd.into(),
            linux_cwd: Self::linux_cwd(&config, cwd).await?,
            config,
            title: String::new(),
            model: String::new(),
            effort: String::new(),
            created_at: now_ms(),
            updated_at: now_ms(),
            pinned: false,
            unread: false,
            deleted: false,
            resumed: false,
            turns: vec![],
            shared: false,
            native_job_id: String::new(),
            native_status: Value::Null,
            token_usage: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            discovered: false,
            native_title: String::new(),
        };
        // 用户要求模型选择读取完整原生目录；只做 stdio 初始化，不创建共享任务连接或发送消息。
        let process = self.spawn_stdio(&c).await?;
        let data = normalize_models(&process.initialization);
        anyhow::ensure!(
            data["data"].as_array().is_some_and(|m| !m.is_empty()),
            "Claude 未返回可用模型"
        );
        self.metadata
            .lock()
            .await
            .insert(key, (Instant::now(), data.clone()));
        Ok(data)
    }

    pub async fn session_models(
        self: &Arc<Self>,
        id: &str,
        include_available: bool,
        force: bool,
    ) -> anyhow::Result<Value> {
        let c = self.conversation(id).await?;
        let key = c.config.instance_id();
        if let Some((at, data)) = self.metadata.lock().await.get(&key)
            && (!include_available || !force && at.elapsed() < MODEL_CATALOG_TTL)
        {
            // 浏览历史只复用实例目录，不启动 CLI；显式打开选择器时按相同有效期核对完整选项。
            return Ok(data.clone());
        }
        let process = self
            .processes
            .lock()
            .await
            .get(id)
            .filter(|p| p.channel.live.load(Ordering::Acquire))
            .cloned();
        if let Some(process) = process {
            let data = normalize_models(&process.initialization);
            if data["data"].as_array().is_some_and(|rows| !rows.is_empty()) {
                self.metadata
                    .lock()
                    .await
                    .insert(key, (Instant::now(), data.clone()));
                return Ok(data);
            }
        }
        if c.discovered && !include_available {
            // 浏览历史及自动刷新保持只读；完整选项仅在用户打开模型选择器时初始化同一实例的 CLI。
            let efforts: Vec<_> = (!c.effort.is_empty())
                .then(|| json!({"reasoningEffort":c.effort}))
                .into_iter()
                .collect();
            let rows: Vec<_> = (!c.model.is_empty())
                .then(|| {
                    json!({"id":c.model,"model":c.model,
                "displayName":c.model,"supportedReasoningEfforts":efforts})
                })
                .into_iter()
                .collect();
            return Ok(
                json!({"ok":true,"backend":"claude","data":rows,"catalogComplete":false,
                "defaultModel":c.model,"defaultReasoningEffort":c.effort}),
            );
        }
        // 历史与新建共用同一实例目录及默认值，当前模型/等级仍由各会话自身保存。
        self.models_in(c.config.clone(), &c.cwd, force).await
    }

    pub async fn create(
        self: &Arc<Self>,
        payload: &Value,
        cwd: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let config = self.config()?;
        self.create_in(config, payload, cwd, authorization).await
    }

    pub async fn create_in(
        self: &Arc<Self>,
        config: WslConfig,
        payload: &Value,
        cwd: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        config.validate()?;
        let content = input_content(payload)?;
        let native_id = uuid::Uuid::new_v4().to_string();
        let standalone;
        let cwd = if cwd.is_empty() {
            standalone = self.directory.join("claude-workspaces").join(&native_id);
            std::fs::create_dir_all(&standalone)?;
            standalone.to_str().context("无项目工作目录编码无效")?
        } else {
            cwd
        };
        let mut c = Conversation {
            backend: claude_backend(),
            id: format!("claude-{}", uuid::Uuid::new_v4()),
            native_id,
            project_id: payload["projectId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map_or(Value::Null, |s| json!(s)),
            cwd: cwd.into(),
            linux_cwd: Self::linux_cwd(&config, cwd).await?,
            config,
            title: payload["prompt"]
                .as_str()
                .unwrap_or("图片会话")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(30)
                .collect(),
            model: String::new(),
            effort: String::new(),
            created_at: now_ms(),
            updated_at: now_ms(),
            pinned: false,
            unread: false,
            deleted: false,
            resumed: false,
            turns: vec![],
            // 用户要求继续共用原版 CLI 会话；待发送消息先由远程服务保存，实际投递时才进入原生会话。
            shared: true,
            native_job_id: String::new(),
            native_status: Value::Null,
            token_usage: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            discovered: false,
            native_title: String::new(),
        };
        let process = self.spawn(&c).await?;
        apply_model(&process, &mut c, payload, authorization).await?;
        authorization.check().await?;
        self.change(|saved| {
            saved.conversations.insert(c.id.clone(), c.clone());
            Ok(())
        })
        .await?;
        self.processes
            .lock()
            .await
            .insert(c.id.clone(), process.clone());
        self.publish(
            json!({"method":"thread/started","params":{"threadId":c.id,"thread":c.view()}}),
        );
        match self
            .start_turn(&c.id, &process, content, payload, authorization)
            .await
        {
            Ok(value) => Ok(value),
            Err(failure) => {
                // 创建记录与发送是两步；失败回执保留原会话 ID，不能让网页盲目重建另一条会话。
                Ok(
                    json!({"ok":false,"threadId":c.id,"status":"created","confirmed":true,
                    "outcomeUnknown":true,"message":format!("Claude 会话已登记，首条消息结果待核对：{failure}")}),
                )
            }
        }
    }

    pub async fn command(
        self: &Arc<Self>,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let id = payload["threadId"]
            .as_str()
            .context("缺少 Claude 会话 ID")?;
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;
        let mut c = self.conversation(id).await?;
        if payload
            .get("backend")
            .is_some_and(|backend| backend != "claude")
        {
            return Err(error(409, "已创建的 Claude 会话不能切换后端"));
        }
        match action {
            "send" => {
                if c.active() && !c.shared {
                    return Err(error(409, "请等待 Claude 完成本轮或先停止生成"));
                }
                let content = input_content(payload)?;
                let process = self
                    .processes
                    .lock()
                    .await
                    .get(id)
                    .filter(|p| p.channel.live.load(Ordering::Acquire))
                    .cloned();
                let process = match process {
                    Some(process) => process,
                    None => {
                        let process = self.spawn(&c).await?;
                        self.processes
                            .lock()
                            .await
                            .insert(id.into(), process.clone());
                        process
                    }
                };
                if c.shared {
                    return self
                        .enqueue_shared(id, &process, payload, authorization)
                        .await;
                }
                apply_model(&process, &mut c, payload, authorization).await?;
                self.change(|saved| {
                    // CLI 可在网页切换模型期间继续产生消息；只更新设置，不能用旧快照覆盖实时历史。
                    let current = saved
                        .conversations
                        .get_mut(id)
                        .context("Claude 会话不存在")?;
                    current.model = c.model;
                    current.effort = c.effort;
                    Ok(())
                })
                .await?;
                self.start_turn(id, &process, content, payload, authorization)
                    .await
            }
            "steerQueued" if c.shared => {
                // 用户要求选哪条只投递哪条；其余网页消息仍保存在服务端，不能再释放原生整队。
                let message_id = payload["messageId"].as_str().context("缺少消息标识")?;
                let process = self
                    .processes
                    .lock()
                    .await
                    .get(id)
                    .cloned()
                    .ok_or_else(|| error(409, "原生会话尚未连接，请稍后核对队列"))?;
                self.dispatch_remote(id, message_id, &process, true, authorization, false)
                    .await
            }
            "removeQueued" | "reorderQueued" | "beginQueuedEdit" | "editQueued"
            | "cancelQueuedEdit" | "dismissQueuedEdit"
                if c.shared =>
            {
                self.edit_remote_queue(id, action, payload, authorization)
                    .await
            }
            "stop" => {
                authorization.check().await?;
                self.change(|saved| {
                    saved
                        .conversations
                        .get_mut(id)
                        .context("Claude 会话不存在")?
                        .remote_queue
                        .pause();
                    Ok(())
                })
                .await?;
                self.remote_queue_changed(id).await;
                // 远程队列空闲时只暂停待发内容；不能把已完成回复改成“已停止”，也不向空闲 CLI 发送按键。
                if c.shared
                    && !c.active()
                    && c.native_status["tempo"] == "idle"
                    && c.native_status["needs"].is_null()
                {
                    return self.queue(id).await;
                }
                if let Some(process) = self.processes.lock().await.get(id).cloned() {
                    self.change(|saved| {
                        if let Some(turn) = saved
                            .conversations
                            .get_mut(id)
                            .and_then(|c| c.turns.last_mut())
                            .filter(|t| t["status"] == "inProgress")
                        {
                            turn["_stopRequested"] = json!(true);
                        }
                        Ok(())
                    })
                    .await?;
                    process
                        .control(json!({"subtype":"interrupt"}), Some(authorization))
                        .await?;
                } else {
                    self.finish(id, None, true, None).await?;
                }
                // 停止回执只确认请求已接受；等待原轮次的 result，避免迟到结果结束下一轮。
                Ok(json!({"ok":true,"threadId":id}))
            }
            "approve" | "answer" => self.respond(id, payload, authorization).await,
            "markRead" | "pinThread" | "rename" | "deleteThread" => {
                if action == "deleteThread" && c.active() {
                    return Err(error(409, "请先停止 Claude 当前任务后再删除会话"));
                }
                authorization.check().await?;
                self.change(|saved| {
                    let c = saved
                        .conversations
                        .get_mut(id)
                        .context("Claude 会话不存在")?;
                    match action {
                        "markRead" => c.unread = false,
                        "pinThread" => c.pinned = payload["pinned"] == true,
                        "rename" => {
                            let title = payload["title"].as_str().unwrap_or("").trim();
                            if title.is_empty() || title.chars().count() > 200 {
                                return Err(error(400, "标题需为 1–200 个字符"));
                            }
                            c.title = title.into();
                        }
                        "deleteThread" => {
                            c.deleted = true;
                            c.turns.clear();
                        }
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .await?;
                if action == "deleteThread" {
                    self.processes.lock().await.remove(id);
                    self.requests.lock().await.remove(id);
                    self.publish(json!({"method":"thread/deleted","params":{"threadId":id}}));
                } else {
                    self.publish(
                        json!({"method":"remote/sidebarChanged","params":{"threadId":id}}),
                    );
                }
                Ok(
                    json!({"ok":true,"threadId":id,"deleted":action=="deleteThread","status":"local_deleted"}),
                )
            }
            _ => Err(error(409, "此功能尚未适配 Claude 会话")),
        }
    }

    async fn start_turn(
        &self,
        id: &str,
        process: &Process,
        content: Vec<Value>,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if process.shared {
            return self
                .enqueue_shared(id, process, payload, authorization)
                .await;
        }
        let turn_id = uuid::Uuid::new_v4().to_string();
        let user_id = payload["clientUserMessageId"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let view_content: Vec<_> = content
            .iter()
            .map(|part| {
                if part["type"] == "image" {
                    json!({"type":"image","url":format!("data:{};base64,{}",
                    part["source"]["media_type"].as_str().unwrap_or(""),
                    part["source"]["data"].as_str().unwrap_or(""))})
                } else {
                    part.clone()
                }
            })
            .collect();
        let turn = json!({"id":turn_id,"status":"inProgress","itemsView":"full","itemsComplete":true,
            "_process":process.channel.generation,
            "startedAt":now_ms(),"turnStartedAtMs":now_ms(),
            "items":[{"type":"userMessage","id":user_id,"clientId":user_id,"content":view_content}]});
        self.change(|saved| {
            let c = saved
                .conversations
                .get_mut(id)
                .context("Claude 会话不存在")?;
            c.turns.push(turn.clone());
            c.updated_at = now_ms();
            // 发送前持久保存原生会话身份和派发意图；成功写入后不再因另一次落盘失败误报“未发送”。
            c.resumed = true;
            Ok(())
        })
        .await?;
        self.publish(json!({"method":"turn/started","params":{"threadId":id,"turn":turn}}));
        let native_id = self.conversation(id).await?.native_id;
        if let Err(failure) = process
            .write(
                json!({"type":"user","session_id":native_id,
            "uuid":user_id,"parent_tool_use_id":null,"message":{"role":"user","content":content}}),
                Some(authorization),
            )
            .await
        {
            let _ = self
                .finish(
                    id,
                    Some("消息派发结果待核对，未自动重发"),
                    true,
                    Some(&process.channel.generation),
                )
                .await;
            return Err(failure);
        }
        Ok(
            json!({"ok":true,"threadId":id,"confirmed":true,"turnId":turn_id,
            "clientUserMessageId":user_id,"executionTransport":"claude-wsl-stdio"}),
        )
    }

    async fn ingest(&self, id: &str, generation: &str, frame: Value) -> anyhow::Result<()> {
        if id.is_empty() {
            return Ok(());
        } // 模型能力探测没有用户会话，不产生列表记录。
        if frame["type"] == "shared_snapshot" {
            return self.ingest_shared(id, generation, &frame).await;
        }
        if frame["type"] == "shared_preview" {
            return self.ingest_shared_preview(id, generation, &frame).await;
        }
        if frame["type"] == "shared_delivery_intent" {
            if self
                .processes
                .lock()
                .await
                .get(id)
                .is_some_and(|p| p.channel.generation == generation)
            {
                self.change(|saved| {
                    saved
                        .conversations
                        .get_mut(id)
                        .context("Claude 会话不存在")?
                        .remote_queue
                        .remember_intent(&frame["intent"]);
                    Ok(())
                })
                .await?;
            }
            return Ok(());
        }
        // 每轮绑定实际进程代次；退出或重连中的旧进程不能修改新轮次、审批和正文。
        if !self
            .saved
            .lock()
            .await
            .conversations
            .get(id)
            .and_then(|c| c.turns.last())
            .is_some_and(|t| t["status"] == "inProgress" && t["_process"] == generation)
        {
            return Ok(());
        }
        if frame["type"] == "control_request" {
            let request_id = frame["request_id"].as_str().context("Claude 请求缺少 ID")?;
            let request = permission_request(id, request_id, &frame["request"])?;
            self.requests
                .lock()
                .await
                .entry(id.into())
                .or_default()
                .insert(request_id.into(), request);
            self.requests_changed(id).await;
            return Ok(());
        }
        if frame["type"] == "control_cancel_request" {
            if let Some(requests) = self.requests.lock().await.get_mut(id) {
                requests.remove(frame["request_id"].as_str().unwrap_or(""));
            }
            self.requests_changed(id).await;
            return Ok(());
        }
        if frame["type"] == "result" {
            // SDK 的 modelUsage.contextWindow 是原生报告；只补最后请求所属模型的容量，不拿结果累计 tokens 替换请求用量。
            let usage = self
                .change(|saved| {
                    let Some(c) = saved.conversations.get_mut(id).filter(|c| !c.deleted) else {
                        return Ok(None);
                    };
                    let model = c.token_usage["model"].as_str().unwrap_or("");
                    let (capacity, source) =
                        native_context_capacity(model, &json!({"modelUsage":frame["modelUsage"]}));
                    let Some(capacity) = capacity else {
                        return Ok(None);
                    };
                    let previous = c.token_usage.clone();
                    c.token_usage["modelContextWindow"] = json!(capacity);
                    c.token_usage["modelContextWindowSource"] = source;
                    if let Some(usage) = c.token_usage.as_object_mut() {
                        usage.remove("usedPercentage");
                    }
                    Ok((previous != c.token_usage).then(|| c.token_usage.clone()))
                })
                .await?;
            if let Some(usage) = usage {
                self.publish(json!({"method":"thread/tokenUsage/updated","params":{"threadId":id,"tokenUsage":usage}}));
            }
            let failure = (frame["is_error"] == true).then(|| {
                frame["result"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| frame["errors"].to_string())
            });
            return self
                .finish(id, failure.as_deref(), false, Some(generation))
                .await;
        }
        let usage = if frame["type"] == "assistant" {
            let initialization = self
                .processes
                .lock()
                .await
                .get(id)
                .map(|process| process.initialization.clone())
                .unwrap_or(Value::Null);
            native_token_usage(std::slice::from_ref(&frame), &initialization)
        } else {
            Value::Null
        };
        let mut saved = self.saved.lock().await;
        let Some(c) = saved.conversations.get_mut(id).filter(|c| !c.deleted) else {
            return Ok(());
        };
        if frame["type"] == "system"
            && frame["subtype"] == "init"
            && c.model.is_empty()
            && let Some(model) = frame["model"].as_str()
        {
            c.model = model.into();
        }
        let Some(turn) = c.turns.last_mut().filter(|t| t["status"] == "inProgress") else {
            return Ok(());
        };
        let turn_id = turn["id"].clone();
        let changed = project_frame(turn, &frame);
        c.updated_at = now_ms();
        let usage_changed = !usage.is_null() && c.token_usage != usage;
        if usage_changed {
            c.token_usage = usage.clone();
        }
        if frame["type"] != "stream_event" {
            self.persist(&saved)?;
        }
        drop(saved);
        for item in changed {
            self.publish(json!({"method":"item/updated","params":{"threadId":id,"turnId":turn_id,"item":item}}));
        }
        if usage_changed {
            self.publish(json!({"method":"thread/tokenUsage/updated","params":{"threadId":id,"tokenUsage":usage}}));
        }
        Ok(())
    }

    async fn ingest_shared(&self, id: &str, generation: &str, frame: &Value) -> anyhow::Result<()> {
        let initialization = {
            let processes = self.processes.lock().await;
            let Some(process) = processes
                .get(id)
                .filter(|p| p.channel.generation == generation)
            else {
                return Ok(());
            };
            process.initialization.clone()
        };
        self.apply_shared_snapshot(id, generation, frame, initialization, true)
            .await
    }

    async fn apply_shared_snapshot(
        &self,
        id: &str,
        generation: &str,
        frame: &Value,
        mut initialization: Value,
        track_unread: bool,
    ) -> anyhow::Result<()> {
        // 用户要求重连及配置变化后容量同步更新；只使用本代 WSL 桥接报告，空配置也明确清除旧值，不能串到其他模型/环境。
        if let Some(context) = frame.get("context_configuration") {
            initialization["contextConfiguration"] = context.clone();
        }
        let usage = native_token_usage(
            frame["messages"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
            &initialization,
        );
        let native_id = frame["native_id"]
            .as_str()
            .context("共享会话缺少原生身份")?;
        let job_id = frame["job_id"].as_str().context("共享会话缺少后台 ID")?;
        anyhow::ensure!(
            uuid::Uuid::parse_str(native_id).is_ok()
                && (job_id.is_empty()
                    || job_id.len() == 8 && job_id.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "Claude 原生会话身份无效"
        );
        let (thread, turns, turn_ids, completed, usage_changed) = self
            .change(|saved| {
                let c = saved
                    .conversations
                    .get_mut(id)
                    .context("Claude 会话不存在")?;
                anyhow::ensure!(c.shared && !c.deleted, "会话不使用共享传输");
                // 用户要求没有后台 job 也可读原生历史；UUID 必须始终是原会话，不能伪造 job 或换 UUID 掩盖恢复失败。
                anyhow::ensure!(
                    !c.resumed || c.native_id == native_id,
                    "原生会话 UUID 不匹配，未替换原历史"
                );
                let initial_import = c.discovered && c.turns.is_empty();
                c.native_id = native_id.into();
                c.native_job_id = job_id.into();
                c.native_status = frame["native_status"].clone();
                let usage_changed = c.token_usage != usage;
                c.token_usage = usage.clone();
                c.native_clients = serde_json::from_value(frame["clients"].clone())?;
                let pending = frame["pending"].as_array().cloned().unwrap_or_default();
                c.remote_queue.reconcile(&c.native_clients, &pending);
                let accepted: Vec<_> = pending
                    .into_iter()
                    .filter(|p| p["deliveryConfirmed"] != false)
                    .collect();
                if c.queued_messages != accepted {
                    c.remote_queue.changed();
                }
                c.queued_messages = accepted;
                c.resumed = true;
                let mut projected = project_shared_history(frame, generation);
                restore_shared_images(&c.turns, &mut projected);
                for turn in &mut projected {
                    if c.turns
                        .iter()
                        .any(|old| old["id"] == turn["id"] && old["_stopRequested"] == true)
                    {
                        turn["_stopRequested"] = json!(true);
                        if turn["status"] != "inProgress" {
                            turn["status"] = json!("interrupted");
                        }
                    }
                }
                let completed = projected.iter().any(|t| {
                    t["status"] != "inProgress"
                        && c.turns
                            .iter()
                            .any(|old| old["id"] == t["id"] && old["status"] == "inProgress")
                });
                if projected != c.turns {
                    c.updated_at = frame["updated_at"]
                        .as_u64()
                        .map_or_else(now_ms, |at| c.updated_at.max(at));
                    // 用户要求已读不因重连、状态变化或预览替换恢复蓝点；首次发现导入不置未读，后续新增正文仍沿用原判断。
                    if track_unread && !initial_import {
                        let repaired = repaired_history_baseline(&c.turns, frame);
                        c.unread = c.unread
                            || shared_history_has_new_content(
                                repaired.as_deref().unwrap_or(&c.turns),
                                &projected,
                            );
                    }
                }
                if !generation.starts_with("native-history-") {
                    c.discovered = false;
                }
                c.turns = projected;
                c.remote_queue.observe_turns(&c.turns, &c.native_status);
                let mut view = c.view();
                for (key, value) in c.queue_snapshot(&self.epoch).as_object().unwrap() {
                    if key != "ok" && key != "threadId" {
                        view[key] = value.clone();
                    }
                }
                Ok((
                    view,
                    c.turns.iter().rev().take(20).cloned().collect::<Vec<_>>(),
                    c.turns
                        .iter()
                        .map(|turn| turn["id"].clone())
                        .collect::<Vec<_>>(),
                    completed,
                    usage_changed,
                ))
            })
            .await?;
        self.observed
            .lock()
            .await
            .insert(id.into(), generation.into());
        let mut requests = BTreeMap::new();
        for (request_id, raw) in frame["requests"].as_object().into_iter().flatten() {
            requests.insert(request_id.clone(), permission_request(id, request_id, raw)?);
        }
        let mut all_requests = self.requests.lock().await;
        let requests_changed = all_requests.get(id) != Some(&requests);
        all_requests.insert(id.into(), requests);
        drop(all_requests);
        if requests_changed {
            self.requests_changed(id).await;
        }
        // 用户要求沿用网页聊天组件；共享消息按原生 UUID 投影成完整轮次，不把终端画面塞进聊天区。
        self.publish(json!({"method":"claude/history/updated","params":{
            "threadId":id,"thread":thread,"turns":turns,"turnIds":turn_ids,"completed":completed}}));
        if usage_changed {
            self.publish(json!({"method":"thread/tokenUsage/updated","params":{"threadId":id,"tokenUsage":usage}}));
        }
        self.publish(json!({"method":"remote/sidebarChanged","params":{"threadId":id}}));
        Ok(())
    }

    async fn ingest_shared_preview(
        &self,
        id: &str,
        generation: &str,
        frame: &Value,
    ) -> anyhow::Result<()> {
        if self
            .processes
            .lock()
            .await
            .get(id)
            .is_none_or(|p| p.channel.generation != generation)
        {
            return Ok(());
        }
        let mut saved = self.saved.lock().await;
        let Some(c) = saved
            .conversations
            .get_mut(id)
            .filter(|c| c.shared && !c.deleted)
        else {
            return Ok(());
        };
        let Some(turn) = c.turns.last_mut().filter(|t| {
            t["status"] == "inProgress" && t["_nativeUserId"] == frame["native_user_id"]
        }) else {
            return Ok(());
        };
        let preview_id = format!(
            "native-live-{}",
            frame["native_user_id"].as_str().unwrap_or("")
        );
        let item = json!({"id":preview_id,"type":"agentMessage","text":frame["text"],"status":"inProgress"});
        let items = turn["items"].as_array_mut().context("共享轮次缺少消息")?;
        if let Some(previous) = items.iter_mut().find(|i| i["id"] == preview_id) {
            *previous = item;
        } else {
            items.push(item);
        }
        // 流式预览只更新内存和既有聊天气泡；原生完整正文到达后替换预览并持久化，保留 Markdown 原文。
        let turn = turn.clone();
        let thread = c.view();
        drop(saved);
        self.publish(json!({"method":"claude/history/updated","params":{
            "threadId":id,"thread":thread,"turns":[turn],"preview":true}}));
        Ok(())
    }

    async fn finish(
        &self,
        id: &str,
        failure: Option<&str>,
        interrupted: bool,
        generation: Option<&str>,
    ) -> anyhow::Result<()> {
        if id.is_empty() {
            return Ok(());
        }
        if !self
            .saved
            .lock()
            .await
            .conversations
            .get(id)
            .is_some_and(|c| {
                c.active()
                    && generation.is_none_or(|g| c.turns.last().is_some_and(|t| t["_process"] == g))
            })
        {
            return Ok(());
        }
        let turn = self
            .change(|saved| {
                let Some(c) = saved.conversations.get_mut(id).filter(|c| !c.deleted) else {
                    return Ok(None);
                };
                let Some(turn) = c.turns.last_mut().filter(|t| {
                    t["status"] == "inProgress"
                        && generation.is_none_or(|generation| t["_process"] == generation)
                }) else {
                    return Ok(None);
                };
                let interrupted = interrupted || turn["_stopRequested"] == true;
                turn["status"] = json!(if interrupted {
                    "interrupted"
                } else if failure.is_some() {
                    "failed"
                } else {
                    "completed"
                });
                turn["turnCompletedAtMs"] = json!(now_ms());
                if let Some(message) = failure {
                    turn["error"] = json!({"message":message});
                }
                for item in turn["items"].as_array_mut().into_iter().flatten() {
                    if item["status"] == "inProgress" {
                        item["status"] = json!(if interrupted {
                            "interrupted"
                        } else {
                            "completed"
                        });
                    }
                }
                c.updated_at = now_ms();
                c.unread = true;
                Ok(Some(turn.clone()))
            })
            .await?;
        self.requests.lock().await.remove(id);
        self.requests_changed(id).await;
        if let Some(turn) = turn {
            self.publish(json!({"method":"turn/completed","params":{"threadId":id,"turn":turn}}));
        }
        Ok(())
    }

    async fn requests_changed(&self, id: &str) {
        let guard = self.requests.lock().await;
        let requests: Vec<_> = guard
            .get(id)
            .map(|r| r.values().cloned().collect())
            .unwrap_or_default();
        let revision = self.request_revision.fetch_add(1, Ordering::AcqRel) + 1;
        drop(guard);
        self.publish(json!({"method":"remote/requestsChanged","params":{
            "threadId":id,"requests":requests,"requestsKnown":true,
            "requestsEpoch":self.epoch,"requestsRevision":revision}}));
    }

    async fn respond(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let request_id = payload["requestId"]
            .as_str()
            .context("缺少 Claude 审批 ID")?;
        let request = self
            .requests
            .lock()
            .await
            .get(id)
            .and_then(|r| r.get(request_id))
            .cloned()
            .ok_or_else(|| error(409, "Claude 请求已经结束，请刷新会话"))?;
        let process = self
            .processes
            .lock()
            .await
            .get(id)
            .cloned()
            .context("Claude 进程已退出")?;
        let original = &request["_claude"];
        let response = if request["method"] == "item/tool/requestUserInput" {
            let answers = payload["response"]["answers"]
                .as_object()
                .context("问题回答格式无效")?;
            let mut values = serde_json::Map::new();
            for question in original["input"]["questions"]
                .as_array()
                .context("Claude 问题格式无效")?
            {
                let key = question["question"]
                    .as_str()
                    .context("Claude 问题缺少文字")?;
                let answer = answers
                    .get(key)
                    .and_then(|a| a["answers"].as_array())
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .context("请回答所有问题")?;
                values.insert(key.into(), json!(answer));
            }
            let mut input = original["input"].clone();
            input["answers"] = json!(values);
            json!({"behavior":"allow","updatedInput":input})
        } else {
            match payload["decision"].as_str() {
                Some("accept") => json!({"behavior":"allow","updatedInput":original["input"]}),
                Some("decline" | "cancel") => {
                    json!({"behavior":"deny","message":"用户拒绝本次操作",
                    "interrupt":payload["decision"]=="cancel"})
                }
                _ => return Err(error(400, "Claude 仅支持本次允许、拒绝或取消")),
            }
        };
        let response = json!({"subtype":"success","request_id":request_id,"response":response});
        if process.shared {
            process
                .control(
                    json!({"subtype":"shared_answer","response":response}),
                    Some(authorization),
                )
                .await?;
        } else {
            process
                .write(
                    json!({"type":"control_response","response":response}),
                    Some(authorization),
                )
                .await?;
        }
        if let Some(requests) = self.requests.lock().await.get_mut(id) {
            requests.remove(request_id);
        }
        self.requests_changed(id).await;
        Ok(json!({"ok":true,"threadId":id,"requestId":request_id,"approvalState":"submitted"}))
    }
}

fn native_project(sidebar: &Value, config: &WslConfig, linux_cwd: &str) -> (Value, String) {
    fn normalized(path: &str, config: &WslConfig) -> String {
        let mut path = path.replace('\\', "/").trim_end_matches('/').to_owned();
        for host in ["wsl.localhost", "wsl$"] {
            let prefix = format!("//{host}/{}/", config.distribution);
            if path
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
            {
                path = format!("/{}", &path[prefix.len()..]);
                break;
            }
        }
        if path.starts_with("/mnt/")
            && path.as_bytes().get(6) == Some(&b'/')
            && path.as_bytes().get(5).is_some_and(u8::is_ascii_alphabetic)
        {
            path = format!("{}:/{}", &path[5..6], &path[7..]);
        }
        if path.as_bytes().get(1) == Some(&b':') {
            path.make_ascii_lowercase();
        }
        path
    }
    let cwd = normalized(linux_cwd, config);
    // 用户要求复用共用 Codex 项目原 ID；仅匹配本机已注册根目录（或其子目录，最长根优先），绝不按 WSL 路径新建 Claude 项目。
    let mut found = None;
    for project in sidebar["projects"].as_array().into_iter().flatten() {
        if project["hostId"]
            .as_str()
            .is_some_and(|host| host != "local")
        {
            continue;
        }
        for root in project["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .chain(
                ["cwd", "path"]
                    .into_iter()
                    .filter_map(|key| project[key].as_str()),
            )
        {
            let root_path = normalized(root, config);
            if !root_path.is_empty()
                && (cwd == root_path || cwd.starts_with(&format!("{root_path}/")))
                && found
                    .as_ref()
                    .is_none_or(|(length, _, _)| root_path.len() > *length)
            {
                let suffix = &cwd[root_path.len()..];
                let display = format!(
                    "{}{}",
                    root.trim_end_matches(['/', '\\']),
                    if root.contains('\\') {
                        suffix.replace('/', "\\")
                    } else {
                        suffix.to_owned()
                    }
                );
                found = Some((root_path.len(), project["id"].clone(), display));
            }
        }
    }
    found
        .map(|(_, id, path)| (id, path))
        .unwrap_or_else(|| (Value::Null, linux_cwd.to_owned()))
}

fn shared_history_has_new_content(previous: &[Value], current: &[Value]) -> bool {
    fn user_content_changed(old: &Value, current: &Value) -> bool {
        use std::borrow::Cow;
        use std::collections::HashSet;
        let accepted: Vec<_> = [old, current]
            .into_iter()
            .flat_map(|item| item["content"].as_array().into_iter().flatten())
            .filter(|part| part["type"] == "image" && part["_nativePath"].is_string())
            .collect();
        let paths: HashSet<_> = accepted
            .iter()
            .filter_map(|image| image["_nativePath"].as_str())
            .map(|path| format!("@{path}"))
            .collect();
        let urls: HashSet<_> = accepted
            .iter()
            .filter_map(|image| image["url"].as_str())
            .collect();
        fn visible<'a>(
            item: &'a Value,
            paths: &HashSet<String>,
            urls: &HashSet<&str>,
        ) -> Vec<Cow<'a, Value>> {
            item["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| {
                    // 用户要求修复历史缺图/隐藏上传路径不重置未读；已受理原图及内部关联元数据不是新增正文。
                    if part["type"] == "image"
                        && (part["_nativePath"].is_string()
                            || part["url"].as_str().is_some_and(|url| urls.contains(url)))
                    {
                        return None;
                    }
                    if let Some(text) = part["text"].as_str() {
                        let text = text
                            .lines()
                            .filter(|line| !paths.contains(*line))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.is_empty() {
                            return None;
                        }
                        if part["text"] != text {
                            let mut part = part.clone();
                            part["text"] = json!(text);
                            return Some(Cow::Owned(part));
                        }
                    }
                    Some(Cow::Borrowed(part))
                })
                .collect()
        }
        visible(old, &paths, &urls) != visible(current, &paths, &urls)
    }
    fn items(turns: &[Value]) -> HashMap<(&str, &str, &str), &Value> {
        let mut result = HashMap::new();
        for turn in turns {
            // 原生用户 UUID 不随网页 client ID 补齐或连接代次改变；不能用展示轮次 ID 判断新消息。
            let native_user = turn["_nativeUserId"]
                .as_str()
                .or_else(|| turn["id"].as_str())
                .unwrap_or("");
            for item in turn["items"].as_array().into_iter().flatten() {
                let kind = item["type"].as_str().unwrap_or("");
                let id = if kind == "userMessage" {
                    ""
                } else {
                    item["id"].as_str().unwrap_or("")
                };
                // 用户要求过滤旧摘要后恢复工具/回复归属不重新置未读；非用户项用原生消息/工具 ID，不能绑定错误的旧轮次。
                let owner = if kind == "userMessage" || id.is_empty() {
                    native_user
                } else {
                    ""
                };
                result.insert((owner, kind, id), item);
            }
        }
        result
    }
    let previous = items(previous);
    items(current).into_iter().any(|(key, item)| {
        previous.get(&key).is_none_or(|old| {
            item.as_object().is_some_and(|fields| {
                fields.iter().any(|(field, value)| {
                    !matches!(field.as_str(), "id" | "clientId" | "status" | "sentAtMs")
                        && if key.1 == "userMessage" && field == "content" {
                            user_content_changed(old, item)
                        } else {
                            old.get(field) != Some(value)
                        }
                })
            })
        })
    })
}

fn repaired_history_baseline(previous: &[Value], frame: &Value) -> Option<Vec<Value>> {
    let rows = frame["messages"].as_array()?;
    let users: HashMap<_, _> = previous
        .iter()
        .filter_map(|turn| turn["_nativeUserId"].as_str().map(|id| (id, turn)))
        .collect();
    if !rows.iter().any(|row| {
        (row["isCompactSummary"] == true
            || row["isVisibleInTranscriptOnly"] == true
            || native_interruption(row).is_some())
            && row["uuid"]
                .as_str()
                .is_some_and(|id| users.contains_key(id))
    }) {
        return None;
    }
    // 用户要求内部摘要/中断假消息修复不误报未读：只修复原快照已见消息之前的归属，其后的真正新内容仍正常判断。
    let item_ids: std::collections::HashSet<_> = previous
        .iter()
        .flat_map(|turn| turn["items"].as_array().into_iter().flatten())
        .filter_map(|item| item["id"].as_str())
        .collect();
    let mut last_user = "";
    let mut boundary = None;
    for (index, row) in rows.iter().enumerate() {
        if row["type"] == "user"
            && row["isMeta"] != true
            && native_interruption(row).is_none()
            && (row["message"]["content"].is_string()
                || row["message"]["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] != "tool_result")))
        {
            last_user = row["uuid"].as_str().unwrap_or("");
            if users.contains_key(last_user) {
                boundary = Some(index);
            }
        }
        if row["type"] == "assistant"
            && row["message"]["content"].as_array().is_some_and(|parts| {
                parts.iter().enumerate().any(|(block, part)| {
                    part["id"].as_str().is_some_and(|id| item_ids.contains(id))
                        || item_ids.contains(
                            format!(
                                "{}-{}",
                                row["message"]["id"].as_str().unwrap_or("assistant"),
                                row["apiBlockIndex"].as_u64().unwrap_or(0) + block as u64
                            )
                            .as_str(),
                        )
                })
            })
            || row["subtype"] == "turn_duration"
                && users
                    .get(last_user)
                    .is_some_and(|turn| turn["_nativeDone"] == true)
        {
            boundary = Some(index);
        }
    }
    let mut repaired = previous.to_vec();
    for row in &rows[..=boundary?] {
        if row["type"] != "user" {
            continue;
        }
        for turn in &mut repaired {
            project_frame(turn, row);
        }
    }
    Some(repaired)
}

fn native_context_capacity(model: &str, initialization: &Value) -> (Option<u64>, Value) {
    if model.is_empty() {
        return (None, Value::Null);
    }
    // 用户要求统一原生口径：statusline、SDK 与模型目录的明确报告可作为容量；配置容量和压缩参数均不能替代。
    let native = &initialization["contextConfiguration"]["native"];
    if native["model"] == model
        && let Some(capacity) = native["context_window_size"]
            .as_u64()
            .filter(|value| *value > 0 && *value <= 9007199254740991)
    {
        return (
            Some(capacity),
            json!({"kind":"native","key":"context_window.context_window_size","model":model}),
        );
    }
    if let Some(capacity) = initialization["modelUsage"][model]["contextWindow"]
        .as_u64()
        .filter(|value| *value > 0 && *value <= 9007199254740991)
    {
        return (
            Some(capacity),
            json!({"kind":"native","key":"modelUsage.contextWindow","model":model}),
        );
    }
    let entry = initialization["models"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["value"] == model || entry["resolvedModel"] == model);
    if let Some((capacity, key)) = entry.and_then(|entry| {
        entry["contextWindow"]
            .as_u64()
            .filter(|value| *value > 0)
            .map(|value| (value, "contextWindow"))
            .or_else(|| {
                entry["runtime"]["max_input_tokens"]
                    .as_u64()
                    .filter(|value| *value > 0)
                    .map(|value| (value, "runtime.max_input_tokens"))
            })
    }) {
        return (
            Some(capacity),
            json!({"kind":"native","key":key,"model":model}),
        );
    }
    (None, Value::Null)
}

fn native_token_usage(messages: &[Value], initialization: &Value) -> Value {
    for row in messages.iter().rev() {
        let message = &row["message"];
        if row["type"] != "assistant"
            || row["isUnmetered"] == true
            || row["isApiErrorMessage"] == true
            || message["model"] == "<synthetic>"
        {
            continue;
        }
        let raw = &message["usage"];
        if !raw.is_object() || !raw["input_tokens"].is_u64() {
            continue;
        }
        let count = |usage: &Value, key: &str| usage.get(key).map_or(Some(0), Value::as_u64);
        // 原版 2.1.322 的 x9e/Ice/HHr 取最后 assistant 请求；缓存读写计入输入，output 不进入上下文百分比，分块绝不相加。
        let usage = raw["iterations"]
            .as_array()
            .filter(|_| {
                [
                    "input_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .filter_map(|key| count(raw, key))
                .any(|value| value > 0)
            })
            .and_then(|iterations| {
                iterations.iter().rev().find(|iteration| {
                    !matches!(
                        iteration["type"].as_str(),
                        Some("advisor_message" | "compaction")
                    )
                })
            })
            .filter(|iteration| {
                matches!(
                    iteration["type"].as_str(),
                    Some("message" | "fallback_message")
                ) && [
                    "input_tokens",
                    "output_tokens",
                    "cache_read_input_tokens",
                    "cache_creation_input_tokens",
                ]
                .iter()
                .all(|key| iteration[*key].is_u64())
                    && [
                        "input_tokens",
                        "cache_read_input_tokens",
                        "cache_creation_input_tokens",
                    ]
                    .iter()
                    .any(|key| iteration[*key].as_u64().unwrap_or(0) > 0)
            })
            .unwrap_or(raw);
        let (Some(input), Some(read), Some(created), Some(output)) = (
            count(usage, "input_tokens"),
            count(usage, "cache_read_input_tokens"),
            count(usage, "cache_creation_input_tokens"),
            count(usage, "output_tokens"),
        ) else {
            continue;
        };
        let Some(total) = input
            .checked_add(read)
            .and_then(|value| value.checked_add(created))
        else {
            continue;
        };
        let model = message["model"].as_str().unwrap_or("");
        let (capacity, capacity_source) = native_context_capacity(model, initialization);
        let mut result = json!({"modelContextWindow":capacity,"modelContextWindowSource":capacity_source,"model":model,"source":"claude-native-usage",
            "messageId":message["id"],"last":{"totalTokens":total,"inputTokens":input,
            "cachedInputTokens":read,"cacheCreationInputTokens":created,"outputTokens":output}});
        let context = &initialization["contextConfiguration"];
        let native = &context["native"];
        // 原生百分比必须对应同一模型、同一次输入占用；旧 statusline 只提供容量，不覆盖新请求的百分比。
        let current = &native["current_usage"];
        let current_total = current["input_tokens"].as_u64().and_then(|input| {
            input
                .checked_add(current["cache_read_input_tokens"].as_u64()?)?
                .checked_add(current["cache_creation_input_tokens"].as_u64()?)
        });
        if capacity.is_some()
            && result["modelContextWindowSource"]["key"] == "context_window.context_window_size"
            && native["model"] == model
            && current_total == Some(total)
            && let Some(percentage) = native["used_percentage"]
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 100.0)
        {
            result["usedPercentage"] = json!(percentage);
        }
        if context["model"] == model {
            let mut compaction = json!({});
            for (key, max) in [
                ("windowTokens", 9007199254740991_u64),
                ("triggerPercent", 100),
            ] {
                if let Some(value) = context["compaction"][key]
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= max)
                {
                    compaction[key] = json!(value);
                }
            }
            if compaction
                .as_object()
                .is_some_and(|value| !value.is_empty())
            {
                result["compaction"] = compaction;
            }
        }
        return result;
    }
    Value::Null
}

fn restore_shared_images(previous: &[Value], current: &mut [Value]) {
    // 用户要求重连不丢已受理原图：原图文件暂不可读时复用已持久化的图片；不把全历史 Base64 再塞进 initialize 控制帧。
    let old_turns: HashMap<_, _> = previous
        .iter()
        .filter_map(|turn| turn["_nativeUserId"].as_str().map(|native| (native, turn)))
        .collect();
    for turn in current {
        let Some(old) = turn["_nativeUserId"]
            .as_str()
            .and_then(|native| old_turns.get(native))
        else {
            continue;
        };
        let Some(parts) = turn["items"]
            .as_array_mut()
            .and_then(|items| items.iter_mut().find(|item| item["type"] == "userMessage"))
            .and_then(|item| item["content"].as_array_mut())
        else {
            continue;
        };
        let images: Vec<_> = old["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "userMessage")
            .flat_map(|item| item["content"].as_array().into_iter().flatten())
            .filter(|image| image["type"] == "image" && image["_nativePath"].is_string())
            .collect();
        if images.is_empty() {
            continue;
        }
        // 原生仅转录部分附件且原图暂不可读时，先去掉对应副本，再按已受理顺序恢复，避免第八张重复成第九张。
        parts.retain(|part| {
            !images.iter().any(|image| {
                part["type"] == "image"
                    && (part["_nativePath"] == image["_nativePath"]
                        || part["_nativePath"].is_null() && part["url"] == image["url"])
            })
        });
        let at = parts
            .iter()
            .position(|part| part["type"] == "image")
            .unwrap_or(parts.len());
        parts.splice(at..at, images.into_iter().cloned());
    }
}

fn native_interruption(row: &Value) -> Option<(&str, &str)> {
    // 用户要求只修正经原生元数据确认的中断，不能误删用户引用或仅文字相同的消息。
    if row["type"] != "user" {
        return None;
    }
    let response = row["interruptedMessageId"]
        .as_str()
        .filter(|id| !id.trim().is_empty())?;
    let content = row["message"]["content"].as_array()?;
    if content.len() != 1 || content[0]["type"] != "text" {
        return None;
    }
    let text = content[0]["text"].as_str()?;
    matches!(
        text.trim(),
        "[Request interrupted by user]" | "[Request interrupted by user for tool use]"
    )
    .then_some((response, text))
}

fn native_response_placeholder(row: &Value) -> bool {
    // 只有原生合成且明确非 API 错误的占位回复不展示；真实模型同名正文和所有错误信息继续保留。
    row["type"] == "assistant"
        && row["message"]["model"] == "<synthetic>"
        && row["isApiErrorMessage"] == false
        && row["message"]["content"].as_array().is_some_and(|parts| {
            parts.len() == 1
                && parts[0]["type"] == "text"
                && parts[0]["text"]
                    .as_str()
                    .is_some_and(|text| text.trim() == "No response requested.")
        })
}

fn project_shared_history(frame: &Value, generation: &str) -> Vec<Value> {
    let mut turns: Vec<Value> = vec![];
    let mut native_owners: HashMap<&str, usize> = HashMap::new();
    let mut web_image_paths = std::collections::HashSet::new();
    for row in frame["messages"].as_array().into_iter().flatten() {
        // 用户要求仅按原生元数据隐藏自动压缩内部摘要，不修改 JSONL，也不按英文前缀误删真实用户引用。
        if row["type"] == "user"
            && (row["isCompactSummary"] == true || row["isVisibleInTranscriptOnly"] == true)
        {
            continue;
        }
        if let Some((response, text)) = native_interruption(row) {
            let owner = native_owners
                .get(response)
                .copied()
                .or_else(|| {
                    row["parentUuid"]
                        .as_str()
                        .and_then(|id| native_owners.get(id).copied())
                })
                .or_else(|| {
                    turns
                        .len()
                        .checked_sub(1)
                        .filter(|index| turns[*index]["_nativeDone"] != true)
                });
            if let Some(index) = owner {
                let turn = &mut turns[index];
                if turn["status"] != "failed" {
                    turn["status"] = json!("interrupted");
                }
                turn["_nativeDone"] = json!(true);
                turn["turnCompletedAtMs"] = row["timestampMs"].clone();
                turn["nativeInterruption"] = json!({"source":"claude","id":row["uuid"],
                    "responseId":response,"text":text});
                if let Some(id) = row["uuid"].as_str() {
                    native_owners.insert(id, index);
                }
            }
            // 原生控制记录只结束关联轮次，后面的工具结果仍归原问题，不能制造新的用户气泡。
            continue;
        }
        if native_response_placeholder(row) {
            continue;
        }
        let mut content = row["message"]["content"].clone();
        if let Some(text) = content.as_str() {
            content = json!([{"type":"text","text":text}]);
        }
        let is_user = row["type"] == "user"
            && row["isMeta"] != true
            && content.as_array().is_some_and(|parts| {
                !parts.is_empty() && parts.iter().any(|p| p["type"] != "tool_result")
            });
        if is_user {
            web_image_paths.clear();
            // 用户要求历史回显保留全部已受理网页图片：按原始路径关联去重，正文移除全部内部 @路径，不依赖 CLI 只转录的部分附件。
            for part in content.as_array_mut().into_iter().flatten() {
                if let Some(text) = part["text"].as_str() {
                    let mut text = text.to_owned();
                    for path in row["remote_upload_paths"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        text = text
                            .lines()
                            .filter(|line| *line != format!("@{path}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                    part["text"] = json!(text);
                }
            }
            for image in row["web_images"].as_array().into_iter().flatten() {
                if let Some(path) = image["filename"].as_str()
                    && web_image_paths.insert(path.to_owned())
                    && let Some(parts) = content.as_array_mut()
                {
                    parts.push(json!({"type":"image","url":image["url"],"_nativePath":path}));
                }
            }
            for part in content.as_array_mut().into_iter().flatten() {
                if part["type"] == "image" && part["source"]["type"] == "base64" {
                    *part = json!({"type":"image","url":format!("data:{};base64,{}",
                        part["source"]["media_type"].as_str().unwrap_or(""),
                        part["source"]["data"].as_str().unwrap_or(""))});
                }
            }
            let native_user = row["uuid"].as_str().unwrap_or("");
            let client_id = frame["clients"][native_user]
                .as_str()
                .unwrap_or(native_user);
            turns.push(json!({"id":client_id,"_nativeUserId":native_user,
                "_process":generation,"status":"completed","itemsView":"full","itemsComplete":true,
                "turnStartedAtMs":row["timestampMs"],
                "items":[{"id":client_id,"clientId":client_id,"type":"userMessage",
                    "content":content,"sentAtMs":row["timestampMs"]}]}));
        } else if let Some(turn) = turns.last_mut() {
            if row["type"] == "attachment" {
                if let Some(user) = turn["items"]
                    .as_array_mut()
                    .and_then(|items| items.iter_mut().find(|item| item["type"] == "userMessage"))
                    && let Some(parts) = user["content"].as_array_mut()
                {
                    // 用户要求继续使用现有图片展示；原版 @图片附件并回同条用户消息，不显示连接端临时路径。
                    if let Some(filename) = row["filename"]
                        .as_str()
                        .filter(|_| row["remote_upload"] == true)
                    {
                        for part in parts.iter_mut() {
                            if let Some(text) = part["text"].as_str() {
                                part["text"] = json!(text.replace(&format!("\n@{filename}"), ""));
                            }
                        }
                        parts.retain(|part| part["text"] != "");
                    }
                    if !row["filename"]
                        .as_str()
                        .is_some_and(|path| web_image_paths.contains(path))
                    {
                        parts.extend(content.as_array().into_iter().flatten().cloned());
                    }
                }
            } else if row["subtype"] == "turn_duration" {
                turn["_nativeDone"] = json!(true);
                turn["turnCompletedAtMs"] = row["timestampMs"].clone();
            } else if matches!(row["type"].as_str(), Some("assistant" | "user")) {
                let mut native_frame = row.clone();
                native_frame["message"]["content"] = content;
                project_frame(turn, &native_frame);
                if row["isApiErrorMessage"] == true {
                    turn["status"] = json!("failed");
                }
            }
        }
        if let Some(index) = turns.len().checked_sub(1) {
            if let Some(id) = row["uuid"].as_str() {
                native_owners.insert(id, index);
            }
            if row["type"] == "assistant"
                && let Some(id) = row["message"]["id"].as_str()
            {
                native_owners.insert(id, index);
            }
        }
    }
    if let Some(turn) = turns.last_mut()
        && turn["_nativeDone"] != true
    {
        if matches!(
            frame["native_status"]["tempo"].as_str(),
            Some("active" | "blocked")
        ) {
            turn["status"] = json!("inProgress");
        } else {
            turn["status"] = json!("interrupted");
        }
    }
    let latest = turns.len().checked_sub(1);
    for (index, turn) in turns.iter_mut().enumerate() {
        if turn["nativeInterruption"]["source"] == "claude"
            || Some(index) == latest && turn["status"] != "inProgress"
        {
            for item in turn["items"].as_array_mut().into_iter().flatten() {
                if item["status"] == "inProgress" {
                    item["status"] = json!("interrupted");
                }
            }
        }
    }
    turns
}

fn model_selection<'a>(
    c: &'a Conversation,
    payload: &'a Value,
    catalog: &'a Value,
) -> anyhow::Result<(&'a str, &'a str)> {
    let model = payload["model"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if c.model.is_empty() {
                catalog["defaultModel"].as_str().unwrap_or("")
            } else {
                &c.model
            }
        });
    let rows = catalog["data"]
        .as_array()
        .ok_or_else(|| error(400, "所选模型未在当前 Claude 的可用模型列表中"))?;
    // 用户要求完整模型 ID 也能选择原生报告的推理等级；精确值优先，只用显式 resolvedModel 对齐别名，不按名称猜测能力。
    let find_model = |id: &str| {
        if id.is_empty() {
            return None;
        }
        rows.iter()
            .find(|row| row["model"] == id)
            .or_else(|| rows.iter().find(|row| row["resolvedModel"] == id))
    };
    let row =
        find_model(model).ok_or_else(|| error(400, "所选模型未在当前 Claude 的可用模型列表中"))?;
    let same_model = model == c.model
        || find_model(&c.model).is_some_and(|current| current["model"] == row["model"]);
    let effort = payload["reasoningEffort"]
        .as_str()
        // 用户要求 Sol / Max 验收：后续普通或排队消息省略设置时保留同一模型的等级；
        // 显式空字符串仍表示恢复默认，切换模型时不继承旧模型可能不支持的等级。
        .unwrap_or_else(|| if same_model { &c.effort } else { "" });
    if !effort.is_empty()
        && !row["supportedReasoningEfforts"]
            .as_array()
            .is_some_and(|values| values.iter().any(|v| v["reasoningEffort"] == effort))
    {
        return Err(error(400, "所选思考深度不受此 Claude 模型支持"));
    }
    Ok((model, effort))
}

async fn apply_model(
    process: &Process,
    c: &mut Conversation,
    payload: &Value,
    auth: &RemoteAuthorization,
) -> anyhow::Result<()> {
    let catalog = normalize_models(&process.initialization);
    let (model, effort) = model_selection(c, payload, &catalog)?;
    process
        .control(json!({"subtype":"set_model","model":model}), Some(auth))
        .await?;
    if !effort.is_empty() || !c.effort.is_empty() {
        process
            .control(
                json!({"subtype":"apply_flag_settings","settings":{
            "effortLevel":if effort.is_empty() {Value::Null} else {json!(effort)}}}),
                Some(auth),
            )
            .await?;
    }
    let (model, effort) = (model.to_owned(), effort.to_owned());
    c.model = model;
    c.effort = effort;
    Ok(())
}

fn normalize_models(initialization: &Value) -> Value {
    let reported = initialization["models"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let default_resolved = reported.first().and_then(|m| m["resolvedModel"].as_str());
    // CLI 额外返回 Default 别名；与明确选项指向同一模型时去重，单模型环境只展示一次。
    let rows: Vec<_> = reported
        .iter()
        .filter(|model| {
            !(model["value"] == "default"
                && default_resolved.is_some_and(|resolved| {
                    reported.iter().any(|other| {
                        other["value"] != "default"
                            && (other["value"] == resolved || other["resolvedModel"] == resolved)
                    })
                }))
        })
        .filter_map(|model| {
            let value = model["value"].as_str()?;
            let efforts: Vec<_> = model["supportedEffortLevels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|effort| json!({"reasoningEffort":effort}))
                .collect();
            Some(
                json!({"id":value,"model":value,"resolvedModel":model["resolvedModel"],
            "displayName":model["displayName"].as_str().unwrap_or(value),
            "description":model["description"],"supportedReasoningEfforts":efforts}),
            )
        })
        .collect();
    let default = rows
        .first()
        .map(|m| m["model"].clone())
        .unwrap_or(Value::Null);
    json!({"ok":true,"backend":"claude","data":rows,"catalogComplete":true,"defaultModel":default,"defaultReasoningEffort":""})
}

pub(super) fn input_content(payload: &Value) -> anyhow::Result<Vec<Value>> {
    if payload["goalMode"] == true
        || payload["queueOnly"] == true
        || payload["skills"].as_array().is_some_and(|s| !s.is_empty())
    {
        return Err(error(
            400,
            "Claude 会话不接受 Codex 目标、队列或 Skill 路径",
        ));
    }
    let mut content = Vec::new();
    if let Some(prompt) = payload["prompt"].as_str().filter(|s| !s.trim().is_empty()) {
        content.push(json!({"type":"text","text":prompt}));
    }
    for image in payload["images"].as_array().into_iter().flatten() {
        let url = image["dataUrl"].as_str().context("Claude 图片格式无效")?;
        let (mime, data) = url
            .strip_prefix("data:")
            .and_then(|s| s.split_once(";base64,"))
            .filter(|(mime, _)| {
                ["image/png", "image/jpeg", "image/gif", "image/webp"].contains(mime)
            })
            .ok_or_else(|| error(400, "Claude 图片只支持 PNG、JPEG、GIF 和 WebP"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("图片 Base64 无效")?;
        anyhow::ensure!(
            !bytes.is_empty() && bytes.len() <= 5 * 1024 * 1024,
            "Claude 单张图片大小需小于 5 MiB"
        );
        content
            .push(json!({"type":"image","source":{"type":"base64","media_type":mime,"data":data}}));
    }
    if content.is_empty() {
        return Err(error(400, "请输入消息或添加图片"));
    }
    Ok(content)
}

fn permission_request(thread: &str, id: &str, request: &Value) -> anyhow::Result<Value> {
    if request["subtype"] != "can_use_tool" {
        return Err(error(409, "Claude 发出了尚未支持的控制请求，未自动批准"));
    }
    if request["tool_name"] == "AskUserQuestion" {
        let questions: Vec<_> = request["input"]["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|q| {
                let mut q = q.clone();
                q["id"] = q["question"].clone();
                q
            })
            .collect();
        return Ok(json!({"id":id,"method":"item/tool/requestUserInput",
            "params":{"threadId":thread,"questions":questions},"_claude":request}));
    }
    Ok(
        json!({"id":id,"method":"item/commandExecution/requestApproval",
        "params":{"threadId":thread,"command":request["input"]["command"].as_str()
            .map(str::to_owned).unwrap_or_else(|| format!("{} {}",request["tool_name"],request["input"])),
            "reason":request["decision_reason"],"availableDecisions":["accept","decline","cancel"]},
        "_claude":request}),
    )
}

fn project_frame(turn: &mut Value, frame: &Value) -> Vec<Value> {
    let Some(items) = turn["items"].as_array_mut() else {
        return vec![];
    };
    let mut changes: Vec<Value> = Vec::new();
    if frame["type"] == "assistant" {
        let message = frame["message"]["id"].as_str().unwrap_or("assistant");
        for (index, block) in frame["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            // 最终 assistant 的 content 可能省略 thinking 块，索引与 stream_event 不同。
            // 使用同一原生消息中已流出的正文/思考项，避免相同回复在 UI 中出现两次。
            let kind = if block["type"] == "thinking" {
                "reasoning"
            } else {
                "agentMessage"
            };
            let full_text = if kind == "reasoning" {
                block["thinking"].as_str()
            } else {
                block["text"].as_str()
            };
            let streamed = full_text
                // 原生历史明确提供块索引时按身份合并；两个独立正文/思考块即使同前缀也不能相互覆盖。
                .filter(|_| frame["apiBlockIndex"].as_u64().is_none())
                .and_then(|text| {
                    items.iter().find(|item| {
                        let prefix = if kind == "reasoning" {
                            item["summary"][0].as_str()
                        } else {
                            item["text"].as_str()
                        };
                        item["type"] == kind
                            && item["id"]
                                .as_str()
                                .is_some_and(|id| id.starts_with(&format!("{message}-")))
                            && prefix.is_some_and(|prefix| {
                                !prefix.is_empty() && text.starts_with(prefix)
                            })
                            && !changes.iter().any(|changed| changed["id"] == item["id"])
                    })
                })
                .and_then(|item| item["id"].as_str())
                .map(str::to_owned);
            let block_index = frame["apiBlockIndex"].as_u64().unwrap_or(0) + index as u64;
            let mut id = streamed.unwrap_or_else(|| format!("{message}-{block_index}"));
            if changes.iter().any(|changed| changed["id"] == id) {
                id = format!("{message}-final-{index}");
            }
            let item = match block["type"].as_str() {
                Some("text") => {
                    json!({"id":id,"type":"agentMessage","text":block["text"],"status":"completed"})
                }
                Some("thinking") => {
                    json!({"id":id,"type":"reasoning","summary":[block["thinking"]],"status":"completed"})
                }
                Some("tool_use") => {
                    json!({"id":block["id"],"type":"dynamicToolCall","tool":block["name"],
                    "arguments":block["input"],"status":"inProgress","contentItems":[]})
                }
                _ => continue,
            };
            // stream_event 的部分内容与最终 assistant 使用同一个原生消息 ID，避免回复重复。
            if let Some(old) = items.iter_mut().find(|i| i["id"] == item["id"]) {
                *old = item.clone();
            } else {
                items.push(item.clone());
            }
            changes.push(item);
        }
    } else if frame["type"] == "user" {
        for block in frame["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] != "tool_result" {
                continue;
            }
            if let Some(item) = items.iter_mut().find(|i| i["id"] == block["tool_use_id"]) {
                item["status"] = json!(if block["is_error"] == true {
                    "failed"
                } else {
                    "completed"
                });
                item["contentItems"] = if block["content"].is_array() {
                    block["content"].clone()
                } else {
                    json!([{"type":"text","text":block["content"].as_str().unwrap_or("")}])
                };
                changes.push(item.clone());
            }
        }
    } else if frame["type"] == "stream_event" {
        let event = &frame["event"];
        if event["type"] == "message_start" {
            turn["_claudeMessage"] = event["message"]["id"].clone();
        } else if event["type"] == "content_block_delta" {
            let message = turn["_claudeMessage"].as_str().unwrap_or("assistant");
            let id = format!("{message}-{}", event["index"].as_u64().unwrap_or(0));
            let thinking = event["delta"]["type"] == "thinking_delta";
            let text = if thinking {
                event["delta"]["thinking"].as_str()
            } else {
                event["delta"]["text"].as_str()
            };
            if let Some(text) = text {
                let items = turn["items"].as_array_mut().unwrap();
                let index = items.iter().position(|i| i["id"] == id).unwrap_or_else(|| {
                    items.push(if thinking {
                        json!({"id":id,"type":"reasoning","summary":[""],"status":"inProgress"})
                    } else {
                        json!({"id":id,"type":"agentMessage","text":"","status":"inProgress"})
                    });
                    items.len() - 1
                });
                let slot = if thinking {
                    &mut items[index]["summary"][0]
                } else {
                    &mut items[index]["text"]
                };
                *slot = json!(format!("{}{text}", slot.as_str().unwrap_or("")));
                changes.push(items[index].clone());
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_and_saved_wsl_environments_share_the_same_validation_rules() {
        assert!(WslConfig::default().validate().is_ok());
        // 只验证内存配置；不启动任何真实发行版或程序。
        for distribution in ["Ubuntu", "claude-us", "CLAUDE-US", "work.profile_2"] {
            let config = WslConfig {
                distribution: distribution.into(),
                user: "fixture-user".into(),
                executable: "/fixture/bin/claude".into(),
            };
            assert!(config.validate().is_ok());
        }
        for distribution in ["", ".", "..", "../Ubuntu", "Ubuntu/other"] {
            let config = WslConfig {
                distribution: distribution.into(),
                ..WslConfig::default()
            };
            assert!(config.validate().is_err());
            assert!(Claude::wsl(&config).is_err());
        }
    }

    #[tokio::test]
    async fn image_payload_at_http_limit_keeps_room_for_the_internal_json_envelope() {
        let content = json!([{"type":"image","source":{"type":"base64","media_type":"image/png",
            "data":"A".repeat(16 * 1024 * 1024)}}]);
        let frame = json!({"type":"control_request","request_id":"limit","request":{
            "subtype":"shared_send","client_id":"images","content":content}});
        let expected = serde_json::to_vec(&frame).unwrap().len() + 1;
        assert!(expected > MAX_FRAME_BYTES && expected < super::super::MAX_ACTION_BODY_BYTES);
        for shared in [false, true] {
            let (input, output) = tokio::io::duplex(64 * 1024);
            let (writer, _) = spawn_writer(input, WRITE_TIMEOUT);
            let process = Process {
                _child: Mutex::new(None),
                writer,
                shared,
                initialization: Value::Null,
                channel: Arc::new(Channel {
                    live: AtomicBool::new(true),
                    generation: "frame-limit".into(),
                    pending: Mutex::new(HashMap::new()),
                }),
            };
            let reader = tokio::spawn(async move {
                let mut line = Vec::new();
                BufReader::new(output)
                    .read_until(b'\n', &mut line)
                    .await
                    .unwrap();
                line.len()
            });
            process.write(frame.clone(), None).await.unwrap();
            assert_eq!(reader.await.unwrap(), expected);
            process.channel.live.store(false, Ordering::Release);
            let failure = process.write(json!({}), None).await.unwrap_err();
            assert!(!failure.downcast_ref::<ApiError>().unwrap().outcome_unknown);
        }
    }

    pub(super) fn fixture() -> (tempfile::TempDir, Arc<Claude>, Conversation) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&root).unwrap();
        let directory = tempfile::tempdir_in(root).unwrap();
        let service = Claude::new(directory.path()).unwrap();
        let c = Conversation {
            backend: claude_backend(),
            id: format!("claude-{}", uuid::Uuid::new_v4()),
            native_id: uuid::Uuid::new_v4().to_string(),
            project_id: json!("shared-project"),
            cwd: "E:\\fixture".into(),
            linux_cwd: "/mnt/e/fixture".into(),
            config: WslConfig::default(),
            title: "测试会话".into(),
            model: "test-model".into(),
            effort: "low".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
            pinned: false,
            unread: false,
            deleted: false,
            resumed: true,
            shared: false,
            native_job_id: String::new(),
            native_status: Value::Null,
            token_usage: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            discovered: false,
            native_title: String::new(),
            turns: vec![
                json!({"id":"turn-1","_process":"new-process","status":"inProgress","items":[]}),
            ],
        };
        (directory, service, c)
    }

    async fn discovery_fixture(
        service: &Arc<Claude>,
        rows: Value,
        snapshot: Value,
    ) -> Arc<Mutex<Vec<Value>>> {
        let channel = Arc::new(Channel {
            live: AtomicBool::new(true),
            generation: "discovery-fixture".into(),
            pending: Mutex::new(HashMap::new()),
        });
        let (input, output) = tokio::io::duplex(64 * 1024);
        let (writer, _) = spawn_writer(input, WRITE_TIMEOUT);
        let process = Arc::new(Process {
            _child: Mutex::new(None),
            writer,
            channel: channel.clone(),
            shared: true,
            initialization: json!({}),
        });
        let calls = Arc::new(Mutex::new(Vec::new()));
        let record = calls.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(output).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame: Value = serde_json::from_str(&line).unwrap();
                let request = &frame["request"];
                record.lock().await.push(request.clone());
                let response = match request["subtype"].as_str() {
                    Some("discover") => json!({"sessions":rows}),
                    Some("read_history") => snapshot.clone(),
                    _ => panic!("只读测试不能启动模型、恢复任务或发送消息"),
                };
                if let Some(sender) = channel
                    .pending
                    .lock()
                    .await
                    .remove(frame["request_id"].as_str().unwrap())
                {
                    let _ = sender.send(Ok(response));
                }
            }
        });
        *service
            .discovery_state(&WslConfig::default())
            .await
            .unwrap()
            .lock()
            .await = DiscoveryState {
            config: Some(WslConfig::default()),
            process: Some(process),
            checked: None,
        };
        calls
    }

    #[tokio::test]
    async fn native_discovery_from_empty_saved_state_reuses_projects_ids_and_deleted_tombstones() {
        let (directory, service, _) = fixture();
        assert!(
            !directory
                .path()
                .join("remote-claude-sessions.json")
                .exists()
        );
        let config = WslConfig::default();
        let native = "38515c78-cbe6-4961-b5b0-6ee76e81876c";
        let cli = "00000000-0000-4000-8000-000000000001";
        // 跨 WSL/Windows 的项目归属夹具沿用 CodexElves 命名，根目录与子目录保持一致。
        let rows = vec![
            json!({"nativeId":native,"cwd":"/mnt/e/workspace/CodexElves","title":"原生标题",
            "createdAt":1000,"updatedAt":2000,"model":"gpt-6.1-sol","effort":"max","jobId":"38515c78","nativeStatus":{"tempo":"idle"}}),
            json!({"nativeId":cli,"cwd":"/home/cli-only","title":"CLI历史","createdAt":1000,"updatedAt":1500,"jobId":""}),
        ];
        let mut sidebar = json!({"recent":[],"pinnedThreads":[],"projects":[{"id":"registered-codex-project","roots":["E:\\workspace\\CodexElves"],"threads":[]}]});
        service
            .register_native_sessions(&config, &rows, &sidebar)
            .await
            .unwrap();
        service
            .register_native_sessions(&config, &rows, &sidebar)
            .await
            .unwrap();
        assert_eq!(service.saved.lock().await.conversations.len(), 2);
        service.merge_sidebar(&mut sidebar).await;
        assert_eq!(sidebar["recent"].as_array().unwrap().len(), 2);
        assert_eq!(sidebar["projects"].as_array().unwrap().len(), 1);
        assert_eq!(sidebar["projects"][0]["threads"][0]["nativeId"], native);
        let id = sidebar["projects"][0]["threads"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!service.conversation(&id).await.unwrap().unread);
        service
            .change(|saved| {
                for c in saved.conversations.values_mut() {
                    if c.native_id == native {
                        c.title = "网页自定义标题".into();
                        c.pinned = true;
                        c.unread = true;
                    } else {
                        c.deleted = true;
                    }
                }
                Ok(())
            })
            .await
            .unwrap();
        let restored = Claude::new(directory.path()).unwrap();
        restored
            .register_native_sessions(&config, &rows, &sidebar)
            .await
            .unwrap();
        let c = restored.conversation(&id).await.unwrap();
        assert_eq!(c.id, id);
        assert_eq!(c.title, "网页自定义标题");
        assert!(c.pinned && c.unread);
        assert_eq!(c.project_id, "registered-codex-project");
        assert!(
            restored
                .saved
                .lock()
                .await
                .conversations
                .values()
                .any(|c| c.native_id == cli && c.deleted)
        );
        assert_eq!(restored.saved.lock().await.conversations.len(), 2);
        assert!(
            restored.processes.lock().await.is_empty(),
            "重启及注册不能启动原生任务"
        );
        let unknown = native_project(&sidebar, &config, "/home/no-project");
        assert!(unknown.0.is_null());
        let nested = native_project(&sidebar, &config, "/mnt/e/workspace/CodexElves/crates");
        assert_eq!(nested.0, "registered-codex-project");
        let (_legacy_directory, legacy_service, mut legacy) = fixture();
        legacy.native_id = native.into();
        legacy.title = "旧网页自定义标题".into();
        legacy.project_id = Value::Null;
        legacy.turns[0]["status"] = json!("completed");
        let legacy_id = legacy.id.clone();
        legacy_service
            .change(|saved| {
                saved.conversations.insert(legacy.id.clone(), legacy);
                Ok(())
            })
            .await
            .unwrap();
        for _ in 0..2 {
            legacy_service
                .register_native_sessions(&config, &rows, &sidebar)
                .await
                .unwrap();
            let migrated = legacy_service.conversation(&legacy_id).await.unwrap();
            assert!(migrated.shared && migrated.discovered);
            assert_eq!(migrated.title, "旧网页自定义标题");
            assert!(
                migrated.project_id.is_null(),
                "旧网页选择无项目不能被扫描改写"
            );
            assert!(!migrated.unread);
        }
    }

    #[tokio::test]
    async fn session_instances_keep_native_ids_readers_models_and_sidebar_independent() {
        let (_directory, service, _) = fixture();
        let a = WslConfig::default();
        let b = WslConfig {
            distribution: "fixture-second-wsl".into(),
            ..a.clone()
        };
        let rows = vec![json!({"nativeId":"38515c78-cbe6-4961-b5b0-6ee76e81876c",
            "cwd":"/home/user/project","title":"same native UUID","createdAt":1000,"updatedAt":2000})];
        let empty = json!({"recent":[],"pinnedThreads":[],"projects":[]});
        for config in [&a, &b] {
            service
                .register_native_sessions(config, &rows, &empty)
                .await
                .unwrap();
        }
        assert_eq!(service.saved.lock().await.conversations.len(), 2);
        let reader_a = service.discovery_state(&a).await.unwrap();
        let reader_b = service.discovery_state(&b).await.unwrap();
        assert!(!Arc::ptr_eq(&reader_a, &reader_b));
        assert!(Arc::ptr_eq(
            &reader_a,
            &service.discovery_state(&a).await.unwrap()
        ));
        for (config, model) in [(&a, "model-a"), (&b, "model-b")] {
            let key = config.instance_id();
            service
                .metadata
                .lock()
                .await
                .insert(key, (Instant::now(), json!({"data":[{"model":model}]})));
            let catalog = service
                .models_in(config.clone(), "/same/project", false)
                .await
                .unwrap();
            assert_eq!(catalog["data"][0]["model"], model);
            let mut sidebar = empty.clone();
            service
                .merge_sidebar_instances(&mut sidebar, Some(&[config.instance_id()]))
                .await;
            assert_eq!(sidebar["recent"].as_array().unwrap().len(), 1);
            assert_eq!(sidebar["recent"][0]["instanceId"], config.instance_id());
            let id = sidebar["recent"][0]["id"].as_str().unwrap();
            assert_eq!(service.instance_id(id).await.unwrap(), config.instance_id());
        }
        let mut old_client = empty;
        service.merge_sidebar(&mut old_client).await;
        assert_eq!(
            old_client["recent"].as_array().unwrap().len(),
            1,
            "旧类型开关只包括原默认实例"
        );
    }

    #[tokio::test]
    async fn discovered_model_choices_use_the_bound_instance_catalog_without_resuming() {
        let (_directory, service, mut c) = fixture();
        c.discovered = true;
        c.shared = true;
        c.config = WslConfig {
            distribution: "fixture-history-wsl".into(),
            user: "fixture-history-user".into(),
            executable: "/fixture/bin/claude".into(),
        };
        c.model = "history-haiku".into();
        c.effort = "high".into();
        c.turns[0]["status"] = json!("completed");
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let unrelated = json!({"data":[{"model":"another-instance-model"}]});
        service.metadata.lock().await.insert(
            WslConfig::default().instance_id(),
            (Instant::now(), unrelated),
        );
        let history = service.session_models(&c.id, false, true).await.unwrap();
        assert_eq!(history["catalogComplete"], false);
        assert_eq!(history["data"].as_array().unwrap().len(), 1);
        assert_eq!(history["data"][0]["model"], "history-haiku");
        let catalog = normalize_models(&json!({"models":[
            {"value":"fixture-opus","supportedEffortLevels":["high","max"]},
            {"value":"history-haiku","supportedEffortLevels":["low","high"]},
            {"value":"fixture-sonnet","supportedEffortLevels":["medium","high"]}
        ]}));
        let key = c.config.instance_id();
        service
            .metadata
            .lock()
            .await
            .insert(key.clone(), (Instant::now(), catalog.clone()));
        let available = service.session_models(&c.id, true, false).await.unwrap();
        assert_eq!(available["data"], catalog["data"]);
        assert_eq!(available["defaultModel"], "fixture-opus");
        assert_eq!(available["defaultReasoningEffort"], "");
        let another_project = service
            .models_in(c.config.clone(), "/another/project", false)
            .await
            .unwrap();
        assert_eq!(another_project, available, "新建与历史会话必须共用实例目录");
        service.metadata.lock().await.insert(
            key,
            (Instant::now() - Duration::from_secs(301), catalog.clone()),
        );
        let refreshed_history = service.session_models(&c.id, false, true).await.unwrap();
        assert_eq!(refreshed_history["data"], catalog["data"]);
        let saved = service.conversation(&c.id).await.unwrap();
        assert_eq!(saved.native_id, c.native_id);
        assert!(saved.config == c.config);
        assert_eq!(saved.model, c.model);
        assert_eq!(saved.effort, c.effort);
        assert_eq!(saved.turns, c.turns);
        assert!(service.processes.lock().await.is_empty());
        assert!(service.discovery.lock().await.is_empty());
    }

    #[tokio::test]
    async fn native_discovered_history_without_job_is_read_only_paginated_and_keeps_uuid_usage() {
        let (directory, service, _) = fixture();
        let native = "38515c78-cbe6-4961-b5b0-6ee76e81876c";
        let rows = json!([{"nativeId":native,"cwd":"/mnt/e/shared","title":"原生历史","createdAt":1000,"updatedAt":2000,
            "model":"gpt-6.1-sol","effort":"max","jobId":"","nativeStatus":{"tempo":"idle"}}]);
        let mut messages = Vec::new();
        for i in 0..29 {
            messages.push(json!({"type":"user","uuid":format!("user-{i}"),"message":{"content":format!("问题-{i}")}}));
            if i == 10 {
                messages.push(json!({"type":"user","uuid":"compact","isCompactSummary":true,"message":{"content":"内部摘要"}}));
            }
            messages.push(json!({"type":"assistant","message":{"id":format!("reply-{i}"),"model":"gpt-6.1-sol",
                "content":[{"type":"text","text":"原生回复"}],"usage":{"input_tokens":181,"cache_read_input_tokens":108032,"cache_creation_input_tokens":0,"output_tokens":545}}}));
            messages.push(json!({"type":"system","subtype":"turn_duration"}));
        }
        let frame = json!({"type":"shared_snapshot","native_id":native,"job_id":"","native_status":{"tempo":"idle"},
            "messages":messages,"clients":{},"pending":[],"requests":{},"updated_at":2000,
            "context_configuration":{"maxContextTokens":600000,"model":"gpt-6.1-sol","source":{
                "kind":"configuration","key":"CLAUDE_CODE_MAX_CONTEXT_TOKENS","scope":"user-settings.env","model":"gpt-6.1-sol"},
                "native":{"model":"gpt-6.1-sol","context_window_size":1000000}}});
        let calls = discovery_fixture(&service, rows.clone(), frame.clone()).await;
        let mut sidebar = json!({"recent":[],"pinnedThreads":[],"projects":[{"id":"same-project","roots":["E:\\shared"],"threads":[]}]});
        service.discover(&sidebar).await.unwrap();
        service.discover(&sidebar).await.unwrap();
        service.merge_sidebar(&mut sidebar).await;
        let id = sidebar["recent"][0]["id"].as_str().unwrap();
        let detail = service.detail(id).await.unwrap();
        assert_eq!(detail["thread"]["nativeId"], native);
        assert_eq!(detail["thread"]["nativeJobId"], "");
        assert_eq!(
            detail["thread"]["tokenUsage"]["modelContextWindow"],
            1000000
        );
        assert_eq!(
            detail["thread"]["tokenUsage"]["last"]["totalTokens"],
            108213
        );
        let mut cursor = Value::Null;
        let mut ids = std::collections::HashSet::new();
        for count in [10, 10, 9] {
            let page = service
                .history(&json!({"threadId":id,"cursor":cursor}))
                .await
                .unwrap();
            assert_eq!(page["data"].as_array().unwrap().len(), count);
            for turn in page["data"].as_array().unwrap() {
                assert!(ids.insert(turn["id"].as_str().unwrap().to_owned()));
            }
            cursor = page["nextCursor"].clone();
        }
        assert!(cursor.is_null());
        assert_eq!(ids.len(), 29);
        assert!(!service.conversation(id).await.unwrap().unread);
        assert!(service.processes.lock().await.is_empty());
        assert_eq!(
            service.session_models(id, false, false).await.unwrap()["defaultModel"],
            "gpt-6.1-sol"
        );
        assert_eq!(
            calls
                .lock()
                .await
                .iter()
                .filter(|call| call["subtype"] == "discover")
                .count(),
            1,
            "高频列表刷新复用发现缓存"
        );
        let restored = Claude::new(directory.path()).unwrap();
        discovery_fixture(&restored, rows, frame.clone()).await;
        assert_eq!(
            restored.detail(id).await.unwrap()["thread"]["nativeId"],
            native
        );
        assert!(!restored.conversation(id).await.unwrap().unread);
        let mut wrong = frame.clone();
        wrong["native_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(
            restored
                .apply_shared_snapshot(id, "native-history-check", &wrong, json!({}), false)
                .await
                .is_err()
        );
        assert_eq!(restored.conversation(id).await.unwrap().native_id, native);
        assert_eq!(restored.conversation(id).await.unwrap().turns.len(), 29);
    }

    #[tokio::test]
    async fn claude_restart_preserves_native_identity_project_and_wsl_without_replaying() {
        let (directory, service, c) = fixture();
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let restored = Claude::new(directory.path()).unwrap();
        let identity = restored.conversation(&c.id).await.unwrap();
        assert_eq!(identity.native_id, c.native_id);
        assert_eq!(identity.project_id, "shared-project");
        assert_eq!(identity.config.distribution, "codex-elves-claude-test");
        assert_eq!(identity.turns[0]["status"], "interrupted");
        assert!(restored.processes.lock().await.is_empty());
    }

    #[tokio::test]
    async fn claude_old_process_result_cannot_complete_the_current_turn() {
        let (_directory, service, c) = fixture();
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        service
            .ingest(
                &c.id,
                "old-process",
                json!({"type":"result","is_error":false}),
            )
            .await
            .unwrap();
        assert!(service.conversation(&c.id).await.unwrap().active());
        service
            .ingest(
                &c.id,
                "new-process",
                json!({"type":"result","is_error":false}),
            )
            .await
            .unwrap();
        assert!(!service.conversation(&c.id).await.unwrap().active());
    }

    #[tokio::test]
    async fn claude_sidebar_uses_the_existing_project_in_recent_and_project_views() {
        let (_directory, service, mut c) = fixture();
        c.pinned = true;
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let mut sidebar = json!({"recent":[{"id":"codex-existing"}],"pinnedThreads":[],
            "projects":[{"id":"shared-project","name":"原项目","threads":[{"id":"codex-existing"}]}]});
        service.merge_sidebar(&mut sidebar).await;
        assert_eq!(sidebar["projects"].as_array().unwrap().len(), 1);
        assert_eq!(sidebar["recent"][1]["id"], c.id);
        assert_eq!(sidebar["projects"][0]["threads"][1]["id"], c.id);
        assert_eq!(sidebar["pinnedThreads"][0]["id"], c.id);
        assert_eq!(sidebar["projects"][0]["name"], "原项目");
    }

    #[test]
    fn claude_frames_merge_stream_and_complete_message_without_duplicates() {
        let mut turn = json!({"items":[]});
        project_frame(
            &mut turn,
            &json!({"type":"stream_event","event":{"type":"message_start","message":{"id":"msg-1"}}}),
        );
        project_frame(
            &mut turn,
            &json!({"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好"}}}),
        );
        project_frame(
            &mut turn,
            &json!({"type":"assistant","message":{"id":"msg-1","content":[{"type":"text","text":"你好 Claude"}]}}),
        );
        assert_eq!(turn["items"].as_array().unwrap().len(), 1);
        assert_eq!(turn["items"][0]["text"], "你好 Claude");
        project_frame(
            &mut turn,
            &json!({"type":"assistant","message":{"id":"msg-1",
            "content":[{"type":"text","text":"你好 Claude"},{"type":"text","text":"第二个独立正文块"}]}}),
        );
        assert_eq!(turn["items"].as_array().unwrap().len(), 2);
        assert_eq!(turn["items"][0]["text"], "你好 Claude");
        assert_eq!(turn["items"][1]["text"], "第二个独立正文块");
    }

    #[test]
    fn native_block_identity_keeps_distinct_text_and_reasoning_with_equal_prefixes() {
        for (kind, field) in [("text", "text"), ("thinking", "thinking")] {
            let mut turn = json!({"items":[]});
            for (index, text) in [(0, "已完成"), (1, "已完成更多内容"), (2, "已完成")]
            {
                project_frame(
                    &mut turn,
                    &json!({"type":"assistant","apiBlockIndex":index,
                    "message":{"id":"same-response","content":[{"type":kind,field:text}]}}),
                );
            }
            assert_eq!(
                turn["items"].as_array().unwrap().len(),
                3,
                "相同前缀不代表同一原生块"
            );
            assert_eq!(turn["items"][0]["id"], "same-response-0");
            assert_eq!(turn["items"][1]["id"], "same-response-1");
            assert_eq!(turn["items"][2]["id"], "same-response-2");
            let before = turn.clone();
            project_frame(
                &mut turn,
                &json!({"type":"assistant","apiBlockIndex":1,
                "message":{"id":"same-response","content":[{"type":kind,field:"已完成更多内容"}]}}),
            );
            assert_eq!(turn, before, "同块重读仍幂等");
        }
    }

    #[test]
    fn claude_models_use_reported_capabilities_and_do_not_invent_effort() {
        let catalog = normalize_models(&json!({"models":[
            {"value":"opus","displayName":"Opus","supportedEffortLevels":["low","high","max"]},
            {"value":"haiku","displayName":"Haiku"}]}));
        assert_eq!(
            catalog["data"][0]["supportedReasoningEfforts"][2]["reasoningEffort"],
            "max"
        );
        assert_eq!(catalog["data"][1]["supportedReasoningEfforts"], json!([]));
        let single = normalize_models(&json!({"models":[
            {"value":"default","resolvedModel":"model-a"},
            {"value":"model-a","resolvedModel":"model-a","supportedEffortLevels":["low"]}]}));
        assert_eq!(single["data"].as_array().unwrap().len(), 1);
        assert_eq!(single["defaultModel"], "model-a");
    }

    #[test]
    fn claude_followup_preserves_effort_but_explicit_reset_and_model_switch_do_not_inherit_it() {
        let (_directory, _service, mut c) = fixture();
        c.model = "gpt-6.1-sol".into();
        c.effort = "max".into();
        let catalog = normalize_models(&json!({"models":[
            {"value":"gpt-6.1-sol","supportedEffortLevels":["medium","high","max"]},
            {"value":"haiku","supportedEffortLevels":["low","high"]}]}));
        for payload in [json!({"prompt":"排队补充"}), json!({"model":"gpt-6.1-sol"})] {
            assert_eq!(
                model_selection(&c, &payload, &catalog).unwrap(),
                ("gpt-6.1-sol", "max")
            );
        }
        assert_eq!(
            model_selection(&c, &json!({"reasoningEffort":""}), &catalog).unwrap(),
            ("gpt-6.1-sol", "")
        );
        assert_eq!(
            model_selection(&c, &json!({"model":"haiku"}), &catalog).unwrap(),
            ("haiku", "")
        );
        assert!(
            model_selection(
                &c,
                &json!({"model":"haiku","reasoningEffort":"max"}),
                &catalog
            )
            .is_err()
        );
    }

    #[test]
    fn claude_model_aliases_accept_reported_effort_and_preserve_same_model_selection() {
        let (_directory, _service, mut c) = fixture();
        c.model = "claude-opus-5-5".into();
        c.effort = "max".into();
        let catalog = normalize_models(&json!({"models":[
            {"value":"opus","resolvedModel":"claude-opus-5-5","supportedEffortLevels":["low","medium","high","xhigh","max"]},
            {"value":"sonnet","resolvedModel":"claude-sonnet-5-5","supportedEffortLevels":["medium","high"]},
            {"value":"haiku","resolvedModel":"claude-haiku-4-5","supportedEffortLevels":[]}
        ]}));
        assert_eq!(
            model_selection(&c, &json!({}), &catalog).unwrap(),
            ("claude-opus-5-5", "max")
        );
        assert_eq!(
            model_selection(&c, &json!({"model":"opus"}), &catalog).unwrap(),
            ("opus", "max")
        );
        assert_eq!(
            model_selection(&c, &json!({"reasoningEffort":"high"}), &catalog).unwrap(),
            ("claude-opus-5-5", "high")
        );
        assert_eq!(
            model_selection(&c, &json!({"reasoningEffort":""}), &catalog).unwrap(),
            ("claude-opus-5-5", "")
        );
        assert_eq!(
            model_selection(&c, &json!({"model":"sonnet"}), &catalog).unwrap(),
            ("sonnet", "")
        );
        for payload in [
            json!({"model":"claude-sonnet-5-5","reasoningEffort":"max"}),
            json!({"model":"haiku","reasoningEffort":"high"}),
            json!({"model":"claude-opus-unreported","reasoningEffort":"max"}),
        ] {
            assert!(model_selection(&c, &payload, &catalog).is_err());
        }
        c.model = "opus".into();
        assert_eq!(
            model_selection(&c, &json!({"model":"claude-opus-5-5"}), &catalog).unwrap(),
            ("claude-opus-5-5", "max")
        );
    }

    #[test]
    fn claude_model_exact_identity_precedes_an_alias_with_different_capabilities() {
        let (_directory, _service, mut c) = fixture();
        c.model = "claude-opus-5-5".into();
        c.effort = "low".into();
        let catalog = normalize_models(&json!({"models":[
            {"value":"opus","resolvedModel":"claude-opus-5-5","supportedEffortLevels":["high","max"]},
            {"value":"claude-opus-5-5","supportedEffortLevels":["low"]}
        ]}));
        assert_eq!(
            model_selection(&c, &json!({}), &catalog).unwrap(),
            ("claude-opus-5-5", "low")
        );
        assert!(model_selection(&c, &json!({"reasoningEffort":"max"}), &catalog).is_err());
    }

    #[test]
    fn claude_permission_requests_preserve_native_questions_and_only_offer_once() {
        let q = permission_request(
            "thread",
            "req",
            &json!({"subtype":"can_use_tool","tool_name":"AskUserQuestion",
            "input":{"questions":[{"question":"选择目录","options":[{"label":"项目"}]}]}}),
        )
        .unwrap();
        assert_eq!(q["params"]["questions"][0]["id"], "选择目录");
        let p = permission_request(
            "thread",
            "req",
            &json!({"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"pwd"}}),
        )
        .unwrap();
        assert!(
            !p["params"]["availableDecisions"]
                .as_array()
                .unwrap()
                .contains(&json!("acceptForSession"))
        );
        assert!(input_content(&json!({"prompt":"test","goalMode":true})).is_err());
    }

    #[tokio::test]
    async fn shared_unread_tracks_new_content_not_previews_status_or_reconnections() {
        let (directory, service, mut c) = fixture();
        let (_auth_directory, auth) = crate::remote_access::tests::service_dispatch_fixture().await;
        c.shared = true;
        c.turns.clear();
        let process = |generation: &str| {
            let (writer, _) = mpsc::channel(1);
            Arc::new(Process {
                _child: Mutex::new(None),
                writer,
                channel: Arc::new(Channel {
                    live: AtomicBool::new(true),
                    generation: generation.into(),
                    pending: Mutex::new(HashMap::new()),
                }),
                shared: true,
                initialization: json!({}),
            })
        };
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        service
            .processes
            .lock()
            .await
            .insert(c.id.clone(), process("first"));
        let mut frame = json!({"type":"shared_snapshot","native_id":c.native_id,
        "job_id":&c.native_id[..8],"native_status":{"tempo":"active"},
        "clients":{"native-user":"web-client"},"pending":[],"requests":{},
        "messages":[
            {"type":"user","uuid":"native-user","message":{"content":"问题"}},
            {"type":"assistant","message":{"id":"reply","content":[{"type":"text","text":"回复"}]}}
        ]});
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        assert!(service.conversation(&c.id).await.unwrap().unread);
        service
            .command("markRead", &json!({"threadId":c.id}), &auth)
            .await
            .unwrap();
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        assert!(!service.conversation(&c.id).await.unwrap().unread);
        service
            .ingest_shared_preview(
                &c.id,
                "first",
                &json!({
            "native_user_id":"native-user","text":"临时预览"}),
            )
            .await
            .unwrap();
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        assert!(
            !service.conversation(&c.id).await.unwrap().unread,
            "移除临时预览不是新消息"
        );
        frame["native_status"]["tempo"] = json!("idle");
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        assert!(
            !service.conversation(&c.id).await.unwrap().unread,
            "仅执行状态变化不能恢复蓝点"
        );
        service
            .processes
            .lock()
            .await
            .insert(c.id.clone(), process("reconnected"));
        service
            .ingest_shared(&c.id, "reconnected", &frame)
            .await
            .unwrap();
        assert!(
            !service.conversation(&c.id).await.unwrap().unread,
            "连接代次变化不是新消息"
        );
        let restored = Claude::new(directory.path()).unwrap();
        restored
            .processes
            .lock()
            .await
            .insert(c.id.clone(), process("restored"));
        frame["clients"]["native-user"] = json!("reconciled-client");
        restored
            .ingest_shared(&c.id, "restored", &frame)
            .await
            .unwrap();
        let read = restored.conversation(&c.id).await.unwrap();
        assert!(!read.unread, "重启及客户端身份补齐后仍保持已读");
        let mut sidebar = json!({"recent":[],"pinnedThreads":[],"projects":[]});
        restored.merge_sidebar(&mut sidebar).await;
        assert_eq!(sidebar["recent"][0]["hasUnreadTurn"], false);
        frame["messages"].as_array_mut().unwrap().push(json!({
            "type":"assistant","message":{"id":"new-reply","content":[{"type":"text","text":"新增回复"}]}}));
        restored
            .ingest_shared(&c.id, "restored", &frame)
            .await
            .unwrap();
        assert!(
            restored.conversation(&c.id).await.unwrap().unread,
            "真正新增回复仍提示未读"
        );
        restored
            .command("markRead", &json!({"threadId":c.id}), &auth)
            .await
            .unwrap();
        frame["messages"][2]["message"]["content"][0]["text"] = json!("新增回复的后续正文");
        restored
            .ingest_shared(&c.id, "restored", &frame)
            .await
            .unwrap();
        assert!(
            restored.conversation(&c.id).await.unwrap().unread,
            "同一消息新增正文也不能漏报"
        );
    }

    #[tokio::test]
    async fn shared_usage_persists_notifies_and_updates_after_compaction_without_native_commands() {
        let (directory, service, mut c) = fixture();
        let (_auth_directory, auth) = crate::remote_access::tests::service_dispatch_fixture().await;
        let _ = service.controller.set(Arc::downgrade(&auth.controller));
        let mut events = auth.controller.events.subscribe();
        c.shared = true;
        c.turns.clear();
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let process = |generation: &str| {
            let (writer, _) = mpsc::channel(1);
            Arc::new(Process {
                _child: Mutex::new(None),
                writer,
                shared: true,
                initialization: json!({}),
                channel: Arc::new(Channel {
                    live: AtomicBool::new(true),
                    generation: generation.into(),
                    pending: Mutex::new(HashMap::new()),
                }),
            })
        };
        service
            .processes
            .lock()
            .await
            .insert(c.id.clone(), process("first"));
        let mut frame = json!({"type":"shared_snapshot","native_id":c.native_id,"job_id":&c.native_id[..8],
        "native_status":{"tempo":"active"},"clients":{},"pending":[],"requests":{},"messages":[
            {"type":"user","uuid":"user","message":{"content":"问题"}},
            {"type":"assistant","message":{"id":"reply","model":"native-model","content":[{"type":"text","text":"回复"}],
                "usage":{"input_tokens":100,"cache_read_input_tokens":1000,"cache_creation_input_tokens":200,"output_tokens":50}}}
        ]});
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        let mut usage = service.conversation(&c.id).await.unwrap().view()["tokenUsage"].clone();
        assert_eq!(usage["last"]["totalTokens"], 1300);
        assert!(usage["modelContextWindow"].is_null());
        let notifications: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| event["method"] == "thread/tokenUsage/updated")
            .collect();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["params"]["tokenUsage"], usage);
        frame["context_configuration"] = json!({"maxContextTokens":600000,"model":"native-model",
            "source":{"kind":"configuration","key":"CLAUDE_CODE_MAX_CONTEXT_TOKENS","scope":"user-settings.env","model":"native-model"},
            "compaction":{"windowTokens":600000,"triggerPercent":90},
            "native":{"model":"native-model","context_window_size":1000000,"used_percentage":0.13,
                "current_usage":{"input_tokens":100,"cache_read_input_tokens":1000,"cache_creation_input_tokens":200}}});
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        usage = service.conversation(&c.id).await.unwrap().view()["tokenUsage"].clone();
        assert_eq!(usage["modelContextWindow"], 1000000);
        assert_eq!(usage["usedPercentage"], 0.13);
        assert_eq!(usage["compaction"]["windowTokens"], 600000);
        assert_eq!(usage["last"]["totalTokens"], 1300);
        let notifications: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| event["method"] == "thread/tokenUsage/updated")
            .collect();
        assert_eq!(notifications.len(), 1, "仅容量补齐也通过原有用量通知更新");
        assert_eq!(notifications[0]["params"]["tokenUsage"], usage);
        service.ingest_shared(&c.id, "first", &frame).await.unwrap();
        assert!(
            !std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| event["method"] == "thread/tokenUsage/updated")
        );
        let restored = Claude::new(directory.path()).unwrap();
        assert_eq!(
            restored.conversation(&c.id).await.unwrap().view()["tokenUsage"],
            usage
        );
        restored
            .processes
            .lock()
            .await
            .insert(c.id.clone(), process("reconnected"));
        frame["messages"].as_array_mut().unwrap().extend([
            json!({"type":"user","uuid":"summary","isCompactSummary":true,"message":{"content":"内部摘要"}}),
            json!({"type":"assistant","message":{"id":"after","model":"native-model","content":[],
                "usage":{"input_tokens":11,"cache_read_input_tokens":22,"cache_creation_input_tokens":3,"output_tokens":4}}})]);
        restored
            .ingest_shared(&c.id, "reconnected", &frame)
            .await
            .unwrap();
        let reconnected = restored.conversation(&c.id).await.unwrap().token_usage;
        assert_eq!(reconnected["modelContextWindow"], 1000000);
        assert!(
            reconnected["usedPercentage"].is_null(),
            "旧请求的原生百分比不能覆盖新请求"
        );
        assert_eq!(
            reconnected["modelContextWindowSource"],
            usage["modelContextWindowSource"]
        );
        assert_eq!(reconnected["last"]["totalTokens"], 36);
        frame["context_configuration"] = json!({});
        restored
            .ingest_shared(&c.id, "reconnected", &frame)
            .await
            .unwrap();
        assert!(
            restored.conversation(&c.id).await.unwrap().token_usage["modelContextWindow"].is_null(),
            "明确清除配置后不能继续复用旧容量"
        );
        frame["native_status"]["tempo"] = json!("idle");
        frame["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"system","subtype":"turn_duration"}));
        restored
            .ingest_shared(&c.id, "reconnected", &frame)
            .await
            .unwrap();
        assert_eq!(
            restored.conversation(&c.id).await.unwrap().token_usage["last"]["totalTokens"],
            36
        );
    }

    #[tokio::test]
    async fn sdk_result_reports_only_the_last_models_capacity_without_replacing_request_usage() {
        for matching in [true, false] {
            let (_directory, service, mut c) = fixture();
            c.token_usage = json!({"model":"native-model","modelContextWindow":null,
                "last":{"totalTokens":1300}});
            let id = c.id.clone();
            service
                .change(|saved| {
                    saved.conversations.insert(id.clone(), c);
                    Ok(())
                })
                .await
                .unwrap();
            let model = if matching {
                "native-model"
            } else {
                "other-model"
            };
            let mut frame = json!({"type":"result","is_error":false,"modelUsage":{}});
            frame["modelUsage"][model] = json!({"contextWindow":1000000,"inputTokens":999999});
            service.ingest(&id, "new-process", frame).await.unwrap();
            let usage = service.conversation(&id).await.unwrap().token_usage;
            assert_eq!(
                usage["last"]["totalTokens"], 1300,
                "SDK 累计量不能覆盖最后请求"
            );
            assert_eq!(
                usage["modelContextWindow"],
                if matching {
                    json!(1000000)
                } else {
                    Value::Null
                }
            );
        }
    }

    #[test]
    fn native_capacity_uses_only_reports_and_keeps_compaction_configuration_separate() {
        let mut messages = vec![
            json!({"type":"assistant","message":{"id":"request","model":"gpt-6.1-sol",
            "usage":{"input_tokens":181,"cache_read_input_tokens":108032,"cache_creation_input_tokens":0,"output_tokens":545}}}),
        ];
        let mut initialization = json!({"models":[{"value":"gpt-6.1-sol"}],"contextConfiguration":{
            "maxContextTokens":600000,"model":"gpt-6.1-sol",
            "compaction":{"windowTokens":600000,"triggerPercent":90},
            "source":{"kind":"configuration","key":"CLAUDE_CODE_MAX_CONTEXT_TOKENS","scope":"user-settings.env","model":"gpt-6.1-sol"}}});
        let usage = native_token_usage(&messages, &initialization);
        assert!(
            usage["modelContextWindow"].is_null(),
            "配置值不能作为原生容量"
        );
        assert_eq!(usage["compaction"]["windowTokens"], 600000);
        assert_eq!(usage["compaction"]["triggerPercent"], 90);
        assert_eq!(
            usage["last"]["totalTokens"], 108213,
            "真实原生输入与缓存口径保持不变"
        );
        initialization["models"][0]["contextWindow"] = json!(700000);
        let reported = native_token_usage(&messages, &initialization);
        assert_eq!(
            reported["modelContextWindow"], 700000,
            "模型目录原生报告与压缩配置独立"
        );
        assert_eq!(reported["modelContextWindowSource"]["kind"], "native");
        initialization["modelUsage"] = json!({"gpt-6.1-sol":{"contextWindow":800000}});
        assert_eq!(
            native_token_usage(&messages, &initialization)["modelContextWindow"],
            800000
        );
        initialization["contextConfiguration"]["native"] = json!({"model":"gpt-6.1-sol","context_window_size":1000000,"used_percentage":10.8,
                "current_usage":{"input_tokens":181,"cache_read_input_tokens":108032,"cache_creation_input_tokens":0}});
        let statusline = native_token_usage(&messages, &initialization);
        assert_eq!(statusline["modelContextWindow"], 1000000);
        assert_eq!(statusline["usedPercentage"], 10.8);
        initialization["contextConfiguration"]["native"]["context_window_size"] = json!(0);
        let fallback = native_token_usage(&messages, &initialization);
        assert_eq!(fallback["modelContextWindow"], 800000);
        assert!(
            fallback["usedPercentage"].is_null(),
            "无效原生窗口的百分比不能配到其他容量来源"
        );
        messages[0]["message"]["model"] = json!("other-model");
        assert!(
            native_token_usage(&messages, &initialization)["modelContextWindow"].is_null(),
            "模型切换不得套旧模型的原生能力或配置缓存"
        );
        initialization["contextConfiguration"]["model"] = json!("other-model");
        initialization["contextConfiguration"]["source"]["model"] = json!("other-model");
        initialization["contextConfiguration"]["maxContextTokens"] = json!(400000);
        assert!(native_token_usage(&messages, &initialization)["modelContextWindow"].is_null());
        for invalid in [
            json!(0),
            json!(-1),
            json!(false),
            json!("600000"),
            json!(9007199254740992_u64),
        ] {
            initialization["contextConfiguration"]["maxContextTokens"] = invalid;
            assert!(native_token_usage(&messages, &initialization)["modelContextWindow"].is_null());
        }
        initialization["contextConfiguration"] =
            json!({"autoCompactWindow":600000,"percentage":95});
        assert!(native_token_usage(&messages, &initialization)["modelContextWindow"].is_null());
    }

    #[test]
    fn native_usage_uses_the_last_request_cache_and_iteration_without_inventing_capacity() {
        let mut messages = vec![
            json!({"type":"assistant","message":{"id":"same-request","model":"native-model",
            "usage":{"input_tokens":100,"cache_read_input_tokens":1000,"cache_creation_input_tokens":200,"output_tokens":50}}}),
        ];
        messages.push(messages[0].clone());
        let usage = native_token_usage(&messages, &json!({}));
        assert_eq!(
            usage["last"]["totalTokens"], 1300,
            "缓存计入，输出及重复分块不相加"
        );
        assert!(usage["modelContextWindow"].is_null());
        let capacity = native_token_usage(
            &messages,
            &json!({"models":[{"value":"alias","resolvedModel":"native-model","contextWindow":4096}]}),
        );
        assert_eq!(capacity["modelContextWindow"], 4096);
        assert!(
            native_token_usage(
                &messages,
                &json!({"models":[{"value":"other","contextWindow":8192}]})
            )["modelContextWindow"]
                .is_null()
        );
        messages[1]["message"]["usage"]["iterations"] = json!([
            {"type":"message","input_tokens":25,"cache_read_input_tokens":300,"cache_creation_input_tokens":5,"output_tokens":10},
            {"type":"compaction","input_tokens":99999}]);
        assert_eq!(
            native_token_usage(&messages, &json!({}))["last"]["totalTokens"],
            330
        );
        messages
            .push(json!({"type":"user","isCompactSummary":true,"message":{"content":"内部摘要"}}));
        assert_eq!(
            native_token_usage(&messages, &json!({}))["last"]["totalTokens"],
            330
        );
        messages.push(json!({"type":"assistant","message":{"id":"after-compact","model":"native-model",
            "usage":{"input_tokens":11,"cache_read_input_tokens":22,"cache_creation_input_tokens":3,"output_tokens":4}}}));
        assert_eq!(
            native_token_usage(&messages, &json!({}))["last"]["totalTokens"],
            36
        );
    }

    #[test]
    fn shared_web_images_remain_complete_deduplicated_and_hidden_paths_survive_reconnect() {
        let paths: Vec<_> = (0..8)
            .map(|i| format!("/native/remote-attachments/client/{i}.png"))
            .collect();
        let urls: Vec<_> = (0..8)
            .map(|i| format!("data:image/png;base64,{i}AAA"))
            .collect();
        let prompt = format!("八张图片\n@{}", paths.join("\n@"));
        let images: Vec<_> = paths
            .iter()
            .zip(&urls)
            .map(|(path, url)| json!({"filename":path,"url":url}))
            .collect();
        let frame = json!({"clients":{"native-user":"client"},"native_status":{"tempo":"idle"},"messages":[
            {"type":"user","uuid":"native-user","message":{"content":prompt},"web_images":images,"remote_upload_paths":paths},
            {"type":"attachment","filename":paths[7],"remote_upload":true,"message":{"content":[{"type":"image","url":urls[7]}]}}
        ]});
        let turns = project_shared_history(&frame, "first");
        let parts = turns[0]["items"][0]["content"].as_array().unwrap();
        assert_eq!(
            parts.iter().filter(|part| part["type"] == "image").count(),
            8
        );
        assert_eq!(parts[0]["text"], "八张图片");
        let mut missing = frame.clone();
        missing["messages"][0]["web_images"] = json!([]);
        let mut restored = project_shared_history(&missing, "reconnected");
        restore_shared_images(&turns, &mut restored);
        assert_eq!(
            restored[0]["items"][0]["content"],
            turns[0]["items"][0]["content"]
        );
        assert!(!shared_history_has_new_content(&turns, &restored));
        let mut legacy = frame.clone();
        legacy["messages"][0]
            .as_object_mut()
            .unwrap()
            .remove("web_images");
        legacy["messages"][0]
            .as_object_mut()
            .unwrap()
            .remove("remote_upload_paths");
        let previous = project_shared_history(&legacy, "old");
        assert!(
            !shared_history_has_new_content(&previous, &turns),
            "补回已受理原图及隐藏路径不是新消息"
        );
        let mut extended = turns.clone();
        extended[0]["items"][0]["content"][0]["text"] = json!("八张图片，新增正文");
        assert!(shared_history_has_new_content(&turns, &extended));
    }

    #[test]
    fn shared_compact_summary_is_not_a_user_but_literal_quotes_and_restored_items_survive() {
        let text = "This session is being continued from a previous conversation.";
        let mut frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"real-user","message":{"content":"真实问题"}},
            {"type":"user","uuid":"compact","isCompactSummary":true,
                "isVisibleInTranscriptOnly":true,"message":{"content":text}},
            {"type":"assistant","message":{"id":"reply","content":[
                {"type":"text","text":"压缩后回复"},
                {"type":"tool_use","id":"tool-after","name":"Read","input":{}}]}},
            {"type":"user","uuid":"tool-result","message":{"content":[
                {"type":"tool_result","tool_use_id":"tool-after","content":"工具结果"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":42},
            {"type":"user","uuid":"quoted-user","message":{"content":text}}
        ]});
        let mut legacy = frame.clone();
        legacy["messages"][1]
            .as_object_mut()
            .unwrap()
            .remove("isCompactSummary");
        legacy["messages"][1]
            .as_object_mut()
            .unwrap()
            .remove("isVisibleInTranscriptOnly");
        let previous = project_shared_history(&legacy, "old");
        let current = project_shared_history(&frame, "reconnected");
        assert_eq!(previous.len(), 3);
        assert_eq!(current.len(), 2, "仅原生内部摘要被过滤");
        assert_eq!(current[0]["_nativeUserId"], "real-user");
        assert_eq!(
            current[0]["_nativeDone"], true,
            "完成标记归真实用户，不再挂在内部摘要轮次"
        );
        assert_eq!(current[0]["items"][1]["text"], "压缩后回复");
        assert_eq!(
            current[0]["items"][2]["contentItems"][0]["text"],
            "工具结果"
        );
        assert_eq!(current[1]["items"][0]["content"][0]["text"], text);
        assert!(
            !shared_history_has_new_content(&previous, &current),
            "摘要移除及同一回复、工具恢复归属不能重新置未读"
        );
        frame["messages"][2]["message"]["content"][0]["text"] = json!("压缩后回复新增正文");
        assert!(shared_history_has_new_content(
            &current,
            &project_shared_history(&frame, "next")
        ));
        let cross = json!({"clients":{},"native_status":{"tempo":"idle"},"messages":[
            {"type":"user","uuid":"real","message":{"content":"真实问题"}},
            {"type":"assistant","message":{"id":"tool-call","content":[{"type":"tool_use","id":"cross-tool","name":"Read","input":{}}]}},
            {"type":"user","uuid":"compact","isCompactSummary":true,"message":{"content":text}},
            {"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"cross-tool","content":"原有结果"}]}},
            {"type":"assistant","message":{"id":"after-tool","content":[{"type":"text","text":"已有回复"}]}}
        ]});
        let mut legacy = cross.clone();
        legacy["messages"][2]
            .as_object_mut()
            .unwrap()
            .remove("isCompactSummary");
        let previous = project_shared_history(&legacy, "old");
        let repaired = repaired_history_baseline(&previous, &cross).unwrap();
        let current = project_shared_history(&cross, "new");
        assert_eq!(
            current[0]["items"][1]["contentItems"][0]["text"],
            "原有结果"
        );
        assert!(
            !shared_history_has_new_content(&repaired, &current),
            "工具调用在摘要前、结果在摘要后时也保持已读"
        );
        let mut updated = cross.clone();
        updated["messages"][4]["message"]["content"][0]["text"] = json!("已有回复新增正文");
        assert!(shared_history_has_new_content(
            &repaired,
            &project_shared_history(&updated, "new")
        ));
    }

    #[test]
    fn shared_native_interruption_keeps_original_question_tools_and_real_quotes() {
        let frame = interruption_fixture();
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 2, "中断不能制造第三条用户消息");
        assert_eq!(turns[0]["id"], "real-user");
        assert_eq!(turns[0]["status"], "interrupted");
        assert_eq!(turns[0]["nativeInterruption"]["id"], "control");
        assert_eq!(turns[0]["nativeInterruption"]["responseId"], "response-one");
        let items = turns[0]["items"].as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|item| item["type"] == "userMessage")
                .count(),
            1
        );
        assert!(
            items
                .iter()
                .any(|item| item["type"] == "reasoning" && item["summary"][0] == "保留思考")
        );
        let tool = items.iter().find(|item| item["id"] == "tool-one").unwrap();
        assert_eq!(tool["status"], "completed");
        assert_eq!(tool["contentItems"][0]["text"], "原生工具记录");
        assert_eq!(
            turns[1]["items"][0]["content"][0]["text"],
            "请解释 [Request interrupted by user]"
        );
        assert_eq!(
            turns[1]["items"][1]["text"], "No response requested.",
            "真实模型正文保留"
        );
        assert!(!items.iter().any(|item| item["id"] == "synthetic-0"));
        assert_eq!(turns, project_shared_history(&frame, "connection"));
    }

    fn interruption_fixture() -> Value {
        json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"real-user","timestampMs":10,"message":{"content":"原来的问题"}},
            {"type":"assistant","uuid":"assistant-one","message":{"id":"response-one","model":"real-model",
                "content":[{"type":"thinking","thinking":"保留思考"},
                    {"type":"tool_use","id":"tool-one","name":"Read","input":{"file_path":"README.md"}},
                    {"type":"text","text":"尚未完成的正文"}]}},
            {"type":"user","uuid":"control","parentUuid":"assistant-one","interruptedMessageId":"response-one",
                "timestampMs":20,"message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}},
            {"type":"user","uuid":"result","message":{"content":[
                {"type":"tool_result","tool_use_id":"tool-one","content":"原生工具记录"}]}},
            {"type":"assistant","uuid":"placeholder","isApiErrorMessage":false,
                "message":{"id":"synthetic","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}},
            {"type":"user","uuid":"quoted-user","timestampMs":30,"message":{"content":"请解释 [Request interrupted by user]"}},
            {"type":"assistant","message":{"id":"real-response","model":"real-model",
                "content":[{"type":"text","text":"No response requested."}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":40}
        ]})
    }

    #[test]
    fn shared_native_interruption_preserves_unmarked_literal_and_synthetic_errors() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"literal","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}},
            {"type":"assistant","isApiErrorMessage":true,"message":{"id":"error","model":"<synthetic>",
                "content":[{"type":"text","text":"No response requested."}]}},
            {"type":"system","subtype":"turn_duration"}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 1);
        assert_eq!(
            turns[0]["items"][0]["content"][0]["text"],
            "[Request interrupted by user]"
        );
        assert_eq!(turns[0]["status"], "failed");
        assert_eq!(turns[0]["items"][1]["text"], "No response requested.");
        assert!(turns[0]["nativeInterruption"].is_null());
    }

    #[test]
    fn shared_native_interruption_targets_response_without_stopping_a_new_question() {
        let frame = json!({"native_status":{"tempo":"active"},"clients":{},"messages":[
            {"type":"user","uuid":"first","message":{"content":"旧问题"}},
            {"type":"assistant","message":{"id":"response-one","content":[{"type":"text","text":"旧回复"}]}},
            {"type":"system","subtype":"turn_duration"},
            {"type":"user","uuid":"current","message":{"content":"新问题"}},
            {"type":"user","uuid":"late-control","interruptedMessageId":"response-one",
                "message":{"content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["status"], "interrupted");
        assert_eq!(turns[1]["status"], "inProgress");
        assert!(turns[1]["nativeInterruption"].is_null());
    }

    #[test]
    fn shared_native_interruption_repair_keeps_read_state_but_real_new_content_is_unread() {
        let frame = interruption_fixture();
        let mut legacy = frame.clone();
        legacy["messages"][2]
            .as_object_mut()
            .unwrap()
            .remove("interruptedMessageId");
        legacy["messages"][2]
            .as_object_mut()
            .unwrap()
            .remove("parentUuid");
        let previous = project_shared_history(&legacy, "old");
        assert_eq!(previous.len(), 3);
        let current = project_shared_history(&frame, "new");
        let baseline = repaired_history_baseline(&previous, &frame).unwrap();
        assert!(!shared_history_has_new_content(&baseline, &current));
        let mut updated = frame.clone();
        updated["messages"].as_array_mut().unwrap().extend([
            json!({"type":"user","uuid":"new-user","message":{"content":"真正新问题"}}),
            json!({"type":"assistant","message":{"id":"new-response","content":[{"type":"text","text":"真正新回复"}]}}),
        ]);
        let baseline = repaired_history_baseline(&previous, &updated).unwrap();
        assert!(shared_history_has_new_content(
            &baseline,
            &project_shared_history(&updated, "new")
        ));
    }

    #[test]
    fn shared_native_history_keeps_cli_messages_and_separate_blocks_without_duplicates() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{"user-web":"web-client"},
        "messages":[
            {"type":"user","uuid":"user-web","message":{"content":"网页消息"}},
            {"type":"assistant","apiBlockIndex":0,"message":{"id":"msg-web",
                "content":[{"type":"thinking","thinking":"思考"}]}},
            {"type":"assistant","apiBlockIndex":1,"message":{"id":"msg-web",
                "content":[{"type":"text","text":"网页回复"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":20},
            {"type":"user","uuid":"user-cli","message":{"content":"CLI 消息"}},
            {"type":"assistant","message":{"id":"msg-cli",
                "content":[{"type":"tool_use","id":"tool-one","name":"Write","input":{"file_path":"a.txt"}}]}},
            {"type":"user","uuid":"result","message":{"content":[
                {"type":"tool_result","tool_use_id":"tool-one","content":"完成"}]}},
            {"type":"assistant","message":{"id":"msg-cli-reply",
                "content":[{"type":"text","text":"CLI 回复"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":40}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["id"], "web-client");
        assert_eq!(turns[0]["items"].as_array().unwrap().len(), 3);
        assert_eq!(turns[0]["items"][1]["type"], "reasoning");
        assert_eq!(turns[0]["items"][2]["text"], "网页回复");
        assert_eq!(turns[1]["id"], "user-cli");
        assert_eq!(turns[1]["items"][1]["status"], "completed");
        assert_eq!(turns, project_shared_history(&frame, "connection"));
    }

    #[test]
    fn shared_native_image_attachment_stays_in_its_user_message_on_reconnect() {
        let filename = "/config/remote-attachments/web-client/0.png";
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{"native-user":"web-client"},
        "messages":[
            {"type":"user","uuid":"native-user","message":{"content":format!("图片问题\n@{filename}")}},
            {"type":"attachment","uuid":"image","filename":filename,"remote_upload":true,
                "message":{"content":[{"type":"image","url":"data:image/png;base64,AQID"}]}},
            {"type":"assistant","message":{"id":"reply","content":[{"type":"text","text":"蓝色"}]}},
            {"type":"system","subtype":"turn_duration"}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0]["id"], "web-client");
        assert_eq!(
            turns[0]["items"][0]["content"],
            json!([
                {"type":"text","text":"图片问题"},
                {"type":"image","url":"data:image/png;base64,AQID"}
            ])
        );
        assert_eq!(turns[0]["items"][1]["text"], "蓝色");
        assert_eq!(turns, project_shared_history(&frame, "connection"));
    }

    #[tokio::test]
    async fn shared_queue_is_sendable_while_running_and_preserves_native_identity_on_restart() {
        let (directory, service, mut c) = fixture();
        c.shared = true;
        c.native_job_id = c.native_id[..8].into();
        c.native_status = json!({"tempo":"active"});
        c.queued_messages = vec![json!({"id":"next","text":"继续工作"})];
        let view = c.view();
        assert_eq!(view["capabilities"]["queue"], true);
        assert_eq!(view["capabilities"]["queueSendNow"], false);
        assert_eq!(view["capabilities"]["remoteQueue"], true);
        assert_eq!(view["queuedMessages"][0]["id"], "next");
        assert!(view["cliCommand"].as_str().unwrap().contains(" attach "));
        assert_eq!(view["nativeId"], c.native_id);
        c.native_status = json!({"tempo":"blocked"});
        c.turns[0]["status"] = json!("completed");
        assert!(!c.active());
        c.turns[0]["status"] = json!("inProgress");
        assert!(c.active());
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let restored = Claude::new(directory.path()).unwrap();
        let restored = restored.conversation(&c.id).await.unwrap();
        assert_eq!(restored.native_id, c.native_id);
        assert_eq!(restored.queued_messages, c.queued_messages);
        assert_eq!(restored.turns[0]["status"], "inProgress");
    }
}
