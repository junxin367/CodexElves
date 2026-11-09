use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{ApiError, Device, NATIVE_EVENT_FAILURE_GRACE, RemoteAccessController};

// 用户要求电脑端 CDP 不可用时自动切换：沿用 3 秒连续异常窗口，
// 单次 CDP 探测最多 5 秒，每次服务准备最多 8 秒；同一轮持续异常的尝试间隔至少 10 秒，避免反复启动。
pub(super) const CDP_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const SERVICE_PREPARE_TIMEOUT: Duration = Duration::from_secs(8);
const RETRY_DELAY: Duration = Duration::from_secs(10);
// 恢复需连续健康 5 秒，长于 3 秒故障窗口；恢复受任务阻塞时也按 10 秒间隔重试，避免抖动反复切换。
const CDP_RECOVERY_GRACE: Duration = Duration::from_secs(5);

pub(super) fn cdp_ready(response: &Value) -> bool {
    response["ok"] == true
        && response["events"].is_array()
        && response["inactive"] != true
        && response["writeAvailable"] != false
        && !matches!(
            response["nativeBridge"].as_str(),
            Some("service" | "desktop-owner-ipc" | "harness" | "read_only")
        )
}

#[derive(Default)]
pub(super) struct FallbackWatch {
    revisions: HashMap<String, u64>,
    unavailable_since: Option<Instant>,
    retry_after: Option<Instant>,
}

impl FallbackWatch {
    pub fn observe(&mut self, candidates: &[(Device, u64)], ready: bool, now: Instant) -> bool {
        let revisions = candidates
            .iter()
            .map(|(device, revision)| (device.id.clone(), *revision))
            .collect();
        // 手动切换或重新授权后重新观察，迟到的旧故障不能覆盖用户的新模式版本。
        if ready || candidates.is_empty() || self.revisions != revisions {
            self.revisions = revisions;
            self.unavailable_since = None;
            self.retry_after = None;
        }
        if ready || candidates.is_empty() {
            return false;
        }
        let started = *self.unavailable_since.get_or_insert(now);
        if now.duration_since(started) < NATIVE_EVENT_FAILURE_GRACE
            || self.retry_after.is_some_and(|until| now < until)
        {
            return false;
        }
        self.retry_after = Some(now + RETRY_DELAY);
        true
    }
}

#[derive(Default)]
pub(super) struct RecoveryWatch {
    revisions: HashMap<String, u64>,
    available_since: Option<Instant>,
    retry_after: Option<Instant>,
}

impl RecoveryWatch {
    pub fn observe(&mut self, candidates: &[(Device, u64)], ready: bool, now: Instant) -> bool {
        let revisions = candidates
            .iter()
            .map(|(device, revision)| (device.id.clone(), *revision))
            .collect();
        if !ready || candidates.is_empty() || self.revisions != revisions {
            self.revisions = revisions;
            self.available_since = None;
            self.retry_after = None;
        }
        if !ready || candidates.is_empty() {
            return false;
        }
        let started = *self.available_since.get_or_insert(now);
        if now.duration_since(started) < CDP_RECOVERY_GRACE
            || self.retry_after.is_some_and(|until| now < until)
        {
            return false;
        }
        self.retry_after = Some(now + RETRY_DELAY);
        true
    }
}

impl RemoteAccessController {
    pub(super) async fn prepare_automatic_cdp(&self) -> anyhow::Result<()> {
        if super::execution::cdp_disabled() {
            return Err(ApiError::new(409, "当前后台禁止 CDP，保留服务模式").into());
        }
        let payload = json!({"cursor":0,"epoch":""});
        self.native_started.store(true, Ordering::Release);
        // 自动恢复探测属于用户原先选择的 CDP 健康管理；服务模式的业务调用仍保持原有 CDP 硬隔离。
        let response = tokio::time::timeout(
            CDP_PROBE_TIMEOUT,
            super::execution::Scope::new(
                self.harness.audit.clone(),
                false,
                "events",
                &payload,
                None,
                None,
            )
            .recorded(self.native.call("events", payload)),
        )
        .await
        .map_err(|_| ApiError::new(503, "CDP 恢复探测超时，保留服务模式"))??;
        if !cdp_ready(&response) {
            return Err(ApiError::new(503, "CDP 尚未恢复可写，保留服务模式").into());
        }
        Ok(())
    }

    pub(super) async fn prepare_automatic_service(&self) -> anyhow::Result<()> {
        tokio::time::timeout(
            SERVICE_PREPARE_TIMEOUT,
            self.harness.prepare_auto_fallback(),
        )
        .await
        .map_err(|_| ApiError::new(503, "服务模式准备超时，保留当前运行方式并稍后重试"))?
    }

    pub(super) async fn cdp_fallback_candidates(&self) -> Vec<(Device, u64)> {
        if super::execution::cdp_disabled() {
            return Vec::new();
        }
        let devices: Vec<_> = self
            .auth
            .lock()
            .await
            .devices
            .iter()
            .filter(|device| device.expires_at_ms > super::now_ms())
            .cloned()
            .collect();
        let modes = self.runtime_modes.read().await;
        devices
            .into_iter()
            .filter_map(|device| {
                let mode = modes.get(&device.id).cloned().unwrap_or_default();
                mode.cdp_injection_enabled
                    .then_some((device, mode.revision))
            })
            .collect()
    }

    pub(super) async fn cdp_recovery_candidates(&self) -> Vec<(Device, u64)> {
        if super::execution::cdp_disabled() {
            return Vec::new();
        }
        let devices: Vec<_> = self
            .auth
            .lock()
            .await
            .devices
            .iter()
            .filter(|device| device.expires_at_ms > super::now_ms())
            .cloned()
            .collect();
        let modes = self.runtime_modes.read().await;
        devices
            .into_iter()
            .filter_map(|device| {
                let mode = modes.get(&device.id).cloned().unwrap_or_default();
                mode.awaits_automatic_cdp_recovery()
                    .then_some((device, mode.revision))
            })
            .collect()
    }

    pub(super) async fn automatically_use_cdp(
        self: &Arc<Self>,
        candidates: &[(Device, u64)],
    ) -> anyhow::Result<usize> {
        if super::execution::cdp_disabled() || !self.running.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let mut changed = 0;
        for (device, revision) in candidates {
            let mode = self.runtime_for(&device.id).await;
            if !mode.awaits_automatic_cdp_recovery() || mode.revision != *revision {
                continue;
            }
            let authorization = self.authorization(device.clone(), None);
            match self
                .set_runtime_mode(
                    json!({"cdpInjectionEnabled":true,"expectedRevision":revision}),
                    &authorization,
                    Some("cdp-recovered"),
                )
                .await
            {
                Ok(value) => {
                    changed += 1;
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "remote_access.runtime_automatic_recovery",
                        json!({"deviceId":authorization.device.id,"runtime":value["runtime"]}),
                    );
                }
                // 服务任务、未知送达队列、临时侧边和在途请求未结束时继续等待；手动改模式/撤销授权也不覆盖。
                Err(failure)
                    if failure
                        .downcast_ref::<ApiError>()
                        .is_some_and(|failure| matches!(failure.code, 401 | 409)) => {}
                Err(failure) => return Err(failure),
            }
        }
        Ok(changed)
    }

    pub(super) async fn automatically_use_service(
        self: &Arc<Self>,
        candidates: &[(Device, u64)],
    ) -> anyhow::Result<usize> {
        let mut current = Vec::new();
        for (device, revision) in candidates {
            let mode = self.runtime_for(&device.id).await;
            if mode.cdp_injection_enabled && mode.revision == *revision {
                current.push((device.clone(), *revision));
            }
        }
        if current.is_empty() || !self.running.load(Ordering::SeqCst) {
            return Ok(0);
        }
        self.prepare_automatic_service().await?;
        let mut changed = 0;
        for (device, revision) in current {
            // 自动路由恢复允许只读设备使用，但不改变只读/审批权限，也不重放任何业务请求。
            let authorization = self.authorization(device, None);
            match self
                .set_runtime_mode(
                    json!({"cdpInjectionEnabled":false,"expectedRevision":revision}),
                    &authorization,
                    Some("cdp-unavailable"),
                )
                .await
            {
                Ok(value) => {
                    changed += 1;
                    let _ = crate::diagnostic_log::append_diagnostic_log(
                        "remote_access.runtime_automatic_fallback",
                        json!({"deviceId":authorization.device.id,"runtime":value["runtime"]}),
                    );
                }
                Err(failure)
                    if failure
                        .downcast_ref::<ApiError>()
                        .is_some_and(|failure| matches!(failure.code, 401 | 409)) => {}
                Err(failure) => return Err(failure),
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Device {
        Device {
            id: "device".into(),
            name: "test".into(),
            token_hash: "test".into(),
            key_id: "key".into(),
            created_at_ms: 1,
            expires_at_ms: u64::MAX,
            last_seen_at_ms: 1,
            public_origin: None,
        }
    }

    #[test]
    fn runtime_fallback_waits_for_sustained_failure_and_resets_after_health_or_manual_revision() {
        let start = Instant::now();
        let mut watch = FallbackWatch::default();
        let mut candidates = vec![(device(), 0)];
        assert!(!watch.observe(&candidates, false, start));
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(2)));
        assert!(watch.observe(&candidates, false, start + Duration::from_secs(3)));
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(4)));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(5)));
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(6)));
        candidates[0].1 = 1;
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(10)));
        assert!(watch.observe(&candidates, false, start + Duration::from_secs(13)));
        assert!(!watch.observe(&[], false, start + Duration::from_secs(20)));
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(21)));
    }

    #[test]
    fn runtime_fallback_detects_read_only_fallback_without_misclassifying_native_success() {
        assert!(cdp_ready(&json!({"ok":true,"events":[]})));
        assert!(cdp_ready(
            &json!({"ok":true,"events":[],"nativeBridge":"cdp","writeAvailable":true})
        ));
        for value in [
            json!({"ok":false}),
            json!({"ok":true,"events":{}}),
            json!({"ok":true,"inactive":true}),
            json!({"ok":true,"writeAvailable":false}),
            json!({"ok":true,"nativeBridge":"service"}),
            json!({"ok":true,"nativeBridge":"desktop-owner-ipc"}),
            json!({"ok":true,"nativeBridge":"read_only"}),
        ] {
            assert!(!cdp_ready(&value));
        }
    }

    #[test]
    fn runtime_recovery_requires_sustained_health_and_resets_after_failure_or_manual_revision() {
        let start = Instant::now();
        let mut watch = RecoveryWatch::default();
        let mut candidates = vec![(device(), 3)];
        assert!(!watch.observe(&candidates, true, start));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(4)));
        assert!(watch.observe(&candidates, true, start + Duration::from_secs(5)));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(6)));
        assert!(watch.observe(&candidates, true, start + Duration::from_secs(15)));
        assert!(!watch.observe(&candidates, false, start + Duration::from_secs(16)));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(17)));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(21)));
        assert!(watch.observe(&candidates, true, start + Duration::from_secs(22)));
        candidates[0].1 = 4;
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(23)));
        assert!(!watch.observe(&[], true, start + Duration::from_secs(29)));
        assert!(!watch.observe(&candidates, true, start + Duration::from_secs(30)));
    }
}
