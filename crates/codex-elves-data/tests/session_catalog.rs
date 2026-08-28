use codex_elves_data::{
    LocalSessionCatalogError, LocalSessionCatalogWarning, aggregate_local_session_catalog,
};
use rusqlite::Connection;
use serde_json::to_value;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn create_thread_db(path: &Path, rows: &[(&str, &str, &str, bool, i64)]) {
    let db = Connection::open(path).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            title TEXT,
            cwd TEXT,
            archived INTEGER,
            updated_at_ms INTEGER
        )",
        [],
    )
    .unwrap();
    for (id, title, cwd, archived, updated_at_ms) in rows {
        db.execute(
            "INSERT INTO threads VALUES (?1, '', ?2, ?3, ?4, ?5)",
            (id, title, cwd, i64::from(*archived), updated_at_ms),
        )
        .unwrap();
    }
}

fn create_automation_db(path: &Path, rows: &[(&str, &str, &str, &str, i64)]) {
    let db = Connection::open(path).unwrap();
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
    for (id, status, title, cwd, updated_at) in rows {
        db.execute(
            "INSERT INTO automation_runs VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            (id, status, title, cwd, updated_at),
        )
        .unwrap();
    }
}

fn create_thread_db_with_optional_updated_at(
    path: &Path,
    rows: &[(&str, &str, &str, Option<i64>)],
) {
    let db = Connection::open(path).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            title TEXT,
            cwd TEXT,
            archived INTEGER,
            updated_at_ms INTEGER
        )",
        [],
    )
    .unwrap();
    for (id, title, cwd, updated_at_ms) in rows {
        db.execute(
            "INSERT INTO threads VALUES (?1, '', ?2, ?3, 0, ?4)",
            (id, title, cwd, updated_at_ms),
        )
        .unwrap();
    }
}

#[test]
fn catalog_excludes_subagent_threads_from_spawn_edges() {
    let temp = tempdir().unwrap();
    let threads = temp.path().join("state.sqlite");
    create_thread_db(
        &threads,
        &[
            ("main-thread", "main session", "C:/workspace", false, 300),
            ("sub-thread", "subagent session", "C:/workspace", false, 200),
            (
                "standalone",
                "standalone session",
                "C:/workspace",
                false,
                100,
            ),
        ],
    );
    {
        let db = Connection::open(&threads).unwrap();
        db.execute(
            "CREATE TABLE thread_spawn_edges (
                parent_thread_id TEXT NOT NULL,
                child_thread_id TEXT NOT NULL,
                status TEXT
            )",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO thread_spawn_edges VALUES ('main-thread', 'sub-thread', 'running')",
            [],
        )
        .unwrap();
    }

    let catalog = aggregate_local_session_catalog(&[threads]).unwrap();

    let session_ids = catalog
        .sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(session_ids, vec!["main-thread", "standalone"]);
}

#[test]
fn catalog_excludes_subagent_threads_from_source_without_spawn_edges() {
    let temp = tempdir().unwrap();
    let threads = temp.path().join("state.sqlite");
    let db = Connection::open(&threads).unwrap();
    db.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT,
            title TEXT,
            cwd TEXT,
            archived INTEGER,
            updated_at_ms INTEGER,
            source TEXT,
            thread_source TEXT
        )",
        [],
    )
    .unwrap();
    db.execute(
        "CREATE TABLE thread_spawn_edges (
            parent_thread_id TEXT NOT NULL,
            child_thread_id TEXT NOT NULL,
            status TEXT
        )",
        [],
    )
    .unwrap();
    let rows = [
        ("main-thread", "main session", r#"{"cli":{}}"#, "user", 500),
        (
            "source-sub-thread",
            "source subagent",
            r#"{"subagent":{"thread_spawn":{"parent_thread_id":"main-thread"}}}"#,
            "subagent",
            400,
        ),
        (
            "source-agent-thread",
            "legacy agent source",
            r#"{"agent":{"parent_thread_id":"main-thread"}}"#,
            "",
            300,
        ),
        (
            "thread-source-sub-thread",
            "thread source subagent",
            "",
            "subagent",
            200,
        ),
        (
            "standalone",
            "standalone session",
            r#"{"cli":{}}"#,
            "user",
            100,
        ),
    ];
    for (id, title, source, thread_source, updated_at_ms) in rows {
        db.execute(
            "INSERT INTO threads VALUES (?1, '', ?2, 'C:/workspace', 0, ?3, ?4, ?5)",
            (id, title, updated_at_ms, source, thread_source),
        )
        .unwrap();
    }
    drop(db);

    let catalog = aggregate_local_session_catalog(&[threads]).unwrap();

    let session_ids = catalog
        .sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(session_ids, vec!["main-thread", "standalone"]);
}

#[test]
fn catalog_excludes_spawn_child_when_edge_and_thread_are_in_different_databases() {
    let temp = tempdir().unwrap();
    let current = temp.path().join("sqlite").join("state_5.sqlite");
    let legacy = temp.path().join("state_5.sqlite");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    create_thread_db(
        &current,
        &[
            ("cross-db-child", "subagent", "C:/workspace", false, 300),
            ("current-main", "current main", "C:/workspace", false, 200),
        ],
    );
    create_thread_db(
        &legacy,
        &[("legacy-main", "legacy main", "C:/workspace", false, 100)],
    );
    let db = Connection::open(&legacy).unwrap();
    db.execute(
        "CREATE TABLE thread_spawn_edges (
            parent_thread_id TEXT NOT NULL,
            child_thread_id TEXT NOT NULL,
            status TEXT
        )",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges VALUES ('legacy-main', 'cross-db-child', 'running')",
        [],
    )
    .unwrap();
    drop(db);

    let catalog = aggregate_local_session_catalog(&[current, legacy]).unwrap();

    let session_ids = catalog
        .sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(session_ids, vec!["current-main", "legacy-main"]);
}

#[test]
fn catalog_aggregates_thread_and_automation_sessions_with_latest_id_winner() {
    let temp = tempdir().unwrap();
    let threads = temp.path().join("state.sqlite");
    let automation = temp.path().join("automation.sqlite");
    create_thread_db(
        &threads,
        &[
            ("shared", "old thread", "C:/workspace", false, 100),
            ("thread-only", "thread", "C:/thread", false, 300),
            ("archived-thread", "archived", "C:/archived", true, 500),
            ("empty-id", "empty", "C:/empty-id", false, 450),
            ("empty-cwd", "empty", "", false, 400),
        ],
    );
    Connection::open(&threads)
        .unwrap()
        .execute("UPDATE threads SET id = '' WHERE id = 'empty-id'", [])
        .unwrap();
    create_automation_db(
        &automation,
        &[
            ("shared", "running", "new automation", "C:/workspace", 200),
            (
                "automation-only",
                "running",
                "automation",
                "C:/automation",
                300,
            ),
            (
                "archived-automation",
                "archived",
                "archived",
                "C:/archived",
                800,
            ),
        ],
    );

    let catalog =
        aggregate_local_session_catalog(&[threads.clone(), automation.clone(), threads]).unwrap();

    assert_eq!(catalog.warnings, Vec::<LocalSessionCatalogWarning>::new());
    assert_eq!(
        catalog
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec!["automation-only", "thread-only", "shared"]
    );
    assert_eq!(catalog.sessions[2].title, "new automation");
    assert_eq!(catalog.sessions[2].updated_at_ms, Some(200));
}

#[test]
fn catalog_returns_aggregate_warning_without_database_path_when_one_database_fails() {
    let temp = tempdir().unwrap();
    let valid = temp.path().join("state.sqlite");
    let invalid = temp.path().join("broken.sqlite");
    create_thread_db(&valid, &[("thread", "title", "C:/workspace", false, 100)]);
    fs::write(&invalid, "not a sqlite database").unwrap();

    let catalog = aggregate_local_session_catalog(&[valid, invalid.clone(), invalid]).unwrap();

    assert_eq!(
        catalog.warnings,
        vec![LocalSessionCatalogWarning::DatabaseReadFailed { count: 1 }]
    );
    assert_eq!(catalog.sessions.len(), 1);
    assert!(!format!("{:?}", catalog.warnings).contains(&temp.path().display().to_string()));
}

#[test]
fn catalog_fails_with_typed_path_free_error_once_for_a_canonical_duplicate_path() {
    let temp = tempdir().unwrap();
    let invalid = temp.path().join("broken.sqlite");
    let alias = temp.path().join(".").join("broken.sqlite");
    fs::write(&invalid, "not a sqlite database").unwrap();

    let error =
        aggregate_local_session_catalog(&[temp.path().join("missing.sqlite"), invalid, alias])
            .unwrap_err();

    assert_eq!(
        error,
        LocalSessionCatalogError::AllExistingDatabasesFailed { count: 1 }
    );
    assert!(
        !error
            .to_string()
            .contains(&temp.path().display().to_string())
    );
}

#[test]
fn catalog_returns_empty_success_when_no_candidate_database_exists() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing.sqlite");

    let catalog = aggregate_local_session_catalog(&[missing.clone()]).unwrap();

    assert!(catalog.sessions.is_empty());
    assert!(catalog.warnings.is_empty());
    assert!(!missing.exists());
}

#[test]
fn catalog_serialization_never_exposes_database_or_rollout_paths() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("state.sqlite");
    create_thread_db(
        &database,
        &[("thread", "title", "C:/workspace", false, 100)],
    );

    let catalog = aggregate_local_session_catalog(&[database]).unwrap();
    let value = to_value(catalog).unwrap();
    let session = &value["sessions"][0];

    assert!(session.get("dbPath").is_none());
    assert!(session.get("rolloutPath").is_none());
    assert!(session.get("archived").is_none());
    assert!(session.get("isSubagent").is_none());
}

#[test]
fn catalog_prefers_newer_current_or_legacy_thread_for_the_same_id() {
    let temp = tempdir().unwrap();
    let current = temp.path().join("sqlite").join("state_5.sqlite");
    let legacy = temp.path().join("state_5.sqlite");
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    create_thread_db(
        &current,
        &[("shared", "current copy", "C:/workspace", false, 100)],
    );
    create_thread_db(
        &legacy,
        &[("shared", "legacy copy", "C:/workspace", false, 200)],
    );

    let catalog = aggregate_local_session_catalog(&[current, legacy]).unwrap();

    assert_eq!(catalog.sessions.len(), 1);
    assert_eq!(catalog.sessions[0].id, "shared");
    assert_eq!(catalog.sessions[0].title, "legacy copy");
    assert_eq!(catalog.sessions[0].updated_at_ms, Some(200));
}

#[test]
fn catalog_sorts_some_timestamps_before_none_and_ids_ascending_on_ties() {
    let temp = tempdir().unwrap();
    let current = temp.path().join("current.sqlite");
    let legacy = temp.path().join("legacy.sqlite");
    create_thread_db_with_optional_updated_at(
        &current,
        &[
            ("zeta", "zeta", "C:/zeta", Some(200)),
            ("alpha", "alpha", "C:/alpha", Some(200)),
            ("duplicate", "old duplicate", "C:/duplicate", None),
            ("no-time", "no time", "C:/none", None),
        ],
    );
    create_thread_db_with_optional_updated_at(
        &legacy,
        &[("duplicate", "new duplicate", "C:/duplicate", Some(100))],
    );

    let catalog = aggregate_local_session_catalog(&[current, legacy]).unwrap();

    assert_eq!(
        catalog
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zeta", "duplicate", "no-time"]
    );
    assert_eq!(catalog.sessions[2].title, "new duplicate");
    assert_eq!(catalog.sessions[2].updated_at_ms, Some(100));
}
