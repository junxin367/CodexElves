use codex_elves_data::{
    ProviderSyncStatus, ProviderSyncTargetSource, audit_provider_sync, load_provider_sync_targets,
    run_provider_sync, run_provider_sync_with_target,
};
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use std::path::Path;
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

fn write_structured_rollout(
    path: &Path,
    provider: &str,
    thread_id: &str,
    source: serde_json::Value,
) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let records = [
        json!({
            "timestamp": "2026-08-06T00:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "model_provider": provider,
                "cwd": "C:/workspace",
                "source": source
            }
        }),
        json!({
            "timestamp": "2026-08-06T00:00:01Z",
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
            records
                .into_iter()
                .map(|record| record.to_string())
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
fn provider_sync_targets_merge_config_and_sqlite_without_scanning_rollouts() {
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
        vec!["custom", "apigather", "openai", "sqlite-provider",]
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
    assert!(openai.sources.contains(&ProviderSyncTargetSource::Sqlite));
    assert!(
        targets
            .targets
            .iter()
            .all(|target| !target.sources.contains(&ProviderSyncTargetSource::Rollout))
    );
    assert!(
        targets
            .targets
            .iter()
            .all(|target| target.id != "legacy-provider")
    );
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
fn provider_sync_accepts_nested_session_meta_ids_when_filename_identifies_primary_thread() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let primary_id = "019fd49e-ea36-7382-9272-20599822ff82";
    let nested_id = "019fd157-b1ec-71c3-b9ff-8f18daf0fc44";
    let rollout = home.join(format!(
        "sessions/2026/rollout-2026-08-06T09-10-12-{nested_id}_{primary_id}.jsonl"
    ));
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    fs::write(
        &rollout,
        format!(
            "{}\n{}\n{}\n",
            json!({
                "type": "session_meta",
                "payload": {
                    "id": primary_id,
                    "model_provider": "old-provider",
                    "source": "vscode"
                }
            }),
            json!({
                "type": "session_meta",
                "payload": {
                    "id": nested_id,
                    "model_provider": "nested-provider",
                    "source": "vscode"
                }
            }),
            json!({
                "type": "event_msg",
                "payload": {"type": "user_message", "message": "hello"}
            })
        ),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert!(
        !result
            .issues
            .iter()
            .any(|issue| issue.kind == codex_elves_data::SessionAnomalyKind::ConflictingThreadIds)
    );
    let providers = fs::read_to_string(rollout)
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
    assert_eq!(providers, vec!["custom", "custom"]);
}

#[test]
fn provider_sync_target_discovery_does_not_scan_session_rollouts() {
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

    assert!(ids.contains(&"custom"));
    assert!(!ids.contains(&"ccx"));
    assert!(!ids.contains(&"CodexElves"));
}

#[test]
fn provider_sync_updates_rollout_sqlite_visibility_without_persistent_backup() {
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
    assert!(result.backup_dir.is_none());
    assert!(
        fs::read_dir(home.join("backups_state/provider-sync"))
            .unwrap()
            .next()
            .is_none()
    );
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
    assert!(result.backup_dir.is_none());
    assert!(
        fs::read_dir(home.join("backups_state/provider-sync"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn provider_sync_removes_successful_and_legacy_provider_sync_backups() {
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
    let legacy_backup = home.join("backups_state/provider-sync/20260612102245");
    fs::create_dir_all(&legacy_backup).unwrap();
    fs::write(
        legacy_backup.join("metadata.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "namespace": "provider-sync",
            "codexHome": home.to_string_lossy(),
            "targetProvider": "openai",
            "changedSessionFiles": 0,
            "managedBy": "Codex++ provider sync"
        }))
        .unwrap(),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert!(result.backup_dir.is_none());
    assert!(
        fs::read_dir(home.join("backups_state/provider-sync"))
            .unwrap()
            .next()
            .is_none()
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

    assert_eq!(result.status, ProviderSyncStatus::Blocked);
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

    assert_eq!(result.status, ProviderSyncStatus::Failed);
    assert!(result.message.contains("Provider sync failed"));
    let restored_first_line = fs::read_to_string(&rollout)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(restored_first_line, original_first_line);
    assert!(
        fs::read_dir(home.join("backups_state/provider-sync"))
            .unwrap()
            .next()
            .is_none()
    );
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

    assert_eq!(result.status, ProviderSyncStatus::Failed);
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
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TRIGGER fail_provider_sync_update BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'boom'); END",
        [],
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Failed);
    assert_eq!(fs::read_to_string(&state_path).unwrap(), original_state);
    assert!(
        fs::read_dir(home.join("backups_state/provider-sync"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn provider_sync_skips_when_home_missing_or_lock_exists_and_cleans_legacy_backups() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join(".missing");
    let result = run_provider_sync(Some(&missing));
    assert_eq!(result.status, ProviderSyncStatus::Blocked);

    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::create_dir_all(home.join("tmp/provider-sync.lock")).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"apigather\"\n").unwrap();
    let result = run_provider_sync(Some(&home));
    assert_eq!(result.status, ProviderSyncStatus::Blocked);
    assert!(result.message.to_lowercase().contains("lock"));

    fs::remove_dir_all(home.join("tmp/provider-sync.lock")).unwrap();
    let backup_root = home.join("backups_state/provider-sync");
    for index in 0..6 {
        let backup = backup_root.join(format!("2000010100000{index}"));
        fs::create_dir_all(&backup).unwrap();
        fs::write(
            backup.join("metadata.json"),
            json!({
                "namespace": "provider-sync",
                "codexHome": home.to_string_lossy(),
                "managedBy": "Codex++ provider sync"
            })
            .to_string(),
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
    assert_eq!(backups, 0);
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
fn provider_sync_rebuilds_missing_user_index() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-missing-index.jsonl");
    write_structured_rollout(&rollout, "old-provider", "thread-missing", json!("vscode"));
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            model_provider TEXT,
            has_user_event INTEGER,
            cwd TEXT,
            source TEXT,
            thread_source TEXT,
            title TEXT
        )",
        [],
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert_eq!(result.sqlite_rows_inserted, 1);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let row = db
        .query_row(
            "SELECT model_provider, has_user_event, source, thread_source, title
             FROM threads WHERE id = 'thread-missing'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "custom");
    assert_eq!(row.1, 1);
    assert_eq!(row.2, "vscode");
    assert_eq!(row.3, "user");
    assert_eq!(row.4, "请修复历史会话");
}

#[test]
fn provider_sync_preserves_paginated_user_event_semantics_and_current_index_metadata() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let existing_rollout = home.join("sessions/2026/rollout-paginated-existing.jsonl");
    let missing_rollout = home.join("sessions/2026/rollout-paginated-missing.jsonl");
    for (path, id) in [
        (&existing_rollout, "thread-existing"),
        (&missing_rollout, "thread-missing"),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "{}\n{}\n{}\n",
                json!({
                    "timestamp": "2026-09-22T00:00:00Z",
                    "type": "session_meta",
                    "payload": {
                        "id": id,
                        "session_id": "parent-session-id",
                        "model_provider": "old-provider",
                        "cwd": "C:/workspace",
                        "source": "vscode",
                        "thread_source": "user",
                        "history_mode": "paginated",
                        "originator": "Codex Desktop",
                        "cli_version": "0.155.0-alpha.9.2",
                        "git": {
                            "commit_hash": "deadbeef",
                            "branch": "main",
                            "repository_url": "https://example.com/repo.git"
                        }
                    }
                }),
                json!({
                    "timestamp": "2026-09-22T00:00:01Z",
                    "type": "turn_context",
                    "payload": {
                        "model": "gpt-5.6-sol",
                        "effort": "high",
                        "sandbox_policy": {"type": "read-only"},
                        "approval_policy": "on-request"
                    }
                }),
                json!({
                    "timestamp": "2026-09-22T00:00:02Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "user_message",
                        "message": "分页历史会话"
                    }
                })
            ),
        )
        .unwrap();
    }
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
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
            git_sha TEXT,
            git_branch TEXT,
            git_origin_url TEXT,
            cli_version TEXT NOT NULL DEFAULT '',
            first_user_message TEXT NOT NULL DEFAULT '',
            model TEXT,
            reasoning_effort TEXT,
            thread_source TEXT,
            preview TEXT NOT NULL DEFAULT '',
            recency_at INTEGER NOT NULL DEFAULT 0,
            recency_at_ms INTEGER NOT NULL DEFAULT 0,
            history_mode TEXT NOT NULL DEFAULT 'legacy',
            is_pinned INTEGER NOT NULL DEFAULT 0,
            originator TEXT
        );
        INSERT INTO threads (
            id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
            sandbox_policy, approval_mode, has_user_event, archived, history_mode
        ) VALUES (
            'thread-existing', '', 0, 0, 'vscode', 'old-provider', 'C:/old', 'Existing',
            '{}', 'on-request', 0, 0, 'PAGINATED'
        );",
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(
        result.status,
        ProviderSyncStatus::Synced,
        "{}",
        result.message
    );
    assert_eq!(result.sqlite_rows_inserted, 1);
    assert_eq!(result.sqlite_user_event_rows_updated, 0);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let existing_has_user_event: i64 = db
        .query_row(
            "SELECT has_user_event FROM threads WHERE id = 'thread-existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(existing_has_user_event, 0);
    let inserted = db
        .query_row(
            "SELECT id, has_user_event, history_mode, originator, model, reasoning_effort,
                    git_sha, git_branch, git_origin_url
             FROM threads WHERE id = 'thread-missing'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        inserted,
        (
            "thread-missing".to_string(),
            0,
            "paginated".to_string(),
            "Codex Desktop".to_string(),
            "gpt-5.6-sol".to_string(),
            "high".to_string(),
            "deadbeef".to_string(),
            "main".to_string(),
            "https://example.com/repo.git".to_string(),
        )
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM threads WHERE id = 'parent-session-id'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn provider_sync_does_not_mark_subagent_as_user_event() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-subagent.jsonl");
    write_structured_rollout(
        &rollout,
        "custom",
        "thread-subagent",
        json!({
            "subagent": {
                "thread_spawn": {
                    "parent_thread_id": "thread-parent",
                    "depth": 1
                }
            }
        }),
    );
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            model_provider TEXT,
            has_user_event INTEGER,
            cwd TEXT
        )",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES (?1, ?2, 'custom', 0, 'C:/workspace')",
        ("thread-subagent", rollout.to_string_lossy().to_string()),
    )
    .unwrap();
    drop(db);

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    let has_user_event: i64 = db
        .query_row(
            "SELECT has_user_event FROM threads WHERE id = 'thread-subagent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(has_user_event, 0);
}

#[test]
fn provider_sync_recovers_dead_owner_lock() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    let lock_dir = home.join("tmp/provider-sync.lock");
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    fs::write(
        lock_dir.join("owner.json"),
        json!({"pid": u32::MAX, "startedAt": 1}).to_string(),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert!(!lock_dir.exists());
}

#[test]
fn provider_sync_cleans_stale_atomic_replacement_artifacts() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-stale-artifact.jsonl");
    write_rollout(&rollout, "custom", "thread-1", "C:/workspace");
    let file_name = rollout.file_name().unwrap().to_string_lossy();
    let old_path = rollout
        .parent()
        .unwrap()
        .join(format!(".{file_name}.provider-sync-deadbeef.old"));
    let temp_path = rollout
        .parent()
        .unwrap()
        .join(format!(".{file_name}.provider-sync-deadbeef.tmp"));
    fs::copy(&rollout, &old_path).unwrap();
    fs::copy(&rollout, &temp_path).unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Synced);
    assert!(!old_path.exists());
    assert!(!temp_path.exists());
    assert!(rollout.exists());
}

#[test]
fn provider_sync_preserves_malformed_lines_and_repairs_valid_metadata() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-malformed.jsonl");
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    fs::write(
        &rollout,
        format!(
            "{}\nnot-json\n{}\n",
            json!({
                "type": "session_meta",
                "payload": {
                    "id": "thread-malformed",
                    "model_provider": "old-provider",
                    "source": "vscode"
                }
            }),
            json!({
                "type": "event_msg",
                "payload": {"type": "user_message", "message": "hello"}
            })
        ),
    )
    .unwrap();

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Partial);
    let text = fs::read_to_string(&rollout).unwrap();
    assert!(text.contains("\nnot-json\n"));
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["payload"]["model_provider"], "custom");
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.kind == codex_elves_data::SessionAnomalyKind::MalformedJson)
    );
}

#[test]
fn provider_sync_updates_only_the_active_threads_database() {
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
    write_structured_rollout(
        &home.join("sessions/2026/rollout-active-db.jsonl"),
        "old-provider",
        "thread-1",
        json!("vscode"),
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
fn provider_sync_isolates_duplicate_thread_ids() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let first = home.join("sessions/2026/rollout-duplicate-a.jsonl");
    let second = home.join("sessions/2026/rollout-duplicate-b.jsonl");
    write_structured_rollout(&first, "old-provider", "thread-duplicate", json!("vscode"));
    write_structured_rollout(&second, "old-provider", "thread-duplicate", json!("vscode"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Partial);
    for path in [first, second] {
        let first_line: serde_json::Value =
            serde_json::from_str(fs::read_to_string(path).unwrap().lines().next().unwrap())
                .unwrap();
        assert_eq!(first_line["payload"]["model_provider"], "old-provider");
    }
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|issue| issue.kind == codex_elves_data::SessionAnomalyKind::DuplicateThreadId)
            .count(),
        2
    );
}

#[test]
fn provider_sync_repairs_healthy_sessions_when_an_empty_rollout_exists() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let empty = home.join("sessions/2026/rollout-empty.jsonl");
    fs::create_dir_all(empty.parent().unwrap()).unwrap();
    fs::write(&empty, "").unwrap();
    let healthy = home.join("sessions/2026/rollout-healthy.jsonl");
    write_structured_rollout(&healthy, "old-provider", "thread-healthy", json!("vscode"));

    let result = run_provider_sync(Some(&home));

    assert_eq!(result.status, ProviderSyncStatus::Partial);
    let first_line: serde_json::Value =
        serde_json::from_str(fs::read_to_string(healthy).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(first_line["payload"]["model_provider"], "custom");
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.kind == codex_elves_data::SessionAnomalyKind::EmptyFile)
    );
}

#[test]
fn provider_sync_recovers_an_unfinished_operation_before_new_writes() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let rollout = home.join("sessions/2026/rollout-recovery.jsonl");
    write_structured_rollout(&rollout, "old-provider", "thread-1", json!("vscode"));
    create_state_db(&home.join("state_5.sqlite"));

    let first = run_provider_sync(Some(&home));
    assert_eq!(first.status, ProviderSyncStatus::Synced);
    assert!(first.backup_dir.is_none());
    let first_backup = home.join("backups_state/provider-sync/20260922000000");
    let backup_rollout = first_backup
        .join("rollouts")
        .join(rollout.strip_prefix(&home).unwrap());
    fs::create_dir_all(backup_rollout.parent().unwrap()).unwrap();
    fs::create_dir_all(first_backup.join("db")).unwrap();
    fs::copy(&rollout, &backup_rollout).unwrap();
    fs::copy(
        home.join("state_5.sqlite"),
        first_backup.join("db/state_5.sqlite"),
    )
    .unwrap();
    fs::write(
        first_backup.join("session-meta-backup.json"),
        serde_json::to_vec_pretty(&json!([{
            "path": rollout.to_string_lossy(),
            "backupPath": backup_rollout.to_string_lossy()
        }]))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        first_backup.join("metadata.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "namespace": "provider-sync",
            "codexHome": home.to_string_lossy(),
            "targetProvider": "custom",
            "operationId": "interrupted-operation",
            "changedSessionFiles": 1,
            "managedBy": "CodexElves provider sync"
        }))
        .unwrap(),
    )
    .unwrap();
    let operation_path = first_backup.join("operation.json");
    fs::write(
        &operation_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "operationId": "interrupted-operation",
            "status": "files_applied",
            "updatedAt": "2026-09-22T00:00:00Z",
            "error": null
        }))
        .unwrap(),
    )
    .unwrap();
    let mut rollout_value: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&rollout)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    rollout_value["payload"]["model_provider"] = json!("corrupted");
    let original_tail = fs::read_to_string(&rollout)
        .unwrap()
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&rollout, format!("{rollout_value}\n{original_tail}\n")).unwrap();
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "UPDATE threads SET model_provider = 'corrupted' WHERE id = 'thread-1'",
        [],
    )
    .unwrap();
    drop(db);

    let second = run_provider_sync(Some(&home));

    assert_eq!(
        second.status,
        ProviderSyncStatus::Synced,
        "{}",
        second.message
    );
    assert!(!first_backup.exists());
    let first_line: serde_json::Value =
        serde_json::from_str(fs::read_to_string(rollout).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(first_line["payload"]["model_provider"], "custom");
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
fn provider_sync_audit_is_read_only_and_reports_repair_plan() {
    let tmp = tempdir().unwrap();
    let home = tmp.path().join(".codex");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
    let user_rollout = home.join("sessions/2026/rollout-audit-user.jsonl");
    write_structured_rollout(
        &user_rollout,
        "old-provider",
        "thread-user",
        json!("vscode"),
    );
    write_structured_rollout(
        &home.join("sessions/2026/rollout-audit-subagent.jsonl"),
        "old-provider",
        "thread-subagent",
        json!({"subagent": {"thread_spawn": {"parent_thread_id": "thread-user"}}}),
    );
    let empty = home.join("sessions/2026/rollout-audit-empty.jsonl");
    fs::write(&empty, "").unwrap();
    let db = Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            model_provider TEXT,
            has_user_event INTEGER,
            cwd TEXT,
            source TEXT,
            thread_source TEXT,
            title TEXT
        )",
        [],
    )
    .unwrap();
    drop(db);
    let lock_dir = home.join("tmp/provider-sync.lock");
    fs::create_dir_all(&lock_dir).unwrap();
    fs::write(
        lock_dir.join("owner.json"),
        json!({"pid": u32::MAX, "startedAt": 1}).to_string(),
    )
    .unwrap();
    let original = fs::read_to_string(&user_rollout).unwrap();
    let original_mtime = fs::metadata(&user_rollout).unwrap().modified().unwrap();

    let audit = audit_provider_sync(Some(&home), None).unwrap();

    assert_eq!(audit.scanned_session_files, 3);
    assert_eq!(audit.user_sessions, 1);
    assert_eq!(audit.subagent_sessions, 1);
    assert_eq!(audit.sqlite_rows_to_insert, 1);
    assert!(audit.stale_lock_detected);
    assert!(
        audit
            .issues
            .iter()
            .any(|issue| issue.kind == codex_elves_data::SessionAnomalyKind::EmptyFile)
    );
    assert_eq!(fs::read_to_string(&user_rollout).unwrap(), original);
    assert_eq!(
        fs::metadata(&user_rollout).unwrap().modified().unwrap(),
        original_mtime
    );
    assert!(lock_dir.exists());
}
