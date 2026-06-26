use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::settings::RelayProfile;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct ModelSource {
    source_id: String,
    source_type: String,
    name: String,
    base_url: String,
    api_key: String,
}

#[derive(Debug, Default)]
struct CodexConfig {
    root: HashMap<String, String>,
    profiles: HashMap<String, HashMap<String, String>>,
    model_providers: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Default)]
struct ConfigModelCatalog {
    models: Vec<String>,
    model_entries: Vec<Value>,
    status: Option<Value>,
}

pub async fn read_codex_model_catalog() -> Value {
    let home = codex_home_dir();
    let env = HashMap::new();
    let client = reqwest::Client::new();
    read_codex_model_catalog_from_home(&home, &env, client).await
}

pub fn read_configured_model_catalog_model_ids_from_home(home: &Path) -> Vec<String> {
    let config_path = home.join("config.toml");
    let (_config, effective, error) = load_codex_config(&config_path);
    if error.is_some() {
        return Vec::new();
    }
    models_from_config_model_catalog_json(home, &effective).models
}

pub fn relay_profile_model_ids_for_proxy(profile: &RelayProfile) -> Vec<String> {
    if !profile.model_mappings.is_empty() {
        return unique_strings(
            profile
                .model_mappings
                .iter()
                .map(|mapping| mapping.request_model.trim().to_string())
                .filter(|model| !model.is_empty())
                .collect(),
        );
    }

    let mut models = Vec::new();
    models.extend(relay_profile_responses_model_ids(profile));
    models.extend(relay_profile_chat_completions_model_ids(profile));
    models.extend(relay_profile_anthropic_model_ids(profile));
    unique_strings(models)
}

pub fn relay_profile_responses_model_ids(profile: &RelayProfile) -> Vec<String> {
    if !profile.model_mappings.is_empty() {
        return unique_strings(
            profile
                .model_mappings
                .iter()
                .filter(|mapping| mapping.protocol == crate::settings::RelayProtocol::Responses)
                .map(|mapping| mapping.request_model.trim().to_string())
                .filter(|model| !model.is_empty())
                .collect(),
        );
    }
    unique_strings(split_model_ids(&profile.responses_model_list))
}

pub fn relay_profile_chat_completions_model_ids(profile: &RelayProfile) -> Vec<String> {
    if !profile.model_mappings.is_empty() {
        return unique_strings(
            profile
                .model_mappings
                .iter()
                .filter(|mapping| {
                    mapping.protocol == crate::settings::RelayProtocol::ChatCompletions
                })
                .map(|mapping| mapping.request_model.trim().to_string())
                .filter(|model| !model.is_empty())
                .collect(),
        );
    }
    unique_strings(split_model_ids(&profile.chat_completions_model_list))
}

pub fn relay_profile_anthropic_model_ids(profile: &RelayProfile) -> Vec<String> {
    if !profile.model_mappings.is_empty() {
        return unique_strings(
            profile
                .model_mappings
                .iter()
                .filter(|mapping| mapping.protocol == crate::settings::RelayProtocol::Anthropic)
                .map(|mapping| mapping.request_model.trim().to_string())
                .filter(|model| !model.is_empty())
                .collect(),
        );
    }
    unique_strings(split_model_ids(&profile.anthropic_model_list))
}

fn split_model_ids(value: &str) -> Vec<String> {
    value
        .split(['\r', '\n', ','])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub async fn read_codex_model_catalog_from_home(
    home: &Path,
    _env: &HashMap<String, String>,
    _client: reqwest::Client,
) -> Value {
    let config_path = home.join("config.toml");
    let (config, effective, error) = load_codex_config(&config_path);
    let mut model = string_value(effective.get("model"));
    let mut model_provider = string_value(effective.get("model_provider"));
    let (resolved_provider, provider_config) =
        provider_config_for_model_provider(&config, &model_provider);
    if model_provider.is_empty() && !resolved_provider.is_empty() {
        model_provider = resolved_provider;
    }
    let provider_name = provider_config
        .as_ref()
        .and_then(|provider| provider.get("name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| model_provider.clone());

    if let Some(error) = error.as_ref().filter(|error| *error != "missing") {
        return json!({
            "status": "failed",
            "path": config_path.to_string_lossy(),
            "message": error,
            "model": model,
            "model_provider": model_provider,
            "provider_name": provider_name,
            "default_model": "",
            "models": [],
            "model_entries": [],
            "sources": [],
            "responses_api": responses_api_status("unknown", "", "")
        });
    }

    let mut source_statuses = Vec::new();
    let catalog = models_from_config_model_catalog_json(home, &effective);
    let models = catalog.models;
    let model_entries = catalog.model_entries;
    if let Some(status) = catalog.status {
        source_statuses.push(status);
    }

    if model.is_empty() {
        model = string_value(effective.get("default_model"));
    }
    let default_model = if models.iter().any(|item| item == &model) {
        model.clone()
    } else {
        models.first().cloned().unwrap_or_default()
    };
    let status = if !models.is_empty() {
        "ok"
    } else if !source_statuses.is_empty()
        && source_statuses
            .iter()
            .any(|source| source.get("status").and_then(Value::as_str) == Some("failed"))
    {
        "failed"
    } else if error.as_deref() == Some("missing") {
        "missing"
    } else {
        "not_configured"
    };
    let responses_api = preferred_responses_api_status(&source_statuses);

    json!({
        "status": status,
        "path": config_path.to_string_lossy(),
        "model": model,
        "model_provider": model_provider,
        "provider_name": provider_name,
        "default_model": default_model,
        "models": models,
        "model_entries": model_entries,
        "sources": source_statuses,
        "responses_api": responses_api
    })
}

fn codex_home_dir() -> PathBuf {
    crate::codex_home::default_codex_home_dir()
}

fn load_codex_config(path: &Path) -> (CodexConfig, HashMap<String, String>, Option<String>) {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (
                CodexConfig::default(),
                HashMap::new(),
                Some("missing".to_string()),
            );
        }
        Err(error) => {
            return (
                CodexConfig::default(),
                HashMap::new(),
                Some(error.to_string()),
            );
        }
    };
    let config = parse_codex_config(&contents);
    let mut effective = config.root.clone();
    if let Some(profile) = config.root.get("profile") {
        if let Some(profile_values) = config.profiles.get(profile) {
            effective.extend(profile_values.clone());
        }
    }
    (config, effective, None)
}

fn parse_codex_config(contents: &str) -> CodexConfig {
    let mut config = CodexConfig::default();
    let mut section = ConfigSection::Root;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = ConfigSection::from_header(trimmed.trim_matches(&['[', ']'][..]));
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = unquote_toml_string(value);
        match &section {
            ConfigSection::Root => {
                config.root.insert(key, value);
            }
            ConfigSection::Profile(name) => {
                config
                    .profiles
                    .entry(name.clone())
                    .or_default()
                    .insert(key, value);
            }
            ConfigSection::ModelProvider(name) => {
                config
                    .model_providers
                    .entry(name.clone())
                    .or_default()
                    .insert(key, value);
            }
            ConfigSection::Other => {}
        }
    }
    config
}

#[derive(Debug, Clone)]
enum ConfigSection {
    Root,
    Profile(String),
    ModelProvider(String),
    Other,
}

impl ConfigSection {
    fn from_header(header: &str) -> Self {
        if let Some(name) = header.strip_prefix("profiles.") {
            return Self::Profile(name.trim_matches('"').to_string());
        }
        if let Some(name) = header.strip_prefix("model_providers.") {
            return Self::ModelProvider(name.trim_matches('"').to_string());
        }
        Self::Other
    }
}

fn provider_config_for_model_provider(
    config: &CodexConfig,
    model_provider: &str,
) -> (String, Option<HashMap<String, String>>) {
    if !model_provider.is_empty() {
        return (
            model_provider.to_string(),
            config.model_providers.get(model_provider).cloned(),
        );
    }
    if config.model_providers.len() == 1 {
        if let Some((name, provider)) = config.model_providers.iter().next() {
            return (name.clone(), Some(provider.clone()));
        }
    }
    (model_provider.to_string(), None)
}

async fn fetch_models_from_source(
    client: &reqwest::Client,
    source: &ModelSource,
) -> (Vec<String>, Value) {
    let endpoint = models_endpoint(&source.base_url);
    let mut safe_source = json!({
        "id": source.source_id,
        "type": source.source_type,
        "name": source.name,
        "base_url": safe_url_for_status(&source.base_url),
        "endpoint": safe_url_for_status(&endpoint),
        "auth": if source.api_key.is_empty() { "missing" } else { "present" },
    });
    if endpoint.is_empty() {
        safe_source["status"] = json!("failed");
        safe_source["message"] = json!("Missing base URL");
        safe_source["models"] = json!(0);
        return (Vec::new(), safe_source);
    }

    let mut request = client
        .get(&endpoint)
        .header(reqwest::header::ACCEPT, "application/json");
    if !source.api_key.is_empty() {
        request = request.bearer_auth(&source.api_key);
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
            Ok(payload) => {
                let models = unique_strings(parse_model_payload(&payload));
                safe_source["status"] = json!("ok");
                safe_source["models"] = json!(models.len());
                (models, safe_source)
            }
            Err(error) => failed_source(safe_source, error.to_string()),
        },
        Ok(response) => failed_source(safe_source, format!("HTTP {}", response.status().as_u16())),
        Err(error) => failed_source(safe_source, error.to_string()),
    }
}

fn failed_source(mut source: Value, message: String) -> (Vec<String>, Value) {
    source["status"] = json!("failed");
    source["message"] = json!(message);
    source["models"] = json!(0);
    source["responses_api"] = responses_api_status("unknown", "", "");
    (Vec::new(), source)
}

fn responses_api_status(status: &str, endpoint: &str, message: &str) -> Value {
    json!({
        "status": status,
        "endpoint": endpoint,
        "message": message
    })
}

pub async fn fetch_relay_profile_model_ids(
    profile: &RelayProfile,
) -> anyhow::Result<(Vec<String>, String)> {
    let source = ModelSource {
        source_id: format!("relay-profile:{}", profile.id),
        source_type: "relay_profile".to_string(),
        name: if profile.name.trim().is_empty() {
            profile.id.clone()
        } else {
            profile.name.trim().to_string()
        },
        base_url: if profile.upstream_base_url.trim().is_empty() {
            profile.base_url.trim().to_string()
        } else {
            profile.upstream_base_url.trim().to_string()
        },
        api_key: profile.api_key.trim().to_string(),
    };
    if source.base_url.is_empty() {
        anyhow::bail!("Base URL 不能为空");
    }
    let endpoint = models_endpoint(&source.base_url);
    let client = crate::http_client::proxied_client(&profile.user_agent)?;
    let (models, status) = fetch_models_from_source(&client, &source).await;
    if models.is_empty() {
        let message = status
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("上游没有返回可用模型");
        anyhow::bail!("{message}");
    }
    Ok((models, endpoint))
}

fn preferred_responses_api_status(sources: &[Value]) -> Value {
    let statuses = sources
        .iter()
        .filter_map(|source| source.get("responses_api"))
        .collect::<Vec<_>>();
    for wanted in ["unsupported", "supported", "failed"] {
        if let Some(status) = statuses
            .iter()
            .find(|status| status.get("status").and_then(Value::as_str) == Some(wanted))
        {
            return (*status).clone();
        }
    }
    responses_api_status("unknown", "", "")
}

fn models_endpoint(base_url: &str) -> String {
    let cleaned = safe_url_for_status(base_url)
        .trim_end_matches('/')
        .to_string();
    if cleaned.is_empty() {
        return String::new();
    }
    if cleaned.ends_with("/models") {
        return cleaned;
    }
    if cleaned.ends_with("/v1") {
        return format!("{cleaned}/models");
    }
    format!("{cleaned}/v1/models")
}

fn parse_model_payload(payload: &Value) -> Vec<String> {
    if let Some(array) = payload.as_array() {
        return array
            .iter()
            .filter_map(|item| {
                item.as_str().map(str::to_string).or_else(|| {
                    item.as_object().and_then(|object| {
                        ["id", "model", "name"]
                            .iter()
                            .filter_map(|key| object.get(*key).and_then(Value::as_str))
                            .find(|value| !value.trim().is_empty())
                            .map(|value| value.trim().to_string())
                    })
                })
            })
            .collect();
    }
    let Some(object) = payload.as_object() else {
        return Vec::new();
    };
    for key in ["data", "models", "items"] {
        if let Some(value) = object.get(key) {
            let nested = parse_model_payload(value);
            if !nested.is_empty() {
                return nested;
            }
        }
    }
    ["id", "model", "name"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .map(|value| vec![value.trim().to_string()])
        .unwrap_or_default()
}

fn models_from_config_model_catalog_json(
    home: &Path,
    effective: &HashMap<String, String>,
) -> ConfigModelCatalog {
    let raw_path = string_value(effective.get("model_catalog_json"));
    if raw_path.is_empty() {
        return ConfigModelCatalog::default();
    }
    let path = resolve_config_path(home, &raw_path);
    let safe_path = path.to_string_lossy().to_string();
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            return ConfigModelCatalog {
                status: Some(json!({
                    "id": "config:model_catalog_json",
                    "type": "model_catalog_json",
                    "name": "Codex model catalog",
                    "path": safe_path,
                    "status": "failed",
                    "message": error.to_string(),
                    "models": 0,
                    "responses_api": responses_api_status("unknown", "", "")
                })),
                ..ConfigModelCatalog::default()
            };
        }
    };
    let payload = match serde_json::from_str::<Value>(&contents) {
        Ok(payload) => payload,
        Err(error) => {
            return ConfigModelCatalog {
                status: Some(json!({
                    "id": "config:model_catalog_json",
                    "type": "model_catalog_json",
                    "name": "Codex model catalog",
                    "path": safe_path,
                    "status": "failed",
                    "message": error.to_string(),
                    "models": 0,
                    "responses_api": responses_api_status("unknown", "", "")
                })),
                ..ConfigModelCatalog::default()
            };
        }
    };
    let model_entries = parse_model_catalog_json_model_entries(&payload);
    let models = unique_strings(
        model_entries
            .iter()
            .filter_map(|model| model.get("slug").and_then(Value::as_str))
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(str::to_string)
            .collect(),
    );
    let count = models.len();
    ConfigModelCatalog {
        models,
        model_entries,
        status: Some(json!({
            "id": "config:model_catalog_json",
            "type": "model_catalog_json",
            "name": "Codex model catalog",
            "path": safe_path,
            "status": "ok",
            "models": count,
            "responses_api": responses_api_status("unknown", "", "")
        })),
    }
}

fn resolve_config_path(home: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        home.join(path)
    }
}

fn parse_model_catalog_json_model_entries(payload: &Value) -> Vec<Value> {
    let Some(models) = payload.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    models
        .iter()
        .filter(|model| catalog_model_visible_in_api(model))
        .filter(|model| {
            model
                .get("slug")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(|slug| !slug.is_empty())
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn catalog_model_visible_in_api(model: &Value) -> bool {
    let supported_in_api = model
        .get("supported_in_api")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !supported_in_api {
        return false;
    }
    let visibility = model
        .get("visibility")
        .and_then(Value::as_str)
        .unwrap_or("list")
        .trim();
    visibility.eq_ignore_ascii_case("list")
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || !seen.insert(value.to_string()) {
            continue;
        }
        result.push(value.to_string());
    }
    result
}

fn safe_url_for_status(url: &str) -> String {
    let mut cleaned = url
        .split('?')
        .next()
        .unwrap_or_default()
        .split('#')
        .next()
        .unwrap_or_default()
        .to_string();
    if let Ok(parsed) = reqwest::Url::parse(&cleaned) {
        let host = parsed.host_str().unwrap_or_default();
        let authority = parsed
            .port()
            .map(|port| format!("{host}:{port}"))
            .unwrap_or_else(|| host.to_string());
        cleaned = format!("{}://{}{}", parsed.scheme(), authority, parsed.path());
    }
    cleaned
}

fn string_value(value: Option<&String>) -> String {
    value
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn unquote_toml_string(value: &str) -> String {
    let value = value.trim();
    if let Ok(parsed) = toml::from_str::<toml::Value>(&format!("value = {value}")) {
        if let Some(value) = parsed.get("value").and_then(toml::Value::as_str) {
            return value.to_string();
        }
    }
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_string()
}
