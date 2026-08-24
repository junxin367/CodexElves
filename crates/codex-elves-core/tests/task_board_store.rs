use codex_elves_core::paths::{default_task_board_lock_path, default_task_board_path};
use codex_elves_core::task_board::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardCatalogProject,
    TaskBoardCatalogSession, TaskBoardCatalogWarning, TaskBoardCatalogWarningCode,
    TaskBoardConversation, TaskBoardDocument, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardSessionCatalog, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError, TaskBoardTask,
    normalize_task_project_cwd, parse_task_board_document, task_board_timestamp_from_bridge_i64,
    validate_task_board_document,
};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::time::{Duration, Instant};

fn valid_document() -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        tasks: vec![TaskBoardTask {
            id: "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e".to_string(),
            title: "完善任务看板".to_string(),
            project: TaskBoardProject {
                cwd: "E:\\code\\codexelves".to_string(),
                label: "CodexElves".to_string(),
            },
            status: TaskBoardStatus::Executing,
            order: 0,
            conversations: vec![TaskBoardConversation {
                session_id: "019c89c0-0000-7000-8000-000000000001".to_string(),
                title: "设计任务看板".to_string(),
                cwd: "e:/CODE/CodexElves/".to_string(),
                updated_at_ms: Some(1_787_544_000_000),
            }],
            created_at_ms: 1_787_544_000_000,
            updated_at_ms: 1_787_544_000_000,
        }],
    }
}

#[test]
fn task_board_paths_share_the_app_state_directory() {
    assert!(default_task_board_path().ends_with(".codex-session-delete/task-board.json"));
    assert!(default_task_board_lock_path().ends_with(".codex-session-delete/task-board.lock"));
}

#[test]
fn schema_v1_empty_document_round_trips_with_camel_case_fields() {
    let document = TaskBoardDocument::empty();

    let json = serde_json::to_value(&document).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["revision"], 0);
    assert_eq!(json["tasks"], serde_json::json!([]));
    assert!(json.get("schema_version").is_none());
    assert_eq!(
        serde_json::from_value::<TaskBoardDocument>(json).unwrap(),
        document
    );
}

#[test]
fn task_board_status_uses_the_five_stable_persisted_values() {
    let statuses = [
        TaskBoardStatus::New,
        TaskBoardStatus::Planning,
        TaskBoardStatus::Executing,
        TaskBoardStatus::Review,
        TaskBoardStatus::Done,
    ];

    assert_eq!(
        serde_json::to_value(statuses).unwrap(),
        serde_json::json!(["new", "planning", "executing", "review", "done"])
    );
}

#[test]
fn cwd_normalization_is_lexical_and_uses_platform_path_identity() {
    assert_eq!(
        normalize_task_project_cwd(" e:/Code/./CodexElves/../CodexElves/ ").unwrap(),
        "E:\\code\\codexelves"
    );
    assert_eq!(
        normalize_task_project_cwd(r"\\SERVER\Share\Team\\").unwrap(),
        r"\\server\share\team"
    );
    assert_eq!(
        normalize_task_project_cwd("/Users/Alice\\CodexElves/").unwrap(),
        "/Users/Alice/CodexElves"
    );
    assert_ne!(
        normalize_task_project_cwd("/Users/Alice/CodexElves").unwrap(),
        normalize_task_project_cwd("/Users/alice/CodexElves").unwrap()
    );
    assert_eq!(normalize_task_project_cwd("/").unwrap(), "/");
    assert_eq!(normalize_task_project_cwd("C:\\").unwrap(), "C:\\");
    assert!(normalize_task_project_cwd("   ").is_err());
}

#[test]
fn document_validation_normalizes_titles_and_cwds() {
    let mut document = valid_document();
    document.tasks[0].title = "  完善任务看板  ".to_string();

    validate_task_board_document(&mut document).unwrap();

    assert_eq!(document.tasks[0].title, "完善任务看板");
    assert_eq!(document.tasks[0].project.cwd, "E:\\code\\codexelves");
    assert_eq!(
        document.tasks[0].conversations[0].cwd,
        "E:\\code\\codexelves"
    );
}

#[test]
fn document_validation_rejects_invalid_task_and_conversation_fields() {
    let mut cases = Vec::new();

    let mut unknown_schema = valid_document();
    unknown_schema.schema_version = 2;
    cases.push(unknown_schema);

    let mut invalid_uuid = valid_document();
    invalid_uuid.tasks[0].id = "not-a-uuid".to_string();
    cases.push(invalid_uuid);

    let mut empty_title = valid_document();
    empty_title.tasks[0].title = " \t ".to_string();
    cases.push(empty_title);

    let mut long_title = valid_document();
    long_title.tasks[0].title = "任".repeat(121);
    cases.push(long_title);

    let mut no_conversations = valid_document();
    no_conversations.tasks[0].conversations.clear();
    cases.push(no_conversations);

    let mut temporary_session = valid_document();
    temporary_session.tasks[0].conversations[0].session_id =
        "local:client-new-thread:temporary".to_string();
    cases.push(temporary_session);

    let mut duplicate_session = valid_document();
    let duplicate_conversation = duplicate_session.tasks[0].conversations[0].clone();
    duplicate_session.tasks[0]
        .conversations
        .push(duplicate_conversation);
    cases.push(duplicate_session);

    let mut cross_project = valid_document();
    cross_project.tasks[0].conversations[0].cwd = "E:\\other".to_string();
    cases.push(cross_project);

    let mut broken_order = valid_document();
    broken_order.tasks[0].order = 1;
    cases.push(broken_order);

    for mut document in cases {
        assert!(validate_task_board_document(&mut document).is_err());
    }
}

#[test]
fn document_validation_rejects_all_temporary_new_thread_session_id_variants() {
    let temporary_session_ids = [
        "new-thread:temporary",
        "client-new-thread:temporary",
        "local:new-thread:temporary",
        "local:client-new-thread:temporary",
        "provider:region:new-thread:temporary",
        "provider:region:client-new-thread:temporary",
    ];

    for session_id in temporary_session_ids {
        let mut document = valid_document();
        document.tasks[0].conversations[0].session_id = session_id.to_string();
        assert!(
            validate_task_board_document(&mut document).is_err(),
            "temporary session id was accepted: {session_id}"
        );
    }
}

#[test]
fn document_validation_rejects_duplicate_task_uuid_and_empty_session_id() {
    let mut duplicate_task = valid_document();
    let mut cloned_task = duplicate_task.tasks[0].clone();
    cloned_task.order = 1;
    duplicate_task.tasks.push(cloned_task);
    assert!(matches!(
        validate_task_board_document(&mut duplicate_task),
        Err(codex_elves_core::task_board::TaskBoardValidationError::DuplicateTaskId { .. })
    ));

    let mut empty_session = valid_document();
    empty_session.tasks[0].conversations[0].session_id = " \t ".to_string();
    assert!(matches!(
        validate_task_board_document(&mut empty_session),
        Err(codex_elves_core::task_board::TaskBoardValidationError::EmptySessionId { .. })
    ));
}

#[test]
fn document_validation_enforces_js_safe_integer_boundaries() {
    let mut at_boundary = valid_document();
    at_boundary.revision = TASK_BOARD_MAX_SAFE_INTEGER;
    at_boundary.tasks[0].created_at_ms = TASK_BOARD_MAX_SAFE_INTEGER;
    at_boundary.tasks[0].updated_at_ms = TASK_BOARD_MAX_SAFE_INTEGER;
    at_boundary.tasks[0].conversations[0].updated_at_ms = Some(TASK_BOARD_MAX_SAFE_INTEGER);
    validate_task_board_document(&mut at_boundary).unwrap();

    let mut cases = Vec::new();
    let mut revision = valid_document();
    revision.revision = TASK_BOARD_MAX_SAFE_INTEGER + 1;
    cases.push(revision);

    let mut created = valid_document();
    created.tasks[0].created_at_ms = TASK_BOARD_MAX_SAFE_INTEGER + 1;
    cases.push(created);

    let mut updated = valid_document();
    updated.tasks[0].updated_at_ms = TASK_BOARD_MAX_SAFE_INTEGER + 1;
    cases.push(updated);

    let mut conversation = valid_document();
    conversation.tasks[0].conversations[0].updated_at_ms = Some(TASK_BOARD_MAX_SAFE_INTEGER + 1);
    cases.push(conversation);

    for mut document in cases {
        assert!(validate_task_board_document(&mut document).is_err());
    }
}

#[test]
fn parsing_rejects_unknown_fields_inside_nested_schema_objects() {
    let mut cases = Vec::new();
    let base = serde_json::to_value(valid_document()).unwrap();

    let mut task_unknown = base.clone();
    task_unknown["tasks"][0]["unexpectedTaskField"] = serde_json::json!(true);
    cases.push(task_unknown);

    let mut project_unknown = base.clone();
    project_unknown["tasks"][0]["project"]["unexpectedProjectField"] = serde_json::json!(true);
    cases.push(project_unknown);

    let mut conversation_unknown = base;
    conversation_unknown["tasks"][0]["conversations"][0]["unexpectedConversationField"] =
        serde_json::json!(true);
    cases.push(conversation_unknown);

    for json in cases {
        assert!(parse_task_board_document(&serde_json::to_vec(&json).unwrap()).is_err());
    }
}

#[test]
fn parsing_rejects_negative_timestamps_and_unknown_schema() {
    let negative_timestamp = br#"{
        "schemaVersion":1,
        "revision":0,
        "tasks":[{
            "id":"62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e",
            "title":"task",
            "project":{"cwd":"/tmp/project","label":"project"},
            "status":"new",
            "order":0,
            "conversations":[{
                "sessionId":"019c89c0-0000-7000-8000-000000000001",
                "title":"session",
                "cwd":"/tmp/project",
                "updatedAtMs":null
            }],
            "createdAtMs":-1,
            "updatedAtMs":0
        }]
    }"#;
    assert!(parse_task_board_document(negative_timestamp).is_err());

    let unknown_schema = br#"{"schemaVersion":2,"revision":0,"tasks":[]}"#;
    assert!(parse_task_board_document(unknown_schema).is_err());
}

#[test]
fn missing_task_board_file_returns_an_empty_snapshot_without_creating_data() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let store = FileTaskBoardStore::new(document_path.clone(), lock_path);

    assert_eq!(store.snapshot().unwrap(), TaskBoardDocument::empty());
    assert!(!document_path.exists());
}

#[test]
fn valid_task_board_file_returns_a_complete_normalized_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let document = valid_document();
    std::fs::write(
        &document_path,
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();
    let store = FileTaskBoardStore::new(document_path, lock_path);

    let snapshot = store.snapshot().unwrap();

    assert_eq!(snapshot.revision, 7);
    assert_eq!(snapshot.tasks.len(), 1);
    assert_eq!(
        snapshot.tasks[0].conversations[0].cwd,
        "E:\\code\\codexelves"
    );
}

#[test]
fn exclusive_mutation_atomically_replaces_an_existing_file_in_the_integration_target() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let original = serde_json::to_vec_pretty(&TaskBoardDocument::empty()).unwrap();
    std::fs::write(&document_path, &original).unwrap();
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

    let replaced = std::fs::read(&document_path).unwrap();
    assert_eq!(result.document.revision, 1);
    assert_ne!(replaced, original);
    assert_eq!(
        parse_task_board_document(&replaced).unwrap().revision,
        result.document.revision
    );
    assert!(std::fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn invalid_task_board_file_returns_its_path_and_preserves_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let original = br#"{"schemaVersion":2,"revision":0,"tasks":[]}"#;
    std::fs::write(&document_path, original).unwrap();
    let store = FileTaskBoardStore::new(document_path.clone(), lock_path);

    let error = store.snapshot().unwrap_err();

    assert!(matches!(
        error,
        TaskBoardStoreError::InvalidFile { ref path, .. } if path == &document_path
    ));
    assert_eq!(std::fs::read(&document_path).unwrap(), original);
}

#[test]
fn malformed_task_board_file_is_not_silently_reset() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let original = b"{not-json";
    std::fs::write(&document_path, original).unwrap();
    let store = FileTaskBoardStore::new(document_path.clone(), lock_path);

    assert!(matches!(
        store.snapshot(),
        Err(TaskBoardStoreError::InvalidFile { ref path, .. }) if path == &document_path
    ));
    assert_eq!(std::fs::read(&document_path).unwrap(), original);
}

#[test]
fn snapshot_returns_busy_after_the_bounded_lock_wait() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock_file.lock_exclusive().unwrap();
    let store = FileTaskBoardStore::new(document_path, lock_path)
        .with_lock_timing(Duration::from_millis(40), Duration::from_millis(2));

    assert!(matches!(store.snapshot(), Err(TaskBoardStoreError::Busy)));

    lock_file.unlock().unwrap();
}

#[test]
fn lock_retry_sleep_never_exceeds_the_remaining_timeout_budget() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    let lock_path = temp.path().join("task-board.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock_file.lock_exclusive().unwrap();
    let store = FileTaskBoardStore::new(document_path, lock_path)
        .with_lock_timing(Duration::from_millis(10), Duration::from_millis(200));

    let started_at = Instant::now();
    assert!(matches!(store.snapshot(), Err(TaskBoardStoreError::Busy)));
    assert!(
        started_at.elapsed() < Duration::from_millis(100),
        "lock retry slept beyond the configured timeout budget"
    );

    lock_file.unlock().unwrap();
}

#[test]
fn session_catalog_dtos_use_the_frozen_camel_case_shape() {
    let catalog = TaskBoardSessionCatalog {
        projects: vec![TaskBoardCatalogProject {
            cwd: "E:\\code\\codexelves".to_string(),
            label: "CodexElves".to_string(),
            session_count: 1,
        }],
        sessions: vec![TaskBoardCatalogSession {
            session_id: "019c89c0-0000-7000-8000-000000000001".to_string(),
            title: "session".to_string(),
            cwd: "E:\\code\\codexelves".to_string(),
            updated_at_ms: Some(1_787_544_000_000),
        }],
        warnings: vec![TaskBoardCatalogWarning {
            code: TaskBoardCatalogWarningCode::CodexDbReadFailed,
            count: 1,
        }],
    };

    let json = serde_json::to_value(catalog).unwrap();

    assert_eq!(json["projects"][0]["sessionCount"], 1);
    assert_eq!(
        json["sessions"][0]["sessionId"],
        "019c89c0-0000-7000-8000-000000000001"
    );
    assert_eq!(json["warnings"][0]["code"], "codex_db_read_failed");
}

#[test]
fn session_catalog_timestamp_round_trips_json_null() {
    let json = serde_json::json!({
        "sessionId": "019c89c0-0000-7000-8000-000000000001",
        "title": "session",
        "cwd": "/tmp/project",
        "updatedAtMs": null
    });

    let session = serde_json::from_value::<TaskBoardCatalogSession>(json).unwrap();

    assert_eq!(session.updated_at_ms, None);
    assert_eq!(
        serde_json::to_value(session).unwrap()["updatedAtMs"],
        serde_json::Value::Null
    );
}

#[test]
fn bridge_timestamp_conversion_enforces_nullable_nonnegative_js_safe_values() {
    assert_eq!(task_board_timestamp_from_bridge_i64(None).unwrap(), None);
    assert_eq!(
        task_board_timestamp_from_bridge_i64(Some(0)).unwrap(),
        Some(0)
    );
    assert_eq!(
        task_board_timestamp_from_bridge_i64(Some(TASK_BOARD_MAX_SAFE_INTEGER.try_into().unwrap()))
            .unwrap(),
        Some(TASK_BOARD_MAX_SAFE_INTEGER)
    );
    assert!(task_board_timestamp_from_bridge_i64(Some(-1)).is_err());
    assert!(
        task_board_timestamp_from_bridge_i64(Some(
            (TASK_BOARD_MAX_SAFE_INTEGER + 1).try_into().unwrap()
        ))
        .is_err()
    );
}
