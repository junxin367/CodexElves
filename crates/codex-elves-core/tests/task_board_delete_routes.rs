use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use codex_elves_core::routes::task_board::TASK_BOARD_DELETE_PATH;
use codex_elves_core::routes::{BridgeContext, CoreRuntimeService, handle_bridge_request};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    TaskBoardAttachConversationsCommand, TaskBoardCreateCommand, TaskBoardDeleteCommand,
    TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardStore,
    TaskBoardStoreError,
};
use serde_json::json;

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn context(store: Arc<dyn TaskBoardStore>) -> BridgeContext {
    BridgeContext::core(Arc::new(CoreRuntimeService::new(0, StatusStore::default())))
        .with_task_board_store(store)
}

#[tokio::test]
async fn delete_route_forwards_exact_command_and_returns_flattened_snapshot() {
    let document = TaskBoardDocument {
        schema_version: 1,
        revision: 8,
        boards: TaskBoardDocument::default_boards(),
        tasks: Vec::new(),
    };
    let store = Arc::new(FakeDeleteStore::success(document.clone()));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_DELETE_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7
        }),
    )
    .await;

    assert_eq!(
        store.calls(),
        vec![TaskBoardDeleteCommand {
            task_id: TASK_ID.to_string(),
            expected_revision: 7,
        }]
    );
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "boards": TaskBoardDocument::default_boards(),
            "tasks": []
        })
    );
}

#[tokio::test]
async fn delete_route_rejects_invalid_payloads_without_calling_the_store() {
    let cases = [
        json!({}),
        json!({"taskId": TASK_ID}),
        json!({"taskId": "not-a-uuid", "expectedRevision": 7}),
        json!({"taskId": TASK_ID, "expectedRevision": JS_MAX_SAFE_INTEGER + 1}),
        json!({"taskId": TASK_ID, "expectedRevision": 7, "extra": true}),
        json!([]),
    ];

    for payload in cases {
        let store = Arc::new(FakeDeleteStore::success(TaskBoardDocument::empty()));
        let response =
            handle_bridge_request(context(store.clone()), TASK_BOARD_DELETE_PATH, payload).await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.calls().is_empty());
    }
}

#[tokio::test]
async fn delete_route_exposes_latest_snapshot_on_revision_conflict() {
    let latest = TaskBoardDocument {
        schema_version: 1,
        revision: 12,
        boards: TaskBoardDocument::default_boards(),
        tasks: Vec::new(),
    };
    let store = Arc::new(FakeDeleteStore::error(
        TaskBoardStoreError::RevisionConflict {
            current: latest.clone(),
        },
    ));

    let response = handle_bridge_request(
        context(store),
        TASK_BOARD_DELETE_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7
        }),
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
            "boards": TaskBoardDocument::default_boards(),
            "tasks": []
        })
    );
}

#[tokio::test]
async fn delete_route_maps_stable_store_errors() {
    let cases = [
        (TaskBoardStoreError::Busy, "task_board_busy"),
        (
            TaskBoardStoreError::InvalidFile {
                path: PathBuf::from("task-board.json"),
                message: "bad json".to_string(),
            },
            "task_file_invalid",
        ),
        (
            TaskBoardStoreError::InvalidInput {
                message: "bad request".to_string(),
            },
            "invalid_input",
        ),
        (TaskBoardStoreError::TaskNotFound, "task_not_found"),
        (
            TaskBoardStoreError::Unavailable {
                path: PathBuf::from("task-board.json"),
                message: "offline".to_string(),
            },
            "task_board_unavailable",
        ),
    ];

    for (error, expected_code) in cases {
        let response = handle_bridge_request(
            context(Arc::new(FakeDeleteStore::error(error))),
            TASK_BOARD_DELETE_PATH,
            json!({
                "taskId": TASK_ID,
                "expectedRevision": 7
            }),
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], expected_code);
    }
}

enum DeleteOutcome {
    Success(TaskBoardDocument),
    Error(Mutex<Option<TaskBoardStoreError>>),
}

struct FakeDeleteStore {
    outcome: DeleteOutcome,
    calls: Mutex<Vec<TaskBoardDeleteCommand>>,
}

impl FakeDeleteStore {
    fn success(document: TaskBoardDocument) -> Self {
        Self {
            outcome: DeleteOutcome::Success(document),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn error(error: TaskBoardStoreError) -> Self {
        Self {
            outcome: DeleteOutcome::Error(Mutex::new(Some(error))),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<TaskBoardDeleteCommand> {
        self.calls.lock().unwrap().clone()
    }
}

impl TaskBoardStore for FakeDeleteStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        panic!("delete route must not call snapshot")
    }

    fn create_task(
        &self,
        _command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("delete route must not call create_task")
    }

    fn attach_conversations(
        &self,
        _command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("delete route must not call attach_conversations")
    }

    fn delete_task(
        &self,
        command: TaskBoardDeleteCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.calls.lock().unwrap().push(command);
        match &self.outcome {
            DeleteOutcome::Success(document) => Ok(TaskBoardMutationResult {
                document: document.clone(),
                changed: true,
                idempotent: false,
            }),
            DeleteOutcome::Error(error) => Err(error.lock().unwrap().take().unwrap()),
        }
    }

    fn move_task(
        &self,
        _command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("delete route must not call move_task")
    }
}
