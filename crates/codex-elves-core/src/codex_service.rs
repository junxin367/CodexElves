use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::mpsc;

use crate::desktop_owner_ipc::{self, WriteFrame};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

// 用户要求移除远程 CDP 及旧兜底路由；仅保留服务模式桌面 IPC 共用的纯快照差异计算。
pub(crate) struct ServiceProjection;

impl ServiceProjection {
    pub(crate) fn snapshot_changes(thread_id: &str, before: &Value, after: &Value) -> Vec<Value> {
        // 桌面观察只计算本次差异；回放历史由统一服务通道保存，不能每个订阅再缓存 2,048 条正文事件。
        let mut events = Vec::new();
        for (field, method, parameter) in [
            ("threadRuntimeStatus", "thread/status/changed", "status"),
            (
                "latestTokenUsageInfo",
                "thread/tokenUsage/updated",
                "tokenUsage",
            ),
        ] {
            if before.get(field) != after.get(field) && after.get(field).is_some() {
                let mut params = json!({"threadId":thread_id});
                params[parameter] = after[field].clone();
                events.push(json!({"method":method,"params":params}));
            }
        }
        if before["title"] != after["title"] && after["title"].as_str().is_some() {
            events.push(json!({"method":"thread/name/updated","params":{
                "threadId":thread_id,"name":after["title"]}}));
        }
        if before["requests"] != after["requests"] && after["requests"].is_array() {
            events.push(json!({"method":"remote/requestsChanged","params":{
                "threadId":thread_id,"requests":after["requests"]}}));
        }
        if before["threadGoal"] != after["threadGoal"] {
            events.push(if after["threadGoal"].is_null() {
                json!({"method":"thread/goal/cleared","params":{"threadId":thread_id}})
            } else {
                json!({"method":"thread/goal/updated","params":{
                    "threadId":thread_id,"goal":after["threadGoal"]}})
            });
        }
        let old_turns = desktop_owner_ipc::snapshot_turns(before);
        let max_started = old_turns
            .iter()
            .filter_map(|turn| turn["turnStartedAtMs"].as_u64())
            .max();
        let old_turns: BTreeMap<_, _> = old_turns
            .into_iter()
            .filter_map(|turn| {
                turn["turnId"]
                    .as_str()
                    .map(str::to_string)
                    .map(|id| (id, turn))
            })
            .collect();
        let mut observed_turns = desktop_owner_ipc::snapshot_turns(after);
        // 桌面完整快照按原生轮次时间发布，避免对象键顺序让新轮次开始后又显示旧轮次完成。
        observed_turns.sort_by_key(|turn| turn["turnStartedAtMs"].as_u64().unwrap_or(0));
        for mut turn in observed_turns {
            let Some(turn_id) = turn["turnId"].as_str().map(str::to_string) else {
                continue;
            };
            let previous = old_turns.get(&turn_id);
            // 长会话只处理有变化的轮次，避免每个流式快照重复遍历已经稳定的历史内容。
            if previous == Some(&turn) {
                continue;
            }
            let new_live_turn = previous.is_some()
                || turn["status"] == "inProgress"
                || turn["turnStartedAtMs"].as_u64().is_some_and(|started| {
                    max_started.map_or_else(
                        || {
                            before["turnHistory"]["kind"] == "canonical"
                                && before["turnHistory"]["history"]["isComplete"] == true
                        },
                        |latest| started > latest,
                    )
                });
            if !new_live_turn {
                // Historical pagination is not a newly completed turn.
                continue;
            }
            turn["id"] = json!(&turn_id);
            let status = turn["status"].as_str().unwrap_or_default();
            let terminal = matches!(
                status,
                "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
            );
            let changed_status = previous.is_none_or(|old| old["status"] != turn["status"]);
            if changed_status && (status == "inProgress" || terminal) {
                let method = if terminal {
                    "turn/completed"
                } else {
                    "turn/started"
                };
                events.push(json!({"method":method,"params":{"threadId":thread_id,
                    "turnId":turn_id,"turn":turn}}));
                continue;
            }
            let old_items = previous.and_then(|old| old["items"].as_array());
            let mut old_by_id = HashMap::new();
            for item in old_items.into_iter().flatten() {
                if let Some(id) = item["id"].as_str() {
                    old_by_id.entry(id).or_insert(item);
                }
            }
            for item in turn["items"].as_array().into_iter().flatten() {
                // 稳定消息 ID 建立一次索引，避免长轮次的每次更新进行平方次数的逐项查找。
                let old_item = if let Some(id) = item["id"].as_str() {
                    old_by_id.get(id).copied()
                } else {
                    old_items.and_then(|items| items.iter().find(|old| old["id"] == item["id"]))
                };
                if old_item != Some(item) {
                    // Exact observed item state, not invented deltas or completion.
                    events.push(json!({"method":"item/updated","params":{
                        "threadId":thread_id,"turnId":turn_id,"item":item}}));
                }
            }
        }
        events
    }
}

pub trait ServiceDispatchPermit: Send {
    fn commit(
        self: Box<Self>,
        dispatch: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()>;
}

#[async_trait]
pub trait ServiceDispatchGuard: Send + Sync {
    async fn prepare_dispatch(&self) -> anyhow::Result<Box<dyn ServiceDispatchPermit>>;
}

pub(crate) struct RunningService {
    pub(crate) child: Child,
    pub(crate) stdin: mpsc::Sender<WriteFrame>,
    pub(crate) stdout: BufReader<ChildStdout>,
}

#[cfg(test)]
impl RunningService {
    pub(crate) fn for_test(mut child: Child) -> anyhow::Result<Self> {
        let (stdin, _) = desktop_owner_ipc::spawn_writer(
            child.stdin.take().context("missing stdin")?,
            REQUEST_TIMEOUT,
        );
        let stdout = BufReader::new(child.stdout.take().context("missing stdout")?);
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    path: PathBuf,
    shell: bool,
    version: Option<crate::cli_wrapper::CodexCliVersion>,
    ordinal: usize,
}

pub(crate) async fn spawn_service() -> anyhow::Result<RunningService> {
    let settings = crate::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    let codex_home = crate::codex_home::codex_home_dir_for_settings(&settings);
    let candidates = discover_candidates().await;
    let mut errors = Vec::new();
    for candidate in candidates {
        match spawn_candidate(&candidate, &codex_home).await {
            Ok(service) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "remote_access.codex_service.started",
                    json!({
                        "executable": candidate.path.to_string_lossy(),
                        "shell": candidate.shell,
                        "processId": service.child.id(),
                        "codexHome": codex_home.to_string_lossy(),
                        "hidden": true
                    }),
                );
                return Ok(service);
            }
            Err(error) => errors.push(format!("{}: {error}", candidate.path.display())),
        }
    }
    let message = if errors.is_empty() {
        "未找到 Codex CLI 可执行文件".to_string()
    } else {
        format!("无法启动 Codex CLI app-server：{}", errors.join("；"))
    };
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "remote_access.codex_service.unavailable",
        json!({ "error": truncate_error(&message) }),
    );
    bail!(message)
}

async fn spawn_candidate(
    candidate: &Candidate,
    codex_home: &Path,
) -> anyhow::Result<RunningService> {
    let mut command = candidate_command(candidate);

    let working_directory = std::env::current_dir().context("读取 CodexElves 工作目录失败")?;
    // 服务模式只指定 Codex 数据目录；保留真实 HOME/USERPROFILE，避免 Cargo、Git 等工具读错用户配置。
    command
        .current_dir(working_directory)
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 {} 失败", candidate.path.display()))?;
    let stdin = child
        .stdin
        .take()
        .context("Codex CLI service 标准输入不可用")?;
    let (stdin, _writer_task) = desktop_owner_ipc::spawn_writer(stdin, REQUEST_TIMEOUT);
    let stdout = child
        .stdout
        .take()
        .context("Codex CLI service 标准输出不可用")?;
    if let Some(stderr) = child.stderr.take() {
        let executable = candidate.path.to_string_lossy().to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "remote_access.codex_service.stderr",
                    json!({
                        "executable": executable,
                        "message": truncate_error(&line)
                    }),
                );
            }
        });
    }
    Ok(RunningService {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

fn candidate_command(candidate: &Candidate) -> Command {
    #[cfg(not(windows))]
    if candidate.shell {
        let command_line = format!(
            "\"{}\" app-server --analytics-default-enabled",
            candidate.path.display()
        );
        let mut command = Command::new("sh");
        command.args(["-lc", &command_line]);
        return command;
    }

    // Rust invokes .cmd/.bat through cmd.exe with the required quoting. Building
    // a cmd /c string with ordinary args escapes its quotes as literal backslashes.
    let mut command = Command::new(&candidate.path);
    command.args(["app-server", "--analytics-default-enabled"]);
    #[cfg(windows)]
    command.creation_flags(crate::windows_create_hidden_process());
    command
}

async fn discover_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |path: PathBuf, shell: bool| {
        if crate::cli_wrapper::is_desktop_codex_path(&path) {
            return;
        }
        let key = format!("{}:{shell}", path.to_string_lossy().to_ascii_lowercase());
        if seen.insert(key) {
            let ordinal = candidates.len();
            candidates.push(Candidate {
                path,
                shell,
                version: None,
                ordinal,
            });
        }
    };

    for path in crate::cli_wrapper::default_user_runtime_candidates() {
        if path.is_file() {
            add(path, false);
        }
    }
    let mut discovery_command = Command::new(if cfg!(windows) { "where.exe" } else { "sh" });
    discovery_command.args(if cfg!(windows) {
        vec!["codex".to_string()]
    } else {
        vec!["-lc".to_string(), "command -v codex".to_string()]
    });
    #[cfg(windows)]
    discovery_command.creation_flags(crate::windows_create_hidden_process());
    if let Ok(output) = discovery_command.output().await {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = PathBuf::from(line.trim());
            if path.as_os_str().is_empty() {
                continue;
            }
            if cfg!(windows) && path.extension().is_none() {
                continue;
            }
            let shell = path.extension().is_none()
                || path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("cmd")
                        || extension.eq_ignore_ascii_case("bat")
                        || extension.eq_ignore_ascii_case("sh")
                });
            add(path, shell);
        }
    }
    add(PathBuf::from("codex"), true);
    // 保留主线的 CLI 选择约束：按实际版本优先使用较新运行时，不能误启桌面可执行文件。
    for candidate in &mut candidates {
        candidate.version = crate::cli_wrapper::probe_codex_cli_version(&candidate.path);
    }
    candidates.sort_by(compare_candidates);
    candidates
}

fn compare_candidates(left: &Candidate, right: &Candidate) -> std::cmp::Ordering {
    match (&left.version, &right.version) {
        (Some(left), Some(right)) => crate::cli_wrapper::compare_codex_versions(right, left),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.ordinal.cmp(&right.ordinal),
    }
    .then_with(|| left.shell.cmp(&right.shell))
    .then_with(|| left.path.cmp(&right.path))
}

pub(crate) fn normalize_model(model: &Value) -> Option<Value> {
    if model.get("hidden").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let model_name = model
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| model.get("id").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())?;
    let mut supported = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 用户要求：没有桌面注入时，GPT 默认提供 Ultra；此补齐仅作用于 CLI 兜底，
    // 桌面界面增强仍按 Codex 的可见等级和功能开关独立处理。
    if model_name
        .trim()
        .rsplit('/')
        .next()
        .is_some_and(|slug| slug.to_ascii_lowercase().starts_with("gpt-"))
        && !supported
            .iter()
            .any(|effort| effort["reasoningEffort"] == "ultra")
    {
        supported.push(json!({
            "reasoningEffort": "ultra",
            "description": "Maximum reasoning with automatic task delegation"
        }));
    }
    let service_tiers = model
        .get("serviceTiers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let request_model = model
        .get("requestModel")
        .or_else(|| model.get("request_model"))
        .and_then(Value::as_str)
        .unwrap_or(model_name);
    // Fast 仅开放给实际请求的 GPT 模型及目录明确提供的档位；显式不支持、未知和非 GPT 保持关闭。
    let fast_supported = request_model
        .trim()
        .rsplit('/')
        .next()
        .is_some_and(|slug| slug.to_ascii_lowercase().starts_with("gpt-"))
        && !["supportsFast", "supports_fast", "fastModeSupported"]
            .iter()
            .any(|key| model.get(*key).and_then(Value::as_bool) == Some(false))
        && service_tiers
            .iter()
            .any(|tier| matches!(tier["id"].as_str(), Some("priority" | "fast")));
    Some(json!({
        "id": model.get("id").and_then(Value::as_str).unwrap_or(model_name),
        "model": model_name,
        "requestModel":request_model,
        "displayName": model.get("displayName").and_then(Value::as_str).unwrap_or(model_name),
        "supportedReasoningEfforts": supported,
        "defaultReasoningEffort": model.get("defaultReasoningEffort").cloned().unwrap_or(Value::Null),
        "serviceTiers":service_tiers,
        "fastModeSupported":fast_supported
    }))
}

fn service_history_items_page(page: &Value) -> (Vec<Value>, Option<String>) {
    let mut items = page
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| entry.get("item").cloned().unwrap_or_else(|| entry.clone()))
        .collect::<Vec<_>>();
    items.reverse();
    let next_cursor = page
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    (items, next_cursor)
}

pub(crate) fn normalize_history_items_page(
    page: &Value,
    previous_cursor: Option<&str>,
) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let (items, next_cursor) = service_history_items_page(page);
    // 保留主线的分页防循环检查；历史项顺序和消息原始字段仍由新版解析逻辑处理。
    anyhow::ensure!(
        previous_cursor.is_none() || next_cursor.as_deref() != previous_cursor,
        "历史分页游标未前进"
    );
    Ok((items, next_cursor))
}

pub(crate) fn owner_snapshot_history(snapshot: &Value, thread_id: &str) -> Option<Value> {
    let complete = if snapshot["turnHistory"]["kind"] == "canonical" {
        snapshot["turnHistory"]["history"]["isComplete"] == true
    } else {
        snapshot["turnsPagination"]["hasLoadedOldest"] == true
    };
    let mut turns = desktop_owner_ipc::snapshot_turns(snapshot);
    if !complete
        || turns.iter().any(|turn| {
            turn.get("itemsPagination").is_some_and(|pagination| {
                pagination["hasLoadedOldest"] != true || pagination["isLoadingOlder"] == true
            })
        })
    {
        return None;
    }
    turns.retain(|turn| turn["turnId"].as_str().is_some_and(|id| !id.is_empty()));
    turns.sort_by(|left, right| {
        right["turnStartedAtMs"]
            .as_u64()
            .cmp(&left["turnStartedAtMs"].as_u64())
            .then_with(|| right["turnId"].as_str().cmp(&left["turnId"].as_str()))
    });
    let mut pagination = serde_json::Map::new();
    for turn in &mut turns {
        let id = turn["turnId"].as_str()?.to_string();
        turn["id"] = json!(&id);
        pagination.insert(
            id,
            json!({"complete":true,"pageComplete":true,"hasMore":false,
                "nextCursor":null,"direction":"older","order":"asc"}),
        );
    }
    Some(json!({
        "ok":true,"scope":"turns","threadId":thread_id,"data":turns,
        "nextCursor":null,"backwardsCursor":null,"turnOrder":"desc","itemOrder":"asc",
        "turnsPageComplete":true,"turnsComplete":true,"itemsPaginationByTurnId":pagination,
        "transcriptEvents":[],"nativeBridge":"desktop-owner-ipc"
    }))
}

pub(crate) async fn local_queue_is_empty(
    home: &Path,
    thread_id: &str,
    snapshot: &Value,
) -> anyhow::Result<bool> {
    let rollout = snapshot["rolloutPath"]
        .as_str()
        .context("桌面会话数据目录未知，无法核对本地队列")?;
    let sessions = tokio::fs::canonicalize(home.join("sessions")).await?;
    let rollout = tokio::fs::canonicalize(rollout).await?;
    anyhow::ensure!(
        rollout.starts_with(&sessions),
        "桌面与后台使用不同的会话数据目录，未发送"
    );
    let state_path = home.join(".codex-global-state.json");
    anyhow::ensure!(
        tokio::fs::metadata(&state_path).await?.len() <= 16 * 1024 * 1024,
        "桌面本地队列状态文件超过大小限制"
    );
    let state: Value = serde_json::from_slice(&tokio::fs::read(&state_path).await?)
        .context("桌面本地队列状态无法解析")?;
    let queue = state.get("queued-follow-ups");
    anyhow::ensure!(
        state.is_object() && queue.is_none_or(Value::is_object),
        "桌面本地队列状态格式未知"
    );
    let messages = queue.and_then(|queue| queue.get(thread_id));
    anyhow::ensure!(
        messages.is_none_or(Value::is_array),
        "桌面本地队列状态格式未知"
    );
    Ok(messages.is_none_or(|messages| messages.as_array().is_some_and(Vec::is_empty)))
}

pub(crate) fn normalize_thread_response(response: &Value) -> anyhow::Result<Value> {
    let thread = response
        .get("thread")
        .context("Codex CLI service 未返回会话元数据")?;
    let mut normalized = normalize_thread(thread)?;
    if let Some(read_state) = response.get("readState") {
        normalized["readState"] = read_state.clone();
        if let Some(first_unread) = read_state.get("firstUnread") {
            normalized["hasUnreadTurn"] = json!(!first_unread.is_null());
        }
    }
    Ok(normalized)
}

pub(crate) fn normalize_owner_thread(
    snapshot: &Value,
    metadata: Option<&Value>,
) -> anyhow::Result<Value> {
    let mut thread = snapshot.clone();
    thread["status"] = snapshot["threadRuntimeStatus"].clone();
    thread["model"] = snapshot["latestModel"].clone();
    thread["reasoningEffort"] = snapshot["latestReasoningEffort"].clone();
    if ["name", "title", "preview"].iter().all(|key| {
        snapshot[*key]
            .as_str()
            .is_none_or(|text| text.trim().is_empty())
    }) {
        if let Some(title) = metadata
            .and_then(|thread| thread.get("title"))
            .filter(|value| value.as_str().is_some_and(|title| !title.trim().is_empty()))
        {
            thread["title"] = title.clone();
        }
    }
    let mut normalized = normalize_thread(&thread)?;
    if let Some(metadata) = metadata {
        for field in ["createdAt", "updatedAt", "recencyAt"] {
            if normalized[field].is_null() {
                normalized[field] = metadata[field].clone();
            }
        }
        if let Some(read_state) = metadata.get("readState") {
            normalized["readState"] = read_state.clone();
        }
        if normalized["hasUnreadTurn"].is_null() {
            normalized["hasUnreadTurn"] = metadata["hasUnreadTurn"].clone();
        }
    }
    normalized["requests"] = snapshot["requests"].clone();
    normalized["requestsKnown"] = json!(snapshot["requests"].is_array());
    normalized["goal"] = snapshot["threadGoal"].clone();
    normalized["tokenUsage"] = snapshot["latestTokenUsageInfo"].clone();
    // 用户要求服务模式切回桌面持有的会话也恢复压缩；IPC 快照已有记录，不能在元数据转换时丢弃。
    let mut turns = desktop_owner_ipc::snapshot_turns(snapshot);
    turns.sort_by_key(|turn| turn["turnStartedAtMs"].as_u64().unwrap_or(0));
    let events: Vec<_> = turns
        .iter()
        .flat_map(|turn| {
            turn["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["type"] == "contextCompaction")
                .map(|item| json!({"turnId":turn["turnId"],"item":item}))
        })
        .collect();
    if let Some(event) = events.last() {
        let item = &event["item"];
        let turn_ended = turns
            .iter()
            .find(|turn| turn["turnId"] == event["turnId"])
            .is_some_and(|turn| {
                turn["status"].as_str().is_some_and(|status| {
                    matches!(
                        status,
                        "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
                    )
                })
            });
        let completed = item["completed"] == true
            || item["completedAtMs"].as_u64().is_some()
            || item["status"].as_str().is_some_and(|status| {
                matches!(
                    status,
                    "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
                )
            });
        normalized["compaction"] = json!({
            "state":if completed || turn_ended {"idle"} else if item["id"] == "pending-manual-context-compaction" {"pending"} else {"running"},
            "turnId":event["turnId"],"itemId":item["id"]});
    }
    normalized["transcriptEvents"] = json!(events);
    Ok(normalized)
}

pub(crate) fn normalize_thread(thread: &Value) -> anyhow::Result<Value> {
    let ephemeral = thread
        .get("ephemeral")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let parent_thread_id = thread
        .get("parentThreadId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let thread_source = thread
        .get("threadSource")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let source_is_subagent = thread
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subAgent") || source.contains_key("subagent"));
    if ephemeral
        || parent_thread_id.is_some()
        || source_is_subagent
        || thread_source.is_some_and(|source| source.eq_ignore_ascii_case("subagent"))
        || thread.get("sideConversation") == Some(&Value::Bool(true))
    {
        bail!("Codex CLI service 只提供主会话元数据");
    }
    normalize_thread_metadata(thread)
}

pub(crate) fn subagent_parent_id(thread: &Value) -> Option<&str> {
    thread["parentThreadId"]
        .as_str()
        .or_else(|| thread["source"]["subAgent"]["thread_spawn"]["parent_thread_id"].as_str())
        .or_else(|| thread["source"]["subagent"]["thread_spawn"]["parent_thread_id"].as_str())
        .filter(|parent| !parent.is_empty() && thread["id"] != *parent)
}

pub(crate) fn normalize_subagent_thread(thread: &Value) -> anyhow::Result<Value> {
    let parent = subagent_parent_id(thread).context("子智能体缺少有效父会话关系")?;
    anyhow::ensure!(
        thread["sideConversation"] != true,
        "侧边会话不能作为子智能体读取"
    );
    let mut value = normalize_thread_metadata(thread)?;
    // 用户要求服务模式恢复子智能体只读查看；此专用转换不改变普通会话和所有写入口的校验。
    value["readOnly"] = json!(true);
    value["viewKind"] = json!("subagent");
    value["parentThreadId"] = json!(parent);
    value["agentName"] = thread["agentNickname"]
        .as_str()
        .or_else(|| thread["source"]["subAgent"]["thread_spawn"]["agent_nickname"].as_str())
        .or_else(|| thread["source"]["subagent"]["thread_spawn"]["agent_nickname"].as_str())
        .or_else(|| thread["agentRole"].as_str())
        .map(|name| json!(name))
        .unwrap_or_else(|| value["title"].clone());
    value["priority"]["eligible"] = json!(false);
    value["requests"] = json!([]);
    value["requestsKnown"] = json!(true);
    value["queuedMessages"] = json!([]);
    value["queuedEdits"] = json!([]);
    value["editableMessage"] = Value::Null;
    Ok(value)
}

// 小于此值视为 Unix 秒；与远程会话服务的时间戳归一化规则
// 及 app.js 的 sidebarTimestamp() 使用同一阈值，三处需同步核对。
const UNIX_SECONDS_LIMIT: u64 = 100_000_000_000;

// 用户要求服务模式会话的排序和日期分组与 Codex App / CDP 一致：app-server 的
// createdAt / updatedAt 是 Unix 秒，CDP 与 Claude 会话为毫秒，统一输出毫秒。
// 已是毫秒的值（如侧边会话的 now_ms()）保持不变，重复规范化不会再次放大。
fn thread_timestamp_ms(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    if let Some(at) = value.as_u64() {
        return json!(if at < UNIX_SECONDS_LIMIT {
            at.saturating_mul(1000)
        } else {
            at
        });
    }
    match value.as_f64() {
        Some(at) if at.is_finite() && at > 0.0 => json!(if at < UNIX_SECONDS_LIMIT as f64 {
            (at * 1000.0).round() as u64
        } else {
            at.round() as u64
        }),
        _ => value.clone(),
    }
}

pub(crate) fn normalize_thread_metadata(thread: &Value) -> anyhow::Result<Value> {
    let id = thread["id"]
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .context("Codex CLI service 返回了无效会话 ID")?;
    let thread_source = thread["threadSource"].as_str();
    let cwd = thread
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Codex CLI service 返回了无效会话工作目录")?;
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            thread
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            thread
                .get("preview")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("新会话");
    let status = thread.get("status").cloned().unwrap_or(Value::Null);
    let status_type = status.as_str().or_else(|| status["type"].as_str());
    let active = match status_type {
        Some("active" | "inProgress" | "running") => Some(true),
        Some("idle") => Some(false),
        _ => None,
    };
    let unread = thread.get("hasUnreadTurn").and_then(Value::as_bool);
    Ok(json!({
        "id": id,
        "title": title,
        "cwd": cwd,
        "createdAt": thread_timestamp_ms(thread.get("createdAt")),
        "updatedAt": thread_timestamp_ms(thread.get("updatedAt")),
        "recencyAt": thread_timestamp_ms(thread.get("recencyAt").or_else(|| thread.get("updatedAt"))),
        "model": thread.get("model").cloned().unwrap_or(Value::Null),
        "reasoningEffort": thread.get("reasoningEffort").cloned().unwrap_or(Value::Null),
        "status": status.clone(),
        "priority": {
            "status": status,
            "attentionState": match active { Some(true) => "active", Some(false) => "idle", None => "unknown" },
            "active": active,
            "eligible": true
        },
        "hasUnreadTurn": unread,
        "readOnly": false,
        "viewKind": "main",
        "parentThreadId": Value::Null,
        "ephemeral": thread.get("ephemeral").cloned().unwrap_or(Value::Null),
        "threadSource": thread_source,
        "source": thread.get("source").cloned().unwrap_or(Value::Null),
        "requests": thread.get("requests").cloned().unwrap_or(Value::Null),
        "requestsKnown": thread.get("requests").is_some_and(Value::is_array),
        "queuedMessages": Value::Null,
        "queuedEdits": Value::Null,
        "transcriptEvents": []
    }))
}

fn truncate_error(value: &str) -> String {
    const MAX_ERROR_CHARS: usize = 1_000;
    value.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_thread_metadata_preserves_preview_object_status_and_outer_read_state() {
        let raw = json!({
            "thread":{"id":"main","name":null,"preview":"真实首条消息",
                "cwd":"C:/workspace","source":"vscode","threadSource":"user",
                "ephemeral":false,"parentThreadId":null,"status":{"type":"active"}},
            "readState":{"firstUnread":{"turnId":"unread-turn"},"revision":21}
        });
        let normalized = normalize_thread_response(&raw).unwrap();
        assert_eq!(normalized["title"], "真实首条消息");
        assert_eq!(normalized["priority"]["active"], true);
        assert_eq!(normalized["hasUnreadTurn"], true);
        assert_eq!(normalized["readState"], raw["readState"]);
        let mut unloaded = raw;
        unloaded["thread"]["status"]["type"] = json!("notLoaded");
        unloaded.as_object_mut().unwrap().remove("readState");
        let unknown = normalize_thread_response(&unloaded).unwrap();
        assert!(unknown["priority"]["active"].is_null());
        assert_eq!(unknown["priority"]["attentionState"], "unknown");
        assert!(unknown["hasUnreadTurn"].is_null());
    }

    #[test]
    fn service_history_items_page_restores_order_and_preserves_payloads_and_cursor() {
        let user = json!({
            "type":"userMessage","id":"user-1","clientId":"client-1",
            "content":[{"type":"text","text":"测试消息","text_elements":[]}]
        });
        let reasoning = json!({
            "type":"reasoning","id":"reasoning-1","summary":[],"content":[]
        });
        let assistant = json!({
            "type":"agentMessage","id":"assistant-1","text":"完整回复",
            "phase":"final_answer","memoryCitation":null,"delivery":null,"questions":null
        });
        let expected = vec![user.clone(), reasoning.clone(), assistant.clone()];
        let cursor = r#"{"requestedThreadId":"main","rolloutOrdinal":30,"scope":{"kind":"items","turnId":"turn-1"}}"#;
        for page in [
            json!({
                "data":[
                    {"item":assistant,"turnId":"turn-1"},
                    {"item":reasoning,"turnId":"turn-1"},
                    {"item":user,"turnId":"turn-1"}
                ],
                "nextCursor":cursor
            }),
            json!({"data":[assistant,reasoning,user],"nextCursor":cursor}),
        ] {
            let (items, next_cursor) = service_history_items_page(&page);
            assert_eq!(items, expected);
            assert_eq!(next_cursor.as_deref(), Some(cursor));
        }
    }

    #[test]
    fn owner_snapshot_history_only_claims_complete_for_complete_items_and_keeps_turn_identity() {
        let snapshot = json!({"turnHistory":{"kind":"canonical","history":{
            "isComplete":true,"entitiesByKey":{
                "tail":{"turnId":"accepted-turn","status":"completed","turnStartedAtMs":1000,
                    "params":{"clientUserMessageId":"stable-message"},
                    "items":[{"type":"agentMessage","text":"done"}]}
            }
        }}});
        let history = owner_snapshot_history(&snapshot, "main").unwrap();
        assert_eq!(history["data"][0]["id"], "accepted-turn");
        assert_eq!(
            history["data"][0]["params"]["clientUserMessageId"],
            "stable-message"
        );
        assert_eq!(history["turnsComplete"], true);
        let mut partial = snapshot;
        partial["turnHistory"]["history"]["entitiesByKey"]["tail"]["itemsPagination"] =
            json!({"hasLoadedOldest":false});
        assert!(owner_snapshot_history(&partial, "main").is_none());
    }

    #[test]
    fn owner_compaction_snapshot_preserves_reentry_state_without_a_new_message() {
        let mut snapshot = json!({"id":"main","cwd":"test","threadRuntimeStatus":{"type":"active"},"requests":[],
            "turns":[{"turnId":"compact-turn","status":"inProgress","turnStartedAtMs":1000,
                "items":[{"id":"compact","type":"contextCompaction","startedAtMs":1000}]}]});
        let running = normalize_owner_thread(&snapshot, None).unwrap();
        assert_eq!(running["compaction"]["state"], "running");
        assert_eq!(running["transcriptEvents"][0]["item"]["startedAtMs"], 1000);
        snapshot["turns"][0]["items"][0]["completedAtMs"] = json!(2000);
        let completed = normalize_owner_thread(&snapshot, None).unwrap();
        assert_eq!(completed["compaction"]["state"], "idle");
        assert_eq!(
            completed["transcriptEvents"][0]["item"]["completedAtMs"],
            2000
        );
        snapshot["turns"][0]["items"][0]
            .as_object_mut()
            .unwrap()
            .remove("completedAtMs");
        snapshot["turns"][0]["status"] = json!("interrupted");
        assert_eq!(
            normalize_owner_thread(&snapshot, None).unwrap()["compaction"]["state"],
            "idle"
        );
    }
    #[test]
    fn normalize_model_keeps_ultra_and_drops_hidden_entries() {
        let model = normalize_model(&json!({
            "id": "gpt-6-astra",
            "model": "gpt-6-astra",
            "displayName": "GPT-6 Astra",
            "hidden": false,
            "supportedReasoningEfforts": [
                {"reasoningEffort": "medium"},
                {"reasoningEffort": "ultra"}
            ],
            "defaultReasoningEffort": "medium"
        }))
        .unwrap();
        assert_eq!(model["model"], "gpt-6-astra");
        assert_eq!(
            model["supportedReasoningEfforts"][1]["reasoningEffort"],
            "ultra"
        );
        assert!(normalize_model(&json!({"model":"hidden","hidden":true})).is_none());
    }

    #[test]
    fn service_model_catalog_adds_ultra_only_for_gpt_without_changing_defaults() {
        for model in ["gpt-5.5", "gpt-6.1-sol", "openai/gpt-6.1-terra"] {
            let original = json!({
                "model": model,
                "defaultReasoningEffort": "xhigh",
                "supportedReasoningEfforts": [{"reasoningEffort": "xhigh"}]
            });
            let normalized = normalize_model(&original).unwrap();
            assert_eq!(normalized["defaultReasoningEffort"], "xhigh");
            assert_eq!(
                normalized["supportedReasoningEfforts"][0]["reasoningEffort"],
                "xhigh"
            );
            assert_eq!(
                normalized["supportedReasoningEfforts"][1]["reasoningEffort"],
                "ultra"
            );
            assert_eq!(normalize_model(&normalized).unwrap(), normalized);
            assert_eq!(
                original["supportedReasoningEfforts"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        for model in ["claude-opus-5", "kimi-k3", "o3", "gptish-model"] {
            let normalized = normalize_model(&json!({
                "model": model,
                "supportedReasoningEfforts": [{"reasoningEffort": "high"}]
            }))
            .unwrap();
            assert_eq!(
                normalized["supportedReasoningEfforts"],
                json!([{"reasoningEffort": "high"}])
            );
        }
    }

    #[test]
    fn normalize_model_uses_id_when_model_name_is_missing() {
        let model = normalize_model(&json!({
            "id": "gpt-fallback",
            "supportedReasoningEfforts": []
        }))
        .unwrap();
        assert_eq!(model["model"], "gpt-fallback");
        assert_eq!(model["displayName"], "gpt-fallback");
    }

    #[test]
    fn normalize_thread_keeps_main_session_metadata_and_rejects_ephemeral_sessions() {
        let thread = normalize_thread(&json!({
            "id": "thread-1",
            "name": "主会话",
            "cwd": "C:/workspace",
            "createdAt": 1,
            "updatedAt": 2,
            "status": "idle",
            "ephemeral": false
        }))
        .unwrap();
        assert_eq!(thread["id"], "thread-1");
        assert_eq!(thread["title"], "主会话");
        assert_eq!(thread["viewKind"], "main");
        assert_eq!(thread["createdAt"], 1000);
        assert_eq!(thread["updatedAt"], 2000);
        assert_eq!(thread["recencyAt"], 2000);
        // app-server 返回 Unix 秒；侧栏日期分组与排序按毫秒比较，已是毫秒的值不能再放大。
        let seconds = normalize_thread(&json!({
            "id": "thread-seconds", "cwd": "C:/workspace",
            "createdAt": 1792319939, "updatedAt": 1792320819
        }))
        .unwrap();
        assert_eq!(seconds["createdAt"], 1_792_319_939_000_u64);
        assert_eq!(seconds["updatedAt"], 1_792_320_819_000_u64);
        assert_eq!(seconds["recencyAt"], 1_792_320_819_000_u64);
        let millis = normalize_thread_metadata(&seconds).unwrap();
        assert_eq!(millis["createdAt"], 1_792_319_939_000_u64);
        assert_eq!(millis["updatedAt"], 1_792_320_819_000_u64);
        assert!(
            normalize_thread(&json!({
                "id": "side-1",
                "cwd": "C:/workspace",
                "ephemeral": true
            }))
            .is_err()
        );
    }

    #[test]
    fn service_history_rejects_a_repeated_item_cursor() {
        let error = normalize_history_items_page(
            &json!({"data": [], "nextCursor": "same-cursor"}),
            Some("same-cursor"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("历史分页游标未前进"));
    }

    #[test]
    fn service_candidates_prefer_newest_cli_runtime() {
        let mut candidates = vec![
            Candidate {
                path: PathBuf::from("codex-old.exe"),
                shell: false,
                version: crate::cli_wrapper::parse_codex_cli_version("codex-cli 0.130.0-alpha.5"),
                ordinal: 0,
            },
            Candidate {
                path: PathBuf::from("codex-current.exe"),
                shell: false,
                version: crate::cli_wrapper::parse_codex_cli_version("codex-cli 0.166.0-alpha.5.1"),
                ordinal: 1,
            },
        ];
        candidates.sort_by(compare_candidates);
        assert_eq!(candidates[0].path, PathBuf::from("codex-current.exe"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_batch_candidate_preserves_arguments_output_and_exit_status() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Codex CLI & wrapper.cmd");
        std::fs::write(
            &path,
            "@echo off\r\necho arguments=%*\r\necho service-stderr 1>&2\r\nexit /b 19\r\n",
        )
        .unwrap();
        let output = candidate_command(&Candidate {
            path,
            shell: true,
            version: None,
            ordinal: 0,
        })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
        assert_eq!(output.status.code(), Some(19));
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("arguments=app-server --analytics-default-enabled")
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("service-stderr"));
    }
}
