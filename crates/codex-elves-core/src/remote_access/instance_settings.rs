//! 设置按实际实例隔离；仅修改明确支持的原生键，远程专属偏好不写入原生配置。
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use std::{collections::BTreeMap, sync::OnceLock};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::permissions::{self, ConfigFile};
use super::{ApiError, RemoteAuthorization, WebState};

const PREFERENCES_FILE: &str = "remote-instance-preferences.json";
type HelpCache = BTreeMap<String, (Instant, String)>;
static CLAUDE_HELP: OnceLock<Mutex<HelpCache>> = OnceLock::new();

fn preferences(directory: &Path) -> Result<(Value, String), ApiError> {
    let (text, revision) = permissions::read_file(&directory.join(PREFERENCES_FILE))?;
    let value = if text.trim().is_empty() {
        json!({"instances":{}})
    } else {
        serde_json::from_str::<Value>(&text)
            .map_err(|_| ApiError::new(422, "实例偏好文件格式无效，未覆盖原文件"))?
    };
    if !value.is_object() || !value["instances"].is_object() {
        return Err(ApiError::new(422, "实例偏好文件格式无效，未覆盖原文件"));
    }
    Ok((value, revision))
}

pub(super) fn ultra_enabled(directory: &Path, instance: &str) -> Result<bool, ApiError> {
    let (value, _) = preferences(directory)?;
    // 用户要求 Codex 默认不开放 Ultra；仅当前实例的显式开启值允许显示，不能按模型支持情况自动开启。
    match &value["instances"][instance]["ultraReasoningEnabled"] {
        Value::Null => Ok(false),
        Value::Bool(enabled) => Ok(*enabled),
        _ => Err(ApiError::new(
            422,
            "Ultra 思考等级配置无效，请重新读取实例设置",
        )),
    }
}

pub(super) fn filter_model_rows(models: &mut [Value], enabled: bool) {
    if enabled {
        return;
    }
    for model in models {
        if let Some(efforts) = model["supportedReasoningEfforts"].as_array_mut() {
            efforts.retain(|effort| effort["reasoningEffort"] != "ultra");
        }
        if model["defaultReasoningEffort"] == "ultra" {
            model["defaultReasoningEffort"] = model["supportedReasoningEfforts"]
                .as_array()
                .and_then(|efforts| efforts.last())
                .map(|effort| effort["reasoningEffort"].clone())
                .unwrap_or(Value::Null);
        }
    }
}

pub(super) fn filter_models(value: &mut Value, enabled: bool) {
    if let Some(models) = value["data"].as_array_mut() {
        filter_model_rows(models, enabled);
    }
    value["ultraReasoningEnabled"] = json!(
        enabled
            && value["data"]
                .as_array()
                .is_some_and(|models| models.iter().any(|model| {
                    model["supportedReasoningEfforts"]
                        .as_array()
                        .is_some_and(|efforts| {
                            efforts
                                .iter()
                                .any(|effort| effort["reasoningEffort"] == "ultra")
                        })
                }))
    );
    if !enabled && value["defaultReasoningEffort"] == "ultra" {
        value["defaultReasoningEffort"] = Value::Null;
    }
}

pub(super) fn claude_auto_compact(
    directory: &Path,
    instance: &str,
) -> Result<Option<String>, ApiError> {
    let (value, _) = preferences(directory)?;
    let value = &value["instances"][instance]["autoCompactWindow"];
    validate_compact(value)?;
    Ok(match value {
        Value::Null => None,
        Value::String(value) => Some(value.clone()),
        value => Some(value.to_string()),
    })
}

fn validate_compact(value: &Value) -> Result<(), ApiError> {
    if value.is_null()
        || value == "auto"
        || value
            .as_u64()
            .is_some_and(|tokens| (100_000..=1_000_000).contains(&tokens))
    {
        Ok(())
    } else {
        Err(ApiError::new(
            400,
            "自动压缩窗口必须为自动或 100000～1000000 Token",
        ))
    }
}

async fn save_preference(
    directory: &Path,
    instance: &str,
    key: &str,
    value: &Value,
    expected: &str,
    authorization: &RemoteAuthorization,
) -> Result<(), ApiError> {
    let (mut document, revision) = preferences(directory)?;
    if revision != expected {
        return Err(ApiError::new(409, "实例偏好已变化，请重新读取后再保存"));
    }
    let instances = document["instances"].as_object_mut().unwrap();
    let entry = instances.entry(instance).or_insert_with(|| json!({}));
    let entry = entry
        .as_object_mut()
        .ok_or_else(|| ApiError::new(422, "实例偏好格式无效，未覆盖原文件"))?;
    if value.is_null() {
        entry.remove(key);
    } else {
        entry.insert(key.to_owned(), value.clone());
    }
    let output = serde_json::to_string_pretty(&document).unwrap() + "\n";
    let permit = authorization
        .prepare_dispatch()
        .await
        .map_err(super::authorization_error)?;
    permit
        .commit(|| permissions::save_file(&directory.join(PREFERENCES_FILE), &output, expected))
        .map_err(super::authorization_error)
}

async fn claude_help(config: &super::claude::WslConfig) -> Result<String, ApiError> {
    let key = format!("{}:{}", config.instance_id(), config.executable);
    let cache = CLAUDE_HELP.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some((at, text)) = cache.lock().await.get(&key)
        && at.elapsed() < Duration::from_secs(60)
    {
        return Ok(text.clone());
    }
    config
        .validate()
        .map_err(|_| ApiError::new(400, "Claude 实例身份或 CLI 路径无效"))?;
    // 用户要求能力查询只走所属 WSL CLI，并固定在该用户 Home 的登录环境，不加载项目上下文。
    let mut command = tokio::process::Command::new("wsl.exe");
    command.args([
        "--distribution",
        &config.distribution,
        "--user",
        &config.user,
        "--cd",
        "~",
    ]);
    super::claude::Claude::wsl_login_exec(&mut command, &config.executable);
    command.arg("--help");
    let text = String::from_utf8(super::preferences::output(command).await?)
        .map_err(|_| ApiError::new(503, "Claude CLI 帮助返回格式无效"))?;
    cache
        .lock()
        .await
        .insert(key, (Instant::now(), text.clone()));
    Ok(text)
}

fn native_document(backend: &str, text: &str) -> Result<Value, ApiError> {
    if backend == "codex" {
        let value = toml::from_str::<toml::Value>(text)
            .map_err(|_| ApiError::new(422, "Codex 配置格式无效，未覆盖原文件"))?;
        serde_json::to_value(value).map_err(|_| ApiError::new(422, "Codex 配置格式无效"))
    } else {
        let value = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|_| ApiError::new(422, "Claude 配置格式无效，未覆盖原文件"))?
        };
        if !value.is_object() {
            return Err(ApiError::new(422, "Claude 配置必须为 JSON 对象"));
        }
        Ok(value)
    }
}

fn optional_bool(value: &Value) -> Result<Value, ApiError> {
    if value.is_null() || value.is_boolean() {
        Ok(value.clone())
    } else {
        Err(ApiError::new(422, "实例配置中的开关类型无效，未覆盖原配置"))
    }
}

fn general_view(backend: &str, document: &Value) -> Result<Value, ApiError> {
    if backend == "codex" {
        let legacy_agents = optional_bool(&document["features"]["multi_agent"])?;
        let v2_agents = optional_bool(&document["features"]["multi_agent_v2"]["enabled"])?;
        let agents = if legacy_agents == true || v2_agents == true {
            json!(true)
        } else if legacy_agents == false && v2_agents == false {
            json!(false)
        } else {
            Value::Null
        };
        Ok(json!({
            "todo":optional_bool(&document["tools"]["update_plan"]["enabled"])?,
            "memories":optional_bool(&document["features"]["memories"])?,
            "multiAgent":agents,
            "goals":optional_bool(&document["features"]["goals"])?
        }))
    } else {
        Ok(json!({
            "awaySummaryEnabled":optional_bool(&document["awaySummaryEnabled"])?,
            "promptSuggestionEnabled":optional_bool(&document["promptSuggestionEnabled"])?
        }))
    }
}

fn set_toml(
    item: &mut toml_edit::Item,
    path: &[&str],
    value: Option<toml_edit::Value>,
) -> Result<(), ApiError> {
    let table = item
        .as_table_like_mut()
        .ok_or_else(|| ApiError::new(422, "原生配置结构不兼容，未覆盖原文件"))?;
    let key = path[0];
    if path.len() == 1 {
        if let Some(mut value) = value {
            if let Some(original) = table.get(key).and_then(toml_edit::Item::as_value) {
                *value.decor_mut() = original.decor().clone();
            }
            table.insert(key, toml_edit::Item::Value(value));
        } else {
            table.remove(key);
        }
    } else {
        if !table.contains_key(key) {
            if value.is_none() {
                return Ok(());
            }
            let mut child = toml_edit::Table::new();
            child.set_implicit(true);
            table.insert(key, toml_edit::Item::Table(child));
        }
        set_toml(table.get_mut(key).unwrap(), &path[1..], value)?;
    }
    Ok(())
}

fn toml_output(original: &str, document: &toml_edit::DocumentMut) -> String {
    // 原生配置编辑必须保留 BOM 和换行格式；常规设置与 MCP 共用，避免 toml_edit 渲染时丢失这些信息。
    let mut output = document.to_string();
    if original.contains("\r\n") {
        output = output.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    if original.starts_with('\u{feff}') && !output.starts_with('\u{feff}') {
        output.insert(0, '\u{feff}');
    }
    output
}

fn updated_native(
    backend: &str,
    section: &str,
    text: &str,
    changes: &Value,
) -> Result<String, ApiError> {
    let changes = changes
        .as_object()
        .filter(|changes| !changes.is_empty() && changes.len() <= 4)
        .ok_or_else(|| ApiError::new(400, "请选择需要保存的设置"))?;
    if backend == "codex" {
        let mut document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| ApiError::new(422, "Codex 配置格式无效，未覆盖原文件"))?;
        if section == "models"
            && changes.contains_key("model")
            && !changes.contains_key("reasoningEffort")
        {
            // 切换默认模型时同步撤销旧模型深度；独立 API 调用也遵守页面的同一约定。
            set_toml(document.as_item_mut(), &["model_reasoning_effort"], None)?;
        }
        for (key, value) in changes {
            let path: &[&str] = match (section, key.as_str()) {
                ("general", "todo") => &["tools", "update_plan", "enabled"],
                ("general", "memories") => &["features", "memories"],
                ("general", "multiAgent") => &["features", "multi_agent_v2", "enabled"],
                ("general", "goals") => &["features", "goals"],
                ("models", "model") => &["model"],
                ("models", "reasoningEffort") => &["model_reasoning_effort"],
                ("hooks", "enabled") => &["features", "hooks"],
                _ => return Err(ApiError::new(400, "不支持的 Codex 实例设置")),
            };
            let replacement = if value.is_null() {
                None
            } else if section == "general" || section == "hooks" {
                Some(toml_edit::Value::from(
                    value
                        .as_bool()
                        .ok_or_else(|| ApiError::new(400, "开关必须为布尔值"))?,
                ))
            } else {
                Some(toml_edit::Value::from(
                    value
                        .as_str()
                        .filter(|value| {
                            !value.is_empty()
                                && value.len() <= 200
                                && !value.chars().any(char::is_control)
                        })
                        .ok_or_else(|| ApiError::new(400, "模型或思考等级无效"))?,
                ))
            };
            // 用户要求 Todo、记忆、子智能体和目标模式按实例控制；保留原文件注释及所有其他配置。
            if section == "general" && key == "multiAgent" {
                // 当前 CLI 同时提供两代子智能体；总开关关闭两者，不能只关闭 V2 后回退旧版。
                set_toml(
                    document.as_item_mut(),
                    &["features", "multi_agent"],
                    replacement.clone(),
                )?;
            }
            set_toml(document.as_item_mut(), path, replacement)?;
        }
        Ok(toml_output(text, &document))
    } else {
        let mut document = native_document(backend, text)?;
        let root = document.as_object_mut().unwrap();
        for (key, value) in changes {
            match (section, key.as_str()) {
                ("general", "awaySummaryEnabled") if value.is_boolean() => {}
                ("general", "promptSuggestionEnabled") if value.is_null() || value.is_boolean() => {
                }
                ("hooks", "enabled") if value.is_boolean() => {
                    root.insert("disableAllHooks".into(), json!(!value.as_bool().unwrap()));
                    continue;
                }
                // CLI 仅确认 model 是可持久化的原生设置；effortLevel 未获确认，不能写入后显示假成功。
                ("models", "model")
                    if value.is_null()
                        || value.as_str().is_some_and(|value| {
                            !value.is_empty()
                                && value.len() <= 200
                                && !value.chars().any(char::is_control)
                        }) => {}
                _ => return Err(ApiError::new(400, "不支持的 Claude 实例设置")),
            }
            if value.is_null() {
                root.remove(key);
            } else {
                root.insert(key.clone(), value.clone());
            }
        }
        let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut output = serde_json::to_string_pretty(&document)
            .unwrap()
            .replace('\n', ending);
        if text.starts_with('\u{feff}') {
            output.insert(0, '\u{feff}');
        }
        output.push_str(ending);
        Ok(output)
    }
}

fn extension_view(section: &str, backend: &str, document: &Value) -> Result<Value, ApiError> {
    // 扩展页只返回名称、状态和数量，不把 MCP 环境变量、Header 或 Hook 命令及其中凭据发送给网页。
    if backend == "claude" {
        let disabled = optional_bool(&document["disableAllHooks"])?;
        let rows =
            if section == "hooks" {
                document["hooks"].as_object().into_iter().flatten().map(|(event, rules)| json!({
                "id":event,"name":event,
                "description":format!("{} 条匹配规则",rules.as_array().map_or(0,Vec::len)),
                "canToggle":false
            })).collect::<Vec<_>>()
            } else {
                vec![]
            };
        return Ok(json!({"data":rows,
            "enabled":disabled.as_bool().map(|disabled|!disabled),
            "message":"仅显示所选实例用户级配置；项目、插件和组织策略继续由原生 CLI 管理。"}));
    }
    Ok(match section {
        "mcp" => {
            if let Some(servers) = document["mcp_servers"].as_object() {
                for value in servers.values() {
                    optional_bool(&value["enabled"])?;
                }
            }
            let rows = document["mcp_servers"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, value)| {
                    json!({
                        "id":name,"name":name,"enabled":value["enabled"],"canToggle":true,
                        "transport":if value["command"].is_string() {"stdio"} else if value["url"].is_string() {"http"} else {"未指定连接方式"}
                    })
                })
                .collect::<Vec<_>>();
            json!({"data":rows,"message":"显示用户级配置中的 MCP 服务；项目和插件服务继续由原生 CLI 管理。"})
        }
        "hooks" => {
            let rows = document["hooks"]["state"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, value)| {
                    json!({
                        "id":name,"name":name,"enabled":value["enabled"].as_bool()
                    })
                })
                .collect::<Vec<_>>();
            json!({"data":rows,"enabled":optional_bool(&document["features"]["hooks"])?})
        }
        _ => json!({"data":[]}),
    })
}

fn updated_mcp(text: &str, change: &Value) -> Result<String, ApiError> {
    let id = change["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 200)
        .ok_or_else(|| ApiError::new(400, "请选择当前实例的 MCP 服务"))?;
    let enabled = change["enabled"]
        .as_bool()
        .ok_or_else(|| ApiError::new(400, "MCP 开关必须为布尔值"))?;
    let original = native_document("codex", text)?;
    if !original["mcp_servers"]
        .as_object()
        .is_some_and(|servers| servers.get(id).is_some_and(Value::is_object))
    {
        return Err(ApiError::new(409, "MCP 服务已不在当前实例中，请重新读取"));
    }
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| ApiError::new(422, "Codex 配置格式无效，未覆盖原文件"))?;
    set_toml(
        document.as_item_mut(),
        &["mcp_servers", id, "enabled"],
        Some(toml_edit::Value::from(enabled)),
    )?;
    Ok(toml_output(text, &document))
}

async fn claude_mcp(
    state: &WebState,
    config: &super::claude::WslConfig,
    authorization: &RemoteAuthorization,
) -> Result<Value, ApiError> {
    if state.settings.read_only {
        return Ok(json!({"data":[],"message":
            "只读访问不会启动 Claude MCP 健康检查，请在该实例的原生 CLI 中查看。"}));
    }
    let mut command = tokio::process::Command::new("wsl.exe");
    command.args([
        "--distribution",
        &config.distribution,
        "--user",
        &config.user,
        "--cd",
        "~",
    ]);
    super::claude::Claude::wsl_login_exec(&mut command, &config.executable);
    command
        .args(["mcp", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_create_hidden_process());
    // CLI 会检测已批准的 MCP；派发前再次检查写保护，不能在授权变成只读后启动本机工具进程。
    let probe = state
        .controller
        .authorization(authorization.device.clone(), Some("probeInstanceSettings"));
    let permit = probe
        .prepare_dispatch()
        .await
        .map_err(super::authorization_error)?;
    let child = permit
        .commit(|| Ok(command.spawn()?))
        .map_err(super::authorization_error)?;
    let output = tokio::time::timeout(Duration::from_secs(25), child.wait_with_output())
        .await
        .map_err(|_| ApiError::new(504, "Claude MCP 检查超时，请在该实例原生 CLI 中核对"))?
        .map_err(|_| ApiError::new(503, "无法读取所选 Claude 实例的 MCP 列表"))?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(ApiError::new(
            503,
            "Claude CLI 未成功返回 MCP 列表，请在该实例原生 CLI 中核对",
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| ApiError::new(503, "Claude MCP 列表编码无效"))?;
    let rows = text
        .lines()
        .filter_map(|line| {
            let (name, status) = line.trim().split_once(": ")?;
            if name.is_empty()
                || name.len() > 160
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.@/-".contains(c))
            {
                return None;
            }
            let status = if status.contains("Connected") {
                "已连接"
            } else if status.contains("Pending approval") {
                "等待原生批准"
            } else if status.contains("Failed") {
                "连接失败"
            } else if status.to_ascii_lowercase().contains("authentication") {
                "需要登录"
            } else if status.to_ascii_lowercase().contains("disabled") {
                "已关闭"
            } else {
                return None;
            };
            // 不转发 CLI 原文：命令、URL、环境和 Header 可能包含凭据，只返回受限名称和状态。
            Some(json!({"id":name,"name":name,"description":status,"canToggle":false}))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() && !text.to_ascii_lowercase().contains("no mcp servers") {
        return Err(ApiError::new(
            503,
            "Claude CLI 返回的 MCP 列表格式尚未支持，未将其显示为空列表",
        ));
    }
    Ok(json!({"data":rows,"message":
        "由所选实例的 Claude CLI 返回；服务增删和项目授权继续在同一 CLI 中管理。"}))
}

async fn model_catalog(
    state: &WebState,
    instance: &str,
    authorization: &RemoteAuthorization,
) -> Result<Value, ApiError> {
    let value = state
        .controller
        .call_authorized(
            "models",
            json!({"instanceId":instance,"includeAvailable":true,"forceReload":true}),
            authorization,
        )
        .await
        .map_err(super::authorization_error)?;
    super::native_result(value)
}

fn validate_models(
    backend: &str,
    document: &Value,
    changes: &Value,
    catalog: &Value,
    ultra: bool,
) -> Result<(), ApiError> {
    if backend == "claude" && changes.get("effortLevel").is_some() {
        return Err(ApiError::new(
            409,
            "尚未确认当前 Claude CLI 的默认思考等级配置，请在会话输入区选择。",
        ));
    }
    let model = changes
        .get("model")
        .unwrap_or(&document["model"])
        .as_str()
        .or_else(|| catalog["defaultModel"].as_str());
    let effort_key = if backend == "codex" {
        "reasoningEffort"
    } else {
        "effortLevel"
    };
    let effort = changes.get(effort_key).and_then(Value::as_str);
    if changes["model"].is_null() && changes.get("model").is_some() && effort.is_some() {
        return Err(ApiError::new(
            400,
            "默认模型选择“使用原生默认”时，默认思考等级需选择“使用模型默认”。",
        ));
    }
    let selected = model.and_then(|model| {
        catalog["data"]
            .as_array()?
            .iter()
            .find(|row| row["model"] == model)
    });
    // 移除失效的模型覆盖不依赖旧模型仍在目录中；只有明确选择新模型时才要求目录命中。
    if changes.get("model").is_some_and(|value| !value.is_null()) && selected.is_none() {
        return Err(ApiError::new(409, "模型已不在当前实例目录中，请重新读取"));
    }
    if let Some(effort) = effort {
        if effort == "ultra" && (backend != "codex" || !ultra) {
            return Err(ApiError::new(409, "当前实例未开放 Ultra 思考等级"));
        }
        if !selected.is_some_and(|model| {
            model["supportedReasoningEfforts"]
                .as_array()
                .is_some_and(|values| {
                    values
                        .iter()
                        .any(|value| value["reasoningEffort"] == effort)
                })
        }) {
            return Err(ApiError::new(409, "当前模型不支持该思考等级，请重新选择"));
        }
    }
    Ok(())
}

async fn save_native(
    target: &ConfigFile,
    text: &str,
    revision: &str,
    payload: &Value,
    authorization: &RemoteAuthorization,
) -> Result<(), ApiError> {
    if payload["expectedRevision"] != revision {
        return Err(ApiError::new(409, "实例配置已变化，请重新读取后再保存"));
    }
    let output = if target.backend == "codex" && payload["section"] == "mcp" {
        let changes = payload["settings"].as_object().unwrap();
        if changes.len() != 1 || !changes.contains_key("server") {
            return Err(ApiError::new(400, "只允许切换已有 MCP 服务"));
        }
        updated_mcp(text, &changes["server"])?
    } else {
        updated_native(
            &target.backend,
            payload["section"].as_str().unwrap_or("general"),
            text,
            &payload["settings"],
        )?
    };
    let native_changes = if target.backend == "claude" && payload["section"] == "hooks" {
        json!({"disableAllHooks":!payload["settings"]["enabled"].as_bool().unwrap()})
    } else {
        payload["settings"].clone()
    };
    permissions::save_settings_file(target, &output, &native_changes, revision, authorization).await
}

pub(super) async fn settings(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let instance = payload["instanceId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 160)
        .ok_or_else(|| ApiError::new(400, "请选择会话实例"))?;
    let section = payload["section"].as_str().unwrap_or("general");
    if !["general", "models", "skills", "mcp", "hooks"].contains(&section) {
        return Err(ApiError::new(400, "实例设置分类无效"));
    }
    let write = match payload["operation"].as_str().unwrap_or("read") {
        "read" => false,
        "save" => true,
        _ => return Err(ApiError::new(400, "实例设置操作无效")),
    };
    if write && state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    let authorization = state
        .controller
        .authorization(device, write.then_some("setInstanceSettings"));
    let environments = super::preferences::detect(&state.controller.state_dir, false).await;
    let target = permissions::config_target(&environments, instance)?;
    let adapter_read_only = target.backend == "codex" && instance != "codex:local";
    if write && adapter_read_only {
        return Err(ApiError::new(
            409,
            "该 Codex 实例的独立执行适配器尚未接入，原生配置仅供查看",
        ));
    }
    if target.backend == "codex"
        && instance != "codex:local"
        && ["models", "skills", "hooks"].contains(&section)
    {
        return Err(ApiError::new(
            409,
            "该 Codex 实例的独立执行适配器尚未接入，未改投本机实例",
        ));
    }
    let mut capabilities = json!({});
    let mut capability_error = String::new();
    if section == "general"
        && let Some(config) = &target.claude_config
    {
        // CLI 能力读取失败时仍保留已接入的回顾配置；未确认的新选项禁用，并把失败原因返回页面。
        let help = match claude_help(config).await {
            Ok(help) => help,
            Err(error) => {
                capability_error = error.message;
                String::new()
            }
        };
        capabilities = json!({
            "promptSuggestions":help.contains("--prompt-suggestions"),
            "autoCompactWindow":help.contains("--autocompact")
        });
    }
    let mut catalog = if section == "models" {
        Some(model_catalog(&state, instance, &authorization).await?)
    } else {
        None
    };
    let _serial = permissions::CONFIG_WRITE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    let (text, revision) = permissions::read_file(&target.path)?;
    let document = native_document(&target.backend, &text)?;
    let directory = &state.controller.state_dir;
    let mut updated = false;
    if write {
        let changes = payload["settings"]
            .as_object()
            .filter(|changes| !changes.is_empty())
            .ok_or_else(|| ApiError::new(400, "请选择需要保存的设置"))?;
        if changes.contains_key("ultraReasoningEnabled") {
            if section != "models" || target.backend != "codex" || changes.len() != 1 {
                return Err(ApiError::new(400, "Ultra 开关需要单独保存"));
            }
            let value = &changes["ultraReasoningEnabled"];
            if !value.is_boolean() {
                return Err(ApiError::new(400, "Ultra 开关必须为布尔值"));
            }
            save_preference(
                directory,
                instance,
                "ultraReasoningEnabled",
                value,
                payload["expectedPreferenceRevision"].as_str().unwrap_or(""),
                &authorization,
            )
            .await?;
        } else if changes.contains_key("autoCompactWindow") {
            if section != "general"
                || target.backend != "claude"
                || changes.len() != 1
                || capabilities["autoCompactWindow"] != true
            {
                return Err(ApiError::new(400, "当前 Claude CLI 不支持自动压缩窗口设置"));
            }
            validate_compact(&changes["autoCompactWindow"])?;
            save_preference(
                directory,
                instance,
                "autoCompactWindow",
                &changes["autoCompactWindow"],
                payload["expectedPreferenceRevision"].as_str().unwrap_or(""),
                &authorization,
            )
            .await?;
        } else if changes.contains_key("skill") {
            if section != "skills"
                || target.backend != "codex"
                || instance != "codex:local"
                || changes.len() != 1
            {
                return Err(ApiError::new(400, "该实例尚不支持原生技能启停"));
            }
            if payload["expectedRevision"] != revision {
                return Err(ApiError::new(409, "实例配置已变化，请重新读取后再保存"));
            }
            state
                .controller
                .harness
                .settings_skills(&target.home, Some(&changes["skill"]), &authorization)
                .await
                .map_err(super::authorization_error)?;
            state
                .controller
                .shared_settings
                .instance_changed(&state.controller, instance);
        } else {
            if changes.contains_key("promptSuggestionEnabled")
                && capabilities["promptSuggestions"] != true
            {
                return Err(ApiError::new(409, "当前 Claude CLI 尚未确认支持输入建议"));
            }
            if let Some(catalog) = &catalog {
                validate_models(
                    &target.backend,
                    &document,
                    &payload["settings"],
                    catalog,
                    ultra_enabled(directory, instance)?,
                )?;
            }
            save_native(&target, &text, &revision, &payload, &authorization).await?;
        }
        if changes.contains_key("ultraReasoningEnabled")
            || changes.contains_key("autoCompactWindow")
        {
            state
                .controller
                .shared_settings
                .instance_changed(&state.controller, instance);
        }
        updated = true;
    }
    if updated && section == "models" {
        catalog = Some(model_catalog(&state, instance, &authorization).await?);
    }
    let (text, revision) = permissions::read_file(&target.path)?;
    let document = native_document(&target.backend, &text)?;
    let (preference, preference_revision) = preferences(directory)?;
    let mut values = match section {
        "general" => general_view(&target.backend, &document)?,
        "models" => json!({
            "model":document["model"],
            "reasoningEffort":if target.backend == "codex" {
                document["model_reasoning_effort"].clone()
            } else {Value::Null},
            "ultraReasoningEnabled":ultra_enabled(directory, instance)?
        }),
        _ => json!({}),
    };
    if target.backend == "claude" && section == "general" {
        let window = &preference["instances"][instance]["autoCompactWindow"];
        validate_compact(window)?;
        values["autoCompactWindow"] = window.clone();
    }
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    let mut response = json!({
        "ok":true,"instanceId":instance,"backend":target.backend,
        "path":target.display,"revision":revision,"preferenceRevision":preference_revision,
        "settings":values,"capabilities":capabilities,"capabilityError":capability_error,
        "readOnly":state.settings.read_only||adapter_read_only,
        "readOnlyReason":if adapter_read_only {
            "该 Codex 实例的独立执行适配器尚未接入，原生配置仅供查看。"
        } else {""},
        "updated":updated
    });
    if let Some(catalog) = catalog {
        response["models"] = catalog["data"].clone();
        response["effectiveModel"] = catalog["defaultModel"].clone();
    }
    drop(_serial);
    if ["skills", "mcp", "hooks"].contains(&section) {
        let mut inventory = extension_view(section, &target.backend, &document)?;
        if target.backend == "codex" && ["skills", "hooks"].contains(&section) {
            if instance != "codex:local" {
                return Err(ApiError::new(
                    409,
                    "该 Codex 实例的独立执行适配器尚未接入，未改投本机实例",
                ));
            }
            let native = if section == "skills" {
                state
                    .controller
                    .harness
                    .settings_skills(&target.home, None, &authorization)
                    .await
            } else {
                state.controller.harness.settings_hooks(&target.home).await
            }
            .map_err(super::authorization_error)?;
            inventory["data"] = native["data"].clone();
            inventory["message"] = native["message"].clone();
        } else if target.backend == "claude" && section == "skills" {
            let native = state
                .controller
                .call_authorized(
                    "skills",
                    json!({"instanceId":instance,"forceReload":true}),
                    &authorization,
                )
                .await
                .map_err(super::authorization_error)?;
            let native = super::native_result(native)?;
            inventory["data"] = native["data"].clone();
            inventory["message"] = json!(if state.settings.read_only {
                "只读访问不会启动 Claude Skill 探测，请在该实例的原生 CLI 中查看。"
            } else {
                "由所选实例的 Claude CLI 返回；单项启停仍由原生 CLI 管理。"
            });
        } else if target.backend == "claude" && section == "mcp" {
            inventory = claude_mcp(
                &state,
                target.claude_config.as_ref().unwrap(),
                &authorization,
            )
            .await?;
        }
        response["inventory"] = inventory;
    }
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}
