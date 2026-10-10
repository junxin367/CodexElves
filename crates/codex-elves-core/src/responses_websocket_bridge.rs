use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
#[cfg(test)]
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
const UPSTREAM_LIVENESS_PING_INTERVAL: Duration = Duration::from_secs(30);
const UPSTREAM_LIVENESS_PONG_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_LIVENESS_TOLERATED_TIMEOUTS: usize = 3;
const UPSTREAM_APPLICATION_IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const OVERSIZED_REQUEST_HTTP_FALLBACK_TTL: Duration = Duration::from_secs(30);
const EARLY_DISCONNECT_HTTP_FALLBACK_TTL: Duration = Duration::from_secs(30);
const UPSTREAM_FAILURE_HTTP_FALLBACK_TTL: Duration = Duration::from_secs(30);
const INITIAL_UPSTREAM_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(300), Duration::from_secs(1)];
const MODEL_CAPACITY_RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];
static NEXT_WEBSOCKET_CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct WebSocketTransportLiveness {
    next_ping_sequence: u64,
    consecutive_timeouts: usize,
    pending_ping: Option<PendingWebSocketPing>,
    last_ping_at: Option<Instant>,
    last_pong_at: Option<Instant>,
    last_pong_rtt: Option<Duration>,
    last_transport_activity_at: Option<Instant>,
}

struct PendingWebSocketPing {
    payload: Vec<u8>,
    sent_at: Instant,
}

impl WebSocketTransportLiveness {
    pub(crate) fn begin_ping(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.pending_ping.is_some() {
            return None;
        }
        self.next_ping_sequence = self.next_ping_sequence.wrapping_add(1);
        let payload = format!("codex-elves:{}", self.next_ping_sequence).into_bytes();
        self.pending_ping = Some(PendingWebSocketPing {
            payload: payload.clone(),
            sent_at: now,
        });
        self.last_ping_at = Some(now);
        Some(payload)
    }

    pub(crate) fn observe_frame(&mut self, message: &Message, now: Instant) -> Option<Duration> {
        self.last_transport_activity_at = Some(now);
        self.consecutive_timeouts = 0;
        let pending_ping = self.pending_ping.take();
        let matched_pong = pending_ping.as_ref().is_some_and(|pending| {
            matches!(message, Message::Pong(payload) if payload.as_ref() == pending.payload.as_slice())
        });
        let pong_rtt = matched_pong
            .then(|| {
                pending_ping
                    .as_ref()
                    .map(|pending| now.duration_since(pending.sent_at))
            })
            .flatten();
        if matched_pong {
            self.last_pong_at = Some(now);
            self.last_pong_rtt = pong_rtt;
        }
        pong_rtt
    }

    fn timeout_elapsed(&self, timeout: Duration, now: Instant) -> Option<Duration> {
        let pending = self.pending_ping.as_ref()?;
        let elapsed = now.duration_since(pending.sent_at);
        (elapsed >= timeout).then_some(elapsed)
    }

    pub(crate) fn take_timeout(&mut self, timeout: Duration, now: Instant) -> Option<Duration> {
        let elapsed = self.timeout_elapsed(timeout, now)?;
        // 每个实际发出的 Ping 只计数一次，允许下一个定时 Ping 继续探测。
        self.pending_ping = None;
        self.consecutive_timeouts += 1;
        Some(elapsed)
    }

    pub(crate) fn timeout_limit_exceeded(&self) -> bool {
        self.consecutive_timeouts > UPSTREAM_LIVENESS_TOLERATED_TIMEOUTS
    }
}

#[cfg(test)]
async fn queue_upstream_liveness_ping(
    upstream_tx: &mpsc::Sender<Message>,
    liveness: &mut WebSocketTransportLiveness,
    now: Instant,
) -> anyhow::Result<()> {
    let Some(payload) = liveness.begin_ping(now) else {
        return Ok(());
    };
    upstream_tx
        .send(Message::Ping(payload.into()))
        .await
        .map_err(|_| anyhow::anyhow!("Responses WebSocket 上游发送队列已关闭"))
}

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
    let connection_context = WebSocketConnectionContext::from_request(&request);
    let request_context =
        crate::request_headers::RequestContext::from_headers(request.headers().clone());

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
            &connection_context,
            Some(message),
        );
        // Codex 仅把 426 识别为“该端点不具备 Responses WebSocket 能力”并立即回退 HTTP。
        // 临时连接故障仍使用 502，避免把偶发故障误判为永久不支持。
        reject_upgrade(&mut stream, StatusCode::UPGRADE_REQUIRED, message).await?;
        return Ok(());
    }
    if crate::session_transport::should_use_http(&relay, &request_context) {
        reject_upgrade(
            &mut stream,
            StatusCode::UPGRADE_REQUIRED,
            "当前会话暂用 HTTP，后台每 3 分钟尝试恢复 WS",
        )
        .await?;
        return Ok(());
    }
    if should_temporarily_fallback_oversized_responses_websocket_to_http(
        &relay.id,
        &connection_context,
    ) {
        let message = "当前会话轮次的请求超过 Responses WebSocket 消息上限，临时改走 HTTP";
        log_websocket_event(
            "helper.responses_websocket_http_fallback",
            &relay,
            remote_addr.as_deref(),
            &connection_context,
            Some(message),
        );
        reject_upgrade(&mut stream, StatusCode::UPGRADE_REQUIRED, message).await?;
        return Ok(());
    }
    if should_temporarily_fallback_early_disconnect_responses_websocket_to_http(
        &relay.id,
        &connection_context,
    ) {
        let message = "当前会话轮次的 Responses WebSocket 在首个请求前异常断开，临时回退 HTTP";
        log_websocket_event(
            "helper.responses_websocket_http_fallback",
            &relay,
            remote_addr.as_deref(),
            &connection_context,
            Some(message),
        );
        reject_upgrade(&mut stream, StatusCode::UPGRADE_REQUIRED, message).await?;
        return Ok(());
    }
    if should_temporarily_fallback_upstream_failure_responses_websocket_to_http(
        &relay.id,
        &connection_context,
    ) {
        let message = "当前会话轮次的 Responses WebSocket 上游连续失败，临时回退 HTTP";
        log_websocket_event(
            "helper.responses_websocket_http_fallback",
            &relay,
            remote_addr.as_deref(),
            &connection_context,
            Some(message),
        );
        reject_upgrade(&mut stream, StatusCode::UPGRADE_REQUIRED, message).await?;
        return Ok(());
    }

    let upstream = match open_responses_websocket_upstream_with_initial_retries(
        &relay,
        &request_context,
    )
    .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            crate::session_transport::mark_http(&relay, &request_context, &format!("{error:#}"));
            let fallback_armed = arm_upstream_failure_responses_websocket_http_fallback(
                &relay.id,
                &connection_context,
            );
            log_websocket_event(
                "helper.responses_websocket_upstream_failed",
                &relay,
                remote_addr.as_deref(),
                &connection_context,
                Some(&error.to_string()),
            );
            if fallback_armed {
                log_websocket_event(
                    "helper.responses_websocket_http_fallback_armed",
                    &relay,
                    remote_addr.as_deref(),
                    &connection_context,
                    Some("初始上游连接重试耗尽"),
                );
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "protocol_proxy.responses_websocket_http_fallback_armed",
                    serde_json::json!({
                        "relayId": relay.id,
                        "relayName": relay.name,
                        "reason": "initial_upstream_connection_exhausted",
                        "retryCount": INITIAL_UPSTREAM_RETRY_DELAYS.len(),
                    }),
                );
            }
            reject_upgrade(
                &mut stream,
                StatusCode::BAD_GATEWAY,
                "Responses WebSocket 上游连接失败，Codex 将按客户端重试策略处理",
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
    let downstream = WebSocketStream::from_partially_read(
        stream,
        trailing_bytes,
        Role::Server,
        Some(crate::responses_websocket::responses_websocket_config()),
    )
    .await;

    log_websocket_event(
        "helper.responses_websocket_connected",
        &relay,
        remote_addr.as_deref(),
        &connection_context,
        None,
    );
    let request_logger = WebSocketRequestLogger::new(
        &relay,
        remote_addr.clone(),
        request_path,
        connection_context.clone(),
    );
    request_logger.set_transport_context(relay.clone(), request_context.clone());
    let result = bridge_responses_websockets(
        downstream,
        upstream,
        &relay,
        request_logger.clone(),
        request_context,
    )
    .await;
    if !request_logger.has_recorded_application_requests()
        && arm_early_disconnect_responses_websocket_http_fallback(&relay.id, &connection_context)
    {
        log_websocket_event(
            "helper.responses_websocket_http_fallback_armed",
            &relay,
            remote_addr.as_deref(),
            &connection_context,
            Some("本地连接在首个 response.create 前结束"),
        );
    }
    if result.is_err()
        && request_logger.has_recorded_application_requests()
        && request_logger.interrupted_before_response()
    {
        let fallback_armed =
            arm_upstream_failure_responses_websocket_http_fallback(&relay.id, &connection_context);
        if fallback_armed {
            log_websocket_event(
                "helper.responses_websocket_http_fallback_armed",
                &relay,
                remote_addr.as_deref(),
                &connection_context,
                Some("首个应用响应前的上游重连耗尽"),
            );
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.responses_websocket_http_fallback_armed",
                serde_json::json!({
                    "relayId": relay.id,
                    "relayName": relay.name,
                    "reason": "upstream_failure_before_first_application_response",
                    "retryCount": INITIAL_UPSTREAM_RETRY_DELAYS.len(),
                }),
            );
        }
    }
    let error_detail = result.as_ref().err().map(|error| format!("{error:#}"));
    log_websocket_event(
        if result.is_ok() {
            "helper.responses_websocket_closed"
        } else {
            "helper.responses_websocket_failed"
        },
        &relay,
        remote_addr.as_deref(),
        &connection_context,
        error_detail.as_deref(),
    );
    result
}

async fn bridge_responses_websockets(
    mut downstream: WebSocketStream<TcpStream>,
    mut upstream: crate::responses_websocket::UpstreamResponsesWebsocket,
    relay: &crate::settings::RelayProfile,
    request_logger: WebSocketRequestLogger,
    request_context: crate::request_headers::RequestContext,
) -> anyhow::Result<()> {
    let continuation = WebSocketContinuationCoordinator::default();
    enum BridgeEvent {
        Downstream(Option<Result<Message, WebSocketError>>),
        Upstream(Option<Result<Message, WebSocketError>>),
        LivenessCheck { send_ping: bool },
    }

    let relay = relay.clone();
    let request_context = request_context.clone();
    let mut downstream_closed = false;
    let mut upstream_closed = false;
    let mut reconnect_attempts = 0usize;
    let mut queued_downstream = VecDeque::new();
    let mut liveness_ping = tokio::time::interval_at(
        tokio::time::Instant::now() + UPSTREAM_LIVENESS_PING_INTERVAL,
        UPSTREAM_LIVENESS_PING_INTERVAL,
    );
    liveness_ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut application_idle_check = tokio::time::interval_at(
        tokio::time::Instant::now() + UPSTREAM_APPLICATION_IDLE_CHECK_INTERVAL,
        UPSTREAM_APPLICATION_IDLE_CHECK_INTERVAL,
    );
    application_idle_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut transport_liveness = WebSocketTransportLiveness::default();

    let result: anyhow::Result<()> = async {
        'bridge: loop {
            let event = if let Some(message) = queued_downstream.pop_front() {
                BridgeEvent::Downstream(Some(Ok(message)))
            } else {
                tokio::select! {
                    biased;
                    message = downstream.next(), if !downstream_closed => {
                        BridgeEvent::Downstream(message)
                    }
                    message = upstream.next() => BridgeEvent::Upstream(message),
                    _ = liveness_ping.tick() => BridgeEvent::LivenessCheck { send_ping: true },
                    _ = application_idle_check.tick() => BridgeEvent::LivenessCheck { send_ping: false },
                }
            };

            match event {
                BridgeEvent::Downstream(message) => {
                    let Some(message) = message else {
                        downstream_closed = true;
                        break;
                    };
                    let message = message.context("读取本地 Responses WebSocket 消息失败")?;
                    let payload = match validate_downstream_message(&message, &relay) {
                        Ok(payload) => payload,
                        Err(error) => {
                            let close = Message::Close(Some(CloseFrame {
                                code: CloseCode::Policy,
                                reason: error.to_string().into(),
                            }));
                            let _ = forward_websocket_message(
                                &mut downstream,
                                close.clone(),
                                "关闭本地 Responses WebSocket 超时",
                                "关闭本地 Responses WebSocket 失败",
                            )
                            .await;
                            let _ = forward_websocket_message(
                                &mut upstream,
                                close,
                                "关闭 Responses WebSocket 上游超时",
                                "关闭 Responses WebSocket 上游失败",
                            )
                            .await;
                            downstream_closed = true;
                            upstream_closed = true;
                            break;
                        }
                    };
                    let (
                        message,
                        request_payload,
                        layered_compaction_options,
                        request_settings,
                        compaction_plan,
                    ) = if let Some((payload, settings, payload_rewritten)) = payload {
                        let (
                            request_payload,
                            forwarded_payload,
                            layered_compaction_options,
                            compaction_plan,
                        ) = prepare_downstream_response_create_payload_with_snapshot(
                            &payload,
                            &settings,
                            &relay,
                            &request_context,
                        )
                        .await;
                        let message = if !payload_rewritten && forwarded_payload == payload {
                            message
                        } else {
                            Message::Text(
                                serde_json::to_string(&forwarded_payload)
                                    .context("序列化处理后的 Responses WebSocket 请求失败")?
                                    .into(),
                            )
                        };
                        (
                            message,
                            Some(request_payload),
                            layered_compaction_options,
                            Some(settings),
                            compaction_plan,
                        )
                    } else {
                        (message, None, None, None, None)
                    };

                    if let Some(request_payload) = request_payload.as_ref() {
                        if crate::layered_compaction::is_any_compaction_request(request_payload)
                            && request_logger.has_pending_requests()
                        {
                            let close = Message::Close(Some(CloseFrame {
                                code: CloseCode::Policy,
                                reason: "compaction cannot overlap an active response".into(),
                            }));
                            let _ = forward_websocket_message(
                                &mut downstream,
                                close.clone(),
                                "关闭本地 Responses WebSocket 超时",
                                "关闭本地 Responses WebSocket 失败",
                            )
                            .await;
                            let _ = forward_websocket_message(
                                &mut upstream,
                                close,
                                "关闭 Responses WebSocket 上游超时",
                                "关闭 Responses WebSocket 上游失败",
                            )
                            .await;
                            downstream_closed = true;
                            upstream_closed = true;
                            break;
                        }
                        if !request_logger.has_pending_requests() {
                            reconnect_attempts = 0;
                        }
                        if let Some(response_messages) =
                            local_compaction_wait_websocket_messages(request_payload)
                        {
                            if let Message::Text(text) = &message {
                                let _ = request_logger.record_request(request_payload, text.as_str());
                            }
                            let model = request_payload
                                .get("model")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let _ = crate::diagnostic_log::append_diagnostic_log(
                                "protocol_proxy.local_compaction_wait_for_user",
                                serde_json::json!({
                                    "transport": "ws",
                                    "model": model,
                                    "reason": "restored local compaction history ends on the assistant side"
                                }),
                            );
                            for response_message in response_messages {
                                if forward_downstream_response(
                                    &mut downstream,
                                    response_message,
                                    &request_logger,
                                    None,
                                )
                                .await?
                                {
                                    downstream_closed = true;
                                    break 'bridge;
                                }
                            }
                            continue;
                        }
                    }

                    if let (Some(request_payload), Message::Text(text)) =
                        (request_payload.as_ref(), &message)
                    {
                        crate::session_transport::observe_native_request(
                            &relay, &request_context, request_payload, text.len(),
                        );
                        let log_id = request_logger.record_request(request_payload, text.as_str());
                        if text.len()
                            > crate::responses_websocket::RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES
                        {
                            request_logger.finish_oversized_request(
                                log_id.as_deref(),
                                text.len(),
                                crate::responses_websocket::RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES,
                            );
                            let downstream_close = Message::Close(Some(
                                websocket_failure_close_frame(
                                    CloseCode::Size,
                                    "request exceeds upstream websocket limit; retry over HTTP",
                                ),
                            ));
                            let upstream_close = Message::Close(Some(CloseFrame {
                                code: CloseCode::Normal,
                                reason: "oversized request routed to HTTP".into(),
                            }));
                            let _ = forward_websocket_message(
                                &mut downstream,
                                downstream_close,
                                "关闭本地 Responses WebSocket 超时",
                                "关闭本地 Responses WebSocket 失败",
                            )
                            .await;
                            let _ = forward_websocket_message(
                                &mut upstream,
                                upstream_close,
                                "关闭 Responses WebSocket 上游超时",
                                "关闭 Responses WebSocket 上游失败",
                            )
                            .await;
                            downstream_closed = true;
                            upstream_closed = true;
                            break;
                        }
                        request_logger.record_independent_compaction(
                            log_id.as_deref(),
                            compaction_plan
                                .as_ref()
                                .and_then(|plan| plan.independent_compaction_model.as_deref()),
                            compaction_plan
                                .as_ref()
                                .and_then(|plan| plan.independent_compaction_usage),
                        );
                        if let Err(error) = continuation
                            .register_request_with_settings_and_plan(
                                request_payload,
                                log_id,
                                layered_compaction_options,
                                request_settings
                                    .as_ref()
                                    .expect("response.create should include a settings snapshot"),
                                compaction_plan,
                            )
                        {
                            let close = Message::Close(Some(CloseFrame {
                                code: CloseCode::Policy,
                                reason: error.to_string().into(),
                            }));
                            let _ = forward_websocket_message(
                                &mut downstream,
                                close.clone(),
                                "关闭本地 Responses WebSocket 超时",
                                "关闭本地 Responses WebSocket 失败",
                            )
                            .await;
                            let _ = forward_websocket_message(
                                &mut upstream,
                                close,
                                "关闭 Responses WebSocket 上游超时",
                                "关闭 Responses WebSocket 上游失败",
                            )
                            .await;
                            downstream_closed = true;
                            upstream_closed = true;
                            break;
                        }
                    }

                    let is_close = matches!(message, Message::Close(_));
                    if forward_websocket_message(
                        &mut upstream,
                        message,
                        "转发 Responses WebSocket 请求超时",
                        "转发 Responses WebSocket 请求失败",
                    )
                    .await.map_err(|error| {
                        if !is_close {
                            request_logger.mark_http_fallback("发送 WS 请求失败");
                        }
                        error
                    })?
                    {
                        upstream_closed = true;
                        downstream_closed = true;
                        break;
                    }
                    if is_close {
                        downstream_closed = true;
                        break;
                    }
                }
                BridgeEvent::Upstream(message) => {
                    let message = match message {
                        Some(Ok(message)) => message,
                        Some(Err(_)) | None if continuation.has_validated_compaction() => {
                            Message::Close(None)
                        }
                        Some(Err(error)) => {
                            let error_message = format!("upstream WebSocket read failed: {error}");
                            if let Some(WebSocketContinuationAction::Flush { messages, metadata }) =
                                continuation
                                    .fail_active_compaction("websocket_read_failed", &error_message)?
                            {
                                request_logger.record_continue_metadata(&metadata);
                                for message in messages {
                                    let message_is_close = matches!(message, Message::Close(_));
                                    if forward_downstream_response(
                                        &mut downstream,
                                        message,
                                        &request_logger,
                                        metadata.log_id.as_deref(),
                                    )
                                    .await?
                                    {
                                        downstream_closed = true;
                                        break;
                                    }
                                    if message_is_close {
                                        downstream_closed = true;
                                        break;
                                    }
                                }
                                if downstream_closed {
                                    break;
                                }
                                continue;
                            }
                            if request_logger.upstream_replay_block_reason().is_none()
                                && reconnect_attempts < INITIAL_UPSTREAM_RETRY_DELAYS.len()
                            {
                                match recover_upstream_connection(
                                    upstream,
                                    &mut downstream,
                                    &mut queued_downstream,
                                    &relay,
                                    &request_context,
                                    &request_logger,
                                    &mut reconnect_attempts,
                                    "read_error",
                                )
                                .await
                                {
                                    Ok(Some(new_upstream)) => {
                                        upstream = new_upstream;
                                        transport_liveness = WebSocketTransportLiveness::default();
                                        liveness_ping.reset();
                                        continue 'bridge;
                                    }
                                    Ok(None) => {
                                        downstream_closed = true;
                                        return Ok(());
                                    }
                                    Err(reconnect_error) => {
                                        let _ = send_downstream_terminal_failure(
                                            &mut downstream,
                                            &request_logger,
                                            &format!(
                                                "{error_message}; 最后一次重连失败：{reconnect_error:#}"
                                            ),
                                        )
                                        .await;
                                        downstream_closed = true;
                                        anyhow::bail!(
                                            "读取上游 Responses WebSocket 消息失败，内部重连已耗尽：{error_message}; 最后一次重连失败：{reconnect_error:#}"
                                        );
                                    }
                                }
                            }
                            let _ = send_downstream_terminal_failure(
                                &mut downstream,
                                &request_logger,
                                &error_message,
                            )
                            .await;
                            downstream_closed = true;
                            anyhow::bail!("读取上游 Responses WebSocket 消息失败：{error_message}");
                        }
                        None => {
                            if let Some(WebSocketContinuationAction::Flush { messages, metadata }) =
                                continuation.fail_active_compaction(
                                    "websocket_ended",
                                    "upstream WebSocket ended before a terminal response.",
                                )?
                            {
                                request_logger.record_continue_metadata(&metadata);
                                for message in messages {
                                    let message_is_close = matches!(message, Message::Close(_));
                                    if forward_downstream_response(
                                        &mut downstream,
                                        message,
                                        &request_logger,
                                        metadata.log_id.as_deref(),
                                    )
                                    .await?
                                    {
                                        downstream_closed = true;
                                        break;
                                    }
                                    if message_is_close {
                                        downstream_closed = true;
                                        break;
                                    }
                                }
                                if downstream_closed {
                                    break;
                                }
                                continue;
                            }
                            let _ = send_downstream_terminal_failure(
                                &mut downstream,
                                &request_logger,
                                "Responses WebSocket 上游未发送 Close 帧就结束连接",
                            )
                            .await;
                            downstream_closed = true;
                            anyhow::bail!("Responses WebSocket 上游未发送 Close 帧就结束连接");
                        }
                    };

                    if transport_liveness.consecutive_timeouts > 0 {
                        request_logger.log_transport_recovered(
                            transport_liveness.consecutive_timeouts,
                            &message,
                        );
                    }
                    let _ = transport_liveness.observe_frame(&message, Instant::now());
                    request_logger.record_upstream_application_activity(&message);
                    let is_close = matches!(message, Message::Close(_));
                    if is_close && request_logger.has_pending_requests() {
                        request_logger.mark_http_fallback("上游在请求完成前关闭 WS");
                    }
                    request_logger.record_first_response_event(&message);
                    match continuation.handle_upstream_message(message)? {
                        WebSocketContinuationAction::Forward(message) => {
                            if forward_downstream_response(
                                &mut downstream,
                                message,
                                &request_logger,
                                None,
                            )
                            .await?
                            {
                                downstream_closed = true;
                                break;
                            }
                        }
                        WebSocketContinuationAction::Buffered => {}
                        WebSocketContinuationAction::Continue { request, metadata } => {
                            request_logger.record_continue_metadata(&metadata);
                            if let Some(delay) = metadata.retry_delay {
                                tokio::time::sleep(delay).await;
                            }
                            let retry_result = async {
                                if metadata.reconnect_upstream {
                                    upstream = crate::responses_websocket::open_responses_websocket_upstream_with_request_context(
                                        &relay, &request_context,
                                    ).await?;
                                    transport_liveness = WebSocketTransportLiveness::default();
                                    liveness_ping.reset();
                                }
                                forward_websocket_message(
                                &mut upstream,
                                request,
                                "转发 Responses WebSocket 续接请求超时",
                                "转发 Responses WebSocket 续接请求失败",
                                ).await
                            }.await;
                            let sent_close = match retry_result {
                                Ok(closed) => closed,
                                Err(error) => {
                                    if let Some(WebSocketContinuationAction::Flush { messages, metadata }) =
                                        continuation.fail_active_compaction("retry_transport_failed", &error.to_string())?
                                    {
                                        request_logger.record_continue_metadata(&metadata);
                                        for message in messages {
                                            forward_downstream_response(&mut downstream, message,
                                                &request_logger, metadata.log_id.as_deref()).await?;
                                        }
                                        downstream_closed = true;
                                        break 'bridge;
                                    }
                                    return Err(error);
                                }
                            };
                            if sent_close {
                                upstream_closed = true;
                                break;
                            }
                            if metadata.reconnect_upstream {
                                continue 'bridge;
                            }
                        }
                        WebSocketContinuationAction::Flush { messages, metadata } => {
                            request_logger.record_continue_metadata(&metadata);
                            let closes_connection = messages
                                .iter()
                                .any(|message| matches!(message, Message::Close(_)));
                            for message in messages {
                                let message_is_close = matches!(message, Message::Close(_));
                                if forward_downstream_response(
                                    &mut downstream,
                                    message,
                                    &request_logger,
                                    metadata.log_id.as_deref(),
                                )
                                .await?
                                {
                                    downstream_closed = true;
                                    break;
                                }
                                if message_is_close {
                                    downstream_closed = true;
                                    break;
                                }
                            }
                            if closes_connection || downstream_closed {
                                break;
                            }
                        }
                    }
                    if is_close {
                        upstream_closed = true;
                        downstream_closed = true;
                        break;
                    }
                }
                BridgeEvent::LivenessCheck { send_ping } => {
                    let now = Instant::now();
                    if let Some(idle_for) =
                        transport_liveness.take_timeout(UPSTREAM_LIVENESS_PONG_TIMEOUT, now)
                    {
                        let replay_block_reason = request_logger.upstream_replay_block_reason();
                        let will_reconnect = transport_liveness.timeout_limit_exceeded()
                            && replay_block_reason.is_none()
                            && reconnect_attempts < INITIAL_UPSTREAM_RETRY_DELAYS.len();
                        request_logger.log_transport_timeout(
                            idle_for,
                            &transport_liveness,
                            now,
                            will_reconnect,
                        );
                        if transport_liveness.timeout_limit_exceeded() {
                            let mut failure_message = format!(
                                "Responses WebSocket 上游连接连续 {} 次 Ping 后超过 {} 秒没有返回任何帧",
                                transport_liveness.consecutive_timeouts,
                                UPSTREAM_LIVENESS_PONG_TIMEOUT.as_secs()
                            );
                            if will_reconnect {
                                match recover_upstream_connection(
                                    upstream,
                                    &mut downstream,
                                    &mut queued_downstream,
                                    &relay,
                                    &request_context,
                                    &request_logger,
                                    &mut reconnect_attempts,
                                    "ping_timeout",
                                )
                                .await
                                {
                                    Ok(Some(new_upstream)) => {
                                        upstream = new_upstream;
                                        transport_liveness = WebSocketTransportLiveness::default();
                                        liveness_ping.reset();
                                        continue 'bridge;
                                    }
                                    Ok(None) => {
                                        downstream_closed = true;
                                        return Ok(());
                                    }
                                    Err(error) => {
                                        failure_message.push_str(&format!("；上游恢复失败：{error:#}"));
                                        let _ = send_downstream_terminal_failure(
                                            &mut downstream,
                                            &request_logger,
                                            &failure_message,
                                        )
                                        .await;
                                        downstream_closed = true;
                                        anyhow::bail!("{failure_message}");
                                    }
                                }
                            }
                            request_logger.log_upstream_reconnect(
                                "ping_timeout",
                                "skipped",
                                reconnect_attempts,
                                Some(replay_block_reason.unwrap_or("reconnect_attempts_exhausted")),
                            );
                            let _ = send_downstream_terminal_failure(
                                &mut downstream,
                                &request_logger,
                                &failure_message,
                            )
                            .await;
                            downstream_closed = true;
                            anyhow::bail!("{failure_message}");
                        }
                    }
                    if send_ping {
                        if let Some(payload) = transport_liveness.begin_ping(now) {
                            forward_websocket_message(
                                &mut upstream,
                                Message::Ping(payload.into()),
                                "转发 Responses WebSocket Ping 超时",
                                "转发 Responses WebSocket Ping 失败",
                            )
                            .await.map_err(|error| {
                                request_logger.mark_http_fallback("发送 WS Ping 失败");
                                error
                            })?;
                        }
                        continue;
                    }
                    if let Some(expired) = request_logger.expired_pending_request(now) {
                        request_logger.log_idle_timeout(&expired);
                        let failure_message = format!(
                            "Responses WebSocket 上游请求 {} 超过 {} 毫秒没有返回应用事件",
                            expired.log_id,
                            expired.idle_timeout.as_millis()
                        );
                        let _ = send_downstream_terminal_failure(
                            &mut downstream,
                            &request_logger,
                            &failure_message,
                        )
                        .await;
                        downstream_closed = true;
                        anyhow::bail!(
                            "Responses WebSocket 上游请求 {} 超过 {} 毫秒没有返回应用事件",
                            expired.log_id,
                            expired.idle_timeout.as_millis()
                        );
                    }
                }
            }
        }

        if !downstream_closed {
            let _ = close_websocket_sink(
                &mut downstream,
                "关闭本地 Responses WebSocket 超时",
                "关闭本地 Responses WebSocket 失败",
            )
            .await;
        }
        if !upstream_closed {
            let _ = close_websocket_sink(
                &mut upstream,
                "关闭 Responses WebSocket 上游超时",
                "关闭 Responses WebSocket 上游失败",
            )
            .await;
        }
        Ok(())
    }
    .await;

    request_logger.log_bridge_shutdown(
        if result.is_err() {
            "bridge"
        } else if upstream_closed {
            "upstream"
        } else {
            "downstream"
        },
        result.is_err(),
        &[],
    );
    if let Err(error) = result {
        request_logger.finish_pending(&format!("{error:#}"), 502);
        return Err(error);
    }
    request_logger.finish_pending("Responses WebSocket 连接在响应完成前关闭", 499);
    Ok(())
}

async fn open_responses_websocket_upstream_with_initial_retries(
    relay: &crate::settings::RelayProfile,
    request_context: &crate::request_headers::RequestContext,
) -> anyhow::Result<crate::responses_websocket::UpstreamResponsesWebsocket> {
    let mut last_error = None;
    for attempt in 0..=INITIAL_UPSTREAM_RETRY_DELAYS.len() {
        if let Some(delay) = attempt
            .checked_sub(1)
            .and_then(|index| INITIAL_UPSTREAM_RETRY_DELAYS.get(index))
        {
            tokio::time::sleep(*delay).await;
        }
        match crate::responses_websocket::open_responses_websocket_upstream_with_request_context(
            relay,
            request_context,
        )
        .await
        {
            Ok(upstream) => return Ok(upstream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Responses WebSocket 上游连接失败"))).context(
        format!(
            "Responses WebSocket 上游初始连接失败（已尝试 {} 次）",
            INITIAL_UPSTREAM_RETRY_DELAYS.len() + 1
        ),
    )
}

fn websocket_failure_close_frame(code: CloseCode, reason: &'static str) -> CloseFrame {
    CloseFrame {
        code,
        reason: reason.into(),
    }
}

async fn send_downstream_terminal_failure(
    downstream: &mut WebSocketStream<TcpStream>,
    request_logger: &WebSocketRequestLogger,
    error_message: &str,
) -> anyhow::Result<()> {
    request_logger.mark_http_fallback(error_message);
    forward_websocket_message(
        downstream,
        Message::Close(Some(websocket_failure_close_frame(
            CloseCode::Restart,
            "upstream websocket reconnect required",
        ))),
        "关闭本地 Responses WebSocket 超时",
        "关闭本地 Responses WebSocket 失败",
    )
    .await?;
    Ok(())
}

async fn reconnect_upstream_with_replay(
    relay: &crate::settings::RelayProfile,
    request_context: &crate::request_headers::RequestContext,
    request_logger: &WebSocketRequestLogger,
    retry_index: usize,
) -> anyhow::Result<crate::responses_websocket::UpstreamResponsesWebsocket> {
    let delay = INITIAL_UPSTREAM_RETRY_DELAYS
        .get(retry_index)
        .copied()
        .context("Responses WebSocket 内部重连次数无效")?;
    tokio::time::sleep(delay).await;
    let mut upstream =
        crate::responses_websocket::open_responses_websocket_upstream_with_request_context(
            relay,
            request_context,
        )
        .await
        .context("Responses WebSocket 内部重连失败")?;
    for message in request_logger.pending_request_messages() {
        if forward_websocket_message(
            &mut upstream,
            message,
            "重放 Responses WebSocket 请求超时",
            "重放 Responses WebSocket 请求失败",
        )
        .await?
        {
            anyhow::bail!("Responses WebSocket 内部重连在重放请求时提前关闭");
        }
    }
    Ok(upstream)
}

async fn recover_upstream_connection(
    mut old_upstream: crate::responses_websocket::UpstreamResponsesWebsocket,
    downstream: &mut WebSocketStream<TcpStream>,
    queued_downstream: &mut VecDeque<Message>,
    relay: &crate::settings::RelayProfile,
    request_context: &crate::request_headers::RequestContext,
    request_logger: &WebSocketRequestLogger,
    attempts: &mut usize,
    reason: &'static str,
) -> anyhow::Result<Option<crate::responses_websocket::UpstreamResponsesWebsocket>> {
    let mut recovery = Box::pin(async {
        request_logger.log_upstream_reconnect(reason, "closing_upstream", *attempts, None);
        if let Err(error) = close_websocket_sink(
            &mut old_upstream,
            "关闭失活的 Responses WebSocket 上游超时",
            "关闭失活的 Responses WebSocket 上游失败",
        )
        .await
        {
            request_logger.log_upstream_reconnect(
                reason,
                "close_failed",
                *attempts,
                Some(&format!("{error:#}")),
            );
        }
        drop(old_upstream);
        let mut last_error = None;
        while *attempts < INITIAL_UPSTREAM_RETRY_DELAYS.len() {
            let retry_index = *attempts;
            *attempts += 1;
            request_logger.log_upstream_reconnect(reason, "attempt", *attempts, None);
            match reconnect_upstream_with_replay(
                relay,
                request_context,
                request_logger,
                retry_index,
            )
            .await
            {
                Ok(upstream) => {
                    request_logger.log_upstream_reconnect(reason, "succeeded", *attempts, None);
                    return Ok(upstream);
                }
                Err(error) => {
                    request_logger.log_upstream_reconnect(
                        reason,
                        "failed",
                        *attempts,
                        Some(&format!("{error:#}")),
                    );
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Responses WebSocket 重连次数已耗尽")))
    });
    let mut queued_bytes: usize = queued_downstream.iter().map(Message::len).sum();
    loop {
        tokio::select! {
            biased;
            message = downstream.next() => {
                match message {
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        // 丢弃恢复 future 会同时取消拨号/重放，并释放其中持有的上游连接。
                        drop(recovery);
                        request_logger.log_upstream_reconnect(
                            reason, "cancelled", *attempts, Some("downstream_closed"),
                        );
                        return Ok(None);
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    Some(Ok(message)) => {
                        queued_bytes += message.len();
                        if queued_bytes > crate::responses_websocket::RESPONSES_TRANSPORT_MAX_BYTES
                            || queued_downstream.len() >= 256
                        {
                            anyhow::bail!("Responses WebSocket 恢复期间下游排队超过 64 MiB 或 256 条消息");
                        }
                        queued_downstream.push_back(message);
                    }
                }
            }
            result = &mut recovery => return result.map(Some),
        }
    }
}

#[derive(Clone, Default)]
struct WebSocketContinuationCoordinator {
    state: Arc<Mutex<WebSocketContinuationState>>,
}

#[derive(Default)]
struct WebSocketContinuationState {
    active: Option<ActiveWebSocketContinuation>,
    discarded_response_ids: VecDeque<String>,
}

struct ActiveWebSocketContinuation {
    mode: ActiveWebSocketMode,
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
    compaction_plan: Option<WebSocketCompactionPlan>,
    capacity_retry_attempts: u8,
}

#[derive(Clone)]
struct WebSocketCompactionFallback {
    request: Value,
    route: crate::layered_compaction::CompactionRoute,
    model: String,
}

#[derive(Clone)]
struct WebSocketCompactionPlan {
    route: crate::layered_compaction::CompactionRoute,
    model: String,
    original_model: String,
    independent_compaction_model: Option<String>,
    independent_compaction_usage: Option<crate::settings::LayeredCompactionModelUsage>,
    retry_request: Option<Value>,
    original_fallback: Option<WebSocketCompactionFallback>,
    independent: bool,
}

#[derive(Clone, Copy)]
enum ActiveWebSocketMode {
    ValidatedCompaction {
        kind: crate::layered_compaction::CompactionKind,
        layered_enabled: bool,
        retain_tokens: u32,
    },
    ContinueThinking,
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
    layered_compaction_triggered: bool,
    layered_compaction_retain_tokens: Option<u32>,
    layered_compaction_retained_items: Option<u32>,
    layered_compaction_retained_chars: Option<u32>,
    layered_compaction_before_response_body: Option<String>,
    retry_delay: Option<Duration>,
    reconnect_upstream: bool,
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
    fn has_validated_compaction(&self) -> bool {
        self.state.lock().ok().is_some_and(|state| {
            state.active.as_ref().is_some_and(|active| {
                matches!(active.mode, ActiveWebSocketMode::ValidatedCompaction { .. })
            })
        })
    }
    #[cfg(test)]
    fn register_request_with_settings(
        &self,
        payload: &Value,
        log_id: Option<String>,
        layered_compaction_options: Option<crate::protocol_proxy::LayeredCompactionOptions>,
        settings: &crate::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.register_request_with_settings_and_plan(
            payload,
            log_id,
            layered_compaction_options,
            settings,
            None,
        )
    }

    fn register_request_with_settings_and_plan(
        &self,
        payload: &Value,
        log_id: Option<String>,
        layered_compaction_options: Option<crate::protocol_proxy::LayeredCompactionOptions>,
        settings: &crate::settings::BackendSettings,
        prepared_compaction_plan: Option<WebSocketCompactionPlan>,
    ) -> anyhow::Result<()> {
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
        if !settings.layered_compaction_enabled
            && crate::layered_compaction::is_any_compaction_request(payload)
        {
            return Ok(());
        }
        if let Some(kind) = crate::layered_compaction::CompactionKind::of_request(payload)
            && let Some(options) = layered_compaction_options
        {
            let mut compaction_options =
                crate::layered_compaction::CompactionOptions::from_settings(settings);
            compaction_options.retain_recent_round = options.enabled;
            compaction_options.retain_tokens = options.retain_tokens;
            let compaction_plan = prepared_compaction_plan.unwrap_or_else(|| {
                let route = crate::layered_compaction::CompactionRoute::for_model(model);
                let first = crate::layered_compaction::prepare_compaction_attempt_request(
                    payload,
                    kind,
                    route,
                    &compaction_options,
                    crate::layered_compaction::CompactionAttempt::First,
                );
                WebSocketCompactionPlan {
                    route,
                    model: model.to_string(),
                    original_model: model.to_string(),
                    independent_compaction_model: None,
                    independent_compaction_usage: None,
                    retry_request: Some(crate::layered_compaction::compaction_retry_request(
                        &first,
                    )),
                    original_fallback: None,
                    independent: false,
                }
            });
            state.active = Some(ActiveWebSocketContinuation {
                mode: ActiveWebSocketMode::ValidatedCompaction {
                    kind,
                    layered_enabled: options.enabled,
                    retain_tokens: options.retain_tokens,
                },
                original_request: payload.clone(),
                log_id,
                max_rounds: 0,
                round: 0,
                completed_rounds: 0,
                accumulated_reasoning_tokens: None,
                buffered_messages: Vec::new(),
                fallback_messages: Vec::new(),
                fallback_response_body: None,
                continue_requests: Vec::new(),
                before_response_body: None,
                compaction_plan: Some(compaction_plan),
                capacity_retry_attempts: 0,
            });
            return Ok(());
        }
        // 原生压缩请求不需要本地状态，也不能误进自动推理续接。
        if crate::layered_compaction::is_any_compaction_request(payload) {
            return Ok(());
        }
        if !settings.gpt_reasoning_continuation
            || !crate::continue_thinking::is_supported_model(model)
        {
            return Ok(());
        }

        state.active = Some(ActiveWebSocketContinuation {
            mode: ActiveWebSocketMode::ContinueThinking,
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
            compaction_plan: None,
            capacity_retry_attempts: 0,
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
        if let Message::Text(text) = &message {
            if let Ok(payload) = serde_json::from_str::<Value>(text.as_str()) {
                if websocket_response_id(&payload).is_some_and(|response_id| {
                    state
                        .discarded_response_ids
                        .iter()
                        .any(|discarded| discarded == &response_id)
                }) {
                    return Ok(WebSocketContinuationAction::Buffered);
                }
            }
        }
        let Some(active) = state.active.as_mut() else {
            return Ok(WebSocketContinuationAction::Forward(message));
        };

        if matches!(message, Message::Ping(_) | Message::Pong(_)) {
            return Ok(WebSocketContinuationAction::Forward(message));
        }
        let mode = active.mode;
        if let ActiveWebSocketMode::ValidatedCompaction {
            kind,
            layered_enabled,
            retain_tokens,
        } = mode
        {
            return handle_validated_compaction_websocket_message(
                &mut state,
                message,
                kind,
                layered_enabled,
                retain_tokens,
            );
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
        if is_retryable_websocket_model_capacity_error(payload.as_ref())
            && active
                .original_request
                .get("model")
                .and_then(Value::as_str)
                .is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("gpt"))
            && usize::from(active.capacity_retry_attempts) < MODEL_CAPACITY_RETRY_DELAYS.len()
        {
            let retry_index = usize::from(active.capacity_retry_attempts);
            active.capacity_retry_attempts += 1;
            active.buffered_messages.clear();
            let request_text = serde_json::to_string(&active.original_request)
                .context("序列化 Responses WebSocket 容量重试请求失败")?;
            let model = active
                .original_request
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.model_capacity_retry",
                serde_json::json!({
                    "transport": "ws",
                    "model": model,
                    "attempt": retry_index + 1,
                    "maxRetries": MODEL_CAPACITY_RETRY_DELAYS.len(),
                    "delayMs": MODEL_CAPACITY_RETRY_DELAYS[retry_index].as_millis()
                }),
            );
            return Ok(WebSocketContinuationAction::Continue {
                request: Message::Text(request_text.into()),
                metadata: WebSocketContinueMetadata {
                    retry_delay: Some(MODEL_CAPACITY_RETRY_DELAYS[retry_index]),
                    ..Default::default()
                },
            });
        }
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

    fn fail_active_compaction(
        &self,
        failure_suffix: &str,
        detail: &str,
    ) -> anyhow::Result<Option<WebSocketContinuationAction>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Responses WebSocket 续接状态锁已损坏"))?;
        let mode = state.active.as_ref().map(|active| active.mode);
        let (code_prefix, label) = match mode {
            Some(ActiveWebSocketMode::ValidatedCompaction { kind, .. }) => {
                let active = state.active.as_ref().expect("active compaction");
                let original_model = active.original_request["model"]
                    .as_str()
                    .unwrap_or_default();
                let route = active
                    .compaction_plan
                    .as_ref()
                    .map(|plan| plan.route)
                    .unwrap_or_else(|| {
                        crate::layered_compaction::CompactionRoute::for_model(original_model)
                    });
                let model = active
                    .compaction_plan
                    .as_ref()
                    .map(|plan| plan.model.as_str())
                    .unwrap_or(original_model);
                crate::layered_compaction::log_compaction_attempt(
                    "ws",
                    kind,
                    route,
                    if active.round == 0 {
                        crate::layered_compaction::CompactionAttempt::First
                    } else {
                        crate::layered_compaction::CompactionAttempt::Retry
                    },
                    model,
                    None,
                    &Err(
                        crate::layered_compaction::CompactionValidationFailure::NoTerminalResponse,
                    ),
                    Some(detail),
                );
                ("compaction", "Context compaction")
            }
            _ => return Ok(None),
        };
        let active = state.active.take().expect("active compaction must exist");
        let code = format!("{code_prefix}_{failure_suffix}");
        let message = format!("{label} {detail}");
        let failure_sse = crate::layered_compaction::compaction_failure_sse(
            &active.original_request,
            None,
            &code,
            &message,
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        messages.push(Message::Close(None));
        Ok(Some(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        }))
    }
}

fn handle_validated_compaction_websocket_message(
    state: &mut WebSocketContinuationState,
    message: Message,
    kind: crate::layered_compaction::CompactionKind,
    layered_enabled: bool,
    retain_tokens: u32,
) -> anyhow::Result<WebSocketContinuationAction> {
    use crate::layered_compaction::*;
    let payload = match &message {
        Message::Text(text) => serde_json::from_str::<Value>(text.as_str()).ok(),
        _ => None,
    };
    let terminal = payload
        .as_ref()
        .and_then(|p| p["type"].as_str())
        .is_some_and(is_terminal_websocket_response_event);
    let malformed = payload.is_none();
    let closed = matches!(message, Message::Close(_));
    let active = state.active.as_mut().expect("active compaction");
    if active.compaction_plan.is_none() {
        let model = active.original_request["model"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let route = CompactionRoute::for_model(&model);
        active.compaction_plan = Some(WebSocketCompactionPlan {
            route,
            model: model.clone(),
            original_model: model,
            independent_compaction_model: None,
            independent_compaction_usage: None,
            retry_request: Some(compaction_retry_request(&active.original_request)),
            original_fallback: None,
            independent: false,
        });
    }
    active.buffered_messages.push(message);
    if !terminal && !malformed {
        return Ok(WebSocketContinuationAction::Buffered);
    }
    let source_sse = websocket_messages_to_responses_sse(&active.buffered_messages);
    let (response, verdict) = if malformed {
        (None, Err(CompactionValidationFailure::NoTerminalResponse))
    } else {
        validate_compaction_sse(&source_sse)
    };
    let plan = active
        .compaction_plan
        .as_ref()
        .expect("validated compaction must include a plan");
    let event_type = payload
        .as_ref()
        .and_then(|payload| payload.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let transport_failure =
        malformed || closed || matches!(event_type, "response.failed" | "error");
    log_compaction_attempt(
        "ws",
        kind,
        plan.route,
        if active.round == 0 {
            CompactionAttempt::First
        } else {
            CompactionAttempt::Retry
        },
        &plan.model,
        response.as_ref(),
        &verdict,
        transport_failure
            .then_some(event_type)
            .filter(|value| !value.is_empty()),
    );
    if verdict.is_err() && active.round == 0 {
        let plan = active
            .compaction_plan
            .as_mut()
            .expect("validated compaction must include a plan");
        let retry = if crate::protocol_proxy::compaction_second_attempt(
            plan.independent,
            transport_failure,
        ) == crate::protocol_proxy::CompactionSecondAttempt::OriginalModelFallback
        {
            let fallback = plan
                .original_fallback
                .take()
                .expect("independent compaction must retain an original-model fallback");
            plan.route = fallback.route;
            plan.model = fallback.model;
            plan.independent = false;
            plan.retry_request = None;
            fallback.request
        } else {
            plan.retry_request
                .take()
                .expect("compaction retry snapshot")
        };
        let request = Message::Text(serde_json::to_string(&retry)?.into());
        active.round = 1;
        active.buffered_messages.clear();
        let metadata = WebSocketContinueMetadata {
            log_id: active.log_id.clone(),
            request_body: Some(serde_json::to_string(&retry)?),
            reconnect_upstream: malformed,
            ..Default::default()
        };
        if let Some(id) = payload.as_ref().and_then(websocket_response_id) {
            remember_discarded_websocket_response_id(state, id);
        }
        return Ok(WebSocketContinuationAction::Continue { request, metadata });
    }
    let active = state.active.take().expect("active compaction");
    let (result, stats) = match verdict {
        Ok(_) => {
            let mut response = response.expect("validated response");
            if let Some(plan) = active.compaction_plan.as_ref()
                && plan.model != plan.original_model
            {
                restore_response_model(&mut response, &plan.original_model);
            }
            let result = build_compaction_success_response(
                &active.original_request,
                kind,
                &response,
                &CompactionOptions {
                    retain_recent_round: layered_enabled,
                    retain_tokens,
                    ..Default::default()
                },
            );
            (result.response, result.layered)
        }
        Err(failure) => (
            compaction_validation_failure_response(
                &active.original_request,
                kind,
                response.as_ref(),
                failure,
            ),
            LayeredCompactionStats::default(),
        ),
    };
    if result["status"] != "completed"
        && let Some(id) = payload.as_ref().and_then(websocket_response_id)
    {
        remember_discarded_websocket_response_id(state, id);
    }
    let mut messages = responses_sse_to_websocket_messages(&compaction_payload_sse(kind, &result));
    if closed {
        messages.push(Message::Close(None));
    }
    Ok(WebSocketContinuationAction::Flush {
        messages,
        metadata: WebSocketContinueMetadata {
            log_id: active.log_id,
            layered_compaction_triggered: stats.triggered,
            layered_compaction_retain_tokens: stats.triggered.then_some(retain_tokens),
            layered_compaction_retained_items: stats.triggered.then_some(stats.retained_items),
            layered_compaction_retained_chars: stats.triggered.then_some(stats.retained_chars),
            layered_compaction_before_response_body: Some(source_sse),
            ..Default::default()
        },
    })
}

fn remember_discarded_websocket_response_id(
    state: &mut WebSocketContinuationState,
    response_id: String,
) {
    const MAX_DISCARDED_RESPONSE_IDS: usize = 64;
    if state
        .discarded_response_ids
        .iter()
        .any(|discarded| discarded == &response_id)
    {
        return;
    }
    if state.discarded_response_ids.len() >= MAX_DISCARDED_RESPONSE_IDS {
        state.discarded_response_ids.pop_front();
    }
    state.discarded_response_ids.push_back(response_id);
}

fn websocket_messages_to_responses_sse(messages: &[Message]) -> String {
    let mut sse = String::new();
    for message in messages {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };
        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type.is_empty() {
            continue;
        }
        sse.push_str("event: ");
        sse.push_str(event_type);
        sse.push_str("\ndata: ");
        sse.push_str(text.as_str());
        sse.push_str("\n\n");
    }
    sse
}

fn responses_sse_to_websocket_messages(sse_text: &str) -> Vec<Message> {
    sse_text
        .split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                return None;
            }
            serde_json::from_str::<Value>(&data).ok()?;
            Some(Message::Text(data.into()))
        })
        .collect()
}

fn add_websocket_reasoning_tokens(total: &mut Option<u64>, reasoning_tokens: Option<u64>) {
    if let Some(reasoning_tokens) = reasoning_tokens {
        *total = Some(total.unwrap_or(0).saturating_add(reasoning_tokens));
    }
}

fn is_retryable_websocket_model_capacity_error(payload: Option<&Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    let error = payload
        .get("error")
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("error"))
        })
        .unwrap_or(payload);
    let code = error
        .get("code")
        .or_else(|| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        code.as_str(),
        "server_is_overloaded" | "model_overloaded" | "model_at_capacity"
    ) {
        return true;
    }
    error
        .get("message")
        .or_else(|| error.get("detail"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("selected model is at capacity")
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
        ..Default::default()
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
    connection_context: WebSocketConnectionContext,
    connection_generation: u64,
    connected_at: Instant,
    application_request_count: usize,
    interrupted_before_response: bool,
    requests: HashMap<String, TrackedWebSocketRequest>,
    active_order: VecDeque<String>,
    unassigned_order: VecDeque<String>,
    response_ids: HashMap<String, String>,
    transport_context: Option<(
        crate::settings::RelayProfile,
        crate::request_headers::RequestContext,
        Option<u64>,
    )>,
}

struct TrackedWebSocketRequest {
    record: crate::proxy_log::ProxyRequestRecord,
    started_at: Instant,
    last_upstream_event_at: Option<Instant>,
    idle_timeout: Duration,
    response_capture: Vec<u8>,
    response_bytes: usize,
    response_truncated: bool,
    transport_timeout_count: usize,
    last_transport_timeout_at: Option<Instant>,
    response_id: Option<String>,
    response_model: crate::proxy_log::UpstreamResponseModelObserver,
    cache_observer: Option<crate::compaction_cache::CacheResponseObserver>,
}

impl TrackedWebSocketRequest {
    fn log_after_transport_timeout(&self, stage: &str, event_type: Option<&str>) {
        let Some(timeout_at) = self.last_transport_timeout_at else {
            return;
        };
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_request_after_ping_timeout",
            serde_json::json!({
                "logId": self.record.id,
                "model": self.record.model,
                "stage": stage,
                "eventType": event_type,
                "pingTimeoutCount": self.transport_timeout_count,
                "sinceLastPingTimeoutMs": timeout_at.elapsed().as_millis(),
                "requestAgeMs": self.started_at.elapsed().as_millis(),
                "statusCode": self.record.status_code,
                "error": self.record.error,
            }),
        );
    }
}

#[derive(Clone, Debug)]
struct ExpiredWebSocketRequest {
    log_id: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    idle_timeout: Duration,
    idle_for: Duration,
}

impl WebSocketRequestLogger {
    fn new(
        relay: &crate::settings::RelayProfile,
        remote_addr: Option<String>,
        path: String,
        connection_context: WebSocketConnectionContext,
    ) -> Self {
        let connected_at = Instant::now();
        Self {
            state: Arc::new(Mutex::new(WebSocketRequestLogState {
                remote_addr,
                path,
                relay_id: relay.id.clone(),
                relay_name: relay.name.clone(),
                endpoint: crate::responses_websocket::responses_websocket_url(
                    crate::responses_websocket::relay_responses_base_url(relay),
                ),
                connection_context,
                connection_generation: NEXT_WEBSOCKET_CONNECTION_GENERATION
                    .fetch_add(1, Ordering::Relaxed),
                connected_at,
                application_request_count: 0,
                interrupted_before_response: false,
                requests: HashMap::new(),
                active_order: VecDeque::new(),
                unassigned_order: VecDeque::new(),
                response_ids: HashMap::new(),
                transport_context: None,
            })),
        }
    }

    fn set_transport_context(
        &self,
        relay: crate::settings::RelayProfile,
        context: crate::request_headers::RequestContext,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.transport_context = Some((relay, context, None));
        }
    }

    fn mark_http_fallback(&self, reason: &str) {
        if let Ok(state) = self.state.lock() {
            if let Some((relay, context, Some(generation))) = state.transport_context.as_ref() {
                crate::session_transport::mark_http_for_generation(
                    relay,
                    context,
                    reason,
                    *generation,
                );
            }
        }
    }

    fn record_request(&self, payload: &Value, request_body: &str) -> Option<String> {
        let metadata = crate::proxy_log::extract_request_metadata(Some(payload));
        let idle_timeout = crate::protocol_proxy::stream_idle_timeout_for_request(Some(payload));
        let id = format!("local-{}", uuid::Uuid::new_v4());
        let timestamp_ms = crate::proxy_log::current_timestamp_ms();
        let started_at = Instant::now();
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        if let Some((relay, context, generation)) = state.transport_context.as_mut() {
            *generation = crate::session_transport::generation(relay, context);
        }
        state.application_request_count += 1;
        let record = crate::proxy_log::ProxyRequestRecord {
            id: id.clone(),
            state: crate::proxy_log::ProxyRequestState::Pending,
            transport: crate::proxy_log::ProxyRequestTransport::Ws,
            timestamp_ms,
            method: "WS".to_string(),
            path: state.path.clone(),
            remote_addr: state.remote_addr.clone(),
            model: metadata.model.clone(),
            upstream_request_model: metadata.model,
            upstream_response_model: None,
            independent_compaction_model: None,
            independent_compaction_usage: None,
            reasoning_tokens: None,
            reasoning_effort: metadata.reasoning_effort,
            reasoning_source: metadata.reasoning_source,
            continue_thinking_triggered: false,
            continue_thinking_rounds: 0,
            continue_thinking_request_body: None,
            continue_thinking_before_response_body: None,
            continue_thinking_after_response_body: None,
            remote_compaction_triggered: crate::proxy_log::request_uses_remote_compaction_v2(Some(
                payload,
            )),
            layered_compaction_triggered: false,
            compaction_requested: metadata.compaction_requested,
            layered_compaction_retain_tokens: None,
            layered_compaction_retained_items: None,
            layered_compaction_retained_chars: None,
            layered_compaction_before_response_body: None,
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
        let cache_observer = state
            .transport_context
            .as_ref()
            .and_then(|(relay, context, _)| {
                let endpoint = state.endpoint.as_deref()?;
                let wire_request =
                    serde_json::from_str::<Value>(request_body).unwrap_or_else(|_| payload.clone());
                crate::compaction_cache::CacheResponseObserver::new(
                    relay,
                    crate::compaction_cache::CacheProtocol::Responses,
                    endpoint,
                    payload,
                    &wire_request,
                    context,
                    true,
                    started_at,
                )
            });
        let application_started_at = state.active_order.is_empty().then_some(started_at);
        state.requests.insert(
            id.clone(),
            TrackedWebSocketRequest {
                record: record.clone(),
                started_at,
                last_upstream_event_at: application_started_at,
                idle_timeout,
                response_capture: Vec::new(),
                response_bytes: 0,
                response_truncated: false,
                transport_timeout_count: 0,
                last_transport_timeout_at: None,
                response_id: None,
                response_model: Default::default(),
                cache_observer,
            },
        );
        state.active_order.push_back(id.clone());
        state.unassigned_order.push_back(id.clone());
        drop(state);
        append_websocket_proxy_log_record(&record);
        if let Ok(state) = self.state.lock() {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.responses_websocket_request",
                serde_json::json!({
                    "logId": id,
                    "relayId": state.relay_id,
                    "relayName": state.relay_name,
                    "endpoint": state.endpoint,
                    "model": record.model,
                    "sessionId": state.connection_context.session_id,
                    "threadId": state.connection_context.thread_id,
                    "turnId": state.connection_context.turn_id,
                    "requestKind": state.connection_context.request_kind,
                    "windowId": state.connection_context.window_id,
                    "connectionGeneration": state.connection_generation,
                    "socketAgeMs": state.connected_at.elapsed().as_millis(),
                    "streamIdleTimeoutMs": idle_timeout.as_millis(),
                }),
            );
        }
        Some(id)
    }

    fn record_independent_compaction(
        &self,
        log_id: Option<&str>,
        model: Option<&str>,
        usage: Option<crate::settings::LayeredCompactionModelUsage>,
    ) {
        let (Some(log_id), Some(model), Some(usage)) = (
            log_id,
            model.map(str::trim).filter(|model| !model.is_empty()),
            usage,
        ) else {
            return;
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(tracked) = state.requests.get_mut(log_id) else {
            return;
        };
        tracked.record.independent_compaction_model = Some(model.to_string());
        tracked.record.independent_compaction_usage = Some(usage);
        tracked.record.upstream_request_model = Some(model.to_string());
        let record = tracked.record.clone();
        drop(state);
        append_websocket_proxy_log_record(&record);
    }

    fn has_recorded_application_requests(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.application_request_count > 0)
    }

    fn interrupted_before_response(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.interrupted_before_response)
    }

    fn has_pending_requests(&self) -> bool {
        self.state
            .lock()
            .map_or(true, |state| !state.requests.is_empty())
    }

    fn upstream_replay_block_reason(&self) -> Option<&'static str> {
        let Ok(state) = self.state.lock() else {
            return Some("request_state_unavailable");
        };
        for tracked in state.requests.values() {
            // 包括已交付给 Codex 的事件，以及压缩/续接正在缓冲的事件。
            // 这些请求都不能从初始 response.create 无条件重放。
            if tracked.record.first_token_ms.is_some() {
                return Some("application_response_already_received");
            }
            let Ok(payload) = serde_json::from_str::<Value>(&tracked.record.request_body) else {
                return Some("request_body_unavailable");
            };
            if payload
                .get("previous_response_id")
                .is_some_and(|id| !id.is_null())
            {
                return Some("previous_response_id_requires_context_recovery");
            }
        }
        None
    }

    fn pending_request_messages(&self) -> Vec<Message> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut request_ids = Vec::new();
        for log_id in &state.active_order {
            if !request_ids.iter().any(|existing| existing == log_id) {
                request_ids.push(log_id.clone());
            }
        }
        for log_id in &state.unassigned_order {
            if !request_ids.iter().any(|existing| existing == log_id) {
                request_ids.push(log_id.clone());
            }
        }
        request_ids
            .into_iter()
            .filter_map(|log_id| state.requests.get(&log_id))
            .filter(|tracked| tracked.record.state == crate::proxy_log::ProxyRequestState::Pending)
            .filter_map(|tracked| {
                (!tracked.record.request_body.trim().is_empty())
                    .then(|| Message::Text(tracked.record.request_body.clone().into()))
            })
            .collect()
    }

    #[cfg(test)]
    fn pending_response_failure_message(&self, error_message: &str) -> Option<Message> {
        let state = self.state.lock().ok()?;
        let log_id = state
            .active_order
            .iter()
            .find(|log_id| state.requests.contains_key(*log_id))?;
        let tracked = state.requests.get(log_id)?;
        let response_id = tracked.response_id.clone().unwrap_or_else(|| {
            format!(
                "resp_codex_elves_failed_{}",
                log_id.trim_start_matches("local-")
            )
        });
        let mut response = serde_json::json!({
            "id": response_id,
            "object": "response",
            "status": "failed",
            "error": {
                "type": "responses_websocket_upstream_failed",
                "code": "responses_websocket_upstream_failed",
                "message": error_message
            }
        });
        if let Some(model) = tracked.record.model.as_deref() {
            response["model"] = Value::String(model.to_string());
        }
        Some(Message::Text(
            serde_json::json!({
                "type": "response.failed",
                "response": response
            })
            .to_string()
            .into(),
        ))
    }

    fn record_upstream_application_activity(&self, message: &Message) {
        let Some(payload) = websocket_application_event_payload(message) else {
            return;
        };
        let response_id = websocket_response_id(&payload);
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        // Responses WebSocket 在同一连接内顺序处理 response.create；后续请求由上游排队，
        // 因此所有尚未终止的上游事件都属于当前队首请求。
        let Some(log_id) = state
            .active_order
            .front()
            .filter(|log_id| state.requests.contains_key(*log_id))
            .cloned()
        else {
            return;
        };
        if let Some(response_id) = response_id {
            state.response_ids.insert(response_id, log_id.clone());
        }
        state.unassigned_order.retain(|id| id != &log_id);
        if let Some(tracked) = state.requests.get_mut(&log_id) {
            if tracked.last_transport_timeout_at.is_some_and(|timeout_at| {
                tracked
                    .last_upstream_event_at
                    .is_none_or(|at| at <= timeout_at)
            }) {
                tracked.log_after_transport_timeout(
                    "application_resumed",
                    payload.get("type").and_then(Value::as_str),
                );
            }
            tracked.last_upstream_event_at = Some(Instant::now());
        }
    }

    fn expired_pending_request(&self, now: Instant) -> Option<ExpiredWebSocketRequest> {
        let state = self.state.lock().ok()?;
        let log_id = state.active_order.front()?;
        let tracked = state.requests.get(log_id)?;
        let last_upstream_event_at = tracked.last_upstream_event_at?;
        websocket_idle_timeout_elapsed(last_upstream_event_at, tracked.idle_timeout, now).map(
            |idle_for| ExpiredWebSocketRequest {
                log_id: log_id.clone(),
                model: tracked.record.model.clone(),
                reasoning_effort: tracked.record.reasoning_effort.clone(),
                idle_timeout: tracked.idle_timeout,
                idle_for,
            },
        )
    }

    fn log_idle_timeout(&self, expired: &ExpiredWebSocketRequest) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_idle_timeout",
            serde_json::json!({
                "logId": expired.log_id,
                "relayId": state.relay_id,
                "relayName": state.relay_name,
                "endpoint": state.endpoint,
                "model": expired.model,
                "reasoningEffort": expired.reasoning_effort,
                "sessionId": state.connection_context.session_id,
                "threadId": state.connection_context.thread_id,
                "turnId": state.connection_context.turn_id,
                "requestKind": state.connection_context.request_kind,
                "windowId": state.connection_context.window_id,
                "connectionGeneration": state.connection_generation,
                "socketAgeMs": state.connected_at.elapsed().as_millis(),
                "streamIdleTimeoutMs": expired.idle_timeout.as_millis(),
                "idleForMs": expired.idle_for.as_millis(),
            }),
        );
    }

    fn log_transport_timeout(
        &self,
        idle_for: Duration,
        liveness: &WebSocketTransportLiveness,
        now: Instant,
        will_reconnect: bool,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let pending_requests: Vec<Value> = state
            .requests
            .iter_mut()
            .map(|(log_id, tracked)| {
                tracked.transport_timeout_count += 1;
                tracked.last_transport_timeout_at = Some(now);
                serde_json::json!({
                    "logId": log_id,
                    "model": tracked.record.model,
                    "requestAgeMs": now.duration_since(tracked.started_at).as_millis(),
                })
            })
            .collect();
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_transport_timeout",
            serde_json::json!({
                "relayId": state.relay_id,
                "relayName": state.relay_name,
                "endpoint": state.endpoint,
                "sessionId": state.connection_context.session_id,
                "threadId": state.connection_context.thread_id,
                "turnId": state.connection_context.turn_id,
                "requestKind": state.connection_context.request_kind,
                "windowId": state.connection_context.window_id,
                "connectionGeneration": state.connection_generation,
                "socketAgeMs": state.connected_at.elapsed().as_millis(),
                "transportIdleTimeoutMs": UPSTREAM_LIVENESS_PONG_TIMEOUT.as_millis(),
                "idleForMs": idle_for.as_millis(),
                "consecutivePingTimeouts": liveness.consecutive_timeouts,
                "toleratedPingTimeouts": UPSTREAM_LIVENESS_TOLERATED_TIMEOUTS,
                "willTerminate": liveness.timeout_limit_exceeded() && !will_reconnect,
                "willReconnect": will_reconnect,
                "pendingRequests": pending_requests,
                "lastPingAgoMs": liveness.last_ping_at.map(|at| now.duration_since(at).as_millis()),
                "lastPongAgoMs": liveness.last_pong_at.map(|at| now.duration_since(at).as_millis()),
                "lastPongRttMs": liveness.last_pong_rtt.map(|rtt| rtt.as_millis()),
                "lastTransportActivityAgoMs": liveness
                    .last_transport_activity_at
                    .map(|at| now.duration_since(at).as_millis()),
            }),
        );
    }

    fn log_upstream_reconnect(
        &self,
        reason: &str,
        stage: &str,
        attempt: usize,
        error: Option<&str>,
    ) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let pending_requests: Vec<Value> = state
            .active_order
            .iter()
            .filter_map(|id| state.requests.get(id))
            .map(|tracked| {
                serde_json::json!({
                    "logId": tracked.record.id,
                    "model": tracked.record.model,
                })
            })
            .collect();
        let _ = crate::diagnostic_log::append_diagnostic_log(
            if reason == "ping_timeout" {
                "protocol_proxy.responses_websocket_ping_reconnect"
            } else {
                "protocol_proxy.responses_websocket_reconnect"
            },
            serde_json::json!({
                "relayId": state.relay_id,
                "threadId": state.connection_context.thread_id,
                "turnId": state.connection_context.turn_id,
                "connectionGeneration": state.connection_generation,
                "stage": stage,
                "reason": reason,
                "attempt": attempt,
                "maxAttempts": INITIAL_UPSTREAM_RETRY_DELAYS.len(),
                "pendingRequests": pending_requests,
                "error": error,
            }),
        );
    }

    fn log_transport_recovered(&self, consecutive_timeouts: usize, message: &Message) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let frame_type = match message {
            Message::Text(_) => "text",
            Message::Binary(_) => "binary",
            Message::Ping(_) => "ping",
            Message::Pong(_) => "pong",
            Message::Close(_) => "close",
            Message::Frame(_) => "frame",
        };
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_transport_activity_after_ping_timeout",
            serde_json::json!({
                "relayId": state.relay_id,
                "threadId": state.connection_context.thread_id,
                "turnId": state.connection_context.turn_id,
                "connectionGeneration": state.connection_generation,
                "previousConsecutivePingTimeouts": consecutive_timeouts,
                "frameType": frame_type,
            }),
        );
    }

    fn log_bridge_shutdown(&self, exit_task: &str, task_failed: bool, shutdown_notes: &[String]) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_bridge_shutdown",
            serde_json::json!({
                "relayId": state.relay_id,
                "relayName": state.relay_name,
                "endpoint": state.endpoint,
                "sessionId": state.connection_context.session_id,
                "threadId": state.connection_context.thread_id,
                "turnId": state.connection_context.turn_id,
                "requestKind": state.connection_context.request_kind,
                "windowId": state.connection_context.window_id,
                "connectionGeneration": state.connection_generation,
                "socketAgeMs": state.connected_at.elapsed().as_millis(),
                "exitTask": exit_task,
                "taskFailed": task_failed,
                "shutdownNotes": shutdown_notes,
            }),
        );
    }

    #[cfg(test)]
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
        if let Some(response_id) = response_id {
            tracked.response_id = Some(response_id);
        }
        tracked.response_model.observe_value(
            &payload,
            crate::protocol_proxy::UpstreamResponseProtocol::Responses,
            Some(event_type),
        );
        tracked.record.upstream_response_model = tracked.response_model.model();
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
        let mut completed_cache_observer = None;
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
        if let Some(response_id) = response_id {
            tracked.response_id = Some(response_id);
        }
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
        if let Some(observer) = tracked.cache_observer.as_mut() {
            observer.observe_json_event(&payload);
        }

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
            tracked.log_after_transport_timeout("terminal_response", Some(event_type));
            update = Some(tracked.record.clone());
            let observer = tracked.cache_observer.take();
            if !failed {
                completed_cache_observer = observer;
            }
        } else if first_response_event {
            update = Some(tracked.record.clone());
        }

        if terminal {
            state.requests.remove(&log_id);
            state.active_order.retain(|id| id != &log_id);
            state.unassigned_order.retain(|id| id != &log_id);
            state.response_ids.retain(|_, id| id != &log_id);
            activate_next_queued_websocket_request(&mut state, Instant::now());
        }
        drop(state);
        if let Some(observer) = completed_cache_observer {
            observer.finish();
        }
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
        if let Some(upstream_model) = metadata
            .request_body
            .as_deref()
            .and_then(|body| serde_json::from_str::<Value>(body).ok())
            .and_then(|request| {
                request
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(ToString::to_string)
            })
        {
            tracked.record.upstream_request_model = Some(upstream_model);
        }
        tracked.record.layered_compaction_triggered = metadata.layered_compaction_triggered;
        tracked.record.layered_compaction_retain_tokens = metadata.layered_compaction_retain_tokens;
        tracked.record.layered_compaction_retained_items =
            metadata.layered_compaction_retained_items;
        tracked.record.layered_compaction_retained_chars =
            metadata.layered_compaction_retained_chars;
        tracked.record.layered_compaction_before_response_body =
            metadata.layered_compaction_before_response_body.clone();
        let record = tracked.record.clone();
        drop(state);
        append_websocket_proxy_log_record(&record);
    }

    fn finish_pending(&self, error: &str, status_code: u16) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.interrupted_before_response = status_code == 502
            && !state.requests.is_empty()
            && state
                .requests
                .values()
                .all(|tracked| tracked.record.first_token_ms.is_none());
        let mut records = Vec::with_capacity(state.requests.len());
        let interrupted_request_count = state.requests.len();
        let connection_context = state.connection_context.clone();
        let relay_id = state.relay_id.clone();
        let relay_name = state.relay_name.clone();
        let endpoint = state.endpoint.clone();
        let connection_generation = state.connection_generation;
        let socket_age_ms = state.connected_at.elapsed().as_millis();
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
            tracked.log_after_transport_timeout("interrupted", None);
            records.push(tracked.record);
        }
        state.active_order.clear();
        state.unassigned_order.clear();
        state.response_ids.clear();
        drop(state);
        for record in records {
            append_websocket_proxy_log_record(&record);
        }
        if interrupted_request_count > 0 {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.responses_websocket_interrupted",
                serde_json::json!({
                    "relayId": relay_id,
                    "relayName": relay_name,
                    "endpoint": endpoint,
                    "sessionId": connection_context.session_id,
                    "threadId": connection_context.thread_id,
                    "turnId": connection_context.turn_id,
                    "requestKind": connection_context.request_kind,
                    "windowId": connection_context.window_id,
                    "connectionGeneration": connection_generation,
                    "socketAgeMs": socket_age_ms,
                    "requestCount": interrupted_request_count,
                    "statusCode": status_code,
                    "error": error,
                }),
            );
        }
    }

    fn finish_oversized_request(
        &self,
        log_id: Option<&str>,
        request_bytes: usize,
        safe_max_bytes: usize,
    ) {
        self.mark_http_fallback("请求超过 WS 的 16 MiB 安全上限");
        let Some(log_id) = log_id else {
            return;
        };
        let (mut record, relay_id, relay_name, endpoint, connection_context, fallback_armed) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let Some(mut tracked) = state.requests.remove(log_id) else {
                return;
            };
            tracked.record.state = crate::proxy_log::ProxyRequestState::Completed;
            tracked.record.status_code = Some(413);
            tracked.record.duration_ms = Some(tracked.started_at.elapsed().as_millis() as u64);
            tracked.record.response_bytes = Some(0);
            tracked.record.response_captured_bytes = Some(0);
            tracked.record.error = Some(format!(
                "Responses WebSocket 请求大小 {request_bytes} 字节超过上游安全上限 {safe_max_bytes} 字节，当前轮次改走 HTTP"
            ));
            state.active_order.retain(|id| id != log_id);
            state.unassigned_order.retain(|id| id != log_id);
            state.response_ids.retain(|_, id| id != log_id);
            activate_next_queued_websocket_request(&mut state, Instant::now());
            let relay_id = state.relay_id.clone();
            let relay_name = state.relay_name.clone();
            let endpoint = state.endpoint.clone();
            let connection_context = state.connection_context.clone();
            let record = tracked.record;
            drop(state);
            let fallback_armed =
                arm_oversized_responses_websocket_http_fallback(&relay_id, &connection_context);
            (
                record,
                relay_id,
                relay_name,
                endpoint,
                connection_context,
                fallback_armed,
            )
        };
        record.response_body.clear();
        append_websocket_proxy_log_record(&record);
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_request_too_large",
            serde_json::json!({
                "logId": log_id,
                "relayId": relay_id,
                "relayName": relay_name,
                "endpoint": endpoint,
                "sessionId": connection_context.session_id,
                "threadId": connection_context.thread_id,
                "turnId": connection_context.turn_id,
                "requestKind": connection_context.request_kind,
                "windowId": connection_context.window_id,
                "requestBytes": request_bytes,
                "safeMaxBytes": safe_max_bytes,
                "httpFallbackArmed": fallback_armed,
            }),
        );
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
        // 上游一次只处理一个 response。续接轮次可能产生新的 response_id，
        // 但仍必须绑定当前队首，不能误分配给后续排队请求。
        if let Some(log_id) = state
            .active_order
            .front()
            .filter(|log_id| state.requests.contains_key(*log_id))
            .cloned()
        {
            state
                .response_ids
                .insert(response_id.to_string(), log_id.clone());
            state.unassigned_order.retain(|id| id != &log_id);
            return Some(log_id);
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

fn activate_next_queued_websocket_request(
    state: &mut WebSocketRequestLogState,
    activated_at: Instant,
) {
    let Some(log_id) = state.active_order.front().cloned() else {
        return;
    };
    if let Some(tracked) = state.requests.get_mut(&log_id) {
        tracked.last_upstream_event_at.get_or_insert(activated_at);
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

fn websocket_application_event_payload(message: &Message) -> Option<Value> {
    let Message::Text(text) = message else {
        return None;
    };
    let payload = serde_json::from_str::<Value>(text.as_str()).ok()?;
    payload
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| !event_type.is_empty())
        .then_some(payload)
}

fn websocket_idle_timeout_elapsed(
    last_upstream_event_at: Instant,
    idle_timeout: Duration,
    now: Instant,
) -> Option<Duration> {
    let idle_for = now.saturating_duration_since(last_upstream_event_at);
    (idle_for >= idle_timeout).then_some(idle_for)
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
    if let Err(error) = crate::proxy_log::enqueue_record_nonblocking(record) {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "helper.local_proxy_log_failed",
            serde_json::json!({
                "id": record.id,
                "transport": "ws",
                "dropped_intermediate": crate::proxy_log::dropped_intermediate_record_count(),
                "error": error.to_string()
            }),
        );
    }
}

async fn forward_downstream_response<S>(
    downstream: &mut S,
    message: Message,
    request_logger: &WebSocketRequestLogger,
    preferred_log_id: Option<&str>,
) -> anyhow::Result<bool>
where
    S: Sink<Message, Error = WebSocketError> + Unpin,
{
    let closed = forward_websocket_message(
        downstream,
        message.clone(),
        "转发 Responses WebSocket 响应超时",
        "转发 Responses WebSocket 响应失败",
    )
    .await?;
    request_logger.record_response_for(&message, preferred_log_id);
    Ok(closed)
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
) -> anyhow::Result<Option<(Value, crate::settings::BackendSettings, bool)>> {
    let Message::Text(text) = message else {
        return Ok(None);
    };
    let settings = current_websocket_settings(relay)?;
    let payload: Value =
        serde_json::from_str(text.as_str()).context("Responses WebSocket 请求不是有效 JSON")?;
    if payload.get("type").and_then(Value::as_str) != Some("response.create") {
        anyhow::bail!("Responses WebSocket 仅支持 response.create 请求");
    }
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if model.is_empty() {
        anyhow::bail!("Responses WebSocket 请求缺少 model");
    }
    let current_relay = settings.active_relay_profile();
    if current_relay.resolve_protocol_for_model(&model)? != RelayProtocol::Responses {
        anyhow::bail!("当前模型不是原生 Responses 协议");
    }
    let rewritten_payload =
        crate::protocol_proxy::apply_system_prompt_override_to_responses_request(
            &payload,
            &current_relay,
        );
    let rewritten_payload = crate::protocol_proxy::rewrite_catalog_model_to_request_model(
        &rewritten_payload,
        &current_relay,
    );
    let payload_rewritten = rewritten_payload != payload;
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.responses_websocket_request",
        serde_json::json!({
            "relayId": relay.id,
            "relayName": relay.name,
            "endpoint": crate::responses_websocket::responses_websocket_url(
                crate::responses_websocket::relay_responses_base_url(relay)
            ),
            "model": rewritten_payload
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        }),
    );
    Ok(Some((rewritten_payload, settings, payload_rewritten)))
}

#[cfg(test)]
fn prepare_downstream_response_create_payload(
    payload: &Value,
) -> anyhow::Result<(
    Value,
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
)> {
    let normalized = normalize_downstream_response_create_payload(payload);
    let model = normalized
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let remote_compaction_v2_bridge =
        crate::layered_compaction::is_remote_compaction_v2_request(Some(&normalized))
            && !crate::layered_compaction::model_supports_native_remote_compaction_v2(model);
    let legacy_compaction = crate::layered_compaction::is_compaction_request(Some(&normalized));
    if !remote_compaction_v2_bridge && !legacy_compaction {
        return Ok((normalized.clone(), normalized, None));
    }
    let settings = SettingsStore::default()
        .load()
        .context("读取 Responses WebSocket 上下文压缩设置失败")?;
    let (request_payload, forwarded_payload, options) =
        prepare_downstream_response_create_payload_with_settings(normalized, &settings);
    Ok((request_payload, forwarded_payload, options))
}

fn local_compaction_wait_websocket_messages(payload: &Value) -> Option<Vec<Message>> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    crate::layered_compaction::local_compaction_requires_real_user(payload, model).then(|| {
        responses_sse_to_websocket_messages(
            &crate::layered_compaction::local_compaction_wait_for_user_sse(payload),
        )
    })
}

async fn prepare_downstream_response_create_payload_with_snapshot(
    payload: &Value,
    settings: &crate::settings::BackendSettings,
    relay: &crate::settings::RelayProfile,
    request_context: &crate::request_headers::RequestContext,
) -> (
    Value,
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
    Option<WebSocketCompactionPlan>,
) {
    let normalized = normalize_downstream_response_create_payload(payload);
    let normalized = if crate::layered_compaction::is_any_compaction_request(&normalized) {
        normalized
    } else {
        restore_oversized_resumed_compaction_checkpoint(normalized, settings).await
    };
    let (forwarded, options, plan) =
        prepare_websocket_compaction_execution(&normalized, settings, relay, request_context);
    // 等待用户的判定依赖原始压缩标识；规范化后的 wire 历史已不再包含该标识。
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let snapshot = if crate::layered_compaction::local_compaction_requires_real_user(payload, model)
    {
        payload.clone()
    } else {
        normalized
    };
    (snapshot, forwarded, options, plan)
}

pub(crate) fn normalize_downstream_response_create_payload(payload: &Value) -> Value {
    if crate::layered_compaction::is_any_compaction_request(payload) {
        return payload.clone();
    }
    let mut normalized = crate::protocol_proxy::normalize_native_responses_request(payload);
    if normalized
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
    {
        return normalized;
    }
    let model = normalized
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let original_bytes = serde_json::to_vec(&normalized)
        .ok()
        .map(|value| value.len());
    let (original_input_items, retained_input_items) = {
        let Some(input) = normalized.get_mut("input").and_then(Value::as_array_mut) else {
            return normalized;
        };
        let Some(latest_compaction_index) = input
            .iter()
            .rposition(|item| item.get("type").and_then(Value::as_str) == Some("compaction"))
        else {
            return normalized;
        };
        if latest_compaction_index == 0 {
            return normalized;
        }

        let original_input_items = input.len();
        input.drain(..latest_compaction_index);
        (original_input_items, input.len())
    };
    let retained_bytes = serde_json::to_vec(&normalized)
        .ok()
        .map(|value| value.len());
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.responses_websocket_compacted_history_pruned",
        serde_json::json!({
            "source": "wire",
            "model": model,
            "droppedInputItems": original_input_items.saturating_sub(retained_input_items),
            "retainedInputItems": retained_input_items,
            "originalBytes": original_bytes,
            "retainedBytes": retained_bytes,
        }),
    );
    normalized
}

async fn restore_oversized_resumed_compaction_checkpoint(
    normalized: Value,
    settings: &crate::settings::BackendSettings,
) -> Value {
    if normalized
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
        || normalized
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|input| {
                input
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("compaction"))
            })
    {
        return normalized;
    }
    let original_bytes = serde_json::to_vec(&normalized)
        .ok()
        .map(|value| value.len());
    if original_bytes.is_some_and(|bytes| {
        bytes <= crate::responses_websocket::RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES
    }) {
        return normalized;
    }

    let model = normalized
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let codex_home = crate::codex_home::codex_home_dir_for_settings(settings);
    let fallback = normalized.clone();
    let restore = tokio::task::spawn_blocking(move || {
        crate::responses_websocket_checkpoint::restore_missing_compaction_checkpoint(
            normalized,
            &codex_home,
        )
    })
    .await;
    let Ok(restore) = restore else {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_checkpoint_restore_skipped",
            serde_json::json!({
                "model": model,
                "reason": "checkpoint_restore_task_failed",
                "originalBytes": original_bytes,
            }),
        );
        return fallback;
    };
    let Some(restored) = restore.restored else {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.responses_websocket_checkpoint_restore_skipped",
            serde_json::json!({
                "model": model,
                "reason": restore.skip_reason.unwrap_or("checkpoint_restore_failed"),
                "originalBytes": original_bytes,
            }),
        );
        return restore.payload;
    };

    let retained_bytes = serde_json::to_vec(&restore.payload)
        .ok()
        .map(|value| value.len());
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.responses_websocket_compacted_history_pruned",
        serde_json::json!({
            "source": "rollout_checkpoint",
            "model": model,
            "windowNumber": restored.window_number,
            "checkpointPrefixItems": restored.checkpoint_prefix_items,
            "droppedInputItems": restored
                .original_input_items
                .saturating_sub(restored.retained_input_items),
            "retainedInputItems": restored.retained_input_items,
            "originalBytes": original_bytes,
            "retainedBytes": retained_bytes,
        }),
    );
    restore.payload
}

#[cfg(test)]
fn prepare_downstream_response_create_payload_with_settings(
    normalized: Value,
    settings: &crate::settings::BackendSettings,
) -> (
    Value,
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
) {
    let (forwarded, options) =
        prepare_websocket_compaction_forwarded_payload(&normalized, settings);
    (normalized, forwarded, options)
}

fn prepare_websocket_compaction_execution(
    normalized: &Value,
    settings: &crate::settings::BackendSettings,
    relay: &crate::settings::RelayProfile,
    request_context: &crate::request_headers::RequestContext,
) -> (
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
    Option<WebSocketCompactionPlan>,
) {
    use crate::layered_compaction::*;

    if !settings.layered_compaction_enabled {
        return (normalized.clone(), None, None);
    }
    let mode = crate::protocol_proxy::compaction_execution_mode(
        normalized,
        crate::protocol_proxy::UpstreamResponseProtocol::Responses,
    );
    if matches!(
        mode,
        crate::protocol_proxy::CompactionExecutionMode::None
            | crate::protocol_proxy::CompactionExecutionMode::NativeRemoteV2
    ) {
        return (normalized.clone(), None, None);
    }
    let kind = CompactionKind::of_request(normalized).expect("compaction request");
    let options = CompactionOptions::from_settings(settings);
    let original_model = normalized
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let endpoint = crate::responses_websocket::responses_websocket_url(
        crate::responses_websocket::relay_responses_base_url(relay),
    )
    .unwrap_or_default();
    let cache_probe = prepare_compaction_attempt_request(
        normalized,
        kind,
        CompactionRoute::CacheReuse,
        &options,
        CompactionAttempt::First,
    );
    let cache_probe = crate::protocol_proxy::normalize_native_responses_request(&cache_probe);
    let cache_decision = crate::compaction_cache::lease_decision(
        relay,
        crate::compaction_cache::CacheProtocol::Responses,
        &endpoint,
        normalized,
        &cache_probe,
        request_context,
    );
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.compaction_cache_lease_decision",
        serde_json::json!({
            "transport": "ws",
            "relayId": relay.id,
            "relayName": relay.name,
            "endpoint": endpoint,
            "protocol": crate::protocol_proxy::UpstreamResponseProtocol::Responses,
            "originalModel": original_model,
            "state": cache_decision.state.as_str(),
            "ttl": cache_decision.ttl,
            "remainingMs": cache_decision.remaining_ms,
            "identitySha256": cache_decision.identity_sha256,
        }),
    );

    let original_route = if cache_decision.is_valid() {
        CompactionRoute::CacheReuse
    } else {
        CompactionRoute::for_model(&original_model)
    };
    let original_first = prepare_compaction_attempt_request(
        normalized,
        kind,
        original_route,
        &options,
        CompactionAttempt::First,
    );
    let original_first = crate::protocol_proxy::normalize_native_responses_request(&original_first);
    let mut forwarded = original_first.clone();
    let mut route = original_route;
    let mut model = original_model.clone();
    let mut independent = false;
    let mut independent_compaction_model = None;
    let mut independent_compaction_usage = None;
    let mut original_fallback = None;

    if settings.should_use_independent_compaction_model(cache_decision.is_valid()) {
        if let Some(resolved) = crate::protocol_proxy::resolve_compaction_model_override_for_attempt(
            normalized, normalized, kind, &options, mode, settings, relay,
        ) {
            if resolved.protocol == RelayProtocol::Responses {
                let independent_route = CompactionRoute::for_model(&resolved.compaction_model);
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "protocol_proxy.compaction_model_override_applied",
                    serde_json::json!({
                        "transport": "ws",
                        "relayId": relay.id,
                        "relayName": relay.name,
                        "originalModel": resolved.original_model,
                        "compactionModel": resolved.compaction_model,
                        "family": crate::model_capabilities::model_family(&resolved.original_model).as_str(),
                        "contextWindow": resolved.context_window,
                        "estimatedInputTokens": resolved.estimated_input_tokens,
                        "outputReserveTokens": resolved.output_reserve_tokens,
                        "cacheLeaseState": cache_decision.state.as_str(),
                        "modelUsage": settings.layered_compaction_model_usage,
                    }),
                );
                forwarded = crate::protocol_proxy::normalize_native_responses_request(
                    &resolved.request_json,
                );
                route = independent_route;
                independent_compaction_model = Some(resolved.compaction_model.clone());
                independent_compaction_usage = Some(settings.layered_compaction_model_usage);
                model = resolved.compaction_model;
                independent = true;
                original_fallback = Some(WebSocketCompactionFallback {
                    request: original_first,
                    route: original_route,
                    model: original_model.clone(),
                });
            } else {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "protocol_proxy.compaction_model_override_skipped",
                    serde_json::json!({
                        "transport": "ws",
                        "relayId": relay.id,
                        "relayName": relay.name,
                        "originalModel": resolved.original_model,
                        "compactionModel": resolved.compaction_model,
                        "modelUsage": settings.layered_compaction_model_usage,
                        "reason": "独立压缩模型不是 Responses 协议，现有 Responses WebSocket 连接无法安全承载，回落原模型",
                    }),
                );
            }
        }
    }

    let retry_request = compaction_retry_request(&forwarded);
    (
        forwarded,
        Some(crate::protocol_proxy::LayeredCompactionOptions {
            enabled: options.retain_recent_round,
            retain_tokens: options.retain_tokens,
        }),
        Some(WebSocketCompactionPlan {
            route,
            model,
            original_model,
            independent_compaction_model,
            independent_compaction_usage,
            retry_request: Some(retry_request),
            original_fallback,
            independent,
        }),
    )
}

#[cfg(test)]
fn prepare_websocket_compaction_forwarded_payload(
    normalized: &Value,
    settings: &crate::settings::BackendSettings,
) -> (
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
) {
    use crate::layered_compaction::*;
    if !settings.layered_compaction_enabled {
        return (normalized.clone(), None);
    }
    let mode = crate::protocol_proxy::compaction_execution_mode(
        normalized,
        crate::protocol_proxy::UpstreamResponseProtocol::Responses,
    );
    if matches!(
        mode,
        crate::protocol_proxy::CompactionExecutionMode::None
            | crate::protocol_proxy::CompactionExecutionMode::NativeRemoteV2
    ) {
        return (normalized.clone(), None);
    }
    let options = CompactionOptions::from_settings(settings);
    let forwarded = prepare_compaction_attempt_request(
        normalized,
        CompactionKind::of_request(normalized).expect("compaction request"),
        CompactionRoute::for_model(normalized["model"].as_str().unwrap_or_default()),
        &options,
        CompactionAttempt::First,
    );
    (
        forwarded,
        Some(crate::protocol_proxy::LayeredCompactionOptions {
            enabled: options.retain_recent_round,
            retain_tokens: options.retain_tokens,
        }),
    )
}

fn ensure_websocket_relay_still_current(
    connected_relay: &crate::settings::RelayProfile,
) -> anyhow::Result<()> {
    current_websocket_settings(connected_relay).map(|_| ())
}

fn current_websocket_settings(
    connected_relay: &crate::settings::RelayProfile,
) -> anyhow::Result<crate::settings::BackendSettings> {
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
    Ok(settings)
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
    connection_context: &WebSocketConnectionContext,
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
            "sessionId": connection_context.session_id,
            "threadId": connection_context.thread_id,
            "turnId": connection_context.turn_id,
            "requestKind": connection_context.request_kind,
            "windowId": connection_context.window_id,
            "error": error,
        }),
    );
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ResponsesWebSocketOversizedFallbackKey {
    relay_id: String,
    thread_id: String,
    turn_id: String,
}

fn responses_websocket_oversized_fallback_key(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> Option<ResponsesWebSocketOversizedFallbackKey> {
    if connection_context.request_kind.as_deref() != Some("turn") {
        return None;
    }
    Some(ResponsesWebSocketOversizedFallbackKey {
        relay_id: relay_id.to_string(),
        thread_id: connection_context.thread_id.clone()?,
        turn_id: connection_context.turn_id.clone()?,
    })
}

fn responses_websocket_oversized_fallbacks()
-> &'static Mutex<HashMap<ResponsesWebSocketOversizedFallbackKey, Instant>> {
    static FALLBACKS: OnceLock<Mutex<HashMap<ResponsesWebSocketOversizedFallbackKey, Instant>>> =
        OnceLock::new();
    FALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ResponsesWebSocketEarlyDisconnectFallbackKey {
    relay_id: String,
    thread_id: String,
    turn_id: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ResponsesWebSocketUpstreamFailureFallbackKey {
    relay_id: String,
    thread_id: String,
    turn_id: String,
}

fn responses_websocket_upstream_failure_fallback_key(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> Option<ResponsesWebSocketUpstreamFailureFallbackKey> {
    if connection_context.request_kind.as_deref() != Some("turn") {
        return None;
    }
    Some(ResponsesWebSocketUpstreamFailureFallbackKey {
        relay_id: relay_id.to_string(),
        thread_id: connection_context.thread_id.clone()?,
        turn_id: connection_context.turn_id.clone()?,
    })
}

fn responses_websocket_upstream_failure_fallbacks()
-> &'static Mutex<HashMap<ResponsesWebSocketUpstreamFailureFallbackKey, Instant>> {
    static FALLBACKS: OnceLock<
        Mutex<HashMap<ResponsesWebSocketUpstreamFailureFallbackKey, Instant>>,
    > = OnceLock::new();
    FALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn arm_upstream_failure_responses_websocket_http_fallback(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_upstream_failure_fallback_key(relay_id, connection_context)
    else {
        return false;
    };
    let Ok(mut fallbacks) = responses_websocket_upstream_failure_fallbacks().lock() else {
        return false;
    };
    let now = Instant::now();
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.insert(key, now + UPSTREAM_FAILURE_HTTP_FALLBACK_TTL);
    true
}

fn should_temporarily_fallback_upstream_failure_responses_websocket_to_http(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_upstream_failure_fallback_key(relay_id, connection_context)
    else {
        return false;
    };
    let now = Instant::now();
    let Ok(mut fallbacks) = responses_websocket_upstream_failure_fallbacks().lock() else {
        return false;
    };
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.contains_key(&key)
}

fn responses_websocket_early_disconnect_fallback_key(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> Option<ResponsesWebSocketEarlyDisconnectFallbackKey> {
    if connection_context.request_kind.as_deref() != Some("turn") {
        return None;
    }
    Some(ResponsesWebSocketEarlyDisconnectFallbackKey {
        relay_id: relay_id.to_string(),
        thread_id: connection_context.thread_id.clone()?,
        turn_id: connection_context.turn_id.clone()?,
    })
}

fn responses_websocket_early_disconnect_fallbacks()
-> &'static Mutex<HashMap<ResponsesWebSocketEarlyDisconnectFallbackKey, Instant>> {
    static FALLBACKS: OnceLock<
        Mutex<HashMap<ResponsesWebSocketEarlyDisconnectFallbackKey, Instant>>,
    > = OnceLock::new();
    FALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn should_temporarily_fallback_early_disconnect_responses_websocket_to_http(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_early_disconnect_fallback_key(relay_id, connection_context)
    else {
        return false;
    };
    let now = Instant::now();
    let Ok(mut fallbacks) = responses_websocket_early_disconnect_fallbacks().lock() else {
        return false;
    };
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.contains_key(&key)
}

fn arm_early_disconnect_responses_websocket_http_fallback(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_early_disconnect_fallback_key(relay_id, connection_context)
    else {
        return false;
    };
    let Ok(mut fallbacks) = responses_websocket_early_disconnect_fallbacks().lock() else {
        return false;
    };
    let now = Instant::now();
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.insert(key, now + EARLY_DISCONNECT_HTTP_FALLBACK_TTL);
    true
}

fn should_temporarily_fallback_oversized_responses_websocket_to_http(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_oversized_fallback_key(relay_id, connection_context) else {
        return false;
    };
    let now = Instant::now();
    let Ok(mut fallbacks) = responses_websocket_oversized_fallbacks().lock() else {
        return false;
    };
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.contains_key(&key)
}

fn arm_oversized_responses_websocket_http_fallback(
    relay_id: &str,
    connection_context: &WebSocketConnectionContext,
) -> bool {
    let Some(key) = responses_websocket_oversized_fallback_key(relay_id, connection_context) else {
        return false;
    };
    let Ok(mut fallbacks) = responses_websocket_oversized_fallbacks().lock() else {
        return false;
    };
    let now = Instant::now();
    fallbacks.retain(|_, expires_at| *expires_at > now);
    fallbacks.insert(key, now + OVERSIZED_REQUEST_HTTP_FALLBACK_TTL);
    true
}

#[derive(Clone, Debug, Default)]
struct WebSocketConnectionContext {
    session_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    request_kind: Option<String>,
    window_id: Option<String>,
}

impl WebSocketConnectionContext {
    fn from_request(request: &Request) -> Self {
        let turn_metadata = request
            .headers()
            .get("x-codex-turn-metadata")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| serde_json::from_str::<Value>(value).ok());
        Self {
            session_id: redact_websocket_identifier(websocket_header(request, "session-id")),
            thread_id: redact_websocket_identifier(
                websocket_header(request, "thread-id")
                    .or_else(|| websocket_header(request, "x-client-request-id")),
            ),
            turn_id: redact_websocket_identifier(
                turn_metadata
                    .as_ref()
                    .and_then(|value| value.get("turn_id"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
            request_kind: bounded_websocket_context_label(
                turn_metadata
                    .as_ref()
                    .and_then(|value| value.get("request_kind"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            ),
            window_id: redact_websocket_identifier(
                websocket_header(request, "x-codex-window-id").or_else(|| {
                    turn_metadata
                        .as_ref()
                        .and_then(|value| value.get("window_id"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                }),
            ),
        }
    }
}

fn redact_websocket_identifier(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    if value.is_empty() {
        return None;
    }
    let digest = Sha256::digest(value.as_bytes());
    let short_hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("sha256:{short_hash}"))
}

pub(crate) fn redact_transport_thread_id(value: &str) -> Option<String> {
    redact_websocket_identifier(Some(value.to_string()))
}

fn bounded_websocket_context_label(value: Option<String>) -> Option<String> {
    let value = value?;
    let bounded = value
        .chars()
        .filter(|character| !character.is_control())
        .take(64)
        .collect::<String>();
    (!bounded.trim().is_empty()).then_some(bounded)
}

fn websocket_header(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveWebSocketContinuation, ActiveWebSocketMode, WebSocketConnectionContext,
        WebSocketContinuationAction, WebSocketContinuationCoordinator, WebSocketContinuationState,
        WebSocketContinueMetadata, WebSocketRequestLogger, WebSocketTransportLiveness,
        activate_next_queued_websocket_request,
        arm_early_disconnect_responses_websocket_http_fallback,
        arm_oversized_responses_websocket_http_fallback,
        arm_upstream_failure_responses_websocket_http_fallback, is_responses_websocket_proxy_path,
        is_responses_websocket_upgrade, local_compaction_wait_websocket_messages,
        prepare_downstream_response_create_payload,
        prepare_downstream_response_create_payload_with_settings,
        prepare_websocket_compaction_execution, queue_upstream_liveness_ping,
        resolve_websocket_log_id,
        should_temporarily_fallback_early_disconnect_responses_websocket_to_http,
        should_temporarily_fallback_oversized_responses_websocket_to_http,
        should_temporarily_fallback_upstream_failure_responses_websocket_to_http,
        websocket_application_event_payload, websocket_failure_close_frame,
        websocket_idle_timeout_elapsed,
    };
    use crate::settings::{
        BackendSettings, LayeredCompactionModelUsage, LayeredCompactionModels, RelayModelMapping,
        RelayProfile, RelayProtocol,
    };
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio_tungstenite::tungstenite::Message;

    fn compaction_relay(id: &str, models: &[(&str, RelayProtocol, &str)]) -> RelayProfile {
        RelayProfile {
            id: id.to_string(),
            name: id.to_string(),
            upstream_base_url: format!("https://{id}.example.test"),
            api_key: format!("key-{id}"),
            local_proxy_enabled: Some(true),
            model_mappings: models
                .iter()
                .map(|(model, protocol, context_window)| RelayModelMapping {
                    request_model: (*model).to_string(),
                    alias: String::new(),
                    protocol: *protocol,
                    context_window: (*context_window).to_string(),
                    system_prompt_override: String::new(),
                })
                .collect(),
            ..RelayProfile::default()
        }
    }

    fn independent_compaction_settings(model: &str) -> BackendSettings {
        independent_compaction_settings_with_usage(model, LayeredCompactionModelUsage::CacheMiss)
    }

    fn independent_compaction_settings_with_usage(
        model: &str,
        usage: LayeredCompactionModelUsage,
    ) -> BackendSettings {
        BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_model_override_enabled: true,
            layered_compaction_model_usage: usage,
            layered_compaction_models: LayeredCompactionModels {
                gpt: model.to_string(),
                ..Default::default()
            },
            ..BackendSettings::default()
        }
    }

    fn legacy_compaction_payload(model: &str, cache_key: &str) -> Value {
        json!({
            "type": "response.create",
            "model": model,
            "prompt_cache_key": cache_key,
            "instructions": "stable system prompt",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "history" }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Summarize."
                    }]
                }
            ]
        })
    }

    #[test]
    fn websocket_unknown_cache_lease_uses_the_independent_responses_model() {
        let relay = compaction_relay(
            "ws-unknown-lease",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("gpt-cheap");
        let payload = legacy_compaction_payload("gpt-original", "ws-unknown-lease-key");
        let (forwarded, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let plan = plan.expect("local compaction should include a plan");

        assert!(options.is_some());
        assert_eq!(forwarded["model"], "gpt-cheap");
        assert_eq!(plan.model, "gpt-cheap");
        assert_eq!(
            plan.independent_compaction_model.as_deref(),
            Some("gpt-cheap")
        );
        assert_eq!(
            plan.independent_compaction_usage,
            Some(LayeredCompactionModelUsage::CacheMiss)
        );
        assert_eq!(
            plan.route,
            crate::layered_compaction::CompactionRoute::CodexLocal
        );
        assert!(plan.independent);
        assert_eq!(
            plan.original_fallback
                .as_ref()
                .expect("independent plan needs original fallback")
                .model,
            "gpt-original"
        );
    }

    #[test]
    fn websocket_unknown_cache_lease_preserves_tools_for_non_gpt_model() {
        let relay = compaction_relay(
            "ws-non-gpt-original",
            &[("deepseek-chat", RelayProtocol::Responses, "128000")],
        );
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            ..Default::default()
        };
        let mut payload = legacy_compaction_payload("deepseek-chat", "ws-non-gpt-original-key");
        payload["tools"] =
            json!([{ "type": "function", "name": "exec_command", "parameters": {} }]);
        payload["tool_choice"] = json!("auto");
        payload["parallel_tool_calls"] = json!(true);
        let (forwarded, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let plan = plan.expect("local compaction should include a plan");

        assert!(options.is_some());
        assert_eq!(
            plan.route,
            crate::layered_compaction::CompactionRoute::CacheReuse
        );
        assert!(!plan.independent);
        for field in ["tools", "tool_choice", "parallel_tool_calls"] {
            assert_eq!(forwarded[field], payload[field], "{field}");
        }
    }

    #[test]
    fn websocket_repeated_compaction_normalizes_first_retry_and_fallback_history() {
        let relay = compaction_relay(
            "ws-repeated-compaction",
            &[
                ("deepseek-chat", RelayProtocol::Responses, "128000"),
                ("glm-test", RelayProtocol::Responses, "128000"),
            ],
        );
        let mut settings = BackendSettings {
            layered_compaction_enabled: true,
            ..Default::default()
        };
        let mut payload = legacy_compaction_payload("deepseek-chat", "ws-repeated-compaction-key");
        let instruction = payload["input"].as_array_mut().unwrap().pop().unwrap();
        payload["input"][0]["content"] = json!([
            {"type": "input_file", "file_url": "https://example.invalid/OLD.pdf"}
        ]);
        payload["input"].as_array_mut().unwrap().extend([
            json!({"type": "compaction", "encrypted_content": "codex-elves-compaction-v2:HANDOFF"}),
            json!({"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "current task"}
            ]}),
        ]);
        let ordinary = super::normalize_downstream_response_create_payload(&payload);
        payload["input"].as_array_mut().unwrap().push(instruction);
        for independent in [false, true] {
            settings.layered_compaction_model_override_enabled = independent;
            settings.layered_compaction_models.other = "glm-test".to_string();
            let (forwarded, _, plan) = prepare_websocket_compaction_execution(
                &payload,
                &settings,
                &relay,
                &crate::request_headers::RequestContext::default(),
            );
            let plan = plan.unwrap();
            assert_eq!(plan.independent, independent);
            let input = ordinary["input"].as_array().unwrap();
            assert_eq!(
                &forwarded["input"].as_array().unwrap()[..input.len()],
                input
            );
            for request in [
                Some(&forwarded),
                plan.retry_request.as_ref(),
                plan.original_fallback
                    .as_ref()
                    .map(|fallback| &fallback.request),
            ]
            .into_iter()
            .flatten()
            {
                let serialized = request.to_string();
                assert!(serialized.contains("HANDOFF"));
                assert!(!serialized.contains("OLD.pdf"));
                assert!(!serialized.contains("codex-elves-compaction-v2:"));
            }
        }
    }

    #[test]
    fn websocket_non_gpt_independent_model_preserves_tools_and_gpt_fallback_removes_them() {
        let relay = compaction_relay(
            "ws-non-gpt-independent",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("deepseek-chat", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("deepseek-chat");
        let mut payload = legacy_compaction_payload("gpt-original", "ws-non-gpt-independent-key");
        payload["tools"] =
            json!([{ "type": "function", "name": "exec_command", "parameters": {} }]);
        payload["tool_choice"] = json!("auto");
        payload["parallel_tool_calls"] = json!(true);
        let (forwarded, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let plan = plan.expect("local compaction should include a plan");

        assert!(options.is_some());
        assert_eq!(forwarded["model"], "deepseek-chat");
        assert_eq!(
            plan.route,
            crate::layered_compaction::CompactionRoute::CacheReuse
        );
        assert!(plan.independent);
        for field in ["tools", "tool_choice", "parallel_tool_calls"] {
            assert_eq!(forwarded[field], payload[field], "{field}");
        }
        let fallback = plan
            .original_fallback
            .as_ref()
            .expect("independent plan needs original fallback");
        assert_eq!(
            fallback.route,
            crate::layered_compaction::CompactionRoute::CodexLocal
        );
        for field in ["tools", "tool_choice", "parallel_tool_calls"] {
            assert!(fallback.request.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn websocket_valid_cache_lease_keeps_the_original_model_and_cache_route() {
        let relay = compaction_relay(
            "ws-valid-lease",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("gpt-cheap");
        let context = crate::request_headers::RequestContext::default();
        let endpoint = crate::responses_websocket::responses_websocket_url(
            crate::responses_websocket::relay_responses_base_url(&relay),
        )
        .unwrap();
        let ordinary = json!({
            "type": "response.create",
            "model": "gpt-original",
            "prompt_cache_key": "ws-valid-lease-key",
            "instructions": "stable system prompt",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "history" }]
            }]
        });
        let mut observer = crate::compaction_cache::CacheResponseObserver::new(
            &relay,
            crate::compaction_cache::CacheProtocol::Responses,
            &endpoint,
            &ordinary,
            &ordinary,
            &context,
            true,
            Instant::now(),
        )
        .unwrap();
        observer.observe_json_event(&json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "usage": {
                    "cache_creation_input_tokens": 100,
                    "cache_ttl": "5m"
                }
            }
        }));
        observer.finish();

        let payload = legacy_compaction_payload("gpt-original", "ws-valid-lease-key");
        let (forwarded, _, plan) =
            prepare_websocket_compaction_execution(&payload, &settings, &relay, &context);
        let plan = plan.unwrap();
        assert_eq!(forwarded["model"], "gpt-original");
        assert_eq!(
            plan.route,
            crate::layered_compaction::CompactionRoute::CacheReuse
        );
        assert!(!plan.independent);
        assert!(plan.independent_compaction_model.is_none());
        assert!(plan.independent_compaction_usage.is_none());
        assert!(plan.original_fallback.is_none());
    }

    #[test]
    fn websocket_default_usage_uses_the_independent_model_with_a_valid_cache_lease() {
        let relay = compaction_relay(
            "ws-default-valid-lease",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings_with_usage(
            "gpt-cheap",
            LayeredCompactionModelUsage::Default,
        );
        let context = crate::request_headers::RequestContext::default();
        let endpoint = crate::responses_websocket::responses_websocket_url(
            crate::responses_websocket::relay_responses_base_url(&relay),
        )
        .unwrap();
        let ordinary = json!({
            "type": "response.create",
            "model": "gpt-original",
            "prompt_cache_key": "ws-default-valid-lease-key",
            "instructions": "stable system prompt",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "history" }]
            }]
        });
        let mut observer = crate::compaction_cache::CacheResponseObserver::new(
            &relay,
            crate::compaction_cache::CacheProtocol::Responses,
            &endpoint,
            &ordinary,
            &ordinary,
            &context,
            true,
            Instant::now(),
        )
        .unwrap();
        observer.observe_json_event(&json!({
            "type": "response.completed",
            "response": {
                "status": "completed",
                "usage": {
                    "cache_creation_input_tokens": 100,
                    "cache_ttl": "5m"
                }
            }
        }));
        observer.finish();

        let payload = legacy_compaction_payload("gpt-original", "ws-default-valid-lease-key");
        let (forwarded, _, plan) =
            prepare_websocket_compaction_execution(&payload, &settings, &relay, &context);
        let plan = plan.unwrap();

        assert_eq!(forwarded["model"], "gpt-cheap");
        assert_eq!(plan.model, "gpt-cheap");
        assert_eq!(
            plan.independent_compaction_model.as_deref(),
            Some("gpt-cheap")
        );
        assert_eq!(
            plan.independent_compaction_usage,
            Some(LayeredCompactionModelUsage::Default)
        );
        assert!(plan.independent);
        assert_eq!(
            plan.original_fallback
                .as_ref()
                .expect("independent plan needs original fallback")
                .route,
            crate::layered_compaction::CompactionRoute::CacheReuse
        );
    }

    #[test]
    fn websocket_skips_an_independent_model_owned_by_another_protocol() {
        let relay = compaction_relay(
            "ws-cross-protocol",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("deepseek-chat", RelayProtocol::ChatCompletions, "128000"),
            ],
        );
        let settings = independent_compaction_settings("deepseek-chat");
        let payload = legacy_compaction_payload("gpt-original", "ws-cross-protocol-key");
        let (forwarded, _, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let plan = plan.unwrap();

        assert_eq!(forwarded["model"], "gpt-original");
        assert_eq!(plan.model, "gpt-original");
        assert!(!plan.independent);
        assert!(plan.independent_compaction_model.is_none());
        assert!(plan.independent_compaction_usage.is_none());
    }

    #[test]
    fn websocket_independent_response_restores_the_original_model_structurally() {
        let relay = compaction_relay(
            "ws-model-restore",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("gpt-cheap");
        let payload = legacy_compaction_payload("gpt-original", "ws-model-restore-key");
        let (_, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings_and_plan(&payload, None, options, &settings, plan)
            .unwrap();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp-independent",
                        "status": "completed",
                        "model": "gpt-cheap",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": "<summary>kept</summary>" }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("valid compaction should flush");
        };
        let sse = super::websocket_messages_to_responses_sse(&messages);
        let terminal = crate::continue_thinking::extract_terminal_response_object(&sse).unwrap();
        assert_eq!(terminal["model"], "gpt-original");
        assert!(sse.contains("kept"));
    }

    #[test]
    fn websocket_independent_transport_failure_uses_original_model_once() {
        let relay = compaction_relay(
            "ws-transport-fallback",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("gpt-cheap");
        let payload = legacy_compaction_payload("gpt-original", "ws-transport-fallback-key");
        let (_, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings_and_plan(&payload, None, options, &settings, plan)
            .unwrap();

        let action = coordinator
            .handle_upstream_message(Message::Close(None))
            .unwrap();
        let WebSocketContinuationAction::Continue {
            request: Message::Text(request),
            metadata,
        } = action
        else {
            panic!("transport failure should use the only original-model fallback");
        };
        let request: Value = serde_json::from_str(&request).unwrap();
        assert_eq!(request["model"], "gpt-original");
        assert!(metadata.reconnect_upstream);

        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp-original-fallback",
                        "status": "completed",
                        "model": "gpt-original",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": "<summary>fallback</summary>" }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        assert!(matches!(action, WebSocketContinuationAction::Flush { .. }));
        assert!(coordinator.state.lock().unwrap().active.is_none());
    }

    #[test]
    fn websocket_native_remote_v2_ignores_the_independent_model_setting() {
        let relay = compaction_relay(
            "ws-native-v2",
            &[
                ("gpt-original", RelayProtocol::Responses, "372000"),
                ("gpt-cheap", RelayProtocol::Responses, "128000"),
            ],
        );
        let settings = independent_compaction_settings("gpt-cheap");
        let payload = json!({
            "type": "response.create",
            "model": "gpt-original",
            "prompt_cache_key": "ws-native-v2-key",
            "input": [{ "type": "compaction_trigger" }]
        });
        let (forwarded, options, plan) = prepare_websocket_compaction_execution(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        );
        assert_eq!(forwarded, payload);
        assert!(options.is_none());
        assert!(plan.is_none());
    }

    #[test]
    fn compaction_contract_websocket_validates_both_attempts_with_retention_on_and_off() {
        use crate::layered_compaction::*;
        for retain in [false, true] {
            for legacy in [false, true] {
                for valid_retry in [false, true] {
                    let settings = BackendSettings {
                        layered_compaction_enabled: true,
                        layered_compaction_retain_recent_round_enabled: retain,
                        ..Default::default()
                    };
                    let mut payload = json!({
                        "type":"response.create","model":"claude-opus-5-5",
                        "instructions":"MAIN","tools":[{"type":"function","name":"read"}],
                        "input":[
                            {"type":"message","role":"user","content":"earlier"},
                            {"type":"message","role":"assistant","content":"done"},
                            {"type":"message","role":"user","content":"next"}
                        ]
                    });
                    payload["input"].as_array_mut().unwrap().push(if legacy {
                        json!({"type":"message","role":"user","content":format!("{COMPACTION_PROMPT_PREFIX}.")})
                    } else { json!({"type":"compaction_trigger"}) });
                    let (original, first, options) =
                        prepare_downstream_response_create_payload_with_settings(
                            payload, &settings,
                        );
                    assert_eq!(first["model"], original["model"]);
                    let coordinator = WebSocketContinuationCoordinator::default();
                    coordinator
                        .register_request_with_settings(&original, None, options, &settings)
                        .unwrap();
                    let response = |id: &str, text: &str, tool: bool| {
                        let mut output = vec![json!({"type":"message","role":"assistant",
                            "content":[{"type":"output_text","text":text}]})];
                        if tool {
                            output.push(json!({"type":"function_call","call_id":"call-1","name":"read","arguments":"{}"}));
                        }
                        Message::Text(json!({"type":"response.completed",
                            "response":{"id":id,"status":"completed","model":"claude-opus-5-5","output":output}
                        }).to_string().into())
                    };
                    let action = coordinator
                        .handle_upstream_message(response(
                            "first",
                            "<summary>must reject tool output</summary>",
                            true,
                        ))
                        .unwrap();
                    let WebSocketContinuationAction::Continue {
                        request: Message::Text(retry),
                        ..
                    } = action
                    else {
                        panic!("首次无效输出必须同模型重试");
                    };
                    let retry: Value = serde_json::from_str(&retry).unwrap();
                    assert_eq!(retry["model"], first["model"]);
                    assert_eq!(retry["input"], first["input"]);
                    assert_eq!(retry["instructions"], COMPACTION_RETRY_SYSTEM_PROMPT);
                    assert!(retry.get("tools").is_none());
                    assert!(matches!(
                        coordinator
                            .handle_upstream_message(response(
                                "first",
                                "<summary>discarded first attempt</summary>",
                                false,
                            ))
                            .unwrap(),
                        WebSocketContinuationAction::Buffered
                    ));
                    let action = coordinator
                        .handle_upstream_message(response(
                            "retry",
                            if valid_retry {
                                "<analysis>notes</analysis><summary>done</summary>"
                            } else {
                                "<analysis>no summary</analysis>"
                            },
                            false,
                        ))
                        .unwrap();
                    let WebSocketContinuationAction::Flush { messages, .. } = action else {
                        panic!("第二次尝试必须终止");
                    };
                    let sse = super::websocket_messages_to_responses_sse(&messages);
                    let terminal =
                        crate::continue_thinking::extract_terminal_response_object(&sse).unwrap();
                    assert_eq!(
                        terminal["status"],
                        if valid_retry { "completed" } else { "failed" }
                    );
                    if !valid_retry {
                        assert_eq!(terminal["output"], json!([]));
                    }
                    assert!(!sse.contains("must reject tool output"));
                    assert!(!sse.contains("notes"));
                    assert!(coordinator.state.lock().unwrap().active.is_none());
                }
            }
        }
    }
    #[test]
    fn disabled_compaction_preserves_websocket_requests_and_responses() {
        let settings = BackendSettings {
            layered_compaction_enabled: false,
            layered_compaction_retain_recent_round_enabled: true,
            layered_compaction_prompt_override: "MUST NOT APPLY".to_string(),
            gpt_reasoning_continuation: true,
            ..Default::default()
        };
        for model in ["gpt-test", "claude-test", "deepseek-test"] {
            for legacy in [false, true] {
                let control = if legacy {
                    json!({"role":"user","content":format!("{}. Harness instruction.",
                        crate::layered_compaction::COMPACTION_PROMPT_PREFIX)})
                } else {
                    json!({"type":"compaction_trigger"})
                };
                let payload = json!({
                    "type":"response.create","model":model,
                    "instructions":"HARNESS SYSTEM",
                    "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}],
                    "input":[{"role":"user","content":"original history"},control]
                });
                let (original, forwarded, options) =
                    prepare_downstream_response_create_payload_with_settings(
                        payload.clone(),
                        &settings,
                    );
                assert_eq!(original, payload);
                assert_eq!(forwarded, payload);
                assert!(options.is_none());
                let coordinator = WebSocketContinuationCoordinator::default();
                coordinator
                    .register_request_with_settings(
                        &original,
                        None,
                        Some(crate::protocol_proxy::LayeredCompactionOptions {
                            enabled: true,
                            retain_tokens: 20_000,
                        }),
                        &settings,
                    )
                    .unwrap();
                let response = Message::Text(json!({
                    "type":"response.completed","response":{
                        "status":"completed","output":[
                            {"type":"message","role":"assistant","content":[
                                {"type":"output_text","text":"Harness summary without tags"}
                            ]},
                            {"type":"function_call","name":"read","call_id":"c1","arguments":"{}"}
                        ]
                    }
                }).to_string().into());
                let WebSocketContinuationAction::Forward(forwarded) = coordinator
                    .handle_upstream_message(response.clone())
                    .unwrap()
                else {
                    panic!("关闭压缩增强时不得校验、缓冲或重试 harness 响应");
                };
                assert_eq!(forwarded, response);
                assert!(coordinator.state.lock().unwrap().active.is_none());
            }
        }
    }

    #[test]
    fn compaction_contract_websocket_reconnects_once_after_invalid_or_closed_stream() {
        for first_failure in [
            Message::Close(None),
            Message::Text("invalid json".into()),
            Message::Binary(vec![0xff].into()),
        ] {
            let settings = BackendSettings {
                layered_compaction_enabled: true,
                ..Default::default()
            };
            let payload = json!({
                "type":"response.create","model":"claude-opus-5-5",
                "instructions":"MAIN",
                "input":[
                    {"type":"message","role":"user","content":"summarize this"},
                    {"type":"compaction_trigger"}
                ]
            });
            let (original, _, options) =
                prepare_downstream_response_create_payload_with_settings(payload, &settings);
            let coordinator = WebSocketContinuationCoordinator::default();
            coordinator
                .register_request_with_settings(&original, None, options, &settings)
                .unwrap();
            assert!(matches!(
                coordinator
                    .handle_upstream_message(Message::Ping(vec![1].into()))
                    .unwrap(),
                WebSocketContinuationAction::Forward(Message::Ping(_))
            ));
            let WebSocketContinuationAction::Continue { metadata, .. } =
                coordinator.handle_upstream_message(first_failure).unwrap()
            else {
                panic!("首次流中断或非法帧必须重试");
            };
            assert!(metadata.reconnect_upstream);
            let WebSocketContinuationAction::Flush { messages, .. } = coordinator
                .handle_upstream_message(Message::Close(None))
                .unwrap()
            else {
                panic!("第二次中断必须返回失败");
            };
            let terminal = crate::continue_thinking::extract_terminal_response_object(
                &super::websocket_messages_to_responses_sse(&messages),
            )
            .unwrap();
            assert_eq!(terminal["status"], "failed");
            assert_eq!(terminal["output"], json!([]));
            assert!(matches!(messages.last(), Some(Message::Close(_))));
            assert!(coordinator.state.lock().unwrap().active.is_none());
        }
    }
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    #[test]
    fn only_responses_upgrade_paths_are_accepted() {
        assert!(is_responses_websocket_proxy_path("/v1/responses"));
        assert!(!is_responses_websocket_proxy_path("/v1/responses/compact"));
        assert!(!is_responses_websocket_proxy_path("/v1/chat/completions"));
    }

    #[tokio::test]
    async fn stale_native_connection_cannot_arm_a_new_session_generation() {
        let relay = crate::settings::RelayProfile::default();
        let context = crate::request_headers::RequestContext::from_http_request(
            format!(
                "GET /v1/responses HTTP/1.1\r\nthread-id: {}\r\n\r\n",
                uuid::Uuid::new_v4()
            )
            .as_bytes(),
        );
        let request = json!({"type":"response.create","model":"gpt-test","input":"hi"});
        crate::session_transport::observe_native_request(&relay, &context, &request, 100);
        let logger =
            WebSocketRequestLogger::new(&relay, None, "/v1/responses".into(), Default::default());
        logger.set_transport_context(relay.clone(), context.clone());
        logger.record_request(&request, &request.to_string());
        crate::session_transport::forget_thread(&context);
        crate::session_transport::observe_native_request(&relay, &context, &request, 100);
        logger.mark_http_fallback("old connection failed after a model switch");
        assert!(!crate::session_transport::should_use_http(&relay, &context));
        crate::session_transport::forget_thread(&context);
    }

    #[test]
    fn detects_websocket_upgrade_without_consuming_trailing_frame_bytes() {
        let mut request = b"GET /v1/responses HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive, Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n".to_vec();
        request.extend_from_slice(&[0x81, 0x00]);

        assert!(is_responses_websocket_upgrade(&request));
    }

    #[test]
    fn websocket_connection_context_extracts_session_thread_and_turn_headers() {
        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri("ws://127.0.0.1/v1/responses")
            .header("session-id", "session-a")
            .header("thread-id", "thread-a")
            .header("x-codex-window-id", "thread-a:3")
            .header(
                "x-codex-turn-metadata",
                r#"{"turn_id":"turn-a","request_kind":"sampling","window_id":"fallback-window"}"#,
            )
            .body(())
            .unwrap();

        let context = WebSocketConnectionContext::from_request(&request);

        assert!(
            context
                .session_id
                .as_deref()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert!(
            context
                .thread_id
                .as_deref()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert!(
            context
                .turn_id
                .as_deref()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert_eq!(context.request_kind.as_deref(), Some("sampling"));
        assert!(
            context
                .window_id
                .as_deref()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert_ne!(context.session_id.as_deref(), Some("session-a"));
        assert_ne!(context.thread_id.as_deref(), Some("thread-a"));
    }

    #[test]
    fn websocket_http_fallback_is_always_available() {
        let context = WebSocketConnectionContext {
            thread_id: Some("sha256:thread".to_string()),
            turn_id: Some("sha256:turn".to_string()),
            request_kind: Some("turn".to_string()),
            ..Default::default()
        };
        assert!(arm_oversized_responses_websocket_http_fallback(
            "relay-test",
            &context
        ));
        assert!(
            should_temporarily_fallback_oversized_responses_websocket_to_http(
                "relay-test",
                &context
            )
        );
        assert!(arm_early_disconnect_responses_websocket_http_fallback(
            "relay-test",
            &context
        ));
        assert!(
            should_temporarily_fallback_early_disconnect_responses_websocket_to_http(
                "relay-test",
                &context
            )
        );
        assert!(arm_upstream_failure_responses_websocket_http_fallback(
            "relay-test",
            &context
        ));
        assert!(
            should_temporarily_fallback_upstream_failure_responses_websocket_to_http(
                "relay-test",
                &context
            )
        );
        let close = websocket_failure_close_frame(CloseCode::Restart, "retry");
        assert_eq!(close.code, CloseCode::Restart);
        assert_eq!(close.reason, "retry");
    }

    #[test]
    fn websocket_prunes_stateless_history_before_latest_compaction() {
        let historical_image = format!("data:image/png;base64,{}", "A".repeat(17 * 1024 * 1024));
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "store": false,
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_image",
                        "image_url": historical_image
                    }]
                },
                {
                    "type": "compaction",
                    "id": "cmp_latest",
                    "encrypted_content": "opaque"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "继续"
                    }]
                }
            ]
        });

        let (request_payload, forwarded_payload, _) =
            prepare_downstream_response_create_payload(&payload).unwrap();

        assert_eq!(request_payload["input"].as_array().unwrap().len(), 2);
        assert_eq!(forwarded_payload["input"][0]["type"], "compaction");
        assert_eq!(forwarded_payload["input"][1]["content"][0]["text"], "继续");
        assert!(
            serde_json::to_string(&forwarded_payload).unwrap().len()
                <= crate::responses_websocket::RESPONSES_UPSTREAM_WEBSOCKET_SAFE_MAX_BYTES
        );
    }

    #[test]
    fn websocket_does_not_prune_previous_response_id_chaining() {
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "previous_response_id": "resp_previous",
            "input": [
                {"type": "message", "role": "user", "content": "历史"},
                {"type": "compaction", "id": "cmp_latest", "encrypted_content": "opaque"},
                {"type": "message", "role": "user", "content": "继续"}
            ]
        });

        let (request_payload, forwarded_payload, _) =
            prepare_downstream_response_create_payload(&payload).unwrap();

        assert_eq!(request_payload["input"].as_array().unwrap().len(), 3);
        assert_eq!(forwarded_payload["input"].as_array().unwrap().len(), 3);
        assert_eq!(forwarded_payload["previous_response_id"], "resp_previous");
    }

    #[test]
    fn websocket_transport_heartbeat_does_not_reset_application_idle_budget() {
        assert!(websocket_application_event_payload(&Message::Ping(vec![].into())).is_none());
        assert!(websocket_application_event_payload(&Message::Pong(vec![].into())).is_none());
        assert!(
            websocket_application_event_payload(&Message::Text(
                r#"{"type":"response.output_text.delta","delta":"ok"}"#
                    .to_string()
                    .into()
            ))
            .is_some()
        );

        let started_at = Instant::now();
        let idle_timeout = Duration::from_secs(900);
        assert!(
            websocket_idle_timeout_elapsed(
                started_at,
                idle_timeout,
                started_at + Duration::from_secs(899)
            )
            .is_none()
        );
        assert_eq!(
            websocket_idle_timeout_elapsed(started_at, idle_timeout, started_at + idle_timeout),
            Some(idle_timeout)
        );
    }

    #[test]
    fn websocket_records_raw_response_model_per_request_before_response_rewriting() {
        let logger = WebSocketRequestLogger::new(
            &RelayProfile::default(),
            None,
            "/v1/responses".into(),
            WebSocketConnectionContext::default(),
        );
        let request = json!({"type":"response.create","model":"gpt-6-astra","input":[]});
        let first_id = logger
            .record_request(&request, &request.to_string())
            .unwrap();
        let second_id = logger
            .record_request(&request, &request.to_string())
            .unwrap();
        let first_created = Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_first","model":"gpt-6-astra"}}"#
                .into(),
        );
        logger.record_first_response_event(&first_created);
        assert_eq!(
            logger.state.lock().unwrap().requests[&first_id]
                .record
                .upstream_response_model
                .as_deref(),
            Some("gpt-6-astra")
        );
        logger.record_response(&first_created);
        let first_completed = Message::Text(
            r#"{"type":"response.completed","response":{"id":"resp_first","model":"gpt-6-astra"}}"#
                .into(),
        );
        logger.record_first_response_event(&first_completed);
        logger.record_response(&first_completed);

        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_second","model":"gpt-5.6-sol"}}"#
                .into(),
        ));
        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.completed","response":{"id":"resp_second","model":"gpt-5.6-luna"}}"#.into(),
        ));
        logger.record_response(&Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_second","model":"gpt-6-astra"}}"#
                .into(),
        ));

        let state = logger.state.lock().unwrap();
        assert!(!state.requests.contains_key(&first_id));
        assert_eq!(
            state.requests[&second_id]
                .record
                .upstream_response_model
                .as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    #[test]
    fn websocket_independent_compaction_log_keeps_model_roles_distinct() {
        let logger = WebSocketRequestLogger::new(
            &RelayProfile::default(),
            None,
            "/v1/responses".into(),
            WebSocketConnectionContext::default(),
        );
        let request = json!({"type":"response.create","model":"claude-opus-5-5","input":[]});
        let id = logger
            .record_request(&request, &request.to_string())
            .unwrap();
        logger.record_independent_compaction(
            Some(&id),
            Some("deepseek-v4.1-flash"),
            Some(crate::settings::LayeredCompactionModelUsage::CacheMiss),
        );
        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_compaction","model":"deepseek-v4.1-flash"}}"#
                .into(),
        ));

        {
            let state = logger.state.lock().unwrap();
            let record = &state.requests[&id].record;
            assert_eq!(record.model.as_deref(), Some("claude-opus-5-5"));
            assert_eq!(
                record.upstream_request_model.as_deref(),
                Some("deepseek-v4.1-flash")
            );
            assert_eq!(
                record.upstream_response_model.as_deref(),
                Some("deepseek-v4.1-flash")
            );
            assert_eq!(
                record.independent_compaction_model.as_deref(),
                Some("deepseek-v4.1-flash")
            );
        }

        logger.record_continue_metadata(&WebSocketContinueMetadata {
            log_id: Some(id.clone()),
            request_body: Some(
                json!({"type":"response.create","model":"claude-opus-5-5","input":[]}).to_string(),
            ),
            ..Default::default()
        });
        assert_eq!(
            logger.state.lock().unwrap().requests[&id]
                .record
                .upstream_request_model
                .as_deref(),
            Some("claude-opus-5-5")
        );
    }

    #[test]
    fn websocket_ping_replay_only_allows_pending_requests_without_application_response_or_context_id()
     {
        let logger = WebSocketRequestLogger::new(
            &RelayProfile::default(),
            None,
            "/v1/responses".to_string(),
            WebSocketConnectionContext::default(),
        );
        assert!(!logger.has_pending_requests());
        assert_eq!(logger.upstream_replay_block_reason(), None);

        let request = json!({"type":"response.create","model":"gpt-5.6","input":[]});
        logger
            .record_request(&request, &request.to_string())
            .unwrap();
        assert_eq!(logger.upstream_replay_block_reason(), None);
        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_first"}}"#.into(),
        ));
        assert_eq!(
            logger.upstream_replay_block_reason(),
            Some("application_response_already_received")
        );
        logger.record_response(&Message::Text(
            r#"{"type":"response.completed","response":{"id":"resp_first"}}"#.into(),
        ));
        assert!(!logger.has_pending_requests());
        assert_eq!(logger.upstream_replay_block_reason(), None);

        // 上一轮收到过响应，不影响下一轮尚未收到响应的完整请求恢复。
        logger
            .record_request(&request, &request.to_string())
            .unwrap();
        assert!(!logger.interrupted_before_response());
        assert_eq!(logger.upstream_replay_block_reason(), None);
        let chained = json!({
            "type":"response.create","model":"gpt-5.6",
            "previous_response_id":"resp_first","input":[]
        });
        logger
            .record_request(&chained, &chained.to_string())
            .unwrap();
        assert_eq!(
            logger.upstream_replay_block_reason(),
            Some("previous_response_id_requires_context_recovery")
        );
        logger.finish_pending("upstream failed on a later request", 502);
        assert!(logger.interrupted_before_response());
    }

    #[tokio::test]
    async fn websocket_terminal_response_is_completed_only_after_downstream_send_succeeds() {
        use futures_util::SinkExt;
        let logger = WebSocketRequestLogger::new(
            &RelayProfile::default(),
            None,
            "/v1/responses".into(),
            WebSocketConnectionContext::default(),
        );
        let request = json!({"type":"response.create","model":"gpt-5.6","input":[]});
        let id = logger
            .record_request(&request, &request.to_string())
            .unwrap();
        let response = Message::Text(
            r#"{"type":"response.completed","response":{"id":"resp_delivery","status":"completed"}}"#.into(),
        );
        logger.record_first_response_event(&response);
        let mut failed_sink = Box::pin(futures_util::sink::unfold((), |(), _: Message| async {
            Err::<(), _>(tokio_tungstenite::tungstenite::Error::ConnectionClosed)
        }));
        assert!(
            super::forward_downstream_response(&mut failed_sink, response.clone(), &logger, None,)
                .await
                .is_err()
        );
        {
            let state = logger.state.lock().unwrap();
            let tracked = state
                .requests
                .get(&id)
                .expect("failed delivery must remain pending");
            assert_eq!(
                tracked.record.state,
                crate::proxy_log::ProxyRequestState::Pending
            );
            assert!(tracked.record.duration_ms.is_none());
        }
        let mut delivered_sink =
            futures_util::sink::drain::<Message>().sink_map_err(|never| match never {});
        super::forward_downstream_response(&mut delivered_sink, response, &logger, None)
            .await
            .unwrap();
        assert!(!logger.has_pending_requests());
    }

    #[test]
    fn websocket_terminal_failure_preserves_the_current_upstream_response_id() {
        let logger = WebSocketRequestLogger::new(
            &RelayProfile::default(),
            None,
            "/v1/responses".into(),
            WebSocketConnectionContext::default(),
        );
        let request = json!({"type":"response.create","model":"gpt-5.6","input":[]});
        logger
            .record_request(&request, &request.to_string())
            .unwrap();
        let failure_id = || {
            let Message::Text(text) = logger.pending_response_failure_message("timeout").unwrap()
            else {
                panic!("expected response.failed");
            };
            serde_json::from_str::<Value>(&text).unwrap()["response"]["id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(failure_id().starts_with("resp_codex_elves_failed_"));
        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.created","response":{"id":"resp_original"}}"#.into(),
        ));
        assert_eq!(failure_id(), "resp_original");
        logger.record_first_response_event(&Message::Text(
            r#"{"type":"response.output_text.delta","response_id":"resp_original","delta":"hi"}"#
                .into(),
        ));
        assert_eq!(failure_id(), "resp_original");
        logger.finish_pending("interrupted after output", 502);
        assert!(!logger.interrupted_before_response());
    }

    #[test]
    fn websocket_transport_liveness_pings_idle_connections_and_times_out_without_activity() {
        let started_at = Instant::now();
        let mut liveness = WebSocketTransportLiveness::default();

        let payload = liveness
            .begin_ping(started_at)
            .expect("idle websocket should still create a ping");

        assert!(payload.starts_with(b"codex-elves:"));
        assert!(liveness.begin_ping(started_at).is_none());
        assert_eq!(
            liveness.timeout_elapsed(
                Duration::from_secs(15),
                started_at + Duration::from_secs(14)
            ),
            None
        );
        assert_eq!(
            liveness.timeout_elapsed(
                Duration::from_secs(15),
                started_at + Duration::from_secs(15)
            ),
            Some(Duration::from_secs(15))
        );
        for attempt in 1..=4 {
            let ping_at = started_at + Duration::from_secs((attempt - 1) * 30);
            if attempt > 1 {
                assert!(liveness.begin_ping(ping_at).is_some());
            }
            let timeout_at = ping_at + Duration::from_secs(15);
            assert!(
                liveness
                    .take_timeout(Duration::from_secs(15), timeout_at)
                    .is_some()
            );
            assert_eq!(liveness.consecutive_timeouts, attempt as usize);
            assert_eq!(liveness.timeout_limit_exceeded(), attempt > 3);
            assert!(
                liveness
                    .take_timeout(Duration::from_secs(15), timeout_at + Duration::from_secs(5))
                    .is_none(),
                "the same expired ping must not be counted again"
            );
        }
    }

    #[tokio::test]
    async fn websocket_transport_liveness_queues_ping_without_application_request() {
        let (upstream_tx, mut upstream_rx) = tokio::sync::mpsc::channel(1);
        let mut liveness = WebSocketTransportLiveness::default();

        queue_upstream_liveness_ping(&upstream_tx, &mut liveness, Instant::now())
            .await
            .unwrap();

        let message = upstream_rx.recv().await.expect("ping should be queued");
        assert!(matches!(message, Message::Ping(_)));
    }

    #[test]
    fn websocket_transport_liveness_accepts_any_upstream_frame_as_transport_activity() {
        let started_at = Instant::now();
        let mut liveness = WebSocketTransportLiveness::default();
        liveness
            .begin_ping(started_at)
            .expect("ping should be created");

        let pong_rtt = liveness.observe_frame(
            &Message::Text(r#"{"type":"response.created"}"#.into()),
            started_at + Duration::from_secs(5),
        );

        assert_eq!(pong_rtt, None);
        assert_eq!(
            liveness.timeout_elapsed(
                Duration::from_secs(15),
                started_at + Duration::from_secs(30)
            ),
            None
        );
        for message in [
            Message::Pong(Vec::new().into()),
            Message::Text(r#"{"type":"response.completed"}"#.into()),
        ] {
            for attempt in 0..3 {
                let ping_at = started_at + Duration::from_secs(attempt * 30);
                assert!(liveness.begin_ping(ping_at).is_some());
                liveness
                    .take_timeout(Duration::from_secs(15), ping_at + Duration::from_secs(15))
                    .unwrap();
            }
            assert_eq!(liveness.consecutive_timeouts, 3);
            liveness.observe_frame(&message, started_at + Duration::from_secs(80));
            assert_eq!(liveness.consecutive_timeouts, 0);
            let ping_at = started_at + Duration::from_secs(90);
            assert!(liveness.begin_ping(ping_at).is_some());
            liveness
                .take_timeout(Duration::from_secs(15), ping_at + Duration::from_secs(15))
                .unwrap();
            assert_eq!(liveness.consecutive_timeouts, 1);
            assert!(!liveness.timeout_limit_exceeded());
            liveness.observe_frame(&message, started_at + Duration::from_secs(110));
        }
    }

    #[test]
    fn websocket_transport_liveness_records_matching_pong_round_trip() {
        let started_at = Instant::now();
        let mut liveness = WebSocketTransportLiveness::default();
        let payload = liveness
            .begin_ping(started_at)
            .expect("ping should be created");

        let pong_rtt = liveness.observe_frame(
            &Message::Pong(payload.into()),
            started_at + Duration::from_millis(125),
        );

        assert_eq!(pong_rtt, Some(Duration::from_millis(125)));
        assert_eq!(liveness.last_pong_rtt, Some(Duration::from_millis(125)));
    }

    #[test]
    fn websocket_queued_request_starts_idle_timer_only_after_activation() {
        let relay = RelayProfile {
            id: "relay-test".to_string(),
            name: "Relay Test".to_string(),
            ..RelayProfile::default()
        };
        let logger = WebSocketRequestLogger::new(
            &relay,
            None,
            "/v1/responses".to_string(),
            WebSocketConnectionContext::default(),
        );
        let first = logger
            .record_request(
                &json!({"type":"response.create","model":"gpt-5.6","stream":true}),
                r#"{"type":"response.create","model":"gpt-5.6","stream":true}"#,
            )
            .unwrap();
        let second = logger
            .record_request(
                &json!({"type":"response.create","model":"gpt-5.6","stream":true}),
                r#"{"type":"response.create","model":"gpt-5.6","stream":true}"#,
            )
            .unwrap();
        let now = Instant::now();
        {
            let mut state = logger.state.lock().unwrap();
            let first_request = state.requests.get_mut(&first).unwrap();
            first_request.last_upstream_event_at = Some(now);
            first_request.idle_timeout = Duration::from_secs(10);
            let second_request = state.requests.get_mut(&second).unwrap();
            second_request.idle_timeout = Duration::from_millis(1);
            assert!(second_request.last_upstream_event_at.is_none());
        }

        assert!(
            logger
                .expired_pending_request(now + Duration::from_millis(5))
                .is_none()
        );

        let activated_at = now + Duration::from_millis(5);
        {
            let mut state = logger.state.lock().unwrap();
            state.requests.remove(&first);
            state.active_order.retain(|id| id != &first);
            state.unassigned_order.retain(|id| id != &first);
            activate_next_queued_websocket_request(&mut state, activated_at);
            assert_eq!(
                state
                    .requests
                    .get(&second)
                    .and_then(|tracked| tracked.last_upstream_event_at),
                Some(activated_at)
            );
        }
    }

    #[test]
    fn websocket_continuation_response_ids_stay_bound_to_current_request() {
        let relay = RelayProfile {
            id: "relay-test".to_string(),
            name: "Relay Test".to_string(),
            ..RelayProfile::default()
        };
        let logger = WebSocketRequestLogger::new(
            &relay,
            None,
            "/v1/responses".to_string(),
            WebSocketConnectionContext::default(),
        );
        let first = logger
            .record_request(
                &json!({"type":"response.create","model":"gpt-5.6","stream":true}),
                "{}",
            )
            .unwrap();
        let second = logger
            .record_request(
                &json!({"type":"response.create","model":"gpt-5.6","stream":true}),
                "{}",
            )
            .unwrap();
        let mut state = logger.state.lock().unwrap();

        assert_eq!(
            resolve_websocket_log_id(&mut state, Some("resp-first")).as_deref(),
            Some(first.as_str())
        );
        assert_eq!(
            resolve_websocket_log_id(&mut state, Some("resp-continuation")).as_deref(),
            Some(first.as_str())
        );
        assert!(
            state
                .unassigned_order
                .iter()
                .any(|log_id| log_id == &second)
        );
    }

    #[test]
    fn websocket_restores_synthetic_compaction_and_preserves_real_trigger() {
        let compaction_request = json!({
            "model": "claude-sonnet-5",
            "input": [{ "type": "compaction_trigger" }]
        });
        let source_response = json!({
            "id": "resp_bridge",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "<summary>WEBSOCKET SUMMARY</summary>" }]
            }]
        });
        let compacted = crate::layered_compaction::rewrite_remote_compaction_v2_response(
            &compaction_request,
            &source_response,
        )
        .expect("bridge should create a synthetic compaction");
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [
                compacted["output"][0].clone(),
                { "type": "compaction_trigger" }
            ]
        });
        let (normalized_payload, forwarded_payload, _) =
            prepare_downstream_response_create_payload(&payload).unwrap();

        assert_eq!(normalized_payload, payload);
        assert_eq!(forwarded_payload, payload);
    }

    #[test]
    fn websocket_claude_assistant_tail_completes_locally_without_output_items() {
        let compaction_request = json!({
            "model": "claude-sonnet-5",
            "input": [{ "type": "compaction_trigger" }]
        });
        let source_response = json!({
            "id": "resp_bridge",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>" }]
            }]
        });
        let compacted = crate::layered_compaction::rewrite_remote_compaction_v2_response(
            &compaction_request,
            &source_response,
        )
        .expect("synthetic compaction");
        let payload = json!({
            "type": "response.create",
            "model": "claude-sonnet-5",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "earlier user" }]
                },
                compacted["output"][0].clone()
            ]
        });
        let messages =
            local_compaction_wait_websocket_messages(&payload).expect("must pause locally");
        let payloads = messages
            .iter()
            .filter_map(|message| {
                let Message::Text(text) = message else {
                    return None;
                };
                serde_json::from_str::<Value>(text.as_str()).ok()
            })
            .collect::<Vec<_>>();

        assert!(payloads.iter().any(|payload| {
            payload.get("type").and_then(Value::as_str) == Some("response.completed")
        }));
        assert!(!payloads.iter().any(|payload| {
            matches!(
                payload.get("type").and_then(Value::as_str),
                Some("response.output_item.added" | "response.output_item.done")
            )
        }));
    }

    #[tokio::test]
    async fn websocket_compaction_wait_survives_the_request_preparation_boundary() {
        let relay = compaction_relay(
            "ws-compaction-wait",
            &[("claude-test", RelayProtocol::Responses, "128000")],
        );
        let settings = BackendSettings::default();
        let mut payload = json!({
            "type": "response.create", "model": "claude-test",
            "input": [{
                "type": "compaction",
                "encrypted_content": "codex-elves-compaction-v2:HANDOFF"
            }]
        });
        let (snapshot, _, _, _) = super::prepare_downstream_response_create_payload_with_snapshot(
            &payload,
            &settings,
            &relay,
            &crate::request_headers::RequestContext::default(),
        )
        .await;
        assert!(local_compaction_wait_websocket_messages(&snapshot).is_some());
        let mut compact = payload.clone();
        compact["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "compaction_trigger"}));
        let (snapshot, forwarded, _, plan) =
            super::prepare_downstream_response_create_payload_with_snapshot(
                &compact,
                &BackendSettings {
                    layered_compaction_enabled: true,
                    ..Default::default()
                },
                &relay,
                &crate::request_headers::RequestContext::default(),
            )
            .await;
        assert!(local_compaction_wait_websocket_messages(&snapshot).is_none());
        assert!(plan.is_some());
        assert!(forwarded.to_string().contains("Handoff checkpoint"));
        payload["input"].as_array_mut().unwrap().push(json!({
            "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "continue"}]
        }));
        let (snapshot, forwarded, _, _) =
            super::prepare_downstream_response_create_payload_with_snapshot(
                &payload,
                &settings,
                &relay,
                &crate::request_headers::RequestContext::default(),
            )
            .await;
        assert!(local_compaction_wait_websocket_messages(&snapshot).is_none());
        assert!(!forwarded.to_string().contains("codex-elves-compaction-v2:"));
    }

    #[test]
    fn websocket_non_gpt_remote_compaction_uses_summary_bridge() {
        let payload = json!({
            "type": "response.create",
            "model": "claude-sonnet-5",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "keep this context" }]
                },
                { "type": "compaction_trigger" }
            ],
            "tools": [{ "type": "function", "name": "exec_command" }]
        });
        let (request_payload, forwarded_payload, options) =
            prepare_downstream_response_create_payload_with_settings(
                payload.clone(),
                &BackendSettings {
                    layered_compaction_enabled: true,
                    ..Default::default()
                },
            );
        assert_eq!(request_payload["input"][1]["type"], "compaction_trigger");
        assert_eq!(forwarded_payload["input"][0]["type"], "message");
        assert_eq!(forwarded_payload["tools"], payload["tools"]);
        assert!(options.is_some());
    }

    #[test]
    fn websocket_bridged_remote_prompt_only_preserves_recent_history() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_prompt_override: "CUSTOM V2 PROMPT".to_string(),
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "claude-sonnet-5",
            "input": [
                { "type": "message", "role": "user", "content": "earlier request" },
                { "type": "message", "role": "assistant", "content": "recent answer" },
                { "type": "message", "role": "user", "content": "recent request" },
                { "type": "compaction_trigger" }
            ]
        });
        let (_, forwarded, options) =
            prepare_downstream_response_create_payload_with_settings(payload.clone(), &settings);
        assert_eq!(
            &forwarded["input"].as_array().unwrap()[..3],
            &payload["input"].as_array().unwrap()[..3]
        );
        assert_eq!(
            forwarded["input"][3]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("CUSTOM V2 PROMPT")
        );
        assert!(!options.unwrap().enabled);
    }

    #[test]
    fn websocket_legacy_compaction_uses_project_default_prompt_and_removes_tools() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_retain_recent_round_enabled: true,
            layered_compaction_prompt_override: String::new(),
            layered_compaction_retain_tokens: 23_456,
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "keep this context" }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
                    }]
                }
            ],
            "tools": [{ "type": "function", "name": "exec_command" }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        });
        let (request_payload, forwarded_payload, options) =
            prepare_downstream_response_create_payload_with_settings(payload, &settings);

        assert!(
            request_payload["input"][1]["content"][0]["text"]
                .as_str()
                .is_some_and(
                    |text| text.starts_with(crate::layered_compaction::COMPACTION_PROMPT_PREFIX)
                )
        );
        assert_eq!(
            forwarded_payload["input"][0]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("")
        );
        assert!(forwarded_payload.get("tools").is_none());
        assert!(forwarded_payload.get("tool_choice").is_none());
        assert!(forwarded_payload.get("parallel_tool_calls").is_none());
        let options = options.expect("legacy compaction should capture settings");
        assert!(options.enabled);
        assert_eq!(options.retain_tokens, 23_456);
    }

    #[test]
    fn websocket_legacy_compaction_without_recent_round_keeps_history_and_changes_prompt() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_prompt_override: "CUSTOM PROMPT".to_string(),
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [
                { "type": "message", "role": "user", "content": "earlier request" },
                { "type": "message", "role": "assistant", "content": "recent answer" },
                { "type": "message", "role": "user", "content": "recent request" },
                { "type": "message", "role": "user", "content":
                    "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary." }
            ]
        });
        let (_, forwarded, options) =
            prepare_downstream_response_create_payload_with_settings(payload.clone(), &settings);
        assert_eq!(
            &forwarded["input"].as_array().unwrap()[..3],
            &payload["input"].as_array().unwrap()[..3]
        );
        assert_eq!(
            forwarded["input"][3]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("CUSTOM PROMPT")
        );
        assert!(!options.unwrap().enabled);
    }

    #[test]
    fn websocket_legacy_prompt_only_forwards_summary_without_continuation() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_prompt_override: "CUSTOM PROMPT".to_string(),
            gpt_reasoning_continuation: true,
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [
                { "type": "message", "role": "user", "content": "recent request" },
                { "type": "message", "role": "user", "content":
                    "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary." }
            ]
        });
        let (request_payload, forwarded, options) =
            prepare_downstream_response_create_payload_with_settings(payload, &settings);
        assert_eq!(
            forwarded["input"][1]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("CUSTOM PROMPT")
        );
        assert!(!options.unwrap().enabled);

        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings(&request_payload, None, options, &settings)
            .expect("关闭补回仍需校验摘要");
        assert!(coordinator.state.lock().unwrap().active.is_some());

        let response = Message::Text(
            r#"{"type":"response.completed","response":{"id":"resp-summary","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"<summary>SUMMARY</summary>"}]}]}}"#
                .into(),
        );
        let action = coordinator
            .handle_upstream_message(response.clone())
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("校验通过后应封装摘要");
        };
        let sse = super::websocket_messages_to_responses_sse(&messages);
        assert!(sse.contains("SUMMARY"));
        assert!(!sse.contains("<summary>"));
        assert!(!sse.contains("codex-elves-compaction-v3:"));
    }

    #[test]
    fn websocket_legacy_compaction_uses_custom_prompt() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_prompt_override: "CUSTOM LEGACY PROMPT".to_string(),
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [{
                "type": "message",
                "role": "user",
                "content": "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
            }]
        });
        let (_, forwarded_payload, options) =
            prepare_downstream_response_create_payload_with_settings(payload, &settings);

        assert_eq!(
            forwarded_payload["input"][0]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("CUSTOM LEGACY PROMPT")
        );
        assert!(options.is_some());
    }

    #[test]
    fn websocket_non_gpt_remote_compaction_uses_request_config_snapshot() {
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings(
                &json!({
                    "type": "response.create",
                    "model": "claude-sonnet-5",
                    "input": [{ "type": "compaction_trigger" }]
                }),
                Some("log-snapshot".to_string()),
                Some(crate::protocol_proxy::LayeredCompactionOptions {
                    enabled: true,
                    retain_tokens: 12_345,
                }),
                &BackendSettings {
                    layered_compaction_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let state = coordinator.state.lock().unwrap();
        let active = state
            .active
            .as_ref()
            .expect("V2 bridge mode should be active");
        assert!(matches!(
            active.mode,
            ActiveWebSocketMode::ValidatedCompaction {
                kind: crate::layered_compaction::CompactionKind::RemoteV2,
                layered_enabled: true,
                retain_tokens: 12_345
            }
        ));
    }

    #[test]
    fn websocket_bridged_remote_prompt_only_returns_plain_summary() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_prompt_override: "CUSTOM V2 PROMPT".to_string(),
            ..Default::default()
        };
        let payload = json!({
            "type": "response.create",
            "model": "claude-sonnet-5",
            "input": [
                { "type": "message", "role": "user", "content": "recent request" },
                { "type": "message", "role": "assistant", "content": "recent answer" },
                { "type": "compaction_trigger" }
            ]
        });
        let (request_payload, forwarded, options) =
            prepare_downstream_response_create_payload_with_settings(payload, &settings);
        assert_eq!(
            forwarded["input"][2]["content"][0]["text"],
            crate::layered_compaction::compaction_instruction("CUSTOM V2 PROMPT")
        );
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings(&request_payload, None, options, &settings)
            .unwrap();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp-plain-summary",
                        "status": "completed",
                        "model": "claude-sonnet-5",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{ "type": "output_text", "text": "<summary>PLAIN SUMMARY</summary>" }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, metadata } = action else {
            panic!("本地桥接 V2 应产生 compaction 响应");
        };
        let done = messages
            .iter()
            .filter_map(|message| match message {
                Message::Text(text) => serde_json::from_str::<Value>(text.as_str()).ok(),
                _ => None,
            })
            .find(|event| event["type"] == "response.output_item.done")
            .expect("应产生一个终结的 compaction item");
        assert_eq!(done["item"]["type"], "compaction");
        assert!(
            done["item"]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("codex-elves-compaction-v2:")
        );
        assert!(!done.to_string().contains("recent answer"));
        assert!(!metadata.layered_compaction_triggered);
    }

    #[test]
    fn websocket_non_gpt_summary_response_becomes_compaction_events() {
        let coordinator = WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    mode: ActiveWebSocketMode::ValidatedCompaction {
                        kind: crate::layered_compaction::CompactionKind::RemoteV2,
                        layered_enabled: true,
                        retain_tokens: crate::layered_compaction::DEFAULT_RETAIN_TOKENS,
                    },
                    original_request: json!({
                        "type": "response.create",
                        "model": "claude-sonnet-5",
                        "input": [
                            {
                                "type": "message",
                                "role": "user",
                                "content": [{ "type": "input_text", "text": "keep this context" }]
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [{ "type": "output_text", "text": "assistant reply to keep" }]
                            },
                            { "type": "compaction_trigger" }
                        ]
                    }),
                    log_id: Some("log-compaction".to_string()),
                    max_rounds: 0,
                    round: 0,
                    completed_rounds: 0,
                    accumulated_reasoning_tokens: None,
                    buffered_messages: Vec::new(),
                    fallback_messages: Vec::new(),
                    fallback_response_body: None,
                    continue_requests: Vec::new(),
                    before_response_body: None,
                    compaction_plan: None,
                    capacity_retry_attempts: 0,
                }),
                ..Default::default()
            })),
        };
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_ws_compact",
                        "object": "response",
                        "status": "completed",
                        "model": "claude-sonnet-5",
                        "output": [{
                            "id": "msg_ws_compact",
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "<summary>WEBSOCKET COMPACTED SUMMARY</summary>"
                            }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("remote compaction terminal response should flush rewritten messages");
        };
        let payloads = messages
            .iter()
            .filter_map(|message| {
                let Message::Text(text) = message else {
                    return None;
                };
                serde_json::from_str::<serde_json::Value>(text.as_str()).ok()
            })
            .collect::<Vec<_>>();
        let done = payloads
            .iter()
            .filter(|payload| {
                payload.get("type").and_then(serde_json::Value::as_str)
                    == Some("response.output_item.done")
            })
            .collect::<Vec<_>>();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0]["item"]["type"], "compaction");
        let expanded =
            crate::layered_compaction::expand_synthetic_local_compaction_request(&json!({
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": "older user" }]
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": "keep this context" }]
                    },
                    done[0]["item"].clone()
                ]
            }));
        let restored = expanded.to_string();
        assert!(restored.contains("WEBSOCKET COMPACTED SUMMARY"));
        assert!(restored.contains("assistant reply to keep"));
        assert_eq!(restored.matches("keep this context").count(), 1);
        assert!(
            !payloads
                .iter()
                .any(|payload| payload["item"]["type"] == "message")
        );
    }

    fn websocket_legacy_compaction_coordinator() -> WebSocketContinuationCoordinator {
        WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    mode: ActiveWebSocketMode::ValidatedCompaction {
                        kind: crate::layered_compaction::CompactionKind::Legacy,
                        layered_enabled: true,
                        retain_tokens: crate::layered_compaction::DEFAULT_RETAIN_TOKENS,
                    },
                    original_request: json!({
                        "type": "response.create",
                        "model": "gpt-5.6",
                        "input": [
                            {
                                "type": "message",
                                "role": "user",
                                "content": [{
                                    "type": "input_text",
                                    "text": "KEEP THIS LEGACY CONTEXT"
                                }]
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [{
                                    "type": "output_text",
                                    "text": "KEEP THIS LEGACY ASSISTANT REPLY"
                                }]
                            },
                            {
                                "type": "message",
                                "role": "user",
                                "content": [{
                                    "type": "input_text",
                                    "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
                                }]
                            }
                        ]
                    }),
                    log_id: Some("log-legacy-compaction".to_string()),
                    max_rounds: 0,
                    round: 0,
                    completed_rounds: 0,
                    accumulated_reasoning_tokens: None,
                    buffered_messages: Vec::new(),
                    fallback_messages: Vec::new(),
                    fallback_response_body: None,
                    continue_requests: Vec::new(),
                    before_response_body: None,
                    compaction_plan: None,
                    capacity_retry_attempts: 0,
                }),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn websocket_legacy_compaction_registers_layered_mode() {
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request_with_settings(
                &json!({
                    "type": "response.create",
                    "model": "gpt-5.6",
                    "input": [{
                        "type": "message",
                        "role": "user",
                        "content": "You are performing a CONTEXT CHECKPOINT COMPACTION."
                    }]
                }),
                Some("log-legacy-snapshot".to_string()),
                Some(crate::protocol_proxy::LayeredCompactionOptions {
                    enabled: true,
                    retain_tokens: 24_000,
                }),
                &BackendSettings {
                    layered_compaction_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let state = coordinator.state.lock().unwrap();
        let active = state
            .active
            .as_ref()
            .expect("legacy layered mode should be active");
        assert!(matches!(
            active.mode,
            ActiveWebSocketMode::ValidatedCompaction {
                kind: crate::layered_compaction::CompactionKind::Legacy,
                layered_enabled: true,
                retain_tokens: 24_000
            }
        ));
    }

    #[test]
    fn websocket_legacy_compaction_completed_response_stores_structured_tail() {
        let coordinator = websocket_legacy_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_ws_legacy",
                        "object": "response",
                        "status": "completed",
                        "model": "gpt-5.6",
                        "output": [{
                            "id": "msg_ws_legacy",
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "<summary>LEGACY LLM SUMMARY</summary>"
                            }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, metadata } = action else {
            panic!("legacy compaction terminal response should flush rewritten messages");
        };
        let payloads = messages
            .iter()
            .filter_map(|message| {
                let Message::Text(text) = message else {
                    return None;
                };
                serde_json::from_str::<serde_json::Value>(text.as_str()).ok()
            })
            .collect::<Vec<_>>();
        let done = payloads
            .iter()
            .find(|payload| {
                payload.get("type").and_then(serde_json::Value::as_str)
                    == Some("response.output_item.done")
            })
            .expect("rewritten legacy response should contain a done message item");
        let text = done["item"]["content"][0]["text"]
            .as_str()
            .expect("rewritten message should contain text");
        // Codex 接收 legacy 摘要后添加固定包装，再作为 user 历史发送。
        let wrapped = format!(
            "{}\n\n{text}",
            crate::layered_compaction::LEGACY_COMPACTION_SUMMARY_PREFIX
        );
        let restored =
            crate::layered_compaction::expand_synthetic_local_compaction_request(&json!({
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": wrapped }]
                }]
            }));
        let restored_input = restored["input"]
            .as_array()
            .expect("v3 payload should restore Responses items");

        assert_eq!(done["item"]["type"], "message");
        assert!(text.contains("LEGACY LLM SUMMARY"));
        assert_eq!(restored_input.len(), 3);
        assert_eq!(restored_input[0]["role"], "assistant");
        assert_eq!(restored_input[1]["role"], "user");
        assert_eq!(
            restored_input[1]["content"][0]["text"],
            "KEEP THIS LEGACY CONTEXT"
        );
        assert_eq!(restored_input[2]["role"], "assistant");
        assert_eq!(
            restored_input[2]["content"][0]["text"],
            "KEEP THIS LEGACY ASSISTANT REPLY"
        );
        assert!(metadata.layered_compaction_triggered);
        assert_eq!(
            metadata.layered_compaction_retain_tokens,
            Some(crate::layered_compaction::DEFAULT_RETAIN_TOKENS)
        );
        // 最近一轮 = [user, assistant]，共 2 条。
        assert_eq!(metadata.layered_compaction_retained_items, Some(2));
        assert!(
            metadata
                .layered_compaction_before_response_body
                .as_deref()
                .is_some_and(|body| body.contains("LEGACY LLM SUMMARY"))
        );
    }

    fn websocket_remote_compaction_coordinator() -> WebSocketContinuationCoordinator {
        WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    mode: ActiveWebSocketMode::ValidatedCompaction {
                        kind: crate::layered_compaction::CompactionKind::RemoteV2,
                        layered_enabled: false,
                        retain_tokens: crate::layered_compaction::DEFAULT_RETAIN_TOKENS,
                    },
                    original_request: json!({
                        "type": "response.create",
                        "model": "claude-sonnet-5",
                        "input": [{ "type": "compaction_trigger" }]
                    }),
                    log_id: Some("log-compaction-failure".to_string()),
                    max_rounds: 0,
                    round: 0,
                    completed_rounds: 0,
                    accumulated_reasoning_tokens: None,
                    buffered_messages: Vec::new(),
                    fallback_messages: Vec::new(),
                    fallback_response_body: None,
                    continue_requests: Vec::new(),
                    before_response_body: None,
                    compaction_plan: None,
                    capacity_retry_attempts: 0,
                }),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn websocket_remote_compaction_transport_failure_emits_failed_before_close() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .fail_active_compaction("websocket_read_failed", "upstream read failed")
            .unwrap()
            .expect("active V2 bridge should be failed closed");
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("transport failure should flush a V2 failure");
        };
        let payload_text = messages
            .iter()
            .filter_map(|message| match message {
                Message::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(payload_text.contains("\"type\":\"response.failed\""));
        assert!(payload_text.contains("compaction_websocket_read_failed"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
    }

    #[test]
    fn websocket_remote_compaction_eof_emits_failed_before_close() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .fail_active_compaction("websocket_ended", "upstream ended without a close frame")
            .unwrap()
            .expect("active V2 bridge should be failed closed on EOF");
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("EOF should flush a V2 failure");
        };
        let payload_text = messages
            .iter()
            .filter_map(|message| match message {
                Message::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(payload_text.contains("\"type\":\"response.failed\""));
        assert!(payload_text.contains("compaction_websocket_ended"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
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
                    mode: ActiveWebSocketMode::ContinueThinking,
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
                    compaction_plan: None,
                    capacity_retry_attempts: 0,
                }),
                ..Default::default()
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
