use super::*;

pub(super) const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_BUNDLE_BYTES: usize = 256 * 1024;
const MAX_EVENTS: usize = 400;
const REQUEST_LIFETIME_MS: u64 = 120_000;
const MIN_UPLOAD_INTERVAL_MS: u64 = 5_000;
const MAX_JSON_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    request_id: String,
    expires_at_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    device_id: String,
    request_id: String,
    received_at_ms: u64,
    bundle: Value,
}

impl Report {
    fn metadata(&self) -> Value {
        json!({
            "deviceId":self.device_id,
            "requestId":self.request_id,
            "receivedAtMs":self.received_at_ms,
            "capturedAtMs":self.bundle["capturedAtMs"],
            "eventCount":self.bundle["events"].as_array().map_or(0, Vec::len),
            "app":self.bundle["app"],
        })
    }
}

#[derive(Default)]
struct StateData {
    pending: HashMap<String, Pending>,
}

#[derive(Default)]
pub(super) struct Diagnostics {
    state: Mutex<StateData>,
}

impl Diagnostics {
    pub(super) async fn pending_for(&self, device_id: &str) -> Option<Value> {
        let mut state = self.state.lock().await;
        if state
            .pending
            .get(device_id)
            .is_some_and(|pending| pending.expires_at_ms <= now_ms())
        {
            state.pending.remove(device_id);
        }
        state.pending.get(device_id).map(|pending| {
            json!({"deviceId":device_id,"requestId":pending.request_id,"expiresAtMs":pending.expires_at_ms})
        })
    }
}

fn valid_identifier(value: &str, maximum: usize, dots: bool) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || dots && byte == b'.'
        })
}

fn integer(value: &Value) -> Option<Value> {
    value
        .as_u64()
        .filter(|value| *value <= MAX_JSON_INTEGER)
        .map(Value::from)
}

fn host(value: &str) -> Option<String> {
    if value.len() > 2048 || value.contains('\\') {
        return None;
    }
    let url = reqwest::Url::parse(&if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    })
    .ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let name = url.host_str()?;
    if name.len() > 160 {
        return None;
    }
    let result = match url.port() {
        Some(port) => format!("{name}:{port}"),
        None => name.to_owned(),
    };
    (result.len() <= 160).then_some(result)
}

fn path(value: &str) -> Option<String> {
    if value.len() > 2048 || value.contains('\\') {
        return None;
    }
    let owned;
    let value = if value.starts_with("http://") || value.starts_with("https://") {
        owned = reqwest::Url::parse(value).ok()?;
        owned.path()
    } else {
        value.split(['?', '#']).next()?
    };
    let value = value
        .strip_prefix("/files/")
        .map_or_else(|| value.to_owned(), |name| format!("/{name}"));
    if is_static_web_asset_path(&value)
        || matches!(
            value.as_str(),
            "/index.html" | "/session-tools.js" | "/session-menu.js"
        )
    {
        return Some(value);
    }
    if matches!(
        value.as_str(),
        "/api/connect"
            | "/api/login-status"
            | "/api/session"
            | "/api/logout"
            | "/api/channels"
            | "/api/channel-events"
            | "/api/events"
            | "/api/events/ws"
            | "/api/diagnostics"
            | "/api/workspace/changes"
            | "/api/workspace/change"
            | "/api/receipts"
            | "/other"
            | "/api/images/:id"
            | "/api/images/:id/thumbnail"
            | "/api/videos/:id"
            | "/api/videos/:id/poster"
    ) {
        return Some(value);
    }
    if let Some(action) = value.strip_prefix("/api/read/")
        && matches!(
            action,
            "sidebar"
                | "history"
                | "thread"
                | "models"
                | "skills"
                | "queue"
                | "queueEdit"
                | "sendState"
                | "promptOptimization"
                | "notifications"
                | "relatedThreads"
        )
    {
        return Some(value);
    }
    if let Some(action) = value.strip_prefix("/api/action/")
        && matches!(
            action,
            "send"
                | "create"
                | "createSideThread"
                | "closeSideThread"
                | "deleteThread"
                | "stop"
                | "approve"
                | "answer"
                | "optimizePrompt"
                | "cancelPromptOptimize"
                | "compact"
                | "markRead"
                | "removeQueued"
                | "reorderQueued"
                | "beginQueuedEdit"
                | "editQueued"
                | "cancelQueuedEdit"
                | "dismissQueuedEdit"
                | "steerQueued"
                | "editMessage"
                | "pinProject"
                | "pinThread"
                | "sort"
                | "rename"
                | "setGoal"
                | "updateGoal"
        )
    {
        return Some(value);
    }
    if channels::media_path(&axum::http::Method::GET, &value) {
        let (kind, suffix) = if value.starts_with("/api/images/") {
            (
                "images",
                value.strip_suffix("/thumbnail").map(|_| "/thumbnail"),
            )
        } else {
            ("videos", value.strip_suffix("/poster").map(|_| "/poster"))
        };
        return Some(format!("/api/{kind}/:id{}", suffix.unwrap_or("")));
    }
    None
}

fn string_field(key: &str, value: &str) -> Option<String> {
    if value.len() > 160 {
        return None;
    }
    let allowed = match key {
        "host" | "originHost" => return host(value),
        "path" => return path(value),
        "method" => matches!(
            value,
            "GET" | "HEAD" | "POST" | "OPTIONS" | "PUT" | "PATCH" | "DELETE"
        ),
        "mime" => matches!(
            value,
            "application/json"
                | "application/javascript"
                | "application/octet-stream"
                | "application/manifest+json"
                | "text/javascript"
                | "text/html"
                | "text/css"
                | "text/plain"
                | "text/event-stream"
                | "image/jpeg"
                | "image/png"
                | "image/webp"
                | "image/gif"
                | "image/avif"
                | "image/svg+xml"
                | "video/mp4"
                | "video/webm"
                | "video/ogg"
                | "audio/mpeg"
                | "audio/mp4"
                | "audio/webm"
                | "unknown"
                | "other"
        ),
        "errorType" => matches!(
            value,
            "Error"
                | "TypeError"
                | "NetworkError"
                | "TimeoutError"
                | "AbortError"
                | "SecurityError"
                | "SyntaxError"
                | "RangeError"
                | "ReferenceError"
                | "URIError"
                | "EvalError"
                | "DOMException"
                | "HttpError"
                | "IOException"
                | "InterruptedIOException"
                | "FileNotFoundException"
                | "SSLException"
                | "SSLHandshakeException"
                | "RemoteException"
                | "MissingWebViewPackageException"
                | "BindException"
                | "UnsupportedOperationException"
                | "Exception"
                | "NullPointerException"
                | "ClassCastException"
                | "IndexOutOfBoundsException"
                | "ArrayIndexOutOfBoundsException"
                | "NoSuchElementException"
                | "ConcurrentModificationException"
                | "UnknownHostException"
                | "SocketTimeoutException"
                | "ConnectException"
                | "IllegalStateException"
                | "IllegalArgumentException"
                | "SecurityException"
                | "RuntimeException"
                | "CancelledException"
                | "JSONException"
                | "unknown"
                | "http"
                | "network"
                | "timeout"
                | "abort"
                | "ssl"
                | "renderer"
                | "resource"
                | "parse"
        ),
        "readyState" => matches!(value, "loading" | "interactive" | "complete"),
        "imageKind" => matches!(
            value,
            "image"
                | "thumbnail"
                | "poster"
                | "inline"
                | "remote"
                | "attachment"
                | "markdown"
                | "preview"
                | "unknown"
                | "full"
                | "video"
        ),
        "network" => matches!(
            value,
            "wifi"
                | "cellular"
                | "ethernet"
                | "bluetooth"
                | "vpn"
                | "none"
                | "unknown"
                | "slow-2g"
                | "2g"
                | "3g"
                | "4g"
                | "5g"
                | "online"
                | "offline"
                | "other"
        ),
        // These fields contain identifiers generated by the app, never error messages.
        "version" | "versionName" | "resourceVersion" | "webViewVersion" => {
            valid_identifier(value, 160, true)
        }
        "requestId" => valid_identifier(value, 80, false),
        "reason" => matches!(
            value,
            "success"
                | "cancelled"
                | "timeout"
                | "response-read-failed"
                | "http-error"
                | "network-error"
                | "manual"
                | "visibility"
                | "online"
                | "offline"
                | "pagehide"
                | "upload-failed"
                | "unsupported"
                | "native-failed"
                | "unknown"
                | "startup-timeout"
                | "load-failed"
                | "ssl-failed"
                | "renderer-crashed"
                | "renderer-killed"
                | "local-read-failed"
                | "select-failed"
                | "auth-required"
                | "logged-out"
                | "invalid-response"
                | "invalid-content-type"
                | "unchanged"
                | "downloaded"
                | "failed"
                | "ready"
                | "bridge-unavailable"
                | "export-failed"
                | "snapshot-failed"
                | "login-failed"
                | "login-success"
                | "cookie-failed"
                | "origin-changed"
                | "error"
                | "unhandled-rejection"
                | "remote"
                | "hidden"
                | "visible"
                | "event"
                | "request"
                | "response"
                | "api"
                | "image"
                | "reload"
                | "reconnect"
                | "auth-lost"
                | "cors-blocked"
                | "corp-blocked"
                | "net-failed"
                | "console-error"
        ),
        "phase" => matches!(
            value,
            "create"
                | "start"
                | "resume"
                | "pause"
                | "stop"
                | "destroy"
                | "login"
                | "page"
                | "ready"
                | "authenticated"
                | "check"
                | "download"
                | "select"
                | "rollback"
                | "request"
                | "response"
                | "end"
                | "failure"
                | "success"
                | "pending"
                | "failed"
                | "complete"
                | "before"
                | "after"
                | "body"
        ),
        "routeName" => matches!(
            value,
            "zrok"
                | "ngrok"
                | "local-browser"
                | "proxy-config"
                | "invalid-target"
                | "not-matched"
                | "computer"
                | "device"
                | "prepare"
                | "profile"
                | "authority"
                | "session"
                | "preferences"
                | "proxy"
                | "webview"
                | "ready"
        ),
        _ => false,
    };
    allowed.then(|| value.to_owned())
}

fn field(key: &str, value: &Value) -> Option<Value> {
    match key {
        "errorCode" => value
            .as_i64()
            .filter(|value| (-1000..=1000).contains(value))
            .map(Value::from),
        "durationMs" => value
            .as_f64()
            .filter(|number| {
                number.is_finite() && *number >= 0.0 && *number <= MAX_JSON_INTEGER as f64
            })
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number),
        "readyState" if value.is_number() => {
            value.as_u64().filter(|value| *value <= 4).map(Value::from)
        }
        "status" => value
            .as_u64()
            .filter(|value| *value == 0 || (100..=599).contains(value))
            .map(Value::from),
        "bytes" | "sdk" | "generation" | "expectedBytes" | "loadedBytes" | "versionCode"
        | "line" | "width" | "height" => integer(value),
        "visible" | "online" => value.as_bool().map(Value::Bool),
        _ => value
            .as_str()
            .and_then(|value| string_field(key, value))
            .map(Value::String),
    }
}

fn sanitize_bundle(value: &Value) -> Result<Value, ApiError> {
    if serde_json::to_vec(value)
        .map_err(|_| ApiError::new(400, "诊断数据格式无效"))?
        .len()
        > MAX_BUNDLE_BYTES
    {
        return Err(ApiError::new(413, "诊断报告超过 256 KiB"));
    }
    let captured_at =
        integer(&value["capturedAtMs"]).ok_or_else(|| ApiError::new(400, "诊断采集时间无效"))?;
    let events = value["events"]
        .as_array()
        .filter(|events| events.len() <= MAX_EVENTS)
        .ok_or_else(|| ApiError::new(400, "诊断事件最多 400 条"))?;
    let app = value["app"]
        .as_object()
        .ok_or_else(|| ApiError::new(400, "诊断应用信息无效"))?;
    if value["schemaVersion"].as_u64() != Some(1) {
        return Err(ApiError::new(400, "不支持的诊断数据版本"));
    }
    let app = app
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "versionName"
                    | "versionCode"
                    | "sdk"
                    | "webViewVersion"
                    | "resourceVersion"
                    | "originHost"
                    | "generation"
            )
        })
        .filter_map(|(key, value)| field(key, value).map(|value| (key.clone(), value)))
        .collect::<serde_json::Map<_, _>>();
    let events = events
        .iter()
        .map(|event| {
            let name = event["name"]
                .as_str()
                .filter(|name| valid_identifier(name, 64, true))
                .ok_or_else(|| ApiError::new(400, "诊断事件名称无效"))?;
            let source = event["source"]
                .as_str()
                .filter(|source| matches!(*source, "web" | "native"))
                .ok_or_else(|| ApiError::new(400, "诊断事件来源无效"))?;
            let time =
                integer(&event["timeMs"]).ok_or_else(|| ApiError::new(400, "诊断事件时间无效"))?;
            let fields = event["fields"]
                .as_object()
                .ok_or_else(|| ApiError::new(400, "诊断事件字段无效"))?;
            let fields = fields
                .iter()
                .filter(|(key, _)| {
                    matches!(
                        key.as_str(),
                        "host"
                            | "path"
                            | "method"
                            | "status"
                            | "durationMs"
                            | "bytes"
                            | "mime"
                            | "errorType"
                            | "errorCode"
                            | "reason"
                            | "visible"
                            | "online"
                            | "version"
                            | "resourceVersion"
                            | "sdk"
                            | "webViewVersion"
                            | "generation"
                            | "routeName"
                            | "readyState"
                            | "expectedBytes"
                            | "loadedBytes"
                            | "phase"
                            | "imageKind"
                            | "requestId"
                            | "network"
                            | "line"
                            | "width"
                            | "height"
                    )
                })
                .filter_map(|(key, value)| field(key, value).map(|value| (key.clone(), value)))
                .collect::<serde_json::Map<_, _>>();
            Ok(json!({"timeMs":time,"source":source,"name":name,"fields":fields}))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let bundle = json!({"schemaVersion":1,"capturedAtMs":captured_at,"app":app,"events":events});
    if serde_json::to_vec(&bundle)
        .map_err(|_| ApiError::new(400, "诊断数据格式无效"))?
        .len()
        > MAX_BUNDLE_BYTES
    {
        return Err(ApiError::new(413, "诊断报告超过 256 KiB"));
    }
    Ok(bundle)
}

fn report_path(directory: &FsPath, device_id: &str) -> PathBuf {
    directory.join(format!("{}.json", hash(device_id)))
}

fn read_report(directory: &FsPath, device_id: &str) -> anyhow::Result<Option<Report>> {
    let path = report_path(directory, device_id);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= (MAX_BUNDLE_BYTES + 4096) as u64,
        "诊断报告文件无效"
    );
    let mut report: Report = serde_json::from_slice(&std::fs::read(path)?)?;
    anyhow::ensure!(
        report.device_id == device_id && valid_identifier(&report.request_id, 80, false),
        "诊断报告设备信息无效"
    );
    report.bundle = sanitize_bundle(&report.bundle)?;
    Ok(Some(report))
}

fn prune_reports(directory: &FsPath, active: &[String]) -> anyhow::Result<()> {
    std::fs::create_dir_all(directory)?;
    let allowed = active
        .iter()
        .take(MAX_DEVICES)
        .map(|id| format!("{}.json", hash(id)))
        .collect::<HashSet<_>>();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Only clean files owned by this module.
        let owned = name
            .strip_suffix(".json")
            .or_else(|| name.strip_suffix(".json.tmp"));
        if owned.is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
            && !allowed.contains(name.as_ref())
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

impl RemoteAccessController {
    pub(super) async fn diagnostics_admin(&self, payload: Value) -> anyhow::Result<Value> {
        self.reload_access_key().await?;
        let auth = self.auth.lock().await;
        let active = auth
            .devices
            .iter()
            .filter(|device| {
                device.expires_at_ms > now_ms()
                    && auth
                        .access_key
                        .as_ref()
                        .is_some_and(|key| key.id == device.key_id)
            })
            .take(MAX_DEVICES)
            .map(|device| (device.id.clone(), device.name.clone()))
            .collect::<Vec<_>>();
        let active_ids = active.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let mut diagnostics = self.diagnostics.state.lock().await;
        diagnostics
            .pending
            .retain(|id, pending| active_ids.contains(id) && pending.expires_at_ms > now_ms());
        let directory = self.state_dir.join("remote-client-diagnostics");
        let mode = payload["mode"].as_str().context("缺少诊断操作模式")?;
        if mode == "request" {
            let device_id = payload["deviceId"].as_str().context("缺少设备 ID")?;
            anyhow::ensure!(
                active_ids.iter().any(|id| id == device_id),
                "设备授权已撤销或过期"
            );
            let reused = diagnostics.pending.contains_key(device_id);
            let pending = diagnostics
                .pending
                .entry(device_id.to_owned())
                .or_insert_with(|| Pending {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expires_at_ms: now_ms() + REQUEST_LIFETIME_MS,
                })
                .clone();
            if !reused {
                self.publish_event(json!({
                    "method":"remote/diagnosticsRequested",
                    "params":{"deviceId":device_id,"requestId":pending.request_id,"expiresAtMs":pending.expires_at_ms}
                }));
            }
            return Ok(json!({
                "status":"ok","deviceId":device_id,"requestId":pending.request_id,
                "expiresAtMs":pending.expires_at_ms,"pending":true,"reused":reused
            }));
        }
        anyhow::ensure!(matches!(mode, "list" | "read"), "不支持的诊断操作模式");
        let requested_device = if mode == "read" {
            let id = payload["deviceId"]
                .as_str()
                .context("缺少设备 ID")?
                .to_owned();
            anyhow::ensure!(active_ids.contains(&id), "设备授权已撤销或过期");
            Some(id)
        } else {
            None
        };
        let read_ids = requested_device.iter().cloned().collect::<Vec<_>>();
        let read_ids = if mode == "list" {
            active_ids.clone()
        } else {
            read_ids
        };
        let reports = tokio::task::spawn_blocking(move || {
            prune_reports(&directory, &active_ids)?;
            Ok::<_, anyhow::Error>(
                read_ids
                    .into_iter()
                    .map(|id| {
                        // A corrupt old report must not hide the other devices.
                        let report = read_report(&directory, &id).ok().flatten();
                        (id, report)
                    })
                    .collect::<HashMap<_, _>>(),
            )
        })
        .await??;
        if let Some(device_id) = requested_device {
            return Ok(json!({
                "status":"ok","deviceId":device_id,"report":reports.get(&device_id).and_then(Option::as_ref),
                "pending":diagnostics.pending.get(&device_id)
            }));
        }
        Ok(json!({
            "status":"ok",
            "devices":active.iter().map(|(id,name)| json!({
                "deviceId":id,"name":name,
                "report":reports.get(id).and_then(Option::as_ref).map(Report::metadata),
                "pending":diagnostics.pending.get(id),
            })).collect::<Vec<_>>()
        }))
    }
}

pub(super) async fn upload(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    use crate::bridge::CdpDispatchPermit;

    let device = authorized(&state, &headers).await?;
    // Media credentials never carry diagnostics, even if this handler is reused directly.
    if channels::media_authorization(&headers) {
        return Err(ApiError::new(403, "媒体凭证不支持诊断上传"));
    }
    let request_id = payload["requestId"]
        .as_str()
        .filter(|id| valid_identifier(id, 80, false))
        .ok_or_else(|| ApiError::new(400, "诊断请求 ID 无效"))?
        .to_owned();
    let bundle = sanitize_bundle(&payload["bundle"])?;
    let authorization = state.controller.authorization(device.clone(), None);
    let permit = authorization
        .prepare_dispatch()
        .await
        .map_err(authorization_error)?;
    let active = permit
        ._auth
        .devices
        .iter()
        .filter(|current| current.expires_at_ms > now_ms() && current.key_id == device.key_id)
        .take(MAX_DEVICES)
        .map(|device| device.id.clone())
        .collect::<Vec<_>>();
    let mut diagnostics = state.controller.diagnostics.state.lock().await;
    diagnostics
        .pending
        .retain(|id, pending| active.contains(id) && pending.expires_at_ms > now_ms());
    let requested = diagnostics
        .pending
        .get(&device.id)
        .is_some_and(|pending| pending.request_id == request_id);
    let report = Report {
        device_id: device.id.clone(),
        request_id: request_id.clone(),
        received_at_ms: now_ms(),
        bundle,
    };
    let directory = state.controller.state_dir.join("remote-client-diagnostics");
    let received_at_ms = report.received_at_ms;
    tokio::task::spawn_blocking(move || {
        permit.commit(|| {
            if !requested
                && read_report(&directory, &report.device_id)
                    .ok()
                    .flatten()
                    .is_some_and(|previous| {
                        report
                            .received_at_ms
                            .saturating_sub(previous.received_at_ms)
                            < MIN_UPLOAD_INTERVAL_MS
                    })
            {
                return Err(ApiError::new(429, "诊断上传过于频繁，请稍后再试").into());
            }
            prune_reports(&directory, &active)?;
            crate::settings::atomic_write(
                &report_path(&directory, &report.device_id),
                &serde_json::to_vec(&report)?,
            )?;
            Ok(())
        })
    })
    .await
    .map_err(|_| ApiError::new(500, "诊断报告保存失败"))?
    .map_err(|error| {
        error
            .downcast_ref::<ApiError>()
            .cloned()
            .unwrap_or_else(|| ApiError::new(500, "诊断报告保存失败"))
    })?;
    if requested {
        diagnostics.pending.remove(&device.id);
    }
    Ok(Json(
        json!({"ok":true,"requestId":request_id,"receivedAtMs":received_at_ms}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OfflineBridge;

    #[async_trait]
    impl NativeRemoteBridge for OfflineBridge {
        async fn call(&self, _action: &str, _payload: Value) -> anyhow::Result<Value> {
            anyhow::bail!("诊断测试不调用桌面桥接")
        }
    }

    fn bundle() -> Value {
        json!({
            "schemaVersion":1,"capturedAtMs":123,
            "app":{"versionName":"0.0.10","versionCode":10,"sdk":35,
                "webViewVersion":"131.0.0","resourceVersion":"abc123",
                "originHost":"https://user:password@mobile.example:443/home?token=secret#fragment",
                "generation":2,"token":"secret","localPath":"C:\\private\\file"},
            "events":[{"timeMs":123,"source":"web","name":"http.failed",
                "fields":{"host":"https://user:secret@mobile.example/?token=private",
                    "path":"/api/read/history?token=private#secret",
                    "method":"POST","status":502,"durationMs":1.25,"bytes":42,
                    "reason":"response-read-failed","errorType":"TypeError",
                    "line":12,"width":50,"height":80,
                    "Cookie":"secret","token":"private","body":"private",
                    "message":"Error: secret","stack":"private",
                    "phase":"complete","routeName":"zrok","online":true}}]
        })
    }

    fn test_state() -> (tempfile::TempDir, WebState, HeaderMap, HeaderMap) {
        let temp_root = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&temp_root).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("remote-diagnostics-")
            .tempdir_in(temp_root)
            .unwrap();
        configure_access_key(directory.path(), "isolated-diagnostics-key").unwrap();
        let controller =
            RemoteAccessController::new(Arc::new(OfflineBridge), directory.path().to_owned())
                .unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            ..Default::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        let mut auth = controller.auth.try_lock().unwrap();
        let key_id = auth.access_key.as_ref().unwrap().id.clone();
        let mut headers = Vec::new();
        for (id, token) in [
            ("device-a", "diagnostics-token-a"),
            ("device-b", "diagnostics-token-b"),
        ] {
            auth.devices.push(Device {
                id: id.into(),
                name: id.into(),
                token_hash: hash(token),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 1_000_000,
                last_seen_at_ms: now_ms(),
                key_id: key_id.clone(),
                public_origin: None,
            });
            let mut header = HeaderMap::new();
            header.insert(
                header::COOKIE,
                HeaderValue::from_str(&format!("{COOKIE_NAME}={token}")).unwrap(),
            );
            headers.push(header);
        }
        drop(auth);
        (
            directory,
            WebState {
                controller,
                settings: RemoteAccessSettings::default(),
            },
            headers.remove(0),
            headers.remove(0),
        )
    }

    #[test]
    fn diagnostics_sanitizes_fields_and_enforces_schema_size_and_event_limits() {
        let clean = sanitize_bundle(&bundle()).unwrap();
        assert_eq!(clean["app"]["originHost"], "mobile.example");
        assert_eq!(clean["events"][0]["fields"]["host"], "mobile.example");
        assert_eq!(clean["events"][0]["fields"]["path"], "/api/read/history");
        assert_eq!(clean["events"][0]["fields"]["durationMs"], 1.25);
        assert_eq!(clean["events"][0]["fields"]["width"], 50);
        let mut stage = bundle();
        stage["events"][0]["fields"]["routeName"] = json!("session");
        assert_eq!(
            sanitize_bundle(&stage).unwrap()["events"][0]["fields"]["routeName"],
            "session"
        );
        let text = serde_json::to_string(&clean).unwrap();
        for private in [
            "secret",
            "private",
            "password",
            "Cookie",
            "token",
            "localPath",
            "stack",
        ] {
            assert!(
                !text.contains(private),
                "诊断报告包含未允许的数据: {private}"
            );
        }
        assert_eq!(sanitize_bundle(&clean).unwrap(), clean);
        let mut invalid = bundle();
        invalid["events"][0]["name"] = json!("bad event");
        assert_eq!(sanitize_bundle(&invalid).unwrap_err().code, 400);
        invalid = bundle();
        invalid["events"] = json!(vec![invalid["events"][0].clone(); MAX_EVENTS + 1]);
        assert_eq!(sanitize_bundle(&invalid).unwrap_err().code, 400);
        invalid = bundle();
        invalid["unknown"] = json!("x".repeat(MAX_BUNDLE_BYTES));
        assert_eq!(sanitize_bundle(&invalid).unwrap_err().code, 413);
        invalid = bundle();
        invalid["events"][0]["fields"]["reason"] = json!("raw_error_with_secret");
        invalid["events"][0]["fields"]["path"] = json!("C:\\private\\image.png");
        invalid["events"][0]["fields"]["bytes"] = json!(-1);
        let clean = sanitize_bundle(&invalid).unwrap();
        assert!(clean["events"][0]["fields"].get("reason").is_none());
        assert!(clean["events"][0]["fields"].get("path").is_none());
        assert!(clean["events"][0]["fields"].get("bytes").is_none());
        assert_eq!(
            path("/api/images/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/thumbnail?channel_token=secret"),
            Some("/api/images/:id/thumbnail".into())
        );
    }

    #[tokio::test]
    async fn diagnostics_request_upload_read_rate_limit_expiry_and_revocation() {
        let (directory, state, headers_a, headers_b) = test_state();
        let controller = &state.controller;
        let mut events = controller.events.subscribe();
        let first = controller
            .admin(
                "diagnostics",
                json!({"mode":"request","deviceId":"device-b"}),
            )
            .await
            .unwrap();
        let repeated = controller
            .admin(
                "diagnostics",
                json!({"mode":"request","deviceId":"device-b"}),
            )
            .await
            .unwrap();
        assert_eq!(first["requestId"], repeated["requestId"]);
        assert_eq!(repeated["reused"], true);
        assert_eq!(
            controller
                .diagnostics
                .pending_for("device-b")
                .await
                .unwrap()["requestId"],
            first["requestId"]
        );
        assert!(
            controller
                .diagnostics
                .pending_for("device-a")
                .await
                .is_none()
        );
        assert_eq!(
            events.recv().await.unwrap()["method"],
            "remote/diagnosticsRequested"
        );
        assert!(events.try_recv().is_err());
        assert!(
            controller
                .admin(
                    "diagnostics",
                    json!({"mode":"request","deviceId":"unknown"})
                )
                .await
                .is_err()
        );
        let response = upload(
            State(state.clone()),
            headers_a.clone(),
            Json(json!({
                "requestId":first["requestId"],"deviceId":"device-b","bundle":bundle()
            })),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        let read_a = controller
            .admin("diagnostics", json!({"mode":"read","deviceId":"device-a"}))
            .await
            .unwrap();
        assert_eq!(read_a["report"]["deviceId"], "device-a");
        let read_b = controller
            .admin("diagnostics", json!({"mode":"read","deviceId":"device-b"}))
            .await
            .unwrap();
        assert!(read_b["report"].is_null());
        assert_eq!(read_b["pending"]["requestId"], first["requestId"]);
        let rate = upload(
            State(state.clone()),
            headers_a.clone(),
            Json(json!({"requestId":"manual-2","bundle":bundle()})),
        )
        .await
        .unwrap_err();
        assert_eq!(rate.code, 429);
        let requested_a = controller
            .admin(
                "diagnostics",
                json!({"mode":"request","deviceId":"device-a"}),
            )
            .await
            .unwrap();
        let _ = upload(
            State(state.clone()),
            headers_a.clone(),
            Json(json!({"requestId":requested_a["requestId"],"bundle":bundle()})),
        )
        .await
        .unwrap();
        let read_a = controller
            .admin("diagnostics", json!({"mode":"read","deviceId":"device-a"}))
            .await
            .unwrap();
        assert!(read_a["pending"].is_null());
        assert!(
            controller
                .diagnostics
                .pending_for("device-a")
                .await
                .is_none()
        );
        assert_eq!(read_a["report"]["requestId"], requested_a["requestId"]);
        controller
            .diagnostics
            .state
            .lock()
            .await
            .pending
            .get_mut("device-b")
            .unwrap()
            .expires_at_ms = 0;
        let expired = controller
            .admin("diagnostics", json!({"mode":"list"}))
            .await
            .unwrap();
        assert!(
            expired["devices"]
                .as_array()
                .unwrap()
                .iter()
                .find(|device| device["deviceId"] == "device-b")
                .unwrap()["pending"]
                .is_null()
        );
        let fresh_b = controller
            .admin(
                "diagnostics",
                json!({"mode":"request","deviceId":"device-b"}),
            )
            .await
            .unwrap();
        assert_ne!(fresh_b["requestId"], first["requestId"]);
        let _ = upload(
            State(state.clone()),
            headers_b.clone(),
            Json(json!({"requestId":fresh_b["requestId"],"bundle":bundle()})),
        )
        .await
        .unwrap();
        assert!(
            report_path(
                &directory.path().join("remote-client-diagnostics"),
                "device-b"
            )
            .is_file()
        );
        controller
            .admin("revoke", json!({"deviceId":"device-b"}))
            .await
            .unwrap();
        assert_eq!(
            upload(
                State(state.clone()),
                headers_b,
                Json(json!({"requestId":"revoked","bundle":bundle()}))
            )
            .await
            .unwrap_err()
            .code,
            401
        );
        assert!(
            controller
                .admin("diagnostics", json!({"mode":"read","deviceId":"device-b"}))
                .await
                .is_err()
        );
        let listed = controller
            .admin("diagnostics", json!({"mode":"list"}))
            .await
            .unwrap();
        assert_eq!(listed["devices"].as_array().unwrap().len(), 1);
        assert!(
            !report_path(
                &directory.path().join("remote-client-diagnostics"),
                "device-b"
            )
            .exists()
        );
    }

    #[tokio::test]
    async fn diagnostics_http_preserves_auth_origin_body_and_media_ticket_boundaries() {
        let (_directory, state, headers, _) = test_state();
        let controller = state.controller.clone();
        *controller.public_origin.write().unwrap() =
            Some("https://diagnostics.shares.zrok.io".into());
        *controller.channels.secondary.write().unwrap() =
            Some("https://diagnostics.ngrok-free.app".into());
        let discovery = channels::discover(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = web_router(
            controller,
            RemoteAccessSettings {
                enabled: true,
                port: address.port(),
                ..Default::default()
            },
        );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{address}/api/diagnostics");
        for (token, origin, expected) in [
            (None, "https://diagnostics.ngrok-free.app", 401),
            (
                Some(format!(
                    "Media {}",
                    discovery["mediaToken"].as_str().unwrap()
                )),
                "https://diagnostics.shares.zrok.io",
                403,
            ),
            (
                Some(format!("Bearer {}", discovery["token"].as_str().unwrap())),
                "https://attacker.example",
                403,
            ),
        ] {
            let mut request = client
                .post(&url)
                .header("Host", "diagnostics.ngrok-free.app")
                .header("Origin", origin);
            if let Some(token) = token {
                request = request.header("Authorization", token);
            }
            // Rejected before JSON parsing or collecting an oversized body.
            let response = request.body("invalid json").send().await.unwrap();
            assert_eq!(response.status().as_u16(), expected);
        }
        let oversized = client
            .post(&url)
            .header("Host", "diagnostics.ngrok-free.app")
            .header("Origin", "https://diagnostics.shares.zrok.io")
            .bearer_auth(discovery["token"].as_str().unwrap())
            .header("Content-Type", "application/json")
            .body("x".repeat(MAX_BODY_BYTES + 1))
            .send()
            .await
            .unwrap();
        assert_eq!(oversized.status().as_u16(), 413);
        let response = client
            .post(&url)
            .header("Host", "diagnostics.ngrok-free.app")
            .header("Origin", "https://diagnostics.shares.zrok.io")
            .bearer_auth(discovery["token"].as_str().unwrap())
            .json(&json!({"requestId":"http-manual","bundle":bundle()}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "https://diagnostics.shares.zrok.io"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.json::<Value>().await.unwrap()["ok"], true);
        assert!(!channels::read_path(
            &axum::http::Method::POST,
            "/api/diagnostics"
        ));
        assert!(channels::request_path(
            &axum::http::Method::POST,
            "/api/diagnostics"
        ));
        server.abort();
    }
}
