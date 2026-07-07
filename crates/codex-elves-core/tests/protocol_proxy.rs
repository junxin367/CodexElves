use codex_elves_core::protocol_proxy::{
    AnthropicSseToResponsesConverter, ChatSseToResponsesConverter, UpstreamResponseProtocol,
    anthropic_message_to_response_with_request, anthropic_messages_url,
    anthropic_sse_to_responses_sse_with_request, apply_continue_thinking_to_responses_stream,
    chat_completion_to_response, chat_completion_to_response_with_request, chat_completions_url,
    chat_sse_to_responses_sse, chat_sse_to_responses_sse_with_request,
    handle_responses_proxy_request, is_chat_completions_proxy_path, is_models_proxy_path,
    is_responses_proxy_path, models_url, open_chat_completions_proxy_request,
    open_models_proxy_request, open_responses_proxy_request,
    open_responses_proxy_request_with_settings, responses_error_from_upstream,
    responses_to_anthropic_messages, responses_to_chat_completions,
    send_upstream_request_with_header_timeout, supported_reasoning_efforts_for_model,
    upstream_deferred_stream_header_timeout, upstream_http_client, upstream_models_header_timeout,
};
use codex_elves_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, AggregateRelayStrategy, BackendSettings,
    RelayMode, RelayModelMapping, RelayProfile, RelayProtocol,
};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
struct ParsedSseEvent {
    event: String,
    data: Value,
}

fn parse_response_sse_events(input: &str) -> Vec<ParsedSseEvent> {
    input
        .split("\n\n")
        .filter_map(|block| {
            let mut event = String::new();
            let mut data_parts = Vec::new();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    event = value.to_string();
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data_parts.push(value);
                }
            }
            if data_parts.is_empty() || data_parts == ["[DONE]"] {
                return None;
            }
            let data = serde_json::from_str::<Value>(&data_parts.join("\n")).ok()?;
            if event.is_empty() {
                event = data
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            Some(ParsedSseEvent { event, data })
        })
        .collect()
}

fn responses_sse_with_reasoning(response_id: &str, reasoning_tokens: u64) -> String {
    responses_sse_with_reasoning_and_output(response_id, reasoning_tokens, json!([]))
}

fn responses_sse_with_reasoning_and_output(
    response_id: &str,
    reasoning_tokens: u64,
    output: Value,
) -> String {
    let output = serde_json::to_string(&output).unwrap();
    format!(
        "event: response.completed\n\
data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"{response_id}\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"gpt-responses\",\"output\":{output},\"usage\":{{\"output_tokens_details\":{{\"reasoning_tokens\":{reasoning_tokens}}}}}}}}}\n\n\
data: [DONE]\n\n"
    )
}

#[test]
fn responses_request_converts_to_chat_completions() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "instructions": "You are helpful.",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ],
        "max_output_tokens": 512,
        "temperature": 0.2,
        "stream": true,
        "tools": [
            {
                "type": "function",
                "name": "lookup",
                "description": "Lookup data",
                "parameters": { "type": "object" }
            }
        ]
    }))
    .unwrap();

    assert_eq!(
        converted,
        json!({
            "model": "gpt-5-mini",
            "messages": [
                { "role": "system", "content": "You are helpful." },
                { "role": "user", "content": "hello" }
            ],
            "max_tokens": 512,
            "temperature": 0.2,
            "stream": true,
            "stream_options": { "include_usage": true },
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "description": "Lookup data",
                        "parameters": { "type": "object", "properties": {}, "required": [] }
                    }
                }
            ]
        })
    );
}

#[test]
fn responses_request_converts_to_anthropic_messages() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-4",
        "instructions": "You are helpful.",
        "input": [
            {
                "type": "message",
                "role": "developer",
                "content": [
                    { "type": "input_text", "text": "Prefer concise answers." }
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,aGVsbG8="
                    }
                ]
            }
        ],
        "max_output_tokens": 512,
        "temperature": 0.2,
        "stream": true,
        "tools": [
            {
                "type": "function",
                "name": "lookup",
                "description": "Lookup data",
                "parameters": { "type": "object" }
            }
        ],
        "tool_choice": { "type": "function", "name": "lookup" }
    }))
    .unwrap();

    assert_eq!(converted["model"], "claude-sonnet-4");
    assert_eq!(converted["max_tokens"], 512);
    let system = converted["system"].as_str().unwrap();
    assert_eq!(system, "You are helpful.\n\nPrefer concise answers.");
    assert_eq!(converted["messages"][0]["role"], "user");
    assert_eq!(converted["messages"][0]["content"][0]["text"], "hello");
    assert_eq!(
        converted["messages"][0]["content"][1],
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "aGVsbG8="
            }
        })
    );
    assert_eq!(converted["tools"][0]["name"], "lookup");
    assert_eq!(
        converted["tools"][0]["input_schema"],
        json!({ "type": "object", "properties": {}, "required": [] })
    );
    assert_eq!(
        converted["tool_choice"],
        json!({ "type": "tool", "name": "lookup" })
    );
    assert_eq!(converted["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(converted["output_config"], json!({ "effort": "high" }));
}

#[test]
fn anthropic_tool_schema_flattens_top_level_union() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": "check automations",
        "tools": [
            {
                "type": "function",
                "name": "codex_app__automation_update",
                "description": "Create, update, view, or delete recurring automations.",
                "parameters": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "mode": { "type": "string", "enum": ["view"] }
                            },
                            "required": ["mode", "id"],
                            "additionalProperties": false
                        },
                        {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": {
                                        "mode": { "type": "string", "enum": ["create"] },
                                        "name": { "type": "string" },
                                        "prompt": { "type": "string" }
                                    },
                                    "required": ["mode", "name", "prompt"],
                                    "additionalProperties": false
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "mode": { "type": "string", "enum": ["delete"] }
                                    },
                                    "required": ["mode", "id"],
                                    "additionalProperties": false
                                }
                            ]
                        }
                    ],
                    "$defs": {
                        "id": { "type": "string" }
                    }
                }
            }
        ]
    }))
    .unwrap();

    let schema = &converted["tools"][0]["input_schema"];
    assert_eq!(
        converted["tools"][0]["name"],
        "codex_app__automation_update"
    );
    assert_eq!(schema["type"], "object");
    assert!(schema.get("oneOf").is_none());
    assert!(schema.get("anyOf").is_none());
    assert!(schema.get("allOf").is_none());
    assert_eq!(schema["required"], json!(["mode"]));
    assert!(schema["properties"].get("id").is_some());
    assert!(schema["properties"].get("name").is_some());
    assert!(schema["properties"].get("prompt").is_some());
    let mode_enum = schema["properties"]["mode"]["enum"].as_array().unwrap();
    assert!(mode_enum.contains(&json!("view")));
    assert!(mode_enum.contains(&json!("create")));
    assert!(mode_enum.contains(&json!("delete")));
    assert!(schema.get("$defs").is_some());
}

#[test]
fn anthropic_request_keeps_agents_context_name_and_dedupes_repeated_blocks() {
    let agents = "# AGENTS.md instructions for E:\\code\\junes\\github\\CodexElves\n\n<INSTRUCTIONS>\n默认使用简体中文。\n</INSTRUCTIONS>";
    let environment = "<environment_context>\n  <cwd>E:\\code\\junes\\github\\CodexElves</cwd>\n</environment_context>";
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-5",
        "instructions": "You are CodexElves.",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": agents },
                    { "type": "input_text", "text": environment },
                    { "type": "input_text", "text": agents },
                    { "type": "input_text", "text": environment },
                    { "type": "input_text", "text": "真实用户问题" }
                ]
            }
        ],
        "max_output_tokens": 512
    }))
    .unwrap();

    assert_eq!(converted["system"], "You are CodexElves.");
    let content = converted["messages"][0]["content"].as_array().unwrap();
    let texts = content
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts.len(), 3);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.starts_with("# AGENTS.md instructions for "))
            .count(),
        1
    );
    assert!(texts.iter().all(|text| !text.contains("CLAUDE.md")));
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.starts_with("<environment_context>"))
            .count(),
        1
    );
    assert!(texts.contains(&"真实用户问题"));
}

#[test]
fn anthropic_request_dedupes_repeated_system_chunks() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-5",
        "instructions": "You are CodexElves.",
        "input": [
            {
                "type": "message",
                "role": "developer",
                "content": [
                    { "type": "input_text", "text": "You are CodexElves." }
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ],
        "max_output_tokens": 512
    }))
    .unwrap();

    assert_eq!(converted["system"], "You are CodexElves.");
    assert_eq!(converted["messages"][0]["role"], "user");
    assert_eq!(converted["messages"][0]["content"][0]["text"], "hello");
}

#[test]
fn anthropic_request_serializes_system_before_messages() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-5",
        "instructions": "You are CodexElves.",
        "input": "hello",
        "max_output_tokens": 512
    }))
    .unwrap();

    let body = serde_json::to_string(&converted).unwrap();
    let max_tokens_index = body.find("\"max_tokens\"").unwrap();
    let system_index = body.find("\"system\"").unwrap();
    let messages_index = body.find("\"messages\"").unwrap();
    assert!(max_tokens_index < system_index);
    assert!(system_index < messages_index);
}

#[test]
fn anthropic_reasoning_effort_is_clamped_by_model_capability() {
    let sonnet = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-4-6",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(sonnet["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(sonnet["output_config"], json!({ "effort": "high" }));

    let opus = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-6",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(opus["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(opus["output_config"], json!({ "effort": "max" }));

    let sonnet5 = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-5",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(sonnet5["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(sonnet5["output_config"], json!({ "effort": "max" }));
}

#[test]
fn anthropic_reasoning_reads_effort_from_model_reasoning_effort_when_reasoning_absent() {
    // App 在自定义模型下可能不发 reasoning 对象，而把思考深度放在顶层 model_reasoning_effort，
    // 协议代理需兜底读取，避免思考深度丢失（被转成 disabled）。
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "model_reasoning_effort": "high",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(converted["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(converted["output_config"], json!({ "effort": "high" }));
}

#[test]
fn anthropic_reasoning_defaults_to_enabled_when_reasoning_is_null() {
    // reasoning 显式为 null 且无任何 effort 字段时，不应被判定为关闭思考，
    // 而是按默认开启（adaptive），避免 CPA 后台显示 none。
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "reasoning": serde_json::Value::Null,
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(converted["thinking"], json!({ "type": "adaptive" }));
    assert!(converted.get("output_config").is_some());
}

#[test]
fn anthropic_message_response_converts_to_responses() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4",
            "content": [
                { "type": "thinking", "thinking": "plan" },
                { "type": "text", "text": "answer" },
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "lookup",
                    "input": { "query": "codex" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "cache_read_input_tokens": 2,
                "output_tokens_details": { "thinking_tokens": 3 }
            }
        }),
        &json!({
            "model": "claude-sonnet-4",
            "input": "hello",
            "tools": [
                {
                    "type": "function",
                    "name": "lookup",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["id"], "resp_msg_test");
    assert_eq!(converted["status"], "completed");
    assert_eq!(converted["output"][0]["type"], "reasoning");
    assert_eq!(converted["output"][0]["reasoning_content"], "plan");
    assert_eq!(converted["output"][1]["type"], "message");
    assert_eq!(converted["output"][1]["content"][0]["text"], "answer");
    assert_eq!(converted["output"][2]["type"], "function_call");
    assert_eq!(converted["output"][2]["name"], "lookup");
    assert_eq!(converted["output"][2]["arguments"], r#"{"query":"codex"}"#);
    assert_eq!(converted["usage"]["input_tokens"], 10);
    assert_eq!(converted["usage"]["output_tokens"], 5);
    assert_eq!(converted["usage"]["cache_read_input_tokens"], 2);
    assert_eq!(
        converted["usage"]["output_tokens_details"]["thinking_tokens"],
        3
    );
    assert_eq!(
        converted["usage"]["output_tokens_details"]["reasoning_tokens"],
        3
    );
}

#[test]
fn anthropic_request_declares_web_search_as_server_side_tool_without_fallback() {
    // 无 MCP 搜索 fallback 时，web_search 应声明为 Anthropic 原生 server-side 工具，
    // 由 Claude 服务端自己执行搜索，避免被当客户端工具导致空结果死循环。
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": "search the web",
        "tools": [{ "type": "web_search" }]
    }))
    .unwrap();

    let tools = converted["tools"].as_array().unwrap();
    let web_search = tools
        .iter()
        .find(|tool| tool["name"] == "web_search")
        .expect("web_search tool present");
    assert_eq!(web_search["type"], "web_search_20250305");
    // 不应再是普通 function 形态（无 input_schema）。
    assert!(web_search.get("input_schema").is_none());
}

#[test]
fn anthropic_request_keeps_web_search_as_function_when_mcp_fallback_available() {
    // 有 MCP 搜索 fallback（如 tavily）时，客户端有真实执行能力，
    // 保留原有可用路径，不切换为 server-side。
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": "search the web",
        "tools": [{ "type": "web_search" }, tavily_namespace_tool()]
    }))
    .unwrap();

    let tools = converted["tools"].as_array().unwrap();
    // 没有 server-side web_search_20250305。
    assert!(
        !tools
            .iter()
            .any(|tool| tool["type"] == "web_search_20250305")
    );
}

#[test]
fn anthropic_request_downgrades_tool_choice_forcing_web_search_to_auto() {
    // Anthropic 不允许用 tool_choice:tool 强制 server-side 工具，命中时应降级为 auto。
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": "search the web",
        "tools": [{ "type": "web_search" }],
        "tool_choice": { "type": "function", "name": "web_search" }
    }))
    .unwrap();

    assert_eq!(converted["tool_choice"], json!({ "type": "auto" }));
}

#[test]
fn anthropic_server_tool_use_web_search_maps_to_web_search_call() {
    // Claude 原生 server-side web_search 响应（server_tool_use + web_search_tool_result）：
    // server_tool_use 转为 Codex web_search_call 进度项；web_search_tool_result 不透传给客户端；
    // 最终 text 正常输出。
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_ws_server",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "server_tool_use",
                    "id": "srvtoolu_1",
                    "name": "web_search",
                    "input": { "query": "claude shannon birth" }
                },
                {
                    "type": "web_search_tool_result",
                    "tool_use_id": "srvtoolu_1",
                    "content": [{ "type": "web_search_result", "url": "https://example.com", "title": "X" }]
                },
                { "type": "text", "text": "Claude Shannon was born in 1916." }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "search",
            "tools": [{ "type": "web_search" }]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    let serialized = converted["output"].to_string();
    // 含 web_search_call 进度项。
    assert!(output.iter().any(|item| item["type"] == "web_search_call"));
    // 含最终 text。
    assert!(serialized.contains("Claude Shannon was born in 1916."));
    // web_search_tool_result 不透传。
    assert!(!serialized.contains("web_search_tool_result"));
}

#[test]
fn anthropic_message_response_maps_web_search_to_native_call() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_web_search",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4",
            "content": [{
                "type": "tool_use",
                "id": "toolu_web",
                "name": "web_search",
                "input": { "query": "pal mcp GitHub" }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-sonnet-4",
            "input": "search",
            "tools": [{ "type": "web_search" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "web_search_call");
    assert_eq!(converted["output"][0]["id"], "ws_toolu_web");
    assert_eq!(converted["output"][0]["status"], "completed");
    assert_eq!(converted["output"][0]["execution"], "client");
    assert_eq!(converted["output"][0]["action"]["type"], "search");
    assert_eq!(converted["output"][0]["action"]["query"], "pal mcp GitHub");
    assert_eq!(
        converted["output"][0]["action"]["queries"],
        json!(["pal mcp GitHub"])
    );
}

#[test]
fn anthropic_message_response_maps_web_search_to_search_mcp_when_available() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_web_search",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4",
            "content": [{
                "type": "tool_use",
                "id": "toolu_web",
                "name": "web_search",
                "input": { "query": "pal mcp GitHub" }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-sonnet-4",
            "input": "search",
            "tools": [
                { "type": "web_search" },
                tavily_namespace_tool()
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["call_id"], "toolu_web");
    assert_eq!(converted["output"][0]["name"], "tavily_search");
    assert_eq!(converted["output"][0]["namespace"], "mcp__tavily");
    assert_eq!(
        converted["output"][0]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[test]
fn anthropic_textual_invoke_response_converts_to_tool_call() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_textual_tool",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "course\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git diff crates/codex-elves-core/src/protocol_proxy.rs</parameter>\n<parameter name=\"yield_time_ms\">3000</parameter>\n<parameter name=\"max_output_tokens\">6000</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "检查 diff",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cmd": { "type": "string" },
                            "yield_time_ms": { "type": "integer" },
                            "max_output_tokens": { "type": "integer" }
                        }
                    }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["name"], "exec_command");
    assert_eq!(
        converted["output"][0]["arguments"],
        r#"{"cmd":"git diff crates/codex-elves-core/src/protocol_proxy.rs","max_output_tokens":"6000","yield_time_ms":"3000"}"#
    );
}

#[test]
fn anthropic_call_prefixed_textual_invoke_response_converts_to_tool_call() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_textual_call_tool",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "检查状态",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cmd": { "type": "string" }
                        }
                    }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["name"], "exec_command");
    assert_eq!(
        converted["output"][0]["arguments"],
        r#"{"cmd":"git status --short"}"#
    );
}

#[test]
fn anthropic_textual_invoke_exec_command_allows_invoke_text_inside_parameter() {
    let command = r#"cd E:\code\junes\github\CodexElves; rg -n "invoke|textual_invoke|call_prefixed|<invoke|antml_tool_call|parse_textual|extract_tool" crates/codex-elves-core/src/protocol_proxy.rs | Select-Object -First 40"#;
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_textual_exec_with_invoke_text",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": format!(
                        "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">{command}</parameter>\n</invoke>"
                    )
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "定位协议转换",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": { "cmd": { "type": "string" } }
                    }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["name"], "exec_command");
    assert_eq!(
        converted["output"][0]["arguments"],
        json!({ "cmd": command }).to_string()
    );
}

#[test]
fn anthropic_textual_invoke_exec_command_keeps_json_like_parameter_as_string() {
    let command = r#"{"query":"codex"}"#;
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_textual_exec_json_like_string",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": format!(
                        "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">{command}</parameter>\n</invoke>"
                    )
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "执行 JSON 字符串命令",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": { "cmd": { "type": "string" } }
                    }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["name"], "exec_command");
    assert_eq!(
        converted["output"][0]["arguments"],
        json!({ "cmd": command }).to_string()
    );
}

#[test]
fn anthropic_textual_invoke_with_only_descriptive_invoke_stays_message_text() {
    let text = "这里仅说明 call<invoke name=...> 会泄漏成文本，没有真实工具调用。";
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_descriptive_invoke_only",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [{ "type": "text", "text": text }],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "解释问题",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "message");
    assert_eq!(converted["output"][0]["content"][0]["text"], text);
    assert!(
        !converted["output"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call")
    );
}

#[test]
fn anthropic_textual_invoke_ignores_descriptive_invoke_text_before_real_call() {
    let command = r#"cd E:\code\junes\github\CodexElves; rg -n "invoke|textual_invoke|call_prefixed|<invoke|antml_tool_call|parse_textual|extract_tool" crates/codex-elves-core/src/protocol_proxy.rs | Select-Object -First 40"#;
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_descriptive_invoke_then_real_call",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": format!(
                        "这里是协议转换 bug：工具调用被当成文本处理了（call<invoke name=...> 泄漏成文本）。\n\n先看现有逻辑。\n\ncall\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">{command}</parameter>\n</invoke>"
                    )
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "定位协议转换",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": { "cmd": { "type": "string" } }
                    }
                }
            ]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output[0]["type"], "message");
    assert!(
        output[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("call<invoke name=...>")
    );
    let call = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("应该还原出后续真实 exec_command 调用");
    assert_eq!(call["name"], "exec_command");
    assert_eq!(call["arguments"], json!({ "cmd": command }).to_string());
}

#[test]
fn anthropic_textual_invoke_skips_multiple_bad_invoke_fragments_before_real_call() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_multiple_bad_invoke_then_real",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "示例一 <invoke name=...>。\n示例二 <invoke>\n示例三 <invoke name=\"\">\n\ncall\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "定位问题",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output[0]["type"], "message");
    assert!(
        output[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("示例三")
    );
    let call = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("应该跳过多个坏片段后还原真实调用");
    assert_eq!(call["name"], "exec_command");
    assert_eq!(call["arguments"], r#"{"cmd":"git status --short"}"#);
}

#[test]
fn anthropic_textual_invoke_converts_multiple_real_calls_in_order() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_multiple_real_calls",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</invoke>\n<invoke name=\"apply_patch_delete_file\">\n<parameter name=\"path\">temp/old.txt</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "连续工具调用",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                },
                { "type": "custom", "name": "apply_patch" }
            ]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0]["type"], "function_call");
    assert_eq!(output[0]["name"], "exec_command");
    assert_eq!(output[0]["arguments"], r#"{"cmd":"git status --short"}"#);
    assert_eq!(output[1]["type"], "custom_tool_call");
    assert_eq!(output[1]["name"], "apply_patch");
    assert_eq!(
        output[1]["input"],
        "*** Begin Patch\n*** Delete File: temp/old.txt\n*** End Patch"
    );
}

#[test]
fn anthropic_textual_invoke_unescapes_parameter_text_with_nested_invoke_literal() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_xml_escaped_nested_invoke_literal",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">printf '&lt;invoke name=&quot;noop&quot;&gt;&lt;/invoke&gt; &amp; done'</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "执行带 XML 字面量的命令",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["name"], "exec_command");
    assert_eq!(
        converted["output"][0]["arguments"],
        json!({ "cmd": "printf '<invoke name=\"noop\"></invoke> & done'" }).to_string()
    );
}

#[test]
fn anthropic_textual_invoke_apply_patch_batch_preserves_structured_operations() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_patch_batch_proxy",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": r#"call
<invoke name="apply_patch_batch">
<parameter name="operations">[{"type":"add_file","path":"temp/new.txt","content":"hello"},{"type":"delete_file","path":"temp/old.txt"}]</parameter>
</invoke>"#
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "批量 patch",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "apply_patch");
    assert_eq!(
        converted["output"][0]["input"],
        "*** Begin Patch\n*** Add File: temp/new.txt\n+hello\n*** Delete File: temp/old.txt\n*** End Patch"
    );
}

#[test]
fn anthropic_textual_invoke_apply_patch_proxy_preserves_update_hunks() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_textual_patch_tool",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": r#"call
<invoke name="apply_patch_update_file">
<parameter name="path">crates/codex-elves-core/tests/tmp_real_config_sync.rs</parameter>
<parameter name="hunks">[{"context":"fn tmp_real_config_sync_only_touches_mcp() {","lines":[{"op":"context","text":"let before = original.clone();"},{"op":"add","text":"assert_eq!(before, after);"}]}]</parameter>
</invoke>"#
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "更新测试",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "apply_patch");
    assert_eq!(
        converted["output"][0]["input"],
        "*** Begin Patch\n*** Update File: crates/codex-elves-core/tests/tmp_real_config_sync.rs\n@@ fn tmp_real_config_sync_only_touches_mcp() {\n let before = original.clone();\n+assert_eq!(before, after);\n*** End Patch"
    );
}

#[test]
fn anthropic_leading_text_then_textual_invoke_splits_message_and_tool_call() {
    // 回归：同一个 text 块里先是正文，末尾才是 call/<invoke> 工具调用。
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_lead_then_invoke",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                {
                    "type": "text",
                    "text": "代码正确。跟 release build。\n\ncall\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">cargo build --release</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "编译",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": { "cmd": { "type": "string" } }
                    }
                }
            ]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    // 第一项是前导正文 message。
    assert_eq!(output[0]["type"], "message");
    assert_eq!(
        output[0]["content"][0]["text"],
        "代码正确。跟 release build。"
    );
    // 紧跟着工具调用。
    let call = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("应该还原出 function_call");
    assert_eq!(call["name"], "exec_command");
    assert_eq!(call["arguments"], r#"{"cmd":"cargo build --release"}"#);
}
#[test]
fn responses_request_preserves_file_audio_and_unknown_content_parts() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "inspect these" },
                    { "type": "input_file", "file_id": "file_doc", "filename": "doc.pdf" },
                    { "type": "input_audio", "data": "UklGRg==", "format": "wav" },
                    { "type": "unknown_part", "payload": { "a": 1 } }
                ]
            }
        ]
    }))
    .unwrap();

    let content = converted["messages"][0]["content"].as_array().unwrap();
    assert_eq!(
        content[0],
        json!({ "type": "text", "text": "inspect these" })
    );
    assert_eq!(
        content[1],
        json!({ "type": "file", "file": { "file_id": "file_doc", "filename": "doc.pdf" } })
    );
    assert_eq!(
        content[2],
        json!({ "type": "input_audio", "input_audio": { "data": "UklGRg==", "format": "wav" } })
    );
    assert_eq!(content[3]["type"], "text");
    assert!(
        content[3]["text"]
            .as_str()
            .unwrap()
            .contains("unknown_part")
    );
}

#[test]
fn responses_request_matches_ccs_reasoning_and_tool_choice_edges() {
    let non_reasoning = responses_to_chat_completions(json!({
        "model": "gpt-4o",
        "reasoning": { "effort": "high" },
        "tool_choice": { "type": "required" },
        "input": "hi"
    }))
    .unwrap();
    assert!(non_reasoning.get("reasoning_effort").is_none());
    assert!(non_reasoning.get("tool_choice").is_none());

    let reasoning = responses_to_chat_completions(json!({
        "model": "gpt-5.4",
        "reasoning": { "effort": "high" },
        "tool_choice": { "type": "function", "name": "lookup" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(reasoning["reasoning_effort"], "high");
    assert!(reasoning.get("tool_choice").is_none());

    let minimal = responses_to_chat_completions(json!({
        "model": "gpt-5.4",
        "reasoning": { "effort": "minimal" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(minimal["reasoning_effort"], "minimal");
}

#[test]
fn proxy_route_matchers_accept_ccswitch_codex_aliases() {
    for path in [
        "/responses",
        "/v1/responses",
        "/v1/v1/responses",
        "/codex/v1/responses",
        "/responses/compact",
        "/v1/responses/compact",
        "/v1/v1/responses/compact",
        "/codex/v1/responses/compact",
    ] {
        assert!(is_responses_proxy_path(path), "{path}");
    }

    for path in [
        "/chat/completions",
        "/v1/chat/completions",
        "/v1/v1/chat/completions",
        "/codex/v1/chat/completions",
    ] {
        assert!(is_chat_completions_proxy_path(path), "{path}");
    }

    for path in ["/models", "/v1/models", "/v1/v1/models", "/codex/v1/models"] {
        assert!(is_models_proxy_path(path), "{path}");
    }
}

#[test]
fn responses_request_applies_ccswitch_reasoning_dialects() {
    let deepseek = responses_to_chat_completions(json!({
        "model": "deepseek-reasoner",
        "reasoning": { "effort": "xhigh" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek["reasoning_effort"], "max");

    let openrouter = responses_to_chat_completions(json!({
        "model": "openrouter/deepseek/deepseek-r1",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(openrouter["reasoning"]["effort"], "xhigh");
    assert!(openrouter.get("reasoning_effort").is_none());

    let openrouter_off = responses_to_chat_completions(json!({
        "model": "openrouter/deepseek/deepseek-r1",
        "reasoning": { "effort": "none" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(openrouter_off["reasoning"]["effort"], "none");

    let kimi = responses_to_chat_completions(json!({
        "model": "kimi-k2-thinking",
        "reasoning": { "effort": "high" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(kimi["thinking"]["type"], "enabled");
    assert!(kimi.get("reasoning_effort").is_none());
}

#[test]
fn glm_chat_reasoning_keeps_enabled_flag_and_effort() {
    let converted = responses_to_chat_completions(json!({
        "model": "glm-5.2",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();

    assert_eq!(converted["thinking"], json!({ "type": "enabled" }));
    assert_eq!(converted["reasoning_effort"], "max");
}

#[test]
fn non_claude_anthropic_compatible_reasoning_keeps_effort_by_model_capability() {
    let glm = responses_to_anthropic_messages(json!({
        "model": "glm-5.2",
        "reasoning": { "effort": "xhigh" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(glm["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(glm["output_config"], json!({ "effort": "high" }));

    let deepseek = responses_to_anthropic_messages(json!({
        "model": "deepseek-reasoner",
        "model_reasoning_effort": "max",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek["thinking"], json!({ "type": "enabled" }));
    assert_eq!(deepseek["output_config"], json!({ "effort": "max" }));

    let deepseek_xhigh = responses_to_anthropic_messages(json!({
        "model": "deepseek-reasoner",
        "model_reasoning_effort": "xhigh",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek_xhigh["thinking"], json!({ "type": "enabled" }));
    assert_eq!(deepseek_xhigh["output_config"], json!({ "effort": "max" }));

    let deepseek_default = responses_to_anthropic_messages(json!({
        "model": "deepseek-v4-pro",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek_default["thinking"], json!({ "type": "enabled" }));
    assert_eq!(
        deepseek_default["output_config"],
        json!({ "effort": "max" })
    );
}

#[test]
fn deepseek_reasoning_efforts_match_official_levels() {
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "deepseek-reasoner",
            UpstreamResponseProtocol::ChatCompletions,
        ),
        vec!["high", "max"]
    );
}

#[test]
fn glm_reasoning_efforts_match_supported_levels() {
    assert_eq!(
        supported_reasoning_efforts_for_model("glm-4.6", UpstreamResponseProtocol::ChatCompletions,),
        vec!["high", "max"]
    );
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "zhipu/glm-4.6",
            UpstreamResponseProtocol::ChatCompletions,
        ),
        vec!["high", "max"]
    );
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "z.ai/glm-4.6",
            UpstreamResponseProtocol::ChatCompletions
        ),
        vec!["high", "max"]
    );
}

#[test]
fn sonnet5_reasoning_efforts_include_max() {
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "claude-sonnet-5",
            UpstreamResponseProtocol::Anthropic,
        ),
        vec!["low", "medium", "high", "xhigh", "max"]
    );
    // 带后缀的变体也应命中 sonnet-5 分支
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "claude-sonnet-5-20250929",
            UpstreamResponseProtocol::Anthropic,
        ),
        vec!["low", "medium", "high", "xhigh", "max"]
    );
}

#[test]
fn responses_request_maps_developer_role_to_system_for_chat_upstream() {
    let converted = responses_to_chat_completions(json!({
        "model": "deepseek-chat",
        "input": [
            {
                "type": "message",
                "role": "developer",
                "content": [
                    { "type": "input_text", "text": "developer instructions" }
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "hello" }
                ]
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "system");
    assert_eq!(
        converted["messages"][0]["content"],
        "developer instructions"
    );
    assert_eq!(converted["messages"][1]["role"], "user");
    assert!(
        !serde_json::to_string(&converted)
            .unwrap()
            .contains("\"developer\"")
    );
}

#[test]
fn responses_request_collapses_system_messages_to_head_for_strict_chat_upstreams() {
    let converted = responses_to_chat_completions(json!({
        "model": "MiniMax-M2.7",
        "instructions": "root system",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            },
            {
                "type": "message",
                "role": "developer",
                "content": [{ "type": "input_text", "text": "late developer" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "ok" }]
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "system");
    assert_eq!(
        converted["messages"][0]["content"],
        "root system\n\nlate developer"
    );
    let system_count = converted["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .count();
    assert_eq!(system_count, 1);
    assert_eq!(converted["messages"][1]["role"], "user");
    assert_eq!(converted["messages"][2]["role"], "assistant");
}

#[test]
fn responses_request_maps_latest_reminder_to_user_like_ccswitch() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "message",
                "role": "latest_reminder",
                "content": [
                    { "type": "input_text", "text": "remember this" }
                ]
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "user");
    assert_eq!(converted["messages"][0]["content"], "remember this");
}

#[test]
fn responses_request_preserves_reasoning_content_for_thinking_followup() {
    let converted = responses_to_chat_completions(json!({
        "model": "deepseek-reasoner",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "use the tool" }]
            },
            {
                "id": "rs_1",
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": "Need to inspect files." }]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "shell",
                "arguments": "{\"cmd\":\"rg foo\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "result"
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][1]["role"], "assistant");
    assert_eq!(
        converted["messages"][1]["reasoning_content"],
        "Need to inspect files."
    );
    assert_eq!(converted["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(converted["messages"][2]["role"], "tool");
}

#[test]
fn anthropic_request_preserves_thinking_signature_for_tool_followup() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "use the tool" }]
            },
            {
                "id": "rs_msg_1",
                "type": "reasoning",
                "reasoning_content": "Need to inspect files.",
                "encrypted_content": "sig_123"
            },
            {
                "type": "function_call",
                "call_id": "toolu_123",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"rg foo\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_123",
                "output": "result"
            }
        ]
    }))
    .unwrap();

    let messages = converted["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "thinking");
    assert_eq!(
        messages[1]["content"][0]["thinking"],
        "Need to inspect files."
    );
    assert_eq!(messages[1]["content"][0]["signature"], "sig_123");
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(converted["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(converted["output_config"], json!({ "effort": "high" }));
}

#[test]
fn anthropic_tool_followup_without_signed_reasoning_preserves_requested_thinking() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "reasoning": { "effort": "xhigh" },
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "use the tool" }]
            },
            {
                "type": "function_call",
                "call_id": "toolu_123",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"rg foo\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_123",
                "output": "result"
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(converted["output_config"], json!({ "effort": "xhigh" }));
    assert_eq!(converted["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(
        converted["messages"][2]["content"][0]["type"],
        "tool_result"
    );
}

#[test]
fn responses_request_merges_reasoning_text_and_tool_calls_like_ccx() {
    let converted = responses_to_chat_completions(json!({
        "model": "deepseek-v4-pro",
        "input": [
            {
                "type": "reasoning",
                "status": "completed",
                "summary": [{ "type": "summary_text", "text": "I need to run go vet." }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "Let me run go vet." }]
            },
            {
                "type": "function_call",
                "call_id": "call_001",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"go vet ./...\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_001",
                "output": "no issues found"
            },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "run tests now" }]
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "assistant");
    assert_eq!(converted["messages"][0]["content"], "Let me run go vet.");
    assert_eq!(
        converted["messages"][0]["reasoning_content"],
        "I need to run go vet."
    );
    assert_eq!(converted["messages"][0]["tool_calls"][0]["id"], "call_001");
    assert_eq!(converted["messages"][1]["role"], "tool");
    assert_eq!(converted["messages"][1]["tool_call_id"], "call_001");
    assert_eq!(converted["messages"][2]["role"], "user");
}

#[test]
fn responses_request_normalizes_empty_assistant_messages_for_chat_upstream() {
    let converted = responses_to_chat_completions(json!({
        "model": "deepseek-chat",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": null
            },
            {
                "type": "message",
                "role": "assistant",
                "content": []
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "assistant");
    assert_eq!(converted["messages"][0]["content"], "");
    assert_eq!(converted["messages"][1]["role"], "assistant");
    assert_eq!(converted["messages"][1]["content"], "");
}

#[test]
fn responses_input_sanitizes_invalid_function_call_arguments_history() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "function_call",
                "call_id": "bad_object",
                "name": "broken_args",
                "arguments": "{foo: \"bar\"}"
            },
            {
                "type": "function_call",
                "call_id": "plain_text",
                "name": "plain_args",
                "arguments": "raw text with \"quotes\" and \\slashes"
            },
            {
                "type": "function_call",
                "call_id": "array_args",
                "name": "array_args",
                "arguments": "[1,2,3]"
            },
            {
                "type": "tool_call",
                "tool_use": {
                    "id": "object_args",
                    "name": "object_args",
                    "input": { "ok": true }
                }
            }
        ]
    }))
    .unwrap();

    let calls = converted["messages"][0]["tool_calls"].as_array().unwrap();
    for call in calls {
        let arguments = call["function"]["arguments"].as_str().unwrap();
        serde_json::from_str::<serde_json::Value>(arguments)
            .expect("chat tool call arguments must always be valid JSON");
    }
    assert_eq!(
        calls[0]["function"]["arguments"],
        "{\"input\":\"{foo: \\\"bar\\\"}\"}"
    );
    assert_eq!(
        calls[1]["function"]["arguments"],
        "{\"input\":\"raw text with \\\"quotes\\\" and \\\\slashes\"}"
    );
    assert_eq!(calls[2]["function"]["arguments"], "{\"input\":[1,2,3]}");
    assert_eq!(calls[3]["function"]["arguments"], "{\"ok\":true}");
}

#[test]
fn responses_request_drops_tool_controls_when_no_chat_tools_survive() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "tools": [
            { "type": "unknown_builtin", "name": "unsupported" }
        ],
        "tool_choice": { "type": "required" },
        "parallel_tool_calls": true
    }))
    .unwrap();

    assert!(converted.get("tools").is_none());
    assert!(converted.get("tool_choice").is_none());
    assert!(converted.get("parallel_tool_calls").is_none());
}

#[test]
fn responses_request_normalizes_function_tool_parameters() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "tools": [
            {
                "type": "function",
                "name": "lookup",
                "parameters": {}
            }
        ]
    }))
    .unwrap();

    let params = &converted["tools"][0]["function"]["parameters"];
    assert_eq!(params["type"], "object");
    assert_eq!(params["properties"], json!({}));
    assert_eq!(params["required"], json!([]));
}

#[test]
fn responses_request_maps_codex_custom_and_namespace_tools_to_chat_functions() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "tools": [
            {
                "type": "custom",
                "name": "exec",
                "description": "Run a command"
            },
            {
                "type": "namespace",
                "name": "mcp__vscode_mcp__",
                "description": "VS Code MCP",
                "tools": [
                    {
                        "type": "function",
                        "name": "open_file",
                        "description": "Open a file",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            },
                            "required": ["path"]
                        }
                    }
                ]
            },
            {
                "type": "web_search"
            },
            {
                "type": "tool_search",
                "description": "Discover deferred tools"
            },
            {
                "type": "web_search_preview"
            },
            {
                "type": "web_search_preview_2025_03_11"
            },
            {
                "type": "local_shell"
            },
            tavily_namespace_tool()
        ],
        "tool_choice": {
            "type": "function",
            "namespace": "mcp__vscode_mcp__",
            "name": "open_file"
        },
        "parallel_tool_calls": true
    }))
    .unwrap();

    let names: Vec<_> = converted["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"exec"));
    assert!(names.contains(&"mcp__vscode_mcp__open_file"));
    assert!(names.contains(&"local_shell"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"tool_search"));
    assert!(names.contains(&"web_search_preview"));
    assert!(names.contains(&"web_search_preview_2025_03_11"));
    assert_eq!(
        converted["tools"][0]["function"]["parameters"]["properties"]["input"]["type"],
        "string"
    );
    assert_eq!(converted["parallel_tool_calls"], true);
    assert_eq!(
        converted["tool_choice"]["function"]["name"],
        "mcp__vscode_mcp__open_file"
    );
}

#[test]
fn responses_request_maps_tool_choice_for_proxy_internal_tools() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "tools": [
            { "type": "web_search_preview_2025_03_11" },
            { "type": "local_shell" },
            tavily_namespace_tool()
        ],
        "tool_choice": { "type": "function", "name": "web_search_preview_2025_03_11" }
    }))
    .unwrap();

    let names: Vec<_> = converted["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"web_search_preview_2025_03_11"));
    assert!(names.contains(&"local_shell"));
    assert!(names.contains(&"mcp__tavily__tavily_search"));
    assert_eq!(
        converted["tool_choice"]["function"]["name"],
        "web_search_preview_2025_03_11"
    );

    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-4",
        "input": "hi",
        "tools": [
            { "type": "web_search_preview" },
            { "type": "computer_use_preview" }
        ],
        "tool_choice": { "type": "web_search_preview" }
    }))
    .unwrap();

    // 无 MCP fallback 时，web_search_preview 声明为 Anthropic 原生 server-side 工具（追加到末尾）；
    // 其他工具保留；tool_choice 强制 web_search 被降级为 auto。
    let anthropic_tools = anthropic["tools"].as_array().unwrap();
    assert!(
        anthropic_tools
            .iter()
            .any(|tool| tool["type"] == "web_search_20250305" && tool["name"] == "web_search")
    );
    assert!(
        anthropic_tools
            .iter()
            .any(|tool| tool["name"] == "computer_use_preview")
    );
    assert_eq!(anthropic["tool_choice"], json!({ "type": "auto" }));
}

#[test]
fn responses_request_stream_includes_usage_and_apply_patch_proxy_tools() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "stream": true,
        "tools": [
            {
                "type": "custom",
                "name": "apply_patch",
                "description": "Patch files"
            }
        ],
        "tool_choice": { "type": "custom", "name": "apply_patch" }
    }))
    .unwrap();

    assert_eq!(converted["stream_options"]["include_usage"], true);
    let names: Vec<_> = converted["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "apply_patch_add_file",
            "apply_patch_delete_file",
            "apply_patch_update_file",
            "apply_patch_replace_file",
            "apply_patch_batch"
        ]
    );
    assert_eq!(
        converted["tools"][2]["function"]["parameters"]["properties"]["hunks"]["items"]["properties"]
            ["lines"]["items"]["required"],
        json!(["op", "text"])
    );
    assert_eq!(
        converted["tool_choice"]["function"]["name"],
        "apply_patch_batch"
    );
}

#[test]
fn responses_input_replays_custom_and_legacy_tool_history() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "custom_tool_call",
                "call_id": "call_custom",
                "name": "exec",
                "input": "ls -la"
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call_custom",
                "output": "ok"
            },
            {
                "type": "tool_call",
                "tool_use": {
                    "id": "call_legacy",
                    "name": "lookup",
                    "input": { "query": "rust" }
                }
            },
            {
                "type": "tool_result",
                "content": {
                    "tool_use_id": "call_legacy",
                    "content": { "result": "found" }
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "assistant");
    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["id"],
        "call_custom"
    );
    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["name"],
        "exec"
    );
    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["arguments"],
        "{\"input\":\"ls -la\"}"
    );
    assert_eq!(converted["messages"][1]["role"], "tool");
    assert_eq!(converted["messages"][1]["content"], "ok");
    assert_eq!(
        converted["messages"][2]["tool_calls"][0]["id"],
        "call_legacy"
    );
    assert_eq!(
        converted["messages"][3]["content"],
        "{\"result\":\"found\"}"
    );
}

#[test]
fn responses_input_replays_server_side_tool_history() {
    let input = json!([
        {
            "type": "message",
            "role": "user",
            "content": "start"
        },
        {
            "type": "tool_search_call",
            "call_id": "call_tool_search",
            "status": "completed",
            "execution": "client",
            "arguments": {
                "query": "pal mcp",
                "limit": 8
            }
        },
        {
            "type": "tool_search_output",
            "call_id": "call_tool_search",
            "status": "completed",
            "execution": "client",
            "tools": [
                {
                    "type": "namespace",
                    "name": "mcp__pal",
                    "tools": []
                }
            ]
        },
        {
            "type": "function_call",
            "call_id": "call_web",
            "name": "web_search_preview",
            "arguments": "{\"query\":\"rust\"}"
        },
        {
            "type": "function_call_output",
            "call_id": "call_web",
            "output": "unsupported call: web_search_preview"
        },
        {
            "type": "message",
            "role": "user",
            "content": "continue"
        }
    ]);

    let chat = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": input
    }))
    .unwrap();
    let chat_text = chat["messages"].to_string();
    assert!(chat_text.contains("tool_search"));
    assert!(chat_text.contains("pal mcp"));
    assert!(chat_text.contains("mcp__pal"));
    assert!(chat_text.contains("web_search_preview"));
    assert!(chat_text.contains("unsupported"));
    assert!(
        chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == "continue")
    );

    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-4",
        "input": input
    }))
    .unwrap();
    let anthropic_text = anthropic["messages"].to_string();
    assert!(anthropic_text.contains("tool_search"));
    assert!(anthropic_text.contains("pal mcp"));
    assert!(anthropic_text.contains("mcp__pal"));
    assert!(anthropic_text.contains("web_search_preview"));
    assert!(anthropic_text.contains("unsupported"));
    assert!(anthropic_text.contains("continue"));
}

#[test]
fn anthropic_tool_result_history_merges_following_user_text_into_same_turn() {
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "start"
            },
            {
                "type": "function_call",
                "call_id": "toolu_01GkD6H6YEdCrW3sAhhCcA3m",
                "name": "update_plan",
                "arguments": "{\"plan\":[]}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_01GkD6H6YEdCrW3sAhhCcA3m",
                "output": "ok"
            },
            {
                "type": "message",
                "role": "developer",
                "content": "Keep replies concise."
            },
            {
                "type": "message",
                "role": "developer",
                "content": "Use default mode."
            },
            {
                "type": "message",
                "role": "user",
                "content": "continue"
            },
            {
                "type": "message",
                "role": "user",
                "content": "next"
            }
        ]
    }))
    .unwrap();

    assert_eq!(
        anthropic["system"],
        "Keep replies concise.\n\nUse default mode."
    );
    let messages = anthropic["messages"].as_array().unwrap();
    // 首条是真实 user（start），tool_use 不在开头，不被 drop-leading 处理。
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "start");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(
        messages[2]["content"],
        json!([
            {
                "type": "tool_result",
                "tool_use_id": "toolu_01GkD6H6YEdCrW3sAhhCcA3m",
                "content": "ok"
            },
            {
                "type": "text",
                "text": "continue"
            },
            {
                "type": "text",
                "text": "next"
            }
        ])
    );
}

#[test]
fn anthropic_parallel_tool_results_can_share_one_user_turn() {
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "start"
            },
            {
                "type": "function_call",
                "call_id": "call_one",
                "name": "lookup",
                "arguments": "{\"query\":\"one\"}"
            },
            {
                "type": "function_call",
                "call_id": "call_two",
                "name": "lookup",
                "arguments": "{\"query\":\"two\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_one",
                "output": "one"
            },
            {
                "type": "function_call_output",
                "call_id": "call_two",
                "output": "two"
            },
            {
                "type": "message",
                "role": "user",
                "content": "next"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    // 首条是真实 user（start）。
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "start");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "call_one");
    assert_eq!(messages[2]["content"][1]["type"], "tool_result");
    assert_eq!(messages[2]["content"][1]["tool_use_id"], "call_two");
    assert_eq!(messages[2]["content"][2]["type"], "text");
    assert_eq!(messages[2]["content"][2]["text"], "next");
}

#[test]
fn anthropic_does_not_merge_tool_result_after_plain_user_text() {
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "before"
            },
            {
                "type": "function_call",
                "call_id": "call_lookup",
                "name": "lookup",
                "arguments": "{\"query\":\"one\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_lookup",
                "output": "one"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "before");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
}

#[test]
fn anthropic_history_starting_with_tool_use_drops_orphan_pair_and_keeps_following_user() {
    // 真实压缩续写形态：开头是闭合的 function_call + function_call_output 对，
    // 后跟真实 user/developer。开头的 update_plan 工具对是悬空上下文（发起它的轮次已被截断），
    // 应丢弃这对 tool_use/tool_result，让首条回到真实 user，而不是补占位。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "function_call",
                "call_id": "toolu_lead",
                "name": "update_plan",
                "arguments": "{\"plan\":[]}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_lead",
                "output": "Plan updated"
            },
            {
                "type": "message",
                "role": "user",
                "content": "continue the task"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    // 首条是真实 user，不再出现 tool_use/tool_result。
    assert_eq!(messages[0]["role"], "user");
    let serialized = anthropic["messages"].to_string();
    assert!(!serialized.contains("tool_use"));
    assert!(!serialized.contains("tool_result"));
    assert!(serialized.contains("continue the task"));
    // 不应出现占位文本（因为有真实 user 兑底）。
    assert!(!serialized.contains("continuing the previous conversation"));
}

#[test]
fn anthropic_history_starting_with_tool_use_then_merged_user_strips_orphan_tool_result_only() {
    // 关键风险：tool_result 后续的普通 user 文本会被合并进同一条 user。
    // 丢弃开头 assistant[tool_use] 后，只能精准删除那条 user 里的悬空 tool_result，
    // 保留合并进来的 text（如 <turn_aborted> 和真实续写意图）。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "function_call",
                "call_id": "toolu_lead",
                "name": "update_plan",
                "arguments": "{\"plan\":[]}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_lead",
                "output": "Plan updated"
            },
            {
                "type": "message",
                "role": "user",
                "content": "<turn_aborted>"
            },
            {
                "type": "message",
                "role": "user",
                "content": "real follow up"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    let content = messages[0]["content"].as_array().unwrap();
    // 悬空 tool_result 被删，两段 text 保留。
    assert!(content.iter().all(|b| b["type"] == "text"));
    let serialized = messages[0]["content"].to_string();
    assert!(serialized.contains("<turn_aborted>"));
    assert!(serialized.contains("real follow up"));
    assert!(!serialized.contains("tool_result"));
}

#[test]
fn anthropic_history_with_parallel_leading_tool_uses_strips_all_orphans() {
    // 并行工具调用：开头两个 function_call + 两个 function_call_output，后跟 user。
    // 两个 tool_use 都是悬空，应全部丢弃/剔除，首条回到真实 user。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_a",
                "name": "lookup",
                "arguments": "{\"q\":\"a\"}"
            },
            {
                "type": "function_call",
                "call_id": "call_b",
                "name": "lookup",
                "arguments": "{\"q\":\"b\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_a",
                "output": "ra"
            },
            {
                "type": "function_call_output",
                "call_id": "call_b",
                "output": "rb"
            },
            {
                "type": "message",
                "role": "user",
                "content": "next"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "user");
    let serialized = anthropic["messages"].to_string();
    assert!(!serialized.contains("tool_use"));
    assert!(!serialized.contains("tool_result"));
    assert!(serialized.contains("next"));
}

#[test]
fn anthropic_history_all_orphan_tool_use_falls_back_to_placeholder_user() {
    // 退化场景：整段历史只有悬空的 tool_use/tool_result，丢完后为空，才补占位 user。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "function_call",
                "call_id": "toolu_only",
                "name": "update_plan",
                "arguments": "{\"plan\":[]}"
            },
            {
                "type": "function_call_output",
                "call_id": "toolu_only",
                "output": "Plan updated"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(
        messages[0]["content"][0]["text"],
        "(continuing the previous conversation)"
    );
}

#[test]
fn anthropic_history_starting_with_assistant_text_drops_until_user() {
    // 开头是 assistant 纯文本（无 tool_use）+ 后跟 user：直接丢弃 assistant 头，首条回到 user。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": "leftover assistant text"
            },
            {
                "type": "message",
                "role": "user",
                "content": "actual question"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "actual question");
}

#[test]
fn anthropic_history_starting_with_user_is_unchanged() {
    // 以 user 开头的正常历史不应被动，不插入多余的前导 user。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "hello"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "hello");
}

#[test]
fn anthropic_orphan_tool_outputs_are_downgraded_to_user_text() {
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "before"
            },
            {
                "type": "function_call_output",
                "call_id": "missing_call",
                "output": "orphan"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["type"], "text");
    assert_eq!(messages[0]["content"][0]["text"], "before");
    assert_eq!(messages[0]["content"][1]["type"], "text");
    assert_eq!(
        messages[0]["content"][1]["text"],
        "Function call output (missing_call): orphan"
    );
}

#[test]
fn anthropic_history_starting_with_orphan_tool_output_is_downgraded() {
    // 压缩续写可能裁掉 function_call，只留下开头的 function_call_output。
    // 此时首条不是 assistant（改动1 不触发），必须靠改动3 降级为普通文本，
    // 否则会产出裸 tool_result 被上游拒绝。
    let anthropic = responses_to_anthropic_messages(json!({
        "model": "claude-opus-4-8",
        "input": [
            {
                "type": "function_call_output",
                "call_id": "truncated_call",
                "output": "done"
            },
            {
                "type": "message",
                "role": "user",
                "content": "continue"
            }
        ]
    }))
    .unwrap();

    let messages = anthropic["messages"].as_array().unwrap();
    // 首条为 user，且全部为 text，不出现任何 tool_result。
    assert_eq!(messages[0]["role"], "user");
    let serialized = anthropic["messages"].to_string();
    assert!(!serialized.contains("tool_result"));
    assert!(serialized.contains("Function call output (truncated_call): done"));
}

#[test]
fn tool_search_output_tools_are_exposed_to_chat_upstream_and_response_context() {
    let request = json!({
        "model": "deepseek-v4-pro",
        "input": [
            {
                "type": "tool_search_call",
                "call_id": "call_tool_search",
                "status": "completed",
                "execution": "client",
                "arguments": {
                    "query": "pal consensus"
                }
            },
            {
                "type": "tool_search_output",
                "call_id": "call_tool_search",
                "status": "completed",
                "execution": "client",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__pal",
                    "description": "PAL MCP",
                    "tools": [{
                        "type": "function",
                        "name": "consensus",
                        "description": "Build multi-model consensus.",
                        "defer_loading": true,
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "step": { "type": "string" }
                            },
                            "required": ["step"],
                            "additionalProperties": false
                        }
                    }]
                }]
            },
            {
                "type": "message",
                "role": "user",
                "content": "use pal"
            }
        ],
        "tools": [{ "type": "tool_search" }]
    });

    let chat = responses_to_chat_completions(request.clone()).unwrap();
    let names: Vec<_> = chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"tool_search"));
    assert!(names.contains(&"mcp__pal__consensus"));

    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_pal_consensus",
            "created": 123,
            "model": "deepseek-v4-pro",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_pal",
                        "type": "function",
                        "function": {
                            "name": "mcp__pal__consensus",
                            "arguments": "{\"step\":\"discuss\"}"
                        }
                    }]
                }
            }]
        }),
        &request,
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["call_id"], "call_pal");
    assert_eq!(converted["output"][0]["name"], "consensus");
    assert_eq!(converted["output"][0]["namespace"], "mcp__pal");
}

#[test]
fn tool_search_output_tools_are_exposed_to_anthropic_upstream_and_response_context() {
    let request = json!({
        "model": "claude-sonnet-4-6",
        "input": [
            {
                "type": "tool_search_output",
                "call_id": "call_tool_search",
                "status": "completed",
                "execution": "client",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__pal",
                    "description": "PAL MCP",
                    "tools": [{
                        "type": "function",
                        "name": "consensus",
                        "description": "Build multi-model consensus.",
                        "defer_loading": true,
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "step": { "type": "string" }
                            },
                            "required": ["step"],
                            "additionalProperties": false
                        }
                    }]
                }]
            },
            {
                "type": "message",
                "role": "user",
                "content": "use pal"
            }
        ],
        "tools": [{ "type": "tool_search" }]
    });

    let anthropic = responses_to_anthropic_messages(request.clone()).unwrap();
    let names: Vec<_> = anthropic["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"tool_search"));
    assert!(names.contains(&"mcp__pal__consensus"));

    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_pal_consensus",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-6",
            "content": [{
                "type": "tool_use",
                "id": "toolu_pal",
                "name": "mcp__pal__consensus",
                "input": {
                    "step": "discuss"
                }
            }],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        }),
        &request,
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["call_id"], "toolu_pal");
    assert_eq!(converted["output"][0]["name"], "consensus");
    assert_eq!(converted["output"][0]["namespace"], "mcp__pal");
}

#[test]
fn responses_input_flattens_namespace_function_history_and_skips_invalid_tool_items() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_ns",
                "namespace": "mcp__vscode_mcp__",
                "name": "execute_command",
                "arguments": "{\"command\":\"save\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_ns",
                "output": "saved"
            },
            {
                "type": "function_call",
                "call_id": "missing_name",
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "output": "orphan"
            }
        ]
    }))
    .unwrap();

    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["name"],
        "mcp__vscode_mcp__execute_command"
    );
    assert_eq!(converted["messages"][1]["tool_call_id"], "call_ns");
    assert_eq!(converted["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn responses_input_downgrades_orphan_tool_outputs_to_user_messages() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "reasoning",
                "summary": [{ "type": "summary_text", "text": "I need the previous tool result." }]
            },
            {
                "type": "function_call_output",
                "call_id": "missing_call",
                "output": "tool output without a matching call"
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "missing_custom",
                "output": "custom output without a matching call"
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][0]["role"], "assistant");
    assert!(converted["messages"][0].get("tool_calls").is_none());
    assert_eq!(converted["messages"][1]["role"], "user");
    assert_eq!(
        converted["messages"][1]["content"],
        "Function call output (missing_call): tool output without a matching call"
    );
    assert_eq!(converted["messages"][2]["role"], "user");
    assert_eq!(
        converted["messages"][2]["content"],
        "Function call output (missing_custom): custom output without a matching call"
    );
}

#[test]
fn responses_input_replays_apply_patch_custom_history_as_proxy_tool() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [
            {
                "type": "custom_tool_call",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** Add File: docs/test.md\n+# Test\n*** End Patch"
            }
        ],
        "tools": [{ "type": "custom", "name": "apply_patch" }]
    }))
    .unwrap();

    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["name"],
        "apply_patch_add_file"
    );
    assert_eq!(
        converted["messages"][0]["tool_calls"][0]["function"]["arguments"],
        "{\"content\":\"# Test\",\"path\":\"docs/test.md\"}"
    );
}

#[test]
fn upstream_chat_error_is_regularized_as_responses_error_envelope() {
    let json_error = responses_error_from_upstream(
        400,
        "application/json",
        br#"{"error":{"message":"bad request","type":"invalid_request_error","code":"bad_model","param":"model"}}"#,
    );
    assert_eq!(json_error["error"]["message"], "bad request");
    assert_eq!(json_error["error"]["type"], "invalid_request_error");
    assert_eq!(json_error["error"]["code"], "bad_model");
    assert_eq!(json_error["error"]["param"], "model");

    let text_error = responses_error_from_upstream(502, "text/html", b"<html>bad gateway</html>");
    assert_eq!(text_error["error"]["message"], "<html>bad gateway</html>");
    assert_eq!(text_error["error"]["type"], "upstream_error");
    assert_eq!(text_error["error"]["code"], "502");
}

#[test]
fn chat_completion_response_converts_to_responses_response() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_123",
        "created": 1710000000,
        "model": "gpt-5-mini",
        "choices": [
            {
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "hi there"
                }
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    }))
    .unwrap();

    assert_eq!(converted["object"], "response");
    assert_eq!(converted["status"], "completed");
    assert_eq!(converted["model"], "gpt-5-mini");
    assert_eq!(converted["usage"]["input_tokens"], 10);
    assert_eq!(converted["usage"]["output_tokens"], 5);
    assert_eq!(converted["output"][0]["type"], "message");
    assert_eq!(converted["output"][0]["content"][0]["text"], "hi there");
}

#[test]
fn chat_completion_response_maps_reasoning_tool_calls_and_usage_details() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_1",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "reasoning_content": "I should check first.",
                "content": "Let me check.",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"city\":\"Tokyo\"}"
                    }
                }]
            }
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "prompt_tokens_details": { "cached_tokens": 3 },
            "completion_tokens_details": { "reasoning_tokens": 2 }
        }
    }))
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "reasoning");
    assert_eq!(
        converted["output"][0]["summary"][0]["text"],
        "I should check first."
    );
    assert_eq!(
        converted["output"][0]["reasoning_content"],
        "I should check first."
    );
    assert_eq!(converted["output"][1]["type"], "message");
    assert_eq!(converted["output"][2]["type"], "function_call");
    assert_eq!(converted["output"][2]["call_id"], "call_1");
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cached_tokens"],
        3
    );
    assert_eq!(
        converted["usage"]["output_tokens_details"]["reasoning_tokens"],
        2
    );
}

#[test]
fn chat_completion_response_maps_web_search_to_native_call() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_web_search",
            "created": 123,
            "model": "gpt-chat",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_web",
                        "type": "function",
                        "function": {
                            "name": "web_search_preview_2025_03_11",
                            "arguments": "{\"query\":\"pal mcp GitHub\"}"
                        }
                    }]
                }
            }]
        }),
        &json!({
            "model": "gpt-chat",
            "input": "search",
            "tools": [{ "type": "web_search_preview_2025_03_11" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "web_search_call");
    assert_eq!(converted["output"][0]["id"], "ws_call_web");
    assert_eq!(converted["output"][0]["status"], "completed");
    assert_eq!(converted["output"][0]["execution"], "client");
    assert_eq!(converted["output"][0]["action"]["type"], "search");
    assert_eq!(converted["output"][0]["action"]["query"], "pal mcp GitHub");
    assert_eq!(
        converted["output"][0]["action"]["queries"],
        json!(["pal mcp GitHub"])
    );
}

#[test]
fn chat_completion_response_maps_web_search_to_search_mcp_when_available() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_web_search",
            "created": 123,
            "model": "gpt-chat",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_web",
                        "type": "function",
                        "function": {
                            "name": "web_search_preview_2025_03_11",
                            "arguments": "{\"query\":\"pal mcp GitHub\"}"
                        }
                    }]
                }
            }]
        }),
        &json!({
            "model": "gpt-chat",
            "input": "search",
            "tools": [
                { "type": "web_search_preview_2025_03_11" },
                tavily_namespace_tool()
            ]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "function_call");
    assert_eq!(converted["output"][0]["call_id"], "call_web");
    assert_eq!(converted["output"][0]["name"], "tavily_search");
    assert_eq!(converted["output"][0]["namespace"], "mcp__tavily");
    assert_eq!(
        converted["output"][0]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[test]
fn chat_completion_response_extracts_reasoning_details_like_ccswitch() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_reasoning_details",
        "created": 123,
        "model": "MiniMax-M2.7",
        "choices": [{
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "reasoning_details": [
                    { "summary": "Step one." },
                    { "parts": [{ "text": "Step two." }] }
                ],
                "content": "final"
            }
        }]
    }))
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "reasoning");
    assert_eq!(
        converted["output"][0]["summary"][0]["text"],
        "Step one.\n\nStep two."
    );
    assert_eq!(converted["output"][1]["content"][0]["text"], "final");
}

#[test]
fn chat_completion_response_accepts_responses_style_usage_fields() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_usage",
        "created": 123,
        "model": "gpt-5.4",
        "choices": [{
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "ok"
            }
        }],
        "usage": {
            "input_tokens": 7,
            "output_tokens": 3,
            "input_tokens_details": { "cached_tokens": 2 },
            "cache_read_input_tokens": 1,
            "cache_creation_input_tokens": 4
        }
    }))
    .unwrap();

    assert_eq!(converted["usage"]["input_tokens"], 7);
    assert_eq!(converted["usage"]["output_tokens"], 3);
    assert_eq!(converted["usage"]["total_tokens"], 15);
    assert!(converted["usage"].get("input_tokens_details").is_none());
    assert_eq!(converted["usage"]["cache_read_input_tokens"], 1);
    assert_eq!(converted["usage"]["cache_creation_input_tokens"], 4);
}

#[test]
fn chat_completion_response_maps_custom_and_namespace_calls_with_request_context() {
    let request = json!({
        "model": "gpt-5-mini",
        "input": "hi",
        "tools": [
            { "type": "custom", "name": "exec" },
            {
                "type": "namespace",
                "name": "mcp__vscode_mcp__",
                "tools": [
                    { "type": "function", "name": "open_file", "parameters": {} }
                ]
            }
        ]
    });
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_tools",
            "created": 123,
            "model": "gpt-5-mini",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_custom",
                            "type": "function",
                            "function": {
                                "name": "exec",
                                "arguments": "{\"input\":\"ls -la\"}"
                            }
                        },
                        {
                            "id": "call_ns",
                            "type": "function",
                            "function": {
                                "name": "mcp__vscode_mcp__open_file",
                                "arguments": "{\"path\":\"src/main.rs\"}"
                            }
                        }
                    ]
                }
            }]
        }),
        &request,
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "exec");
    assert_eq!(converted["output"][0]["input"], "ls -la");
    assert_eq!(converted["output"][1]["type"], "function_call");
    assert_eq!(converted["output"][1]["name"], "open_file");
    assert_eq!(converted["output"][1]["namespace"], "mcp__vscode_mcp__");
}

#[test]
fn chat_completion_response_reconstructs_apply_patch_proxy_call() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_patch",
            "created": 123,
            "model": "gpt-5-mini",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch_add_file",
                            "arguments": "{\"path\":\"README.md\",\"content\":\"hello\"}"
                        }
                    }]
                }
            }]
        }),
        &json!({
            "model": "gpt-5-mini",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "apply_patch");
    assert_eq!(
        converted["output"][0]["input"],
        "*** Begin Patch\n*** Add File: README.md\n+hello\n*** End Patch"
    );
}

#[test]
fn chat_completion_response_reconstructs_apply_patch_replace_file_proxy_call() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_patch_replace",
            "created": 123,
            "model": "gpt-5-mini",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch_replace_file",
                            "arguments": "{\"path\":\"README.md\",\"content\":\"hello\"}"
                        }
                    }]
                }
            }]
        }),
        &json!({
            "model": "gpt-5-mini",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "apply_patch");
    assert_eq!(
        converted["output"][0]["input"],
        "*** Begin Patch\n*** Delete File: README.md\n*** Add File: README.md\n+hello\n*** End Patch"
    );
}

#[test]
fn chat_completion_response_remaps_string_apply_patch_proxy_tools() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_patch_string_tool",
            "created": 123,
            "model": "gpt-5-mini",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch_add_file",
                            "arguments": "{\"path\":\"docs/test.md\",\"content\":\"# Test\\n\"}"
                        }
                    }]
                }
            }]
        }),
        &json!({
            "model": "gpt-5-mini",
            "tools": ["apply_patch_add_file", "apply_patch_batch"]
        }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "custom_tool_call");
    assert_eq!(converted["output"][0]["name"], "apply_patch");
    assert_eq!(
        converted["output"][0]["input"],
        "*** Begin Patch\n*** Add File: docs/test.md\n+# Test\n*** End Patch"
    );
}

#[test]
fn chat_completion_response_maps_gemini_and_claude_cache_usage_like_ccx() {
    let gemini = chat_completion_to_response(json!({
        "id": "chatcmpl_gemini_usage",
        "created": 123,
        "model": "gemini-proxy",
        "choices": [{ "finish_reason": "stop", "message": { "role": "assistant", "content": "ok" } }],
        "usage": {
            "promptTokenCount": 20,
            "cachedContentTokenCount": 5,
            "candidatesTokenCount": 7
        }
    }))
    .unwrap();
    assert_eq!(gemini["usage"]["input_tokens"], 15);
    assert_eq!(gemini["usage"]["output_tokens"], 7);
    assert_eq!(gemini["usage"]["total_tokens"], 27);
    assert_eq!(gemini["usage"]["input_tokens_details"]["cached_tokens"], 5);

    let claude = chat_completion_to_response(json!({
        "id": "chatcmpl_claude_usage",
        "created": 123,
        "model": "claude-proxy",
        "choices": [{ "finish_reason": "stop", "message": { "role": "assistant", "content": "ok" } }],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 3,
            "cache_read_input_tokens": 2,
            "cache_creation_5m_input_tokens": 4,
            "cache_creation_1h_input_tokens": 6
        }
    }))
    .unwrap();
    assert_eq!(claude["usage"]["input_tokens"], 10);
    assert_eq!(claude["usage"]["total_tokens"], 25);
    assert_eq!(claude["usage"]["cache_read_input_tokens"], 2);
    assert_eq!(claude["usage"]["cache_creation_5m_input_tokens"], 4);
    assert_eq!(claude["usage"]["cache_creation_1h_input_tokens"], 6);
    assert_eq!(claude["usage"]["cache_ttl"], "mixed");
    assert!(claude["usage"].get("input_tokens_details").is_none());
}

#[test]
fn chat_completion_response_splits_inline_think_block() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_think",
        "created": 123,
        "model": "MiniMax-M2.7",
        "choices": [{
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "<think>\nNeed context.\n</think>\n\npong"
            }
        }]
    }))
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "reasoning");
    assert_eq!(
        converted["output"][0]["summary"][0]["text"],
        "Need context."
    );
    assert_eq!(converted["output"][1]["type"], "message");
    assert_eq!(converted["output"][1]["content"][0]["text"], "pong");
}

#[test]
fn chat_sse_converts_to_responses_sse_events() {
    let converted = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_1","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"content":"hel"},"finish_reason":null}]}

data: {"id":"chatcmpl_1","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}

data: [DONE]

"#,
    );

    assert!(converted.contains("event: response.created"));
    assert!(converted.contains("event: response.output_text.delta"));
    assert!(converted.contains("\"delta\":\"hel\""));
    assert!(converted.contains("\"text\":\"hello\""));
    assert!(converted.contains("\"input_tokens\":3"));
    assert!(converted.contains("event: response.completed"));
    assert!(converted.contains("data: [DONE]"));
}

#[test]
fn chat_sse_emits_sequence_numbers_and_incomplete_terminal_event() {
    let converted = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_len","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"content":"partial"},"finish_reason":"length"}]}

data: [DONE]

"#,
    );

    let events = parse_response_sse_events(&converted);
    assert!(!events.is_empty());
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.data["sequence_number"], json!(index as u64));
    }
    let terminal = events.last().unwrap();
    assert_eq!(terminal.event, "response.incomplete");
    assert_eq!(terminal.data["type"], "response.incomplete");
    assert_eq!(terminal.data["response"]["status"], "incomplete");
    assert_eq!(
        terminal.data["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert!(!converted.contains("event: response.completed"));
}

#[test]
fn chat_sse_converts_reasoning_inline_think_tools_and_errors_like_ccs() {
    let reasoning = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_reason","created":123,"model":"deepseek-reasoner","choices":[{"delta":{"reasoning_content":"Need context. "}}]}

data: {"id":"chatcmpl_reason","created":123,"model":"deepseek-reasoner","choices":[{"delta":{"content":"Done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":6,"total_tokens":10,"completion_tokens_details":{"reasoning_tokens":3}}}

data: [DONE]

"#,
    );
    assert!(reasoning.contains("event: response.in_progress"));
    assert!(reasoning.contains("event: response.reasoning_summary_part.added"));
    assert!(reasoning.contains("event: response.reasoning_summary_text.delta"));
    assert!(reasoning.contains("event: response.reasoning_summary_text.done"));
    assert!(reasoning.contains("\"reasoning_content\":\"Need context. \""));
    assert!(reasoning.contains("\"type\":\"reasoning\""));
    assert!(reasoning.contains("\"text\":\"Done\""));
    assert!(reasoning.contains("\"reasoning_tokens\":3"));

    let inline_think = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_minimax","created":123,"model":"MiniMax-M2.7","choices":[{"delta":{"content":"<think>\nNeed"}}]}

data: {"id":"chatcmpl_minimax","created":123,"model":"MiniMax-M2.7","choices":[{"delta":{"content":" context.</think>\n\npong"},"finish_reason":"stop"}]}

"#,
    );
    assert!(inline_think.contains("Need context."));
    assert!(inline_think.contains("\"text\":\"pong\""));
    assert!(!inline_think.contains("<think>"));
    assert!(!inline_think.contains("</think>"));

    let tool = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_tool","model":"gpt-5.4","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather"}}]}}]}

data: {"id":"chatcmpl_tool","model":"gpt-5.4","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":\"Tokyo\"}"}}]},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
    );
    assert!(tool.contains("event: response.function_call_arguments.delta"));
    assert!(tool.contains("event: response.function_call_arguments.done"));
    assert!(tool.contains("\"type\":\"function_call\""));
    assert!(tool.contains("\"call_id\":\"call_1\""));
    let tool_events = parse_response_sse_events(&tool);
    let arguments_done = tool_events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "get_weather");

    let error = chat_sse_to_responses_sse(
        r#"event: error
data: {"error":{"message":"bad request","type":"invalid_request_error"}}

data: [DONE]

"#,
    );
    assert!(error.contains("event: response.failed"));
    assert!(error.contains("bad request"));
    assert!(error.contains("invalid_request_error"));
    assert!(!error.contains("event: response.completed"));
}

#[test]
fn chat_sse_maps_web_search_to_native_call_events() {
    let converted = chat_sse_to_responses_sse_with_request(
        r#"data: {"id":"chatcmpl_web","model":"gpt-chat","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_web","type":"function","function":{"name":"web_search","arguments":"{\"query\":\"pal mcp GitHub\"}"}}]}}]}

data: {"id":"chatcmpl_web","model":"gpt-chat","choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
        &json!({
            "model": "gpt-chat",
            "tools": [{ "type": "web_search" }]
        }),
    );

    assert!(!converted.contains("response.function_call_arguments"));
    let events = parse_response_sse_events(&converted);
    let added = events
        .iter()
        .find(|event| event.event == "response.output_item.added")
        .unwrap();
    assert_eq!(added.data["item"]["type"], "web_search_call");
    assert_eq!(added.data["item"]["status"], "in_progress");
    assert_eq!(added.data["item"]["execution"], "client");
    let done = events
        .iter()
        .find(|event| event.event == "response.output_item.done")
        .unwrap();
    assert_eq!(done.data["item"]["type"], "web_search_call");
    assert_eq!(done.data["item"]["id"], "ws_call_web");
    assert_eq!(done.data["item"]["status"], "completed");
    assert_eq!(done.data["item"]["execution"], "client");
    assert_eq!(done.data["item"]["action"]["type"], "search");
    assert_eq!(done.data["item"]["action"]["query"], "pal mcp GitHub");
}

#[test]
fn chat_sse_maps_web_search_to_search_mcp_events_when_available() {
    let converted = chat_sse_to_responses_sse_with_request(
        r#"data: {"id":"chatcmpl_web","model":"gpt-chat","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_web","type":"function","function":{"name":"web_search","arguments":"{\"query\":\"pal mcp GitHub\"}"}}]}}]}

data: {"id":"chatcmpl_web","model":"gpt-chat","choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
        &json!({
            "model": "gpt-chat",
            "tools": [
                { "type": "web_search" },
                tavily_namespace_tool()
            ]
        }),
    );

    assert!(converted.contains("response.function_call_arguments.done"));
    assert!(!converted.contains("web_search_call"));
    let events = parse_response_sse_events(&converted);
    let added = events
        .iter()
        .find(|event| event.event == "response.output_item.added")
        .unwrap();
    assert_eq!(added.data["item"]["type"], "function_call");
    assert_eq!(added.data["item"]["name"], "tavily_search");
    assert_eq!(added.data["item"]["namespace"], "mcp__tavily");
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "tavily_search");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
    let done = events
        .iter()
        .find(|event| event.event == "response.output_item.done")
        .unwrap();
    assert_eq!(done.data["item"]["type"], "function_call");
    assert_eq!(done.data["item"]["name"], "tavily_search");
    assert_eq!(done.data["item"]["namespace"], "mcp__tavily");
    assert_eq!(
        done.data["item"]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[test]
fn chat_sse_maps_custom_tool_call_with_request_context() {
    let converted = chat_sse_to_responses_sse_with_request(
        r#"data: {"id":"chatcmpl_custom","model":"gpt-5.4","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_custom","type":"function","function":{"name":"exec"}}]}}]}

data: {"id":"chatcmpl_custom","model":"gpt-5.4","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"input\":"}}]}}]}

data: {"id":"chatcmpl_custom","model":"gpt-5.4","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls -la\"}"}}]},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
        &json!({
            "model": "gpt-5.4",
            "tools": [{ "type": "custom", "name": "exec" }]
        }),
    );

    assert!(converted.contains("response.custom_tool_call_input.delta"));
    assert!(converted.contains("response.custom_tool_call_input.done"));
    assert_eq!(
        converted
            .matches("event: response.custom_tool_call_input.delta")
            .count(),
        1
    );
    assert_eq!(
        converted
            .matches("event: response.custom_tool_call_input.done")
            .count(),
        1
    );
    assert!(converted.contains("\"type\":\"custom_tool_call\""));
    assert!(converted.contains("\"name\":\"exec\""));
    assert!(converted.contains("\"input\":\"ls -la\""));
    assert!(converted.contains("data: [DONE]"));

    let events = parse_response_sse_events(&converted);
    let done = events
        .iter()
        .find(|event| event.event == "response.custom_tool_call_input.done")
        .unwrap();
    assert_eq!(done.data["input"], "ls -la");
}

#[test]
fn anthropic_sse_converts_to_responses_sse_events() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_stream","type":"message","role":"assistant","model":"claude-sonnet-4","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Need context."}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig_stream"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: content_block_start
data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"codex\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":2}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9,"output_tokens_details":{"thinking_tokens":4}}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-sonnet-4",
            "tools": [{ "type": "function", "name": "lookup", "parameters": { "type": "object" } }]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        events
            .iter()
            .any(|event| event.event == "response.reasoning_summary_text.delta")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event == "response.output_text.delta")
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "lookup");
    assert_eq!(arguments_done.data["arguments"], r#"{"query":"codex"}"#);
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(completed.data["response"]["usage"]["input_tokens"], 7);
    assert_eq!(completed.data["response"]["usage"]["output_tokens"], 9);
    assert_eq!(
        completed.data["response"]["usage"]["output_tokens_details"]["thinking_tokens"],
        4
    );
    assert_eq!(
        completed.data["response"]["usage"]["output_tokens_details"]["reasoning_tokens"],
        4
    );
    assert_eq!(
        completed.data["response"]["output"][0]["encrypted_content"],
        "sig_stream"
    );
    assert!(converted.contains("data: [DONE]"));
}

#[test]
fn anthropic_sse_maps_web_search_to_native_call_events() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_web_stream","type":"message","role":"assistant","model":"claude-sonnet-4","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_web","name":"web_search","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"pal mcp GitHub\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-sonnet-4",
            "tools": [{ "type": "web_search" }]
        }),
    );

    assert!(!converted.contains("response.function_call_arguments"));
    let events = parse_response_sse_events(&converted);
    let added = events
        .iter()
        .find(|event| event.event == "response.output_item.added")
        .unwrap();
    assert_eq!(added.data["item"]["type"], "web_search_call");
    assert_eq!(added.data["item"]["status"], "in_progress");
    assert_eq!(added.data["item"]["execution"], "client");
    let done = events
        .iter()
        .find(|event| event.event == "response.output_item.done")
        .unwrap();
    assert_eq!(done.data["item"]["type"], "web_search_call");
    assert_eq!(done.data["item"]["id"], "ws_toolu_web");
    assert_eq!(done.data["item"]["status"], "completed");
    assert_eq!(done.data["item"]["execution"], "client");
    assert_eq!(done.data["item"]["action"]["type"], "search");
    assert_eq!(done.data["item"]["action"]["query"], "pal mcp GitHub");
}

#[test]
fn anthropic_sse_maps_web_search_to_search_mcp_events_when_available() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_web_stream","type":"message","role":"assistant","model":"claude-sonnet-4","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_web","name":"web_search","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"pal mcp GitHub\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-sonnet-4",
            "tools": [
                { "type": "web_search" },
                tavily_namespace_tool()
            ]
        }),
    );

    assert!(converted.contains("response.function_call_arguments.done"));
    assert!(!converted.contains("web_search_call"));
    let events = parse_response_sse_events(&converted);
    let added = events
        .iter()
        .find(|event| event.event == "response.output_item.added")
        .unwrap();
    assert_eq!(added.data["item"]["type"], "function_call");
    assert_eq!(added.data["item"]["name"], "tavily_search");
    assert_eq!(added.data["item"]["namespace"], "mcp__tavily");
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "tavily_search");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
    let done = events
        .iter()
        .find(|event| event.event == "response.output_item.done")
        .unwrap();
    assert_eq!(done.data["item"]["type"], "function_call");
    assert_eq!(done.data["item"]["name"], "tavily_search");
    assert_eq!(done.data["item"]["namespace"], "mcp__tavily");
    assert_eq!(
        done.data["item"]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[test]
fn anthropic_sse_textual_invoke_converts_to_tool_call_events() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_textual_stream","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"course\n<invoke name=\"exec_command\">\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<parameter name=\"cmd\">git status --short</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.output_text.delta")
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"git status --short"}"#
    );
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(
        completed.data["response"]["output"][0]["type"],
        "function_call"
    );
}

#[test]
fn anthropic_sse_call_prefixed_textual_invoke_converts_to_tool_call_events() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_textual_stream_call","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call\n<invoke name=\"exec_command\">\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<parameter name=\"cmd\">git diff crates/codex-elves-core/src/protocol_proxy.rs</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.output_text.delta")
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"git diff crates/codex-elves-core/src/protocol_proxy.rs"}"#
    );
}

#[test]
fn anthropic_sse_ignores_descriptive_invoke_text_before_real_exec_call() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_stream_descriptive_invoke","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"这里是协议转换 bug：工具调用被当成文本处理了（call<invoke name=...> 泄漏成文本）。\n\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"先看现有逻辑。\n\ncall\n<invoke name=\"exec_command\">\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<parameter name=\"cmd\">cd E:\\code\\junes\\github\\CodexElves; rg -n \"invoke|textual_invoke|call_prefixed|<invoke|antml_tool_call|parse_textual|extract_tool\" crates/codex-elves-core/src/protocol_proxy.rs | Select-Object -First 40</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta: String = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect();
    assert!(
        text_delta.contains("call<invoke name=...>") && text_delta.contains("先看现有逻辑"),
        "描述正文应保留为文本，实际={text_delta:?}"
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该还原出后续真实 exec_command 调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert!(
        arguments_done.data["arguments"]
            .as_str()
            .unwrap()
            .contains("\"cmd\":\"cd E:\\\\code\\\\junes\\\\github\\\\CodexElves; rg -n")
    );
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    let output = completed.data["response"]["output"].as_array().unwrap();
    assert!(output.iter().any(|item| item["type"] == "message"));
    assert!(output.iter().any(|item| item["type"] == "function_call"));
}

#[test]
fn anthropic_sse_textual_invoke_split_tag_across_chunks_converts_exec_call() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_split_invoke_tag","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call\n<in"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"voke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</in"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"voke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.output_text.delta"),
        "完整工具调用分片不应泄漏为文本"
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该跨 chunk 还原 exec_command 调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"git status --short"}"#
    );
}

#[test]
fn anthropic_sse_textual_invoke_split_after_marker_whitespace_converts_exec_call() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_split_marker_whitespace","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call\n\n\n<in"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"voke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.output_text.delta"),
        "marker 和 <invoke> 之间有多空白时仍不应泄漏为文本"
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该跨 marker 空白 chunk 还原 exec_command 调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"git status --short"}"#
    );
}

#[test]
fn anthropic_sse_drops_standalone_call_marker_before_native_tool_use() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_native_tool_with_call_marker","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"cmd":"git status --short"}}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.output_text.delta"),
        "原生 tool_use 前的孤立 call 标记不应显示成正文"
    );
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该保留原生工具调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"git status --short"}"#
    );
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    let output = completed.data["response"]["output"].as_array().unwrap();
    assert!(!output.iter().any(|item| item["type"] == "message"));
    assert!(output.iter().any(|item| item["type"] == "function_call"));
}

#[test]
fn anthropic_sse_keeps_marker_suffix_word_before_native_tool_use() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_native_tool_with_marker_suffix","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"recall"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"cmd":"git status --short"}}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta: String = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect();
    assert_eq!(text_delta, "recall");
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该保留后续原生工具调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
}

#[test]
fn anthropic_sse_keeps_standalone_call_marker_when_no_tool_follows() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_call_marker_without_tool","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta: String = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect();
    assert_eq!(text_delta, "call");
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(
        completed.data["response"]["output"][0]["content"][0]["text"],
        "call"
    );
}

#[test]
fn anthropic_sse_keeps_consecutive_standalone_markers_without_tool() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_consecutive_markers_without_tool","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"codex"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta: String = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect();
    assert_eq!(text_delta, "callcodex");
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(
        completed.data["response"]["output"][0]["content"][0]["text"],
        "callcodex"
    );
}

#[test]
fn anthropic_sse_textual_invoke_apply_patch_proxy_preserves_update_hunks() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_stream_patch_proxy","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call\n<invoke name=\"apply_patch_update_file\">\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<parameter name=\"path\">crates/codex-elves-core/tests/tmp_real_config_sync.rs</parameter>\n<parameter name=\"hunks\">[{\"context\":\"fn tmp_real_config_sync_only_touches_mcp() {\",\"lines\":[{\"op\":\"context\",\"text\":\"let before = original.clone();\"},{\"op\":\"add\",\"text\":\"assert_eq!(before, after);\"}]}]</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [{ "type": "custom", "name": "apply_patch" }]
        }),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.function_call_arguments.done"),
        "apply_patch proxy 应还原为 custom tool，而不是普通 function_call"
    );
    let input_done = events
        .iter()
        .find(|event| event.event == "response.custom_tool_call_input.done")
        .expect("应该还原出 apply_patch custom tool");
    assert_eq!(
        input_done.data["input"],
        "*** Begin Patch\n*** Update File: crates/codex-elves-core/tests/tmp_real_config_sync.rs\n@@ fn tmp_real_config_sync_only_touches_mcp() {\n let before = original.clone();\n+assert_eq!(before, after);\n*** End Patch"
    );
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(
        completed.data["response"]["output"][0]["type"],
        "custom_tool_call"
    );
    assert_eq!(
        completed.data["response"]["output"][0]["name"],
        "apply_patch"
    );
}

#[test]
fn anthropic_sse_leading_text_then_textual_invoke_splits_message_and_tool_call() {
    // 回归：模型先输出一段正文，再在同一文本块末尾追加 call/<invoke> 工具调用，
    // 且跨多个 delta 分块。以前流式会因「开头不像工具调用」而整块透传，导致工具变文本。
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_lead_then_invoke","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"代码正确。现在做严格的逻辑复核：跟 release "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"build。\n\ncall\n<invoke name=\"exec_command\">\n"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"<parameter name=\"cmd\">cargo build --release</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    // 前导正文仍作为文本输出。
    let text_delta: String = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect();
    assert!(
        text_delta.contains("代码正确") && text_delta.contains("release build。"),
        "前导正文应保留为文本，实际={text_delta:?}"
    );
    // 末尾的 <invoke> 应被还原为工具调用。
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该还原出 function_call");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"cmd":"cargo build --release"}"#
    );
    // 输出同时含 message 和 function_call 两类 item。
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    let output = completed.data["response"]["output"].as_array().unwrap();
    assert!(output.iter().any(|item| item["type"] == "message"));
    assert!(output.iter().any(|item| item["type"] == "function_call"));
}
#[test]
fn chat_sse_maps_refusal_delta_to_responses_refusal_events() {
    let converted = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_refusal","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"refusal":"No"},"finish_reason":null}]}

data: {"id":"chatcmpl_refusal","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"refusal":"pe"},"finish_reason":"stop"}]}

data: [DONE]

"#,
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        events
            .iter()
            .any(|event| event.event == "response.refusal.delta")
    );
    let refusal_done = events
        .iter()
        .find(|event| event.event == "response.refusal.done")
        .unwrap();
    assert_eq!(refusal_done.data["refusal"], "Nope");
    let content_done = events
        .iter()
        .find(|event| {
            event.event == "response.content_part.done" && event.data["part"]["type"] == "refusal"
        })
        .unwrap();
    assert_eq!(content_done.data["part"]["refusal"], "Nope");
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    assert_eq!(
        completed.data["response"]["output"][0]["content"][0]["type"],
        "refusal"
    );
}

#[test]
fn chat_sse_converter_handles_partial_chunks_and_utf8_boundaries() {
    let sse = "data: {\"id\":\"chatcmpl_utf8\",\"created\":123,\"model\":\"gpt-5.4\",\"choices\":[{\"delta\":{\"content\":\"你好\"},\"finish_reason\":\"stop\"}]}\r\n\r\n";
    let bytes = sse.as_bytes();
    let split = bytes
        .windows("好".len())
        .position(|window| window == "好".as_bytes())
        .unwrap()
        + 1;

    let mut converter = ChatSseToResponsesConverter::default();
    let mut output = converter.push_bytes(&bytes[..split]);
    output.extend(converter.push_bytes(&bytes[split..]));
    output.extend(converter.finish());
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("\"delta\":\"你好\""));
    assert!(output.contains("event: response.completed"));
}

#[test]
fn anthropic_sse_converter_handles_partial_chunks_and_utf8_boundaries() {
    let sse = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_utf8","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#;
    let bytes = sse.as_bytes();
    let split = bytes
        .windows("好".len())
        .position(|window| window == "好".as_bytes())
        .unwrap()
        + 1;

    let mut converter = AnthropicSseToResponsesConverter::default();
    let mut output = converter.push_bytes(&bytes[..split]);
    output.extend(converter.push_bytes(&bytes[split..]));
    output.extend(converter.finish());
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("\"delta\":\"你好\""));
    assert!(output.contains("event: response.completed"));
}

#[test]
fn chat_sse_fails_on_invalid_json_or_unfinished_stream() {
    let invalid = chat_sse_to_responses_sse("data: {bad json}\n\n");
    let invalid_events = parse_response_sse_events(&invalid);
    let failed = invalid_events
        .iter()
        .find(|event| event.event == "response.failed")
        .unwrap();
    assert_eq!(failed.data["response"]["error"]["type"], "invalid_sse_json");
    assert!(!invalid.contains("event: response.completed"));

    let unfinished = chat_sse_to_responses_sse(
        r#"data: {"id":"chatcmpl_drop","created":1710000000,"model":"gpt-5-mini","choices":[{"delta":{"content":"hello"},"finish_reason":null}]}

"#,
    );
    let unfinished_events = parse_response_sse_events(&unfinished);
    let failed = unfinished_events
        .iter()
        .find(|event| event.event == "response.failed")
        .unwrap();
    assert_eq!(failed.data["response"]["error"]["type"], "stream_error");
    assert!(!unfinished.contains("event: response.completed"));
}

#[test]
fn anthropic_sse_fails_on_unfinished_stream() {
    let unfinished = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_drop","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}

"#,
        &json!({
            "model": "claude-opus-4-8"
        }),
    );
    let events = parse_response_sse_events(&unfinished);
    let failed = events
        .iter()
        .find(|event| event.event == "response.failed")
        .unwrap();
    assert_eq!(failed.data["response"]["error"]["type"], "stream_error");
    assert!(!unfinished.contains("event: response.completed"));
}

#[test]
fn chat_completions_url_normalizes_common_base_urls() {
    assert_eq!(
        chat_completions_url("https://api.example.test"),
        "https://api.example.test/v1/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/v1"),
        "https://api.example.test/v1/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/openai"),
        "https://api.example.test/openai/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/v1/chat/completions"),
        "https://api.example.test/v1/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/v2"),
        "https://api.example.test/v2/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/v1beta"),
        "https://api.example.test/v1beta/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://api.example.test/openai#"),
        "https://api.example.test/openai/chat/completions"
    );
}

#[test]
fn anthropic_messages_url_normalizes_common_base_urls() {
    assert_eq!(
        anthropic_messages_url("https://api.example.test"),
        "https://api.example.test/v1/messages"
    );
    assert_eq!(
        anthropic_messages_url("https://api.example.test/v1"),
        "https://api.example.test/v1/messages"
    );
    assert_eq!(
        anthropic_messages_url("https://api.example.test/openai"),
        "https://api.example.test/openai/messages"
    );
    assert_eq!(
        anthropic_messages_url("https://api.example.test/v1/messages"),
        "https://api.example.test/v1/messages"
    );
    assert_eq!(
        anthropic_messages_url("https://api.example.test/v2"),
        "https://api.example.test/v2/messages"
    );
    assert_eq!(
        anthropic_messages_url("https://api.example.test/openai#"),
        "https://api.example.test/openai/messages"
    );
}

#[test]
fn models_url_normalizes_common_base_urls() {
    assert_eq!(
        models_url("https://api.example.test"),
        "https://api.example.test/v1/models"
    );
    assert_eq!(
        models_url("https://api.example.test/v1"),
        "https://api.example.test/v1/models"
    );
    assert_eq!(
        models_url("https://api.example.test/v1/chat/completions"),
        "https://api.example.test/v1/models"
    );
    assert_eq!(
        models_url("https://api.example.test/models"),
        "https://api.example.test/models"
    );
    assert_eq!(
        models_url("https://api.example.test/v2"),
        "https://api.example.test/v2/models"
    );
    assert_eq!(
        models_url("https://api.example.test/v1beta"),
        "https://api.example.test/v1beta/models"
    );
    assert_eq!(
        models_url("https://api.example.test/openai#"),
        "https://api.example.test/openai/models"
    );
}

#[test]
fn models_proxy_path_matches_v1_models() {
    assert!(is_models_proxy_path("/models"));
    assert!(is_models_proxy_path("/v1/models"));
    assert!(is_models_proxy_path("/v1/models?limit=10"));
    assert!(!is_models_proxy_path("/v1/responses"));
}

#[test]
fn retained_upstream_header_timeouts_match_proxy_policy() {
    assert_eq!(upstream_models_header_timeout(), Duration::from_secs(30));
    assert_eq!(
        upstream_deferred_stream_header_timeout(),
        Duration::from_secs(900)
    );
}

#[tokio::test]
async fn upstream_request_returns_when_provider_accepts_but_never_sends_headers() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let Ok((_stream, _addr)) = listener.accept().await else {
            return;
        };
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let started = Instant::now();
    let result = send_upstream_request_with_header_timeout(
        upstream_http_client()
            .unwrap()
            .get(format!("http://{addr}/v1/models")),
        Duration::from_millis(100),
    )
    .await;

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    server.abort();
}

#[tokio::test]
async fn aggregate_proxy_fails_over_to_next_member_in_same_request() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let first_addr = first.local_addr().unwrap();
    let second = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let second_addr = second.local_addr().unwrap();
    let first_server = tokio::spawn(respond_once(
        first,
        "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 11\r\ncontent-type: application/json\r\n\r\n{\"error\":1}",
    ));
    let second_server = tokio::spawn(respond_once(
        second,
        "HTTP/1.1 200 OK\r\ncontent-length: 35\r\ncontent-type: application/json\r\n\r\n{\"id\":\"resp_1\",\"object\":\"response\"}",
    ));
    let settings = aggregate_proxy_settings(
        "failover",
        format!("http://{first_addr}/v1"),
        format!("http://{second_addr}/v1"),
    );

    let result = open_responses_proxy_request_with_settings(
        r#"{"model":"gpt-5-mini","input":"hi","stream":false}"#,
        settings,
    )
    .await
    .unwrap();
    let body = result
        .response
        .expect("non-stream aggregate response should include upstream response")
        .bytes()
        .await
        .unwrap();

    assert_eq!(result.status_code, 200);
    assert_eq!(body.as_ref(), br#"{"id":"resp_1","object":"response"}"#);
    first_server.await.unwrap();
    second_server.await.unwrap();
}

#[tokio::test]
async fn aggregate_stream_request_sends_sse_accept_header() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let fallback = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let fallback_addr = fallback.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let read = stream.read(&mut buffer).await.unwrap();
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-length: 14\r\ncontent-type: text/event-stream\r\n\r\ndata: [DONE]\n\n",
            )
            .await
            .unwrap();
        request
    });
    let fallback_server = tokio::spawn(respond_once(
        fallback,
        "HTTP/1.1 200 OK\r\ncontent-length: 14\r\ncontent-type: text/event-stream\r\n\r\ndata: [DONE]\n\n",
    ));
    let settings = aggregate_proxy_settings(
        "stream",
        format!("http://{addr}/v1"),
        format!("http://{fallback_addr}/v1"),
    );

    let result = open_responses_proxy_request_with_settings(
        r#"{"model":"gpt-5-mini","input":"hi","stream":true}"#,
        settings,
    )
    .await
    .unwrap();
    let request = server.await.unwrap();

    assert_eq!(result.status_code, 200);
    assert!(result.is_stream);
    assert!(
        request
            .to_ascii_lowercase()
            .contains("accept: text/event-stream")
    );
    fallback_server.abort();
}

#[tokio::test]
async fn continue_thinking_reports_accumulated_reasoning_tokens() {
    let server = spawn_chat_server_with_response(responses_sse_with_reasoning("resp_continue", 38));
    let settings = BackendSettings {
        gpt_reasoning_continuation: true,
        relay_profiles: vec![RelayProfile {
            id: "responses".to_string(),
            name: "Responses".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::Responses,
            relay_mode: RelayMode::MixedApi,
            model_mappings: vec![RelayModelMapping {
                request_model: "gpt-responses".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "responses".to_string(),
        ..BackendSettings::default()
    };

    let result = apply_continue_thinking_to_responses_stream(
        &json!({
            "model": "gpt-responses",
            "input": "hi",
            "stream": true,
            "reasoning": { "effort": "high" }
        }),
        settings,
        None,
        responses_sse_with_reasoning("resp_first", 516),
    )
    .await;

    assert!(result.triggered);
    assert_eq!(result.rounds, 1);
    assert_eq!(result.reasoning_tokens, Some(554));
    assert!(result.sse_text.contains("\"reasoning_tokens\":38"));
    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
    assert!(request.body.contains("continue_thinking"));
}

#[tokio::test]
async fn continue_thinking_skips_response_with_tool_call_output() {
    let settings = BackendSettings {
        gpt_reasoning_continuation: true,
        relay_profiles: vec![RelayProfile {
            id: "responses".to_string(),
            name: "Responses".to_string(),
            base_url: "http://127.0.0.1:9/v1".to_string(),
            upstream_base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::Responses,
            relay_mode: RelayMode::MixedApi,
            model_mappings: vec![RelayModelMapping {
                request_model: "gpt-responses".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "responses".to_string(),
        ..BackendSettings::default()
    };
    let first_round = responses_sse_with_reasoning_and_output(
        "resp_tool_call",
        516,
        json!([
            {
                "id": "rs_1",
                "type": "reasoning",
                "encrypted_content": "abc123"
            },
            {
                "id": "fc_1",
                "type": "function_call",
                "name": "exec_command",
                "arguments": "{\"cmd\":\"pwd\"}",
                "call_id": "call_1"
            }
        ]),
    );

    let result = apply_continue_thinking_to_responses_stream(
        &json!({
            "model": "gpt-responses",
            "input": "hi",
            "stream": true,
            "reasoning": { "effort": "high" }
        }),
        settings,
        None,
        first_round,
    )
    .await;

    assert!(!result.triggered);
    assert_eq!(result.rounds, 0);
    assert_eq!(result.reasoning_tokens, Some(516));
    assert!(result.sse_text.contains("resp_tool_call"));
    assert!(result.request_body.is_none());
    assert!(result.before_response_body.is_none());
    assert!(result.after_response_body.is_none());
}

#[tokio::test]
async fn continue_thinking_respects_configured_max_rounds() {
    let server =
        spawn_chat_server_with_response(responses_sse_with_reasoning("resp_continue_one", 516));
    let settings = BackendSettings {
        gpt_reasoning_continuation: true,
        gpt_reasoning_continuation_max_rounds: 1,
        relay_profiles: vec![RelayProfile {
            id: "responses".to_string(),
            name: "Responses".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::Responses,
            relay_mode: RelayMode::MixedApi,
            model_mappings: vec![RelayModelMapping {
                request_model: "gpt-responses".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "responses".to_string(),
        ..BackendSettings::default()
    };

    let result = apply_continue_thinking_to_responses_stream(
        &json!({
            "model": "gpt-responses",
            "input": "hi",
            "stream": true,
            "reasoning": { "effort": "high" }
        }),
        settings,
        None,
        responses_sse_with_reasoning("resp_first", 516),
    )
    .await;

    assert!(result.triggered);
    assert_eq!(result.rounds, 1);
    assert_eq!(result.reasoning_tokens, Some(1032));
    assert!(result.sse_text.contains("resp_continue_one"));
    let request = server.finish();
    assert!(request.body.contains("call_continue_thinking_1"));
}

async fn respond_once(listener: tokio::net::TcpListener, response: &'static str) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut buffer = [0; 1024];
    let _ = stream.read(&mut buffer).await.unwrap();
    stream.write_all(response.as_bytes()).await.unwrap();
}

fn aggregate_proxy_settings(
    id_suffix: &str,
    first_base_url: String,
    second_base_url: String,
) -> BackendSettings {
    let first_id = format!("proxy-{id_suffix}-a");
    let second_id = format!("proxy-{id_suffix}-b");
    let aggregate_id = format!("proxy-{id_suffix}-agg");
    BackendSettings {
        relay_profiles: vec![
            RelayProfile {
                id: first_id.clone(),
                name: "first".to_string(),
                base_url: first_base_url,
                api_key: "sk-first".to_string(),
                model_mappings: vec![RelayModelMapping {
                    request_model: "gpt-5-mini".to_string(),
                    protocol: RelayProtocol::Responses,
                    context_window: "200000".to_string(),
                }],
                ..RelayProfile::default()
            },
            RelayProfile {
                id: second_id.clone(),
                name: "second".to_string(),
                base_url: second_base_url,
                api_key: "sk-second".to_string(),
                model_mappings: vec![RelayModelMapping {
                    request_model: "gpt-5-mini".to_string(),
                    protocol: RelayProtocol::Responses,
                    context_window: "200000".to_string(),
                }],
                ..RelayProfile::default()
            },
            RelayProfile {
                id: aggregate_id.clone(),
                name: "aggregate".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
        ],
        active_relay_id: aggregate_id.clone(),
        active_aggregate_relay_id: aggregate_id.clone(),
        aggregate_relay_profiles: vec![AggregateRelayProfile {
            id: aggregate_id,
            name: "aggregate".to_string(),
            strategy: AggregateRelayStrategy::RequestRoundRobin,
            members: vec![
                AggregateRelayMember {
                    relay_id: first_id,
                    weight: 1,
                },
                AggregateRelayMember {
                    relay_id: second_id,
                    weight: 1,
                },
            ],
        }],
        ..BackendSettings::default()
    }
}
#[tokio::test]
async fn chat_completions_proxy_uses_configured_user_agent() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "Configured-Codex-UA/1.0");

    let upstream = open_chat_completions_proxy_request(
        r#"{"model":"gpt-5.5","messages":[{"role":"user","content":"hello"}]}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    assert_eq!(request.user_agent, "Configured-Codex-UA/1.0");
}

#[tokio::test]
async fn chat_completions_proxy_passes_through_original_user_agent_when_unconfigured() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "");

    let upstream = open_chat_completions_proxy_request(
        r#"{"model":"gpt-5.5","messages":[{"role":"user","content":"hello"}]}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    assert_eq!(request.user_agent, "Original-Codex-UA/1.0");
}

#[tokio::test]
async fn responses_proxy_passes_through_original_user_agent_when_unconfigured() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "");

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-5.5","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    assert_eq!(request.user_agent, "Original-Codex-UA/1.0");
}

#[tokio::test]
async fn responses_proxy_directs_responses_models_to_responses_upstream() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-responses","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::Responses
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
}

#[tokio::test]
async fn responses_proxy_directs_chat_models_to_chat_completions_upstream() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-chat","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::ChatCompletions
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/chat/completions");
}

#[tokio::test]
async fn responses_proxy_replaces_system_prompt_before_chat_conversion() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings_with_system_prompt(
        temp.path(),
        &server.base_url,
        "只使用新的系统提示词。",
    );

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-chat","instructions":"old system","input":[{"type":"message","role":"developer","content":[{"type":"input_text","text":"old developer"}]},{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],"stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::ChatCompletions
    );

    let request = server.finish();
    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(
        logged_body["messages"][0],
        json!({ "role": "system", "content": "只使用新的系统提示词。" })
    );
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(
        body["messages"][0],
        json!({ "role": "system", "content": "只使用新的系统提示词。" })
    );
    assert!(
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message.get("content").and_then(Value::as_str) != Some("old developer"))
    );
}

#[tokio::test]
async fn responses_proxy_replaces_system_prompt_for_responses_upstream() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings_with_system_prompt(
        temp.path(),
        &server.base_url,
        "新的 Responses 提示词",
    );

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-responses","instructions":"old system","input":[{"type":"message","role":"system","content":[{"type":"input_text","text":"old system in input"}]},{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],"stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::Responses
    );

    let request = server.finish();
    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(logged_body["instructions"], "新的 Responses 提示词");
    assert_eq!(logged_body["input"].as_array().unwrap().len(), 1);
    assert_eq!(logged_body["input"][0]["role"], "user");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["instructions"], "新的 Responses 提示词");
    assert_eq!(body["input"].as_array().unwrap().len(), 1);
    assert_eq!(body["input"][0]["role"], "user");
}

#[tokio::test]
async fn responses_proxy_rewrites_default_system_prompt_model_for_responses_log() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-responses","instructions":"You are Codex, a coding agent based on GPT-5. GPT-5.6 Sol is available.","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(
        logged_body["instructions"],
        "You are Codex, a coding agent based on the gpt-responses model. gpt-responses is available."
    );
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["instructions"], logged_body["instructions"]);
}

#[tokio::test]
async fn responses_proxy_rewrites_default_system_prompt_model_before_chat_conversion() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-chat","instructions":"You are Codex, a coding agent based on GPT-5.","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(
        logged_body["messages"][0],
        json!({ "role": "system", "content": "You are Codex, a coding agent based on the gpt-chat model." })
    );
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["messages"][0], logged_body["messages"][0]);
}

#[tokio::test]
async fn responses_proxy_e2e_chat_upstream_regular_text_with_tools_still_returns_message() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "chatcmpl_text",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-chat",
        "choices": [{
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "pong"
            }
        }],
        "usage": {
            "prompt_tokens": 8,
            "completion_tokens": 3
        }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "gpt-chat",
        "input": "hello",
        "stream": false,
        "tools": [
            { "type": "tool_search" },
            { "type": "web_search" },
            tavily_namespace_tool(),
            pal_namespace_tool(),
            { "type": "local_shell" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/chat/completions");
    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "message");
    assert_eq!(response_body["output"][0]["content"][0]["text"], "pong");
}

#[tokio::test]
async fn responses_proxy_e2e_anthropic_upstream_regular_text_with_tools_still_returns_message() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "msg_text",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4",
        "content": [{
            "type": "text",
            "text": "pong"
        }],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 8,
            "output_tokens": 3
        }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "claude-sonnet-4",
        "input": "hello",
        "stream": false,
        "tools": [
            { "type": "tool_search" },
            { "type": "web_search" },
            tavily_namespace_tool(),
            pal_namespace_tool(),
            { "type": "local_shell" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/messages");
    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "message");
    assert_eq!(response_body["output"][0]["content"][0]["text"], "pong");
}

#[tokio::test]
async fn responses_proxy_e2e_chat_upstream_roundtrips_regular_namespace_tool_call() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "chatcmpl_pal",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-chat",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_pal",
                    "type": "function",
                    "function": {
                        "name": "mcp__pal__version",
                        "arguments": "{}"
                    }
                }]
            }
        }]
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "gpt-chat",
        "input": "check pal version",
        "stream": false,
        "tools": [pal_namespace_tool()]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/chat/completions");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    assert!(
        upstream_body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "mcp__pal__version")
    );
    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "function_call");
    assert_eq!(response_body["output"][0]["call_id"], "call_pal");
    assert_eq!(response_body["output"][0]["name"], "version");
    assert_eq!(response_body["output"][0]["namespace"], "mcp__pal");
}

#[tokio::test]
async fn responses_proxy_e2e_anthropic_upstream_roundtrips_regular_namespace_tool_call() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "msg_pal",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4",
        "content": [{
            "type": "tool_use",
            "id": "toolu_pal",
            "name": "mcp__pal__version",
            "input": {}
        }],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "claude-sonnet-4",
        "input": "check pal version",
        "stream": false,
        "tools": [pal_namespace_tool()]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/messages");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    assert!(
        upstream_body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "mcp__pal__version")
    );
    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "function_call");
    assert_eq!(response_body["output"][0]["call_id"], "toolu_pal");
    assert_eq!(response_body["output"][0]["name"], "version");
    assert_eq!(response_body["output"][0]["namespace"], "mcp__pal");
}

#[tokio::test]
async fn responses_proxy_e2e_chat_upstream_maps_web_search_to_search_mcp_when_available() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "chatcmpl_web",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-chat",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_web",
                    "type": "function",
                    "function": {
                        "name": "web_search",
                        "arguments": "{\"query\":\"pal mcp GitHub\"}"
                    }
                }]
            }
        }]
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "gpt-chat",
        "input": "search pal mcp",
        "stream": false,
        "tools": [
            { "type": "web_search" },
            tavily_namespace_tool()
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/chat/completions");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    let upstream_tool_names = upstream_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(upstream_tool_names.contains(&"web_search"));
    assert!(upstream_tool_names.contains(&"mcp__tavily__tavily_search"));

    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "function_call");
    assert_eq!(response_body["output"][0]["call_id"], "call_web");
    assert_eq!(response_body["output"][0]["name"], "tavily_search");
    assert_eq!(response_body["output"][0]["namespace"], "mcp__tavily");
    assert_eq!(
        response_body["output"][0]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[tokio::test]
async fn responses_proxy_e2e_anthropic_upstream_maps_web_search_to_search_mcp_when_available() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "msg_web",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4",
        "content": [{
            "type": "tool_use",
            "id": "toolu_web",
            "name": "web_search",
            "input": { "query": "pal mcp GitHub" }
        }],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "claude-sonnet-4",
        "input": "search pal mcp",
        "stream": false,
        "tools": [
            { "type": "web_search" },
            tavily_namespace_tool()
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/messages");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    let upstream_tool_names = upstream_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(upstream_tool_names.contains(&"web_search"));
    assert!(upstream_tool_names.contains(&"mcp__tavily__tavily_search"));

    assert_eq!(response.status, "200 OK");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "function_call");
    assert_eq!(response_body["output"][0]["call_id"], "toolu_web");
    assert_eq!(response_body["output"][0]["name"], "tavily_search");
    assert_eq!(response_body["output"][0]["namespace"], "mcp__tavily");
    assert_eq!(
        response_body["output"][0]["arguments"],
        r#"{"query":"pal mcp GitHub"}"#
    );
}

#[tokio::test]
async fn responses_proxy_e2e_chat_upstream_roundtrips_builtin_proxy_tools() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let local_shell_args = json!({ "input": "pwd" }).to_string();
    let computer_use_args = json!({ "input": "screenshot" }).to_string();
    let upstream_response = json!({
        "id": "chatcmpl_tools",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-chat",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_local_shell",
                        "type": "function",
                        "function": {
                            "name": "local_shell",
                            "arguments": local_shell_args
                        }
                    },
                    {
                        "id": "call_computer_use",
                        "type": "function",
                        "function": {
                            "name": "computer_use_preview",
                            "arguments": computer_use_args
                        }
                    }
                ]
            }
        }]
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "gpt-chat",
        "input": "find tools and search docs",
        "stream": false,
        "tools": [
            { "type": "local_shell" },
            { "type": "computer_use_preview" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/chat/completions");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    let upstream_tool_names = upstream_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(upstream_tool_names.contains(&"local_shell"));
    assert!(upstream_tool_names.contains(&"computer_use_preview"));

    assert_eq!(response.status, "200 OK");
    assert_eq!(response.content_type, "application/json; charset=utf-8");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "custom_tool_call");
    assert_eq!(response_body["output"][0]["name"], "local_shell");
    assert_eq!(response_body["output"][0]["input"], "pwd");
    assert_eq!(response_body["output"][1]["type"], "custom_tool_call");
    assert_eq!(response_body["output"][1]["name"], "computer_use_preview");
    assert_eq!(response_body["output"][1]["input"], "screenshot");
}

#[tokio::test]
async fn responses_proxy_e2e_anthropic_upstream_roundtrips_builtin_proxy_tools() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "msg_tools",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4",
        "content": [
            {
                "type": "tool_use",
                "id": "toolu_search",
                "name": "local_shell",
                "input": { "input": "pwd" }
            },
            {
                "type": "tool_use",
                "id": "toolu_web",
                "name": "computer_use_preview",
                "input": { "input": "screenshot" }
            }
        ],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5
        }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "claude-sonnet-4",
        "input": "find tools and search docs",
        "stream": false,
        "tools": [
            { "type": "local_shell" },
            { "type": "computer_use_preview" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let upstream_request = server.finish();

    assert_eq!(upstream_request.path, "/v1/messages");
    assert_eq!(upstream_request.x_api_key, "sk-test");
    assert_eq!(upstream_request.anthropic_version, "2023-06-01");
    let upstream_body: Value = serde_json::from_str(&upstream_request.body).unwrap();
    let upstream_tool_names = upstream_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(upstream_tool_names.contains(&"local_shell"));
    assert!(upstream_tool_names.contains(&"computer_use_preview"));

    assert_eq!(response.status, "200 OK");
    assert_eq!(response.content_type, "application/json; charset=utf-8");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "custom_tool_call");
    assert_eq!(response_body["output"][0]["name"], "local_shell");
    assert_eq!(response_body["output"][0]["input"], "pwd");
    assert_eq!(response_body["output"][1]["type"], "custom_tool_call");
    assert_eq!(response_body["output"][1]["name"], "computer_use_preview");
    assert_eq!(response_body["output"][1]["input"], "screenshot");
}

#[tokio::test]
async fn responses_proxy_e2e_chat_upstream_returns_codex_tool_call() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let tool_args = json!({ "query": "shell" }).to_string();
    let upstream_response = json!({
        "id": "chatcmpl_tool_search",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-chat",
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_tool_search",
                    "type": "function",
                    "function": {
                        "name": "tool_search",
                        "arguments": tool_args
                    }
                }]
            }
        }]
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "gpt-chat",
        "input": "find shell tools",
        "stream": false,
        "tools": [
            { "type": "tool_search" },
            { "type": "local_shell" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let request = server.finish();

    assert_eq!(request.path, "/v1/chat/completions");
    let first_body: Value = serde_json::from_str(&request.body).unwrap();
    let first_tool_names = first_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(first_tool_names.contains(&"tool_search"));
    assert!(first_tool_names.contains(&"local_shell"));
    assert_eq!(first_body["stream"], false);

    assert_eq!(response.status, "200 OK");
    assert_eq!(response.content_type, "application/json; charset=utf-8");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "tool_search_call");
    assert_eq!(response_body["output"][0]["call_id"], "call_tool_search");
    assert_eq!(response_body["output"][0]["execution"], "client");
    assert_eq!(
        response_body["output"][0]["arguments"],
        json!({ "query": "shell" })
    );
}

#[tokio::test]
async fn responses_proxy_e2e_anthropic_upstream_returns_codex_tool_call() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let upstream_response = json!({
        "id": "msg_tool_search",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4",
        "content": [{
            "type": "tool_use",
            "id": "toolu_search",
            "name": "tool_search",
            "input": { "query": "shell" }
        }],
        "stop_reason": "tool_use",
        "usage": { "input_tokens": 10, "output_tokens": 5 }
    })
    .to_string();
    let server = spawn_chat_server_with_response(upstream_response);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let request_body = json!({
        "model": "claude-sonnet-4",
        "input": "find shell tools",
        "stream": false,
        "tools": [
            { "type": "tool_search" },
            { "type": "local_shell" }
        ]
    })
    .to_string();

    let response = handle_responses_proxy_request(&request_body).await.unwrap();
    let request = server.finish();

    assert_eq!(request.path, "/v1/messages");
    let first_body: Value = serde_json::from_str(&request.body).unwrap();
    let first_tool_names = first_body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(first_tool_names.contains(&"tool_search"));
    assert!(first_tool_names.contains(&"local_shell"));
    assert_eq!(first_body["stream"], false);

    assert_eq!(response.status, "200 OK");
    assert_eq!(response.content_type, "application/json; charset=utf-8");
    let response_body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(response_body["output"][0]["type"], "tool_search_call");
    assert_eq!(response_body["output"][0]["call_id"], "toolu_search");
    assert_eq!(response_body["output"][0]["execution"], "client");
    assert_eq!(
        response_body["output"][0]["arguments"],
        json!({ "query": "shell" })
    );
}

#[tokio::test]
async fn responses_proxy_directs_anthropic_models_to_anthropic_upstream() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"claude-sonnet-4","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.x_api_key, "sk-test");
    assert_eq!(request.anthropic_version, "2023-06-01");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "claude-sonnet-4");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(body["output_config"], json!({ "effort": "high" }));
}

#[tokio::test]
async fn responses_proxy_falls_back_to_relay_protocol_for_models_missing_from_protocol_lists() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-unlisted","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();

    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::Responses
    );
    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
}

#[tokio::test]
async fn responses_proxy_falls_back_to_chat_protocol_for_unlisted_chat_relay_models() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "");

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-unlisted","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();

    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::ChatCompletions
    );
    let request = server.finish();
    assert_eq!(request.path, "/v1/chat/completions");
}

#[tokio::test]
async fn responses_proxy_falls_back_to_anthropic_protocol_for_unlisted_anthropic_relay_models() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_anthropic_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"claude-unlisted","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();

    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        codex_elves_core::protocol_proxy::UpstreamResponseProtocol::Anthropic
    );
    let request = server.finish();
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.x_api_key, "sk-test");
    assert_eq!(request.anthropic_version, "2023-06-01");
}

#[tokio::test]
async fn models_proxy_passes_through_original_user_agent_when_unconfigured() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "");

    let upstream = open_models_proxy_request(Some("Original-Codex-UA/1.0"))
        .await
        .unwrap();
    assert_eq!(upstream.status_code, 200);

    let request = server.finish();
    assert_eq!(request.user_agent, "Original-Codex-UA/1.0");
}

#[test]
fn chat_request_strips_web_search_when_no_mcp_fallback() {
    // Chat 路径无 MCP 搜索 fallback 时,剥离 web_search 避免模型调用后死循环。
    let converted = responses_to_chat_completions(json!({
        "model": "glm-5.2",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "search the web" }]
            }
        ],
        "tools": [
            {
                "type": "function",
                "name": "lookup",
                "description": "Lookup",
                "parameters": { "type": "object" }
            },
            { "type": "web_search_preview" }
        ]
    }))
    .unwrap();

    let tools = converted["tools"].as_array().expect("tools present");
    // web_search 被剥离,只剩 lookup。
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["function"]["name"], "lookup");
}

#[test]
fn chat_request_keeps_web_search_when_mcp_fallback_available() {
    // Chat 路径有 MCP 搜索 fallback(tavily)时,保留 web_search function,
    // 响应方向(tool_call_added_item)会把模型对 web_search 的调用改写成 tavily。
    let converted = responses_to_chat_completions(json!({
        "model": "glm-5.2",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "search the web" }]
            }
        ],
        "tools": [
            { "type": "web_search_preview" },
            tavily_namespace_tool()
        ]
    }))
    .unwrap();

    let tools = converted["tools"].as_array().expect("tools present");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| {
            tool.get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
        })
        .collect();
    // web_search function 保留(响应方向会改写),tavily 扁平化为 mcp__tavily__tavily_search。
    assert!(names.contains(&"web_search_preview"));
    assert!(names.contains(&"mcp__tavily__tavily_search"));
}

fn tavily_namespace_tool() -> Value {
    json!({
        "type": "namespace",
        "name": "mcp__tavily",
        "description": "Tavily web search MCP",
        "tools": [{
            "type": "function",
            "name": "tavily_search",
            "description": "Search the web for current information.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }]
    })
}

fn pal_namespace_tool() -> Value {
    json!({
        "type": "namespace",
        "name": "mcp__pal",
        "description": "PAL MCP",
        "tools": [{
            "type": "function",
            "name": "version",
            "description": "Return PAL MCP version.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }]
    })
}

fn write_mixed_relay_settings(settings_dir: &Path, base_url: &str) {
    write_mixed_relay_settings_with_system_prompt(settings_dir, base_url, "");
}

fn write_mixed_relay_settings_with_system_prompt(
    settings_dir: &Path,
    base_url: &str,
    system_prompt: &str,
) {
    let settings = json!({
        "relayProfiles": [{
            "id": "mixed",
            "name": "Mixed",
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "protocol": "responses",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [
                {
                    "requestModel": "gpt-responses",
                    "protocol": "responses",
                    "contextWindow": "200000"
                },
                {
                    "requestModel": "gpt-chat",
                    "protocol": "chatCompletions",
                    "contextWindow": "200000"
                },
                {
                    "requestModel": "claude-sonnet-4",
                    "protocol": "anthropic",
                    "contextWindow": "200000"
                }
            ],
            "systemPromptOverride": system_prompt
        }],
        "activeRelayId": "mixed"
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
}

fn write_chat_relay_settings(settings_dir: &Path, base_url: &str, user_agent: &str) {
    let settings = json!({
        "relayProfiles": [{
            "id": "chat",
            "name": "Chat",
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "protocol": "chatCompletions",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [
                {
                    "requestModel": "gpt-5.5",
                    "protocol": "chatCompletions",
                    "contextWindow": "200000"
                }
            ],
            "userAgent": user_agent
        }],
        "activeRelayId": "chat"
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
}

fn write_anthropic_relay_settings(settings_dir: &Path, base_url: &str) {
    let settings = json!({
        "relayProfiles": [{
            "id": "anthropic",
            "name": "Anthropic",
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "protocol": "anthropic",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [
                {
                    "requestModel": "claude-listed",
                    "protocol": "anthropic",
                    "contextWindow": "200000"
                }
            ]
        }],
        "activeRelayId": "anthropic"
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
}

struct SettingsPathGuard {
    previous: Option<PathBuf>,
}

fn settings_path_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

impl SettingsPathGuard {
    fn set(path: PathBuf) -> Self {
        let previous = codex_elves_core::paths::set_settings_path_for_tests(Some(path));
        Self { previous }
    }
}

impl Drop for SettingsPathGuard {
    fn drop(&mut self) {
        codex_elves_core::paths::set_settings_path_for_tests(self.previous.take());
    }
}

struct ChatServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<ChatRequest>>,
}

impl ChatServer {
    fn finish(self) -> ChatRequest {
        self.handle.join().unwrap().into_iter().next().unwrap()
    }
}

struct ChatRequest {
    path: String,
    user_agent: String,
    x_api_key: String,
    anthropic_version: String,
    body: String,
}

fn spawn_chat_server() -> ChatServer {
    spawn_chat_server_with_response(
        r#"{"id":"chatcmpl-test","object":"chat.completion","choices":[]}"#,
    )
}

fn spawn_chat_server_with_response(response_body: impl Into<String>) -> ChatServer {
    spawn_chat_server_with_responses(vec![response_body.into()])
}

fn spawn_chat_server_with_responses(response_bodies: Vec<String>) -> ChatServer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for response_body in response_bodies {
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < std::time::Duration::from_secs(5),
                            "test upstream did not receive a request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept test request: {error}"),
                }
            };
            let mut request_bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => std::thread::sleep(std::time::Duration::from_millis(10)),
                    Ok(bytes) => {
                        request_bytes.extend_from_slice(&buffer[..bytes]);
                        let request = String::from_utf8_lossy(&request_bytes);
                        if let Some(header_end) = request.find("\r\n\r\n") {
                            let content_length = request
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':').and_then(|(name, value)| {
                                        name.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().ok())
                                            .flatten()
                                    })
                                })
                                .unwrap_or(0);
                            if request_bytes.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to read test request: {error}"),
                }
            }
            let request = String::from_utf8_lossy(&request_bytes).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_string();
            let header_value = |header_name: &str| {
                request.lines().find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case(header_name)
                            .then(|| value.trim().to_string())
                    })
                })
            };
            let user_agent = header_value("user-agent").unwrap_or_default();
            let x_api_key = header_value("x-api-key").unwrap_or_default();
            let anthropic_version = header_value("anthropic-version").unwrap_or_default();
            let request_body = request
                .split_once("\r\n\r\n")
                .map(|(_, body)| body.to_string())
                .unwrap_or_default();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(ChatRequest {
                path,
                user_agent,
                x_api_key,
                anthropic_version,
                body: request_body,
            });
        }
        requests
    });
    ChatServer { base_url, handle }
}
