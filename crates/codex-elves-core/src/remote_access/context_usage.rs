use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};

const TAIL_BYTES: u64 = 256 * 1024;
const MAX_TAIL_BYTES: u64 = 8 * 1024 * 1024;
const CACHE_CAPACITY: usize = 64;

struct CachedUsage {
    len: u64,
    modified: SystemTime,
    usage: Option<Value>,
    accessed: Instant,
}

fn valid_usage(value: &Value) -> bool {
    value["modelContextWindow"].as_u64().is_some_and(|n| n > 0)
        && value["last"]["totalTokens"].as_u64().is_some()
}

pub(super) fn needs_usage(value: &Value, thread_id: &str) -> bool {
    value["ok"] == true
        && value["thread"]["id"].as_str() == Some(thread_id)
        && !valid_usage(&value["thread"]["tokenUsage"])
}

pub(super) fn fill_missing(home: &Path, thread_id: &str, mut value: Value) -> Value {
    if !needs_usage(&value, thread_id) {
        return value;
    }
    // 路径只由本机数据库的精确会话 ID 得到，不接受客户端路径，也不按标题匹配。
    for db_path in crate::codex_sqlite::codex_session_db_paths_from_home(home) {
        let Ok(db) = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        let _ = db.busy_timeout(Duration::from_millis(100));
        let Ok(Some(path)) = db
            .query_row(
                "SELECT rollout_path FROM threads WHERE id = ?1 LIMIT 1",
                [thread_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        else {
            continue;
        };
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            home.join(path)
        };
        if let Ok(Some(usage)) = read_usage(&path) {
            value["thread"]["tokenUsage"] = usage;
            break;
        }
    }
    value
}

fn read_usage(path: &Path) -> std::io::Result<Option<Value>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedUsage>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let len = metadata.len();
    let modified = metadata.modified()?;
    {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.get_mut(path)
            && entry.len == len
            && entry.modified == modified
        {
            entry.accessed = Instant::now();
            return Ok(entry.usage.clone());
        }
    }
    let mut size = len.min(TAIL_BYTES);
    let usage = loop {
        let offset = len - size;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; size as usize];
        file.read_exact(&mut bytes)?;
        let start = if offset == 0 {
            0
        } else {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |index| index + 1)
        };
        let usage = bytes[start..]
            .rsplit(|byte| *byte == b'\n')
            .find_map(usage_from_line);
        if usage.is_some() || offset == 0 || size >= MAX_TAIL_BYTES {
            break usage;
        }
        size = (size * 2).min(len).min(MAX_TAIL_BYTES);
    };
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= CACHE_CAPACITY
        && !cache.contains_key(path)
        && let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.accessed)
            .map(|(path, _)| path.clone())
    {
        cache.remove(&oldest);
    }
    cache.insert(
        path.to_path_buf(),
        CachedUsage {
            len,
            modified,
            usage: usage.clone(),
            accessed: Instant::now(),
        },
    );
    Ok(usage)
}

fn usage_from_line(line: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(line).ok()?;
    if !text.contains("\"token_count\"") {
        return None;
    }
    let event: Value = serde_json::from_str(text).ok()?;
    if event["type"] != "event_msg" || event["payload"]["type"] != "token_count" {
        return None;
    }
    let info = &event["payload"]["info"];
    let capacity = info["model_context_window"].as_u64().filter(|n| *n > 0)?;
    // 这里是最后一次请求占用，不是整段对话累计用量，也不是最后一轮的请求总和。
    let used = info["last_token_usage"]["total_tokens"].as_u64()?;
    Some(json!({
        "modelContextWindow": capacity,
        "last": { "totalTokens": used }
    }))
}
