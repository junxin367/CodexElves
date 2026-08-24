use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDocument, TaskBoardMoveCommand,
    TaskBoardMutationResult, TaskBoardStatus, TaskBoardStoreError,
};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(super) fn move_task(
    store: &FileTaskBoardStore,
    command: TaskBoardMoveCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let task_id = normalize_task_id(&command.task_id)?;

    store.with_exclusive_document(|mut current| {
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        let source_index = current
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or(TaskBoardStoreError::TaskNotFound)?;
        let source_status = current.tasks[source_index].status;
        let source_order = ordered_task_ids(&current, source_status);
        let original = current.clone();
        let mut moved = current.tasks.remove(source_index);
        let mut target_order = ordered_task_ids(&current, command.to_status);
        let target_len =
            u32::try_from(target_order.len()).map_err(|_| TaskBoardStoreError::InvalidInput {
                message: "target column contains too many tasks".to_string(),
            })?;
        if command.target_index > target_len {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "target index exceeds the target column length after removal".to_string(),
            });
        }

        let target_index = usize::try_from(command.target_index).map_err(|_| {
            TaskBoardStoreError::InvalidInput {
                message: "target index exceeds the supported range".to_string(),
            }
        })?;
        target_order.insert(target_index, moved.id.clone());

        if source_status == command.to_status && target_order == source_order {
            return Ok(TaskBoardMutationResult {
                document: original,
                changed: false,
                idempotent: false,
            });
        }

        moved.status = command.to_status;
        moved.updated_at_ms = unix_timestamp_ms()?;
        current.revision =
            current
                .revision
                .checked_add(1)
                .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                    message: "task board revision cannot be incremented".to_string(),
                })?;
        current.tasks.insert(source_index, moved);

        if source_status != command.to_status {
            let source_order = ordered_task_ids(&current, source_status);
            assign_orders(&mut current, source_status, &source_order)?;
        }
        assign_orders(&mut current, command.to_status, &target_order)?;

        Ok(TaskBoardMutationResult {
            document: current,
            changed: true,
            idempotent: false,
        })
    })
}

fn normalize_task_id(raw_task_id: &str) -> Result<String, TaskBoardStoreError> {
    Uuid::parse_str(raw_task_id.trim())
        .map(|task_id| task_id.hyphenated().to_string())
        .map_err(|_| TaskBoardStoreError::InvalidInput {
            message: "task id must be a UUID".to_string(),
        })
}

fn ordered_task_ids(document: &TaskBoardDocument, status: TaskBoardStatus) -> Vec<String> {
    let mut tasks = document
        .tasks
        .iter()
        .filter(|task| task.status == status)
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| task.order);
    tasks.into_iter().map(|task| task.id.clone()).collect()
}

fn assign_orders(
    document: &mut TaskBoardDocument,
    status: TaskBoardStatus,
    task_ids: &[String],
) -> Result<(), TaskBoardStoreError> {
    for (order, task_id) in task_ids.iter().enumerate() {
        let order = u32::try_from(order).map_err(|_| TaskBoardStoreError::InvalidInput {
            message: "task order exceeds the supported range".to_string(),
        })?;
        let task = document
            .tasks
            .iter_mut()
            .find(|task| task.id == *task_id)
            .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                message: "task disappeared during move".to_string(),
            })?;
        task.order = order;
        task.status = status;
    }
    Ok(())
}

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
