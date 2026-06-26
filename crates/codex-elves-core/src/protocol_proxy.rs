//! Codex Responses API 与 OpenAI Chat Completions 的本地协议转换。
//!
//! Codex Chat 与 Responses 协议之间的转换实现。

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};

use crate::relay_rotation::{RotationContext, RotationEvent};
use crate::settings::SettingsStore;

pub const DEFAULT_PROTOCOL_PROXY_PORT: u16 = 45221;
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const UPSTREAM_HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const UPSTREAM_STREAM_HEADER_TIMEOUT: Duration = Duration::from_secs(120);
const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";
const EXTRA_CHAT_PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];
const ERROR_BODY_PREVIEW_LIMIT: usize = 1024;
const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_DEFAULT_REASONING_EFFORT: &str = "high";
const REASONING_EFFORT_ORDER: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max"];
static PROTOCOL_PROXY_DIAGNOSTIC_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_protocol_proxy_diagnostic_id() -> String {
    let sequence = PROTOCOL_PROXY_DIAGNOSTIC_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ppx-{}-{sequence}", std::process::id())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatReasoningStyle {
    Default,
    DeepSeek,
    LowHigh,
    OpenRouter,
    Thinking,
    EnableThinking,
    ReasoningSplit,
}

#[derive(Debug, Clone, Default)]
struct CodexToolContext {
    custom_tools: BTreeMap<String, CodexCustomToolSpec>,
    function_tools: BTreeMap<String, CodexFunctionToolSpec>,
    web_search_fallback: Option<CodexWebSearchFallbackTool>,
    has_custom_tools: bool,
    has_namespace_tools: bool,
}

#[derive(Debug, Clone)]
struct CodexCustomToolSpec {
    openai_name: String,
    kind: CodexCustomToolKind,
    proxy_action: Option<CodexPatchProxyAction>,
}

#[derive(Debug, Clone, Default)]
struct CodexFunctionToolSpec {
    namespace: String,
    name: String,
}

#[derive(Debug, Clone)]
struct CodexWebSearchFallbackTool {
    namespace: String,
    name: String,
    query_parameter: String,
    score: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexCustomToolKind {
    Raw,
    ApplyPatch,
    BuiltIn,
}

impl Default for CodexCustomToolKind {
    fn default() -> Self {
        Self::Raw
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexPatchProxyAction {
    AddFile,
    DeleteFile,
    UpdateFile,
    ReplaceFile,
    Batch,
}

impl CodexPatchProxyAction {
    fn suffix(self) -> &'static str {
        match self {
            Self::AddFile => "add_file",
            Self::DeleteFile => "delete_file",
            Self::UpdateFile => "update_file",
            Self::ReplaceFile => "replace_file",
            Self::Batch => "batch",
        }
    }
}

impl CodexToolContext {
    fn is_custom_tool_proxy(&self, upstream_name: &str) -> bool {
        self.custom_tools.contains_key(upstream_name)
    }

    fn original_custom_tool_name(&self, upstream_name: &str) -> String {
        self.custom_tools
            .get(upstream_name)
            .map(|spec| spec.openai_name.clone())
            .unwrap_or_else(|| upstream_name.to_string())
    }

    fn openai_name_for_function_tool(&self, upstream_name: &str) -> (String, String) {
        let Some(spec) = self.function_tools.get(upstream_name) else {
            return (upstream_name.to_string(), String::new());
        };
        let name = if spec.name.is_empty() {
            upstream_name.to_string()
        } else {
            spec.name.clone()
        };
        (name, spec.namespace.clone())
    }

    fn web_search_fallback_tool(&self) -> Option<&CodexWebSearchFallbackTool> {
        self.web_search_fallback.as_ref()
    }
}

pub fn local_responses_proxy_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

pub fn local_models_proxy_response() -> anyhow::Result<Option<ProxyHttpResponse>> {
    let settings = SettingsStore::default().load().unwrap_or_default();
    let relay = settings.active_relay_profile();
    if !relay.local_proxy_enabled() {
        return Ok(None);
    }
    let models = crate::model_catalog::relay_profile_model_ids_for_proxy(&relay);
    if models.is_empty() {
        return Ok(None);
    }
    let data = models
        .into_iter()
        .map(|model| {
            json!({
                "id": model,
                "object": "model"
            })
        })
        .collect::<Vec<_>>();
    Ok(Some(ProxyHttpResponse {
        status: "200 OK".to_string(),
        content_type: "application/json; charset=utf-8".to_string(),
        body: serde_json::to_vec(&json!({
            "object": "list",
            "data": data
        }))?,
    }))
}

pub fn responses_to_chat_completions(body: Value) -> anyhow::Result<Value> {
    let mut result = json!({});

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let text = instruction_text(instructions);
        if !text.is_empty() {
            messages.push(json!({ "role": "system", "content": text }));
        }
    }

    if let Some(input) = body.get("input") {
        append_responses_input(input, &mut messages);
    }
    normalize_chat_messages(&mut messages);
    let messages = collapse_system_messages_to_head(messages);
    result["messages"] = json!(messages);

    let model = body.get("model").and_then(Value::as_str).unwrap_or("");
    if let Some(value) = body.get("max_output_tokens") {
        if is_openai_o_series(model) {
            result["max_completion_tokens"] = value.clone();
        } else {
            result["max_tokens"] = value.clone();
        }
    }
    if let Some(value) = body.get("max_tokens") {
        result["max_tokens"] = value.clone();
    }
    if let Some(value) = body.get("max_completion_tokens") {
        result["max_completion_tokens"] = value.clone();
    }

    for key in ["temperature", "top_p", "stream"] {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }
    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        let mut stream_options = body
            .get("stream_options")
            .cloned()
            .unwrap_or_else(|| json!({}));
        stream_options["include_usage"] = json!(true);
        result["stream_options"] = stream_options;
    }

    apply_chat_reasoning_options(&mut result, &body, model);

    let conversion_tools = tools_for_proxy_conversion(&body);
    let tool_context = build_codex_tool_context_from_tools(&conversion_tools);
    let mut has_chat_tools = false;
    if !conversion_tools.is_empty() {
        let converted = responses_tools_to_chat_tools(&conversion_tools, &tool_context);
        if !converted.is_empty() {
            has_chat_tools = true;
            result["tools"] = json!(converted);
        }
    }

    if has_chat_tools {
        if let Some(tool_choice) = body
            .get("tool_choice")
            .and_then(|value| responses_tool_choice_to_chat(value, &tool_context))
        {
            result["tool_choice"] = tool_choice;
        }
        if let Some(value) = body.get("parallel_tool_calls") {
            result["parallel_tool_calls"] = value.clone();
        }
    }

    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if *key == "stream_options" && result.get("stream_options").is_some() {
            continue;
        }
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }

    Ok(result)
}

pub fn chat_completion_to_response(body: Value) -> anyhow::Result<Value> {
    chat_completion_to_response_with_context(body, &CodexToolContext::default(), None)
}

pub fn chat_completion_to_response_with_request(
    body: Value,
    original_request: &Value,
) -> anyhow::Result<Value> {
    let context = build_codex_tool_context_for_request(original_request);
    chat_completion_to_response_with_context(body, &context, Some(original_request))
}

fn chat_completion_to_response_with_context(
    body: Value,
    tool_context: &CodexToolContext,
    original_request: Option<&Value>,
) -> anyhow::Result<Value> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("chat response missing choices"))?;
    let choice = choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("chat response choices is empty"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| anyhow::anyhow!("chat response choice missing message"))?;

    let response_id = response_id_from_chat_id(body.get("id").and_then(Value::as_str));
    let mut output = Vec::new();
    if let Some(reasoning) = chat_reasoning_to_response_output_item(message, &response_id) {
        output.push(reasoning);
    }
    if let Some(message) = chat_message_to_response_output_item(message, &response_id) {
        output.push(message);
    }
    output.extend(chat_tool_calls_to_response_output_items(
        message,
        tool_context,
    ));

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": body.get("created").and_then(Value::as_u64).unwrap_or(0),
        "status": response_status(choice.get("finish_reason").and_then(Value::as_str)),
        "model": body.get("model").and_then(Value::as_str).unwrap_or(""),
        "output": output,
        "usage": chat_usage_to_responses_usage(body.get("usage"))
    });

    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }
    copy_response_request_fields(&mut response, original_request);

    Ok(response)
}

pub fn responses_to_anthropic_messages(body: Value) -> anyhow::Result<Value> {
    responses_to_anthropic_messages_with_diagnostic_id(body, None)
}

fn responses_to_anthropic_messages_with_diagnostic_id(
    body: Value,
    diagnostic_id: Option<&str>,
) -> anyhow::Result<Value> {
    let mut result = json!({});

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    result["max_tokens"] = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
        .or_else(|| body.get("max_completion_tokens"))
        .cloned()
        .unwrap_or_else(|| json!(32000));

    let mut system_chunks = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let text = instruction_text(instructions);
        if !text.is_empty() {
            system_chunks.push(text);
        }
    }

    let mut messages = Vec::new();
    if let Some(input) = body.get("input") {
        append_responses_input_to_anthropic(input, &mut messages, &mut system_chunks);
    }
    if messages.is_empty() {
        messages.push(json!({ "role": "user", "content": [{ "type": "text", "text": "" }] }));
    }
    result["messages"] = json!(messages);

    for key in ["temperature", "top_p", "stream"] {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }
    if let Some(stop) = body.get("stop") {
        result["stop_sequences"] = match stop {
            Value::String(_) => json!([stop.clone()]),
            Value::Array(_) => stop.clone(),
            _ => Value::Null,
        };
    }
    if let Some(user_id) = body.pointer("/metadata/user_id").and_then(Value::as_str) {
        if !user_id.is_empty() {
            result["metadata"] = json!({ "user_id": user_id });
        }
    }

    let conversion_tools = tools_for_proxy_conversion(&body);
    let tool_context = build_codex_tool_context_from_tools(&conversion_tools);
    let mut has_tools = false;
    if !conversion_tools.is_empty() {
        let converted = responses_tools_to_anthropic_tools(&conversion_tools, &tool_context);
        if !converted.is_empty() {
            has_tools = true;
            result["tools"] = json!(converted);
        }
    }
    if has_tools {
        if let Some(tool_choice) = body
            .get("tool_choice")
            .and_then(|value| responses_tool_choice_to_anthropic(value, &tool_context))
        {
            result["tool_choice"] = tool_choice;
        }
    }

    if !system_chunks.is_empty() {
        result["system"] = json!(system_chunks.join("\n\n"));
    }

    let model = body.get("model").and_then(Value::as_str).unwrap_or("");
    apply_anthropic_reasoning_options(&mut result, &body, model);
    log_anthropic_request_shape(&result, &body, diagnostic_id);

    Ok(result)
}

pub fn anthropic_message_to_response(body: Value) -> anyhow::Result<Value> {
    anthropic_message_to_response_with_context(body, &CodexToolContext::default(), None, None)
}

pub fn anthropic_message_to_response_with_request(
    body: Value,
    original_request: &Value,
) -> anyhow::Result<Value> {
    anthropic_message_to_response_with_request_and_diagnostic_id(body, original_request, None)
}

pub fn anthropic_message_to_response_with_request_and_diagnostic_id(
    body: Value,
    original_request: &Value,
    diagnostic_id: Option<&str>,
) -> anyhow::Result<Value> {
    let context = build_codex_tool_context_for_request(original_request);
    anthropic_message_to_response_with_context(
        body,
        &context,
        Some(original_request),
        diagnostic_id,
    )
}

fn anthropic_message_to_response_with_context(
    body: Value,
    tool_context: &CodexToolContext,
    original_request: Option<&Value>,
    diagnostic_id: Option<&str>,
) -> anyhow::Result<Value> {
    let response_id = response_id_from_chat_id(body.get("id").and_then(Value::as_str));
    let stop_reason = body.get("stop_reason").and_then(Value::as_str);
    let status = anthropic_response_status(stop_reason);
    let output = anthropic_content_to_response_output_items(
        body.get("content").unwrap_or(&Value::Null),
        &response_id,
        tool_context,
    );
    log_anthropic_response_shape(&body, &output, false, diagnostic_id);
    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": 0,
        "status": status,
        "model": body.get("model").and_then(Value::as_str).unwrap_or(""),
        "output": output,
        "usage": anthropic_usage_to_responses_usage(body.get("usage"))
    });

    if status == "incomplete" {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }
    copy_response_request_fields(&mut response, original_request);

    Ok(response)
}

pub struct ProxyHttpResponse {
    pub status: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub struct UpstreamProxyResponse {
    pub status_code: u16,
    pub content_type: String,
    pub is_stream: bool,
    pub response_protocol: UpstreamResponseProtocol,
    pub diagnostic_id: String,
    pub relay_id: Option<String>,
    pub relay_name: Option<String>,
    pub endpoint: Option<String>,
    pub response: Option<reqwest::Response>,
    pub body_override: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpstreamResponseProtocol {
    Responses,
    ChatCompletions,
    Anthropic,
}

impl UpstreamProxyResponse {
    pub fn status(&self) -> String {
        http_status_line(self.status_code)
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    pub async fn into_body_bytes(mut self) -> anyhow::Result<Vec<u8>> {
        if let Some(body) = self.body_override.take() {
            return Ok(body);
        }
        let Some(response) = self.response.take() else {
            anyhow::bail!("上游响应体已被本地代理消费");
        };
        Ok(response.bytes().await?.to_vec())
    }

    pub fn into_response(mut self) -> anyhow::Result<reqwest::Response> {
        let Some(response) = self.response.take() else {
            anyhow::bail!("上游响应已被本地代理消费");
        };
        Ok(response)
    }
}

pub fn upstream_header_timeout() -> Duration {
    UPSTREAM_HEADER_TIMEOUT
}

pub fn upstream_stream_header_timeout() -> Duration {
    UPSTREAM_STREAM_HEADER_TIMEOUT
}

pub fn upstream_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(UPSTREAM_CONNECT_TIMEOUT)
        .user_agent("CodexElves/ProtocolProxy")
        .build()
        .context("failed to build upstream HTTP client")
}

pub async fn send_upstream_request(
    request: reqwest::RequestBuilder,
) -> anyhow::Result<reqwest::Response> {
    send_upstream_request_with_header_timeout(request, UPSTREAM_HEADER_TIMEOUT).await
}

pub async fn send_upstream_request_for_responses(
    request: reqwest::RequestBuilder,
    is_stream: bool,
) -> anyhow::Result<reqwest::Response> {
    let timeout = response_header_timeout(is_stream);
    send_upstream_request_with_header_timeout(request, timeout).await
}

pub async fn send_upstream_request_with_header_timeout(
    request: reqwest::RequestBuilder,
    timeout: Duration,
) -> anyhow::Result<reqwest::Response> {
    tokio::time::timeout(timeout, request.send())
        .await
        .with_context(|| format!("上游请求超过 {} 秒未返回响应头", timeout.as_secs()))?
        .context("上游请求失败")
}

pub struct ChatSseToResponsesConverter {
    buffer: String,
    utf8_remainder: Vec<u8>,
    state: ChatSseState,
    diagnostic_id: Option<String>,
    failed: bool,
    saw_done: bool,
    failure_source: Option<&'static str>,
    failure_message: Option<String>,
    failure_type: Option<String>,
}

impl Default for ChatSseToResponsesConverter {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            utf8_remainder: Vec::new(),
            state: ChatSseState::default(),
            diagnostic_id: None,
            failed: false,
            saw_done: false,
            failure_source: None,
            failure_message: None,
            failure_type: None,
        }
    }
}

impl ChatSseToResponsesConverter {
    pub fn with_request(original_request: &Value) -> Self {
        Self::with_request_and_diagnostic_id(original_request, None)
    }

    pub fn with_request_and_diagnostic_id(
        original_request: &Value,
        diagnostic_id: Option<&str>,
    ) -> Self {
        Self {
            state: ChatSseState::with_request(original_request),
            diagnostic_id: diagnostic_id.map(ToString::to_string),
            ..Self::default()
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<u8> {
        append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, bytes);
        let mut output = String::new();
        while let Some(block) = take_sse_block(&mut self.buffer) {
            if block.trim().is_empty() {
                continue;
            }
            self.handle_block(&block, &mut output);
            if self.failed {
                break;
            }
        }
        output.into_bytes()
    }

    pub fn finish(&mut self) -> Vec<u8> {
        if !self.utf8_remainder.is_empty() {
            self.buffer
                .push_str(&String::from_utf8_lossy(&self.utf8_remainder));
            self.utf8_remainder.clear();
        }

        let mut output = String::new();
        if !self.failed {
            if !self.buffer.trim().is_empty() && !self.state.completed {
                let pending = std::mem::take(&mut self.buffer);
                self.handle_block(&pending, &mut output);
            }
        }
        if !self.failed && !self.state.completed {
            if !self.saw_done && self.state.finish_reason.is_none() {
                self.record_failure_into(
                    &mut output,
                    "missing_completion_marker",
                    "upstream stream ended before a completion marker".to_string(),
                    Some("stream_error".to_string()),
                );
                return output.into_bytes();
            }
            self.state.finalize_into(&mut output);
        }
        output.into_bytes()
    }

    pub fn fail(&mut self, message: String, error_type: Option<String>) -> Vec<u8> {
        let mut output = String::new();
        self.record_failure_into(&mut output, "stream_read_error", message, error_type);
        output.into_bytes()
    }

    pub fn diagnostic_summary(&self) -> Value {
        json!({
            "diagnosticId": self.diagnostic_id.as_deref(),
            "responseProtocol": "chat_completions",
            "model": self.state.model.as_str(),
            "terminalStatus": stream_terminal_status(self.failed, self.state.completed),
            "sawDone": self.saw_done,
            "finishReason": self.state.finish_reason.as_deref(),
            "failureSource": self.failure_source,
            "failureType": self.failure_type.as_deref(),
            "failureMessage": self.failure_message.as_deref().map(diagnostic_text_preview),
        })
    }

    fn handle_block(&mut self, block: &str, output: &mut String) {
        let mut event_name: Option<String> = None;
        let mut data_parts = Vec::new();
        for line in block.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                event_name = Some(event.trim().to_string());
            }
            if let Some(data) = strip_sse_field(line, "data") {
                data_parts.push(data.to_string());
            }
        }

        if data_parts.is_empty() {
            return;
        }
        let data = data_parts.join("\n");
        if data.trim() == "[DONE]" {
            self.saw_done = true;
            self.state.finalize_into(output);
            return;
        }

        let Ok(chunk) = serde_json::from_str::<Value>(&data) else {
            self.record_failure_into(
                output,
                "invalid_sse_json",
                "upstream stream sent invalid JSON data".to_string(),
                Some("invalid_sse_json".to_string()),
            );
            return;
        };
        if event_name.as_deref() == Some("error") || chunk.get("error").is_some() {
            let (message, error_type) = extract_chat_sse_error(&chunk);
            self.log_upstream_error_event(
                "chat_completions",
                event_name.as_deref(),
                &chunk,
                &message,
                error_type.as_deref(),
            );
            self.record_failure_into(output, "upstream_sse_error", message, error_type);
            return;
        }
        self.state.handle_chat_chunk_into(&chunk, output);
    }

    fn record_failure_into(
        &mut self,
        output: &mut String,
        source: &'static str,
        message: String,
        error_type: Option<String>,
    ) {
        self.failure_source = Some(source);
        self.failure_message = Some(message.clone());
        self.failure_type = error_type.clone();
        log_stream_conversion_failure(
            "chat_completions",
            self.diagnostic_id.as_deref(),
            self.state.model.as_str(),
            source,
            error_type.as_deref(),
            &message,
        );
        self.state.failed_into(output, message, error_type);
        self.failed = true;
    }

    fn log_upstream_error_event(
        &self,
        response_protocol: &'static str,
        event_name: Option<&str>,
        chunk: &Value,
        message: &str,
        error_type: Option<&str>,
    ) {
        log_stream_upstream_error_event(
            response_protocol,
            self.diagnostic_id.as_deref(),
            self.state.model.as_str(),
            event_name,
            error_type,
            message,
            chunk,
        );
    }
}

pub struct AnthropicSseToResponsesConverter {
    buffer: String,
    utf8_remainder: Vec<u8>,
    state: AnthropicSseState,
    failed: bool,
    saw_message_stop: bool,
    failure_source: Option<&'static str>,
    failure_message: Option<String>,
    failure_type: Option<String>,
}

impl Default for AnthropicSseToResponsesConverter {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            utf8_remainder: Vec::new(),
            state: AnthropicSseState::default(),
            failed: false,
            saw_message_stop: false,
            failure_source: None,
            failure_message: None,
            failure_type: None,
        }
    }
}

impl AnthropicSseToResponsesConverter {
    pub fn with_request(original_request: &Value) -> Self {
        Self::with_request_and_diagnostic_id(original_request, None)
    }

    pub fn with_request_and_diagnostic_id(
        original_request: &Value,
        diagnostic_id: Option<&str>,
    ) -> Self {
        Self {
            state: AnthropicSseState::with_request_and_diagnostic_id(
                original_request,
                diagnostic_id,
            ),
            ..Self::default()
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<u8> {
        append_utf8_safe(&mut self.buffer, &mut self.utf8_remainder, bytes);
        let mut output = String::new();
        while let Some(block) = take_sse_block(&mut self.buffer) {
            if block.trim().is_empty() {
                continue;
            }
            self.handle_block(&block, &mut output);
            if self.failed {
                break;
            }
        }
        output.into_bytes()
    }

    pub fn finish(&mut self) -> Vec<u8> {
        if !self.utf8_remainder.is_empty() {
            self.buffer
                .push_str(&String::from_utf8_lossy(&self.utf8_remainder));
            self.utf8_remainder.clear();
        }

        let mut output = String::new();
        if !self.failed && !self.buffer.trim().is_empty() && !self.state.inner.completed {
            let pending = std::mem::take(&mut self.buffer);
            self.handle_block(&pending, &mut output);
        }
        if !self.failed && !self.state.inner.completed {
            if !self.saw_message_stop {
                self.record_failure_into(
                    &mut output,
                    "missing_completion_marker",
                    "upstream stream ended before a completion marker".to_string(),
                    Some("stream_error".to_string()),
                );
                return output.into_bytes();
            }
            self.state.inner.finalize_into(&mut output);
        }
        output.into_bytes()
    }

    pub fn fail(&mut self, message: String, error_type: Option<String>) -> Vec<u8> {
        let mut output = String::new();
        self.record_failure_into(&mut output, "stream_read_error", message, error_type);
        output.into_bytes()
    }

    pub fn diagnostic_summary(&self) -> Value {
        json!({
            "diagnosticId": self.state.diagnostic_id.as_deref(),
            "responseProtocol": "anthropic",
            "model": self.state.inner.model.as_str(),
            "terminalStatus": stream_terminal_status(self.failed, self.state.inner.completed),
            "sawMessageStop": self.saw_message_stop,
            "finishReason": self.state.inner.finish_reason.as_deref(),
            "failureSource": self.failure_source,
            "failureType": self.failure_type.as_deref(),
            "failureMessage": self.failure_message.as_deref().map(diagnostic_text_preview),
        })
    }

    fn handle_block(&mut self, block: &str, output: &mut String) {
        let mut event_name: Option<String> = None;
        let mut data_parts = Vec::new();
        for line in block.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                event_name = Some(event.trim().to_string());
            }
            if let Some(data) = strip_sse_field(line, "data") {
                data_parts.push(data.to_string());
            }
        }

        if data_parts.is_empty() {
            return;
        }
        let data = data_parts.join("\n");
        if data.trim() == "[DONE]" {
            self.saw_message_stop = true;
            self.state.inner.finalize_into(output);
            return;
        }

        let Ok(chunk) = serde_json::from_str::<Value>(&data) else {
            self.record_failure_into(
                output,
                "invalid_sse_json",
                "upstream stream sent invalid JSON data".to_string(),
                Some("invalid_sse_json".to_string()),
            );
            return;
        };
        let event = event_name
            .as_deref()
            .or_else(|| chunk.get("type").and_then(Value::as_str))
            .unwrap_or("");
        if event == "error" || chunk.get("error").is_some() {
            let (message, error_type) = extract_chat_sse_error(&chunk);
            log_stream_upstream_error_event(
                "anthropic",
                self.state.diagnostic_id.as_deref(),
                self.state.inner.model.as_str(),
                Some(event),
                error_type.as_deref(),
                &message,
                &chunk,
            );
            self.record_failure_into(output, "upstream_sse_error", message, error_type);
            return;
        }
        if event == "message_stop" {
            self.saw_message_stop = true;
        }
        self.state
            .handle_anthropic_event_into(event, &chunk, output);
    }

    fn record_failure_into(
        &mut self,
        output: &mut String,
        source: &'static str,
        message: String,
        error_type: Option<String>,
    ) {
        self.failure_source = Some(source);
        self.failure_message = Some(message.clone());
        self.failure_type = error_type.clone();
        log_stream_conversion_failure(
            "anthropic",
            self.state.diagnostic_id.as_deref(),
            self.state.inner.model.as_str(),
            source,
            error_type.as_deref(),
            &message,
        );
        self.state.inner.failed_into(output, message, error_type);
        self.failed = true;
    }
}

pub fn is_responses_proxy_path(path: &str) -> bool {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    matches!(
        path,
        "/responses"
            | "/v1/responses"
            | "/v1/v1/responses"
            | "/codex/v1/responses"
            | "/responses/compact"
            | "/v1/responses/compact"
            | "/v1/v1/responses/compact"
            | "/codex/v1/responses/compact"
    )
}

pub fn is_chat_completions_proxy_path(path: &str) -> bool {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    matches!(
        path,
        "/chat/completions"
            | "/v1/chat/completions"
            | "/v1/v1/chat/completions"
            | "/codex/v1/chat/completions"
    )
}

pub fn is_models_proxy_path(path: &str) -> bool {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    matches!(
        path,
        "/models" | "/v1/models" | "/v1/v1/models" | "/codex/v1/models"
    )
}

pub async fn open_responses_proxy_request(
    body: &str,
    original_user_agent: Option<&str>,
) -> anyhow::Result<UpstreamProxyResponse> {
    let settings = SettingsStore::default().load().unwrap_or_default();
    open_responses_proxy_request_with_settings_and_user_agent(body, settings, original_user_agent)
        .await
}

pub async fn open_responses_proxy_request_with_settings(
    body: &str,
    settings: crate::settings::BackendSettings,
) -> anyhow::Result<UpstreamProxyResponse> {
    open_responses_proxy_request_with_settings_and_user_agent(body, settings, None).await
}

async fn open_responses_proxy_request_with_settings_and_user_agent(
    body: &str,
    settings: crate::settings::BackendSettings,
    original_user_agent: Option<&str>,
) -> anyhow::Result<UpstreamProxyResponse> {
    let diagnostic_id = next_protocol_proxy_diagnostic_id();
    let request_json: Value = serde_json::from_str(body)?;
    let is_stream = request_json
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    log_responses_request_metadata(&request_json, body.len(), is_stream, &diagnostic_id);
    let context = RotationContext {
        conversation_id: conversation_id_from_responses_request(&request_json),
    };
    let relay = crate::relay_rotation::select_relay_for_request(&settings, context)?;
    let mut relays = vec![relay.clone()];
    relays.extend(crate::relay_rotation::fallback_relays_after(
        &settings, &relay.id,
    )?);
    let relay_count = relays.len();
    for (attempt, relay) in relays.into_iter().enumerate() {
        validate_upstream(&relay)?;
        let response_protocol = responses_proxy_target_protocol(&relay, &request_json)?;
        let endpoint = upstream_endpoint_for_protocol(&relay, response_protocol);
        let has_more_candidates = attempt + 1 < relay_count;
        let upstream_is_stream = is_stream;
        let header_timeout = response_header_timeout(upstream_is_stream);
        let translated_server_side_tools =
            if response_protocol == UpstreamResponseProtocol::Responses {
                Vec::new()
            } else {
                proxy_internal_tool_types(request_json.get("tools"))
            };
        if !translated_server_side_tools.is_empty() {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "protocol_proxy.server_side_tools_translated",
                json!({
                    "diagnosticId": diagnostic_id.as_str(),
                    "relayId": relay.id,
                    "relayName": relay.name,
                    "responseProtocol": response_protocol,
                    "tools": translated_server_side_tools
                }),
            );
        }
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.upstream_request",
            json!({
                "diagnosticId": diagnostic_id.as_str(),
                "relayId": relay.id,
                "relayName": relay.name,
                "endpoint": endpoint,
                "responseProtocol": response_protocol,
                "stream": upstream_is_stream,
                "clientStream": is_stream,
                "attempt": attempt + 1,
                "candidateCount": relay_count,
                "headerTimeoutSeconds": header_timeout.as_secs()
            }),
        );
        let client = crate::http_client::proxied_client(&effective_user_agent(
            &relay.user_agent,
            original_user_agent,
        ))?;
        let upstream = match send_responses_upstream_request(
            &client,
            &relay,
            &request_json,
            response_protocol,
            upstream_is_stream,
            &diagnostic_id,
        )
        .await
        {
            Ok(upstream) => upstream,
            Err(error) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "protocol_proxy.upstream_request_failed",
                    json!({
                        "diagnosticId": diagnostic_id.as_str(),
                        "relayId": relay.id,
                        "relayName": relay.name,
                        "endpoint": endpoint,
                        "responseProtocol": response_protocol,
                        "stream": upstream_is_stream,
                        "clientStream": is_stream,
                        "attempt": attempt + 1,
                        "candidateCount": relay_count,
                        "headerTimeoutSeconds": header_timeout.as_secs(),
                        "willFailover": has_more_candidates,
                        "error": error.to_string()
                    }),
                );
                crate::relay_rotation::record_relay_request_failure(&settings);
                if has_more_candidates {
                    continue;
                }
                return Err(error).with_context(|| {
                    format!(
                        "供应商「{}」请求上游失败，endpoint: {}",
                        relay.name, endpoint
                    )
                });
            }
        };
        let status_code = upstream.status().as_u16();
        let mut upstream_response = Some(upstream);
        let mut body_override = None;
        let mut status_code = status_code;
        let mut content_type = upstream_response
            .as_ref()
            .unwrap()
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        if response_protocol == UpstreamResponseProtocol::Anthropic {
            let mut anthropic_request = responses_to_anthropic_messages_with_diagnostic_id(
                request_json.clone(),
                Some(&diagnostic_id),
            )?;
            apply_cached_anthropic_reasoning_compatibility(&mut anthropic_request);
            if should_retry_anthropic_max_effort(status_code, &content_type, &anthropic_request) {
                let response = upstream_response
                    .take()
                    .expect("anthropic response is present before retry inspection");
                let error_body = response.bytes().await?.to_vec();
                if let Some(fallback_effort) =
                    anthropic_effort_fallback_from_error(&error_body, "max")
                {
                    remember_anthropic_reasoning_compatibility(&anthropic_request, fallback_effort);
                    let mut retry_request = anthropic_request.clone();
                    apply_cached_anthropic_reasoning_compatibility(&mut retry_request);
                    let retry = send_anthropic_messages_request(
                        &client,
                        &relay,
                        &retry_request,
                        upstream_is_stream,
                    )
                    .await?;
                    status_code = retry.status().as_u16();
                    content_type = retry
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    upstream_response = Some(retry);
                } else {
                    body_override = Some(error_body);
                }
            }
        }
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.upstream_response",
            json!({
                "diagnosticId": diagnostic_id.as_str(),
                "relayId": relay.id,
                "relayName": relay.name,
                "endpoint": endpoint,
                "responseProtocol": response_protocol,
                "stream": upstream_is_stream,
                "clientStream": is_stream,
                "statusCode": status_code,
                "attempt": attempt + 1,
                "candidateCount": relay_count,
                "headerTimeoutSeconds": header_timeout.as_secs(),
                "willFailover": has_more_candidates && !(200..300).contains(&status_code)
            }),
        );
        crate::relay_rotation::record_relay_request_event(
            &settings,
            if (200..300).contains(&status_code) {
                RotationEvent::Success
            } else {
                RotationEvent::Failure
            },
        );
        if (200..300).contains(&status_code) || !has_more_candidates {
            return Ok(UpstreamProxyResponse {
                status_code,
                is_stream: upstream_is_stream || content_type.contains("text/event-stream"),
                content_type,
                response_protocol,
                diagnostic_id,
                relay_id: Some(relay.id),
                relay_name: Some(relay.name),
                endpoint: Some(endpoint),
                response: upstream_response,
                body_override,
            });
        }
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "protocol_proxy.upstream_failover",
            json!({
                "diagnosticId": diagnostic_id.as_str(),
                "relayId": relay.id,
                "relayName": relay.name,
                "endpoint": endpoint,
                "responseProtocol": response_protocol,
                "stream": upstream_is_stream,
                "clientStream": is_stream,
                "statusCode": status_code,
                "attempt": attempt + 1,
                "candidateCount": relay_count,
                "headerTimeoutSeconds": header_timeout.as_secs()
            }),
        );
    }
    anyhow::bail!("未找到可用的聚合供应商成员")
}

async fn send_responses_upstream_request(
    client: &reqwest::Client,
    relay: &crate::settings::RelayProfile,
    request_json: &Value,
    response_protocol: UpstreamResponseProtocol,
    is_stream: bool,
    diagnostic_id: &str,
) -> anyhow::Result<reqwest::Response> {
    match response_protocol {
        UpstreamResponseProtocol::Responses => {
            send_upstream_request_for_responses(
                upstream_request_builder(
                    client.clone(),
                    &responses_url(&relay.base_url),
                    relay.api_key.trim(),
                    is_stream,
                    request_json,
                ),
                is_stream,
            )
            .await
        }
        UpstreamResponseProtocol::ChatCompletions => {
            let chat_request = responses_to_chat_completions(responses_request_with_stream(
                request_json,
                is_stream,
            ))?;
            send_chat_completions_request(client, relay, &chat_request, is_stream).await
        }
        UpstreamResponseProtocol::Anthropic => {
            let mut anthropic_request = responses_to_anthropic_messages_with_diagnostic_id(
                responses_request_with_stream(request_json, is_stream),
                Some(diagnostic_id),
            )?;
            apply_cached_anthropic_reasoning_compatibility(&mut anthropic_request);
            send_anthropic_messages_request(client, relay, &anthropic_request, is_stream).await
        }
    }
}

fn responses_request_with_stream(request_json: &Value, is_stream: bool) -> Value {
    let mut request = request_json.clone();
    if let Some(object) = request.as_object_mut() {
        object.insert("stream".to_string(), json!(is_stream));
        if !is_stream {
            object.remove("stream_options");
        }
    }
    request
}

async fn send_chat_completions_request(
    client: &reqwest::Client,
    relay: &crate::settings::RelayProfile,
    chat_request: &Value,
    is_stream: bool,
) -> anyhow::Result<reqwest::Response> {
    send_upstream_request_for_responses(
        upstream_request_builder(
            client.clone(),
            &chat_completions_url(&relay.base_url),
            relay.api_key.trim(),
            is_stream,
            chat_request,
        ),
        is_stream,
    )
    .await
}

fn upstream_endpoint_for_protocol(
    relay: &crate::settings::RelayProfile,
    response_protocol: UpstreamResponseProtocol,
) -> String {
    match response_protocol {
        UpstreamResponseProtocol::Responses => responses_url(&relay.base_url),
        UpstreamResponseProtocol::ChatCompletions => chat_completions_url(&relay.base_url),
        UpstreamResponseProtocol::Anthropic => anthropic_messages_url(&relay.base_url),
    }
}

fn responses_proxy_target_protocol(
    relay: &crate::settings::RelayProfile,
    request_json: &Value,
) -> anyhow::Result<UpstreamResponseProtocol> {
    let model = request_json
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if model.is_empty() {
        anyhow::bail!("请求缺少 model，无法按模型列表选择上游协议");
    }
    let responses_models = crate::model_catalog::relay_profile_responses_model_ids(relay);
    if responses_models.iter().any(|item| item == model) {
        return Ok(UpstreamResponseProtocol::Responses);
    }
    let chat_models = crate::model_catalog::relay_profile_chat_completions_model_ids(relay);
    if chat_models.iter().any(|item| item == model) {
        return Ok(UpstreamResponseProtocol::ChatCompletions);
    }
    let anthropic_models = crate::model_catalog::relay_profile_anthropic_model_ids(relay);
    if anthropic_models.iter().any(|item| item == model) {
        return Ok(UpstreamResponseProtocol::Anthropic);
    }
    Ok(response_protocol_for_relay_protocol(relay.protocol))
}

fn response_protocol_for_relay_protocol(
    protocol: crate::settings::RelayProtocol,
) -> UpstreamResponseProtocol {
    match protocol {
        crate::settings::RelayProtocol::Responses => UpstreamResponseProtocol::Responses,
        crate::settings::RelayProtocol::ChatCompletions => {
            UpstreamResponseProtocol::ChatCompletions
        }
        crate::settings::RelayProtocol::Anthropic => UpstreamResponseProtocol::Anthropic,
    }
}

async fn send_anthropic_messages_request(
    client: &reqwest::Client,
    relay: &crate::settings::RelayProfile,
    anthropic_request: &Value,
    is_stream: bool,
) -> anyhow::Result<reqwest::Response> {
    send_upstream_request_for_responses(
        client
            .post(anthropic_messages_url(&relay.base_url))
            .header("x-api-key", relay.api_key.trim())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(anthropic_request),
        is_stream,
    )
    .await
}

pub async fn open_models_proxy_request(
    original_user_agent: Option<&str>,
) -> anyhow::Result<UpstreamProxyResponse> {
    let diagnostic_id = next_protocol_proxy_diagnostic_id();
    let settings = SettingsStore::default().load().unwrap_or_default();
    let aggregate_enabled = settings.active_aggregate_relay_profile().is_some();
    let relay = if aggregate_enabled {
        crate::relay_rotation::select_relay_for_probe(&settings)?
    } else {
        settings.active_relay_profile()
    };
    if !aggregate_enabled && !relay.local_proxy_enabled() {
        anyhow::bail!("当前中转未启用本地代理");
    }
    validate_upstream(&relay)?;

    let endpoint = models_url(&relay.base_url);
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.models_request",
        json!({
            "diagnosticId": diagnostic_id.as_str(),
            "relayId": relay.id,
            "relayName": relay.name,
            "endpoint": endpoint,
            "responseProtocol": UpstreamResponseProtocol::Responses
        }),
    );
    let upstream = send_upstream_request(
        crate::http_client::proxied_client(&effective_user_agent(
            &relay.user_agent,
            original_user_agent,
        ))?
        .get(&endpoint)
        .bearer_auth(relay.api_key.trim()),
    )
    .await?;
    let status_code = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json; charset=utf-8")
        .to_string();

    Ok(UpstreamProxyResponse {
        status_code,
        is_stream: false,
        content_type,
        response_protocol: UpstreamResponseProtocol::Responses,
        diagnostic_id,
        relay_id: Some(relay.id),
        relay_name: Some(relay.name),
        endpoint: Some(endpoint),
        response: Some(upstream),
        body_override: None,
    })
}

pub async fn open_chat_completions_proxy_request(
    body: &str,
    original_user_agent: Option<&str>,
) -> anyhow::Result<UpstreamProxyResponse> {
    let diagnostic_id = next_protocol_proxy_diagnostic_id();
    let settings = SettingsStore::default().load().unwrap_or_default();
    let relay = settings.active_relay_profile();
    if !relay.local_proxy_enabled() {
        anyhow::bail!("当前中转未启用本地代理");
    }
    if relay.base_url.trim().is_empty() {
        anyhow::bail!("Chat Completions 上游 Base URL 不能为空");
    }
    if relay.api_key.trim().is_empty() {
        anyhow::bail!("Chat Completions 上游 Key 不能为空");
    }

    let request_json: Value = serde_json::from_str(body)?;
    let is_stream = request_json
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let upstream = crate::http_client::proxied_client(&effective_user_agent(
        &relay.user_agent,
        original_user_agent,
    ))?
    .post(chat_completions_url(&relay.base_url))
    .bearer_auth(relay.api_key.trim())
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .json(&request_json)
    .send()
    .await?;
    let status_code = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok(UpstreamProxyResponse {
        status_code,
        is_stream: is_stream || content_type.contains("text/event-stream"),
        content_type,
        response_protocol: UpstreamResponseProtocol::ChatCompletions,
        diagnostic_id,
        relay_id: Some(relay.id),
        relay_name: Some(relay.name),
        endpoint: Some(chat_completions_url(&relay.base_url)),
        response: Some(upstream),
        body_override: None,
    })
}

fn response_header_timeout(is_stream: bool) -> Duration {
    if is_stream {
        UPSTREAM_STREAM_HEADER_TIMEOUT
    } else {
        UPSTREAM_HEADER_TIMEOUT
    }
}

fn upstream_request_builder(
    client: reqwest::Client,
    endpoint: &str,
    api_key: &str,
    is_stream: bool,
    upstream_body: &Value,
) -> reqwest::RequestBuilder {
    let mut builder = client
        .post(endpoint)
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if is_stream {
        builder = builder
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .header(reqwest::header::CACHE_CONTROL, "no-cache");
    }
    builder.json(upstream_body)
}

fn validate_upstream(relay: &crate::settings::RelayProfile) -> anyhow::Result<()> {
    if relay.base_url.trim().is_empty() {
        anyhow::bail!("上游 Base URL 不能为空");
    }
    if relay.api_key.trim().is_empty() {
        anyhow::bail!("上游 Key 不能为空");
    }
    Ok(())
}

fn conversation_id_from_responses_request(body: &Value) -> Option<String> {
    for key in ["conversation", "conversation_id", "previous_response_id"] {
        if let Some(value) = body.get(key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn effective_user_agent(configured_user_agent: &str, original_user_agent: Option<&str>) -> String {
    let configured_user_agent = configured_user_agent.trim();
    if !configured_user_agent.is_empty() {
        return configured_user_agent.to_string();
    }
    original_user_agent
        .map(str::trim)
        .filter(|user_agent| !user_agent.is_empty())
        .unwrap_or("")
        .to_string()
}

pub async fn handle_responses_proxy_request(body: &str) -> anyhow::Result<ProxyHttpResponse> {
    let request_json: Value = serde_json::from_str(body)?;
    let upstream = open_responses_proxy_request(body, None).await?;
    let status_code = upstream.status_code;
    let upstream_content_type = upstream.content_type.clone();
    let is_stream = upstream.is_stream;
    let response_protocol = upstream.response_protocol;
    let diagnostic_id = upstream.diagnostic_id.clone();
    let upstream_body = upstream.into_body_bytes().await?;

    if !(200..300).contains(&status_code) {
        if response_protocol == UpstreamResponseProtocol::Responses {
            return Ok(ProxyHttpResponse {
                status: http_status_line(status_code),
                content_type: if upstream_content_type.is_empty() {
                    "application/json; charset=utf-8".to_string()
                } else {
                    upstream_content_type
                },
                body: upstream_body,
            });
        }
        let error =
            responses_error_from_upstream(status_code, &upstream_content_type, &upstream_body);
        return Ok(ProxyHttpResponse {
            status: http_status_line(status_code),
            content_type: "application/json; charset=utf-8".to_string(),
            body: serde_json::to_vec(&error)?,
        });
    }

    if is_stream {
        if response_protocol == UpstreamResponseProtocol::Responses {
            return Ok(ProxyHttpResponse {
                status: "200 OK".to_string(),
                content_type: upstream_content_type,
                body: upstream_body,
            });
        }
        let text = String::from_utf8_lossy(&upstream_body);
        let body = match response_protocol {
            UpstreamResponseProtocol::ChatCompletions => {
                chat_sse_to_responses_sse_with_request(&text, &request_json).into_bytes()
            }
            UpstreamResponseProtocol::Anthropic => {
                anthropic_sse_to_responses_sse_with_request_and_diagnostic_id(
                    &text,
                    &request_json,
                    Some(&diagnostic_id),
                )
                .into_bytes()
            }
            UpstreamResponseProtocol::Responses => unreachable!(),
        };
        return Ok(ProxyHttpResponse {
            status: "200 OK".to_string(),
            content_type: "text/event-stream; charset=utf-8".to_string(),
            body,
        });
    }

    if response_protocol == UpstreamResponseProtocol::Responses {
        return Ok(ProxyHttpResponse {
            status: "200 OK".to_string(),
            content_type: upstream_content_type,
            body: upstream_body,
        });
    }

    let upstream_json: Value = serde_json::from_slice(&upstream_body)?;
    let response_json = match response_protocol {
        UpstreamResponseProtocol::ChatCompletions => {
            chat_completion_to_response_with_request(upstream_json, &request_json)?
        }
        UpstreamResponseProtocol::Anthropic => {
            anthropic_message_to_response_with_request_and_diagnostic_id(
                upstream_json,
                &request_json,
                Some(&diagnostic_id),
            )?
        }
        UpstreamResponseProtocol::Responses => unreachable!(),
    };
    Ok(ProxyHttpResponse {
        status: "200 OK".to_string(),
        content_type: "application/json; charset=utf-8".to_string(),
        body: serde_json::to_vec(&response_json)?,
    })
}

pub fn chat_completions_url(base_url: &str) -> String {
    let skip_version_prefix = base_url.trim().ends_with('#');
    let base = base_url.trim().trim_end_matches('#').trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/chat/completions") {
        return base.to_string();
    }
    let origin_only = base
        .split_once("://")
        .map_or(!base.contains('/'), |(_, rest)| !rest.contains('/'));
    let mut url = if skip_version_prefix || has_version_suffix(base) || !origin_only {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    };
    while url.contains("/v1/v1") {
        url = url.replace("/v1/v1", "/v1");
    }
    url
}

pub fn responses_url(base_url: &str) -> String {
    let skip_version_prefix = base_url.trim().ends_with('#');
    let base = base_url.trim().trim_end_matches('#').trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/responses") {
        return base.to_string();
    }
    let origin_only = base
        .split_once("://")
        .map_or(!base.contains('/'), |(_, rest)| !rest.contains('/'));
    let mut url = if skip_version_prefix || has_version_suffix(base) || !origin_only {
        format!("{base}/responses")
    } else {
        format!("{base}/v1/responses")
    };
    while url.contains("/v1/v1") {
        url = url.replace("/v1/v1", "/v1");
    }
    url
}

pub fn anthropic_messages_url(base_url: &str) -> String {
    let skip_version_prefix = base_url.trim().ends_with('#');
    let base = base_url.trim().trim_end_matches('#').trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/messages") {
        return base.to_string();
    }
    let origin_only = base
        .split_once("://")
        .map_or(!base.contains('/'), |(_, rest)| !rest.contains('/'));
    let mut url = if skip_version_prefix || has_version_suffix(base) || !origin_only {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    };
    while url.contains("/v1/v1") {
        url = url.replace("/v1/v1", "/v1");
    }
    url
}

pub fn models_url(base_url: &str) -> String {
    let skip_version_prefix = base_url.trim().ends_with('#');
    let mut base = base_url
        .trim()
        .trim_end_matches('#')
        .trim_end_matches('/')
        .to_string();
    if base.to_ascii_lowercase().ends_with("/chat/completions") {
        base.truncate(base.len() - "/chat/completions".len());
    }
    if base.to_ascii_lowercase().ends_with("/models") {
        return base;
    }
    let origin_only = base
        .split_once("://")
        .map_or(!base.contains('/'), |(_, rest)| !rest.contains('/'));
    let mut url = if skip_version_prefix || has_version_suffix(&base) || !origin_only {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    };
    while url.contains("/v1/v1") {
        url = url.replace("/v1/v1", "/v1");
    }
    url
}

fn has_version_suffix(base_url: &str) -> bool {
    let segment = base_url.rsplit('/').next().unwrap_or(base_url);
    let Some(rest) = segment.strip_prefix('v') else {
        return false;
    };
    rest.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

pub fn chat_sse_to_responses_sse(input: &str) -> String {
    let mut converter = ChatSseToResponsesConverter::default();
    let mut output = converter.push_bytes(input.as_bytes());
    output.extend(converter.finish());
    String::from_utf8(output).unwrap_or_default()
}

pub fn chat_sse_to_responses_sse_with_request(input: &str, original_request: &Value) -> String {
    let mut converter = ChatSseToResponsesConverter::with_request(original_request);
    let mut output = converter.push_bytes(input.as_bytes());
    output.extend(converter.finish());
    String::from_utf8(output).unwrap_or_default()
}

pub fn anthropic_sse_to_responses_sse(input: &str) -> String {
    let mut converter = AnthropicSseToResponsesConverter::default();
    let mut output = converter.push_bytes(input.as_bytes());
    output.extend(converter.finish());
    String::from_utf8(output).unwrap_or_default()
}

pub fn anthropic_sse_to_responses_sse_with_request(
    input: &str,
    original_request: &Value,
) -> String {
    anthropic_sse_to_responses_sse_with_request_and_diagnostic_id(input, original_request, None)
}

pub fn anthropic_sse_to_responses_sse_with_request_and_diagnostic_id(
    input: &str,
    original_request: &Value,
    diagnostic_id: Option<&str>,
) -> String {
    let mut converter = AnthropicSseToResponsesConverter::with_request_and_diagnostic_id(
        original_request,
        diagnostic_id,
    );
    let mut output = converter.push_bytes(input.as_bytes());
    output.extend(converter.finish());
    String::from_utf8(output).unwrap_or_default()
}

pub fn response_id_from_chat_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("compat");
    if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

fn push_sse(output: &mut String, event: &str, mut data: Value, next_sequence_number: &mut u64) {
    if let Some(object) = data.as_object_mut() {
        object
            .entry("sequence_number".to_string())
            .or_insert_with(|| json!(*next_sequence_number));
        *next_sequence_number += 1;
    }
    output.push_str("event: ");
    output.push_str(event);
    output.push_str("\ndata: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_default());
    output.push_str("\n\n");
}

#[derive(Debug, Default)]
struct TextItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    content_kind: OutputContentKind,
    added: bool,
    done: bool,
}

#[derive(Debug, Default)]
struct ReasoningItemState {
    output_index: Option<u32>,
    item_id: String,
    text: String,
    added: bool,
    done: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum OutputContentKind {
    #[default]
    OutputText,
    Refusal,
}

impl OutputContentKind {
    fn added_part(self) -> Value {
        match self {
            Self::OutputText => json!({ "type": "output_text", "text": "", "annotations": [] }),
            Self::Refusal => json!({ "type": "refusal", "refusal": "" }),
        }
    }

    fn done_part(self, text: &str) -> Value {
        match self {
            Self::OutputText => json!({ "type": "output_text", "text": text, "annotations": [] }),
            Self::Refusal => json!({ "type": "refusal", "refusal": text }),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum InlineThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

#[derive(Debug, Default)]
struct InlineThinkState {
    mode: InlineThinkMode,
    buffer: String,
}

#[derive(Debug, Default)]
struct ToolCallState {
    output_index: Option<u32>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    added: bool,
    done: bool,
}

#[derive(Debug)]
struct ChatSseState {
    response_started: bool,
    completed: bool,
    response_id: String,
    model: String,
    created_at: u64,
    next_output_index: u32,
    next_sequence_number: u64,
    text: TextItemState,
    reasoning: ReasoningItemState,
    inline_think: InlineThinkState,
    tools: BTreeMap<usize, ToolCallState>,
    output_items: Vec<(u32, Value)>,
    latest_usage: Option<Value>,
    finish_reason: Option<String>,
    tool_context: CodexToolContext,
    original_request: Option<Value>,
}

impl Default for ChatSseState {
    fn default() -> Self {
        Self {
            response_started: false,
            completed: false,
            response_id: "resp_compat".to_string(),
            model: String::new(),
            created_at: 0,
            next_output_index: 0,
            next_sequence_number: 0,
            text: TextItemState::default(),
            reasoning: ReasoningItemState::default(),
            inline_think: InlineThinkState::default(),
            tools: BTreeMap::new(),
            output_items: Vec::new(),
            latest_usage: None,
            finish_reason: None,
            tool_context: CodexToolContext::default(),
            original_request: None,
        }
    }
}

impl ChatSseState {
    fn with_request(original_request: &Value) -> Self {
        Self {
            tool_context: build_codex_tool_context_for_request(original_request),
            original_request: Some(original_request.clone()),
            ..Self::default()
        }
    }

    fn handle_chat_chunk_into(&mut self, chunk: &Value, output: &mut String) {
        if let Some(id) = chunk.get("id").and_then(Value::as_str) {
            self.response_id = response_id_from_chat_id(Some(id));
        }
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(created) = chunk.get("created").and_then(Value::as_u64) {
            self.created_at = created;
        }
        self.ensure_response_started_into(output);

        if let Some(usage) = chunk.get("usage").filter(|value| !value.is_null()) {
            self.latest_usage = Some(chat_usage_to_responses_usage(Some(usage)));
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(reasoning) = chat_delta_reasoning_text(delta) {
                self.push_reasoning_delta_into(&reasoning, output);
            }

            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                if !content.is_empty() {
                    self.push_content_delta_into(content, output);
                }
            }

            if let Some(refusal) = delta.get("refusal").and_then(Value::as_str) {
                if !refusal.is_empty() {
                    self.push_refusal_delta_into(refusal, output);
                }
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                self.flush_inline_think_at_boundary_into(output);
                self.finalize_reasoning_into(output);
                for tool_call in tool_calls {
                    self.push_tool_call_delta_into(tool_call, output);
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(finish_reason.to_string());
        }
    }

    fn push_content_delta_into(&mut self, delta: &str, output: &mut String) {
        match self.inline_think.mode {
            InlineThinkMode::Text => {
                self.finalize_reasoning_into(output);
                self.push_message_delta_into(delta, OutputContentKind::OutputText, output);
            }
            InlineThinkMode::Detecting => {
                self.inline_think.buffer.push_str(delta);
                match leading_think_prefix_decision(&self.inline_think.buffer) {
                    ThinkPrefixDecision::NeedMore => {}
                    ThinkPrefixDecision::Reasoning => {
                        self.inline_think.mode = InlineThinkMode::Reasoning;
                        self.drain_complete_inline_think_into(output);
                    }
                    ThinkPrefixDecision::Text => {
                        self.inline_think.mode = InlineThinkMode::Text;
                        let text = std::mem::take(&mut self.inline_think.buffer);
                        self.finalize_reasoning_into(output);
                        self.push_message_delta_into(&text, OutputContentKind::OutputText, output);
                    }
                }
            }
            InlineThinkMode::Reasoning => {
                self.inline_think.buffer.push_str(delta);
                self.drain_complete_inline_think_into(output);
            }
        }
    }

    fn drain_complete_inline_think_into(&mut self, output: &mut String) {
        let Some((reasoning, answer)) = split_leading_think_block(&self.inline_think.buffer) else {
            return;
        };
        self.inline_think.mode = InlineThinkMode::Text;
        self.inline_think.buffer.clear();
        if !reasoning.is_empty() {
            self.push_reasoning_delta_into(&reasoning, output);
            self.finalize_reasoning_into(output);
        }
        if !answer.is_empty() {
            self.push_message_delta_into(&answer, OutputContentKind::OutputText, output);
        }
    }

    fn flush_inline_think_at_boundary_into(&mut self, output: &mut String) {
        match self.inline_think.mode {
            InlineThinkMode::Text => {}
            InlineThinkMode::Detecting => {
                self.inline_think.mode = InlineThinkMode::Text;
                let text = std::mem::take(&mut self.inline_think.buffer);
                if !text.is_empty() {
                    self.finalize_reasoning_into(output);
                    self.push_message_delta_into(&text, OutputContentKind::OutputText, output);
                }
            }
            InlineThinkMode::Reasoning => {
                let buffered = std::mem::take(&mut self.inline_think.buffer);
                self.inline_think.mode = InlineThinkMode::Text;
                if let Some((reasoning, answer)) = split_leading_think_block(&buffered) {
                    if !reasoning.is_empty() {
                        self.push_reasoning_delta_into(&reasoning, output);
                        self.finalize_reasoning_into(output);
                    }
                    if !answer.is_empty() {
                        self.push_message_delta_into(
                            &answer,
                            OutputContentKind::OutputText,
                            output,
                        );
                    }
                    return;
                }
                let reasoning = strip_leading_think_open_tag(&buffered).unwrap_or(buffered);
                if !reasoning.is_empty() {
                    self.push_reasoning_delta_into(&reasoning, output);
                    self.finalize_reasoning_into(output);
                }
            }
        }
    }

    fn push_refusal_delta_into(&mut self, delta: &str, output: &mut String) {
        self.flush_inline_think_at_boundary_into(output);
        self.finalize_reasoning_into(output);
        self.push_message_delta_into(delta, OutputContentKind::Refusal, output);
    }

    fn ensure_response_started_into(&mut self, output: &mut String) {
        if self.response_started {
            return;
        }
        self.response_started = true;
        push_sse(
            output,
            "response.created",
            json!({
                "type": "response.created",
                "response": self.base_response("in_progress", Vec::new())
            }),
            &mut self.next_sequence_number,
        );
        push_sse(
            output,
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "response": self.base_response("in_progress", Vec::new())
            }),
            &mut self.next_sequence_number,
        );
    }

    fn push_reasoning_delta_into(&mut self, delta: &str, output: &mut String) {
        if !self.reasoning.added {
            let output_index = self.next_output_index();
            let item_id = format!("rs_{}", self.response_id);
            self.reasoning.output_index = Some(output_index);
            self.reasoning.item_id = item_id.clone();
            self.reasoning.added = true;

            push_sse(
                output,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": item_id,
                        "type": "reasoning",
                        "status": "in_progress",
                        "reasoning_content": "",
                        "summary": []
                    }
                }),
                &mut self.next_sequence_number,
            );
            push_sse(
                output,
                "response.reasoning_summary_part.added",
                json!({
                    "type": "response.reasoning_summary_part.added",
                    "item_id": self.reasoning.item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": "" }
                }),
                &mut self.next_sequence_number,
            );
        }

        self.reasoning.text.push_str(delta);
        let output_index = self.reasoning.output_index.unwrap_or(0);
        push_sse(
            output,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": self.reasoning.item_id,
                "output_index": output_index,
                "summary_index": 0,
                "delta": delta
            }),
            &mut self.next_sequence_number,
        );
    }

    fn push_message_delta_into(
        &mut self,
        delta: &str,
        content_kind: OutputContentKind,
        output: &mut String,
    ) {
        if self.text.added && !self.text.done && self.text.content_kind != content_kind {
            self.finalize_text_into(output);
            self.text = TextItemState::default();
        }
        if !self.text.added {
            let output_index = self.next_output_index();
            let item_id = format!("{}_msg", self.response_id);
            self.text.output_index = Some(output_index);
            self.text.item_id = item_id.clone();
            self.text.content_kind = content_kind;
            self.text.added = true;
            push_sse(
                output,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": item_id,
                        "type": "message",
                        "status": "in_progress",
                        "role": "assistant",
                        "content": []
                    }
                }),
                &mut self.next_sequence_number,
            );
            push_sse(
                output,
                "response.content_part.added",
                json!({
                    "type": "response.content_part.added",
                    "item_id": self.text.item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": content_kind.added_part()
                }),
                &mut self.next_sequence_number,
            );
        }

        self.text.text.push_str(delta);
        let output_index = self.text.output_index.unwrap_or(0);
        let (event, event_type) = match self.text.content_kind {
            OutputContentKind::OutputText => {
                ("response.output_text.delta", "response.output_text.delta")
            }
            OutputContentKind::Refusal => ("response.refusal.delta", "response.refusal.delta"),
        };
        push_sse(
            output,
            event,
            json!({
                "type": event_type,
                "item_id": self.text.item_id,
                "output_index": output_index,
                "content_index": 0,
                "delta": delta
            }),
            &mut self.next_sequence_number,
        );
    }

    fn push_tool_call_delta_into(&mut self, tool_call: &Value, output: &mut String) {
        let chat_index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let id_delta = tool_call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let function = tool_call.get("function").unwrap_or(&Value::Null);
        let name_delta = function
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string);
        let args_delta = function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut should_add = false;
        let mut output_index = None;
        let mut item_id = String::new();
        let mut pending_arguments = String::new();

        {
            let state = self.tools.entry(chat_index).or_default();
            if let Some(id) = id_delta {
                state.call_id = id;
            }
            if let Some(name) = name_delta {
                if !name.is_empty() {
                    state.name = name;
                }
            }
            if !args_delta.is_empty() {
                state.arguments.push_str(&args_delta);
            }

            if !state.added && (!state.call_id.is_empty() || !state.name.is_empty()) {
                should_add = true;
                pending_arguments = state.arguments.clone();
            } else if state.added {
                output_index = state.output_index;
                item_id = state.item_id.clone();
            }
        }

        if should_add {
            let assigned = self.next_output_index();
            let state = self.tools.get_mut(&chat_index).expect("tool state exists");
            state.added = true;
            if state.call_id.is_empty() {
                state.call_id = format!("call_{chat_index}");
            }
            if state.name.is_empty() {
                state.name = "unknown_tool".to_string();
            }
            state.output_index = Some(assigned);
            state.item_id = format!("fc_{}", state.call_id);
            let added_item = tool_call_added_item(state, assigned, &self.tool_context);
            push_sse(
                output,
                "response.output_item.added",
                added_item,
                &mut self.next_sequence_number,
            );
            if !pending_arguments.is_empty() {
                push_tool_call_delta_sse(
                    output,
                    state,
                    assigned,
                    &pending_arguments,
                    &self.tool_context,
                    &mut self.next_sequence_number,
                );
            }
        } else if !args_delta.is_empty() {
            if let Some(output_index) = output_index {
                let state = ToolCallState {
                    output_index: Some(output_index),
                    item_id,
                    name: self
                        .tools
                        .get(&chat_index)
                        .map(|state| state.name.clone())
                        .unwrap_or_default(),
                    call_id: self
                        .tools
                        .get(&chat_index)
                        .map(|state| state.call_id.clone())
                        .unwrap_or_default(),
                    ..ToolCallState::default()
                };
                push_tool_call_delta_sse(
                    output,
                    &state,
                    output_index,
                    &args_delta,
                    &self.tool_context,
                    &mut self.next_sequence_number,
                );
            }
        }
    }

    fn finalize_into(&mut self, output: &mut String) {
        if self.completed {
            return;
        }
        self.ensure_response_started_into(output);
        self.flush_inline_think_at_boundary_into(output);
        self.finalize_reasoning_into(output);
        self.finalize_text_into(output);
        self.finalize_tools_into(output);

        let status = response_status(self.finish_reason.as_deref());
        let mut response = self.base_response(status, self.completed_output_items());
        if status == "incomplete" {
            response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
        }
        copy_response_request_fields(&mut response, self.original_request.as_ref());
        let terminal_event = if status == "incomplete" {
            "response.incomplete"
        } else {
            "response.completed"
        };
        push_sse(
            output,
            terminal_event,
            json!({
                "type": terminal_event,
                "response": response
            }),
            &mut self.next_sequence_number,
        );
        output.push_str("data: [DONE]\n\n");
        self.completed = true;
    }

    fn finalize_reasoning_into(&mut self, output: &mut String) {
        if !self.reasoning.added || self.reasoning.done {
            return;
        }
        let output_index = self.reasoning.output_index.unwrap_or(0);
        let item = json!({
            "id": self.reasoning.item_id,
            "type": "reasoning",
            "reasoning_content": self.reasoning.text,
            "summary": [{ "type": "summary_text", "text": self.reasoning.text }]
        });
        self.output_items.push((output_index, item.clone()));
        self.reasoning.done = true;
        push_sse(
            output,
            "response.reasoning_summary_text.done",
            json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": self.reasoning.item_id,
                "output_index": output_index,
                "summary_index": 0,
                "text": self.reasoning.text
            }),
            &mut self.next_sequence_number,
        );
        push_sse(
            output,
            "response.reasoning_summary_part.done",
            json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": self.reasoning.item_id,
                "output_index": output_index,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": self.reasoning.text }
            }),
            &mut self.next_sequence_number,
        );
        push_sse(
            output,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item
            }),
            &mut self.next_sequence_number,
        );
    }

    fn finalize_text_into(&mut self, output: &mut String) {
        if !self.text.added || self.text.done {
            return;
        }
        let output_index = self.text.output_index.unwrap_or(0);
        let item = json!({
            "id": self.text.item_id,
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [self.text.content_kind.done_part(&self.text.text)]
        });
        self.output_items.push((output_index, item.clone()));
        self.text.done = true;
        let (done_event, done_type, done_value_key) = match self.text.content_kind {
            OutputContentKind::OutputText => (
                "response.output_text.done",
                "response.output_text.done",
                "text",
            ),
            OutputContentKind::Refusal => {
                ("response.refusal.done", "response.refusal.done", "refusal")
            }
        };
        let mut done_payload = json!({
            "type": done_type,
            "item_id": self.text.item_id,
            "output_index": output_index,
            "content_index": 0
        });
        done_payload[done_value_key] = json!(self.text.text);
        push_sse(
            output,
            done_event,
            done_payload,
            &mut self.next_sequence_number,
        );
        push_sse(
            output,
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "item_id": self.text.item_id,
                "output_index": output_index,
                "content_index": 0,
                "part": self.text.content_kind.done_part(&self.text.text)
            }),
            &mut self.next_sequence_number,
        );
        push_sse(
            output,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item
            }),
            &mut self.next_sequence_number,
        );
    }

    fn finalize_tools_into(&mut self, output: &mut String) {
        let keys: Vec<usize> = self.tools.keys().copied().collect();
        for key in keys {
            if self.tools.get(&key).map(|state| state.done).unwrap_or(true) {
                continue;
            }
            if self
                .tools
                .get(&key)
                .map(|state| !state.added && !state.done)
                .unwrap_or(false)
            {
                let assigned = self.next_output_index();
                let state = self.tools.get_mut(&key).expect("tool state exists");
                state.added = true;
                if state.call_id.is_empty() {
                    state.call_id = format!("call_{key}");
                }
                if state.name.is_empty() {
                    state.name = "unknown_tool".to_string();
                }
                state.output_index = Some(assigned);
                state.item_id = format!("fc_{}", state.call_id);
                let added_item = tool_call_added_item(state, assigned, &self.tool_context);
                push_sse(
                    output,
                    "response.output_item.added",
                    added_item,
                    &mut self.next_sequence_number,
                );
            }

            let state = self.tools.get_mut(&key).expect("tool state exists");
            let output_index = state.output_index.unwrap_or(0);
            let item = tool_call_done_item(state, &self.tool_context);
            state.done = true;
            self.output_items.push((output_index, item.clone()));
            push_tool_call_done_sse(
                output,
                state,
                output_index,
                &self.tool_context,
                &mut self.next_sequence_number,
            );
            push_sse(
                output,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": output_index,
                    "item": item
                }),
                &mut self.next_sequence_number,
            );
        }
    }

    fn failed_into(&mut self, output: &mut String, message: String, error_type: Option<String>) {
        self.completed = true;
        let mut error = json!({ "message": message });
        if let Some(error_type) = error_type.filter(|value| !value.is_empty()) {
            error["type"] = json!(error_type);
        }
        let mut response = self.base_response("failed", self.completed_output_items());
        response["error"] = error;
        push_sse(
            output,
            "response.failed",
            json!({
                "type": "response.failed",
                "response": response
            }),
            &mut self.next_sequence_number,
        );
    }

    fn completed_output_items(&self) -> Vec<Value> {
        let mut output_items = self.output_items.clone();
        output_items.sort_by_key(|(output_index, _)| *output_index);
        output_items.into_iter().map(|(_, item)| item).collect()
    }

    fn base_response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": self.latest_usage.clone().unwrap_or_else(default_responses_usage)
        })
    }

    fn next_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }
}

#[derive(Debug, Clone, Copy)]
enum AnthropicBlockKind {
    Text,
    Thinking,
    Tool,
}

#[derive(Debug, Default)]
struct AnthropicSseState {
    inner: ChatSseState,
    diagnostic_id: Option<String>,
    blocks: BTreeMap<usize, AnthropicBlockKind>,
    text_buffers: BTreeMap<usize, String>,
    usage: Value,
    text_block_count: usize,
    thinking_block_count: usize,
    native_tool_use_block_count: usize,
    other_block_count: usize,
    textual_invoke_block_count: usize,
    textual_invoke_call_count: usize,
    native_tool_names: BTreeSet<String>,
    textual_invoke_tool_names: BTreeSet<String>,
    pending_text_marker: Option<String>,
}

impl AnthropicSseState {
    fn with_request_and_diagnostic_id(
        original_request: &Value,
        diagnostic_id: Option<&str>,
    ) -> Self {
        Self {
            inner: ChatSseState::with_request(original_request),
            diagnostic_id: diagnostic_id.map(ToString::to_string),
            blocks: BTreeMap::new(),
            text_buffers: BTreeMap::new(),
            usage: json!({}),
            text_block_count: 0,
            thinking_block_count: 0,
            native_tool_use_block_count: 0,
            other_block_count: 0,
            textual_invoke_block_count: 0,
            textual_invoke_call_count: 0,
            native_tool_names: BTreeSet::new(),
            textual_invoke_tool_names: BTreeSet::new(),
            pending_text_marker: None,
        }
    }

    fn handle_anthropic_event_into(&mut self, event: &str, chunk: &Value, output: &mut String) {
        match event {
            "message_start" => self.handle_message_start_into(chunk, output),
            "content_block_start" => self.handle_content_block_start_into(chunk, output),
            "content_block_delta" => self.handle_content_block_delta_into(chunk, output),
            "content_block_stop" => self.handle_content_block_stop(chunk, output),
            "message_delta" => self.handle_message_delta_into(chunk),
            "message_stop" => self.handle_message_stop_into(output),
            _ => {}
        }
    }

    fn handle_message_start_into(&mut self, chunk: &Value, output: &mut String) {
        let message = chunk.get("message").unwrap_or(chunk);
        if let Some(id) = message.get("id").and_then(Value::as_str) {
            self.inner.response_id = response_id_from_chat_id(Some(id));
        }
        if let Some(model) = message.get("model").and_then(Value::as_str) {
            if !model.is_empty() {
                self.inner.model = model.to_string();
            }
        }
        self.merge_usage(message.get("usage"));
        self.inner.ensure_response_started_into(output);
    }

    fn handle_content_block_start_into(&mut self, chunk: &Value, output: &mut String) {
        let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let block = chunk.get("content_block").unwrap_or(&Value::Null);
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        if block_type == "tool_use" {
            self.pending_text_marker.take();
        } else {
            // Anthropic 每个 text 块都会先发 content_block_start；这里先冲刷旧 marker，避免顺序落到新正文之后。
            self.flush_pending_text_marker_into(output);
        }
        match block_type {
            "text" => {
                self.text_block_count += 1;
                self.blocks.insert(index, AnthropicBlockKind::Text);
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        self.handle_text_delta_into(index, text, output);
                    }
                }
            }
            "thinking" => {
                self.thinking_block_count += 1;
                self.blocks.insert(index, AnthropicBlockKind::Thinking);
                if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                    if !thinking.is_empty() {
                        self.inner.push_reasoning_delta_into(thinking, output);
                    }
                }
            }
            "tool_use" => {
                self.native_tool_use_block_count += 1;
                self.blocks.insert(index, AnthropicBlockKind::Tool);
                self.inner.flush_inline_think_at_boundary_into(output);
                self.inner.finalize_reasoning_into(output);
                if let Some(name) = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                {
                    self.native_tool_names.insert(name.to_string());
                }
                let fake = json!({
                    "index": index,
                    "id": block.get("id").and_then(Value::as_str).unwrap_or(""),
                    "function": {
                        "name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                        "arguments": ""
                    }
                });
                self.inner.push_tool_call_delta_into(&fake, output);
                if let Some(input) = block.get("input").filter(|value| {
                    value
                        .as_object()
                        .map(|object| !object.is_empty())
                        .unwrap_or(false)
                }) {
                    let fake = json!({
                        "index": index,
                        "function": {
                            "arguments": canonical_json_string(input)
                        }
                    });
                    self.inner.push_tool_call_delta_into(&fake, output);
                }
            }
            _ => {
                self.other_block_count += 1;
            }
        }
    }

    fn handle_content_block_delta_into(&mut self, chunk: &Value, output: &mut String) {
        let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let delta = chunk.get("delta").unwrap_or(&Value::Null);
        match delta.get("type").and_then(Value::as_str).unwrap_or("") {
            "text_delta" => {
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        self.handle_text_delta_into(index, text, output);
                    }
                }
            }
            "thinking_delta" => {
                if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                    if !thinking.is_empty() {
                        self.inner.push_reasoning_delta_into(thinking, output);
                    }
                }
            }
            "input_json_delta" => {
                self.inner.flush_inline_think_at_boundary_into(output);
                self.inner.finalize_reasoning_into(output);
                let partial = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !partial.is_empty() {
                    let fake = json!({
                        "index": index,
                        "function": {
                            "arguments": partial
                        }
                    });
                    self.inner.push_tool_call_delta_into(&fake, output);
                }
            }
            _ => {}
        }
    }

    fn handle_content_block_stop(&mut self, chunk: &Value, output: &mut String) {
        let index = chunk.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let kind = self.blocks.remove(&index);
        if matches!(kind, Some(AnthropicBlockKind::Text)) {
            if let Some(text) = self.text_buffers.remove(&index) {
                self.flush_buffered_text_block_into(index, &text, output);
            }
        }
    }

    fn handle_text_delta_into(&mut self, index: usize, text: &str, output: &mut String) {
        let buffer = self.text_buffers.entry(index).or_default();
        buffer.push_str(text);
        // 缓冲区已出现 `<invoke`：进入工具调用区，不再透传，等 block 结束后统一切分。
        if buffer.contains("<invoke") {
            return;
        }

        // 尚未出现 `<invoke`：把「绝不可能属于工具调用起点」的前缀作为正文透传，
        // 尾部可能是起点开头的一小段留在缓冲区继续等待。
        let safe_len = textual_invoke_safe_passthrough_len(buffer);
        if safe_len == 0 {
            return;
        }
        let passthrough: String = buffer.drain(..safe_len).collect();
        if buffer.is_empty() {
            self.text_buffers.remove(&index);
        }
        self.inner.push_content_delta_into(&passthrough, output);
    }

    fn flush_buffered_text_block_into(&mut self, index: usize, text: &str, output: &mut String) {
        let marker_kinds = text_tool_marker_kinds(text);
        let split = split_text_into_message_and_tool_calls(text);
        if !marker_kinds.is_empty() {
            log_anthropic_text_tool_marker_detected(
                true,
                &self.inner.model,
                Some(index),
                marker_kinds,
                split.is_some(),
                text.len(),
                text,
                self.diagnostic_id.as_deref(),
            );
        }
        let Some((leading, calls)) = split else {
            if !text.is_empty() {
                if is_standalone_textual_tool_marker(text) {
                    self.flush_pending_text_marker_into(output);
                    self.pending_text_marker = Some(text.to_string());
                } else {
                    self.inner.push_content_delta_into(text, output);
                }
            }
            return;
        };

        // 先输出工具调用前的正文，再把工具调用转为 tool_call delta。
        if !leading.is_empty() {
            self.inner.push_content_delta_into(&leading, output);
        }

        self.textual_invoke_block_count += 1;
        self.textual_invoke_call_count += calls.len();
        for call in &calls {
            self.textual_invoke_tool_names.insert(call.name.clone());
        }
        log_anthropic_textual_invoke_detected(
            true,
            &self.inner.model,
            Some(index),
            calls.len(),
            calls.iter().map(|call| call.name.clone()).collect(),
            text.len(),
            self.diagnostic_id.as_deref(),
        );

        self.inner.flush_inline_think_at_boundary_into(output);
        self.inner.finalize_reasoning_into(output);
        for (offset, call) in calls.into_iter().enumerate() {
            let stream_index = index.saturating_mul(1000).saturating_add(offset);
            let fake = json!({
                "index": stream_index,
                "id": format!("call_textual_invoke_{stream_index}"),
                "function": {
                    "name": call.name,
                    "arguments": canonical_json_string(&Value::Object(call.arguments))
                }
            });
            self.inner.push_tool_call_delta_into(&fake, output);
        }
    }

    fn flush_pending_text_marker_into(&mut self, output: &mut String) {
        if let Some(text) = self.pending_text_marker.take() {
            self.inner.push_content_delta_into(&text, output);
        }
    }

    fn handle_message_delta_into(&mut self, chunk: &Value) {
        if let Some(stop_reason) = chunk.pointer("/delta/stop_reason").and_then(Value::as_str) {
            self.inner.finish_reason =
                anthropic_stop_reason_to_chat_finish_reason(Some(stop_reason)).map(str::to_string);
        }
        self.merge_usage(chunk.get("usage"));
    }

    fn handle_message_stop_into(&mut self, output: &mut String) {
        self.flush_pending_text_marker_into(output);
        log_anthropic_stream_response_shape(self);
        self.inner.finalize_into(output);
    }

    fn merge_usage(&mut self, usage: Option<&Value>) {
        let Some(usage) = usage.and_then(Value::as_object) else {
            return;
        };
        if !self.usage.is_object() {
            self.usage = json!({});
        }
        let Some(target) = self.usage.as_object_mut() else {
            return;
        };
        for (key, value) in usage {
            target.insert(key.clone(), value.clone());
        }
        self.inner.latest_usage = Some(anthropic_usage_to_responses_usage(Some(&self.usage)));
    }
}

fn take_sse_block(buffer: &mut String) -> Option<String> {
    let lf = buffer.find("\n\n").map(|index| (index, 2));
    let crlf = buffer.find("\r\n\r\n").map(|index| (index, 4));
    let (index, delimiter_len) = match (lf, crlf) {
        (Some(left), Some(right)) => {
            if left.0 <= right.0 {
                left
            } else {
                right
            }
        }
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => return None,
    };
    let block = buffer[..index].to_string();
    buffer.drain(..index + delimiter_len);
    Some(block)
}

fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut combined = Vec::new();
    if !remainder.is_empty() {
        combined.extend_from_slice(remainder);
        remainder.clear();
    }
    combined.extend_from_slice(bytes);

    match std::str::from_utf8(&combined) {
        Ok(text) => buffer.push_str(text),
        Err(error) => {
            let valid = error.valid_up_to();
            if valid > 0 {
                buffer.push_str(std::str::from_utf8(&combined[..valid]).unwrap_or_default());
            }
            if error.error_len().is_none() {
                remainder.extend_from_slice(&combined[valid..]);
            } else {
                buffer.push_str(&String::from_utf8_lossy(&combined[valid..]));
            }
        }
    }
}

fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(field)?.strip_prefix(':')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn chat_delta_reasoning_text(delta: &Value) -> Option<String> {
    extract_reasoning_field_text(delta)
}

enum ThinkPrefixDecision {
    NeedMore,
    Reasoning,
    Text,
}

fn leading_think_prefix_decision(buffer: &str) -> ThinkPrefixDecision {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return ThinkPrefixDecision::NeedMore;
    }
    if trimmed.starts_with(THINK_OPEN_TAG) {
        return ThinkPrefixDecision::Reasoning;
    }
    if THINK_OPEN_TAG.starts_with(trimmed) {
        return ThinkPrefixDecision::NeedMore;
    }
    ThinkPrefixDecision::Text
}

fn extract_chat_sse_error(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    (message, error_type)
}

fn http_status_line(status: u16) -> String {
    match status {
        200 => "200 OK".to_string(),
        400 => "400 Bad Request".to_string(),
        401 => "401 Unauthorized".to_string(),
        403 => "403 Forbidden".to_string(),
        404 => "404 Not Found".to_string(),
        429 => "429 Too Many Requests".to_string(),
        500 => "500 Internal Server Error".to_string(),
        502 => "502 Bad Gateway".to_string(),
        503 => "503 Service Unavailable".to_string(),
        _ => format!("{status} Upstream"),
    }
}

pub fn responses_error_from_upstream(status_code: u16, content_type: &str, body: &[u8]) -> Value {
    let (message, error_type, code, param) = upstream_error_parts(status_code, content_type, body);
    let mut error = json!({
        "message": message,
        "type": error_type.unwrap_or_else(|| "upstream_error".to_string()),
    });
    if let Some(code) = code {
        error["code"] = json!(code);
    }
    if let Some(param) = param {
        error["param"] = json!(param);
    }
    json!({ "error": error })
}

fn upstream_error_parts(
    status_code: u16,
    content_type: &str,
    body: &[u8],
) -> (String, Option<String>, Option<String>, Option<String>) {
    if content_type.to_ascii_lowercase().contains("json") {
        if let Ok(value) = serde_json::from_slice::<Value>(body) {
            let error = value.get("error").unwrap_or(&value);
            let message = error
                .get("message")
                .or_else(|| error.get("detail"))
                .or_else(|| error.get("error"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| truncate_error_preview(&value.to_string()));
            let error_type = error
                .get("type")
                .or_else(|| error.get("error_type"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let code = error.get("code").and_then(|value| {
                value
                    .as_str()
                    .map(ToString::to_string)
                    .or_else(|| value.as_i64().map(|number| number.to_string()))
            });
            let param = error
                .get("param")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            return (message, error_type, code, param);
        }
    }

    let preview = truncate_error_preview(&String::from_utf8_lossy(body));
    let message = if preview.trim().is_empty() {
        format!("Upstream returned HTTP {status_code}")
    } else {
        preview
    };
    (message, None, Some(status_code.to_string()), None)
}

fn truncate_error_preview(input: &str) -> String {
    input.chars().take(ERROR_BODY_PREVIEW_LIMIT).collect()
}

fn append_responses_input(input: &Value, messages: &mut Vec<Value>) {
    match input {
        Value::String(text) => messages.push(json!({ "role": "user", "content": text })),
        Value::Array(items) => {
            let mut pending_tool_calls = Vec::new();
            let mut pending_reasoning = Vec::new();
            let mut seen_tool_call_ids = BTreeSet::new();
            let mut ignored_tool_call_ids = BTreeSet::new();
            for item in items {
                append_responses_item(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_reasoning,
                    &mut seen_tool_call_ids,
                    &mut ignored_tool_call_ids,
                );
            }
            flush_tool_calls(messages, &mut pending_tool_calls, &mut pending_reasoning);
            flush_reasoning(messages, &mut pending_reasoning);
        }
        Value::Object(_) => {
            let mut pending_tool_calls = Vec::new();
            let mut pending_reasoning = Vec::new();
            let mut seen_tool_call_ids = BTreeSet::new();
            let mut ignored_tool_call_ids = BTreeSet::new();
            append_responses_item(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
                &mut seen_tool_call_ids,
                &mut ignored_tool_call_ids,
            );
            flush_tool_calls(messages, &mut pending_tool_calls, &mut pending_reasoning);
            flush_reasoning(messages, &mut pending_reasoning);
        }
        _ => {}
    }
}

fn append_responses_input_to_anthropic(
    input: &Value,
    messages: &mut Vec<Value>,
    system_chunks: &mut Vec<String>,
) {
    match input {
        Value::String(text) => push_anthropic_message(
            messages,
            "user",
            vec![json!({ "type": "text", "text": text })],
        ),
        Value::Array(items) => {
            let mut ignored_tool_call_ids = BTreeSet::new();
            for item in items {
                append_responses_item_to_anthropic(
                    item,
                    messages,
                    system_chunks,
                    &mut ignored_tool_call_ids,
                );
            }
        }
        Value::Object(_) => {
            let mut ignored_tool_call_ids = BTreeSet::new();
            append_responses_item_to_anthropic(
                input,
                messages,
                system_chunks,
                &mut ignored_tool_call_ids,
            );
        }
        _ => {}
    }
}

fn append_responses_item_to_anthropic(
    item: &Value,
    messages: &mut Vec<Value>,
    system_chunks: &mut Vec<String>,
    ignored_tool_call_ids: &mut BTreeSet<String>,
) {
    match item.get("type").and_then(Value::as_str) {
        Some("function_call") => {
            let name = responses_history_function_name(item);
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !call_id.is_empty() && is_filtered_server_side_tool_type(&name) {
                ignored_tool_call_ids.insert(call_id.to_string());
                return;
            }
            if !name.is_empty() && !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": anthropic_tool_input_from_arguments(item.get("arguments"))
                    })],
                );
            }
        }
        Some("function_call_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            if !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": response_output_text(item.get("output").unwrap_or(&Value::Null))
                    })],
                );
            }
        }
        Some("custom_tool_call") => {
            let raw_name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let input = item
                .get("input")
                .or_else(|| item.get("arguments"))
                .unwrap_or(&Value::Null);
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !call_id.is_empty() && is_filtered_server_side_tool_type(raw_name) {
                ignored_tool_call_ids.insert(call_id.to_string());
                return;
            }
            let (name, arguments) = build_custom_tool_call_history(raw_name, input);
            if !name.is_empty() && !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": anthropic_tool_input_from_argument_string(&arguments)
                    })],
                );
            }
        }
        Some("custom_tool_call_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            if !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": response_output_text(item.get("output").unwrap_or(&Value::Null))
                    })],
                );
            }
        }
        Some("tool_search_call") => {
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": "tool_search",
                        "input": tool_search_arguments_from_value(item.get("arguments"))
                    })],
                );
            }
        }
        Some("tool_search_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if !call_id.is_empty() {
                push_anthropic_message(
                    messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": tool_search_output_text(item)
                    })],
                );
            }
        }
        Some("tool_call") => {
            let Some(tool_use) = item.get("tool_use") else {
                return;
            };
            let call_id = tool_use
                .get("id")
                .or_else(|| item.get("call_id"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = tool_use.get("name").and_then(Value::as_str).unwrap_or("");
            if !call_id.is_empty() && is_filtered_server_side_tool_type(name) {
                ignored_tool_call_ids.insert(call_id.to_string());
                return;
            }
            if !call_id.is_empty() && !name.is_empty() {
                push_anthropic_message(
                    messages,
                    "assistant",
                    vec![json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": anthropic_tool_input_from_arguments(tool_use.get("input"))
                    })],
                );
            }
        }
        Some("tool_result") => {
            let content = item.get("content").unwrap_or(&Value::Null);
            let call_id = content
                .get("tool_use_id")
                .or_else(|| item.get("tool_call_id"))
                .or_else(|| item.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            if !call_id.is_empty() {
                let output = content.get("content").unwrap_or(content);
                push_anthropic_message(
                    messages,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": response_output_text(output)
                    })],
                );
            }
        }
        Some("reasoning") => {
            if let Some(text) = responses_reasoning_text(item) {
                if !text.is_empty() {
                    push_anthropic_message(
                        messages,
                        "assistant",
                        vec![json!({ "type": "thinking", "thinking": text })],
                    );
                }
            }
        }
        _ => {
            if item.get("role").is_none() && item.get("content").is_none() {
                return;
            }
            let role = item.get("role").and_then(Value::as_str);
            let anthropic_role = responses_role_to_anthropic_role(role);
            let content = item.get("content").unwrap_or(&Value::Null);
            if anthropic_role == "system" {
                let text = responses_content_to_anthropic_system_text(content);
                if !text.is_empty() {
                    system_chunks.push(text);
                }
                return;
            }
            let content = responses_content_to_anthropic_content(content);
            push_anthropic_message(messages, anthropic_role, content);
        }
    }
}

fn responses_role_to_anthropic_role(role: Option<&str>) -> &'static str {
    match role {
        Some("developer") | Some("system") => "system",
        Some("assistant") => "assistant",
        Some("user") | Some("latest_reminder") | Some("tool") | None => "user",
        Some(_) => "user",
    }
}

fn push_anthropic_message(messages: &mut Vec<Value>, role: &str, content: Vec<Value>) {
    if content.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some(role) {
            if let Some(existing) = last.get_mut("content").and_then(Value::as_array_mut) {
                existing.extend(content);
                return;
            }
        }
    }
    messages.push(json!({ "role": role, "content": content }));
}

fn responses_content_to_anthropic_content(content: &Value) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({ "type": "text", "text": text })],
        Value::Array(parts) => {
            let mut converted = Vec::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            converted.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    "refusal" => {
                        if let Some(text) = part.get("refusal").and_then(Value::as_str) {
                            converted.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    "input_image" => {
                        if let Some(source) = anthropic_image_source(part) {
                            converted.push(json!({ "type": "image", "source": source }));
                        }
                    }
                    _ => converted.push(json!({
                        "type": "text",
                        "text": canonical_json_string(part)
                    })),
                }
            }
            converted
        }
        Value::Null => Vec::new(),
        other => vec![json!({ "type": "text", "text": response_output_text(other) })],
    }
}

fn responses_content_to_anthropic_system_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(
                |part| match part.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_text" | "output_text" | "text" => {
                        part.get("text").and_then(Value::as_str).map(str::to_string)
                    }
                    "refusal" => part
                        .get("refusal")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    _ => Some(canonical_json_string(part)),
                },
            )
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => response_output_text(other),
    }
}

fn anthropic_image_source(part: &Value) -> Option<Value> {
    let image_url = part.get("image_url")?;
    let url = image_url
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| image_url.as_str())?;
    if let Some(source) = data_url_to_anthropic_source(url) {
        return Some(source);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(json!({ "type": "url", "url": url }));
    }
    None
}

fn data_url_to_anthropic_source(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("data:")?;
    let (media_type, data) = rest.split_once(";base64,")?;
    if media_type.is_empty() || data.is_empty() {
        return None;
    }
    Some(json!({
        "type": "base64",
        "media_type": media_type,
        "data": data
    }))
}

fn anthropic_tool_input_from_arguments(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return json!({});
    };
    match value {
        Value::String(text) => anthropic_tool_input_from_argument_string(text),
        Value::Object(_) => value.clone(),
        Value::Null => json!({}),
        other => json!({ "input": other.clone() }),
    }
}

fn anthropic_tool_input_from_argument_string(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(value) => json!({ "input": value }),
        Err(_) => json!({ "input": arguments }),
    }
}

fn append_responses_item(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Vec<String>,
    seen_tool_call_ids: &mut BTreeSet<String>,
    ignored_tool_call_ids: &mut BTreeSet<String>,
) {
    match item.get("type").and_then(Value::as_str) {
        Some("function_call") => {
            let name = responses_history_function_name(item);
            if name.is_empty() {
                return;
            }
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if is_filtered_server_side_tool_type(&name) {
                ignored_tool_call_ids.insert(call_id.to_string());
                return;
            }
            seen_tool_call_ids.insert(call_id.to_string());
            pending_tool_calls.push(json!({
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": responses_arguments_to_chat(item.get("arguments").unwrap_or(&json!({})))
                }
            }));
        }
        Some("function_call_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            if !seen_tool_call_ids.contains(call_id) {
                flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
                flush_reasoning(messages, pending_reasoning);
                messages.push(orphan_tool_output_message(
                    call_id,
                    item.get("output").unwrap_or(&Value::Null),
                ));
                return;
            }
            flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": response_output_text(item.get("output").unwrap_or(&Value::Null))
            }));
        }
        Some("custom_tool_call") => {
            let raw_name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let input = item
                .get("input")
                .or_else(|| item.get("arguments"))
                .unwrap_or(&Value::Null);
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if is_filtered_server_side_tool_type(raw_name) {
                ignored_tool_call_ids.insert(call_id.to_string());
                return;
            }
            let (name, arguments) = build_custom_tool_call_history(raw_name, input);
            seen_tool_call_ids.insert(call_id.to_string());
            pending_tool_calls.push(json!({
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            }));
        }
        Some("custom_tool_call_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            if !seen_tool_call_ids.contains(call_id) {
                flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
                flush_reasoning(messages, pending_reasoning);
                messages.push(orphan_tool_output_message(
                    call_id,
                    item.get("output").unwrap_or(&Value::Null),
                ));
                return;
            }
            flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": response_output_text(item.get("output").unwrap_or(&Value::Null))
            }));
        }
        Some("tool_search_call") => {
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            seen_tool_call_ids.insert(call_id.to_string());
            pending_tool_calls.push(json!({
                "id": call_id,
                "type": "function",
                "function": {
                    "name": "tool_search",
                    "arguments": canonical_json_string(&tool_search_arguments_from_value(item.get("arguments")))
                }
            }));
        }
        Some("tool_search_output") => {
            let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if !seen_tool_call_ids.contains(call_id) {
                flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
                flush_reasoning(messages, pending_reasoning);
                messages.push(orphan_tool_output_message(call_id, item));
                return;
            }
            flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": tool_search_output_text(item)
            }));
        }
        Some("tool_call") => {
            if let Some(tool_use) = item.get("tool_use") {
                let call_id = tool_use
                    .get("id")
                    .or_else(|| item.get("call_id"))
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if call_id.is_empty() {
                    return;
                }
                let name = tool_use.get("name").and_then(Value::as_str).unwrap_or("");
                if is_filtered_server_side_tool_type(name) {
                    ignored_tool_call_ids.insert(call_id.to_string());
                    return;
                }
                seen_tool_call_ids.insert(call_id.to_string());
                pending_tool_calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": responses_arguments_to_chat(tool_use.get("input").unwrap_or(&json!({})))
                    }
                }));
            }
        }
        Some("tool_result") => {
            flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
            let content = item.get("content").unwrap_or(&Value::Null);
            let call_id = content
                .get("tool_use_id")
                .or_else(|| item.get("tool_call_id"))
                .or_else(|| item.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if call_id.is_empty() {
                return;
            }
            if ignored_tool_call_ids.contains(call_id) {
                return;
            }
            let output = content.get("content").unwrap_or(content);
            if !seen_tool_call_ids.contains(call_id) {
                flush_reasoning(messages, pending_reasoning);
                messages.push(orphan_tool_output_message(call_id, output));
                return;
            }
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": response_output_text(output)
            }));
        }
        Some("reasoning") => {
            if let Some(text) = responses_reasoning_text(item) {
                if !text.is_empty() {
                    pending_reasoning.push(text);
                }
            }
        }
        _ => {
            flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
            if item.get("role").is_some() || item.get("content").is_some() {
                let role = responses_role_to_chat_role(item.get("role").and_then(Value::as_str));
                let mut message = json!({
                    "role": role,
                    "content": responses_content_to_chat_content(
                        role,
                        item.get("content").unwrap_or(&Value::Null)
                        )
                });
                if role == "assistant" {
                    if !pending_reasoning.is_empty() && pending_tool_calls.is_empty() {
                        message["reasoning_content"] =
                            json!(std::mem::take(pending_reasoning).join("\n"));
                    }
                } else if !pending_reasoning.is_empty() {
                    flush_tool_calls(messages, pending_tool_calls, pending_reasoning);
                    flush_reasoning(messages, pending_reasoning);
                }
                messages.push(message);
            }
        }
    }
}

fn orphan_tool_output_message(call_id: &str, output: &Value) -> Value {
    json!({
        "role": "user",
        "content": format!(
            "Function call output ({call_id}): {}",
            response_output_text(output)
        )
    })
}

fn normalize_chat_messages(messages: &mut [Value]) {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_content = match message.get("content") {
            Some(Value::Null) | None => false,
            Some(Value::String(_)) => true,
            Some(Value::Array(parts)) => !parts.is_empty(),
            Some(_) => true,
        };
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|tool_calls| !tool_calls.is_empty());
        if !has_content && !has_tool_calls {
            message["content"] = json!("");
        }
    }
}

fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    let mut system_chunks = Vec::new();
    let mut rest = Vec::with_capacity(messages.len());

    for message in messages {
        if message.get("role").and_then(Value::as_str) == Some("system") {
            if let Some(text) = message.get("content").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    system_chunks.push(text.to_string());
                }
                continue;
            }
        }
        rest.push(message);
    }

    let mut output = Vec::with_capacity(rest.len() + usize::from(!system_chunks.is_empty()));
    if !system_chunks.is_empty() {
        output.push(json!({
            "role": "system",
            "content": system_chunks.join("\n\n")
        }));
    }
    output.extend(rest);
    output
}

fn responses_role_to_chat_role(role: Option<&str>) -> &'static str {
    match role {
        Some("developer") | Some("system") => "system",
        Some("assistant") => "assistant",
        Some("tool") => "tool",
        Some("latest_reminder") => "user",
        Some("user") | None => "user",
        Some(_) => "user",
    }
}

fn flush_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Vec<String>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some("assistant") {
            merge_tool_calls_into_message(last, std::mem::take(pending_tool_calls));
            return;
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": "",
        "tool_calls": std::mem::take(pending_tool_calls)
    });
    if !pending_reasoning.is_empty() {
        message["reasoning_content"] = json!(std::mem::take(pending_reasoning).join("\n"));
    }
    messages.push(message);
}

fn flush_reasoning(messages: &mut Vec<Value>, pending_reasoning: &mut Vec<String>) {
    if pending_reasoning.is_empty() {
        return;
    }
    let reasoning = std::mem::take(pending_reasoning).join("\n");
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some("assistant") {
            append_reasoning_to_assistant_message(last, &reasoning);
            return;
        }
    }
    messages.push(json!({
        "role": "assistant",
        "content": "",
        "reasoning_content": reasoning
    }));
}

fn append_reasoning_to_assistant_message(message: &mut Value, reasoning: &str) {
    if reasoning.is_empty() {
        return;
    }
    let existing = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .unwrap_or("");
    message["reasoning_content"] = if existing.is_empty() {
        json!(reasoning)
    } else {
        json!(format!("{existing}\n{reasoning}"))
    };
    if message.get("content").is_none() || message.get("content") == Some(&Value::Null) {
        message["content"] = json!("");
    }
}

fn merge_tool_calls_into_message(message: &mut Value, incoming: Vec<Value>) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    let existing = object
        .entry("tool_calls".to_string())
        .or_insert_with(|| json!([]));
    let Some(existing_array) = existing.as_array_mut() else {
        *existing = json!(incoming);
        return;
    };
    for tool_call in incoming {
        let id = tool_call.get("id").and_then(Value::as_str).unwrap_or("");
        if !id.is_empty()
            && existing_array
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some(id))
        {
            continue;
        }
        existing_array.push(tool_call);
    }
    if message.get("content").is_none() || message.get("content") == Some(&Value::Null) {
        message["content"] = json!("");
    }
}

fn responses_reasoning_text(item: &Value) -> Option<String> {
    extract_reasoning_summary_text(item).or_else(|| extract_reasoning_field_text(item))
}

fn responses_content_to_chat_content(_role: &str, content: &Value) -> Value {
    if content.is_null() || content.is_string() {
        return content.clone();
    }

    let Some(parts) = content.as_array() else {
        return content.clone();
    };
    let mut chat_parts = Vec::new();
    let mut has_non_text_part = false;

    for part in parts {
        match part.get("type").and_then(Value::as_str).unwrap_or("") {
            "input_text" | "output_text" | "text" => {
                if let Some(value) = part.get("text").and_then(Value::as_str) {
                    if !value.is_empty() {
                        chat_parts.push(json!({ "type": "text", "text": value }));
                    }
                }
            }
            "refusal" => {
                if let Some(value) = part.get("refusal").and_then(Value::as_str) {
                    if !value.is_empty() {
                        chat_parts.push(json!({ "type": "text", "text": value }));
                    }
                }
            }
            "input_image" => {
                if let Some(image_url) = part.get("image_url").filter(|value| !value.is_null()) {
                    let image_url = if image_url.is_object() {
                        image_url.clone()
                    } else {
                        json!({ "url": image_url.as_str().unwrap_or_default() })
                    };
                    chat_parts.push(json!({ "type": "image_url", "image_url": image_url }));
                    has_non_text_part = true;
                } else if let Some(file_id) = part.get("file_id").and_then(Value::as_str) {
                    let image_url = json!({ "file_id": file_id });
                    chat_parts.push(json!({ "type": "image_url", "image_url": image_url }));
                    has_non_text_part = true;
                }
            }
            "input_file" => {
                let mut file = json!({});
                copy_string_field(part, &mut file, "file_id");
                copy_string_field(part, &mut file, "file_data");
                copy_string_field(part, &mut file, "file_url");
                copy_string_field(part, &mut file, "filename");
                if file
                    .as_object()
                    .map(|object| object.is_empty())
                    .unwrap_or(true)
                {
                    chat_parts.push(json!({ "type": "text", "text": canonical_json_string(part) }));
                } else {
                    chat_parts.push(json!({ "type": "file", "file": file }));
                    has_non_text_part = true;
                }
            }
            "input_audio" => {
                let input_audio = part.get("input_audio").cloned().unwrap_or_else(|| {
                    let mut audio = json!({});
                    copy_string_field(part, &mut audio, "data");
                    copy_string_field(part, &mut audio, "format");
                    audio
                });
                if input_audio
                    .as_object()
                    .map(|object| object.is_empty())
                    .unwrap_or(false)
                {
                    chat_parts.push(json!({ "type": "text", "text": canonical_json_string(part) }));
                } else {
                    chat_parts.push(json!({ "type": "input_audio", "input_audio": input_audio }));
                    has_non_text_part = true;
                }
            }
            _ => {
                chat_parts.push(json!({ "type": "text", "text": canonical_json_string(part) }));
            }
        }
    }

    if !has_non_text_part {
        return Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Value::Array(chat_parts)
}

fn copy_string_field(source: &Value, target: &mut Value, field: &str) {
    if let Some(value) = source.get(field).and_then(Value::as_str) {
        if !value.is_empty() {
            target[field] = json!(value);
        }
    }
}

fn responses_history_function_name(item: &Value) -> String {
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let namespace = item.get("namespace").and_then(Value::as_str).unwrap_or("");
    if name.is_empty() {
        String::new()
    } else if namespace.is_empty() {
        name.to_string()
    } else {
        flatten_namespace_tool_name(namespace, name)
    }
}

fn tools_for_proxy_conversion(request: &Value) -> Vec<Value> {
    let mut tools = Vec::new();
    if let Some(request_tools) = request.get("tools").and_then(Value::as_array) {
        append_unique_tools(&mut tools, request_tools);
    }
    append_tool_search_output_tools(request.get("input"), &mut tools);
    tools
}

fn append_tool_search_output_tools(input: Option<&Value>, tools: &mut Vec<Value>) {
    let Some(items) = input.and_then(Value::as_array) else {
        return;
    };
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("tool_search_output") {
            continue;
        }
        if let Some(found_tools) = item.get("tools").and_then(Value::as_array) {
            append_unique_tools(tools, found_tools);
        }
    }
}

fn append_unique_tools(tools: &mut Vec<Value>, additions: &[Value]) {
    for tool in additions {
        append_unique_tool(tools, tool.clone());
    }
}

fn append_unique_tool(tools: &mut Vec<Value>, tool: Value) {
    if tool.get("type").and_then(Value::as_str) == Some("namespace") {
        if merge_namespace_tool(tools, &tool) {
            return;
        }
    }
    let key = tool_identity_key(&tool);
    if tools
        .iter()
        .any(|existing| tool_identity_key(existing) == key)
    {
        return;
    }
    tools.push(tool);
}

fn merge_namespace_tool(tools: &mut [Value], tool: &Value) -> bool {
    let namespace = tool.get("name").and_then(Value::as_str).unwrap_or("");
    if namespace.is_empty() {
        return false;
    }
    let Some(existing) = tools.iter_mut().find(|existing| {
        existing.get("type").and_then(Value::as_str) == Some("namespace")
            && existing.get("name").and_then(Value::as_str) == Some(namespace)
    }) else {
        return false;
    };

    if existing
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        if let Some(description) = tool.get("description") {
            existing["description"] = description.clone();
        }
    }

    let Some(incoming_children) = tool.get("tools").and_then(Value::as_array) else {
        return true;
    };
    if !existing.get("tools").is_some_and(Value::is_array) {
        existing["tools"] = json!([]);
    }
    let Some(existing_children) = existing.get_mut("tools").and_then(Value::as_array_mut) else {
        return true;
    };
    for child in incoming_children {
        let key = tool_identity_key(child);
        if existing_children
            .iter()
            .any(|existing_child| tool_identity_key(existing_child) == key)
        {
            continue;
        }
        existing_children.push(child.clone());
    }
    true
}

fn tool_identity_key(tool: &Value) -> String {
    if let Some(name) = tool.as_str() {
        return format!("string:{name}");
    }
    let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
    if !tool_type.is_empty() || !name.is_empty() {
        return format!("{tool_type}:{name}");
    }
    tool.to_string()
}

fn build_codex_tool_context_for_request(request: &Value) -> CodexToolContext {
    let tools = tools_for_proxy_conversion(request);
    build_codex_tool_context_from_tools(&tools)
}

fn build_codex_tool_context_from_tools(tools: &[Value]) -> CodexToolContext {
    let mut context = CodexToolContext::default();
    add_tools_to_codex_tool_context(&mut context, tools);
    context
}

fn add_tools_to_codex_tool_context(context: &mut CodexToolContext, tools: &[Value]) {
    for tool in tools {
        if let Some(name) = tool.as_str().filter(|name| !name.is_empty()) {
            if let Some(action) = proxy_action_from_upstream_name(name) {
                context.custom_tools.insert(
                    name.to_string(),
                    CodexCustomToolSpec {
                        openai_name: "apply_patch".to_string(),
                        kind: CodexCustomToolKind::ApplyPatch,
                        proxy_action: Some(action),
                    },
                );
                context.has_custom_tools = true;
                continue;
            }
            context.custom_tools.insert(
                name.to_string(),
                CodexCustomToolSpec {
                    openai_name: name.to_string(),
                    kind: CodexCustomToolKind::Raw,
                    proxy_action: None,
                },
            );
            context.has_custom_tools = true;
            continue;
        }
        let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
        match tool_type {
            "custom" => {
                let Some(name) = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                else {
                    continue;
                };
                let kind = detect_codex_custom_tool_kind(tool, name);
                context.custom_tools.insert(
                    name.to_string(),
                    CodexCustomToolSpec {
                        openai_name: name.to_string(),
                        kind,
                        proxy_action: None,
                    },
                );
                if kind == CodexCustomToolKind::ApplyPatch {
                    for action in [
                        CodexPatchProxyAction::AddFile,
                        CodexPatchProxyAction::DeleteFile,
                        CodexPatchProxyAction::UpdateFile,
                        CodexPatchProxyAction::ReplaceFile,
                        CodexPatchProxyAction::Batch,
                    ] {
                        let proxy_name = format!("{name}_{}", action.suffix());
                        context.custom_tools.insert(
                            proxy_name,
                            CodexCustomToolSpec {
                                openai_name: name.to_string(),
                                kind: CodexCustomToolKind::ApplyPatch,
                                proxy_action: Some(action),
                            },
                        );
                    }
                }
                context.has_custom_tools = true;
            }
            "function" => {
                if let Some(name) = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    context.function_tools.insert(
                        name.to_string(),
                        CodexFunctionToolSpec {
                            name: name.to_string(),
                            namespace: String::new(),
                        },
                    );
                }
            }
            "namespace" => add_namespace_tools_to_context(&mut *context, tool),
            _ if is_builtin_proxy_tool_type(tool_type) => {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .unwrap_or(tool_type);
                context.custom_tools.insert(
                    name.to_string(),
                    CodexCustomToolSpec {
                        openai_name: name.to_string(),
                        kind: CodexCustomToolKind::BuiltIn,
                        proxy_action: None,
                    },
                );
                context.has_custom_tools = true;
            }
            _ => {}
        }
    }
}

fn add_namespace_tools_to_context(context: &mut CodexToolContext, namespace_tool: &Value) {
    let namespace = namespace_tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(children) = namespace_tool.get("tools").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        if child.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let Some(name) = child
            .get("name")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        let flat = flatten_namespace_tool_name(namespace, name);
        if namespace.is_empty() {
            context.function_tools.insert(
                flat,
                CodexFunctionToolSpec {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                },
            );
        } else if context
            .function_tools
            .get(&flat)
            .is_none_or(|spec| !spec.namespace.is_empty())
        {
            context.function_tools.insert(
                flat.clone(),
                CodexFunctionToolSpec {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                },
            );
            maybe_set_web_search_fallback(context, namespace, name, &flat, child);
            context.has_namespace_tools = true;
        }
    }
}

fn maybe_set_web_search_fallback(
    context: &mut CodexToolContext,
    namespace: &str,
    name: &str,
    flat_name: &str,
    tool: &Value,
) {
    let Some(score) = web_search_fallback_score(namespace, name, flat_name, tool) else {
        return;
    };
    if context
        .web_search_fallback
        .as_ref()
        .is_some_and(|candidate| candidate.score >= score)
    {
        return;
    }
    context.web_search_fallback = Some(CodexWebSearchFallbackTool {
        namespace: namespace.to_string(),
        name: name.to_string(),
        query_parameter: web_search_fallback_query_parameter(tool),
        score,
    });
}

fn web_search_fallback_score(
    namespace: &str,
    name: &str,
    flat_name: &str,
    tool: &Value,
) -> Option<i32> {
    let namespace = namespace.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let flat_name = flat_name.to_ascii_lowercase();
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let haystack = format!("{namespace} {name} {flat_name} {description}");

    if flat_name == "mcp__tavily__tavily_search" {
        return Some(100);
    }
    if flat_name == "mcp__exa_search__web_search_exa" {
        return Some(95);
    }
    if name == "web_search" || name == "web_search_exa" {
        return Some(90);
    }
    if name == "tavily_search" {
        return Some(85);
    }
    if name.contains("web_search") {
        return Some(80);
    }
    if name.contains("search") && namespace.contains("tavily") {
        return Some(75);
    }
    if name.contains("search") && namespace.contains("exa") {
        return Some(70);
    }
    if name.contains("search") && haystack.contains("web") {
        return Some(60);
    }
    None
}

fn web_search_fallback_query_parameter(tool: &Value) -> String {
    let properties = tool
        .pointer("/parameters/properties")
        .and_then(Value::as_object);
    for candidate in ["query", "q", "search_query", "input", "text"] {
        if properties.is_some_and(|properties| properties.contains_key(candidate)) {
            return candidate.to_string();
        }
    }
    "query".to_string()
}

fn responses_tools_to_chat_tools(tools: &[Value], context: &CodexToolContext) -> Vec<Value> {
    let mut converted = Vec::new();
    for tool in tools {
        if let Some(name) = tool.as_str().filter(|name| !name.is_empty()) {
            converted.push(generic_custom_proxy_tool(name, ""));
            continue;
        }
        match tool.get("type").and_then(Value::as_str).unwrap_or("") {
            "function" => {
                if let Some(tool) = responses_function_tool_to_chat_tool(tool) {
                    converted.push(tool);
                }
            }
            "custom" => {
                let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .unwrap_or(tool_type);
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if detect_codex_custom_tool_kind(tool, name) == CodexCustomToolKind::ApplyPatch {
                    converted.extend(apply_patch_proxy_tools(name, description));
                } else {
                    converted.push(generic_custom_proxy_tool(name, description));
                }
            }
            tool_type if is_builtin_proxy_tool_type(tool_type) => {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .unwrap_or(tool_type);
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                converted.push(generic_custom_proxy_tool(name, description));
            }
            tool_type if is_proxy_internal_tool_type(tool_type) => {
                converted.push(proxy_internal_tool_to_chat_tool(tool_type, tool));
            }
            "namespace" => converted.extend(namespace_tool_to_chat_tools(tool, context)),
            _ => {}
        }
    }
    converted
}

fn responses_tools_to_anthropic_tools(tools: &[Value], context: &CodexToolContext) -> Vec<Value> {
    responses_tools_to_chat_tools(tools, context)
        .into_iter()
        .filter_map(|tool| chat_tool_to_anthropic_tool(&tool))
        .collect()
}

fn proxy_internal_tool_to_chat_tool(tool_type: &str, tool: &Value) -> Value {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(tool_type);
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| proxy_internal_tool_description(tool_type));
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"],
                "additionalProperties": true
            }
        }
    })
}

fn proxy_internal_tool_description(tool_type: &str) -> &'static str {
    match tool_type {
        "tool_search" => "Search the tool catalog available to this Codex session.",
        "web_search" | "web_search_preview" | "web_search_preview_2025_03_11" => {
            "Search the web for up-to-date information."
        }
        _ => "Proxy-managed built-in tool.",
    }
}

fn chat_tool_to_anthropic_tool(tool: &Value) -> Option<Value> {
    let function = tool.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let mut anthropic_tool = json!({
        "name": name,
        "input_schema": normalize_chat_tool_parameters(
            function.get("parameters").unwrap_or(&json!({}))
        )
    });
    if let Some(description) = function
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        anthropic_tool["description"] = json!(description);
    }
    Some(anthropic_tool)
}

fn detect_codex_custom_tool_kind(tool: &Value, name: &str) -> CodexCustomToolKind {
    if name == "apply_patch" {
        return CodexCustomToolKind::ApplyPatch;
    }
    if let Some(definition) = tool.pointer("/format/definition").and_then(Value::as_str) {
        if definition.contains("begin_patch")
            && definition.contains("end_patch")
            && definition.contains("add_hunk")
        {
            return CodexCustomToolKind::ApplyPatch;
        }
    }
    if tool
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(is_builtin_proxy_tool_type)
    {
        CodexCustomToolKind::BuiltIn
    } else {
        CodexCustomToolKind::Raw
    }
}

fn is_builtin_proxy_tool_type(tool_type: &str) -> bool {
    matches!(
        tool_type,
        "local_shell" | "computer_use" | "computer_use_preview"
    )
}

fn is_filtered_server_side_tool_type(tool_type: &str) -> bool {
    let _ = tool_type;
    false
}

fn is_proxy_internal_tool_type(tool_type: &str) -> bool {
    matches!(
        tool_type,
        "tool_search" | "web_search" | "web_search_preview" | "web_search_preview_2025_03_11"
    )
}

fn is_codex_tool_search_name(name: &str) -> bool {
    name == "tool_search"
}

fn is_codex_web_search_name(name: &str) -> bool {
    matches!(
        name,
        "web_search" | "web_search_preview" | "web_search_preview_2025_03_11"
    )
}

fn proxy_internal_tool_types(tools: Option<&Value>) -> Vec<String> {
    let Some(tools) = tools.and_then(Value::as_array) else {
        return Vec::new();
    };
    tools
        .iter()
        .filter_map(|tool| tool.get("type").and_then(Value::as_str))
        .filter(|tool_type| is_proxy_internal_tool_type(tool_type))
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn responses_function_tool_to_chat_tool(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }
    if tool.get("function").is_some() {
        let mut chat_tool = tool.clone();
        if let Some(strict) = tool.get("strict").cloned() {
            if let Some(function) = chat_tool.get_mut("function").and_then(Value::as_object_mut) {
                function.entry("strict".to_string()).or_insert(strict);
            }
            if let Some(object) = chat_tool.as_object_mut() {
                object.remove("strict");
            }
        }
        if let Some(function) = chat_tool.get_mut("function").and_then(Value::as_object_mut) {
            let normalized =
                normalize_chat_tool_parameters(function.get("parameters").unwrap_or(&json!({})));
            function.insert("parameters".to_string(), normalized);
        }
        return Some(chat_tool);
    }
    let mut function = json!({
        "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": normalize_chat_tool_parameters(tool.get("parameters").unwrap_or(&json!({})))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }
    Some(json!({
        "type": "function",
        "function": function
    }))
}

fn namespace_tool_to_chat_tools(namespace_tool: &Value, context: &CodexToolContext) -> Vec<Value> {
    let namespace = namespace_tool
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let namespace_description = namespace_tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(children) = namespace_tool.get("tools").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut converted = Vec::new();
    for child in children {
        if child.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let Some(name) = child
            .get("name")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        let flat = flatten_namespace_tool_name(namespace, name);
        if namespace != ""
            && context
                .function_tools
                .get(&flat)
                .is_some_and(|spec| spec.namespace.is_empty())
        {
            continue;
        }
        let description = combine_namespace_description(
            namespace_description,
            child
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let mut function = json!({
            "name": flat,
            "parameters": normalize_chat_tool_parameters(child.get("parameters").unwrap_or(&json!({})))
        });
        if !description.is_empty() {
            function["description"] = json!(description);
        }
        converted.push(json!({
            "type": "function",
            "function": function
        }));
    }
    converted
}

fn normalize_chat_tool_parameters(parameters: &Value) -> Value {
    let mut normalized = if parameters.is_object() {
        parameters.clone()
    } else {
        json!({})
    };
    if normalized.get("type").is_none() {
        normalized["type"] = json!("object");
    }
    if normalized.get("properties").is_none() {
        normalized["properties"] = json!({});
    }
    if normalized.get("required").is_none() {
        normalized["required"] = json!([]);
    }
    normalized
}

fn generic_custom_proxy_tool(name: &str, description: &str) -> Value {
    let description = if description.trim().is_empty() {
        format!("FREEFORM custom tool: {name}. Put only the tool input text here.")
    } else {
        format!(
            "{}\n\nThis is a FREEFORM tool. Do not wrap the input in JSON or markdown.",
            description.trim()
        )
    };
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Raw freeform input for this custom tool."
                    }
                },
                "required": ["input"]
            }
        }
    })
}

fn apply_patch_proxy_tools(name: &str, description: &str) -> Vec<Value> {
    vec![
        function_tool(
            &format!("{name}_add_file"),
            &patch_proxy_description(
                description,
                "add_file",
                "Create one new file by providing a target path and full file content.",
            ),
            apply_patch_add_file_schema(),
        ),
        function_tool(
            &format!("{name}_delete_file"),
            &patch_proxy_description(
                description,
                "delete_file",
                "Delete one file by providing a target path.",
            ),
            apply_patch_delete_file_schema(),
        ),
        function_tool(
            &format!("{name}_update_file"),
            &patch_proxy_description(
                description,
                "update_file",
                "Edit one existing file with structured hunks.",
            ),
            apply_patch_update_file_schema(),
        ),
        function_tool(
            &format!("{name}_replace_file"),
            &patch_proxy_description(
                description,
                "replace_file",
                "Replace one existing file by providing a target path and full new file content.",
            ),
            apply_patch_replace_file_schema(),
        ),
        function_tool(
            &format!("{name}_batch"),
            &patch_proxy_description(
                description,
                "batch",
                "Edit files by providing structured JSON patch operations.",
            ),
            apply_patch_batch_schema(),
        ),
    ]
}

fn function_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

fn patch_proxy_description(description: &str, action: &str, default_description: &str) -> String {
    if description.trim().is_empty() {
        default_description.to_string()
    } else {
        format!("{} (proxy action: {action})", description.trim())
    }
}

fn apply_patch_add_file_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Target file path." },
            "content": { "type": "string", "description": "Full file content without patch '+' prefixes." }
        },
        "required": ["path", "content"]
    })
}

fn apply_patch_delete_file_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Target file path." }
        },
        "required": ["path"]
    })
}

fn apply_patch_update_file_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Target file path." },
            "move_to": { "type": "string", "description": "Optional destination path for move operations." },
            "hunks": apply_patch_hunks_schema()
        },
        "required": ["path", "hunks"]
    })
}

fn apply_patch_replace_file_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Target file path." },
            "content": { "type": "string", "description": "Full replacement content." }
        },
        "required": ["path", "content"]
    })
}

fn apply_patch_batch_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "operations": {
                "type": "array",
                "description": "Ordered list of file patch operations.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "type": { "type": "string", "enum": ["add_file", "delete_file", "update_file", "replace_file"] },
                        "path": { "type": "string" },
                        "move_to": { "type": "string", "description": "Optional destination path for move operations (update_file only)." },
                        "content": { "type": "string", "description": "Full file content for add_file / replace_file." },
                        "hunks": apply_patch_hunks_schema()
                    },
                    "required": ["type", "path"]
                }
            }
        },
        "required": ["operations"]
    })
}

fn apply_patch_hunks_schema() -> Value {
    json!({
        "type": "array",
        "description": "Structured update hunks (required when type=update_file).",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "context": { "type": "string", "description": "Optional @@ context header text." },
                "lines": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "op": { "type": "string", "enum": ["context", "add", "remove"] },
                            "text": { "type": "string" }
                        },
                        "required": ["op", "text"]
                    }
                }
            },
            "required": ["lines"]
        }
    })
}

fn proxy_action_from_upstream_name(name: &str) -> Option<CodexPatchProxyAction> {
    if name.ends_with("_add_file") {
        Some(CodexPatchProxyAction::AddFile)
    } else if name.ends_with("_delete_file") {
        Some(CodexPatchProxyAction::DeleteFile)
    } else if name.ends_with("_update_file") {
        Some(CodexPatchProxyAction::UpdateFile)
    } else if name.ends_with("_replace_file") {
        Some(CodexPatchProxyAction::ReplaceFile)
    } else if name.ends_with("_batch") {
        Some(CodexPatchProxyAction::Batch)
    } else {
        None
    }
}

fn combine_namespace_description(namespace_description: &str, child_description: &str) -> String {
    let namespace_description = namespace_description.trim();
    let child_description = child_description.trim();
    match (
        namespace_description.is_empty(),
        child_description.is_empty(),
    ) {
        (true, true) => String::new(),
        (true, false) => child_description.to_string(),
        (false, true) => namespace_description.to_string(),
        (false, false) => format!("{namespace_description}\n\n{child_description}"),
    }
}

fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        return name.to_string();
    }
    if name.is_empty() {
        return namespace.to_string();
    }
    if namespace.ends_with("__") || name.starts_with("__") {
        format!("{namespace}{name}")
    } else {
        format!("{namespace}__{name}")
    }
}

fn responses_tool_choice_to_chat(tool_choice: &Value, context: &CodexToolContext) -> Option<Value> {
    match tool_choice {
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            if let Some(namespace) = object.get("namespace").and_then(Value::as_str) {
                let name = object.get("name").and_then(Value::as_str).unwrap_or("");
                let flat_name = flatten_namespace_tool_name(namespace, name);
                if !chat_tool_choice_target_available(context, &flat_name) {
                    return None;
                }
                return Some(json!({
                    "type": "function",
                    "function": {
                        "name": flat_name
                    }
                }));
            }
            if let Some(function) = object.get("function").and_then(Value::as_object) {
                if let Some(namespace) = function.get("namespace").and_then(Value::as_str) {
                    let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                    let flat_name = flatten_namespace_tool_name(namespace, name);
                    if !chat_tool_choice_target_available(context, &flat_name) {
                        return None;
                    }
                    return Some(json!({
                        "type": "function",
                        "function": {
                            "name": flat_name
                        }
                    }));
                }
            }
            let name = object.get("name").and_then(Value::as_str).unwrap_or("");
            if !chat_tool_choice_target_available(context, name) {
                return None;
            }
            Some(json!({
                "type": "function",
                "function": {
                    "name": name
                }
            }))
        }
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("custom") => {
            let name = object.get("name").and_then(Value::as_str)?;
            let spec = context.custom_tools.get(name)?;
            let upstream_name = if spec.kind == CodexCustomToolKind::ApplyPatch {
                format!("{}_batch", spec.openai_name)
            } else {
                spec.openai_name.clone()
            };
            Some(json!({
                "type": "function",
                "function": { "name": upstream_name }
            }))
        }
        Value::Object(object)
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(is_proxy_internal_tool_type) =>
        {
            let name = object.get("type").and_then(Value::as_str).unwrap_or("");
            Some(json!({
                "type": "function",
                "function": { "name": name }
            }))
        }
        other => Some(other.clone()),
    }
}

fn chat_tool_choice_target_available(context: &CodexToolContext, name: &str) -> bool {
    !name.is_empty()
        && (is_proxy_internal_tool_type(name)
            || context.function_tools.contains_key(name)
            || context.custom_tools.contains_key(name))
}

fn responses_tool_choice_to_anthropic(
    tool_choice: &Value,
    context: &CodexToolContext,
) -> Option<Value> {
    match tool_choice {
        Value::String(value) => match value.trim() {
            "auto" => Some(json!({ "type": "auto" })),
            "required" => Some(json!({ "type": "any" })),
            "none" => Some(json!({ "type": "none" })),
            _ => None,
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Some(json!({ "type": "auto" })),
            Some("required") => Some(json!({ "type": "any" })),
            Some("none") => Some(json!({ "type": "none" })),
            _ => {
                let chat_choice = responses_tool_choice_to_chat(tool_choice, context)?;
                let name = chat_choice
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .or_else(|| chat_choice.get("name").and_then(Value::as_str))?;
                (!name.is_empty()).then(|| json!({ "type": "tool", "name": name }))
            }
        },
        _ => None,
    }
}

fn chat_reasoning_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let reasoning = chat_reasoning_text(message)?;
    if reasoning.is_empty() {
        return None;
    }
    Some(json!({
        "id": format!("rs_{response_id}"),
        "type": "reasoning",
        "reasoning_content": reasoning,
        "summary": [{ "type": "summary_text", "text": reasoning }]
    }))
}

fn chat_reasoning_text(message: &Value) -> Option<String> {
    if let Some(reasoning) = extract_reasoning_field_text(message) {
        return Some(reasoning);
    }

    if let Some(content) = message.get("content").and_then(Value::as_str) {
        if let Some((reasoning, _answer)) = split_leading_think_block(content) {
            if !reasoning.is_empty() {
                return Some(reasoning);
            }
        }
    }

    None
}

fn chat_message_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        let text = split_leading_think_block(text)
            .map(|(_reasoning, answer)| answer)
            .unwrap_or_else(|| text.to_string());
        if !text.is_empty() {
            content.push(json!({ "type": "output_text", "text": text, "annotations": [] }));
        }
    } else if let Some(parts) = message.get("content").and_then(Value::as_array) {
        for part in parts {
            match part.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" | "output_text" => {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            content.push(
                                json!({ "type": "output_text", "text": text, "annotations": [] }),
                            );
                        }
                    }
                }
                "refusal" => {
                    if let Some(refusal) = part.get("refusal").and_then(Value::as_str) {
                        if !refusal.is_empty() {
                            content.push(json!({ "type": "refusal", "refusal": refusal }));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
        if !refusal.is_empty() {
            content.push(json!({ "type": "refusal", "refusal": refusal }));
        }
    }

    if content.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("{response_id}_msg"),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    }))
}

fn chat_tool_calls_to_response_output_items(
    message: &Value,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let mut output = Vec::new();
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            output.push(chat_tool_call_to_response_item(
                tool_call,
                index,
                tool_context,
            ));
        }
    } else if let Some(function_call) = message.get("function_call") {
        output.push(chat_legacy_function_call_to_response_item(
            function_call,
            tool_context,
        ));
    }
    output
}

fn chat_tool_call_to_response_item(
    tool_call: &Value,
    index: usize,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = tool_call
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{index}"));
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    let name = function.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = responses_arguments_to_chat(function.get("arguments").unwrap_or(&json!({})));
    response_tool_call_item(&call_id, name, &arguments, tool_context)
}

fn chat_legacy_function_call_to_response_item(
    function_call: &Value,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = function_call
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("call_0");
    let name = function_call
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments =
        responses_arguments_to_chat(function_call.get("arguments").unwrap_or(&json!({})));
    response_tool_call_item(call_id, name, &arguments, tool_context)
}

fn tool_call_added_item(
    state: &ToolCallState,
    output_index: u32,
    tool_context: &CodexToolContext,
) -> Value {
    if is_codex_web_search_name(&state.name) {
        if let Some(fallback) = tool_context.web_search_fallback_tool() {
            let mut item = json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": {
                    "id": state.item_id,
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": state.call_id,
                    "name": fallback.name,
                    "arguments": ""
                }
            });
            if !fallback.namespace.is_empty() {
                item["item"]["namespace"] = json!(fallback.namespace);
            }
            return item;
        }
        return json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {
                "id": web_search_item_id(&state.call_id),
                "type": "web_search_call",
                "status": "in_progress",
                "execution": "client",
                "action": { "type": "other" }
            }
        });
    }
    if is_codex_tool_search_name(&state.name) {
        return json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {
                "id": format!("tsc_{}", state.call_id),
                "type": "tool_search_call",
                "status": "in_progress",
                "call_id": state.call_id,
                "execution": "client",
                "arguments": {}
            }
        });
    }
    if tool_context.is_custom_tool_proxy(&state.name) {
        return json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": {
                "id": format!("ctc_{}", state.call_id),
                "type": "custom_tool_call",
                "status": "in_progress",
                "call_id": state.call_id,
                "name": tool_context.original_custom_tool_name(&state.name),
                "input": ""
            }
        });
    }
    let (display_name, namespace) = tool_context.openai_name_for_function_tool(&state.name);
    let mut item = json!({
        "type": "response.output_item.added",
        "output_index": output_index,
        "item": {
            "id": state.item_id,
            "type": "function_call",
            "status": "in_progress",
            "call_id": state.call_id,
            "name": display_name,
            "arguments": ""
        }
    });
    if !namespace.is_empty() {
        item["item"]["namespace"] = json!(namespace);
    }
    item
}

fn push_tool_call_delta_sse(
    output: &mut String,
    state: &ToolCallState,
    output_index: u32,
    delta: &str,
    tool_context: &CodexToolContext,
    next_sequence_number: &mut u64,
) {
    if is_codex_tool_search_name(&state.name)
        || (is_codex_web_search_name(&state.name)
            && tool_context.web_search_fallback_tool().is_none())
    {
        let _ = (output, output_index, delta, next_sequence_number);
        return;
    }
    if is_codex_web_search_name(&state.name) {
        let _ = (output, output_index, delta, next_sequence_number);
        return;
    }
    if tool_context.is_custom_tool_proxy(&state.name) {
        let _ = delta;
    } else {
        push_sse(
            output,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": state.item_id,
                "output_index": output_index,
                "delta": delta
            }),
            next_sequence_number,
        );
    }
}

fn push_tool_call_done_sse(
    output: &mut String,
    state: &ToolCallState,
    output_index: u32,
    tool_context: &CodexToolContext,
    next_sequence_number: &mut u64,
) {
    if is_codex_tool_search_name(&state.name) {
        let _ = (output, output_index, tool_context, next_sequence_number);
        return;
    }
    if is_codex_web_search_name(&state.name) {
        if let Some(fallback) = tool_context.web_search_fallback_tool() {
            push_sse(
                output,
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": state.item_id,
                    "name": fallback.name,
                    "output_index": output_index,
                    "arguments": web_search_fallback_arguments(&state.arguments, fallback)
                }),
                next_sequence_number,
            );
        } else {
            let _ = (output, output_index, next_sequence_number);
        }
        return;
    }
    if tool_context.is_custom_tool_proxy(&state.name) {
        let input = reconstruct_custom_tool_call_input_with_context(
            tool_context,
            &state.name,
            &state.arguments,
        );
        push_sse(
            output,
            "response.custom_tool_call_input.delta",
            json!({
                "type": "response.custom_tool_call_input.delta",
                "item_id": format!("ctc_{}", state.call_id),
                "call_id": state.call_id,
                "output_index": output_index,
                "delta": input.clone()
            }),
            next_sequence_number,
        );
        push_sse(
            output,
            "response.custom_tool_call_input.done",
            json!({
                "type": "response.custom_tool_call_input.done",
                "item_id": format!("ctc_{}", state.call_id),
                "output_index": output_index,
                "input": input
            }),
            next_sequence_number,
        );
        return;
    }
    let (display_name, _namespace) = tool_context.openai_name_for_function_tool(&state.name);
    push_sse(
        output,
        "response.function_call_arguments.done",
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": state.item_id,
            "name": display_name,
            "output_index": output_index,
            "arguments": state.arguments
        }),
        next_sequence_number,
    );
}

fn tool_call_done_item(state: &ToolCallState, tool_context: &CodexToolContext) -> Value {
    response_tool_call_item(&state.call_id, &state.name, &state.arguments, tool_context)
}

fn response_tool_call_item(
    call_id: &str,
    name: &str,
    arguments: &str,
    tool_context: &CodexToolContext,
) -> Value {
    if is_codex_web_search_name(name) {
        if let Some(fallback) = tool_context.web_search_fallback_tool() {
            return web_search_fallback_function_call_item(call_id, arguments, fallback);
        }
        return web_search_call_item(call_id, arguments);
    }
    if is_codex_tool_search_name(name) {
        return tool_search_call_item(call_id, arguments);
    }
    if tool_context.is_custom_tool_proxy(name) {
        return json!({
            "id": format!("ctc_{call_id}"),
            "type": "custom_tool_call",
            "status": "completed",
            "call_id": call_id,
            "name": tool_context.original_custom_tool_name(name),
            "input": reconstruct_custom_tool_call_input_with_context(tool_context, name, arguments)
        });
    }
    let (display_name, namespace) = tool_context.openai_name_for_function_tool(name);
    let mut item = json!({
        "id": format!("fc_{call_id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": display_name,
        "arguments": arguments
    });
    if !namespace.is_empty() {
        item["namespace"] = json!(namespace);
    }
    item
}

fn web_search_fallback_function_call_item(
    call_id: &str,
    arguments: &str,
    fallback: &CodexWebSearchFallbackTool,
) -> Value {
    let mut item = json!({
        "id": format!("fc_{call_id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": call_id,
        "name": fallback.name,
        "arguments": web_search_fallback_arguments(arguments, fallback)
    });
    if !fallback.namespace.is_empty() {
        item["namespace"] = json!(fallback.namespace);
    }
    item
}

fn tool_search_call_item(call_id: &str, arguments: &str) -> Value {
    json!({
        "id": format!("tsc_{call_id}"),
        "type": "tool_search_call",
        "status": "completed",
        "call_id": call_id,
        "execution": "client",
        "arguments": tool_search_arguments_from_argument_string(arguments)
    })
}

fn web_search_item_id(call_id: &str) -> String {
    if call_id.starts_with("ws_") {
        call_id.to_string()
    } else {
        format!("ws_{call_id}")
    }
}

fn web_search_call_item(call_id: &str, arguments: &str) -> Value {
    let query = web_search_query_from_argument_string(arguments);
    let action = if query.is_empty() {
        json!({ "type": "other" })
    } else {
        json!({
            "type": "search",
            "query": query.clone(),
            "queries": [query]
        })
    };
    json!({
        "id": web_search_item_id(call_id),
        "type": "web_search_call",
        "status": "completed",
        "execution": "client",
        "action": action
    })
}

fn web_search_query_from_argument_string(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => object
            .get("query")
            .or_else(|| object.get("q"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        Ok(Value::String(text)) => text.trim().to_string(),
        Ok(other) => response_output_text(&other).trim().to_string(),
        Err(_) => trimmed.to_string(),
    }
}

fn web_search_fallback_arguments(arguments: &str, fallback: &CodexWebSearchFallbackTool) -> String {
    let query = web_search_query_from_argument_string(arguments);
    let mut object = serde_json::Map::new();
    if !query.is_empty() {
        object.insert(fallback.query_parameter.clone(), Value::String(query));
    }
    canonical_json_string(&Value::Object(object))
}

fn anthropic_content_to_response_output_items(
    content: &Value,
    response_id: &str,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let Some(blocks) = content.as_array() else {
        return Vec::new();
    };

    let mut reasoning_chunks = Vec::new();
    let mut message_content = Vec::new();
    let mut tool_items = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "thinking" => {
                if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                    if !text.is_empty() {
                        reasoning_chunks.push(text.to_string());
                    }
                }
            }
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if let Some((leading, calls)) = split_text_into_message_and_tool_calls(text) {
                        if !leading.is_empty() {
                            message_content.push(json!({
                                "type": "output_text",
                                "text": leading,
                                "annotations": []
                            }));
                        }
                        tool_items
                            .extend(textual_invoke_calls_to_response_items(calls, tool_context));
                    } else if !text.is_empty() {
                        message_content.push(
                            json!({ "type": "output_text", "text": text, "annotations": [] }),
                        );
                    }
                }
            }
            "tool_use" => {
                let call_id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("call_{index}"));
                let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = block
                    .get("input")
                    .map(canonical_json_string)
                    .unwrap_or_else(|| "{}".to_string());
                tool_items.push(response_tool_call_item(
                    &call_id,
                    name,
                    &arguments,
                    tool_context,
                ));
            }
            _ => {}
        }
    }

    let mut output = Vec::new();
    if !reasoning_chunks.is_empty() {
        let reasoning = reasoning_chunks.join("\n\n");
        output.push(json!({
            "id": format!("rs_{response_id}"),
            "type": "reasoning",
            "reasoning_content": reasoning,
            "summary": [{ "type": "summary_text", "text": reasoning }]
        }));
    }
    if !message_content.is_empty() {
        output.push(json!({
            "id": format!("{response_id}_msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": message_content
        }));
    }
    output.extend(tool_items);
    output
}

fn log_anthropic_request_shape(
    request: &Value,
    original_body: &Value,
    diagnostic_id: Option<&str>,
) {
    let tool_names = request
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let original_tool_types = original_body
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    tool.get("type")
                        .and_then(Value::as_str)
                        .or_else(|| tool.as_str())
                        .unwrap_or("unknown")
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let system = request.get("system").and_then(Value::as_str).unwrap_or("");
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.anthropic_request_shape",
        json!({
            "diagnosticId": diagnostic_id,
            "model": request.get("model").and_then(Value::as_str).unwrap_or(""),
            "originalToolCount": original_tool_types.len(),
            "originalToolTypes": original_tool_types,
            "anthropicToolCount": tool_names.len(),
            "anthropicToolNames": tool_names,
            "hasToolChoice": request.get("tool_choice").is_some(),
            "hasSystem": !system.is_empty(),
            "systemLength": system.len(),
            "thinkingType": request.pointer("/thinking/type").and_then(Value::as_str).unwrap_or(""),
            "outputConfigEffort": request.pointer("/output_config/effort").and_then(Value::as_str).unwrap_or(""),
            "reasoningSource": reasoning_source_label(original_body)
        }),
    );
}

fn log_anthropic_response_shape(
    body: &Value,
    output: &[Value],
    stream: bool,
    diagnostic_id: Option<&str>,
) {
    let content = body.get("content").unwrap_or(&Value::Null);
    let mut block_counts = BTreeMap::<String, usize>::new();
    let mut native_tool_names = BTreeSet::<String>::new();
    let mut textual_invoke_tool_names = BTreeSet::<String>::new();
    let mut textual_invoke_block_count = 0usize;
    let mut textual_invoke_call_count = 0usize;
    let mut text_block_count = 0usize;
    let mut max_text_block_len = 0usize;
    let mut text_tool_marker_block_count = 0usize;
    let mut text_tool_marker_kind_set = BTreeSet::<String>::new();

    if let Some(blocks) = content.as_array() {
        for block in blocks {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            *block_counts.entry(block_type.clone()).or_default() += 1;
            if block_type == "tool_use" {
                if let Some(name) = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                {
                    native_tool_names.insert(name.to_string());
                }
            }
            if block_type == "text" {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    text_block_count += 1;
                    max_text_block_len = max_text_block_len.max(text.len());
                    let marker_kinds = text_tool_marker_kinds(text);
                    if !marker_kinds.is_empty() {
                        text_tool_marker_block_count += 1;
                        for kind in &marker_kinds {
                            text_tool_marker_kind_set.insert(kind.clone());
                        }
                        log_anthropic_text_tool_marker_detected(
                            false,
                            body.get("model").and_then(Value::as_str).unwrap_or(""),
                            None,
                            marker_kinds,
                            parse_textual_invoke_tool_calls(text).is_some(),
                            text.len(),
                            text,
                            diagnostic_id,
                        );
                    }
                    if let Some(calls) = parse_textual_invoke_tool_calls(text) {
                        textual_invoke_block_count += 1;
                        textual_invoke_call_count += calls.len();
                        for call in calls {
                            textual_invoke_tool_names.insert(call.name);
                        }
                    }
                }
            }
        }
    }

    let output_counts = output_item_type_counts(output);
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.anthropic_response_shape",
        json!({
            "diagnosticId": diagnostic_id,
            "stream": stream,
            "model": body.get("model").and_then(Value::as_str).unwrap_or(""),
            "stopReason": body.get("stop_reason").and_then(Value::as_str).unwrap_or(""),
            "contentBlockCounts": block_counts,
            "nativeToolUseNames": native_tool_names.into_iter().collect::<Vec<_>>(),
            "textBlockCount": text_block_count,
            "maxTextBlockLength": max_text_block_len,
            "textToolMarkerBlockCount": text_tool_marker_block_count,
            "textToolMarkerKinds": text_tool_marker_kind_set.into_iter().collect::<Vec<_>>(),
            "textualInvokeBlockCount": textual_invoke_block_count,
            "textualInvokeCallCount": textual_invoke_call_count,
            "textualInvokeToolNames": textual_invoke_tool_names.into_iter().collect::<Vec<_>>(),
            "convertedOutputCounts": output_counts
        }),
    );
}

fn log_anthropic_stream_response_shape(state: &AnthropicSseState) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.anthropic_stream_response_shape",
        json!({
            "diagnosticId": state.diagnostic_id.as_deref(),
            "stream": true,
            "model": state.inner.model.as_str(),
            "textBlockCount": state.text_block_count,
            "thinkingBlockCount": state.thinking_block_count,
            "nativeToolUseBlockCount": state.native_tool_use_block_count,
            "nativeToolUseNames": state.native_tool_names.iter().cloned().collect::<Vec<_>>(),
            "otherBlockCount": state.other_block_count,
            "textualInvokeBlockCount": state.textual_invoke_block_count,
            "textualInvokeCallCount": state.textual_invoke_call_count,
            "textualInvokeToolNames": state.textual_invoke_tool_names.iter().cloned().collect::<Vec<_>>()
        }),
    );
}

fn stream_terminal_status(failed: bool, completed: bool) -> &'static str {
    if failed {
        "response_failed"
    } else if completed {
        "response_completed"
    } else {
        "missing_terminal_event"
    }
}

fn log_stream_upstream_error_event(
    response_protocol: &'static str,
    diagnostic_id: Option<&str>,
    model: &str,
    event_name: Option<&str>,
    error_type: Option<&str>,
    message: &str,
    chunk: &Value,
) {
    let error = chunk.get("error").unwrap_or(chunk);
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.stream_upstream_error_event",
        json!({
            "diagnosticId": diagnostic_id,
            "responseProtocol": response_protocol,
            "model": model,
            "event": event_name,
            "errorType": error_type,
            "errorMessage": diagnostic_text_preview(message),
            "errorPreview": diagnostic_text_preview(&error.to_string()),
        }),
    );
}

fn log_stream_conversion_failure(
    response_protocol: &'static str,
    diagnostic_id: Option<&str>,
    model: &str,
    source: &'static str,
    error_type: Option<&str>,
    message: &str,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.stream_conversion_failure",
        json!({
            "diagnosticId": diagnostic_id,
            "responseProtocol": response_protocol,
            "model": model,
            "source": source,
            "errorType": error_type,
            "errorMessage": diagnostic_text_preview(message),
        }),
    );
}

fn log_anthropic_textual_invoke_detected(
    stream: bool,
    model: &str,
    block_index: Option<usize>,
    call_count: usize,
    tool_names: Vec<String>,
    text_length: usize,
    diagnostic_id: Option<&str>,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.anthropic_textual_invoke_detected",
        json!({
            "diagnosticId": diagnostic_id,
            "stream": stream,
            "model": model,
            "blockIndex": block_index,
            "callCount": call_count,
            "toolNames": tool_names,
            "textLength": text_length
        }),
    );
}

fn log_anthropic_text_tool_marker_detected(
    stream: bool,
    model: &str,
    block_index: Option<usize>,
    marker_kinds: Vec<String>,
    parsed_invoke: bool,
    text_length: usize,
    text: &str,
    diagnostic_id: Option<&str>,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.anthropic_text_tool_marker_detected",
        json!({
            "diagnosticId": diagnostic_id,
            "stream": stream,
            "model": model,
            "blockIndex": block_index,
            "markerKinds": marker_kinds,
            "parsedInvoke": parsed_invoke,
            "textLength": text_length,
            "preview": diagnostic_text_preview(text),
        }),
    );
}

fn output_item_type_counts(output: &[Value]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in output {
        let item_type = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *counts.entry(item_type).or_default() += 1;
    }
    counts
}

fn reasoning_source_label(body: &Value) -> &'static str {
    if body
        .pointer("/reasoning/effort")
        .and_then(Value::as_str)
        .is_some()
    {
        "reasoning.effort"
    } else if body
        .get("model_reasoning_effort")
        .and_then(Value::as_str)
        .is_some()
    {
        "model_reasoning_effort"
    } else if body
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .is_some()
    {
        "reasoning_effort"
    } else if body.get("reasoning").and_then(Value::as_str).is_some() {
        "reasoning_string"
    } else if body.get("reasoning").is_some() {
        "reasoning_object"
    } else {
        "absent"
    }
}

fn log_responses_request_metadata(
    request_json: &Value,
    body_bytes: usize,
    is_stream: bool,
    diagnostic_id: &str,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.responses_request_metadata",
        json!({
            "diagnosticId": diagnostic_id,
            "model": request_json.get("model").and_then(Value::as_str).unwrap_or(""),
            "reasoningEffort": extract_requested_reasoning_effort(request_json).unwrap_or_default(),
            "reasoningSource": reasoning_source_label(request_json),
            "serviceTier": request_json.get("service_tier").and_then(Value::as_str).unwrap_or(""),
            "stream": is_stream,
            "bodyBytes": body_bytes
        }),
    );
}

fn textual_invoke_calls_to_response_items(
    calls: Vec<TextualInvokeToolCall>,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            response_tool_call_item(
                &format!("call_textual_invoke_{index}"),
                &call.name,
                &canonical_json_string(&Value::Object(call.arguments)),
                tool_context,
            )
        })
        .collect()
}

#[derive(Debug)]
struct TextualInvokeToolCall {
    name: String,
    arguments: serde_json::Map<String, Value>,
}

/// 把一段文本切成「前导正文」和「文本化工具调用」两部分。
///
/// 模型有时会先输出正常正文，再在结尾追加 `<invoke>` 形式的工具调用。
/// 该函数定位第一个工具调用起点（含可选的 course/codex/call 前缀标记），
/// 起点之前作为正文返回，起点之后交给 `parse_textual_invoke_tool_calls` 解析。
/// 若起点之后无法解析为完整工具调用，则视为普通文本，返回 `None`。
fn split_text_into_message_and_tool_calls(
    text: &str,
) -> Option<(String, Vec<TextualInvokeToolCall>)> {
    let mut search_from = 0;
    while let Some(relative_pos) = text[search_from..].find("<invoke") {
        let invoke_pos = search_from + relative_pos;
        let start = textual_invoke_region_start_before(text, invoke_pos);
        if let Some(calls) = parse_textual_invoke_tool_calls(&text[start..]) {
            let leading = text[..start].trim_end();
            return Some((leading.to_string(), calls));
        }
        search_from = invoke_pos + "<invoke".len();
    }
    None
}

fn textual_invoke_region_start_before(text: &str, invoke_pos: usize) -> usize {
    let trimmed_before = text[..invoke_pos].trim_end();
    for marker in ["course", "codex", "call"] {
        if let Some(prefix) = trimmed_before.strip_suffix(marker) {
            // 前缀标记必须是独立词：其前面要么是文本开头，要么是空白/换行。
            if prefix.is_empty() || prefix.ends_with(char::is_whitespace) {
                return prefix.len();
            }
        }
    }
    invoke_pos
}

fn parse_textual_invoke_tool_calls(text: &str) -> Option<Vec<TextualInvokeToolCall>> {
    let mut rest = strip_textual_invoke_prefix(text.trim());
    if !rest.starts_with("<invoke") {
        return None;
    }

    let mut calls = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with("<invoke") {
            return None;
        }

        let open_end = rest.find('>')?;
        let open_tag = &rest[..=open_end];
        let name = xml_attribute(open_tag, "name")?;
        if name.trim().is_empty() {
            return None;
        }

        let after_open = &rest[open_end + 1..];
        let close_start = after_open.find("</invoke>")?;
        let body = &after_open[..close_start];
        let after_close = &after_open[close_start + "</invoke>".len()..];

        calls.push(TextualInvokeToolCall {
            name: name.trim().to_string(),
            arguments: parse_textual_invoke_parameters(name.trim(), body)?,
        });

        rest = after_close.trim_start();
    }

    (!calls.is_empty()).then_some(calls)
}

fn strip_textual_invoke_prefix(text: &str) -> &str {
    for marker in ["course", "codex", "call"] {
        if let Some(rest) = text.strip_prefix(marker) {
            if rest.trim_start().starts_with("<invoke") {
                return rest.trim_start();
            }
        }
    }
    text
}

/// 流式场景：当文本块尚未出现 `<invoke` 时，计算「可安全透传」的前缀字节长度。
///
/// 尾部可能是下一个工具调用起点的开头（`<invoke` 的不完整前缀，或 course/codex/call
/// 标记的前缀），这部分必须保留继续缓冲；其余前缀可以作为正文先输出。
fn textual_invoke_safe_passthrough_len(buffer: &str) -> usize {
    // 尾部最多保留这么多字节用于拼接检测（足以容纳 "course" + 空白 + "<invoke"）。
    const MAX_PENDING_TAIL: usize = 16;
    let start = buffer.len().saturating_sub(MAX_PENDING_TAIL);
    // 从距尾部 MAX_PENDING_TAIL 处向后逐个字节边界尝试，找到最早的「可能是起点」位置。
    for candidate in start..=buffer.len() {
        if !buffer.is_char_boundary(candidate) {
            continue;
        }
        let at_text_boundary = candidate == 0
            || buffer[..candidate]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
        if textual_invoke_tail_may_start_call(&buffer[candidate..], at_text_boundary) {
            return candidate;
        }
    }
    buffer.len()
}

/// 尾部子串是否可能是一个尚未输完的工具调用起点（`<invoke` 或 marker 前缀）。
fn textual_invoke_tail_may_start_call(tail: &str, at_text_boundary: bool) -> bool {
    if tail.is_empty() {
        return false;
    }
    // 尾部是 `<invoke` 的前缀（包括只有 `<`）。
    if "<invoke".starts_with(tail) {
        return true;
    }
    // 尾部是 course/codex/call 标记的前缀，或标记后跟着空白/`<invoke` 前缀。
    if !at_text_boundary {
        return false;
    }
    for marker in ["course", "codex", "call"] {
        if marker.starts_with(tail) {
            return true;
        }
        if let Some(rest) = tail.strip_prefix(marker) {
            let trimmed = rest.trim_start();
            // marker 后面必须是空白起头（表明 marker 是独立词）。
            if rest.len() != trimmed.len() || rest.is_empty() {
                if trimmed.is_empty() || "<invoke".starts_with(trimmed) {
                    return true;
                }
            }
        }
    }
    false
}

fn text_tool_marker_kinds(text: &str) -> Vec<String> {
    let mut kinds = BTreeSet::new();
    let trimmed = text.trim_start();
    let stripped = strip_textual_invoke_prefix(trimmed);
    if stripped.starts_with("<invoke") {
        kinds.insert("xmlInvokeAtStart".to_string());
    }
    if trimmed.contains("<invoke") {
        kinds.insert("xmlInvoke".to_string());
    }
    for marker in ["course", "codex", "call"] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            if rest.trim_start().starts_with("<invoke") {
                kinds.insert(format!("{marker}Prefix"));
            }
        }
    }
    if trimmed.starts_with("tool_use:") || trimmed.contains("\ntool_use:") {
        kinds.insert("toolUseText".to_string());
    }
    kinds.into_iter().collect()
}

fn is_standalone_textual_tool_marker(text: &str) -> bool {
    matches!(text.trim(), "course" | "codex" | "call")
}

fn diagnostic_text_preview(text: &str) -> String {
    const PREVIEW_LIMIT: usize = 240;
    let mut preview = String::new();
    for ch in text.chars().take(PREVIEW_LIMIT) {
        preview.push(ch);
    }
    if text.chars().count() > PREVIEW_LIMIT {
        preview.push_str("...");
    }
    preview
}

fn parse_textual_invoke_parameters(
    tool_name: &str,
    body: &str,
) -> Option<serde_json::Map<String, Value>> {
    let mut rest = body;
    let mut arguments = serde_json::Map::new();

    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if !rest.starts_with("<parameter") {
            return None;
        }

        let open_end = rest.find('>')?;
        let open_tag = &rest[..=open_end];
        let name = xml_attribute(open_tag, "name")?;
        if name.trim().is_empty() {
            return None;
        }

        let after_open = &rest[open_end + 1..];
        let close_start = after_open.find("</parameter>")?;
        let parameter_name = name.trim();
        let value = xml_unescape_text(after_open[..close_start].trim());
        let value = if textual_invoke_parameter_is_json(tool_name, parameter_name) {
            serde_json::from_str::<Value>(&value).unwrap_or(Value::String(value))
        } else {
            Value::String(value)
        };
        arguments.insert(parameter_name.to_string(), value);
        rest = &after_open[close_start + "</parameter>".len()..];
    }

    Some(arguments)
}

fn textual_invoke_parameter_is_json(tool_name: &str, parameter_name: &str) -> bool {
    matches!(
        (tool_name, parameter_name),
        ("apply_patch_update_file", "hunks") | ("apply_patch_batch", "operations")
    )
}

fn xml_attribute(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let prefix = format!("{name}={quote}");
        if let Some(start) = tag.find(&prefix) {
            let value_start = start + prefix.len();
            let value_end = tag[value_start..].find(quote)? + value_start;
            return Some(tag[value_start..value_end].to_string());
        }
    }
    None
}

fn xml_unescape_text(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    if !after_ws.starts_with(THINK_OPEN_TAG) {
        return None;
    }
    let body_start = leading_ws_len + THINK_OPEN_TAG.len();
    let close_relative = text[body_start..].find(THINK_CLOSE_TAG)?;
    let close_start = body_start + close_relative;
    let answer_start = close_start + THINK_CLOSE_TAG.len();
    Some((
        text[body_start..close_start].trim().to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws
        .strip_prefix(THINK_OPEN_TAG)
        .map(|value| value.trim().to_string())
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}

fn extract_reasoning_field_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(reasoning) = value.get("reasoning") {
        for key in ["content", "text", "summary"] {
            if let Some(text) = reasoning.get(key).and_then(Value::as_str) {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    value
        .get("reasoning_details")
        .and_then(extract_reasoning_details_text)
}

fn extract_reasoning_details_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(extract_reasoning_detail_part_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(_) => extract_reasoning_detail_part_text(value),
        _ => None,
    }
}

fn extract_reasoning_detail_part_text(value: &Value) -> Option<String> {
    for key in ["text", "content", "summary"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(parts) = value.get("parts").and_then(Value::as_array) {
        let text = parts
            .iter()
            .filter_map(extract_reasoning_detail_part_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        return (!text.is_empty()).then_some(text);
    }

    None
}

fn extract_reasoning_summary_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "content", "text"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    let summary = value.get("summary")?;
    if let Some(text) = summary.as_str() {
        return (!text.is_empty()).then(|| text.to_string());
    }

    let parts = summary.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .or_else(|| part.get("content").and_then(Value::as_str))
                .or_else(|| part.as_str())
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    (!text.is_empty()).then_some(text)
}

fn default_responses_usage() -> Value {
    json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 })
}

fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object() && !value.is_null()) else {
        return default_responses_usage();
    };
    let mut input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .or_else(|| usage.get("promptTokenCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut input_tokens_include_cache = usage.get("prompt_tokens").is_some();
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .or_else(|| usage.get("candidatesTokenCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut cached_tokens = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cachedContentTokenCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation_5m = usage
        .get("cache_creation_5m_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation_1h = usage
        .get("cache_creation_1h_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let has_claude_cache_fields = usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some()
        || usage.get("cache_creation_5m_input_tokens").is_some()
        || usage.get("cache_creation_1h_input_tokens").is_some();
    let has_cache_details = cached_tokens > 0
        || usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .is_some()
        || usage
            .pointer("/input_tokens_details/cached_tokens")
            .is_some();

    if let Some(value) = usage.get("input_tokens").and_then(Value::as_u64) {
        input_tokens = value;
        input_tokens_include_cache = false;
    }
    if let Some(cache_read) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
        cached_tokens = cache_read;
    }
    if let Some(prompt_tokens) = usage.get("promptTokenCount").and_then(Value::as_u64) {
        cached_tokens = usage
            .get("cachedContentTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        input_tokens = prompt_tokens.saturating_sub(cached_tokens);
        input_tokens_include_cache = false;
    }

    let usage_input_tokens = if input_tokens_include_cache {
        input_tokens.saturating_sub(
            cached_tokens
                + effective_cache_creation_tokens(
                    cache_creation,
                    cache_creation_5m,
                    cache_creation_1h,
                ),
        )
    } else {
        input_tokens
    };
    let should_recalculate_total = usage.get("total_tokens").is_none()
        || cached_tokens > 0
        || effective_cache_creation_tokens(cache_creation, cache_creation_5m, cache_creation_1h)
            > 0
        || usage.get("promptTokenCount").is_some();
    let total_tokens = if should_recalculate_total {
        usage_input_tokens
            + output_tokens
            + cached_tokens
            + effective_cache_creation_tokens(cache_creation, cache_creation_5m, cache_creation_1h)
    } else {
        usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(usage_input_tokens + output_tokens)
    };
    let mut result = json!({
        "input_tokens": usage_input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens
    });

    if !has_claude_cache_fields && has_cache_details && cached_tokens > 0 {
        result["input_tokens_details"] = json!({ "cached_tokens": cached_tokens });
    }
    if let Some(details) = usage.get("completion_tokens_details") {
        result["output_tokens_details"] = details.clone();
    }
    if let Some(cache_read) = usage.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = cache_read.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = cache_creation.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_5m_input_tokens") {
        result["cache_creation_5m_input_tokens"] = cache_creation.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_1h_input_tokens") {
        result["cache_creation_1h_input_tokens"] = cache_creation.clone();
    }
    let cache_ttl = match (cache_creation_5m > 0, cache_creation_1h > 0) {
        (true, true) => Some("mixed"),
        (true, false) => Some("5m"),
        (false, true) => Some("1h"),
        (false, false) => None,
    };
    if let Some(cache_ttl) = cache_ttl {
        result["cache_ttl"] = json!(cache_ttl);
    }
    result
}

fn anthropic_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    chat_usage_to_responses_usage(usage)
}

fn effective_cache_creation_tokens(
    cache_creation: u64,
    cache_creation_5m: u64,
    cache_creation_1h: u64,
) -> u64 {
    if cache_creation > 0 {
        cache_creation
    } else {
        cache_creation_5m + cache_creation_1h
    }
}

fn response_status(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    }
}

fn anthropic_response_status(stop_reason: Option<&str>) -> &'static str {
    match stop_reason {
        Some("max_tokens") => "incomplete",
        _ => "completed",
    }
}

fn anthropic_stop_reason_to_chat_finish_reason(stop_reason: Option<&str>) -> Option<&str> {
    match stop_reason {
        Some("max_tokens") => Some("length"),
        Some("end_turn") | Some("stop_sequence") => Some("stop"),
        Some("tool_use") => Some("tool_calls"),
        Some(value) => Some(value),
        None => None,
    }
}

fn response_output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => canonical_json_string(other),
    }
}

fn tool_search_arguments_from_value(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => tool_search_arguments_from_argument_string(text),
        Some(Value::Object(_)) => value.cloned().unwrap_or_else(|| json!({})),
        Some(Value::Null) | None => json!({}),
        Some(other) => json!({ "query": response_output_text(other) }),
    }
}

fn tool_search_arguments_from_argument_string(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return json!({});
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(Value::String(text)) => json!({ "query": text }),
        Ok(other) => json!({ "query": response_output_text(&other) }),
        Err(_) => json!({ "query": arguments }),
    }
}

fn tool_search_output_text(item: &Value) -> String {
    if let Some(output) = item.get("output") {
        return response_output_text(output);
    }
    if let Some(tools) = item.get("tools") {
        return response_output_text(tools);
    }
    response_output_text(item)
}

fn build_custom_tool_call_history(name: &str, input: &Value) -> (String, String) {
    let input = response_output_text(input);
    if name == "apply_patch" || input.starts_with("*** Begin Patch") {
        let operations = parse_apply_patch_operations(&input);
        if operations.len() == 1 {
            let action = operations[0]
                .get("type")
                .and_then(Value::as_str)
                .and_then(single_apply_patch_action)
                .unwrap_or(CodexPatchProxyAction::Batch);
            return (
                format!("{name}_{}", action.suffix()),
                build_apply_patch_operation_arguments(&operations[0], action),
            );
        }
        return (
            format!("{name}_batch"),
            json!({ "operations": operations, "raw_patch": input }).to_string(),
        );
    }
    (name.to_string(), json!({ "input": input }).to_string())
}

fn reconstruct_custom_tool_call_input_with_context(
    tool_context: &CodexToolContext,
    upstream_name: &str,
    arguments: &str,
) -> String {
    if let Some(spec) = tool_context.custom_tools.get(upstream_name) {
        if spec.kind == CodexCustomToolKind::ApplyPatch {
            return reconstruct_apply_patch_input(spec.proxy_action, arguments);
        }
    }
    reconstruct_custom_tool_call_input(arguments)
}

fn reconstruct_custom_tool_call_input(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(arguments) else {
        return arguments.to_string();
    };
    value
        .get("input")
        .map(response_output_text)
        .unwrap_or_else(|| arguments.to_string())
}

fn reconstruct_apply_patch_input(action: Option<CodexPatchProxyAction>, arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(arguments) else {
        return arguments.to_string();
    };
    if let Some(raw_patch) = value
        .get("raw_patch")
        .or_else(|| value.get("patch"))
        .or_else(|| value.get("input"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return raw_patch.to_string();
    }

    let operations = match action.unwrap_or(CodexPatchProxyAction::Batch) {
        CodexPatchProxyAction::AddFile => vec![json!({
            "type": "add_file",
            "path": value.get("path").and_then(Value::as_str).unwrap_or(""),
            "content": value.get("content").and_then(Value::as_str).unwrap_or("")
        })],
        CodexPatchProxyAction::DeleteFile => vec![json!({
            "type": "delete_file",
            "path": value.get("path").and_then(Value::as_str).unwrap_or("")
        })],
        CodexPatchProxyAction::UpdateFile => vec![json!({
            "type": "update_file",
            "path": value.get("path").and_then(Value::as_str).unwrap_or(""),
            "move_to": value.get("move_to").and_then(Value::as_str).unwrap_or(""),
            "hunks": value.get("hunks").cloned().unwrap_or_else(|| json!([]))
        })],
        CodexPatchProxyAction::ReplaceFile => vec![json!({
            "type": "replace_file",
            "path": value.get("path").and_then(Value::as_str).unwrap_or(""),
            "content": value.get("content").and_then(Value::as_str).unwrap_or("")
        })],
        CodexPatchProxyAction::Batch => value
            .get("operations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    };

    build_apply_patch_text(&operations)
}

fn build_apply_patch_text(operations: &[Value]) -> String {
    let mut text = String::from("*** Begin Patch");
    for operation in operations {
        let op_type = operation.get("type").and_then(Value::as_str).unwrap_or("");
        let path = operation.get("path").and_then(Value::as_str).unwrap_or("");
        match op_type {
            "add_file" => {
                text.push_str(&format!("\n*** Add File: {path}"));
                for line in operation
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .lines()
                {
                    text.push_str("\n+");
                    text.push_str(line);
                }
            }
            "delete_file" => {
                text.push_str(&format!("\n*** Delete File: {path}"));
            }
            "update_file" => {
                text.push_str(&format!("\n*** Update File: {path}"));
                if let Some(move_to) = operation.get("move_to").and_then(Value::as_str) {
                    if !move_to.is_empty() {
                        text.push_str(&format!("\n*** Move to: {move_to}"));
                    }
                }
                if let Some(hunks) = operation.get("hunks").and_then(Value::as_array) {
                    for hunk in hunks {
                        let context = hunk.get("context").and_then(Value::as_str).unwrap_or("");
                        if context.is_empty() {
                            text.push_str("\n@@");
                        } else {
                            text.push_str(&format!("\n@@ {context}"));
                        }
                        if let Some(lines) = hunk.get("lines").and_then(Value::as_array) {
                            for line in lines {
                                text.push('\n');
                                text.push_str(line_op_prefix(
                                    line.get("op").and_then(Value::as_str).unwrap_or("context"),
                                ));
                                text.push_str(
                                    line.get("text").and_then(Value::as_str).unwrap_or(""),
                                );
                            }
                        }
                    }
                }
            }
            "replace_file" => {
                text.push_str(&format!("\n*** Delete File: {path}"));
                text.push_str(&format!("\n*** Add File: {path}"));
                for line in operation
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .lines()
                {
                    text.push_str("\n+");
                    text.push_str(line);
                }
            }
            _ => {}
        }
    }
    text.push_str("\n*** End Patch");
    text
}

fn line_op_prefix(op: &str) -> &'static str {
    match op {
        "add" => "+",
        "remove" | "delete" => "-",
        _ => " ",
    }
}

fn parse_apply_patch_operations(input: &str) -> Vec<Value> {
    let mut operations = Vec::new();
    let mut current: Option<serde_json::Map<String, Value>> = None;
    let mut content_lines: Vec<String> = Vec::new();
    let mut hunks: Vec<Value> = Vec::new();
    let mut current_hunk: Option<serde_json::Map<String, Value>> = None;
    let mut hunk_lines: Vec<Value> = Vec::new();

    let flush_hunk = |current_hunk: &mut Option<serde_json::Map<String, Value>>,
                      hunk_lines: &mut Vec<Value>,
                      hunks: &mut Vec<Value>| {
        if let Some(mut hunk) = current_hunk.take() {
            hunk.insert("lines".to_string(), json!(std::mem::take(hunk_lines)));
            hunks.push(Value::Object(hunk));
        }
    };
    let flush_operation = |current: &mut Option<serde_json::Map<String, Value>>,
                           content_lines: &mut Vec<String>,
                           hunks: &mut Vec<Value>,
                           operations: &mut Vec<Value>| {
        if let Some(mut operation) = current.take() {
            match operation.get("type").and_then(Value::as_str).unwrap_or("") {
                "add_file" | "replace_file" => {
                    operation.insert("content".to_string(), json!(content_lines.join("\n")));
                }
                "update_file" => {
                    operation.insert("hunks".to_string(), json!(std::mem::take(hunks)));
                }
                _ => {}
            }
            content_lines.clear();
            operations.push(Value::Object(operation));
        }
    };

    for raw_line in input.lines() {
        if raw_line == "*** Begin Patch" || raw_line == "*** End Patch" {
            continue;
        }
        if let Some(path) = raw_line.strip_prefix("*** Add File: ") {
            flush_hunk(&mut current_hunk, &mut hunk_lines, &mut hunks);
            flush_operation(
                &mut current,
                &mut content_lines,
                &mut hunks,
                &mut operations,
            );
            current = Some(serde_json::Map::from_iter([
                ("type".to_string(), json!("add_file")),
                ("path".to_string(), json!(path)),
            ]));
            continue;
        }
        if let Some(path) = raw_line.strip_prefix("*** Delete File: ") {
            flush_hunk(&mut current_hunk, &mut hunk_lines, &mut hunks);
            flush_operation(
                &mut current,
                &mut content_lines,
                &mut hunks,
                &mut operations,
            );
            current = Some(serde_json::Map::from_iter([
                ("type".to_string(), json!("delete_file")),
                ("path".to_string(), json!(path)),
            ]));
            continue;
        }
        if let Some(path) = raw_line.strip_prefix("*** Update File: ") {
            flush_hunk(&mut current_hunk, &mut hunk_lines, &mut hunks);
            flush_operation(
                &mut current,
                &mut content_lines,
                &mut hunks,
                &mut operations,
            );
            current = Some(serde_json::Map::from_iter([
                ("type".to_string(), json!("update_file")),
                ("path".to_string(), json!(path)),
            ]));
            continue;
        }
        if let Some(path) = raw_line.strip_prefix("*** Move to: ") {
            if let Some(operation) = current.as_mut() {
                operation.insert("move_to".to_string(), json!(path));
            }
            continue;
        }
        if raw_line.starts_with("@@") {
            flush_hunk(&mut current_hunk, &mut hunk_lines, &mut hunks);
            let context = raw_line.strip_prefix("@@").unwrap_or("").trim().to_string();
            current_hunk = Some(serde_json::Map::from_iter([(
                "context".to_string(),
                json!(context),
            )]));
            continue;
        }
        if let Some(operation) = current.as_ref() {
            match operation.get("type").and_then(Value::as_str).unwrap_or("") {
                "add_file" | "replace_file" => {
                    if let Some(line) = raw_line.strip_prefix('+') {
                        content_lines.push(line.to_string());
                    }
                }
                "update_file" => {
                    let (op, text) = match raw_line.chars().next() {
                        Some('+') => ("add", &raw_line[1..]),
                        Some('-') => ("remove", &raw_line[1..]),
                        Some(' ') => ("context", &raw_line[1..]),
                        _ => ("context", raw_line),
                    };
                    hunk_lines.push(json!({ "op": op, "text": text }));
                }
                _ => {}
            }
        }
    }

    flush_hunk(&mut current_hunk, &mut hunk_lines, &mut hunks);
    flush_operation(
        &mut current,
        &mut content_lines,
        &mut hunks,
        &mut operations,
    );
    operations
}

fn single_apply_patch_action(op_type: &str) -> Option<CodexPatchProxyAction> {
    match op_type {
        "add_file" => Some(CodexPatchProxyAction::AddFile),
        "delete_file" => Some(CodexPatchProxyAction::DeleteFile),
        "update_file" => Some(CodexPatchProxyAction::UpdateFile),
        "replace_file" => Some(CodexPatchProxyAction::ReplaceFile),
        _ => None,
    }
}

fn build_apply_patch_operation_arguments(
    operation: &Value,
    action: CodexPatchProxyAction,
) -> String {
    match action {
        CodexPatchProxyAction::AddFile | CodexPatchProxyAction::ReplaceFile => json!({
            "content": operation.get("content").and_then(Value::as_str).unwrap_or(""),
            "path": operation.get("path").and_then(Value::as_str).unwrap_or("")
        })
        .to_string(),
        CodexPatchProxyAction::DeleteFile => json!({
            "path": operation.get("path").and_then(Value::as_str).unwrap_or("")
        })
        .to_string(),
        CodexPatchProxyAction::UpdateFile => {
            let mut args = json!({
                "hunks": operation.get("hunks").cloned().unwrap_or_else(|| json!([])),
                "path": operation.get("path").and_then(Value::as_str).unwrap_or("")
            });
            if let Some(move_to) = operation.get("move_to").and_then(Value::as_str) {
                if !move_to.is_empty() {
                    args["move_to"] = json!(move_to);
                }
            }
            args.to_string()
        }
        CodexPatchProxyAction::Batch => json!({ "operations": [operation.clone()] }).to_string(),
    }
}

fn copy_response_request_fields(response: &mut Value, original_request: Option<&Value>) {
    let Some(original_request) = original_request else {
        return;
    };
    for key in [
        "instructions",
        "max_output_tokens",
        "parallel_tool_calls",
        "previous_response_id",
        "reasoning",
        "temperature",
        "tool_choice",
        "tools",
        "top_p",
        "metadata",
    ] {
        if let Some(value) = original_request.get(key) {
            response[key] = value.clone();
        }
    }
}

fn responses_arguments_to_chat(value: &Value) -> String {
    match value {
        Value::String(text) => normalize_chat_tool_arguments_string(text),
        Value::Object(_) => canonical_json_string(value),
        Value::Null => "{}".to_string(),
        other => canonical_json_string(&json!({ "input": other })),
    }
}

fn normalize_chat_tool_arguments_string(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "{}".to_string();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(_)) => trimmed.to_string(),
        Ok(value) => canonical_json_string(&json!({ "input": value })),
        Err(_) => canonical_json_string(&json!({ "input": text })),
    }
}

fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
        Value::Array(values) => {
            let parts = values.iter().map(canonical_json_string).collect::<Vec<_>>();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            let parts = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).unwrap_or_default();
                    format!("{key}:{}", canonical_json_string(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(","))
        }
    }
}

fn apply_chat_reasoning_options(result: &mut Value, body: &Value, model: &str) {
    let Some(reasoning_enabled) = reasoning_requested(body) else {
        return;
    };
    let style = infer_chat_reasoning_style(model);

    match style {
        ChatReasoningStyle::Thinking => {
            result["thinking"] = json!({
                "type": if reasoning_enabled { "enabled" } else { "disabled" }
            });
        }
        ChatReasoningStyle::EnableThinking => {
            result["enable_thinking"] = json!(reasoning_enabled);
        }
        ChatReasoningStyle::ReasoningSplit => {
            result["reasoning_split"] = json!(reasoning_enabled);
        }
        _ => {}
    }

    if !reasoning_enabled {
        if style == ChatReasoningStyle::OpenRouter {
            result["reasoning"] = json!({ "effort": "none" });
        }
        return;
    }

    let Some(effort) = extract_requested_reasoning_effort(body) else {
        return;
    };
    let Some(mapped) = map_chat_reasoning_effort(&effort, style, model) else {
        return;
    };

    match style {
        ChatReasoningStyle::OpenRouter => {
            result["reasoning"] = json!({ "effort": mapped });
        }
        ChatReasoningStyle::DeepSeek
        | ChatReasoningStyle::LowHigh
        | ChatReasoningStyle::Default
            if supports_reasoning_effort(model)
                || model.to_ascii_lowercase().contains("claude") =>
        {
            // Claude 模型经 Chat Completions 协议转发时也透传 reasoning_effort，
            // 由上游/中转站映射为 output_config.effort，避免协议转换丢失思考深度
            result["reasoning_effort"] = json!(mapped);
        }
        _ => {}
    }
}

fn apply_anthropic_reasoning_options(result: &mut Value, body: &Value, model: &str) {
    if !model.to_ascii_lowercase().contains("claude") {
        return;
    }
    let reasoning_enabled = reasoning_requested(body).unwrap_or(true);
    if !reasoning_enabled {
        result["thinking"] = json!({ "type": "disabled" });
        return;
    }
    let effort = extract_requested_reasoning_effort(body)
        .and_then(|effort| map_anthropic_reasoning_effort(&effort, model))
        .unwrap_or(ANTHROPIC_DEFAULT_REASONING_EFFORT);
    result["thinking"] = json!({ "type": "adaptive" });
    result["output_config"] = json!({ "effort": effort });
}

/// 从请求体的多个可能位置提取思考深度字符串。
/// App 在不同模式下可能把 effort 放在 reasoning.effort、顶层 reasoning（字符串）、
/// model_reasoning_effort、reasoning_effort，需统一兜底读取，避免协议代理丢失思考深度。
fn extract_requested_reasoning_effort(body: &Value) -> Option<String> {
    if let Some(effort) = body.pointer("/reasoning/effort").and_then(Value::as_str) {
        let trimmed = effort.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    for key in ["model_reasoning_effort", "reasoning_effort"] {
        if let Some(effort) = body.get(key).and_then(Value::as_str) {
            let trimmed = effort.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    // 顶层 reasoning 也可能直接是字符串（如 "high"）
    if let Some(effort) = body.get("reasoning").and_then(Value::as_str) {
        let trimmed = effort.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn reasoning_requested(body: &Value) -> Option<bool> {
    if let Some(effort) = extract_requested_reasoning_effort(body) {
        return Some(!matches!(
            effort.trim().to_ascii_lowercase().as_str(),
            "none" | "off" | "disabled"
        ));
    }

    // reasoning 为对象（非 null）时视为开启；为 null 时返回 None（交由调用方决定默认行为）
    match body.get("reasoning") {
        Some(Value::Null) | None => None,
        Some(_) => Some(true),
    }
}

fn infer_chat_reasoning_style(model: &str) -> ChatReasoningStyle {
    let model = model.to_ascii_lowercase();
    if model.contains("openrouter") || model.starts_with("openrouter/") {
        return ChatReasoningStyle::OpenRouter;
    }
    if model.contains("deepseek") {
        return ChatReasoningStyle::DeepSeek;
    }
    if model.contains("qwen") || model.contains("dashscope") || model.contains("bailian") {
        return ChatReasoningStyle::EnableThinking;
    }
    if model.contains("kimi")
        || model.contains("moonshot")
        || model.contains("glm")
        || model.contains("zhipu")
        || model.contains("z.ai")
        || model.contains("mimo")
    {
        return ChatReasoningStyle::Thinking;
    }
    if model.contains("minimax") {
        return ChatReasoningStyle::ReasoningSplit;
    }
    if model.contains("siliconflow") {
        return ChatReasoningStyle::EnableThinking;
    }
    if model.contains("stepfun") || model.contains("step-3.5-flash-2603") {
        return ChatReasoningStyle::LowHigh;
    }
    ChatReasoningStyle::Default
}

fn map_chat_reasoning_effort(
    effort: &str,
    style: ChatReasoningStyle,
    model: &str,
) -> Option<&'static str> {
    let mut effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }
    if style == ChatReasoningStyle::DeepSeek && effort == "xhigh" {
        effort = "max".to_string();
    }

    match style {
        ChatReasoningStyle::OpenRouter => {
            let mapped = clamp_reasoning_effort_for_model(
                &effort,
                model,
                UpstreamResponseProtocol::ChatCompletions,
            )?;
            Some(if mapped == "max" { "xhigh" } else { mapped })
        }
        _ => clamp_reasoning_effort_for_model(
            &effort,
            model,
            UpstreamResponseProtocol::ChatCompletions,
        ),
    }
}

fn map_anthropic_reasoning_effort(effort: &str, model: &str) -> Option<&'static str> {
    clamp_reasoning_effort_for_model(effort, model, UpstreamResponseProtocol::Anthropic)
}

pub fn supported_reasoning_efforts_for_model(
    model: &str,
    protocol: UpstreamResponseProtocol,
) -> Vec<&'static str> {
    let model = model.trim().to_ascii_lowercase();
    if model.contains("claude") || protocol == UpstreamResponseProtocol::Anthropic {
        if model.contains("opus-4-7") || model.contains("opus-4-8") || model.contains("fable-5") {
            return levels(&["low", "medium", "high", "xhigh", "max"]);
        }
        if model.contains("opus-4-6") {
            return levels(&["low", "medium", "high", "max"]);
        }
        if model.contains("sonnet-4-6") {
            return levels(&["low", "medium", "high"]);
        }
        return levels(&["low", "medium", "high"]);
    }

    if model.contains("gpt-") || is_openai_o_series(&model) {
        return levels(&["minimal", "low", "medium", "high", "xhigh"]);
    }
    if model.contains("deepseek") {
        return levels(&["high", "max"]);
    }
    if model.contains("glm") || model.contains("zhipu") || model.contains("z.ai") {
        return levels(&["high", "max"]);
    }
    if model.contains("grok") || model.contains("xai") {
        return levels(&["low", "medium", "high"]);
    }
    if model.contains("gemini-3.1-pro") {
        return levels(&["low", "medium", "high"]);
    }
    if model.contains("gemini-3-pro") {
        return levels(&["low", "high"]);
    }
    if model.contains("gemini-3") {
        return levels(&["minimal", "low", "medium", "high"]);
    }
    if model.contains("stepfun") || model.contains("step-3.5-flash-2603") {
        return levels(&["low", "high"]);
    }

    match protocol {
        UpstreamResponseProtocol::Anthropic => levels(&["low", "medium", "high"]),
        _ => levels(&["minimal", "low", "medium", "high", "xhigh"]),
    }
}

fn levels(values: &[&'static str]) -> Vec<&'static str> {
    values.to_vec()
}

fn clamp_reasoning_effort_for_model(
    effort: &str,
    model: &str,
    protocol: UpstreamResponseProtocol,
) -> Option<&'static str> {
    let effort = normalize_reasoning_effort(effort)?;
    let supported = supported_reasoning_efforts_for_model(model, protocol);
    if supported.contains(&effort) {
        return Some(effort);
    }
    let requested_index = reasoning_effort_index(effort)?;
    supported.into_iter().rev().find(|candidate| {
        reasoning_effort_index(candidate).is_some_and(|index| index <= requested_index)
    })
}

fn normalize_reasoning_effort(effort: &str) -> Option<&'static str> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        _ => None,
    }
}

fn reasoning_effort_index(effort: &str) -> Option<usize> {
    REASONING_EFFORT_ORDER
        .iter()
        .position(|candidate| *candidate == effort)
}

fn anthropic_reasoning_compatibility_cache() -> &'static Mutex<BTreeMap<String, String>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn anthropic_model_cache_key(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn apply_cached_anthropic_reasoning_compatibility(request: &mut Value) {
    let Some(model) = request.get("model").and_then(Value::as_str) else {
        return;
    };
    let key = anthropic_model_cache_key(model);
    if key.is_empty() {
        return;
    }
    let fallback = anthropic_reasoning_compatibility_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned());
    let Some(fallback) = fallback else {
        return;
    };
    if request.pointer("/thinking/type").and_then(Value::as_str) != Some("adaptive") {
        return;
    }
    if request
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
        == Some("max")
    {
        request["output_config"]["effort"] = json!(fallback);
    }
}

fn remember_anthropic_reasoning_compatibility(request: &Value, fallback_effort: &str) {
    let Some(model) = request.get("model").and_then(Value::as_str) else {
        return;
    };
    let key = anthropic_model_cache_key(model);
    if key.is_empty() {
        return;
    }
    if let Ok(mut cache) = anthropic_reasoning_compatibility_cache().lock() {
        cache.insert(key, fallback_effort.to_string());
    }
}

pub fn clear_anthropic_reasoning_compatibility_cache_for_tests() {
    if let Ok(mut cache) = anthropic_reasoning_compatibility_cache().lock() {
        cache.clear();
    }
}

fn should_retry_anthropic_max_effort(
    status_code: u16,
    content_type: &str,
    request: &Value,
) -> bool {
    status_code == 400
        && content_type.to_ascii_lowercase().contains("json")
        && request.pointer("/thinking/type").and_then(Value::as_str) == Some("adaptive")
        && request
            .pointer("/output_config/effort")
            .and_then(Value::as_str)
            == Some("max")
}

fn anthropic_effort_fallback_from_error(
    body: &[u8],
    unsupported_effort: &str,
) -> Option<&'static str> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let error = value.get("error").unwrap_or(&value);
    let message = error
        .get("message")
        .or_else(|| error.get("detail"))
        .or_else(|| error.get("error"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let unsupported_effort = unsupported_effort.to_ascii_lowercase();
    if !message.contains(&unsupported_effort)
        || !message.contains("not supported")
        || !message.contains("valid level")
    {
        return None;
    }
    highest_anthropic_effort_in_message(&message)
}

fn highest_anthropic_effort_in_message(message: &str) -> Option<&'static str> {
    for candidate in ["high", "medium", "low"] {
        if message.contains(candidate) {
            return Some(candidate);
        }
    }
    None
}

fn supports_reasoning_effort(model: &str) -> bool {
    is_openai_o_series(model)
        || model
            .to_lowercase()
            .strip_prefix("gpt-")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|ch| ch.is_ascii_digit() && ch >= '5')
        || infer_chat_reasoning_style(model) == ChatReasoningStyle::DeepSeek
        || infer_chat_reasoning_style(model) == ChatReasoningStyle::LowHigh
}

fn is_openai_o_series(model: &str) -> bool {
    model.len() > 1
        && model.starts_with('o')
        && model
            .as_bytes()
            .get(1)
            .is_some_and(|byte| byte.is_ascii_digit())
}
