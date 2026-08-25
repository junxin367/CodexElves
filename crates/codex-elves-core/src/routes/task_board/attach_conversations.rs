use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardAttachConversationsCommand, TaskBoardCatalogSession,
    TaskBoardConversation, TaskBoardSessionCatalog, TaskBoardStore, normalize_task_project_cwd,
};

use super::{BridgeDataService, failed, snapshot_success, store_error};

const SESSION_CATALOG_UNAVAILABLE_MESSAGE: &str =
    "Task board session catalog service is unavailable";
const SESSION_NOT_FOUND_MESSAGE: &str = "One or more task board sessions were not found";
const TASK_BOARD_UNAVAILABLE_MESSAGE: &str = "Task board storage is unavailable";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttachConversationsRequest {
    task_id: String,
    expected_revision: u64,
    session_ids: Vec<String>,
}

struct ValidatedAttachConversationsRequest {
    task_id: String,
    expected_revision: u64,
    session_ids: Vec<String>,
}

pub(super) async fn handle(
    store: Arc<dyn TaskBoardStore>,
    data: Arc<dyn BridgeDataService>,
    payload: Value,
) -> Value {
    let request = match serde_json::from_value::<AttachConversationsRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error.to_string()),
    };
    let request = match validate_request(request) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let catalog = match data.task_board_session_catalog().await {
        Ok(catalog) => catalog,
        Err(_) => {
            return failed(
                "session_catalog_unavailable",
                SESSION_CATALOG_UNAVAILABLE_MESSAGE,
            );
        }
    };
    let command = match build_command(request, catalog) {
        Ok(command) => command,
        Err(response) => return response,
    };

    match tokio::task::spawn_blocking(move || store.attach_conversations(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", TASK_BOARD_UNAVAILABLE_MESSAGE),
    }
}

fn validate_request(
    request: AttachConversationsRequest,
) -> Result<ValidatedAttachConversationsRequest, Value> {
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

    let mut seen_session_ids = HashSet::new();
    let mut session_ids = Vec::with_capacity(request.session_ids.len());
    for session_id in request.session_ids {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(failed("invalid_input", "Session id must not be empty"));
        }
        if is_temporary_session_id(session_id) {
            return Err(failed(
                "invalid_input",
                "Temporary session ids cannot be added to a task",
            ));
        }
        if !seen_session_ids.insert(session_id.to_ascii_lowercase()) {
            return Err(failed("invalid_input", "Session ids must be unique"));
        }
        session_ids.push(session_id.to_string());
    }

    Ok(ValidatedAttachConversationsRequest {
        task_id,
        expected_revision: request.expected_revision,
        session_ids,
    })
}

fn build_command(
    request: ValidatedAttachConversationsRequest,
    catalog: TaskBoardSessionCatalog,
) -> Result<TaskBoardAttachConversationsCommand, Value> {
    let mut session_index = HashMap::new();
    for session in catalog.sessions {
        let session_id = session.session_id.trim();
        if session_id.is_empty() || is_temporary_session_id(session_id) {
            continue;
        }
        session_index
            .entry(session_id.to_ascii_lowercase())
            .or_insert(session);
    }

    let mut conversations = Vec::with_capacity(request.session_ids.len());
    for requested_session_id in request.session_ids {
        let Some(session) = session_index.get(&requested_session_id.to_ascii_lowercase()) else {
            return Err(failed("session_not_found", SESSION_NOT_FOUND_MESSAGE));
        };
        conversations.push(authoritative_conversation(session)?);
    }

    Ok(TaskBoardAttachConversationsCommand {
        task_id: request.task_id,
        expected_revision: request.expected_revision,
        conversations,
    })
}

fn authoritative_conversation(
    session: &TaskBoardCatalogSession,
) -> Result<TaskBoardConversation, Value> {
    if session
        .updated_at_ms
        .is_some_and(|timestamp| timestamp > TASK_BOARD_MAX_SAFE_INTEGER)
    {
        return Err(failed(
            "session_catalog_unavailable",
            SESSION_CATALOG_UNAVAILABLE_MESSAGE,
        ));
    }
    let cwd = normalize_task_project_cwd(&session.cwd).map_err(|_| {
        failed(
            "session_catalog_unavailable",
            SESSION_CATALOG_UNAVAILABLE_MESSAGE,
        )
    })?;
    Ok(TaskBoardConversation {
        session_id: session.session_id.trim().to_string(),
        title: session.title.clone(),
        cwd,
        updated_at_ms: session.updated_at_ms,
    })
}

fn is_temporary_session_id(session_id: &str) -> bool {
    session_id.starts_with("new-thread:")
        || session_id.starts_with("client-new-thread:")
        || session_id.contains(":new-thread:")
        || session_id.contains(":client-new-thread:")
}
