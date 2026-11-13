//! 实例的原生全局权限与远程会话覆盖分开保存；会话菜单不得改写用户配置文件。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{ApiError, RemoteAuthorization, WebState};

const MAX_CONFIG_BYTES: usize = 2 * 1024 * 1024;
static CONFIG_WRITE: OnceLock<Mutex<()>> = OnceLock::new();
type ModeCache = BTreeMap<String, (Instant, Vec<String>)>;
static CLAUDE_MODE_CACHE: OnceLock<Mutex<ModeCache>> = OnceLock::new();

pub(super) const CODEX_MODES: &[&str] = &["configured", "read-only", "auto", "full-access"];
pub(super) const CLAUDE_MODES: &[&str] = &[
    "configured",
    "default",
    "manual",
    "acceptEdits",
    "plan",
    "bypassPermissions",
    "dontAsk",
    "auto",
];

pub(super) fn validate_mode<'a>(backend: &str, value: &'a Value) -> Result<&'a str, ApiError> {
    let modes = match backend {
        "codex" => CODEX_MODES,
        "claude" => CLAUDE_MODES,
        _ => return Err(ApiError::new(400, "该实例尚不支持权限设置")),
    };
    value
        .as_str()
        .filter(|mode| modes.contains(mode))
        .ok_or_else(|| ApiError::new(400, "会话权限模式无效"))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Selection {
    pub instance_id: String,
    pub mode: String,
    #[serde(default)]
    pub revision: u64,
}

pub(super) struct SessionPermissions {
    path: PathBuf,
    selections: Mutex<BTreeMap<String, Selection>>,
}

impl SessionPermissions {
    pub fn open(directory: &Path) -> anyhow::Result<Self> {
        let path = directory.join("remote-session-permissions.json");
        let selections = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            selections: Mutex::new(selections),
        })
    }

    pub async fn get(&self, thread: &str) -> Option<Selection> {
        self.selections.lock().await.get(thread).cloned()
    }

    pub async fn save(
        &self,
        thread: &str,
        instance: &str,
        mode: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<u64> {
        let mut selections = self.selections.lock().await;
        let mut next = selections.clone();
        let revision = next
            .get(thread)
            .map_or(0, |value| value.revision)
            .checked_add(1)
            .ok_or_else(|| ApiError::new(409, "会话权限版本已达上限"))?;
        next.insert(
            thread.to_owned(),
            Selection {
                instance_id: instance.to_owned(),
                mode: mode.to_owned(),
                revision,
            },
        );
        let bytes = serde_json::to_vec(&next)?;
        let permit = authorization.prepare_dispatch().await?;
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            crate::settings::atomic_write(&self.path, &bytes)
        })?;
        *selections = next;
        Ok(revision)
    }
}

struct ConfigFile {
    path: PathBuf,
    display: String,
    backend: String,
    claude_config: Option<super::claude::WslConfig>,
}

fn config_target(
    environments: &[super::preferences::Environment],
    instance_id: &str,
) -> Result<ConfigFile, ApiError> {
    let instance = super::instances::catalog(environments)
        .into_iter()
        .find(|instance| instance.id == instance_id)
        .ok_or_else(|| ApiError::new(404, "会话实例暂不可用，请重新检测"))?;
    let environment = environments
        .iter()
        .find(|environment| {
            environment.id == instance.environment_id && environment.error.is_none()
        })
        .ok_or_else(|| ApiError::new(404, "无法确认该实例的配置目录"))?;
    let (home, name) = match instance.backend.as_str() {
        "codex" => (&environment.codex_home, "config.toml"),
        "claude" => (&environment.claude_home, "settings.json"),
        _ => return Err(ApiError::new(400, "该实例尚不支持权限设置")),
    };
    let root = if environment.distribution.is_empty() {
        PathBuf::from(home)
    } else {
        super::preferences::wsl_path(&environment.distribution, home)?
    };
    if home.is_empty() || !root.is_absolute() {
        return Err(ApiError::new(403, "实例配置目录无效"));
    }
    Ok(ConfigFile {
        path: root.join(name),
        display: format!("{}/{name}", home.trim_end_matches(['/', '\\'])),
        backend: instance.backend.clone(),
        claude_config: (instance.backend == "claude").then(|| super::claude::WslConfig {
            distribution: environment.distribution.clone(),
            user: environment.user.clone(),
            executable: environment.claude_executable.clone(),
        }),
    })
}

fn modes_from_help(text: &str) -> Vec<String> {
    let Some(section) = text.split("--permission-mode ").nth(1) else {
        return Vec::new();
    };
    let section = section.split("\n  -").next().unwrap_or(section);
    CLAUDE_MODES
        .iter()
        .filter(|mode| {
            **mode != "configured"
                && (section.contains(&format!("\"{mode}\""))
                    || section.contains(&format!("'{mode}'")))
        })
        .map(|mode| (*mode).to_owned())
        .collect()
}

pub(super) async fn claude_modes(config: &super::claude::WslConfig) -> Vec<String> {
    let key = format!("{}:{}", config.instance_id(), config.executable);
    let cache = CLAUDE_MODE_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some((at, modes)) = cache.lock().await.get(&key)
        && at.elapsed() < Duration::from_secs(60)
    {
        return modes.clone();
    }
    // 只运行 --help，不初始化模型、MCP 或用户会话；按实际实例版本识别 manual / default 等名称。
    let mut command = if config.distribution.is_empty() {
        tokio::process::Command::new(&config.executable)
    } else {
        let mut command = tokio::process::Command::new("wsl.exe");
        command.args([
            "--distribution",
            &config.distribution,
            "--user",
            &config.user,
            "--exec",
            &config.executable,
        ]);
        command
    };
    command.arg("--help");
    let modes = super::preferences::output(command)
        .await
        .ok()
        .map(|bytes| modes_from_help(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    cache
        .lock()
        .await
        .insert(key, (Instant::now(), modes.clone()));
    modes
}

pub(super) async fn configured_claude_mode(
    directory: &Path,
    instance: &str,
) -> Result<String, ApiError> {
    let environments = super::preferences::detect(directory, false).await;
    let target = config_target(&environments, instance)?;
    if target.backend != "claude" {
        return Err(ApiError::new(400, "Claude 权限配置实例不匹配"));
    }
    let (text, _) = read_file(&target.path)?;
    let settings = global_view("claude", &text)?;
    Ok(settings["permissionMode"]
        .as_str()
        .filter(|mode| !mode.is_empty())
        .unwrap_or("default")
        .to_owned())
}

fn read_file(path: &Path) -> Result<(String, String), ApiError> {
    let (bytes, exists) = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > MAX_CONFIG_BYTES as u64
            {
                return Err(ApiError::new(403, "配置文件必须为不超过 2 MiB 的普通文件"));
            }
            (
                std::fs::read(path).map_err(|_| ApiError::new(403, "无法读取实例配置文件"))?,
                true,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), false),
        Err(_) => return Err(ApiError::new(403, "无法读取实例配置文件")),
    };
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ApiError::new(413, "实例配置文件超过 2 MiB"));
    }
    let revision = super::hash(&format!(
        "{}:{}",
        exists,
        super::hash(&String::from_utf8_lossy(&bytes))
    ));
    let text = String::from_utf8(bytes)
        .map_err(|_| ApiError::new(422, "实例配置文件需要使用 UTF-8 编码"))?;
    Ok((text, revision))
}

fn save_file(path: &Path, text: &str, expected: &str) -> anyhow::Result<()> {
    use std::io::Write;
    if text.len() > MAX_CONFIG_BYTES {
        return Err(ApiError::new(413, "实例配置文件超过 2 MiB").into());
    }
    if read_file(path)?.1 != expected {
        return Err(ApiError::new(409, "实例配置已变化，请重新读取").into());
    }
    let permissions = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    if permissions
        .as_ref()
        .is_some_and(std::fs::Permissions::readonly)
    {
        return Err(ApiError::new(403, "实例配置文件为只读，未修改原文件").into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::new(400, "实例配置目录无效"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".permissions-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Some(permissions) = permissions {
            std::fs::set_permissions(&temporary, permissions)?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if read_file(path)?.1 != expected {
            return Err(ApiError::new(409, "实例配置已变化，请重新读取").into());
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn global_view(backend: &str, text: &str) -> Result<Value, ApiError> {
    if backend == "codex" {
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| ApiError::new(422, "Codex 配置格式无效，未覆盖原文件"))?;
        let approval = document
            .get("approval_policy")
            .and_then(toml_edit::Item::as_str);
        let sandbox = document
            .get("sandbox_mode")
            .and_then(toml_edit::Item::as_str);
        let custom_approval = document.get("approval_policy").is_some() && approval.is_none();
        Ok(json!({
            "approvalPolicy":approval.unwrap_or(""),
            "sandboxMode":sandbox.unwrap_or(""),
            "permissionProfile":document.get("default_permissions").and_then(toml_edit::Item::as_str),
            "customApproval":custom_approval
        }))
    } else {
        let document: Value = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(text)
                .map_err(|_| ApiError::new(422, "Claude 配置格式无效，未覆盖原文件"))?
        };
        if !document.is_object() {
            return Err(ApiError::new(422, "Claude 配置必须为 JSON 对象"));
        }
        Ok(json!({"permissionMode":document["permissions"]["defaultMode"].as_str().unwrap_or("")}))
    }
}

fn updated_config(backend: &str, text: &str, settings: &Value) -> Result<String, ApiError> {
    let object = settings
        .as_object()
        .ok_or_else(|| ApiError::new(400, "权限配置无效"))?;
    if backend == "codex" {
        if object
            .keys()
            .any(|key| !["approvalPolicy", "sandboxMode"].contains(&key.as_str()))
        {
            return Err(ApiError::new(400, "只允许修改批准策略和沙盒设置"));
        }
        let mut document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| ApiError::new(422, "Codex 配置格式无效，未覆盖原文件"))?;
        for (field, key, allowed) in [
            (
                "approvalPolicy",
                "approval_policy",
                &["", "on-request", "never"][..],
            ),
            (
                "sandboxMode",
                "sandbox_mode",
                &["", "read-only", "workspace-write", "danger-full-access"][..],
            ),
        ] {
            let Some(value) = object.get(field) else {
                continue;
            };
            let value = value
                .as_str()
                .filter(|value| allowed.contains(value))
                .ok_or_else(|| ApiError::new(400, "Codex 权限配置值无效"))?;
            if field == "sandboxMode" && document.get("default_permissions").is_some() {
                return Err(ApiError::new(
                    409,
                    "该实例正在使用命名权限配置，请在原生配置中调整沙盒",
                ));
            }
            if value.is_empty() {
                document.remove(key);
            } else {
                let mut replacement = toml_edit::Value::from(value);
                if let Some(original) = document.get(key).and_then(toml_edit::Item::as_value) {
                    *replacement.decor_mut() = original.decor().clone();
                }
                document[key] = toml_edit::Item::Value(replacement);
            }
        }
        // 用户要求与原生设置页一样修改实例全局配置；TOML 注释、模型、密钥及其他表均须原样保留。
        Ok(document.to_string())
    } else {
        if object.len() != 1 || !object.contains_key("permissionMode") {
            return Err(ApiError::new(400, "只允许修改 Claude 默认权限模式"));
        }
        let mode = object["permissionMode"]
            .as_str()
            .filter(|mode| mode.is_empty() || CLAUDE_MODES.contains(mode) && *mode != "configured")
            .ok_or_else(|| ApiError::new(400, "Claude 权限模式无效"))?;
        let mut document: Value = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(text)
                .map_err(|_| ApiError::new(422, "Claude 配置格式无效，未覆盖原文件"))?
        };
        let root = document
            .as_object_mut()
            .ok_or_else(|| ApiError::new(422, "Claude 配置必须为 JSON 对象"))?;
        let permissions = root
            .entry("permissions")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| ApiError::new(422, "Claude permissions 配置格式无效"))?;
        if mode.is_empty() {
            permissions.remove("defaultMode");
        } else {
            permissions.insert("defaultMode".into(), json!(mode));
        }
        // 只替换默认模式，必须保留原生 allow / ask / deny、Hooks、MCP 和其他设置。
        let mut output = serde_json::to_string_pretty(&document).unwrap();
        if text.contains("\r\n") {
            output = output.replace('\n', "\r\n");
        }
        output.push_str(if text.contains("\r\n") { "\r\n" } else { "\n" });
        Ok(output)
    }
}

pub(super) async fn settings(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let instance = payload["instanceId"]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= 160)
        .ok_or_else(|| ApiError::new(400, "请选择会话实例"))?;
    let write = match payload["operation"].as_str().unwrap_or("read") {
        "read" => false,
        "save" => true,
        _ => return Err(ApiError::new(400, "权限配置操作无效")),
    };
    if write && state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    let authorization = state
        .controller
        .authorization(device, write.then_some("setPermissions"));
    let environments = super::preferences::detect(&state.controller.state_dir, false).await;
    let target = config_target(&environments, instance)?;
    let supported_modes = match &target.claude_config {
        Some(config) => Some(claude_modes(config).await),
        None => None,
    };
    let _serial = CONFIG_WRITE.get_or_init(|| Mutex::new(())).lock().await;
    let (mut text, revision) = read_file(&target.path)?;
    if write {
        if payload["expectedRevision"] != revision {
            return Err(ApiError::new(409, "实例配置已变化，请重新读取后再保存"));
        }
        if target.backend == "claude"
            && let Some(mode) = payload["settings"]["permissionMode"]
                .as_str()
                .filter(|mode| !mode.is_empty())
            && supported_modes
                .as_ref()
                .is_none_or(|modes| !modes.iter().any(|supported| supported == mode))
        {
            return Err(ApiError::new(
                409,
                "该 Claude CLI 尚未确认支持此权限模式，请重新读取",
            ));
        }
        text = updated_config(&target.backend, &text, &payload["settings"])?;
        let permit = authorization
            .prepare_dispatch()
            .await
            .map_err(super::authorization_error)?;
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            save_file(&target.path, &text, &revision)
        })
        .map_err(super::authorization_error)?;
    }
    let (text, revision) = read_file(&target.path)?;
    let settings = global_view(&target.backend, &text)?;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "ok":true, "instanceId":instance, "backend":target.backend,
            "path":target.display, "revision":revision, "settings":settings,
            "supportedModes":supported_modes,
            "readOnly":state.settings.read_only
        })),
    )
        .into_response())
}

/// 与 native.js 的会话预设保持一致；完全访问必须同时关闭沙盒和命令审批。
pub(super) fn codex_preset(mode: &str, cwd: &str) -> Result<Value, ApiError> {
    let (approval, sandbox) = match mode {
        "read-only" => ("on-request", json!({"type":"readOnly"})),
        "auto" => (
            "on-request",
            json!({"type":"workspaceWrite","writableRoots":[cwd],
            "networkAccess":false,"excludeTmpdirEnvVar":false,"excludeSlashTmp":false}),
        ),
        "full-access" => ("never", json!({"type":"dangerFullAccess"})),
        _ => return Err(ApiError::new(400, "Codex 会话权限模式无效")),
    };
    Ok(json!({"approvalPolicy":approval,"approvalsReviewer":"user","sandboxPolicy":sandbox}))
}

pub(super) fn codex_config_preset(config: &Value) -> Result<Value, ApiError> {
    let config = config.get("config").unwrap_or(config);
    let approval = config
        .get("approval_policy")
        .filter(|value| value.is_string() || value.is_object())
        .ok_or_else(|| ApiError::new(503, "无法确认实例的批准策略"))?;
    let sandbox = match config["sandbox_mode"].as_str() {
        Some("read-only") => json!({"type":"readOnly"}),
        Some("danger-full-access") => json!({"type":"dangerFullAccess"}),
        Some("workspace-write") => json!({"type":"workspaceWrite",
            "writableRoots":config["sandbox_workspace_write"]["writable_roots"].as_array().cloned().unwrap_or_default(),
            "networkAccess":config["sandbox_workspace_write"]["network_access"].as_bool().unwrap_or(false),
            "excludeTmpdirEnvVar":config["sandbox_workspace_write"]["exclude_tmpdir_env_var"].as_bool().unwrap_or(false),
            "excludeSlashTmp":config["sandbox_workspace_write"]["exclude_slash_tmp"].as_bool().unwrap_or(false)}),
        _ => return Err(ApiError::new(409, "该实例使用当前远程端尚不支持的沙盒配置")),
    };
    Ok(json!({"approvalPolicy":approval,
        "approvalsReviewer":config["approvals_reviewer"].as_str().unwrap_or("user"),"sandboxPolicy":sandbox}))
}

pub(super) fn apply_codex(
    payload: &Value,
    params: &mut Value,
    cwd: &str,
    creating: bool,
) -> anyhow::Result<()> {
    let Some(mode) = payload.get("permissionMode") else {
        return Ok(());
    };
    let mode = validate_mode("codex", mode)?;
    let mut preset = if mode == "configured" {
        payload
            .get("permissionConfig")
            .filter(|value| {
                value["approvalPolicy"].is_string() || value["approvalPolicy"].is_object()
            })
            .cloned()
            .ok_or_else(|| ApiError::new(503, "实例权限尚未读取，请重新选择权限"))?
    } else {
        codex_preset(mode, cwd)?
    };
    if preset["sandboxPolicy"]["type"] == "workspaceWrite" {
        let roots = preset["sandboxPolicy"]["writableRoots"]
            .as_array_mut()
            .ok_or_else(|| ApiError::new(503, "实例可写目录配置无效"))?;
        if !cwd.is_empty() && !roots.iter().any(|root| root == cwd) {
            roots.push(json!(cwd));
        }
    }
    params["approvalPolicy"] = preset["approvalPolicy"].clone();
    params["approvalsReviewer"] = preset["approvalsReviewer"].clone();
    if creating {
        let mode = match preset["sandboxPolicy"]["type"].as_str() {
            Some("readOnly") => "read-only",
            Some("workspaceWrite") => "workspace-write",
            Some("dangerFullAccess") => "danger-full-access",
            _ => return Err(ApiError::new(409, "不支持此会话沙盒").into()),
        };
        params["sandbox"] = json!(mode);
        if mode == "workspace-write" {
            let sandbox = &preset["sandboxPolicy"];
            params["config"] = json!({
                "sandbox_workspace_write.writable_roots":sandbox["writableRoots"],
                "sandbox_workspace_write.network_access":sandbox["networkAccess"],
                "sandbox_workspace_write.exclude_tmpdir_env_var":sandbox["excludeTmpdirEnvVar"],
                "sandbox_workspace_write.exclude_slash_tmp":sandbox["excludeSlashTmp"]
            });
        }
    } else {
        params["sandboxPolicy"] = preset["sandboxPolicy"].clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_codex_updates_preserve_comments_and_unrelated_config() {
        let original = "# 用户配置\napproval_policy = \"on-request\" # 审批说明\n\
sandbox_mode = \"workspace-write\" # 沙盒说明\nmodel = \"fixture\"\n\n\
[mcp_servers.fixture]\ncommand = \"fixture-command\"\n";
        let updated = updated_config(
            "codex",
            original,
            &json!({
                "approvalPolicy":"never","sandboxMode":"danger-full-access"
            }),
        )
        .unwrap();
        assert_eq!(
            updated,
            original
                .replace("\"on-request\"", "\"never\"")
                .replace("\"workspace-write\"", "\"danger-full-access\"")
        );
        let view = global_view("codex", &updated).unwrap();
        assert_eq!(view["approvalPolicy"], "never");
        assert_eq!(view["sandboxMode"], "danger-full-access");
        let cleared = updated_config("codex", &updated, &json!({"approvalPolicy":""})).unwrap();
        assert!(!cleared.contains("approval_policy"));
        assert!(cleared.contains("sandbox_mode = \"danger-full-access\""));
        assert!(cleared.contains("[mcp_servers.fixture]"));
    }

    #[test]
    fn named_codex_profiles_and_invalid_fields_cannot_be_overwritten() {
        let original = "default_permissions = \"fixture\"\napproval_policy = { reject = { sandbox_approval = true } }\n";
        assert_eq!(
            global_view("codex", original).unwrap()["customApproval"],
            true
        );
        assert!(
            updated_config(
                "codex",
                original,
                &json!({"sandboxMode":"danger-full-access"})
            )
            .is_err()
        );
        assert!(updated_config("codex", original, &json!({"model":"different"})).is_err());
        assert!(updated_config("codex", original, &json!({"approvalPolicy":"invalid"})).is_err());
        assert!(
            updated_config("codex", "invalid = [", &json!({"approvalPolicy":"never"})).is_err()
        );
        let updated =
            updated_config("codex", original, &json!({"approvalPolicy":"on-request"})).unwrap();
        assert!(updated.contains("default_permissions = \"fixture\""));
    }

    #[test]
    fn global_claude_updates_only_default_mode_and_preserve_other_rules() {
        let original = json!({
            "permissions":{"defaultMode":"default","allow":["Read"],"ask":["Bash"],"deny":["Write"]},
            "hooks":{"Stop":[{"command":"fixture"}]},"enabledMcpjsonServers":["fixture"]
        });
        let text = serde_json::to_string_pretty(&original)
            .unwrap()
            .replace('\n', "\r\n");
        let updated =
            updated_config("claude", &text, &json!({"permissionMode":"acceptEdits"})).unwrap();
        assert!(updated.ends_with("\r\n"));
        let mut expected = original.clone();
        expected["permissions"]["defaultMode"] = json!("acceptEdits");
        assert_eq!(serde_json::from_str::<Value>(&updated).unwrap(), expected);
        let cleared = updated_config("claude", &updated, &json!({"permissionMode":""})).unwrap();
        expected["permissions"]
            .as_object_mut()
            .unwrap()
            .remove("defaultMode");
        assert_eq!(serde_json::from_str::<Value>(&cleared).unwrap(), expected);
        assert!(updated_config("claude", "[]", &json!({"permissionMode":"default"})).is_err());
        assert!(
            updated_config(
                "claude",
                "{\"permissions\":[]}",
                &json!({"permissionMode":"default"})
            )
            .is_err()
        );
        assert!(updated_config("claude", "{}", &json!({"permissionMode":"configured"})).is_err());
        assert!(
            updated_config(
                "claude",
                "{}",
                &json!({"permissionMode":"default","allow":[]})
            )
            .is_err()
        );
    }

    #[test]
    fn global_file_saves_reject_stale_revisions_without_losing_external_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let missing = read_file(&path).unwrap().1;
        save_file(&path, "approval_policy = \"on-request\"\n", &missing).unwrap();
        let first = read_file(&path).unwrap().1;
        assert_ne!(first, missing);
        std::fs::write(&path, "# 外部更新\napproval_policy = \"never\"\n").unwrap();
        assert!(save_file(&path, "approval_policy = \"on-request\"\n", &first).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# 外部更新\napproval_policy = \"never\"\n"
        );
        let latest = read_file(&path).unwrap().1;
        save_file(&path, "approval_policy = \"on-request\"\n", &latest).unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn codex_full_access_and_workspace_presets_keep_approval_semantics() {
        let preset = codex_preset("full-access", "/fixture").unwrap();
        assert_eq!(preset["approvalPolicy"], "never");
        assert_eq!(preset["sandboxPolicy"]["type"], "dangerFullAccess");
        let mut params = json!({"threadId":"fixture","model":"unchanged"});
        apply_codex(
            &json!({"permissionMode":"auto"}),
            &mut params,
            "/fixture",
            false,
        )
        .unwrap();
        assert_eq!(params["approvalPolicy"], "on-request");
        assert_eq!(params["sandboxPolicy"]["type"], "workspaceWrite");
        assert_eq!(
            params["sandboxPolicy"]["writableRoots"],
            json!(["/fixture"])
        );
        assert_eq!(params["model"], "unchanged");
        apply_codex(
            &json!({"permissionMode":"full-access"}),
            &mut params,
            "/fixture",
            true,
        )
        .unwrap();
        assert_eq!(params["sandbox"], "danger-full-access");
        assert_eq!(params["approvalPolicy"], "never");
        assert!(
            apply_codex(
                &json!({"permissionMode":"configured"}),
                &mut params,
                "/fixture",
                false
            )
            .is_err()
        );
        assert!(validate_mode("codex", &json!("bypassPermissions")).is_err());
        assert!(validate_mode("claude", &json!("full-access")).is_err());
    }

    #[test]
    fn configured_codex_permissions_preserve_native_restrictions() {
        let preset = codex_config_preset(&json!({"config":{
            "approval_policy":"on-request","sandbox_mode":"danger-full-access"
        }}))
        .unwrap();
        assert_eq!(
            preset["approvalPolicy"], "on-request",
            "完整访问范围不能自行变成免审批"
        );
        let preset = codex_config_preset(&json!({"config":{
            "approval_policy":"never","sandbox_mode":"workspace-write",
            "sandbox_workspace_write":{"writable_roots":["/extra"],"network_access":true}
        }}))
        .unwrap();
        let mut params = json!({});
        apply_codex(
            &json!({"permissionMode":"configured","permissionConfig":preset}),
            &mut params,
            "/fixture",
            true,
        )
        .unwrap();
        assert_eq!(params["sandbox"], "workspace-write");
        assert_eq!(
            params["config"]["sandbox_workspace_write.writable_roots"],
            json!(["/extra", "/fixture"])
        );
        assert_eq!(
            params["config"]["sandbox_workspace_write.network_access"],
            true
        );
        assert!(codex_config_preset(&json!({"config":{"approval_policy":"never"}})).is_err());
    }

    #[test]
    fn claude_help_modes_come_from_the_actual_permission_option() {
        let modes = modes_from_help(
            "  --permission-mode <mode>  choices: \"acceptEdits\", \"manual\", \"plan\", \"auto\"\n  --other \"bypassPermissions\"",
        );
        assert_eq!(modes, ["manual", "acceptEdits", "plan", "auto"]);
        assert!(modes_from_help("  --other \"bypassPermissions\"").is_empty());
    }
}
