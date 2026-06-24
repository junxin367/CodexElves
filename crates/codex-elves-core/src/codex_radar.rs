use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const CODEX_RADAR_CURRENT_URL: &str = "https://codexradar.com/current.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarSnapshot {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<String>,
    #[serde(alias = "monitored_at")]
    pub monitored_at: Option<String>,
    pub timezone: Option<String>,
    pub links: Option<CodexRadarLinks>,
    #[serde(alias = "model_iq")]
    pub model_iq: CodexRadarModelIq,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarLinks {
    pub html: Option<String>,
    pub rss: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarModelIq {
    pub latest: Option<CodexRadarIqRun>,
    #[serde(default)]
    pub recent_days: Vec<CodexRadarIqRun>,
    #[serde(default)]
    pub comparisons: std::collections::BTreeMap<String, CodexRadarIqComparison>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarIqComparison {
    pub label: String,
    pub model: Option<String>,
    #[serde(alias = "reasoning_effort")]
    pub reasoning_effort: Option<String>,
    pub latest: Option<CodexRadarIqRun>,
    #[serde(default)]
    pub recent_days: Vec<CodexRadarIqRun>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarIqRun {
    pub date: String,
    pub score: f64,
    pub status: String,
    pub passed: u32,
    pub tasks: u32,
    #[serde(default)]
    pub invalid: u32,
    #[serde(alias = "total_tokens", default)]
    pub total_tokens: u64,
    #[serde(alias = "input_tokens", default)]
    pub input_tokens: u64,
    #[serde(alias = "cached_input_tokens", default)]
    pub cached_input_tokens: u64,
    #[serde(alias = "output_tokens", default)]
    pub output_tokens: u64,
    #[serde(alias = "wall_seconds", default)]
    pub wall_seconds: u64,
    #[serde(alias = "wall_time_human", default)]
    pub wall_time_human: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(alias = "reasoning_effort", default)]
    pub reasoning_effort: Option<String>,
    #[serde(alias = "valid_tasks", default)]
    pub valid_tasks: Option<u32>,
    #[serde(alias = "cost_usd", default)]
    pub cost_usd: Option<f64>,
}

pub async fn fetch_current_snapshot() -> anyhow::Result<CodexRadarSnapshot> {
    let client = crate::http_client::proxied_client("CodexElves/CodexRadar")?;
    client
        .get(CODEX_RADAR_CURRENT_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("failed to request Codex Radar current snapshot")?
        .error_for_status()
        .context("Codex Radar current snapshot returned an error status")?
        .json::<CodexRadarSnapshot>()
        .await
        .context("failed to decode Codex Radar current snapshot")
}
