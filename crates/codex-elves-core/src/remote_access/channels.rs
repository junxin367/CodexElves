use super::*;
use axum::http::Method;

const TICKET_LIFETIME_MS: u64 = 10 * 60 * 1000;
const JOURNAL_SIZE: usize = 1024;

struct Ticket {
    device_id: String,
    expires_at: u64,
    media_only: bool,
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
            .filter(|ticket| {
                ticket.expires_at > now_ms() && ticket.media_only == media_authorization(headers)
            })
            .map(|ticket| ticket.device_id.clone())
    }

    fn issue(&self, device: &Device, media_only: bool) -> (String, u64) {
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
                media_only,
            },
        );
        (token, expires_at)
    }
}

pub(super) fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("Media "))
        })
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

pub(super) fn media_authorization(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("Media "))
}

pub(super) fn read_path(method: &Method, path: &str) -> bool {
    if media_path(method, path) {
        return true;
    }
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
        &Method::POST => {
            path.starts_with("/api/read/")
                || matches!(path, "/api/workspace/changes" | "/api/workspace/change")
        }
        _ => false,
    }
}

pub(super) fn request_path(method: &Method, path: &str) -> bool {
    read_path(method, path)
        || method == Method::POST
            && (path == "/api/logout"
                || path.strip_prefix("/api/action/").is_some_and(|action| {
                    !action.is_empty()
                        && action.len() <= 64
                        && action.bytes().all(|byte| byte.is_ascii_alphabetic())
                }))
}

pub(super) fn media_path(method: &Method, path: &str) -> bool {
    if !matches!(*method, Method::GET | Method::HEAD) {
        return false;
    }
    let (id, video) = if let Some(path) = path.strip_prefix("/api/images/") {
        (path.strip_suffix("/thumbnail").unwrap_or(path), false)
    } else if let Some(path) = path.strip_prefix("/api/videos/") {
        (path.strip_suffix("/poster").unwrap_or(path), true)
    } else {
        return false;
    };
    if video {
        id.len() == 64
            && id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    } else {
        (20..=128).contains(&id.len())
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }
}

pub(super) fn media_token(uri: &Uri) -> Option<String> {
    let mut url = reqwest::Url::parse("http://localhost").ok()?;
    url.set_query(uri.query());
    let tokens = url
        .query_pairs()
        .filter(|(key, _)| key == "channel_token")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    (tokens.len() == 1
        && tokens[0].len() == 64
        && tokens[0].bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| tokens[0].clone())
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
    let (token, expires_at) = state.controller.channels.issue(&device, false);
    let (media_token, _) = state.controller.channels.issue(&device, true);
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
        "origins":origins,"token":token,"mediaToken":media_token,"expiresAtMs":expires_at,"sendActions":["send","create","setGoal"],"requestRoutingVersion":2}),
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
