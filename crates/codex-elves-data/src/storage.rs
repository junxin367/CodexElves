use crate::BackupStore;
use codex_elves_core::models::{DeleteResult, DeleteStatus, SessionRef};
use rusqlite::types::{ToSqlOutput, Value as SqlValue, ValueRef};
use rusqlite::{Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub fn delete_local_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    backup_store: BackupStore,
    session: &SessionRef,
) -> DeleteResult {
    let mut result = not_found(&session.session_id, "会话在本地存储中已不存在".to_string());
    let mut deleted_count = 0usize;
    for db_path in db_paths {
        let adapter = SQLiteStorageAdapter::new(db_path, backup_store.clone());
        let candidate_result = adapter.delete_local(session);
        if matches!(candidate_result.status, DeleteStatus::LocalDeleted) {
            deleted_count += 1;
            result = candidate_result;
        } else if deleted_count == 0 {
            result = candidate_result;
        }
    }
    if deleted_count > 1 {
        result.message = format!("已从 {deleted_count} 个本地存储删除");
    }
    result
}

pub fn move_codex_thread_workspace_from_paths(
    db_paths: impl IntoIterator<Item = PathBuf>,
    backup_store: BackupStore,
    session: &SessionRef,
    target_cwd: &str,
) -> Value {
    let mut result = json!({
        "status": "failed",
        "session_id": session.session_id,
        "message": "会话在本地存储中已不存在"
    });
    for db_path in db_paths {
        let adapter = SQLiteStorageAdapter::new(db_path, backup_store.clone());
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
    backup_store: BackupStore,
    session: &SessionRef,
) -> Value {
    let mut result = json!({
        "status": "failed",
        "session_id": session.session_id,
        "message": "会话在本地存储中已不存在",
        "history": []
    });
    let mut best: Option<(bool, i64, Value)> = None;
    for db_path in db_paths {
        let adapter = SQLiteStorageAdapter::new(db_path, backup_store.clone());
        let candidate = adapter.codex_thread_usage_history(session);
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
    backup_store: BackupStore,
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

#[derive(Debug, Clone)]
struct OwnedSqlValue(SqlValue);

impl ToSql for OwnedSqlValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(self.0.clone()))
    }
}

impl SQLiteStorageAdapter {
    pub fn new(db_path: impl Into<PathBuf>, backup_store: BackupStore) -> Self {
        Self {
            db_path: db_path.into(),
            backup_store,
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
        let title = optional_column_expression(&columns, "title", "''");
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

    pub fn undo(&self, token: &str) -> DeleteResult {
        let result = (|| -> anyhow::Result<DeleteResult> {
            let backup = self.backup_store.read_backup(token)?;
            let session_id = backup["session_id"].as_str().unwrap_or("").to_string();
            let mut db = Connection::open(&self.db_path)?;
            if let Some(tables) = backup["tables"].as_object() {
                validate_restore_tables(tables)?;
                detect_restore_conflicts(&db, tables)?;
                detect_file_restore_conflicts(tables)?;
                let tx = db.transaction()?;
                for (table, rows) in tables {
                    if table.starts_with("__") {
                        continue;
                    }
                    let Some(rows) = rows.as_array() else {
                        continue;
                    };
                    for row in rows {
                        if let Some(row) = row.as_object() {
                            if table == "agent_job_items"
                                && update_existing_agent_job_item(&tx, row)?
                            {
                                continue;
                            }
                            insert_row(&tx, table, row)?;
                        }
                    }
                }
                tx.commit()?;
                if let Some(files) = tables.get("__files").and_then(Value::as_array) {
                    for file in files {
                        let Some(path) = file.get("path").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(content) = file.get("content_b64").and_then(Value::as_str) else {
                            continue;
                        };
                        let bytes = base64::Engine::decode(
                            &base64::engine::general_purpose::STANDARD,
                            content,
                        )?;
                        if let Some(parent) = Path::new(path).parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(path, bytes)?;
                    }
                }
            }
            Ok(DeleteResult {
                status: DeleteStatus::Undone,
                session_id,
                message: "Local session restored from backup".to_string(),
                undo_token: Some(token.to_string()),
                backup_path: None,
            })
        })();
        result.unwrap_or_else(|err| failed_with_undo("", err.to_string(), token, None))
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
            let mut sort_keys = Vec::new();
            for thread_id in thread_ids {
                if let Some(mut payload) = fetch_thread_timestamp_payload(&db, &thread_id)? {
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
        if !self.db_path.exists() {
            return json!({
                "status": "failed",
                "session_id": session.session_id,
                "message": format!("Database not found: {}", self.db_path.to_string_lossy()),
                "history": []
            });
        }
        let result = (|| -> anyhow::Result<Value> {
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
                read_rollout_usage_history(&rollout, &thread.id)?,
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
                match read_rollout_usage_history(&path, &record.id) {
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
            Ok(json!({
                "status": "ok",
                "session_id": thread.id,
                "requested_session_id": requested_thread_id,
                "title": thread.title,
                "matched_by": thread.matched_by,
                "thread_updated_at_ms": thread.updated_at_ms,
                "db_path": self.db_path.to_string_lossy().to_string(),
                "rollout_path": rollout_path,
                "history": root_usage.history,
                "summary": Value::Object(summary),
            }))
        })();
        result.unwrap_or_else(|err| {
            json!({
                "status": "failed",
                "session_id": session.session_id,
                "message": err.to_string(),
                "history": []
            })
        })
    }

    fn delete_generic_session(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let sessions = select_dicts(
            db,
            "SELECT * FROM sessions WHERE id = ?1",
            &[&session.session_id],
        )?;
        if sessions.is_empty() {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        }
        let messages = if has_table(db, "messages")? {
            select_dicts(
                db,
                "SELECT * FROM messages WHERE session_id = ?1",
                &[&session.session_id],
            )?
        } else {
            Vec::new()
        };
        let token = self.backup_store.write_backup(
            &session.session_id,
            &self.db_path,
            json!({"sessions": sessions, "messages": messages}),
        )?;
        let backup_path = self.backup_store.path_for(&token);
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
            return Ok(failed_with_undo(
                &session.session_id,
                err.to_string(),
                &token,
                Some(&backup_path),
            ));
        }
        Ok(local_deleted(&session.session_id, &token, &backup_path))
    }

    fn delete_codex_thread(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let thread_id = normalize_codex_thread_id(&session.session_id);
        let thread_rows = select_dicts(db, "SELECT * FROM threads WHERE id = ?1", &[&thread_id])?;
        if thread_rows.is_empty() {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        }
        let mut tables = Map::new();
        tables.insert("threads".to_string(), Value::Array(thread_rows));
        backup_related_rows(
            db,
            &mut tables,
            "thread_dynamic_tools",
            "thread_id = ?1",
            &[&thread_id],
        )?;
        backup_related_rows(
            db,
            &mut tables,
            "thread_goals",
            "thread_id = ?1",
            &[&thread_id],
        )?;
        backup_related_rows(
            db,
            &mut tables,
            "thread_spawn_edges",
            "parent_thread_id = ?1 OR child_thread_id = ?1",
            &[&thread_id],
        )?;
        backup_related_rows(
            db,
            &mut tables,
            "stage1_outputs",
            "thread_id = ?1",
            &[&thread_id],
        )?;
        backup_related_rows(
            db,
            &mut tables,
            "agent_job_items",
            "assigned_thread_id = ?1",
            &[&thread_id],
        )?;
        let file_backups = rollout_file_backups(tables.get("threads").and_then(Value::as_array));
        if !file_backups.is_empty() {
            tables.insert("__files".to_string(), Value::Array(file_backups.clone()));
        }
        let token =
            self.backup_store
                .write_backup(&thread_id, &self.db_path, Value::Object(tables))?;
        let backup_path = self.backup_store.path_for(&token);
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
            return Ok(failed_with_undo(
                &thread_id,
                err.to_string(),
                &token,
                Some(&backup_path),
            ));
        }
        let mut file_errors = Vec::new();
        for file in file_backups {
            if let Some(path) = file.get("path").and_then(Value::as_str) {
                if let Err(err) = fs::remove_file(path) {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        file_errors.push(format!("{path}: {err}"));
                    }
                }
            }
        }
        if !file_errors.is_empty() {
            return Ok(DeleteResult {
                status: DeleteStatus::Failed,
                session_id: thread_id,
                message: format!(
                    "本地数据库已删除，但文件删除失败：{}",
                    file_errors.join("; ")
                ),
                undo_token: Some(token.clone()),
                backup_path: Some(backup_path.to_string_lossy().to_string()),
            });
        }
        Ok(local_deleted(&thread_id, &token, &backup_path))
    }

    fn delete_codex_automation_run(
        &self,
        db: &mut Connection,
        session: &SessionRef,
    ) -> anyhow::Result<DeleteResult> {
        let thread_id = normalize_codex_thread_id(&session.session_id);
        let mut tables = Map::new();
        backup_related_rows(
            db,
            &mut tables,
            "automation_runs",
            "thread_id = ?1",
            &[&thread_id],
        )?;
        backup_related_rows(
            db,
            &mut tables,
            "inbox_items",
            "thread_id = ?1",
            &[&thread_id],
        )?;
        if tables.values().all(|rows| {
            rows.as_array()
                .map(|items| items.is_empty())
                .unwrap_or(true)
        }) {
            return Ok(not_found(
                &session.session_id,
                "会话在本地存储中已不存在".to_string(),
            ));
        }
        let token =
            self.backup_store
                .write_backup(&thread_id, &self.db_path, Value::Object(tables))?;
        let backup_path = self.backup_store.path_for(&token);
        let delete_result = (|| -> anyhow::Result<()> {
            let tx = db.transaction()?;
            delete_related_rows(&tx, "automation_runs", "thread_id = ?1", &[&thread_id])?;
            delete_related_rows(&tx, "inbox_items", "thread_id = ?1", &[&thread_id])?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(err) = delete_result {
            return Ok(failed_with_undo(
                &thread_id,
                err.to_string(),
                &token,
                Some(&backup_path),
            ));
        }
        Ok(local_deleted(&thread_id, &token, &backup_path))
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
        undo_token: None,
        backup_path: None,
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
        undo_token: None,
        backup_path: None,
    }
}

fn local_deleted(session_id: &str, token: &str, backup_path: &Path) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::LocalDeleted,
        session_id: session_id.to_string(),
        message: "已从本地存储删除".to_string(),
        undo_token: Some(token.to_string()),
        backup_path: Some(backup_path.to_string_lossy().to_string()),
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

#[derive(Debug)]
struct RolloutUsageReport {
    history: Vec<Value>,
    total_usage: TokenUsageTotals,
    turn_usage: HashMap<String, TokenUsageTotals>,
    last_turn_usage: TokenUsageTotals,
    last_turn_id: String,
    observed_at: String,
    turn_count: usize,
    task_running: bool,
    spawned_child_turns: HashMap<String, String>,
    forked_from_id: Option<String>,
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
) -> anyhow::Result<RolloutUsageReport> {
    let file = File::open(rollout_path)?;
    let reader = BufReader::new(file);
    let mut current_turn_id = String::new();
    let mut history = Vec::new();
    let mut turn_ids = HashSet::new();
    let mut turn_usage = HashMap::<String, TokenUsageTotals>::new();
    let mut active_task_turns = HashSet::new();
    let mut spawned_child_turns = HashMap::new();
    let mut accumulated_total = TokenUsageTotals::default();
    let mut latest_cumulative_total = None;
    let mut latest_turn_id = String::new();
    let mut latest_observed_at = String::new();
    let mut forked_from_id = None;
    let mut own_session_meta_seen = false;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "session_meta" => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                let session_id = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
                    .and_then(Value::as_str);
                if !own_session_meta_seen && session_id == Some(thread_id) {
                    own_session_meta_seen = true;
                    forked_from_id = payload
                        .get("forked_from_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                }
            }
            "turn_context" => {
                current_turn_id = value
                    .get("payload")
                    .and_then(|payload| payload.get("turn_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !current_turn_id.is_empty() {
                    latest_turn_id = current_turn_id.clone();
                }
            }
            "event_msg" => {
                let Some(payload) = value.get("payload") else {
                    continue;
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
                        active_task_turns.insert(turn_id.clone());
                        latest_turn_id = turn_id;
                    }
                    continue;
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
                        active_task_turns.clear();
                    } else {
                        active_task_turns.remove(turn_id);
                    }
                    continue;
                }
                if event_type != "token_count" {
                    continue;
                }
                let info = match payload.get("info") {
                    Some(info) => info,
                    None => continue,
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
                    continue;
                }
                if total_usage.has_usage() && latest_cumulative_total == Some(total_usage) {
                    continue;
                }
                accumulated_total.add(last_usage);
                if total_usage.has_usage() {
                    latest_cumulative_total = Some(total_usage);
                }
                let usage_turn_id = if current_turn_id.is_empty() {
                    latest_turn_id.clone()
                } else {
                    current_turn_id.clone()
                };
                if !usage_turn_id.is_empty() {
                    turn_usage
                        .entry(usage_turn_id.clone())
                        .or_default()
                        .add(last_usage);
                    turn_ids.insert(usage_turn_id.clone());
                    if latest_turn_id.is_empty() {
                        latest_turn_id = usage_turn_id.clone();
                    }
                }
                latest_observed_at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                history.push(json!({
                    "source": "rollout-history",
                    "conversation_id": format!("local:{thread_id}"),
                    "turn_id": usage_turn_id,
                    "observed_at": latest_observed_at,
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
            "response_item" => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) != Some("function_call_output") {
                    continue;
                }
                let Some(child_thread_id) = spawned_child_thread_id(payload) else {
                    continue;
                };
                let turn_id = payload
                    .get("internal_chat_message_metadata_passthrough")
                    .and_then(|metadata| metadata.get("turn_id"))
                    .and_then(Value::as_str)
                    .unwrap_or(current_turn_id.as_str())
                    .trim();
                if !turn_id.is_empty() {
                    spawned_child_turns
                        .entry(child_thread_id)
                        .or_insert_with(|| turn_id.to_string());
                }
            }
            _ => {}
        }
    }

    let mut total_usage = latest_cumulative_total.unwrap_or(accumulated_total);
    total_usage.fill_missing_from(accumulated_total);
    let last_turn_usage = turn_usage.get(&latest_turn_id).copied().unwrap_or_default();
    Ok(RolloutUsageReport {
        history,
        total_usage,
        turn_usage,
        last_turn_usage,
        last_turn_id: latest_turn_id,
        observed_at: latest_observed_at,
        turn_count: turn_ids.len(),
        task_running: !active_task_turns.is_empty(),
        spawned_child_turns,
        forked_from_id,
    })
}

fn failed_with_undo(
    session_id: &str,
    message: String,
    token: &str,
    backup_path: Option<&Path>,
) -> DeleteResult {
    DeleteResult {
        status: DeleteStatus::Failed,
        session_id: session_id.to_string(),
        message,
        undo_token: Some(token.to_string()),
        backup_path: backup_path.map(|path| path.to_string_lossy().to_string()),
    }
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

fn select_dicts(db: &Connection, sql: &str, params: &[&dyn ToSql]) -> anyhow::Result<Vec<Value>> {
    let mut stmt = db.prepare(sql)?;
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let rows = stmt.query_map(params, |row| {
        let mut data = Map::new();
        for (index, column) in columns.iter().enumerate() {
            data.insert(column.clone(), sql_value_to_json(row.get_ref(index)?));
        }
        Ok(Value::Object(data))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn validate_restore_tables(tables: &Map<String, Value>) -> anyhow::Result<()> {
    let allowed = [
        "sessions",
        "messages",
        "threads",
        "thread_dynamic_tools",
        "thread_goals",
        "thread_spawn_edges",
        "stage1_outputs",
        "agent_job_items",
        "automation_runs",
        "inbox_items",
        "__files",
    ];
    for table in tables.keys() {
        if !allowed.contains(&table.as_str()) {
            anyhow::bail!("unknown restore table: {table}");
        }
    }
    Ok(())
}

fn detect_restore_conflicts(db: &Connection, tables: &Map<String, Value>) -> anyhow::Result<()> {
    for (table, rows) in tables {
        if table.starts_with("__") {
            continue;
        }
        let Some(rows) = rows.as_array() else {
            continue;
        };
        for row in rows {
            let Some(row) = row.as_object() else {
                continue;
            };
            if restore_row_conflicts(db, table, row)? {
                anyhow::bail!("restore conflict: {table} row already exists");
            }
        }
    }
    Ok(())
}

fn restore_row_conflicts(
    db: &Connection,
    table: &str,
    row: &Map<String, Value>,
) -> anyhow::Result<bool> {
    let key_columns = restore_conflict_key_columns(table, row);
    if key_columns.is_empty() || !has_table(db, table)? {
        return Ok(false);
    }
    let where_clause = key_columns
        .iter()
        .enumerate()
        .map(|(index, column)| format!("\"{}\" = ?{}", column.replace('"', "\"\""), index + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let values = key_columns
        .iter()
        .map(|column| OwnedSqlValue(json_to_sql_value(&row[*column])))
        .collect::<Vec<_>>();
    let refs = values
        .iter()
        .map(|value| value as &dyn ToSql)
        .collect::<Vec<_>>();
    Ok(db
        .query_row(
            &format!("SELECT 1 FROM \"{table}\" WHERE {where_clause} LIMIT 1"),
            refs.as_slice(),
            |_| Ok(()),
        )
        .is_ok())
}

fn restore_conflict_key_columns<'a>(table: &str, row: &'a Map<String, Value>) -> Vec<&'a String> {
    let wanted: &[&str] = match table {
        "sessions" | "threads" => &["id"],
        "messages" => &["id"],
        "automation_runs" | "inbox_items" => &["thread_id"],
        "thread_dynamic_tools" => &["thread_id", "tool_name"],
        "thread_goals" => &["thread_id", "goal"],
        "thread_spawn_edges" => &["parent_thread_id", "child_thread_id"],
        "stage1_outputs" => &["thread_id"],
        _ => &[],
    };
    let keys = wanted
        .iter()
        .filter_map(|column| row.get_key_value(*column).map(|(key, _)| key))
        .collect::<Vec<_>>();
    if table == "messages" && keys.is_empty() {
        row.get_key_value("session_id")
            .map(|(key, _)| vec![key])
            .unwrap_or_default()
    } else {
        keys
    }
}

fn detect_file_restore_conflicts(tables: &Map<String, Value>) -> anyhow::Result<()> {
    let Some(files) = tables.get("__files").and_then(Value::as_array) else {
        return Ok(());
    };
    let allowed_paths = allowed_backup_file_paths(tables);
    for file in files {
        if let Some(path) = file.get("path").and_then(Value::as_str) {
            if !allowed_paths.contains(path) {
                anyhow::bail!("unexpected backup file path: {path}");
            }
            if Path::new(path).exists() {
                anyhow::bail!("restore conflict: file already exists: {path}");
            }
        }
    }
    Ok(())
}

fn allowed_backup_file_paths(tables: &Map<String, Value>) -> HashSet<String> {
    tables
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("rollout_path").and_then(Value::as_str))
        .filter(|path| !path.trim().is_empty())
        .map(ToString::to_string)
        .collect()
}

fn insert_row(db: &Connection, table: &str, row: &Map<String, Value>) -> anyhow::Result<()> {
    let columns: Vec<&String> = row.keys().collect();
    if columns.is_empty() {
        return Ok(());
    }
    let quoted = columns
        .iter()
        .map(|column| format!("\"{}\"", column.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let marks = (0..columns.len())
        .map(|index| format!("?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let values = columns
        .iter()
        .map(|column| OwnedSqlValue(json_to_sql_value(&row[*column])))
        .collect::<Vec<_>>();
    let refs = values
        .iter()
        .map(|value| value as &dyn ToSql)
        .collect::<Vec<_>>();
    db.execute(
        &format!("INSERT INTO \"{table}\" ({quoted}) VALUES ({marks})"),
        refs.as_slice(),
    )?;
    Ok(())
}

fn update_existing_agent_job_item(
    db: &Connection,
    row: &Map<String, Value>,
) -> anyhow::Result<bool> {
    let Some(id) = row.get("id") else {
        return Ok(false);
    };
    if !row.contains_key("assigned_thread_id") || !has_table(db, "agent_job_items")? {
        return Ok(false);
    }
    let id_value = OwnedSqlValue(json_to_sql_value(id));
    let current_assignment = db.query_row(
        "SELECT assigned_thread_id FROM agent_job_items WHERE id = ?1 LIMIT 1",
        [&id_value as &dyn ToSql],
        |row| row.get::<_, Option<String>>(0),
    );
    let current_assignment = match current_assignment {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    if current_assignment.is_some() {
        anyhow::bail!("restore conflict: agent_job_items row already assigned");
    }
    let assigned = OwnedSqlValue(json_to_sql_value(&row["assigned_thread_id"]));
    db.execute(
        "UPDATE agent_job_items SET assigned_thread_id = ?1 WHERE id = ?2 AND assigned_thread_id IS NULL",
        [&assigned as &dyn ToSql, &id_value as &dyn ToSql],
    )?;
    Ok(true)
}

fn backup_related_rows(
    db: &Connection,
    tables: &mut Map<String, Value>,
    table: &str,
    where_clause: &str,
    params: &[&dyn ToSql],
) -> anyhow::Result<()> {
    if has_table(db, table)? {
        let rows = select_dicts(
            db,
            &format!("SELECT * FROM \"{table}\" WHERE {where_clause}"),
            params,
        )?;
        tables.insert(table.to_string(), Value::Array(rows));
    }
    Ok(())
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

fn rollout_file_backups(thread_rows: Option<&Vec<Value>>) -> Vec<Value> {
    thread_rows
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("rollout_path").and_then(Value::as_str))
        .filter_map(|path| {
            let bytes = fs::read(path).ok()?;
            Some(json!({
                "path": path,
                "content_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
            }))
        })
        .collect()
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
    let timestamp_columns = codex_thread_timestamp_columns(db)?;
    let mut columns = vec!["id".to_string()];
    columns.extend(timestamp_columns);
    let sql = format!("SELECT {} FROM threads WHERE id = ?1", columns.join(", "));
    let mut stmt = db.prepare(&sql)?;
    let row = stmt.query_row([thread_id], |row| {
        let mut selected = Map::new();
        for (index, column) in columns.iter().enumerate() {
            selected.insert(column.clone(), sql_value_to_json(row.get_ref(index)?));
        }
        Ok(selected)
    });
    match row {
        Ok(row) => {
            let mut payload = Map::new();
            add_timestamp_payload(&mut payload, &row);
            Ok(Some(payload))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
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

fn json_to_sql_value(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(value) => SqlValue::Integer(i64::from(*value)),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                SqlValue::Integer(value)
            } else if let Some(value) = number.as_f64() {
                SqlValue::Real(value)
            } else {
                SqlValue::Text(number.to_string())
            }
        }
        Value::String(value) => SqlValue::Text(value.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}
