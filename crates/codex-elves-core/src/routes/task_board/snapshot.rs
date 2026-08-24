use std::sync::Arc;

use serde_json::Value;

use crate::task_board::TaskBoardStore;

use super::{parse_empty_request, snapshot_success, store_error, task_board_unavailable};

pub(super) async fn handle(store: Arc<dyn TaskBoardStore>, payload: Value) -> Value {
    if let Err(response) = parse_empty_request(payload) {
        return response;
    }

    match tokio::task::spawn_blocking(move || store.snapshot()).await {
        Ok(Ok(document)) => snapshot_success(document),
        Ok(Err(error)) => store_error(error),
        Err(_) => task_board_unavailable(),
    }
}
