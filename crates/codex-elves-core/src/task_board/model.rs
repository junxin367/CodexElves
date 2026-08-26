use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

pub const TASK_BOARD_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardDocument {
    pub schema_version: u32,
    pub revision: u64,
    #[serde(default = "TaskBoardDocument::default_boards")]
    pub boards: Vec<TaskBoardColumn>,
    pub tasks: Vec<TaskBoardTask>,
}

impl TaskBoardDocument {
    pub fn empty() -> Self {
        Self {
            schema_version: TASK_BOARD_SCHEMA_VERSION,
            revision: 0,
            boards: Self::default_boards(),
            tasks: Vec::new(),
        }
    }

    pub fn default_boards() -> Vec<TaskBoardColumn> {
        vec![
            TaskBoardColumn {
                id: TaskBoardStatus::Planning,
                label: "规划".to_string(),
                color: "#60a5fa".to_string(),
            },
            TaskBoardColumn {
                id: TaskBoardStatus::Executing,
                label: "执行".to_string(),
                color: "#c084fc".to_string(),
            },
            TaskBoardColumn {
                id: TaskBoardStatus::Review,
                label: "验收".to_string(),
                color: "#fbbf24".to_string(),
            },
            TaskBoardColumn {
                id: TaskBoardStatus::Done,
                label: "完成".to_string(),
                color: "#34d399".to_string(),
            },
        ]
    }

    pub fn contains_status(&self, status: TaskBoardStatus) -> bool {
        status == TaskBoardStatus::New || self.boards.iter().any(|board| board.id == status)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskBoardStatus {
    New,
    Planning,
    Executing,
    Review,
    Done,
    Custom(Uuid),
}

impl TaskBoardStatus {
    pub const ALL: [Self; 5] = [
        Self::New,
        Self::Planning,
        Self::Executing,
        Self::Review,
        Self::Done,
    ];

    pub fn custom(id: Uuid) -> Self {
        Self::Custom(id)
    }

    pub fn is_custom(self) -> bool {
        matches!(self, Self::Custom(_))
    }

    pub fn is_unassigned(self) -> bool {
        self == Self::New
    }

    pub fn persisted_id(self) -> String {
        match self {
            Self::New => "new".to_string(),
            Self::Planning => "planning".to_string(),
            Self::Executing => "executing".to_string(),
            Self::Review => "review".to_string(),
            Self::Done => "done".to_string(),
            Self::Custom(id) => id.hyphenated().to_string(),
        }
    }
}

impl Serialize for TaskBoardStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.persisted_id())
    }
}

impl<'de> Deserialize<'de> for TaskBoardStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim() {
            "new" => Ok(Self::New),
            "planning" => Ok(Self::Planning),
            "executing" => Ok(Self::Executing),
            "review" => Ok(Self::Review),
            "done" => Ok(Self::Done),
            custom => Uuid::parse_str(custom)
                .map(Self::Custom)
                .map_err(|_| D::Error::custom("task board status must be a known id or UUID")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBoardColumn {
    pub id: TaskBoardStatus,
    pub label: String,
    pub color: String,
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
pub struct TaskBoardDeleteCommand {
    pub task_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardCreateBoardCommand {
    pub board_id: TaskBoardStatus,
    pub expected_revision: u64,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardDeleteBoardCommand {
    pub board_id: TaskBoardStatus,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardRenameBoardCommand {
    pub board_id: TaskBoardStatus,
    pub expected_revision: u64,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskBoardMoveBoardCommand {
    pub board_id: TaskBoardStatus,
    pub target_index: u32,
    pub expected_revision: u64,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_aliases: Vec<String>,
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
