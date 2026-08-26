use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardMoveBoardCommand,
    TaskBoardMutationResult, TaskBoardStoreError,
};

pub(super) fn move_board(
    store: &FileTaskBoardStore,
    command: TaskBoardMoveBoardCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let source_index = current
            .boards
            .iter()
            .position(|board| board.id == command.board_id)
            .ok_or(TaskBoardStoreError::BoardNotFound)?;
        let target_len = current.boards.len().saturating_sub(1);
        let target_index = usize::try_from(command.target_index).map_err(|_| {
            TaskBoardStoreError::InvalidInput {
                message: "target index exceeds the supported range".to_string(),
            }
        })?;
        if target_index > target_len {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "target index exceeds the managed board length after removal".to_string(),
            });
        }
        if source_index == target_index {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        let board = current.boards.remove(source_index);
        current.boards.insert(target_index, board);
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
    command: TaskBoardMoveBoardCommand,
) -> Result<TaskBoardMoveBoardCommand, TaskBoardStoreError> {
    if command.board_id.is_unassigned() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "the unassigned column cannot be moved".to_string(),
        });
    }
    if command.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "expected revision exceeds the JavaScript safe integer range".to_string(),
        });
    }
    Ok(command)
}
