use codex_elves_core::task_board::{
    FileTaskBoardStore, TaskBoardAttachConversationsCommand, TaskBoardConversation,
    TaskBoardCreateCommand, TaskBoardProject, TaskBoardStore, TaskBoardStoreError,
};

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const MISSING_TASK_ID: &str = "72a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const SESSION_A: &str = "019c89c0-0000-7000-8000-000000000001";
const SESSION_B: &str = "019c89c0-0000-7000-8000-000000000002";

fn store_in(temp: &tempfile::TempDir) -> FileTaskBoardStore {
    FileTaskBoardStore::new(
        temp.path().join("task-board.json"),
        temp.path().join("task-board.lock"),
    )
}

fn conversation(session_id: &str, cwd: &str) -> TaskBoardConversation {
    TaskBoardConversation {
        session_id: session_id.to_string(),
        title: format!("会话 {session_id}"),
        cwd: cwd.to_string(),
        updated_at_ms: Some(1_787_544_000_000),
    }
}

fn seed_task(store: &FileTaskBoardStore) {
    store
        .create_task(TaskBoardCreateCommand {
            task_id: TASK_ID.to_string(),
            expected_revision: 0,
            title: "完善任务看板".to_string(),
            project: TaskBoardProject {
                cwd: "E:\\code\\codexelves".to_string(),
                label: "CodexElves".to_string(),
            },
            conversations: vec![conversation(SESSION_A, "E:\\code\\codexelves")],
        })
        .unwrap();
}

fn attach_command(
    task_id: &str,
    expected_revision: u64,
    conversations: Vec<TaskBoardConversation>,
) -> TaskBoardAttachConversationsCommand {
    TaskBoardAttachConversationsCommand {
        task_id: task_id.to_string(),
        expected_revision,
        conversations,
    }
}

#[test]
fn attach_appends_conversations_and_increments_revision_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    seed_task(&store);
    let before = store.snapshot().unwrap();

    let result = store
        .attach_conversations(attach_command(
            TASK_ID,
            1,
            vec![conversation(SESSION_B, "e:/CODE/CodexElves/")],
        ))
        .unwrap();

    assert!(result.changed);
    assert!(!result.idempotent);
    assert_eq!(result.document.revision, 2);
    assert_eq!(
        result.document.tasks[0]
            .conversations
            .iter()
            .map(|conversation| conversation.session_id.as_str())
            .collect::<Vec<_>>(),
        vec![SESSION_A, SESSION_B]
    );
    assert_eq!(
        result.document.tasks[0].conversations[1].cwd,
        "E:\\code\\codexelves"
    );
    assert!(result.document.tasks[0].updated_at_ms >= before.tasks[0].updated_at_ms);
    assert_eq!(store.snapshot().unwrap(), result.document);
}

#[test]
fn attach_retry_is_idempotent_before_revision_check() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    seed_task(&store);
    let command = attach_command(
        TASK_ID,
        1,
        vec![conversation(SESSION_B, "E:\\code\\codexelves")],
    );
    let first = store.attach_conversations(command.clone()).unwrap();
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let retry = store.attach_conversations(command).unwrap();

    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document, first.document);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn attach_rejects_cross_project_and_stale_revision_without_changing_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    seed_task(&store);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    assert!(matches!(
        store.attach_conversations(attach_command(
            TASK_ID,
            1,
            vec![conversation(SESSION_B, "E:\\other")],
        )),
        Err(TaskBoardStoreError::ProjectMismatch)
    ));
    assert!(matches!(
        store.attach_conversations(attach_command(
            TASK_ID,
            0,
            vec![conversation(SESSION_B, "E:\\code\\codexelves")],
        )),
        Err(TaskBoardStoreError::RevisionConflict { .. })
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn attach_rejects_missing_task_and_invalid_session_sets() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    seed_task(&store);

    assert!(matches!(
        store.attach_conversations(attach_command(
            MISSING_TASK_ID,
            1,
            vec![conversation(SESSION_B, "E:\\code\\codexelves")],
        )),
        Err(TaskBoardStoreError::TaskNotFound)
    ));
    assert!(matches!(
        store.attach_conversations(attach_command(TASK_ID, 1, Vec::new())),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert!(matches!(
        store.attach_conversations(attach_command(
            TASK_ID,
            1,
            vec![
                conversation(SESSION_B, "E:\\code\\codexelves"),
                conversation(&SESSION_B.to_ascii_uppercase(), "E:\\code\\codexelves"),
            ],
        )),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert!(matches!(
        store.attach_conversations(attach_command(
            TASK_ID,
            1,
            vec![conversation(
                "local:client-new-thread:temporary",
                "E:\\code\\codexelves",
            )],
        )),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
}
