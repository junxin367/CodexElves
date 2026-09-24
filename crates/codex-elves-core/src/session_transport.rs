//! Native Responses transport recovery, scoped to a relay and a Codex thread.
//! HTTP clients stay connected to the local proxy; recovery changes the actual upstream transport.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::request_headers::RequestContext;
use crate::responses_websocket::{
    RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES, UpstreamResponsesWebsocket,
    open_responses_websocket_upstream_with_request_context,
};
use crate::settings::RelayProfile;

const RECOVERY_INTERVAL: Duration = Duration::from_secs(180);
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_OBSERVATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SessionKey {
    relay_id: String,
    thread_id: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Http,
    Probing,
    Ws,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Probing => "probing",
            Self::Ws => "ws",
        }
    }
}

#[derive(Clone)]
struct Session {
    mode: Mode,
    generation: u64,
    next_probe_ms: Option<u64>,
    reason: String,
    model: String,
    request_bytes: usize,
    needs_previous_response: bool,
    last_seen_ms: u64,
    last_seen_order: u64,
    relay: RelayProfile,
    context: RequestContext,
}

fn sessions() -> &'static Mutex<HashMap<SessionKey, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<SessionKey, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(relay: &RelayProfile, context: &RequestContext) -> Option<SessionKey> {
    Some(SessionKey {
        relay_id: relay.id.clone(),
        thread_id: context.thread_id()?.to_string(),
    })
}

fn now_ms() -> u64 {
    crate::proxy_log::current_timestamp_ms()
}

fn new_session(relay: &RelayProfile, context: &RequestContext) -> Session {
    Session {
        mode: Mode::Ws,
        generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        next_probe_ms: None,
        reason: String::new(),
        model: String::new(),
        request_bytes: 0,
        needs_previous_response: false,
        last_seen_ms: now_ms(),
        last_seen_order: NEXT_OBSERVATION.fetch_add(1, Ordering::Relaxed),
        relay: relay.clone(),
        context: context.clone(),
    }
}

fn log_state(key: &SessionKey, session: &Session, event: &str) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.session_transport",
        json!({
            "relayId": key.relay_id,
            "threadId": crate::responses_websocket_bridge::redact_transport_thread_id(&key.thread_id),
            "event": event,
            "mode": session.mode.label(),
            "reason": session.reason,
            "model": session.model,
            "requestBytes": session.request_bytes,
            "nextProbeAtMs": session.next_probe_ms,
        }),
    );
}

pub(crate) fn observe_native_request(
    relay: &RelayProfile,
    context: &RequestContext,
    request: &Value,
    bytes: usize,
) {
    let Some(key) = key(relay, context) else {
        return;
    };
    let Ok(mut states) = sessions().lock() else {
        return;
    };
    let state = states
        .entry(key.clone())
        .or_insert_with(|| new_session(relay, context));
    let reschedule = if !same_upstream(&state.relay, relay) {
        // A -> B -> A is still a new configuration lifetime. Never let an
        // old A connection regain authority just because the fields match again.
        state.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        if state.mode != Mode::Ws {
            state.mode = Mode::Http;
            state.next_probe_ms = Some(
                state
                    .next_probe_ms
                    .filter(|deadline| *deadline > now_ms())
                    .unwrap_or_else(|| now_ms() + RECOVERY_INTERVAL.as_millis() as u64),
            );
            state.reason = "供应商配置已改变，等待下一次 WS 探测".to_string();
            Some(state.generation)
        } else {
            None
        }
    } else {
        None
    };
    state.model = request
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    state.request_bytes = bytes;
    state.needs_previous_response = request
        .get("previous_response_id")
        .is_some_and(|id| !id.is_null());
    state.last_seen_ms = now_ms();
    state.last_seen_order = NEXT_OBSERVATION.fetch_add(1, Ordering::Relaxed);
    state.relay = relay.clone();
    state.context = context.clone();
    drop(states);
    if let Some(generation) = reschedule {
        schedule_probe(key, generation);
    }
}

pub(crate) fn mark_http(relay: &RelayProfile, context: &RequestContext, reason: &str) {
    mark_http_inner(relay, context, reason, None);
}

pub(crate) fn generation(relay: &RelayProfile, context: &RequestContext) -> Option<u64> {
    let key = key(relay, context)?;
    sessions()
        .lock()
        .ok()?
        .get(&key)
        .map(|state| state.generation)
}

pub(crate) fn mark_http_for_generation(
    relay: &RelayProfile,
    context: &RequestContext,
    reason: &str,
    generation: u64,
) {
    mark_http_inner(relay, context, reason, Some(generation));
}

fn same_upstream(left: &RelayProfile, right: &RelayProfile) -> bool {
    crate::responses_websocket::relay_responses_base_url(left)
        == crate::responses_websocket::relay_responses_base_url(right)
        && left.api_key.trim() == right.api_key.trim()
        && left.user_agent.trim() == right.user_agent.trim()
        && left.responses_websocket_enabled.unwrap_or(true)
            == right.responses_websocket_enabled.unwrap_or(true)
        && left.local_proxy_enabled() == right.local_proxy_enabled()
}

fn mark_http_inner(
    relay: &RelayProfile,
    context: &RequestContext,
    reason: &str,
    attempt_generation: Option<u64>,
) {
    let Some(key) = key(relay, context) else {
        return;
    };
    let (generation, snapshot) = {
        let Ok(mut states) = sessions().lock() else {
            return;
        };
        if let Some(generation) = attempt_generation
            && !states.get(&key).is_some_and(|state| {
                state.generation == generation && same_upstream(&state.relay, relay)
            })
        {
            // An old stream must not recreate a removed state or overwrite a new provider.
            return;
        }
        let state = states
            .entry(key.clone())
            .or_insert_with(|| new_session(relay, context));
        if state.mode == Mode::Http {
            // Ordinary HTTP traffic and repeated failures must not postpone the next probe.
            return;
        }
        state.mode = Mode::Http;
        state.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        state.next_probe_ms = Some(now_ms() + RECOVERY_INTERVAL.as_millis() as u64);
        state.reason = reason.to_string();
        state.relay = relay.clone();
        state.context = context.clone();
        (state.generation, state.clone())
    };
    schedule_probe(key.clone(), generation);
    log_state(&key, &snapshot, "fallback");
}

pub(crate) fn forget_thread(context: &RequestContext) {
    let Some(thread_id) = context.thread_id() else {
        return;
    };
    if let Ok(mut states) = sessions().lock() {
        states.retain(|key, _| key.thread_id != thread_id);
    }
}

pub(crate) fn should_use_http(relay: &RelayProfile, context: &RequestContext) -> bool {
    key(relay, context)
        .and_then(|key| {
            sessions()
                .lock()
                .ok()?
                .get(&key)
                .map(|state| state.mode != Mode::Ws)
        })
        .unwrap_or(false)
}

pub(crate) fn status(thread_ids: &[String]) -> Value {
    let Ok(states) = sessions().lock() else {
        return json!({"sessions":[]});
    };
    json!({
        "sessions": states.iter().filter(|(key, _)| thread_ids.contains(&key.thread_id))
            .map(|(key, state)| json!({
                "threadId": key.thread_id,
                "relayId": key.relay_id,
                "relayName": state.relay.name,
                "mode": state.mode.label(),
                "model": state.model,
                "reason": state.reason,
                "requestBytes": state.request_bytes,
                "nextProbeAtMs": state.next_probe_ms,
                "lastSeenAtMs": state.last_seen_ms,
                "lastSeenOrder": state.last_seen_order,
            })).collect::<Vec<_>>()
    })
}

fn schedule_probe(key: SessionKey, generation: u64) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        loop {
            let deadline = {
                let Ok(states) = sessions().lock() else {
                    return;
                };
                let Some(state) = states.get(&key) else {
                    return;
                };
                if state.generation != generation || state.mode == Mode::Ws {
                    return;
                }
                state.next_probe_ms.unwrap_or_else(now_ms)
            };
            tokio::time::sleep(Duration::from_millis(deadline.saturating_sub(now_ms()))).await;
            if !refresh_probe_relay(&key, generation) {
                return;
            }
            if !probe_once(&key, generation, || refresh_probe_relay(&key, generation)).await {
                return;
            }
        }
    });
}

// Refresh the actual provider, including a configured failover provider.
fn refresh_probe_relay(key: &SessionKey, generation: u64) -> bool {
    let Ok(settings) = crate::settings::SettingsStore::default().load() else {
        return true;
    };
    refresh_probe_relay_with_settings(key, generation, &settings)
}

fn refresh_probe_relay_with_settings(
    key: &SessionKey,
    generation: u64,
    settings: &crate::settings::BackendSettings,
) -> bool {
    let relay = settings
        .relay_profiles
        .iter()
        .find(|relay| relay.id == key.relay_id)
        .cloned()
        .unwrap_or_else(|| settings.active_relay_profile());
    let eligible = settings.relay_profiles_enabled
        && settings.active_aggregate_relay_profile().is_none()
        && relay.id == key.relay_id
        && relay.local_proxy_enabled()
        && crate::responses_websocket::relay_prefers_native_responses_websocket(&relay);
    let Ok(mut states) = sessions().lock() else {
        return false;
    };
    let Some(state) = states.get_mut(key) else {
        return false;
    };
    if state.generation != generation {
        return false;
    }
    if eligible {
        state.relay = relay;
        true
    } else {
        states.remove(key);
        false
    }
}

async fn probe_once(
    key: &SessionKey,
    generation: u64,
    refresh_policy: impl FnOnce() -> bool,
) -> bool {
    let snapshot = {
        let Ok(mut states) = sessions().lock() else {
            return false;
        };
        let Some(state) = states.get_mut(key) else {
            return false;
        };
        if state.generation != generation || state.mode == Mode::Ws {
            return false;
        }
        state.mode = Mode::Probing;
        state.clone()
    };
    log_state(key, &snapshot, "probe_started");
    let result = if snapshot.request_bytes > RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES {
        Err(anyhow::anyhow!("最近请求仍超过 WS 的 16 MiB 安全上限"))
    } else if snapshot.needs_previous_response {
        Err(anyhow::anyhow!(
            "最近请求需要原响应上下文，等待完整请求后恢复 WS"
        ))
    } else {
        tokio::time::timeout(PROBE_TIMEOUT, async {
            let mut socket = open_responses_websocket_upstream_with_request_context(
                &snapshot.relay,
                &snapshot.context,
            )
            .await?;
            let ping = format!("codex-elves-recovery:{generation}").into_bytes();
            socket.send(Message::Ping(ping.clone().into())).await?;
            loop {
                match socket.next().await {
                    Some(Ok(Message::Pong(payload))) if payload.as_ref() == ping.as_slice() => {
                        let _ =
                            tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
                        return Ok(());
                    }
                    Some(Ok(Message::Ping(payload))) => socket.send(Message::Pong(payload)).await?,
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Err(error)) => return Err(error.into()),
                    _ => anyhow::bail!("WS 探测未收到有效 Pong"),
                }
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("WS 恢复探测超时"))
        .and_then(|result| result)
    };
    if !refresh_policy() {
        return false;
    }
    let Ok(mut states) = sessions().lock() else {
        return false;
    };
    let Some(state) = states.get_mut(key) else {
        return false;
    };
    if state.generation != generation {
        return false;
    }
    let result = if same_upstream(&state.relay, &snapshot.relay) {
        result
    } else {
        Err(anyhow::anyhow!("供应商配置已改变，等待下一次 WS 探测"))
    };
    let retry = match result {
        Ok(())
            if state.request_bytes <= RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES
                && !state.needs_previous_response =>
        {
            state.mode = Mode::Ws;
            state.next_probe_ms = None;
            state.reason.clear();
            false
        }
        result => {
            state.mode = Mode::Http;
            state.reason = result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "最新请求尚不适合恢复 WS".to_string());
            state.next_probe_ms = Some(now_ms() + RECOVERY_INTERVAL.as_millis() as u64);
            true
        }
    };
    let snapshot = state.clone();
    drop(states);
    // Disk logging must not hold the mutex shared by every active conversation.
    log_state(
        key,
        &snapshot,
        if retry {
            "probe_failed"
        } else {
            "ws_recovered"
        },
    );
    retry
}

/// Returns an actual WS-backed SSE response after recovery, or lets the existing HTTP path run.
pub(crate) async fn try_ws_for_http(
    relay: &RelayProfile,
    context: &RequestContext,
    request: &Value,
    first_response_timeout: Option<Duration>,
) -> Option<(reqwest::Response, Value)> {
    if request.get("background").and_then(Value::as_bool) == Some(true) {
        // Preserve HTTP background execution rather than silently changing it
        // into foreground generation on a WebSocket.
        return None;
    }
    let key = key(relay, context)?;
    let mut ws_request =
        crate::responses_websocket_bridge::normalize_downstream_response_create_payload(request);
    // HTTP-only fields are not part of response.create.
    if let Some(object) = ws_request.as_object_mut() {
        object.remove("stream");
        object.remove("background");
    }
    ws_request["type"] = json!("response.create");
    let text = serde_json::to_string(&ws_request).ok()?;
    let known = sessions().lock().ok()?.contains_key(&key);
    observe_native_request(relay, context, &ws_request, text.len());
    if !known {
        mark_http(relay, context, "客户端已使用 HTTP，等待 WS 恢复探测");
    }
    if should_use_http(relay, context) {
        return None;
    }
    if text.len() > RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES {
        mark_http(relay, context, "请求超过 WS 的 16 MiB 安全上限");
        return None;
    }
    if ws_request
        .get("previous_response_id")
        .is_some_and(|id| !id.is_null())
    {
        mark_http(relay, context, "增量请求需要原响应上下文，继续使用 HTTP");
        return None;
    }
    let generation = {
        let states = sessions().lock().ok()?;
        let state = states.get(&key)?;
        if state.mode != Mode::Ws {
            return None;
        }
        state.generation
    };
    let attempt = async {
        let mut socket =
            open_responses_websocket_upstream_with_request_context(relay, context).await?;
        tokio::time::timeout(
            Duration::from_secs(30),
            socket.send(Message::Text(text.into())),
        )
        .await
        .context("发送恢复后的 WS 请求超时")??;
        let mut stream =
            WsSseStream::new(socket, relay.clone(), context.clone(), request, generation);
        let first = stream.next_event().await?;
        // Even an initial failure event is an authoritative application result.
        // Forward it; only transport failure before any event may try HTTP.
        stream.first = Some(first);
        let body = reqwest::Body::wrap_stream(futures_util::stream::unfold(
            stream,
            |mut state| async move {
                if let Some(first) = state.first.take() {
                    return Some((
                        Ok::<_, std::io::Error>(format!("data: {first}\n\n").into_bytes()),
                        state,
                    ));
                }
                if state.finished {
                    return None;
                }
                let data = match state.next_event().await {
                    Ok(text) => text,
                    Err(error) => {
                        mark_http_inner(
                            &state.relay,
                            &state.context,
                            &error.to_string(),
                            Some(state.generation),
                        );
                        state.finished = true;
                        json!({
                        "type": "response.failed",
                        "response": {
                            "id": state.response_id.clone().unwrap_or_else(|| format!("resp_failed_{}", uuid::Uuid::new_v4())),
                            "object": "response",
                            "status": "failed",
                            "error": {"code":"responses_websocket_upstream_failed","message":error.to_string()}
                        }
                    }).to_string()
                    }
                };
                Some((Ok(format!("data: {data}\n\n").into_bytes()), state))
            },
        ));
        let response = tokio_tungstenite::tungstenite::http::Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .body(body)?;
        Ok::<_, anyhow::Error>(reqwest::Response::from(response))
    };
    let result = match first_response_timeout {
        Some(timeout) => tokio::time::timeout(timeout, attempt)
            .await
            .context("恢复后的 WS 请求等待首响应超时")
            .and_then(|result| result),
        None => attempt.await,
    };
    match result {
        Ok(response) => Some((response, ws_request)),
        Err(error) => {
            mark_http_inner(relay, context, &error.to_string(), Some(generation));
            None
        }
    }
}

struct WsSseStream {
    socket: UpstreamResponsesWebsocket,
    relay: RelayProfile,
    context: RequestContext,
    generation: u64,
    first: Option<String>,
    finished: bool,
    response_id: Option<String>,
    last_application: Instant,
    idle_timeout: Duration,
    liveness: crate::responses_websocket_bridge::WebSocketTransportLiveness,
    ping: tokio::time::Interval,
    check: tokio::time::Interval,
}

impl WsSseStream {
    fn new(
        socket: UpstreamResponsesWebsocket,
        relay: RelayProfile,
        context: RequestContext,
        request: &Value,
        generation: u64,
    ) -> Self {
        let mut ping = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(30),
            Duration::from_secs(30),
        );
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            socket,
            relay,
            context,
            generation,
            first: None,
            finished: false,
            response_id: None,
            last_application: Instant::now(),
            idle_timeout: crate::protocol_proxy::stream_idle_timeout_for_request(Some(request)),
            liveness: Default::default(),
            ping,
            check: tokio::time::interval(Duration::from_secs(5)),
        }
    }

    async fn next_event(&mut self) -> anyhow::Result<String> {
        loop {
            tokio::select! {
                message = self.socket.next() => {
                    let message = message.context("WS 上游在响应完成前结束")??;
                    self.liveness.observe_frame(&message, Instant::now());
                    match message {
                        Message::Text(text) => {
                            let value: Value = serde_json::from_str(&text)?;
                            let event = value.get("type").and_then(Value::as_str).unwrap_or_default();
                            if !event.starts_with("response.") && event != "error" { continue; }
                            self.last_application = Instant::now();
                            if let Some(id) = value.pointer("/response/id").or_else(|| value.get("response_id")).and_then(Value::as_str) {
                                self.response_id = Some(id.to_string());
                            }
                            self.finished = matches!(event, "response.completed" | "response.incomplete" | "response.failed" | "error");
                            if self.finished {
                                if matches!(event, "response.failed" | "error") {
                                    mark_http_inner(&self.relay, &self.context, "WS 返回失败事件", Some(self.generation));
                                }
                                let _ = tokio::time::timeout(Duration::from_secs(1), self.socket.close(None)).await;
                            }
                            // WS frames may contain pretty-printed JSON. SSE data fields
                            // are line based, so emit compact JSON without literal CR/LF.
                            return Ok(value.to_string());
                        }
                        Message::Ping(payload) => {
                            tokio::time::timeout(Duration::from_secs(30), self.socket.send(Message::Pong(payload)))
                                .await.context("WS Pong 发送超时")??;
                        }
                        Message::Pong(_) => {}
                        Message::Close(_) => anyhow::bail!("WS 上游在响应完成前关闭"),
                        _ => anyhow::bail!("WS 上游返回非文本应用帧"),
                    }
                }
                _ = self.ping.tick() => {
                    if let Some(payload) = self.liveness.begin_ping(Instant::now()) {
                        tokio::time::timeout(Duration::from_secs(30), self.socket.send(Message::Ping(payload.into())))
                            .await.context("WS Ping 发送超时")??;
                    }
                }
                _ = self.check.tick() => {
                    let now = Instant::now();
                    if self.liveness.take_timeout(Duration::from_secs(15), now).is_some()
                        && self.liveness.timeout_limit_exceeded()
                    {
                        anyhow::bail!("WS 上游连续 4 次 Ping 超时");
                    }
                    if now.duration_since(self.last_application) >= self.idle_timeout {
                        anyhow::bail!("WS 上游应用响应超时");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn probe_once(key: &SessionKey, generation: u64) -> bool {
        super::probe_once(key, generation, || true).await
    }

    async fn try_ws_for_http(
        relay: &RelayProfile,
        context: &RequestContext,
        request: &Value,
    ) -> Option<reqwest::Response> {
        super::try_ws_for_http(relay, context, request, None)
            .await
            .map(|(response, _)| response)
    }

    fn fixture(address: std::net::SocketAddr) -> (RelayProfile, RequestContext, Value) {
        let mut relay = RelayProfile {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Recovery test".into(),
            base_url: format!("http://{address}"),
            upstream_base_url: format!("http://{address}"),
            api_key: "test-recovery-key".into(),
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            local_proxy_enabled: Some(true),
            model_mappings: vec![crate::settings::RelayModelMapping {
                request_model: "gpt-test".into(),
                protocol: crate::settings::RelayProtocol::Responses,
                alias: String::new(),
                context_window: String::new(),
            }],
            ..Default::default()
        };
        crate::responses_websocket::normalize_responses_websocket_capability(&mut relay);
        relay.responses_websocket.state =
            crate::settings::ResponsesWebsocketCapabilityState::Supported;
        let context = RequestContext::from_http_request(
            format!(
                "POST /v1/responses HTTP/1.1\r\nthread-id: {}\r\n\r\n",
                uuid::Uuid::new_v4()
            )
            .as_bytes(),
        );
        (
            relay,
            context,
            json!({"model":"gpt-test", "input":"hello", "stream":true}),
        )
    }

    fn snapshot(relay: &RelayProfile, context: &RequestContext) -> (SessionKey, Session) {
        let key = key(relay, context).unwrap();
        let state = sessions().lock().unwrap().get(&key).unwrap().clone();
        (key, state)
    }

    async fn pong_probe(listener: &TcpListener) {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let Some(Ok(Message::Ping(payload))) = socket.next().await else {
            panic!("probe must only send a control Ping, never a model request");
        };
        socket.send(Message::Pong(payload)).await.unwrap();
        // Consume the client's close so the next accept is a fresh business request.
        let _ = socket.next().await;
    }

    #[tokio::test]
    async fn fallback_deadline_is_fixed_and_sessions_and_relays_are_isolated() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        let before = now_ms();
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "disconnected");
        let (_, first) = snapshot(&relay, &context);
        assert_eq!(RECOVERY_INTERVAL, Duration::from_secs(180));
        assert!(first.next_probe_ms.unwrap() >= before + 180_000);
        assert!(first.next_probe_ms.unwrap() <= now_ms() + 180_000);
        observe_native_request(&relay, &context, &request, 120);
        mark_http(&relay, &context, "another HTTP retry");
        let (key, second) = snapshot(&relay, &context);
        assert_eq!(first.next_probe_ms, second.next_probe_ms);
        assert_eq!(first.generation, second.generation);
        assert!(try_ws_for_http(&relay, &context, &request).await.is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        let (_, other_context, _) = fixture(listener.local_addr().unwrap());
        assert!(!should_use_http(&relay, &other_context));
        let mut other_relay = relay.clone();
        other_relay.id = uuid::Uuid::new_v4().to_string();
        assert!(!should_use_http(&other_relay, &context));
        let public = status(&[key.thread_id.clone()]);
        assert_eq!(public["sessions"][0]["mode"], "http");
        assert!(!public.to_string().contains("test-recovery-key"));
        sessions().lock().unwrap().remove(&key);
    }

    #[tokio::test]
    async fn failed_probe_waits_another_three_minutes_and_success_routes_following_requests_to_ws()
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "initial failure");
        let (key, state) = snapshot(&relay, &context);
        let server = tokio::spawn(async move {
            // First probe: handshake succeeds, but the transport cannot answer Ping.
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Ping(_)))));
            socket.close(None).await.unwrap();
            pong_probe(&listener).await;
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let Some(Ok(Message::Text(text))) = socket.next().await else {
                    panic!("expected WS request")
                };
                let payload: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(payload["type"], "response.create");
                assert!(payload.get("stream").is_none());
                assert_eq!(payload["input"], "hello");
                socket.send(Message::Text(json!({
                    "type":"response.completed",
                    "response":{"id":format!("resp_{index}"),"status":"completed","output":[]}
                }).to_string().into())).await.unwrap();
                let _ = socket.next().await;
            }
        });
        let before = now_ms();
        assert!(probe_once(&key, state.generation).await);
        let (_, failed) = snapshot(&relay, &context);
        assert_eq!(failed.mode.label(), "http");
        assert!(failed.next_probe_ms.unwrap() >= before + 180_000);
        assert!(!probe_once(&key, state.generation).await);
        assert!(!should_use_http(&relay, &context));
        assert!(snapshot(&relay, &context).1.next_probe_ms.is_none());
        for index in 0..2 {
            let settings = crate::settings::BackendSettings {
                active_relay_id: relay.id.clone(),
                relay_profiles: vec![relay.clone()],
                ..Default::default()
            };
            let upstream = crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(), settings, &context,
            ).await.expect("HTTP entry point must use recovered WS");
            assert!(upstream.endpoint.as_deref().unwrap().starts_with("ws://"));
            let response = upstream.response.unwrap();
            assert_eq!(response.headers()["content-type"], "text/event-stream");
            let text = response.text().await.unwrap();
            assert!(text.starts_with("data: "));
            assert!(text.contains(&format!("resp_{index}")));
        }
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        sessions().lock().unwrap().remove(&key);
    }

    #[tokio::test]
    async fn oversized_and_incremental_requests_do_not_claim_a_false_recovery() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, mut request) = fixture(listener.local_addr().unwrap());
        observe_native_request(
            &relay,
            &context,
            &request,
            RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES + 1,
        );
        mark_http(&relay, &context, "oversized");
        let (key, state) = snapshot(&relay, &context);
        assert!(probe_once(&key, state.generation).await);
        assert!(should_use_http(&relay, &context));
        request["previous_response_id"] = json!("resp_on_old_connection");
        observe_native_request(&relay, &context, &request, 100);
        assert!(probe_once(&key, state.generation).await);
        assert!(should_use_http(&relay, &context));
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        // A later full request can be probed in the next cycle.
        request
            .as_object_mut()
            .unwrap()
            .remove("previous_response_id");
        observe_native_request(&relay, &context, &request, 100);
        let server = tokio::spawn(async move { pong_probe(&listener).await });
        assert!(!probe_once(&key, state.generation).await);
        assert!(!should_use_http(&relay, &context));
        server.await.unwrap();
        sessions().lock().unwrap().remove(&key);
    }

    #[tokio::test]
    async fn failure_before_first_event_allows_http_but_partial_output_is_not_replayed() {
        for partial in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (relay, context, request) = fixture(listener.local_addr().unwrap());
            observe_native_request(&relay, &context, &request, 100);
            let server =
                tokio::spawn(async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
                    if partial {
                        socket.send(Message::Text(json!({
                        "type":"response.created",
                        "response":{"id":"resp_partial","status":"in_progress","output":[]}
                    }).to_string().into())).await.unwrap();
                    }
                    socket.close(None).await.unwrap();
                    assert!(
                        tokio::time::timeout(Duration::from_millis(30), listener.accept())
                            .await
                            .is_err()
                    );
                });
            let response = try_ws_for_http(&relay, &context, &request).await;
            if partial {
                let text = response
                    .expect("already-delivered events must not be replayed")
                    .text()
                    .await
                    .unwrap();
                assert!(text.contains("response.created"));
                assert!(text.contains("response.failed"));
                assert!(text.contains("\"id\":\"resp_partial\""));
            } else {
                assert!(
                    response.is_none(),
                    "existing HTTP path must handle this attempt"
                );
            }
            assert!(should_use_http(&relay, &context));
            server.await.unwrap();
            let (key, _) = snapshot(&relay, &context);
            sessions().lock().unwrap().remove(&key);
        }
    }

    #[tokio::test]
    async fn multiline_websocket_json_is_a_complete_sse_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let expected = json!({
            "type":"response.completed",
            "response":{"id":"resp_pretty","status":"completed","output":[
                {"type":"message","content":[{"type":"output_text","text":"first\nsecond\r\nthird"}]}
            ]}
        });
        let wire = serde_json::to_string_pretty(&expected)
            .unwrap()
            .replace('\n', "\r\n");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            socket.send(Message::Text(wire.into())).await.unwrap();
            let _ = socket.next().await;
        });
        let sse = try_ws_for_http(&relay, &context, &request)
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        // Match SSE parsing: only data fields contribute to an event's JSON.
        let data = sse
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(serde_json::from_str::<Value>(&data).unwrap(), expected);
        server.await.unwrap();
        sessions()
            .lock()
            .unwrap()
            .remove(&key(&relay, &context).unwrap());
    }

    #[tokio::test]
    async fn old_probe_does_not_recover_a_changed_relay_configuration() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "disconnected");
        let (key, state) = snapshot(&relay, &context);
        let (ping_sent, ping_received) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let Some(Ok(Message::Ping(payload))) = socket.next().await else {
                panic!("expected Ping")
            };
            ping_sent.send(()).unwrap();
            released.await.unwrap();
            socket.send(Message::Pong(payload)).await.unwrap();
            let _ = socket.next().await;
        });
        let probe_key = key.clone();
        let probe = tokio::spawn(async move { probe_once(&probe_key, state.generation).await });
        ping_received.await.unwrap();
        let mut changed = relay.clone();
        changed.api_key = "replacement-test-key".into();
        observe_native_request(&changed, &context, &request, 100);
        release.send(()).unwrap();
        assert!(
            !probe.await.unwrap(),
            "the obsolete probe must stop; the new generation owns its timer"
        );
        assert!(should_use_http(&changed, &context));
        let (_, changed_state) = snapshot(&changed, &context);
        assert_ne!(changed_state.generation, state.generation);
        assert!(
            changed_state
                .next_probe_ms
                .is_some_and(|deadline| deadline > now_ms())
        );
        server.await.unwrap();
        sessions().lock().unwrap().remove(&key);
    }

    #[tokio::test]
    async fn probe_rechecks_disabled_policy_after_the_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "disconnected");
        let (key, state) = snapshot(&relay, &context);
        let mut disabled = relay.clone();
        disabled.responses_websocket_enabled = Some(false);
        let settings = crate::settings::BackendSettings {
            active_relay_id: disabled.id.clone(),
            relay_profiles: vec![disabled],
            ..Default::default()
        };
        let server = tokio::spawn(async move { pong_probe(&listener).await });
        assert!(
            !super::probe_once(&key, state.generation, || {
                refresh_probe_relay_with_settings(&key, state.generation, &settings)
            })
            .await
        );
        assert!(
            status(&[key.thread_id.clone()])["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn configured_failover_provider_keeps_its_recovery_timer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        let (active, _, _) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "failover connection interrupted");
        let (key, state) = snapshot(&relay, &context);
        let settings = crate::settings::BackendSettings {
            active_relay_id: active.id.clone(),
            relay_profiles: vec![active, relay.clone()],
            ..Default::default()
        };
        assert!(refresh_probe_relay_with_settings(
            &key,
            state.generation,
            &settings
        ));
        assert!(should_use_http(&relay, &context));
        sessions().lock().unwrap().remove(&key);
    }

    #[tokio::test]
    async fn stale_ws_stream_failure_cannot_replace_a_new_configuration_or_recreate_state() {
        for scenario in 0..3 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (relay, context, request) = fixture(listener.local_addr().unwrap());
            observe_native_request(&relay, &context, &request, 100);
            let (release, released) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
                socket
                    .send(Message::Text(
                        json!({
                            "type":"response.created","response":{"id":"resp_old"}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                released.await.unwrap();
                socket.close(None).await.unwrap();
            });
            let response = try_ws_for_http(&relay, &context, &request).await.unwrap();
            let mut changed = relay.clone();
            changed.api_key = "new-test-key".into();
            observe_native_request(&changed, &context, &request, 100);
            if scenario > 0 {
                forget_thread(&context);
            }
            if scenario == 2 {
                observe_native_request(&relay, &context, &request, 100);
            }
            release.send(()).unwrap();
            assert!(response.text().await.unwrap().contains("response.failed"));
            assert!(!should_use_http(&changed, &context));
            let key = key(&relay, &context).unwrap();
            if scenario == 1 {
                assert!(
                    status(&[key.thread_id.clone()])["sessions"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            }
            sessions().lock().unwrap().remove(&key);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn normal_http_only_request_clears_stale_transport_status() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let key = key(&relay, &context).unwrap();
        let mut http_relay = relay.clone();
        http_relay.responses_websocket_enabled = Some(false);
        let settings = crate::settings::BackendSettings {
            active_relay_id: http_relay.id.clone(),
            relay_profiles: vec![http_relay],
            ..Default::default()
        };
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
                if let Some(head_end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&received[..head_end]).to_lowercase();
                    assert!(head.starts_with("post /v1/responses"));
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    if received.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let body = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_http\",\"status\":\"completed\",\"output\":[]}}\n\n";
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
        });
        let upstream =
            crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(),
                settings,
                &context,
            )
            .await
            .unwrap();
        assert!(upstream.endpoint.unwrap().starts_with("http://"));
        assert!(
            upstream
                .response
                .unwrap()
                .text()
                .await
                .unwrap()
                .contains("resp_http")
        );
        assert!(
            status(&[key.thread_id])["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_the_http_body_releases_ws_without_arming_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            socket
                .send(Message::Text(
                    json!({
                        "type":"response.created","response":{"id":"resp_cancel"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let end = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap();
            assert!(matches!(
                end,
                None | Some(Err(_)) | Some(Ok(Message::Close(_)))
            ));
        });
        let response = try_ws_for_http(&relay, &context, &request).await.unwrap();
        drop(response);
        server.await.unwrap();
        assert!(!should_use_http(&relay, &context));
        sessions()
            .lock()
            .unwrap()
            .remove(&key(&relay, &context).unwrap());
    }

    async fn reply_to_http(mut stream: tokio::net::TcpStream) -> Value {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut received = Vec::new();
        let body = loop {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            received.extend_from_slice(&buffer[..count]);
            if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&received[..end]).to_lowercase();
                assert!(head.starts_with("post /v1/responses"));
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse::<usize>()
                    .unwrap();
                if received.len() >= end + 4 + length {
                    break serde_json::from_slice::<Value>(&received[end + 4..end + 4 + length])
                        .unwrap();
                }
            }
        };
        let response = "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_http\",\"status\":\"completed\",\"output\":[]}}\n\n";
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len(),
        ).as_bytes()).await.unwrap();
        body
    }

    fn settings_for(relay: &RelayProfile) -> crate::settings::BackendSettings {
        crate::settings::BackendSettings {
            active_relay_id: relay.id.clone(),
            relay_profiles: vec![relay.clone()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn recovered_ws_log_matches_the_actual_pruned_wire_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, mut request) = fixture(listener.local_addr().unwrap());
        request["background"] = json!(false);
        request["input"] = json!([
            {"role":"user","content":"obsolete history"},
            {"type":"compaction","encrypted_content":"checkpoint"},
            {"role":"user","content":"new request"}
        ]);
        observe_native_request(&relay, &context, &request, 100);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let Some(Ok(Message::Text(text))) = socket.next().await else {
                panic!("expected WS")
            };
            let payload: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(payload["input"].as_array().unwrap().len(), 2);
            assert!(payload.get("background").is_none());
            assert!(payload.get("stream").is_none());
            socket.send(Message::Text(json!({
                "type":"response.completed","response":{"id":"resp_wire","status":"completed","output":[]}
            }).to_string().into())).await.unwrap();
            let _ = socket.next().await;
            text.to_string()
        });
        let upstream =
            crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(),
                settings_for(&relay),
                &context,
            )
            .await
            .unwrap();
        let _ = upstream.response.unwrap().bytes().await.unwrap();
        assert_eq!(upstream.request_body, server.await.unwrap());
        forget_thread(&context);
    }

    #[tokio::test]
    async fn recovered_ws_honors_the_callers_first_response_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            // A successful handshake alone must not defeat the caller's timeout.
            let _ = socket.next().await;
            let (stream, _) = listener.accept().await.unwrap();
            reply_to_http(stream).await
        });
        let result = tokio::time::timeout(Duration::from_secs(2),
            crate::protocol_proxy::open_responses_proxy_request_with_settings_request_context_and_timeout(
                &request.to_string(), settings_for(&relay), &context, Some(Duration::from_millis(150)),
            ),
        ).await;
        if result.is_err() {
            server.abort();
        }
        let upstream = result
            .expect("WS must respect the 150ms timeout and allow HTTP fallback")
            .unwrap();
        assert!(upstream.endpoint.as_deref().unwrap().starts_with("http://"));
        assert!(
            upstream
                .response
                .unwrap()
                .text()
                .await
                .unwrap()
                .contains("resp_http")
        );
        assert_eq!(server.await.unwrap(), request);
        assert!(should_use_http(&relay, &context));
        forget_thread(&context);
    }

    #[tokio::test]
    async fn background_requests_keep_their_http_semantics() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, mut request) = fixture(listener.local_addr().unwrap());
        request["background"] = json!(true);
        observe_native_request(&relay, &context, &request, 100);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut first = [0; 1];
            stream.peek(&mut first).await.unwrap();
            if first[0] == b'G' {
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
                socket.send(Message::Text(json!({
                    "type":"response.completed","response":{"id":"resp_wrong","status":"completed","output":[]}
                }).to_string().into())).await.unwrap();
                let _ = socket.next().await;
                None
            } else {
                Some(reply_to_http(stream).await)
            }
        });
        let upstream =
            crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(),
                settings_for(&relay),
                &context,
            )
            .await
            .unwrap();
        let _ = upstream.response.unwrap().bytes().await.unwrap();
        assert_eq!(server.await.unwrap(), Some(request));
        assert!(
            !should_use_http(&relay, &context),
            "a background request is not a WS failure"
        );
        forget_thread(&context);
    }

    #[tokio::test]
    async fn returning_to_an_old_configuration_does_not_revalidate_its_inflight_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let old = generation(&relay, &context).unwrap();
        let mut changed = relay.clone();
        changed.api_key = "replacement-test-key".into();
        observe_native_request(&changed, &context, &request, 100);
        observe_native_request(&relay, &context, &request, 100);
        mark_http_for_generation(
            &relay,
            &context,
            "late failure from the first connection",
            old,
        );
        assert!(!should_use_http(&relay, &context));
        forget_thread(&context);
    }

    #[tokio::test]
    async fn configuration_changes_preserve_an_existing_http_recovery_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "first failure");
        let (key, before) = snapshot(&relay, &context);
        let mut changed = relay.clone();
        changed.api_key = "replacement-test-key".into();
        observe_native_request(&changed, &context, &request, 100);
        observe_native_request(&relay, &context, &request, 100);
        let (_, after) = snapshot(&relay, &context);
        assert_ne!(before.generation, after.generation);
        assert_eq!(before.next_probe_ms, after.next_probe_ms);
        assert!(!probe_once(&key, before.generation).await);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        assert!(should_use_http(&relay, &context));
        forget_thread(&context);
    }

    #[tokio::test]
    async fn an_authoritative_failure_event_is_delivered_without_reexecuting_the_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        observe_native_request(&relay, &context, &request, 100);
        let failure = json!({
            "type":"response.failed",
            "response":{"id":"resp_already_executed","status":"failed","output":[],
                "error":{"code":"server_error","message":"execution failed after accepting the request"}}
        });
        let wire = failure.to_string();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            socket.send(Message::Text(wire.into())).await.unwrap();
            let _ = socket.next().await;
            if let Ok(Ok((stream, _))) =
                tokio::time::timeout(Duration::from_millis(150), listener.accept()).await
            {
                reply_to_http(stream).await;
                2
            } else {
                1
            }
        });
        let upstream =
            crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(),
                settings_for(&relay),
                &context,
            )
            .await
            .unwrap();
        let text = upstream.response.unwrap().text().await.unwrap();
        let count = server.await.unwrap();
        assert_eq!(
            count, 1,
            "a completed failure is an application result, not a reason to execute again"
        );
        assert!(text.contains("resp_already_executed"));
        assert!(text.contains("response.failed"));
        assert!(!text.contains("response.completed"));
        forget_thread(&context);
    }

    #[test]
    fn transport_status_order_does_not_depend_on_clock_resolution_or_clock_adjustments() {
        let (older, context, request) = fixture("127.0.0.1:1".parse().unwrap());
        let (newer, _, _) = fixture("127.0.0.1:1".parse().unwrap());
        observe_native_request(&older, &context, &request, 100);
        observe_native_request(&newer, &context, &request, 100);
        {
            let mut states = sessions().lock().unwrap();
            states
                .get_mut(&key(&older, &context).unwrap())
                .unwrap()
                .last_seen_ms = 200;
            states
                .get_mut(&key(&newer, &context).unwrap())
                .unwrap()
                .last_seen_ms = 100;
        }
        let result = status(&[context.thread_id().unwrap().to_string()]);
        let latest = result["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .max_by_key(|session| session["lastSeenOrder"].as_u64().unwrap())
            .unwrap();
        assert_eq!(latest["relayId"], newer.id);
        forget_thread(&context);
    }

    #[tokio::test]
    #[ignore = "uses two real three-minute recovery intervals; run explicitly for end-to-end timer checks"]
    async fn real_clock_failure_then_recovery_keeps_the_three_minute_cycle() {
        struct SettingsGuard(Option<std::path::PathBuf>);
        impl Drop for SettingsGuard {
            fn drop(&mut self) {
                crate::paths::set_settings_path_for_tests(self.0.take());
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let _settings = SettingsGuard(crate::paths::set_settings_path_for_tests(Some(
            temp.path().join("settings.json"),
        )));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (relay, context, request) = fixture(listener.local_addr().unwrap());
        crate::settings::SettingsStore::default()
            .save(&settings_for(&relay))
            .unwrap();
        let configured = crate::settings::SettingsStore::default()
            .load()
            .unwrap()
            .active_relay_profile();
        assert!(crate::responses_websocket::relay_prefers_native_responses_websocket(&configured));
        assert!(!configured.api_key.is_empty());
        let relay = configured;
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (second_tx, second_rx) = tokio::sync::oneshot::channel();
        let started = Instant::now();
        let provider = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let first_at = started.elapsed();
            assert!(first_at >= Duration::from_secs(179));
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Ping(_)))));
            socket.close(None).await.unwrap();
            first_tx.send(first_at).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let second_at = started.elapsed();
            assert!(second_at.saturating_sub(first_at) >= Duration::from_secs(179));
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let Some(Ok(Message::Ping(payload))) = socket.next().await else {
                panic!("expected probe Ping")
            };
            socket.send(Message::Pong(payload)).await.unwrap();
            let _ = socket.next().await;
            second_tx.send(second_at).unwrap();
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let Some(Ok(Message::Text(text))) = socket.next().await else {
                    panic!("expected WS business request")
                };
                assert_eq!(
                    serde_json::from_str::<Value>(&text).unwrap()["type"],
                    "response.create"
                );
                socket.send(Message::Text(json!({
                    "type":"response.completed","response":{"id":"resp_real_clock","status":"completed","output":[]}
                }).to_string().into())).await.unwrap();
                let _ = socket.next().await;
            }
        });
        observe_native_request(&relay, &context, &request, 100);
        mark_http(&relay, &context, "real clock test");
        eprintln!("REAL_CLOCK: waiting for the first scheduled 180-second probe");
        let first_at = tokio::time::timeout(Duration::from_secs(200), first_rx)
            .await
            .unwrap()
            .unwrap();
        eprintln!(
            "REAL_CLOCK: first probe failed at {:.3}s; HTTP remains active",
            first_at.as_secs_f64()
        );
        assert!(should_use_http(&relay, &context));
        let second_at = tokio::time::timeout(Duration::from_secs(200), second_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while should_use_http(&relay, &context) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        eprintln!(
            "REAL_CLOCK: second probe recovered WS at {:.3}s",
            second_at.as_secs_f64()
        );
        for _ in 0..2 {
            let upstream = crate::protocol_proxy::open_responses_proxy_request_with_settings_and_request_context(
                &request.to_string(), settings_for(&relay), &context,
            ).await.unwrap();
            assert!(upstream.endpoint.unwrap().starts_with("ws://"));
            assert!(
                upstream
                    .response
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
                    .contains("response.completed")
            );
        }
        provider.await.unwrap();
        forget_thread(&context);
        eprintln!("REAL_CLOCK: both subsequent HTTP-entry requests used actual upstream WS");
    }
}
