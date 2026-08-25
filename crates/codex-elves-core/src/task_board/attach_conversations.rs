use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardAttachConversationsCommand,
    TaskBoardMutationResult, TaskBoardStoreError, normalize_task_project_cwd,
    validate_task_board_document,
};

pub(super) fn attach_conversations(
    store: &FileTaskBoardStore,
    command: TaskBoardAttachConversationsCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let task_index = current
            .tasks
            .iter()
            .position(|task| task.id == command.task_id)
            .ok_or(TaskBoardStoreError::TaskNotFound)?;
        let existing_session_ids = current.tasks[task_index]
            .conversations
            .iter()
            .map(|conversation| conversation.session_id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let missing = command
            .conversations
            .into_iter()
            .filter(|conversation| {
                !existing_session_ids.contains(&conversation.session_id.to_ascii_lowercase())
            })
            .collect::<Vec<_>>();

        if missing.is_empty() {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        let project_cwd = current.tasks[task_index].project.cwd.clone();
        if missing
            .iter()
            .any(|conversation| conversation.cwd != project_cwd)
        {
            return Err(TaskBoardStoreError::ProjectMismatch);
        }

        current.tasks[task_index].conversations.extend(missing);
        current.tasks[task_index].updated_at_ms = unix_timestamp_ms()?;
        current.revision =
            current
                .revision
                .checked_add(1)
                .ok_or_else(|| TaskBoardStoreError::InvalidInput {
                    message: "task board revision cannot be incremented".to_string(),
                })?;
        validate_task_board_document(&mut current).map_err(|error| {
            TaskBoardStoreError::InvalidInput {
                message: error.to_string(),
            }
        })?;

        Ok(TaskBoardMutationResult {
            document: current,
            changed: true,
            idempotent: false,
        })
    })
}

fn normalize_command(
    command: TaskBoardAttachConversationsCommand,
) -> Result<TaskBoardAttachConversationsCommand, TaskBoardStoreError> {
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
    if command.conversations.is_empty() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "at least one conversation is required".to_string(),
        });
    }

    let mut seen_session_ids = HashSet::new();
    let mut conversations = Vec::with_capacity(command.conversations.len());
    for mut conversation in command.conversations {
        conversation.session_id = conversation.session_id.trim().to_string();
        if conversation.session_id.is_empty() {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "session id must not be empty".to_string(),
            });
        }
        if is_temporary_session_id(&conversation.session_id) {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "temporary session ids cannot be added to a task".to_string(),
            });
        }
        if !seen_session_ids.insert(conversation.session_id.to_ascii_lowercase()) {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "session ids must be unique".to_string(),
            });
        }
        if conversation
            .updated_at_ms
            .is_some_and(|timestamp| timestamp > TASK_BOARD_MAX_SAFE_INTEGER)
        {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "conversation timestamp exceeds the JavaScript safe integer range"
                    .to_string(),
            });
        }
        conversation.cwd = normalize_task_project_cwd(&conversation.cwd).map_err(|error| {
            TaskBoardStoreError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        conversations.push(conversation);
    }

    Ok(TaskBoardAttachConversationsCommand {
        task_id,
        expected_revision: command.expected_revision,
        conversations,
    })
}

fn is_temporary_session_id(session_id: &str) -> bool {
    session_id.starts_with("new-thread:")
        || session_id.starts_with("client-new-thread:")
        || session_id.contains(":new-thread:")
        || session_id.contains(":client-new-thread:")
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
