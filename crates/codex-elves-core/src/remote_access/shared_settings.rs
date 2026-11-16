//! 除外观之外的远程偏好由同一服务保存；不按设备分库，不存放草稿、凭据或操作系统授权。
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{ApiError, RemoteAccessController, RemoteAuthorization, WebState, permissions};

const MAX_BYTES: usize = 64 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    revision: u64,
    #[serde(default = "empty_object")]
    values: Value,
}

fn empty_object() -> Value {
    json!({})
}

pub(super) fn defaults() -> Value {
    json!({
        "conversationWidth":950,"projectThreadLimit":5,
        "sessionTypes":{"codex":true,"claude":false},"sessionInstances":null,"sessionFilter":"all",
        "sidebar":{"mode":null,"chats":null,"projects":null,"pinned":null,"showPinned":false},
        "newChat":{},
        "notifications":{"enabled":false,"approval":true,"question":true,"completed":true,"failed":false},
        "conversationNaming":super::conversation_naming::Settings::default()
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b":-_".contains(&byte)
        })
}

fn valid_text(value: &Value, limit: usize) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.len() <= limit && !text.chars().any(char::is_control))
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
    })
}

fn validate(value: &Value) -> Result<(), ApiError> {
    let invalid = || ApiError::new(400, "共享设置字段或取值无效");
    if !exact_keys(
        value,
        &[
            "conversationWidth",
            "projectThreadLimit",
            "sessionTypes",
            "sessionInstances",
            "sessionFilter",
            "sidebar",
            "newChat",
            "notifications",
            "conversationNaming",
        ],
    ) || !value["conversationWidth"]
        .as_u64()
        .is_some_and(|width| (320..=3840).contains(&width))
        || !value["projectThreadLimit"]
            .as_u64()
            .is_some_and(|limit| [5, 10, 20, 50].contains(&limit))
    {
        return Err(invalid());
    }
    for key in ["sessionTypes", "sessionInstances"] {
        if key == "sessionInstances" && value[key].is_null() {
            continue;
        }
        if !value[key].as_object().is_some_and(|entries| {
            entries.len() <= 128
                && entries
                    .iter()
                    .all(|(id, value)| valid_id(id) && value.is_boolean())
        }) {
            return Err(invalid());
        }
    }
    let filter = &value["sessionFilter"];
    super::conversation_naming::validate(&value["conversationNaming"])?;
    if filter != "all"
        && !filter.as_array().is_some_and(|ids| {
            !ids.is_empty()
                && ids.len() <= 128
                && ids.iter().all(|id| id.as_str().is_some_and(valid_id))
        })
    {
        return Err(invalid());
    }
    let sidebar = &value["sidebar"];
    if !exact_keys(
        sidebar,
        &["mode", "chats", "projects", "pinned", "showPinned"],
    ) || !(sidebar["mode"].is_null()
        || ["project", "list"]
            .iter()
            .any(|mode| sidebar["mode"] == *mode))
        || !["chats", "projects", "pinned"].iter().all(|key| {
            sidebar[key].is_null()
                || ["manual", "updated_at"]
                    .iter()
                    .any(|mode| sidebar[key] == *mode)
        })
        || !sidebar["showPinned"].is_boolean()
    {
        return Err(invalid());
    }
    if !value["newChat"].as_object().is_some_and(|instances| {
        instances.len() <= 128
            && instances.iter().all(|(id, choice)| {
                valid_id(id)
                    && choice.as_object().is_some_and(|fields| {
                        fields.len() <= 3
                            && fields.iter().all(|(key, value)| match key.as_str() {
                                "model" => valid_text(value, 200),
                                "reasoningEffort" => valid_text(value, 32),
                                "permissionMode" => {
                                    value == ""
                                        || permissions::validate_mode(
                                            id.split(':').next().unwrap_or(""),
                                            value,
                                        )
                                        .is_ok()
                                }
                                _ => false,
                            })
                    })
            })
    }) {
        return Err(invalid());
    }
    if !exact_keys(
        &value["notifications"],
        &["enabled", "approval", "question", "completed", "failed"],
    ) || !value["notifications"]
        .as_object()
        .unwrap()
        .values()
        .all(Value::is_boolean)
        || value["notifications"]["failed"] != false
    {
        return Err(invalid());
    }
    Ok(())
}

fn leaves(
    value: &Value,
    path: &mut Vec<String>,
    result: &mut Vec<(Vec<String>, Value)>,
) -> Result<(), ApiError> {
    if path.len() > 4 || result.len() >= 1024 {
        return Err(ApiError::new(400, "共享设置结构过深或字段过多"));
    }
    if let Some(object) = value.as_object().filter(|object| !object.is_empty()) {
        for (key, value) in object {
            path.push(key.clone());
            leaves(value, path, result)?;
            path.pop();
        }
    } else {
        result.push((path.clone(), value.clone()));
    }
    Ok(())
}

fn at<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |value, key| value.as_object()?.get(key))
}

fn put(value: &mut Value, path: &[String], replacement: Value) {
    if path.is_empty() {
        *value = replacement;
        return;
    }
    if !value.is_object() {
        *value = json!({});
    }
    let entry = value
        .as_object_mut()
        .unwrap()
        .entry(path[0].clone())
        .or_insert(Value::Null);
    put(entry, &path[1..], replacement);
}

fn effective(values: &Value) -> Result<Value, ApiError> {
    if !values.is_object() {
        return Err(ApiError::new(422, "共享设置文件格式无效，未覆盖原文件"));
    }
    let mut result = defaults();
    let mut fields = vec![];
    leaves(values, &mut vec![], &mut fields)?;
    for (path, value) in fields {
        if !path.is_empty() {
            put(&mut result, &path, value);
        }
    }
    validate(&result)?;
    Ok(result)
}

pub(super) struct SharedSettings {
    path: PathBuf,
    serial: Mutex<()>,
    epoch: String,
    instance_revision: AtomicU64,
}

impl SharedSettings {
    pub fn new(directory: &std::path::Path) -> Self {
        Self {
            path: directory.join("remote-shared-settings.json"),
            serial: Mutex::new(()),
            epoch: uuid::Uuid::new_v4().to_string(),
            instance_revision: AtomicU64::new(0),
        }
    }

    fn read(&self) -> Result<(Document, String), ApiError> {
        let (text, file_revision) = permissions::read_file(&self.path)?;
        let document = if text.is_empty() {
            Document {
                revision: 0,
                values: json!({}),
            }
        } else {
            serde_json::from_str(&text)
                .map_err(|_| ApiError::new(422, "共享设置文件格式无效，未覆盖原文件"))?
        };
        if document.revision > MAX_REVISION {
            return Err(ApiError::new(422, "共享设置版本超出客户端可精确表示的范围"));
        }
        effective(&document.values)?;
        Ok((document, file_revision))
    }

    fn view(&self, document: &Document) -> Result<Value, ApiError> {
        Ok(
            json!({"revision":document.revision,"settings":effective(&document.values)?,"stored":document.values,
            "epoch":self.epoch,"instanceRevision":self.instance_revision.load(Ordering::Acquire)}),
        )
    }

    pub async fn snapshot(&self) -> Result<Value, ApiError> {
        let _serial = self.serial.lock().await;
        self.view(&self.read()?.0)
    }

    pub fn instance_changed(&self, controller: &RemoteAccessController, instance: &str) {
        let revision = self.instance_revision.fetch_add(1, Ordering::AcqRel) + 1;
        controller.publish_event(json!({"method":"remote/settingsChanged","params":{
            "kind":"instance","instanceId":instance,"epoch":self.epoch,"instanceRevision":revision
        }}));
    }

    async fn update(
        &self,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> Result<Value, ApiError> {
        let patch = &payload["patch"];
        if !patch.as_object().is_some_and(|object| !object.is_empty())
            || serde_json::to_vec(patch).unwrap().len() > MAX_BYTES
        {
            return Err(ApiError::new(400, "共享设置修改内容无效或超过 64 KiB"));
        }
        let initialize = payload["operation"] == "initialize";
        if !initialize && payload["operation"] != "save" {
            return Err(ApiError::new(400, "共享设置操作无效"));
        }
        let expected = payload["expectedRevision"]
            .as_u64()
            .ok_or_else(|| ApiError::new(400, "缺少共享设置版本"))?;
        // 命名配置保存与最终标题写回互斥；锁顺序固定为配置锁、共享文件锁，保存完成后不再落入旧配置结果。
        let _naming_configuration = if patch.get("conversationNaming").is_some() {
            Some(
                authorization
                    .controller
                    .conversation_naming
                    .configuration
                    .write()
                    .await,
            )
        } else {
            None
        };
        let _serial = self.serial.lock().await;
        let (mut document, file_revision) = self.read()?;
        let current = effective(&document.values)?;
        let mut changes = vec![];
        leaves(patch, &mut vec![], &mut changes)?;
        // 用户要求命名实例与模型关联：即使两端选择同名模型，旧端也不能把另一实例的模型合并进新实例。
        let naming_target_patch = changes.iter().any(|(path, _)| {
            path.first().is_some_and(|key| key == "conversationNaming")
                && path
                    .get(1)
                    .is_some_and(|key| matches!(key.as_str(), "instanceId" | "model"))
        });
        let naming_target_conflict = naming_target_patch
            && ["instanceId", "model"].iter().any(|key| {
                payload["base"]["conversationNaming"][*key] != current["conversationNaming"][*key]
            });
        // 多端仅提交显式修改的叶子字段；旧版本可合并不冲突字段，同一字段被其他端修改时返回冲突，不自动覆盖。
        if expected > document.revision
            || !initialize
                && expected != document.revision
                && (naming_target_conflict
                    || changes
                        .iter()
                        .any(|(path, _)| at(&payload["base"], path) != at(&current, path)))
        {
            return Err(ApiError::new(
                409,
                "设置已在其他客户端更新，请重新读取后再修改",
            ));
        }
        let before = document.values.clone();
        for (path, value) in changes {
            // 空实例映射表示明确关闭全部；模型和等级按实例成对迁移，不能被后来客户端的旧值拆散。
            let group_initialized = initialize
                && (matches!(
                    path.first().map(String::as_str),
                    Some("sessionInstances" | "notifications")
                ) && before.get(&path[0]).is_some()
                    || path.first().is_some_and(|key| key == "newChat")
                        && (path.len() > 1 && at(&before, &path[..2]).is_some()
                            || before["newChat"]
                                .as_object()
                                .is_some_and(|values| values.is_empty())));
            if !group_initialized && (!initialize || at(&before, &path).is_none()) {
                put(&mut document.values, &path, value);
            }
        }
        effective(&document.values)?;
        if document.values != before {
            // 网页与 Android 共用整数版本；保持在 JavaScript 可精确表示的 53 位范围内。
            document.revision = document
                .revision
                .checked_add(1)
                .filter(|revision| *revision <= MAX_REVISION)
                .ok_or_else(|| ApiError::new(409, "共享设置版本已达上限"))?;
            let output = serde_json::to_string(&document).unwrap();
            if output.len() > MAX_BYTES {
                return Err(ApiError::new(413, "共享设置超过 64 KiB"));
            }
            let permit = authorization
                .prepare_dispatch()
                .await
                .map_err(super::authorization_error)?;
            permit
                .commit(|| permissions::save_file(&self.path, &output, &file_revision))
                .map_err(super::authorization_error)?;
            authorization
                .controller
                .publish_event(json!({"method":"remote/settingsChanged",
                "params":{"kind":"preferences","epoch":self.epoch,"revision":document.revision}}));
        }
        authorization
            .check()
            .await
            .map_err(super::authorization_error)?;
        self.view(&document)
    }
}

pub(super) async fn read(
    State(state): State<WebState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state.controller.authorization(device, None);
    let snapshot = state.controller.shared_settings.snapshot().await?;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"ok":true,"snapshot":snapshot})),
    )
        .into_response())
}

pub(super) async fn write(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    if state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    let authorization = state
        .controller
        .authorization(device, Some("setSharedSettings"));
    let snapshot = state
        .controller
        .shared_settings
        .update(&payload, &authorization)
        .await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"ok":true,"snapshot":snapshot})),
    )
        .into_response())
}

#[cfg(test)]
mod shared_settings_development {
    use super::*;

    async fn fixture() -> (tempfile::TempDir, RemoteAuthorization) {
        let (directory, auth) = super::super::tests::service_dispatch_fixture().await;
        let authorization = auth
            .controller
            .authorization(auth.device.clone(), Some("setSharedSettings"));
        (directory, authorization)
    }

    #[tokio::test]
    async fn shared_settings_development_merge_independent_client_fields_and_persist() {
        let (directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        let a = settings.snapshot().await.unwrap();
        let first = settings
            .update(
                &json!({"operation":"save","expectedRevision":a["revision"],
            "base":a["settings"],"patch":{"conversationWidth":1100}}),
                &auth,
            )
            .await
            .unwrap();
        let second = settings
            .update(
                &json!({"operation":"save","expectedRevision":a["revision"],
            "base":a["settings"],"patch":{"projectThreadLimit":20}}),
                &auth,
            )
            .await
            .unwrap();
        assert_eq!(first["settings"]["conversationWidth"], 1100);
        assert_eq!(second["settings"]["conversationWidth"], 1100);
        assert_eq!(second["settings"]["projectThreadLimit"], 20);
        let reopened = SharedSettings::new(directory.path())
            .snapshot()
            .await
            .unwrap();
        assert_eq!(reopened["settings"], second["settings"]);
        assert_eq!(reopened["revision"], 2);
    }

    #[tokio::test]
    async fn shared_settings_development_conflicting_field_does_not_overwrite() {
        let (_directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        let a = settings.snapshot().await.unwrap();
        settings
            .update(
                &json!({"operation":"save","expectedRevision":0,"base":a["settings"],
            "patch":{"conversationWidth":1100}}),
                &auth,
            )
            .await
            .unwrap();
        let failure = settings
            .update(
                &json!({"operation":"save","expectedRevision":0,"base":a["settings"],
            "patch":{"conversationWidth":2000}}),
                &auth,
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, 409);
        assert_eq!(
            settings.snapshot().await.unwrap()["settings"]["conversationWidth"],
            1100
        );
    }

    #[tokio::test]
    async fn shared_settings_development_naming_is_global_and_keeps_field_conflicts() {
        let (directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        let initial = settings.snapshot().await.unwrap();
        let first = settings.update(&json!({"operation":"save","expectedRevision":0,"base":initial["settings"],
            "patch":{"conversationNaming":{"enabled":true,"instanceId":"claude:wsl:fixture","model":"fixture-model"}}}),&auth).await.unwrap();
        let second = settings
            .update(
                &json!({"operation":"save","expectedRevision":0,"base":initial["settings"],
            "patch":{"conversationNaming":{"maxTitleChars":48}}}),
                &auth,
            )
            .await
            .unwrap();
        assert_eq!(
            second["settings"]["conversationNaming"]["instanceId"],
            "claude:wsl:fixture"
        );
        assert_eq!(
            second["settings"]["conversationNaming"]["model"],
            "fixture-model"
        );
        assert_eq!(
            second["settings"]["conversationNaming"]["maxTitleChars"],
            48
        );
        let conflict = settings
            .update(
                &json!({"operation":"save","expectedRevision":0,"base":initial["settings"],
            "patch":{"conversationNaming":{"model":"stale-model"}}}),
                &auth,
            )
            .await
            .unwrap_err();
        assert_eq!(conflict.code, 409);
        let moved = settings
            .update(
                &json!({"operation":"save","expectedRevision":second["revision"],
            "base":second["settings"],"patch":{"conversationNaming":{"instanceId":"codex:local"}}}),
                &auth,
            )
            .await
            .unwrap();
        let stale_model = settings.update(&json!({"operation":"save","expectedRevision":second["revision"],
            "base":second["settings"],"patch":{"conversationNaming":{"model":"model-from-old-instance"}}}),&auth).await.unwrap_err();
        assert_eq!(stale_model.code, 409);
        let reopened = SharedSettings::new(directory.path())
            .snapshot()
            .await
            .unwrap();
        assert_eq!(
            reopened["settings"]["conversationNaming"],
            moved["settings"]["conversationNaming"]
        );
        assert_eq!(
            first["settings"]["conversationNaming"]["refreshEveryTurns"],
            5
        );
    }

    #[tokio::test]
    async fn shared_settings_development_initialization_only_fills_missing_fields() {
        let (_directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        settings
            .update(
                &json!({"operation":"initialize","expectedRevision":0,
            "patch":{"conversationWidth":1100,"sidebar":{"mode":"list"}}}),
                &auth,
            )
            .await
            .unwrap();
        let value = settings
            .update(
                &json!({"operation":"initialize","expectedRevision":0,
            "patch":{"conversationWidth":2000,"sidebar":{"mode":"project","showPinned":true}}}),
                &auth,
            )
            .await
            .unwrap();
        assert_eq!(value["settings"]["conversationWidth"], 1100);
        assert_eq!(value["settings"]["sidebar"]["mode"], "list");
        assert_eq!(value["settings"]["sidebar"]["showPinned"], true);
    }

    #[tokio::test]
    async fn shared_settings_development_migration_preserves_empty_instances_and_model_pair() {
        let (_directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        settings
            .update(
                &json!({"operation":"save","expectedRevision":0,"patch":{
            "sessionInstances":{},"newChat":{"codex:local":{"model":"new-model"}}}}),
                &auth,
            )
            .await
            .unwrap();
        let value = settings
            .update(
                &json!({"operation":"initialize","expectedRevision":0,"patch":{
            "sessionInstances":{"codex:local":true},"newChat":{"codex:local":{
                "model":"old-model","reasoningEffort":"ultra"}}}}),
                &auth,
            )
            .await
            .unwrap();
        assert_eq!(value["settings"]["sessionInstances"], json!({}));
        assert_eq!(
            value["settings"]["newChat"]["codex:local"]["model"],
            "new-model"
        );
        assert!(value["settings"]["newChat"]["codex:local"]["reasoningEffort"].is_null());
    }

    #[tokio::test]
    async fn shared_settings_development_reject_appearance_unknown_fields_and_invalid_choices() {
        let (_directory, auth) = fixture().await;
        let settings = &auth.controller.shared_settings;
        for patch in [
            json!({"fontSize":18}),
            json!({"conversationWidth":10}),
            json!({"notifications":{"failed":true}}),
            json!({"sessionInstances":{"bad/path":true}}),
            json!({"newChat":{"codex:local":{"permissionMode":"bypassPermissions"}}}),
        ] {
            assert!(
                settings
                    .update(
                        &json!({"operation":"save","expectedRevision":0,"patch":patch}),
                        &auth
                    )
                    .await
                    .is_err()
            );
        }
        assert_eq!(settings.snapshot().await.unwrap()["revision"], 0);
    }

    #[tokio::test]
    async fn shared_settings_development_readonly_policy_stops_writes() {
        let (_directory, auth) = fixture().await;
        auth.controller
            .publish_access_policy(&super::super::RemoteAccessSettings {
                enabled: true,
                read_only: true,
                ..Default::default()
            });
        let failure = auth
            .controller
            .shared_settings
            .update(
                &json!({"operation":"save",
            "expectedRevision":0,"patch":{"projectThreadLimit":50}}),
                &auth,
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, 403);
        assert_eq!(
            auth.controller.shared_settings.snapshot().await.unwrap()["revision"],
            0
        );
    }
}
