use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardMoveCommand, TaskBoardStatus, TaskBoardStore,
};

use super::{failed, snapshot_success, store_error};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MoveRequest {
    task_id: String,
    to_status: TaskBoardStatus,
    target_index: u32,
    expected_revision: u64,
}

pub(super) async fn handle(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    let command = match parse_command(payload) {
        Ok(command) => command,
        Err(response) => return response,
    };

    match tokio::task::spawn_blocking(move || store.move_task(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", "Task board move worker failed"),
    }
}

fn parse_command(payload: Value) -> Result<TaskBoardMoveCommand, Value> {
    let request = serde_json::from_value::<MoveRequest>(payload).map_err(|_| invalid_request())?;
    let task_id = Uuid::parse_str(request.task_id.trim())
        .map_err(|_| invalid_request())?
        .hyphenated()
        .to_string();
    if request.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(invalid_request());
    }

    Ok(TaskBoardMoveCommand {
        task_id,
        to_status: request.to_status,
        target_index: request.target_index,
        expected_revision: request.expected_revision,
    })
}

fn invalid_request() -> Value {
    failed("invalid_input", "Task board move request is invalid")
}
