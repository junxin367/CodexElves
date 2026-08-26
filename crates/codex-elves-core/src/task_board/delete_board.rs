use std::collections::HashMap;

use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDeleteBoardCommand,
    TaskBoardMutationResult, TaskBoardStatus, TaskBoardStoreError, unix_timestamp_ms,
};

pub(super) fn delete_board(
    store: &FileTaskBoardStore,
    command: TaskBoardDeleteBoardCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let Some(board_index) = current
            .boards
            .iter()
            .position(|board| board.id == command.board_id)
        else {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        };
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        let mut unassigned = current
            .tasks
            .iter()
            .filter(|task| task.status == TaskBoardStatus::New)
            .map(|task| (task.order, task.id.clone()))
            .collect::<Vec<_>>();
        unassigned.sort_by_key(|(order, _)| *order);
        let mut reassigned = current
            .tasks
            .iter()
            .filter(|task| task.status == command.board_id)
            .map(|task| (task.order, task.id.clone()))
            .collect::<Vec<_>>();
        reassigned.sort_by_key(|(order, _)| *order);

        let timestamp_ms = (!reassigned.is_empty())
            .then(unix_timestamp_ms)
            .transpose()?;
        let ordered_ids = unassigned
            .into_iter()
            .chain(reassigned.into_iter())
            .map(|(_, task_id)| task_id)
            .collect::<Vec<_>>();
        let orders = ordered_ids
            .iter()
            .enumerate()
            .map(|(order, task_id)| {
                u32::try_from(order)
                    .map(|order| (task_id.as_str(), order))
                    .map_err(|_| TaskBoardStoreError::InvalidInput {
                        message: "task order exceeds the supported range".to_string(),
                    })
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        for task in &mut current.tasks {
            let Some(order) = orders.get(task.id.as_str()).copied() else {
                continue;
            };
            if task.status == command.board_id {
                task.status = TaskBoardStatus::New;
                if let Some(timestamp_ms) = timestamp_ms {
                    task.updated_at_ms = timestamp_ms;
                }
            }
            task.order = order;
        }

        current.boards.remove(board_index);
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
    command: TaskBoardDeleteBoardCommand,
) -> Result<TaskBoardDeleteBoardCommand, TaskBoardStoreError> {
    if command.board_id.is_unassigned() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "the unassigned column cannot be deleted".to_string(),
        });
    }
    if command.expected_revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "expected revision exceeds the JavaScript safe integer range".to_string(),
        });
    }
    Ok(command)
}
