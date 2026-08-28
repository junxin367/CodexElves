use uuid::Uuid;

use super::validation::normalize_task_title;
use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardMutationResult,
    TaskBoardRenameTaskCommand, TaskBoardStoreError, unix_timestamp_ms,
};

pub(super) fn rename_task(
    store: &FileTaskBoardStore,
    command: TaskBoardRenameTaskCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let task_index = current
            .tasks
            .iter()
            .position(|task| task.id == command.task_id)
            .ok_or(TaskBoardStoreError::TaskNotFound)?;
        if current.tasks[task_index].title == command.title {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        current.tasks[task_index].title = command.title;
        current.tasks[task_index].updated_at_ms = unix_timestamp_ms()?;
        current.revision =
            current
                .revision
                .checked_add(1)
                .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                    message: "task board revision cannot be incremented".to_string(),
                })?;

        Ok(TaskBoardMutationResult {
            document: current,
            changed: true,
            idempotent: false,
        })
    })
}

fn normalize_command(
    command: TaskBoardRenameTaskCommand,
) -> Result<TaskBoardRenameTaskCommand, TaskBoardStoreError> {
    let task_id = Uuid::parse_str(command.task_id.trim())
        .map(|task_id| task_id.hyphenated().to_string())
        .map_err(|_| TaskBoardStoreError::InvalidInput {
            message: "task id must be a UUID".to_string(),
        })?;
    if command.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "expected revision exceeds the JavaScript safe integer range".to_string(),
        });
    }
    let title = normalize_task_title(&command.title).map_err(|error| {
        TaskBoardStoreError::InvalidInput {
            message: error.to_string(),
        }
    })?;
    Ok(TaskBoardRenameTaskCommand {
        task_id,
        expected_revision: command.expected_revision,
        title,
    })
}
