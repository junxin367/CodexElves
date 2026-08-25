use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::task_board::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardCatalogProject, TaskBoardCatalogSession,
    TaskBoardConversation, TaskBoardCreateCommand, TaskBoardProject, TaskBoardSessionCatalog,
    TaskBoardStore, is_temporary_session_id, normalize_task_project_cwd,
};

use super::{BridgeDataService, failed, snapshot_success, store_error};

const SESSION_CATALOG_UNAVAILABLE_MESSAGE: &str =
    "Task board session catalog service is unavailable";
const SESSION_NOT_FOUND_MESSAGE: &str = "One or more task board sessions were not found";
const PROJECT_MISMATCH_MESSAGE: &str = "Task board sessions must belong to the requested project";
const TASK_BOARD_UNAVAILABLE_MESSAGE: &str = "Task board storage is unavailable";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateRequest {
    task_id: String,
    expected_revision: u64,
    title: String,
    project: CreateProject,
    session_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateProject {
    cwd: String,
    label: String,
}

struct ValidatedCreateRequest {
    task_id: String,
    expected_revision: u64,
    title: String,
    project_cwd: String,
    session_ids: Vec<String>,
}

pub(super) async fn handle(
    store: Arc<dyn TaskBoardStore>,
    data: Arc<dyn BridgeDataService>,
    payload: Value,
) -> Value {
    let request = match serde_json::from_value::<CreateRequest>(payload) {
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

    match tokio::task::spawn_blocking(move || store.create_task(command)).await {
        Ok(Ok(result)) => snapshot_success(result.document),
        Ok(Err(error)) => store_error(error),
        Err(_) => failed("task_board_unavailable", TASK_BOARD_UNAVAILABLE_MESSAGE),
    }
}

fn validate_request(request: CreateRequest) -> Result<ValidatedCreateRequest, Value> {
    let task_id = request.task_id.trim();
    let task_id = Uuid::parse_str(task_id)
        .map(|task_id| task_id.hyphenated().to_string())
        .map_err(|_| failed("invalid_input", "Task id must be a UUID"))?;
    if request.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(failed(
            "invalid_input",
            "Expected revision exceeds the JavaScript safe integer range",
        ));
    }

    let title = request.title.trim();
    let title_chars = title.chars().count();
    if title_chars == 0 || title_chars > 120 {
        return Err(failed(
            "invalid_input",
            "Task title must contain between 1 and 120 Unicode characters",
        ));
    }

    let project_cwd = normalize_task_project_cwd(&request.project.cwd)
        .map_err(|error| failed("invalid_input", error.to_string()))?;
    let _ = request.project.label;
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

    Ok(ValidatedCreateRequest {
        task_id,
        expected_revision: request.expected_revision,
        title: title.to_string(),
        project_cwd,
        session_ids,
    })
}

fn build_command(
    request: ValidatedCreateRequest,
    catalog: TaskBoardSessionCatalog,
) -> Result<TaskBoardCreateCommand, Value> {
    let project_label = authoritative_project_label(&catalog.projects, &request.project_cwd);
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
    let mut fallback_cwd = None;
    for requested_session_id in request.session_ids {
        let Some(session) = session_index.get(&requested_session_id.to_ascii_lowercase()) else {
            return Err(failed("session_not_found", SESSION_NOT_FOUND_MESSAGE));
        };
        let conversation = authoritative_conversation(session, &request.project_cwd)?;
        fallback_cwd.get_or_insert_with(|| session.cwd.clone());
        conversations.push(conversation);
    }
    let project_label = project_label.unwrap_or_else(|| {
        fallback_project_label(fallback_cwd.as_deref().unwrap_or(&request.project_cwd))
    });

    Ok(TaskBoardCreateCommand {
        task_id: request.task_id,
        expected_revision: request.expected_revision,
        title: request.title,
        project: TaskBoardProject {
            cwd: request.project_cwd,
            label: project_label,
        },
        conversations,
    })
}

fn authoritative_conversation(
    session: &TaskBoardCatalogSession,
    project_cwd: &str,
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
    let session_cwd = normalize_task_project_cwd(&session.cwd).map_err(|_| {
        failed(
            "session_catalog_unavailable",
            SESSION_CATALOG_UNAVAILABLE_MESSAGE,
        )
    })?;
    if session_cwd != project_cwd {
        return Err(failed("project_mismatch", PROJECT_MISMATCH_MESSAGE));
    }
    Ok(TaskBoardConversation {
        session_id: session.session_id.trim().to_string(),
        title: session.title.clone(),
        cwd: session_cwd,
        updated_at_ms: session.updated_at_ms,
    })
}

fn authoritative_project_label(
    projects: &[TaskBoardCatalogProject],
    project_cwd: &str,
) -> Option<String> {
    projects.iter().find_map(|project| {
        let normalized_cwd = normalize_task_project_cwd(&project.cwd).ok()?;
        let label = project.label.trim();
        (normalized_cwd == project_cwd && !label.is_empty()).then(|| label.to_string())
    })
}

fn fallback_project_label(authoritative_cwd: &str) -> String {
    let trimmed = authoritative_cwd
        .trim()
        .trim_end_matches(|character| character == '\\' || character == '/');
    trimmed
        .rsplit(|character| character == '\\' || character == '/')
        .find(|component| !component.is_empty())
        .filter(|component| !component.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| authoritative_cwd.trim().to_string())
}
