use std::collections::HashSet;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDetachConversationsCommand, TaskBoardStore,
    is_temporary_session_id,
};

use super::{failed, snapshot_success, store_error};

const TASK_BOARD_UNAVAILABLE_MESSAGE: &str = "Task board storage is unavailable";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetachConversationsRequest {
    task_id: String,
    expected_revision: u64,
    session_ids: Vec<String>,
}

pub(super) async fn handle(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    let request = match serde_json::from_value::<DetachConversationsRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error.to_string()),
    };
    let command = match validate_request(request) {
        Ok(command) => command,
        Err(response) => return response,
    };

    match tokio::task::spawn_blocking(move || store.detach_conversations(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", TASK_BOARD_UNAVAILABLE_MESSAGE),
    }
}

fn validate_request(
    request: DetachConversationsRequest,
) -> Result<TaskBoardDetachConversationsCommand, Value> {
    let task_id = Uuid::parse_str(request.task_id.trim())
        .map(|task_id| task_id.hyphenated().to_string())
        .map_err(|_| failed("invalid_input", "Task id must be a UUID"))?;
    if request.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(failed(
            "invalid_input",
            "Expected revision exceeds the JavaScript safe integer range",
        ));
    }
    if request.session_ids.is_empty() {
        return Err(failed(
            "invalid_input",
            "At least one permanent session id is required",
        ));
    }

    let mut seen = HashSet::new();
    let mut session_ids = Vec::with_capacity(request.session_ids.len());
    for session_id in request.session_ids {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(failed("invalid_input", "Session id must not be empty"));
        }
        if is_temporary_session_id(session_id) {
            return Err(failed(
                "invalid_input",
                "Temporary session ids cannot be removed from a task",
            ));
        }
        if !seen.insert(session_id.to_ascii_lowercase()) {
            return Err(failed("invalid_input", "Session ids must be unique"));
        }
        session_ids.push(session_id.to_string());
    }

    Ok(TaskBoardDetachConversationsCommand {
        task_id,
        expected_revision: request.expected_revision,
        session_ids,
    })
}
