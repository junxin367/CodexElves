use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_MODEL_PAGES: usize = 128;
const SERVICE_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct RpcFailure {
    method: String,
    code: i64,
    message: String,
}

impl Display for RpcFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "RPC {} 失败（{}）：{}",
            self.method, self.code, self.message
        )
    }
}

impl std::error::Error for RpcFailure {}

struct RunningService {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    executable: String,
    initialized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    path: PathBuf,
    shell: bool,
}

pub struct CodexCliService {
    process: Mutex<Option<RunningService>>,
    next_request_id: AtomicU64,
    cooldown_until: Mutex<Option<Instant>>,
}

impl CodexCliService {
    pub fn new() -> Self {
        Self {
            process: Mutex::new(None),
            next_request_id: AtomicU64::new(1),
            cooldown_until: Mutex::new(None),
        }
    }

    pub async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        match action {
            "models" => self.models().await,
            "skills" => self.skills(payload).await,
            "thread" => self.thread(payload).await,
            "workspaceContext" => self.workspace_context(payload).await,
            _ => bail!("Codex CLI service 暂不支持远程操作 {action}"),
        }
    }

    async fn models(&self) -> anyhow::Result<Value> {
        self.ensure_service_available().await?;
        let mut process_guard = self.process.lock().await;
        if process_guard.is_none() {
            *process_guard = Some(spawn_service().await?);
        }

        let result = match self
            .ensure_initialized(process_guard.as_mut().unwrap())
            .await
        {
            Ok(()) => {
                self.models_with_process(process_guard.as_mut().unwrap())
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            self.handle_request_failure(&mut process_guard, "models", error)
                .await;
        }
        result
    }

    async fn skills(&self, payload: Value) -> anyhow::Result<Value> {
        self.ensure_service_available().await?;
        let thread_id = payload.get("threadId").and_then(Value::as_str);
        let cwd = match payload.get("cwd").and_then(Value::as_str) {
            Some(cwd) if !cwd.trim().is_empty() => cwd.to_owned(),
            _ if thread_id.is_some() => {
                let thread = self
                    .request(
                        "thread/read",
                        json!({
                            "threadId": thread_id.unwrap(),
                            "includeTurns": false
                        }),
                    )
                    .await?;
                thread
                    .get("thread")
                    .and_then(|value| value.get("cwd"))
                    .and_then(Value::as_str)
                    .filter(|cwd| !cwd.trim().is_empty())
                    .map(str::to_owned)
                    .context("Codex CLI service 未返回会话工作目录")?
            }
            _ => bail!("Codex CLI service 需要会话工作目录"),
        };
        let response = self
            .request(
                "skills/list",
                json!({
                    "cwds": [cwd],
                    "forceReload": payload.get("forceReload").and_then(Value::as_bool).unwrap_or(false)
                }),
            )
            .await?;
        let entry = response
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|entry| entry.get("cwd").and_then(Value::as_str) == Some(cwd.as_str()));
        let data = entry
            .and_then(|entry| entry.get("skills"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let errors = entry
            .and_then(|entry| entry.get("errors"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(json!({
            "ok": true,
            "cwd": cwd,
            "data": data,
            "errors": errors
        }))
    }

    async fn thread(&self, payload: Value) -> anyhow::Result<Value> {
        let thread_id = payload
            .get("threadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("缺少会话 ID")?;
        let response = self
            .request(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false
                }),
            )
            .await?;
        let thread = response
            .get("thread")
            .context("Codex CLI service 未返回会话元数据")?;
        let normalized = normalize_thread(thread)?;
        let goal = self
            .request("thread/goal/get", json!({ "threadId": thread_id }))
            .await
            .ok()
            .and_then(|value| value.get("goal").cloned())
            .unwrap_or(Value::Null);
        let mut normalized = normalized;
        normalized["goal"] = goal;
        Ok(json!({ "ok": true, "thread": normalized }))
    }

    async fn workspace_context(&self, payload: Value) -> anyhow::Result<Value> {
        let thread_id = payload
            .get("threadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("Codex CLI service 需要 threadId")?;
        let response = self
            .request(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false
                }),
            )
            .await?;
        let cwd = response
            .get("thread")
            .and_then(|thread| thread.get("cwd"))
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.trim().is_empty())
            .context("Codex CLI service 未返回会话工作目录")?;
        Ok(json!({
            "ok": true,
            "hostId": "local",
            "threadId": thread_id,
            "cwd": cwd
        }))
    }

    async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let mut process_guard = self.process.lock().await;
        if process_guard.is_none() {
            *process_guard = Some(spawn_service().await?);
        }
        let process = process_guard.as_mut().unwrap();
        let result = match self.ensure_initialized(process).await {
            Ok(()) => {
                let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
                rpc_request(process, request_id, method, params).await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            self.handle_request_failure(&mut process_guard, method, error)
                .await;
        }
        result
    }

    async fn ensure_service_available(&self) -> anyhow::Result<()> {
        let now = Instant::now();
        let mut cooldown = self.cooldown_until.lock().await;
        let Some(until) = *cooldown else {
            return Ok(());
        };
        if until <= now {
            *cooldown = None;
            return Ok(());
        }
        let retry_after = until.duration_since(now).as_secs().max(1);
        bail!("Codex CLI service 暂时跳过，约 {retry_after} 秒后重试");
    }

    async fn handle_request_failure(
        &self,
        process_guard: &mut Option<RunningService>,
        action: &str,
        error: &anyhow::Error,
    ) {
        if is_nonfatal_optional_rpc_failure(action, error) {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.codex_service_optional_unavailable",
                json!({
                    "action": action,
                    "error": truncate_error(&error.to_string())
                }),
            );
            return;
        }

        if let Some(process) = process_guard.as_mut() {
            let _ = process.child.start_kill();
        }
        *process_guard = None;
        self.enter_cooldown_if_needed(action, error).await;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service_rpc_failed",
            json!({
                "action": action,
                "error": truncate_error(&error.to_string())
            }),
        );
    }

    async fn enter_cooldown_if_needed(&self, action: &str, error: &anyhow::Error) {
        if !should_enter_service_cooldown(action, error) {
            return;
        }
        let now = Instant::now();
        let mut cooldown = self.cooldown_until.lock().await;
        if cooldown.is_some_and(|until| until > now) {
            return;
        }
        *cooldown = Some(now + SERVICE_FAILURE_COOLDOWN);
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service_cooldown",
            json!({
                "action": action,
                "retryAfterSeconds": SERVICE_FAILURE_COOLDOWN.as_secs(),
                "error": truncate_error(&error.to_string())
            }),
        );
    }

    async fn ensure_initialized(&self, process: &mut RunningService) -> anyhow::Result<()> {
        if process.initialized {
            return Ok(());
        }
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        rpc_request(
            process,
            request_id,
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-elves-remote-access",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }),
        )
        .await
        .context("Codex CLI service 初始化失败")?;
        send_notification(process, "initialized", json!({})).await?;
        process.initialized = true;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service.initialized",
            json!({ "executable": process.executable }),
        );
        Ok(())
    }

    async fn models_with_process(&self, process: &mut RunningService) -> anyhow::Result<Value> {
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        let mut cursors = HashSet::new();
        for _ in 0..MAX_MODEL_PAGES {
            let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            let page = rpc_request(
                process,
                request_id,
                "model/list",
                json!({
                    "cursor": cursor,
                    "limit": 100,
                    "includeHidden": false
                }),
            )
            .await
            .context("Codex CLI service 模型列表读取失败")?;
            if let Some(entries) = page.get("data").and_then(Value::as_array) {
                models.extend(entries.iter().filter_map(normalize_model));
            }
            let next_cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
            let Some(next) = next_cursor.as_str().filter(|value| !value.is_empty()) else {
                break;
            };
            if !cursors.insert(next.to_string()) {
                bail!("Codex CLI service 模型列表分页游标重复");
            }
            cursor = Value::String(next.to_string());
        }
        if models.is_empty() {
            bail!("Codex CLI service 返回的模型列表为空");
        }

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let config = rpc_request(
            process,
            request_id,
            "config/read",
            json!({ "cwd": null, "includeLayers": false }),
        )
        .await
        .context("Codex CLI service 配置读取失败")?;
        let config = config.get("config").unwrap_or(&config);
        let default_model = config
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        let default_reasoning_effort = config
            .get("model_reasoning_effort")
            .or_else(|| config.get("modelReasoningEffort"))
            .or_else(|| config.get("reasoningEffort"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        let ultra_enabled = models.iter().any(|model| {
            model["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|effort| effort["reasoningEffort"] == "ultra")
        });

        Ok(json!({
            "ok": true,
            "data": models,
            "ultraReasoningEnabled": ultra_enabled,
            "defaultModel": default_model,
            "defaultReasoningEffort": default_reasoning_effort
        }))
    }
}

async fn spawn_service() -> anyhow::Result<RunningService> {
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
    let mut command = if candidate.shell {
        let command_line = format!(
            "\"{}\" app-server --analytics-default-enabled",
            candidate.path.display()
        );
        let mut command = Command::new(if cfg!(windows) { "cmd.exe" } else { "sh" });
        if cfg!(windows) {
            command.args(["/d", "/s", "/c", &command_line]);
        } else {
            command.args(["-lc", &command_line]);
        }
        command
    } else {
        let mut command = Command::new(&candidate.path);
        command.args(["app-server", "--analytics-default-enabled"]);
        command
    };

    let working_directory = std::env::current_dir().context("读取 CodexElves 工作目录失败")?;
    command
        .current_dir(working_directory)
        .env("CODEX_HOME", codex_home)
        .env("HOME", codex_home)
        .env("USERPROFILE", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_create_hidden_process());

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 {} 失败", candidate.path.display()))?;
    let stdin = child
        .stdin
        .take()
        .context("Codex CLI service 标准输入不可用")?;
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
        executable: candidate.path.to_string_lossy().to_string(),
        initialized: false,
    })
}

async fn discover_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |path: PathBuf, shell: bool| {
        let key = format!("{}:{shell}", path.to_string_lossy().to_ascii_lowercase());
        if seen.insert(key) {
            candidates.push(Candidate { path, shell });
        }
    };

    for path in crate::cli_wrapper::default_user_runtime_candidates() {
        if path.is_file() {
            add(path, false);
        }
    }
    if let Ok(output) = Command::new(if cfg!(windows) { "where.exe" } else { "sh" })
        .args(if cfg!(windows) {
            vec!["codex".to_string()]
        } else {
            vec!["-lc".to_string(), "command -v codex".to_string()]
        })
        .output()
        .await
    {
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
    candidates
}

async fn send_notification(
    process: &mut RunningService,
    method: &str,
    params: Value,
) -> anyhow::Result<()> {
    let line = serde_json::to_string(&json!({
        "method": method,
        "params": params
    }))?;
    process.stdin.write_all(line.as_bytes()).await?;
    process.stdin.write_all(b"\n").await?;
    process.stdin.flush().await?;
    Ok(())
}

async fn rpc_request(
    process: &mut RunningService,
    request_id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    let line = serde_json::to_string(&json!({
        "id": request_id,
        "method": method,
        "params": params
    }))?;
    process.stdin.write_all(line.as_bytes()).await?;
    process.stdin.write_all(b"\n").await?;
    process.stdin.flush().await?;

    loop {
        let mut response_line = String::new();
        let bytes_read = tokio::time::timeout(
            REQUEST_TIMEOUT,
            process.stdout.read_line(&mut response_line),
        )
        .await
        .context("等待 Codex CLI service 响应超时")??;
        if bytes_read == 0 {
            let status = process.child.try_wait().ok().flatten();
            bail!("Codex CLI service 已退出（状态：{status:?}）");
        }
        let response: Value = match serde_json::from_str(response_line.trim()) {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.get("id").and_then(Value::as_u64) != Some(request_id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(anyhow::Error::new(RpcFailure {
                method: method.to_string(),
                code,
                message: message.to_string(),
            }));
        }
        return Ok(response.get("result").cloned().unwrap_or(Value::Null));
    }
}

fn rpc_failure(error: &anyhow::Error) -> Option<&RpcFailure> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RpcFailure>())
}

fn is_nonfatal_optional_rpc_failure(method: &str, error: &anyhow::Error) -> bool {
    let Some(failure) = rpc_failure(error) else {
        return false;
    };
    method == "thread/goal/get" && matches!(failure.code, -32600 | -32601)
}

fn is_configuration_failure(error: &anyhow::Error) -> bool {
    let text = error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
        .to_ascii_lowercase();
    text.contains("config.toml")
        || text.contains("failed to reload config")
        || text.contains("invalid configuration")
}

fn should_enter_service_cooldown(action: &str, error: &anyhow::Error) -> bool {
    matches!(action, "models" | "skills" | "skills/list") && is_configuration_failure(error)
}

fn normalize_model(model: &Value) -> Option<Value> {
    if model.get("hidden").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let model_name = model
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| model.get("id").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())?;
    let supported = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some(json!({
        "id": model.get("id").and_then(Value::as_str).unwrap_or(model_name),
        "model": model_name,
        "displayName": model.get("displayName").and_then(Value::as_str).unwrap_or(model_name),
        "supportedReasoningEfforts": supported,
        "defaultReasoningEffort": model.get("defaultReasoningEffort").cloned().unwrap_or(Value::Null)
    }))
}

fn normalize_thread(thread: &Value) -> anyhow::Result<Value> {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Codex CLI service 返回了无效会话 ID")?;
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
    if ephemeral || parent_thread_id.is_some() || thread_source == Some("subagent") {
        bail!("Codex CLI service 只提供主会话元数据");
    }
    let cwd = thread
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Codex CLI service 返回了无效会话工作目录")?;
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| thread.get("title").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("新会话");
    let status = thread
        .get("status")
        .cloned()
        .unwrap_or_else(|| json!("idle"));
    let active = status
        .as_str()
        .is_some_and(|value| matches!(value, "active" | "inProgress" | "running"));
    Ok(json!({
        "id": id,
        "title": title,
        "cwd": cwd,
        "createdAt": thread.get("createdAt").cloned().unwrap_or(Value::Null),
        "updatedAt": thread.get("updatedAt").cloned().unwrap_or(Value::Null),
        "recencyAt": thread.get("recencyAt").cloned().unwrap_or_else(|| thread.get("updatedAt").cloned().unwrap_or(Value::Null)),
        "model": thread.get("model").cloned().unwrap_or(Value::Null),
        "reasoningEffort": thread.get("reasoningEffort").cloned().unwrap_or(Value::Null),
        "status": status.clone(),
        "priority": {
            "status": status,
            "attentionState": if active { "active" } else { "idle" },
            "active": active,
            "eligible": true
        },
        "hasUnreadTurn": false,
        "readOnly": false,
        "viewKind": "main",
        "parentThreadId": Value::Null,
        "ephemeral": false,
        "requests": [],
        "queuedMessages": [],
        "queuedEdits": [],
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
    fn unsupported_goal_rpc_is_optional_and_keeps_service_available() {
        let error = anyhow::Error::new(RpcFailure {
            method: "thread/goal/get".to_string(),
            code: -32600,
            message: "goals feature is disabled".to_string(),
        });
        assert!(is_nonfatal_optional_rpc_failure("thread/goal/get", &error));
        assert!(!is_nonfatal_optional_rpc_failure("thread/read", &error));
    }

    #[test]
    fn unsupported_model_rpc_is_not_treated_as_optional() {
        let error = anyhow::Error::new(RpcFailure {
            method: "model/list".to_string(),
            code: -32601,
            message: "method not found".to_string(),
        });
        assert!(!is_nonfatal_optional_rpc_failure("model/list", &error));
    }

    #[test]
    fn configuration_failures_are_marked_for_cooldown() {
        let error = anyhow::Error::new(RpcFailure {
            method: "config/read".to_string(),
            code: -32603,
            message: "failed to reload config: C:/Users/test/.codex/config.toml".to_string(),
        });
        assert!(is_configuration_failure(&error));
        assert!(should_enter_service_cooldown("models", &error));
        assert!(should_enter_service_cooldown("skills/list", &error));
        assert!(!should_enter_service_cooldown("thread/read", &error));
    }

    #[cfg(windows)]
    #[test]
    fn service_process_flags_disable_console_creation_and_attachment() {
        assert_eq!(crate::windows_create_hidden_process(), 0x08000008);
    }
}
