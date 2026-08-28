use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use async_trait::async_trait;
use codex_elves_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_elves_core::routes::task_board::{
    TASK_BOARD_ATTACH_CONVERSATIONS_PATH, TASK_BOARD_CREATE_BOARD_PATH, TASK_BOARD_CREATE_PATH,
    TASK_BOARD_DELETE_BOARD_PATH, TASK_BOARD_DELETE_PATH, TASK_BOARD_DETACH_CONVERSATIONS_PATH,
    TASK_BOARD_MOVE_BOARD_PATH, TASK_BOARD_MOVE_PATH, TASK_BOARD_RENAME_BOARD_PATH,
    TASK_BOARD_RENAME_TASK_PATH, TASK_BOARD_SESSION_CATALOG_PATH, TASK_BOARD_SNAPSHOT_PATH,
};
use codex_elves_core::routes::{
    BridgeContext, BridgeDataService, CoreRuntimeService, CoreSettingsService,
    handle_bridge_request,
};
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    FileTaskBoardStore, TaskBoardAttachConversationsCommand, TaskBoardCatalogProject,
    TaskBoardCatalogSession, TaskBoardCatalogWarning, TaskBoardCatalogWarningCode,
    TaskBoardConversation, TaskBoardCreateCommand, TaskBoardDocument, TaskBoardMoveCommand,
    TaskBoardMutationResult, TaskBoardProject, TaskBoardSessionCatalog, TaskBoardStatus,
    TaskBoardStore, TaskBoardStoreError, TaskBoardTask,
};
use fs2::FileExt;
use serde_json::{Value, json};

fn runtime() -> Arc<CoreRuntimeService> {
    Arc::new(CoreRuntimeService::new(0, StatusStore::default()))
}

fn sample_document() -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        boards: TaskBoardDocument::default_boards(),
        tasks: vec![TaskBoardTask {
            id: "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e".to_string(),
            title: "完善任务看板".to_string(),
            project: TaskBoardProject {
                cwd: "E:\\code\\codexelves".to_string(),
                label: "CodexElves".to_string(),
            },
            status: TaskBoardStatus::Executing,
            order: 0,
            conversations: vec![TaskBoardConversation {
                session_id: "019c89c0-0000-7000-8000-000000000001".to_string(),
                title: "设计任务看板".to_string(),
                cwd: "E:\\code\\codexelves".to_string(),
                updated_at_ms: Some(1_787_544_000_000),
            }],
            created_at_ms: 1_787_544_000_000,
            updated_at_ms: 1_787_544_000_000,
        }],
    }
}

fn sample_catalog() -> TaskBoardSessionCatalog {
    TaskBoardSessionCatalog {
        projects: vec![TaskBoardCatalogProject {
            cwd: "E:\\private\\project".to_string(),
            label: "Private Project".to_string(),
            session_count: 1,
        }],
        sessions: vec![TaskBoardCatalogSession {
            session_id: "private-session-id".to_string(),
            title: "private-session-title".to_string(),
            cwd: "E:\\private\\project".to_string(),
            updated_at_ms: Some(1_787_544_000_000),
            session_aliases: Vec::new(),
        }],
        warnings: vec![TaskBoardCatalogWarning {
            code: TaskBoardCatalogWarningCode::CodexDbReadFailed,
            count: 2,
        }],
    }
}

fn context(store: Arc<dyn TaskBoardStore>, data: Arc<dyn BridgeDataService>) -> BridgeContext {
    BridgeContext::core_with_data(runtime(), data).with_task_board_store(store)
}

#[tokio::test]
async fn twelve_task_board_route_constants_dispatch_through_the_existing_bridge_match() {
    assert_eq!(TASK_BOARD_SNAPSHOT_PATH, "/task-board/snapshot");
    assert_eq!(
        TASK_BOARD_SESSION_CATALOG_PATH,
        "/task-board/session-catalog"
    );
    assert_eq!(TASK_BOARD_CREATE_PATH, "/task-board/task-create");
    assert_eq!(TASK_BOARD_DELETE_PATH, "/task-board/task-delete");
    assert_eq!(TASK_BOARD_RENAME_TASK_PATH, "/task-board/task-rename");
    assert_eq!(TASK_BOARD_CREATE_BOARD_PATH, "/task-board/board-create");
    assert_eq!(TASK_BOARD_DELETE_BOARD_PATH, "/task-board/board-delete");
    assert_eq!(TASK_BOARD_RENAME_BOARD_PATH, "/task-board/board-rename");
    assert_eq!(TASK_BOARD_MOVE_BOARD_PATH, "/task-board/board-move");
    assert_eq!(
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        "/task-board/task-conversations-attach"
    );
    assert_eq!(
        TASK_BOARD_DETACH_CONVERSATIONS_PATH,
        "/task-board/task-conversations-detach"
    );
    assert_eq!(TASK_BOARD_MOVE_PATH, "/task-board/task-move");

    let ctx = context(
        Arc::new(FakeStore::document(TaskBoardDocument::empty())),
        Arc::new(CatalogData::success(TaskBoardSessionCatalog::default())),
    );

    assert_eq!(
        handle_bridge_request(ctx.clone(), TASK_BOARD_SNAPSHOT_PATH, json!({})).await["status"],
        "ok"
    );
    assert_eq!(
        handle_bridge_request(ctx.clone(), TASK_BOARD_SESSION_CATALOG_PATH, json!({})).await["status"],
        "ok"
    );
    for path in [
        TASK_BOARD_CREATE_PATH,
        TASK_BOARD_DELETE_PATH,
        TASK_BOARD_RENAME_TASK_PATH,
        TASK_BOARD_CREATE_BOARD_PATH,
        TASK_BOARD_DELETE_BOARD_PATH,
        TASK_BOARD_RENAME_BOARD_PATH,
        TASK_BOARD_MOVE_BOARD_PATH,
        TASK_BOARD_ATTACH_CONVERSATIONS_PATH,
        TASK_BOARD_DETACH_CONVERSATIONS_PATH,
        TASK_BOARD_MOVE_PATH,
    ] {
        let response = handle_bridge_request(ctx.clone(), path, json!({})).await;
        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "invalid_input");
        assert_ne!(response["message"], "Unknown bridge path");
    }
}

#[tokio::test]
async fn existing_constructor_and_default_catalog_implementations_remain_compatible() {
    let ctx = BridgeContext::new(
        Arc::new(CoreSettingsService::default()),
        runtime(),
        Arc::new(LegacyData),
    )
    .with_task_board_store(Arc::new(FakeStore::document(TaskBoardDocument::empty())));

    let legacy = handle_bridge_request(ctx, TASK_BOARD_SESSION_CATALOG_PATH, json!({})).await;
    assert_eq!(
        legacy,
        json!({
            "status": "failed",
            "code": "session_catalog_unavailable",
            "message": "Task board session catalog service is unavailable"
        })
    );

    let unavailable = handle_bridge_request(
        BridgeContext::core(runtime())
            .with_task_board_store(Arc::new(FakeStore::document(TaskBoardDocument::empty()))),
        TASK_BOARD_SESSION_CATALOG_PATH,
        json!({}),
    )
    .await;
    assert_eq!(unavailable["status"], "failed");
    assert_eq!(unavailable["code"], "session_catalog_unavailable");
    assert!(
        unavailable["message"]
            .as_str()
            .unwrap()
            .contains("unavailable")
    );
}

#[tokio::test]
async fn read_routes_accept_only_an_exact_empty_object() {
    let ctx = context(
        Arc::new(FakeStore::document(TaskBoardDocument::empty())),
        Arc::new(CatalogData::success(TaskBoardSessionCatalog::default())),
    );

    for path in [TASK_BOARD_SNAPSHOT_PATH, TASK_BOARD_SESSION_CATALOG_PATH] {
        for payload in [json!({"unexpected": true}), json!(null), json!([])] {
            let response = handle_bridge_request(ctx.clone(), path, payload).await;
            assert_eq!(response["status"], "failed", "{path}");
            assert_eq!(response["code"], "invalid_input", "{path}");
            assert!(
                response["message"]
                    .as_str()
                    .is_some_and(|message| !message.is_empty()),
                "{path}"
            );
            assert_eq!(
                response.as_object().unwrap().keys().collect::<Vec<_>>(),
                vec!["status", "code", "message"]
            );
        }
    }
}

#[tokio::test]
async fn missing_task_file_returns_the_exact_empty_snapshot_shape() {
    let temp = tempfile::tempdir().unwrap();
    let store = FileTaskBoardStore::new(
        temp.path().join("task-board.json"),
        temp.path().join("task-board.lock"),
    );
    let ctx = context(
        Arc::new(store),
        Arc::new(CatalogData::failure("catalog must not affect snapshot")),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 0,
            "boards": TaskBoardDocument::default_boards(),
            "tasks": []
        })
    );
}

#[tokio::test]
async fn valid_task_file_returns_the_exact_flattened_camel_case_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    std::fs::write(
        &document_path,
        serde_json::to_vec_pretty(&sample_document()).unwrap(),
    )
    .unwrap();
    let store = FileTaskBoardStore::new(document_path, temp.path().join("task-board.lock"));
    let ctx = context(
        Arc::new(store),
        Arc::new(CatalogData::failure("catalog must not affect snapshot")),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    assert_eq!(
        response,
        json!({
            "status": "ok",
            "schemaVersion": 1,
            "revision": 7,
            "boards": TaskBoardDocument::default_boards(),
            "tasks": [{
                "id": "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e",
                "title": "完善任务看板",
                "project": {
                    "cwd": "E:\\code\\codexelves",
                    "label": "CodexElves"
                },
                "status": "executing",
                "order": 0,
                "conversations": [{
                    "sessionId": "019c89c0-0000-7000-8000-000000000001",
                    "title": "设计任务看板",
                    "cwd": "E:\\code\\codexelves",
                    "updatedAtMs": 1_787_544_000_000_u64
                }],
                "createdAtMs": 1_787_544_000_000_u64,
                "updatedAtMs": 1_787_544_000_000_u64
            }]
        })
    );
    assert!(response.get("schema_version").is_none());
}

#[tokio::test]
async fn busy_snapshot_returns_the_stable_busy_error_without_a_path() {
    let temp = tempfile::tempdir().unwrap();
    let lock_path = temp.path().join("task-board.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock_file.lock_exclusive().unwrap();
    let store = FileTaskBoardStore::new(temp.path().join("task-board.json"), lock_path)
        .with_lock_timing(Duration::from_millis(20), Duration::from_millis(2));
    let ctx = context(Arc::new(store), Arc::new(LegacyData));

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    assert_eq!(response["status"], "failed");
    assert_eq!(response["code"], "task_board_busy");
    assert!(response.get("path").is_none());
    lock_file.unlock().unwrap();
}

#[tokio::test]
async fn storage_unavailable_snapshot_is_fixed_and_does_not_leak_storage_details() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let path = PathBuf::from("E:\\private\\task-board.lock");
    let path_text = path.to_string_lossy().to_string();
    let private_message = concat!(
        "dbPath=C:\\private\\codex.sqlite ",
        "rolloutPath=E:\\secret\\rollout.json ",
        "body=private-storage-body"
    );
    let ctx = context(
        Arc::new(FakeStore::unavailable(path, private_message)),
        Arc::new(LegacyData),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "task_board_unavailable",
            "message": "Task board storage is unavailable"
        })
    );
    let response_text = serde_json::to_string(&response).unwrap();
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    for private_text in [
        private_message,
        path_text.as_str(),
        "dbPath",
        "rolloutPath",
        "private-storage-body",
    ] {
        assert!(!response_text.contains(private_text));
        assert!(!diagnostics.contains(private_text));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn snapshot_runs_the_store_on_a_blocking_worker() {
    let observed_thread = Arc::new(Mutex::new(None));
    let caller_thread = std::thread::current().id();
    let ctx = context(
        Arc::new(
            FakeStore::document(TaskBoardDocument::empty())
                .with_observed_thread(observed_thread.clone()),
        ),
        Arc::new(LegacyData),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    assert_eq!(response["status"], "ok");
    assert_ne!(*observed_thread.lock().unwrap(), Some(caller_thread));
}

#[tokio::test]
async fn snapshot_join_failure_is_fixed_and_does_not_leak_panic_details() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let panic_message = concat!(
        "dbPath=C:\\private\\snapshot.sqlite ",
        "rolloutPath=E:\\secret\\snapshot-rollout.json ",
        "body=private-snapshot-body"
    );
    let ctx = context(
        Arc::new(FakeStore::panic(panic_message)),
        Arc::new(LegacyData),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "failed",
            "code": "task_board_unavailable",
            "message": "Task board storage is unavailable"
        })
    );
    let response_text = serde_json::to_string(&response).unwrap();
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    for private_text in [
        panic_message,
        "dbPath",
        "rolloutPath",
        "private-snapshot-body",
    ] {
        assert!(!response_text.contains(private_text));
        assert!(!diagnostics.contains(private_text));
    }
}

#[tokio::test]
async fn catalog_returns_the_exact_private_safe_shape_with_warnings_and_redacted_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let ctx = context(
        Arc::new(FakeStore::document(TaskBoardDocument::empty())),
        Arc::new(CatalogData::success(sample_catalog())),
    );

    let response = handle_bridge_request(ctx, TASK_BOARD_SESSION_CATALOG_PATH, json!({})).await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        response,
        json!({
            "status": "ok",
            "projects": [{
                "cwd": "E:\\private\\project",
                "label": "Private Project",
                "sessionCount": 1
            }],
            "sessions": [{
                "sessionId": "private-session-id",
                "title": "private-session-title",
                "cwd": "E:\\private\\project",
                "updatedAtMs": 1_787_544_000_000_u64
            }],
            "warnings": [{
                "code": "codex_db_read_failed",
                "count": 2
            }]
        })
    );
    let encoded = serde_json::to_string(&response).unwrap();
    for forbidden_key in ["dbPath", "rolloutPath", "body", "content"] {
        assert!(!encoded.contains(forbidden_key));
    }
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    for private_text in [
        "private-session-id",
        "private-session-title",
        "E:\\\\private\\\\project",
        "Private Project",
    ] {
        assert!(!diagnostics.contains(private_text));
    }
}

#[tokio::test]
async fn catalog_failure_is_redacted_and_does_not_prevent_snapshot_success() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("diagnostic.log");
    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(log_path.clone()));
    let provider_error = concat!(
        "catalog provider failed: ",
        "dbPath=C:\\Users\\alice\\.codex\\state_5.sqlite ",
        "rolloutPath=E:\\private\\rollout\\session.json ",
        "body=private-session-body"
    );
    let ctx = context(
        Arc::new(FakeStore::document(sample_document())),
        Arc::new(CatalogData::failure(provider_error)),
    );

    let catalog =
        handle_bridge_request(ctx.clone(), TASK_BOARD_SESSION_CATALOG_PATH, json!({})).await;
    let snapshot = handle_bridge_request(ctx, TASK_BOARD_SNAPSHOT_PATH, json!({})).await;

    codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    assert_eq!(
        catalog,
        json!({
            "status": "failed",
            "code": "session_catalog_unavailable",
            "message": "Task board session catalog service is unavailable"
        })
    );
    let diagnostics = std::fs::read_to_string(log_path).unwrap();
    for private_text in [
        provider_error,
        "dbPath",
        "rolloutPath",
        "C:\\Users\\alice\\.codex\\state_5.sqlite",
        "E:\\private\\rollout\\session.json",
        "private-session-body",
    ] {
        assert!(
            !serde_json::to_string(&catalog)
                .unwrap()
                .contains(private_text)
        );
        assert!(!diagnostics.contains(private_text));
    }
    assert_eq!(snapshot["status"], "ok");
    assert_eq!(snapshot["revision"], 7);
}

#[tokio::test]
async fn corrupt_task_file_does_not_prevent_catalog_success() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("task-board.json");
    std::fs::write(&document_path, b"{not-json").unwrap();
    let store = FileTaskBoardStore::new(document_path.clone(), temp.path().join("task-board.lock"));
    let ctx = context(
        Arc::new(store),
        Arc::new(CatalogData::success(sample_catalog())),
    );

    let snapshot = handle_bridge_request(ctx.clone(), TASK_BOARD_SNAPSHOT_PATH, json!({})).await;
    let catalog = handle_bridge_request(ctx, TASK_BOARD_SESSION_CATALOG_PATH, json!({})).await;

    assert_eq!(snapshot["status"], "failed");
    assert_eq!(snapshot["code"], "task_file_invalid");
    assert_eq!(snapshot["path"], document_path.to_string_lossy().as_ref());
    assert_eq!(catalog["status"], "ok");
    assert_eq!(catalog["warnings"][0]["count"], 2);
}

enum SnapshotOutcome {
    Document(TaskBoardDocument),
    Unavailable(PathBuf, String),
    Panic(String),
}

struct FakeStore {
    outcome: SnapshotOutcome,
    observed_thread: Option<Arc<Mutex<Option<ThreadId>>>>,
}

impl FakeStore {
    fn document(document: TaskBoardDocument) -> Self {
        Self {
            outcome: SnapshotOutcome::Document(document),
            observed_thread: None,
        }
    }

    fn unavailable(path: PathBuf, message: &str) -> Self {
        Self {
            outcome: SnapshotOutcome::Unavailable(path, message.to_string()),
            observed_thread: None,
        }
    }

    fn panic(message: &str) -> Self {
        Self {
            outcome: SnapshotOutcome::Panic(message.to_string()),
            observed_thread: None,
        }
    }

    fn with_observed_thread(mut self, observed_thread: Arc<Mutex<Option<ThreadId>>>) -> Self {
        self.observed_thread = Some(observed_thread);
        self
    }
}

impl TaskBoardStore for FakeStore {
    fn snapshot(&self) -> Result<TaskBoardDocument, TaskBoardStoreError> {
        if let Some(observed_thread) = &self.observed_thread {
            *observed_thread.lock().unwrap() = Some(std::thread::current().id());
        }
        match &self.outcome {
            SnapshotOutcome::Document(document) => Ok(document.clone()),
            SnapshotOutcome::Unavailable(path, message) => Err(TaskBoardStoreError::Unavailable {
                path: path.clone(),
                message: message.clone(),
            }),
            SnapshotOutcome::Panic(message) => panic!("{message}"),
        }
    }

    fn create_task(
        &self,
        _command: TaskBoardCreateCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("T-006 placeholder must not call the store")
    }

    fn attach_conversations(
        &self,
        _command: TaskBoardAttachConversationsCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("T-006 placeholder must not call the store")
    }

    fn move_task(
        &self,
        _command: TaskBoardMoveCommand,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError> {
        panic!("T-006 placeholder must not call the store")
    }
}

struct LegacyData;

struct CatalogData {
    catalog: Option<TaskBoardSessionCatalog>,
    error: Option<String>,
}

impl CatalogData {
    fn success(catalog: TaskBoardSessionCatalog) -> Self {
        Self {
            catalog: Some(catalog),
            error: None,
        }
    }

    fn failure(message: &str) -> Self {
        Self {
            catalog: None,
            error: Some(message.to_string()),
        }
    }
}

#[async_trait]
impl BridgeDataService for LegacyData {
    async fn delete(&self, _session: SessionRef) -> anyhow::Result<DeleteResult> {
        anyhow::bail!("unused data route")
    }

    async fn export_markdown(&self, _session: SessionRef) -> anyhow::Result<ExportResult> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_history(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_summary(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn find_archived_thread_by_title(
        &self,
        _title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        anyhow::bail!("unused data route")
    }

    async fn move_thread_workspace(
        &self,
        _session: SessionRef,
        _target_cwd: String,
    ) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_key(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_keys(&self, _sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }
}

#[async_trait]
impl BridgeDataService for CatalogData {
    async fn delete(&self, _session: SessionRef) -> anyhow::Result<DeleteResult> {
        anyhow::bail!("unused data route")
    }

    async fn export_markdown(&self, _session: SessionRef) -> anyhow::Result<ExportResult> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_history(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_usage_summary(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn find_archived_thread_by_title(
        &self,
        _title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        anyhow::bail!("unused data route")
    }

    async fn move_thread_workspace(
        &self,
        _session: SessionRef,
        _target_cwd: String,
    ) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_key(&self, _session: SessionRef) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn thread_sort_keys(&self, _sessions: Vec<SessionRef>) -> anyhow::Result<Value> {
        anyhow::bail!("unused data route")
    }

    async fn task_board_session_catalog(&self) -> anyhow::Result<TaskBoardSessionCatalog> {
        match (&self.catalog, &self.error) {
            (Some(catalog), None) => Ok(catalog.clone()),
            (None, Some(error)) => anyhow::bail!("{error}"),
            _ => anyhow::bail!("invalid catalog fake"),
        }
    }
}
