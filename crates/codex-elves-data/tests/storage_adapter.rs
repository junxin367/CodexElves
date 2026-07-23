use codex_elves_core::models::{DeleteStatus, SessionRef};
use codex_elves_data::{
    BackupStore, SQLiteStorageAdapter, codex_thread_usage_history_from_paths,
    delete_local_from_paths, move_codex_thread_workspace_from_paths, undo_local_from_backup,
};
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

fn session(id: &str, title: &str) -> SessionRef {
    SessionRef::new(id, title).unwrap()
}

fn create_supported_db(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT NOT NULL, body TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO sessions (id, title) VALUES ('s1', 'First')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO messages (session_id, body) VALUES ('s1', 'hello')",
        [],
    )
    .unwrap();
}

fn create_codex_thread_db(path: &Path, rollout_path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT, cwd TEXT, archived INTEGER, archived_at INTEGER, updated_at INTEGER, updated_at_ms INTEGER)", []).unwrap();
    db.execute(
        "CREATE TABLE thread_dynamic_tools (thread_id TEXT NOT NULL, tool_name TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TABLE thread_goals (thread_id TEXT NOT NULL, goal TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute("CREATE TABLE thread_spawn_edges (parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL, status TEXT NOT NULL)", []).unwrap();
    db.execute(
        "CREATE TABLE stage1_outputs (thread_id TEXT NOT NULL, output TEXT NOT NULL)",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TABLE agent_job_items (id TEXT PRIMARY KEY, assigned_thread_id TEXT)",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('t1', ?1, 'Codex Thread', '/old/project', 0, NULL, 100, 100000)", [rollout_path.to_string_lossy().to_string()]).unwrap();
    db.execute(
        "INSERT INTO thread_dynamic_tools (thread_id, tool_name) VALUES ('t1', 'Read')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_goals (thread_id, goal) VALUES ('t1', 'delete me')",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status) VALUES ('t1', 'child', 'running')", []).unwrap();
    db.execute("INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status) VALUES ('parent', 't1', 'done')", []).unwrap();
    db.execute(
        "INSERT INTO stage1_outputs (thread_id, output) VALUES ('t1', 'cached')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO agent_job_items (id, assigned_thread_id) VALUES ('job1', 't1')",
        [],
    )
    .unwrap();
}

fn overwrite_backup_source_db(backup_store: &BackupStore, token: &str, source_db: &Path) {
    let backup_path = backup_store.path_for(token);
    let mut backup = backup_store.read_backup(token).unwrap();
    backup["source_db"] = json!(source_db.to_string_lossy().to_string());
    fs::write(backup_path, serde_json::to_string_pretty(&backup).unwrap()).unwrap();
}

fn thread_count(path: &Path, id: &str) -> i64 {
    let db = Connection::open(path).unwrap();
    db.query_row("SELECT COUNT(*) FROM threads WHERE id = ?1", [id], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap()
}

#[test]
fn backup_store_writes_reads_and_sanitizes_tokens() {
    let tmp = tempdir().unwrap();
    let store = BackupStore::new(tmp.path());

    let token = store
        .write_backup(
            "s1",
            Path::new("C:/state/codex.sqlite"),
            json!({"sessions": [{"id": "s1", "title": "Hello"}]}),
        )
        .unwrap();
    let backup = store.read_backup(&token).unwrap();

    assert_eq!(backup["session_id"], "s1");
    assert_eq!(backup["source_db"], "C:/state/codex.sqlite");
    assert_eq!(backup["tables"]["sessions"][0]["title"], "Hello");
    assert_eq!(
        store.path_for("../bad token!").file_name().unwrap(),
        "badtoken.json"
    );
    assert!(store.read_backup("missing").is_err());
}

#[test]
fn delete_local_session_creates_backup_and_undo_restores_rows() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex.sqlite");
    create_supported_db(&db_path);
    let backup_store = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup_store.clone());

    let deleted = adapter.delete_local(&session("s1", "First"));

    assert_eq!(deleted.status, DeleteStatus::LocalDeleted);
    assert_eq!(deleted.message, "已从本地存储删除");
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sessions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);

    let restored = undo_local_from_backup(
        vec![db_path.clone()],
        backup_store,
        deleted.undo_token.as_deref().unwrap(),
    );

    assert_eq!(restored.status, DeleteStatus::Undone);
    assert_eq!(restored.message, "Local session restored from backup");
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM sessions WHERE id = 's1'", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "First"
    );
    assert_eq!(
        db.query_row(
            "SELECT body FROM messages WHERE session_id = 's1'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "hello"
    );
}

#[test]
fn undo_fails_on_existing_db_row_conflict_without_overwriting_new_row() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex.sqlite");
    create_supported_db(&db_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));
    let deleted = adapter.delete_local(&session("s1", "First"));
    let token = deleted.undo_token.as_deref().unwrap();
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "INSERT INTO sessions (id, title) VALUES ('s1', 'New Session')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO messages (session_id, body) VALUES ('s1', 'new body')",
        [],
    )
    .unwrap();
    drop(db);

    let restored = adapter.undo(token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(restored.undo_token.as_deref(), Some(token));
    assert!(restored.message.to_lowercase().contains("restore conflict"));
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM sessions WHERE id = 's1'", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "New Session"
    );
    assert_eq!(
        db.query_row(
            "SELECT body FROM messages WHERE session_id = 's1'",
            [],
            |row| { row.get::<_, String>(0) }
        )
        .unwrap(),
        "new body"
    );
}

#[test]
fn undo_fails_on_existing_rollout_file_conflict_without_overwriting_new_file() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "old rollout\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));
    let deleted = adapter.delete_local(&session("t1", "Codex Thread"));
    let token = deleted.undo_token.as_deref().unwrap();
    fs::write(&rollout_path, "new rollout\n").unwrap();

    let restored = adapter.undo(token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(restored.undo_token.as_deref(), Some(token));
    assert!(restored.message.to_lowercase().contains("restore conflict"));
    assert_eq!(fs::read_to_string(&rollout_path).unwrap(), "new rollout\n");
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn undo_fails_for_unknown_backup_table_without_executing_it() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex.sqlite");
    create_supported_db(&db_path);
    let backup_store = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup_store.clone());
    let deleted = adapter.delete_local(&session("s1", "First"));
    let token = deleted.undo_token.as_deref().unwrap();
    let backup_path = backup_store.path_for(token);
    let mut backup: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&backup_path).unwrap()).unwrap();
    backup["tables"]["evil_table"] = json!([{"id": "owned"}]);
    fs::write(&backup_path, serde_json::to_string_pretty(&backup).unwrap()).unwrap();

    let restored = adapter.undo(token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(restored.undo_token.as_deref(), Some(token));
    assert!(
        restored
            .message
            .to_lowercase()
            .contains("unknown restore table")
    );
    let db = Connection::open(&db_path).unwrap();
    let table_exists = db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'evil_table'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    assert!(!table_exists);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sessions WHERE id = 's1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn undo_rejects_backup_file_paths_outside_thread_rollouts() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    let outside_path = tmp.path().join("outside.txt");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let backup_store = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup_store.clone());
    let deleted = adapter.delete_local(&session("t1", "Codex Thread"));
    let token = deleted.undo_token.as_deref().unwrap();
    let backup_path = backup_store.path_for(token);
    let mut backup: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&backup_path).unwrap()).unwrap();
    backup["tables"]["__files"] = json!([{
        "path": outside_path.to_string_lossy().to_string(),
        "content_b64": "b3duZWQ="
    }]);
    fs::write(&backup_path, serde_json::to_string_pretty(&backup).unwrap()).unwrap();

    let restored = adapter.undo(token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(restored.undo_token.as_deref(), Some(token));
    assert!(
        restored
            .message
            .to_lowercase()
            .contains("unexpected backup file path")
    );
    assert!(!outside_path.exists());
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn generic_delete_rolls_back_when_later_delete_fails() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex.sqlite");
    create_supported_db(&db_path);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TRIGGER fail_session_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .unwrap();
    drop(db);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let result = adapter.delete_local(&session("s1", "First"));

    assert_eq!(result.status, DeleteStatus::Failed);
    assert!(result.undo_token.is_some());
    assert!(result.backup_path.is_some());
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sessions WHERE id = 's1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = 's1'",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        1
    );
}

#[test]
fn delete_codex_thread_schema_removes_related_rows_file_and_undo_restores_everything() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let deleted = adapter.delete_local(&session("local:t1", "Codex Thread"));

    assert_eq!(deleted.status, DeleteStatus::LocalDeleted);
    assert!(!rollout_path.exists());
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT assigned_thread_id FROM agent_job_items WHERE id = 'job1'",
            [],
            |row| row.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    drop(db);

    let restored = adapter.undo(deleted.undo_token.as_deref().unwrap());

    assert_eq!(restored.status, DeleteStatus::Undone);
    assert_eq!(
        fs::read_to_string(&rollout_path).unwrap(),
        "{\"type\":\"message\"}\n"
    );
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT title FROM threads WHERE id = 't1'", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "Codex Thread"
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM thread_spawn_edges WHERE parent_thread_id = 't1' OR child_thread_id = 't1'", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT assigned_thread_id FROM agent_job_items WHERE id = 'job1'",
            [],
            |row| row.get::<_, Option<String>>(0)
        )
        .unwrap(),
        Some("t1".to_string())
    );
}

#[test]
fn delete_local_from_paths_removes_duplicate_threads_from_all_databases() {
    let tmp = tempdir().unwrap();
    let first_db = tmp.path().join("first.sqlite");
    let second_db = tmp.path().join("second.sqlite");
    let first_rollout = tmp.path().join("first.jsonl");
    let second_rollout = tmp.path().join("second.jsonl");
    fs::write(&first_rollout, "{\"type\":\"message\"}\n").unwrap();
    fs::write(&second_rollout, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&first_db, &first_rollout);
    create_codex_thread_db(&second_db, &second_rollout);

    let result = delete_local_from_paths(
        vec![first_db.clone(), second_db.clone()],
        BackupStore::new(tmp.path().join("backups")),
        &session("t1", "Codex Thread"),
    );

    assert_eq!(result.status, DeleteStatus::LocalDeleted);
    assert_eq!(result.message, "已从 2 个本地存储删除");
    assert_eq!(thread_count(&first_db, "t1"), 0);
    assert_eq!(thread_count(&second_db, "t1"), 0);
    assert!(!first_rollout.exists());
    assert!(!second_rollout.exists());
}

#[test]
fn multi_database_delete_undo_restores_all_source_databases() {
    let tmp = tempdir().unwrap();
    let first_db = tmp.path().join("first.sqlite");
    let second_db = tmp.path().join("second.sqlite");
    let first_rollout = tmp.path().join("first.jsonl");
    let second_rollout = tmp.path().join("second.jsonl");
    fs::write(
        &first_rollout,
        "{\"type\":\"message\",\"source\":\"first\"}\n",
    )
    .unwrap();
    fs::write(
        &second_rollout,
        "{\"type\":\"message\",\"source\":\"second\"}\n",
    )
    .unwrap();
    create_codex_thread_db(&first_db, &first_rollout);
    create_codex_thread_db(&second_db, &second_rollout);
    let backup_store = BackupStore::new(tmp.path().join("backups"));

    let deleted = delete_local_from_paths(
        vec![first_db.clone(), second_db.clone()],
        backup_store.clone(),
        &session("t1", "Codex Thread"),
    );

    assert_eq!(deleted.status, DeleteStatus::LocalDeleted);
    assert_eq!(deleted.message, "已从 2 个本地存储删除");
    let undo_token = deleted.undo_token.as_deref().unwrap();
    assert_eq!(
        backup_store.read_backup(undo_token).unwrap()["kind"],
        "multi_database_delete"
    );
    assert_eq!(thread_count(&first_db, "t1"), 0);
    assert_eq!(thread_count(&second_db, "t1"), 0);
    assert!(!first_rollout.exists());
    assert!(!second_rollout.exists());

    let restored = undo_local_from_backup(
        vec![first_db.clone(), second_db.clone()],
        backup_store,
        undo_token,
    );

    assert_eq!(restored.status, DeleteStatus::Undone);
    assert_eq!(restored.message, "已从 2 个本地存储恢复");
    assert_eq!(restored.undo_token.as_deref(), Some(undo_token));
    assert_eq!(thread_count(&first_db, "t1"), 1);
    assert_eq!(thread_count(&second_db, "t1"), 1);
    assert_eq!(
        fs::read_to_string(&first_rollout).unwrap(),
        "{\"type\":\"message\",\"source\":\"first\"}\n"
    );
    assert_eq!(
        fs::read_to_string(&second_rollout).unwrap(),
        "{\"type\":\"message\",\"source\":\"second\"}\n"
    );
}

#[test]
fn multi_database_undo_preflight_conflict_restores_neither_database() {
    let tmp = tempdir().unwrap();
    let first_db = tmp.path().join("first.sqlite");
    let second_db = tmp.path().join("second.sqlite");
    let first_rollout = tmp.path().join("first.jsonl");
    let second_rollout = tmp.path().join("second.jsonl");
    fs::write(
        &first_rollout,
        "{\"type\":\"message\",\"source\":\"first\"}\n",
    )
    .unwrap();
    fs::write(
        &second_rollout,
        "{\"type\":\"message\",\"source\":\"second\"}\n",
    )
    .unwrap();
    create_codex_thread_db(&first_db, &first_rollout);
    create_codex_thread_db(&second_db, &second_rollout);
    let backup_store = BackupStore::new(tmp.path().join("backups"));

    let deleted = delete_local_from_paths(
        vec![first_db.clone(), second_db.clone()],
        backup_store.clone(),
        &session("t1", "Codex Thread"),
    );
    let undo_token = deleted.undo_token.as_deref().unwrap();
    let db = Connection::open(&second_db).unwrap();
    db.execute(
        "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms)
         VALUES ('t1', ?1, 'Conflict', '/conflict', 0, NULL, 1, 1000)",
        [second_rollout.to_string_lossy().to_string()],
    )
    .unwrap();
    drop(db);

    let restored = undo_local_from_backup(
        vec![first_db.clone(), second_db.clone()],
        backup_store,
        undo_token,
    );

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(thread_count(&first_db, "t1"), 0);
    assert_eq!(thread_count(&second_db, "t1"), 1);
    assert!(!first_rollout.exists());
    assert!(!second_rollout.exists());
}

#[test]
fn single_database_undo_rejects_source_outside_allowed_paths_without_writes() {
    let tmp = tempdir().unwrap();
    let allowed_db = tmp.path().join("allowed.sqlite");
    let external_db = tmp.path().join("external.sqlite");
    let allowed_rollout = tmp.path().join("allowed.jsonl");
    let external_rollout = tmp.path().join("external.jsonl");
    fs::write(
        &allowed_rollout,
        "{\"type\":\"message\",\"source\":\"allowed\"}\n",
    )
    .unwrap();
    fs::write(
        &external_rollout,
        "{\"type\":\"message\",\"source\":\"external\"}\n",
    )
    .unwrap();
    create_codex_thread_db(&allowed_db, &allowed_rollout);
    create_codex_thread_db(&external_db, &external_rollout);
    let backup_store = BackupStore::new(tmp.path().join("backups"));
    let external_adapter = SQLiteStorageAdapter::new(&external_db, backup_store.clone());
    assert_eq!(
        external_adapter
            .delete_local(&session("t1", "Codex Thread"))
            .status,
        DeleteStatus::LocalDeleted
    );
    let allowed_adapter = SQLiteStorageAdapter::new(&allowed_db, backup_store.clone());
    let deleted = allowed_adapter.delete_local(&session("t1", "Codex Thread"));
    let undo_token = deleted.undo_token.as_deref().unwrap();
    overwrite_backup_source_db(&backup_store, undo_token, &external_db);

    let restored = undo_local_from_backup(vec![allowed_db.clone()], backup_store, undo_token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(thread_count(&allowed_db, "t1"), 0);
    assert_eq!(thread_count(&external_db, "t1"), 0);
    assert!(!allowed_rollout.exists());
    assert!(!external_rollout.exists());
}

#[test]
fn multi_database_undo_rejects_source_outside_allowed_paths_without_writes() {
    let tmp = tempdir().unwrap();
    let first_db = tmp.path().join("first.sqlite");
    let second_db = tmp.path().join("second.sqlite");
    let external_db = tmp.path().join("external.sqlite");
    let first_rollout = tmp.path().join("first.jsonl");
    let second_rollout = tmp.path().join("second.jsonl");
    let external_rollout = tmp.path().join("external.jsonl");
    fs::write(
        &first_rollout,
        "{\"type\":\"message\",\"source\":\"first\"}\n",
    )
    .unwrap();
    fs::write(
        &second_rollout,
        "{\"type\":\"message\",\"source\":\"second\"}\n",
    )
    .unwrap();
    fs::write(
        &external_rollout,
        "{\"type\":\"message\",\"source\":\"external\"}\n",
    )
    .unwrap();
    create_codex_thread_db(&first_db, &first_rollout);
    create_codex_thread_db(&second_db, &second_rollout);
    create_codex_thread_db(&external_db, &external_rollout);
    let backup_store = BackupStore::new(tmp.path().join("backups"));
    let external_adapter = SQLiteStorageAdapter::new(&external_db, backup_store.clone());
    assert_eq!(
        external_adapter
            .delete_local(&session("t1", "Codex Thread"))
            .status,
        DeleteStatus::LocalDeleted
    );
    let deleted = delete_local_from_paths(
        vec![first_db.clone(), second_db.clone()],
        backup_store.clone(),
        &session("t1", "Codex Thread"),
    );
    let undo_token = deleted.undo_token.as_deref().unwrap().to_string();
    let mut aggregate = backup_store.read_backup(&undo_token).unwrap();
    let child_token = aggregate["targets"][0]["token"]
        .as_str()
        .unwrap()
        .to_string();
    overwrite_backup_source_db(&backup_store, &child_token, &external_db);
    aggregate["targets"][0]["source_db"] = json!(external_db.to_string_lossy().to_string());
    fs::write(
        backup_store.path_for(&undo_token),
        serde_json::to_string_pretty(&aggregate).unwrap(),
    )
    .unwrap();

    let restored = undo_local_from_backup(
        vec![first_db.clone(), second_db.clone()],
        backup_store,
        &undo_token,
    );

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(thread_count(&first_db, "t1"), 0);
    assert_eq!(thread_count(&second_db, "t1"), 0);
    assert_eq!(thread_count(&external_db, "t1"), 0);
    assert!(!first_rollout.exists());
    assert!(!second_rollout.exists());
    assert!(!external_rollout.exists());
}

#[test]
fn move_thread_workspace_from_paths_uses_database_that_contains_thread() {
    let tmp = tempdir().unwrap();
    let stale_db = tmp.path().join("stale.sqlite");
    let live_db = tmp.path().join("live.sqlite");
    let stale_rollout = tmp.path().join("stale.jsonl");
    let live_rollout = tmp.path().join("live.jsonl");
    fs::write(&stale_rollout, "{\"type\":\"message\"}\n").unwrap();
    fs::write(
        &live_rollout,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\",\"cwd\":\"/old/project\",\"title\":\"Codex Thread\"}}\n",
    )
    .unwrap();
    create_codex_thread_db(&stale_db, &stale_rollout);
    create_codex_thread_db(&live_db, &live_rollout);
    Connection::open(&stale_db)
        .unwrap()
        .execute("DELETE FROM threads WHERE id = 't1'", [])
        .unwrap();

    let result = move_codex_thread_workspace_from_paths(
        vec![stale_db.clone(), live_db.clone()],
        BackupStore::new(tmp.path().join("backups")),
        &session("local:t1", "Codex Thread"),
        "/new/project",
    );

    assert_eq!(result["status"], "moved");
    assert_eq!(result["target_cwd"], "/new/project");
    assert_eq!(result["db_path"], live_db.to_string_lossy().to_string());
    assert_eq!(
        Connection::open(&live_db)
            .unwrap()
            .query_row("SELECT cwd FROM threads WHERE id = 't1'", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "/new/project"
    );
    assert_eq!(thread_count(&stale_db, "t1"), 0);
}

#[test]
fn list_local_sessions_reads_codex_threads_ordered_by_update_time() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let backup = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, title TEXT, cwd TEXT, model_provider TEXT, archived INTEGER, updated_at_ms INTEGER)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('t1', 'r1.jsonl', 'First', 'C:/a', 'openai', 0, 100)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('t2', 'r2.jsonl', 'Second', 'C:/b', 'custom', 1, 300)",
        [],
    )
    .unwrap();
    drop(db);

    let sessions = adapter.list_local_sessions().unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, "t2");
    assert_eq!(sessions[0].title, "Second");
    assert_eq!(sessions[0].model_provider, "custom");
    assert!(sessions[0].archived);
    assert_eq!(sessions[1].id, "t1");
}

#[test]
fn list_local_sessions_reads_codex_automation_runs_schema() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex-dev.db");
    let backup = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE automation_runs (
            thread_id TEXT PRIMARY KEY,
            status TEXT,
            thread_title TEXT,
            source_cwd TEXT,
            created_at INTEGER,
            updated_at INTEGER
        )",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO automation_runs VALUES ('t1', 'running', 'First', 'C:/a', 100, 200)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO automation_runs VALUES ('t2', 'archived', 'Second', 'C:/b', 300, 400)",
        [],
    )
    .unwrap();
    drop(db);

    let sessions = adapter.list_local_sessions().unwrap();

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, "t2");
    assert_eq!(sessions[0].title, "Second");
    assert_eq!(sessions[0].cwd, "C:/b");
    assert!(sessions[0].archived);
    assert_eq!(sessions[0].db_path, db_path.to_string_lossy());
    assert_eq!(sessions[1].id, "t1");
}

#[test]
fn delete_local_session_removes_codex_automation_run_and_inbox_items() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("codex-dev.db");
    let backup = BackupStore::new(tmp.path().join("backups"));
    let adapter = SQLiteStorageAdapter::new(&db_path, backup);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY, thread_title TEXT)",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TABLE inbox_items (id TEXT PRIMARY KEY, thread_id TEXT, title TEXT)",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO automation_runs VALUES ('t1', 'First')", [])
        .unwrap();
    db.execute("INSERT INTO inbox_items VALUES ('i1', 't1', 'Inbox')", [])
        .unwrap();
    drop(db);

    let result = adapter.delete_local(&session("t1", "First"));

    assert_eq!(result.status, DeleteStatus::LocalDeleted);
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM automation_runs WHERE thread_id = 't1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM inbox_items WHERE thread_id = 't1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn undo_codex_thread_delete_fails_when_agent_job_was_reassigned() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let deleted = adapter.delete_local(&session("local:t1", "Codex Thread"));

    assert_eq!(deleted.status, DeleteStatus::LocalDeleted);
    let token = deleted.undo_token.as_deref().unwrap();
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('t2', NULL, 'Other Thread', '/new/project', 0, NULL, 200, 200000)",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE agent_job_items SET assigned_thread_id = 't2' WHERE id = 'job1'",
        [],
    )
    .unwrap();
    drop(db);

    let restored = adapter.undo(token);

    assert_eq!(restored.status, DeleteStatus::Failed);
    assert_eq!(restored.undo_token.as_deref(), Some(token));
    assert!(restored.message.to_lowercase().contains("restore conflict"));
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT assigned_thread_id FROM agent_job_items WHERE id = 'job1'",
            [],
            |row| row.get::<_, Option<String>>(0)
        )
        .unwrap(),
        Some("t2".to_string())
    );
}

#[test]
fn codex_delete_rolls_back_when_related_delete_fails() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "CREATE TRIGGER fail_goals_delete BEFORE DELETE ON thread_goals BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .unwrap();
    drop(db);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let result = adapter.delete_local(&session("t1", "Codex Thread"));

    assert_eq!(result.status, DeleteStatus::Failed);
    assert!(result.undo_token.is_some());
    assert!(rollout_path.exists());
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads WHERE id = 't1'", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM thread_dynamic_tools WHERE thread_id = 't1'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM thread_goals WHERE thread_id = 't1'",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        1
    );
}

#[test]
fn missing_db_and_unsupported_schema_return_failed_results() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join("missing.sqlite");
    let adapter = SQLiteStorageAdapter::new(&missing, BackupStore::new(tmp.path().join("backups")));

    let result = adapter.delete_local(&session("s1", "First"));

    assert_eq!(result.status, DeleteStatus::Failed);
    assert!(result.message.contains("Database not found"));

    let db_path = tmp.path().join("unknown.sqlite");
    let db = Connection::open(&db_path).unwrap();
    db.execute("CREATE TABLE unrelated (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(db);
    let adapter =
        SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups2")));

    let result = adapter.delete_local(&session("s1", "First"));

    assert_eq!(result.status, DeleteStatus::Failed);
    assert!(result.message.contains("Unsupported"));
}

#[test]
fn delete_codex_thread_returns_not_found_when_thread_absent() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    // 会话在本地 DB 中不存在（仅残留在 UI），应返回 NotFound 而非 Failed
    let result = adapter.delete_local(&session("local:does-not-exist", "Ghost"));

    assert_eq!(result.status, DeleteStatus::NotFound);
}

#[test]
fn delete_local_from_paths_returns_not_found_when_thread_absent_everywhere() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);

    let result = delete_local_from_paths(
        vec![db_path.clone()],
        BackupStore::new(tmp.path().join("backups")),
        &session("local:ghost", "Ghost"),
    );

    assert_eq!(result.status, DeleteStatus::NotFound);
}

#[test]
fn archived_lookup_workspace_move_and_sort_keys_match_expected_shape() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\",\"cwd\":\"/old/project\",\"title\":\"Codex Thread\"}}\n{\"type\":\"session_meta\",\"payload\":{\"id\":\"other\",\"cwd\":\"/old/project\"}}\n",
    )
    .unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "UPDATE threads SET archived = 1, archived_at = 123 WHERE id = 't1'",
        [],
    )
    .unwrap();
    db.execute("INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('t2', ?1, 'Second', '/other/project', 0, NULL, 200, 200000)", [rollout_path.to_string_lossy().to_string()]).unwrap();
    drop(db);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    assert_eq!(
        adapter.find_archived_thread_by_title("Codex Thread 2026年5月9日，1:19 · RustGUI"),
        Some(session("t1", "Codex Thread"))
    );

    let moved =
        adapter.move_codex_thread_workspace(&session("local:t1", "Codex Thread"), "/new/project");
    assert_eq!(moved["status"], "moved");
    assert_eq!(moved["previous_cwd"], "/old/project");
    assert_eq!(moved["target_cwd"], "/new/project");
    assert_eq!(moved["rollout_updated"], true);
    assert_eq!(moved["updated_at"], 100);
    assert_eq!(moved["updated_at_ms"], 100000);
    let text = fs::read_to_string(&rollout_path).unwrap();
    assert!(text.contains("\"id\":\"t1\",\"cwd\":\"/new/project\""));
    assert!(text.contains("\"id\":\"other\",\"cwd\":\"/old/project\""));

    assert_eq!(
        adapter.codex_thread_sort_key(&session("local:t1", "Codex Thread")),
        json!({"status": "ok", "session_id": "t1", "updated_at": 100, "updated_at_ms": 100000, "created_at_ms": null})
    );
    assert_eq!(
        adapter.codex_thread_sort_keys(&[
            session("local:t2", "Second"),
            session("local:t1", "Codex Thread")
        ]),
        json!({
            "status": "ok",
            "sort_keys": [
                {"session_id": "t2", "updated_at": 200, "updated_at_ms": 200000, "created_at_ms": null},
                {"session_id": "t1", "updated_at": 100, "updated_at_ms": 100000, "created_at_ms": null}
            ]
        })
    );

    assert_eq!(
        adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread")),
        json!({
            "status": "ok",
            "session_id": "t1",
            "requested_session_id": "t1",
            "title": "Codex Thread",
            "matched_by": "id",
            "thread_updated_at_ms": 100000,
            "db_path": db_path.to_string_lossy().to_string(),
            "rollout_path": rollout_path.to_string_lossy().to_string(),
            "history": [],
            "summary": {
                "totalUsage": {
                    "inputTokens": 0,
                    "outputTokens": 0,
                    "totalTokens": 0,
                    "cachedTokens": 0,
                    "cacheCreationTokens": 0,
                    "cacheTokens": 0
                },
                "lastTurnUsage": {
                    "inputTokens": 0,
                    "outputTokens": 0,
                    "totalTokens": 0,
                    "cachedTokens": 0,
                    "cacheCreationTokens": 0,
                    "cacheTokens": 0
                },
                "lastTurnId": "",
                "observedAt": "",
                "turnCount": 0
            }
        })
    );
}

#[test]
fn thread_sort_keys_preserves_request_order_for_two_hundred_ids() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(&rollout_path, "{\"type\":\"message\"}\n").unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let db = Connection::open(&db_path).unwrap();
    for index in 2..=200 {
        db.execute(
            "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms)
             VALUES (?1, ?2, ?3, '/project', 0, NULL, ?4, ?5)",
            (
                format!("t{index}"),
                rollout_path.to_string_lossy().to_string(),
                format!("Thread {index}"),
                index as i64,
                (index * 1000) as i64,
            ),
        )
        .unwrap();
    }
    drop(db);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));
    let sessions = (1..=200)
        .rev()
        .map(|index| session(&format!("local:t{index}"), &format!("Thread {index}")))
        .collect::<Vec<_>>();

    let result = adapter.codex_thread_sort_keys(&sessions);

    assert_eq!(result["status"], "ok");
    let sort_keys = result["sort_keys"].as_array().unwrap();
    assert_eq!(sort_keys.len(), 200);
    assert_eq!(sort_keys[0]["session_id"], "t200");
    assert_eq!(sort_keys[0]["updated_at_ms"], 200000);
    assert_eq!(sort_keys[199]["session_id"], "t1");
    assert_eq!(sort_keys[199]["updated_at_ms"], 100000);
}

#[test]
fn thread_usage_history_reads_rollout_token_count_events() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":5000,\"cached_input_tokens\":1500,\"output_tokens\":500,\"total_tokens\":5500},\"last_token_usage\":{\"input_tokens\":1200,\"cached_input_tokens\":900,\"output_tokens\":120,\"total_tokens\":1320},\"model_context_window\":258400}}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"ignore\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":7000,\"cached_input_tokens\":2500,\"output_tokens\":750,\"total_tokens\":7750},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1200,\"output_tokens\":250,\"total_tokens\":2250},\"model_context_window\":258400}}}\n"
        ),
    )
    .unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    assert_eq!(
        adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread")),
        json!({
            "status": "ok",
            "session_id": "t1",
            "requested_session_id": "t1",
            "title": "Codex Thread",
            "matched_by": "id",
            "thread_updated_at_ms": 100000,
            "db_path": db_path.to_string_lossy().to_string(),
            "rollout_path": rollout_path.to_string_lossy().to_string(),
            "history": [
                {
                    "source": "rollout-history",
                    "conversation_id": "local:t1",
                    "turn_id": "turn-1",
                    "observed_at": "2026-06-02T05:00:00Z",
                    "usage": {
                        "inputTokens": 1200,
                        "outputTokens": 120,
                        "totalTokens": 1320,
                        "cachedTokens": 900,
                        "cacheReadTokens": 900,
                        "cacheCreationTokens": 0,
                        "contextUsed": 5500,
                        "contextLimit": 258400,
                        "hasBreakdown": true
                    }
                },
                {
                    "source": "rollout-history",
                    "conversation_id": "local:t1",
                    "turn_id": "turn-2",
                    "observed_at": "2026-06-02T05:01:00Z",
                    "usage": {
                        "inputTokens": 2000,
                        "outputTokens": 250,
                        "totalTokens": 2250,
                        "cachedTokens": 1200,
                        "cacheReadTokens": 1200,
                        "cacheCreationTokens": 0,
                        "contextUsed": 7750,
                        "contextLimit": 258400,
                        "hasBreakdown": true
                    }
                }
            ],
            "summary": {
                "totalUsage": {
                    "inputTokens": 7000,
                    "outputTokens": 750,
                    "totalTokens": 7750,
                    "cachedTokens": 2500,
                    "cacheCreationTokens": 0,
                    "cacheTokens": 2500
                },
                "lastTurnUsage": {
                    "inputTokens": 2000,
                    "outputTokens": 250,
                    "totalTokens": 2250,
                    "cachedTokens": 1200,
                    "cacheCreationTokens": 0,
                    "cacheTokens": 1200
                },
                "lastTurnId": "turn-2",
                "observedAt": "2026-06-02T05:01:00Z",
                "turnCount": 2
            }
        })
    );
}

#[test]
fn thread_usage_history_handles_appended_partial_lines_and_replaced_rollouts() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n"
        ),
    )
    .unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    assert_eq!(
        adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"))["summary"]["totalUsage"]
            ["totalTokens"],
        1100
    );

    fs::OpenOptions::new()
        .append(true)
        .open(&rollout_path)
        .unwrap()
        .write_all(
            b"{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}",
        )
        .unwrap();
    assert_eq!(
        adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"))["summary"]["totalUsage"]
            ["totalTokens"],
        1100
    );

    fs::OpenOptions::new()
        .append(true)
        .open(&rollout_path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let appended = adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"));
    assert_eq!(appended["summary"]["totalUsage"]["totalTokens"], 3300);
    assert_eq!(appended["summary"]["lastTurnId"], "turn-2");

    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"replacement\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":700,\"cached_input_tokens\":200,\"output_tokens\":70,\"total_tokens\":770},\"last_token_usage\":{\"input_tokens\":700,\"cached_input_tokens\":200,\"output_tokens\":70,\"total_tokens\":770}}}}\n"
        ),
    )
    .unwrap();
    let replaced = adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"));
    assert_eq!(replaced["summary"]["totalUsage"]["totalTokens"], 770);
    assert_eq!(replaced["summary"]["lastTurnId"], "replacement");
}

#[test]
fn thread_usage_history_groups_all_calls_in_latest_turn() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}\n",
            "{\"timestamp\":\"2026-06-02T05:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":6000,\"cached_input_tokens\":2900,\"output_tokens\":650,\"total_tokens\":6650},\"last_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1500,\"output_tokens\":350,\"total_tokens\":3350}}}}\n"
        ),
    )
    .unwrap();
    create_codex_thread_db(&db_path, &rollout_path);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let result = adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"));

    assert_eq!(result["summary"]["lastTurnId"], "turn-2");
    assert_eq!(result["summary"]["turnCount"], 2);
    assert_eq!(result["summary"]["totalUsage"]["totalTokens"], 6650);
    assert_eq!(result["summary"]["totalUsage"]["cachedTokens"], 2900);
    assert_eq!(result["summary"]["lastTurnUsage"]["inputTokens"], 5000);
    assert_eq!(result["summary"]["lastTurnUsage"]["outputTokens"], 550);
    assert_eq!(result["summary"]["lastTurnUsage"]["totalTokens"], 5550);
    assert_eq!(result["summary"]["lastTurnUsage"]["cachedTokens"], 2500);
}

#[test]
fn thread_usage_history_searches_all_databases_and_resolves_temporary_id_by_title() {
    let tmp = tempdir().unwrap();
    let unsupported_path = tmp.path().join("automation.sqlite");
    let supported_path = tmp.path().join("state_5.sqlite");
    let rollout_path = tmp.path().join("rollout.jsonl");
    let db = Connection::open(&unsupported_path).unwrap();
    db.execute(
        "CREATE TABLE automation_runs (thread_id TEXT PRIMARY KEY)",
        [],
    )
    .unwrap();
    drop(db);
    fs::write(
        &rollout_path,
        "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n",
    )
    .unwrap();
    create_codex_thread_db(&supported_path, &rollout_path);

    let result = codex_thread_usage_history_from_paths(
        vec![unsupported_path, supported_path.clone()],
        BackupStore::new(tmp.path().join("backups")),
        &session("local:client-new-thread:temporary-123", "Codex Thread"),
    );

    assert_eq!(result["status"], "ok");
    assert_eq!(result["session_id"], "t1");
    assert_eq!(
        result["requested_session_id"],
        "client-new-thread:temporary-123"
    );
    assert_eq!(result["matched_by"], "title");
    assert_eq!(
        result["db_path"],
        supported_path.to_string_lossy().to_string()
    );
    assert_eq!(result["summary"]["totalUsage"]["totalTokens"], 1100);
}

#[test]
fn thread_usage_history_includes_recursive_subagents_in_parent_totals_and_latest_turn() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("state_5.sqlite");
    let root_rollout = tmp.path().join("root-rollout.jsonl");
    let old_child_rollout = tmp.path().join("old-child-rollout.jsonl");
    let child_rollout = tmp.path().join("child-rollout.jsonl");
    let grandchild_rollout = tmp.path().join("grandchild-rollout.jsonl");
    fs::write(
        &root_rollout,
        concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"output\":\"{\\\"agent_id\\\":\\\"old-child\\\"}\",\"internal_chat_message_metadata_passthrough\":{\"turn_id\":\"turn-1\"}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-2\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"output\":\"{\\\"agent_id\\\":\\\"child\\\"}\",\"internal_chat_message_metadata_passthrough\":{\"turn_id\":\"turn-2\"}}}\n"
        ),
    )
    .unwrap();
    fs::write(
        &old_child_rollout,
        concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"old-turn\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"old-turn\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:30Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":200,\"cached_input_tokens\":50,\"output_tokens\":20,\"total_tokens\":220},\"last_token_usage\":{\"input_tokens\":200,\"cached_input_tokens\":50,\"output_tokens\":20,\"total_tokens\":220}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"old-turn\"}}\n"
        ),
    )
    .unwrap();
    fs::write(
        &child_rollout,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"forked_from_id\":\"t1\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"child-turn-1\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"child-turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:30Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3550,\"cached_input_tokens\":1700,\"output_tokens\":350,\"total_tokens\":3900},\"last_token_usage\":{\"input_tokens\":550,\"cached_input_tokens\":300,\"output_tokens\":50,\"total_tokens\":600}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"child-turn-1\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"child-turn-2\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"child-turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":4000,\"cached_input_tokens\":1900,\"output_tokens\":400,\"total_tokens\":4400},\"last_token_usage\":{\"input_tokens\":450,\"cached_input_tokens\":200,\"output_tokens\":50,\"total_tokens\":500}}}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"output\":\"{\\\"agent_id\\\":\\\"grandchild\\\"}\",\"internal_chat_message_metadata_passthrough\":{\"turn_id\":\"child-turn-2\"}}}\n"
        ),
    )
    .unwrap();
    fs::write(
        &grandchild_rollout,
        concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"grandchild\",\"forked_from_id\":\"child\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100},\"last_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,\"output_tokens\":100,\"total_tokens\":1100}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3000,\"cached_input_tokens\":1400,\"output_tokens\":300,\"total_tokens\":3300},\"last_token_usage\":{\"input_tokens\":2000,\"cached_input_tokens\":1000,\"output_tokens\":200,\"total_tokens\":2200}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"child-turn-1\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:01:30Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":3550,\"cached_input_tokens\":1700,\"output_tokens\":350,\"total_tokens\":3900},\"last_token_usage\":{\"input_tokens\":550,\"cached_input_tokens\":300,\"output_tokens\":50,\"total_tokens\":600}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"child-turn-2\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":4000,\"cached_input_tokens\":1900,\"output_tokens\":400,\"total_tokens\":4400},\"last_token_usage\":{\"input_tokens\":450,\"cached_input_tokens\":200,\"output_tokens\":50,\"total_tokens\":500}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"grandchild-turn\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"grandchild-turn\"}}\n",
            "{\"timestamp\":\"2026-06-02T05:03:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":4500,\"cached_input_tokens\":2100,\"output_tokens\":450,\"total_tokens\":4950},\"last_token_usage\":{\"input_tokens\":500,\"cached_input_tokens\":200,\"output_tokens\":50,\"total_tokens\":550}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"grandchild-turn\"}}\n"
        ),
    )
    .unwrap();
    create_codex_thread_db(&db_path, &root_rollout);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('old-child', ?1, 'Old child', '/old/project', 0, NULL, 101, 101000)",
        [old_child_rollout.to_string_lossy().to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('child', ?1, 'Child', '/old/project', 0, NULL, 102, 102000)",
        [child_rollout.to_string_lossy().to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads (id, rollout_path, title, cwd, archived, archived_at, updated_at, updated_at_ms) VALUES ('grandchild', ?1, 'Grandchild', '/old/project', 0, NULL, 103, 103000)",
        [grandchild_rollout.to_string_lossy().to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status) VALUES ('t1', 'old-child', 'cancelled')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status) VALUES ('child', 'grandchild', 'unknown')",
        [],
    )
    .unwrap();
    drop(db);
    let adapter = SQLiteStorageAdapter::new(&db_path, BackupStore::new(tmp.path().join("backups")));

    let result = adapter.codex_thread_usage_history(&session("local:t1", "Codex Thread"));

    assert_eq!(result["status"], "ok");
    assert_eq!(result["summary"]["totalUsage"]["totalTokens"], 5170);
    assert_eq!(result["summary"]["ownTotalUsage"]["totalTokens"], 3300);
    assert_eq!(
        result["summary"]["descendantTotalUsage"]["totalTokens"],
        1870
    );
    assert_eq!(result["summary"]["lastTurnId"], "turn-2");
    assert_eq!(result["summary"]["lastTurnUsage"]["totalTokens"], 3850);
    assert_eq!(result["summary"]["descendantCount"], 3);
    assert_eq!(result["summary"]["lastTurnDescendantCount"], 2);
    assert_eq!(result["summary"]["isRunning"], true);
    assert_eq!(result["summary"]["activeThreadCount"], 2);
    assert_eq!(result["summary"]["lastTurnRunning"], true);
    assert_eq!(result["summary"]["observedAt"], "2026-06-02T05:03:00Z");
    assert_eq!(
        result["summary"]["includedThreadIds"],
        json!(["t1", "child", "old-child", "grandchild"])
    );
    assert!(
        result["summary"]
            .get("unassociatedDescendantCount")
            .is_none()
    );
}
