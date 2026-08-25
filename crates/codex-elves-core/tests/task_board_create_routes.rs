use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use async_trait::async_trait;
use codex_elves_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_elves_core::routes::task_board::TASK_BOARD_CREATE_PATH;
use codex_elves_core::routes::{
    BridgeContext, BridgeDataService, CoreRuntimeService, handle_bridge_request,
};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardAttachConversationsCommand, TaskBoardCatalogProject,
    TaskBoardCatalogSession, TaskBoardConversation, TaskBoardCreateCommand, TaskBoardDocument,
    TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject, TaskBoardSessionCatalog,
    TaskBoardStatus, TaskBoardStore, TaskBoardStoreError, TaskBoardTask,
};
use serde_json::{Value, json};

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const SESSION_A: &str = "019c89c0-0000-7000-8000-000000000001";
const SESSION_B: &str = "019c89c0-0000-7000-8000-000000000002";
const PROJECT_CWD: &str = "E:\\code\\codexelves";

fn runtime() -> Arc<CoreRuntimeService> {
    Arc::new(CoreRuntimeService::new(0, StatusStore::default()))
}

fn context(store: Arc<FakeStore>, data: Arc<CatalogData>) -> BridgeContext {
    BridgeContext::core_with_data(runtime(), data).with_task_board_store(store)
}

fn valid_payload(session_ids: Value) -> Value {
    json!({
        "taskId": format!(" {TASK_ID} "),
        "expectedRevision": 7,
        "title": "  完善任务看板  ",
        "project": {
            "cwd": " e:/CODE/CodexElves/ ",
            "label": "伪造客户端项目名"
        },
        "sessionIds": session_ids
    })
}

fn catalog_with_sessions(sessions: Vec<TaskBoardCatalogSession>) -> TaskBoardSessionCatalog {
    TaskBoardSessionCatalog {
        projects: vec![TaskBoardCatalogProject {
            cwd: "E:\\Code\\CodexElves\\".to_string(),
            label: "Authoritative Project".to_string(),
            session_count: sessions.len().try_into().unwrap(),
        }],
        sessions,
        warnings: Vec::new(),
    }
}

fn session(
    session_id: &str,
    title: &str,
    cwd: &str,
    updated_at_ms: Option<u64>,
) -> TaskBoardCatalogSession {
    TaskBoardCatalogSession {
        session_id: session_id.to_string(),
        title: title.to_string(),
        cwd: cwd.to_string(),
        updated_at_ms,
    }
}

fn response_document(revision: u64) -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision,
        tasks: vec![TaskBoardTask {
            id: TASK_ID.to_string(),
            title: "完善任务看板".to_string(),
            project: TaskBoardProject {
                cwd: PROJECT_CWD.to_string(),
                label: "Authoritative Project".to_string(),
            },
            status: TaskBoardStatus::New,
            order: 0,
            conversations: vec![TaskBoardConversation {
                session_id: SESSION_A.to_string(),
                title: "服务端会话 A".to_string(),
                cwd: PROJECT_CWD.to_string(),
                updated_at_ms: Some(1_787_544_000_001),
            }],
            created_at_ms: 1_787_544_000_100,
            updated_at_ms: 1_787_544_000_100,
        }],
    }
}

fn success_context() -> (BridgeContext, Arc<FakeStore>, Arc<CatalogData>) {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        "E:\\CODE\\CodexElves",
        Some(1_787_544_000_001),
    )])));
    (context(store.clone(), data.clone()), store, data)
}

#[tokio::test]
async fn single_session_create_returns_exact_snapshot_and_uses_authoritative_metadata() {
    let (ctx, store, data) = success_context();

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "tasks": [{
                "id": TASK_ID,
                "title": "完善任务看板",
                "project": {
                    "cwd": PROJECT_CWD,
                    "label": "Authoritative Project"
                },
                "status": "new",
                "order": 0,
                "conversations": [{
                    "sessionId": SESSION_A,
                    "title": "服务端会话 A",
                    "cwd": PROJECT_CWD,
                    "updatedAtMs": 1_787_544_000_001_u64
                }],
                "createdAtMs": 1_787_544_000_100_u64,
                "updatedAtMs": 1_787_544_000_100_u64
            }]
        })
    );
    assert_eq!(data.calls(), 1);
    assert_eq!(store.calls(), 1);
    let command = store.commands().remove(0);
    assert_eq!(command.task_id, TASK_ID);
    assert_eq!(command.expected_revision, 7);
    assert_eq!(command.title, "完善任务看板");
    assert_eq!(
        command.project,
        TaskBoardProject {
            cwd: PROJECT_CWD.to_string(),
            label: "Authoritative Project".to_string(),
        }
    );
    assert_eq!(
        command.conversations,
        vec![TaskBoardConversation {
            session_id: SESSION_A.to_string(),
            title: "服务端会话 A".to_string(),
            cwd: PROJECT_CWD.to_string(),
            updated_at_ms: Some(1_787_544_000_001),
        }]
    );
}

#[tokio::test]
async fn extended_windows_catalog_cwd_matches_normal_requested_project() {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        r"\\?\E:\CODE\CodexElves",
        Some(1_787_544_000_001),
    )])));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(response["status"], "ok");
    assert_eq!(store.calls(), 1);
    let command = store.commands().remove(0);
    assert_eq!(command.project.cwd, PROJECT_CWD);
    assert_eq!(command.conversations[0].cwd, PROJECT_CWD);
}

#[tokio::test]
async fn multiple_session_create_rebuilds_metadata_in_requested_order() {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![
        session(SESSION_A, "服务端会话 A", "E:\\code\\codexelves", Some(101)),
        session(SESSION_B, "服务端会话 B", "e:/CODE/CodexElves/", None),
    ])));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_B, SESSION_A])),
    )
    .await;

    assert_eq!(response["status"], "ok");
    let command = store.commands().remove(0);
    assert_eq!(
        command
            .conversations
            .iter()
            .map(|conversation| (
                conversation.session_id.as_str(),
                conversation.title.as_str(),
                conversation.cwd.as_str(),
                conversation.updated_at_ms,
            ))
            .collect::<Vec<_>>(),
        vec![
            (SESSION_B, "服务端会话 B", PROJECT_CWD, None),
            (SESSION_A, "服务端会话 A", PROJECT_CWD, Some(101)),
        ]
    );
}

#[tokio::test]
async fn project_label_falls_back_to_authoritative_session_cwd_basename() {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(TaskBoardSessionCatalog {
        projects: Vec::new(),
        sessions: vec![session(
            SESSION_A,
            "服务端会话 A",
            "E:\\Code\\FallbackProject\\",
            Some(1),
        )],
        warnings: Vec::new(),
    }));
    let ctx = context(store.clone(), data);
    let mut payload = valid_payload(json!([SESSION_A]));
    payload["project"]["cwd"] = json!("e:/code/fallbackproject");

    let response = handle_bridge_request(ctx, TASK_BOARD_CREATE_PATH, payload).await;

    assert_eq!(response["status"], "ok");
    assert_eq!(store.commands().remove(0).project.label, "FallbackProject");
}

#[tokio::test]
async fn idempotent_no_op_returns_the_complete_current_snapshot() {
    let document = response_document(12);
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: document.clone(),
        changed: false,
        idempotent: true,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        Some(1_787_544_000_001),
    )])));
    let ctx = context(store, data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 12,
            "tasks": document.tasks
        })
    );
}

#[tokio::test]
async fn invalid_or_unknown_create_payloads_fail_before_catalog_or_store() {
    let mut unknown = valid_payload(json!([SESSION_A]));
    unknown["unexpected"] = json!(true);
    let mut project_unknown = valid_payload(json!([SESSION_A]));
    project_unknown["project"]["unexpected"] = json!(true);
    let mut negative_revision = valid_payload(json!([SESSION_A]));
    negative_revision["expectedRevision"] = json!(-1);
    let mut unsafe_revision = valid_payload(json!([SESSION_A]));
    unsafe_revision["expectedRevision"] = json!(TASK_BOARD_MAX_SAFE_INTEGER + 1);
    let mut invalid_uuid = valid_payload(json!([SESSION_A]));
    invalid_uuid["taskId"] = json!("not-a-uuid");
    let mut empty_title = valid_payload(json!([SESSION_A]));
    empty_title["title"] = json!(" \t ");
    let mut long_title = valid_payload(json!([SESSION_A]));
    long_title["title"] = json!("任".repeat(121));
    let empty_sessions = valid_payload(json!([]));
    let empty_session_id = valid_payload(json!([" \t "]));
    let duplicate_sessions = valid_payload(json!([SESSION_A, SESSION_A.to_ascii_uppercase()]));
    let temporary_sessions = [
        "new-thread:temporary",
        "client-new-thread:temporary",
        "local:new-thread:temporary",
        "local:client-new-thread:temporary",
        "provider:region:new-thread:temporary",
        "provider:region:client-new-thread:temporary",
    ]
    .into_iter()
    .map(|session_id| valid_payload(json!([session_id])))
    .collect::<Vec<_>>();
    let mut empty_cwd = valid_payload(json!([SESSION_A]));
    empty_cwd["project"]["cwd"] = json!(" \t ");

    let mut cases = vec![
        unknown,
        project_unknown,
        json!(null),
        json!([]),
        negative_revision,
        unsafe_revision,
        invalid_uuid,
        empty_title,
        long_title,
        empty_sessions,
        empty_session_id,
        duplicate_sessions,
        empty_cwd,
    ];
    cases.extend(temporary_sessions);

    for payload in cases {
        let store = Arc::new(FakeStore::new(StoreOutcome::Success {
            document: response_document(8),
            changed: true,
            idempotent: false,
        }));
        let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
            SESSION_A,
            "服务端会话 A",
            PROJECT_CWD,
            None,
        )])));
        let ctx = context(store.clone(), data.clone());

        let response = handle_bridge_request(ctx, TASK_BOARD_CREATE_PATH, payload).await;

        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert_eq!(store.calls(), 0);
        assert_eq!(data.calls(), 0);
    }
}

#[tokio::test]
async fn missing_authoritative_session_fails_before_store() {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        None,
    )])));
    let ctx = context(store.clone(), data.clone());

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A, SESSION_B])),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "session_not_found",
            "message": "One or more task board sessions were not found"
        })
    );
    assert_eq!(data.calls(), 1);
    assert_eq!(store.calls(), 0);
}

#[tokio::test]
async fn cross_project_authoritative_session_fails_before_store() {
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        "E:\\other-project",
        None,
    )])));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "project_mismatch",
            "message": "Task board sessions must belong to the requested project"
        })
    );
    assert_eq!(store.calls(), 0);
}

#[tokio::test]
async fn catalog_failure_is_private_and_never_calls_store() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let provider_error = concat!(
        "catalog provider failed: ",
        "dbPath=C:\\Users\\alice\\.codex\\state_5.sqlite ",
        "rolloutPath=E:\\private\\rollout\\session.json ",
        "body=private-session-body"
    );
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::failure(provider_error));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "session_catalog_unavailable",
            "message": "Task board session catalog service is unavailable"
        })
    );
    let response_text = serde_json::to_string(&response).unwrap();
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    for private_text in [
        provider_error,
        "dbPath",
        "rolloutPath",
        "C:\\Users\\alice\\.codex\\state_5.sqlite",
        "E:\\private\\rollout\\session.json",
        "private-session-body",
    ] {
        assert!(!response_text.contains(private_text));
        assert!(!diagnostics.contains(private_text));
    }
    assert_eq!(store.calls(), 0);
}

#[tokio::test]
async fn revision_conflict_returns_the_exact_latest_snapshot() {
    let current = response_document(9);
    let store = Arc::new(FakeStore::new(StoreOutcome::RevisionConflict(
        current.clone(),
    )));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        None,
    )])));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "Task board revision conflicts with the current snapshot",
            "schemaVersion": 1,
            "revision": 9,
            "tasks": current.tasks
        })
    );
    assert_eq!(store.calls(), 1);
}

#[tokio::test]
async fn store_failures_use_the_frozen_error_codes() {
    let cases = [
        (StoreOutcome::TaskIdConflict, "task_id_conflict", None),
        (StoreOutcome::Busy, "task_board_busy", None),
        (
            StoreOutcome::InvalidFile(PathBuf::from("E:\\private\\task-board.json")),
            "task_file_invalid",
            Some("E:\\private\\task-board.json"),
        ),
        (
            StoreOutcome::InvalidInput("injected store validation failure".to_string()),
            "invalid_input",
            None,
        ),
    ];

    for (outcome, expected_code, expected_path) in cases {
        let store = Arc::new(FakeStore::new(outcome));
        let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
            SESSION_A,
            "服务端会话 A",
            PROJECT_CWD,
            None,
        )])));
        let ctx = context(store.clone(), data);

        let response = handle_bridge_request(
            ctx,
            TASK_BOARD_CREATE_PATH,
            valid_payload(json!([SESSION_A])),
        )
        .await;

        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], expected_code);
        assert_eq!(response.get("path").and_then(Value::as_str), expected_path);
        assert_eq!(store.calls(), 1);
    }
}

#[tokio::test]
async fn unavailable_create_store_error_is_fixed_and_private() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let unavailable_path = PathBuf::from("E:\\private\\task-board.lock");
    let unavailable_message = concat!(
        "dbPath=C:\\private\\codex.sqlite ",
        "rolloutPath=E:\\secret\\rollout.json ",
        "body=private-storage-body"
    );
    let store = Arc::new(FakeStore::new(StoreOutcome::Unavailable(
        unavailable_path.clone(),
        unavailable_message.to_string(),
    )));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        None,
    )])));

    let response = handle_bridge_request(
        context(store.clone(), data),
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "task_board_unavailable",
            "message": "Task board storage is unavailable"
        })
    );
    let response_text = serde_json::to_string(&response).unwrap();
    let diagnostics = std::fs::read_to_string(log_path).unwrap_or_default();
    for private_text in [
        unavailable_path.to_string_lossy().as_ref(),
        unavailable_message,
        "C:\\private\\codex.sqlite",
        "E:\\secret\\rollout.json",
        "private-storage-body",
    ] {
        assert!(!response_text.contains(private_text));
        assert!(!diagnostics.contains(private_text));
    }
    assert_eq!(store.calls(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn create_runs_the_store_on_a_blocking_worker() {
    let caller_thread = std::thread::current().id();
    let store = Arc::new(FakeStore::new(StoreOutcome::Success {
        document: response_document(8),
        changed: true,
        idempotent: false,
    }));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        None,
    )])));
    let ctx = context(store.clone(), data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    assert_eq!(response["status"], "ok");
    assert_ne!(store.thread_ids().as_slice(), &[caller_thread]);
}

#[tokio::test]
async fn create_join_failure_returns_a_fixed_private_safe_unavailable_error() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let panic_message =
        "dbPath=C:\\private\\task-board.json rolloutPath=E:\\secret\\rollout body=secret";
    let store = Arc::new(FakeStore::new(StoreOutcome::Panic(
        panic_message.to_string(),
    )));
    let data = Arc::new(CatalogData::success(catalog_with_sessions(vec![session(
        SESSION_A,
        "服务端会话 A",
        PROJECT_CWD,
        None,
    )])));
    let ctx = context(store, data);

    let response = handle_bridge_request(
        ctx,
        TASK_BOARD_CREATE_PATH,
        valid_payload(json!([SESSION_A])),
    )
    .await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "task_board_unavailable",
            "message": "Task board storage is unavailable"
        })
    );
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    assert!(
        !serde_json::to_string(&response)
            .unwrap()
            .contains(panic_message)
    );
    assert!(!diagnostics.contains(panic_message));
}

#[derive(Clone)]
enum StoreOutcome {
    Success {
        document: TaskBoardDocument,
        changed: bool,
        idempotent: bool,
    },
    RevisionConflict(TaskBoardDocument),
    TaskIdConflict,
    Busy,
    InvalidFile(PathBuf),
    InvalidInput(String),
    Unavailable(PathBuf, String),
    Panic(String),
}

struct FakeStore {
    outcome: StoreOutcome,
    calls: AtomicUsize,
    commands: Mutex<Vec<TaskBoardCreateCommand>>,
    thread_ids: Mutex<Vec<ThreadId>>,
}

impl FakeStore {
    fn new(outcome: StoreOutcome) -> Self {
        Self {
            outcome,
            calls: AtomicUsize::new(0),
            commands: Mutex::new(Vec::new()),
            thread_ids: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn commands(&self) -> Vec<TaskBoardCreateCommand> {
        self.commands.lock().unwrap().clone()
    }

    fn thread_ids(&self) -> Vec<ThreadId> {
        self.thread_ids.lock().unwrap().clone()
    }
}

impl TaskBoardStore for FakeStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        panic!("create route must not call snapshot")
    }

    fn create_task(
        &self,
        command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.commands.lock().unwrap().push(command);
        self.thread_ids
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        match &self.outcome {
            StoreOutcome::Success {
                document,
                changed,
                idempotent,
            } => Ok(TaskBoardMutationResult {
                document: document.clone(),
                changed: *changed,
                idempotent: *idempotent,
            }),
            StoreOutcome::RevisionConflict(current) => Err(TaskBoardStoreError::RevisionConflict {
                current: current.clone(),
            }),
            StoreOutcome::TaskIdConflict => Err(TaskBoardStoreError::TaskIdConflict),
            StoreOutcome::Busy => Err(TaskBoardStoreError::Busy),
            StoreOutcome::InvalidFile(path) => Err(TaskBoardStoreError::InvalidFile {
                path: path.clone(),
                message: "injected invalid task file".to_string(),
            }),
            StoreOutcome::InvalidInput(message) => Err(TaskBoardStoreError::InvalidInput {
                message: message.clone(),
            }),
            StoreOutcome::Unavailable(path, message) => Err(TaskBoardStoreError::Unavailable {
                path: path.clone(),
                message: message.clone(),
            }),
            StoreOutcome::Panic(message) => panic!("{message}"),
        }
    }

    fn attach_conversations(
        &self,
        _command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("create route must not call attach_conversations")
    }

    fn move_task(
        &self,
        _command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("create route must not call move_task")
    }
}

struct CatalogData {
    catalog: Option<TaskBoardSessionCatalog>,
    error: Option<String>,
    calls: AtomicUsize,
}

impl CatalogData {
    fn success(catalog: TaskBoardSessionCatalog) -> Self {
        Self {
            catalog: Some(catalog),
            error: None,
            calls: AtomicUsize::new(0),
        }
    }

    fn failure(message: &str) -> Self {
        Self {
            catalog: None,
            error: Some(message.to_string()),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl BridgeDataService for CatalogData {
    async fn delete(&self, _session: SessionRef) -> anyhow::Result<DeleteResult> {
        anyhow::bail!("unused data route")
    }

    async fn export_markdown(&self, _session: SessionRef) -> anyhow::Result<ExportResult> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_history(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_summary(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn find_archived_thread_by_title(
        &self,
        _title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        anyhow::bail!("unused data route")
    }

    async fn move_thread_workspace(
        &self,
        _session: SessionRef,
        _target_cwd: String,
    ) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_key(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_keys(&self, _sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn task_board_session_catalog(&self) -> anyhow::Result<TaskBoardSessionCatalog> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match (&self.catalog, &self.error) {
            (Some(catalog), None) => Ok(catalog.clone()),
            (None, Some(error)) => anyhow::bail!("{error}"),
            _ => anyhow::bail!("invalid catalog fake"),
        }
    }
}
