use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LaunchMode {
    #[default]
    Patch,
    Relay,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayContextSelection {
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<String>,
}

impl Default for RelayContextSelection {
    fn default() -> Self {
        Self {
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            plugins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayModelMapping {
    #[serde(default)]
    pub request_model: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub protocol: RelayProtocol,
    #[serde(default)]
    pub context_window: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LayeredCompactionModels {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gpt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claude: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub other: String,
}

impl LayeredCompactionModels {
    pub fn model_for_family(&self, family: crate::model_capabilities::ModelFamily) -> &str {
        match family {
            crate::model_capabilities::ModelFamily::Gpt => &self.gpt,
            crate::model_capabilities::ModelFamily::Claude => &self.claude,
            crate::model_capabilities::ModelFamily::Other => &self.other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResponsesWebsocketCapabilityState {
    #[default]
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesWebsocketCapability {
    #[serde(default)]
    pub state: ResponsesWebsocketCapabilityState,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub checked_at_ms: Option<u64>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProfile {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing)]
    pub model: String,
    #[serde(default = "default_relay_base_url", skip_serializing)]
    pub base_url: String,
    #[serde(rename = "upstreamBaseUrl", default)]
    pub upstream_base_url: String,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "deserialize_profile_api_key"
    )]
    pub api_key: String,
    #[serde(default)]
    pub protocol: RelayProtocol,
    #[serde(
        rename = "localProxyEnabled",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub local_proxy_enabled: Option<bool>,
    #[serde(rename = "relayMode", default)]
    pub relay_mode: RelayMode,
    #[serde(rename = "officialMixApiKey", default)]
    pub official_mix_api_key: bool,
    #[serde(rename = "testModel", default)]
    pub test_model: String,
    #[serde(rename = "configContents", default)]
    pub config_contents: String,
    #[serde(rename = "authContents", default)]
    pub auth_contents: String,
    #[serde(rename = "useCommonConfig", default = "default_true")]
    pub use_common_config: bool,
    #[serde(rename = "contextSelection", default)]
    pub context_selection: RelayContextSelection,
    #[serde(rename = "contextSelectionInitialized", default)]
    pub context_selection_initialized: bool,
    #[serde(rename = "contextWindow", default)]
    pub context_window: String,
    #[serde(rename = "autoCompactLimit", default)]
    pub auto_compact_limit: String,
    #[serde(rename = "modelInsertMode", default)]
    pub model_insert_mode: RelayModelInsertMode,
    #[serde(rename = "modelMappings", default)]
    pub model_mappings: Vec<RelayModelMapping>,
    #[serde(rename = "modelList", default)]
    pub model_list: String,
    #[serde(rename = "responsesModelList", default)]
    pub responses_model_list: String,
    #[serde(rename = "chatCompletionsModelList", default)]
    pub chat_completions_model_list: String,
    #[serde(rename = "anthropicModelList", default)]
    pub anthropic_model_list: String,
    #[serde(rename = "responsesWebsocket", default)]
    pub responses_websocket: ResponsesWebsocketCapability,
    #[serde(
        rename = "responsesWebsocketEnabled",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub responses_websocket_enabled: Option<bool>,
    #[serde(
        rename = "userAgent",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub user_agent: String,
    #[serde(
        rename = "systemPromptOverride",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub system_prompt_override: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AggregateRelayStrategy {
    #[default]
    Failover,
    ConversationRoundRobin,
    RequestRoundRobin,
    WeightedRoundRobin,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRelayMember {
    #[serde(rename = "relayId")]
    pub relay_id: String,
    #[serde(default = "default_aggregate_member_weight")]
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRelayProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub strategy: AggregateRelayStrategy,
    #[serde(default)]
    pub members: Vec<AggregateRelayMember>,
}

impl Default for RelayProfile {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "默认中转".to_string(),
            model: String::new(),
            base_url: default_relay_base_url(),
            upstream_base_url: String::new(),
            api_key: String::new(),
            protocol: RelayProtocol::Responses,
            local_proxy_enabled: None,
            relay_mode: RelayMode::Official,
            official_mix_api_key: false,
            test_model: String::new(),
            config_contents: String::new(),
            auth_contents: String::new(),
            use_common_config: true,
            context_selection: RelayContextSelection::default(),
            context_selection_initialized: false,
            context_window: String::new(),
            auto_compact_limit: String::new(),
            model_insert_mode: RelayModelInsertMode::Patch,
            model_mappings: Vec::new(),
            model_list: String::new(),
            responses_model_list: String::new(),
            chat_completions_model_list: String::new(),
            anthropic_model_list: String::new(),
            responses_websocket: ResponsesWebsocketCapability::default(),
            responses_websocket_enabled: None,
            user_agent: String::new(),
            system_prompt_override: String::new(),
        }
    }
}

impl RelayProfile {
    pub fn local_proxy_enabled(&self) -> bool {
        self.local_proxy_enabled.unwrap_or(false)
    }

    fn model_mapping_for_catalog_model(&self, model: &str) -> Option<&RelayModelMapping> {
        let model = model.trim();
        if model.is_empty() {
            return None;
        }
        let catalog_mapping = relay_model_mapping_catalog_slugs(&self.model_mappings)
            .iter()
            .position(|catalog_slug| catalog_slug == model)
            .and_then(|index| self.model_mappings.get(index));
        let legacy_mapping = legacy_relay_model_mapping_catalog_slugs(&self.model_mappings)
            .iter()
            .position(|catalog_slug| catalog_slug == model)
            .and_then(|index| self.model_mappings.get(index));
        let catalog_mappings = relay_model_mappings_for_catalog(&self.model_mappings);
        let migrated_mapping = relay_model_mapping_catalog_slugs(&catalog_mappings)
            .iter()
            .position(|catalog_slug| catalog_slug == model)
            .and_then(|index| catalog_mappings.get(index))
            .and_then(|mapping| {
                self.model_mappings
                    .iter()
                    .find(|candidate| relay_model_mapping_parameters_match(candidate, mapping))
            });
        let resolved = if validate_catalog_model_identifiers_only(&self.model_mappings).is_err() {
            legacy_mapping.or(migrated_mapping).or(catalog_mapping)
        } else {
            catalog_mapping.or(migrated_mapping).or(legacy_mapping)
        };
        resolved.or_else(|| {
            self.model_mappings
                .iter()
                .find(|mapping| mapping.request_model.trim() == model)
        })
    }

    pub(crate) fn request_model_for_catalog_model(&self, model: &str) -> String {
        self.model_mapping_for_catalog_model(model)
            .map(|mapping| mapping.request_model.trim().to_string())
            .filter(|request_model| !request_model.is_empty())
            .unwrap_or_else(|| model.trim().to_string())
    }

    pub(crate) fn catalog_model_for_configured_model(&self, model: &str) -> String {
        let model = model.trim();
        if model.is_empty() || self.model_mappings.is_empty() {
            return model.to_string();
        }
        let Some(mapping) = self.model_mapping_for_catalog_model(model) else {
            return model.to_string();
        };
        let catalog_mappings = relay_model_mappings_for_catalog(&self.model_mappings);
        let catalog_slugs = relay_model_mapping_catalog_slugs(&catalog_mappings);
        let catalog_model = catalog_mappings
            .iter()
            .position(|candidate| relay_model_mapping_parameters_match(candidate, mapping))
            .and_then(|index| catalog_slugs.get(index))
            .cloned()
            .unwrap_or_default();
        if catalog_model.is_empty() {
            model.to_string()
        } else {
            catalog_model
        }
    }

    pub fn context_window_for_active_model(&self) -> String {
        let model = self.model.trim();
        if !model.is_empty() && !self.model_mappings.is_empty() {
            return self
                .model_mapping_for_catalog_model(model)
                .map(|mapping| mapping.context_window.trim().to_string())
                .unwrap_or_default();
        }
        self.context_window.trim().to_string()
    }

    pub fn context_window_for_model(&self, model: &str) -> Option<u64> {
        let model = model.trim();
        if model.is_empty() {
            return None;
        }
        self.model_mapping_for_catalog_model(model)
            .map(|mapping| mapping.context_window.trim())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                (self.model_mappings.is_empty() && self.model.trim() == model)
                    .then_some(self.context_window.trim())
                    .filter(|value| !value.is_empty())
            })
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .or_else(|| crate::model_capabilities::required_model_context_window(model))
    }

    pub fn resolve_protocol_for_model(&self, model: &str) -> anyhow::Result<RelayProtocol> {
        let model = model.trim();
        if model.is_empty() {
            anyhow::bail!("模型不能为空，无法确定协议归属");
        }
        self.validate_model_protocol_assignments()?;

        if !self.model_mappings.is_empty() {
            return self
                .model_mapping_for_catalog_model(model)
                .map(|mapping| mapping.protocol)
                .with_context(|| format!("模型「{model}」没有明确协议归属"));
        }

        let mut resolved = None;
        for (protocol, model_list) in [
            (RelayProtocol::Responses, self.responses_model_list.as_str()),
            (
                RelayProtocol::ChatCompletions,
                self.chat_completions_model_list.as_str(),
            ),
            (RelayProtocol::Anthropic, self.anthropic_model_list.as_str()),
        ] {
            if split_relay_model_ids(model_list).any(|candidate| candidate == model) {
                resolved = Some(protocol);
                break;
            }
        }
        resolved.with_context(|| format!("模型「{model}」没有明确协议归属"))
    }

    pub fn validate_model_protocol_assignments(&self) -> anyhow::Result<()> {
        let mut assignments = HashMap::<String, RelayProtocol>::new();
        if !self.model_mappings.is_empty() {
            for mapping in &self.model_mappings {
                let request_model = mapping.request_model.trim();
                if request_model.is_empty() {
                    continue;
                }
                insert_model_protocol_assignment(
                    &mut assignments,
                    request_model,
                    mapping.protocol,
                )?;
            }
            return Ok(());
        }

        for (protocol, model_list) in [
            (RelayProtocol::Responses, self.responses_model_list.as_str()),
            (
                RelayProtocol::ChatCompletions,
                self.chat_completions_model_list.as_str(),
            ),
            (RelayProtocol::Anthropic, self.anthropic_model_list.as_str()),
        ] {
            for model in split_relay_model_ids(model_list) {
                insert_model_protocol_assignment(&mut assignments, model, protocol)?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_model_catalog_identifiers(&self) -> anyhow::Result<()> {
        self.validate_model_protocol_assignments()?;
        validate_catalog_model_identifiers_only(&relay_model_mappings_for_catalog(
            &self.model_mappings,
        ))
    }

    fn validate_model_mapping_save_constraints(&self) -> anyhow::Result<()> {
        let mut parameter_signatures = HashSet::new();
        for mapping in &self.model_mappings {
            let request_model = mapping.request_model.trim();
            if request_model.is_empty() {
                continue;
            }
            let context_window = normalized_model_context_window(&mapping.context_window);
            if !parameter_signatures.insert((
                request_model.to_string(),
                mapping.protocol,
                context_window,
            )) {
                anyhow::bail!(
                    "模型「{request_model}」的模型配置重复：协议为 {}，上下文大小为「{}」；别名不能用于区分完全相同的参数",
                    relay_protocol_label(mapping.protocol),
                    mapping.context_window.trim()
                );
            }
        }
        validate_catalog_model_identifiers_only(&self.model_mappings)
    }
}

fn normalized_model_context_window(value: &str) -> String {
    let value = value.trim();
    value
        .parse::<u64>()
        .map(|parsed| parsed.to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn relay_model_mapping_parameters_match(
    left: &RelayModelMapping,
    right: &RelayModelMapping,
) -> bool {
    left.request_model.trim() == right.request_model.trim()
        && left.protocol == right.protocol
        && normalized_model_context_window(&left.context_window)
            == normalized_model_context_window(&right.context_window)
}

fn validate_catalog_model_identifiers_only(mappings: &[RelayModelMapping]) -> anyhow::Result<()> {
    let catalog_slugs = relay_model_mapping_catalog_slugs(mappings);
    let legacy_slugs = legacy_relay_model_mapping_catalog_slugs(mappings);
    let mut catalog_identifiers = HashSet::new();

    for (index, (mapping, catalog_slug)) in mappings.iter().zip(&catalog_slugs).enumerate() {
        if mapping.request_model.trim().is_empty() || catalog_slug.is_empty() {
            continue;
        }
        if !catalog_identifiers.insert(catalog_slug.clone()) {
            anyhow::bail!(
                "模型标识重复：「{catalog_slug}」；请使用唯一别名，或调整请求模型与上下文大小"
            );
        }
        if mappings
            .iter()
            .enumerate()
            .any(|(candidate_index, candidate)| {
                let candidate_request_model = candidate.request_model.trim();
                candidate_index != index
                    && candidate_request_model == catalog_slug
                    && candidate_request_model != mapping.request_model.trim()
            })
        {
            anyhow::bail!(
                "模型标识「{catalog_slug}」与另一条请求模型冲突；请使用不会与真实请求模型重名的别名"
            );
        }
        if legacy_slugs
            .iter()
            .enumerate()
            .any(|(candidate_index, legacy_slug)| {
                candidate_index != index
                    && legacy_slug == catalog_slug
                    && mappings[candidate_index].request_model.trim()
                        != mapping.request_model.trim()
            })
        {
            anyhow::bail!("模型标识「{catalog_slug}」与另一条旧版模型标识冲突；请更换别名");
        }
    }
    Ok(())
}

pub(crate) fn relay_model_mapping_catalog_slugs(mappings: &[RelayModelMapping]) -> Vec<String> {
    let mut unaliased_counts = HashMap::<&str, usize>::new();
    for mapping in mappings {
        let request_model = mapping.request_model.trim();
        if !request_model.is_empty() && mapping.alias.trim().is_empty() {
            *unaliased_counts.entry(request_model).or_default() += 1;
        }
    }

    mappings
        .iter()
        .map(|mapping| {
            let request_model = mapping.request_model.trim();
            let include_context_window = unaliased_counts
                .get(request_model)
                .copied()
                .unwrap_or_default()
                > 1;
            relay_model_mapping_catalog_slug(mapping, include_context_window)
        })
        .collect()
}

fn relay_model_mapping_catalog_slug(
    mapping: &RelayModelMapping,
    include_context_window: bool,
) -> String {
    let request_model = mapping.request_model.trim();
    if request_model.is_empty() {
        return String::new();
    }
    let alias = mapping.alias.trim();
    if !alias.is_empty() {
        return alias.to_string();
    }
    let context_window = mapping.context_window.trim();
    if !include_context_window || context_window.is_empty() {
        request_model.to_string()
    } else {
        format!("{request_model} {context_window}")
    }
}

pub(crate) fn relay_model_mappings_for_catalog(
    mappings: &[RelayModelMapping],
) -> Vec<RelayModelMapping> {
    let mut parameter_signatures = HashSet::new();
    let mut deduplicated = mappings
        .iter()
        .filter(|mapping| {
            let request_model = mapping.request_model.trim();
            !request_model.is_empty()
                && parameter_signatures.insert((
                    request_model.to_string(),
                    mapping.protocol,
                    normalized_model_context_window(&mapping.context_window),
                ))
        })
        .cloned()
        .collect::<Vec<_>>();
    if validate_catalog_model_identifiers_only(&deduplicated).is_err() {
        for mapping in &mut deduplicated {
            mapping.alias.clear();
        }
    }
    deduplicated
}

fn legacy_relay_model_mapping_catalog_slugs(mappings: &[RelayModelMapping]) -> Vec<String> {
    let reserved_request_models = mappings
        .iter()
        .map(|mapping| mapping.request_model.trim())
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let mut used_catalog_slugs = HashSet::new();
    let mut occurrences = HashMap::<String, usize>::new();

    mappings
        .iter()
        .map(|mapping| {
            let request_model = mapping.request_model.trim();
            if request_model.is_empty() {
                return String::new();
            }
            let occurrence = occurrences.entry(request_model.to_string()).or_default();
            *occurrence += 1;
            if *occurrence == 1 {
                used_catalog_slugs.insert(request_model.to_string());
                return request_model.to_string();
            }

            let base = format!("{request_model}--codex-elves-alias-{occurrence}");
            let mut catalog_slug = base.clone();
            let mut collision_index = 2;
            while reserved_request_models.contains(&catalog_slug)
                || !used_catalog_slugs.insert(catalog_slug.clone())
            {
                catalog_slug = format!("{base}-{collision_index}");
                collision_index += 1;
            }
            catalog_slug
        })
        .collect()
}

fn split_relay_model_ids(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(['\r', '\n', ','])
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn insert_model_protocol_assignment(
    assignments: &mut HashMap<String, RelayProtocol>,
    model: &str,
    protocol: RelayProtocol,
) -> anyhow::Result<()> {
    if model.is_empty() {
        return Ok(());
    }
    if let Some(existing) = assignments.get(model) {
        if *existing != protocol {
            anyhow::bail!(
                "模型「{model}」存在冲突协议归属：{} 与 {}",
                relay_protocol_label(*existing),
                relay_protocol_label(protocol)
            );
        }
        return Ok(());
    }
    assignments.insert(model.to_string(), protocol);
    Ok(())
}

fn relay_protocol_label(protocol: RelayProtocol) -> &'static str {
    match protocol {
        RelayProtocol::Responses => "Responses API",
        RelayProtocol::ChatCompletions => "Chat Completions",
        RelayProtocol::Anthropic => "Anthropic",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RelayModelInsertMode {
    ModelCatalog,
    #[default]
    Patch,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "camelCase")]
pub enum RelayProtocol {
    #[default]
    Responses,
    ChatCompletions,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RelayMode {
    #[default]
    Official,
    MixedApi,
    PureApi,
    Aggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BackendSettings {
    #[serde(rename = "codexAppPath", default)]
    pub codex_app_path: String,
    #[serde(
        rename = "codexHomePath",
        default,
        deserialize_with = "deserialize_codex_home_path"
    )]
    pub codex_home_path: String,
    #[serde(rename = "codexExtraArgs", default)]
    pub codex_extra_args: Vec<String>,
    #[serde(rename = "githubReleaseUpdatePromptEnabled", default = "default_true")]
    pub github_release_update_prompt_enabled: bool,
    #[serde(rename = "providerSyncEnabled", default)]
    pub provider_sync_enabled: bool,
    #[serde(rename = "providerSyncSavedProviders", default)]
    pub provider_sync_saved_providers: Vec<String>,
    #[serde(rename = "providerSyncManualProviders", default)]
    pub provider_sync_manual_providers: Vec<String>,
    #[serde(rename = "providerSyncLastSelectedProvider", default)]
    pub provider_sync_last_selected_provider: String,
    #[serde(rename = "relayProfilesEnabled", default = "default_true")]
    pub relay_profiles_enabled: bool,
    #[serde(rename = "enhancementsEnabled", default = "default_true")]
    pub enhancements_enabled: bool,
    #[serde(rename = "computerUseGuardEnabled", default = "default_true")]
    pub computer_use_guard_enabled: bool,
    #[serde(rename = "codexAppPluginEntryUnlock", default = "default_true")]
    pub codex_app_plugin_entry_unlock: bool,
    #[serde(rename = "codexAppPluginMarketplaceUnlock", default = "default_true")]
    pub codex_app_plugin_marketplace_unlock: bool,
    #[serde(rename = "codexAppTaskBoard", default = "default_true")]
    pub codex_app_task_board: bool,
    #[serde(rename = "codexAppSessionDelete", default = "default_true")]
    pub codex_app_session_delete: bool,
    #[serde(rename = "codexAppMarkdownExport", default)]
    pub codex_app_markdown_export: bool,
    #[serde(rename = "codexAppProjectMove", default)]
    pub codex_app_project_move: bool,
    #[serde(rename = "codexAppConversationView", default = "default_true")]
    pub codex_app_conversation_view: bool,
    #[serde(rename = "codexAppTokenUsage", default)]
    pub codex_app_token_usage: bool,
    #[serde(rename = "codexAppUpstreamWorktreeCreate", default)]
    pub codex_app_upstream_worktree_create: bool,
    #[serde(rename = "codexAppNativeMenuPlacement", default = "default_true")]
    pub codex_app_native_menu_placement: bool,
    #[serde(rename = "codexAppServiceTierControls", default)]
    pub codex_app_service_tier_controls: bool,
    #[serde(rename = "codexAppImageOverlayEnabled", default)]
    pub codex_app_image_overlay_enabled: bool,
    #[serde(rename = "codexAppImageOverlayPath", default)]
    pub codex_app_image_overlay_path: String,
    #[serde(
        rename = "codexAppImageOverlayOpacity",
        default = "default_image_overlay_opacity",
        deserialize_with = "deserialize_image_overlay_opacity"
    )]
    pub codex_app_image_overlay_opacity: u8,
    #[serde(rename = "codexAppActiveSkinId", default)]
    pub codex_app_active_skin_id: String,
    #[serde(rename = "codexGoalsEnabled", default)]
    pub codex_goals_enabled: bool,
    #[serde(rename = "lanProxyEnabled", default)]
    pub lan_proxy_enabled: bool,
    #[serde(rename = "gptReasoningContinuation", default)]
    pub gpt_reasoning_continuation: bool,
    #[serde(
        rename = "gptReasoningContinuationMaxRounds",
        default = "default_gpt_reasoning_continuation_max_rounds",
        deserialize_with = "deserialize_gpt_reasoning_continuation_max_rounds"
    )]
    pub gpt_reasoning_continuation_max_rounds: u8,
    #[serde(rename = "layeredCompactionEnabled", default)]
    pub layered_compaction_enabled: bool,
    #[serde(
        rename = "layeredCompactionRetainTokens",
        default = "default_layered_compaction_retain_tokens",
        deserialize_with = "deserialize_layered_compaction_retain_tokens"
    )]
    pub layered_compaction_retain_tokens: u32,
    #[serde(
        rename = "layeredCompactionPromptOverride",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub layered_compaction_prompt_override: String,
    #[serde(rename = "layeredCompactionModelOverrideEnabled", default)]
    pub layered_compaction_model_override_enabled: bool,
    #[serde(rename = "layeredCompactionModels", default)]
    pub layered_compaction_models: LayeredCompactionModels,
    /// 旧版单一压缩模型配置，仅用于读取迁移。
    #[serde(
        rename = "layeredCompactionModel",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub layered_compaction_model: String,
    #[serde(rename = "launchMode", default)]
    pub launch_mode: LaunchMode,
    #[serde(rename = "relayBaseUrl", default = "default_relay_base_url")]
    pub relay_base_url: String,
    #[serde(rename = "relayApiKey", default)]
    pub relay_api_key: String,
    #[serde(rename = "relayProfiles", default = "default_relay_profiles")]
    pub relay_profiles: Vec<RelayProfile>,
    #[serde(rename = "relayCommonConfigContents", default)]
    pub relay_common_config_contents: String,
    #[serde(rename = "relayContextConfigContents", default)]
    pub relay_context_config_contents: String,
    #[serde(rename = "activeRelayId", default = "default_active_relay_id")]
    pub active_relay_id: String,
    #[serde(rename = "aggregateRelayProfiles", default)]
    pub aggregate_relay_profiles: Vec<AggregateRelayProfile>,
    #[serde(rename = "activeAggregateRelayId", default)]
    pub active_aggregate_relay_id: String,
    #[serde(rename = "relayTestModel", default = "default_relay_test_model")]
    pub relay_test_model: String,
    #[serde(rename = "cliWrapperEnabled", default)]
    pub cli_wrapper_enabled: bool,
    #[serde(rename = "cliWrapperBaseUrl", default)]
    pub cli_wrapper_base_url: String,
    #[serde(rename = "cliWrapperApiKey", default)]
    pub cli_wrapper_api_key: String,
    #[serde(
        rename = "cliWrapperApiKeyEnv",
        default = "default_api_key_env",
        deserialize_with = "empty_as_default_api_key_env"
    )]
    pub cli_wrapper_api_key_env: String,
}

impl Default for BackendSettings {
    fn default() -> Self {
        Self {
            codex_app_path: String::new(),
            codex_home_path: String::new(),
            codex_extra_args: Vec::new(),
            github_release_update_prompt_enabled: true,
            provider_sync_enabled: false,
            provider_sync_saved_providers: Vec::new(),
            provider_sync_manual_providers: Vec::new(),
            provider_sync_last_selected_provider: String::new(),
            relay_profiles_enabled: true,
            enhancements_enabled: true,
            computer_use_guard_enabled: true,
            codex_app_plugin_entry_unlock: true,
            codex_app_plugin_marketplace_unlock: true,
            codex_app_task_board: true,
            codex_app_session_delete: true,
            codex_app_markdown_export: false,
            codex_app_project_move: false,
            codex_app_conversation_view: true,
            codex_app_token_usage: false,
            codex_app_upstream_worktree_create: false,
            codex_app_native_menu_placement: true,
            codex_app_service_tier_controls: false,
            codex_app_image_overlay_enabled: false,
            codex_app_image_overlay_path: String::new(),
            codex_app_image_overlay_opacity: default_image_overlay_opacity(),
            codex_app_active_skin_id: String::new(),
            codex_goals_enabled: false,
            lan_proxy_enabled: false,
            gpt_reasoning_continuation: false,
            gpt_reasoning_continuation_max_rounds: default_gpt_reasoning_continuation_max_rounds(),
            layered_compaction_enabled: false,
            layered_compaction_retain_tokens: default_layered_compaction_retain_tokens(),
            layered_compaction_prompt_override: String::new(),
            layered_compaction_model_override_enabled: false,
            layered_compaction_models: LayeredCompactionModels::default(),
            layered_compaction_model: String::new(),
            launch_mode: LaunchMode::Patch,
            relay_base_url: default_relay_base_url(),
            relay_api_key: String::new(),
            relay_profiles: default_relay_profiles(),
            relay_common_config_contents: String::new(),
            relay_context_config_contents: String::new(),
            active_relay_id: default_active_relay_id(),
            aggregate_relay_profiles: Vec::new(),
            active_aggregate_relay_id: String::new(),
            relay_test_model: default_relay_test_model(),
            cli_wrapper_enabled: false,
            cli_wrapper_base_url: String::new(),
            cli_wrapper_api_key: String::new(),
            cli_wrapper_api_key_env: default_api_key_env(),
        }
    }
}

impl BackendSettings {
    pub fn compaction_model_for_family(
        &self,
        family: crate::model_capabilities::ModelFamily,
    ) -> &str {
        let configured = self
            .layered_compaction_models
            .model_for_family(family)
            .trim();
        if !configured.is_empty() {
            return configured;
        }
        let legacy = self.layered_compaction_model.trim();
        if !legacy.is_empty() {
            return legacy;
        }
        ""
    }
}

impl BackendSettings {
    pub fn active_relay_profile(&self) -> RelayProfile {
        if self.active_relay_id == default_active_relay_id()
            && self.relay_profiles.len() == 1
            && self.relay_profiles[0] == RelayProfile::default()
            && (!self.relay_api_key.is_empty() || self.relay_base_url != default_relay_base_url())
        {
            return RelayProfile {
                id: default_active_relay_id(),
                name: "默认中转".to_string(),
                model: String::new(),
                base_url: if self.relay_base_url.is_empty() {
                    default_relay_base_url()
                } else {
                    self.relay_base_url.clone()
                },
                upstream_base_url: if self.relay_base_url.is_empty() {
                    default_relay_base_url()
                } else {
                    self.relay_base_url.clone()
                },
                api_key: self.relay_api_key.clone(),
                protocol: RelayProtocol::Responses,
                local_proxy_enabled: Some(false),
                relay_mode: RelayMode::MixedApi,
                official_mix_api_key: true,
                test_model: String::new(),
                config_contents: String::new(),
                auth_contents: String::new(),
                use_common_config: true,
                context_selection: RelayContextSelection::default(),
                context_selection_initialized: false,
                context_window: String::new(),
                auto_compact_limit: String::new(),
                model_insert_mode: RelayModelInsertMode::Patch,
                model_mappings: Vec::new(),
                model_list: String::new(),
                responses_model_list: String::new(),
                chat_completions_model_list: String::new(),
                anthropic_model_list: String::new(),
                responses_websocket: ResponsesWebsocketCapability::default(),
                responses_websocket_enabled: None,
                user_agent: String::new(),
                system_prompt_override: String::new(),
            };
        }

        if let Some(profile) = self
            .relay_profiles
            .iter()
            .find(|profile| profile.id == self.active_relay_id)
        {
            return profile.clone();
        }

        RelayProfile {
            id: if self.active_relay_id.is_empty() {
                default_active_relay_id()
            } else {
                self.active_relay_id.clone()
            },
            name: "默认中转".to_string(),
            model: String::new(),
            base_url: if self.relay_base_url.is_empty() {
                default_relay_base_url()
            } else {
                self.relay_base_url.clone()
            },
            upstream_base_url: if self.relay_base_url.is_empty() {
                default_relay_base_url()
            } else {
                self.relay_base_url.clone()
            },
            api_key: self.relay_api_key.clone(),
            protocol: RelayProtocol::Responses,
            local_proxy_enabled: Some(false),
            relay_mode: RelayMode::Official,
            official_mix_api_key: false,
            test_model: String::new(),
            config_contents: String::new(),
            auth_contents: String::new(),
            use_common_config: true,
            context_selection: RelayContextSelection::default(),
            context_selection_initialized: false,
            context_window: String::new(),
            auto_compact_limit: String::new(),
            model_insert_mode: RelayModelInsertMode::Patch,
            model_mappings: Vec::new(),
            model_list: String::new(),
            responses_model_list: String::new(),
            chat_completions_model_list: String::new(),
            anthropic_model_list: String::new(),
            responses_websocket: ResponsesWebsocketCapability::default(),
            responses_websocket_enabled: None,
            user_agent: String::new(),
            system_prompt_override: String::new(),
        }
    }

    pub fn active_aggregate_relay_profile(&self) -> Option<AggregateRelayProfile> {
        let active_relay = self
            .relay_profiles
            .iter()
            .find(|profile| profile.id == self.active_relay_id)?;
        if active_relay.relay_mode != RelayMode::Aggregate {
            return None;
        }

        let active_aggregate_id = if self.active_aggregate_relay_id.trim().is_empty() {
            active_relay.id.as_str()
        } else {
            self.active_aggregate_relay_id.trim()
        };

        if active_aggregate_id != active_relay.id {
            return None;
        }

        self.aggregate_relay_profiles
            .iter()
            .find(|profile| profile.id == active_aggregate_id)
            .cloned()
    }

    pub fn active_relay_uses_protocol_proxy(&self) -> bool {
        self.relay_profiles_enabled
            && (self.active_aggregate_relay_profile().is_some()
                || self.active_relay_profile().local_proxy_enabled())
    }
}

pub fn default_api_key_env() -> String {
    "CUSTOM_OPENAI_API_KEY".to_string()
}

fn default_image_overlay_opacity() -> u8 {
    35
}

fn clamp_image_overlay_opacity(value: u8) -> u8 {
    value.clamp(1, 100)
}

fn default_gpt_reasoning_continuation_max_rounds() -> u8 {
    crate::continue_thinking::MAX_CONTINUE_ROUNDS as u8
}

fn clamp_gpt_reasoning_continuation_max_rounds(value: u64) -> u8 {
    value.clamp(1, 9) as u8
}

fn default_layered_compaction_retain_tokens() -> u32 {
    crate::layered_compaction::DEFAULT_RETAIN_TOKENS
}

fn clamp_layered_compaction_retain_tokens(value: u64) -> u32 {
    value.clamp(
        crate::layered_compaction::MIN_RETAIN_TOKENS as u64,
        crate::layered_compaction::MAX_RETAIN_TOKENS as u64,
    ) as u32
}

pub fn default_true() -> bool {
    true
}

pub fn default_relay_base_url() -> String {
    String::new()
}

pub fn default_active_relay_id() -> String {
    "default".to_string()
}

pub fn default_relay_test_model() -> String {
    "gpt-5.4-mini".to_string()
}

pub fn default_relay_profiles() -> Vec<RelayProfile> {
    vec![RelayProfile::default()]
}

pub fn default_aggregate_member_weight() -> u32 {
    1
}

pub fn empty_as_default_api_key_env<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_api_key_env))
}

fn deserialize_image_overlay_opacity<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u8>::deserialize(deserializer)?
        .map(clamp_image_overlay_opacity)
        .unwrap_or_else(default_image_overlay_opacity))
}

fn deserialize_gpt_reasoning_continuation_max_rounds<'de, D>(
    deserializer: D,
) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(deserializer)?
        .map(clamp_gpt_reasoning_continuation_max_rounds)
        .unwrap_or_else(default_gpt_reasoning_continuation_max_rounds))
}

fn deserialize_layered_compaction_retain_tokens<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(deserializer)?
        .map(clamp_layered_compaction_retain_tokens)
        .unwrap_or_else(default_layered_compaction_retain_tokens))
}

fn deserialize_profile_api_key<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_codex_home_path<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?
        .map(|value| normalize_codex_home_path(&value))
        .unwrap_or_default())
}

pub fn normalize_codex_extra_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.trim())
        .filter(|arg| !arg.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn normalize_codex_home_path(path: &str) -> String {
    path.trim().trim_matches('"').trim().to_string()
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new(crate::paths::default_settings_path())
    }
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> anyhow::Result<BackendSettings> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BackendSettings::default());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read settings {}", self.path.display()));
            }
        };

        let settings =
            normalize_settings_config_sections(serde_json::from_str(&contents).unwrap_or_default());
        validate_relay_model_protocol_assignments(&settings)?;
        Ok(settings)
    }

    pub fn save(&self, settings: &BackendSettings) -> anyhow::Result<()> {
        let mut settings = normalize_settings_config_sections(settings.clone());
        settings.codex_home_path = normalize_codex_home_path(&settings.codex_home_path);
        settings.codex_extra_args = normalize_codex_extra_args(&settings.codex_extra_args);
        validate_relay_model_mapping_save_constraints(&settings)?;
        let bytes = serde_json::to_vec_pretty(&settings)?;
        atomic_write(&self.path, &bytes)
    }

    pub fn update(&self, payload: Value) -> anyhow::Result<BackendSettings> {
        let Value::Object(payload) = payload else {
            return self.load();
        };

        let mut raw = self.load_raw_object()?;
        merge_known_setting_fields(&mut raw, &payload);
        let settings = normalize_settings_config_sections(
            serde_json::from_value(Value::Object(raw.clone())).unwrap_or_default(),
        );
        validate_relay_model_mapping_save_constraints(&settings)?;
        raw.insert(
            "relayCommonConfigContents".to_string(),
            Value::String(settings.relay_common_config_contents.clone()),
        );
        raw.insert(
            "relayContextConfigContents".to_string(),
            Value::String(settings.relay_context_config_contents.clone()),
        );
        let bytes = serde_json::to_vec_pretty(&Value::Object(raw))?;
        atomic_write(&self.path, &bytes)?;
        Ok(settings)
    }

    fn load_raw_object(&self) -> anyhow::Result<Map<String, Value>> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(settings_to_object(&BackendSettings::default()));
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read settings {}", self.path.display()));
            }
        };

        match serde_json::from_str::<Value>(&contents) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) | Err(_) => Ok(settings_to_object(&BackendSettings::default())),
        }
    }
}

fn validate_relay_model_protocol_assignments(settings: &BackendSettings) -> anyhow::Result<()> {
    for profile in &settings.relay_profiles {
        profile
            .validate_model_protocol_assignments()
            .with_context(|| format!("供应商「{}」模型协议配置无效", profile.name))?;
    }
    Ok(())
}

fn validate_relay_model_mapping_save_constraints(settings: &BackendSettings) -> anyhow::Result<()> {
    validate_relay_model_protocol_assignments(settings)?;
    for profile in &settings.relay_profiles {
        profile
            .validate_model_mapping_save_constraints()
            .with_context(|| format!("供应商「{}」模型配置无效", profile.name))?;
    }
    Ok(())
}

fn merge_known_setting_fields(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    if let Some(value) = source.get("codexAppPath").and_then(Value::as_str) {
        target.insert("codexAppPath".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = source.get("codexHomePath").and_then(Value::as_str) {
        target.insert(
            "codexHomePath".to_string(),
            Value::String(normalize_codex_home_path(value)),
        );
    }
    if let Some(value) = source.get("codexExtraArgs").and_then(Value::as_array) {
        let args = value
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        target.insert(
            "codexExtraArgs".to_string(),
            Value::Array(
                normalize_codex_extra_args(&args)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    merge_bool_setting(target, source, "githubReleaseUpdatePromptEnabled");
    if let Some(value) = source.get("providerSyncEnabled").and_then(Value::as_bool) {
        target.insert("providerSyncEnabled".to_string(), Value::Bool(value));
    }
    if let Some(value) = source.get("relayProfilesEnabled").and_then(Value::as_bool) {
        target.insert("relayProfilesEnabled".to_string(), Value::Bool(value));
    }
    if let Some(value) = source.get("enhancementsEnabled").and_then(Value::as_bool) {
        target.insert("enhancementsEnabled".to_string(), Value::Bool(value));
    }
    if let Some(value) = source
        .get("computerUseGuardEnabled")
        .and_then(Value::as_bool)
    {
        target.insert("computerUseGuardEnabled".to_string(), Value::Bool(value));
    }
    merge_bool_setting(target, source, "codexAppPluginEntryUnlock");
    merge_bool_setting(target, source, "codexAppPluginMarketplaceUnlock");
    merge_bool_setting(target, source, "codexAppTaskBoard");
    merge_bool_setting(target, source, "codexAppSessionDelete");
    merge_bool_setting(target, source, "codexAppMarkdownExport");
    merge_bool_setting(target, source, "codexAppProjectMove");
    merge_bool_setting(target, source, "codexAppConversationView");
    merge_bool_setting(target, source, "codexAppTokenUsage");
    merge_bool_setting(target, source, "codexAppUpstreamWorktreeCreate");
    merge_bool_setting(target, source, "codexAppNativeMenuPlacement");
    merge_bool_setting(target, source, "codexAppServiceTierControls");
    merge_bool_setting(target, source, "codexAppImageOverlayEnabled");
    if let Some(value) = source
        .get("codexAppImageOverlayPath")
        .and_then(Value::as_str)
    {
        target.insert(
            "codexAppImageOverlayPath".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source
        .get("codexAppImageOverlayOpacity")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
    {
        target.insert(
            "codexAppImageOverlayOpacity".to_string(),
            Value::Number(serde_json::Number::from(clamp_image_overlay_opacity(value))),
        );
    }
    if let Some(value) = source.get("codexAppActiveSkinId").and_then(Value::as_str) {
        target.insert(
            "codexAppActiveSkinId".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source.get("codexGoalsEnabled").and_then(Value::as_bool) {
        target.insert("codexGoalsEnabled".to_string(), Value::Bool(value));
    }
    merge_bool_setting(target, source, "lanProxyEnabled");
    merge_bool_setting(target, source, "gptReasoningContinuation");
    if let Some(value) = source
        .get("gptReasoningContinuationMaxRounds")
        .and_then(Value::as_u64)
    {
        target.insert(
            "gptReasoningContinuationMaxRounds".to_string(),
            Value::Number(serde_json::Number::from(
                clamp_gpt_reasoning_continuation_max_rounds(value),
            )),
        );
    }
    if let Some(value) = source.get("launchMode").and_then(Value::as_str) {
        if matches!(value, "patch" | "relay") {
            target.insert("launchMode".to_string(), Value::String(value.to_string()));
        }
    }
    if let Some(value) = source.get("relayBaseUrl").and_then(Value::as_str) {
        target.insert("relayBaseUrl".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = source.get("relayApiKey").and_then(Value::as_str) {
        target.insert("relayApiKey".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = source.get("relayProfiles").and_then(Value::as_array) {
        let mut profiles = serde_json::from_value::<Vec<RelayProfile>>(Value::Array(value.clone()))
            .unwrap_or_default();
        preserve_official_mix_bearer_tokens(&mut profiles, target);
        for profile in &mut profiles {
            let _ = crate::relay_config::normalize_relay_profile_for_storage(profile);
            crate::responses_websocket::normalize_responses_websocket_capability(profile);
        }
        target.insert(
            "relayProfiles".to_string(),
            serde_json::to_value(profiles).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
    }
    if let Some(value) = source
        .get("relayCommonConfigContents")
        .and_then(Value::as_str)
    {
        target.insert(
            "relayCommonConfigContents".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source
        .get("relayContextConfigContents")
        .and_then(Value::as_str)
    {
        target.insert(
            "relayContextConfigContents".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source.get("activeRelayId").and_then(Value::as_str) {
        target.insert(
            "activeRelayId".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source
        .get("aggregateRelayProfiles")
        .and_then(Value::as_array)
    {
        target.insert(
            "aggregateRelayProfiles".to_string(),
            Value::Array(value.clone()),
        );
    }
    if let Some(value) = source.get("activeAggregateRelayId").and_then(Value::as_str) {
        target.insert(
            "activeAggregateRelayId".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source.get("relayTestModel").and_then(Value::as_str) {
        target.insert(
            "relayTestModel".to_string(),
            Value::String(if value.trim().is_empty() {
                default_relay_test_model()
            } else {
                value.trim().to_string()
            }),
        );
    }
    if let Some(value) = source.get("cliWrapperEnabled").and_then(Value::as_bool) {
        target.insert("cliWrapperEnabled".to_string(), Value::Bool(value));
    }
    if let Some(value) = source.get("cliWrapperBaseUrl").and_then(Value::as_str) {
        target.insert(
            "cliWrapperBaseUrl".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source.get("cliWrapperApiKey").and_then(Value::as_str) {
        target.insert(
            "cliWrapperApiKey".to_string(),
            Value::String(value.to_string()),
        );
    }
    if let Some(value) = source.get("cliWrapperApiKeyEnv").and_then(Value::as_str) {
        target.insert(
            "cliWrapperApiKeyEnv".to_string(),
            Value::String(if value.is_empty() {
                default_api_key_env()
            } else {
                value.to_string()
            }),
        );
    }
}

fn merge_bool_setting(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key).and_then(Value::as_bool) {
        target.insert(key.to_string(), Value::Bool(value));
    }
}

fn preserve_official_mix_bearer_tokens(
    profiles: &mut [RelayProfile],
    previous: &Map<String, Value>,
) {
    let previous_tokens = previous
        .get("relayProfiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| serde_json::from_value::<RelayProfile>(value.clone()).ok())
        .filter_map(|profile| {
            if profile.relay_mode != RelayMode::Official || !profile.official_mix_api_key {
                return None;
            }
            let token = experimental_bearer_token_from_config_text(&profile.config_contents)?;
            Some((profile.id, token))
        })
        .collect::<HashMap<_, _>>();

    for profile in profiles {
        if profile.relay_mode != RelayMode::Official || !profile.official_mix_api_key {
            continue;
        }
        if experimental_bearer_token_from_config_text(&profile.config_contents).is_some() {
            continue;
        }
        let token = if profile.api_key.trim().is_empty() {
            previous_tokens.get(&profile.id).cloned()
        } else {
            Some(profile.api_key.trim().to_string())
        };
        let Some(token) = token else {
            continue;
        };
        profile.config_contents =
            set_or_replace_experimental_bearer_token(&profile.config_contents, &token);
    }
}

fn set_or_replace_experimental_bearer_token(contents: &str, token: &str) -> String {
    let mut doc = parse_toml_document(contents).unwrap_or_else(|_| DocumentMut::new());
    let provider_id = active_provider_id(&doc).unwrap_or_else(|| "codex-elves-relay".to_string());
    doc["model_provider"] = toml_edit::value(provider_id.as_str());
    doc["model_providers"][provider_id.as_str()]["experimental_bearer_token"] =
        toml_edit::value(token.trim());
    ensure_text_newline(doc.to_string())
}

fn ensure_text_newline(mut value: String) -> String {
    if !value.is_empty() && !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

fn experimental_bearer_token_from_config_text(contents: &str) -> Option<String> {
    let doc = parse_toml_document(contents).ok()?;
    let provider_id = active_provider_id(&doc)?;
    doc.get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(&provider_id))
        .and_then(Item::as_table)
        .and_then(|provider| provider.get("experimental_bearer_token"))
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn active_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(ToString::to_string)
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

fn settings_to_object(settings: &BackendSettings) -> Map<String, Value> {
    match serde_json::to_value(settings).unwrap_or_else(|_| Value::Object(Map::new())) {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn normalize_settings_config_sections(mut settings: BackendSettings) -> BackendSettings {
    let (common, extracted_context) =
        split_context_config_sections(&settings.relay_common_config_contents);
    let context = join_config_sections(&[
        settings.relay_context_config_contents.as_str(),
        extracted_context.as_str(),
    ]);
    settings.relay_common_config_contents = crate::relay_config::normalize_config_text(&common);
    settings.relay_context_config_contents = crate::relay_config::normalize_config_text(&context);
    for profile in &mut settings.relay_profiles {
        let _ = crate::relay_config::normalize_relay_profile_for_storage(profile);
        crate::responses_websocket::normalize_responses_websocket_capability(profile);
    }
    settings.codex_home_path = normalize_codex_home_path(&settings.codex_home_path);
    settings.codex_app_image_overlay_opacity =
        clamp_image_overlay_opacity(settings.codex_app_image_overlay_opacity);
    settings.gpt_reasoning_continuation_max_rounds = clamp_gpt_reasoning_continuation_max_rounds(
        u64::from(settings.gpt_reasoning_continuation_max_rounds),
    );
    settings
}

fn split_context_config_sections(config: &str) -> (String, String) {
    let mut common = Vec::new();
    let mut context = Vec::new();
    let mut in_context_table = false;

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_context_table = is_context_table_header(trimmed);
        }
        if in_context_table {
            context.push(line);
        } else {
            common.push(line);
        }
    }

    (
        normalize_text_config(common.join("\n")),
        normalize_text_config(context.join("\n")),
    )
}

fn is_context_table_header(header: &str) -> bool {
    header.starts_with("[mcp_servers.")
        || header.starts_with("[skills.")
        || header.starts_with("[plugins.")
}

fn join_config_sections(sections: &[&str]) -> String {
    let joined = sections
        .iter()
        .map(|section| section.trim())
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    normalize_text_config(joined)
}

fn normalize_text_config(contents: String) -> String {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let temp_path = temp_path_for(path);
    fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write temp file {}", temp_path.display()))?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            path.display(),
            temp_path.display()
        )
    })?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut temp_path = path.to_path_buf();
    let extension = path.extension().and_then(|value| value.to_str());
    temp_path.set_extension(match extension {
        Some(extension) => format!("{extension}.tmp"),
        None => "tmp".to_string(),
    });
    temp_path
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codex-elves-core-settings-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn settings_default_matches_expected_behavior() {
        let settings = BackendSettings::default();
        assert!(settings.github_release_update_prompt_enabled);
        assert!(!settings.provider_sync_enabled);
        assert!(settings.relay_profiles_enabled);
        assert!(settings.enhancements_enabled);
        assert!(settings.computer_use_guard_enabled);
        assert!(settings.codex_app_plugin_entry_unlock);
        assert!(settings.codex_app_plugin_marketplace_unlock);
        assert!(settings.codex_app_task_board);
        assert!(settings.codex_app_session_delete);
        assert!(!settings.codex_app_markdown_export);
        assert!(!settings.codex_app_project_move);
        assert!(settings.codex_app_conversation_view);
        assert!(!settings.codex_app_token_usage);
        assert!(!settings.codex_app_upstream_worktree_create);
        assert!(settings.codex_app_native_menu_placement);
        assert!(!settings.codex_goals_enabled);
        assert!(!settings.lan_proxy_enabled);
        assert!(settings.codex_app_path.is_empty());
        assert!(settings.codex_extra_args.is_empty());
        assert_eq!(settings.launch_mode, LaunchMode::Patch);
        assert_eq!(settings.relay_base_url, default_relay_base_url());
        assert!(settings.relay_api_key.is_empty());
        assert_eq!(settings.relay_profiles[0].relay_mode, RelayMode::Official);
        assert!(settings.relay_common_config_contents.is_empty());
        assert_eq!(settings.relay_test_model, default_relay_test_model());
        assert!(!settings.cli_wrapper_enabled);
        assert_eq!(settings.cli_wrapper_api_key_env, "CUSTOM_OPENAI_API_KEY");
        assert_eq!(settings.gpt_reasoning_continuation_max_rounds, 3);
        assert!(settings.codex_home_path.is_empty());
        assert!(
            serde_json::to_value(&settings)
                .unwrap()
                .get("codexAppPluginAutoExpand")
                .is_none()
        );
    }

    #[test]
    fn settings_deserialize_uses_existing_json_keys() {
        let settings: BackendSettings = serde_json::from_str(
            r#"{"codexAppPath":"C:\\Portable\\Codex\\app","codexHomePath":" C:\\Portable\\CodexHome ","providerSyncEnabled":true,"codexGoalsEnabled":true,"lanProxyEnabled":true,"cliWrapperEnabled":true,"cliWrapperBaseUrl":"https://example.test","cliWrapperApiKey":"sk-test","cliWrapperApiKeyEnv":""}"#,
        )
        .unwrap();
        assert_eq!(settings.codex_app_path, r"C:\Portable\Codex\app");
        assert_eq!(settings.codex_home_path, r"C:\Portable\CodexHome");
        assert!(settings.provider_sync_enabled);
        assert!(settings.codex_goals_enabled);
        assert!(settings.lan_proxy_enabled);
        assert!(settings.cli_wrapper_enabled);
        assert_eq!(settings.cli_wrapper_base_url, "https://example.test");
        assert_eq!(settings.cli_wrapper_api_key, "sk-test");
        assert_eq!(settings.cli_wrapper_api_key_env, "CUSTOM_OPENAI_API_KEY");
        assert_eq!(settings.relay_base_url, default_relay_base_url());
        assert!(settings.codex_extra_args.is_empty());
        assert!(settings.codex_app_task_board);
        assert_eq!(settings.gpt_reasoning_continuation_max_rounds, 3);
        assert_eq!(settings.layered_compaction_retain_tokens, 20_000);
    }

    #[test]
    fn settings_store_update_persists_lan_proxy_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(temp.path().join("settings.json"));

        store.update(json!({ "lanProxyEnabled": true })).unwrap();
        assert!(store.load().unwrap().lan_proxy_enabled);
    }

    #[test]
    fn settings_clamps_gpt_reasoning_continuation_max_rounds() {
        let low: BackendSettings =
            serde_json::from_str(r#"{"gptReasoningContinuationMaxRounds":0}"#).unwrap();
        assert_eq!(low.gpt_reasoning_continuation_max_rounds, 1);

        let high: BackendSettings =
            serde_json::from_str(r#"{"gptReasoningContinuationMaxRounds":999}"#).unwrap();
        assert_eq!(high.gpt_reasoning_continuation_max_rounds, 9);
    }

    #[test]
    fn settings_clamps_layered_compaction_retain_tokens() {
        let low: BackendSettings =
            serde_json::from_str(r#"{"layeredCompactionRetainTokens":10000}"#).unwrap();
        assert_eq!(low.layered_compaction_retain_tokens, 20_000);

        let high: BackendSettings =
            serde_json::from_str(r#"{"layeredCompactionRetainTokens":999999}"#).unwrap();
        assert_eq!(high.layered_compaction_retain_tokens, 64_000);
    }

    #[test]
    fn legacy_single_compaction_model_applies_to_all_source_families() {
        let settings = BackendSettings {
            layered_compaction_model: "deepseek-chat".to_string(),
            ..BackendSettings::default()
        };

        assert_eq!(
            settings.compaction_model_for_family(crate::model_capabilities::ModelFamily::Gpt),
            "deepseek-chat"
        );
        assert_eq!(
            settings.compaction_model_for_family(crate::model_capabilities::ModelFamily::Claude),
            "deepseek-chat"
        );
        assert_eq!(
            settings.compaction_model_for_family(crate::model_capabilities::ModelFamily::Other),
            "deepseek-chat"
        );
    }

    #[test]
    fn settings_deserialize_keeps_plugin_unlock_switches_independent() {
        let settings: BackendSettings = serde_json::from_str(
            r#"{
                "codexAppPluginEntryUnlock": false,
                "codexAppPluginMarketplaceUnlock": true
            }"#,
        )
        .unwrap();

        assert!(!settings.codex_app_plugin_entry_unlock);
        assert!(settings.codex_app_plugin_marketplace_unlock);

        let legacy_settings: BackendSettings = serde_json::from_str(
            r#"{
                "codexAppPluginEntryUnlock": false
            }"#,
        )
        .unwrap();

        assert!(!legacy_settings.codex_app_plugin_entry_unlock);
        assert!(legacy_settings.codex_app_plugin_marketplace_unlock);
    }

    #[test]
    fn settings_deserialize_reads_codex_extra_args() {
        let settings: BackendSettings = serde_json::from_str(
            r#"{"codexExtraArgs":["--force_high_performance_gpu"," --ignored-trimmed-by-ui "]}"#,
        )
        .unwrap();

        assert_eq!(
            settings.codex_extra_args,
            vec![
                "--force_high_performance_gpu".to_string(),
                " --ignored-trimmed-by-ui ".to_string(),
            ]
        );
    }

    #[test]
    fn relay_profile_official_mix_api_key_defaults_to_false() {
        let profile: RelayProfile =
            serde_json::from_str(r#"{"id":"official","name":"官方","relayMode":"official"}"#)
                .unwrap();

        assert_eq!(profile.relay_mode, RelayMode::Official);
        assert!(!profile.official_mix_api_key);
        assert!(profile.test_model.is_empty());
    }

    #[test]
    fn relay_profile_context_fields_default_to_empty() {
        let profile = RelayProfile::default();

        assert!(profile.context_selection.mcp_servers.is_empty());
        assert!(profile.context_selection.skills.is_empty());
        assert!(profile.context_selection.plugins.is_empty());
        assert!(profile.use_common_config);
        assert!(!profile.context_selection_initialized);
        assert!(profile.context_window.is_empty());
        assert!(profile.auto_compact_limit.is_empty());
        assert_eq!(profile.model_insert_mode, RelayModelInsertMode::Patch);
        assert!(profile.model_mappings.is_empty());
        assert!(profile.model_list.is_empty());
    }

    #[test]
    fn relay_profile_context_fields_deserialize_from_camel_case() {
        let profile: RelayProfile = serde_json::from_str(
            r#"{
                "id":"relay-a",
                "name":"供应商 A",
                "contextSelection":{
                    "mcpServers":["context7"],
                    "skills":["writer"],
                    "plugins":["local"]
                },
                "contextSelectionInitialized":true,
                "useCommonConfig":false,
                "contextWindow":"200000",
                "autoCompactLimit":"160000",
                "modelInsertMode":"patch",
                "modelMappings":[
                    {"requestModel":"qwen3-coder","protocol":"responses","contextWindow":"200000"},
                    {"requestModel":"deepseek-coder","protocol":"chatCompletions","contextWindow":"128000"},
                    {"requestModel":"claude-sonnet-4","protocol":"anthropic","contextWindow":"200000"}
                ],
                "modelList":"qwen3-coder\ndeepseek-coder",
                "anthropicModelList":"claude-sonnet-4"
            }"#,
        )
        .unwrap();

        assert_eq!(profile.context_selection.mcp_servers, vec!["context7"]);
        assert_eq!(profile.context_selection.skills, vec!["writer"]);
        assert_eq!(profile.context_selection.plugins, vec!["local"]);
        assert!(!profile.use_common_config);
        assert!(profile.context_selection_initialized);
        assert_eq!(profile.context_window, "200000");
        assert_eq!(profile.auto_compact_limit, "160000");
        assert_eq!(profile.model_insert_mode, RelayModelInsertMode::Patch);
        assert_eq!(profile.model_mappings.len(), 3);
        assert_eq!(profile.model_mappings[0].request_model, "qwen3-coder");
        assert_eq!(profile.model_mappings[0].protocol, RelayProtocol::Responses);
        assert_eq!(profile.model_mappings[0].context_window, "200000");
        assert_eq!(
            profile.model_mappings[1].protocol,
            RelayProtocol::ChatCompletions
        );
        assert_eq!(profile.model_mappings[2].protocol, RelayProtocol::Anthropic);
        assert_eq!(profile.model_list, "qwen3-coder\ndeepseek-coder");
        assert_eq!(profile.anthropic_model_list, "claude-sonnet-4");
    }

    #[test]
    fn relay_profile_round_trips_model_mapping_alias() {
        let profile: RelayProfile = serde_json::from_str(
            r#"{
                "id":"relay-a",
                "name":"供应商 A",
                "modelMappings":[
                    {
                        "requestModel":"gpt-5.6-sol",
                        "alias":"主力编程模型",
                        "protocol":"responses",
                        "contextWindow":"372000"
                    }
                ]
            }"#,
        )
        .unwrap();

        let saved = serde_json::to_value(&profile).unwrap();

        assert_eq!(saved["modelMappings"][0]["requestModel"], "gpt-5.6-sol");
        assert_eq!(saved["modelMappings"][0]["alias"], "主力编程模型");
    }

    #[test]
    fn relay_profile_context_window_backfills_required_fable_only() {
        let profile = RelayProfile {
            model_mappings: vec![
                RelayModelMapping {
                    request_model: "claude-fable-5".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Anthropic,
                    context_window: String::new(),
                },
                RelayModelMapping {
                    request_model: "gpt-5.6".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: String::new(),
                },
            ],
            ..RelayProfile::default()
        };

        assert_eq!(
            profile.context_window_for_model("claude-fable-5"),
            Some(1_000_000)
        );
        assert_eq!(profile.context_window_for_model("gpt-5.6"), None);
    }

    #[test]
    fn relay_profile_protocol_resolution_requires_explicit_model_mapping() {
        let profile = RelayProfile {
            protocol: RelayProtocol::Responses,
            model_mappings: vec![
                RelayModelMapping {
                    request_model: "gpt-chat".to_string(),
                    alias: "对话模型".to_string(),
                    protocol: RelayProtocol::ChatCompletions,
                    context_window: "200000".to_string(),
                },
                RelayModelMapping {
                    request_model: "claude-sonnet-4".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Anthropic,
                    context_window: "200000".to_string(),
                },
            ],
            ..RelayProfile::default()
        };

        assert_eq!(
            profile.resolve_protocol_for_model("gpt-chat").unwrap(),
            RelayProtocol::ChatCompletions
        );
        assert_eq!(
            profile.resolve_protocol_for_model("对话模型").unwrap(),
            RelayProtocol::ChatCompletions
        );
        assert_eq!(
            profile
                .resolve_protocol_for_model("claude-sonnet-4")
                .unwrap(),
            RelayProtocol::Anthropic
        );
        let error = profile.resolve_protocol_for_model("gpt-other").unwrap_err();
        assert!(error.to_string().contains("没有明确协议归属"), "{error:#}");
    }

    #[test]
    fn relay_profile_keeps_plain_request_model_when_only_other_mapping_has_alias() {
        let profile = RelayProfile {
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
            ..RelayProfile::default()
        };

        assert_eq!(
            relay_model_mapping_catalog_slugs(&profile.model_mappings),
            vec!["gpt-5.6-sol", "gpt-5.6-sol [500K]"]
        );

        assert_eq!(
            profile.resolve_protocol_for_model("gpt-5.6-sol").unwrap(),
            RelayProtocol::Responses
        );
        assert_eq!(
            profile.context_window_for_model("gpt-5.6-sol"),
            Some(372_000)
        );
        assert_eq!(
            profile.catalog_model_for_configured_model("gpt-5.6-sol"),
            "gpt-5.6-sol"
        );
        assert_eq!(
            profile
                .resolve_protocol_for_model("gpt-5.6-sol [500K]")
                .unwrap(),
            RelayProtocol::Responses
        );
        assert_eq!(
            profile.context_window_for_model("gpt-5.6-sol [500K]"),
            Some(500_000)
        );
    }

    #[test]
    fn relay_profile_keeps_plain_request_models_when_each_unaliased_model_is_unique() {
        let mappings = vec![
            RelayModelMapping {
                request_model: "gpt-5.6-terra".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "372000".to_string(),
            },
            RelayModelMapping {
                request_model: "gpt-5.6-luna".to_string(),
                alias: String::new(),
                protocol: RelayProtocol::Responses,
                context_window: "500000".to_string(),
            },
        ];

        assert_eq!(
            relay_model_mapping_catalog_slugs(&mappings),
            vec!["gpt-5.6-terra", "gpt-5.6-luna"]
        );
    }

    #[test]
    fn relay_profile_adds_context_for_duplicate_unaliased_request_models() {
        let profile = RelayProfile {
            model_mappings: vec![
                RelayModelMapping {
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "372000".to_string(),
                },
                RelayModelMapping {
                    request_model: "gpt-5.6-sol".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "500000".to_string(),
                },
            ],
            ..RelayProfile::default()
        };

        assert_eq!(
            relay_model_mapping_catalog_slugs(&profile.model_mappings),
            vec!["gpt-5.6-sol 372000", "gpt-5.6-sol 500000"]
        );
        assert_eq!(
            profile.context_window_for_model("gpt-5.6-sol 372000"),
            Some(372_000)
        );
        assert_eq!(
            profile.context_window_for_model("gpt-5.6-sol 500000"),
            Some(500_000)
        );
    }

    #[test]
    fn legacy_ambiguous_alias_keeps_legacy_request_model_precedence() {
        let profile = RelayProfile {
            model_mappings: vec![
                RelayModelMapping {
                    request_model: "model-a".to_string(),
                    alias: "model-b".to_string(),
                    protocol: RelayProtocol::ChatCompletions,
                    context_window: "400000".to_string(),
                },
                RelayModelMapping {
                    request_model: "model-b".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "500000".to_string(),
                },
            ],
            ..RelayProfile::default()
        };

        assert_eq!(
            profile.request_model_for_catalog_model("model-b"),
            "model-b"
        );
        assert_eq!(
            profile.request_model_for_catalog_model("model-a"),
            "model-a"
        );
        assert_eq!(
            profile.catalog_model_for_configured_model("model-b"),
            "model-b"
        );
        assert_eq!(
            profile.resolve_protocol_for_model("model-b").unwrap(),
            RelayProtocol::Responses
        );
        assert_eq!(profile.context_window_for_model("model-b"), Some(500_000));
    }

    #[test]
    fn relay_profile_rejects_conflicting_duplicate_model_mappings() {
        let profile = RelayProfile {
            model_mappings: vec![
                RelayModelMapping {
                    request_model: "gpt-conflict".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::Responses,
                    context_window: "200000".to_string(),
                },
                RelayModelMapping {
                    request_model: "gpt-conflict".to_string(),
                    alias: String::new(),
                    protocol: RelayProtocol::ChatCompletions,
                    context_window: "200000".to_string(),
                },
            ],
            ..RelayProfile::default()
        };

        let error = profile.validate_model_protocol_assignments().unwrap_err();
        assert!(error.to_string().contains("冲突协议归属"), "{error:#}");
        assert!(profile.resolve_protocol_for_model("gpt-conflict").is_err());
    }

    #[test]
    fn settings_store_rejects_duplicate_model_mapping_parameters_ignoring_alias() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "duplicate-model-parameters".to_string(),
                name: "重复模型参数".to_string(),
                model_mappings: vec![
                    RelayModelMapping {
                        request_model: "gpt-5.6-sol".to_string(),
                        alias: "gpt-5.6-sol [500K]".to_string(),
                        protocol: RelayProtocol::Responses,
                        context_window: "500000".to_string(),
                    },
                    RelayModelMapping {
                        request_model: "gpt-5.6-sol".to_string(),
                        alias: "编程模型".to_string(),
                        protocol: RelayProtocol::Responses,
                        context_window: "500000".to_string(),
                    },
                ],
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let error = store.save(&settings).unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("模型配置重复"), "{message}");
        assert!(!path.exists());
    }

    #[test]
    fn settings_store_loads_legacy_duplicate_parameters_but_rejects_resave() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{
                "relayProfiles": [{
                    "id": "legacy-duplicates",
                    "name": "历史重复参数",
                    "modelMappings": [
                        {
                            "requestModel": "gpt-5.6-sol",
                            "alias": "gpt-5.6-sol [500K]",
                            "protocol": "responses",
                            "contextWindow": "500000"
                        },
                        {
                            "requestModel": "gpt-5.6-sol",
                            "alias": "编程模型",
                            "protocol": "responses",
                            "contextWindow": "500000"
                        }
                    ]
                }]
            }"#,
        )
        .unwrap();
        let store = SettingsStore::new(path);

        let loaded = store.load().unwrap();

        assert_eq!(loaded.relay_profiles[0].model_mappings.len(), 2);
        let error = store.save(&loaded).unwrap_err();
        assert!(format!("{error:#}").contains("模型配置重复"));
    }

    #[test]
    fn settings_store_rejects_duplicate_catalog_model_identifiers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "duplicate-catalog-model".to_string(),
                name: "重复目录标识".to_string(),
                model_mappings: vec![
                    RelayModelMapping {
                        request_model: "gpt-5.6-sol".to_string(),
                        alias: "主力模型".to_string(),
                        protocol: RelayProtocol::Responses,
                        context_window: "500000".to_string(),
                    },
                    RelayModelMapping {
                        request_model: "gpt-5.6-luna".to_string(),
                        alias: "主力模型".to_string(),
                        protocol: RelayProtocol::Responses,
                        context_window: "600000".to_string(),
                    },
                ],
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let error = store.save(&settings).unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("模型标识重复"), "{message}");
        assert!(!path.exists());
    }

    #[test]
    fn settings_store_rejects_catalog_identifier_matching_other_request_model() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "ambiguous-catalog-model".to_string(),
                name: "目录标识歧义".to_string(),
                model_mappings: vec![
                    RelayModelMapping {
                        request_model: "model-a".to_string(),
                        alias: "model-b".to_string(),
                        protocol: RelayProtocol::Responses,
                        context_window: "400000".to_string(),
                    },
                    RelayModelMapping {
                        request_model: "model-b".to_string(),
                        alias: String::new(),
                        protocol: RelayProtocol::Responses,
                        context_window: "500000".to_string(),
                    },
                ],
                ..RelayProfile::default()
            }],
            ..BackendSettings::default()
        };

        let error = store.save(&settings).unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("与另一条请求模型冲突"), "{message}");
        assert!(!path.exists());
    }

    #[test]
    fn relay_profile_rejects_model_list_protocol_conflicts() {
        let profile = RelayProfile {
            responses_model_list: "shared-model".to_string(),
            anthropic_model_list: "shared-model".to_string(),
            ..RelayProfile::default()
        };

        let error = profile.validate_model_protocol_assignments().unwrap_err();
        assert!(error.to_string().contains("冲突协议归属"), "{error:#}");
    }

    #[test]
    fn relay_profile_derived_fields_are_read_but_not_serialized() {
        let profile: RelayProfile = serde_json::from_str(
            r#"{
                "id":"relay-a",
                "name":"供应商 A",
                "model":"gpt-5.4",
                "baseUrl":"https://relay.example/v1",
                "apiKey":"sk-test",
                "configContents":"model = \"gpt-5.4\"\n",
                "authContents":"{\"OPENAI_API_KEY\":\"sk-test\"}"
            }"#,
        )
        .unwrap();

        assert_eq!(profile.model, "gpt-5.4");
        assert_eq!(profile.base_url, "https://relay.example/v1");
        assert_eq!(profile.api_key, "sk-test");

        let saved = serde_json::to_value(&profile).unwrap();
        assert!(saved.get("model").is_none());
        assert!(saved.get("baseUrl").is_none());
        assert!(saved.get("apiKey").is_none());
        assert_eq!(saved["configContents"], "model = \"gpt-5.4\"\n");
        assert_eq!(saved["authContents"], "{\"OPENAI_API_KEY\":\"sk-test\"}");
    }

    #[test]
    fn chat_protocol_profile_roundtrip_migrates_upstream_base_url_out_of_config() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "relay-chat".to_string(),
                name: "DeepSeek".to_string(),
                protocol: RelayProtocol::ChatCompletions,
                relay_mode: RelayMode::PureApi,
                config_contents: r#"model = "deepseek-chat"
codex_elves_chat_base_url = "https://api.deepseek.com"
model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://127.0.0.1:45221/v1"
"#
                .to_string(),
                auth_contents: r#"{"OPENAI_API_KEY":"sk-test"}"#.to_string(),
                ..RelayProfile::default()
            }],
            active_relay_id: "relay-chat".to_string(),
            ..BackendSettings::default()
        };

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();
        let active = loaded.active_relay_profile();

        assert_eq!(active.protocol, RelayProtocol::ChatCompletions);
        assert_eq!(active.base_url, "https://api.deepseek.com");
        assert_eq!(active.upstream_base_url, "https://api.deepseek.com");
        assert_eq!(active.api_key, "sk-test");
        assert!(!active.config_contents.contains("codex_elves_chat_base_url"));

        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        let profile = &saved["relayProfiles"][0];
        assert!(profile.get("baseUrl").is_none());
        assert_eq!(profile["upstreamBaseUrl"], "https://api.deepseek.com");
        assert!(profile.get("apiKey").is_none());
        assert!(
            !profile["configContents"]
                .as_str()
                .unwrap()
                .contains("codex_elves_chat_base_url")
        );
    }

    #[test]
    fn responses_websocket_cache_roundtrip_resets_after_base_url_change() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        let mut settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "relay-responses".to_string(),
                name: "Responses".to_string(),
                relay_mode: RelayMode::PureApi,
                protocol: RelayProtocol::Responses,
                upstream_base_url: "https://relay-a.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                auth_contents: r#"{"OPENAI_API_KEY":"sk-test"}"#.to_string(),
                config_contents: r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://relay-a.example/v1"
"#
                .to_string(),
                responses_websocket: ResponsesWebsocketCapability {
                    state: ResponsesWebsocketCapabilityState::Supported,
                    endpoint: "wss://relay-a.example/v1/responses".to_string(),
                    checked_at_ms: Some(1_720_000_000_000),
                    message: "握手成功".to_string(),
                },
                ..RelayProfile::default()
            }],
            active_relay_id: "relay-responses".to_string(),
            ..BackendSettings::default()
        };

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.relay_profiles[0].responses_websocket.state,
            ResponsesWebsocketCapabilityState::Supported
        );

        settings = loaded;
        settings.relay_profiles[0].upstream_base_url = "https://relay-b.example/v1".to_string();
        settings.relay_profiles[0].config_contents = settings.relay_profiles[0]
            .config_contents
            .replace("https://relay-a.example/v1", "https://relay-b.example/v1");
        store.save(&settings).unwrap();

        let changed = store.load().unwrap();
        assert_eq!(
            changed.relay_profiles[0].responses_websocket.state,
            ResponsesWebsocketCapabilityState::Unknown
        );
        assert_eq!(
            changed.relay_profiles[0].responses_websocket.endpoint,
            "wss://relay-b.example/v1/responses"
        );
        assert_eq!(
            changed.relay_profiles[0].responses_websocket.checked_at_ms,
            None
        );
        assert!(
            changed.relay_profiles[0]
                .responses_websocket
                .message
                .is_empty()
        );
    }

    #[test]
    fn official_profile_without_mix_does_not_persist_api_config() {
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "official".to_string(),
                name: "官方".to_string(),
                relay_mode: RelayMode::Official,
                official_mix_api_key: false,
                model: "gpt-5.5".to_string(),
                base_url: "https://relay.example/v1".to_string(),
                api_key: "sk-test".to_string(),
                config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
requires_openai_auth = true
"#
                .to_string(),
                auth_contents: r#"{"OPENAI_API_KEY":"sk-test"}"#.to_string(),
                ..RelayProfile::default()
            }],
            active_relay_id: "official".to_string(),
            ..BackendSettings::default()
        };

        let value = settings_to_object(&normalize_settings_config_sections(settings));
        let profile = &value["relayProfiles"][0];
        assert_eq!(profile["relayMode"], "official");
        assert_eq!(profile["officialMixApiKey"], false);
        assert_eq!(profile["configContents"], "");
        assert_eq!(profile["authContents"], "");
        assert!(profile.get("model").is_none());
        assert!(profile.get("baseUrl").is_none());
        assert!(profile.get("apiKey").is_none());
    }

    #[test]
    fn official_mix_profile_keeps_key_in_config_not_auth() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "official-mix".to_string(),
                name: "官方混入".to_string(),
                relay_mode: RelayMode::Official,
                official_mix_api_key: true,
                model: "gpt-5.5".to_string(),
                base_url: "https://relay.example/v1".to_string(),
                api_key: "sk-mix".to_string(),
                config_contents: r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
requires_openai_auth = true
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-mix"
"#
                .to_string(),
                auth_contents: r#"{"OPENAI_API_KEY":"sk-mix","auth_mode":"chatgpt"}"#.to_string(),
                ..RelayProfile::default()
            }],
            active_relay_id: "official-mix".to_string(),
            ..BackendSettings::default()
        };

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();
        let profile = &loaded.relay_profiles[0];

        assert_eq!(profile.relay_mode, RelayMode::Official);
        assert!(profile.official_mix_api_key);
        assert_eq!(profile.api_key, "sk-mix");
        assert!(!profile.auth_contents.contains("OPENAI_API_KEY"));
        assert!(
            profile
                .config_contents
                .contains(r#"experimental_bearer_token = "sk-mix""#)
        );

        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(saved["relayProfiles"][0].get("apiKey").is_none());
        assert!(
            !saved["relayProfiles"][0]["authContents"]
                .as_str()
                .unwrap()
                .contains("OPENAI_API_KEY")
        );
        assert!(
            saved["relayProfiles"][0]["configContents"]
                .as_str()
                .unwrap()
                .contains(r#"experimental_bearer_token = "sk-mix""#)
        );
    }

    #[test]
    fn settings_update_preserves_official_mix_key_when_payload_loses_it() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        store
            .save(&BackendSettings {
                relay_profiles: vec![RelayProfile {
                    id: "official-mix".to_string(),
                    name: "官方混入".to_string(),
                    relay_mode: RelayMode::Official,
                    official_mix_api_key: true,
                    config_contents: r#"model_provider = "custom"

[model_providers.other]
base_url = "https://other.example/v1"
experimental_bearer_token = "sk-other"

[model_providers.custom]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-existing"
"#
                    .to_string(),
                    ..RelayProfile::default()
                }],
                active_relay_id: "official-mix".to_string(),
                ..BackendSettings::default()
            })
            .unwrap();

        let updated = store
            .update(json!({
                "relayProfiles": [{
                    "id": "official-mix",
                    "name": "官方混入",
                    "relayMode": "official",
                    "officialMixApiKey": true,
                    "configContents": "model_provider = \"custom\"\n\n[model_providers.other]\nbase_url = \"https://other.example/v1\"\nexperimental_bearer_token = \"sk-other\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nexperimental_bearer_token = \"\"\n",
                    "authContents": ""
                }],
                "activeRelayId": "official-mix"
            }))
            .unwrap();

        let profile = &updated.relay_profiles[0];
        assert_eq!(profile.api_key, "sk-existing");
        assert!(!profile.config_contents.contains("sk-other"));
        assert!(profile.config_contents.contains(
            r#"[model_providers.custom]
base_url = "https://relay.example/v1"
experimental_bearer_token = "sk-existing""#
        ));
    }

    #[test]
    fn official_mix_update_uses_api_key_when_config_token_missing() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayProfiles": [{
                    "id": "official-mix",
                    "name": "官方混入",
                    "relayMode": "official",
                    "officialMixApiKey": true,
                    "baseUrl": "https://relay.example/v1",
                    "apiKey": "sk-new",
                    "configContents": "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\n",
                    "authContents": ""
                }],
                "activeRelayId": "official-mix"
            }))
            .unwrap();

        let profile = &updated.relay_profiles[0];
        assert_eq!(profile.api_key, "sk-new");
        assert!(
            profile
                .config_contents
                .contains(r#"experimental_bearer_token = "sk-new""#)
        );
        assert!(!profile.auth_contents.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn settings_update_preserves_manual_official_mix_config_token() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayProfiles": [{
                    "id": "official-mix",
                    "name": "官方混入",
                    "relayMode": "official",
                    "officialMixApiKey": true,
                    "configContents": "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example/v1\"\nexperimental_bearer_token = \"22222222222222222222222222222222222\"\n",
                    "authContents": ""
                }],
                "activeRelayId": "official-mix"
            }))
            .unwrap();

        let profile = &updated.relay_profiles[0];
        assert_eq!(profile.relay_mode, RelayMode::Official);
        assert!(profile.official_mix_api_key);
        assert_eq!(profile.api_key, "22222222222222222222222222222222222");
        assert!(
            profile
                .config_contents
                .contains(r#"experimental_bearer_token = "22222222222222222222222222222222222""#)
        );
        assert!(!profile.auth_contents.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn settings_store_load_missing_file_returns_default() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        assert_eq!(store.load().unwrap(), BackendSettings::default());
    }

    #[test]
    fn settings_store_load_bad_json_returns_default() {
        let dir = temp_dir();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{bad json").unwrap();
        let store = SettingsStore::new(path);

        assert_eq!(store.load().unwrap(), BackendSettings::default());
    }

    #[test]
    fn settings_store_save_load_roundtrip_uses_custom_path() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("nested").join("settings.json"));
        let settings = BackendSettings {
            provider_sync_enabled: true,
            cli_wrapper_enabled: true,
            cli_wrapper_base_url: "https://example.test".to_string(),
            cli_wrapper_api_key: "sk-test".to_string(),
            cli_wrapper_api_key_env: "CUSTOM_ENV".to_string(),
            codex_extra_args: vec!["--force_high_performance_gpu".to_string()],
            layered_compaction_enabled: true,
            layered_compaction_model_override_enabled: true,
            layered_compaction_models: LayeredCompactionModels {
                gpt: "deepseek-chat".to_string(),
                claude: "gpt-5.6".to_string(),
                other: "claude-sonnet-4-6".to_string(),
            },
            ..BackendSettings::default()
        };

        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), settings);
    }

    #[test]
    fn settings_store_rejects_conflicting_model_protocols_before_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = BackendSettings {
            relay_profiles: vec![RelayProfile {
                id: "conflict".to_string(),
                name: "冲突供应商".to_string(),
                model_mappings: vec![
                    RelayModelMapping {
                        request_model: "shared-model".to_string(),
                        alias: String::new(),
                        protocol: RelayProtocol::Responses,
                        context_window: String::new(),
                    },
                    RelayModelMapping {
                        request_model: "shared-model".to_string(),
                        alias: String::new(),
                        protocol: RelayProtocol::Anthropic,
                        context_window: String::new(),
                    },
                ],
                ..RelayProfile::default()
            }],
            active_relay_id: "conflict".to_string(),
            ..BackendSettings::default()
        };

        let error = store.save(&settings).unwrap_err();
        assert!(error.to_string().contains("模型协议配置无效"), "{error:#}");
        assert!(!path.exists(), "校验失败时不得写入设置文件");
    }

    #[test]
    fn settings_store_save_load_roundtrip_preserves_aggregate_relay_settings() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        let settings = BackendSettings {
            relay_profiles: vec![
                RelayProfile {
                    id: "relay-a".to_string(),
                    name: "中转 A".to_string(),
                    ..RelayProfile::default()
                },
                RelayProfile {
                    id: "relay-b".to_string(),
                    name: "中转 B".to_string(),
                    ..RelayProfile::default()
                },
                RelayProfile {
                    id: "agg".to_string(),
                    name: "聚合".to_string(),
                    relay_mode: RelayMode::Aggregate,
                    ..RelayProfile::default()
                },
            ],
            active_relay_id: "agg".to_string(),
            aggregate_relay_profiles: vec![AggregateRelayProfile {
                id: "agg".to_string(),
                name: "聚合".to_string(),
                strategy: AggregateRelayStrategy::WeightedRoundRobin,
                members: vec![
                    AggregateRelayMember {
                        relay_id: "relay-a".to_string(),
                        weight: 1,
                    },
                    AggregateRelayMember {
                        relay_id: "relay-b".to_string(),
                        weight: 3,
                    },
                ],
            }],
            active_aggregate_relay_id: "agg".to_string(),
            ..BackendSettings::default()
        };

        store.save(&settings).unwrap();

        let loaded = store.load().unwrap();
        let active_aggregate = loaded.active_aggregate_relay_profile().unwrap();
        assert_eq!(loaded.active_relay_id, "agg");
        assert_eq!(
            loaded.aggregate_relay_profiles,
            settings.aggregate_relay_profiles
        );
        assert_eq!(loaded.relay_profiles[0], settings.relay_profiles[0]);
        assert_eq!(loaded.relay_profiles[1], settings.relay_profiles[1]);
        assert_eq!(loaded.relay_profiles[2].relay_mode, RelayMode::Aggregate);
        assert_eq!(
            loaded.relay_profiles[2].base_url,
            crate::protocol_proxy::local_responses_proxy_base_url(
                crate::protocol_proxy::DEFAULT_PROTOCOL_PROXY_PORT
            )
        );
        assert_eq!(loaded.relay_profiles[2].api_key, "codex-elves-aggregate");
        assert_eq!(
            active_aggregate.strategy,
            AggregateRelayStrategy::WeightedRoundRobin
        );
        assert_eq!(active_aggregate.members[1].relay_id, "relay-b");
        assert_eq!(active_aggregate.members[1].weight, 3);
        assert!(loaded.active_relay_uses_protocol_proxy());
    }

    #[test]
    fn settings_store_update_only_mutates_present_known_fields() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));
        let initial = BackendSettings {
            provider_sync_enabled: false,
            cli_wrapper_enabled: true,
            cli_wrapper_base_url: "https://old.test".to_string(),
            cli_wrapper_api_key: "old-key".to_string(),
            cli_wrapper_api_key_env: "OLD_ENV".to_string(),
            ..BackendSettings::default()
        };
        store.save(&initial).unwrap();

        let updated = store
            .update(json!({
            "providerSyncEnabled": true,
            "githubReleaseUpdatePromptEnabled": false,
            "codexAppPath": "C:\\Portable\\Codex\\Codex.exe",
            "codexHomePath": " C:\\Portable\\CodexHome ",
            "enhancementsEnabled": false,
            "codexAppPluginEntryUnlock": false,
            "codexAppTaskBoard": false,
            "codexAppSessionDelete": false,
            "codexAppConversationView": true,
            "codexAppTokenUsage": true,
            "codexAppServiceTierControls": true,
            "codexGoalsEnabled": true,
            "relayBaseUrl": "https://relay.example.test/v1",
            "relayApiKey": "sk-relay",
            "codexExtraArgs": ["--force_high_performance_gpu", "", "  ", " --enable-gpu "],
            "cliWrapperApiKeyEnv": "",
            "unknownKey": "ignored"
            }))
            .unwrap();

        assert!(updated.provider_sync_enabled);
        assert!(!updated.github_release_update_prompt_enabled);
        assert_eq!(updated.codex_app_path, r"C:\Portable\Codex\Codex.exe");
        assert_eq!(updated.codex_home_path, r"C:\Portable\CodexHome");
        assert!(!updated.enhancements_enabled);
        assert!(!updated.codex_app_plugin_entry_unlock);
        assert!(!updated.codex_app_task_board);
        assert!(!updated.codex_app_session_delete);
        assert!(updated.codex_app_conversation_view);
        assert!(updated.codex_app_token_usage);
        assert!(updated.codex_app_service_tier_controls);
        assert!(updated.codex_goals_enabled);
        assert_eq!(updated.relay_base_url, "https://relay.example.test/v1");
        assert_eq!(updated.relay_api_key, "sk-relay");
        assert_eq!(
            updated.codex_extra_args,
            vec![
                "--force_high_performance_gpu".to_string(),
                "--enable-gpu".to_string(),
            ]
        );
        assert!(updated.cli_wrapper_enabled);
        assert_eq!(updated.cli_wrapper_base_url, "https://old.test");
        assert_eq!(updated.cli_wrapper_api_key, "old-key");
        assert_eq!(updated.cli_wrapper_api_key_env, "CUSTOM_OPENAI_API_KEY");
        assert_eq!(store.load().unwrap(), updated);
    }

    #[test]
    fn settings_store_update_persists_image_overlay_settings() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "codexAppImageOverlayEnabled": true,
                "codexAppImageOverlayPath": "C:\\Users\\me\\Pictures\\overlay.png",
                "codexAppImageOverlayOpacity": 42
            }))
            .unwrap();

        assert!(updated.codex_app_image_overlay_enabled);
        assert_eq!(
            updated.codex_app_image_overlay_path,
            r"C:\Users\me\Pictures\overlay.png"
        );
        assert_eq!(updated.codex_app_image_overlay_opacity, 42);
        assert_eq!(store.load().unwrap(), updated);
    }

    #[test]
    fn settings_store_update_persists_launch_mode() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store.update(json!({"launchMode": "relay"})).unwrap();
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();

        assert_eq!(updated.launch_mode, LaunchMode::Relay);
        assert_eq!(saved["launchMode"], json!("relay"));
    }

    #[test]
    fn settings_store_update_persists_relay_profiles_and_active_profile() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayProfiles": [
                    {
                        "id": "relay-a",
                        "name": "中转 A",
                        "baseUrl": "https://relay-a.example/v1",
                        "apiKey": "sk-a"
                    },
                    {
                        "id": "relay-b",
                        "name": "中转 B",
                        "baseUrl": "https://relay-b.example/v1",
                        "apiKey": "sk-b"
                    }
                ],
                "activeRelayId": "relay-b",
                "relayTestModel": "claude-sonnet-4"
            }))
            .unwrap();

        let active = updated.active_relay_profile();
        assert_eq!(updated.relay_profiles.len(), 2);
        assert_eq!(active.id, "relay-b");
        assert_eq!(active.name, "中转 B");
        assert_eq!(updated.relay_test_model, "claude-sonnet-4");

        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(saved["relayProfiles"][1].get("baseUrl").is_none());
        assert!(saved["relayProfiles"][1].get("apiKey").is_none());
    }

    #[test]
    fn settings_store_update_does_not_persist_relay_profile_derived_fields() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayProfiles": [
                    {
                        "id": "relay-a",
                        "name": "供应商 A",
                        "relayMode": "pureApi",
                        "model": "gpt-5.4",
                        "baseUrl": "https://relay.example/v1",
                        "apiKey": "sk-a",
                        "configContents": "model = \"gpt-5.4\"\n",
                        "authContents": "{\"OPENAI_API_KEY\":\"sk-a\"}"
                    }
                ],
                "activeRelayId": "relay-a"
            }))
            .unwrap();

        assert_eq!(updated.relay_profiles[0].id, "relay-a");
        assert_eq!(updated.relay_profiles[0].name, "供应商 A");

        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        let saved_profile = &saved["relayProfiles"][0];
        assert!(saved_profile.get("model").is_none());
        assert!(saved_profile.get("baseUrl").is_none());
        assert!(saved_profile.get("apiKey").is_none());
        // pureApi 模式下，configContents 会被补全为完整可用配置（新增 provider 必需字段），
        // 但用户手写的关键信息（model）不应丢失。
        let saved_config_contents = saved_profile["configContents"].as_str().unwrap_or_default();
        assert!(saved_config_contents.contains("model = \"gpt-5.4\""));
        assert_eq!(
            saved_profile["authContents"],
            "{\"OPENAI_API_KEY\":\"sk-a\"}"
        );
    }

    #[test]
    fn settings_store_update_moves_context_tables_out_of_common_config() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayCommonConfigContents": "[mcp_servers.context7]\ncommand = \"npx\"\n"
            }))
            .unwrap();

        assert!(updated.relay_common_config_contents.is_empty());
        assert_eq!(
            updated.relay_context_config_contents,
            "[mcp_servers.context7]\ncommand = \"npx\"\n"
        );
        assert_eq!(store.load().unwrap(), updated);
    }

    #[test]
    fn settings_store_update_extracts_context_config_from_common_config() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayCommonConfigContents": "model_reasoning_effort = \"high\"\n\n[mcp_servers.context7]\ncommand = \"npx\"\n\n[plugins.\"superpowers@openai-curated\"]\nenabled = true\n"
            }))
            .unwrap();

        assert_eq!(
            updated.relay_common_config_contents,
            "model_reasoning_effort = \"high\"\n"
        );
        assert!(
            updated
                .relay_context_config_contents
                .contains("[mcp_servers.context7]")
        );
        assert!(
            updated
                .relay_context_config_contents
                .contains("[plugins.\"superpowers@openai-curated\"]")
        );
        assert_eq!(store.load().unwrap(), updated);
    }

    #[test]
    fn settings_store_update_persists_aggregate_relay_profiles_and_active_id() {
        let dir = temp_dir();
        let store = SettingsStore::new(dir.join("settings.json"));

        let updated = store
            .update(json!({
                "relayProfiles": [
                    { "id": "relay-a", "name": "中转 A" },
                    { "id": "relay-b", "name": "中转 B" },
                    { "id": "agg", "name": "聚合", "relayMode": "aggregate" }
                ],
                "activeRelayId": "agg",
                "aggregateRelayProfiles": [
                    {
                        "id": "agg",
                        "name": "聚合",
                        "strategy": "weightedRoundRobin",
                        "members": [
                            { "relayId": "relay-a", "weight": 1 },
                            { "relayId": "relay-b", "weight": 4 }
                        ]
                    }
                ],
                "activeAggregateRelayId": "agg"
            }))
            .unwrap();

        let active_aggregate = updated.active_aggregate_relay_profile().unwrap();
        assert_eq!(updated.active_relay_id, "agg");
        assert_eq!(updated.active_aggregate_relay_id, "agg");
        assert_eq!(
            active_aggregate.strategy,
            AggregateRelayStrategy::WeightedRoundRobin
        );
        assert_eq!(active_aggregate.members.len(), 2);
        assert_eq!(active_aggregate.members[1].relay_id, "relay-b");
        assert_eq!(active_aggregate.members[1].weight, 4);
        assert!(updated.active_relay_uses_protocol_proxy());
    }

    #[test]
    fn active_relay_uses_protocol_proxy_requires_relay_profiles_enabled() {
        let local_proxy_settings = BackendSettings {
            relay_profiles_enabled: false,
            relay_profiles: vec![RelayProfile {
                id: "relay-chat".to_string(),
                name: "Chat".to_string(),
                local_proxy_enabled: Some(true),
                ..RelayProfile::default()
            }],
            active_relay_id: "relay-chat".to_string(),
            ..BackendSettings::default()
        };

        assert!(!local_proxy_settings.active_relay_uses_protocol_proxy());

        let aggregate_settings = BackendSettings {
            relay_profiles_enabled: false,
            relay_profiles: vec![RelayProfile {
                id: "agg".to_string(),
                name: "聚合".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            }],
            active_relay_id: "agg".to_string(),
            aggregate_relay_profiles: vec![AggregateRelayProfile {
                id: "agg".to_string(),
                name: "聚合".to_string(),
                strategy: AggregateRelayStrategy::WeightedRoundRobin,
                members: Vec::new(),
            }],
            active_aggregate_relay_id: "agg".to_string(),
            ..BackendSettings::default()
        };

        assert!(
            aggregate_settings
                .active_aggregate_relay_profile()
                .is_some()
        );
        assert!(!aggregate_settings.active_relay_uses_protocol_proxy());
    }

    #[test]
    fn active_relay_profile_uses_legacy_single_relay_when_profiles_are_default() {
        let settings = BackendSettings {
            relay_base_url: "https://legacy.example/v1".to_string(),
            relay_api_key: "sk-legacy".to_string(),
            ..BackendSettings::default()
        };

        let active = settings.active_relay_profile();

        assert_eq!(active.id, "default");
        assert_eq!(active.name, "默认中转");
        assert_eq!(active.base_url, "https://legacy.example/v1");
        assert_eq!(active.api_key, "sk-legacy");
        assert_eq!(active.relay_mode, RelayMode::MixedApi);
        assert!(active.official_mix_api_key);
    }

    #[test]
    fn settings_store_update_preserves_existing_unknown_fields() {
        let dir = temp_dir();
        let path = dir.join("settings.json");
        let store = SettingsStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{"providerSyncEnabled":false,"customField":{"nested":true}}"#,
        )
        .unwrap();

        let updated = store
            .update(json!({
                "providerSyncEnabled": true
            }))
            .unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert!(updated.provider_sync_enabled);
        assert_eq!(saved["providerSyncEnabled"], json!(true));
        assert_eq!(saved["codexExtraArgs"], Value::Null);
        assert_eq!(saved["customField"], json!({"nested": true}));
    }

    #[test]
    fn settings_store_update_persists_codex_extra_args_and_preserves_unknown_fields() {
        let dir = temp_dir();
        let path = dir.join("settings.json");
        let store = SettingsStore::new(path.clone());
        std::fs::write(
            &path,
            r#"{"providerSyncEnabled":false,"customField":{"nested":true}}"#,
        )
        .unwrap();

        let updated = store
            .update(json!({
                "codexExtraArgs": ["--force_high_performance_gpu", "--enable-features=UseOzonePlatform"]
            }))
            .unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(
            updated.codex_extra_args,
            vec![
                "--force_high_performance_gpu".to_string(),
                "--enable-features=UseOzonePlatform".to_string(),
            ]
        );
        assert_eq!(
            saved["codexExtraArgs"],
            json!([
                "--force_high_performance_gpu",
                "--enable-features=UseOzonePlatform"
            ])
        );
        assert_eq!(saved["customField"], json!({"nested": true}));
    }

    #[test]
    fn settings_store_update_with_non_object_payload_does_not_write_file() {
        let dir = temp_dir();
        let path = dir.join("settings.json");
        let store = SettingsStore::new(path.clone());
        let original = r#"{"providerSyncEnabled":false,"customField":"keep me"}"#;
        std::fs::write(&path, original).unwrap();

        let updated = store.update(json!(null)).unwrap();

        assert!(!updated.provider_sync_enabled);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
