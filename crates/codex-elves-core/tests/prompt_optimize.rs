use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use codex_elves_core::prompt_optimize::PromptOptimizeService;
use codex_elves_core::settings::{
    BackendSettings, RelayMode, RelayModelMapping, RelayProfile, RelayProtocol,
};
use serde_json::{Value, json};

struct TestServer {
    base_url: String,
    handle: thread::JoinHandle<TestRequest>,
}

impl TestServer {
    fn finish(self) -> TestRequest {
        self.handle.join().unwrap()
    }
}

struct TestRequest {
    path: String,
    body: Value,
}

fn spawn_server(response_body: impl Into<String>, response_delay: Duration) -> TestServer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}/v1");
    listener.set_nonblocking(true).unwrap();
    let response_body = response_body.into();
    let handle = thread::spawn(move || {
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "test upstream did not receive a request"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("failed to accept test request: {error}"),
            }
        };
        stream.set_nonblocking(true).unwrap();
        let mut request_bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => thread::sleep(Duration::from_millis(5)),
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
                            .unwrap_or_default();
                        if request_bytes.len() >= header_end + 4 + content_length {
                            break;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
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
        let body = request
            .split_once("\r\n\r\n")
            .and_then(|(_, body)| serde_json::from_str(body).ok())
            .unwrap_or(Value::Null);

        thread::sleep(response_delay);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes());
        TestRequest { path, body }
    });
    TestServer { base_url, handle }
}

fn settings_for(
    server: &TestServer,
    model: &str,
    protocol: RelayProtocol,
    system_prompt_override: &str,
) -> BackendSettings {
    BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "active".to_string(),
            name: "Active supplier".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            local_proxy_enabled: Some(true),
            relay_mode: RelayMode::MixedApi,
            model_mappings: vec![RelayModelMapping {
                request_model: model.to_string(),
                alias: String::new(),
                protocol,
                context_window: "200000".to_string(),
            }],
            system_prompt_override: system_prompt_override.to_string(),
            ..RelayProfile::default()
        }],
        active_relay_id: "active".to_string(),
        ..BackendSettings::default()
    }
}

fn payload(request_id: &str, model: &str) -> Value {
    json!({
        "requestId": request_id,
        "model": model,
        "reasoningEffort": "medium",
        "systemPrompt": "Improve the draft and output only the result.",
        "input": "draft",
    })
}

#[tokio::test]
async fn responses_optimization_uses_active_supplier_and_ignores_its_prompt_override() {
    let server = spawn_server(
        json!({
            "id": "resp-test",
            "object": "response",
            "status": "completed",
            "model": "gpt-responses",
            "output": [{
                "id": "msg-test",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": "```markdown\noptimized responses\n```",
                    "annotations": []
                }]
            }],
            "usage": {
                "input_tokens": 2300,
                "output_tokens": 50
            }
        })
        .to_string(),
        Duration::ZERO,
    );
    let settings = settings_for(
        &server,
        "gpt-responses",
        RelayProtocol::Responses,
        "SUPPLIER OVERRIDE MUST NOT WIN",
    );
    let service = PromptOptimizeService::with_timeout(Duration::from_secs(2));

    let result = service
        .optimize(settings, payload("responses-1", "gpt-responses"))
        .await
        .unwrap();
    let request = server.finish();

    assert_eq!(result["status"], "ok");
    assert_eq!(result["text"], "optimized responses");
    assert_eq!(result["protocol"], "responses");
    assert_eq!(result["totalTokens"], 2350);
    assert_eq!(request.path, "/v1/responses");
    assert_eq!(
        request.body["instructions"],
        "Improve the draft and output only the result."
    );
    assert_ne!(
        request.body["instructions"],
        "SUPPLIER OVERRIDE MUST NOT WIN"
    );
}

#[tokio::test]
async fn responses_optimization_sends_recent_context_with_reference_markers() {
    let server = spawn_server(
        json!({
            "id": "resp-context",
            "object": "response",
            "status": "completed",
            "model": "gpt-responses",
            "output": [{
                "id": "msg-context",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": "optimized with context",
                    "annotations": []
                }]
            }]
        })
        .to_string(),
        Duration::ZERO,
    );
    let settings = settings_for(&server, "gpt-responses", RelayProtocol::Responses, "");
    let service = PromptOptimizeService::with_timeout(Duration::from_secs(2));
    let mut request_payload = payload("responses-context", "gpt-responses");
    request_payload["recentContext"] = json!({
        "user": "previous user request",
        "assistant": "previous assistant response",
    });

    let result = service.optimize(settings, request_payload).await.unwrap();
    let request = server.finish();
    let instructions = request.body["instructions"].as_str().unwrap();
    let input = request.body["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap();

    assert_eq!(result["text"], "optimized with context");
    assert!(instructions.contains("当前执行过程中的最近一轮上下文"));
    assert!(instructions.contains("提示词相关背景信息"));
    assert!(instructions.contains("未经你的独立验证"));
    assert!(instructions.contains("保留“根据上一轮上下文”等来源"));
    assert!(instructions.contains("保留不确定性"));
    assert!(input.contains("【当前执行过程中的最近一轮上下文】"));
    assert!(input.contains("【用途：提示词相关背景信息】"));
    assert!(input.contains("可能包含未经独立验证的事实、判断或结论"));
    assert!(input.contains("用户：\nprevious user request"));
    assert!(input.contains("助手：\nprevious assistant response"));
    assert!(input.ends_with("【本次待优化提示词】\ndraft"));
}

#[tokio::test]
async fn chat_completions_optimization_is_converted_back_to_responses_text() {
    let server = spawn_server(
        json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "gpt-chat",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "optimized chat"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
        })
        .to_string(),
        Duration::ZERO,
    );
    let settings = settings_for(&server, "gpt-chat", RelayProtocol::ChatCompletions, "");
    let service = PromptOptimizeService::with_timeout(Duration::from_secs(2));

    let result = service
        .optimize(settings, payload("chat-1", "gpt-chat"))
        .await
        .unwrap();
    let request = server.finish();

    assert_eq!(result["text"], "optimized chat");
    assert_eq!(result["protocol"], "chatCompletions");
    assert_eq!(result["totalTokens"], 3);
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(request.body["messages"][0]["role"], "system");
    assert_eq!(
        request.body["messages"][0]["content"],
        "Improve the draft and output only the result."
    );
}

#[tokio::test]
async fn anthropic_optimization_is_converted_back_to_responses_text() {
    let server = spawn_server(
        json!({
            "id": "msg-test",
            "type": "message",
            "role": "assistant",
            "model": "claude-test",
            "content": [{"type": "text", "text": "optimized anthropic"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        })
        .to_string(),
        Duration::ZERO,
    );
    let settings = settings_for(&server, "claude-test", RelayProtocol::Anthropic, "");
    let service = PromptOptimizeService::with_timeout(Duration::from_secs(2));

    let result = service
        .optimize(settings, payload("anthropic-1", "claude-test"))
        .await
        .unwrap();
    let request = server.finish();

    assert_eq!(result["text"], "optimized anthropic");
    assert_eq!(result["protocol"], "anthropic");
    assert_eq!(result["totalTokens"], 3);
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(
        request.body["system"],
        "Improve the draft and output only the result."
    );
}

#[tokio::test]
async fn optimization_infers_responses_for_unassigned_gpt_model() {
    let server = spawn_server(
        json!({
            "id": "resp-inferred",
            "object": "response",
            "status": "completed",
            "model": "gpt-5.3-codex-spark",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "optimized inferred"}]
            }]
        })
        .to_string(),
        Duration::ZERO,
    );
    let settings = BackendSettings {
        relay_profiles: vec![RelayProfile {
            id: "active".to_string(),
            name: "Active supplier".to_string(),
            base_url: server.base_url.clone(),
            upstream_base_url: server.base_url.clone(),
            api_key: "sk-test".to_string(),
            local_proxy_enabled: Some(true),
            relay_mode: RelayMode::MixedApi,
            ..RelayProfile::default()
        }],
        active_relay_id: "active".to_string(),
        ..BackendSettings::default()
    };
    let service = PromptOptimizeService::with_timeout(Duration::from_secs(2));

    let result = service
        .optimize(
            settings,
            payload("inferred-protocol", "gpt-5.3-codex-spark"),
        )
        .await
        .unwrap();
    let request = server.finish();

    assert_eq!(result["text"], "optimized inferred");
    assert_eq!(result["protocol"], "responses");
    assert_eq!(request.path, "/v1/responses");
    assert_eq!(request.body["model"], "gpt-5.3-codex-spark");
}

#[tokio::test]
async fn cancellation_aborts_an_active_optimization_without_returning_text() {
    let server = spawn_server(
        json!({
            "id": "resp-slow",
            "object": "response",
            "status": "completed",
            "model": "gpt-responses",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "too late"}]
            }]
        })
        .to_string(),
        Duration::from_millis(300),
    );
    let settings = settings_for(&server, "gpt-responses", RelayProtocol::Responses, "");
    let service = Arc::new(PromptOptimizeService::with_timeout(Duration::from_secs(2)));
    let optimize_task = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .optimize(settings, payload("cancel-1", "gpt-responses"))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(40)).await;

    let cancelled = service.cancel(&json!({"requestId": "cancel-1"})).unwrap();
    let result = optimize_task.await.unwrap().unwrap();
    let _ = server.finish();

    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(result["status"], "cancelled");
    assert!(result.get("text").is_none());
}

#[tokio::test]
async fn timeout_preserves_the_original_input_contract() {
    let server = spawn_server(
        json!({
            "id": "resp-timeout",
            "object": "response",
            "status": "completed",
            "model": "gpt-responses",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "too late"}]
            }]
        })
        .to_string(),
        Duration::from_millis(200),
    );
    let settings = settings_for(&server, "gpt-responses", RelayProtocol::Responses, "");
    let service = PromptOptimizeService::with_timeout(Duration::from_millis(30));

    let error = service
        .optimize(settings, payload("timeout-1", "gpt-responses"))
        .await
        .unwrap_err();
    let _ = server.finish();

    assert_eq!(error.to_string(), "优化超时，原输入未改动");
}
