use super::{
    TaskBoardCreateCommand, TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult,
    parse_task_board_document, validate_task_board_document,
};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);
// T-002/T-003 consume this counter through the mutation seam below.
#[cfg_attr(not(test), allow(dead_code))]
static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

pub trait TaskBoardStore: Send + Sync {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError>;
    fn create_task(
        &self,
        command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>;
    fn move_task(
        &self,
        command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>;
}

#[derive(Clone, Debug)]
pub struct FileTaskBoardStore {
    document_path: PathBuf,
    lock_path: PathBuf,
    lock_timeout: Duration,
    lock_retry_interval: Duration,
}

impl FileTaskBoardStore {
    pub fn new(document_path: PathBuf, lock_path: PathBuf) -> Self {
        Self {
            document_path,
            lock_path,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
            lock_retry_interval: DEFAULT_LOCK_RETRY_INTERVAL,
        }
    }

    pub fn from_default_paths() -> Self {
        Self::new(
            crate::paths::default_task_board_path(),
            crate::paths::default_task_board_lock_path(),
        )
    }

    #[doc(hidden)]
    pub fn with_lock_timing(mut self, timeout: Duration, retry_interval: Duration) -> Self {
        self.lock_timeout = timeout;
        self.lock_retry_interval = retry_interval;
        self
    }

    pub fn document_path(&self) -> &Path {
        &self.document_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    fn open_lock_file(&self) -> Result<File, TaskBoardStoreError> {
        if let Some(parent) = self
            .lock_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| TaskBoardStoreError::Unavailable {
                path: self.lock_path.clone(),
                message: error.to_string(),
            })?;
        }
        OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .map_err(|error| TaskBoardStoreError::Unavailable {
                path: self.lock_path.clone(),
                message: error.to_string(),
            })
    }

    fn acquire_lock(
        &self,
        mode: TaskBoardLockMode,
    ) -> Result<TaskBoardFileLock, TaskBoardStoreError> {
        let file = self.open_lock_file()?;
        let deadline = Instant::now()
            .checked_add(self.lock_timeout)
            .unwrap_or_else(Instant::now);
        let result = retry_lock_until(
            deadline,
            self.lock_retry_interval,
            || match mode {
                TaskBoardLockMode::Shared => FileExt::try_lock_shared(&file),
                TaskBoardLockMode::Exclusive => FileExt::try_lock_exclusive(&file),
            },
            Instant::now,
            |sleep_for| {
                if sleep_for.is_zero() {
                    thread::yield_now();
                } else {
                    thread::sleep(sleep_for);
                }
            },
        );
        match result {
            Ok(()) => Ok(TaskBoardFileLock { file }),
            Err(LockRetryError::Busy) => Err(TaskBoardStoreError::Busy),
            Err(LockRetryError::AcquiredAfterDeadline) => {
                let _ = FileExt::unlock(&file);
                Err(TaskBoardStoreError::Busy)
            }
            Err(LockRetryError::Io(error)) => Err(TaskBoardStoreError::Unavailable {
                path: self.lock_path.clone(),
                message: error.to_string(),
            }),
        }
    }

    fn read_document_unlocked(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        let bytes = match std::fs::read(&self.document_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TaskBoardDocument::empty());
            }
            Err(error) => {
                return Err(TaskBoardStoreError::Unavailable {
                    path: self.document_path.clone(),
                    message: error.to_string(),
                });
            }
        };
        parse_task_board_document(&bytes).map_err(|error| TaskBoardStoreError::InvalidFile {
            path: self.document_path.clone(),
            message: error.to_string(),
        })
    }

    /// Low-level validated mutation seam consumed by the independently owned T-002/T-003 modules.
    #[doc(hidden)]
    pub fn with_exclusive_document<F>(
        &self,
        mutation: F,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>
    where
        F: FnOnce(TaskBoardDocument) -> Result<TaskBoardMutationResult, TaskBoardStoreError>,
    {
        let _lock = self.acquire_lock(TaskBoardLockMode::Exclusive)?;
        let current = self.read_document_unlocked()?;
        let mut result = mutation(current.clone())?;

        if !result.changed {
            if result.document != current {
                return Err(TaskBoardStoreError::InvalidInput {
                    message: "an unchanged mutation must return the current snapshot".to_string(),
                });
            }
            return Ok(result);
        }

        let expected_revision =
            current
                .revision
                .checked_add(1)
                .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                    message: "task board revision cannot be incremented".to_string(),
                })?;
        if result.document.revision != expected_revision {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "a changed mutation must increment revision exactly once".to_string(),
            });
        }
        validate_task_board_document(&mut result.document).map_err(|error| {
            TaskBoardStoreError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        self.atomic_replace_document(&result.document)?;
        Ok(result)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn atomic_replace_document(
        &self,
        document: &TaskBoardDocument,
    ) -> Result<(), TaskBoardStoreError> {
        self.atomic_replace_document_with_parent_sync(document, sync_parent_directory)
    }

    fn atomic_replace_document_with_parent_sync<F>(
        &self,
        document: &TaskBoardDocument,
        sync_parent: F,
    ) -> Result<(), TaskBoardStoreError>
    where
        F: FnOnce(&Path) -> std::io::Result<()>,
    {
        let mut bytes = serde_json::to_vec_pretty(document).map_err(|error| {
            TaskBoardStoreError::Unavailable {
                path: self.document_path.clone(),
                message: error.to_string(),
            }
        })?;
        bytes.push(b'\n');

        let parent = self
            .document_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| TaskBoardStoreError::Unavailable {
            path: self.document_path.clone(),
            message: error.to_string(),
        })?;

        let temp_path = parent.join(format!(
            ".{}.{}.{}.tmp",
            self.document_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("task-board.json"),
            std::process::id(),
            NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let write_result = (|| -> std::io::Result<()> {
            let mut temp_file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)?;
            temp_file.write_all(&bytes)?;
            temp_file.flush()?;
            temp_file.sync_all()?;
            std::fs::rename(&temp_path, &self.document_path)?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TaskBoardStoreError::Unavailable {
                path: self.document_path.clone(),
                message: error.to_string(),
            });
        }
        let _ = sync_parent(parent);
        Ok(())
    }
}

impl Default for FileTaskBoardStore {
    fn default() -> Self {
        Self::from_default_paths()
    }
}

impl TaskBoardStore for FileTaskBoardStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        let _lock = self.acquire_lock(TaskBoardLockMode::Shared)?;
        self.read_document_unlocked()
    }

    fn create_task(
        &self,
        command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        super::create::create_task(self, command)
    }

    fn move_task(
        &self,
        command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        super::move_task::move_task(self, command)
    }
}

struct TaskBoardFileLock {
    file: File,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
enum TaskBoardLockMode {
    Shared,
    Exclusive,
}

impl Drop for TaskBoardFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn lock_is_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock || matches!(error.raw_os_error(), Some(32 | 33))
}

enum LockRetryError {
    Busy,
    AcquiredAfterDeadline,
    Io(std::io::Error),
}

fn retry_lock_until<TryLock, Now, Sleep>(
    deadline: Instant,
    retry_interval: Duration,
    mut try_lock: TryLock,
    mut now: Now,
    mut sleep: Sleep,
) -> Result<(), LockRetryError>
where
    TryLock: FnMut() -> std::io::Result<()>,
    Now: FnMut() -> Instant,
    Sleep: FnMut(Duration),
{
    let mut is_retry = false;
    loop {
        if is_retry && now() >= deadline {
            return Err(LockRetryError::Busy);
        }
        match try_lock() {
            Ok(()) => {
                if is_retry && now() >= deadline {
                    return Err(LockRetryError::AcquiredAfterDeadline);
                }
                return Ok(());
            }
            Err(error) if lock_is_contended(&error) => {
                let current = now();
                if current >= deadline {
                    return Err(LockRetryError::Busy);
                }
                sleep(retry_interval.min(deadline.saturating_duration_since(current)));
                is_retry = true;
            }
            Err(error) => return Err(LockRetryError::Io(error)),
        }
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
#[cfg_attr(not(test), allow(dead_code))]
fn sync_parent_directory(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}

#[derive(Debug, Error)]
pub enum TaskBoardStoreError {
    #[error("task board lock is busy")]
    Busy,
    #[error("task board file {path} is invalid: {message}", path = path.display())]
    InvalidFile { path: PathBuf, message: String },
    #[error("task board input is invalid: {message}")]
    InvalidInput { message: String },
    #[error("task board revision conflicts with the current snapshot")]
    RevisionConflict { current: TaskBoardDocument },
    #[error("task id conflicts with an existing task")]
    TaskIdConflict,
    #[error("task was not found")]
    TaskNotFound,
    #[error("task board storage {path} is unavailable: {message}", path = path.display())]
    Unavailable { path: PathBuf, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn exclusive_mutation_atomically_replaces_an_existing_document() {
        let temp = tempfile::tempdir().unwrap();
        let document_path = temp.path().join("task-board.json");
        let lock_path = temp.path().join("task-board.lock");
        std::fs::write(
            &document_path,
            serde_json::to_vec_pretty(&TaskBoardDocument::empty()).unwrap(),
        )
        .unwrap();
        let store = FileTaskBoardStore::new(document_path.clone(), lock_path);

        let result = store
            .with_exclusive_document(|mut current| {
                current.revision = 1;
                Ok(TaskBoardMutationResult {
                    document: current,
                    changed: true,
                    idempotent: false,
                })
            })
            .unwrap();

        assert_eq!(result.document.revision, 1);
        assert_eq!(
            parse_task_board_document(&std::fs::read(document_path).unwrap())
                .unwrap()
                .revision,
            1
        );
    }

    #[test]
    fn lock_retry_never_attempts_again_after_the_deadline() {
        let started_at = Instant::now();
        let now = Cell::new(started_at);
        let attempts = Cell::new(0usize);

        let result = retry_lock_until(
            started_at + Duration::from_millis(10),
            Duration::from_millis(10),
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 1 {
                    Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
                } else {
                    Ok(())
                }
            },
            || now.get(),
            |duration| now.set(now.get() + duration),
        );

        assert!(matches!(result, Err(LockRetryError::Busy)));
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn parent_sync_failure_after_rename_is_best_effort() {
        let temp = tempfile::tempdir().unwrap();
        let document_path = temp.path().join("task-board.json");
        let lock_path = temp.path().join("task-board.lock");
        std::fs::write(
            &document_path,
            serde_json::to_vec_pretty(&TaskBoardDocument::empty()).unwrap(),
        )
        .unwrap();
        let store = FileTaskBoardStore::new(document_path.clone(), lock_path);
        let mut replacement = TaskBoardDocument::empty();
        replacement.revision = 1;

        let result = store.atomic_replace_document_with_parent_sync(&replacement, |_| {
            Err(std::io::Error::other("injected parent sync failure"))
        });

        assert!(result.is_ok());
        assert_eq!(
            parse_task_board_document(&std::fs::read(document_path).unwrap())
                .unwrap()
                .revision,
            1
        );
    }
}
