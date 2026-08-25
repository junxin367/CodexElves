use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codex_elves_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_elves_core::routes::task_board::{
    TASK_BOARD_ATTACH_CONVERSATIONS_PATH, TASK_BOARD_DETACH_CONVERSATIONS_PATH,
};
use codex_elves_core::routes::{
    BridgeContext, BridgeDataService, CoreRuntimeService, handle_bridge_request,
};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    TaskBoardAttachConversationsCommand, TaskBoardCatalogProject, TaskBoardCatalogSession,
    TaskBoardConversation, TaskBoardCreateCommand, TaskBoardDetachConversationsCommand,
    TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardSessionCatalog, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError, TaskBoardTask,
};
use serde_json::{Value, json};

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const SESSION_A: &str = "019c89c0-0000-7000-8000-000000000001";
const SESSION_B: &str = "019c89c0-0000-7000-8000-000000000002";
const PROJECT_CWD: &str = "E:\\code\\codexelves";

fn response_document(revision: u64) -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision,
        tasks: vec![TaskBoardTask {
            id: TASK_ID.to_string(),
            title: "完善任务看板".to_string(),
            project: TaskBoardProject {
                cwd: PROJECT_CWD.to_string(),
                label: "CodexElves".to_string(),
            },
            status: TaskBoardStatus::Executing,
            order: 0,
            conversations: vec![
                TaskBoardConversation {
                    session_id: SESSION_A.to_string(),
                    title: "已有会话".to_string(),
                    cwd: PROJECT_CWD.to_string(),
                    updated_at_ms: Some(10),
                },
                TaskBoardConversation {
                    session_id: SESSION_B.to_string(),
                    title: "目录权威标题".to_string(),
                    cwd: PROJECT_CWD.to_string(),
                    updated_at_ms: Some(20),
                },
            ],
            created_at_ms: 100,
            updated_at_ms: 200,
        }],
    }
}

fn catalog() -> TaskBoardSessionCatalog {
    TaskBoardSessionCatalog {
        projects: vec![TaskBoardCatalogProject {
            cwd: PROJECT_CWD.to_string(),
            label: "CodexElves".to_string(),
            session_count: 2,
        }],
        sessions: vec![TaskBoardCatalogSession {
            session_id: SESSION_B.to_string(),
            title: "目录权威标题".to_string(),
            cwd: "e:/CODE/CodexElves/".to_string(),
            updated_at_ms: Some(20),
        }],
        warnings: Vec::new(),
    }
}

fn context(store: Arc<FakeStore>, data: Arc<CatalogData>) -> BridgeContext {
    BridgeContext::core_with_data(
        Arc::new(CoreRuntimeService::new(0, StatusStore::default())),
        data,
    )
    .with_task_board_store(store)
}

#[tokio::test]
async fn attach_route_uses_authoritative_catalog_metadata_and_returns_snapshot() {
    let store = Arc::new(FakeStore::success(response_document(8)));
    let data = Arc::new(CatalogData::success(catalog()));

    let response = handle_bridge_request(
        context(store.clone(), data.clone()),
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        json!({
            "taskId": format!(" {TASK_ID} "),
            "expectedRevision": 7,
            "sessionIds": [SESSION_B]
        }),
    )
    .await;

    assert_eq!(response["status"], "ok");
    assert_eq!(response["revision"], 8);
    assert_eq!(data.calls.load(Ordering::SeqCst), 1);
    let commands = store.commands.lock().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].task_id, TASK_ID);
    assert_eq!(commands[0].expected_revision, 7);
    assert_eq!(
        commands[0].conversations,
        vec![TaskBoardConversation {
            session_id: SESSION_B.to_string(),
            title: "目录权威标题".to_string(),
            cwd: PROJECT_CWD.to_string(),
            updated_at_ms: Some(20),
        }]
    );
}

#[tokio::test]
async fn attach_route_rejects_temporary_duplicate_and_missing_sessions_before_store_mutation() {
    for session_ids in [
        json!([]),
        json!([SESSION_B, SESSION_B.to_ascii_uppercase()]),
        json!(["local:client-new-thread:temporary"]),
    ] {
        let store = Arc::new(FakeStore::success(response_document(8)));
        let data = Arc::new(CatalogData::success(catalog()));
        let response = handle_bridge_request(
            context(store.clone(), data.clone()),
            TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
            json!({
                "taskId": TASK_ID,
                "expectedRevision": 7,
                "sessionIds": session_ids
            }),
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.commands.lock().unwrap().is_empty());
    }

    let store = Arc::new(FakeStore::success(response_document(8)));
    let data = Arc::new(CatalogData::success(catalog()));
    let response = handle_bridge_request(
        context(store.clone(), data),
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7,
            "sessionIds": ["missing-session"]
        }),
    )
    .await;
    assert_eq!(response["code"], "session_not_found");
    assert!(store.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn attach_route_exposes_stable_project_mismatch_and_revision_conflict_codes() {
    let mismatch_store = Arc::new(FakeStore::error(TaskBoardStoreError::ProjectMismatch));
    let mismatch = handle_bridge_request(
        context(mismatch_store, Arc::new(CatalogData::success(catalog()))),
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7,
            "sessionIds": [SESSION_B]
        }),
    )
    .await;
    assert_eq!(mismatch["status"], "failed");
    assert_eq!(mismatch["code"], "project_mismatch");

    let conflict_store = Arc::new(FakeStore::error(TaskBoardStoreError::RevisionConflict {
        current: response_document(9),
    }));
    let conflict = handle_bridge_request(
        context(conflict_store, Arc::new(CatalogData::success(catalog()))),
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7,
            "sessionIds": [SESSION_B]
        }),
    )
    .await;
    assert_eq!(conflict["status"], "conflict");
    assert_eq!(conflict["code"], "revision_conflict");
    assert_eq!(conflict["revision"], 9);
}

#[tokio::test]
async fn detach_route_forwards_only_link_identity_without_reading_the_catalog() {
    let store = Arc::new(FakeStore::success(response_document(8)));
    let data = Arc::new(CatalogData::success(catalog()));

    let response = handle_bridge_request(
        context(store.clone(), data.clone()),
        TASK_BOARD_DETACH_CONVERSATIONS_PATH,
        json!({
            "taskId": format!(" {TASK_ID} "),
            "expectedRevision": 7,
            "sessionIds": [format!(" {SESSION_A} ")]
        }),
    )
    .await;

    assert_eq!(response["status"], "ok");
    assert_eq!(response["revision"], 8);
    assert_eq!(data.calls.load(Ordering::SeqCst), 0);
    let commands = store.detach_commands.lock().unwrap();
    assert_eq!(
        commands.as_slice(),
        &[TaskBoardDetachConversationsCommand {
            task_id: TASK_ID.to_string(),
            expected_revision: 7,
            session_ids: vec![SESSION_A.to_string()],
        }]
    );
}

#[tokio::test]
async fn detach_route_rejects_invalid_sets_and_exposes_revision_conflicts() {
    for session_ids in [
        json!([]),
        json!([SESSION_A, SESSION_A.to_ascii_uppercase()]),
        json!(["local:client-new-thread:temporary"]),
    ] {
        let store = Arc::new(FakeStore::success(response_document(8)));
        let response = handle_bridge_request(
            context(store.clone(), Arc::new(CatalogData::success(catalog()))),
            TASK_BOARD_DETACH_CONVERSATIONS_PATH,
            json!({
                "taskId": TASK_ID,
                "expectedRevision": 7,
                "sessionIds": session_ids
            }),
        )
        .await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert!(store.detach_commands.lock().unwrap().is_empty());
    }

    let store = Arc::new(FakeStore::error(TaskBoardStoreError::RevisionConflict {
        current: response_document(9),
    }));
    let response = handle_bridge_request(
        context(store, Arc::new(CatalogData::success(catalog()))),
        TASK_BOARD_DETACH_CONVERSATIONS_PATH,
        json!({
            "taskId": TASK_ID,
            "expectedRevision": 7,
            "sessionIds": [SESSION_A]
        }),
    )
    .await;
    assert_eq!(response["status"], "conflict");
    assert_eq!(response["code"], "revision_conflict");
    assert_eq!(response["revision"], 9);
}

enum StoreOutcome {
    Success(TaskBoardDocument),
    Error(Mutex<Option<TaskBoardStoreError>>),
}

struct FakeStore {
    outcome: StoreOutcome,
    commands: Mutex<Vec<TaskBoardAttachConversationsCommand>>,
    detach_commands: Mutex<Vec<TaskBoardDetachConversationsCommand>>,
}

impl FakeStore {
    fn success(document: TaskBoardDocument) -> Self {
        Self {
            outcome: StoreOutcome::Success(document),
            commands: Mutex::new(Vec::new()),
            detach_commands: Mutex::new(Vec::new()),
        }
    }

    fn error(error: TaskBoardStoreError) -> Self {
        Self {
            outcome: StoreOutcome::Error(Mutex::new(Some(error))),
            commands: Mutex::new(Vec::new()),
            detach_commands: Mutex::new(Vec::new()),
        }
    }
}

impl TaskBoardStore for FakeStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        Ok(TaskBoardDocument::empty())
    }

    fn create_task(
        &self,
        _command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("unexpected create")
    }

    fn move_task(
        &self,
        _command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("unexpected move")
    }

    fn attach_conversations(
        &self,
        command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.commands.lock().unwrap().push(command);
        match &self.outcome {
            StoreOutcome::Success(document) => Ok(TaskBoardMutationResult {
                document: document.clone(),
                changed: true,
                idempotent: false,
            }),
            StoreOutcome::Error(error) => Err(error.lock().unwrap().take().unwrap()),
        }
    }

    fn detach_conversations(
        &self,
        command: TaskBoardDetachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        self.detach_commands.lock().unwrap().push(command);
        match &self.outcome {
            StoreOutcome::Success(document) => Ok(TaskBoardMutationResult {
                document: document.clone(),
                changed: true,
                idempotent: false,
            }),
            StoreOutcome::Error(error) => Err(error.lock().unwrap().take().unwrap()),
        }
    }
}

struct CatalogData {
    catalog: TaskBoardSessionCatalog,
    calls: AtomicUsize,
}

impl CatalogData {
    fn success(catalog: TaskBoardSessionCatalog) -> Self {
        Self {
            catalog,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl BridgeDataService for CatalogData {
    async fn delete(&self, _session: SessionRef) -> anyhow::Result<DeleteResult> {
        anyhow::bail!("unused")
    }

    async fn export_markdown(&self, _session: SessionRef) -> anyhow::Result<ExportResult> {
        anyhow::bail!("unused")
    }

    async fn thread_usage_history(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused")
    }

    async fn thread_usage_summary(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused")
    }

    async fn find_archived_thread_by_title(
        &self,
        _title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        anyhow::bail!("unused")
    }

    async fn move_thread_workspace(
        &self,
        _session: SessionRef,
        _target_cwd: String,
    ) -> anyhow::Result<Value> {
        anyhow::bail!("unused")
    }

    async fn thread_sort_key(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused")
    }

    async fn thread_sort_keys(&self, _sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        anyhow::bail!("unused")
    }

    async fn task_board_session_catalog(&self) -> anyhow::Result<TaskBoardSessionCatalog> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.catalog.clone())
    }
}
