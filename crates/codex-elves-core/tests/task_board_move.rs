use codex_elves_core::task_board::{
    FileTaskBoardStore, TASK_BOARD_MAX_SAFE_INTEGER, TaskBoardConversation, TaskBoardDocument,
    TaskBoardMoveCommand, TaskBoardProject, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
    TaskBoardTask, parse_task_board_document,
};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TASK_A: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const TASK_B: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4f";
const TASK_C: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d50";
const TASK_D: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d51";
const TASK_E: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d52";
const TASK_F: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d53";

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
            session_id: "019c89c0-0000-7000-8000-000000000001".to_string(),
            title: "会话".to_string(),
            cwd: "E:\\code\\codexelves".to_string(),
            updated_at_ms: Some(123),
        }],
        created_at_ms: 100,
        updated_at_ms: 200,
    }
}

fn document(tasks: Vec<TaskBoardTask>) -> TaskBoardDocument {
    TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        boards: TaskBoardDocument::default_boards(),
        tasks,
    }
}

fn write_document(store: &FileTaskBoardStore, document: &TaskBoardDocument) {
    std::fs::write(
        store.document_path(),
        serde_json::to_vec_pretty(document).unwrap(),
    )
    .unwrap();
}

fn command(
    task_id: &str,
    to_status: TaskBoardStatus,
    target_index: u32,
    expected_revision: u64,
) -> TaskBoardMoveCommand {
    TaskBoardMoveCommand {
        task_id: task_id.to_string(),
        to_status,
        target_index,
        expected_revision,
    }
}

fn column_ids(document: &TaskBoardDocument, status: TaskBoardStatus) -> Vec<String> {
    let mut tasks = document
        .tasks
        .iter()
        .filter(|task| task.status == status)
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| task.order);
    tasks.into_iter().map(|task| task.id.clone()).collect()
}

fn task_by_id<'a>(document: &'a TaskBoardDocument, id: &str) -> &'a TaskBoardTask {
    document
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap_or_else(|| panic!("task {id} is missing"))
}

fn assert_continuous_orders(document: &TaskBoardDocument) {
    for status in TaskBoardStatus::ALL {
        let orders = document
            .tasks
            .iter()
            .filter(|task| task.status == status)
            .map(|task| task.order)
            .collect::<Vec<_>>();
        assert_eq!(
            orders.len(),
            orders
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "orders must be unique in {status:?}"
        );
        assert_eq!(
            orders
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            (0..document
                .tasks
                .iter()
                .filter(|task| task.status == status)
                .count() as u32)
                .collect(),
            "orders must be continuous in {status:?}"
        );
    }
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

#[test]
fn moves_a_task_into_each_of_the_five_destination_columns() {
    for destination in TaskBoardStatus::ALL {
        let temp = tempfile::tempdir().unwrap();
        let store = store_in(&temp);
        let original = match destination {
            TaskBoardStatus::New => document(vec![
                task(TASK_A, TaskBoardStatus::New, 0),
                task(TASK_B, TaskBoardStatus::New, 1),
            ]),
            _ => document(vec![
                task(TASK_A, TaskBoardStatus::New, 0),
                task(TASK_B, destination, 0),
            ]),
        };
        write_document(&store, &original);

        let target_index = if destination == TaskBoardStatus::New {
            1
        } else {
            0
        };
        let result = store
            .move_task(command(TASK_A, destination, target_index, 7))
            .unwrap();

        assert!(result.changed, "destination: {destination:?}");
        assert!(!result.idempotent, "destination: {destination:?}");
        assert_eq!(result.document.revision, 8);
        assert_eq!(task_by_id(&result.document, TASK_A).status, destination);
        let expected_ids = if destination == TaskBoardStatus::New {
            vec![TASK_B.to_string(), TASK_A.to_string()]
        } else {
            vec![TASK_A.to_string(), TASK_B.to_string()]
        };
        assert_eq!(column_ids(&result.document, destination), expected_ids);
        assert_continuous_orders(&result.document);
        assert_eq!(store.snapshot().unwrap(), result.document);
    }
}

#[test]
fn cross_column_move_reorders_only_affected_columns_and_updates_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::New, 0),
        task(TASK_B, TaskBoardStatus::Planning, 0),
        task(TASK_C, TaskBoardStatus::Planning, 1),
        task(TASK_D, TaskBoardStatus::Executing, 0),
        task(TASK_E, TaskBoardStatus::Review, 0),
        task(TASK_F, TaskBoardStatus::Done, 0),
    ]);
    write_document(&store, &original);
    let before_move = unix_timestamp_ms();

    let result = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 1, 7))
        .unwrap();
    let after_move = unix_timestamp_ms();

    assert!(result.changed);
    assert!(!result.idempotent);
    assert_eq!(result.document.revision, 8);
    assert_eq!(
        column_ids(&result.document, TaskBoardStatus::New),
        Vec::<String>::new()
    );
    assert_eq!(
        column_ids(&result.document, TaskBoardStatus::Planning),
        vec![TASK_B.to_string(), TASK_A.to_string(), TASK_C.to_string()]
    );
    let moved = task_by_id(&result.document, TASK_A);
    assert_eq!(moved.status, TaskBoardStatus::Planning);
    assert_eq!(moved.order, 1);
    assert_eq!(moved.created_at_ms, 100);
    assert!(moved.updated_at_ms >= before_move);
    assert!(moved.updated_at_ms <= after_move);
    assert!(moved.updated_at_ms <= TASK_BOARD_MAX_SAFE_INTEGER);
    for id in [TASK_D, TASK_E, TASK_F] {
        assert_eq!(task_by_id(&result.document, id), task_by_id(&original, id));
    }
    assert_continuous_orders(&result.document);
}

#[test]
fn same_column_forward_and_backward_reorders_use_post_removal_indexes() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::Planning, 0),
        task(TASK_B, TaskBoardStatus::Planning, 1),
        task(TASK_C, TaskBoardStatus::Planning, 2),
    ]);
    write_document(&store, &original);

    let forward = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 2, 7))
        .unwrap();
    assert_eq!(
        column_ids(&forward.document, TaskBoardStatus::Planning),
        vec![TASK_B.to_string(), TASK_C.to_string(), TASK_A.to_string()]
    );
    assert_eq!(forward.document.revision, 8);

    let backward = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 0, 8))
        .unwrap();
    assert_eq!(
        column_ids(&backward.document, TaskBoardStatus::Planning),
        vec![TASK_A.to_string(), TASK_B.to_string(), TASK_C.to_string()]
    );
    assert_eq!(backward.document.revision, 9);
    assert_continuous_orders(&backward.document);
}

#[test]
fn inserts_at_zero_and_end_after_removing_the_source_task() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::New, 0),
        task(TASK_B, TaskBoardStatus::Planning, 0),
        task(TASK_C, TaskBoardStatus::Planning, 1),
    ]);
    write_document(&store, &original);

    let at_zero = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 0, 7))
        .unwrap();
    assert_eq!(
        column_ids(&at_zero.document, TaskBoardStatus::Planning),
        vec![TASK_A.to_string(), TASK_B.to_string(), TASK_C.to_string()]
    );

    let at_end = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 2, 8))
        .unwrap();
    assert_eq!(
        column_ids(&at_end.document, TaskBoardStatus::Planning),
        vec![TASK_B.to_string(), TASK_C.to_string(), TASK_A.to_string()]
    );
    assert_continuous_orders(&at_end.document);
}

#[test]
fn exact_same_column_noop_returns_current_snapshot_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::Planning, 0),
        task(TASK_B, TaskBoardStatus::Planning, 1),
        task(TASK_C, TaskBoardStatus::Planning, 2),
    ]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let result = store
        .move_task(command(TASK_B, TaskBoardStatus::Planning, 1, 7))
        .unwrap();

    assert!(!result.changed);
    assert!(!result.idempotent);
    assert_eq!(result.document, original);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn out_of_range_target_index_preserves_bytes_and_revision() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::New, 0),
        task(TASK_B, TaskBoardStatus::Planning, 0),
        task(TASK_C, TaskBoardStatus::Planning, 1),
    ]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let error = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 3, 7))
        .unwrap_err();

    assert!(matches!(error, TaskBoardStoreError::InvalidInput { .. }));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
    assert_eq!(store.snapshot().unwrap(), original);
}

#[test]
fn missing_task_and_stale_revision_preserve_bytes_and_return_the_latest_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![task(TASK_A, TaskBoardStatus::New, 0)]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let missing = store
        .move_task(command(TASK_B, TaskBoardStatus::Planning, 0, 7))
        .unwrap_err();
    assert!(matches!(missing, TaskBoardStoreError::TaskNotFound));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );

    let stale = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 0, 6))
        .unwrap_err();
    assert!(matches!(
        stale,
        TaskBoardStoreError::RevisionConflict { ref current } if current == &original
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
    assert_eq!(store.snapshot().unwrap(), original);
}

#[test]
fn invalid_uuid_is_rejected_without_touching_the_document() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![task(TASK_A, TaskBoardStatus::New, 0)]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let error = store
        .move_task(command(
            "not-a-uuid",
            TaskBoardStatus::Planning,
            0,
            original.revision,
        ))
        .unwrap_err();

    assert!(matches!(error, TaskBoardStoreError::InvalidInput { .. }));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
    assert_eq!(store.snapshot().unwrap(), original);
}

#[test]
fn locked_move_returns_busy_without_writing_the_document() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![task(TASK_A, TaskBoardStatus::New, 0)]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(store.lock_path())
        .unwrap();
    lock_file.lock_exclusive().unwrap();
    let store = store.with_lock_timing(Duration::from_millis(40), Duration::from_millis(2));

    let error = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 0, 7))
        .unwrap_err();

    assert!(matches!(error, TaskBoardStoreError::Busy));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
    lock_file.unlock().unwrap();
}

#[test]
fn changed_move_is_atomically_persisted_without_temporary_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = document(vec![
        task(TASK_A, TaskBoardStatus::New, 0),
        task(TASK_B, TaskBoardStatus::Planning, 0),
    ]);
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    let result = store
        .move_task(command(TASK_A, TaskBoardStatus::Planning, 1, 7))
        .unwrap();
    let persisted = std::fs::read(store.document_path()).unwrap();

    assert!(result.changed);
    assert_ne!(persisted, original_bytes);
    assert_eq!(
        parse_task_board_document(&persisted).unwrap(),
        result.document
    );
    assert!(std::fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}
