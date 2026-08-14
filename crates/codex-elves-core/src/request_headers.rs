//! Local proxy request-header context and route-scoped forwarding policy.
//!
//! The local helper must retain enough Codex request context to talk to a
//! native Responses upstream, without becoming a transparent HTTP proxy.

use std::collections::HashSet;

use tokio_tungstenite::tungstenite::http::header::USER_AGENT;
use tokio_tungstenite::tungstenite::http::{HeaderMap, HeaderName, HeaderValue};

/// The actual protocol selected for an outbound request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpstreamHeaderRoute {
    NativeResponsesHttp,
    NativeResponsesWebSocket,
    ConvertedProtocol,
}

/// Validated inbound request headers.
///
/// This type intentionally does not implement `Debug` or serialization so
/// opaque routing values and client-provided headers cannot be logged by
/// accident.
#[derive(Clone, Default)]
pub struct RequestContext {
    headers: HeaderMap,
}

impl RequestContext {
    pub fn from_headers(headers: HeaderMap) -> Self {
        Self { headers }
    }

    pub fn from_user_agent(user_agent: Option<&str>) -> Self {
        let mut headers = HeaderMap::new();
        if let Some(user_agent) = user_agent.map(str::trim).filter(|value| !value.is_empty())
            && let Ok(value) = HeaderValue::from_str(user_agent)
        {
            headers.insert(USER_AGENT, value);
        }
        Self { headers }
    }

    /// Parse the request head without accepting malformed values for forwarding.
    ///
    /// The helper still handles its request using the established parser. This
    /// parser only decides whether a header is eligible for the isolated
    /// forwarding context.
    pub fn from_http_request(request: &[u8]) -> Self {
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return Self::default();
        };
        let Ok(head) = std::str::from_utf8(&request[..header_end]) else {
            return Self::default();
        };

        let mut headers = HeaderMap::new();
        for line in head.split("\r\n").skip(1) {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let Ok(name) = HeaderName::from_bytes(name.trim().as_bytes()) else {
                continue;
            };
            let Ok(value) = HeaderValue::from_str(value.trim()) else {
                continue;
            };
            headers.append(name, value);
        }
        Self { headers }
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Build a fresh outbound map after the final upstream protocol is known.
    ///
    /// Converted protocols deliberately receive no Codex/OpenAI request
    /// semantics. Their protocol-specific authentication, framing, and
    /// content negotiation are rebuilt by the caller.
    pub fn headers_for(&self, route: UpstreamHeaderRoute) -> HeaderMap {
        if route == UpstreamHeaderRoute::ConvertedProtocol {
            return HeaderMap::new();
        }

        let connection_options = connection_options(&self.headers);
        let mut names = HashSet::new();
        names.extend(self.headers.keys().cloned());

        let mut forwarded = HeaderMap::new();
        for name in names {
            if !is_native_responses_semantic_header(&name)
                || connection_options.contains(name.as_str())
                || is_proxy_owned_or_sensitive_header(&name)
            {
                continue;
            }
            for value in self.headers.get_all(&name) {
                forwarded.append(name.clone(), value.clone());
            }
        }
        forwarded
    }
}

fn connection_options(headers: &HeaderMap) -> HashSet<String> {
    let mut options = HashSet::new();
    for header_name in ["connection", "proxy-connection"] {
        for value in headers.get_all(header_name) {
            let Ok(value) = value.to_str() else {
                continue;
            };
            for option in value.split(',') {
                let option = option.trim();
                if !option.is_empty() {
                    options.insert(option.to_ascii_lowercase());
                }
            }
        }
    }
    options
}

fn is_native_responses_semantic_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    name.starts_with("x-codex-")
        || matches!(
            name,
            "openai-beta"
                | "session-id"
                | "session_id"
                | "thread-id"
                | "thread_id"
                | "x-client-request-id"
                | "originator"
                | "traceparent"
                | "tracestate"
                | "baggage"
        )
}

fn is_proxy_owned_or_sensitive_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    matches!(
        name,
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "content-length"
            | "content-type"
            | "accept"
            | "cache-control"
            | "expect"
            | "content-encoding"
            | "accept-encoding"
            | "cookie"
            | "origin"
            | "referer"
            | "forwarded"
            | "via"
            | "x-forwarded-for"
            | "x-forwarded-host"
            | "x-forwarded-proto"
            | "x-codex-api-key"
            | "x-codex-authorization"
            | "x-codex-signature"
            | "x-codex-integrity"
    ) || name.starts_with("sec-websocket-")
        || name.starts_with("sec-fetch-")
        || name.contains("signature")
        || name.contains("integrity")
}
