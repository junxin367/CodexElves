//! Responses 协议"续思考"逻辑：当上游 GPT reasoning 命中固定量化网格（很可能是
//! 加密 reasoning 分块导致 usage.reasoning_tokens 恒为 518 的整数倍减 2）且思考量
//! 明显偏少（n 太小）时，把上一轮 reasoning（含 encrypted_content）连同一个伪造的
//! continue_thinking 工具调用/返回回传给上游，让模型从截断处继续思考，而不是
//! 直接把思考不足产出的错误答案发给客户端。
//!
//! 仅覆盖 GPT + Responses 直连协议；Chat Completions / Anthropic 路径不处理。

use serde_json::{Value, json};

/// reasoning token 的量化网格步长（经真实糖果题采样验证：所有 reasoning_tokens
/// 均满足 `(tokens + GRID_OFFSET) % GRID_STEP == 0`）。
pub const GRID_STEP: u64 = 518;
const GRID_OFFSET: u64 = 2;

/// 判定思考量是否达标所需的最小网格倍数。n < MIN_GRID_MULTIPLE 时续写。
/// 实测：n=1（516 token）基本必错；n>=5 均答对；此处取中间偏保守值。
pub const MIN_GRID_MULTIPLE: u64 = 3;

/// 最多续写轮数，防止死循环/失控成本。
pub const MAX_CONTINUE_ROUNDS: u32 = 3;

const CONTINUE_TOOL_NAME: &str = "continue_thinking";
const CONTINUE_TOOL_OUTPUT: &str = "Please continue thinking about the query.";
const WEBSOCKET_CONTINUE_DRAFT_INSTRUCTION: &str = "The previous assistant response is an \
unpublished, incomplete draft generated inside the proxy. Continue reasoning from the available \
context and produce a complete replacement answer. Do not mention, quote, or defer to the draft.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketContinueMode {
    StatelessReplay,
    LatestResponse,
}

impl WebSocketContinueMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StatelessReplay => "stateless_replay",
            Self::LatestResponse => "latest_response",
        }
    }
}

pub struct WebSocketContinueRequest {
    pub request: Value,
    pub mode: WebSocketContinueMode,
}

/// 计算 reasoning_tokens 落在网格上的倍数 n；不在网格上返回 None。
pub fn grid_multiple(reasoning_tokens: u64) -> Option<u64> {
    if reasoning_tokens == 0 {
        return None;
    }
    let numerator = reasoning_tokens + GRID_OFFSET;
    if numerator % GRID_STEP == 0 {
        Some(numerator / GRID_STEP)
    } else {
        None
    }
}

/// 是否需要续写：命中网格 且 倍数低于阈值。
pub fn should_continue_thinking(reasoning_tokens: Option<u64>) -> bool {
    match reasoning_tokens.and_then(grid_multiple) {
        Some(n) => n < MIN_GRID_MULTIPLE,
        None => false,
    }
}

/// 模型名是否属于本功能覆盖范围（GPT 系列）。
pub fn is_supported_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gpt-")
}

/// 工具声明：追加到请求 tools 数组中，使伪造的 function_call/function_call_output
/// 在 Responses 协议层面合法。
fn continue_thinking_tool_declaration() -> Value {
    json!({
        "type": "function",
        "name": CONTINUE_TOOL_NAME,
        "description": "Continue the previous reasoning without restarting.",
        "parameters": {
            "type": "object",
            "properties": { "continue": { "type": "boolean" } },
            "required": ["continue"]
        }
    })
}

/// 基于原始请求 + 上一轮 output items，构造续写请求体。
///
/// - `original_request`: 客户端发来的原始 Responses 请求 JSON（含 input/tools/reasoning 等）。
/// - `previous_output_items`: 上一轮 `response.completed` 里的权威 `output` 数组
///   只会回传 reasoning item（需带 encrypted_content 才有效），刻意丢弃
///   上一轮最终答案 message，避免模型被错误答案锚定。
/// - `round_index`: 第几次续写（从 1 开始），用于生成唯一 call_id。
pub fn build_continue_request(
    original_request: &Value,
    previous_output_items: &[Value],
    round_index: u32,
) -> Value {
    let mut request = original_request.clone();

    let mut input = original_request
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    input.extend(previous_output_items.iter().filter_map(|item| {
        if item.get("type").and_then(Value::as_str) == Some("reasoning")
            && item
                .get("encrypted_content")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        {
            Some(item.clone())
        } else {
            None
        }
    }));

    let call_id = format!("call_continue_thinking_{round_index}");
    input.push(json!({
        "type": "function_call",
        "name": CONTINUE_TOOL_NAME,
        "arguments": "{\"continue\":true}",
        "call_id": call_id
    }));
    input.push(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": CONTINUE_TOOL_OUTPUT
    }));

    request["input"] = Value::Array(input);

    let mut tools = original_request
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let already_declared = tools
        .iter()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some(CONTINUE_TOOL_NAME));
    if !already_declared {
        tools.push(continue_thinking_tool_declaration());
    }
    request["tools"] = Value::Array(tools);

    request
}

/// 从 `response.completed` / `response.incomplete` 事件的 `response` 字段里
/// 提取 reasoning_tokens。
pub fn extract_reasoning_tokens(response_object: &Value) -> Option<u64> {
    response_object
        .pointer("/usage/output_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64)
}

/// 从 `response.completed` / `response.incomplete` 事件的 `response` 字段里
/// 提取权威 output items 数组。
pub fn extract_output_items(response_object: &Value) -> Vec<Value> {
    response_object
        .get("output")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn is_tool_call_output_item_type(kind: &str) -> bool {
    matches!(
        kind,
        "function_call"
            | "custom_tool_call"
            | "tool_call"
            | "tool_search_call"
            | "web_search_call"
            | "file_search_call"
            | "computer_call"
            | "computer_use_call"
            | "local_shell_call"
            | "code_interpreter_call"
            | "image_generation_call"
            | "mcp_call"
    ) || kind.ends_with("_tool_call")
}

/// 终止响应已经在请求工具调用时，不再把低 reasoning token 视为需要续写。
/// 这类响应不是最终答案，后续应由正常工具调用链路推进。
pub fn response_tool_call_types(response_object: &Value) -> Vec<String> {
    response_object
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("type").and_then(Value::as_str))
                .filter(|kind| is_tool_call_output_item_type(kind))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 终止响应已经在请求工具调用时，不再把低 reasoning token 视为需要续写。
/// 这类响应不是最终答案，后续应由正常工具调用链路推进。
pub fn response_contains_tool_call(response_object: &Value) -> bool {
    response_object
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(is_tool_call_output_item_type)
            })
        })
}

/// 是否应对当前终止响应发起下一轮续思考。
///
/// HTTP/SSE 与 WebSocket 路径共用同一判定：reasoning token 命中低网格，
/// 且当前响应不是等待客户端处理的工具调用。
pub fn should_continue_response(response_object: &Value) -> bool {
    should_continue_thinking(extract_reasoning_tokens(response_object))
        && !response_contains_tool_call(response_object)
}

fn remove_websocket_transport_fields(request: &mut Value) {
    let Some(object) = request.as_object_mut() else {
        return;
    };
    object.remove("stream");
    object.remove("stream_options");
    object.remove("background");
}

fn request_uses_server_managed_context(request: &Value) -> bool {
    request
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
        || request
            .get("conversation")
            .is_some_and(|value| !value.is_null())
        || request
            .get("conversation_id")
            .is_some_and(|value| !value.is_null())
}

fn prepare_websocket_stateless_continue_request(mut request: Value) -> Value {
    request["type"] = Value::String("response.create".to_string());
    if let Some(object) = request.as_object_mut() {
        object.remove("previous_response_id");
        object.remove("conversation");
        object.remove("conversation_id");
    }
    remove_websocket_transport_fields(&mut request);
    request
}

fn build_websocket_latest_response_request(original_request: &Value, response_id: &str) -> Value {
    let mut request = original_request.clone();
    request["type"] = Value::String("response.create".to_string());
    request["previous_response_id"] = Value::String(response_id.to_string());
    request["input"] = json!([{
        "type": "message",
        "role": "developer",
        "content": [{
            "type": "input_text",
            "text": WEBSOCKET_CONTINUE_DRAFT_INSTRUCTION
        }]
    }]);
    if let Some(object) = request.as_object_mut() {
        object.remove("conversation");
        object.remove("conversation_id");
    }
    remove_websocket_transport_fields(&mut request);
    request
}

/// 构造 Responses WebSocket 自动推理续接请求。
///
/// - 原请求不依赖服务端会话状态时，重放原始输入和 encrypted reasoning，并丢弃草稿答案。
/// - 原请求只携带增量 input 时，必须使用刚完成的 response ID，避免旧 ID 已从连接缓存淘汰，
///   同时不能删除 ID 后丢失此前会话上下文。
pub fn build_websocket_continue_request(
    original_request: &Value,
    response_object: &Value,
    round_index: u32,
) -> Option<WebSocketContinueRequest> {
    if request_uses_server_managed_context(original_request) {
        let response_id = response_object
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())?;
        return Some(WebSocketContinueRequest {
            request: build_websocket_latest_response_request(original_request, response_id),
            mode: WebSocketContinueMode::LatestResponse,
        });
    }

    let request = build_continue_request(
        original_request,
        &extract_output_items(response_object),
        round_index,
    );
    Some(WebSocketContinueRequest {
        request: prepare_websocket_stateless_continue_request(request),
        mode: WebSocketContinueMode::StatelessReplay,
    })
}

/// 从一段完整的 Responses SSE 文本中提取终止事件（response.completed /
/// response.incomplete / response.failed）里的 `response` 对象。取最后一个
/// 匹配的事件（正常情况下每轮响应只有一个终止事件）。
pub fn extract_terminal_response_object(sse_text: &str) -> Option<Value> {
    let mut result = None;
    for block in sse_text.split("\n\n") {
        for line in block.lines() {
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
            if matches!(
                event_type,
                "response.completed" | "response.incomplete" | "response.failed"
            ) {
                if let Some(response) = event.get("response") {
                    result = Some(response.clone());
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_multiple_matches_observed_samples() {
        assert_eq!(grid_multiple(516), Some(1));
        assert_eq!(grid_multiple(1034), Some(2));
        assert_eq!(grid_multiple(2588), Some(5));
        assert_eq!(grid_multiple(10876), Some(21));
    }

    #[test]
    fn grid_multiple_rejects_off_grid_values() {
        assert_eq!(grid_multiple(1200), None);
        assert_eq!(grid_multiple(0), None);
        assert_eq!(grid_multiple(517), None);
    }

    #[test]
    fn should_continue_thinking_only_for_low_multiples() {
        assert!(should_continue_thinking(Some(516))); // n=1
        assert!(should_continue_thinking(Some(1034))); // n=2
        assert!(!should_continue_thinking(Some(1552))); // n=3, 已达阈值
        assert!(!should_continue_thinking(Some(2588))); // n=5
        assert!(!should_continue_thinking(Some(1200))); // 不在网格上
        assert!(!should_continue_thinking(None));
    }

    #[test]
    fn should_continue_response_skips_tool_calls() {
        let response = json!({
            "usage": {"output_tokens_details": {"reasoning_tokens": 516}},
            "output": []
        });
        assert!(should_continue_response(&response));

        let tool_response = json!({
            "usage": {"output_tokens_details": {"reasoning_tokens": 516}},
            "output": [{
                "type": "function_call",
                "name": "lookup",
                "call_id": "call_1",
                "arguments": "{}"
            }]
        });
        assert!(!should_continue_response(&tool_response));
    }

    #[test]
    fn websocket_stateless_continue_request_replays_reasoning_without_session_fields() {
        let original = json!({
            "model": "gpt-test",
            "input": [{"type": "message", "role": "user", "content": []}],
            "stream": true,
            "stream_options": {"include_usage": true},
            "background": false,
            "include": ["reasoning.encrypted_content"]
        });
        let response = json!({
            "id": "resp_short",
            "output": [{
                "id": "rs_short",
                "type": "reasoning",
                "encrypted_content": "encrypted-short"
            }]
        });
        let continued = build_websocket_continue_request(&original, &response, 1).unwrap();
        let request = continued.request;

        assert_eq!(continued.mode, WebSocketContinueMode::StatelessReplay);
        assert_eq!(request["type"], "response.create");
        assert!(request.get("stream").is_none());
        assert!(request.get("stream_options").is_none());
        assert!(request.get("background").is_none());
        assert!(request.get("previous_response_id").is_none());
        assert!(request.get("conversation").is_none());
        assert!(request.get("conversation_id").is_none());
        assert!(
            request["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["encrypted_content"] == "encrypted-short")
        );
    }

    #[test]
    fn websocket_stateful_continue_request_uses_latest_response_without_replaying_user_input() {
        let original = json!({
            "model": "gpt-test",
            "instructions": "keep the original behavior",
            "previous_response_id": "resp_parent",
            "conversation_id": "legacy-conversation",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "current user turn"}]
            }],
            "stream": true,
            "stream_options": {"include_usage": true},
            "background": false,
            "include": ["reasoning.encrypted_content"]
        });
        let response = json!({
            "id": "resp_short",
            "output": [{
                "id": "rs_short",
                "type": "reasoning",
                "encrypted_content": "encrypted-short"
            }]
        });
        let continued = build_websocket_continue_request(&original, &response, 1).unwrap();
        let request = continued.request;
        let input = request["input"].as_array().unwrap();

        assert_eq!(continued.mode, WebSocketContinueMode::LatestResponse);
        assert_eq!(request["type"], "response.create");
        assert_eq!(request["previous_response_id"], "resp_short");
        assert_eq!(request["instructions"], "keep the original behavior");
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "developer");
        assert!(!request.to_string().contains("current user turn"));
        assert!(!request.to_string().contains("encrypted-short"));
        assert!(request.get("conversation").is_none());
        assert!(request.get("conversation_id").is_none());
        assert!(request.get("stream").is_none());
        assert!(request.get("stream_options").is_none());
        assert!(request.get("background").is_none());
    }

    #[test]
    fn websocket_stateful_continue_request_requires_latest_response_id() {
        let original = json!({
            "model": "gpt-test",
            "previous_response_id": "resp_parent",
            "input": []
        });

        assert!(build_websocket_continue_request(&original, &json!({}), 1).is_none());
    }

    #[test]
    fn is_supported_model_only_matches_gpt() {
        assert!(is_supported_model("gpt-5.5"));
        assert!(is_supported_model("GPT-5"));
        assert!(!is_supported_model("claude-opus-4-8"));
        assert!(!is_supported_model("deepseek-v4-pro"));
    }

    #[test]
    fn build_continue_request_injects_history_and_tool_roundtrip() {
        let original = json!({
            "model": "gpt-5.5",
            "stream": true,
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        });
        let previous_items = vec![json!({
            "id": "rs_1",
            "type": "reasoning",
            "encrypted_content": "abc123"
        })];

        let continued = build_continue_request(&original, &previous_items, 1);
        let input = continued["input"].as_array().unwrap();
        // 原始 user message + reasoning item + function_call + function_call_output
        assert_eq!(input.len(), 4);
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[1]["encrypted_content"], "abc123");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["name"], "continue_thinking");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], input[2]["call_id"]);

        let tools = continued["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "continue_thinking"));
    }

    #[test]
    fn build_continue_request_does_not_duplicate_tool_declaration() {
        let original = json!({
            "model": "gpt-5.5",
            "input": [],
            "tools": [{"type": "function", "name": "continue_thinking"}]
        });
        let continued = build_continue_request(&original, &[], 1);
        let tools = continued["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn build_continue_request_drops_previous_final_answer_message() {
        let original = json!({
            "model": "gpt-5.5",
            "input": [{"type": "message", "role": "user", "content": []}]
        });
        let previous_items = vec![
            json!({
                "id": "rs_1",
                "type": "reasoning",
                "encrypted_content": "abc123"
            }),
            json!({
                "id": "msg_wrong",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "错误答案 29"}]
            }),
        ];

        let continued = build_continue_request(&original, &previous_items, 1);
        let input = continued["input"].as_array().unwrap();
        assert_eq!(input.len(), 4);
        assert_eq!(input[1]["type"], "reasoning");
        assert!(
            !input
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some("msg_wrong"))
        );
    }

    #[test]
    fn extract_reasoning_tokens_reads_nested_usage() {
        let response = json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });
        assert_eq!(extract_reasoning_tokens(&response), Some(516));
        assert_eq!(extract_reasoning_tokens(&json!({})), None);
    }

    #[test]
    fn extract_output_items_returns_output_array() {
        let response = json!({ "output": [{"type": "reasoning"}, {"type": "message"}] });
        assert_eq!(extract_output_items(&response).len(), 2);
        assert!(extract_output_items(&json!({})).is_empty());
    }

    #[test]
    fn response_contains_tool_call_detects_tool_call_outputs() {
        let response = json!({
            "output": [
                {"type": "reasoning"},
                {"type": "function_call", "name": "exec_command"},
                {"type": "custom_tool_call", "name": "apply_patch"},
                {"type": "local_shell_call", "name": "shell"},
                {"type": "web_search_call", "name": "search"},
                {"type": "tool_search_call", "name": "tool_search"}
            ]
        });

        assert!(response_contains_tool_call(&response));
        assert_eq!(
            response_tool_call_types(&response),
            vec![
                "function_call",
                "custom_tool_call",
                "local_shell_call",
                "web_search_call",
                "tool_search_call"
            ]
        );
    }

    #[test]
    fn response_contains_tool_call_ignores_final_message_outputs() {
        let response = json!({
            "output": [
                {"type": "reasoning"},
                {"type": "message", "content": [{"type": "output_text", "text": "done"}]},
                {"type": "function_call_output", "output": "tool result"},
                {"type": "unknown_call", "output": "not a known tool call"}
            ]
        });

        assert!(!response_contains_tool_call(&response));
        assert!(!response_contains_tool_call(&json!({})));
    }

    #[test]
    fn extract_terminal_response_object_finds_completed_event() {
        let sse = "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n";
        let response = extract_terminal_response_object(sse).expect("should find terminal event");
        assert_eq!(response["status"], "completed");
        assert_eq!(extract_reasoning_tokens(&response), Some(516));
    }

    #[test]
    fn extract_terminal_response_object_returns_none_without_terminal_event() {
        let sse = "event: response.created\ndata: {\"type\":\"response.created\"}\n\n";
        assert!(extract_terminal_response_object(sse).is_none());
    }
}
