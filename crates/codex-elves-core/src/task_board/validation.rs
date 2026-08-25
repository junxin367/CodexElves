use super::{TASK_BOARD_SCHEMA_VERSION, TaskBoardDocument, TaskBoardStatus};
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

const MAX_TITLE_CHARS: usize = 120;
pub const TASK_BOARD_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const TEMPORARY_NEW_THREAD_SEGMENT: &str = "new-thread:";
const TEMPORARY_CLIENT_NEW_THREAD_SEGMENT: &str = "client-new-thread:";

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TaskBoardValidationError {
    #[error("task board JSON is invalid: {message}")]
    InvalidJson { message: String },
    #[error("unsupported task board schema version {found}")]
    UnsupportedSchema { found: u32 },
    #[error("task board revision exceeds the JavaScript safe integer range")]
    RevisionOutOfRange,
    #[error("task id is not a UUID: {task_id}")]
    InvalidTaskId { task_id: String },
    #[error("task id is duplicated: {task_id}")]
    DuplicateTaskId { task_id: String },
    #[error("task title must contain between 1 and 120 Unicode characters")]
    InvalidTitle,
    #[error("task {task_id} must contain at least one conversation")]
    MissingConversations { task_id: String },
    #[error("task {task_id} contains an empty session id")]
    EmptySessionId { task_id: String },
    #[error("task {task_id} contains a temporary session id")]
    TemporarySessionId { task_id: String },
    #[error("task {task_id} contains duplicate session id {session_id}")]
    DuplicateSessionId { task_id: String, session_id: String },
    #[error("task {task_id} contains conversations from another project")]
    ProjectMismatch { task_id: String },
    #[error("task order is not continuous for {status:?}: expected {expected}, found {found}")]
    InvalidOrder {
        status: TaskBoardStatus,
        expected: u32,
        found: u32,
    },
    #[error("task board timestamp exceeds the JavaScript safe integer range")]
    TimestampOutOfRange,
    #[error("project cwd is empty")]
    EmptyCwd,
    #[error("project cwd is not a valid lexical path: {message}")]
    InvalidCwd { message: String },
}

pub fn parse_task_board_document(
    bytes: &[u8],
) -> Result<TaskBoardDocument, TaskBoardValidationError> {
    let mut document = serde_json::from_slice::<TaskBoardDocument>(bytes).map_err(|error| {
        TaskBoardValidationError::InvalidJson {
            message: error.to_string(),
        }
    })?;
    validate_task_board_document(&mut document)?;
    Ok(document)
}

pub fn validate_task_board_document(
    document: &mut TaskBoardDocument,
) -> Result<(), TaskBoardValidationError> {
    if document.schema_version != TASK_BOARD_SCHEMA_VERSION {
        return Err(TaskBoardValidationError::UnsupportedSchema {
            found: document.schema_version,
        });
    }
    if document.revision > TASK_BOARD_MAX_SAFE_INTEGER {
        return Err(TaskBoardValidationError::RevisionOutOfRange);
    }

    let mut task_ids = HashSet::new();
    for task in &mut document.tasks {
        let raw_task_id = task.id.trim();
        let parsed_task_id =
            Uuid::parse_str(raw_task_id).map_err(|_| TaskBoardValidationError::InvalidTaskId {
                task_id: raw_task_id.to_string(),
            })?;
        task.id = parsed_task_id.hyphenated().to_string();
        if !task_ids.insert(parsed_task_id) {
            return Err(TaskBoardValidationError::DuplicateTaskId {
                task_id: task.id.clone(),
            });
        }

        task.title = task.title.trim().to_string();
        let title_chars = task.title.chars().count();
        if title_chars == 0 || title_chars > MAX_TITLE_CHARS {
            return Err(TaskBoardValidationError::InvalidTitle);
        }
        if task.conversations.is_empty() {
            return Err(TaskBoardValidationError::MissingConversations {
                task_id: task.id.clone(),
            });
        }
        if task.created_at_ms > TASK_BOARD_MAX_SAFE_INTEGER
            || task.updated_at_ms > TASK_BOARD_MAX_SAFE_INTEGER
        {
            return Err(TaskBoardValidationError::TimestampOutOfRange);
        }

        task.project.cwd = normalize_task_project_cwd(&task.project.cwd)?;
        let mut session_ids = HashSet::new();
        for conversation in &mut task.conversations {
            conversation.session_id = conversation.session_id.trim().to_string();
            if conversation.session_id.is_empty() {
                return Err(TaskBoardValidationError::EmptySessionId {
                    task_id: task.id.clone(),
                });
            }
            if is_temporary_session_id(&conversation.session_id) {
                return Err(TaskBoardValidationError::TemporarySessionId {
                    task_id: task.id.clone(),
                });
            }
            let session_identity = conversation.session_id.to_ascii_lowercase();
            if !session_ids.insert(session_identity) {
                return Err(TaskBoardValidationError::DuplicateSessionId {
                    task_id: task.id.clone(),
                    session_id: conversation.session_id.clone(),
                });
            }
            if conversation
                .updated_at_ms
                .is_some_and(|timestamp| timestamp > TASK_BOARD_MAX_SAFE_INTEGER)
            {
                return Err(TaskBoardValidationError::TimestampOutOfRange);
            }
            conversation.cwd = normalize_task_project_cwd(&conversation.cwd)?;
            if conversation.cwd != task.project.cwd {
                return Err(TaskBoardValidationError::ProjectMismatch {
                    task_id: task.id.clone(),
                });
            }
        }
    }

    for status in TaskBoardStatus::ALL {
        let mut orders = document
            .tasks
            .iter()
            .filter(|task| task.status == status)
            .map(|task| task.order)
            .collect::<Vec<_>>();
        orders.sort_unstable();
        for (expected, found) in orders.into_iter().enumerate() {
            let expected = expected as u32;
            if found != expected {
                return Err(TaskBoardValidationError::InvalidOrder {
                    status,
                    expected,
                    found,
                });
            }
        }
    }

    Ok(())
}

/// Converts nullable signed timestamps from data providers before W2 Bridge DTO construction.
///
/// Bridge routes should use this seam instead of casting provider values directly so JSON output
/// remains nullable, nonnegative, and safe for JavaScript integer transport.
pub fn task_board_timestamp_from_bridge_i64(
    value: Option<i64>,
) -> Result<Option<u64>, TaskBoardValidationError> {
    value
        .map(|timestamp| {
            let timestamp = u64::try_from(timestamp)
                .map_err(|_| TaskBoardValidationError::TimestampOutOfRange)?;
            if timestamp > TASK_BOARD_MAX_SAFE_INTEGER {
                return Err(TaskBoardValidationError::TimestampOutOfRange);
            }
            Ok(timestamp)
        })
        .transpose()
}

fn is_temporary_session_id(session_id: &str) -> bool {
    session_id.starts_with(TEMPORARY_NEW_THREAD_SEGMENT)
        || session_id.starts_with(TEMPORARY_CLIENT_NEW_THREAD_SEGMENT)
        || session_id.contains(":new-thread:")
        || session_id.contains(":client-new-thread:")
}

pub fn normalize_task_project_cwd(raw: &str) -> Result<String, TaskBoardValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(TaskBoardValidationError::EmptyCwd);
    }

    if looks_like_windows_path(trimmed) {
        normalize_windows_path(trimmed)
    } else {
        normalize_unix_path(trimmed)
    }
}

fn looks_like_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with(r"\\")
        || path.starts_with("//")
        || (!path.starts_with('/') && path.contains('\\'))
}

fn normalize_windows_path(path: &str) -> Result<String, TaskBoardValidationError> {
    let path = strip_supported_windows_extended_path_prefix(path.replace('/', r"\"));
    if path.starts_with(r"\\") {
        return normalize_unc_path(&path);
    }

    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_uppercase();
        let rest = &path[2..];
        let rooted = rest.starts_with('\\');
        let components = normalize_components(rest.split('\\'), rooted, 0)
            .into_iter()
            .map(|component| component.to_lowercase())
            .collect::<Vec<_>>();
        if rooted {
            if components.is_empty() {
                return Ok(format!("{drive}:\\"));
            }
            return Ok(format!("{drive}:\\{}", components.join(r"\")));
        }
        if components.is_empty() {
            return Ok(format!("{drive}:"));
        }
        return Ok(format!("{drive}:{}", components.join(r"\")));
    }

    let rooted = path.starts_with('\\');
    let components = normalize_components(path.split('\\'), rooted, 0)
        .into_iter()
        .map(|component| component.to_lowercase())
        .collect::<Vec<_>>();
    if rooted {
        if components.is_empty() {
            Ok(r"\".to_string())
        } else {
            Ok(format!(r"\{}", components.join(r"\")))
        }
    } else if components.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(components.join(r"\"))
    }
}

fn strip_supported_windows_extended_path_prefix(path: String) -> String {
    if !path.starts_with(r"\\?\") {
        return path;
    }

    let extended_path = &path[4..];
    if extended_path
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"))
    {
        return format!(r"\\{}", &extended_path[4..]);
    }

    let bytes = extended_path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return extended_path.to_string();
    }

    path
}

fn normalize_unc_path(path: &str) -> Result<String, TaskBoardValidationError> {
    let raw_components = path
        .trim_start_matches('\\')
        .split('\\')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if raw_components.len() < 2
        || matches!(raw_components[0], "." | "..")
        || matches!(raw_components[1], "." | "..")
    {
        return Err(TaskBoardValidationError::InvalidCwd {
            message: "UNC paths require a server and share".to_string(),
        });
    }

    let mut components = vec![
        raw_components[0].to_lowercase(),
        raw_components[1].to_lowercase(),
    ];
    for component in &raw_components[2..] {
        match *component {
            "." => {}
            ".." if components.len() > 2 => {
                components.pop();
            }
            ".." => {}
            value => components.push(value.to_lowercase()),
        }
    }
    Ok(format!(r"\\{}", components.join(r"\")))
}

fn normalize_unix_path(path: &str) -> Result<String, TaskBoardValidationError> {
    let path = path.replace('\\', "/");
    let rooted = path.starts_with('/');
    let components = normalize_components(path.split('/'), rooted, 0);
    if rooted {
        if components.is_empty() {
            Ok("/".to_string())
        } else {
            Ok(format!("/{}", components.join("/")))
        }
    } else if components.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(components.join("/"))
    }
}

fn normalize_components<'a>(
    components: impl IntoIterator<Item = &'a str>,
    rooted: bool,
    floor: usize,
) -> Vec<&'a str> {
    let mut normalized = Vec::new();
    for component in components {
        match component {
            "" | "." => {}
            ".." if normalized.len() > floor && normalized.last() != Some(&"..") => {
                normalized.pop();
            }
            ".." if !rooted => normalized.push(component),
            ".." => {}
            _ => normalized.push(component),
        }
    }
    normalized
}
