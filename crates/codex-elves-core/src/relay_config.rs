use anyhow::Context;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::settings::{RelayContextSelection, RelayProfile, RelayProtocol};

const RELAY_PROVIDER: &str = "custom";
const LEGACY_RELAY_PROVIDERS: &[&str] = &["CodexElves", "CodexPP"];
const CHAT_UPSTREAM_BASE_URL_KEY: &str = "codex_elves_chat_base_url";
const GENERATED_MODEL_CATALOG_FILENAME: &str = "codex-elves-model-catalog.json";
const RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "openai",
    "ollama",
    "lmstudio",
    "oss",
    "ollama-chat",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptAuthStatus {
    pub authenticated: bool,
    pub source: String,
    pub account_label: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayConfigStatus {
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
    pub config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    pub authenticated: bool,
    pub auth_source: String,
    pub account_label: Option<String>,
    pub config_path: String,
    pub configured: bool,
    pub requires_openai_auth: bool,
    pub has_bearer_token: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayApplyResult {
    pub config_path: String,
    pub backup_path: Option<String>,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayProfileTestResult {
    pub http_status: u16,
    pub endpoint: String,
    pub response_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContextEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub toml_body: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContextEntries {
    pub mcp_servers: Vec<CodexContextEntry>,
    pub skills: Vec<CodexContextEntry>,
    pub plugins: Vec<CodexContextEntry>,
}

pub fn default_codex_home_dir() -> PathBuf {
    crate::codex_home::default_codex_home_dir()
}

pub fn default_relay_status() -> RelayStatus {
    relay_status_from_home(&default_codex_home_dir())
}

pub fn set_codex_goals_feature_in_home(home: &Path, enabled: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    let config_path = home.join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let updated = match parse_toml_document(&existing) {
        Ok(mut doc) => {
            if enabled {
                let features = table_mut_or_insert(&mut doc, "features")?;
                features["goals"] = toml_edit::value(true);
            } else if let Some(features) = table_mut_if_exists(&mut doc, "features") {
                features.remove("goals");
                if features.is_empty() {
                    doc.as_table_mut().remove("features");
                }
            }
            ensure_trailing_newline(doc.to_string())
        }
        Err(_) => set_codex_goals_feature_text_fallback(&existing, enabled),
    };
    crate::settings::atomic_write(&config_path, updated.as_bytes())
}

fn set_codex_goals_feature_text_fallback(existing: &str, enabled: bool) -> String {
    let mut kept = Vec::new();
    let mut skipping_features = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed == "[features]" {
            skipping_features = true;
            continue;
        }
        if skipping_features && trimmed.starts_with('[') && trimmed.ends_with(']') {
            skipping_features = false;
        }
        if !skipping_features {
            kept.push(line);
        }
    }

    let mut updated = kept.join("\n").trim_end().to_string();
    if enabled {
        if !updated.is_empty() {
            updated.push_str("\n\n");
        }
        updated.push_str("[features]\ngoals = true");
    }
    ensure_trailing_newline(updated)
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} 必须是 TOML table"))
}

fn table_mut_if_exists<'a>(doc: &'a mut DocumentMut, key: &str) -> Option<&'a mut Table> {
    doc.get_mut(key).and_then(Item::as_table_mut)
}

pub fn relay_status_from_home(home: &Path) -> RelayStatus {
    let auth = chatgpt_auth_status_from_home(home);
    let config = relay_config_status_from_home(home);
    RelayStatus {
        authenticated: auth.authenticated,
        auth_source: auth.source,
        account_label: auth.account_label,
        config_path: config.config_path,
        configured: config.configured,
        requires_openai_auth: config.requires_openai_auth,
        has_bearer_token: config.has_bearer_token,
    }
}

pub fn chatgpt_auth_status_from_home(home: &Path) -> ChatGptAuthStatus {
    let auth_path = home.join("auth.json");
    if let Some(account_label) = auth_json_chatgpt_account_label(&auth_path) {
        return ChatGptAuthStatus {
            authenticated: true,
            source: auth_path.to_string_lossy().to_string(),
            account_label,
            message: "已通过 auth.json 和 config.toml 检测到 ChatGPT 登录。".to_string(),
        };
    }

    ChatGptAuthStatus {
        authenticated: false,
        source: String::new(),
        account_label: None,
        message: "未检测到 ChatGPT 登录账号。".to_string(),
    }
}

pub fn relay_config_status_from_home(home: &Path) -> RelayConfigStatus {
    let config_path = home.join("config.toml");
    let contents = std::fs::read_to_string(&config_path).unwrap_or_default();
    let auth_contents = std::fs::read_to_string(home.join("auth.json")).unwrap_or_default();
    let root_provider = root_key_string(&contents, "model_provider");
    let provider = root_provider
        .as_ref()
        .and_then(|provider| table_values(&contents, &format!("model_providers.{provider}")));
    let requires_openai_auth = provider
        .as_ref()
        .and_then(|values| values.get("requires_openai_auth"))
        .map(|value| value.trim() == "true")
        .unwrap_or(false);
    let has_bearer_token = provider
        .as_ref()
        .and_then(|values| values.get("experimental_bearer_token"))
        .map(|value| unquote_toml_string(value).trim().to_string())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_base_url = provider
        .as_ref()
        .and_then(|values| values.get("base_url"))
        .map(|value| !unquote_toml_string(value).trim().is_empty())
        .unwrap_or(false);
    RelayConfigStatus {
        configured: root_provider.is_some()
            && requires_openai_auth
            && (has_bearer_token || codex_auth_api_key(&auth_contents).is_some())
            && has_base_url,
        requires_openai_auth,
        has_bearer_token,
        config_path: config_path.to_string_lossy().to_string(),
    }
}

pub fn apply_relay_config_to_home(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_config_to_home_with_protocol(
        home,
        base_url,
        bearer_token,
        RelayProtocol::Responses,
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    )
}

pub fn apply_relay_config_to_home_with_protocol(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
    protocol: RelayProtocol,
    proxy_port: u16,
) -> anyhow::Result<RelayApplyResult> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        anyhow::bail!("中转 Base URL 不能为空");
    }
    let bearer_token = bearer_token.trim();
    if bearer_token.is_empty() {
        anyhow::bail!("中转 Key 不能为空");
    }
    let codex_base_url = codex_base_url_for_protocol(base_url, protocol, proxy_port);
    let existing = read_optional_text(&home.join("config.toml"))?;
    let updated = upsert_model_provider_config(&existing, &codex_base_url, bearer_token)?;
    let auth_contents = serde_json::to_string_pretty(&json!({
        "OPENAI_API_KEY": bearer_token
    }))?;
    let backup_path =
        write_codex_live_atomic(home, Some(&updated), Some(auth_contents.as_bytes()), false)?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_pure_api_config_to_home(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_pure_api_config_to_home_with_protocol(
        home,
        base_url,
        bearer_token,
        RelayProtocol::Responses,
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    )
}

pub fn apply_relay_files_to_home(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_files_to_home_with_computer_use_guard(home, config_contents, auth_contents, false)
}

pub fn apply_relay_files_to_home_with_computer_use_guard(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    if config_contents.trim().is_empty() {
        anyhow::bail!("config.toml 内容不能为空");
    }
    std::fs::create_dir_all(home)?;

    let backup_path = write_codex_live_atomic(
        home,
        Some(config_contents),
        Some(auth_contents.as_bytes()),
        preserve_computer_use_guard,
    )?;

    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_relay_files_to_home_with_common(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let config_contents = merge_common_config_into_config(config_contents, common_config_contents)?;
    apply_relay_files_to_home(home, &config_contents, auth_contents)
}

pub fn apply_relay_files_to_home_with_context(
    home: &Path,
    config_contents: &str,
    auth_contents: &str,
    common_config_contents: &str,
    selection: &RelayContextSelection,
    context_window: &str,
    auto_compact_limit: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = filter_common_config_for_selection(common_config_contents, selection)?;
    let config_with_common = merge_common_config_into_config(config_contents, &selected_common)?;
    let config_with_common =
        preserve_unmanaged_live_context_entries(home, &config_with_common, common_config_contents)?;
    let config_with_limits =
        apply_context_limits_to_config(&config_with_common, context_window, auto_compact_limit)?;
    apply_relay_files_to_home(home, &config_with_limits, auth_contents)
}

pub fn apply_relay_profile_files_to_home_with_context(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = if profile.use_common_config {
        filter_common_config_for_profile(common_config_contents, profile)?
    } else {
        String::new()
    };
    let profile_config = complete_relay_profile_config(profile)?;
    let config_with_common = merge_common_config_into_config(&profile_config, &selected_common)?;
    let config_with_common =
        preserve_unmanaged_live_context_entries(home, &config_with_common, common_config_contents)?;
    let context_window = profile.context_window_for_active_model();
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog =
        apply_generated_model_catalog_to_config(home, &config_with_limits, profile)?;
    apply_relay_files_to_home(home, &config_with_catalog, &profile.auth_contents)
}

pub fn apply_relay_profile_to_home_with_switch_rules(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        home,
        profile,
        common_config_contents,
        false,
    )
}

pub fn apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    let _ = common_config_contents;
    let live_config = read_optional_text(&home.join("config.toml"))?;
    let config_with_catalog =
        apply_relay_profile_owned_fields_to_config(home, &live_config, profile)?;

    if matches!(
        profile.relay_mode,
        crate::settings::RelayMode::PureApi | crate::settings::RelayMode::Aggregate
    ) {
        let api_key = relay_profile_owned_api_key(profile);
        if api_key.trim().is_empty() {
            anyhow::bail!("供应商 API Key 不能为空");
        }
        let auth_contents = serde_json::to_string_pretty(&json!({
            "OPENAI_API_KEY": api_key.trim()
        }))?;
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &auth_contents,
            preserve_computer_use_guard,
        )
    } else {
        let auth_contents = official_profile_auth_for_switch(home, &profile.auth_contents)?;
        apply_relay_files_to_home_with_computer_use_guard(
            home,
            &config_with_catalog,
            &auth_contents,
            preserve_computer_use_guard,
        )
    }
}

fn apply_relay_profile_owned_fields_to_config(
    home: &Path,
    live_config: &str,
    profile: &RelayProfile,
) -> anyhow::Result<String> {
    let provider_id = relay_profile_provider_id(profile)?;
    let base_url = relay_profile_owned_base_url(profile);
    if base_url.trim().is_empty() {
        anyhow::bail!("供应商 Base URL 不能为空");
    }
    let api_key = relay_profile_owned_api_key(profile);
    if api_key.trim().is_empty() {
        anyhow::bail!("供应商 API Key 不能为空");
    }

    let mut updated = ensure_trailing_newline(live_config.trim_end().to_string());
    updated = set_root_toml_string_line(&updated, "model_provider", &provider_id);

    let model = relay_profile_owned_model(profile);
    if !model.trim().is_empty() {
        updated = set_root_toml_string_line(&updated, "model", model.trim());
    }

    updated = remove_root_key(&updated, CHAT_UPSTREAM_BASE_URL_KEY);
    let provider_table = model_provider_table_name(&provider_id);
    let codex_base_url = codex_base_url_for_proxy(
        base_url.trim(),
        profile.local_proxy_enabled(),
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    );
    updated = set_table_toml_string_line(&updated, &provider_table, "name", &provider_id);
    updated = set_table_toml_string_line(&updated, &provider_table, "wire_api", "responses");
    updated = set_table_toml_raw_line(&updated, &provider_table, "requires_openai_auth", "true");
    updated = set_table_toml_string_line(&updated, &provider_table, "base_url", &codex_base_url);
    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        updated = remove_table_key(&updated, &provider_table, "experimental_bearer_token");
    } else {
        updated = set_table_toml_string_line(
            &updated,
            &provider_table,
            "experimental_bearer_token",
            &api_key,
        );
    }

    let context_window = profile.context_window_for_active_model();
    if !context_window.trim().is_empty() && relay_profile_catalog_rows(profile).is_empty() {
        updated = set_root_toml_raw_line(&updated, "model_context_window", context_window.trim());
    }
    if !profile.auto_compact_limit.trim().is_empty() {
        updated = set_root_toml_raw_line(
            &updated,
            "model_auto_compact_token_limit",
            profile.auto_compact_limit.trim(),
        );
    }

    apply_owned_model_catalog_to_config(home, &updated, profile)
}

fn apply_owned_model_catalog_to_config(
    home: &Path,
    config_text: &str,
    profile: &RelayProfile,
) -> anyhow::Result<String> {
    let rows = relay_profile_catalog_rows(profile);
    if rows.is_empty() {
        return Ok(ensure_trailing_newline(config_text.trim_end().to_string()));
    }

    let config_with_catalog = set_root_toml_string_line(
        config_text,
        "model_catalog_json",
        GENERATED_MODEL_CATALOG_FILENAME,
    );
    let catalog = generated_model_catalog_json(profile, &config_with_catalog, rows)?;
    let path = home.join(GENERATED_MODEL_CATALOG_FILENAME);
    std::fs::create_dir_all(home)?;
    let bytes = serde_json::to_vec_pretty(&catalog)?;
    crate::settings::atomic_write(&path, &bytes).context("写入模型目录失败")?;
    Ok(config_with_catalog)
}

pub fn sync_applied_relay_profile_model_catalog_to_home(
    home: &Path,
    profile: &RelayProfile,
) -> anyhow::Result<bool> {
    let live_config = read_optional_text(&home.join("config.toml"))?;
    let live_provider = root_key_string(&live_config, "model_provider").unwrap_or_default();
    let profile_provider = relay_profile_provider_id(profile)?;
    if live_provider.trim() != profile_provider.trim() {
        return Ok(false);
    }

    let rows = relay_profile_catalog_rows(profile);
    let config_with_catalog = if rows.is_empty() {
        live_config.clone()
    } else {
        set_root_toml_string_line(
            &live_config,
            "model_catalog_json",
            GENERATED_MODEL_CATALOG_FILENAME,
        )
    };
    if config_with_catalog != live_config {
        write_codex_live_atomic(home, Some(&config_with_catalog), None, false)?;
    }
    let catalog = generated_model_catalog_json(profile, &config_with_catalog, rows)?;
    let path = home.join(GENERATED_MODEL_CATALOG_FILENAME);
    std::fs::create_dir_all(home)?;
    let bytes = serde_json::to_vec_pretty(&catalog)?;
    crate::settings::atomic_write(&path, &bytes).context("写入模型目录失败")?;
    Ok(true)
}

pub fn apply_relay_profile_config_to_home_with_context(
    home: &Path,
    profile: &RelayProfile,
    common_config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    let selected_common = if profile.use_common_config {
        filter_common_config_for_selection(common_config_contents, &profile.context_selection)?
    } else {
        String::new()
    };
    let profile_config = complete_relay_profile_config(profile)?;
    let config_with_common = merge_common_config_into_config(&profile_config, &selected_common)?;
    let context_window = profile.context_window_for_active_model();
    let config_with_limits = apply_context_limits_to_config(
        &config_with_common,
        &context_window,
        &profile.auto_compact_limit,
    )?;
    let config_with_catalog =
        apply_generated_model_catalog_to_config(home, &config_with_limits, profile)?;
    apply_relay_config_file_to_home(home, &config_with_catalog)
}

pub fn apply_relay_config_file_to_home(
    home: &Path,
    config_contents: &str,
) -> anyhow::Result<RelayApplyResult> {
    if config_contents.trim().is_empty() {
        anyhow::bail!("config.toml 内容不能为空");
    }
    std::fs::create_dir_all(home)?;

    let backup_path = write_codex_live_atomic(home, Some(config_contents), None, false)?;

    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub fn apply_pure_api_config_to_home_with_protocol(
    home: &Path,
    base_url: &str,
    bearer_token: &str,
    protocol: RelayProtocol,
    proxy_port: u16,
) -> anyhow::Result<RelayApplyResult> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        anyhow::bail!("中转 Base URL 不能为空");
    }
    let bearer_token = bearer_token.trim();
    if bearer_token.is_empty() {
        anyhow::bail!("中转 Key 不能为空");
    }
    let codex_base_url = codex_base_url_for_protocol(base_url, protocol, proxy_port);
    let existing = read_optional_text(&home.join("config.toml"))?;
    let updated = upsert_model_provider_config(&existing, &codex_base_url, bearer_token)?;
    let auth_contents = serde_json::to_string_pretty(&json!({
        "OPENAI_API_KEY": bearer_token
    }))?;
    let backup_path =
        write_codex_live_atomic(home, Some(&updated), Some(auth_contents.as_bytes()), false)?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

pub async fn test_relay_profile(
    profile: &RelayProfile,
    model: &str,
) -> anyhow::Result<RelayProfileTestResult> {
    let base_url = relay_profile_base_url(profile);
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        anyhow::bail!("Base URL 不能为空");
    }
    let api_key = relay_profile_api_key(profile);
    let api_key = api_key.trim();
    if api_key.is_empty() {
        anyhow::bail!("API Key 不能为空");
    }

    let test_model = model.trim();
    if test_model.is_empty() {
        anyhow::bail!("测试模型不能为空");
    }

    let protocol = profile.protocol_for_model(test_model);
    let client = crate::http_client::proxied_client("CodexElves/RelayTest")?;
    let endpoint = match protocol {
        RelayProtocol::Responses => format!("{base_url}/responses"),
        RelayProtocol::ChatCompletions => format!("{base_url}/chat/completions"),
        RelayProtocol::Anthropic => format!("{base_url}/messages"),
    };
    let payload = relay_profile_test_payload(protocol, test_model);
    let response = relay_profile_test_request(client.post(&endpoint), protocol, api_key)
        .json(&payload)
        .send()
        .await?;
    let http_status = response.status().as_u16();

    // 如果 404 且 base_url 末尾没有 /v1，尝试自动补 /v1 后再发一次。
    // 许多上游（中转站、自建代理）暴露的路径以 /v1/ 开头，
    // 用户容易遗漏这个前缀，导致 /responses 或 /chat/completions 404。
    if http_status == 404 && !base_url.ends_with("/v1") {
        let v1_url = format!("{base_url}/v1");
        let v1_endpoint = match protocol {
            RelayProtocol::Responses => format!("{v1_url}/responses"),
            RelayProtocol::ChatCompletions => format!("{v1_url}/chat/completions"),
            RelayProtocol::Anthropic => format!("{v1_url}/messages"),
        };
        let v1_response = relay_profile_test_request(client.post(&v1_endpoint), protocol, api_key)
            .json(&payload)
            .send()
            .await?;
        let v1_status = v1_response.status().as_u16();
        if v1_status < 400 {
            let response_text = v1_response.text().await.unwrap_or_default();
            return Ok(RelayProfileTestResult {
                http_status: v1_status,
                endpoint: v1_endpoint,
                response_preview: format!(
                    "（Base URL 建议加上 /v1 前缀）{}",
                    response_text.chars().take(280).collect::<String>()
                ),
            });
        }
    }

    let response_text = response.text().await.unwrap_or_default();
    Ok(RelayProfileTestResult {
        http_status,
        endpoint,
        response_preview: response_text.chars().take(320).collect(),
    })
}

fn relay_profile_test_payload(protocol: RelayProtocol, model: &str) -> Value {
    match protocol {
        RelayProtocol::Responses => serde_json::json!({
            "model": model,
            "input": "hi",
            "max_output_tokens": 16
        }),
        RelayProtocol::ChatCompletions => serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 16
        }),
        RelayProtocol::Anthropic => serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "hi" }
            ],
            "max_tokens": 16
        }),
    }
}

fn relay_profile_test_request(
    request: reqwest::RequestBuilder,
    protocol: RelayProtocol,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let request = request.header(reqwest::header::CONTENT_TYPE, "application/json");
    match protocol {
        RelayProtocol::Anthropic => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        RelayProtocol::Responses | RelayProtocol::ChatCompletions => request.bearer_auth(api_key),
    }
}

fn codex_base_url_for_protocol(base_url: &str, protocol: RelayProtocol, proxy_port: u16) -> String {
    match protocol {
        RelayProtocol::Responses => base_url.to_string(),
        RelayProtocol::ChatCompletions | RelayProtocol::Anthropic => {
            crate::protocol_proxy::local_responses_proxy_base_url(proxy_port)
        }
    }
}

pub fn codex_base_url_for_proxy(
    base_url: &str,
    local_proxy_enabled: bool,
    proxy_port: u16,
) -> String {
    if local_proxy_enabled {
        crate::protocol_proxy::local_responses_proxy_base_url(proxy_port)
    } else {
        base_url.to_string()
    }
}

pub fn clear_relay_config_to_home(home: &Path) -> anyhow::Result<RelayApplyResult> {
    clear_relay_config_to_home_with_auth(home, None)
}

pub fn clear_relay_config_to_home_with_auth(
    home: &Path,
    auth_contents: Option<&str>,
) -> anyhow::Result<RelayApplyResult> {
    clear_relay_config_to_home_with_auth_and_computer_use_guard(home, auth_contents, false)
}

pub fn clear_relay_config_to_home_with_auth_and_computer_use_guard(
    home: &Path,
    auth_contents: Option<&str>,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<RelayApplyResult> {
    std::fs::create_dir_all(home)?;
    let auth_bytes = match auth_contents {
        Some(contents) if !contents.trim().is_empty() => Some(contents.as_bytes().to_vec()),
        _ => pure_api_auth_json_removed(home)?,
    };
    let config_path = home.join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut without_tables = remove_table(&existing, &format!("model_providers.{RELAY_PROVIDER}"));
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        without_tables = remove_table(
            &without_tables,
            &format!("model_providers.{legacy_provider}"),
        );
    }
    let mut updated = without_tables;
    for key in [
        "OPENAI_API_KEY",
        "model_provider",
        "model_catalog_json",
        "base_url",
    ] {
        updated = remove_root_key(&updated, key);
    }
    let backup_path = write_codex_live_atomic(
        home,
        Some(&updated),
        auth_bytes.as_deref(),
        preserve_computer_use_guard,
    )?;
    let status = relay_config_status_from_home(home);
    Ok(RelayApplyResult {
        config_path: status.config_path,
        backup_path,
        configured: status.configured,
    })
}

fn pure_api_auth_json_removed(home: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let auth_path = home.join("auth.json");
    if !auth_path.exists() {
        return Ok(None);
    }

    let existing = std::fs::read_to_string(&auth_path)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&existing) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    if object.remove("OPENAI_API_KEY").is_none() {
        return Ok(None);
    }

    Ok(Some(serde_json::to_vec_pretty(&value)?))
}

pub fn backfill_relay_profile_from_home(
    home: &Path,
    profile: &mut RelayProfile,
) -> anyhow::Result<()> {
    profile.config_contents = read_optional_text(&home.join("config.toml"))?;
    profile.auth_contents = read_optional_text(&home.join("auth.json"))?;
    let live_config = profile.config_contents.clone();
    sync_context_limits_from_config(profile, &live_config);
    if profile.model.trim().is_empty() {
        if let Some(model) = root_key_string(&profile.config_contents, "model") {
            profile.model = model;
        }
    }
    Ok(())
}

pub fn backfill_relay_profile_from_home_with_common(
    home: &Path,
    profile: &mut RelayProfile,
    common_config_contents: &mut String,
) -> anyhow::Result<()> {
    let live_config = read_optional_text(&home.join("config.toml"))?;
    let template_config = profile.config_contents.clone();
    let template_auth = profile.auth_contents.clone();
    profile.config_contents = if profile.use_common_config {
        strip_common_config_from_config(&live_config, common_config_contents)?
    } else {
        ensure_trailing_newline(live_config.clone())
    };
    profile.config_contents =
        restore_profile_provider_id_for_backfill(&profile.config_contents, &template_config)?;
    profile.auth_contents = read_optional_text(&home.join("auth.json"))?;
    restore_profile_auth_from_live_config(profile, &template_auth)?;
    sync_profile_mode_from_backfilled_live(profile);
    sync_context_limits_from_config(profile, &live_config);
    if profile.model.trim().is_empty() {
        if let Some(model) = root_key_string(&live_config, "model") {
            profile.model = model;
        }
    }
    Ok(())
}

pub fn extract_common_config_from_config(config_text: &str) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    for key in [
        "model",
        "model_provider",
        "base_url",
        "model_catalog_json",
        CHAT_UPSTREAM_BASE_URL_KEY,
    ] {
        doc.as_table_mut().remove(key);
    }
    doc.as_table_mut().remove("model_providers");
    Ok(normalize_optional_toml(doc))
}

pub fn sanitize_common_config_contents(common_config: &str) -> String {
    match parse_toml_document(common_config) {
        Ok(mut doc) => {
            remove_provider_specific_common_keys(doc.as_table_mut());
            normalize_optional_toml(doc)
        }
        Err(_) => sanitize_common_config_text_fallback(common_config),
    }
}

pub fn strip_common_config_from_config(
    config_text: &str,
    common_config_contents: &str,
) -> anyhow::Result<String> {
    let trimmed = common_config_contents.trim();
    if trimmed.is_empty() {
        return Ok(normalize_duplicate_toml_text(config_text));
    }

    match (
        parse_toml_document(config_text),
        parse_toml_document(trimmed),
    ) {
        (Ok(mut target_doc), Ok(source_doc)) => {
            remove_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
            Ok(normalize_optional_toml(target_doc))
        }
        _ => Ok(strip_common_config_text_fallback(config_text, trimmed)),
    }
}

pub fn merge_common_config_into_config(
    config_text: &str,
    common_config_contents: &str,
) -> anyhow::Result<String> {
    let sanitized_common = sanitize_common_config_contents(common_config_contents);
    let trimmed = sanitized_common.trim();
    if trimmed.is_empty() {
        return Ok(ensure_trailing_newline(config_text.to_string()));
    }

    let mut target_doc = parse_toml_document(config_text)?;
    let source_doc = parse_toml_document(trimmed)?;
    merge_toml_table_like(target_doc.as_table_mut(), source_doc.as_table());
    Ok(normalize_optional_toml(target_doc))
}

pub fn list_context_entries_from_common_config(
    common_config: &str,
) -> anyhow::Result<CodexContextEntries> {
    let normalized = normalize_duplicate_toml_text(common_config);
    let doc = parse_toml_document(&normalized)?;
    Ok(CodexContextEntries {
        mcp_servers: list_context_entries_for_table(&normalized, &doc, "mcp_servers"),
        skills: list_context_entries_for_table(&normalized, &doc, "skills"),
        plugins: list_context_entries_for_table(&normalized, &doc, "plugins"),
    })
}

pub fn upsert_context_entry_in_common_config(
    common_config: &str,
    kind: &str,
    id: &str,
    toml_body: &str,
) -> anyhow::Result<String> {
    let id = id.trim();
    if id.is_empty() {
        anyhow::bail!("上下文 id 不能为空");
    }
    let table_name = context_table_name(kind)?;
    let existing_header = matching_context_text_blocks(common_config, table_name, id)
        .into_iter()
        .find(|block| block.is_root)
        .and_then(|block| normalized_lines(&block.text).into_iter().next());
    let section =
        context_entry_section_text(table_name, id, toml_body, existing_header.as_deref())?;
    let updated = upsert_context_text_block(common_config, table_name, id, &section);
    parse_toml_document(&updated)?;
    Ok(normalize_text_toml_preserving_layout(&updated))
}

pub fn delete_context_entry_from_common_config(
    common_config: &str,
    kind: &str,
    id: &str,
) -> anyhow::Result<String> {
    let table_name = context_table_name(kind)?;
    let updated = delete_context_text_block(common_config, table_name, id.trim());
    parse_toml_document(&updated)?;
    Ok(normalize_text_toml_preserving_layout(&updated))
}

pub fn filter_common_config_for_selection(
    common_config: &str,
    selection: &RelayContextSelection,
) -> anyhow::Result<String> {
    let sanitized_common = sanitize_common_config_contents(common_config);
    let mut filtered = parse_toml_document(&sanitized_common)?;
    filter_context_tables_for_selection(filtered.as_table_mut(), selection);
    remove_disabled_context_tables(filtered.as_table_mut());
    Ok(normalize_optional_toml(filtered))
}

fn filter_common_config_for_profile(
    common_config: &str,
    profile: &RelayProfile,
) -> anyhow::Result<String> {
    if profile.context_selection_initialized {
        filter_common_config_for_selection(common_config, &profile.context_selection)
    } else {
        let sanitized_common = sanitize_common_config_contents(common_config);
        let mut filtered = parse_toml_document(&sanitized_common)?;
        remove_disabled_context_tables(filtered.as_table_mut());
        Ok(normalize_optional_toml(filtered))
    }
}

pub fn sync_live_config_context_entries(
    live_config: &str,
    context_config: &str,
) -> anyhow::Result<String> {
    let normalized_context = normalize_duplicate_toml_text(context_config);
    parse_toml_document(live_config)?;
    if normalized_context.trim().is_empty() {
        return Ok(normalize_text_toml_preserving_layout(live_config));
    }
    let managed_doc = parse_toml_document(&normalized_context)?;
    let context_blocks = context_text_blocks(&normalized_context);
    let mut updated = live_config.to_string();
    for block in context_blocks {
        if context_doc_entry_enabled(&managed_doc, &block.table_name, &block.id) {
            updated =
                upsert_context_text_block(&updated, &block.table_name, &block.id, &block.text);
        } else {
            updated = delete_context_text_block(&updated, &block.table_name, &block.id);
        }
    }
    parse_toml_document(&updated)?;
    Ok(normalize_text_toml_preserving_layout(&updated))
}

pub fn sync_live_config_context_entry(
    live_config: &str,
    context_config: &str,
    kind: &str,
    id: &str,
) -> anyhow::Result<String> {
    let table_name = context_table_name(kind)?;
    let id = id.trim();
    if id.is_empty() {
        anyhow::bail!("上下文 id 不能为空");
    }

    let normalized_context = normalize_duplicate_toml_text(context_config);
    parse_toml_document(live_config)?;
    let managed_doc = parse_toml_document(&normalized_context)?;
    let blocks = matching_context_text_blocks(&normalized_context, table_name, id);
    let section = context_section_text_from_blocks(&blocks);
    let updated = if let Some(section) = section {
        if context_doc_entry_enabled(&managed_doc, table_name, id) {
            upsert_context_text_block(live_config, table_name, id, &section)
        } else {
            delete_context_text_block(live_config, table_name, id)
        }
    } else {
        delete_context_text_block(live_config, table_name, id)
    };
    parse_toml_document(&updated)?;
    Ok(normalize_text_toml_preserving_layout(&updated))
}

fn context_section_text_from_blocks(blocks: &[ContextTextBlock]) -> Option<String> {
    let root_index = blocks.iter().position(|block| block.is_root)?;
    let mut sections = vec![blocks[root_index].text.trim_end().to_string()];
    sections.extend(blocks.iter().enumerate().filter_map(|(index, block)| {
        (index != root_index).then(|| block.text.trim_end().to_string())
    }));
    Some(sections.join("\n\n"))
}

fn preserve_unmanaged_live_context_entries(
    home: &Path,
    config_text: &str,
    managed_context_config: &str,
) -> anyhow::Result<String> {
    if managed_context_config.trim().is_empty() {
        return Ok(ensure_trailing_newline(config_text.to_string()));
    }
    let live_config = read_optional_text(&home.join("config.toml"))?;
    if live_config.trim().is_empty() {
        return Ok(ensure_trailing_newline(config_text.to_string()));
    }
    let mut target_doc = parse_toml_document(config_text)?;
    let live_doc = parse_toml_document(&live_config)?;
    let managed_doc =
        parse_toml_document(&sanitize_common_config_contents(managed_context_config))?;
    preserve_unmanaged_context_tables(
        target_doc.as_table_mut(),
        live_doc.as_table(),
        managed_doc.as_table(),
    );
    Ok(normalize_optional_toml(target_doc))
}

fn filter_context_tables_for_selection(
    table: &mut toml_edit::Table,
    selection: &RelayContextSelection,
) {
    filter_context_table_for_ids(table, "mcp_servers", &selection.mcp_servers);
    filter_context_table_for_ids(table, "skills", &selection.skills);
    filter_context_table_for_ids(table, "plugins", &selection.plugins);
}

fn filter_context_table_for_ids(
    table: &mut toml_edit::Table,
    table_name: &str,
    selected_ids: &[String],
) {
    let Some(item) = table.get_mut(table_name) else {
        return;
    };
    let Some(context_table) = item.as_table_mut() else {
        return;
    };
    let selected = selected_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .collect::<HashSet<_>>();
    let remove_ids = context_table
        .iter()
        .filter_map(|(id, _)| (!selected.contains(id)).then_some(id.to_string()))
        .collect::<Vec<_>>();
    for id in remove_ids {
        context_table.remove(&id);
    }
}

fn preserve_unmanaged_context_tables(
    target: &mut toml_edit::Table,
    live: &toml_edit::Table,
    managed: &toml_edit::Table,
) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        preserve_unmanaged_context_table(target, live, managed, table_name);
    }
}

fn preserve_unmanaged_context_table(
    target: &mut toml_edit::Table,
    live: &toml_edit::Table,
    managed: &toml_edit::Table,
    table_name: &str,
) {
    let Some(live_item) = live.get(table_name) else {
        return;
    };
    let Some(live_table) = live_item.as_table_like() else {
        return;
    };
    if target.get(table_name).is_none() {
        target[table_name] = toml_edit::table();
    }
    let Some(target_table) = target.get_mut(table_name).and_then(Item::as_table_like_mut) else {
        return;
    };
    let managed_ids = managed
        .get(table_name)
        .and_then(Item::as_table_like)
        .map(|table| {
            table
                .iter()
                .map(|(id, _)| id.to_string())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    for (id, item) in live_table.iter() {
        if !managed_ids.contains(id) && target_table.get(id).is_none() {
            target_table.insert(id, item.clone());
        }
    }
}

fn remove_disabled_context_tables(table: &mut toml_edit::Table) {
    for table_name in ["mcp_servers", "skills", "plugins"] {
        let Some(item) = table.get_mut(table_name) else {
            continue;
        };
        let Some(context_table) = item.as_table_mut() else {
            continue;
        };
        let disabled_ids: Vec<String> = context_table
            .iter()
            .filter_map(|(id, item)| {
                let enabled = item.as_table().map(context_entry_enabled).unwrap_or(true);
                (!enabled).then_some(id.to_string())
            })
            .collect();
        for id in disabled_ids {
            context_table.remove(&id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextTextBlock {
    table_name: String,
    id: String,
    is_root: bool,
    start: usize,
    end: usize,
    text: String,
}

fn context_text_blocks(contents: &str) -> Vec<ContextTextBlock> {
    let lines = normalized_lines(contents);
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let Some((table_name, id, is_root)) = context_block_header(&lines[index]) else {
            index += 1;
            continue;
        };

        let start = index;
        let mut scan_end = start + 1;
        while scan_end < lines.len() {
            if let Some(path) = toml_header_path_from_line(&lines[scan_end]) {
                if !toml_path_belongs_to_context_id(&path, &table_name, &id) {
                    break;
                }
                if !is_root && path.len() == 2 {
                    break;
                }
            }
            scan_end += 1;
        }

        let mut end = scan_end;
        while end > start + 1 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }

        blocks.push(ContextTextBlock {
            table_name,
            id,
            is_root,
            start,
            end,
            text: lines[start..end].join("\n"),
        });
        index = scan_end;
    }

    blocks
}

fn context_block_header(line: &str) -> Option<(String, String, bool)> {
    let path = toml_table_path_from_line(line)?;
    if path.len() < 2 || !is_context_table_name(&path[0]) {
        return None;
    }
    Some((path[0].clone(), path[1].clone(), path.len() == 2))
}

fn toml_header_path_from_line(line: &str) -> Option<Vec<String>> {
    toml_table_path_from_line(line).or_else(|| toml_array_table_path_from_line(line))
}

fn context_block_body_text(block: &ContextTextBlock) -> String {
    let body = normalized_lines(&block.text)
        .into_iter()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    normalize_text_toml_preserving_layout(&body)
}

fn context_entry_section_text(
    table_name: &str,
    id: &str,
    toml_body: &str,
    root_header: Option<&str>,
) -> anyhow::Result<String> {
    let mut lines = vec![
        root_header
            .map(|header| header.trim_end().to_string())
            .unwrap_or_else(|| {
                format!(
                    "[{}.{}]",
                    toml_key_segment(table_name),
                    toml_key_segment(id)
                )
            }),
    ];

    for line in normalized_section_lines(toml_body) {
        if let Some(line) = context_body_line_to_absolute(table_name, id, &line)? {
            lines.push(line);
        }
    }

    let section = lines.join("\n");
    parse_toml_document(&section)?;
    Ok(section)
}

fn context_body_line_to_absolute(
    table_name: &str,
    id: &str,
    line: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(path) = toml_table_path_from_line(line) {
        return context_body_path_to_absolute(table_name, id, line, &path, false);
    }
    if let Some(path) = toml_array_table_path_from_line(line) {
        return context_body_path_to_absolute(table_name, id, line, &path, true);
    }
    Ok(Some(line.trim_end().to_string()))
}

fn context_body_path_to_absolute(
    table_name: &str,
    id: &str,
    line: &str,
    path: &[String],
    is_array: bool,
) -> anyhow::Result<Option<String>> {
    if path.len() >= 2 && path[0] == table_name && path[1] == id {
        if path.len() == 2 {
            return Ok(None);
        }
        return Ok(Some(line.trim_end().to_string()));
    }
    if path.len() >= 2 && is_context_table_name(&path[0]) {
        anyhow::bail!("不能在一个扩展项中修改其他扩展项配置：{line}");
    }

    let mut absolute_path = vec![table_name.to_string(), id.to_string()];
    absolute_path.extend(path.iter().cloned());
    let joined = absolute_path
        .iter()
        .map(|segment| toml_key_segment(segment))
        .collect::<Vec<_>>()
        .join(".");
    if is_array {
        Ok(Some(format!("[[{joined}]]")))
    } else {
        Ok(Some(format!("[{joined}]")))
    }
}

fn toml_path_belongs_to_context_id(path: &[String], table_name: &str, id: &str) -> bool {
    path.len() >= 2 && path[0] == table_name && path[1] == id
}

fn is_context_table_name(table_name: &str) -> bool {
    matches!(table_name, "mcp_servers" | "skills" | "plugins")
}

fn toml_table_path_from_line(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    if trimmed.starts_with("[[") {
        return None;
    }

    let doc_text = format!("{trimmed}\n");
    let doc = doc_text.parse::<DocumentMut>().ok()?;
    single_table_path(doc.as_table())
}

fn toml_array_table_path_from_line(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with("[[") {
        return None;
    }

    let doc_text = format!("{trimmed}\n");
    let doc = doc_text.parse::<DocumentMut>().ok()?;
    single_table_or_array_path(doc.as_table())
}

fn single_table_path(table: &dyn TableLike) -> Option<Vec<String>> {
    let mut entries = table.iter();
    let (key, item) = entries.next()?;
    if entries.next().is_some() {
        return None;
    }

    let mut path = vec![key.to_string()];
    if let Some(child) = item.as_table_like()
        && let Some(mut child_path) = single_table_path(child)
    {
        path.append(&mut child_path);
    }
    Some(path)
}

fn single_table_or_array_path(table: &dyn TableLike) -> Option<Vec<String>> {
    let mut entries = table.iter();
    let (key, item) = entries.next()?;
    if entries.next().is_some() {
        return None;
    }

    let mut path = vec![key.to_string()];
    if item.as_array_of_tables().is_some() {
        return Some(path);
    }
    if let Some(child) = item.as_table_like()
        && let Some(mut child_path) = single_table_or_array_path(child)
    {
        path.append(&mut child_path);
    }
    Some(path)
}

fn context_doc_entry_enabled(doc: &DocumentMut, table_name: &str, id: &str) -> bool {
    doc.get(table_name)
        .and_then(Item::as_table)
        .and_then(|table| table.get(id))
        .and_then(Item::as_table)
        .map(context_entry_enabled)
        .unwrap_or(true)
}

fn upsert_context_text_block(contents: &str, table_name: &str, id: &str, section: &str) -> String {
    let mut lines = normalized_lines(contents);
    let section_lines = normalized_section_lines(section);
    if section_lines.is_empty() {
        return finish_toml_lines(lines);
    }

    let blocks = matching_context_text_blocks(contents, table_name, id);
    if !blocks.is_empty() {
        let original_insert_at = blocks
            .iter()
            .find(|block| block.is_root)
            .or_else(|| blocks.first())
            .map(|block| block.start)
            .unwrap_or(lines.len());
        let insert_at = remove_context_text_blocks(&mut lines, &blocks, original_insert_at);
        lines.splice(insert_at..insert_at, section_lines.clone());
        ensure_context_block_spacing(&mut lines, insert_at, section_lines.len());
        return finish_toml_lines(lines);
    }

    let insert_at = context_text_blocks(contents)
        .into_iter()
        .filter(|block| block.table_name == table_name)
        .map(|block| block.end)
        .last()
        .unwrap_or_else(|| {
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            lines.len()
        });

    lines.splice(insert_at..insert_at, section_lines.clone());
    ensure_context_block_spacing(&mut lines, insert_at, section_lines.len());
    finish_toml_lines(lines)
}

fn delete_context_text_block(contents: &str, table_name: &str, id: &str) -> String {
    let mut lines = normalized_lines(contents);
    let blocks = matching_context_text_blocks(contents, table_name, id);
    if blocks.is_empty() {
        return finish_toml_lines(lines);
    }

    let first_start = blocks.iter().map(|block| block.start).min().unwrap_or(0);
    let index = remove_context_text_blocks(&mut lines, &blocks, first_start);
    normalize_gap_after_delete(&mut lines, index);
    finish_toml_lines(lines)
}

fn matching_context_text_blocks(
    contents: &str,
    table_name: &str,
    id: &str,
) -> Vec<ContextTextBlock> {
    context_text_blocks(contents)
        .into_iter()
        .filter(|block| block.table_name == table_name && block.id == id)
        .collect()
}

fn remove_context_text_blocks(
    lines: &mut Vec<String>,
    blocks: &[ContextTextBlock],
    insert_at: usize,
) -> usize {
    let mut adjusted_insert_at = insert_at;
    let mut ranges = blocks
        .iter()
        .map(|block| (block.start, block.end))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges.dedup();

    for (start, end) in ranges.into_iter().rev() {
        lines.drain(start..end);
        if start < adjusted_insert_at {
            adjusted_insert_at -= end - start;
        }
    }

    adjusted_insert_at
}

fn normalized_section_lines(section: &str) -> Vec<String> {
    section
        .trim()
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

fn ensure_context_block_spacing(lines: &mut Vec<String>, mut start: usize, block_len: usize) {
    if block_len == 0 {
        return;
    }

    if start > 0 {
        let mut blank_count = 0;
        while start > blank_count && lines[start - blank_count - 1].trim().is_empty() {
            blank_count += 1;
        }
        if blank_count == 0 {
            lines.insert(start, String::new());
            start += 1;
        } else if blank_count > 1 {
            lines.drain(start - blank_count..start - 1);
            start -= blank_count - 1;
        }
    }

    let end = start + block_len;
    if end < lines.len() {
        let mut blank_count = 0;
        while end + blank_count < lines.len() && lines[end + blank_count].trim().is_empty() {
            blank_count += 1;
        }
        if blank_count == 0 {
            lines.insert(end, String::new());
        } else if blank_count > 1 {
            lines.drain(end + 1..end + blank_count);
        }
    }
}

fn normalize_gap_after_delete(lines: &mut Vec<String>, mut index: usize) {
    if lines.is_empty() {
        return;
    }

    if index == 0 {
        while lines.first().is_some_and(|line| line.trim().is_empty()) {
            lines.remove(0);
        }
        return;
    }

    if index >= lines.len() {
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        return;
    }

    while index > 0 && lines[index - 1].trim().is_empty() {
        lines.remove(index - 1);
        index -= 1;
    }
    while index < lines.len() && lines[index].trim().is_empty() {
        lines.remove(index);
    }
    if index > 0 && index < lines.len() {
        lines.insert(index, String::new());
    }
}

fn finish_toml_lines(mut lines: Vec<String>) -> String {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let contents = lines.join("\n");
    normalize_text_toml_preserving_layout(&contents)
}

fn normalize_text_toml_preserving_layout(contents: &str) -> String {
    if contents.trim().is_empty() {
        String::new()
    } else {
        ensure_trailing_newline(contents.to_string())
    }
}

fn write_codex_live_atomic(
    home: &Path,
    config_text: Option<&str>,
    auth_bytes: Option<&[u8]>,
    preserve_computer_use_guard: bool,
) -> anyhow::Result<Option<String>> {
    std::fs::create_dir_all(home)?;
    let config_path = home.join("config.toml");
    let auth_path = home.join("auth.json");
    #[cfg(windows)]
    let guarded_config_text = match config_text {
        Some(config_text) if preserve_computer_use_guard => {
            let notify_exe = crate::computer_use_guard::find_computer_use_notify_exe(home);
            let marketplace_path =
                crate::computer_use_guard::ensure_openai_bundled_marketplace(home)?;
            let guarded = if let Some(marketplace_path) = marketplace_path.as_deref() {
                crate::computer_use_guard::guard_config_text_with_marketplace(
                    config_text,
                    notify_exe.as_deref(),
                    Some(marketplace_path),
                )?
            } else {
                crate::computer_use_guard::guard_config_text(config_text, notify_exe.as_deref())?
            };
            Some(guarded)
        }
        Some(config_text) => Some(config_text.to_string()),
        None => None,
    };
    #[cfg(windows)]
    let config_text = guarded_config_text.as_deref();

    let config_text = match config_text {
        Some(config_text) => Some(
            crate::plugin_marketplace::preserve_openai_curated_remote_marketplace_config(
                home,
                config_text,
            )?,
        ),
        None => None,
    };
    let config_text = config_text.as_deref();

    if let Some(config_text) = config_text {
        validate_toml_config(config_text, &config_path)?;
    }
    if let Some(auth_bytes) = auth_bytes {
        validate_auth_json(auth_bytes, &auth_path)?;
    }

    let old_config = read_optional_bytes(&config_path)?;
    let old_auth = read_optional_bytes(&auth_path)?;

    // 幂等跳过：待写入内容与磁盘现有最终字节完全一致时，不重写也不备份，
    // 避免每次启动都覆盖用户 config.toml 并堆积大量备份目录。
    // 注：Windows 下 config_text 已经过 computer_use_guard 改写，此处比较的即最终落盘字节。
    let config_unchanged = match config_text {
        Some(config_text) => old_config.as_deref() == Some(config_text.as_bytes()),
        None => true,
    };
    let auth_unchanged = match auth_bytes {
        Some(auth_bytes) => old_auth.as_deref() == Some(auth_bytes),
        None => true,
    };
    if config_unchanged && auth_unchanged {
        return Ok(None);
    }

    let backup_path = create_live_backup(home, old_config.as_deref(), old_auth.as_deref())?;
    let mut auth_written = false;

    if let Some(auth_bytes) = auth_bytes {
        if let Err(error) = crate::settings::atomic_write(&auth_path, auth_bytes) {
            return Err(error.context("写入 auth.json 失败"));
        }
        auth_written = true;
    }

    if let Some(config_text) = config_text {
        if let Err(error) = crate::settings::atomic_write(&config_path, config_text.as_bytes()) {
            if auth_written {
                let _ = restore_optional_file(&auth_path, old_auth.as_deref());
            }
            let _ = restore_optional_file(&config_path, old_config.as_deref());
            return Err(error.context("写入 config.toml 失败"));
        }
    }

    Ok(backup_path)
}

fn active_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(ToString::to_string)
}

fn active_or_default_provider_id(doc: &DocumentMut) -> String {
    active_provider_id(doc)
        .filter(|provider| {
            is_custom_provider_id(provider) && !LEGACY_RELAY_PROVIDERS.contains(&provider.as_str())
        })
        .unwrap_or_else(|| RELAY_PROVIDER.to_string())
}

fn is_custom_provider_id(provider: &str) -> bool {
    !provider.is_empty() && !RESERVED_MODEL_PROVIDER_IDS.contains(&provider)
}

fn provider_table_exists(doc: &DocumentMut, provider_id: &str) -> bool {
    doc.get("model_providers")
        .and_then(Item::as_table)
        .and_then(|table| table.get(provider_id))
        .is_some()
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .with_context(|| "config.toml TOML 解析失败")
    }
}

fn remove_provider_specific_common_keys(table: &mut dyn TableLike) {
    for key in [
        "model",
        "model_provider",
        "base_url",
        "model_catalog_json",
        CHAT_UPSTREAM_BASE_URL_KEY,
    ] {
        table.remove(key);
    }
    table.remove("model_providers");
}

fn sanitize_common_config_text_fallback(common_config: &str) -> String {
    let mut kept = Vec::new();
    let mut in_root = true;
    let mut skipping_model_providers = false;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            skipping_model_providers =
                trimmed == "[model_providers]" || trimmed.starts_with("[model_providers.");
            if skipping_model_providers {
                continue;
            }
        } else if skipping_model_providers {
            continue;
        }

        if in_root {
            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                if matches!(
                    key,
                    "model"
                        | "model_provider"
                        | "base_url"
                        | "model_catalog_json"
                        | CHAT_UPSTREAM_BASE_URL_KEY
                ) {
                    continue;
                }
            }
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

fn normalize_text_toml(contents: String) -> String {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        ensure_trailing_newline(trimmed.to_string())
    }
}

pub fn normalize_config_text(contents: &str) -> String {
    normalize_duplicate_toml_text(contents)
}

fn normalize_duplicate_toml_text(contents: &str) -> String {
    let mut seen_root_keys = HashSet::new();
    let mut seen_headers = HashSet::new();
    let mut kept = Vec::new();
    let mut skipping_duplicate_table = false;
    let mut in_root = true;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            // [[array.table]] 是 TOML 数组表：多个同名元素合法且各自独立，
            // 绝不能当作“重复表”去重，否则会误删 skills.config / hooks 等数组元素。
            if trimmed.starts_with("[[") {
                skipping_duplicate_table = false;
                kept.push(line);
                continue;
            }
            skipping_duplicate_table = !seen_headers.insert(trimmed.to_string());
            if skipping_duplicate_table {
                continue;
            }
            kept.push(line);
            continue;
        }

        if skipping_duplicate_table {
            continue;
        }

        if in_root
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            if !key.is_empty() && !key.contains('.') && !seen_root_keys.insert(key.to_string()) {
                continue;
            }
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

fn strip_common_config_text_fallback(config_text: &str, common_config: &str) -> String {
    let normalized = normalize_duplicate_toml_text(config_text);
    let anchors = common_config_anchors(common_config);
    if anchors.root_keys.is_empty() && anchors.table_headers.is_empty() {
        return normalized;
    }

    let mut kept = Vec::new();
    let mut skipping_table = false;

    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            skipping_table = anchors.table_headers.contains(trimmed);
            if skipping_table {
                continue;
            }
            kept.push(line);
            continue;
        }

        if skipping_table {
            continue;
        }

        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some((key, _)) = trimmed.split_once('=')
            && anchors.root_keys.contains(key.trim())
        {
            continue;
        }

        kept.push(line);
    }

    normalize_text_toml(kept.join("\n"))
}

struct CommonConfigAnchors {
    root_keys: HashSet<String>,
    table_headers: HashSet<String>,
}

fn common_config_anchors(common_config: &str) -> CommonConfigAnchors {
    let mut root_keys = HashSet::new();
    let mut table_headers = HashSet::new();
    let mut in_root = true;

    for line in common_config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_root = false;
            table_headers.insert(trimmed.to_string());
            continue;
        }

        if in_root
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            if !key.is_empty() {
                root_keys.insert(key.to_string());
            }
        }
    }

    CommonConfigAnchors {
        root_keys,
        table_headers,
    }
}

fn validate_toml_config(config_text: &str, path: &Path) -> anyhow::Result<()> {
    if config_text.trim().is_empty() {
        return Ok(());
    }
    config_text
        .parse::<toml::Table>()
        .with_context(|| format!("{} 不是有效 TOML", path.display()))?;
    Ok(())
}

fn validate_auth_json(auth_bytes: &[u8], path: &Path) -> anyhow::Result<()> {
    if auth_bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    serde_json::from_slice::<Value>(auth_bytes)
        .with_context(|| format!("{} 不是有效 JSON", path.display()))?;
    Ok(())
}

fn parse_optional_positive_u64(value: &str, label: &str) -> anyhow::Result<Option<u64>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed
        .parse::<u64>()
        .with_context(|| format!("{label}必须是正整数"))?;
    if parsed == 0 {
        anyhow::bail!("{label}必须大于 0");
    }
    Ok(Some(parsed))
}

fn apply_context_limits_to_config(
    config_text: &str,
    context_window: &str,
    auto_compact_limit: &str,
) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_text)?;
    if let Some(value) = parse_optional_positive_u64(context_window, "上下文大小")? {
        doc["model_context_window"] = toml_edit::value(value as i64);
    }
    if let Some(value) = parse_optional_positive_u64(auto_compact_limit, "压缩上下文大小")? {
        doc["model_auto_compact_token_limit"] = toml_edit::value(value as i64);
    }
    Ok(normalize_optional_toml(doc))
}

fn apply_generated_model_catalog_to_config(
    home: &Path,
    config_text: &str,
    profile: &RelayProfile,
) -> anyhow::Result<String> {
    let rows = relay_profile_catalog_rows(profile);
    let mut doc = parse_toml_document(config_text)?;
    if rows.is_empty() {
        doc.as_table_mut().remove("model_catalog_json");
        return Ok(normalize_optional_toml(doc));
    }

    doc["model_catalog_json"] = toml_edit::value(GENERATED_MODEL_CATALOG_FILENAME);
    // 已挂接 catalog 后，per-model 上下文大小由 catalog 提供；
    // 顶层 model_context_window 会全局覆盖所有模型（导致如 Claude 1m 被压成激活模型的值），
    // 因此在 catalog 模式下移除顶层覆盖，让各模型使用各自的 context_window。
    doc.as_table_mut().remove("model_context_window");
    let config_with_catalog = normalize_optional_toml(doc);
    let catalog = generated_model_catalog_json(profile, &config_with_catalog, rows)?;
    let path = home.join(GENERATED_MODEL_CATALOG_FILENAME);
    std::fs::create_dir_all(home)?;
    let bytes = serde_json::to_vec_pretty(&catalog)?;
    crate::settings::atomic_write(&path, &bytes).context("写入模型目录失败")?;
    Ok(config_with_catalog)
}

/// 从打包的 codex-models.json 中按模型 slug 取出 fast 能力字段。
///
/// 仅当该模型确实声明了 `service_tiers`（含 priority）时返回 `Some`，
/// 用于让生成的 catalog 与打包数据保持一致，避免把所有模型的 fast 能力抹平为 `[]`。
/// 返回 `(service_tiers, additional_speed_tiers)`，均为 JSON 数组。
fn fast_service_tier_capability(slug: &str) -> Option<(Value, Value)> {
    let slug = slug.trim();
    if slug.is_empty() {
        return None;
    }
    let source = serde_json::from_str::<Value>(include_str!("../assets/codex-models.json")).ok()?;
    let models = source.get("models").and_then(Value::as_array)?;
    let model = models
        .iter()
        .find(|model| model.get("slug").and_then(Value::as_str) == Some(slug))?;
    let service_tiers = model.get("service_tiers").and_then(Value::as_array)?;
    let has_priority = service_tiers
        .iter()
        .any(|tier| tier.get("id").and_then(Value::as_str) == Some("priority"));
    if !has_priority {
        return None;
    }
    let speed_tiers = model
        .get("additional_speed_tiers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some((
        Value::Array(service_tiers.clone()),
        Value::Array(speed_tiers),
    ))
}

const GENERATED_CATALOG_FALLBACK_BASE_INSTRUCTIONS: &str = "You are Codex, a coding agent. You and the user share one workspace, and your job is to collaborate with them until their goal is genuinely handled.";

fn rewrite_generated_catalog_prompt_text_for_model(text: &str, model: &str) -> String {
    let text = replace_gpt_identity_phrases_with_model(text, model);
    replace_gpt_identity_tokens_with_model(&text, model)
}

fn replace_gpt_identity_phrases_with_model(text: &str, model: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    let model = prompt_model_name(model);
    let replacement = format!(" based on the {model} model");

    while let Some((start, pattern_len)) = find_next_gpt_identity_phrase(&lower, index) {
        result.push_str(&text[index..start]);
        let end = consume_optional_model_word(
            text,
            consume_gpt_identity_suffix(text, start + pattern_len),
        );
        result.push_str(&replacement);
        index = end;
    }

    result.push_str(&text[index..]);
    result
}

fn find_next_gpt_identity_phrase(lower: &str, from: usize) -> Option<(usize, usize)> {
    [" based on gpt", " based on the gpt"]
        .into_iter()
        .filter_map(|pattern| {
            lower[from..]
                .find(pattern)
                .map(|offset| (from + offset, pattern.len()))
        })
        .min_by_key(|(start, _)| *start)
}

fn consume_gpt_identity_suffix(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut cursor = from;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            cursor += 1;
        } else if byte == b'.'
            && cursor + 1 < bytes.len()
            && bytes[cursor + 1].is_ascii_alphanumeric()
        {
            cursor += 1;
        } else {
            break;
        }
    }

    cursor
}

fn consume_optional_model_word(text: &str, from: usize) -> usize {
    let candidate = text.get(from..from + 6).unwrap_or_default();
    if candidate.eq_ignore_ascii_case(" model") {
        from + 6
    } else {
        from
    }
}

fn replace_gpt_identity_tokens_with_model(text: &str, model: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    let model = prompt_model_name(model);

    while let Some(offset) = lower[index..].find("gpt") {
        let start = index + offset;
        result.push_str(&text[index..start]);
        if is_gpt_identity_token_start(text, start) {
            let end = consume_gpt_identity_suffix(text, start + 3);
            result.push_str(&model);
            index = end;
        } else {
            result.push_str(&text[start..start + 3]);
            index = start + 3;
        }
    }

    result.push_str(&text[index..]);
    result
}

fn prompt_model_name(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        "Codex".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_gpt_identity_token_start(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    if start > 0 {
        let previous = bytes[start - 1];
        if previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'-' {
            return false;
        }
    }

    let Some(next) = bytes.get(start + 3).copied() else {
        return false;
    };
    next.is_ascii_digit() || next == b'-' || next == b'_'
}

fn rewrite_generated_catalog_prompt_value_for_model(value: Value, model: &str) -> Value {
    match value {
        Value::String(text) => Value::String(rewrite_generated_catalog_prompt_text_for_model(
            &text, model,
        )),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| rewrite_generated_catalog_prompt_value_for_model(value, model))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        rewrite_generated_catalog_prompt_value_for_model(value, model),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

fn generated_catalog_prompt_fields(profile: &RelayProfile) -> (String, Value, bool) {
    let override_prompt = profile.system_prompt_override.trim();
    if !override_prompt.is_empty() {
        let base_instructions = override_prompt.to_string();
        return (
            base_instructions.clone(),
            json!({
                "instructions_template": base_instructions,
                "instructions_variables": {
                    "personality_default": "",
                    "personality_pragmatic": ""
                }
            }),
            false,
        );
    }

    let source = serde_json::from_str::<Value>(include_str!("../assets/codex-models.json")).ok();
    let first_model = source
        .as_ref()
        .and_then(|value| value.get("models"))
        .and_then(Value::as_array)
        .and_then(|models| models.first());
    let base_instructions = first_model
        .and_then(|model| model.get("base_instructions"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| GENERATED_CATALOG_FALLBACK_BASE_INSTRUCTIONS.to_string());
    let model_messages = first_model
        .and_then(|model| model.get("model_messages"))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "instructions_template": base_instructions.clone(),
                "instructions_variables": {
                    "personality_default": "",
                    "personality_pragmatic": ""
                }
            })
        });
    (base_instructions, model_messages, true)
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogModelRow {
    model: String,
    context_window: String,
    protocol: RelayProtocol,
}

pub(crate) fn generated_model_catalog_json(
    profile: &RelayProfile,
    config_text: &str,
    rows: Vec<CatalogModelRow>,
) -> anyhow::Result<Value> {
    let reasoning_effort = catalog_reasoning_effort(config_text);
    let auto_compact_limit =
        parse_optional_positive_u64(&profile.auto_compact_limit, "压缩上下文大小")?;
    let (base_instructions, model_messages, rewrite_prompt_model) =
        generated_catalog_prompt_fields(profile);
    let mut models = Vec::new();

    for (index, row) in rows.into_iter().enumerate() {
        let model = row.model;
        let model_base_instructions = if rewrite_prompt_model {
            rewrite_generated_catalog_prompt_text_for_model(&base_instructions, &model)
        } else {
            base_instructions.clone()
        };
        let model_messages = if rewrite_prompt_model {
            rewrite_generated_catalog_prompt_value_for_model(model_messages.clone(), &model)
        } else {
            model_messages.clone()
        };
        let fast_capability = fast_service_tier_capability(&model);
        let protocol = upstream_response_protocol_for_relay(row.protocol);
        let supported_reasoning_levels =
            crate::protocol_proxy::supported_reasoning_efforts_for_model(&model, protocol);
        let default_reasoning_level = catalog_default_reasoning_effort(
            &model,
            &reasoning_effort,
            &supported_reasoning_levels,
        );
        let context_window = catalog_context_window_for_model(&model, &row.context_window);
        let mut entry = Map::new();
        entry.insert("slug".to_string(), json!(model.clone()));
        entry.insert("display_name".to_string(), json!(model.clone()));
        entry.insert("description".to_string(), json!(model));
        entry.insert("priority".to_string(), json!(1000 + index as u64));
        entry.insert("visibility".to_string(), json!("list"));
        entry.insert("supported_in_api".to_string(), json!(true));
        entry.insert(
            "base_instructions".to_string(),
            json!(model_base_instructions),
        );
        entry.insert("model_messages".to_string(), model_messages.clone());
        // codex 解析 model_catalog_json 时 shell_type 为必填字段，缺失会导致整份目录解析失败
        entry.insert("shell_type".to_string(), json!("shell_command"));
        entry.insert("apply_patch_tool_type".to_string(), json!("freeform"));
        entry.insert("web_search_tool_type".to_string(), json!("text_and_image"));
        entry.insert(
            "additional_speed_tiers".to_string(),
            fast_capability
                .as_ref()
                .map(|(_, speed)| speed.clone())
                .unwrap_or_else(|| json!([])),
        );
        entry.insert("availability_nux".to_string(), Value::Null);
        entry.insert("default_verbosity".to_string(), json!("low"));
        entry.insert("effective_context_window_percent".to_string(), json!(95));
        entry.insert("experimental_supported_tools".to_string(), json!([]));
        entry.insert("input_modalities".to_string(), json!(["text", "image"]));
        entry.insert(
            "service_tiers".to_string(),
            fast_capability
                .as_ref()
                .map(|(tiers, _)| tiers.clone())
                .unwrap_or_else(|| json!([])),
        );
        entry.insert("support_verbosity".to_string(), json!(true));
        entry.insert("supports_image_detail_original".to_string(), json!(true));
        entry.insert("supports_parallel_tool_calls".to_string(), json!(true));
        entry.insert("supports_reasoning_summaries".to_string(), json!(true));
        entry.insert("supports_search_tool".to_string(), json!(true));
        entry.insert("default_reasoning_summary".to_string(), json!("none"));
        entry.insert(
            "truncation_policy".to_string(),
            json!({ "mode": "tokens", "limit": 10000 }),
        );
        entry.insert("upgrade".to_string(), Value::Null);
        entry.insert("use_responses_lite".to_string(), json!(false));
        entry.insert(
            "default_reasoning_level".to_string(),
            json!(default_reasoning_level),
        );
        entry.insert(
            "supported_reasoning_levels".to_string(),
            json!(
                supported_reasoning_levels
                    .iter()
                    .map(|effort| json!({
                        "effort": effort,
                        "description": reasoning_effort_description(effort)
                    }))
                    .collect::<Vec<_>>()
            ),
        );

        if let Some(value) = parse_optional_positive_u64(&context_window, "上下文大小")? {
            entry.insert("context_window".to_string(), json!(value));
            entry.insert("max_context_window".to_string(), json!(value));
        }
        if let Some(value) = auto_compact_limit {
            entry.insert("auto_compact_token_limit".to_string(), json!(value));
        }

        models.push(Value::Object(entry));
    }

    Ok(json!({ "models": models }))
}

pub(crate) fn relay_profile_catalog_rows(profile: &RelayProfile) -> Vec<CatalogModelRow> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    if !profile.model_mappings.is_empty() {
        for mapping in &profile.model_mappings {
            let model = mapping.request_model.trim();
            if model.is_empty() || !seen.insert(model.to_string()) {
                continue;
            }
            rows.push(CatalogModelRow {
                model: model.to_string(),
                context_window: mapping.context_window.trim().to_string(),
                protocol: mapping.protocol,
            });
        }
        return rows;
    }

    let active_model = relay_profile_model(profile);
    if !profile.system_prompt_override.trim().is_empty() && !active_model.trim().is_empty() {
        rows.push(CatalogModelRow {
            model: active_model.trim().to_string(),
            context_window: profile.context_window_for_active_model(),
            protocol: profile.protocol,
        });
        seen.insert(active_model.trim().to_string());
    }

    for (protocol, models) in [
        (
            RelayProtocol::Responses,
            crate::model_catalog::relay_profile_responses_model_ids(profile),
        ),
        (
            RelayProtocol::ChatCompletions,
            crate::model_catalog::relay_profile_chat_completions_model_ids(profile),
        ),
        (
            RelayProtocol::Anthropic,
            crate::model_catalog::relay_profile_anthropic_model_ids(profile),
        ),
    ] {
        for model in models {
            let model = model.trim().to_string();
            if model.is_empty() || !seen.insert(model.clone()) {
                continue;
            }
            rows.push(CatalogModelRow {
                model,
                context_window: String::new(),
                protocol,
            });
        }
    }
    rows
}

fn upstream_response_protocol_for_relay(
    protocol: RelayProtocol,
) -> crate::protocol_proxy::UpstreamResponseProtocol {
    match protocol {
        RelayProtocol::Responses => crate::protocol_proxy::UpstreamResponseProtocol::Responses,
        RelayProtocol::ChatCompletions => {
            crate::protocol_proxy::UpstreamResponseProtocol::ChatCompletions
        }
        RelayProtocol::Anthropic => crate::protocol_proxy::UpstreamResponseProtocol::Anthropic,
    }
}

fn catalog_default_reasoning_effort(
    model: &str,
    configured: &str,
    supported: &[&'static str],
) -> &'static str {
    if model_prefers_max_reasoning_default(model) && supported.iter().any(|effort| *effort == "max")
    {
        return "max";
    }

    let configured = configured.trim();
    if supported.iter().any(|effort| *effort == configured) {
        return supported
            .iter()
            .copied()
            .find(|effort| *effort == configured)
            .unwrap_or("medium");
    }
    for fallback in ["high", "medium", "low", "minimal"] {
        if supported.iter().any(|effort| *effort == fallback) {
            return fallback;
        }
    }
    supported.first().copied().unwrap_or("medium")
}

fn catalog_context_window_for_model(model: &str, configured: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    default_catalog_context_window(model)
        .unwrap_or("")
        .to_string()
}

fn default_catalog_context_window(model: &str) -> Option<&'static str> {
    let normalized = model.trim().to_ascii_lowercase();
    let model = normalized
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(normalized.as_str());
    if model == "gpt-5.4" || model == "gpt-5.6" || model.starts_with("gpt-5.6-") {
        return Some("1000000");
    }
    None
}

fn model_prefers_max_reasoning_default(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("deepseek")
        || model.contains("glm")
        || model.contains("zhipu")
        || model.contains("z.ai")
}

fn reasoning_effort_description(effort: &str) -> &'static str {
    match effort {
        "minimal" => "Minimal reasoning",
        "low" => "Low reasoning",
        "medium" => "Medium reasoning",
        "high" => "High reasoning",
        "xhigh" => "Extra high reasoning",
        "max" => "Max reasoning",
        _ => "Reasoning",
    }
}

pub(crate) fn catalog_reasoning_effort(config_text: &str) -> String {
    match root_key_string(config_text, "model_reasoning_effort")
        .unwrap_or_default()
        .trim()
    {
        "minimal" => "minimal".to_string(),
        "low" => "low".to_string(),
        "medium" => "medium".to_string(),
        "high" => "high".to_string(),
        "xhigh" => "xhigh".to_string(),
        "max" => "max".to_string(),
        _ => "medium".to_string(),
    }
}

fn sync_context_limits_from_config(profile: &mut RelayProfile, config_text: &str) {
    if let Some(value) = root_positive_int_string(config_text, "model_context_window") {
        profile.context_window = value;
    }
    if let Some(value) = root_positive_int_string(config_text, "model_auto_compact_token_limit") {
        profile.auto_compact_limit = value;
    }
}

fn root_positive_int_string(config_text: &str, key: &str) -> Option<String> {
    if let Ok(doc) = parse_toml_document(config_text) {
        if let Some(value) = doc
            .get(key)
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_integer)
            .filter(|value| *value > 0)
        {
            return Some(value.to_string());
        }
    }

    root_key_value(config_text, key)
        .and_then(|value| value.split('#').next())
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
}

fn toml_value_is_subset(target: &toml_edit::Value, source: &toml_edit::Value) -> bool {
    match (target, source) {
        (toml_edit::Value::String(target), toml_edit::Value::String(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Integer(target), toml_edit::Value::Integer(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Float(target), toml_edit::Value::Float(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Boolean(target), toml_edit::Value::Boolean(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Datetime(target), toml_edit::Value::Datetime(source)) => {
            target.value() == source.value()
        }
        (toml_edit::Value::Array(target), toml_edit::Value::Array(source)) => {
            toml_array_contains_subset(target, source)
        }
        (toml_edit::Value::InlineTable(target), toml_edit::Value::InlineTable(source)) => {
            source.iter().all(|(key, source_item)| {
                target
                    .get(key)
                    .is_some_and(|target_item| toml_value_is_subset(target_item, source_item))
            })
        }
        _ => false,
    }
}

fn toml_array_contains_subset(target: &toml_edit::Array, source: &toml_edit::Array) -> bool {
    let mut matched = vec![false; target.len()];
    let target_items: Vec<&toml_edit::Value> = target.iter().collect();

    source.iter().all(|source_item| {
        if let Some((index, _)) = target_items
            .iter()
            .enumerate()
            .find(|(index, target_item)| {
                !matched[*index] && toml_value_is_subset(target_item, source_item)
            })
        {
            matched[index] = true;
            true
        } else {
            false
        }
    })
}

fn toml_remove_array_items(target: &mut toml_edit::Array, source: &toml_edit::Array) {
    for source_item in source.iter() {
        let index = {
            let target_items: Vec<&toml_edit::Value> = target.iter().collect();
            target_items
                .iter()
                .enumerate()
                .find(|(_, target_item)| toml_value_is_subset(target_item, source_item))
                .map(|(index, _)| index)
        };

        if let Some(index) = index {
            target.remove(index);
        }
    }
}

fn merge_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            merge_toml_table_like(target_table, source_table);
            return;
        }
    }

    *target = source.clone();
}

fn merge_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        match target.get_mut(key) {
            Some(target_item) => merge_toml_item(target_item, source_item),
            None => {
                target.insert(key, source_item.clone());
            }
        }
    }
}

fn remove_toml_item(target: &mut Item, source: &Item) {
    if let Some(source_table) = source.as_table_like() {
        if let Some(target_table) = target.as_table_like_mut() {
            remove_toml_table_like(target_table, source_table);
            if target_table.is_empty() {
                *target = Item::None;
            }
            return;
        }
    }

    if let Some(source_value) = source.as_value() {
        let mut remove_item = false;

        if let Some(target_value) = target.as_value_mut() {
            match (target_value, source_value) {
                (toml_edit::Value::Array(target_arr), toml_edit::Value::Array(source_arr)) => {
                    toml_remove_array_items(target_arr, source_arr);
                    remove_item = target_arr.is_empty();
                }
                (target_value, source_value)
                    if toml_value_is_subset(target_value, source_value) =>
                {
                    remove_item = true;
                }
                _ => {}
            }
        }

        if remove_item {
            *target = Item::None;
        }
    }
}

fn remove_toml_table_like(target: &mut dyn TableLike, source: &dyn TableLike) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_string()).collect();

    for key in keys {
        let mut remove_key = false;
        if let (Some(target_item), Some(source_item)) = (target.get_mut(&key), source.get(&key)) {
            remove_toml_item(target_item, source_item);
            remove_key = target_item.is_none()
                || target_item
                    .as_table_like()
                    .is_some_and(|table_like| table_like.is_empty());
        }

        if remove_key {
            target.remove(&key);
        }
    }
}

fn normalize_optional_toml(doc: DocumentMut) -> String {
    let contents = doc.to_string();
    if contents.trim().is_empty() {
        String::new()
    } else {
        ensure_trailing_newline(contents)
    }
}

fn list_context_entries_for_table(
    contents: &str,
    doc: &DocumentMut,
    table_name: &str,
) -> Vec<CodexContextEntry> {
    let Some(table) = doc.get(table_name).and_then(Item::as_table) else {
        return Vec::new();
    };
    let mut seen_ids = HashSet::new();
    let mut entries = Vec::new();

    for block in context_text_blocks(contents)
        .into_iter()
        .filter(|block| block.table_name == table_name && block.is_root)
    {
        if !seen_ids.insert(block.id.clone()) {
            continue;
        }
        let Some(table) = table.get(&block.id).and_then(Item::as_table) else {
            continue;
        };
        if table.is_implicit() {
            continue;
        }
        let body = context_block_body_text(&block);
        entries.push(CodexContextEntry {
            id: block.id.clone(),
            kind: context_kind_name(table_name).to_string(),
            title: block.id,
            summary: context_entry_summary(&body),
            toml_body: body,
            enabled: context_entry_enabled(table),
        });
    }

    entries.extend(table.iter().filter_map(|(id, item)| {
        if seen_ids.contains(id) {
            return None;
        }
        let table = item.as_table()?;
        if table.is_implicit() {
            return None;
        }
        let body = table_body_to_string(table);
        Some(CodexContextEntry {
            id: id.to_string(),
            kind: context_kind_name(table_name).to_string(),
            title: id.to_string(),
            summary: context_entry_summary(&body),
            toml_body: body,
            enabled: context_entry_enabled(table),
        })
    }));
    entries
}

fn table_body_to_string(table: &Table) -> String {
    let mut doc = DocumentMut::new();
    merge_toml_table_like(doc.as_table_mut(), table);
    normalize_optional_toml(doc)
}

fn context_table_name(kind: &str) -> anyhow::Result<&'static str> {
    match kind {
        "mcp" | "mcpServer" | "mcpServers" => Ok("mcp_servers"),
        "skill" | "skills" => Ok("skills"),
        "plugin" | "plugins" => Ok("plugins"),
        other => anyhow::bail!("未知上下文类型：{other}"),
    }
}

fn context_kind_name(table: &str) -> &'static str {
    match table {
        "mcp_servers" => "mcp",
        "skills" => "skill",
        "plugins" => "plugin",
        _ => "unknown",
    }
}

fn context_entry_summary(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(96)
        .collect()
}

fn context_entry_enabled(table: &Table) -> bool {
    if table
        .get("enabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|enabled| !enabled)
    {
        return false;
    }
    if table
        .get("disabled")
        .and_then(|value| value.as_bool())
        .is_some_and(|disabled| disabled)
    {
        return false;
    }
    true
}

fn set_provider_id(doc: &mut DocumentMut, provider_id: &str) {
    doc["model_provider"] = toml_edit::value(provider_id);
}

fn restore_profile_provider_id_for_backfill(
    live_config: &str,
    template_config: &str,
) -> anyhow::Result<String> {
    let Some(template_provider_id) = provider_id_with_table_from_config(template_config)? else {
        return Ok(ensure_trailing_newline(live_config.to_string()));
    };
    if live_config.trim().is_empty() {
        return Ok(ensure_trailing_newline(live_config.to_string()));
    }

    let mut doc = parse_toml_document(live_config)?;
    let Some(live_provider_id) = active_provider_id(&doc) else {
        return Ok(ensure_trailing_newline(doc.to_string()));
    };
    if live_provider_id == template_provider_id {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }
    if live_provider_id != RELAY_PROVIDER || template_provider_id == RELAY_PROVIDER {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }
    if !provider_table_exists(&doc, &live_provider_id) {
        return Ok(ensure_trailing_newline(doc.to_string()));
    }

    rename_provider_table(&mut doc, &live_provider_id, &template_provider_id);
    rewrite_profile_provider_refs(&mut doc, &live_provider_id, &template_provider_id);
    set_provider_id(&mut doc, &template_provider_id);
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn provider_id_with_table_from_config(config_text: &str) -> anyhow::Result<Option<String>> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }
    let doc = parse_toml_document(config_text)?;
    let Some(provider_id) = active_provider_id(&doc) else {
        return Ok(None);
    };
    Ok(provider_table_exists(&doc, &provider_id).then_some(provider_id))
}

fn restore_profile_auth_from_live_config(
    profile: &mut RelayProfile,
    template_auth: &str,
) -> anyhow::Result<()> {
    let Some(token) = experimental_bearer_token_from_config(&profile.config_contents)? else {
        return Ok(());
    };
    profile.api_key = token.clone();

    if profile.relay_mode == crate::settings::RelayMode::Official && profile.official_mix_api_key {
        profile.auth_contents = remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
        return Ok(());
    }

    if !profile.auth_contents.trim().is_empty() {
        if codex_auth_api_key(&profile.auth_contents).is_none() {
            return Ok(());
        }
        profile.config_contents =
            remove_experimental_bearer_token_from_config(&profile.config_contents)?;
        return Ok(());
    }

    profile.config_contents =
        remove_experimental_bearer_token_from_config(&profile.config_contents)?;

    let mut auth = if template_auth.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(template_auth).with_context(|| "auth.json JSON 解析失败")?
    };
    if !auth.is_object() {
        auth = json!({});
    }
    if let Some(auth_object) = auth.as_object_mut() {
        auth_object.insert("OPENAI_API_KEY".to_string(), Value::String(token));
    } else {
        anyhow::bail!("auth.json 必须是 JSON 对象");
    }
    profile.auth_contents = serde_json::to_string_pretty(&auth)?;
    Ok(())
}

fn sync_profile_mode_from_backfilled_live(profile: &mut RelayProfile) {
    if profile.relay_mode == crate::settings::RelayMode::Official && !profile.official_mix_api_key {
        return;
    }

    if codex_auth_api_key(&profile.auth_contents)
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        profile.relay_mode = crate::settings::RelayMode::PureApi;
        profile.official_mix_api_key = false;
        return;
    }

    let has_provider_endpoint = provider_string_from_config(&profile.config_contents, "base_url")
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if has_provider_endpoint || !profile.api_key.trim().is_empty() {
        profile.relay_mode = crate::settings::RelayMode::Official;
        profile.official_mix_api_key = true;
    }
}

fn official_profile_auth_for_switch(home: &Path, auth_contents: &str) -> anyhow::Result<String> {
    let source = if auth_contents.trim().is_empty() {
        read_optional_text(&home.join("auth.json"))?
    } else {
        auth_contents.to_string()
    };
    remove_openai_api_key_from_auth_contents(&source)
}

fn codex_auth_api_key(auth_contents: &str) -> Option<String> {
    let auth: Value = serde_json::from_str(auth_contents).ok()?;
    auth.get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}

/// 解析 profile 實際使用的模型：優先取 config.toml 裡的 `model =`，
/// 否則退回 profile.model 欄位。供應商測試用它做回退，避免串到別家供應商的模型名。
pub fn relay_profile_model(profile: &RelayProfile) -> String {
    root_key_string(&profile.config_contents, "model")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.model.trim().to_string())
}

fn relay_profile_base_url(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return crate::protocol_proxy::local_responses_proxy_base_url(
            crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        );
    }
    if profile.local_proxy_enabled() {
        if !profile.upstream_base_url.trim().is_empty() {
            return profile.upstream_base_url.trim().to_string();
        }
        if let Some(value) = root_key_string(&profile.config_contents, CHAT_UPSTREAM_BASE_URL_KEY)
            .filter(|value| !value.trim().is_empty())
        {
            return value;
        }
        if !profile.base_url.trim().is_empty() {
            return profile.base_url.trim().to_string();
        }
    }
    let provider_base_url = provider_string_from_config(&profile.config_contents, "base_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    if profile.local_proxy_enabled()
        && provider_base_url
            == crate::protocol_proxy::local_responses_proxy_base_url(
                crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
            )
    {
        String::new()
    } else if !provider_base_url.is_empty() {
        provider_base_url
    } else {
        profile.base_url.trim().to_string()
    }
}

fn relay_profile_api_key(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return "codex-elves-aggregate".to_string();
    }
    if profile.relay_mode == crate::settings::RelayMode::Official {
        return experimental_bearer_token_from_config(&profile.config_contents)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| profile.api_key.trim().to_string());
    }
    codex_auth_api_key(&profile.auth_contents)
        .or_else(|| {
            experimental_bearer_token_from_config(&profile.config_contents)
                .ok()
                .flatten()
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| profile.api_key.trim().to_string())
}

fn complete_relay_profile_config(profile: &RelayProfile) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(&profile.config_contents)?;
    let provider_id = active_or_default_provider_id(&doc);
    set_provider_id(&mut doc, &provider_id);

    let model = relay_profile_model(profile);
    if !model.trim().is_empty() {
        doc["model"] = toml_edit::value(model.trim());
    }

    let base_url = relay_profile_base_url(profile);
    let api_key = relay_profile_api_key(profile);
    doc.as_table_mut().remove(CHAT_UPSTREAM_BASE_URL_KEY);
    retain_only_provider_table(&mut doc, &provider_id);
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        if provider_id != *legacy_provider {
            remove_provider_table(&mut doc, legacy_provider);
        }
    }
    let provider = ensure_provider_table(&mut doc, &provider_id)?;
    if provider
        .get("name")
        .and_then(Item::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        provider["name"] = toml_edit::value(provider_id.as_str());
    }
    if provider
        .get("wire_api")
        .and_then(Item::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        provider["wire_api"] = toml_edit::value("responses");
    }
    if provider
        .get("requires_openai_auth")
        .and_then(Item::as_bool)
        .is_none()
    {
        provider["requires_openai_auth"] = toml_edit::value(true);
    }
    let provider_base_url = codex_base_url_for_proxy(
        base_url.trim(),
        profile.local_proxy_enabled(),
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    );
    if !provider_base_url.trim().is_empty() {
        provider["base_url"] = toml_edit::value(provider_base_url.trim());
    }
    if profile.relay_mode == crate::settings::RelayMode::PureApi {
        provider.remove("experimental_bearer_token");
    } else if !api_key.trim().is_empty() {
        provider["experimental_bearer_token"] = toml_edit::value(api_key.trim());
    }

    Ok(move_model_providers_before_profiles(
        &ensure_trailing_newline(doc.to_string()),
    ))
}

pub fn normalize_relay_profile_for_storage(profile: &mut RelayProfile) -> anyhow::Result<()> {
    if profile.relay_mode == crate::settings::RelayMode::Official && !profile.official_mix_api_key {
        let has_api_config = !profile.base_url.trim().is_empty()
            || !profile.api_key.trim().is_empty()
            || codex_auth_api_key(&profile.auth_contents).is_some()
            || config_has_model_provider(profile.config_contents.as_str());
        if has_api_config {
            profile.config_contents.clear();
        }
        if !profile.model_list.trim().is_empty() {
            profile.model_list = merge_model_into_model_list(&profile.model, &profile.model_list);
        }
        profile.model.clear();
        profile.base_url.clear();
        profile.upstream_base_url.clear();
        profile.api_key.clear();
        if auth_contents_looks_like_chatgpt_auth(&profile.auth_contents) {
            profile.auth_contents =
                remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
        } else {
            profile.auth_contents.clear();
        }
        return Ok(());
    }
    let provider_base_url = provider_string_from_config(&profile.config_contents, "base_url")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    let local_proxy_base_url = crate::protocol_proxy::local_responses_proxy_base_url(
        crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
    );
    if profile.local_proxy_enabled.is_none() && provider_base_url == local_proxy_base_url {
        profile.local_proxy_enabled = Some(true);
    }
    let source_base_url = relay_profile_base_url(profile);
    let source_api_key = relay_profile_api_key(profile);
    if !profile.config_contents.trim().is_empty()
        || profile.relay_mode == crate::settings::RelayMode::PureApi
        || profile.official_mix_api_key
    {
        profile.config_contents = complete_relay_profile_config(profile)?;
    }
    if profile.relay_mode == crate::settings::RelayMode::PureApi
        && profile.auth_contents.trim().is_empty()
        && !source_api_key.trim().is_empty()
    {
        profile.auth_contents = serde_json::to_string_pretty(&json!({
            "OPENAI_API_KEY": source_api_key.trim()
        }))?;
    }
    if profile.relay_mode == crate::settings::RelayMode::Official {
        profile.auth_contents = remove_openai_api_key_from_auth_contents(&profile.auth_contents)?;
    }
    profile.model = relay_profile_model(profile);
    profile.model_list = merge_model_into_model_list(&profile.model, &profile.model_list);
    profile.upstream_base_url = source_base_url.clone();
    profile.base_url = source_base_url;
    profile.api_key = relay_profile_api_key(profile);
    Ok(())
}

fn remove_openai_api_key_from_auth_contents(auth_contents: &str) -> anyhow::Result<String> {
    if auth_contents.trim().is_empty() {
        return Ok(String::new());
    }
    let mut value =
        serde_json::from_str::<Value>(auth_contents).with_context(|| "auth.json JSON 解析失败")?;
    let Some(object) = value.as_object_mut() else {
        anyhow::bail!("auth.json 必须是 JSON 对象");
    };
    object.remove("OPENAI_API_KEY");
    if object.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

fn merge_model_into_model_list(model: &str, model_list: &str) -> String {
    let model = model.trim();
    let mut models = Vec::new();
    if !model.is_empty() {
        models.push(model.to_string());
    }
    for item in model_list.split(['\r', '\n', ',']).map(str::trim) {
        if !item.is_empty() && !models.iter().any(|existing| existing == item) {
            models.push(item.to_string());
        }
    }
    models.join("\n")
}

fn config_has_model_provider(config_contents: &str) -> bool {
    parse_toml_document(config_contents)
        .ok()
        .and_then(|doc| {
            doc.get("model_provider")
                .and_then(Item::as_str)
                .map(str::to_string)
        })
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn auth_contents_looks_like_chatgpt_auth(contents: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(contents) else {
        return false;
    };
    let is_chatgpt = value
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(|mode| mode.eq_ignore_ascii_case("chatgpt"))
        .unwrap_or(false);
    is_chatgpt
        && value
            .get("tokens")
            .map(tokens_have_login_secret)
            .unwrap_or(false)
}

fn provider_string_from_config(config_contents: &str, key: &str) -> Option<String> {
    let doc = parse_toml_document(config_contents).ok()?;
    let active = active_provider_id(&doc);
    if let Some(provider_id) = active.as_deref() {
        if let Some(value) = doc
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get(provider_id))
            .and_then(Item::as_table)
            .and_then(|provider| provider.get(key))
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }

    for provider in provider_tables(&doc) {
        if let Some(value) = provider
            .get(key)
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn experimental_bearer_token_from_config(config_contents: &str) -> anyhow::Result<Option<String>> {
    let doc = parse_toml_document(config_contents)?;
    if let Some(provider_id) = active_provider_id(&doc) {
        if let Some(token) = doc
            .get("model_providers")
            .and_then(Item::as_table)
            .and_then(|providers| providers.get(&provider_id))
            .and_then(Item::as_table)
            .and_then(|provider| provider.get("experimental_bearer_token"))
            .and_then(Item::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return Ok(Some(token.to_string()));
        }
    }
    Ok(None)
}

fn remove_experimental_bearer_token_from_config(config_contents: &str) -> anyhow::Result<String> {
    let mut doc = parse_toml_document(config_contents)?;
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        for (_, item) in providers.iter_mut() {
            if let Some(provider) = item.as_table_like_mut() {
                provider.remove("experimental_bearer_token");
            }
        }
    }
    Ok(ensure_trailing_newline(doc.to_string()))
}

fn provider_tables(doc: &DocumentMut) -> Vec<&dyn TableLike> {
    let mut tables: Vec<&dyn TableLike> = Vec::new();
    if let Some(providers) = doc.get("model_providers").and_then(Item::as_table) {
        for (_, item) in providers.iter() {
            if let Some(provider) = item.as_table_like() {
                tables.push(provider);
            }
        }
    }
    tables
}

fn ensure_provider_table<'a>(
    doc: &'a mut DocumentMut,
    provider_id: &str,
) -> anyhow::Result<&'a mut Table> {
    let providers = table_mut_or_insert(doc, "model_providers")?;
    if !providers.contains_key(provider_id)
        || providers
            .get(provider_id)
            .and_then(Item::as_table)
            .is_none()
    {
        providers.insert(provider_id, toml_edit::table());
    }
    providers
        .get_mut(provider_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("model_providers.{provider_id} 必须是 TOML table"))
}

fn remove_provider_table(doc: &mut DocumentMut, provider_id: &str) {
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        providers.remove(provider_id);
        if providers.is_empty() {
            doc.as_table_mut().remove("model_providers");
        }
    }
}

fn retain_only_provider_table(doc: &mut DocumentMut, provider_id: &str) {
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        let provider = providers
            .remove(provider_id)
            .unwrap_or_else(toml_edit::table);
        providers.clear();
        providers.insert(provider_id, provider);
    }
}

fn rename_provider_table(doc: &mut DocumentMut, from: &str, to: &str) {
    if from == to {
        return;
    }
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        let moved = providers.remove(from).unwrap_or_else(toml_edit::table);
        providers.insert(to, moved);
    }
}

fn rewrite_profile_provider_refs(doc: &mut DocumentMut, from: &str, to: &str) {
    let Some(profiles) = doc.get_mut("profiles").and_then(Item::as_table_mut) else {
        return;
    };
    for (_, item) in profiles.iter_mut() {
        let Some(profile) = item.as_table_mut() else {
            continue;
        };
        if profile
            .get("model_provider")
            .and_then(Item::as_str)
            .is_some_and(|provider| provider == from)
        {
            profile.insert("model_provider", toml_edit::value(to));
        }
    }
}

fn read_optional_text(path: &Path) -> anyhow::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn read_optional_bytes(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn restore_optional_file(path: &Path, contents: Option<&[u8]>) -> anyhow::Result<()> {
    match contents {
        Some(contents) => crate::settings::atomic_write(path, contents),
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn create_live_backup(
    home: &Path,
    config: Option<&[u8]>,
    auth: Option<&[u8]>,
) -> anyhow::Result<Option<String>> {
    if config.is_none() && auth.is_none() {
        return Ok(None);
    }

    let backup_dir = home
        .join("backups")
        .join(format!("codex-elves-live-{}", timestamp_millis()));
    std::fs::create_dir_all(&backup_dir)?;
    if let Some(config) = config {
        std::fs::write(backup_dir.join("config.toml"), config)?;
    }
    if let Some(auth) = auth {
        std::fs::write(backup_dir.join("auth.json"), auth)?;
    }
    prune_live_backups(home, LIVE_BACKUP_KEEP);
    Ok(Some(backup_dir.to_string_lossy().to_string()))
}

/// 实时备份目录保留份数上限，避免长期运行后 ~/.codex/backups 无限堆积。
const LIVE_BACKUP_KEEP: usize = 20;

/// 清理多余的实时备份目录，仅保留最近的 `keep` 份（按目录名时间戳倒序）。
/// 仅处理 `codex-elves-live-` 前缀目录，其它内容不受影响；任何 IO 错误都静默忽略，不影响主流程。
fn prune_live_backups(home: &Path, keep: usize) {
    let backups_dir = home.join("backups");
    let Ok(entries) = std::fs::read_dir(&backups_dir) else {
        return;
    };
    let mut live_dirs: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("codex-elves-live-") && entry.path().is_dir() {
                Some((name, entry.path()))
            } else {
                None
            }
        })
        .collect();
    if live_dirs.len() <= keep {
        return;
    }
    // 目录名包含递增时间戳，按名称升序排序后，前面的是较旧的。
    live_dirs.sort_by(|a, b| a.0.cmp(&b.0));
    let remove_count = live_dirs.len() - keep;
    for (_, path) in live_dirs.into_iter().take(remove_count) {
        let _ = std::fs::remove_dir_all(&path);
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

fn move_model_providers_before_profiles(contents: &str) -> String {
    let lines = contents.lines().collect::<Vec<_>>();
    let Some(provider_start) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("[model_providers."))
    else {
        return ensure_trailing_newline(contents.to_string());
    };
    let provider_end = lines[provider_start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| provider_start + 1 + offset)
        .unwrap_or(lines.len());
    let Some(profile_start) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("[profiles."))
    else {
        return ensure_trailing_newline(contents.to_string());
    };
    if provider_start < profile_start {
        return ensure_trailing_newline(contents.to_string());
    }

    let mut output = Vec::with_capacity(lines.len());
    output.extend_from_slice(&lines[..profile_start]);
    output.extend_from_slice(&lines[provider_start..provider_end]);
    if output.last().is_some_and(|line| !line.trim().is_empty()) {
        output.push("");
    }
    output.extend_from_slice(&lines[profile_start..provider_start]);
    output.extend_from_slice(&lines[provider_end..]);
    ensure_trailing_newline(output.join("\n"))
}

fn auth_json_chatgpt_account_label(path: &Path) -> Option<Option<String>> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return None;
    };
    let Ok(value) = serde_json::from_str::<Value>(&contents) else {
        return None;
    };
    let is_chatgpt = value
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(|mode| mode.eq_ignore_ascii_case("chatgpt"))
        .unwrap_or(false);
    let tokens = value.get("tokens")?;
    if !is_chatgpt || !tokens_have_login_secret(tokens) {
        return None;
    }
    Some(account_label_from_tokens(tokens))
}

fn tokens_have_login_secret(tokens: &Value) -> bool {
    ["access_token", "id_token", "refresh_token"]
        .iter()
        .any(|key| {
            tokens
                .get(*key)
                .and_then(Value::as_str)
                .map(|token| !token.trim().is_empty())
                .unwrap_or(false)
        })
}

fn account_label_from_tokens(tokens: &Value) -> Option<String> {
    ["id_token", "access_token"].iter().find_map(|key| {
        tokens
            .get(*key)
            .and_then(Value::as_str)
            .and_then(account_label_from_jwt)
    })
}

fn account_label_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE
                .decode(payload.as_bytes())
                .ok()
        })?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("email")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("https://api.openai.com/profile")
                .and_then(|profile| profile.get("email"))
                .and_then(Value::as_str)
        })
        .or_else(|| value.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_catalog_prompt_replaces_gpt_identity_tokens_with_model() {
        let rewritten = rewrite_generated_catalog_prompt_text_for_model(
            "You are Codex, a coding agent based on GPT-5.5. GPT5.5 is available. Powered by GPT-5-Codex.",
            "deepseek-coder",
        );

        assert!(
            rewritten.contains("You are Codex, a coding agent based on the deepseek-coder model.")
        );
        assert!(rewritten.contains("deepseek-coder is available"));
        assert!(rewritten.contains("Powered by deepseek-coder."));
        assert!(!rewritten.contains("GPT-5"));
        assert!(!rewritten.contains("GPT5"));
        assert!(!rewritten.contains(".5"));
    }

    #[test]
    fn generated_catalog_prompt_replaces_nested_model_messages() {
        let value = json!({
            "instructions_template": "You are Codex, a coding agent based on the GPT-5 model.",
            "instructions_variables": {
                "personality_pragmatic": "Use GPT-5.5 behavior carefully."
            }
        });

        let rewritten = rewrite_generated_catalog_prompt_value_for_model(value, "qwen3-coder");
        let text = rewritten.to_string();
        assert!(text.contains("qwen3-coder"));
        assert!(!text.contains("GPT-5"));
        assert!(!text.contains(".5"));
    }

    #[test]
    fn generated_catalog_prompt_replaces_older_gpt_versions_without_word_damage() {
        let rewritten = rewrite_generated_catalog_prompt_text_for_model(
            "Prefer GPT-4 API compatibility over gpt-3.5 assumptions, but keep gptable untouched.",
            "claude-sonnet-4",
        );

        assert!(
            rewritten.contains(
                "Prefer claude-sonnet-4 API compatibility over claude-sonnet-4 assumptions"
            )
        );
        assert!(rewritten.contains("gptable untouched"));
        assert!(!rewritten.contains("GPT-4"));
        assert!(!rewritten.contains("gpt-3"));
        assert!(!rewritten.contains(".5"));
    }

    #[test]
    fn backfill_relay_profile_from_home_with_common_restores_template_provider_id() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("config.toml"),
            "model_provider = \"custom\"\nmodel = \"gpt-image-2\"\n\n[model_providers.custom]\nname = \"custom\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("auth.json"), "{}\n").unwrap();

        let mut profile = RelayProfile {
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            config_contents: "model_provider = \"ai\"\nmodel = \"gpt-image-2\"\n\n[model_providers.ai]\nname = \"ai\"\nwire_api = \"responses\"\nrequires_openai_auth = true\nbase_url = \"https://ahg.codes\"\n"
                .to_string(),
            auth_contents: "{}\n".to_string(),
            ..RelayProfile::default()
        };
        let mut common = String::new();

        backfill_relay_profile_from_home_with_common(temp.path(), &mut profile, &mut common)
            .unwrap();

        assert!(profile.config_contents.contains("model_provider = \"ai\""));
        assert!(profile.config_contents.contains("[model_providers.ai]"));
        assert!(!profile.config_contents.contains("[model_providers.custom]"));
    }

    #[test]
    fn relay_profile_model_prefers_config_then_field_then_empty() {
        // 1. 供應商測試的回退第一級：config.toml 的 model = 優先
        let from_config = RelayProfile {
            config_contents: "model = \"deepseek-v4-flash\"\nmodel_provider = \"custom\"\n"
                .to_string(),
            model: "should-not-be-used".to_string(),
            ..RelayProfile::default()
        };
        assert_eq!(relay_profile_model(&from_config), "deepseek-v4-flash");

        // 2. config 沒寫 model 時退回 profile.model 欄位
        let from_field = RelayProfile {
            config_contents: "model_provider = \"custom\"\n".to_string(),
            model: "deepseek-v4-pro".to_string(),
            ..RelayProfile::default()
        };
        assert_eq!(relay_profile_model(&from_field), "deepseek-v4-pro");

        // 3. 兩者皆空 → 空字串；呼叫端據此才回退到全域 relayTestModel
        let empty = RelayProfile {
            config_contents: String::new(),
            model: String::new(),
            ..RelayProfile::default()
        };
        assert!(relay_profile_model(&empty).trim().is_empty());
    }

    #[test]
    fn fast_service_tier_capability_matches_packaged_models() {
        let (tiers, speed) = fast_service_tier_capability("gpt-5.5").expect("gpt-5.5 应支持 fast");
        assert!(
            tiers.as_array().is_some_and(|items| items
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some("priority"))),
            "service_tiers 应包含 priority: {tiers}"
        );
        assert!(
            speed
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("fast"))),
            "additional_speed_tiers 应包含 fast: {speed}"
        );

        assert!(fast_service_tier_capability("gpt-5.2").is_none());
        assert!(fast_service_tier_capability("claude-sonnet-4.5").is_none());
        assert!(fast_service_tier_capability("unknown-model").is_none());
    }

    #[test]
    fn generated_model_catalog_backfills_fast_capability() {
        let profile = RelayProfile {
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            model_mappings: vec![
                crate::settings::RelayModelMapping {
                    request_model: "gpt-5.5".to_string(),
                    context_window: "400000".to_string(),
                    protocol: RelayProtocol::Responses,
                },
                crate::settings::RelayModelMapping {
                    request_model: "gpt-5.2".to_string(),
                    context_window: "400000".to_string(),
                    protocol: RelayProtocol::Responses,
                },
            ],
            ..RelayProfile::default()
        };
        let rows = relay_profile_catalog_rows(&profile);
        let catalog = generated_model_catalog_json(&profile, "", rows).unwrap();
        let models = catalog.get("models").and_then(Value::as_array).unwrap();

        let fast_model = models
            .iter()
            .find(|m| m.get("slug").and_then(Value::as_str) == Some("gpt-5.5"))
            .expect("应存在 gpt-5.5");
        let tiers = fast_model
            .get("service_tiers")
            .and_then(Value::as_array)
            .unwrap();
        assert!(
            tiers
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some("priority")),
            "gpt-5.5 的 catalog 应保留 priority service_tier"
        );
        let speed = fast_model
            .get("additional_speed_tiers")
            .and_then(Value::as_array)
            .unwrap();
        assert!(speed.iter().any(|item| item.as_str() == Some("fast")));

        let standard_model = models
            .iter()
            .find(|m| m.get("slug").and_then(Value::as_str) == Some("gpt-5.2"))
            .expect("应存在 gpt-5.2");
        let standard_tiers = standard_model
            .get("service_tiers")
            .and_then(Value::as_array)
            .unwrap();
        assert!(standard_tiers.is_empty(), "gpt-5.2 不应有 fast tier");
    }

    #[test]
    fn generated_model_catalog_backfills_gpt54_and_gpt56_context_defaults() {
        let profile = RelayProfile {
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            model_mappings: vec![
                crate::settings::RelayModelMapping {
                    request_model: "gpt-5.4".to_string(),
                    context_window: String::new(),
                    protocol: RelayProtocol::Responses,
                },
                crate::settings::RelayModelMapping {
                    request_model: "openai/gpt-5.6-custom".to_string(),
                    context_window: String::new(),
                    protocol: RelayProtocol::Responses,
                },
            ],
            ..RelayProfile::default()
        };
        let rows = relay_profile_catalog_rows(&profile);
        let catalog = generated_model_catalog_json(&profile, "", rows).unwrap();
        let models = catalog.get("models").and_then(Value::as_array).unwrap();

        for slug in ["gpt-5.4", "openai/gpt-5.6-custom"] {
            let model = models
                .iter()
                .find(|model| model.get("slug").and_then(Value::as_str) == Some(slug))
                .unwrap_or_else(|| panic!("应存在模型 {slug}"));
            assert_eq!(model["context_window"], 1_000_000);
            assert_eq!(model["max_context_window"], 1_000_000);
        }

        let gpt56 = models
            .iter()
            .find(|model| {
                model.get("slug").and_then(Value::as_str) == Some("openai/gpt-5.6-custom")
            })
            .expect("应存在 gpt-5.6 自定义模型");
        assert!(
            gpt56["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|level| level.get("effort").and_then(Value::as_str) == Some("max"))
        );
    }

    #[test]
    fn generated_model_catalog_uses_system_prompt_override() {
        let profile = RelayProfile {
            model: "gpt-direct".to_string(),
            protocol: RelayProtocol::Responses,
            system_prompt_override: "第一行\n第二行".to_string(),
            ..RelayProfile::default()
        };
        let rows = relay_profile_catalog_rows(&profile);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "gpt-direct");

        let catalog = generated_model_catalog_json(&profile, "", rows).unwrap();
        let model = &catalog["models"][0];
        assert_eq!(model["base_instructions"], "第一行\n第二行");
        assert_eq!(
            model["model_messages"]["instructions_template"],
            "第一行\n第二行"
        );
    }

    #[test]
    fn generated_model_catalog_is_created_for_direct_prompt_override_without_model_list() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let profile = RelayProfile {
            model: "gpt-direct".to_string(),
            base_url: "https://example.test/v1".to_string(),
            api_key: "sk-test".to_string(),
            protocol: RelayProtocol::Responses,
            local_proxy_enabled: Some(false),
            relay_mode: crate::settings::RelayMode::PureApi,
            system_prompt_override: "直连提示词".to_string(),
            config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://example.test/v1"
"#
            .to_string(),
            ..RelayProfile::default()
        };

        apply_relay_profile_files_to_home_with_context(home, &profile, "").unwrap();
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(config.contains(r#"model_catalog_json = "codex-elves-model-catalog.json""#));
        let catalog: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(GENERATED_MODEL_CATALOG_FILENAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(catalog["models"][0]["slug"], "gpt-direct");
        assert_eq!(catalog["models"][0]["base_instructions"], "直连提示词");
    }

    #[test]
    fn sync_applied_relay_profile_model_catalog_updates_generated_file_order() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        std::fs::write(
            home.join("config.toml"),
            r#"
model = "deepseek-coder"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:45221/v1"
"#,
        )
        .unwrap();

        let mut profile = RelayProfile {
            relay_mode: crate::settings::RelayMode::PureApi,
            protocol: crate::settings::RelayProtocol::Responses,
            model_mappings: vec![
                crate::settings::RelayModelMapping {
                    request_model: "deepseek-coder".to_string(),
                    context_window: "128000".to_string(),
                    protocol: RelayProtocol::Responses,
                },
                crate::settings::RelayModelMapping {
                    request_model: "qwen3-coder".to_string(),
                    context_window: "200000".to_string(),
                    protocol: RelayProtocol::ChatCompletions,
                },
            ],
            ..RelayProfile::default()
        };

        assert!(sync_applied_relay_profile_model_catalog_to_home(home, &profile).unwrap());
        assert!(
            std::fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains(r#"model_catalog_json = "codex-elves-model-catalog.json""#)
        );
        let first_catalog: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(GENERATED_MODEL_CATALOG_FILENAME)).unwrap(),
        )
        .unwrap();
        let first_models = first_catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|model| model.get("slug").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(first_models, vec!["deepseek-coder", "qwen3-coder"]);

        profile.model_mappings = vec![
            crate::settings::RelayModelMapping {
                request_model: "claude-opus-4.6".to_string(),
                context_window: "1000000".to_string(),
                protocol: RelayProtocol::Anthropic,
            },
            crate::settings::RelayModelMapping {
                request_model: "qwen3-coder".to_string(),
                context_window: "200000".to_string(),
                protocol: RelayProtocol::ChatCompletions,
            },
            crate::settings::RelayModelMapping {
                request_model: "deepseek-coder".to_string(),
                context_window: "128000".to_string(),
                protocol: RelayProtocol::Responses,
            },
        ];

        assert!(sync_applied_relay_profile_model_catalog_to_home(home, &profile).unwrap());
        let updated_catalog: Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(GENERATED_MODEL_CATALOG_FILENAME)).unwrap(),
        )
        .unwrap();
        let updated_models = updated_catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|model| model.get("slug").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            updated_models,
            vec!["claude-opus-4.6", "qwen3-coder", "deepseek-coder"]
        );
        assert_eq!(updated_catalog["models"][0]["context_window"], 1_000_000);
    }

    fn count_live_backups(home: &Path) -> usize {
        let backups = home.join("backups");
        if !backups.exists() {
            return 0;
        }
        std::fs::read_dir(&backups)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("codex-elves-live-")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn write_codex_live_atomic_skips_rewrite_when_content_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let config = "model = \"gpt-5.5\"\n";
        let auth = b"{}\n";

        let first = write_codex_live_atomic(home, Some(config), Some(auth), false).unwrap();
        assert!(first.is_none(), "首次无旧文件，不应产生备份");
        assert_eq!(
            std::fs::read_to_string(home.join("config.toml")).unwrap(),
            config
        );

        let second = write_codex_live_atomic(home, Some(config), Some(auth), false).unwrap();
        assert!(second.is_none(), "内容未变不应产生备份");
        assert_eq!(
            count_live_backups(home),
            0,
            "内容未变时不应生成任何备份目录"
        );

        let third = write_codex_live_atomic(home, Some("model = \"gpt-5.4\"\n"), Some(auth), false)
            .unwrap();
        assert!(third.is_some(), "内容变化应产生备份");
        assert_eq!(count_live_backups(home), 1);
    }

    #[test]
    fn prune_live_backups_keeps_only_recent() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let backups = home.join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for ts in 1000u64..1025 {
            std::fs::create_dir_all(backups.join(format!("codex-elves-live-{ts}"))).unwrap();
        }
        std::fs::create_dir_all(backups.join("keep-me")).unwrap();

        prune_live_backups(home, 20);

        assert_eq!(count_live_backups(home), 20, "只保留最近 20 份");
        assert!(
            !backups.join("codex-elves-live-1000").exists(),
            "最旧的应被删除"
        );
        assert!(
            backups.join("codex-elves-live-1024").exists(),
            "最新的应保留"
        );
        assert!(backups.join("keep-me").exists(), "无关目录不受影响");
    }

    #[test]
    fn normalize_duplicate_toml_text_keeps_array_of_tables() {
        // [[array.table]] 是 TOML 数组表，多个同名元素合法，不能被当作重复表删除
        let input = concat!(
            "[[skills.config]]\n",
            "name = \"a\"\n",
            "enabled = false\n",
            "\n",
            "[[skills.config]]\n",
            "name = \"b\"\n",
            "enabled = false\n",
        );
        let out = normalize_duplicate_toml_text(input);
        assert!(out.contains("name = \"a\""), "第一个数组表应保留");
        assert!(
            out.contains("name = \"b\""),
            "第二个数组表不应被误删：\n{out}"
        );
    }

    #[test]
    fn sync_live_context_preserves_array_of_tables() {
        // 保存 mcp 时不应误删 [[skills.config]] 等数组表
        let live = concat!(
            "model = \"gpt-5.5\"\n",
            "\n",
            "[[skills.config]]\n",
            "name = \"superpowers:writing-skills\"\n",
            "enabled = false\n",
            "\n",
            "[[skills.config]]\n",
            "name = \"superpowers:using-git-worktrees\"\n",
            "enabled = false\n",
            "\n",
            "[mcp_servers.old]\n",
            "command = \"node\"\n",
        );
        let context = "[mcp_servers.context7]\ncommand = \"npx\"\n";
        let out = sync_live_config_context_entries(live, context).unwrap();
        assert!(out.contains("superpowers:writing-skills"), "skill 1 应保留");
        assert!(
            out.contains("superpowers:using-git-worktrees"),
            "skill 2 不应被误删：\n{out}"
        );
        assert_eq!(
            out.matches("[[skills.config]]").count(),
            2,
            "两个数组表都应保留：\n{out}"
        );
        assert!(out.contains("[mcp_servers.context7]"), "新 mcp 应写入");
    }

    #[test]
    fn sync_live_context_updates_existing_mcp_in_place() {
        // 更新已存在的 mcp 时应原地更新、保留位置，不漂移到文件末尾
        let live = concat!(
            "model = \"gpt-5.5\"\n",
            "\n",
            "[mcp_servers.alpha]\n",
            "command = \"old\"\n",
            "\n",
            "[features]\n",
            "hooks = true\n",
        );
        let context = "[mcp_servers.alpha]\ncommand = \"new\"\n";
        let out = sync_live_config_context_entries(live, context).unwrap();
        // alpha 仍在 features 之前（保留原位置），且 command 已更新
        let alpha_pos = out.find("[mcp_servers.alpha]").unwrap();
        let features_pos = out.find("[features]").unwrap();
        assert!(
            alpha_pos < features_pos,
            "mcp 应保留在原位置，不漂移：\n{out}"
        );
        assert!(out.contains("command = \"new\""), "内容应更新");
        assert!(!out.contains("command = \"old\""), "旧值应被替换");
    }

    #[test]
    fn sync_live_context_removes_disabled_mcp() {
        // context 里标记 enabled=false 的 mcp 应从 live 中移除
        let live = concat!(
            "[mcp_servers.keepme]\n",
            "command = \"a\"\n",
            "\n",
            "[mcp_servers.dropme]\n",
            "command = \"b\"\n",
        );
        let context = concat!(
            "[mcp_servers.keepme]\n",
            "command = \"a\"\n",
            "\n",
            "[mcp_servers.dropme]\n",
            "command = \"b\"\n",
            "enabled = false\n",
        );
        let out = sync_live_config_context_entries(live, context).unwrap();
        assert!(out.contains("[mcp_servers.keepme]"), "启用项应保留");
        assert!(
            !out.contains("[mcp_servers.dropme]"),
            "禁用项应被移除：\n{out}"
        );
    }
}

pub fn root_key_string(contents: &str, key: &str) -> Option<String> {
    root_key_value(contents, key).map(unquote_toml_string)
}

fn root_key_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            return None;
        }
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Some(value);
        }
    }
    None
}

fn upsert_model_provider_config(
    contents: &str,
    base_url: &str,
    bearer_token: &str,
) -> anyhow::Result<String> {
    let doc = parse_toml_document(contents)?;
    let provider_id = active_or_default_provider_id(&doc);
    let mut updated = set_root_toml_string_line(contents, "model_provider", &provider_id);
    for legacy_provider in LEGACY_RELAY_PROVIDERS {
        updated = remove_table(
            &updated,
            &format!("model_providers.{}", toml_key_segment(legacy_provider)),
        );
    }
    if provider_id != RELAY_PROVIDER {
        updated = remove_table(&updated, &format!("model_providers.{RELAY_PROVIDER}"));
    }

    let provider_table = model_provider_table_name(&provider_id);
    updated = set_table_toml_string_line(&updated, &provider_table, "name", &provider_id);
    updated = set_table_toml_string_line(&updated, &provider_table, "wire_api", "responses");
    updated = set_table_toml_raw_line(&updated, &provider_table, "requires_openai_auth", "true");
    updated = set_table_toml_string_line(&updated, &provider_table, "base_url", base_url);
    updated = set_table_toml_string_line(
        &updated,
        &provider_table,
        "experimental_bearer_token",
        bearer_token,
    );
    parse_toml_document(&updated)?;
    Ok(updated)
}

fn relay_profile_provider_id(profile: &RelayProfile) -> anyhow::Result<String> {
    let doc = parse_toml_document(&profile.config_contents)?;
    Ok(active_or_default_provider_id(&doc))
}

fn relay_profile_owned_model(profile: &RelayProfile) -> String {
    if !profile.model.trim().is_empty() {
        profile.model.trim().to_string()
    } else {
        root_key_string(&profile.config_contents, "model").unwrap_or_default()
    }
}

fn relay_profile_owned_base_url(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return crate::protocol_proxy::local_responses_proxy_base_url(
            crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT,
        );
    }
    if !profile.upstream_base_url.trim().is_empty() {
        return profile.upstream_base_url.trim().to_string();
    }
    if !profile.base_url.trim().is_empty() {
        return profile.base_url.trim().to_string();
    }
    provider_string_from_config(&profile.config_contents, "base_url").unwrap_or_default()
}

fn relay_profile_owned_api_key(profile: &RelayProfile) -> String {
    if profile.relay_mode == crate::settings::RelayMode::Aggregate {
        return "codex-elves-aggregate".to_string();
    }
    if !profile.api_key.trim().is_empty() {
        return profile.api_key.trim().to_string();
    }
    codex_auth_api_key(&profile.auth_contents)
        .or_else(|| {
            experimental_bearer_token_from_config(&profile.config_contents)
                .ok()
                .flatten()
        })
        .unwrap_or_default()
}

fn model_provider_table_name(provider_id: &str) -> String {
    format!("model_providers.{}", toml_key_segment(provider_id))
}

fn set_root_toml_string_line(contents: &str, key: &str, value: &str) -> String {
    set_root_toml_raw_line(contents, key, &toml_string_literal(value))
}

fn set_root_toml_raw_line(contents: &str, key: &str, raw_value: &str) -> String {
    let line_text = format!("{key} = {raw_value}");
    let mut lines = normalized_lines(contents);
    let root_end = root_section_end(&lines);
    for line in lines.iter_mut().take(root_end) {
        if root_line_key(line) == Some(key) {
            *line = line_text;
            return ensure_trailing_newline(lines.join("\n").trim_end().to_string());
        }
    }
    let insert_at = root_end;
    lines.insert(insert_at, line_text);
    ensure_trailing_newline(lines.join("\n").trim_end().to_string())
}

fn set_table_toml_string_line(contents: &str, table: &str, key: &str, value: &str) -> String {
    set_table_toml_raw_line(contents, table, key, &toml_string_literal(value))
}

fn set_table_toml_raw_line(contents: &str, table: &str, key: &str, raw_value: &str) -> String {
    let header = format!("[{table}]");
    let line_text = format!("{key} = {raw_value}");
    let mut lines = normalized_lines(contents);
    let Some((start, end)) = table_bounds(&lines, &header) else {
        return append_table_with_line(&lines, &header, &line_text);
    };
    for line in lines.iter_mut().take(end).skip(start + 1) {
        if root_line_key(line) == Some(key) {
            *line = line_text;
            return ensure_trailing_newline(lines.join("\n").trim_end().to_string());
        }
    }
    let mut insert_at = end;
    while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    lines.insert(insert_at, line_text);
    ensure_trailing_newline(lines.join("\n").trim_end().to_string())
}

fn remove_table_key(contents: &str, table: &str, key: &str) -> String {
    let header = format!("[{table}]");
    let lines = normalized_lines(contents);
    let Some((start, end)) = table_bounds(&lines, &header) else {
        return ensure_trailing_newline(lines.join("\n").trim_end().to_string());
    };
    let next = lines
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| {
            if index > start && index < end && root_line_key(&line) == Some(key) {
                None
            } else {
                Some(line)
            }
        })
        .collect::<Vec<_>>();
    ensure_trailing_newline(next.join("\n").trim_end().to_string())
}

fn append_table_with_line(lines: &[String], header: &str, line_text: &str) -> String {
    let mut next = lines.to_vec();
    while next.last().is_some_and(|line| line.trim().is_empty()) {
        next.pop();
    }
    if !next.is_empty() {
        next.push(String::new());
    }
    next.push(header.to_string());
    next.push(line_text.to_string());
    ensure_trailing_newline(next.join("\n").trim_end().to_string())
}

fn table_bounds(lines: &[String], header: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| line.trim() == header)?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    Some((start, end))
}

fn root_section_end(lines: &[String]) -> usize {
    lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len())
}

fn normalized_lines(contents: &str) -> Vec<String> {
    contents.lines().map(ToString::to_string).collect()
}

fn toml_key_segment(key: &str) -> String {
    if key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        key.to_string()
    } else {
        toml_string_literal(key)
    }
}

fn toml_string_literal(value: &str) -> String {
    toml_edit::value(value).to_string()
}

fn remove_table(contents: &str, table: &str) -> String {
    let header = format!("[{table}]");
    let mut lines = Vec::new();
    let mut skipping = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed == header {
                skipping = true;
                continue;
            }
            skipping = false;
        }
        if !skipping {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

fn remove_root_key(contents: &str, key: &str) -> String {
    let mut lines = Vec::new();
    let mut in_root = true;
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if in_root && root_line_key(line) == Some(key) {
            continue;
        }
        lines.push(line.to_string());
    }
    lines.join("\n")
}

fn table_values(contents: &str, table: &str) -> Option<std::collections::HashMap<String, String>> {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut values = std::collections::HashMap::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_table {
                break;
            }
            in_table = trimmed == header;
            continue;
        }
        if !in_table || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    in_table.then_some(values)
}

fn unquote_toml_string(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn root_line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    trimmed.split_once('=').map(|(key, _)| key.trim())
}
