use codex_elves_data::{
    ProviderSyncProgressPhase, ProviderSyncStatus, ProviderSyncTargetSource,
    cleanup_stale_provider_sync_lock, load_provider_sync_targets,
    preview_provider_sync_with_target, preview_provider_sync_with_target_and_progress,
    run_provider_sync, run_provider_sync_with_target,
};
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tempfile::tempdir;

fn write_rollout(path: &Path, provider: &str, thread_id: &str, cwd: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let first = json!({
        "type": "session_meta",
        "payload": {
            "id": thread_id,
            "model_provider": provider,
            "cwd": cwd
        }
    });
    let event = json!({"type": "event_msg", "payload": {"type": "user_message"}});
    fs::write(path, format!("{first}\n{event}\n")).unwrap();
}

fn write_rollout_with_providers(path: &Path, providers: &[&str], thread_id: &str, cwd: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut lines = Vec::new();
    for provider in providers {
        lines.push(
            json!({
                "type": "session_meta",
                "payload": {
                    "id": thread_id,
                    "model_provider": provider,
                    "cwd": cwd
                }
            })
            .to_string(),
        );
        lines.push(json!({"type": "event_msg", "payload": {"type": "task_started"}}).to_string());
    }
    lines.push(json!({"type": "event_msg", "payload": {"type": "user_message"}}).to_string());
    fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

fn write_indexable_rollout(path: &Path, provider: &str, thread_id: &str, thread_source: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        json!({
            "timestamp": "2026-08-05T01:02:03Z",
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "model_provider": provider,
                "cwd": "C:/workspace",
                "source": "vscode",
                "thread_source": thread_source,
                "cli_version": "0.99.0"
            }
        }),
        json!({
            "timestamp": "2026-08-05T01:02:04Z",
            "type": "turn_context",
            "payload": {
                "approval_policy": "never",
                "sandbox_policy": {"type": "danger-full-access"}
            }
        }),
        json!({
            "timestamp": "2026-08-05T01:02:05Z",
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": "请修复历史会话"
            }
        }),
    ];
    fs::write(
        path,
        format!(
            "{}\n",
            lines
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
}

fn create_state_db(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, archived INTEGER, has_user_event INTEGER, cwd TEXT)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('thread-1', 'old-provider', 0, 0, 'C:/old')",
        [],
    )
    .unwrap();
}

fn create_full_state_db(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            source TEXT NOT NULL,
            model_provider TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            sandbox_policy TEXT NOT NULL,
            approval_mode TEXT NOT NULL,
            tokens_used INTEGER NOT NULL DEFAULT 0,
            has_user_event INTEGER NOT NULL DEFAULT 0,
            archived INTEGER NOT NULL DEFAULT 0,
            cli_version TEXT NOT NULL DEFAULT '',
            first_user_message TEXT NOT NULL DEFAULT '',
            memory_mode TEXT NOT NULL DEFAULT 'enabled',
            preview TEXT NOT NULL DEFAULT '',
            recency_at INTEGER NOT NULL DEFAULT 0,
            recency_at_ms INTEGER NOT NULL DEFAULT 0,
            history_mode TEXT NOT NULL DEFAULT 'legacy',
            is_pinned INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();
}

fn create_state_db_with_providers(path: &Path, rows: &[(&str, &str, i64)]) {
    let db = Connection::open(path).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, archived INTEGER, has_user_event INTEGER, cwd TEXT)",
        [],
    )
    .unwrap();
    for (id, provider, archived) in rows {
        db.execute(
            "INSERT INTO threads VALUES (?1, ?2, ?3, 1, 'C:/workspace')",
            (id, provider, archived),
        )
        .unwrap();
    }
}

#[test]
fn provider_sync_targets_merge_config_rollout_sqlite_and_sort_current_first() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"

[model_providers.apigather]
name = "apigather"
"#,
    )
    .unwrap();
    write_rollout(
        &home.join("sessions/2026/rollout-openai.jsonl"),
        "openai",
        "thread-openai",
        "C:/workspace/openai",
    );
    write_rollout(
        &home.join("archived_sessions/rollout-legacy.jsonl"),
        "legacy-provider",
        "thread-legacy",
        "C:/workspace/legacy",
    );
    create_state_db_with_providers(
        &home.join("state_5.sqlite"),
        &[
            ("thread-sqlite", "sqlite-provider", 0),
            ("thread-openai", "openai", 1),
        ],
    );

    let targets = load_provider_sync_targets(Some(&home));

    assert_eq!(targets.current_provider, "custom");
    let ids = targets
        .targets
        .iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "custom",
            "apigather",
            "legacy-provider",
            "openai",
            "sqlite-provider",
        ]
    );
    let custom = targets
        .targets
        .iter()
        .find(|target| target.id == "custom")
        .unwrap();
    assert!(custom.is_current_provider);
    assert!(custom.sources.contains(&ProviderSyncTargetSource::Config));
    let openai = targets
        .targets
        .iter()
        .find(|target| target.id == "openai")
        .unwrap();
    assert!(openai.sources.contains(&ProviderSyncTargetSource::Config));
    assert!(openai.sources.contains(&ProviderSyncTargetSource::Rollout));
    assert!(openai.sources.contains(&ProviderSyncTargetSource::Sqlite));
    let legacy = targets
        .targets
        .iter()
        .find(|target| target.id == "legacy-provider")
        .unwrap();
    assert_eq!(legacy.sources, vec![ProviderSyncTargetSource::Rollout]);
}

#[test]
fn provider_sync_maps_official_mixed_to_custom_provider_id() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        r#"model_provider = "custom"

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://example.com/v1"
experimental_bearer_token = "sk-test"
"#,
    )
    .unwrap();
    let rollout = home.join("sessions/2026/rollout-official-mix.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    create_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.target_provider, "custom");
    assert_eq!(result.changed_session_files, 1);
    assert_eq!(result.sqlite_provider_rows_updated, 1);
    let first: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&rollout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["payload"]["model_provider"], "custom");
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let provider: String = db
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(provider, "custom");
}

#[test]
fn provider_sync_rewrites_all_session_meta_model_providers() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-multi-meta.jsonl");
    write_rollout_with_providers(
        &rollout,
        &["openai", "ccx", "CodexElves"],
        "thread-1",
        "C:/workspace",
    );
    create_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.target_provider, "apigather");
    assert_eq!(result.changed_session_files, 1);

    let providers = fs::read_to_string(&rollout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["type"] == "session_meta")
        .map(|record| {
            record["payload"]["model_provider"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(providers, vec!["apigather", "apigather", "apigather"]);
}

#[test]
fn provider_sync_target_discovery_reads_all_session_meta_providers() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    write_rollout_with_providers(
        &home.join("sessions/2026/rollout-multi-meta.jsonl"),
        &["openai", "ccx", "CodexElves"],
        "thread-1",
        "C:/workspace",
    );

    let targets = load_provider_sync_targets(Some(&home));
    let ids = targets
        .targets
        .iter()
        .map(|target| target.id.as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&"openai"));
    assert!(ids.contains(&"ccx"));
    assert!(ids.contains(&"CodexElves"));
}

#[test]
fn provider_sync_updates_rollout_sqlite_visibility_and_creates_backup() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-abc.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    create_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.target_provider, "apigather");
    assert_eq!(result.changed_session_files, 1);
    assert_eq!(result.sqlite_rows_updated, 3);
    assert_eq!(result.sqlite_provider_rows_updated, 1);
    assert_eq!(result.sqlite_user_event_rows_updated, 1);
    assert_eq!(result.sqlite_cwd_rows_updated, 1);
    let first: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&rollout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["payload"]["model_provider"], "apigather");
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row = db
        .query_row(
            "SELECT model_provider, has_user_event, cwd FROM threads WHERE id = 'thread-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        ("apigather".to_string(), 1, "C:/workspace".to_string())
    );
    let backup_dir = result.backup_dir.unwrap();
    assert!(backup_dir.join("session-meta-backup.json").exists());
    assert!(backup_dir.join("db/state_5.sqlite").exists());
}

#[test]
fn provider_sync_updates_new_codex_sqlite_directory_db() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    let sqlite_dir = home.join("sqlite");
    fs::create_dir_all(&sqlite_dir).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-abc.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    let db_path = sqlite_dir.join("codex-dev.db");
    create_state_db(&db_path);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.sqlite_rows_updated, 3);
    let db = Connection::open(&db_path).unwrap();
    let row = db
        .query_row(
            "SELECT model_provider, has_user_event, cwd FROM threads WHERE id = 'thread-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        ("apigather".to_string(), 1, "C:/workspace".to_string())
    );
    let backup_dir = result.backup_dir.unwrap();
    assert!(backup_dir.join("db/sqlite/codex-dev.db").exists());
}

#[test]
fn provider_sync_backup_metadata_contains_reference_fields_and_managed_marker() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-backup.jsonl"),
        "openai",
        "thread-1",
        "C:/workspace",
    );
    create_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    let backup_dir = result.backup_dir.unwrap();
    let metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(backup_dir.join("metadata.json")).unwrap())
            .unwrap();
    assert_eq!(metadata["version"], 1);
    assert_eq!(metadata["namespace"], "provider-sync");
    assert_eq!(metadata["codexHome"], home.to_string_lossy().to_string());
    assert_eq!(metadata["targetProvider"], "apigather");
    assert_eq!(metadata["changedSessionFiles"], 1);
    assert_eq!(metadata["managedBy"], "CodexElves provider sync");
    assert!(metadata["createdAt"].as_str().unwrap().contains('T'));
    assert!(
        metadata["dbFiles"]
            .as_array()
            .unwrap()
            .contains(&json!("state_5.sqlite"))
    );
}

#[test]
fn provider_sync_explicit_target_overrides_config_without_switching_config() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-target.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    create_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync_with_target(Some(&home), Some("custom"));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.target_provider, "custom");
    assert_eq!(
        fs::read_to_string(home.join("config.toml")).unwrap(),
        "model_provider = \"apigather\"\n"
    );
    let first: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&rollout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["payload"]["model_provider"], "custom");
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let provider: String = db
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(provider, "custom");
}

#[test]
fn provider_sync_rejects_invalid_explicit_target_before_writes() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/rollout-invalid-target.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    let original = fs::read_to_string(&rollout).unwrap();

    let result = run_provider_sync_with_target(Some(&home), Some("bad\nprovider"));

    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    assert!(result.message.contains("Invalid provider sync target"));
    assert_eq!(fs::read_to_string(&rollout).unwrap(), original);
    assert!(result.backup_dir.is_none());
}

#[test]
fn provider_sync_repairs_sqlite_when_rollout_provider_matches_and_normalizes_paths() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("archived_sessions/rollout-current.jsonl"),
        "apigather",
        "thread-1",
        "\\\\?\\C:\\workspace",
    );
    create_state_db(&home.join("state_5.sqlite"));
    fs::write(
        home.join(".codex-global-state.json"),
        json!({
            "electron-saved-workspace-roots": ["\\\\?\\C:\\workspace"],
            "project-order": ["\\\\?\\C:\\workspace"],
            "active-workspace-roots": "\\\\?\\C:\\workspace",
            "electron-workspace-root-labels": {"\\\\?\\C:\\workspace": "Workspace"}
        })
        .to_string(),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.changed_session_files, 0);
    assert_eq!(result.sqlite_rows_updated, 3);
    assert_eq!(result.sqlite_provider_rows_updated, 1);
    assert_eq!(result.sqlite_user_event_rows_updated, 1);
    assert_eq!(result.sqlite_cwd_rows_updated, 1);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row: String = db
        .query_row("SELECT cwd FROM threads WHERE id = 'thread-1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(row, "C:/workspace");
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join(".codex-global-state.json")).unwrap())
            .unwrap();
    assert_eq!(
        state["electron-saved-workspace-roots"],
        json!(["C:/workspace"])
    );
    assert_eq!(state["project-order"], json!(["C:/workspace"]));
    assert_eq!(state["active-workspace-roots"], json!("C:/workspace"));
    assert_eq!(
        state["electron-workspace-root-labels"],
        json!({"C:/workspace": "Workspace"})
    );
}

#[test]
fn provider_sync_does_not_restore_cwd_for_projectless_threads() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-projectless.jsonl"),
        "apigather",
        "thread-1",
        "C:/old/project",
    );
    create_state_db(&home.join("state_5.sqlite"));
    fs::write(
        home.join(".codex-global-state.json"),
        json!({
            "projectless-thread-ids": ["thread-1"]
        })
        .to_string(),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.sqlite_cwd_rows_updated, 0);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row: String = db
        .query_row("SELECT cwd FROM threads WHERE id = 'thread-1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(row, "C:/old");
}

#[test]
fn provider_sync_normalizes_open_in_target_preferences_per_path() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-current.jsonl"),
        "apigather",
        "thread-1",
        "\\\\?\\C:\\workspace",
    );
    create_state_db(&home.join("state_5.sqlite"));
    fs::write(
        home.join(".codex-global-state.json"),
        json!({
            "electron-saved-workspace-roots": ["\\\\?\\C:\\workspace"],
            "project-order": ["\\\\?\\C:\\workspace"],
            "active-workspace-roots": ["\\\\?\\C:\\workspace"],
            "electron-workspace-root-labels": {"\\\\?\\C:\\workspace": "Workspace"},
            "open-in-target-preferences": {
                "perPath": {
                    "\\\\?\\C:\\workspace": "terminal"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join(".codex-global-state.json")).unwrap())
            .unwrap();
    assert_eq!(
        state["open-in-target-preferences"]["perPath"],
        json!({"C:/workspace": "terminal"})
    );
    assert!(home.join(".codex-global-state.json.bak").exists());
}

#[test]
fn provider_sync_restores_rollout_first_line_when_later_step_fails() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/rollout-needs-rewrite.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");
    let original_first_line = fs::read_to_string(&rollout)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, archived INTEGER, has_user_event INTEGER, cwd TEXT)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('thread-1', 'old-provider', 0, 0, 'C:/old')",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TRIGGER fail_provider_sync_update BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    assert!(result.message.contains("Provider sync skipped"));
    let restored_first_line = fs::read_to_string(&rollout)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(restored_first_line, original_first_line);
}

#[test]
fn provider_sync_rolls_back_sqlite_provider_update_when_later_update_fails() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-current.jsonl"),
        "apigather",
        "thread-1",
        "C:/workspace",
    );
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, archived INTEGER, has_user_event INTEGER, cwd TEXT)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES ('thread-1', 'old-provider', 0, 1, 'C:/old')",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TRIGGER fail_cwd_update BEFORE UPDATE OF cwd ON threads BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row = db
        .query_row(
            "SELECT model_provider, has_user_event, cwd FROM threads WHERE id = 'thread-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row, ("old-provider".to_string(), 1, "C:/old".to_string()));
}

#[test]
fn provider_sync_restores_global_state_when_later_step_fails() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-current.jsonl"),
        "apigather",
        "thread-1",
        "\\\\?\\C:\\workspace",
    );
    create_state_db(&home.join("state_5.sqlite"));
    let state_path = home.join(".codex-global-state.json");
    let original_state = json!({
        "electron-saved-workspace-roots": ["\\\\?\\C:\\workspace"],
        "project-order": ["\\\\?\\C:\\workspace"]
    })
    .to_string();
    fs::write(&state_path, &original_state).unwrap();
    fs::create_dir_all(home.join("backups_state/provider-sync/blocker")).unwrap();
    fs::write(
        home.join("backups_state/provider-sync/blocker/metadata.json"),
        json!({"managedBy": "CodexElves provider sync"}).to_string(),
    )
    .unwrap();

    let result = run_provider_sync_with_target(Some(&home), Some("bad/provider"));

    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    assert_eq!(fs::read_to_string(&state_path).unwrap(), original_state);
}

#[test]
fn provider_sync_skips_when_home_missing_or_lock_exists_and_prunes_backups() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join(".missing");
    let result = run_provider_sync(Some(&missing));
    assert_eq!(result.status, ProviderSyncStatus::Skipped);

    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::create_dir_all(home.join("tmp/provider-sync.lock")).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let result = run_provider_sync(Some(&home));
    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    assert!(result.message.to_lowercase().contains("lock"));

    fs::remove_dir_all(home.join("tmp/provider-sync.lock")).unwrap();
    let backup_root = home.join("backups_state/provider-sync");
    for index in 0..6 {
        let backup = backup_root.join(format!("2000010100000{index}"));
        fs::create_dir_all(&backup).unwrap();
        fs::write(
            backup.join("metadata.json"),
            json!({"managedBy": "CodexElves provider sync"}).to_string(),
        )
        .unwrap();
    }
    write_rollout(
        &home.join("sessions/rollout-new.jsonl"),
        "openai",
        "thread-1",
        "C:/workspace",
    );
    let result = run_provider_sync(Some(&home));
    assert_eq!(result.status, ProviderSyncStatus::Synced);
    let backups = fs::read_dir(&backup_root)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().is_dir())
        .count();
    assert_eq!(backups, 5);
}

#[test]
fn provider_sync_preserves_rollout_mtime() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-mtime.jsonl");
    write_rollout(&rollout, "openai", "thread-1", "C:/workspace");

    let past = SystemTime::now() - Duration::from_secs(86400);
    let file = fs::File::options().write(true).open(&rollout).unwrap();
    file.set_times(fs::FileTimes::new().set_modified(past))
        .unwrap();
    drop(file);

    let mtime_before = fs::metadata(&rollout).unwrap().modified().unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.changed_session_files, 1);

    let mtime_after = fs::metadata(&rollout).unwrap().modified().unwrap();
    let drift = mtime_after
        .duration_since(mtime_before)
        .or_else(|e| Ok::<_, std::convert::Infallible>(e.duration()))
        .unwrap();
    assert!(
        drift < Duration::from_secs(2),
        "mtime drifted by {drift:?}, expected < 2s"
    );
}

#[test]
fn provider_sync_preview_is_read_only_and_rebuilds_missing_user_index_on_apply() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-missing-index.jsonl");
    write_indexable_rollout(&rollout, "team", "thread-missing", "user");
    create_full_state_db(&home.join("state_5.sqlite"));
    let original = fs::read_to_string(&rollout).unwrap();

    let preview = preview_provider_sync_with_target(Some(&home), Some("custom")).unwrap();

    assert_eq!(preview.active_db_path, Some(home.join("state_5.sqlite")));
    assert_eq!(preview.changed_session_files, 1);
    assert_eq!(preview.sqlite_rows_to_insert, 1);
    assert_eq!(fs::read_to_string(&rollout).unwrap(), original);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);

    let result = run_provider_sync_with_target(Some(&home), Some("custom"));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.sqlite_rows_inserted, 1);
    assert_eq!(result.active_db_path, Some(home.join("state_5.sqlite")));
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row = db
        .query_row(
            "SELECT model_provider, source, cwd, title, sandbox_policy, approval_mode, has_user_event, history_mode
             FROM threads WHERE id = 'thread-missing'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "custom");
    assert_eq!(row.1, "vscode");
    assert_eq!(row.2, "C:/workspace");
    assert_eq!(row.3, "请修复历史会话");
    assert_eq!(row.4, r#"{"type":"danger-full-access"}"#);
    assert_eq!(row.5, "never");
    assert_eq!(row.6, 1);
    assert_eq!(row.7, "legacy");
}

#[test]
fn provider_sync_preview_reports_real_scan_progress() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    for index in 0..30 {
        write_rollout(
            &home.join(format!("sessions/2026/rollout-{index:02}.jsonl")),
            "team",
            &format!("thread-{index:02}"),
            "C:/workspace",
        );
    }
    let events = Mutex::new(Vec::new());

    let preview =
        preview_provider_sync_with_target_and_progress(Some(&home), Some("custom"), |event| {
            events.lock().unwrap().push(event);
        })
        .unwrap();

    assert_eq!(preview.scanned_session_files, 30);
    let events = events.into_inner().unwrap();
    assert_eq!(
        events.first().map(|event| event.phase),
        Some(ProviderSyncProgressPhase::Preparing)
    );
    assert_eq!(
        events.last().map(|event| event.phase),
        Some(ProviderSyncProgressPhase::Complete)
    );
    assert_eq!(events.last().map(|event| event.percent), Some(100));
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].percent <= pair[1].percent)
    );
    let last_scan = events
        .iter()
        .rev()
        .find(|event| event.phase == ProviderSyncProgressPhase::ScanningSessions)
        .unwrap();
    assert_eq!(last_scan.completed, 30);
    assert_eq!(last_scan.total, 30);
}

#[test]
fn manual_provider_sync_cleanup_removes_only_dead_owner_lock() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    let lock_dir = home.join("tmp/provider-sync.lock");
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(
        lock_dir.join("owner.json"),
        json!({
            "pid": u32::MAX,
            "startedAt": 1
        })
        .to_string(),
    )
    .unwrap();

    let removed = cleanup_stale_provider_sync_lock(Some(&home)).unwrap();

    assert!(removed);
    assert!(!lock_dir.exists());
    assert!(home.join("tmp/provider-sync.lock.lease").exists());

    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(
        lock_dir.join("owner.json"),
        json!({
            "pid": std::process::id(),
            "startedAt": 1
        })
        .to_string(),
    )
    .unwrap();

    let error = cleanup_stale_provider_sync_lock(Some(&home)).unwrap_err();

    assert!(error.to_string().contains("正在运行"));
    assert!(lock_dir.exists());
}

#[test]
fn provider_sync_does_not_index_subagent_rollout() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    write_indexable_rollout(
        &home.join("sessions/2026/rollout-subagent.jsonl"),
        "team",
        "thread-subagent",
        "subagent",
    );
    create_full_state_db(&home.join("state_5.sqlite"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.sqlite_rows_inserted, 0);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM threads", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn provider_sync_updates_only_the_most_active_session_database() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    let sqlite_dir = home.join("sqlite");
    fs::create_dir_all(&sqlite_dir).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let stale_db = sqlite_dir.join("state_5.sqlite");
    create_state_db(&stale_db);
    let stale_file = fs::File::options().write(true).open(&stale_db).unwrap();
    stale_file
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(86_400)),
        )
        .unwrap();
    drop(stale_file);
    let active_db = home.join("state_5.sqlite");
    create_state_db(&active_db);
    write_rollout(
        &home.join("sessions/2026/rollout-active-db.jsonl"),
        "team",
        "thread-1",
        "C:/workspace",
    );

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.active_db_path, Some(active_db.clone()));
    let active_provider: String = Connection::open(active_db)
        .unwrap()
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stale_provider: String = Connection::open(stale_db)
        .unwrap()
        .query_row(
            "SELECT model_provider FROM threads WHERE id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_provider, "custom");
    assert_eq!(stale_provider, "old-provider");
}

#[test]
fn provider_sync_recovers_dead_owner_lock_but_blocks_live_owner() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    let lock_dir = home.join("tmp/provider-sync.lock");
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    write_rollout(
        &home.join("sessions/rollout-lock.jsonl"),
        "team",
        "thread-1",
        "C:/workspace",
    );
    fs::write(
        lock_dir.join("owner.json"),
        json!({
            "pid": u32::MAX,
            "startedAt": 1
        })
        .to_string(),
    )
    .unwrap();

    let recovered = run_provider_sync(Some(&home));

    assert_eq!(recovered.status, ProviderSyncStatus::Synced);
    assert!(!lock_dir.exists());

    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(
        lock_dir.join("owner.json"),
        json!({
            "pid": std::process::id(),
            "startedAt": 1
        })
        .to_string(),
    )
    .unwrap();

    let blocked = run_provider_sync(Some(&home));

    assert_eq!(blocked.status, ProviderSyncStatus::Skipped);
    assert_eq!(blocked.error_code.as_deref(), Some("provider_sync_locked"));
    assert!(blocked.message.contains("正在运行"));
    assert!(lock_dir.exists());
}

#[test]
fn provider_sync_rejects_unsupported_threads_schema_before_writes() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/rollout-unsupported-schema.jsonl");
    write_rollout(&rollout, "team", "thread-unsupported", "C:/workspace");
    let original = fs::read_to_string(&rollout).unwrap();
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute("CREATE TABLE threads (id TEXT PRIMARY KEY)", [])
        .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Skipped);
    assert!(result.message.contains("缺少 id 或 model_provider"));
    assert_eq!(fs::read_to_string(&rollout).unwrap(), original);
    assert!(result.backup_dir.is_none());
}
