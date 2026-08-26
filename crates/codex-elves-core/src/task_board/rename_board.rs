use super::validation::normalize_task_board_label;
use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardMutationResult,
    TaskBoardRenameBoardCommand, TaskBoardStoreError,
};

pub(super) fn rename_board(
    store: &FileTaskBoardStore,
    command: TaskBoardRenameBoardCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let board_index = current
            .boards
            .iter()
            .position(|board| board.id == command.board_id)
            .ok_or(TaskBoardStoreError::BoardNotFound)?;
        if current.boards[board_index].label == command.label {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }
        if current.boards.iter().enumerate().any(|(index, board)| {
            index != board_index && board.label.to_lowercase() == command.label.to_lowercase()
        }) {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "task board label already exists".to_string(),
            });
        }

        current.boards[board_index].label = command.label;
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
    command: TaskBoardRenameBoardCommand,
) -> Result<TaskBoardRenameBoardCommand, TaskBoardStoreError> {
    if command.board_id.is_unassigned() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "the unassigned column cannot be renamed".to_string(),
        });
    }
    if command.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "expected revision exceeds the JavaScript safe integer range".to_string(),
        });
    }
    let label = normalize_task_board_label(&command.label).map_err(|error| {
        TaskBoardStoreError::InvalidInput {
            message: error.to_string(),
        }
    })?;
    Ok(TaskBoardRenameBoardCommand { label, ..command })
}
