use codex_elves_core::codex_sqlite::codex_session_db_path_from_home;
use codex_elves_core::relay_config::{
    apply_pure_api_config_to_home, apply_relay_config_file_to_home, apply_relay_config_to_home,
    apply_relay_files_to_home, apply_relay_files_to_home_with_common,
    apply_relay_profile_files_to_home_with_context, apply_relay_profile_to_home_with_switch_rules,
    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard,
    backfill_relay_profile_from_home, backfill_relay_profile_from_home_with_common,
    chatgpt_auth_status_from_home, clear_relay_config_to_home,
    clear_relay_config_to_home_with_auth, delete_context_entry_from_common_config,
    extract_common_config_from_config, filter_common_config_for_selection,
    list_context_entries_from_common_config, normalize_relay_profile_for_storage,
    relay_config_status_from_home, sanitize_common_config_contents,
    set_codex_goals_feature_in_home, strip_common_config_from_config,
    sync_applied_relay_profile_websocket_to_home, sync_live_config_context_entries,
    sync_live_config_context_entry, upsert_context_entry_in_common_config,
};
use codex_elves_core::settings::{
    RelayContextSelection, RelayMode, RelayModelMapping, RelayProfile, RelayProtocol,
    ResponsesWebsocketCapability, ResponsesWebsocketCapabilityState,
};

fn write_remote_plugin_marketplace_snapshot(home: &std::path::Path) {
    let root = home.join(".tmp").join("plugins-remote");
    std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
    std::fs::create_dir_all(
        root.join("plugins")
            .join("product-design")
            .join(".codex-plugin"),
    )
    .unwrap();
    std::fs::write(
        root.join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design","path":"./plugins/product-design"}]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("plugins")
            .join("product-design")
            .join(".codex-plugin")
            .join("plugin.json"),
        r#"{"name":"product-design"}"#,
    )
    .unwrap();
}

#[test]
fn codex_session_db_path_prefers_new_sqlite_directory_threads_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();
    std::fs::write(home.join("state_5.sqlite"), b"legacy").unwrap();

    let ignored = rusqlite::Connection::open(sqlite_dir.join("other.db")).unwrap();
    ignored
        .execute("CREATE TABLE metadata (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(ignored);

    let selected_path = sqlite_dir.join("codex-dev.db");
    let selected = rusqlite::Connection::open(&selected_path).unwrap();
    selected
        .execute("CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT)", [])
        .unwrap();
    drop(selected);

    assert_eq!(codex_session_db_path_from_home(home), selected_path);
}

#[test]
fn apply_relay_config_preserves_cached_remote_plugin_marketplace() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    write_remote_plugin_marketplace_snapshot(home);

    apply_relay_files_to_home(
        home,
        r#"model = "gpt-5"
model_provider = "chatgpt"
"#,
        r#"{"auth_mode":"chatgpt"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(config.contains("[marketplaces.openai-curated-remote]"));
    assert!(config.contains(r#"source_type = "local""#));
    assert!(config.contains(".tmp\\plugins-remote") || config.contains(".tmp/plugins-remote"));
}

#[test]
fn codex_session_db_path_accepts_new_automation_runs_schema() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();

    let selected_path = sqlite_dir.join("codex-dev.db");
    let selected = rusqlite::Connection::open(&selected_path).unwrap();
    selected
        .execute(
            "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();
    drop(selected);

    assert_eq!(codex_session_db_path_from_home(home), selected_path);
}

#[test]
fn codex_session_db_path_prefers_threads_db_over_codex_dev_inbox_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let sqlite_dir = home.join("sqlite");
    std::fs::create_dir(&sqlite_dir).unwrap();

    let inbox_path = sqlite_dir.join("codex-dev.db");
    let inbox = rusqlite::Connection::open(&inbox_path).unwrap();
    inbox
        .execute(
            "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();
    inbox
        .execute("CREATE TABLE inbox_items (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(inbox);

    let threads_path = sqlite_dir.join("state_5.sqlite");
    let threads = rusqlite::Connection::open(&threads_path).unwrap();
    threads
        .execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, title TEXT)",
            [],
        )
        .unwrap();
    drop(threads);

    assert_eq!(codex_session_db_path_from_home(home), threads_path);
}

#[test]
fn codex_session_db_path_falls_back_to_legacy_state_db() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();

    assert_eq!(
        codex_session_db_path_from_home(home),
        home.join("state_5.sqlite")
    );
}

#[test]
fn detects_chatgpt_login_from_auth_json_and_config_provider() {
    let temp = tempfile::tempdir().unwrap();
    let id_token = format!(
        "header.{}.signature",
        base64_url_no_pad(r#"{"email":"user@example.test","name":"Codex User"}"#)
    );
    std::fs::write(
        temp.path().join("auth.json"),
        format!(
            r#"{{"auth_mode":"chatgpt","tokens":{{"id_token":"{id_token}","access_token":"access-token","refresh_token":"refresh-token"}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "chatgpt"
"#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
    assert_eq!(status.account_label.as_deref(), Some("user@example.test"));
}

#[test]
fn detects_chatgpt_login_when_config_exists_without_model_provider() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "gpt-5""#).unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
}

#[test]
fn rejects_auth_json_tokens_without_chatgpt_auth_mode() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"apikey","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "chatgpt""#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(!status.authenticated);
}

#[test]
fn detects_chatgpt_login_from_auth_json_without_config_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"access-token"}}"#,
    )
    .unwrap();

    let status = chatgpt_auth_status_from_home(temp.path());

    assert!(status.authenticated);
    assert!(status.source.contains("auth.json"));
}

#[test]
fn reports_relay_configured_when_required_keys_exist() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
OPENAI_API_KEY = "sk-should-be-removed"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
experimental_bearer_token = "sk-test-redacted"
"#,
    )
    .unwrap();

    let status = relay_config_status_from_home(temp.path());

    assert!(status.configured);
    assert!(status.requires_openai_auth);
    assert!(status.has_bearer_token);
}

#[test]
fn reports_pure_api_configured_from_auth_api_key_without_bearer_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:45221/v1"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#,
    )
    .unwrap();

    let status = relay_config_status_from_home(temp.path());

    assert!(status.configured);
    assert!(status.requires_openai_auth);
    assert!(!status.has_bearer_token);
}

#[test]
fn apply_relay_config_patches_provider_without_dropping_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-mpgm24lf.json'
model_provider = "custom1"
[model_providers.custom1]
name = "custom1"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    let result = apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(updated.contains(r#"model = "gpt-5""#));
    assert!(updated.contains("model_catalog_json"));
    assert!(updated.contains(r#"model_provider = "custom1""#));
    assert!(updated.contains("[model_providers.custom1]"));
    assert!(updated.contains("[profiles.default]"));
    assert!(!updated.contains(r#"model_provider = "custom""#));
    assert!(!updated.contains("[model_providers.custom]"));
    assert!(updated.contains(r#"name = "custom1""#));
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains("requires_openai_auth = true"));
    assert!(updated.contains("supports_websockets = false"));
    assert!(updated.contains(r#"base_url = "https://relay.example.test/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
}

#[test]
fn apply_chat_protocol_relay_points_codex_to_local_responses_proxy() {
    let temp = tempfile::tempdir().unwrap();

    let result = codex_elves_core::relay_config::apply_relay_config_to_home_with_protocol(
        temp.path(),
        "https://chat-only.example.test/v1",
        "sk-test-redacted",
        RelayProtocol::ChatCompletions,
        45221,
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains(r#"base_url = "http://127.0.0.1:45221/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
    assert!(!updated.contains("codex_elves_chat_base_url"));
}

#[test]
fn apply_aggregate_relay_points_codex_to_local_responses_proxy_without_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "agg".to_string(),
        name: "聚合供应商 1".to_string(),
        relay_mode: RelayMode::Aggregate,
        config_contents: String::new(),
        auth_contents: String::new(),
        ..RelayProfile::default()
    };

    let result = apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(result.configured);
    assert!(updated.contains(r#"wire_api = "responses""#));
    assert!(updated.contains(r#"base_url = "http://127.0.0.1:45221/v1""#));
    assert!(updated.contains(r#"experimental_bearer_token = "codex-elves-aggregate""#));
}

#[test]
fn chat_protocol_profile_keeps_upstream_base_url_separate_from_codex_proxy() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = RelayProfile {
        id: "relay-chat".to_string(),
        model: "deepseek-chat".to_string(),
        upstream_base_url: "https://api.deepseek.com".to_string(),
        api_key: "sk-test-redacted".to_string(),
        protocol: RelayProtocol::ChatCompletions,
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "deepseek-chat"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:45221/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#.to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.upstream_base_url, "https://api.deepseek.com");
    assert_eq!(profile.base_url, "https://api.deepseek.com");
    assert!(
        !profile
            .config_contents
            .contains("codex_elves_chat_base_url")
    );
    assert!(
        profile
            .config_contents
            .contains(r#"base_url = "http://127.0.0.1:45221/v1""#)
    );

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();
    let live = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!live.contains("codex_elves_chat_base_url"));
    assert!(live.contains(r#"base_url = "http://127.0.0.1:45221/v1""#));
}

#[test]
fn official_mix_api_profile_does_not_generate_auth_api_key() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-mix".to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert!(profile.auth_contents.trim().is_empty());
    assert!(
        profile
            .config_contents
            .contains(r#"wire_api = "responses""#)
    );
    assert!(
        profile
            .config_contents
            .contains("requires_openai_auth = true")
    );
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-mix""#)
    );
}

#[test]
fn official_mix_api_profile_does_not_take_api_key_from_auth() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        auth_contents: r#"{"OPENAI_API_KEY":"sk-pure-api"}"#.to_string(),
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-mix"
"#
        .to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.api_key, "sk-mix");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-mix""#)
    );
    assert!(!profile.config_contents.contains("sk-pure-api"));
}

#[test]
fn official_mix_api_profile_removes_auth_api_key_on_storage() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        api_key: "sk-official-mix".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-pure-api","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#.to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official");
}

#[test]
fn apply_pure_api_config_switches_auth_json_and_patches_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"old"}}"#,
    )
    .unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "gpt-5""#).unwrap();

    let result = apply_pure_api_config_to_home(
        temp.path(),
        "http://192.168.188.245:3001/v1",
        "sk-test-redacted",
    )
    .unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(result.configured);
    assert!(config.contains(r#"model = "gpt-5""#));
    assert_eq!(
        auth,
        serde_json::json!({"OPENAI_API_KEY":"sk-test-redacted"})
    );
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(config.contains(r#"base_url = "http://192.168.188.245:3001/v1""#));
    assert!(config.contains(r#"experimental_bearer_token = "sk-test-redacted""#));
}

#[test]
fn apply_relay_files_switches_complete_config_and_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), r#"model = "old""#).unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let result = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay-a.example/v1"
experimental_bearer_token = "sk-a"
"#,
        r#"{"OPENAI_API_KEY":"sk-a"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();

    assert!(result.configured);
    let backup_path = result.backup_path.as_ref().expect("backup path");
    assert!(backup_path.contains("codex-elves-live-"));
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(backup_path).join("config.toml")).unwrap(),
        r#"model = "old""#
    );
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(backup_path).join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
    assert!(config.contains(r#"base_url = "https://relay-a.example/v1""#));
    assert_eq!(auth, r#"{"OPENAI_API_KEY":"sk-a"}"#);
}

#[test]
fn apply_relay_files_allows_empty_isolated_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"OPENAI_API_KEY":"old"}"#).unwrap();

    let result = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "chatgpt"
"#,
        "",
    )
    .unwrap();

    assert!(!result.configured);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        ""
    );
}

#[test]
fn lists_codex_context_entries_from_common_config() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
    )
    .unwrap();

    assert_eq!(entries.mcp_servers[0].id, "context7");
    assert_eq!(entries.mcp_servers[0].summary, r#"command = "npx""#);
    assert_eq!(entries.skills[0].id, "writer");
    assert_eq!(entries.plugins[0].id, "local");
}

#[test]
fn lists_codex_context_entries_with_parent_mcp_table() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers]

[mcp_servers.ida-pro-mcp]
type = "stdio"
command = 'C:\Users\Administrator\AppData\Local\Programs\Python\Python313\python.exe'
args = ['C:\Users\Administrator\AppData\Local\Programs\Python\Python313\Lib\site-packages\ida_pro_mcp\server.py']
disabled = false
timeout = 1800
"#,
    )
    .unwrap();

    assert_eq!(entries.mcp_servers.len(), 1);
    assert_eq!(entries.mcp_servers[0].id, "ida-pro-mcp");
    assert!(entries.mcp_servers[0].enabled);
    assert!(
        entries.mcp_servers[0]
            .toml_body
            .contains("disabled = false")
    );
}

#[test]
fn lists_codex_context_entries_ignores_child_only_mcp_tool_tables() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers.sequential-thinking.tools.sequentialthinking]
approval_mode = "approved"

[mcp_servers.fetch]
command = "fetch"
"#,
    )
    .unwrap();

    assert_eq!(entries.mcp_servers.len(), 1);
    assert_eq!(entries.mcp_servers[0].id, "fetch");
    assert!(
        !entries
            .mcp_servers
            .iter()
            .any(|entry| entry.id == "sequential-thinking")
    );
}

#[test]
fn lists_codex_context_entries_with_enabled_state() {
    let entries = list_context_entries_from_common_config(
        r#"[mcp_servers.enabled_mcp]
disabled = false

[mcp_servers.disabled_mcp]
disabled = true

[plugins.enabled_plugin]
enabled = true

[plugins.disabled_plugin]
enabled = false
"#,
    )
    .unwrap();

    assert!(entries.mcp_servers[0].enabled);
    assert!(!entries.mcp_servers[1].enabled);
    assert!(entries.plugins[0].enabled);
    assert!(!entries.plugins[1].enabled);
}

#[test]
fn sync_live_config_context_entries_toggles_live_context_by_enabled_state() {
    let live = r#"model = "gpt-5"

[mcp_servers]

[mcp_servers.ida-pro-mcp]
command = "python"
enabled = true

[plugins."browser@openai-bundled"]
enabled = true
"#;
    let disabled = r#"[mcp_servers.ida-pro-mcp]
command = "python"
enabled = false

[plugins."browser@openai-bundled"]
enabled = true
"#;

    let updated = sync_live_config_context_entries(live, disabled).unwrap();

    assert!(updated.contains(r#"model = "gpt-5""#));
    assert!(!updated.contains("[mcp_servers.ida-pro-mcp]"));
    assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));

    let enabled = r#"[mcp_servers.ida-pro-mcp]
command = "python"
enabled = true
"#;

    let updated = sync_live_config_context_entries(&updated, enabled).unwrap();

    assert!(updated.contains("[mcp_servers.ida-pro-mcp]"));
    assert!(updated.contains(r#"command = "python""#));
    assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));
}

#[test]
fn upserts_and_deletes_context_entry_in_common_config() {
    let common = upsert_context_entry_in_common_config(
        "",
        "mcp",
        "context7",
        r#"command = "npx"
args = ["-y", "@upstash/context7-mcp"]
"#,
    )
    .unwrap();

    assert!(common.contains("[mcp_servers.context7]"));
    assert!(common.contains(r#"command = "npx""#));

    let updated =
        upsert_context_entry_in_common_config(&common, "mcp", "context7", r#"command = "bunx""#)
            .unwrap();

    assert!(updated.contains(r#"command = "bunx""#));
    assert!(!updated.contains(r#"command = "npx""#));

    let deleted = delete_context_entry_from_common_config(&updated, "mcp", "context7").unwrap();
    assert!(!deleted.contains("[mcp_servers.context7]"));
}

#[test]
fn upserts_context_entry_tolerates_duplicate_existing_context_tables() {
    let common = r#"[plugins."browser@openai-bundled"]
enabled = true

[plugins."browser@openai-bundled"]
enabled = true
"#;

    let updated = upsert_context_entry_in_common_config(
        common,
        "plugin",
        "browser@openai-bundled",
        "enabled = false",
    )
    .unwrap();

    assert_eq!(
        updated
            .matches("[plugins.\"browser@openai-bundled\"]")
            .count(),
        1
    );
    assert!(updated.contains("enabled = false"));
}

fn redacted_real_config_context_sample() -> &'static str {
    r#"model = "gpt-5.5"
model_provider = "provider-redacted-1"
approval_policy = "on-request"

[model_providers.provider-redacted-1]
name = "provider-redacted-1"
base_url = "https://example.invalid/v1"
env_key = "API_KEY_REDACTED"

[projects."project-redacted-1"]
trust_level = "trusted"
path = "<WORKSPACE>/project-1"

[projects."project-redacted-2"]
trust_level = "untrusted"
path = "<WORKSPACE>/project-2"

[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--api-key", "<MCP_API_KEY>"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "approved"

[features]
goals = true

[mcp_servers."mcp-redacted-2"]
command = "python"
args = ["<PATH>/mcp-redacted-2.py"]

[plugins."plugin-redacted-1"]
enabled = true
path = "<HOME>/.codex/plugins/plugin-redacted-1"

[plugins."plugin-redacted-2"]
enabled = false
source = "https://example.invalid/plugin-index.json"
"#
}

#[test]
fn upsert_context_entry_in_redacted_real_config_replaces_only_target_block() {
    let common = redacted_real_config_context_sample();
    let old_block = r#"[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--api-key", "<MCP_API_KEY>"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "approved""#;
    let new_block = r#"[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--mode", "safe"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "untrusted""#;
    let new_body = new_block
        .strip_prefix("[mcp_servers.\"mcp-redacted-1\"]\n")
        .unwrap();

    let updated =
        upsert_context_entry_in_common_config(common, "mcp", "mcp-redacted-1", new_body).unwrap();

    assert_eq!(
        updated,
        common.replacen(old_block, new_block, 1),
        "编辑 MCP 时只能替换选中的 MCP 块及其子表"
    );
}

#[test]
fn delete_context_entry_in_redacted_real_config_removes_only_target_block() {
    let common = redacted_real_config_context_sample();
    let old_block_with_gap = r#"[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--api-key", "<MCP_API_KEY>"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "approved"

"#;

    let deleted = delete_context_entry_from_common_config(common, "mcp", "mcp-redacted-1").unwrap();

    assert_eq!(
        deleted,
        common.replacen(old_block_with_gap, "", 1),
        "删除 MCP 时只能移除选中的 MCP 块及其子表，并保留周边配置顺序"
    );
    assert!(deleted.contains("[projects.\"project-redacted-1\"]"));
    assert!(deleted.contains("[features]"));
    assert!(deleted.contains("[mcp_servers.\"mcp-redacted-2\"]"));
    assert!(deleted.contains("[plugins.\"plugin-redacted-1\"]"));
}

#[test]
fn upsert_context_entry_in_redacted_real_config_appends_after_last_mcp_block() {
    let common = redacted_real_config_context_sample();
    let updated = upsert_context_entry_in_common_config(
        common,
        "mcp",
        "mcp-redacted-3",
        r#"command = "bunx"
args = ["<PATH>/mcp-redacted-3.js"]
"#,
    )
    .unwrap();

    let second = updated.find("[mcp_servers.\"mcp-redacted-2\"]").unwrap();
    let third = updated.find("[mcp_servers.mcp-redacted-3]").unwrap();
    let plugin = updated.find("[plugins.\"plugin-redacted-1\"]").unwrap();
    assert!(
        second < third && third < plugin,
        "新增 MCP 应插到同类最后一个 MCP 后面，而不是文件末尾：\n{updated}"
    );
    assert!(
        updated.contains("[mcp_servers.\"mcp-redacted-2\"]\ncommand = \"python\"\nargs = [\"<PATH>/mcp-redacted-2.py\"]\n\n[mcp_servers.mcp-redacted-3]\ncommand = \"bunx\"\nargs = [\"<PATH>/mcp-redacted-3.js\"]\n\n[plugins.\"plugin-redacted-1\"]"),
        "新增块与相邻配置块之间应只有一个空行：\n{updated}"
    );
}

#[test]
fn list_then_upsert_redacted_real_config_mcp_is_textually_stable() {
    let common = redacted_real_config_context_sample();
    let entries = list_context_entries_from_common_config(common).unwrap();
    let entry = entries
        .mcp_servers
        .iter()
        .find(|entry| entry.id == "mcp-redacted-1")
        .unwrap();

    let updated =
        upsert_context_entry_in_common_config(common, "mcp", &entry.id, &entry.toml_body).unwrap();

    assert_eq!(
        updated, common,
        "打开 MCP 编辑器后不修改直接保存，不应改变 config.toml 文本"
    );
}

#[test]
fn sync_live_config_context_entry_updates_only_selected_target() {
    let live = redacted_real_config_context_sample();
    let old_target = r#"[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--api-key", "<MCP_API_KEY>"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "approved""#;
    let new_target = r#"[mcp_servers."mcp-redacted-1"]
command = "node"
args = ["<PATH>/mcp-redacted-1/server.js", "--mode", "safe"]

[mcp_servers."mcp-redacted-1".env]
MCP_API_KEY = "<MCP_API_KEY>"
MCP_BASE_URL = "https://example.invalid/mcp"

[mcp_servers."mcp-redacted-1".tools."tool-redacted-1"]
approval_mode = "untrusted""#;
    let context = live.replacen(old_target, new_target, 1).replacen(
        "command = \"python\"",
        "command = \"context-python\"",
        1,
    );

    let updated = sync_live_config_context_entry(live, &context, "mcp", "mcp-redacted-1").unwrap();

    assert_eq!(
        updated,
        live.replacen(old_target, new_target, 1),
        "同步单个 MCP 时不能顺带改写其他 managed MCP"
    );
}

#[test]
fn sync_live_config_context_entry_keeps_out_of_order_context_child_blocks() {
    let live = r#"model = "gpt-5"

[features]
goals = true

[mcp_servers.alpha]
command = "old"

[mcp_servers.beta]
command = "beta"
"#;
    let context = r#"[mcp_servers.alpha.tools.read]
description = "new tool"

[mcp_servers.alpha]
command = "new"
"#;

    let updated = sync_live_config_context_entry(live, context, "mcp", "alpha").unwrap();

    assert!(updated.contains("[mcp_servers.alpha]\ncommand = \"new\""));
    assert!(updated.contains("[mcp_servers.alpha.tools.read]\ndescription = \"new tool\""));
    assert!(updated.contains("[features]\ngoals = true"));
    assert!(updated.contains("[mcp_servers.beta]\ncommand = \"beta\""));
    assert!(!updated.contains("command = \"old\""));
}

#[test]
fn sync_live_config_context_entry_deletes_out_of_order_live_child_blocks() {
    let live = r#"model = "gpt-5"

[mcp_servers.alpha.tools.read]
description = "old tool"

[features]
goals = true

[mcp_servers.alpha]
command = "old"

[mcp_servers.beta]
command = "beta"
"#;
    let context = r#"[mcp_servers.alpha]
enabled = false
command = "old"
"#;

    let updated = sync_live_config_context_entry(live, context, "mcp", "alpha").unwrap();

    assert!(!updated.contains("[mcp_servers.alpha"));
    assert!(updated.contains("[features]\ngoals = true"));
    assert!(updated.contains("[mcp_servers.beta]\ncommand = \"beta\""));
}

#[test]
fn context_entry_text_patch_does_not_absorb_following_array_tables() {
    let common = r#"[mcp_servers.alpha]
command = "old"

[[skills.config]]
name = "skill-redacted-1"
enabled = false

[plugins.local]
enabled = true
"#;

    let updated =
        upsert_context_entry_in_common_config(common, "mcp", "alpha", "command = \"new\"\n")
            .unwrap();

    assert_eq!(
        updated,
        common.replacen("command = \"old\"", "command = \"new\"", 1),
        "编辑 MCP 不能误吞后续数组表"
    );

    let deleted = delete_context_entry_from_common_config(common, "mcp", "alpha").unwrap();
    assert_eq!(
        deleted,
        r#"[[skills.config]]
name = "skill-redacted-1"
enabled = false

[plugins.local]
enabled = true
"#,
        "删除 MCP 不能删除后续数组表"
    );
}

#[test]
fn global_common_config_filters_context_by_supplier_selection() {
    let filtered = filter_common_config_for_selection(
        r#"disable_response_storage = true

[features]
goals = true

[mcp_servers.context7]
command = "npx"

[mcp_servers.memory]
command = "memory"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
        &RelayContextSelection {
            mcp_servers: vec!["memory".to_string()],
            skills: vec![],
            plugins: vec!["local".to_string()],
        },
    )
    .unwrap();

    assert!(filtered.contains("disable_response_storage = true"));
    assert!(filtered.contains("[features]"));
    assert!(filtered.contains("goals = true"));
    assert!(!filtered.contains("[mcp_servers.context7]"));
    assert!(filtered.contains("[mcp_servers.memory]"));
    assert!(!filtered.contains("[skills.writer]"));
    assert!(filtered.contains("[plugins.local]"));
}

#[test]
fn extracts_codex_common_config_without_provider_fields() {
    let extracted = extract_common_config_from_config(
        r#"model = "gpt-5"
model_provider = "custom"
base_url = "https://root-provider.example/v1"
model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
OPENAI_API_KEY = "sk-root"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true

[plugins.local]
path = "C:\\Tools\\plugin"
"#,
    )
    .unwrap();

    assert!(extracted.contains("[mcp_servers.context7]"));
    assert!(extracted.contains("[skills.writer]"));
    assert!(extracted.contains("[plugins.local]"));
    assert!(!extracted.contains("model_provider"));
    assert!(!extracted.contains("model ="));
    assert!(!extracted.contains("model_catalog_json"));
    assert!(!extracted.contains("base_url = \"https://root-provider.example/v1\""));
    assert!(extracted.contains("OPENAI_API_KEY = \"sk-root\""));
    assert!(!extracted.contains("[model_providers"));
}

#[test]
fn sanitizes_model_catalog_json_from_common_config() {
    let sanitized = sanitize_common_config_contents(
        r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_reasoning_effort = "high"

[features]
goals = true
"#,
    );

    assert!(!sanitized.contains("model_catalog_json"));
    assert!(sanitized.contains("model_reasoning_effort = \"high\""));
    assert!(sanitized.contains("[features]"));
    assert!(sanitized.contains("goals = true"));
}

#[test]
fn sanitizes_model_catalog_json_from_invalid_common_config() {
    let sanitized = sanitize_common_config_contents(
        r#"model_catalog_json = "C:\\Users\\Administrator\\.codex\\model-catalogs\\relay-a.json"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-b.json'
model_reasoning_effort = "high"
"#,
    );

    assert!(!sanitized.contains("model_catalog_json"));
    assert!(sanitized.contains("model_reasoning_effort = \"high\""));
}

#[test]
fn strips_common_config_from_provider_config_only_when_values_match() {
    let common = r#"[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = true
"#;
    let stripped = strip_common_config_from_config(
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[skills.writer]
enabled = false
"#,
        common,
    )
    .unwrap();

    assert!(stripped.contains(r#"model = "gpt-5""#));
    assert!(stripped.contains("[model_providers.custom]"));
    assert!(!stripped.contains("[mcp_servers.context7]"));
    assert!(stripped.contains("[skills.writer]"));
    assert!(stripped.contains("enabled = false"));
}

#[test]
fn apply_relay_files_with_common_preserves_mcp_skills_and_plugins() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"
[mcp_servers.old]
command = "old"
"#,
    )
    .unwrap();

    apply_relay_files_to_home_with_common(
        temp.path(),
        r#"model = "gpt-5"
model_provider = "custom"
model_catalog_json = 'C:\Users\Administrator\.codex\model-catalogs\relay-mpgm24lf.json'
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.context7]
command = "npx"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5""#));
    assert!(config.contains(r#"base_url = "https://relay.example/v1""#));
    assert!(config.contains("[mcp_servers.context7]"));
    assert!(config.contains("[skills.writer]"));
    assert!(config.contains("[plugins.local]"));
}

#[test]
fn apply_relay_files_with_context_selection_writes_only_selected_global_context() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection {
        mcp_servers: vec!["memory".to_string()],
        skills: vec![],
        plugins: vec!["local".to_string()],
    };

    codex_elves_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.context7]
command = "npx"

[mcp_servers.memory]
command = "memory"

[skills.writer]
enabled = true

[plugins.local]
path = "plugin.js"
"#,
        &selection,
        "200000",
        "160000",
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.memory]"));
    assert!(!config.contains("[mcp_servers.context7]"));
    assert!(!config.contains("[skills.writer]"));
    assert!(config.contains("[plugins.local]"));
    assert!(config.contains("model_context_window = 200000"));
    assert!(config.contains("model_auto_compact_token_limit = 160000"));
}

#[test]
fn apply_relay_files_with_context_skips_disabled_global_context() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection {
        mcp_servers: vec!["enabled_one".to_string()],
        skills: vec!["disabled_skill".to_string()],
        plugins: vec!["disabled_one".to_string(), "enabled_two".to_string()],
    };

    codex_elves_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        r#"[mcp_servers.enabled_one]
command = "npx"

[plugins.disabled_one]
enabled = false

[skills.disabled_skill]
enabled = false

[plugins.enabled_two]
enabled = true
"#,
        &selection,
        "",
        "",
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.enabled_one]"));
    assert!(config.contains("[plugins.enabled_two]"));
    assert!(!config.contains("[plugins.disabled_one]"));
    assert!(!config.contains("[skills.disabled_skill]"));
}

#[test]
fn apply_relay_profile_writes_generated_model_catalog_json_for_selected_models() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "qwen3-coder".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_reasoning_effort = "xhigh"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_mappings: vec![
            RelayModelMapping {
                request_model: "deepseek-coder".to_string(),
                protocol: RelayProtocol::ChatCompletions,
                context_window: "128000".to_string(),
            },
            RelayModelMapping {
                request_model: "qwen3-coder".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: "200000".to_string(),
            },
            RelayModelMapping {
                request_model: "glm-5.2".to_string(),
                protocol: RelayProtocol::ChatCompletions,
                context_window: "1000000".to_string(),
            },
        ],
        context_window: "200000".to_string(),
        auto_compact_limit: "160000".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "codex-elves-model-catalog.json""#));
    assert!(!config.contains("model_context_window"));
    assert!(config.contains("model_auto_compact_token_limit = 160000"));
    let catalog: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(temp.path().join("codex-elves-model-catalog.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(catalog["models"][0]["slug"], "deepseek-coder");
    assert_eq!(catalog["models"][0]["shell_type"], "shell_command");
    assert_eq!(catalog["models"][0]["apply_patch_tool_type"], "freeform");
    assert_eq!(
        catalog["models"][0]["web_search_tool_type"],
        "text_and_image"
    );
    assert_eq!(catalog["models"][0]["priority"], 1000);
    assert_eq!(
        catalog["models"][0]["input_modalities"],
        serde_json::json!(["text", "image"])
    );
    assert_eq!(
        catalog["models"][0]["truncation_policy"],
        serde_json::json!({ "mode": "tokens", "limit": 10000 })
    );
    assert!(
        catalog["models"][0]["base_instructions"]
            .as_str()
            .is_some_and(|value| value.contains("You are Codex"))
    );
    assert!(
        catalog["models"][0]["base_instructions"]
            .as_str()
            .is_some_and(|value| !value.contains("GPT-5")),
        "非 GPT 供应商模型不能继承 GPT-5 身份说明"
    );
    assert!(
        catalog["models"][0]["model_messages"]["instructions_template"]
            .as_str()
            .is_some_and(|value| value.contains("{{ personality }}"))
    );
    assert!(
        catalog["models"][0]["model_messages"]["instructions_template"]
            .as_str()
            .is_some_and(|value| !value.contains("GPT-5")),
        "非 GPT 供应商模型的模板不能继承 GPT-5 身份说明"
    );
    assert_eq!(catalog["models"][0]["context_window"], 128000);
    assert_eq!(catalog["models"][0]["auto_compact_token_limit"], 160000);
    assert_eq!(catalog["models"][0]["default_reasoning_level"], "max");
    assert_eq!(
        catalog["models"][0]["supported_reasoning_levels"],
        serde_json::json!([
            { "effort": "high", "description": "High reasoning" },
            { "effort": "max", "description": "Max reasoning" }
        ])
    );
    assert_eq!(catalog["models"][1]["slug"], "qwen3-coder");
    assert_eq!(catalog["models"][1]["context_window"], 200000);
    assert_eq!(catalog["models"][1]["default_reasoning_level"], "xhigh");
    assert_eq!(catalog["models"][2]["slug"], "glm-5.2");
    assert_eq!(catalog["models"][2]["default_reasoning_level"], "max");
    assert_eq!(
        catalog["models"][2]["supported_reasoning_levels"],
        serde_json::json!([
            { "effort": "high", "description": "High reasoning" },
            { "effort": "max", "description": "Max reasoning" }
        ])
    );
}

#[test]
fn normalize_relay_profile_writes_supports_websockets_true_for_supported_responses_provider() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        base_url: "https://relay.example".to_string(),
        upstream_base_url: "https://relay.example".to_string(),
        api_key: "sk-test".to_string(),
        responses_websocket: ResponsesWebsocketCapability {
            state: ResponsesWebsocketCapabilityState::Supported,
            endpoint: "wss://relay.example/v1/responses".to_string(),
            checked_at_ms: Some(1),
            message: "supported".to_string(),
        },
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert!(
        profile
            .config_contents
            .contains("supports_websockets = true")
    );
}

#[test]
fn normalize_relay_profile_writes_supports_websockets_false_when_system_prompt_is_overridden() {
    let mut profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        base_url: "https://relay.example".to_string(),
        upstream_base_url: "https://relay.example".to_string(),
        api_key: "sk-test".to_string(),
        system_prompt_override: "custom prompt".to_string(),
        responses_websocket: ResponsesWebsocketCapability {
            state: ResponsesWebsocketCapabilityState::Supported,
            endpoint: "wss://relay.example/v1/responses".to_string(),
            checked_at_ms: Some(1),
            message: "supported".to_string(),
        },
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
supports_websockets = true
"#
        .to_string(),
        ..RelayProfile::default()
    };

    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert!(
        profile
            .config_contents
            .contains("supports_websockets = false")
    );
    assert!(
        !profile
            .config_contents
            .contains("supports_websockets = true")
    );
}

#[test]
fn sync_applied_relay_profile_websocket_updates_provider_and_responses_model_preferences() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    std::fs::write(
        home.join("config.toml"),
        r#"model_provider = "custom"
model = "gpt-test"
model_catalog_json = "codex-elves-model-catalog.json"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
supports_websockets = true
base_url = "http://127.0.0.1:45221/v1"
"#,
    )
    .unwrap();
    std::fs::write(
        home.join("codex-elves-model-catalog.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "models": [
                {"slug": "gpt-test"},
                {"slug": "gpt-explicit", "prefer_websockets": false},
                {"slug": "claude-test"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let mut profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        protocol: RelayProtocol::Responses,
        base_url: "https://relay.example/v1".to_string(),
        upstream_base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-test".to_string(),
        config_contents: "model_provider = \"custom\"\n".to_string(),
        model_mappings: vec![
            RelayModelMapping {
                request_model: "gpt-test".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "gpt-explicit".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "claude-test".to_string(),
                protocol: RelayProtocol::Anthropic,
                context_window: String::new(),
            },
        ],
        ..RelayProfile::default()
    };

    assert!(sync_applied_relay_profile_websocket_to_home(home, &profile).unwrap());
    let disabled = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(disabled.contains("supports_websockets = false"));
    assert!(disabled.contains("model = \"gpt-test\""));
    let disabled_catalog: serde_json::Value = serde_json::from_slice(
        &std::fs::read(home.join("codex-elves-model-catalog.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        disabled_catalog["models"][0]["prefer_websockets"],
        serde_json::json!(false)
    );
    assert_eq!(
        disabled_catalog["models"][1]["prefer_websockets"],
        serde_json::json!(false)
    );
    assert!(
        disabled_catalog["models"][2]
            .get("prefer_websockets")
            .is_none()
    );

    profile.responses_websocket = ResponsesWebsocketCapability {
        state: ResponsesWebsocketCapabilityState::Supported,
        endpoint: "wss://relay.example/v1/responses".to_string(),
        checked_at_ms: Some(1),
        message: "supported".to_string(),
    };
    assert!(sync_applied_relay_profile_websocket_to_home(home, &profile).unwrap());
    let enabled = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(enabled.contains("supports_websockets = true"));
    assert!(enabled.contains("model = \"gpt-test\""));
    let enabled_catalog: serde_json::Value = serde_json::from_slice(
        &std::fs::read(home.join("codex-elves-model-catalog.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        enabled_catalog["models"][0]["prefer_websockets"],
        serde_json::json!(true)
    );
    assert_eq!(
        enabled_catalog["models"][1]["prefer_websockets"],
        serde_json::json!(true)
    );
    assert!(
        enabled_catalog["models"][2]
            .get("prefer_websockets")
            .is_none()
    );

    profile.responses_websocket_enabled = Some(false);
    assert!(sync_applied_relay_profile_websocket_to_home(home, &profile).unwrap());
    let explicitly_disabled = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(explicitly_disabled.contains("supports_websockets = false"));
    let explicitly_disabled_catalog: serde_json::Value = serde_json::from_slice(
        &std::fs::read(home.join("codex-elves-model-catalog.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        explicitly_disabled_catalog["models"][0]["prefer_websockets"],
        serde_json::json!(false)
    );
    assert_eq!(
        explicitly_disabled_catalog["models"][1]["prefer_websockets"],
        serde_json::json!(false)
    );
}

#[test]
fn apply_relay_profile_overwrites_user_model_catalog_json_with_generated_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_catalog_json = "C:\\old\\catalog.json"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        model_insert_mode: Default::default(),
        model_mappings: vec![RelayModelMapping {
            request_model: "deepseek-coder".to_string(),
            protocol: RelayProtocol::Responses,
            context_window: "64000".to_string(),
        }],
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "codex-elves-model-catalog.json""#));
    assert!(!config.contains(r#"C:\\old\\catalog.json"#));
    let catalog =
        std::fs::read_to_string(temp.path().join("codex-elves-model-catalog.json")).unwrap();
    assert!(catalog.contains("deepseek-coder"));
}

#[test]
fn apply_relay_profile_skips_common_config_when_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        use_common_config: false,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        context_selection: RelayContextSelection {
            mcp_servers: vec!["context7".to_string()],
            skills: vec![],
            plugins: vec![],
        },
        ..RelayProfile::default()
    };

    apply_relay_profile_files_to_home_with_context(
        temp.path(),
        &profile,
        r#"disable_response_storage = true

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("disable_response_storage = true"));
    assert!(!config.contains("[mcp_servers.context7]"));
}

#[test]
fn set_codex_goals_feature_writes_and_removes_feature_flag() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.4-mini"

[features]
other = true
"#,
    )
    .unwrap();

    set_codex_goals_feature_in_home(temp.path(), true).unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("goals = true"));
    assert!(config.contains("other = true"));

    set_codex_goals_feature_in_home(temp.path(), false).unwrap();
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("other = true"));
    assert!(!config.contains("goals = true"));
}

#[test]
fn set_codex_goals_feature_tolerates_invalid_existing_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"

[marketplaces.openai-bundled]
last_updated = "2026-05-25T11:52:46Z"
"#,
    )
    .unwrap();

    set_codex_goals_feature_in_home(temp.path(), true).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[features]"));
    assert!(config.contains("goals = true"));
}

#[test]
fn apply_relay_files_with_context_rejects_invalid_context_token_values() {
    let temp = tempfile::tempdir().unwrap();
    let selection = RelayContextSelection::default();

    let error = codex_elves_core::relay_config::apply_relay_files_to_home_with_context(
        temp.path(),
        r#"model_provider = "custom""#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
        "",
        &selection,
        "abc",
        "",
    )
    .unwrap_err();

    assert!(error.to_string().contains("上下文大小"));
}

#[test]
fn apply_relay_files_uses_custom_provider_id_and_updates_profile_refs() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "stable-live"
[model_providers.stable-live]
name = "stable-live"
base_url = "https://old.example/v1"
"#,
    )
    .unwrap();

    apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://new.example/v1"
experimental_bearer_token = "sk-new"

[profiles.default]
model_provider = "custom"
"#,
        r#"{"OPENAI_API_KEY":"sk-new"}"#,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"base_url = "https://new.example/v1""#));
    assert!(config.contains("[profiles.default]"));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(!config.contains("[model_providers.stable-live]"));
}

#[test]
fn backfill_relay_profile_restores_template_provider_id_from_stable_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://new.example/v1"
experimental_bearer_token = "sk-new"

[profiles.default]
model_provider = "custom"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        config_contents: r#"model_provider = "vendor_alpha"

[model_providers.vendor_alpha]
name = "vendor_alpha"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://old.example/v1"

[profiles.default]
model_provider = "vendor_alpha"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "vendor_alpha""#)
    );
    assert!(
        profile
            .config_contents
            .contains("[model_providers.vendor_alpha]")
    );
    assert!(!profile.config_contents.contains("[model_providers.custom]"));
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "vendor_alpha""#)
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
}

#[test]
fn apply_relay_files_rejects_invalid_toml_before_auth_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), "model = \"old\"\n").unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let error =
        apply_relay_files_to_home(temp.path(), "model = [", r#"{"OPENAI_API_KEY":"sk-new"}"#)
            .unwrap_err();

    assert!(error.to_string().contains("TOML"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
        "model = \"old\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
}

#[test]
fn apply_relay_files_rejects_invalid_auth_json_before_config_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("config.toml"), "model = \"old\"\n").unwrap();
    std::fs::write(temp.path().join("auth.json"), r#"{"old":true}"#).unwrap();

    let error = apply_relay_files_to_home(
        temp.path(),
        r#"model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-new"
"#,
        "{",
    )
    .unwrap_err();

    assert!(error.to_string().contains("JSON"));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("config.toml")).unwrap(),
        "model = \"old\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("auth.json")).unwrap(),
        r#"{"old":true}"#
    );
}

#[test]
fn apply_relay_config_file_switches_config_without_touching_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    std::fs::write(
        home.join("config.toml"),
        "model_provider = \"CodexElves\"\nbase_url = \"old\"\n",
    )
    .unwrap();
    std::fs::write(home.join("auth.json"), "{\"auth_mode\":\"chatgpt\"}\n").unwrap();

    let result = apply_relay_config_file_to_home(
        home,
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"http://127.0.0.1:45221/v1\"\nexperimental_bearer_token = \"sk-new\"\n",
    )
    .unwrap();

    assert!(result.configured);
    assert!(
        std::fs::read_to_string(home.join("config.toml"))
            .unwrap()
            .contains("http://127.0.0.1:45221/v1")
    );
    assert_eq!(
        std::fs::read_to_string(home.join("auth.json")).unwrap(),
        "{\"auth_mode\":\"chatgpt\"}\n"
    );
}

#[test]
fn apply_relay_config_preserves_profiles_from_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    let provider_index = updated.find(r#"model_provider = "custom""#).unwrap();
    let codexpp_index = updated.find("[model_providers.custom]").unwrap();

    assert!(provider_index < codexpp_index);
    assert!(updated.contains("[profiles.default]"));
    assert!(updated.contains(r#"model = "gpt-5""#));
}

#[test]
fn apply_relay_config_removes_legacy_codexpp_provider_table() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "CodexPP"
[model_providers.CodexPP]
name = "CodexPP"
base_url = "https://old.example.test/v1"
"#,
    )
    .unwrap();

    apply_relay_config_to_home(
        temp.path(),
        "https://relay.example.test/v1",
        "sk-test-redacted",
    )
    .unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(updated.contains(r#"model_provider = "custom""#));
    assert!(updated.contains("[model_providers.custom]"));
    assert!(!updated.contains("[model_providers.CodexPP]"));
}

#[test]
fn clear_relay_config_removes_model_provider_and_preserves_other_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-test-redacted"

[model_providers.CodexPP]
name = "CodexPP"
base_url = "https://old.example.test/v1"

[model_providers.custom1]
name = "custom1"
wire_api = "responses"
base_url = "https://keep.example.test/v1"

[profiles.default]
model = "gpt-5-mini"
"#,
    )
    .unwrap();

    let result = clear_relay_config_to_home(temp.path()).unwrap();
    let updated = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();

    assert!(!result.configured);
    assert!(
        result
            .backup_path
            .as_ref()
            .is_some_and(|path| path.contains("codex-elves-live-"))
    );
    assert!(updated.contains(r#"model = "gpt-5""#));
    assert!(!updated.contains("model_provider ="));
    assert!(!updated.contains("model_catalog_json"));
    assert!(!updated.contains("OPENAI_API_KEY"));
    assert!(!updated.contains("[model_providers.custom]"));
    assert!(!updated.contains("[model_providers.CodexPP]"));
    assert!(!updated.contains("[model_providers]\n"));
    assert!(!updated.contains("experimental_bearer_token"));
    assert!(updated.contains("[model_providers.custom1]"));
    assert!(updated.contains(r#"base_url = "https://keep.example.test/v1""#));
    assert!(updated.contains("[profiles.default]"));
}

#[test]
fn clear_relay_config_removes_pure_api_auth_json_key_and_preserves_other_auth_fields() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted","auth_mode":"chatgpt","tokens":{"access_token":"keep"}}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"
[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-test-redacted"
"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let auth_object = auth.as_object().unwrap();
    assert!(!auth_object.contains_key("OPENAI_API_KEY"));
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "keep");
}

#[test]
fn clear_relay_config_removes_openai_api_key_when_auth_json_only_contains_pure_api_key() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-test-redacted"}"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    let auth_object = auth.as_object().unwrap();
    assert!(!auth_object.contains_key("OPENAI_API_KEY"));
    assert!(auth_object.is_empty());
}

#[test]
fn clear_relay_config_with_auth_restores_official_profile_auth_json() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-relay"}"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"
[model_providers.custom]
base_url = "https://relay.example.test/v1"
experimental_bearer_token = "sk-relay"
"#,
    )
    .unwrap();

    clear_relay_config_to_home_with_auth(
        temp.path(),
        Some(r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official-edited"}}"#),
    )
    .unwrap();

    let auth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(temp.path().join("auth.json")).unwrap())
            .unwrap();
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official-edited");
    assert!(auth.get("OPENAI_API_KEY").is_none());
}

#[test]
fn backfill_relay_profile_reads_live_files_and_model() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        "model = \"gpt-5\"\nmodel_provider = \"live\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();

    backfill_relay_profile_from_home(temp.path(), &mut profile).unwrap();

    assert_eq!(profile.model, "gpt-5");
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "live""#)
    );
    assert_eq!(profile.auth_contents, r#"{"OPENAI_API_KEY":"sk-live"}"#);
}

#[test]
fn backfill_relay_profile_reads_live_context_limits() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "mimo-v2.5-pro"
model_provider = "custom"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.custom]
base_url = "http://127.0.0.1:45221/v1"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();

    backfill_relay_profile_from_home(temp.path(), &mut profile).unwrap();

    assert_eq!(profile.context_window, "1000000");
    assert_eq!(profile.auto_compact_limit, "900000");
    assert!(
        profile
            .config_contents
            .contains("model_context_window = 1000000")
    );
    assert!(
        profile
            .config_contents
            .contains("model_auto_compact_token_limit = 900000")
    );
}

#[test]
fn backfill_relay_profile_with_common_strips_common_config_for_switching() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "live"
[model_providers.live]
base_url = "https://relay.example/v1"

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[mcp_servers.context7]
command = "npx"
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert_eq!(profile.model, "gpt-5");
    assert!(!profile.config_contents.contains("[mcp_servers.context7]"));
    assert!(
        profile
            .config_contents
            .contains(r#"model_provider = "live""#)
    );
    assert_eq!(profile.auth_contents, r#"{"OPENAI_API_KEY":"sk-live"}"#);
}

#[test]
fn backfill_relay_profile_with_common_reads_live_context_limits() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "mimo-v2.5-pro"
model_provider = "custom"
model_context_window = 1000000
model_auto_compact_token_limit = 900000

[model_providers.custom]
base_url = "http://127.0.0.1:45221/v1"

[mcp_servers.context7]
command = "npx"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[mcp_servers.context7]
command = "npx"
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    apply_relay_profile_files_to_home_with_context(temp.path(), &profile, &common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert_eq!(profile.context_window, "1000000");
    assert_eq!(profile.auto_compact_limit, "900000");
    assert!(config.contains("model_context_window = 1000000"));
    assert!(config.contains("model_auto_compact_token_limit = 900000"));
}

#[test]
fn backfill_relay_profile_with_common_tolerates_duplicate_live_toml() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.5"
model_reasoning_effort = "high"
model_provider = "aaa"
model_reasoning_effort = "high"

[model_providers.aaa]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-live-token"

[marketplaces.openai-bundled]
last_updated = "new"

[marketplaces.openai-bundled]
last_updated = "old"

[plugins."superpowers@openai-curated"]
enabled = true
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = r#"[plugins."superpowers@openai-curated"]
enabled = true
"#
    .to_string();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert_eq!(profile.model, "gpt-5.5");
    assert!(
        profile
            .config_contents
            .contains(r#"model_reasoning_effort = "high""#)
    );
    assert_eq!(
        profile
            .config_contents
            .matches("model_reasoning_effort")
            .count(),
        1
    );
    assert_eq!(
        profile
            .config_contents
            .matches("[marketplaces.openai-bundled]")
            .count(),
        1
    );
    assert!(
        !profile
            .config_contents
            .contains("[plugins.\"superpowers@openai-curated\"]")
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live-token");
}

#[test]
fn backfill_relay_profile_with_common_lifts_bearer_token_to_auth() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "live"
[model_providers.live]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-live-token"
"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(
        !profile
            .config_contents
            .contains("experimental_bearer_token")
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live-token");
}

#[test]
fn backfill_relay_profile_prefers_live_auth_over_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-old"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-edited"}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        relay_mode: RelayMode::PureApi,
        auth_contents: r#"{"OPENAI_API_KEY":"sk-old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-edited");
    assert!(
        !profile
            .config_contents
            .contains("experimental_bearer_token")
    );
}

#[test]
fn apply_relay_profile_preserves_provider_specific_id_in_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut provider_b = RelayProfile {
        id: "provider-b".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "aihubmix"
model = "gpt-5.4"
profile = "work"

[model_providers.aihubmix]
name = "AiHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
requires_openai_auth = true

[profiles.work]
model_provider = "aihubmix"
model = "gpt-5.4"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"aihubmix-key"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &provider_b, "").unwrap();
    let live_config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(live_config.contains(r#"model_provider = "aihubmix""#));
    assert!(live_config.contains("[model_providers.aihubmix]"));
    assert!(!live_config.contains("[model_providers.custom]"));

    let mut common = String::new();
    backfill_relay_profile_from_home_with_common(temp.path(), &mut provider_b, &mut common)
        .unwrap();

    assert!(
        provider_b
            .config_contents
            .contains(r#"model_provider = "aihubmix""#)
    );
    assert!(
        provider_b
            .config_contents
            .contains("[model_providers.aihubmix]")
    );
    assert!(provider_b.config_contents.contains(r#"name = "aihubmix""#));
    assert!(
        provider_b
            .config_contents
            .contains(r#"model_provider = "aihubmix""#)
    );
    assert!(
        !provider_b
            .config_contents
            .contains("[model_providers.custom]")
    );
    let auth: serde_json::Value = serde_json::from_str(&provider_b.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "aihubmix-key");
    assert!(auth.get("tokens").is_none());
}

#[test]
fn backfill_current_profile_preserves_external_live_provider_id_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_provider = "manual_edit"
model = "gpt-5.4"

[model_providers.manual_edit]
name = "Manual Edit"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-live"}"#,
    )
    .unwrap();

    let mut current = RelayProfile {
        id: "provider-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "old_snapshot"
model = "gpt-5.4"

[model_providers.old_snapshot]
name = "Old Snapshot"
base_url = "https://old.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-old"}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();

    assert!(
        current
            .config_contents
            .contains(r#"model_provider = "manual_edit""#)
    );
    assert!(
        current
            .config_contents
            .contains("[model_providers.manual_edit]")
    );
    assert!(current.config_contents.contains(r#"name = "Manual Edit""#));
    assert!(!current.config_contents.contains("old_snapshot"));
    let auth: serde_json::Value = serde_json::from_str(&current.auth_contents).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-live");
}

#[test]
fn backfill_official_profile_promotes_external_pure_api_live_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "manual_api"

[model_providers.manual_api]
name = "Manual API"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-manual"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_promotes_external_official_mix_live_edit_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "manual_mix"

[model_providers.manual_mix]
name = "Manual Mix"
base_url = "https://manual.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-mix"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"old"}}"#.to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_does_not_promote_codex_elves_switch_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "deepseek-chat"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://third-party.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third-party"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_profile_does_not_promote_custom_numbered_live_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5.5"
model_provider = "custom1"

[model_providers.custom1]
name = "custom1"
base_url = "https://third-party.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-third-party"}"#,
    )
    .unwrap();
    let mut current = RelayProfile {
        id: "official".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: false,
        config_contents: String::new(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut current, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut current).unwrap();

    assert_eq!(current.relay_mode, RelayMode::Official);
    assert!(!current.official_mix_api_key);
    assert!(current.config_contents.is_empty());
    assert!(current.api_key.is_empty());
    assert!(!current.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn backfill_official_mix_profile_keeps_key_after_switch_roundtrip_storage() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-saved-mix"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-saved-mix"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.relay_mode, RelayMode::Official);
    assert!(profile.official_mix_api_key);
    assert_eq!(profile.api_key, "sk-saved-mix");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "sk-saved-mix""#)
    );
    let auth: serde_json::Value = serde_json::from_str(&profile.auth_contents).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["tokens"]["access_token"], "official");
}

#[test]
fn backfill_official_mix_profile_keeps_mix_mode_when_live_auth_has_api_key() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "333333333333333333333"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"333333333333333333333","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "22222222222222222222222222222222222"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();
    normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.relay_mode, RelayMode::Official);
    assert!(profile.official_mix_api_key);
    assert_eq!(profile.api_key, "333333333333333333333");
    assert!(
        profile
            .config_contents
            .contains(r#"experimental_bearer_token = "333333333333333333333""#)
    );
    assert!(!profile.auth_contents.contains("OPENAI_API_KEY"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_switches_auth_and_writes_provider_token() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "qwen3-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
    assert!(auth.get("auth_mode").is_none());
    assert!(auth.get("tokens").is_none());
    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("experimental_bearer_token"));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_repairs_incomplete_provider_config() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "live-model"
model_provider = "live_provider"

[model_providers.live_provider]
base_url = "https://live.example/v1"
experimental_bearer_token = "sk-live"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        model: "qwen3-coder".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"[model_providers.custom]
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "qwen3-coder""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(config.contains(r#"base_url = "https://relay.example/v1""#));
    assert!(config.contains("[model_providers.live_provider]"));
    assert!(config.contains("https://live.example/v1"));
    let custom_start = config.find("[model_providers.custom]").unwrap();
    let custom_section = &config[custom_start..];
    assert!(!custom_section.contains("experimental_bearer_token"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_uses_struct_fields_and_ignores_extra_config_contents()
 {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"
disable_response_storage = true

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5.4""#));
    assert!(!config.contains("disable_response_storage = true"));
    assert!(config.contains(r#"model_provider = "max_ai""#));
    assert!(config.contains("[model_providers.max_ai]"));
    assert!(config.contains(r#"name = "max_ai""#));
    assert!(config.contains(r#"base_url = "https://max2.jojocode.com/v1""#));
    assert!(!config.contains("experimental_bearer_token"));
    assert!(!config.contains("[model_providers.custom]"));
}

#[cfg(windows)]
#[test]
fn apply_relay_profile_to_home_with_switch_rules_does_not_preserve_computer_use_guard_config_by_default()
 {
    let temp = tempfile::tempdir().unwrap();
    let helper = temp
        .path()
        .join("plugins")
        .join("cache")
        .join("openai-bundled")
        .join("computer-use")
        .join("26.608.12217")
        .join("node_modules")
        .join("@oai")
        .join("sky")
        .join("bin")
        .join("windows")
        .join("codex-computer-use.exe");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, "").unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"

[features]
js_repl = false

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("js_repl = false"));
    assert!(!config.contains("[plugins.\"browser@openai-bundled\"]"));
    assert!(!config.contains("[plugins.\"chrome@openai-bundled\"]"));
    assert!(!config.contains("[plugins.\"computer-use@openai-bundled\"]"));
    assert!(!config.contains(r#"notify = ["#));
    assert!(!config.contains("codex-computer-use.exe"));
}

#[cfg(windows)]
#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_computer_use_guard_config_when_enabled()
{
    let temp = tempfile::tempdir().unwrap();
    let helper = temp
        .path()
        .join("plugins")
        .join("cache")
        .join("openai-bundled")
        .join("computer-use")
        .join("26.608.12217")
        .join("node_modules")
        .join("@oai")
        .join("sky")
        .join("bin")
        .join("windows")
        .join("codex-computer-use.exe");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, "").unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "max_ai"
model = "gpt-5.4"

[features]
js_repl = false

[model_providers.max_ai]
name = "max_ai"
base_url = "https://max2.jojocode.com/v1"
wire_api = "responses"
requires_openai_auth = true
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        temp.path(),
        &profile,
        "",
        true,
    )
    .unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("js_repl = true"));
    assert!(config.contains("[plugins.\"browser@openai-bundled\"]"));
    assert!(config.contains("[plugins.\"chrome@openai-bundled\"]"));
    assert!(config.contains("[plugins.\"computer-use@openai-bundled\"]"));
    assert!(config.contains(r#"notify = ["#));
    assert!(config.contains("codex-computer-use.exe"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_unmanaged_live_context_entries() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"

[mcp_servers.manual]
command = "manual-command"

[plugins.manual]
enabled = true
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    let common = r#"[mcp_servers.managed]
command = "managed-command"
"#;

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.manual]"));
    assert!(config.contains(r#"command = "manual-command""#));
    assert!(config.contains("[plugins.manual]"));
    assert!(!config.contains("[mcp_servers.managed]"));
    assert!(!config.contains(r#"command = "managed-command""#));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_live_config_text_order() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"

[mcp_servers.live]
command = "old-live-command"
"#,
    )
    .unwrap();
    let config_contents = r#"# supplier-owned order must stay stable
model_provider = "custom"
model = "gpt-5.5"

[model_providers.custom]
base_url = "https://relay.example/v1"
requires_openai_auth = true
wire_api = "responses"
name = "custom"

[mcp_servers.supplier]
command = "supplier-command"
"#;
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: config_contents.to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.live]"));
    assert!(config.contains(r#"command = "old-live-command""#));
    assert!(!config.contains("[mcp_servers.supplier]"));
    assert!(config.contains(r#"model = "gpt-5.5""#));
    assert!(config.contains("[model_providers.custom]"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_patches_owned_fields_only() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"# user-owned heading
approval_policy = "never"
model = "old-model"
model_provider = "old_provider"
custom_user_key = "keep-me"

[mcp_servers.keep]
command = "node"

[model_providers.old_provider]
name = "old"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://old.example/v1"

[features]
goals = true
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        model: "qwen3-coder".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        upstream_base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"

[mcp_servers.js_repl]
command = "should-not-be-added"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-old"}"#.to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("# user-owned heading"));
    assert!(config.contains(r#"approval_policy = "never""#));
    assert!(config.contains(r#"custom_user_key = "keep-me""#));
    assert!(config.contains("[mcp_servers.keep]"));
    assert!(!config.contains("js_repl"));
    assert!(config.contains(r#"model = "qwen3-coder""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.old_provider]"));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"base_url = "https://relay.example/v1""#));
    assert!(!config.contains("experimental_bearer_token"));
    assert!(
        config.find("[mcp_servers.keep]").unwrap() < config.find("[features]").unwrap(),
        "用户原有 section 顺序应保持：\n{config}"
    );
    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    assert!(auth.contains(r#""OPENAI_API_KEY": "sk-new""#));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_patches_generated_model_catalog_only_when_owned() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_catalog_json = "user-catalog.json"
model_provider = "custom"

[mcp_servers.keep]
command = "node"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://old.example/v1"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        name: "Relay A".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        upstream_base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        model_mappings: vec![RelayModelMapping {
            request_model: "qwen3-coder".to_string(),
            protocol: RelayProtocol::Responses,
            context_window: "200000".to_string(),
        }],
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "codex-elves-model-catalog.json""#));
    assert!(!config.contains("user-catalog.json"));
    assert!(
        config.find("model_catalog_json").unwrap() < config.find("[mcp_servers.keep]").unwrap(),
        "root key 只应原位替换，不应重排 section：\n{config}"
    );
    let catalog =
        std::fs::read_to_string(temp.path().join("codex-elves-model-catalog.json")).unwrap();
    assert!(catalog.contains("qwen3-coder"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_preserves_user_model_catalog_when_unowned() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model_catalog_json = "user-catalog.json"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://old.example/v1"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        base_url: "https://relay.example/v1".to_string(),
        upstream_base_url: "https://relay.example/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model_catalog_json = "user-catalog.json""#));
    assert!(!temp.path().join("codex-elves-model-catalog.json").exists());
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_does_not_sync_unselected_managed_context_entries()
{
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "old"

[mcp_servers.manual]
command = "manual-command"

[mcp_servers.managed]
command = "old-managed"
"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-a".to_string(),
        relay_mode: RelayMode::PureApi,
        context_selection_initialized: true,
        context_selection: RelayContextSelection::default(),
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    let common = r#"[mcp_servers.managed]
command = "managed-command"
"#;

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, common).unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains("[mcp_servers.manual]"));
    assert!(config.contains("[mcp_servers.managed]"));
    assert!(config.contains(r#"command = "old-managed""#));
    assert!(!config.contains(r#"command = "managed-command""#));
}

#[test]
fn filter_common_config_for_selection_writes_only_selected_context_entries() {
    let common = r#"model_reasoning_effort = "high"

[mcp_servers.keep]
command = "keep"

[mcp_servers.skip]
command = "skip"

[skills.writer]
enabled = true

[plugins.browser]
enabled = true
"#;
    let selection = RelayContextSelection {
        mcp_servers: vec!["keep".to_string()],
        skills: Vec::new(),
        plugins: vec!["browser".to_string()],
    };

    let filtered = filter_common_config_for_selection(common, &selection).unwrap();

    assert!(filtered.contains("model_reasoning_effort"));
    assert!(filtered.contains("[mcp_servers.keep]"));
    assert!(!filtered.contains("[mcp_servers.skip]"));
    assert!(!filtered.contains("[skills.writer]"));
    assert!(filtered.contains("[plugins.browser]"));
}

#[test]
fn sync_live_config_context_entries_preserves_unmanaged_live_entries() {
    let live = r#"model = "gpt-5"

[mcp_servers.manual]
command = "manual"

[mcp_servers.managed]
command = "old"
"#;
    let context = r#"[mcp_servers.managed]
command = "new"

[mcp_servers.disabled]
enabled = false
command = "disabled"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("[mcp_servers.manual]"));
    assert!(updated.contains(r#"command = "manual""#));
    assert!(updated.contains("[mcp_servers.managed]"));
    assert!(updated.contains(r#"command = "new""#));
    assert!(!updated.contains("[mcp_servers.disabled]"));
}

#[test]
fn sync_live_config_context_entries_removes_disabled_managed_entries_from_live() {
    let live = r#"model = "gpt-5"

[mcp_servers.manual]
command = "manual"

[mcp_servers.managed]
command = "old"
"#;
    let context = r#"[mcp_servers.managed]
enabled = false
command = "old"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("[mcp_servers.manual]"));
    assert!(!updated.contains("[mcp_servers.managed]"));
}

#[test]
fn sync_live_config_context_entries_replaces_only_target_block_text() {
    let live = r#"# keep this heading
model = "gpt-5"

[mcp_servers.alpha]
command = "old"

[features]
goals = true

[mcp_servers.beta]
command = "beta"
"#;
    let context = r#"[mcp_servers.alpha]
command = "new"
args = ["--fresh"]
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("# keep this heading"));
    assert!(updated.contains("[mcp_servers.alpha]\ncommand = \"new\"\nargs = [\"--fresh\"]"));
    assert!(updated.contains("[features]\ngoals = true"));
    assert!(updated.contains("[mcp_servers.beta]\ncommand = \"beta\""));
    assert!(!updated.contains("command = \"old\""));
    assert!(
        updated.find("[mcp_servers.alpha]").unwrap() < updated.find("[features]").unwrap(),
        "existing block position should not drift:\n{updated}"
    );
    assert!(
        updated.find("[features]").unwrap() < updated.find("[mcp_servers.beta]").unwrap(),
        "unrelated sections should keep their order:\n{updated}"
    );
}

#[test]
fn sync_live_config_context_entries_appends_after_last_same_kind_block() {
    let live = r#"model = "gpt-5"

[mcp_servers.alpha]
command = "alpha"

[features]
goals = true

[mcp_servers.beta]
command = "beta"

[plugins.local]
enabled = true
"#;
    let context = r#"[mcp_servers.gamma]
command = "gamma"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    let beta = updated.find("[mcp_servers.beta]").unwrap();
    let gamma = updated.find("[mcp_servers.gamma]").unwrap();
    let plugin = updated.find("[plugins.local]").unwrap();
    assert!(
        beta < gamma,
        "new mcp should follow last existing mcp:\n{updated}"
    );
    assert!(
        gamma < plugin,
        "new mcp should not be appended to file end:\n{updated}"
    );
    assert!(
        updated.contains("[mcp_servers.beta]\ncommand = \"beta\"\n\n[mcp_servers.gamma]\ncommand = \"gamma\"\n\n[plugins.local]"),
        "context blocks should be separated by one blank line:\n{updated}"
    );
}

#[test]
fn sync_live_config_context_entries_deletes_target_block_with_children_only() {
    let live = r#"model = "gpt-5"

[mcp_servers.keep]
command = "keep"

[mcp_servers.drop]
command = "drop"

[mcp_servers.drop.env]
TOKEN = "old"

[plugins.local]
enabled = true
"#;
    let context = r#"[mcp_servers.drop]
enabled = false
command = "drop"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(updated.contains("[mcp_servers.keep]"));
    assert!(!updated.contains("[mcp_servers.drop]"));
    assert!(!updated.contains("[mcp_servers.drop.env]"));
    assert!(updated.contains("[plugins.local]"));
    assert!(
        updated.contains("[mcp_servers.keep]\ncommand = \"keep\"\n\n[plugins.local]"),
        "delete should leave a single blank line between adjacent blocks:\n{updated}"
    );
}

#[test]
fn sync_live_config_context_entries_replaces_out_of_order_child_and_root_blocks() {
    let live = r#"model = "gpt-5"

[mcp_servers.alpha.tools.read]
description = "old tool"

[features]
goals = true

[mcp_servers.alpha]
command = "old"

[mcp_servers.beta]
command = "beta"
"#;
    let context = r#"[mcp_servers.alpha]
command = "new"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(!updated.contains("[mcp_servers.alpha.tools.read]"));
    assert!(!updated.contains("description = \"old tool\""));
    assert!(!updated.contains("command = \"old\""));
    assert_eq!(updated.matches("[mcp_servers.alpha]").count(), 1);
    assert!(
        updated.contains("[features]\ngoals = true\n\n[mcp_servers.alpha]\ncommand = \"new\"\n\n[mcp_servers.beta]"),
        "replacement should use the root block position and keep TOML valid:\n{updated}"
    );
}

#[test]
fn sync_live_config_context_entries_deletes_out_of_order_child_and_root_blocks() {
    let live = r#"model = "gpt-5"

[mcp_servers.alpha.tools.read]
description = "old tool"

[features]
goals = true

[mcp_servers.alpha]
command = "old"

[mcp_servers.beta]
command = "beta"
"#;
    let context = r#"[mcp_servers.alpha]
enabled = false
command = "old"
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(!updated.contains("[mcp_servers.alpha"));
    assert!(updated.contains("[features]\ngoals = true"));
    assert!(updated.contains("[mcp_servers.beta]\ncommand = \"beta\""));
}

#[test]
fn sync_live_config_context_entries_matches_quoted_ids_semantically() {
    let live = r#"model = "gpt-5"

[plugins.'browser@openai-bundled']
enabled = true
"#;
    let context = r#"[plugins."browser@openai-bundled"]
enabled = false
"#;

    let updated = sync_live_config_context_entries(live, context).unwrap();

    assert!(!updated.contains("browser@openai-bundled"));
    assert_eq!(updated, "model = \"gpt-5\"\n");
}

#[test]
fn sync_live_config_context_entries_rejects_invalid_live_without_patch() {
    let live = r#"model = "gpt-5"

[mcp_servers.alpha]
command = "old"

[mcp_servers.alpha]
command = "duplicate"
"#;
    let context = r#"[mcp_servers.alpha]
command = "new"
"#;

    let error = sync_live_config_context_entries(live, context).unwrap_err();

    assert!(error.to_string().contains("config.toml TOML 解析失败"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_writes_provider_even_when_auth_has_no_api_key() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "relay-empty-auth".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers]

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
"#
        .to_string(),
        auth_contents: "{}".to_string(),
        ..RelayProfile::default()
    };

    let error = apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "")
        .expect_err("缺少 API Key 时不应写入供应商配置");

    assert!(error.to_string().contains("API Key"));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_switches_auth_even_when_provider_token_exists() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "relay-provider-token".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
experimental_bearer_token = "sk-provider-token"
"#
        .to_string(),
        auth_contents: "{}".to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-provider-token");

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!config.contains("experimental_bearer_token"));
}

#[test]
fn apply_official_mix_profile_clears_live_auth_api_key_and_keeps_login() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-pure-api","auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-official-mix"
"#
        .to_string(),
        auth_contents: r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#
            .to_string(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
    assert!(auth.get("OPENAI_API_KEY").is_none());
    assert_eq!(auth["auth_mode"], "chatgpt");
    assert_eq!(auth["tokens"]["access_token"], "official");

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"experimental_bearer_token = "sk-official-mix""#));
    assert!(config.contains("requires_openai_auth = true"));
}

#[test]
fn apply_official_mix_profile_keeps_config_token_when_api_key_field_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let profile = RelayProfile {
        id: "official-mix".to_string(),
        relay_mode: RelayMode::Official,
        official_mix_api_key: true,
        config_contents: r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-from-config"
"#
        .to_string(),
        auth_contents: String::new(),
        api_key: String::new(),
        ..RelayProfile::default()
    };

    apply_relay_profile_to_home_with_switch_rules(temp.path(), &profile, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"experimental_bearer_token = "sk-from-config""#));
    let auth = std::fs::read_to_string(temp.path().join("auth.json")).unwrap();
    assert!(auth.trim().is_empty());
}

#[test]
fn strip_common_config_with_duplicate_context_tables_preserves_provider_config() {
    let config = r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://192.168.188.245:3001/v1"
"#;
    let common = r#"model_reasoning_effort = "high"

[mcp_servers]

[plugins."documents@openai-primary-runtime"]
enabled = true

[mcp_servers]

[mcp_servers.ida-pro-mcp]
command = "python"
"#;

    let stripped = strip_common_config_from_config(config, common).unwrap();

    assert!(stripped.contains(r#"model = "gpt-5.5""#));
    assert!(stripped.contains(r#"model_provider = "custom""#));
    assert!(stripped.contains("[model_providers.custom]"));
    assert!(stripped.contains(r#"base_url = "http://192.168.188.245:3001/v1""#));
}

#[test]
fn apply_relay_profile_to_home_with_switch_rules_survives_official_roundtrip() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://old.example/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "sk-old"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-old","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();

    clear_relay_config_to_home(temp.path()).unwrap();
    let mut official = RelayProfile {
        relay_mode: RelayMode::Official,
        use_common_config: true,
        ..RelayProfile::default()
    };
    let mut common = String::new();
    backfill_relay_profile_from_home_with_common(temp.path(), &mut official, &mut common).unwrap();

    let mut relay = RelayProfile {
        id: "relay-a".to_string(),
        model: "gpt-5.4".to_string(),
        base_url: "https://max2.jojocode.com/v1".to_string(),
        api_key: "sk-new".to_string(),
        relay_mode: RelayMode::PureApi,
        config_contents: r#"[model_providers.custom]
experimental_bearer_token = "sk-new"
"#
        .to_string(),
        auth_contents: r#"{"OPENAI_API_KEY":"sk-new"}"#.to_string(),
        ..RelayProfile::default()
    };
    normalize_relay_profile_for_storage(&mut relay).unwrap();
    apply_relay_profile_to_home_with_switch_rules(temp.path(), &relay, "").unwrap();

    let config = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(config.contains(r#"model = "gpt-5.4""#));
    assert!(config.contains(r#"model_provider = "custom""#));
    assert!(config.contains("[model_providers.custom]"));
    assert!(config.contains(r#"name = "custom""#));
    assert!(config.contains(r#"base_url = "https://max2.jojocode.com/v1""#));
    assert!(config.contains(r#"wire_api = "responses""#));
    assert!(config.contains("requires_openai_auth = true"));
    assert!(!config.contains("experimental_bearer_token"));
}

#[test]
fn backfill_relay_profile_from_official_config_without_model_providers_does_not_panic() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.toml"),
        r#"model = "gpt-5"

[features]
goals = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"official"}}"#,
    )
    .unwrap();
    let mut profile = RelayProfile::default();
    let mut common = String::new();

    backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common).unwrap();

    assert!(profile.config_contents.contains(r#"model = "gpt-5""#));
    assert!(!profile.auth_contents.is_empty());
}

fn base64_url_no_pad(value: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.as_bytes())
}
