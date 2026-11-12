use std::collections::HashSet;

use serde_json::{Value, json};

use super::projects::DesktopProjects;
use super::{Harness, error, required, sent_key};
use crate::codex_service::{
    normalize_model, normalize_owner_thread, normalize_thread, normalize_thread_response,
    owner_snapshot_history,
};

// 40 只限制一次引擎读取的条数；执行中的轮次必须读取全部分页，不能因此隐藏“继续”等开头输入。
const HISTORY_ITEM_PAGE_LIMIT: usize = 40;

struct HistoryItems {
    items: Vec<Value>,
    next_cursor: Option<String>,
    newest_snapshot_item_id: Option<String>,
}

impl HistoryItems {
    fn pagination(&self) -> Value {
        json!({"nextCursor":self.next_cursor,"hasMore":self.next_cursor.is_some(),
            "complete":self.next_cursor.is_none(),"pageComplete":true,"direction":"older","order":"asc",
            "pageLimit":HISTORY_ITEM_PAGE_LIMIT,"newestSnapshotItemId":self.newest_snapshot_item_id})
    }

    fn apply_to(self, turn: &mut Value) {
        let complete = self.next_cursor.is_none();
        turn["items"] = json!(self.items);
        // 网页直接读取每轮字段；不能只填写 itemsPaginationByTurnId，否则“加载更早”没有可用游标。
        turn["itemsView"] = json!(if complete { "full" } else { "summary" });
        turn["itemsPageComplete"] = json!(true);
        turn["itemsComplete"] = json!(complete);
        turn["itemsNextCursor"] = json!(self.next_cursor);
        turn["newestSnapshotItemId"] = json!(self.newest_snapshot_item_id);
    }

    fn prepend_visible_context(&mut self, first: Self) {
        let loaded: HashSet<_> = self
            .items
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        let first_user = first.items.iter().position(|item| {
            item["type"] == "userMessage"
                || item["type"] == "steeringUserMessage"
                    && (item["status"] == "accepted" || item["serverUserMessageId"].is_string())
        });
        let mut context: Vec<_> = first
            .items
            .into_iter()
            .enumerate()
            .filter(|(index, item)| {
                !loaded.contains(item["id"].as_str().unwrap_or(""))
                    && (first_user.is_some_and(|first| *index <= first)
                        || item["type"] == "agentMessage")
            })
            .map(|(_, mut item)| {
                // 用户要求重进会话仍看见首条输入；补回的开头不是连续分页内容，不能充当下一页定位点。
                item["remoteHistoryContext"] = json!(true);
                item
            })
            .collect();
        context.append(&mut self.items);
        self.items = context;
    }
}

fn history_cursor<'a>(value: &'a Value, label: &str) -> anyhow::Result<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|cursor| cursor.len() <= 4096)
        .map(Some)
        .ok_or_else(|| error(400, &format!("{label}无效")))
}

fn executing_history_turn(status: &Value) -> bool {
    matches!(
        status.as_str().or_else(|| status["type"].as_str()),
        Some("active" | "inProgress" | "running" | "pending")
    )
}

impl Harness {
    pub(super) async fn desktop_projects(&self) -> anyhow::Result<DesktopProjects> {
        #[cfg(test)]
        let home = self.project_codex_home.clone();
        #[cfg(not(test))]
        let home = crate::codex_home::default_codex_home_dir();
        tokio::task::spawn_blocking(move || DesktopProjects::load(&home)).await?
    }

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
        let ((mut rows, complete), project_rows, desktop, native_pins) = tokio::try_join!(
            self.catalog.snapshot(&self.runtime, &self.projection),
            self.list_all("project/list", json!({"limit":100})),
            self.desktop_projects(),
            self.native_pinned_threads()
        )?;
        // 用户要求项目效果对齐 CDP：只列出已保存项目，并保留 project/list 的桌面顺序；
        // 会话工作目录不能补成项目，否则无项目会话和历史临时目录会进入新建选择器。
        let mut projects: Vec<Value> = project_rows
            .into_iter()
            .filter_map(|mut project| {
                project["id"].as_str()?;
                let roots: Vec<_> = project["roots"]
                    .as_array()?
                    .iter()
                    .filter_map(|r| r["path"].as_str().map(str::to_owned))
                    .collect();
                project["cwd"] = roots.first().map(|s| json!(s)).unwrap_or(Value::Null);
                project["roots"] = json!(roots);
                project["threads"] = json!([]);
                desktop.display_project(&mut project);
                Some(project)
            })
            .collect();
        // 侧栏只需要置顶与排序，不能复制整份消息队列、图片和历史回执。
        let (mut thread_pins, project_pins, project_pin_overrides, preferences) = {
            let store = self.store.lock().await;
            (
                store.data.thread_pins.clone(),
                store.data.project_pins.clone(),
                store.data.project_pin_overrides.clone(),
                store.data.preferences.clone(),
            )
        };
        let pinned_threads_complete = native_pins.is_some() || complete;
        let native_pin_order = native_pins.as_ref().map(|pins| {
            pins.iter()
                .map(|row| row["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        });
        if let Some(pins) = native_pins {
            thread_pins = pins
                .iter()
                .map(|row| row["id"].as_str().unwrap().to_owned())
                .collect();
            // 置顶会话不受最近列表分页影响；合并时使用原生置顶读取的最新元数据，并按 ID 去重。
            rows.retain(|row| !thread_pins.contains(row["id"].as_str().unwrap_or("")));
            rows.extend(pins);
        }
        let mut recent = Vec::new();
        let projection = self.projection.lock().await;
        let mut side_ids = Vec::new();
        let mut unread_sides = Vec::new();
        let mut side_attention = std::collections::HashMap::<String, Value>::new();
        for (id, side) in &projection.sides.threads {
            if side.closed {
                continue;
            }
            let summary = projection.thread(id, side.view()?);
            let active = summary["priority"]["active"] == true;
            let waiting = summary["priority"]["attentionState"] == "waiting";
            let unread = side.unread && !active && !waiting;
            side_ids.push(id.clone());
            if unread {
                unread_sides.push(json!({"id":id,"parentThreadId":side.parent}));
            }
            let flags = side_attention
                .entry(side.parent.clone())
                .or_insert_with(|| json!({"active":false,"waiting":false,"unread":false}));
            flags["active"] = json!(flags["active"] == true || active);
            flags["waiting"] = json!(flags["waiting"] == true || waiting);
            flags["unread"] = json!(flags["unread"] == true || unread);
        }
        for row in rows {
            let Ok(mut thread) = normalize_thread(&row) else {
                continue;
            };
            let project_index = desktop.assigned_index(&row, &projects);
            thread["projectId"] = project_index
                .map(|index| projects[index]["id"].clone())
                .unwrap_or(Value::Null);
            thread["pinned"] = json!(thread_pins.contains(thread["id"].as_str().unwrap_or("")));
            thread = projection.thread(thread["id"].as_str().unwrap_or(""), thread.clone());
            if let Some(attention) = side_attention.get(thread["id"].as_str().unwrap_or("")) {
                thread["sideThreadAttention"] = attention.clone();
            }
            if let Some(index) = project_index {
                let project = &mut projects[index];
                if let Some(entries) = project["threads"].as_array_mut() {
                    entries.push(thread.clone());
                }
                project["recencyAt"] = json!(
                    project["recencyAt"]
                        .as_u64()
                        .unwrap_or(0)
                        .max(thread["recencyAt"].as_u64().unwrap_or(0))
                );
            }
            recent.push(thread);
        }
        drop(projection);
        for project in &mut projects {
            let id = project["id"].as_str().unwrap_or("");
            let server_id = project["serverProjectId"].as_str().unwrap_or(id);
            let pinned = project_pin_overrides
                .get(id)
                .or_else(|| project_pin_overrides.get(server_id))
                .copied()
                .unwrap_or_else(|| {
                    project_pins.contains(id)
                        || project_pins.contains(server_id)
                        || desktop
                            .pinned
                            .iter()
                            .any(|pinned| pinned == id || pinned == server_id)
                });
            project["pinned"] = json!(pinned);
        }
        // 与 CDP 一样先展示置顶项目，再按桌面项目顺序排列；未记录的新项目保留引擎顺序。
        projects.sort_by_key(|project| {
            let pinned = project["pinned"] == true;
            let order = if pinned {
                &desktop.pinned
            } else {
                &desktop.order
            };
            (
                !pinned,
                order
                    .iter()
                    .position(|id| project["id"] == *id || project["serverProjectId"] == *id)
                    .unwrap_or(usize::MAX),
            )
        });
        let mut pins: Vec<_> = recent
            .iter()
            .filter(|t| t["pinned"] == true)
            .cloned()
            .collect();
        if let Some(order) = native_pin_order {
            let positions: std::collections::HashMap<_, _> = order
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            pins.sort_by_key(|thread| positions[thread["id"].as_str().unwrap()]);
        }
        let preferences = if preferences.is_object() {
            preferences
        } else {
            json!({"mode":"project","chats":"updated_at","projects":"updated_at","pinned":"manual"})
        };
        Ok(
            json!({"ok":true,"nativeBridge":"harness","complete":complete,"projects":projects,
            "recent":recent,"pinnedThreads":pins,"pinnedThreadsComplete":pinned_threads_complete,
            "nextCursor":null,"sideThreadIds":side_ids,
            "unreadSideThreads":unread_sides,"creationAvailable":true,"preferences":preferences}),
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
        // 用户要求与 CDP 的已保存项目一致；旧缓存的合成项目不能再用于文件、Skill 或新建，
        // 无项目会话仍可通过 threadId 读取自己的工作目录，不为项目查询遍历会话。
        let (projects, desktop) = tokio::try_join!(
            self.list_all("project/list", json!({"limit":100})),
            self.desktop_projects()
        )?;
        let server_id = desktop.server_id(id);
        let project = projects
            .iter()
            .find(|project| project["id"] == server_id)
            .ok_or_else(|| error(404, "项目不存在"))?;
        let cwd = project["roots"]
            .as_array()
            .and_then(|roots| roots.first())
            .and_then(|root| root["path"].as_str())
            .ok_or_else(|| error(404, "项目没有可用工作目录"))?;
        Ok(json!({"ok":true,"hostId":"local","projectId":id,"serverProjectId":server_id,"cwd":cwd}))
    }

    pub(super) async fn thread(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        if self.known_side(id).await {
            return self.side_detail(id).await;
        }
        let (response, owner) = tokio::join!(
            self.runtime
                .read("thread/read", json!({"threadId":id,"includeTurns":false}),),
            self.owner_view_snapshot(id)
        );
        let owner = owner?;
        let response = match response {
            Ok(response) => response,
            Err(failure) => {
                if super::subagents::missing_thread_read(&failure)
                    && let Some(snapshot) = owner.as_ref()
                    && crate::codex_service::normalize_subagent_thread(snapshot).is_ok()
                {
                    return self.subagent_detail(id, snapshot, Some(snapshot)).await;
                }
                return Err(self.subagent_read_error(id, failure).await);
            }
        };
        if crate::codex_service::subagent_parent_id(&response["thread"]).is_some() {
            return self
                .subagent_detail(id, &response["thread"], owner.as_ref())
                .await;
        }
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
            // 读取历史期间也可能收到压缩完成事件，返回前重新合并当前投影，避免切回会话看到旧状态。
            self.projection
                .lock()
                .await
                .decorate_compaction(id, &mut thread);
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

    pub(super) async fn latest_turn(&self, id: &str) -> anyhow::Result<Option<Value>> {
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
        if self.known_side(id).await {
            return self.side_history(payload).await;
        }
        let identity = match self
            .runtime
            .read("thread/read", json!({"threadId":id,"includeTurns":false}))
            .await
        {
            Ok(identity) => identity,
            Err(failure) => {
                if super::subagents::missing_thread_read(&failure)
                    && let Some(owner) = self.owner_view_snapshot(id).await?
                    && crate::codex_service::normalize_subagent_thread(&owner).is_ok()
                {
                    if payload["cursor"].is_null()
                        && payload["turnId"].is_null()
                        && payload["itemCursor"].is_null()
                    {
                        if let Some(history) = owner_snapshot_history(&owner, id) {
                            return Ok(history);
                        }
                    }
                    return Err(error(503, "子智能体的桌面历史快照尚未完整，请稍后重试"));
                }
                return Err(self.subagent_read_error(id, failure).await);
            }
        };
        anyhow::ensure!(identity["thread"]["id"] == id, "会话历史身份不匹配");
        let subagent = crate::codex_service::subagent_parent_id(&identity["thread"]).is_some();
        if subagent {
            crate::codex_service::normalize_subagent_thread(&identity["thread"])?;
            if identity["thread"]["ephemeral"] == true {
                return self.ephemeral_subagent_history(id, payload).await;
            }
        } else {
            normalize_thread_response(&identity)?;
        }
        let cursor = history_cursor(&payload["cursor"], "会话分页游标")?;
        if cursor.is_none() && payload["turnId"].is_null() {
            if let Some(snapshot) = self.owner_view_snapshot(id).await? {
                if !subagent {
                    self.follow_owner(id, snapshot.clone()).await;
                }
                if let Some(history) = owner_snapshot_history(&snapshot, id) {
                    return Ok(if subagent {
                        history
                    } else {
                        self.projection.lock().await.history(id, history, true)
                    });
                }
            }
        }
        if let Some(turn_id) = payload["turnId"].as_str() {
            // 用户要求服务模式可以实际翻页；网页沿用 CDP 契约发送 itemCursor，不能误读成轮次 cursor。
            let item_cursor = history_cursor(&payload["itemCursor"], "消息分页游标")?;
            let page = self
                .history_turn_items(id, turn_id, &payload["turnStatus"], item_cursor)
                .await?;
            let history = json!({"ok":true,"scope":"items","threadId":id,"turnId":turn_id,"data":page.items,
                "itemOrder":"asc","itemDirection":"older","itemsPageComplete":true,
                "itemsComplete":page.next_cursor.is_none(),"itemsNextCursor":page.next_cursor,
                "newestSnapshotItemId":page.newest_snapshot_item_id,"pageLimit":HISTORY_ITEM_PAGE_LIMIT,"transcriptEvents":[]});
            return Ok(self.projection.lock().await.history(id, history, false));
        }
        let page = self
            .runtime
            .read(
                "thread/turns/list",
                json!({"threadId":id,"cursor":cursor,
            "limit":3,"itemsView":"notLoaded","sortDirection":"desc"}),
            )
            .await?;
        let mut turns = page["data"]
            .as_array()
            .ok_or_else(|| error(503, "会话历史分页返回无效"))?
            .clone();
        let next_cursor = history_cursor(&page["nextCursor"], "引擎会话分页游标")
            .map_err(|_| error(503, "引擎会话分页游标无效"))?;
        if cursor.is_some() && next_cursor == cursor {
            return Err(error(503, "会话历史分页游标未前进"));
        }
        let mut pagination = serde_json::Map::new();
        // 每页最多 3 轮，最多并发 3 次内容读取；保留轮次和消息原有顺序。
        let item_reads = turns.iter().map(|turn| async move {
            let turn_id = required(turn, "id")?;
            self.history_turn_items(id, turn_id, &turn["status"], None)
                .await
        });
        let items = futures_util::future::try_join_all(item_reads).await?;
        for (turn, items) in turns.iter_mut().zip(items) {
            let turn_id = required(turn, "id")?.to_owned();
            pagination.insert(turn_id, items.pagination());
            items.apply_to(turn);
        }
        let history = json!({"ok":true,"scope":"turns","threadId":id,"data":turns,
            "nextCursor":next_cursor,"backwardsCursor":page["backwardsCursor"],
            "turnOrder":"desc","itemOrder":"asc","turnsPageComplete":true,
            "turnsComplete":next_cursor.is_none(),"itemsPaginationByTurnId":pagination,"transcriptEvents":[]});
        Ok(self
            .projection
            .lock()
            .await
            .history(id, history, cursor.is_none()))
    }

    async fn history_items_page(
        &self,
        id: &str,
        turn_id: &str,
        cursor: Option<&str>,
        direction: &str,
    ) -> anyhow::Result<HistoryItems> {
        let page = self
            .runtime
            .read(
                "thread/items/list",
                json!({"threadId":id,"turnId":turn_id,"cursor":cursor,
                "limit":HISTORY_ITEM_PAGE_LIMIT,"sortDirection":direction}),
            )
            .await?;
        let entries = page["data"]
            .as_array()
            .ok_or_else(|| error(503, "会话消息分页返回无效"))?;
        history_cursor(&page["nextCursor"], "引擎消息分页游标")
            .map_err(|_| error(503, "引擎消息分页游标无效"))?;
        for entry in entries {
            let item = entry.get("item").unwrap_or(entry);
            if !item.is_object()
                || item["id"].as_str().is_none_or(str::is_empty)
                || entry.get("item").is_some()
                    && entry.get("turnId").is_some_and(|value| value != turn_id)
            {
                return Err(error(503, "会话消息内容或分页归属无效"));
            }
        }
        // 首屏、升序开头与单轮翻页共用新版包装解析，仍兼容旧版裸 item 数组。
        let (mut items, next_cursor) =
            crate::codex_service::normalize_history_items_page(&page, cursor)?;
        for (item, entry) in items.iter_mut().zip(entries.iter().rev()) {
            if entry.get("item").is_some() {
                for field in ["startedAtMs", "completedAtMs"] {
                    if item[field].is_null() && entry[field].is_number() {
                        item[field] = entry[field].clone();
                    }
                }
            }
        }
        if direction == "asc" {
            items.reverse();
        }
        let newest_snapshot_item_id = items
            .last()
            .and_then(|item| item["id"].as_str())
            .map(str::to_owned);
        Ok(HistoryItems {
            items,
            next_cursor,
            newest_snapshot_item_id,
        })
    }

    async fn history_turn_items(
        &self,
        id: &str,
        turn_id: &str,
        status: &Value,
        cursor: Option<&str>,
    ) -> anyhow::Result<HistoryItems> {
        let mut page = self.history_items_page(id, turn_id, cursor, "desc").await?;
        if !executing_history_turn(status) {
            if cursor.is_none() && page.next_cursor.is_some() {
                let first = self.history_items_page(id, turn_id, None, "asc").await?;
                page.prepend_visible_context(first);
            }
            return Ok(page);
        }
        // 用户要求停止后“继续”的输入在重开时可见；执行中读完全部页，终态轮次仍按原方式懒加载。
        let newest_snapshot_item_id = page.newest_snapshot_item_id;
        let mut pages = vec![page.items];
        let mut next_cursor = page.next_cursor;
        let mut seen = HashSet::new();
        if let Some(cursor) = cursor {
            seen.insert(cursor.to_owned());
        }
        while let Some(cursor) = next_cursor {
            if !seen.insert(cursor.clone()) {
                return Err(error(503, "会话消息分页游标循环"));
            }
            page = self
                .history_items_page(id, turn_id, Some(&cursor), "desc")
                .await?;
            pages.push(page.items);
            next_cursor = page.next_cursor;
        }
        let mut loaded = HashSet::new();
        let items = pages
            .into_iter()
            .rev()
            .flatten()
            .filter(|item| loaded.insert(item["id"].as_str().unwrap_or("").to_owned()))
            .collect();
        Ok(HistoryItems {
            items,
            next_cursor: None,
            newest_snapshot_item_id,
        })
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
        if self.known_side(id).await {
            let mut projection = self.projection.lock().await;
            let side = projection.sides.threads.get_mut(id).unwrap();
            let client = payload["clientUserMessageId"].as_str().unwrap();
            let evidence = side.turns.iter().find_map(|turn| {
                turn["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| {
                        (item["type"] == "userMessage"
                            || item["type"] == "steeringUserMessage"
                                && item["status"] == "accepted")
                            && (item["clientUserMessageId"] == client || item["clientId"] == client)
                    })
                    .map(|item| {
                        json!({"ok":true,"confirmed":true,"deliveryState":"accepted","threadId":id,
                        "clientUserMessageId":client,"turnId":turn["id"],"messageId":item["id"]})
                    })
            });
            if let Some(result) = evidence {
                if let Some(sent) = side.sent.get_mut(client) {
                    sent.1 = result.clone();
                }
                return Ok(result);
            }
            return Ok(side.sent.get(client).map(|(_, result)| result.clone()).unwrap_or_else(||
                json!({"ok":true,"confirmed":false,"deliveryState":"unknown","threadId":id,"clientUserMessageId":client})));
        }
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
        if let Some(message) = queued.as_ref().filter(|q| {
            matches!(
                q.state.as_str(),
                "sending" | "unknown" | "direct-sending" | "direct-unknown"
            )
        }) {
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
