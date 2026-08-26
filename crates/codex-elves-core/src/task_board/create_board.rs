use super::validation::{MAX_TASK_BOARD_COUNT, normalize_task_board_label};
use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardColumn, TaskBoardCreateBoardCommand,
    TaskBoardMutationResult, TaskBoardStoreError,
};

const BOARD_COLORS: [&str; 8] = [
    "#60a5fa", "#c084fc", "#fbbf24", "#34d399", "#fb7185", "#22d3ee", "#a78bfa", "#f97316",
];

pub(super) fn create_board(
    store: &FileTaskBoardStore,
    command: TaskBoardCreateBoardCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        if let Some(existing) = current
            .boards
            .iter()
            .find(|board| board.id == command.board_id)
        {
            if existing.label == command.label {
                return Ok(TaskBoardMutationResult {
                    document: current,
                    changed: false,
                    idempotent: true,
                });
            }
            return Err(TaskBoardStoreError::BoardIdConflict);
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }
        if current.boards.len() >= MAX_TASK_BOARD_COUNT {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "task board contains too many managed boards".to_string(),
            });
        }
        if current
            .boards
            .iter()
            .any(|board| board.label.to_lowercase() == command.label.to_lowercase())
        {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "task board label already exists".to_string(),
            });
        }

        let color = BOARD_COLORS[current.boards.len() % BOARD_COLORS.len()].to_string();
        current.boards.push(TaskBoardColumn {
            id: command.board_id,
            label: command.label,
            color,
        });
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
    command: TaskBoardCreateBoardCommand,
) -> Result<TaskBoardCreateBoardCommand, TaskBoardStoreError> {
    if !command.board_id.is_custom() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "new task boards must use a UUID id".to_string(),
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
    Ok(TaskBoardCreateBoardCommand { label, ..command })
}
