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
    } else if settings.gpt_reasoning_continuation {
        Some("GPT 推理续接启用时使用 HTTP/SSE")
    } else if !crate::responses_websocket::relay_supports_native_responses_websocket(&relay) {
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
    let downstream_request_logger = request_logger.clone();
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
                downstream_request_logger.record_request(payload, text.as_str());
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
    let upstream_reader = async move {
        let mut received_close = false;
        while let Some(message) = upstream_stream.next().await {
            let message = message.context("读取上游 Responses WebSocket 消息失败")?;
            upstream_request_logger.record_response(&message);
            let is_close = matches!(message, Message::Close(_));
            if to_downstream_tx.send(message).await.is_err() {
                if is_close {
                    return Ok::<(), anyhow::Error>(());
                }
                anyhow::bail!("Responses WebSocket 本地发送队列已关闭");
            }
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

    fn record_request(&self, payload: &Value, request_body: &str) {
        let metadata = crate::proxy_log::extract_request_metadata(Some(payload));
        let id = format!("local-{}", uuid::Uuid::new_v4());
        let timestamp_ms = crate::proxy_log::current_timestamp_ms();
        let Ok(mut state) = self.state.lock() else {
            return;
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
        state.unassigned_order.push_back(id);
        drop(state);
        append_websocket_proxy_log_record(&record);
    }

    fn record_response(&self, message: &Message) {
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
        let Some(log_id) = resolve_websocket_log_id(&mut state, response_id.as_deref()) else {
            return;
        };
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
            tracked.record.reasoning_tokens =
                crate::proxy_log::extract_reasoning_tokens_from_response_body(
                    &tracked.response_capture,
                );
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
            tracked.record.reasoning_tokens =
                crate::proxy_log::extract_reasoning_tokens_from_response_body(
                    &tracked.response_capture,
                );
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
    if !settings.relay_profiles_enabled
        || settings.active_aggregate_relay_profile().is_some()
        || settings.gpt_reasoning_continuation
    {
        anyhow::bail!("当前设置已不再允许 Responses WebSocket");
    }
    let current = settings.active_relay_profile();
    if current.id != connected_relay.id
        || !current.local_proxy_enabled()
        || !crate::responses_websocket::relay_supports_native_responses_websocket(&current)
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
    use super::{is_responses_websocket_proxy_path, is_responses_websocket_upgrade};

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
}
