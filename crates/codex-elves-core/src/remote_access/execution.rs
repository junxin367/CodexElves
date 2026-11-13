//! 远程执行路径的约束与诊断；不记录提示词、附件或授权凭据。
use std::collections::{BTreeSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use super::RemoteAuthorization;

// 只保留最近 256 条请求元数据，限制常驻内存；完整写操作轨迹仍进入已有轮转日志。
const MAX_RECORDS: usize = 256;

tokio::task_local! {
    static CURRENT: Scope;
}

#[derive(Default)]
pub(super) struct Audit {
    records: Mutex<VecDeque<Value>>,
}

impl Audit {
    pub fn snapshot(&self) -> Value {
        json!({
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
            && Arc::ptr_eq(&scope.audit, &audit)
        {
            return scope;
        }
        Self::new(audit, action, payload, authorization)
    }

    pub fn new(
        audit: Arc<Audit>,
        action: &str,
        payload: &Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> Self {
        // 所有远程调用都记录在服务执行作用域中。
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
            audit,
            metadata: json!({
                "requestId":request_id,"action":action,
                "deviceId":authorization.map(|a| a.device.id.as_str()),
                "threadId":payload["threadId"].as_str().filter(|id| id.len() <= 128
                    && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))),
                "mode":"service"
            }),
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }

    pub fn run<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        // 将业务 Future 放到堆上，避免多层权限作用域放大 Windows 工作线程的栈占用。
        let future = Box::pin(future);
        async move { CURRENT.scope(self, future).await }
    }

    pub fn recorded<F>(self, future: F) -> impl Future<Output = anyhow::Result<Value>>
    where
        F: Future<Output = anyhow::Result<Value>>,
    {
        let future = Box::pin(future);
        async move {
            let scope = self;
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

// 后台队列、目录预热和事件监听显式绑定服务执行作用域。
pub(super) fn spawn_service<F>(
    audit: Arc<Audit>,
    action: &'static str,
    future: F,
) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let scope = Scope::new(audit, action, &json!({}), None);
    tokio::spawn(scope.run(future))
}

pub(super) fn spawn_inherited<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let scope = CURRENT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| Scope::new(Arc::new(Audit::default()), "background", &json!({}), None));
    tokio::spawn(scope.run(future))
}
