use serde::{Deserialize, Serialize};

pub const TASK_BOARD_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardDocument {
    pub schema_version: u32,
    pub revision: u64,
    pub tasks: Vec<TaskBoardTask>,
}

impl TaskBoardDocument {
    pub fn empty() -> Self {
        Self {
            schema_version: TASK_BOARD_SCHEMA_VERSION,
            revision: 0,
            tasks: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskBoardStatus {
    New,
    Planning,
    Executing,
    Review,
    Done,
}

impl TaskBoardStatus {
    pub const ALL: [Self; 5] = [
        Self::New,
        Self::Planning,
        Self::Executing,
        Self::Review,
        Self::Done,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardTask {
    pub id: String,
    pub title: String,
    pub project: TaskBoardProject,
    pub status: TaskBoardStatus,
    pub order: u32,
    pub conversations: Vec<TaskBoardConversation>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardProject {
    pub cwd: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardConversation {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardCreateCommand {
    pub task_id: String,
    pub expected_revision: u64,
    pub title: String,
    pub project: TaskBoardProject,
    pub conversations: Vec<TaskBoardConversation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardAttachConversationsCommand {
    pub task_id: String,
    pub expected_revision: u64,
    pub conversations: Vec<TaskBoardConversation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardDetachConversationsCommand {
    pub task_id: String,
    pub expected_revision: u64,
    pub session_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardMoveCommand {
    pub task_id: String,
    pub to_status: TaskBoardStatus,
    pub target_index: u32,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardMutationResult {
    pub document: TaskBoardDocument,
    pub changed: bool,
    pub idempotent: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardSessionCatalog {
    pub projects: Vec<TaskBoardCatalogProject>,
    pub sessions: Vec<TaskBoardCatalogSession>,
    pub warnings: Vec<TaskBoardCatalogWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardCatalogProject {
    pub cwd: String,
    pub label: String,
    pub session_count: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardCatalogSession {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardCatalogWarning {
    pub code: TaskBoardCatalogWarningCode,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskBoardCatalogWarningCode {
    CodexDbReadFailed,
}
