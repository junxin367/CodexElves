use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::task_board::{TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardRenameTaskCommand, TaskBoardStore};

use super::{failed, snapshot_success, store_error};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenameTaskRequest {
    task_id: String,
    expected_revision: u64,
    title: String,
}

pub(super) async fn handle(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    let request = match serde_json::from_value::<RenameTaskRequest>(payload) {
        Ok(request) => request,
        Err(_) => return invalid_request(),
    };
    if request.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER
        || Uuid::parse_str(request.task_id.trim()).is_err()
    {
        return invalid_request();
    }
    let command = TaskBoardRenameTaskCommand {
        task_id: request.task_id,
        expected_revision: request.expected_revision,
        title: request.title,
    };

    match tokio::task::spawn_blocking(move || store.rename_task(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", "Task rename worker failed"),
    }
}

fn invalid_request() -> Value {
    failed("invalid_input", "Task rename request is invalid")
}
