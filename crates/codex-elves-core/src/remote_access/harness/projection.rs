use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{Value, json};

use super::super::now_ms;

const MAX_EVENTS: usize = 2048;
const MAX_NOTIFICATIONS: usize = 256;

struct CompactionRecord {
    thread_id: String,
    turn_id: String,
    item: Value,
    sequence: u64,
}

struct DesktopState {
    connected: bool,
    requests_known: bool,
    requests: Vec<Value>,
    active_turn: Option<String>,
}

pub(super) struct Projection {
    pub epoch: String,
    pub sequence: u64,
    pub generation: String,
    generation_index: u64,
    hidden_threads: HashSet<String>,
    pub threads: HashMap<String, Value>,
    pub subagents: super::subagents::SubagentState,
    pub sides: super::sides::SideState,
    pub requests: HashMap<String, Value>,
    pub active: HashMap<String, String>,
    desktop: HashMap<String, DesktopState>,
    pub compactions: HashMap<String, Value>,
    compaction_records: VecDeque<CompactionRecord>,
    completed: VecDeque<String>,
    events: VecDeque<(u64, Value)>,
    notifications: VecDeque<Value>,
    pending_notifications: HashMap<String, Value>,
    notification_sequence: u64,
}

impl Default for Projection {
    fn default() -> Self {
        Self {
            epoch: format!("harness-{}", uuid::Uuid::new_v4()),
            sequence: 0,
            generation: String::new(),
            generation_index: 0,
            hidden_threads: HashSet::new(),
            threads: HashMap::new(),
            subagents: super::subagents::SubagentState::default(),
            sides: super::sides::SideState::default(),
            requests: HashMap::new(),
            active: HashMap::new(),
            desktop: HashMap::new(),
            compactions: HashMap::new(),
            compaction_records: VecDeque::new(),
            completed: VecDeque::new(),
            events: VecDeque::new(),
            notifications: VecDeque::new(),
            pending_notifications: HashMap::new(),
            notification_sequence: 0,
        }
    }
}

impl Projection {
    pub fn observe_desktop(&mut self, id: &str, snapshot: &Value, events: &Value) {
        let Ok(thread) =
            crate::codex_service::normalize_owner_thread(snapshot, self.threads.get(id))
        else {
            return;
        };
        let reconnected = self.desktop.get(id).is_none_or(|state| !state.connected);
        let runtime_active = matches!(
            thread["status"]
                .as_str()
                .or_else(|| thread["status"]["type"].as_str()),
            Some("active" | "inProgress" | "running" | "waiting")
        );
        let active_turn = crate::desktop_owner_ipc::snapshot_turns(snapshot)
            .into_iter()
            .filter(|turn| runtime_active && turn["status"] == "inProgress")
            .max_by_key(|turn| turn["turnStartedAtMs"].as_u64().unwrap_or(0))
            .and_then(|turn| turn["turnId"].as_str().map(str::to_owned));
        let requests_known = snapshot["requests"].is_array();
        let requests = snapshot["requests"].as_array().cloned().unwrap_or_else(|| {
            self.desktop
                .get(id)
                .map(|state| state.requests.clone())
                .unwrap_or_default()
        });
        self.desktop.insert(
            id.to_owned(),
            DesktopState {
                connected: true,
                requests_known,
                requests,
                active_turn: active_turn.clone(),
            },
        );
        self.threads.insert(id.to_owned(), thread.clone());
        let mut turn_changed = false;
        for event in events.as_array().into_iter().flatten() {
            if event["method"] == "remote/notification" {
                continue;
            }
            let mut event = event.clone();
            if event["method"] == "remote/requestsChanged" {
                // 桌面审批使用同一网页状态版本；不能混用临时快照转换器的 epoch。
                event["params"]["requestsEpoch"] = json!(&self.epoch);
                event["params"]["requestsRevision"] = json!(self.sequence + 1);
            }
            turn_changed |= matches!(
                event["method"].as_str(),
                Some("turn/started" | "turn/completed")
            );
            self.ingest(event);
        }
        match active_turn {
            Some(turn) => {
                self.active.insert(id.to_owned(), turn);
            }
            None => {
                self.active.remove(id);
            }
        }
        self.threads.insert(id.to_owned(), thread.clone());
        if reconnected || turn_changed {
            // 同一快照可能同时结束旧轮次并开始新轮次，最后必须恢复桌面的实际运行状态。
            self.push(json!({"method":"thread/status/changed","params":{"threadId":id,"status":thread["status"]}}));
        }
        if reconnected {
            self.push(json!({"method":"remote/resync","params":{"threadId":id}}));
        }
    }

    pub fn desktop_disconnected(&mut self, id: &str) {
        let Some(state) = self.desktop.get_mut(id).filter(|state| state.connected) else {
            return;
        };
        state.connected = false;
        state.active_turn = None;
        self.active.remove(id);
        if let Some(thread) = self.threads.get_mut(id) {
            // 丢失桌面连接只能标为未知，不能把仍可能执行的会话宣告为空闲或已停止。
            thread["status"] = json!({"type":"unknown"});
        }
        self.push(json!({"method":"thread/status/changed","params":{"threadId":id,"status":{"type":"unknown"}}}));
        self.push(json!({"method":"remote/resync","params":{"threadId":id}}));
    }

    pub fn forget_desktop(&mut self, id: &str) {
        if self.desktop.remove(id).is_some() {
            self.active.remove(id);
        }
    }

    pub fn register_side(&mut self, id: String, side: super::sides::SideThread, view: Value) {
        self.hidden_threads.remove(&id);
        self.sides.threads.insert(id.clone(), side);
        self.threads.insert(id.clone(), view);
        self.side_changed(&id);
    }

    pub fn side_changed(&mut self, id: &str) {
        if let Some(side) = self.sides.threads.get(id) {
            self.push(json!({"method":"remote/relatedThreadsChanged","params":{
                "threadId":side.parent,"closedThreadIds":if side.closed {json!([id])} else {json!([])}}}));
        }
    }

    pub fn close_side(&mut self, id: &str) {
        if let Some(side) = self.sides.threads.get_mut(id) {
            side.closed = true;
            side.closing = false;
            side.turns.clear();
            side.sent.clear();
        }
        self.clear_side_runtime(id);
        self.side_changed(id);
    }

    pub fn expire_side(&mut self, id: &str) {
        if let Some(side) = self.sides.threads.get_mut(id) {
            side.expire();
        }
        self.clear_side_runtime(id);
        self.side_changed(id);
    }

    fn clear_side_runtime(&mut self, id: &str) {
        self.active.remove(id);
        self.threads.remove(id);
        self.forget_compactions(id);
        let requests: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, request)| request["params"]["threadId"] == id)
            .map(|(key, _)| key.clone())
            .collect();
        for key in requests {
            self.resolve(&key);
        }
    }

    pub fn push(&mut self, event: Value) {
        if event["method"] == "thread/deleted" {
            self.forget_compactions(event["params"]["threadId"].as_str().unwrap_or(""));
        }
        self.sequence += 1;
        self.events.push_back((self.sequence, event));
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    pub fn events(&self, payload: &Value) -> Value {
        let cursor = payload["cursor"].as_u64().unwrap_or(0);
        let reset = payload["epoch"] != self.epoch
            || cursor > self.sequence
            || self
                .events
                .front()
                .is_some_and(|(sequence, _)| cursor.saturating_add(1) < *sequence);
        let events: Vec<_> = self
            .events
            .iter()
            .filter(|(sequence, _)| !reset && *sequence > cursor)
            .map(|(_, value)| value.clone())
            .collect();
        json!({"ok":true,"epoch":self.epoch,"cursor":self.sequence,"reset":reset,
            "nativeBridge":"harness","writeAvailable":true,"events":events})
    }

    pub fn requests_for(&self, thread_id: &str) -> Vec<Value> {
        if let Some(desktop) = self.desktop.get(thread_id) {
            return desktop.requests.clone();
        }
        self.requests
            .values()
            .filter(|r| r["params"]["threadId"] == thread_id)
            .cloned()
            .map(|mut request| {
                request.as_object_mut().unwrap().remove("_generation");
                request.as_object_mut().unwrap().remove("_generationIndex");
                request
            })
            .collect()
    }

    pub fn thread(&self, thread_id: &str, mut thread: Value) -> Value {
        // 过期侧边保留最终内容，但不能沿用旧快照的运行、审批或压缩状态。
        if thread["sideThreadExpired"] == true {
            thread["status"] = json!({"type":"idle"});
            thread["requests"] = json!([]);
            thread["requestsKnown"] = json!(true);
            thread["requestsEpoch"] = json!(&self.epoch);
            thread["requestsRevision"] = json!(self.sequence);
            thread["priority"] = json!({"status":thread["status"],"active":false,"eligible":true,
                "attentionState":if thread["hasUnreadTurn"] == true {"unread"} else {"idle"}});
            return thread;
        }
        if let Some(snapshot) = self.threads.get(thread_id) {
            for field in [
                "status",
                "model",
                "reasoningEffort",
                "goal",
                "tokenUsage",
                "readState",
            ] {
                if let Some(value) = snapshot.get(field) {
                    thread[field] = value.clone();
                }
            }
        }
        if let Some(unread) = self
            .threads
            .get(thread_id)
            .and_then(|snapshot| snapshot["hasUnreadTurn"].as_bool())
            .filter(|_| thread["serviceSide"] != true)
        {
            thread["hasUnreadTurn"] = json!(unread);
        }
        thread["requests"] = json!(self.requests_for(thread_id));
        thread["requestsKnown"] = json!(
            self.desktop
                .get(thread_id)
                .is_none_or(|state| state.connected && state.requests_known)
        );
        thread["requestsEpoch"] = json!(&self.epoch);
        thread["requestsRevision"] = json!(self.sequence);
        // 目录复用旧摘要时，活动/等待状态仍以本轮事件为准，不能沿用缓存中的 idle 优先级。
        let status = thread["status"]
            .as_str()
            .or_else(|| thread["status"]["type"].as_str());
        let active = matches!(status, Some("active" | "inProgress" | "running"))
            || self.active.contains_key(thread_id);
        let waiting = thread["requests"]
            .as_array()
            .is_some_and(|requests| !requests.is_empty())
            || status == Some("waiting")
            || thread["status"]["activeFlags"]
                .as_array()
                .is_some_and(|flags| {
                    flags
                        .iter()
                        .any(|flag| flag == "waitingOnApproval" || flag == "waitingOnUserInput")
                });
        thread["priority"] = json!({
            "status":thread["status"],"active":active,"eligible":true,
            "attentionState":if waiting {"waiting"} else if active {"active"}
                else if thread["hasUnreadTurn"] == true {"unread"} else {"idle"}
        });
        self.decorate_compaction(thread_id, &mut thread);
        thread
    }

    pub fn compaction_revision(&self, id: &str) -> u64 {
        self.compaction_records
            .iter()
            .rev()
            .find(|record| record.thread_id == id)
            .map_or(0, |record| record.sequence)
    }

    pub fn forget_compactions(&mut self, id: &str) {
        self.compactions.remove(id);
        self.compaction_records
            .retain(|record| record.thread_id != id);
    }

    pub fn compaction(&self, id: &str) -> Value {
        self.compactions.get(id).cloned().unwrap_or_else(|| {
            self.compaction_records
                .iter()
                .rev()
                .find(|record| record.thread_id == id)
                .map_or_else(
                    || json!({"state":"idle"}),
                    |record| json!({"state":"idle","turnId":record.turn_id,"itemId":record.item["id"]}),
                )
        })
    }

    pub fn decorate_compaction(&self, id: &str, thread: &mut Value) {
        if self.compactions.contains_key(id) || self.compaction_revision(id) > 0 {
            thread["compaction"] = self.compaction(id);
        } else if thread["compaction"].is_null() {
            thread["compaction"] = json!({"state":"idle"});
        }
        let events = thread["transcriptEvents"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut events = events;
        for record in self
            .compaction_records
            .iter()
            .filter(|record| record.thread_id == id)
        {
            events.retain(|entry| {
                entry["turnId"] != record.turn_id || entry["item"]["id"] != record.item["id"]
            });
            events.push(json!({"turnId":record.turn_id,"item":record.item}));
        }
        thread["transcriptEvents"] = json!(events);
    }

    fn remember_compaction(&mut self, id: &str, turn_id: &str, mut item: Value, completed: bool) {
        if id.is_empty() || turn_id.is_empty() || item["id"].as_str().is_none_or(str::is_empty) {
            return;
        }
        if let Some(index) = self.compaction_records.iter().position(|record| {
            record.thread_id == id && record.turn_id == turn_id && record.item["id"] == item["id"]
        }) {
            let previous = self.compaction_records.remove(index).unwrap();
            let mut merged = previous.item.clone();
            for (key, value) in item.as_object().into_iter().flatten() {
                merged[key] = value.clone();
            }
            if previous.item["completed"] == true && !completed {
                item = previous.item;
            } else {
                item = merged;
            }
        }
        item["completed"] = json!(completed || item["completed"] == true);
        if item["startedAtMs"].as_u64().is_none() {
            item["startedAtMs"] = json!(now_ms());
        }
        self.compaction_records.push_back(CompactionRecord {
            thread_id: id.to_owned(),
            turn_id: turn_id.to_owned(),
            item,
            sequence: self.sequence + 1,
        });
        // 与实时事件队列同上限；只淘汰已结束记录，进行中的压缩不能因其它会话活动而丢失。
        while self.compaction_records.len() > MAX_EVENTS {
            let Some(index) = self
                .compaction_records
                .iter()
                .position(|record| record.item["completed"] == true)
            else {
                break;
            };
            self.compaction_records.remove(index);
        }
    }

    pub fn history(&self, id: &str, mut history: Value, latest: bool) -> Value {
        let mut events = history["transcriptEvents"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let item_turn = history["turnId"].as_str().map(str::to_owned);
        let mut turns = history["data"].as_array().cloned().unwrap_or_default();
        let oldest = turns.iter().filter_map(turn_started_ms).min().unwrap_or(0);
        for record in self
            .compaction_records
            .iter()
            .filter(|record| record.thread_id == id)
        {
            let included = if let Some(turn_id) = &item_turn {
                *turn_id == record.turn_id
            } else {
                let exists = turns.iter().any(|turn| turn["id"] == record.turn_id);
                let started = record.item["startedAtMs"].as_u64().unwrap_or(0);
                // 用户要求切回会话立即显示压缩；历史尚未落盘该轮时，以已收到的真实生命周期补齐。
                if !exists && latest && (record.item["completed"] != true || started >= oldest) {
                    let status = if record.item["completed"] != true {
                        "inProgress"
                    } else {
                        record.item["status"]
                            .as_str()
                            .filter(|status| {
                                matches!(
                                    *status,
                                    "failed" | "interrupted" | "cancelled" | "canceled"
                                )
                            })
                            .unwrap_or("completed")
                    };
                    let index = turns
                        .iter()
                        .position(|turn| turn_started_ms(turn).unwrap_or(0) < started)
                        .unwrap_or(turns.len());
                    turns.insert(
                        index,
                        json!({"id":record.turn_id,"status":status,
                        "startedAt":started / 1000,"turnStartedAtMs":started,"items":[],
                        "itemsView":"summary","itemsPageComplete":false,"itemsComplete":false}),
                    );
                    history["itemsPaginationByTurnId"][&record.turn_id] = json!({
                        "nextCursor":null,"hasMore":false,"complete":false,"pageComplete":false,
                        "direction":"older","order":"asc"});
                }
                exists || turns.iter().any(|turn| turn["id"] == record.turn_id)
            };
            if included {
                events.retain(|entry| {
                    entry["turnId"] != record.turn_id || entry["item"]["id"] != record.item["id"]
                });
                events.push(json!({"turnId":record.turn_id,"item":record.item}));
            }
        }
        if item_turn.is_none() {
            history["data"] = json!(turns);
        }
        history["transcriptEvents"] = json!(events);
        history
    }

    pub fn ingest(&mut self, mut frame: Value) {
        let generation = frame["_generation"].as_str().unwrap_or("").to_owned();
        if frame["method"] == "harness/connected" {
            let index = frame["_generationIndex"].as_u64().unwrap_or(0);
            if index <= self.generation_index {
                return;
            }
            self.generation_index = index;
            self.generation = generation.clone();
            self.epoch = format!("harness-{}", uuid::Uuid::new_v4());
            self.events.clear();
            self.sequence = 0;
            self.requests.clear();
            self.pending_notifications.clear();
            self.notifications.clear();
            self.notification_sequence = 0;
            self.active.clear();
            // app-server 重连不等于桌面 IPC 断开，桌面的执行与审批状态独立保留。
            self.active
                .extend(self.desktop.iter().filter_map(|(id, state)| {
                    state
                        .active_turn
                        .as_ref()
                        .filter(|_| state.connected)
                        .map(|turn| (id.clone(), turn.clone()))
                }));
            self.compactions.clear();
            self.compaction_records.clear();
            self.completed.clear();
            self.hidden_threads.clear();
            self.subagents = super::subagents::SubagentState::default();
            self.sides.expire();
            self.push(json!({"method":"remote/resync"}));
            return;
        }
        // 进程代次只由连接建立事件更新；旧连接迟到的通知和断线不能覆盖新状态。
        if !generation.is_empty() && generation != self.generation {
            return;
        }
        let method = frame["method"].as_str().unwrap_or("").to_owned();
        let id = frame["params"]["threadId"]
            .as_str()
            .or_else(|| frame["params"]["thread"]["id"].as_str())
            .unwrap_or("")
            .to_owned();
        let side = self.sides.threads.contains_key(&id);
        if let Some((_, closed)) = self.sides.observe(&frame) {
            if closed {
                self.close_side(&id);
            } else if self.sides.threads[&id].expired {
                self.expire_side(&id);
            } else {
                self.side_changed(&id);
            }
        }
        if side && (self.sides.threads[&id].closed || self.sides.threads[&id].expired) {
            return;
        }
        let (subagent, parents) = self.subagents.observe(&frame);
        for parent in parents {
            self.push(
                json!({"method":"remote/relatedThreadsChanged","params":{"threadId":parent}}),
            );
        }
        if subagent {
            // 子智能体事件只用于只读展示，不加入主会话队列、审批或通知集合。
            if frame.get("id").is_none() {
                self.push(frame);
            }
            return;
        }
        if self.hidden_threads.contains(&id) {
            return;
        }
        if method == "thread/section/changed" {
            // 原生置顶或取消置顶后沿用网页现有侧栏刷新事件，不要求客户端借用 CDP 同步。
            self.push(json!({"method":"remote/sidebarChanged","params":{"reason":"section"}}));
            return;
        }
        if frame.get("id").is_some() {
            // 请求与普通通知分开保存；审批必须回复原连接和原请求 ID。
            let key = request_key(&generation, &frame["id"]);
            self.requests.insert(key.clone(), frame.clone());
            self.notify(
                &id,
                if method.contains("Approval") {
                    "approval"
                } else {
                    "question"
                },
                Some(&key),
            );
            self.push(
                json!({"method":"remote/requestsChanged","params":{"threadId":id,
                "requests":self.requests_for(&id),"requestsEpoch":self.epoch,
                "requestsRevision":self.sequence+1}}),
            );
            return;
        }
        match method.as_str() {
            "thread/started" => {
                if side {
                    self.push(frame);
                    return;
                }
                if let Ok(mut thread) =
                    crate::codex_service::normalize_thread(&frame["params"]["thread"])
                {
                    if let Some(previous) = self.threads.get(&id) {
                        for key in ["model", "reasoningEffort", "serviceTier"] {
                            if thread[key].is_null() {
                                thread[key] = previous[key].clone();
                            }
                        }
                    }
                    self.threads.insert(id.clone(), thread);
                } else {
                    self.hidden_threads.insert(id.clone());
                    return;
                }
            }
            "thread/status/changed" => {
                self.threads.entry(id.clone()).or_insert_with(|| json!({}))["status"] =
                    frame["params"]["status"].clone();
            }
            "thread/settings/updated" => {
                let thread = self.threads.entry(id.clone()).or_insert_with(|| json!({}));
                let settings = frame["params"]
                    .get("threadSettings")
                    .unwrap_or(&frame["params"]);
                for (source, target) in [
                    ("model", "model"),
                    ("reasoningEffort", "reasoningEffort"),
                    ("effort", "reasoningEffort"),
                ] {
                    if let Some(value) = settings.get(source) {
                        thread[target] = value.clone();
                    }
                }
            }
            "thread/readState/updated" => {
                let thread = self.threads.entry(id.clone()).or_insert_with(|| json!({}));
                thread["readState"] = frame["params"]["readState"].clone();
                thread["hasUnreadTurn"] =
                    json!(!frame["params"]["readState"]["firstUnread"].is_null());
            }
            "turn/started" => {
                if let Some(turn_id) = frame["params"]["turn"]["id"].as_str() {
                    self.started(&id, turn_id);
                }
            }
            "turn/completed" => {
                if let Some(turn_id) = frame["params"]["turn"]["id"].as_str() {
                    let key = format!("{id}:{turn_id}");
                    if self.completed.contains(&key) {
                        return;
                    }
                    self.completed.push_back(key);
                    while self.completed.len() > 2048 {
                        self.completed.pop_front();
                    }
                }
                if self
                    .active
                    .get(&id)
                    .is_some_and(|turn| frame["params"]["turn"]["id"] == *turn)
                {
                    self.active.remove(&id);
                }
                if self
                    .compactions
                    .get(&id)
                    .is_some_and(|state| state["turnId"] == frame["params"]["turn"]["id"])
                {
                    self.compactions.remove(&id);
                }
                let turn_id = frame["params"]["turn"]["id"].as_str().unwrap_or("");
                for record in self.compaction_records.iter_mut().filter(|record| {
                    record.thread_id == id
                        && record.turn_id == turn_id
                        && record.item["completed"] != true
                }) {
                    record.item["completed"] = json!(true);
                    record.item["status"] = frame["params"]["turn"]["status"].clone();
                    record.sequence = self.sequence + 1;
                }
                self.notify(
                    &id,
                    if frame["params"]["turn"]["status"] == "failed" {
                        "failed"
                    } else {
                        "completed"
                    },
                    None,
                );
            }
            "item/started" if frame["params"]["item"]["type"] == "contextCompaction" => {
                let turn_id = frame["params"]["turnId"].as_str().unwrap_or("");
                if self.compaction_records.iter().any(|record| {
                    record.thread_id == id
                        && record.turn_id == turn_id
                        && record.item["id"] == frame["params"]["item"]["id"]
                        && record.item["completed"] == true
                }) {
                    return;
                }
                self.remember_compaction(&id, turn_id, frame["params"]["item"].clone(), false);
                self.compactions.insert(
                    id.clone(),
                    json!({"state":"running",
                    "turnId":frame["params"]["turnId"],"itemId":frame["params"]["item"]["id"]}),
                );
            }
            "item/completed" if frame["params"]["item"]["type"] == "contextCompaction" => {
                let turn_id = frame["params"]["turnId"].as_str().unwrap_or("");
                self.remember_compaction(&id, turn_id, frame["params"]["item"].clone(), true);
                if self.compactions.get(&id).is_some_and(|state| {
                    state["turnId"].is_null() || state["turnId"] == frame["params"]["turnId"]
                }) {
                    self.compactions.remove(&id);
                }
            }
            "item/completed" if frame["params"]["item"]["type"] == "agentMessage" => {
                if let Some(mut request) =
                    async_question(&id, &frame["params"]["turnId"], &frame["params"]["item"])
                {
                    request["_generation"] = json!(&generation);
                    self.ingest(request);
                }
            }
            "thread/goal/updated" | "thread/goal/cleared" => {
                self.threads.entry(id.clone()).or_insert_with(|| json!({}))["goal"] =
                    frame["params"]["goal"].clone();
            }
            "thread/tokenUsage/updated" => {
                self.threads.entry(id.clone()).or_insert_with(|| json!({}))["tokenUsage"] =
                    frame["params"]["tokenUsage"].clone();
            }
            "serverRequest/resolved" => {
                let key = request_key(&generation, &frame["params"]["requestId"]);
                self.resolve(&key);
            }
            "harness/disconnected" => {
                let sides: Vec<_> = self.sides.threads.keys().cloned().collect();
                for id in sides {
                    self.expire_side(&id);
                }
                self.requests.clear();
                self.pending_notifications.clear();
                for (id, thread) in &mut self.threads {
                    if !self.desktop.contains_key(id) {
                        thread["status"] = json!({"type":"notLoaded"});
                    }
                }
                self.push(json!({"method":"remote/resync"}));
                return;
            }
            _ => {}
        }
        frame.as_object_mut().unwrap().remove("_generation");
        frame.as_object_mut().unwrap().remove("_generationIndex");
        self.push(frame);
    }

    pub fn started(&mut self, thread_id: &str, turn_id: &str) {
        if !self.completed.contains(&format!("{thread_id}:{turn_id}")) {
            self.active.insert(thread_id.to_owned(), turn_id.to_owned());
        }
    }

    pub fn resolve(&mut self, key: &str) {
        let request = self.requests.remove(key);
        if let Some(previous) = self.pending_notifications.remove(key) {
            self.notification_sequence += 1;
            let mut event = previous;
            event["id"] = json!(format!("{}:{}", self.epoch, self.notification_sequence));
            event["sequence"] = json!(self.notification_sequence);
            event["action"] = json!("dismiss");
            event["createdAtMs"] = json!(now_ms());
            self.add_notification(event);
        }
        if let Some(request) = request {
            let thread_id = request["params"]["threadId"].as_str().unwrap_or("");
            self.push(
                json!({"method":"remote/requestsChanged","params":{"threadId":thread_id,
                "requests":self.requests_for(thread_id),"requestsEpoch":self.epoch,
                "requestsRevision":self.sequence+1}}),
            );
        }
    }

    fn notify(&mut self, thread_id: &str, kind: &str, pending_key: Option<&str>) {
        if thread_id.is_empty() {
            return;
        }
        self.notification_sequence += 1;
        let id = format!("{}:{}", self.epoch, self.notification_sequence);
        let title: String = self
            .threads
            .get(thread_id)
            .and_then(|t| t["title"].as_str())
            .unwrap_or("会话")
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect();
        let event = json!({"id":id,"epoch":self.epoch,"sequence":self.notification_sequence,
            "threadId":thread_id,"kind":kind,"title":if title.is_empty() {"会话"} else {&title},
            "createdAtMs":now_ms(),"action":"notify","notificationId":id});
        if let Some(key) = pending_key {
            self.pending_notifications
                .insert(key.to_owned(), event.clone());
        }
        self.add_notification(event);
    }

    fn add_notification(&mut self, event: Value) {
        self.notifications.push_back(event.clone());
        while self.notifications.len() > MAX_NOTIFICATIONS {
            self.notifications.pop_front();
        }
        self.push(json!({"method":"remote/notification","params":event}));
    }

    pub fn notifications(&self, payload: &Value) -> Value {
        let cursor = payload["cursor"].as_u64().unwrap_or(0);
        let reset = payload["epoch"] != self.epoch
            || cursor > self.notification_sequence
            || self
                .notifications
                .front()
                .is_some_and(|e| cursor.saturating_add(1) < e["sequence"].as_u64().unwrap_or(0));
        let events: Vec<_> = self
            .notifications
            .iter()
            .filter(|e| !reset && e["sequence"].as_u64().unwrap_or(0) > cursor)
            .cloned()
            .collect();
        let pending: Vec<_> = self.pending_notifications.values().cloned().collect();
        json!({"ok":true,"schemaVersion":1,"notificationEpoch":self.epoch,
            "notificationCursor":self.notification_sequence,"reset":reset,"truncated":false,
            "pending":pending,"events":events})
    }
}

pub(super) fn request_key(generation: &str, id: &Value) -> String {
    format!("{generation}:{id}")
}

pub(super) fn async_question(thread_id: &str, turn_id: &Value, item: &Value) -> Option<Value> {
    let item_id = item["id"].as_str()?;
    let turn_id = turn_id.as_str()?;
    let questions:Vec<_>=item["questions"].as_array()?.iter().enumerate().filter_map(|(index,q)|{
        Some(json!({"id":serde_json::to_string(&json!(["request_user_input_async",item_id,index])).ok()?,
            "question":q["title"].as_str()?,"options":q["options"].as_array().into_iter().flatten()
                .filter_map(|o|o.as_str().map(|label|json!({"label":label}))).collect::<Vec<_>>()}))
    }).collect();
    if questions.is_empty() {
        return None;
    }
    Some(
        json!({"id":serde_json::to_string(&json!(["async-question",turn_id,item_id])).ok()?,
        "method":"remote/requestUserInputAsync","params":{"threadId":thread_id,"turnId":turn_id,"questions":questions}}),
    )
}

fn turn_started_ms(turn: &Value) -> Option<u64> {
    turn["turnStartedAtMs"].as_u64().or_else(|| {
        turn["startedAt"].as_u64().map(|value| {
            if value < 100_000_000_000 {
                value.saturating_mul(1000)
            } else {
                value
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_edges_incomplete_approvals_do_not_claim_known_empty() {
        let mut projection = Projection::default();
        let mut snapshot = json!({"id":"desktop","cwd":"C:\\repo","source":"vscode",
            "threadSource":"user","ephemeral":false,"forkedFromId":null,
            "threadRuntimeStatus":{"type":"active"},"turns":[],
            "requests":[{"id":17,"method":"item/commandExecution/requestApproval","params":{"threadId":"desktop"}}]});
        projection.observe_desktop("desktop", &snapshot, &json!([]));
        snapshot.as_object_mut().unwrap().remove("requests");
        projection.observe_desktop("desktop", &snapshot, &json!([]));
        let thread = projection.thread("desktop", json!({"id":"desktop"}));
        assert_eq!(
            thread["requestsKnown"], false,
            "字段缺失不能当作审批已全部处理"
        );
        assert_eq!(
            thread["requests"][0]["id"], 17,
            "未取得完整快照前保留上次审批"
        );
        snapshot["requests"] = json!([]);
        projection.observe_desktop("desktop", &snapshot, &json!([]));
        let thread = projection.thread("desktop", json!({"id":"desktop"}));
        assert_eq!(thread["requestsKnown"], true);
        assert_eq!(thread["requests"], json!([]));
    }

    #[test]
    fn desktop_projection_keeps_approvals_and_running_state_across_engine_reconnect() {
        let mut projection = Projection::default();
        let snapshot = json!({"id":"desktop","cwd":"C:\\repo","source":"vscode",
            "threadSource":"user","ephemeral":false,"forkedFromId":null,
            "threadRuntimeStatus":{"type":"active","activeFlags":["waitingOnApproval"]},
            "requests":[{"id":17,"method":"item/commandExecution/requestApproval","params":{"threadId":"desktop"}}],
            "turns":[{"turnId":"turn","status":"inProgress","turnStartedAtMs":1,"items":[]}]});
        projection.observe_desktop("desktop", &snapshot, &json!([]));
        let thread = projection.thread("desktop", json!({"id":"desktop"}));
        assert_eq!(thread["requests"][0]["id"], 17);
        assert_eq!(thread["requestsKnown"], true);
        assert_eq!(thread["priority"]["attentionState"], "waiting");
        projection.ingest(
            json!({"method":"harness/connected","_generation":"engine-2","_generationIndex":2}),
        );
        projection.ingest(json!({"method":"harness/disconnected","_generation":"engine-2"}));
        let thread = projection.thread("desktop", json!({"id":"desktop"}));
        assert_eq!(thread["status"]["type"], "active");
        assert_eq!(projection.active["desktop"], "turn");
        assert_eq!(thread["requests"][0]["id"], 17);
        projection.desktop_disconnected("desktop");
        let thread = projection.thread("desktop", json!({"id":"desktop"}));
        assert_eq!(thread["status"]["type"], "unknown");
        assert_eq!(thread["requestsKnown"], false);
        assert!(!projection.active.contains_key("desktop"));
        projection.observe_desktop("desktop", &snapshot, &json!([]));
        assert_eq!(
            projection.thread("desktop", json!({}))["requestsKnown"],
            true
        );
        projection.forget_desktop("desktop");
        assert!(projection.requests_for("desktop").is_empty());
    }

    #[test]
    fn desktop_projection_restores_authoritative_status_after_a_turn_rollover() {
        let mut projection = Projection::default();
        let snapshot = json!({"id":"desktop","cwd":"C:\\repo","source":"vscode",
            "threadSource":"user","ephemeral":false,"forkedFromId":null,
            "threadRuntimeStatus":{"type":"active"},"requests":[],
            "turns":[{"turnId":"new","status":"inProgress","turnStartedAtMs":2,"items":[]}]});
        projection.observe_desktop("desktop", &snapshot, &json!([
            {"method":"turn/started","params":{"threadId":"desktop","turn":{"id":"new","status":"inProgress"}}},
            {"method":"turn/completed","params":{"threadId":"desktop","turn":{"id":"old","status":"completed"}}}
        ]));
        assert_eq!(projection.active["desktop"], "new");
        assert_eq!(projection.threads["desktop"]["status"]["type"], "active");
        let last_status = projection
            .events
            .iter()
            .rev()
            .find(|(_, event)| event["method"] == "thread/status/changed")
            .unwrap();
        assert_eq!(last_status.1["params"]["status"]["type"], "active");
    }

    #[test]
    fn harness_compaction_survives_reentry_before_history_persistence_and_after_completion() {
        let mut projection = Projection::default();
        let item = json!({"id":"compact-item","type":"contextCompaction","startedAtMs":2000});
        projection.ingest(json!({"method":"item/started","params":{
            "threadId":"main","turnId":"compact-turn","item":item}}));
        let history = json!({"scope":"turns","data":[{"id":"previous","startedAt":1,"items":[]}],
            "itemsPaginationByTurnId":{},"transcriptEvents":[]});
        let active = projection.history("main", history.clone(), true);
        assert_eq!(active["data"][0]["id"], "compact-turn");
        assert_eq!(active["data"][0]["status"], "inProgress");
        assert_eq!(active["transcriptEvents"][0]["item"]["completed"], false);
        assert_eq!(
            active["itemsPaginationByTurnId"]["compact-turn"]["complete"],
            false
        );
        let thread = projection.thread("main", json!({}));
        assert_eq!(thread["compaction"]["state"], "running");
        assert_eq!(thread["transcriptEvents"][0]["item"]["id"], "compact-item");
        projection.ingest(json!({"method":"item/completed","params":{
            "threadId":"main","turnId":"compact-turn","item":item}}));
        let completed = projection.history("main", history, true);
        assert_eq!(completed["transcriptEvents"][0]["item"]["completed"], true);
        assert_eq!(projection.compaction("main")["state"], "idle");
        // 迟到的开始事件不能把完成记录重新锁定，且不依赖发送新消息来恢复显示。
        projection.ingest(json!({"method":"item/started","params":{
            "threadId":"main","turnId":"compact-turn","item":item}}));
        assert_eq!(projection.compaction("main")["state"], "idle");
        assert_eq!(
            projection.thread("main", json!({}))["transcriptEvents"][0]["item"]["completed"],
            true
        );
    }

    #[test]
    fn harness_compaction_history_keeps_pagination_and_other_threads_isolated() {
        let mut projection = Projection::default();
        projection.ingest(
            json!({"method":"item/started","params":{"threadId":"main","turnId":"compact-turn",
            "item":{"id":"compact","type":"contextCompaction","startedAtMs":2000}}}),
        );
        let page = json!({"scope":"turns","data":[{"id":"older","startedAt":1,"items":[]}],
            "itemsPaginationByTurnId":{},"transcriptEvents":[],"nextCursor":"older-cursor"});
        let older = projection.history("main", page.clone(), false);
        assert_eq!(older, page);
        assert_eq!(projection.history("other", page.clone(), true), page);
        let persisted = json!({"scope":"turns","data":[{"id":"compact-turn","startedAt":2,"items":[]}],
            "itemsPaginationByTurnId":{},"transcriptEvents":[]});
        assert_eq!(
            projection.history("main", persisted, true)["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        projection
            .ingest(json!({"method":"harness/connected","_generation":"new","_generationIndex":1}));
        assert!(projection.compaction_records.is_empty());
    }

    #[test]
    fn harness_cached_summary_uses_live_activity_approval_and_read_state() {
        let mut projection = Projection::default();
        let cached = crate::codex_service::normalize_thread(&json!({
            "id":"main","cwd":"test","status":{"type":"idle"},"hasUnreadTurn":true
        }))
        .unwrap();
        projection.ingest(json!({"method":"turn/started","params":{
            "threadId":"main","turn":{"id":"turn-a"}
        }}));
        assert_eq!(
            projection.thread("main", cached.clone())["priority"]["active"],
            true
        );
        projection.ingest(
            json!({"id":7,"method":"item/commandExecution/requestApproval",
            "params":{"threadId":"main","turnId":"turn-a"}}),
        );
        assert_eq!(
            projection.thread("main", cached.clone())["priority"]["attentionState"],
            "waiting"
        );
        projection.resolve(&request_key("", &json!(7)));
        projection.ingest(json!({"method":"turn/completed","params":{
            "threadId":"main","turn":{"id":"turn-a","status":"completed"}
        }}));
        projection.ingest(json!({"method":"thread/readState/updated","params":{
            "threadId":"main","readState":{"firstUnread":null,"revision":"read"}
        }}));
        let idle = projection.thread("main", cached);
        assert_eq!(idle["priority"]["active"], false);
        assert_eq!(idle["priority"]["attentionState"], "idle");
        assert_eq!(idle["hasUnreadTurn"], false);
    }

    #[test]
    fn harness_projection_rejects_old_generations_and_preserves_terminal_order() {
        let mut projection = Projection::default();
        projection
            .ingest(json!({"method":"harness/connected","_generation":"new","_generationIndex":2}));
        let epoch = projection.epoch.clone();
        projection
            .ingest(json!({"method":"harness/connected","_generation":"old","_generationIndex":1}));
        projection.ingest(json!({"method":"harness/disconnected","_generation":"old"}));
        assert_eq!(projection.epoch, epoch);
        projection.ingest(json!({"method":"turn/completed","_generation":"new",
            "params":{"threadId":"thread-a","turn":{"id":"turn-a","status":"completed"}}}));
        projection.started("thread-a", "turn-a");
        assert!(projection.active.is_empty());
        let sequence = projection.sequence;
        projection.ingest(json!({"method":"turn/completed","_generation":"new",
            "params":{"threadId":"thread-a","turn":{"id":"turn-a","status":"completed"}}}));
        assert_eq!(projection.sequence, sequence);
    }

    #[test]
    fn harness_projection_approval_notifications_survive_snapshot_and_resolve() {
        let mut projection = Projection::default();
        projection.ingest(
            json!({"method":"harness/connected","_generation":"engine","_generationIndex":1}),
        );
        projection.ingest(
            json!({"id":7,"method":"item/commandExecution/requestApproval","_generation":"engine",
            "params":{"threadId":"thread-a","command":"echo test"}}),
        );
        let snapshot = projection.notifications(&json!({}));
        assert_eq!(snapshot["pending"].as_array().unwrap().len(), 1);
        super::super::super::validate_notifications_response(&snapshot, &json!({})).unwrap();
        let cursor = projection.notification_sequence;
        projection.resolve(&request_key("engine", &json!(7)));
        let request = json!({"epoch":projection.epoch,"cursor":cursor});
        let updated = projection.notifications(&request);
        super::super::super::validate_notifications_response(&updated, &request).unwrap();
        assert_eq!(updated["pending"], json!([]));
        assert_eq!(updated["events"][0]["action"], "dismiss");
        assert!(projection.requests.is_empty());
    }

    #[test]
    fn harness_subagent_events_refresh_parents_without_adding_writable_requests() {
        let mut projection = Projection::default();
        projection
            .ingest(json!({"method":"harness/connected","_generation":"one","_generationIndex":1}));
        projection.ingest(
            json!({"method":"thread/started","_generation":"one","params":{
            "thread":{"id":"child","cwd":"test","parentThreadId":"parent","ephemeral":true,
                "agentNickname":"测试子智能体","status":{"type":"active"}}}}),
        );
        assert!(projection.subagents.threads.contains_key("child"));
        assert!(!projection.threads.contains_key("child"));
        projection.ingest(
            json!({"id":17,"method":"item/commandExecution/requestApproval","_generation":"one",
            "params":{"threadId":"child","turnId":"turn"}}),
        );
        projection.ingest(
            json!({"method":"item/agentMessage/delta","_generation":"one",
            "params":{"threadId":"child","turnId":"turn","itemId":"reply","delta":"片段"}}),
        );
        assert!(projection.requests.is_empty());
        assert!(projection.notifications.is_empty());
        assert!(projection.events.iter().any(|(_, event)| event["method"]
            == "remote/relatedThreadsChanged"
            && event["params"]["threadId"] == "parent"));
        assert!(
            projection
                .events
                .iter()
                .any(|(_, event)| event["method"] == "item/agentMessage/delta"
                    && event["params"]["threadId"] == "child")
        );
        projection
            .ingest(json!({"method":"harness/connected","_generation":"two","_generationIndex":2}));
        projection.ingest(
            json!({"method":"thread/started","_generation":"one","params":{
            "thread":{"id":"old-child","cwd":"test","parentThreadId":"parent","ephemeral":true}}}),
        );
        assert!(
            projection.subagents.threads.is_empty(),
            "旧连接的临时子会话不能在重连后复活"
        );
    }

    #[test]
    fn harness_projection_filters_ephemeral_children_and_resets_truncated_event_cursors() {
        let mut projection = Projection::default();
        projection.ingest(json!({"method":"thread/started","params":{"thread":{"id":"child","cwd":"test","ephemeral":true}}}));
        projection.ingest(json!({"method":"turn/completed","params":{"threadId":"child","turn":{"id":"child-turn","status":"completed"}}}));
        assert!(projection.notifications.is_empty());
        for index in 0..MAX_EVENTS + 2 {
            projection.push(json!({"method":"test/event","params":{"index":index}}));
        }
        let response = projection.events(&json!({"epoch":projection.epoch,"cursor":0}));
        assert_eq!(response["reset"], true);
        assert_eq!(response["events"], json!([]));
    }
}
