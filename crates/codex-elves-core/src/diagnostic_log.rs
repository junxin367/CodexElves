use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

static TEST_LOG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static LOG_SENDER: OnceLock<SyncSender<LogWrite>> = OnceLock::new();
static DROPPED_LOG_WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct LogWrite {
    path: PathBuf,
    line: String,
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticRecord {
    timestamp_ms: u64,
    pid: u32,
    event: String,
    detail: Value,
}

pub fn append_diagnostic_log(event: &str, detail: impl Serialize) -> std::io::Result<()> {
    let path = diagnostic_log_path();
    let detail = serde_json::to_value(detail).unwrap_or_else(|error| {
        json!({
            "serialization_error": error.to_string()
        })
    });
    let record = DiagnosticRecord {
        timestamp_ms: now_ms(),
        pid: std::process::id(),
        event: event.to_string(),
        detail,
    };
    let line = serde_json::to_string(&record).unwrap_or_else(|error| {
        json!({
            "timestamp_ms": now_ms(),
            "pid": std::process::id(),
            "event": "diagnostic_log.serialization_failed",
            "detail": {
                "message": error.to_string()
            }
        })
        .to_string()
    });

    if test_log_path_is_active() {
        return write_log_line(&path, &line);
    }

    enqueue_log_write(LogWrite { path, line })
}

fn enqueue_log_write(write: LogWrite) -> std::io::Result<()> {
    let sender = LOG_SENDER.get_or_init(start_log_writer);
    let path = write.path.clone();
    match sender.try_send(write) {
        Ok(()) => {
            flush_dropped_log_count(sender, &path);
            Ok(())
        }
        Err(TrySendError::Full(_)) => {
            DROPPED_LOG_WRITES.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Err(TrySendError::Disconnected(write)) => {
            write_log_line(&write.path, &write.line)?;
            let dropped = DROPPED_LOG_WRITES.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                let dropped_write = dropped_log_write(&write.path, dropped);
                write_log_line(&dropped_write.path, &dropped_write.line)?;
            }
            Ok(())
        }
    }
}

fn flush_dropped_log_count(sender: &SyncSender<LogWrite>, path: &PathBuf) {
    let dropped = DROPPED_LOG_WRITES.swap(0, Ordering::Relaxed);
    if dropped == 0 {
        return;
    }
    let write = dropped_log_write(path, dropped);
    match sender.try_send(write) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            DROPPED_LOG_WRITES.fetch_add(dropped + 1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(write)) => {
            let _ = write_log_line(&write.path, &write.line);
        }
    }
}

fn dropped_log_write(path: &PathBuf, dropped: u64) -> LogWrite {
    let record = DiagnosticRecord {
        timestamp_ms: now_ms(),
        pid: std::process::id(),
        event: "diagnostic_log.dropped".to_string(),
        detail: json!({ "dropped": dropped }),
    };
    let line = serde_json::to_string(&record).unwrap_or_else(|_| {
        format!(
            r#"{{"timestamp_ms":{},"pid":{},"event":"diagnostic_log.dropped","detail":{{"dropped":{}}}}}"#,
            now_ms(),
            std::process::id(),
            dropped
        )
    });
    LogWrite {
        path: path.clone(),
        line,
    }
}

fn start_log_writer() -> SyncSender<LogWrite> {
    let (tx, rx) = mpsc::sync_channel::<LogWrite>(2048);
    std::thread::Builder::new()
        .name("codex-elves-diagnostic-log".to_string())
        .spawn(move || {
            let mut current_path: Option<PathBuf> = None;
            let mut current_file: Option<std::io::BufWriter<std::fs::File>> = None;
            loop {
                let write = match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(write) => Some(write),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(file) = current_file.as_mut() {
                            let _ = file.flush();
                        }
                        None
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let Some(write) = write else {
                    continue;
                };
                if current_path.as_ref() != Some(&write.path) {
                    if let Some(file) = current_file.as_mut() {
                        let _ = file.flush();
                    }
                    current_file = open_log_file(&write.path).ok().map(std::io::BufWriter::new);
                    current_path = Some(write.path.clone());
                }
                if let Some(file) = current_file.as_mut() {
                    let _ = writeln!(file, "{}", write.line);
                }
            }
            if let Some(file) = current_file.as_mut() {
                let _ = file.flush();
            }
        })
        .ok();
    tx
}

fn write_log_line(path: &PathBuf, line: &str) -> std::io::Result<()> {
    let mut file = open_log_file(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn open_log_file(path: &PathBuf) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    Ok(file)
}

pub fn diagnostic_log_path() -> PathBuf {
    if let Some(lock) = TEST_LOG_PATH.get() {
        if let Ok(guard) = lock.lock() {
            if let Some(path) = &*guard {
                return path.clone();
            }
        }
    }
    crate::paths::default_diagnostic_log_path()
}

fn test_log_path_is_active() -> bool {
    TEST_LOG_PATH
        .get()
        .and_then(|lock| {
            lock.lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|_| ()))
        })
        .is_some()
}

#[doc(hidden)]
pub fn set_diagnostic_log_path_for_tests(path: Option<PathBuf>) {
    let lock = TEST_LOG_PATH.get_or_init(|| Mutex::new(None));
    *lock.lock().expect("test log path lock poisoned") = path;
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
