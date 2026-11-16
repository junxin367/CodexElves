use serde_json::{Value, json};

use super::{Harness, error, runtime::UnsupportedMethod};

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

fn resolve_defaults(models: &[Value], config: &Value, requirements: &Value) -> Value {
    let policy = &requirements["models"]["newThread"];
    let requested_model = policy["model"]
        .as_str()
        .or_else(|| config["model"].as_str());
    let model = requested_model
        .and_then(|name| models.iter().find(|model| model["model"] == name))
        .or_else(|| models.iter().find(|model| model["isDefault"] == true))
        .or_else(|| models.first());
    // 模型和深度按独立引擎配置一起解析；配置中的深度只属于匹配的模型，失效时采用该模型目录默认值。
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
    // 新建权限只遵循独立引擎的生效配置，不借用 Codex App 当前账号的内部偏好。
    let mode = configured;
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
        let (config, mut models, requirements) = tokio::try_join!(
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
        let mut effective = config["config"].clone();
        let ultra = self
            .controller
            .get()
            .and_then(std::sync::Weak::upgrade)
            .map(|controller| {
                super::super::instance_settings::ultra_enabled(
                    &controller.state_dir,
                    payload["instanceId"].as_str().unwrap_or("codex:local"),
                )
            })
            .transpose()?
            .unwrap_or(false);
        // 用户要求关闭 Ultra 时新会话也不隐式采用该等级；仅调整远程新建默认值，不改写原生配置或已有会话。
        super::super::instance_settings::filter_model_rows(&mut models, ultra);
        if !ultra && effective["model_reasoning_effort"] == "ultra" {
            effective["model_reasoning_effort"] = Value::Null;
        }
        if let Some(model) = payload["model"].as_str().filter(|model| !model.is_empty())
            && effective["model"] != model
        {
            effective["model"] = json!(model);
            effective["model_reasoning_effort"] = Value::Null;
        }
        Ok((
            resolve_defaults(&models, &effective, &requirements["requirements"]),
            config,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let resolved = resolve_defaults(&models, &config, &Value::Null);
        assert_eq!(resolved["model"], "first");
        assert_eq!(resolved["reasoningEffort"], "medium");
        assert_eq!(resolved["permissionMode"], "full-access");
        let required = json!({"models":{"newThread":{"model":"second"}},
            "allowedSandboxModes":["workspace-write"]});
        let resolved = resolve_defaults(&models, &config, &required);
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
            resolve_defaults(&models, &custom, &Value::Null)["permissionMode"],
            "configured"
        );
        let custom_workspace = json!({"approval_policy":"on-request","sandbox_mode":"workspace-write",
            "sandbox_workspace_write":{"network_access":true}});
        assert_eq!(configured_permission(&custom_workspace), "configured");
    }
}
