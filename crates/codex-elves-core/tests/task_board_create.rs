use codex_elves_core::task_board::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardConversation, TaskBoardCreateCommand,
    TaskBoardProject, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
};
use std::sync::{Arc, Barrier};

const TASK_ID: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const SESSION_A: &str = "019c89c0-0000-7000-8000-000000000001";
const SESSION_B: &str = "019c89c0-0000-7000-8000-000000000002";
const OTHER_TASK_ID: &str = "72a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";

fn store_in(temp: &tempfile::TempDir) -> FileTaskBoardStore {
    FileTaskBoardStore::new(
        temp.path().join("task-board.json"),
        temp.path().join("task-board.lock"),
    )
}

fn create_command(task_id: &str, expected_revision: u64) -> TaskBoardCreateCommand {
    TaskBoardCreateCommand {
        task_id: task_id.to_string(),
        expected_revision,
        title: "  完善任务看板  ".to_string(),
        project: TaskBoardProject {
            cwd: " e:/Code/CodexElves/ ".to_string(),
            label: "CodexElves".to_string(),
        },
        conversations: vec![
            TaskBoardConversation {
                session_id: SESSION_A.to_string(),
                title: "设计任务看板".to_string(),
                cwd: "E:\\CODE\\CodexElves".to_string(),
                updated_at_ms: Some(1_787_544_000_000),
            },
            TaskBoardConversation {
                session_id: SESSION_B.to_string(),
                title: "实现任务看板".to_string(),
                cwd: "e:/code/codexelves/".to_string(),
                updated_at_ms: None,
            },
        ],
    }
}

#[test]
fn create_appends_a_normalized_task_to_the_new_column() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);

    let result = store.create_task(create_command(TASK_ID, 0)).unwrap();

    assert!(result.changed);
    assert!(!result.idempotent);
    assert_eq!(result.document.revision, 1);
    assert_eq!(result.document.tasks.len(), 1);
    let task = &result.document.tasks[0];
    assert_eq!(task.id, TASK_ID);
    assert_eq!(task.title, "完善任务看板");
    assert_eq!(task.project.cwd, "E:\\code\\codexelves");
    assert_eq!(task.status, TaskBoardStatus::New);
    assert_eq!(task.order, 0);
    assert_eq!(task.conversations.len(), 2);
    assert!(
        task.conversations
            .iter()
            .all(|conversation| conversation.cwd == task.project.cwd)
    );
    assert_eq!(task.created_at_ms, task.updated_at_ms);
    assert!(task.created_at_ms > 0);
    assert!(task.created_at_ms <= TASK_BOARD_MAX_SAFE_INTEGER);
    assert_eq!(store.snapshot().unwrap(), result.document);
}

#[test]
fn same_id_and_semantic_identity_is_idempotent_before_revision_check() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let first = store.create_task(create_command(TASK_ID, 0)).unwrap();
    let mut persisted = first.document;
    persisted.revision = 7;
    persisted.tasks[0].project.label = "stored label".to_string();
    persisted.tasks[0].status = TaskBoardStatus::Done;
    persisted.tasks[0].order = 0;
    persisted.tasks[0].created_at_ms = 10;
    persisted.tasks[0].updated_at_ms = 20;
    persisted.tasks[0].conversations[0].title = "stored title A".to_string();
    persisted.tasks[0].conversations[0].updated_at_ms = None;
    persisted.tasks[0].conversations[1].title = "stored title B".to_string();
    persisted.tasks[0].conversations[1].updated_at_ms = Some(30);
    std::fs::write(
        store.document_path(),
        serde_json::to_vec_pretty(&persisted).unwrap(),
    )
    .unwrap();
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let mut retry = create_command(TASK_ID, 0);
    retry.project.label = "retry label is ignored".to_string();
    retry.conversations.reverse();
    retry.conversations[0].title = "retry title is ignored".to_string();
    retry.conversations[0].updated_at_ms = Some(999);
    let result = store.create_task(retry).unwrap();

    assert!(!result.changed);
    assert!(result.idempotent);
    assert_eq!(result.document, persisted);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn same_id_with_different_semantic_identity_is_task_id_conflict_before_revision_check() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    store.create_task(create_command(TASK_ID, 0)).unwrap();
    let original_bytes = std::fs::read(store.document_path()).unwrap();
    let mut conflicting = create_command(TASK_ID, 0);
    conflicting.title = "不同任务".to_string();

    assert!(matches!(
        store.create_task(conflicting),
        Err(TaskBoardStoreError::TaskIdConflict)
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn new_id_with_stale_revision_returns_the_latest_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let first = store.create_task(create_command(TASK_ID, 0)).unwrap();
    let original_bytes = std::fs::read(store.document_path()).unwrap();
    let stale = create_command(OTHER_TASK_ID, 0);

    let error = store.create_task(stale).unwrap_err();

    assert!(matches!(
        error,
        TaskBoardStoreError::RevisionConflict { current } if current == first.document
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn invalid_create_inputs_return_invalid_input_without_changing_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    store.create_task(create_command(TASK_ID, 0)).unwrap();
    let original_bytes = std::fs::read(store.document_path()).unwrap();
    let mut cases = Vec::new();

    let mut invalid_uuid = create_command("not-a-uuid", 1);
    invalid_uuid.title = "invalid uuid".to_string();
    cases.push(invalid_uuid);

    let mut empty_title = create_command(OTHER_TASK_ID, 1);
    empty_title.title = " \t ".to_string();
    cases.push(empty_title);

    let mut no_conversations = create_command(OTHER_TASK_ID, 1);
    no_conversations.conversations.clear();
    cases.push(no_conversations);

    let mut duplicate_conversation = create_command(OTHER_TASK_ID, 1);
    duplicate_conversation.conversations[1].session_id = SESSION_A.to_ascii_uppercase();
    cases.push(duplicate_conversation);

    let mut temporary_session = create_command(OTHER_TASK_ID, 1);
    temporary_session.conversations[0].session_id =
        "provider:local:client-new-thread:temporary".to_string();
    cases.push(temporary_session);

    let mut cross_project = create_command(OTHER_TASK_ID, 1);
    cross_project.conversations[0].cwd = "E:\\other-project".to_string();
    cases.push(cross_project);

    let mut unsafe_expected_revision =
        create_command(OTHER_TASK_ID, TASK_BOARD_MAX_SAFE_INTEGER + 1);
    unsafe_expected_revision.title = "unsafe revision".to_string();
    cases.push(unsafe_expected_revision);

    let mut unsafe_timestamp = create_command(OTHER_TASK_ID, 1);
    unsafe_timestamp.conversations[0].updated_at_ms = Some(TASK_BOARD_MAX_SAFE_INTEGER + 1);
    cases.push(unsafe_timestamp);

    for command in cases {
        assert!(matches!(
            store.create_task(command),
            Err(TaskBoardStoreError::InvalidInput { .. })
        ));
        assert_eq!(
            std::fs::read(store.document_path()).unwrap(),
            original_bytes
        );
    }
}

#[test]
fn successive_creates_preserve_tasks_and_append_continuous_new_order() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let first = store.create_task(create_command(TASK_ID, 0)).unwrap();
    let first_task = first.document.tasks[0].clone();

    let second = store.create_task(create_command(OTHER_TASK_ID, 1)).unwrap();

    assert!(second.changed);
    assert!(!second.idempotent);
    assert_eq!(second.document.revision, 2);
    assert_eq!(second.document.tasks.len(), 2);
    assert_eq!(second.document.tasks[0], first_task);
    assert_eq!(second.document.tasks[0].order, 0);
    assert_eq!(second.document.tasks[1].order, 1);
    assert!(
        second
            .document
            .tasks
            .iter()
            .all(|task| task.status == TaskBoardStatus::New)
    );
}

#[test]
fn concurrent_creates_use_revision_conflict_then_retry_without_lost_update() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(store_in(&temp));
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();

    for task_id in [TASK_ID, OTHER_TASK_ID] {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let task_id = task_id.to_string();
        handles.push(std::thread::spawn(move || {
            let command = create_command(&task_id, 0);
            barrier.wait();
            (task_id, store.create_task(command))
        }));
    }
    barrier.wait();

    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    let mut success_count = 0;
    let mut loser_id = None;
    for (task_id, outcome) in outcomes {
        match outcome {
            Ok(result) => {
                success_count += 1;
                assert!(result.changed);
                assert_eq!(result.document.revision, 1);
            }
            Err(TaskBoardStoreError::RevisionConflict { current }) => {
                assert_eq!(current.revision, 1);
                assert_eq!(current.tasks.len(), 1);
                loser_id = Some(task_id);
            }
            Err(error) => panic!("unexpected concurrent create result: {error}"),
        }
    }
    assert_eq!(success_count, 1);

    let retry = store
        .create_task(create_command(&loser_id.unwrap(), 1))
        .unwrap();

    assert_eq!(retry.document.revision, 2);
    assert_eq!(retry.document.tasks.len(), 2);
    assert_eq!(
        retry
            .document
            .tasks
            .iter()
            .map(|task| task.order)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(store.snapshot().unwrap(), retry.document);
}
