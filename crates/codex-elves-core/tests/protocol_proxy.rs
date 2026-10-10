use codex_elves_core::layered_compaction::{
    DEFAULT_RETAIN_TOKENS, MIN_RETAIN_TOKENS,
    rewrite_remote_compaction_v2_response_with_layered_compaction,
};
use codex_elves_core::protocol_proxy::{
    AnthropicSseToResponsesConverter, ChatSseToResponsesConverter, UpstreamResponseProtocol,
    anthropic_message_to_response_with_request, anthropic_messages_url,
    anthropic_sse_to_responses_sse_with_request, apply_continue_thinking_to_responses_stream,
    apply_continue_thinking_to_responses_stream_with_request_context, chat_completion_to_response,
    chat_completion_to_response_with_request, chat_completions_url, chat_sse_to_responses_sse,
    chat_sse_to_responses_sse_with_request,
    clear_anthropic_reasoning_compatibility_cache_for_tests, handle_responses_proxy_request,
    is_chat_completions_proxy_path, is_models_proxy_path, is_responses_proxy_path, models_url,
    open_chat_completions_proxy_request, open_models_proxy_request, open_responses_proxy_request,
    open_responses_proxy_request_with_settings,
    open_responses_proxy_request_with_settings_and_request_context, responses_error_from_upstream,
    responses_to_anthropic_messages, responses_to_chat_completions,
    send_upstream_request_with_header_timeout, stream_idle_timeout_for_reasoning_effort,
    stream_idle_timeout_for_request, stream_idle_timeout_ms_for_reasoning_effort,
    supported_reasoning_efforts_for_model, upstream_error_is_timeout, upstream_http_client,
    upstream_models_header_timeout,
};
use codex_elves_core::request_headers::RequestContext;
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

mod protocol_stream_integrity_review {
    use super::*;

    fn request(compat: bool) -> Value {
        json!({
            "model":"claude-test","input":"test",
            "codex_elves_compat":{"textual_tool_calls":compat,"inline_citations":compat},
            "tools":[{"type":"function","name":"write_file","parameters":{
                "type":"object","properties":{"content":{"type":"string"}}
            }}]
        })
    }

    fn wire(blocks: &[Value], stop: &str) -> String {
        let mut events = vec![
            json!({"type":"message_start","message":{"id":"msg_integrity","model":"claude-test"}}),
        ];
        for (index, block) in blocks.iter().enumerate() {
            events.push(json!({"type":"content_block_start","index":index,"content_block":block}));
            events.push(json!({"type":"content_block_stop","index":index}));
        }
        events.push(json!({"type":"message_delta","delta":{"stop_reason":stop}}));
        events.push(json!({"type":"message_stop"}));
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect()
    }

    fn responses(blocks: Vec<Value>, request: &Value) -> Vec<Value> {
        let direct = anthropic_message_to_response_with_request(
            json!({"id":"msg_integrity","model":"claude-test","content":blocks,"stop_reason":"end_turn"}),
            request,
        ).unwrap();
        let sse = anthropic_sse_to_responses_sse_with_request(&wire(&blocks, "end_turn"), request);
        let events = parse_response_sse_events(&sse);
        vec![direct, events.last().unwrap().data["response"].clone()]
    }

    fn text(response: &Value) -> String {
        response["output"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect()
    }

    #[test]
    fn truncated_tool_does_not_block_later_item_completion() {
        for finish in ["length", "content_filter"] {
            let chunks = [
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"cut","function":{"name":"f","arguments":"{}"}}]}}]}),
                json!({"choices":[{"delta":{"content":"kept answer"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":finish}]}),
            ];
            let sse = format!(
                "{}data: [DONE]\n\n",
                chunks
                    .iter()
                    .map(|v| format!("data: {v}\n\n"))
                    .collect::<String>()
            );
            let output = chat_sse_to_responses_sse(&sse);
            let events = parse_response_sse_events(&output);
            let done: Vec<_> = events
                .iter()
                .filter(|e| e.event == "response.output_item.done")
                .collect();
            assert_eq!(done.len(), 1, "{output}");
            assert_eq!(done[0].data["item"]["type"], "message");
            assert_eq!(done[0].data["item"]["content"][0]["text"], "kept answer");
            assert!(
                !events
                    .iter()
                    .any(|e| e.event == "response.function_call_arguments.done")
            );
        }
    }

    #[test]
    fn text_tools_require_explicit_compatibility_and_ignore_code_examples() {
        let xml =
            "<invoke name=\"write_file\"><parameter name=\"content\">demo</parameter></invoke>";
        for response in responses(vec![json!({"type":"text","text":xml})], &request(false)) {
            assert_eq!(text(&response), xml);
            assert!(
                response["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|i| i["type"] == "message")
            );
        }
        for example in [format!("```xml\n{xml}\n```"), format!("`{xml}`")] {
            for response in responses(vec![json!({"type":"text","text":example})], &request(true)) {
                assert_eq!(text(&response), example);
                assert!(
                    response["output"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|i| i["type"] == "message")
                );
            }
        }
        let mut disabled = request(true);
        disabled["tool_choice"] = json!("none");
        for response in responses(vec![json!({"type":"text","text":xml})], &disabled) {
            assert_eq!(text(&response), xml);
        }
        for response in responses(vec![json!({"type":"text","text":xml})], &request(true)) {
            assert_eq!(response["output"][0]["type"], "function_call");
        }
    }

    #[test]
    fn text_tool_string_parameters_preserve_whitespace() {
        for content in ["    print(\"x\")\n", " \r\n ", "\n\ntext\t"] {
            let xml = format!(
                "<invoke name=\"write_file\"><parameter name=\"content\">{content}</parameter></invoke>"
            );
            for response in responses(vec![json!({"type":"text","text":xml})], &request(true)) {
                let args: Value =
                    serde_json::from_str(response["output"][0]["arguments"].as_str().unwrap())
                        .unwrap();
                assert_eq!(args["content"], content);
            }
        }
    }

    #[test]
    fn html_cite_code_is_preserved_and_wrapper_cleanup_is_opt_in() {
        for raw in [
            "<cite>Book</cite>",
            "```html\n<cite>Book</cite>\n```",
            "`<cite>Book</cite>`",
        ] {
            for response in responses(vec![json!({"type":"text","text":raw})], &request(false)) {
                assert_eq!(text(&response), raw);
            }
            let chat = chat_completion_to_response_with_request(
                json!({"choices":[{"message":{"content":raw},"finish_reason":"stop"}]}),
                &request(false),
            )
            .unwrap();
            assert_eq!(text(&chat), raw);
            if raw.starts_with('`') {
                for response in responses(vec![json!({"type":"text","text":raw})], &request(true)) {
                    assert_eq!(text(&response), raw);
                }
            }
        }
        for response in responses(
            vec![json!({"type":"text","text":"a<cite>Book</cite>b"})],
            &request(true),
        ) {
            assert_eq!(text(&response), "aBookb");
        }
    }

    #[test]
    fn citations_preserve_sources_and_raw_roundtrip() {
        let citation = json!({"type":"web_search_result_location","url":"https://example.com/source",
            "title":"Source","encrypted_index":"opaque-location","cited_text":"Evidence"});
        for response in responses(
            vec![json!({"type":"text","text":"中🙂 Evidence","citations":[citation]})],
            &request(false),
        ) {
            let part = &response["output"][0]["content"][0];
            let annotation = &part["annotations"][0];
            assert_eq!(annotation["type"], "url_citation");
            assert_eq!(annotation["url"], citation["url"]);
            assert_eq!(annotation["title"], citation["title"]);
            assert_eq!(annotation["start_index"], 3);
            assert_eq!(annotation["end_index"], 11);
            let replay = responses_to_anthropic_messages(json!({
                "model":"claude-test","input":[{"role":"user","content":"test"},response["output"][0].clone()]
            })).unwrap();
            assert_eq!(
                replay["messages"][1]["content"][0]["citations"][0],
                citation
            );
        }
        let sse = format!(
            "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"c\"}}}}\n\n\
             data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"Evidence\"}}}}\n\n\
             data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"citations_delta\",\"citation\":{citation}}}}}\n\n\
             data: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
             data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}}}}\n\n\
             data: {{\"type\":\"message_stop\"}}\n\n"
        );
        let converted = anthropic_sse_to_responses_sse_with_request(&sse, &request(false));
        let events = parse_response_sse_events(&converted);
        assert_eq!(
            events.last().unwrap().data["response"]["output"][0]["content"][0]["annotations"][0]["url"],
            citation["url"]
        );
    }

    #[test]
    fn server_tools_are_opaque_history_not_client_calls() {
        let blocks = vec![
            json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"q"}}),
            json!({"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[{"type":"web_search_result","url":"https://example.com","encrypted_content":"opaque"}]}),
            json!({"type":"text","text":"answer"}),
        ];
        for response in responses(blocks.clone(), &request(false)) {
            let items = response["output"].as_array().unwrap();
            assert!(items.iter().all(|item| !matches!(
                item["type"].as_str(),
                Some("function_call" | "custom_tool_call" | "web_search_call")
            )));
            assert_eq!(text(&response), "answer");
            assert_eq!(items[0]["type"], "reasoning");
            assert!(
                items[0]["encrypted_content"]
                    .as_str()
                    .unwrap()
                    .starts_with("codex-elves-anthropic-content-v1:")
            );
            let mut history = vec![json!({"role":"user","content":"test"})];
            history.extend(items.iter().cloned());
            let replay =
                responses_to_anthropic_messages(json!({"model":"claude-test","input":history}))
                    .unwrap();
            assert_eq!(replay["messages"][1]["content"][0], blocks[0]);
            assert_eq!(replay["messages"][1]["content"][1], blocks[1]);
        }
    }

    #[test]
    fn compatibility_code_and_citations_survive_every_fragment_boundary() {
        let examples = [
            "```xml\n<invoke name=\"write_file\"><parameter name=\"content\">demo</parameter></invoke>\n```",
            "`<invoke name=\"write_file\"><parameter name=\"content\">demo</parameter></invoke>`",
            "~~~html\n<cite>HTML</cite>\n~~~",
            "``<cite>inline ` code</cite>``",
        ];
        for example in examples {
            let deltas: Vec<_> = example.chars().map(|ch| ch.to_string()).collect();
            let upstream = anthropic_sse_from_text_deltas(&deltas);
            for width in [1, 7, 4096] {
                let mut converter = AnthropicSseToResponsesConverter::with_request(&request(true));
                let mut output = Vec::new();
                for bytes in upstream.as_bytes().chunks(width) {
                    output.extend(converter.push_bytes(bytes));
                }
                output.extend(converter.finish());
                let rendered = String::from_utf8(output).unwrap();
                assert_eq!(collect_stream_output_text(&rendered), example);
                assert!(!parse_response_sse_events(&rendered).iter().any(|event| {
                    event.event == "response.output_item.done"
                        && event.data["item"]["type"] == "function_call"
                }));
            }
        }
        let first = json!({"type":"web_search_result_location","url":"https://example.com/1","title":"1","cited_text":"甲"});
        let second = json!({"type":"web_search_result_location","url":"https://example.com/2","title":"2","cited_text":"乙","encrypted_index":"second"});
        let streamed = anthropic_sse_to_responses_sse_with_request(
            &wire(
                &[
                    json!({"type":"text","text":"甲","citations":[first]}),
                    json!({"type":"text","text":"🙂乙","citations":[second]}),
                ],
                "end_turn",
            ),
            &request(false),
        );
        let events = parse_response_sse_events(&streamed);
        let annotations =
            &events.last().unwrap().data["response"]["output"][0]["content"][0]["annotations"];
        assert_eq!(annotations[0]["start_index"], 0);
        assert_eq!(annotations[0]["end_index"], 1);
        assert_eq!(annotations[1]["start_index"], 2);
        assert_eq!(annotations[1]["end_index"], 3);
    }

    #[test]
    fn server_fetch_and_client_tools_replay_without_changing_execution_owner() {
        let blocks = vec![
            json!({"type":"server_tool_use","id":"srvtoolu_fetch","name":"web_fetch","input":{"url":"https://example.com"}}),
            json!({"type":"web_fetch_tool_result","tool_use_id":"srvtoolu_fetch","content":{"type":"web_fetch_result","url":"https://example.com","retrieved_at":"2026-10-05T00:00:00Z","content":{"type":"document","source":{"type":"text","media_type":"text/plain","data":"source"}}}}),
            json!({"type":"tool_use","id":"client_write","name":"write_file","input":{"content":"client"}}),
        ];
        for response in responses(blocks.clone(), &request(false)) {
            let items = response["output"].as_array().unwrap();
            let calls: Vec<_> = items
                .iter()
                .filter(|item| item["type"] == "function_call")
                .collect();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0]["call_id"], "client_write");
            let mut history = vec![json!({"role":"user","content":"test"})];
            history.extend(items.iter().cloned());
            history.push(
                json!({"type":"function_call_output","call_id":"client_write","output":"done"}),
            );
            let replay = responses_to_anthropic_messages(json!({
                "model":"claude-test","input":history,"tools":request(false)["tools"]
            }))
            .unwrap();
            assert_eq!(replay["messages"][1]["content"][0], blocks[0]);
            assert_eq!(replay["messages"][1]["content"][1], blocks[1]);
            assert_eq!(replay["messages"][1]["content"][2], blocks[2]);
            assert_eq!(
                replay["messages"][2]["content"][0]["tool_use_id"],
                "client_write"
            );
        }
        // 服务端挂起等待客户端结果时，没有对应 server result 也必须保留调用。
        let pending = vec![blocks[0].clone(), blocks[2].clone()];
        for response in responses(pending, &request(false)) {
            assert!(
                response["output"][0]["encrypted_content"]
                    .as_str()
                    .unwrap()
                    .contains("srvtoolu_fetch")
            );
        }
    }

    #[test]
    fn server_partial_json_and_truncation_never_create_client_calls_or_invalid_history() {
        for arguments in [r#"{"query":"complete"}"#, r#"{"query":"cut"#] {
            let events = vec![
                json!({"type":"message_start","message":{"id":"msg_server_delta","model":"claude-test"}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"srvtoolu_search","name":"web_search","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"partial answer"}}),
                json!({"type":"content_block_stop","index":1}),
                json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"}}),
                json!({"type":"message_stop"}),
            ];
            let upstream: String = events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect();
            let mut converter = AnthropicSseToResponsesConverter::with_request(&request(false));
            let mut output = Vec::new();
            for byte in upstream.as_bytes().chunks(1) {
                output.extend(converter.push_bytes(byte));
            }
            output.extend(converter.finish());
            let converted = String::from_utf8(output).unwrap();
            let output_events = parse_response_sse_events(&converted);
            let response = &output_events.last().unwrap().data["response"];
            assert_eq!(response["status"], "incomplete");
            assert_eq!(text(response), "partial answer");
            assert!(
                !output_events
                    .iter()
                    .any(|event| event.event == "response.function_call_arguments.done")
            );
            let done: Vec<_> = output_events
                .iter()
                .filter(|event| event.event == "response.output_item.done")
                .collect();
            assert!(
                done.iter()
                    .any(|event| event.data["item"]["type"] == "message")
            );
            let reasoning: Vec<_> = response["output"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "reasoning")
                .collect();
            if arguments.ends_with('}') {
                assert_eq!(reasoning.len(), 1);
                let payload = reasoning[0]["encrypted_content"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("codex-elves-anthropic-content-v1:")
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(payload).unwrap()["input"]["query"],
                    "complete"
                );
            } else {
                assert!(reasoning.is_empty());
            }
        }
    }

    #[test]
    fn namespace_identity_not_flattened_spelling_controls_tool_permissions() {
        for kind in ["function", "custom"] {
            let declared = json!({"type":kind,"name":"b__c","parameters":{"type":"object"}});
            for allowed in [false, true] {
                let wrong = json!({"type":kind,"namespace":"a__b","name":"c"});
                let choice = if allowed {
                    json!({"type":"allowed_tools","mode":"required","tools":[wrong]})
                } else {
                    wrong
                };
                let request = json!({"model":"claude-test","input":"test","tools":[
                    {"type":"namespace","name":"a","tools":[declared]}
                ],"tool_choice":choice});
                assert!(
                    responses_to_chat_completions(request.clone()).is_err(),
                    "{request}"
                );
                assert!(responses_to_anthropic_messages(request).is_err());
            }
        }
    }

    #[test]
    fn legacy_internal_function_choice_requires_exact_declared_identity() {
        for choice in [
            json!({"type":"function","name":"tool_search"}),
            json!({"type":"function","namespace":"","name":"tool_search"}),
            json!({"type":"function","function":{"namespace":"","name":"tool_search"}}),
        ] {
            let request = json!({"model":"claude-test","input":"test",
                "tools":[{"type":"custom","name":"tool_search"},{"type":"tool_search"}],
                "tool_choice":choice});
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anthropic = responses_to_anthropic_messages(request).unwrap();
            assert_eq!(chat["tool_choice"]["function"]["name"], "tool_search");
            assert_eq!(anthropic["tool_choice"]["name"], "tool_search");
        }
        for (tools, choice) in [
            (
                json!([{"type":"custom","name":"tool_search"}]),
                json!({"type":"function","name":"tool_search"}),
            ),
            (
                json!([{"type":"custom","name":"tool_search"},{"type":"tool_search"}]),
                json!({"type":"function","name":"tool_search_2"}),
            ),
            (
                json!([{"type":"tool_search"}]),
                json!({"type":"function","namespace":"other","name":"tool_search"}),
            ),
        ] {
            for allowed in [false, true] {
                let restriction = if allowed {
                    json!({"type":"allowed_tools","mode":"required","tools":[choice]})
                } else {
                    choice.clone()
                };
                let request = json!({"model":"claude-test","input":"test","tools":tools,"tool_choice":restriction});
                assert!(
                    responses_to_chat_completions(request.clone()).is_err(),
                    "{request}"
                );
                assert!(responses_to_anthropic_messages(request).is_err());
            }
        }
        let request = json!({"model":"claude-test","input":"test",
            "tools":[{"type":"function","name":"tool_search","parameters":{"type":"object"}},{"type":"tool_search"}],
            "tool_choice":{"type":"function","name":"tool_search"}});
        assert_eq!(
            responses_to_chat_completions(request.clone()).unwrap()["tool_choice"]["function"]["name"],
            "tool_search_2"
        );
        assert_eq!(
            responses_to_anthropic_messages(request).unwrap()["tool_choice"]["name"],
            "tool_search_2"
        );
    }

    #[test]
    fn internal_tool_names_do_not_capture_same_named_custom_or_function_tools() {
        for ordinary_kind in ["custom", "function"] {
            for internal_first in [false, true] {
                let ordinary = json!({"type":ordinary_kind,"name":"tool_search","parameters":{"type":"object"}});
                let internal = json!({"type":"tool_search"});
                let tools = if internal_first {
                    vec![internal, ordinary]
                } else {
                    vec![ordinary, internal]
                };
                let request = json!({
                    "model":"claude-test","tools":tools,"tool_choice":{"type":"tool_search"},
                    "input":[{"role":"user","content":"test"},
                        {"type":"tool_search_call","call_id":"search","arguments":{"query":"q"}},
                        {"type":"tool_search_output","call_id":"search","tools":[]}]
                });
                let chat = responses_to_chat_completions(request.clone()).unwrap();
                let anthropic = responses_to_anthropic_messages(request.clone()).unwrap();
                let names: Vec<_> = chat["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tool| tool["function"]["name"].as_str().unwrap())
                    .collect();
                assert!(names.contains(&"tool_search") && names.contains(&"tool_search_2"));
                assert_eq!(chat["tool_choice"]["function"]["name"], "tool_search");
                assert_eq!(
                    chat["messages"][1]["tool_calls"][0]["function"]["name"],
                    "tool_search"
                );
                assert_eq!(anthropic["tool_choice"]["name"], "tool_search");
                assert_eq!(
                    anthropic["messages"][1]["content"][0]["name"],
                    "tool_search"
                );
                for (alias, expected) in [
                    ("tool_search", "tool_search_call"),
                    (
                        "tool_search_2",
                        if ordinary_kind == "custom" {
                            "custom_tool_call"
                        } else {
                            "function_call"
                        },
                    ),
                ] {
                    let function = json!({"name":alias,"arguments":"{\"query\":\"q\"}"});
                    let response = chat_completion_to_response_with_request(json!({
                        "choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"c","function":function}]}}]
                    }), &request).unwrap();
                    assert_eq!(response["output"][0]["type"], expected);
                    let upstream = format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({
                            "choices":[{"finish_reason":"tool_calls","delta":{"tool_calls":[{"index":0,"id":"c","function":function}]}}]
                        })
                    );
                    let converted = chat_sse_to_responses_sse_with_request(&upstream, &request);
                    assert_eq!(
                        parse_response_sse_events(&converted).last().unwrap().data["response"]["output"]
                            [0]["type"],
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn legacy_function_history_and_textual_patch_arguments_use_actual_aliases() {
        let tools = json!([
            {"type":"custom","name":"apply_patch"},
            {"type":"function","name":"apply_patch_add_file","parameters":{"type":"object"}}
        ]);
        let request = json!({"model":"claude-test","tools":tools,"input":[
            {"role":"user","content":"test"},
            {"type":"tool_call","tool_use":{"id":"c","name":"apply_patch_add_file","input":{}}},
            {"type":"tool_result","call_id":"c","content":{"content":"done"}}
        ]});
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        assert_eq!(
            chat["messages"][1]["tool_calls"][0]["function"]["name"],
            "apply_patch_add_file_2"
        );
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            anthropic["messages"][1]["content"][0]["name"],
            "apply_patch_add_file_2"
        );
        for namespace in ["", "files"] {
            let patch = if namespace.is_empty() {
                json!({"type":"custom","name":"apply_patch"})
            } else {
                json!({"type":"namespace","name":namespace,"tools":[{"type":"custom","name":"apply_patch"}]})
            };
            let preferred = if namespace.is_empty() {
                "apply_patch_update_file"
            } else {
                "files__apply_patch_update_file"
            };
            let request = json!({"model":"claude-test","input":"test",
                "codex_elves_compat":{"textual_tool_calls":true},
                "tools":[{"type":"function","name":preferred,"parameters":{"type":"object"}},patch]
            });
            let xml = format!(
                "<invoke name=\"{preferred}_2\"><parameter name=\"path\">a</parameter><parameter name=\"hunks\">[{{\"lines\":[{{\"op\":\"add\",\"text\":\"new\"}}]}}]</parameter></invoke>"
            );
            for response in responses(vec![json!({"type":"text","text":xml})], &request) {
                assert_eq!(response["output"][0]["type"], "custom_tool_call");
                assert_eq!(response["output"][0]["name"], "apply_patch");
                assert!(
                    response["output"][0]["input"]
                        .as_str()
                        .unwrap()
                        .contains("+new")
                );
                if !namespace.is_empty() {
                    assert_eq!(response["output"][0]["namespace"], namespace);
                }
            }
        }
    }
}

// 旧供应商兼容用例显式启用，普通请求默认保持原始正文。
fn legacy_text_compat_request(request: &Value) -> Value {
    let mut request = request.clone();
    request["codex_elves_compat"] = json!({"textual_tool_calls":true,"inline_citations":true});
    request
}

fn anthropic_message_to_response_with_compat(
    body: Value,
    request: &Value,
) -> anyhow::Result<Value> {
    anthropic_message_to_response_with_request(body, &legacy_text_compat_request(request))
}

fn anthropic_sse_to_responses_sse_with_compat(input: &str, request: &Value) -> String {
    anthropic_sse_to_responses_sse_with_request(input, &legacy_text_compat_request(request))
}

mod protocol_history_integrity_review {
    use super::*;

    fn function(name: &str) -> Value {
        json!({"type":"function","name":name,"parameters":{"type":"object","properties":{}}})
    }

    fn chat_call(request: &Value, name: &str, arguments: Value) -> Value {
        chat_completion_to_response_with_request(
            json!({"id":"history","choices":[{"message":{"tool_calls":[{
                "id":"call_history","type":"function","function":{
                    "name":name,"arguments":arguments.to_string()
                }
            }]},"finish_reason":"tool_calls"}]}),
            request,
        )
        .unwrap()["output"][0]
            .clone()
    }

    #[test]
    fn additional_tools_restore_declarations_and_namespace_identity() {
        let request = json!({
            "model":"claude-test",
            "input":[
                {"type":"additional_tools","role":"system","tools":[
                    {"type":"namespace","name":"collaboration","tools":[function("spawn_agent")]},
                    {"type":"namespace","name":"files","tools":[{"type":"custom","name":"raw"}]}
                ]},
                {"role":"user","content":"使用工具"}
            ]
        });
        for anthropic in [false, true] {
            let upstream = if anthropic {
                responses_to_anthropic_messages(request.clone()).unwrap()
            } else {
                responses_to_chat_completions(request.clone()).unwrap()
            };
            assert_eq!(upstream["tools"].as_array().unwrap().len(), 2);
            assert!(
                !upstream["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["content"].is_null()),
                "工具声明不能变成空消息"
            );
        }
        let call = chat_call(&request, "collaboration__spawn_agent", json!({}));
        assert_eq!(call["namespace"], "collaboration");
        assert_eq!(call["encrypted_function_args"], json!([]));
        let custom = chat_call(&request, "files__raw", json!({"input":"原文"}));
        assert_eq!(custom["type"], "custom_tool_call");
        assert_eq!(custom["namespace"], "files");
    }

    #[test]
    fn restored_tool_declarations_use_the_same_catalog_for_responses_and_streams() {
        let request = json!({
            "model":"claude-test",
            "tools":[function("files__raw")],
            "input":[{"type":"compaction","encrypted_content":format!(
                "codex-elves-compaction-v3:{}", json!({
                    "summary":"历史",
                    "retained_tail":[
                        {"type":"additional_tools","tools":[
                            {"type":"namespace","name":"files","tools":[{"type":"custom","name":"raw"}]}
                        ]},
                        {"role":"user","content":"继续"}
                    ]
                })
            )}]
        });
        let mut selected = request.clone();
        selected["tool_choice"] = json!({"type":"custom","namespace":"files","name":"raw"});
        let upstream = responses_to_chat_completions(selected.clone()).unwrap();
        let alias = upstream["tool_choice"]["function"]["name"]
            .as_str()
            .unwrap();
        let call = chat_call(&selected, alias, json!({"input":"原文"}));
        assert_eq!(call["type"], "custom_tool_call");
        assert_eq!(call["name"], "raw");
        assert_eq!(call["namespace"], "files");
        let stream = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"id":"restored","choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":alias,"arguments":"{\"input\":\"原文\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})
        );
        let converted = chat_sse_to_responses_sse_with_request(&stream, &selected);
        let events = parse_response_sse_events(&converted);
        let done = events
            .iter()
            .find(|event| event.event == "response.output_item.done")
            .unwrap();
        assert_eq!(done.data["item"]["namespace"], "files");
        assert_eq!(done.data["item"]["name"], "raw");
    }

    #[test]
    fn colliding_and_long_tool_names_keep_all_four_mapping_directions() {
        let long_namespace = "n".repeat(50);
        let long_name = "f".repeat(50);
        let tools = json!([
            {"type":"namespace","name":"a","tools":[function("b__c")]},
            {"type":"namespace","name":"a__b","tools":[function("c")]},
            function("a__b__c"),
            {"type":"namespace","name":long_namespace,"tools":[function(&long_name)]}
        ]);
        let identities = [
            ("a", "b__c"),
            ("a__b", "c"),
            ("", "a__b__c"),
            (long_namespace.as_str(), long_name.as_str()),
        ];
        let mut aliases = std::collections::BTreeSet::new();
        for (namespace, name) in identities {
            let request = json!({
                "model":"claude-test","tools":tools,
                "tool_choice":{"type":"function","namespace":namespace,"name":name},
                "input":[{"role":"user","content":"继续"},
                    {"type":"function_call","namespace":namespace,"name":name,
                     "call_id":"history","arguments":"{}"},
                    {"type":"function_call_output","call_id":"history","output":"完成"}]
            });
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let upstream_name = chat["tool_choice"]["function"]["name"].as_str().unwrap();
            assert!(upstream_name.len() <= 64);
            assert!(
                upstream_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            );
            assert!(aliases.insert(upstream_name.to_string()), "{chat}");
            assert_eq!(chat["tools"].as_array().unwrap().len(), 4);
            assert_eq!(
                chat["messages"][1]["tool_calls"][0]["function"]["name"],
                upstream_name
            );
            let call = chat_call(&request, upstream_name, json!({}));
            assert_eq!(call["name"], name);
            assert_eq!(call["namespace"].as_str().unwrap_or(""), namespace);
            let anthropic = responses_to_anthropic_messages(request).unwrap();
            assert_eq!(anthropic["tool_choice"]["name"], upstream_name);
            assert_eq!(
                anthropic["messages"][1]["content"][0]["name"],
                upstream_name
            );
        }
    }

    #[test]
    fn custom_proxy_aliases_do_not_shadow_real_functions() {
        let request = json!({
            "model":"claude-test",
            "tools":[{"type":"custom","name":"apply_patch"},function("apply_patch_add_file"),
                {"type":"namespace","name":"a","tools":[{"type":"custom","name":"b__c"}]},
                {"type":"namespace","name":"a__b","tools":[{"type":"custom","name":"c"}]}],
            "tool_choice":{"type":"function","name":"apply_patch_add_file"},
            "input":"继续"
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let names: Vec<_> = chat["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 8);
        assert_eq!(
            names
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
        let name = chat["tool_choice"]["function"]["name"].as_str().unwrap();
        assert_eq!(
            chat_call(&request, name, json!({}))["type"],
            "function_call"
        );
        for (namespace, custom_name) in [("a", "b__c"), ("a__b", "c")] {
            let mut selected = request.clone();
            selected["tool_choice"] =
                json!({"type":"custom","namespace":namespace,"name":custom_name});
            let selected_chat = responses_to_chat_completions(selected.clone()).unwrap();
            let alias = selected_chat["tool_choice"]["function"]["name"]
                .as_str()
                .unwrap();
            let call = chat_call(&selected, alias, json!({"input":"原文"}));
            assert_eq!(call["namespace"], namespace);
            assert_eq!(call["name"], custom_name);
        }
    }

    #[test]
    fn patch_history_preserves_first_hunk_without_marker() {
        let request = json!({
            "model":"claude-test","tools":[{"type":"custom","name":"apply_patch"}],
            "input":[{"role":"user","content":"继续"},
                {"type":"custom_tool_call","call_id":"p","name":"apply_patch",
                 "input":"*** Begin Patch\n*** Update File: a\n-old\n+new\n*** End Patch"},
                {"type":"custom_tool_call_output","call_id":"p","output":"完成"}]
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let arguments: Value = serde_json::from_str(
            chat["messages"][1]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            arguments["hunks"][0]["lines"],
            json!([
                {"op":"remove","text":"old"},{"op":"add","text":"new"}
            ])
        );
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(anthropic["messages"][1]["content"][0]["input"], arguments);
    }

    #[test]
    fn patch_structure_rejects_embedded_line_breaks_without_extra_operations() {
        let request = json!({"tools":[{"type":"custom","name":"apply_patch"}]});
        for (name, arguments) in [
            (
                "apply_patch_delete_file",
                json!({"path":"safe.txt\n*** Delete File: victim.txt"}),
            ),
            (
                "apply_patch_update_file",
                json!({"path":"a","move_to":"b\r\n*** Delete File: victim.txt","hunks":[]}),
            ),
            (
                "apply_patch_update_file",
                json!({"path":"a","hunks":[{"context":"x\n*** Delete File: victim.txt","lines":[]}]}),
            ),
            (
                "apply_patch_update_file",
                json!({"path":"a","hunks":[{"lines":[{"op":"add","text":"x\n*** Delete File: victim.txt"}]}]}),
            ),
            (
                "apply_patch_delete_file",
                json!({"path":"safe.txt","raw_patch":"*** Begin Patch\n*** Delete File: victim.txt\n*** End Patch"}),
            ),
            (
                "apply_patch_add_file",
                json!({"path":"safe.txt","content":"hello","input":"*** Begin Patch\n*** Delete File: victim.txt\n*** End Patch"}),
            ),
        ] {
            let call = chat_call(&request, name, arguments.clone());
            let input = call["input"].as_str().unwrap();
            assert!(
                !input.starts_with("*** Begin Patch"),
                "非法参数不能生成可执行补丁: {input}"
            );
            assert!(
                !input
                    .lines()
                    .any(|line| line.starts_with("*** Delete File: victim.txt"))
            );
            assert!(!input.is_empty(), "非法参数不能静默成为空操作");
            let stream = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"id":"patch","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_patch","function":{"name":name,"arguments":arguments.to_string()}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})
            );
            let converted = chat_sse_to_responses_sse_with_request(&stream, &request);
            let events = parse_response_sse_events(&converted);
            let done = events
                .iter()
                .find(|event| event.event == "response.custom_tool_call_input.done")
                .unwrap();
            assert_eq!(done.data["input"], input);
        }
    }

    #[test]
    fn raw_custom_patch_shaped_input_is_not_reinterpreted() {
        let patch = "*** Begin Patch\n*** Add File: note\n+hello\n*** End Patch";
        let request = json!({
            "model":"claude-test",
            "tools":[{"type":"namespace","name":"archive","tools":[{"type":"custom","name":"raw"}]}],
            "input":[{"role":"user","content":"继续"},
                {"type":"custom_tool_call","namespace":"archive","name":"raw","call_id":"c","input":patch},
                {"type":"custom_tool_call_output","call_id":"c","output":"保存"}]
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let call = &chat["messages"][1]["tool_calls"][0]["function"];
        assert_eq!(call["name"], "archive__raw");
        assert_eq!(
            serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap(),
            json!({"input":patch})
        );
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            anthropic["messages"][1]["content"][0]["name"],
            "archive__raw"
        );
        assert_eq!(
            anthropic["messages"][1]["content"][0]["input"],
            json!({"input":patch})
        );
    }

    #[test]
    fn claude_does_not_receive_foreign_ciphertext_as_a_thinking_signature() {
        let request = json!({
            "model":"claude-test","input":[{"role":"user","content":"继续"},
                {"type":"reasoning","id":"rs_native","summary":[{"type":"summary_text","text":"历史摘要"}],
                 "encrypted_content":"opaque-openai-reasoning"},
                {"type":"reasoning","reasoning_content":"Chat 推理但没有签名"},
                {"role":"assistant","content":"历史回答"},{"role":"user","content":"继续"}]
        });
        let converted = responses_to_anthropic_messages(request).unwrap();
        let text = converted["messages"].to_string();
        assert!(!text.contains("opaque-openai-reasoning"));
        assert!(!text.contains("\"type\":\"thinking\""));
        assert!(text.contains("历史摘要"));
        assert!(text.contains("Chat 推理但没有签名"));
    }

    #[test]
    fn new_anthropic_signatures_are_enveloped_and_legacy_bridge_still_replays() {
        let request = json!({"model":"claude-test","input":"继续"});
        let block = json!({"type":"thinking","thinking":"原始推理","signature":"real-signature"});
        let response = anthropic_message_to_response_with_request(
            json!({"id":"msg_origin","content":[block.clone(),{"type":"text","text":"回答"}],"stop_reason":"end_turn"}),
            &request,
        ).unwrap();
        assert!(
            response["output"][0]["encrypted_content"]
                .as_str()
                .unwrap()
                .starts_with("codex-elves-anthropic-thinking-v1:")
        );
        for reasoning in [
            response["output"][0].clone(),
            json!({"type":"reasoning","reasoning_content":"原始推理","encrypted_content":"real-signature"}),
        ] {
            let replay = responses_to_anthropic_messages(json!({
                "model":"claude-test","input":[{"role":"user","content":"继续"},reasoning,{"role":"assistant","content":"回答"}]
            })).unwrap();
            assert_eq!(replay["messages"][1]["content"][0], block);
        }
    }
}

mod protocol_request_contract_review {
    use super::*;

    fn function(name: &str) -> Value {
        json!({"type":"function","name":name,"parameters":{"type":"object","properties":{}}})
    }

    #[test]
    fn pdf_inputs_and_tool_results_remain_native_documents() {
        let documents = json!([
            {"type":"input_file","filename":"中文.pdf","file_data":"data:application/pdf;base64,JVBERi0xLjcK"},
            {"type":"input_file","file_url":"https://example.test/report.pdf"}
        ]);
        let converted = responses_to_anthropic_messages(json!({
            "model":"claude-test",
            "input":[
                {"role":"user","content":documents},
                {"type":"function_call","name":"read","call_id":"c","arguments":"{}"},
                {"type":"function_call_output","call_id":"c","output":documents}
            ]
        }))
        .unwrap();
        let expected = json!([
            {"type":"document","title":"中文.pdf","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0xLjcK"}},
            {"type":"document","source":{"type":"url","url":"https://example.test/report.pdf"}}
        ]);
        assert_eq!(converted["messages"][0]["content"], expected);
        assert_eq!(converted["messages"][2]["content"][0]["content"], expected);
    }

    #[test]
    fn unsupported_anthropic_file_sources_return_errors_instead_of_text() {
        for part in [
            json!({"type":"input_file","file_id":"file_pdf"}),
            json!({"type":"input_file","filename":"audio.wav","file_data":"data:audio/wav;base64,YQ=="}),
            json!({"type":"input_file","filename":"missing.pdf"}),
        ] {
            let result = responses_to_anthropic_messages(json!({
                "model":"claude-test","input":[{"role":"user","content":[part]}]
            }));
            assert!(result.is_err(), "无法解析的文件不可变成文本: {result:?}");
            assert!(result.unwrap_err().to_string().contains("input_file"));
        }
    }

    #[test]
    fn file_id_images_are_rejected_in_messages_and_tool_outputs() {
        for input in [
            json!([{"role":"user","content":[{"type":"input_image","file_id":"file_img"}]}]),
            json!([
                {"role":"user","content":"查看"},
                {"type":"function_call","name":"read","call_id":"c","arguments":"{}"},
                {"type":"function_call_output","call_id":"c","output":[{"type":"input_image","file_id":"file_img"}]}
            ]),
        ] {
            let request = json!({"model":"test","input":input});
            for result in [
                responses_to_chat_completions(request.clone()),
                responses_to_anthropic_messages(request),
            ] {
                assert!(result.is_err(), "图片内容不可静默丢失: {result:?}");
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("input_image.file_id")
                );
            }
        }
        let request = json!({"model":"test","input":[{"role":"user","content":[{
            "type":"input_image","file_id":"file_img","image_url":"https://example.test/image.png"
        }]}]});
        assert!(responses_to_chat_completions(request.clone()).is_ok());
        assert!(responses_to_anthropic_messages(request).is_ok());
    }

    #[test]
    fn unresolved_server_history_references_fail_before_conversion() {
        for request in [
            json!({"model":"test","previous_response_id":"resp_old","input":"继续"}),
            json!({"model":"test","input":[{"type":"item_reference","id":"msg_old"},{"role":"user","content":"继续"}]}),
            json!({"model":"test","conversation":{"id":"conv_old"},"input":"继续"}),
        ] {
            assert!(responses_to_chat_completions(request.clone()).is_err());
            assert!(responses_to_anthropic_messages(request).is_err());
        }
        // conversation 字符串是现有聚合路由的绑定标识，不能删除这个兼容路径。
        let request = json!({
            "model":"test","conversation":"local-binding","previous_response_id":null,
            "input":[{"role":"user","content":"完整历史"}]
        });
        assert!(responses_to_chat_completions(request.clone()).is_ok());
        assert!(responses_to_anthropic_messages(request).is_ok());
    }

    #[test]
    fn restored_compaction_history_is_validated_after_expansion() {
        for tail in [
            json!([{"type":"item_reference","id":"msg_old"}]),
            json!([{"role":"user","content":[{"type":"input_image","file_id":"file_img"}]}]),
        ] {
            let request = json!({"model":"test","input":[{
                "type":"compaction",
                "encrypted_content":format!("codex-elves-compaction-v3:{}", json!({
                    "summary":"历史摘要","retained_tail":tail
                }))
            }]});
            assert!(responses_to_chat_completions(request.clone()).is_err());
            assert!(responses_to_anthropic_messages(request).is_err());
        }
    }

    #[test]
    fn allowed_tools_preserve_the_permitted_subset_and_required_mode() {
        let request = json!({
            "model":"test","input":"读取",
            "tools":[function("read"),function("write"),{
                "type":"namespace","name":"files","tools":[function("list")]
            }],
            "tool_choice":{"type":"allowed_tools","mode":"required","tools":[
                {"type":"function","name":"read"},
                {"type":"function","namespace":"files","name":"list"}
            ]}
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        assert_eq!(chat["tools"].as_array().unwrap().len(), 3);
        assert_eq!(
            chat["tool_choice"],
            json!({
                "type":"allowed_tools","allowed_tools":{"mode":"required","tools":[
                    {"type":"function","function":{"name":"read"}},
                    {"type":"function","function":{"name":"files__list"}}
                ]}
            })
        );
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(anthropic["tool_choice"]["type"], "any");
        let names: Vec<_> = anthropic["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["read", "files__list"]);
    }

    #[test]
    fn invalid_allowed_tool_sets_cannot_fall_back_to_unrestricted_tools() {
        for choice in [
            json!({"type":"allowed_tools","mode":"required","tools":[{"type":"function","name":"missing"}]}),
            json!({"type":"allowed_tools","mode":"required","tools":[]}),
            json!({"type":"allowed_tools","mode":"invalid","tools":[{"type":"function","name":"read"}]}),
            json!({"type":"allowed_tools","mode":"auto","tools":[{"type":"file_search"}]}),
        ] {
            let request = json!({"model":"test","input":"读取","tools":[function("read")],"tool_choice":choice});
            assert!(responses_to_chat_completions(request.clone()).is_err());
            assert!(responses_to_anthropic_messages(request).is_err());
        }
    }

    fn custom_namespace_request() -> Value {
        json!({
            "model":"test","input":"执行",
            "tools":[{"type":"namespace","name":"editor","description":"编辑工具","tools":[
                {"type":"custom","name":"write","description":"原样写入文本"},
                {"type":"custom","name":"apply_patch"}
            ]}],
            "tool_choice":{"type":"custom","namespace":"editor","name":"write"}
        })
    }

    #[test]
    fn namespace_custom_tools_round_trip_names_inputs_choices_and_history() {
        let request = custom_namespace_request();
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        assert_eq!(chat["tools"][0]["function"]["name"], "editor__write");
        assert_eq!(chat["tool_choice"]["function"]["name"], "editor__write");
        assert!(
            chat["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "editor__apply_patch_batch")
        );
        let anthropic = responses_to_anthropic_messages(request.clone()).unwrap();
        assert_eq!(anthropic["tools"][0]["name"], "editor__write");
        assert_eq!(anthropic["tool_choice"]["name"], "editor__write");
        let input = "中文\n\"引号\"和路径 C:\\work";
        let chat_response = chat_completion_to_response_with_request(json!({
            "id":"chatcmpl_custom","choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{
                "id":"c","type":"function","function":{"name":"editor__write","arguments":json!({"input":input}).to_string()}
            }]}}]
        }), &request).unwrap();
        let anthropic_response = anthropic_message_to_response_with_request(json!({
            "id":"msg_custom","type":"message","role":"assistant","stop_reason":"tool_use",
            "content":[{"type":"tool_use","id":"c","name":"editor__write","input":{"input":input}}]
        }), &request).unwrap();
        for response in [chat_response, anthropic_response] {
            let call = &response["output"][0];
            assert_eq!(call["type"], "custom_tool_call");
            assert_eq!(call["name"], "write");
            assert_eq!(call["namespace"], "editor");
            assert_eq!(call["input"], input);
            let mut replay = request.clone();
            replay["input"] = json!([
                {"role":"user","content":"执行"},call,
                {"type":"custom_tool_call_output","call_id":"c","output":"ok"}
            ]);
            let chat_replay = responses_to_chat_completions(replay.clone()).unwrap();
            assert_eq!(
                chat_replay["messages"][1]["tool_calls"][0]["function"]["name"],
                "editor__write"
            );
            let arguments: Value = serde_json::from_str(
                chat_replay["messages"][1]["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(arguments["input"], input);
            let anthropic_replay = responses_to_anthropic_messages(replay).unwrap();
            assert_eq!(
                anthropic_replay["messages"][1]["content"][0]["name"],
                "editor__write"
            );
            assert_eq!(
                anthropic_replay["messages"][1]["content"][0]["input"]["input"],
                input
            );
        }
    }

    #[test]
    fn allowed_namespace_custom_patch_tools_include_all_proxy_actions() {
        let mut request = custom_namespace_request();
        request["tool_choice"] = json!({"type":"allowed_tools","mode":"auto","tools":[{
            "type":"custom","namespace":"editor","name":"apply_patch"
        }]});
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let allowed = chat["tool_choice"]["allowed_tools"]["tools"]
            .as_array()
            .unwrap();
        assert_eq!(allowed.len(), 5);
        assert!(allowed.iter().all(|tool| {
            tool["function"]["name"]
                .as_str()
                .unwrap()
                .starts_with("editor__apply_patch_")
        }));
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(anthropic["tools"].as_array().unwrap().len(), 5);
        assert_eq!(anthropic["tool_choice"]["type"], "auto");
    }

    #[test]
    fn namespace_custom_stream_events_and_patch_history_keep_the_namespace() {
        let request = custom_namespace_request();
        let chat_wire = [
            json!({"id":"chatcmpl_custom","choices":[{"index":0,"delta":{"tool_calls":[{
                "index":0,"id":"c","type":"function","function":{"name":"editor__write","arguments":"{\"input\":\"你好\"}"}
            }]}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>() + "data: [DONE]\n\n";
        let anthropic_wire = [
            json!({"type":"message_start","message":{"id":"msg_custom","type":"message","role":"assistant","content":[]}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"c","name":"editor__write","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"input\":\"你好\"}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
        for output in [
            chat_sse_to_responses_sse_with_request(&chat_wire, &request),
            anthropic_sse_to_responses_sse_with_request(&anthropic_wire, &request),
        ] {
            let events: Vec<Value> = output
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|data| serde_json::from_str(data).ok())
                .collect();
            for event_type in ["response.output_item.added", "response.output_item.done"] {
                let event = events
                    .iter()
                    .find(|event| event["type"] == event_type)
                    .unwrap();
                assert_eq!(event["item"]["type"], "custom_tool_call");
                assert_eq!(event["item"]["namespace"], "editor");
                assert_eq!(event["item"]["name"], "write");
            }
            let completed = events
                .iter()
                .find(|event| event["type"] == "response.completed")
                .unwrap();
            assert_eq!(completed["response"]["output"][0]["input"], "你好");
        }
        let mut replay = request;
        replay["input"] = json!([
            {"role":"user","content":"修改文件"},
            {"type":"custom_tool_call","namespace":"editor","name":"apply_patch","call_id":"p",
                "input":"*** Begin Patch\n*** Add File: a.txt\n+你好\n*** End Patch"},
            {"type":"custom_tool_call_output","call_id":"p","output":"ok"}
        ]);
        replay["tool_choice"] = json!({"type":"custom","namespace":"editor","name":"apply_patch"});
        let chat = responses_to_chat_completions(replay.clone()).unwrap();
        assert_eq!(
            chat["tool_choice"]["function"]["name"],
            "editor__apply_patch_batch"
        );
        assert_eq!(
            chat["messages"][1]["tool_calls"][0]["function"]["name"],
            "editor__apply_patch_add_file"
        );
        let anthropic = responses_to_anthropic_messages(replay).unwrap();
        assert_eq!(
            anthropic["tool_choice"]["name"],
            "editor__apply_patch_batch"
        );
        assert_eq!(
            anthropic["messages"][1]["content"][0]["name"],
            "editor__apply_patch_add_file"
        );
    }
}

mod anthropic_image_limits {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use image::{DynamicImage, GenericImageView, ImageFormat};
    use std::io::Cursor;

    fn image_url(width: u32, height: u32, format: ImageFormat) -> String {
        let image = if format == ImageFormat::Jpeg {
            DynamicImage::new_rgb8(width, height)
        } else {
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                width,
                height,
                image::Rgba([20, 40, 60, 128]),
            ))
        };
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        format!(
            "data:{};base64,{}",
            format.to_mime_type(),
            STANDARD.encode(bytes.into_inner())
        )
    }

    fn request_with_images(count: usize, last_url: &str) -> Value {
        let small = image_url(2, 2, ImageFormat::Png);
        let mut historical: Vec<_> = (0..count - 1)
            .map(|_| json!({"type":"input_image","image_url":small}))
            .collect();
        if historical.is_empty() {
            historical.push(json!({"type":"input_text","text":"查看图片"}));
        }
        json!({
            "model":"claude-test",
            "input":[
                {"type":"message","role":"user","content":historical},
                {"type":"function_call","name":"view_image","call_id":"call_image","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_image","output":[
                    {"type":"input_text","text":"图片已读取"},
                    {"type":"input_image","image_url":last_url}
                ]}
            ]
        })
    }

    fn last_source(request: &Value) -> &Value {
        let messages = request["messages"].as_array().unwrap();
        &messages.last().unwrap()["content"][0]["content"][1]["source"]
    }

    fn decoded_source(source: &Value) -> DynamicImage {
        let bytes = STANDARD.decode(source["data"].as_str().unwrap()).unwrap();
        image::load_from_memory(&bytes).unwrap()
    }

    #[test]
    fn threshold_counts_history_and_tool_images_across_messages() {
        let url = image_url(1099, 2048, ImageFormat::Jpeg);
        for count in [20, 21, 33] {
            let source = request_with_images(count, &url);
            let converted = responses_to_anthropic_messages(source).unwrap();
            let result = last_source(&converted);
            if count == 20 {
                assert_eq!(result["data"], url.split_once(";base64,").unwrap().1);
                assert_eq!(decoded_source(result).dimensions(), (1099, 2048));
            } else {
                assert_eq!(decoded_source(result).dimensions(), (1073, 2000));
            }
            assert_eq!(
                converted["messages"][2]["content"][0]["tool_use_id"],
                "call_image"
            );
            assert_eq!(
                converted["messages"][2]["content"][0]["content"][0]["text"],
                "图片已读取"
            );
        }
    }

    #[test]
    fn resizes_supported_formats_and_preserves_png_alpha() {
        for format in [ImageFormat::Png, ImageFormat::Gif, ImageFormat::WebP] {
            let url = image_url(2048, 32, format);
            let converted = responses_to_anthropic_messages(request_with_images(21, &url)).unwrap();
            let source = last_source(&converted);
            assert_eq!(source["media_type"], "image/png");
            let image = decoded_source(source);
            assert_eq!(image.dimensions(), (2000, 31));
            if format != ImageFormat::Gif {
                assert_eq!(image.to_rgba8().get_pixel(1000, 15).0, [20, 40, 60, 128]);
            }
        }
    }

    #[test]
    fn compliant_images_keep_original_bytes_and_native_chat_is_unchanged() {
        let at_limit = image_url(2000, 16, ImageFormat::Jpeg);
        let request = request_with_images(21, &at_limit);
        let converted = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            last_source(&converted)["data"],
            at_limit.split_once(";base64,").unwrap().1
        );

        let oversized = image_url(2048, 32, ImageFormat::Png);
        let request = request_with_images(21, &oversized);
        let chat = responses_to_chat_completions(request).unwrap();
        let content = chat["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_array()
            .unwrap();
        assert!(
            content
                .iter()
                .any(|part| part["image_url"]["url"] == oversized)
        );
    }

    #[test]
    fn small_requests_only_resize_beyond_single_image_limit() {
        for width in [8000, 8001] {
            let url = image_url(width, 8, ImageFormat::Png);
            let converted = responses_to_anthropic_messages(request_with_images(1, &url)).unwrap();
            let source = last_source(&converted);
            assert_eq!(decoded_source(source).dimensions(), (8000, 8));
            if width == 8000 {
                assert_eq!(source["data"], url.split_once(";base64,").unwrap().1);
            }
        }
    }

    #[test]
    fn jpeg_orientation_is_applied_before_reencoding() {
        use image::ImageEncoder as _;
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        // 小端 TIFF：Orientation=6（顺时针旋转 90 度）。
        encoder
            .set_exif_metadata(vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0,
                0, 0, 0,
            ])
            .unwrap();
        encoder
            .encode_image(&image::RgbImage::new(2048, 32))
            .unwrap();
        let url = format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes));
        let converted = responses_to_anthropic_messages(request_with_images(21, &url)).unwrap();
        assert_eq!(
            decoded_source(last_source(&converted)).dimensions(),
            (31, 2000)
        );
    }

    #[tokio::test]
    async fn upstream_http_receives_resized_images_in_stream_and_json_modes() {
        let _lock = settings_path_test_lock().lock().unwrap();
        let url = image_url(2048, 32, ImageFormat::Jpeg);
        for stream in [false, true] {
            let server = spawn_chat_server();
            let settings = BackendSettings {
                relay_profiles: vec![RelayProfile {
                    id: "image-limit-test".to_string(),
                    name: "Image Limit Test".to_string(),
                    base_url: server.base_url.clone(),
                    upstream_base_url: server.base_url.clone(),
                    api_key: "sk-test".to_string(),
                    model_mappings: vec![RelayModelMapping {
                        system_prompt_override: String::new(),
                        request_model: "claude-test".to_string(),
                        alias: String::new(),
                        protocol: RelayProtocol::Anthropic,
                        context_window: "1000000".to_string(),
                    }],
                    ..Default::default()
                }],
                active_relay_id: "image-limit-test".to_string(),
                ..Default::default()
            };
            let mut request = request_with_images(21, &url);
            request["stream"] = json!(stream);
            let response =
                open_responses_proxy_request_with_settings(&request.to_string(), settings)
                    .await
                    .unwrap();
            assert_eq!(response.status_code, 200);
            assert_eq!(
                response.response_protocol,
                UpstreamResponseProtocol::Anthropic
            );
            drop(response);
            let captured = server.finish();
            assert_eq!(captured.path, "/v1/messages");
            let sent: Value = serde_json::from_str(&captured.body).unwrap();
            assert_eq!(decoded_source(last_source(&sent)).dimensions(), (2000, 31));
            assert_eq!(sent["stream"], stream);
        }
    }

    #[test]
    fn url_images_count_without_download_and_tool_arguments_stay_untouched() {
        let oversized = image_url(2048, 32, ImageFormat::Png);
        let mut request = request_with_images(21, &oversized);
        for image in request["input"][0]["content"].as_array_mut().unwrap() {
            image["image_url"] = json!("https://example.invalid/test.png");
        }
        let business_input = json!({
            "type":"image","source":{"type":"base64","media_type":"image/png",
            "data":oversized.split_once(";base64,").unwrap().1}
        });
        request["input"][1]["arguments"] = json!(business_input.to_string());
        let converted = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            decoded_source(last_source(&converted)).dimensions(),
            (2000, 31)
        );
        assert_eq!(
            converted["messages"][0]["content"][0]["source"],
            json!({"type":"url","url":"https://example.invalid/test.png"})
        );
        assert_eq!(
            converted["messages"][1]["content"][0]["input"],
            business_input
        );

        // 工具业务参数中的 image 对象不能使真实的 20 张图片误触发多图限制。
        let mut request = request_with_images(20, &oversized);
        request["input"][1]["arguments"] = json!(business_input.to_string());
        let converted = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            decoded_source(last_source(&converted)).dimensions(),
            (2048, 32)
        );
    }
}

mod tool_conversion_audit {
    use super::*;

    fn tool_request(tools: Value) -> Value {
        json!({ "model": "claude-opus-4-8", "input": "audit", "tools": tools })
    }

    #[test]
    fn declaration_preserves_tool_search_parameters() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "number", "minimum": 1, "maximum": 8 }
            },
            "required": ["query"],
            "additionalProperties": false
        });
        let request = tool_request(json!([{
            "type": "tool_search", "execution": "client", "parameters": parameters
        }]));
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(chat["tools"][0]["function"]["parameters"], parameters);
        assert_eq!(anthropic["tools"][0]["input_schema"], parameters);
    }

    #[test]
    fn namespace_tools_preserve_strict_flags() {
        for strict in [true, false] {
            let request = tool_request(json!([{
                "type": "namespace", "name": "mcp__audit", "tools": [{
                    "type": "function", "name": "inspect", "strict": strict,
                    "parameters": { "type": "object", "properties": {} }
                }]
            }]));
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anthropic = responses_to_anthropic_messages(request).unwrap();
            assert_eq!(chat["tools"][0]["function"]["strict"], strict);
            assert_eq!(anthropic["tools"][0]["strict"], strict);
        }
    }

    #[test]
    fn removed_web_search_cannot_remain_the_forced_chat_tool() {
        let mut request = tool_request(json!([
            {"type":"web_search"},
            {"type":"function","name":"exec_command","parameters":{"type":"object"}}
        ]));
        request["tool_choice"] = json!({"type":"web_search"});
        let converted = responses_to_chat_completions(request).unwrap();
        assert!(
            converted["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["function"]["name"] != "web_search")
        );
        assert!(
            converted.get("tool_choice").is_none(),
            "不能强制调用已从声明中移除的工具"
        );
    }

    #[test]
    fn anthropic_preserves_explicit_parallel_tool_control() {
        for parallel in [true, false] {
            let mut request = tool_request(json!([{
                "type":"function","name":"inspect","parameters":{"type":"object"}
            }]));
            request["parallel_tool_calls"] = json!(parallel);
            let converted = responses_to_anthropic_messages(request).unwrap();
            assert_eq!(converted["tool_choice"]["type"], "auto");
            assert_eq!(
                converted["tool_choice"]["disable_parallel_tool_use"],
                !parallel
            );
        }
    }

    #[test]
    fn wrapped_function_declarations_and_choice_are_not_dropped() {
        let mut request = tool_request(json!([
            { "type": "function", "function": {
                "name": "first", "parameters": { "type": "object", "properties": {} }
            }},
            { "type": "function", "function": {
                "name": "second", "parameters": { "type": "object", "properties": {} }
            }}
        ]));
        request["tool_choice"] = json!({
            "type": "function", "function": { "name": "second" }
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let anthropic = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(chat["tools"].as_array().unwrap().len(), 2);
        assert_eq!(anthropic["tools"].as_array().unwrap().len(), 2);
        assert_eq!(chat["tool_choice"]["function"]["name"], "second");
        assert_eq!(anthropic["tool_choice"]["name"], "second");
    }

    fn typed_text_request() -> Value {
        tool_request(json!([{
            "type": "namespace", "name": "mcp__audit", "tools": [{
                "type": "function", "name": "inspect",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "session_id": { "$ref": "#/$defs/id" },
                        "enabled": { "type": "boolean" },
                        "items": { "type": "array", "items": { "type": "integer" } },
                        "config": { "type": "object" },
                        "optional": { "type": ["integer", "null"] },
                        "code": { "type": "string" }
                    },
                    "$defs": { "id": { "type": "integer" } }
                }
            }]
        }]))
    }

    #[test]
    fn textual_invocations_preserve_declared_types_in_json_and_stream() {
        let text = concat!(
            "<invoke name=\"mcp__audit__inspect\">",
            "<parameter name=\"session_id\">42</parameter>",
            "<parameter name=\"enabled\">false</parameter>",
            "<parameter name=\"items\">[1,2]</parameter>",
            "<parameter name=\"config\">{\"key\":\"中文\"}</parameter>",
            "<parameter name=\"optional\">null</parameter>",
            "<parameter name=\"code\">{\"literal\":true}</parameter>",
            "</invoke>"
        );
        let request = typed_text_request();
        let direct = anthropic_message_to_response_with_compat(
            json!({
                "id": "msg_typed", "model": "claude-opus-4-8",
                "content": [{ "type": "text", "text": text }], "stop_reason": "end_turn"
            }),
            &request,
        )
        .unwrap();
        let streamed = anthropic_sse_to_responses_sse_with_compat(
            &anthropic_sse_from_text_deltas(&[text.to_string()]),
            &request,
        );
        let events = parse_response_sse_events(&streamed);
        let stream_item = &events
            .iter()
            .find(|e| e.event == "response.output_item.done")
            .unwrap()
            .data["item"];
        let expected = json!({
            "session_id": 42, "enabled": false, "items": [1,2],
            "config": { "key": "中文" }, "optional": null, "code": "{\"literal\":true}"
        });
        for item in [&direct["output"][0], stream_item] {
            assert_eq!(item["namespace"], "mcp__audit");
            assert_eq!(item["name"], "inspect");
            assert_eq!(
                serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn textual_invocation_ids_do_not_collide_between_responses() {
        let request = typed_text_request();
        let ids: Vec<_> = ["msg_first", "msg_second"]
            .into_iter()
            .map(|id| {
                anthropic_message_to_response_with_compat(
                    json!({
                        "id": id, "model": "claude-opus-4-8",
                        "content": [{
                            "type": "text",
                            "text": "<invoke name=\"mcp__audit__inspect\"><parameter name=\"session_id\">42</parameter></invoke>"
                        }],
                        "stop_reason": "end_turn"
                    }),
                    &request,
                )
                .unwrap()["output"][0]["call_id"]
                    .clone()
            })
            .collect();
        assert_ne!(ids[0], ids[1], "连续两轮工具调用不能复用同一 call_id");
    }

    #[test]
    fn textual_arguments_follow_properties_inside_composed_schemas() {
        for composition in ["oneOf", "anyOf", "allOf"] {
            let mut parameters = json!({"type":"object"});
            parameters[composition] = json!([{
                "type":"object","properties":{
                    "count":{"type":"integer"},"enabled":{"type":"boolean"},
                    "literal":{"anyOf":[{"type":"string"},{"type":"integer"}]}
                }
            }]);
            let request =
                tool_request(json!([{"type":"function","name":"inspect","parameters":parameters}]));
            let converted = anthropic_message_to_response_with_compat(json!({
                "id":"msg_composed","model":"claude-opus-4-8","stop_reason":"end_turn",
                "content":[{"type":"text","text":"<invoke name=\"inspect\"><parameter name=\"count\">7</parameter><parameter name=\"enabled\">false</parameter><parameter name=\"literal\">42</parameter></invoke>"}]
            }), &request).unwrap();
            let arguments: Value =
                serde_json::from_str(converted["output"][0]["arguments"].as_str().unwrap())
                    .unwrap();
            assert_eq!(
                arguments,
                json!({"count":7,"enabled":false,"literal":"42"}),
                "{composition}"
            );
        }
    }

    #[test]
    fn chat_stream_waits_for_complete_header_and_normalizes_empty_arguments() {
        let request = tool_request(json!([
            { "type": "namespace", "name": "mcp__audit", "tools": [{
                "type": "function", "name": "inspect",
                "parameters": { "type": "object", "properties": {} }
            }] },
            { "type": "function", "name": "no_args", "parameters": { "type": "object" } }
        ]));
        let chunks = [
            json!({"index": 0, "id": "call_frag"}),
            json!({"index": 0, "function": {"name": "mcp__audit__"}}),
            json!({"index": 0, "function": {"name": "inspect", "arguments": "{"}}),
            json!({"index": 0, "function": {"arguments": "}"}}),
            json!({"index": 1, "id": "call_empty", "function": {"name": "no_args"}}),
        ];
        let mut upstream = String::new();
        for call in chunks {
            upstream.push_str(&format!(
                "data: {}\n\n",
                json!({
                    "id": "chatcmpl_frag", "model": "claude-opus-4-8",
                    "choices": [{ "delta": { "tool_calls": [call] } }]
                })
            ));
        }
        upstream.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n");
        let stream = chat_sse_to_responses_sse_with_request(&upstream, &request);
        let events = parse_response_sse_events(&stream);
        let added: Vec<_> = events
            .iter()
            .filter(|e| e.event == "response.output_item.added")
            .collect();
        let done: Vec<_> = events
            .iter()
            .filter(|e| e.event == "response.output_item.done")
            .collect();
        assert_eq!(added.len(), 2);
        assert_eq!(done.len(), 2);
        for (a, d) in added.iter().zip(&done) {
            assert_eq!(a.data["item"]["name"], d.data["item"]["name"]);
            assert_eq!(a.data["item"]["id"], d.data["item"]["id"]);
            assert_eq!(d.data["item"]["arguments"], "{}");
        }
        assert_eq!(done[0].data["item"]["name"], "inspect");
        assert_eq!(done[0].data["item"]["namespace"], "mcp__audit");
    }

    #[test]
    fn apply_patch_history_preserves_end_of_file_hunks() {
        let patch = "*** Begin Patch\n*** Update File: old.txt\n@@\n-last\n+new\n*** End of File\n*** End Patch";
        let request = json!({
            "model": "gpt-5-mini",
            "tools": [{ "type": "custom", "name": "apply_patch" }],
            "input": [{
                "type": "custom_tool_call", "call_id": "call_eof", "name": "apply_patch",
                "input": patch
            }]
        });
        let chat = responses_to_chat_completions(request.clone()).unwrap();
        let function = &chat["messages"][0]["tool_calls"][0]["function"];
        let response = chat_completion_to_response_with_request(
            json!({
                "id": "chatcmpl_eof", "model": "gpt-5-mini", "choices": [{
                    "finish_reason": "tool_calls", "message": { "tool_calls": [{
                        "id": "call_eof", "type": "function", "function": function
                    }] }
                }]
            }),
            &request,
        )
        .unwrap();
        assert_eq!(response["output"][0]["input"], patch);
    }

    fn sample_arguments(schema: &Value, root: &Value, depth: usize) -> Value {
        if depth > 12 {
            return Value::Null;
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            if let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p)) {
                return sample_arguments(target, root, depth + 1);
            }
        }
        if let Some(value) = schema.get("const").or_else(|| schema.pointer("/enum/0")) {
            return value.clone();
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(branch) = schema.get(key).and_then(|v| v.get(0)) {
                return sample_arguments(branch, root, depth + 1);
            }
        }
        let kind = schema
            .get("type")
            .and_then(|v| v.as_str().or_else(|| v.get(0)?.as_str()));
        match kind.unwrap_or("object") {
            "string" => json!("中文 \"quoted\" \\path\n{\"literal\":false}"),
            "boolean" => json!(false),
            "integer" | "number" => schema.get("minimum").cloned().unwrap_or(json!(1)),
            "null" => Value::Null,
            "array" => {
                let count = schema
                    .get("minItems")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .min(3);
                json!(
                    (0..count)
                        .map(|_| sample_arguments(
                            schema.get("items").unwrap_or(&json!({})),
                            root,
                            depth + 1
                        ))
                        .collect::<Vec<_>>()
                )
            }
            _ => {
                let mut object = serde_json::Map::new();
                if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                    for (key, value) in properties {
                        object.insert(key.clone(), sample_arguments(value, root, depth + 1));
                    }
                }
                if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
                    for branch in branches {
                        if let Value::Object(values) = sample_arguments(branch, root, depth + 1) {
                            object.extend(values);
                        }
                    }
                }
                Value::Object(object)
            }
        }
    }

    fn mock_tool_response(
        request: &Value,
        name: &str,
        arguments: &Value,
        anthropic: bool,
        stream: bool,
    ) -> Value {
        let call_id = "call_matrix";
        if !stream {
            return if anthropic {
                anthropic_message_to_response_with_request(json!({
                    "id": "msg_matrix", "model": "claude-opus-4-8",
                    "content": [{ "type": "tool_use", "id": call_id, "name": name, "input": arguments }],
                    "stop_reason": "tool_use"
                }), request).unwrap()["output"][0].clone()
            } else {
                chat_completion_to_response_with_request(
                    json!({
                        "id": "chatcmpl_matrix", "model": "gpt-chat", "choices": [{
                            "finish_reason": "tool_calls", "message": { "tool_calls": [{
                                "id": call_id, "type": "function",
                                "function": { "name": name, "arguments": arguments.to_string() }
                            }] }
                        }]
                    }),
                    request,
                )
                .unwrap()["output"][0]
                    .clone()
            };
        }
        let mut chunks = Vec::new();
        if anthropic {
            chunks.push(json!({"type":"message_start","message":{"id":"msg_matrix","model":"claude-opus-4-8","content":[]}}));
            chunks.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call_id,"name":name,"input":{}}}));
            // 同时覆盖 JSON 参数逐字符分片和中文 UTF-8 在网络字节边界处分片。
            for ch in arguments.to_string().chars() {
                chunks.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":ch.to_string()}}));
            }
            chunks.push(json!({"type":"content_block_stop","index":0}));
            chunks.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1}}));
            chunks.push(json!({"type":"message_stop"}));
        } else {
            chunks.push(json!({"id":"chatcmpl_matrix","model":"gpt-chat","choices":[{"delta":{"tool_calls":[{"index":0,"id":call_id,"function":{"name":name}}]}}]}));
            for ch in arguments.to_string().chars() {
                chunks.push(json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":ch.to_string()}}]}}]}));
            }
            chunks.push(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
        }
        let mut wire: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
        if !anthropic {
            wire.push_str("data: [DONE]\n\n");
        }
        let mut bytes = Vec::new();
        if anthropic {
            let mut converter = AnthropicSseToResponsesConverter::with_request(request);
            for chunk in wire.as_bytes().chunks(1) {
                bytes.extend(converter.push_bytes(chunk));
            }
            bytes.extend(converter.finish());
        } else {
            let mut converter = ChatSseToResponsesConverter::with_request(request);
            for chunk in wire.as_bytes().chunks(1) {
                bytes.extend(converter.push_bytes(chunk));
            }
            bytes.extend(converter.finish());
        }
        let events = parse_response_sse_events(std::str::from_utf8(&bytes).unwrap());
        assert!(!events.iter().any(|e| e.event == "response.failed"));
        let added = &events
            .iter()
            .find(|e| e.event == "response.output_item.added")
            .unwrap()
            .data["item"];
        let done = &events
            .iter()
            .find(|e| e.event == "response.output_item.done")
            .unwrap()
            .data["item"];
        assert_eq!(added["id"], done["id"]);
        assert_eq!(added["name"], done["name"]);
        assert_eq!(added["call_id"], done["call_id"]);
        assert_eq!(
            added.get("encrypted_function_args"),
            done.get("encrypted_function_args")
        );
        let complete = &events
            .iter()
            .find(|e| e.event == "response.completed")
            .unwrap()
            .data;
        assert_eq!(complete["response"]["output"][0], *done);
        done.clone()
    }

    fn v2_collaboration_request(namespace: &str, name: &str) -> Value {
        tool_request(json!([{
            "type": "namespace", "name": namespace, "tools": [{
                "type": "function", "name": name,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string", "description": "Initial plain-text task",
                            "encrypted": true
                        },
                        "task_name": {"type": "string"},
                        "target": {"type": "string"}
                    },
                    "required": ["message"], "additionalProperties": false
                }
            }]
        }]))
    }

    #[test]
    fn v2_collaboration_plaintext_survives_all_tools_and_wire_modes() {
        for name in ["spawn_agent", "followup_task", "send_message"] {
            let request = v2_collaboration_request("collaboration", name);
            for message in [
                "你好",
                "任务\n保留 \"引号\"、路径 C:\\temp\\a",
                "gAAAAA_plain_text",
                "",
            ] {
                let arguments = json!({
                    "message": message, "task_name": "worker", "target": "/root/worker"
                });
                for anthropic in [false, true] {
                    for stream in [false, true] {
                        let item = mock_tool_response(
                            &request,
                            &format!("collaboration__{name}"),
                            &arguments,
                            anthropic,
                            stream,
                        );
                        assert_eq!(item["namespace"], "collaboration");
                        assert_eq!(item["name"], name);
                        assert_eq!(item["encrypted_function_args"], json!([]));
                        assert_eq!(
                            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap())
                                .unwrap(),
                            arguments
                        );
                        // 父代理回放时参数与工具结果仍保持配对，不让明文标记污染参数。
                        let mut replay = request.clone();
                        replay["input"] = json!([
                            {"role":"user","content":"开始"},
                            item,
                            {"type":"function_call_output","call_id":"call_matrix","output":"ok"}
                        ]);
                        let chat = responses_to_chat_completions(replay.clone()).unwrap();
                        assert_eq!(
                            serde_json::from_str::<Value>(
                                chat["messages"][1]["tool_calls"][0]["function"]["arguments"]
                                    .as_str()
                                    .unwrap()
                            )
                            .unwrap(),
                            arguments
                        );
                        let anth = responses_to_anthropic_messages(replay).unwrap();
                        assert_eq!(anth["messages"][1]["content"][0]["input"], arguments);
                    }
                }
            }
        }
    }

    #[test]
    fn v2_collaboration_encryption_schema_is_removed_only_for_translated_messages() {
        for (namespace, name, plaintext) in [
            ("collaboration", "spawn_agent", true),
            ("collaboration", "followup_task", true),
            ("collaboration", "send_message", true),
            ("collaboration", "list_agents", false),
            ("mcp__mail", "send_message", false),
            ("functions", "spawn_agent", false),
        ] {
            let request = v2_collaboration_request(namespace, name);
            let mut expected = request["tools"][0]["tools"][0]["parameters"].clone();
            if plaintext {
                expected["properties"]["message"]
                    .as_object_mut()
                    .unwrap()
                    .remove("encrypted");
            }
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anth = responses_to_anthropic_messages(request.clone()).unwrap();
            assert_eq!(chat["tools"][0]["function"]["parameters"], expected);
            assert_eq!(anth["tools"][0]["input_schema"], expected);
            assert_eq!(
                request["tools"][0]["tools"][0]["parameters"]["properties"]["message"]["encrypted"],
                true
            );
            for anthropic in [false, true] {
                let item = mock_tool_response(
                    &request,
                    &format!("{namespace}__{name}"),
                    &json!({"message":"你好"}),
                    anthropic,
                    true,
                );
                assert_eq!(item.get("encrypted_function_args").is_some(), plaintext);
            }
        }
        // V1 和没有已声明 namespace 的同名函数不能被误认成 V2。
        let request = tool_request(json!([{
            "type":"function","name":"spawn_agent","parameters":{"type":"object"}
        }]));
        for name in ["spawn_agent", "collaboration__spawn_agent"] {
            let item = mock_tool_response(&request, name, &json!({"message":"你好"}), true, false);
            assert!(item.get("encrypted_function_args").is_none());
        }
    }

    #[test]
    fn v2_collaboration_textual_tool_fallback_marks_plaintext() {
        for name in ["spawn_agent", "followup_task", "send_message"] {
            let request = v2_collaboration_request("collaboration", name);
            let text = format!(
                "<invoke name=\"collaboration__{name}\"><parameter name=\"message\">你好</parameter></invoke>"
            );
            let direct = anthropic_message_to_response_with_compat(
                json!({
                    "id":"msg_v2","model":"claude-test","stop_reason":"end_turn",
                    "content":[{"type":"text","text":text}]
                }),
                &request,
            )
            .unwrap();
            let stream = anthropic_sse_to_responses_sse_with_compat(
                &anthropic_sse_from_text_deltas(&[text]),
                &request,
            );
            let events = parse_response_sse_events(&stream);
            let done = &events
                .iter()
                .find(|e| e.event == "response.output_item.done")
                .unwrap()
                .data["item"];
            for item in [&direct["output"][0], done] {
                assert_eq!(item["encrypted_function_args"], json!([]));
                assert_eq!(
                    serde_json::from_str::<Value>(item["arguments"].as_str().unwrap()).unwrap(),
                    json!({"message":"你好"})
                );
            }
        }
    }

    #[test]
    fn v2_collaboration_plaintext_agent_message_reaches_translated_children() {
        for kind in ["NEW_TASK", "MESSAGE"] {
            let text = format!(
                "Message Type: {kind}\nTask name: /root/worker\nSender: /root\nPayload:\n你好\n继续工作"
            );
            // 与 Codex 收到 encrypted_function_args: [] 后生成的 agent_message 一致。
            let request = json!({
                "model":"claude-test",
                "input":[{
                    "type":"agent_message", "author":"/root", "recipient":"/root/worker",
                    "content":[{"type":"input_text","text":text}]
                }]
            });
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anth = responses_to_anthropic_messages(request).unwrap();
            assert_eq!(chat["messages"][0]["role"], "user");
            assert_eq!(chat["messages"][0]["content"], text);
            assert_eq!(anth["messages"][0]["role"], "user");
            assert_eq!(anth["messages"][0]["content"][0]["text"], text);
        }
    }

    #[test]
    fn function_catalog_roundtrips_all_tools_across_four_wire_modes() {
        let parameters = json!({
            "type":"object","properties":{
                "code":{"type":"string"},"session_id":{"type":"integer"},"enabled":{"type":"boolean"},
                "items":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}}}},
                "optional":{"type":["null","string"]}
            },"required":["code","session_id","enabled","items"],"additionalProperties":false
        });
        let mut requests = vec![tool_request(json!([
            {"type":"function","name":"exec_command","parameters":parameters,"strict":false},
            {"type":"namespace","name":"mcp__audit","tools":[
                {"type":"function","name":"inspect","parameters":parameters,"strict":true},
                {"type":"function","name":"no_args","parameters":{"type":"object","properties":{}}}
            ]},
            {"type":"function","function":{"name":"wrapped","parameters":parameters,"strict":false}}
        ]))];
        // 可选真实会话目录补充样本；无外部样本时仍完整执行上述确定性回归。
        if let Ok(path) = std::env::var("CODEX_ELVES_TOOL_CATALOG_FIXTURE") {
            let tools: Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            requests.push(tool_request(tools));
        }
        let mut checked = 0;
        for request in requests {
            let mut functions = Vec::new();
            for tool in request["tools"].as_array().unwrap() {
                if tool["type"] == "namespace" {
                    for child in tool["tools"].as_array().unwrap() {
                        if child["type"] == "function" {
                            functions.push((tool["name"].as_str().unwrap(), child));
                        }
                    }
                } else if tool["type"] == "function" {
                    functions.push(("", tool.get("function").unwrap_or(tool)));
                }
            }
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anth = responses_to_anthropic_messages(request.clone()).unwrap();
            for (namespace, tool) in functions {
                let name = tool["name"].as_str().unwrap();
                let flat = if namespace.is_empty() {
                    name.to_string()
                } else {
                    format!("{namespace}__{name}")
                };
                let arguments = sample_arguments(&tool["parameters"], &tool["parameters"], 0);
                assert_eq!(
                    chat["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|t| t["function"]["name"] == flat)
                        .count(),
                    1
                );
                assert_eq!(
                    anth["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|t| t["name"] == flat)
                        .count(),
                    1
                );
                for anthropic in [false, true] {
                    for stream in [false, true] {
                        let item =
                            mock_tool_response(&request, &flat, &arguments, anthropic, stream);
                        assert_eq!(item["name"], name);
                        assert_eq!(
                            item.get("namespace").and_then(Value::as_str).unwrap_or(""),
                            namespace
                        );
                        assert_eq!(
                            serde_json::from_str::<Value>(item["arguments"].as_str().unwrap())
                                .unwrap(),
                            arguments
                        );
                        let mut followup = request.clone();
                        followup["input"] = json!([
                            {"role":"user","content":"audit"},
                            item,
                            {"type":"function_call_output","call_id":"call_matrix","output":"工具结果：成功"}
                        ]);
                        let replay = if anthropic {
                            responses_to_anthropic_messages(followup)
                        } else {
                            responses_to_chat_completions(followup)
                        }
                        .unwrap();
                        if anthropic {
                            assert_eq!(replay["messages"][1]["content"][0]["name"], flat);
                            assert_eq!(replay["messages"][1]["content"][0]["input"], arguments);
                            assert_eq!(
                                replay["messages"][2]["content"][0]["tool_use_id"],
                                "call_matrix"
                            );
                        } else {
                            assert_eq!(
                                replay["messages"][1]["tool_calls"][0]["function"]["name"],
                                flat
                            );
                            assert_eq!(
                                serde_json::from_str::<Value>(
                                    replay["messages"][1]["tool_calls"][0]["function"]["arguments"]
                                        .as_str()
                                        .unwrap()
                                )
                                .unwrap(),
                                arguments
                            );
                            assert_eq!(replay["messages"][2]["tool_call_id"], "call_matrix");
                        }
                        checked += 1;
                    }
                }
            }
        }
        eprintln!("function catalog roundtrip scenarios: {checked}");
    }

    #[test]
    fn custom_payloads_roundtrip_without_losing_whitespace_or_json_text() {
        let request = tool_request(json!([{"type":"custom","name":"exec"}]));
        for payload in [
            "",
            "\nprint('中文')\n",
            "{\"input\":\"literal JSON\"}",
            "  leading and trailing  ",
        ] {
            for anthropic in [false, true] {
                for stream in [false, true] {
                    let item = mock_tool_response(
                        &request,
                        "exec",
                        &json!({"input":payload}),
                        anthropic,
                        stream,
                    );
                    assert_eq!(item["type"], "custom_tool_call");
                    assert_eq!(item["input"], payload);
                    let mut followup = request.clone();
                    followup["input"] = json!([
                        {"role":"user","content":"audit"}, item,
                        {"type":"custom_tool_call_output","call_id":"call_matrix","output":"done"}
                    ]);
                    let replay = if anthropic {
                        responses_to_anthropic_messages(followup)
                    } else {
                        responses_to_chat_completions(followup)
                    }
                    .unwrap();
                    if anthropic {
                        assert_eq!(
                            replay["messages"][1]["content"][0]["input"]["input"],
                            payload
                        );
                    } else {
                        let args: Value = serde_json::from_str(
                            replay["messages"][1]["tool_calls"][0]["function"]["arguments"]
                                .as_str()
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(args["input"], payload);
                    }
                }
            }
        }
    }

    #[test]
    fn all_patch_actions_preserve_action_identity_in_four_modes() {
        let request = tool_request(json!([{"type":"custom","name":"apply_patch"}]));
        let cases = [
            (
                "add_file",
                json!({"path":"new.txt","content":"中文\nsecond"}),
            ),
            ("delete_file", json!({"path":"old.txt"})),
            (
                "replace_file",
                json!({"path":"old.txt","content":"replaced"}),
            ),
            (
                "update_file",
                json!({"path":"old.txt","move_to":"renamed.txt","hunks":[{
                    "lines":[{"op":"remove","text":"old"},{"op":"add","text":"new"}],
                    "end_of_file":true
                }]}),
            ),
            (
                "batch",
                json!({"operations":[
                    {"type":"replace_file","path":"one.txt","content":"one"},
                    {"type":"add_file","path":"two.txt","content":"two"}
                ]}),
            ),
        ];
        for (action, arguments) in cases {
            for anthropic in [false, true] {
                for stream in [false, true] {
                    let name = format!("apply_patch_{action}");
                    let item = mock_tool_response(&request, &name, &arguments, anthropic, stream);
                    assert_eq!(item["type"], "custom_tool_call");
                    assert_eq!(item["name"], "apply_patch");
                    let patch = item["input"].as_str().unwrap();
                    assert!(patch.starts_with("*** Begin Patch\n"));
                    assert!(patch.ends_with("\n*** End Patch"));
                    let mut followup = request.clone();
                    followup["input"] = json!([
                        {"role":"user","content":"audit"}, item,
                        {"type":"custom_tool_call_output","call_id":"call_matrix","output":"done"}
                    ]);
                    let replay = if anthropic {
                        responses_to_anthropic_messages(followup)
                    } else {
                        responses_to_chat_completions(followup)
                    }
                    .unwrap();
                    let replay_name = if anthropic {
                        &replay["messages"][1]["content"][0]["name"]
                    } else {
                        &replay["messages"][1]["tool_calls"][0]["function"]["name"]
                    };
                    assert_eq!(replay_name, &name);
                    if action == "replace_file" || action == "batch" {
                        assert!(!patch.contains("*** Delete File:"));
                    }
                    if action == "update_file" {
                        assert!(patch.contains("*** Move to: renamed.txt"));
                        assert!(patch.contains("\n*** End of File\n"));
                    }
                }
            }
        }
    }

    #[test]
    fn tool_search_loads_and_calls_namespaced_tools_in_four_modes() {
        let schema = json!({
            "type":"object","properties":{"query":{"type":"string"},"limit":{"type":"number"}},
            "required":["query"],"additionalProperties":false
        });
        for anthropic in [false, true] {
            for stream in [false, true] {
                let request = tool_request(
                    json!([{"type":"tool_search","execution":"client","parameters":schema}]),
                );
                let arguments = json!({"query":"audit lookup","limit":3});
                let mut search =
                    mock_tool_response(&request, "tool_search", &arguments, anthropic, stream);
                assert_eq!(search["type"], "tool_search_call");
                assert_eq!(search["arguments"], arguments);
                search["call_id"] = json!("call_search");
                let mut discovered = request.clone();
                discovered["input"] = json!([
                    {"role":"user","content":"audit"},
                    search,
                    {"type":"tool_search_output","call_id":"call_search","execution":"client","tools":[{
                        "type":"namespace","name":"mcp__discovered","tools":[{
                            "type":"function","name":"lookup","strict":false,"defer_loading":true,
                            "parameters":{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}
                        }]
                    }]}
                ]);
                let declaration = if anthropic {
                    responses_to_anthropic_messages(discovered.clone())
                } else {
                    responses_to_chat_completions(discovered.clone())
                }
                .unwrap();
                assert!(declaration["tools"].as_array().unwrap().iter().any(|t| {
                    if anthropic {
                        t["name"] == "mcp__discovered__lookup"
                    } else {
                        t["function"]["name"] == "mcp__discovered__lookup"
                    }
                }));
                let call = mock_tool_response(
                    &discovered,
                    "mcp__discovered__lookup",
                    &json!({"id":7}),
                    anthropic,
                    stream,
                );
                assert_eq!(call["namespace"], "mcp__discovered");
                assert_eq!(call["name"], "lookup");
                assert_eq!(call["arguments"], "{\"id\":7}");
            }
        }
    }

    #[test]
    fn interleaved_parallel_calls_keep_custom_and_function_arguments_separate() {
        let request = tool_request(json!([
            {"type":"custom","name":"exec"},
            {"type":"namespace","name":"mcp__audit","tools":[{
                "type":"function","name":"inspect","parameters":{"type":"object"}
            }]}
        ]));
        let chunks = [
            json!({"index":3,"id":"call_custom","function":{"name":"exec","arguments":"{\"input\":\""}}),
            json!({"index":8,"id":"call_function","function":{"name":"mcp__audit__inspect","arguments":"{\"count\":"}}),
            json!({"index":8,"function":{"arguments":"7}"}}),
            json!({"index":3,"function":{"arguments":"中文\"}"}}),
        ];
        let mut wire: String = chunks.into_iter().map(|call| format!("data: {}\n\n", json!({
            "id":"chatcmpl_parallel","model":"gpt-chat","choices":[{"delta":{"tool_calls":[call]}}]
        }))).collect();
        wire.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n");
        let converted = chat_sse_to_responses_sse_with_request(&wire, &request);
        let events = parse_response_sse_events(&converted);
        let output = &events
            .iter()
            .find(|e| e.event == "response.completed")
            .unwrap()
            .data["response"]["output"];
        assert_eq!(output[0]["call_id"], "call_custom");
        assert_eq!(output[0]["type"], "custom_tool_call");
        assert_eq!(output[0]["input"], "中文");
        assert_eq!(output[1]["call_id"], "call_function");
        assert_eq!(output[1]["namespace"], "mcp__audit");
        assert_eq!(output[1]["arguments"], "{\"count\":7}");
    }
}

#[derive(Debug)]
struct ParsedSseEvent {
    event: String,
    data: Value,
}

mod protocol_translation_fidelity_review {
    use super::*;

    #[test]
    fn chat_image_detail_survives_user_and_tool_history_translation() {
        for detail in ["low", "high", "auto", "original"] {
            for nested in [false, true] {
                let url = "https://example.test/image.png";
                let image = json!({
                    "type":"input_image","detail":detail,
                    "image_url":if nested { json!({"url":url,"detail":"low"}) } else { json!(url) }
                });
                let converted = responses_to_chat_completions(json!({
                    "model":"chat-test","input":[
                        {"role":"user","content":[image.clone()]},
                        {"type":"function_call","call_id":"call_image","name":"view","arguments":"{}"},
                        {"type":"function_call_output","call_id":"call_image","output":[image]}
                    ]
                })).unwrap();
                let messages = converted["messages"].as_array().unwrap();
                for message_index in [0, 3] {
                    assert_eq!(
                        messages[message_index]["content"][0]["image_url"],
                        json!({
                            "url":url,"detail":detail
                        })
                    );
                }
                assert_eq!(messages[2]["role"], "tool");
                assert_eq!(messages[2]["tool_call_id"], "call_image");
            }
        }
        let converted = responses_to_chat_completions(json!({
            "model":"chat-test","input":[{"role":"user","content":[
                {"type":"input_image","image_url":{"url":"https://example.test/a.png","detail":"high"}},
                {"type":"input_image","image_url":"https://example.test/b.png"}
            ]}]
        })).unwrap();
        assert_eq!(
            converted["messages"][0]["content"][0]["image_url"]["detail"],
            "high"
        );
        assert!(
            converted["messages"][0]["content"][1]["image_url"]
                .get("detail")
                .is_none()
        );
    }

    #[test]
    fn upstream_function_arguments_match_between_json_and_sse_without_rewriting() {
        for arguments in [
            "{\"path\":",
            "\"quoted 中文\"",
            "[1,2]",
            "true",
            "null",
            "raw text",
            " \n{\"path\":\"中文\"} \t",
            "",
        ] {
            let expected = if arguments.is_empty() {
                "{}"
            } else {
                arguments
            };
            for legacy in [false, true] {
                let function = json!({"name":"inspect","arguments":arguments});
                let message = if legacy {
                    json!({"function_call":function})
                } else {
                    json!({"tool_calls":[{"id":"call_0","type":"function","function":function}]})
                };
                let response = chat_completion_to_response(json!({
                    "id":"chatcmpl_fidelity","choices":[{"message":message,"finish_reason":"tool_calls"}]
                })).unwrap();
                let mut wire = String::new();
                for fragment in
                    std::iter::once(String::new()).chain(arguments.chars().map(|ch| ch.to_string()))
                {
                    let function = if fragment.is_empty() {
                        json!({"name":"inspect","arguments":""})
                    } else {
                        json!({"arguments":fragment})
                    };
                    let delta = if legacy {
                        json!({"function_call":function})
                    } else {
                        json!({"tool_calls":[{"index":0,"id":"call_0","function":function}]})
                    };
                    wire.push_str(&format!(
                        "data: {}\n\n",
                        json!({
                            "id":"chatcmpl_fidelity","choices":[{"delta":delta}]
                        })
                    ));
                }
                wire.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n");
                let events = parse_response_sse_events(&chat_sse_to_responses_sse(&wire));
                let streamed = &events
                    .iter()
                    .find(|event| event.event == "response.completed")
                    .unwrap()
                    .data["response"];
                assert_eq!(
                    response["output"][0]["arguments"], expected,
                    "legacy={legacy}"
                );
                assert_eq!(response["output"], streamed["output"]);
                let done = events
                    .iter()
                    .find(|event| event.event == "response.function_call_arguments.done")
                    .unwrap();
                assert_eq!(done.data["arguments"], expected);
            }
        }
    }

    fn tool_result_request(output: Value, custom: bool, orphan: bool) -> Value {
        let mut input = vec![json!({"role":"user","content":"inspect"})];
        if !orphan {
            input.push(if custom {
                json!({"type":"custom_tool_call","call_id":"call_text","name":"exec","input":"pwd"})
            } else {
                json!({"type":"function_call","call_id":"call_text","name":"inspect","arguments":"{}"})
            });
        }
        input.push(json!({
            "type":if custom { "custom_tool_call_output" } else { "function_call_output" },
            "call_id":"call_text","output":output
        }));
        json!({"model":"test","input":input})
    }

    #[test]
    fn typed_tool_result_text_is_replayed_as_text_in_both_protocols() {
        for custom in [false, true] {
            for orphan in [false, true] {
                let request = tool_result_request(
                    json!([
                        {"type":"input_text","text":"first 中文\nline"},
                        {"type":"input_text","text":"{\"literal\":true}"}
                    ]),
                    custom,
                    orphan,
                );
                let expected = "first 中文\nline\n{\"literal\":true}";
                let chat = responses_to_chat_completions(request.clone()).unwrap();
                let anthropic = responses_to_anthropic_messages(request).unwrap();
                let chat_messages = chat["messages"].as_array().unwrap();
                let anthropic_messages = anthropic["messages"].as_array().unwrap();
                if orphan {
                    assert_eq!(
                        chat_messages.last().unwrap()["content"],
                        format!("Function call output (call_text): {expected}")
                    );
                    assert!(
                        anthropic_messages.last().unwrap()["content"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|block| {
                                block["text"]
                                    == format!("Function call output (call_text): {expected}")
                            })
                    );
                } else {
                    assert_eq!(chat_messages.len(), 3);
                    assert_eq!(chat_messages[2]["content"], expected);
                    assert_eq!(anthropic_messages[2]["content"][0]["content"], expected);
                    assert_eq!(
                        anthropic_messages[2]["content"][0]["tool_use_id"],
                        "call_text"
                    );
                }
            }
        }
    }

    #[test]
    fn arbitrary_json_tool_results_are_not_mistaken_for_text_blocks() {
        for output in [
            json!({"text":"data","count":2}),
            json!([{"text":"data"},{"count":2}]),
            json!([{"type":"input_text","text":"known"},{"type":"vendor","payload":7}]),
            json!([{"type":"input_text","text":42}]),
        ] {
            let request = tool_result_request(output.clone(), false, false);
            let chat = responses_to_chat_completions(request.clone()).unwrap();
            let anthropic = responses_to_anthropic_messages(request).unwrap();
            for text in [
                &chat["messages"][2]["content"],
                &anthropic["messages"][2]["content"][0]["content"],
            ] {
                assert_eq!(
                    serde_json::from_str::<Value>(text.as_str().unwrap()).unwrap(),
                    output
                );
            }
        }
    }
}

mod protocol_five_round_review {
    use super::*;

    fn convert_stream(bytes: &[u8], chunk_size: usize, anthropic: bool) -> Vec<ParsedSseEvent> {
        let mut output = Vec::new();
        if anthropic {
            let mut converter = AnthropicSseToResponsesConverter::default();
            for chunk in bytes.chunks(chunk_size) {
                output.extend(converter.push_bytes(chunk));
            }
            output.extend(converter.finish());
        } else {
            let mut converter = ChatSseToResponsesConverter::default();
            for chunk in bytes.chunks(chunk_size) {
                output.extend(converter.push_bytes(chunk));
            }
            output.extend(converter.finish());
        }
        parse_response_sse_events(std::str::from_utf8(&output).unwrap())
    }

    fn text_stream(anthropic: bool, text: &str) -> String {
        if anthropic {
            [
                (
                    "message_start",
                    json!({"message":{"id":"msg_framing","model":"test","usage":{}}}),
                ),
                (
                    "content_block_start",
                    json!({"index":0,"content_block":{"type":"text","text":""}}),
                ),
                (
                    "content_block_delta",
                    json!({"index":0,"delta":{"type":"text_delta","text":text}}),
                ),
                ("content_block_stop", json!({"index":0})),
                ("message_delta", json!({"delta":{"stop_reason":"end_turn"}})),
                ("message_stop", json!({})),
            ]
            .into_iter()
            .map(|(event, data)| format!("event: {event}\ndata: {data}\n\n"))
            .collect()
        } else {
            format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"id":"chatcmpl_framing","model":"test","choices":[{
                    "index":0,"delta":{"content":text},"finish_reason":"stop"
                }]})
            )
        }
    }

    fn completed_response(events: &[ParsedSseEvent]) -> &Value {
        assert!(
            !events.iter().any(|event| event.event == "response.failed"),
            "{events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event == "response.completed")
                .count(),
            1
        );
        &events
            .iter()
            .find(|event| event.event == "response.completed")
            .unwrap()
            .data["response"]
    }

    #[test]
    fn round_1_sse_line_endings_are_independent_of_byte_boundaries() {
        for anthropic in [false, true] {
            let source = text_stream(anthropic, "中文🙂");
            for endings in [
                vec!["\n"],
                vec!["\r\n"],
                vec!["\r"],
                vec!["\r\n", "\n", "\r"],
            ] {
                let mut wire = String::new();
                for (index, line) in source.split_inclusive('\n').enumerate() {
                    wire.push_str(line.strip_suffix('\n').unwrap());
                    wire.push_str(endings[index % endings.len()]);
                }
                for chunk_size in [1, 2, 7, 4096] {
                    let events = convert_stream(wire.as_bytes(), chunk_size, anthropic);
                    let response = completed_response(&events);
                    assert_eq!(response["output"][0]["content"][0]["text"], "中文🙂");
                }
            }
        }
    }

    #[test]
    fn round_1_sse_strips_only_the_leading_bom() {
        for anthropic in [false, true] {
            let source = format!("\u{feff}{}", text_stream(anthropic, "keep \u{feff} inside"));
            for chunk_size in [1, 2, 7, 4096] {
                let events = convert_stream(source.as_bytes(), chunk_size, anthropic);
                let response = completed_response(&events);
                assert_eq!(
                    response["output"][0]["content"][0]["text"],
                    "keep \u{feff} inside"
                );
                assert!(response["id"].as_str().unwrap().contains("framing"));
            }
        }
    }

    #[test]
    fn round_2_invalid_utf8_does_not_corrupt_following_split_unicode() {
        for anthropic in [false, true] {
            let mut wire = text_stream(anthropic, "broken X中文🙂 end").into_bytes();
            let invalid = wire.iter().position(|byte| *byte == b'X').unwrap();
            wire[invalid] = 0xff;
            // 同一分片先遇到非法字节，末尾再落在正常中文字符中间。
            for chunk_size in [1, 2, 3, 7, invalid + 2, invalid + 3, wire.len()] {
                let events = convert_stream(&wire, chunk_size, anthropic);
                let response = completed_response(&events);
                assert_eq!(
                    response["output"][0]["content"][0]["text"], "broken \u{fffd}中文🙂 end",
                    "anthropic={anthropic}, chunk_size={chunk_size}"
                );
            }
        }
    }

    #[test]
    fn round_3_nonstream_selects_choice_zero_by_index() {
        let selected = json!({"index":0,"message":{"role":"assistant","content":"chosen"},"finish_reason":"stop"});
        let other = json!({"index":1,"message":{"role":"assistant","content":"other"},"finish_reason":"length"});
        for choices in [
            json!([other, selected]),
            json!([selected, other]),
            json!([{"message":{"role":"assistant","content":"chosen"},"finish_reason":"stop"}]),
        ] {
            let response = chat_completion_to_response(json!({
                "id":"chatcmpl_choices","model":"test","choices":choices
            }))
            .unwrap();
            assert_eq!(response["status"], "completed");
            assert_eq!(response["output"][0]["content"][0]["text"], "chosen");
        }
        assert!(chat_completion_to_response(json!({"choices":[other]})).is_err());
    }

    #[test]
    fn round_3_stream_does_not_mix_candidate_text_tools_or_finish_reasons() {
        let chunks = [
            json!({"choices":[{"index":1,"delta":{"content":"other"}}]}),
            json!({"choices":[
                {"index":1,"delta":{"reasoning_content":"wrong plan"}},
                {"index":0,"delta":{"reasoning_content":"chosen plan"}}
            ]}),
            json!({"choices":[{"index":0,"delta":{"content":"chosen 中文"}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{
                "index":0,"id":"call_chosen","function":{"name":"lookup","arguments":"{\"id\":"}
            }]}}]}),
            json!({"choices":[{"index":1,"delta":{"tool_calls":[{
                "index":0,"id":"call_other","function":{"name":"other","arguments":"bad"}
            }]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{
                "index":0,"function":{"arguments":"7}"}
            }]},"finish_reason":"tool_calls"}]}),
            json!({"choices":[{"index":1,"delta":{},"finish_reason":"length"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":5}}),
        ];
        let mut wire: String = chunks
            .into_iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .collect();
        wire.push_str("data: [DONE]\n\n");
        for chunk_size in [1, 13, 4096] {
            let events = convert_stream(wire.as_bytes(), chunk_size, false);
            let response = completed_response(&events);
            assert_eq!(response["output"].as_array().unwrap().len(), 3);
            assert_eq!(response["output"][0]["reasoning_content"], "chosen plan");
            assert_eq!(response["output"][1]["content"][0]["text"], "chosen 中文");
            assert_eq!(response["output"][2]["call_id"], "call_chosen");
            assert_eq!(response["output"][2]["name"], "lookup");
            assert_eq!(response["output"][2]["arguments"], "{\"id\":7}");
            assert_eq!(response["usage"]["total_tokens"], 16);
        }
    }

    #[test]
    fn round_4_usage_details_always_have_a_numeric_reasoning_count() {
        for (details, expected) in [
            (json!(null), 0),
            (json!({}), 0),
            (json!({"audio_tokens":2}), 0),
            (json!({"reasoning_tokens":null,"audio_tokens":2}), 0),
            (json!({"reasoning_tokens":4,"audio_tokens":2}), 4),
        ] {
            for anthropic in [false, true] {
                let usage = if anthropic {
                    json!({"input_tokens":11,"output_tokens":5,"output_tokens_details":details})
                } else {
                    json!({"prompt_tokens":11,"completion_tokens":5,"completion_tokens_details":details})
                };
                let response = if anthropic {
                    anthropic_message_to_response_with_request(
                        json!({
                            "id":"msg_usage","model":"test","content":[{"type":"text","text":"ok"}],
                            "stop_reason":"end_turn","usage":usage
                        }),
                        &json!({}),
                    )
                    .unwrap()
                } else {
                    chat_completion_to_response(json!({
                        "id":"chatcmpl_usage","model":"test","choices":[{
                            "index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"
                        }],"usage":usage
                    })).unwrap()
                };
                let wire = if anthropic {
                    format!(
                        "event: message_start\ndata: {}\n\nevent: message_stop\ndata: {{}}\n\n",
                        json!({"message":{"id":"msg_usage","model":"test","usage":usage}})
                    )
                } else {
                    format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[],"usage":usage})
                    )
                };
                let events = convert_stream(wire.as_bytes(), 7, anthropic);
                for usage in [&response["usage"], &completed_response(&events)["usage"]] {
                    assert_eq!(usage["total_tokens"], 16);
                    assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], expected);
                    if let Some(audio) = details.get("audio_tokens") {
                        assert_eq!(&usage["output_tokens_details"]["audio_tokens"], audio);
                    }
                }
            }
        }
    }

    #[test]
    fn round_4_null_detail_aliases_do_not_hide_thinking_usage() {
        for extra in [
            json!({"completion_tokens_details":null,"output_tokens_details":{"thinking_tokens":3}}),
            json!({"completion_tokens_details":{"reasoning_tokens":null},"thinking_tokens":3}),
            json!({"output_tokens_details":{"thinking_tokens":null},"thinking_tokens":3}),
        ] {
            let mut usage = json!({"prompt_tokens":11,"completion_tokens":5});
            usage
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let response = chat_completion_to_response(json!({
                "choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":usage
            }))
            .unwrap();
            assert_eq!(
                response["usage"]["output_tokens_details"]["reasoning_tokens"],
                3
            );
        }
    }

    #[test]
    fn round_5_model_discovery_accepts_all_configured_protocol_endpoints() {
        for endpoint in [
            "chat/completions",
            "responses",
            "messages",
            "models",
            "RESPONSES",
            "MESSAGES",
        ] {
            for (base, suffix, expected) in [
                (
                    "https://example.test/v1",
                    "",
                    "https://example.test/v1/models",
                ),
                (
                    "https://example.test/gateway/v2",
                    "/",
                    "https://example.test/gateway/v2/models",
                ),
                ("https://example.test", "#", "https://example.test/models"),
            ] {
                assert_eq!(models_url(&format!("{base}/{endpoint}{suffix}")), expected);
            }
        }
        assert_eq!(
            models_url("https://example.test/custom-messages"),
            "https://example.test/custom-messages/models"
        );
    }

    #[test]
    fn round_5_version_deduplication_respects_path_segment_boundaries() {
        let builders: [(&str, fn(&str) -> String); 4] = [
            ("chat/completions", chat_completions_url),
            ("responses", codex_elves_core::protocol_proxy::responses_url),
            ("messages", anthropic_messages_url),
            ("models", models_url),
        ];
        for (endpoint, build) in builders {
            for version in ["v10", "v11", "v1beta", "v1-custom"] {
                let base = format!("https://example.test/gateway/v1/{version}");
                assert_eq!(build(&base), format!("{base}/{endpoint}"));
            }
            assert_eq!(
                build("https://example.test/v1/v1/v1"),
                format!("https://example.test/v1/{endpoint}")
            );
        }
    }
}

mod protocol_request_review {
    use super::*;

    #[test]
    fn merged_tool_history_keeps_reasoning_before_parallel_results() {
        let variants = [
            (
                json!({"type":"function_call","call_id":"call_first","name":"lookup","arguments":"{}"}),
                json!({"type":"function_call_output","call_id":"call_first","output":"first result"}),
            ),
            (
                json!({"type":"custom_tool_call","call_id":"call_first","name":"exec","input":"pwd"}),
                json!({"type":"custom_tool_call_output","call_id":"call_first","output":"first result"}),
            ),
            (
                json!({"type":"tool_search_call","call_id":"call_first","execution":"client","arguments":{"query":"lookup"}}),
                json!({"type":"tool_search_output","call_id":"call_first","execution":"client","tools":[]}),
            ),
            (
                json!({"type":"tool_call","tool_use":{"id":"call_first","name":"lookup","input":{}}}),
                json!({"type":"tool_result","content":{"tool_use_id":"call_first","content":"first result"}}),
            ),
        ];
        for (call, output) in variants {
            for continued in [false, true] {
                let mut input = vec![
                    json!({"role":"user","content":"inspect"}),
                    json!({"type":"reasoning","summary":[{"type":"summary_text","text":"initial plan"}]}),
                    json!({"role":"assistant","content":"checking"}),
                    json!({"type":"reasoning","summary":[{"type":"summary_text","text":"tool plan 中文"}]}),
                    call.clone(),
                    json!({"type":"reasoning","summary":[{"type":"summary_text","text":"parallel plan"}]}),
                    json!({"type":"function_call","call_id":"call_second","name":"lookup","arguments":"{\"id\":2}"}),
                    json!({"type":"function_call_output","call_id":"call_second","output":"second result"}),
                    output.clone(),
                ];
                if continued {
                    input.push(json!({"role":"assistant","content":"finished"}));
                }
                let converted = responses_to_chat_completions(json!({
                    "model":"deepseek-reasoner","input":input
                }))
                .unwrap();
                let messages = converted["messages"].as_array().unwrap();
                assert_eq!(
                    messages[1]["reasoning_content"], "initial plan\ntool plan 中文\nparallel plan",
                    "call={call}, continued={continued}, messages={messages:?}"
                );
                assert_eq!(messages.len(), if continued { 5 } else { 4 });
                assert_eq!(messages[1]["content"], "checking");
                assert_eq!(messages[1]["tool_calls"].as_array().unwrap().len(), 2);
                assert_eq!(messages[1]["tool_calls"][0]["id"], "call_first");
                assert_eq!(messages[1]["tool_calls"][1]["id"], "call_second");
                assert_eq!(messages[2]["role"], "tool");
                assert_eq!(messages[2]["tool_call_id"], "call_second");
                assert_eq!(messages[2]["content"], "second result");
                assert_eq!(messages[3]["role"], "tool");
                assert_eq!(messages[3]["tool_call_id"], "call_first");
                if continued {
                    assert_eq!(messages[4]["content"], "finished");
                    assert!(messages[4].get("reasoning_content").is_none());
                }
            }
        }
    }

    #[test]
    fn invalid_stream_options_return_errors_without_panicking() {
        for options in [json!(true), json!(42), json!("invalid"), json!([])] {
            let result = std::panic::catch_unwind(|| {
                responses_to_chat_completions(json!({
                    "model":"chat-test","input":"hello","stream":true,
                    "stream_options":options
                }))
            });
            assert!(result.is_ok(), "stream_options={options} caused a panic");
            let error = result.unwrap().expect_err("invalid options must fail");
            assert!(error.to_string().contains("stream_options"));
        }
    }

    #[test]
    fn stream_options_preserve_extensions_and_enable_usage() {
        for options in [
            None,
            Some(Value::Null),
            Some(json!({})),
            Some(json!({"include_usage":false,"vendor_extension":{"enabled":true}})),
        ] {
            let mut request = json!({"model":"chat-test","input":"hello","stream":true});
            if let Some(options) = &options {
                request["stream_options"] = options.clone();
            }
            let converted = responses_to_chat_completions(request).unwrap();
            let mut expected = options.filter(Value::is_object).unwrap_or(json!({}));
            expected["include_usage"] = json!(true);
            assert_eq!(converted["stream_options"], expected);
        }
    }
}

mod protocol_order_review {
    use super::*;

    fn blocks() -> Vec<Value> {
        vec![
            json!({"type":"text","text":"before 中文"}),
            json!({"type":"thinking","thinking":"plan","signature":"sig_plan"}),
            json!({"type":"text","text":"between"}),
            json!({"type":"tool_use","id":"call_order","name":"lookup","input":{}}),
            json!({"type":"text","text":"after"}),
            json!({"type":"redacted_thinking","data":"opaque_order"}),
            json!({"type":"text","text":"end"}),
        ]
    }

    fn assert_order(response: &Value) {
        let output = response["output"].as_array().unwrap();
        assert_eq!(
            output
                .iter()
                .map(|item| item["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "message",
                "reasoning",
                "message",
                "function_call",
                "message",
                "reasoning",
                "message"
            ]
        );
        for (index, text) in [(0, "before 中文"), (2, "between"), (4, "after"), (6, "end")] {
            assert_eq!(output[index]["content"][0]["text"], text);
        }
        assert_eq!(output[3]["arguments"], "{}");
        let mut input = vec![json!({"role":"user","content":"start"})];
        input.extend(output.iter().cloned());
        input.push(json!({"type":"function_call_output","call_id":"call_order","output":"done"}));
        let replay = responses_to_anthropic_messages(json!({"input":input})).unwrap();
        assert_eq!(replay["messages"][1]["content"], json!(blocks()));
    }

    fn stream_wire(stop_reason: &str) -> String {
        let mut chunks =
            vec![json!({"type":"message_start","message":{"id":"msg_order","content":[]}})];
        for (index, block) in blocks().iter().enumerate() {
            chunks.push(json!({"type":"content_block_start","index":index,"content_block":block}));
            chunks.push(json!({"type":"content_block_stop","index":index}));
        }
        chunks.push(json!({"type":"message_delta","delta":{"stop_reason":stop_reason}}));
        chunks.push(json!({"type":"message_stop"}));
        chunks
            .iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .collect()
    }

    #[test]
    fn nonstream_preserves_interleaved_block_order_in_history() {
        let response = anthropic_message_to_response_with_request(
            json!({"id":"msg_order","content":blocks(),"stop_reason":"tool_use"}),
            &json!({}),
        )
        .unwrap();
        assert_order(&response);
    }

    #[test]
    fn stream_preserves_interleaved_block_order_and_output_indices() {
        let wire = stream_wire("tool_use");
        for width in [1, 23, 4096] {
            let mut converter = AnthropicSseToResponsesConverter::default();
            let mut converted = Vec::new();
            for bytes in wire.as_bytes().chunks(width) {
                converted.extend(converter.push_bytes(bytes));
            }
            converted.extend(converter.finish());
            let events = parse_response_sse_events(&String::from_utf8(converted).unwrap());
            let response = &events.last().unwrap().data["response"];
            assert_order(response);
            for event in events.iter().filter(|event| {
                matches!(
                    event.event.as_str(),
                    "response.output_item.added" | "response.output_item.done"
                )
            }) {
                let index = event.data["output_index"].as_u64().unwrap() as usize;
                assert_eq!(event.data["item"]["id"], response["output"][index]["id"]);
                if event.event == "response.output_item.done" {
                    assert_eq!(event.data["item"], response["output"][index]);
                }
            }
        }
    }

    #[test]
    fn done_event_arrival_order_matches_history_without_using_indices() {
        let stream =
            anthropic_sse_to_responses_sse_with_request(&stream_wire("tool_use"), &json!({}));
        let events = parse_response_sse_events(&stream);
        // Codex ResponseEvent::OutputItemDone 仅保留 item，不保留 output_index。
        let history: Vec<_> = events
            .iter()
            .filter(|event| event.event == "response.output_item.done")
            .map(|event| event.data["item"].clone())
            .collect();
        assert_eq!(
            json!(history),
            events.last().unwrap().data["response"]["output"]
        );
        assert_order(&json!({"output":history}));
    }

    #[test]
    fn truncated_interleaving_still_terminates_without_completing_tool() {
        let stream =
            anthropic_sse_to_responses_sse_with_request(&stream_wire("max_tokens"), &json!({}));
        let events = parse_response_sse_events(&stream);
        assert_eq!(events.last().unwrap().event, "response.incomplete");
        assert!(!events.iter().any(
            |event| event.event == "response.function_call_arguments.done"
                || (event.event == "response.output_item.done"
                    && event.data["item"]["type"] == "function_call")
        ));
        let output = events.last().unwrap().data["response"]["output"]
            .as_array()
            .unwrap();
        let text: String = output
            .iter()
            .filter_map(|item| item["content"][0]["text"].as_str())
            .collect();
        assert_eq!(text, "before 中文betweenafterend");
    }

    #[test]
    fn chat_stream_does_not_merge_text_across_reasoning_or_empty_argument_tools() {
        let deltas = [
            json!({"content":"before"}),
            json!({"reasoning_content":"plan"}),
            json!({"content":"between"}),
            json!({"tool_calls":[{"index":0,"id":"call_order","function":{"name":"lookup","arguments":""}}]}),
            json!({"content":"after"}),
        ];
        let mut wire: String = deltas
            .iter()
            .map(|delta| format!("data: {}\n\n", json!({"choices":[{"delta":delta}]})))
            .collect();
        wire.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n");
        let stream = chat_sse_to_responses_sse(&wire);
        let events = parse_response_sse_events(&stream);
        let output = events.last().unwrap().data["response"]["output"]
            .as_array()
            .unwrap();
        assert_eq!(
            output
                .iter()
                .map(|item| item["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "message",
                "reasoning",
                "message",
                "function_call",
                "message"
            ]
        );
        assert_eq!(output[0]["content"][0]["text"], "before");
        assert_eq!(output[2]["content"][0]["text"], "between");
        assert_eq!(output[4]["content"][0]["text"], "after");
        assert_eq!(output[3]["arguments"], "{}");
    }
}

mod protocol_continuation_review {
    use super::*;

    fn thinking_blocks() -> Vec<Value> {
        vec![
            json!({"type":"thinking","thinking":"first 中文","signature":"sig_first"}),
            json!({"type":"thinking","thinking":"<cite>second</cite>","signature":"sig_second"}),
            json!({"type":"redacted_thinking","data":"opaque_redacted_data"}),
            json!({"type":"thinking","thinking":"","signature":"sig_empty"}),
        ]
    }

    fn assert_thinking_replay(response: &Value, expected: &[Value]) {
        let mut input = vec![json!({"role":"user","content":"look up the answer"})];
        input.extend(response["output"].as_array().unwrap().iter().cloned());
        input.push(json!({"type":"function_call_output","call_id":"call_lookup","output":"done"}));
        let replay = responses_to_anthropic_messages(json!({
            "model":"claude-sonnet-4-6","input":input
        }))
        .unwrap();
        let blocks = replay["messages"][1]["content"].as_array().unwrap();
        assert_eq!(&blocks[..expected.len().min(blocks.len())], expected);
        assert_eq!(blocks[expected.len()]["type"], "tool_use");
        let ids: Vec<_> = response["output"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        assert_eq!(
            ids.len(),
            ids.iter().collect::<std::collections::BTreeSet<_>>().len()
        );
    }

    #[test]
    fn nonstream_signed_and_redacted_thinking_roundtrips_exactly() {
        let blocks = thinking_blocks();
        let mut content = blocks.clone();
        content.push(
            json!({"type":"tool_use","id":"call_lookup","name":"lookup","input":{"q":"test"}}),
        );
        let response = anthropic_message_to_response_with_request(
            json!({
                "id":"msg_thinking","content":content,"stop_reason":"tool_use"
            }),
            &json!({}),
        )
        .unwrap();
        assert_thinking_replay(&response, &blocks);
    }

    #[test]
    fn fragmented_stream_preserves_each_thinking_block_and_signature() {
        let blocks = thinking_blocks();
        let mut chunks =
            vec![json!({"type":"message_start","message":{"id":"msg_thinking","content":[]}})];
        for (index, block) in blocks.iter().enumerate() {
            if block["type"] == "thinking" {
                chunks.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"thinking","thinking":""}}));
                chunks.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"thinking_delta","thinking":block["thinking"]}}));
                let signature = block["signature"].as_str().unwrap();
                for part in [&signature[..4], &signature[4..]] {
                    chunks.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":part}}));
                }
            } else {
                chunks.push(
                    json!({"type":"content_block_start","index":index,"content_block":block}),
                );
            }
            chunks.push(json!({"type":"content_block_stop","index":index}));
        }
        chunks.extend([
            json!({"type":"content_block_start","index":4,"content_block":{"type":"tool_use","id":"call_lookup","name":"lookup","input":{}}}),
            json!({"type":"content_block_delta","index":4,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"test\"}"}}),
            json!({"type":"content_block_stop","index":4}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ]);
        let wire: String = chunks
            .iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .collect();
        for width in [1, 7, 512] {
            let mut converter = AnthropicSseToResponsesConverter::default();
            let mut output = Vec::new();
            for bytes in wire.as_bytes().chunks(width) {
                output.extend(converter.push_bytes(bytes));
            }
            output.extend(converter.finish());
            let events = parse_response_sse_events(&String::from_utf8(output).unwrap());
            let response = &events.last().unwrap().data["response"];
            assert_eq!(response["status"], "completed");
            assert_thinking_replay(response, &blocks);
            let done: Vec<_> = events
                .iter()
                .filter(|event| event.event == "response.output_item.done")
                .map(|event| event.data["item"].clone())
                .collect();
            assert_eq!(done, *response["output"].as_array().unwrap());
        }
    }

    #[test]
    fn truncated_tools_are_not_published_as_executable_calls() {
        for (name, tool) in [
            (
                "lookup",
                json!({"type":"function","name":"lookup","parameters":{"type":"object"}}),
            ),
            ("exec", json!({"type":"custom","name":"exec"})),
        ] {
            let request = json!({"tools":[tool]});
            let function = json!({"name":name,"arguments":"{\"input\":\"partial"});
            let json_response = chat_completion_to_response_with_request(json!({
                "choices":[{"message":{"content":"partial","tool_calls":[{"id":"call_cut","function":function}]},"finish_reason":"length"}]
            }), &request).unwrap();
            let wire = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({
                    "choices":[{"delta":{"content":"partial","tool_calls":[{"index":0,"id":"call_cut","function":function}]},"finish_reason":"length"}]
                })
            );
            let stream = chat_sse_to_responses_sse_with_request(&wire, &request);
            let events = parse_response_sse_events(&stream);
            assert!(!events.iter().any(|event| matches!(
                event.event.as_str(),
                "response.function_call_arguments.done" | "response.custom_tool_call_input.done"
            )));
            for response in [&json_response, &events.last().unwrap().data["response"]] {
                assert_eq!(response["status"], "incomplete");
                for item in response["output"].as_array().unwrap() {
                    assert_ne!(item["type"], "function_call");
                    assert_ne!(item["type"], "custom_tool_call");
                    if item["type"] == "message" {
                        assert_eq!(item["status"], "incomplete");
                    }
                }
            }
            let response = anthropic_message_to_response_with_request(json!({
                "content":[{"type":"text","text":"partial"},{"type":"tool_use","id":"call_cut","name":name,"input":{}}],
                "stop_reason":"max_tokens"
            }), &request).unwrap();
            let wire = [
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_cut","name":name,"input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"input\":\"partial"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"}}),
                json!({"type":"message_stop"}),
            ].iter().map(|chunk| format!("data: {chunk}\n\n")).collect::<String>();
            let stream = anthropic_sse_to_responses_sse_with_request(&wire, &request);
            let events = parse_response_sse_events(&stream);
            assert!(!events.iter().any(|event| matches!(
                event.event.as_str(),
                "response.function_call_arguments.done" | "response.custom_tool_call_input.done"
            )));
            for response in [&response, &events.last().unwrap().data["response"]] {
                assert_eq!(response["status"], "incomplete");
                assert!(
                    response["output"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|item| matches!(item["type"].as_str(), Some("message" | "reasoning")))
                );
            }
        }
    }

    #[test]
    fn message_stop_with_unclosed_tool_block_fails_without_publishing_call() {
        let wire = [
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_cut","name":"lookup","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"partial"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ].iter().map(|chunk| format!("data: {chunk}\n\n")).collect::<String>();
        let stream = anthropic_sse_to_responses_sse_with_request(&wire, &json!({}));
        let events = parse_response_sse_events(&stream);
        assert_eq!(events.last().unwrap().event, "response.failed");
        assert!(!stream.contains("event: response.function_call_arguments.done"));
        assert!(!stream.contains("event: response.completed"));
    }

    #[test]
    fn legacy_function_call_stream_matches_nonstream() {
        let json_response = chat_completion_to_response(json!({
            "choices":[{"message":{"function_call":{"name":"lookup","arguments":"{\"q\":\"test\"}"}},"finish_reason":"function_call"}]
        })).unwrap();
        let stream = chat_sse_to_responses_sse(
            "data: {\"choices\":[{\"delta\":{\"function_call\":{\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":\"}}}]}\n\ndata: {\"choices\":[{\"delta\":{\"function_call\":{\"arguments\":\"\\\"test\\\"}\"}},\"finish_reason\":\"function_call\"}]}\n\ndata: [DONE]\n\n",
        );
        let events = parse_response_sse_events(&stream);
        assert_eq!(
            events.last().unwrap().data["response"]["output"],
            json_response["output"]
        );
    }

    #[test]
    fn alternating_text_and_refusal_items_have_unique_ids() {
        let mut wire = String::new();
        for delta in [
            json!({"content":"before"}),
            json!({"refusal":"refused"}),
            json!({"content":"after"}),
        ] {
            wire.push_str(&format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":delta}]})
            ));
        }
        wire.push_str("data: [DONE]\n\n");
        let events = parse_response_sse_events(&chat_sse_to_responses_sse(&wire));
        let output = events.last().unwrap().data["response"]["output"]
            .as_array()
            .unwrap();
        assert_eq!(output.len(), 3);
        let ids: std::collections::BTreeSet<_> = output
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(output[0]["content"][0]["text"], "before");
        assert_eq!(output[1]["content"][0]["refusal"], "refused");
        assert_eq!(output[2]["content"][0]["text"], "after");
    }
}

#[test]
fn protocol_review_content_filter_is_incomplete_in_json_and_stream() {
    let converted = chat_completion_to_response(json!({
        "choices": [{"message": {"content": "partial"}, "finish_reason": "content_filter"}]
    }))
    .unwrap();
    assert_eq!(converted["status"], "incomplete");
    assert_eq!(converted["incomplete_details"]["reason"], "content_filter");

    let stream = chat_sse_to_responses_sse(
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n",
    );
    let events = parse_response_sse_events(&stream);
    let terminal = events.last().unwrap();
    assert_eq!(terminal.event, "response.incomplete");
    assert_eq!(
        terminal.data["response"]["incomplete_details"]["reason"],
        "content_filter"
    );
    assert!(!stream.contains("event: response.completed"));
}

#[test]
fn protocol_review_anthropic_truncation_is_consistent_in_json_and_stream() {
    for (stop_reason, reason) in [
        ("max_tokens", "max_output_tokens"),
        ("model_context_window_exceeded", "max_output_tokens"),
        ("refusal", "content_filter"),
    ] {
        let converted = anthropic_message_to_response_with_request(
            json!({"content": [{"type": "text", "text": "partial"}], "stop_reason": stop_reason}),
            &json!({}),
        )
        .unwrap();
        let stream = anthropic_sse_to_responses_sse_with_request(
            &format!(
                "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
                json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}})
            ),
            &json!({}),
        );
        let events = parse_response_sse_events(&stream);
        assert_eq!(
            events.last().unwrap().event,
            "response.incomplete",
            "{stop_reason}"
        );
        for response in [&converted, &events.last().unwrap().data["response"]] {
            assert_eq!(response["status"], "incomplete", "{stop_reason}");
            assert_eq!(
                response["incomplete_details"]["reason"], reason,
                "{stop_reason}"
            );
        }
    }
}

#[test]
fn protocol_review_anthropic_pause_is_not_reported_as_completed() {
    let error = anthropic_message_to_response_with_request(
        json!({"content": [{"type": "text", "text": "Searching…"}], "stop_reason": "pause_turn"}),
        &json!({}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("pause_turn"));
    let stream = anthropic_sse_to_responses_sse_with_request(
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        &json!({}),
    );
    let events = parse_response_sse_events(&stream);
    assert_eq!(events.last().unwrap().event, "response.failed");
    assert_eq!(
        events.last().unwrap().data["response"]["error"]["type"],
        "unsupported_upstream_continuation"
    );
    assert!(!stream.contains("response.completed"));
    let stream = anthropic_sse_to_responses_sse_with_request(
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"}}\n\ndata: [DONE]\n\n",
        &json!({}),
    );
    assert!(stream.contains("response.failed"));
    assert!(!stream.contains("response.completed"));
}

#[test]
fn protocol_review_chat_preserves_responses_text_format() {
    let schema = json!({
        "name": "answer", "description": "structured answer", "strict": true,
        "schema": {"type": "object", "properties": {"answer": {"type": "string"}},
                   "required": ["answer"], "additionalProperties": false}
    });
    let mut format = schema.clone();
    format["type"] = json!("json_schema");
    let converted = responses_to_chat_completions(json!({
        "model": "chat-model", "input": "Return JSON",
        "text": {"format": format},
        "response_format": {"type": "text"}
    }))
    .unwrap();
    assert_eq!(
        converted["response_format"],
        json!({"type": "json_schema", "json_schema": schema})
    );
    for kind in ["json_object", "text"] {
        let converted = responses_to_chat_completions(json!({
            "input": "Return JSON", "text": {"format": {"type": kind}}
        }))
        .unwrap();
        assert_eq!(converted["response_format"], json!({"type": kind}));
    }
    let converted = responses_to_chat_completions(json!({
        "input": "Return JSON", "response_format": {"type": "json_object"}
    }))
    .unwrap();
    assert_eq!(converted["response_format"], json!({"type": "json_object"}));
}

#[test]
fn protocol_review_anthropic_preserves_schema_and_reasoning_effort() {
    let schema = json!({
        "type": "object", "properties": {"answer": {"type": "string"}},
        "required": ["answer"], "additionalProperties": false
    });
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-4-6", "input": "Return JSON",
        "reasoning": {"effort": "high"},
        "text": {"format": {"type": "json_schema", "name": "answer", "strict": true, "schema": schema}}
    })).unwrap();
    assert_eq!(
        converted["output_config"]["format"],
        json!({"type": "json_schema", "schema": schema})
    );
    assert_eq!(converted["output_config"]["effort"], "high");
    assert!(
        responses_to_anthropic_messages(json!({
            "input": "hi", "text": {"format": {"type": "text"}}
        }))
        .unwrap()
        .pointer("/output_config/format")
        .is_none()
    );
    for format in [
        json!({"type": "json_object"}),
        json!({"type": "json_schema"}),
    ] {
        assert!(
            responses_to_anthropic_messages(json!({
                "input": "hi", "text": {"format": format}
            }))
            .is_err(),
            "{format}"
        );
    }
}

#[test]
fn protocol_review_chat_usage_includes_cached_input_in_json_and_stream() {
    for (usage, expected_input, cached, written) in [
        (
            json!({"prompt_tokens": 100, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 80}}),
            100,
            80,
            0,
        ),
        (
            json!({"input_tokens": 100, "output_tokens": 5, "input_tokens_details": {"cached_tokens": 80}}),
            100,
            80,
            0,
        ),
        (
            json!({"promptTokenCount": 100, "candidatesTokenCount": 5, "cachedContentTokenCount": 80}),
            100,
            80,
            0,
        ),
        (
            json!({"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 80, "cache_creation_input_tokens": 10}),
            100,
            80,
            10,
        ),
        (
            json!({"input_tokens": 10, "output_tokens": 5, "cache_creation": {"ephemeral_5m_input_tokens": 40, "ephemeral_1h_input_tokens": 50}}),
            100,
            0,
            90,
        ),
    ] {
        let converted = chat_completion_to_response(json!({
            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": usage
        }))
        .unwrap();
        let stream = chat_sse_to_responses_sse(&format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"delta": {"content": "ok"}, "finish_reason": "stop"}]}),
            json!({"choices": [], "usage": usage})
        ));
        let events = parse_response_sse_events(&stream);
        for actual in [
            &converted["usage"],
            &events.last().unwrap().data["response"]["usage"],
        ] {
            assert_eq!(actual["input_tokens"], expected_input, "{usage}");
            assert_eq!(
                actual["input_tokens_details"]["cached_tokens"], cached,
                "{usage}"
            );
            assert_eq!(
                actual["input_tokens_details"]["cache_write_tokens"], written,
                "{usage}"
            );
            assert_eq!(actual["total_tokens"], expected_input + 5, "{usage}");
        }
    }
}

#[test]
fn protocol_review_anthropic_nonstream_rejects_errors_and_invalid_envelopes() {
    for body in [
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
        json!({"error": {"message": "busy"}, "content": []}),
        json!({}),
        json!({"content": "not an array"}),
        json!({"type": "error", "content": []}),
    ] {
        assert!(
            anthropic_message_to_response_with_request(body.clone(), &json!({})).is_err(),
            "{body}"
        );
    }
    assert!(
        anthropic_message_to_response_with_request(
            json!({"type": "message", "content": [], "stop_reason": "end_turn", "error": null}),
            &json!({})
        )
        .is_ok()
    );
}

#[test]
fn protocol_review_stream_terminal_is_not_reopened_by_trailing_chunks() {
    let chat = b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let anthropic = b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let tail = b"data: {\"error\":{\"message\":\"late error\"}}\n\ndata: {broken}\n\n";
    for split in [false, true] {
        let mut converter = ChatSseToResponsesConverter::default();
        let mut input = chat.to_vec();
        if !split {
            input.extend(tail);
        }
        let mut output = converter.push_bytes(&input);
        if split {
            output.extend(converter.push_bytes(tail));
        }
        output.extend(converter.fail("late read error".into(), None));
        output.extend(converter.finish());
        let events = parse_response_sse_events(&String::from_utf8(output).unwrap());
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event.event.as_str(),
                    "response.completed" | "response.failed"
                ))
                .count(),
            1
        );
        assert_eq!(
            converter.diagnostic_summary()["terminalStatus"],
            "response_completed"
        );

        let mut converter = AnthropicSseToResponsesConverter::default();
        let mut input = anthropic.to_vec();
        if !split {
            input.extend(tail);
        }
        let mut output = converter.push_bytes(&input);
        if split {
            output.extend(converter.push_bytes(tail));
        }
        output.extend(converter.fail("late read error".into(), None));
        output.extend(converter.finish());
        let events = parse_response_sse_events(&String::from_utf8(output).unwrap());
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event.event.as_str(),
                    "response.completed" | "response.failed"
                ))
                .count(),
            1
        );
        assert_eq!(
            converter.diagnostic_summary()["terminalStatus"],
            "response_completed"
        );
    }
    let mut chat_converter = ChatSseToResponsesConverter::default();
    let failed = chat_converter.fail("first failure".into(), None);
    assert!(
        String::from_utf8(failed)
            .unwrap()
            .contains("response.failed")
    );
    assert!(chat_converter.push_bytes(chat).is_empty());
    assert!(
        chat_converter
            .fail("second failure".into(), None)
            .is_empty()
    );
    assert!(chat_converter.finish().is_empty());
    assert_eq!(
        chat_converter.diagnostic_summary()["failureMessage"],
        "first failure"
    );

    let mut anthropic_converter = AnthropicSseToResponsesConverter::default();
    let failed = anthropic_converter.fail("first failure".into(), None);
    assert!(
        String::from_utf8(failed)
            .unwrap()
            .contains("response.failed")
    );
    assert!(anthropic_converter.push_bytes(anthropic).is_empty());
    assert!(
        anthropic_converter
            .fail("second failure".into(), None)
            .is_empty()
    );
    assert!(anthropic_converter.finish().is_empty());
    assert_eq!(
        anthropic_converter.diagnostic_summary()["failureMessage"],
        "first failure"
    );
}

#[test]
fn protocol_review_null_error_is_not_a_stream_failure() {
    let stream = chat_sse_to_responses_sse(
        "data: {\"error\":null,\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    );
    assert!(!stream.contains("response.failed"));
    assert_eq!(collect_stream_output_text(&stream), "ok");
    let stream = anthropic_sse_to_responses_sse_with_request(
        "event: message_start\ndata: {\"error\":null,\"type\":\"message_start\",\"message\":{\"id\":\"msg_ok\",\"content\":[]}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        &json!({}),
    );
    assert!(!stream.contains("response.failed"));
    assert!(stream.contains("response.completed"));
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

/// 把一组文本分片包成一个完整的 Anthropic 流式响应（单个 text 块）。
fn anthropic_sse_from_text_deltas(deltas: &[String]) -> String {
    let mut sse = String::new();
    sse.push_str("event: message_start\ndata: ");
    sse.push_str(
        &json!({
            "type": "message_start",
            "message": {
                "id": "msg_split",
                "type": "message",
                "role": "assistant",
                "model": "claude-opus-4-8",
                "content": [],
                "usage": { "input_tokens": 7 }
            }
        })
        .to_string(),
    );
    sse.push_str("\n\nevent: content_block_start\ndata: ");
    sse.push_str(
        &json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "text", "text": "" }
        })
        .to_string(),
    );
    sse.push_str("\n\n");
    for delta in deltas {
        sse.push_str("event: content_block_delta\ndata: ");
        sse.push_str(
            &json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": delta }
            })
            .to_string(),
        );
        sse.push_str("\n\n");
    }
    sse.push_str("event: content_block_stop\ndata: ");
    sse.push_str(&json!({ "type": "content_block_stop", "index": 0 }).to_string());
    sse.push_str("\n\nevent: message_delta\ndata: ");
    sse.push_str(
        &json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 9 }
        })
        .to_string(),
    );
    sse.push_str("\n\nevent: message_stop\ndata: ");
    sse.push_str(&json!({ "type": "message_stop" }).to_string());
    sse.push_str("\n\n");
    sse
}

fn collect_stream_output_text(converted: &str) -> String {
    parse_response_sse_events(converted)
        .into_iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str().map(ToString::to_string))
        .collect()
}

/// 按给定切点把文本拆成多个 delta，跑完整流式转换，返回拼接后的正文。
fn stream_text_with_cuts(text: &str, cuts: &[usize]) -> String {
    let mut deltas = Vec::new();
    let mut prev = 0;
    for &cut in cuts {
        deltas.push(text[prev..cut].to_string());
        prev = cut;
    }
    deltas.push(text[prev..].to_string());
    let converted = anthropic_sse_to_responses_sse_with_compat(
        &anthropic_sse_from_text_deltas(&deltas),
        &json!({ "model": "claude-opus-4-8" }),
    );
    collect_stream_output_text(&converted)
}

fn char_boundaries(text: &str) -> Vec<usize> {
    (1..text.len())
        .filter(|index| text.is_char_boundary(*index))
        .collect()
}

/// 验证所有「单切点」分片（覆盖标签在任意位置被一分为二）与逐字符分片（最恶劣形态）。
///
/// 不做更高阶的组合穷举：多切点场景已被「逐字符」这个最强约束覆盖，
/// 而穷举组合会把单个用例拖到十秒级，反过来干扰同进程里对时序敏感的网络用例。
fn assert_all_fragmentations_yield(text: &str, expected: &str) {
    let boundaries = char_boundaries(text);
    assert_eq!(
        stream_text_with_cuts(text, &[]),
        expected,
        "不分片时输出不符：text={text:?}"
    );
    for &boundary in &boundaries {
        assert_eq!(
            stream_text_with_cuts(text, &[boundary]),
            expected,
            "在字节 {boundary} 处分片时输出不符：text={text:?}"
        );
    }
    assert_eq!(
        stream_text_with_cuts(text, &boundaries),
        expected,
        "逐字符分片时输出不符：text={text:?}"
    );
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

fn remote_compaction_v2_request() -> Value {
    json!({
        "model": "claude-sonnet-5",
        "stream": true,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "fix the proxy" }]
            },
            { "type": "compaction_trigger" }
        ],
        "tools": [{
            "type": "function",
            "name": "exec_command",
            "description": "Run a command",
            "parameters": { "type": "object" }
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true
    })
}

#[test]
fn remote_compaction_v2_claude_trigger_preserves_tools_and_replaces_instruction() {
    let chat = responses_to_chat_completions(remote_compaction_v2_request()).unwrap();
    assert!(chat.get("tools").is_some());
    assert!(chat.get("tool_choice").is_some());
    assert_eq!(chat["parallel_tool_calls"], true);
    let chat_messages = chat.get("messages").and_then(Value::as_array).unwrap();
    assert!(
        chat_messages
            .last()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .is_some_and(|text| {
                text.starts_with(
                    codex_elves_core::layered_compaction::COMPACTION_INSTRUCTION_PREFIX,
                )
            })
    );
    assert!(!chat.to_string().contains("compaction_trigger"));

    let anthropic = responses_to_anthropic_messages(remote_compaction_v2_request()).unwrap();
    assert!(anthropic.get("tools").is_some());
    assert!(anthropic.get("tool_choice").is_some());
    let anthropic_messages = anthropic.get("messages").and_then(Value::as_array).unwrap();
    assert!(
        anthropic_messages
            .last()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content.iter().any(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| {
                            text.starts_with(
                                codex_elves_core::layered_compaction::COMPACTION_INSTRUCTION_PREFIX,
                            )
                        })
                })
            })
    );
    assert!(!anthropic.to_string().contains("compaction_trigger"));
}

#[test]
fn remote_compaction_v2_response_is_single_compaction_and_restores_history() {
    let compacted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl-compact",
            "created": 123,
            "model": "claude-sonnet-5",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "<summary>SUMMARY FROM BRIDGE</summary>"

                }
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120
            }
        }),
        &remote_compaction_v2_request(),
    )
    .unwrap();

    let output = compacted.get("output").and_then(Value::as_array).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["type"], "compaction");

    let history_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "historical user context" }]
            },
            output[0].clone(),
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "continue" }]
            }
        ]
    });
    let chat = responses_to_chat_completions(history_request.clone()).unwrap();
    assert!(chat["messages"].as_array().unwrap().iter().any(|message| {
        message["role"] == "assistant"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.contains("SUMMARY FROM BRIDGE"))
    }));

    let anthropic = responses_to_anthropic_messages(history_request).unwrap();
    assert!(
        anthropic["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "user"
                    && message["content"].as_array().is_some_and(|content| {
                        content.iter().any(|part| {
                            part["text"]
                                .as_str()
                                .is_some_and(|text| text.contains("SUMMARY FROM BRIDGE"))
                        })
                    })
            })
    );
}

#[test]
fn structured_compaction_restores_real_roles_and_tool_pairing_across_protocols() {
    let current_user = json!({
        "type": "message",
        "id": "msg-current-user",
        "role": "user",
        "content": [{ "type": "input_text", "text": "按推荐处理" }]
    });
    let source_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "接下来做什么" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "推荐方案：执行方案 1" }]
            },
            current_user.clone(),
            {
                "type": "function_call",
                "call_id": "call-probe",
                "name": "shell_command",
                "arguments": "{\"command\":\"run probe\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call-probe",
                "output": "probe ready"
            },
            { "type": "compaction_trigger" }
        ]
    });
    let source_response = json!({
        "id": "resp-structured",
        "status": "completed",
        "model": "claude-sonnet-5",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "<summary>较早历史摘要</summary>" }]
        }]
    });
    let compacted = rewrite_remote_compaction_v2_response_with_layered_compaction(
        &source_request,
        &source_response,
        true,
        DEFAULT_RETAIN_TOKENS,
    )
    .expect("structured compaction response");
    let history_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "更早保留的 user" }]
            },
            current_user,
            compacted.response["output"][0].clone()
        ]
    });

    let chat = responses_to_chat_completions(history_request.clone()).unwrap();
    let chat_messages = chat["messages"].as_array().unwrap();
    let chat_text = serde_json::to_string(chat_messages).unwrap();
    assert_eq!(chat_text.matches("按推荐处理").count(), 1);
    assert!(chat_text.contains("推荐方案：执行方案 1"));
    assert!(chat_text.contains("较早历史摘要"));
    assert!(chat_text.contains("\"id\":\"call-probe\""));
    assert!(chat_messages.iter().any(|message| {
        message["role"] == "tool"
            && message["tool_call_id"] == "call-probe"
            && message["content"] == "probe ready"
    }));

    let anthropic = responses_to_anthropic_messages(history_request).unwrap();
    let anthropic_messages = anthropic["messages"].as_array().unwrap();
    let anthropic_text = serde_json::to_string(anthropic_messages).unwrap();
    assert_eq!(anthropic_text.matches("按推荐处理").count(), 1);
    assert!(anthropic_text.contains("推荐方案：执行方案 1"));
    assert!(anthropic_text.contains("较早历史摘要"));
    assert!(anthropic_text.contains("\"id\":\"call-probe\""));
    assert_eq!(anthropic["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(anthropic["output_config"]["effort"], "high");
    let last = anthropic_messages.last().unwrap();
    assert_eq!(last["role"], "user");
    assert!(
        last["content"]
            .as_array()
            .is_some_and(|content| content.iter().any(|block| {
                block["type"] == "tool_result"
                    && block["tool_use_id"] == "call-probe"
                    && block["content"] == "probe ready"
            }))
    );
}

fn compacted_thinking_history(anchor_reasoning: bool, tool_reasoning: bool) -> Value {
    let reasoning = |text: &str, signature: &str| {
        json!({
            "type": "reasoning",
            "summary": [{ "type": "summary_text", "text": text }],
            "encrypted_content": signature
        })
    };
    let mut tail = Vec::new();
    if anchor_reasoning {
        tail.push(reasoning("anchor reasoning", "anchor-signature"));
    }
    tail.push(json!({
        "type": "message", "role": "assistant",
        "content": [{ "type": "output_text", "text": "Previous answer" }]
    }));
    tail.push(json!({
        "type": "message", "role": "user",
        "content": [{ "type": "input_text", "text": "Continue the task" }]
    }));
    if tool_reasoning {
        tail.push(reasoning("tool reasoning", "tool-signature"));
    }
    tail.push(json!({
        "type": "function_call", "call_id": "call-history",
        "name": "lookup", "arguments": "{\"query\":\"history\"}"
    }));
    tail.push(json!({
        "type": "function_call_output", "call_id": "call-history",
        "output": [
            { "type": "input_text", "text": "Historical result" },
            { "type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=" }
        ]
    }));
    json!({
        "model": "deepseek-v4.1-flash",
        "reasoning": { "effort": "max" },
        "tools": [{ "type": "function", "name": "lookup", "parameters": { "type": "object" } }],
        "input": [{
            "type": "compaction",
            "encrypted_content": format!("codex-elves-compaction-v3:{}", json!({
                "summary": "Earlier history summary", "retained_tail": tail
            }))
        }]
    })
}

#[test]
fn chat_compacted_thinking_history_does_not_create_an_unsigned_assistant_summary() {
    let converted = responses_to_chat_completions(compacted_thinking_history(true, true)).unwrap();
    let messages = converted["messages"].as_array().unwrap();
    let assistants: Vec<_> = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .collect();
    assert_eq!(assistants.len(), 2);
    assert_eq!(assistants[0]["reasoning_content"], "anchor reasoning");
    assert_eq!(assistants[1]["reasoning_content"], "tool reasoning");
    assert_eq!(assistants[1]["tool_calls"][0]["id"], "call-history");
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "user" && m.to_string().contains("Earlier history summary"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-history")
    );
}

#[test]
fn chat_compacted_legacy_missing_reasoning_keeps_context_images_and_complete_turns() {
    for complete_tool_turn in [false, true] {
        let converted =
            responses_to_chat_completions(compacted_thinking_history(false, complete_tool_turn))
                .unwrap();
        let messages = converted["messages"].as_array().unwrap();
        let assistants: Vec<_> = messages
            .iter()
            .filter(|m| m["role"] == "assistant")
            .collect();
        assert_eq!(assistants.len(), usize::from(complete_tool_turn));
        assert!(
            messages
                .iter()
                .any(|m| m["role"] == "user" && m.to_string().contains("Previous answer"))
        );
        assert!(messages.iter().any(|m| m["role"] == "user"
            && m["content"].as_array().is_some_and(|parts| {
                parts.iter().any(|p| {
                    p["type"] == "image_url"
                        && p["image_url"]["url"] == "data:image/png;base64,aGVsbG8="
                })
            })));
        if complete_tool_turn {
            assert_eq!(assistants[0]["reasoning_content"], "tool reasoning");
            assert!(
                messages
                    .iter()
                    .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-history")
            );
        } else {
            assert!(messages.iter().all(|m| m["role"] != "tool"));
            assert!(
                messages
                    .iter()
                    .any(|m| m["role"] == "user" && m.to_string().contains("call-history"))
            );
        }
    }
}

#[test]
fn anthropic_compacted_thinking_history_keeps_signed_blocks_and_requested_effort() {
    let converted =
        responses_to_anthropic_messages(compacted_thinking_history(true, true)).unwrap();
    assert_eq!(converted["thinking"]["type"], "adaptive");
    assert_eq!(converted["output_config"]["effort"], "max");
    let messages = converted["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "user");
    assert!(
        messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Earlier history summary")
    );
    let assistants: Vec<_> = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .collect();
    assert_eq!(assistants.len(), 2);
    assert_eq!(
        assistants[0]["content"][0],
        json!({
            "type": "thinking", "thinking": "anchor reasoning", "signature": "anchor-signature"
        })
    );
    assert_eq!(
        assistants[1]["content"][0],
        json!({
            "type": "thinking", "thinking": "tool reasoning", "signature": "tool-signature"
        })
    );
    assert_eq!(assistants[1]["content"][1]["id"], "call-history");
    assert_eq!(
        messages.last().unwrap()["content"][0]["tool_use_id"],
        "call-history"
    );
}

#[test]
fn anthropic_repeated_compaction_preserves_the_anchor_reasoning_before_its_answer() {
    let mut request = compacted_thinking_history(true, true);
    let summary_response = json!({
        "id": "resp-summary", "status": "completed",
        "output": [{
            "type": "message", "role": "assistant",
            "content": [{ "type": "output_text", "text": "<summary>Updated history summary</summary>" }]
        }]
    });
    for _ in 0..2 {
        request["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": "compaction_trigger" }));
        let compacted = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &summary_response,
            true,
            DEFAULT_RETAIN_TOKENS,
        )
        .unwrap();
        request["input"] = compacted.response["output"].clone();
        let converted = responses_to_anthropic_messages(request.clone()).unwrap();
        let assistants: Vec<_> = converted["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "assistant")
            .collect();
        assert_eq!(assistants.len(), 2);
        assert_eq!(assistants[0]["content"][0]["thinking"], "anchor reasoning");
        assert_eq!(assistants[0]["content"][0]["signature"], "anchor-signature");
        assert_eq!(assistants[0]["content"][1]["text"], "Previous answer");
        assert_eq!(assistants[1]["content"][0]["signature"], "tool-signature");
        assert_eq!(converted["output_config"]["effort"], "max");
    }
}

#[test]
fn anthropic_compaction_recovery_is_scoped_to_deepseek_thinking_with_tools() {
    for scenario in ["disabled", "no-tools", "claude", "uncompacted"] {
        let mut request = compacted_thinking_history(false, false);
        match scenario {
            "disabled" => request["reasoning"] = json!({ "effort": "none" }),
            "no-tools" => {
                request.as_object_mut().unwrap().remove("tools");
            }
            "claude" => request["model"] = json!("claude-sonnet-5"),
            _ => {
                request =
                    codex_elves_core::layered_compaction::expand_synthetic_local_compaction_request(
                        &request,
                    );
            }
        }
        let converted = responses_to_anthropic_messages(request).unwrap();
        assert_eq!(
            converted["thinking"]["type"],
            if scenario == "disabled" {
                "disabled"
            } else {
                "adaptive"
            },
            "{scenario}"
        );
        assert!(
            converted["messages"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|m| m["content"].as_array().unwrap())
                .any(|b| b["type"] == "tool_use" && b["id"] == "call-history"),
            "{scenario}"
        );
    }
}

#[test]
fn anthropic_compacted_legacy_anchor_becomes_context_without_disabling_thinking() {
    let converted =
        responses_to_anthropic_messages(compacted_thinking_history(false, true)).unwrap();
    assert_eq!(converted["thinking"]["type"], "adaptive");
    assert_eq!(converted["output_config"]["effort"], "max");
    let messages = converted["messages"].as_array().unwrap();
    let assistants: Vec<_> = messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .collect();
    assert_eq!(assistants.len(), 1);
    assert_eq!(assistants[0]["content"][0]["thinking"], "tool reasoning");
    assert_eq!(assistants[0]["content"][1]["id"], "call-history");
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "user" && m.to_string().contains("Previous answer"))
    );
    assert_eq!(
        messages.last().unwrap()["content"][0]["tool_use_id"],
        "call-history"
    );
}

#[test]
fn anthropic_compacted_legacy_tool_pair_becomes_context_and_keeps_images() {
    let converted =
        responses_to_anthropic_messages(compacted_thinking_history(false, false)).unwrap();
    assert_eq!(converted["thinking"]["type"], "adaptive");
    let messages = converted["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["role"] == "user"));
    let text = messages
        .iter()
        .flat_map(|m| m["content"].as_array().unwrap())
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Previous answer"));
    assert!(text.contains("call-history"));
    assert!(text.contains("lookup"));
    assert!(text.contains("Historical result"));
    assert!(!text.contains("aGVsbG8="));
    assert!(
        messages
            .iter()
            .flat_map(|m| m["content"].as_array().unwrap())
            .any(|b| b["type"] == "image" && b["source"]["data"] == "aGVsbG8=")
    );
    assert!(
        !messages
            .iter()
            .flat_map(|m| m["content"].as_array().unwrap())
            .any(|b| b["type"] == "tool_use" || b["type"] == "tool_result")
    );
}

#[test]
fn structured_compaction_trims_legacy_tool_result_without_breaking_protocol_pairing() {
    let current_user = json!({
        "type": "message",
        "id": "msg-current-user-legacy",
        "role": "user",
        "content": [{ "type": "input_text", "text": "继续查询" }]
    });
    let source_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "查询推荐方案" }]
            },
            current_user.clone(),
            {
                "type": "tool_call",
                "tool_use": {
                    "id": "call-legacy",
                    "name": "lookup",
                    "input": { "query": "weather" }
                }
            },
            {
                "type": "tool_result",
                "content": {
                    "tool_use_id": "call-legacy",
                    "content": format!("BEGIN\n{}\nEND", "界".repeat(30_000))
                }
            },
            { "type": "compaction_trigger" }
        ]
    });
    let source_response = json!({
        "id": "resp-legacy-trimmed",
        "status": "completed",
        "model": "claude-sonnet-5",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "<summary>较早历史摘要</summary>" }]
        }]
    });
    let compacted = rewrite_remote_compaction_v2_response_with_layered_compaction(
        &source_request,
        &source_response,
        true,
        MIN_RETAIN_TOKENS,
    )
    .expect("structured compaction response");
    assert!(compacted.layered.triggered);
    let history_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "更早保留的 user" }]
            },
            current_user,
            compacted.response["output"][0].clone()
        ]
    });

    let chat = responses_to_chat_completions(history_request.clone()).unwrap();
    let chat_messages = chat["messages"].as_array().unwrap();
    assert!(chat_messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["tool_calls"].as_array().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call["id"] == "call-legacy" && call["function"]["name"] == "lookup")
            })
    }));
    let chat_result = chat_messages
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "call-legacy")
        .expect("chat tool result remains paired");
    let chat_content = chat_result["content"].as_str().unwrap();
    assert!(chat_content.starts_with("BEGIN"));
    assert!(chat_content.ends_with("END"));
    assert!(chat_content.contains("<truncated:tool;~"));

    let anthropic = responses_to_anthropic_messages(history_request).unwrap();
    let anthropic_messages = anthropic["messages"].as_array().unwrap();
    assert!(anthropic_messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["content"].as_array().is_some_and(|content| {
                content.iter().any(|block| {
                    block["type"] == "tool_use"
                        && block["id"] == "call-legacy"
                        && block["name"] == "lookup"
                })
            })
    }));
    let anthropic_result = anthropic_messages
        .iter()
        .flat_map(|message| message["content"].as_array().into_iter().flatten())
        .find(|block| block["type"] == "tool_result" && block["tool_use_id"] == "call-legacy")
        .expect("anthropic tool result remains paired");
    let anthropic_content = anthropic_result["content"].as_str().unwrap();
    assert!(anthropic_content.starts_with("BEGIN"));
    assert!(anthropic_content.ends_with("END"));
    assert!(anthropic_content.contains("<truncated:tool;~"));
}

#[test]
fn structured_compaction_trims_tool_search_descriptions_without_losing_dynamic_tools() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "step": {
                "type": "string",
                "description": format!(
                    "PARAMETER_SCHEMA_MUST_REMAIN_UNCHANGED data:image/png;base64,{} END_SCHEMA",
                    "S".repeat(12_000)
                )
            }
        },
        "required": ["step"],
        "additionalProperties": false
    });
    let current_user = json!({
        "type": "message",
        "id": "msg-current-user-tool-search",
        "role": "user",
        "content": [{ "type": "input_text", "text": "继续使用工具" }]
    });
    let source_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "查找 consensus 工具" }]
            },
            current_user.clone(),
            {
                "type": "tool_search_call",
                "call_id": "call-tool-search",
                "status": "completed",
                "execution": "client",
                "arguments": { "query": "consensus" }
            },
            {
                "type": "tool_search_output",
                "call_id": "call-tool-search",
                "status": "completed",
                "execution": "client",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__pal",
                    "description": format!("BEGIN_NAMESPACE{}END_NAMESPACE", "界".repeat(12_000)),
                    "tools": [{
                        "type": "function",
                        "name": "consensus",
                        "description": format!("BEGIN_TOOL{}END_TOOL", "界".repeat(12_000)),
                        "parameters": parameters.clone()
                    }]
                }]
            },
            { "type": "compaction_trigger" }
        ],
        "tools": [{ "type": "tool_search" }]
    });
    let source_response = json!({
        "id": "resp-tool-search-trimmed",
        "status": "completed",
        "model": "claude-sonnet-5",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "<summary>较早历史摘要</summary>" }]
        }]
    });
    let compacted = rewrite_remote_compaction_v2_response_with_layered_compaction(
        &source_request,
        &source_response,
        true,
        MIN_RETAIN_TOKENS,
    )
    .expect("structured compaction response");
    assert!(compacted.layered.triggered);
    let history_request = json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "更早保留的 user" }]
            },
            current_user,
            compacted.response["output"][0].clone()
        ],
        "tools": [{ "type": "tool_search" }]
    });

    let chat = responses_to_chat_completions(history_request.clone()).unwrap();
    let chat_tool = chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "mcp__pal__consensus")
        .expect("trimmed tool search output still registers chat tool");
    assert_eq!(chat_tool["function"]["parameters"], parameters);
    assert!(
        chat_tool["function"]["description"]
            .as_str()
            .unwrap()
            .contains("<truncated:tool-desc;~")
    );

    let anthropic = responses_to_anthropic_messages(history_request).unwrap();
    let anthropic_tool = anthropic["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "mcp__pal__consensus")
        .expect("trimmed tool search output still registers anthropic tool");
    assert_eq!(anthropic_tool["input_schema"], parameters);
    assert!(
        anthropic_tool["description"]
            .as_str()
            .unwrap()
            .contains("<truncated:tool-desc;~")
    );
}

#[tokio::test]
async fn claude_synthetic_assistant_tail_completes_locally_without_upstream_prefill() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let compaction_request = json!({
        "model": "claude-sonnet-5",
        "input": [{ "type": "compaction_trigger" }]
    });
    let source_response = json!({
        "id": "resp-summary",
        "status": "completed",
        "model": "claude-sonnet-5",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>" }]
        }]
    });
    let compacted = rewrite_remote_compaction_v2_response_with_layered_compaction(
        &compaction_request,
        &source_response,
        false,
        DEFAULT_RETAIN_TOKENS,
    )
    .expect("legacy synthetic compaction");
    let request = json!({
        "model": "claude-sonnet-5",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "earlier user" }]
            },
            compacted.response["output"][0].clone()
        ]
    });

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .expect("assistant-tail pause should not need an upstream");
    assert_eq!(response.status, "200 OK");
    let body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"], json!([]));

    let mut stream_request = request;
    stream_request["stream"] = json!(true);
    let response = handle_responses_proxy_request(&stream_request.to_string())
        .await
        .expect("streaming assistant-tail pause should not need an upstream");
    let body = String::from_utf8(response.body).unwrap();
    assert!(body.contains("event: response.completed"));
    assert!(!body.contains("response.output_item.done"));
}

#[test]
fn remote_compaction_v2_tool_only_response_fails_closed() {
    let response = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl-tool-only",
            "created": 123,
            "model": "claude-sonnet-5",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_only",
                        "type": "function",
                        "function": {
                            "name": "exec_command",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
        &remote_compaction_v2_request(),
    )
    .unwrap();

    assert_eq!(response["status"], "failed");
    assert_eq!(response["output"], json!([]));
    assert_eq!(
        response["error"]["code"],
        "remote_compaction_summary_missing"
    );
}

#[test]
fn remote_compaction_v2_incomplete_chat_response_fails_closed() {
    let response = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl-incomplete",
            "created": 123,
            "model": "claude-sonnet-5",
            "choices": [{
                "index": 0,
                "finish_reason": "length",
                "message": {
                    "role": "assistant",
                    "content": "PARTIAL SUMMARY MUST NOT BE USED"
                }
            }]
        }),
        &remote_compaction_v2_request(),
    )
    .unwrap();

    assert_eq!(response["status"], "failed");
    assert_eq!(response["output"], json!([]));
    assert_eq!(
        response["error"]["code"],
        "remote_compaction_upstream_incomplete"
    );
    assert!(
        !response
            .to_string()
            .contains("PARTIAL SUMMARY MUST NOT BE USED")
    );
}

#[test]
fn remote_compaction_v2_chat_sse_with_message_and_tool_fails_closed() {
    let converted = chat_sse_to_responses_sse_with_request(
        r#"data: {"id":"chatcmpl-compact","created":123,"model":"claude-sonnet-5","choices":[{"index":0,"delta":{"role":"assistant","content":"CHAT STREAM SUMMARY"}}]}

data: {"id":"chatcmpl-compact","created":123,"model":"claude-sonnet-5","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_unexpected","type":"function","function":{"name":"exec_command","arguments":"{}"}}]}}]}

data: {"id":"chatcmpl-compact","created":123,"model":"claude-sonnet-5","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120}}

data: [DONE]

"#,
        &remote_compaction_v2_request(),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.completed")
    );
    let failed = events
        .iter()
        .find(|event| event.event == "response.failed")
        .unwrap();
    assert_eq!(failed.data["response"]["output"], json!([]));
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
fn anthropic_request_preserves_strict_tool_definition() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": "look up the record",
        "tools": [{
            "type": "function",
            "name": "lookup",
            "description": "Look up one record.",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"],
                "additionalProperties": false
            }
        }]
    }))
    .unwrap();

    assert_eq!(converted["tools"][0]["name"], "lookup");
    assert_eq!(converted["tools"][0]["strict"], true);
}

#[test]
fn anthropic_request_removes_count_marker_before_tool_use_history() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "codex_elves_compat":{"textual_tool_calls":true},
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "继续" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "先检查训练器。\n\ncount"
                }]
            },
            {
                "type": "function_call",
                "name": "shell_command",
                "call_id": "toolu_count_history",
                "arguments": "{\"command\":\"git status --short\"}"
            }
        ]
    }))
    .unwrap();

    let assistant = &converted["messages"][1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(assistant["content"][0]["type"], "text");
    assert_eq!(assistant["content"][0]["text"], "先检查训练器。");
    assert_eq!(assistant["content"][1]["type"], "tool_use");
    assert_eq!(assistant["content"][1]["id"], "toolu_count_history");
    assert_eq!(assistant["content"].as_array().unwrap().len(), 2);
}

#[test]
fn anthropic_request_keeps_count_history_without_following_tool() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "返回这个单词" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "count" }]
            },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "继续" }]
            }
        ]
    }))
    .unwrap();

    assert_eq!(converted["messages"][1]["role"], "assistant");
    assert_eq!(converted["messages"][1]["content"][0]["text"], "count");
}

#[test]
fn anthropic_request_keeps_normal_sentence_ending_in_count_before_tool_use() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "检查数量" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Please verify the count"
                }]
            },
            {
                "type": "function_call",
                "name": "shell_command",
                "call_id": "toolu_normal_count_history",
                "arguments": "{\"command\":\"git status --short\"}"
            }
        ]
    }))
    .unwrap();

    let assistant = &converted["messages"][1];
    assert_eq!(assistant["content"][0]["text"], "Please verify the count");
    assert_eq!(assistant["content"][1]["type"], "tool_use");
}

#[test]
fn anthropic_tool_result_keeps_images_as_image_blocks() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "look" }]
            },
            {
                "type": "function_call",
                "name": "view_image",
                "call_id": "call-1",
                "arguments": "{\"path\":\"shot.png\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call-1",
                "output": [
                    { "type": "input_text", "text": "screenshot" },
                    {
                        "type": "input_image",
                        "detail": "high",
                        "image_url": "data:image/png;base64,aGVsbG8="
                    }
                ]
            }
        ]
    }))
    .unwrap();

    let tool_result = &converted["messages"][2]["content"][0];
    assert_eq!(tool_result["type"], "tool_result");
    assert_eq!(tool_result["tool_use_id"], "call-1");
    assert_eq!(
        tool_result["content"],
        json!([
            { "type": "text", "text": "screenshot" },
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "aGVsbG8="
                }
            }
        ])
    );
}

#[test]
fn anthropic_legacy_tool_result_keeps_images_as_image_blocks() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "look" }]
            },
            {
                "type": "tool_call",
                "tool_use": {
                    "id": "call-legacy-image",
                    "name": "view_image",
                    "input": { "path": "screenshot.png" }
                }
            },
            {
                "type": "tool_result",
                "content": {
                    "tool_use_id": "call-legacy-image",
                    "content": [
                        { "type": "input_text", "text": "screenshot" },
                        {
                            "type": "input_image",
                            "image_url": "data:image/png;base64,aGVsbG8="
                        }
                    ]
                }
            }
        ]
    }))
    .unwrap();

    let tool_result = &converted["messages"][2]["content"][0];
    assert_eq!(tool_result["type"], "tool_result");
    assert_eq!(tool_result["tool_use_id"], "call-legacy-image");
    assert_eq!(
        tool_result["content"],
        json!([
            { "type": "text", "text": "screenshot" },
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "aGVsbG8="
                }
            }
        ])
    );
}

#[test]
fn anthropic_tool_result_keeps_plain_text_as_string() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "run" }]
            },
            {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call-2",
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call-2",
                "output": "exit code 0"
            }
        ]
    }))
    .unwrap();

    assert_eq!(
        converted["messages"][2]["content"][0]["content"],
        json!("exit code 0")
    );
}

#[test]
fn anthropic_orphan_tool_output_keeps_images_as_image_blocks() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "look" }]
            },
            {
                "type": "function_call_output",
                "call_id": "orphan-1",
                "output": [
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,aGVsbG8="
                    }
                ]
            }
        ]
    }))
    .unwrap();

    let content = &converted["messages"][0]["content"];
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[1]["text"], "Function call output (orphan-1):");
    assert_eq!(
        content[2],
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "aGVsbG8="
            }
        })
    );
}

#[test]
fn chat_tool_output_moves_images_into_following_user_message() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "look" }]
            },
            {
                "type": "function_call",
                "name": "view_image",
                "call_id": "call-1",
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "call-1",
                "output": [
                    { "type": "input_text", "text": "screenshot" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,aGVsbG8="
                    }
                ]
            }
        ]
    }))
    .unwrap();

    let messages = converted["messages"].as_array().unwrap();
    let tool = messages.iter().find(|m| m["role"] == "tool").unwrap();
    // tool 消息只能是纯文本，不能包含 base64。
    assert_eq!(tool["content"], json!("screenshot"));
    assert_eq!(tool["tool_call_id"], "call-1");

    // 图片另起一条 user 消息，且紧跟在 tool 消息之后。
    let tool_index = messages.iter().position(|m| m["role"] == "tool").unwrap();
    let image_message = &messages[tool_index + 1];
    assert_eq!(image_message["role"], "user");
    assert_eq!(
        image_message["content"][0],
        json!({
            "type": "image_url",
            "image_url": { "url": "data:image/png;base64,aGVsbG8=" }
        })
    );
}

#[test]
fn chat_parallel_tool_outputs_keep_tool_messages_contiguous() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "look" }]
            },
            { "type": "function_call", "name": "view_image", "call_id": "call-1", "arguments": "{}" },
            { "type": "function_call", "name": "exec_command", "call_id": "call-2", "arguments": "{}" },
            {
                "type": "function_call_output",
                "call_id": "call-1",
                "output": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,aGVsbG8="
                }]
            },
            {
                "type": "function_call_output",
                "call_id": "call-2",
                "output": "exit 0"
            }
        ]
    }))
    .unwrap();

    let messages = converted["messages"].as_array().unwrap();
    let roles = messages
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    // 两条 tool 消息必须相邻，图片 user 消息只能排在它们之后。
    assert_eq!(roles, vec!["user", "assistant", "tool", "tool", "user"]);
    assert_eq!(messages[4]["content"][0]["type"], "image_url");
}

#[test]
fn chat_tool_output_keeps_plain_text_as_string() {
    let converted = responses_to_chat_completions(json!({
        "model": "gpt-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "run" }]
            },
            { "type": "function_call", "name": "exec_command", "call_id": "call-9", "arguments": "{}" },
            {
                "type": "function_call_output",
                "call_id": "call-9",
                "output": "exit code 0"
            }
        ]
    }))
    .unwrap();

    let messages = converted["messages"].as_array().unwrap();
    let tool = messages.iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool["content"], json!("exit code 0"));
    // 无图片时不得凭空多出 user 消息。
    assert_eq!(messages.len(), 3);
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
fn anthropic_tool_schema_preserves_all_of_instead_of_weakening_constraints() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "input": "update the record",
        "tools": [{
            "type": "function",
            "name": "update_record",
            "parameters": {
                "allOf": [
                    {
                        "type": "object",
                        "properties": {
                            "score": {
                                "type": "number",
                                "minimum": 0
                            }
                        },
                        "required": ["score"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "score": {
                                "type": "number",
                                "maximum": 10
                            }
                        },
                        "required": ["score"],
                        "additionalProperties": false
                    }
                ]
            }
        }]
    }))
    .unwrap();

    let schema = &converted["tools"][0]["input_schema"];
    assert_eq!(schema["allOf"][0]["properties"]["score"]["minimum"], 0);
    assert_eq!(schema["allOf"][1]["properties"]["score"]["maximum"], 10);
    assert!(
        schema["allOf"][0]["properties"]["score"]
            .get("anyOf")
            .is_none()
    );
    assert!(
        schema["allOf"][1]["properties"]["score"]
            .get("anyOf")
            .is_none()
    );
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
    assert_eq!(sonnet["output_config"], json!({ "effort": "max" }));

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
fn anthropic_max_tokens_follow_model_capability_and_reasoning_effort() {
    for (model, effort, expected) in [
        ("claude-opus-5", "medium", 32_000_u64),
        ("claude-opus-5", "high", 64_000),
        ("claude-opus-5", "xhigh", 128_000),
        ("claude-opus-5", "max", 128_000),
        ("anthropic/claude-sonnet-6", "max", 128_000),
        ("deepseek-v4-pro", "high", 64_000),
        // 推理参数仍会把 DeepSeek xhigh 映射为 max，但输出预算保留 128K 档位。
        ("deepseek-v4-pro", "xhigh", 128_000),
        ("deepseek-v4-pro", "max", 384_000),
        // 后续 DeepSeek 大版本按家族继承，不需要逐个添加完整模型名。
        ("deepseek-v5-agent", "max", 384_000),
        ("glm-5.2", "max", 128_000),
        ("future-anthropic-model", "medium", 32_000),
        ("future-anthropic-model", "high", 64_000),
        ("future-anthropic-model", "xhigh", 128_000),
        // 未识别新模型的最大能力兜底为 128K。
        ("future-anthropic-model", "max", 128_000),
    ] {
        let converted = responses_to_anthropic_messages(json!({
            "model": model,
            "reasoning": { "effort": effort },
            "input": "hi"
        }))
        .unwrap();
        assert_eq!(
            converted["max_tokens"],
            json!(expected),
            "{model} / {effort} 输出上限错误"
        );
    }
}

#[test]
fn anthropic_reasoning_tier_overrides_legacy_client_max_token_hints() {
    let high_with_lower_hint = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "reasoning": { "effort": "high" },
        "max_output_tokens": 16_000,
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(high_with_lower_hint["max_tokens"], 16_000);

    let high_with_higher_hint = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "reasoning": { "effort": "high" },
        "max_output_tokens": 256_000,
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(high_with_higher_hint["max_tokens"], 64_000);

    let xhigh_with_legacy_hint = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "reasoning": { "effort": "xhigh" },
        "max_tokens": 32_000,
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(xhigh_with_legacy_hint["max_tokens"], 128_000);

    let max_effort = responses_to_anthropic_messages(json!({
        "model": "claude-opus-5",
        "reasoning": { "effort": "max" },
        "max_output_tokens": 16_000,
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(max_effort["max_tokens"], 128_000);
}

#[test]
fn anthropic_max_tokens_keep_legacy_claude_models_within_known_limits() {
    for (model, expected) in [
        ("claude-3-7-sonnet", 8_192_u64),
        ("claude-opus-4-1", 32_000),
        ("claude-opus-4-5", 64_000),
        ("claude-opus-4-8", 64_000),
        ("claude-sonnet-4-6", 64_000),
    ] {
        let converted = responses_to_anthropic_messages(json!({
            "model": model,
            "reasoning": { "effort": "high" },
            "input": "hi"
        }))
        .unwrap();
        assert_eq!(
            converted["max_tokens"],
            json!(expected),
            "{model} 旧模型输出能力应保持兼容"
        );
    }
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
    assert_eq!(converted["output"][1]["id"], "msg_test");
    assert_eq!(converted["output"][1]["content"][0]["text"], "answer");
    assert_eq!(converted["output"][2]["type"], "function_call");
    assert_eq!(converted["output"][2]["name"], "lookup");
    assert_eq!(converted["output"][2]["arguments"], r#"{"query":"codex"}"#);
    assert_eq!(converted["usage"]["input_tokens"], 12);
    assert_eq!(converted["usage"]["output_tokens"], 5);
    assert_eq!(converted["usage"]["total_tokens"], 17);
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cached_tokens"],
        2
    );
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cache_write_tokens"],
        0
    );
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
fn anthropic_usage_maps_cache_reads_and_writes_into_responses_input_details() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_anthropic_cache_usage",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 10,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 4,
                    "ephemeral_1h_input_tokens": 6
                }
            }
        }),
        &json!({ "model": "claude-opus-5", "input": "hello" }),
    )
    .unwrap();

    assert_eq!(converted["usage"]["input_tokens"], 22);
    assert_eq!(converted["usage"]["output_tokens"], 3);
    assert_eq!(converted["usage"]["total_tokens"], 25);
    assert_eq!(
        converted["usage"]["input_tokens_details"],
        json!({ "cached_tokens": 2, "cache_write_tokens": 10 })
    );
    assert_eq!(converted["usage"]["cache_read_input_tokens"], 2);
    assert_eq!(converted["usage"]["cache_creation_input_tokens"], 10);
    assert_eq!(
        converted["usage"]["cache_creation"],
        json!({
            "ephemeral_5m_input_tokens": 4,
            "ephemeral_1h_input_tokens": 6
        })
    );
    assert_eq!(
        converted["usage"]["output_tokens_details"],
        json!({ "reasoning_tokens": 0 })
    );
    assert_eq!(converted["usage"]["cache_ttl"], "mixed");
}

#[test]
fn anthropic_usage_saturates_malformed_cache_counts() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_anthropic_cache_overflow",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 18446744073709551615u64,
                    "ephemeral_1h_input_tokens": 18446744073709551615u64
                }
            }
        }),
        &json!({ "model": "claude-opus-5", "input": "hello" }),
    )
    .unwrap();

    assert_eq!(converted["usage"]["input_tokens"], u64::MAX);
    assert_eq!(converted["usage"]["total_tokens"], u64::MAX);
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cache_write_tokens"],
        u64::MAX
    );
}

#[test]
fn anthropic_missing_usage_includes_required_responses_details() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_anthropic_missing_usage",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn"
        }),
        &json!({ "model": "claude-opus-5", "input": "hello" }),
    )
    .unwrap();

    assert_eq!(
        converted["usage"],
        json!({
            "input_tokens": 0,
            "input_tokens_details": {
                "cached_tokens": 0,
                "cache_write_tokens": 0
            },
            "output_tokens": 0,
            "output_tokens_details": {
                "reasoning_tokens": 0
            },
            "total_tokens": 0
        })
    );
}

#[test]
fn anthropic_message_response_drops_count_before_native_tool_use() {
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_count_native",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [
                {
                    "type": "text",
                    "text": "现在检查 `_sample_cdf`。\n\ncount"
                },
                {
                    "type": "tool_use",
                    "id": "toolu_count_native",
                    "name": "shell_command",
                    "input": { "command": "git status --short" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-5",
            "input": "继续",
            "tools": [{
                "type": "function",
                "name": "shell_command",
                "parameters": { "type": "object" }
            }]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output[0]["type"], "message");
    assert_eq!(output[0]["content"][0]["text"], "现在检查 `_sample_cdf`。");
    assert_eq!(output[1]["type"], "function_call");
    assert_eq!(output[1]["name"], "shell_command");
    assert!(!converted["output"].to_string().contains("\n\ncount"));
}

#[test]
fn anthropic_message_response_drops_standalone_count_before_native_tool_use() {
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_count_only_native",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [
                { "type": "text", "text": "count" },
                {
                    "type": "tool_use",
                    "id": "toolu_count_only",
                    "name": "shell_command",
                    "input": { "command": "git status --short" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-5",
            "input": "继续",
            "tools": [{
                "type": "function",
                "name": "shell_command",
                "parameters": { "type": "object" }
            }]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["type"], "function_call");
}

#[test]
fn anthropic_message_response_keeps_count_without_tool_and_in_normal_code() {
    for text in ["count", "counter discount\nlet count = 1;"] {
        let converted = anthropic_message_to_response_with_request(
            json!({
                "id": "msg_count_plain",
                "type": "message",
                "role": "assistant",
                "model": "claude-opus-5",
                "content": [{ "type": "text", "text": text }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 10, "output_tokens": 5 }
            }),
            &json!({ "model": "claude-opus-5", "input": "返回正文" }),
        )
        .unwrap();

        assert_eq!(converted["output"][0]["content"][0]["text"], text);
    }
}

#[test]
fn anthropic_message_response_keeps_normal_sentence_ending_in_count_before_tool_use() {
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_normal_count_native",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [
                { "type": "text", "text": "Please verify the count" },
                {
                    "type": "tool_use",
                    "id": "toolu_normal_count",
                    "name": "shell_command",
                    "input": { "command": "git status --short" }
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-5",
            "input": "检查数量",
            "tools": [{
                "type": "function",
                "name": "shell_command",
                "parameters": { "type": "object" }
            }]
        }),
    )
    .unwrap();

    assert_eq!(
        converted["output"][0]["content"][0]["text"],
        "Please verify the count"
    );
    assert_eq!(converted["output"][1]["call_id"], "toolu_normal_count");
}

#[test]
fn anthropic_message_response_strips_inline_cite_wrappers() {
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_cite",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [{
                "type": "text",
                "text": "规则：<cite>将回答作为新输入回到 EXPAND</cite>。"
            }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "确认规则"
        }),
    )
    .unwrap();

    assert_eq!(
        converted["output"][0]["content"][0]["text"],
        "规则：将回答作为新输入回到 EXPAND。"
    );
}

#[test]
fn anthropic_message_response_strips_inline_cite_wrappers_with_attributes() {
    // 带属性的开标签（`<cite index="4-1">`）必须和无属性写法一样被完整剥离，
    // 否则会出现「闭标签被删、开标签残留在正文」的不对称结果。
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_cite_attr",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [{
                "type": "text",
                "text": "文档写得很直白：<cite index=\"4-1\">没有别的选项</cite>。另见 <cite index=\"6-21,6-22\">缓存前缀共享</cite>。"
            }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "确认结论"
        }),
    )
    .unwrap();

    assert_eq!(
        converted["output"][0]["content"][0]["text"],
        "文档写得很直白：没有别的选项。另见 缓存前缀共享。"
    );
}

#[test]
fn anthropic_message_response_keeps_non_cite_angle_brackets() {
    // 引用标记剥离必须按标签名精确匹配，不能误吞正文里的小于号或同前缀标签。
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_cite_guard",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [{
                "type": "text",
                "text": "当 a < b 且 c<d 时成立；<citation>保留</citation>。"
            }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-4-8",
            "input": "确认结论"
        }),
    )
    .unwrap();

    assert_eq!(
        converted["output"][0]["content"][0]["text"],
        "当 a < b 且 c<d 时成立；<citation>保留</citation>。"
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
fn anthropic_server_tool_use_web_search_keeps_server_history_without_client_calls() {
    // Claude 原生 server-side web_search 响应（server_tool_use + web_search_tool_result）：
    // 调用及结果使用不可执行历史封装，最终 text 正常输出。
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
    assert!(output.iter().all(|item| !matches!(
        item["type"].as_str(),
        Some("function_call" | "web_search_call")
    )));
    assert_eq!(
        output
            .iter()
            .filter(|item| item["type"] == "reasoning")
            .count(),
        2
    );
    // 含最终 text。
    assert!(serialized.contains("Claude Shannon was born in 1916."));
    assert!(serialized.contains("codex-elves-anthropic-content-v1:"));
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
    let converted = anthropic_message_to_response_with_compat(
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
        r#"{"cmd":"git diff crates/codex-elves-core/src/protocol_proxy.rs","max_output_tokens":6000,"yield_time_ms":3000}"#
    );
}

#[test]
fn anthropic_call_prefixed_textual_invoke_response_converts_to_tool_call() {
    let converted = anthropic_message_to_response_with_compat(
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
fn anthropic_count_prefixed_textual_invoke_response_converts_to_tool_call() {
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_textual_count_tool",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [
                {
                    "type": "text",
                    "text": "正文保留。\n\ncount\n<invoke name=\"exec_command\">\n<parameter name=\"command\">git status --short</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "stop",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-5",
            "input": "检查状态",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" }
                        }
                    }
                }
            ]
        }),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    assert_eq!(output[0]["type"], "message");
    assert_eq!(output[0]["content"][0]["text"], "正文保留。");
    assert_eq!(output[1]["type"], "function_call");
    assert_eq!(output[1]["name"], "exec_command");
    assert_eq!(
        output[1]["arguments"],
        r#"{"command":"git status --short"}"#
    );
}

#[test]
fn anthropic_textual_invoke_exec_command_allows_invoke_text_inside_parameter() {
    let command = r#"cd E:\code\junes\github\CodexElves; rg -n "invoke|textual_invoke|call_prefixed|<invoke|antml_tool_call|parse_textual|extract_tool" crates/codex-elves-core/src/protocol_proxy.rs | Select-Object -First 40"#;
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
fn anthropic_textual_invoke_uses_unique_ids_across_text_blocks() {
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_textual_calls_across_blocks",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-5",
            "content": [
                {
                    "type": "text",
                    "text": "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git status --short</parameter>\n</invoke>"
                },
                {
                    "type": "text",
                    "text": "call\n<invoke name=\"exec_command\">\n<parameter name=\"cmd\">git diff --check</parameter>\n</invoke>"
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({
            "model": "claude-opus-5",
            "input": "检查状态",
            "tools": [{
                "type": "function",
                "name": "exec_command",
                "parameters": {
                    "type": "object",
                    "properties": { "cmd": { "type": "string" } }
                }
            }]
        }),
    )
    .unwrap();

    let calls = converted["output"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "function_call")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[0]["call_id"]
            .as_str()
            .unwrap()
            .starts_with("call_textual_")
    );
    assert!(calls[0]["call_id"].as_str().unwrap().ends_with("_0"));
    assert!(calls[1]["call_id"].as_str().unwrap().ends_with("_1"));
    assert_ne!(calls[0]["id"], calls[1]["id"]);
}

#[test]
fn anthropic_textual_invoke_unescapes_parameter_text_with_nested_invoke_literal() {
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
    let converted = anthropic_message_to_response_with_compat(
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
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(reasoning["reasoning_effort"], "high");
    assert!(reasoning.get("tool_choice").is_none());
    let undeclared_choice = json!({
        "model":"gpt-5.4","reasoning":{"effort":"high"},
        "tool_choice":{"type":"function","name":"lookup"},"input":"hi"
    });
    assert!(responses_to_chat_completions(undeclared_choice.clone()).is_err());
    assert!(responses_to_anthropic_messages(undeclared_choice).is_err());

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

    let xhigh = responses_to_chat_completions(json!({
        "model": "glm-5.2",
        "reasoning": { "effort": "xhigh" },
        "input": "hi"
    }))
    .unwrap();

    assert_eq!(xhigh["thinking"], json!({ "type": "enabled" }));
    assert_eq!(xhigh["reasoning_effort"], "max");
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
    assert_eq!(glm["output_config"], json!({ "effort": "max" }));

    let glm_default = responses_to_anthropic_messages(json!({
        "model": "glm-5.2",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(glm_default["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(glm_default["output_config"], json!({ "effort": "max" }));

    let deepseek = responses_to_anthropic_messages(json!({
        "model": "deepseek-reasoner",
        "model_reasoning_effort": "max",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(deepseek["output_config"], json!({ "effort": "max" }));

    let deepseek_xhigh = responses_to_anthropic_messages(json!({
        "model": "deepseek-reasoner",
        "model_reasoning_effort": "xhigh",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek_xhigh["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(deepseek_xhigh["output_config"], json!({ "effort": "max" }));

    let deepseek_default = responses_to_anthropic_messages(json!({
        "model": "deepseek-v4-pro",
        "input": "hi"
    }))
    .unwrap();
    assert_eq!(deepseek_default["thinking"], json!({ "type": "adaptive" }));
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
fn gpt56_reasoning_efforts_match_snapshot_capabilities() {
    // gpt-5.6-sol 快照支持到 ultra；其它 gpt-5.6 支持到 max；普通 gpt 最高 xhigh。
    for model in [
        "gpt-5.6-sol",
        "openai/gpt-5.6-sol",
        "gpt-5.6-sol-2026-07-09",
    ] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::Responses),
            vec!["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
            "{model} 应支持到 ultra"
        );
    }
    for model in [
        "gpt-5.6",
        "gpt-5.6-terra",
        "gpt-5.6-luna-2026-07-09",
        "openai/gpt-5.6-custom",
    ] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::Responses),
            vec!["minimal", "low", "medium", "high", "xhigh", "max"],
            "{model} 应支持到 max"
        );
    }
    assert_eq!(
        supported_reasoning_efforts_for_model("gpt-5.5", UpstreamResponseProtocol::Responses),
        vec!["minimal", "low", "medium", "high", "xhigh"],
        "普通 gpt 最高 xhigh"
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
fn future_models_inherit_top_reasoning_efforts_across_families() {
    // GPT：5.6 起支持 max，更新版本不得被降到 xhigh。
    for model in ["gpt-5.7", "gpt-5.6-terra", "gpt-6", "openai/gpt-5.9-custom"] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::Responses),
            vec!["minimal", "low", "medium", "high", "xhigh", "max"],
            "{model} 应支持到 max"
        );
    }
    // sol 快照线的 ultra 能力同样按版本继承。
    for model in ["gpt-5.6-sol", "gpt-5.7-sol", "openai/gpt-6-sol-2026-09-01"] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::Responses),
            vec!["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
            "{model} 应支持到 ultra"
        );
    }
    // 名字里含 sol 前缀的其它模型不得被误判为 sol 快照。
    assert!(
        !supported_reasoning_efforts_for_model(
            "gpt-5.7-solar",
            UpstreamResponseProtocol::Responses
        )
        .contains(&"ultra")
    );
    // 旧代 GPT 仍保持 xhigh 上限。
    assert_eq!(
        supported_reasoning_efforts_for_model("gpt-5.5", UpstreamResponseProtocol::Responses),
        vec!["minimal", "low", "medium", "high", "xhigh"]
    );

    // Gemini：3.1 起 pro 补齐 medium，更新版本继承。
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "gemini-3.2-pro",
            UpstreamResponseProtocol::Responses
        ),
        vec!["low", "medium", "high"]
    );
    assert_eq!(
        supported_reasoning_efforts_for_model("gemini-3-pro", UpstreamResponseProtocol::Responses),
        vec!["low", "high"]
    );

    // StepFun：能力按家族前缀识别，不绑定单个日期快照名。
    for model in ["step-3.5-flash", "step-3.5-flash-2603", "step-4-flash"] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::ChatCompletions),
            vec!["low", "high"],
            "{model} 应命中 stepfun 能力表"
        );
    }
}
#[test]
fn future_claude_models_inherit_top_reasoning_efforts() {
    // 能力按家族+版本推导，未列入名单的新模型不得被降到 high。
    for model in [
        "claude-opus-5",
        "claude-opus-5-20260301",
        "claude-opus-5.1",
        "anthropic/claude-sonnet-6",
    ] {
        assert_eq!(
            supported_reasoning_efforts_for_model(model, UpstreamResponseProtocol::Anthropic),
            vec!["low", "medium", "high", "xhigh", "max"],
            "{model} 应支持到 max"
        );
    }
    // 点分隔版本写法与连字符写法能力必须一致。
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "claude-opus-4.6",
            UpstreamResponseProtocol::Anthropic
        ),
        vec!["low", "medium", "high", "max"]
    );
    // 旧代低能力模型仍保持保守档位。
    assert_eq!(
        supported_reasoning_efforts_for_model(
            "claude-sonnet-4-6",
            UpstreamResponseProtocol::Anthropic
        ),
        vec!["low", "medium", "high", "max"]
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
    let request = json!({
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
    });
    let converted = responses_to_chat_completions(request.clone()).unwrap();

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

    let batch = &converted["tools"][4]["function"];
    let description = batch["description"].as_str().unwrap();
    for instruction in [
        "Each file path must appear in only one operation per batch",
        "the executor rejects duplicate paths",
        "the hunks of one update_file operation",
        "supply the final full content",
        "use separate tool calls and wait for each to complete",
        "Patch files",
    ] {
        assert!(
            description.contains(instruction),
            "missing batch instruction: {instruction}"
        );
    }
    assert!(
        batch["parameters"]["properties"]["operations"]["description"]
            .as_str()
            .unwrap()
            .contains("distinct target paths")
    );

    let anthropic = responses_to_anthropic_messages(request).unwrap();
    let anthropic_batch = anthropic["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "apply_patch_batch")
        .unwrap();
    assert_eq!(anthropic_batch["description"], batch["description"]);
    assert_eq!(anthropic_batch["input_schema"], batch["parameters"]);
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
    assert_eq!(converted["usage"]["total_tokens"], 10);
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cached_tokens"],
        2
    );
    assert_eq!(
        converted["usage"]["input_tokens_details"]["cache_write_tokens"],
        4
    );
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
        "*** Begin Patch\n*** Add File: README.md\n+hello\n*** End Patch"
    );

    assert_apply_patch_replace_history(&converted["output"][0], "README.md", "hello");
}

fn assert_apply_patch_replace_history(item: &Value, path: &str, content: &str) {
    let request = json!({
        "model": "claude-opus-4-8",
        "input": [
            { "role": "user", "content": "Replace the file." },
            item,
            {
                "type": "custom_tool_call_output",
                "call_id": item["call_id"],
                "output": "Success"
            }
        ],
        "tools": [{ "type": "custom", "name": "apply_patch" }]
    });
    let expected = json!({ "path": path, "content": content });
    let chat = responses_to_chat_completions(request.clone()).unwrap();
    let call = &chat["messages"][1]["tool_calls"][0];
    assert_eq!(call["id"], item["call_id"]);
    assert_eq!(call["function"]["name"], "apply_patch_replace_file");
    assert_eq!(
        serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap()).unwrap(),
        expected
    );
    assert_eq!(chat["messages"][2]["tool_call_id"], item["call_id"]);

    let anthropic = responses_to_anthropic_messages(request).unwrap();
    let call = &anthropic["messages"][1]["content"][0];
    assert_eq!(call["id"], item["call_id"]);
    assert_eq!(call["name"], "apply_patch_replace_file");
    assert_eq!(call["input"], expected);
    assert_eq!(
        anthropic["messages"][2]["content"][0]["tool_use_id"],
        item["call_id"]
    );
}

#[test]
fn anthropic_apply_patch_replace_file_preserves_history_and_single_file_operation() {
    for content in ["", "替换内容", "first\n\n最后一行"] {
        let converted = anthropic_message_to_response_with_request(
            json!({
                "id": "msg_replace",
                "model": "claude-opus-4-8",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_replace",
                    "name": "apply_patch_replace_file",
                    "input": { "path": "目录/existing.txt", "content": content }
                }],
                "stop_reason": "tool_use"
            }),
            &json!({ "tools": [{ "type": "custom", "name": "apply_patch" }] }),
        )
        .unwrap();
        let item = &converted["output"][0];
        let patch = item["input"].as_str().unwrap();
        assert_eq!(patch.matches("*** Add File: ").count(), 1);
        assert!(!patch.contains("*** Delete File: "));
        assert_apply_patch_replace_history(item, "目录/existing.txt", content);
    }
}

#[test]
fn legacy_apply_patch_replace_history_does_not_replay_as_batch() {
    assert_apply_patch_replace_history(
        &json!({
            "id": "ctc_toolu_legacy",
            "type": "custom_tool_call",
            "name": "apply_patch",
            "call_id": "toolu_legacy",
            "input": "*** Begin Patch\n*** Delete File: existing.txt\n*** Add File: existing.txt\n+new\n*** End Patch"
        }),
        "existing.txt",
        "new",
    );

    let request = json!({
        "model": "gpt-5-mini",
        "input": [{
            "type": "custom_tool_call",
            "name": "apply_patch",
            "call_id": "call_different_paths",
            "input": "*** Begin Patch\n*** Delete File: old.txt\n*** Add File: new.txt\n+new\n*** End Patch"
        }],
        "tools": [{ "type": "custom", "name": "apply_patch" }]
    });
    let chat = responses_to_chat_completions(request).unwrap();
    assert_eq!(
        chat["messages"][0]["tool_calls"][0]["function"]["name"], "apply_patch_batch",
        "不同路径的删除和新增仍是两个操作"
    );
}

#[test]
fn apply_patch_replace_stream_preserves_identity_through_both_protocols() {
    let request = json!({
        "model": "claude-opus-4-8",
        "tools": [{ "type": "custom", "name": "apply_patch" }]
    });
    let chat = chat_sse_to_responses_sse_with_request(
        r#"data: {"id":"chatcmpl_replace","model":"claude-opus-4-8","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_replace","type":"function","function":{"name":"apply_patch_replace_file","arguments":"{\"path\":\"existing.txt\","}}]}}]}

data: {"id":"chatcmpl_replace","model":"claude-opus-4-8","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"content\":\"replacement\"}"}}]},"finish_reason":"tool_calls"}]}

data: [DONE]

"#,
        &request,
    );
    let anthropic = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_replace","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_replace","name":"apply_patch_replace_file","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"existing.txt\","}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"content\":\"replacement\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &request,
    );

    for stream in [chat, anthropic] {
        let events = parse_response_sse_events(&stream);
        let item = &events
            .iter()
            .find(|event| event.event == "response.output_item.done")
            .unwrap()
            .data["item"];
        let expected_patch =
            "*** Begin Patch\n*** Add File: existing.txt\n+replacement\n*** End Patch";
        assert_eq!(item["input"], expected_patch);
        assert_eq!(item["call_id"], "call_replace");
        assert_apply_patch_replace_history(item, "existing.txt", "replacement");
        for event in &events {
            match event.event.as_str() {
                "response.output_item.added" => {
                    assert_eq!(event.data["item"]["id"], item["id"]);
                }
                "response.custom_tool_call_input.delta" => {
                    assert_eq!(event.data["item_id"], item["id"]);
                    assert_eq!(event.data["delta"], expected_patch);
                }
                "response.custom_tool_call_input.done" => {
                    assert_eq!(event.data["item_id"], item["id"]);
                    assert_eq!(event.data["input"], expected_patch);
                }
                "response.completed" => {
                    assert_eq!(event.data["response"]["output"][0], *item);
                }
                _ => {}
            }
        }
    }
}

#[test]
fn apply_patch_batch_with_one_replacement_keeps_batch_identity() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_batch",
            "model": "gpt-5-mini",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": { "tool_calls": [{
                    "id": "call_batch",
                    "type": "function",
                    "function": {
                        "name": "apply_patch_batch",
                        "arguments": json!({ "operations": [{
                            "type": "replace_file",
                            "path": "existing.txt",
                            "content": "replacement"
                        }] }).to_string()
                    }
                }] }
            }]
        }),
        &json!({ "tools": [{ "type": "custom", "name": "apply_patch" }] }),
    )
    .unwrap();
    let item = &converted["output"][0];
    assert_eq!(
        item["input"],
        "*** Begin Patch\n*** Add File: existing.txt\n+replacement\n*** End Patch"
    );
    let replay = responses_to_chat_completions(json!({
        "model": "gpt-5-mini",
        "input": [item],
        "tools": [{ "type": "custom", "name": "apply_patch" }]
    }))
    .unwrap();
    assert_eq!(
        replay["messages"][0]["tool_calls"][0]["function"]["name"],
        "apply_patch_batch"
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
fn chat_completion_response_maps_gemini_and_claude_cache_usage_to_responses_totals() {
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
    assert_eq!(gemini["usage"]["input_tokens"], 20);
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
    assert_eq!(claude["usage"]["input_tokens"], 22);
    assert_eq!(claude["usage"]["total_tokens"], 25);
    assert_eq!(claude["usage"]["cache_read_input_tokens"], 2);
    assert_eq!(claude["usage"]["cache_creation_5m_input_tokens"], 4);
    assert_eq!(claude["usage"]["cache_creation_1h_input_tokens"], 6);
    assert_eq!(claude["usage"]["cache_ttl"], "mixed");
    assert_eq!(claude["usage"]["input_tokens_details"]["cached_tokens"], 2);
    assert_eq!(
        claude["usage"]["input_tokens_details"]["cache_write_tokens"],
        10
    );
}

#[test]
fn chat_usage_saturates_malformed_cache_counts() {
    let converted = chat_completion_to_response(json!({
        "id": "chatcmpl_usage_overflow",
        "created": 123,
        "model": "claude-proxy",
        "choices": [{
            "finish_reason": "stop",
            "message": { "role": "assistant", "content": "ok" }
        }],
        "usage": {
            "input_tokens": 1,
            "output_tokens": 1,
            "cache_read_input_tokens": 18446744073709551615u64,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 18446744073709551615u64,
                "ephemeral_1h_input_tokens": 18446744073709551615u64
            }
        }
    }))
    .unwrap();

    assert_eq!(converted["usage"]["total_tokens"], u64::MAX);
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
    assert_eq!(completed.data["response"]["id"], "resp_msg_stream");
    assert_eq!(completed.data["response"]["output"][1]["id"], "msg_stream");
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
    let replay = responses_to_anthropic_messages(json!({
        "model":"claude-test","input":[
            {"role":"user","content":"继续"},
            completed.data["response"]["output"][0].clone(),
            {"role":"assistant","content":"历史回答"}
        ]
    }))
    .unwrap();
    assert_eq!(
        replay["messages"][1]["content"][0],
        json!({
            "type":"thinking","thinking":"Need context.","signature":"sig_stream"
        })
    );
    assert!(converted.contains("data: [DONE]"));
}

#[test]
fn remote_compaction_v2_anthropic_sse_with_message_and_tool_fails_closed() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_compact","type":"message","role":"assistant","model":"claude-sonnet-5","content":[],"usage":{"input_tokens":100}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"STREAM SUMMARY"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_unexpected","name":"exec_command","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":20}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &remote_compaction_v2_request(),
    );

    let events = parse_response_sse_events(&converted);
    assert!(
        !events
            .iter()
            .any(|event| event.event == "response.completed")
    );
    let failed = events
        .iter()
        .find(|event| event.event == "response.failed")
        .unwrap();
    assert_eq!(failed.data["response"]["output"], json!([]));
}

#[test]
fn anthropic_sse_strips_fragmented_inline_cite_wrappers() {
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_cite_stream","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"规则：<ci"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"te>将回答作为新输入回到 EXPAND</ci"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"te>。"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8"
        }),
    );

    let text = parse_response_sse_events(&converted)
        .into_iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str().map(ToString::to_string))
        .collect::<String>();

    assert_eq!(text, "规则：将回答作为新输入回到 EXPAND。");
    assert!(!converted.contains("<cite>"));
    assert!(!converted.contains("</cite>"));
}

#[test]
fn anthropic_sse_strips_fragmented_inline_cite_wrappers_with_attributes() {
    // 带属性的开标签长度不固定，且会被上游切分到多个 delta；
    // 缓冲必须等到 `>` 才判定，否则开标签会原样泄露到正文。
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_cite_attr_stream","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"文档写得很直白：<cite ind"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ex=\"4-1\">没有别的选项</ci"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"te>。"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-4-8"
        }),
    );

    let text = parse_response_sse_events(&converted)
        .into_iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str().map(ToString::to_string))
        .collect::<String>();

    assert_eq!(text, "文档写得很直白：没有别的选项。");
    assert!(!converted.contains("<cite"));
    assert!(!converted.contains("</cite"));
}

#[test]
fn anthropic_sse_strips_inline_cite_under_every_fragmentation() {
    // 历次复发都是因为手写样例没撞上真实分片点。这里穷举所有切分方式（含逐字符），
    // 从构造上消除「某个特定分片位置没覆盖到」这类 bug。
    for (text, expected) in [
        ("a<cite index=\"4-1\">b</cite>c", "abc"),
        ("x<cite>y</cite>z", "xyz"),
        (
            "p<cite index=\"1\">q</cite>r<cite index=\"2\">s</cite>t",
            "pqrst",
        ),
    ] {
        assert_all_fragmentations_yield(text, expected);
    }
}

#[test]
fn anthropic_sse_keeps_non_cite_markup_under_every_fragmentation() {
    // 剥离逻辑不能在任何分片方式下吞掉正常正文（小于号、同前缀标签）。
    for text in [
        "if a < b then",
        "<citation>x</citation>",
        "Vec<City> 列表",
        "a <cite中文> b",
    ] {
        assert_all_fragmentations_yield(text, text);
    }
}

#[test]
fn anthropic_sse_keeps_unclosed_cite_tag_text() {
    // 上游截断导致开标签永远等不到 `>` 时，块结束必须兵底吐出残留文本，不能静默丢失。
    for text in ["尾巴 <cite ind", "尾巴 <ci", "尾巴 <", "尾巴 </cit"] {
        assert_all_fragmentations_yield(text, text);
    }
}

#[test]
fn anthropic_sse_strips_inline_cite_without_breaking_textual_invoke() {
    // 引用剥离和文本式工具调用解析共用同一个文本缓冲，必须确认两者不互相破坏。
    let converted = anthropic_sse_to_responses_sse_with_compat(
        &anthropic_sse_from_text_deltas(&[
            "结论：<cite ind".to_string(),
            "ex=\"4-1\">看这里</ci".to_string(),
            "te>\n\n<invoke name=\"shell\">".to_string(),
            "<parameter name=\"cmd\">ls</parameter></invoke>".to_string(),
        ]),
        &json!({ "model": "claude-opus-4-8", "tools":[{"type":"function","name":"shell","parameters":{"type":"object"}}] }),
    );

    assert_eq!(collect_stream_output_text(&converted), "结论：看这里");
    assert!(!converted.contains("<cite"));

    let call = parse_response_sse_events(&converted)
        .into_iter()
        .find(|event| {
            event.event == "response.output_item.done"
                && event.data["item"]["type"] == "function_call"
        })
        .expect("文本式工具调用应该被识别为 function_call");
    assert_eq!(call.data["item"]["name"], "shell");
}

#[test]
fn anthropic_sse_strips_inline_cite_across_multiple_text_blocks() {
    // 引用残留按 block index 分开缓存；多个 text 块各自截断时不能串位。
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_multi","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"一<cite ind"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ex=\"1\">二</cite>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"三<cite ind"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"ex=\"2\">四</cite>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({ "model": "claude-opus-4-8" }),
    );

    assert_eq!(collect_stream_output_text(&converted), "一二三四");
    assert!(!converted.contains("<cite"));
}

#[test]
fn anthropic_stream_and_non_stream_agree_on_inline_cite_stripping() {
    for text in [
        "普通中文回复",
        "前缀<cite source=\"one\">引用内容</cite>后缀",
        "代码中的 <T> 和 <value> 保留",
    ] {
        let direct = anthropic_message_to_response_with_compat(
            json!({
                "id": "msg_cite_compare", "model": "claude-opus-4-8",
                "content": [{ "type": "text", "text": text }],
                "stop_reason": "end_turn"
            }),
            &json!({}),
        )
        .unwrap();
        let chunks: Vec<_> = text.chars().map(|c| c.to_string()).collect();
        let streamed = anthropic_sse_to_responses_sse_with_compat(
            &anthropic_sse_from_text_deltas(&chunks),
            &json!({}),
        );
        assert_eq!(
            collect_stream_output_text(&streamed),
            direct["output"][0]["content"][0]["text"].as_str().unwrap()
        );
    }
}

/// 引用标记剥离已收敛到共享文本出口，Chat Completions 协议必须同样生效。
#[test]
fn chat_completion_response_strips_inline_cite_wrappers() {
    let converted = chat_completion_to_response_with_request(
        json!({
            "id": "chatcmpl_cite",
            "object": "chat.completion",
            "model": "claude-opus-4-8",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "文档：<cite index=\"4-1\">结论</cite>。a < b 保留",
                    "reasoning_content": "推理：<cite index=\"9-9\">依据</cite>。"
                },
                "finish_reason": "stop"
            }]
        }),
        &legacy_text_compat_request(&json!({ "model": "claude-opus-4-8" })),
    )
    .unwrap();

    let output = converted["output"].as_array().unwrap();
    let reasoning = output
        .iter()
        .find(|item| item["type"] == "reasoning")
        .expect("应该有 reasoning item");
    assert_eq!(reasoning["reasoning_content"], "推理：依据。");
    assert_eq!(reasoning["summary"][0]["text"], "推理：依据。");

    let message = output
        .iter()
        .find(|item| item["type"] == "message")
        .expect("应该有 message item");
    assert_eq!(message["content"][0]["text"], "文档：结论。a < b 保留");
}

#[test]
fn chat_sse_strips_fragmented_inline_cite_wrappers() {
    // Chat 流式与 Anthropic 流式现在共用同一个过滤器，分片行为必须一致。
    let converted = chat_sse_to_responses_sse_with_request(
        concat!(
            "data: {\"id\":\"c\",\"model\":\"claude-opus-4-8\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"文档：<cite ind\"}}]}\n\n",
            "data: {\"id\":\"c\",\"model\":\"claude-opus-4-8\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ex=\\\"4-1\\\">结论</ci\"}}]}\n\n",
            "data: {\"id\":\"c\",\"model\":\"claude-opus-4-8\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"te>。\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        ),
        &legacy_text_compat_request(&json!({ "model": "claude-opus-4-8" })),
    );

    assert_eq!(collect_stream_output_text(&converted), "文档：结论。");
    assert!(!converted.contains("<cite"));
    assert!(!converted.contains("</cite"));
}

#[test]
fn anthropic_response_strips_inline_cite_wrappers_in_thinking() {
    // 推理内容也会带引用标记；展开推理详情时不能看到裸露的 `<cite ...>`。
    let converted = anthropic_message_to_response_with_compat(
        json!({
            "id": "msg_think_cite",
            "type": "message",
            "role": "assistant",
            "model": "claude-opus-4-8",
            "content": [
                { "type": "thinking", "thinking": "我查到<cite index=\"4-1\">证据</cite>。" },
                { "type": "text", "text": "结论" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        }),
        &json!({ "model": "claude-opus-4-8" }),
    )
    .unwrap();

    assert_eq!(converted["output"][0]["type"], "reasoning");
    assert_eq!(converted["output"][0]["reasoning_content"], "我查到证据。");
    assert_eq!(converted["output"][0]["summary"][0]["text"], "我查到证据。");
}

#[test]
fn anthropic_sse_strips_inline_cite_wrappers_in_thinking_stream() {
    // 流式推理同样会被分片，引用过滤器必须在推理通道上独立生效。
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_think_stream","type":"message","role":"assistant","model":"claude-opus-4-8","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"我查到<cite ind"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"ex=\"4-1\">证据</ci"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"te>。"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({ "model": "claude-opus-4-8" }),
    );

    let reasoning = parse_response_sse_events(&converted)
        .into_iter()
        .filter(|event| event.event == "response.reasoning_summary_text.delta")
        .filter_map(|event| event.data["delta"].as_str().map(ToString::to_string))
        .collect::<String>();

    assert_eq!(reasoning, "我查到证据。");
    assert!(!converted.contains("<cite"));
    // 非流式和流式是两条独立代码路径，历史上就是它们不一致才出的问题。
    for text in [
        "文档：<cite index=\"4-1\">结论</cite>。",
        "无属性 <cite>x</cite> 尾巴",
        "if a < b && c<d then",
        "<citation>保留</citation>",
        "未闭合 <cite ind",
    ] {
        let non_stream = anthropic_message_to_response_with_compat(
            json!({
                "id": "msg_agree",
                "type": "message",
                "role": "assistant",
                "model": "claude-opus-4-8",
                "content": [{ "type": "text", "text": text }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 10, "output_tokens": 5 }
            }),
            &json!({ "model": "claude-opus-4-8" }),
        )
        .unwrap();
        let non_stream_text = non_stream["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        assert_eq!(
            stream_text_with_cuts(text, &char_boundaries(text)),
            non_stream_text,
            "流式与非流式结果不一致：text={text:?}"
        );
    }
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
fn anthropic_sse_count_prefixed_textual_invoke_converts_to_tool_call_events() {
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_textual_stream_count","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"正文保留。\n\nco"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"unt\n<in"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"voke name=\"exec_command\">\n<parameter name=\"command\">git status --short</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-5",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": {
                        "type": "object",
                        "properties": { "command": { "type": "string" } }
                    }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect::<String>();
    assert_eq!(text_delta, "正文保留。");
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该还原出 exec_command 调用");
    assert_eq!(arguments_done.data["name"], "exec_command");
    assert_eq!(
        arguments_done.data["arguments"],
        r#"{"command":"git status --short"}"#
    );
}

#[test]
fn anthropic_sse_keeps_textual_and_native_tool_state_indices_distinct() {
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_mixed_tool_indices","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call\n<invoke name=\"exec_command\">\n<parameter name=\"command\">git status --short</parameter>\n</invoke>\n<invoke name=\"exec_command\">\n<parameter name=\"command\">git diff --check</parameter>\n</invoke>"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_native_after_textual","name":"shell_command","input":{"command":"pwd"}}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-5",
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "parameters": { "type": "object" }
                },
                {
                    "type": "function",
                    "name": "shell_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let completed = events
        .iter()
        .find(|event| event.event == "response.completed")
        .unwrap();
    let calls = completed.data["response"]["output"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "function_call")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    assert!(
        calls[0]["call_id"]
            .as_str()
            .unwrap()
            .starts_with("call_textual_")
    );
    assert!(calls[0]["call_id"].as_str().unwrap().ends_with("_0"));
    assert!(calls[1]["call_id"].as_str().unwrap().ends_with("_1"));
    assert_eq!(calls[2]["call_id"], "toolu_native_after_textual");
    assert_eq!(calls[0]["name"], "exec_command");
    assert_eq!(calls[1]["name"], "exec_command");
    assert_eq!(calls[2]["name"], "shell_command");
    assert_ne!(calls[0]["id"], calls[1]["id"]);
    assert_ne!(calls[1]["id"], calls[2]["id"]);
}

#[test]
fn anthropic_sse_ignores_descriptive_invoke_text_before_real_exec_call() {
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
fn anthropic_sse_drops_fragmented_count_marker_before_native_tool_use() {
    let converted = anthropic_sse_to_responses_sse_with_compat(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_native_tool_with_count_marker","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"现在检查 `_sample_cdf`。\n\nco"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"unt"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_count_stream","name":"shell_command","input":{"command":"git status --short"}}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-5",
            "tools": [
                {
                    "type": "function",
                    "name": "shell_command",
                    "parameters": { "type": "object" }
                }
            ]
        }),
    );

    let events = parse_response_sse_events(&converted);
    let text_delta = events
        .iter()
        .filter(|event| event.event == "response.output_text.delta")
        .filter_map(|event| event.data["delta"].as_str())
        .collect::<String>();
    assert_eq!(text_delta, "现在检查 `_sample_cdf`。");
    assert!(!text_delta.contains("count"));
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该保留原生工具调用");
    assert_eq!(arguments_done.data["name"], "shell_command");
}

#[test]
fn anthropic_sse_keeps_normal_sentence_ending_in_count_before_native_tool_use() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_normal_count_stream","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Please verify the co"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"unt"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_normal_count_stream","name":"shell_command","input":{"command":"git status --short"}}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({
            "model": "claude-opus-5",
            "tools": [{
                "type": "function",
                "name": "shell_command",
                "parameters": { "type": "object" }
            }]
        }),
    );

    assert_eq!(
        collect_stream_output_text(&converted),
        "Please verify the count"
    );
    let events = parse_response_sse_events(&converted);
    let arguments_done = events
        .iter()
        .find(|event| event.event == "response.function_call_arguments.done")
        .expect("应该保留原生工具调用");
    assert_eq!(arguments_done.data["name"], "shell_command");
}

#[test]
fn anthropic_sse_keeps_count_marker_when_no_tool_follows() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_count_without_tool","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"count"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}

event: message_stop
data: {"type":"message_stop"}

"#,
        &json!({ "model": "claude-opus-5" }),
    );

    assert_eq!(collect_stream_output_text(&converted), "count");
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
    let converted = anthropic_sse_to_responses_sse_with_compat(
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
fn anthropic_sse_diagnostics_distinguish_done_from_message_stop_and_pending_blocks() {
    let mut converter =
        AnthropicSseToResponsesConverter::with_request(&legacy_text_compat_request(&json!({})));
    let output = converter.push_bytes(
        br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_done_only","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"call"}}

data: [DONE]

"#,
    );
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("response.failed")
    );

    let summary = converter.diagnostic_summary();
    assert_eq!(summary["sawDone"], true);
    assert_eq!(summary["sawMessageStop"], false);
    assert_eq!(summary["terminalStatus"], "response_failed");
    assert_eq!(summary["failureSource"], "incomplete_done_marker");
    assert_eq!(summary["openBlockCount"], 1);
    assert_eq!(summary["pendingTextBufferCount"], 1);
}

#[test]
fn anthropic_sse_done_after_closed_text_block_flushes_pending_marker() {
    let converted = anthropic_sse_to_responses_sse_with_request(
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_done_after_closed_text","type":"message","role":"assistant","model":"claude-opus-5","content":[],"usage":{"input_tokens":7}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"count"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

data: [DONE]

"#,
        &json!({ "model": "claude-opus-5" }),
    );

    assert_eq!(collect_stream_output_text(&converted), "count");
    assert!(converted.contains("event: response.completed"));
    assert!(!converted.contains("event: response.failed"));
}

#[test]
fn anthropic_sse_ignores_done_after_message_stop_terminal_event() {
    let mut converter = AnthropicSseToResponsesConverter::default();
    let mut output = converter.push_bytes(
        br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"mystery_block","value":"x"}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );
    output.extend(converter.push_bytes(b"data: [DONE]\n\n"));
    let output = String::from_utf8(output).unwrap();

    assert_eq!(output.matches("event: response.completed").count(), 1);
    assert!(!output.contains("event: response.failed"));
    let summary = converter.diagnostic_summary();
    assert_eq!(summary["sawMessageStop"], true);
    assert_eq!(summary["sawDone"], true);
    assert_eq!(summary["terminalStatus"], "response_completed");
}

#[test]
fn anthropic_sse_diagnostics_separate_server_results_from_unknown_blocks() {
    let mut converter = AnthropicSseToResponsesConverter::default();
    converter.push_bytes(
        br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]}}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"mystery_block","value":"x"}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );

    let summary = converter.diagnostic_summary();
    assert_eq!(summary["serverToolResultBlockCount"], 1);
    assert_eq!(summary["unknownBlockCount"], 1);
    assert_eq!(summary["unknownBlockTypes"], json!(["mystery_block"]));
    assert_eq!(summary["openBlockCount"], 2);
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
fn stream_idle_timeouts_match_proxy_policy() {
    assert_eq!(upstream_models_header_timeout(), Duration::from_secs(30));
    assert_eq!(
        stream_idle_timeout_for_reasoning_effort(None),
        Duration::from_secs(900)
    );
    for (effort, expected_ms) in [
        (None, 900_000),
        (Some("minimal"), 900_000),
        (Some("low"), 900_000),
        (Some("medium"), 900_000),
        (Some("high"), 900_000),
        (Some("xhigh"), 1_500_000),
        (Some("max"), 1_800_000),
        (Some("ultra"), 1_800_000),
    ] {
        assert_eq!(
            stream_idle_timeout_ms_for_reasoning_effort(effort),
            expected_ms,
            "推理级别对应的流空闲超时错误: {effort:?}"
        );
    }
    for (request, expected) in [
        (json!({}), Duration::from_secs(900)),
        (
            json!({ "reasoning": { "effort": "high" } }),
            Duration::from_secs(900),
        ),
        (
            json!({ "reasoning": { "effort": "xhigh" } }),
            Duration::from_secs(1500),
        ),
        (
            json!({ "model_reasoning_effort": "max" }),
            Duration::from_secs(1800),
        ),
        (
            json!({ "reasoning_effort": "ultra" }),
            Duration::from_secs(1800),
        ),
    ] {
        assert_eq!(
            stream_idle_timeout_for_request(Some(&request)),
            expected,
            "推理级别对应的流式响应头超时错误: {request}"
        );
    }
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
    assert!(upstream_error_is_timeout(&result.unwrap_err()));
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

    assert_eq!(
        result.status_code,
        200,
        "aggregate failover response: {}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(body.as_ref(), br#"{"id":"resp_1","object":"response"}"#);
    first_server.await.unwrap();
    second_server.await.unwrap();
}

#[tokio::test]
async fn aggregate_failover_reapplies_header_policy_after_switching_to_chat_completions() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first_server = spawn_chat_server_with_status_responses(vec![(
        "500 Internal Server Error".to_string(),
        r#"{"error":{"message":"retry another member"}}"#.to_string(),
    )]);
    let second_server = spawn_chat_server_with_response(
        r#"{"id":"chatcmpl_failover","object":"chat.completion","choices":[]}"#,
    );
    let mut settings = aggregate_proxy_settings(
        "header-policy-failover",
        first_server.base_url.clone(),
        second_server.base_url.clone(),
    );
    settings.relay_profiles[1].protocol = RelayProtocol::ChatCompletions;
    settings.relay_profiles[1].model_mappings[0].protocol = RelayProtocol::ChatCompletions;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("remote_compaction_v2"),
    );
    let request_context = RequestContext::from_headers(headers);
    let upstream = open_responses_proxy_request_with_settings_and_request_context(
        r#"{"model":"gpt-5-mini","input":"hi","stream":false}"#,
        settings,
        &request_context,
    )
    .await
    .unwrap();

    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::ChatCompletions
    );
    drop(upstream);

    let first = first_server.finish();
    let second = second_server.finish();
    assert_eq!(first.path, "/v1/responses");
    assert_eq!(first.x_codex_beta_features, "remote_compaction_v2");
    assert_eq!(second.path, "/v1/chat/completions");
    assert!(
        second.x_codex_beta_features.is_empty(),
        "Codex Responses semantics must not leak after failover changes the target protocol"
    );
}

#[tokio::test]
async fn aggregate_compaction_uses_bound_conversation_protocol_instead_of_probe() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_response(
        json!({
            "id": "resp_bound_native",
            "object": "response",
            "status": "completed",
            "model": "gpt-5-mini",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "<summary>native response</summary>"}]
            }]
        })
        .to_string(),
    );
    let mut settings = aggregate_proxy_settings(
        "compaction-bound-native",
        server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.layered_compaction_enabled = true;
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::ConversationRoundRobin;
    settings.relay_profiles[1].model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    let selected = codex_elves_core::relay_rotation::select_relay_for_request(
        &settings,
        codex_elves_core::relay_rotation::RotationContext::for_conversation("bound-native"),
    )
    .unwrap();
    assert_eq!(selected.id, settings.relay_profiles[0].id);
    assert_eq!(
        codex_elves_core::relay_rotation::select_relay_for_probe(&settings)
            .unwrap()
            .id,
        settings.relay_profiles[1].id,
    );
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-5-mini");
    request["conversation"] = json!("bound-native");
    request["stream"] = json!(false);

    let upstream = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(upstream.relay_id.as_deref(), Some(selected.id.as_str()));
    let captured = server.finish();
    let forwarded: Value = serde_json::from_str(&captured.body).unwrap();
    assert_eq!(captured.path, "/v1/responses");
    assert_eq!(
        forwarded["input"], request["input"],
        "native V2 input must remain intact"
    );
    assert_eq!(forwarded["tools"], request["tools"]);
}

#[tokio::test]
async fn aggregate_compaction_keeps_conversation_rotation_after_local_bridge() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_response(
        json!({
            "id": "chatcmpl_bound_bridge",
            "model": "gpt-5-mini",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "<summary>bridge response</summary>"},
                "finish_reason": "stop"
            }]
        })
        .to_string(),
    );
    let mut settings = aggregate_proxy_settings(
        "compaction-bound-bridge",
        server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.layered_compaction_enabled = true;
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::ConversationRoundRobin;
    settings.relay_profiles[0].model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    let selected = codex_elves_core::relay_rotation::select_relay_for_request(
        &settings,
        codex_elves_core::relay_rotation::RotationContext::for_conversation("bound-bridge"),
    )
    .unwrap();
    assert_eq!(selected.id, settings.relay_profiles[0].id);
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-5-mini");
    request["conversation"] = json!("bound-bridge");
    request["stream"] = json!(false);

    let upstream =
        open_responses_proxy_request_with_settings(&request.to_string(), settings.clone())
            .await
            .unwrap();
    let response: Value =
        serde_json::from_slice(&upstream.into_body_bytes().await.unwrap()).unwrap();
    assert_eq!(response["status"], "completed");
    let captured = server.finish();
    assert_eq!(captured.path, "/v1/chat/completions");
    assert!(!captured.body.contains("compaction_trigger"));
    let next = codex_elves_core::relay_rotation::select_relay_for_request(
        &settings,
        codex_elves_core::relay_rotation::RotationContext::for_conversation("next-conversation"),
    )
    .unwrap();
    assert_eq!(
        next.id, settings.relay_profiles[1].id,
        "local bridge must not reset aggregate rotation"
    );
    let bound = codex_elves_core::relay_rotation::select_relay_for_request(
        &settings,
        codex_elves_core::relay_rotation::RotationContext::for_conversation("bound-bridge"),
    )
    .unwrap();
    assert_eq!(bound.id, selected.id);
}

#[tokio::test]
async fn aggregate_compaction_failover_uses_the_final_retry_outcome() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let mut mismatches = Vec::new();
    for retry_succeeds in [false, true] {
        let failure = json!({"error": {"message": "temporary provider failure"}}).to_string();
        let retry = if retry_succeeds {
            (
                "200 OK".to_string(),
                json!({
                    "id": "chatcmpl_retry",
                    "model": "gpt-5-mini",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "<summary>recovered</summary>"},
                        "finish_reason": "stop"
                    }]
                })
                .to_string(),
            )
        } else {
            ("500 Internal Server Error".to_string(), failure.clone())
        };
        let server = spawn_chat_server_with_status_responses(vec![
            ("500 Internal Server Error".to_string(), failure),
            retry,
        ]);
        let mut settings = aggregate_proxy_settings(
            &format!("compaction-final-outcome-{retry_succeeds}"),
            server.base_url.clone(),
            "http://127.0.0.1:9/v1".to_string(),
        );
        settings.layered_compaction_enabled = true;
        settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::Failover;
        settings.relay_profiles[0].model_mappings[0].protocol = RelayProtocol::ChatCompletions;
        let mut request = remote_compaction_v2_request();
        request["model"] = json!("gpt-5-mini");
        request["stream"] = json!(false);
        let upstream =
            open_responses_proxy_request_with_settings(&request.to_string(), settings.clone())
                .await
                .unwrap();
        let body: Value =
            serde_json::from_slice(&upstream.into_body_bytes().await.unwrap()).unwrap();
        assert_eq!(
            body["status"],
            if retry_succeeds {
                "completed"
            } else {
                "failed"
            }
        );
        assert_eq!(server.finish_all().len(), 2);
        let next = codex_elves_core::relay_rotation::select_relay_for_request(
            &settings,
            codex_elves_core::relay_rotation::RotationContext::default(),
        )
        .unwrap();
        let expected = &settings.relay_profiles[usize::from(!retry_succeeds)].id;
        if &next.id != expected {
            mismatches.push(format!(
                "retry_succeeds={retry_succeeds}: expected {expected}, got {}",
                next.id
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "failover must follow the final request outcome: {mismatches:?}"
    );
}

#[tokio::test]
async fn aggregate_remote_compaction_retries_selected_candidate_with_actual_protocol() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first_server = spawn_chat_server_with_status_responses(vec![
        (
            "500 Internal Server Error".to_string(),
            r#"{"error":1}"#.to_string(),
        ),
        (
            "200 OK".to_string(),
            json!({
                "id": "chatcmpl_compaction",
                "created": 0,
                "model": "gpt-5-mini",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "<summary>RETRY SUMMARY</summary>"
                    },
                    "finish_reason": "stop"
                }]
            })
            .to_string(),
        ),
    ]);
    let mut settings = aggregate_proxy_settings(
        "compaction-same-candidate-retry",
        first_server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.relay_profiles[0].protocol = RelayProtocol::ChatCompletions;
    settings.relay_profiles[0].model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    let mut request = remote_compaction_v2_request();
    settings.layered_compaction_enabled = true;
    request["model"] = json!("gpt-5-mini");
    request["stream"] = json!(false);

    let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();

    assert_eq!(
        result.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let upstream_requests = first_server.finish_all();
    assert_eq!(upstream_requests.len(), 2);
    assert!(
        upstream_requests
            .iter()
            .all(|request| request.path == "/v1/chat/completions")
    );
    assert!(!upstream_requests[1].body.contains("compaction_trigger"));
    let first: Value = serde_json::from_str(&upstream_requests[0].body).unwrap();
    let retry: Value = serde_json::from_str(&upstream_requests[1].body).unwrap();
    assert!(!first["tools"].as_array().unwrap().is_empty());
    assert_eq!(retry["tools"], first["tools"]);
    assert_eq!(retry["tool_choice"], first["tool_choice"]);
    assert_eq!(retry["messages"][0], first["messages"][0]);
    assert!(
        upstream_requests[1].body.contains("[Handoff checkpoint]"),
        "actual retry request: {}",
        upstream_requests[1].body
    );
}

#[tokio::test]
async fn responses_proxy_legacy_compaction_blank_override_uses_project_default_prompt() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server();
    let relay_id = "legacy-compaction-default".to_string();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: relay_id.clone(),
            name: "legacy compaction".to_string(),
            base_url: server.base_url.clone(),
            api_key: "sk-legacy".to_string(),
            protocol: RelayProtocol::Responses,
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "gpt-5-mini".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: relay_id,
        layered_compaction_enabled: true,
        layered_compaction_prompt_override: String::new(),
        ..BackendSettings::default()
    };
    let request = json!({
        "model": "gpt-5-mini",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "earlier request" }]
            },
            {
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "recent answer" }]
            },
            { "type": "function_call", "name": "exec_command", "call_id": "call_recent", "arguments": "{}" },
            { "type": "function_call_output", "call_id": "call_recent", "output": "tool result" },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "recent request" }]
            },
            {
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
                }]
            }
        ]
    });

    let upstream = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(upstream.status_code, 200);
    let captured = server.finish();
    let forwarded: Value = serde_json::from_str(&captured.body).unwrap();

    assert_eq!(forwarded["input"].as_array().unwrap().len(), 6);
    assert_eq!(
        &forwarded["input"].as_array().unwrap()[..5],
        &request["input"].as_array().unwrap()[..5],
        "关闭补回时摘要模型仍应收到完整最近一轮及工具调用配对"
    );
    assert_eq!(
        forwarded["input"][5]["content"][0]["text"],
        codex_elves_core::layered_compaction::compaction_instruction("")
    );
}

#[tokio::test]
async fn aggregate_remote_compaction_retries_same_candidate_after_2xx_invalid_body() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first_server = spawn_chat_server_with_responses(vec![
        "not-json".to_string(),
        json!({
            "id": "chatcmpl_after_invalid_body",
            "created": 0,
            "model": "gpt-5-mini",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "<summary>SUMMARY AFTER INVALID BODY</summary>"
                },
                "finish_reason": "stop"
            }]
        })
        .to_string(),
    ]);
    let mut settings = aggregate_proxy_settings(
        "compaction-invalid-body-same-candidate",
        first_server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::Failover;
    for relay in &mut settings.relay_profiles[..2] {
        relay.protocol = RelayProtocol::ChatCompletions;
        relay.model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    }
    let mut request = remote_compaction_v2_request();
    settings.layered_compaction_enabled = true;
    request["model"] = json!("gpt-5-mini");
    request["stream"] = json!(false);

    let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();

    assert_eq!(result.status_code, 200);
    assert_eq!(
        result.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let upstream_requests = first_server.finish_all();
    assert_eq!(upstream_requests.len(), 2);
    assert!(
        upstream_requests
            .iter()
            .all(|request| request.path == "/v1/chat/completions")
    );
}

#[tokio::test]
async fn aggregate_remote_compaction_does_not_switch_candidate_after_2xx_truncated_body() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first_server = spawn_truncated_response_server();
    let mut settings = aggregate_proxy_settings(
        "compaction-truncated-body-same-candidate",
        first_server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::Failover;
    for relay in &mut settings.relay_profiles[..2] {
        relay.protocol = RelayProtocol::ChatCompletions;
        relay.model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    }
    let mut request = remote_compaction_v2_request();
    settings.layered_compaction_enabled = true;
    request["model"] = json!("gpt-5-mini");
    request["stream"] = json!(false);

    let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();

    assert_eq!(result.status_code, 200);
    assert_eq!(
        result.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let response: Value = serde_json::from_slice(&result.into_body_bytes().await.unwrap()).unwrap();
    assert_eq!(response["status"], "failed");
    assert_eq!(first_server.finish().path, "/v1/chat/completions");
}

#[tokio::test]
async fn aggregate_remote_compaction_stream_retries_same_candidate_after_unfinished_2xx_body() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let first_server = spawn_chat_server_with_responses(vec![
        r#"data: {"id":"chatcmpl_unfinished","created":0,"model":"gpt-5-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"PARTIAL"}}]}

"#
        .to_string(),
        r#"data: {"id":"chatcmpl_stream_retry","created":0,"model":"gpt-5-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"<summary>STREAM SUMMARY AFTER RETRY</summary>"}}]}

data: {"id":"chatcmpl_stream_retry","created":0,"model":"gpt-5-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}

data: [DONE]

"#
        .to_string(),
    ]);
    let mut settings = aggregate_proxy_settings(
        "compaction-stream-body-same-candidate",
        first_server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::Failover;
    for relay in &mut settings.relay_profiles[..2] {
        relay.protocol = RelayProtocol::ChatCompletions;
        relay.model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    }
    let mut request = remote_compaction_v2_request();
    settings.layered_compaction_enabled = true;
    request["model"] = json!("gpt-5-mini");
    request["stream"] = json!(true);

    let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();

    assert_eq!(result.status_code, 200);
    assert!(result.is_stream);
    assert_eq!(
        result.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let upstream_requests = first_server.finish_all();
    assert_eq!(upstream_requests.len(), 2);
    assert!(
        upstream_requests
            .iter()
            .all(|request| request.path == "/v1/chat/completions")
    );
}

#[tokio::test]
async fn aggregate_anthropic_retry_body_failure_does_not_switch_candidate() {
    let _lock = settings_path_test_lock().lock().unwrap();
    clear_anthropic_reasoning_compatibility_cache_for_tests();
    let first_server = spawn_truncated_response_server_with_status("400 Bad Request");
    let mut settings = aggregate_proxy_settings(
        "anthropic-retry-read-failure",
        first_server.base_url.clone(),
        "http://127.0.0.1:9/v1".to_string(),
    );
    settings.relay_profiles[0].protocol = RelayProtocol::Anthropic;
    settings.relay_profiles[0].model_mappings[0].request_model = "claude-sonnet-5".to_string();
    settings.relay_profiles[0].model_mappings[0].protocol = RelayProtocol::Anthropic;
    settings.relay_profiles[1].protocol = RelayProtocol::ChatCompletions;
    settings.relay_profiles[1].model_mappings[0].request_model = "claude-sonnet-5".to_string();
    settings.relay_profiles[1].model_mappings[0].protocol = RelayProtocol::ChatCompletions;
    let mut request = remote_compaction_v2_request();
    settings.layered_compaction_enabled = true;
    request["model"] = json!("claude-sonnet-5");
    request["stream"] = json!(false);
    request["reasoning"] = json!({ "effort": "max" });

    let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();

    assert_eq!(result.status_code, 200);
    assert_eq!(
        result.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let response: Value = serde_json::from_slice(&result.into_body_bytes().await.unwrap()).unwrap();
    assert_eq!(response["status"], "failed");
    assert_eq!(first_server.finish().path, "/v1/messages");
    codex_elves_core::relay_rotation::record_relay_request_event(
        &BackendSettings::default(),
        codex_elves_core::relay_rotation::RotationEvent::Success,
    );
    clear_anthropic_reasoning_compatibility_cache_for_tests();
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
    let _lock = settings_path_test_lock().lock().unwrap();
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
                system_prompt_override: String::new(),
                request_model: "gpt-responses".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "responses".to_string(),
        ..BackendSettings::default()
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-codex-beta-features",
        reqwest::header::HeaderValue::from_static("remote_compaction_v2"),
    );
    let request_context = RequestContext::from_headers(headers);
    let result = apply_continue_thinking_to_responses_stream_with_request_context(
        &json!({
            "model": "gpt-responses",
            "input": "hi",
            "stream": true,
            "reasoning": { "effort": "high" }
        }),
        settings,
        &request_context,
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
    assert_eq!(request.x_codex_beta_features, "remote_compaction_v2");
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
                system_prompt_override: String::new(),
                request_model: "gpt-responses".to_string(),
                alias: String::new(),
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
async fn prompt_only_compaction_does_not_trigger_gpt_reasoning_continuation() {
    let settings = BackendSettings {
        layered_compaction_enabled: true,
        layered_compaction_retain_recent_round_enabled: false,
        gpt_reasoning_continuation: true,
        ..BackendSettings::default()
    };
    let first_round = responses_sse_with_reasoning("resp_compaction_summary", 516);
    let result = apply_continue_thinking_to_responses_stream(
        &json!({
            "model": "gpt-5.6",
            "stream": true,
            "reasoning": { "effort": "high" },
            "input": [
                { "type": "message", "role": "user", "content": "recent request" },
                {
                    "type": "message",
                    "role": "user",
                    "content": "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a summary."
                }
            ]
        }),
        settings,
        None,
        first_round.clone(),
    )
    .await;

    assert!(!result.triggered);
    assert_eq!(result.rounds, 0);
    assert!(result.request_body.is_none());
    assert_eq!(result.sse_text, first_round);
}

#[tokio::test]
async fn continue_thinking_respects_configured_max_rounds() {
    let _lock = settings_path_test_lock().lock().unwrap();
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
                system_prompt_override: String::new(),
                request_model: "gpt-responses".to_string(),
                alias: String::new(),
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
                    system_prompt_override: String::new(),
                    request_model: "gpt-5-mini".to_string(),
                    alias: String::new(),
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
                    system_prompt_override: String::new(),
                    request_model: "gpt-5-mini".to_string(),
                    alias: String::new(),
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
async fn responses_proxy_rewrites_alias_slug_and_prompt_identity_to_request_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "duplicate-alias".to_string(),
            name: "Duplicate alias".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            relay_mode: RelayMode::PureApi,
            model_mappings: vec![
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "372000".to_string(),
                },
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: "gpt-5.6-sol [500K]".to_string(),
                    protocol: RelayProtocol::Responses,
                    context_window: "500000".to_string(),
                },
            ],
            ..RelayProfile::default()
        }],
        active_relay_id: "duplicate-alias".to_string(),
        ..BackendSettings::default()
    };

    let upstream = open_responses_proxy_request_with_settings(
        r#"{"model":"gpt-5.6-sol [500K]","instructions":"You are Codex, an agent based on the gpt-5.6-sol [500K] model.","input":"hello","stream":false}"#,
        settings,
    )
    .await
    .unwrap();

    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "gpt-5.6-sol");
    assert_eq!(
        body["instructions"],
        "You are Codex, an agent based on the gpt-5.6-sol model."
    );
}

#[tokio::test]
async fn responses_proxy_accepts_legacy_alias_slug_without_forwarding_it() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "legacy-duplicate-alias".to_string(),
            name: "Legacy duplicate alias".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            relay_mode: RelayMode::PureApi,
            model_mappings: vec![
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "372000".to_string(),
                },
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: "gpt-5.6-sol [500K]".to_string(),
                    protocol: RelayProtocol::Responses,
                    context_window: "500000".to_string(),
                },
            ],
            ..RelayProfile::default()
        }],
        active_relay_id: "legacy-duplicate-alias".to_string(),
        ..BackendSettings::default()
    };

    open_responses_proxy_request_with_settings(
        r#"{"model":"gpt-5.6-sol--codex-elves-alias-2","instructions":"You are Codex, an agent based on the gpt-5.6-sol--codex-elves-alias-2 model.","input":"hello","stream":false}"#,
        settings,
    )
    .await
    .unwrap();

    let request = server.finish();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "gpt-5.6-sol");
    assert_eq!(
        body["instructions"],
        "You are Codex, an agent based on the gpt-5.6-sol model."
    );
}

#[tokio::test]
async fn responses_proxy_legacy_ambiguous_alias_prefers_legacy_request_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "legacy-ambiguous-alias".to_string(),
            name: "Legacy ambiguous alias".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            relay_mode: RelayMode::PureApi,
            model_mappings: vec![
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "model-a".to_string(),
                    alias: "model-b".to_string(),
                    protocol: RelayProtocol::ChatCompletions,
                    context_window: "400000".to_string(),
                },
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "model-b".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "500000".to_string(),
                },
            ],
            ..RelayProfile::default()
        }],
        active_relay_id: "legacy-ambiguous-alias".to_string(),
        ..BackendSettings::default()
    };

    let upstream = open_responses_proxy_request_with_settings(
        r#"{"model":"model-b","input":"hello","stream":false}"#,
        settings,
    )
    .await
    .unwrap();

    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Responses
    );
    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "model-b");
}

#[tokio::test]
async fn responses_proxy_clamps_unsupported_gpt_reasoning_to_model_max() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-responses","input":"hello","stream":false,"reasoning":{"effort":"ultra"},"model_reasoning_effort":"max","reasoning_effort":"ultra"}"#,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Responses
    );

    let request = server.finish();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["reasoning"]["effort"], "xhigh");
    assert_eq!(body["model_reasoning_effort"], "xhigh");
    assert_eq!(body["reasoning_effort"], "xhigh");
}

#[tokio::test]
async fn responses_proxy_rejects_conflicting_duplicate_model_mappings() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "conflict".to_string(),
            name: "Conflict".to_string(),
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: "sk-test".to_string(),
            relay_mode: RelayMode::PureApi,
            model_mappings: vec![
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "shared-model".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "200000".to_string(),
                },
                RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "shared-model".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::ChatCompletions,
                    context_window: "200000".to_string(),
                },
            ],
            ..RelayProfile::default()
        }],
        active_relay_id: "conflict".to_string(),
        ..BackendSettings::default()
    };

    let error = match open_responses_proxy_request_with_settings(
        r#"{"model":"shared-model","input":"hello","stream":false}"#,
        settings,
    )
    .await
    {
        Ok(_) => panic!("冲突协议归属应拒绝请求"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("冲突协议归属"), "{error:#}");
}

#[tokio::test]
async fn native_remote_compaction_non_stream_preserves_success_status() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_status_response_server(
        "201 Created",
        r#"{"id":"resp_native_compaction","object":"response","status":"completed","output":[{"type":"compaction","encrypted_content":"opaque"}]}"#,
    );
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-responses");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();

    assert_eq!(response.status, "201 Created");
    assert_eq!(server.finish().path, "/v1/responses");
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
async fn remote_compaction_v2_chat_malformed_json_response_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server_with_response("not-json");
    write_mixed_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-chat");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/chat/completions");
}

#[tokio::test]
async fn remote_compaction_v2_anthropic_malformed_json_response_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server_with_response("not-json");
    write_mixed_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("claude-sonnet-4");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/messages");
}

#[tokio::test]
async fn remote_compaction_v2_chat_truncated_body_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_truncated_response_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-chat");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/chat/completions");
}

#[tokio::test]
async fn remote_compaction_v2_anthropic_truncated_body_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_truncated_response_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("claude-sonnet-4");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/messages");
}

#[tokio::test]
async fn remote_compaction_v2_upstream_connection_failure_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    write_mixed_relay_settings(temp.path(), "not-a-valid-url");
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("claude-sonnet-4");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
}

#[tokio::test]
async fn remote_compaction_v2_non_success_status_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_status_response_server(
        "500 Internal Server Error",
        r#"{"error":{"message":"upstream exploded"}}"#,
    );
    write_mixed_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("gpt-chat");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/chat/completions");
}

#[tokio::test]
async fn remote_compaction_v2_final_anthropic_retry_body_failure_fails_closed() {
    let _lock = settings_path_test_lock().lock().unwrap();
    clear_anthropic_reasoning_compatibility_cache_for_tests();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_truncated_response_server_with_status("400 Bad Request");
    write_anthropic_sonnet5_relay_settings(temp.path(), &server.base_url);
    enable_compaction_for_test(temp.path());
    let mut request = remote_compaction_v2_request();
    request["model"] = json!("claude-sonnet-5");
    request["stream"] = json!(false);
    request["reasoning"] = json!({ "effort": "max" });

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response.status, "200 OK");
    assert_eq!(body["status"], "failed");
    assert_eq!(body["output"], json!([]));
    assert_eq!(
        body["error"]["code"],
        "remote_compaction_no_terminal_response"
    );
    assert_eq!(server.finish().path, "/v1/messages");
}

#[tokio::test]
async fn responses_proxy_model_system_prompt_wins_for_all_protocols_and_aliases() {
    let _lock = settings_path_test_lock().lock().unwrap();
    for (model, protocol, path) in [
        ("gpt-responses", RelayProtocol::Responses, "/v1/responses"),
        (
            "gpt-chat",
            RelayProtocol::ChatCompletions,
            "/v1/chat/completions",
        ),
        ("claude-sonnet", RelayProtocol::Anthropic, "/v1/messages"),
    ] {
        for (alias, prompt) in [
            ("custom", "custom prompt: GPT-5.6 Sol"),
            ("inherit", "supplier prompt"),
        ] {
            let server = spawn_chat_server();
            let settings = BackendSettings {
                relay_profiles: vec![RelayProfile {
                    id: "prompts".to_string(),
                    name: "Prompts".to_string(),
                    base_url: server.base_url.clone(),
                    upstream_base_url: server.base_url.clone(),
                    api_key: "sk-test".to_string(),
                    relay_mode: RelayMode::MixedApi,
                    system_prompt_override: "supplier prompt".to_string(),
                    model_mappings: vec![
                        RelayModelMapping {
                            request_model: model.to_string(),
                            alias: "custom".to_string(),
                            protocol,
                            context_window: "200000".to_string(),
                            system_prompt_override: "custom prompt: GPT-5.6 Sol".to_string(),
                        },
                        RelayModelMapping {
                            request_model: model.to_string(),
                            alias: "inherit".to_string(),
                            protocol,
                            context_window: "200000".to_string(),
                            system_prompt_override: " \n ".to_string(),
                        },
                    ],
                    ..RelayProfile::default()
                }],
                active_relay_id: "prompts".to_string(),
                ..BackendSettings::default()
            };
            let upstream = open_responses_proxy_request_with_settings(
                &json!({
                    "model": alias,
                    "instructions": "old system",
                    "input": [
                        {"type": "message", "role": "developer", "content": "old developer"},
                        {"type": "message", "role": "user", "content": "hello"}
                    ],
                    "stream": false
                })
                .to_string(),
                settings,
            )
            .await
            .unwrap();
            assert_eq!(upstream.status_code, 200);
            let request = server.finish();
            assert_eq!(request.path, path);
            let body: Value = serde_json::from_str(&request.body).unwrap();
            assert_eq!(body["model"], model);
            match protocol {
                RelayProtocol::Responses => {
                    assert_eq!(body["instructions"], prompt);
                    assert_eq!(body["input"].as_array().unwrap().len(), 1);
                    assert_eq!(body["input"][0]["role"], "user");
                }
                RelayProtocol::ChatCompletions => {
                    assert_eq!(
                        body["messages"][0],
                        json!({"role": "system", "content": prompt})
                    );
                }
                RelayProtocol::Anthropic => {
                    assert!(body["system"].to_string().contains(prompt));
                }
            }
            assert!(!request.body.contains("old system"));
            assert!(!request.body.contains("old developer"));
            if alias == "custom" {
                assert!(!request.body.contains("supplier prompt"));
            }
        }
    }
}

#[tokio::test]
async fn chat_proxy_model_system_prompt_is_selected_before_alias_is_rewritten() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let _guard = SettingsPathGuard::set(path.clone());
    let server = spawn_chat_server();
    std::fs::write(path, json!({
        "activeRelayId": "prompts",
        "relayProfiles": [{
            "id": "prompts",
            "name": "Prompts",
            "baseUrl": server.base_url,
            "upstreamBaseUrl": server.base_url,
            "apiKey": "sk-test",
            "relayMode": "mixedApi",
            "systemPromptOverride": "supplier prompt",
            "modelMappings": [
                {"requestModel": "gpt-chat", "alias": "first", "protocol": "chatCompletions", "systemPromptOverride": "first prompt"},
                {"requestModel": "gpt-chat", "alias": "second", "protocol": "chatCompletions", "systemPromptOverride": "second prompt"}
            ]
        }]
    }).to_string()).unwrap();
    let upstream = open_chat_completions_proxy_request(
        &json!({
            "model": "second",
            "messages": [
                {"role": "system", "content": "old system"},
                {"role": "developer", "content": "old developer"},
                {"role": "user", "content": "hello"}
            ],
            "stream": false
        })
        .to_string(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    let request = server.finish();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "gpt-chat");
    assert_eq!(
        body["messages"],
        json!([
            {"role": "system", "content": "second prompt"},
            {"role": "user", "content": "hello"}
        ])
    );
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
async fn responses_proxy_accepts_anthropic_history_after_switching_to_responses_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server();
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "responses".to_string(),
            name: "Responses".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::Responses,
            relay_mode: RelayMode::MixedApi,
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "gpt-responses".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "responses".to_string(),
        ..BackendSettings::default()
    };
    let converted = anthropic_message_to_response_with_request(
        json!({
            "id": "msg_current",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4",
            "content": [{ "type": "text", "text": "current answer" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 8, "output_tokens": 3 }
        }),
        &json!({
            "model": "claude-sonnet-4",
            "input": "current question"
        }),
    )
    .unwrap();
    let current_message = converted["output"][0].clone();
    let legacy_message = json!({
        "id": "resp_msg_legacy_msg",
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": "legacy answer", "annotations": [] }]
    });
    let request_body = json!({
        "model": "gpt-responses",
        "input": [
            legacy_message,
            current_message,
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "continue" }]
            }
        ],
        "stream": false
    })
    .to_string();

    let upstream = open_responses_proxy_request_with_settings(&request_body, settings)
        .await
        .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Responses
    );

    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(logged_body["input"][0]["id"], "msg_legacy");
    assert_eq!(logged_body["input"][1]["id"], "msg_current");

    let forwarded = server.finish();
    assert_eq!(forwarded.path, "/v1/responses");
    let forwarded_body: Value = serde_json::from_str(&forwarded.body).unwrap();
    assert_eq!(forwarded_body["input"][0]["id"], "msg_legacy");
    assert_eq!(forwarded_body["input"][1]["id"], "msg_current");
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
async fn responses_proxy_rewrites_inherited_claude_identity_before_anthropic_conversion() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"glm-5.2","instructions":"You are Codex, a coding agent based on the claude-sonnet-5 model. Keep claude-sonnet-5 compatibility notes.","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(upstream.status_code, 200);
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    let logged_body: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(
        logged_body["system"],
        "You are Codex, a coding agent based on the glm-5.2 model. Keep claude-sonnet-5 compatibility notes."
    );
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["system"], logged_body["system"]);
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
async fn responses_proxy_keeps_deepseek_adaptive_for_anthropic_upstream() {
    let _lock = settings_path_test_lock().lock().unwrap();
    clear_anthropic_reasoning_compatibility_cache_for_tests();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"deepseek-v4-flash","input":"hello","stream":false,"reasoning":{"effort":"max"}}"#,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "deepseek-v4-flash");
    assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(body["output_config"], json!({ "effort": "max" }));
}

#[tokio::test]
async fn responses_proxy_preserves_deepseek_high_for_cli_proxy_api_gateway() {
    let _lock = settings_path_test_lock().lock().unwrap();
    clear_anthropic_reasoning_compatibility_cache_for_tests();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_cli_proxy_api_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"deepseek-v4-flash","input":"hello","stream":false,"reasoning":{"effort":"high"}}"#,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "deepseek-v4-flash");
    assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(body["output_config"], json!({ "effort": "high" }));
}

fn deepseek_reasoning_replay_error() -> String {
    json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": "Error from provider (Console Go): Upstream request failed: [invalid_request_error] The `reasoning_content` in the thinking mode must be passed back to the API."
        }
    })
    .to_string()
}

#[tokio::test]
async fn deepseek_reasoning_replay_retries_rejected_anthropic_history_as_chat() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server_with_status_responses(vec![
        ("400 Bad Request".into(), deepseek_reasoning_replay_error()),
        (
            "200 OK".into(),
            json!({
                "id": "chatcmpl-replay", "model": "deepseek-v4-flash",
                "choices": [{
                    "message": { "role": "assistant", "reasoning_content": "Continue safely.", "content": "Ready." },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
            })
            .to_string(),
        ),
    ]);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let mut request = compacted_thinking_history(true, true);
    request["model"] = json!("deepseek-v4-flash");
    request["stream"] = json!(false);

    let response = handle_responses_proxy_request(&request.to_string())
        .await
        .unwrap();
    assert_eq!(response.status, "200 OK");
    let body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["output"][0]["reasoning_content"], "Continue safely.");
    assert_eq!(body["output"][1]["content"][0]["text"], "Ready.");
    let requests = server.finish_all();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(requests[1].path, "/v1/chat/completions");
    let first: Value = serde_json::from_str(&requests[0].body).unwrap();
    let retry: Value = serde_json::from_str(&requests[1].body).unwrap();
    assert_eq!(first["thinking"]["type"], "adaptive");
    assert_eq!(retry["model"], first["model"]);
    assert_eq!(retry["reasoning_effort"], "max");
    let assistants: Vec<_> = retry["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "assistant")
        .collect();
    assert_eq!(assistants.len(), 2);
    assert_eq!(assistants[0]["reasoning_content"], "anchor reasoning");
    assert_eq!(assistants[1]["reasoning_content"], "tool reasoning");
    assert_eq!(assistants[1]["tool_calls"][0]["id"], "call-history");
    assert!(
        retry["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == "call-history")
    );
    assert!(
        retry["messages"]
            .to_string()
            .contains("Earlier history summary")
    );
    assert!(
        retry["messages"]
            .to_string()
            .contains("data:image/png;base64,aGVsbG8=")
    );
}

#[tokio::test]
async fn deepseek_reasoning_replay_stream_preserves_effort_without_requiring_compaction() {
    let _lock = settings_path_test_lock().lock().unwrap();
    for effort in [None, Some("low"), Some("max")] {
        let temp = tempfile::tempdir().unwrap();
        let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
        let server = spawn_chat_server_with_status_responses(vec![
            ("400 Bad Request".into(), deepseek_reasoning_replay_error()),
            (
                "200 OK".into(),
                concat!(
                    "data: {\"id\":\"chatcmpl-replay\",\"model\":\"deepseek-v4-flash\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Check the result.\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Ready.\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                ).into(),
            ),
        ]);
        write_mixed_relay_settings(temp.path(), &server.base_url);
        let mut request = compacted_thinking_history(true, true);
        // 使用无压缩标记的完整历史，兼容恢复旧任务、供应商缓存失效等相同回放场景。
        let payload = request["input"][0]["encrypted_content"].as_str().unwrap();
        let payload: Value =
            serde_json::from_str(payload.strip_prefix("codex-elves-compaction-v3:").unwrap())
                .unwrap();
        request["input"] = payload["retained_tail"].clone();
        request["input"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({ "role": "user", "content": "Start the task." }));
        request["model"] = json!("deepseek-v4-flash");
        request["stream"] = json!(true);
        request["max_output_tokens"] = json!(1024);
        if let Some(effort) = effort {
            request["reasoning"] = json!({ "effort": effort });
        } else {
            request.as_object_mut().unwrap().remove("reasoning");
        }

        let response = handle_responses_proxy_request(&request.to_string())
            .await
            .unwrap();
        assert_eq!(response.status, "200 OK");
        let sse = String::from_utf8(response.body).unwrap();
        assert_eq!(collect_stream_output_text(&sse), "Ready.");
        let completed = parse_response_sse_events(&sse)
            .into_iter()
            .find(|event| event.event == "response.completed")
            .unwrap();
        assert_eq!(
            completed.data["response"]["output"][0]["reasoning_content"],
            "Check the result."
        );
        let requests = server.finish_all();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/v1/messages");
        assert_eq!(requests[1].path, "/v1/chat/completions");
        let first: Value = serde_json::from_str(&requests[0].body).unwrap();
        let retry: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(retry["thinking"]["type"], "enabled");
        assert_eq!(retry["reasoning_effort"], first["output_config"]["effort"]);
        assert_eq!(retry["max_tokens"], 1024);
        assert_eq!(retry["stream"], true);
    }
}

#[tokio::test]
async fn deepseek_reasoning_replay_does_not_retry_unrelated_errors_or_protocols() {
    let _lock = settings_path_test_lock().lock().unwrap();
    for scenario in [
        "unrelated",
        "status-500",
        "disabled",
        "no-tools",
        "claude",
        "native-responses",
        "missing-reasoning",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
        let error = if scenario == "unrelated" {
            json!({ "error": { "type": "invalid_request_error", "message": "Invalid tools schema" } }).to_string()
        } else {
            deepseek_reasoning_replay_error()
        };
        let status = if scenario == "status-500" {
            "500 Internal Server Error"
        } else {
            "400 Bad Request"
        };
        let server = spawn_chat_server_with_status_responses(vec![(status.into(), error.clone())]);
        write_mixed_relay_settings(temp.path(), &server.base_url);
        let mut request = compacted_thinking_history(true, true);
        request["model"] = json!("deepseek-v4-flash");
        request["stream"] = json!(false);
        let mut settings = codex_elves_core::settings::SettingsStore::default()
            .load()
            .unwrap();
        match scenario {
            "disabled" => request["reasoning"] = json!({ "effort": "none" }),
            "no-tools" => {
                request.as_object_mut().unwrap().remove("tools");
            }
            "claude" => request["model"] = json!("claude-sonnet-4"),
            "native-responses" => {
                settings.relay_profiles[0]
                    .model_mappings
                    .iter_mut()
                    .find(|mapping| mapping.request_model == "deepseek-v4-flash")
                    .unwrap()
                    .protocol = RelayProtocol::Responses;
            }
            "missing-reasoning" => {
                request["input"] = json!([
                    { "role": "user", "content": "Start." },
                    { "role": "assistant", "content": "Historical answer with no reasoning." },
                    { "role": "user", "content": "Continue." }
                ]);
            }
            _ => {}
        }
        let upstream = open_responses_proxy_request_with_settings(&request.to_string(), settings)
            .await
            .unwrap();
        assert_eq!(
            upstream.status_code,
            if scenario == "status-500" { 500 } else { 400 },
            "{scenario}"
        );
        assert_eq!(
            String::from_utf8(upstream.into_body_bytes().await.unwrap()).unwrap(),
            error,
            "{scenario}"
        );
        let requests = server.finish_all();
        assert_eq!(requests.len(), 1, "{scenario}");
        assert_eq!(
            requests[0].path,
            if scenario == "native-responses" {
                "/v1/responses"
            } else {
                "/v1/messages"
            },
            "{scenario}"
        );
    }
}

#[tokio::test]
async fn deepseek_reasoning_replay_stops_after_one_rejected_chat_retry() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server_with_status_responses(vec![
        ("400 Bad Request".into(), deepseek_reasoning_replay_error()),
        ("400 Bad Request".into(), deepseek_reasoning_replay_error()),
    ]);
    write_mixed_relay_settings(temp.path(), &server.base_url);
    let mut request = compacted_thinking_history(true, true);
    request["model"] = json!("deepseek-v4-flash");
    request["stream"] = json!(false);
    let upstream = open_responses_proxy_request(&request.to_string(), None)
        .await
        .unwrap();
    assert_eq!(upstream.status_code, 400);
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::ChatCompletions
    );
    assert!(
        upstream
            .endpoint
            .as_deref()
            .unwrap()
            .ends_with("/v1/chat/completions")
    );
    let logged: Value = serde_json::from_str(&upstream.request_body).unwrap();
    assert_eq!(logged["reasoning_effort"], "max");
    assert!(logged.get("output_config").is_none());
    let requests = server.finish_all();
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn responses_proxy_infers_responses_for_unlisted_gpt_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_mixed_relay_settings(temp.path(), &server.base_url);

    let upstream = open_responses_proxy_request(
        r#"{"model":"gpt-5.3-codex-spark","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Responses
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/responses");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "gpt-5.3-codex-spark");
}

#[tokio::test]
async fn responses_proxy_infers_anthropic_for_unlisted_non_gpt_non_claude_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _guard = SettingsPathGuard::set(temp.path().join("settings.json"));
    let server = spawn_chat_server();
    write_chat_relay_settings(temp.path(), &server.base_url, "");

    let upstream = open_responses_proxy_request(
        r#"{"model":"deepseek-v5","input":"hello","stream":false}"#,
        Some("Original-Codex-UA/1.0"),
    )
    .await
    .unwrap();
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/messages");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "deepseek-v5");
}

#[tokio::test]
async fn responses_proxy_infers_anthropic_for_unlisted_claude_model() {
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
    assert_eq!(
        upstream.response_protocol,
        UpstreamResponseProtocol::Anthropic
    );

    let request = server.finish();
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.x_api_key, "sk-test");
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "claude-unlisted");
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

#[test]
fn chat_post_compaction_request_drops_old_images_before_translation() {
    let converted = responses_to_chat_completions(json!({
        "model": "glm-5.2",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "old image caption" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,OLD_CHAT_IMAGE"
                    }
                ]
            },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "text-only protocol anchor" }]
            },
            {
                "type": "compaction",
                "encrypted_content": "codex-elves-compaction-v2:CHAT HANDOFF SUMMARY"
            },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "current request" }]
            }
        ]
    }))
    .unwrap();
    let wire = converted.to_string();

    assert!(!wire.contains("OLD_CHAT_IMAGE"));
    assert!(!wire.contains("old image caption"));
    assert!(wire.contains("text-only protocol anchor"));
    assert!(wire.contains("CHAT HANDOFF SUMMARY"));
    assert!(wire.contains("current request"));
}

#[test]
fn anthropic_post_compaction_request_keeps_canonical_context_but_drops_old_images() {
    let converted = responses_to_anthropic_messages(json!({
        "model": "claude-sonnet-5",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "old anthropic image caption" },
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,OLD_ANTHROPIC_IMAGE"
                    }
                ]
            },
            {
                "type": "message",
                "role": "developer",
                "content": [{
                    "type": "input_text",
                    "text": "canonical developer instruction"
                }]
            },
            {
                "type": "compaction",
                "encrypted_content": "codex-elves-compaction-v2:ANTHROPIC HANDOFF SUMMARY"
            },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "current anthropic request" }]
            }
        ]
    }))
    .unwrap();
    let wire = converted.to_string();

    assert!(!wire.contains("OLD_ANTHROPIC_IMAGE"));
    assert!(!wire.contains("old anthropic image caption"));
    assert!(wire.contains("canonical developer instruction"));
    assert!(wire.contains("ANTHROPIC HANDOFF SUMMARY"));
    assert!(wire.contains("current anthropic request"));
}

mod post_compaction_boundaries {
    use super::*;
    use codex_elves_core::layered_compaction::{
        COMPACTION_PROMPT_PREFIX, CompactionAttempt, CompactionKind, CompactionOptions,
        CompactionRoute, prepare_compaction_attempt_request,
    };

    fn user(text: &str) -> Value {
        json!({"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": text}
        ]})
    }

    fn marker() -> Value {
        json!({
            "type": "compaction",
            "encrypted_content": "codex-elves-compaction-v2:VERIFIED HANDOFF"
        })
    }

    fn request(input: Vec<Value>) -> Value {
        json!({
            "model": "claude-test",
            "instructions": "stable system",
            "input": input,
            "tools": [{"type": "function", "name": "shell", "parameters": {"type": "object"}}]
        })
    }

    fn declaration(kind: &str, namespace: &str, name: &str) -> Value {
        json!({
            "type": kind,
            "tools": [{
                "type": "namespace", "name": namespace,
                "tools": [{"type": "function", "name": name, "parameters": {"type": "object"}}]
            }]
        })
    }

    fn convert(source: &Value, anthropic: bool) -> Value {
        if anthropic {
            responses_to_anthropic_messages(source.clone()).unwrap()
        } else {
            responses_to_chat_completions(source.clone()).unwrap()
        }
    }

    #[test]
    fn dynamic_declarations_survive_compaction_and_forced_choice() {
        for kind in ["additional_tools", "tool_search_output"] {
            let mut source = request(vec![
                declaration(kind, "files", "read"),
                user("old task"),
                marker(),
                user("read the file"),
            ]);
            source["tool_choice"] =
                json!({"type": "function", "namespace": "files", "name": "read"});
            source["text"] = json!({
                "format":{
                    "type":"json_schema",
                    "name":"answer",
                    "schema":{"type":"object"}
                }
            });
            for anthropic in [false, true] {
                let wire = convert(&source, anthropic);
                assert_eq!(wire["tools"].as_array().unwrap().len(), 2);
                let name = if anthropic {
                    &wire["tool_choice"]["name"]
                } else {
                    &wire["tool_choice"]["function"]["name"]
                };
                assert_eq!(name, "files__read", "{kind}, anthropic={anthropic}");
            }
        }
    }

    #[test]
    fn compaction_retained_round_keeps_dynamic_tools_and_disables_forced_choice() {
        for kind in ["additional_tools", "tool_search_output"] {
            let mut source = request(vec![
                marker(),
                user("earlier task"),
                json!({"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "previous answer"}
                ]}),
                user("current task"),
                declaration(kind, "files", "read"),
                json!({"type": "message", "role": "developer", "content": [
                    {"type": "input_text", "text": "TAIL_SYSTEM_RULE"}
                ]}),
            ]);
            source["tool_choice"] =
                json!({"type": "function", "namespace": "files", "name": "read"});
            source["text"] = json!({
                "format":{
                    "type":"json_schema",
                    "name":"answer",
                    "schema":{"type":"object"}
                }
            });
            for anthropic in [false, true] {
                let ordinary = convert(&source, anthropic);
                let mut compact = source.clone();
                compact["input"]
                    .as_array_mut()
                    .unwrap()
                    .push(user(COMPACTION_PROMPT_PREFIX));
                let prepared = prepare_compaction_attempt_request(
                    &compact,
                    CompactionKind::Legacy,
                    CompactionRoute::CacheReuse,
                    &CompactionOptions {
                        retain_recent_round: true,
                        ..Default::default()
                    },
                    CompactionAttempt::First,
                );
                let wire = convert(&prepared, anthropic);
                assert_eq!(wire["tools"], ordinary["tools"]);
                assert_eq!(
                    wire["tool_choice"],
                    if anthropic {
                        json!({"type":"none"})
                    } else {
                        json!("none")
                    }
                );
                assert_eq!(wire["system"], ordinary["system"]);
                if anthropic {
                    assert!(wire.pointer("/output_config/format").is_none());
                } else {
                    assert_eq!(wire["response_format"], json!({"type":"text"}));
                }
                assert!(wire.to_string().contains("TAIL_SYSTEM_RULE"));
                assert!(!wire.to_string().contains("current task"));
                assert!(!wire.to_string().contains("previous answer"));
            }
        }
    }

    #[test]
    fn compaction_first_attempt_and_retry_preserve_dynamic_tools() {
        for structured in [false, true] {
            let tools = vec![
                declaration("additional_tools", "files", "read"),
                declaration("tool_search_output", "files", "write"),
            ];
            let mut input = if structured {
                vec![json!({
                    "type": "compaction",
                    "encrypted_content": format!("codex-elves-compaction-v3:{}", json!({
                        "summary": "previous summary", "retained_tail": tools
                    }))
                })]
            } else {
                tools
            };
            input.extend([user("current task"), user(COMPACTION_PROMPT_PREFIX)]);
            let source = request(input);
            for attempt in CompactionAttempt::ALL {
                let prepared = prepare_compaction_attempt_request(
                    &source,
                    CompactionKind::Legacy,
                    CompactionRoute::CacheReuse,
                    &CompactionOptions::default(),
                    attempt,
                );
                for anthropic in [false, true] {
                    let wire = convert(&prepared, anthropic);
                    let ordinary = convert(&source, anthropic);
                    assert_eq!(wire["tools"], ordinary["tools"]);
                    assert_eq!(wire["system"], ordinary["system"]);
                    assert!(wire.to_string().contains("current task"));
                }
            }
        }
    }

    #[test]
    fn colliding_dynamic_names_round_trip_through_json_and_sse() {
        for kind in ["additional_tools", "tool_search_output"] {
            let mut source = request(vec![
                declaration(kind, "archive", "files__read"),
                user("old task"),
                marker(),
                declaration("additional_tools", "archive__files", "read"),
                user("use the current tool"),
            ]);
            source["tool_choice"] = json!({
                "type": "function", "namespace": "archive__files", "name": "read"
            });
            for anthropic in [false, true] {
                let wire = convert(&source, anthropic);
                assert_eq!(wire["tools"].as_array().unwrap().len(), 3);
                let name = if anthropic {
                    &wire["tool_choice"]["name"]
                } else {
                    &wire["tool_choice"]["function"]["name"]
                };
                let (direct, stream) = if anthropic {
                    let block =
                        json!({"type": "tool_use", "id": "call_new", "name": name, "input": {}});
                    let response = json!({
                        "id": "msg_collision", "model": "claude-test", "role": "assistant",
                        "content": [block.clone()], "stop_reason": "tool_use"
                    });
                    let events = [
                        json!({"type": "message_start", "message": {
                            "id": "msg_collision", "model": "claude-test", "role": "assistant", "content": []
                        }}),
                        json!({"type": "content_block_start", "index": 0, "content_block": block}),
                        json!({"type": "content_block_stop", "index": 0}),
                        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
                        json!({"type": "message_stop"}),
                    ];
                    let sse = events
                        .iter()
                        .map(|event| format!("data: {event}\n\n"))
                        .collect::<String>();
                    (
                        anthropic_message_to_response_with_request(response, &source).unwrap(),
                        anthropic_sse_to_responses_sse_with_request(&sse, &source),
                    )
                } else {
                    let call = json!({
                        "id": "call_new", "type": "function",
                        "function": {"name": name, "arguments": "{}"}
                    });
                    let response = json!({
                        "id": "chat_collision", "model": "claude-test",
                        "choices": [{"message": {"role": "assistant", "tool_calls": [call.clone()]},
                            "finish_reason": "tool_calls"}]
                    });
                    let mut delta_call = call;
                    delta_call["index"] = json!(0);
                    let event = json!({
                        "id": "chat_collision", "model": "claude-test",
                        "choices": [{"delta": {"tool_calls": [delta_call]}, "finish_reason": "tool_calls"}]
                    });
                    let sse = format!("data: {event}\n\ndata: [DONE]\n\n");
                    (
                        chat_completion_to_response_with_request(response, &source).unwrap(),
                        chat_sse_to_responses_sse_with_request(&sse, &source),
                    )
                };
                let events = parse_response_sse_events(&stream);
                let streamed = &events.last().unwrap().data["response"];
                for response in [&direct, streamed] {
                    let call = response["output"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|item| item["type"] == "function_call")
                        .unwrap();
                    assert_eq!(call["namespace"], "archive__files", "{response}");
                    assert_eq!(call["name"], "read", "{response}");
                }
            }
        }
    }

    #[test]
    fn compaction_keeps_normal_wire_history_prefix_for_legacy_and_v2_triggers() {
        for anthropic in [false, true] {
            for kind in [CompactionKind::Legacy, CompactionKind::RemoteV2] {
                for non_trailing in [false, true] {
                    if non_trailing && kind == CompactionKind::Legacy {
                        continue;
                    }
                    let source = request(vec![
                        user("already summarized"),
                        json!({"type": "message", "role": "user", "content": [
                            {"type": "input_file", "file_url": "https://example.invalid/OLD.pdf"}
                        ]}),
                        declaration("additional_tools", "files", "read"),
                        marker(),
                        user("current task"),
                        json!({"type": "message", "role": "assistant", "content": [
                            {"type": "output_text", "text": "current answer"}
                        ]}),
                    ]);
                    let ordinary = convert(&source, anthropic);
                    let mut compact = source;
                    compact["input"].as_array_mut().unwrap().push(match kind {
                        CompactionKind::Legacy => user(COMPACTION_PROMPT_PREFIX),
                        CompactionKind::RemoteV2 => json!({"type": "compaction_trigger"}),
                    });
                    if non_trailing {
                        compact["input"]
                            .as_array_mut()
                            .unwrap()
                            .push(user("post trigger reminder"));
                    }
                    let prepared = prepare_compaction_attempt_request(
                        &compact,
                        kind,
                        CompactionRoute::CacheReuse,
                        &CompactionOptions::default(),
                        CompactionAttempt::First,
                    );
                    let wire = convert(&prepared, anthropic);
                    assert_eq!(wire["tools"], ordinary["tools"]);
                    assert_eq!(wire["system"], ordinary["system"]);
                    let messages = ordinary["messages"].as_array().unwrap();
                    assert_eq!(
                        &wire["messages"].as_array().unwrap()[..messages.len()],
                        messages
                    );
                    assert!(!wire.to_string().contains("OLD.pdf"));
                    assert!(wire.to_string().contains("Handoff checkpoint"));
                }
            }
        }
    }

    #[test]
    fn legacy_context_and_mixed_context_media_are_projected_before_both_protocols() {
        let source = request(vec![
            json!({
                "type": "message", "role": "user",
                "content": [
                    {"type": "input_text", "text": "# AGENTS.md instructions for E:\\project\nPROJECT_RULE"},
                    {"type": "input_image", "image_url": "https://example.invalid/OLD.png"}
                ],
                "internal_chat_message_metadata_passthrough": {
                    "content_item_kinds": ["agents_md.instructions", "user.image"]
                }
            }),
            user("<environment_context><cwd>E:\\project</cwd></environment_context>"),
            user("<INSTRUCTIONS>GLOBAL_RULE</INSTRUCTIONS>"),
            json!({"type": "message", "role": "user", "content": [
                {"type": "input_file", "file_url": "https://example.invalid/OLD.pdf"}
            ]}),
            marker(),
            user("new task"),
        ]);
        for anthropic in [false, true] {
            let wire = convert(&source, anthropic).to_string();
            assert!(wire.contains("PROJECT_RULE"));
            assert!(wire.contains("environment_context"));
            assert!(wire.contains("GLOBAL_RULE"));
            assert!(!wire.contains("OLD.png"));
            assert!(!wire.contains("OLD.pdf"));
        }
    }
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

fn enable_compaction_for_test(settings_dir: &Path) {
    let path = settings_dir.join("settings.json");
    let mut settings: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["layeredCompactionEnabled"] = json!(true);
    std::fs::write(path, serde_json::to_vec(&settings).unwrap()).unwrap();
}

fn write_mixed_relay_settings(settings_dir: &Path, base_url: &str) {
    write_named_mixed_relay_settings_with_system_prompt(settings_dir, base_url, "Mixed", "");
}

fn write_cli_proxy_api_relay_settings(settings_dir: &Path, base_url: &str) {
    write_named_mixed_relay_settings_with_system_prompt(settings_dir, base_url, "CPA", "");
}

fn write_mixed_relay_settings_with_system_prompt(
    settings_dir: &Path,
    base_url: &str,
    system_prompt: &str,
) {
    write_named_mixed_relay_settings_with_system_prompt(
        settings_dir,
        base_url,
        "Mixed",
        system_prompt,
    );
}

fn write_named_mixed_relay_settings_with_system_prompt(
    settings_dir: &Path,
    base_url: &str,
    relay_name: &str,
    system_prompt: &str,
) {
    let settings = json!({
        "relayProfiles": [{
            "id": "mixed",
            "name": relay_name,
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
                },
                {
                    "requestModel": "glm-5.2",
                    "protocol": "anthropic",
                    "contextWindow": "1000000"
                },
                {
                    "requestModel": "deepseek-v4-flash",
                    "protocol": "anthropic",
                    "contextWindow": "1000000"
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

fn write_anthropic_sonnet5_relay_settings(settings_dir: &Path, base_url: &str) {
    let settings = json!({
        "relayProfiles": [{
            "id": "anthropic-sonnet5",
            "name": "Anthropic Sonnet 5",
            "baseUrl": base_url,
            "upstreamBaseUrl": base_url,
            "apiKey": "sk-test",
            "protocol": "anthropic",
            "localProxyEnabled": true,
            "relayMode": "mixedApi",
            "modelMappings": [{
                "requestModel": "claude-sonnet-5",
                "protocol": "anthropic",
                "contextWindow": "200000"
            }]
        }],
        "activeRelayId": "anthropic-sonnet5"
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).unwrap(),
    )
    .unwrap();
}

struct SettingsPathGuard {
    previous: Option<PathBuf>,
    previous_app_state: Option<PathBuf>,
}

fn settings_path_test_lock() -> &'static Mutex<()> {
    // 普通代理请求也会重置进程级轮转状态，显式传入 settings 的测试同样需要此锁。
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

impl SettingsPathGuard {
    fn set(path: PathBuf) -> Self {
        let previous_app_state = codex_elves_core::paths::set_app_state_dir_for_tests(
            path.parent().map(Path::to_path_buf),
        );
        let previous = codex_elves_core::paths::set_settings_path_for_tests(Some(path));
        Self {
            previous,
            previous_app_state,
        }
    }
}

impl Drop for SettingsPathGuard {
    fn drop(&mut self) {
        let _ = codex_elves_core::proxy_log::flush_pending_records();
        codex_elves_core::paths::set_settings_path_for_tests(self.previous.take());
        codex_elves_core::paths::set_app_state_dir_for_tests(self.previous_app_state.take());
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

    fn finish_all(self) -> Vec<ChatRequest> {
        self.handle.join().unwrap()
    }
}

struct ChatRequest {
    path: String,
    user_agent: String,
    x_api_key: String,
    anthropic_version: String,
    x_codex_beta_features: String,
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
    spawn_chat_server_with_status_responses(
        response_bodies
            .into_iter()
            .map(|body| ("200 OK".to_string(), body))
            .collect(),
    )
}

fn spawn_chat_server_with_status_responses(responses: Vec<(String, String)>) -> ChatServer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, response_body) in responses {
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
            let x_codex_beta_features = header_value("x-codex-beta-features").unwrap_or_default();
            let request_body = request
                .split_once("\r\n\r\n")
                .map(|(_, body)| body.to_string())
                .unwrap_or_default();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(ChatRequest {
                path,
                user_agent,
                x_api_key,
                anthropic_version,
                x_codex_beta_features,
                body: request_body,
            });
        }
        requests
    });
    ChatServer { base_url, handle }
}

struct TruncatedResponseServer {
    base_url: String,
    handle: thread::JoinHandle<ChatRequest>,
}

impl TruncatedResponseServer {
    fn finish(self) -> ChatRequest {
        self.handle.join().unwrap()
    }
}

fn spawn_truncated_response_server() -> TruncatedResponseServer {
    spawn_raw_response_server("200 OK", "{\"partial\":true}", 128)
}

fn spawn_truncated_response_server_with_status(status: &str) -> TruncatedResponseServer {
    spawn_raw_response_server(status, "{\"partial\":true}", 128)
}

fn spawn_status_response_server(status: &str, response_body: &str) -> TruncatedResponseServer {
    spawn_raw_response_server(status, response_body, response_body.len())
}

fn spawn_raw_response_server(
    status: &str,
    response_body: &str,
    declared_content_length: usize,
) -> TruncatedResponseServer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    let status = status.to_string();
    let response_body = response_body.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request_bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let bytes = stream.read(&mut buffer).unwrap();
            assert!(bytes > 0, "test upstream request ended before its body");
            request_bytes.extend_from_slice(&buffer[..bytes]);
            let request = String::from_utf8_lossy(&request_bytes);
            let Some(header_end) = request.find("\r\n\r\n") else {
                continue;
            };
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
        let request_body = request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {declared_content_length}\r\nConnection: close\r\n\r\n{response_body}"
        );
        stream.write_all(response.as_bytes()).unwrap();
        ChatRequest {
            path,
            user_agent: header_value("user-agent").unwrap_or_default(),
            x_api_key: header_value("x-api-key").unwrap_or_default(),
            anthropic_version: header_value("anthropic-version").unwrap_or_default(),
            x_codex_beta_features: header_value("x-codex-beta-features").unwrap_or_default(),
            body: request_body,
        }
    });
    TruncatedResponseServer { base_url, handle }
}

fn legacy_compaction_request() -> serde_json::Value {
    json!({
        "model": "claude-opus-4-5",
        "reasoning": { "effort": "max" },
        "model_reasoning_effort": "high",
        "reasoning_effort": "low",
        "input": [
            { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
            { "type": "reasoning", "encrypted_content": "opaque-original-model-blob" },
            {
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Summarize."
                }]
            }
        ]
    })
}

#[tokio::test]
async fn native_remote_compaction_keeps_session_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_status_responses(vec![(
        "200 OK".to_string(),
        r#"{"id":"resp-remote","status":"completed","model":"gpt-5.4","output":[]}"#.to_string(),
    )]);
    let settings = BackendSettings {
        layered_compaction_enabled: true,

        relay_profiles: vec![RelayProfile {
            id: "remote-compaction-original-model".to_string(),
            name: "Remote Compaction Original Model".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "gpt-5.4".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "1000000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "remote-compaction-original-model".to_string(),
        ..BackendSettings::default()
    };
    let request = json!({
        "model": "gpt-5.4",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "small context" }]
            },
            { "type": "compaction_trigger" }
        ]
    });

    let response = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    drop(response);

    let requests = server.finish_all();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path.ends_with("/responses"));
    let upstream: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(upstream["model"], "gpt-5.4");
    assert_eq!(upstream["input"][1]["type"], "compaction_trigger");
}

#[tokio::test]
async fn disabled_compaction_preserves_harness_legacy_requests_and_untagged_summary() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let _guard = SettingsPathGuard::set(path.clone());
    for (model, protocol) in [
        ("gpt-test", RelayProtocol::Responses),
        ("deepseek-test", RelayProtocol::ChatCompletions),
        ("claude-test", RelayProtocol::Anthropic),
    ] {
        let reply = match protocol {
            RelayProtocol::Responses => json!({
                "id":"resp_harness","status":"completed","model":model,
                "output":[{"type":"message","role":"assistant","content":[
                    {"type":"output_text","text":"HARNESS SUMMARY"}
                ]}]
            }),
            RelayProtocol::ChatCompletions => json!({
                "id":"chatcmpl_harness","model":model,"choices":[{
                    "message":{"role":"assistant","content":"HARNESS SUMMARY"},"finish_reason":"stop"
                }]
            }),
            RelayProtocol::Anthropic => json!({
                "id":"msg_harness","type":"message","role":"assistant","model":model,
                "content":[{"type":"text","text":"HARNESS SUMMARY"}],"stop_reason":"end_turn",
                "usage":{"input_tokens":10,"output_tokens":5}
            }),
        };
        let server = spawn_chat_server_with_response(reply.to_string());
        let settings = BackendSettings {
            layered_compaction_enabled: false,
            layered_compaction_retain_recent_round_enabled: true,
            layered_compaction_prompt_override: "MUST NOT APPLY".to_string(),

            relay_profiles: vec![RelayProfile {
                id: "harness".to_string(),
                base_url: server.base_url.clone(),
                upstream_base_url: server.base_url.clone(),
                relay_mode: RelayMode::MixedApi,
                api_key: "sk-test".to_string(),
                local_proxy_enabled: Some(true),
                model_mappings: vec![RelayModelMapping {
                    request_model: model.to_string(),
                    alias: String::new(),
                    protocol,
                    context_window: "200000".to_string(),
                    system_prompt_override: String::new(),
                }],
                ..Default::default()
            }],
            active_relay_id: "harness".to_string(),
            ..Default::default()
        };
        let mut saved_settings = serde_json::to_value(&settings).unwrap();
        // API key 不参与 BackendSettings 的普通序列化，测试配置显式写入占位值。
        saved_settings["relayProfiles"][0]["apiKey"] = json!("sk-test");
        std::fs::write(&path, serde_json::to_vec(&saved_settings).unwrap()).unwrap();
        let instruction = format!(
            "{}. Keep the harness format.",
            codex_elves_core::layered_compaction::COMPACTION_PROMPT_PREFIX
        );
        let request = json!({
            "model":model,"stream":false,"instructions":"HARNESS SYSTEM",
            "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}],
            "tool_choice":"auto",
            "input":[
                {"type":"message","role":"user","content":"original history"},
                {"type":"message","role":"user","content":instruction}
            ]
        });
        let result = handle_responses_proxy_request(&request.to_string())
            .await
            .unwrap();
        let response: Value = serde_json::from_slice(&result.body).unwrap();
        assert_eq!(response["status"], "completed");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "HARNESS SUMMARY"
        );
        assert!(!String::from_utf8_lossy(&result.body).contains("codex-elves-compaction-"));
        let captured = server.finish();
        let wire: Value = serde_json::from_str(&captured.body).unwrap();
        let expected = match protocol {
            RelayProtocol::Responses => request.clone(),
            RelayProtocol::ChatCompletions => responses_to_chat_completions(request).unwrap(),
            RelayProtocol::Anthropic => responses_to_anthropic_messages(request).unwrap(),
        };
        for field in [
            "model",
            "instructions",
            "system",
            "input",
            "messages",
            "tools",
            "tool_choice",
        ] {
            assert_eq!(wire.get(field), expected.get(field), "{model}: {field}");
        }
        assert!(!captured.body.contains("MUST NOT APPLY"));
        assert!(!captured.body.contains("[Handoff checkpoint]"));
    }
}

#[tokio::test]
async fn disabled_compaction_passes_responses_v2_through_and_does_not_emulate_other_protocols() {
    let _lock = settings_path_test_lock().lock().unwrap();
    for stream in [false, true] {
        let request = json!({
            "model":"claude-test","stream":stream,
            "instructions":"HARNESS SYSTEM",
            "input":[{"role":"user","content":"original history"},{"type":"compaction_trigger"}]
        });
        let response = json!({
            "id":"resp_native","status":"completed","output":[
                {"type":"compaction","encrypted_content":"upstream-opaque-payload"}
            ]
        });
        let body = if stream {
            format!(
                "event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","response":response})
            )
        } else {
            response.to_string()
        };
        let server = spawn_chat_server_with_response(body.clone());
        let settings = BackendSettings {
            layered_compaction_enabled: false,
            layered_compaction_retain_recent_round_enabled: true,
            layered_compaction_prompt_override: "MUST NOT APPLY".to_string(),
            relay_profiles: vec![RelayProfile {
                id: "harness".to_string(),
                base_url: server.base_url.clone(),
                api_key: "sk-test".to_string(),
                model_mappings: vec![RelayModelMapping {
                    request_model: "claude-test".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "200000".to_string(),
                    system_prompt_override: String::new(),
                }],
                ..Default::default()
            }],
            active_relay_id: "harness".to_string(),
            ..Default::default()
        };
        let result =
            open_responses_proxy_request_with_settings(&request.to_string(), settings.clone())
                .await
                .unwrap();
        assert_eq!(
            result.response_protocol,
            UpstreamResponseProtocol::Responses
        );
        assert_eq!(result.status_code, 200);
        assert_eq!(result.into_body_bytes().await.unwrap(), body.as_bytes());
        let captured: Value = serde_json::from_str(&server.finish().body).unwrap();
        assert_eq!(captured["input"], request["input"]);
        assert_eq!(captured["instructions"], request["instructions"]);
        for protocol in [RelayProtocol::ChatCompletions, RelayProtocol::Anthropic] {
            let mut settings = settings.clone();
            settings.relay_profiles[0].model_mappings[0].protocol = protocol;
            // 服务器已经关闭；不支持的协议应直接返回错误，不能偷偷发起摘要请求。
            let result = open_responses_proxy_request_with_settings(&request.to_string(), settings)
                .await
                .unwrap();
            assert_eq!(result.status_code, 400);
            let response: Value =
                serde_json::from_slice(&result.into_body_bytes().await.unwrap()).unwrap();
            assert_eq!(response["error"]["code"], "unsupported_compaction");
            assert!(response.get("output").is_none());
        }
    }
}

#[tokio::test]
async fn compaction_contract_http_retries_same_model_and_only_returns_validated_summary() {
    let _lock = settings_path_test_lock().lock().unwrap();
    use codex_elves_core::layered_compaction::{
        COMPACTION_RETRY_INSTRUCTION, compaction_instruction,
    };
    for retain in [false, true] {
        for succeeds_on_retry in [false, true] {
            let reply = |text: &str| {
                json!({
                    "id":"msg-handoff","type":"message","role":"assistant",
                    "model":"claude-opus-5-5","stop_reason":"end_turn",
                    "content":[{"type":"text","text":text}],
                    "usage":{"input_tokens":100,"cache_read_input_tokens":900,"output_tokens":20}
                })
                .to_string()
            };
            let server = spawn_chat_server_with_status_responses(vec![
                ("200 OK".to_string(), reply("我先把相关文件找出来。")),
                (
                    "200 OK".to_string(),
                    reply(if succeeds_on_retry {
                        "<analysis>notes</analysis><summary>已完成检查，下一步只需复核。</summary>"
                    } else {
                        "<analysis>only notes</analysis>"
                    }),
                ),
            ]);
            let settings = BackendSettings {
                layered_compaction_enabled: true,
                layered_compaction_retain_recent_round_enabled: retain,
                layered_compaction_prompt_override: "CUSTOM HANDOFF".to_string(),

                relay_profiles: vec![RelayProfile {
                    id: "handoff".to_string(),
                    name: "handoff".to_string(),
                    base_url: server.base_url.clone(),
                    api_key: "sk-test".to_string(),
                    model_mappings: vec![RelayModelMapping {
                        system_prompt_override: String::new(),
                        request_model: "claude-opus-5-5".to_string(),
                        alias: String::new(),
                        protocol: RelayProtocol::Anthropic,
                        context_window: "1000000".to_string(),
                    }],
                    ..Default::default()
                }],
                active_relay_id: "handoff".to_string(),
                ..Default::default()
            };
            let request = json!({
                "model":"claude-opus-5-5","stream":false,
                "instructions":"MAIN SYSTEM","reasoning":{"effort":"max"},
                "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}],
                "tool_choice":"auto","parallel_tool_calls":true,
                "input":[
                    {"type":"message","role":"user","content":"earlier work"},
                    {"type":"reasoning","summary":[{"type":"summary_text","text":"prior reasoning"}]},
                    {"type":"message","role":"assistant","content":"checks completed"},
                    {"type":"message","role":"user","content":"continue"},
                    {"type":"compaction_trigger"}
                ]
            });
            let response =
                open_responses_proxy_request_with_settings(&request.to_string(), settings)
                    .await
                    .unwrap();
            let body: Value =
                serde_json::from_slice(&response.into_body_bytes().await.unwrap()).unwrap();
            assert_eq!(
                body["status"],
                if succeeds_on_retry {
                    "completed"
                } else {
                    "failed"
                }
            );
            if succeeds_on_retry {
                let payload = body["output"][0]["encrypted_content"].as_str().unwrap();
                assert!(payload.starts_with(if retain {
                    "codex-elves-compaction-v3:"
                } else {
                    "codex-elves-compaction-v2:"
                }));
                assert!(payload.contains("已完成检查"));
                assert!(!payload.contains("<summary>"));
                assert!(!payload.contains("notes"));
            } else {
                assert_eq!(body["output"], json!([]));
            }
            let requests = server.finish_all();
            assert_eq!(requests.len(), 2);
            let first: Value = serde_json::from_str(&requests[0].body).unwrap();
            let retry: Value = serde_json::from_str(&requests[1].body).unwrap();
            let mut main = request.clone();
            main["input"].as_array_mut().unwrap().pop();
            let main = responses_to_anthropic_messages(main).unwrap();
            for field in [
                "system",
                "tools",
                "tool_choice",
                "parallel_tool_calls",
                "thinking",
                "output_config",
                "model",
            ] {
                assert_eq!(
                    first.get(field),
                    main.get(field),
                    "{field}; retain={retain}"
                );
            }
            assert_eq!(retry["model"], first["model"]);
            for field in [
                "system",
                "tools",
                "tool_choice",
                "thinking",
                "output_config",
            ] {
                assert_eq!(retry.get(field), first.get(field), "{field}");
            }
            let first_messages = first["messages"].as_array().unwrap();
            let retry_messages = retry["messages"].as_array().unwrap();
            assert_eq!(
                &retry_messages[..first_messages.len() - 1],
                &first_messages[..first_messages.len() - 1]
            );
            let first_content = first_messages.last().unwrap()["content"]
                .as_array()
                .unwrap();
            let retry_content = retry_messages.last().unwrap()["content"]
                .as_array()
                .unwrap();
            assert_eq!(&retry_content[..first_content.len()], first_content);
            assert_eq!(
                retry_content.last().unwrap()["text"],
                COMPACTION_RETRY_INSTRUCTION
            );
            assert!(first["messages"].to_string().contains(
                &serde_json::to_string(&compaction_instruction("CUSTOM HANDOFF")).unwrap()
            ));
        }
    }
}

#[tokio::test]
async fn bridged_claude_remote_compaction_keeps_session_model_and_tools() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_status_responses(vec![(
        "200 OK".to_string(),
        r#"{"id":"msg-bridge","type":"message","role":"assistant","model":"claude-opus-4-8","stop_reason":"end_turn","content":[{"type":"text","text":"<summary>BRIDGED SUMMARY</summary>"}]}"#.to_string(),
    )]);
    let settings = BackendSettings {
        layered_compaction_enabled: true,

        relay_profiles: vec![RelayProfile {
            id: "bridged-compaction-main-model".to_string(),
            name: "Bridged Compaction Main Model".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "claude-opus-4-8".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Anthropic,
                context_window: "1000000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "bridged-compaction-main-model".to_string(),
        ..BackendSettings::default()
    };
    let request = json!({
        "model": "claude-opus-4-8",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "small context" }]
            },
            { "type": "compaction_trigger" }
        ],
        "tools": [{ "type": "function", "name": "exec_command" }],
        "tool_choice": "auto"
    });

    let response = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    drop(response);

    let requests = server.finish_all();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path.ends_with("/messages"));
    let upstream: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(upstream["model"], "claude-opus-4-8");
    assert!(upstream.get("tools").is_some());
    assert_eq!(upstream["tool_choice"]["type"], "auto");
}

#[tokio::test]
async fn bridged_claude_remote_compaction_preserves_reasoning_effort() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_status_responses(vec![(
        "200 OK".to_string(),
        r#"{"id":"msg-bridge","type":"message","role":"assistant","model":"claude-opus-4-8","stop_reason":"end_turn","content":[{"type":"text","text":"<summary>BRIDGED CLAUDE SUMMARY</summary>"}],"usage":{"input_tokens":10,"output_tokens":5}}"#.to_string(),
    )]);
    let settings = BackendSettings {
        layered_compaction_enabled: true,

        relay_profiles: vec![RelayProfile {
            id: "bridged-claude-compaction-main-model".to_string(),
            name: "Bridged Claude Compaction Main Model".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "claude-opus-4-8".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Anthropic,
                context_window: "1000000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "bridged-claude-compaction-main-model".to_string(),
        ..BackendSettings::default()
    };
    let request = json!({
        "model": "claude-opus-4-8",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "small context" }]
            },
            { "type": "compaction_trigger" }
        ]
    });

    let response = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    drop(response);

    let requests = server.finish_all();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path.ends_with("/messages"));
    let upstream: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert_eq!(upstream["model"], "claude-opus-4-8");
    assert_eq!(upstream["output_config"], json!({ "effort": "high" }));
    assert!(!requests[0].body.contains("compaction_trigger"));
    assert!(
        upstream["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
    );
}

#[tokio::test]
async fn failed_compaction_retries_once_with_same_session_model() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let server = spawn_chat_server_with_status_responses(vec![
        (
            "404 Not Found".to_string(),
            r#"{"error":{"code":"model_not_found","message":"model unavailable"}}"#.to_string(),
        ),
        (
            "200 OK".to_string(),
            r#"{"id":"resp-original","status":"completed","model":"gpt-5.4","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"<summary>summary</summary>"}]}]}"#
                .to_string(),
        ),
    ]);
    let settings = BackendSettings {
        layered_compaction_enabled: true,

        relay_profiles: vec![RelayProfile {
            id: "compaction-retry".to_string(),
            name: "Compaction Retry".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            model_mappings: vec![RelayModelMapping {
                system_prompt_override: String::new(),
                request_model: "claude-opus-4-8".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Anthropic,
                context_window: "1000000".to_string(),
            }],
            ..RelayProfile::default()
        }],
        active_relay_id: "compaction-retry".to_string(),
        ..BackendSettings::default()
    };
    let request = json!({
        "model": "claude-opus-4-8",
        "stream": false,
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "small context" }]
            },
            {
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "You are performing a CONTEXT CHECKPOINT COMPACTION. Summarize."
                }]
            }
        ]
    });

    let response = open_responses_proxy_request_with_settings(&request.to_string(), settings)
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);

    let requests = server.finish_all();
    assert_eq!(requests.len(), 2);
    let first: Value = serde_json::from_str(&requests[0].body).unwrap();
    let second: Value = serde_json::from_str(&requests[1].body).unwrap();
    assert!(requests[0].path.ends_with("/messages"));
    assert!(requests[1].path.ends_with("/messages"));
    assert_eq!(first["model"], "claude-opus-4-8");
    assert_eq!(first["output_config"]["effort"], "high");
    assert_eq!(second["model"], "claude-opus-4-8");
}

#[tokio::test]
async fn compaction_preserves_large_history_across_upstream_rejection_retry() {
    let _lock = settings_path_test_lock().lock().unwrap();
    let summary = |model: &str| {
        json!({
            "id": "msg-capacity",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{"type": "text", "text": "<summary>summary</summary>"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 600, "output_tokens": 10}
        })
        .to_string()
    };
    for status in ["200 OK", "400 Bad Request", "413 Payload Too Large"] {
        let rejected = status != "200 OK";
        let mut responses = vec![(
            status.to_string(),
            if rejected {
                r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: context length exceeded"}}"#.to_string()
            } else {
                summary("claude-opus-4-8")
            },
        )];
        if rejected {
            responses.push(("200 OK".to_string(), summary("claude-opus-4-8")));
        }
        let server = spawn_chat_server_with_status_responses(responses);
        let settings = BackendSettings {
            layered_compaction_enabled: true,
            layered_compaction_retain_recent_round_enabled: false,

            relay_profiles: vec![RelayProfile {
                id: "compaction-capacity".to_string(),
                base_url: server.base_url.clone(),
                upstream_base_url: server.base_url.clone(),
                api_key: "sk-test".to_string(),
                model_mappings: vec![RelayModelMapping {
                    system_prompt_override: String::new(),
                    request_model: "claude-opus-4-8".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Anthropic,
                    context_window: "1000000".to_string(),
                }],
                ..RelayProfile::default()
            }],
            active_relay_id: "compaction-capacity".to_string(),
            ..BackendSettings::default()
        };
        let history = "中文历史与代码 function_call(arguments);\n".repeat(600);
        let mut request = legacy_compaction_request();
        request["model"] = json!("claude-opus-4-8");
        request["stream"] = json!(false);
        request["input"][0] = json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": history}]
        });
        let response = open_responses_proxy_request_with_settings(&request.to_string(), settings)
            .await
            .unwrap();
        assert_eq!(response.status_code, 200);
        drop(response);

        let requests = server.finish_all();
        assert_eq!(requests.len(), if rejected { 2 } else { 1 });
        for captured in &requests {
            assert!(captured.path.ends_with("/messages"));
            let body: Value = serde_json::from_str(&captured.body).unwrap();
            assert_eq!(body["model"], "claude-opus-4-8");
            assert!(
                body["messages"]
                    .to_string()
                    .contains(&serde_json::to_string(&history).unwrap()),
                "首次请求和同模型重试均应保留完整历史"
            );
        }
    }
}
