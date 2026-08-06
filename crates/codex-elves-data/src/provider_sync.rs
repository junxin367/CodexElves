use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, Transaction, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DEFAULT_PROVIDER: &str = "openai";
const SESSION_DIRS: [&str; 2] = ["sessions", "archived_sessions"];
const BACKUP_KEEP_COUNT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSyncStatus {
    Disabled,
    Skipped,
    Synced,
    Partial,
    Blocked,
    RecoveryRequired,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    User,
    Subagent,
    Automation,
    NonInteractive,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAnomalyKind {
    EmptyFile,
    InvalidUtf8,
    MalformedJson,
    MissingSessionMeta,
    InvalidSessionMeta,
    ConflictingThreadIds,
    DuplicateThreadId,
    ConcurrentModification,
    Locked,
    MissingIndex,
    UnsupportedIndexSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRepairIssue {
    pub path: PathBuf,
    pub thread_id: Option<String>,
    pub kind: SessionAnomalyKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSyncResult {
    pub status: ProviderSyncStatus,
    pub message: String,
    pub error_code: Option<String>,
    pub target_provider: String,
    pub backup_dir: Option<PathBuf>,
    pub active_db_path: Option<PathBuf>,
    pub operation_id: Option<String>,
    pub scanned_session_files: usize,
    pub changed_session_files: usize,
    pub skipped_locked_rollout_files: Vec<PathBuf>,
    pub sqlite_rows_updated: usize,
    pub sqlite_rows_inserted: usize,
    pub sqlite_provider_rows_updated: usize,
    pub sqlite_user_event_rows_updated: usize,
    pub sqlite_cwd_rows_updated: usize,
    pub updated_workspace_roots: usize,
    pub encrypted_content_warning: Option<String>,
    pub issues: Vec<SessionRepairIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncAudit {
    pub target_provider: String,
    pub scanned_session_files: usize,
    pub user_sessions: usize,
    pub subagent_sessions: usize,
    pub automation_sessions: usize,
    pub other_sessions: usize,
    pub active_db_path: Option<PathBuf>,
    pub sqlite_rows_to_update: usize,
    pub sqlite_rows_to_insert: usize,
    pub stale_lock_detected: bool,
    pub issues: Vec<SessionRepairIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSyncTargetSource {
    Config,
    Rollout,
    Sqlite,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncTargetOption {
    pub id: String,
    pub sources: Vec<ProviderSyncTargetSource>,
    pub is_current_provider: bool,
    pub is_manual: bool,
    pub is_saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSyncTargetList {
    pub current_provider: String,
    pub targets: Vec<ProviderSyncTargetOption>,
}

#[derive(Debug, Clone)]
struct SessionChange {
    path: PathBuf,
    original_text: String,
    next_text: String,
    original_session_meta_lines: Vec<String>,
    thread_id: Option<String>,
    cwd: Option<String>,
    has_user_event: bool,
    kind: SessionKind,
    rewrite_needed: bool,
    original_mtime: Option<SystemTime>,
    original_len: u64,
    index_record: Option<SessionIndexRecord>,
}

#[derive(Debug, Default)]
struct RolloutRewrite {
    next_text: String,
    rewrite_needed: bool,
    thread_id: Option<String>,
    cwd: Option<String>,
    providers: Vec<String>,
    original_session_meta_lines: Vec<String>,
    session_meta_count: usize,
    session_meta_payload: Option<Map<String, Value>>,
    session_meta_payloads: HashMap<String, Map<String, Value>>,
    cwd_by_thread_id: HashMap<String, String>,
    first_turn_context: Option<Map<String, Value>>,
    first_user_message: Option<String>,
    has_user_event: bool,
    created_timestamp_ms: Option<i64>,
    latest_timestamp_ms: Option<i64>,
    thread_ids: HashSet<String>,
    malformed_json_lines: usize,
}

#[derive(Debug, Default)]
struct SessionChanges {
    changes: Vec<SessionChange>,
    scanned_session_files: usize,
    skipped_locked_rollout_files: Vec<PathBuf>,
    encrypted_content_counts: HashMap<String, usize>,
    issues: Vec<SessionRepairIssue>,
}

#[derive(Debug, Default)]
struct AppliedSessionChanges {
    changes: Vec<SessionChange>,
    skipped_locked_rollout_files: Vec<PathBuf>,
    issues: Vec<SessionRepairIssue>,
}

#[derive(Debug, Default)]
struct SqliteUpdateCounts {
    provider_rows: usize,
    user_event_rows: usize,
    cwd_rows: usize,
    inserted_rows: usize,
}

impl SqliteUpdateCounts {
    fn total(&self) -> usize {
        self.provider_rows + self.user_event_rows + self.cwd_rows + self.inserted_rows
    }
}

#[derive(Debug, Clone)]
struct SessionIndexRecord {
    thread_id: String,
    rollout_path: PathBuf,
    created_at: i64,
    updated_at: i64,
    source: String,
    thread_source: String,
    cwd: String,
    title: String,
    first_user_message: String,
    sandbox_policy: String,
    approval_mode: String,
    cli_version: String,
    archived: bool,
}

#[derive(Debug, Clone)]
struct ActiveDb {
    path: PathBuf,
    columns: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DbEvidence {
    migration_version: i64,
    rollout_overlap: i64,
    latest_activity_ms: i64,
    file_activity_ms: i64,
    row_count: i64,
    schema_columns: usize,
}

#[derive(Debug, Clone)]
struct DbCandidate {
    active_db: ActiveDb,
    evidence: DbEvidence,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSyncLockOwner {
    pid: u32,
    started_at: u64,
    token: Option<String>,
}

struct ProviderSyncLockGuard {
    path: PathBuf,
    token: String,
    _lease: ProviderSyncLeaseGuard,
}

struct ProviderSyncLeaseGuard {
    file: fs::File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationStatus {
    Prepared,
    FilesApplied,
    GlobalStateApplied,
    DbCommitted,
    Committed,
    RollingBack,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationJournal {
    version: u32,
    operation_id: String,
    status: OperationStatus,
    updated_at: String,
    error: Option<String>,
}

impl Drop for ProviderSyncLockGuard {
    fn drop(&mut self) {
        let owner_path = self.path.join("owner.json");
        let owns_lock = fs::read_to_string(owner_path)
            .ok()
            .and_then(|text| serde_json::from_str::<ProviderSyncLockOwner>(&text).ok())
            .and_then(|owner| owner.token)
            .is_some_and(|token| token == self.token);
        if owns_lock {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl Drop for ProviderSyncLeaseGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub fn run_provider_sync(codex_home: Option<&Path>) -> ProviderSyncResult {
    run_provider_sync_with_target(codex_home, None)
}

pub fn audit_provider_sync(
    codex_home: Option<&Path>,
    explicit_target_provider: Option<&str>,
) -> anyhow::Result<ProviderSyncAudit> {
    let home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs_home().join(".codex"));
    if !home.exists() {
        anyhow::bail!("Codex home not found: {}", home.to_string_lossy());
    }
    let target_provider =
        resolve_target_provider(&home.join("config.toml"), explicit_target_provider)
            .map_err(anyhow::Error::msg)?;
    let collected = collect_session_changes(&home, &target_provider)?;
    let rollout_thread_ids = collected
        .changes
        .iter()
        .filter_map(|change| change.thread_id.clone())
        .collect::<HashSet<_>>();
    let active_db = resolve_active_db(&home, &rollout_thread_ids)?;
    let user_event_thread_ids = collected
        .changes
        .iter()
        .filter(|change| change.kind == SessionKind::User && change.has_user_event)
        .filter_map(|change| change.thread_id.clone())
        .collect::<HashSet<_>>();
    let cwd_by_thread_id = collected
        .changes
        .iter()
        .filter(|change| change.kind == SessionKind::User)
        .filter_map(|change| Some((change.thread_id.clone()?, change.cwd.clone()?)))
        .collect::<HashMap<_, _>>();
    let sqlite_updates = count_sqlite_updates(
        active_db.as_ref(),
        &target_provider,
        &user_event_thread_ids,
        &cwd_by_thread_id,
        &collected.changes,
    )?;
    let mut user_sessions = 0;
    let mut subagent_sessions = 0;
    let mut automation_sessions = 0;
    let mut other_sessions = 0;
    for change in &collected.changes {
        match change.kind {
            SessionKind::User => user_sessions += 1,
            SessionKind::Subagent => subagent_sessions += 1,
            SessionKind::Automation => automation_sessions += 1,
            SessionKind::NonInteractive | SessionKind::Unknown => other_sessions += 1,
        }
    }
    Ok(ProviderSyncAudit {
        target_provider,
        scanned_session_files: collected.scanned_session_files,
        user_sessions,
        subagent_sessions,
        automation_sessions,
        other_sessions,
        active_db_path: active_db.map(|db| db.path),
        sqlite_rows_to_update: sqlite_updates.total() - sqlite_updates.inserted_rows,
        sqlite_rows_to_insert: sqlite_updates.inserted_rows,
        stale_lock_detected: stale_lock_owner_is_dead(&home.join("tmp/provider-sync.lock")),
        issues: collected.issues,
    })
}

pub fn run_provider_sync_with_target_guarded(
    codex_home: Option<&Path>,
    explicit_target_provider: Option<&str>,
) -> ProviderSyncResult {
    let process_ids = codex_elves_core::watcher::find_codex_processes();
    if !process_ids.is_empty() {
        let target_provider = codex_home
            .map(|home| read_current_provider(&home.join("config.toml")))
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        return error_result(
            ProviderSyncStatus::Blocked,
            format!(
                "检测到 Codex/ChatGPT 正在运行（PID：{}），请完全关闭后重试",
                process_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "codex_running",
            explicit_target_provider.unwrap_or(&target_provider),
        );
    }
    run_provider_sync_with_target(codex_home, explicit_target_provider)
}

pub fn run_provider_sync_with_target(
    codex_home: Option<&Path>,
    explicit_target_provider: Option<&str>,
) -> ProviderSyncResult {
    let home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs_home().join(".codex"));
    if !home.exists() {
        return error_result(
            ProviderSyncStatus::Blocked,
            format!("Codex home not found: {}", home.to_string_lossy()),
            "codex_home_not_found",
            DEFAULT_PROVIDER,
        );
    }
    let target_provider =
        match resolve_target_provider(&home.join("config.toml"), explicit_target_provider) {
            Ok(provider) => provider,
            Err(message) => {
                return error_result(
                    ProviderSyncStatus::Blocked,
                    message,
                    "invalid_target_provider",
                    DEFAULT_PROVIDER,
                );
            }
        };
    let lock_dir = home.join("tmp/provider-sync.lock");
    let _lock = match acquire_lock(&lock_dir) {
        Ok(lock) => lock,
        Err(error) => {
            return error_result(
                ProviderSyncStatus::Blocked,
                error.to_string(),
                "provider_sync_locked",
                &target_provider,
            );
        }
    };
    if let Err(error) = recover_incomplete_operations(&home) {
        return error_result(
            ProviderSyncStatus::RecoveryRequired,
            format!("Provider sync recovery is required: {error}"),
            "provider_sync_recovery_required",
            &target_provider,
        );
    }
    let sync_result = (|| -> anyhow::Result<ProviderSyncResult> {
        let mut collected = collect_session_changes(&home, &target_provider)?;
        let encrypted_content_warning =
            build_encrypted_content_warning(&collected.encrypted_content_counts, &target_provider);
        let rewrite_changes = collected
            .changes
            .iter()
            .filter(|change| change.rewrite_needed)
            .cloned()
            .collect::<Vec<_>>();
        let thread_ids_with_user_events = collected
            .changes
            .iter()
            .filter(|change| change.kind == SessionKind::User && change.has_user_event)
            .filter_map(|change| change.thread_id.clone())
            .collect::<HashSet<_>>();
        let projectless_thread_ids =
            load_projectless_thread_ids(&home.join(".codex-global-state.json"))?;
        let cwd_by_thread_id = collected
            .changes
            .iter()
            .filter(|change| change.kind == SessionKind::User)
            .filter_map(|change| Some((change.thread_id.clone()?, change.cwd.clone()?)))
            .filter(|(thread_id, _)| !projectless_thread_ids.contains(thread_id))
            .collect::<HashMap<_, _>>();
        let rollout_thread_ids = collected
            .changes
            .iter()
            .filter_map(|change| change.thread_id.clone())
            .collect::<HashSet<_>>();
        let active_db = resolve_active_db(&home, &rollout_thread_ids)?;
        let sqlite_update_count = count_sqlite_updates(
            active_db.as_ref(),
            &target_provider,
            &thread_ids_with_user_events,
            &cwd_by_thread_id,
            &collected.changes,
        )?;
        let global_state_update_count =
            count_global_state_updates(&home.join(".codex-global-state.json"))?;
        if rewrite_changes.is_empty()
            && sqlite_update_count.total() == 0
            && global_state_update_count == 0
        {
            let mut synced = result(
                if collected.issues.is_empty() && collected.skipped_locked_rollout_files.is_empty()
                {
                    ProviderSyncStatus::Synced
                } else {
                    ProviderSyncStatus::Partial
                },
                "Provider sync already up to date",
                &target_provider,
                None,
                0,
                0,
            );
            synced.active_db_path = active_db.map(|db| db.path);
            synced.scanned_session_files = collected.scanned_session_files;
            synced.skipped_locked_rollout_files = collected.skipped_locked_rollout_files;
            synced.encrypted_content_warning = encrypted_content_warning;
            synced.issues = collected.issues;
            return Ok(synced);
        }
        validate_insert_schema(active_db.as_ref(), &collected.changes)?;
        let operation_id = Uuid::new_v4().to_string();
        let backup_dir = create_backup(&home, &target_provider, &operation_id, &rewrite_changes)?;
        let global_state_path = home.join(".codex-global-state.json");
        let global_state_backup_path = home.join(".codex-global-state.json.bak");
        let original_global_state = read_optional_bytes(&global_state_path)?;
        let original_global_state_backup = read_optional_bytes(&global_state_backup_path)?;
        let applied = apply_session_changes(&rewrite_changes)?;
        collected.issues.extend(applied.issues.clone());
        let apply_result = (|| -> anyhow::Result<(SqliteUpdateCounts, usize)> {
            write_operation_status(
                &backup_dir,
                &operation_id,
                OperationStatus::FilesApplied,
                None,
            )?;
            let updated_workspace_roots = apply_global_state_update(&global_state_path)?;
            write_operation_status(
                &backup_dir,
                &operation_id,
                OperationStatus::GlobalStateApplied,
                None,
            )?;
            let sqlite_updates = apply_sqlite_update(
                active_db.as_ref(),
                &target_provider,
                &thread_ids_with_user_events,
                &cwd_by_thread_id,
                &collected.changes,
            )?;
            write_operation_status(
                &backup_dir,
                &operation_id,
                OperationStatus::DbCommitted,
                None,
            )?;
            Ok((sqlite_updates, updated_workspace_roots))
        })();
        let (sqlite_updates, updated_workspace_roots) = match apply_result {
            Ok(counts) => counts,
            Err(err) => {
                let _ = write_operation_status(
                    &backup_dir,
                    &operation_id,
                    OperationStatus::RollingBack,
                    Some(err.to_string()),
                );
                let session_restore = restore_session_changes(&applied.changes);
                let state_restore =
                    restore_optional_bytes(&global_state_path, original_global_state.as_deref())
                        .and_then(|_| {
                            restore_optional_bytes(
                                &global_state_backup_path,
                                original_global_state_backup.as_deref(),
                            )
                        });
                return match (session_restore, state_restore) {
                    (Ok(()), Ok(())) => {
                        let _ = write_operation_status(
                            &backup_dir,
                            &operation_id,
                            OperationStatus::RolledBack,
                            Some(err.to_string()),
                        );
                        Err(err)
                    }
                    (session_result, state_result) => {
                        let rollback_error = format!(
                            "{err}; rollback failed: sessions={:?}; global_state={:?}",
                            session_result.err(),
                            state_result.err()
                        );
                        let _ = write_operation_status(
                            &backup_dir,
                            &operation_id,
                            OperationStatus::RecoveryRequired,
                            Some(rollback_error.clone()),
                        );
                        anyhow::bail!("{rollback_error}")
                    }
                };
            }
        };
        write_operation_status(&backup_dir, &operation_id, OperationStatus::Committed, None)?;
        let _ = prune_backups(&home);
        let status = if collected.issues.is_empty()
            && collected.skipped_locked_rollout_files.is_empty()
            && applied.skipped_locked_rollout_files.is_empty()
        {
            ProviderSyncStatus::Synced
        } else {
            ProviderSyncStatus::Partial
        };
        let mut synced = result(
            status,
            "Provider sync complete",
            &target_provider,
            Some(backup_dir),
            applied.changes.len(),
            sqlite_updates.total(),
        );
        synced.active_db_path = active_db.map(|db| db.path);
        synced.operation_id = Some(operation_id);
        synced.scanned_session_files = collected.scanned_session_files;
        synced.skipped_locked_rollout_files = collected.skipped_locked_rollout_files;
        synced
            .skipped_locked_rollout_files
            .extend(applied.skipped_locked_rollout_files);
        synced.skipped_locked_rollout_files.sort();
        synced.skipped_locked_rollout_files.dedup();
        synced.sqlite_provider_rows_updated = sqlite_updates.provider_rows;
        synced.sqlite_user_event_rows_updated = sqlite_updates.user_event_rows;
        synced.sqlite_cwd_rows_updated = sqlite_updates.cwd_rows;
        synced.sqlite_rows_inserted = sqlite_updates.inserted_rows;
        synced.updated_workspace_roots = updated_workspace_roots;
        synced.encrypted_content_warning = encrypted_content_warning;
        synced.issues = collected.issues;
        Ok(synced)
    })();
    sync_result.unwrap_or_else(|err| {
        let recovery_required = has_recovery_required_operation(&home);
        error_result(
            if recovery_required {
                ProviderSyncStatus::RecoveryRequired
            } else {
                ProviderSyncStatus::Failed
            },
            format!("Provider sync failed: {err}"),
            if recovery_required {
                "provider_sync_recovery_required"
            } else {
                "provider_sync_failed"
            },
            &target_provider,
        )
    })
}

fn result(
    status: ProviderSyncStatus,
    message: impl Into<String>,
    target_provider: &str,
    backup_dir: Option<PathBuf>,
    changed_session_files: usize,
    sqlite_rows_updated: usize,
) -> ProviderSyncResult {
    ProviderSyncResult {
        status,
        message: message.into(),
        error_code: None,
        target_provider: target_provider.to_string(),
        backup_dir,
        active_db_path: None,
        operation_id: None,
        scanned_session_files: 0,
        changed_session_files,
        skipped_locked_rollout_files: Vec::new(),
        sqlite_rows_updated,
        sqlite_rows_inserted: 0,
        sqlite_provider_rows_updated: 0,
        sqlite_user_event_rows_updated: 0,
        sqlite_cwd_rows_updated: 0,
        updated_workspace_roots: 0,
        encrypted_content_warning: None,
        issues: Vec::new(),
    }
}

fn error_result(
    status: ProviderSyncStatus,
    message: impl Into<String>,
    error_code: &str,
    target_provider: &str,
) -> ProviderSyncResult {
    let mut result = result(status, message, target_provider, None, 0, 0);
    result.error_code = Some(error_code.to_string());
    result
}

fn dirs_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load_provider_sync_targets(codex_home: Option<&Path>) -> ProviderSyncTargetList {
    let home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs_home().join(".codex"));
    let current_provider = read_current_provider(&home.join("config.toml"));
    let mut sources: HashMap<String, HashSet<ProviderSyncTargetSource>> = HashMap::new();

    fn add_sources(
        sources: &mut HashMap<String, HashSet<ProviderSyncTargetSource>>,
        ids: impl IntoIterator<Item = String>,
        source: ProviderSyncTargetSource,
    ) {
        for id in ids {
            if !is_valid_provider_id_for_discovery(&id) {
                continue;
            }
            sources.entry(id).or_default().insert(source);
        }
    }

    add_sources(
        &mut sources,
        list_configured_provider_ids(&home.join("config.toml")),
        ProviderSyncTargetSource::Config,
    );
    add_sources(
        &mut sources,
        [current_provider.clone()],
        ProviderSyncTargetSource::Config,
    );
    if let Ok(ids) = rollout_provider_ids(&home) {
        add_sources(&mut sources, ids, ProviderSyncTargetSource::Rollout);
    }
    for db_path in codex_elves_core::codex_sqlite::codex_session_db_paths_from_home(&home) {
        if let Ok(ids) = sqlite_provider_ids(&db_path) {
            add_sources(&mut sources, ids, ProviderSyncTargetSource::Sqlite);
        }
    }

    let mut targets = sources
        .into_iter()
        .map(|(id, source_set)| {
            let mut source_list = source_set.into_iter().collect::<Vec<_>>();
            source_list.sort();
            ProviderSyncTargetOption {
                is_current_provider: id == current_provider,
                is_manual: source_list.contains(&ProviderSyncTargetSource::Manual),
                is_saved: false,
                id,
                sources: source_list,
            }
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .is_current_provider
            .cmp(&left.is_current_provider)
            .then_with(|| left.id.cmp(&right.id))
    });

    ProviderSyncTargetList {
        current_provider,
        targets,
    }
}

fn read_current_provider(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return DEFAULT_PROVIDER.to_string();
    };
    let provider = root_toml_string_value(&text, "model_provider").unwrap_or_default();
    if provider.trim().is_empty() {
        DEFAULT_PROVIDER.to_string()
    } else {
        provider
    }
}

fn resolve_target_provider(
    config_path: &Path,
    explicit_target_provider: Option<&str>,
) -> Result<String, String> {
    if let Some(raw) = explicit_target_provider {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(read_current_provider(config_path));
        }
        if !is_valid_explicit_provider_id(trimmed) {
            return Err(format!("Invalid provider sync target: {trimmed:?}"));
        }
        return Ok(trimmed.to_string());
    }
    Ok(read_current_provider(config_path))
}

fn is_valid_explicit_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn list_configured_provider_ids(path: &Path) -> Vec<String> {
    let mut ids = HashSet::new();
    ids.insert(DEFAULT_PROVIDER.to_string());
    let Ok(text) = fs::read_to_string(path) else {
        return sorted_provider_ids(ids);
    };
    for line in text.lines() {
        let stripped = line.trim();
        let Some(section) = stripped
            .strip_prefix("[model_providers.")
            .and_then(|rest| rest.strip_suffix(']'))
        else {
            continue;
        };
        let id = section.trim();
        if is_valid_provider_id_for_discovery(id) {
            ids.insert(id.to_string());
        }
    }
    sorted_provider_ids(ids)
}

fn sorted_provider_ids(ids: HashSet<String>) -> Vec<String> {
    let mut ids = ids
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn is_valid_provider_id_for_discovery(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn root_toml_string_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            break;
        }
        let Some(raw) = toml_key_raw_value(stripped, key) else {
            continue;
        };
        return toml_string_value(raw);
    }
    None
}

fn toml_key_raw_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start();
    rest.strip_prefix('=').map(str::trim_start)
}

fn toml_string_value(raw: &str) -> Option<String> {
    let quote = raw.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut value = String::new();
    let mut escaping = false;
    for ch in raw[quote.len_utf8()..].chars() {
        if quote == '"' && escaping {
            value.push(ch);
            escaping = false;
        } else if quote == '"' && ch == '\\' {
            escaping = true;
        } else if ch == quote {
            return Some(value);
        } else {
            value.push(ch);
        }
    }
    None
}

fn acquire_lock(path: &Path) -> anyhow::Result<ProviderSyncLockGuard> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let lease = acquire_provider_sync_lease(parent)?;
    if path.exists() {
        recover_stale_lock(path)?;
    }
    fs::create_dir(path)?;
    let token = Uuid::new_v4().to_string();
    let owner = ProviderSyncLockOwner {
        pid: std::process::id(),
        started_at: now_secs(),
        token: Some(token.clone()),
    };
    if let Err(error) = fs::write(path.join("owner.json"), serde_json::to_vec(&owner)?) {
        let _ = fs::remove_dir_all(path);
        return Err(error.into());
    }
    Ok(ProviderSyncLockGuard {
        path: path.to_path_buf(),
        token,
        _lease: lease,
    })
}

fn acquire_provider_sync_lease(parent: &Path) -> anyhow::Result<ProviderSyncLeaseGuard> {
    let lease_path = parent.join("provider-sync.lock.lease");
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lease_path)?;
    FileExt::try_lock_exclusive(&file).map_err(|error| {
        anyhow::anyhow!(
            "Provider sync lease is busy ({}): {error}",
            lease_path.to_string_lossy()
        )
    })?;
    Ok(ProviderSyncLeaseGuard { file })
}

fn recover_stale_lock(path: &Path) -> anyhow::Result<()> {
    let owner_path = path.join("owner.json");
    if let Ok(text) = fs::read_to_string(&owner_path)
        && let Ok(owner) = serde_json::from_str::<ProviderSyncLockOwner>(&text)
    {
        if codex_elves_core::watcher::process_id_is_running(owner.pid) {
            anyhow::bail!(
                "Provider sync is already running (PID {}): {}",
                owner.pid,
                path.to_string_lossy()
            );
        }
        return remove_stale_lock(path);
    }

    anyhow::bail!(
        "Provider sync lock owner is unreadable; automatic cleanup is unsafe: {}",
        path.to_string_lossy()
    )
}

fn stale_lock_owner_is_dead(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(text) = fs::read_to_string(path.join("owner.json")) else {
        return false;
    };
    let Ok(owner) = serde_json::from_str::<ProviderSyncLockOwner>(&text) else {
        return false;
    };
    !codex_elves_core::watcher::process_id_is_running(owner.pid)
}

fn remove_stale_lock(path: &Path) -> anyhow::Result<()> {
    let stale_path = path.with_file_name(format!("provider-sync.lock.stale-{}", Uuid::new_v4()));
    match fs::rename(path, &stale_path) {
        Ok(()) => {
            fs::remove_dir_all(stale_path)?;
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn collect_session_changes(home: &Path, target_provider: &str) -> anyhow::Result<SessionChanges> {
    let mut collected = SessionChanges::default();
    let files = rollout_files(home)?;
    collected.scanned_session_files = files.len();
    for path in files {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if is_locked_io_error(&error) => {
                collected.skipped_locked_rollout_files.push(path.clone());
                collected.issues.push(SessionRepairIssue {
                    path,
                    thread_id: None,
                    kind: SessionAnomalyKind::Locked,
                    message: "rollout file is locked".to_string(),
                });
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if bytes.is_empty() {
            collected.issues.push(SessionRepairIssue {
                path,
                thread_id: None,
                kind: SessionAnomalyKind::EmptyFile,
                message: "rollout file is empty; content cannot be reconstructed".to_string(),
            });
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                collected.issues.push(SessionRepairIssue {
                    path,
                    thread_id: None,
                    kind: SessionAnomalyKind::InvalidUtf8,
                    message: "rollout file is not valid UTF-8".to_string(),
                });
                continue;
            }
        };
        let mut rewrite = rewrite_rollout_session_meta_providers(&text, target_provider)?;
        if rewrite.session_meta_count == 0 {
            collected.issues.push(SessionRepairIssue {
                path,
                thread_id: None,
                kind: SessionAnomalyKind::MissingSessionMeta,
                message: "no valid session_meta record found".to_string(),
            });
            continue;
        }
        let path_thread_id = rollout_thread_id_from_path(&path);
        let canonical_thread_id = path_thread_id
            .as_ref()
            .filter(|thread_id| rewrite.thread_ids.contains(*thread_id))
            .cloned()
            .or_else(|| {
                (rewrite.thread_ids.len() == 1)
                    .then(|| rewrite.thread_ids.iter().next().cloned())
                    .flatten()
            });
        let Some(canonical_thread_id) = canonical_thread_id else {
            collected.issues.push(SessionRepairIssue {
                path,
                thread_id: rewrite.thread_id.clone(),
                kind: SessionAnomalyKind::ConflictingThreadIds,
                message: format!(
                    "rollout contains conflicting session ids: {}",
                    rewrite
                        .thread_ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
            continue;
        };
        rewrite.thread_id = Some(canonical_thread_id.clone());
        if let Some(payload) = rewrite.session_meta_payloads.get(&canonical_thread_id) {
            rewrite.session_meta_payload = Some(payload.clone());
        }
        if let Some(cwd) = rewrite.cwd_by_thread_id.get(&canonical_thread_id) {
            rewrite.cwd = Some(cwd.clone());
        }
        if rewrite.malformed_json_lines > 0 {
            collected.issues.push(SessionRepairIssue {
                path: path.clone(),
                thread_id: rewrite.thread_id.clone(),
                kind: SessionAnomalyKind::MalformedJson,
                message: format!(
                    "{} malformed JSON line(s) were preserved",
                    rewrite.malformed_json_lines
                ),
            });
        }
        let has_user_event = rewrite.has_user_event;
        let mut kind = rewrite
            .session_meta_payload
            .as_ref()
            .map(classify_session_kind)
            .unwrap_or(SessionKind::Unknown);
        if kind == SessionKind::Unknown && has_user_event {
            kind = SessionKind::User;
        }
        if text.contains("encrypted_content") {
            for provider in &rewrite.providers {
                *collected
                    .encrypted_content_counts
                    .entry(provider.clone())
                    .or_insert(0) += 1;
            }
        }
        let metadata = fs::metadata(&path)?;
        let original_mtime = metadata.modified().ok();
        let index_record =
            build_session_index_record(&path, &rewrite, original_mtime, kind, has_user_event);
        collected.changes.push(SessionChange {
            path,
            original_text: text,
            next_text: rewrite.next_text,
            original_session_meta_lines: rewrite.original_session_meta_lines,
            thread_id: rewrite.thread_id,
            cwd: rewrite.cwd,
            has_user_event,
            kind,
            rewrite_needed: rewrite.rewrite_needed,
            original_mtime,
            original_len: metadata.len(),
            index_record,
        });
    }
    let mut paths_by_thread_id = HashMap::<String, Vec<PathBuf>>::new();
    for change in &collected.changes {
        if let Some(thread_id) = &change.thread_id {
            paths_by_thread_id
                .entry(thread_id.clone())
                .or_default()
                .push(change.path.clone());
        }
    }
    let duplicate_ids = paths_by_thread_id
        .iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(thread_id, paths)| (thread_id.clone(), paths.clone()))
        .collect::<HashMap<_, _>>();
    for (thread_id, paths) in &duplicate_ids {
        for path in paths {
            collected.issues.push(SessionRepairIssue {
                path: path.clone(),
                thread_id: Some(thread_id.clone()),
                kind: SessionAnomalyKind::DuplicateThreadId,
                message: format!(
                    "thread id is shared by {} rollout files; all writes for this id were skipped",
                    paths.len()
                ),
            });
        }
    }
    collected.changes.retain(|change| {
        change
            .thread_id
            .as_ref()
            .is_none_or(|thread_id| !duplicate_ids.contains_key(thread_id))
    });
    Ok(collected)
}

fn rewrite_rollout_session_meta_providers(
    text: &str,
    target_provider: &str,
) -> anyhow::Result<RolloutRewrite> {
    let mut rewrite = RolloutRewrite::default();
    for segment in text.split_inclusive('\n') {
        let (line, line_ending) = split_line_ending(segment);
        let mut next_line = line.to_string();
        if !line.trim().is_empty() {
            match serde_json::from_str::<Value>(line) {
                Ok(mut record) => {
                    update_latest_rollout_timestamp(&mut rewrite, &record);
                    let record_type = record
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if record_type == "session_meta" {
                        if rewrite.created_timestamp_ms.is_none() {
                            rewrite.created_timestamp_ms = rollout_timestamp_ms(&record);
                        }
                        let Some(payload) =
                            record.get_mut("payload").and_then(Value::as_object_mut)
                        else {
                            rewrite.next_text.push_str(&next_line);
                            rewrite.next_text.push_str(line_ending);
                            continue;
                        };
                        rewrite.session_meta_count += 1;
                        rewrite.original_session_meta_lines.push(line.to_string());
                        if rewrite.session_meta_payload.is_none() {
                            rewrite.session_meta_payload = Some(payload.clone());
                        }
                        if rewrite.thread_id.is_none() {
                            rewrite.thread_id = payload
                                .get("id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string);
                        }
                        if let Some(thread_id) = payload.get("id").and_then(Value::as_str) {
                            rewrite.thread_ids.insert(thread_id.to_string());
                            rewrite
                                .session_meta_payloads
                                .entry(thread_id.to_string())
                                .or_insert_with(|| payload.clone());
                            if let Some(cwd) = payload
                                .get("cwd")
                                .and_then(Value::as_str)
                                .and_then(to_desktop_workspace_path)
                            {
                                rewrite
                                    .cwd_by_thread_id
                                    .entry(thread_id.to_string())
                                    .or_insert(cwd);
                            }
                        }
                        if rewrite.cwd.is_none() {
                            rewrite.cwd = payload
                                .get("cwd")
                                .and_then(Value::as_str)
                                .and_then(to_desktop_workspace_path);
                        }
                        let provider = payload
                            .get("model_provider")
                            .and_then(Value::as_str)
                            .unwrap_or("(missing)")
                            .to_string();
                        rewrite.providers.push(provider);
                        if payload.get("model_provider").and_then(Value::as_str)
                            != Some(target_provider)
                        {
                            payload.insert("model_provider".to_string(), json!(target_provider));
                            next_line = serde_json::to_string(&record)?;
                            rewrite.rewrite_needed = true;
                        }
                    } else if record_type == "turn_context" && rewrite.first_turn_context.is_none()
                    {
                        rewrite.first_turn_context =
                            record.get("payload").and_then(Value::as_object).cloned();
                    } else if is_user_event_record(&record) {
                        rewrite.has_user_event = true;
                        if rewrite.first_user_message.is_none() {
                            rewrite.first_user_message = extract_user_message(&record);
                        }
                    }
                }
                Err(_) => rewrite.malformed_json_lines += 1,
            }
        }
        rewrite.next_text.push_str(&next_line);
        rewrite.next_text.push_str(line_ending);
    }
    Ok(rewrite)
}

fn classify_session_kind(payload: &Map<String, Value>) -> SessionKind {
    if payload
        .get("source")
        .and_then(Value::as_object)
        .is_some_and(|source| source.contains_key("subagent") || source.contains_key("agent"))
        || [
            "forked_from_id",
            "parent_thread_id",
            "agent_role",
            "agent_path",
        ]
        .iter()
        .any(|key| {
            payload
                .get(*key)
                .and_then(value_to_storage_string)
                .is_some_and(|value| !value.trim().is_empty())
        })
    {
        return SessionKind::Subagent;
    }

    let source = payload
        .get("thread_source")
        .or_else(|| payload.get("source"))
        .and_then(value_to_storage_string)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if source.contains("automation") {
        SessionKind::Automation
    } else if matches!(
        source.as_str(),
        "user" | "vscode" | "cli" | "exec" | "desktop" | "app"
    ) {
        SessionKind::User
    } else if source.contains("noninteractive") {
        SessionKind::NonInteractive
    } else {
        SessionKind::Unknown
    }
}

fn update_latest_rollout_timestamp(rewrite: &mut RolloutRewrite, record: &Value) {
    let Some(timestamp_ms) = rollout_timestamp_ms(record) else {
        return;
    };
    rewrite.latest_timestamp_ms = Some(
        rewrite
            .latest_timestamp_ms
            .unwrap_or(timestamp_ms)
            .max(timestamp_ms),
    );
}

fn rollout_timestamp_ms(record: &Value) -> Option<i64> {
    let raw = record.get("timestamp").or_else(|| {
        record
            .get("payload")
            .and_then(|payload| payload.get("timestamp"))
    })?;
    if let Some(value) = raw.as_i64() {
        return Some(if value.abs() < 10_000_000_000 {
            value.saturating_mul(1_000)
        } else {
            value
        });
    }
    chrono::DateTime::parse_from_rfc3339(raw.as_str()?)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn extract_user_message(record: &Value) -> Option<String> {
    let record_type = record.get("type").and_then(Value::as_str)?;
    let payload = record.get("payload")?;
    match record_type {
        "event_msg" => {
            let event_type = payload.get("type").and_then(Value::as_str)?;
            matches!(event_type, "user_message" | "user_input")
                .then(|| first_text(payload))
                .flatten()
        }
        "response_item" => (payload.get("role").and_then(Value::as_str) == Some("user"))
            .then(|| first_text(payload))
            .flatten(),
        "user_message" | "user_input" => first_text(payload),
        _ => None,
    }
}

fn is_user_event_record(record: &Value) -> bool {
    let Some(record_type) = record.get("type").and_then(Value::as_str) else {
        return false;
    };
    let Some(payload) = record.get("payload") else {
        return false;
    };
    match record_type {
        "event_msg" => matches!(
            payload.get("type").and_then(Value::as_str),
            Some("user_message" | "user_input")
        ),
        "response_item" => payload.get("role").and_then(Value::as_str) == Some("user"),
        "user_message" | "user_input" => true,
        _ => false,
    }
}

fn first_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Array(items) => items.iter().find_map(first_text),
        Value::Object(object) => ["message", "text", "content", "input_text", "value"]
            .iter()
            .filter_map(|key| object.get(*key))
            .find_map(first_text),
        _ => None,
    }
}

fn value_to_storage_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Null => None,
        value => serde_json::to_string(value).ok(),
    }
}

fn build_session_index_record(
    path: &Path,
    rewrite: &RolloutRewrite,
    original_mtime: Option<SystemTime>,
    kind: SessionKind,
    has_user_event: bool,
) -> Option<SessionIndexRecord> {
    if kind != SessionKind::User || !has_user_event {
        return None;
    }
    let payload = rewrite.session_meta_payload.as_ref()?;
    let thread_id = rewrite.thread_id.clone()?;
    let first_user_message = rewrite.first_user_message.clone().unwrap_or_default();
    let fallback_timestamp_ms = original_mtime
        .and_then(system_time_millis)
        .unwrap_or_else(|| now_secs() as i64 * 1_000);
    let created_at_ms = rewrite
        .created_timestamp_ms
        .unwrap_or(fallback_timestamp_ms);
    let updated_at_ms = rewrite
        .latest_timestamp_ms
        .unwrap_or(fallback_timestamp_ms)
        .max(created_at_ms);
    let context = rewrite.first_turn_context.as_ref();
    let sandbox_policy = context
        .and_then(|value| {
            value
                .get("sandbox_policy")
                .or_else(|| value.get("sandboxPolicy"))
        })
        .and_then(value_to_storage_string)
        .unwrap_or_else(|| json!({"type": "read-only"}).to_string());
    let approval_mode = context
        .and_then(|value| {
            value
                .get("approval_policy")
                .or_else(|| value.get("approval_mode"))
                .or_else(|| value.get("approvalPolicy"))
                .or_else(|| value.get("approvalMode"))
        })
        .and_then(value_to_storage_string)
        .unwrap_or_else(|| "on-request".to_string());
    let source = payload
        .get("source")
        .and_then(value_to_storage_string)
        .unwrap_or_else(|| "user".to_string());
    Some(SessionIndexRecord {
        thread_id,
        rollout_path: path.to_path_buf(),
        created_at: created_at_ms / 1_000,
        updated_at: updated_at_ms / 1_000,
        source,
        thread_source: "user".to_string(),
        cwd: rewrite.cwd.clone().unwrap_or_default(),
        title: if first_user_message.is_empty() {
            "历史会话".to_string()
        } else {
            first_user_message.chars().take(120).collect()
        },
        first_user_message,
        sandbox_policy,
        approval_mode,
        cli_version: payload
            .get("cli_version")
            .or_else(|| payload.get("version"))
            .and_then(value_to_storage_string)
            .unwrap_or_default(),
        archived: path.components().any(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("archived_sessions")
        }),
    })
}

fn system_time_millis(value: SystemTime) -> Option<i64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn rollout_files(home: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dirname in SESSION_DIRS {
        let root = home.join(dirname);
        if root.exists() {
            collect_rollout_files(&root, &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn rollout_provider_ids(home: &Path) -> anyhow::Result<Vec<String>> {
    let mut ids = HashSet::new();
    for path in rollout_files(home)? {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if is_locked_io_error(&error) => continue,
            Err(error) => return Err(error.into()),
        };
        for segment in text.split_inclusive('\n') {
            let (line, _) = split_line_ending(segment);
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if record.get("type").and_then(Value::as_str) != Some("session_meta") {
                continue;
            }
            let Some(provider) = record
                .get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("model_provider"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if is_valid_provider_id_for_discovery(provider) {
                ids.insert(provider.to_string());
            }
        }
    }
    Ok(sorted_provider_ids(ids))
}

fn collect_rollout_files(root: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rollout_files(&path, files)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn rollout_thread_id_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let candidate = stem.rsplit('-').take(5).collect::<Vec<_>>();
    if candidate.len() != 5 {
        return None;
    }
    let candidate = candidate.into_iter().rev().collect::<Vec<_>>().join("-");
    Uuid::parse_str(&candidate).ok().map(|_| candidate)
}

fn split_line_ending(segment: &str) -> (&str, &str) {
    if let Some(line) = segment.strip_suffix("\r\n") {
        (line, "\r\n")
    } else if let Some(line) = segment.strip_suffix('\n') {
        (line, "\n")
    } else {
        (segment, "")
    }
}

fn to_desktop_workspace_path(value: &str) -> Option<String> {
    let stripped = value.trim();
    if stripped.is_empty() {
        return None;
    }
    let lower = stripped.to_ascii_lowercase();
    if lower.starts_with(r"\\?\unc\") {
        return Some(format!(r"\\{}", stripped[8..].replace('/', r"\")));
    }
    if stripped.starts_with(r"\\?\") {
        return Some(stripped[4..].replace('\\', "/"));
    }
    Some(stripped.to_string())
}

fn is_locked_io_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::PermissionDenied)
        || matches!(error.raw_os_error(), Some(32 | 33))
}

fn build_encrypted_content_warning(
    encrypted_content_counts: &HashMap<String, usize>,
    target_provider: &str,
) -> Option<String> {
    let risky_providers = encrypted_content_counts
        .iter()
        .filter(|(provider, count)| provider.as_str() != target_provider && **count > 0)
        .map(|(provider, _)| provider.as_str())
        .collect::<Vec<_>>();
    if risky_providers.is_empty() {
        return None;
    }
    let total = encrypted_content_counts.values().sum::<usize>();
    Some(format!(
        "检测到 {total} 个会话文件包含来自 {} 的 encrypted_content。可见会话元数据已同步到 {target_provider}，但继续或压缩这些历史可能出现 invalid_encrypted_content；需要可靠续聊时请切回原供应商/账号或开启新会话。",
        risky_providers.join(", ")
    ))
}

fn create_backup(
    home: &Path,
    target_provider: &str,
    operation_id: &str,
    changes: &[SessionChange],
) -> anyhow::Result<PathBuf> {
    let backup_root = home.join("backups_state/provider-sync");
    let mut backup_dir = backup_root.join(timestamp_name());
    let mut suffix = 0;
    while backup_dir.exists() {
        suffix += 1;
        backup_dir = backup_root.join(format!("{}-{suffix}", timestamp_name()));
    }
    fs::create_dir_all(&backup_dir)?;
    for name in [
        "config.toml",
        ".codex-global-state.json",
        ".codex-global-state.json.bak",
    ] {
        let source = home.join(name);
        if source.exists() {
            fs::copy(&source, backup_dir.join(name))?;
        }
    }
    let db_dir = backup_dir.join("db");
    let mut db_files = Vec::new();
    for db_path in codex_elves_core::codex_sqlite::codex_session_db_paths_from_home(home) {
        for source in codex_elves_core::codex_sqlite::codex_sqlite_sidecar_paths(&db_path) {
            if !source.exists() {
                continue;
            }
            let relative = codex_elves_core::codex_sqlite::relative_to_codex_home(home, &source);
            let target = db_dir.join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source, &target)?;
            db_files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    let manifest = changes
        .iter()
        .map(|change| {
            let relative = change
                .path
                .strip_prefix(home)
                .unwrap_or(&change.path)
                .to_path_buf();
            let backup_path = backup_dir.join("rollouts").join(&relative);
            if let Some(parent) = backup_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&backup_path, change.original_text.as_bytes())?;
            Ok::<Value, anyhow::Error>(json!({
                "path": change.path.to_string_lossy(),
                "backupPath": backup_path.to_string_lossy(),
                "originalSessionMetaLines": change.original_session_meta_lines,
            }))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    fs::write(
        backup_dir.join("session-meta-backup.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    fs::write(
        backup_dir.join("metadata.json"),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "namespace": "provider-sync",
            "codexHome": home.to_string_lossy(),
            "targetProvider": target_provider,
            "operationId": operation_id,
            "createdAt": chrono::Utc::now().to_rfc3339(),
            "dbFiles": db_files,
            "changedSessionFiles": changes.len(),
            "managedBy": "CodexElves provider sync"
        }))?,
    )?;
    write_operation_status(&backup_dir, operation_id, OperationStatus::Prepared, None)?;
    Ok(backup_dir)
}

fn write_operation_status(
    backup_dir: &Path,
    operation_id: &str,
    status: OperationStatus,
    error: Option<String>,
) -> anyhow::Result<()> {
    let journal = OperationJournal {
        version: 1,
        operation_id: operation_id.to_string(),
        status,
        updated_at: chrono::Utc::now().to_rfc3339(),
        error,
    };
    let path = backup_dir.join("operation.json");
    let next = serde_json::to_vec_pretty(&journal)?;
    if path.exists() {
        replace_file_contents(&path, &next)?;
    } else {
        fs::write(path, next)?;
    }
    Ok(())
}

fn recover_incomplete_operations(home: &Path) -> anyhow::Result<usize> {
    let root = home.join("backups_state/provider-sync");
    if !root.exists() {
        return Ok(0);
    }
    let mut recovered = 0;
    for entry in fs::read_dir(&root)? {
        let backup_dir = entry?.path();
        if !backup_dir.is_dir() {
            continue;
        }
        let operation_path = backup_dir.join("operation.json");
        let Ok(text) = fs::read_to_string(&operation_path) else {
            continue;
        };
        let journal: OperationJournal = serde_json::from_str(&text)?;
        if matches!(
            journal.status,
            OperationStatus::Committed | OperationStatus::RolledBack
        ) {
            continue;
        }
        let default_home = codex_elves_core::codex_home::default_codex_home_dir();
        if home == default_home && !codex_elves_core::watcher::find_codex_processes().is_empty() {
            anyhow::bail!(
                "unfinished operation {} cannot be recovered while Codex/ChatGPT is running",
                journal.operation_id
            );
        }
        restore_backup_snapshot(home, &backup_dir)?;
        write_operation_status(
            &backup_dir,
            &journal.operation_id,
            OperationStatus::RolledBack,
            Some("automatically recovered before the next provider sync".to_string()),
        )?;
        recovered += 1;
    }
    Ok(recovered)
}

fn has_recovery_required_operation(home: &Path) -> bool {
    let root = home.join("backups_state/provider-sync");
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        fs::read_to_string(entry.path().join("operation.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<OperationJournal>(&text).ok())
            .is_some_and(|journal| matches!(journal.status, OperationStatus::RecoveryRequired))
    })
}

fn restore_backup_snapshot(home: &Path, backup_dir: &Path) -> anyhow::Result<()> {
    let manifest_path = backup_dir.join("session-meta-backup.json");
    if manifest_path.exists() {
        let manifest = serde_json::from_str::<Value>(&fs::read_to_string(&manifest_path)?)?;
        for item in manifest.as_array().into_iter().flatten() {
            let Some(path) = item.get("path").and_then(Value::as_str) else {
                continue;
            };
            let Some(backup_path) = item.get("backupPath").and_then(Value::as_str) else {
                continue;
            };
            let path = PathBuf::from(path);
            let contents = fs::read(backup_path)?;
            if path.exists() {
                replace_file_contents(&path, &contents)?;
            } else {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, contents)?;
            }
        }
    }

    for name in [".codex-global-state.json", ".codex-global-state.json.bak"] {
        let source = backup_dir.join(name);
        if !source.exists() {
            continue;
        }
        let target = home.join(name);
        let contents = fs::read(source)?;
        if target.exists() {
            replace_file_contents(&target, &contents)?;
        } else {
            fs::write(target, contents)?;
        }
    }

    let db_root = backup_dir.join("db");
    if db_root.exists() {
        let mut files = Vec::new();
        collect_all_files(&db_root, &mut files)?;
        for source in files {
            let relative = source.strip_prefix(&db_root)?;
            let target = home.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let contents = fs::read(&source)?;
            if target.exists() {
                replace_file_contents(&target, &contents)?;
            } else {
                fs::write(target, contents)?;
            }
        }
    }
    Ok(())
}

fn collect_all_files(root: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_all_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn apply_session_changes(changes: &[SessionChange]) -> anyhow::Result<AppliedSessionChanges> {
    let mut applied = AppliedSessionChanges::default();
    for change in changes {
        let metadata = match fs::metadata(&change.path) {
            Ok(metadata) => metadata,
            Err(error) if is_locked_io_error(&error) => {
                applied
                    .skipped_locked_rollout_files
                    .push(change.path.clone());
                applied.issues.push(SessionRepairIssue {
                    path: change.path.clone(),
                    thread_id: change.thread_id.clone(),
                    kind: SessionAnomalyKind::Locked,
                    message: "rollout became locked before write".to_string(),
                });
                continue;
            }
            Err(error) => {
                return match restore_session_changes(&applied.changes) {
                    Ok(()) => Err(error.into()),
                    Err(restore_error) => Err(std::io::Error::other(format!(
                        "rollout metadata read failed: {error}; restore failed: {restore_error}"
                    ))
                    .into()),
                };
            }
        };
        let current_mtime = metadata.modified().ok();
        if metadata.len() != change.original_len || current_mtime != change.original_mtime {
            applied.issues.push(SessionRepairIssue {
                path: change.path.clone(),
                thread_id: change.thread_id.clone(),
                kind: SessionAnomalyKind::ConcurrentModification,
                message: "rollout changed after scanning; write was skipped".to_string(),
            });
            continue;
        }
        match replace_file_contents(&change.path, change.next_text.as_bytes()) {
            Ok(()) => {}
            Err(error) if is_locked_io_error(&error) => {
                applied
                    .skipped_locked_rollout_files
                    .push(change.path.clone());
                applied.issues.push(SessionRepairIssue {
                    path: change.path.clone(),
                    thread_id: change.thread_id.clone(),
                    kind: SessionAnomalyKind::Locked,
                    message: "rollout became locked during atomic replace".to_string(),
                });
                continue;
            }
            Err(error) => {
                return match restore_session_changes(&applied.changes) {
                    Ok(()) => Err(error.into()),
                    Err(restore_error) => Err(std::io::Error::other(format!(
                        "rollout write failed: {error}; restore failed: {restore_error}"
                    ))
                    .into()),
                };
            }
        }
        restore_file_mtime(&change.path, change.original_mtime);
        applied.changes.push(change.clone());
    }
    Ok(applied)
}

fn restore_session_changes(changes: &[SessionChange]) -> anyhow::Result<()> {
    for change in changes {
        replace_file_contents(&change.path, change.original_text.as_bytes())?;
        restore_file_mtime(&change.path, change.original_mtime);
    }
    Ok(())
}

fn replace_file_contents(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rollout.jsonl");
    let token = Uuid::new_v4();
    let temp_path = parent.join(format!(".{file_name}.provider-sync-{token}.tmp"));
    let old_path = parent.join(format!(".{file_name}.provider-sync-{token}.old"));
    let mut temp = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;
    temp.write_all(contents)?;
    temp.sync_all()?;
    drop(temp);

    if let Err(error) = fs::rename(path, &old_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let restore_error = fs::rename(&old_path, path);
        let _ = fs::remove_file(&temp_path);
        return match restore_error {
            Ok(()) => Err(error),
            Err(restore_error) => Err(std::io::Error::other(format!(
                "replace failed: {error}; restore failed: {restore_error}"
            ))),
        };
    }
    let _ = fs::remove_file(old_path);
    Ok(())
}

fn read_optional_bytes(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn restore_optional_bytes(path: &Path, contents: Option<&[u8]>) -> anyhow::Result<()> {
    match contents {
        Some(contents) => {
            replace_file_contents(path, contents)?;
        }
        None if path.exists() => {
            fs::remove_file(path)?;
        }
        None => {}
    }
    Ok(())
}

fn restore_file_mtime(path: &Path, mtime: Option<SystemTime>) {
    let Some(mtime) = mtime else { return };
    let Ok(file) = fs::File::options().write(true).open(path) else {
        return;
    };
    let times = std::fs::FileTimes::new().set_modified(mtime);
    let _ = file.set_times(times);
}

fn table_columns(db: &Connection, table: &str) -> anyhow::Result<HashSet<String>> {
    let mut stmt = db.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table.replace('"', "\"\"")
    ))?;
    Ok(stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<HashSet<_>>>()?)
}

fn sqlite_provider_ids(path: &Path) -> anyhow::Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let db = Connection::open(path)?;
    let columns = table_columns(&db, "threads")?;
    if !columns.contains("model_provider") {
        return Ok(Vec::new());
    }
    let mut stmt = db.prepare(
        "SELECT DISTINCT COALESCE(model_provider, '') FROM threads WHERE COALESCE(model_provider, '') <> ''",
    )?;
    let mut ids = HashSet::new();
    for item in stmt.query_map([], |row| row.get::<_, String>(0))? {
        let id = item?;
        if is_valid_provider_id_for_discovery(&id) {
            ids.insert(id);
        }
    }
    Ok(sorted_provider_ids(ids))
}

fn resolve_active_db(
    home: &Path,
    rollout_thread_ids: &HashSet<String>,
) -> anyhow::Result<Option<ActiveDb>> {
    let mut candidates = Vec::new();
    for path in codex_elves_core::codex_sqlite::codex_session_db_paths_from_home(home) {
        if let Some(candidate) = db_candidate(&path, rollout_thread_ids)? {
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|left, right| right.evidence.cmp(&left.evidence));
    if candidates.len() > 1 && candidates[0].evidence == candidates[1].evidence {
        anyhow::bail!(
            "ambiguous active threads database: {} and {} have identical evidence",
            candidates[0].active_db.path.to_string_lossy(),
            candidates[1].active_db.path.to_string_lossy()
        );
    }
    Ok(candidates.into_iter().next().map(|item| item.active_db))
}

fn db_candidate(
    path: &Path,
    rollout_thread_ids: &HashSet<String>,
) -> anyhow::Result<Option<DbCandidate>> {
    if !path.exists() {
        return Ok(None);
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let columns = table_columns(&db, "threads")?;
    if columns.is_empty() {
        return Ok(None);
    }
    if !columns.contains("id") || !columns.contains("model_provider") {
        anyhow::bail!(
            "unsupported threads schema in {}: id/model_provider is missing",
            path.to_string_lossy()
        );
    }
    let row_count = db.query_row("SELECT COUNT(*) FROM threads", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let migration_version = if table_columns(&db, "_sqlx_migrations")?.contains("version") {
        db.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE COALESCE(success, 1) = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?
    } else {
        0
    };
    let mut rollout_overlap = 0_i64;
    if !rollout_thread_ids.is_empty() {
        let mut stmt = db.prepare("SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)")?;
        for thread_id in rollout_thread_ids {
            if stmt.query_row([thread_id], |row| row.get::<_, i64>(0))? != 0 {
                rollout_overlap += 1;
            }
        }
    }
    let latest_activity_ms = max_thread_activity_ms(&db, &columns)?;
    let file_activity_ms = codex_elves_core::codex_sqlite::codex_sqlite_sidecar_paths(path)
        .iter()
        .filter_map(|candidate| fs::metadata(candidate).ok())
        .filter_map(|metadata| metadata.modified().ok())
        .filter_map(system_time_millis)
        .max()
        .unwrap_or_default();
    Ok(Some(DbCandidate {
        active_db: ActiveDb {
            path: path.to_path_buf(),
            columns: columns.clone(),
        },
        evidence: DbEvidence {
            migration_version,
            rollout_overlap,
            latest_activity_ms,
            file_activity_ms,
            row_count,
            schema_columns: columns.len(),
        },
    }))
}

fn max_thread_activity_ms(db: &Connection, columns: &HashSet<String>) -> anyhow::Result<i64> {
    let mut latest = 0_i64;
    for column in ["recency_at_ms", "updated_at_ms", "created_at_ms"] {
        if columns.contains(column) {
            latest = latest.max(max_integer_column(db, column)?);
        }
    }
    for column in ["recency_at", "updated_at", "created_at"] {
        if columns.contains(column) {
            latest = latest.max(max_integer_column(db, column)?.saturating_mul(1_000));
        }
    }
    Ok(latest)
}

fn max_integer_column(db: &Connection, column: &str) -> anyhow::Result<i64> {
    let sql = format!(
        "SELECT COALESCE(MAX(CAST(\"{}\" AS INTEGER)), 0) FROM threads",
        column.replace('"', "\"\"")
    );
    Ok(db.query_row(&sql, [], |row| row.get::<_, i64>(0))?)
}

fn count_sqlite_updates(
    active_db: Option<&ActiveDb>,
    target_provider: &str,
    user_event_thread_ids: &HashSet<String>,
    cwd_by_thread_id: &HashMap<String, String>,
    changes: &[SessionChange],
) -> anyhow::Result<SqliteUpdateCounts> {
    let Some(active_db) = active_db else {
        return Ok(SqliteUpdateCounts::default());
    };
    let db = Connection::open_with_flags(&active_db.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut counts = SqliteUpdateCounts::default();
    for thread_id in changes
        .iter()
        .filter_map(|change| change.thread_id.as_deref())
    {
        counts.provider_rows += db.query_row(
            "SELECT COUNT(*) FROM threads WHERE id = ?1 AND COALESCE(model_provider, '') <> ?2",
            (thread_id, target_provider),
            |row| row.get::<_, i64>(0),
        )? as usize;
    }
    if active_db.columns.contains("has_user_event") {
        for thread_id in user_event_thread_ids {
            counts.user_event_rows += db.query_row(
                "SELECT COUNT(*) FROM threads WHERE id = ?1 AND COALESCE(has_user_event, 0) <> 1",
                [thread_id],
                |row| row.get::<_, i64>(0),
            )? as usize;
        }
    }
    if active_db.columns.contains("cwd") {
        for (thread_id, cwd) in cwd_by_thread_id {
            counts.cwd_rows += db.query_row(
                "SELECT COUNT(*) FROM threads WHERE id = ?1 AND COALESCE(cwd, '') <> ?2",
                (thread_id, cwd),
                |row| row.get::<_, i64>(0),
            )? as usize;
        }
    }
    counts.inserted_rows = missing_index_records(&db, changes)?.len();
    Ok(counts)
}

fn apply_sqlite_update(
    active_db: Option<&ActiveDb>,
    target_provider: &str,
    user_event_thread_ids: &HashSet<String>,
    cwd_by_thread_id: &HashMap<String, String>,
    changes: &[SessionChange],
) -> anyhow::Result<SqliteUpdateCounts> {
    let Some(active_db) = active_db else {
        return Ok(SqliteUpdateCounts::default());
    };
    let mut db = Connection::open(&active_db.path)?;
    let column_info = table_column_info(&db, "threads")?;
    let tx = db.transaction()?;
    let mut counts = SqliteUpdateCounts::default();
    for thread_id in changes
        .iter()
        .filter_map(|change| change.thread_id.as_deref())
    {
        counts.provider_rows += tx.execute(
            "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND COALESCE(model_provider, '') <> ?1",
            (target_provider, thread_id),
        )?;
    }
    if active_db.columns.contains("has_user_event") {
        for thread_id in user_event_thread_ids {
            counts.user_event_rows += tx.execute(
                "UPDATE threads SET has_user_event = 1 WHERE id = ?1 AND COALESCE(has_user_event, 0) <> 1",
                [thread_id],
            )?;
        }
    }
    if active_db.columns.contains("cwd") {
        for (thread_id, cwd) in cwd_by_thread_id {
            counts.cwd_rows += tx.execute(
                "UPDATE threads SET cwd = ?1 WHERE id = ?2 AND COALESCE(cwd, '') <> ?1",
                (cwd, thread_id),
            )?;
        }
    }
    for record in missing_index_records(&tx, changes)? {
        insert_thread_index(&tx, &column_info, record, target_provider)?;
        counts.inserted_rows += 1;
    }
    tx.commit()?;
    Ok(counts)
}

#[derive(Debug)]
struct TableColumnInfo {
    name: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key: bool,
}

fn table_column_info(db: &Connection, table: &str) -> anyhow::Result<Vec<TableColumnInfo>> {
    let mut stmt = db.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table.replace('"', "\"\"")
    ))?;
    Ok(stmt
        .query_map([], |row| {
            Ok(TableColumnInfo {
                name: row.get(1)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default_value: row.get(4)?,
                primary_key: row.get::<_, i64>(5)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn validate_insert_schema(
    active_db: Option<&ActiveDb>,
    changes: &[SessionChange],
) -> anyhow::Result<()> {
    let Some(active_db) = active_db else {
        return Ok(());
    };
    if !changes.iter().any(|change| change.index_record.is_some()) {
        return Ok(());
    }
    let db = Connection::open_with_flags(&active_db.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let column_info = table_column_info(&db, "threads")?;
    let known = known_index_columns();
    for column in column_info {
        if column.not_null
            && column.default_value.is_none()
            && !column.primary_key
            && !known.contains(column.name.as_str())
        {
            anyhow::bail!(
                "unsupported threads schema: required column {} has no trusted source",
                column.name
            );
        }
    }
    Ok(())
}

fn known_index_columns() -> HashSet<&'static str> {
    [
        "id",
        "rollout_path",
        "created_at",
        "updated_at",
        "created_at_ms",
        "updated_at_ms",
        "recency_at",
        "recency_at_ms",
        "source",
        "thread_source",
        "model_provider",
        "cwd",
        "title",
        "sandbox_policy",
        "approval_mode",
        "tokens_used",
        "has_user_event",
        "archived",
        "archived_at",
        "cli_version",
        "first_user_message",
        "memory_mode",
        "preview",
        "history_mode",
        "is_pinned",
    ]
    .into_iter()
    .collect()
}

fn missing_index_records<'a>(
    db: &Connection,
    changes: &'a [SessionChange],
) -> anyhow::Result<Vec<&'a SessionIndexRecord>> {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    for record in changes
        .iter()
        .filter_map(|change| change.index_record.as_ref())
    {
        if !seen.insert(record.thread_id.as_str()) {
            continue;
        }
        let exists = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE id = ?1)",
            [&record.thread_id],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !exists {
            missing.push(record);
        }
    }
    Ok(missing)
}

fn insert_thread_index(
    tx: &Transaction<'_>,
    column_info: &[TableColumnInfo],
    record: &SessionIndexRecord,
    target_provider: &str,
) -> anyhow::Result<()> {
    let updated_at_ms = record.updated_at.saturating_mul(1_000);
    let created_at_ms = record.created_at.saturating_mul(1_000);
    let archived = i64::from(record.archived);
    let mut known = HashMap::<String, SqlValue>::from([
        ("id".to_string(), SqlValue::Text(record.thread_id.clone())),
        (
            "rollout_path".to_string(),
            SqlValue::Text(record.rollout_path.to_string_lossy().to_string()),
        ),
        (
            "created_at".to_string(),
            SqlValue::Integer(record.created_at),
        ),
        (
            "updated_at".to_string(),
            SqlValue::Integer(record.updated_at),
        ),
        (
            "created_at_ms".to_string(),
            SqlValue::Integer(created_at_ms),
        ),
        (
            "updated_at_ms".to_string(),
            SqlValue::Integer(updated_at_ms),
        ),
        (
            "recency_at".to_string(),
            SqlValue::Integer(record.updated_at),
        ),
        (
            "recency_at_ms".to_string(),
            SqlValue::Integer(updated_at_ms),
        ),
        ("source".to_string(), SqlValue::Text(record.source.clone())),
        (
            "thread_source".to_string(),
            SqlValue::Text(record.thread_source.clone()),
        ),
        (
            "model_provider".to_string(),
            SqlValue::Text(target_provider.to_string()),
        ),
        ("cwd".to_string(), SqlValue::Text(record.cwd.clone())),
        ("title".to_string(), SqlValue::Text(record.title.clone())),
        (
            "sandbox_policy".to_string(),
            SqlValue::Text(record.sandbox_policy.clone()),
        ),
        (
            "approval_mode".to_string(),
            SqlValue::Text(record.approval_mode.clone()),
        ),
        ("tokens_used".to_string(), SqlValue::Integer(0)),
        ("has_user_event".to_string(), SqlValue::Integer(1)),
        ("archived".to_string(), SqlValue::Integer(archived)),
        (
            "cli_version".to_string(),
            SqlValue::Text(record.cli_version.clone()),
        ),
        (
            "first_user_message".to_string(),
            SqlValue::Text(record.first_user_message.clone()),
        ),
        (
            "memory_mode".to_string(),
            SqlValue::Text("enabled".to_string()),
        ),
        (
            "preview".to_string(),
            SqlValue::Text(record.first_user_message.clone()),
        ),
        (
            "history_mode".to_string(),
            SqlValue::Text("legacy".to_string()),
        ),
        ("is_pinned".to_string(), SqlValue::Integer(0)),
    ]);
    if record.archived {
        known.insert(
            "archived_at".to_string(),
            SqlValue::Integer(record.updated_at),
        );
    }

    let mut columns = Vec::new();
    let mut values = Vec::new();
    for column in column_info {
        if let Some(value) = known.remove(&column.name) {
            columns.push(column.name.clone());
            values.push(value);
        } else if column.not_null && column.default_value.is_none() && !column.primary_key {
            anyhow::bail!(
                "cannot rebuild {}: required column {} has no trusted source",
                record.thread_id,
                column.name
            );
        }
    }
    let quoted_columns = columns
        .iter()
        .map(|column| format!("\"{}\"", column.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=values.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    tx.execute(
        &format!("INSERT INTO threads ({quoted_columns}) VALUES ({placeholders})"),
        params_from_iter(values),
    )?;
    Ok(())
}

fn load_global_state(path: &Path) -> anyhow::Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    Ok(serde_json::from_str::<Value>(&fs::read_to_string(path)?)?
        .as_object()
        .cloned()
        .unwrap_or_default())
}

fn load_projectless_thread_ids(path: &Path) -> anyhow::Result<HashSet<String>> {
    let state = load_global_state(path)?;
    let mut ids = HashSet::new();
    if let Some(items) = state
        .get("projectless-thread-ids")
        .and_then(Value::as_array)
    {
        for item in items {
            if let Some(id) = item.as_str().filter(|id| !id.trim().is_empty()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}

fn normalized_global_state(state: &Map<String, Value>) -> Map<String, Value> {
    let mut next = Map::new();
    if let Some(value) = state.get("electron-saved-workspace-roots") {
        next.insert(
            "electron-saved-workspace-roots".to_string(),
            json!(dedupe_paths(path_array(value))),
        );
    }
    if let Some(value) = state.get("project-order") {
        next.insert(
            "project-order".to_string(),
            json!(dedupe_paths(path_array(value))),
        );
    }
    if let Some(value) = state.get("active-workspace-roots") {
        let normalized = dedupe_paths(path_array(value));
        let next_value = if value.is_array() {
            json!(normalized)
        } else if let Some(first) = normalized.first() {
            json!(first)
        } else {
            value.clone()
        };
        next.insert("active-workspace-roots".to_string(), next_value);
    }
    if let Some(value) = state
        .get("electron-workspace-root-labels")
        .and_then(Value::as_object)
    {
        let mut labels = Map::new();
        for (key, item) in value {
            labels.insert(
                to_desktop_workspace_path(key).unwrap_or_else(|| key.clone()),
                item.clone(),
            );
        }
        next.insert(
            "electron-workspace-root-labels".to_string(),
            Value::Object(labels),
        );
    }
    if let Some(open_targets) = state
        .get("open-in-target-preferences")
        .and_then(Value::as_object)
    {
        let mut next_open_targets = open_targets.clone();
        if let Some(per_path) =
            copy_resolved_object_keys(open_targets.get("perPath").and_then(Value::as_object))
        {
            next_open_targets.insert("perPath".to_string(), Value::Object(per_path));
        }
        next.insert(
            "open-in-target-preferences".to_string(),
            Value::Object(next_open_targets),
        );
    }
    next
}

fn copy_resolved_object_keys(value: Option<&Map<String, Value>>) -> Option<Map<String, Value>> {
    let value = value?;
    let mut next = Map::new();
    for (key, item) in value {
        next.insert(
            to_desktop_workspace_path(key).unwrap_or_else(|| key.clone()),
            item.clone(),
        );
    }
    Some(next)
}

fn count_global_state_updates(path: &Path) -> anyhow::Result<usize> {
    let state = load_global_state(path)?;
    let next = normalized_global_state(&state);
    Ok(next
        .iter()
        .filter(|(key, value)| state.get(*key) != Some(*value))
        .count())
}

fn apply_global_state_update(path: &Path) -> anyhow::Result<usize> {
    let mut state = load_global_state(path)?;
    let next = normalized_global_state(&state);
    let count = next
        .iter()
        .filter(|(key, value)| state.get(*key) != Some(*value))
        .count();
    if count > 0 {
        for (key, value) in next {
            state.insert(key, value);
        }
        let text = serde_json::to_string_pretty(&Value::Object(state))?;
        fs::write(path, &text)?;
        if let Some(parent) = path.parent() {
            fs::write(parent.join(".codex-global-state.json.bak"), text)?;
        }
    }
    Ok(count)
}

fn path_array(value: &Value) -> Vec<String> {
    if let Some(items) = value.as_array() {
        items
            .iter()
            .filter_map(Value::as_str)
            .filter(|item| !item.trim().is_empty())
            .map(ToString::to_string)
            .collect()
    } else if let Some(value) = value.as_str().filter(|item| !item.trim().is_empty()) {
        vec![value.to_string()]
    } else {
        Vec::new()
    }
}

fn dedupe_paths(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        let Some(desktop) = to_desktop_workspace_path(&path) else {
            continue;
        };
        let comparable = desktop
            .replace('/', r"\")
            .trim_end_matches('\\')
            .to_ascii_lowercase();
        if seen.insert(comparable) {
            result.push(desktop);
        }
    }
    result
}

fn prune_backups(home: &Path) -> anyhow::Result<()> {
    let root = home.join("backups_state/provider-sync");
    if !root.exists() {
        return Ok(());
    }
    let mut managed = Vec::new();
    for entry in fs::read_dir(&root)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(text) = fs::read_to_string(path.join("metadata.json")) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value.get("managedBy").and_then(Value::as_str) == Some("CodexElves provider sync") {
            managed.push(path);
        }
    }
    managed.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for path in managed.into_iter().skip(BACKUP_KEEP_COUNT) {
        let _ = fs::remove_dir_all(path);
    }
    Ok(())
}

fn timestamp_name() -> String {
    chrono::Local::now().format("%Y%m%d%H%M%S").to_string()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
