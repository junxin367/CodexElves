use std::path::Path;

use anyhow::Context;
use serde_json::{Value, json};

use super::{Harness, error, runtime::UnsupportedMethod};

fn desktop_permission(state: &Value) -> Option<&str> {
    // 用户要求默认权限沿用 Codex App；只读取当前桌面账号所属的偏好，不能借用其他账号的记录。
    let atoms = match state["electron-renderer-content-owner"].as_str() {
        Some(owner) => state.get(format!("electron-owner-atoms:{owner}"))?,
        None => &state["electron-persisted-atom-state"],
    };
    let selection = &atoms["permission-selection-by-host-id:local"];
    match selection["kind"].as_str() {
        Some("agent-mode") => selection["agentMode"].as_str(),
        Some("custom" | "profile" | "server-default") => Some("configured"),
        _ => atoms["agent-mode-by-host-id"]["local"].as_str(),
    }
}

fn load_desktop_permission(home: &Path) -> anyhow::Result<Option<String>> {
    let path = home.join(".codex-global-state.json");
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(failure) => return Err(failure).context("无法读取 Codex App 默认权限"),
    };
    anyhow::ensure!(metadata.len() <= 16 * 1024 * 1024, "Codex App 偏好文件过大");
    let state: Value =
        serde_json::from_slice(&std::fs::read(path)?).context("Codex App 默认权限配置格式无效")?;
    Ok(desktop_permission(&state).map(str::to_owned))
}

pub(super) fn model_effort(model: &Value, preferred: Option<&str>) -> Option<String> {
    let efforts = model["supportedReasoningEfforts"].as_array()?;
    let supported = |value: &str| {
        efforts
            .iter()
            .any(|effort| effort["reasoningEffort"] == value)
    };
    preferred
        .filter(|value| supported(value))
        .or_else(|| {
            model["defaultReasoningEffort"]
                .as_str()
                .filter(|value| supported(value))
        })
        .or_else(|| efforts.first()?.get("reasoningEffort")?.as_str())
        .map(str::to_owned)
}

fn permission_allowed(mode: &str, requirements: &Value) -> bool {
    let (approval, reviewer, sandbox, profile) = match mode {
        "auto" => ("on-request", "user", "workspace-write", ":workspace"),
        "guardian-approvals" => ("on-request", "auto_review", "workspace-write", ":workspace"),
        "read-only" => ("on-request", "user", "read-only", ":read-only"),
        "full-access" => ("never", "user", "danger-full-access", ":danger-full-access"),
        // 自定义配置交给原生配置解析和执行引擎校验，不能擅自转换成更宽松的预设。
        "configured" => return true,
        _ => return false,
    };
    let allowed = |key: &str, value: &str| {
        requirements[key]
            .as_array()
            .is_none_or(|values| values.iter().any(|item| item == value))
    };
    allowed("allowedApprovalPolicies", approval)
        && allowed("allowedApprovalsReviewers", reviewer)
        && allowed("allowedSandboxModes", sandbox)
        && requirements["allowedPermissionProfiles"]
            .as_object()
            .is_none_or(|profiles| profiles.get(profile) == Some(&json!(true)))
}

fn configured_permission(config: &Value) -> &str {
    match config["default_permissions"].as_str() {
        Some(":read-only") => return "read-only",
        Some(":workspace") => return "auto",
        Some(":danger-full-access") => return "full-access",
        Some(_) => return "configured",
        None => {}
    }
    let workspace = &config["sandbox_workspace_write"];
    if config["sandbox_mode"] == "workspace-write"
        && (workspace["network_access"] == true
            || workspace["exclude_tmpdir_env_var"] == true
            || workspace["exclude_slash_tmp"] == true
            || workspace["writable_roots"]
                .as_array()
                .is_some_and(|roots| !roots.is_empty()))
    {
        return "configured";
    }
    match (
        config["approval_policy"].as_str(),
        config["sandbox_mode"].as_str(),
        config["approvals_reviewer"].as_str().unwrap_or("user"),
    ) {
        (Some("never"), Some("danger-full-access"), "user") => "full-access",
        (Some("on-request"), Some("workspace-write"), "auto_review") => "guardian-approvals",
        (Some("on-request"), Some("workspace-write"), "user") => "auto",
        (Some("on-request"), Some("read-only"), "user") => "read-only",
        (None, None, _) => "auto",
        _ => "configured",
    }
}

fn resolve_defaults(
    models: &[Value],
    config: &Value,
    desktop: Option<&str>,
    requirements: &Value,
) -> Value {
    let policy = &requirements["models"]["newThread"];
    let requested_model = policy["model"]
        .as_str()
        .or_else(|| config["model"].as_str());
    let model = requested_model
        .and_then(|name| models.iter().find(|model| model["model"] == name))
        .or_else(|| models.iter().find(|model| model["isDefault"] == true))
        .or_else(|| models.first());
    // 用户要求模型和深度按 App 一起解析；配置中的深度只属于匹配的模型，失效时采用该模型目录默认值。
    let effort = model.and_then(|model| {
        model_effort(
            model,
            policy["modelReasoningEffort"].as_str().or_else(|| {
                (model["model"].as_str() == config["model"].as_str() && policy["model"].is_null())
                    .then(|| config["model_reasoning_effort"].as_str())
                    .flatten()
            }),
        )
    });
    let configured = configured_permission(config);
    let mode = desktop
        .filter(|mode| permission_allowed(mode, requirements))
        .unwrap_or(configured);
    let mode = if permission_allowed(mode, requirements) {
        mode
    } else {
        "configured"
    };
    let modes: Vec<_> = [
        "auto",
        "guardian-approvals",
        "full-access",
        "configured",
        "read-only",
    ]
    .into_iter()
    .filter(|mode| permission_allowed(mode, requirements))
    .collect();
    json!({"ok":true,"model":model.map(|model| &model["model"]),"reasoningEffort":effort,
        "permissionMode":mode,"permissionModes":modes})
}

impl Harness {
    pub(super) async fn creation_defaults(
        &self,
        payload: &Value,
    ) -> anyhow::Result<(Value, Value)> {
        let cwd = if payload["projectId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        {
            self.workspace_context(payload).await?["cwd"].clone()
        } else {
            Value::Null
        };
        // 模型目录仍由实例共享；项目只影响生效配置，不能因切换项目重建模型目录。
        let (config, models, requirements) = tokio::try_join!(
            self.runtime.request(
                "config/read",
                json!({"cwd":cwd,"includeLayers":false}),
                None
            ),
            self.list_all("model/list", json!({"limit":100,"includeHidden":false})),
            async {
                match self
                    .runtime
                    .read("configRequirements/read", json!({}))
                    .await
                {
                    Ok(value) => Ok(value),
                    Err(failure) if failure.downcast_ref::<UnsupportedMethod>().is_some() => {
                        Ok(json!({}))
                    }
                    Err(failure) => Err(failure),
                }
            }
        )?;
        if !config["config"].is_object() {
            return Err(error(503, "无法确认 Codex 新会话的生效配置"));
        }
        #[cfg(test)]
        let home = self.project_codex_home.clone();
        #[cfg(not(test))]
        let home = crate::codex_home::codex_home_dir_for_settings(
            &crate::settings::SettingsStore::default().load()?,
        );
        let desktop = tokio::task::spawn_blocking(move || load_desktop_permission(&home)).await??;
        let mut effective = config["config"].clone();
        if let Some(model) = payload["model"].as_str().filter(|model| !model.is_empty())
            && effective["model"] != model
        {
            effective["model"] = json!(model);
            effective["model_reasoning_effort"] = Value::Null;
        }
        Ok((
            resolve_defaults(
                &models,
                &effective,
                desktop.as_deref(),
                &requirements["requirements"],
            ),
            config,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_permission_uses_only_the_current_account_and_preserves_custom_selection() {
        let mut state = json!({
            "electron-renderer-content-owner":"current",
            "electron-owner-atoms:current":{"permission-selection-by-host-id:local":
                {"kind":"agent-mode","agentMode":"full-access"}},
            "electron-owner-atoms:other":{"permission-selection-by-host-id:local":
                {"kind":"agent-mode","agentMode":"auto"}}
        });
        assert_eq!(desktop_permission(&state), Some("full-access"));
        state["electron-owner-atoms:current"]["permission-selection-by-host-id:local"] =
            json!({"kind":"custom"});
        assert_eq!(desktop_permission(&state), Some("configured"));
        state["electron-renderer-content-owner"] = json!("missing");
        assert_eq!(desktop_permission(&state), None);
    }

    #[test]
    fn defaults_keep_model_effort_permissions_and_requirements_consistent() {
        let models = vec![
            json!({"model":"first","isDefault":true,"defaultReasoningEffort":"medium",
                "supportedReasoningEfforts":[{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}),
            json!({"model":"second","defaultReasoningEffort":"low",
                "supportedReasoningEfforts":[{"reasoningEffort":"low"}]}),
        ];
        let config = json!({"model":"first","model_reasoning_effort":"xhigh",
            "approval_policy":"never","sandbox_mode":"danger-full-access"});
        let resolved = resolve_defaults(&models, &config, None, &Value::Null);
        assert_eq!(resolved["model"], "first");
        assert_eq!(resolved["reasoningEffort"], "medium");
        assert_eq!(resolved["permissionMode"], "full-access");
        let required = json!({"models":{"newThread":{"model":"second"}},
            "allowedSandboxModes":["workspace-write"]});
        let resolved = resolve_defaults(&models, &config, Some("full-access"), &required);
        assert_eq!(resolved["model"], "second");
        assert_eq!(resolved["reasoningEffort"], "low");
        assert_eq!(resolved["permissionMode"], "configured");
        assert!(
            !resolved["permissionModes"]
                .as_array()
                .unwrap()
                .contains(&json!("full-access"))
        );
        let custom = json!({"approval_policy":"on-request","sandbox_mode":"danger-full-access"});
        assert_eq!(
            resolve_defaults(&models, &custom, None, &Value::Null)["permissionMode"],
            "configured"
        );
        let custom_workspace = json!({"approval_policy":"on-request","sandbox_mode":"workspace-write",
            "sandbox_workspace_write":{"network_access":true}});
        assert_eq!(configured_permission(&custom_workspace), "configured");
    }
}
