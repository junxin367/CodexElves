use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::{AbortHandle, Abortable};
use serde_json::{Value, json};

use super::RemoteAuthorization;
use crate::prompt_optimize::{PromptOptimizeService, load_preferences, remote_payload};
use crate::settings::SettingsStore;

const MAX_REQUESTS: usize = 64;
const RETENTION: Duration = Duration::from_secs(600);

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
