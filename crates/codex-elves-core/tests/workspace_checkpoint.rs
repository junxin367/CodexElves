use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use codex_elves_core::workspace_checkpoint::{
    BindTurnRequest, CreateCheckpointRequest, ListCheckpointsRequest, RestoreCheckpointRequest,
    RestoreForRevertRequest, WorkspaceCheckpointKind, WorkspaceCheckpointService,
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
fn checkpoint_records_per_file_status_and_line_changes() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "before\n").unwrap();
    fs::write(workspace.join("deleted.txt"), "delete me\n").unwrap();
    let service = WorkspaceCheckpointService::new(temp.path().join("state"));

    create_bound_checkpoint(&service, &workspace, "request-1", "turn-1", "first");
    fs::write(workspace.join("value.txt"), "after\n").unwrap();
    fs::write(workspace.join("created.txt"), "created\n").unwrap();
    fs::remove_file(workspace.join("deleted.txt")).unwrap();

    let checkpoint = create_bound_checkpoint(&service, &workspace, "request-2", "turn-2", "second");
    assert_eq!(checkpoint.changed_file_count, 3);
    let changes = checkpoint
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

    let listed = service
        .list_checkpoints(ListCheckpointsRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            limit: Some(1),
        })
        .unwrap();
    assert_eq!(
        listed.checkpoints[0].changed_files,
        checkpoint.changed_files
    );
}

#[test]
fn checkpoint_store_inside_workspace_is_excluded_from_its_own_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(workspace.join("value.txt"), "one").unwrap();
    let checkpoint_root = workspace.join(".codex-elves-state");
    let service = WorkspaceCheckpointService::new(checkpoint_root.clone());

    let first = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-1".to_string(),
            prompt_preview: String::new(),
        })
        .unwrap()
        .checkpoint;
    fs::write(workspace.join("value.txt"), "two").unwrap();
    let second = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: "request-2".to_string(),
            prompt_preview: String::new(),
        })
        .unwrap()
        .checkpoint;

    assert_eq!(first.changed_file_count, 1);
    assert_eq!(second.changed_file_count, 1);
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

fn create_bound_checkpoint(
    service: &WorkspaceCheckpointService,
    workspace: &Path,
    request_id: &str,
    turn_id: &str,
    prompt: &str,
) -> codex_elves_core::workspace_checkpoint::WorkspaceCheckpoint {
    let checkpoint = service
        .create_checkpoint(CreateCheckpointRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            thread_id: "thread-1".to_string(),
            request_id: request_id.to_string(),
            prompt_preview: prompt.to_string(),
        })
        .unwrap()
        .checkpoint;
    service
        .bind_turn(BindTurnRequest {
            cwd: workspace.to_string_lossy().into_owned(),
            checkpoint_id: checkpoint.id.clone(),
            thread_id: "thread-1".to_string(),
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
