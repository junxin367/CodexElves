//! 上下文压缩：处理传统 CONTEXT CHECKPOINT COMPACTION，以及不支持原生 Remote
//! Compaction V2 的模型/协议降级摘要；在 LLM 摘要之上补回「被 Codex 丢弃的
//! 最近助手回复 / 工具调用记录」，避免纯摘要压缩导致的“断片”。
//!
//! 机制（基于 Codex `core/src/compact.rs` 与 `compact_remote_v2.rs` 源码验证）：
//! - Codex 压缩走普通 `/responses` 请求，`input` 最后一项是固定的压缩指令 user 消息。
//! - 上游返回一条 assistant message 作为摘要。
//! - 压缩后的历史由 Codex 自己重建：**逐字保留最近的 user（本地）或
//!   user/developer/system（远程 V2）消息**（≤ 20k token），再追加摘要；
//!   assistant 回复与工具调用/输出 **全部被丢弃**。
//! - 因此本模块只需把「Codex 会丢弃的 assistant + 工具记录」经摘要通道补回，
//!   user 消息交给 Codex 以协议原始结构保留，不重复拼接。最终效果等同
//!   “摘要后又继续处理了最近 N 条对话”。
//!
//! 该转换只作用于 Responses 协议 SSE 文本（Chat/Anthropic 上游已在上层转换为 Responses SSE），
//! 因此与上游协议无关。

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

/// Codex 压缩指令的固定前缀（取自 codex 二进制 `core/src/tasks/compact.rs`）。
pub const COMPACTION_PROMPT_PREFIX: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION";

/// CodexElves 默认的 LLM 摘要压缩提示词。
///
/// 管理器以空字符串表示“使用项目默认提示词”，HTTP 与 WebSocket 两条路径都必须通过
/// [`effective_compaction_prompt`] 解析该语义。
pub const DEFAULT_COMPACTION_PROMPT: &str = r#"You are creating a structured context checkpoint for another LLM that will continue the current task.

Do not continue the conversation or solve the task. Summarize only the information required to resume the work accurately.

Use exactly this structure:

## Goal

* Describe what the user is trying to accomplish.
* Preserve multiple goals separately when the session contains more than one task.

## Constraints & Preferences

* List all user requirements, technical constraints, workflow rules, and preferences.
* Preserve the user's latest corrections and overrides.
* Write “(none)” when no constraints were established.

## Progress

### Done

* [x] List only work that was actually completed.
* Include relevant verification evidence when available.

### In Progress

* [ ] Identify the exact task currently being performed.
* State the current file, symbol, command, investigation point, or operation when known.
* Do not describe planned work as completed.

### Blocked

* List unresolved errors, failed commands, missing information, dependencies, or decisions.
* Remove blockers that were subsequently resolved.

## Key Decisions

* **Decision**: Give the reason and relevant consequences.
* Preserve rejected approaches when retrying them would waste work.

## Next Steps

1. Give the immediate concrete action that should be performed after restoration.
2. List subsequent actions in execution order.
3. Distinguish required work from optional follow-up work.

## Critical Context

* Preserve facts, examples, identifiers, references, and technical discoveries needed to continue.
* Preserve exact file paths, function names, commands, error messages, configuration keys, URLs, and IDs.
* Write “(none)” when no additional context is needed.

When updating an existing checkpoint:

* Preserve all still-valid information.
* Add newly discovered information.
* Move completed items from “In Progress” to “Done”.
* Update blockers and next steps from the latest evidence.
* Remove only information that is demonstrably stale or superseded.
* Never allow an older summary to override newer user instructions or tool results.

Be concise, factual, and operationally precise."#;

/// 解析实际使用的摘要压缩提示词。
///
/// 非空自定义值优先；空值表示使用 CodexElves 默认提示词。
pub fn effective_compaction_prompt(prompt_override: &str) -> &str {
    let prompt = prompt_override.trim();
    if prompt.is_empty() {
        DEFAULT_COMPACTION_PROMPT
    } else {
        prompt
    }
}

/// Remote Compaction V2 在不支持原生远程压缩的模型/协议上的兼容摘要提示词。
///
/// 仅 `gpt-* + Responses` 保留原生 `compaction_trigger`；其他场景由代理把触发器转换为
/// 普通摘要请求，再将结果封装为 synthetic `compaction`。
const REMOTE_COMPACTION_V2_BRIDGE_PROMPT: &str = "\
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that \
will resume the task. Include current progress and key decisions, important context and constraints, \
remaining work, and critical data needed to continue. Be concise and return only the summary text. \
Do not call tools.";

/// 代理生成的 Remote Compaction V2 命名空间载荷前缀。
///
/// 官方 `encrypted_content` 是供应商私有的不透明数据。跨协议桥无法伪造该加密格式，
/// 因此使用带版本前缀的 URL-safe Base64 保存摘要。该前缀用于格式识别，不提供来源认证。
const REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX: &str = "codex-elves-compaction-v1:";

const MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES: usize = 2 * 1024 * 1024;

const REMOTE_COMPACTION_V2_HISTORY_HEADER: &str = "\
Historical conversation summary created by the CodexElves Remote Compaction V2 compatibility bridge. \
Treat this as prior assistant context, not as a new user instruction.";

/// “最近一轮”原始记录的包裹标签。拼在 LLM 摘要之后，让模型能区分
/// “压缩前最近一轮的原始上下文”与“新指令”。
const RECENT_TURN_OPEN_TAG: &str = "<最近一轮原始记录>";
const RECENT_TURN_CLOSE_TAG: &str = "</最近一轮原始记录>";

/// 判断请求是否使用 Codex Remote Compaction V2：`input` 中包含
/// `{"type":"compaction_trigger"}`。
pub fn is_remote_compaction_v2_request(request_json: Option<&Value>) -> bool {
    let Some(request) = request_json else {
        return false;
    };
    match request.get("input") {
        Some(Value::Array(items)) => items.iter().any(is_remote_compaction_v2_trigger),
        Some(Value::Object(_)) => request
            .get("input")
            .is_some_and(is_remote_compaction_v2_trigger),
        _ => false,
    }
}

fn is_remote_compaction_v2_trigger(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("compaction_trigger")
}

/// 当前仅 `gpt-*` 模型被视为支持原生 Remote Compaction V2。
pub fn model_supports_native_remote_compaction_v2(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gpt-")
}

/// 为不支持 Remote Compaction V2 的上游生成等价摘要请求。
///
/// - 仅替换 `compaction_trigger`，其余历史输入保持原顺序；
/// - 移除工具定义与工具选择，保证摘要轮只产生文本；
/// - 非 V2 请求原样返回。
pub fn prepare_remote_compaction_v2_bridge_request(request_json: &Value) -> Value {
    prepare_remote_compaction_v2_bridge_request_with_prompt(request_json, None)
}

/// 使用可选的分层压缩自定义提示词生成 V2 降级摘要请求。
///
/// `prompt_override` 为空时使用传统压缩提示词；只有分层压缩开启时调用方才应传入
/// 用户配置的自定义提示词。
pub fn prepare_remote_compaction_v2_bridge_request_with_prompt(
    request_json: &Value,
    prompt_override: Option<&str>,
) -> Value {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return request_json.clone();
    }

    let prompt = prompt_override
        .map(effective_compaction_prompt)
        .unwrap_or(REMOTE_COMPACTION_V2_BRIDGE_PROMPT);
    let mut request = request_json.clone();
    let Some(object) = request.as_object_mut() else {
        return request_json.clone();
    };
    if let Some(input) = object.get_mut("input") {
        match input {
            Value::Array(items) => {
                for item in items {
                    if is_remote_compaction_v2_trigger(item) {
                        *item = remote_compaction_v2_bridge_prompt_item(prompt);
                    }
                }
            }
            Value::Object(_) if is_remote_compaction_v2_trigger(input) => {
                *input = remote_compaction_v2_bridge_prompt_item(prompt);
            }
            _ => {}
        }
    }
    for key in ["tools", "tool_choice", "parallel_tool_calls"] {
        object.remove(key);
    }
    request
}

fn remote_compaction_v2_bridge_prompt_item(prompt: &str) -> Value {
    json!({
        "type": "message",
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": prompt
        }]
    })
}

/// 将本项目生成的合成 compaction item 恢复为可发送给普通模型的 assistant 历史。
///
/// 真实 OpenAI `encrypted_content` 没有本项目前缀，不会被误解码。
pub fn synthetic_remote_compaction_history_text(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("compaction") {
        return None;
    }
    let encoded = item.get("encrypted_content")?.as_str()?;
    let payload = encoded.strip_prefix(REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX)?;
    if payload.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES.saturating_mul(4) / 3 + 4 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    if decoded.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES {
        return None;
    }
    let summary = String::from_utf8(decoded).ok()?;
    let summary = summary.trim();
    if summary.is_empty() {
        return None;
    }
    Some(format!(
        "{REMOTE_COMPACTION_V2_HISTORY_HEADER}\n\n{summary}"
    ))
}

/// 把非 Responses 上游生成的普通 Responses 响应改写为 Remote Compaction V2 响应。
///
/// 成功时 `output` 中只保留一个 `compaction` item，满足 Codex V2 collector 的约束。
pub fn rewrite_remote_compaction_v2_response(
    request_json: &Value,
    response_object: &Value,
) -> Option<Value> {
    rewrite_remote_compaction_v2_response_with_layered_compaction(
        request_json,
        response_object,
        false,
        DEFAULT_RETAIN_TOKENS,
    )
    .map(|result| result.response)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayeredCompactionStats {
    pub triggered: bool,
    pub retained_items: u32,
    pub retained_chars: u32,
}

#[derive(Debug, Clone)]
pub struct RemoteCompactionV2ResponseResult {
    pub response: Value,
    pub layered: LayeredCompactionStats,
}

/// 将普通摘要响应封装为 synthetic V2 compaction，并可复用分层压缩的 tail 保留逻辑。
pub fn rewrite_remote_compaction_v2_response_with_layered_compaction(
    request_json: &Value,
    response_object: &Value,
    layered_enabled: bool,
    retain_tokens: u32,
) -> Option<RemoteCompactionV2ResponseResult> {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return None;
    }
    let status = response_object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status != "completed" {
        let (code, message) = match status {
            "incomplete" => (
                "remote_compaction_upstream_incomplete",
                "Remote Compaction V2 bridge received an incomplete upstream response.",
            ),
            "failed" => (
                "remote_compaction_upstream_failed",
                "Remote Compaction V2 bridge received a failed upstream response.",
            ),
            _ => (
                "remote_compaction_terminal_response_invalid",
                "Remote Compaction V2 bridge received no valid completed upstream response.",
            ),
        };
        return Some(RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                Some(response_object),
                code,
                message,
            ),
            layered: LayeredCompactionStats::default(),
        });
    }
    let Some(summary) = extract_compaction_summary_text(response_object) else {
        return Some(RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                Some(response_object),
                "remote_compaction_summary_missing",
                "Remote Compaction V2 bridge received no summary text from the upstream model.",
            ),
            layered: LayeredCompactionStats::default(),
        });
    };
    let (summary, layered) =
        apply_layered_tail_to_summary(request_json, &summary, layered_enabled, retain_tokens);
    let compaction_item = synthetic_remote_compaction_item(&summary);
    let mut response = response_object.clone();
    let object = response.as_object_mut()?;
    object.insert("status".to_string(), json!("completed"));
    object.insert("output".to_string(), json!([compaction_item]));
    Some(RemoteCompactionV2ResponseResult { response, layered })
}

/// SSE 版本的 Remote Compaction V2 响应改写。
pub fn rewrite_remote_compaction_v2_responses_sse(
    request_json: &Value,
    sse_text: String,
) -> Option<String> {
    rewrite_remote_compaction_v2_responses_sse_with_layered_compaction(
        request_json,
        false,
        DEFAULT_RETAIN_TOKENS,
        sse_text,
    )
    .map(|result| result.sse_text)
}

/// SSE 版本的 V2 synthetic compaction 封装，可选应用分层压缩 tail。
pub fn rewrite_remote_compaction_v2_responses_sse_with_layered_compaction(
    request_json: &Value,
    layered_enabled: bool,
    retain_tokens: u32,
    sse_text: String,
) -> Option<LayeredCompactionResult> {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return None;
    }
    let normalized_sse = sse_text.replace("\r\n", "\n").replace('\r', "\n");
    let rewritten = match extract_single_remote_compaction_v2_terminal_response(&normalized_sse) {
        Ok(response_object) => rewrite_remote_compaction_v2_response_with_layered_compaction(
            request_json,
            &response_object,
            layered_enabled,
            retain_tokens,
        )
        .expect("Remote Compaction V2 request must always produce a terminal bridge result"),
        Err(error) => RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                None,
                error.code(),
                error.message(),
            ),
            layered: LayeredCompactionStats::default(),
        },
    };
    let rewritten_sse =
        if rewritten.response.get("status").and_then(Value::as_str) == Some("completed") {
            build_responses_sse_for_compaction(&rewritten.response)
        } else {
            build_responses_sse_for_remote_compaction_failure(&rewritten.response)
        };
    Some(LayeredCompactionResult {
        sse_text: rewritten_sse,
        triggered: rewritten.layered.triggered,
        retained_items: rewritten.layered.retained_items,
        retained_chars: rewritten.layered.retained_chars,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteCompactionV2SseTerminalError {
    MalformedEvent,
    UpstreamErrorEvent,
    MissingTerminal,
    MultipleTerminals,
    InvalidTerminal,
}

impl RemoteCompactionV2SseTerminalError {
    fn code(self) -> &'static str {
        match self {
            Self::MalformedEvent => "remote_compaction_sse_parse_failed",
            Self::UpstreamErrorEvent => "remote_compaction_upstream_failed",
            Self::MissingTerminal => "remote_compaction_terminal_response_missing",
            Self::MultipleTerminals => "remote_compaction_multiple_terminal_responses",
            Self::InvalidTerminal => "remote_compaction_terminal_response_invalid",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::MalformedEvent => {
                "Remote Compaction V2 bridge received a malformed upstream SSE event."
            }
            Self::UpstreamErrorEvent => {
                "Remote Compaction V2 bridge received an upstream error event."
            }
            Self::MissingTerminal => {
                "Remote Compaction V2 bridge received no terminal upstream response."
            }
            Self::MultipleTerminals => {
                "Remote Compaction V2 bridge received multiple terminal upstream responses."
            }
            Self::InvalidTerminal => {
                "Remote Compaction V2 bridge received an invalid terminal upstream response."
            }
        }
    }
}

fn extract_single_remote_compaction_v2_terminal_response(
    sse_text: &str,
) -> Result<Value, RemoteCompactionV2SseTerminalError> {
    let mut terminal_response = None;
    for block in sse_text.split("\n\n") {
        let data = block
            .lines()
            .filter_map(|line| line.trim().strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event = serde_json::from_str::<Value>(&data)
            .map_err(|_| RemoteCompactionV2SseTerminalError::MalformedEvent)?;
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "error" {
            return Err(RemoteCompactionV2SseTerminalError::UpstreamErrorEvent);
        }
        if !matches!(
            event_type,
            "response.completed" | "response.incomplete" | "response.failed"
        ) {
            continue;
        }
        if terminal_response.is_some() {
            return Err(RemoteCompactionV2SseTerminalError::MultipleTerminals);
        }
        let response = event
            .get("response")
            .filter(|response| response.is_object())
            .cloned()
            .ok_or(RemoteCompactionV2SseTerminalError::InvalidTerminal)?;
        let expected_status = match event_type {
            "response.completed" => "completed",
            "response.incomplete" => "incomplete",
            "response.failed" => "failed",
            _ => unreachable!("terminal event type was already matched"),
        };
        if response.get("status").and_then(Value::as_str) != Some(expected_status) {
            return Err(RemoteCompactionV2SseTerminalError::InvalidTerminal);
        }
        terminal_response = Some(response);
    }
    terminal_response.ok_or(RemoteCompactionV2SseTerminalError::MissingTerminal)
}

fn apply_layered_tail_to_summary(
    request_json: &Value,
    summary: &str,
    enabled: bool,
    retain_tokens: u32,
) -> (String, LayeredCompactionStats) {
    if !enabled {
        return (summary.to_string(), LayeredCompactionStats::default());
    }
    let input = request_json
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let skip_last_item = !is_remote_compaction_v2_request(Some(request_json));
    let TailSection {
        text: tail,
        items,
        chars,
    } = build_tail_section(&input, retain_budget_chars(retain_tokens), skip_last_item);
    if tail.is_empty() {
        return (summary.to_string(), LayeredCompactionStats::default());
    }
    (
        combine_summary_with_tail(summary, &tail),
        LayeredCompactionStats {
            triggered: true,
            retained_items: items,
            retained_chars: chars,
        },
    )
}

fn synthetic_remote_compaction_item(summary: &str) -> Value {
    let summary =
        truncate_utf8_to_byte_limit(summary.trim(), MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES);
    let encoded = URL_SAFE_NO_PAD.encode(summary.as_bytes());
    json!({
        "type": "compaction",
        "encrypted_content": format!("{REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX}{encoded}")
    })
}

fn truncate_utf8_to_byte_limit(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// 为 Remote Compaction V2 降级桥生成规范的失败响应。
///
/// 所有异常终结都必须丢弃普通 message/tool 输出，避免 Codex V2 collector
/// 再次遇到“0 个或多个 compaction item”的不确定状态。
pub fn remote_compaction_v2_failure_response(
    request_json: &Value,
    response_object: Option<&Value>,
    code: &str,
    message: &str,
) -> Value {
    let mut response = response_object
        .filter(|response| response.is_object())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "id": "resp_compaction",
                "object": "response",
                "created_at": 0,
                "model": request_json
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                "usage": null
            })
        });
    let object = response
        .as_object_mut()
        .expect("remote compaction failure response must be an object");
    object.insert("status".to_string(), json!("failed"));
    object.insert("output".to_string(), json!([]));
    object.remove("incomplete_details");
    object.insert(
        "error".to_string(),
        json!({
            "code": code,
            "message": message
        }),
    );
    response
}

/// 为流式 / WebSocket 降级桥生成只包含 `response.failed` 的规范 SSE。
pub fn remote_compaction_v2_failure_sse(request_json: &Value, code: &str, message: &str) -> String {
    compaction_failure_sse(request_json, None, code, message)
}

/// 为传统分层压缩或 Remote Compaction V2 桥生成规范的失败 SSE。
///
/// `response_object` 存在时保留其响应 ID、模型和 usage；所有普通输出都会被清空。
pub fn compaction_failure_sse(
    request_json: &Value,
    response_object: Option<&Value>,
    code: &str,
    message: &str,
) -> String {
    let response =
        remote_compaction_v2_failure_response(request_json, response_object, code, message);
    build_responses_sse_for_remote_compaction_failure(&response)
}

fn extract_compaction_summary_text(response_object: &Value) -> Option<String> {
    if let Some(text) = extract_message_text(response_object) {
        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = item.get("summary").and_then(Value::as_array) {
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                }
            }
        }
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

/// 分层压缩结果。
#[derive(Debug, Clone)]
pub struct LayeredCompactionResult {
    /// 最终回注给 Codex 的 Responses SSE 文本。
    pub sse_text: String,
    /// 是否真正触发了改写。
    pub triggered: bool,
    /// 实际保留的原始记录条数。
    pub retained_items: u32,
    /// 实际保留的原始记录字符数（用于诊断）。
    pub retained_chars: u32,
}

impl LayeredCompactionResult {
    fn unchanged(sse_text: String) -> Self {
        Self {
            sse_text,
            triggered: false,
            retained_items: 0,
            retained_chars: 0,
        }
    }
}

/// 判断请求是否是 Codex 的上下文压缩请求：`input` 最后一项是 user 消息，
/// 且其文本以固定压缩指令前缀开头。
pub fn is_compaction_request(request_json: Option<&Value>) -> bool {
    let Some(request) = request_json else {
        return false;
    };
    let Some(input) = request.get("input").and_then(Value::as_array) else {
        return false;
    };
    let Some(last) = input.last() else {
        return false;
    };
    if last.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    item_text(last)
        .trim_start()
        .starts_with(COMPACTION_PROMPT_PREFIX)
}

/// 若请求是 Codex 压缩请求且配置了自定义压缩提示词，将 `input` 最后一项（压缩指令）的文本
/// 替换为自定义内容，保持其余结构（type/role/content 数组形态）不变。
///
/// - 非压缩请求、自定义提示词为空、或无法定位最后一项时原样返回（继续使用 Codex 默认提示词）。
pub fn apply_custom_compaction_prompt(request_json: &Value, custom_prompt: &str) -> Value {
    let custom_prompt = custom_prompt.trim();
    if custom_prompt.is_empty() || !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    let mut updated = request_json.clone();
    let Some(input) = updated
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
        .and_then(Value::as_array_mut)
    else {
        return request_json.clone();
    };
    let Some(last) = input.last_mut() else {
        return request_json.clone();
    };
    replace_message_text(last, custom_prompt);
    updated
}

/// 为传统上下文压缩准备只生成摘要文本的上游请求。
///
/// 请求身份仍由调用方保存的原始请求判断；转发副本会使用有效项目提示词，并移除工具字段，
/// 避免摘要阶段产生工具调用。
pub fn prepare_legacy_layered_compaction_request(
    request_json: &Value,
    prompt_override: &str,
) -> Value {
    if !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    let mut request =
        apply_custom_compaction_prompt(request_json, effective_compaction_prompt(prompt_override));
    if let Some(object) = request.as_object_mut() {
        for key in ["tools", "tool_choice", "parallel_tool_calls"] {
            object.remove(key);
        }
    }
    request
}

/// 判断 completed Responses 对象是否包含传统压缩可用的 assistant 摘要文本。
pub fn has_completed_compaction_summary(response_object: &Value) -> bool {
    response_object.get("status").and_then(Value::as_str) == Some("completed")
        && extract_message_text(response_object).is_some_and(|summary| !summary.trim().is_empty())
}

/// 将 message item 的文本内容整体替换为 `text`，兼容字符串 content 与
/// content 数组两种形态：数组形态只保留第一个文本块并替换其 `text`，其余块丢弃
/// （压缩指令本身只有单一文本块，不存在多块情况）。
fn replace_message_text(item: &mut Value, text: &str) {
    let Some(object) = item.as_object_mut() else {
        return;
    };
    match object.get("content") {
        Some(Value::Array(parts)) if !parts.is_empty() => {
            let kind = parts[0]
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("input_text")
                .to_string();
            object.insert(
                "content".to_string(),
                json!([{ "type": kind, "text": text }]),
            );
        }
        _ => {
            object.insert("content".to_string(), json!(text));
        }
    }
}

/// 在压缩响应 SSE 上应用分层压缩：把上游摘要与请求中最近的原始记录合并回注。
///
/// - `enabled` 为 false、非压缩请求、或无法解析终止响应/摘要时，原样返回。
/// - `retain_tokens` 是希望保留的最近原始记录预算（按 token 估算，约 4 字符/token）。
pub fn apply_layered_compaction_to_responses_sse(
    request_json: &Value,
    enabled: bool,
    retain_tokens: u32,
    sse_text: String,
) -> LayeredCompactionResult {
    if !enabled || !is_compaction_request(Some(request_json)) {
        return LayeredCompactionResult::unchanged(sse_text);
    }
    let Some(response_object) =
        crate::continue_thinking::extract_terminal_response_object(&sse_text)
    else {
        return LayeredCompactionResult::unchanged(sse_text);
    };
    // 只在终止状态为 completed 时改写；incomplete/failed 保持原样。
    if response_object.get("status").and_then(Value::as_str) != Some("completed") {
        return LayeredCompactionResult::unchanged(sse_text);
    }
    let Some(summary) = extract_message_text(&response_object) else {
        return LayeredCompactionResult::unchanged(sse_text);
    };
    if summary.trim().is_empty() {
        return LayeredCompactionResult::unchanged(sse_text);
    }

    let (combined, stats) =
        apply_layered_tail_to_summary(request_json, &summary, true, retain_tokens);
    if !stats.triggered {
        return LayeredCompactionResult::unchanged(sse_text);
    }

    let rebuilt = build_responses_sse_for_message(&response_object, &combined);

    LayeredCompactionResult {
        sse_text: rebuilt,
        triggered: true,
        retained_items: stats.retained_items,
        retained_chars: stats.retained_chars,
    }
}

/// token 预算换算为字符预算（粗略 4 字符 ≈ 1 token），并做上下限约束。
fn retain_budget_chars(retain_tokens: u32) -> usize {
    let tokens = retain_tokens.clamp(MIN_RETAIN_TOKENS, MAX_RETAIN_TOKENS);
    tokens as usize * 4
}

/// 保留 token 预算的下限 / 上限 / 默认值。
pub const MIN_RETAIN_TOKENS: u32 = 20_000;
pub const MAX_RETAIN_TOKENS: u32 = 64_000;
pub const DEFAULT_RETAIN_TOKENS: u32 = 20_000;

/// 单条工具调用 / 工具输出的独立截断上限（与总预算解耦），
/// 防止一条巨大的命令输出（如 `cat`/`ls -R`）占满整个 tail 预算，
/// 挤掉更有价值的助手消息。参考 pi agent 对 tool result 的独立
/// 截断策略（pi 固定 2000 字符）。
const MAX_TOOL_ITEM_CHARS: usize = 4_000;

struct TailSection {
    text: String,
    items: u32,
    chars: u32,
}

/// item 在 tail 中的语义分类，用于两阶段预算分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemCategory {
    /// 助手文字回复（用户/developer/system 由 Codex 原生保留，此处不重复），
    /// 携带对话意图，优先保留。
    Message,
    /// 工具调用 / 工具输出，需要原子配对且优先级低于 message。
    Tool,
}

/// 从 `input` 末尾向前选取“最近一轮”对话（最后一条 user 消息 → 结尾），跳过
/// 最后一项（压缩指令本身）。保留 user 消息与其后的 assistant 回复、工具调用/输出，
/// 按协议原始顺序拼接。
///
/// 与旧版“按预算填满”不同：目标是完整保留最近一轮，让压缩后的模型看到
/// “上一轮在做什么”的原始上下文。`budget_chars` 仅作为安全上限，
/// 防止异常巨大的一轮占满上下文；正常情况下不会触及。
///
/// 算法：
/// 1. 从末尾向前找到最后一条 user 消息作为“最近一轮”的起点；若历史中没有
///    user 消息，则保留全部（去掉压缩指令）。
/// 2. 逐条文本化选中区间的 user/assistant 消息与工具调用/输出；单条工具记录独立
///    截断到 `MAX_TOOL_ITEM_CHARS`，避免一条巨大输出占满上下文。
/// 3. 孤儿 `function_call_output`（配对的 `function_call` 不在区间内）直接丢弃。
/// 4. 若全部拼接后仍超过 `budget_chars` 安全上限，从最早的条目开始丢弃直到卡回预算内。
fn build_tail_section(input: &[Value], budget_chars: usize, skip_last_item: bool) -> TailSection {
    let end = if skip_last_item {
        input.len().saturating_sub(1)
    } else {
        input.len()
    };
    let items = &input[..end];

    // 最近一轮起点：最后一条 user 消息的下标；找不到则从头保留。
    let turn_start = items
        .iter()
        .rposition(|item| {
            item.get("type")
                .and_then(Value::as_str)
                .unwrap_or("message")
                == "message"
                && item.get("role").and_then(Value::as_str) == Some("user")
        })
        .unwrap_or(0);
    let turn = &items[turn_start..];

    let call_index = index_function_calls_by_call_id(turn);

    // 按原始顺序文本化最近一轮的每一条，工具记录单条截断；孤儿工具输出丢弃。
    let mut pieces: Vec<String> = Vec::new();
    for item in turn {
        if let Some(call_id) = function_call_output_call_id(item) {
            if !call_index.contains_key(call_id) {
                continue;
            }
        }
        let Some((category, text)) = textualize_item(item) else {
            continue;
        };
        let text = match category {
            ItemCategory::Message => text,
            ItemCategory::Tool => truncate_chars(&text, MAX_TOOL_ITEM_CHARS),
        };
        pieces.push(text);
    }

    // 安全上限：若一轮异常巨大超过 budget_chars，从最早的条目开始丢弃。
    let mut total = tail_pieces_chars(&pieces);
    while total > budget_chars && pieces.len() > 1 {
        let removed = pieces.remove(0);
        total = total.saturating_sub(removed.chars().count() + 2);
    }
    // 仍只剩一条且超预算时，单条硬截断。
    if total > budget_chars {
        if let Some(only) = pieces.first_mut() {
            *only = truncate_chars(only, budget_chars);
        }
    }

    let text = pieces.join("\n\n");
    let used = text.chars().count();

    TailSection {
        text,
        items: pieces.len() as u32,
        chars: used as u32,
    }
}

/// 估算拼接后总字符数（含每段之间的 `\n\n` 分隔）。
fn tail_pieces_chars(pieces: &[String]) -> usize {
    let body: usize = pieces.iter().map(|piece| piece.chars().count()).sum();
    let separators = pieces.len().saturating_sub(1) * 2;
    body + separators
}

/// 预先扫描工具调用项，建立 `call_id -> input 下标` 索引，用于原子配对。
fn index_function_calls_by_call_id(items: &[Value]) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if !matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call" | "local_shell_call" | "tool_call")
        ) {
            continue;
        }
        if let Some(call_id) = function_call_id(item) {
            map.insert(call_id, index);
        }
    }
    map
}

/// `function_call` 的配对键：优先 `call_id`，否则回退到 `id`
/// （与代码库其他处理 `function_call` 的规则保持一致，见 protocol_proxy.rs）。
fn function_call_id(item: &Value) -> Option<&str> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// 若 item 是工具输出，返回其配对 ID。
fn function_call_output_call_id(item: &Value) -> Option<&str> {
    if !matches!(
        item.get("type").and_then(Value::as_str),
        Some(
            "function_call_output"
                | "custom_tool_call_output"
                | "local_shell_call_output"
                | "tool_call_output"
        )
    ) {
        return None;
    }
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// 把单个 `input` item 转成可读文本并标注分类；无有效内容返回 None。
fn textualize_item(item: &Value) -> Option<(ItemCategory, String)> {
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match kind {
        "message" => {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            // 最近一轮只包含 user / assistant 对话；developer/system 是会话开头的
            // 系统脚手架，不属于最近一轮，也由 Codex 自己管理，因此跳过。
            if role != "user" && role != "assistant" {
                return None;
            }
            let text = item_text(item);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            Some((ItemCategory::Message, format!("[{role}]\n{text}")))
        }
        "function_call" | "custom_tool_call" | "local_shell_call" | "tool_call" => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
            let args = item
                .get("arguments")
                .map(stringify_arguments)
                .unwrap_or_default();
            let args = args.trim();
            let text = if args.is_empty() {
                format!("[tool_call: {name}]")
            } else {
                format!("[tool_call: {name}]\n{args}")
            };
            Some((ItemCategory::Tool, text))
        }
        "function_call_output"
        | "custom_tool_call_output"
        | "local_shell_call_output"
        | "tool_call_output" => {
            let output = item.get("output").map(stringify_output).unwrap_or_default();
            let output = output.trim();
            if output.is_empty() {
                None
            } else {
                Some((ItemCategory::Tool, format!("[tool_output]\n{output}")))
            }
        }
        // reasoning 等内部项对续接价值低，跳过以节省预算。
        _ => None,
    }
}

/// 提取 message item 的文本（支持字符串 content 与 content 数组）。
fn item_text(item: &Value) -> String {
    match item.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                } else if let Some(part_text) = part.as_str() {
                    text.push_str(part_text);
                }
            }
            text
        }
        _ => String::new(),
    }
}

fn stringify_arguments(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn stringify_output(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                } else if let Some(part_text) = part.as_str() {
                    text.push_str(part_text);
                }
            }
            if text.is_empty() {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                text
            }
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// 按字符数截断（保留 UTF-8 边界），超长时追加省略标记。
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// 合成回注文本：LLM 摘要 + 标签包住的“最近一轮”原始记录。
///
/// tail 用标签包住，让模型明确知道这是压缩前最近一轮的原始上下文，而非新指令。
fn combine_summary_with_tail(summary: &str, tail: &str) -> String {
    format!(
        "{summary}\n\n{open}\n{tail}\n{close}",
        summary = summary.trim_end(),
        tail = tail.trim(),
        open = RECENT_TURN_OPEN_TAG,
        close = RECENT_TURN_CLOSE_TAG,
    )
}

/// 以终止响应对象为骨架，重建一条只含单个 assistant message 的完整 Responses SSE。
fn build_responses_sse_for_message(response_object: &Value, message_text: &str) -> String {
    let response_id = response_object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_compaction")
        .to_string();
    let created_at = response_object
        .get("created_at")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let model = response_object
        .get("model")
        .cloned()
        .unwrap_or_else(|| json!(""));
    let usage = response_object
        .get("usage")
        .cloned()
        .unwrap_or_else(|| json!(null));
    // 复用合法的 message item id；旧格式先规范化，否则从 response id 独立派生。
    let item_id = existing_message_item_id(response_object)
        .and_then(|id| crate::protocol_proxy::normalize_responses_message_item_id(&id))
        .unwrap_or_else(|| crate::protocol_proxy::response_message_item_id(&response_id));

    let mut sequence = 0u64;
    let mut output = String::new();

    let base_response = |status: &str, output_items: Value| {
        json!({
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": status,
            "model": model,
            "output": output_items,
            "usage": usage
        })
    };

    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": []
            }
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.content_part.added",
        json!({
            "type": "response.content_part.added",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": { "type": "output_text", "text": "", "annotations": [] }
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_text.delta",
        json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "delta": message_text
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_text.done",
        json!({
            "type": "response.output_text.done",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "text": message_text
        }),
        &mut sequence,
    );
    let done_part = json!({ "type": "output_text", "text": message_text, "annotations": [] });
    push_event(
        &mut output,
        "response.content_part.done",
        json!({
            "type": "response.content_part.done",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": done_part
        }),
        &mut sequence,
    );
    let message_item = json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": message_text, "annotations": [] }]
    });
    push_event(
        &mut output,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": message_item
        }),
        &mut sequence,
    );
    // response.completed：以原终止响应为骨架，替换 output/status，保留 instructions/tools 等字段。
    let mut completed = response_object.clone();
    if let Some(object) = completed.as_object_mut() {
        object.insert("status".to_string(), json!("completed"));
        object.insert("output".to_string(), json!([message_item]));
    }
    push_event(
        &mut output,
        "response.completed",
        json!({ "type": "response.completed", "response": completed }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

/// 以终止响应对象为骨架，重建只含单个 `compaction` item 的完整 Responses SSE。
fn build_responses_sse_for_compaction(response_object: &Value) -> String {
    let response_id = response_object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_compaction")
        .to_string();
    let created_at = response_object
        .get("created_at")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let model = response_object
        .get("model")
        .cloned()
        .unwrap_or_else(|| json!(""));
    let usage = response_object
        .get("usage")
        .cloned()
        .unwrap_or_else(|| json!(null));
    let compaction_item = response_object
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or_else(|| synthetic_remote_compaction_item(""));

    let mut sequence = 0u64;
    let mut output = String::new();
    let base_response = |status: &str, output_items: Value| {
        json!({
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": status,
            "model": model,
            "output": output_items,
            "usage": usage
        })
    };

    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": compaction_item
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": compaction_item
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.completed",
        json!({ "type": "response.completed", "response": response_object }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

fn build_responses_sse_for_remote_compaction_failure(response_object: &Value) -> String {
    let response_id = response_object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_compaction")
        .to_string();
    let created_at = response_object
        .get("created_at")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let model = response_object
        .get("model")
        .cloned()
        .unwrap_or_else(|| json!(""));
    let usage = response_object
        .get("usage")
        .cloned()
        .unwrap_or_else(|| json!(null));
    let mut sequence = 0u64;
    let mut output = String::new();
    let base_response = |status: &str| {
        json!({
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": status,
            "model": model,
            "output": [],
            "usage": usage
        })
    };

    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": base_response("in_progress") }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": base_response("in_progress") }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.failed",
        json!({ "type": "response.failed", "response": response_object }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

fn existing_message_item_id(response_object: &Value) -> Option<String> {
    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("message") {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// 从终止响应对象中提取 assistant message 的纯文本。
fn extract_message_text(response_object: &Value) -> Option<String> {
    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = item.get("content").and_then(Value::as_array) {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("output_text") | Some("text") | None => {
                        if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                            text.push_str(part_text);
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(direct) = item.get("content").and_then(Value::as_str) {
            text.push_str(direct);
        }
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// 写入一个带 `sequence_number` 的 SSE 事件。
fn push_event(output: &mut String, event: &str, mut data: Value, sequence: &mut u64) {
    if let Some(object) = data.as_object_mut() {
        object
            .entry("sequence_number".to_string())
            .or_insert_with(|| json!(*sequence));
        *sequence += 1;
    }
    output.push_str("event: ");
    output.push_str(event);
    output.push_str("\ndata: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_default());
    output.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compaction_prompt_item() -> Value {
        json!({
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": format!("{COMPACTION_PROMPT_PREFIX}. Create a handoff summary.\n")
            }]
        })
    }

    fn user_message(text: &str) -> Value {
        json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": text }]
        })
    }

    fn assistant_message(text: &str) -> Value {
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": text }]
        })
    }

    fn remote_compaction_v2_request() -> Value {
        json!({
            "model": "claude-sonnet-5",
            "stream": true,
            "input": [
                user_message("implement the fix"),
                {
                    "type": "compaction_trigger"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "exec_command",
                "parameters": { "type": "object" }
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        })
    }

    /// 上游返回的压缩摘要 SSE（单条 assistant message，completed）。
    fn summary_sse(summary: &str) -> String {
        let response = json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "gpt-5.6-sol",
            "output": [{
                "id": "resp_test_msg",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": summary, "annotations": [] }]
            }],
            "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 }
        });
        format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::to_string(&json!({
                "type": "response.completed",
                "response": response
            }))
            .unwrap()
        )
    }

    #[test]
    fn detects_compaction_request_by_trailing_instruction() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        assert!(is_compaction_request(Some(&request)));
    }

    #[test]
    fn ignores_normal_request() {
        let request = json!({ "input": [user_message("just a normal question")] });
        assert!(!is_compaction_request(Some(&request)));
    }

    #[test]
    fn detects_remote_compaction_v2_trigger() {
        assert!(is_remote_compaction_v2_request(Some(
            &remote_compaction_v2_request()
        )));
        assert!(!is_remote_compaction_v2_request(Some(&json!({
            "input": [user_message("normal")]
        }))));
    }

    #[test]
    fn effective_prompt_uses_project_default_for_blank_override() {
        assert_eq!(effective_compaction_prompt(""), DEFAULT_COMPACTION_PROMPT);
        assert_eq!(
            effective_compaction_prompt(" \r\n\t"),
            DEFAULT_COMPACTION_PROMPT
        );
        assert_eq!(
            effective_compaction_prompt("  CUSTOM SUMMARY PROMPT  "),
            "CUSTOM SUMMARY PROMPT"
        );
    }

    #[test]
    fn remote_compaction_v2_bridge_replaces_trigger_and_removes_tools() {
        let rewritten =
            prepare_remote_compaction_v2_bridge_request(&remote_compaction_v2_request());
        assert!(rewritten.get("tools").is_none());
        assert!(rewritten.get("tool_choice").is_none());
        assert!(rewritten.get("parallel_tool_calls").is_none());
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "user");
        assert!(item_text(&input[1]).starts_with(COMPACTION_PROMPT_PREFIX));
    }

    #[test]
    fn remote_compaction_v2_bridge_uses_layered_custom_prompt() {
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_prompt(
            &remote_compaction_v2_request(),
            Some("CUSTOM LAYERED COMPACTION PROMPT"),
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(item_text(&input[1]), "CUSTOM LAYERED COMPACTION PROMPT");
    }

    #[test]
    fn remote_compaction_v2_bridge_uses_project_default_for_blank_override() {
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_prompt(
            &remote_compaction_v2_request(),
            Some(""),
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(item_text(&input[1]), DEFAULT_COMPACTION_PROMPT);
    }

    #[test]
    fn remote_compaction_v2_response_contains_exactly_one_compaction_item() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [
                {
                    "id": "msg_bridge",
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "SUMMARY", "annotations": [] }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_unexpected",
                    "name": "exec_command",
                    "arguments": "{}"
                }
            ],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 response should be rewritten");
        let output = rewritten.get("output").and_then(Value::as_array).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "compaction");
        let restored = synthetic_remote_compaction_history_text(&output[0])
            .expect("synthetic compaction should be decodable");
        assert!(restored.contains("SUMMARY"));
    }

    #[test]
    fn synthetic_remote_compaction_is_limited_to_decodable_size() {
        let summary = format!("{}界", "x".repeat(MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES));
        let item = synthetic_remote_compaction_item(&summary);
        let restored = synthetic_remote_compaction_history_text(&item)
            .expect("size-limited synthetic compaction should remain decodable");

        assert!(restored.ends_with('x'));
        assert!(!restored.ends_with('界'));
    }

    #[test]
    fn remote_compaction_v2_uses_layered_tail_when_enabled() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("USER CONTEXT KEPT NATIVELY"),
                assistant_message("KEEP THIS ASSISTANT CONTEXT"),
                { "type": "compaction_trigger" }
            ]
        });
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "id": "msg_bridge",
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY", "annotations": [] }]
            }]
        });
        let rewritten = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            MIN_RETAIN_TOKENS,
        )
        .expect("V2 layered response should be rewritten");
        assert!(rewritten.layered.triggered);
        // 最近一轮 = 最后一条 user → 结尾：此例为 [user, assistant]。
        assert_eq!(rewritten.layered.retained_items, 2);
        let restored =
            synthetic_remote_compaction_history_text(&rewritten.response["output"][0]).unwrap();
        assert!(restored.contains("SUMMARY"));
        // 最近一轮完整保留，含该 user 消息与其后的 assistant 回复。
        assert!(restored.contains("USER CONTEXT KEPT NATIVELY"));
        assert!(restored.contains("KEEP THIS ASSISTANT CONTEXT"));

        let plain = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            false,
            MIN_RETAIN_TOKENS,
        )
        .expect("V2 plain response should be rewritten");
        assert!(!plain.layered.triggered);
        let restored_plain =
            synthetic_remote_compaction_history_text(&plain.response["output"][0]).unwrap();
        assert!(restored_plain.contains("SUMMARY"));
        assert!(!restored_plain.contains("USER CONTEXT KEPT NATIVELY"));
        assert!(!restored_plain.contains("KEEP THIS ASSISTANT CONTEXT"));
    }

    #[test]
    fn remote_compaction_v2_sse_emits_only_one_done_output_item() {
        let rewritten = rewrite_remote_compaction_v2_responses_sse(
            &remote_compaction_v2_request(),
            summary_sse("SUMMARY"),
        )
        .expect("V2 SSE should be rewritten");
        let done_items = rewritten
            .split("\n\n")
            .filter(|event| event.starts_with("event: response.output_item.done"))
            .collect::<Vec<_>>();
        assert_eq!(done_items.len(), 1);
        assert!(done_items[0].contains("\"type\":\"compaction\""));
        assert!(!rewritten.contains("\"type\":\"message\",\"status\":\"completed\""));
        let terminal = crate::continue_thinking::extract_terminal_response_object(&rewritten)
            .expect("rewritten SSE has terminal response");
        assert_eq!(
            terminal
                .get("output")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(terminal["output"][0]["type"], "compaction");
    }

    #[test]
    fn remote_compaction_v2_without_summary_fails_closed() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "function_call",
                "call_id": "call_only",
                "name": "exec_command",
                "arguments": "{}"
            }],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 completed response must never fall back to ordinary outputs");
        assert_eq!(rewritten["status"], "failed");
        assert_eq!(rewritten["output"], json!([]));
        assert_eq!(
            rewritten["error"]["code"],
            "remote_compaction_summary_missing"
        );
    }

    #[test]
    fn remote_compaction_v2_sse_without_summary_emits_failed_terminal_event() {
        let response = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "function_call",
                "call_id": "call_only",
                "name": "exec_command",
                "arguments": "{}"
            }],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let source = format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::to_string(&json!({
                "type": "response.completed",
                "response": response
            }))
            .unwrap()
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 SSE must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_summary_missing"));
        assert!(!rewritten.contains("event: response.output_item.done"));
        assert!(!rewritten.contains("event: response.completed"));
    }

    #[test]
    fn remote_compaction_v2_incomplete_response_fails_closed() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "incomplete",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "PARTIAL SUMMARY" }]
            }],
            "incomplete_details": { "reason": "max_output_tokens" }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 incomplete response must fail closed");
        assert_eq!(rewritten["status"], "failed");
        assert_eq!(rewritten["output"], json!([]));
        assert_eq!(
            rewritten["error"]["code"],
            "remote_compaction_upstream_incomplete"
        );
        assert!(rewritten.get("incomplete_details").is_none());
    }

    #[test]
    fn remote_compaction_v2_sse_without_terminal_event_fails_closed() {
        let source = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_bridge\",\"status\":\"in_progress\"}}\n\n".to_string();
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 SSE without terminal response must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_terminal_response_missing"));
        assert!(!rewritten.contains("event: response.output_item.done"));
    }

    #[test]
    fn remote_compaction_v2_malformed_sse_event_fails_even_if_completed_follows() {
        let source = format!(
            "event: response.output_text.delta\ndata: {{malformed-json}}\n\n{}",
            summary_sse("SUMMARY MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("malformed V2 SSE must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_sse_parse_failed"));
        assert!(!rewritten.contains("event: response.output_item.done"));
        assert!(!rewritten.contains("SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_multiple_terminal_events_fail_closed() {
        let failed = serde_json::to_string(&json!({
            "type": "response.failed",
            "response": {
                "id": "resp_duplicate_terminal",
                "status": "failed",
                "output": [],
                "error": { "message": "first terminal failed" }
            }
        }))
        .unwrap();
        let source = format!(
            "event: response.failed\ndata: {failed}\n\n{}",
            summary_sse("LATE SUMMARY MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("multiple V2 terminal events must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_multiple_terminal_responses"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("LATE SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_terminal_event_status_mismatch_fails_closed() {
        let source = serde_json::to_string(&json!({
            "type": "response.failed",
            "response": {
                "id": "resp_mismatched_terminal",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "MISMATCHED SUMMARY MUST NOT BE USED"
                    }]
                }]
            }
        }))
        .unwrap();
        let rewritten = rewrite_remote_compaction_v2_responses_sse(
            &remote_compaction_v2_request(),
            format!("event: response.failed\ndata: {source}\n\n"),
        )
        .expect("mismatched V2 terminal must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_terminal_response_invalid"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("MISMATCHED SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_error_event_fails_even_if_completed_follows() {
        let source = format!(
            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"upstream failed\"}}}}\n\n{}",
            summary_sse("SUMMARY AFTER ERROR MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 error event must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_upstream_failed"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("SUMMARY AFTER ERROR MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_accepts_crlf_sse_event_boundaries() {
        let source = summary_sse("CRLF SUMMARY").replace('\n', "\r\n");
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("valid CRLF V2 SSE should be rewritten");
        assert!(rewritten.contains("event: response.completed"));
        assert!(rewritten.contains("\"type\":\"compaction\""));
        assert!(!rewritten.contains("remote_compaction_sse_parse_failed"));
    }

    #[test]
    fn remote_compaction_v2_tail_keeps_latest_item_after_non_trailing_trigger() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("earlier context"),
                { "type": "compaction_trigger" },
                assistant_message("latest context after trigger")
            ]
        });
        let source = json!({
            "id": "resp_non_trailing_trigger",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY" }]
            }]
        });
        let rewritten = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            DEFAULT_RETAIN_TOKENS,
        )
        .expect("V2 request should be rewritten");
        let restored =
            synthetic_remote_compaction_history_text(&rewritten.response["output"][0]).unwrap();
        assert!(restored.contains("latest context after trigger"));
    }

    #[test]
    fn custom_prompt_replaces_last_input_item_text() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        let rewritten = apply_custom_compaction_prompt(&request, "自定义压缩提示词");
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(input.len(), 2, "不应增减 item 数量");
        assert_eq!(item_text(&input[1]), "自定义压缩提示词");
        assert_eq!(input[1]["role"], "user");
        // 未受影响的其他 item 保持不变。
        assert_eq!(item_text(&input[0]), "hi");
    }

    #[test]
    fn legacy_layered_request_uses_effective_prompt_and_removes_tools() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()],
            "tools": [{ "type": "function", "name": "exec_command" }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        });
        let rewritten = prepare_legacy_layered_compaction_request(&request, "");
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();

        assert_eq!(item_text(&input[1]), DEFAULT_COMPACTION_PROMPT);
        assert!(rewritten.get("tools").is_none());
        assert!(rewritten.get("tool_choice").is_none());
        assert!(rewritten.get("parallel_tool_calls").is_none());
        assert_eq!(
            item_text(&request["input"][1]),
            item_text(&compaction_prompt_item())
        );
    }

    #[test]
    fn empty_custom_prompt_keeps_default_codex_prompt() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        let rewritten = apply_custom_compaction_prompt(&request, "   ");
        assert_eq!(
            rewritten, request,
            "空自定义提示词应原样返回（继续用 codex 默认提示词）"
        );
    }

    #[test]
    fn custom_prompt_ignored_for_non_compaction_request() {
        let request = json!({ "input": [user_message("just a normal question")] });
        let rewritten = apply_custom_compaction_prompt(&request, "自定义提示词");
        assert_eq!(rewritten, request, "非压缩请求不应被改写");
    }

    #[test]
    fn disabled_returns_unchanged() {
        let request = json!({
            "input": [user_message("hi"), assistant_message("ok"), compaction_prompt_item()]
        });
        let sse = summary_sse("SUMMARY");
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            false,
            DEFAULT_RETAIN_TOKENS,
            sse.clone(),
        );
        assert!(!result.triggered);
        assert_eq!(result.sse_text, sse);
    }

    #[test]
    fn injects_summary_and_recent_tail() {
        let request = json!({
            "input": [
                // 更早一轮（不属于最近一轮，不应出现在 tail）。
                user_message("old question that should be dropped"),
                assistant_message("old answer that should be dropped"),
                // 最近一轮：从这条 user 消息开始直到结尾。
                user_message("the most recent user request"),
                function_call_item("call_1", "exec_command"),
                function_call_output_item("call_1", "file_a\nfile_b"),
                assistant_message("most recent assistant note"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("SUMMARY-BODY"),
        );
        assert!(result.triggered);
        assert!(result.retained_items >= 1);

        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("rebuilt sse has terminal response");
        let text = extract_message_text(&response).expect("rebuilt message text");
        assert!(text.contains("SUMMARY-BODY"), "keeps summary");
        // 最近一轮完整保留：user 请求 + 工具调用/输出 + 助手回复。
        assert!(
            text.contains("the most recent user request"),
            "keeps recent user turn"
        );
        assert!(
            text.contains("most recent assistant note"),
            "keeps recent tail"
        );
        assert!(text.contains("exec_command"), "keeps recent tool call");
        assert!(text.contains("file_b"), "keeps recent tool output");
        // 更早一轮（上一个 user 之前）不属于最近一轮，不应出现。
        assert!(
            !text.contains("old question that should be dropped"),
            "earlier turn must not appear in the recent-turn tail"
        );
        assert!(!text.contains("old answer that should be dropped"));
        let output = response.get("output").and_then(Value::as_array).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["role"], "assistant");
        assert_eq!(output[0]["id"], "msg_test");
    }

    #[test]
    fn tail_respects_char_budget() {
        let big = "x".repeat(10_000);
        let request = json!({
            "input": [user_message(&big), assistant_message(&big), compaction_prompt_item()]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        assert!(result.retained_chars <= MIN_RETAIN_TOKENS * 4);
    }

    #[test]
    fn single_oversized_message_is_truncated_to_hard_budget() {
        let budget_chars = retain_budget_chars(MIN_RETAIN_TOKENS);
        let huge_message = "x".repeat(budget_chars * 2);
        // 用 assistant 消息：user 消息由 Codex 原生保留，不入 tail。
        let input = vec![assistant_message(&huge_message), compaction_prompt_item()];
        let tail = build_tail_section(&input, budget_chars, true);

        assert!(tail.text.chars().count() <= budget_chars);
        assert_eq!(tail.chars as usize, tail.text.chars().count());
        assert_eq!(tail.items, 1);
    }

    fn function_call_item(call_id: &str, name: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": "{}"
        })
    }

    fn function_call_output_item(call_id: &str, output: &str) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output
        })
    }

    /// 问题1：会话总量远低于保留预算时，应将全部原始记录保留，不报错也不硬填充。
    #[test]
    fn session_smaller_than_budget_keeps_everything() {
        let request = json!({
            "input": [
                user_message("hi"),
                assistant_message("hello"),
                user_message("how are you"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        // 用户消息由 Codex 原生保留，tail 只补回被丢弃的 assistant 回复。
        // 此例中仅 1 条 assistant（“hello”）应被补回；两条 user 不入 tail。
        assert_eq!(result.retained_items, 1);
        assert!(result.retained_chars < DEFAULT_RETAIN_TOKENS * 4 / 10);
    }

    /// 问题2：单条超大工具输出不得挤掉其他记录（与总预算解耦）。
    #[test]
    fn oversized_tool_output_does_not_starve_other_items() {
        let huge_output = "y".repeat(50_000);
        let request = json!({
            "input": [
                assistant_message("earlier assistant note that must survive"),
                assistant_message("another earlier assistant note that must survive"),
                function_call_item("call_1", "exec_command"),
                function_call_output_item("call_1", &huge_output),
                assistant_message("most recent assistant note"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        // 巨大工具输出必须被单条截断，不能占满预算挤掉更早的消息。
        assert!(
            text.contains("earlier assistant note that must survive"),
            "oversized tool output must not evict earlier assistant message"
        );
        assert!(text.contains("most recent assistant note"));
        // 巨大输出本身仍在（被截断），但不应包含完整 50000 字符。
        assert!(text.contains("tool_output"));
        assert!(text.len() < huge_output.len());
    }

    /// 问题2：选中 function_call_output 时必须原子保留对应 function_call，不产生孤儿项。
    #[test]
    fn tool_call_and_output_are_kept_atomically() {
        let request = json!({
            "input": [
                user_message("start"),
                function_call_item("call_1", "exec_command"),
                function_call_output_item("call_1", "output one"),
                function_call_item("call_2", "exec_command"),
                function_call_output_item("call_2", "output two"),
                compaction_prompt_item()
            ]
        });
        // 极小预算，只够选中最后一条 output，验证它不会单独出现。
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        if text.contains("output two") {
            assert!(
                text.contains("exec_command"),
                "tool_call_output must never appear without its tool_call"
            );
        }
    }

    #[test]
    fn custom_tool_call_and_output_are_kept_atomically() {
        let input = vec![
            json!({
                "type": "custom_tool_call",
                "name": "apply_patch",
                "call_id": "call_custom",
                "arguments": "{\"patch\":\"x\"}"
            }),
            json!({
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "output": "custom tool result"
            }),
            compaction_prompt_item(),
        ];
        let tail = build_tail_section(&input, 10_000, true);
        assert!(tail.text.contains("apply_patch"));
        assert!(tail.text.contains("custom tool result"));

        let orphan = vec![
            json!({
                "type": "custom_tool_call_output",
                "call_id": "missing_call",
                "output": "orphan must be dropped"
            }),
            compaction_prompt_item(),
        ];
        let orphan_tail = build_tail_section(&orphan, 10_000, true);
        assert!(!orphan_tail.text.contains("orphan must be dropped"));
    }

    #[test]
    fn tool_call_pairs_never_exceed_hard_budget() {
        let budget_chars = retain_budget_chars(MIN_RETAIN_TOKENS);
        let mut input = Vec::new();
        for index in 0..20 {
            let call_id = format!("call_{index}");
            input.push(json!({
                "type": "function_call",
                "call_id": call_id,
                "name": "exec_command",
                "arguments": "a".repeat(MAX_TOOL_ITEM_CHARS * 2)
            }));
            input.push(function_call_output_item(
                &format!("call_{index}"),
                &"b".repeat(MAX_TOOL_ITEM_CHARS * 2),
            ));
        }
        input.push(compaction_prompt_item());

        let tail = build_tail_section(&input, budget_chars, true);
        assert!(tail.text.chars().count() <= budget_chars);
        assert_eq!(tail.chars as usize, tail.text.chars().count());
        for index in 0..20 {
            let call_id = format!("call_{index}");
            let occurrences = tail.text.matches(&call_id).count();
            assert!(
                occurrences == 0 || occurrences >= 2,
                "tool output must not be retained without its paired call"
            );
        }
    }

    /// 问题2（核心场景）：最近大量连续工具调用比 message 预算大得多时，
    /// 仍应能穿透这些工具调用回溯到更早的真实用户/助手消息，而不是被工具
    /// 调用完全挤满。
    #[test]
    fn message_survives_when_recent_history_is_all_tool_calls() {
        let mut input = vec![
            assistant_message("the actual assistant plan we care about"),
            assistant_message("acknowledged, starting work"),
        ];
        // 模拟最近 20 轮工具调用，每条输出都接近单条截断上限，总量远超过
        // 整个预算，模拟你提出的“最近 10000 token 都是工具调用”场景。
        for i in 0..20 {
            let call_id = format!("call_{i}");
            input.push(function_call_item(&call_id, "exec_command"));
            input.push(function_call_output_item(&call_id, &"z".repeat(3_000)));
        }
        input.push(compaction_prompt_item());
        let request = json!({ "input": input });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        assert!(
            text.contains("the actual assistant plan we care about"),
            "message budget must reserve room for the earlier assistant message even when \
recent history is dominated by tool calls"
        );
    }

    #[test]
    fn non_completed_status_unchanged() {
        let request = json!({
            "input": [user_message("hi"), assistant_message("ok"), compaction_prompt_item()]
        });
        let sse = "event: response.incomplete\ndata: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"output\":[]}}\n\n".to_string();
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            sse.clone(),
        );
        assert!(!result.triggered);
        assert_eq!(result.sse_text, sse);
    }
}
