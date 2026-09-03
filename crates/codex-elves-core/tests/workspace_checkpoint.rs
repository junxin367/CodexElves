use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use codex_elves_core::workspace_checkpoint::{
    BindTurnRequest, CompleteTurnRequest, CreateCheckpointRequest,
    DeleteWorkspaceCheckpointDataRequest, ListCheckpointsRequest, RestoreCheckpointRequest,
    RestoreForRevertRequest, WorkspaceCheckpointChangeScope, WorkspaceCheckpointKind,
    WorkspaceCheckpointService, WorkspaceCheckpointTurnStatus,
};

#[test]
fn shadow_git_checkpoints_restore_turn_state_without_touching_project_git() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let checkpoint_root = temp.path().join("checkpoint-state");
    fs::create_dir_all(&workspace).unwrap();
    init_project_git(&workspace);
    fs::write(workspace.join("tracked.txt"), "baseline\n").unwrap();
    fs::write(workspace.join(".gitignore"), "ignored.txt\n").unwrap();
    run_project_git(&workspace, ["add", "."]);
    run_project_git(&workspace, ["commit", "-m", "baseline"]);
    let project_head = git_stdout(&workspace, ["rev-parse", "HEAD"]);

    let service = WorkspaceCheckpointService::new(checkpoint_root.clone());
    let first = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-1".to_string(),
            prompt_preview: "first prompt".to_string(),
        })
        .unwrap()
        .checkpoint;
    service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: first.id.clone(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        })
        .unwrap();

    fs::write(workspace.join("tracked.txt"), "after first AI turn\n").unwrap();
    fs::write(workspace.join("created-by-first.txt"), "first\n").unwrap();
    service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap();
    let second = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-2".to_string(),
            prompt_preview: "second prompt".to_string(),
        })
        .unwrap()
        .checkpoint;
    service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: second.id.clone(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
        })
        .unwrap();

    fs::write(workspace.join("tracked.txt"), "after second AI turn\n").unwrap();
    fs::write(workspace.join("created-by-second.txt"), "second\n").unwrap();
    fs::write(workspace.join("ignored.txt"), "ignored current value\n").unwrap();
    service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap();

    let restored = service
        .restore_for_revert(RestoreForRevertRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            before_turn_id: "turn-1".to_string(),
            num_turns: None,
        })
        .unwrap();

    assert_eq!(restored.restored_checkpoint.id, first.id);
    assert_eq!(
        fs::read_to_string(workspace.join("tracked.txt")).unwrap(),
        "baseline\n"
    );
    assert!(!workspace.join("created-by-first.txt").exists());
    assert!(!workspace.join("created-by-second.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.join("ignored.txt")).unwrap(),
        "ignored current value\n"
    );
    assert!(!restored.partial);
    assert_eq!(git_stdout(&workspace, ["rev-parse", "HEAD"]), project_head);
    assert!(
        run_project_git(&workspace, ["diff", "--cached", "--quiet"])
            .status
            .success()
    );
    assert!(checkpoint_root.is_dir());
    assert!(!workspace.join(".codex-elves-checkpoint").exists());

    service
        .restore_checkpoint(RestoreCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: restored.safety_checkpoint.id,
            thread_id: "thread-1".to_string(),
        })
        .unwrap();
    assert_eq!(
        fs::read_to_string(workspace.join("tracked.txt")).unwrap(),
        "after second AI turn\n"
    );
    assert!(workspace.join("created-by-first.txt").is_file());
    assert!(workspace.join("created-by-second.txt").is_file());
    assert_eq!(git_stdout(&workspace, ["rev-parse", "HEAD"]), project_head);
}

#[test]
fn checkpoint_requests_are_idempotent_and_rollback_count_selects_the_target_turn() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "zero").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let first = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "first");
    let duplicate = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-1".to_string(),
            prompt_preview: "duplicate".to_string(),
        })
        .unwrap()
        .checkpoint;
    assert_eq!(duplicate.id, first.id);

    fs::write(workspace.join("value.txt"), "one").unwrap();
    create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "second");
    fs::write(workspace.join("value.txt"), "two").unwrap();
    create_bound_checkpoint(&service, &workspace, "request-3", "turn-3", "third");
    fs::write(workspace.join("value.txt"), "three").unwrap();

    let restored = service
        .restore_for_revert(RestoreForRevertRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            before_turn_id: String::new(),
            num_turns: Some(2),
        })
        .unwrap();

    assert_eq!(
        restored.restored_checkpoint.turn_id.as_deref(),
        Some("turn-2")
    );
    assert_eq!(
        fs::read_to_string(workspace.join("value.txt")).unwrap(),
        "one"
    );
}

#[test]
fn completed_turn_records_changes_on_the_same_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    fs::write(workspace.join("deleted.txt"), "delete me\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "first");
    assert_eq!(
        checkpoint.change_scope,
        WorkspaceCheckpointChangeScope::Turn
    );
    assert_eq!(checkpoint.turn_status, None);
    assert_eq!(checkpoint.changed_file_count, 0);
    assert!(checkpoint.changed_files.is_empty());

    fs::write(workspace.join("value.txt"), "after\n").unwrap();
    fs::write(workspace.join("created.txt"), "created\n").unwrap();
    fs::remove_file(workspace.join("deleted.txt")).unwrap();

    let completed = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap()
        .checkpoint;

    assert_eq!(completed.id, checkpoint.id);
    assert_eq!(
        completed.turn_status,
        Some(WorkspaceCheckpointTurnStatus::Completed)
    );
    assert!(completed.completed_at_ms.is_some());
    assert_eq!(completed.changed_file_count, 3);
    let changes = completed
        .changed_files
        .iter()
        .map(|change| (change.path.as_str(), change))
        .collect::<BTreeMap<_, _>>();

    let modified = changes["value.txt"];
    assert_eq!(modified.status, "M");
    assert_eq!(modified.additions, Some(1));
    assert_eq!(modified.deletions, Some(1));

    let created = changes["created.txt"];
    assert_eq!(created.status, "A");
    assert_eq!(created.additions, Some(1));
    assert_eq!(created.deletions, Some(0));

    let deleted = changes["deleted.txt"];
    assert_eq!(deleted.status, "D");
    assert_eq!(deleted.additions, Some(0));
    assert_eq!(deleted.deletions, Some(1));

    let next = create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "second");
    assert_eq!(next.turn_status, None);
    assert_eq!(next.changed_file_count, 0);
    assert!(next.changed_files.is_empty());

    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(2),
        })
        .unwrap();
    assert_eq!(listed.checkpoints[0].id, next.id);
    assert!(listed.checkpoints[0].changed_files.is_empty());
    assert_eq!(listed.checkpoints[1].id, completed.id);
    assert_eq!(listed.checkpoints[1].changed_files, completed.changed_files);
}

#[test]
fn first_turn_checkpoint_is_an_initialization_summary_without_baseline_file_rows() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(workspace.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    fs::write(workspace.join("src").join("one.rs"), "one\n").unwrap();
    fs::write(workspace.join("src").join("two.rs"), "two\n").unwrap();
    fs::write(workspace.join("ignored.txt"), "ignored\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let first = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "initialize");
    assert!(first.initialization);
    assert_eq!(first.initial_file_count, Some(4));
    assert_eq!(first.changed_file_count, 0);
    assert!(first.changed_files.is_empty());

    fs::write(workspace.join("value.txt"), "after\n").unwrap();
    let completed = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap()
        .checkpoint;
    assert!(completed.initialization);
    assert_eq!(completed.initial_file_count, Some(4));
    assert_eq!(completed.changed_file_count, 1);

    let second = create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "second");
    assert!(!second.initialization);
    assert_eq!(second.initial_file_count, None);

    let other_thread = create_bound_checkpoint_for_thread(
        &service,
        &workspace,
        "thread-2",
        "request-other",
        "turn-other",
        "other thread",
    );
    assert!(other_thread.initialization);
    assert_eq!(other_thread.initial_file_count, Some(4));
}

#[test]
fn completing_a_turn_twice_keeps_the_first_terminal_result() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "first");
    fs::write(workspace.join("value.txt"), "first result\n").unwrap();
    let failed = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Failed,
        })
        .unwrap()
        .checkpoint;

    fs::write(workspace.join("value.txt"), "later result\n").unwrap();
    fs::write(workspace.join("later.txt"), "later\n").unwrap();
    let duplicate = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap()
        .checkpoint;

    assert_eq!(duplicate.id, checkpoint.id);
    assert_eq!(duplicate, failed);
    assert_eq!(
        duplicate.turn_status,
        Some(WorkspaceCheckpointTurnStatus::Failed)
    );
    assert_eq!(duplicate.changed_file_count, 1);
    assert_eq!(duplicate.changed_files[0].path, "value.txt");
}

#[test]
fn legacy_checkpoint_records_without_turn_fields_remain_readable() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "legacy");
    let summary = service.management_summary().unwrap();
    let events_path = Path::new(&summary.workspaces[0].storage_path).join("events.jsonl");
    let rewritten = fs::read_to_string(&events_path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut event: serde_json::Value = serde_json::from_str(line).unwrap();
            if event["event"] == "created" && event["checkpoint"]["id"] == checkpoint.id {
                let record = event["checkpoint"].as_object_mut().unwrap();
                record.remove("initialization");
                record.remove("initialFileCount");
                record.remove("changeScope");
                record.remove("turnStatus");
                record.remove("completedAtMs");
                record.insert("changedFileCount".to_string(), serde_json::json!(1));
                record.insert(
                    "changedFiles".to_string(),
                    serde_json::json!([{
                        "path": "legacy.txt",
                        "status": "M",
                        "additions": 1,
                        "deletions": 0
                    }]),
                );
            }
            serde_json::to_string(&event).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&events_path, format!("{rewritten}\n")).unwrap();

    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(listed.checkpoints.len(), 1);
    assert_eq!(
        listed.checkpoints[0].change_scope,
        WorkspaceCheckpointChangeScope::LegacyBeforeTurn
    );
    assert!(!listed.checkpoints[0].initialization);
    assert_eq!(listed.checkpoints[0].initial_file_count, None);
    assert_eq!(listed.checkpoints[0].turn_status, None);
    assert_eq!(listed.checkpoints[0].changed_files[0].path, "legacy.txt");
}

#[test]
fn checkpoint_store_inside_workspace_is_excluded_from_its_own_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "one").unwrap();
    let checkpoint_root = workspace.join(".codex-elves-state");
    let service = WorkspaceCheckpointService::new(checkpoint_root.clone());

    create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "");
    fs::write(workspace.join("value.txt"), "two").unwrap();
    let first = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap()
        .checkpoint;
    assert_eq!(first.changed_file_count, 1);
    assert_eq!(first.changed_files[0].path, "value.txt");

    create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "");
    let second = service
        .complete_turn(CompleteTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
            status: WorkspaceCheckpointTurnStatus::Completed,
        })
        .unwrap()
        .checkpoint;
    assert_eq!(second.changed_file_count, 0);
    assert!(second.changed_files.is_empty());
    assert!(checkpoint_root.is_dir());
}

#[test]
fn list_includes_turn_and_restore_safety_checkpoints_in_reverse_time_order() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "one").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));
    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "prompt");
    fs::write(workspace.join("value.txt"), "two").unwrap();
    service
        .restore_checkpoint(RestoreCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: checkpoint.id,
            thread_id: "thread-1".to_string(),
        })
        .unwrap();

    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(listed.checkpoints.len(), 2);
    assert_eq!(
        listed.checkpoints[0].kind,
        WorkspaceCheckpointKind::RestoreSafety
    );
    assert_eq!(
        listed.checkpoints[1].kind,
        WorkspaceCheckpointKind::TurnStart
    );
}

#[test]
fn checkpoint_preserves_raw_line_endings_despite_project_attributes() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join(".gitattributes"), "*.txt text eol=lf\n").unwrap();
    fs::write(workspace.join("value.txt"), b"before\r\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));
    let checkpoint = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-1".to_string(),
            prompt_preview: String::new(),
        })
        .unwrap()
        .checkpoint;

    fs::write(workspace.join("value.txt"), b"after\r\n").unwrap();
    service
        .restore_checkpoint(RestoreCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: checkpoint.id,
            thread_id: "thread-1".to_string(),
        })
        .unwrap();

    assert_eq!(
        fs::read(workspace.join("value.txt")).unwrap(),
        b"before\r\n"
    );
}

#[test]
fn preview_revert_reports_current_workspace_changes_without_mutating_history() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));
    create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "prompt");

    let unchanged = service
        .preview_revert(RestoreForRevertRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            before_turn_id: String::new(),
            num_turns: Some(1),
        })
        .unwrap();
    assert!(!unchanged.has_changes);
    assert!(unchanged.changed_paths.is_empty());

    fs::write(workspace.join("value.txt"), "after\n").unwrap();
    fs::write(workspace.join("created.txt"), "created\n").unwrap();
    fs::write(workspace.join("ignored.txt"), "ignored\n").unwrap();

    let changed = service
        .preview_revert(RestoreForRevertRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            before_turn_id: String::new(),
            num_turns: Some(1),
        })
        .unwrap();
    assert!(changed.has_changes);
    assert_eq!(
        changed.changed_paths.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["created.txt".to_string(), "value.txt".to_string()])
    );

    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(listed.checkpoints.len(), 1);
    assert_eq!(
        listed.checkpoints[0].kind,
        WorkspaceCheckpointKind::TurnStart
    );
    assert_eq!(
        fs::read_to_string(workspace.join("value.txt")).unwrap(),
        "after\n"
    );
    assert!(workspace.join("created.txt").is_file());
    assert!(workspace.join("ignored.txt").is_file());
}

#[test]
fn retention_is_applied_per_bound_thread_after_the_new_turn_is_durable() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "zero").unwrap();
    let service =
        WorkspaceCheckpointService::new(temp.path().join("state")).with_retention_rounds(2);

    let first = create_bound_checkpoint_for_thread(
        &service,
        &workspace,
        "thread-a",
        "request-a1",
        "turn-a1",
        "first",
    );
    fs::write(workspace.join("value.txt"), "one").unwrap();
    let second = create_bound_checkpoint_for_thread(
        &service,
        &workspace,
        "thread-a",
        "request-a2",
        "turn-a2",
        "second",
    );
    fs::write(workspace.join("value.txt"), "two").unwrap();
    let third_pending = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-a".to_string(),
            request_id: "request-a3".to_string(),
            prompt_preview: "third".to_string(),
        })
        .unwrap()
        .checkpoint;

    let before_bind = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-a".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(before_bind.checkpoints.len(), 3);
    assert!(
        before_bind
            .checkpoints
            .iter()
            .any(|item| item.id == first.id)
    );

    let third = service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: third_pending.id.clone(),
            thread_id: "thread-a".to_string(),
            turn_id: "turn-a3".to_string(),
        })
        .unwrap();
    assert_eq!(third.checkpoint.id, third_pending.id);
    assert!(third.checkpoint.accepted);
    assert_eq!(third.pruned_count, 1);

    let thread_a = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-a".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(thread_a.checkpoints.len(), 2);
    assert!(!thread_a.checkpoints.iter().any(|item| item.id == first.id));
    assert!(thread_a.checkpoints.iter().any(|item| item.id == second.id));
    assert!(
        thread_a
            .checkpoints
            .iter()
            .any(|item| item.id == third.checkpoint.id)
    );

    fs::write(workspace.join("value.txt"), "pending").unwrap();
    let pending = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-a".to_string(),
            request_id: "request-a4".to_string(),
            prompt_preview: "pending".to_string(),
        })
        .unwrap()
        .checkpoint;
    assert!(!pending.accepted);

    for (index, value) in ["b-zero", "b-one", "b-two"].into_iter().enumerate() {
        fs::write(workspace.join("value.txt"), value).unwrap();
        create_bound_checkpoint_for_thread(
            &service,
            &workspace,
            "thread-b",
            &format!("request-b{}", index + 1),
            &format!("turn-b{}", index + 1),
            value,
        );
    }

    let thread_a = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-a".to_string(),
            limit: Some(10),
        })
        .unwrap();
    let thread_b = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-b".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(
        thread_a
            .checkpoints
            .iter()
            .filter(|item| item.accepted)
            .count(),
        2
    );
    assert_eq!(
        thread_a
            .checkpoints
            .iter()
            .filter(|item| !item.accepted)
            .count(),
        1
    );
    assert_eq!(thread_b.checkpoints.len(), 2);

    let summary = service.management_summary().unwrap();
    assert_eq!(summary.thread_count, 2);
    assert_eq!(summary.turn_count, 4);
    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.retention_rounds, 2);
}

#[test]
fn binding_can_attach_a_pending_checkpoint_to_its_thread() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    let pending = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: String::new(),
            request_id: "request-1".to_string(),
            prompt_preview: "prompt".to_string(),
        })
        .unwrap()
        .checkpoint;
    let bound = service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: pending.id.clone(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
        })
        .unwrap()
        .checkpoint;

    assert_eq!(bound.thread_id, "thread-1");
    assert!(bound.initialization);
    assert_eq!(bound.initial_file_count, Some(1));
    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(10),
        })
        .unwrap();
    assert_eq!(listed.checkpoints.len(), 1);
    assert_eq!(listed.checkpoints[0].id, pending.id);
}

#[test]
fn cleanup_expires_pending_checkpoints_and_keeps_only_three_safety_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "baseline").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));
    let original = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "prompt");

    for index in 0..4 {
        fs::write(workspace.join("value.txt"), format!("changed-{index}")).unwrap();
        service
            .restore_checkpoint(RestoreCheckpointRequest {
                cwd: workspace.to_string_lossy().into_owned(),
                checkpoint_id: original.id.clone(),
                thread_id: "thread-1".to_string(),
            })
            .unwrap();
    }

    let pending = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "pending-request".to_string(),
            prompt_preview: "pending".to_string(),
        })
        .unwrap()
        .checkpoint;
    let summary = service.management_summary().unwrap();
    let workspace_state = Path::new(&summary.workspaces[0].storage_path);
    let events_path = workspace_state.join("events.jsonl");
    let rewritten = fs::read_to_string(&events_path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut event: serde_json::Value = serde_json::from_str(line).unwrap();
            if event["event"] == "created" && event["checkpoint"]["id"] == pending.id {
                event["checkpoint"]["createdAtMs"] = serde_json::json!(0);
            }
            serde_json::to_string(&event).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&events_path, format!("{rewritten}\n")).unwrap();

    let maintenance = service.cleanup_storage().unwrap();
    assert_eq!(maintenance.deleted_checkpoints, 1);
    assert_eq!(maintenance.summary.turn_count, 1);
    assert_eq!(maintenance.summary.safety_count, 3);
    assert_eq!(maintenance.summary.pending_count, 0);
    assert_eq!(maintenance.summary.checkpoint_count, 4);
}

#[test]
fn compact_storage_reclaims_pruned_parentless_checkpoint_commits() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "zero").unwrap();
    let service =
        WorkspaceCheckpointService::new(temp.path().join("state")).with_retention_rounds(2);

    let first = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "first");
    fs::write(workspace.join("value.txt"), "one").unwrap();
    create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "second");
    fs::write(workspace.join("value.txt"), "two").unwrap();
    let third = create_bound_checkpoint(&service, &workspace, "request-3", "turn-3", "third");

    let summary = service.management_summary().unwrap();
    let git_dir = Path::new(&summary.workspaces[0].storage_path)
        .join("repository")
        .join(".git");
    let parents = shadow_git_stdout(
        &git_dir,
        [
            "rev-list",
            "--parents",
            "-n",
            "1",
            third.commit_hash.as_str(),
        ],
    );
    assert_eq!(parents.split_whitespace().count(), 1);

    let compacted = service.compact_storage().unwrap();
    assert_eq!(compacted.compacted_workspaces, 1);
    assert_eq!(compacted.summary.turn_count, 2);
    let object = format!("{}^{{commit}}", first.commit_hash);
    assert!(
        !run_shadow_git(&git_dir, ["cat-file", "-e", object.as_str()])
            .status
            .success()
    );
}

#[test]
fn management_delete_scopes_and_storage_migration_preserve_recoverability() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let source = temp.path().join("source-state");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before").unwrap();
    let service = WorkspaceCheckpointService::new(source.clone());
    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "prompt");

    let nested_target = source.join("nested-target");
    assert!(
        service
            .migrate_storage(nested_target.clone(), |_| Ok(()))
            .is_err()
    );
    assert!(!nested_target.exists());

    let failed_target = temp.path().join("failed-target");
    fs::create_dir_all(&failed_target).unwrap();
    assert!(
        service
            .migrate_storage(failed_target.clone(), |_| anyhow::bail!("save failed"))
            .is_err()
    );
    assert!(failed_target.is_dir());
    assert!(fs::read_dir(&failed_target).unwrap().next().is_none());
    assert_eq!(
        service
            .list_checkpoints(ListCheckpointsRequest {
                cwd: workspace.to_string_lossy().into_owned(),
                thread_id: "thread-1".to_string(),
                limit: Some(10),
            })
            .unwrap()
            .checkpoints[0]
            .id,
        checkpoint.id
    );

    let target = temp.path().join("migrated-state");
    let mut committed_target = None;
    let migrated = service
        .migrate_storage(target, |resolved| {
            committed_target = Some(resolved.to_path_buf());
            Ok(())
        })
        .unwrap();
    assert_eq!(committed_target.as_deref(), Some(migrated.as_path()));
    assert!(!source.exists());

    let migrated_service = WorkspaceCheckpointService::new(migrated);
    let summary = migrated_service.management_summary().unwrap();
    assert_eq!(summary.workspace_count, 1);
    assert_eq!(summary.thread_count, 1);
    assert_eq!(summary.checkpoint_count, 1);
    assert!(summary.total_bytes > 0);
    let workspace_key = summary.workspaces[0].key.clone();

    let deleted = migrated_service
        .delete_data(DeleteWorkspaceCheckpointDataRequest {
            scope: "checkpoint".to_string(),
            workspace_key: workspace_key.clone(),
            thread_id: String::new(),
            checkpoint_id: checkpoint.id,
        })
        .unwrap();
    assert_eq!(deleted.deleted_checkpoints, 1);
    assert_eq!(deleted.summary.checkpoint_count, 0);

    let deleted_workspace = migrated_service
        .delete_data(DeleteWorkspaceCheckpointDataRequest {
            scope: "workspace".to_string(),
            workspace_key,
            thread_id: String::new(),
            checkpoint_id: String::new(),
        })
        .unwrap();
    assert_eq!(deleted_workspace.summary.workspace_count, 0);
}

fn create_bound_checkpoint(
    service: &WorkspaceCheckpointService,
    workspace: &Path,
    request_id: &str,
    turn_id: &str,
    prompt: &str,
) -> codex_elves_core::workspace_checkpoint::WorkspaceCheckpoint {
    create_bound_checkpoint_for_thread(service, workspace, "thread-1", request_id, turn_id, prompt)
}

fn create_bound_checkpoint_for_thread(
    service: &WorkspaceCheckpointService,
    workspace: &Path,
    thread_id: &str,
    request_id: &str,
    turn_id: &str,
    prompt: &str,
) -> codex_elves_core::workspace_checkpoint::WorkspaceCheckpoint {
    let checkpoint = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: thread_id.to_string(),
            request_id: request_id.to_string(),
            prompt_preview: prompt.to_string(),
        })
        .unwrap()
        .checkpoint;
    service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: checkpoint.id.clone(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
        })
        .unwrap()
        .checkpoint
}

fn init_project_git(workspace: &Path) {
    run_project_git(workspace, ["init", "--quiet"]);
    run_project_git(workspace, ["config", "user.name", "Checkpoint Test"]);
    run_project_git(
        workspace,
        ["config", "user.email", "checkpoint-test@example.invalid"],
    );
}

fn git_stdout<const N: usize>(workspace: &Path, args: [&str; N]) -> String {
    let output = run_project_git(workspace, args);
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run_project_git<I, S>(workspace: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("git should be available for checkpoint tests")
}

fn shadow_git_stdout<const N: usize>(git_dir: &Path, args: [&str; N]) -> String {
    let output = run_shadow_git(git_dir, args);
    assert!(
        output.status.success(),
        "shadow git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn run_shadow_git<I, S>(git_dir: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .output()
        .expect("git should be available for checkpoint tests")
}
