use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDeleteBoardCommand, TaskBoardStatus, TaskBoardStore,
};

use super::{failed, snapshot_success, store_error};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteBoardRequest {
    board_id: TaskBoardStatus,
    expected_revision: u64,
}

pub(super) async fn handle(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    let request = match serde_json::from_value::<DeleteBoardRequest>(payload) {
        Ok(request) => request,
        Err(_) => return invalid_request(),
    };
    if request.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER || request.board_id.is_unassigned() {
        return invalid_request();
    }
    let command = TaskBoardDeleteBoardCommand {
        board_id: request.board_id,
        expected_revision: request.expected_revision,
    };

    match tokio::task::spawn_blocking(move || store.delete_board(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", "Task board delete worker failed"),
    }
}

fn invalid_request() -> Value {
    failed("invalid_input", "Task board delete request is invalid")
}
