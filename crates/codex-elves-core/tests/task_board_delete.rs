use codex_elves_core::task_board::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardConversation, TaskBoardDeleteCommand,
    TaskBoardDocument, TaskBoardProject, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
    TaskBoardTask,
};

const TASK_A: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const TASK_B: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4f";
const TASK_C: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d50";
const TASK_D: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d51";

fn store_in(temp: &tempfile::TempDir) -> FileTaskBoardStore {
    FileTaskBoardStore::new(
        temp.path().join("task-board.json"),
        temp.path().join("task-board.lock"),
    )
}

fn task(id: &str, status: TaskBoardStatus, order: u32) -> TaskBoardTask {
    TaskBoardTask {
        id: id.to_string(),
        title: format!("任务 {id}"),
        project: TaskBoardProject {
            cwd: "E:\\code\\codexelves".to_string(),
            label: "CodexElves".to_string(),
        },
        status,
        order,
        conversations: vec![TaskBoardConversation {
            session_id: format!("session-{id}"),
            title: "原始会话".to_string(),
            cwd: "E:\\code\\codexelves".to_string(),
            updated_at_ms: Some(123),
        }],
        created_at_ms: 100,
        updated_at_ms: 200,
    }
}

fn document() -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        tasks: vec![
            task(TASK_A, TaskBoardStatus::New, 0),
            task(TASK_B, TaskBoardStatus::New, 1),
            task(TASK_C, TaskBoardStatus::New, 2),
            task(TASK_D, TaskBoardStatus::Planning, 0),
        ],
    }
}

fn write_document(store: &FileTaskBoardStore, document: &TaskBoardDocument) {
    std::fs::write(
        store.document_path(),
        serde_json::to_vec_pretty(document).unwrap(),
    )
    .unwrap();
}

fn command(task_id: &str, expected_revision: u64) -> TaskBoardDeleteCommand {
    TaskBoardDeleteCommand {
        task_id: task_id.to_string(),
        expected_revision,
    }
}

#[test]
fn delete_removes_only_the_task_and_compacts_its_status_column() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    write_document(&store, &document());

    let result = store.delete_task(command(TASK_B, 7)).unwrap();

    assert!(result.changed);
    assert!(!result.idempotent);
    assert_eq!(result.document.revision, 8);
    assert_eq!(
        result
            .document
            .tasks
            .iter()
            .map(|task| (task.id.as_str(), task.status, task.order))
            .collect::<Vec<_>>(),
        vec![
            (TASK_A, TaskBoardStatus::New, 0),
            (TASK_C, TaskBoardStatus::New, 1),
            (TASK_D, TaskBoardStatus::Planning, 0),
        ]
    );
    assert!(
        result
            .document
            .tasks
            .iter()
            .all(|task| task.conversations[0].session_id != format!("session-{TASK_B}"))
    );
    assert_eq!(store.snapshot().unwrap(), result.document);
}

#[test]
fn delete_retry_is_idempotent_before_revision_check() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    write_document(&store, &document());
    let deleted = store.delete_task(command(TASK_B, 7)).unwrap();

    let retry = store.delete_task(command(TASK_B, 7)).unwrap();

    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document, deleted.document);
}

#[test]
fn stale_revision_and_invalid_commands_preserve_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    write_document(&store, &document());
    let original = std::fs::read(store.document_path()).unwrap();

    let stale = store.delete_task(command(TASK_B, 6));
    assert!(matches!(
        stale,
        Err(TaskBoardStoreError::RevisionConflict { current }) if current == document()
    ));
    assert_eq!(std::fs::read(store.document_path()).unwrap(), original);

    for invalid in [
        command("not-a-uuid", 7),
        command(TASK_B, TASK_BOARD_MAX_SAFE_INTEGER + 1),
    ] {
        assert!(matches!(
            store.delete_task(invalid),
            Err(TaskBoardStoreError::InvalidInput { .. })
        ));
        assert_eq!(std::fs::read(store.document_path()).unwrap(), original);
    }
}
