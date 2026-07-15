use codex_elves_core::proxy_log::{ProxyRequestState, ProxyRequestTransport};
use codex_elves_core::responses_websocket::{
    normalize_responses_websocket_capability, probe_active_relay_responses_websocket_if_needed,
    probe_responses_websocket, relay_prefers_native_responses_websocket,
    relay_supports_native_responses_websocket, relay_websocket_enabled_for_settings,
    responses_websocket_url,
};
use codex_elves_core::responses_websocket_bridge::handle_responses_websocket_connection;
use codex_elves_core::settings::{
    BackendSettings, RelayMode, RelayModelMapping, RelayProfile, RelayProtocol,
    ResponsesWebsocketCapabilityState, SettingsStore,
};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};

fn native_responses_profile() -> RelayProfile {
    RelayProfile {
        relay_mode: RelayMode::PureApi,
        base_url: "https://relay.example.test/v1".to_string(),
        protocol: RelayProtocol::Responses,
        ..RelayProfile::default()
    }
}

#[test]
fn capability_cache_defaults_and_serializes_with_camel_case_fields() {
    let profile: RelayProfile = serde_json::from_str(
        r#"{"id":"relay-a","name":"供应商 A","baseUrl":"https://relay.example.test/v1"}"#,
    )
    .unwrap();

    assert_eq!(
        profile.responses_websocket.state,
        ResponsesWebsocketCapabilityState::Unknown
    );
    assert!(profile.responses_websocket.endpoint.is_empty());
    assert_eq!(profile.responses_websocket.checked_at_ms, None);
    assert!(profile.responses_websocket.message.is_empty());
    assert_eq!(profile.responses_websocket_enabled, None);

    let serialized = serde_json::to_value(profile).unwrap();
    assert_eq!(
        serialized["responsesWebsocket"]["state"],
        serde_json::json!("unknown")
    );
    assert_eq!(
        serialized["responsesWebsocket"]["checkedAtMs"],
        serde_json::Value::Null
    );
    assert!(serialized.get("responsesWebsocketEnabled").is_none());
}

#[test]
fn normalizes_http_and_https_base_urls_to_responses_websocket_endpoints() {
    assert_eq!(
        responses_websocket_url("https://relay.example.test"),
        Some("wss://relay.example.test/v1/responses".to_string())
    );
    assert_eq!(
        responses_websocket_url(" http://localhost:8787/v1/ "),
        Some("ws://localhost:8787/v1/responses".to_string())
    );
    assert_eq!(
        responses_websocket_url("https://relay.example.test/openai#"),
        Some("wss://relay.example.test/openai/responses".to_string())
    );
    assert_eq!(responses_websocket_url("ftp://relay.example.test"), None);
    assert_eq!(responses_websocket_url("not a url"), None);
}

#[test]
fn base_url_change_resets_cached_capability_to_unknown() {
    let mut profile = native_responses_profile();
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    profile.responses_websocket.checked_at_ms = Some(1_720_000_000_000);
    profile.responses_websocket.message = "握手成功".to_string();

    assert!(relay_supports_native_responses_websocket(&profile));

    profile.base_url = "https://next.example.test/api".to_string();
    normalize_responses_websocket_capability(&mut profile);

    assert_eq!(
        profile.responses_websocket.state,
        ResponsesWebsocketCapabilityState::Unknown
    );
    assert_eq!(
        profile.responses_websocket.endpoint,
        "wss://next.example.test/api/responses"
    );
    assert_eq!(profile.responses_websocket.checked_at_ms, None);
    assert!(profile.responses_websocket.message.is_empty());
    assert!(!relay_supports_native_responses_websocket(&profile));
}

#[test]
fn native_responses_websocket_supports_mixed_profiles_with_responses_models() {
    let mut profile = native_responses_profile();
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;

    assert!(relay_supports_native_responses_websocket(&profile));

    profile.protocol = RelayProtocol::ChatCompletions;
    profile.model_mappings = vec![
        RelayModelMapping {
            request_model: "gpt-responses".to_string(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        },
        RelayModelMapping {
            request_model: "claude-sonnet".to_string(),
            protocol: RelayProtocol::Anthropic,
            context_window: String::new(),
        },
    ];
    assert!(relay_supports_native_responses_websocket(&profile));

    profile.model_mappings = vec![RelayModelMapping {
        request_model: "chat-model".to_string(),
        protocol: RelayProtocol::ChatCompletions,
        context_window: String::new(),
    }];
    assert!(!relay_supports_native_responses_websocket(&profile));

    profile.system_prompt_override = "使用自定义系统提示词".to_string();
    assert!(!relay_supports_native_responses_websocket(&profile));
    profile.system_prompt_override.clear();
    profile.model_mappings = vec![
        RelayModelMapping {
            request_model: "gpt-responses".to_string(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        },
        RelayModelMapping {
            request_model: "claude-sonnet".to_string(),
            protocol: RelayProtocol::Anthropic,
            context_window: String::new(),
        },
    ];

    profile.relay_mode = RelayMode::Aggregate;
    assert!(!relay_supports_native_responses_websocket(&profile));
    profile.relay_mode = RelayMode::Official;
    assert!(!relay_supports_native_responses_websocket(&profile));
    profile.official_mix_api_key = true;
    assert!(relay_supports_native_responses_websocket(&profile));

    profile.responses_websocket.endpoint = "wss://other.example.test/v1/responses".to_string();
    assert!(!relay_supports_native_responses_websocket(&profile));
}

#[test]
fn reasoning_continuation_keeps_cached_websocket_support_enabled() {
    let mut profile = native_responses_profile();
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    let enabled = BackendSettings::default();
    let disabled = BackendSettings {
        gpt_reasoning_continuation: true,
        ..BackendSettings::default()
    };

    assert!(relay_websocket_enabled_for_settings(&enabled, &profile));
    assert!(relay_websocket_enabled_for_settings(&disabled, &profile));
    assert_eq!(
        profile.responses_websocket.state,
        ResponsesWebsocketCapabilityState::Supported
    );
}

#[test]
fn explicit_websocket_preference_disables_usage_without_clearing_capability() {
    let mut profile = native_responses_profile();
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;

    assert!(relay_supports_native_responses_websocket(&profile));
    assert!(relay_prefers_native_responses_websocket(&profile));

    profile.responses_websocket_enabled = Some(false);

    assert!(relay_supports_native_responses_websocket(&profile));
    assert!(!relay_prefers_native_responses_websocket(&profile));
    assert!(!relay_websocket_enabled_for_settings(
        &BackendSettings::default(),
        &profile
    ));
}

#[tokio::test]
async fn probe_uses_real_websocket_handshake_with_bearer_and_configured_user_agent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured_headers = Arc::new(Mutex::new(None));
    let server_headers = Arc::clone(&captured_headers);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let socket = accept_hdr_async(stream, move |request: &Request, response: Response| {
            let authorization = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let user_agent = request
                .headers()
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            *server_headers.lock().unwrap() = Some((authorization, user_agent));
            Ok(response)
        })
        .await
        .unwrap();
        drop(socket);
    });

    let profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        base_url: format!("http://{address}"),
        api_key: "sk-probe-secret".to_string(),
        user_agent: "Codex-Probe-Test/1.0".to_string(),
        ..RelayProfile::default()
    };
    let result = probe_responses_websocket(&profile).await;
    server.await.unwrap();

    assert_eq!(result.state, ResponsesWebsocketCapabilityState::Supported);
    assert!(result.checked_at_ms.is_some());
    assert!(!result.message.contains("sk-probe-secret"));
    assert_eq!(
        captured_headers.lock().unwrap().clone(),
        Some((
            "Bearer sk-probe-secret".to_string(),
            "Codex-Probe-Test/1.0".to_string()
        ))
    );
}

#[tokio::test]
async fn probe_caches_only_explicit_unsupported_http_statuses() {
    for status in [200, 204, 404, 405, 410, 422, 426, 501] {
        let (base_url, server) = spawn_http_status_server(status, "sensitive response body").await;
        let profile = RelayProfile {
            relay_mode: RelayMode::PureApi,
            protocol: RelayProtocol::Responses,
            base_url,
            api_key: "sk-unsupported-secret".to_string(),
            ..RelayProfile::default()
        };

        let result = probe_responses_websocket(&profile).await;
        server.await.unwrap();

        assert_eq!(
            result.state,
            ResponsesWebsocketCapabilityState::Unsupported,
            "HTTP {status} should be explicit unsupported"
        );
        assert!(result.checked_at_ms.is_some());
        assert!(!result.message.contains("sensitive response body"));
        assert!(!result.message.contains("sk-unsupported-secret"));
    }
}

#[tokio::test]
async fn authentication_and_temporary_http_failures_remain_unknown() {
    for status in [400, 401, 403, 408, 429, 500, 503] {
        let (base_url, server) = spawn_http_status_server(status, "temporary secret body").await;
        let profile = RelayProfile {
            relay_mode: RelayMode::PureApi,
            protocol: RelayProtocol::Responses,
            base_url,
            api_key: "sk-temporary-secret".to_string(),
            ..RelayProfile::default()
        };

        let result = probe_responses_websocket(&profile).await;
        server.await.unwrap();

        assert_eq!(
            result.state,
            ResponsesWebsocketCapabilityState::Unknown,
            "HTTP {status} should remain unknown"
        );
        assert_eq!(result.checked_at_ms, None);
        assert!(!result.message.contains("temporary secret body"));
        assert!(!result.message.contains("sk-temporary-secret"));
    }
}

#[tokio::test]
async fn matching_explicit_cache_skips_network_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let server_connections = Arc::clone(&connections);
    let server = tokio::spawn(async move {
        if tokio::time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_ok()
        {
            server_connections.fetch_add(1, Ordering::SeqCst);
        }
    });

    let mut profile = RelayProfile {
        id: "cached-relay".to_string(),
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        base_url: format!("http://{address}"),
        ..RelayProfile::default()
    };
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    profile.responses_websocket.checked_at_ms = Some(1_720_000_000_000);
    profile.responses_websocket.message = "已有缓存".to_string();
    let mut settings = BackendSettings {
        active_relay_id: profile.id.clone(),
        relay_profiles: vec![profile],
        ..BackendSettings::default()
    };

    probe_active_relay_responses_websocket_if_needed(&mut settings).await;
    server.await.unwrap();

    assert_eq!(connections.load(Ordering::SeqCst), 0);
    assert_eq!(
        settings.relay_profiles[0].responses_websocket.state,
        ResponsesWebsocketCapabilityState::Supported
    );
    assert_eq!(
        settings.relay_profiles[0].responses_websocket.message,
        "已有缓存"
    );
}

#[tokio::test]
async fn local_proxy_bridges_responses_websocket_messages_and_authentication() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
            assert_eq!(
                request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer sk-bridge-secret")
            );
            Ok(response)
        })
        .await
        .unwrap();
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = message else {
            panic!("expected response.create text message");
        };
        let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        assert_eq!(payload["type"], "response.create");
        assert_eq!(payload["model"], "gpt-bridge");
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_bridge"}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let _ = socket.close(None).await;
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    let _proxy_log_path = ProxyLogPathGuard::new(temp.path().join("proxy-requests.jsonl"));
    let mut profile = RelayProfile {
        id: "relay-bridge".to_string(),
        name: "Bridge".to_string(),
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        local_proxy_enabled: Some(true),
        base_url: format!("http://{upstream_address}"),
        upstream_base_url: format!("http://{upstream_address}"),
        api_key: "sk-bridge-secret".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-bridge-secret"}"#.to_string(),
        config_contents: format!(
            "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"http://127.0.0.1:45221/v1\"\n"
        ),
        ..RelayProfile::default()
    };
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    SettingsStore::default()
        .save(&BackendSettings {
            relay_profiles: vec![profile],
            active_relay_id: "relay-bridge".to_string(),
            ..BackendSettings::default()
        })
        .unwrap();

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr))
            .await
            .unwrap();
    });

    let (mut client, _) = connect_async(format!("ws://{local_address}/v1/responses"))
        .await
        .unwrap();
    client
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-bridge",
                "input": [{"role": "user", "content": "hi"}],
                "stream": true
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let response = client.next().await.unwrap().unwrap();
    let Message::Text(text) = response else {
        panic!("expected response.completed text message");
    };
    let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
    assert_eq!(payload["type"], "response.completed");
    let _ = client.close(None).await;

    local_server.await.unwrap();
    upstream.await.unwrap();

    let summaries = codex_elves_core::proxy_log::read_summaries(10).unwrap();
    let summary = summaries
        .iter()
        .find(|entry| entry.model.as_deref() == Some("gpt-bridge"))
        .expect("websocket request should be recorded");
    assert_eq!(summary.state, ProxyRequestState::Completed);
    assert_eq!(summary.transport, ProxyRequestTransport::Ws);
    assert_eq!(summary.response_protocol.as_deref(), Some("responses"));
    assert_eq!(summary.status_code, Some(200));
    assert!(summary.first_token_ms.is_some());
    assert!(summary.duration_ms.is_some());

    let detail = codex_elves_core::proxy_log::find_record(&summary.id)
        .unwrap()
        .expect("websocket request detail should exist");
    assert!(detail.request_body.contains("\"type\":\"response.create\""));
    assert!(
        detail
            .response_body
            .contains("\"type\":\"response.completed\"")
    );
}

#[tokio::test]
async fn local_proxy_preserves_a_websocket_frame_read_with_the_upgrade_request() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(stream, |_request: &Request, response: Response| {
            Ok(response)
        })
        .await
        .unwrap();
        let message = socket.next().await.unwrap().unwrap();
        let Message::Text(text) = message else {
            panic!("expected response.create text message");
        };
        let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        let _ = socket.close(None).await;
        payload
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    let _proxy_log_path = ProxyLogPathGuard::new(temp.path().join("proxy-requests.jsonl"));
    save_supported_websocket_settings(upstream_address, false);

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });

    let payload = serde_json::json!({
        "type": "response.create",
        "model": "gpt-trailing-frame",
        "input": [{"role": "user", "content": "hi"}]
    })
    .to_string();
    let mut request = format!(
        "GET /v1/responses HTTP/1.1\r\nHost: {local_address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    )
    .into_bytes();
    request.extend_from_slice(&masked_text_frame(payload.as_bytes()));

    let mut client = tokio::net::TcpStream::connect(local_address).await.unwrap();
    client.write_all(&request).await.unwrap();
    let response = read_upgrade_request(&mut client).await;
    assert!(
        response.starts_with(b"HTTP/1.1 101"),
        "expected websocket upgrade, got {}",
        String::from_utf8_lossy(&response)
    );
    client.write_all(&masked_close_frame()).await.unwrap();

    let upstream_payload = upstream.await.unwrap();
    assert_eq!(upstream_payload["type"], "response.create");
    assert_eq!(upstream_payload["model"], "gpt-trailing-frame");
    local_server.await.unwrap().unwrap();
    drop(client);
}

#[tokio::test]
async fn upstream_connection_failure_rejects_upgrade_before_sending_101() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let unused_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unused_address = unused_listener.local_addr().unwrap();
    drop(unused_listener);

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    save_supported_websocket_settings(unused_address, false);

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });

    let error = connect_async(format!("ws://{local_address}/v1/responses"))
        .await
        .unwrap_err();
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP rejection before websocket upgrade");
    };
    assert_eq!(response.status().as_u16(), 502);
    local_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn explicitly_disabled_websocket_rejects_upgrade_before_connecting_upstream() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let unused_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unused_address = unused_listener.local_addr().unwrap();
    drop(unused_listener);

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    save_supported_websocket_settings(unused_address, false);
    let mut settings = SettingsStore::default().load().unwrap();
    settings.relay_profiles[0].responses_websocket_enabled = Some(false);
    SettingsStore::default().save(&settings).unwrap();

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });

    let error = connect_async(format!("ws://{local_address}/v1/responses"))
        .await
        .unwrap_err();
    let WebSocketError::Http(response) = error else {
        panic!("expected HTTP rejection before websocket upgrade");
    };
    assert_eq!(response.status().as_u16(), 409);
    local_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn reasoning_continuation_reuses_the_same_websocket_and_only_returns_the_final_round() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(stream, |_request: &Request, response: Response| {
            Ok(response)
        })
        .await
        .unwrap();

        let first_request = socket.next().await.unwrap().unwrap();
        let Message::Text(first_request) = first_request else {
            panic!("expected first response.create");
        };
        let first_request: serde_json::Value =
            serde_json::from_str(first_request.as_str()).unwrap();
        assert_eq!(first_request["type"], "response.create");
        assert_eq!(first_request["model"], "gpt-websocket-test");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_short",
                        "status": "completed",
                        "output": [{
                            "id": "rs_short",
                            "type": "reasoning",
                            "encrypted_content": "encrypted-short",
                            "summary": []
                        }],
                        "usage": {
                            "output_tokens_details": {
                                "reasoning_tokens": 516
                            }
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let continue_request = socket.next().await.unwrap().unwrap();
        let Message::Text(continue_request) = continue_request else {
            panic!("expected websocket continuation response.create");
        };
        let continue_request: serde_json::Value =
            serde_json::from_str(continue_request.as_str()).unwrap();
        assert_eq!(continue_request["type"], "response.create");
        assert_eq!(continue_request["model"], "gpt-websocket-test");
        assert!(continue_request.get("stream").is_none());
        assert!(continue_request.get("background").is_none());
        assert!(
            continue_request["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "continue_thinking")
        );
        assert!(
            continue_request["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["encrypted_content"] == "encrypted-short")
        );

        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_final",
                        "status": "completed",
                        "output": [{
                            "id": "msg_final",
                            "type": "message",
                            "role": "assistant",
                            "status": "completed",
                            "content": [{
                                "type": "output_text",
                                "text": "final answer"
                            }]
                        }],
                        "usage": {
                            "output_tokens_details": {
                                "reasoning_tokens": 1552
                            }
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let _ = socket.close(None).await;
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    let _proxy_log_path = ProxyLogPathGuard::new(temp.path().join("proxy-requests.jsonl"));
    save_supported_websocket_settings(upstream_address, true);

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });

    let (mut client, _) = connect_async(format!("ws://{local_address}/v1/responses"))
        .await
        .unwrap();
    client
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-websocket-test",
                "input": [{
                    "role": "user",
                    "content": "think carefully"
                }],
                "stream": true,
                "background": false
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let response = client.next().await.unwrap().unwrap();
    let Message::Text(response) = response else {
        panic!("expected final websocket response");
    };
    let response: serde_json::Value = serde_json::from_str(response.as_str()).unwrap();
    assert_eq!(response["type"], "response.completed");
    assert_eq!(response["response"]["id"], "resp_final");
    assert!(!response.to_string().contains("resp_short"));

    let _ = client.close(None).await;
    local_server.await.unwrap().unwrap();
    upstream.await.unwrap();

    let summaries = codex_elves_core::proxy_log::read_summaries(10).unwrap();
    let summary = summaries
        .iter()
        .find(|entry| entry.model.as_deref() == Some("gpt-websocket-test"))
        .expect("websocket continuation request should be recorded");
    assert_eq!(summary.transport, ProxyRequestTransport::Ws);
    assert_eq!(summary.state, ProxyRequestState::Completed);
    let detail = codex_elves_core::proxy_log::find_record(&summary.id)
        .unwrap()
        .expect("websocket continuation request detail should exist");
    assert!(detail.continue_thinking_triggered);
    assert_eq!(detail.continue_thinking_rounds, 1);
    assert_eq!(detail.reasoning_tokens, Some(2068));
    assert!(
        detail
            .continue_thinking_request_body
            .as_deref()
            .is_some_and(|body| body.contains("continue_thinking"))
    );
    assert!(
        detail
            .continue_thinking_before_response_body
            .as_deref()
            .is_some_and(|body| body.contains("resp_short"))
    );
    assert!(
        detail
            .continue_thinking_after_response_body
            .as_deref()
            .is_some_and(|body| body.contains("resp_final"))
    );
    assert!(!detail.response_body.contains("resp_short"));
    assert!(detail.response_body.contains("resp_final"));
}

async fn spawn_http_status_server(
    status: u16,
    body: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0_u8; 4096];
        let _ = stream.read(&mut request).await.unwrap();
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    (format!("http://{address}"), server)
}

async fn read_upgrade_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 2048];
    loop {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0, "client closed before websocket upgrade completed");
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return request;
        }
    }
}

fn websocket_settings_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

struct SettingsPathGuard {
    previous: Option<PathBuf>,
}

impl SettingsPathGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            previous: codex_elves_core::paths::set_settings_path_for_tests(Some(path)),
        }
    }
}

impl Drop for SettingsPathGuard {
    fn drop(&mut self) {
        codex_elves_core::paths::set_settings_path_for_tests(self.previous.take());
    }
}

struct ProxyLogPathGuard {
    previous: Option<PathBuf>,
}

impl ProxyLogPathGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            previous: codex_elves_core::paths::set_proxy_log_path_for_tests(Some(path)),
        }
    }
}

impl Drop for ProxyLogPathGuard {
    fn drop(&mut self) {
        codex_elves_core::paths::set_proxy_log_path_for_tests(self.previous.take());
    }
}

fn save_supported_websocket_settings(
    upstream_address: std::net::SocketAddr,
    reasoning_continuation: bool,
) {
    let mut profile = RelayProfile {
        id: "relay-websocket-test".to_string(),
        name: "WebSocket Test".to_string(),
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        local_proxy_enabled: Some(true),
        base_url: format!("http://{upstream_address}"),
        upstream_base_url: format!("http://{upstream_address}"),
        api_key: "sk-websocket-test".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-websocket-test"}"#.to_string(),
        config_contents: "model_provider = \"custom\"\n".to_string(),
        ..RelayProfile::default()
    };
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    SettingsStore::default()
        .save(&BackendSettings {
            relay_profiles: vec![profile],
            active_relay_id: "relay-websocket-test".to_string(),
            gpt_reasoning_continuation: reasoning_continuation,
            ..BackendSettings::default()
        })
        .unwrap();
}

fn masked_text_frame(payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= u16::MAX as usize);
    let mask = [0x12_u8, 0x34, 0x56, 0x78];
    let mut frame = Vec::with_capacity(payload.len() + 8);
    frame.push(0x81);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    frame
}

fn masked_close_frame() -> [u8; 6] {
    [0x88, 0x80, 0x12, 0x34, 0x56, 0x78]
}
