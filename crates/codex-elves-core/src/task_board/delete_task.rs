use uuid::Uuid;

use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDeleteCommand,
    TaskBoardMutationResult, TaskBoardStoreError,
};

pub(super) fn delete_task(
    store: &FileTaskBoardStore,
    command: TaskBoardDeleteCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let Some(task_index) = current
            .tasks
            .iter()
            .position(|task| task.id == command.task_id)
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

        let deleted_status = current.tasks[task_index].status;
        current.tasks.remove(task_index);
        let mut status_task_indices = current
            .tasks
            .iter()
            .enumerate()
            .filter_map(|(index, task)| (task.status == deleted_status).then_some(index))
            .collect::<Vec<_>>();
        status_task_indices.sort_by_key(|index| current.tasks[*index].order);
        for (order, index) in status_task_indices.into_iter().enumerate() {
            current.tasks[index].order =
                u32::try_from(order).map_err(|_| TaskBoardStoreError::InvalidInput {
                    message: "task order exceeds the supported range".to_string(),
                })?;
        }
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
    command: TaskBoardDeleteCommand,
) -> Result<TaskBoardDeleteCommand, TaskBoardStoreError> {
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
    Ok(TaskBoardDeleteCommand {
        task_id,
        expected_revision: command.expected_revision,
    })
}
