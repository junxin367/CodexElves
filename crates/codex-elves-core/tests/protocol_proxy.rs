use codex_elves_core::protocol_proxy::{
    ChatSseToResponsesConverter, anthropic_message_to_response_with_request,
    anthropic_messages_url, anthropic_sse_to_responses_sse_with_request,
    chat_completion_to_response, chat_completion_to_response_with_request, chat_completions_url,
    chat_sse_to_responses_sse, chat_sse_to_responses_sse_with_request,
    is_chat_completions_proxy_path, is_models_proxy_path, is_responses_proxy_path, models_url,
    open_chat_completions_proxy_request, open_models_proxy_request, open_responses_proxy_request,
    open_responses_proxy_request_with_settings, responses_error_from_upstream,
    responses_to_anthropic_messages, responses_to_chat_completions,
    send_upstream_request_with_header_timeout, upstream_header_timeout, upstream_http_client,
    upstream_stream_header_timeout,
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
                "cache_read_input_tokens": 2
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
    assert_eq!(deepseek["reasoning_effort"], "high");

    let openrouter = responses_to_chat_completions(json!({
        "model": "openrouter/deepseek/deepseek-r1",
        "reasoning": { "effort": "max" },
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(openrouter["reasoning"]["effort"], "high");
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
            }
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
    assert!(names.contains(&"web_search"));
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
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

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
    assert!(converted.contains("data: [DONE]"));
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
fn upstream_header_timeout_is_bounded_for_hung_providers() {
    assert!(upstream_header_timeout() >= Duration::from_secs(30));
    assert!(upstream_header_timeout() <= Duration::from_secs(60));
    assert!(upstream_stream_header_timeout() >= Duration::from_secs(120));
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
async fn responses_proxy_rejects_models_missing_from_protocol_lists() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    write_mixed_relay_settings(temp.path(), "https://example.test/v1");

    let error = open_responses_proxy_request(
        r#"{"model":"gpt-unlisted","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .err()
    .unwrap();

    assert!(
        error
            .to_string()
            .contains("未配置到 Responses API、Chat Completions 或 Anthropic 模型列表")
    );
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

fn write_mixed_relay_settings(settings_dir: &Path, base_url: &str) {
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
            ]
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
    handle: thread::JoinHandle<ChatRequest>,
}

impl ChatServer {
    fn finish(self) -> ChatRequest {
        self.handle.join().unwrap()
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
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
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
        let response_body = r#"{"id":"chatcmpl-test","object":"chat.completion","choices":[]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).unwrap();
        ChatRequest {
            path,
            user_agent,
            x_api_key,
            anthropic_version,
            body: request_body,
        }
    });
    ChatServer { base_url, handle }
}
