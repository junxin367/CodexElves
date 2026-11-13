use std::collections::{HashMap, HashSet};

use anyhow::Context;
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};

use super::{Harness, error, required};
use crate::codex_service::{normalize_subagent_thread, normalize_thread, subagent_parent_id};

// 只保留当前连接的少量子智能体元数据；正文通过只读接口读取，不在服务状态文件持久化临时会话。
const MAX_OBSERVED_CHILDREN: usize = 512;
const READ_CONCURRENCY: usize = 4;

pub(super) fn missing_thread_read(failure: &anyhow::Error) -> bool {
    failure
        .downcast_ref::<super::super::ApiError>()
        .is_some_and(|error| {
            let message = error.message.to_lowercase();
            error.code == 409
                && [
                    "thread not loaded",
                    "no rollout found",
                    "unknown thread",
                    "thread not found",
                ]
                .iter()
                .any(|pattern| message.contains(pattern))
        })
}

#[derive(Default)]
pub(super) struct SubagentState {
    pub threads: HashMap<String, Value>,
    pub candidates: HashMap<String, HashSet<String>>,
}

pub(super) fn candidate_ids(item: &Value) -> HashSet<String> {
    let kind = item["type"]
        .as_str()
        .unwrap_or("")
        .replace('_', "")
        .to_lowercase();
    if !matches!(
        kind.as_str(),
        "subagentactivity" | "collabagenttoolcall" | "multiagentaction"
    ) {
        return HashSet::new();
    }
    let mut ids = HashSet::new();
    for key in ["agentThreadId", "agent_thread_id"] {
        if let Some(id) = item[key].as_str() {
            ids.insert(id.to_owned());
        }
    }
    for key in ["receiverThreadIds", "receiver_thread_ids"] {
        for id in item[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            ids.insert(id.to_owned());
        }
    }
    for row in item["receiverThreads"].as_array().into_iter().flatten() {
        if let Some(id) = row["threadId"].as_str() {
            ids.insert(id.to_owned());
        }
    }
    for key in ["agentsStates", "agents_states"] {
        ids.extend(
            item[key]
                .as_object()
                .into_iter()
                .flat_map(|states| states.keys())
                .cloned(),
        );
    }
    ids.retain(|id| !id.is_empty() && id.len() <= 120);
    ids
}

impl SubagentState {
    pub fn remember(&mut self, raw: &Value) {
        if normalize_subagent_thread(raw).is_err() {
            return;
        }
        let id = raw["id"].as_str().unwrap();
        if self.threads.len() >= MAX_OBSERVED_CHILDREN && !self.threads.contains_key(id) {
            if let Some(oldest) = self.threads.keys().next().cloned() {
                self.threads.remove(&oldest);
            }
        }
        let entry = self
            .threads
            .entry(id.to_owned())
            .or_insert_with(|| json!({}));
        entry["_unavailable"] = json!(raw["_unavailable"] == true);
        for key in [
            "id",
            "cwd",
            "name",
            "title",
            "source",
            "threadSource",
            "parentThreadId",
            "ephemeral",
            "agentNickname",
            "agentRole",
            "status",
            "createdAt",
            "updatedAt",
            "model",
            "reasoningEffort",
            "_latestTurn",
        ] {
            if let Some(value) = raw.get(key) {
                entry[key] = value.clone();
            }
        }
    }

    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut seen = HashSet::from([id.to_owned()]);
        let mut result = Vec::new();
        let mut current = id;
        while let Some(parent) = self.threads.get(current).and_then(subagent_parent_id) {
            if !seen.insert(parent.to_owned()) {
                break;
            }
            result.push(parent.to_owned());
            current = parent;
        }
        result
    }

    pub fn observe(&mut self, frame: &Value) -> (bool, Vec<String>) {
        let method = frame["method"].as_str().unwrap_or("");
        let id = frame["params"]["threadId"]
            .as_str()
            .or_else(|| frame["params"]["thread"]["id"].as_str())
            .unwrap_or("");
        if method == "thread/started" {
            self.remember(&frame["params"]["thread"]);
        }
        let mut changed = Vec::new();
        let candidates = candidate_ids(&frame["params"]["item"]);
        if !id.is_empty() && !candidates.is_empty() {
            if self.candidates.len() < MAX_OBSERVED_CHILDREN || self.candidates.contains_key(id) {
                let remembered = self.candidates.entry(id.to_owned()).or_default();
                for candidate in candidates {
                    if remembered.len() >= MAX_OBSERVED_CHILDREN {
                        break;
                    }
                    remembered.insert(candidate);
                }
            }
            changed.push(id.to_owned());
        }
        let Some(thread) = self.threads.get_mut(id) else {
            return (false, changed);
        };
        match method {
            "thread/closed" | "thread/unloaded" => {
                thread["_unavailable"] = json!(true);
                thread["status"] = json!({"type":"notLoaded"});
            }
            "thread/status/changed" => thread["status"] = frame["params"]["status"].clone(),
            "turn/started" | "turn/completed" => {
                let turn = &frame["params"]["turn"];
                thread["_latestTurn"] = json!({"id":turn["id"],"status":turn["status"],
                    "turnStartedAtMs":turn["turnStartedAtMs"],"startedAt":turn["startedAt"]});
            }
            "thread/settings/updated" => {
                let settings = frame["params"]
                    .get("threadSettings")
                    .unwrap_or(&frame["params"]);
                for key in ["model", "reasoningEffort"] {
                    if let Some(value) = settings.get(key) {
                        thread[key] = value.clone();
                    }
                }
            }
            _ => {}
        }
        // 文本 delta 直接刷新只读正文；只有状态或轮次变化才刷新父列表，避免每个输出片段都触发目录补读。
        if matches!(
            method,
            "thread/started"
                | "thread/closed"
                | "thread/unloaded"
                | "thread/status/changed"
                | "turn/started"
                | "turn/completed"
                | "thread/settings/updated"
        ) {
            changed.extend(self.ancestors(id));
        }
        (true, changed)
    }
}

fn belongs_to(root: &str, id: &str, rows: &HashMap<String, Value>) -> bool {
    let mut seen = HashSet::from([id]);
    let mut current = id;
    while let Some(parent) = rows.get(current).and_then(subagent_parent_id) {
        if parent == root {
            return true;
        }
        if !seen.insert(parent) {
            return false;
        }
        current = parent;
    }
    false
}

fn page_cursor(page: &Value, seen: &mut HashSet<String>) -> anyhow::Result<Option<String>> {
    match &page["nextCursor"] {
        Value::Null => Ok(None),
        Value::String(cursor) if !cursor.is_empty() && cursor.len() <= 4096 => {
            anyhow::ensure!(seen.insert(cursor.clone()), "子智能体分页游标循环");
            Ok(Some(cursor.clone()))
        }
        _ => Err(error(503, "子智能体分页游标无效")),
    }
}

impl Harness {
    async fn read_subagent_candidate(&self, id: &str) -> anyhow::Result<Value> {
        match self
            .runtime
            .read("thread/read", json!({"threadId":id,"includeTurns":false}))
            .await
        {
            Ok(response) => {
                anyhow::ensure!(response["thread"]["id"] == id, "子智能体读取身份不匹配");
                Ok(response["thread"].clone())
            }
            Err(failure) => {
                if !missing_thread_read(&failure) {
                    return Err(failure);
                }
                // 桌面持有的临时子智能体可能只存在于其内存；使用既有只读 IPC，不启用桌面注入或接管。
                let Some(mut owner) = self.owner_view_snapshot(id).await? else {
                    return Err(failure);
                };
                anyhow::ensure!(owner["id"] == id, "子智能体持有者身份不匹配");
                normalize_subagent_thread(&owner)?;
                if !owner["threadRuntimeStatus"].is_null() {
                    owner["status"] = owner["threadRuntimeStatus"].clone();
                }
                let turns = crate::desktop_owner_ipc::snapshot_turns(&owner);
                if let Some(turn) = turns.last() {
                    owner["_latestTurn"] = json!({"id":turn["turnId"],"status":turn["status"],
                        "turnStartedAtMs":turn["turnStartedAtMs"],"startedAt":turn["startedAt"]});
                }
                Ok(owner)
            }
        }
    }
    pub(super) async fn known_subagent(&self, id: &str) -> Option<Value> {
        self.projection
            .lock()
            .await
            .subagents
            .threads
            .get(id)
            .cloned()
    }

    pub(super) async fn subagent_read_error(
        &self,
        id: &str,
        failure: anyhow::Error,
    ) -> anyhow::Error {
        if missing_thread_read(&failure)
            && self
                .known_subagent(id)
                .await
                .is_some_and(|raw| raw["ephemeral"] == true)
        {
            error(410, "临时子智能体已不可读取，请返回主会话；不会重新启动它")
        } else {
            failure
        }
    }

    pub(super) async fn subagent_detail(
        &self,
        id: &str,
        raw: &Value,
        owner: Option<&Value>,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(raw["id"] == id, "子智能体身份不匹配");
        let mut metadata = raw.clone();
        if let Some(owner) = owner {
            anyhow::ensure!(owner["id"] == id, "子智能体快照身份不匹配");
            for (source, target) in [
                ("threadRuntimeStatus", "status"),
                ("latestModel", "model"),
                ("latestReasoningEffort", "reasoningEffort"),
            ] {
                if let Some(value) = owner.get(source).filter(|value| !value.is_null()) {
                    metadata[target] = value.clone();
                }
            }
        }
        self.projection.lock().await.subagents.remember(&metadata);
        let mut thread = normalize_subagent_thread(&metadata)?;
        thread["nativeBridge"] = json!("harness");
        thread["goal"] = Value::Null;
        Ok(json!({"ok":true,"thread":thread,"nativeBridge":"harness"}))
    }

    pub(super) async fn ephemeral_subagent_history(
        &self,
        id: &str,
        payload: &Value,
    ) -> anyhow::Result<Value> {
        // 临时子会话只读取当前快照；不会为查看内容 resume、接管或重新启动已结束的子智能体。
        if let Some(owner) = self.owner_view_snapshot(id).await? {
            if payload["cursor"].is_null()
                && payload["turnId"].is_null()
                && payload["itemCursor"].is_null()
            {
                if let Some(history) = crate::codex_service::owner_snapshot_history(&owner, id) {
                    return Ok(history);
                }
            }
            return Err(error(503, "临时子智能体的桌面历史快照尚未完整，请稍后重试"));
        }
        if !payload["cursor"].is_null() || !payload["itemCursor"].is_null() {
            return Err(error(400, "临时子智能体使用当前完整快照，请刷新后读取"));
        }
        let response = match self
            .runtime
            .read("thread/read", json!({"threadId":id,"includeTurns":true}))
            .await
        {
            Ok(response) => response,
            Err(failure) if missing_thread_read(&failure) => {
                return Err(error(
                    410,
                    "临时子智能体已不可读取，请返回主会话；不会重新启动它",
                ));
            }
            Err(failure) => {
                // 当前引擎可能禁止临时会话 includeTurns；能力不足或短暂断连不是会话失效，不能误返回 410。
                if failure
                    .downcast_ref::<super::super::ApiError>()
                    .is_some_and(|api| {
                        api.code == 409
                            && api
                                .message
                                .contains("ephemeral threads do not support includeTurns")
                    })
                {
                    return Err(error(
                        503,
                        "当前引擎未提供临时子智能体内容快照，请在桌面查看",
                    ));
                }
                return Err(failure);
            }
        };
        let raw = &response["thread"];
        anyhow::ensure!(raw["id"] == id, "临时子智能体快照身份不匹配");
        normalize_subagent_thread(raw)?;
        let mut turns = raw["turns"]
            .as_array()
            .ok_or_else(|| error(503, "临时子智能体尚无可读取的历史快照"))?
            .clone();
        let mut pagination = serde_json::Map::new();
        for turn in &mut turns {
            let turn_id = required(turn, "id")?.to_owned();
            if turn["itemsView"] == "notLoaded" || turn["itemsComplete"] == false {
                return Err(error(503, "临时子智能体快照内容尚未完整"));
            }
            let items = turn["items"]
                .as_array()
                .ok_or_else(|| error(503, "临时子智能体快照内容无效"))?;
            let reversed: Vec<_> = items.iter().rev().cloned().collect();
            let (items, _) = crate::codex_service::normalize_history_items_page(
                &json!({"data":reversed,"nextCursor":null}),
                None,
            )?;
            turn["items"] = json!(items);
            turn["itemsView"] = json!("full");
            turn["itemsComplete"] = json!(true);
            turn["itemsPageComplete"] = json!(true);
            turn["itemsNextCursor"] = Value::Null;
            pagination.insert(
                turn_id,
                json!({"complete":true,"pageComplete":true,
                "hasMore":false,"nextCursor":null,"direction":"older","order":"asc"}),
            );
        }
        if let Some(turn_id) = payload["turnId"].as_str() {
            let turn = turns
                .iter()
                .find(|turn| turn["id"] == turn_id)
                .ok_or_else(|| error(404, "子智能体轮次已不可读取"))?;
            return Ok(
                json!({"ok":true,"scope":"items","threadId":id,"turnId":turn_id,
                "data":turn["items"],"itemsComplete":true,"itemsPageComplete":true,
                "itemsNextCursor":null,"itemOrder":"asc","itemDirection":"older","transcriptEvents":[]}),
            );
        }
        turns.reverse();
        Ok(
            json!({"ok":true,"scope":"turns","threadId":id,"data":turns,"nextCursor":null,
            "turnOrder":"desc","itemOrder":"asc","turnsComplete":true,"turnsPageComplete":true,
            "itemsPaginationByTurnId":pagination,"transcriptEvents":[],"nativeBridge":"harness"}),
        )
    }

    async fn subagent_list_pages(
        &self,
        method: &str,
        mut params: Value,
    ) -> anyhow::Result<Vec<Value>> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let page = self.runtime.read(method, params.clone()).await?;
            rows.extend(
                page["data"]
                    .as_array()
                    .context("子智能体列表返回无效")?
                    .iter()
                    .cloned(),
            );
            let Some(cursor) = page_cursor(&page, &mut seen)? else {
                return Ok(rows);
            };
            params["cursor"] = json!(cursor);
        }
    }

    pub(super) async fn related_subagents(&self, payload: &Value) -> anyhow::Result<Value> {
        let root = required(payload, "threadId")?;
        let generation = self.runtime.connection().await?.generation.clone();
        let root_read = self
            .runtime
            .read("thread/read", json!({"threadId":root,"includeTurns":false}))
            .await?;
        let parent = &root_read["thread"];
        anyhow::ensure!(parent["id"] == root, "父会话读取身份不匹配");
        normalize_thread(parent).or_else(|_| normalize_subagent_thread(parent))?;
        let source_kinds = json!([
            "cli",
            "vscode",
            "exec",
            "appServer",
            "subAgent",
            "subAgentReview",
            "subAgentCompact",
            "subAgentThreadSpawn",
            "subAgentOther",
            "unknown"
        ]);
        let mut rows = HashMap::new();
        let mut expected = HashSet::new();
        let mut complete = true;
        // 用户要求完整父子关系：读取所有后代及归档页；临时子智能体从同一引擎的已加载目录与事件补齐。
        for archived in [false, true] {
            let page = self
                .subagent_list_pages(
                    "thread/list",
                    json!({
                        "ancestorThreadId":root,"sourceKinds":source_kinds,"modelProviders":[],
                        "archived":archived,"limit":100,"cursor":null,"useStateDbOnly":true
                    }),
                )
                .await?;
            for row in page {
                if normalize_subagent_thread(&row).is_ok() {
                    let id = required(&row, "id")?.to_owned();
                    expected.insert(id.clone());
                    rows.insert(id, row);
                } else {
                    complete = false;
                }
            }
        }
        let mut candidates = {
            let projection = self.projection.lock().await;
            for (id, row) in &projection.subagents.threads {
                if !rows.contains_key(id) {
                    rows.insert(id.clone(), row.clone());
                }
            }
            projection
                .subagents
                .candidates
                .get(root)
                .cloned()
                .unwrap_or_default()
        };
        match self
            .subagent_list_pages("thread/loaded/list", json!({"limit":100,"cursor":null}))
            .await
        {
            Ok(ids) => {
                for id in ids {
                    candidates.insert(id.as_str().context("已加载子会话 ID 无效")?.to_owned());
                }
            }
            Err(_) => complete = false,
        }
        // 主会话的最近活动还会包含桌面持有的临时子智能体 ID；读取不会 resume 或争抢执行归属。
        if let Ok(Some(turn)) = self.latest_turn(root).await {
            for item in turn["items"].as_array().into_iter().flatten() {
                candidates.extend(candidate_ids(item));
            }
        }
        if let Some(owner) = self.owner_view_snapshot(root).await? {
            for turn in crate::desktop_owner_ipc::snapshot_turns(&owner) {
                for item in turn["items"].as_array().into_iter().flatten() {
                    candidates.extend(candidate_ids(item));
                }
            }
        }
        candidates.remove(root);
        for id in rows.keys() {
            candidates.insert(id.clone());
        }
        let reads = stream::iter(candidates)
            .map(|id| async move {
                let result = self.read_subagent_candidate(&id).await;
                (id, result)
            })
            .buffer_unordered(READ_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        for (id, result) in reads {
            match result {
                Ok(result) if result["id"] == id => {
                    if normalize_subagent_thread(&result).is_ok() {
                        let mut fresh = result;
                        if fresh["_latestTurn"].is_null()
                            && let Some(latest) =
                                rows.get(&id).and_then(|row| row.get("_latestTurn"))
                        {
                            fresh["_latestTurn"] = latest.clone();
                        }
                        rows.insert(id, fresh);
                    } else {
                        rows.remove(&id);
                    }
                }
                _ => {
                    if let Some(row) = rows.get_mut(&id) {
                        row["_unavailable"] = json!(true);
                    }
                    complete = false;
                }
            }
        }
        // 缺失中间父节点时逐层只读核实；不以工具调用里的 ID 直接建立父子归属。
        for _ in 0..64 {
            let missing: HashSet<_> = rows
                .values()
                .filter_map(subagent_parent_id)
                .filter(|parent| *parent != root && !rows.contains_key(*parent))
                .map(str::to_owned)
                .collect();
            if missing.is_empty() {
                break;
            }
            let mut added = false;
            for id in missing {
                if let Ok(read) = self.read_subagent_candidate(&id).await {
                    if normalize_subagent_thread(&read).is_ok() {
                        rows.insert(id, read);
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        let confirmed: Vec<_> = rows
            .iter()
            .filter(|(id, _)| belongs_to(root, id, &rows))
            .map(|(_, row)| row.clone())
            .collect();
        complete &= expected
            .iter()
            .all(|id| confirmed.iter().any(|row| row["id"] == *id));
        let summaries = stream::iter(confirmed).map(|raw| async move {
            let id = required(&raw, "id")?;
            let latest = self.latest_turn(id).await;
            let turn = latest.as_ref().ok().and_then(Option::as_ref).unwrap_or(&raw["_latestTurn"]);
            let status = raw["status"].as_str().or_else(|| raw["status"]["type"].as_str()).unwrap_or("");
            let waiting = status == "waiting" || raw["status"]["activeFlags"].as_array()
                .is_some_and(|flags| flags.iter().any(|flag| flag == "waitingOnApproval" || flag == "waitingOnUserInput"));
            let terminal = matches!(turn["status"].as_str(), Some("completed" | "failed" | "interrupted" | "cancelled" | "canceled"));
            let state = if raw["_unavailable"] == true && !terminal { "unknown" }
                else if waiting { "waiting" } else if status == "active" || turn["status"] == "inProgress" { "active" }
                else if terminal { "done" } else { "unknown" };
            let normalized = normalize_subagent_thread(&raw)?;
            let started = turn["turnStartedAtMs"].as_u64().or_else(|| turn["startedAt"].as_u64().map(|s| s.saturating_mul(1000)));
            Ok::<_, anyhow::Error>((raw.clone(), json!({
                "id":id,"name":normalized["agentName"],"parentThreadId":subagent_parent_id(&raw),
                "ephemeral":raw["ephemeral"].as_bool().unwrap_or(false),"status":state,
                "outcome":turn["status"],"startedAtMs":started,
                "elapsedMs":started.map(|time| super::super::now_ms().saturating_sub(time))
            }), state != "unknown"))
        }).buffer_unordered(READ_CONCURRENCY).collect::<Vec<_>>().await;
        let mut active = Vec::new();
        let mut finished = Vec::new();
        {
            let mut projection = self.projection.lock().await;
            anyhow::ensure!(
                projection.generation == generation,
                "子智能体读取期间连接已变化，请重试"
            );
            for result in summaries {
                let (raw, summary, known) = result?;
                projection.subagents.remember(&raw);
                complete &= known;
                if summary["status"] == "done" {
                    finished.push(summary);
                } else {
                    active.push(summary);
                }
            }
        }
        for items in [&mut active, &mut finished] {
            items.sort_by(|a, b| {
                a["startedAtMs"]
                    .as_u64()
                    .cmp(&b["startedAtMs"].as_u64())
                    .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
            });
        }
        anyhow::ensure!(
            self.runtime.connection().await?.generation == generation,
            "子智能体读取期间连接已变化，请重试"
        );
        Ok(
            json!({"ok":true,"threadId":root,"subagents":active,"completedSubagentCount":finished.len(),
            "completedSubagents":finished,"subagentsComplete":complete,"subagentsLoading":false,
            "subagentsError":if complete {Value::Null} else {json!("部分子智能体状态暂不可用，请重试")},
            "sideThreads":[],"canCreateSideThread":false}),
        )
    }
}
