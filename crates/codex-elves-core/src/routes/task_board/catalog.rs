use std::sync::Arc;

use serde_json::{Value, json};

use super::{BridgeDataService, failed, parse_empty_request};

const SESSION_CATALOG_UNAVAILABLE_MESSAGE: &str =
    "Task board session catalog service is unavailable";

pub(super) async fn handle(data: Arc<dyn BridgeDataService>, payload: Value) -> Value {
    if let Err(response) = parse_empty_request(payload) {
        return response;
    }

    match data.task_board_session_catalog().await {
        Ok(catalog) => json!({
            "status": "ok",
            "projects": catalog.projects,
            "sessions": catalog.sessions,
            "warnings": catalog.warnings
        }),
        Err(_) => failed(
            "session_catalog_unavailable",
            SESSION_CATALOG_UNAVAILABLE_MESSAGE,
        ),
    }
}
