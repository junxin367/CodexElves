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

/// Responses HTTP operations have different upstream endpoints and response schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ResponsesRequestOperation {
    #[default]
    Create,
    Compact,
}

/// Validated inbound request headers.
///
/// This type intentionally does not implement `Debug` or serialization so
/// opaque routing values and client-provided headers cannot be logged by
/// accident.
#[derive(Clone, Default)]
pub struct RequestContext {
    headers: HeaderMap,
    responses_operation: ResponsesRequestOperation,
}

impl RequestContext {
    pub fn from_headers(headers: HeaderMap) -> Self {
        Self {
            headers,
            ..Self::default()
        }
    }

    pub fn from_user_agent(user_agent: Option<&str>) -> Self {
        let mut headers = HeaderMap::new();
        if let Some(user_agent) = user_agent.map(str::trim).filter(|value| !value.is_empty())
            && let Ok(value) = HeaderValue::from_str(user_agent)
        {
            headers.insert(USER_AGENT, value);
        }
        Self::from_headers(headers)
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
        Self::from_headers(headers)
    }

    /// Set the operation from the validated local HTTP route, never from a forwarded header.
    pub fn with_responses_path(mut self, path: &str) -> Self {
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        self.responses_operation = if matches!(
            path,
            "/responses/compact"
                | "/v1/responses/compact"
                | "/v1/v1/responses/compact"
                | "/codex/v1/responses/compact"
        ) {
            ResponsesRequestOperation::Compact
        } else {
            ResponsesRequestOperation::Create
        };
        self
    }

    pub fn responses_operation(&self) -> ResponsesRequestOperation {
        self.responses_operation
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn thread_id(&self) -> Option<&str> {
        self.headers
            .get("thread-id")
            .or_else(|| self.headers.get("x-client-request-id"))
            .or_else(|| self.headers.get("session-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn log_thread_id(&self) -> Option<&str> {
        self.headers
            .get("thread-id")
            .or_else(|| self.headers.get("session-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn turn_id(&self) -> Option<String> {
        let metadata = self
            .headers
            .get("x-codex-turn-metadata")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
        metadata
            .get("turn_id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    pub(crate) fn is_primary_agent_request(&self, request: &serde_json::Value) -> bool {
        // 每个请求的元数据优先于连接头，WebSocket 连接头可能属于更早的请求。
        let metadata =
            if let Some(value) = request.pointer("/client_metadata/x-codex-turn-metadata") {
                if request
                    .pointer("/client_metadata/x-openai-subagent")
                    .is_some()
                {
                    return false;
                }
                value.as_str()
            } else {
                if self.headers.contains_key("x-openai-subagent") {
                    return false;
                }
                self.headers
                    .get("x-codex-turn-metadata")
                    .and_then(|value| value.to_str().ok())
            };
        let Some(metadata) =
            metadata.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        else {
            return false;
        };
        metadata
            .get("agent_name")
            .and_then(serde_json::Value::as_str)
            == Some("/root")
            && metadata
                .get("thread_source")
                .and_then(serde_json::Value::as_str)
                == Some("user")
            && ["subagent_kind", "parent_thread_id"]
                .iter()
                .all(|key| metadata.get(key).is_none_or(serde_json::Value::is_null))
    }

    pub(crate) fn cache_window_identity(&self) -> Option<String> {
        self.headers
            .get("x-codex-window-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                let metadata = self
                    .headers
                    .get("x-codex-turn-metadata")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
                ["window_id", "turn_id"].into_iter().find_map(|field| {
                    metadata
                        .get(field)
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                })
            })
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

#[cfg(test)]
mod tests {
    use super::RequestContext;
    use serde_json::{Value, json};

    fn context(metadata: &str) -> RequestContext {
        RequestContext::from_http_request(
            format!("POST /v1/responses HTTP/1.1\r\nx-codex-turn-metadata: {metadata}\r\n\r\n")
                .as_bytes(),
        )
    }

    fn request(metadata: Value) -> Value {
        json!({"client_metadata": {"x-codex-turn-metadata": metadata.to_string()}})
    }

    #[test]
    fn primary_agent_requires_explicit_root_user_metadata() {
        let root = json!({"agent_name": "/root", "thread_source": "user"});
        assert!(context(&root.to_string()).is_primary_agent_request(&json!({})));
        assert!(RequestContext::default().is_primary_agent_request(&request(root)));
        for metadata in [
            json!({}),
            json!({"agent_name": "/root"}),
            json!({"thread_source": "user"}),
            json!({"agent_name": "/root/worker", "thread_source": "user"}),
            json!({"agent_name": "/root/worker/reviewer", "thread_source": "user"}),
            json!({"agent_name": "/root", "thread_source": "subagent"}),
            json!({"agent_name": "/root", "thread_source": "user", "subagent_kind": "thread_spawn"}),
            json!({"agent_name": "/root", "thread_source": "user", "parent_thread_id": "parent"}),
        ] {
            assert!(
                !RequestContext::default().is_primary_agent_request(&request(metadata.clone())),
                "{metadata}"
            );
        }
        assert!(!RequestContext::default().is_primary_agent_request(&json!({
            "input": "agent_name=/root, thread_source=user"
        })));
    }

    #[test]
    fn primary_agent_uses_per_request_metadata_instead_of_stale_connection_headers() {
        let root = json!({"agent_name": "/root", "thread_source": "user"});
        let child = json!({"agent_name": "/root/worker", "thread_source": "user"});
        assert!(!context(&root.to_string()).is_primary_agent_request(&request(child.clone())));
        assert!(context(&child.to_string()).is_primary_agent_request(&request(root)));
    }

    #[test]
    fn primary_agent_does_not_fall_back_from_invalid_request_metadata() {
        let root = context(r#"{"agent_name":"/root","thread_source":"user"}"#);
        for metadata in [json!("{"), Value::Null, json!(42), json!({})] {
            assert!(!root.is_primary_agent_request(&json!({
                "client_metadata": {"x-codex-turn-metadata": metadata}
            })));
        }
        assert!(!context("{").is_primary_agent_request(&json!({})));
    }

    #[test]
    fn primary_agent_excludes_legacy_subagent_markers_but_allows_user_forks() {
        let root = json!({"agent_name": "/root", "thread_source": "user"});
        let context = RequestContext::from_http_request(
            format!(
                "POST /v1/responses HTTP/1.1\r\nx-openai-subagent: collab_spawn\r\nx-codex-turn-metadata: {root}\r\n\r\n"
            )
            .as_bytes(),
        );
        assert!(!context.is_primary_agent_request(&json!({})));
        let mut child = request(root);
        child["client_metadata"]["x-openai-subagent"] = json!("collab_spawn");
        assert!(!RequestContext::default().is_primary_agent_request(&child));
        assert!(
            RequestContext::default().is_primary_agent_request(&request(json!({
                "agent_name": "/root",
                "thread_source": "user",
                "forked_from_thread_id": "another-user-thread"
            })))
        );
    }
}
