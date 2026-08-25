use super::{
    FileTaskBoardStore, TASK_BOARD_SCHEMA_VERSION, TaskBoardCreateCommand, TaskBoardDocument,
    TaskBoardMutationResult, TaskBoardStatus, TaskBoardStoreError, TaskBoardTask,
    unix_timestamp_ms, validate_task_board_document,
};
use std::collections::HashSet;

pub(super) fn create_task(
    store: &FileTaskBoardStore,
    command: TaskBoardCreateCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let normalized = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        if let Some(existing) = current
            .tasks
            .iter()
            .find(|task| task.id == normalized.task.id)
        {
            if same_semantic_identity(existing, &normalized.task) {
                return Ok(TaskBoardMutationResult {
                    document: current,
                    changed: false,
                    idempotent: true,
                });
            }
            return Err(TaskBoardStoreError::TaskIdConflict);
        }
        if current.revision != normalized.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        let order = current
            .tasks
            .iter()
            .filter(|task| task.status == TaskBoardStatus::New)
            .count()
            .try_into()
            .map_err(|_| TaskBoardStoreError::InvalidInput {
                message: "new task order exceeds the supported range".to_string(),
            })?;
        let timestamp_ms = unix_timestamp_ms()?;
        let mut task = normalized.task;
        task.order = order;
        task.created_at_ms = timestamp_ms;
        task.updated_at_ms = timestamp_ms;
        current.revision =
            current
                .revision
                .checked_add(1)
                .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                    message: "task board revision cannot be incremented".to_string(),
                })?;
        current.tasks.push(task);

        Ok(TaskBoardMutationResult {
            document: current,
            changed: true,
            idempotent: false,
        })
    })
}

fn same_semantic_identity(existing: &TaskBoardTask, candidate: &TaskBoardTask) -> bool {
    existing.title == candidate.title
        && existing.project.cwd == candidate.project.cwd
        && session_identity(&existing.conversations) == session_identity(&candidate.conversations)
}

fn session_identity(conversations: &[super::TaskBoardConversation]) -> HashSet<String> {
    conversations
        .iter()
        .map(|conversation| conversation.session_id.to_ascii_lowercase())
        .collect()
}

struct NormalizedCreateCommand {
    expected_revision: u64,
    task: TaskBoardTask,
}

fn normalize_command(
    command: TaskBoardCreateCommand,
) -> Result<NormalizedCreateCommand, TaskBoardStoreError> {
    if command.conversations.is_empty() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "at least one conversation is required".to_string(),
        });
    }
    let expected_revision = command.expected_revision;
    let mut document = TaskBoardDocument {
        schema_version: TASK_BOARD_SCHEMA_VERSION,
        revision: expected_revision,
        tasks: vec![TaskBoardTask {
            id: command.task_id,
            title: command.title,
            project: command.project,
            status: TaskBoardStatus::New,
            order: 0,
            conversations: command.conversations,
            created_at_ms: 0,
            updated_at_ms: 0,
        }],
    };
    validate_task_board_document(&mut document).map_err(|error| {
        TaskBoardStoreError::InvalidInput {
            message: error.to_string(),
        }
    })?;
    Ok(NormalizedCreateCommand {
        expected_revision,
        task: document.tasks.remove(0),
    })
}
