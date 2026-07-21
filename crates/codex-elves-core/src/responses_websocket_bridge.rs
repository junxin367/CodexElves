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
            let (message, request_payload, layered_compaction_options) =
                if let Some(payload) = payload {
                    let (request_payload, forwarded_payload, layered_compaction_options) =
                        prepare_downstream_response_create_payload(&payload)?;
                    let message = if forwarded_payload == payload {
                        message
                    } else {
                        Message::Text(
                            serde_json::to_string(&forwarded_payload)
                                .context("序列化处理后的 Responses WebSocket 请求失败")?
                                .into(),
                        )
                    };
                    (message, Some(request_payload), layered_compaction_options)
                } else {
                    (message, None, None)
                };
            if let (Some(request_payload), Message::Text(text)) =
                (request_payload.as_ref(), &message)
            {
                let log_id =
                    downstream_request_logger.record_request(request_payload, text.as_str());
                if let Err(error) = downstream_continuation.register_request(
                    request_payload,
                    log_id,
                    layered_compaction_options,
                ) {
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
            let message = match message {
                Ok(message) => message,
                Err(error) => {
                    let error_message = format!("upstream WebSocket read failed: {error}");
                    let mut compaction_failed_closed = false;
                    if let Some(WebSocketContinuationAction::Flush { messages, metadata }) =
                        upstream_continuation
                            .fail_active_compaction("websocket_read_failed", &error_message)?
                    {
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
                        compaction_failed_closed = true;
                    }
                    if compaction_failed_closed {
                        // 返回 Err 会让 try_join! 立即取消 downstream_writer，刚入队的
                        // response.failed 可能来不及发出；压缩请求已规范终结后按正常关闭处理。
                        received_close = true;
                        break;
                    }
                    return Err(error).context("读取上游 Responses WebSocket 消息失败");
                }
            };
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
                    let closes_connection = messages
                        .iter()
                        .any(|message| matches!(message, Message::Close(_)));
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
                    if closes_connection {
                        received_close = true;
                        break;
                    }
                }
            };
            if is_close {
                received_close = true;
                break;
            }
        }
        if !received_close {
            if let Some(WebSocketContinuationAction::Flush { messages, metadata }) =
                upstream_continuation.fail_active_compaction(
                    "websocket_ended",
                    "upstream WebSocket ended before a terminal response.",
                )?
            {
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
            } else {
                let _ = to_downstream_tx.send(Message::Close(None)).await;
            }
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
}

#[derive(Clone, Copy)]
enum ActiveWebSocketMode {
    ContinueThinking,
    LegacyLayeredCompaction {
        retain_tokens: u32,
    },
    RemoteCompactionV2 {
        layered_enabled: bool,
        retain_tokens: u32,
    },
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
    fn register_request(
        &self,
        payload: &Value,
        log_id: Option<String>,
        layered_compaction_options: Option<crate::protocol_proxy::LayeredCompactionOptions>,
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
        if crate::layered_compaction::is_remote_compaction_v2_request(Some(payload))
            && !crate::layered_compaction::model_supports_native_remote_compaction_v2(model)
        {
            let options = layered_compaction_options
                .context("Responses WebSocket V2 降级请求缺少上下文压缩配置快照")?;
            state.active = Some(ActiveWebSocketContinuation {
                mode: ActiveWebSocketMode::RemoteCompactionV2 {
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
            });
            return Ok(());
        }
        if crate::layered_compaction::is_compaction_request(Some(payload)) {
            let options = layered_compaction_options
                .filter(|options| options.enabled)
                .context("Responses WebSocket 传统压缩请求缺少已启用的上下文压缩配置快照")?;
            state.active = Some(ActiveWebSocketContinuation {
                mode: ActiveWebSocketMode::LegacyLayeredCompaction {
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
            });
            return Ok(());
        }
        let settings = SettingsStore::default()
            .load()
            .context("读取自动推理续接设置失败")?;
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
        if let ActiveWebSocketMode::RemoteCompactionV2 {
            layered_enabled,
            retain_tokens,
        } = mode
        {
            return handle_remote_compaction_v2_websocket_message(
                &mut state,
                message,
                layered_enabled,
                retain_tokens,
            );
        }
        if let ActiveWebSocketMode::LegacyLayeredCompaction { retain_tokens } = mode {
            return handle_legacy_layered_compaction_websocket_message(
                &mut state,
                message,
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
            Some(ActiveWebSocketMode::LegacyLayeredCompaction { .. }) => {
                ("layered_compaction", "Layered compaction")
            }
            Some(ActiveWebSocketMode::RemoteCompactionV2 { .. }) => {
                ("remote_compaction", "Remote Compaction V2")
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

fn handle_remote_compaction_v2_websocket_message(
    state: &mut WebSocketContinuationState,
    message: Message,
    layered_enabled: bool,
    retain_tokens: u32,
) -> anyhow::Result<WebSocketContinuationAction> {
    if let Message::Close(close_frame) = message {
        let active = state
            .active
            .take()
            .expect("active remote compaction must exist");
        let failure_sse = crate::layered_compaction::remote_compaction_v2_failure_sse(
            &active.original_request,
            "remote_compaction_websocket_closed",
            "Remote Compaction V2 upstream WebSocket closed before a terminal response.",
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        messages.push(Message::Close(close_frame));
        return Ok(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        });
    }

    if matches!(message, Message::Binary(_)) {
        let active = state
            .active
            .take()
            .expect("active remote compaction must exist");
        let failure_sse = crate::layered_compaction::remote_compaction_v2_failure_sse(
            &active.original_request,
            "remote_compaction_websocket_binary_frame",
            "Remote Compaction V2 bridge received an unsupported binary WebSocket frame.",
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        messages.push(Message::Close(None));
        return Ok(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        });
    }

    let Message::Text(text) = &message else {
        return Ok(WebSocketContinuationAction::Forward(message));
    };
    let payload = match serde_json::from_str::<Value>(text.as_str()) {
        Ok(payload) => payload,
        Err(error) => {
            let active = state
                .active
                .take()
                .expect("active remote compaction must exist");
            let failure_sse = crate::layered_compaction::remote_compaction_v2_failure_sse(
                &active.original_request,
                "remote_compaction_websocket_event_parse_failed",
                &format!(
                    "Remote Compaction V2 bridge could not parse an upstream WebSocket event: {error}"
                ),
            );
            let mut messages = responses_sse_to_websocket_messages(&failure_sse);
            messages.push(Message::Close(None));
            return Ok(WebSocketContinuationAction::Flush {
                messages,
                metadata: WebSocketContinueMetadata {
                    log_id: active.log_id,
                    ..Default::default()
                },
            });
        }
    };
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active = state
        .active
        .as_mut()
        .expect("active remote compaction must exist");
    active.buffered_messages.push(message);
    if !is_terminal_websocket_response_event(event_type) {
        return Ok(WebSocketContinuationAction::Buffered);
    }

    let active = state
        .active
        .take()
        .expect("active remote compaction must exist");
    let source_sse = websocket_messages_to_responses_sse(&active.buffered_messages);
    let rewritten =
        crate::layered_compaction::rewrite_remote_compaction_v2_responses_sse_with_layered_compaction(
            &active.original_request,
            layered_enabled,
            retain_tokens,
            source_sse.clone(),
        )
        .expect("Remote Compaction V2 WebSocket request must produce a terminal bridge result");
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "remote_compaction_v2.websocket_bridge_response",
        serde_json::json!({
            "layeredCompactionTriggered": rewritten.triggered,
            "retainedItems": rewritten.retained_items,
            "retainedChars": rewritten.retained_chars,
            "retainTokens": retain_tokens
        }),
    );
    let bridge_failed =
        crate::continue_thinking::extract_terminal_response_object(&rewritten.sse_text)
            .and_then(|response| {
                response
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .as_deref()
            == Some("failed");
    let failed_response_id = bridge_failed
        .then(|| websocket_response_id(&payload))
        .flatten();
    if let Some(response_id) = failed_response_id.as_ref() {
        remember_discarded_websocket_response_id(state, response_id.clone());
    }
    let mut messages = responses_sse_to_websocket_messages(&rewritten.sse_text);
    if bridge_failed && failed_response_id.is_none() {
        messages.push(Message::Close(None));
    }
    Ok(WebSocketContinuationAction::Flush {
        messages,
        metadata: WebSocketContinueMetadata {
            log_id: active.log_id,
            layered_compaction_triggered: rewritten.triggered,
            layered_compaction_retain_tokens: rewritten.triggered.then_some(retain_tokens),
            layered_compaction_retained_items: rewritten
                .triggered
                .then_some(rewritten.retained_items),
            layered_compaction_retained_chars: rewritten
                .triggered
                .then_some(rewritten.retained_chars),
            layered_compaction_before_response_body: rewritten.triggered.then_some(source_sse),
            ..Default::default()
        },
    })
}

fn handle_legacy_layered_compaction_websocket_message(
    state: &mut WebSocketContinuationState,
    message: Message,
    retain_tokens: u32,
) -> anyhow::Result<WebSocketContinuationAction> {
    if let Message::Close(close_frame) = message {
        let active = state
            .active
            .take()
            .expect("active layered compaction must exist");
        let failure_sse = crate::layered_compaction::compaction_failure_sse(
            &active.original_request,
            None,
            "layered_compaction_websocket_closed",
            "Layered compaction upstream WebSocket closed before a terminal response.",
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        messages.push(Message::Close(close_frame));
        return Ok(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        });
    }

    if matches!(message, Message::Binary(_)) {
        let active = state
            .active
            .take()
            .expect("active layered compaction must exist");
        let failure_sse = crate::layered_compaction::compaction_failure_sse(
            &active.original_request,
            None,
            "layered_compaction_websocket_binary_frame",
            "Layered compaction received an unsupported binary WebSocket frame.",
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        messages.push(Message::Close(None));
        return Ok(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        });
    }

    let Message::Text(text) = &message else {
        return Ok(WebSocketContinuationAction::Forward(message));
    };
    let payload = match serde_json::from_str::<Value>(text.as_str()) {
        Ok(payload) => payload,
        Err(error) => {
            let active = state
                .active
                .take()
                .expect("active layered compaction must exist");
            let failure_sse = crate::layered_compaction::compaction_failure_sse(
                &active.original_request,
                None,
                "layered_compaction_websocket_event_parse_failed",
                &format!("Layered compaction could not parse an upstream WebSocket event: {error}"),
            );
            let mut messages = responses_sse_to_websocket_messages(&failure_sse);
            messages.push(Message::Close(None));
            return Ok(WebSocketContinuationAction::Flush {
                messages,
                metadata: WebSocketContinueMetadata {
                    log_id: active.log_id,
                    ..Default::default()
                },
            });
        }
    };
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active = state
        .active
        .as_mut()
        .expect("active layered compaction must exist");
    active.buffered_messages.push(message);
    if !is_terminal_websocket_response_event(event_type) {
        return Ok(WebSocketContinuationAction::Buffered);
    }

    let response_object = payload
        .get("response")
        .filter(|response| response.is_object());
    let response_id = websocket_response_id(&payload);
    let terminal_error = match event_type {
        "response.completed"
            if response_object
                .is_some_and(crate::layered_compaction::has_completed_compaction_summary) =>
        {
            None
        }
        "response.completed" => Some((
            "layered_compaction_summary_missing",
            "Layered compaction received no valid assistant summary text.",
        )),
        "response.incomplete" => Some((
            "layered_compaction_upstream_incomplete",
            "Layered compaction received an incomplete upstream response.",
        )),
        "response.failed" | "error" => Some((
            "layered_compaction_upstream_failed",
            "Layered compaction received a failed upstream response.",
        )),
        _ => Some((
            "layered_compaction_terminal_response_invalid",
            "Layered compaction received an invalid terminal upstream response.",
        )),
    };
    if let Some((code, message)) = terminal_error {
        let active = state
            .active
            .take()
            .expect("active layered compaction must exist");
        if let Some(response_id) = response_id.as_ref() {
            remember_discarded_websocket_response_id(state, response_id.clone());
        }
        let failure_sse = crate::layered_compaction::compaction_failure_sse(
            &active.original_request,
            response_object,
            code,
            message,
        );
        let mut messages = responses_sse_to_websocket_messages(&failure_sse);
        if response_id.is_none() {
            messages.push(Message::Close(None));
        }
        return Ok(WebSocketContinuationAction::Flush {
            messages,
            metadata: WebSocketContinueMetadata {
                log_id: active.log_id,
                ..Default::default()
            },
        });
    }

    let active = state
        .active
        .take()
        .expect("active layered compaction must exist");
    let source_sse = websocket_messages_to_responses_sse(&active.buffered_messages);
    let rewritten = crate::layered_compaction::apply_layered_compaction_to_responses_sse(
        &active.original_request,
        true,
        retain_tokens,
        source_sse.clone(),
    );
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "layered_compaction.websocket_response",
        serde_json::json!({
            "layeredCompactionTriggered": rewritten.triggered,
            "retainedItems": rewritten.retained_items,
            "retainedChars": rewritten.retained_chars,
            "retainTokens": retain_tokens
        }),
    );
    let messages = responses_sse_to_websocket_messages(&rewritten.sse_text);
    Ok(WebSocketContinuationAction::Flush {
        messages,
        metadata: WebSocketContinueMetadata {
            log_id: active.log_id,
            layered_compaction_triggered: rewritten.triggered,
            layered_compaction_retain_tokens: rewritten.triggered.then_some(retain_tokens),
            layered_compaction_retained_items: rewritten
                .triggered
                .then_some(rewritten.retained_items),
            layered_compaction_retained_chars: rewritten
                .triggered
                .then_some(rewritten.retained_chars),
            layered_compaction_before_response_body: rewritten.triggered.then_some(source_sse),
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
            remote_compaction_triggered: crate::proxy_log::request_uses_remote_compaction_v2(Some(
                payload,
            )),
            layered_compaction_triggered: false,
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

fn prepare_downstream_response_create_payload(
    payload: &Value,
) -> anyhow::Result<(
    Value,
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
)> {
    let normalized = crate::protocol_proxy::normalize_native_responses_request(payload);
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
    Ok(prepare_downstream_response_create_payload_with_settings(
        normalized, &settings,
    ))
}

fn prepare_downstream_response_create_payload_with_settings(
    normalized: Value,
    settings: &crate::settings::BackendSettings,
) -> (
    Value,
    Value,
    Option<crate::protocol_proxy::LayeredCompactionOptions>,
) {
    let model = normalized
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if crate::layered_compaction::is_remote_compaction_v2_request(Some(&normalized))
        && !crate::layered_compaction::model_supports_native_remote_compaction_v2(model)
    {
        let forwarded =
            crate::layered_compaction::prepare_remote_compaction_v2_bridge_request_with_prompt(
                &normalized,
                settings
                    .layered_compaction_enabled
                    .then_some(settings.layered_compaction_prompt_override.as_str()),
            );
        return (
            normalized,
            forwarded,
            Some(crate::protocol_proxy::LayeredCompactionOptions {
                enabled: settings.layered_compaction_enabled,
                retain_tokens: settings.layered_compaction_retain_tokens,
            }),
        );
    }
    if crate::layered_compaction::is_compaction_request(Some(&normalized))
        && settings.layered_compaction_enabled
    {
        let forwarded = crate::layered_compaction::prepare_legacy_layered_compaction_request(
            &normalized,
            &settings.layered_compaction_prompt_override,
        );
        return (
            normalized,
            forwarded,
            Some(crate::protocol_proxy::LayeredCompactionOptions {
                enabled: true,
                retain_tokens: settings.layered_compaction_retain_tokens,
            }),
        );
    }
    (normalized.clone(), normalized, None)
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
        ActiveWebSocketContinuation, ActiveWebSocketMode, WebSocketContinuationAction,
        WebSocketContinuationCoordinator, WebSocketContinuationState,
        is_responses_websocket_proxy_path, is_responses_websocket_upgrade,
        prepare_downstream_response_create_payload,
        prepare_downstream_response_create_payload_with_settings,
    };
    use crate::settings::BackendSettings;
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
                "content": [{ "type": "output_text", "text": "WEBSOCKET SUMMARY" }]
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

        assert_eq!(normalized_payload["input"][0]["type"], "message");
        assert_eq!(normalized_payload["input"][0]["role"], "assistant");
        assert!(
            normalized_payload["input"][0]["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("WEBSOCKET SUMMARY"))
        );
        assert_eq!(normalized_payload["input"][1]["type"], "compaction_trigger");
        assert_eq!(forwarded_payload["input"][1]["type"], "compaction_trigger");
        assert!(forwarded_payload.to_string().contains("WEBSOCKET SUMMARY"));
        assert!(
            !forwarded_payload
                .to_string()
                .contains("codex-elves-compaction-v1:")
        );
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
            prepare_downstream_response_create_payload(&payload).unwrap();
        assert_eq!(request_payload["input"][1]["type"], "compaction_trigger");
        assert_eq!(forwarded_payload["input"][1]["type"], "message");
        assert!(forwarded_payload.get("tools").is_none());
        assert!(options.is_some());
    }

    #[test]
    fn websocket_legacy_compaction_uses_project_default_prompt_and_removes_tools() {
        let settings = BackendSettings {
            layered_compaction_enabled: true,
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
            forwarded_payload["input"][1]["content"][0]["text"],
            crate::layered_compaction::DEFAULT_COMPACTION_PROMPT
        );
        assert!(forwarded_payload.get("tools").is_none());
        assert!(forwarded_payload.get("tool_choice").is_none());
        assert!(forwarded_payload.get("parallel_tool_calls").is_none());
        let options = options.expect("legacy compaction should capture settings");
        assert!(options.enabled);
        assert_eq!(options.retain_tokens, 23_456);
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
            forwarded_payload["input"][0]["content"],
            "CUSTOM LEGACY PROMPT"
        );
        assert!(options.is_some());
    }

    #[test]
    fn websocket_non_gpt_remote_compaction_uses_request_config_snapshot() {
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request(
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
            )
            .unwrap();
        let state = coordinator.state.lock().unwrap();
        let active = state
            .active
            .as_ref()
            .expect("V2 bridge mode should be active");
        assert!(matches!(
            active.mode,
            ActiveWebSocketMode::RemoteCompactionV2 {
                layered_enabled: true,
                retain_tokens: 12_345
            }
        ));
    }

    #[test]
    fn websocket_non_gpt_summary_response_becomes_compaction_events() {
        let coordinator = WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    mode: ActiveWebSocketMode::RemoteCompactionV2 {
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
                                "text": "WEBSOCKET COMPACTED SUMMARY"
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
        let restored =
            crate::layered_compaction::synthetic_remote_compaction_history_text(&done[0]["item"])
                .expect("websocket compaction should be decodable");
        assert!(restored.contains("WEBSOCKET COMPACTED SUMMARY"));
        // 最近一轮完整保留：user 消息 + 其后的 assistant 回复。
        assert!(restored.contains("assistant reply to keep"));
        assert!(restored.contains("keep this context"));
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
                    mode: ActiveWebSocketMode::LegacyLayeredCompaction {
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
                }),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn websocket_legacy_compaction_registers_layered_mode() {
        let coordinator = WebSocketContinuationCoordinator::default();
        coordinator
            .register_request(
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
            )
            .unwrap();
        let state = coordinator.state.lock().unwrap();
        let active = state
            .active
            .as_ref()
            .expect("legacy layered mode should be active");
        assert!(matches!(
            active.mode,
            ActiveWebSocketMode::LegacyLayeredCompaction {
                retain_tokens: 24_000
            }
        ));
    }

    #[test]
    fn websocket_legacy_compaction_completed_response_appends_recent_context() {
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
                                "text": "LEGACY LLM SUMMARY"
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

        assert_eq!(done["item"]["type"], "message");
        assert!(text.contains("LEGACY LLM SUMMARY"));
        // 最近一轮完整保留：user 消息 + 其后的 assistant 回复。
        assert!(text.contains("KEEP THIS LEGACY ASSISTANT REPLY"));
        assert!(text.contains("KEEP THIS LEGACY CONTEXT"));
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

    #[test]
    fn websocket_legacy_compaction_incomplete_response_fails_closed() {
        let coordinator = websocket_legacy_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.incomplete",
                    "response": {
                        "id": "resp_ws_legacy_incomplete",
                        "object": "response",
                        "status": "incomplete",
                        "model": "gpt-5.6",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "PARTIAL LEGACY SUMMARY MUST NOT LEAK"
                            }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("incomplete legacy response should flush a failure");
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
        assert!(payload_text.contains("layered_compaction_upstream_incomplete"));
        assert!(!payload_text.contains("PARTIAL LEGACY SUMMARY MUST NOT LEAK"));
        assert!(!payload_text.contains("\"type\":\"response.output_item.done\""));
    }

    #[test]
    fn websocket_legacy_compaction_close_discards_partial_output_and_fails() {
        let coordinator = websocket_legacy_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.output_text.delta",
                    "delta": "PARTIAL LEGACY OUTPUT MUST NOT LEAK"
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        assert!(matches!(action, WebSocketContinuationAction::Buffered));

        let action = coordinator
            .handle_upstream_message(Message::Close(None))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("early legacy close should flush a failure");
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
        assert!(payload_text.contains("layered_compaction_websocket_closed"));
        assert!(!payload_text.contains("PARTIAL LEGACY OUTPUT MUST NOT LEAK"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
    }

    fn websocket_remote_compaction_coordinator() -> WebSocketContinuationCoordinator {
        WebSocketContinuationCoordinator {
            state: Arc::new(Mutex::new(WebSocketContinuationState {
                active: Some(ActiveWebSocketContinuation {
                    mode: ActiveWebSocketMode::RemoteCompactionV2 {
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
                }),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn websocket_remote_compaction_incomplete_response_fails_closed() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.incomplete",
                    "response": {
                        "id": "resp_ws_incomplete",
                        "object": "response",
                        "status": "incomplete",
                        "model": "claude-sonnet-5",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "PARTIAL SUMMARY MUST NOT LEAK"
                            }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("incomplete V2 response should flush a failure");
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
        assert!(payload_text.contains("remote_compaction_upstream_incomplete"));
        assert!(!payload_text.contains("PARTIAL SUMMARY MUST NOT LEAK"));
        assert!(!payload_text.contains("\"type\":\"response.output_item.done\""));

        let late_completed = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_ws_incomplete",
                        "status": "completed",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": "LATE COMPLETED MUST NOT LEAK"
                            }]
                        }]
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        assert!(matches!(
            late_completed,
            WebSocketContinuationAction::Buffered
        ));
    }

    #[test]
    fn websocket_remote_compaction_failure_without_response_id_closes_connection() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "error",
                    "error": {
                        "message": "upstream failed without a response id"
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("unidentified V2 failure should flush failure and close");
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
        assert!(payload_text.contains("remote_compaction_upstream_failed"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
    }

    #[test]
    fn websocket_remote_compaction_close_discards_buffered_output_and_fails() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text(
                json!({
                    "type": "response.output_text.delta",
                    "delta": "PARTIAL OUTPUT MUST NOT LEAK"
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        assert!(matches!(action, WebSocketContinuationAction::Buffered));

        let action = coordinator
            .handle_upstream_message(Message::Close(None))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("early close should flush a V2 failure");
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
        assert!(payload_text.contains("remote_compaction_websocket_closed"));
        assert!(!payload_text.contains("PARTIAL OUTPUT MUST NOT LEAK"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
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
        assert!(payload_text.contains("remote_compaction_websocket_read_failed"));
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
        assert!(payload_text.contains("remote_compaction_websocket_ended"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
    }

    #[test]
    fn websocket_remote_compaction_malformed_event_fails_immediately() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Text("{malformed-json}".into()))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("malformed WebSocket event should flush a V2 failure");
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
        assert!(payload_text.contains("remote_compaction_websocket_event_parse_failed"));
        assert!(!payload_text.contains("\"type\":\"response.output_item.done\""));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
        assert!(coordinator.state.lock().unwrap().active.is_none());
    }

    #[test]
    fn websocket_remote_compaction_binary_frame_fails_and_closes() {
        let coordinator = websocket_remote_compaction_coordinator();
        let action = coordinator
            .handle_upstream_message(Message::Binary(vec![1, 2, 3].into()))
            .unwrap();
        let WebSocketContinuationAction::Flush { messages, .. } = action else {
            panic!("binary V2 frame should flush failure and close");
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
        assert!(payload_text.contains("remote_compaction_websocket_binary_frame"));
        assert!(matches!(messages.last(), Some(Message::Close(_))));
        assert!(coordinator.state.lock().unwrap().active.is_none());
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
