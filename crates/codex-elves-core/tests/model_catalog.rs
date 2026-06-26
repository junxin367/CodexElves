use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use codex_elves_core::model_catalog::{
    read_codex_model_catalog, read_codex_model_catalog_from_home,
    read_configured_model_catalog_model_ids_from_home, relay_profile_model_ids_for_proxy,
};
use codex_elves_core::settings::{RelayModelMapping, RelayProfile, RelayProtocol};
use serde_json::json;

#[tokio::test]
async fn model_catalog_does_not_fetch_models_from_codex_config_provider() {
    let temp = tempfile::tempdir().unwrap();
    write_config(
        temp.path(),
        r#"
model = "qwen3-coder"
model_provider = "relay"

[model_providers.relay]
name = "Relay"
base_url = "http://127.0.0.1:9/v1"
experimental_bearer_token = "relay-key"
"#,
    );

    let result = read_codex_model_catalog_from_home(
        temp.path(),
        &HashMap::new(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
    )
    .await;

    assert_eq!(result["status"], "not_configured");
    assert_eq!(result["model_provider"], "relay");
    assert_eq!(result["provider_name"], "Relay");
    assert_eq!(result["default_model"], "");
    assert_eq!(result["models"], json!([]));
    assert_eq!(result["sources"], json!([]));
    assert_eq!(
        result["responses_api"],
        json!({
            "status": "unknown",
            "endpoint": "",
            "message": ""
        })
    );
}

#[tokio::test]
async fn model_catalog_reads_codex_home_catalog_instead_of_settings_profile() {
    let _lock = model_catalog_env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let codex_home = temp.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    let previous_codex_home = std::env::var_os("CODEX_HOME");
    unsafe {
        std::env::set_var("CODEX_HOME", &codex_home);
    }
    std::fs::write(
        codex_home.join("codex-elves-model-catalog.json"),
        json!({
            "models": [
                {
                    "slug": "catalog-first",
                    "visibility": "list",
                    "supported_in_api": true,
                    "context_window": 200000
                },
                {
                    "slug": "catalog-second",
                    "visibility": "list",
                    "supported_in_api": true
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    write_config(
        &codex_home,
        r#"
model = "catalog-second"
model_provider = "relay-a"
model_catalog_json = "codex-elves-model-catalog.json"

[model_providers.relay-a]
name = "Relay A"
base_url = "http://127.0.0.1:9/v1"
"#,
    );

    let result = read_codex_model_catalog().await;

    match previous_codex_home {
        Some(value) => unsafe {
            std::env::set_var("CODEX_HOME", value);
        },
        None => unsafe {
            std::env::remove_var("CODEX_HOME");
        },
    }

    assert_eq!(result["status"], "ok");
    assert_eq!(result["model_provider"], "relay-a");
    assert_eq!(result["provider_name"], "Relay A");
    assert_eq!(result["default_model"], "catalog-second");
    assert_eq!(result["models"], json!(["catalog-first", "catalog-second"]));
    assert_eq!(result["model_entries"][0]["context_window"], 200000);
    assert_eq!(result["sources"][0]["type"], "model_catalog_json");
}

#[test]
fn relay_profile_model_ids_preserve_mapping_order_for_catalog_generation() {
    let profile = RelayProfile {
        model_mappings: vec![
            RelayModelMapping {
                request_model: "gpt-responses".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "gpt-chat".to_string(),
                protocol: RelayProtocol::ChatCompletions,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "gpt-shared".to_string(),
                protocol: RelayProtocol::Responses,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "gpt-chat".to_string(),
                protocol: RelayProtocol::Anthropic,
                context_window: String::new(),
            },
            RelayModelMapping {
                request_model: "claude-sonnet-4".to_string(),
                protocol: RelayProtocol::Anthropic,
                context_window: String::new(),
            },
        ],
        ..RelayProfile::default()
    };

    assert_eq!(
        relay_profile_model_ids_for_proxy(&profile),
        vec![
            "gpt-responses".to_string(),
            "gpt-chat".to_string(),
            "gpt-shared".to_string(),
            "claude-sonnet-4".to_string()
        ]
    );
}

fn model_catalog_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test]
async fn model_catalog_uses_single_provider_when_root_model_provider_is_absent() {
    let temp = tempfile::tempdir().unwrap();
    write_config(
        temp.path(),
        r#"
[model_providers.only]
name = "Only Provider"
base_url = "http://127.0.0.1:9/v1"
"#,
    );

    let result = read_codex_model_catalog_from_home(
        temp.path(),
        &HashMap::new(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
    )
    .await;

    assert_eq!(result["status"], "not_configured");
    assert_eq!(result["model_provider"], "only");
    assert_eq!(result["models"], json!([]));
    assert_eq!(result["sources"], json!([]));
    assert_eq!(result["responses_api"]["status"], "unknown");
}

#[tokio::test]
async fn model_catalog_reads_models_from_config_model_catalog_json_only() {
    let temp = tempfile::tempdir().unwrap();
    let catalog_path = temp.path().join("custom-models.json");
    std::fs::write(
        &catalog_path,
        json!({
            "models": [
                {
                    "slug": "gpt-5.6",
                    "display_name": "GPT-5.6",
                    "visibility": "list",
                    "supported_in_api": true
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    write_config(
        temp.path(),
        &format!(
            r#"
model = "gpt-5.6"
model_provider = "relay"
model_catalog_json = "{}"

[model_providers.relay]
name = "Relay"
base_url = "http://127.0.0.1:9/v1"
experimental_bearer_token = "relay-key"
"#,
            catalog_path.display().to_string().replace('\\', "\\\\"),
        ),
    );

    let result = read_codex_model_catalog_from_home(
        temp.path(),
        &HashMap::new(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
    )
    .await;

    assert_eq!(result["status"], "ok");
    assert_eq!(result["default_model"], "gpt-5.6");
    assert_eq!(result["models"], json!(["gpt-5.6"]));
    assert_eq!(result["model_entries"][0]["display_name"], "GPT-5.6");
}

#[tokio::test]
async fn model_catalog_reads_single_quoted_config_model_catalog_json_path() {
    let temp = tempfile::tempdir().unwrap();
    let catalog_path = temp.path().join("literal-path-models.json");
    std::fs::write(
        &catalog_path,
        json!({
            "models": [
                {
                    "slug": "gpt-5.6",
                    "visibility": "list",
                    "supported_in_api": true
                },
                {
                    "slug": "hidden-test-model",
                    "visibility": "hidden",
                    "supported_in_api": true
                },
                {
                    "slug": "chatgpt-only-test-model",
                    "visibility": "list",
                    "supported_in_api": false
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    write_config(
        temp.path(),
        &format!(
            r#"
model = "gpt-5.6"
model_catalog_json = '{}'
"#,
            catalog_path.display()
        ),
    );

    let result = read_codex_model_catalog_from_home(
        temp.path(),
        &HashMap::new(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
    )
    .await;

    assert_eq!(result["status"], "ok");
    assert_eq!(result["default_model"], "gpt-5.6");
    assert_eq!(result["models"], json!(["gpt-5.6"]));
    assert_eq!(result["sources"][0]["status"], "ok");
    assert_eq!(result["sources"][0]["models"], 1);
    assert_eq!(
        read_configured_model_catalog_model_ids_from_home(temp.path()),
        vec!["gpt-5.6".to_string()]
    );
}

#[tokio::test]
async fn model_catalog_leaves_responses_api_unknown_without_probe() {
    let temp = tempfile::tempdir().unwrap();
    let catalog_path = temp.path().join("legacy-models.json");
    std::fs::write(
        &catalog_path,
        json!({
            "models": [
                {
                    "slug": "legacy-model",
                    "visibility": "list",
                    "supported_in_api": true
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    write_config(
        temp.path(),
        &format!(
            r#"
model = "legacy-model"
model_catalog_json = "{}"

[model_providers.legacy]
name = "Legacy"
base_url = "http://127.0.0.1:9/v1"
"#,
            catalog_path.display().to_string().replace('\\', "\\\\"),
        ),
    );

    let result = read_codex_model_catalog_from_home(
        temp.path(),
        &HashMap::new(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
    )
    .await;

    assert_eq!(result["status"], "ok");
    assert_eq!(result["responses_api"]["status"], "unknown");
    assert_eq!(result["responses_api"]["endpoint"], "");
    assert_eq!(result["sources"][0]["responses_api"]["status"], "unknown");
}

fn write_config(home: &Path, contents: &str) {
    std::fs::write(home.join("config.toml"), contents.trim_start()).unwrap();
}
