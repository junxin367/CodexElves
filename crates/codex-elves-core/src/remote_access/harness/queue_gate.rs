use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::AsyncBufReadExt;

use super::Harness;
use super::store::{Data, QueueGate};

const UNKNOWN_END: &str = "上次任务未确认正常结束，待发送消息已暂停；请手动重试。";
const INTERRUPTED_END: &str = "上次任务被中断或失败，待发送消息已暂停；请手动重试。";

impl Data {
    fn has_queue(&self, id: &str) -> bool {
        self.queues.get(id).is_some_and(|queue| {
            queue
                .iter()
                .any(|message| !message.state.starts_with("direct-"))
        })
    }

    pub(super) fn pause_queue(&mut self, id: &str, reason: &str) -> bool {
        let mut paused = false;
        for message in self.queues.get_mut(id).into_iter().flatten() {
            // 只暂停尚未派发的消息；编辑草稿、发送中和结果未知的原回执不能改写。
            if message.state == "queued" {
                message.state = "paused".into();
                paused = true;
            }
        }
        if self.has_queue(id) {
            let gate = self.queue_gates.entry(id.to_owned()).or_default();
            gate.phase = "paused".into();
            gate.reason = reason.into();
        }
        paused
    }

    pub(super) fn wait_queue(&mut self, id: &str, turn: Option<String>, compacting: bool) -> bool {
        if !self.has_queue(id) {
            return false;
        }
        let previous = self.queue_gates.get(id).cloned().unwrap_or_default();
        let kind = if compacting { "compaction" } else { "turn" };
        // 同一轮次的重试和迟到 start 回执不回退已确认终态。
        // 同一 turnId 也可能随后进入独立压缩，必须单独等待压缩完成，不能沿用上一轮的 ready。
        if turn.is_some() && previous.turn_id == turn && previous.kind == kind {
            return false;
        }
        let missing_end = matches!(previous.phase.as_str(), "waiting" | "recovering")
            && previous.kind != "compaction"
            && turn.is_some()
            && (previous.turn_id.is_some() && previous.turn_id != turn
                || previous.phase == "recovering" && previous.turn_id.is_none());
        let paused = missing_end && self.pause_queue(id, UNKNOWN_END);
        let reason = self
            .queue_gates
            .get(id)
            .map(|gate| gate.reason.clone())
            .unwrap_or_default();
        let inherited = (previous.kind == kind
            && matches!(previous.phase.as_str(), "waiting" | "recovering"))
        .then_some(previous.turn_id)
        .flatten();
        self.queue_gates.insert(
            id.to_owned(),
            QueueGate {
                turn_id: turn.or(inherited),
                kind: if compacting { "compaction" } else { "turn" }.into(),
                phase: "waiting".into(),
                reason,
            },
        );
        paused
    }

    pub(super) fn authorize_queue_edit(
        &mut self,
        id: &str,
        message_id: &str,
        turn: Option<String>,
        compacting: bool,
    ) {
        let reason = self
            .queue_gates
            .get(id)
            .map(|gate| gate.reason.clone())
            .unwrap_or_default();
        if self
            .queue_gates
            .get(id)
            .is_some_and(|gate| matches!(gate.phase.as_str(), "recovering" | "paused"))
        {
            for message in self.queues.get_mut(id).into_iter().flatten() {
                if message.id != message_id && message.state == "queued" {
                    message.state = "paused".into();
                }
            }
        }
        // 用户重新点击发送属于明确提交；仍在执行或压缩时继续等待该任务，不自动解除其它消息的暂停。
        self.queue_gates.insert(
            id.to_owned(),
            QueueGate {
                phase: if turn.is_some() || compacting {
                    "waiting"
                } else {
                    "ready"
                }
                .into(),
                kind: if compacting { "compaction" } else { "turn" }.into(),
                turn_id: turn,
                reason,
            },
        );
    }

    pub(super) fn finish_queue(&mut self, id: &str, turn: &str, status: &str) -> bool {
        let Some(gate) = self.queue_gates.get(id) else {
            return false;
        };
        if gate
            .turn_id
            .as_deref()
            .is_some_and(|expected| expected != turn)
            || matches!(gate.phase.as_str(), "ready" | "paused")
            // 未能记录当前轮次时，迟到的旧完成事件不能解除等待，须再核对持久完成记录。
            || gate.turn_id.is_none() && gate.kind != "compaction"
        {
            return false;
        }
        if status == "completed" {
            let gate = self.queue_gates.get_mut(id).unwrap();
            gate.turn_id = (!turn.is_empty()).then(|| turn.to_owned());
            gate.phase = "ready".into();
            false
        } else {
            self.pause_queue(id, INTERRUPTED_END)
        }
    }
}

impl Harness {
    pub(super) async fn queue_anchor(&self, id: &str) -> anyhow::Result<(Option<String>, bool)> {
        {
            let projection = self.projection.lock().await;
            if let Some(turn) = projection.active.get(id) {
                return Ok((Some(turn.clone()), false));
            }
            if let Some(compaction) = projection.compactions.get(id) {
                return Ok((compaction["turnId"].as_str().map(str::to_owned), true));
            }
        }
        let latest = self.latest_turn(id).await?;
        Ok((
            latest
                .filter(|turn| turn["status"] == "inProgress")
                .and_then(|turn| turn["id"].as_str().map(str::to_owned)),
            false,
        ))
    }

    pub(super) async fn remember_queue_wait(
        &self,
        id: &str,
        turn: Option<String>,
        compacting: bool,
    ) -> anyhow::Result<()> {
        let mut store = self.store.lock().await;
        if !store.data.has_queue(id)
            || turn.is_some()
                && store.data.queue_gates.get(id).is_some_and(|gate| {
                    gate.turn_id == turn
                        && gate.kind == if compacting { "compaction" } else { "turn" }
                })
        {
            return Ok(());
        }
        let paused = store.commit(|data| Ok(data.wait_queue(id, turn, compacting)))?;
        drop(store);
        if paused {
            self.queue_changed(id).await;
        }
        Ok(())
    }

    pub(super) async fn observe_queue_lifecycle(&self, frame: &Value) -> anyhow::Result<bool> {
        let method = frame["method"].as_str().unwrap_or("");
        // 原生 error.willRetry=true 仍属于当前轮次；只等原生终态，不在远程层重放 turn/start。
        if !matches!(
            method,
            "turn/started"
                | "turn/completed"
                | "item/started"
                | "item/completed"
                | "harness/disconnected"
        ) || frame.get("id").is_some()
        {
            return Ok(false);
        }
        let generation = frame["_generation"].as_str().unwrap_or("");
        let projection = self.projection.lock().await;
        if !generation.is_empty() && generation != projection.generation {
            return Ok(false);
        }
        drop(projection);
        let mut store = self.store.lock().await;
        if method == "harness/disconnected" {
            if store
                .data
                .queue_gates
                .values()
                .any(|gate| gate.phase == "waiting")
            {
                store.commit(|data| {
                    for gate in data
                        .queue_gates
                        .values_mut()
                        .filter(|gate| gate.phase == "waiting")
                    {
                        gate.phase = "recovering".into();
                    }
                    Ok(())
                })?;
            }
            return Ok(false);
        }
        let id = frame["params"]["threadId"].as_str().unwrap_or("");
        if !store.data.has_queue(id) {
            return Ok(false);
        }
        let turn = frame["params"]["turn"]["id"]
            .as_str()
            .or_else(|| frame["params"]["turnId"].as_str())
            .unwrap_or("");
        let compacting = store.data.queue_gates.get(id).is_some_and(|gate| {
            gate.kind == "compaction" && !matches!(gate.phase.as_str(), "ready" | "paused")
        });
        if method.starts_with("item/")
            && (frame["params"]["item"]["type"] != "contextCompaction" || !compacting)
        {
            return Ok(false);
        }
        let paused = store.commit(|data| {
            Ok(match method {
                "turn/started" | "item/started" => {
                    data.wait_queue(id, (!turn.is_empty()).then(|| turn.to_owned()), compacting)
                }
                "turn/completed" => data.finish_queue(id, turn, {
                    let outcome = &frame["params"]["turn"];
                    if outcome["error"].is_null() {
                        outcome["status"].as_str().unwrap_or("unknown")
                    } else {
                        "failed"
                    }
                }),
                "item/completed" => data.finish_queue(
                    id,
                    turn,
                    if matches!(
                        frame["params"]["item"]["status"].as_str(),
                        Some("failed" | "interrupted")
                    ) {
                        "failed"
                    } else {
                        "completed"
                    },
                ),
                _ => false,
            })
        })?;
        self.queue_recovery_after.lock().await.remove(id);
        Ok(paused)
    }

    async fn pause_recovered_queue(
        &self,
        id: &str,
        reason: &str,
        expected: &QueueGate,
    ) -> anyhow::Result<bool> {
        let mut store = self.store.lock().await;
        if store
            .data
            .queue_gates
            .get(id)
            .is_some_and(|gate| gate != expected)
        {
            return Ok(false);
        }
        store.commit(|data| Ok(data.pause_queue(id, reason)))?;
        drop(store);
        self.queue_changed(id).await;
        self.queue_recovery_after.lock().await.remove(id);
        Ok(false)
    }

    async fn hold_unconfirmed_queue(&self, id: &str, expected: &QueueGate) -> anyhow::Result<bool> {
        let mut store = self.store.lock().await;
        // 核对期间可能刚收到正常完成事件，旧读取不能把已确认完成重新改成待确认。
        if store
            .data
            .queue_gates
            .get(id)
            .is_some_and(|gate| gate != expected)
        {
            return Ok(false);
        }
        let changed = store
            .data
            .queue_gates
            .get(id)
            .is_none_or(|gate| gate.phase != "recovering" || gate.reason != UNKNOWN_END);
        if changed {
            store.commit(|data| {
                let gate = data.queue_gates.entry(id.to_owned()).or_default();
                gate.phase = "recovering".into();
                gate.reason = UNKNOWN_END.into();
                Ok(())
            })?;
        }
        drop(store);
        // 未确认结束时最多每 3 秒重复只读核对；正常完成事件直接解除等待，不增加正常队列延迟。
        self.queue_recovery_after
            .lock()
            .await
            .insert(id.to_owned(), Instant::now() + Duration::from_secs(3));
        if changed {
            self.queue_changed(id).await;
        }
        Ok(false)
    }

    pub(super) async fn queue_completion_confirmed(
        &self,
        id: &str,
        identity: &Value,
        queued_at: u64,
    ) -> anyhow::Result<bool> {
        let (gate, queued_at) = {
            let store = self.store.lock().await;
            (
                store
                    .data
                    .queue_gates
                    .get(id)
                    .cloned()
                    .unwrap_or(QueueGate {
                        kind: "turn".into(),
                        phase: "recovering".into(),
                        ..Default::default()
                    }),
                store
                    .data
                    .queues
                    .get(id)
                    .into_iter()
                    .flatten()
                    .filter(|message| message.state == "queued")
                    .map(|message| message.created_at)
                    .max()
                    .unwrap_or(queued_at),
            )
        };
        if gate.phase == "ready" {
            self.queue_recovery_after.lock().await.remove(id);
            return Ok(true);
        }
        if gate.phase == "paused" {
            return Ok(false);
        }
        if self
            .queue_recovery_after
            .lock()
            .await
            .get(id)
            .is_some_and(|until| Instant::now() < *until)
        {
            return Ok(false);
        }
        let observed = self
            .projection
            .lock()
            .await
            .queue_turn_outcomes
            .get(id)
            .cloned();
        if let Some(observed) = observed
            && gate
                .turn_id
                .as_deref()
                .is_some_and(|turn| observed["turnId"] == turn)
            && observed["recoveryCheck"] != true
        {
            if observed["status"] != "completed" {
                return self.pause_recovered_queue(id, INTERRUPTED_END, &gate).await;
            }
            self.store.lock().await.commit(|data| {
                if data.queue_gates.get(id) == Some(&gate) {
                    data.queue_gates.get_mut(id).unwrap().phase = "ready".into();
                }
                Ok(())
            })?;
            return Ok(self
                .store
                .lock()
                .await
                .data
                .queue_gates
                .get(id)
                .is_some_and(|gate| gate.phase == "ready"));
        }
        if gate.kind == "compaction" {
            return self.hold_unconfirmed_queue(id, &gate).await;
        }
        let page = self
            .runtime
            .request(
                "thread/turns/list",
                json!({"threadId":id,"limit":1,"itemsView":"notLoaded","sortDirection":"desc"}),
                None,
            )
            .await?;
        let Some(turn) = page["data"].as_array().and_then(|turns| turns.first()) else {
            return self.hold_unconfirmed_queue(id, &gate).await;
        };
        let turn_id = turn["id"].as_str().unwrap_or("");
        if turn["status"] == "inProgress" {
            self.remember_queue_wait(id, Some(turn_id.to_owned()), false)
                .await?;
            return Ok(false);
        }
        if matches!(
            turn["status"].as_str(),
            Some("failed" | "interrupted" | "cancelled" | "canceled")
        ) {
            return self.pause_recovered_queue(id, INTERRUPTED_END, &gate).await;
        }
        if turn["status"] != "completed"
            || gate
                .turn_id
                .as_deref()
                .is_some_and(|expected| expected != turn_id)
        {
            return self.hold_unconfirmed_queue(id, &gate).await;
        }
        let mut path = identity["thread"]["path"]
            .as_str()
            .or_else(|| identity["thread"]["rolloutPath"].as_str())
            .map(str::to_owned);
        if path.is_none()
            && let Some(owner) = self.owner_view_snapshot(id).await?
        {
            path = owner["rolloutPath"]
                .as_str()
                .or_else(|| owner["path"].as_str())
                .map(str::to_owned);
        }
        // 用户要求异常恢复不抢发：恢复快照中的 completed 可能只是进程退出后的重建值。
        // 必须找到该轮原生 task_complete，且完成时间晚于入队时间；不以“已有回复文字”代替完成回执。
        let confirmed = if let Some(path) = path {
            rollout_completed(Path::new(&path), turn_id, queued_at)
                .await
                .unwrap_or(false)
        } else {
            false
        };
        if !confirmed {
            return self.hold_unconfirmed_queue(id, &gate).await;
        }
        let mut store = self.store.lock().await;
        if store
            .data
            .queue_gates
            .get(id)
            .is_some_and(|current| current != &gate)
        {
            return Ok(false);
        }
        store.commit(|data| {
            data.queue_gates.insert(
                id.to_owned(),
                QueueGate {
                    turn_id: Some(turn_id.to_owned()),
                    kind: "turn".into(),
                    phase: "ready".into(),
                    reason: gate.reason,
                },
            );
            Ok(())
        })?;
        Ok(true)
    }
}

async fn rollout_completed(path: &Path, target: &str, queued_at: u64) -> anyhow::Result<bool> {
    if !path.is_absolute() || target.is_empty() {
        return Ok(false);
    }
    let file = tokio::fs::File::open(path).await?;
    let mut lines = tokio::io::BufReader::new(file).lines();
    let mut current = None::<String>;
    let mut last_started = None::<String>;
    let mut target_started = false;
    let mut terminal_failure = false;
    let mut completed = false;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if ![
            "task_started",
            "task_complete",
            "turn_started",
            "turn_complete",
            "turn_aborted",
            "\"error\"",
            "\"turn_context\"",
        ]
        .iter()
        .any(|kind| line.contains(kind))
        {
            // 不能跳过损坏尾行，否则可能漏掉崩溃前刚开始的下一轮；IgnoredAny 只校验、不分配正文对象。
            if serde_json::from_str::<serde::de::IgnoredAny>(&line).is_err() {
                return Ok(false);
            }
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            return Ok(false);
        };
        let payload = &record["payload"];
        if record["type"] == "turn_context" {
            if let Some(id) = payload["turn_id"].as_str() {
                current = Some(id.to_owned());
                last_started = current.clone();
            }
            continue;
        }
        if record["type"] != "event_msg" {
            continue;
        }
        let id = payload["turn_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| current.clone());
        match payload["type"].as_str() {
            Some("task_started" | "turn_started") => {
                current = id.clone();
                last_started = id.clone();
                if id.as_deref() == Some(target) {
                    target_started = true;
                    completed = false;
                }
            }
            Some("task_complete" | "turn_complete") if id.as_deref() == Some(target) => {
                // Codex 的 task_complete 也可携带终止错误；结束不等于成功，重试通知 stream_error 不作失败处理。
                terminal_failure |= !payload["error"].is_null();
                completed = !terminal_failure
                    && record["timestamp"]
                        .as_str()
                        .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
                        .and_then(|timestamp| u64::try_from(timestamp.timestamp_millis()).ok())
                        .is_some_and(|timestamp| timestamp >= queued_at);
            }
            Some("turn_aborted") if id.as_deref() == Some(target) => {
                terminal_failure = true;
                completed = false;
            }
            Some("error") if id.as_deref() == Some(target) && native_terminal_error(payload) => {
                terminal_failure = true;
                completed = false;
            }
            _ => {}
        }
    }
    Ok(target_started && completed && last_started.as_deref() == Some(target))
}

fn native_terminal_error(payload: &Value) -> bool {
    if payload["willRetry"] == true || payload["will_retry"] == true {
        return false;
    }
    let info = &payload["codex_error_info"];
    // 与原生 ErrorEvent.affects_turn_status 一致：回退失败和不可引导提示不改变当前轮次的成功状态。
    !matches!(
        info.as_str(),
        Some("thread_rollback_failed" | "active_turn_not_steerable")
    ) && info.get("active_turn_not_steerable").is_none()
}

#[cfg(test)]
mod tests {
    use super::super::store::{Queued, Store};
    use super::*;

    #[tokio::test]
    async fn queue_recovery_requires_the_same_native_completed_turn_after_all_queued_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let started = json!({"timestamp":"2026-11-13T10:00:00Z","type":"event_msg",
            "payload":{"type":"task_started","turn_id":"original"}});
        let done = json!({"timestamp":"2026-11-13T10:00:02Z","type":"event_msg",
            "payload":{"type":"task_complete","turn_id":"original"}});
        let write = |records: Vec<Value>| {
            std::fs::write(
                &path,
                records
                    .iter()
                    .map(|record| format!("{record}\n"))
                    .collect::<String>(),
            )
            .unwrap()
        };
        let queued = chrono::DateTime::parse_from_rfc3339("2026-11-13T10:00:01Z")
            .unwrap()
            .timestamp_millis() as u64;
        write(vec![done.clone()]);
        assert!(
            !rollout_completed(&path, "original", queued).await.unwrap(),
            "缺少同一轮次的原生开始记录时，不能只凭结束记录恢复队列"
        );
        write(vec![
            started.clone(),
            json!({"type":"response_item","payload":{
            "type":"message","role":"assistant","phase":"final_answer","content":[{"text":"已输出文字但未正常结束"}]}}),
        ]);
        assert!(!rollout_completed(&path, "original", queued).await.unwrap());
        write(vec![started.clone(), done.clone()]);
        assert!(rollout_completed(&path, "original", queued).await.unwrap());
        std::fs::write(&path, format!("{started}\n{done}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\"")).unwrap();
        assert!(
            !rollout_completed(&path, "original", queued).await.unwrap(),
            "损坏的新轮次尾行不能忽略"
        );
        write(vec![started.clone(), done.clone()]);
        assert!(
            !rollout_completed(&path, "original", queued + 2000)
                .await
                .unwrap()
        );
        assert!(!rollout_completed(&path, "unrelated", queued).await.unwrap());
        write(vec![
            started.clone(),
            done.clone(),
            json!({"type":"event_msg",
            "payload":{"type":"task_started","turn_id":"later-unfinished"}}),
        ]);
        assert!(!rollout_completed(&path, "original", queued).await.unwrap());
        write(vec![
            started,
            done,
            json!({"type":"event_msg",
            "payload":{"type":"turn_aborted","turn_id":"original"}}),
        ]);
        assert!(!rollout_completed(&path, "original", queued).await.unwrap());
    }

    #[tokio::test]
    async fn queue_recovery_distinguishes_native_retries_from_terminal_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let started = json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"A"}});
        let done = json!({"timestamp":"2026-11-13T10:00:02Z","type":"event_msg",
            "payload":{"type":"task_complete","turn_id":"A"}});
        let write = |records: Vec<Value>| {
            std::fs::write(
                &path,
                records
                    .iter()
                    .map(|record| format!("{record}\n"))
                    .collect::<String>(),
            )
            .unwrap()
        };
        for retry in [
            json!({"type":"stream_error","message":"Reconnecting... 1/5"}),
            json!({"type":"error","willRetry":true,"message":"temporary error"}),
            json!({"type":"error","codex_error_info":"thread_rollback_failed"}),
            json!({"type":"error","codex_error_info":{"active_turn_not_steerable":{"turn_kind":"compact"}}}),
        ] {
            write(vec![
                started.clone(),
                json!({"type":"event_msg","payload":retry}),
                done.clone(),
            ]);
            assert!(
                rollout_completed(&path, "A", 1).await.unwrap(),
                "原生重试或不影响轮次状态的提示不能阻止成功后的续发"
            );
        }
        for terminal in [
            json!({"type":"error","message":"retry limit reached"}),
            json!({"type":"error","codex_error_info":{"response_too_many_failed_attempts":{"http_status_code":503}}}),
        ] {
            write(vec![
                started.clone(),
                json!({"type":"event_msg","payload":terminal}),
                done.clone(),
            ]);
            assert!(
                !rollout_completed(&path, "A", 1).await.unwrap(),
                "终止错误之后即使出现结束记录，也不能恢复自动续发"
            );
        }
        let mut failed_end = done.clone();
        failed_end["payload"]["error"] = json!({"message":"upstream failure"});
        write(vec![started, failed_end, done]);
        assert!(!rollout_completed(&path, "A", 1).await.unwrap());

        write(vec![
            json!({"type":"event_msg","payload":{"type":"turn_started","turn_id":"A"}}),
            json!({"timestamp":"2026-11-13T10:00:02Z","type":"event_msg",
                "payload":{"type":"turn_complete","turn_id":"A"}}),
        ]);
        assert!(rollout_completed(&path, "A", 1).await.unwrap());
    }

    #[test]
    fn queue_recovery_keeps_order_and_unknown_receipts_and_preserves_confirmed_completion() {
        let directory = tempfile::tempdir().unwrap();
        let message = |id: &str, state: &str| Queued {
            id: id.into(),
            payload: json!({"prompt":id,"clientUserMessageId":id}),
            created_at: 1,
            device_id: "device".into(),
            key_id: "key".into(),
            state: state.into(),
            request_id: None,
        };
        let mut store = Store::open(directory.path()).unwrap();
        store
            .commit(|data| {
                data.queues.insert(
                    "thread".into(),
                    vec![
                        message("B", "queued"),
                        message("C", "queued"),
                        message("unknown", "sending"),
                        message("draft", "editing"),
                    ],
                );
                data.wait_queue("thread", Some("A".into()), false);
                Ok(())
            })
            .unwrap();
        let mut restored = Store::open(directory.path()).unwrap();
        assert_eq!(restored.data.queue_gates["thread"].phase, "recovering");
        assert_eq!(
            restored.data.queue_gates["thread"].turn_id.as_deref(),
            Some("A")
        );
        restored
            .commit(|data| {
                assert!(data.finish_queue("thread", "A", "interrupted"));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            restored.data.queues["thread"]
                .iter()
                .map(|message| message.state.as_str())
                .collect::<Vec<_>>(),
            ["paused", "paused", "unknown", "editing"]
        );
        assert_eq!(
            restored.data.queues["thread"]
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            ["B", "C", "unknown", "draft"]
        );
        restored
            .commit(|data| {
                data.authorize_queue_edit("thread", "B", None, false);
                data.queues.get_mut("thread").unwrap()[0].state = "queued".into();
                Ok(())
            })
            .unwrap();
        let confirmed = Store::open(directory.path()).unwrap();
        assert_eq!(confirmed.data.queue_gates["thread"].phase, "ready");
        assert_eq!(confirmed.data.queues["thread"][1].state, "paused");
    }

    #[test]
    fn compaction_on_the_completed_turn_keeps_its_own_completion_boundary() {
        let mut data = Data::default();
        data.queues.insert(
            "thread".into(),
            vec![Queued {
                id: "queued".into(),
                payload: json!({}),
                created_at: 1,
                device_id: "device".into(),
                key_id: "key".into(),
                state: "queued".into(),
                request_id: None,
            }],
        );
        data.wait_queue("thread", None, false);
        data.finish_queue("thread", "stale", "completed");
        assert_eq!(data.queue_gates["thread"].phase, "waiting");
        data.wait_queue("thread", Some("A".into()), false);
        data.finish_queue("thread", "A", "completed");
        data.wait_queue("thread", Some("A".into()), true);
        assert_eq!(data.queue_gates["thread"].kind, "compaction");
        assert_eq!(data.queue_gates["thread"].phase, "waiting");
        data.finish_queue("thread", "A", "completed");
        assert_eq!(data.queue_gates["thread"].phase, "ready");
    }
}
