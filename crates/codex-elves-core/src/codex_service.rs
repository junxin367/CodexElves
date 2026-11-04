use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc};

use crate::desktop_owner_ipc::{self, OwnerSend, WriteFrame};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_MODEL_PAGES: usize = 128;
const SERVICE_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);
const SERVICE_LIST_PAGE_LIMIT: usize = 100;
const SERVICE_HISTORY_TURN_LIMIT: usize = 3;
const SERVICE_HISTORY_ITEM_LIMIT: usize = 40;
const MAX_OWNER_WATCHERS: usize = 8;
const MAX_SERVICE_EVENTS: usize = 2_048;
const MAX_SERVICE_NOTIFICATIONS: usize = 256;

#[derive(Clone, Default)]
struct OwnerView {
    generation: u64,
    owner: Option<String>,
    revision: u64,
    connected: bool,
    snapshot: Value,
}

struct ServiceProjection {
    epoch: String,
    sequence: u64,
    events: VecDeque<(u64, Value)>,
    notification_sequence: u64,
    owner_generation: u64,
    notifications: VecDeque<Value>,
    threads: BTreeMap<String, OwnerView>,
    metadata: BTreeMap<String, Value>,
}

impl ServiceProjection {
    fn new() -> Self {
        Self {
            epoch: format!("codex-service-{}", uuid::Uuid::new_v4()),
            sequence: 0,
            events: VecDeque::new(),
            notification_sequence: 0,
            owner_generation: 0,
            notifications: VecDeque::new(),
            threads: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    fn push(&mut self, event: Value) {
        self.sequence += 1;
        self.events.push_back((self.sequence, event));
        while self.events.len() > MAX_SERVICE_EVENTS {
            self.events.pop_front();
        }
    }

    fn remember_metadata(&mut self, thread_id: &str, mut metadata: Value) {
        if let Some(previous) = self.metadata.get(thread_id) {
            for field in ["readState", "hasUnreadTurn"] {
                if metadata.get(field).is_none_or(Value::is_null) {
                    if let Some(value) = previous.get(field) {
                        metadata[field] = value.clone();
                    }
                }
            }
        }
        self.metadata.insert(thread_id.to_string(), metadata);
        while self.metadata.len() > 512 {
            self.metadata.pop_first();
        }
    }

    fn begin_subscription(&mut self, thread_id: &str) -> u64 {
        self.owner_generation += 1;
        let generation = self.owner_generation;
        let view = self.threads.entry(thread_id.to_string()).or_default();
        view.generation = generation;
        view.revision = 0;
        view.connected = false;
        generation
    }

    fn subscribe_from(&mut self, thread_id: &str, snapshot: &Value) -> u64 {
        self.threads
            .entry(thread_id.to_string())
            .or_insert_with(|| OwnerView {
                snapshot: snapshot.clone(),
                ..OwnerView::default()
            });
        self.begin_subscription(thread_id)
    }

    fn observe(
        &mut self,
        thread_id: &str,
        generation: u64,
        owner: &str,
        revision: u64,
        snapshot: Value,
    ) {
        let previous = self.threads.remove(thread_id);
        let Some(previous) = previous else { return };
        // Revisions belong to one following subscription. A replacement stream
        // can restart at r1; callbacks from the old stream cannot take it over.
        if previous.generation != generation
            || previous.owner.as_deref() == Some(owner) && revision < previous.revision
        {
            self.threads.insert(thread_id.to_string(), previous);
            return;
        }
        if !previous.snapshot.is_null() {
            if previous.owner.as_deref().is_some_and(|old| old != owner) {
                self.push(json!({"method":"remote/resync"}));
            } else {
                self.project_changes(thread_id, &previous.snapshot, &snapshot);
            }
        }
        self.threads.insert(
            thread_id.to_string(),
            OwnerView {
                generation,
                owner: Some(owner.to_string()),
                revision,
                connected: true,
                snapshot,
            },
        );
    }

    fn owner_unavailable(&mut self, thread_id: &str, generation: u64) {
        if let Some(view) = self.threads.get_mut(thread_id) {
            if view.generation == generation && view.connected {
                view.connected = false;
                self.push(json!({"method":"remote/resync"}));
            }
        }
    }

    fn project_changes(&mut self, thread_id: &str, before: &Value, after: &Value) {
        for (field, method, parameter) in [
            ("threadRuntimeStatus", "thread/status/changed", "status"),
            (
                "latestTokenUsageInfo",
                "thread/tokenUsage/updated",
                "tokenUsage",
            ),
        ] {
            if before.get(field) != after.get(field) && after.get(field).is_some() {
                let mut params = json!({"threadId":thread_id});
                params[parameter] = after[field].clone();
                self.push(json!({"method":method,"params":params}));
            }
        }
        if before["title"] != after["title"] && after["title"].as_str().is_some() {
            self.push(json!({"method":"thread/name/updated","params":{
                "threadId":thread_id,"name":after["title"]}}));
        }
        if before["requests"] != after["requests"] && after["requests"].is_array() {
            self.push(json!({"method":"remote/requestsChanged","params":{
                "threadId":thread_id,"requests":after["requests"],
                "requestsEpoch":self.epoch,"requestsRevision":self.sequence+1}}));
        }
        if before["threadGoal"] != after["threadGoal"] {
            self.push(if after["threadGoal"].is_null() {
                json!({"method":"thread/goal/cleared","params":{"threadId":thread_id}})
            } else {
                json!({"method":"thread/goal/updated","params":{
                    "threadId":thread_id,"goal":after["threadGoal"]}})
            });
        }
        let old_turns = desktop_owner_ipc::snapshot_turns(before);
        let max_started = old_turns
            .iter()
            .filter_map(|turn| turn["turnStartedAtMs"].as_u64())
            .max();
        let old_turns: BTreeMap<_, _> = old_turns
            .into_iter()
            .filter_map(|turn| {
                turn["turnId"]
                    .as_str()
                    .map(str::to_string)
                    .map(|id| (id, turn))
            })
            .collect();
        for mut turn in desktop_owner_ipc::snapshot_turns(after) {
            let Some(turn_id) = turn["turnId"].as_str().map(str::to_string) else {
                continue;
            };
            let previous = old_turns.get(&turn_id);
            let new_live_turn = previous.is_some()
                || turn["status"] == "inProgress"
                || turn["turnStartedAtMs"].as_u64().is_some_and(|started| {
                    max_started.map_or_else(
                        || {
                            before["turnHistory"]["kind"] == "canonical"
                                && before["turnHistory"]["history"]["isComplete"] == true
                        },
                        |latest| started > latest,
                    )
                });
            if !new_live_turn {
                // Historical pagination is not a newly completed turn.
                continue;
            }
            turn["id"] = json!(&turn_id);
            let status = turn["status"].as_str().unwrap_or_default();
            let terminal = matches!(
                status,
                "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
            );
            let changed_status = previous.is_none_or(|old| old["status"] != turn["status"]);
            if changed_status && (status == "inProgress" || terminal) {
                let method = if terminal {
                    "turn/completed"
                } else {
                    "turn/started"
                };
                self.push(json!({"method":method,"params":{"threadId":thread_id,
                    "turnId":turn_id,"turn":turn}}));
                if terminal && matches!(status, "completed" | "failed") {
                    self.notify_terminal(thread_id, &turn);
                }
                continue;
            }
            for item in turn["items"].as_array().into_iter().flatten() {
                let old_item = previous
                    .and_then(|old| old["items"].as_array())
                    .and_then(|items| items.iter().find(|old| old["id"] == item["id"]));
                if old_item != Some(item) {
                    // Exact observed item state, not invented deltas or completion.
                    self.push(json!({"method":"item/updated","params":{
                        "threadId":thread_id,"turnId":turn_id,"item":item}}));
                }
            }
        }
    }

    fn notify_terminal(&mut self, thread_id: &str, turn: &Value) {
        self.notification_sequence += 1;
        let sequence = self.notification_sequence;
        let id = format!("{}:{sequence}", self.epoch);
        let title = self
            .metadata
            .get(thread_id)
            .and_then(|thread| thread["title"].as_str())
            .unwrap_or("会话");
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect();
        let created_at = turn["turnCompletedAtMs"].as_u64().unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        });
        let event = json!({"id":id,"epoch":self.epoch,"sequence":sequence,
            "threadId":thread_id,"kind":if turn["status"]=="failed" {"failed"} else {"completed"},
            "title":if title.is_empty() {"会话"} else {&title},"createdAtMs":created_at,
            "action":"notify","notificationId":id});
        self.notifications.push_back(event.clone());
        while self.notifications.len() > MAX_SERVICE_NOTIFICATIONS {
            self.notifications.pop_front();
        }
        self.push(json!({"method":"remote/notification","params":event}));
    }

    fn events(&self, payload: &Value) -> Value {
        let cursor = payload["cursor"].as_u64().unwrap_or(0);
        let reset = payload["epoch"] != self.epoch
            || cursor > self.sequence
            || self
                .events
                .front()
                .is_some_and(|(sequence, _)| cursor.saturating_add(1) < *sequence);
        let events: Vec<_> = if reset {
            Vec::new()
        } else {
            self.events
                .iter()
                .filter(|(sequence, _)| *sequence > cursor)
                .map(|(_, event)| event.clone())
                .collect()
        };
        json!({"ok":true,"epoch":self.epoch,"cursor":self.sequence,"reset":reset,
            "nativeBridge":"service","events":events})
    }

    fn notifications(&self, payload: &Value) -> Value {
        let cursor = payload["cursor"].as_u64().unwrap_or(0);
        let reset = payload["epoch"] != self.epoch
            || cursor > self.notification_sequence
            || self.notifications.front().is_some_and(|event| {
                cursor.saturating_add(1) < event["sequence"].as_u64().unwrap_or(0)
            });
        let events: Vec<_> = if reset {
            Vec::new()
        } else {
            self.notifications
                .iter()
                .filter(|event| event["sequence"].as_u64().unwrap_or(0) > cursor)
                .cloned()
                .collect()
        };
        json!({"ok":true,"schemaVersion":1,"notificationEpoch":self.epoch,
            "notificationCursor":self.notification_sequence,"reset":reset,
            "truncated":false,"pending":[],"events":events})
    }
}

async fn watch_owner_projection(
    thread_id: String,
    projection: Arc<Mutex<ServiceProjection>>,
    mut generation: u64,
) {
    loop {
        let (changes, mut snapshots) = mpsc::channel(8);
        let watch = desktop_owner_ipc::watch_snapshots(thread_id.clone(), changes);
        tokio::pin!(watch);
        loop {
            tokio::select! {
                Some(observation) = snapshots.recv() => {
                    projection.lock().await.observe(
                        &thread_id, generation, &observation.owner,
                        observation.revision, observation.snapshot,
                    );
                }
                _ = &mut watch => {
                    while let Ok(observation) = snapshots.try_recv() {
                        projection.lock().await.observe(
                            &thread_id, generation, &observation.owner,
                            observation.revision, observation.snapshot,
                        );
                    }
                    break;
                }
            }
        }
        projection
            .lock()
            .await
            .owner_unavailable(&thread_id, generation);
        // Reconnect is read-only and cannot resume or acquire a writer.
        tokio::time::sleep(Duration::from_secs(2)).await;
        generation = projection.lock().await.begin_subscription(&thread_id);
    }
}

pub trait ServiceDispatchPermit: Send {
    fn commit(
        self: Box<Self>,
        dispatch: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()>;
}

#[async_trait]
pub trait ServiceDispatchGuard: Send + Sync {
    async fn prepare_dispatch(&self) -> anyhow::Result<Box<dyn ServiceDispatchPermit>>;
}

#[derive(Debug)]
struct RpcFailure {
    method: String,
    code: i64,
    message: String,
}

impl Display for RpcFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "RPC {} 失败（{}）：{}",
            self.method, self.code, self.message
        )
    }
}

impl std::error::Error for RpcFailure {}

struct RunningService {
    child: Child,
    stdin: mpsc::Sender<WriteFrame>,
    stdout: BufReader<ChildStdout>,
    executable: String,
    initialized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    path: PathBuf,
    shell: bool,
    version: Option<crate::cli_wrapper::CodexCliVersion>,
    ordinal: usize,
}

pub struct CodexCliService {
    process: Mutex<Option<RunningService>>,
    next_request_id: AtomicU64,
    cooldown_until: Mutex<Option<Instant>>,
    projection: Arc<Mutex<ServiceProjection>>,
    owner_watchers: Mutex<VecDeque<(String, tokio::task::JoinHandle<()>)>>,
}

impl Drop for CodexCliService {
    fn drop(&mut self) {
        let watchers = self.owner_watchers.get_mut();
        for (_, task) in watchers.drain(..) {
            task.abort();
        }
    }
}

impl CodexCliService {
    pub fn new() -> Self {
        Self {
            process: Mutex::new(None),
            next_request_id: AtomicU64::new(1),
            cooldown_until: Mutex::new(None),
            projection: Arc::new(Mutex::new(ServiceProjection::new())),
            owner_watchers: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        match action {
            "sidebar" => self.sidebar(payload).await,
            "history" => self.history(payload).await,
            "events" => {
                let owner_connected = self
                    .projection
                    .lock()
                    .await
                    .threads
                    .values()
                    .any(|view| view.connected);
                if !owner_connected && !desktop_owner_ipc::available().await {
                    self.request(
                        "thread/list",
                        json!({"cursor": null, "limit": 1, "sortKey": "updated_at", "archived": false}),
                    ).await?;
                }
                Ok(self.projection.lock().await.events(&payload))
            }
            "notifications" => Ok(self.projection.lock().await.notifications(&payload)),
            "models" => self.models().await,
            "skills" => self.skills(payload).await,
            "thread" => self.thread(payload).await,
            "workspaceContext" => self.workspace_context(payload).await,
            "send" => self.send(payload, None).await,
            _ => bail!("Codex CLI service 暂不支持远程操作 {action}"),
        }
    }

    pub async fn call_with_dispatch_guard(
        &self,
        action: &str,
        payload: Value,
        guard: &dyn ServiceDispatchGuard,
    ) -> anyhow::Result<Value> {
        if action == "send" {
            self.send(payload, Some(guard)).await
        } else {
            self.call(action, payload).await
        }
    }

    async fn follow_owner(&self, thread_id: &str, snapshot: &Value) {
        let mut watchers = self.owner_watchers.lock().await;
        if let Some(position) = watchers.iter().position(|(id, _)| id == thread_id) {
            let watcher = watchers.remove(position).unwrap();
            if !watcher.1.is_finished() {
                watchers.push_back(watcher);
                return;
            }
            watcher.1.abort();
        }
        let mut projection = self.projection.lock().await;
        if watchers.len() >= MAX_OWNER_WATCHERS {
            let (evicted, task) = watchers.pop_front().unwrap();
            task.abort();
            if projection
                .threads
                .remove(&evicted)
                .is_some_and(|view| view.connected)
            {
                projection.push(json!({"method":"remote/resync"}));
            }
        }
        // Register the baseline and stream generation atomically before spawn.
        // A very fast turn can finish before the first subscription snapshot.
        let generation = projection.subscribe_from(thread_id, snapshot);
        drop(projection);
        let projection = Arc::clone(&self.projection);
        let watched_id = thread_id.to_string();
        let task = tokio::spawn(async move {
            watch_owner_projection(watched_id, projection, generation).await;
        });
        watchers.push_back((thread_id.to_string(), task));
    }

    async fn owner_metadata(&self, thread_id: &str) -> Option<Value> {
        if let Some(metadata) = self
            .projection
            .lock()
            .await
            .metadata
            .get(thread_id)
            .cloned()
        {
            return Some(metadata);
        }
        // Read static metadata only. This independent runtime never resumes the
        // owner's thread and cannot override its authoritative runtime status.
        let response = self
            .request(
                "thread/read",
                json!({"threadId":thread_id,"includeTurns":false}),
            )
            .await
            .ok()?;
        let metadata = normalize_thread_response(&response).ok()?;
        self.projection
            .lock()
            .await
            .remember_metadata(thread_id, metadata.clone());
        Some(metadata)
    }

    async fn sidebar(&self, _payload: Value) -> anyhow::Result<Value> {
        let mut cursor = None;
        let mut cursors = std::collections::HashSet::new();
        let mut recent = Vec::new();
        loop {
            let page = self
                .request(
                    "thread/list",
                    json!({
                        "cursor": cursor,
                        "limit": SERVICE_LIST_PAGE_LIMIT,
                        "sortKey": "updated_at",
                        "archived": false
                    }),
                )
                .await?;
            for thread in page
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Ok(normalized) = normalize_thread(thread) {
                    if let Some(id) = normalized["id"].as_str() {
                        self.projection
                            .lock()
                            .await
                            .remember_metadata(id, normalized.clone());
                    }
                    recent.push(normalized);
                }
            }
            let next = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let Some(next) = next else { break };
            if !cursors.insert(next.clone()) {
                bail!("Codex CLI service 会话列表分页游标重复");
            }
            cursor = Some(next);
        }
        Ok(json!({
            "ok": true,
            "complete": true,
            "projects": service_projects(&mut recent),
            "recent": recent,
            "pinnedThreads": [],
            "nextCursor": null,
            "sideThreadIds": [],
            "unreadSideThreads": [],
            "creationAvailable": false,
            "preferences": {
                "mode": "list",
                "chats": "updated_at",
                "projects": "updated_at",
                "pinned": "updated_at"
            }
        }))
    }

    async fn send(
        &self,
        payload: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Value> {
        let thread_id = required_service_thread_id(&payload)?;
        let params = match service_turn_start_params(&payload, thread_id) {
            Ok(params) => params,
            Err(error) => return Ok(service_not_sent_error(thread_id, &format!("{error:#}"))),
        };
        let preflight = ServiceOwnerPreflight { service: self };
        let owner = desktop_owner_ipc::try_send(&payload, params.clone(), &preflight, guard).await;
        owner_or_cli(owner, || self.send_with_cli(&payload, params, guard)).await
    }

    async fn send_with_cli(
        &self,
        payload: &Value,
        params: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Value> {
        let thread_id = required_service_thread_id(payload)?;
        let response = match self
            .request(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false
                }),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => return Ok(service_not_sent_error(thread_id, &format!("{error:#}"))),
        };
        let mut thread = response.get("thread").unwrap_or(&response).clone();
        if let Err(error) = desktop_owner_ipc::require_main_thread(&thread, thread_id) {
            return Ok(service_not_sent_error(thread_id, &format!("{error:#}")));
        }
        let status = thread
            .get("status")
            .and_then(|value| value.get("type"))
            .or_else(|| thread.get("status"))
            .and_then(Value::as_str);
        if matches!(status, Some("notLoaded" | "not_loaded")) {
            match self
                .request_with_guard("thread/resume", json!({ "threadId": thread_id }), guard)
                .await
            {
                Ok(response) => thread = response.get("thread").unwrap_or(&response).clone(),
                Err(error) => return service_write_failure(thread_id, error),
            }
        }
        if let Err(error) = desktop_owner_ipc::require_main_thread(&thread, thread_id) {
            return Ok(service_not_sent_error(thread_id, &format!("{error:#}")));
        }
        let status = thread["status"]
            .as_str()
            .or_else(|| thread["status"]["type"].as_str());
        if status != Some("idle") {
            return Ok(service_not_sent_error(
                thread_id,
                "发送兜底仅支持状态明确且空闲的主会话",
            ));
        }
        let mut queue_snapshot = thread.clone();
        queue_snapshot["rolloutPath"] = thread["path"].clone();
        if let Err(error) = desktop_owner_ipc::OwnerPreflight::check_queues(
            &ServiceOwnerPreflight { service: self },
            thread_id,
            &queue_snapshot,
        )
        .await
        {
            return Ok(service_not_sent_error(thread_id, &format!("{error:#}")));
        }
        let client_user_message_id = params["clientUserMessageId"].clone();
        let response = match self.request_with_guard("turn/start", params, guard).await {
            Ok(response) => response,
            Err(error) => return service_write_failure(thread_id, error),
        };
        let turn = response
            .get("turn")
            .context("Codex CLI service 未返回轮次信息")?;
        Ok(json!({
            "ok": true,
            "threadId": thread_id,
            "turnId": turn.get("id").cloned().unwrap_or(Value::Null),
            "clientUserMessageId": client_user_message_id,
            "status": turn.get("status").cloned().unwrap_or(Value::Null),
            "deliveryState": "accepted",
            "nativeBridge": "service"
        }))
    }

    async fn history(&self, payload: Value) -> anyhow::Result<Value> {
        let thread_id = payload
            .get("threadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("缺少会话 ID")?;
        if payload.get("cursor").is_none_or(Value::is_null)
            && payload.get("turnId").is_none_or(Value::is_null)
        {
            if let Some(snapshot) = desktop_owner_ipc::read_snapshot(thread_id).await? {
                self.follow_owner(thread_id, &snapshot).await;
                if let Some(history) = owner_snapshot_history(&snapshot, thread_id) {
                    return Ok(history);
                }
            }
        }
        if let Some(turn_id) = payload
            .get("turnId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            let (items, next_cursor) = self.history_items(thread_id, turn_id, None).await?;
            return Ok(json!({
                "ok": true,
                "scope": "items",
                "threadId": thread_id,
                "turnId": turn_id,
                "data": items,
                "itemOrder": "asc",
                "itemDirection": "older",
                "itemsPageComplete": true,
                "itemsComplete": next_cursor.is_none(),
                "itemsNextCursor": next_cursor,
                "newestSnapshotItemId": null,
                "pageLimit": SERVICE_HISTORY_ITEM_LIMIT,
                "transcriptEvents": []
            }));
        }

        let cursor = payload.get("cursor").cloned().unwrap_or(Value::Null);
        let page = self
            .request(
                "thread/turns/list",
                json!({
                    "threadId": thread_id,
                    "cursor": cursor,
                    "limit": SERVICE_HISTORY_TURN_LIMIT,
                    "itemsView": "notLoaded",
                    "sortDirection": "desc"
                }),
            )
            .await?;
        let mut data = Vec::new();
        let mut items_pagination = serde_json::Map::new();
        for raw_turn in page
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(turn_id) = raw_turn
                .get("id")
                .or_else(|| raw_turn.get("turnId"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let (items, next_item_cursor) = self.history_items(thread_id, turn_id, None).await?;
            let mut turn = raw_turn.clone();
            if let Some(object) = turn.as_object_mut() {
                object.insert("id".to_string(), Value::String(turn_id.to_owned()));
                object.insert("items".to_string(), Value::Array(items));
            }
            items_pagination.insert(
                turn_id.to_owned(),
                json!({
                    "nextCursor": next_item_cursor,
                    "hasMore": next_item_cursor.is_some(),
                    "complete": next_item_cursor.is_none(),
                    "pageComplete": true,
                    "direction": "older",
                    "order": "asc",
                    "newestSnapshotItemId": null,
                    "pageLimit": SERVICE_HISTORY_ITEM_LIMIT
                }),
            );
            data.push(turn);
        }
        let next_cursor = page
            .get("nextCursor")
            .cloned()
            .filter(|value| !value.is_null());
        Ok(json!({
            "ok": true,
            "scope": "turns",
            "threadId": thread_id,
            "data": data,
            "nextCursor": next_cursor,
            "backwardsCursor": page.get("backwardsCursor").cloned().unwrap_or(Value::Null),
            "turnOrder": "desc",
            "itemOrder": "asc",
            "turnsPageComplete": true,
            "turnsComplete": next_cursor.is_none(),
            "itemsPaginationByTurnId": items_pagination,
            "transcriptEvents": []
        }))
    }

    async fn history_items(
        &self,
        thread_id: &str,
        turn_id: &str,
        cursor: Option<&str>,
    ) -> anyhow::Result<(Vec<Value>, Option<String>)> {
        let page = self
            .request(
                "thread/items/list",
                json!({
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "cursor": cursor,
                    "limit": SERVICE_HISTORY_ITEM_LIMIT,
                    "sortDirection": "desc"
                }),
            )
            .await?;
        normalize_history_items_page(&page, cursor)
    }

    async fn models(&self) -> anyhow::Result<Value> {
        let mut process_guard = self.process.lock().await;
        self.ensure_service_available().await?;
        self.ensure_process_started(&mut process_guard, "models")
            .await?;

        let result = match self
            .ensure_initialized(process_guard.as_mut().unwrap())
            .await
        {
            Ok(()) => {
                self.models_with_process(process_guard.as_mut().unwrap())
                    .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            self.handle_request_failure(&mut process_guard, "models", error)
                .await;
        }
        result
    }

    async fn skills(&self, payload: Value) -> anyhow::Result<Value> {
        self.ensure_service_available().await?;
        let thread_id = payload.get("threadId").and_then(Value::as_str);
        let cwd = match payload.get("cwd").and_then(Value::as_str) {
            Some(cwd) if !cwd.trim().is_empty() => cwd.to_owned(),
            _ if thread_id.is_some() => {
                let thread = self
                    .request(
                        "thread/read",
                        json!({
                            "threadId": thread_id.unwrap(),
                            "includeTurns": false
                        }),
                    )
                    .await?;
                thread
                    .get("thread")
                    .and_then(|value| value.get("cwd"))
                    .and_then(Value::as_str)
                    .filter(|cwd| !cwd.trim().is_empty())
                    .map(str::to_owned)
                    .context("Codex CLI service 未返回会话工作目录")?
            }
            _ => bail!("Codex CLI service 需要会话工作目录"),
        };
        let response = self
            .request(
                "skills/list",
                json!({
                    "cwds": [cwd],
                    "forceReload": payload.get("forceReload").and_then(Value::as_bool).unwrap_or(false)
                }),
            )
            .await?;
        let entry = response
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|entry| entry.get("cwd").and_then(Value::as_str) == Some(cwd.as_str()));
        let data = entry
            .and_then(|entry| entry.get("skills"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let errors = entry
            .and_then(|entry| entry.get("errors"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        Ok(json!({
            "ok": true,
            "cwd": cwd,
            "data": data,
            "errors": errors
        }))
    }

    async fn thread(&self, payload: Value) -> anyhow::Result<Value> {
        let thread_id = payload
            .get("threadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("缺少会话 ID")?;
        if let Some(snapshot) = desktop_owner_ipc::read_snapshot(thread_id).await? {
            self.follow_owner(thread_id, &snapshot).await;
            let metadata = self.owner_metadata(thread_id).await;
            let normalized = normalize_owner_thread(&snapshot, metadata.as_ref())?;
            return Ok(json!({"ok":true,"thread":normalized,"nativeBridge":"desktop-owner-ipc"}));
        }
        let response = self
            .request(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false
                }),
            )
            .await?;
        let normalized = normalize_thread_response(&response)?;
        self.projection
            .lock()
            .await
            .remember_metadata(thread_id, normalized.clone());
        let goal = self
            .request("thread/goal/get", json!({ "threadId": thread_id }))
            .await
            .ok()
            .and_then(|value| value.get("goal").cloned())
            .unwrap_or(Value::Null);
        let mut normalized = normalized;
        normalized["goal"] = goal;
        Ok(json!({ "ok": true, "thread": normalized }))
    }

    async fn workspace_context(&self, payload: Value) -> anyhow::Result<Value> {
        let thread_id = payload
            .get("threadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("Codex CLI service 需要 threadId")?;
        let response = self
            .request(
                "thread/read",
                json!({
                    "threadId": thread_id,
                    "includeTurns": false
                }),
            )
            .await?;
        let cwd = response
            .get("thread")
            .and_then(|thread| thread.get("cwd"))
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.trim().is_empty())
            .context("Codex CLI service 未返回会话工作目录")?;
        Ok(json!({
            "ok": true,
            "hostId": "local",
            "threadId": thread_id,
            "cwd": cwd
        }))
    }

    async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.request_with_guard(method, params, None).await
    }

    async fn request_with_guard(
        &self,
        method: &str,
        params: Value,
        guard: Option<&dyn ServiceDispatchGuard>,
    ) -> anyhow::Result<Value> {
        let mut process_guard = self.process.lock().await;
        self.ensure_service_available().await?;
        self.ensure_process_started(&mut process_guard, method)
            .await?;
        let process = process_guard.as_mut().unwrap();
        let result = match self.ensure_initialized(process).await {
            Ok(()) => {
                let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
                rpc_request_guarded(process, request_id, method, params, guard).await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            self.handle_request_failure(&mut process_guard, method, error)
                .await;
        }
        result
    }

    async fn ensure_process_started(
        &self,
        process_guard: &mut Option<RunningService>,
        action: &str,
    ) -> anyhow::Result<()> {
        if process_guard.is_none() {
            match spawn_service().await {
                Ok(process) => *process_guard = Some(process),
                Err(error) => {
                    self.enter_cooldown_if_needed(action, &error).await;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    async fn ensure_service_available(&self) -> anyhow::Result<()> {
        let now = Instant::now();
        let mut cooldown = self.cooldown_until.lock().await;
        let Some(until) = *cooldown else {
            return Ok(());
        };
        if until <= now {
            *cooldown = None;
            return Ok(());
        }
        let retry_after = until.duration_since(now).as_secs().max(1);
        bail!("Codex CLI service 暂时跳过，约 {retry_after} 秒后重试");
    }

    async fn handle_request_failure(
        &self,
        process_guard: &mut Option<RunningService>,
        action: &str,
        error: &anyhow::Error,
    ) {
        if error
            .downcast_ref::<desktop_owner_ipc::DispatchFailure>()
            .is_some_and(|failure| !failure.dispatched)
        {
            return;
        }
        if is_nonfatal_missing_thread_failure(action, error) {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.codex_service_thread_missing",
                json!({
                    "action": action,
                    "error": truncate_error(&error.to_string())
                }),
            );
            return;
        }
        if is_nonfatal_optional_rpc_failure(action, error) {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.codex_service_optional_unavailable",
                json!({
                    "action": action,
                    "error": truncate_error(&error.to_string())
                }),
            );
            return;
        }

        if let Some(process) = process_guard.as_mut() {
            let _ = process.child.start_kill();
        }
        *process_guard = None;
        self.enter_cooldown_if_needed(action, error).await;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service_rpc_failed",
            json!({
                "action": action,
                "error": truncate_error(&format!("{error:#}"))
            }),
        );
    }

    async fn enter_cooldown_if_needed(&self, action: &str, error: &anyhow::Error) {
        if !should_enter_service_cooldown(error) {
            return;
        }
        let now = Instant::now();
        let mut cooldown = self.cooldown_until.lock().await;
        if cooldown.is_some_and(|until| until > now) {
            return;
        }
        *cooldown = Some(now + SERVICE_FAILURE_COOLDOWN);
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service_cooldown",
            json!({
                "action": action,
                "retryAfterSeconds": SERVICE_FAILURE_COOLDOWN.as_secs(),
                "error": truncate_error(&format!("{error:#}"))
            }),
        );
    }

    async fn ensure_initialized(&self, process: &mut RunningService) -> anyhow::Result<()> {
        if process.initialized {
            return Ok(());
        }
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        rpc_request(
            process,
            request_id,
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-elves-remote-access",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }),
        )
        .await
        .context("Codex CLI service 初始化失败")?;
        send_notification(process, "initialized", json!({})).await?;
        process.initialized = true;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "remote_access.codex_service.initialized",
            json!({ "executable": process.executable }),
        );
        Ok(())
    }

    async fn models_with_process(&self, process: &mut RunningService) -> anyhow::Result<Value> {
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        let mut cursors = HashSet::new();
        for _ in 0..MAX_MODEL_PAGES {
            let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            let page = rpc_request(
                process,
                request_id,
                "model/list",
                json!({
                    "cursor": cursor,
                    "limit": 100,
                    "includeHidden": false
                }),
            )
            .await
            .context("Codex CLI service 模型列表读取失败")?;
            if let Some(entries) = page.get("data").and_then(Value::as_array) {
                models.extend(entries.iter().filter_map(normalize_model));
            }
            let next_cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
            let Some(next) = next_cursor.as_str().filter(|value| !value.is_empty()) else {
                break;
            };
            if !cursors.insert(next.to_string()) {
                bail!("Codex CLI service 模型列表分页游标重复");
            }
            cursor = Value::String(next.to_string());
        }
        if models.is_empty() {
            bail!("Codex CLI service 返回的模型列表为空");
        }

        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let config = rpc_request(
            process,
            request_id,
            "config/read",
            json!({ "cwd": null, "includeLayers": false }),
        )
        .await
        .context("Codex CLI service 配置读取失败")?;
        let config = config.get("config").unwrap_or(&config);
        let default_model = config
            .get("model")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        let default_reasoning_effort = config
            .get("model_reasoning_effort")
            .or_else(|| config.get("modelReasoningEffort"))
            .or_else(|| config.get("reasoningEffort"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        let ultra_enabled = models.iter().any(|model| {
            model["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|effort| effort["reasoningEffort"] == "ultra")
        });

        Ok(json!({
            "ok": true,
            "data": models,
            "ultraReasoningEnabled": ultra_enabled,
            "defaultModel": default_model,
            "defaultReasoningEffort": default_reasoning_effort
        }))
    }
}

async fn spawn_service() -> anyhow::Result<RunningService> {
    let settings = crate::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    let codex_home = crate::codex_home::codex_home_dir_for_settings(&settings);
    let candidates = discover_candidates().await;
    let mut errors = Vec::new();
    for candidate in candidates {
        match spawn_candidate(&candidate, &codex_home).await {
            Ok(service) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "remote_access.codex_service.started",
                    json!({
                        "executable": candidate.path.to_string_lossy(),
                        "shell": candidate.shell,
                        "processId": service.child.id(),
                        "codexHome": codex_home.to_string_lossy(),
                        "hidden": true
                    }),
                );
                return Ok(service);
            }
            Err(error) => errors.push(format!("{}: {error}", candidate.path.display())),
        }
    }
    let message = if errors.is_empty() {
        "未找到 Codex CLI 可执行文件".to_string()
    } else {
        format!("无法启动 Codex CLI app-server：{}", errors.join("；"))
    };
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "remote_access.codex_service.unavailable",
        json!({ "error": truncate_error(&message) }),
    );
    bail!(message)
}

async fn spawn_candidate(
    candidate: &Candidate,
    codex_home: &Path,
) -> anyhow::Result<RunningService> {
    let mut command = candidate_command(candidate);

    let working_directory = std::env::current_dir().context("读取 CodexElves 工作目录失败")?;
    command
        .current_dir(working_directory)
        .env("CODEX_HOME", codex_home)
        .env("HOME", codex_home)
        .env("USERPROFILE", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 {} 失败", candidate.path.display()))?;
    let stdin = child
        .stdin
        .take()
        .context("Codex CLI service 标准输入不可用")?;
    let (stdin, _writer_task) = desktop_owner_ipc::spawn_writer(stdin, REQUEST_TIMEOUT);
    let stdout = child
        .stdout
        .take()
        .context("Codex CLI service 标准输出不可用")?;
    if let Some(stderr) = child.stderr.take() {
        let executable = candidate.path.to_string_lossy().to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "remote_access.codex_service.stderr",
                    json!({
                        "executable": executable,
                        "message": truncate_error(&line)
                    }),
                );
            }
        });
    }
    Ok(RunningService {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        executable: candidate.path.to_string_lossy().to_string(),
        initialized: false,
    })
}

fn candidate_command(candidate: &Candidate) -> Command {
    #[cfg(not(windows))]
    if candidate.shell {
        let command_line = format!(
            "\"{}\" app-server --analytics-default-enabled",
            candidate.path.display()
        );
        let mut command = Command::new("sh");
        command.args(["-lc", &command_line]);
        return command;
    }

    // Rust invokes .cmd/.bat through cmd.exe with the required quoting. Building
    // a cmd /c string with ordinary args escapes its quotes as literal backslashes.
    let mut command = Command::new(&candidate.path);
    command.args(["app-server", "--analytics-default-enabled"]);
    #[cfg(windows)]
    command.creation_flags(crate::windows_create_hidden_process());
    command
}

async fn discover_candidates() -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |path: PathBuf, shell: bool| {
        if crate::cli_wrapper::is_desktop_codex_path(&path) {
            return;
        }
        let key = format!("{}:{shell}", path.to_string_lossy().to_ascii_lowercase());
        if seen.insert(key) {
            let ordinal = candidates.len();
            candidates.push(Candidate {
                path,
                shell,
                version: None,
                ordinal,
            });
        }
    };

    for path in crate::cli_wrapper::default_user_runtime_candidates() {
        if path.is_file() {
            add(path, false);
        }
    }
    let mut discovery_command = Command::new(if cfg!(windows) { "where.exe" } else { "sh" });
    discovery_command.args(if cfg!(windows) {
        vec!["codex".to_string()]
    } else {
        vec!["-lc".to_string(), "command -v codex".to_string()]
    });
    #[cfg(windows)]
    discovery_command.creation_flags(crate::windows_create_hidden_process());
    if let Ok(output) = discovery_command.output().await {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = PathBuf::from(line.trim());
            if path.as_os_str().is_empty() {
                continue;
            }
            if cfg!(windows) && path.extension().is_none() {
                continue;
            }
            let shell = path.extension().is_none()
                || path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("cmd")
                        || extension.eq_ignore_ascii_case("bat")
                        || extension.eq_ignore_ascii_case("sh")
                });
            add(path, shell);
        }
    }
    add(PathBuf::from("codex"), true);
    // 保留主线的 CLI 选择约束：按实际版本优先使用较新运行时，不能误启桌面可执行文件。
    for candidate in &mut candidates {
        candidate.version = crate::cli_wrapper::probe_codex_cli_version(&candidate.path);
    }
    candidates.sort_by(compare_candidates);
    candidates
}

fn compare_candidates(left: &Candidate, right: &Candidate) -> std::cmp::Ordering {
    match (&left.version, &right.version) {
        (Some(left), Some(right)) => crate::cli_wrapper::compare_codex_versions(right, left),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.ordinal.cmp(&right.ordinal),
    }
    .then_with(|| left.shell.cmp(&right.shell))
    .then_with(|| left.path.cmp(&right.path))
}

async fn send_notification(
    process: &mut RunningService,
    method: &str,
    params: Value,
) -> anyhow::Result<()> {
    let mut line = serde_json::to_vec(&json!({
        "method": method,
        "params": params
    }))?;
    line.push(b'\n');
    desktop_owner_ipc::guarded_write(&process.stdin, line, None, REQUEST_TIMEOUT).await?;
    Ok(())
}

async fn rpc_request(
    process: &mut RunningService,
    request_id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    rpc_request_guarded(process, request_id, method, params, None).await
}

async fn rpc_request_guarded(
    process: &mut RunningService,
    request_id: u64,
    method: &str,
    params: Value,
    guard: Option<&dyn ServiceDispatchGuard>,
) -> anyhow::Result<Value> {
    let mut line = serde_json::to_vec(&json!({
        "id": request_id,
        "method": method,
        "params": params
    }))?;
    line.push(b'\n');
    desktop_owner_ipc::guarded_write(&process.stdin, line, guard, REQUEST_TIMEOUT).await?;

    loop {
        let mut response_line = String::new();
        let bytes_read = tokio::time::timeout(
            REQUEST_TIMEOUT,
            process.stdout.read_line(&mut response_line),
        )
        .await
        .context("等待 Codex CLI service 响应超时")??;
        if bytes_read == 0 {
            let status = process.child.try_wait().ok().flatten();
            bail!("Codex CLI service 已退出（状态：{status:?}）");
        }
        let response: Value = match serde_json::from_str(response_line.trim()) {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.get("id").and_then(Value::as_u64) != Some(request_id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(anyhow::Error::new(RpcFailure {
                method: method.to_string(),
                code,
                message: message.to_string(),
            }));
        }
        return Ok(response.get("result").cloned().unwrap_or(Value::Null));
    }
}

fn rpc_failure(error: &anyhow::Error) -> Option<&RpcFailure> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RpcFailure>())
}

fn is_nonfatal_optional_rpc_failure(method: &str, error: &anyhow::Error) -> bool {
    let Some(failure) = rpc_failure(error) else {
        return false;
    };
    method == "thread/goal/get" && matches!(failure.code, -32600 | -32601)
}

fn is_nonfatal_missing_thread_failure(method: &str, error: &anyhow::Error) -> bool {
    let Some(failure) = rpc_failure(error) else {
        return false;
    };
    method == "thread/read"
        && failure.method == "thread/read"
        && (failure.message.contains("thread not loaded:")
            || failure.message.contains("unknown thread:")
            || failure.message.contains("no rollout found for thread id "))
}

fn is_configuration_failure(error: &anyhow::Error) -> bool {
    let text = error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
        .to_ascii_lowercase();
    text.contains("config.toml")
        || text.contains("failed to reload config")
        || text.contains("invalid configuration")
}

fn should_enter_service_cooldown(error: &anyhow::Error) -> bool {
    is_configuration_failure(error)
        || rpc_failure(error).is_none_or(|failure| failure.method == "initialize")
}

fn normalize_model(model: &Value) -> Option<Value> {
    if model.get("hidden").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let model_name = model
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| model.get("id").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())?;
    let supported = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some(json!({
        "id": model.get("id").and_then(Value::as_str).unwrap_or(model_name),
        "model": model_name,
        "displayName": model.get("displayName").and_then(Value::as_str).unwrap_or(model_name),
        "supportedReasoningEfforts": supported,
        "defaultReasoningEffort": model.get("defaultReasoningEffort").cloned().unwrap_or(Value::Null)
    }))
}

fn service_projects(recent: &mut [Value]) -> Vec<Value> {
    let mut projects: std::collections::HashMap<String, (String, Vec<Value>)> =
        std::collections::HashMap::new();
    for thread in recent.iter_mut() {
        let Some(cwd) = thread
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        let normalized = cwd.trim_end_matches(['/', '\\']);
        let key = if cfg!(windows) {
            normalized.to_ascii_lowercase()
        } else {
            normalized.to_owned()
        };
        if key.is_empty() {
            continue;
        }
        let project_id = format!("cwd:{key}");
        thread["projectId"] = json!(&project_id);
        projects
            .entry(project_id)
            .or_insert_with(|| (cwd.clone(), Vec::new()))
            .1
            .push(thread.clone());
    }
    let mut projects = projects
        .into_iter()
        .map(|(id, (cwd, threads))| {
            let recency = threads
                .iter()
                .filter_map(|thread| thread.get("recencyAt").and_then(Value::as_u64))
                .max()
                .unwrap_or(0);
            let name = std::path::Path::new(&cwd)
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(cwd.as_str())
                .to_owned();
            json!({"id":id,"name":name,"cwd":cwd,"roots":[cwd],"pinned":false,
            "recencyAt":recency,"threads":threads})
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        right["recencyAt"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&left["recencyAt"].as_u64().unwrap_or(0))
            .then_with(|| {
                left["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(right["name"].as_str().unwrap_or(""))
            })
    });
    projects
}

fn required_service_thread_id(payload: &Value) -> anyhow::Result<&str> {
    payload
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("缺少会话 ID")
}

fn service_history_items_page(page: &Value) -> (Vec<Value>, Option<String>) {
    let mut items = page
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| entry.get("item").cloned().unwrap_or_else(|| entry.clone()))
        .collect::<Vec<_>>();
    items.reverse();
    let next_cursor = page
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    (items, next_cursor)
}

fn normalize_history_items_page(
    page: &Value,
    previous_cursor: Option<&str>,
) -> anyhow::Result<(Vec<Value>, Option<String>)> {
    let (items, next_cursor) = service_history_items_page(page);
    // 保留主线的分页防循环检查；历史项顺序和消息原始字段仍由新版解析逻辑处理。
    anyhow::ensure!(
        previous_cursor.is_none() || next_cursor.as_deref() != previous_cursor,
        "历史分页游标未前进"
    );
    Ok((items, next_cursor))
}

fn owner_snapshot_history(snapshot: &Value, thread_id: &str) -> Option<Value> {
    let complete = if snapshot["turnHistory"]["kind"] == "canonical" {
        snapshot["turnHistory"]["history"]["isComplete"] == true
    } else {
        snapshot["turnsPagination"]["hasLoadedOldest"] == true
    };
    let mut turns = desktop_owner_ipc::snapshot_turns(snapshot);
    if !complete
        || turns.iter().any(|turn| {
            turn.get("itemsPagination").is_some_and(|pagination| {
                pagination["hasLoadedOldest"] != true || pagination["isLoadingOlder"] == true
            })
        })
    {
        return None;
    }
    turns.retain(|turn| turn["turnId"].as_str().is_some_and(|id| !id.is_empty()));
    turns.sort_by(|left, right| {
        right["turnStartedAtMs"]
            .as_u64()
            .cmp(&left["turnStartedAtMs"].as_u64())
            .then_with(|| right["turnId"].as_str().cmp(&left["turnId"].as_str()))
    });
    let mut pagination = serde_json::Map::new();
    for turn in &mut turns {
        let id = turn["turnId"].as_str()?.to_string();
        turn["id"] = json!(&id);
        pagination.insert(
            id,
            json!({"complete":true,"pageComplete":true,"hasMore":false,
                "nextCursor":null,"direction":"older","order":"asc"}),
        );
    }
    Some(json!({
        "ok":true,"scope":"turns","threadId":thread_id,"data":turns,
        "nextCursor":null,"backwardsCursor":null,"turnOrder":"desc","itemOrder":"asc",
        "turnsPageComplete":true,"turnsComplete":true,"itemsPaginationByTurnId":pagination,
        "transcriptEvents":[],"nativeBridge":"desktop-owner-ipc"
    }))
}

async fn owner_or_cli<F, Fut>(owner: OwnerSend, fallback: F) -> anyhow::Result<Value>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Value>>,
{
    match owner {
        OwnerSend::Handled(response) => Ok(response),
        OwnerSend::NoOwner => fallback().await,
        OwnerSend::AuthorizationDenied(error) => Err(error),
    }
}

struct ServiceOwnerPreflight<'a> {
    service: &'a CodexCliService,
}

#[async_trait]
impl desktop_owner_ipc::OwnerPreflight for ServiceOwnerPreflight<'_> {
    async fn check_queues(&self, thread_id: &str, snapshot: &Value) -> anyhow::Result<()> {
        self.service.follow_owner(thread_id, snapshot).await;
        let settings = crate::settings::SettingsStore::default().load()?;
        let home = crate::codex_home::codex_home_dir_for_settings(&settings);
        require_empty_local_queue(&home, thread_id, snapshot).await?;
        // This is read-only; never resume the thread in the independent CLI.
        // The server queue and desktop coordinator queue are distinct sources.
        let page = self
            .service
            .request(
                "thread/queue/list",
                json!({"threadId":thread_id,"cursor":null}),
            )
            .await
            .context("无法读取服务端队列，未发送")?;
        ensure_empty_server_queue(&page)
    }
}

fn ensure_empty_server_queue(page: &Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        page["data"].as_array().is_some_and(Vec::is_empty) && page["nextCursor"].is_null(),
        "会话存在服务端队列消息或队列状态未知，未发送"
    );
    Ok(())
}

async fn require_empty_local_queue(
    home: &Path,
    thread_id: &str,
    snapshot: &Value,
) -> anyhow::Result<()> {
    let rollout = snapshot["rolloutPath"]
        .as_str()
        .context("桌面会话数据目录未知，无法核对本地队列")?;
    let sessions = tokio::fs::canonicalize(home.join("sessions")).await?;
    let rollout = tokio::fs::canonicalize(rollout).await?;
    anyhow::ensure!(
        rollout.starts_with(&sessions),
        "桌面与后台使用不同的会话数据目录，未发送"
    );
    let state_path = home.join(".codex-global-state.json");
    anyhow::ensure!(
        tokio::fs::metadata(&state_path).await?.len() <= 16 * 1024 * 1024,
        "桌面本地队列状态文件超过大小限制"
    );
    let state: Value = serde_json::from_slice(&tokio::fs::read(&state_path).await?)
        .context("桌面本地队列状态无法解析")?;
    let queue = state.get("queued-follow-ups");
    anyhow::ensure!(
        state.is_object() && queue.is_none_or(Value::is_object),
        "桌面本地队列状态格式未知"
    );
    let messages = queue.and_then(|queue| queue.get(thread_id));
    anyhow::ensure!(
        messages.is_none_or(|messages| messages.as_array().is_some_and(Vec::is_empty)),
        "会话存在桌面队列消息或本地队列状态未知，未发送"
    );
    Ok(())
}

fn service_turn_start_params(payload: &Value, thread_id: &str) -> anyhow::Result<Value> {
    if payload.get("hostId").is_some_and(|host| host != "local") {
        bail!("此发送兜底仅支持本机主会话");
    }
    let prompt = payload
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("消息不能为空")?;
    if payload.get("queueOnly").and_then(Value::as_bool) == Some(true) {
        bail!("CLI 兜底暂不支持原生队列写入");
    }
    if payload
        .get("images")
        .and_then(Value::as_array)
        .is_some_and(|images| !images.is_empty())
    {
        bail!("CLI 兜底暂不支持图片消息");
    }
    // 用户要求发送兜底保留原始语义；纯文本通道不能静默丢弃 Skill 或把目标当普通消息发送。
    if payload
        .get("skills")
        .and_then(Value::as_array)
        .is_some_and(|skills| !skills.is_empty())
        || payload.get("goalMode").and_then(Value::as_bool) == Some(true)
    {
        bail!("CLI 兜底仅支持普通纯文本消息，暂不支持 Skill 或目标消息");
    }
    let mut params = json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": prompt}],
        "clientUserMessageId": payload.get("clientUserMessageId")
            .and_then(Value::as_str).filter(|id| !id.is_empty())
            .map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    });
    for (source, target) in [("model", "model"), ("reasoningEffort", "effort")] {
        if let Some(value) = payload
            .get(source)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            params[target] = json!(value);
        }
    }
    Ok(params)
}

fn service_write_error(thread_id: &str, error: anyhow::Error) -> Value {
    let rpc_error = error.downcast_ref::<RpcFailure>();
    let not_sent = error
        .downcast_ref::<desktop_owner_ipc::DispatchFailure>()
        .map_or(rpc_error.is_some(), |failure| !failure.dispatched);
    json!({
        "ok": false,
        "threadId": thread_id,
        "notSent": not_sent,
        "outcomeUnknown": !not_sent,
        "code": 503,
        "message": format!("{error:#}"),
        "nativeBridge": "service"
    })
}

fn service_write_failure(thread_id: &str, error: anyhow::Error) -> anyhow::Result<Value> {
    if error
        .downcast_ref::<desktop_owner_ipc::DispatchFailure>()
        .is_some_and(|failure| failure.authorization_denied)
    {
        Err(error)
    } else {
        Ok(service_write_error(thread_id, error))
    }
}

fn service_not_sent_error(thread_id: &str, message: &str) -> Value {
    json!({
        "ok":false,"threadId":thread_id,"notSent":true,"outcomeUnknown":false,
        "code":409,"message":message,"nativeBridge":"service"
    })
}

fn normalize_thread_response(response: &Value) -> anyhow::Result<Value> {
    let thread = response
        .get("thread")
        .context("Codex CLI service 未返回会话元数据")?;
    let mut normalized = normalize_thread(thread)?;
    if let Some(read_state) = response.get("readState") {
        normalized["readState"] = read_state.clone();
        if let Some(first_unread) = read_state.get("firstUnread") {
            normalized["hasUnreadTurn"] = json!(!first_unread.is_null());
        }
    }
    Ok(normalized)
}

fn normalize_owner_thread(snapshot: &Value, metadata: Option<&Value>) -> anyhow::Result<Value> {
    let mut thread = snapshot.clone();
    thread["status"] = snapshot["threadRuntimeStatus"].clone();
    thread["model"] = snapshot["latestModel"].clone();
    thread["reasoningEffort"] = snapshot["latestReasoningEffort"].clone();
    if ["name", "title", "preview"].iter().all(|key| {
        snapshot[*key]
            .as_str()
            .is_none_or(|text| text.trim().is_empty())
    }) {
        if let Some(title) = metadata
            .and_then(|thread| thread.get("title"))
            .filter(|value| value.as_str().is_some_and(|title| !title.trim().is_empty()))
        {
            thread["title"] = title.clone();
        }
    }
    let mut normalized = normalize_thread(&thread)?;
    if let Some(metadata) = metadata {
        for field in ["createdAt", "updatedAt", "recencyAt"] {
            if normalized[field].is_null() {
                normalized[field] = metadata[field].clone();
            }
        }
        if let Some(read_state) = metadata.get("readState") {
            normalized["readState"] = read_state.clone();
        }
        if normalized["hasUnreadTurn"].is_null() {
            normalized["hasUnreadTurn"] = metadata["hasUnreadTurn"].clone();
        }
    }
    normalized["requests"] = snapshot["requests"].clone();
    normalized["requestsKnown"] = json!(snapshot["requests"].is_array());
    normalized["goal"] = snapshot["threadGoal"].clone();
    normalized["tokenUsage"] = snapshot["latestTokenUsageInfo"].clone();
    Ok(normalized)
}

fn normalize_thread(thread: &Value) -> anyhow::Result<Value> {
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Codex CLI service 返回了无效会话 ID")?;
    let ephemeral = thread
        .get("ephemeral")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let parent_thread_id = thread
        .get("parentThreadId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let thread_source = thread
        .get("threadSource")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let source_is_subagent = thread
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subAgent") || source.contains_key("subagent"));
    if ephemeral
        || parent_thread_id.is_some()
        || source_is_subagent
        || thread_source.is_some_and(|source| source.eq_ignore_ascii_case("subagent"))
        || thread.get("sideConversation") == Some(&Value::Bool(true))
    {
        bail!("Codex CLI service 只提供主会话元数据");
    }
    let cwd = thread
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Codex CLI service 返回了无效会话工作目录")?;
    let title = thread
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            thread
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| {
            thread
                .get("preview")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or("新会话");
    let status = thread.get("status").cloned().unwrap_or(Value::Null);
    let status_type = status.as_str().or_else(|| status["type"].as_str());
    let active = match status_type {
        Some("active" | "inProgress" | "running") => Some(true),
        Some("idle") => Some(false),
        _ => None,
    };
    let unread = thread.get("hasUnreadTurn").and_then(Value::as_bool);
    Ok(json!({
        "id": id,
        "title": title,
        "cwd": cwd,
        "createdAt": thread.get("createdAt").cloned().unwrap_or(Value::Null),
        "updatedAt": thread.get("updatedAt").cloned().unwrap_or(Value::Null),
        "recencyAt": thread.get("recencyAt").cloned().unwrap_or_else(|| thread.get("updatedAt").cloned().unwrap_or(Value::Null)),
        "model": thread.get("model").cloned().unwrap_or(Value::Null),
        "reasoningEffort": thread.get("reasoningEffort").cloned().unwrap_or(Value::Null),
        "status": status.clone(),
        "priority": {
            "status": status,
            "attentionState": match active { Some(true) => "active", Some(false) => "idle", None => "unknown" },
            "active": active,
            "eligible": true
        },
        "hasUnreadTurn": unread,
        "readOnly": false,
        "viewKind": "main",
        "parentThreadId": Value::Null,
        "ephemeral": thread.get("ephemeral").cloned().unwrap_or(Value::Null),
        "threadSource": thread_source,
        "source": thread.get("source").cloned().unwrap_or(Value::Null),
        "requests": thread.get("requests").cloned().unwrap_or(Value::Null),
        "requestsKnown": thread.get("requests").is_some_and(Value::is_array),
        "queuedMessages": Value::Null,
        "queuedEdits": Value::Null,
        "transcriptEvents": []
    }))
}

fn truncate_error(value: &str) -> String {
    const MAX_ERROR_CHARS: usize = 1_000;
    value.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_send_params_resume_unloaded_main_thread() {
        let payload = json!({
            "threadId": "thread-1",
            "prompt": "测试消息",
            "model": "gpt-6-astra",
            "reasoningEffort": "high"
        });
        let params = service_turn_start_params(&payload, "thread-1").unwrap();
        assert_eq!(params["threadId"], "thread-1");
        assert_eq!(params["input"][0]["text"], "测试消息");
        assert_eq!(params["model"], "gpt-6-astra");
        assert_eq!(params["effort"], "high");
        assert!(
            service_turn_start_params(
                &json!({"threadId":"thread-1","prompt":"x","queueOnly":true}),
                "thread-1"
            )
            .is_err()
        );
        for extra in [
            json!({"skills":[{"name":"fixture","path":"fixture/SKILL.md"}]}),
            json!({"goalMode":true}),
            json!({"images":[{"dataUrl":"data:image/png;base64,fixture"}]}),
        ] {
            let mut unsupported = json!({"threadId":"thread-1","prompt":"x"});
            unsupported
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(service_turn_start_params(&unsupported, "thread-1").is_err());
        }
    }

    #[tokio::test]
    async fn handled_owner_results_never_start_the_independent_writer_fallback() {
        for response in [
            json!({"ok":true,"turnId":"same-owner-turn"}),
            json!({"ok":false,"notSent":false,"outcomeUnknown":true}),
            json!({"ok":false,"notSent":true,"outcomeUnknown":false}),
        ] {
            let expected = response.clone();
            let result = owner_or_cli(OwnerSend::Handled(response), || async {
                panic!("an owner result must never launch a second writer")
            })
            .await
            .unwrap();
            assert_eq!(result, expected);
        }
        assert_eq!(
            owner_or_cli(OwnerSend::NoOwner, || async {
                Ok(json!({"fallback":true}))
            })
            .await
            .unwrap()["fallback"],
            true
        );
    }

    #[test]
    fn raw_thread_metadata_preserves_preview_object_status_and_outer_read_state() {
        let raw = json!({
            "thread":{"id":"main","name":null,"preview":"真实首条消息",
                "cwd":"C:/workspace","source":"vscode","threadSource":"user",
                "ephemeral":false,"parentThreadId":null,"status":{"type":"active"}},
            "readState":{"firstUnread":{"turnId":"unread-turn"},"revision":21}
        });
        let normalized = normalize_thread_response(&raw).unwrap();
        assert_eq!(normalized["title"], "真实首条消息");
        assert_eq!(normalized["priority"]["active"], true);
        assert_eq!(normalized["hasUnreadTurn"], true);
        assert_eq!(normalized["readState"], raw["readState"]);
        let mut unloaded = raw;
        unloaded["thread"]["status"]["type"] = json!("notLoaded");
        unloaded.as_object_mut().unwrap().remove("readState");
        let unknown = normalize_thread_response(&unloaded).unwrap();
        assert!(unknown["priority"]["active"].is_null());
        assert_eq!(unknown["priority"]["attentionState"], "unknown");
        assert!(unknown["hasUnreadTurn"].is_null());
    }

    #[test]
    fn nested_subagent_metadata_and_wrong_host_writes_are_rejected() {
        let raw = json!({
            "id":"child","cwd":"C:/workspace","ephemeral":false,
            "source":{"subAgent":{"thread_spawn":{"parent_thread_id":"parent"}}}
        });
        assert!(normalize_thread(&raw).is_err());
        assert!(
            service_turn_start_params(
                &json!({"threadId":"main","hostId":"remote","prompt":"x"}),
                "main"
            )
            .is_err()
        );
        assert!(ensure_empty_server_queue(&json!({"data":[],"nextCursor":null})).is_ok());
        for page in [
            json!({}),
            json!({"data":[{"id":"queued"}]}),
            json!({"data":[],"nextCursor":"more"}),
        ] {
            assert!(ensure_empty_server_queue(&page).is_err());
        }
    }

    #[test]
    fn service_history_items_page_restores_order_and_preserves_payloads_and_cursor() {
        let user = json!({
            "type":"userMessage","id":"user-1","clientId":"client-1",
            "content":[{"type":"text","text":"测试消息","text_elements":[]}]
        });
        let reasoning = json!({
            "type":"reasoning","id":"reasoning-1","summary":[],"content":[]
        });
        let assistant = json!({
            "type":"agentMessage","id":"assistant-1","text":"完整回复",
            "phase":"final_answer","memoryCitation":null,"delivery":null,"questions":null
        });
        let expected = vec![user.clone(), reasoning.clone(), assistant.clone()];
        let cursor = r#"{"requestedThreadId":"main","rolloutOrdinal":30,"scope":{"kind":"items","turnId":"turn-1"}}"#;
        for page in [
            json!({
                "data":[
                    {"item":assistant,"turnId":"turn-1"},
                    {"item":reasoning,"turnId":"turn-1"},
                    {"item":user,"turnId":"turn-1"}
                ],
                "nextCursor":cursor
            }),
            json!({"data":[assistant,reasoning,user],"nextCursor":cursor}),
        ] {
            let (items, next_cursor) = service_history_items_page(&page);
            assert_eq!(items, expected);
            assert_eq!(next_cursor.as_deref(), Some(cursor));
        }
    }

    #[test]
    fn owner_snapshot_history_only_claims_complete_for_complete_items_and_keeps_turn_identity() {
        let snapshot = json!({"turnHistory":{"kind":"canonical","history":{
            "isComplete":true,"entitiesByKey":{
                "tail":{"turnId":"accepted-turn","status":"completed","turnStartedAtMs":1000,
                    "params":{"clientUserMessageId":"stable-message"},
                    "items":[{"type":"agentMessage","text":"done"}]}
            }
        }}});
        let history = owner_snapshot_history(&snapshot, "main").unwrap();
        assert_eq!(history["data"][0]["id"], "accepted-turn");
        assert_eq!(
            history["data"][0]["params"]["clientUserMessageId"],
            "stable-message"
        );
        assert_eq!(history["turnsComplete"], true);
        let mut partial = snapshot;
        partial["turnHistory"]["history"]["entitiesByKey"]["tail"]["itemsPagination"] =
            json!({"hasLoadedOldest":false});
        assert!(owner_snapshot_history(&partial, "main").is_none());
    }
    #[test]
    fn normalize_model_keeps_ultra_and_drops_hidden_entries() {
        let model = normalize_model(&json!({
            "id": "gpt-6-astra",
            "model": "gpt-6-astra",
            "displayName": "GPT-6 Astra",
            "hidden": false,
            "supportedReasoningEfforts": [
                {"reasoningEffort": "medium"},
                {"reasoningEffort": "ultra"}
            ],
            "defaultReasoningEffort": "medium"
        }))
        .unwrap();
        assert_eq!(model["model"], "gpt-6-astra");
        assert_eq!(
            model["supportedReasoningEfforts"][1]["reasoningEffort"],
            "ultra"
        );
        assert!(normalize_model(&json!({"model":"hidden","hidden":true})).is_none());
    }

    #[tokio::test]
    async fn queue_checks_keep_local_and_server_queue_evidence_separate() {
        let home = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(home.path().join("sessions"))
            .await
            .unwrap();
        let rollout = home.path().join("sessions/main.jsonl");
        tokio::fs::write(&rollout, b"{}").await.unwrap();
        let state_path = home.path().join(".codex-global-state.json");
        tokio::fs::write(&state_path, br#"{"queued-follow-ups":{}}"#)
            .await
            .unwrap();
        let snapshot = json!({"rolloutPath":rollout.to_string_lossy()});
        require_empty_local_queue(home.path(), "main", &snapshot)
            .await
            .unwrap();
        // A missing local key is insufficient: the positively observed server
        // queue has its own rows, independently of the desktop JSON file.
        assert!(
            ensure_empty_server_queue(&json!({"data":[
            {"id":"server-P","clientUserMessageId":"client-L"}],"nextCursor":null}))
            .is_err()
        );
        tokio::fs::write(
            &state_path,
            br#"{"queued-follow-ups":{"main":[{"id":"local-Q"}]}}"#,
        )
        .await
        .unwrap();
        assert!(
            require_empty_local_queue(home.path(), "main", &snapshot)
                .await
                .is_err()
        );
        assert!(
            require_empty_local_queue(
                home.path(),
                "main",
                &json!({"rolloutPath":home.path().join("outside.jsonl").to_string_lossy()})
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn normalize_model_uses_id_when_model_name_is_missing() {
        let model = normalize_model(&json!({
            "id": "gpt-fallback",
            "supportedReasoningEfforts": []
        }))
        .unwrap();
        assert_eq!(model["model"], "gpt-fallback");
        assert_eq!(model["displayName"], "gpt-fallback");
    }

    #[test]
    fn normalize_thread_keeps_main_session_metadata_and_rejects_ephemeral_sessions() {
        let thread = normalize_thread(&json!({
            "id": "thread-1",
            "name": "主会话",
            "cwd": "C:/workspace",
            "createdAt": 1,
            "updatedAt": 2,
            "status": "idle",
            "ephemeral": false
        }))
        .unwrap();
        assert_eq!(thread["id"], "thread-1");
        assert_eq!(thread["title"], "主会话");
        assert_eq!(thread["viewKind"], "main");
        assert!(
            normalize_thread(&json!({
                "id": "side-1",
                "cwd": "C:/workspace",
                "ephemeral": true
            }))
            .is_err()
        );
    }

    #[test]
    fn service_only_reads_are_explicitly_degraded() {
        let projection = ServiceProjection::new();
        let events = projection.events(&json!({}));
        let notifications = projection.notifications(&json!({}));
        assert_eq!(events["nativeBridge"], "service");
        assert_eq!(events["events"], json!([]));
        assert_eq!(notifications.as_object().unwrap().len(), 8);
        assert_eq!(notifications["pending"], json!([]));
        assert_eq!(events["reset"], true);
        assert_eq!(notifications["reset"], true);
    }

    fn projection_snapshot(status: &str, turns: Vec<Value>) -> Value {
        let turns: serde_json::Map<String, Value> = turns
            .into_iter()
            .map(|turn| (turn["turnId"].as_str().unwrap().to_string(), turn))
            .collect();
        json!({
            "id":"main","sessionId":"main","hostId":"local","cwd":"C:/workspace",
            "source":"vscode","threadSource":"user","ephemeral":false,
            "sideConversation":false,"parentThreadId":null,"forkedFromId":null,
            "resumeState":"resumed","threadRuntimeStatus":{"type":status},
            "title":null,"requests":[],"threadGoal":null,
            "latestModel":"gpt-6-astra","latestReasoningEffort":"high",
            "latestTokenUsageInfo":null,
            "turnHistory":{"kind":"canonical","history":{
                "isComplete":true,"entitiesByKey":turns
            }}
        })
    }

    fn projection_turn(id: &str, status: &str, started: u64, text: &str) -> Value {
        json!({
            "turnId":id,"status":status,"turnStartedAtMs":started,
            "params":{"clientUserMessageId":format!("client-{id}")},
            "items":[{"id":format!("item-{id}"),"type":"agentMessage","text":text}]
        })
    }

    #[test]
    fn owner_projection_seeds_without_replaying_history_then_projects_real_turn_changes() {
        let mut projection = ServiceProjection::new();
        projection.remember_metadata("main", json!({"id":"main","title":"真实会话"}));
        let initial = projection_snapshot(
            "idle",
            vec![projection_turn("old", "completed", 100, "历史回复")],
        );
        let generation = projection.begin_subscription("main");
        projection.observe("main", generation, "owner", 1, initial.clone());
        assert_eq!(projection.sequence, 0);
        assert_eq!(projection.notification_sequence, 0);

        let mut active = initial.clone();
        active["threadRuntimeStatus"] = json!({"type":"active"});
        active["turnHistory"]["history"]["entitiesByKey"]["new"] =
            projection_turn("new", "inProgress", 200, "");
        projection.observe("main", generation, "owner", 2, active.clone());
        let mut streaming = active.clone();
        streaming["turnHistory"]["history"]["entitiesByKey"]["new"]["items"][0]["text"] =
            json!("真实增量");
        projection.observe("main", generation, "owner", 3, streaming.clone());
        let mut completed = streaming;
        completed["threadRuntimeStatus"] = json!({"type":"idle"});
        completed["turnHistory"]["history"]["entitiesByKey"]["new"] =
            projection_turn("new", "completed", 200, "最终回复");
        projection.observe("main", generation, "owner", 4, completed.clone());

        let response = projection.events(&json!({"epoch":projection.epoch,"cursor":0}));
        let events = response["events"].as_array().unwrap();
        assert!(events.iter().any(|event| event["method"] == "item/updated"
            && event["params"]["item"]["text"] == "真实增量"));
        let terminal: Vec<_> = events
            .iter()
            .filter(|event| event["method"] == "turn/completed")
            .collect();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0]["params"]["turnId"], "new");
        assert_eq!(
            terminal[0]["params"]["turn"]["items"][0]["text"],
            "最终回复"
        );
        assert_eq!(
            terminal[0]["params"]["turn"]["params"]["clientUserMessageId"],
            "client-new"
        );
        assert!(
            events
                .iter()
                .any(|event| event["method"] == "thread/status/changed"
                    && event["params"]["status"]["type"] == "idle")
        );

        let notifications = projection.notifications(&json!({"epoch":projection.epoch,"cursor":0}));
        assert_eq!(notifications.as_object().unwrap().len(), 8);
        assert_eq!(notifications["events"].as_array().unwrap().len(), 1);
        assert_eq!(notifications["events"][0].as_object().unwrap().len(), 9);
        assert_eq!(notifications["events"][0]["kind"], "completed");
        assert_eq!(notifications["events"][0]["title"], "真实会话");
        let last_sequence = projection.sequence;
        projection.observe("main", generation, "owner", 4, completed);
        projection.observe("main", generation, "owner", 2, active);
        assert_eq!(projection.sequence, last_sequence);
        assert_eq!(projection.notification_sequence, 1);
    }

    #[test]
    fn owner_projection_keeps_active_read_baseline_before_first_completed_push() {
        let mut projection = ServiceProjection::new();
        let active = projection_snapshot(
            "active",
            vec![projection_turn("fast", "inProgress", 100, "")],
        );
        let generation = projection.subscribe_from("main", &active);
        let completed = projection_snapshot(
            "idle",
            vec![projection_turn("fast", "completed", 100, "快速完成")],
        );
        projection.observe("main", generation, "owner", 1, completed.clone());
        assert_eq!(projection.notification_sequence, 1);
        assert!(
            projection
                .events
                .iter()
                .any(|(_, event)| event["method"] == "turn/completed"
                    && event["params"]["turn"]["items"][0]["text"] == "快速完成")
        );
        // A delayed read cannot overwrite a newer streamed observation.
        let replacement = projection.subscribe_from("main", &active);
        projection.observe("main", replacement, "owner", 1, completed.clone());
        assert_eq!(projection.threads["main"].snapshot, completed);
        assert_eq!(projection.notification_sequence, 1);
    }

    #[test]
    fn owner_tool_lifecycle_is_identical_in_history_and_live_events() {
        let mut projection = ServiceProjection::new();
        let idle = projection_snapshot("idle", vec![]);
        let generation = projection.subscribe_from("main", &idle);
        projection.observe("main", generation, "owner", 1, idle);
        let tool = json!({
            "id":"sleep-tool","type":"sleep","durationMs":15000,"startedAtMs":1000
        });
        let mut turn = projection_turn("live", "inProgress", 500, "");
        turn["completedItemIds"] = json!({});
        turn["items"] = json!([tool]);
        let active = projection_snapshot("active", vec![turn.clone()]);
        projection.observe("main", generation, "owner", 2, active.clone());
        let started = projection
            .events
            .iter()
            .find(|(_, event)| event["method"] == "turn/started")
            .unwrap()
            .1
            .clone();
        let active_history = owner_snapshot_history(&active, "main").unwrap();
        assert_eq!(
            active_history["data"][0]["items"],
            started["params"]["turn"]["items"]
        );
        assert_eq!(
            active_history["data"][0]["items"][0]["status"],
            "inProgress"
        );
        assert_eq!(
            active_history["data"][0]["items"][0]["remoteItemCompleted"],
            false
        );
        assert_eq!(projection.notification_sequence, 0);

        turn["completedItemIds"]["sleep-tool"] = json!(true);
        turn["items"][0]["completedAtMs"] = json!(16000);
        let tool_done = projection_snapshot("active", vec![turn.clone()]);
        projection.observe("main", generation, "owner", 3, tool_done.clone());
        let update = projection
            .events
            .iter()
            .rev()
            .find(|(_, event)| event["method"] == "item/updated")
            .unwrap()
            .1
            .clone();
        let done_history = owner_snapshot_history(&tool_done, "main").unwrap();
        assert_eq!(
            done_history["data"][0]["items"][0],
            update["params"]["item"]
        );
        assert_eq!(update["params"]["item"]["status"], "completed");
        assert_eq!(update["params"]["item"]["completedAtMs"], 16000);
        assert_eq!(projection.notification_sequence, 0);
        assert!(
            !projection
                .events
                .iter()
                .any(|(_, event)| event["method"] == "turn/completed")
        );

        turn["status"] = json!("completed");
        let finished = projection_snapshot("idle", vec![turn]);
        projection.observe("main", generation, "owner", 4, finished.clone());
        let terminal = projection
            .events
            .iter()
            .find(|(_, event)| event["method"] == "turn/completed")
            .unwrap()
            .1
            .clone();
        let final_history = owner_snapshot_history(&finished, "main").unwrap();
        assert_eq!(
            final_history["data"][0]["items"],
            terminal["params"]["turn"]["items"]
        );
        assert_eq!(projection.notification_sequence, 1);
    }

    #[test]
    fn owner_projection_uses_stream_generations_when_same_owner_revision_restarts() {
        let mut projection = ServiceProjection::new();
        let active = projection_snapshot(
            "active",
            vec![projection_turn("live", "inProgress", 100, "开始")],
        );
        let old_generation = projection.subscribe_from("main", &active);
        projection.observe("main", old_generation, "owner", 10, active.clone());
        projection.owner_unavailable("main", old_generation);
        let new_generation = projection.subscribe_from("main", &active);
        let completed = projection_snapshot(
            "idle",
            vec![projection_turn("live", "completed", 100, "完成")],
        );
        projection.observe("main", new_generation, "owner", 1, completed.clone());
        assert!(projection.threads["main"].connected);
        assert_eq!(projection.threads["main"].revision, 1);
        assert_eq!(projection.notification_sequence, 1);
        let sequence = projection.sequence;
        projection.observe("main", old_generation, "owner", 11, active);
        projection.owner_unavailable("main", old_generation);
        assert_eq!(projection.threads["main"].snapshot, completed);
        assert!(projection.threads["main"].connected);
        assert_eq!(projection.sequence, sequence);
    }

    #[test]
    fn owner_projection_resyncs_owner_changes_without_historical_completion_notifications() {
        let mut projection = ServiceProjection::new();
        let initial = projection_snapshot("idle", vec![]);
        let generation = projection.subscribe_from("main", &initial);
        projection.observe("main", generation, "owner-a", 1, initial);
        let replacement = projection.begin_subscription("main");
        let historical = projection_snapshot(
            "idle",
            vec![projection_turn("old", "completed", 100, "历史")],
        );
        projection.observe("main", replacement, "owner-b", 1, historical);
        assert_eq!(
            projection.events.back().unwrap().1["method"],
            "remote/resync"
        );
        assert_eq!(projection.notification_sequence, 0);
    }

    #[test]
    fn owner_projection_does_not_notify_for_paginated_history_or_infer_terminal_from_idle() {
        let mut projection = ServiceProjection::new();
        let mut partial = projection_snapshot(
            "idle",
            vec![projection_turn("latest", "completed", 200, "已存在")],
        );
        partial["turnHistory"]["history"]["isComplete"] = json!(false);
        let generation = projection.subscribe_from("main", &partial);
        projection.observe("main", generation, "owner", 1, partial.clone());
        partial["turnHistory"]["history"]["entitiesByKey"]["older"] =
            projection_turn("older", "completed", 100, "分页加载");
        projection.observe("main", generation, "owner", 2, partial);
        assert_eq!(projection.notification_sequence, 0);
        assert_eq!(projection.sequence, 0);

        let mut empty_partial = projection_snapshot("active", vec![]);
        empty_partial["turnHistory"]["history"]["isComplete"] = json!(false);
        let generation = projection.subscribe_from("empty", &empty_partial);
        projection.observe("empty", generation, "owner", 1, empty_partial.clone());
        empty_partial["threadRuntimeStatus"] = json!({"type":"idle"});
        empty_partial["turnHistory"]["history"]["entitiesByKey"]["older"] =
            projection_turn("older", "completed", 100, "补载历史");
        projection.observe("empty", generation, "owner", 2, empty_partial);
        assert_eq!(projection.notification_sequence, 0);
        assert!(
            !projection
                .events
                .iter()
                .any(|(_, event)| event["method"] == "turn/completed")
        );
    }

    #[test]
    fn owner_projection_resets_stale_cursors_and_bounds_journals() {
        let mut projection = ServiceProjection::new();
        for _ in 0..=MAX_SERVICE_EVENTS {
            projection.push(json!({"method":"remote/resync"}));
        }
        assert_eq!(projection.events.len(), MAX_SERVICE_EVENTS);
        assert_eq!(
            projection.events(&json!({"epoch":projection.epoch,"cursor":0}))["reset"],
            true
        );
        assert_eq!(
            projection.events(&json!({"epoch":"old","cursor":0}))["reset"],
            true
        );
        assert_eq!(
            projection.events(&json!({"epoch":projection.epoch,"cursor":u64::MAX}))["reset"],
            true
        );
        let tail =
            projection.events(&json!({"epoch":projection.epoch,"cursor":projection.sequence-1}));
        assert_eq!(tail["reset"], false);
        assert_eq!(tail["events"].as_array().unwrap().len(), 1);
        for _ in 0..=MAX_SERVICE_NOTIFICATIONS {
            projection.notify_terminal(
                "main",
                &projection_turn("done", "completed", 100, "真实完成状态"),
            );
        }
        assert_eq!(projection.notifications.len(), MAX_SERVICE_NOTIFICATIONS);
        let reset = projection.notifications(&json!({"epoch":projection.epoch,"cursor":0}));
        assert_eq!(reset["reset"], true);
        assert_eq!(reset["events"], json!([]));
        let tail = projection.notifications(&json!({
            "epoch":projection.epoch,"cursor":projection.notification_sequence-1
        }));
        assert_eq!(tail["reset"], false);
        assert_eq!(tail["events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn owner_metadata_keeps_real_title_and_read_state_without_overriding_live_status() {
        let metadata = normalize_thread_response(&json!({
            "thread":{"id":"main","name":null,"preview":"正确标题","cwd":"C:/workspace",
                "status":{"type":"notLoaded"}},
            "readState":{"firstUnread":{"turnId":"unread"},"revision":2}
        }))
        .unwrap();
        let snapshot = projection_snapshot("active", vec![]);
        let normalized = normalize_owner_thread(&snapshot, Some(&metadata)).unwrap();
        assert_eq!(normalized["title"], "正确标题");
        assert_eq!(normalized["readState"], metadata["readState"]);
        assert_eq!(normalized["hasUnreadTurn"], true);
        assert_eq!(normalized["status"]["type"], "active");
        assert_eq!(normalized["priority"]["active"], true);
        assert_eq!(normalized["model"], "gpt-6-astra");
        let mut named = snapshot;
        named["title"] = json!("owner 的新标题");
        assert_eq!(
            normalize_owner_thread(&named, Some(&metadata)).unwrap()["title"],
            "owner 的新标题"
        );
        let mut projection = ServiceProjection::new();
        projection.remember_metadata("main", metadata.clone());
        projection.remember_metadata(
            "main",
            json!({"id":"main","title":"列表标题","hasUnreadTurn":null}),
        );
        assert_eq!(
            projection.metadata["main"]["readState"],
            metadata["readState"]
        );
        assert_eq!(projection.metadata["main"]["hasUnreadTurn"], true);
    }

    #[tokio::test]
    async fn dropping_service_aborts_only_its_owner_watcher_tasks() {
        let service = CodexCliService::new();
        let (dropped, signal) = tokio::sync::oneshot::channel();
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let ready = Arc::new(tokio::sync::Notify::new());
        let task_ready = Arc::clone(&ready);
        let task = tokio::spawn(async move {
            let _signal = DropSignal(Some(dropped));
            task_ready.notify_one();
            std::future::pending::<()>().await;
        });
        ready.notified().await;
        service
            .owner_watchers
            .lock()
            .await
            .push_back(("main".to_string(), task));
        drop(service);
        tokio::time::timeout(Duration::from_secs(1), signal)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn unsupported_goal_rpc_is_optional_and_keeps_service_available() {
        let error = anyhow::Error::new(RpcFailure {
            method: "thread/goal/get".to_string(),
            code: -32600,
            message: "goals feature is disabled".to_string(),
        });
        assert!(is_nonfatal_optional_rpc_failure("thread/goal/get", &error));
        assert!(!is_nonfatal_optional_rpc_failure("thread/read", &error));
    }

    #[test]
    fn missing_thread_read_keeps_service_available() {
        let error = anyhow::Error::new(RpcFailure {
            method: "thread/read".to_string(),
            code: -32600,
            message: "thread not loaded: side-thread".to_string(),
        });
        assert!(is_nonfatal_missing_thread_failure("thread/read", &error));
        assert!(!is_nonfatal_missing_thread_failure("skills/list", &error));
    }

    #[test]
    fn unrelated_thread_read_failure_still_restarts_service() {
        let error = anyhow::Error::new(RpcFailure {
            method: "thread/read".to_string(),
            code: -32603,
            message: "failed to reload config: C:/Users/test/.codex/config.toml".to_string(),
        });
        assert!(!is_nonfatal_missing_thread_failure("thread/read", &error));
    }

    #[test]
    fn unsupported_model_rpc_is_not_treated_as_optional() {
        let error = anyhow::Error::new(RpcFailure {
            method: "model/list".to_string(),
            code: -32601,
            message: "method not found".to_string(),
        });
        assert!(!is_nonfatal_optional_rpc_failure("model/list", &error));
    }

    #[test]
    fn service_history_rejects_a_repeated_item_cursor() {
        let error = normalize_history_items_page(
            &json!({"data": [], "nextCursor": "same-cursor"}),
            Some("same-cursor"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("历史分页游标未前进"));
    }

    #[test]
    fn service_candidates_prefer_newest_cli_runtime() {
        let mut candidates = vec![
            Candidate {
                path: PathBuf::from("codex-old.exe"),
                shell: false,
                version: crate::cli_wrapper::parse_codex_cli_version("codex-cli 0.130.0-alpha.5"),
                ordinal: 0,
            },
            Candidate {
                path: PathBuf::from("codex-current.exe"),
                shell: false,
                version: crate::cli_wrapper::parse_codex_cli_version("codex-cli 0.166.0-alpha.5.1"),
                ordinal: 1,
            },
        ];
        candidates.sort_by(compare_candidates);
        assert_eq!(candidates[0].path, PathBuf::from("codex-current.exe"));
    }

    #[test]
    fn configuration_failures_are_marked_for_cooldown() {
        let error = anyhow::Error::new(RpcFailure {
            method: "config/read".to_string(),
            code: -32603,
            message: "failed to reload config: C:/Users/test/.codex/config.toml".to_string(),
        });
        assert!(is_configuration_failure(&error));
        assert!(should_enter_service_cooldown(&error));
    }

    #[test]
    fn initialization_and_transport_failures_are_marked_for_cooldown() {
        let initialization = anyhow::Error::new(RpcFailure {
            method: "initialize".to_string(),
            code: -32603,
            message: "initialization failed".to_string(),
        })
        .context("Codex CLI service 初始化失败");
        assert!(should_enter_service_cooldown(&initialization));
        let transport = anyhow::anyhow!("Codex CLI service 已退出（状态：Some(ExitStatus(1))）")
            .context("Codex CLI service 初始化失败");
        assert!(should_enter_service_cooldown(&transport));
        let request = anyhow::Error::new(RpcFailure {
            method: "turn/start".to_string(),
            code: -32602,
            message: "invalid input".to_string(),
        });
        assert!(!should_enter_service_cooldown(&request));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_batch_candidate_preserves_arguments_output_and_exit_status() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Codex CLI & wrapper.cmd");
        std::fs::write(
            &path,
            "@echo off\r\necho arguments=%*\r\necho service-stderr 1>&2\r\nexit /b 19\r\n",
        )
        .unwrap();
        let output = candidate_command(&Candidate {
            path,
            shell: true,
            version: None,
            ordinal: 0,
        })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
        assert_eq!(output.status.code(), Some(19));
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("arguments=app-server --analytics-default-enabled")
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("service-stderr"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn service_cooldown_blocks_thread_reads_and_queued_model_probes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cooldown-only.cmd");
        std::fs::write(
            &path,
            "@echo off\r\nset /p request=\r\necho {\"id\":1,\"result\":{}}\r\n",
        )
        .unwrap();
        let mut process = spawn_candidate(
            &Candidate {
                path,
                shell: true,
                version: None,
                ordinal: 0,
            },
            directory.path(),
        )
        .await
        .unwrap();
        process.initialized = true;
        let service = std::sync::Arc::new(CodexCliService::new());
        let mut process_guard = service.process.lock().await;
        *process_guard = Some(process);
        let queued_service = std::sync::Arc::clone(&service);
        let queued = tokio::spawn(async move { queued_service.models().await });
        tokio::task::yield_now().await;

        *service.cooldown_until.lock().await = Some(Instant::now() + SERVICE_FAILURE_COOLDOWN);
        drop(process_guard);

        let queued_error = tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(queued_error.to_string().contains("秒后重试"));
        let thread_error = service
            .request("thread/read", json!({"threadId": "not-a-real-thread"}))
            .await
            .unwrap_err();
        assert!(thread_error.to_string().contains("秒后重试"));
        let mut process = service.process.lock().await.take().unwrap();
        process.child.kill().await.unwrap();
    }
}
