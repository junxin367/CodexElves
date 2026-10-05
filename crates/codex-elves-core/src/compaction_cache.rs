//! Prompt-cache lease inference used only to choose the local compaction model.
//!
//! Providers do not expose an absolute cache expiry. We therefore keep a
//! bounded, in-memory lease derived from trustworthy usage evidence. Restarting
//! the helper intentionally resets all leases to unknown.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::request_headers::RequestContext;
use crate::settings::RelayProfile;

const FIVE_MINUTES: Duration = Duration::from_secs(5 * 60);
const ONE_HOUR: Duration = Duration::from_secs(60 * 60);
const MAX_CACHE_LEASES: usize = 256;
const MAX_PENDING_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
static NEXT_OBSERVATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheProtocol {
    Responses,
    ChatCompletions,
    Anthropic,
}

impl CacheProtocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::Responses => "responses",
            Self::ChatCompletions => "chatCompletions",
            Self::Anthropic => "anthropic",
        }
    }

    fn refreshes_on_hit(self) -> bool {
        self == Self::Anthropic
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CacheTtl {
    FiveMinutes,
    OneHour,
}

impl CacheTtl {
    fn duration(self) -> Duration {
        match self {
            Self::FiveMinutes => FIVE_MINUTES,
            Self::OneHour => ONE_HOUR,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheLeaseKey {
    relay_id: String,
    endpoint: String,
    credential_sha256: String,
    protocol: String,
    model: String,
    session_identity: String,
    window_identity: String,
    prefix_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheLeaseState {
    Valid,
    Expired,
    Missing,
    IdentityUnavailable,
}

impl CacheLeaseState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Expired => "expired",
            Self::Missing => "missing",
            Self::IdentityUnavailable => "identity_unavailable",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CacheLeaseDecision {
    pub(crate) state: CacheLeaseState,
    pub(crate) ttl: Option<&'static str>,
    pub(crate) remaining_ms: Option<u64>,
    pub(crate) identity_sha256: Option<String>,
}

impl CacheLeaseDecision {
    pub(crate) fn is_valid(&self) -> bool {
        self.state == CacheLeaseState::Valid
    }

    fn identity_unavailable() -> Self {
        Self {
            state: CacheLeaseState::IdentityUnavailable,
            ttl: None,
            remaining_ms: None,
            identity_sha256: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct UsageEvidence {
    cached_tokens: u64,
    cache_creation_tokens: u64,
    cache_creation_5m_tokens: u64,
    cache_creation_1h_tokens: u64,
    explicit_ttl: Option<CacheTtl>,
}

impl UsageEvidence {
    fn merge_usage(&mut self, usage: &Value) {
        self.cached_tokens = self.cached_tokens.max(
            usage
                .pointer("/input_tokens_details/cached_tokens")
                .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
                .or_else(|| usage.get("prompt_cache_hit_tokens"))
                .or_else(|| usage.get("cachedContentTokenCount"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        self.cache_creation_tokens = self.cache_creation_tokens.max(
            usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        self.cache_creation_5m_tokens = self.cache_creation_5m_tokens.max(
            usage
                .pointer("/cache_creation/ephemeral_5m_input_tokens")
                .or_else(|| usage.get("cache_creation_5m_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        self.cache_creation_1h_tokens = self.cache_creation_1h_tokens.max(
            usage
                .pointer("/cache_creation/ephemeral_1h_input_tokens")
                .or_else(|| usage.get("cache_creation_1h_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        if let Some(ttl) = usage.get("cache_ttl").and_then(Value::as_str) {
            self.explicit_ttl = match ttl.trim().to_ascii_lowercase().as_str() {
                "1h" => Some(CacheTtl::OneHour),
                "5m" | "mixed" => Some(CacheTtl::FiveMinutes),
                _ => self.explicit_ttl,
            };
        }
    }

    fn creation_ttl(
        self,
        protocol: CacheProtocol,
        request_ttl_hint: Option<CacheTtl>,
    ) -> Option<CacheTtl> {
        if self.cache_creation_5m_tokens > 0 {
            return Some(CacheTtl::FiveMinutes);
        }
        if self.cache_creation_1h_tokens > 0 {
            return Some(CacheTtl::OneHour);
        }
        if self.cache_creation_tokens == 0 {
            return None;
        }
        self.explicit_ttl
            .or(request_ttl_hint)
            .or_else(|| (protocol == CacheProtocol::Anthropic).then_some(CacheTtl::FiveMinutes))
    }
}

#[derive(Clone)]
struct CacheLease {
    ttl: CacheTtl,
    expires_at: Instant,
    observation: u64,
    refresh_on_hit: bool,
}

#[derive(Default)]
struct CacheLeaseStore {
    leases: HashMap<CacheLeaseKey, CacheLease>,
    order: VecDeque<(CacheLeaseKey, u64)>,
}

impl CacheLeaseStore {
    fn decision(&mut self, key: &CacheLeaseKey, now: Instant) -> CacheLeaseDecision {
        let identity_sha256 = Some(cache_key_sha256(key));
        let Some(lease) = self.leases.get(key).cloned() else {
            return CacheLeaseDecision {
                state: CacheLeaseState::Missing,
                ttl: None,
                remaining_ms: None,
                identity_sha256,
            };
        };
        if now >= lease.expires_at {
            self.leases.remove(key);
            self.order.retain(|(candidate, _)| candidate != key);
            return CacheLeaseDecision {
                state: CacheLeaseState::Expired,
                ttl: Some(lease.ttl.label()),
                remaining_ms: Some(0),
                identity_sha256,
            };
        }
        CacheLeaseDecision {
            state: CacheLeaseState::Valid,
            ttl: Some(lease.ttl.label()),
            remaining_ms: Some(
                lease
                    .expires_at
                    .saturating_duration_since(now)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            ),
            identity_sha256,
        }
    }

    fn observe(
        &mut self,
        key: CacheLeaseKey,
        protocol: CacheProtocol,
        request_ttl_hint: Option<CacheTtl>,
        evidence: UsageEvidence,
        started_at: Instant,
        observation: u64,
    ) -> Option<CacheTtl> {
        if let Some(ttl) = evidence.creation_ttl(protocol, request_ttl_hint) {
            if self
                .leases
                .get(&key)
                .is_some_and(|lease| lease.observation > observation)
            {
                return None;
            }
            self.insert(
                key,
                CacheLease {
                    ttl,
                    expires_at: started_at + ttl.duration(),
                    observation,
                    refresh_on_hit: protocol.refreshes_on_hit(),
                },
            );
            return Some(ttl);
        }
        if evidence.cached_tokens == 0 {
            return None;
        }
        let Some(existing) = self.leases.get(&key).cloned() else {
            return None;
        };
        if !existing.refresh_on_hit || existing.observation > observation {
            return None;
        }
        let ttl = existing.ttl;
        self.insert(
            key,
            CacheLease {
                ttl,
                expires_at: started_at + ttl.duration(),
                observation,
                refresh_on_hit: true,
            },
        );
        Some(ttl)
    }

    fn insert(&mut self, key: CacheLeaseKey, lease: CacheLease) {
        // 同一缓存键刷新租约时替换旧索引，队列大小随有效租约数量保持有界。
        self.order.retain(|(candidate, _)| candidate != &key);
        self.order.push_back((key.clone(), lease.observation));
        self.leases.insert(key, lease);
        while self.leases.len() > MAX_CACHE_LEASES {
            let Some((candidate, observation)) = self.order.pop_front() else {
                break;
            };
            if self
                .leases
                .get(&candidate)
                .is_some_and(|lease| lease.observation == observation)
            {
                self.leases.remove(&candidate);
            }
        }
    }
}

fn leases() -> &'static Mutex<CacheLeaseStore> {
    static LEASES: OnceLock<Mutex<CacheLeaseStore>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(CacheLeaseStore::default()))
}

pub(crate) fn lease_decision(
    relay: &RelayProfile,
    protocol: CacheProtocol,
    endpoint: &str,
    source_request: &Value,
    wire_request: &Value,
    request_context: &RequestContext,
) -> CacheLeaseDecision {
    let Some(key) = cache_lease_key(
        relay,
        protocol,
        endpoint,
        source_request,
        wire_request,
        request_context,
    ) else {
        return CacheLeaseDecision::identity_unavailable();
    };
    leases()
        .lock()
        .map(|mut store| store.decision(&key, Instant::now()))
        .unwrap_or(CacheLeaseDecision {
            state: CacheLeaseState::Missing,
            ttl: None,
            remaining_ms: None,
            identity_sha256: Some(cache_key_sha256(&key)),
        })
}

pub(crate) struct CacheResponseObserver {
    key: CacheLeaseKey,
    protocol: CacheProtocol,
    request_ttl_hint: Option<CacheTtl>,
    started_at: Instant,
    observation: u64,
    is_stream: bool,
    terminal_success: bool,
    terminal_failure: bool,
    evidence: UsageEvidence,
    pending: Vec<u8>,
    sse_scan_from: usize,
}

impl CacheResponseObserver {
    pub(crate) fn new(
        relay: &RelayProfile,
        protocol: CacheProtocol,
        endpoint: &str,
        source_request: &Value,
        wire_request: &Value,
        request_context: &RequestContext,
        is_stream: bool,
        started_at: Instant,
    ) -> Option<Self> {
        if crate::layered_compaction::is_any_compaction_request(source_request) {
            return None;
        }
        Some(Self {
            key: cache_lease_key(
                relay,
                protocol,
                endpoint,
                source_request,
                wire_request,
                request_context,
            )?,
            protocol,
            request_ttl_hint: request_cache_ttl_hint(wire_request),
            started_at,
            observation: NEXT_OBSERVATION.fetch_add(1, Ordering::Relaxed),
            is_stream,
            terminal_success: false,
            terminal_failure: false,
            evidence: UsageEvidence::default(),
            pending: Vec::new(),
            sse_scan_from: 0,
        })
    }

    pub(crate) fn observe_bytes(&mut self, bytes: &[u8]) {
        if self.terminal_failure {
            return;
        }
        if self.pending.len().saturating_add(bytes.len()) > MAX_PENDING_RESPONSE_BYTES {
            // 只放弃缓存租约推断，不影响调用方继续转发响应；也不能把截断内容当成完整证据。
            self.terminal_failure = true;
            self.pending = Vec::new();
            self.sse_scan_from = 0;
            return;
        }
        if self.is_stream {
            self.pending.extend_from_slice(bytes);
            self.consume_sse_blocks(false);
        } else {
            self.pending.extend_from_slice(bytes);
        }
    }

    pub(crate) fn observe_json_event(&mut self, payload: &Value) {
        observe_usage_candidates(&mut self.evidence, payload);
        let event_type = payload.get("type").and_then(Value::as_str);
        self.terminal_failure |= response_has_upstream_error(payload);
        self.terminal_success |= matches!(
            event_type,
            Some("response.completed" | "response.incomplete" | "message_stop")
        ) || payload
            .pointer("/response/status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "completed" | "incomplete"));
    }

    pub(crate) fn finish(mut self) {
        if self.is_stream {
            self.consume_sse_blocks(true);
        } else if let Ok(payload) = serde_json::from_slice::<Value>(&self.pending) {
            observe_usage_candidates(&mut self.evidence, &payload);
            self.terminal_success = non_stream_response_succeeded(&payload);
        }
        if !self.terminal_success || self.terminal_failure {
            return;
        }
        let observed_ttl = leases().lock().ok().and_then(|mut store| {
            store.observe(
                self.key.clone(),
                self.protocol,
                self.request_ttl_hint,
                self.evidence,
                self.started_at,
                self.observation,
            )
        });
        if let Some(ttl) = observed_ttl {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.compaction_cache_lease_observed",
                serde_json::json!({
                    "identitySha256": cache_key_sha256(&self.key),
                    "protocol": self.protocol.as_str(),
                    "ttl": ttl.label(),
                    "cacheReadTokens": self.evidence.cached_tokens,
                    "cacheCreationTokens": self.evidence.cache_creation_tokens,
                    "cacheCreation5mTokens": self.evidence.cache_creation_5m_tokens,
                    "cacheCreation1hTokens": self.evidence.cache_creation_1h_tokens,
                }),
            );
        }
    }

    fn consume_sse_blocks(&mut self, finish: bool) {
        // 批量借用完整事件，最后只搬移一次未完成的尾部，避免逐事件复制和搬移。
        let mut pending = std::mem::take(&mut self.pending);
        let mut consumed = 0;
        let mut scan_from = self.sse_scan_from;
        while let Some((end, separator_len)) = sse_block_end(&pending[scan_from..]) {
            let end = scan_from + end;
            self.observe_sse_block(&pending[consumed..end]);
            consumed = end + separator_len;
            scan_from = consumed;
        }
        if finish {
            if consumed < pending.len() {
                self.observe_sse_block(&pending[consumed..]);
            }
            pending.clear();
        } else if consumed > 0 {
            pending.drain(..consumed);
        }
        // 最长分隔符为 CRLF CRLF；只重扫末尾三个字节以覆盖跨网络分片的分隔符。
        self.sse_scan_from = pending.len().saturating_sub(3);
        self.pending = pending;
    }

    fn observe_sse_block(&mut self, block: &[u8]) {
        let text = String::from_utf8_lossy(block);
        if text.lines().any(|line| {
            line.strip_prefix("event:")
                .is_some_and(|event| matches!(event.trim(), "error" | "response.failed"))
        }) {
            self.terminal_failure = true;
            return;
        }
        let mut data_lines = text
            .lines()
            .filter_map(|line| line.trim_end_matches('\r').strip_prefix("data:"))
            .map(str::trim_start);
        let Some(first) = data_lines.next() else {
            return;
        };
        let data = if let Some(second) = data_lines.next() {
            let mut data = String::with_capacity(first.len() + second.len() + 1);
            data.push_str(first);
            data.push('\n');
            data.push_str(second);
            for line in data_lines {
                data.push('\n');
                data.push_str(line);
            }
            std::borrow::Cow::Owned(data)
        } else {
            std::borrow::Cow::Borrowed(first)
        };
        if data.is_empty() {
            return;
        }
        if data == "[DONE]" {
            self.terminal_success = true;
            return;
        }
        if let Ok(payload) = serde_json::from_str::<Value>(&data) {
            self.observe_json_event(&payload);
            if payload
                .get("choices")
                .and_then(Value::as_array)
                .is_some_and(|choices| {
                    choices.iter().any(|choice| {
                        choice
                            .get("finish_reason")
                            .is_some_and(|reason| !reason.is_null())
                    })
                })
            {
                self.terminal_success = true;
            }
        }
    }
}

fn cache_lease_key(
    relay: &RelayProfile,
    protocol: CacheProtocol,
    endpoint: &str,
    source_request: &Value,
    wire_request: &Value,
    request_context: &RequestContext,
) -> Option<CacheLeaseKey> {
    let session_identity = first_non_empty_string(
        source_request,
        &[
            "/prompt_cache_key",
            "/metadata/prompt_cache_key",
            "/metadata/session_id",
            "/metadata/thread_id",
        ],
    )
    .or_else(|| request_context.thread_id().map(ToString::to_string))?;
    let model = wire_request
        .get("model")
        .or_else(|| source_request.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let window_identity = first_non_empty_string(
        source_request,
        &[
            "/client_metadata/window_id",
            "/metadata/window_id",
            "/metadata/turn_id",
        ],
    )
    .or_else(|| request_context.cache_window_identity())
    .unwrap_or_default();
    Some(CacheLeaseKey {
        relay_id: relay.id.trim().to_string(),
        endpoint: endpoint.trim().trim_end_matches('/').to_string(),
        credential_sha256: sha256_hex(relay.api_key.trim().as_bytes()),
        protocol: protocol.as_str().to_string(),
        model,
        session_identity,
        window_identity,
        prefix_sha256: prefix_sha256(wire_request),
    })
}

fn first_non_empty_string(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn prefix_sha256(request: &Value) -> String {
    const CACHE_SENSITIVE_FIELDS: [&str; 13] = [
        "instructions",
        "system",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "thinking",
        "reasoning",
        "output_config",
        "text",
        "temperature",
        "prompt_cache_retention",
        "prompt_cache_options",
        "service_tier",
    ];
    let mut prefix = Map::new();
    for field in CACHE_SENSITIVE_FIELDS {
        if let Some(value) = request.get(field) {
            prefix.insert(field.to_string(), value.clone());
        }
    }
    for field in ["messages", "input"] {
        let Some(items) = request.get(field).and_then(Value::as_array) else {
            continue;
        };
        let leading = items
            .iter()
            .take_while(|item| {
                matches!(
                    item.get("role").and_then(Value::as_str),
                    Some("system" | "developer")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if !leading.is_empty() {
            prefix.insert(format!("leading_{field}"), Value::Array(leading));
        }
    }
    sha256_hex(&serde_json::to_vec(&Value::Object(prefix)).unwrap_or_default())
}

fn request_cache_ttl_hint(request: &Value) -> Option<CacheTtl> {
    let mut saw_cache_control = false;
    let mut saw_one_hour = false;
    let mut saw_five_minutes = false;
    visit_cache_controls(
        request,
        &mut saw_cache_control,
        &mut saw_one_hour,
        &mut saw_five_minutes,
    );
    if saw_five_minutes || (saw_cache_control && !saw_one_hour) {
        Some(CacheTtl::FiveMinutes)
    } else if saw_one_hour {
        Some(CacheTtl::OneHour)
    } else {
        None
    }
}

fn visit_cache_controls(
    value: &Value,
    saw_cache_control: &mut bool,
    saw_one_hour: &mut bool,
    saw_five_minutes: &mut bool,
) {
    match value {
        Value::Array(items) => {
            for item in items {
                visit_cache_controls(item, saw_cache_control, saw_one_hour, saw_five_minutes);
            }
        }
        Value::Object(object) => {
            if let Some(cache_control) = object.get("cache_control").and_then(Value::as_object) {
                *saw_cache_control = true;
                match cache_control
                    .get("ttl")
                    .and_then(Value::as_str)
                    .map(str::trim)
                {
                    Some("1h") => *saw_one_hour = true,
                    _ => *saw_five_minutes = true,
                }
            }
            for child in object.values() {
                visit_cache_controls(child, saw_cache_control, saw_one_hour, saw_five_minutes);
            }
        }
        _ => {}
    }
}

fn observe_usage_candidates(evidence: &mut UsageEvidence, payload: &Value) {
    for usage in [
        payload.get("usage"),
        payload.pointer("/response/usage"),
        payload.pointer("/message/usage"),
        payload.pointer("/delta/usage"),
    ]
    .into_iter()
    .flatten()
    {
        evidence.merge_usage(usage);
    }
}

fn non_stream_response_succeeded(payload: &Value) -> bool {
    !response_has_upstream_error(payload)
}

fn response_has_upstream_error(payload: &Value) -> bool {
    matches!(
        payload.get("type").and_then(Value::as_str),
        Some("error" | "response.failed")
    ) || ["/error", "/response/error", "/message/error"]
        .into_iter()
        .any(|pointer| {
            payload
                .pointer(pointer)
                .is_some_and(|error| !error.is_null())
        })
        || [
            payload.get("status"),
            payload.pointer("/response/status"),
            payload.pointer("/message/status"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|status| status == "failed")
}

fn sse_block_end(bytes: &[u8]) -> Option<(usize, usize)> {
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        if index >= 1 && bytes[index - 1] == b'\n' {
            return Some((index - 1, 2));
        }
        if index >= 3 && &bytes[index - 3..=index] == b"\r\n\r\n" {
            return Some((index - 3, 4));
        }
    }
    None
}

fn cache_key_sha256(key: &CacheLeaseKey) -> String {
    sha256_hex(
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            key.relay_id,
            key.endpoint,
            key.credential_sha256,
            key.protocol,
            key.model,
            key.session_identity,
            key.window_identity,
            key.prefix_sha256
        )
        .as_bytes(),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(suffix: &str) -> CacheLeaseKey {
        CacheLeaseKey {
            relay_id: "relay".to_string(),
            endpoint: "https://example.test/v1/messages".to_string(),
            credential_sha256: "credential".to_string(),
            protocol: "anthropic".to_string(),
            model: "claude".to_string(),
            session_identity: "session".to_string(),
            window_identity: String::new(),
            prefix_sha256: suffix.to_string(),
        }
    }

    #[test]
    fn lease_boundaries_use_strict_expiry_and_mixed_uses_five_minutes() {
        let started_at = Instant::now();
        let mut store = CacheLeaseStore::default();
        let evidence = UsageEvidence {
            cache_creation_5m_tokens: 10,
            cache_creation_1h_tokens: 20,
            ..Default::default()
        };
        assert_eq!(
            store.observe(
                key("mixed"),
                CacheProtocol::Anthropic,
                None,
                evidence,
                started_at,
                1,
            ),
            Some(CacheTtl::FiveMinutes)
        );
        assert_eq!(
            store
                .decision(
                    &key("mixed"),
                    started_at + FIVE_MINUTES - Duration::from_millis(1)
                )
                .state,
            CacheLeaseState::Valid
        );
        assert_eq!(
            store
                .decision(&key("mixed"), started_at + FIVE_MINUTES)
                .state,
            CacheLeaseState::Expired
        );
        assert_eq!(
            store
                .decision(
                    &key("mixed"),
                    started_at + FIVE_MINUTES + Duration::from_millis(1)
                )
                .state,
            CacheLeaseState::Missing
        );
    }

    #[test]
    fn one_hour_creation_keeps_one_hour_lease() {
        let started_at = Instant::now();
        let mut store = CacheLeaseStore::default();
        let evidence = UsageEvidence {
            cache_creation_1h_tokens: 20,
            ..Default::default()
        };
        store.observe(
            key("one-hour"),
            CacheProtocol::Anthropic,
            None,
            evidence,
            started_at,
            1,
        );
        let decision = store.decision(
            &key("one-hour"),
            started_at + ONE_HOUR - Duration::from_millis(1),
        );
        assert_eq!(decision.state, CacheLeaseState::Valid);
        assert_eq!(decision.ttl, Some("1h"));
    }

    #[test]
    fn cached_hit_refreshes_only_an_existing_anthropic_lease() {
        let started_at = Instant::now();
        let mut store = CacheLeaseStore::default();
        let hit = UsageEvidence {
            cached_tokens: 10,
            ..Default::default()
        };
        assert_eq!(
            store.observe(
                key("missing"),
                CacheProtocol::Anthropic,
                None,
                hit,
                started_at,
                1,
            ),
            None
        );
        store.observe(
            key("refresh"),
            CacheProtocol::Anthropic,
            None,
            UsageEvidence {
                cache_creation_5m_tokens: 10,
                ..Default::default()
            },
            started_at,
            2,
        );
        let refreshed_at = started_at + Duration::from_secs(120);
        assert_eq!(
            store.observe(
                key("refresh"),
                CacheProtocol::Anthropic,
                None,
                hit,
                refreshed_at,
                3,
            ),
            Some(CacheTtl::FiveMinutes)
        );
        assert_eq!(
            store
                .decision(
                    &key("refresh"),
                    refreshed_at + FIVE_MINUTES - Duration::from_millis(1),
                )
                .state,
            CacheLeaseState::Valid
        );
    }

    #[test]
    fn older_response_cannot_overwrite_newer_lease() {
        let started_at = Instant::now();
        let mut store = CacheLeaseStore::default();
        store.observe(
            key("ordered"),
            CacheProtocol::Anthropic,
            None,
            UsageEvidence {
                cache_creation_1h_tokens: 1,
                ..Default::default()
            },
            started_at + Duration::from_secs(10),
            2,
        );
        assert_eq!(
            store.observe(
                key("ordered"),
                CacheProtocol::Anthropic,
                None,
                UsageEvidence {
                    cache_creation_5m_tokens: 1,
                    ..Default::default()
                },
                started_at,
                1,
            ),
            None
        );
        let decision = store.decision(&key("ordered"), started_at + Duration::from_secs(600));
        assert_eq!(decision.state, CacheLeaseState::Valid);
        assert_eq!(decision.ttl, Some("1h"));
    }

    #[test]
    fn lease_store_is_bounded_and_evicts_old_observations() {
        let started_at = Instant::now();
        let mut store = CacheLeaseStore::default();
        let repeated_key = key("repeated");
        for observation in 0..100_000 {
            store.observe(
                repeated_key.clone(),
                CacheProtocol::Anthropic,
                None,
                UsageEvidence {
                    cache_creation_5m_tokens: u64::from(observation == 0),
                    cached_tokens: u64::from(observation > 0),
                    ..Default::default()
                },
                started_at,
                observation,
            );
        }
        assert_eq!(store.leases.len(), 1);
        assert_eq!(store.order.len(), 1);
        assert_eq!(
            store
                .decision(&repeated_key, started_at + FIVE_MINUTES)
                .state,
            CacheLeaseState::Expired
        );
        assert!(store.order.is_empty());

        for observation in 0..(MAX_CACHE_LEASES as u64 + 32) {
            store.observe(
                key(&format!("bounded-{observation}")),
                CacheProtocol::Anthropic,
                None,
                UsageEvidence {
                    cache_creation_5m_tokens: 1,
                    ..Default::default()
                },
                started_at,
                observation,
            );
        }
        assert_eq!(store.leases.len(), MAX_CACHE_LEASES);
        assert_eq!(store.order.len(), MAX_CACHE_LEASES);
        assert_eq!(
            store.decision(&key("bounded-0"), started_at).state,
            CacheLeaseState::Missing
        );
        assert_eq!(
            store
                .decision(
                    &key(&format!("bounded-{}", MAX_CACHE_LEASES as u64 + 31)),
                    started_at
                )
                .state,
            CacheLeaseState::Valid
        );
    }

    #[test]
    fn cache_identity_changes_with_model_relay_and_prefix() {
        let context = RequestContext::default();
        let source = json!({
            "model": "claude-a",
            "prompt_cache_key": "session",
            "instructions": "system-a",
            "metadata": { "window_id": "window-a" }
        });
        let relay = RelayProfile {
            id: "relay-a".to_string(),
            api_key: "secret".to_string(),
            ..RelayProfile::default()
        };
        let first = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &source,
            &source,
            &context,
        )
        .unwrap();

        let mut other_relay = relay.clone();
        other_relay.id = "relay-b".to_string();
        let relay_key = cache_lease_key(
            &other_relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &source,
            &source,
            &context,
        )
        .unwrap();
        let mut other_credential = relay.clone();
        other_credential.api_key = "other-secret".to_string();
        let credential_key = cache_lease_key(
            &other_credential,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &source,
            &source,
            &context,
        )
        .unwrap();
        let endpoint_key = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://other.example.test/v1/messages",
            &source,
            &source,
            &context,
        )
        .unwrap();
        let protocol_key = cache_lease_key(
            &relay,
            CacheProtocol::Responses,
            "https://example.test/v1/messages",
            &source,
            &source,
            &context,
        )
        .unwrap();
        let mut other_model = source.clone();
        other_model["model"] = json!("claude-b");
        let model_key = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &other_model,
            &other_model,
            &context,
        )
        .unwrap();
        let mut other_session = source.clone();
        other_session["prompt_cache_key"] = json!("other-session");
        let session_key = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &other_session,
            &other_session,
            &context,
        )
        .unwrap();
        let mut other_window = source.clone();
        other_window["metadata"]["window_id"] = json!("window-b");
        let window_key = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &other_window,
            &other_window,
            &context,
        )
        .unwrap();
        let mut other_prefix = source.clone();
        other_prefix["instructions"] = json!("system-b");
        let prefix_key = cache_lease_key(
            &relay,
            CacheProtocol::Anthropic,
            "https://example.test/v1/messages",
            &other_prefix,
            &other_prefix,
            &context,
        )
        .unwrap();
        for candidate in [
            relay_key,
            credential_key,
            endpoint_key,
            protocol_key,
            model_key,
            session_key,
            window_key,
            prefix_key,
        ] {
            assert_ne!(first, candidate);
        }
    }

    #[test]
    fn cache_identity_includes_the_request_window_header() {
        let relay = RelayProfile {
            id: "relay-window".to_string(),
            api_key: "secret".to_string(),
            ..RelayProfile::default()
        };
        let source = json!({
            "model": "gpt-5.6",
            "prompt_cache_key": "window-session",
            "instructions": "stable"
        });
        let context = |window: &str| {
            RequestContext::from_http_request(
                format!(
                    "POST /v1/responses HTTP/1.1\r\nthread-id: thread\r\nx-codex-window-id: {window}\r\n\r\n"
                )
                .as_bytes(),
            )
        };
        let first = cache_lease_key(
            &relay,
            CacheProtocol::Responses,
            "https://example.test/v1/responses",
            &source,
            &source,
            &context("window-a"),
        )
        .unwrap();
        let second = cache_lease_key(
            &relay,
            CacheProtocol::Responses,
            "https://example.test/v1/responses",
            &source,
            &source,
            &context("window-b"),
        )
        .unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn failed_stream_and_compaction_requests_do_not_establish_leases() {
        let relay = RelayProfile {
            id: "relay-failed-stream".to_string(),
            api_key: "secret".to_string(),
            ..RelayProfile::default()
        };
        let context = RequestContext::default();
        let source = json!({
            "model": "gpt-5.6",
            "prompt_cache_key": "failed-stream-session",
            "instructions": "stable"
        });
        let endpoint = "https://example.test/v1/responses";
        let mut observer = CacheResponseObserver::new(
            &relay,
            CacheProtocol::Responses,
            endpoint,
            &source,
            &source,
            &context,
            true,
            Instant::now(),
        )
        .expect("ordinary request should be observed");
        observer.observe_bytes(
            b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"usage\":{\"cache_creation_input_tokens\":10,\"cache_ttl\":\"5m\"}}}\n\ndata: [DONE]\n\n",
        );
        observer.finish();
        assert_eq!(
            lease_decision(
                &relay,
                CacheProtocol::Responses,
                endpoint,
                &source,
                &source,
                &context,
            )
            .state,
            CacheLeaseState::Missing
        );

        let compaction = json!({
            "model": "gpt-5.6",
            "prompt_cache_key": "failed-stream-session",
            "input": [{ "type": "compaction_trigger" }]
        });
        assert!(
            CacheResponseObserver::new(
                &relay,
                CacheProtocol::Responses,
                endpoint,
                &compaction,
                &compaction,
                &context,
                true,
                Instant::now(),
            )
            .is_none()
        );
    }

    #[test]
    fn error_envelopes_do_not_establish_cache_leases() {
        let errors = [
            json!({"error": {"message": "overloaded"}}),
            json!({"type": "error"}),
            json!({"type": "response.failed"}),
            json!({"status": "failed"}),
            json!({"response": {"error": {"message": "overloaded"}}}),
            json!({"response": {"status": "failed"}}),
            json!({"message": {"error": {"message": "overloaded"}}}),
            json!({"message": {"status": "failed"}}),
        ];
        for (index, error) in errors.into_iter().enumerate() {
            for is_stream in [false, true] {
                let relay = RelayProfile::default();
                let context = RequestContext::default();
                let source = json!({
                    "model": "gpt-test",
                    "prompt_cache_key": format!("error-envelope-{index}-{is_stream}")
                });
                let endpoint = "https://example.test/v1/responses";
                let mut observer = CacheResponseObserver::new(
                    &relay,
                    CacheProtocol::Responses,
                    endpoint,
                    &source,
                    &source,
                    &context,
                    is_stream,
                    Instant::now(),
                )
                .unwrap();
                let mut payload = error.clone();
                payload["usage"] = json!({"cache_creation_input_tokens": 10, "cache_ttl": "5m"});
                let body = if is_stream {
                    format!("data: {payload}\n\ndata: [DONE]\n\n")
                } else {
                    payload.to_string()
                };
                observer.observe_bytes(body.as_bytes());
                observer.finish();
                assert_eq!(
                    lease_decision(
                        &relay,
                        CacheProtocol::Responses,
                        endpoint,
                        &source,
                        &source,
                        &context,
                    )
                    .state,
                    CacheLeaseState::Missing,
                    "error envelope {index}, stream={is_stream}"
                );
            }
        }
    }

    #[test]
    fn sse_error_event_without_json_type_invalidates_cache_evidence() {
        for event in ["error", "response.failed"] {
            let relay = RelayProfile::default();
            let context = RequestContext::default();
            let source = json!({
                "model": "gpt-test",
                "prompt_cache_key": format!("error-event-{event}")
            });
            let endpoint = "https://example.test/v1/responses";
            let mut observer = CacheResponseObserver::new(
                &relay,
                CacheProtocol::Responses,
                endpoint,
                &source,
                &source,
                &context,
                true,
                Instant::now(),
            )
            .unwrap();
            let body = format!(
                "data: {{\"usage\":{{\"cache_creation_input_tokens\":10,\"cache_ttl\":\"5m\"}}}}\r\n\r\nevent: {event}\r\ndata: overloaded\r\n\r\ndata: [DONE]\r\n\r\n"
            );
            for chunk in body.as_bytes().chunks(7) {
                observer.observe_bytes(chunk);
            }
            observer.finish();
            assert_eq!(
                lease_decision(
                    &relay,
                    CacheProtocol::Responses,
                    endpoint,
                    &source,
                    &source,
                    &context,
                )
                .state,
                CacheLeaseState::Missing,
            );
        }
    }

    #[test]
    fn sse_delimiter_finds_the_first_lf_or_crlf_boundary() {
        for length in 0..=8 {
            for mut encoded in 0..3_usize.pow(length) {
                let bytes = (0..length)
                    .map(|_| {
                        let byte = [b'x', b'\r', b'\n'][encoded % 3];
                        encoded /= 3;
                        byte
                    })
                    .collect::<Vec<_>>();
                let expected = (0..bytes.len()).find_map(|index| {
                    let suffix = &bytes[index..];
                    if suffix.starts_with(b"\n\n") {
                        Some((index, 2))
                    } else if suffix.starts_with(b"\r\n\r\n") {
                        Some((index, 4))
                    } else {
                        None
                    }
                });
                assert_eq!(sse_block_end(&bytes), expected, "{bytes:?}");
            }
        }
    }

    #[test]
    fn observer_preserves_mixed_multiline_events_at_every_byte_boundary() {
        let relay = RelayProfile::default();
        let context = RequestContext::default();
        let source = json!({"model": "gpt-test", "prompt_cache_key": "mixed-multiline-boundaries"});
        let body = concat!(
            ": heartbeat\r\n\r\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好🙂\"}\n\n",
            "data: {\"type\":\"response.completed\",\r\n",
            "data: \"response\":{\"status\":\"completed\",\"usage\":{\"cache_creation_input_tokens\":53,\"cache_ttl\":\"5m\"}}}\r\n\r\n",
            "data: [DONE]"
        ).as_bytes();
        for split in 0..=body.len() {
            let mut observer = CacheResponseObserver::new(
                &relay,
                CacheProtocol::Responses,
                "https://example.test/v1/responses",
                &source,
                &source,
                &context,
                true,
                Instant::now(),
            )
            .unwrap();
            observer.observe_bytes(&body[..split]);
            observer.observe_bytes(&body[split..]);
            observer.consume_sse_blocks(true);
            assert!(observer.terminal_success, "split={split}");
            assert!(!observer.terminal_failure, "split={split}");
            assert_eq!(observer.evidence.cache_creation_tokens, 53, "split={split}");
            assert!(observer.pending.is_empty());
        }
    }

    #[test]
    fn observer_keeps_large_fragmented_event_until_its_delimiter_arrives() {
        let relay = RelayProfile::default();
        let context = RequestContext::default();
        let source = json!({"model": "gpt-test", "prompt_cache_key": "large-fragmented-frame"});
        let mut observer = CacheResponseObserver::new(
            &relay,
            CacheProtocol::Responses,
            "https://example.test/v1/responses",
            &source,
            &source,
            &context,
            true,
            Instant::now(),
        )
        .unwrap();
        let body = format!(
            "data: {}\r\n\r\n",
            json!({
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "output_text": "你".repeat(32_768),
                    "usage": {"cache_creation_input_tokens": 71, "cache_ttl": "5m"}
                }
            })
        );
        for chunk in body.as_bytes().chunks(17) {
            observer.observe_bytes(chunk);
        }
        assert!(observer.terminal_success);
        assert!(!observer.terminal_failure);
        assert_eq!(observer.evidence.cache_creation_tokens, 71);
        assert!(observer.pending.is_empty());
    }

    #[test]
    fn oversized_response_cannot_grow_observer_or_establish_a_lease() {
        for is_stream in [true, false] {
            let relay = RelayProfile::default();
            let context = RequestContext::default();
            let source = json!({
                "model": "gpt-test",
                "prompt_cache_key": format!("oversized-response-{is_stream}"),
            });
            let endpoint = "https://example.test/v1/responses";
            let mut observer = CacheResponseObserver::new(
                &relay,
                CacheProtocol::Responses,
                endpoint,
                &source,
                &source,
                &context,
                is_stream,
                Instant::now(),
            )
            .unwrap();
            observer.observe_bytes(b"data: {\"incomplete\":");
            observer.observe_bytes(&vec![b'x'; MAX_PENDING_RESPONSE_BYTES + 1]);
            assert!(
                observer.pending.len() <= MAX_PENDING_RESPONSE_BYTES,
                "observer exceeded its memory bound: {} bytes",
                observer.pending.len()
            );
            assert!(
                observer.pending.is_empty(),
                "oversized input must be discarded"
            );
            observer.observe_bytes(b"data: [DONE]\n\n");
            assert!(
                observer.pending.is_empty(),
                "disabled observer must not buffer more bytes"
            );
            observer.observe_json_event(&json!({
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": {"cache_creation_input_tokens": 10, "cache_ttl": "5m"}
                }
            }));
            observer.finish();
            assert_eq!(
                lease_decision(
                    &relay,
                    CacheProtocol::Responses,
                    endpoint,
                    &source,
                    &source,
                    &context
                )
                .state,
                CacheLeaseState::Missing,
            );
        }
    }

    #[test]
    fn complete_response_split_across_chunks_still_establishes_a_lease() {
        for is_stream in [false, true] {
            let relay = RelayProfile::default();
            let context = RequestContext::default();
            let source = json!({
                "model": "gpt-test",
                "prompt_cache_key": format!("chunked-complete-response-{is_stream}"),
            });
            let endpoint = "https://example.test/v1/responses";
            let mut observer = CacheResponseObserver::new(
                &relay,
                CacheProtocol::Responses,
                endpoint,
                &source,
                &source,
                &context,
                is_stream,
                Instant::now(),
            )
            .unwrap();
            let response = json!({
                "status": "completed",
                "error": null,
                "output_text": "The words error and response.failed are ordinary output.",
                "usage": {"cache_creation_input_tokens": 10, "cache_ttl": "5m"}
            });
            let body = if is_stream {
                format!(
                    "data: {}\n\n",
                    json!({"type": "response.completed", "response": response})
                )
            } else {
                response.to_string()
            };
            for chunk in body.as_bytes().chunks(7) {
                observer.observe_bytes(chunk);
            }
            observer.finish();
            assert_eq!(
                lease_decision(
                    &relay,
                    CacheProtocol::Responses,
                    endpoint,
                    &source,
                    &source,
                    &context
                )
                .state,
                CacheLeaseState::Valid,
            );
        }
    }

    #[test]
    fn explicit_mixed_usage_is_conservative_five_minutes() {
        let mut evidence = UsageEvidence::default();
        evidence.merge_usage(&json!({
            "cache_creation_input_tokens": 30,
            "cache_ttl": "mixed"
        }));
        assert_eq!(
            evidence.creation_ttl(CacheProtocol::Responses, None),
            Some(CacheTtl::FiveMinutes)
        );
    }
}
