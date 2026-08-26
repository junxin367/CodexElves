use std::sync::{Arc, Mutex};

use codex_elves_core::routes::task_board::{
    TASK_BOARD_CREATE_BOARD_PATH, TASK_BOARD_DELETE_BOARD_PATH, TASK_BOARD_MOVE_BOARD_PATH,
    TASK_BOARD_RENAME_BOARD_PATH,
};
use codex_elves_core::routes::{BridgeContext, CoreRuntimeService, handle_bridge_request};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    TaskBoardAttachConversationsCommand, TaskBoardColumn, TaskBoardCreateBoardCommand,
    TaskBoardCreateCommand, TaskBoardDeleteBoardCommand, TaskBoardDocument,
    TaskBoardMoveBoardCommand, TaskBoardMoveCommand, TaskBoardMutationResult,
    TaskBoardRenameBoardCommand, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
};
use serde_json::json;
use uuid::Uuid;

const BOARD_ID: &str = "44d5ad90-897a-4c28-a09d-88d05d5b64f6";
const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn context(store: Arc<dyn TaskBoardStore>) -> BridgeContext {
    BridgeContext::core(Arc::new(CoreRuntimeService::new(0, StatusStore::default())))
        .with_task_board_store(store)
}

fn custom_status() -> TaskBoardStatus {
    TaskBoardStatus::custom(Uuid::parse_str(BOARD_ID).unwrap())
}

fn document_with_custom_board(revision: u64) -> TaskBoardDocument {
    let mut document = TaskBoardDocument::empty();
    document.revision = revision;
    document.boards.push(TaskBoardColumn {
        id: custom_status(),
        label: "待发布".to_string(),
        color: "#fb7185".to_string(),
    });
    document
}

#[tokio::test]
async fn create_board_route_forwards_the_exact_command_and_returns_boards() {
    let document = document_with_custom_board(8);
    let store = Arc::new(FakeBoardStore::success(document.clone()));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_CREATE_BOARD_PATH,
        json!({
            "boardId": BOARD_ID,
            "expectedRevision": 7,
            "label": "待发布"
        }),
    )
    .await;

    assert_eq!(
        store.create_calls(),
        vec![TaskBoardCreateBoardCommand {
            board_id: custom_status(),
            expected_revision: 7,
            label: "待发布".to_string(),
        }]
    );
    assert!(store.delete_calls().is_empty());
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "boards": document.boards,
            "tasks": []
        })
    );
}

#[tokio::test]
async fn delete_board_route_forwards_known_board_ids_and_returns_the_new_snapshot() {
    let mut document = TaskBoardDocument::empty();
    document.revision = 8;
    document
        .boards
        .retain(|board| board.id != TaskBoardStatus::Planning);
    let store = Arc::new(FakeBoardStore::success(document.clone()));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_DELETE_BOARD_PATH,
        json!({
            "boardId": "planning",
            "expectedRevision": 7
        }),
    )
    .await;

    assert_eq!(
        store.delete_calls(),
        vec![TaskBoardDeleteBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 7,
        }]
    );
    assert!(store.create_calls().is_empty());
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "boards": document.boards,
            "tasks": []
        })
    );
}

#[tokio::test]
async fn rename_board_route_forwards_the_exact_command_and_returns_the_new_snapshot() {
    let mut document = document_with_custom_board(8);
    document.boards.last_mut().unwrap().label = "发布队列".to_string();
    let store = Arc::new(FakeBoardStore::success(document.clone()));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_RENAME_BOARD_PATH,
        json!({
            "boardId": BOARD_ID,
            "expectedRevision": 7,
            "label": "发布队列"
        }),
    )
    .await;

    assert_eq!(
        store.rename_calls(),
        vec![TaskBoardRenameBoardCommand {
            board_id: custom_status(),
            expected_revision: 7,
            label: "发布队列".to_string(),
        }]
    );
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "boards": document.boards,
            "tasks": []
        })
    );
}

#[tokio::test]
async fn move_board_route_forwards_the_exact_command_and_returns_the_new_snapshot() {
    let mut document = TaskBoardDocument::empty();
    document.revision = 8;
    let planning = document.boards.remove(0);
    document.boards.push(planning);
    let store = Arc::new(FakeBoardStore::success(document.clone()));

    let response = handle_bridge_request(
        context(store.clone()),
        TASK_BOARD_MOVE_BOARD_PATH,
        json!({
            "boardId": "planning",
            "targetIndex": 3,
            "expectedRevision": 7
        }),
    )
    .await;

    assert_eq!(
        store.move_board_calls(),
        vec![TaskBoardMoveBoardCommand {
            board_id: TaskBoardStatus::Planning,
            target_index: 3,
            expected_revision: 7,
        }]
    );
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 8,
            "boards": document.boards,
            "tasks": []
        })
    );
}

#[tokio::test]
async fn board_routes_reject_invalid_payloads_before_calling_the_store() {
    let create_cases = [
        json!({}),
        json!({"boardId": "planning", "expectedRevision": 7, "label": "待发布"}),
        json!({"boardId": "not-a-uuid", "expectedRevision": 7, "label": "待发布"}),
        json!({"boardId": BOARD_ID, "expectedRevision": JS_MAX_SAFE_INTEGER + 1, "label": "待发布"}),
        json!({"boardId": BOARD_ID, "expectedRevision": 7, "label": "待发布", "extra": true}),
        json!([]),
    ];
    for payload in create_cases {
        let store = Arc::new(FakeBoardStore::success(TaskBoardDocument::empty()));
        let response = handle_bridge_request(
            context(store.clone()),
            TASK_BOARD_CREATE_BOARD_PATH,
            payload,
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.create_calls().is_empty());
        assert!(store.delete_calls().is_empty());
    }

    let delete_cases = [
        json!({}),
        json!({"boardId": "new", "expectedRevision": 7}),
        json!({"boardId": "not-a-uuid", "expectedRevision": 7}),
        json!({"boardId": BOARD_ID, "expectedRevision": JS_MAX_SAFE_INTEGER + 1}),
        json!({"boardId": BOARD_ID, "expectedRevision": 7, "extra": true}),
        json!([]),
    ];
    for payload in delete_cases {
        let store = Arc::new(FakeBoardStore::success(TaskBoardDocument::empty()));
        let response = handle_bridge_request(
            context(store.clone()),
            TASK_BOARD_DELETE_BOARD_PATH,
            payload,
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.create_calls().is_empty());
        assert!(store.delete_calls().is_empty());
    }

    let rename_cases = [
        json!({}),
        json!({"boardId": "new", "expectedRevision": 7, "label": "收件箱"}),
        json!({"boardId": "not-a-uuid", "expectedRevision": 7, "label": "待发布"}),
        json!({"boardId": BOARD_ID, "expectedRevision": JS_MAX_SAFE_INTEGER + 1, "label": "待发布"}),
        json!({"boardId": BOARD_ID, "expectedRevision": 7, "label": "待发布", "extra": true}),
        json!([]),
    ];
    for payload in rename_cases {
        let store = Arc::new(FakeBoardStore::success(TaskBoardDocument::empty()));
        let response = handle_bridge_request(
            context(store.clone()),
            TASK_BOARD_RENAME_BOARD_PATH,
            payload,
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.rename_calls().is_empty());
    }

    let move_cases = [
        json!({}),
        json!({"boardId": "new", "targetIndex": 0, "expectedRevision": 7}),
        json!({"boardId": "not-a-uuid", "targetIndex": 0, "expectedRevision": 7}),
        json!({"boardId": BOARD_ID, "targetIndex": 0, "expectedRevision": JS_MAX_SAFE_INTEGER + 1}),
        json!({"boardId": BOARD_ID, "targetIndex": 0, "expectedRevision": 7, "extra": true}),
        json!([]),
    ];
    for payload in move_cases {
        let store = Arc::new(FakeBoardStore::success(TaskBoardDocument::empty()));
        let response =
            handle_bridge_request(context(store.clone()), TASK_BOARD_MOVE_BOARD_PATH, payload)
                .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.move_board_calls().is_empty());
    }
}

#[tokio::test]
async fn board_routes_include_boards_in_conflicts_and_map_board_errors() {
    let latest = document_with_custom_board(12);
    let conflict = handle_bridge_request(
        context(Arc::new(FakeBoardStore::error(
            TaskBoardStoreError::RevisionConflict {
                current: latest.clone(),
            },
        ))),
        TASK_BOARD_CREATE_BOARD_PATH,
        json!({
            "boardId": BOARD_ID,
            "expectedRevision": 7,
            "label": "待发布"
        }),
    )
    .await;
    assert_eq!(
        conflict,
        json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "Task board revision conflicts with the current snapshot",
            "schemaVersion": 1,
            "revision": 12,
            "boards": latest.boards,
            "tasks": []
        })
    );

    for (path, payload, error, expected_code) in [
        (
            TASK_BOARD_CREATE_BOARD_PATH,
            json!({"boardId": BOARD_ID, "expectedRevision": 7, "label": "待发布"}),
            TaskBoardStoreError::BoardIdConflict,
            "board_id_conflict",
        ),
        (
            TASK_BOARD_DELETE_BOARD_PATH,
            json!({"boardId": BOARD_ID, "expectedRevision": 7}),
            TaskBoardStoreError::BoardNotFound,
            "board_not_found",
        ),
    ] {
        let response = handle_bridge_request(
            context(Arc::new(FakeBoardStore::error(error))),
            path,
            payload,
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], expected_code);
    }
}

enum BoardOutcome {
    Success(TaskBoardDocument),
    Error(Mutex<Option<TaskBoardStoreError>>),
}

struct FakeBoardStore {
    outcome: BoardOutcome,
    create_calls: Mutex<Vec<TaskBoardCreateBoardCommand>>,
    delete_calls: Mutex<Vec<TaskBoardDeleteBoardCommand>>,
    rename_calls: Mutex<Vec<TaskBoardRenameBoardCommand>>,
    move_board_calls: Mutex<Vec<TaskBoardMoveBoardCommand>>,
}

impl FakeBoardStore {
    fn success(document: TaskBoardDocument) -> Self {
        Self {
            outcome: BoardOutcome::Success(document),
            create_calls: Mutex::new(Vec::new()),
            delete_calls: Mutex::new(Vec::new()),
            rename_calls: Mutex::new(Vec::new()),
            move_board_calls: Mutex::new(Vec::new()),
        }
    }

    fn error(error: TaskBoardStoreError) -> Self {
        Self {
            outcome: BoardOutcome::Error(Mutex::new(Some(error))),
            create_calls: Mutex::new(Vec::new()),
            delete_calls: Mutex::new(Vec::new()),
            rename_calls: Mutex::new(Vec::new()),
            move_board_calls: Mutex::new(Vec::new()),
        }
    }

    fn create_calls(&self) -> Vec<TaskBoardCreateBoardCommand> {
        self.create_calls.lock().unwrap().clone()
    }

    fn delete_calls(&self) -> Vec<TaskBoardDeleteBoardCommand> {
        self.delete_calls.lock().unwrap().clone()
    }

    fn rename_calls(&self) -> Vec<TaskBoardRenameBoardCommand> {
        self.rename_calls.lock().unwrap().clone()
    }

    fn move_board_calls(&self) -> Vec<TaskBoardMoveBoardCommand> {
        self.move_board_calls.lock().unwrap().clone()
    }

    fn outcome(&self) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        match &self.outcome {
            BoardOutcome::Success(document) => Ok(TaskBoardMutationResult {
                document: document.clone(),
                changed: true,
                idempotent: false,
            }),
            BoardOutcome::Error(error) => Err(error.lock().unwrap().take().unwrap()),
        }
    }
}

impl TaskBoardStore for FakeBoardStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        panic!("board mutation routes must not call snapshot")
    }

    fn create_task(
        &self,
        _command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("board mutation routes must not call create_task")
    }

    fn attach_conversations(
        &self,
        _command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("board mutation routes must not call attach_conversations")
    }

    fn create_board(
        &self,
        command: TaskBoardCreateBoardCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.create_calls.lock().unwrap().push(command);
        self.outcome()
    }

    fn delete_board(
        &self,
        command: TaskBoardDeleteBoardCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.delete_calls.lock().unwrap().push(command);
        self.outcome()
    }

    fn rename_board(
        &self,
        command: TaskBoardRenameBoardCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.rename_calls.lock().unwrap().push(command);
        self.outcome()
    }

    fn move_board(
        &self,
        command: TaskBoardMoveBoardCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.move_board_calls.lock().unwrap().push(command);
        self.outcome()
    }

    fn move_task(
        &self,
        _command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("board mutation routes must not call move_task")
    }
}
