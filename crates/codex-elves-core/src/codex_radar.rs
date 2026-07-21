use anyhow::Context;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const CODEX_RADAR_HTML_URL: &str = "https://codexradar.com/";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarSnapshot {
    pub schema_version: Option<String>,
    pub monitored_at: Option<String>,
    pub timezone: Option<String>,
    pub links: Option<CodexRadarLinks>,
    pub model_iq: CodexRadarModelIq,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarLinks {
    pub html: Option<String>,
    pub rss: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarModelIq {
    pub latest: Option<CodexRadarIqRun>,
    pub recent_days: Vec<CodexRadarIqRun>,
    pub comparisons: std::collections::BTreeMap<String, CodexRadarIqComparison>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRadarIqComparison {
    pub label: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub latest: Option<CodexRadarIqRun>,
    pub recent_days: Vec<CodexRadarIqRun>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
        let key = if raw_key.is_empty() {
            run.model
                .clone()
                .unwrap_or_else(|| "primary".to_string())
                .replace([' ', '.', '-'], "_")
                .to_ascii_lowercase()
        } else {
            raw_key
        };
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
            && let Some(key) = extract_attr(block, "data-model-key")
        {
            let tooltip_key = extract_attr(block, "data-model-iq-tooltip-key");
            if tooltip_key
                .as_deref()
                .is_some_and(|value| !value.starts_with("iq|"))
            {
                rest = &rest[end + "</circle>".len()..];
                continue;
            }
            let description =
                extract_tag_text(block, "title").or_else(|| extract_attr(block, "aria-label"));
            if let Some(mut run) = description.and_then(|value| parse_run_title(&value)) {
                if let Some(date) = tooltip_key.as_deref().and_then(parse_tooltip_date) {
                    run.date = date;
                }
                apply_key_to_run(&key, &mut run);
                runs.push((key, run));
            }
        }
        rest = &rest[end + "</circle>".len()..];
    }
    runs
}

fn parse_tooltip_date(value: &str) -> Option<String> {
    let mut parts = value.split('|');
    if parts.next()? != "iq" {
        return None;
    }
    parts
        .next()
        .filter(|date| !date.is_empty())
        .map(str::to_string)
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
    if let Some(model) = model_from_key(key) {
        run.model = Some(model);
    }
    if let Some(reasoning_effort) = reasoning_effort_from_key(key) {
        run.reasoning_effort = Some(reasoning_effort);
    }
}

fn model_from_key(key: &str) -> Option<String> {
    let (model_key, _effort) = key.rsplit_once('_')?;
    if model_key.is_empty() {
        return None;
    }
    if let Some(version_and_variant) = model_key.strip_prefix("gpt_") {
        let (version, variant) = version_and_variant
            .split_once('_')
            .unwrap_or((version_and_variant, ""));
        if version.len() >= 2 && version.chars().all(|ch| ch.is_ascii_digit()) {
            let (major, minor) = version.split_at(1);
            let mut model = format!("gpt-{major}.{minor}");
            if !variant.is_empty() {
                model.push('-');
                model.push_str(&variant.replace('_', "-"));
            }
            return Some(model);
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
    fn snapshot_serializes_recent_days_for_manager_camel_case_payload() {
        let snapshot = CodexRadarSnapshot {
            schema_version: Some("html-scrape".to_string()),
            monitored_at: Some("6月24日12:52更新".to_string()),
            timezone: Some("Asia/Shanghai".to_string()),
            links: None,
            model_iq: CodexRadarModelIq {
                latest: None,
                recent_days: vec![CodexRadarIqRun {
                    date: "2026-06-24-am".to_string(),
                    score: 87.5,
                    status: "yellow".to_string(),
                    passed: 7,
                    tasks: 12,
                    invalid: 0,
                    total_tokens: 0,
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    output_tokens: 0,
                    wall_seconds: 0,
                    wall_time_human: String::new(),
                    model: Some("gpt-5.5".to_string()),
                    reasoning_effort: Some("xhigh".to_string()),
                    valid_tasks: Some(12),
                    cost_usd: None,
                }],
                comparisons: BTreeMap::new(),
            },
        };

        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["modelIq"]["recentDays"].as_array().unwrap().len(), 1);
        assert!(value["modelIq"].get("recent_days").is_none());
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
    fn snapshot_scrapes_current_aria_label_and_tooltip_key_markup() {
        let snapshot = parse_snapshot_from_html(
            r#"
            <section class="model-iq">
              <h2>降智雷达 <span>7月13日09:16更新</span></h2>
              <div class="model-iq-score-chip model-iq-score-chip-primary" data-model-key="gpt_56_sol_max">
                <span>Sol max</span>
                <div class="model-iq-score-metrics"><strong>150.0</strong></div>
              </div>
              <svg>
                <circle
                  class="model-iq-series-dot"
                  data-model-key="gpt_56_sol_max"
                  data-model-iq-tooltip-key="iq|2026-07-13-am|150.000000"
                  aria-label="7.13_am Sol max: IQ指数 150.0, 10/10, 费用 $34.94, 耗时 166分钟, cache命中率 95.6%"
                ></circle>
                <circle
                  class="model-iq-series-dot"
                  data-model-key="gpt_56_sol_max"
                  data-model-iq-tooltip-key="cost|2026-07-13-am|34.940000"
                  aria-label="7.13_am Sol max: IQ指数 150.0, 10/10, 费用 $34.94, 耗时 166分钟, cache命中率 95.6%"
                ></circle>
              </svg>
            </section>
            "#,
        )
        .unwrap();

        let latest = snapshot.model_iq.latest.unwrap();
        assert_eq!(snapshot.monitored_at.as_deref(), Some("7月13日09:16更新"));
        assert_eq!(latest.date, "2026-07-13-am");
        assert_eq!(latest.score, 150.0);
        assert_eq!(latest.passed, 10);
        assert_eq!(latest.tasks, 10);
        assert_eq!(latest.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(latest.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(snapshot.model_iq.recent_days.len(), 1);
        assert_eq!(snapshot.model_iq.comparisons.len(), 1);
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
    fn model_and_effort_use_data_model_key_consistently() {
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
            model: Some("Sol".to_string()),
            reasoning_effort: Some("medium".to_string()),
            valid_tasks: Some(12),
            cost_usd: None,
        };

        apply_key_to_run("gpt_56_sol_max", &mut run);

        assert_eq!(run.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(run.reasoning_effort.as_deref(), Some("max"));
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
