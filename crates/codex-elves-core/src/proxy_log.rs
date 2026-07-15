use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_CAPTURED_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub const STARTUP_RETAINED_RECORDS: usize = 10;
pub const RUNTIME_RETAINED_RECORDS: usize = 500;
const PROXY_INDEX_HEADER: &str = r#"{"format":"codex-elves-proxy-index","version":1}"#;
const MAX_PROXY_INDEX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PROXY_INDEX_UPDATES: usize = RUNTIME_RETAINED_RECORDS * 3;
const LARGE_LOG_RECORD_SAFETY_BYTES: usize = 1024;
const MAX_RETAINED_REQUEST_BODY_BYTES: usize = 64 * 1024;
const MAX_SUMMARY_ERROR_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestRecord {
    pub id: String,
    #[serde(default = "default_proxy_request_state")]
    pub state: ProxyRequestState,
    #[serde(default = "default_proxy_request_transport")]
    pub transport: ProxyRequestTransport,
    pub timestamp_ms: u64,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub reasoning_source: Option<String>,
    #[serde(default)]
    pub continue_thinking_triggered: bool,
    #[serde(default)]
    pub continue_thinking_rounds: u32,
    #[serde(default)]
    pub continue_thinking_request_body: Option<String>,
    #[serde(default)]
    pub continue_thinking_before_response_body: Option<String>,
    #[serde(default)]
    pub continue_thinking_after_response_body: Option<String>,
    pub service_tier: Option<String>,
    pub relay_id: Option<String>,
    pub relay_name: Option<String>,
    pub endpoint: Option<String>,
    pub response_protocol: Option<String>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    pub stream: bool,
    pub request_bytes: usize,
    #[serde(default)]
    pub response_bytes: Option<usize>,
    #[serde(default)]
    pub response_captured_bytes: Option<usize>,
    #[serde(default)]
    pub response_truncated: bool,
    pub request_body: String,
    pub response_body: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequestSummary {
    pub id: String,
    #[serde(default = "default_proxy_request_state")]
    pub state: ProxyRequestState,
    #[serde(default = "default_proxy_request_transport")]
    pub transport: ProxyRequestTransport,
    pub timestamp_ms: u64,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    pub reasoning_effort: Option<String>,
    pub reasoning_source: Option<String>,
    #[serde(default)]
    pub continue_thinking_triggered: bool,
    #[serde(default)]
    pub continue_thinking_rounds: u32,
    pub service_tier: Option<String>,
    pub relay_id: Option<String>,
    pub relay_name: Option<String>,
    pub endpoint: Option<String>,
    pub response_protocol: Option<String>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    pub stream: bool,
    pub request_bytes: usize,
    #[serde(default)]
    pub response_bytes: Option<usize>,
    #[serde(default)]
    pub response_captured_bytes: Option<usize>,
    #[serde(default)]
    pub response_truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyRequestState {
    Pending,
    Completed,
}

fn default_proxy_request_state() -> ProxyRequestState {
    ProxyRequestState::Completed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyRequestTransport {
    Http,
    Ws,
}

fn default_proxy_request_transport() -> ProxyRequestTransport {
    ProxyRequestTransport::Http
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
            state: record.state,
            transport: record.transport,
            timestamp_ms: record.timestamp_ms,
            method: record.method.clone(),
            path: record.path.clone(),
            remote_addr: record.remote_addr.clone(),
            model: record.model.clone(),
            reasoning_tokens: record
                .reasoning_tokens
                .or_else(|| infer_reasoning_tokens_for_summary(record)),
            reasoning_effort: record.reasoning_effort.clone(),
            reasoning_source: record.reasoning_source.clone(),
            continue_thinking_triggered: record.continue_thinking_triggered,
            continue_thinking_rounds: record.continue_thinking_rounds,
            service_tier: record.service_tier.clone(),
            relay_id: record.relay_id.clone(),
            relay_name: record.relay_name.clone(),
            endpoint: record.endpoint.clone(),
            response_protocol: record.response_protocol.clone(),
            status_code: record.status_code,
            first_token_ms: record.first_token_ms,
            duration_ms: record.duration_ms,
            stream: record.stream,
            request_bytes: record.request_bytes,
            response_bytes: record.response_bytes,
            response_captured_bytes: record.response_captured_bytes,
            response_truncated: record.response_truncated,
            error: record
                .error
                .as_deref()
                .map(|error| truncate_to_utf8_byte_limit(error, MAX_SUMMARY_ERROR_BYTES)),
        }
    }
}

fn infer_reasoning_tokens_for_summary(record: &ProxyRequestRecord) -> Option<u64> {
    if record.response_body.is_empty()
        || !response_body_may_contain_reasoning(&record.response_body)
    {
        return None;
    }
    extract_reasoning_tokens_from_response_body(record.response_body.as_bytes())
}

fn response_body_may_contain_reasoning(response_body: &str) -> bool {
    response_body.contains("reasoning_tokens")
        || response_body.contains("thinking_tokens")
        || response_body.contains("reasoning_content")
        || response_body.contains("reasoning_summary")
        || response_body.contains("\"thinking\"")
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
        return find_reasoning_tokens(&value)
            .filter(|tokens| *tokens > 0)
            .or_else(|| estimate_reasoning_tokens_from_value(&value))
            .or_else(|| find_reasoning_tokens(&value));
    }

    let mut exact_tokens = None;
    let mut terminal_texts = Vec::new();
    let mut done_texts = BTreeMap::new();
    let mut delta_texts = BTreeMap::new();
    let mut fallback_texts = Vec::new();

    for value in text.lines().filter_map(parse_sse_data_line) {
        if let Some(tokens) = find_reasoning_tokens(&value) {
            exact_tokens = Some(exact_tokens.unwrap_or(0).max(tokens));
        }
        collect_sse_reasoning_texts(
            &value,
            &mut terminal_texts,
            &mut done_texts,
            &mut delta_texts,
            &mut fallback_texts,
        );
    }

    exact_tokens
        .filter(|tokens| *tokens > 0)
        .or_else(|| estimate_reasoning_tokens_from_texts(&terminal_texts))
        .or_else(|| estimate_reasoning_tokens_from_texts(done_texts.values()))
        .or_else(|| estimate_reasoning_tokens_from_texts(delta_texts.values()))
        .or_else(|| estimate_reasoning_tokens_from_texts(&fallback_texts))
        .or(exact_tokens)
}

pub fn append_record(record: &ProxyRequestRecord) -> std::io::Result<()> {
    let path = default_log_path();
    append_record_at_path(&path, record)
}

fn append_record_at_path(path: &Path, record: &ProxyRequestRecord) -> std::io::Result<()> {
    let mut index_file = open_index_file(path)?;
    index_file.lock_exclusive()?;
    let result: std::io::Result<()> = (|| {
        ensure_index_format_locked(path, &mut index_file)?;
        write_detail_record(path, record)?;

        let mut updates = read_summaries_from_locked_index(&mut index_file)?;
        let summary = ProxyRequestSummary::from(record);
        updates.push(summary.clone());
        let update_count = updates.len();
        let mut summaries = dedupe_summaries(updates);
        sort_summaries(&mut summaries);
        let removed = if summaries.len() > RUNTIME_RETAINED_RECORDS {
            summaries.split_off(RUNTIME_RETAINED_RECORDS)
        } else {
            Vec::new()
        };
        if removed.is_empty() && update_count <= MAX_PROXY_INDEX_UPDATES {
            append_summary_to_locked_index(&mut index_file, &summary)?;
        } else {
            write_summaries_to_locked_index(&mut index_file, &summaries)?;
        }
        remove_detail_records(path, &removed)?;
        Ok(())
    })();
    let unlock_result = index_file.unlock();
    result?;
    unlock_result
}

pub fn read_summaries(limit: usize) -> std::io::Result<Vec<ProxyRequestSummary>> {
    let path = default_log_path();
    read_summaries_at_path(&path, limit)
}

pub fn find_record(id: &str) -> std::io::Result<Option<ProxyRequestRecord>> {
    let path = default_log_path();
    find_record_at_path(&path, id)
}

pub fn clear_records() -> std::io::Result<()> {
    let path = default_log_path();
    clear_records_at_path(&path)
}

fn clear_records_at_path(path: &Path) -> std::io::Result<()> {
    let mut index_file = open_index_file(path)?;
    index_file.lock_exclusive()?;
    let result = (|| {
        write_summaries_to_locked_index(&mut index_file, &[])?;
        clear_detail_directory(path)
    })();
    let unlock_result = index_file.unlock();
    result?;
    unlock_result
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

fn read_summaries_at_path(path: &Path, limit: usize) -> std::io::Result<Vec<ProxyRequestSummary>> {
    let mut index_file = open_index_file(path)?;
    index_file.lock_exclusive()?;
    let result: std::io::Result<Vec<ProxyRequestSummary>> = (|| {
        ensure_index_format_locked(path, &mut index_file)?;
        let updates = read_summaries_from_locked_index(&mut index_file)?;
        let mut summaries = dedupe_summaries(updates);
        sort_summaries(&mut summaries);
        summaries.truncate(limit);
        Ok(summaries)
    })();
    let unlock_result = index_file.unlock();
    let summaries = result?;
    unlock_result?;
    Ok(summaries)
}

fn find_record_at_path(path: &Path, id: &str) -> std::io::Result<Option<ProxyRequestRecord>> {
    let mut index_file = open_index_file(path)?;
    index_file.lock_exclusive()?;
    let ensure_result = ensure_index_format_locked(path, &mut index_file);
    let unlock_result = index_file.unlock();
    ensure_result?;
    unlock_result?;

    let detail_path = detail_record_path(path, id);
    if !detail_path.is_file() {
        return Ok(None);
    }
    let mut detail_file = fs::File::open(detail_path)?;
    detail_file.lock_shared()?;
    let mut text = String::new();
    let read_result = detail_file.read_to_string(&mut text);
    let unlock_result = detail_file.unlock();
    read_result?;
    unlock_result?;
    serde_json::from_str::<ProxyRequestRecord>(&text)
        .map(Some)
        .map_err(std::io::Error::other)
}

fn retain_recent_records_at_path(path: &Path, limit: usize) -> std::io::Result<()> {
    let mut index_file = open_index_file(path)?;
    index_file.lock_exclusive()?;
    let result = (|| {
        ensure_index_format_locked(path, &mut index_file)?;
        let updates = read_summaries_from_locked_index(&mut index_file)?;
        let mut summaries = dedupe_summaries(updates);
        sort_summaries(&mut summaries);
        let removed = if summaries.len() > limit {
            summaries.split_off(limit)
        } else {
            Vec::new()
        };
        write_summaries_to_locked_index(&mut index_file, &summaries)?;
        remove_detail_records(path, &removed)
    })();
    let unlock_result = index_file.unlock();
    result?;
    unlock_result
}

fn open_index_file(path: &Path) -> std::io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}

fn ensure_index_format_locked(path: &Path, file: &mut fs::File) -> std::io::Result<()> {
    let expected = format!("{PROXY_INDEX_HEADER}\n");
    let mut prefix = vec![0_u8; expected.len()];
    file.seek(SeekFrom::Start(0))?;
    let read = file.read(&mut prefix)?;
    let valid_header = read == expected.len() && prefix == expected.as_bytes();
    let valid_size = file.metadata()?.len() <= MAX_PROXY_INDEX_BYTES;
    if valid_header && valid_size {
        return Ok(());
    }

    write_summaries_to_locked_index(file, &[])?;
    clear_detail_directory(path)
}

fn read_summaries_from_locked_index(
    file: &mut fs::File,
) -> std::io::Result<Vec<ProxyRequestSummary>> {
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<ProxyRequestSummary>(line).ok())
        .collect())
}

fn write_summaries_to_locked_index(
    file: &mut fs::File,
    summaries: &[ProxyRequestSummary],
) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "{PROXY_INDEX_HEADER}")?;
    for summary in summaries {
        serde_json::to_writer(&mut *file, summary).map_err(std::io::Error::other)?;
        writeln!(file)?;
    }
    file.flush()
}

fn append_summary_to_locked_index(
    file: &mut fs::File,
    summary: &ProxyRequestSummary,
) -> std::io::Result<()> {
    file.seek(SeekFrom::End(0))?;
    serde_json::to_writer(&mut *file, summary).map_err(std::io::Error::other)?;
    writeln!(file)?;
    file.flush()
}

fn dedupe_summaries(updates: Vec<ProxyRequestSummary>) -> Vec<ProxyRequestSummary> {
    let mut seen_ids = HashSet::new();
    let mut summaries = Vec::new();
    for summary in updates.into_iter().rev() {
        if seen_ids.insert(summary.id.clone()) {
            summaries.push(summary);
        }
    }
    summaries
}

fn sort_summaries(summaries: &mut [ProxyRequestSummary]) {
    summaries.sort_by(|left, right| {
        right
            .timestamp_ms
            .cmp(&left.timestamp_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn detail_directory(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("proxy-requests");
    path.parent()
        .unwrap_or_else(|| Path::new(""))
        .join(format!("{stem}-details"))
}

fn detail_record_path(path: &Path, id: &str) -> PathBuf {
    let digest = Sha256::digest(id.as_bytes());
    let mut name = String::with_capacity(digest.len() * 2 + 5);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".json");
    detail_directory(path).join(name)
}

fn write_detail_record(path: &Path, record: &ProxyRequestRecord) -> std::io::Result<()> {
    let directory = detail_directory(path);
    fs::create_dir_all(&directory)?;
    let detail_path = detail_record_path(path, &record.id);
    let mut detail_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(detail_path)?;
    detail_file.lock_exclusive()?;
    let result = (|| {
        let line = serialize_record_for_log(record).map_err(std::io::Error::other)?;
        detail_file.set_len(0)?;
        detail_file.seek(SeekFrom::Start(0))?;
        detail_file.write_all(line.as_bytes())?;
        detail_file.flush()
    })();
    let unlock_result = detail_file.unlock();
    result?;
    unlock_result
}

fn remove_detail_records(path: &Path, summaries: &[ProxyRequestSummary]) -> std::io::Result<()> {
    for summary in summaries {
        let detail_path = detail_record_path(path, &summary.id);
        match fs::remove_file(detail_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn clear_detail_directory(path: &Path) -> std::io::Result<()> {
    let directory = detail_directory(path);
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn serialize_record_for_log(record: &ProxyRequestRecord) -> serde_json::Result<String> {
    let line = serde_json::to_string(record)?;
    if line.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES {
        return Ok(line);
    }

    let mut trimmed = record.clone();
    trimmed.request_body.clear();
    trimmed.response_body.clear();
    trimmed.continue_thinking_request_body = None;
    trimmed.continue_thinking_before_response_body = None;
    trimmed.continue_thinking_after_response_body = None;
    trimmed.response_truncated = true;
    trimmed.response_captured_bytes = Some(0);

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
        trimmed.response_captured_bytes = Some(trimmed.response_body.len());

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

fn parse_sse_data_line(line: &str) -> Option<Value> {
    let data = line.trim().strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<Value>(data).ok()
}

fn collect_sse_reasoning_texts(
    value: &Value,
    terminal_texts: &mut Vec<String>,
    done_texts: &mut BTreeMap<String, String>,
    delta_texts: &mut BTreeMap<String, String>,
    fallback_texts: &mut Vec<String>,
) {
    match value.get("type").and_then(Value::as_str) {
        Some("response.completed" | "response.incomplete" | "response.failed") => {
            if let Some(response) = value.get("response") {
                let texts = response_output_reasoning_texts(response);
                if !texts.is_empty() {
                    *terminal_texts = texts;
                    return;
                }
            }
        }
        Some("response.output_item.done") => {
            if let Some(item) = value.get("item") {
                if let Some(text) = reasoning_item_text(item) {
                    done_texts.insert(reasoning_event_key(value), text);
                    return;
                }
            }
        }
        Some("response.reasoning_summary_text.done") => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    done_texts.insert(reasoning_event_key(value), text.to_string());
                    return;
                }
            }
        }
        Some("response.reasoning_summary_part.done") => {
            if let Some(text) = value
                .get("part")
                .and_then(|part| part.get("text"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    done_texts.insert(reasoning_event_key(value), text.to_string());
                    return;
                }
            }
        }
        Some("response.reasoning_summary_text.delta") => {
            if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    delta_texts
                        .entry(reasoning_event_key(value))
                        .or_default()
                        .push_str(delta);
                    return;
                }
            }
        }
        Some("content_block_start") => {
            if let Some(text) = value
                .get("content_block")
                .and_then(|block| block.get("thinking"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    delta_texts
                        .entry(reasoning_event_key(value))
                        .or_default()
                        .push_str(text);
                    return;
                }
            }
        }
        Some("content_block_delta") => {
            if let Some(text) = value
                .get("delta")
                .and_then(|delta| delta.get("thinking"))
                .and_then(Value::as_str)
            {
                if !text.is_empty() {
                    delta_texts
                        .entry(reasoning_event_key(value))
                        .or_default()
                        .push_str(text);
                    return;
                }
            }
        }
        _ => {}
    }

    collect_chat_delta_reasoning_texts(value, delta_texts);
    if fallback_texts.is_empty() {
        fallback_texts.extend(reasoning_texts_from_value(value));
    }
}

fn reasoning_event_key(value: &Value) -> String {
    let item = value
        .get("item_id")
        .or_else(|| value.get("index"))
        .and_then(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        })
        .unwrap_or_else(|| "reasoning".to_string());
    let output_index = value
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let summary_index = value
        .get("summary_index")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    format!("{item}:{output_index}:{summary_index}")
}

fn estimate_reasoning_tokens_from_value(value: &Value) -> Option<u64> {
    estimate_reasoning_tokens_from_texts(reasoning_texts_from_value(value))
}

fn reasoning_texts_from_value(value: &Value) -> Vec<String> {
    let mut texts = response_output_reasoning_texts(value);
    if !texts.is_empty() {
        return texts;
    }

    if let Some(response) = value.get("response") {
        texts = response_output_reasoning_texts(response);
        if !texts.is_empty() {
            return texts;
        }
    }

    collect_chat_message_reasoning_texts(value, &mut texts);
    if !texts.is_empty() {
        return texts;
    }

    collect_anthropic_reasoning_texts(value, &mut texts);
    if !texts.is_empty() {
        return texts;
    }

    collect_generic_reasoning_texts(value, &mut texts);
    texts
}

fn response_output_reasoning_texts(value: &Value) -> Vec<String> {
    value
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(reasoning_item_text)
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn reasoning_item_text(item: &Value) -> Option<String> {
    let is_reasoning_item = item.get("type").and_then(Value::as_str) == Some("reasoning")
        || item.get("reasoning_content").is_some();
    if !is_reasoning_item {
        return None;
    }

    item.get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
        .or_else(|| summary_text(item.get("summary")))
        .or_else(|| {
            item.get("content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            item.get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
        })
}

fn summary_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("content").and_then(Value::as_str))
                        .or_else(|| part.as_str())
                })
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(_) => value
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| value.get("content").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .map(ToString::to_string),
        _ => None,
    }
}

fn collect_chat_delta_reasoning_texts(value: &Value, texts: &mut BTreeMap<String, String>) {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for (index, choice) in choices.iter().enumerate() {
        let Some(text) = choice
            .get("delta")
            .and_then(reasoning_text_from_object)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        texts
            .entry(format!("chat-choice-{index}"))
            .or_default()
            .push_str(&text);
    }
}

fn collect_chat_message_reasoning_texts(value: &Value, texts: &mut Vec<String>) {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for choice in choices {
        if let Some(text) = choice
            .get("message")
            .and_then(reasoning_text_from_object)
            .filter(|text| !text.is_empty())
        {
            texts.push(text);
        }
    }
}

fn collect_anthropic_reasoning_texts(value: &Value, texts: &mut Vec<String>) {
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return;
    };
    for block in content {
        if block.get("type").and_then(Value::as_str) == Some("thinking") {
            if let Some(text) = block
                .get("thinking")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                texts.push(text.to_string());
            }
        }
    }
}

fn collect_generic_reasoning_texts(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(text) = reasoning_text_from_object(value) {
                texts.push(text);
                return;
            }
            if let Some(text) = summary_text(map.get("summary")) {
                texts.push(text);
                return;
            }
            for child in map.values() {
                collect_generic_reasoning_texts(child, texts);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_generic_reasoning_texts(item, texts);
            }
        }
        _ => {}
    }
}

fn reasoning_text_from_object(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    value
        .get("reasoning")
        .and_then(|reasoning| {
            reasoning
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| reasoning.get("text").and_then(Value::as_str))
                .or_else(|| reasoning.get("summary").and_then(Value::as_str))
        })
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn estimate_reasoning_tokens_from_texts(
    texts: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<u64> {
    let mut total = 0_u64;
    for text in texts {
        total = total.saturating_add(estimate_reasoning_tokens_from_text(text.as_ref()));
    }
    (total > 0).then_some(total)
}

fn estimate_reasoning_tokens_from_text(text: &str) -> u64 {
    let mut tokens = 0_u64;
    let mut ascii_word_len = 0_u64;

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ascii_word_len += 1;
            continue;
        }

        tokens = tokens.saturating_add(ascii_word_tokens(ascii_word_len));
        ascii_word_len = 0;

        if ch.is_whitespace() {
            continue;
        }
        if is_cjk_or_kana_or_hangul(ch) {
            tokens += 1;
        } else if ch.is_ascii_punctuation() {
            tokens += 1;
        } else {
            tokens += 1;
        }
    }

    tokens.saturating_add(ascii_word_tokens(ascii_word_len))
}

fn ascii_word_tokens(len: u64) -> u64 {
    if len == 0 { 0 } else { len.div_ceil(4) }
}

fn is_cjk_or_kana_or_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30ff
            | 0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xac00..=0xd7af
            | 0xf900..=0xfaff
            | 0x20000..=0x2a6df
            | 0x2a700..=0x2b73f
            | 0x2b740..=0x2b81f
            | 0x2b820..=0x2ceaf
    )
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

#[cfg(test)]
mod tests {
    use super::{
        ProxyRequestRecord, ProxyRequestState, append_record, append_record_at_path,
        clear_records_at_path, current_timestamp_ms, extract_reasoning_tokens_from_response_body,
        extract_request_metadata, find_record, find_record_at_path, read_summaries,
        read_summaries_at_path, retain_recent_records, serialize_record_for_log,
    };

    fn temp_proxy_log_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "codex-elves-{name}-{}-{}.jsonl",
            std::process::id(),
            super::current_timestamp_ms()
        ))
    }

    fn remove_proxy_log_artifacts(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(super::detail_directory(path));
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
    fn estimates_reasoning_tokens_from_chat_reasoning_content() {
        let body = br#"{
            "id": "chatcmpl_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "abcd efgh",
                    "content": "done"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(2));
    }

    #[test]
    fn estimates_reasoning_tokens_from_responses_sse_reasoning_content() {
        let body = br#"event: response.reasoning_summary_text.delta
data: {"type":"response.reasoning_summary_text.delta","item_id":"rs_1","output_index":0,"summary_index":0,"delta":"abcd "}

event: response.reasoning_summary_text.delta
data: {"type":"response.reasoning_summary_text.delta","item_id":"rs_1","output_index":0,"summary_index":0,"delta":"efgh"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","output":[{"type":"reasoning","reasoning_content":"abcd efgh","summary":[{"type":"summary_text","text":"abcd efgh"}]}],"usage":{"output_tokens":5}}}

data: [DONE]
"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(2));
    }

    #[test]
    fn keeps_reported_reasoning_tokens_before_text_estimate() {
        let body = br#"{
            "id": "chatcmpl_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "abcd efgh ijkl mnop",
                    "content": "done"
                }
            }],
            "usage": {
                "completion_tokens_details": {
                    "reasoning_tokens": 3
                }
            }
        }"#;

        assert_eq!(extract_reasoning_tokens_from_response_body(body), Some(3));
    }

    #[test]
    fn summary_backfills_reasoning_tokens_from_existing_response_body() {
        let mut record = sample_proxy_record("existing-log");
        record.reasoning_tokens = None;
        record.response_body = r#"{
            "id": "chatcmpl_1",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "abcd efgh",
                    "content": "done"
                }
            }]
        }"#
        .to_string();

        let summary = super::ProxyRequestSummary::from(&record);

        assert_eq!(record.reasoning_tokens, None);
        assert_eq!(summary.reasoning_tokens, Some(2));
    }

    #[test]
    fn summary_trims_large_error_without_changing_detail_record() {
        let mut record = sample_proxy_record("large-error");
        record.error = Some("x".repeat(super::MAX_SUMMARY_ERROR_BYTES + 128));

        let summary = super::ProxyRequestSummary::from(&record);

        assert_eq!(
            summary.error.as_deref().map(str::len),
            Some(super::MAX_SUMMARY_ERROR_BYTES)
        );
        assert_eq!(
            record.error.as_deref().map(str::len),
            Some(super::MAX_SUMMARY_ERROR_BYTES + 128)
        );
    }

    #[test]
    fn legacy_record_without_transport_defaults_to_http() {
        let mut value = serde_json::to_value(sample_proxy_record("legacy-transport")).unwrap();
        value.as_object_mut().unwrap().remove("transport");

        let record: ProxyRequestRecord = serde_json::from_value(value).unwrap();

        assert_eq!(record.transport, super::ProxyRequestTransport::Http);
    }

    #[test]
    fn append_record_writes_locked_jsonl_file() {
        let path = temp_proxy_log_path("append-record");
        let previous = crate::paths::set_proxy_log_path_for_tests(Some(path.clone()));
        let record = ProxyRequestRecord {
            id: "test-record".to_string(),
            state: ProxyRequestState::Completed,
            transport: super::ProxyRequestTransport::Http,
            timestamp_ms: current_timestamp_ms(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            remote_addr: Some("127.0.0.1:1".to_string()),
            model: Some("gpt-5.4".to_string()),
            reasoning_tokens: Some(516),
            reasoning_effort: Some("medium".to_string()),
            reasoning_source: Some("reasoning.effort".to_string()),
            continue_thinking_triggered: false,
            continue_thinking_rounds: 0,
            continue_thinking_request_body: None,
            continue_thinking_before_response_body: None,
            continue_thinking_after_response_body: None,
            service_tier: Some("auto".to_string()),
            relay_id: Some("relay-test".to_string()),
            relay_name: Some("Test".to_string()),
            endpoint: Some("https://example.test/v1/responses".to_string()),
            response_protocol: Some("responses".to_string()),
            status_code: Some(200),
            first_token_ms: Some(4),
            duration_ms: Some(10),
            stream: false,
            request_bytes: 2,
            response_bytes: Some(2),
            response_captured_bytes: Some(2),
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
        assert_eq!(found.transport, super::ProxyRequestTransport::Http);
        assert_eq!(found.reasoning_tokens, Some(516));
        assert_eq!(found.request_body, "{}");
        assert_eq!(found.response_body, "{}");
        let index_text = std::fs::read_to_string(&path).expect("read proxy log index");
        assert!(index_text.starts_with(super::PROXY_INDEX_HEADER));
        assert!(!index_text.contains("requestBody"));
        assert!(!index_text.contains("responseBody"));

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
        assert!(
            find_record("test-record")
                .expect("read removed detail")
                .is_none()
        );

        remove_proxy_log_artifacts(&path);
        crate::paths::set_proxy_log_path_for_tests(previous);
    }

    #[test]
    fn read_summaries_deduplicates_pending_record_by_latest_entry() {
        let path = temp_proxy_log_path("dedupe-pending");
        let mut pending = sample_proxy_record("same-request");
        pending.state = ProxyRequestState::Pending;
        pending.status_code = None;
        pending.first_token_ms = None;
        pending.duration_ms = None;
        pending.response_bytes = None;
        pending.response_captured_bytes = None;
        pending.response_body.clear();

        let mut completed = pending.clone();
        completed.state = ProxyRequestState::Completed;
        completed.timestamp_ms += 1;
        completed.status_code = Some(200);
        completed.duration_ms = Some(12);
        completed.response_bytes = Some(2);
        completed.response_captured_bytes = Some(2);
        completed.response_body = "{}".to_string();

        append_record_at_path(&path, &pending).expect("append pending proxy log record");
        append_record_at_path(&path, &completed).expect("append completed proxy log record");

        let summaries = read_summaries_at_path(&path, 10).expect("read proxy log summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "same-request");
        assert_eq!(summaries[0].state, ProxyRequestState::Completed);
        assert_eq!(summaries[0].status_code, Some(200));
        assert_eq!(summaries[0].duration_ms, Some(12));
        let detail = find_record_at_path(&path, "same-request")
            .expect("read proxy log detail")
            .expect("detail should exist");
        assert_eq!(detail.state, ProxyRequestState::Completed);
        assert_eq!(detail.response_body, "{}");

        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn read_summaries_keeps_latest_pending_first_token_update() {
        let path = temp_proxy_log_path("dedupe-first-token");
        let mut pending = sample_proxy_record("stream-request");
        pending.state = ProxyRequestState::Pending;
        pending.status_code = None;
        pending.first_token_ms = None;
        pending.duration_ms = None;
        pending.response_bytes = None;
        pending.response_captured_bytes = None;
        pending.response_body.clear();

        let mut first_token = pending.clone();
        first_token.timestamp_ms += 1;
        first_token.status_code = Some(200);
        first_token.first_token_ms = Some(345);
        first_token.response_protocol = Some("responses".to_string());

        append_record_at_path(&path, &pending).expect("append pending proxy log record");
        append_record_at_path(&path, &first_token).expect("append first token proxy log record");

        let summaries = read_summaries_at_path(&path, 10).expect("read proxy log summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "stream-request");
        assert_eq!(summaries[0].state, ProxyRequestState::Pending);
        assert_eq!(summaries[0].status_code, Some(200));
        assert_eq!(summaries[0].first_token_ms, Some(345));
        assert_eq!(summaries[0].duration_ms, None);
        let detail = find_record_at_path(&path, "stream-request")
            .expect("read proxy log detail")
            .expect("detail should exist");
        assert_eq!(detail.response_body, "");

        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn read_summaries_orders_by_request_timestamp_not_update_order() {
        let path = temp_proxy_log_path("request-time-order");
        let mut older = sample_proxy_record("older-request");
        older.state = ProxyRequestState::Pending;
        older.timestamp_ms = 100;
        older.status_code = None;
        older.first_token_ms = None;
        older.duration_ms = None;
        older.response_bytes = None;
        older.response_captured_bytes = None;
        older.response_body.clear();

        let mut newer = sample_proxy_record("newer-request");
        newer.state = ProxyRequestState::Pending;
        newer.timestamp_ms = 200;
        newer.status_code = None;
        newer.first_token_ms = None;
        newer.duration_ms = None;
        newer.response_bytes = None;
        newer.response_captured_bytes = None;
        newer.response_body.clear();

        let mut older_first_token = older.clone();
        older_first_token.status_code = Some(200);
        older_first_token.first_token_ms = Some(345);

        append_record_at_path(&path, &older).expect("append older pending record");
        append_record_at_path(&path, &newer).expect("append newer pending record");
        append_record_at_path(&path, &older_first_token).expect("append older first token update");

        let summaries = read_summaries_at_path(&path, 10).expect("read proxy log summaries");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, "newer-request");
        assert_eq!(summaries[0].timestamp_ms, 200);
        assert_eq!(summaries[1].id, "older-request");
        assert_eq!(summaries[1].timestamp_ms, 100);
        assert_eq!(summaries[1].first_token_ms, Some(345));

        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn append_record_discards_legacy_single_file_log() {
        let path = temp_proxy_log_path("discard-legacy-log");
        let legacy = sample_proxy_record("legacy-record");
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&legacy).expect("serialize legacy proxy log")
            ),
        )
        .expect("seed legacy proxy log");
        let record = sample_proxy_record("fresh-record");

        append_record_at_path(&path, &record).expect("append proxy log record");

        let summaries = read_summaries_at_path(&path, 10).expect("read proxy log summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "fresh-record");
        assert!(
            find_record_at_path(&path, "legacy-record")
                .expect("read discarded legacy detail")
                .is_none()
        );
        assert!(
            find_record_at_path(&path, "fresh-record")
                .expect("read fresh detail")
                .is_some()
        );

        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn read_summaries_discards_legacy_single_file_log() {
        let path = temp_proxy_log_path("discard-legacy-log-on-read");
        let legacy = sample_proxy_record("legacy-record");
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&legacy).expect("serialize legacy proxy log")
            ),
        )
        .expect("seed legacy proxy log");

        let summaries = read_summaries_at_path(&path, 10).expect("read proxy log summaries");

        assert!(summaries.is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).expect("read reset proxy log index"),
            format!("{}\n", super::PROXY_INDEX_HEADER)
        );
        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn append_record_retains_recent_runtime_record_limit() {
        let path = temp_proxy_log_path("runtime-retain-records");
        let record = sample_proxy_record("record-0");

        for index in 0..=super::RUNTIME_RETAINED_RECORDS {
            let mut next = record.clone();
            next.id = format!("record-{index}");
            next.timestamp_ms += index as u64;
            append_record_at_path(&path, &next).expect("append proxy log record");
        }

        let text = std::fs::read_to_string(&path).expect("read proxy log");
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), super::RUNTIME_RETAINED_RECORDS + 1);
        assert_eq!(lines[0], super::PROXY_INDEX_HEADER);
        assert!(!text.contains("\"id\":\"record-0\""));
        assert!(text.contains("\"id\":\"record-1\""));
        assert!(text.contains(&format!(
            "\"id\":\"record-{}\"",
            super::RUNTIME_RETAINED_RECORDS
        )));
        assert!(
            find_record_at_path(&path, "record-0")
                .expect("read evicted detail")
                .is_none()
        );
        assert!(
            find_record_at_path(
                &path,
                &format!("record-{}", super::RUNTIME_RETAINED_RECORDS)
            )
            .expect("read retained detail")
            .is_some()
        );

        remove_proxy_log_artifacts(&path);
    }

    #[test]
    fn large_proxy_record_is_trimmed_to_fit_single_log_file_limit() {
        let mut record = sample_proxy_record("large-record");
        record.request_body = "q".repeat(128 * 1024);
        record.response_body = "r".repeat(crate::log_limits::MAX_LOG_FILE_BYTES as usize + 1);
        record.response_captured_bytes = Some(record.response_body.len());
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
        append_record_at_path(&path, &sample_proxy_record("old")).expect("seed proxy log");

        clear_records_at_path(&path).expect("clear proxy logs");

        let text = std::fs::read_to_string(&path).expect("read cleared proxy log");
        assert_eq!(text, format!("{}\n", super::PROXY_INDEX_HEADER));
        assert!(
            find_record_at_path(&path, "old")
                .expect("read cleared detail")
                .is_none()
        );

        remove_proxy_log_artifacts(&path);
    }

    fn sample_proxy_record(id: &str) -> ProxyRequestRecord {
        ProxyRequestRecord {
            id: id.to_string(),
            state: ProxyRequestState::Completed,
            transport: super::ProxyRequestTransport::Http,
            timestamp_ms: current_timestamp_ms(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            remote_addr: Some("127.0.0.1:1".to_string()),
            model: Some("glm-5.2".to_string()),
            reasoning_tokens: None,
            reasoning_effort: None,
            reasoning_source: None,
            continue_thinking_triggered: false,
            continue_thinking_rounds: 0,
            continue_thinking_request_body: None,
            continue_thinking_before_response_body: None,
            continue_thinking_after_response_body: None,
            service_tier: None,
            relay_id: Some("relay-test".to_string()),
            relay_name: Some("Test".to_string()),
            endpoint: Some("https://example.test/v1/chat/completions".to_string()),
            response_protocol: Some("chatCompletions".to_string()),
            status_code: Some(200),
            first_token_ms: None,
            duration_ms: Some(10),
            stream: false,
            request_bytes: 2,
            response_bytes: Some(2),
            response_captured_bytes: Some(2),
            response_truncated: false,
            request_body: "{}".to_string(),
            response_body: "{}".to_string(),
            error: None,
        }
    }
}
