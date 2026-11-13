use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::{AbortHandle, Abortable};
use serde_json::{Value, json};

use super::{ApiError, RemoteAuthorization, WebState};
use crate::prompt_optimize::{PromptOptimizeService, load_preferences, remote_payload};
use crate::settings::SettingsStore;

const MAX_REQUESTS: usize = 64;
const RETENTION: Duration = Duration::from_secs(600);

pub(super) async fn settings(
    axum::extract::State(state): axum::extract::State<WebState>,
    headers: axum::http::HeaderMap,
    axum::Json(payload): axum::Json<Value>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let device = super::authorized(&state, &headers).await?;
    let writing = payload.get("settings").is_some();
    if writing && state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "优化设置参数无效"))?;
    if object
        .keys()
        .any(|key| !["settings", "expectedRevision"].contains(&key.as_str()))
    {
        return Err(ApiError::new(400, "优化设置包含不支持的字段"));
    }
    let directory = &state.controller.state_dir;
    let mut authorization = state.controller.authorization(device, None);
    let catalog = crate::model_catalog::read_codex_model_catalog().await;
    let backend = SettingsStore::new(directory.join("settings.json"))
        .load()
        .map_err(|e| ApiError::new(400, e.to_string()))?;
    let mut preferences =
        load_preferences(directory).map_err(|e| ApiError::new(400, e.to_string()))?;
    let revision = super::hash(&preferences.to_string());
    if writing {
        if payload["expectedRevision"].as_str() != Some(&revision) {
            return Err(ApiError::new(
                409,
                "提示词优化设置已变化，请重新打开设置后保存",
            ));
        }
        crate::prompt_optimize::validate_preferences(&payload["settings"])
            .map_err(|e| ApiError::new(400, e.to_string()))?;
        authorization.write_action = Some("savePromptOptimizationSettings".into());
        let permit = authorization
            .prepare_dispatch()
            .await
            .map_err(super::authorization_error)?;
        // PC 远程右键设置与桌面注入版共用同一文件；保存只修改优化偏好，不进入会话引擎或模型请求。
        permit
            .commit(|| {
                if super::hash(&load_preferences(directory)?.to_string()) != revision {
                    return Err(
                        ApiError::new(409, "提示词优化设置已变化，请重新打开设置后保存").into(),
                    );
                }
                crate::prompt_optimize::save_preferences(directory, &payload["settings"]).map_err(
                    |_| {
                        ApiError::new(500, "无法保存提示词优化设置，请检查本机配置目录的写入权限")
                            .into()
                    },
                )
            })
            .map_err(super::authorization_error)?;
        preferences = load_preferences(directory).map_err(|e| ApiError::new(400, e.to_string()))?;
    }
    let profile = backend.active_relay_profile();
    let mut ids: Vec<String> = catalog["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model.as_str().map(str::to_owned))
        .collect();
    if ids.is_empty() {
        ids = crate::model_catalog::relay_profile_model_ids_for_proxy(&profile);
    }
    let models: Vec<Value> = ids.iter().map(|model| {
        let entry = catalog["model_entries"].as_array().into_iter().flatten()
            .find(|entry| entry["slug"] == *model);
        let protocol = match profile.resolve_protocol_for_model(model)
            .unwrap_or_else(|_| crate::settings::infer_relay_protocol_for_model(model)) {
            crate::settings::RelayProtocol::Responses => crate::protocol_proxy::UpstreamResponseProtocol::Responses,
            crate::settings::RelayProtocol::ChatCompletions => crate::protocol_proxy::UpstreamResponseProtocol::ChatCompletions,
            crate::settings::RelayProtocol::Anthropic => crate::protocol_proxy::UpstreamResponseProtocol::Anthropic,
        };
        let declared = entry.and_then(|entry| entry["supported_reasoning_levels"].as_array()
            .or_else(|| entry["supportedReasoningEfforts"].as_array()));
        let supported = declared.map(|levels| levels.iter().filter_map(|level| level["effort"].as_str()
            .or_else(|| level["reasoningEffort"].as_str()).or_else(|| level.as_str()))
            .map(str::to_owned).collect::<Vec<_>>()).filter(|levels| !levels.is_empty())
            .unwrap_or_else(|| crate::protocol_proxy::supported_reasoning_efforts_for_model(model, protocol)
                .into_iter().map(str::to_owned).collect());
        let efforts = supported.into_iter().filter(|level| ["low","medium","high","xhigh","max"].contains(&level.as_str()))
            .filter(|level| crate::model_capabilities::model_family(model) != crate::model_capabilities::ModelFamily::Gpt
                || level != "max").collect::<Vec<_>>();
        json!({"value":model,"label":entry.and_then(|entry| entry["display_name"].as_str()).unwrap_or(model),
            "efforts":if efforts.is_empty() {vec!["medium".to_owned()]} else {efforts}})
    }).collect();
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(([("Cache-Control", "no-store")], axum::Json(json!({
        "ok":true,"settings":preferences,"revision":super::hash(&preferences.to_string()),
        "defaults":crate::prompt_optimize::default_preferences(),"models":models,
        "defaultModel":catalog["default_model"],
        // 只返回模型展示数据；供应商 URL、认证密钥及整个桌面设置不得传给网页。
        "supplier":catalog["provider_name"].as_str().filter(|name| !name.is_empty()).unwrap_or(&profile.name)
    }))).into_response())
}

struct Request {
    created_at: Instant,
    result: Value,
    abort: Option<AbortHandle>,
}

#[derive(Default)]
pub(super) struct RemotePromptOptimization {
    requests: Mutex<HashMap<String, Request>>,
    optimizer: Arc<PromptOptimizeService>,
}

impl RemotePromptOptimization {
    pub async fn call(
        self: &Arc<Self>,
        action: &str,
        payload: Value,
        directory: &Path,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        authorization.check().await?;
        let id = payload["requestId"].as_str().unwrap_or("");
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(json!({"ok":false,"code":400,"message":"提示词优化请求 ID 无效"}));
        }
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        requests.retain(|_, request| {
            request.abort.is_some() || request.created_at.elapsed() < RETENTION
        });
        if action == "promptOptimization" {
            return Ok(requests
                .get(id)
                .map(|request| request.result.clone())
                .unwrap_or_else(
                    || json!({"ok":false,"code":404,"message":"未找到提示词优化请求"}),
                ));
        }
        if action == "cancelPromptOptimize" {
            if let Some(request) = requests.get_mut(id) {
                if let Some(abort) = request.abort.take() {
                    abort.abort();
                    request.result = json!({"ok":true,"state":"cancelled"});
                }
                return Ok(request.result.clone());
            }
        } else if let Some(request) = requests.get(id) {
            return Ok(request.result.clone());
        }
        if requests.len() >= MAX_REQUESTS {
            return Ok(json!({"ok":false,"code":429,"message":"提示词优化请求过多，请稍后重试"}));
        }
        if action == "cancelPromptOptimize" {
            // 取消可能比原请求先到，保留回执，稍后到达的 start 不得重新发给供应商。
            let result = json!({"ok":true,"state":"cancelled"});
            requests.insert(
                id.to_string(),
                Request {
                    created_at: Instant::now(),
                    result: result.clone(),
                    abort: None,
                },
            );
            return Ok(result);
        }

        let prepared = (|| {
            let settings = SettingsStore::new(directory.join("settings.json")).load()?;
            let preferences = load_preferences(directory)?;
            let request = remote_payload(&preferences, &payload)?;
            anyhow::Ok((settings, request))
        })();
        let (settings, request) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return Ok(json!({
                    "ok":false,"code":400,"message":error.to_string()
                }));
            }
        };
        let (abort, registration) = AbortHandle::new_pair();
        let result = json!({"ok":true,"state":"pending"});
        requests.insert(
            id.to_string(),
            Request {
                created_at: Instant::now(),
                result: result.clone(),
                abort: Some(abort),
            },
        );
        drop(requests);

        let state = Arc::downgrade(self);
        let optimizer = self.optimizer.clone();
        let authorization = authorization.clone();
        let id = id.to_string();
        super::execution::spawn_inherited(async move {
            let result = Abortable::new(
                async {
                    authorization.check().await?;
                    optimizer.optimize(settings, request).await
                },
                registration,
            )
            .await;
            let result = match result {
                Err(_) => json!({"ok":true,"state":"cancelled"}),
                Ok(Ok(value)) if value["status"] == "ok" => json!({
                    "ok":true,"state":"completed","text":value["text"],"totalTokens":value["totalTokens"]
                }),
                Ok(Ok(_)) => json!({"ok":true,"state":"cancelled"}),
                Ok(Err(error)) => json!({"ok":true,"state":"failed","message":error.to_string()}),
            };
            if let Some(state) = state.upgrade() {
                let mut requests = state
                    .requests
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Some(request) = requests.get_mut(&id) {
                    if request.abort.take().is_some() {
                        request.result = result;
                    }
                }
            }
        });
        Ok(result)
    }

    pub fn cancel_all(&self) {
        let mut requests = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for request in requests.values_mut() {
            if let Some(abort) = request.abort.take() {
                abort.abort();
                request.result = json!({"ok":true,"state":"cancelled"});
            }
        }
    }
}

impl Drop for RemotePromptOptimization {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, body::to_bytes, extract::State, http::HeaderMap};

    #[tokio::test]
    async fn remote_optimizer_settings_share_preferences_and_require_write_authorization() {
        let (directory, authorization) = super::super::tests::service_dispatch_fixture().await;
        let state = WebState {
            controller: authorization.controller.clone(),
            settings: super::super::RemoteAccessSettings::default(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            format!(
                "{}=isolated-service-dispatch-token",
                super::super::COOKIE_NAME
            )
            .parse()
            .unwrap(),
        );
        let response = settings(State(state.clone()), headers.clone(), Json(json!({})))
            .await
            .unwrap();
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 256 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(value["ok"], true);
        assert!(
            value["defaults"]["prompts"]["coding"]
                .as_str()
                .unwrap()
                .contains("软件工程")
        );
        assert!(value["models"].as_array().unwrap().iter().all(|model| {
            model
                .as_object()
                .unwrap()
                .keys()
                .all(|key| ["value", "label", "efforts"].contains(&key.as_str()))
        }));
        let mut preferences = value["defaults"].clone();
        preferences["model"] = json!("configured-model");
        preferences["style"] = json!("coding");
        preferences["includeRecentContext"] = json!(true);
        let payload = json!({"settings":preferences,"expectedRevision":value["revision"]});
        let response = settings(State(state.clone()), headers.clone(), Json(payload.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(load_preferences(directory.path()).unwrap(), preferences);
        let stale = settings(State(state.clone()), headers.clone(), Json(payload.clone()))
            .await
            .err()
            .unwrap();
        assert_eq!(stale.code, 409);
        let mut read_only = state.clone();
        read_only.settings.read_only = true;
        let denied = settings(State(read_only), headers.clone(), Json(payload))
            .await
            .err()
            .unwrap();
        assert_eq!(denied.code, 403);
        let denied = settings(State(state), HeaderMap::new(), Json(json!({})))
            .await
            .err()
            .unwrap();
        assert_eq!(denied.code, 401);
        assert_eq!(load_preferences(directory.path()).unwrap(), preferences);
    }
}
