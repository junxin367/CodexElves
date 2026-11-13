//! 编辑框底行只读原生用量；与发送、审批及正文投影互不等待。
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde_json::{Value, json};

const SAFE_INTEGER: u64 = 9_007_199_254_740_991;
// 用户要求统计不能拖慢会话：只定位当前会话文件，首次最多尾读 8 MiB，后续只读追加；不扫描子代理或整个归档。
const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
const CACHE_CAPACITY: usize = 64;

fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|value| *value <= SAFE_INTEGER)
}

fn timestamp(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()?
            .timestamp_millis()
            .try_into()
            .ok()
    })
}

fn summary(
    source: &str,
    turn_id: &str,
    total: Option<u64>,
    usage: Option<(u64, u64, u64, u64)>,
    duration: Option<u64>,
) -> Value {
    let speed = usage
        .zip(duration)
        .and_then(|((_, output, _, _), ms)| (ms > 0).then_some(output as f64 * 1000.0 / ms as f64));
    json!({"source":source,"turnId":turn_id,"totalTokens":total,
        "inputTokens":usage.map(|usage| usage.0),"outputTokens":usage.map(|usage| usage.1),
        "turnTokens":usage.map(|usage| usage.2),"steps":usage.map(|usage| usage.3),
        "durationMs":duration,"tokensPerSecond":speed})
}

/// 同一 assistant 的多段内容共用请求身份；重复快照和流式中间值只接替，不累计成多步。
pub(super) fn observe_claude(turn: &mut Value, frame: &Value) {
    if frame["type"] != "assistant"
        || frame["isApiErrorMessage"] == true
        || frame["message"]["model"] == "<synthetic>"
    {
        return;
    }
    let message = &frame["message"];
    let Some(id) = message["id"].as_str().or_else(|| frame["uuid"].as_str()) else {
        return;
    };
    let raw = &message["usage"];
    let parts: Vec<_> = raw["iterations"]
        .as_array()
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.iter().collect())
        .unwrap_or_else(|| vec![raw]);
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut steps = 0_u64;
    for part in parts {
        if part["type"].as_str().is_some_and(|kind| {
            !matches!(
                kind,
                "message" | "fallback_message" | "compaction" | "advisor_message"
            )
        }) {
            continue;
        }
        let (Some(tokens), Some(generated)) =
            (count(&part["input_tokens"]), count(&part["output_tokens"]))
        else {
            return;
        };
        let cached = part.get("cache_read_input_tokens").map_or(Some(0), count);
        let created = part
            .get("cache_creation_input_tokens")
            .map_or(Some(0), count);
        let (Some(cached), Some(created)) = (cached, created) else {
            return;
        };
        input = input
            .saturating_add(tokens)
            .saturating_add(cached)
            .saturating_add(created);
        output = output.saturating_add(generated);
        steps += 1;
    }
    if steps == 0 || input.saturating_add(output) > SAFE_INTEGER {
        return;
    }
    let current = &turn["_metricsRequests"][id];
    input = input.max(count(&current["inputTokens"]).unwrap_or(0));
    output = output.max(count(&current["outputTokens"]).unwrap_or(0));
    steps = steps.max(count(&current["steps"]).unwrap_or(0));
    if input.saturating_add(output) > SAFE_INTEGER {
        return;
    }
    turn["_metricsRequests"][id] = json!({"inputTokens":input,"outputTokens":output,
        "steps":steps,"observedAtMs":frame["timestampMs"]});
}

pub(super) fn claude(turns: &[Value]) -> Value {
    let latest = turns.iter().rev().find(|turn| {
        turn["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["type"] == "userMessage"))
    });
    let mut total = 0_u64;
    let mut seen = HashSet::new();
    let mut any_usage = false;
    for turn in turns {
        for (id, usage) in turn["_metricsRequests"].as_object().into_iter().flatten() {
            if seen.insert(id)
                && let (Some(input), Some(output)) =
                    (count(&usage["inputTokens"]), count(&usage["outputTokens"]))
            {
                total = total.saturating_add(input).saturating_add(output);
                any_usage = true;
            }
        }
    }
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut steps = 0_u64;
    let mut observed = None;
    for usage in latest
        .into_iter()
        .flat_map(|turn| turn["_metricsRequests"].as_object())
        .flat_map(|requests| requests.values())
    {
        input = input.saturating_add(count(&usage["inputTokens"]).unwrap_or(0));
        output = output.saturating_add(count(&usage["outputTokens"]).unwrap_or(0));
        steps = steps.saturating_add(count(&usage["steps"]).unwrap_or(0));
        observed = observed.max(timestamp(&usage["observedAtMs"]));
    }
    let duration = latest.and_then(|turn| {
        count(&turn["durationMs"]).or_else(|| {
            let start =
                timestamp(&turn["turnStartedAtMs"]).or_else(|| timestamp(&turn["startedAt"]))?;
            let end = timestamp(&turn["turnCompletedAtMs"])
                .or_else(|| timestamp(&turn["completedAt"]))
                .or(observed)?;
            end.checked_sub(start)
        })
    });
    // 用户确认“步”为模型调用次数，“总”为当前原生会话累计；不把工具数量或子代理汇总混入。
    summary(
        "claude-native",
        latest.and_then(|turn| turn["id"].as_str()).unwrap_or(""),
        (any_usage && total <= SAFE_INTEGER).then_some(total),
        (steps > 0).then_some((input, output, input.saturating_add(output), steps)),
        duration,
    )
}

#[derive(Clone, Default)]
struct CodexMetrics {
    turn_id: String,
    started: Option<u64>,
    ended: Option<u64>,
    observed: Option<u64>,
    total: Option<u64>,
    previous_total: Option<u64>,
    input: u64,
    output: u64,
    tokens: u64,
    steps: u64,
}

impl CodexMetrics {
    fn consume(&mut self, line: &[u8]) {
        // 正文和图片不参与统计，不解析它们的大 JSON。
        if ![
            b"\"token_count\"".as_slice(),
            b"\"task_started\"",
            b"\"task_complete\"",
            b"\"task_completed\"",
            b"\"task_aborted\"",
            b"\"turn_aborted\"",
        ]
        .iter()
        .any(|needle| line.windows(needle.len()).any(|part| part == *needle))
        {
            return;
        }
        let Ok(row) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if row["type"] != "event_msg" {
            return;
        }
        let payload = &row["payload"];
        let at = timestamp(&row["timestamp"]);
        match payload["type"].as_str() {
            Some("task_started") => {
                let Some(id) = payload["turn_id"].as_str().filter(|id| !id.is_empty()) else {
                    return;
                };
                if self.turn_id != id {
                    self.turn_id = id.to_owned();
                    self.started = at;
                    self.ended = None;
                    self.observed = None;
                    self.input = 0;
                    self.output = 0;
                    self.tokens = 0;
                    self.steps = 0;
                }
            }
            Some("task_complete" | "task_completed" | "task_aborted" | "turn_aborted") => {
                if payload["turn_id"]
                    .as_str()
                    .is_none_or(|id| id == self.turn_id)
                {
                    self.ended = at;
                }
            }
            Some("token_count") => {
                let info = &payload["info"];
                let cumulative = count(&info["total_token_usage"]["total_tokens"]);
                if let Some(cumulative) = cumulative {
                    self.total = Some(cumulative);
                    if self.previous_total == Some(cumulative) {
                        return;
                    }
                    self.previous_total = Some(cumulative);
                } else {
                    return;
                }
                let last = &info["last_token_usage"];
                if !self.turn_id.is_empty()
                    && let (Some(input), Some(output), Some(tokens)) = (
                        count(&last["input_tokens"]),
                        count(&last["output_tokens"]),
                        count(&last["total_tokens"]),
                    )
                    && tokens > 0
                {
                    self.input = self.input.saturating_add(input);
                    self.output = self.output.saturating_add(output);
                    self.tokens = self.tokens.saturating_add(tokens);
                    self.steps += 1;
                    self.observed = at;
                }
            }
            _ => {}
        }
    }

    fn value(&self) -> Value {
        let duration = self
            .started
            .zip(self.ended.or(self.observed))
            .and_then(|(start, end)| end.checked_sub(start));
        summary(
            "codex-rollout",
            &self.turn_id,
            self.total,
            (self.steps > 0 && self.tokens <= SAFE_INTEGER).then_some((
                self.input,
                self.output,
                self.tokens,
                self.steps,
            )),
            duration,
        )
    }
}

#[derive(Clone)]
struct Cached {
    len: u64,
    modified: Option<SystemTime>,
    offset: u64,
    tail: Vec<u8>,
    parser: CodexMetrics,
}

pub(super) fn codex(home: &Path, id: &str) -> Value {
    let Some(path) = super::context_usage::rollout_path(home, id) else {
        return Value::Null;
    };
    read_codex(&path).unwrap_or(Value::Null)
}

fn read_codex(path: &Path) -> std::io::Result<Value> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let (len, modified) = (metadata.len(), metadata.modified().ok());
    let cached = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(path)
        .cloned();
    if let Some(entry) = &cached
        && entry.len == len
        && entry.modified == modified
    {
        return Ok(entry.parser.value());
    }
    let mut parser = CodexMetrics::default();
    let mut offset = len.saturating_sub(MAX_READ_BYTES);
    if let Some(entry) = cached
        && len > entry.len
        && len.saturating_sub(entry.offset) <= MAX_READ_BYTES
    {
        file.seek(SeekFrom::Start(
            entry.offset.saturating_sub(entry.tail.len() as u64),
        ))?;
        let mut tail = vec![0; entry.tail.len()];
        file.read_exact(&mut tail)?;
        if tail == entry.tail {
            offset = entry.offset;
            parser = entry.parser;
        }
    }
    let skip_first = if offset > 0 {
        file.seek(SeekFrom::Start(offset - 1))?;
        let mut previous = [0];
        file.read_exact(&mut previous)?;
        previous[0] != b'\n'
    } else {
        false
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES).read_to_end(&mut bytes)?;
    let start = if skip_first {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    } else {
        0
    };
    let end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(start, |index| index + 1);
    for line in bytes[start..end].split(|byte| *byte == b'\n') {
        parser.consume(line);
    }
    let parsed = offset + end as u64;
    let tail = bytes[end.saturating_sub(128)..end].to_vec();
    let result = parser.value();
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= CACHE_CAPACITY && !cache.contains_key(path) {
        cache.clear();
    }
    cache.insert(
        path.to_owned(),
        Cached {
            len,
            modified,
            offset: parsed,
            tail,
            parser,
        },
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn request(id: &str, input: u64, output: u64) -> Value {
        json!({"type":"assistant","timestampMs":2000,
            "message":{"id":id,"model":"native-model","usage":{
                "input_tokens":input,"output_tokens":output,"cache_read_input_tokens":20,
                "cache_creation_input_tokens":30}}})
    }

    fn turn(id: &str) -> Value {
        json!({"id":id,"turnStartedAtMs":1000,"turnCompletedAtMs":3000,
            "items":[{"type":"userMessage"}]})
    }

    fn event(kind: &str, at: &str, fields: Value) -> Value {
        let mut payload = fields;
        payload["type"] = json!(kind);
        json!({"type":"event_msg","timestamp":at,"payload":payload})
    }

    fn usage(total: u64, last: u64) -> Value {
        event(
            "token_count",
            "2026-11-13T00:00:02Z",
            json!({"info":{"total_token_usage":{"total_tokens":total},
                "last_token_usage":{"total_tokens":last,"input_tokens":last-20,"output_tokens":20}}}),
        )
    }

    #[test]
    fn session_metrics_claude_counts_model_calls_once_per_native_request() {
        let mut current = turn("first");
        let row = request("same-request", 100, 40);
        observe_claude(&mut current, &row);
        observe_claude(&mut current, &row);
        observe_claude(&mut current, &request("same-request", 100, 10));
        let value = claude(&[current]);
        assert_eq!(value["steps"], 1);
        assert_eq!(value["turnTokens"], 190);
        assert_eq!(value["tokensPerSecond"], 20.0);
    }

    #[test]
    fn session_metrics_claude_iterations_replace_top_level_and_preserve_cache_tokens() {
        let mut current = turn("iterations");
        let mut row = request("request", 9000, 9000);
        row["message"]["usage"]["iterations"] = json!([
            {"type":"message","input_tokens":100,"output_tokens":20,"cache_read_input_tokens":50},
            {"type":"compaction","input_tokens":10,"output_tokens":5},
            {"type":"advisor_message","input_tokens":9999,"output_tokens":9999}
        ]);
        observe_claude(&mut current, &row);
        let value = claude(&[current]);
        assert_eq!(value["steps"], 3);
        assert_eq!(value["turnTokens"], 20183);
        assert_eq!(value["totalTokens"], 20183);
    }

    #[test]
    fn session_metrics_claude_new_turn_does_not_relabel_previous_turn_usage() {
        let mut previous = turn("previous");
        observe_claude(&mut previous, &request("one", 100, 40));
        let next = turn("next");
        let value = claude(&[previous, next]);
        assert_eq!(value["totalTokens"], 190);
        assert_eq!(value["turnId"], "next");
        assert!(value["turnTokens"].is_null());
        assert!(value["steps"].is_null());
        assert!(value["tokensPerSecond"].is_null());
    }

    #[test]
    fn session_metrics_claude_split_message_keeps_input_and_final_output_without_counting_twice() {
        let mut current = turn("split");
        observe_claude(&mut current, &request("same", 100, 0));
        observe_claude(&mut current, &request("same", 0, 40));
        let value = claude(&[current]);
        assert_eq!(value["steps"], 1);
        assert_eq!(value["inputTokens"], 150);
        assert_eq!(value["outputTokens"], 40);
        assert_eq!(value["turnTokens"], 190);
    }

    #[test]
    fn session_metrics_codex_repeated_usage_and_rate_updates_do_not_increase_steps() {
        let mut parser = CodexMetrics::default();
        for row in [
            event(
                "task_started",
                "2026-11-13T00:00:00Z",
                json!({"turn_id":"turn"}),
            ),
            usage(1200, 200),
            usage(1200, 200),
            event(
                "token_count",
                "2026-11-13T00:00:03Z",
                json!({"rate_limits":{}}),
            ),
            usage(1500, 300),
            event(
                "task_complete",
                "2026-11-13T00:00:04Z",
                json!({"turn_id":"turn"}),
            ),
        ] {
            parser.consume(&serde_json::to_vec(&row).unwrap());
        }
        let value = parser.value();
        assert_eq!(value["turnTokens"], 500);
        assert_eq!(value["totalTokens"], 1500);
        assert_eq!(value["steps"], 2);
        assert_eq!(value["tokensPerSecond"], 10.0);
    }

    #[test]
    fn session_metrics_codex_new_turn_skips_previous_request_snapshot() {
        let mut parser = CodexMetrics::default();
        for row in [
            event(
                "task_started",
                "2026-11-13T00:00:00Z",
                json!({"turn_id":"previous"}),
            ),
            usage(1200, 200),
            event(
                "task_started",
                "2026-11-13T00:00:03Z",
                json!({"turn_id":"next"}),
            ),
            usage(1200, 200),
        ] {
            parser.consume(&serde_json::to_vec(&row).unwrap());
        }
        assert_eq!(parser.value()["totalTokens"], 1200);
        assert!(parser.value()["turnTokens"].is_null());
        assert!(parser.value()["steps"].is_null());
    }

    #[test]
    fn session_metrics_codex_cache_only_accepts_complete_appended_lines_and_detects_rewrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let started = event(
            "task_started",
            "2026-11-13T00:00:00Z",
            json!({"turn_id":"turn"}),
        );
        std::fs::write(&path, format!("{started}\n{}\n", usage(1200, 200))).unwrap();
        assert_eq!(read_codex(&path).unwrap()["steps"], 1);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        let line = serde_json::to_string(&usage(1500, 300)).unwrap();
        file.write_all(line.as_bytes()).unwrap();
        assert_eq!(read_codex(&path).unwrap()["steps"], 1);
        file.write_all(b"\n").unwrap();
        assert_eq!(read_codex(&path).unwrap()["steps"], 2);
        drop(file);
        std::fs::write(&path, format!("{started}\n{}\n", usage(900, 100))).unwrap();
        let rewritten = read_codex(&path).unwrap();
        assert_eq!(rewritten["totalTokens"], 900);
        assert_eq!(rewritten["steps"], 1);
    }
}
