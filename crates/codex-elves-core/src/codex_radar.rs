use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CODEX_RADAR_CURRENT_URL: &str = "https://codexradar.com/current.json";
pub const CODEX_RADAR_HTML_URL: &str = "https://codexradar.com/";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarSnapshot {
    #[serde(alias = "schema_version")]
    pub schema_version: Option<String>,
    #[serde(alias = "monitored_at")]
    pub monitored_at: Option<String>,
    pub timezone: Option<String>,
    pub links: Option<CodexRadarLinks>,
    #[serde(alias = "model_iq", default)]
    pub model_iq: CodexRadarModelIq,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarLinks {
    pub html: Option<String>,
    pub rss: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarModelIq {
    #[serde(default)]
    pub latest: Option<CodexRadarIqRun>,
    #[serde(alias = "recent_days", default)]
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
    #[serde(alias = "recent_days", default)]
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
    let client = crate::http_client::proxied_client(BROWSER_USER_AGENT)?;
    let html = client
        .get(CODEX_RADAR_HTML_URL)
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(reqwest::header::ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await
        .context("failed to request Codex Radar page")?
        .error_for_status()
        .context("Codex Radar page returned an error status")?
        .text()
        .await
        .context("failed to read Codex Radar page")?;
    parse_snapshot_from_html(&html).context("failed to scrape Codex Radar page")
}

fn parse_snapshot_from_html(html: &str) -> anyhow::Result<CodexRadarSnapshot> {
    let mut comparisons: BTreeMap<String, CodexRadarIqComparison> = BTreeMap::new();
    let card_scores = parse_score_cards(html);
    let card_scores_by_key = card_scores
        .iter()
        .map(|card| (card.key.clone(), (card.label.clone(), card.score)))
        .collect::<BTreeMap<_, _>>();
    let mut first_chart_key = None;
    for (raw_key, run) in parse_chart_runs(html) {
        let key = run
            .reasoning_effort
            .as_deref()
            .and_then(|effort| run.model.as_deref().map(|model| model_key(model, effort)))
            .unwrap_or_else(|| {
                if raw_key.is_empty() {
                    run.model
                        .clone()
                        .unwrap_or_else(|| "primary".to_string())
                        .replace([' ', '.', '-'], "_")
                        .to_ascii_lowercase()
                } else {
                    raw_key.clone()
                }
            });
        if first_chart_key.is_none() {
            first_chart_key = Some(key.clone());
        }
        let label = card_scores_by_key
            .get(&key)
            .map(|item| item.0.clone())
            .or_else(|| run.model.clone())
            .unwrap_or_else(|| key.clone());
        let entry = comparisons
            .entry(key)
            .or_insert_with(|| CodexRadarIqComparison {
                label,
                model: run.model.clone(),
                reasoning_effort: run.reasoning_effort.clone(),
                latest: None,
                recent_days: Vec::new(),
            });
        entry.recent_days.push(run.clone());
        entry.latest = Some(run);
    }

    for card in &card_scores {
        let entry = comparisons
            .entry(card.key.clone())
            .or_insert_with(|| CodexRadarIqComparison {
                label: card.label.clone(),
                model: model_from_label(&card.label),
                reasoning_effort: reasoning_effort_from_label(&card.label),
                latest: None,
                recent_days: Vec::new(),
            });
        if entry.latest.is_none() {
            entry.model = entry.model.clone().or_else(|| model_from_key(&card.key));
            entry.reasoning_effort = entry
                .reasoning_effort
                .clone()
                .or_else(|| reasoning_effort_from_key(&card.key));
        }
    }

    for comparison in comparisons.values_mut() {
        comparison.recent_days = normalize_recent_runs(std::mem::take(&mut comparison.recent_days));
        comparison.latest = comparison
            .recent_days
            .last()
            .cloned()
            .or_else(|| comparison.latest.take());
    }

    if comparisons.is_empty() {
        anyhow::bail!("Codex Radar page did not contain model IQ data");
    }

    let primary_key = first_chart_key
        .filter(|key| comparisons.contains_key(key))
        .or_else(|| {
            card_scores
                .first()
                .map(|card| card.key.clone())
                .filter(|key| comparisons.contains_key(key))
        })
        .or_else(|| comparisons.keys().next().cloned());
    let latest = primary_key
        .as_ref()
        .and_then(|key| comparisons.get(key))
        .and_then(|item| item.latest.clone());
    let recent_days = primary_key
        .as_ref()
        .and_then(|key| comparisons.get(key))
        .map(|item| item.recent_days.clone())
        .unwrap_or_default();

    Ok(CodexRadarSnapshot {
        schema_version: Some("html-scrape".to_string()),
        monitored_at: parse_update_label(html),
        timezone: Some("Asia/Shanghai".to_string()),
        links: Some(CodexRadarLinks {
            html: Some(CODEX_RADAR_HTML_URL.to_string()),
            rss: Some("https://codexradar.com/feed.xml".to_string()),
        }),
        model_iq: CodexRadarModelIq {
            latest,
            recent_days,
            comparisons,
        },
    })
}

#[derive(Debug, Clone)]
struct CodexRadarScoreCard {
    key: String,
    label: String,
    score: f64,
}

fn parse_score_cards(html: &str) -> Vec<CodexRadarScoreCard> {
    let mut scores = Vec::new();
    let mut rest = html;
    while let Some(index) = rest.find("<div") {
        rest = &rest[index..];
        let Some(end) = rest.find("</div>") else {
            break;
        };
        let block = &rest[..end];
        if block.contains("model-iq-score-chip") {
            if let (Some(key), Some(label), Some(score)) = (
                extract_attr(block, "data-model-key"),
                extract_tag_text(block, "span"),
                extract_tag_text(block, "strong").and_then(|value| value.parse::<f64>().ok()),
            ) {
                scores.push(CodexRadarScoreCard { key, label, score });
            }
        }
        rest = &rest[end + "</div>".len()..];
    }
    scores
}

fn parse_chart_runs(html: &str) -> Vec<(String, CodexRadarIqRun)> {
    let mut runs = Vec::new();
    let mut rest = html;
    while let Some(index) = rest.find("<circle") {
        rest = &rest[index..];
        let Some(end) = rest.find("</circle>") else {
            break;
        };
        let block = &rest[..end];
        if block.contains("model-iq-series-dot")
            && let (Some(key), Some(title)) = (
                extract_attr(block, "data-model-key"),
                extract_tag_text(block, "title"),
            )
            && let Some(mut run) = parse_run_title(&title)
        {
            if run.model.is_none() || run.reasoning_effort.is_none() {
                apply_key_to_run(&key, &mut run);
            }
            runs.push((key, run));
        }
        rest = &rest[end + "</circle>".len()..];
    }
    runs
}

fn normalize_recent_runs(runs: Vec<CodexRadarIqRun>) -> Vec<CodexRadarIqRun> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for run in runs.into_iter().rev() {
        let key = format!(
            "{}|{}|{}",
            run.date,
            run.model.as_deref().unwrap_or_default(),
            run.reasoning_effort.as_deref().unwrap_or_default()
        );
        if seen.insert(key) {
            normalized.push(run);
        }
        if normalized.len() >= 10 {
            break;
        }
    }
    normalized.reverse();
    normalized
}

fn parse_run_title(title: &str) -> Option<CodexRadarIqRun> {
    let (head, tail) = title.split_once(':')?;
    let (date, label) = head.trim().split_once(' ')?;
    let mut parts = tail.split(',').map(str::trim);
    let score = parts.find_map(|part| part.split_whitespace().last()?.parse::<f64>().ok())?;
    let pass_part = title
        .split(',')
        .map(str::trim)
        .find(|part| part.contains('/'))?;
    let (passed, tasks) = pass_part.split_once('/')?;
    let passed = passed.trim().parse::<u32>().ok()?;
    let tasks = tasks.trim().parse::<u32>().ok()?;
    let cost_usd = title
        .split("费用 $")
        .nth(1)
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.trim().parse::<f64>().ok());
    let wall_time_human = title
        .split("耗时 ")
        .nth(1)
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let wall_seconds = wall_time_human
        .strip_suffix("分钟")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|minutes| minutes * 60)
        .unwrap_or_default();

    Some(CodexRadarIqRun {
        date: date.replace('.', "-"),
        score,
        status: status_for_score(score).to_string(),
        passed,
        tasks,
        invalid: 0,
        total_tokens: 0,
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        wall_seconds,
        wall_time_human,
        model: model_from_label(label),
        reasoning_effort: reasoning_effort_from_label(label),
        valid_tasks: Some(tasks),
        cost_usd,
    })
}

fn extract_attr(block: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}=\"");
    let start = block.find(&pattern)? + pattern.len();
    let end = block[start..].find('"')? + start;
    Some(html_unescape(&block[start..end]))
}

fn extract_tag_text(block: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}");
    let start = block.find(&start_tag)?;
    let after_start = block[start..].find('>')? + start + 1;
    let end_tag = format!("</{tag}>");
    let end = block[after_start..].find(&end_tag)? + after_start;
    Some(html_unescape(block[after_start..end].trim()))
}

fn parse_update_label(html: &str) -> Option<String> {
    let index = html.find("降智雷达 <span>")?;
    let rest = &html[index + "降智雷达 <span>".len()..];
    let end = rest.find("</span>")?;
    Some(html_unescape(rest[..end].trim()))
}

fn apply_key_to_run(key: &str, run: &mut CodexRadarIqRun) {
    if run.model.is_none() {
        run.model = model_from_key(key);
    }
    if run.reasoning_effort.is_none() {
        run.reasoning_effort = reasoning_effort_from_key(key);
    }
}

fn model_from_key(key: &str) -> Option<String> {
    let mut parts = key.rsplitn(2, '_');
    let _effort = parts.next()?;
    let model_key = parts.next().unwrap_or_default();
    if model_key.is_empty() {
        return None;
    }
    if let Some(version) = model_key.strip_prefix("gpt_") {
        if version.len() >= 2 && version.chars().all(|ch| ch.is_ascii_digit()) {
            let (major, minor) = version.split_at(1);
            return Some(format!("gpt-{major}.{minor}"));
        }
    }
    Some(model_key.replace('_', "-"))
}

fn reasoning_effort_from_key(key: &str) -> Option<String> {
    key.rsplit_once('_')
        .map(|(_, effort)| effort)
        .filter(|effort| !effort.is_empty())
        .map(str::to_string)
}

fn model_from_label(label: &str) -> Option<String> {
    let label = label.trim().replace('-', " ");
    let mut parts: Vec<&str> = label.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    parts.pop();
    Some(parts.join("-"))
}

fn reasoning_effort_from_label(label: &str) -> Option<String> {
    label
        .trim()
        .split([' ', '-'])
        .next_back()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn model_key(model: &str, reasoning_effort: &str) -> String {
    let mut normalized = model.trim().to_ascii_lowercase();
    if let Some(rest) = normalized.strip_prefix("gpt-") {
        normalized = format!("gpt_{}", rest.replace('.', ""));
    } else {
        normalized = normalized.replace(['.', '-', ' '], "_");
    }
    format!("{}_{}", normalized, reasoning_effort.to_ascii_lowercase())
}

fn status_for_score(score: f64) -> &'static str {
    if score >= 100.0 {
        "green"
    } else if score >= 80.0 {
        "yellow"
    } else {
        "red"
    }
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_deserializes_snake_case_recent_days_from_current_json() {
        let snapshot: CodexRadarSnapshot = serde_json::from_str(
            r#"{
              "schema_version": "2.0",
              "monitored_at": "2026-06-24T04:52:00.084111+08:00",
              "model_iq": {
                "latest": {
                  "date": "2026-06-24-am",
                  "score": 87.5,
                  "status": "yellow",
                  "passed": 7,
                  "tasks": 12
                },
                "recent_days": [
                  {
                    "date": "2026-06-23",
                    "score": 125.0,
                    "status": "green",
                    "passed": 10,
                    "tasks": 12
                  }
                ],
                "comparisons": {
                  "gpt_55_high": {
                    "label": "GPT-5.5 high",
                    "model": "gpt-5.5",
                    "reasoning_effort": "high",
                    "latest": {
                      "date": "2026-06-24-am",
                      "score": 100.0,
                      "status": "green",
                      "passed": 8,
                      "tasks": 12
                    },
                    "recent_days": [
                      {
                        "date": "2026-06-23",
                        "score": 87.5,
                        "status": "yellow",
                        "passed": 7,
                        "tasks": 12
                      }
                    ]
                  }
                }
              }
            }"#,
        )
        .unwrap();

        assert_eq!(snapshot.model_iq.recent_days.len(), 1);
        assert_eq!(snapshot.model_iq.recent_days[0].date, "2026-06-23");
        let comparison = snapshot.model_iq.comparisons.get("gpt_55_high").unwrap();
        assert_eq!(comparison.recent_days.len(), 1);
        assert_eq!(comparison.recent_days[0].score, 87.5);
    }

    #[test]
    fn snapshot_serializes_recent_days_for_manager_camel_case_payload() {
        let snapshot: CodexRadarSnapshot = serde_json::from_str(
            r#"{
              "model_iq": {
                "latest": null,
                "recent_days": [
                  {
                    "date": "2026-06-24-am",
                    "score": 87.5,
                    "status": "yellow",
                    "passed": 7,
                    "tasks": 12
                  }
                ],
                "comparisons": {}
              }
            }"#,
        )
        .unwrap();

        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["modelIq"]["recentDays"].as_array().unwrap().len(), 1);
        assert!(value["modelIq"].get("recent_days").is_none());
    }

    #[test]
    fn snapshot_deserializes_public_summary_without_model_iq() {
        let snapshot: CodexRadarSnapshot = serde_json::from_str(
            r#"{
              "schema_version": "2.0",
              "service": "codex-reset-radar",
              "type": "public_summary",
              "monitored_at": "2026-06-30T09:17:16.847471+08:00",
              "timezone": "Asia/Shanghai",
              "links": {
                "html": "https://codexradar.com/",
                "rss": "https://codexradar.com/feed.xml",
                "full_api": "https://codexradar.com/api/v1/current"
              },
              "api_access": {
                "status": "public_summary"
              }
            }"#,
        )
        .unwrap();

        assert_eq!(snapshot.schema_version.as_deref(), Some("2.0"));
        assert!(snapshot.model_iq.latest.is_none());
        assert!(snapshot.model_iq.recent_days.is_empty());
        assert!(snapshot.model_iq.comparisons.is_empty());
    }

    #[test]
    fn snapshot_scrapes_model_iq_from_resilient_html_markers() {
        let snapshot = parse_snapshot_from_html(
            r#"
            <section class="model-iq">
              <h2>降智雷达 <span>6月29日13:59更新</span></h2>
              <div class="model-iq-score-chip some-extra-class" data-model-key="gpt_55_xhigh">
                <span>GPT-5.5-xhigh</span>
                <strong>75.0</strong>
              </div>
              <svg>
                <circle class="model-iq-series-dot changed-order" data-extra="1" data-model-key="gpt_55_xhigh">
                  <title>6.29_pm GPT-5.5 xhigh: IQ指数 75.0, 6/12, 费用 $31.05, 耗时 109分钟, cache命中率 93.8%</title>
                </circle>
              </svg>
            </section>
            "#,
        )
        .unwrap();

        let latest = snapshot.model_iq.latest.unwrap();
        assert_eq!(snapshot.monitored_at.as_deref(), Some("6月29日13:59更新"));
        assert_eq!(latest.date, "6-29_pm");
        assert_eq!(latest.passed, 6);
        assert_eq!(latest.tasks, 12);
        assert_eq!(latest.wall_seconds, 6540);
        assert_eq!(latest.cost_usd, Some(31.05));
        assert!(snapshot.model_iq.comparisons.contains_key("gpt_55_xhigh"));
    }

    #[test]
    fn snapshot_uses_first_score_card_as_primary_without_chart_runs() {
        let snapshot = parse_snapshot_from_html(
            r#"
            <section class="model-iq">
              <div class="model-iq-score-chip" data-model-key="gpt_54_high">
                <span>GPT-5.4-high</span>
                <strong>88.0</strong>
              </div>
              <div class="model-iq-score-chip" data-model-key="gpt_55_xhigh">
                <span>GPT-5.5-xhigh</span>
                <strong>75.0</strong>
              </div>
            </section>
            "#,
        )
        .unwrap();

        assert!(snapshot.model_iq.latest.is_none());
        assert!(snapshot.model_iq.recent_days.is_empty());
        assert!(snapshot.model_iq.comparisons.contains_key("gpt_54_high"));
        assert!(snapshot.model_iq.comparisons.contains_key("gpt_55_xhigh"));
    }

    #[test]
    fn snapshot_limits_recent_report_to_latest_ten_unique_runs() {
        let mut html = String::from(
            r#"
            <section class="model-iq">
              <div class="model-iq-score-chip" data-model-key="gpt_55_xhigh">
                <span>GPT-5.5-xhigh</span>
                <strong>75.0</strong>
              </div>
            "#,
        );
        for day in 1..=12 {
            html.push_str(&format!(
                r#"
                <circle class="model-iq-series-dot" data-model-key="gpt_55_xhigh">
                  <title>6.{day:02}_pm GPT-5.5 xhigh: IQ指数 75.0, 6/12, 费用 $31.05, 耗时 109分钟</title>
                </circle>
                "#
            ));
        }
        html.push_str(
            r#"
              <circle class="model-iq-series-dot" data-model-key="gpt_55_xhigh">
                <title>6.12_pm GPT-5.5 xhigh: IQ指数 75.0, 6/12, 费用 $31.05, 耗时 109分钟</title>
              </circle>
            </section>
            "#,
        );

        let snapshot = parse_snapshot_from_html(&html).unwrap();

        assert_eq!(snapshot.model_iq.recent_days.len(), 10);
        assert_eq!(snapshot.model_iq.recent_days[0].date, "6-03_pm");
        assert_eq!(snapshot.model_iq.recent_days[9].date, "6-12_pm");
        assert_eq!(snapshot.model_iq.latest.unwrap().date, "6-12_pm");
        let comparison = snapshot.model_iq.comparisons.get("gpt_55_xhigh").unwrap();
        assert_eq!(comparison.recent_days.len(), 10);
    }

    #[test]
    fn model_and_effort_fall_back_to_data_model_key_consistently() {
        let mut run = CodexRadarIqRun {
            date: "6-29_pm".to_string(),
            score: 75.0,
            status: "red".to_string(),
            passed: 6,
            tasks: 12,
            invalid: 0,
            total_tokens: 0,
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            wall_seconds: 0,
            wall_time_human: String::new(),
            model: None,
            reasoning_effort: None,
            valid_tasks: Some(12),
            cost_usd: None,
        };

        apply_key_to_run("gpt_55_xhigh", &mut run);

        assert_eq!(run.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(run.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn run_title_parser_accepts_non_localized_score_label() {
        let run = parse_run_title("6.29_pm GPT-5.5 xhigh: IQ score 75.0, 6/12").unwrap();

        assert_eq!(run.date, "6-29_pm");
        assert_eq!(run.score, 75.0);
        assert_eq!(run.passed, 6);
        assert_eq!(run.tasks, 12);
        assert_eq!(run.model.as_deref(), Some("GPT-5.5"));
        assert_eq!(run.reasoning_effort.as_deref(), Some("xhigh"));
    }
}
