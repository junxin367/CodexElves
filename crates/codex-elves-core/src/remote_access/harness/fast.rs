use serde_json::{Map, Value, json};

use super::{Harness, error, required};
use crate::remote_access::RemoteAuthorization;

fn model<'a>(catalog: &'a Value, name: &str) -> Option<&'a Value> {
    let models = catalog["data"].as_array()?;
    models
        .iter()
        .find(|row| row["model"] == name || row["id"] == name)
        .or_else(|| models.iter().find(|row| row["requestModel"] == name))
}

fn alias_target<'a>(aliases: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    aliases
        .iter()
        .find(|(alias, _)| alias.trim().eq_ignore_ascii_case(name.trim()))
        .and_then(|(_, target)| target.as_str())
}

fn request_model(catalog: &Value, name: &str) -> String {
    catalog["fastModelAliases"]
        .as_object()
        .and_then(|aliases| alias_target(aliases, name))
        .or_else(|| {
            model(catalog, name).and_then(|row| {
                row["requestModel"]
                    .as_str()
                    .or_else(|| row["model"].as_str())
            })
        })
        .unwrap_or(name)
        .to_owned()
}

fn fast_tier<'a>(catalog: &'a Value, name: &str) -> Option<&'a str> {
    if !is_gpt_model(&request_model(catalog, name)) {
        return None;
    }
    // API 目录没有当前 GPT 行或档位声明时，也按用户要求使用 priority。
    model(catalog, name)
        .and_then(|row| row["serviceTiers"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|tier| tier["id"].as_str())
        .find(|id| matches!(*id, "priority" | "fast"))
        .or(Some("priority"))
}

fn is_gpt_model(name: &str) -> bool {
    let name = name
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    // 与 native-fast-unlock.js 的 /^gpt-\d/ 一致，只核对实际请求模型。
    name.strip_prefix("gpt-")
        .and_then(|suffix| suffix.as_bytes().first())
        .is_some_and(u8::is_ascii_digit)
}

pub(super) fn service_model(raw: &Value, aliases: &Map<String, Value>) -> Option<Value> {
    let mut normalized = crate::codex_service::normalize_model(raw)?;
    let name = normalized["model"].as_str()?;
    let actual = alias_target(aliases, name)
        .or_else(|| normalized["requestModel"].as_str())
        .unwrap_or(name)
        .to_owned();
    let supported = is_gpt_model(&actual);
    // 用户要求服务模式对 GPT 直接开放 Fast：API 目录缺少 tiers 或旧支持标记为 false 均不拦截。
    // 复用注入的别名解析，GPT 名称指向非 GPT 时仍关闭；账号、管理员和配置限制由实例能力单独核对。
    normalized["requestModel"] = json!(actual);
    normalized["fastModeSupported"] = json!(supported);
    if supported
        && !normalized["serviceTiers"]
            .as_array()?
            .iter()
            .any(|tier| matches!(tier["id"].as_str(), Some("priority" | "fast")))
    {
        normalized["serviceTiers"]
            .as_array_mut()?
            .push(json!({"id":"priority","name":"Fast"}));
    }
    Some(normalized)
}

fn configured_fast(config: &Value) -> Option<bool> {
    if let Some(value) = config["config"]["features"].get("fast_mode") {
        return value.as_bool();
    }
    // 新版公开 Config 不包含 features；用原始层及 origins 核对，不能把字段缺失当成显式允许。
    let layers = config["layers"].as_array()?;
    let active = |layer: &&Value| layer.get("disabledReason").is_none_or(Value::is_null);
    if let Some(origin) = config["origins"].get("features.fast_mode") {
        return layers
            .iter()
            .filter(active)
            .find(|layer| layer["name"] == origin["name"] && layer["version"] == origin["version"])
            .and_then(|layer| layer["config"]["features"]["fast_mode"].as_bool());
    }
    let mut selected = None;
    for layer in layers.iter().filter(active) {
        if let Some(value) = layer["config"]["features"].get("fast_mode") {
            let value = value.as_bool()?;
            if selected.is_some_and(|selected| selected != value) {
                // 无来源映射的冲突不能猜测层级顺序，保持能力未知并要求刷新。
                return None;
            }
            selected = Some(value);
        }
    }
    Some(selected.unwrap_or(true))
}

impl Harness {
    pub(super) async fn fast_availability(&self, config: &Value, fresh: bool) -> Value {
        let read = |method: &'static str, params: Value| async move {
            if fresh {
                self.runtime.request(method, params, None).await
            } else {
                self.runtime.read(method, params).await
            }
        };
        // 服务模式直接核对引擎的账号、配置和管理员要求，不读取或调用桌面注入状态。
        let environment = tokio::try_join!(
            read("account/read", json!({"refreshToken":false})),
            read("configRequirements/read", json!({}))
        );
        let (account, requirements) = match environment {
            Ok(environment) => environment,
            Err(failure) => {
                let unsupported = failure
                    .downcast_ref::<super::runtime::UnsupportedMethod>()
                    .is_some();
                return json!({"available":if unsupported { json!(false) } else { Value::Null },
                    "reason":"当前引擎暂不能确认 Fast 的账号或管理员限制，请刷新后重试"});
            }
        };
        let known_account = matches!(
            account["account"]["type"].as_str(),
            Some("apiKey" | "apikey" | "chatgpt" | "chatgptAuthTokens")
        );
        let requirements_known = matches!(
            requirements.get("requirements"),
            Some(Value::Object(_) | Value::Null)
        );
        let available = known_account
            && requirements_known
            && config.get("config").is_some_and(Value::is_object)
            && requirements["requirements"]["featureRequirements"]["fast_mode"] != false
            && configured_fast(config) != Some(false)
            && !config["config"]["model_provider"]
                .as_str()
                .is_some_and(|provider| provider.to_ascii_lowercase().contains("copilot"));
        let available = if available {
            configured_fast(config)
                .map(Value::Bool)
                .unwrap_or(Value::Null)
        } else {
            json!(false)
        };
        json!({"available":available,
            "reason":if available == true { "" } else if available.is_null() {
                "暂不能确认当前 Fast 配置，请刷新后重试"
            } else { "当前账号或配置未开放 Fast" }})
    }

    pub(super) async fn fast_preference(&self, id: &str) -> bool {
        if let Some(side) = self.projection.lock().await.sides.threads.get(id) {
            return side.metadata["fastMode"] == true;
        }
        self.store
            .lock()
            .await
            .data
            .settings
            .get(id)
            .is_some_and(|settings| settings["fastMode"] == true)
    }

    pub(super) async fn fast_state(&self, payload: &Value) -> anyhow::Result<Value> {
        let name = required(payload, "model")?;
        let catalog = self.models(&json!({})).await?;
        let supported = catalog["fastModeAvailability"]["available"] == true
            && fast_tier(&catalog, name).is_some();
        let id = payload["threadId"].as_str();
        Ok(
            json!({"ok":true,"threadId":id,"model":name,"supported":supported,
            "enabled":supported && if let Some(id) = id { self.fast_preference(id).await } else { false },
            "reason":if fast_tier(&catalog, name).is_none() { json!("当前请求模型不是 GPT，不能开启 Fast") }
                else { catalog["fastModeAvailability"]["reason"].clone() }}),
        )
    }

    pub(super) async fn validate_fast_model(&self, name: &str) -> anyhow::Result<Value> {
        let catalog = self.models(&json!({"forceReload":true})).await?;
        if catalog["fastModeAvailability"]["available"] != true {
            return Err(error(
                409,
                "当前账号或配置未开放 Fast，请关闭 Fast 或刷新模型目录后重试",
            ));
        }
        if fast_tier(&catalog, name).is_none() {
            return Err(error(409, "当前请求模型不是 GPT，不能开启 Fast"));
        }
        Ok(catalog)
    }

    pub(super) async fn set_fast(
        &self,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let name = required(payload, "model")?;
        let enabled = payload["enabled"]
            .as_bool()
            .ok_or_else(|| error(400, "Fast 开关值无效"))?;
        let detail = self.thread(payload).await?;
        let thread = &detail["thread"];
        if thread["readOnly"] == true || thread["viewKind"] == "subagent" {
            return Err(error(403, "此会话仅支持只读查看"));
        }
        let actual = required(thread, "model")?;
        let catalog = if enabled {
            self.validate_fast_model(name).await?
        } else {
            self.models(&json!({})).await?
        };
        let same_model = actual == name
            || request_model(&catalog, actual) == request_model(&catalog, name)
            || model(&catalog, actual)
                .zip(model(&catalog, name))
                .is_some_and(|(left, right)| {
                    left["model"] == right["model"] && left["id"] == right["id"]
                });
        if !same_model {
            return Err(error(409, "当前会话模型已变化，请刷新后重新设置 Fast"));
        }
        // 用户要求执行中可切换，但只影响之后新发起的轮次（含尚未派发的队列）。
        // 此处仅保存会话偏好，禁止调用 resume、turn/start、steer 或修改当前轮次的服务档位。
        let permit = authorization.prepare_dispatch().await?;
        let mut projection = self.projection.lock().await;
        if let Some(side) = projection.sides.threads.get_mut(id) {
            if side.closed || side.expired || side.closing {
                return Err(error(410, "侧边会话已过期，Fast 设置未保存"));
            }
            permit.commit(|| {
                side.metadata["fastMode"] = json!(enabled);
                Ok(())
            })?;
        } else {
            drop(projection);
            let mut store = self.store.lock().await;
            permit.commit(|| {
                store.commit(|data| {
                    data.settings
                        .entry(id.to_owned())
                        .or_insert_with(|| json!({}))["fastMode"] = json!(enabled);
                    Ok(())
                })
            })?;
            drop(store);
            projection = self.projection.lock().await;
        }
        if let Some(thread) = projection.threads.get_mut(id) {
            thread["fastMode"] = json!(enabled);
        }
        projection.push(json!({"method":"thread/settings/updated","params":{
            "threadId":id,"fastMode":enabled}}));
        Ok(
            json!({"ok":true,"threadId":id,"model":name,"supported":true,
            "enabled":enabled,"effectiveFrom":"nextTurn","nativeBridge":"harness"}),
        )
    }

    pub(super) async fn apply_fast_to_turn(
        &self,
        id: &str,
        params: &mut Value,
    ) -> anyhow::Result<()> {
        if !self.fast_preference(id).await {
            let selected = if params["serviceTier"].is_string() {
                params["serviceTier"].clone()
            } else if let Some(side) = self.projection.lock().await.sides.threads.get(id) {
                side.metadata["serviceTier"].clone()
            } else {
                self.store
                    .lock()
                    .await
                    .data
                    .settings
                    .get(id)
                    .map(|settings| settings["serviceTier"].clone())
                    .unwrap_or(Value::Null)
            };
            let selected = if selected.is_null() {
                self.runtime
                    .read("config/read", json!({"cwd":null,"includeLayers":false}))
                    .await
                    .ok()
                    .map(|config| config["config"]["service_tier"].clone())
                    .unwrap_or(Value::Null)
            } else {
                selected
            };
            // 关闭 Fast 必须显式恢复 Standard，避免继承上一轮 Fast；其它显式档位仍保留。
            if selected.is_null()
                || matches!(selected.as_str(), Some("priority" | "fast" | "default"))
            {
                params["serviceTier"] = json!("default");
            } else if !params["serviceTier"].is_string() && selected.is_string() {
                params["serviceTier"] = selected;
            }
            return Ok(());
        }
        let saved = self.store.lock().await.data.settings.get(id).cloned();
        let name = if let Some(name) = params["model"].as_str() {
            name.to_owned()
        } else if self.locally_loaded(id).await?
            && let Some(name) = saved
                .as_ref()
                .and_then(|settings| settings["model"].as_str())
        {
            name.to_owned()
        } else {
            let detail = self.thread(&json!({"threadId":id})).await?;
            required(&detail["thread"], "model")?.to_owned()
        };
        let catalog = self.models(&json!({"forceReload":true})).await?;
        // 当前请求模型优先于保存开关时的模型；切换到非 GPT/未知模型时不得继承 Fast。
        let Some(tier) = fast_tier(&catalog, &name) else {
            if params["serviceTier"].is_null()
                || matches!(params["serviceTier"].as_str(), Some("priority" | "fast"))
            {
                params["serviceTier"] = json!("default");
            }
            return Ok(());
        };
        if catalog["fastModeAvailability"]["available"] != true {
            return Err(error(
                409,
                "Fast 暂不可用，消息尚未发送；请关闭 Fast 后重试",
            ));
        }
        params["serviceTier"] = json!(tier);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_configuration_uses_effective_origins_and_never_guesses_conflicting_layers() {
        assert_eq!(
            configured_fast(&json!({"config":{"features":{"fast_mode":false}}})),
            Some(false)
        );
        assert_eq!(
            configured_fast(&json!({"config":{},"layers":[{"config":{}}]})),
            Some(true)
        );
        assert_eq!(configured_fast(&json!({"config":{}})), None);
        let user = json!({"name":{"type":"user"},"version":"user-v1","config":{"features":{"fast_mode":true}}});
        let system = json!({"name":{"type":"system"},"version":"system-v1","config":{"features":{"fast_mode":false}}});
        let mut config = json!({"config":{},"layers":[system.clone(),user.clone()]});
        assert_eq!(configured_fast(&config), None);
        config["origins"] =
            json!({"features.fast_mode":{"name":user["name"],"version":user["version"]}});
        assert_eq!(configured_fast(&config), Some(true));
        config["origins"]["features.fast_mode"]["name"] = system["name"].clone();
        config["origins"]["features.fast_mode"]["version"] = system["version"].clone();
        assert_eq!(configured_fast(&config), Some(false));
        assert_eq!(
            configured_fast(&json!({"config":{},"layers":[
                {"disabledReason":"untrusted","config":{"features":{"fast_mode":false}}}, user
            ]})),
            Some(true)
        );
        assert_eq!(
            configured_fast(&json!({"config":{},"layers":[
                {"config":{"features":{"fast_mode":"false"}}}
            ]})),
            None
        );
    }

    #[test]
    fn service_fast_uses_actual_gpt_model_without_requiring_declared_capability() {
        let normalize = |row: Value| service_model(&row, &Map::new()).unwrap();
        for row in [
            json!({"model":"claude-opus","serviceTiers":[{"id":"priority"}],"supports_fast":true}),
            json!({"model":"unknown-alias","serviceTiers":[{"id":"priority"}]}),
            json!({"model":"gpt-unknown","serviceTiers":[{"id":"priority"}]}),
            json!({"model":"gpt-6.1-alias","requestModel":"claude-opus","serviceTiers":[{"id":"priority"}]}),
        ] {
            let catalog = json!({"data":[normalize(row)]});
            let name = catalog["data"][0]["model"].as_str().unwrap();
            assert_eq!(fast_tier(&catalog, name), None);
        }
        for row in [
            json!({"model":"gpt-6.1-sol"}),
            json!({"model":"openai/GPT-6.1-sol","serviceTiers":[]}),
            json!({"model":"gpt-6.1-sol","serviceTiers":[],"supports_fast":false}),
            json!({"model":"gpt-6.1-sol","serviceTiers":[{"id":"priority"}],"supports_fast":true,"fastModeSupported":false}),
            json!({"model":"gpt-6.1-custom","serviceTiers":[{"id":"flex"}]}),
        ] {
            let normalized = normalize(row);
            let catalog = json!({"data":[normalized.clone()]});
            let name = normalized["model"].as_str().unwrap();
            assert_eq!(normalized["fastModeSupported"], true);
            assert_eq!(fast_tier(&catalog, name), Some("priority"));
            assert_eq!(normalize(normalized.clone()), normalized);
        }
        let alias = normalize(json!({"id":"alias-id","model":"sol-alias",
            "requestModel":"gpt-6.1-sol","serviceTiers":[]}));
        let catalog = json!({"data":[alias]});
        assert_eq!(fast_tier(&catalog, "sol-alias"), Some("priority"));
        assert_eq!(fast_tier(&catalog, "alias-id"), Some("priority"));
        assert_eq!(fast_tier(&catalog, "gpt-6.1-sol"), Some("priority"));
        assert_eq!(fast_tier(&catalog, "other-thread-model"), None);
        let explicit = normalize(json!({"model":"gpt-6.1-sol","serviceTiers":[{"id":"fast"}]}));
        assert_eq!(
            fast_tier(&json!({"data":[explicit]}), "gpt-6.1-sol"),
            Some("fast")
        );
        let aliases = serde_json::from_value::<Map<String, Value>>(json!({
            "主力模型":"openai/gpt-6.1-sol",
            "GPT-6.1-alias":"claude-opus"
        }))
        .unwrap();
        for (name, expected) in [("主力模型", Some("priority")), ("gpt-6.1-alias", None)] {
            let normalized = service_model(&json!({"model":name}), &aliases).unwrap();
            assert_eq!(fast_tier(&json!({"data":[normalized]}), name), expected);
        }
        let catalog = json!({"data":[],"fastModelAliases":aliases});
        assert_eq!(fast_tier(&catalog, "gpt-6.1-unlisted"), Some("priority"));
        assert_eq!(fast_tier(&catalog, "主力模型"), Some("priority"));
        assert_eq!(fast_tier(&catalog, "gpt-6.1-alias"), None);
        assert_eq!(fast_tier(&catalog, "unknown-alias"), None);
    }
}
