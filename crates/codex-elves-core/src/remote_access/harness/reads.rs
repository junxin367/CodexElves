use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Harness, error, required, sent_key};
use crate::codex_service::{
    normalize_model, normalize_owner_thread, normalize_thread, normalize_thread_response,
    owner_snapshot_history,
};

impl Harness {
    pub(super) async fn list_all(
        &self,
        method: &str,
        mut params: Value,
    ) -> anyhow::Result<Vec<Value>> {
        let mut all = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let page = self.runtime.read(method, params.clone()).await?;
            all.extend(page["data"].as_array().into_iter().flatten().cloned());
            let Some(cursor) = page["nextCursor"].as_str().filter(|s| !s.is_empty()) else {
                break;
            };
            if !seen.insert(cursor.to_owned()) {
                return Err(error(503, "会话分页游标重复"));
            }
            params["cursor"] = json!(cursor);
        }
        Ok(all)
    }

    pub(super) async fn sidebar(&self) -> anyhow::Result<Value> {
        // 会话与项目互不依赖；首屏只等最近一页，后续目录由服务后台补齐。
        let ((rows, complete), project_rows) = tokio::try_join!(
            self.catalog.snapshot(&self.runtime, &self.projection),
            self.list_all("project/list", json!({"limit":100}))
        )?;
        let mut projects: BTreeMap<String, Value> = project_rows
            .into_iter()
            .filter_map(|mut project| {
                let id = project["id"].as_str()?.to_owned();
                let roots: Vec<_> = project["roots"]
                    .as_array()?
                    .iter()
                    .filter_map(|r| r["path"].as_str().map(str::to_owned))
                    .collect();
                project["cwd"] = roots.first().map(|s| json!(s)).unwrap_or(Value::Null);
                project["roots"] = json!(roots);
                project["threads"] = json!([]);
                Some((id, project))
            })
            .collect();
        // 侧栏只需要置顶与排序，不能复制整份消息队列、图片和历史回执。
        let (thread_pins, project_pins, preferences) = {
            let store = self.store.lock().await;
            (
                store.data.thread_pins.clone(),
                store.data.project_pins.clone(),
                store.data.preferences.clone(),
            )
        };
        let mut recent = Vec::new();
        let projection = self.projection.lock().await;
        for row in rows {
            let Ok(mut thread) = normalize_thread(&row) else {
                continue;
            };
            let cwd = thread["cwd"].as_str().unwrap_or("").to_owned();
            let project_id = row["projectId"]
                .as_str()
                .filter(|id| projects.contains_key(*id))
                .map(str::to_owned)
                .or_else(|| {
                    projects
                        .iter()
                        .find(|(_, p)| {
                            p["roots"].as_array().is_some_and(|roots| {
                                roots.iter().any(|root| {
                                    root.as_str().is_some_and(|path| same_path(path, &cwd))
                                })
                            })
                        })
                        .map(|(id, _)| id.clone())
                })
                .unwrap_or_else(|| workspace_id(&cwd));
            let project = projects.entry(project_id.clone()).or_insert_with(|| json!({
                "id":project_id,"name":std::path::Path::new(&cwd).file_name().and_then(|s|s.to_str()).unwrap_or(&cwd),
                "cwd":cwd,"roots":[cwd],"threads":[],"recencyAt":0,"synthetic":true
            }));
            thread["projectId"] = json!(&project_id);
            thread["pinned"] = json!(thread_pins.contains(thread["id"].as_str().unwrap_or("")));
            thread = projection.thread(thread["id"].as_str().unwrap_or(""), thread.clone());
            if let Some(entries) = project["threads"].as_array_mut() {
                entries.push(thread.clone());
            }
            project["pinned"] = json!(project_pins.contains(&project_id));
            project["recencyAt"] = json!(
                project["recencyAt"]
                    .as_u64()
                    .unwrap_or(0)
                    .max(thread["recencyAt"].as_u64().unwrap_or(0))
            );
            recent.push(thread);
        }
        drop(projection);
        for (id, project) in &mut projects {
            project["pinned"] = json!(project_pins.contains(id));
        }
        let pins: Vec<_> = recent
            .iter()
            .filter(|t| t["pinned"] == true)
            .cloned()
            .collect();
        let preferences = if preferences.is_object() {
            preferences
        } else {
            json!({"mode":"project","chats":"updated_at","projects":"updated_at","pinned":"manual"})
        };
        Ok(
            json!({"ok":true,"nativeBridge":"harness","complete":complete,"projects":projects.into_values().collect::<Vec<_>>(),
            "recent":recent,"pinnedThreads":pins,"nextCursor":null,"sideThreadIds":[],
            "unreadSideThreads":[],"creationAvailable":true,"preferences":preferences}),
        )
    }

    pub(super) async fn workspace_context(&self, payload: &Value) -> anyhow::Result<Value> {
        if let Some(id) = payload["threadId"].as_str() {
            let response = self
                .runtime
                .read("thread/read", json!({"threadId":id,"includeTurns":false}))
                .await?;
            return Ok(
                json!({"ok":true,"hostId":"local","threadId":id,"cwd":response["thread"]["cwd"]}),
            );
        }
        let id = required(payload, "projectId")?;
        // 项目文件、Skill 和新建会话只读项目根目录，不为获取路径重扫全部会话。
        if !id.starts_with("workspace-") {
            let projects = self.list_all("project/list", json!({"limit":100})).await?;
            let project = projects
                .iter()
                .find(|project| project["id"] == id)
                .ok_or_else(|| error(404, "项目不存在"))?;
            let cwd = project["roots"]
                .as_array()
                .and_then(|roots| roots.first())
                .and_then(|root| root["path"].as_str())
                .ok_or_else(|| error(404, "项目没有可用工作目录"))?;
            return Ok(json!({"ok":true,"hostId":"local","projectId":id,"cwd":cwd}));
        }
        let project = self
            .catalog
            .find(&self.runtime, &self.projection, |row| {
                row["cwd"]
                    .as_str()
                    .is_some_and(|cwd| workspace_id(cwd) == id)
            })
            .await?
            .ok_or_else(|| error(404, "项目不存在"))?;
        Ok(json!({"ok":true,"hostId":"local","projectId":id,"cwd":project["cwd"]}))
    }

    pub(super) async fn thread(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let (response, owner) = tokio::try_join!(
            self.runtime
                .read("thread/read", json!({"threadId":id,"includeTurns":false}),),
            self.owner_snapshot(id)
        )?;
        let mut thread = normalize_thread_response(&response)?;
        if let Some(settings) = self.store.lock().await.data.settings.get(id) {
            for key in ["model", "reasoningEffort", "serviceTier"] {
                if thread[key].is_null() {
                    thread[key] = settings[key].clone();
                }
            }
        }
        let mut latest;
        if let Some(snapshot) = owner {
            thread = normalize_owner_thread(&snapshot, Some(&thread))?;
            latest = crate::desktop_owner_ipc::snapshot_turns(&snapshot)
                .into_iter()
                .last();
            self.follow_owner(id, snapshot).await;
        } else {
            thread = self.projection.lock().await.thread(id, thread);
            // 最近一轮与目标互不依赖，并行读取；控制和派发校验不使用短时读缓存。
            let (goal, history) = tokio::try_join!(
                self.runtime.read("thread/goal/get", json!({"threadId":id})),
                self.latest_turn(id)
            )?;
            thread["goal"] = goal["goal"].clone();
            latest = history;
        }
        if latest.is_none() {
            latest = self.latest_turn(id).await?;
        }
        thread["editableMessage"] = Value::Null;
        if let Some(turn) = &latest {
            let turn_id = turn
                .get("id")
                .or_else(|| turn.get("turnId"))
                .cloned()
                .unwrap_or(Value::Null);
            let items = turn["items"].as_array().cloned().unwrap_or_default();
            if turn["status"] != "inProgress"
                && !items.iter().any(|i| i["type"] == "steeringUserMessage")
            {
                if let Some(message) = items.iter().find(|i| i["type"] == "userMessage") {
                    thread["editableMessage"] =
                        json!({"turnId":turn_id,"messageId":message["id"],"editable":true});
                }
            }
            if turn["status"] == "inProgress" {
                let requests = thread["requests"].as_array_mut();
                if let Some(requests) = requests {
                    for item in &items {
                        if let Some(request) = super::projection::async_question(id, &turn_id, item)
                        {
                            if !requests.iter().any(|r| r["id"] == request["id"]) {
                                requests.push(request);
                            }
                        }
                    }
                }
            }
        }
        {
            let store = self.store.lock().await;
            if let Some(requests) = thread["requests"].as_array_mut() {
                requests.retain(|r| {
                    !store
                        .data
                        .answered_questions
                        .contains_key(&format!("{id}:{}", r["id"].as_str().unwrap_or("")))
                });
            }
        }
        let queue = self.queue(id).await;
        if queue["queuedMessages"]
            .as_array()
            .is_some_and(|q| !q.is_empty())
            || queue["queuedEdits"]
                .as_array()
                .is_some_and(|q| !q.is_empty())
        {
            thread["editableMessage"] = Value::Null;
        }
        for key in [
            "queuedMessages",
            "queuedEdits",
            "queueEpoch",
            "queueRevision",
        ] {
            thread[key] = queue[key].clone();
        }
        Ok(json!({"ok":true,"thread":thread,"nativeBridge":"harness"}))
    }

    async fn latest_turn(&self, id: &str) -> anyhow::Result<Option<Value>> {
        let history = self
            .runtime
            .read(
                "thread/turns/list",
                json!({"threadId":id,"limit":1,"itemsView":"full","sortDirection":"desc"}),
            )
            .await?;
        Ok(history["data"]
            .as_array()
            .and_then(|turns| turns.first())
            .cloned())
    }

    pub(super) async fn history(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        if payload["cursor"].is_null() && payload["turnId"].is_null() {
            if let Some(snapshot) = self.owner_snapshot(id).await? {
                self.follow_owner(id, snapshot.clone()).await;
                if let Some(history) = owner_snapshot_history(&snapshot, id) {
                    return Ok(history);
                }
            }
        }
        if let Some(turn_id) = payload["turnId"].as_str() {
            let page = self
                .runtime
                .read(
                    "thread/items/list",
                    json!({"threadId":id,"turnId":turn_id,
                "cursor":payload["cursor"],"limit":40,"sortDirection":"desc"}),
                )
                .await?;
            let mut items = page["data"].as_array().cloned().unwrap_or_default();
            items.reverse();
            return Ok(
                json!({"ok":true,"scope":"items","threadId":id,"turnId":turn_id,"data":items,
                "itemOrder":"asc","itemDirection":"older","itemsPageComplete":true,
                "itemsComplete":page["nextCursor"].is_null(),"itemsNextCursor":page["nextCursor"],
                "newestSnapshotItemId":null,"pageLimit":40,"transcriptEvents":[]}),
            );
        }
        let page = self
            .runtime
            .read(
                "thread/turns/list",
                json!({"threadId":id,"cursor":payload["cursor"],
            "limit":3,"itemsView":"notLoaded","sortDirection":"desc"}),
            )
            .await?;
        let mut turns = page["data"].as_array().cloned().unwrap_or_default();
        let mut pagination = serde_json::Map::new();
        // 每页最多 3 轮，最多并发 3 次内容读取；保留轮次和消息原有顺序。
        let item_reads = turns.iter().map(|turn| async move {
            let turn_id = required(turn, "id")?;
            self.runtime
                .read(
                    "thread/items/list",
                    json!({"threadId":id,"turnId":turn_id,"limit":40,"sortDirection":"desc"}),
                )
                .await
        });
        let items = futures_util::future::try_join_all(item_reads).await?;
        for (turn, items) in turns.iter_mut().zip(items) {
            let turn_id = required(turn, "id")?.to_owned();
            let mut rows = items["data"].as_array().cloned().unwrap_or_default();
            rows.reverse();
            turn["items"] = json!(rows);
            pagination.insert(turn_id,json!({"nextCursor":items["nextCursor"],
                "hasMore":!items["nextCursor"].is_null(),"complete":items["nextCursor"].is_null(),
                "pageComplete":true,"direction":"older","order":"asc","pageLimit":40,"newestSnapshotItemId":null}));
        }
        Ok(json!({"ok":true,"scope":"turns","threadId":id,"data":turns,
            "nextCursor":page["nextCursor"],"backwardsCursor":page["backwardsCursor"],
            "turnOrder":"desc","itemOrder":"asc","turnsPageComplete":true,
            "turnsComplete":page["nextCursor"].is_null(),"itemsPaginationByTurnId":pagination,"transcriptEvents":[]}))
    }

    pub(super) async fn models(&self, payload: &Value) -> anyhow::Result<Value> {
        if payload["forceReload"] == true {
            self.runtime.invalidate_read("model/list").await;
            self.runtime.invalidate_read("config/read").await;
        }
        let (model_rows, config) = tokio::try_join!(
            self.list_all("model/list", json!({"limit":100,"includeHidden":false})),
            self.runtime
                .read("config/read", json!({"cwd":null,"includeLayers":false}))
        )?;
        let models: Vec<_> = model_rows.iter().filter_map(normalize_model).collect();
        Ok(
            json!({"ok":true,"data":models,"ultraReasoningEnabled":models.iter().any(|m|
            m["supportedReasoningEfforts"].as_array().is_some_and(|efforts|
                efforts.iter().any(|e|e["reasoningEffort"]=="ultra"))),
            "defaultModel":config["config"]["model"],
            "defaultReasoningEffort":config["config"]["model_reasoning_effort"]}),
        )
    }

    pub(super) async fn skills(&self, payload: &Value) -> anyhow::Result<Value> {
        let context = self.workspace_context(payload).await?;
        let cwd = required(&context, "cwd")?;
        let params = json!({"cwds":[cwd],"forceReload":payload["forceReload"]==true});
        let response = if payload["forceReload"] == true {
            self.runtime.invalidate_read("skills").await;
            self.runtime.request("skills/list", params, None).await?
        } else {
            self.runtime.read("skills/list", params).await?
        };
        let entry = response["data"]
            .as_array()
            .and_then(|entries| entries.iter().find(|e| e["cwd"] == cwd));
        Ok(
            json!({"ok":true,"cwd":cwd,"data":entry.map(|e|e["skills"].clone()).unwrap_or_else(||json!([])),
            "errors":entry.map(|e|e["errors"].clone()).unwrap_or_else(||json!([]))}),
        )
    }

    pub(super) async fn send_state(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        required(payload, "clientUserMessageId")?;
        let key = sent_key(id, payload);
        if let Some(result) = self.store.lock().await.data.sent.get(&key).cloned() {
            return Ok(result);
        }
        let queued = self
            .store
            .lock()
            .await
            .data
            .queues
            .get(id)
            .into_iter()
            .flatten()
            .find(|q| q.payload["clientUserMessageId"] == payload["clientUserMessageId"])
            .cloned();
        if let Some(message) = queued
            .as_ref()
            .filter(|q| matches!(q.state.as_str(), "sending" | "unknown"))
        {
            // 断线后只用同一客户端消息 ID 的历史证据确认送达；找不到证据继续保留未知状态。
            if let Ok(history) = self
                .runtime
                .request(
                    "thread/turns/list",
                    json!({"threadId":id,"limit":10,"itemsView":"full","sortDirection":"desc"}),
                    None,
                )
                .await
            {
                for turn in history["data"].as_array().into_iter().flatten() {
                    if let Some(item) =
                        turn["items"].as_array().into_iter().flatten().find(|item| {
                            (item["type"] == "userMessage"
                                || item["type"] == "steeringUserMessage"
                                    && item["status"] == "accepted")
                                && (item["clientUserMessageId"] == payload["clientUserMessageId"]
                                    || item["clientId"] == payload["clientUserMessageId"])
                        })
                    {
                        let result = json!({"ok":true,"confirmed":true,"deliveryState":"accepted","threadId":id,
                            "clientUserMessageId":payload["clientUserMessageId"],"turnId":turn["id"],"messageId":item["id"]});
                        self.store.lock().await.commit(|data| {
                            data.sent.insert(key.clone(), result.clone());
                            data.sent_hashes.insert(
                                key.clone(),
                                super::commands::payload_hash(&message.payload),
                            );
                            if let Some(queue) = data.queues.get_mut(id) {
                                queue.retain(|q| q.id != message.id);
                            }
                            Ok(())
                        })?;
                        self.queue_changed(id).await;
                        return Ok(result);
                    }
                }
            }
        }
        Ok(match queued {
            Some(message) if matches!(message.state.as_str(), "queued" | "editing" | "paused") => {
                json!({
                "ok":true,"confirmed":true,"deliveryState":"queued","threadId":id,
                "clientUserMessageId":payload["clientUserMessageId"],"messageId":message.id})
            }
            _ => json!({"ok":true,"confirmed":false,"deliveryState":"unknown","threadId":id,
                "clientUserMessageId":payload["clientUserMessageId"]}),
        })
    }

    pub(super) async fn queue_edit(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let edit_id = required(payload, "editId")?;
        let mut snapshot = self.queue(id).await;
        let store = self.store.lock().await;
        let edit = store.data.edits.get(edit_id);
        if edit.is_some_and(|e| e.thread_id != id) {
            return Err(error(409, "编辑标识不属于此会话"));
        }
        snapshot["editId"] = json!(edit_id);
        snapshot["editState"] = json!(edit.map(|e| e.state.as_str()).unwrap_or("missing"));
        snapshot["canResume"] = json!(edit.is_some_and(|e| e.state == "editing"));
        snapshot["canDismiss"] = json!(edit.is_none_or(|e| e.state != "editing"));
        snapshot["operationPending"] = json!(false);
        snapshot["draftText"] = edit
            .map(|e| e.original.payload["prompt"].clone())
            .unwrap_or(json!(""));
        snapshot["result"] = edit.map(|e| e.result.clone()).unwrap_or(Value::Null);
        Ok(snapshot)
    }
}

fn same_path(left: &str, right: &str) -> bool {
    let left = left.trim_end_matches(['/', '\\']).replace('\\', "/");
    let right = right.trim_end_matches(['/', '\\']).replace('\\', "/");
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn workspace_id(cwd: &str) -> String {
    format!(
        "workspace-{:x}",
        Sha256::digest(cwd.to_ascii_lowercase().as_bytes())
    )
}
