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

#[path = "claude_usage.rs"]
mod subscription_usage;

#[path = "claude_controls.rs"]
mod native_controls;

#[path = "claude_subagents.rs"]
mod subagents;

#[path = "claude_skills.rs"]
mod skills;

#[path = "claude_tools.rs"]
mod tools;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);
// 同一会话类型实例的完整模型目录缓存五分钟；新建与历史会话共用此期限。
const MODEL_CATALOG_TTL: Duration = Duration::from_secs(300);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_SHARED_FRAME_BYTES: usize = 64 * 1024 * 1024;

fn error(code: u16, message: impl Into<String>) -> anyhow::Error {
    ApiError::new(code, message.into()).into()
}

fn dispatched_unknown(mut failure: ApiError) -> ApiError {
    // Claude 原生回退/发送失败保留具体原因；结果未知不等于设备授权失效，且不能触发自动重发。
    failure.outcome_unknown = true;
    failure
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
            dispatched_unknown(error).into()
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
    // 用户要求权限只覆盖当前会话；缺字段的既有会话继续沿用原生权限，不自动提升访问范围。
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    permission_revision: u64,
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
    // 目标、压缩和命令回执均来自同一原生会话；重启只恢复观察，不重发命令。
    #[serde(default)]
    native_control: Value,
    #[serde(default)]
    native_clients: BTreeMap<String, String>,
    #[serde(default)]
    queued_messages: Vec<Value>,
    #[serde(default)]
    remote_queue: RemoteQueue,
    // 终端预览只属于当前连接的内存视图；其他设置落盘也不能把它保存成原生正式历史。
    #[serde(skip)]
    live_preview: Option<SharedPreview>,
    // 用户要求原生发现与网页创建区分：发现项重启/读历史只读，首次用户发送才接入原任务；本地标题修改不能被扫描覆盖。
    #[serde(default)]
    discovered: bool,
    #[serde(default)]
    native_title: String,
    // 历史编辑按原生 UUID 定位；保留回退分支已发生的数字用量，不能把历史回退当成消费撤销。
    #[serde(default)]
    native_history_head: String,
    #[serde(default)]
    spent_requests: BTreeMap<String, Value>,
}

#[derive(Clone)]
struct SharedPreview {
    generation: String,
    native_user_id: String,
    basis: String,
    text: String,
}

impl SharedPreview {
    fn matches(&self, turn: &Value) -> bool {
        matches!(
            turn["status"].as_str(),
            Some("inProgress" | "pending" | "unknown")
        ) && turn["_nativeDone"] != true
            && turn["_process"].as_str() == Some(self.generation.as_str())
            && turn["_nativeUserId"].as_str() == Some(self.native_user_id.as_str())
            && turn["_nativePreviewBasis"].as_str() == Some(self.basis.as_str())
    }
}

fn shared_preview_basis(frame: &Value) -> Option<&str> {
    frame["preview_basis"]
        .as_str()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn claude_backend() -> String {
    "claude".into()
}

impl Conversation {
    fn apply_native_settings(&mut self, settings: &Value) {
        if !settings.is_object() {
            return;
        }
        if !self.native_control.is_object() {
            self.native_control = json!({});
        }
        self.native_control["settings"] = settings.clone();
        // 用户要求区分命令受理与实际生效；只用原生确认后的值更新会话模型，待确认选择单独展示。
        if let Some(model) = settings["model"].as_str() {
            self.model = model.into();
        }
        if let Some(effort) = settings["effort"].as_str() {
            self.effort = effort.into();
        }
    }

    fn displayed_turn(&self, turn: &Value) -> Value {
        let mut value = turn.clone();
        if let Some(fields) = value.as_object_mut() {
            fields.remove("_metricsRequests");
            fields.remove("_metricsVersion");
            // 任务清单的原始调用与跨轮次状态只用于合成 plan，不发给网页。
            fields.remove("_claudePlanCalls");
            fields.remove("_claudeTasks");
            fields.remove("_claudeTaskSeed");
        }
        if let Some(items) = value["items"].as_array_mut() {
            for item in items
                .iter_mut()
                .filter(|item| item["type"] == "userMessage")
            {
                // 用户要求编辑任意历史用户消息；逐条保留原生定位，原生控制记录和内部摘要不可编辑。
                item["messageMetadata"] = json!({"turnId":turn["id"],"messageId":item["id"],
                    "editable":self.shared && turn["_nativeEditable"] == true});
            }
        }
        if let Some(preview) = self
            .live_preview
            .as_ref()
            .filter(|preview| preview.matches(turn))
            && let Some(items) = value["items"].as_array_mut()
        {
            items.push(json!({"id":format!("native-live-{}",preview.native_user_id),
                "type":"agentMessage","text":preview.text,
                "status":if turn["status"]=="inProgress" {"inProgress"} else {"unknown"},"_nativePreview":true}));
        }
        value
    }

    fn metrics(&self) -> Value {
        if self.spent_requests.is_empty() {
            return super::session_metrics::claude(&self.turns);
        }
        let mut turns = self.turns.clone();
        turns.push(json!({"_metricsRequests":self.spent_requests}));
        super::session_metrics::claude(&turns)
    }

    fn running_for_send(&self) -> bool {
        // 用户要求只有明确执行中才入队；共享会话的新鲜空闲/阻塞状态优先于尚未更新的历史轮次。
        if self.shared {
            // 共享状态未知时不能用旧历史的 inProgress 推断仍在执行，最终派发还会由原生入口复核。
            return self.native_status["tempo"] == "active";
        }
        self.turns
            .last()
            .is_some_and(|turn| turn["status"] == "inProgress")
    }

    fn active(&self) -> bool {
        // 原版已回复的澄清也可能标为 blocked；只按生成活动或未完成轮次显示执行中，保留真正的工具等待。
        if self.shared {
            // 与发送准入共用原生状态；已确认 idle/未知时，旧历史的 inProgress 不能继续撑住执行图标与队列。
            return self.running_for_send()
                || self.native_status["tempo"] == "blocked"
                    && self
                        .turns
                        .last()
                        .is_some_and(|t| t["status"] == "inProgress");
        }
        self.turns
            .last()
            .is_some_and(|t| t["status"] == "inProgress")
    }

    fn view(&self) -> Value {
        // 用户要求实例显示名统一简化；wslUser、实例 ID 和实际运行用户保持原用途。
        // 子代理沿用原生身份和只读详情，侧边会话仍独立保持未适配；不能据此开放子代理写操作。
        let capabilities = json!({"sideThreads":false,"subagents":true,
            "goalComposer":self.shared && self.native_control["capabilities"]["goal"] == true,
            "goalPause":false,"goalResume":false,
            "messageEdit":self.shared && !self.native_history_head.is_empty(),
            "messageEditHistory":self.shared,"messageEditModelSettings":self.shared,
            "queue":self.shared,"queueSendNow":false,
            "remoteQueue":self.shared,"queueEditRecovery":self.shared,
            "queuedImageEditing":self.shared,"queuedImageRemoval":self.shared,
            "compact":self.shared && self.native_control["capabilities"]["compact"] == true});
        json!({"id":self.id,"backend":self.backend,"title":self.title,"cwd":self.cwd,
            "projectId":self.project_id,"model":self.model,"reasoningEffort":self.effort,
            "createdAt":self.created_at,"updatedAt":self.updated_at,"recencyAt":self.updated_at,
            "pinned":self.pinned,"hasUnreadTurn":self.unread,"hostId":"local","ephemeral":false,
            "status":if self.shared && self.native_status["sessionStatus"] == "waiting" {
                json!({"type":"waiting","activeFlags":[if self.native_status["needs"] == "input needed" {
                    "waitingOnUserInput"
                } else {"waitingOnApproval"}]})
            } else {json!({"type":if self.active() {"active"} else {"idle"}})},
            "editableMessage":null,"editHistoryHead":self.native_history_head,
            "messageEditReady":self.shared && self.native_status["tempo"]=="idle"
                && self.native_status["needs"].is_null() && self.native_status["sessionStatus"]!="waiting",
            "queuedMessages":self.remote_queue.messages(&self.queued_messages),
            "queuedEdits":self.remote_queue.edits(),"goal":self.native_control["goal"],"tokenUsage":self.token_usage,
            "compaction":self.native_control["compaction"],
            "nativeCommands":self.native_control["commands"],
            "goalNotice":self.native_control["goalNotice"],
            "nativeSettings":self.native_control["settings"],
            "permissionMode":self.permission_mode,
            "permissionRevision":self.permission_revision,
            "permissionModes":self.native_control["capabilities"]["permissionModes"],
            "nativeDialog":self.native_control["dialog"],
            // 原生底栏汇报的后台任务摘要（如 “1 shell”），仅共享会话在线且画面可读时存在，网页负责翻译展示。
            "backgroundTasks":if self.shared {self.native_status["background"].clone()} else {Value::Null},
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
            "capabilities":capabilities})
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
                    dispatched_unknown(ApiError::new(503, "Claude 控制请求已派发，回执等待超时"))
                })?
                .map_err(|_| {
                    dispatched_unknown(ApiError::new(503, "Claude 控制请求已派发，连接已关闭"))
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
    skill_metadata: Mutex<HashMap<String, (Instant, Value)>>,
    // 用户要求账号额度按 WSL 实例隔离；只缓存白名单百分比与重置时间，不保存登录凭据。
    subscription_usage_cache: Mutex<HashMap<String, subscription_usage::CachedUsage>>,
    local_usage_cache: Mutex<HashMap<String, subscription_usage::CachedLocalUsage>>,
    usage_prices: Mutex<subscription_usage::PriceCache>,
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
            if conversation.shared {
                // 兼容旧版曾被其他状态写入带入的临时记录，升级后只从原生日志恢复正文。
                for turn in &mut conversation.turns {
                    if let Some(items) = turn["items"].as_array_mut() {
                        items.retain(|item| {
                            item["_nativePreview"] != true
                                && !item["id"]
                                    .as_str()
                                    .is_some_and(|id| id.starts_with("native-live-"))
                        });
                    }
                }
            }
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
            skill_metadata: Mutex::new(HashMap::new()),
            subscription_usage_cache: Mutex::new(HashMap::new()),
            local_usage_cache: Mutex::new(HashMap::new()),
            usage_prices: Mutex::new(subscription_usage::PriceCache::default()),
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

    #[cfg(test)]
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
        let parent = subagents::parse_id(thread_id).map(|(parent, _)| parent);
        Ok(self
            .conversation(parent.as_deref().unwrap_or(thread_id))
            .await?
            .config
            .instance_id())
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
        subagents::parse_id(id).is_some()
            || id
                .strip_prefix("claude-")
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
    }

    pub fn is_subagent_id(id: &str) -> bool {
        subagents::parse_id(id).is_some()
    }

    pub async fn workspace(&self, id: &str) -> anyhow::Result<Value> {
        let parent = subagents::parse_id(id).map(|(parent, _)| parent);
        let c = self.conversation(parent.as_deref().unwrap_or(id)).await?;
        Ok(
            json!({"ok":true,"threadId":id,"cwd":c.cwd,"linuxCwd":c.linux_cwd,
            "wslDistribution":c.config.distribution,"wslUser":c.config.user,
            "instanceId":c.config.instance_id(),"hostId":"local","backend":"claude"}),
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
            permission_mode: None,
            permission_revision: 0,
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
            native_control: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            live_preview: None,
            discovered: true,
            native_title: String::new(),
            native_history_head: String::new(),
            spent_requests: BTreeMap::new(),
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
                permission_mode: None,
                permission_revision: 0,
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
                native_control: Value::Null,
                native_clients: BTreeMap::new(),
                queued_messages: vec![],
                remote_queue: RemoteQueue::default(),
                live_preview: None,
                discovered: true,
                native_title: title.into(),
                native_history_head: String::new(),
                spent_requests: BTreeMap::new(),
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
        let pending_control = c.native_control["commands"]
            .as_array()
            .is_some_and(|commands| {
                commands.iter().any(|command| {
                    matches!(
                        command["state"].as_str(),
                        Some("pending" | "running" | "unknown")
                    )
                })
            });
        if (!c.discovered && !pending_control)
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
        let result = process
            .control(
                json!({"subtype":"read_history","native_id":c.native_id,
            "clients":c.native_clients,"pending":pending,"model":c.model,"native_control":c.native_control}),
                None,
            )
            .await;
        let frame = match result {
            Ok(frame) => frame,
            Err(failure) => {
                self.clear_missing_history_unread(&c, &failure).await?;
                return Err(failure);
            }
        };
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

    async fn clear_missing_history_unread(
        &self,
        observed: &Conversation,
        failure: &anyhow::Error,
    ) -> anyhow::Result<()> {
        // 用户要求此类无有效原生历史的会话也消除未读；只匹配指定 WSL 读取器的明确结果，超时、断网等错误仍保留未读。
        if !observed.unread
            || !failure.downcast_ref::<ApiError>().is_some_and(|failure| {
                failure.code == 503
                    && failure.message == "Claude 控制请求失败：指定 WSL 内没有该原生会话的有效历史"
            })
        {
            return Ok(());
        }
        // 只读接入仍不修改已读状态，不能借历史失败绕过原有 markRead 的权限边界。
        if let Some(controller) = self.controller.get().and_then(Weak::upgrade)
            && controller.access_policy.read().unwrap().read_only
        {
            return Ok(());
        }
        let mut saved = self.saved.lock().await;
        let unchanged = saved
            .conversations
            .get(&observed.id)
            .is_some_and(|current| {
                !current.deleted
                    && current.unread
                    && current.native_id == observed.native_id
                    && current.config == observed.config
                    && current.updated_at == observed.updated_at
                    && current.turns == observed.turns
            });
        // 只确认本次读取所见的内容；较早失败不能消除读取期间新到达回复的未读标记。
        if !unchanged {
            return Ok(());
        }
        let mut next = saved.clone();
        next.conversations.get_mut(&observed.id).unwrap().unread = false;
        self.persist(&next)?;
        *saved = next;
        drop(saved);
        self.publish(json!({"method":"remote/sidebarChanged","params":{"threadId":observed.id}}));
        Ok(())
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
        if Self::is_subagent_id(id) {
            return self.subagent_detail(id).await;
        }
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

    pub async fn session_metrics(self: &Arc<Self>, id: &str) -> anyhow::Result<Value> {
        let backfill = {
            let saved = self.saved.lock().await;
            let c = saved
                .conversations
                .get(id)
                .filter(|c| !c.deleted)
                .ok_or_else(|| error(404, "Claude 会话不存在"))?;
            if !c.shared || c.turns.iter().all(|turn| turn["_metricsVersion"] == 1) {
                return Ok(c.metrics());
            }
            (
                c.config.clone(),
                c.native_id.clone(),
                c.native_clients.clone(),
                c.model.clone(),
            )
        };
        // 旧存档只在独立统计请求中按需补读一次当前 JSONL；不进入 thread/send 路径，不取得发送锁、不启动 Claude。
        let reader = self.native_reader(&backfill.0).await?;
        let frame = reader
            .control(
                json!({"subtype":"read_history","native_id":backfill.1,
            "clients":backfill.2,"pending":[],"model":backfill.3}),
                None,
            )
            .await?;
        anyhow::ensure!(
            frame["native_id"] == backfill.1 && frame["messages"].is_array(),
            "Claude 统计原生身份不匹配"
        );
        let projected = project_shared_history(&frame, "metrics-readonly");
        let by_id: HashMap<_, _> = projected
            .iter()
            .filter_map(|turn| {
                turn["id"]
                    .as_str()
                    .map(|id| (id, turn["_metricsRequests"].clone()))
            })
            .collect();
        let mut saved = self.saved.lock().await;
        let c = saved
            .conversations
            .get_mut(id)
            .filter(|c| !c.deleted && c.native_id == backfill.1)
            .ok_or_else(|| error(404, "Claude 会话不存在"))?;
        // 补读期间原生快照或另一页可能已补齐数字，不能让较早的只读结果覆盖新统计。
        if c.turns.iter().all(|turn| turn["_metricsVersion"] == 1) {
            return Ok(c.metrics());
        }
        for turn in &mut c.turns {
            if let Some(requests) = turn["id"].as_str().and_then(|id| by_id.get(id)) {
                turn["_metricsRequests"] = requests.clone();
            }
            turn["_metricsVersion"] = json!(1);
        }
        // 只补内存数字，不持久化整份正文、不发布历史更新，也不改变未读、执行或回执状态。
        Ok(c.metrics())
    }

    pub async fn history(self: &Arc<Self>, payload: &Value) -> anyhow::Result<Value> {
        let id = payload["threadId"]
            .as_str()
            .context("缺少 Claude 会话 ID")?;
        if Self::is_subagent_id(id) {
            return self.subagent_history(payload).await;
        }
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
        let turns: Vec<_> = c.turns[start..end]
            .iter()
            .rev()
            .map(|turn| c.displayed_turn(turn))
            .collect();
        Ok(json!({"ok":true,"threadId":id,"data":turns,
            // 用户要求修复中断假消息；HTTP 分页也提供完整有效 ID，旧缓存无需等待事件流才能退役错误轮次。
            "turnIds":c.turns.iter().map(|turn| &turn["id"]).collect::<Vec<_>>(),
            "nextCursor":if start == 0 {Value::Null} else {json!(start.to_string())},
            "transcriptEvents":[],"backend":"claude"}))
    }

    pub async fn queue(&self, id: &str) -> anyhow::Result<Value> {
        if let Some((parent, _)) = subagents::parse_id(id) {
            self.conversation(&parent).await?;
            return Ok(json!({"ok":true,"threadId":id,"queueEpoch":self.epoch,
                "queueRevision":0,"queuedMessages":[],"queuedEdits":[],"queuePaused":false}));
        }
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
            permission_mode: None,
            ..c.clone()
        };
        let mut catalog = self.spawn_stdio(&probe).await?.initialization.clone();
        catalog["permissionModes"] = json!(super::permissions::claude_modes(&c.config).await);
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
        if !c.id.is_empty()
            && let Some(mode) = c
                .permission_mode
                .as_deref()
                .filter(|mode| *mode != "configured")
        {
            let mode = supported_permission_mode(&c.config, mode).await?;
            command.args(["--permission-mode", &mode]);
            if mode == "bypassPermissions" {
                // 只有用户为此会话明确选择完全访问时，才允许原生 CLI 提供绕过审批能力。
                command.arg("--allow-dangerously-skip-permissions");
            }
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
                                let message = format!(
                                    "Claude 控制请求失败：{}",
                                    response["error"].as_str().unwrap_or("原生请求失败")
                                );
                                let failure = ApiError::new(
                                    match response["code"].as_str() {
                                        Some(
                                            "EBUSY" | "EUNSUPPORTED" | "EDIALOGCHANGED" | "ESTALE"
                                            | "ESESSION" | "EQUESTIONCHANGED",
                                        ) => 409,
                                        Some("EINVAL" | "EIMAGE") => 400,
                                        Some("ENOENT") => 404,
                                        _ => 503,
                                    },
                                    message,
                                );
                                Err(if response["not_sent"] == true {
                                    failure.into()
                                } else {
                                    dispatched_unknown(failure).into()
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
                let _ = sender.send(Err(
                    dispatched_unknown(ApiError::new(503, detail.clone())).into()
                ));
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
                    "permission_mode":c.permission_mode,
                    "clients":c.native_clients,"pending":pending,"native_control":c.native_control})
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
        let process = self.catalog_process(config, cwd).await?;
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

    async fn catalog_process(
        self: &Arc<Self>,
        config: WslConfig,
        cwd: &str,
    ) -> anyhow::Result<Arc<Process>> {
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
            permission_mode: None,
            permission_revision: 0,
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
            native_control: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            live_preview: None,
            discovered: false,
            native_title: String::new(),
            native_history_head: String::new(),
            spent_requests: BTreeMap::new(),
        };
        // 模型与 Skill 目录共用原生初始化方式；探测不恢复已有会话，不发送模型消息。
        self.spawn_stdio(&c).await
    }

    pub async fn session_models(
        self: &Arc<Self>,
        id: &str,
        include_available: bool,
        force: bool,
    ) -> anyhow::Result<Value> {
        if Self::is_subagent_id(id) {
            return self.subagent_models(id).await;
        }
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

    pub async fn project_trust(
        &self,
        config: WslConfig,
        cwd: &str,
        confirmed_path: Option<&str>,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        if confirmed_path.is_some() && authorization.is_none() {
            return Err(error(403, "确认目录信任需要当前写入授权"));
        }
        config.validate()?;
        let managed = cwd.is_empty();
        let directory = if managed {
            self.directory.join("claude-workspaces")
        } else {
            PathBuf::from(cwd)
        };
        let linux_cwd = Self::linux_cwd(
            &config,
            directory.to_str().context("Claude 项目目录编码无效")?,
        )
        .await?;
        let script = self.directory.join("claude-project-trust.py");
        crate::settings::atomic_write(&script, include_bytes!("claude_project_trust.py"))?;
        let linux_script = Self::linux_cwd(
            &config,
            script.to_str().context("Claude 信任连接端路径编码无效")?,
        )
        .await?;
        let mut command = Self::wsl(&config)?;
        let mut child = command
            .args(["--exec", "python3", "-u", &linux_script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("无法检查 Claude 项目目录信任，首条消息尚未发送")?;
        let (writer, _) = spawn_writer(
            child.stdin.take().context("目录信任连接不可用")?,
            WRITE_TIMEOUT,
        );
        let mut bytes = serde_json::to_vec(&json!({
            "cwd":linux_cwd,"managed":managed,
            "confirm":confirmed_path.is_some(),"expectedPath":confirmed_path
        }))?;
        bytes.push(b'\n');
        // 信任写入只由用户确认后的 create 携带；提交到所选实例前复核设备和当前策略。
        guarded_write(
            &writer,
            bytes,
            authorization.map(|value| value as &dyn crate::codex_service::ServiceDispatchGuard),
            WRITE_TIMEOUT,
        )
        .await
        .map_err(|_| error(503, "无法确认目录信任操作，首条消息尚未发送，请重试"))?;
        drop(writer);
        let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .map_err(|_| error(503, "目录信任检查超时，首条消息尚未发送"))??;
        if !output.status.success() || output.stdout.len() > 32 * 1024 {
            return Err(error(503, "目录信任检查失败，首条消息尚未发送"));
        }
        let mut result: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| error(503, "目录信任检查结果无效，首条消息尚未发送"))?;
        if result["ok"] != true {
            return Err(error(
                409,
                "无法安全确认或保存 Claude 目录信任，首条消息尚未发送，请核对后重试",
            ));
        }
        result["instanceId"] = json!(config.instance_id());
        result["instanceLabel"] = json!(super::instances::label(
            "claude",
            &config.distribution,
            &config.user
        ));
        Ok(result)
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
        if let Some(mode) = payload.get("permissionMode") {
            let mode = super::permissions::validate_mode("claude", mode)?;
            if mode != "configured" {
                supported_permission_mode(&config, mode).await?;
            }
        }
        let goal = payload["goalMode"] == true;
        let content = if goal {
            native_controls::goal_argument(payload)?;
            vec![]
        } else {
            input_content(payload)?
        };
        // 用户要求在远程页面确认目录信任；先检查/保存，再启动 CLI 或登记会话，拒绝和取消不留下空会话。
        if payload
            .get("trustProject")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(error(400, "目录信任确认参数无效"));
        }
        let confirmed = if payload["trustProject"] == true {
            Some(
                payload["expectedProjectPath"]
                    .as_str()
                    .filter(|path| !path.is_empty() && path.len() <= 4096)
                    .ok_or_else(|| error(400, "缺少已确认的项目目录"))?,
            )
        } else {
            None
        };
        let trust = self
            .project_trust(config.clone(), cwd, confirmed, Some(authorization))
            .await?;
        if trust["trusted"] != true {
            return Err(error(
                409,
                "项目目录尚未被 Claude 信任，请在远程页面确认信任后继续；首条消息尚未发送",
            ));
        }
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
            permission_mode: payload
                .get("permissionMode")
                .and_then(Value::as_str)
                .map(str::to_owned),
            permission_revision: u64::from(payload.get("permissionMode").is_some()),
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
            native_control: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            live_preview: None,
            discovered: false,
            native_title: String::new(),
            native_history_head: String::new(),
            spent_requests: BTreeMap::new(),
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
        let started = if goal {
            self.dispatch_native_command(
                &c,
                &process,
                "goal",
                native_controls::goal_argument(payload)?,
                None,
                authorization,
            )
            .await
        } else {
            self.start_turn(&c.id, &process, content, payload, authorization)
                .await
        };
        match started {
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
            "editMessage" => {
                // 执行中禁止编辑，提交与普通发送共用已有会话锁；不停止任务、不把编辑重发放入队列。
                if !c.shared || c.active() {
                    return Err(error(409, "请等待 Claude 结束当前执行后再编辑历史消息"));
                }
                let turn = c
                    .turns
                    .iter()
                    .find(|turn| {
                        turn["id"] == payload["turnId"]
                            && turn["_nativeEditable"] == true
                            && turn["items"].as_array().is_some_and(|items| {
                                items.iter().any(|item| {
                                    item["type"] == "userMessage"
                                        && item["id"] == payload["messageId"]
                                })
                            })
                    })
                    .ok_or_else(|| error(409, "原生历史消息已变化，请重新打开编辑"))?;
                if c.native_history_head.is_empty()
                    || payload["historyHead"] != c.native_history_head
                {
                    return Err(error(409, "原生历史已变化，请重新打开编辑"));
                }
                let edit = json!({"native_user_id":turn["_nativeUserId"],"history_head":c.native_history_head});
                let existing = self
                    .processes
                    .lock()
                    .await
                    .get(id)
                    .filter(|process| process.channel.live.load(Ordering::Acquire))
                    .cloned();
                let process = match existing {
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
                self.enqueue_shared_edit(id, &process, payload, edit, authorization)
                    .await
            }
            "compact" | "setGoal" | "updateGoal" => {
                self.native_command_action(&mut c, action, payload, authorization)
                    .await
            }
            "send" => {
                if c.active() && !c.shared {
                    return Err(error(409, "请等待 Claude 完成本轮或先停止生成"));
                }
                let content = input_content(payload)?;
                if let Some(mode) = payload["permissionMode"].as_str() {
                    c.permission_mode = Some(mode.to_owned());
                }
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
                apply_permissions(&process, &mut c, payload, authorization).await?;
                apply_model(&process, &mut c, payload, authorization).await?;
                self.change(|saved| {
                    // CLI 可在网页切换模型期间继续产生消息；只更新设置，不能用旧快照覆盖实时历史。
                    let current = saved
                        .conversations
                        .get_mut(id)
                        .context("Claude 会话不存在")?;
                    current.model = c.model;
                    current.effort = c.effort;
                    current.permission_mode = c.permission_mode;
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

    pub async fn permission_selection(
        &self,
        id: &str,
    ) -> anyhow::Result<Option<super::permissions::Selection>> {
        let c = self.conversation(id).await?;
        Ok(c.permission_mode.map(|mode| super::permissions::Selection {
            instance_id: c.config.instance_id(),
            mode,
            revision: c.permission_revision,
        }))
    }

    pub async fn set_permissions(
        self: &Arc<Self>,
        id: &str,
        mode: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;
        let mut c = self.conversation(id).await?;
        if c.active()
            || !c.remote_queue.messages(&c.queued_messages).is_empty()
            || !c.remote_queue.edits().is_empty()
        {
            return Err(error(
                409,
                "请等待当前 Claude 任务和待发消息结束后再修改权限",
            ));
        }
        let process = self
            .processes
            .lock()
            .await
            .get(id)
            .filter(|process| process.channel.live.load(Ordering::Acquire))
            .cloned();
        let process = match process {
            Some(process) => process,
            None => {
                c.permission_mode = Some(mode.to_owned());
                let process = self.spawn(&c).await?;
                self.processes
                    .lock()
                    .await
                    .insert(id.to_owned(), process.clone());
                process
            }
        };
        apply_permissions(
            &process,
            &mut c,
            &json!({"permissionMode":mode}),
            authorization,
        )
        .await?;
        let revision = c
            .permission_revision
            .checked_add(1)
            .ok_or_else(|| error(409, "会话权限版本已达上限"))?;
        self.change(|saved| {
            let current = saved
                .conversations
                .get_mut(id)
                .context("Claude 会话不存在")?;
            current.permission_mode = Some(mode.to_owned());
            current.permission_revision = revision;
            if let Some(actual) = c.native_control["settings"]["permissionMode"].as_str() {
                if !current.native_control.is_object() {
                    current.native_control = json!({});
                }
                if !current.native_control["settings"].is_object() {
                    current.native_control["settings"] = json!({});
                }
                current.native_control["settings"]["permissionMode"] = json!(actual);
            }
            Ok(())
        })
        .await?;
        let result = json!({"ok":true,"threadId":id,"instanceId":c.config.instance_id(),
            "permissionMode":mode,"permissionRevision":revision,"appliesTo":"nextTurn"});
        self.publish(json!({"method":"thread/settings/updated","params":result}));
        Ok(result)
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
            let mut turn = turn.clone();
            // 原生任务清单跨轮次延续：新轮次从上一轮的最终清单开始。
            if let Some(tasks) = c.turns.last().and_then(tools::carried_tasks) {
                turn["_claudeTaskSeed"] = tasks;
            }
            c.turns.push(turn);
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
        if frame["type"] == "shared_native_command_intent" {
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
                        .native_control = frame["native_control"].clone();
                    Ok(())
                })
                .await?;
            }
            return Ok(());
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
                        model_context_capacity(model, &json!({"modelUsage":frame["modelUsage"]}));
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
        let previous_plan = turn["plan"].clone();
        let changed = project_frame(turn, &frame);
        // 任务清单合成的计划沿用 Codex 计划事件推送，网页按轮次字段版本合并。
        let plan = (turn["plan"] != previous_plan).then(|| turn["plan"].clone());
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
        if let Some(plan) = plan {
            self.publish(json!({"method":"turn/plan/updated","params":{"threadId":id,"turnId":turn_id,"plan":plan}}));
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
        let (thread, turns, turn_ids, completed, usage_changed, changed, reverted) = self
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
                let previous_view = c.view();
                if frame["native_control"].is_object() {
                    c.native_control = frame["native_control"].clone();
                    c.apply_native_settings(&frame["native_control"]["settings"]);
                }
                let previous_queue = c.queue_snapshot(&self.epoch);
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
                subagents::decorate_turns(id, &mut projected);
                restore_shared_images(&c.turns, &mut projected);
                let reverted = frame.get("history_head").is_some()
                    && c.turns.iter().any(|old| {
                        old["items"].as_array().is_some_and(|items| {
                            items.iter().any(|item| item["type"] == "userMessage")
                        }) && !projected.iter().any(|turn| turn["id"] == old["id"])
                    });
                if reverted {
                    for old in c
                        .turns
                        .iter()
                        .filter(|old| !projected.iter().any(|turn| turn["id"] == old["id"]))
                    {
                        for (request, usage) in
                            old["_metricsRequests"].as_object().into_iter().flatten()
                        {
                            c.spent_requests
                                .entry(request.clone())
                                .or_insert_with(|| usage.clone());
                        }
                    }
                }
                if let Some(retired) = frame["retired_usage"]
                    .as_array()
                    .filter(|rows| !rows.is_empty())
                {
                    let mut ledger = json!({"_metricsRequests":c.spent_requests});
                    for usage in retired {
                        super::session_metrics::observe_claude(&mut ledger, usage);
                    }
                    c.spent_requests = serde_json::from_value(ledger["_metricsRequests"].clone())?;
                }
                if let Some(head) = frame.get("history_head") {
                    c.native_history_head = head.as_str().unwrap_or("").into();
                }
                for turn in &mut projected {
                    // 已确认结束且无回复的输入保留正文与终态；后续新轮次不能让它再次等待启动。
                    if let Some(old) = c.turns.iter().find(|old| {
                        old["id"] == turn["id"] && old["_nativeEndedWithoutReply"] == true
                    }) && matches!(
                        turn["status"].as_str(),
                        Some("completed" | "pending" | "unknown")
                    ) && turn["items"]
                        .as_array()
                        .is_some_and(|items| items.iter().all(|item| item["type"] == "userMessage"))
                    {
                        turn["status"] = json!("completed");
                        turn["_nativeDone"] = json!(true);
                        turn["_nativeEndedWithoutReply"] = json!(true);
                        turn["turnCompletedAtMs"] = old["turnCompletedAtMs"].clone();
                        if old["_nativeWithdrawn"] == true {
                            turn["_nativeWithdrawn"] = json!(true);
                        }
                    }
                    if c.turns.iter().any(|old| {
                        old["id"] == turn["id"]
                            && old["_stopRequested"] == true
                            && old["_nativeDone"] != true
                    }) {
                        turn["_stopRequested"] = json!(true);
                        if turn["status"] != "inProgress" {
                            turn["status"] = json!("interrupted");
                        }
                    }
                }
                let completed = projected.iter().any(|t| {
                    matches!(
                        t["status"].as_str(),
                        Some("completed" | "failed" | "interrupted")
                    ) && c
                        .turns
                        .iter()
                        .any(|old| old["id"] == t["id"] && old["status"] == "inProgress")
                });
                let history_changed = projected != c.turns;
                if history_changed {
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
                // 用户要求正文不反复出现/隐藏：元数据快照沿用同一预览，正式正文或轮次变化时一次性退役。
                if c.live_preview.as_ref().is_some_and(|preview| {
                    !c.turns.last().is_some_and(|turn| preview.matches(turn))
                }) {
                    c.live_preview = None;
                }
                c.remote_queue.observe_turns(&c.turns, &c.native_status);
                let mut view = c.view();
                let changed = history_changed
                    || previous_view != view
                    || previous_queue != c.queue_snapshot(&self.epoch);
                for (key, value) in c.queue_snapshot(&self.epoch).as_object().unwrap() {
                    if key != "ok" && key != "threadId" {
                        view[key] = value.clone();
                    }
                }
                Ok((
                    view,
                    c.turns
                        .iter()
                        .rev()
                        .take(20)
                        .map(|turn| c.displayed_turn(turn))
                        .collect::<Vec<_>>(),
                    c.turns
                        .iter()
                        .map(|turn| turn["id"].clone())
                        .collect::<Vec<_>>(),
                    completed,
                    usage_changed,
                    changed,
                    reverted,
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
        if reverted {
            // 显式原生回退沿用已有缓存失效事件；已确认消息的防丢逻辑不能把被回退的旧分支补回来。
            self.publish(json!({"method":"remote/threadReverted","params":{"threadId":id}}));
        }
        if changed {
            self.publish(json!({"method":"claude/history/updated","params":{
                "threadId":id,"thread":thread,"turns":turns,"turnIds":turn_ids,"completed":completed}}));
            self.publish(json!({"method":"remote/sidebarChanged","params":{"threadId":id}}));
        }
        if usage_changed {
            self.publish(json!({"method":"thread/tokenUsage/updated","params":{"threadId":id,"tokenUsage":usage}}));
        }
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
        let (Some(basis), Some(text), Some(native_user_id)) = (
            shared_preview_basis(frame),
            frame["text"].as_str(),
            frame["native_user_id"].as_str(),
        ) else {
            return Ok(());
        };
        // 预览最多 64 KiB，超过时等待正式历史；不把不完整终端数据扩展成无界正文。
        if text.trim().is_empty() || text.len() > 64 * 1024 {
            return Ok(());
        }
        let preview = SharedPreview {
            generation: generation.into(),
            native_user_id: native_user_id.into(),
            basis: basis.into(),
            text: text.into(),
        };
        let Some(turn) = c
            .turns
            .last()
            .filter(|turn| turn["status"] == "inProgress" && preview.matches(turn))
        else {
            return Ok(());
        };
        if let Some(previous) = c
            .live_preview
            .as_ref()
            .filter(|previous| previous.matches(turn))
        {
            let compact = |text: &str| {
                text.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>()
            };
            let old = compact(&previous.text);
            let next = compact(text);
            if next == old || !next.starts_with(&old) {
                return Ok(());
            }
        }
        c.live_preview = Some(preview);
        let turn = c.displayed_turn(c.turns.last().unwrap());
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
        if original["nativeQuestion"].is_object()
            && original["nativeQuestion"]["state"] != "waiting"
        {
            return Err(error(409, "回答已经提交，正在核对原生结果，不会重复提交"));
        }
        if original["subtype"] == "native_dialog"
            && (original["dialog"]["remoteSupported"] != true
                || original["dialog"]["state"] != "waiting"
                || !matches!(payload["decision"].as_str(), Some("accept" | "decline")))
        {
            return Err(error(
                409,
                "该原生确认已提交或需在 Claude CLI 中处理，请重新读取状态",
            ));
        }
        // 用户要求网页只能关闭终端回退菜单（decline），不能从网页确认回退。
        if original["subtype"] == "native_dialog"
            && original["dialog"]["kind"] == "rewind"
            && payload["decision"] != "decline"
        {
            return Err(error(
                409,
                "网页只能关闭终端回退菜单，确认回退请在原版 Claude CLI 中操作",
            ));
        }
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
                    .filter(|answer| !answer.trim().is_empty())
                    .context("请回答所有问题")?;
                values.insert(key.into(), json!(answer));
            }
            let mut input = original["input"].clone();
            input["answers"] = json!(values);
            json!({"behavior":"allow","updatedInput":input})
        } else {
            let mut response = match payload["decision"].as_str() {
                Some("accept") => json!({"behavior":"allow","updatedInput":original["input"]}),
                Some("decline" | "cancel") => {
                    json!({"behavior":"deny","message":"用户拒绝本次操作",
                    "interrupt":payload["decision"]=="cancel"})
                }
                _ => return Err(error(400, "Claude 仅支持本次允许、拒绝或取消")),
            };
            // 计划审批：批准时带所选原生编号，拒绝时带修改意见；只发给共享连接端，不进入 print 控制协议。
            if original["subtype"] == "native_dialog" && original["dialog"]["kind"] == "plan" {
                response["option"] = payload["option"].clone();
                response["feedback"] = payload["feedback"].clone();
            }
            response
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
                // 补回历史配置变更提示不算新回复；CLI 重复同步同一配置也不能制造未读消息。
                if kind == "modelChanged" && item["source"] == "claude"
                    || item["remoteCompactionLifecycle"] == true
                {
                    // 原生压缩的状态变化不是新回复；重连补回状态行不能让已读会话反复变成未读。
                    continue;
                }
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

fn model_context_capacity(model: &str, initialization: &Value) -> (Option<u64>, Value) {
    if model.is_empty() {
        return (None, Value::Null);
    }
    // 用户要求模型上下文优先；读取不到时，使用同一模型的配置压缩窗口作为容量，不额外折算提前压缩比例。
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
    let context = &initialization["contextConfiguration"];
    if context["model"] == model
        && let Some(capacity) = context["compaction"]["windowTokens"]
            .as_u64()
            .filter(|value| *value > 0 && *value <= 9007199254740991)
    {
        return (
            Some(capacity),
            json!({"kind":"configuration","key":"compaction.windowTokens","model":model}),
        );
    }
    (None, Value::Null)
}

fn native_token_usage(messages: &[Value], initialization: &Value) -> Value {
    let mut compacted = None;
    for row in messages.iter().rev() {
        if compacted.is_none()
            && row["subtype"] == "compact_boundary"
            && let Some(tokens) = row["compactMetadata"]["postTokens"]
                .as_u64()
                .filter(|value| *value > 0 && *value <= 9007199254740991)
        {
            compacted = Some((tokens, row["uuid"].clone()));
        }
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
        let (capacity, capacity_source) = model_context_capacity(model, initialization);
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
        if let Some((tokens, id)) = compacted {
            // 压缩后沿用原生 postTokens；下一次真实请求自然接替，不继续显示压缩前的输入占用或捏造细分用量。
            result["last"] = json!({"totalTokens":tokens});
            result["messageId"] = id;
            result["source"] = json!("claude-native-compaction");
            result.as_object_mut().unwrap().remove("usedPercentage");
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

fn native_setting_control(row: &Value) -> bool {
    ["nativeSettingCommand", "nativeSettingOutput"]
        .into_iter()
        .any(|field| {
            matches!(row[field]["name"].as_str(), Some("model" | "effort"))
                && row[field]["value"].is_string()
        })
}

fn project_shared_setting_changes(
    frame: &Value,
    turns: &mut [Value],
    owners: &HashMap<&str, usize>,
) {
    let mut previous_model: Option<String> = None;
    let mut previous_effort: Option<String> = None;
    let mut observed_turns = std::collections::HashSet::new();
    for row in frame["messages"].as_array().into_iter().flatten() {
        // 用户要求按首次实际使用新配置的位置显示变更；启动参数也会生效，不能只靠晚到的斜杠命令。
        if row["type"] != "assistant"
            || row["isApiErrorMessage"] == true
            || row["message"]["model"] == "<synthetic>"
        {
            continue;
        }
        let model = row["requestedModel"]
            .as_str()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                row["message"]["model"]
                    .as_str()
                    .filter(|value| !value.is_empty())
            });
        let effort = row["effort"].as_str().filter(|value| !value.is_empty());
        if model.is_none() && effort.is_none() {
            continue;
        }
        let model_changed = previous_model
            .as_deref()
            .zip(model)
            .is_some_and(|(previous, current)| previous != current);
        let effort_changed = previous_effort
            .as_deref()
            .zip(effort)
            .is_some_and(|(previous, current)| previous != current);
        let owner = row["uuid"]
            .as_str()
            .and_then(|id| owners.get(id))
            .or_else(|| row["message"]["id"].as_str().and_then(|id| owners.get(id)))
            .copied();
        let first_request = owner.is_some_and(|index| observed_turns.insert(index));
        if (model_changed || effort_changed)
            && let Some(turn) = owner.and_then(|index| turns.get_mut(index))
        {
            let native_user = turn["_nativeUserId"].as_str().unwrap_or("").to_owned();
            let response = row["message"]["id"]
                .as_str()
                .or_else(|| row["uuid"].as_str())
                .unwrap_or("request");
            if let Some(items) = turn["items"].as_array_mut() {
                let input = items.iter().position(|item| item["type"] == "userMessage");
                let prefix = format!("{response}-");
                let tool_ids: Vec<_> = row["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "tool_use")
                    .filter_map(|block| block["id"].as_str())
                    .collect();
                let position = if first_request {
                    input
                } else {
                    items.iter().position(|item| {
                        item["id"]
                            .as_str()
                            .is_some_and(|id| id.starts_with(&prefix) || tool_ids.contains(&id))
                    })
                }
                .unwrap_or(items.len());
                let before = items.get(position).map(|item| item["id"].clone());
                items.insert(
                    position,
                    json!({
                        "id":format!("claude-settings:{native_user}:{response}"),
                        "type":"modelChanged","source":"claude","status":"completed",
                        "fromModel":previous_model,"toModel":model.or(previous_model.as_deref()),
                        "fromEffort":previous_effort,"toEffort":effort,
                        "beforeItemId":before,"remoteInlineEvent":!first_request,
                        "startedAtMs":row["timestampMs"]
                    }),
                );
            }
        }
        if let Some(model) = model {
            previous_model = Some(model.to_owned());
            if model_changed {
                previous_effort = effort.map(str::to_owned);
            }
        }
        if let Some(effort) = effort {
            previous_effort = Some(effort.to_owned());
        }
    }
}

fn project_shared_history(frame: &Value, generation: &str) -> Vec<Value> {
    let mut turns: Vec<Value> = vec![];
    let mut native_owners: HashMap<&str, usize> = HashMap::new();
    let mut native_positions = HashMap::new();
    let compactions = frame["native_control"]["compactionEvents"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut web_image_paths = std::collections::HashSet::new();
    for row in frame["messages"].as_array().into_iter().flatten() {
        let agent_notification = subagents::observe_notification(&mut turns, row);
        // 原生任务通知开启自动续跑轮次（见 claude_tools::push_notification），随后的回复归入该轮次。
        if row["nativeTaskNotification"].is_object() {
            if let Some((index, item_id)) = tools::push_notification(&mut turns, row, generation)
                && let Some(id) = row["uuid"].as_str()
            {
                native_owners.insert(id, index);
                native_positions.insert(id.to_owned(), (index, item_id));
            }
            continue;
        }
        if agent_notification {
            continue;
        }
        // 终端本地斜杠命令与原生系统提示以事件行附在当前轮次末尾，不制造用户气泡、不可编辑。
        if let Some(item) = tools::event_item(row) {
            let item_id = item["id"].as_str().unwrap_or_default().to_owned();
            match turns.last_mut() {
                Some(turn) => turn["items"].as_array_mut().unwrap().push(item),
                None => turns.push(
                    json!({"id":format!("event-{item_id}"),"_nativeUserId":row["uuid"],
                    "_nativeDone":true,"status":"completed","itemsComplete":true,"items":[item]}),
                ),
            }
            if let Some(id) = row["uuid"].as_str() {
                native_owners.insert(id, turns.len() - 1);
                native_positions.insert(id.to_owned(), (turns.len() - 1, item_id));
            }
            continue;
        }
        if tools::attach_local_output(&mut turns, row) {
            continue;
        }
        if row["nativeControlOutput"].is_object() || row["attachment"]["type"] == "goal_status" {
            continue;
        }
        if row["subtype"] == "compact_boundary" {
            let mut event = compactions
                .iter()
                .find(|event| event["nativeId"] == row["uuid"])
                .cloned()
                .unwrap_or_else(|| json!({"id":row["uuid"]}));
            event["status"] = json!("completed");
            event["nativeId"] = row["uuid"].clone();
            event["completedAtMs"] = row["timestampMs"].clone();
            event["source"] = row["compactMetadata"]["trigger"].clone();
            for key in ["preTokens", "postTokens", "durationMs"] {
                event[key] = row["compactMetadata"][key].clone();
            }
            let Some(item) = native_controls::compaction_item(&event) else {
                continue;
            };
            let item_id = item["id"].as_str().unwrap().to_owned();
            if let Some(turn) = turns.last_mut() {
                turn["items"].as_array_mut().unwrap().push(item);
            } else {
                turns.push(
                    json!({"id":format!("compact-{}",row["uuid"].as_str().unwrap_or("")),
                    "_nativeUserId":row["uuid"],"_nativeDone":true,"status":"completed",
                    "itemsComplete":true,"items":[item]}),
                );
            }
            if let Some(id) = row["uuid"].as_str() {
                native_positions.insert(id.to_owned(), (turns.len() - 1, item_id));
            }
            continue;
        }
        // 连接端按原生控制记录标记模型/等级命令和成对回显；用户手写相同标签仍是普通消息。
        if native_setting_control(row) {
            continue;
        }
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
        if row["nativeControlCommand"].is_object() {
            let command = &row["nativeControlCommand"];
            let value = command["value"].as_str().unwrap_or("");
            if command["name"] != "goal"
                || value.is_empty()
                || ["clear", "stop", "off", "reset", "none", "cancel"]
                    .contains(&value.to_ascii_lowercase().as_str())
            {
                continue;
            }
            // 原生接受后只展示一条目标正文，控制 XML 与回显不制造重复用户消息。
            content = json!(value);
        }
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
            let mut turn = json!({"id":client_id,"_nativeUserId":native_user,
                "_nativeEditable":row["nativeEditable"] == true,
                "_process":generation,"status":"completed","itemsView":"full","itemsComplete":true,
                "turnStartedAtMs":row["timestampMs"],
                "items":[{"id":client_id,"clientId":client_id,"type":"userMessage",
                    "content":content,"sentAtMs":row["timestampMs"]}]});
            // 原生任务清单跨轮次延续：新轮次从上一轮的最终清单开始。
            if let Some(tasks) = turns.last().and_then(tools::carried_tasks) {
                turn["_claudeTaskSeed"] = tasks;
            }
            turns.push(turn);
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
                if row["durationMs"]
                    .as_u64()
                    .is_some_and(|ms| ms <= 9_007_199_254_740_991)
                {
                    turn["durationMs"] = row["durationMs"].clone();
                }
            } else if matches!(row["type"].as_str(), Some("assistant" | "user")) {
                let mut native_frame = row.clone();
                native_frame["message"]["content"] = content;
                project_frame(turn, &native_frame);
                subagents::observe_result(turn, &native_frame);
                if frame["subagentReadOnly"] == true
                    && row["type"] == "assistant"
                    && matches!(
                        row["message"]["stop_reason"].as_str(),
                        Some("end_turn" | "stop_sequence")
                    )
                {
                    // 子代理原生 end_turn 结束的是本次输入轮次；不得把多次续聊合并，或将等待下一次输入计入执行耗时。
                    turn["_nativeDone"] = json!(true);
                    turn["turnCompletedAtMs"] = row["timestampMs"].clone();
                }
                if row["isApiErrorMessage"] == true {
                    turn["status"] = json!("failed");
                }
            }
        }
        if let Some(index) = turns.len().checked_sub(1) {
            if let Some(id) = row["uuid"].as_str() {
                native_owners.insert(id, index);
                if let Some(item_id) = turns[index]["items"]
                    .as_array()
                    .and_then(|items| items.last())
                    .and_then(|item| item["id"].as_str())
                {
                    native_positions.insert(id.to_owned(), (index, item_id.to_owned()));
                }
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
        && turn["status"] != "failed"
    {
        let accepted_without_reply = turn["_nativeUserId"]
            .as_str()
            .is_some_and(|native| frame["clients"][native].is_string())
            && turn["items"]
                .as_array()
                .is_some_and(|items| items.iter().all(|item| item["type"] == "userMessage"));
        // 出字前停止：连接端依据终端输入框确认原生已撤回本条输入（见 claude_shared.py withdrawn_prompt）。
        let withdrawn = turn["_nativeUserId"]
            .as_str()
            .is_some_and(|native| frame["native_status"]["withdrawnPrompt"] == native)
            && turn["items"]
                .as_array()
                .is_some_and(|items| items.iter().all(|item| item["type"] == "userMessage"));
        let ended_at = frame["native_status"]["endedAtMs"].as_u64();
        let ended_without_reply = accepted_without_reply
            && frame["native_status"]["tempo"] == "idle"
            && matches!(
                frame["native_status"]["state"].as_str(),
                Some("done" | "ended")
            )
            && ended_at
                .zip(turn["turnStartedAtMs"].as_u64())
                .is_some_and(|(ended, started)| started > 0 && ended >= started);
        if withdrawn {
            // 用户要求如实显示“已撤回”，不能一直等待开始；沿用“结束且无回复”标记，排队续发同样暂停。
            turn["status"] = json!("completed");
            turn["_nativeDone"] = json!(true);
            turn["_nativeEndedWithoutReply"] = json!(true);
            turn["_nativeWithdrawn"] = json!(true);
        } else if ended_without_reply {
            // 用户要求取消后不无限等待：必须有晚于本条原生输入的结束时间，单独 idle 或旧完成记录均不够。
            // 原生未给出取消原因时只陈述“已结束且无回复”，不伪造成功回复，也不移除已接收正文。
            turn["status"] = json!("completed");
            turn["_nativeDone"] = json!(true);
            turn["_nativeEndedWithoutReply"] = json!(true);
            turn["turnCompletedAtMs"] = json!(ended_at);
        } else if matches!(
            frame["native_status"]["tempo"].as_str(),
            Some("active" | "blocked")
        ) {
            turn["status"] = json!("inProgress");
        } else if frame["native_status"]["state"] == "stopped" {
            turn["status"] = json!("interrupted");
        } else if frame["native_status"]["state"] == "failed" {
            turn["status"] = json!("failed");
        } else {
            // 用户要求发送期间不闪现“已停止”：原生用户消息可先于工作状态落盘，idle/未知不是中断证据。
            // 已接收且尚无回复的网页输入保持待启动；缺少明确结束证据的旧历史保持未知，不能伪造完成或停止。
            turn["status"] = json!(if accepted_without_reply {
                "pending"
            } else {
                "unknown"
            });
        }
    }
    tools::settle_background(&mut turns, &frame["native_status"]);
    let latest = turns.len().checked_sub(1);
    for (index, turn) in turns.iter_mut().enumerate() {
        turn["_metricsVersion"] = json!(1);
        if turn["nativeInterruption"]["source"] == "claude"
            || Some(index) == latest && turn["status"] != "inProgress"
        {
            let unfinished = match turn["status"].as_str() {
                Some("interrupted") => "interrupted",
                Some("failed") => "failed",
                _ => "unknown",
            };
            for item in turn["items"].as_array_mut().into_iter().flatten() {
                if item["status"] == "inProgress" {
                    item["status"] = json!(unfinished);
                }
            }
        }
    }
    project_shared_setting_changes(frame, &mut turns, &native_owners);
    if let (Some(turn), Some(basis)) = (turns.last_mut(), shared_preview_basis(frame)) {
        turn["_nativePreviewBasis"] = json!(basis);
    }
    native_controls::project_compactions(compactions, &mut turns, &native_positions);
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

async fn supported_permission_mode(config: &WslConfig, mode: &str) -> anyhow::Result<String> {
    let modes = super::permissions::claude_modes(config).await;
    let selected = if modes.iter().any(|value| value == mode) {
        mode
    } else if mode == "default" && modes.iter().any(|value| value == "manual") {
        "manual"
    } else if mode == "manual" && modes.iter().any(|value| value == "default") {
        "default"
    } else {
        return Err(error(
            409,
            "当前 Claude CLI 尚未确认支持所选权限模式，请重新读取实例权限",
        ));
    };
    Ok(selected.to_owned())
}

async fn apply_permissions(
    process: &Process,
    c: &mut Conversation,
    payload: &Value,
    auth: &RemoteAuthorization,
) -> anyhow::Result<()> {
    let Some(mode) = payload.get("permissionMode") else {
        return Ok(());
    };
    let mode = super::permissions::validate_mode("claude", mode)?;
    let native_mode = if process.shared && mode == "configured" {
        "configured".to_owned()
    } else {
        let configured;
        let requested = if mode == "configured" {
            configured = super::permissions::configured_claude_mode(
                &auth.controller.state_dir,
                &c.config.instance_id(),
            )
            .await?;
            configured.as_str()
        } else {
            mode
        };
        supported_permission_mode(&c.config, requested).await?
    };
    let result = process
        .control(
            json!({"subtype":"set_permission_mode","mode":native_mode}),
            Some(auth),
        )
        .await?;
    if let Some(actual) = result["nativeSettings"]["permissionMode"].as_str() {
        if !c.native_control.is_object() {
            c.native_control = json!({});
        }
        if !c.native_control["settings"].is_object() {
            c.native_control["settings"] = json!({});
        }
        c.native_control["settings"]["permissionMode"] = json!(actual);
    }
    c.permission_mode = Some(mode.to_owned());
    Ok(())
}

async fn apply_model(
    process: &Process,
    c: &mut Conversation,
    payload: &Value,
    auth: &RemoteAuthorization,
) -> anyhow::Result<()> {
    let catalog = normalize_models(&process.initialization);
    let (model, effort) = model_selection(c, payload, &catalog)?;
    let pending = |name: &str| {
        process.shared
            && matches!(
                c.native_control["settings"]["changes"][name]["state"].as_str(),
                Some("pending" | "confirmation" | "confirming")
            )
    };
    let mut applied = json!({"nativeSettings":c.native_control["settings"]});
    // 等待原生确认时继续发送普通消息，不能顺带把尚未生效的选择改回旧模型或重复发切换命令。
    if payload["model"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
        || !pending("model")
    {
        applied = process
            .control(json!({"subtype":"set_model","model":model}), Some(auth))
            .await?;
    }
    if (!effort.is_empty() || !c.effort.is_empty())
        && (payload.get("reasoningEffort").is_some() || !pending("effort") && !pending("model"))
    {
        applied = process
            .control(
                json!({"subtype":"apply_flag_settings","settings":{
            "effortLevel":if effort.is_empty() {Value::Null} else {json!(effort)}}}),
                Some(auth),
            )
            .await?;
    }
    let (model, effort) = (model.to_owned(), effort.to_owned());
    if process.shared && applied["nativeSettings"].is_object() {
        c.apply_native_settings(&applied["nativeSettings"]);
        if !applied["nativeSettings"]["model"].is_string() {
            c.model = model;
        }
        if !applied["nativeSettings"]["effort"].is_string() {
            c.effort = effort;
        }
    } else {
        c.model = model;
        c.effort = effort;
    }
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
    let has_command = |name: &str| {
        initialization["commands"]
            .as_array()
            .is_some_and(|commands| commands.iter().any(|command| command["name"] == name))
    };
    json!({"ok":true,"backend":"claude","data":rows,"catalogComplete":true,"defaultModel":default,"defaultReasoningEffort":"",
        "nativeCapabilities":{"goal":has_command("goal"),"compact":has_command("compact")}})
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
    if request["subtype"] == "native_dialog" {
        let dialog = &request["dialog"];
        // plan：计划模式审批框，批准项编号与修改意见由连接端 answer_plan 按原生交互发送。
        // rewind：原版终端打开的回退菜单；用户要求网页只能关闭（取消），确认回退只能在终端操作。
        let kind = dialog["kind"].as_str();
        let supported = dialog["remoteSupported"] == true
            && matches!(kind, Some("model" | "effort" | "plan" | "rewind"))
            && dialog["state"] == "waiting";
        let decisions = if !supported {
            json!([])
        } else if kind == Some("rewind") {
            json!(["decline"])
        } else {
            json!(["accept", "decline"])
        };
        // 复用现有审批权限、请求版本与回执；未知原生弹窗只读提示，不开放任意终端输入。
        return Ok(
            json!({"id":id,"method":"item/commandExecution/requestApproval",
            "params":{"threadId":thread,"nativeDialog":dialog,"availableDecisions":decisions},
            "_claude":request}),
        );
    }
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
            "params":{"threadId":thread,"questions":questions,"nativeQuestion":request["nativeQuestion"]},"_claude":request}));
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
    turn["_metricsVersion"] = json!(1);
    super::session_metrics::observe_claude(turn, frame);
    if !turn["items"].is_array() {
        return vec![];
    }
    let mut changes: Vec<Value> = Vec::new();
    if frame["type"] == "assistant" {
        let mut plan_calls = Vec::new();
        let items = turn["items"].as_array_mut().unwrap();
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
                // 用户要求按 Claude 原生工具语义渲染，映射规则见 claude_tools.rs；任务清单工具并入轮次计划。
                Some("tool_use") => match tools::tool_item(block) {
                    Some(item) => item,
                    None => {
                        plan_calls.push(block);
                        continue;
                    }
                },
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
        for block in plan_calls {
            tools::observe_plan_call(turn, block);
        }
    } else if frame["type"] == "user" {
        for block in frame["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] != "tool_result" {
                continue;
            }
            let items = turn["items"].as_array_mut().unwrap();
            if let Some(item) = items.iter_mut().find(|i| i["id"] == block["tool_use_id"]) {
                tools::apply_result(item, block, frame);
                changes.push(item.clone());
            } else if let Some(item) = tools::observe_plan_result(turn, block, frame) {
                // 失败的任务清单调用不改变计划，单独显示通用失败卡片保留原生报错。
                let items = turn["items"].as_array_mut().unwrap();
                match items.iter_mut().find(|old| old["id"] == item["id"]) {
                    Some(old) => *old = item.clone(),
                    None => items.push(item.clone()),
                }
                changes.push(item);
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
            permission_mode: None,
            permission_revision: 0,
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
            native_control: Value::Null,
            native_clients: BTreeMap::new(),
            queued_messages: vec![],
            remote_queue: RemoteQueue::default(),
            live_preview: None,
            discovered: false,
            native_title: String::new(),
            native_history_head: String::new(),
            spent_requests: BTreeMap::new(),
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
        discovery_fixture_response(service, rows, Ok(snapshot)).await
    }

    async fn discovery_fixture_response(
        service: &Arc<Claude>,
        rows: Value,
        snapshot: Result<Value, (u16, String)>,
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
                    Some("discover") => Ok(json!({"sessions":rows})),
                    Some("read_history") => snapshot
                        .clone()
                        .map_err(|(code, message)| error(code, message)),
                    _ => panic!("只读测试不能启动模型、恢复任务或发送消息"),
                };
                if let Some(sender) = channel
                    .pending
                    .lock()
                    .await
                    .remove(frame["request_id"].as_str().unwrap())
                {
                    let _ = sender.send(response);
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
    async fn missing_native_history_clears_only_its_unread_and_preserves_the_read_failure() {
        const MISSING: &str = "Claude 控制请求失败：指定 WSL 内没有该原生会话的有效历史";
        for action in ["thread", "history"] {
            for (code, message, clears) in [
                (503, MISSING, true),
                (503, "Claude 控制请求已派发，回执等待超时", false),
                (503, "Claude 控制请求已派发，连接已关闭", false),
                (403, MISSING, false),
                (503, "其他历史读取错误", false),
            ] {
                let (directory, service, mut c) = fixture();
                c.shared = true;
                c.discovered = true;
                c.unread = true;
                c.pinned = true;
                let mut other = c.clone();
                other.id = format!("claude-{}", uuid::Uuid::new_v4());
                other.native_id = uuid::Uuid::new_v4().to_string();
                service
                    .change(|saved| {
                        saved.conversations.insert(c.id.clone(), c.clone());
                        saved.conversations.insert(other.id.clone(), other.clone());
                        Ok(())
                    })
                    .await
                    .unwrap();
                let calls =
                    discovery_fixture_response(&service, json!([]), Err((code, message.into())))
                        .await;
                let failure = match action {
                    "thread" => service.detail(&c.id).await.unwrap_err(),
                    _ => service
                        .history(&json!({"threadId":c.id,"cursor":null}))
                        .await
                        .unwrap_err(),
                };
                assert_eq!(failure.downcast_ref::<ApiError>().unwrap().message, message);
                assert_eq!(calls.lock().await.len(), 1, "失败不重读或重发任务");
                let restored = Claude::new(directory.path()).unwrap();
                let current = restored.conversation(&c.id).await.unwrap();
                assert_eq!(current.unread, !clears, "{action}: {code} {message}");
                assert_eq!(current.turns, c.turns, "清除未读不能删除保留历史");
                assert!(restored.conversation(&other.id).await.unwrap().unread);
                let mut sidebar = json!({"recent":[],"pinnedThreads":[],
                    "projects":[{"id":c.project_id,"threads":[]}]});
                restored.merge_sidebar(&mut sidebar).await;
                for row in sidebar["recent"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .chain(sidebar["pinnedThreads"].as_array().unwrap())
                    .chain(sidebar["projects"][0]["threads"].as_array().unwrap())
                    .filter(|row| row["id"] == c.id)
                {
                    assert_eq!(row["hasUnreadTurn"], !clears);
                }
            }
        }
    }

    #[tokio::test]
    async fn missing_native_history_does_not_acknowledge_newer_replies_or_other_instances() {
        let (_directory, service, mut observed) = fixture();
        observed.unread = true;
        for change in ["reply", "instance", "native-id", "deleted"] {
            let mut current = observed.clone();
            match change {
                "reply" => {
                    current.turns[0]["items"] = json!([
                        {"id":"new-reply","type":"agentMessage","text":"读取期间新增回复"}
                    ])
                }
                "instance" => current.config.distribution = "other-instance".into(),
                "native-id" => current.native_id = uuid::Uuid::new_v4().to_string(),
                _ => current.deleted = true,
            }
            service
                .change(|saved| {
                    saved.conversations.insert(current.id.clone(), current);
                    Ok(())
                })
                .await
                .unwrap();
            service
                .clear_missing_history_unread(
                    &observed,
                    &error(
                        503,
                        "Claude 控制请求失败：指定 WSL 内没有该原生会话的有效历史",
                    ),
                )
                .await
                .unwrap();
            assert!(
                service.saved.lock().await.conversations[&observed.id].unread,
                "{change}: 旧读取失败不能清除变化后的未读"
            );
        }
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
    fn claude_native_dialogs_preserve_waiting_and_require_explicit_decisions() {
        let (_, _, mut c) = fixture();
        c.shared = true;
        c.model = "sonnet".into();
        c.native_status =
            json!({"tempo":"blocked","sessionStatus":"waiting","needs":"dialog open"});
        c.apply_native_settings(&json!({"model":"sonnet","changes":{"model":{
            "id":"change","requested":"opus","previous":"sonnet","state":"confirmation"
        }}}));
        assert_eq!(c.view()["status"]["type"], "waiting");
        assert!(!c.running_for_send(), "等待确认不能触发普通消息入网页队列");
        assert_eq!(c.view()["model"], "sonnet");
        assert_eq!(
            c.view()["nativeSettings"]["changes"]["model"]["requested"],
            "opus"
        );
        c.apply_native_settings(&json!({"model":"sonnet","changes":{"model":{
            "id":"change","requested":"opus","state":"rejected"
        }}}));
        assert_eq!(c.model, "sonnet");
        c.apply_native_settings(&json!({"model":"opus","changes":{"model":{"state":"applied"}}}));
        assert_eq!(c.model, "opus");

        for (kind, state, supported, decisions) in [
            ("model", "waiting", true, json!(["accept", "decline"])),
            ("effort", "waiting", true, json!(["accept", "decline"])),
            ("model", "submitted", true, json!([])),
            ("unknown", "waiting", false, json!([])),
            // 终端回退菜单只允许网页关闭（取消），不提供确认回退。
            ("rewind", "waiting", true, json!(["decline"])),
        ] {
            let request = permission_request(
                "thread",
                "dialog",
                &json!({
                    "subtype":"native_dialog", "dialog":{"kind":kind,"state":state,
                        "remoteSupported":supported,"target":"Opus 5.5","description":"原生提示"}
                }),
            )
            .unwrap();
            assert_eq!(request["method"], "item/commandExecution/requestApproval");
            assert_eq!(request["params"]["nativeDialog"]["description"], "原生提示");
            assert_eq!(request["params"]["availableDecisions"], decisions);
        }
    }

    #[test]
    fn claude_live_questions_share_question_cards_and_keep_all_questions() {
        let (_directory, _service, mut c) = fixture();
        c.shared = true;
        c.native_status =
            json!({"tempo":"blocked","sessionStatus":"waiting","needs":"input needed"});
        assert_eq!(
            c.view()["status"],
            json!({"type":"waiting","activeFlags":["waitingOnUserInput"]})
        );
        assert!(!c.running_for_send());
        let raw = json!({"subtype":"can_use_tool","tool_name":"AskUserQuestion",
        "nativeQuestion":{"id":"live-question","state":"waiting"},
        "input":{"questions":[
            {"question":"选择分支？","options":[{"label":"test"},{"label":"pre-production"}]},
            {"question":"只有主分支怎么办？","options":[{"label":"master"},{"label":"跳过"}]}
        ]}});
        let request = permission_request("thread", "live-question", &raw).unwrap();
        assert_eq!(request["method"], "item/tool/requestUserInput");
        assert_eq!(request["params"]["questions"].as_array().unwrap().len(), 2);
        assert_eq!(
            request["params"]["questions"][1]["id"],
            "只有主分支怎么办？"
        );
        assert_eq!(request["params"]["nativeQuestion"]["state"], "waiting");
        assert_eq!(request["_claude"], raw);
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
    async fn shared_preview_survives_metadata_snapshots_without_persisting_or_reviving_after_canonical_text()
     {
        let (directory, service, mut c) = fixture();
        c.shared = true;
        c.turns.clear();
        let (writer, _) = mpsc::channel(1);
        service.processes.lock().await.insert(
            c.id.clone(),
            Arc::new(Process {
                _child: Mutex::new(None),
                writer,
                shared: true,
                initialization: json!({}),
                channel: Arc::new(Channel {
                    live: AtomicBool::new(true),
                    generation: "preview-fixture".into(),
                    pending: Mutex::new(HashMap::new()),
                }),
            }),
        );
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let mut frame = json!({"type":"shared_snapshot","native_id":c.native_id,
            "job_id":&c.native_id[..8],"native_status":{"tempo":"active"},
            "clients":{"native-user":"client"},"pending":[],"requests":{},
            "preview_basis":"a".repeat(64),
            "messages":[{"type":"user","uuid":"native-user","message":{"content":"question"}}]});
        service
            .ingest_shared(&c.id, "preview-fixture", &frame)
            .await
            .unwrap();
        let preview = json!({"native_user_id":"native-user","text":"partial response","preview_basis":"a".repeat(64)});
        service
            .ingest_shared_preview(&c.id, "preview-fixture", &preview)
            .await
            .unwrap();
        let history = service.history(&json!({"threadId":c.id})).await.unwrap();
        assert_eq!(history["data"][0]["items"].as_array().unwrap().len(), 2);
        frame["native_status"]["detail"] = json!("metadata changed");
        service
            .ingest_shared(&c.id, "preview-fixture", &frame)
            .await
            .unwrap();
        let history = service.history(&json!({"threadId":c.id})).await.unwrap();
        assert_eq!(
            history["data"][0]["items"].as_array().unwrap().len(),
            2,
            "正式正文尚未变化时，状态快照不能反复删除预览"
        );
        frame["native_status"]["tempo"] = json!("idle");
        service
            .ingest_shared(&c.id, "preview-fixture", &frame)
            .await
            .unwrap();
        assert_eq!(
            service.history(&json!({"threadId":c.id})).await.unwrap()["data"][0]["items"]
                .as_array()
                .unwrap()
                .len(),
            2,
            "短暂空闲且没有结束证据时，已经展示的预览也不能消失"
        );
        frame["native_status"]["tempo"] = json!("active");
        service
            .change(|saved| {
                saved.conversations.get_mut(&c.id).unwrap().pinned = true;
                Ok(())
            })
            .await
            .unwrap();
        let stored: Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("remote-claude-sessions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            stored["conversations"][&c.id]["turns"][0]["items"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "其他设置落盘也不能把终端预览保存为正式历史"
        );
        let mut legacy = stored;
        legacy["conversations"][&c.id]["turns"][0]["items"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"native-live-native-user",
                "type":"agentMessage","text":"legacy terminal fragment"}));
        std::fs::write(
            directory.path().join("remote-claude-sessions.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let reopened = Claude::new(directory.path())
            .unwrap()
            .conversation(&c.id)
            .await
            .unwrap();
        assert_eq!(
            reopened.turns[0]["items"].as_array().unwrap().len(),
            1,
            "升级重开时清理旧版曾持久化的终端气泡"
        );
        assert!(reopened.live_preview.is_none());
        frame["preview_basis"] = json!("b".repeat(64));
        frame["messages"].as_array_mut().unwrap().push(json!({
            "type":"assistant","uuid":"reply-row","message":{
                "id":"reply","content":[{"type":"text","text":"partial **response** complete"}]}}));
        service
            .ingest_shared(&c.id, "preview-fixture", &frame)
            .await
            .unwrap();
        service
            .ingest_shared_preview(&c.id, "preview-fixture", &preview)
            .await
            .unwrap();
        let history = service.history(&json!({"threadId":c.id})).await.unwrap();
        assert_eq!(history["data"][0]["items"].as_array().unwrap().len(), 2);
        assert_eq!(
            history["data"][0]["items"][1]["text"], "partial **response** complete",
            "正式 Markdown 接替后，迟到终端预览不能复活"
        );
        let next_preview = json!({"native_user_id":"native-user",
            "text":"next response fragment","preview_basis":"b".repeat(64)});
        service
            .ingest_shared_preview(&c.id, "retired-generation", &next_preview)
            .await
            .unwrap();
        assert!(
            service
                .conversation(&c.id)
                .await
                .unwrap()
                .live_preview
                .is_none()
        );
        service
            .ingest_shared_preview(&c.id, "preview-fixture", &next_preview)
            .await
            .unwrap();
        assert_eq!(
            service.history(&json!({"threadId":c.id})).await.unwrap()["data"][0]["items"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        frame["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"system","subtype":"turn_duration"}));
        service
            .ingest_shared(&c.id, "preview-fixture", &frame)
            .await
            .unwrap();
        service
            .ingest_shared_preview(&c.id, "preview-fixture", &next_preview)
            .await
            .unwrap();
        let ended = service.history(&json!({"threadId":c.id})).await.unwrap();
        assert_eq!(ended["data"][0]["status"], "completed");
        assert_eq!(
            ended["data"][0]["items"].as_array().unwrap().len(),
            2,
            "已结束轮次只保留正式正文，不恢复残留或迟到预览"
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
    fn context_capacity_prefers_model_reports_then_the_configured_compaction_window() {
        let mut messages = vec![
            json!({"type":"assistant","message":{"id":"request","model":"gpt-6.1-sol",
            "usage":{"input_tokens":181,"cache_read_input_tokens":108032,"cache_creation_input_tokens":0,"output_tokens":545}}}),
        ];
        let mut initialization = json!({"models":[{"value":"gpt-6.1-sol"}],"contextConfiguration":{
            "maxContextTokens":600000,"model":"gpt-6.1-sol",
            "compaction":{"windowTokens":600000,"triggerPercent":90},
            "source":{"kind":"configuration","key":"CLAUDE_CODE_MAX_CONTEXT_TOKENS","scope":"user-settings.env","model":"gpt-6.1-sol"}}});
        let usage = native_token_usage(&messages, &initialization);
        assert_eq!(usage["modelContextWindow"], 600000);
        assert_eq!(usage["modelContextWindowSource"]["kind"], "configuration");
        assert_eq!(
            usage["modelContextWindowSource"]["key"],
            "compaction.windowTokens"
        );
        assert!(usage["usedPercentage"].is_null());
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
            "模型目录报告优先于配置压缩窗口"
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
        assert_eq!(
            native_token_usage(&messages, &initialization)["modelContextWindow"],
            600000,
            "模型绑定更新后使用该模型的配置窗口，不改用其他配置字段"
        );
        for invalid in [
            json!(0),
            json!(-1),
            json!(false),
            json!("600000"),
            json!(9007199254740992_u64),
        ] {
            initialization["contextConfiguration"]["compaction"]["windowTokens"] = invalid;
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
    fn shared_settings_history_anchors_effective_change_and_hides_only_native_controls() {
        let response = |id: &str, effort: &str| {
            json!({"type":"assistant","uuid":id,"requestedModel":"claude-opus-5-5","effort":effort,
                "message":{"id":id,"model":"claude-opus-5-5","content":[{"type":"text","text":"回复"}]}})
        };
        let literal = "<command-name>/effort</command-name>\n<command-message>effort</command-message>\n<command-args>xhigh</command-args>";
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"old-user","message":{"content":"之前的问题"}},
            response("old-response", "max"),
            {"type":"user","uuid":"changed-user","timestampMs":100,"message":{"content":"这条消息开始使用新等级"}},
            response("changed-response", "xhigh"), response("same-turn-response", "xhigh"),
            {"type":"user","uuid":"model-command","nativeSettingCommand":{"name":"model","value":"claude-opus-5-5"},
                "message":{"content":"<command-name>/model</command-name>"}},
            {"type":"user","uuid":"model-output","nativeSettingOutput":{"name":"model","value":"claude-opus-5-5","commandId":"model-command"},
                "message":{"content":"<local-command-stdout>Set model</local-command-stdout>"}},
            {"type":"user","uuid":"effort-command","nativeSettingCommand":{"name":"effort","value":"xhigh"},
                "message":{"content":literal}},
            {"type":"user","uuid":"effort-output","nativeSettingOutput":{"name":"effort","value":"xhigh","commandId":"effort-command"},
                "message":{"content":"<local-command-stdout>Set effort level</local-command-stdout>"}},
            {"type":"user","uuid":"next-user","message":{"content":"恢复后继续"}},
            response("next-response", "xhigh"),
            {"type":"user","uuid":"literal-user","message":{"content":literal}}
        ]});
        let turns = project_shared_history(&frame, "first");
        assert_eq!(turns.len(), 4, "命令与成对回显不能形成聊天轮次");
        let event = &turns[1]["items"][0];
        assert_eq!(event["type"], "modelChanged");
        assert_eq!(event["fromEffort"], "max");
        assert_eq!(event["toEffort"], "xhigh");
        assert_eq!(event["beforeItemId"], "changed-user");
        assert_eq!(event["remoteInlineEvent"], false);
        assert_eq!(turns[1]["items"][1]["type"], "userMessage");
        assert_eq!(turns[3]["items"][0]["content"][0]["text"], literal);
        assert_eq!(
            turns
                .iter()
                .flat_map(|turn| turn["items"].as_array().unwrap())
                .filter(|item| item["type"] == "modelChanged")
                .count(),
            1,
            "同一轮多次请求及恢复时同配置重设只保留首次实际变更"
        );
        let restored = project_shared_history(&frame, "reconnected");
        assert_eq!(
            restored[1]["items"][0], *event,
            "重连保留事件身份与原生位置"
        );
        let mut old_frame = frame.clone();
        for row in old_frame["messages"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("nativeSettingCommand");
            row.as_object_mut().unwrap().remove("nativeSettingOutput");
        }
        let mut legacy = project_shared_history(&old_frame, "old");
        for turn in &mut legacy {
            turn["items"]
                .as_array_mut()
                .unwrap()
                .retain(|item| item["type"] != "modelChanged");
        }
        assert!(
            !shared_history_has_new_content(&legacy, &turns),
            "清理旧命令和补回变更提示不重置未读"
        );
    }

    #[tokio::test]
    async fn project_trust_confirmation_requires_authorization_before_any_side_effects() {
        let directory = tempfile::tempdir().unwrap();
        let service = Claude::new(directory.path()).unwrap();
        let failure = service
            .project_trust(
                WslConfig::default(),
                "unused",
                Some("/confirmed-project"),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(failure.downcast_ref::<ApiError>().unwrap().code, 403);
        assert!(!directory.path().join("claude-project-trust.py").exists());
        assert!(service.saved.lock().await.conversations.is_empty());
    }

    #[tokio::test]
    async fn project_trust_invalid_create_confirmation_does_not_spawn_or_register_a_session() {
        let directory = tempfile::tempdir().unwrap();
        let service = Claude::new(directory.path()).unwrap();
        let (_auth_directory, auth) = crate::remote_access::tests::service_dispatch_fixture().await;
        for trust in [
            json!({"trustProject":"yes"}),
            json!({"trustProject":true}),
            json!({"trustProject":true,"expectedProjectPath":""}),
            json!({"trustProject":true,"expectedProjectPath":"x".repeat(4097)}),
        ] {
            let mut payload = trust;
            payload["prompt"] = json!("不应派发的首条消息");
            let failure = service
                .create_in(WslConfig::default(), &payload, "unused", &auth)
                .await
                .unwrap_err();
            assert_eq!(failure.downcast_ref::<ApiError>().unwrap().code, 400);
        }
        assert!(!directory.path().join("claude-project-trust.py").exists());
        assert!(service.processes.lock().await.is_empty());
        assert!(service.saved.lock().await.conversations.is_empty());
    }

    #[test]
    fn shared_settings_history_keeps_mid_turn_model_change_at_the_effective_request() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"first-user","message":{"content":"初始问题"}},
            {"type":"assistant","uuid":"first-response","requestedModel":"model-a","effort":"high",
                "message":{"id":"first-response","model":"proxy-alias","content":[{"type":"text","text":"首轮"}]}},
            {"type":"user","uuid":"next-user","message":{"content":"下一轮"}},
            {"type":"assistant","uuid":"second-response","requestedModel":"model-b","effort":"high",
                "message":{"id":"second-response","model":"proxy-alias","content":[{"type":"text","text":"前半轮"}]}},
            {"type":"assistant","uuid":"synthetic","requestedModel":"wrong-model","effort":"low",
                "isApiErrorMessage":true,"message":{"id":"error","model":"<synthetic>","content":[{"type":"text","text":"错误"}]}},
            {"type":"assistant","uuid":"third-response","requestedModel":"model-b","effort":"max",
                "message":{"id":"third-response","model":"proxy-alias","content":[{"type":"text","text":"后半轮"}]}}
        ]});
        let turns = project_shared_history(&frame, "first");
        let items = turns[1]["items"].as_array().unwrap();
        assert_eq!(items[0]["type"], "modelChanged");
        assert_eq!(items[0]["fromModel"], "model-a");
        assert_eq!(
            items[0]["toModel"], "model-b",
            "以实际请求型号为准，不能用代理回复别名"
        );
        let inline = items
            .iter()
            .position(|item| item["type"] == "modelChanged" && item["remoteInlineEvent"] == true)
            .unwrap();
        let before = items
            .iter()
            .position(|item| item["id"] == "second-response-0")
            .unwrap();
        let after = items
            .iter()
            .position(|item| item["id"] == "third-response-0")
            .unwrap();
        assert!(
            before < inline && inline < after,
            "本轮中途生效的等级变化不能提前到问题之前"
        );
        assert_eq!(items[inline]["fromEffort"], "high");
        assert_eq!(
            items[inline]["toEffort"], "max",
            "错误合成回复不能覆盖已确认配置"
        );
    }

    #[test]
    fn shared_settings_history_does_not_invent_a_change_from_missing_request_metadata() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"first-user","message":{"content":"旧历史"}},
            {"type":"assistant","message":{"id":"old","content":[{"type":"text","text":"旧回复"}]}},
            {"type":"user","uuid":"next-user","message":{"content":"新请求"}},
            {"type":"assistant","requestedModel":"model-a","effort":"max",
                "message":{"id":"new","content":[{"type":"text","text":"新回复"}]}}
        ]});
        let turns = project_shared_history(&frame, "first");
        assert!(
            turns
                .iter()
                .flat_map(|turn| turn["items"].as_array().unwrap())
                .all(|item| item["type"] != "modelChanged"),
            "缺失旧配置时只建立基线，不猜造切换"
        );
    }

    #[test]
    fn native_controls_preserve_goal_text_and_compaction_without_command_echo_duplicates() {
        let frame = json!({"native_status":{"tempo":"idle"},"messages":[
            {"type":"attachment","uuid":"goal-state","attachment":{"type":"goal_status","condition":"完成验收","met":false,"sentinel":true}},
            {"type":"user","uuid":"goal-input","timestampMs":1000,
                "nativeControlCommand":{"name":"goal","value":"完成验收"},"message":{"content":"原生控制 XML"}},
            {"type":"user","uuid":"goal-output","nativeControlOutput":{"name":"goal","value":"完成验收"},"message":{"content":"Goal set"}},
            {"type":"assistant","uuid":"reply","message":{"id":"reply","content":[{"type":"text","text":"开始执行"}]}},
            {"type":"system","uuid":"compact-done","subtype":"compact_boundary","timestampMs":2000,
                "compactMetadata":{"trigger":"manual","preTokens":10000,"postTokens":2000}},
            {"type":"user","uuid":"next","message":{"content":"下一条"}}
        ]});
        let turns = project_shared_history(&frame, "generation");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["items"][0]["content"][0]["text"], "完成验收");
        let events: Vec<_> = turns[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "contextCompaction")
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["postTokens"], 2000);
        assert_eq!(turns[1]["items"][0]["content"][0]["text"], "下一条");
    }

    #[test]
    fn native_controls_validate_goal_limits_and_preserve_native_state_across_serialization() {
        for prompt in ["", "CLEAR", "stop"] {
            assert!(native_controls::goal_argument(&json!({"prompt":prompt})).is_err());
        }
        assert!(native_controls::goal_argument(&json!({"prompt":"😀".repeat(2000)})).is_ok());
        assert!(native_controls::goal_argument(&json!({"prompt":"😀".repeat(2001)})).is_err());
        assert!(native_controls::goal_argument(&json!({"prompt":"目标","images":[{}]})).is_err());
        let (_directory, _service, mut c) = fixture();
        c.shared = true;
        c.native_control = json!({"goal":{"objective":"目标","status":"active","createdAt":100},
            "capabilities":{"goal":true,"compact":true},"compaction":{"state":"unknown","itemId":"operation"},
            "commands":[{"id":"operation","name":"compact","state":"unknown"}]});
        let reopened: Conversation =
            serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
        assert_eq!(reopened.view()["goal"], c.view()["goal"]);
        assert_eq!(reopened.view()["compaction"]["state"], "unknown");
        assert_eq!(reopened.view()["capabilities"]["goalComposer"], true);
        assert_eq!(reopened.view()["capabilities"]["goalPause"], false);
    }

    #[test]
    fn native_controls_compaction_lifecycle_reuses_one_row_at_the_original_position() {
        let mut frame = json!({"messages":[
            {"type":"user","uuid":"question","message":{"content":"问题"}},
            {"type":"assistant","uuid":"before-row","message":{"id":"before","content":[{"type":"text","text":"先前回复"}]}},
            {"type":"system","uuid":"turn-end","subtype":"turn_duration"},
            {"type":"assistant","uuid":"after-row","message":{"id":"after","content":[{"type":"text","text":"继续回复"}]}}
        ],"native_control":{"compactionEvents":[
            {"id":"operation","status":"inProgress","afterUuid":"turn-end","startedAtMs":1000}
        ]}});
        let running = project_shared_history(&frame, "generation");
        let items = running[0]["items"].as_array().unwrap();
        assert_eq!(items[2]["id"], "operation");
        assert_eq!(items[2]["status"], "inProgress");
        assert_eq!(items[2]["remoteCompactionLifecycle"], true);
        assert_eq!(items[3]["text"], "继续回复");
        frame["native_control"]["compactionEvents"][0]["status"] = json!("interrupted");
        let stopped = project_shared_history(&frame, "reconnect");
        assert_eq!(stopped[0]["items"][2]["status"], "interrupted");
        assert!(!shared_history_has_new_content(&running, &stopped));
        frame["native_control"]["compactionEvents"][0] = json!({
            "id":"operation","status":"completed","nativeId":"boundary","afterUuid":"turn-end","startedAtMs":1000});
        frame["messages"].as_array_mut().unwrap().insert(3, json!({
            "type":"system","uuid":"boundary","subtype":"compact_boundary","timestampMs":2000,
            "compactMetadata":{"trigger":"auto","preTokens":2000,"postTokens":500,"durationMs":1000}}));
        let completed = project_shared_history(&frame, "completed");
        let items = completed[0]["items"].as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|item| item["type"] == "contextCompaction")
                .count(),
            1
        );
        assert_eq!(items[2]["id"], "operation");
        assert_eq!(items[2]["source"], "automatic");
        assert_eq!(items[2]["durationMs"], 1000);
        assert_eq!(items[3]["text"], "继续回复");
    }

    #[test]
    fn native_controls_compaction_usage_is_replaced_only_by_a_later_real_request() {
        let request = json!({"type":"assistant","message":{"id":"before","model":"fixture",
            "usage":{"input_tokens":10000,"output_tokens":12}}});
        let boundary = json!({"type":"system","subtype":"compact_boundary","uuid":"boundary",
            "compactMetadata":{"postTokens":2200}});
        let mut rows = vec![request.clone(), boundary];
        let compacted = native_token_usage(&rows, &json!({}));
        assert_eq!(compacted["last"]["totalTokens"], 2200);
        assert!(compacted["last"]["inputTokens"].is_null());
        rows.push(
            json!({"type":"assistant","message":{"id":"after","model":"fixture",
            "usage":{"input_tokens":2300,"output_tokens":10}}}),
        );
        assert_eq!(
            native_token_usage(&rows, &json!({}))["last"]["totalTokens"],
            2300
        );
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

    #[test]
    fn shared_starting_turn_never_invents_a_stop_from_idle_metadata() {
        let mut frame = json!({"clients":{"native-new":"web-new"},
        "native_status":{"tempo":"idle"},"messages":[
            {"type":"user","uuid":"native-new","message":{"content":"new input"}}
        ]});
        let pending = project_shared_history(&frame, "connection");
        assert_eq!(pending[0]["status"], "pending");
        assert_ne!(pending[0]["_nativeDone"], true);
        frame["native_status"]["tempo"] = json!("active");
        assert_eq!(
            project_shared_history(&frame, "connection")[0]["status"],
            "inProgress"
        );
        frame["native_status"]["tempo"] = json!("idle");
        assert_eq!(
            project_shared_history(&frame, "connection")[0]["status"],
            "pending"
        );
        frame["messages"].as_array_mut().unwrap().push(json!({
            "type":"system","subtype":"turn_duration","timestampMs":123}));
        let completed = project_shared_history(&frame, "connection");
        assert_eq!(completed[0]["status"], "completed");
        assert_eq!(completed[0]["_nativeDone"], true);
    }

    #[test]
    fn shared_terminal_evidence_ends_only_the_matching_accepted_input_without_reply() {
        let mut frame = json!({"clients":{"native-new":"web-new"},
        "native_status":{"tempo":"idle","state":"done","endedAtMs":900},"messages":[
            {"type":"user","uuid":"native-new","timestampMs":1000,"message":{"content":"new input"}}
        ]});
        assert_eq!(
            project_shared_history(&frame, "connection")[0]["status"],
            "pending"
        );
        frame["native_status"]["endedAtMs"] = json!(1100);
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns[0]["status"], "completed");
        assert_eq!(turns[0]["_nativeDone"], true);
        assert_eq!(turns[0]["_nativeEndedWithoutReply"], true);
        assert_eq!(turns[0]["turnCompletedAtMs"], 1100);
        assert_eq!(turns[0]["items"][0]["content"][0]["text"], "new input");
        frame["native_status"]["tempo"] = json!("active");
        let running = project_shared_history(&frame, "connection");
        assert_eq!(running[0]["status"], "inProgress");
        assert!(running[0]["_nativeEndedWithoutReply"].is_null());
    }

    #[tokio::test]
    async fn shared_snapshot_repairs_false_stop_and_preserves_confirmed_empty_end() {
        let (_directory, service, mut c) = fixture();
        c.shared = true;
        let mut frame = json!({"native_id":c.native_id,"job_id":&c.native_id[..8],
        "native_status":{"tempo":"idle"},"clients":{"native-old":"web-old"},
        "pending":[],"requests":{},"messages":[
            {"type":"user","uuid":"native-old","timestampMs":1000,"message":{"content":"first"}},
            {"type":"assistant","message":{"id":"reply","content":[{"type":"text","text":"done"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":2000}
        ]});
        c.turns = project_shared_history(&frame, "connection");
        c.turns[0]["_stopRequested"] = json!(true);
        c.turns[0]["status"] = json!("interrupted");
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        service
            .apply_shared_snapshot(&c.id, "connection", &frame, json!({}), false)
            .await
            .unwrap();
        let repaired = service.conversation(&c.id).await.unwrap();
        assert_eq!(repaired.turns[0]["status"], "completed");
        assert_ne!(repaired.turns[0]["_stopRequested"], true);
        frame["messages"].as_array_mut().unwrap().push(json!({
            "type":"user","uuid":"native-next","timestampMs":3000,"message":{"content":"second"}}));
        frame["clients"]["native-next"] = json!("web-next");
        frame["native_status"] = json!({"tempo":"idle","state":"done","endedAtMs":3100});
        service
            .apply_shared_snapshot(&c.id, "connection", &frame, json!({}), false)
            .await
            .unwrap();
        assert_eq!(
            service.conversation(&c.id).await.unwrap().turns[1]["_nativeEndedWithoutReply"],
            true
        );
        frame["messages"].as_array_mut().unwrap().push(json!({
            "type":"user","uuid":"native-latest","timestampMs":4000,"message":{"content":"third"}}));
        frame["native_status"] = json!({"tempo":"active"});
        service
            .apply_shared_snapshot(&c.id, "connection", &frame, json!({}), false)
            .await
            .unwrap();
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.turns[1]["_nativeEndedWithoutReply"], true);
        assert_eq!(current.turns[1]["turnCompletedAtMs"], 3100);
        assert_eq!(current.turns[2]["status"], "inProgress");
    }

    #[test]
    fn shared_idle_state_does_not_reuse_old_running_history() {
        let (_directory, _service, mut c) = fixture();
        c.shared = true;
        for tempo in ["idle", "unknown"] {
            c.native_status = json!({"tempo":tempo});
            assert!(!c.running_for_send());
            assert!(!c.active());
        }
        c.native_status = json!({"tempo":"blocked","needs":"permission"});
        assert!(c.active(), "真实审批等待仍保留执行展示");
    }

    #[test]
    fn shared_error_is_not_overwritten_by_stale_activity_metadata() {
        for tempo in ["active", "idle", "blocked"] {
            let frame = json!({"clients":{},"native_status":{"tempo":tempo},"messages":[
                {"type":"user","uuid":"native-error","message":{"content":"input"}},
                {"type":"assistant","isApiErrorMessage":true,"message":{
                    "id":"failure","content":[{"type":"text","text":"request failed"}]}}
            ]});
            assert_eq!(
                project_shared_history(&frame, "connection")[0]["status"],
                "failed",
                "{tempo}"
            );
        }
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
    fn shared_task_notification_opens_an_auto_turn_instead_of_a_user_message() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"u1","nativeEditable":true,"timestampMs":10,"message":{"content":"跑测试"}},
            {"type":"assistant","uuid":"a1","message":{"id":"m1","content":[
                {"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo test","run_in_background":true}},
                {"type":"tool_use","id":"t1","name":"TaskCreate","input":{"subject":"等待测试"}}]}},
            {"type":"user","uuid":"r1","nativeToolResult":{"toolUseId":"b1","backgroundTaskId":"bg1"},
                "message":{"content":[{"type":"tool_result","tool_use_id":"b1","content":"Command running in background with ID: bg1."}]}},
            {"type":"user","uuid":"r2","nativeToolResult":{"toolUseId":"t1","createdTaskId":"1"},
                "message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"Task #1 created"}]}},
            {"type":"assistant","uuid":"a2","message":{"id":"m2","content":[{"type":"text","text":"已在后台运行"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":20,"durationMs":10},
            {"type":"user","uuid":"n1","timestampMs":30,"nativeTaskNotification":{"taskId":"bg1","toolUseId":"b1",
                "status":"completed","summary":"Background command \"cargo test\" completed (exit code 0)"},
                "message":{"content":"<task-notification><task-id>bg1</task-id></task-notification>"}},
            {"type":"assistant","uuid":"a3","message":{"id":"m3","content":[
                {"type":"tool_use","id":"t2","name":"TaskUpdate","input":{"taskId":"1","status":"completed"}},
                {"type":"text","text":"测试全部通过"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":40,"durationMs":7}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 2);
        let first: Vec<_> = turns[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["type"].clone())
            .collect();
        assert_eq!(
            first,
            [
                json!("userMessage"),
                json!("commandExecution"),
                json!("agentMessage")
            ]
        );
        assert_eq!(turns[0]["items"][1]["backgroundStatus"], "completed");
        assert_eq!(turns[0]["items"][1]["exitCode"], 0);
        assert_eq!(
            turns[0]["plan"],
            json!([{"step":"等待测试","status":"pending"}])
        );
        // 自动续跑轮次：不是用户气泡、不可编辑，随后的回复与耗时归入新轮次，任务清单从上一轮延续。
        assert_eq!(turns[1]["id"], "n1");
        assert_eq!(turns[1]["_nativeEditable"], false);
        assert_eq!(turns[1]["items"][0]["type"], "claudeTaskNotification");
        assert_eq!(turns[1]["items"][1]["text"], "测试全部通过");
        assert_eq!(turns[1]["durationMs"], 7);
        assert_eq!(
            turns[1]["plan"],
            json!([{"step":"等待测试","status":"completed"}])
        );
        assert!(
            !turns
                .iter()
                .flat_map(|turn| turn["items"].as_array().unwrap())
                .any(|item| item["type"] == "userMessage"
                    && item["content"].to_string().contains("task-notification"))
        );
        assert_eq!(turns, project_shared_history(&frame, "connection"));
    }

    #[test]
    fn shared_withdrawn_prompt_is_closed_instead_of_waiting_to_start() {
        // 2.1.323 实测：出字前停止后原生状态为 idle（不是 done/ended），未确认撤回时网页会一直等待开始。
        let frame = json!({"native_status":{"tempo":"idle","state":"idle","withdrawnPrompt":"u1"},
            "clients":{"u1":"web-1"},"messages":[{"type":"user","uuid":"u1","timestampMs":10,
                "nativeEditable":true,"message":{"content":"用三句话介绍一下杭州西湖。"}}]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(
            (
                turns[0]["status"].clone(),
                turns[0]["_nativeWithdrawn"].clone(),
                turns[0]["_nativeEndedWithoutReply"].clone()
            ),
            (json!("completed"), json!(true), json!(true))
        );
        let mut waiting = frame.clone();
        waiting["native_status"]["withdrawnPrompt"] = Value::Null;
        let turns = project_shared_history(&waiting, "connection");
        assert_eq!(turns[0]["status"], "pending");
        assert!(turns[0]["_nativeWithdrawn"].is_null());
    }

    #[test]
    fn shared_local_commands_and_notices_render_as_events_in_the_current_turn() {
        let frame = json!({"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"c1","nativeLocalCommand":{"name":"/resume","args":""},
                "message":{"content":"<command-name>/resume</command-name>"}},
            {"type":"user","uuid":"c2","nativeLocalOutput":{"commandId":"c1","stream":"stdout","text":"已在后台运行"},
                "message":{"content":"<local-command-stdout>已在后台运行</local-command-stdout>"}},
            {"type":"user","uuid":"u1","nativeEditable":true,"message":{"content":"你好"}},
            {"type":"assistant","uuid":"a1","message":{"id":"m1","content":[{"type":"text","text":"你好！"}]}},
            {"type":"system","subtype":"turn_duration","uuid":"d1","durationMs":5},
            {"type":"system","subtype":"away_summary","uuid":"s1",
                "nativeSystemNotice":{"kind":"awaySummary","text":"已问候"}}
        ]});
        let turns = project_shared_history(&frame, "connection");
        assert_eq!(turns.len(), 2);
        assert_eq!(
            turns[0]["items"],
            json!([{"id":"c1","type":"claudeLocalCommand","source":"claude","name":"/resume","args":"",
                "output":"已在后台运行","outputStream":"stdout","completedAtMs":null}])
        );
        let types: Vec<_> = turns[1]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["type"].clone())
            .collect();
        assert_eq!(
            types,
            [
                json!("userMessage"),
                json!("agentMessage"),
                json!("claudeNotice")
            ]
        );
        assert_eq!(turns[1]["items"][2]["text"], "已问候");
        assert_eq!(
            (turns[1]["status"].clone(), turns[1]["durationMs"].clone()),
            (json!("completed"), json!(5))
        );
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

    #[test]
    fn history_edit_dispatch_unknown_keeps_native_failure_reason() {
        let failure = dispatched_unknown(ApiError::new(503, "原生回退界面未能确认"));
        assert!(failure.outcome_unknown);
        assert_eq!(failure.code, 503);
        assert_eq!(failure.message, "原生回退界面未能确认");
    }

    #[test]
    fn history_edit_metadata_supports_earlier_messages_and_requires_native_idle() {
        let (_directory, _service, mut c) = fixture();
        c.shared = true;
        c.native_history_head = "done".into();
        c.native_status = json!({"tempo":"idle","sessionStatus":"idle"});
        c.turns = project_shared_history(
            &json!({"clients":{},"native_status":{"tempo":"idle"},"messages":[
                {"type":"user","uuid":"first","nativeEditable":true,"message":{"content":"第一条"}},
                {"type":"assistant","message":{"content":[{"type":"text","text":"回复"}]}},
                {"type":"user","uuid":"second","nativeEditable":true,"message":{"content":"第二条"}},
                {"type":"system","subtype":"turn_duration"}
            ]}),
            "generation",
        );
        assert_eq!(c.view()["capabilities"]["messageEdit"], true);
        assert_eq!(c.view()["messageEditReady"], true);
        for turn in &c.turns {
            let item = &c.displayed_turn(turn)["items"][0];
            assert_eq!(item["messageMetadata"]["editable"], true);
            assert_eq!(item["messageMetadata"]["turnId"], turn["id"]);
        }
        c.native_status = json!({"tempo":"active","sessionStatus":"busy"});
        assert_eq!(c.view()["messageEditReady"], false);
        c.native_status = json!({"tempo":"unknown"});
        assert_eq!(c.view()["messageEditReady"], false);
    }

    #[tokio::test]
    async fn history_edit_retires_old_turns_and_preserves_consumed_metrics_after_restart() {
        let (directory, service, mut c) = fixture();
        c.shared = true;
        c.turns.clear();
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        let frame = |messages: Value, head: &str| {
            json!({
                "native_id":c.native_id,"job_id":"","history_head":head,
                "native_status":{"tempo":"idle","sessionStatus":"idle"},"clients":{},"pending":[],
                "requests":{},"messages":messages
            })
        };
        let rows = json!([
            {"type":"user","uuid":"first","nativeEditable":true,"message":{"content":"原问题"}},
            {"type":"assistant","uuid":"old-answer","message":{"id":"old-request","model":"test-model",
                "content":[{"type":"text","text":"原回复"}],"usage":{"input_tokens":100,"output_tokens":10}}},
            {"type":"system","subtype":"turn_duration"}
        ]);
        service
            .apply_shared_snapshot(
                &c.id,
                "generation",
                &frame(rows, "old-head"),
                json!({}),
                true,
            )
            .await
            .unwrap();
        let mut replacement = frame(
            json!([
                {"type":"user","uuid":"replacement","nativeEditable":true,"message":{"content":"修改后的问题"}},
                {"type":"assistant","uuid":"new-answer","message":{"id":"new-request","model":"test-model",
                    "content":[{"type":"text","text":"新回复"}],"usage":{"input_tokens":200,"output_tokens":20}}},
                {"type":"system","subtype":"turn_duration"}
            ]),
            "new-head",
        );
        replacement["retired_usage"] = json!([
            {"type":"assistant","uuid":"older-answer","message":{"id":"older-request","model":"test-model",
                "usage":{"input_tokens":30,"output_tokens":3}}}
        ]);
        for _ in 0..2 {
            service
                .apply_shared_snapshot(&c.id, "generation", &replacement, json!({}), true)
                .await
                .unwrap();
        }
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.turns.len(), 1);
        assert_eq!(current.turns[0]["id"], "replacement");
        assert_eq!(current.metrics()["totalTokens"], 363);
        assert_eq!(current.metrics()["turnTokens"], 220);
        assert_eq!(current.metrics()["steps"], 1);
        let restored = Claude::new(directory.path()).unwrap();
        let restored = restored.conversation(&c.id).await.unwrap();
        assert_eq!(restored.native_id, c.native_id);
        assert_eq!(restored.metrics()["totalTokens"], 363);
        assert_eq!(restored.native_history_head, "new-head");
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
