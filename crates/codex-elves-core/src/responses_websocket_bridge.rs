use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::handshake::server::{Request, create_response, write_response};
use tokio_tungstenite::tungstenite::http::{
    HeaderName, HeaderValue, Method, StatusCode, Uri, Version,
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role};
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};

use crate::settings::{RelayProtocol, SettingsStore};

const FRAME_SEND_TIMEOUT: Duration = Duration::from_secs(30);

pub fn is_responses_websocket_upgrade(request_bytes: &[u8]) -> bool {
    let Ok((request, _)) = parse_websocket_upgrade_request(request_bytes) else {
        return false;
    };
    is_responses_websocket_proxy_path(request.uri().path())
        && request
            .headers()
            .get("connection")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split([',', ' '])
                    .any(|token| token.eq_ignore_ascii_case("upgrade"))
            })
        && request
            .headers()
            .get("upgrade")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

pub fn is_responses_websocket_proxy_path(path: &str) -> bool {
    matches!(
        path,
        "/responses" | "/v1/responses" | "/v1/v1/responses" | "/codex/v1/responses"
    )
}

pub async fn handle_responses_websocket_connection(
    mut stream: TcpStream,
    request_bytes: Vec<u8>,
    remote_addr: Option<SocketAddr>,
) -> anyhow::Result<()> {
    let remote_addr = remote_addr.map(|address| address.to_string());
    let (request, trailing_bytes) = match parse_websocket_upgrade_request(&request_bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            reject_upgrade(&mut stream, StatusCode::BAD_REQUEST, &error.to_string()).await?;
            return Ok(());
        }
    };
    if !is_responses_websocket_proxy_path(request.uri().path()) {
        reject_upgrade(
            &mut stream,
            StatusCode::NOT_FOUND,
            "未知 Responses WebSocket 路径",
        )
        .await?;
        return Ok(());
    }

    let settings = SettingsStore::default().load().unwrap_or_default();
    let relay = settings.active_relay_profile();
    let rejection = if !settings.relay_profiles_enabled {
        Some("供应商功能已关闭")
    } else if settings.active_aggregate_relay_profile().is_some() {
        Some("聚合供应商暂不支持 Responses WebSocket")
    } else if !relay.local_proxy_enabled() {
        Some("当前供应商未启用本地代理")
    } else if !crate::responses_websocket::relay_prefers_native_responses_websocket(&relay) {
        Some("当前供应商没有可用的原生 Responses WebSocket 能力")
    } else {
        None
    };
    if let Some(message) = rejection {
        log_websocket_event(
            "helper.responses_websocket_rejected",
            &relay,
            remote_addr.as_deref(),
            Some(message),
        );
        reject_upgrade(&mut stream, StatusCode::CONFLICT, message).await?;
        return Ok(());
    }

    let original_user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok());
    let upstream = match crate::responses_websocket::open_responses_websocket_upstream(
        &relay,
        original_user_agent,
    )
    .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            log_websocket_event(
                "helper.responses_websocket_upstream_failed",
                &relay,
                remote_addr.as_deref(),
                Some(&error.to_string()),
            );
            reject_upgrade(
                &mut stream,
                StatusCode::BAD_GATEWAY,
                "Responses WebSocket 上游连接失败，已回退 HTTP",
            )
            .await?;
            return Ok(());
        }
    };
    if let Err(error) = ensure_websocket_relay_still_current(&relay) {
        reject_upgrade(&mut stream, StatusCode::CONFLICT, &error.to_string()).await?;
        return Ok(());
    }

    let response = match create_response(&request) {
        Ok(response) => response,
        Err(error) => {
            reject_upgrade(&mut stream, StatusCode::BAD_REQUEST, &error.to_string()).await?;
            return Ok(());
        }
    };
    let request_path = request.uri().path().to_string();
    let mut response_bytes = Vec::new();
    write_response(&mut response_bytes, &response)?;
    stream.write_all(&response_bytes).await?;
    let downstream =
        WebSocketStream::from_partially_read(stream, trailing_bytes, Role::Server, None).await;

    log_websocket_event(
        "helper.responses_websocket_connected",
        &relay,
        remote_addr.as_deref(),
        None,
    );
    let request_logger = WebSocketRequestLogger::new(&relay, remote_addr.clone(), request_path);
    let result = bridge_responses_websockets(downstream, upstream, &relay, request_logger).await;
    log_websocket_event(
        if result.is_ok() {
            "helper.responses_websocket_closed"
        } else {
            "helper.responses_websocket_failed"
        },
        &relay,
        remote_addr.as_deref(),
        result
            .as_ref()
            .err()
            .map(|error| error.to_string())
            .as_deref(),
    );
    result
}

async fn bridge_responses_websockets(
    downstream: WebSocketStream<TcpStream>,
    upstream: crate::responses_websocket::UpstreamResponsesWebsocket,
    relay: &crate::settings::RelayProfile,
    request_logger: WebSocketRequestLogger,
) -> anyhow::Result<()> {
    let (mut downstream_sink, mut downstream_stream) = downstream.split();
    let (mut upstream_sink, mut upstream_stream) = upstream.split();
    let (to_upstream_tx, mut to_upstream_rx) = mpsc::channel::<Message>(64);
    let (to_downstream_tx, mut to_downstream_rx) = mpsc::channel::<Message>(64);
    let continuation = WebSocketContinuationCoordinator::default();

    let upstream_writer = async move {
        while let Some(message) = to_upstream_rx.recv().await {
            if forward_websocket_message(
                &mut upstream_sink,
                message,
                "转发 Responses WebSocket 请求超时",
                "转发 Responses WebSocket 请求失败",
            )
            .await?
            {
                return Ok::<(), anyhow::Error>(());
            }
        }
        close_websocket_sink(
            &mut upstream_sink,
            "关闭 Responses WebSocket 上游超时",
            "关闭 Responses WebSocket 上游失败",
        )
        .await?;
        Ok::<(), anyhow::Error>(())
    };

    let downstream_writer = async move {
        while let Some(message) = to_downstream_rx.recv().await {
            if forward_websocket_message(
                &mut downstream_sink,
                message,
                "转发 Responses WebSocket 响应超时",
                "转发 Responses WebSocket 响应失败",
            )
            .await?
            {
                return Ok::<(), anyhow::Error>(());
            }
        }
        close_websocket_sink(
            &mut downstream_sink,
            "关闭本地 Responses WebSocket 超时",
            "关闭本地 Responses WebSocket 失败",
        )
        .await?;
        Ok::<(), anyhow::Error>(())
    };

    let relay = relay.clone();
    let downstream_close_tx = to_downstream_tx.clone();
    let upstream_close_tx = to_upstream_tx.clone();
    let continuation_upstream_tx = to_upstream_tx.clone();
    let downstream_request_logger = request_logger.clone();
    let downstream_continuation = continuation.clone();
    let downstream_reader = async move {
        let mut received_close = false;
        while let Some(message) = downstream_stream.next().await {
            let message = message.context("读取本地 Responses WebSocket 消息失败")?;
            let payload = match validate_downstream_message(&message, &relay) {
                Ok(payload) => payload,
                Err(error) => {
                    let close = Message::Close(Some(CloseFrame {
                        code: CloseCode::Policy,
                        reason: error.to_string().into(),
                    }));
                    let _ = downstream_close_tx.send(close.clone()).await;
                    let _ = upstream_close_tx.send(close).await;
                    return Ok::<(), anyhow::Error>(());
                }
            };
            if let (Some(payload), Message::Text(text)) = (payload.as_ref(), &message) {
                let log_id = downstream_request_logger.record_request(payload, text.as_str());
                if let Err(error) = downstream_continuation.register_request(payload, log_id) {
                    let close = Message::Close(Some(CloseFrame {
                        code: CloseCode::Policy,
                        reason: error.to_string().into(),
                    }));
                    let _ = downstream_close_tx.send(close.clone()).await;
                    let _ = upstream_close_tx.send(close).await;
                    return Ok::<(), anyhow::Error>(());
                }
            }
            let is_close = matches!(message, Message::Close(_));
            if to_upstream_tx.send(message).await.is_err() {
                if is_close {
                    return Ok::<(), anyhow::Error>(());
                }
                anyhow::bail!("Responses WebSocket 上游发送队列已关闭");
            }
            if is_close {
                received_close = true;
                break;
            }
        }
        if !received_close {
            let _ = to_upstream_tx.send(Message::Close(None)).await;
        }
        Ok::<(), anyhow::Error>(())
    };

    let upstream_request_logger = request_logger.clone();
    let upstream_continuation = continuation.clone();
    let upstream_reader = async move {
        let mut received_close = false;
        while let Some(message) = upstream_stream.next().await {
            let message = message.context("读取上游 Responses WebSocket 消息失败")?;
            let is_close = matches!(message, Message::Close(_));
            upstream_request_logger.record_first_response_event(&message);
            match upstream_continuation.handle_upstream_message(message)? {
                WebSocketContinuationAction::Forward(message) => {
                    upstream_request_logger.record_response(&message);
                    if to_downstream_tx.send(message).await.is_err() {
                        if is_close {
                            return Ok::<(), anyhow::Error>(());
                        }
                        anyhow::bail!("Responses WebSocket 本地发送队列已关闭");
                    }
                }
                WebSocketContinuationAction::Buffered => {}
                WebSocketContinuationAction::Continue { request, metadata } => {
                    upstream_request_logger.record_continue_metadata(&metadata);
                    if continuation_upstream_tx.send(request).await.is_err() {
                        anyhow::bail!("Responses WebSocket 续接请求发送队列已关闭");
                    }
                }
                WebSocketContinuationAction::Flush { messages, metadata } => {
                    upstream_request_logger.record_continue_metadata(&metadata);
                    for message in messages {
                        let message_is_close = matches!(message, Message::Close(_));
                        upstream_request_logger
                            .record_response_for(&message, metadata.log_id.as_deref());
                        if to_downstream_tx.send(message).await.is_err() {
                            if message_is_close {
                                return Ok::<(), anyhow::Error>(());
                            }
                            anyhow::bail!("Responses WebSocket 本地发送队列已关闭");
                        }
                    }
                }
            };
            if is_close {
                received_close = true;
                break;
            }
        }
        if !received_close {
            let _ = to_downstream_tx.send(Message::Close(None)).await;
        }
        Ok::<(), anyhow::Error>(())
    };

    let result = tokio::try_join!(
        upstream_writer,
        downstream_writer,
        downstream_reader,
        upstream_reader
    );
    match &result {
        Ok(_) => request_logger.finish_pending("Responses WebSocket 连接在响应完成前关闭", 499),
        Err(error) => request_logger.finish_pending(&error.to_string(), 502),
    }
    result?;
    Ok(())
}

#[derive(Clone, Default)]
struct WebSocketContinuationCoordinator {
    state: Arc<Mutex<WebSocketContinuationState>>,
}

#[derive(Default)]
struct WebSocketContinuationState {
    active: Option<ActiveWebSocketContinuation>,
}

struct ActiveWebSocketContinuation {
    original_request: Value,
    log_id: Option<String>,
    max_rounds: u32,
    round: u32,
    completed_rounds: u32,
    accumulated_reasoning_tokens: Option<u64>,
    buffered_messages: Vec<Message>,
    fallback_messages: Vec<Message>,
    fallback_response_body: Option<String>,
    continue_requests: Vec<Value>,
    before_response_body: Option<String>,
}

#[derive(Clone, Default)]
struct WebSocketContinueMetadata {
    log_id: Option<String>,
    triggered: bool,
    rounds: u32,
    reasoning_tokens: Option<u64>,
    request_body: Option<String>,
    before_response_body: Option<String>,
    after_response_body: Option<String>,
}

enum WebSocketContinuationAction {
    Forward(Message),
    Buffered,
    Continue {
        request: Message,
        metadata: WebSocketContinueMetadata,
    },
    Flush {
        messages: Vec<Message>,
        metadata: WebSocketContinueMetadata,
    },
}

impl WebSocketContinuationCoordinator {
    fn register_request(&self, payload: &Value, log_id: Option<String>) -> anyhow::Result<()> {
        let settings = SettingsStore::default()
            .load()
            .context("读取自动推理续接设置失败")?;
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Responses WebSocket 续接状态锁已损坏"))?;
        if state.active.is_some() {
            anyhow::bail!("自动推理续接期间不支持并发 response.create");
        }
        if !settings.gpt_reasoning_continuation
            || !crate::continue_thinking::is_supported_model(model)
        {
            return Ok(());
        }

        state.active = Some(ActiveWebSocketContinuation {
            original_request: payload.clone(),
            log_id,
            max_rounds: u32::from(settings.gpt_reasoning_continuation_max_rounds),
            round: 0,
            completed_rounds: 0,
            accumulated_reasoning_tokens: None,
            buffered_messages: Vec::new(),
            fallback_messages: Vec::new(),
            fallback_response_body: None,
            continue_requests: Vec::new(),
            before_response_body: None,
        });
        Ok(())
    }

    fn handle_upstream_message(
        &self,
        message: Message,
    ) -> anyhow::Result<WebSocketContinuationAction> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Responses WebSocket 续接状态锁已损坏"))?;
        let Some(active) = state.active.as_mut() else {
            return Ok(WebSocketContinuationAction::Forward(message));
        };

        if matches!(
            message,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_)
        ) {
            return Ok(WebSocketContinuationAction::Forward(message));
        }
        if matches!(message, Message::Close(_)) {
            let mut active = state.active.take().expect("active continuation must exist");
            let mut messages = if active.round > 0 && !active.fallback_messages.is_empty() {
                std::mem::take(&mut active.fallback_messages)
            } else {
                std::mem::take(&mut active.buffered_messages)
            };
            messages.push(message);
            let metadata =
                websocket_continue_metadata(&active, active.fallback_response_body.clone());
            return Ok(WebSocketContinuationAction::Flush { messages, metadata });
        }

        let Message::Text(text) = &message else {
            return Ok(WebSocketContinuationAction::Forward(message));
        };
        let payload = serde_json::from_str::<Value>(text.as_str()).ok();
        let event_type = payload
            .as_ref()
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        active.buffered_messages.push(message);
        if !is_terminal_websocket_response_event(event_type) {
            return Ok(WebSocketContinuationAction::Buffered);
        }

        let response_object = payload
            .as_ref()
            .and_then(|payload| payload.get("response"))
            .cloned();
        let reasoning_tokens = response_object
            .as_ref()
            .and_then(crate::continue_thinking::extract_reasoning_tokens);
        add_websocket_reasoning_tokens(&mut active.accumulated_reasoning_tokens, reasoning_tokens);
        active.completed_rounds = active.round;
        if active.round > 0 {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "continue_thinking.round_completed",
                serde_json::json!({
                    "transport": "ws",
                    "round": active.round,
                    "responseId": response_object
                        .as_ref()
                        .and_then(|response| response.get("id"))
                        .and_then(Value::as_str)
                }),
            );
        }

        let should_continue = matches!(event_type, "response.completed" | "response.incomplete")
            && response_object
                .as_ref()
                .is_some_and(crate::continue_thinking::should_continue_response)
            && active.round < active.max_rounds;
        if should_continue {
            let next_round = active.round + 1;
            let response_object = response_object
                .as_ref()
                .expect("continuation requires a terminal response object");
            let Some(continue_request) = crate::continue_thinking::build_websocket_continue_request(
                &active.original_request,
                response_object,
                next_round,
            ) else {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "continue_thinking.round_skipped",
                    serde_json::json!({
                        "transport": "ws",
                        "reason": "missing_latest_response_id",
                        "round": next_round
                    }),
                );
                let active = state.active.take().expect("active continuation must exist");
                return Ok(WebSocketContinuationAction::Flush {
                    messages: active.buffered_messages.clone(),
                    metadata: websocket_continue_metadata(&active, None),
                });
            };
            active.round = next_round;
            if active.before_response_body.is_none() {
                active.before_response_body = response_object
                    .as_object()
                    .and_then(|_| serde_json::to_string_pretty(response_object).ok());
            }
            active.continue_requests.push(serde_json::json!({
                "round": active.round,
                "mode": continue_request.mode.as_str(),
                "request": continue_request.request.clone()
            }));
            let request_text = serde_json::to_string(&continue_request.request)
                .context("序列化 Responses WebSocket 续接请求失败")?;
            active.fallback_messages = std::mem::take(&mut active.buffered_messages);
            active.fallback_response_body = serde_json::to_string_pretty(response_object).ok();
            let metadata = websocket_continue_metadata(active, None);
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "continue_thinking.round_start",
                serde_json::json!({
                    "transport": "ws",
                    "model": active
                        .original_request
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    "mode": continue_request.mode.as_str(),
                    "round": active.round,
                    "reasoningTokens": reasoning_tokens,
                    "gridMultiple": reasoning_tokens
                        .and_then(crate::continue_thinking::grid_multiple)
                }),
            );
            return Ok(WebSocketContinuationAction::Continue {
                request: Message::Text(request_text.into()),
                metadata,
            });
        }

        let active = state.active.take().expect("active continuation must exist");
        let after_response_body = if active.round > 0 {
            response_object
                .as_ref()
                .and_then(|response| serde_json::to_string_pretty(response).ok())
        } else {
            None
        };
        Ok(WebSocketContinuationAction::Flush {
            messages: active.buffered_messages.clone(),
            metadata: websocket_continue_metadata(&active, after_response_body),
        })
    }
}

fn add_websocket_reasoning_tokens(total: &mut Option<u64>, reasoning_tokens: Option<u64>) {
    if let Some(reasoning_tokens) = reasoning_tokens {
        *total = Some(total.unwrap_or(0).saturating_add(reasoning_tokens));
    }
}

fn websocket_continue_metadata(
    active: &ActiveWebSocketContinuation,
    after_response_body: Option<String>,
) -> WebSocketContinueMetadata {
    WebSocketContinueMetadata {
        log_id: active.log_id.clone(),
        triggered: active.round > 0,
        rounds: active.completed_rounds,
        reasoning_tokens: active.accumulated_reasoning_tokens,
        request_body: websocket_continue_request_body(&active.continue_requests),
        before_response_body: active.before_response_body.clone(),
        after_response_body,
    }
}

fn websocket_continue_request_body(requests: &[Value]) -> Option<String> {
    match requests {
        [] => None,
        [single] => single
            .get("request")
            .and_then(|request| serde_json::to_string_pretty(request).ok()),
        _ => serde_json::to_string_pretty(&serde_json::json!({ "rounds": requests })).ok(),
    }
}

#[derive(Clone)]
struct WebSocketRequestLogger {
    state: Arc<Mutex<WebSocketRequestLogState>>,
}

struct WebSocketRequestLogState {
    remote_addr: Option<String>,
    path: String,
    relay_id: String,
    relay_name: String,
    endpoint: Option<String>,
    requests: HashMap<String, TrackedWebSocketRequest>,
    active_order: VecDeque<String>,
    unassigned_order: VecDeque<String>,
    response_ids: HashMap<String, String>,
}

struct TrackedWebSocketRequest {
    record: crate::proxy_log::ProxyRequestRecord,
    started_at: Instant,
    response_capture: Vec<u8>,
    response_bytes: usize,
    response_truncated: bool,
}

impl WebSocketRequestLogger {
    fn new(
        relay: &crate::settings::RelayProfile,
        remote_addr: Option<String>,
        path: String,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(WebSocketRequestLogState {
                remote_addr,
                path,
                relay_id: relay.id.clone(),
                relay_name: relay.name.clone(),
                endpoint: crate::responses_websocket::responses_websocket_url(
                    crate::responses_websocket::relay_responses_base_url(relay),
                ),
                requests: HashMap::new(),
                active_order: VecDeque::new(),
                unassigned_order: VecDeque::new(),
                response_ids: HashMap::new(),
            })),
        }
    }

    fn record_request(&self, payload: &Value, request_body: &str) -> Option<String> {
        let metadata = crate::proxy_log::extract_request_metadata(Some(payload));
        let id = format!("local-{}", uuid::Uuid::new_v4());
        let timestamp_ms = crate::proxy_log::current_timestamp_ms();
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        let record = crate::proxy_log::ProxyRequestRecord {
            id: id.clone(),
            state: crate::proxy_log::ProxyRequestState::Pending,
            transport: crate::proxy_log::ProxyRequestTransport::Ws,
            timestamp_ms,
            method: "WS".to_string(),
            path: state.path.clone(),
            remote_addr: state.remote_addr.clone(),
            model: metadata.model,
            reasoning_tokens: None,
            reasoning_effort: metadata.reasoning_effort,
            reasoning_source: metadata.reasoning_source,
            continue_thinking_triggered: false,
            continue_thinking_rounds: 0,
            continue_thinking_request_body: None,
            continue_thinking_before_response_body: None,
            continue_thinking_after_response_body: None,
            service_tier: metadata.service_tier,
            relay_id: Some(state.relay_id.clone()),
            relay_name: Some(state.relay_name.clone()),
            endpoint: state.endpoint.clone(),
            response_protocol: Some("responses".to_string()),
            status_code: None,
            first_token_ms: None,
            duration_ms: None,
            stream: true,
            request_bytes: request_body.len(),
            response_bytes: None,
            response_captured_bytes: None,
            response_truncated: false,
            request_body: request_body.to_string(),
            response_body: String::new(),
            error: None,
        };
        state.requests.insert(
            id.clone(),
            TrackedWebSocketRequest {
                record: record.clone(),
                started_at: Instant::now(),
                response_capture: Vec::new(),
                response_bytes: 0,
                response_truncated: false,
            },
        );
        state.active_order.push_back(id.clone());
        state.unassigned_order.push_back(id.clone());
        drop(state);
        append_websocket_proxy_log_record(&record);
        Some(id)
    }

    fn record_response(&self, message: &Message) {
        self.record_response_for(message, None);
    }

    fn record_first_response_event(&self, message: &Message) {
        let Message::Text(text) = message else {
            return;
        };
        let Ok(payload) = serde_json::from_str::<Value>(text.as_str()) else {
            return;
        };
        let Some(event_type) = payload.get("type").and_then(Value::as_str) else {
            return;
        };
        if !event_type.starts_with("response.") && event_type != "error" {
            return;
        }

        let response_id = websocket_response_id(&payload);
        let mut update = None;
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(log_id) = resolve_websocket_log_id(&mut state, response_id.as_deref()) else {
            return;
        };
        if let Some(response_id) = response_id.as_deref() {
            state
                .response_ids
                .insert(response_id.to_string(), log_id.clone());
        }
        let Some(tracked) = state.requests.get_mut(&log_id) else {
            return;
        };
        if tracked.record.first_token_ms.is_none() {
            tracked.record.first_token_ms = Some(tracked.started_at.elapsed().as_millis() as u64);
            tracked.record.status_code = Some(200);
            update = Some(tracked.record.clone());
        }
        drop(state);
        if let Some(record) = update {
            append_websocket_proxy_log_record(&record);
        }
    }

    fn record_response_for(&self, message: &Message, preferred_log_id: Option<&str>) {
        let Message::Text(text) = message else {
            return;
        };
        let Ok(payload) = serde_json::from_str::<Value>(text.as_str()) else {
            return;
        };
        let Some(event_type) = payload.get("type").and_then(Value::as_str) else {
            return;
        };
        if !event_type.starts_with("response.") && event_type != "error" {
            return;
        }
        let response_id = websocket_response_id(&payload);
        let terminal = is_terminal_websocket_response_event(event_type);
        let failed = matches!(event_type, "response.failed" | "error");
        let mut update = None;
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let log_id = preferred_log_id
            .filter(|log_id| state.requests.contains_key(*log_id))
            .map(ToString::to_string)
            .or_else(|| resolve_websocket_log_id(&mut state, response_id.as_deref()));
        let Some(log_id) = log_id else {
            return;
        };
        if let Some(response_id) = response_id.as_deref() {
            state
                .response_ids
                .insert(response_id.to_string(), log_id.clone());
        }
        let Some(tracked) = state.requests.get_mut(&log_id) else {
            return;
        };
        let first_response_event = tracked.record.first_token_ms.is_none();
        if first_response_event {
            tracked.record.first_token_ms = Some(tracked.started_at.elapsed().as_millis() as u64);
            tracked.record.status_code = Some(200);
        }
        tracked.response_bytes = tracked
            .response_bytes
            .saturating_add(text.len().saturating_add(1));
        tracked.response_truncated |=
            crate::proxy_log::append_capture(&mut tracked.response_capture, text.as_bytes());
        tracked.response_truncated |=
            crate::proxy_log::append_capture(&mut tracked.response_capture, b"\n");

        if terminal {
            tracked.record.state = crate::proxy_log::ProxyRequestState::Completed;
            tracked.record.status_code = Some(if failed { 500 } else { 200 });
            tracked.record.duration_ms = Some(tracked.started_at.elapsed().as_millis() as u64);
            tracked.record.response_bytes = Some(tracked.response_bytes);
            tracked.record.response_captured_bytes = Some(tracked.response_capture.len());
            tracked.record.response_truncated = tracked.response_truncated;
            tracked.record.response_body =
                String::from_utf8_lossy(&tracked.response_capture).into_owned();
            let final_reasoning_tokens =
                crate::proxy_log::extract_reasoning_tokens_from_response_body(
                    &tracked.response_capture,
                );
            if tracked.record.reasoning_tokens.is_none() {
                tracked.record.reasoning_tokens = final_reasoning_tokens;
            }
            tracked.record.error = websocket_response_error(&payload, event_type);
            update = Some(tracked.record.clone());
        } else if first_response_event {
            update = Some(tracked.record.clone());
        }

        if terminal {
            state.requests.remove(&log_id);
            state.active_order.retain(|id| id != &log_id);
            state.unassigned_order.retain(|id| id != &log_id);
            state.response_ids.retain(|_, id| id != &log_id);
        }
        drop(state);
        if let Some(record) = update {
            append_websocket_proxy_log_record(&record);
        }
    }

    fn record_continue_metadata(&self, metadata: &WebSocketContinueMetadata) {
        let Some(log_id) = metadata.log_id.as_deref() else {
            return;
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(tracked) = state.requests.get_mut(log_id) else {
            return;
        };
        tracked.record.reasoning_tokens = metadata.reasoning_tokens;
        tracked.record.continue_thinking_triggered = metadata.triggered;
        tracked.record.continue_thinking_rounds = metadata.rounds;
        tracked.record.continue_thinking_request_body = metadata.request_body.clone();
        tracked.record.continue_thinking_before_response_body =
            metadata.before_response_body.clone();
        tracked.record.continue_thinking_after_response_body = metadata.after_response_body.clone();
        let record = tracked.record.clone();
        drop(state);
        append_websocket_proxy_log_record(&record);
    }

    fn finish_pending(&self, error: &str, status_code: u16) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let mut records = Vec::with_capacity(state.requests.len());
        for (_, mut tracked) in state.requests.drain() {
            tracked.record.state = crate::proxy_log::ProxyRequestState::Completed;
            tracked.record.status_code = Some(status_code);
            tracked.record.duration_ms = Some(tracked.started_at.elapsed().as_millis() as u64);
            tracked.record.response_bytes = Some(tracked.response_bytes);
            tracked.record.response_captured_bytes = Some(tracked.response_capture.len());
            tracked.record.response_truncated = tracked.response_truncated;
            tracked.record.response_body =
                String::from_utf8_lossy(&tracked.response_capture).into_owned();
            let captured_reasoning_tokens =
                crate::proxy_log::extract_reasoning_tokens_from_response_body(
                    &tracked.response_capture,
                );
            if tracked.record.reasoning_tokens.is_none() {
                tracked.record.reasoning_tokens = captured_reasoning_tokens;
            }
            tracked.record.error = Some(error.to_string());
            records.push(tracked.record);
        }
        state.active_order.clear();
        state.unassigned_order.clear();
        state.response_ids.clear();
        drop(state);
        for record in records {
            append_websocket_proxy_log_record(&record);
        }
    }
}

fn resolve_websocket_log_id(
    state: &mut WebSocketRequestLogState,
    response_id: Option<&str>,
) -> Option<String> {
    if let Some(response_id) = response_id {
        if let Some(log_id) = state.response_ids.get(response_id) {
            return Some(log_id.clone());
        }
        while let Some(log_id) = state.unassigned_order.pop_front() {
            if state.requests.contains_key(&log_id) {
                state
                    .response_ids
                    .insert(response_id.to_string(), log_id.clone());
                return Some(log_id);
            }
        }
    }

    let mut active = state
        .active_order
        .iter()
        .filter(|id| state.requests.contains_key(*id));
    let only = active.next()?.clone();
    if active.next().is_none() {
        Some(only)
    } else {
        None
    }
}

fn websocket_response_id(payload: &Value) -> Option<String> {
    payload
        .get("response_id")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("id"))
                .and_then(Value::as_str)
        })
        .map(ToString::to_string)
}

fn is_terminal_websocket_response_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "response.completed" | "response.incomplete" | "response.failed" | "error"
    )
}

fn websocket_response_error(payload: &Value, event_type: &str) -> Option<String> {
    if !matches!(event_type, "response.failed" | "error") {
        return None;
    }
    payload
        .pointer("/response/error/message")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .map(ToString::to_string)
        .or_else(|| Some(format!("Responses WebSocket 返回 {event_type}")))
}

fn append_websocket_proxy_log_record(record: &crate::proxy_log::ProxyRequestRecord) {
    if let Err(error) = crate::proxy_log::append_record(record) {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "helper.local_proxy_log_failed",
            serde_json::json!({
                "id": record.id,
                "transport": "ws",
                "error": error.to_string()
            }),
        );
    }
}

async fn forward_websocket_message<S>(
    sink: &mut S,
    message: Message,
    timeout_message: &'static str,
    failure_message: &'static str,
) -> anyhow::Result<bool>
where
    S: Sink<Message, Error = WebSocketError> + Unpin,
{
    let is_close = matches!(message, Message::Close(_));
    match tokio::time::timeout(FRAME_SEND_TIMEOUT, sink.send(message))
        .await
        .context(timeout_message)?
    {
        Ok(()) => {}
        Err(error) if is_close && is_expected_websocket_close_error(&error) => {
            match tokio::time::timeout(FRAME_SEND_TIMEOUT, sink.flush())
                .await
                .context(timeout_message)?
            {
                Ok(()) => {}
                Err(error) if is_expected_websocket_close_error(&error) => {}
                Err(error) => return Err(error).context(failure_message),
            }
        }
        Err(error) => return Err(error).context(failure_message),
    }
    Ok(is_close)
}

async fn close_websocket_sink<S>(
    sink: &mut S,
    timeout_message: &'static str,
    failure_message: &'static str,
) -> anyhow::Result<()>
where
    S: Sink<Message, Error = WebSocketError> + Unpin,
{
    match tokio::time::timeout(FRAME_SEND_TIMEOUT, sink.close())
        .await
        .context(timeout_message)?
    {
        Ok(()) => Ok(()),
        Err(error) if is_expected_websocket_close_error(&error) => Ok(()),
        Err(error) => Err(error).context(failure_message),
    }
}

fn is_expected_websocket_close_error(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::ConnectionClosed
            | WebSocketError::AlreadyClosed
            | WebSocketError::Protocol(ProtocolError::SendAfterClosing)
    )
}

fn validate_downstream_message(
    message: &Message,
    relay: &crate::settings::RelayProfile,
) -> anyhow::Result<Option<Value>> {
    let Message::Text(text) = message else {
        return Ok(None);
    };
    ensure_websocket_relay_still_current(relay)?;
    let payload: Value =
        serde_json::from_str(text.as_str()).context("Responses WebSocket 请求不是有效 JSON")?;
    if payload.get("type").and_then(Value::as_str) != Some("response.create") {
        anyhow::bail!("Responses WebSocket 仅支持 response.create 请求");
    }
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if model.is_empty() {
        anyhow::bail!("Responses WebSocket 请求缺少 model");
    }
    if relay.protocol_for_model(model) != RelayProtocol::Responses {
        anyhow::bail!("当前模型不是原生 Responses 协议");
    }
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.responses_websocket_request",
        serde_json::json!({
            "relayId": relay.id,
            "relayName": relay.name,
            "endpoint": crate::responses_websocket::responses_websocket_url(
                crate::responses_websocket::relay_responses_base_url(relay)
            ),
            "model": model,
        }),
    );
    Ok(Some(payload))
}

fn ensure_websocket_relay_still_current(
    connected_relay: &crate::settings::RelayProfile,
) -> anyhow::Result<()> {
    let settings = SettingsStore::default()
        .load()
        .context("读取当前供应商设置失败")?;
    if !settings.relay_profiles_enabled || settings.active_aggregate_relay_profile().is_some() {
        anyhow::bail!("当前设置已不再允许 Responses WebSocket");
    }
    let current = settings.active_relay_profile();
    if current.id != connected_relay.id
        || !current.local_proxy_enabled()
        || !crate::responses_websocket::relay_prefers_native_responses_websocket(&current)
        || crate::responses_websocket::relay_responses_base_url(&current).trim()
            != crate::responses_websocket::relay_responses_base_url(connected_relay).trim()
        || current.api_key != connected_relay.api_key
        || current.user_agent != connected_relay.user_agent
    {
        anyhow::bail!("当前供应商已变化，请重新建立 Responses WebSocket");
    }
    Ok(())
}

fn parse_websocket_upgrade_request(request_bytes: &[u8]) -> anyhow::Result<(Request, Vec<u8>)> {
    let header_end = request_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .context("WebSocket Upgrade 请求头不完整")?;
    let head_end = header_end + 4;
    let head = std::str::from_utf8(&request_bytes[..head_end])
        .context("WebSocket Upgrade 请求头不是 UTF-8")?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().context("WebSocket Upgrade 请求行缺失")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().context("WebSocket 请求方法缺失")?;
    let uri = request_parts.next().context("WebSocket 请求路径缺失")?;
    let version = request_parts.next().context("WebSocket HTTP 版本缺失")?;
    if request_parts.next().is_some() {
        anyhow::bail!("WebSocket Upgrade 请求行无效");
    }

    let mut request = Request::builder()
        .method(Method::from_bytes(method.as_bytes())?)
        .uri(Uri::try_from(uri)?)
        .version(match version {
            "HTTP/1.1" => Version::HTTP_11,
            "HTTP/1.0" => Version::HTTP_10,
            _ => anyhow::bail!("不支持的 WebSocket HTTP 版本"),
        })
        .body(())?;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .context("WebSocket Upgrade 请求头格式无效")?;
        request.headers_mut().append(
            HeaderName::from_bytes(name.trim().as_bytes())?,
            HeaderValue::from_str(value.trim())?,
        );
    }

    Ok((request, request_bytes[head_end..].to_vec()))
}

async fn reject_upgrade(
    stream: &mut TcpStream,
    status: StatusCode,
    message: &str,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(&serde_json::json!({
        "status": "failed",
        "message": message,
    }))?;
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("Error"),
        body.len(),
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await?;
    Ok(())
}

fn log_websocket_event(
    event: &str,
    relay: &crate::settings::RelayProfile,
    remote_addr: Option<&str>,
    error: Option<&str>,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        event,
        serde_json::json!({
            "relayId": relay.id,
            "relayName": relay.name,
            "endpoint": crate::responses_websocket::responses_websocket_url(
                crate::responses_websocket::relay_responses_base_url(relay)
            ),
            "remoteAddr": remote_addr,
            "error": error,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveWebSocketContinuation, WebSocketContinuationAction, WebSocketContinuationCoordinator,
        WebSocketContinuationState, is_responses_websocket_proxy_path,
        is_responses_websocket_upgrade,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn only_responses_upgrade_paths_are_accepted() {
        assert!(is_responses_websocket_proxy_path("/v1/responses"));
        assert!(!is_responses_websocket_proxy_path("/v1/responses/compact"));
        assert!(!is_responses_websocket_proxy_path("/v1/chat/completions"));
    }

    #[test]
    fn detects_websocket_upgrade_without_consuming_trailing_frame_bytes() {
        let mut request = b"GET /v1/responses HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive, Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n".to_vec();
        request.extend_from_slice(&[0x81, 0x00]);

        assert!(is_responses_websocket_upgrade(&request));
    }

    #[test]
    fn continuation_close_falls_back_to_last_completed_round() {
        let first_terminal = Message::Text(
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_short",
                    "output": [{
                        "type": "reasoning",
                        "encrypted_content": "encrypted-short"
                    }],
                    "usage": {
                        "output_tokens_details": {
                            "reasoning_tokens": 516
                        }
                    }
                }
            })
            .to_string()
            .into(),
        );
        let coordinator = WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    original_request: json!({
                        "type": "response.create",
                        "model": "gpt-test",
                        "input": []
                    }),
                    log_id: Some("log-1".to_string()),
                    max_rounds: 3,
                    round: 0,
                    completed_rounds: 0,
                    accumulated_reasoning_tokens: None,
                    buffered_messages: Vec::new(),
                    fallback_messages: Vec::new(),
                    fallback_response_body: None,
                    continue_requests: Vec::new(),
                    before_response_body: None,
                }),
            })),
        };

        let action = coordinator
            .handle_upstream_message(first_terminal.clone())
            .unwrap();
        assert!(matches!(
            action,
            WebSocketContinuationAction::Continue { .. }
        ));

        let action = coordinator
            .handle_upstream_message(Message::Close(None))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, metadata } = action else {
            panic!("expected fallback flush");
        };
        assert_eq!(messages.first(), Some(&first_terminal));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
        assert_eq!(metadata.reasoning_tokens, Some(516));
        assert_eq!(metadata.rounds, 0);
        assert!(
            metadata
                .after_response_body
                .as_deref()
                .is_some_and(|body| body.contains("resp_short"))
        );
    }
}
