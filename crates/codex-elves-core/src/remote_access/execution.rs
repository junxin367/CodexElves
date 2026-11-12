//! 远程执行路径的约束与诊断；不记录提示词、附件或授权凭据。
use std::collections::{BTreeSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use super::RemoteAuthorization;

// 只保留最近 256 条请求元数据，限制常驻内存；完整写操作轨迹仍进入已有轮转日志。
const MAX_RECORDS: usize = 256;
static REQUIRE_EXPLICIT_CDP: AtomicBool = AtomicBool::new(false);
static PROCESS_CDP_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PROCESS_BLOCKED_CDP_ATTEMPTS: AtomicU64 = AtomicU64::new(0);

fn queue_steer_diagnostic(value: &Value) -> Option<Value> {
    if value["version"] != 1
        || value["stage"] != "prepared-contract"
        || !matches!(value["phase"].as_str(), Some("start" | "steer" | "unknown"))
        || !matches!(
            value["adapter"].as_str(),
            Some("request-client" | "manager")
        )
        || !matches!(value["queueKind"].as_str(), Some("server" | "local"))
    {
        return None;
    }
    // 用户要求引导失败能关联请求并定位校验项；与 native.js 同步固定字段，只持久化布尔值和数量，拒绝透传正文及凭据。
    let mut result = json!({
        "version":1,"stage":"prepared-contract","phase":value["phase"],
        "adapter":value["adapter"],"queueKind":value["queueKind"],"checks":{},"inputs":{},"restores":{}
    });
    for key in [
        "conversationId",
        "resume.conversationId",
        "steer.clientUserMessageId",
        "steer.restoreMessage",
        "steer.input",
        "steer.expectedTurnId",
        "steer.targetTurnId",
        "steer.serverInput",
        "start.required",
        "start.threadId",
        "start.clientUserMessageId",
        "start.input",
        "start.restoreMessage",
        "admissionStart.threadId",
        "admissionStart.clientUserMessageId",
        "admissionStart.input",
        "admissionStart.restoreMessage",
    ] {
        if let Some(flag) = value["checks"][key].as_bool() {
            result["checks"][key] = json!(flag);
        }
    }
    if result["checks"].as_object()?.is_empty() {
        return None;
    }
    for group in ["inputs", "restores"] {
        for role in ["steer", "server", "start", "admissionStart"] {
            let item = &value[group][role];
            if !item.is_object() || group == "restores" && role == "server" {
                continue;
            }
            let mut summary = json!({});
            if group == "restores" {
                for key in [
                    "present",
                    "idMatches",
                    "clientIdMatches",
                    "serverIdMatches",
                    "textMatches",
                    "contextMatches",
                    "promptMatches",
                    "turnTriggerMatches",
                ] {
                    if let Some(flag) = item[key].as_bool() {
                        summary[key] = json!(flag);
                    }
                }
            }
            let counts: &[&str] = if group == "inputs" {
                &["items", "text", "image", "localImage", "skill", "other"]
            } else {
                &["expectedFieldCount", "actualFieldCount"]
            };
            for key in counts {
                if let Some(count) = item[*key].as_u64() {
                    summary[*key] = json!(count);
                }
            }
            result[group][role] = summary;
        }
    }
    Some(result)
}

tokio::task_local! {
    static CURRENT: Scope;
}

#[derive(Default)]
pub(super) struct Audit {
    service_cdp_attempts: AtomicU64,
    records: Mutex<VecDeque<Value>>,
}

impl Audit {
    pub fn snapshot(&self) -> Value {
        json!({
            "serviceCdpAttempts":self.service_cdp_attempts.load(Ordering::Relaxed),
            "processCdpEntryAttempts":PROCESS_CDP_ATTEMPTS.load(Ordering::Relaxed),
            "processBlockedCdpAttempts":PROCESS_BLOCKED_CDP_ATTEMPTS.load(Ordering::Relaxed),
            "cdpDisabled":cdp_disabled(),
            "records":self.records.lock().unwrap_or_else(|e| e.into_inner()).clone()
        })
    }

    fn record(&self, value: Value) {
        let mut records = self.records.lock().unwrap_or_else(|e| e.into_inner());
        records.push_back(value);
        while records.len() > MAX_RECORDS {
            records.pop_front();
        }
    }
}

#[derive(Default)]
struct Progress {
    transports: BTreeSet<&'static str>,
    transport: Option<&'static str>,
    write_transport: Option<&'static str>,
    phase: String,
}

#[derive(Clone)]
pub(super) struct Scope {
    service_only: bool,
    audit: Arc<Audit>,
    metadata: Value,
    progress: Arc<Mutex<Progress>>,
}

impl Scope {
    pub fn service(
        audit: Arc<Audit>,
        action: &str,
        payload: &Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> Self {
        if let Ok(scope) = CURRENT.try_with(Clone::clone)
            && scope.service_only
            && Arc::ptr_eq(&scope.audit, &audit)
        {
            return scope;
        }
        Self::new(audit, true, action, payload, authorization, None)
    }

    pub fn new(
        audit: Arc<Audit>,
        service_only: bool,
        action: &str,
        payload: &Value,
        authorization: Option<&RemoteAuthorization>,
        revision: Option<u64>,
    ) -> Self {
        // 用户要求派生任务不能解除服务模式约束；嵌套作用域只能收紧，不能重新允许 CDP。
        let service_only = service_only
            || CURRENT
                .try_with(|scope| scope.service_only)
                .unwrap_or(false);
        let request_id = authorization
            .and_then(|a| a.request_id.clone())
            .or_else(|| {
                CURRENT
                    .try_with(|scope| scope.metadata["requestId"].as_str().map(str::to_owned))
                    .ok()
                    .flatten()
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            service_only,
            audit,
            metadata: json!({
                "requestId":request_id,"action":action,
                "deviceId":authorization.map(|a| a.device.id.as_str()),
                "threadId":payload["threadId"].as_str().filter(|id| id.len() <= 128
                    && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))),
                "runtimeRevision":revision,
                "mode":if service_only {"service"} else {"cdp"}
            }),
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }

    pub fn run<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        // 将业务 Future 放到堆上，避免多层权限作用域放大 Windows 工作线程的栈占用。
        let future = Box::pin(future);
        async move { CURRENT.scope(self.inherit_policy(), future).await }
    }

    fn inherit_policy(mut self) -> Self {
        // Future 可以在外部构造后再进入服务作用域，真正执行时必须再次继承禁止 CDP 的约束。
        if CURRENT
            .try_with(|scope| scope.service_only)
            .unwrap_or(false)
        {
            self.service_only = true;
            self.metadata["mode"] = json!("service");
        }
        self
    }

    pub fn recorded<F>(self, future: F) -> impl Future<Output = anyhow::Result<Value>>
    where
        F: Future<Output = anyhow::Result<Value>>,
    {
        let future = Box::pin(future);
        async move {
            let scope = self.inherit_policy();
            let started = Instant::now();
            let mut result = scope.clone().run(future).await;
            let mut record = scope.metadata.clone();
            let progress = scope.progress.lock().unwrap_or_else(|e| e.into_inner());
            record["executionTransport"] = json!(progress.write_transport.or(progress.transport));
            record["transports"] = json!(progress.transports);
            record["phase"] = json!(progress.phase);
            record["durationMs"] = json!(started.elapsed().as_millis() as u64);
            record["timeMs"] = json!(super::now_ms());
            record["outcome"] = json!(match &result {
                Ok(value) if value["outcomeUnknown"] == true => "unknown",
                Ok(value) if value["deliveryState"] == "queued" => "queued",
                Ok(value) if value["ok"] == false => "rejected",
                Ok(_) => "completed",
                Err(_) => "failed",
            });
            if scope.metadata["action"] == "steerQueued"
                && let Ok(value) = &result
                && value["ok"] == false
                && value["notSent"] == true
                && let Some(detail) = queue_steer_diagnostic(&value["queueSteerDiagnostic"])
            {
                // 复用本请求的 requestId、threadId、设备和通道，服务重启后仍可从现有轮转日志追踪失败校验。
                record["queueSteerDiagnostic"] = detail;
            }
            if let Ok(value) = &mut result
                && value.is_object()
                && !record["executionTransport"].is_null()
                && value.get("executionTransport").is_none()
            {
                value["executionTransport"] = record["executionTransport"].clone();
            }
            // 每次业务请求保存最终通道；流式轮询不逐次写文件，避免诊断挤占事件处理。
            if scope.metadata["action"] != "events" {
                scope.audit.record(record.clone());
                let _ =
                    crate::diagnostic_log::append_diagnostic_log("remote_access.execution", record);
            }
            result
        }
    }
}

pub(crate) fn transport(name: &'static str, phase: &str, write: bool) {
    let _ = CURRENT.try_with(|scope| {
        let mut progress = scope.progress.lock().unwrap_or_else(|e| e.into_inner());
        progress.transports.insert(name);
        progress.transport = Some(name);
        if write {
            progress.write_transport = Some(name);
        }
        progress.phase = phase.to_owned();
    });
}

pub(super) fn runtime_revision() -> Option<u64> {
    CURRENT
        .try_with(|scope| scope.metadata["runtimeRevision"].as_u64())
        .ok()
        .flatten()
}

// 用户要求后台队列、目录预热和事件监听也禁止借用 CDP；Tokio 新任务必须显式绑定约束。
pub(super) fn spawn_service<F>(
    audit: Arc<Audit>,
    action: &'static str,
    future: F,
) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let scope = Scope::new(audit, true, action, &json!({}), None, None);
    tokio::spawn(scope.run(future))
}

pub(super) fn spawn_inherited<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let scope = CURRENT.try_with(Clone::clone).unwrap_or_else(|_| {
        Scope::new(
            Arc::new(Audit::default()),
            true,
            "background",
            &json!({}),
            None,
            None,
        )
    });
    tokio::spawn(scope.run(future))
}

pub(crate) fn require_explicit_cdp() {
    // 独立远程后台中，丢失请求上下文的新任务也不能偷偷建立 CDP 连接。
    REQUIRE_EXPLICIT_CDP.store(true, Ordering::Release);
}

pub(crate) fn cdp_disabled() -> bool {
    std::env::var_os("CODEX_ELVES_DISABLE_CDP").is_some_and(|value| value == "1")
}

#[derive(Debug)]
struct CdpForbidden(&'static str);

impl std::fmt::Display for CdpForbidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "当前执行路径禁止调用 CDP（{}）", self.0)
    }
}

impl std::error::Error for CdpForbidden {}

pub(super) fn is_cdp_forbidden(error: &anyhow::Error) -> bool {
    error.downcast_ref::<CdpForbidden>().is_some()
}

#[track_caller]
pub(crate) fn check_cdp_allowed(phase: &'static str) -> anyhow::Result<()> {
    PROCESS_CDP_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    let scope = CURRENT.try_with(Clone::clone).ok();
    let service_only = scope.as_ref().is_some_and(|scope| scope.service_only);
    if cdp_disabled()
        || service_only
        || scope.is_none() && REQUIRE_EXPLICIT_CDP.load(Ordering::Acquire)
    {
        PROCESS_BLOCKED_CDP_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let mut detail = scope.as_ref().map_or_else(
            || json!({"mode":"unscoped","origin":"remote-background"}),
            |scope| scope.metadata.clone(),
        );
        detail["phase"] = json!(phase);
        let location = std::panic::Location::caller();
        detail["location"] = json!(format!("{}:{}", location.file(), location.line()));
        detail["timeMs"] = json!(super::now_ms());
        detail["outcome"] = json!("blocked");
        if let Some(scope) = &scope {
            if service_only {
                scope
                    .audit
                    .service_cdp_attempts
                    .fetch_add(1, Ordering::Relaxed);
            }
            scope.audit.record(detail.clone());
        }
        let _ = crate::diagnostic_log::append_diagnostic_log("remote_access.cdp_forbidden", detail);
        return Err(CdpForbidden(phase).into());
    }
    transport("cdp", phase, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queue_steer_contract_diagnostic_is_correlated_and_redacted() {
        let directory = tempfile::tempdir().unwrap();
        let log_path = directory.path().join("queue-steer-diagnostic.log");
        struct ResetLogPath;
        impl Drop for ResetLogPath {
            fn drop(&mut self) {
                crate::diagnostic_log::set_diagnostic_log_path_for_tests(None);
            }
        }
        crate::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
        let _reset_log_path = ResetLogPath;
        let audit = Arc::new(Audit::default());
        let result = Scope::new(
            audit.clone(),
            false,
            "steerQueued",
            &json!({"threadId":"diagnostic-thread"}),
            None,
            Some(7),
        )
        .recorded(async {
            Ok(json!({
                "ok":false,"notSent":true,
                "queueSteerDiagnostic":{
                    "version":1,"stage":"prepared-contract","phase":"steer",
                    "adapter":"request-client","queueKind":"server",
                    "checks":{"steer.expectedTurnId":false,"steer.input":true,"private-secret":false},
                    "inputs":{"steer":{"items":2,"text":1,"image":1,"url":"private-secret"}},
                    "restores":{"steer":{"present":true,"promptMatches":false,"text":"private-secret",
                        "expectedFieldCount":8,"actualFieldCount":9}},
                    "prompt":"private-secret","token":"private-secret"
                }
            }))
        })
        .await
        .unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["notSent"], true);
        let records = audit.snapshot()["records"].as_array().unwrap().clone();
        let record = records.last().unwrap();
        assert_eq!(record["threadId"], "diagnostic-thread");
        assert!(
            record["requestId"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );
        assert_eq!(record["runtimeRevision"], 7);
        assert_eq!(record["outcome"], "rejected");
        assert_eq!(
            record["queueSteerDiagnostic"]["checks"]["steer.expectedTurnId"],
            false
        );
        assert_eq!(
            record["queueSteerDiagnostic"]["inputs"]["steer"]["image"],
            1
        );
        assert_eq!(
            record["queueSteerDiagnostic"]["restores"]["steer"]["promptMatches"],
            false
        );
        assert!(!record.to_string().contains("private-secret"));
        let saved = std::fs::read_to_string(log_path).unwrap();
        let saved: Value = serde_json::from_str(saved.trim()).unwrap();
        assert_eq!(saved["event"], "remote_access.execution");
        assert_eq!(saved["detail"], *record);
    }

    #[test]
    fn queue_steer_contract_diagnostic_rejects_unknown_schema_and_text_values() {
        assert!(queue_steer_diagnostic(&json!(null)).is_none());
        let mut value = json!({
            "version":1,"stage":"prepared-contract","phase":"steer",
            "adapter":"request-client","queueKind":"local",
            "checks":{"steer.input":"private-secret"}
        });
        assert!(queue_steer_diagnostic(&value).is_none());
        value["checks"]["steer.input"] = json!(false);
        value["phase"] = json!("private-secret");
        assert!(queue_steer_diagnostic(&value).is_none());
    }
}
