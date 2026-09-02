use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const APP_STATE_DIR: &str = ".codex-session-delete";
const SETTINGS_FILE: &str = "settings.json";
const LATEST_STATUS_FILE: &str = "latest-status.json";
const DIAGNOSTIC_LOG_FILE: &str = "codex-elves.log";
const PROXY_LOG_FILE: &str = "proxy-requests.jsonl";
const SUPPRESSED_THREADS_FILE: &str = "suppressed-threads.json";
const SKINS_FILE: &str = "skins.json";
const TASK_BOARD_FILE: &str = "task-board.json";
const TASK_BOARD_LOCK_FILE: &str = "task-board.lock";
const WORKSPACE_CHECKPOINTS_DIR: &str = "workspace-checkpoints";
const OBSOLETE_SESSION_BACKUPS_DIR: &str = "backups";
const MIGRATIONS_DIR: &str = "migrations";
const SESSION_BACKUP_CLEANUP_MARKER: &str = "session-backups-v1.done";
const SESSION_BACKUP_CLEANUP_LOCK: &str = "session-backups-v1.lock";

pub fn default_app_state_dir() -> PathBuf {
    if let Some(path) = app_state_dir_for_tests() {
        return path;
    }
    if let Some(home_dir) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
        return home_dir.join(APP_STATE_DIR);
    }

    PathBuf::from(APP_STATE_DIR)
}

pub fn default_settings_path() -> PathBuf {
    if let Some(path) = settings_path_for_tests() {
        return path;
    }
    default_app_state_dir().join(SETTINGS_FILE)
}

pub fn default_latest_status_path() -> PathBuf {
    default_app_state_dir().join(LATEST_STATUS_FILE)
}

pub fn default_diagnostic_log_path() -> PathBuf {
    default_app_state_dir().join(DIAGNOSTIC_LOG_FILE)
}

pub fn default_suppressed_threads_path() -> PathBuf {
    default_app_state_dir().join(SUPPRESSED_THREADS_FILE)
}

pub fn default_skins_path() -> PathBuf {
    if let Some(path) = skins_path_for_tests() {
        return path;
    }
    default_app_state_dir().join(SKINS_FILE)
}

pub fn default_task_board_path() -> PathBuf {
    default_app_state_dir().join(TASK_BOARD_FILE)
}

pub fn default_task_board_lock_path() -> PathBuf {
    default_app_state_dir().join(TASK_BOARD_LOCK_FILE)
}

pub fn default_workspace_checkpoints_dir() -> PathBuf {
    default_app_state_dir().join(WORKSPACE_CHECKPOINTS_DIR)
}

pub fn default_proxy_log_path() -> PathBuf {
    if let Some(path) = proxy_log_path_for_tests() {
        return path;
    }
    default_app_state_dir().join(PROXY_LOG_FILE)
}

pub fn obsolete_session_backup_cleanup_needed() -> bool {
    obsolete_session_backup_cleanup_needed_at(&default_app_state_dir())
}

pub fn cleanup_obsolete_session_backups() -> anyhow::Result<usize> {
    cleanup_obsolete_session_backups_at(&default_app_state_dir())
}

fn obsolete_session_backup_cleanup_needed_at(app_state_dir: &std::path::Path) -> bool {
    !session_backup_cleanup_marker_path_from(app_state_dir).is_file()
}

fn cleanup_obsolete_session_backups_at(app_state_dir: &std::path::Path) -> anyhow::Result<usize> {
    let marker_path = session_backup_cleanup_marker_path_from(&app_state_dir);
    if marker_path.is_file() {
        return Ok(0);
    }
    let migrations_dir = app_state_dir.join(MIGRATIONS_DIR);
    std::fs::create_dir_all(&migrations_dir)?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(migrations_dir.join(SESSION_BACKUP_CLEANUP_LOCK))?;
    lock_file.lock_exclusive()?;
    if marker_path.is_file() {
        return Ok(0);
    }

    let removed =
        cleanup_obsolete_session_backup_files(&app_state_dir.join(OBSOLETE_SESSION_BACKUPS_DIR))?;
    std::fs::write(marker_path, b"completed\n")?;
    Ok(removed)
}

fn session_backup_cleanup_marker_path_from(app_state_dir: &std::path::Path) -> PathBuf {
    app_state_dir
        .join(MIGRATIONS_DIR)
        .join(SESSION_BACKUP_CLEANUP_MARKER)
}

fn cleanup_obsolete_session_backup_files(backups_dir: &std::path::Path) -> anyhow::Result<usize> {
    let mut removed = 0usize;
    let entries = match std::fs::read_dir(backups_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if obsolete_session_backup_filename(name) {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    match std::fs::read_dir(backups_dir) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                if let Err(error) = std::fs::remove_dir(backups_dir) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        return Err(error.into());
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(removed)
}

fn obsolete_session_backup_filename(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else {
        return false;
    };
    let Some((epoch, token)) = stem.split_once('-') else {
        return false;
    };
    !epoch.is_empty()
        && epoch.bytes().all(|byte| byte.is_ascii_digit())
        && token.len() == 32
        && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn settings_path_for_tests() -> Option<PathBuf> {
    SETTINGS_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|path| path.clone())
}

static SETTINGS_PATH_FOR_TESTS: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static APP_STATE_DIR_FOR_TESTS: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static SKINS_PATH_FOR_TESTS: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static PROXY_LOG_PATH_FOR_TESTS: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn app_state_dir_for_tests() -> Option<PathBuf> {
    APP_STATE_DIR_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|path| path.clone())
}

pub fn set_app_state_dir_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    APP_STATE_DIR_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut current| std::mem::replace(&mut *current, path))
}

pub fn set_settings_path_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    SETTINGS_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut current| std::mem::replace(&mut *current, path))
}

fn skins_path_for_tests() -> Option<PathBuf> {
    SKINS_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|path| path.clone())
}

pub fn set_skins_path_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    SKINS_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut current| std::mem::replace(&mut *current, path))
}

pub(crate) fn proxy_log_path_for_tests() -> Option<PathBuf> {
    PROXY_LOG_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|path| path.clone())
}

pub fn set_proxy_log_path_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    PROXY_LOG_PATH_FOR_TESTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|mut current| std::mem::replace(&mut *current, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_path_uses_app_state_directory() {
        let path = default_settings_path();

        assert!(path.ends_with(".codex-session-delete/settings.json"));
    }

    #[test]
    fn default_latest_status_path_uses_app_state_directory() {
        let path = default_latest_status_path();

        assert!(path.ends_with(".codex-session-delete/latest-status.json"));
    }

    #[test]
    fn default_diagnostic_log_path_uses_app_state_directory() {
        let path = default_diagnostic_log_path();

        assert!(path.ends_with(".codex-session-delete/codex-elves.log"));
    }

    #[test]
    fn default_proxy_log_path_uses_app_state_directory() {
        let path = default_proxy_log_path();

        assert!(path.ends_with(".codex-session-delete/proxy-requests.jsonl"));
    }

    #[test]
    fn cleanup_obsolete_session_backups_runs_once_and_preserves_other_files() {
        let temp = tempfile::tempdir().unwrap();
        let app_state_dir = temp.path().join(".codex-session-delete");
        let backups_dir = app_state_dir.join("backups");
        std::fs::create_dir_all(&backups_dir).unwrap();
        let obsolete = backups_dir.join("1787048785-da1547faf8834dc192a160c1072b7890.json");
        let unrelated = backups_dir.join("env-conflicts-1787048785000.json");
        std::fs::write(&obsolete, "{}").unwrap();
        std::fs::write(&unrelated, "{}").unwrap();
        std::fs::write(app_state_dir.join(SETTINGS_FILE), "{}").unwrap();
        let removed = cleanup_obsolete_session_backups_at(&app_state_dir).unwrap();

        assert_eq!(removed, 1);
        assert!(!obsolete.exists());
        assert!(unrelated.exists());
        assert!(backups_dir.exists());
        assert!(app_state_dir.join(SETTINGS_FILE).exists());
        assert!(!obsolete_session_backup_cleanup_needed_at(&app_state_dir));

        let later_backup = backups_dir.join("1787048786-11111111111111111111111111111111.json");
        std::fs::write(&later_backup, "{}").unwrap();
        assert_eq!(
            cleanup_obsolete_session_backups_at(&app_state_dir).unwrap(),
            0
        );
        assert!(
            later_backup.exists(),
            "完成标记存在后不应再次扫描和删除备份目录"
        );
    }

    #[test]
    fn cleanup_obsolete_session_backups_retries_after_failed_migration() {
        let temp = tempfile::tempdir().unwrap();
        let app_state_dir = temp.path().join(".codex-session-delete");
        let backups_path = app_state_dir.join("backups");
        std::fs::create_dir_all(&app_state_dir).unwrap();
        std::fs::write(&backups_path, "not a directory").unwrap();

        assert!(cleanup_obsolete_session_backups_at(&app_state_dir).is_err());
        assert!(obsolete_session_backup_cleanup_needed_at(&app_state_dir));

        std::fs::remove_file(&backups_path).unwrap();
        std::fs::create_dir_all(&backups_path).unwrap();
        let obsolete = backups_path.join("1787048785-da1547faf8834dc192a160c1072b7890.json");
        std::fs::write(&obsolete, "{}").unwrap();

        assert_eq!(
            cleanup_obsolete_session_backups_at(&app_state_dir).unwrap(),
            1
        );
        assert!(!obsolete.exists());
        assert!(!obsolete_session_backup_cleanup_needed_at(&app_state_dir));
    }
}
