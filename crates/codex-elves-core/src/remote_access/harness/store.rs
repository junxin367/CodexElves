use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::super::{RemoteAuthorization, now_ms};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Queued {
    pub id: String,
    pub payload: Value,
    pub created_at: u64,
    pub device_id: String,
    pub key_id: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_revision: Option<u64>,
}

impl Queued {
    pub fn new(mut payload: Value, authorization: &RemoteAuthorization) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        if payload["clientUserMessageId"]
            .as_str()
            .is_none_or(str::is_empty)
        {
            payload["clientUserMessageId"] = json!(uuid::Uuid::new_v4().to_string());
        }
        Self {
            id,
            payload,
            created_at: now_ms(),
            device_id: authorization.device.id.clone(),
            key_id: authorization.device.key_id.clone(),
            state: "queued".into(),
            request_id: authorization.request_id.clone(),
            runtime_revision: super::execution::runtime_revision(),
        }
    }

    pub fn view(&self) -> Value {
        let images: Vec<_> = self.payload["images"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|image| json!({"src":image["dataUrl"],"name":image["name"]}))
            .collect();
        json!({"id":self.id,"clientUserMessageId":self.payload["clientUserMessageId"],
            "text":self.payload["prompt"],"prompt":self.payload["prompt"],"createdAt":self.created_at,
            "canSendNow":matches!(self.state.as_str(),"queued"|"paused"),"pausedReason":match self.state.as_str() {
                "unknown" => json!("消息送达状态待核对"), "paused" => json!("消息已暂停"), _ => Value::Null },
            "remoteDeliveryState":if self.state=="unknown" {"outcome-unknown"} else {"queued"},
            "submission":{"status":match self.state.as_str(){"unknown"=>"outcome-unknown","sending"=>"sending",_=>"queued"}},
            "context":{"prompt":self.payload["prompt"],"imageAttachments":images},
            "submissionOptions":{"clientUserMessageId":self.payload["clientUserMessageId"],
                "model":self.payload["model"],"reasoningEffort":self.payload["reasoningEffort"]}})
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Edit {
    pub thread_id: String,
    pub message_id: String,
    pub state: String,
    pub original: Queued,
    pub result: Value,
    #[serde(default)]
    pub terminal_fingerprint: Option<String>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct QueueGate {
    pub turn_id: Option<String>,
    pub kind: String,
    pub phase: String,
    pub reason: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Data {
    pub queues: BTreeMap<String, Vec<Queued>>,
    pub edits: BTreeMap<String, Edit>,
    pub sent: BTreeMap<String, Value>,
    pub sent_hashes: BTreeMap<String, String>,
    pub thread_pins: BTreeSet<String>,
    pub project_pins: BTreeSet<String>,
    pub project_pin_overrides: BTreeMap<String, bool>,
    pub preferences: Value,
    pub settings: BTreeMap<String, Value>,
    pub answered_questions: BTreeMap<String, Value>,
    pub thread_devices: BTreeMap<String, String>,
    pub queue_gates: BTreeMap<String, QueueGate>,
}

pub(super) struct Store {
    pub path: PathBuf,
    pub data: Data,
    pub revision: u64,
}

impl Store {
    pub fn open(directory: &Path) -> anyhow::Result<Self> {
        let path = directory.join("remote-harness-state.json");
        let mut data: Data = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("远程会话服务状态文件损坏，未覆盖原文件")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(error) => return Err(error.into()),
        };
        // 用户要求重连后可继续操作；已派发但无回执的消息必须待核对，不能重发。
        for queue in data.queues.values_mut() {
            for message in queue
                .iter_mut()
                .filter(|message| matches!(message.state.as_str(), "sending" | "direct-sending"))
            {
                message.state = if message.state == "direct-sending" {
                    "direct-unknown"
                } else {
                    "unknown"
                }
                .into();
            }
        }
        // 用户要求异常退出后不自动续发：保留上一轮依赖，重启后重新核对正常完成凭据。
        // 旧版本没有记录依赖轮次的队列也需要核对，不能把恢复后的 idle 当成正常结束。
        for (id, queue) in &data.queues {
            if queue.iter().any(|message| message.state == "queued") {
                let gate = data
                    .queue_gates
                    .entry(id.clone())
                    .or_insert_with(|| QueueGate {
                        kind: "turn".into(),
                        phase: "recovering".into(),
                        ..Default::default()
                    });
                if gate.phase == "waiting" {
                    gate.phase = "recovering".into();
                }
            }
        }
        Ok(Self {
            path,
            data,
            revision: 1,
        })
    }

    pub fn commit<T>(
        &mut self,
        change: impl FnOnce(&mut Data) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut next = self.data.clone();
        let value = change(&mut next)?;
        next.queue_gates.retain(|id, _| {
            next.queues.get(id).is_some_and(|queue| {
                queue
                    .iter()
                    .any(|message| !message.state.starts_with("direct-"))
            })
        });
        while next.sent.len() > 4096 {
            next.sent.pop_first();
        }
        next.sent_hashes.retain(|id, _| next.sent.contains_key(id));
        // 已结束编辑只保留回执和内容指纹，不能长期重复保存原图片挤占待发容量。
        let completed: Vec<_> = next
            .edits
            .iter_mut()
            .filter(|(_, edit)| matches!(edit.state.as_str(), "committed" | "canceled"))
            .map(|(id, edit)| {
                edit.original.payload = json!({"prompt":""});
                id.clone()
            })
            .collect();
        for id in completed.iter().take(completed.len().saturating_sub(256)) {
            next.edits.remove(id);
        }
        while next.answered_questions.len() > 4096 {
            next.answered_questions.pop_first();
        }
        let bytes = serde_json::to_vec(&next)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "远程待发送内容已达 64 MiB，请先处理现有队列"
        );
        crate::settings::atomic_write(&self.path, &bytes)?;
        self.data = next;
        self.revision += 1;
        Ok(value)
    }

    pub fn queue(&self, thread_id: &str, epoch: &str) -> Value {
        self.data.queue(thread_id, epoch, self.revision)
    }
}

impl Data {
    pub fn queue(&self, thread_id: &str, epoch: &str, revision: u64) -> Value {
        let messages: Vec<_> = self
            .queues
            .get(thread_id)
            .into_iter()
            .flatten()
            .filter(|q| q.state != "editing" && !q.state.starts_with("direct-"))
            .map(|message| {
                let mut view = message.view();
                if let Some(gate) = self.queue_gates.get(thread_id)
                    && (message.state == "paused"
                        || message.state == "queued" && gate.phase == "recovering")
                    && !gate.reason.is_empty()
                {
                    view["pausedReason"] = json!(&gate.reason);
                }
                view
            })
            .collect();
        let edits: Vec<_> = self.edits.iter().filter(|(_, e)| e.thread_id == thread_id
            && !["committed","canceled"].contains(&e.state.as_str()))
            .map(|(id,e)| json!({"editId":id,"messageId":e.message_id,"state":e.state,
                "editState":e.state,"originalText":e.original.payload["prompt"],"createdAt":e.original.created_at})).collect();
        json!({"ok":true,"threadId":thread_id,"queueEpoch":epoch,"queueRevision":revision,
            "queuedMessages":messages,"queuedEdits":edits})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_store_restart_never_replays_an_unconfirmed_dispatch() {
        for state in ["sending", "direct-sending"] {
            let directory = tempfile::tempdir().unwrap();
            let mut store = Store::open(directory.path()).unwrap();
            store
                .commit(|data| {
                    data.queues.insert(
                        "thread-a".into(),
                        vec![Queued {
                            id: "message-a".into(),
                            payload: json!({"prompt":"test","clientUserMessageId":"client-a"}),
                            created_at: 1,
                            device_id: "device-a".into(),
                            key_id: "key-a".into(),
                            state: state.into(),
                            request_id: None,
                            runtime_revision: None,
                        }],
                    );
                    Ok(())
                })
                .unwrap();
            let reopened = Store::open(directory.path()).unwrap();
            assert_eq!(
                reopened.data.queues["thread-a"][0].state,
                if state == "sending" {
                    "unknown"
                } else {
                    "direct-unknown"
                }
            );
            let view = reopened.queue("thread-a", "epoch");
            if state == "direct-sending" {
                assert!(
                    view["queuedMessages"].as_array().unwrap().is_empty(),
                    "直接发送意图重启后继续只读核对，不能变成待发送队列"
                );
                continue;
            }
            assert_eq!(view["queuedMessages"][0]["canSendNow"], false);
            assert_eq!(
                view["queuedMessages"][0]["submission"]["status"],
                "outcome-unknown"
            );
        }
    }

    #[test]
    fn harness_store_failed_persistence_does_not_publish_a_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        std::fs::write(directory.path().join("blocked"), "file").unwrap();
        store.path = directory.path().join("blocked").join("state.json");
        let revision = store.revision;
        assert!(
            store
                .commit(|data| {
                    data.thread_pins.insert("thread-a".into());
                    Ok(())
                })
                .is_err()
        );
        assert!(store.data.thread_pins.is_empty());
        assert_eq!(store.revision, revision);
    }
}
