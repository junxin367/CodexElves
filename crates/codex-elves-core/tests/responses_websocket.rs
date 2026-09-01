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
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WebSocketError, Message};
use tokio_tungstenite::{accept_hdr_async, accept_hdr_async_with_config};

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
            alias: String::new(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        },
        RelayModelMapping {
            request_model: "claude-sonnet".to_string(),
            alias: String::new(),
            protocol: RelayProtocol::Anthropic,
            context_window: String::new(),
        },
    ];
    assert!(relay_supports_native_responses_websocket(&profile));

    profile.model_mappings = vec![RelayModelMapping {
        request_model: "chat-model".to_string(),
        alias: String::new(),
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
            alias: String::new(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        },
        RelayModelMapping {
            request_model: "claude-sonnet".to_string(),
            alias: String::new(),
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
            assert_eq!(
                request
                    .headers()
                    .get("x-codex-beta-features")
                    .and_then(|value| value.to_str().ok()),
                Some("remote_compaction_v2")
            );
            assert_eq!(
                request
                    .headers()
                    .get("x-codex-turn-state")
                    .and_then(|value| value.to_str().ok()),
                Some("turn-state-from-client")
            );
            assert_eq!(
                request
                    .headers()
                    .get("openai-beta")
                    .and_then(|value| value.to_str().ok()),
                Some("responses_websockets=2026-02-06")
            );
            assert!(request.headers().get("x-forwarded-for").is_none());
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
        model_mappings: vec![RelayModelMapping {
            request_model: "gpt-bridge".to_string(),
            alias: String::new(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        }],
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

    let mut client_request = format!("ws://{local_address}/v1/responses")
        .into_client_request()
        .unwrap();
    client_request.headers_mut().insert(
        "x-codex-beta-features",
        HeaderValue::from_static("remote_compaction_v2"),
    );
    client_request.headers_mut().insert(
        "x-codex-turn-state",
        HeaderValue::from_static("turn-state-from-client"),
    );
    client_request.headers_mut().insert(
        "openai-beta",
        HeaderValue::from_static("responses_websockets=2026-02-06"),
    );
    client_request.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer local-client-token"),
    );
    client_request
        .headers_mut()
        .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.42"));
    let (mut client, _) = connect_async(client_request).await.unwrap();
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
async fn local_proxy_rewrites_websocket_alias_slug_and_prompt_identity_to_request_model() {
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
        assert_eq!(payload["model"], "gpt-5.6-sol");
        assert_eq!(
            payload["instructions"],
            "You are Codex, an agent based on the gpt-5.6-sol model."
        );
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_alias"}
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
        id: "relay-websocket-alias".to_string(),
        name: "WebSocket Alias".to_string(),
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        local_proxy_enabled: Some(true),
        base_url: format!("http://{upstream_address}"),
        upstream_base_url: format!("http://{upstream_address}"),
        api_key: "sk-websocket-alias".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-websocket-alias"}"#.to_string(),
        model_mappings: vec![RelayModelMapping {
            request_model: "gpt-5.6-sol".to_string(),
            alias: "gpt-5.6-sol [500K]".to_string(),
            protocol: RelayProtocol::Responses,
            context_window: "500000".to_string(),
        }],
        config_contents: "model_provider = \"custom\"\n".to_string(),
        ..RelayProfile::default()
    };
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    SettingsStore::default()
        .save(&BackendSettings {
            relay_profiles: vec![profile],
            active_relay_id: "relay-websocket-alias".to_string(),
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
                "model": "gpt-5.6-sol [500K]",
                "instructions": "You are Codex, an agent based on the gpt-5.6-sol [500K] model.",
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
}

#[tokio::test]
async fn local_proxy_accepts_legacy_hidden_alias_slug_without_forwarding_it() {
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
        assert_eq!(payload["model"], "gpt-5.6-sol");
        assert_eq!(
            payload["instructions"],
            "You are Codex, an agent based on the gpt-5.6-sol model."
        );
        assert_eq!(
            payload["input"][0]["content"][0]["text"],
            "Use the gpt-5.6-sol model."
        );
        assert!(!text.contains("--codex-elves-alias-2"));
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_legacy_hidden_alias"}
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
        id: "relay-websocket-legacy-hidden-alias".to_string(),
        name: "WebSocket Legacy Hidden Alias".to_string(),
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        local_proxy_enabled: Some(true),
        base_url: format!("http://{upstream_address}"),
        upstream_base_url: format!("http://{upstream_address}"),
        api_key: "sk-websocket-legacy-hidden-alias".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-websocket-legacy-hidden-alias"}"#.to_string(),
        model_mappings: vec![
            RelayModelMapping {
                request_model: "gpt-5.6-sol".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "372000".to_string(),
            },
            RelayModelMapping {
                request_model: "gpt-5.6-sol".to_string(),
                alias: "gpt-5.6-sol [500K]".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: "500000".to_string(),
            },
        ],
        config_contents: "model_provider = \"custom\"\n".to_string(),
        ..RelayProfile::default()
    };
    normalize_responses_websocket_capability(&mut profile);
    profile.responses_websocket.state = ResponsesWebsocketCapabilityState::Supported;
    SettingsStore::default()
        .save(&BackendSettings {
            relay_profiles: vec![profile],
            active_relay_id: "relay-websocket-legacy-hidden-alias".to_string(),
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
                "model": "gpt-5.6-sol--codex-elves-alias-2",
                "instructions": "You are Codex, an agent based on the gpt-5.6-sol--codex-elves-alias-2 model.",
                "input": [
                    {
                        "type": "message",
                        "role": "developer",
                        "content": [{
                            "type": "input_text",
                            "text": "Use the gpt-5.6-sol--codex-elves-alias-2 model."
                        }]
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": "hi"
                        }]
                    }
                ],
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
}

#[tokio::test]
async fn local_proxy_does_not_turn_upstream_pong_into_application_event() {
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
        let request = socket.next().await.unwrap().unwrap();
        assert!(matches!(request, Message::Text(_)));
        socket
            .send(Message::Pong(b"alive".to_vec().into()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_keepalive"}
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
    save_supported_websocket_settings(upstream_address, false, "gpt-keepalive");

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
                "model": "gpt-keepalive",
                "input": [{"role": "user", "content": "hi"}],
                "stream": true
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let mut saw_synthetic_event = false;
    loop {
        let message = tokio::time::timeout(Duration::from_secs(1), client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Text(text) = message else {
            continue;
        };
        let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        match payload["type"].as_str() {
            Some("codex_elves.keepalive") => saw_synthetic_event = true,
            Some("response.completed") => break,
            _ => {}
        }
    }
    assert!(!saw_synthetic_event);

    let _ = client.close(None).await;
    local_server.await.unwrap();
    upstream.await.unwrap();
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
    save_supported_websocket_settings(upstream_address, false, "gpt-trailing-frame");

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
async fn local_proxy_prunes_compacted_image_history_before_websocket_size_check() {
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
        assert!(text.len() < 16 * 1024 * 1024);
        let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "compaction");
        assert_eq!(input[1]["content"][0]["text"], "继续");
        assert!(!text.contains("data:image/png;base64"));

        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_compacted_image_history"}
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
    save_supported_websocket_settings(upstream_address, false, "gpt-compacted-images");

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
                "model": "gpt-compacted-images",
                "store": false,
                "stream": true,
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_image",
                            "image_url": format!(
                                "data:image/png;base64,{}",
                                "A".repeat(17 * 1024 * 1024)
                            )
                        }]
                    },
                    {
                        "type": "compaction",
                        "id": "cmp_latest",
                        "encrypted_content": "opaque"
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": "继续"
                        }]
                    }
                ]
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
}

#[tokio::test]
async fn local_proxy_restores_resumed_compaction_checkpoint_before_websocket_size_check() {
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
        assert!(text.len() < 16 * 1024 * 1024);
        let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "compaction");
        assert_eq!(input[0]["id"], "cmp_window_75");
        assert_eq!(input[1]["content"][0]["text"], "继续");
        assert!(!text.contains("data:image/png;base64"));

        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_resumed_compaction_checkpoint"}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let _ = socket.close(None).await;
    });

    let temp = tempfile::tempdir().unwrap();
    let codex_home = temp.path().join("codex-home");
    let thread_id = uuid::Uuid::new_v4().to_string();
    let rollout_path = codex_home
        .join("sessions")
        .join("2026")
        .join("08")
        .join("12")
        .join(format!("rollout-2026-08-12T00-00-00-{thread_id}.jsonl"));
    std::fs::create_dir_all(rollout_path.parent().unwrap()).unwrap();
    let historical_message = serde_json::json!({
        "type": "message",
        "id": "msg_historical_image",
        "role": "user",
        "content": [{
            "type": "input_image",
            "image_url": format!(
                "data:image/png;base64,{}",
                "A".repeat(17 * 1024 * 1024)
            )
        }]
    });
    let checkpoint = serde_json::json!({
        "timestamp": "2026-08-12T00:00:00Z",
        "type": "compacted",
        "payload": {
            "message": "",
            "replacement_history": [
                historical_message.clone(),
                {
                    "type": "compaction",
                    "id": "cmp_window_75",
                    "encrypted_content": "opaque"
                }
            ],
            "window_number": 75,
            "window_id": "window-75"
        }
    });
    std::fs::write(&rollout_path, format!("{checkpoint}\n")).unwrap();
    let state_db = rusqlite::Connection::open(codex_home.join("state_5.sqlite")).unwrap();
    state_db
        .execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT NOT NULL
            );",
        )
        .unwrap();
    state_db
        .execute(
            "INSERT INTO threads (id, rollout_path) VALUES (?1, ?2)",
            (&thread_id, rollout_path.to_string_lossy().as_ref()),
        )
        .unwrap();
    drop(state_db);

    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    let _proxy_log_path = ProxyLogPathGuard::new(temp.path().join("proxy-requests.jsonl"));
    save_supported_websocket_settings(upstream_address, false, "gpt-resumed-compacted-images");
    let mut settings = SettingsStore::default().load().unwrap();
    settings.codex_home_path = codex_home.to_string_lossy().to_string();
    SettingsStore::default().save(&settings).unwrap();

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
                "model": "gpt-resumed-compacted-images",
                "store": false,
                "stream": true,
                "input": [
                    historical_message,
                    {
                        "type": "message",
                        "id": "msg_current",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": "继续"
                        }]
                    }
                ],
                "client_metadata": {
                    "thread_id": thread_id,
                    "session_id": thread_id,
                    "x-codex-window-id": format!("{thread_id}:75")
                }
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
}

#[tokio::test]
async fn local_proxy_falls_back_same_turn_when_websocket_request_exceeds_16_mib() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        let config = WebSocketConfig::default()
            .max_frame_size(Some(64 * 1024 * 1024))
            .max_message_size(Some(64 * 1024 * 1024));
        let mut socket = accept_hdr_async_with_config(
            stream,
            |_request: &Request, response: Response| Ok(response),
            Some(config),
        )
        .await
        .unwrap();
        let message = socket.next().await.unwrap().unwrap();
        assert!(
            matches!(message, Message::Close(_)),
            "oversized response.create must not reach the upstream websocket"
        );
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    let _proxy_log_path = ProxyLogPathGuard::new(temp.path().join("proxy-requests.jsonl"));
    save_supported_websocket_settings(upstream_address, false, "gpt-large-frame");

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
            let request_bytes = read_upgrade_request(&mut stream).await;
            let _ = handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr))
                .await;
        }
    });

    let websocket_request = || {
        let mut request = format!("ws://{local_address}/v1/responses")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("session-id", HeaderValue::from_static("large-session"));
        request
            .headers_mut()
            .insert("thread-id", HeaderValue::from_static("large-thread"));
        request.headers_mut().insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static(
                r#"{"turn_id":"large-turn","request_kind":"turn","window_id":"large-window"}"#,
            ),
        );
        request
    };
    let (mut client, _) = connect_async(websocket_request()).await.unwrap();
    let content = "x".repeat(17 * 1024 * 1024);
    client
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-large-frame",
                "input": [{"role": "user", "content": content}]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let message = tokio::time::timeout(Duration::from_secs(10), client.next())
        .await
        .unwrap()
        .unwrap();
    let Message::Close(Some(close)) = message.unwrap() else {
        panic!("oversized response.create should receive a websocket close");
    };
    assert_eq!(close.code, CloseCode::Size);

    let error = connect_async(websocket_request()).await.unwrap_err();
    let WebSocketError::Http(response) = error else {
        panic!("same turn should be rejected before websocket upgrade");
    };
    assert_eq!(response.status().as_u16(), 426);

    upstream.await.unwrap();
    local_server.await.unwrap();
}

#[tokio::test]
async fn unassigned_gpt_model_uses_inferred_responses_websocket_protocol() {
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
        assert_eq!(payload["model"], "gpt-5.3-codex-spark");
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {"id": "resp_inferred_protocol"}
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
    save_supported_websocket_settings(upstream_address, false, "gpt-assigned");

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
                "model": "gpt-5.3-codex-spark",
                "input": [{"role": "user", "content": "hi"}]
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

    local_server.await.unwrap().unwrap();
    upstream.await.unwrap();
}

#[tokio::test]
async fn upstream_connection_failure_rejects_upgrade_before_sending_101() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let unused_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unused_address = unused_listener.local_addr().unwrap();
    drop(unused_listener);

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    save_supported_websocket_settings(unused_address, false, "gpt-websocket-test");

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
    save_supported_websocket_settings(unused_address, false, "gpt-websocket-test");
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
    assert_eq!(response.status().as_u16(), 426);
    local_server.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_proxy_downgrades_same_turn_after_disconnect_before_first_request() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_connection_count = Arc::new(AtomicUsize::new(0));
    let upstream_count = upstream_connection_count.clone();
    let upstream = tokio::spawn(async move {
        loop {
            let accepted =
                tokio::time::timeout(Duration::from_secs(1), upstream_listener.accept()).await;
            let Ok(Ok((stream, _))) = accepted else {
                break;
            };
            upstream_count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let Ok(mut socket) =
                    accept_hdr_async(stream, |_request: &Request, response: Response| {
                        Ok(response)
                    })
                    .await
                else {
                    return;
                };
                while let Some(message) = socket.next().await {
                    if matches!(message, Ok(Message::Close(_)) | Err(_)) {
                        break;
                    }
                }
            });
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    save_supported_websocket_settings(upstream_address, false, "gpt-side-turn");

    let local_listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
    let local_address = local_listener.local_addr().unwrap();
    let first_listener = local_listener.clone();
    let first_local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = first_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });

    let mut first_client = tokio::net::TcpStream::connect(local_address).await.unwrap();
    let upgrade_request = format!(
        "GET /v1/responses HTTP/1.1\r\n\
         Host: {local_address}\r\n\
         Connection: Upgrade\r\n\
         Upgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         session-id: side-session\r\n\
         thread-id: side-thread\r\n\
         x-codex-window-id: side-window\r\n\
         x-codex-turn-metadata: {{\"turn_id\":\"side-turn\",\"request_kind\":\"turn\",\"window_id\":\"side-window\"}}\r\n\
         \r\n"
    );
    first_client
        .write_all(upgrade_request.as_bytes())
        .await
        .unwrap();
    let first_response = read_upgrade_request(&mut first_client).await;
    assert!(
        first_response.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&first_response)
    );
    first_client.shutdown().await.unwrap();
    drop(first_client);
    let first_result = tokio::time::timeout(Duration::from_secs(2), first_local_server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        first_result
            .unwrap_err()
            .to_string()
            .contains("读取本地 Responses WebSocket 消息失败")
    );

    let second_listener = local_listener.clone();
    let second_local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = second_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });
    let mut second_client = tokio::net::TcpStream::connect(local_address).await.unwrap();
    second_client
        .write_all(upgrade_request.as_bytes())
        .await
        .unwrap();
    let second_response = read_upgrade_request(&mut second_client).await;
    assert!(
        second_response.starts_with(b"HTTP/1.1 426"),
        "{}",
        String::from_utf8_lossy(&second_response)
    );
    drop(second_client);
    second_local_server.await.unwrap().unwrap();

    let third_listener = local_listener.clone();
    let third_local_server = tokio::spawn(async move {
        let (mut stream, remote_addr) = third_listener.accept().await.unwrap();
        let request_bytes = read_upgrade_request(&mut stream).await;
        handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr)).await
    });
    let mut third_client = tokio::net::TcpStream::connect(local_address).await.unwrap();
    let different_turn_request =
        upgrade_request.replace("\"side-turn\"", "\"different-side-turn\"");
    third_client
        .write_all(different_turn_request.as_bytes())
        .await
        .unwrap();
    let third_response = read_upgrade_request(&mut third_client).await;
    assert!(
        third_response.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&third_response)
    );
    third_client.shutdown().await.unwrap();
    drop(third_client);
    let third_result = tokio::time::timeout(Duration::from_secs(2), third_local_server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        third_result
            .unwrap_err()
            .to_string()
            .contains("读取本地 Responses WebSocket 消息失败")
    );

    upstream.await.unwrap();
    assert_eq!(upstream_connection_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn local_proxy_keeps_websocket_for_same_turn_after_application_request() {
    let _settings_lock = websocket_settings_test_lock().lock().await;
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream = tokio::spawn(async move {
        for connection_index in 0..2 {
            let (stream, _) = upstream_listener.accept().await.unwrap();
            let mut socket = accept_hdr_async(stream, |_request: &Request, response: Response| {
                Ok(response)
            })
            .await
            .unwrap();
            if connection_index == 0 {
                let request = socket.next().await.unwrap().unwrap();
                let Message::Text(request) = request else {
                    panic!("expected response.create");
                };
                let request: serde_json::Value = serde_json::from_str(request.as_str()).unwrap();
                assert_eq!(request["type"], "response.create");
                socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type": "response.completed",
                            "response": {"id": "resp_side_turn"}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
            }
            while let Some(message) = socket.next().await {
                if matches!(message, Ok(Message::Close(_)) | Err(_)) {
                    break;
                }
            }
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let _settings_path = SettingsPathGuard::new(temp.path().join("settings.json"));
    save_supported_websocket_settings(upstream_address, false, "gpt-side-turn-requested");

    let local_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let local_server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, remote_addr) = local_listener.accept().await.unwrap();
            let request_bytes = read_upgrade_request(&mut stream).await;
            let _ = handle_responses_websocket_connection(stream, request_bytes, Some(remote_addr))
                .await;
        }
    });

    let request = websocket_turn_request(
        local_address,
        "requested-side-thread",
        "requested-side-turn",
    );
    let (mut first_client, _) = connect_async(request).await.unwrap();
    first_client
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-side-turn-requested",
                "input": [{"role": "user", "content": "hi"}],
                "stream": true
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let response = first_client.next().await.unwrap().unwrap();
    let Message::Text(response) = response else {
        panic!("expected response.completed");
    };
    let response: serde_json::Value = serde_json::from_str(response.as_str()).unwrap();
    assert_eq!(response["type"], "response.completed");
    first_client.close(None).await.unwrap();
    drop(first_client);

    let request = websocket_turn_request(
        local_address,
        "requested-side-thread",
        "requested-side-turn",
    );
    let (mut second_client, response) = connect_async(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 101);
    second_client.close(None).await.unwrap();
    drop(second_client);

    local_server.await.unwrap();
    upstream.await.unwrap();
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
        assert_eq!(first_request["previous_response_id"], "resp_parent");
        assert_eq!(first_request["store"], false);

        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.created",
                    "response": {
                        "id": "resp_short",
                        "status": "in_progress"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;

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
        assert_eq!(continue_request["previous_response_id"], "resp_short");
        assert!(continue_request.get("stream").is_none());
        assert!(continue_request.get("stream_options").is_none());
        assert!(continue_request.get("background").is_none());
        assert!(continue_request.get("conversation").is_none());
        assert!(continue_request.get("conversation_id").is_none());
        let continue_input = continue_request["input"].as_array().unwrap();
        assert_eq!(continue_input.len(), 1);
        assert_eq!(continue_input[0]["role"], "developer");
        assert!(!continue_request.to_string().contains("think carefully"));
        assert!(!continue_request.to_string().contains("encrypted-short"));

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
    save_supported_websocket_settings(upstream_address, true, "gpt-websocket-test");

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
                "store": false,
                "previous_response_id": "resp_parent",
                "input": [{
                    "role": "user",
                    "content": "think carefully"
                }],
                "stream": true,
                "stream_options": {
                    "include_usage": true
                },
                "background": false
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let response = loop {
        let message = client.next().await.unwrap().unwrap();
        let Message::Text(response) = message else {
            continue;
        };
        break serde_json::from_str::<serde_json::Value>(response.as_str()).unwrap();
    };
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
    let first_token_ms = summary
        .first_token_ms
        .expect("websocket first response event should record first token latency");
    let duration_ms = summary
        .duration_ms
        .expect("websocket terminal event should record full duration");
    assert!(
        duration_ms >= first_token_ms.saturating_add(50),
        "first token latency {first_token_ms}ms should be lower than duration {duration_ms}ms"
    );
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
            .is_some_and(|body| {
                body.contains("\"previous_response_id\": \"resp_short\"")
                    && body.contains("unpublished, incomplete draft")
            })
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

fn websocket_turn_request(
    local_address: std::net::SocketAddr,
    thread_id: &str,
    turn_id: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut request = format!("ws://{local_address}/v1/responses")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("session-id", HeaderValue::from_static("side-session"));
    request
        .headers_mut()
        .insert("thread-id", HeaderValue::from_str(thread_id).unwrap());
    request
        .headers_mut()
        .insert("x-codex-window-id", HeaderValue::from_static("side-window"));
    let turn_metadata = serde_json::json!({
        "turn_id": turn_id,
        "request_kind": "turn",
        "window_id": "side-window"
    })
    .to_string();
    request.headers_mut().insert(
        "x-codex-turn-metadata",
        HeaderValue::from_str(&turn_metadata).unwrap(),
    );
    request
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
    request_model: &str,
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
        model_mappings: vec![RelayModelMapping {
            request_model: request_model.to_string(),
            alias: String::new(),
            protocol: RelayProtocol::Responses,
            context_window: String::new(),
        }],
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
