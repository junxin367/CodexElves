use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::task_board::{TaskBoardDocument, TaskBoardStore, TaskBoardStoreError};

use super::BridgeDataService;

mod attach_conversations;
mod catalog;
mod create;
mod create_board;
mod delete_board;
mod delete_task;
mod detach_conversations;
mod move_board;
mod move_task;
mod rename_board;
mod snapshot;

pub const TASK_BOARD_SNAPSHOT_PATH: &str = "/task-board/snapshot";
pub const TASK_BOARD_OPEN_WINDOW_PATH: &str = "/task-board/open-window";
pub const TASK_BOARD_SESSION_CATALOG_PATH: &str = "/task-board/session-catalog";
pub const TASK_BOARD_CREATE_PATH: &str = "/task-board/task-create";
pub const TASK_BOARD_DELETE_PATH: &str = "/task-board/task-delete";
pub const TASK_BOARD_CREATE_BOARD_PATH: &str = "/task-board/board-create";
pub const TASK_BOARD_DELETE_BOARD_PATH: &str = "/task-board/board-delete";
pub const TASK_BOARD_RENAME_BOARD_PATH: &str = "/task-board/board-rename";
pub const TASK_BOARD_MOVE_BOARD_PATH: &str = "/task-board/board-move";
pub const TASK_BOARD_ATTACH_CONVERSATIONS_PATH: &str = "/task-board/task-conversations-attach";
pub const TASK_BOARD_DETACH_CONVERSATIONS_PATH: &str = "/task-board/task-conversations-detach";
pub const TASK_BOARD_MOVE_PATH: &str = "/task-board/task-move";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyRequest {}

pub(super) async fn handle_snapshot(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    snapshot::handle(store, payload).await
}

pub(super) async fn handle_catalog(data: Arc<dyn BridgeDataService>, payload: Value) -> Value {
    catalog::handle(data, payload).await
}

pub(super) async fn handle_create(
    store: Arc<dyn TaskBoardStore>,
    data: Arc<dyn BridgeDataService>,
    payload: Value,
) -> Value {
    create::handle(store, data, payload).await
}

pub(super) async fn handle_attach_conversations(
    store: Arc<dyn TaskBoardStore>,
    data: Arc<dyn BridgeDataService>,
    payload: Value,
) -> Value {
    attach_conversations::handle(store, data, payload).await
}

pub(super) async fn handle_detach_conversations(
    store: Arc<dyn TaskBoardStore>,
    payload: Value,
) -> Value {
    detach_conversations::handle(store, payload).await
}

pub(super) async fn handle_delete(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    delete_task::handle(store, payload).await
}

pub(super) async fn handle_create_board(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    create_board::handle(store, payload).await
}

pub(super) async fn handle_delete_board(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    delete_board::handle(store, payload).await
}

pub(super) async fn handle_rename_board(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    rename_board::handle(store, payload).await
}

pub(super) async fn handle_move_board(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    move_board::handle(store, payload).await
}

pub(super) async fn handle_move(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    move_task::handle(store, payload).await
}

fn parse_empty_request(payload: Value) -> Result<(), Value> {
    if !matches!(&payload, Value::Object(object) if object.is_empty()) {
        return Err(failed(
            "invalid_input",
            "Task board read request must be an empty object",
        ));
    }
    serde_json::from_value::<EmptyRequest>(payload)
        .map(|_| ())
        .map_err(|error| failed("invalid_input", error.to_string()))
}

fn snapshot_success(document: TaskBoardDocument) -> Value {
    json!({
        "status": "ok",
        "schemaVersion": document.schema_version,
        "revision": document.revision,
        "boards": document.boards,
        "tasks": document.tasks
    })
}

fn store_error(error: TaskBoardStoreError) -> Value {
    match error {
        TaskBoardStoreError::Busy => failed("task_board_busy", "Task board storage is busy"),
        TaskBoardStoreError::InvalidFile { path, message } => failed_with_path(
            "task_file_invalid",
            format!("Task board file is invalid: {message}"),
            &path,
        ),
        TaskBoardStoreError::InvalidInput { message } => failed("invalid_input", message),
        TaskBoardStoreError::SessionNotFound { .. } => failed(
            "session_not_found",
            "One or more task board sessions were not found",
        ),
        TaskBoardStoreError::RevisionConflict { current } => json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "Task board revision conflicts with the current snapshot",
            "schemaVersion": current.schema_version,
            "revision": current.revision,
            "boards": current.boards,
            "tasks": current.tasks
        }),
        TaskBoardStoreError::TaskIdConflict => failed(
            "task_id_conflict",
            "Task id conflicts with an existing task",
        ),
        TaskBoardStoreError::BoardIdConflict => failed(
            "board_id_conflict",
            "Task board id conflicts with an existing board",
        ),
        TaskBoardStoreError::BoardNotFound => failed("board_not_found", "Task board was not found"),
        TaskBoardStoreError::ProjectMismatch => failed(
            "project_mismatch",
            "Task board conversations must belong to the task project",
        ),
        TaskBoardStoreError::TaskNotFound => failed("task_not_found", "Task was not found"),
        TaskBoardStoreError::Unavailable { .. } => task_board_unavailable(),
    }
}

fn task_board_unavailable() -> Value {
    failed(
        "task_board_unavailable",
        "Task board storage is unavailable",
    )
}

fn failed(code: &str, message: impl Into<String>) -> Value {
    json!({
        "status": "failed",
        "code": code,
        "message": message.into()
    })
}

fn failed_with_path(code: &str, message: impl Into<String>, path: &Path) -> Value {
    json!({
        "status": "failed",
        "code": code,
        "message": message.into(),
        "path": path.to_string_lossy()
    })
}
