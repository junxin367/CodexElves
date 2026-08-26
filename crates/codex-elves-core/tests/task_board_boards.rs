use codex_elves_core::task_board::{
    FileTaskBoardStore, TaskBoardColumn, TaskBoardCreateBoardCommand, TaskBoardDeleteBoardCommand,
    TaskBoardDocument, TaskBoardMoveBoardCommand, TaskBoardMoveCommand, TaskBoardProject,
    TaskBoardRenameBoardCommand, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
    TaskBoardTask, parse_task_board_document,
};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const CUSTOM_BOARD_ID: &str = "44d5ad90-897a-4c28-a09d-88d05d5b64f6";
const SECOND_BOARD_ID: &str = "33ebf7f5-845f-40f1-8f35-c611b12258ce";
const TASK_A: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4e";
const TASK_B: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d4f";
const TASK_C: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d50";
const TASK_D: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d51";
const TASK_E: &str = "62a0a38e-65bd-4c49-b6ef-3d19d06f2d52";

fn store_in(temp: &tempfile::TempDir) -> FileTaskBoardStore {
    FileTaskBoardStore::new(
        temp.path().join("task-board.json"),
        temp.path().join("task-board.lock"),
    )
}

fn status(id: &str) -> TaskBoardStatus {
    TaskBoardStatus::custom(Uuid::parse_str(id).unwrap())
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
        conversations: Vec::new(),
        created_at_ms: 100,
        updated_at_ms: 200,
    }
}

fn write_document(store: &FileTaskBoardStore, document: &TaskBoardDocument) {
    std::fs::write(
        store.document_path(),
        serde_json::to_vec_pretty(document).unwrap(),
    )
    .unwrap();
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

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

#[test]
fn create_board_appends_normalized_metadata_and_is_idempotent_by_id_and_label() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let custom_status = status(CUSTOM_BOARD_ID);

    let created = store
        .create_board(TaskBoardCreateBoardCommand {
            board_id: custom_status,
            expected_revision: 0,
            label: "  待发布  ".to_string(),
        })
        .unwrap();

    assert!(created.changed);
    assert!(!created.idempotent);
    assert_eq!(created.document.revision, 1);
    assert_eq!(
        created.document.boards.last(),
        Some(&TaskBoardColumn {
            id: custom_status,
            label: "待发布".to_string(),
            color: "#fb7185".to_string(),
        })
    );
    assert_eq!(store.snapshot().unwrap(), created.document);

    let original_bytes = std::fs::read(store.document_path()).unwrap();
    let retry = store
        .create_board(TaskBoardCreateBoardCommand {
            board_id: custom_status,
            expected_revision: 0,
            label: "待发布".to_string(),
        })
        .unwrap();
    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document.revision, 1);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );

    assert!(matches!(
        store.create_board(TaskBoardCreateBoardCommand {
            board_id: custom_status,
            expected_revision: 1,
            label: "另一个名称".to_string(),
        }),
        Err(TaskBoardStoreError::BoardIdConflict)
    ));
    assert!(matches!(
        store.create_board(TaskBoardCreateBoardCommand {
            board_id: status(SECOND_BOARD_ID),
            expected_revision: 1,
            label: "待发布".to_string(),
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn rename_board_normalizes_the_label_and_preserves_identity_color_order_and_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let custom_status = status(CUSTOM_BOARD_ID);
    let mut original = TaskBoardDocument::empty();
    original.revision = 7;
    original.boards.push(TaskBoardColumn {
        id: custom_status,
        label: "待发布".to_string(),
        color: "#fb7185".to_string(),
    });
    original.tasks.push(task(TASK_A, custom_status, 0));
    write_document(&store, &original);

    let renamed = store
        .rename_board(TaskBoardRenameBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 7,
            label: "  需求池  ".to_string(),
        })
        .unwrap();

    assert!(renamed.changed);
    assert!(!renamed.idempotent);
    assert_eq!(renamed.document.revision, 8);
    assert_eq!(
        renamed
            .document
            .boards
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        original
            .boards
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>()
    );
    let planning = renamed
        .document
        .boards
        .iter()
        .find(|board| board.id == TaskBoardStatus::Planning)
        .unwrap();
    assert_eq!(planning.label, "需求池");
    assert_eq!(planning.color, "#60a5fa");
    assert_eq!(renamed.document.tasks, original.tasks);
    assert_eq!(store.snapshot().unwrap(), renamed.document);

    let persisted_bytes = std::fs::read(store.document_path()).unwrap();
    let retry = store
        .rename_board(TaskBoardRenameBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 7,
            label: "需求池".to_string(),
        })
        .unwrap();
    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document, renamed.document);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        persisted_bytes
    );

    assert!(matches!(
        store.rename_board(TaskBoardRenameBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 8,
            label: "执行".to_string(),
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
}

#[test]
fn move_board_persists_managed_board_order_and_is_idempotent_at_the_target_index() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let mut original = TaskBoardDocument::empty();
    original.revision = 7;
    original
        .tasks
        .push(task(TASK_A, TaskBoardStatus::Planning, 0));
    write_document(&store, &original);

    let moved = store
        .move_board(TaskBoardMoveBoardCommand {
            board_id: TaskBoardStatus::Planning,
            target_index: 3,
            expected_revision: 7,
        })
        .unwrap();

    assert!(moved.changed);
    assert!(!moved.idempotent);
    assert_eq!(moved.document.revision, 8);
    assert_eq!(
        moved
            .document
            .boards
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![
            TaskBoardStatus::Executing,
            TaskBoardStatus::Review,
            TaskBoardStatus::Done,
            TaskBoardStatus::Planning,
        ]
    );
    assert_eq!(moved.document.tasks, original.tasks);
    assert_eq!(store.snapshot().unwrap(), moved.document);

    let persisted_bytes = std::fs::read(store.document_path()).unwrap();
    let retry = store
        .move_board(TaskBoardMoveBoardCommand {
            board_id: TaskBoardStatus::Planning,
            target_index: 3,
            expected_revision: 7,
        })
        .unwrap();
    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document, moved.document);
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        persisted_bytes
    );
}

#[test]
fn rename_and_move_board_reject_stale_revisions_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let mut original = TaskBoardDocument::empty();
    original.revision = 7;
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    match store.rename_board(TaskBoardRenameBoardCommand {
        board_id: TaskBoardStatus::Planning,
        expected_revision: 6,
        label: "需求池".to_string(),
    }) {
        Err(TaskBoardStoreError::RevisionConflict { current }) => {
            assert_eq!(current, original);
        }
        result => panic!("expected rename revision conflict, got {result:?}"),
    }
    match store.move_board(TaskBoardMoveBoardCommand {
        board_id: TaskBoardStatus::Planning,
        target_index: 3,
        expected_revision: 6,
    }) {
        Err(TaskBoardStoreError::RevisionConflict { current }) => {
            assert_eq!(current, original);
        }
        result => panic!("expected move revision conflict, got {result:?}"),
    }
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );
}

#[test]
fn delete_default_board_moves_its_tasks_after_existing_unassigned_tasks_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let custom_status = status(CUSTOM_BOARD_ID);
    let mut boards = TaskBoardDocument::default_boards();
    boards.push(TaskBoardColumn {
        id: custom_status,
        label: "待发布".to_string(),
        color: "#fb7185".to_string(),
    });
    let original = TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        boards,
        tasks: vec![
            task(TASK_A, TaskBoardStatus::New, 0),
            task(TASK_B, TaskBoardStatus::New, 1),
            task(TASK_C, TaskBoardStatus::Planning, 0),
            task(TASK_D, TaskBoardStatus::Planning, 1),
            task(TASK_E, custom_status, 0),
        ],
    };
    write_document(&store, &original);
    let before_delete = unix_timestamp_ms();

    let deleted = store
        .delete_board(TaskBoardDeleteBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 7,
        })
        .unwrap();
    let after_delete = unix_timestamp_ms();

    assert!(deleted.changed);
    assert!(!deleted.idempotent);
    assert_eq!(deleted.document.revision, 8);
    assert!(
        deleted
            .document
            .boards
            .iter()
            .all(|board| board.id != TaskBoardStatus::Planning)
    );
    assert_eq!(
        column_ids(&deleted.document, TaskBoardStatus::New),
        [TASK_A, TASK_B, TASK_C, TASK_D].map(str::to_string)
    );
    assert_eq!(
        column_ids(&deleted.document, custom_status),
        vec![TASK_E.to_string()]
    );
    for task_id in [TASK_C, TASK_D] {
        let moved = deleted
            .document
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(moved.status, TaskBoardStatus::New);
        assert!(moved.updated_at_ms >= before_delete);
        assert!(moved.updated_at_ms <= after_delete);
    }
    for task_id in [TASK_A, TASK_B, TASK_E] {
        let before = original
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        let after = deleted
            .document
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(after.updated_at_ms, before.updated_at_ms);
    }

    let persisted =
        parse_task_board_document(&std::fs::read(store.document_path()).unwrap()).unwrap();
    assert_eq!(persisted, deleted.document);

    let retry = store
        .delete_board(TaskBoardDeleteBoardCommand {
            board_id: TaskBoardStatus::Planning,
            expected_revision: 7,
        })
        .unwrap();
    assert!(!retry.changed);
    assert!(retry.idempotent);
    assert_eq!(retry.document, deleted.document);
}

#[test]
fn tasks_can_move_into_a_custom_managed_board() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let custom_status = status(CUSTOM_BOARD_ID);
    let mut document = TaskBoardDocument::empty();
    document.revision = 7;
    document.boards.push(TaskBoardColumn {
        id: custom_status,
        label: "待发布".to_string(),
        color: "#fb7185".to_string(),
    });
    document.tasks.push(task(TASK_A, TaskBoardStatus::New, 0));
    write_document(&store, &document);

    let moved = store
        .move_task(TaskBoardMoveCommand {
            task_id: TASK_A.to_string(),
            to_status: custom_status,
            target_index: 0,
            expected_revision: 7,
        })
        .unwrap();

    assert!(moved.changed);
    assert_eq!(moved.document.revision, 8);
    assert!(column_ids(&moved.document, TaskBoardStatus::New).is_empty());
    assert_eq!(
        column_ids(&moved.document, custom_status),
        vec![TASK_A.to_string()]
    );
    assert_eq!(store.snapshot().unwrap(), moved.document);
}

#[test]
fn unassigned_cannot_be_deleted_and_tasks_cannot_move_to_a_missing_board() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_in(&temp);
    let original = TaskBoardDocument {
        schema_version: 1,
        revision: 7,
        boards: TaskBoardDocument::default_boards(),
        tasks: vec![task(TASK_A, TaskBoardStatus::New, 0)],
    };
    write_document(&store, &original);
    let original_bytes = std::fs::read(store.document_path()).unwrap();

    assert!(matches!(
        store.delete_board(TaskBoardDeleteBoardCommand {
            board_id: TaskBoardStatus::New,
            expected_revision: 7,
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert!(matches!(
        store.rename_board(TaskBoardRenameBoardCommand {
            board_id: TaskBoardStatus::New,
            expected_revision: 7,
            label: "收件箱".to_string(),
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert!(matches!(
        store.move_board(TaskBoardMoveBoardCommand {
            board_id: TaskBoardStatus::New,
            target_index: 0,
            expected_revision: 7,
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
    assert!(matches!(
        store.move_task(TaskBoardMoveCommand {
            task_id: TASK_A.to_string(),
            to_status: status(CUSTOM_BOARD_ID),
            target_index: 0,
            expected_revision: 7,
        }),
        Err(TaskBoardStoreError::BoardNotFound)
    ));
    assert_eq!(
        std::fs::read(store.document_path()).unwrap(),
        original_bytes
    );

    let missing_delete = store
        .delete_board(TaskBoardDeleteBoardCommand {
            board_id: status(CUSTOM_BOARD_ID),
            expected_revision: 0,
        })
        .unwrap();
    assert!(!missing_delete.changed);
    assert!(missing_delete.idempotent);
    assert_eq!(missing_delete.document, original);

    assert!(matches!(
        store.rename_board(TaskBoardRenameBoardCommand {
            board_id: status(CUSTOM_BOARD_ID),
            expected_revision: 7,
            label: "不存在".to_string(),
        }),
        Err(TaskBoardStoreError::BoardNotFound)
    ));
    assert!(matches!(
        store.move_board(TaskBoardMoveBoardCommand {
            board_id: status(CUSTOM_BOARD_ID),
            target_index: 0,
            expected_revision: 7,
        }),
        Err(TaskBoardStoreError::BoardNotFound)
    ));
    assert!(matches!(
        store.move_board(TaskBoardMoveBoardCommand {
            board_id: TaskBoardStatus::Planning,
            target_index: 4,
            expected_revision: 7,
        }),
        Err(TaskBoardStoreError::InvalidInput { .. })
    ));
}
