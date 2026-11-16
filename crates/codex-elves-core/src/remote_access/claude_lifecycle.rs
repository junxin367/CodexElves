//! 退出时回收受管 Claude 会话；启动仅补偿未确认的回收，不扫描或终止其他原生会话。
use super::*;
use fs2::FileExt;
use futures_util::{StreamExt, stream};
use std::fs::OpenOptions;
use tokio::io::AsyncReadExt;

const CLEANUP_FILE: &str = "remote-claude-cleanup.json";
const CLEANUP_LOCK: &str = "remote-claude-cleanup.lock";
// 用户要求退出回收为主、启动补偿；四个并发探针避免大量历史会话同时启动 WSL。
// 总预算 55 秒，给 service.rs 的 60 秒退出预算留出持久化及关闭连接的时间。
const CLEANUP_BUDGET: Duration = Duration::from_secs(55);
const RETRY_INTERVAL_MS: u64 = 15_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Intent {
    thread_id: String,
    native_id: String,
    job_id: String,
    config: WslConfig,
    generation: String,
    requested_at: u64,
    checked_at: u64,
    reason: String,
    result: Value,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct State {
    intents: BTreeMap<String, Intent>,
    stopped: BTreeMap<String, String>,
    runtime_active: Option<bool>,
    last_report: Value,
}

fn identity(native_id: &str, job_id: &str, config: &WslConfig) -> String {
    serde_json::to_string(&(native_id, job_id, config)).unwrap_or_default()
}

pub(super) struct CleanupStore(PathBuf);

impl CleanupStore {
    pub(super) fn new(directory: &Path) -> Self {
        Self(directory.to_owned())
    }

    fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> anyhow::Result<T> {
        std::fs::create_dir_all(&self.0)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.0.join(CLEANUP_LOCK))?;
        lock.lock_exclusive()?;
        let path = self.0.join(CLEANUP_FILE);
        let mut state: State = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Claude 待回收记录损坏，未覆盖原文件")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(error) => return Err(error.into()),
        };
        let before = serde_json::to_vec(&state)?;
        let result = change(&mut state);
        let bytes = serde_json::to_vec(&state)?;
        if bytes != before {
            anyhow::ensure!(bytes.len() <= 4 * 1024 * 1024, "Claude 待回收记录过大");
            crate::settings::atomic_write(&path, &bytes)?;
        }
        Ok(result)
    }

    pub(super) fn schedule(&self, saved: &Saved, reason: &str) -> anyhow::Result<()> {
        self.update(|state| {
            for c in saved.conversations.values().filter(|c| {
                c.backend == "claude"
                    && Claude::owns_id(&c.id)
                    && c.shared
                    && !c.deleted
                    && !c.discovered
                    && !c.native_job_id.is_empty()
                    && uuid::Uuid::parse_str(&c.native_id).is_ok()
                    && c.config.validate().is_ok()
            }) {
                if state.stopped.get(&c.id)
                    == Some(&identity(&c.native_id, &c.native_job_id, &c.config))
                {
                    continue;
                }
                if let Some(intent) = state.intents.get_mut(&c.id).filter(|intent| {
                    intent.native_id == c.native_id
                        && intent.job_id == c.native_job_id
                        && intent.config == c.config
                }) {
                    if intent.reason != reason {
                        intent.reason = reason.into();
                        intent.checked_at = 0;
                    }
                    continue;
                }
                state.intents.insert(
                    c.id.clone(),
                    Intent {
                        thread_id: c.id.clone(),
                        native_id: c.native_id.clone(),
                        job_id: c.native_job_id.clone(),
                        config: c.config.clone(),
                        generation: uuid::Uuid::new_v4().to_string(),
                        requested_at: now_ms(),
                        checked_at: 0,
                        reason: reason.into(),
                        result: json!({"stopped":false,"reason":"not_checked"}),
                    },
                );
            }
        })
    }

    pub(super) fn begin_runtime(&self, saved: &Saved) -> anyhow::Result<()> {
        let previous_active = self.update(|state| {
            let previous = state.runtime_active;
            state.runtime_active = Some(true);
            previous
        })?;
        // 用户要求启动仅补偿：正常退出只重查已有待回收项；缺少旧版日志或异常退出才补登记受管身份。
        // 已确认停止的身份保持完成；原生终端后来恢复它，不等于本服务重新取得了回收所有权。
        if previous_active != Some(false) {
            self.schedule(saved, "startup_recovery")?;
        }
        Ok(())
    }

    pub(super) fn finish_runtime(&self) -> anyhow::Result<()> {
        self.update(|state| state.runtime_active = Some(false))
    }

    pub(super) fn cancel(&self, id: &str) -> anyhow::Result<()> {
        // 用户继续使用会话时先取消旧意图；实际停止与发送仍共用同一会话锁，迟到检查不能误停新任务。
        self.update(|state| {
            state.intents.remove(id);
            state.stopped.remove(id);
        })?;
        Ok(())
    }

    fn pending(&self, force: bool) -> anyhow::Result<Vec<Intent>> {
        self.update(|state| {
            state
                .intents
                .values()
                .filter(|intent| {
                    force || now_ms().saturating_sub(intent.checked_at) >= RETRY_INTERVAL_MS
                })
                .cloned()
                .collect()
        })
    }

    fn current(&self, intent: &Intent) -> anyhow::Result<bool> {
        self.update(|state| {
            state.intents.get(&intent.thread_id).is_some_and(|current| {
                current.generation == intent.generation
                    && current.native_id == intent.native_id
                    && current.config == intent.config
            })
        })
    }

    fn finish(&self, intent: &Intent, result: &Value) -> anyhow::Result<()> {
        self.update(|state| {
            let Some(current) = state.intents.get_mut(&intent.thread_id) else {
                return;
            };
            if current.generation != intent.generation {
                return;
            }
            if result["stopped"] == true {
                state.stopped.insert(
                    intent.thread_id.clone(),
                    identity(&intent.native_id, &intent.job_id, &intent.config),
                );
                state.intents.remove(&intent.thread_id);
            } else {
                current.checked_at = now_ms();
                current.result = result.clone();
            }
        })
    }

    pub(super) fn report(&self) -> anyhow::Result<Value> {
        self.update(|state| {
            json!({
                "complete":state.intents.is_empty(),
                "pending":state.intents.values().map(|intent| json!({
                    "threadId":intent.thread_id,"requestedAt":intent.requested_at,
                    "checkedAt":intent.checked_at,"reason":intent.reason,"result":intent.result
                })).collect::<Vec<_>>(),
                "lastReport":state.last_report
            })
        })
    }
}

/// 安装包或旧服务无法正常响应时先保存意图，下一次启动仍能补做；不改用户启用开关或会话历史。
pub(super) fn remember_shutdown(directory: &Path, reason: &str) -> anyhow::Result<()> {
    let path = directory.join("remote-claude-sessions.json");
    let saved: Saved = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("无法核对 Claude 受管会话")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Saved::default(),
        Err(error) => return Err(error.into()),
    };
    CleanupStore::new(directory).schedule(&saved, reason)
}

impl Claude {
    pub(in crate::remote_access) fn record_clean_exit(&self) -> anyhow::Result<()> {
        self.cleanup_store.finish_runtime()
    }

    pub(super) async fn wait_for_startup_cleanup(&self) {
        loop {
            let ready = self.startup_cleanup_notify.notified();
            if self.startup_cleanup_ready.load(Ordering::Acquire) {
                return;
            }
            ready.await;
        }
    }

    pub(in crate::remote_access) fn cleanup_report(&self) -> Value {
        self.cleanup_store
            .report()
            .unwrap_or_else(|_| json!({"complete":false,"error":"Claude 回收记录暂时不可读取"}))
    }

    pub(super) async fn recover_cleanup(self: &Arc<Self>, force: bool) -> Value {
        let Ok(_serial) = self.cleanup_serial.try_lock() else {
            return self.cleanup_report();
        };
        let intents = match self.cleanup_store.pending(force) {
            Ok(intents) => intents,
            Err(_) => return self.cleanup_report(),
        };
        if intents.is_empty() {
            return self.cleanup_report();
        }
        let results = stream::iter(intents.into_iter().map(|intent| async move {
            let outcome = tokio::time::timeout(RELEASE_TIMEOUT, async {
                let lock = self.lock(&intent.thread_id).await;
                let _guard = lock.lock().await;
                if !self.cleanup_store.current(&intent)? {
                    return Ok(json!({"stopped":false,"reason":"cancelled"}));
                }
                let c = self.conversation(&intent.thread_id).await?;
                anyhow::ensure!(
                    c.native_id == intent.native_id
                        && c.native_job_id == intent.job_id
                        && c.config == intent.config,
                    "Claude 回收目标身份已变化"
                );
                if c.turns.last().is_some_and(|turn| {
                    matches!(turn["status"].as_str(), Some("pending" | "unknown"))
                        && turn["_nativeDone"] != true
                }) || !c.remote_queue.edits().is_empty()
                {
                    return Ok(json!({"stopped":false,"reason":"unconfirmed_delivery"}));
                }
                let process = self
                    .processes
                    .lock()
                    .await
                    .get(&intent.thread_id)
                    .filter(|process| process.channel.live.load(Ordering::Acquire))
                    .cloned();
                let result = if let Some(process) = process {
                    self.shutdown_process(&process).await?
                } else {
                    self.cleanup_probe(&intent).await?
                };
                if result["stopped"] == true {
                    self.processes.lock().await.remove(&intent.thread_id);
                }
                Ok::<Value, anyhow::Error>(result)
            })
            .await;
            let result = match outcome {
                Ok(Ok(value)) => value,
                Ok(Err(_)) => json!({"stopped":false,"reason":"cleanup_failed"}),
                Err(_) => json!({"stopped":false,"reason":"outcome_unknown"}),
            };
            // 不再吞掉保留、失败或超时结果；保留原始意图，只在已确认进程退出后移除。
            let persistence_failed = self.cleanup_store.finish(&intent, &result).is_err();
            let entry = json!({"threadId":intent.thread_id,"result":result,
                "persistenceFailed":persistence_failed});
            let _ = crate::diagnostic_log::append_diagnostic_log("claude.cleanup", entry.clone());
            Ok::<Value, anyhow::Error>(entry)
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>();
        let outcomes = tokio::time::timeout(CLEANUP_BUDGET, results).await;
        let report = match outcomes {
            Ok(results) => json!({
                "checkedAt":now_ms(),
                "results":results.into_iter().filter_map(Result::ok).collect::<Vec<_>>()
            }),
            Err(_) => json!({"checkedAt":now_ms(),"reason":"outcome_unknown"}),
        };
        let _ = self
            .cleanup_store
            .update(|state| state.last_report = report);
        self.cleanup_report()
    }

    async fn cleanup_probe(&self, intent: &Intent) -> anyhow::Result<Value> {
        let script = self.directory.join("claude-shared.py");
        crate::settings::atomic_write(&script, include_bytes!("claude_shared.py"))?;
        let linux_script = Self::linux_cwd(
            &intent.config,
            script.to_str().context("Claude 回收连接端路径无效")?,
        )
        .await?;
        let mut command = Self::wsl(&intent.config)?;
        // 回收探针固定在同一 WSL 用户 Home，不加载项目上下文、不启动或恢复模型会话。
        command.args(["--cd", "~"]);
        Self::wsl_login_exec(&mut command, "python3");
        command.args([
            "-I",
            "-u",
            &linux_script,
            "--cli",
            &intent.config.executable,
            "--native-id",
            &intent.native_id,
            "--job-id",
            &intent.job_id,
            "--cleanup",
        ]);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("Claude 回收输出不可用")?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        anyhow::ensure!(
            child.wait().await?.success() && bytes.len() <= 16 * 1024,
            "Claude 回收未确认"
        );
        let value: Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(value["stopped"].is_boolean(), "Claude 回收响应无效");
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Saved, CleanupStore, String) {
        let (directory, _, mut c) = super::super::tests::fixture();
        c.shared = true;
        c.native_job_id = "1234abcd".into();
        let id = c.id.clone();
        let mut saved = Saved::default();
        saved.conversations.insert(id.clone(), c);
        let store = CleanupStore::new(directory.path());
        (directory, saved, store, id)
    }

    #[test]
    fn retained_result_survives_restart_and_only_confirmed_stop_completes_cleanup() {
        let (directory, saved, store, _) = fixture();
        store.schedule(&saved, "exit").unwrap();
        let intent = store.pending(true).unwrap().pop().unwrap();
        store
            .finish(&intent, &json!({"stopped":false,"reason":"native_draft"}))
            .unwrap();
        let recovered = CleanupStore::new(directory.path());
        assert_eq!(
            recovered.report().unwrap()["pending"][0]["result"]["reason"],
            "native_draft"
        );
        assert!(
            recovered.pending(false).unwrap().is_empty(),
            "保留项按间隔重试，不忙循环"
        );
        recovered.finish(&intent, &json!({"stopped":true})).unwrap();
        assert_eq!(recovered.report().unwrap()["complete"], true);
        recovered.schedule(&saved, "service_shutdown").unwrap();
        assert!(
            recovered.pending(true).unwrap().is_empty(),
            "同次关闭不重复回收已确认停止的身份"
        );
        recovered.finish_runtime().unwrap();
        recovered.begin_runtime(&saved).unwrap();
        assert!(
            recovered.pending(true).unwrap().is_empty(),
            "正常退出后不重新回收已完成的历史身份，避免打断用户后来在原生终端恢复的会话"
        );
        recovered
            .cancel(&saved.conversations.keys().next().unwrap())
            .unwrap();
        recovered.begin_runtime(&saved).unwrap();
        assert_eq!(
            recovered.pending(true).unwrap().len(),
            1,
            "远程重新使用后异常退出，下次启动须补偿尚未完成的回收"
        );
        let next = recovered.pending(true).unwrap().pop().unwrap();
        recovered
            .finish(&next, &json!({"stopped":false,"reason":"native_draft"}))
            .unwrap();
        recovered.schedule(&saved, "service_shutdown").unwrap();
        assert_eq!(
            recovered.pending(false).unwrap().len(),
            1,
            "旧停止回执不能阻止新一次退出重新核对已保留的会话"
        );
    }

    #[test]
    fn user_reuse_cancels_old_intent_and_late_result_cannot_remove_new_one() {
        let (_directory, saved, store, id) = fixture();
        store.schedule(&saved, "exit").unwrap();
        let old = store.pending(true).unwrap().pop().unwrap();
        store.cancel(&id).unwrap();
        assert!(!store.current(&old).unwrap());
        store.schedule(&saved, "next_exit").unwrap();
        let current = store.pending(true).unwrap().pop().unwrap();
        assert_ne!(old.generation, current.generation);
        store.finish(&old, &json!({"stopped":true})).unwrap();
        assert!(
            store.current(&current).unwrap(),
            "旧回收回执不能完成下一次退出的清理"
        );
    }

    #[test]
    fn startup_cleanup_excludes_discovered_deleted_and_unstarted_sessions() {
        let (_directory, mut saved, store, id) = fixture();
        for (suffix, discovered, deleted, job) in [
            ("discovered", true, false, "12345678"),
            ("deleted", false, true, "12345678"),
            ("unstarted", false, false, ""),
        ] {
            let mut c = saved.conversations[&id].clone();
            c.id = format!("claude-{}", uuid::Uuid::new_v4());
            c.title = suffix.into();
            c.discovered = discovered;
            c.deleted = deleted;
            c.native_job_id = job.into();
            saved.conversations.insert(c.id.clone(), c);
        }
        store.schedule(&saved, "startup_recovery").unwrap();
        let pending = store.pending(true).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].thread_id, id);
    }

    #[test]
    fn damaged_cleanup_record_is_not_overwritten_or_treated_as_success() {
        let (directory, saved, store, _) = fixture();
        let path = directory.path().join(CLEANUP_FILE);
        std::fs::write(&path, b"not-json").unwrap();
        assert!(store.schedule(&saved, "exit").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"not-json");
    }
}
