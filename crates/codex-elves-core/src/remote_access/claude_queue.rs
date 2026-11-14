//! 网页待发送队列与 Claude 已受理内容分开保存；只在实际投递时进入同一个原生会话。
use super::*;
use std::collections::HashSet;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct RemoteQueue {
    messages: Vec<Message>,
    edits: BTreeMap<String, Edit>,
    accepted: BTreeMap<String, String>,
    revision: u64,
    paused: bool,
    awaiting_turn: Option<String>,
    awaiting_started: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    id: String,
    payload: Value,
    created_at: u64,
    device_id: String,
    key_id: String,
    state: String,
    // 直接发送的在途/未知记录仅用于核对原回执；只有原生明确忙碌后才成为可见等待项。
    #[serde(default)]
    direct: bool,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    native_intent: Option<Value>,
    // 历史编辑仅复用直接发送的在途回执；定位由后端核对生成，不接收网页指定的原生路径。
    #[serde(default)]
    native_edit: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Edit {
    original: Message,
    state: String,
    fingerprint: Option<String>,
    result: Value,
}

fn fingerprint(payload: &Value) -> String {
    super::super::hash(&serde_json::to_string(payload).unwrap_or_default())
}

fn image_source(image: &Value) -> String {
    use sha2::{Digest, Sha256};
    image["dataUrl"]
        .as_str()
        .and_then(|s| super::super::decode_data_image(s, super::super::MAX_INPUT_IMAGE_BYTES).ok())
        .map(|i| format!("inline:{:x}", Sha256::digest(&i.bytes)))
        .unwrap_or_default()
}

fn edit_fingerprint(action: &str, payload: &Value) -> String {
    let images: Vec<_> = payload["images"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| json!([i["name"], i["mime"], i["dataUrl"], i["size"]]))
        .collect();
    fingerprint(&json!([
        action,
        if action == "editQueued" {
            payload["prompt"].as_str().map(str::trim)
        } else {
            None
        },
        payload["retainedImageSources"],
        images
    ]))
}

fn required<'a>(payload: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    payload[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error(400, format!("缺少 {key}")))
}

impl Message {
    fn view(&self, paused: bool) -> Value {
        let images: Vec<_> = self.payload["images"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| json!({"src":i["dataUrl"],"name":i["name"]}))
            .collect();
        json!({"id":self.id,"clientUserMessageId":self.payload["clientUserMessageId"],
            "text":self.payload["prompt"],"createdAt":self.created_at,"remoteQueueSource":"remote",
            "canSendNow":matches!(self.state.as_str(),"queued"|"paused"),
            "pausedReason":if self.reason.is_empty() {
                if paused {json!("队列已暂停")} else {Value::Null}
            } else {json!(self.reason)},
            "remoteDeliveryState":if self.state=="unknown" {"outcome-unknown"} else {"queued"},
            "submission":{"status":match self.state.as_str() {
                "sending"=>"sending","unknown"=>"outcome-unknown",_=>"queued"}},
            "context":{"prompt":self.payload["prompt"],"imageAttachments":images},
            "submissionOptions":{"clientUserMessageId":self.payload["clientUserMessageId"],
                "model":self.payload["model"],"reasoningEffort":self.payload["reasoningEffort"]}})
    }
}

impl RemoteQueue {
    pub(super) fn pause(&mut self) {
        self.paused = true;
        self.revision += 1;
    }

    pub(super) fn changed(&mut self) {
        self.revision += 1;
    }

    pub(super) fn observe_turns(&mut self, turns: &[Value], status: &Value) {
        let Some(client) = self.awaiting_turn.as_deref() else {
            return;
        };
        let Some(index) = turns.iter().position(|t| t["id"] == client) else {
            return;
        };
        let turn = &turns[index];
        // 用户要求引导完成后队列自然续发：CLI 可能把完成回执记在后续提示轮次。
        // 同一顺序历史中后续轮次已完成，也确认此前受理请求结束；不按提示文案识别，不解除既有暂停。
        if let Some(ended) = turns[index..]
            .iter()
            .find(|turn| turn["_nativeDone"] == true || turn["status"] == "failed")
        {
            self.awaiting_turn = None;
            // 原生中断也带结束标记，只有正常完成允许续发；中断或失败必须保持暂停，不能误启动下一条。
            if (matches!(ended["status"].as_str(), Some("interrupted" | "failed"))
                || ended["_nativeEndedWithoutReply"] == true)
                && !self.paused
            {
                // 接收后被原生结束且没有回复时停止自动续发；下一条用户主动发送仍按真实空闲状态直接投递。
                self.paused = true;
                self.revision += 1;
            }
        } else if matches!(status["tempo"].as_str(), Some("active" | "blocked")) {
            self.awaiting_started = true;
        } else if self.awaiting_started
            && status["tempo"] == "idle"
            && turn["status"] == "interrupted"
        {
            // 原版 CLI 也可能停止当前轮次；只有真正完成才能自动续发，原生中断后保留暂停。
            self.awaiting_turn = None;
            self.paused = true;
            self.revision += 1;
        }
    }

    fn compact(&mut self) {
        // 编辑结束只保留结果和指纹；图片不在编辑回执中重复保存，最多保留 256 个已结束编辑。
        let ended: Vec<_> = self
            .edits
            .iter_mut()
            .filter(|(_, e)| e.state != "editing")
            .map(|(id, e)| {
                e.original.payload = Value::Null;
                id.clone()
            })
            .collect();
        for id in ended.iter().take(ended.len().saturating_sub(256)) {
            self.edits.remove(id);
        }
        while self.accepted.len() > 4096 {
            self.accepted.pop_first();
        }
    }

    pub(super) fn remember_intent(&mut self, intent: &Value) {
        if let Some(message) = self
            .messages
            .iter_mut()
            .find(|m| intent["clientUserMessageId"] == m.id && m.state == "sending")
        {
            message.native_intent = Some(intent.clone());
        }
    }
    pub(super) fn recover(&mut self) {
        // 用户要求断线不丢消息；派发中断只能待核对，服务重启绝不能把未知结果恢复成可自动发送。
        for message in &mut self.messages {
            if message.state == "sending" {
                message.state = "unknown".into();
                message.reason = "消息送达结果待核对，未自动重发".into();
            }
        }
    }

    pub(super) fn native_intents(&self) -> Vec<Value> {
        self.messages
            .iter()
            .filter_map(|m| m.native_intent.clone())
            .collect()
    }

    pub(super) fn reconcile(&mut self, clients: &BTreeMap<String, String>, pending: &[Value]) {
        let before = self.messages.len();
        self.messages.retain(|message| {
            let client = message.payload["clientUserMessageId"]
                .as_str()
                .unwrap_or("");
            let received = clients.values().any(|id| id == client)
                || pending
                    .iter()
                    .any(|p| p["clientUserMessageId"] == client && p["deliveryConfirmed"] != false);
            if received && matches!(message.state.as_str(), "sending" | "unknown") {
                self.accepted
                    .insert(client.into(), fingerprint(&message.payload));
                false
            } else {
                true
            }
        });
        if self.messages.len() != before {
            self.revision += 1;
        }
        while self.accepted.len() > 4096 {
            self.accepted.pop_first();
        }
    }

    pub(super) fn messages(&self, native: &[Value]) -> Vec<Value> {
        let mut messages: Vec<_> = native
            .iter()
            .filter(|p| {
                !self
                    .messages
                    .iter()
                    .any(|m| m.payload["clientUserMessageId"] == p["clientUserMessageId"])
            })
            .cloned()
            .map(|mut p| {
                // 旧版已经投递的原生队列只能确认接收，不能转成网页待发送后再投递一次。
                p["remoteQueueSource"] = json!("native");
                p["remoteDeliveryState"] = json!("accepted");
                p["canSendNow"] = json!(false);
                p["submission"] = json!({"status":"accepted"});
                p
            })
            .collect();
        messages.extend(
            self.messages
                .iter()
                .filter(|m| m.state != "editing" && !m.direct)
                .map(|m| m.view(self.paused)),
        );
        messages
    }

    pub(super) fn edits(&self) -> Vec<Value> {
        self.edits
            .iter()
            .filter(|(_, e)| e.state == "editing")
            .map(|(id, e)| {
                json!({"editId":id,"messageId":e.original.id,"state":e.state,
                "originalText":e.original.payload["prompt"],"createdAt":e.original.created_at})
            })
            .collect()
    }
}

impl Conversation {
    pub(super) fn queue_snapshot(&self, epoch: &str) -> Value {
        json!({"ok":true,"threadId":self.id,"queueEpoch":epoch,
            "queueRevision":self.remote_queue.revision,
            "queuedMessages":self.remote_queue.messages(&self.queued_messages),
            "queuedEdits":self.remote_queue.edits(),"queuePaused":self.remote_queue.paused})
    }

    fn can_auto_send(&self) -> bool {
        self.shared
            && !self.deleted
            && !self.active()
            && self.queued_messages.is_empty()
            && !self.remote_queue.paused
            && self.remote_queue.awaiting_turn.is_none()
            && self
                .remote_queue
                .messages
                // 共用输入框按用户要求把取回内容视为草稿；只跳过编辑槽位，不能跳过暂停或未知消息。
                .iter()
                .find(|m| m.state != "editing")
                .is_some_and(|m| m.state == "queued" && !m.direct)
            && (self.native_status["tempo"] == "idle"
                || self.native_status["tempo"] == "blocked"
                    && self.native_status["needs"].is_null()
                    && self
                        .turns
                        .last()
                        .is_some_and(|t| t["status"] == "completed"))
    }
}

impl Claude {
    pub(in crate::remote_access) async fn queue_send_state(
        &self,
        payload: &Value,
    ) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let client = required(payload, "clientUserMessageId")?;
        let c = self.conversation(id).await?;
        let message = c.remote_queue.messages.iter().find(|m| m.id == client);
        let accepted = c.remote_queue.accepted.contains_key(client)
            || c.native_clients.values().any(|id| id == client)
            || c.queued_messages
                .iter()
                .any(|m| m["clientUserMessageId"] == client);
        let queued = message.is_some_and(|m| {
            !m.direct && matches!(m.state.as_str(), "queued" | "paused" | "editing")
        });
        let mut result = c.queue_snapshot(&self.epoch);
        result["clientUserMessageId"] = json!(client);
        result["confirmed"] = json!(accepted || queued);
        result["deliveryState"] = json!(if accepted {
            "accepted"
        } else if queued {
            "queued"
        } else {
            "unknown"
        });
        result["messageId"] = json!(message.map(|m| m.id.as_str()).unwrap_or(client));
        Ok(result)
    }

    pub(super) async fn remote_queue_changed(&self, id: &str) {
        if let Ok(snapshot) = self.queue(id).await {
            self.publish(json!({"method":"thread/queue/changed","params":snapshot}));
        }
    }

    pub(in crate::remote_access) async fn queue_edit_state(
        &self,
        payload: &Value,
    ) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let edit_id = required(payload, "editId")?;
        let c = self.conversation(id).await?;
        let edit = c.remote_queue.edits.get(edit_id);
        let mut result = c.queue_snapshot(&self.epoch);
        result["editId"] = json!(edit_id);
        result["editState"] = json!(edit.map(|e| e.state.as_str()).unwrap_or("missing"));
        result["canResume"] = json!(edit.is_some_and(|e| e.state == "editing"));
        result["canDismiss"] = json!(edit.is_none_or(|e| e.state != "editing"));
        result["operationPending"] = json!(false);
        result["operationFingerprint"] = edit.and_then(|e| e.fingerprint.clone()).into();
        result["result"] = edit.map(|e| e.result.clone()).unwrap_or(Value::Null);
        Ok(result)
    }

    pub(super) async fn enqueue_shared(
        &self,
        id: &str,
        process: &Process,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        self.enqueue_shared_input(id, process, payload, None, authorization)
            .await
    }

    pub(super) async fn enqueue_shared_edit(
        &self,
        id: &str,
        process: &Process,
        payload: &Value,
        native_edit: Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        self.enqueue_shared_input(id, process, payload, Some(native_edit), authorization)
            .await
    }

    async fn enqueue_shared_input(
        &self,
        id: &str,
        process: &Process,
        payload: &Value,
        native_edit: Option<Value>,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        input_content(payload)?;
        let c = self.conversation(id).await?;
        let catalog = normalize_models(&process.initialization);
        let (model, effort) = model_selection(&c, payload, &catalog)?;
        let mut payload = payload.clone();
        let client_id = payload["clientUserMessageId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        anyhow::ensure!(
            client_id.len() <= 128
                && client_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "消息标识无效"
        );
        payload["clientUserMessageId"] = json!(client_id);
        payload["model"] = json!(model);
        payload["reasoningEffort"] = json!(effort);
        let payload_hash = fingerprint(&payload);
        // 用户要求以当前执行状态决定排队：发送前刷新同一连接的原生快照，不能沿用上轮 active 缓存。
        // 此处仅查询；失败时本次消息尚未准入，必须保留草稿，不能转成送达未知或创建队列项。
        process
            .control(json!({"subtype":"shared_sync"}), Some(authorization))
            .await
            .map_err(|failure| {
                if failure
                    .downcast_ref::<ApiError>()
                    .is_some_and(|e| matches!(e.code, 401 | 403))
                {
                    failure
                } else {
                    error(503, "暂时无法核对 Claude 当前执行状态，本次消息尚未投递")
                }
            })?;
        authorization.check().await?;
        let direct = self
            .change(|saved| {
                let c = saved
                    .conversations
                    .get_mut(id)
                    .context("Claude 会话不存在")?;
                if let Some(hash) = c.remote_queue.accepted.get(&client_id) {
                    anyhow::ensure!(hash == &payload_hash, "相同消息标识的内容已变化");
                    return Ok(false);
                }
                if let Some(existing) = c.remote_queue.messages.iter().find(|m| m.id == client_id) {
                    anyhow::ensure!(
                        fingerprint(&existing.payload) == payload_hash,
                        "相同消息标识的内容已变化"
                    );
                    if existing.direct || matches!(existing.state.as_str(), "sending" | "unknown") {
                        return Err(ApiError::new(409, "原消息送达状态待核对，不会重复发送")
                            .after_admission()
                            .into());
                    }
                    return Ok(false);
                }
                // 普通消息只在确认执行中时排队；旧队列、awaiting 标记或未知状态不能替代执行证据。
                if native_edit.is_some()
                    && (c.active()
                        || c.native_status["tempo"] != "idle"
                        || !c.native_status["needs"].is_null())
                {
                    return Err(error(409, "请等待 Claude 结束当前执行后再编辑历史消息"));
                }
                let direct = native_edit.is_some() || !c.running_for_send();
                if direct
                    && c.remote_queue
                        .messages
                        .iter()
                        .any(|m| matches!(m.state.as_str(), "sending" | "unknown"))
                {
                    return Err(error(409, "请先核对尚未确认的消息"));
                }
                c.remote_queue.messages.push(Message {
                    id: client_id.clone(),
                    payload: payload.clone(),
                    created_at: now_ms(),
                    device_id: authorization.device.id.clone(),
                    key_id: authorization.device.key_id.clone(),
                    state: "queued".into(),
                    direct,
                    reason: String::new(),
                    native_intent: None,
                    native_edit: native_edit.clone(),
                });
                if c.remote_queue.paused && !c.active() && c.queued_messages.is_empty() {
                    c.remote_queue.awaiting_turn = None;
                }
                c.remote_queue.paused = false;
                c.remote_queue.revision += 1;
                c.updated_at = now_ms();
                Ok(direct)
            })
            .await?;
        if direct {
            return self
                .dispatch_remote(id, &client_id, process, false, authorization, true)
                .await;
        }
        self.remote_queue_changed(id).await;
        let c = self.conversation(id).await?;
        let mut result = c.queue_snapshot(&self.epoch);
        result["confirmed"] = json!(true);
        result["clientUserMessageId"] = json!(client_id);
        result["messageId"] = json!(client_id);
        result["deliveryState"] = json!(if c.remote_queue.accepted.contains_key(&client_id) {
            "accepted"
        } else {
            "queued"
        });
        Ok(result)
    }

    pub(super) async fn dispatch_remote(
        &self,
        id: &str,
        message_id: &str,
        process: &Process,
        steer: bool,
        authorization: &RemoteAuthorization,
        remove_if_unsent: bool,
    ) -> anyhow::Result<Value> {
        let admission = async {
            authorization.check().await?;
            self.change(|saved| {
                let c = saved
                    .conversations
                    .get_mut(id)
                    .context("Claude 会话不存在")?;
                anyhow::ensure!(
                    !c.remote_queue
                        .messages
                        .iter()
                        .any(|m| matches!(m.state.as_str(), "sending" | "unknown")),
                    "请先核对尚未确认的消息"
                );
                let message = c
                    .remote_queue
                    .messages
                    .iter_mut()
                    .find(|m| m.id == message_id)
                    .ok_or_else(|| error(409, "队列消息已变化"))?;
                anyhow::ensure!(
                    matches!(message.state.as_str(), "queued" | "paused"),
                    "此消息正在编辑或发送"
                );
                message.state = "sending".into();
                message.reason.clear();
                let result = message.clone();
                c.remote_queue.paused = false;
                c.remote_queue.awaiting_turn = Some(message_id.into());
                c.remote_queue.awaiting_started = false;
                c.remote_queue.revision += 1;
                Ok(result)
            })
            .await
        }
        .await;
        let message = match admission {
            Ok(message) => message,
            Err(failure) => {
                if remove_if_unsent {
                    // 直接发送尚未准入时失败（含授权或并发核对），不能留下新建的等待项。
                    self.change(|saved| {
                        if let Some(c) = saved.conversations.get_mut(id) {
                            c.remote_queue.messages.retain(|m| {
                                m.id != message_id
                                    || !matches!(m.state.as_str(), "queued" | "paused")
                            });
                            c.remote_queue.revision += 1;
                        }
                        Ok(())
                    })
                    .await?;
                }
                return Err(failure);
            }
        };
        // 用户要求即时发送未成功时不能留下队列项；首次投递先记录送达意图，确认排队后再通知页面。
        if !remove_if_unsent {
            self.remote_queue_changed(id).await;
        }
        let mut edit_settings = None;
        let result = async {
            let mut c = self.conversation(id).await?;
            // 用户要求权限不能阻断消息：普通投递交由连接端确认或登记待生效，原生审批和草稿继续保留；
            // 显式 next 引导仍不改写当前轮次权限。
            if !steer {
                apply_permissions(process, &mut c, &message.payload, authorization).await?;
            }
            let catalog = normalize_models(&process.initialization);
            let (model, effort) = model_selection(&c, &message.payload, &catalog)?;
            if let Some(edit) = &message.native_edit {
                let (model, effort) = (model.to_owned(), effort.to_owned());
                edit_settings = Some((model.clone(), effort.clone()));
                return process
                    .control(
                        json!({"subtype":"shared_edit_message","client_id":message.id,
                    "native_user_id":edit["native_user_id"],"history_head":edit["history_head"],
                    "prompt":message.payload["prompt"],"model":model,"effort":effort}),
                        Some(authorization),
                    )
                    .await;
            }
            let explicit_pending_setting = [("model", "model"), ("effort", "reasoningEffort")]
                .into_iter()
                .any(|(name, field)| {
                    message.payload.get(field).is_some()
                        && matches!(
                            c.native_control["settings"]["changes"][name]["state"].as_str(),
                            Some("pending" | "confirmation" | "confirming")
                        )
                });
            // 待确认目标与实际模型分开保存；用户明确重新选择原值时也须交给 CLI，不能按旧实际值跳过。
            if model != c.model || effort != c.effort || explicit_pending_setting {
                apply_model(process, &mut c, &message.payload, authorization).await?;
            }
            self.change(|saved| {
                let current = saved
                    .conversations
                    .get_mut(id)
                    .context("Claude 会话不存在")?;
                current.model = c.model;
                current.effort = c.effort;
                current.permission_mode = c.permission_mode;
                Ok(())
            })
            .await?;
            process
                .control(
                    json!({"subtype":"shared_send","client_id":message.id,
                    "content":input_content(&message.payload)?,"steer":steer,
                    // 普通直接发送允许由 CLI 保留到审批完成；只有网页队列的自动续发才要求原生空闲。
                    "idle_only":!steer && !remove_if_unsent}),
                    Some(authorization),
                )
                .await
        }
        .await;
        let not_sent = result.as_ref().err().is_some_and(|e| {
            e.downcast_ref::<ApiError>()
                .is_some_and(|e| !e.outcome_unknown)
                || e.downcast_ref::<crate::desktop_owner_ipc::DispatchFailure>()
                    .is_some_and(|e| !e.dispatched)
        });
        let deferred = result.as_ref().is_ok_and(|r| r["deferred"] == true);
        self.change(|saved| {
            let c = saved
                .conversations
                .get_mut(id)
                .context("Claude 会话不存在")?;
            if deferred {
                c.remote_queue.awaiting_turn = None;
                if let Some(m) = c
                    .remote_queue
                    .messages
                    .iter_mut()
                    .find(|m| m.id == message.id)
                {
                    m.state = "queued".into();
                    m.direct = false;
                }
            } else if result.is_ok() {
                if let Some((model, effort)) = &edit_settings {
                    c.model = model.clone();
                    c.effort = effort.clone();
                }
                c.remote_queue.messages.retain(|m| m.id != message.id);
                c.remote_queue
                    .accepted
                    .insert(message.id.clone(), fingerprint(&message.payload));
                // 原生 next 引导只确认消息投递；不能把正在继续执行或已经完成的旧轮次标成“已停止”。
            } else if not_sent && remove_if_unsent {
                // 明确未投递的新消息回到原输入框；已有待发消息的重试失败和送达未知仍保留原有核对机制。
                c.remote_queue.messages.retain(|m| m.id != message.id);
                c.remote_queue.awaiting_turn = None;
            } else if let Some(m) = c
                .remote_queue
                .messages
                .iter_mut()
                .find(|m| m.id == message.id)
            {
                m.state = if not_sent { "paused" } else { "unknown" }.into();
                m.reason = if not_sent {
                    "投递尚未开始，请核对后重试"
                } else {
                    "消息送达结果待核对，未自动重发"
                }
                .into();
                if not_sent {
                    c.remote_queue.awaiting_turn = None;
                }
            }
            c.remote_queue.revision += 1;
            c.remote_queue.compact();
            Ok(())
        })
        .await
        .map_err(|e| {
            dispatched_unknown(ApiError::new(503, format!("消息投递状态保存失败：{e}")))
        })?;
        self.remote_queue_changed(id).await;
        result?;
        let c = self.conversation(id).await?;
        let mut response = c.queue_snapshot(&self.epoch);
        response["confirmed"] = json!(true);
        response["messageId"] = json!(message.id);
        response["clientUserMessageId"] = json!(message.id);
        response["scope"] = json!("single");
        response["deliveryState"] = json!(if deferred { "queued" } else { "accepted" });
        response["executionTransport"] = json!("claude-wsl-shared");
        response["nativeId"] = json!(c.native_id);
        response["nativeJobId"] = json!(c.native_job_id);
        Ok(response)
    }

    pub(super) async fn drain_remote_queues(self: &Arc<Self>) {
        let candidates: Vec<_> = self
            .saved
            .lock()
            .await
            .conversations
            .values()
            .filter(|c| c.can_auto_send())
            .map(|c| c.id.clone())
            .collect();
        for id in candidates {
            let lock = self.lock(&id).await;
            let Ok(_guard) = lock.try_lock() else {
                continue;
            };
            let Ok(c) = self.conversation(&id).await else {
                continue;
            };
            if !c.can_auto_send() {
                continue;
            }
            let Some(process) = self
                .processes
                .lock()
                .await
                .get(&id)
                .filter(|p| p.channel.live.load(Ordering::Acquire))
                .cloned()
            else {
                continue;
            };
            // 重启后必须先收到本代连接的原生快照；旧的 idle 缓存不能触发自动发送。
            if self.observed.lock().await.get(&id) != Some(&process.channel.generation)
                || self
                    .requests
                    .lock()
                    .await
                    .get(&id)
                    .is_some_and(|r| !r.is_empty())
            {
                continue;
            }
            let Some(message) = c
                .remote_queue
                .messages
                .iter()
                .find(|m| m.state != "editing")
                .cloned()
            else {
                continue;
            };
            let Some(controller) = self.controller.get().and_then(Weak::upgrade) else {
                continue;
            };
            let device = controller
                .auth
                .lock()
                .await
                .devices
                .iter()
                .find(|d| d.id == message.device_id && d.key_id == message.key_id)
                .cloned();
            let Some(device) = device else { continue };
            let auth = controller.authorization(device, Some("send"));
            // 自动续发也逐次核对原设备授权；退出、只读或撤销后不能继续派发。
            if auth.check().await.is_err() {
                continue;
            }
            let _ = self
                .dispatch_remote(&id, &message.id, &process, false, &auth, false)
                .await;
        }
    }

    pub(super) async fn edit_remote_queue(
        &self,
        id: &str,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        authorization.check().await?;
        let mut response = self
            .change(|saved| {
                let c = saved
                    .conversations
                    .get_mut(id)
                    .context("Claude 会话不存在")?;
                let result = c.remote_queue.edit(action, payload)?;
                c.remote_queue.compact();
                c.remote_queue.revision += 1;
                Ok(result)
            })
            .await?;
        let c = self.conversation(id).await?;
        for (key, value) in c.queue_snapshot(&self.epoch).as_object().unwrap() {
            response[key] = value.clone();
        }
        self.remote_queue_changed(id).await;
        Ok(response)
    }
}

impl RemoteQueue {
    fn edit(&mut self, action: &str, payload: &Value) -> anyhow::Result<Value> {
        let mut response = json!({"ok":true});
        if action == "reorderQueued" {
            let ids = payload["messageIds"].as_array().context("队列顺序无效")?;
            let expected = payload["expectedMessageIds"]
                .as_array()
                .context("缺少队列顺序基线")?;
            anyhow::ensure!(
                ids.len() == self.messages.len()
                    && expected.len() == self.messages.len()
                    && self
                        .messages
                        .iter()
                        .zip(expected)
                        .all(|(m, id)| id == &json!(m.id))
                    && ids.iter().collect::<HashSet<_>>().len() == ids.len()
                    && self
                        .messages
                        .iter()
                        .all(|m| matches!(m.state.as_str(), "queued" | "paused"))
                    && ids
                        .iter()
                        .all(|id| self.messages.iter().any(|m| id == &json!(m.id))),
                "队列顺序已变化，请刷新后重试"
            );
            self.messages
                .sort_by_key(|m| ids.iter().position(|id| id == &json!(m.id)).unwrap());
            return Ok(response);
        }
        if matches!(
            action,
            "editQueued" | "cancelQueuedEdit" | "dismissQueuedEdit"
        ) {
            let edit_id = required(payload, "editId")?;
            let edit = self
                .edits
                .get(edit_id)
                .cloned()
                .ok_or_else(|| error(409, "队列编辑已失效"))?;
            if action == "dismissQueuedEdit" {
                anyhow::ensure!(
                    payload["confirmed"] == true && edit.state != "editing",
                    "编辑尚未结束"
                );
                self.edits.remove(edit_id);
                return Ok(json!({"ok":true,"editId":edit_id,"editState":"missing"}));
            }
            let operation = edit_fingerprint(action, payload);
            if edit.state != "editing" {
                anyhow::ensure!(
                    edit.fingerprint.as_deref() == Some(&operation),
                    "此次编辑已按其他内容完成"
                );
                return Ok(edit.result);
            }
            let message = self
                .messages
                .iter_mut()
                .find(|m| m.id == edit.original.id && m.state == "editing")
                .context("编辑中的消息已变化")?;
            if action == "cancelQueuedEdit" {
                *message = edit.original.clone();
            } else {
                let mut edited = message.payload.clone();
                edited["prompt"] = json!(
                    payload["prompt"]
                        .as_str()
                        .context("消息正文格式无效")?
                        .trim()
                );
                let original: Vec<_> = edit.original.payload["images"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .collect();
                let retained = payload["retainedImageSources"].as_array();
                // 用户要求最多 8 张原图完整编辑；每张只解码并计算一次摘要，校验与筛选复用，保留原图片顺序。
                let sources: Vec<_> = if retained.is_some() {
                    original
                        .iter()
                        .map(|image| json!(image_source(image)))
                        .collect()
                } else {
                    Vec::new()
                };
                if let Some(retained) = retained {
                    anyhow::ensure!(
                        retained.iter().all(|source| sources.contains(source))
                            && retained.iter().collect::<HashSet<_>>().len() == retained.len(),
                        "保留图片的来源已变化"
                    );
                }
                let mut images: Vec<_> = original
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        retained.is_none_or(|values| values.contains(&sources[*index]))
                    })
                    .map(|(_, image)| image.clone())
                    .collect();
                images.extend(payload["images"].as_array().into_iter().flatten().cloned());
                edited["images"] = json!(images);
                // 用户要求保留+新增仍共用 8 张、12 MiB 总量上限；HTTP 校验只看到新增图片，合并后必须再核对。
                super::super::validate_input_images(&edited)?;
                input_content(&edited)?;
                message.payload = edited;
            }
            message.state = "queued".into();
            message.reason.clear();
            let state = if action == "editQueued" {
                "committed"
            } else {
                "canceled"
            };
            response = json!({"ok":true,"editId":edit_id,"messageId":message.id,"editState":state});
            self.edits.insert(
                edit_id.into(),
                Edit {
                    original: edit.original,
                    state: state.into(),
                    fingerprint: Some(operation),
                    result: response.clone(),
                },
            );
            return Ok(response);
        }
        let message_id = required(payload, "messageId")?;
        let index = self
            .messages
            .iter()
            .position(|m| m.id == message_id)
            .ok_or_else(|| error(409, "此消息已交给 Claude，或队列已经变化"))?;
        anyhow::ensure!(
            matches!(self.messages[index].state.as_str(), "queued" | "paused"),
            "消息正在编辑、发送或等待核对"
        );
        if action == "removeQueued" {
            self.messages.remove(index);
            return Ok(response);
        }
        anyhow::ensure!(
            payload["originalText"] == self.messages[index].payload["prompt"]
                && payload["clientUserMessageId"]
                    == self.messages[index].payload["clientUserMessageId"]
                && payload["createdAt"] == self.messages[index].created_at,
            "队列消息的内容或身份已变化"
        );
        anyhow::ensure!(action == "beginQueuedEdit", "不支持的队列操作");
        let edit_id = required(payload, "editId")?;
        anyhow::ensure!(!self.edits.contains_key(edit_id), "编辑标识已使用");
        anyhow::ensure!(
            !self.edits.values().any(|e| e.state == "editing"),
            "请先完成当前队列编辑"
        );
        if let Some(expected) = payload["expectedImageSources"].as_array() {
            let images: Vec<_> = self.messages[index].payload["images"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| json!(image_source(i)))
                .collect();
            anyhow::ensure!(expected == &images, "队列图片已变化");
        }
        let original = self.messages[index].clone();
        self.messages[index].state = "editing".into();
        self.edits.insert(
            edit_id.into(),
            Edit {
                original,
                state: "editing".into(),
                fingerprint: None,
                result: Value::Null,
            },
        );
        Ok(json!({"ok":true,"editId":edit_id,"messageId":message_id,"editState":"editing"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Clone, Copy)]
    enum Reply {
        Accepted,
        Deferred,
        Rejected,
        ControlRejected,
        Unknown,
        SyncIdle,
        SyncActive,
        SyncRejected,
        PermissionPending,
    }

    async fn fixture() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Arc<Claude>,
        Conversation,
        RemoteAuthorization,
        Arc<Mutex<Vec<Value>>>,
        Arc<Mutex<VecDeque<Reply>>>,
    ) {
        let (directory, service, mut c) = super::super::tests::fixture();
        let (auth_directory, auth) = crate::remote_access::tests::service_dispatch_fixture().await;
        c.shared = true;
        c.native_job_id = c.native_id[..8].into();
        c.native_status = json!({"tempo":"active","state":"working"});
        service
            .change(|saved| {
                saved.conversations.insert(c.id.clone(), c.clone());
                Ok(())
            })
            .await
            .unwrap();
        service
            .controller
            .set(Arc::downgrade(&auth.controller))
            .ok();
        let channel = Arc::new(Channel {
            live: AtomicBool::new(true),
            generation: "queue-fixture".into(),
            pending: Mutex::new(HashMap::new()),
        });
        let (input, output) = tokio::io::duplex(64 * 1024);
        let (writer, _) = spawn_writer(input, WRITE_TIMEOUT);
        let process = Arc::new(Process {
            _child: Mutex::new(None),
            writer,
            channel: channel.clone(),
            shared: true,
            initialization: json!({"models":[{"value":"test-model","supportedEffortLevels":["low","max"]}]}),
        });
        service.processes.lock().await.insert(c.id.clone(), process);
        service
            .observed
            .lock()
            .await
            .insert(c.id.clone(), channel.generation.clone());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::<Reply>::new()));
        let (record, plan, weak, id) = (
            calls.clone(),
            replies.clone(),
            Arc::downgrade(&service),
            c.id.clone(),
        );
        tokio::spawn(async move {
            let mut lines = BufReader::new(output).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let frame: Value = serde_json::from_str(&line).unwrap();
                let request = &frame["request"];
                if request["subtype"] == "shared_sync" {
                    let next = {
                        let mut plan = plan.lock().await;
                        if matches!(
                            plan.front(),
                            Some(Reply::SyncIdle | Reply::SyncActive | Reply::SyncRejected)
                        ) {
                            plan.pop_front()
                        } else {
                            None
                        }
                    };
                    if let Some(state @ (Reply::SyncIdle | Reply::SyncActive)) = next
                        && let Some(service) = weak.upgrade()
                    {
                        service
                            .change(|saved| {
                                let c = saved.conversations.get_mut(&id).unwrap();
                                let active = matches!(state, Reply::SyncActive);
                                c.native_status =
                                    json!({"tempo":if active {"active"} else {"idle"}});
                                c.turns.last_mut().unwrap()["status"] =
                                    json!(if active { "inProgress" } else { "completed" });
                                Ok(())
                            })
                            .await
                            .unwrap();
                    }
                    if let Some(sender) = channel
                        .pending
                        .lock()
                        .await
                        .remove(frame["request_id"].as_str().unwrap())
                    {
                        let response = if matches!(next, Some(Reply::SyncRejected)) {
                            Err(ApiError::new(503, "状态查询失败").after_admission().into())
                        } else {
                            Ok(json!({}))
                        };
                        let _ = sender.send(response);
                    }
                    continue;
                }
                record.lock().await.push(request.clone());
                let reply = plan.lock().await.pop_front().unwrap_or(Reply::Accepted);
                if request["subtype"] == "shared_send"
                    && matches!(reply, Reply::Accepted)
                    && let Some(service) = weak.upgrade()
                {
                    service
                        .change(|saved| {
                            let c = saved.conversations.get_mut(&id).unwrap();
                            c.native_status = json!({"tempo":"active","state":"working"});
                            c.queued_messages.push(json!({"id":request["client_id"],
                            "clientUserMessageId":request["client_id"],"deliveryConfirmed":true}));
                            Ok(())
                        })
                        .await
                        .unwrap();
                }
                let response = match reply {
                    Reply::Accepted => Ok(json!({"accepted":true})),
                    Reply::Deferred => Ok(json!({"deferred":true,"accepted":false})),
                    Reply::Rejected => Err(ApiError::new(409, "尚未投递").into()),
                    Reply::ControlRejected => Err(ApiError::new(
                        503,
                        "Claude 控制请求失败：原生会话仍在其他 CLI 中打开",
                    )
                    .into()),
                    Reply::Unknown => Err(ApiError::new(503, "回执未知").after_admission().into()),
                    Reply::PermissionPending => Ok(json!({"permissionPending":true,
                        "nativeSettings":{"permissionMode":"acceptEdits","changes":{
                            "permissionMode":{"id":"pending-permission","requested":"configured",
                            "state":"pending","reason":"原生正在等待审批"}}}})),
                    Reply::SyncIdle | Reply::SyncActive | Reply::SyncRejected => unreachable!(),
                };
                if let Some(sender) = channel
                    .pending
                    .lock()
                    .await
                    .remove(frame["request_id"].as_str().unwrap())
                {
                    let _ = sender.send(response);
                }
            }
        });
        (directory, auth_directory, service, c, auth, calls, replies)
    }

    fn payload(c: &Conversation, id: &str) -> Value {
        json!({"threadId":c.id,"clientUserMessageId":id,"prompt":format!("正文-{id}")})
    }

    #[tokio::test]
    async fn permission_pending_during_native_approval_does_not_block_direct_send() {
        let (_directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
        service
            .change(|saved| {
                let current = saved.conversations.get_mut(&c.id).unwrap();
                current.native_status = json!({"tempo":"blocked","sessionStatus":"waiting",
                    "needs":"permission prompt"});
                current.permission_mode = Some("plan".into());
                Ok(())
            })
            .await
            .unwrap();
        replies
            .lock()
            .await
            .extend([Reply::PermissionPending, Reply::Accepted]);
        let mut input = payload(&c, "permission-followup");
        input["permissionMode"] = json!("configured");
        let result = service.command("send", &input, &auth).await.unwrap();
        assert_eq!(result["deliveryState"], "accepted");
        let calls = calls.lock().await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["subtype"], "set_permission_mode");
        assert_eq!(calls[1]["subtype"], "shared_send");
        assert_eq!(calls[1]["idle_only"], false);
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.permission_mode.as_deref(), Some("plan"));
        assert_eq!(current.turns[0]["status"], "inProgress");
        assert_ne!(current.turns[0]["_stopRequested"], true);
    }

    #[tokio::test]
    async fn permission_change_while_running_preserves_the_turn_and_reports_pending_state() {
        let (_directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
        replies.lock().await.push_back(Reply::PermissionPending);
        let result = service
            .set_permissions(&c.id, "configured", &auth)
            .await
            .unwrap();
        assert_eq!(result["permissionPending"], true);
        assert_eq!(result["appliesTo"], "whenReady");
        assert_eq!(result["nativePermissionMode"], "acceptEdits");
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.permission_mode.as_deref(), Some("configured"));
        assert_eq!(current.turns[0]["status"], "inProgress");
        assert_eq!(
            current.native_control["settings"]["changes"]["permissionMode"]["state"],
            "pending"
        );
        assert_eq!(calls.lock().await.len(), 1);
        assert_eq!(calls.lock().await[0]["subtype"], "set_permission_mode");
    }

    #[test]
    fn queue_image_edit_preserves_selected_original_and_rejects_foreign_sources() {
        let images: Vec<_> = (0..3).map(|color| {
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(1, 1, image::Rgb([color, 0, 0])))
                .write_to(&mut png, image::ImageFormat::Png).unwrap();
            json!({"name":format!("{color}.png"),"mime":"image/png","size":png.get_ref().len(),
                "dataUrl":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png.get_ref()))})
        }).collect();
        let original = Message {
            id: "images".into(),
            payload: json!({"prompt":"原图","clientUserMessageId":"images",
            "images":[images[0],images[1]]}),
            created_at: 1,
            device_id: "device".into(),
            key_id: "key".into(),
            state: "queued".into(),
            direct: false,
            reason: String::new(),
            native_intent: None,
            native_edit: None,
        };
        let mut queue = RemoteQueue {
            messages: vec![original.clone()],
            ..Default::default()
        };
        queue.edit("beginQueuedEdit", &json!({"messageId":"images","editId":"edit-images","clientUserMessageId":"images",
            "originalText":"原图","createdAt":1,"expectedImageSources":[image_source(&images[0]),image_source(&images[1])]})).unwrap();
        assert!(
            queue
                .edit(
                    "editQueued",
                    &json!({"editId":"edit-images","prompt":"修改",
            "retainedImageSources":["inline:foreign"]})
                )
                .is_err()
        );
        assert_eq!(queue.messages[0].payload, original.payload);
        queue
            .edit(
                "editQueued",
                &json!({"editId":"edit-images","prompt":"修改",
            "retainedImageSources":[image_source(&images[1])],"images":[images[2]]}),
            )
            .unwrap();
        assert_eq!(
            queue.messages[0].payload["images"],
            json!([images[1], images[2]])
        );
    }

    #[test]
    fn editing_queue_validates_combined_retained_and_new_image_size_without_losing_original() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut bytes = png.into_inner();
        bytes.resize(4 * 1024 * 1024, 0);
        let image = json!({"name":"image.png","mime":"image/png","size":bytes.len(),
            "dataUrl":format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))});
        let payload = json!({"clientUserMessageId":"image-message","prompt":"原内容","images":[image.clone(),image.clone(),image.clone()]});
        super::super::super::validate_input_images(&payload).unwrap();
        let original = Message {
            id: "image-message".into(),
            payload: payload.clone(),
            created_at: 1,
            device_id: "device".into(),
            key_id: "key".into(),
            state: "queued".into(),
            direct: false,
            reason: String::new(),
            native_intent: None,
            native_edit: None,
        };
        let mut queue = RemoteQueue {
            messages: vec![original],
            ..Default::default()
        };
        queue
            .edit(
                "beginQueuedEdit",
                &json!({"messageId":"image-message","editId":"image-edit",
            "originalText":"原内容","clientUserMessageId":"image-message","createdAt":1}),
            )
            .unwrap();
        let failure = queue
            .edit(
                "editQueued",
                &json!({"editId":"image-edit","prompt":"修改",
            "images":[image.clone()]}),
            )
            .unwrap_err();
        assert_eq!(failure.downcast_ref::<ApiError>().unwrap().code, 413);
        assert_eq!(queue.messages[0].payload, payload);
        assert_eq!(queue.messages[0].state, "editing");
        queue
            .edit(
                "editQueued",
                &json!({"editId":"image-edit","prompt":"移除旧图后保存",
            "retainedImageSources":[],"images":[image]}),
            )
            .unwrap();
        assert_eq!(
            queue.messages[0].payload["images"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn remote_queue_holds_multiple_messages_and_steers_only_selected_message() {
        let (directory, _auth_directory, service, c, auth, calls, _) = fixture().await;
        for id in ["B", "C", "D"] {
            let result = service
                .command("send", &payload(&c, id), &auth)
                .await
                .unwrap();
            assert_eq!(result["deliveryState"], "queued");
        }
        assert!(
            calls.lock().await.is_empty(),
            "执行中排队不能触碰原生输入或模型设置"
        );
        let restored = Claude::new(directory.path()).unwrap();
        assert_eq!(
            restored.queue(&c.id).await.unwrap()["queuedMessages"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        let result = service
            .command(
                "steerQueued",
                &json!({"threadId":c.id,"messageId":"C"}),
                &auth,
            )
            .await
            .unwrap();
        assert_eq!(result["scope"], "single");
        let sent = calls.lock().await.clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["subtype"], "shared_send");
        assert_eq!(sent[0]["client_id"], "C");
        assert_eq!(sent[0]["steer"], true);
        assert_eq!(sent[0]["idle_only"], false);
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.turns[0]["status"], "inProgress");
        assert_ne!(
            current.turns[0]["_stopRequested"], true,
            "原生引导不能被解释为停止旧轮次"
        );
        assert_eq!(
            current
                .remote_queue
                .messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["B", "D"]
        );
        assert_eq!(current.native_id, c.native_id);
        let old_native = service.queue(&c.id).await.unwrap();
        assert_eq!(
            old_native["queuedMessages"][0]["remoteDeliveryState"],
            "accepted"
        );
        assert!(
            service
                .command(
                    "removeQueued",
                    &json!({"threadId":c.id,"messageId":"C"}),
                    &auth
                )
                .await
                .is_err()
        );
    }

    #[test]
    fn compact_summary_completion_releases_the_real_awaiting_turn_without_pausing() {
        let frame = json!({"clients":{"native-user":"web-client"},"native_status":{"tempo":"idle"},"messages":[
            {"type":"user","uuid":"native-user","message":{"content":"真实消息"}},
            {"type":"user","uuid":"compact","isCompactSummary":true,"isVisibleInTranscriptOnly":true,
                "message":{"content":"内部摘要"}},
            {"type":"assistant","message":{"id":"reply","content":[{"type":"text","text":"完成回复"}]}},
            {"type":"system","subtype":"turn_duration","timestampMs":100}
        ]});
        let turns = super::super::project_shared_history(&frame, "reconnected");
        let mut queue = RemoteQueue {
            awaiting_turn: Some("web-client".into()),
            awaiting_started: true,
            ..Default::default()
        };
        queue.observe_turns(&turns, &frame["native_status"]);
        assert!(queue.awaiting_turn.is_none());
        assert!(!queue.paused, "真实完成后现有队列可自然续发，不误判为中断");
    }

    #[test]
    fn native_empty_end_clears_waiting_but_does_not_auto_send_the_next_message() {
        let mut queue = RemoteQueue {
            awaiting_turn: Some("web-client".into()),
            awaiting_started: false,
            ..Default::default()
        };
        queue.observe_turns(
            &[json!({"id":"web-client","status":"completed",
            "_nativeDone":true,"_nativeEndedWithoutReply":true})],
            &json!({"tempo":"idle"}),
        );
        assert!(queue.awaiting_turn.is_none());
        assert!(queue.paused, "无回复的原生结束不能当作正常回复后自动续发");
    }

    #[test]
    fn later_native_completion_releases_prior_request_without_resuming_paused_queue() {
        let mut turns = vec![
            json!({"id":"older","status":"completed","_nativeDone":true}),
            json!({"id":"web-client","status":"completed"}),
            json!({"id":"native-notice","status":"completed"}),
        ];
        let idle = json!({"tempo":"idle"});
        let mut queue = RemoteQueue {
            awaiting_turn: Some("web-client".into()),
            awaiting_started: true,
            ..Default::default()
        };
        queue.observe_turns(&turns, &idle);
        assert_eq!(
            queue.awaiting_turn.as_deref(),
            Some("web-client"),
            "更早的完成回执不能释放当前请求"
        );
        turns[2]["_nativeDone"] = json!(true);
        queue.observe_turns(&turns, &idle);
        assert!(
            queue.awaiting_turn.is_none(),
            "后续原生提示轮次的真实完成回执也应释放此前受理请求"
        );
        assert!(!queue.paused);

        queue.awaiting_turn = Some("web-client".into());
        queue.paused = true;
        queue.observe_turns(&turns, &idle);
        assert!(queue.awaiting_turn.is_none());
        assert!(queue.paused, "完成确认不能解除用户已暂停的队列");

        queue.awaiting_turn = Some("web-client".into());
        queue.paused = false;
        turns[2].as_object_mut().unwrap().remove("_nativeDone");
        turns[1]["status"] = json!("interrupted");
        queue.observe_turns(&turns, &idle);
        assert!(queue.awaiting_turn.is_none());
        assert!(queue.paused, "没有后续完成回执的原生中断仍须暂停");
    }

    #[tokio::test]
    async fn remote_queue_unknown_survives_restart_and_never_becomes_retryable() {
        let (directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
        service
            .command("send", &payload(&c, "B"), &auth)
            .await
            .unwrap();
        replies.lock().await.push_back(Reply::Unknown);
        assert!(
            service
                .command(
                    "steerQueued",
                    &json!({"threadId":c.id,"messageId":"B"}),
                    &auth
                )
                .await
                .is_err()
        );
        let restored = Claude::new(directory.path()).unwrap();
        let message = restored.queue(&c.id).await.unwrap()["queuedMessages"][0].clone();
        assert_eq!(message["submission"]["status"], "outcome-unknown");
        assert_eq!(message["canSendNow"], false);
        assert!(
            service
                .command(
                    "steerQueued",
                    &json!({"threadId":c.id,"messageId":"B"}),
                    &auth
                )
                .await
                .is_err()
        );
        assert_eq!(calls.lock().await.len(), 1);
        service
            .change(|saved| {
                let c = saved.conversations.get_mut(&c.id).unwrap();
                c.native_clients.insert("native-B".into(), "B".into());
                c.remote_queue.reconcile(&c.native_clients, &[]);
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            service
                .conversation(&c.id)
                .await
                .unwrap()
                .remote_queue
                .messages
                .is_empty()
        );
    }

    #[test]
    fn native_interruption_completion_marker_does_not_resume_the_queue() {
        let mut queue = RemoteQueue {
            awaiting_turn: Some("client".into()),
            awaiting_started: true,
            ..Default::default()
        };
        queue.observe_turns(
            &[
                json!({"id":"client","status":"interrupted","_nativeDone":true,
                "nativeInterruption":{"source":"claude"}}),
            ],
            &json!({"tempo":"idle"}),
        );
        assert!(queue.awaiting_turn.is_none());
        assert!(queue.paused, "已结束的中断轮次不能被当成正常完成后自动续发");
    }

    #[tokio::test]
    async fn only_confirmed_running_enqueues_new_messages() {
        for state in [
            "idle",
            "unknown",
            "unknown-stale-history",
            "old-queue",
            "stale-history",
            "blocked",
        ] {
            let (_directory, _auth_directory, service, c, auth, calls, _replies) = fixture().await;
            if state == "old-queue" {
                service
                    .command("send", &payload(&c, "old"), &auth)
                    .await
                    .unwrap();
                assert!(calls.lock().await.is_empty(), "明确执行中应入队");
            }
            service
                .change(|saved| {
                    let current = saved.conversations.get_mut(&c.id).unwrap();
                    current.native_status = match state {
                        "unknown" | "unknown-stale-history" => json!({}),
                        "blocked" => json!({"tempo":"blocked","needs":{"kind":"clarification"}}),
                        _ => json!({"tempo":"idle"}),
                    };
                    if !matches!(state, "stale-history" | "unknown-stale-history") {
                        current.turns.last_mut().unwrap()["status"] = json!("completed");
                    }
                    if state == "old-queue" {
                        current.queued_messages.push(json!({"id":"native-old"}));
                        current.remote_queue.awaiting_turn = Some("old".into());
                    }
                    Ok(())
                })
                .await
                .unwrap();
            let result = service
                .command("send", &payload(&c, "new"), &auth)
                .await
                .unwrap();
            assert_eq!(result["deliveryState"], "accepted", "{state}: {result}");
            assert_eq!(calls.lock().await.len(), 1, "{state} 应直接投递一次");
            let current = service.conversation(&c.id).await.unwrap();
            assert!(current.remote_queue.accepted.contains_key("new"));
            assert_eq!(
                current.remote_queue.messages.len(),
                usize::from(state == "old-queue")
            );
            if state == "old-queue" {
                assert_eq!(current.remote_queue.messages[0].id, "old");
            }
        }
    }

    #[tokio::test]
    async fn new_send_refreshes_activity_before_admission_and_query_failure_stays_unsent() {
        for (reply, cached_active, expected) in [
            (Reply::SyncIdle, true, "accepted"),
            (Reply::SyncActive, false, "queued"),
            (Reply::SyncRejected, true, "failed"),
        ] {
            let (_directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
            service
                .change(|saved| {
                    let c = saved.conversations.get_mut(&c.id).unwrap();
                    c.native_status = json!({"tempo":if cached_active {"active"} else {"idle"}});
                    c.turns.last_mut().unwrap()["status"] = json!(if cached_active {
                        "inProgress"
                    } else {
                        "completed"
                    });
                    Ok(())
                })
                .await
                .unwrap();
            replies.lock().await.push_back(reply);
            let input = payload(&c, "fresh-status");
            let result = service.command("send", &input, &auth).await;
            if expected == "failed" {
                let failure = result.unwrap_err();
                assert!(!failure.downcast_ref::<ApiError>().unwrap().outcome_unknown);
                assert!(
                    service.queue(&c.id).await.unwrap()["queuedMessages"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            } else {
                assert_eq!(result.unwrap()["deliveryState"], expected);
            }
            assert_eq!(
                calls.lock().await.len(),
                usize::from(expected == "accepted")
            );
        }
    }

    #[tokio::test]
    async fn idle_unknown_receipt_does_not_admit_another_waiting_message() {
        let (_directory, _auth_directory, service, c, auth, calls, _replies) = fixture().await;
        service
            .command("send", &payload(&c, "old"), &auth)
            .await
            .unwrap();
        service
            .change(|saved| {
                let current = saved.conversations.get_mut(&c.id).unwrap();
                current.native_status = json!({"tempo":"idle"});
                current.remote_queue.messages[0].state = "unknown".into();
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            service
                .command("send", &payload(&c, "new"), &auth)
                .await
                .is_err()
        );
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(current.remote_queue.messages.len(), 1);
        assert_eq!(current.remote_queue.messages[0].id, "old");
        assert_eq!(current.remote_queue.messages[0].state, "unknown");
        assert!(
            calls.lock().await.is_empty(),
            "未知回执不能触发重发或另建等待项"
        );
    }

    #[tokio::test]
    async fn new_direct_send_rejection_drops_queue_entry_but_preserves_deferred_and_unknown() {
        for reply in [Reply::ControlRejected, Reply::Deferred, Reply::Unknown] {
            let (directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
            service
                .change(|saved| {
                    let c = saved.conversations.get_mut(&c.id).unwrap();
                    c.native_status = json!({"tempo":"idle"});
                    c.turns.last_mut().unwrap()["status"] = json!("completed");
                    Ok(())
                })
                .await
                .unwrap();
            let mut input = payload(&c, "first-direct");
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(1, 1)
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            input["images"] = json!([{"name":"fixture.png","mime":"image/png","size":png.get_ref().len(),
                "dataUrl":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png.get_ref()))}]);
            let original = input.clone();
            replies.lock().await.push_back(reply);
            let result = service.command("send", &input, &auth).await;
            assert_eq!(input, original, "失败不能改写原文字或图片");
            assert_eq!(calls.lock().await.len(), 1, "每次提交只投递一次");
            let restored = Claude::new(directory.path()).unwrap();
            let queue = restored.queue(&c.id).await.unwrap();
            let state = restored.queue_send_state(&input).await.unwrap();
            if matches!(reply, Reply::ControlRejected) {
                let failure = result.unwrap_err();
                assert!(!failure.downcast_ref::<ApiError>().unwrap().outcome_unknown);
                assert!(queue["queuedMessages"].as_array().unwrap().is_empty());
                let restored = restored.conversation(&c.id).await.unwrap();
                assert!(restored.remote_queue.awaiting_turn.is_none());
                assert!(restored.remote_queue.accepted.is_empty());
                assert_eq!(state["confirmed"], false, "明确拒绝不能被回读误认为已排队");
                service.drain_remote_queues().await;
                assert_eq!(calls.lock().await.len(), 1, "明确拒绝不能随后自动投递");
            } else if matches!(reply, Reply::Deferred) {
                assert_eq!(result.unwrap()["deliveryState"], "queued");
                assert_eq!(queue["queuedMessages"].as_array().unwrap().len(), 1);
                assert_eq!(state["confirmed"], true, "原生忙碌时仍支持正常排队");
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .downcast_ref::<ApiError>()
                        .unwrap()
                        .outcome_unknown
                );
                assert!(
                    queue["queuedMessages"].as_array().unwrap().is_empty(),
                    "直接发送的未知意图不能显示为待发送队列"
                );
                let current = restored.conversation(&c.id).await.unwrap();
                assert_eq!(current.remote_queue.messages[0].state, "unknown");
                assert!(current.remote_queue.messages[0].direct);
                assert_eq!(
                    state["confirmed"], false,
                    "送达未知不能误报已排队或允许重发"
                );
                let repeated = service.command("send", &input, &auth).await.unwrap_err();
                assert!(
                    repeated.downcast_ref::<ApiError>().unwrap().outcome_unknown,
                    "相同未知消息身份仍保留待核对，不能改报入队成功"
                );
                assert_eq!(calls.lock().await.len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn remote_queue_known_rejection_can_retry_but_native_busy_stays_queued() {
        let (_directory, _auth_directory, service, c, auth, calls, replies) = fixture().await;
        service
            .command("send", &payload(&c, "B"), &auth)
            .await
            .unwrap();
        replies.lock().await.push_back(Reply::Deferred);
        let process = service.processes.lock().await[&c.id].clone();
        let result = service
            .dispatch_remote(&c.id, "B", &process, false, &auth, false)
            .await
            .unwrap();
        assert_eq!(result["deliveryState"], "queued");
        assert_eq!(
            service
                .conversation(&c.id)
                .await
                .unwrap()
                .remote_queue
                .messages[0]
                .state,
            "queued"
        );
        replies.lock().await.push_back(Reply::Rejected);
        assert!(
            service
                .dispatch_remote(&c.id, "B", &process, true, &auth, false)
                .await
                .is_err()
        );
        assert_eq!(
            service
                .conversation(&c.id)
                .await
                .unwrap()
                .remote_queue
                .messages[0]
                .state,
            "paused"
        );
        service
            .dispatch_remote(&c.id, "B", &process, true, &auth, false)
            .await
            .unwrap();
        assert_eq!(calls.lock().await.len(), 3);
    }

    #[tokio::test]
    async fn remote_queue_auto_sends_one_in_order_and_respects_stop_and_revocation() {
        let (_directory, _auth_directory, service, c, auth, calls, _) = fixture().await;
        for id in ["B", "C", "D"] {
            service
                .command("send", &payload(&c, id), &auth)
                .await
                .unwrap();
        }
        let idle = |saved: &mut Saved| {
            let c = saved.conversations.get_mut(&c.id).unwrap();
            c.native_status = json!({"tempo":"idle"});
            c.queued_messages.clear();
            c.turns.last_mut().unwrap()["status"] = json!("completed");
            Ok(())
        };
        service.change(idle).await.unwrap();
        service.drain_remote_queues().await;
        service.drain_remote_queues().await;
        assert_eq!(calls.lock().await.len(), 1);
        assert_eq!(calls.lock().await[0]["client_id"], "B");
        assert_eq!(calls.lock().await[0]["idle_only"], true);
        service.change(idle).await.unwrap();
        service.drain_remote_queues().await;
        assert_eq!(
            calls.lock().await.len(),
            1,
            "接收后短暂 idle 还不是本轮完成，不能提前续发"
        );
        service
            .change(|saved| {
                let c = saved.conversations.get_mut(&c.id).unwrap();
                c.turns
                    .push(json!({"id":"B","status":"completed","_nativeDone":true,"items":[]}));
                c.remote_queue.observe_turns(&c.turns, &c.native_status);
                Ok(())
            })
            .await
            .unwrap();
        service.drain_remote_queues().await;
        assert_eq!(calls.lock().await[1]["client_id"], "C");
        service
            .command("stop", &json!({"threadId":c.id}), &auth)
            .await
            .unwrap();
        service.change(idle).await.unwrap();
        service.drain_remote_queues().await;
        assert_eq!(
            calls.lock().await.len(),
            3,
            "停止后只增加停止请求，剩余队列保持暂停"
        );
        assert_eq!(calls.lock().await[2]["subtype"], "interrupt");
        service
            .change(|saved| {
                let c = saved.conversations.get_mut(&c.id).unwrap();
                c.remote_queue.paused = false;
                c.remote_queue.awaiting_turn = None;
                Ok(())
            })
            .await
            .unwrap();
        auth.controller
            .auth
            .lock()
            .await
            .devices
            .retain(|d| d.id != auth.device.id);
        service.drain_remote_queues().await;
        assert_eq!(calls.lock().await.len(), 3, "撤销授权后不能自动续发");
        assert_eq!(
            service
                .conversation(&c.id)
                .await
                .unwrap()
                .remote_queue
                .messages
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn remote_queue_edit_draft_does_not_block_and_resend_preserves_position() {
        for resend in [false, true] {
            let (_directory, _auth_directory, service, c, auth, calls, _) = fixture().await;
            for id in ["B", "C", "D"] {
                service
                    .command("send", &payload(&c, id), &auth)
                    .await
                    .unwrap();
            }
            let original = service
                .conversation(&c.id)
                .await
                .unwrap()
                .remote_queue
                .messages[0]
                .clone();
            service
                .command(
                    "beginQueuedEdit",
                    &json!({"threadId":c.id,"messageId":"B","editId":"draft-B",
                        "originalText":"正文-B","clientUserMessageId":"B","createdAt":original.created_at}),
                    &auth,
                )
                .await
                .unwrap();
            let finish_current = |saved: &mut Saved| {
                let current = saved.conversations.get_mut(&c.id).unwrap();
                current.native_status = json!({"tempo":"idle"});
                current.queued_messages.clear();
                if let Some(client) = current.remote_queue.awaiting_turn.clone() {
                    current.turns.push(
                        json!({"id":client,"status":"completed","_nativeDone":true,"items":[]}),
                    );
                } else {
                    current.turns.last_mut().unwrap()["status"] = json!("completed");
                }
                current
                    .remote_queue
                    .observe_turns(&current.turns, &current.native_status);
                Ok(())
            };
            service.change(finish_current).await.unwrap();
            for state in ["paused", "unknown", "sending"] {
                service
                    .change(|saved| {
                        saved
                            .conversations
                            .get_mut(&c.id)
                            .unwrap()
                            .remote_queue
                            .messages[1]
                            .state = state.into();
                        Ok(())
                    })
                    .await
                    .unwrap();
                service.drain_remote_queues().await;
                assert!(
                    calls.lock().await.is_empty(),
                    "跳过编辑草稿不能跳过暂停、发送中或未知结果"
                );
            }
            service
                .change(|saved| {
                    saved
                        .conversations
                        .get_mut(&c.id)
                        .unwrap()
                        .remote_queue
                        .messages[1]
                        .state = "queued".into();
                    Ok(())
                })
                .await
                .unwrap();
            service.drain_remote_queues().await;
            assert_eq!(calls.lock().await[0]["client_id"], "C");
            assert_eq!(
                service
                    .conversation(&c.id)
                    .await
                    .unwrap()
                    .remote_queue
                    .edits["draft-B"]
                    .state,
                "editing",
                "原消息仍是未发送草稿"
            );
            if resend {
                service
                    .command(
                        "editQueued",
                        &json!({"threadId":c.id,"editId":"draft-B","prompt":"编辑后-B"}),
                        &auth,
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    service
                        .conversation(&c.id)
                        .await
                        .unwrap()
                        .remote_queue
                        .messages
                        .iter()
                        .map(|message| message.id.as_str())
                        .collect::<Vec<_>>(),
                    ["B", "D"]
                );
                service.change(finish_current).await.unwrap();
                service.drain_remote_queues().await;
            }
            service.change(finish_current).await.unwrap();
            service.drain_remote_queues().await;
            assert_eq!(
                calls
                    .lock()
                    .await
                    .iter()
                    .map(|request| request["client_id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                if resend {
                    vec!["C", "B", "D"]
                } else {
                    vec!["C", "D"]
                }
            );
        }
    }

    #[tokio::test]
    async fn remote_queue_edit_reorder_delete_and_recovery_preserve_position_and_images() {
        let (directory, _auth_directory, service, c, auth, calls, _) = fixture().await;
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = json!({"name":"test.png","mime":"image/png","size":png.get_ref().len(),
            "dataUrl":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png.get_ref()))});
        let source = image_source(&image);
        for id in ["B", "C", "D"] {
            let mut input = payload(&c, id);
            if id == "B" {
                input["images"] = json!([image]);
            }
            service.command("send", &input, &auth).await.unwrap();
        }
        service
            .command(
                "reorderQueued",
                &json!({"threadId":c.id,
            "expectedMessageIds":["B","C","D"],"messageIds":["D","B","C"]}),
                &auth,
            )
            .await
            .unwrap();
        assert!(
            service
                .command(
                    "reorderQueued",
                    &json!({"threadId":c.id,
            "expectedMessageIds":["B","C","D"],"messageIds":["C","B","D"]}),
                    &auth
                )
                .await
                .is_err()
        );
        let message = service
            .conversation(&c.id)
            .await
            .unwrap()
            .remote_queue
            .messages[1]
            .clone();
        let begin = json!({"threadId":c.id,"messageId":"B","editId":"edit-B","originalText":"正文-B",
            "clientUserMessageId":"B","createdAt":message.created_at,"expectedImageSources":[source]});
        service
            .command("beginQueuedEdit", &begin, &auth)
            .await
            .unwrap();
        let restored = Claude::new(directory.path()).unwrap();
        assert_eq!(
            restored
                .queue_edit_state(&json!({"threadId":c.id,"editId":"edit-B"}))
                .await
                .unwrap()["canResume"],
            true
        );
        let edit = json!({"threadId":c.id,"editId":"edit-B","prompt":"编辑后-B","retainedImageSources":[source]});
        assert_eq!(
            service.command("editQueued", &edit, &auth).await.unwrap()["editState"],
            "committed"
        );
        assert_eq!(
            service.command("editQueued", &edit, &auth).await.unwrap()["editState"],
            "committed"
        );
        assert!(
            service
                .command(
                    "editQueued",
                    &json!({"threadId":c.id,"editId":"edit-B","prompt":"冲突"}),
                    &auth
                )
                .await
                .is_err()
        );
        service
            .command(
                "removeQueued",
                &json!({"threadId":c.id,"messageId":"C"}),
                &auth,
            )
            .await
            .unwrap();
        let current = service.conversation(&c.id).await.unwrap();
        assert_eq!(
            current
                .remote_queue
                .messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["D", "B"]
        );
        assert_eq!(
            current.remote_queue.messages[1].payload["prompt"],
            "编辑后-B"
        );
        assert_eq!(
            current.remote_queue.messages[1].payload["images"],
            json!([image])
        );
        let mut second_begin = begin.clone();
        second_begin["editId"] = json!("edit-B2");
        second_begin["originalText"] = json!("编辑后-B");
        service
            .command("beginQueuedEdit", &second_begin, &auth)
            .await
            .unwrap();
        service
            .command(
                "cancelQueuedEdit",
                &json!({"threadId":c.id,"editId":"edit-B2"}),
                &auth,
            )
            .await
            .unwrap();
        let restored = service.conversation(&c.id).await.unwrap();
        assert_eq!(
            restored.remote_queue.messages[1].payload,
            current.remote_queue.messages[1].payload
        );
        assert_eq!(
            service
                .queue_send_state(&json!({"threadId":c.id,"clientUserMessageId":"B"}))
                .await
                .unwrap()["deliveryState"],
            "queued"
        );
        assert!(calls.lock().await.is_empty());
    }

    #[test]
    fn unconfirmed_intent_is_not_native_acceptance_and_dispatching_restarts_as_unknown() {
        let message = Message {
            id: "C".into(),
            payload: json!({"clientUserMessageId":"C","prompt":"C"}),
            created_at: 1,
            device_id: "device".into(),
            key_id: "key".into(),
            state: "sending".into(),
            direct: false,
            reason: String::new(),
            native_intent: Some(json!({"clientUserMessageId":"C","deliveryConfirmed":false})),
            native_edit: None,
        };
        let mut queue = RemoteQueue {
            messages: vec![message],
            ..Default::default()
        };
        queue.recover();
        queue.reconcile(
            &BTreeMap::new(),
            &[json!({"clientUserMessageId":"C","deliveryConfirmed":false})],
        );
        assert_eq!(queue.messages[0].state, "unknown");
        assert_eq!(queue.messages(&[])[0]["canSendNow"], false);
        queue.reconcile(
            &BTreeMap::new(),
            &[json!({"clientUserMessageId":"C","deliveryConfirmed":true})],
        );
        assert!(queue.messages.is_empty());
        assert!(queue.accepted.contains_key("C"));
    }

    #[tokio::test]
    async fn pausing_an_idle_remote_queue_does_not_interrupt_a_completed_reply_or_touch_cli() {
        let (_directory, _auth_directory, service, c, auth, calls, _) = fixture().await;
        service
            .command("send", &payload(&c, "B"), &auth)
            .await
            .unwrap();
        service
            .change(|saved| {
                let c = saved.conversations.get_mut(&c.id).unwrap();
                c.native_status = json!({"tempo":"idle"});
                c.turns[0]["status"] = json!("completed");
                Ok(())
            })
            .await
            .unwrap();
        let result = service
            .command("stop", &json!({"threadId":c.id}), &auth)
            .await
            .unwrap();
        assert_eq!(result["queuePaused"], true);
        let c = service.conversation(&c.id).await.unwrap();
        assert_eq!(c.turns[0]["status"], "completed");
        assert_ne!(c.turns[0]["_stopRequested"], true);
        assert!(calls.lock().await.is_empty());
    }
}
