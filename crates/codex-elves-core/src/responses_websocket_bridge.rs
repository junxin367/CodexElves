use std::net::SocketAddr;
use std::time::Duration;

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
    let result = bridge_responses_websockets(downstream, upstream, &relay).await;
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
    let downstream_reader = async move {
        let mut received_close = false;
        while let Some(message) = downstream_stream.next().await {
            let message = message.context("读取本地 Responses WebSocket 消息失败")?;
            if let Err(error) = validate_downstream_message(&message, &relay) {
                let close = Message::Close(Some(CloseFrame {
                    code: CloseCode::Policy,
                    reason: error.to_string().into(),
                }));
                let _ = downstream_close_tx.send(close.clone()).await;
                let _ = upstream_close_tx.send(close).await;
                return Ok::<(), anyhow::Error>(());
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

    let upstream_reader = async move {
        let mut received_close = false;
        while let Some(message) = upstream_stream.next().await {
            let message = message.context("读取上游 Responses WebSocket 消息失败")?;
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

    tokio::try_join!(
        upstream_writer,
        downstream_writer,
        downstream_reader,
        upstream_reader
    )?;
    Ok(())
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
) -> anyhow::Result<()> {
    let Message::Text(text) = message else {
        return Ok(());
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
    Ok(())
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
