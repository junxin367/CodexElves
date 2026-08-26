use codex_elves_core::models::{DeleteResult, DeleteStatus, SessionRef};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, ToSql, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

pub fn delete_local_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
) -> DeleteResult {
    let mut deleted_count = 0usize;
    let mut deleted_session_id = session.session_id.clone();
    let mut partial_count = 0usize;
    let mut failed_count = 0usize;
    let mut issues = Vec::new();
    let mut first_failure = None;
    for db_path in db_paths {
        if !db_path.is_file() {
            continue;
        }
        let adapter = SQLiteStorageAdapter::new(db_path);
        let candidate_result = adapter.delete_local(session);
        match candidate_result.status {
            DeleteStatus::LocalDeleted | DeleteStatus::ServerDeleted => {
                deleted_count += 1;
                deleted_session_id = candidate_result.session_id;
            }
            DeleteStatus::Partial => {
                deleted_count += 1;
                partial_count += 1;
                deleted_session_id = candidate_result.session_id;
                issues.push(candidate_result.message);
            }
            DeleteStatus::Failed => {
                failed_count += 1;
                issues.push(candidate_result.message.clone());
                first_failure.get_or_insert(candidate_result);
            }
            DeleteStatus::NotFound => {}
        }
    }
    if deleted_count == 0 {
        return first_failure.unwrap_or_else(|| {
            not_found(&session.session_id, "会话在本地存储中已不存在".to_string())
        });
    }
    if issues.is_empty() {
        return DeleteResult {
            status: DeleteStatus::LocalDeleted,
            session_id: deleted_session_id,
            message: if deleted_count > 1 {
                format!("已从 {deleted_count} 个本地存储删除")
            } else {
                "已从本地存储删除".to_string()
            },
        };
    }
    DeleteResult {
        status: DeleteStatus::Partial,
        session_id: deleted_session_id,
        message: if deleted_count == 1 && partial_count == 1 && failed_count == 0 {
            issues.remove(0)
        } else {
            format!(
                "已从 {deleted_count} 个本地存储删除，但部分清理失败：{}",
                issues.join("; ")
            )
        },
    }
}

pub fn move_codex_thread_workspace_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
    target_cwd: &str,
) -> Value {
    let mut result = json!({
        "status": "failed",
        "session_id": session.session_id,
        "message": "会话在本地存储中已不存在"
    });
    for db_path in db_paths {
        let adapter = SQLiteStorageAdapter::new(db_path);
        let candidate_result = adapter.move_codex_thread_workspace(session, target_cwd);
        if candidate_result.get("status").and_then(Value::as_str) == Some("moved") {
            return candidate_result;
        }
        result = candidate_result;
    }
    result
}

pub fn codex_thread_usage_history_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
) -> Value {
    codex_thread_usage_from_paths(db_paths, session, true)
}

pub fn codex_thread_usage_summary_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
) -> Value {
    codex_thread_usage_from_paths(db_paths, session, false)
}

fn codex_thread_usage_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    session: &SessionRef,
    include_history: bool,
) -> Value {
    let mut result = json!({
        "status": "failed",
        "session_id": session.session_id,
        "message": "会话在本地存储中已不存在",
        "history": []
    });
    let mut best: Option<(bool, i64, Value)> = None;
    for db_path in db_paths {
        let adapter = SQLiteStorageAdapter::new(db_path);
        let candidate = adapter.codex_thread_usage(session, include_history);
        if candidate.get("status").and_then(Value::as_str) != Some("ok") {
            result = candidate;
            continue;
        }
        let matched_by_id = candidate.get("matched_by").and_then(Value::as_str) == Some("id");
        let updated_at_ms = candidate
            .get("thread_updated_at_ms")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let replace = best
            .as_ref()
            .map(|(best_id, best_updated, _)| {
                matched_by_id && !best_id
                    || matched_by_id == *best_id && updated_at_ms > *best_updated
            })
            .unwrap_or(true);
        if replace {
            best = Some((matched_by_id, updated_at_ms, candidate));
        }
    }
    best.map(|(_, _, value)| value).unwrap_or(result)
}

#[derive(Debug, Clone)]
pub struct SQLiteStorageAdapter {
    db_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaKind {
    GenericSessions,
    CodexThreads,
    CodexAutomationRuns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSession {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model_provider: String,
    pub archived: bool,
    pub updated_at_ms: Option<i64>,
    pub rollout_path: String,
    pub db_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum LocalSessionCatalogWarning {
    DatabaseReadFailed { count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionCatalogEntry {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub model_provider: String,
    pub updated_at_ms: Option<i64>,
}

impl From<LocalSession> for LocalSessionCatalogEntry {
    fn from(session: LocalSession) -> Self {
        Self {
            id: session.id,
            title: session.title,
            cwd: session.cwd,
            model_provider: session.model_provider,
            updated_at_ms: session.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionCatalog {
    pub sessions: Vec<LocalSessionCatalogEntry>,
    pub warnings: Vec<LocalSessionCatalogWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalSessionCatalogError {
    #[error("all existing session databases failed to read ({count})")]
    AllExistingDatabasesFailed { count: usize },
}

pub fn aggregate_local_session_catalog(
    candidate_paths: &[PathBuf],
) -> Result<LocalSessionCatalog, LocalSessionCatalogError> {
    let mut seen_paths = HashSet::new();
    let mut sessions = Vec::new();
    let mut existing_database_count = 0usize;
    let mut successful_database_count = 0usize;
    let mut failed_database_count = 0usize;

    for candidate_path in candidate_paths {
        if !candidate_path.is_file() {
            continue;
        }

        let path_key = canonical_database_path_key(candidate_path);
        if !seen_paths.insert(path_key) {
            continue;
        }

        existing_database_count += 1;
        match SQLiteStorageAdapter::new(candidate_path).list_local_sessions() {
            Ok(mut database_sessions) => {
                successful_database_count += 1;
                sessions.append(&mut database_sessions);
            }
            Err(_) => failed_database_count += 1,
        }
    }

    if existing_database_count > 0 && successful_database_count == 0 {
        return Err(LocalSessionCatalogError::AllExistingDatabasesFailed {
            count: failed_database_count,
        });
    }

    sessions.retain(|session| {
        !session.archived && !session.id.trim().is_empty() && !session.cwd.trim().is_empty()
    });
    sessions.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut seen_session_ids = HashSet::new();
    sessions.retain(|session| seen_session_ids.insert(session.id.clone()));

    let warnings = if failed_database_count == 0 {
        Vec::new()
    } else {
        vec![LocalSessionCatalogWarning::DatabaseReadFailed {
            count: failed_database_count,
        }]
    };

    Ok(LocalSessionCatalog {
        sessions: sessions
            .into_iter()
            .map(LocalSessionCatalogEntry::from)
            .collect(),
        warnings,
    })
}

fn canonical_database_path_key(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        path
    }
}

impl SQLiteStorageAdapter {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn delete_local(&self, session: &SessionRef) -> DeleteResult {
        if !self.db_path.exists() {
            return failed(
                &session.session_id,
                format!("Database not found: {}", self.db_path.to_string_lossy()),
            );
        }
        let result = (|| -> anyhow::Result<DeleteResult> {
            let mut db = Connection::open(&self.db_path)?;
            match schema_kind(&db)? {
                Some(SchemaKind::GenericSessions) => self.delete_generic_session(&mut db, session),
                Some(SchemaKind::CodexThreads) => self.delete_codex_thread(&mut db, session),
                Some(SchemaKind::CodexAutomationRuns) => {
                    self.delete_codex_automation_run(&mut db, session)
                }
                None => Ok(failed(
                    &session.session_id,
                    "Unsupported local storage schema".to_string(),
                )),
            }
        })();
        result.unwrap_or_else(|err| failed(&session.session_id, err.to_string()))
    }

    pub fn list_local_sessions(&self) -> anyhow::Result<Vec<LocalSession>> {
        if !self.db_path.exists() {
            return Ok(Vec::new());
        }
        let db = Connection::open(&self.db_path)?;
        match schema_kind(&db)? {
            Some(SchemaKind::CodexThreads) => self.list_codex_threads(&db),
            Some(SchemaKind::CodexAutomationRuns) => self.list_codex_automation_runs(&db),
            _ => anyhow::bail!("Unsupported local storage schema"),
        }
    }

    fn list_codex_threads(&self, db: &Connection) -> anyhow::Result<Vec<LocalSession>> {
        let columns = table_columns(&db, "threads")?
            .into_iter()
            .collect::<HashSet<_>>();
        let title = match (columns.contains("name"), columns.contains("title")) {
            (true, true) => "COALESCE(NULLIF(TRIM(name), ''), title, '')",
            (true, false) => "COALESCE(NULLIF(TRIM(name), ''), '')",
            (false, true) => "COALESCE(title, '')",
            (false, false) => "''",
        };
        let cwd = optional_column_expression(&columns, "cwd", "''");
        let model_provider = optional_column_expression(&columns, "model_provider", "''");
        let archived = optional_column_expression(&columns, "archived", "0");
        let updated_at_ms = if columns.contains("updated_at_ms") {
            "updated_at_ms"
        } else if columns.contains("updated_at") {
            "updated_at * 1000"
        } else if columns.contains("created_at_ms") {
            "created_at_ms"
        } else {
            "NULL"
        };
        let rollout_path = optional_column_expression(&columns, "rollout_path", "''");
        let sql = format!(
            "SELECT id, {title}, {cwd}, {model_provider}, {archived}, {updated_at_ms}, {rollout_path}
             FROM threads
             ORDER BY COALESCE({updated_at_ms}, 0) DESC, id DESC"
        );
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(LocalSession {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                cwd: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                model_provider: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                archived: row.get::<_, Option<i64>>(4)?.unwrap_or_default() != 0,
                updated_at_ms: row.get(5)?,
                rollout_path: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                db_path: self.db_path.to_string_lossy().to_string(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn list_codex_automation_runs(&self, db: &Connection) -> anyhow::Result<Vec<LocalSession>> {
        let columns = table_columns(db, "automation_runs")?
            .into_iter()
            .collect::<HashSet<_>>();
        let title = optional_column_expression(&columns, "thread_title", "''");
        let cwd = optional_column_expression(&columns, "source_cwd", "''");
        let status = optional_column_expression(&columns, "status", "''");
        let updated_at = optional_column_expression(&columns, "updated_at", "NULL");
        let created_at = optional_column_expression(&columns, "created_at", "NULL");
        let sql = format!(
            "SELECT thread_id, {title}, {cwd}, {status}, {updated_at}, {created_at}
             FROM automation_runs
             WHERE COALESCE(thread_id, '') <> ''
             ORDER BY COALESCE({updated_at}, {created_at}, 0) DESC, thread_id DESC"
        );
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let updated_at_ms = row
                .get::<_, Option<i64>>(4)?
                .or(row.get::<_, Option<i64>>(5)?);
            Ok(LocalSession {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                cwd: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                model_provider: String::new(),
                archived: row
                    .get::<_, Option<String>>(3)?
                    .map(|status| status.eq_ignore_ascii_case("archived"))
                    .unwrap_or(false),
                updated_at_ms,
                rollout_path: String::new(),
                db_path: self.db_path.to_string_lossy().to_string(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn find_archived_thread_by_title(&self, title: &str) -> Option<SessionRef> {
        let db = Connection::open(&self.db_path).ok()?;
        if schema_kind(&db).ok().flatten() != Some(SchemaKind::CodexThreads)
            || !has_columns(&db, "threads", &["archived"]).ok()?
        {
            return None;
        }
        let mut stmt = db
            .prepare(
                "SELECT id, title FROM threads
                 WHERE archived = 1 AND (title = ?1 OR title LIKE ?2 OR ?1 LIKE '%' || title || '%')
                 ORDER BY archived_at DESC LIMIT 1",
            )
            .ok()?;
        let mut rows = stmt.query((title, format!("%{title}%"))).ok()?;
        let row = rows.next().ok().flatten()?;
        let id: String = row.get(0).ok()?;
        let row_title: Option<String> = row.get(1).ok()?;
        SessionRef::new(id, row_title.unwrap_or_else(|| title.to_string())).ok()
    }

    pub fn move_codex_thread_workspace(
        &self,
        session: &SessionRef,
        target_cwd: &str,
    ) -> serde_json::Value {
        let target = target_cwd.trim();
        if target.is_empty() {
            return json!({"status": "failed", "session_id": session.session_id, "message": "目标项目路径为空"});
        }
        if !self.db_path.exists() {
            return json!({"status": "failed", "session_id": session.session_id, "message": format!("Database not found: {}", self.db_path.to_string_lossy())});
        }
        let result = (|| -> anyhow::Result<Value> {
            let db = Connection::open(&self.db_path)?;
            if schema_kind(&db)? != Some(SchemaKind::CodexThreads)
                || !has_columns(&db, "threads", &["cwd", "rollout_path"])?
            {
                return Ok(
                    json!({"status": "failed", "session_id": session.session_id, "message": "Unsupported local storage schema"}),
                );
            }
            let thread_id = normalize_codex_thread_id(&session.session_id);
            let timestamp_columns = codex_thread_timestamp_columns(&db)?;
            let mut columns = vec![
                "id".to_string(),
                "title".to_string(),
                "cwd".to_string(),
                "rollout_path".to_string(),
            ];
            columns.extend(timestamp_columns);
            let sql = format!("SELECT {} FROM threads WHERE id = ?1", columns.join(", "));
            let mut stmt = db.prepare(&sql)?;
            let row = stmt.query_row([&thread_id], |row| {
                let mut data = Map::new();
                for (index, column) in columns.iter().enumerate() {
                    data.insert(column.clone(), sql_value_to_json(row.get_ref(index)?));
                }
                Ok(data)
            });
            let row = match row {
                Ok(row) => row,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Ok(
                        json!({"status": "failed", "session_id": thread_id, "message": "Thread not found in local storage"}),
                    );
                }
                Err(err) => return Err(err.into()),
            };
            let previous_cwd = row
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let rollout_path = row
                .get("rollout_path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            db.execute(
                "UPDATE threads SET cwd = ?1 WHERE id = ?2",
                (target, thread_id.as_str()),
            )?;
            let rollout = update_rollout_session_meta_cwd(&rollout_path, &thread_id, target);
            let mut payload = json!({
                "status": "moved",
                "session_id": thread_id,
                "message": "已移动对话",
                "previous_cwd": previous_cwd,
                "target_cwd": target,
                "rollout_updated": rollout.0,
                "rollout_error": rollout.1,
            });
            if let Some(payload) = payload.as_object_mut() {
                add_timestamp_payload(payload, &row);
                payload.insert(
                    "db_path".to_string(),
                    json!(self.db_path.to_string_lossy().to_string()),
                );
            }
            Ok(payload)
        })();
        result.unwrap_or_else(|err| json!({"status": "failed", "session_id": session.session_id, "message": err.to_string()}))
    }

    pub fn codex_thread_sort_key(&self, session: &SessionRef) -> serde_json::Value {
        if !self.db_path.exists() {
            return json!({"status": "failed", "session_id": session.session_id, "message": format!("Database not found: {}", self.db_path.to_string_lossy())});
        }
        let result = (|| -> anyhow::Result<Value> {
            let db = Connection::open(&self.db_path)?;
            if schema_kind(&db)? != Some(SchemaKind::CodexThreads) {
                return Ok(
                    json!({"status": "failed", "session_id": session.session_id, "message": "Unsupported local storage schema"}),
                );
            }
            let thread_id = normalize_codex_thread_id(&session.session_id);
            match fetch_thread_timestamp_payload(&db, &thread_id)? {
                Some(mut payload) => {
                    payload.insert("status".to_string(), json!("ok"));
                    payload.insert("session_id".to_string(), json!(thread_id));
                    Ok(Value::Object(payload))
                }
                None => Ok(
                    json!({"status": "failed", "session_id": thread_id, "message": "Thread not found in local storage"}),
                ),
            }
        })();
        result.unwrap_or_else(|err| json!({"status": "failed", "session_id": session.session_id, "message": err.to_string()}))
    }

    pub fn codex_thread_sort_keys(&self, sessions: &[SessionRef]) -> serde_json::Value {
        if !self.db_path.exists() {
            return json!({"status": "failed", "message": format!("Database not found: {}", self.db_path.to_string_lossy()), "sort_keys": []});
        }
        let thread_ids = sessions
            .iter()
            .filter(|session| !session.session_id.is_empty())
            .map(|session| normalize_codex_thread_id(&session.session_id))
            .fold(Vec::<String>::new(), |mut acc, id| {
                if !acc.contains(&id) && acc.len() < 200 {
                    acc.push(id);
                }
                acc
            });
        if thread_ids.is_empty() {
            return json!({"status": "ok", "sort_keys": []});
        }
        let result = (|| -> anyhow::Result<Value> {
            let db = Connection::open(&self.db_path)?;
            if schema_kind(&db)? != Some(SchemaKind::CodexThreads) {
                return Ok(
                    json!({"status": "failed", "message": "Unsupported local storage schema", "sort_keys": []}),
                );
            }
            let timestamp_payloads = fetch_thread_timestamp_payloads(&db, &thread_ids)?;
            let mut sort_keys = Vec::new();
            for thread_id in thread_ids {
                if let Some(mut payload) = timestamp_payloads.get(&thread_id).cloned() {
                    payload.insert("session_id".to_string(), json!(thread_id));
                    sort_keys.push(Value::Object(payload));
                }
            }
            Ok(json!({"status": "ok", "sort_keys": sort_keys}))
        })();
        result.unwrap_or_else(
            |err| json!({"status": "failed", "message": err.to_string(), "sort_keys": []}),
        )
    }

    pub fn codex_thread_usage_history(&self, session: &SessionRef) -> serde_json::Value {
        self.codex_thread_usage(session, true)
    }

    pub fn codex_thread_usage_summary(&self, session: &SessionRef) -> serde_json::Value {
        self.codex_thread_usage(session, false)
    }

    fn codex_thread_usage(&self, session: &SessionRef, include_history: bool) -> serde_json::Value {
        let result = (|| -> anyhow::Result<Value> {
            if !self.db_path.exists() {
                return Ok(json!({
                    "status": "failed",
                    "session_id": session.session_id,
                    "message": format!("Database not found: {}", self.db_path.to_string_lossy()),
                    "history": []
                }));
            }
            let db = Connection::open(&self.db_path)?;
            if schema_kind(&db)? != Some(SchemaKind::CodexThreads)
                || !has_columns(&db, "threads", &["rollout_path"])?
            {
                return Ok(json!({
                    "status": "failed",
                    "session_id": session.session_id,
                    "message": "Unsupported local storage schema",
                    "history": []
                }));
            }
            let requested_thread_id = normalize_codex_thread_id(&session.session_id);
            let Some(thread) = resolve_thread_usage_record(&db, session)? else {
                return Ok(json!({
                    "status": "failed",
                    "session_id": requested_thread_id,
                    "message": "Thread not found in local storage",
                    "history": []
                }));
            };
            let Some(rollout_path) = thread
                .rollout_path
                .clone()
                .filter(|path| !path.trim().is_empty())
            else {
                return Ok(json!({
                    "status": "failed",
                    "session_id": thread.id,
                    "message": "Thread rollout path is empty",
                    "history": []
                }));
            };
            let rollout = PathBuf::from(&rollout_path);
            if !rollout.is_file() {
                return Ok(json!({
                    "status": "failed",
                    "session_id": thread.id,
                    "message": format!("rollout file not found: {rollout_path}"),
                    "history": []
                }));
            }
            let graph = thread_usage_graph(&db, &thread)?;
            let mut reports = HashMap::new();
            reports.insert(
                thread.id.clone(),
                read_rollout_usage_history(&rollout, &thread.id, include_history)?,
            );
            let mut partial_errors = Vec::new();
            for node in graph.iter().skip(1) {
                let Some(record) = &node.record else {
                    continue;
                };
                let Some(path) = record
                    .rollout_path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                else {
                    partial_errors.push(json!({
                        "threadId": record.id,
                        "message": "Thread rollout path is empty",
                    }));
                    continue;
                };
                let path = PathBuf::from(path);
                if !path.is_file() {
                    partial_errors.push(json!({
                        "threadId": record.id,
                        "message": format!("rollout file not found: {}", path.to_string_lossy()),
                    }));
                    continue;
                }
                match read_rollout_usage_history(&path, &record.id, include_history) {
                    Ok(report) => {
                        reports.insert(record.id.clone(), report);
                    }
                    Err(error) => partial_errors.push(json!({
                        "threadId": record.id,
                        "message": error.to_string(),
                    })),
                }
            }

            let root_usage = reports
                .get(&thread.id)
                .expect("root rollout report must be available");
            let mut total_usage = root_usage.total_usage;
            let mut descendant_total_usage = TokenUsageTotals::default();
            let mut last_turn_usage = root_usage.last_turn_usage;
            let root_last_turn_id = root_usage.last_turn_id.clone();
            let mut root_turn_by_thread = HashMap::<String, Option<String>>::new();
            root_turn_by_thread.insert(thread.id.clone(), None);
            let mut included_thread_ids = vec![thread.id.clone()];
            let mut descendant_count = 0usize;
            let mut last_turn_descendant_count = 0usize;
            let mut unassociated_descendant_count = 0usize;
            let mut active_thread_count = usize::from(root_usage.task_running);
            let mut last_turn_running = root_usage.task_running;
            let mut observed_at = root_usage.observed_at.clone();

            for node in graph.iter().skip(1) {
                let root_turn_id = if node.depth == 1 {
                    root_usage
                        .spawned_child_turns
                        .get(&node.id)
                        .cloned()
                        .or_else(|| {
                            (root_usage.turn_count <= 1 && !root_last_turn_id.is_empty())
                                .then(|| root_last_turn_id.clone())
                        })
                } else {
                    node.parent_id
                        .as_ref()
                        .and_then(|parent_id| root_turn_by_thread.get(parent_id))
                        .cloned()
                        .flatten()
                };
                root_turn_by_thread.insert(node.id.clone(), root_turn_id.clone());
                let Some(report) = reports.get(&node.id) else {
                    continue;
                };
                let parent_report = node
                    .parent_id
                    .as_ref()
                    .and_then(|parent_id| reports.get(parent_id));
                let descendant_usage = match rollout_own_usage(
                    report,
                    parent_report,
                    node.parent_id.as_deref(),
                ) {
                    Some(usage) => usage,
                    None => {
                        partial_errors.push(json!({
                            "threadId": node.id,
                            "message": "Forked thread usage was excluded because its inherited parent usage could not be isolated",
                        }));
                        TokenUsageTotals::default()
                    }
                };
                included_thread_ids.push(node.id.clone());
                descendant_count += 1;
                total_usage.add(descendant_usage);
                descendant_total_usage.add(descendant_usage);
                if report.task_running {
                    active_thread_count += 1;
                }
                if report.observed_at > observed_at {
                    observed_at = report.observed_at.clone();
                }
                if root_turn_id.as_deref() == Some(root_last_turn_id.as_str())
                    && !root_last_turn_id.is_empty()
                {
                    last_turn_usage.add(descendant_usage);
                    last_turn_descendant_count += 1;
                    if report.task_running {
                        last_turn_running = true;
                    }
                } else if root_turn_id.is_none() {
                    unassociated_descendant_count += 1;
                }
            }

            let mut summary = serde_json::Map::new();
            summary.insert(
                "totalUsage".to_string(),
                token_usage_summary_value(total_usage),
            );
            summary.insert(
                "lastTurnUsage".to_string(),
                token_usage_summary_value(last_turn_usage),
            );
            summary.insert("lastTurnId".to_string(), json!(root_last_turn_id));
            if let Some(started_at) = root_usage.last_turn_started_at.as_deref() {
                summary.insert("lastTurnStartedAt".to_string(), json!(started_at));
            }
            if let Some(completed_at) = root_usage.last_turn_completed_at.as_deref() {
                summary.insert("lastTurnCompletedAt".to_string(), json!(completed_at));
            }
            summary.insert("observedAt".to_string(), json!(observed_at));
            summary.insert("turnCount".to_string(), json!(root_usage.turn_count));
            if descendant_count > 0 {
                summary.insert(
                    "ownTotalUsage".to_string(),
                    token_usage_summary_value(root_usage.total_usage),
                );
                summary.insert(
                    "descendantTotalUsage".to_string(),
                    token_usage_summary_value(descendant_total_usage),
                );
                summary.insert("descendantCount".to_string(), json!(descendant_count));
                summary.insert(
                    "lastTurnDescendantCount".to_string(),
                    json!(last_turn_descendant_count),
                );
                summary.insert("includedThreadIds".to_string(), json!(included_thread_ids));
                if unassociated_descendant_count > 0 {
                    summary.insert(
                        "unassociatedDescendantCount".to_string(),
                        json!(unassociated_descendant_count),
                    );
                }
            }
            if active_thread_count > 0 {
                summary.insert("isRunning".to_string(), json!(true));
                summary.insert("activeThreadCount".to_string(), json!(active_thread_count));
                summary.insert("lastTurnRunning".to_string(), json!(last_turn_running));
            }
            if !partial_errors.is_empty() {
                summary.insert("partialErrors".to_string(), json!(partial_errors));
            }
            let mut response = json!({
                "status": "ok",
                "session_id": thread.id,
                "requested_session_id": requested_thread_id,
                "title": thread.title,
                "matched_by": thread.matched_by,
                "thread_updated_at_ms": thread.updated_at_ms,
                "db_path": self.db_path.to_string_lossy().to_string(),
                "rollout_path": rollout_path,
                "summary": Value::Object(summary),
            });
            if include_history {
                response
                    .as_object_mut()
                    .expect("thread usage response should be an object")
                    .insert("history".to_string(), json!(root_usage.history));
            }
            Ok(response)
        })();
        let db_value = result.unwrap_or_else(|err| {
            json!({
                "status": "failed",
                "session_id": session.session_id,
                "message": err.to_string(),
                "history": []
            })
        });
        if db_value.get("status").and_then(Value::as_str) == Some("ok") {
            return db_value;
        }
        // db 路径失败（常见于 Codex 升级后换了 SQLite schema）时，不依赖 db，
        // 直接按 session_id 在 sessions 目录定位 rollout 文件兵底统计。
        if let Some(sessions_dir) = codex_sessions_dir_from_db_path(&self.db_path) {
            if let Some(rollout) =
                find_rollout_path_by_session_id(&sessions_dir, &session.session_id)
            {
                match rollout_only_usage_value(&session.session_id, &rollout, include_history) {
                    Ok(value) => return value,
                    Err(_) => return db_value,
                }
            }
        }
        db_value
    }

    fn delete_generic_session(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let exists = db
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1 LIMIT 1",
                [&session.session_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        }
        let delete_result = (|| -> anyhow::Result<()> {
            let tx = db.transaction()?;
            if has_table(&tx, "messages")? {
                tx.execute(
                    "DELETE FROM messages WHERE session_id = ?1",
                    [&session.session_id],
                )?;
            }
            tx.execute("DELETE FROM sessions WHERE id = ?1", [&session.session_id])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(err) = delete_result {
            return Ok(failed(&session.session_id, err.to_string()));
        }
        Ok(local_deleted(&session.session_id))
    }

    fn delete_codex_thread(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let thread_id = normalize_codex_thread_id(&session.session_id);
        let rollout_path = db
            .query_row(
                "SELECT rollout_path FROM threads WHERE id = ?1 LIMIT 1",
                [&thread_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        let Some(rollout_path) = rollout_path else {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        };
        let delete_result = (|| -> anyhow::Result<()> {
            let tx = db.transaction()?;
            delete_related_rows(&tx, "thread_dynamic_tools", "thread_id = ?1", &[&thread_id])?;
            delete_related_rows(&tx, "thread_goals", "thread_id = ?1", &[&thread_id])?;
            delete_related_rows(
                &tx,
                "thread_spawn_edges",
                "parent_thread_id = ?1 OR child_thread_id = ?1",
                &[&thread_id],
            )?;
            delete_related_rows(&tx, "stage1_outputs", "thread_id = ?1", &[&thread_id])?;
            if has_table(&tx, "agent_job_items")?
                && has_columns(&tx, "agent_job_items", &["assigned_thread_id"])?
            {
                tx.execute(
                    "UPDATE agent_job_items SET assigned_thread_id = NULL WHERE assigned_thread_id = ?1",
                    [&thread_id],
                )?;
            }
            tx.execute("DELETE FROM threads WHERE id = ?1", [&thread_id])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(err) = delete_result {
            return Ok(failed(&thread_id, err.to_string()));
        }
        let mut file_errors = Vec::new();
        if let Some(path) = rollout_path.filter(|path| !path.trim().is_empty()) {
            if let Err(err) = fs::remove_file(&path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    file_errors.push(format!("{path}: {err}"));
                }
            }
        }
        if !file_errors.is_empty() {
            return Ok(DeleteResult {
                status: DeleteStatus::Partial,
                session_id: thread_id,
                message: format!(
                    "本地数据库已删除，但文件删除失败：{}",
                    file_errors.join("; ")
                ),
            });
        }
        Ok(local_deleted(&thread_id))
    }

    fn delete_codex_automation_run(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let thread_id = normalize_codex_thread_id(&session.session_id);
        let automation_exists = has_table(db, "automation_runs")?
            && db.query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE thread_id = ?1",
                [&thread_id],
                |row| row.get::<_, i64>(0),
            )? > 0;
        let inbox_exists = has_table(db, "inbox_items")?
            && db.query_row(
                "SELECT COUNT(*) FROM inbox_items WHERE thread_id = ?1",
                [&thread_id],
                |row| row.get::<_, i64>(0),
            )? > 0;
        if !automation_exists && !inbox_exists {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        }
        let delete_result = (|| -> anyhow::Result<()> {
            let tx = db.transaction()?;
            delete_related_rows(&tx, "automation_runs", "thread_id = ?1", &[&thread_id])?;
            delete_related_rows(&tx, "inbox_items", "thread_id = ?1", &[&thread_id])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(err) = delete_result {
            return Ok(failed(&thread_id, err.to_string()));
        }
        Ok(local_deleted(&thread_id))
    }
}

#[derive(Debug, Clone)]
struct ThreadUsageRecord {
    id: String,
    title: String,
    rollout_path: Option<String>,
    updated_at_ms: i64,
    matched_by: &'static str,
}

#[derive(Debug)]
struct ThreadUsageGraphNode {
    id: String,
    parent_id: Option<String>,
    depth: usize,
    record: Option<ThreadUsageRecord>,
}

fn thread_usage_graph(
    db: &Connection,
    root: &ThreadUsageRecord,
) -> anyhow::Result<Vec<ThreadUsageGraphNode>> {
    let mut nodes = vec![ThreadUsageGraphNode {
        id: root.id.clone(),
        parent_id: None,
        depth: 0,
        record: Some(root.clone()),
    }];
    if !has_table(db, "thread_spawn_edges")?
        || !has_columns(
            db,
            "thread_spawn_edges",
            &["parent_thread_id", "child_thread_id"],
        )?
    {
        return Ok(nodes);
    }
    let columns = table_columns(db, "threads")?
        .into_iter()
        .collect::<HashSet<_>>();
    let updated_at_ms = if columns.contains("updated_at_ms") {
        "t.updated_at_ms"
    } else if columns.contains("updated_at") {
        "t.updated_at * 1000"
    } else if columns.contains("created_at_ms") {
        "t.created_at_ms"
    } else {
        "NULL"
    };
    let query = format!(
        "SELECT e.child_thread_id, t.id, t.title, t.rollout_path, {updated_at_ms}
         FROM thread_spawn_edges AS e
         LEFT JOIN threads AS t ON t.id = e.child_thread_id
         WHERE e.parent_thread_id = ?1
         ORDER BY e.child_thread_id"
    );
    let mut queue = VecDeque::from([(root.id.clone(), 0usize)]);
    let mut visited = HashSet::from([root.id.clone()]);
    while let Some((parent_id, depth)) = queue.pop_front() {
        if depth >= 64 {
            continue;
        }
        let mut statement = db.prepare(&query)?;
        let children = statement.query_map([parent_id.as_str()], |row| {
            let child_id: String = row.get(0)?;
            let record_id: Option<String> = row.get(1)?;
            let record = match record_id {
                Some(id) => Some(ThreadUsageRecord {
                    id,
                    title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    rollout_path: row.get(3)?,
                    updated_at_ms: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
                    matched_by: "descendant",
                }),
                None => None,
            };
            Ok((child_id, record))
        })?;
        for child in children {
            let (child_id, record) = child?;
            if !visited.insert(child_id.clone()) {
                continue;
            }
            let child_depth = depth + 1;
            nodes.push(ThreadUsageGraphNode {
                id: child_id.clone(),
                parent_id: Some(parent_id.clone()),
                depth: child_depth,
                record,
            });
            queue.push_back((child_id, child_depth));
        }
    }
    Ok(nodes)
}

fn resolve_thread_usage_record(
    db: &Connection,
    session: &SessionRef,
) -> anyhow::Result<Option<ThreadUsageRecord>> {
    let columns = table_columns(db, "threads")?
        .into_iter()
        .collect::<HashSet<_>>();
    let updated_at_ms = if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else if columns.contains("updated_at") {
        "updated_at * 1000"
    } else if columns.contains("created_at_ms") {
        "created_at_ms"
    } else {
        "NULL"
    };
    let select =
        format!("SELECT id, title, rollout_path, {updated_at_ms} AS sort_value FROM threads");
    let thread_id = normalize_codex_thread_id(&session.session_id);
    let exact = db
        .query_row(&format!("{select} WHERE id = ?1"), [&thread_id], |row| {
            Ok(ThreadUsageRecord {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                rollout_path: row.get(2)?,
                updated_at_ms: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                matched_by: "id",
            })
        })
        .optional()?;
    if exact.is_some() {
        return Ok(exact);
    }
    if !is_temporary_codex_thread_id(&thread_id) {
        return Ok(None);
    }
    let title = session.title.trim();
    let title_prefix = title.trim_end_matches('…').trim_end_matches("...").trim();
    if title_prefix.chars().count() < 8 {
        return Ok(None);
    }
    let fallback = db
        .query_row(
            &format!(
                "{select}
                 WHERE COALESCE(title, '') <> ''
                   AND (
                     title = ?1
                     OR substr(title, 1, length(?2)) = ?2
                     OR instr(?1, title) = 1
                   )
                 ORDER BY sort_value DESC, id DESC
                 LIMIT 1"
            ),
            (title, title_prefix),
            |row| {
                Ok(ThreadUsageRecord {
                    id: row.get(0)?,
                    title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    rollout_path: row.get(2)?,
                    updated_at_ms: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                    matched_by: "title",
                })
            },
        )
        .optional()?;
    Ok(fallback)
}

fn is_temporary_codex_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with("client-new-thread:") || thread_id.starts_with("new-thread:")
}

fn optional_column_expression<'a>(
    columns: &HashSet<String>,
    column: &'a str,
    fallback: &'a str,
) -> &'a str {
    if columns.contains(column) {
        column
    } else {
        fallback
    }
}

fn failed(session_id: &str, message: String) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::Failed,
        session_id: session_id.to_string(),
        message,
    }
}

/// 会话/thread 在本地存储中本来就不存在（非错误）。
/// 这种场景下“删除”的目标（会话不存在）其实已达成，
/// 前端据此可以直接移除残留的 UI 行。
fn not_found(session_id: &str, message: String) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::NotFound,
        session_id: session_id.to_string(),
        message,
    }
}

fn local_deleted(session_id: &str) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::LocalDeleted,
        session_id: session_id.to_string(),
        message: "已从本地存储删除".to_string(),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cached_tokens: u64,
    cache_creation_tokens: u64,
}

impl TokenUsageTotals {
    fn from_json(value: Option<&Value>) -> Self {
        let input_tokens = usage_u64(value, "input_tokens");
        let output_tokens = usage_u64(value, "output_tokens");
        let total_tokens =
            usage_u64(value, "total_tokens").max(input_tokens.saturating_add(output_tokens));
        let cached_tokens = value
            .and_then(|usage| {
                usage
                    .get("cached_input_tokens")
                    .or_else(|| usage.get("cache_read_input_tokens"))
            })
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let cache_creation_tokens = value
            .and_then(|usage| {
                usage
                    .get("cache_write_input_tokens")
                    .or_else(|| usage.get("cache_creation_input_tokens"))
            })
            .and_then(Value::as_u64)
            .unwrap_or_default();
        Self {
            input_tokens,
            output_tokens,
            total_tokens,
            cached_tokens,
            cache_creation_tokens,
        }
    }

    fn add(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(other.cache_creation_tokens);
    }

    fn fill_missing_from(&mut self, fallback: Self) {
        if self.input_tokens == 0 {
            self.input_tokens = fallback.input_tokens;
        }
        if self.output_tokens == 0 {
            self.output_tokens = fallback.output_tokens;
        }
        if self.total_tokens == 0 {
            self.total_tokens = fallback.total_tokens;
        }
        if self.cached_tokens == 0 {
            self.cached_tokens = fallback.cached_tokens;
        }
        if self.cache_creation_tokens == 0 {
            self.cache_creation_tokens = fallback.cache_creation_tokens;
        }
    }

    fn has_usage(self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.total_tokens > 0
            || self.cached_tokens > 0
            || self.cache_creation_tokens > 0
    }
}

#[derive(Debug, Clone)]
struct RolloutUsageReport {
    history: Vec<Value>,
    total_usage: TokenUsageTotals,
    turn_usage: HashMap<String, TokenUsageTotals>,
    last_turn_usage: TokenUsageTotals,
    last_turn_id: String,
    last_turn_started_at: Option<String>,
    last_turn_completed_at: Option<String>,
    observed_at: String,
    turn_count: usize,
    task_running: bool,
    spawned_child_turns: HashMap<String, String>,
    forked_from_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RolloutUsageParser {
    current_turn_id: String,
    history: Vec<Value>,
    turn_ids: HashSet<String>,
    turn_usage: HashMap<String, TokenUsageTotals>,
    active_task_turns: HashSet<String>,
    turn_started_at: HashMap<String, String>,
    turn_completed_at: HashMap<String, String>,
    spawned_child_turns: HashMap<String, String>,
    accumulated_total: TokenUsageTotals,
    latest_cumulative_total: Option<TokenUsageTotals>,
    latest_turn_id: String,
    latest_observed_at: String,
    forked_from_id: Option<String>,
    own_session_meta_seen: bool,
}

impl RolloutUsageParser {
    fn report(&self) -> RolloutUsageReport {
        let mut total_usage = self
            .latest_cumulative_total
            .unwrap_or(self.accumulated_total);
        total_usage.fill_missing_from(self.accumulated_total);
        let last_turn_usage = self
            .turn_usage
            .get(&self.latest_turn_id)
            .copied()
            .unwrap_or_default();
        RolloutUsageReport {
            history: self.history.clone(),
            total_usage,
            turn_usage: self.turn_usage.clone(),
            last_turn_usage,
            last_turn_id: self.latest_turn_id.clone(),
            last_turn_started_at: self.turn_started_at.get(&self.latest_turn_id).cloned(),
            last_turn_completed_at: self.turn_completed_at.get(&self.latest_turn_id).cloned(),
            observed_at: self.latest_observed_at.clone(),
            turn_count: self.turn_ids.len(),
            // Forked rollout files replay the parent's history before the
            // child's own turns. If the fork happened while the parent was
            // running, that inherited task_started event has no matching
            // completion in the child file and must not keep the child
            // running forever. A thread's runtime state is determined by its
            // latest turn; older unmatched turns are only inherited history.
            task_running: !self.latest_turn_id.is_empty()
                && self.active_task_turns.contains(&self.latest_turn_id),
            spawned_child_turns: self.spawned_child_turns.clone(),
            forked_from_id: self.forked_from_id.clone(),
        }
    }

    fn consume_line(&mut self, line: &str, thread_id: &str, include_history: bool) {
        if line.trim().is_empty() {
            return;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => return,
        };
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "session_meta" => {
                let Some(payload) = value.get("payload") else {
                    return;
                };
                let session_id = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
                    .and_then(Value::as_str);
                if !self.own_session_meta_seen && session_id == Some(thread_id) {
                    self.own_session_meta_seen = true;
                    self.forked_from_id = payload
                        .get("forked_from_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                }
            }
            "turn_context" => {
                self.current_turn_id = value
                    .get("payload")
                    .and_then(|payload| payload.get("turn_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !self.current_turn_id.is_empty() {
                    self.latest_turn_id = self.current_turn_id.clone();
                }
            }
            "event_msg" => {
                let Some(payload) = value.get("payload") else {
                    return;
                };
                let event_type = payload
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if event_type == "task_started" {
                    let turn_id = payload
                        .get("turn_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if !turn_id.is_empty() {
                        if let Some(timestamp) = rollout_event_timestamp(&value) {
                            self.turn_started_at
                                .entry(turn_id.clone())
                                .or_insert(timestamp);
                        }
                        self.active_task_turns.insert(turn_id.clone());
                        self.latest_turn_id = turn_id;
                    }
                    return;
                }
                if matches!(
                    event_type,
                    "task_complete" | "task_completed" | "task_aborted" | "turn_aborted"
                ) {
                    let turn_id = payload
                        .get("turn_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if turn_id.is_empty() {
                        self.active_task_turns.clear();
                    } else {
                        if let Some(timestamp) = rollout_event_timestamp(&value) {
                            self.turn_completed_at
                                .insert(turn_id.to_string(), timestamp);
                        }
                        self.active_task_turns.remove(turn_id);
                    }
                    return;
                }
                if event_type != "token_count" {
                    return;
                }
                let Some(info) = payload.get("info") else {
                    return;
                };
                let last = info.get("last_token_usage");
                let total = info.get("total_token_usage");
                let model_context_window = info
                    .get("model_context_window")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let last_usage = TokenUsageTotals::from_json(last);
                let total_usage = TokenUsageTotals::from_json(total);
                let context_used = total_usage.total_tokens.max(last_usage.total_tokens);
                if !last_usage.has_usage() && !total_usage.has_usage() {
                    return;
                }
                if total_usage.has_usage() && self.latest_cumulative_total == Some(total_usage) {
                    return;
                }
                self.accumulated_total.add(last_usage);
                if total_usage.has_usage() {
                    self.latest_cumulative_total = Some(total_usage);
                }
                let usage_turn_id = if self.current_turn_id.is_empty() {
                    self.latest_turn_id.clone()
                } else {
                    self.current_turn_id.clone()
                };
                if !usage_turn_id.is_empty() {
                    self.turn_usage
                        .entry(usage_turn_id.clone())
                        .or_default()
                        .add(last_usage);
                    self.turn_ids.insert(usage_turn_id.clone());
                    if self.latest_turn_id.is_empty() {
                        self.latest_turn_id = usage_turn_id.clone();
                    }
                }
                self.latest_observed_at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if include_history {
                    self.history.push(json!({
                        "source": "rollout-history",
                        "conversation_id": format!("local:{thread_id}"),
                        "turn_id": usage_turn_id,
                        "observed_at": self.latest_observed_at,
                        "usage": {
                            "inputTokens": last_usage.input_tokens,
                            "outputTokens": last_usage.output_tokens,
                            "totalTokens": last_usage.total_tokens,
                            "cachedTokens": last_usage.cached_tokens,
                            "cacheReadTokens": last_usage.cached_tokens,
                            "cacheCreationTokens": last_usage.cache_creation_tokens,
                            "contextUsed": context_used,
                            "contextLimit": model_context_window,
                            "hasBreakdown": last_usage.input_tokens > 0
                                || last_usage.output_tokens > 0
                                || last_usage.cached_tokens > 0
                                || last_usage.cache_creation_tokens > 0,
                        }
                    }));
                }
            }
            "response_item" => {
                let Some(payload) = value.get("payload") else {
                    return;
                };
                if payload.get("type").and_then(Value::as_str) != Some("function_call_output") {
                    return;
                }
                let Some(child_thread_id) = spawned_child_thread_id(payload) else {
                    return;
                };
                let turn_id = payload
                    .get("internal_chat_message_metadata_passthrough")
                    .and_then(|metadata| metadata.get("turn_id"))
                    .and_then(Value::as_str)
                    .unwrap_or(self.current_turn_id.as_str())
                    .trim();
                if !turn_id.is_empty() {
                    self.spawned_child_turns
                        .entry(child_thread_id)
                        .or_insert_with(|| turn_id.to_string());
                }
            }
            _ => {}
        }
    }
}

fn rollout_event_timestamp(value: &Value) -> Option<String> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|timestamp| !timestamp.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RolloutFileStamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

#[derive(Debug, Clone)]
struct CachedRolloutUsage {
    observed_len: u64,
    parsed_offset: u64,
    stamp: RolloutFileStamp,
    tail: Vec<u8>,
    parser: RolloutUsageParser,
    last_access: u64,
}

type RolloutUsageCacheKey = (PathBuf, String, bool);

#[derive(Default)]
struct RolloutUsageCache {
    entries: HashMap<RolloutUsageCacheKey, CachedRolloutUsage>,
    access_tick: u64,
}

// 父会话可能包含数百个 subagent。容量小于单个会话图时，顺序遍历会让 LRU
// 每轮都淘汰下一轮即将读取的条目，退化成反复全量解析。
const ROLLOUT_USAGE_CACHE_CAPACITY: usize = 512;
const ROLLOUT_USAGE_CACHE_TAIL_BYTES: u64 = 4096;

fn rollout_usage_cache() -> &'static Mutex<RolloutUsageCache> {
    static CACHE: OnceLock<Mutex<RolloutUsageCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(RolloutUsageCache::default()))
}

fn rollout_file_stamp(rollout_path: &Path) -> anyhow::Result<RolloutFileStamp> {
    let metadata = fs::metadata(rollout_path)?;
    Ok(RolloutFileStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    })
}

fn rollout_tail_matches(
    rollout_path: &Path,
    parsed_offset: u64,
    expected_tail: &[u8],
) -> anyhow::Result<bool> {
    if parsed_offset == 0 || expected_tail.is_empty() {
        return Ok(true);
    }
    let mut file = File::open(rollout_path)?;
    file.seek(SeekFrom::Start(
        parsed_offset.saturating_sub(expected_tail.len() as u64),
    ))?;
    let mut actual_tail = Vec::with_capacity(expected_tail.len());
    file.take(expected_tail.len() as u64)
        .read_to_end(&mut actual_tail)?;
    Ok(actual_tail == expected_tail)
}

fn rollout_tail(rollout_path: &Path, parsed_offset: u64) -> anyhow::Result<Vec<u8>> {
    if parsed_offset == 0 {
        return Ok(Vec::new());
    }
    let start = parsed_offset.saturating_sub(ROLLOUT_USAGE_CACHE_TAIL_BYTES);
    let mut file = File::open(rollout_path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::with_capacity((parsed_offset - start) as usize);
    file.take(parsed_offset - start).read_to_end(&mut tail)?;
    Ok(tail)
}

fn insert_rollout_usage_cache(cache_key: RolloutUsageCacheKey, entry: CachedRolloutUsage) {
    let mut cache = rollout_usage_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.access_tick = cache.access_tick.saturating_add(1);
    let access_tick = cache.access_tick;
    let mut entry = entry;
    entry.last_access = access_tick;
    if !cache.entries.contains_key(&cache_key)
        && cache.entries.len() >= ROLLOUT_USAGE_CACHE_CAPACITY
    {
        if let Some(oldest_key) = cache
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(path, _)| path.clone())
        {
            cache.entries.remove(&oldest_key);
        }
    }
    cache.entries.insert(cache_key, entry);
}

fn rollout_own_usage(
    report: &RolloutUsageReport,
    parent_report: Option<&RolloutUsageReport>,
    parent_id: Option<&str>,
) -> Option<TokenUsageTotals> {
    let Some(forked_from_id) = report.forked_from_id.as_deref() else {
        return Some(report.total_usage);
    };
    if parent_id != Some(forked_from_id) {
        return None;
    }
    let parent_report = parent_report?;
    let mut own_usage = TokenUsageTotals::default();
    for (turn_id, usage) in &report.turn_usage {
        if !parent_report.turn_usage.contains_key(turn_id) {
            own_usage.add(*usage);
        }
    }
    Some(own_usage)
}

fn usage_u64(value: Option<&Value>, key: &str) -> u64 {
    value
        .and_then(|usage| usage.get(key))
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

fn token_usage_summary_value(usage: TokenUsageTotals) -> Value {
    json!({
        "inputTokens": usage.input_tokens,
        "outputTokens": usage.output_tokens,
        "totalTokens": usage.total_tokens,
        "cachedTokens": usage.cached_tokens,
        "cacheCreationTokens": usage.cache_creation_tokens,
        "cacheTokens": usage.cached_tokens.saturating_add(usage.cache_creation_tokens),
    })
}

/// 从 db_path 推导 Codex 会话 rollout 目录（`~/.codex/sqlite/x.db` -> `~/.codex/sessions`）。
fn codex_sessions_dir_from_db_path(db_path: &Path) -> Option<PathBuf> {
    // db 可能在 `~/.codex/sqlite/x.db`（两层）或 `~/.codex/state_5.sqlite`（一层）。
    // 若父目录名为 `sqlite` 则 codex_home = 父.父，否则 codex_home = 父。
    let parent = db_path.parent()?;
    let codex_home = if parent.file_name().and_then(|name| name.to_str()) == Some("sqlite") {
        parent.parent()?
    } else {
        parent
    };
    let dir = codex_home.join("sessions");
    dir.is_dir().then_some(dir)
}

/// 不依赖 SQLite schema，直接按 session_id 在 sessions 目录递归定位 rollout 文件。
///
/// Codex rollout 命名规则稳定：`rollout-<时间戳>-<uuid>.jsonl`，文件名含会话 uuid。
/// 这样无论 Codex 如何改 db 表结构，只要 rollout 文件在就能统计 token。
fn find_rollout_path_by_session_id(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let needle = normalize_codex_thread_id(session_id);
    if needle.is_empty() {
        return None;
    }
    // 递归扫描（广度优先 + 深度上限），避免异常目录结构导致无限递归。
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((sessions_dir.to_path_buf(), 0));
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > 6 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                queue.push_back((path, depth + 1));
            } else if file_type.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("rollout-")
                    && name.ends_with(".jsonl")
                    && name.contains(&needle)
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// 基于单个 rollout 文件组装 token 统计（不含子会话图，用于 db schema 不兼容时的兵底）。
///
/// 输出 shape 与成功路径一致（只少 descendant 相关字段），前端无需区分。
fn rollout_only_usage_value(
    session_id: &str,
    rollout_path: &Path,
    include_history: bool,
) -> anyhow::Result<Value> {
    let thread_id = normalize_codex_thread_id(session_id);
    let report = read_rollout_usage_history(rollout_path, &thread_id, include_history)?;
    let mut summary = serde_json::Map::new();
    summary.insert(
        "totalUsage".to_string(),
        token_usage_summary_value(report.total_usage),
    );
    summary.insert(
        "lastTurnUsage".to_string(),
        token_usage_summary_value(report.last_turn_usage),
    );
    summary.insert("lastTurnId".to_string(), json!(report.last_turn_id));
    if let Some(started_at) = report.last_turn_started_at.as_deref() {
        summary.insert("lastTurnStartedAt".to_string(), json!(started_at));
    }
    if let Some(completed_at) = report.last_turn_completed_at.as_deref() {
        summary.insert("lastTurnCompletedAt".to_string(), json!(completed_at));
    }
    summary.insert("observedAt".to_string(), json!(report.observed_at));
    summary.insert("turnCount".to_string(), json!(report.turn_count));
    if report.task_running {
        summary.insert("isRunning".to_string(), json!(true));
        summary.insert("activeThreadCount".to_string(), json!(1));
        summary.insert("lastTurnRunning".to_string(), json!(true));
    }
    let mut response = json!({
        "status": "ok",
        "session_id": thread_id,
        "requested_session_id": thread_id,
        "matched_by": "rollout_file",
        "rollout_path": rollout_path.to_string_lossy().to_string(),
        "summary": Value::Object(summary),
    });
    if include_history {
        response
            .as_object_mut()
            .expect("rollout usage response should be an object")
            .insert("history".to_string(), json!(report.history));
    }
    Ok(response)
}

fn spawned_child_thread_id(payload: &Value) -> Option<String> {
    fn from_value(value: &Value) -> Option<String> {
        [
            "agent_id",
            "agentId",
            "thread_id",
            "threadId",
            "child_thread_id",
            "childThreadId",
        ]
        .into_iter()
        .find_map(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    }

    let output = payload.get("output")?;
    if let Some(thread_id) = from_value(output) {
        return Some(thread_id);
    }
    let text = output.as_str()?.trim();
    let parsed = serde_json::from_str::<Value>(text).ok()?;
    from_value(&parsed)
}

fn read_rollout_usage_history(
    rollout_path: &Path,
    thread_id: &str,
    include_history: bool,
) -> anyhow::Result<RolloutUsageReport> {
    let stamp = rollout_file_stamp(rollout_path)?;
    let cache_key = (
        rollout_path.to_path_buf(),
        thread_id.to_string(),
        include_history,
    );
    let cached_entry = {
        let mut cache = rollout_usage_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.access_tick = cache.access_tick.saturating_add(1);
        let access_tick = cache.access_tick;
        if let Some(entry) = cache.entries.get_mut(&cache_key) {
            entry.last_access = access_tick;
            Some(entry.clone())
        } else {
            None
        }
    };
    let (mut parser, start_offset) = if let Some(entry) = cached_entry {
        if entry.stamp == stamp {
            return Ok(entry.parser.report());
        }
        let can_continue = stamp.len > entry.observed_len
            && entry.stamp.created == stamp.created
            && rollout_tail_matches(rollout_path, entry.parsed_offset, &entry.tail)?;
        if can_continue {
            (entry.parser, entry.parsed_offset)
        } else {
            (RolloutUsageParser::default(), 0)
        }
    } else {
        (RolloutUsageParser::default(), 0)
    };

    let mut file = File::open(rollout_path)?;
    let remaining = stamp.len.saturating_sub(start_offset);
    file.seek(SeekFrom::Start(start_offset))?;
    let mut reader = BufReader::new(file.take(remaining));
    let mut parsed_offset = start_offset;
    let mut line = Vec::new();
    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            break;
        }
        if line.last().copied() != Some(b'\n') {
            break;
        }
        let line = std::str::from_utf8(&line)?;
        parser.consume_line(
            line.trim_end_matches(['\r', '\n']),
            thread_id,
            include_history,
        );
        parsed_offset = parsed_offset.saturating_add(bytes_read as u64);
    }

    let report = parser.report();
    insert_rollout_usage_cache(
        cache_key,
        CachedRolloutUsage {
            observed_len: stamp.len,
            parsed_offset,
            stamp,
            tail: rollout_tail(rollout_path, parsed_offset)?,
            parser,
            last_access: 0,
        },
    );
    Ok(report)
}

fn normalize_codex_thread_id(session_id: &str) -> String {
    session_id
        .strip_prefix("local:")
        .unwrap_or(session_id)
        .to_string()
}

fn schema_kind(db: &Connection) -> anyhow::Result<Option<SchemaKind>> {
    if has_table(db, "sessions")? && has_columns(db, "sessions", &["id", "title"])? {
        if has_table(db, "messages")? && !has_columns(db, "messages", &["session_id"])? {
            return Ok(None);
        }
        return Ok(Some(SchemaKind::GenericSessions));
    }
    if has_table(db, "threads")? && has_columns(db, "threads", &["id", "title", "rollout_path"])? {
        return Ok(Some(SchemaKind::CodexThreads));
    }
    if has_table(db, "automation_runs")? && has_columns(db, "automation_runs", &["thread_id"])? {
        return Ok(Some(SchemaKind::CodexAutomationRuns));
    }
    Ok(None)
}

fn has_table(db: &Connection, table: &str) -> anyhow::Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .is_ok())
}

fn has_columns(db: &Connection, table: &str, columns: &[&str]) -> anyhow::Result<bool> {
    let existing: HashSet<String> = table_columns(db, table)?.into_iter().collect();
    Ok(columns.iter().all(|column| existing.contains(*column)))
}

fn table_columns(db: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    let mut stmt = db.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table.replace('"', "\"\"")
    ))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn delete_related_rows(
    db: &Connection,
    table: &str,
    where_clause: &str,
    params: &[&dyn ToSql],
) -> anyhow::Result<()> {
    if has_table(db, table)? {
        db.execute(
            &format!("DELETE FROM \"{table}\" WHERE {where_clause}"),
            params,
        )?;
    }
    Ok(())
}

fn update_rollout_session_meta_cwd(
    rollout_path: &str,
    thread_id: &str,
    target_cwd: &str,
) -> (bool, String) {
    if rollout_path.is_empty() || !Path::new(rollout_path).is_file() {
        return (false, String::new());
    }
    let result = (|| -> anyhow::Result<bool> {
        let text = fs::read_to_string(rollout_path)?;
        let mut changed = false;
        let mut output = String::new();
        for line in text.split_inclusive('\n') {
            let (body, end) = line
                .strip_suffix('\n')
                .map_or((line, ""), |body| (body, "\n"));
            let mut raw = line.to_string();
            if let Ok(mut item) = serde_json::from_str::<Value>(body) {
                if item.get("type") == Some(&json!("session_meta"))
                    && item["payload"]["id"] == thread_id
                    && item["payload"]["cwd"] != target_cwd
                {
                    if let Some(payload) = item.get_mut("payload").and_then(Value::as_object_mut) {
                        payload.insert("cwd".to_string(), json!(target_cwd));
                        raw = serde_json::to_string(&item)? + end;
                        changed = true;
                    }
                }
            }
            output.push_str(&raw);
        }
        if changed {
            fs::write(rollout_path, output)?;
        }
        Ok(changed)
    })();
    match result {
        Ok(changed) => (changed, String::new()),
        Err(err) => (false, err.to_string()),
    }
}

fn codex_thread_timestamp_columns(db: &Connection) -> anyhow::Result<Vec<String>> {
    let existing: HashSet<String> = table_columns(db, "threads")?.into_iter().collect();
    Ok(["updated_at", "updated_at_ms", "created_at_ms"]
        .iter()
        .filter(|column| existing.contains(**column))
        .map(|column| column.to_string())
        .collect())
}

fn fetch_thread_timestamp_payload(
    db: &Connection,
    thread_id: &str,
) -> anyhow::Result<Option<Map<String, Value>>> {
    let mut payloads = fetch_thread_timestamp_payloads(db, &[thread_id.to_string()])?;
    Ok(payloads.remove(thread_id))
}

fn fetch_thread_timestamp_payloads(
    db: &Connection,
    thread_ids: &[String],
) -> anyhow::Result<HashMap<String, Map<String, Value>>> {
    if thread_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let timestamp_columns = codex_thread_timestamp_columns(db)?;
    let mut columns = vec!["id".to_string()];
    columns.extend(timestamp_columns);
    let mut payloads = HashMap::new();
    for thread_ids in thread_ids.chunks(200) {
        let placeholders = std::iter::repeat("?")
            .take(thread_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {} FROM threads WHERE id IN ({placeholders})",
            columns.join(", ")
        );
        let mut statement = db.prepare(&sql)?;
        let rows = statement.query_map(
            params_from_iter(thread_ids.iter()),
            |row| -> rusqlite::Result<(String, Map<String, Value>)> {
                let mut selected = Map::new();
                for (index, column) in columns.iter().enumerate() {
                    selected.insert(column.clone(), sql_value_to_json(row.get_ref(index)?));
                }
                let thread_id = selected
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Ok((thread_id, selected))
            },
        )?;
        for row in rows {
            let (thread_id, selected) = row?;
            let mut payload = Map::new();
            add_timestamp_payload(&mut payload, &selected);
            payloads.insert(thread_id, payload);
        }
    }
    Ok(payloads)
}

fn add_timestamp_payload(payload: &mut Map<String, Value>, row: &Map<String, Value>) {
    for column in ["updated_at", "updated_at_ms", "created_at_ms"] {
        payload.insert(
            column.to_string(),
            row.get(column).cloned().unwrap_or(Value::Null),
        );
    }
}

fn sql_value_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => json!(value),
        ValueRef::Real(value) => json!(value),
        ValueRef::Text(value) => json!(String::from_utf8_lossy(value).to_string()),
        ValueRef::Blob(value) => json!(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            value
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_usage_cache_retains_large_parent_thread_graph() {
        const GRAPH_SIZE: usize = 194;
        let prefix = format!("rollout-usage-cache-large-graph-{}", std::process::id());
        let keys = (0..GRAPH_SIZE)
            .map(|index| {
                (
                    PathBuf::from(format!("{prefix}-{index}.jsonl")),
                    format!("thread-{index}"),
                    false,
                )
            })
            .collect::<Vec<_>>();

        for key in &keys {
            insert_rollout_usage_cache(
                key.clone(),
                CachedRolloutUsage {
                    observed_len: 0,
                    parsed_offset: 0,
                    stamp: RolloutFileStamp {
                        len: 0,
                        modified: None,
                        created: None,
                    },
                    tail: Vec::new(),
                    parser: RolloutUsageParser::default(),
                    last_access: 0,
                },
            );
        }

        let retained = {
            let cache = rollout_usage_cache()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            keys.iter()
                .filter(|key| cache.entries.contains_key(*key))
                .count()
        };

        let mut cache = rollout_usage_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for key in &keys {
            cache.entries.remove(key);
        }

        assert_eq!(retained, GRAPH_SIZE);
    }
}
