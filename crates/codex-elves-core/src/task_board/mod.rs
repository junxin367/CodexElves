mod attach_conversations;
mod create;
mod detach_conversations;
mod model;
mod move_task;
mod store;
mod validation;

use std::time::{SystemTime, UNIX_EPOCH};

pub use model::{
    TASK_BOARD_SCHEMA_VERSION, TaskBoardAttachConversationsCommand, TaskBoardCatalogProject,
    TaskBoardCatalogSession, TaskBoardCatalogWarning, TaskBoardCatalogWarningCode,
    TaskBoardConversation, TaskBoardCreateCommand, TaskBoardDetachConversationsCommand,
    TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardSessionCatalog, TaskBoardStatus, TaskBoardTask,
};
pub use store::{FileTaskBoardStore, TaskBoardStore, TaskBoardStoreError};
pub(crate) use validation::is_temporary_session_id;
pub use validation::{
    TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardValidationError, normalize_task_project_cwd,
    parse_task_board_document, task_board_timestamp_from_bridge_i64, validate_task_board_document,
};

fn unix_timestamp_ms() -> Result<u64, TaskBoardStoreError> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| TaskBoardStoreError::InvalidInput {
            message: format!("system clock is before Unix epoch: {error}"),
        })?
        .as_millis();
    let timestamp_ms =
        u64::try_from(timestamp_ms).map_err(|_| TaskBoardStoreError::InvalidInput {
            message: "system timestamp exceeds the supported range".to_string(),
        })?;
    if timestamp_ms > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "system timestamp exceeds the JavaScript safe integer range".to_string(),
        });
    }
    Ok(timestamp_ms)
}
