use serde_json::{Value, json};

use super::super::RemoteAuthorization;
use super::store::{Edit, Queued};
use super::{Harness, error, required, sent_key};
use crate::codex_service::normalize_thread_response;

impl Harness {
    pub(super) async fn command(
        &self,
        action: &str,
        payload: Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if action == "setFast" {
            return self.set_fast(&payload, authorization).await;
        }
        if action == "createSideThread" {
            return self.create_side(&payload, authorization).await;
        }
        if action == "closeSideThread"
            || self
                .known_side(payload["threadId"].as_str().unwrap_or(""))
                .await
        {
            return self.side_command(action, &payload, authorization).await;
        }
        if action == "pinThread"
            && let Some(result) = self.pin_native_thread(&payload, authorization).await?
        {
            return Ok(result);
        }
        if matches!(action, "pinProject" | "pinThread" | "sort") {
            if action == "pinThread" {
                // 子智能体只读约束也覆盖旧引擎的本地置顶回退，不能绕过主会话身份校验。
                let id = required(&payload, "threadId")?;
                let identity = self
                    .runtime
                    .request(
                        "thread/read",
                        json!({"threadId":id,"includeTurns":false}),
                        None,
                    )
                    .await?;
                normalize_thread_response(&identity)?;
            }
            return self.preference(action, &payload, authorization).await;
        }
        if action == "create" {
            return self.create(&payload, authorization).await;
        }
        let id = required(&payload, "threadId")?.to_owned();
        // 所有写入口先核对持久主会话；子智能体只读查看不能通过自行构造请求变成写入或接管。
        let identity = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":id,"includeTurns":false}),
                None,
            )
            .await?;
        normalize_thread_response(&identity)?;
        if action == "answer" && payload["method"] == "remote/requestUserInputAsync" {
            return self.answer_async(&id, &payload, authorization).await;
        }
        match action {
            "send" => {
                self.enqueue(&id, payload, &identity["thread"], authorization)
                    .await
            }
            "stop" => {
                let snapshot = self.owner_snapshot(&id).await?;
                let turn_id = match &snapshot {
                    Some(snapshot) => crate::desktop_owner_ipc::snapshot_turns(snapshot)
                        .iter()
                        .rev()
                        .find(|t| t["status"] == "inProgress")
                        .and_then(|t| t["turnId"].as_str())
                        .map(str::to_owned),
                    None => self.projection.lock().await.active.get(&id).cloned(),
                };
                if let Some(turn_id) = turn_id {
                    if self.owner_request(
                        &id,
                        "thread-follower-interrupt-turn",
                        json!({"conversationId":id,"mode":"user-stop","expectedTurnId":turn_id}),
                        4,
                        Some(authorization),
                    )
                    .await?
                    .is_none()
                    {
                        self.ensure_loaded(&id, authorization).await?;
                        self.runtime
                            .request(
                                "turn/interrupt",
                                json!({"threadId":id,"turnId":turn_id}),
                                Some(authorization),
                            )
                            .await?;
                    }
                }
                self.store.lock().await.commit(|data| {
                    for message in data.queues.get_mut(&id).into_iter().flatten() {
                        if message.state == "queued" {
                            message.state = "paused".into();
                        }
                    }
                    Ok(())
                })?;
                self.queue_changed(&id).await;
                Ok(json!({"ok":true,"threadId":id}))
            }
            "compact" => {
                let revision = self.projection.lock().await.compaction_revision(&id);
                if self
                    .owner_request(
                        &id,
                        "thread-follower-compact-thread",
                        json!({"conversationId":id}),
                        1,
                        Some(authorization),
                    )
                    .await?
                    .is_none()
                {
                    self.ensure_loaded(&id, authorization).await?;
                    self.runtime
                        .request(
                            "thread/compact/start",
                            json!({"threadId":id}),
                            Some(authorization),
                        )
                        .await?;
                }
                let mut projection = self.projection.lock().await;
                // 用户要求切回仍恢复真实压缩状态；开始/完成事件可能早于 RPC 回执，不能再覆盖为 pending。
                if projection.compaction_revision(&id) == revision {
                    projection.compactions.insert(id.clone(),
                        json!({"state":"pending","turnId":null,"itemId":"pending-manual-context-compaction"}));
                }
                let compaction = projection.compaction(&id);
                Ok(json!({"ok":true,"threadId":id,"compaction":compaction}))
            }
            "rename" => {
                let title = required(&payload, "title")?.trim();
                if title.chars().count() > 200 {
                    return Err(error(400, "标题需为 1–200 个字符"));
                }
                self.runtime
                    .request(
                        "thread/name/set",
                        json!({"threadId":id,"name":title}),
                        Some(authorization),
                    )
                    .await?;
                let event =
                    json!({"method":"thread/name/updated","params":{"threadId":id,"name":title}});
                self.catalog.observe(&event).await;
                self.projection.lock().await.push(event);
                Ok(json!({"ok":true,"threadId":id}))
            }
            "markRead" => {
                if let Some(revision) = identity["readState"]["revision"].as_str() {
                    let read = self
                        .runtime
                        .request(
                            "thread/readState/update",
                            json!({"threadId":id,
                        "expectedRevision":revision,"operation":{"type":"read"}}),
                            Some(authorization),
                        )
                        .await?;
                    self.catalog
                        .observe(&json!({
                            "method":"thread/readState/updated",
                            "params":{"threadId":id,"readState":read["readState"]}
                        }))
                        .await;
                }
                Ok(json!({"ok":true,"threadId":id}))
            }
            "deleteThread" => {
                self.runtime
                    .request("thread/delete", json!({"threadId":id}), Some(authorization))
                    .await?;
                self.store.lock().await.commit(|data| {
                    data.queues.remove(&id);
                    data.edits.retain(|_, e| e.thread_id != id);
                    data.thread_pins.remove(&id);
                    Ok(())
                })?;
                self.loaded.lock().await.remove(&id);
                let event = json!({"method":"thread/deleted","params":{"threadId":id}});
                self.catalog.observe(&event).await;
                self.projection.lock().await.push(event);
                // 用户要求成功删除不误报“未确认”；仅在引擎确认删除和本地清理成功后返回完整删除回执。
                Ok(json!({"ok":true,"threadId":id,"deleted":true,"status":"local_deleted"}))
            }
            "setGoal" => self.set_goal(&id, &payload, authorization).await,
            "updateGoal" => self.update_goal(&id, &payload, authorization).await,
            "editMessage" => self.edit_message(&id, &payload, authorization).await,
            "approve" | "answer" => self.respond(&id, action, &payload, authorization).await,
            "removeQueued" | "reorderQueued" | "beginQueuedEdit" | "editQueued"
            | "cancelQueuedEdit" | "dismissQueuedEdit" => {
                self.edit_queue(&id, action, &payload, authorization).await
            }
            "steerQueued" => {
                let message_id = required(&payload, "messageId")?;
                let message = self
                    .store
                    .lock()
                    .await
                    .data
                    .queues
                    .get(&id)
                    .and_then(|q| q.iter().find(|q| q.id == message_id))
                    .cloned()
                    .ok_or_else(|| error(409, "队列消息已变化"))?;
                if !["queued", "paused"].contains(&message.state.as_str()) {
                    return Err(error(409, "此队列消息当前不能引导"));
                }
                let input = inputs(&message.payload)?;
                self.store.lock().await.commit(|data| {
                    data.queues
                        .get_mut(&id)
                        .unwrap()
                        .iter_mut()
                        .find(|q| q.id == message_id)
                        .unwrap()
                        .state = "sending".into();
                    Ok(())
                })?;
                let result = self
                    .steer(&id, input, &message.payload, authorization)
                    .await;
                self.store.lock().await.commit(|data| {
                    if let Ok(response) = &result {
                        data.queues
                            .get_mut(&id)
                            .unwrap()
                            .retain(|q| q.id != message_id);
                        data.sent
                            .insert(sent_key(&id, &message.payload), response.clone());
                    } else if let Some(message) = data
                        .queues
                        .get_mut(&id)
                        .unwrap()
                        .iter_mut()
                        .find(|q| q.id == message_id)
                    {
                        message.state = "unknown".into();
                    }
                    Ok(())
                })?;
                self.queue_changed(&id).await;
                let mut result = result?;
                result["queuedMessages"] = self.queue(&id).await["queuedMessages"].clone();
                Ok(result)
            }
            _ => Err(error(404, "不支持的远程会话操作")),
        }
    }

    async fn preference(
        &self,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let mut store = self.store.lock().await;
        let permit = authorization.prepare_dispatch().await?;
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            store.commit(|data| {
            match action {
                "pinThread"|"pinProject" => {
                    let id=required(payload,if action=="pinThread" {"threadId"} else {"projectId"})?;
                    let pins=if action=="pinThread" {&mut data.thread_pins} else {&mut data.project_pins};
                    if payload["pinned"]==true { pins.insert(id.to_owned()); } else { pins.remove(id); }
                    // 初始置顶与桌面对齐；服务端明确取消置顶后，不能被桌面默认值重新覆盖。
                    if action=="pinProject" { data.project_pin_overrides.insert(id.to_owned(),payload["pinned"]==true); }
                }
                "sort" => {
                    if !["manual","updated_at"].contains(&required(payload,"mode")?) { return Err(error(400,"不支持的排序方式")); }
                    if !data.preferences.is_object() { data.preferences=json!({"mode":"project","chats":"updated_at","projects":"updated_at","pinned":"manual"}); }
                    match required(payload,"target")? {
                        "grouping" => data.preferences["mode"]=json!(if payload["grouping"]=="list" {"list"} else {"project"}),
                        key @ ("chats"|"projects"|"pinned") => data.preferences[key]=payload["mode"].clone(),
                        _ => return Err(error(400,"排序对象无效")),
                    }
                }
                _ => unreachable!(),
            }
            Ok(())
        })
        })?;
        drop(store);
        self.projection
            .lock()
            .await
            .push(json!({"method":"remote/sidebarChanged"}));
        Ok(json!({"ok":true}))
    }

    async fn enqueue(
        &self,
        id: &str,
        payload: Value,
        fresh_thread: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        inputs(&payload)?;
        let direct = !self.send_is_running(id, fresh_thread).await?;
        let mut message = Queued::new(payload, authorization);
        let client_id = message.payload["clientUserMessageId"].clone();
        let mut store = self.store.lock().await;
        if let Some(previous) = store.data.sent.get(&sent_key(id, &message.payload)) {
            if store
                .data
                .sent_hashes
                .get(&sent_key(id, &message.payload))
                .is_some_and(|hash| hash != &payload_hash(&message.payload))
            {
                return Err(error(409, "消息标识已用于其他内容"));
            }
            return Ok(previous.clone());
        }
        if let Some(previous) = store
            .data
            .queues
            .get(id)
            .into_iter()
            .flatten()
            .find(|q| q.payload["clientUserMessageId"] == client_id)
        {
            if previous.payload != message.payload {
                return Err(error(409, "消息标识已用于其他内容"));
            }
            if matches!(
                previous.state.as_str(),
                "sending" | "unknown" | "direct-sending" | "direct-unknown"
            ) {
                return Ok(
                    json!({"ok":false,"code":409,"outcomeUnknown":true,"threadId":id,
                    "clientUserMessageId":client_id,"message":"原消息送达状态待核对，不会重复发送"}),
                );
            }
            return Ok(
                json!({"ok":true,"confirmed":true,"threadId":id,"messageId":previous.id,
                "clientUserMessageId":client_id,"deliveryState":"queued"}),
            );
        }
        if direct
            && store.data.queues.get(id).into_iter().flatten().any(|q| {
                matches!(
                    q.state.as_str(),
                    "sending" | "unknown" | "direct-sending" | "direct-unknown"
                )
            })
        {
            return Err(error(409, "请先核对尚未确认的消息"));
        }
        // 用户要求只有明确执行中才入队；直接发送保存的仅是防重复投递意图，不展示为待发送队列。
        if direct {
            message.state = "direct-sending".into();
        }
        let permit = authorization.prepare_dispatch().await?;
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            store.commit(|data| {
                let queue = data.queues.entry(id.to_owned()).or_default();
                if queue.len() >= 100 {
                    return Err(error(429, "此会话的待发送消息已达上限"));
                }
                queue.push(message.clone());
                Ok(())
            })
        })?;
        drop(store);
        if direct {
            return self.dispatch_direct(id, &message, authorization).await;
        }
        self.queue_changed(id).await;
        let mut response = self.queue(id).await;
        response["messageId"] = json!(&message.id);
        response["clientUserMessageId"] = client_id;
        response["deliveryState"] = json!("queued");
        response["confirmed"] = json!(true);
        Ok(response)
    }

    async fn send_is_running(&self, id: &str, fresh_thread: &Value) -> anyhow::Result<bool> {
        if let Some(snapshot) = self.owner_snapshot(id).await? {
            let status = &snapshot["threadRuntimeStatus"];
            let status = status.as_str().or_else(|| status["type"].as_str());
            if matches!(status, Some("active" | "inProgress" | "running")) {
                return Ok(true);
            }
            if status == Some("idle") {
                return Ok(false);
            }
            return Ok(crate::desktop_owner_ipc::snapshot_turns(&snapshot)
                .iter()
                .any(|turn| turn["status"] == "inProgress"));
        }
        let projection = self.projection.lock().await;
        if projection.compactions.contains_key(id) {
            return Ok(true);
        }
        let status = &fresh_thread["status"];
        let status = status.as_str().or_else(|| status["type"].as_str());
        Ok(match status {
            Some("active" | "inProgress" | "running") => true,
            Some("idle" | "completed" | "failed" | "interrupted") => false,
            _ => projection.active.contains_key(id),
        })
    }

    async fn dispatch_direct(
        &self,
        id: &str,
        message: &Queued,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let mut attempted = false;
        let result: anyhow::Result<Option<Value>> = async {
            let fresh = self
                .runtime
                .request(
                    "thread/read",
                    json!({"threadId":id,"includeTurns":false}),
                    None,
                )
                .await?;
            normalize_thread_response(&fresh)?;
            // 准备之后再检查；只有此时明确开始执行，才把本次意图转换为真正的队列消息。
            if self.send_is_running(id, &fresh["thread"]).await? {
                return Ok(None);
            }
            authorization.check().await?;
            attempted = true;
            self.start_turn(id, &message.payload, authorization)
                .await
                .map(Some)
        }
        .await;
        self.store.lock().await.commit(|data| {
            match &result {
                Ok(Some(response)) => {
                    data.sent
                        .insert(sent_key(id, &message.payload), response.clone());
                    data.sent_hashes.insert(
                        sent_key(id, &message.payload),
                        payload_hash(&message.payload),
                    );
                    if let Some(queue) = data.queues.get_mut(id) {
                        queue.retain(|q| q.id != message.id);
                    }
                }
                Ok(None) => {
                    if let Some(queued) = data
                        .queues
                        .get_mut(id)
                        .and_then(|q| q.iter_mut().find(|q| q.id == message.id))
                    {
                        queued.state = "queued".into();
                    }
                }
                Err(failure) => {
                    let not_sent = !attempted
                        || failure
                            .downcast_ref::<super::super::ApiError>()
                            .is_some_and(|error| !error.outcome_unknown)
                        || failure
                            .downcast_ref::<crate::desktop_owner_ipc::DispatchFailure>()
                            .is_some_and(|error| !error.dispatched);
                    if let Some(queue) = data.queues.get_mut(id) {
                        if not_sent {
                            queue.retain(|q| q.id != message.id);
                        } else if let Some(queued) = queue.iter_mut().find(|q| q.id == message.id) {
                            queued.state = "direct-unknown".into();
                        }
                    }
                }
            }
            Ok(())
        })?;
        self.queue_changed(id).await;
        if let Some(response) = result? {
            return Ok(response);
        }
        let mut response = self.queue(id).await;
        response["messageId"] = json!(message.id);
        response["clientUserMessageId"] = message.payload["clientUserMessageId"].clone();
        response["deliveryState"] = json!("queued");
        response["confirmed"] = json!(true);
        Ok(response)
    }

    pub(super) async fn start_turn(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        self.start_input(id, payload, inputs(payload)?, authorization)
            .await
    }

    async fn start_input(
        &self,
        id: &str,
        payload: &Value,
        input: Vec<Value>,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        self.store.lock().await.commit(|data| {
            data.thread_devices
                .insert(id.to_owned(), authorization.device.id.clone());
            Ok(())
        })?;
        let mut params = json!({"threadId":id,"input":input,
            "clientUserMessageId":payload["clientUserMessageId"]});
        for (source, target) in [
            ("model", "model"),
            ("reasoningEffort", "effort"),
            ("serviceTier", "serviceTier"),
        ] {
            if payload[source].as_str().is_some_and(|s| !s.is_empty()) {
                params[target] = payload[source].clone();
            }
        }
        // Fast 按保存成功后的偏好在新轮次派发时读取，排队消息不冻结入队时的旧档位。
        self.apply_fast_to_turn(id, &mut params).await?;
        let applied_tier = params["serviceTier"].clone();
        let (response, execution_transport) = if let Some(response) = self
            .owner_request(
                id,
                "thread-follower-start-turn",
                json!({"conversationId":id,"turnStart":{"request":params,
                "context":{"inheritThreadSettings":true}}}),
                2,
                Some(authorization),
            )
            .await?
        {
            (
                response.get("result").cloned().unwrap_or(response),
                "desktop-ipc",
            )
        } else {
            self.ensure_loaded(id, authorization).await?;
            (
                self.runtime
                    .request("turn/start", params, Some(authorization))
                    .await?,
                "app-server-stdio",
            )
        };
        let turn_id = required(&response["turn"], "id")?;
        self.projection.lock().await.started(id, turn_id);
        let mut remembered = payload.clone();
        // 创建草稿中的旧 Fast 值不能在发送回执到达时覆盖执行期间后来保存的会话偏好。
        if let Some(remembered) = remembered.as_object_mut() {
            remembered.remove("fastMode");
        }
        if !applied_tier.is_null() {
            remembered["serviceTier"] = applied_tier;
        }
        self.remember_settings(id, &remembered).await?;
        Ok(
            json!({"ok":true,"confirmed":true,"threadId":id,"turnId":turn_id,
            "clientUserMessageId":payload["clientUserMessageId"],"deliveryState":"accepted",
            "status":response["turn"]["status"],"nativeBridge":"harness",
            "executionTransport":execution_transport}),
        )
    }

    async fn steer(
        &self,
        id: &str,
        input: Vec<Value>,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let client_id = payload["clientUserMessageId"].clone();
        if let Some(response) = self
            .owner_request(
                id,
                "thread-follower-steer-turn",
                json!({"conversationId":id,"input":input,"clientUserMessageId":client_id}),
                1,
                Some(authorization),
            )
            .await?
        {
            return Ok(
                json!({"ok":true,"threadId":id,"confirmed":true,"deliveryState":"accepted",
                "clientUserMessageId":client_id,"result":response}),
            );
        }
        let turn_id = self.projection.lock().await.active.get(id).cloned();
        if let Some(turn_id) = turn_id {
            let response = self
                .runtime
                .request(
                    "turn/steer",
                    json!({"threadId":id,"input":input,
                "expectedTurnId":turn_id,"clientUserMessageId":client_id}),
                    Some(authorization),
                )
                .await?;
            Ok(
                json!({"ok":true,"threadId":id,"confirmed":true,"deliveryState":"accepted",
                "clientUserMessageId":client_id,"turnId":response["turnId"]}),
            )
        } else {
            self.start_turn(id, payload, authorization).await
        }
    }

    async fn create(
        &self,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        inputs(payload)?;
        if payload["fastMode"] == true {
            self.validate_fast_model(required(payload, "model")?)
                .await?;
        }
        let (cwd, project_id) =
            if let Some(id) = payload["projectId"].as_str().filter(|s| !s.is_empty()) {
                // 新建属于实际写入，提交前重新核对项目根目录，不能用展示缓存决定写入位置。
                self.runtime.invalidate_read("project/list").await;
                let context = self.workspace_context(&json!({"projectId":id})).await?;
                // 展示 ID 与 CDP 相同；实际创建必须使用已验证的引擎项目 ID。
                (
                    required(&context, "cwd")?.to_owned(),
                    Some(required(&context, "serverProjectId")?.to_owned()),
                )
            } else {
                let settings = crate::settings::SettingsStore::default().load()?;
                let directory = crate::codex_home::codex_home_dir_for_settings(&settings)
                    .join("remote-workspaces")
                    .join(uuid::Uuid::new_v4().to_string());
                std::fs::create_dir_all(&directory)?;
                (directory.to_string_lossy().into_owned(), None)
            };
        let mut params = json!({"cwd":cwd,"ephemeral":false,"threadSource":"user"});
        if let Some(project_id) = project_id {
            params["projectId"] = json!(project_id);
        }
        if let Some(model) = payload["model"].as_str().filter(|s| !s.is_empty()) {
            params["model"] = json!(model);
        }
        let response = self
            .runtime
            .request("thread/start", params, Some(authorization))
            .await?;
        let id = required(&response["thread"], "id")?.to_owned();
        self.catalog
            .observe(&json!({
                "method":"thread/started","params":{"thread":response["thread"]}
            }))
            .await;
        let mut initial_settings = response.clone();
        // 新会话的 Fast 仅取本次草稿；后续新会话仍默认关闭，不继承全局或其它会话。
        initial_settings["fastMode"] = json!(payload["fastMode"] == true);
        self.remember_settings(&id, &initial_settings).await?;
        self.loaded.lock().await.insert(
            id.clone(),
            self.runtime.connection().await?.generation.clone(),
        );
        let mut first = payload.clone();
        first["threadId"] = json!(&id);
        first["clientUserMessageId"] = json!(uuid::Uuid::new_v4().to_string());
        let sent = if payload["goalMode"] == true {
            self.set_goal(&id, &first, authorization).await
        } else {
            self.start_turn(&id, &first, authorization).await
        };
        match sent {
            Ok(mut sent) => {
                sent["conversationId"] = json!(&id);
                sent["status"] = json!("created");
                sent["firstTurn"] = json!({"status":"accepted"});
                Ok(sent)
            }
            Err(error) => Ok(
                json!({"ok":false,"code":409,"threadId":id,"conversationId":id,
                "outcomeUnknown":true,"message":format!("会话已创建，首条消息结果待核对：{error}"),
                "firstTurn":{"status":"outcome-unknown"}}),
            ),
        }
    }

    async fn set_goal(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let existing = self
            .runtime
            .request("thread/goal/get", json!({"threadId":id}), None)
            .await?;
        if !existing["goal"].is_null() && existing["goal"]["status"] != "complete" {
            return Err(error(409, "此会话已有未结束的目标"));
        }
        let mut first = payload.clone();
        let objective = required(payload, "prompt")?.trim().to_owned();
        first["prompt"] = json!(format!("/goal {objective}"));
        first["clientUserMessageId"] = json!(uuid::Uuid::new_v4().to_string());
        let mut sent = self.start_turn(id, &first, authorization).await?;
        let goal = self
            .runtime
            .request(
                "thread/goal/set",
                json!({"threadId":id,"objective":objective,
            "status":"active"}),
                Some(authorization),
            )
            .await;
        match goal {
            Ok(goal) => {
                sent["goal"] = goal["goal"].clone();
                sent["goalSubmission"] = json!({"threadId":id,"phase":"complete","firstTurnAccepted":true,"goalSet":"succeeded"});
                Ok(sent)
            }
            Err(error) => Ok(
                json!({"ok":false,"code":409,"threadId":id,"outcomeUnknown":true,
                "message":format!("首条目标消息已发送，目标状态待核对：{error}"),
                "goalSubmission":{"threadId":id,"phase":"goalSet","firstTurnAccepted":true,"goalSet":"unknown"}}),
            ),
        }
    }

    async fn update_goal(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let current = self
            .runtime
            .request("thread/goal/get", json!({"threadId":id}), None)
            .await?;
        let current = &current["goal"];
        if current["createdAt"] != payload["expectedCreatedAt"]
            || current["objective"] != payload["expectedObjective"]
            || current["status"] != payload["expectedStatus"]
        {
            return Err(error(409, "目标已变化，请重新读取"));
        }
        let operation = payload["operation"].as_str().unwrap_or("edit");
        let response = if operation == "delete" {
            self.runtime
                .request(
                    "thread/goal/clear",
                    json!({"threadId":id}),
                    Some(authorization),
                )
                .await?;
            json!({"goal":null})
        } else {
            let status = match operation {
                "pause" => json!("paused"),
                "resume" => json!("active"),
                _ => current["status"].clone(),
            };
            self.runtime
                .request(
                    "thread/goal/set",
                    json!({"threadId":id,"objective":payload["objective"],
                "status":status}),
                    Some(authorization),
                )
                .await?
        };
        self.projection
            .lock()
            .await
            .threads
            .entry(id.to_owned())
            .or_insert_with(|| json!({}))["goal"] = response["goal"].clone();
        Ok(json!({"ok":true,"threadId":id,"goal":response["goal"]}))
    }

    async fn edit_message(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if let Some(response)=self.owner_request(id,"thread-follower-edit-last-user-turn",
            json!({"conversationId":id,"turnId":payload["turnId"],"message":payload["prompt"],
                "allowEarlierMessages":false,"agentMode":"default","shouldSendPermissionOverrides":false}),2,Some(authorization)).await? {
            return Ok(json!({"ok":true,"threadId":id,"result":response}));
        }
        let history = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":id,"includeTurns":true}),
                None,
            )
            .await?;
        let turn = history["thread"]["turns"]
            .as_array()
            .and_then(|t| t.last())
            .ok_or_else(|| error(409, "没有可编辑的消息"))?;
        if turn["id"] != payload["turnId"]
            || turn["status"] == "inProgress"
            || !turn["items"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|i| i["type"] == "userMessage" && i["id"] == payload["messageId"])
            })
        {
            return Err(error(409, "最后一条用户消息已变化"));
        }
        let message = turn["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == payload["messageId"])
            .unwrap();
        let mut input = message["content"]
            .as_array()
            .cloned()
            .ok_or_else(|| error(409, "原消息输入尚未完整加载"))?;
        let mut replaced = false;
        for item in &mut input {
            if item["type"] == "text" && !replaced {
                item["text"] = payload["prompt"].clone();
                item["text_elements"] = json!([]);
                replaced = true;
            }
        }
        if !replaced {
            input.insert(
                0,
                json!({"type":"text","text":payload["prompt"],"text_elements":[]}),
            );
        }
        self.ensure_loaded(id, authorization).await?;
        self.runtime
            .request(
                "thread/revert",
                json!({"threadId":id,"beforeTurnId":payload["turnId"]}),
                Some(authorization),
            )
            .await?;
        // 回退后以新的原生历史为准，不能把已删除轮次中的压缩记录重新补回。
        self.projection.lock().await.forget_compactions(id);
        let mut followup = payload.clone();
        followup["clientUserMessageId"] = json!(uuid::Uuid::new_v4().to_string());
        // 修改文字仍保留原消息的图片等输入，不能在回退后悄悄丢失附件。
        let result = self.start_input(id, &followup, input, authorization).await;
        self.projection
            .lock()
            .await
            .push(json!({"method":"remote/threadReverted","params":{"threadId":id}}));
        result
    }

    pub(super) async fn respond(
        &self,
        id: &str,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let method = required(payload, "method")?;
        let owner_snapshot = self.owner_snapshot(id).await?;
        let request = if let Some(snapshot) = &owner_snapshot {
            snapshot["requests"]
                .as_array()
                .and_then(|r| {
                    r.iter()
                        .find(|r| r["id"] == payload["requestId"] && r["method"] == method)
                })
                .cloned()
        } else {
            self.projection
                .lock()
                .await
                .requests
                .values()
                .find(|r| {
                    r["params"]["threadId"] == id
                        && r["id"] == payload["requestId"]
                        && r["method"] == method
                })
                .cloned()
        }
        .ok_or_else(|| error(409, "此审批或询问已结束"))?;
        let result = approval_response(action, method, payload, &request)?;
        let owner_method = match method {
            "item/commandExecution/requestApproval" => "thread-follower-command-approval-decision",
            "item/fileChange/requestApproval" => "thread-follower-file-approval-decision",
            "item/permissions/requestApproval" => {
                "thread-follower-permissions-request-approval-response"
            }
            "item/tool/requestUserInput" => "thread-follower-submit-user-input",
            "mcpServer/elicitation/request" => {
                "thread-follower-submit-mcp-server-elicitation-response"
            }
            _ => return Err(error(400, "不支持的回答类型")),
        };
        if owner_snapshot.is_some() {
            let mut params = json!({"conversationId":id,"requestId":payload["requestId"]});
            if action == "approve" {
                params["decision"] = result["decision"].clone();
            } else {
                params["response"] = result;
            }
            if self
                .owner_request(id, owner_method, params, 1, Some(authorization))
                .await?
                .is_none()
            {
                return Err(error(409, "审批所属的会话连接已变化"));
            }
        } else {
            let generation = required(&request, "_generation")?;
            self.runtime
                .respond(generation, request["id"].clone(), result, authorization)
                .await?;
            self.projection
                .lock()
                .await
                .resolve(&super::projection::request_key(generation, &request["id"]));
        }
        Ok(json!({"ok":true,"threadId":id}))
    }

    async fn answer_async(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let request_id = required(payload, "requestId")?;
        let key = format!("{id}:{request_id}");
        if self
            .store
            .lock()
            .await
            .data
            .answered_questions
            .contains_key(&key)
        {
            return Err(error(409, "此回答已提交或结果待核对"));
        }
        let current = self.thread(&json!({"threadId":id})).await?;
        let request = current["thread"]["requests"]
            .as_array()
            .and_then(|requests| {
                requests.iter().find(|r| {
                    r["id"] == request_id && r["method"] == "remote/requestUserInputAsync"
                })
            })
            .ok_or_else(|| error(409, "此问题已结束或状态已变化"))?;
        let reply = async_reply_message(request, payload)?;
        let skip = reply.is_none();
        let message = reply
            .unwrap_or_else(|| json!({"clientUserMessageId":uuid::Uuid::new_v4().to_string()}));
        let mut store = self.store.lock().await;
        let permit = authorization.prepare_dispatch().await?;
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            store.commit(|data|{
            data.answered_questions.insert(key.clone(),json!({"state":if skip {"skipped"} else {"sending"},"clientUserMessageId":message["clientUserMessageId"]}));Ok(())
        })
        })?;
        drop(store);
        if !skip {
            self.steer(id, inputs(&message)?, &message, authorization)
                .await?;
            self.store.lock().await.commit(|data| {
                data.answered_questions.get_mut(&key).unwrap()["state"] = json!("accepted");
                Ok(())
            })?;
        }
        let mut projection = self.projection.lock().await;
        let keys: Vec<_> = projection
            .requests
            .iter()
            .filter(|(_, r)| r["id"] == request_id && r["params"]["threadId"] == id)
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            projection.resolve(&key);
        }
        Ok(json!({"ok":true,"threadId":id,"requestId":request_id,"skipped":skip}))
    }

    async fn edit_queue(
        &self,
        id: &str,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let mut store = self.store.lock().await;
        let permit = authorization.prepare_dispatch().await?;
        let mut response = json!({"ok":true,"threadId":id});
        crate::bridge::CdpDispatchPermit::commit(permit, || {
            store.commit(|data| {
                let queue = data.queues.entry(id.to_owned()).or_default();
                if action == "reorderQueued" {
                    let ids = payload["messageIds"]
                        .as_array()
                        .ok_or_else(|| error(400, "队列顺序无效"))?;
                    let expected = payload["expectedMessageIds"]
                        .as_array()
                        .ok_or_else(|| error(400, "缺少队列顺序基线"))?;
                    if ids.len() != queue.len()
                        || expected.len() != queue.len()
                        || queue.iter().zip(expected).any(|(q, id)| id != &json!(q.id))
                        || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
                        || queue
                            .iter()
                            .any(|q| !["queued", "paused"].contains(&q.state.as_str()))
                        || ids
                            .iter()
                            .any(|id| !queue.iter().any(|q| id == &json!(q.id)))
                    {
                        return Err(error(409, "队列顺序已变化"));
                    }
                    queue.sort_by_key(|q| ids.iter().position(|id| id == &json!(q.id)).unwrap());
                    return Ok(());
                }
                if matches!(
                    action,
                    "editQueued" | "cancelQueuedEdit" | "dismissQueuedEdit"
                ) && payload["editId"].is_string()
                {
                    let edit_id = required(payload, "editId")?;
                    let edit = data
                        .edits
                        .get_mut(edit_id)
                        .ok_or_else(|| error(409, "队列编辑已失效"))?;
                    if edit.thread_id != id {
                        return Err(error(409, "编辑标识不属于此会话"));
                    }
                    if action == "dismissQueuedEdit" {
                        if payload["confirmed"] != true || edit.state == "editing" {
                            return Err(error(409, "编辑尚未结束"));
                        }
                        data.edits.remove(edit_id);
                        response["editState"] = json!("dismissed");
                        return Ok(());
                    }
                    if edit.state != "editing" {
                        if edit.terminal_fingerprint.as_deref()
                            != Some(&super::super::hash(&serde_json::to_string(&json!([
                                action, payload
                            ]))?))
                        {
                            return Err(error(409, "这次编辑已按其他内容完成"));
                        }
                        response = edit.result.clone();
                        return Ok(());
                    }
                    let message = queue
                        .iter_mut()
                        .find(|q| q.id == edit.message_id && q.state == "editing")
                        .ok_or_else(|| error(409, "编辑中的队列消息已变化"))?;
                    if action == "editQueued" {
                        message.payload["prompt"] = payload["prompt"].clone();
                        let retained = payload["retainedImageSources"].as_array();
                        let mut images: Vec<_> = edit.original.payload["images"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|i| {
                                retained
                                    .is_none_or(|sources| sources.contains(&json!(image_source(i))))
                            })
                            .cloned()
                            .collect();
                        images.extend(payload["images"].as_array().into_iter().flatten().cloned());
                        message.payload["images"] = json!(images);
                        inputs(&message.payload)?;
                    } else {
                        *message = edit.original.clone();
                    }
                    message.state = "queued".into();
                    edit.state = if action == "editQueued" {
                        "committed"
                    } else {
                        "canceled"
                    }
                    .into();
                    response["editId"] = json!(edit_id);
                    response["editState"] = json!(&edit.state);
                    response["messageId"] = json!(&message.id);
                    edit.result = response.clone();
                    edit.terminal_fingerprint =
                        Some(super::super::hash(&serde_json::to_string(&json!([
                            action, payload
                        ]))?));
                    return Ok(());
                }
                let message_id = required(payload, "messageId")?;
                let index = queue
                    .iter()
                    .position(|q| q.id == message_id)
                    .ok_or_else(|| error(409, "队列消息已变化"))?;
                if !["queued", "paused"].contains(&queue[index].state.as_str()) {
                    return Err(error(409, "消息正在发送或待核对"));
                }
                if action == "removeQueued" {
                    queue.remove(index);
                    return Ok(());
                }
                if payload["originalText"] != queue[index].payload["prompt"]
                    || payload["clientUserMessageId"] != queue[index].payload["clientUserMessageId"]
                    || payload["createdAt"] != queue[index].created_at
                {
                    return Err(error(409, "队列消息的内容或身份已变化"));
                }
                if action == "beginQueuedEdit" {
                    let edit_id = required(payload, "editId")?;
                    if data.edits.contains_key(edit_id) {
                        return Err(error(409, "编辑标识已使用"));
                    }
                    let original = queue[index].clone();
                    if let Some(expected) = payload["expectedImageSources"].as_array() {
                        let current: Vec<_> = original.payload["images"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|i| json!(image_source(i)))
                            .collect();
                        if &current != expected {
                            return Err(error(409, "队列图片已变化"));
                        }
                    }
                    queue[index].state = "editing".into();
                    data.edits.insert(
                        edit_id.to_owned(),
                        Edit {
                            thread_id: id.to_owned(),
                            message_id: message_id.to_owned(),
                            state: "editing".into(),
                            original,
                            result: Value::Null,
                            terminal_fingerprint: None,
                        },
                    );
                    response["editId"] = json!(edit_id);
                    response["editState"] = json!("editing");
                    response["messageId"] = json!(message_id);
                } else {
                    queue[index].payload["prompt"] = payload["prompt"].clone();
                    inputs(&queue[index].payload)?;
                }
                Ok(())
            })
        })?;
        drop(store);
        let snapshot = self.queue(id).await;
        for key in [
            "queueEpoch",
            "queueRevision",
            "queuedMessages",
            "queuedEdits",
        ] {
            response[key] = snapshot[key].clone();
        }
        self.queue_changed(id).await;
        Ok(response)
    }
}

pub(super) fn payload_hash(payload: &Value) -> String {
    super::super::hash(&serde_json::to_string(payload).unwrap_or_default())
}

pub(super) fn async_reply_message(
    request: &Value,
    payload: &Value,
) -> anyhow::Result<Option<Value>> {
    let response = &payload["response"];
    if response["skip"] == true {
        if response.as_object().is_none_or(|object| object.len() != 1) {
            return Err(error(400, "跳过问题不能同时提交回答"));
        }
        return Ok(None);
    }
    let answers = response["answers"]
        .as_object()
        .ok_or_else(|| error(400, "询问答案无效"))?;
    let questions = request["params"]["questions"]
        .as_array()
        .ok_or_else(|| error(400, "问题内容无效"))?;
    if answers.len() != questions.len() {
        return Err(error(400, "请回答所有问题"));
    }
    let mut replies = Vec::new();
    for question in questions {
        let id = required(question, "id")?;
        let values = answers
            .get(id)
            .and_then(|answer| answer["answers"].as_array())
            .filter(|answers| answers.len() == 1)
            .ok_or_else(|| error(400, "请回答所有问题"))?;
        let answer = values[0]
            .as_str()
            .filter(|answer| !answer.trim().is_empty())
            .ok_or_else(|| error(400, "回答不能为空"))?;
        replies.push(
            json!({"questionItemId":id,"question":question["question"],"answer":answer.trim()}),
        );
    }
    Ok(Some(
        json!({"prompt":format!("<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",serde_json::to_string(&replies)?),
        "clientUserMessageId":uuid::Uuid::new_v4().to_string()}),
    ))
}

fn image_source(image: &Value) -> String {
    use sha2::{Digest, Sha256};
    image["dataUrl"]
        .as_str()
        .and_then(|source| {
            super::super::decode_data_image(source, super::super::MAX_INPUT_IMAGE_BYTES).ok()
        })
        .map(|image| format!("inline:{:x}", Sha256::digest(&image.bytes)))
        .unwrap_or_default()
}

pub(super) fn inputs(payload: &Value) -> anyhow::Result<Vec<Value>> {
    let mut input = Vec::new();
    if let Some(text) = payload["prompt"].as_str().filter(|s| !s.trim().is_empty()) {
        input.push(json!({"type":"text","text":text,"text_elements":[]}));
    }
    for image in payload["images"].as_array().into_iter().flatten() {
        input.push(json!({"type":"image","url":required(image,"dataUrl")?}));
    }
    for skill in payload["skills"].as_array().into_iter().flatten() {
        input.push(
            json!({"type":"skill","name":required(skill,"name")?,"path":required(skill,"path")?}),
        );
    }
    if input.is_empty() {
        return Err(error(400, "消息不能为空"));
    }
    Ok(input)
}

fn approval_response(
    action: &str,
    method: &str,
    payload: &Value,
    request: &Value,
) -> anyhow::Result<Value> {
    if action == "approve" {
        let decision = required(payload, "decision")?;
        if !["accept", "acceptForSession", "decline", "cancel"].contains(&decision)
            || request["params"]["availableDecisions"]
                .as_array()
                .is_some_and(|a| !a.contains(&json!(decision)))
        {
            return Err(error(400, "此请求不允许该审批决定"));
        }
        if ![
            "item/commandExecution/requestApproval",
            "item/fileChange/requestApproval",
        ]
        .contains(&method)
        {
            return Err(error(400, "此请求需要回答表单"));
        }
        return Ok(json!({"decision":decision}));
    }
    let response = &payload["response"];
    match method {
        "item/tool/requestUserInput" => {
            let answers = response["answers"]
                .as_object()
                .ok_or_else(|| error(400, "询问答案无效"))?;
            let questions = request["params"]["questions"]
                .as_array()
                .ok_or_else(|| error(400, "询问内容无效"))?;
            if answers
                .keys()
                .any(|id| !questions.iter().any(|q| q["id"] == *id))
                || questions.iter().any(|q| {
                    q["id"].as_str().is_none_or(|id| {
                        answers
                            .get(id)
                            .and_then(|a| a["answers"].as_array())
                            .is_none_or(|a| !a.iter().all(Value::is_string))
                    })
                })
            {
                return Err(error(400, "请用文本回答所有问题"));
            }
            Ok(json!({"answers":answers}))
        }
        "item/permissions/requestApproval" => {
            if !["turn", "session"].contains(&response["scope"].as_str().unwrap_or(""))
                || !response["grant"].is_boolean()
            {
                return Err(error(400, "权限决定无效"));
            }
            Ok(
                json!({"scope":response["scope"],"permissions":if response["grant"]==true {
                request["params"]["permissions"].clone()} else {json!({})}}),
            )
        }
        "mcpServer/elicitation/request" => {
            if !["accept", "decline", "cancel"].contains(&response["action"].as_str().unwrap_or(""))
            {
                return Err(error(400, "回答无效"));
            }
            Ok(response.clone())
        }
        _ => Err(error(400, "不支持的回答类型")),
    }
}
