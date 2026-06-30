use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CAPTURED_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub const STARTUP_RETAINED_RECORDS: usize = 10;
pub const RUNTIME_RETAINED_RECORDS: usize = 200;
const LARGE_LOG_RECORD_SAFETY_BYTES: usize = 1024;
const MAX_RETAINED_REQUEST_BODY_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestRecord {
    pub id: String,
    pub timestamp_ms: u64,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub reasoning_source: Option<String>,
    pub service_tier: Option<String>,
    pub relay_id: Option<String>,
    pub relay_name: Option<String>,
    pub endpoint: Option<String>,
    pub response_protocol: Option<String>,
    pub status_code: u16,
    pub duration_ms: u64,
    pub stream: bool,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub response_captured_bytes: usize,
    pub response_truncated: bool,
    pub request_body: String,
    pub response_body: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestSummary {
    pub id: String,
    pub timestamp_ms: u64,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub reasoning_source: Option<String>,
    pub service_tier: Option<String>,
    pub relay_id: Option<String>,
    pub relay_name: Option<String>,
    pub endpoint: Option<String>,
    pub response_protocol: Option<String>,
    pub status_code: u16,
    pub duration_ms: u64,
    pub stream: bool,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub response_captured_bytes: usize,
    pub response_truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RequestMetadata {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub reasoning_source: Option<String>,
    pub service_tier: Option<String>,
}

impl From<&ProxyRequestRecord> for ProxyRequestSummary {
    fn from(record: &ProxyRequestRecord) -> Self {
        Self {
            id: record.id.clone(),
            timestamp_ms: record.timestamp_ms,
            method: record.method.clone(),
            path: record.path.clone(),
            remote_addr: record.remote_addr.clone(),
            model: record.model.clone(),
            reasoning_tokens: record.reasoning_tokens,
            reasoning_effort: record.reasoning_effort.clone(),
            reasoning_source: record.reasoning_source.clone(),
            service_tier: record.service_tier.clone(),
            relay_id: record.relay_id.clone(),
            relay_name: record.relay_name.clone(),
            endpoint: record.endpoint.clone(),
            response_protocol: record.response_protocol.clone(),
            status_code: record.status_code,
            duration_ms: record.duration_ms,
            stream: record.stream,
            request_bytes: record.request_bytes,
            response_bytes: record.response_bytes,
            response_captured_bytes: record.response_captured_bytes,
            response_truncated: record.response_truncated,
            error: record.error.clone(),
        }
    }
}

pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn extract_request_metadata(request_json: Option<&Value>) -> RequestMetadata {
    let model = request_json
        .and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);
    let service_tier = request_json
        .and_then(|value| value.get("service_tier"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);
    let reasoning = request_json.and_then(|value| value.get("reasoning"));
    let reasoning_effort = reasoning
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .or_else(|| {
            request_json
                .and_then(|value| value.get("reasoning_effort"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            request_json
                .and_then(|value| value.pointer("/output_config/effort"))
                .and_then(Value::as_str)
        })
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);
    let reasoning_source = if reasoning
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .is_some()
    {
        Some("reasoning.effort".to_string())
    } else if request_json
        .and_then(|value| value.get("reasoning_effort"))
        .and_then(Value::as_str)
        .is_some()
    {
        Some("reasoning_effort".to_string())
    } else if request_json
        .and_then(|value| value.pointer("/output_config/effort"))
        .and_then(Value::as_str)
        .is_some()
    {
        Some("output_config.effort".to_string())
    } else {
        None
    };

    RequestMetadata {
        model,
        reasoning_effort,
        reasoning_source,
        service_tier,
    }
}

pub fn append_capture(buffer: &mut Vec<u8>, bytes: &[u8]) -> bool {
    if buffer.len() >= MAX_CAPTURED_RESPONSE_BYTES {
        return !bytes.is_empty();
    }
    let remaining = MAX_CAPTURED_RESPONSE_BYTES - buffer.len();
    let take = remaining.min(bytes.len());
    buffer.extend_from_slice(&bytes[..take]);
    take < bytes.len()
}

pub fn extract_reasoning_tokens_from_response_body(body: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(body);
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        return find_reasoning_tokens(&value);
    }

    text.lines()
        .filter_map(|line| line.trim().strip_prefix("data:"))
        .filter_map(|data| {
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                return None;
            }
            serde_json::from_str::<Value>(data)
                .ok()
                .and_then(|value| find_reasoning_tokens(&value))
        })
        .max()
}

pub fn append_record(record: &ProxyRequestRecord) -> std::io::Result<()> {
    let path = default_log_path();
    append_record_at_path(&path, record)
}

fn append_record_at_path(path: &PathBuf, record: &ProxyRequestRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock_exclusive()?;
    let line = serialize_record_for_log(record)?;
    file.seek(SeekFrom::End(0))?;
    writeln!(file, "{line}")?;
    retain_recent_records_in_file(&mut file, RUNTIME_RETAINED_RECORDS)?;
    file.unlock()?;
    Ok(())
}

pub fn read_summaries(limit: usize) -> std::io::Result<Vec<ProxyRequestSummary>> {
    let records = read_records(limit)?;
    Ok(records.iter().map(ProxyRequestSummary::from).collect())
}

pub fn find_record(id: &str) -> std::io::Result<Option<ProxyRequestRecord>> {
    let path = default_log_path();
    if !path.is_file() {
        return Ok(None);
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut found = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<ProxyRequestRecord>(&line) {
            if record.id == id {
                found = Some(record);
            }
        }
    }
    Ok(found)
}

pub fn clear_records() -> std::io::Result<()> {
    let path = default_log_path();
    clear_records_at_path(&path)
}

fn clear_records_at_path(path: &PathBuf) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock_exclusive()?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.unlock()?;
    Ok(())
}

pub fn retain_recent_records(limit: usize) -> std::io::Result<()> {
    let path = default_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    retain_recent_records_at_path(&path, limit)
}

pub fn default_log_path() -> PathBuf {
    crate::paths::default_proxy_log_path()
}

fn read_records(limit: usize) -> std::io::Result<Vec<ProxyRequestRecord>> {
    let path = default_log_path();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut records = Vec::new();
    for line in text.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<ProxyRequestRecord>(line) {
            records.push(record);
        }
        if records.len() >= limit {
            break;
        }
    }
    Ok(records)
}

fn retain_recent_records_at_path(path: &PathBuf, limit: usize) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    retain_recent_records_in_file(&mut file, limit)?;
    file.unlock()?;
    Ok(())
}

fn retain_recent_records_in_file(file: &mut fs::File, limit: usize) -> std::io::Result<()> {
    if limit == 0 {
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        return Ok(());
    }

    file.flush()?;
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() <= limit {
        file.seek(SeekFrom::End(0))?;
        return Ok(());
    }

    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    for line in &lines[lines.len() - limit..] {
        writeln!(file, "{line}")?;
    }
    file.flush()?;
    Ok(())
}

fn serialize_record_for_log(record: &ProxyRequestRecord) -> serde_json::Result<String> {
    let line = serde_json::to_string(record)?;
    if line.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES {
        return Ok(line);
    }

    let mut trimmed = record.clone();
    trimmed.request_body.clear();
    trimmed.response_body.clear();
    trimmed.response_truncated = true;
    trimmed.response_captured_bytes = 0;

    let empty_line = serde_json::to_string(&trimmed)?;
    let max_bytes = crate::log_limits::MAX_LOG_FILE_BYTES as usize;
    if empty_line.len() + 1 >= max_bytes {
        return Ok(empty_line);
    }

    let available_body_bytes = max_bytes
        .saturating_sub(empty_line.len() + 1)
        .saturating_sub(LARGE_LOG_RECORD_SAFETY_BYTES);
    let mut request_budget = available_body_bytes
        .min(MAX_RETAINED_REQUEST_BODY_BYTES)
        .min(record.request_body.len());
    let mut response_budget = available_body_bytes
        .saturating_sub(request_budget)
        .min(record.response_body.len());

    loop {
        trimmed.request_body = truncate_to_utf8_byte_limit(&record.request_body, request_budget);
        trimmed.response_body = truncate_to_utf8_byte_limit(&record.response_body, response_budget);
        trimmed.response_truncated = true;
        trimmed.response_captured_bytes = trimmed.response_body.len();

        let line = serde_json::to_string(&trimmed)?;
        if line.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES {
            return Ok(line);
        }

        if response_budget > 0 {
            response_budget /= 2;
        } else if request_budget > 0 {
            request_budget /= 2;
        } else {
            return Ok(line);
        }
    }
}

fn truncate_to_utf8_byte_limit(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn find_reasoning_tokens(value: &Value) -> Option<u64> {
    match value {
        Value::Object(map) => {
            if let Some(tokens) = map.get("reasoning_tokens").and_then(value_to_u64) {
                return Some(tokens);
            }
            if let Some(tokens) = map.get("thinking_tokens").and_then(value_to_u64) {
                return Some(tokens);
            }
            for child in map.values() {
                if let Some(tokens) = find_reasoning_tokens(child) {
                    return Some(tokens);
                }
            }
            None
        }
        Value::Array(items) => items.iter().filter_map(find_reasoning_tokens).max(),
        _ => None,
    }
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

#[cfg(test)]
mod tests {
    use super::{
        ProxyRequestRecord, append_record, append_record_at_path, clear_records_at_path,
        current_timestamp_ms, extract_reasoning_tokens_from_response_body,
        extract_request_metadata, find_record, read_summaries, retain_recent_records,
        serialize_record_for_log,
    };

    fn temp_proxy_log_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "codex-elves-{name}-{}-{}.jsonl",
            std::process::id(),
            super::current_timestamp_ms()
        ))
    }

    #[test]
    fn extracts_reasoning_tokens_from_responses_usage() {
        let body = br#"{
            "id": "resp_1",
            "usage": {
                "output_tokens": 760,
                "output_tokens_details": {
                    "reasoning_tokens": 516
                }
            }
        }"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(516));
    }

    #[test]
    fn extracts_anthropic_output_config_effort_from_request_metadata() {
        let request = serde_json::json!({
            "model": "claude-opus-4-8",
            "thinking": { "type": "adaptive" },
            "output_config": { "effort": "high" }
        });

        let metadata = extract_request_metadata(Some(&request));

        assert_eq!(metadata.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(
            metadata.reasoning_source.as_deref(),
            Some("output_config.effort")
        );
    }

    #[test]
    fn extracts_reasoning_tokens_from_chat_completions_usage() {
        let body = br#"{
            "id": "chatcmpl_1",
            "usage": {
                "completion_tokens": 760,
                "completion_tokens_details": {
                    "reasoning_tokens": 516
                }
            }
        }"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(516));
    }

    #[test]
    fn extracts_reasoning_tokens_from_sse_data_lines() {
        let body = br#"event: response.completed
data: {"type":"response.completed","response":{"usage":{"output_tokens_details":{"reasoning_tokens":516}}}}

data: [DONE]
"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(516));
    }

    #[test]
    fn extracts_thinking_tokens_from_anthropic_usage() {
        let body = br#"{
            "id": "msg_1",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 760,
                "output_tokens_details": {
                    "thinking_tokens": 516
                }
            }
        }"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(516));
    }

    #[test]
    fn append_record_writes_locked_jsonl_file() {
        let path = temp_proxy_log_path("append-record");
        let previous = crate::paths::set_proxy_log_path_for_tests(Some(path.clone()));
        let record = ProxyRequestRecord {
            id: "test-record".to_string(),
            timestamp_ms: current_timestamp_ms(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            remote_addr: Some("127.0.0.1:1".to_string()),
            model: Some("gpt-5.4".to_string()),
            reasoning_tokens: Some(516),
            reasoning_effort: Some("medium".to_string()),
            reasoning_source: Some("reasoning.effort".to_string()),
            service_tier: Some("auto".to_string()),
            relay_id: Some("relay-test".to_string()),
            relay_name: Some("Test".to_string()),
            endpoint: Some("https://example.test/v1/responses".to_string()),
            response_protocol: Some("responses".to_string()),
            status_code: 200,
            duration_ms: 10,
            stream: false,
            request_bytes: 2,
            response_bytes: 2,
            response_captured_bytes: 2,
            response_truncated: false,
            request_body: "{}".to_string(),
            response_body: "{}".to_string(),
            error: None,
        };

        append_record(&record).expect("append proxy log record");
        let found = find_record("test-record")
            .expect("read proxy log record")
            .expect("record should exist");

        assert_eq!(found.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(found.reasoning_tokens, Some(516));

        for index in 0..12 {
            let mut next = record.clone();
            next.id = format!("test-record-{index}");
            next.timestamp_ms += index as u64 + 1;
            append_record(&next).expect("append proxy log record");
        }
        let summaries = read_summaries(20).expect("read proxy log summaries");
        assert_eq!(summaries.len(), 13);
        assert_eq!(
            summaries.first().map(|entry| entry.id.as_str()),
            Some("test-record-11")
        );
        assert_eq!(
            summaries.last().map(|entry| entry.id.as_str()),
            Some("test-record")
        );

        retain_recent_records(super::STARTUP_RETAINED_RECORDS).expect("retain recent proxy logs");
        let summaries = read_summaries(20).expect("read retained proxy log summaries");
        assert_eq!(summaries.len(), super::STARTUP_RETAINED_RECORDS);
        assert_eq!(
            summaries.first().map(|entry| entry.id.as_str()),
            Some("test-record-11")
        );
        assert_eq!(
            summaries.last().map(|entry| entry.id.as_str()),
            Some("test-record-2")
        );

        let _ = std::fs::remove_file(path);
        crate::paths::set_proxy_log_path_for_tests(previous);
    }

    #[test]
    fn append_record_does_not_clear_existing_log_by_file_size_under_runtime_record_limit() {
        let path = temp_proxy_log_path("keep-under-record-limit");
        let mut seed = "x".repeat(crate::log_limits::MAX_LOG_FILE_BYTES as usize - 2);
        seed.push('\n');
        std::fs::write(&path, seed).expect("seed proxy log");
        let record = sample_proxy_record("fresh-record");

        append_record_at_path(&path, &record).expect("append proxy log record");

        let metadata = std::fs::metadata(&path).expect("read proxy log metadata");
        assert!(metadata.len() > crate::log_limits::MAX_LOG_FILE_BYTES);
        let text = std::fs::read_to_string(&path).expect("read proxy log");
        assert!(text.starts_with('x'));
        assert!(text.contains("fresh-record"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn append_record_retains_recent_runtime_record_limit() {
        let path = temp_proxy_log_path("runtime-retain-records");
        let record = sample_proxy_record("record-0");

        for index in 0..=super::RUNTIME_RETAINED_RECORDS {
            let mut next = record.clone();
            next.id = format!("record-{index}");
            append_record_at_path(&path, &next).expect("append proxy log record");
        }

        let text = std::fs::read_to_string(&path).expect("read proxy log");
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), super::RUNTIME_RETAINED_RECORDS);
        assert!(!text.contains("\"id\":\"record-0\""));
        assert!(text.contains("\"id\":\"record-1\""));
        assert!(text.contains(&format!(
            "\"id\":\"record-{}\"",
            super::RUNTIME_RETAINED_RECORDS
        )));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn large_proxy_record_is_trimmed_to_fit_single_log_file_limit() {
        let mut record = sample_proxy_record("large-record");
        record.request_body = "q".repeat(128 * 1024);
        record.response_body = "r".repeat(crate::log_limits::MAX_LOG_FILE_BYTES as usize + 1);
        record.response_captured_bytes = record.response_body.len();
        record.response_truncated = false;

        let line = serialize_record_for_log(&record).expect("serialize trimmed proxy log record");
        let parsed: ProxyRequestRecord =
            serde_json::from_str(&line).expect("parse trimmed proxy log record");

        assert!(line.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES);
        assert_eq!(parsed.id, "large-record");
        assert!(parsed.response_truncated);
        assert!(parsed.response_body.len() < record.response_body.len());
        assert!(parsed.request_body.len() <= super::MAX_RETAINED_REQUEST_BODY_BYTES);
    }

    #[test]
    fn clear_records_uses_locked_file_truncation() {
        let path = temp_proxy_log_path("clear-records");
        std::fs::write(&path, "old\n").expect("seed proxy log");

        clear_records_at_path(&path).expect("clear proxy logs");

        let text = std::fs::read_to_string(&path).expect("read cleared proxy log");
        assert!(text.is_empty());

        let _ = std::fs::remove_file(path);
    }

    fn sample_proxy_record(id: &str) -> ProxyRequestRecord {
        ProxyRequestRecord {
            id: id.to_string(),
            timestamp_ms: current_timestamp_ms(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            remote_addr: Some("127.0.0.1:1".to_string()),
            model: Some("glm-5.2".to_string()),
            reasoning_tokens: None,
            reasoning_effort: None,
            reasoning_source: None,
            service_tier: None,
            relay_id: Some("relay-test".to_string()),
            relay_name: Some("Test".to_string()),
            endpoint: Some("https://example.test/v1/chat/completions".to_string()),
            response_protocol: Some("chatCompletions".to_string()),
            status_code: 200,
            duration_ms: 10,
            stream: false,
            request_bytes: 2,
            response_bytes: 2,
            response_captured_bytes: 2,
            response_truncated: false,
            request_body: "{}".to_string(),
            response_body: "{}".to_string(),
            error: None,
        }
    }
}
