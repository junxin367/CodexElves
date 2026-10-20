use super::*;
use axum::http::Method;

const TICKET_LIFETIME_MS: u64 = 10 * 60 * 1000;
const JOURNAL_SIZE: usize = 1024;

struct Ticket {
    device_id: String,
    expires_at: u64,
}

struct Journal {
    sequence: u64,
    entries: VecDeque<Value>,
}

pub(super) struct Channels {
    pub secondary: Arc<RwLock<Option<String>>>,
    pub primary: RwLock<Option<String>>,
    pub(super) epoch: String,
    journal: std::sync::Mutex<Journal>,
    tickets: std::sync::Mutex<HashMap<String, Ticket>>,
}

impl Channels {
    pub fn new() -> Self {
        Self {
            secondary: Arc::new(RwLock::new(None)),
            primary: RwLock::new(None),
            epoch: uuid::Uuid::new_v4().to_string(),
            journal: std::sync::Mutex::new(Journal {
                sequence: 0,
                entries: VecDeque::new(),
            }),
            tickets: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn origins(&self) -> Vec<String> {
        self.secondary
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    pub fn device_id(&self, headers: &HeaderMap) -> Option<String> {
        let token = bearer(headers)?;
        if token.len() != 64 {
            return None;
        }
        self.tickets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&hash(token))
            .filter(|ticket| ticket.expires_at > now_ms())
            .map(|ticket| ticket.device_id.clone())
    }

    fn issue(&self, device: &Device) -> (String, u64) {
        let token = random_token();
        let expires_at = (now_ms() + TICKET_LIFETIME_MS).min(device.expires_at_ms);
        let mut tickets = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        tickets.retain(|_, ticket| ticket.expires_at > now_ms());
        // Bound renewals even when a compromised authorized client polls discovery.
        if tickets.len() >= MAX_DEVICES * 16 {
            if let Some(oldest) = tickets
                .iter()
                .min_by_key(|(_, ticket)| ticket.expires_at)
                .map(|(key, _)| key.clone())
            {
                tickets.remove(&oldest);
            }
        }
        tickets.insert(
            hash(&token),
            Ticket {
                device_id: device.id.clone(),
                expires_at,
            },
        );
        (token, expires_at)
    }
}

pub(super) fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("sec-websocket-protocol")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.split(',')
                        .map(str::trim)
                        .find_map(|v| v.strip_prefix("codex-channel."))
                })
        })
}

pub(super) fn read_path(method: &Method, path: &str) -> bool {
    match method {
        &Method::GET => {
            matches!(
                path,
                "/api/channels"
                    | "/api/session"
                    | "/api/events"
                    | "/api/events/ws"
                    | "/api/channel-events"
            ) || path.starts_with("/api/receipts/")
        }
        &Method::POST => path.starts_with("/api/read/"),
        _ => false,
    }
}

pub(super) fn request_path(method: &Method, path: &str) -> bool {
    read_path(method, path)
        || method == Method::POST
            && matches!(
                path,
                "/api/action/send" | "/api/action/create" | "/api/action/setGoal"
            )
}

impl RemoteAccessController {
    pub(super) fn publish_event(&self, mut event: Value) {
        let mut journal = self
            .channels
            .journal
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        journal.sequence += 1;
        event["channelEpoch"] = json!(self.channels.epoch);
        event["channelSequence"] = json!(journal.sequence);
        journal.entries.push_back(event.clone());
        if journal.entries.len() > JOURNAL_SIZE {
            journal.entries.pop_front();
        }
        // Send under the same lock so all subscribers see one canonical order.
        let _ = self.events.send(event);
    }
}

pub(super) async fn discover(
    State(state): State<WebState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    let (token, expires_at) = state.controller.channels.issue(&device);
    let origins = state
        .controller
        .public_origin()
        .into_iter()
        .chain(state.controller.channels.origins())
        .collect::<Vec<_>>();
    state
        .controller
        .authorization(device.clone(), None)
        .check()
        .await
        .map_err(authorization_error)?;
    Ok(Json(
        json!({"ok":true,"schemaVersion":1,"deviceId":device.id,"serverId":state.controller.channels.epoch,
        "origins":origins,"token":token,"expiresAtMs":expires_at,"sendActions":["send","create","setGoal"]}),
    ))
}

pub(super) fn replay(controller: &RemoteAccessController, uri: &Uri) -> Value {
    let mut url = reqwest::Url::parse("http://localhost").expect("static URL");
    url.set_query(uri.query());
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    let epoch = query.get("epoch").map(String::as_str).unwrap_or("");
    let after = query.get("after").and_then(|v| v.parse::<u64>().ok());
    let journal = controller
        .channels
        .journal
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let reset = epoch != controller.channels.epoch
        || after.is_none_or(|after| {
            after > journal.sequence
                || journal
                    .entries
                    .front()
                    .is_some_and(|entry| after + 1 < entry["channelSequence"].as_u64().unwrap_or(0))
        });
    let entries: Vec<_> = if reset {
        Vec::new()
    } else {
        journal
            .entries
            .iter()
            .filter(|entry| entry["channelSequence"].as_u64().unwrap_or(0) > after.unwrap())
            .cloned()
            .collect()
    };
    json!({"method":"remote/streamReady","channelEpoch":controller.channels.epoch,
        "cursor":if reset { journal.sequence } else { after.unwrap() },"reset":reset,
        "events":entries,"latest":journal.sequence})
}

pub(super) async fn catch_up(
    State(state): State<WebState>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Json<Value>, ApiError> {
    let device = authorized(&state, &headers).await?;
    let value = replay(&state.controller, &uri);
    state
        .controller
        .authorization(device, None)
        .check()
        .await
        .map_err(authorization_error)?;
    Ok(Json(value))
}
