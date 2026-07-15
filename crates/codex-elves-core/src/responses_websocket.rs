use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::settings::{
    BackendSettings, RelayMode, RelayProfile, RelayProtocol, ResponsesWebsocketCapability,
    ResponsesWebsocketCapabilityState,
};
use anyhow::Context;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::Error as WebsocketError;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, USER_AGENT};
use tokio_tungstenite::tungstenite::http::{HeaderValue, Request, StatusCode};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
pub type UpstreamResponsesWebsocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 返回供应商原生 Responses WebSocket 的规范化端点。
pub fn responses_websocket_url(base_url: &str) -> Option<String> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return None;
    }

    let skip_version_prefix = base_url.ends_with('#');
    let base_url = base_url.trim_end_matches('#').trim();
    let mut base_url = reqwest::Url::parse(base_url).ok()?;
    base_url.set_query(None);
    base_url.set_fragment(None);

    let path = base_url.path().trim_end_matches('/').to_string();
    base_url.set_path(if path.is_empty() { "/" } else { &path });
    let base_url = base_url.as_str().trim_end_matches('/');
    let base_url = if skip_version_prefix {
        format!("{base_url}#")
    } else {
        base_url.to_string()
    };

    let mut endpoint =
        reqwest::Url::parse(&crate::protocol_proxy::responses_url(&base_url)).ok()?;
    match endpoint.scheme() {
        "http" => endpoint.set_scheme("ws").ok()?,
        "https" => endpoint.set_scheme("wss").ok()?,
        "ws" | "wss" => {}
        _ => return None,
    }
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    Some(endpoint.into())
}

/// 当上游 Base URL 变更时，使已缓存的能力结果失效。
pub fn normalize_responses_websocket_capability(profile: &mut RelayProfile) {
    let endpoint = responses_websocket_url(relay_responses_base_url(profile));
    let cached_endpoint = responses_websocket_url(&profile.responses_websocket.endpoint);

    match endpoint {
        Some(endpoint) if cached_endpoint.as_deref() == Some(endpoint.as_str()) => {
            profile.responses_websocket.endpoint = endpoint;
            if profile.responses_websocket.state == ResponsesWebsocketCapabilityState::Unknown {
                profile.responses_websocket.checked_at_ms = None;
            }
        }
        Some(endpoint) => {
            profile.responses_websocket = unknown_capability(endpoint);
        }
        None => {
            profile.responses_websocket = ResponsesWebsocketCapability::default();
        }
    }
}

/// 对供应商原生 Responses WebSocket 端点执行一次真实握手探测。
///
/// 端点匹配且已有明确缓存时直接返回缓存，不会发起网络请求。
pub async fn probe_responses_websocket(profile: &RelayProfile) -> ResponsesWebsocketCapability {
    let mut profile = profile.clone();
    normalize_responses_websocket_capability(&mut profile);
    if profile.responses_websocket.state != ResponsesWebsocketCapabilityState::Unknown {
        return profile.responses_websocket;
    }

    let endpoint = profile.responses_websocket.endpoint.clone();
    if endpoint.is_empty() {
        return unknown_probe_result(endpoint, "Responses WebSocket 端点无效。");
    }

    let request = match responses_websocket_request(&profile, None) {
        Ok(request) => request,
        Err(_) => {
            return unknown_probe_result(endpoint, "Responses WebSocket 请求或鉴权配置无效。");
        }
    };

    match tokio::time::timeout(PROBE_TIMEOUT, tokio_tungstenite::connect_async(request)).await {
        Err(_) => unknown_probe_result(endpoint, "Responses WebSocket 探测超时。"),
        Ok(Ok((socket, response))) => {
            drop(socket);
            classify_probe_http_status(endpoint, response.status())
        }
        Ok(Err(WebsocketError::Http(response))) => {
            classify_probe_http_status(endpoint, response.status())
        }
        Ok(Err(error)) => unknown_probe_result(endpoint, websocket_error_message(&error)),
    }
}

pub async fn open_responses_websocket_upstream(
    profile: &RelayProfile,
    original_user_agent: Option<&str>,
) -> anyhow::Result<UpstreamResponsesWebsocket> {
    let request = responses_websocket_request(profile, original_user_agent)?;
    let result = tokio::time::timeout(PROBE_TIMEOUT, tokio_tungstenite::connect_async(request))
        .await
        .context("Responses WebSocket 上游连接超时")?;
    let (socket, response) = result.map_err(|error| {
        anyhow::anyhow!(
            "{}",
            match error {
                WebsocketError::Http(ref response) => {
                    format!(
                        "Responses WebSocket 上游返回 HTTP {}",
                        response.status().as_u16()
                    )
                }
                _ => websocket_error_message(&error).to_string(),
            }
        )
    })?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        anyhow::bail!(
            "Responses WebSocket 上游返回 HTTP {}",
            response.status().as_u16()
        );
    }
    Ok(socket)
}

/// 仅在当前目标供应商适用且尚无明确缓存时执行探测，并把结果写回设置。
pub async fn probe_active_relay_responses_websocket_if_needed(settings: &mut BackendSettings) {
    let Some(profile_index) = settings
        .relay_profiles
        .iter()
        .position(|profile| profile.id == settings.active_relay_id)
    else {
        return;
    };

    normalize_responses_websocket_capability(&mut settings.relay_profiles[profile_index]);
    if !relay_can_probe_native_responses_websocket(&settings.relay_profiles[profile_index]) {
        return;
    }

    let result = probe_responses_websocket(&settings.relay_profiles[profile_index]).await;
    settings.relay_profiles[profile_index].responses_websocket = result;
}

/// 供应商端点探测成功且至少存在一个原生 Responses 模型时启用 WebSocket。
pub fn relay_supports_native_responses_websocket(profile: &RelayProfile) -> bool {
    if !relay_can_probe_native_responses_websocket(profile)
        || profile.responses_websocket.state != ResponsesWebsocketCapabilityState::Supported
    {
        return false;
    }

    let Some(endpoint) = responses_websocket_url(relay_responses_base_url(profile)) else {
        return false;
    };
    responses_websocket_url(&profile.responses_websocket.endpoint).as_deref()
        == Some(endpoint.as_str())
}

/// 供应商能力探测成功且用户没有显式关闭时，实际启用 Responses WebSocket。
pub fn relay_prefers_native_responses_websocket(profile: &RelayProfile) -> bool {
    profile.responses_websocket_enabled.unwrap_or(true)
        && relay_supports_native_responses_websocket(profile)
}

pub fn relay_websocket_enabled_for_settings(
    _settings: &BackendSettings,
    profile: &RelayProfile,
) -> bool {
    relay_prefers_native_responses_websocket(profile)
}

pub fn relay_can_probe_native_responses_websocket(profile: &RelayProfile) -> bool {
    profile.relay_mode != RelayMode::Aggregate
        && (profile.relay_mode != RelayMode::Official || profile.official_mix_api_key)
        && relay_has_native_responses_model(profile)
        && profile.system_prompt_override.trim().is_empty()
}

fn relay_has_native_responses_model(profile: &RelayProfile) -> bool {
    if !profile.model_mappings.is_empty() {
        let mut has_mapping = false;
        for mapping in &profile.model_mappings {
            if mapping.request_model.trim().is_empty() {
                continue;
            }
            has_mapping = true;
            if mapping.protocol == RelayProtocol::Responses {
                return true;
            }
        }
        if has_mapping {
            return false;
        }
    }
    has_models(&profile.responses_model_list) || profile.protocol == RelayProtocol::Responses
}

pub fn relay_responses_base_url(profile: &RelayProfile) -> &str {
    if profile.upstream_base_url.trim().is_empty() {
        profile.base_url.as_str()
    } else {
        profile.upstream_base_url.as_str()
    }
}

fn responses_websocket_request(
    profile: &RelayProfile,
    original_user_agent: Option<&str>,
) -> anyhow::Result<Request<()>> {
    let endpoint = responses_websocket_url(relay_responses_base_url(profile))
        .ok_or_else(|| anyhow::anyhow!("Responses WebSocket 端点无效"))?;
    if profile.api_key.trim().is_empty() {
        anyhow::bail!("Responses WebSocket API Key 为空");
    }
    let mut request = endpoint
        .as_str()
        .into_client_request()
        .context("Responses WebSocket 请求无效")?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", profile.api_key.trim()))
        .context("Responses WebSocket 鉴权配置无效")?;
    let user_agent = HeaderValue::from_str(&effective_user_agent(
        &profile.user_agent,
        original_user_agent,
    ))
    .context("Responses WebSocket User-Agent 无效")?;
    request.headers_mut().insert(AUTHORIZATION, authorization);
    request.headers_mut().insert(USER_AGENT, user_agent);
    Ok(request)
}

fn unknown_capability(endpoint: String) -> ResponsesWebsocketCapability {
    ResponsesWebsocketCapability {
        endpoint,
        ..ResponsesWebsocketCapability::default()
    }
}

fn classify_probe_http_status(
    endpoint: String,
    status: StatusCode,
) -> ResponsesWebsocketCapability {
    if status == StatusCode::SWITCHING_PROTOCOLS {
        return explicit_probe_result(
            endpoint,
            ResponsesWebsocketCapabilityState::Supported,
            "Responses WebSocket 握手成功。",
        );
    }

    if (status.is_success() && status != StatusCode::SWITCHING_PROTOCOLS)
        || matches!(status.as_u16(), 404 | 405 | 410 | 422 | 426 | 501)
    {
        return explicit_probe_result(
            endpoint,
            ResponsesWebsocketCapabilityState::Unsupported,
            &format!(
                "端点返回 HTTP {}，不支持 Responses WebSocket。",
                status.as_u16()
            ),
        );
    }

    unknown_probe_result(
        endpoint,
        &format!("端点返回 HTTP {}，可能是鉴权或临时故障。", status.as_u16()),
    )
}

fn explicit_probe_result(
    endpoint: String,
    state: ResponsesWebsocketCapabilityState,
    message: &str,
) -> ResponsesWebsocketCapability {
    ResponsesWebsocketCapability {
        state,
        endpoint,
        checked_at_ms: Some(current_timestamp_ms()),
        message: message.to_string(),
    }
}

fn unknown_probe_result(endpoint: String, message: &str) -> ResponsesWebsocketCapability {
    ResponsesWebsocketCapability {
        endpoint,
        message: message.to_string(),
        ..ResponsesWebsocketCapability::default()
    }
}

fn websocket_error_message(error: &WebsocketError) -> &'static str {
    match error {
        WebsocketError::Tls(_) => "Responses WebSocket TLS 连接失败。",
        WebsocketError::Io(_) => "Responses WebSocket 连接失败。",
        WebsocketError::Url(_) | WebsocketError::HttpFormat(_) => "Responses WebSocket 请求无效。",
        _ => "Responses WebSocket 握手失败。",
    }
}

fn effective_user_agent(configured_user_agent: &str, original_user_agent: Option<&str>) -> String {
    let configured_user_agent = configured_user_agent.trim();
    if !configured_user_agent.is_empty() {
        return configured_user_agent.to_string();
    }
    original_user_agent
        .map(str::trim)
        .filter(|user_agent| !user_agent.is_empty())
        .unwrap_or(concat!("CodexElves/", env!("CARGO_PKG_VERSION")))
        .to_string()
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn has_models(model_list: &str) -> bool {
    model_list
        .split(['\r', '\n', ','])
        .any(|model| !model.trim().is_empty())
}
