use std::collections::HashSet;
use uuid::Uuid;

use super::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardDetachConversationsCommand,
    TaskBoardMutationResult, TaskBoardStoreError, is_temporary_session_id, unix_timestamp_ms,
};

pub(super) fn detach_conversations(
    store: &FileTaskBoardStore,
    command: TaskBoardDetachConversationsCommand,
) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
    let command = normalize_command(command)?;
    store.with_exclusive_document(|mut current| {
        let task_index = current
            .tasks
            .iter()
            .position(|task| task.id == command.task_id)
            .ok_or(TaskBoardStoreError::TaskNotFound)?;
        let requested = command
            .session_ids
            .iter()
            .map(|session_id| session_id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let has_match = current.tasks[task_index]
            .conversations
            .iter()
            .any(|conversation| requested.contains(&conversation.session_id.to_ascii_lowercase()));

        if !has_match {
            return Ok(TaskBoardMutationResult {
                document: current,
                changed: false,
                idempotent: true,
            });
        }
        if current.revision != command.expected_revision {
            return Err(TaskBoardStoreError::RevisionConflict { current });
        }

        current.tasks[task_index]
            .conversations
            .retain(|conversation| {
                !requested.contains(&conversation.session_id.to_ascii_lowercase())
            });
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
    command: TaskBoardDetachConversationsCommand,
) -> Result<TaskBoardDetachConversationsCommand, TaskBoardStoreError> {
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
    if command.session_ids.is_empty() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "at least one session id is required".to_string(),
        });
    }

    let mut seen = HashSet::new();
    let mut session_ids = Vec::with_capacity(command.session_ids.len());
    for session_id in command.session_ids {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "session id must not be empty".to_string(),
            });
        }
        if is_temporary_session_id(session_id) {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "temporary session ids cannot be removed from a task".to_string(),
            });
        }
        if !seen.insert(session_id.to_ascii_lowercase()) {
            return Err(TaskBoardStoreError::InvalidInput {
                message: "session ids must be unique".to_string(),
            });
        }
        session_ids.push(session_id.to_string());
    }

    Ok(TaskBoardDetachConversationsCommand {
        task_id,
        expected_revision: command.expected_revision,
        session_ids,
    })
}
