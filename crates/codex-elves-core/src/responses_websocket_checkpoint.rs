use std::collections::VecDeque;
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CHECKPOINT_CACHE_CAPACITY: usize = 16;
const MAX_ROLLOUT_LINE_BYTES: usize = 128 * 1024 * 1024;
const REVERSE_SCAN_CHUNK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct RestoredCompactionCheckpoint {
    pub(crate) checkpoint_prefix_items: usize,
    pub(crate) original_input_items: usize,
    pub(crate) retained_input_items: usize,
    pub(crate) window_number: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct CompactionCheckpointRestore {
    pub(crate) payload: Value,
    pub(crate) restored: Option<RestoredCompactionCheckpoint>,
    pub(crate) skip_reason: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowSelector {
    raw: String,
    number: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CheckpointCacheKey {
    rollout_path: PathBuf,
    window: WindowSelector,
}

#[derive(Clone, Debug)]
struct CachedCheckpoint {
    compaction: Value,
    prefix_digests: Vec<[u8; 32]>,
    window_number: Option<u64>,
}

#[derive(Default)]
struct CheckpointCache {
    entries: VecDeque<(CheckpointCacheKey, CachedCheckpoint)>,
}

impl CheckpointCache {
    fn get(&mut self, key: &CheckpointCacheKey) -> Option<CachedCheckpoint> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = self.entries.remove(index)?;
        let checkpoint = entry.1.clone();
        self.entries.push_front(entry);
        Some(checkpoint)
    }

    fn insert(&mut self, key: CheckpointCacheKey, checkpoint: CachedCheckpoint) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            self.entries.remove(index);
        }
        self.entries.push_front((key, checkpoint));
        self.entries.truncate(CHECKPOINT_CACHE_CAPACITY);
    }
}

pub(crate) fn restore_missing_compaction_checkpoint(
    mut payload: Value,
    codex_home: &Path,
) -> CompactionCheckpointRestore {
    if payload
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
    {
        return skipped(payload, "previous_response_id_present");
    }
    let Some(input) = payload.get("input").and_then(Value::as_array) else {
        return skipped(payload, "input_missing");
    };
    if input
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("compaction"))
    {
        return skipped(payload, "wire_compaction_present");
    }

    let Some(thread_id) = request_thread_id(&payload) else {
        return skipped(payload, "thread_id_missing");
    };
    let Some(window) = request_window_selector(&payload, &thread_id) else {
        return skipped(payload, "window_id_missing");
    };
    let Some(rollout_path) = find_rollout_path(codex_home, &thread_id) else {
        return skipped(payload, "rollout_not_found");
    };
    let cache_key = CheckpointCacheKey {
        rollout_path: rollout_path.clone(),
        window: window.clone(),
    };
    let checkpoint = cached_checkpoint(&cache_key)
        .or_else(|| load_checkpoint(&rollout_path, &window))
        .inspect(|checkpoint| cache_checkpoint(cache_key, checkpoint.clone()));
    let Some(checkpoint) = checkpoint else {
        return skipped(payload, "checkpoint_not_found");
    };

    let Some(input) = payload.get_mut("input").and_then(Value::as_array_mut) else {
        return skipped(payload, "input_missing");
    };
    let prefix_len = checkpoint.prefix_digests.len();
    if prefix_len == 0 || input.len() < prefix_len {
        return skipped(payload, "checkpoint_prefix_length_invalid");
    }
    let prefix_matches = input
        .iter()
        .take(prefix_len)
        .zip(&checkpoint.prefix_digests)
        .all(|(item, expected)| value_digest(item).as_ref() == Some(expected));
    if !prefix_matches {
        return skipped(payload, "checkpoint_prefix_mismatch");
    }

    let original_input_items = input.len();
    input.splice(..prefix_len, std::iter::once(checkpoint.compaction));
    let retained_input_items = input.len();
    CompactionCheckpointRestore {
        payload,
        restored: Some(RestoredCompactionCheckpoint {
            checkpoint_prefix_items: prefix_len,
            original_input_items,
            retained_input_items,
            window_number: checkpoint.window_number,
        }),
        skip_reason: None,
    }
}

fn skipped(payload: Value, reason: &'static str) -> CompactionCheckpointRestore {
    CompactionCheckpointRestore {
        payload,
        restored: None,
        skip_reason: Some(reason),
    }
}

fn request_thread_id(payload: &Value) -> Option<String> {
    let metadata = payload.get("client_metadata").and_then(Value::as_object);
    ["thread_id", "session_id"]
        .into_iter()
        .filter_map(|key| metadata?.get(key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| Uuid::parse_str(value).is_ok())
        .or_else(|| payload.get("prompt_cache_key").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| Uuid::parse_str(value).is_ok())
        .map(ToString::to_string)
}

fn request_window_selector(payload: &Value, thread_id: &str) -> Option<WindowSelector> {
    let metadata = payload.get("client_metadata")?.as_object()?;
    let raw = metadata
        .get("x-codex-window-id")
        .and_then(Value::as_str)
        .or_else(|| metadata.get("window_id").and_then(Value::as_str))
        .map(ToString::to_string)
        .or_else(|| {
            let turn_metadata = metadata.get("x-codex-turn-metadata")?;
            match turn_metadata {
                Value::Object(object) => object
                    .get("window_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                Value::String(text) => serde_json::from_str::<Value>(text).ok().and_then(|value| {
                    value
                        .get("window_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                }),
                _ => None,
            }
        })?;
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let number = raw
        .rsplit_once(':')
        .filter(|(prefix, _)| prefix.trim() == thread_id)
        .and_then(|(_, number)| number.parse::<u64>().ok())
        .or_else(|| raw.parse::<u64>().ok());
    Some(WindowSelector { raw, number })
}

fn find_rollout_path(codex_home: &Path, thread_id: &str) -> Option<PathBuf> {
    find_rollout_path_from_state_db(codex_home, thread_id)
        .or_else(|| find_rollout_path_by_filename(codex_home, thread_id))
}

fn find_rollout_path_from_state_db(codex_home: &Path, thread_id: &str) -> Option<PathBuf> {
    for db_path in crate::codex_sqlite::codex_session_db_paths_from_home(codex_home) {
        let Ok(connection) = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        let _ = connection.busy_timeout(Duration::from_millis(100));
        let Ok(path) = connection
            .query_row(
                "SELECT rollout_path FROM threads WHERE id = ?1 LIMIT 1",
                [thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        else {
            continue;
        };
        let Some(path) = path else {
            continue;
        };
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            codex_home.join(path)
        };
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn find_rollout_path_by_filename(codex_home: &Path, thread_id: &str) -> Option<PathBuf> {
    let mut matches = Vec::new();
    for root in [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ] {
        collect_rollout_matches(&root, thread_id, &mut matches);
    }
    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    matches.pop()
}

fn collect_rollout_matches(root: &Path, thread_id: &str, matches: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollout_matches(&path, thread_id, matches);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.contains(thread_id) && name.ends_with(".jsonl") {
            matches.push(path);
        }
    }
}

fn cached_checkpoint(key: &CheckpointCacheKey) -> Option<CachedCheckpoint> {
    checkpoint_cache().lock().ok()?.get(key)
}

fn cache_checkpoint(key: CheckpointCacheKey, checkpoint: CachedCheckpoint) {
    if let Ok(mut cache) = checkpoint_cache().lock() {
        cache.insert(key, checkpoint);
    }
}

fn checkpoint_cache() -> &'static Mutex<CheckpointCache> {
    static CACHE: OnceLock<Mutex<CheckpointCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CheckpointCache::default()))
}

fn load_checkpoint(path: &Path, window: &WindowSelector) -> Option<CachedCheckpoint> {
    let mut file = File::open(path).ok()?;
    let mut position = file.metadata().ok()?.len();
    let mut pending = Vec::new();
    let mut pending_oversized = false;

    while position > 0 {
        let read_size = usize::try_from(position.min(REVERSE_SCAN_CHUNK_BYTES as u64)).ok()?;
        position = position.saturating_sub(read_size as u64);
        file.seek(SeekFrom::Start(position)).ok()?;
        let mut chunk = vec![0_u8; read_size];
        file.read_exact(&mut chunk).ok()?;

        let mut end = chunk.len();
        let mut completed_pending = false;
        while let Some(newline) = chunk[..end].iter().rposition(|byte| *byte == b'\n') {
            let segment = &chunk[newline + 1..end];
            if !completed_pending {
                if !pending_oversized
                    && let Some(line) = join_line_parts(segment, &pending)
                    && let Some(checkpoint) = checkpoint_from_line(&line, window)
                {
                    return Some(checkpoint);
                }
                pending.clear();
                pending_oversized = false;
                completed_pending = true;
            } else if let Some(checkpoint) = checkpoint_from_line(segment, window) {
                return Some(checkpoint);
            }
            end = newline;
        }

        if completed_pending {
            pending.clear();
            pending_oversized = chunk[..end].len() > MAX_ROLLOUT_LINE_BYTES;
            if !pending_oversized {
                pending.extend_from_slice(&chunk[..end]);
            }
        } else if !pending_oversized {
            if chunk.len().saturating_add(pending.len()) > MAX_ROLLOUT_LINE_BYTES {
                pending.clear();
                pending_oversized = true;
            } else {
                let mut combined = Vec::with_capacity(chunk.len() + pending.len());
                combined.extend_from_slice(&chunk);
                combined.extend_from_slice(&pending);
                pending = combined;
            }
        }
    }

    if pending_oversized {
        None
    } else {
        checkpoint_from_line(&pending, window)
    }
}

fn join_line_parts(prefix: &[u8], suffix: &[u8]) -> Option<Vec<u8>> {
    if prefix.len().saturating_add(suffix.len()) > MAX_ROLLOUT_LINE_BYTES {
        return None;
    }
    let mut line = Vec::with_capacity(prefix.len() + suffix.len());
    line.extend_from_slice(prefix);
    line.extend_from_slice(suffix);
    Some(line)
}

fn checkpoint_from_line(line: &[u8], window: &WindowSelector) -> Option<CachedCheckpoint> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty()
        || line.len() > MAX_ROLLOUT_LINE_BYTES
        || !line
            .windows(b"\"compacted\"".len())
            .any(|candidate| candidate == b"\"compacted\"")
    {
        return None;
    }
    let value = serde_json::from_slice::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("compacted") {
        return None;
    }
    let payload = value.get("payload")?;
    checkpoint_matches_window(payload, window).then(|| checkpoint_from_payload(payload))?
}

fn checkpoint_matches_window(payload: &Value, window: &WindowSelector) -> bool {
    window
        .number
        .is_some_and(|number| payload.get("window_number").and_then(Value::as_u64) == Some(number))
        || payload
            .get("window_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value == window.raw)
}

fn checkpoint_from_payload(payload: &Value) -> Option<CachedCheckpoint> {
    let history = payload.get("replacement_history")?.as_array()?;
    let compaction_index = history
        .iter()
        .rposition(|item| item.get("type").and_then(Value::as_str) == Some("compaction"))?;
    if compaction_index == 0 || compaction_index + 1 != history.len() {
        return None;
    }
    let prefix = &history[..compaction_index];
    if prefix.iter().any(|item| {
        item.get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| id.trim().is_empty())
    }) {
        return None;
    }
    let prefix_digests = prefix
        .iter()
        .map(value_digest)
        .collect::<Option<Vec<_>>>()?;
    Some(CachedCheckpoint {
        compaction: history[compaction_index].clone(),
        prefix_digests,
        window_number: payload.get("window_number").and_then(Value::as_u64),
    })
}

fn value_digest(value: &Value) -> Option<[u8; 32]> {
    let serialized = serde_json::to_vec(value).ok()?;
    let digest = Sha256::digest(serialized);
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restores_exact_checkpoint_prefix_and_rejects_modified_history() {
        let temp = tempfile::tempdir().unwrap();
        let codex_home = temp.path();
        let thread_id = Uuid::new_v4().to_string();
        let rollout_path = codex_home
            .join("sessions")
            .join("2026")
            .join("08")
            .join("12")
            .join(format!("rollout-2026-08-12T00-00-00-{thread_id}.jsonl"));
        fs::create_dir_all(rollout_path.parent().unwrap()).unwrap();
        let historical = json!({
            "type": "message",
            "id": "msg_history",
            "role": "user",
            "content": [{"type": "input_text", "text": "历史"}]
        });
        let compaction = json!({
            "type": "compaction",
            "id": "cmp_window_7",
            "encrypted_content": "opaque"
        });
        let checkpoint = json!({
            "timestamp": "2026-08-12T00:00:00Z",
            "type": "compacted",
            "payload": {
                "message": "",
                "replacement_history": [historical.clone(), compaction.clone()],
                "window_number": 7,
                "window_id": "window-seven"
            }
        });
        fs::write(&rollout_path, format!("{checkpoint}\n")).unwrap();
        create_thread_state_db(codex_home, &thread_id, &rollout_path);

        let payload = json!({
            "type": "response.create",
            "model": "gpt-5.6",
            "input": [
                historical.clone(),
                {
                    "type": "message",
                    "id": "msg_current",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "继续"}]
                }
            ],
            "client_metadata": {
                "thread_id": thread_id,
                "session_id": thread_id,
                "x-codex-window-id": format!("{thread_id}:7")
            }
        });
        let restored = restore_missing_compaction_checkpoint(payload.clone(), codex_home);
        assert_eq!(restored.payload["input"].as_array().unwrap().len(), 2);
        assert_eq!(restored.payload["input"][0], compaction);
        assert_eq!(
            restored
                .restored
                .as_ref()
                .map(|metadata| metadata.checkpoint_prefix_items),
            Some(1)
        );

        let mut modified = payload;
        modified["input"][0]["content"][0]["text"] = json!("被修改");
        let skipped = restore_missing_compaction_checkpoint(modified.clone(), codex_home);
        assert_eq!(skipped.payload, modified);
        assert_eq!(skipped.skip_reason, Some("checkpoint_prefix_mismatch"));
    }

    fn create_thread_state_db(codex_home: &Path, thread_id: &str, rollout_path: &Path) {
        let db_path = codex_home.join("state_5.sqlite");
        let connection = Connection::open(db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    rollout_path TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (id, rollout_path) VALUES (?1, ?2)",
                (thread_id, rollout_path.to_string_lossy().as_ref()),
            )
            .unwrap();
    }
}
