use std::collections::HashMap;

use serde_json::{Value, json};

use super::super::{ApiError, RemoteAuthorization, now_ms};
use super::store::{Data, Queued};
use super::{Harness, error, required};

// 用户要求服务独立管理且过期后仍可读取：最多保留 16 个未关闭会话（含过期），每个正文最多 16 MiB，
// 合计正文上限约 256 MiB；达到上限明确停止接受新输入，不静默截断或落盘恢复。
const MAX_OPEN_SIDES: usize = 16;
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
// 侧边待发队列与正文分别限制为 16 MiB，条数沿用主会话的 100 条上限，避免临时图片无限占用内存。
const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;

// 用户要求与 Codex App 保持相同任务边界：继承历史仅供参考，侧边只能执行边界后的新要求。
// 开发者指令持续约束后续轮次，历史边界消息标明分界位置；两处共用规则，不能只清空界面正文。
const SIDE_CONVERSATION_INSTRUCTIONS: &str = r#"You are a side-conversation assistant, separate from the main thread.
Treat inherited parent-thread history before the side-conversation boundary as reference context only. Follow only user instructions submitted after that boundary.

- Do not present yourself as continuing the main thread's active task.
- Do not continue, execute, or complete instructions, plans, tool calls, approvals, edits, or requests that appear only in inherited history.
- Treat tool calls and outputs before the boundary as past activity in the parent thread, not active work or authorization.
- Answer questions and perform lightweight, non-mutating inspection without disrupting the main thread. If no user question has been submitted after the boundary, wait for one.
- Do not interact with existing or new sub-agents, even if sub-agents were used in the inherited history.
- Do not modify files, source, git state, permissions, configuration, or other workspace state unless the user explicitly requests that mutation after the boundary. Keep requested mutations minimal, local to the request, and avoid disrupting the main thread.
- Request escalated permissions or broader sandbox access only when necessary for an operation the user explicitly requests after the boundary, including a read-only operation, and when the thread's approval policy permits it.
- Send messages to another thread only with explicit user authorization after the boundary."#;

fn side_boundary_items(parent: &str) -> Value {
    let reference = json!({"threadId":parent});
    let text = format!(
        "Side conversation boundary.\n\n\
         Everything before this boundary is inherited history from the parent thread. \
         It is reference context only. It is not your current task.\n\n\
         {SIDE_CONVERSATION_INSTRUCTIONS}\n\n\
         Parent chat: {reference}\n\
         Use this reference with read_thread, send_message_to_thread, or wait_threads only when those tools are available."
    );
    json!([{"type":"message","role":"user","content":[{"type":"input_text","text":text}]}])
}

fn side_permissions(response: &Value) -> anyhow::Result<Value> {
    let approval = &response["approvalPolicy"];
    let reviewer = &response["approvalsReviewer"];
    let sandbox = &response["sandbox"];
    if !(approval.is_string() || approval.is_object())
        || !reviewer.is_string()
        || !sandbox["type"].is_string()
    {
        return Err(error(503, "无法确认侧边会话继承的原生权限"));
    }
    let mut permissions = json!({"approvalPolicy":approval,"approvalsReviewer":reviewer});
    // 与 App 一样保留原生权限配置 ID 和工作区根；自定义配置不能折算成更宽松的普通预设。
    if let Some(profile) = response["activePermissionProfile"]["id"].as_str() {
        permissions["permissions"] = json!(profile);
    } else {
        permissions["sandboxPolicy"] = sandbox.clone();
    }
    if response["runtimeWorkspaceRoots"].is_array() {
        permissions["runtimeWorkspaceRoots"] = response["runtimeWorkspaceRoots"].clone();
    }
    Ok(permissions)
}

fn apply_side_fork_permissions(params: &mut Value, permissions: &Value) -> anyhow::Result<()> {
    if !(permissions["approvalPolicy"].is_string() || permissions["approvalPolicy"].is_object())
        || !permissions["approvalsReviewer"].is_string()
    {
        return Err(error(503, "无法确认主会话当前的批准权限"));
    }
    params["approvalPolicy"] = permissions["approvalPolicy"].clone();
    params["approvalsReviewer"] = permissions["approvalsReviewer"].clone();
    if let Some(profile) = permissions["activePermissionProfile"]["id"]
        .as_str()
        .or_else(|| permissions["permissions"].as_str())
    {
        params["permissions"] = json!(profile);
    } else {
        let sandbox = &permissions["sandboxPolicy"];
        params["sandbox"] = json!(match sandbox["type"].as_str() {
            Some("readOnly") => "read-only",
            Some("workspaceWrite") => "workspace-write",
            Some("dangerFullAccess") => "danger-full-access",
            _ => return Err(error(409, "无法继承主会话当前的沙盒权限")),
        });
        if sandbox["type"] == "workspaceWrite" {
            for (key, field) in [
                ("writable_roots", "writableRoots"),
                ("network_access", "networkAccess"),
                ("exclude_tmpdir_env_var", "excludeTmpdirEnvVar"),
                ("exclude_slash_tmp", "excludeSlashTmp"),
            ] {
                if let Some(value) = sandbox.get(field) {
                    params["config"][format!("sandbox_workspace_write.{key}")] = value.clone();
                }
            }
        }
    }
    if permissions["runtimeWorkspaceRoots"].is_array() {
        params["runtimeWorkspaceRoots"] = permissions["runtimeWorkspaceRoots"].clone();
    }
    Ok(())
}

pub(super) struct SideThread {
    pub parent: String,
    pub generation: String,
    pub metadata: Value,
    pub turns: Vec<Value>,
    pub closed: bool,
    pub expired: bool,
    pub closing: bool,
    pub incomplete: bool,
    pub unread: bool,
    pub sent: HashMap<String, (String, Value)>,
    // 用户要求侧边执行中普通发送也排队；仅复用队列/编辑/完成门控结构，绝不写入持久 Store。
    pub queue: Data,
    pub queue_revision: u64,
    // 用户要求侧边继承创建时的主会话权限且不提供切换入口；只在本次临时会话内保存原生权限快照。
    permissions: Value,
    bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side() -> SideThread {
        SideThread {
            parent: "main".into(),
            generation: "engine".into(),
            metadata: json!({"id":"side","cwd":"test","name":"侧边聊天","ephemeral":true,"status":{"type":"idle"}}),
            turns: Vec::new(),
            closed: false,
            expired: false,
            closing: false,
            incomplete: false,
            unread: false,
            sent: HashMap::new(),
            queue: Data::default(),
            queue_revision: 1,
            permissions: json!({"approvalPolicy":"on-request","approvalsReviewer":"user",
                "sandboxPolicy":{"type":"readOnly"}}),
            bytes: 0,
        }
    }

    #[test]
    fn harness_service_side_history_keeps_input_and_tools_when_completion_only_has_summary() {
        let mut side = side();
        side.observe(&json!({"method":"turn/started","params":{"turn":{"id":"turn","status":"inProgress","items":[]}}}));
        for index in 0..96 {
            let item = if index == 0 {
                json!({"id":"item-0","type":"userMessage","content":[{"type":"text","text":"首条侧边输入"}]})
            } else {
                json!({"id":format!("item-{index}"),"type":"agentMessage","text":format!("输出 {index}")})
            };
            side.observe(
                &json!({"method":"item/completed","params":{"turnId":"turn","item":item}}),
            );
        }
        let active = side.history(&json!({"threadId":"side"})).unwrap();
        assert_eq!(active["data"][0]["items"].as_array().unwrap().len(), 96);
        side.observe(&json!({"method":"turn/completed","params":{"turn":{
            "id":"turn","status":"completed","itemsView":"summary",
            "items":[{"id":"item-95","type":"agentMessage","text":"最后输出"}]}}}));
        let first = side.history(&json!({"threadId":"side"})).unwrap();
        assert_eq!(first["data"][0]["items"][0]["id"], "item-0");
        assert_eq!(first["data"][0]["items"][0]["remoteHistoryContext"], true);
        let second = side
            .history(&json!({"threadId":"side","turnId":"turn",
            "itemCursor":first["data"][0]["itemsNextCursor"]}))
            .unwrap();
        assert_eq!(second["scope"], "items");
        assert_eq!(second["data"].as_array().unwrap().len(), 40);
        assert_eq!(second["data"][0]["id"], "item-16");
        assert_eq!(side.turns[0]["items"].as_array().unwrap().len(), 96);
        assert!(side.unread);
    }

    #[test]
    fn harness_service_side_stream_and_generation_expiry_preserve_read_only_history() {
        let mut state = SideState::default();
        state.threads.insert("side".into(), side());
        for frame in [
            json!({"method":"turn/started","params":{"threadId":"side","turn":{"id":"turn","status":"inProgress","items":[]}}}),
            json!({"method":"item/started","params":{"threadId":"side","turnId":"turn","item":{"id":"answer","type":"agentMessage","text":""}}}),
            json!({"method":"item/agentMessage/delta","params":{"threadId":"side","turnId":"turn","itemId":"answer","delta":"实时片段"}}),
        ] {
            state.observe(&frame);
        }
        let history = state.threads["side"]
            .history(&json!({"threadId":"side"}))
            .unwrap();
        assert_eq!(history["data"][0]["items"][0]["text"], "实时片段");
        state.expire();
        state.observe(&json!({"method":"turn/started","params":{"threadId":"side","turn":{"id":"late","items":[]}}}));
        let retained = &state.threads["side"];
        assert!(!retained.closed);
        assert_eq!(retained.view().unwrap()["sideThreadExpired"], true);
        assert_eq!(retained.view().unwrap()["readOnly"], true);
        assert_eq!(retained.turns.len(), 1, "过期后拒绝迟到事件，保留最后正文");
        assert_eq!(
            retained.history(&json!({"threadId":"side"})).unwrap()["data"][0]["items"][0]["text"],
            "实时片段"
        );
    }

    #[test]
    fn harness_service_side_expiry_events_keep_history_until_explicit_close() {
        for method in [
            "thread/unloaded",
            "thread/closed",
            "harness/disconnected",
            "harness/connected",
        ] {
            let mut projection = super::super::projection::Projection::default();
            projection.ingest(
                json!({"method":"harness/connected","_generation":"engine","_generationIndex":1}),
            );
            let mut side = side();
            side.unread = true;
            side.turns.push(json!({"id":"turn","status":"completed",
                "items":[{"id":"reply","type":"agentMessage","text":"最终正文"}]}));
            projection.register_side("side".into(), side, json!({}));
            projection.active.insert("side".into(), "turn".into());
            projection
                .compactions
                .insert("side".into(), json!({"state":"running"}));
            projection.requests.insert(
                "request".into(),
                json!({"id":1,"method":"item/tool/requestUserInput","params":{"threadId":"side"}}),
            );
            let generation = if method == "harness/connected" {
                "new"
            } else {
                "engine"
            };
            projection.ingest(json!({"method":method,"_generation":generation,
                "_generationIndex":2,"params":{"threadId":"side"}}));
            let retained = &projection.sides.threads["side"];
            assert!(retained.expired, "{method}");
            assert!(!retained.closed, "{method}");
            assert_eq!(retained.turns[0]["items"][0]["text"], "最终正文");
            let view = projection.thread("side", retained.view().unwrap());
            assert_eq!(view["readOnly"], true);
            assert_eq!(view["priority"]["active"], false);
            assert_eq!(view["priority"]["attentionState"], "unread");
            assert_eq!(view["requests"], json!([]));
            projection.ingest(json!({"method":"turn/started","_generation":generation,
                "params":{"threadId":"side","turn":{"id":"late","status":"inProgress"}}}));
            assert_eq!(projection.sides.threads["side"].turns.len(), 1);
            projection.close_side("side");
            assert!(projection.sides.threads["side"].turns.is_empty());
            assert!(projection.sides.threads["side"].closed);
        }
    }
}

#[derive(Default)]
pub(super) struct SideState {
    pub threads: HashMap<String, SideThread>,
}

impl SideThread {
    pub fn view(&self) -> anyhow::Result<Value> {
        let mut view = crate::codex_service::normalize_thread_metadata(&self.metadata)?;
        view["viewKind"] = json!("side");
        view["serviceSide"] = json!(true);
        view["serviceTier"] = self.metadata["serviceTier"].clone();
        view["fastMode"] = json!(self.metadata["fastMode"] == true);
        view["permissionMode"] = self.metadata["permissionMode"].clone();
        view["parentThreadId"] = json!(self.parent);
        view["ephemeral"] = json!(true);
        view["sideHistoryVersion"] = json!(1);
        view["inheritedTurnIds"] = json!([]);
        view["sideThreadClosed"] = json!(self.closed);
        view["sideThreadExpired"] = json!(self.expired);
        view["hasUnreadTurn"] = json!(self.unread);
        view["readOnly"] = json!(self.closed || self.closing || self.expired);
        view["editableMessage"] = Value::Null;
        view["goal"] = Value::Null;
        let queue = self.queue_snapshot();
        for key in [
            "queuedMessages",
            "queuedEdits",
            "queueEpoch",
            "queueRevision",
        ] {
            view[key] = queue[key].clone();
        }
        view["capabilities"]["remoteQueue"] = json!(true);
        view["capabilities"]["sessionPermissions"] = json!(false);
        Ok(view)
    }

    pub fn queue_snapshot(&self) -> Value {
        self.queue.queue(
            self.metadata["id"].as_str().unwrap_or(""),
            &self.generation,
            self.queue_revision,
        )
    }

    pub fn clear_queue(&mut self) {
        self.queue = Data::default();
        self.queue_revision += 1;
    }

    pub fn change_queue(
        &mut self,
        change: impl FnOnce(&mut Data) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut next = self.queue.clone();
        change(&mut next)?;
        // 与 Store::commit 同步保留最近 256 份已结束编辑回执并释放原图；编辑中的草稿仍计入 16 MiB 上限。
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
        if serde_json::to_vec(&next)?.len() > MAX_QUEUE_BYTES {
            return Err(error(
                429,
                "侧边会话的待发送内容已达 16 MiB，请先处理现有队列",
            ));
        }
        self.queue = next;
        self.queue_revision += 1;
        Ok(())
    }

    pub fn expire(&mut self) {
        if self.closed {
            return;
        }
        // 用户要求过期后可补读最终内容并确认已读；保留有界内存正文，禁止恢复执行或落盘。
        self.expired = true;
        self.closing = false;
        self.sent.clear();
        self.clear_queue();
        self.metadata["status"] = json!({"type":"idle"});
        for turn in &mut self.turns {
            if turn["status"] == "inProgress" {
                turn["status"] = json!("interrupted");
            }
        }
    }

    fn upsert_item(&mut self, turn_id: &str, item: Value) {
        if self.incomplete {
            return;
        }
        let Some(turn) = self.turns.iter_mut().find(|turn| turn["id"] == turn_id) else {
            self.incomplete = true;
            return;
        };
        let Some(items) = turn["items"].as_array_mut() else {
            return;
        };
        let index = items
            .iter()
            .position(|previous| previous["id"] == item["id"]);
        let previous_bytes = index.map_or(0, |index| items[index].to_string().len());
        let next_bytes = self.bytes.saturating_sub(previous_bytes) + item.to_string().len();
        if next_bytes > MAX_HISTORY_BYTES {
            self.incomplete = true;
            return;
        }
        self.bytes = next_bytes;
        match index {
            Some(index) => items[index] = item,
            None => items.push(item),
        }
    }

    fn observe(&mut self, frame: &Value) {
        let params = &frame["params"];
        match frame["method"].as_str().unwrap_or("") {
            "thread/closed" | "thread/unloaded" => {
                if self.closing {
                    self.closed = true;
                    self.closing = false;
                    self.turns.clear();
                    self.sent.clear();
                    self.clear_queue();
                    self.bytes = 0;
                } else {
                    self.expire();
                }
            }
            "thread/status/changed" => self.metadata["status"] = params["status"].clone(),
            "turn/started" | "turn/completed" => {
                let raw = &params["turn"];
                let Some(id) = raw["id"].as_str() else { return };
                if !self.turns.iter().any(|turn| turn["id"] == id) {
                    self.turns.push(json!({"id":id,"items":[]}));
                }
                let turn = self.turns.iter_mut().find(|turn| turn["id"] == id).unwrap();
                // turn/completed 在当前协议只带摘要 items，不能覆盖已接收的首条输入和工具记录。
                for key in ["status", "error", "startedAt", "completedAt", "durationMs"] {
                    if let Some(value) = raw.get(key) {
                        turn[key] = value.clone();
                    }
                }
                for item in raw["items"].as_array().into_iter().flatten() {
                    self.upsert_item(id, item.clone());
                }
                let completed = frame["method"] == "turn/completed";
                if completed {
                    self.queue.finish_queue(
                        self.metadata["id"].as_str().unwrap_or(""),
                        id,
                        if raw["error"].is_null() {
                            raw["status"].as_str().unwrap_or("unknown")
                        } else {
                            "failed"
                        },
                    );
                } else {
                    self.queue.wait_queue(
                        self.metadata["id"].as_str().unwrap_or(""),
                        Some(id.to_owned()),
                        false,
                    );
                }
                self.queue_revision += 1;
                self.metadata["status"] = json!({"type":if completed {"idle"} else {"active"}});
                self.unread = completed;
            }
            "item/started" | "item/completed" => {
                if let Some(id) = params["turnId"].as_str() {
                    self.upsert_item(id, params["item"].clone());
                }
            }
            method if method.ends_with("/delta") || method.ends_with("Delta") => {
                let Some(turn) = self
                    .turns
                    .iter()
                    .find(|turn| turn["id"] == params["turnId"])
                else {
                    return;
                };
                let Some(mut item) = turn["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| item["id"] == params["itemId"])
                    .cloned()
                else {
                    return;
                };
                let delta = params["delta"].as_str().unwrap_or("");
                let field = match method {
                    "item/agentMessage/delta" | "item/plan/delta" => Some("text"),
                    "item/commandExecution/outputDelta" => Some("aggregatedOutput"),
                    _ => None,
                };
                if let Some(field) = field {
                    item[field] = json!(format!("{}{delta}", item[field].as_str().unwrap_or("")));
                } else if method == "item/reasoning/summaryTextDelta" {
                    let index = params["summaryIndex"].as_u64().unwrap_or(0) as usize;
                    if index < 1024 {
                        if !item["summary"].is_array() {
                            item["summary"] = json!([]);
                        }
                        let summary = item["summary"].as_array_mut().unwrap();
                        while summary.len() <= index {
                            summary.push(json!(""));
                        }
                        summary[index] =
                            json!(format!("{}{delta}", summary[index].as_str().unwrap_or("")));
                    }
                }
                if let Some(id) = params["turnId"].as_str() {
                    self.upsert_item(id, item);
                }
            }
            _ => {}
        }
    }

    pub fn history(&self, payload: &Value) -> anyhow::Result<Value> {
        if self.incomplete {
            return Err(error(
                503,
                "临时侧边会话的完整记录不可用，请保留当前消息并关闭此侧边会话",
            ));
        }
        let mut turns: Vec<_> = self.turns.iter().rev().cloned().collect();
        // 只展示创建之后的侧边输入；模型继承的主会话上下文不混入侧边聊天区。
        if let Some(id) = payload["turnId"].as_str() {
            let turn = turns
                .iter()
                .find(|turn| turn["id"] == id)
                .ok_or_else(|| error(404, "侧边会话轮次不存在"))?;
            let (items, next) = item_page(turn, &payload["itemCursor"])?;
            return Ok(
                json!({"ok":true,"scope":"items","threadId":payload["threadId"],"turnId":id,
                "data":items,"itemsComplete":next.is_none(),"itemsPageComplete":true,
                "itemsNextCursor":next,"itemOrder":"asc","itemDirection":"older","ephemeral":true,"transcriptEvents":[]}),
            );
        }
        let start = cursor_index(&turns, &payload["cursor"], "side-turn:")?;
        let has_more = start + 3 < turns.len();
        turns = turns.into_iter().skip(start).take(3).collect();
        let next = if has_more {
            turns
                .last()
                .and_then(|turn| turn["id"].as_str())
                .map(|id| format!("side-turn:{id}"))
        } else {
            None
        };
        let mut pagination = serde_json::Map::new();
        for turn in &mut turns {
            let (items, cursor) = item_page(turn, &Value::Null)?;
            turn["items"] = json!(items);
            turn["itemsComplete"] = json!(cursor.is_none());
            turn["itemsPageComplete"] = json!(true);
            turn["itemsView"] = json!(if cursor.is_none() { "full" } else { "summary" });
            turn["itemsNextCursor"] = json!(cursor);
            pagination.insert(turn["id"].as_str().unwrap().to_owned(),
                json!({"complete":cursor.is_none(),"pageComplete":true,"nextCursor":cursor,"hasMore":cursor.is_some()}));
        }
        Ok(
            json!({"ok":true,"scope":"turns","threadId":payload["threadId"],"data":turns,"nextCursor":next,
            "turnOrder":"desc","itemOrder":"asc","turnsPageComplete":true,"turnsComplete":next.is_none(),
            "ephemeral":true,"sideHistoryVersion":1,"inheritedTurnIds":[],"itemsPaginationByTurnId":pagination,
            "transcriptEvents":[],"nativeBridge":"harness"}),
        )
    }
}

fn cursor_index(items: &[Value], cursor: &Value, prefix: &str) -> anyhow::Result<usize> {
    if cursor.is_null() {
        return Ok(0);
    }
    let id = cursor
        .as_str()
        .and_then(|value| value.strip_prefix(prefix))
        .ok_or_else(|| error(400, "侧边会话分页游标无效"))?;
    items
        .iter()
        .position(|item| item["id"] == id)
        .map(|index| index + 1)
        .ok_or_else(|| error(409, "侧边会话记录已变化，请重新读取"))
}

fn item_page(turn: &Value, cursor: &Value) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let reversed: Vec<_> = turn["items"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .cloned()
        .collect();
    let start = cursor_index(&reversed, cursor, "side-item:")?;
    // 执行中的一轮保留全部输入和工具；完成后按 40 条向前翻页，与主会话页面的分页方向一致。
    let limit = if turn["status"] == "inProgress" {
        reversed.len()
    } else {
        40
    };
    let data: Vec<_> = reversed.iter().skip(start).take(limit).cloned().collect();
    let next = if start + data.len() < reversed.len() {
        data.last()
            .and_then(|item| item["id"].as_str())
            .map(|id| format!("side-item:{id}"))
    } else {
        None
    };
    let (mut items, _) = crate::codex_service::normalize_history_items_page(
        &json!({"data":data,"nextCursor":null}),
        None,
    )?;
    if cursor.is_null() && next.is_some() {
        if let Some(first) = turn["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["type"] == "userMessage")
        {
            if !items.iter().any(|item| item["id"] == first["id"]) {
                let mut first = first.clone();
                first["remoteHistoryContext"] = json!(true);
                items.insert(0, first);
            }
        }
    }
    Ok((items, next))
}

impl SideState {
    pub fn expire(&mut self) {
        for side in self.threads.values_mut() {
            side.expire();
        }
    }

    pub fn observe(&mut self, frame: &Value) -> Option<(String, bool)> {
        let id = frame["params"]["threadId"]
            .as_str()
            .or_else(|| frame["params"]["thread"]["id"].as_str())?;
        let side = self.threads.get_mut(id)?;
        if side.closed || side.expired {
            return None;
        }
        side.observe(frame);
        matches!(
            frame["method"].as_str(),
            Some(
                "turn/started"
                    | "turn/completed"
                    | "thread/status/changed"
                    | "thread/closed"
                    | "thread/unloaded"
            )
        )
        .then(|| (side.parent.clone(), side.closed))
    }
}

impl Harness {
    pub async fn known_side(&self, id: &str) -> bool {
        self.projection.lock().await.sides.threads.contains_key(id)
    }

    async fn side_generation(&self, id: &str) -> anyhow::Result<String> {
        let connection = self.runtime.connection().await?;
        let projection = self.projection.lock().await;
        let side = projection
            .sides
            .threads
            .get(id)
            .ok_or_else(|| error(409, "此会话不是服务创建的侧边会话"))?;
        if side.closed || side.expired || side.generation != connection.generation {
            return Err(error(410, "侧边会话已过期，不能恢复或重新执行"));
        }
        Ok(side.generation.clone())
    }

    pub(super) async fn side_detail(&self, id: &str) -> anyhow::Result<Value> {
        let projection = self.projection.lock().await;
        let side = &projection.sides.threads[id];
        if side.closed {
            return Err(error(410, "侧边会话已关闭，内容已释放"));
        }
        let thread = projection.thread(id, side.view()?);
        Ok(json!({"ok":true,"thread":thread,"nativeBridge":"harness"}))
    }

    pub(super) async fn side_history(&self, payload: &Value) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        let projection = self.projection.lock().await;
        let side = &projection.sides.threads[id];
        if side.closed {
            return Err(error(410, "侧边会话已关闭，内容已释放"));
        }
        side.history(payload)
    }

    pub(super) async fn related_sides(&self, payload: &Value) -> anyhow::Result<Value> {
        let parent = required(payload, "threadId")?;
        let identity = self
            .runtime
            .read(
                "thread/read",
                json!({"threadId":parent,"includeTurns":false}),
            )
            .await?;
        crate::codex_service::normalize_thread_response(&identity)?;
        // 子智能体发现失败不阻断服务侧边会话创建和查看，也不伪装成完整空列表。
        let mut related = match self.related_subagents(payload).await {
            Ok(value) => value,
            Err(failure) => {
                json!({"ok":true,"threadId":parent,"subagents":[],"completedSubagents":[],
                "completedSubagentCount":0,"subagentsComplete":false,"subagentsLoading":false,
                "subagentsError":failure.to_string()})
            }
        };
        let projection = self.projection.lock().await;
        let mut rows = Vec::new();
        for (id, side) in &projection.sides.threads {
            if side.parent != parent || side.closed {
                continue;
            }
            let detail = projection.thread(id, side.view()?);
            rows.push(json!({"id":id,"title":detail["title"],"createdAtMs":side.metadata["createdAt"],
                "ephemeral":true,"running":detail["priority"]["active"] == true && detail["priority"]["attentionState"] != "waiting",
                "hasUnreadTurn":side.unread && detail["priority"]["active"] != true && detail["priority"]["attentionState"] != "waiting"}));
        }
        rows.sort_by_key(|row| row["createdAtMs"].as_u64().unwrap_or(0));
        related["sideThreads"] = json!(rows);
        related["canCreateSideThread"] = json!(
            projection
                .sides
                .threads
                .values()
                .filter(|side| !side.closed)
                .count()
                < MAX_OPEN_SIDES
        );
        Ok(related)
    }

    pub(super) async fn create_side(
        &self,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let parent = required(payload, "threadId")?;
        let gate = self.thread_lock("$service-side-create").await;
        let _serial = gate.lock().await;
        let connection = self.runtime.connection().await?;
        let identity = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":parent,"includeTurns":false}),
                None,
            )
            .await?;
        let main = crate::codex_service::normalize_thread_response(&identity)?;
        if self
            .projection
            .lock()
            .await
            .sides
            .threads
            .values()
            .filter(|side| !side.closed)
            .count()
            >= MAX_OPEN_SIDES
        {
            return Err(error(429, "临时侧边会话已达上限，请先关闭不再使用的会话"));
        }
        // 与 App 一样把侧边规则追加到当前项目的开发者指令，不能覆盖用户已有的配置要求。
        let config = self
            .runtime
            .read(
                "config/read",
                json!({"cwd":main["cwd"],"includeLayers":false}),
            )
            .await?;
        if !config["config"].is_object() {
            return Err(error(503, "无法确认侧边会话的开发者指令配置"));
        }
        let configured_instructions = match &config["config"]["developer_instructions"] {
            Value::Null => "",
            Value::String(instructions) => instructions.trim(),
            _ => return Err(error(503, "侧边会话的开发者指令配置格式无效")),
        };
        let instructions = if configured_instructions.is_empty() {
            SIDE_CONVERSATION_INSTRUCTIONS.to_owned()
        } else {
            format!("{configured_instructions}\n\n{SIDE_CONVERSATION_INSTRUCTIONS}")
        };
        let owner = self.owner_snapshot(parent).await?;
        let saved = self
            .store
            .lock()
            .await
            .data
            .settings
            .get(parent)
            .cloned()
            .unwrap_or(Value::Null);
        let mut params = json!({"threadId":parent,"ephemeral":true,"excludeTurns":true,"threadSource":"user",
            "developerInstructions":instructions,"config":{"thread_unload_delay_secs":0}});
        // 用户要求继承点击创建时的主会话选择，包括尚未发送的模型/深度；旧客户端再读原生及已保存设置。
        let source = owner.as_ref().unwrap_or(&main);
        let model = payload["model"]
            .as_str()
            .filter(|model| !model.is_empty())
            .or_else(|| {
                source["latestCollaborationMode"]["settings"]["model"]
                    .as_str()
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| {
                source["latestModel"]
                    .as_str()
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| saved["model"].as_str().filter(|model| !model.is_empty()))
            .or_else(|| main["model"].as_str().filter(|model| !model.is_empty()));
        if let Some(model) = model {
            params["model"] = json!(model);
        }
        let effort = payload.get("reasoningEffort").or_else(|| {
            source["latestCollaborationMode"]["settings"]
                .get("reasoning_effort")
                .or_else(|| source.get("latestReasoningEffort"))
                .or_else(|| saved.get("reasoningEffort"))
                .or_else(|| main.get("reasoningEffort"))
        });
        if let Some(effort) = effort
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            params["config"]["model_reasoning_effort"] = json!(effort);
        }
        // 主会话在网页保存的后续权限优先；无覆盖时采用桌面原生权限，服务持有的父会话则由原生 fork 继承。
        let permissions = match payload["permissionMode"].as_str() {
            Some("configured") => {
                if let Some(profile) = config["config"]["default_permissions"].as_str() {
                    params["permissions"] = json!(profile);
                    None
                } else {
                    Some(super::super::permissions::codex_config_preset(&config)?)
                }
            }
            Some(mode) => Some(super::super::permissions::codex_preset(
                mode,
                main["cwd"].as_str().unwrap_or(""),
            )?),
            None => match owner.as_ref() {
                Some(snapshot) if snapshot["currentPermissions"].is_object() => {
                    Some(snapshot["currentPermissions"].clone())
                }
                Some(_) => return Err(error(503, "无法确认主会话当前的原生权限")),
                None => None,
            },
        };
        if let Some(permissions) = permissions {
            apply_side_fork_permissions(&mut params, &permissions)?;
        }
        let fork = self
            .runtime
            .request_in_generation(&connection.generation, "thread/fork", params, authorization)
            .await?;
        let raw = &fork["thread"];
        let valid = raw["ephemeral"] == true
            && raw["path"].is_null()
            && raw["forkedFromId"] == parent
            && raw["cwd"] == main["cwd"]
            && raw["id"]
                .as_str()
                .is_some_and(|id| !id.is_empty() && id != parent)
            && crate::codex_service::subagent_parent_id(raw).is_none();
        if !valid {
            return Err(ApiError {
                code: 503,
                message: "侧边会话已创建，但身份尚未确认；请查询原回执，不要重复创建".into(),
                outcome_unknown: true,
            }
            .into());
        }
        let id = raw["id"].as_str().unwrap().to_owned();
        // 必须先确认边界已写入模型上下文，再登记可发送的侧边会话；失败不允许降级为普通 fork。
        let initialized = async {
            let permissions = side_permissions(&fork)?;
            self.runtime
                .request_in_generation(
                    &connection.generation,
                    "thread/inject_items",
                    json!({"threadId":id,"items":side_boundary_items(parent)}),
                    authorization,
                )
                .await?;
            Ok::<_, anyhow::Error>(permissions)
        }
        .await;
        let permissions = match initialized {
            Ok(permissions) => permissions,
            Err(failure) => {
                // 仅释放已验证身份的本次临时分支，不重试结果未知的注入，也不触碰父会话。
                let released = self
                    .runtime
                    .request_in_generation(
                        &connection.generation,
                        "thread/unsubscribe",
                        json!({"threadId":id}),
                        authorization,
                    )
                    .await
                    .is_ok_and(|response| {
                        matches!(
                            response["status"].as_str(),
                            Some("unsubscribed" | "notLoaded" | "notSubscribed")
                        )
                    });
                return Err(ApiError {
                code: 503,
                message: if released {
                    format!("侧边会话初始化失败，临时分支已关闭：{failure}")
                } else {
                    format!("侧边会话初始化失败，关闭结果待核对；请查询原回执，不要重复创建：{failure}")
                },
                outcome_unknown: !released,
            }
            .into());
            }
        };
        let mut metadata = raw.clone();
        metadata["name"] = json!("侧边聊天");
        metadata["createdAt"] = json!(now_ms());
        for key in ["model", "reasoningEffort", "serviceTier"] {
            metadata[key] = fork[key].clone();
        }
        if let Some(mode) = payload["permissionMode"].as_str() {
            metadata["permissionMode"] = json!(mode);
        }
        let side = SideThread {
            parent: parent.to_owned(),
            generation: connection.generation.clone(),
            metadata,
            turns: Vec::new(),
            closed: false,
            expired: false,
            closing: false,
            incomplete: false,
            unread: false,
            sent: HashMap::new(),
            queue: Data::default(),
            queue_revision: 1,
            permissions,
            bytes: 0,
        };
        let mut projection = self.projection.lock().await;
        if projection.generation != connection.generation {
            return Err(ApiError {
                code: 503,
                message: "创建侧边会话期间服务连接已变化，请查询原回执".into(),
                outcome_unknown: true,
            }
            .into());
        }
        // 只保留最近 512 个条目；过期但未关闭的正文仍计入 16 个会话上限，主动关闭才释放正文。
        if projection.sides.threads.len() >= 512 {
            if let Some(old) = projection
                .sides
                .threads
                .iter()
                .find(|(_, side)| side.closed)
                .map(|(id, _)| id.clone())
            {
                projection.sides.threads.remove(&old);
            }
        }
        let view = side.view()?;
        projection.register_side(id.clone(), side, view.clone());
        drop(projection);
        self.loaded
            .lock()
            .await
            .insert(id.clone(), connection.generation.clone());
        Ok(json!({"ok":true,"threadId":id,"parentThreadId":parent,"thread":view}))
    }

    pub(super) async fn side_command(
        &self,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let id = required(payload, "threadId")?;
        if matches!(action, "markRead" | "closeSideThread") {
            let permit = authorization.prepare_dispatch().await?;
            let mut projection = self.projection.lock().await;
            let side = projection
                .sides
                .threads
                .get(id)
                .ok_or_else(|| error(409, "此会话不是服务创建的侧边会话"))?;
            if action == "markRead" {
                if side.closed {
                    return Err(error(410, "侧边会话已关闭，内容已释放"));
                }
                // 已读只修改当前保留正文的标记，不依赖已过期的引擎连接，也不恢复临时会话。
                return permit.commit(|| {
                    let side = projection.sides.threads.get_mut(id).unwrap();
                    if side.unread {
                        side.unread = false;
                        projection.side_changed(id);
                    }
                    Ok(json!({"ok":true,"threadId":id}))
                });
            }
            let parent = required(payload, "parentThreadId")?;
            if side.parent != parent || parent == id {
                return Err(error(409, "侧边会话父子关系不匹配"));
            }
            if side.closed {
                return Ok(json!({"ok":true,"threadId":id,"parentThreadId":parent,"closed":true}));
            }
            if side.expired {
                return permit.commit(|| {
                    projection.close_side(id);
                    Ok(json!({"ok":true,"threadId":id,"parentThreadId":parent,"closed":true}))
                });
            }
        }
        let generation = self.side_generation(id).await?;
        match action {
            "send" => self.send_side(id, payload, authorization).await,
            "removeQueued" | "reorderQueued" | "beginQueuedEdit" | "editQueued"
            | "cancelQueuedEdit" | "dismissQueuedEdit" => {
                self.edit_queue(id, action, payload, authorization).await
            }
            "steerQueued" => {
                let message_id = required(payload, "messageId")?;
                let message = self.projection.lock().await.sides.threads[id]
                    .queue
                    .queues
                    .get(id)
                    .and_then(|queue| queue.iter().find(|message| message.id == message_id))
                    .cloned()
                    .ok_or_else(|| error(409, "队列消息已变化"))?;
                self.dispatch_side_queued(id, &message, authorization, true)
                    .await?;
                let mut response = self.queue(id).await;
                if let Some((_, receipt)) = self.projection.lock().await.sides.threads[id].sent.get(
                    message.payload["clientUserMessageId"]
                        .as_str()
                        .unwrap_or(""),
                ) {
                    for key in ["turnId", "nativeBridge", "executionTransport"] {
                        if let Some(value) = receipt.get(key) {
                            response[key] = value.clone();
                        }
                    }
                }
                response["confirmed"] = json!(true);
                response["deliveryState"] = json!("accepted");
                response["messageId"] = json!(message_id);
                response["clientUserMessageId"] = message.payload["clientUserMessageId"].clone();
                Ok(response)
            }
            "stop" | "closeSideThread" => {
                // 用户主动停止只结束当前任务；尚未派发的侧边消息保持暂停，不能被迟到完成事件自动续发。
                let permit = authorization.prepare_dispatch().await?;
                let mut projection = self.projection.lock().await;
                permit.commit(|| {
                    let side = projection.sides.threads.get_mut(id).unwrap();
                    side.queue.pause_queue(id, "消息已暂停");
                    side.queue_revision += 1;
                    projection.side_changed(id);
                    Ok(())
                })?;
                drop(projection);
                let active = self.projection.lock().await.active.get(id).cloned();
                if action == "closeSideThread" {
                    self.projection
                        .lock()
                        .await
                        .sides
                        .threads
                        .get_mut(id)
                        .unwrap()
                        .closing = true;
                }
                if let Some(turn) = active {
                    self.runtime
                        .request_in_generation(
                            &generation,
                            "turn/interrupt",
                            json!({"threadId":id,"turnId":turn}),
                            authorization,
                        )
                        .await?;
                }
                if action == "closeSideThread" {
                    let response = self
                        .runtime
                        .request_in_generation(
                            &generation,
                            "thread/unsubscribe",
                            json!({"threadId":id}),
                            authorization,
                        )
                        .await?;
                    if !matches!(
                        response["status"].as_str(),
                        Some("unsubscribed" | "notLoaded" | "notSubscribed")
                    ) {
                        return Err(ApiError {
                            code: 503,
                            message: "侧边会话关闭结果未确认，请查询原回执".into(),
                            outcome_unknown: true,
                        }
                        .into());
                    }
                    self.projection.lock().await.close_side(id);
                    self.loaded.lock().await.remove(id);
                    return Ok(
                        json!({"ok":true,"threadId":id,"parentThreadId":payload["parentThreadId"],"closed":true}),
                    );
                }
                Ok(json!({"ok":true,"threadId":id}))
            }
            "approve" | "answer" => {
                if payload["method"] == "remote/requestUserInputAsync" {
                    return self.answer_side_async(id, payload, authorization).await;
                }
                self.respond(id, action, payload, authorization).await
            }
            "compact" => {
                self.runtime
                    .request_in_generation(
                        &generation,
                        "thread/compact/start",
                        json!({"threadId":id}),
                        authorization,
                    )
                    .await?;
                Ok(json!({"ok":true,"threadId":id}))
            }
            _ => Err(error(409, "此操作不适用于临时侧边会话")),
        }
    }

    pub(super) async fn send_side(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        self.submit_side(id, payload, authorization, false).await
    }

    async fn submit_side(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
        steer: bool,
    ) -> anyhow::Result<Value> {
        super::commands::inputs(payload)?;
        let client = required(payload, "clientUserMessageId")?;
        let fingerprint = super::commands::payload_hash(payload);
        let generation = self.side_generation(id).await?;
        // 与 Fast/队列编辑共用“授权锁 → 投影锁”顺序，避免不同侧边会话并发操作时交叉等待。
        let permit = authorization.prepare_dispatch().await?;
        let mut projection = self.projection.lock().await;
        let active = projection.active.get(id).cloned();
        if projection.compactions.contains_key(id) || payload["queueOnly"] == true {
            return Err(error(
                409,
                "临时侧边会话压缩期间不能排队发送，请等待压缩完成",
            ));
        }
        let side = projection.sides.threads.get_mut(id).unwrap();
        if side.closed || side.expired || side.generation != generation {
            return Err(error(410, "侧边会话已过期，不能恢复或重新执行"));
        }
        if side.closing || side.incomplete {
            return Err(error(409, "此侧边会话不能继续发送，请先关闭"));
        }
        // 执行中的普通消息只排队，不携带新模型/深度；模型选择由任务结束后的新发送应用。
        if active.is_some()
            && ["model", "reasoningEffort", "serviceTier"]
                .iter()
                .any(|key| {
                    payload[*key]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                        && !side.metadata[*key].is_null()
                        && payload[*key] != side.metadata[*key]
                })
        {
            return Err(error(
                409,
                "侧边任务执行中不能切换模型或思考深度，请先停止任务",
            ));
        }
        if let Some((hash, result)) = side.sent.get(client) {
            if hash != &fingerprint {
                return Err(error(409, "消息标识已用于其他内容"));
            }
            if result["deliveryState"] == "queued" {
                let mut response = side.queue_snapshot();
                for (key, value) in result.as_object().into_iter().flatten() {
                    response[key] = value.clone();
                }
                return Ok(response);
            }
            return Ok(result.clone());
        }
        if side.sent.len() >= 4096 {
            return Err(error(429, "临时侧边会话消息已达上限，请新建侧边会话"));
        }
        if let Some(turn) = active.as_ref().filter(|_| !steer) {
            let mut message = Queued::new(payload.clone(), authorization);
            // Fast 在实际派发时读取当前会话档位；队列本身不冻结模型、思考深度或 Fast。
            for key in ["model", "reasoningEffort", "serviceTier"] {
                message.payload.as_object_mut().unwrap().remove(key);
            }
            if side
                .queue
                .queues
                .get(id)
                .is_some_and(|queue| queue.len() >= 100)
            {
                return Err(error(429, "此会话的待发送消息已达上限"));
            }
            if serde_json::to_vec(&side.queue)?.len() + serde_json::to_vec(&message)?.len()
                > MAX_QUEUE_BYTES
            {
                return Err(error(
                    429,
                    "侧边会话的待发送内容已达 16 MiB，请先处理现有队列",
                ));
            }
            let response = permit.commit(|| {
                side.queue
                    .queues
                    .entry(id.to_owned())
                    .or_default()
                    .push(message.clone());
                side.queue.wait_queue(id, Some(turn.clone()), false);
                side.queue_revision += 1;
                let receipt = json!({"ok":true,"threadId":id,"messageId":message.id,
                    "clientUserMessageId":client,"deliveryState":"queued","confirmed":true});
                let mut response = side.queue_snapshot();
                for (key, value) in receipt.as_object().unwrap() {
                    response[key] = value.clone();
                }
                // 回执只保存消息身份；队列图片和完整快照仅保留一份，避免每次入队重复缓存整条队列。
                side.sent.insert(client.to_owned(), (fingerprint, receipt));
                Ok(response)
            })?;
            projection.side_changed(id);
            return Ok(response);
        }
        drop(projection);
        drop(permit);
        self.deliver_side(id, payload, authorization, active).await
    }

    async fn deliver_side(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
        active: Option<String>,
    ) -> anyhow::Result<Value> {
        let input = super::commands::inputs(payload)?;
        let client = required(payload, "clientUserMessageId")?;
        let generation = self.side_generation(id).await?;
        let mut params = json!({"threadId":id,"input":input,"clientUserMessageId":client});
        let method = if let Some(turn) = &active {
            params["expectedTurnId"] = json!(turn);
            "turn/steer"
        } else {
            for (from, to) in [
                ("model", "model"),
                ("reasoningEffort", "effort"),
                ("serviceTier", "serviceTier"),
            ] {
                if payload[from].is_string() {
                    params[to] = payload[from].clone();
                }
            }
            "turn/start"
        };
        if method == "turn/start" {
            // 每个新轮次沿用创建时的原生权限；忽略客户端权限覆盖，父会话或全局后续变化不改写侧边快照。
            let projection = self.projection.lock().await;
            let side = &projection.sides.threads[id];
            for (key, value) in side.permissions.as_object().into_iter().flatten() {
                params[key] = value.clone();
            }
        }
        if method == "turn/start"
            && let Err(failure) = self.apply_fast_to_turn(id, &mut params).await
        {
            // Fast 准备失败发生在轮次写入之前；保留已有入队回执，直接发送尚未登记未知意图。
            return Err(failure);
        }
        let applied_tier = params["serviceTier"].clone();
        let mut projection = self.projection.lock().await;
        let side = projection.sides.threads.get_mut(id).unwrap();
        if side.closed || side.closing || side.expired || side.generation != generation {
            return Err(error(410, "侧边会话已过期，不能恢复或重新执行"));
        }
        let unknown = json!({"ok":false,"code":503,"threadId":id,"clientUserMessageId":client,
            "outcomeUnknown":true,"confirmed":false,"deliveryState":"unknown","message":"消息结果待核对，不会重复发送"});
        side.sent
            .entry(client.to_owned())
            .or_insert_with(|| (super::commands::payload_hash(payload), Value::Null))
            .1 = unknown;
        drop(projection);
        let response = self
            .runtime
            .request_in_generation(&generation, method, params, authorization)
            .await;
        let mut projection = self.projection.lock().await;
        let side = projection.sides.threads.get_mut(id).unwrap();
        let response = match response {
            Ok(response) => response,
            Err(failure) => {
                if failure
                    .downcast_ref::<ApiError>()
                    .is_some_and(|failure| !failure.outcome_unknown)
                {
                    if let Some(queued) = side
                        .queue
                        .queues
                        .get(id)
                        .into_iter()
                        .flatten()
                        .find(|queued| queued.payload["clientUserMessageId"] == client)
                    {
                        if let Some(sent) = side.sent.get_mut(client) {
                            sent.1 = json!({"ok":true,"confirmed":true,"threadId":id,
                                "messageId":queued.id,"clientUserMessageId":client,"deliveryState":"queued"});
                        }
                    } else {
                        side.sent.remove(client);
                    }
                }
                return Err(failure);
            }
        };
        let turn = response["turn"]["id"]
            .as_str()
            .or_else(|| response["turnId"].as_str())
            .or(active.as_deref())
            .ok_or_else(|| {
                anyhow::Error::from(ApiError {
                    code: 503,
                    message: "侧边消息轮次尚未确认，请查询原回执".into(),
                    outcome_unknown: true,
                })
            })?
            .to_owned();
        let result = json!({"ok":true,"confirmed":true,"threadId":id,"turnId":turn,"clientUserMessageId":client,
            "deliveryState":"accepted","nativeBridge":"harness","executionTransport":"app-server-stdio"});
        if side.closed || side.expired || side.generation != generation {
            return Ok(result);
        }
        if method == "turn/start" && !applied_tier.is_null() {
            side.metadata["serviceTier"] = applied_tier;
        }
        if let Some(sent) = side.sent.get_mut(client) {
            sent.1 = result.clone();
        }
        // 沿用侧边命名约定：首条已接受的有效文字取 15 个 Unicode 码点，后续消息不覆盖名称。
        if side.metadata["name"] == "侧边聊天" {
            if let Some(text) = payload["prompt"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
            {
                side.metadata["name"] = json!(
                    text.split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(15)
                        .collect::<String>()
                );
            }
        }
        if active.is_none() {
            for key in ["model", "reasoningEffort", "serviceTier"] {
                if payload[key].is_string() {
                    side.metadata[key] = payload[key].clone();
                }
            }
        }
        if active.is_none()
            && let Some(snapshot) = projection.threads.get_mut(id)
        {
            for key in ["model", "reasoningEffort", "serviceTier"] {
                if payload[key].is_string() {
                    snapshot[key] = payload[key].clone();
                }
            }
        }
        projection.started(id, &turn);
        projection.side_changed(id);
        Ok(result)
    }

    pub(super) async fn dispatch_side_queued(
        &self,
        id: &str,
        message: &Queued,
        authorization: &RemoteAuthorization,
        manual: bool,
    ) -> anyhow::Result<()> {
        let generation = self.side_generation(id).await?;
        let permit = authorization.prepare_dispatch().await?;
        let mut projection = self.projection.lock().await;
        let active = projection.active.get(id).cloned();
        if projection.compactions.contains_key(id) {
            return if manual {
                Err(error(409, "侧边会话压缩期间不能引导队列消息"))
            } else {
                Ok(())
            };
        }
        if !manual && active.is_some() {
            return Ok(());
        }
        let side = projection.sides.threads.get_mut(id).unwrap();
        if side.closed
            || side.expired
            || side.closing
            || side.incomplete
            || side.generation != generation
        {
            return Err(error(410, "此侧边会话不能继续发送"));
        }
        if !manual
            && !side
                .queue
                .queue_gates
                .get(id)
                .is_some_and(|gate| gate.phase == "ready")
        {
            return Ok(());
        }
        let queued = side
            .queue
            .queues
            .get_mut(id)
            .and_then(|queue| queue.iter_mut().find(|queued| queued.id == message.id))
            .ok_or_else(|| error(409, "队列消息已变化"))?;
        if !["queued", "paused"].contains(&queued.state.as_str())
            || !manual && queued.state != "queued"
        {
            return Err(error(409, "消息正在发送或待核对"));
        }
        permit.commit(|| {
            queued.state = "sending".into();
            side.queue_revision += 1;
            Ok(())
        })?;
        projection.side_changed(id);
        drop(projection);
        let result = self
            .deliver_side(id, &message.payload, authorization, active)
            .await;
        let mut projection = self.projection.lock().await;
        if let Some(side) = projection
            .sides
            .threads
            .get_mut(id)
            .filter(|side| !side.closed && !side.expired && side.generation == generation)
        {
            if result.is_ok() {
                if let Some(queue) = side.queue.queues.get_mut(id) {
                    queue.retain(|queued| queued.id != message.id);
                }
            } else {
                let state = if result
                    .as_ref()
                    .unwrap_err()
                    .downcast_ref::<ApiError>()
                    .is_some_and(|failure| !failure.outcome_unknown)
                {
                    "paused"
                } else {
                    "unknown"
                };
                if let Some(queued) = side
                    .queue
                    .queues
                    .get_mut(id)
                    .and_then(|queue| queue.iter_mut().find(|queued| queued.id == message.id))
                {
                    queued.state = state.into();
                }
                side.queue
                    .pause_queue(id, "侧边消息未确认送达，待发送消息已暂停");
            }
            side.queue_revision += 1;
            projection.side_changed(id);
        }
        result.map(|_| ())
    }

    async fn answer_side_async(
        &self,
        id: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        let request_id = required(payload, "requestId")?;
        let request = self
            .projection
            .lock()
            .await
            .requests
            .values()
            .find(|request| {
                request["params"]["threadId"] == id
                    && request["id"] == request_id
                    && request["method"] == "remote/requestUserInputAsync"
            })
            .cloned()
            .ok_or_else(|| error(409, "此问题已结束或已回答"))?;
        let message = super::commands::async_reply_message(&request, payload)?;
        if let Some(mut message) = message {
            // 同一问题的回执即使迟到，也复用同一客户端消息 ID，不能因重复回答再次引导模型。
            message["clientUserMessageId"] = json!(format!("side-question-{request_id}"));
            // 提问答案需要补充到等待中的任务；仅普通发送进入 FIFO，避免答案排队造成当前任务一直等待。
            let result = self.submit_side(id, &message, authorization, true).await?;
            if result["ok"] != true || result["confirmed"] != true {
                return Ok(result);
            }
        }
        let permit = authorization.prepare_dispatch().await?;
        let mut projection = self.projection.lock().await;
        let key = projection
            .requests
            .iter()
            .find(|(_, request)| request["params"]["threadId"] == id && request["id"] == request_id)
            .map(|(key, _)| key.clone());
        permit.commit(|| {
            if let Some(key) = key {
                projection.resolve(&key);
            }
            Ok(())
        })?;
        Ok(json!({"ok":true,"threadId":id}))
    }
}
