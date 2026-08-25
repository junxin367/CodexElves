use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use codex_elves_core::routes::task_board::TASK_BOARD_MOVE_PATH;
use codex_elves_core::routes::{BridgeContext, CoreRuntimeService, handle_bridge_request};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    TaskBoardAttachConversationsCommand, TaskBoardConversation, TaskBoardCreateCommand,
    TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardStatus, TaskBoardStore, TaskBoardStoreError, TaskBoardTask,
};
use serde_json::{Value, json};

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn context(store: Arc<dyn TaskBoardStore>) -> BridgeContext {
    BridgeContext::core(Arc::new(CoreRuntimeService::new(0, StatusStore::default())))
        .with_task_board_store(store)
}

fn task(status: TaskBoardStatus) -> TaskBoardTask {
    TaskBoardTask {
        id: TASK_ID.to_string(),
        title: "完善任务看板".to_string(),
        project: TaskBoardProject {
            cwd: "E:\\code\\codexelves".to_string(),
            label: "CodexElves".to_string(),
        },
        status,
        order: 0,
        conversations: vec![TaskBoardConversation {
            session_id: "019c89c0-0000-7000-8000-000000000001".to_string(),
            title: "设计任务看板".to_string(),
            cwd: "E:\\code\\codexelves".to_string(),
            updated_at_ms: Some(1_787_544_000_000),
        }],
        created_at_ms: 1_787_544_000_000,
        updated_at_ms: 1_787_544_000_001,
    }
}

fn document(revision: u64, status: TaskBoardStatus) -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision,
        tasks: vec![task(status)],
    }
}

fn move_payload(status: &str, target_index: u32, expected_revision: u64) -> Value {
    json!({
        "taskId": TASK_ID,
        "toStatus": status,
        "targetIndex": target_index,
        "expectedRevision": expected_revision
    })
}

fn expected_snapshot(revision: u64, status: &str) -> Value {
    json!({
        "status": "ok",
        "schemaVersion": 1,
        "revision": revision,
        "tasks": [{
            "id": TASK_ID,
            "title": "完善任务看板",
            "project": {
                "cwd": "E:\\code\\codexelves",
                "label": "CodexElves"
            },
            "status": status,
            "order": 0,
            "conversations": [{
                "sessionId": "019c89c0-0000-7000-8000-000000000001",
                "title": "设计任务看板",
                "cwd": "E:\\code\\codexelves",
                "updatedAtMs": 1_787_544_000_000_u64
            }],
            "createdAtMs": 1_787_544_000_000_u64,
            "updatedAtMs": 1_787_544_000_001_u64
        }]
    })
}

#[tokio::test]
async fn move_accepts_all_five_statuses_and_forwards_zero_and_end_indexes() {
    let cases = [
        (TaskBoardStatus::New, "new", 0),
        (TaskBoardStatus::Planning, "planning", 2),
        (TaskBoardStatus::Executing, "executing", 0),
        (TaskBoardStatus::Review, "review", 3),
        (TaskBoardStatus::Done, "done", 4),
    ];

    for (status, wire_status, target_index) in cases {
        let store = Arc::new(FakeMoveStore::success(
            TaskBoardMutationResult {
                document: document(8, status),
                changed: true,
                idempotent: false,
            },
            None,
        ));

        let response = handle_bridge_request(
            context(store.clone()),
            TASK_BOARD_MOVE_PATH,
            move_payload(wire_status, target_index, 7),
        )
        .await;

        assert_eq!(response, expected_snapshot(8, wire_status));
        assert_eq!(
            store.calls(),
            vec![TaskBoardMoveCommand {
                task_id: TASK_ID.to_string(),
                to_status: status,
                target_index,
                expected_revision: 7,
            }]
        );
    }
}

#[tokio::test]
async fn move_noop_returns_the_full_success_snapshot() {
    let store = Arc::new(FakeMoveStore::success(
        TaskBoardMutationResult {
            document: document(7, TaskBoardStatus::Planning),
            changed: false,
            idempotent: false,
        },
        None,
    ));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_MOVE_PATH,
        move_payload("planning", 0, 7),
    )
    .await;

    assert_eq!(response, expected_snapshot(7, "planning"));
    assert_eq!(store.calls().len(), 1);
}

#[tokio::test]
async fn invalid_move_requests_are_rejected_without_calling_the_store() {
    let store = Arc::new(FakeMoveStore::success(
        TaskBoardMutationResult {
            document: document(8, TaskBoardStatus::Review),
            changed: true,
            idempotent: false,
        },
        None,
    ));
    let invalid_payloads = [
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": 0,
            "expectedRevision": 7,
            "unexpected": true
        }),
        json!({
            "taskId": "not-a-uuid",
            "toStatus": "review",
            "targetIndex": 0,
            "expectedRevision": 7
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "blocked",
            "targetIndex": 0,
            "expectedRevision": 7
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": -1,
            "expectedRevision": 7
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": 0.5,
            "expectedRevision": 7
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": 0,
            "expectedRevision": -1
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": 0,
            "expectedRevision": 0.5
        }),
        json!({
            "taskId": TASK_ID,
            "toStatus": "review",
            "targetIndex": 0,
            "expectedRevision": JS_MAX_SAFE_INTEGER + 1
        }),
    ];

    for payload in invalid_payloads {
        let response =
            handle_bridge_request(context(store.clone()), TASK_BOARD_MOVE_PATH, payload).await;
        assert_eq!(
            response,
            json!({
                "status": "failed",
                "code": "invalid_input",
                "message": "Task board move request is invalid"
            })
        );
    }
    assert!(store.calls().is_empty());
}

#[tokio::test]
async fn move_store_errors_use_the_shared_frozen_error_envelopes() {
    let invalid_file_path = PathBuf::from("E:\\private\\invalid-task-board.json");
    let cases = [
        (
            FakeMoveOutcome::Error(FakeMoveError::InvalidInput("bad target".to_string())),
            json!({
                "status": "failed",
                "code": "invalid_input",
                "message": "bad target"
            }),
        ),
        (
            FakeMoveOutcome::Error(FakeMoveError::TaskNotFound),
            json!({
                "status": "failed",
                "code": "task_not_found",
                "message": "Task was not found"
            }),
        ),
        (
            FakeMoveOutcome::Error(FakeMoveError::Busy),
            json!({
                "status": "failed",
                "code": "task_board_busy",
                "message": "Task board storage is busy"
            }),
        ),
        (
            FakeMoveOutcome::Error(FakeMoveError::InvalidFile(
                invalid_file_path.clone(),
                "invalid JSON".to_string(),
            )),
            json!({
                "status": "failed",
                "code": "task_file_invalid",
                "message": "Task board file is invalid: invalid JSON",
                "path": invalid_file_path.to_string_lossy()
            }),
        ),
        (
            FakeMoveOutcome::Error(FakeMoveError::TaskIdConflict),
            json!({
                "status": "failed",
                "code": "task_id_conflict",
                "message": "Task id conflicts with an existing task"
            }),
        ),
    ];

    for (outcome, expected) in cases {
        let store = Arc::new(FakeMoveStore::with_outcome(outcome, None));
        let response = handle_bridge_request(
            context(store.clone()),
            TASK_BOARD_MOVE_PATH,
            move_payload("review", 0, 7),
        )
        .await;
        assert_eq!(response, expected);
        assert_eq!(store.calls().len(), 1);
    }
}

#[tokio::test]
async fn unavailable_move_store_error_is_fixed_and_private() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let unavailable_path = PathBuf::from("E:\\private\\task-board.lock");
    let unavailable_message = concat!(
        "dbPath=C:\\private\\codex.sqlite ",
        "rolloutPath=E:\\secret\\rollout.json ",
        "body=private-storage-body"
    );
    let store = Arc::new(FakeMoveStore::with_outcome(
        FakeMoveOutcome::Error(FakeMoveError::Unavailable(
            unavailable_path.clone(),
            unavailable_message.to_string(),
        )),
        None,
    ));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_MOVE_PATH,
        move_payload("review", 0, 7),
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
    assert_eq!(store.calls().len(), 1);
}

#[tokio::test]
async fn revision_conflict_returns_the_exact_latest_flattened_snapshot() {
    let current = document(12, TaskBoardStatus::Done);
    let store = Arc::new(FakeMoveStore::with_outcome(
        FakeMoveOutcome::Error(FakeMoveError::RevisionConflict(current)),
        None,
    ));

    let response = handle_bridge_request(
        context(store),
        TASK_BOARD_MOVE_PATH,
        move_payload("done", 0, 7),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "Task board revision conflicts with the current snapshot",
            "schemaVersion": 1,
            "revision": 12,
            "tasks": [{
                "id": TASK_ID,
                "title": "完善任务看板",
                "project": {
                    "cwd": "E:\\code\\codexelves",
                    "label": "CodexElves"
                },
                "status": "done",
                "order": 0,
                "conversations": [{
                    "sessionId": "019c89c0-0000-7000-8000-000000000001",
                    "title": "设计任务看板",
                    "cwd": "E:\\code\\codexelves",
                    "updatedAtMs": 1_787_544_000_000_u64
                }],
                "createdAtMs": 1_787_544_000_000_u64,
                "updatedAtMs": 1_787_544_000_001_u64
            }]
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn move_uses_a_blocking_worker_for_the_store_call() {
    let observed_thread = Arc::new(Mutex::new(None));
    let caller_thread = std::thread::current().id();
    let store = Arc::new(FakeMoveStore::success(
        TaskBoardMutationResult {
            document: document(8, TaskBoardStatus::Executing),
            changed: true,
            idempotent: false,
        },
        Some(observed_thread.clone()),
    ));

    let response = handle_bridge_request(
        context(store),
        TASK_BOARD_MOVE_PATH,
        move_payload("executing", 0, 7),
    )
    .await;

    assert_eq!(response, expected_snapshot(8, "executing"));
    assert_ne!(*observed_thread.lock().unwrap(), Some(caller_thread));
}

#[tokio::test]
async fn move_worker_join_failure_is_fixed_and_does_not_leak_panic_text() {
    let store = Arc::new(FakeMoveStore::with_outcome(FakeMoveOutcome::Panic, None));

    let response = handle_bridge_request(
        context(store),
        TASK_BOARD_MOVE_PATH,
        move_payload("review", 0, 7),
    )
    .await;

    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "task_board_unavailable",
            "message": "Task board move worker failed"
        })
    );
}

#[derive(Clone)]
enum FakeMoveOutcome {
    Success(TaskBoardMutationResult),
    Error(FakeMoveError),
    Panic,
}

#[derive(Clone)]
enum FakeMoveError {
    Busy,
    InvalidFile(PathBuf, String),
    InvalidInput(String),
    RevisionConflict(TaskBoardDocument),
    TaskIdConflict,
    TaskNotFound,
    Unavailable(PathBuf, String),
}

struct FakeMoveStore {
    outcome: FakeMoveOutcome,
    calls: Arc<Mutex<Vec<TaskBoardMoveCommand>>>,
    observed_thread: Option<Arc<Mutex<Option<ThreadId>>>>,
}

impl FakeMoveStore {
    fn success(
        result: TaskBoardMutationResult,
        observed_thread: Option<Arc<Mutex<Option<ThreadId>>>>,
    ) -> Self {
        Self::with_outcome(FakeMoveOutcome::Success(result), observed_thread)
    }

    fn with_outcome(
        outcome: FakeMoveOutcome,
        observed_thread: Option<Arc<Mutex<Option<ThreadId>>>>,
    ) -> Self {
        Self {
            outcome,
            calls: Arc::new(Mutex::new(Vec::new())),
            observed_thread,
        }
    }

    fn calls(&self) -> Vec<TaskBoardMoveCommand> {
        self.calls.lock().unwrap().clone()
    }
}

impl TaskBoardStore for FakeMoveStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        panic!("T-008 move route must not call snapshot")
    }

    fn create_task(
        &self,
        _command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("T-008 move route must not call create_task")
    }

    fn attach_conversations(
        &self,
        _command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("T-008 move route must not call attach_conversations")
    }

    fn move_task(
        &self,
        command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.calls.lock().unwrap().push(command);
        if let Some(observed_thread) = &self.observed_thread {
            *observed_thread.lock().unwrap() = Some(std::thread::current().id());
        }
        match &self.outcome {
            FakeMoveOutcome::Success(result) => Ok(result.clone()),
            FakeMoveOutcome::Error(error) => Err(match error {
                FakeMoveError::Busy => TaskBoardStoreError::Busy,
                FakeMoveError::InvalidFile(path, message) => TaskBoardStoreError::InvalidFile {
                    path: path.clone(),
                    message: message.clone(),
                },
                FakeMoveError::InvalidInput(message) => TaskBoardStoreError::InvalidInput {
                    message: message.clone(),
                },
                FakeMoveError::RevisionConflict(current) => TaskBoardStoreError::RevisionConflict {
                    current: current.clone(),
                },
                FakeMoveError::TaskIdConflict => TaskBoardStoreError::TaskIdConflict,
                FakeMoveError::TaskNotFound => TaskBoardStoreError::TaskNotFound,
                FakeMoveError::Unavailable(path, message) => TaskBoardStoreError::Unavailable {
                    path: path.clone(),
                    message: message.clone(),
                },
            }),
            FakeMoveOutcome::Panic => panic!("private worker panic detail"),
        }
    }
}
