use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
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
    let line = bound_diagnostic_line(event, line);

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

fn bound_diagnostic_line(event: &str, line: String) -> String {
    if line.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES {
        return line;
    }

    let record = DiagnosticRecord {
        timestamp_ms: now_ms(),
        pid: std::process::id(),
        event: "diagnostic_log.entry_too_large".to_string(),
        detail: json!({
            "original_event": truncate_for_diagnostic_summary(event, 1024),
            "original_bytes": line.len()
        }),
    };
    serde_json::to_string(&record).unwrap_or_else(|_| {
        format!(
            r#"{{"timestamp_ms":{},"pid":{},"event":"diagnostic_log.entry_too_large","detail":{{"original_bytes":{}}}}}"#,
            now_ms(),
            std::process::id(),
            line.len()
        )
    })
}

fn truncate_for_diagnostic_summary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
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
                let mut batch = vec![write];
                while batch.len() < 64 {
                    match rx.try_recv() {
                        Ok(write) => batch.push(write),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                let mut index = 0;
                while index < batch.len() {
                    let path = batch[index].path.clone();
                    let end = batch[index..]
                        .iter()
                        .position(|write| write.path != path)
                        .map_or(batch.len(), |offset| index + offset);
                    if current_path.as_ref() != Some(&path) {
                        if let Some(file) = current_file.as_mut() {
                            let _ = file.flush();
                        }
                        current_file = open_log_file(&path).ok().map(std::io::BufWriter::new);
                        current_path = Some(path);
                    }
                    let Some(file) = current_file.as_mut() else {
                        index = end;
                        continue;
                    };
                    let result = (|| -> std::io::Result<()> {
                        file.flush()?;
                        lock_log_file(file.get_mut())?;
                        let incoming_bytes = batch[index..end]
                            .iter()
                            .map(|write| write.line.len() as u64 + 1)
                            .sum();
                        crate::log_limits::clear_if_append_would_exceed(
                            file.get_mut(),
                            incoming_bytes,
                        )?;
                        for write in &batch[index..end] {
                            writeln!(file, "{}", write.line)?;
                        }
                        file.flush()?;
                        file.get_mut().unlock()?;
                        Ok(())
                    })();
                    if result.is_err() {
                        let _ = file.get_mut().unlock();
                        current_file = None;
                        current_path = None;
                    }
                    index = end;
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
    lock_log_file(&file)?;
    crate::log_limits::clear_if_append_would_exceed(&mut file, line.len() as u64 + 1)?;
    writeln!(file, "{line}")?;
    file.unlock()?;
    Ok(())
}

fn lock_log_file(file: &std::fs::File) -> std::io::Result<()> {
    let mut last_error = None;
    for _ in 0..500 {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("diagnostic log lock failed")))
}

fn open_log_file(path: &PathBuf) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
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

#[cfg(test)]
mod tests {
    #[test]
    fn write_log_line_clears_existing_log_when_append_would_exceed_limit() {
        let path = std::env::temp_dir().join(format!(
            "codex-elves-diagnostic-log-limit-{}-{}.log",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::write(
            &path,
            vec![b'x'; crate::log_limits::MAX_LOG_FILE_BYTES as usize - 2],
        )
        .expect("seed oversized diagnostic log");

        super::write_log_line(&path, "small").expect("append diagnostic log line");

        let metadata = std::fs::metadata(&path).expect("read diagnostic log metadata");
        assert!(metadata.len() <= crate::log_limits::MAX_LOG_FILE_BYTES);
        let text = std::fs::read_to_string(&path).expect("read diagnostic log");
        assert_eq!(text, "small\n");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_diagnostic_entry_is_replaced_with_small_summary() {
        let line = "x".repeat(crate::log_limits::MAX_LOG_FILE_BYTES as usize + 1);
        let bounded = super::bound_diagnostic_line("renderer.large_payload", line);

        assert!(bounded.len() as u64 + 1 <= crate::log_limits::MAX_LOG_FILE_BYTES);
        assert!(bounded.contains("diagnostic_log.entry_too_large"));
        assert!(bounded.contains("renderer.large_payload"));
    }
}
