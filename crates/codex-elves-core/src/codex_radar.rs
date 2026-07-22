use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

pub const CODEX_RADAR_HTML_URL: &str = "https://codexradar.com/";
pub const CODEX_RADAR_CURRENT_URL: &str = "https://codexradar.com/current.json";

const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct CodexRadarSnapshot {
    pub schema_version: Option<String>,
    pub monitored_at: Option<String>,
    pub timezone: Option<String>,
    pub links: Option<CodexRadarLinks>,
    pub model_iq: CodexRadarModelIq,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct CodexRadarLinks {
    pub html: Option<String>,
    pub rss: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct CodexRadarModelIq {
    pub latest: Option<CodexRadarIqRun>,
    pub recent_days: Vec<CodexRadarIqRun>,
    pub comparisons: BTreeMap<String, CodexRadarIqComparison>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct CodexRadarIqComparison {
    pub label: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub latest: Option<CodexRadarIqRun>,
    pub recent_days: Vec<CodexRadarIqRun>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct CodexRadarIqRun {
    pub date: String,
    pub score: f64,
    pub status: String,
    pub passed: u32,
    pub tasks: u32,
    pub invalid: u32,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub wall_seconds: u64,
    pub wall_time_human: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub valid_tasks: Option<u32>,
    pub cost_usd: Option<f64>,
}

pub async fn fetch_current_snapshot() -> anyhow::Result<CodexRadarSnapshot> {
    let client =
        crate::http_client::proxied_client_with_timeout(BROWSER_USER_AGENT, FETCH_TIMEOUT)?;
    let body = client
        .get(CODEX_RADAR_CURRENT_URL)
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/plain;q=0.9, */*;q=0.8",
        )
        .header(reqwest::header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await
        .context("failed to request Codex Radar current snapshot")?
        .error_for_status()
        .context("Codex Radar current snapshot returned an error status")?
        .text()
        .await
        .context("failed to read Codex Radar current snapshot")?;

    parse_current_snapshot(&body).context("failed to parse Codex Radar current snapshot")
}

fn parse_current_snapshot(payload: &str) -> anyhow::Result<CodexRadarSnapshot> {
    let snapshot = serde_json::from_str::<CodexRadarSnapshot>(payload)
        .context("Codex Radar current snapshot was not valid JSON")?;
    let model_iq = &snapshot.model_iq;
    let has_model_iq = model_iq.latest.is_some()
        || !model_iq.recent_days.is_empty()
        || model_iq
            .comparisons
            .values()
            .any(|comparison| comparison.latest.is_some() || !comparison.recent_days.is_empty());
    if !has_model_iq {
        anyhow::bail!("Codex Radar current snapshot did not contain model IQ data");
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_deserializes_current_json_schema_and_serializes_for_manager() {
        let snapshot = parse_current_snapshot(
            r#"
            {
              "schema_version": "2.0",
              "monitored_at": "2026-07-22T06:12:58+08:00",
              "timezone": "Asia/Shanghai",
              "links": {
                "html": "https://codexradar.com/",
                "rss": "https://codexradar.com/feed.xml",
                "full_api": "https://codexradar.com/api/v1/current"
              },
              "model_iq": {
                "latest": {
                  "date": "2026-07-22T06:12:58+08:00",
                  "score": 103.6,
                  "status": "green",
                  "passed": 77,
                  "tasks": 112,
                  "invalid": 0,
                  "total_tokens": 1440495393,
                  "input_tokens": 1434292308,
                  "cached_input_tokens": 1403734016,
                  "output_tokens": 6203085,
                  "wall_seconds": 239036,
                  "wall_time_human": "66小时24分",
                  "model": "gpt-5.6-sol",
                  "reasoning_effort": "max",
                  "valid_tasks": 112,
                  "cost_usd": 1063.020276
                },
                "recent_days": [],
                "comparisons": {
                  "gpt_56_sol_xhigh": {
                    "label": "GPT-5.6 Sol xhigh",
                    "model": "gpt-5.6-sol",
                    "reasoning_effort": "xhigh",
                    "latest": null,
                    "recent_days": []
                  }
                }
              }
            }
            "#,
        )
        .unwrap();

        assert_eq!(snapshot.schema_version.as_deref(), Some("2.0"));
        assert_eq!(
            snapshot.model_iq.latest.as_ref().map(|run| run.score),
            Some(103.6)
        );
        assert!(
            snapshot
                .model_iq
                .comparisons
                .contains_key("gpt_56_sol_xhigh")
        );

        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["schemaVersion"], "2.0");
        assert_eq!(value["monitoredAt"], "2026-07-22T06:12:58+08:00");
        assert_eq!(value["modelIq"]["latest"]["reasoningEffort"], "max");
        assert!(value["model_iq"].is_null());
    }

    #[test]
    fn snapshot_rejects_payload_without_model_iq_data() {
        let error = parse_current_snapshot(
            r#"
            {
              "model_iq": {
                "latest": null,
                "recent_days": [],
                "comparisons": {}
              }
            }
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("did not contain model IQ data"));
    }
}
