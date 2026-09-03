use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const CREATE_PATH: &str = "/workspace-checkpoint/create";
pub const BIND_TURN_PATH: &str = "/workspace-checkpoint/bind-turn";
pub const COMPLETE_TURN_PATH: &str = "/workspace-checkpoint/complete-turn";
pub const LIST_PATH: &str = "/workspace-checkpoint/list";
pub const RESTORE_PATH: &str = "/workspace-checkpoint/restore";
pub const PREVIEW_REVERT_PATH: &str = "/workspace-checkpoint/preview-revert";
pub const RESTORE_FOR_REVERT_PATH: &str = "/workspace-checkpoint/restore-for-revert";

const SCHEMA_VERSION: u32 = 1;
const STORAGE_VERSION: u32 = 2;
const MAX_LIST_LIMIT: usize = 200;
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_PROMPT_PREVIEW_CHARS: usize = 280;
const MAX_GIT_ERROR_CHARS: usize = 2_000;
const MAX_RESTORE_SAFETY_CHECKPOINTS_PER_THREAD: usize = 3;
const PENDING_CHECKPOINT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const ROOT_LOCK_FILE: &str = ".checkpoint-root.lock";
const CHECKPOINT_REF_PREFIX: &str = "refs/codexelves/checkpoints/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceCheckpointKind {
    TurnStart,
    RestoreSafety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceCheckpointChangeScope {
    LegacyBeforeTurn,
    Turn,
    Snapshot,
}

impl Default for WorkspaceCheckpointChangeScope {
    fn default() -> Self {
        Self::LegacyBeforeTurn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceCheckpointTurnStatus {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpointFileChange {
    pub path: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpoint {
    pub schema_version: u32,
    pub id: String,
    pub request_id: String,
    pub workspace: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub commit_hash: String,
    pub created_at_ms: u64,
    pub prompt_preview: String,
    pub kind: WorkspaceCheckpointKind,
    pub accepted: bool,
    #[serde(default)]
    pub initialization: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_file_count: Option<usize>,
    #[serde(default)]
    pub change_scope: WorkspaceCheckpointChangeScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_status: Option<WorkspaceCheckpointTurnStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    pub changed_file_count: usize,
    #[serde(default)]
    pub changed_files: Vec<WorkspaceCheckpointFileChange>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCheckpointRequest {
    pub cwd: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub prompt_preview: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindTurnRequest {
    pub cwd: String,
    pub checkpoint_id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteTurnRequest {
    pub cwd: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
    pub status: WorkspaceCheckpointTurnStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCheckpointsRequest {
    pub cwd: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreCheckpointRequest {
    pub cwd: String,
    pub checkpoint_id: String,
    #[serde(default)]
    pub thread_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreForRevertRequest {
    pub cwd: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub before_turn_id: String,
    #[serde(default)]
    pub num_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCheckpointResult {
    pub checkpoint: WorkspaceCheckpoint,
    #[serde(default)]
    pub pruned_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteTurnResult {
    pub checkpoint: WorkspaceCheckpoint,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCheckpointsResult {
    pub workspace: String,
    pub checkpoints: Vec<WorkspaceCheckpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreCheckpointResult {
    pub workspace: String,
    pub restored_checkpoint: WorkspaceCheckpoint,
    pub safety_checkpoint: WorkspaceCheckpoint,
    pub changed_paths: Vec<String>,
    pub partial: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRevertResult {
    pub workspace: String,
    pub checkpoint: WorkspaceCheckpoint,
    pub changed_paths: Vec<String>,
    pub has_changes: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpointThreadSummary {
    pub thread_id: String,
    pub checkpoint_count: usize,
    pub turn_count: usize,
    pub safety_count: usize,
    pub pending_count: usize,
    pub last_activity_ms: u64,
    pub checkpoints: Vec<WorkspaceCheckpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpointWorkspaceSummary {
    pub key: String,
    pub workspace: String,
    pub storage_path: String,
    pub bytes: u64,
    pub checkpoint_count: usize,
    pub turn_count: usize,
    pub safety_count: usize,
    pub pending_count: usize,
    pub last_activity_ms: u64,
    pub threads: Vec<WorkspaceCheckpointThreadSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpointManagementSummary {
    pub root: String,
    pub total_bytes: u64,
    pub workspace_count: usize,
    pub thread_count: usize,
    pub checkpoint_count: usize,
    pub turn_count: usize,
    pub safety_count: usize,
    pub pending_count: usize,
    pub retention_rounds: u16,
    pub workspaces: Vec<WorkspaceCheckpointWorkspaceSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCheckpointMaintenanceResult {
    pub deleted_checkpoints: usize,
    pub compacted_workspaces: usize,
    pub reclaimed_bytes: u64,
    pub summary: WorkspaceCheckpointManagementSummary,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkspaceCheckpointDataRequest {
    pub scope: String,
    #[serde(default)]
    pub workspace_key: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub checkpoint_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum CheckpointEvent {
    Created {
        checkpoint: WorkspaceCheckpoint,
    },
    Bound {
        checkpoint_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
    Initialized {
        checkpoint_id: String,
        initial_file_count: usize,
    },
    Completed {
        checkpoint_id: String,
        status: WorkspaceCheckpointTurnStatus,
        completed_at_ms: u64,
        #[serde(default)]
        changed_files: Vec<WorkspaceCheckpointFileChange>,
    },
}

#[derive(Debug, Clone)]
pub struct WorkspaceCheckpointService {
    root: PathBuf,
    retention_rounds: Option<usize>,
    dynamic_settings: bool,
}

impl Default for WorkspaceCheckpointService {
    fn default() -> Self {
        Self {
            root: crate::paths::default_workspace_checkpoints_dir(),
            retention_rounds: Some(usize::from(
                crate::settings::default_workspace_checkpoint_retention_rounds(),
            )),
            dynamic_settings: true,
        }
    }
}

impl WorkspaceCheckpointService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            retention_rounds: Some(usize::from(
                crate::settings::default_workspace_checkpoint_retention_rounds(),
            )),
            dynamic_settings: false,
        }
    }

    pub fn with_retention_rounds(mut self, retention_rounds: u16) -> Self {
        self.retention_rounds = (retention_rounds > 0).then_some(usize::from(retention_rounds));
        self
    }

    pub fn for_settings(
        &self,
        settings: &crate::settings::BackendSettings,
    ) -> anyhow::Result<Self> {
        if !self.dynamic_settings {
            return Ok(self.clone());
        }
        let root = configured_root(settings)?;
        Ok(Self::new(root)
            .with_retention_rounds(settings.codex_app_workspace_checkpoint_retention_rounds))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn retention_rounds(&self) -> Option<usize> {
        self.retention_rounds
    }

    pub fn create_checkpoint(
        &self,
        request: CreateCheckpointRequest,
    ) -> anyhow::Result<CreateCheckpointResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;
        let pruned_count = self.cleanup_workspace_locked(&workspace, &state, false)?;

        let request_id = non_empty_or_uuid(&request.request_id);
        let thread_id = request.thread_id.trim().to_string();
        let checkpoints = self.load_checkpoints(&state)?;
        let existing = checkpoints
            .iter()
            .find(|checkpoint| {
                checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                    && checkpoint.thread_id == thread_id
                    && checkpoint.request_id == request_id
            })
            .cloned();
        if let Some(checkpoint) = existing {
            return Ok(CreateCheckpointResult {
                checkpoint,
                pruned_count,
            });
        }
        let initialization = !thread_id.is_empty()
            && !checkpoints.iter().any(|checkpoint| {
                checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                    && checkpoint.thread_id == thread_id
            });

        let checkpoint = self.snapshot_locked(
            &workspace,
            &state,
            SnapshotMetadata {
                id: Uuid::new_v4().hyphenated().to_string(),
                request_id,
                thread_id,
                prompt_preview: truncate_chars(
                    request.prompt_preview.trim(),
                    MAX_PROMPT_PREVIEW_CHARS,
                ),
                kind: WorkspaceCheckpointKind::TurnStart,
                accepted: false,
                initialization,
                change_scope: WorkspaceCheckpointChangeScope::Turn,
            },
        )?;
        Ok(CreateCheckpointResult {
            checkpoint,
            pruned_count,
        })
    }

    pub fn bind_turn(&self, request: BindTurnRequest) -> anyhow::Result<CreateCheckpointResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;
        let checkpoints = self.load_checkpoints(&state)?;
        let checkpoint = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == request.checkpoint_id.trim())
            .cloned()
            .ok_or_else(|| anyhow!("未找到待绑定的工作区 Checkpoint"))?;
        validate_thread_scope(&checkpoint, &request.thread_id)?;

        let thread_id = optional_trimmed(&request.thread_id)
            .or_else(|| optional_trimmed(&checkpoint.thread_id));
        let turn_id = optional_trimmed(&request.turn_id);
        let initialize_on_bind = !checkpoint.initialization
            && checkpoint.thread_id.trim().is_empty()
            && thread_id.as_ref().is_some_and(|thread_id| {
                !checkpoints.iter().any(|candidate| {
                    candidate.id != checkpoint.id
                        && candidate.kind == WorkspaceCheckpointKind::TurnStart
                        && candidate.thread_id == *thread_id
                })
            });
        if initialize_on_bind {
            self.verify_commit(&state, &workspace, &checkpoint.commit_hash)?;
            self.append_event(
                &state,
                &CheckpointEvent::Initialized {
                    checkpoint_id: checkpoint.id.clone(),
                    initial_file_count: self.commit_file_count(
                        &state,
                        &workspace,
                        &checkpoint.commit_hash,
                    )?,
                },
            )?;
        }
        if !checkpoint.accepted
            || checkpoint.thread_id != thread_id.as_deref().unwrap_or_default()
            || checkpoint.turn_id != turn_id
        {
            self.append_event(
                &state,
                &CheckpointEvent::Bound {
                    checkpoint_id: checkpoint.id.clone(),
                    thread_id,
                    turn_id,
                },
            )?;
        }
        let pruned_count = self.cleanup_workspace_locked(&workspace, &state, true)?;
        let checkpoint = self
            .load_checkpoints(&state)?
            .into_iter()
            .find(|candidate| candidate.id == checkpoint.id)
            .ok_or_else(|| anyhow!("Checkpoint 绑定后状态读取失败"))?;
        Ok(CreateCheckpointResult {
            checkpoint,
            pruned_count,
        })
    }

    pub fn complete_turn(
        &self,
        request: CompleteTurnRequest,
    ) -> anyhow::Result<CompleteTurnResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;

        let thread_id = request.thread_id.trim();
        if thread_id.is_empty() {
            bail!("完成工作区 Checkpoint 必须提供 threadId");
        }
        let turn_id = request.turn_id.trim();
        if turn_id.is_empty() {
            bail!("完成工作区 Checkpoint 必须提供 turnId");
        }

        let checkpoint = self
            .load_checkpoints(&state)?
            .into_iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                    && checkpoint.accepted
                    && checkpoint.thread_id == thread_id
                    && checkpoint.turn_id.as_deref() == Some(turn_id)
            })
            .ok_or_else(|| anyhow!("未找到与该轮次对应的工作区 Checkpoint"))?;
        if checkpoint.turn_status.is_some() {
            return Ok(CompleteTurnResult { checkpoint });
        }

        self.verify_commit(&state, &workspace, &checkpoint.commit_hash)?;
        let changed_files =
            self.worktree_file_changes(&state, &workspace, &checkpoint.commit_hash)?;
        self.append_event(
            &state,
            &CheckpointEvent::Completed {
                checkpoint_id: checkpoint.id.clone(),
                status: request.status,
                completed_at_ms: now_ms(),
                changed_files,
            },
        )?;
        let checkpoint = self
            .load_checkpoints(&state)?
            .into_iter()
            .find(|candidate| candidate.id == checkpoint.id)
            .ok_or_else(|| anyhow!("Checkpoint 完成后状态读取失败"))?;
        Ok(CompleteTurnResult { checkpoint })
    }

    pub fn list_checkpoints(
        &self,
        request: ListCheckpointsRequest,
    ) -> anyhow::Result<ListCheckpointsResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        let thread_id = request.thread_id.trim();
        let limit = request
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT);
        let checkpoints = self
            .load_checkpoints(&state)?
            .into_iter()
            .rev()
            .filter(|checkpoint| thread_id.is_empty() || checkpoint.thread_id == thread_id)
            .take(limit)
            .collect();
        Ok(ListCheckpointsResult {
            workspace: workspace_string(&workspace),
            checkpoints,
        })
    }

    pub fn restore_checkpoint(
        &self,
        request: RestoreCheckpointRequest,
    ) -> anyhow::Result<RestoreCheckpointResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;
        let checkpoint = self
            .load_checkpoints(&state)?
            .into_iter()
            .find(|checkpoint| checkpoint.id == request.checkpoint_id.trim())
            .ok_or_else(|| anyhow!("未找到要恢复的工作区 Checkpoint"))?;
        validate_thread_scope(&checkpoint, &request.thread_id)?;
        self.restore_locked(&workspace, &state, checkpoint)
    }

    pub fn restore_for_revert(
        &self,
        request: RestoreForRevertRequest,
    ) -> anyhow::Result<RestoreCheckpointResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;
        let checkpoint = self.checkpoint_for_revert(&state, &request)?;

        self.restore_locked(&workspace, &state, checkpoint)
    }

    pub fn preview_revert(
        &self,
        request: RestoreForRevertRequest,
    ) -> anyhow::Result<PreviewRevertResult> {
        let _root_lock = self.lock_root_shared()?;
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;
        let checkpoint = self.checkpoint_for_revert(&state, &request)?;
        self.verify_commit(&state, &workspace, &checkpoint.commit_hash)?;
        let changed_paths =
            self.worktree_changed_paths(&state, &workspace, &checkpoint.commit_hash)?;
        let has_changes = !changed_paths.is_empty();

        Ok(PreviewRevertResult {
            workspace: workspace_string(&workspace),
            checkpoint,
            changed_paths,
            has_changes,
        })
    }

    pub fn management_summary(&self) -> anyhow::Result<WorkspaceCheckpointManagementSummary> {
        let _root_lock = self.lock_root_shared()?;
        self.management_summary_locked()
    }

    pub fn cleanup_storage(&self) -> anyhow::Result<WorkspaceCheckpointMaintenanceResult> {
        let _root_lock = self.lock_root_shared()?;
        let before = directory_size(&self.root)?;
        let mut deleted_checkpoints = 0;
        for (_, state) in self.workspace_states()? {
            let workspace = workspace_path_for_state(&state);
            let _workspace_lock = self.lock_workspace(&state)?;
            deleted_checkpoints += self.cleanup_workspace_locked(&workspace, &state, true)?;
        }
        let summary = self.management_summary_locked()?;
        Ok(WorkspaceCheckpointMaintenanceResult {
            deleted_checkpoints,
            compacted_workspaces: 0,
            reclaimed_bytes: before.saturating_sub(summary.total_bytes),
            summary,
        })
    }

    pub fn compact_storage(&self) -> anyhow::Result<WorkspaceCheckpointMaintenanceResult> {
        let _root_lock = self.lock_root_shared()?;
        let before = directory_size(&self.root)?;
        let mut deleted_checkpoints = 0;
        let mut compacted_workspaces = 0;
        for (_, state) in self.workspace_states()? {
            let workspace = workspace_path_for_state(&state);
            let _workspace_lock = self.lock_workspace(&state)?;
            deleted_checkpoints += self.cleanup_workspace_locked(&workspace, &state, false)?;
            if state.git_dir.is_dir() {
                self.reconcile_checkpoint_refs_locked(&workspace, &state)?;
                self.run_git_checked(
                    &state,
                    &workspace,
                    ["reflog", "expire", "--expire=now", "--all"],
                    "清理 Checkpoint 引用日志",
                )?;
                self.run_git_checked(
                    &state,
                    &workspace,
                    ["gc", "--quiet", "--prune=now"],
                    "压缩 Checkpoint 存储",
                )?;
                compacted_workspaces += 1;
            }
        }
        let summary = self.management_summary_locked()?;
        Ok(WorkspaceCheckpointMaintenanceResult {
            deleted_checkpoints,
            compacted_workspaces,
            reclaimed_bytes: before.saturating_sub(summary.total_bytes),
            summary,
        })
    }

    pub fn delete_data(
        &self,
        request: DeleteWorkspaceCheckpointDataRequest,
    ) -> anyhow::Result<WorkspaceCheckpointMaintenanceResult> {
        let _root_lock = self.lock_root_shared()?;
        let before = directory_size(&self.root)?;
        let scope = request.scope.trim();
        let mut deleted_checkpoints = 0;
        match scope {
            "all" => {
                for (_, state) in self.workspace_states()? {
                    deleted_checkpoints += self.delete_workspace_state(state)?;
                }
            }
            "workspace" => {
                let state = self.state_for_management_key(&request.workspace_key)?;
                deleted_checkpoints += self.delete_workspace_state(state)?;
            }
            "thread" | "checkpoint" => {
                let state = self.state_for_management_key(&request.workspace_key)?;
                let workspace = workspace_path_for_state(&state);
                let _workspace_lock = self.lock_workspace(&state)?;
                let checkpoints = self.load_checkpoints(&state)?;
                let remaining = checkpoints
                    .iter()
                    .filter(|checkpoint| {
                        if scope == "thread" {
                            checkpoint.thread_id != request.thread_id.trim()
                        } else {
                            checkpoint.id != request.checkpoint_id.trim()
                        }
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                deleted_checkpoints = checkpoints.len().saturating_sub(remaining.len());
                if deleted_checkpoints > 0 {
                    self.replace_checkpoints_locked(&workspace, &state, &remaining)?;
                    self.run_git_checked(
                        &state,
                        &workspace,
                        ["gc", "--auto", "--quiet"],
                        "整理 Checkpoint 存储",
                    )?;
                }
            }
            _ => bail!("不支持的 Checkpoint 删除范围"),
        }
        let summary = self.management_summary_locked()?;
        Ok(WorkspaceCheckpointMaintenanceResult {
            deleted_checkpoints,
            compacted_workspaces: 0,
            reclaimed_bytes: before.saturating_sub(summary.total_bytes),
            summary,
        })
    }

    pub fn migrate_storage<F>(&self, target: PathBuf, commit_settings: F) -> anyhow::Result<PathBuf>
    where
        F: FnOnce(&Path) -> anyhow::Result<()>,
    {
        validate_storage_root_path(&target)?;
        if same_path(&self.root, &target) {
            commit_settings(&target)?;
            return Ok(target);
        }
        if path_is_within(&target, &self.root) || path_is_within(&self.root, &target) {
            bail!("新的 Checkpoint 储存目录不能与当前目录互相嵌套");
        }

        let root_lock = self.lock_root_exclusive()?;
        for (_, state) in self.workspace_states()? {
            if let Some(descriptor) = read_workspace_descriptor(&state.dir) {
                let workspace = PathBuf::from(descriptor.workspace);
                if path_is_within(&target, &workspace) {
                    bail!("Checkpoint 储存目录不能位于已管理的工作区内部");
                }
            }
        }
        let target_existed = target.exists();
        let target = prepare_storage_target(&target)?;
        if same_path(&self.root, &target) {
            commit_settings(&target)?;
            return Ok(target);
        }
        if path_is_within(&target, &self.root) || path_is_within(&self.root, &target) {
            if !target_existed {
                let _ = fs::remove_dir(&target);
            }
            bail!("新的 Checkpoint 储存目录不能与当前目录互相嵌套");
        }
        for (_, state) in self.workspace_states()? {
            if let Some(descriptor) = read_workspace_descriptor(&state.dir) {
                let workspace = PathBuf::from(descriptor.workspace);
                if path_is_within(&target, &workspace) {
                    if !target_existed {
                        let _ = fs::remove_dir(&target);
                    }
                    bail!("Checkpoint 储存目录不能位于已管理的工作区内部");
                }
            }
        }
        if fs::read_dir(&target)?.next().transpose()?.is_some() {
            bail!("新的 Checkpoint 储存目录必须为空");
        }

        let migration_result = (|| {
            copy_directory_contents(&self.root, &target, &[ROOT_LOCK_FILE])?;
            let source_bytes = directory_size_excluding(&self.root, &[ROOT_LOCK_FILE])?;
            let target_bytes = directory_size(&target)?;
            if source_bytes != target_bytes {
                bail!(
                    "Checkpoint 数据迁移校验失败：源目录 {source_bytes} 字节，目标目录 {target_bytes} 字节"
                );
            }
            commit_settings(&target)?;
            Ok(())
        })();
        if let Err(error) = migration_result {
            let _ = clear_directory_contents(&target, &[]);
            if !target_existed {
                let _ = fs::remove_dir(&target);
            }
            return Err(error);
        }

        let _ = clear_directory_contents(&self.root, &[ROOT_LOCK_FILE]);
        drop(root_lock);
        let _ = fs::remove_file(self.root.join(ROOT_LOCK_FILE));
        let _ = fs::remove_dir(&self.root);
        Ok(target)
    }

    fn management_summary_locked(&self) -> anyhow::Result<WorkspaceCheckpointManagementSummary> {
        let mut workspaces = Vec::new();
        let mut thread_count = 0;
        let mut checkpoint_count = 0;
        let mut turn_count = 0;
        let mut safety_count = 0;
        let mut pending_count = 0;

        for (key, state) in self.workspace_states()? {
            let _workspace_lock = self.lock_workspace(&state)?;
            let checkpoints = self.load_checkpoints(&state)?;
            let descriptor = read_workspace_descriptor(&state.dir);
            let workspace = descriptor
                .as_ref()
                .map(|descriptor| descriptor.workspace.clone())
                .or_else(|| {
                    checkpoints
                        .first()
                        .map(|checkpoint| checkpoint.workspace.clone())
                })
                .unwrap_or_else(|| "未知工作区".to_string());
            let mut grouped = BTreeMap::<String, Vec<WorkspaceCheckpoint>>::new();
            for checkpoint in checkpoints {
                grouped
                    .entry(checkpoint.thread_id.clone())
                    .or_default()
                    .push(checkpoint);
            }
            let mut threads = grouped
                .into_iter()
                .map(|(thread_id, mut checkpoints)| {
                    checkpoints.sort_by_key(|checkpoint| checkpoint.created_at_ms);
                    let turn_count = checkpoints
                        .iter()
                        .filter(|checkpoint| {
                            checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                                && checkpoint.accepted
                        })
                        .count();
                    let safety_count = checkpoints
                        .iter()
                        .filter(|checkpoint| {
                            checkpoint.kind == WorkspaceCheckpointKind::RestoreSafety
                        })
                        .count();
                    let pending_count = checkpoints
                        .iter()
                        .filter(|checkpoint| {
                            checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                                && !checkpoint.accepted
                        })
                        .count();
                    let last_activity_ms = checkpoints
                        .iter()
                        .map(|checkpoint| checkpoint.created_at_ms)
                        .max()
                        .unwrap_or_default();
                    checkpoints.reverse();
                    WorkspaceCheckpointThreadSummary {
                        thread_id,
                        checkpoint_count: checkpoints.len(),
                        turn_count,
                        safety_count,
                        pending_count,
                        last_activity_ms,
                        checkpoints,
                    }
                })
                .collect::<Vec<_>>();
            threads.sort_by_key(|thread| std::cmp::Reverse(thread.last_activity_ms));

            let workspace_checkpoint_count =
                threads.iter().map(|thread| thread.checkpoint_count).sum();
            let workspace_turn_count = threads.iter().map(|thread| thread.turn_count).sum();
            let workspace_safety_count = threads.iter().map(|thread| thread.safety_count).sum();
            let workspace_pending_count = threads.iter().map(|thread| thread.pending_count).sum();
            let last_activity_ms = threads
                .iter()
                .map(|thread| thread.last_activity_ms)
                .max()
                .unwrap_or_default();
            thread_count += threads
                .iter()
                .filter(|thread| !thread.thread_id.is_empty())
                .count();
            checkpoint_count += workspace_checkpoint_count;
            turn_count += workspace_turn_count;
            safety_count += workspace_safety_count;
            pending_count += workspace_pending_count;
            workspaces.push(WorkspaceCheckpointWorkspaceSummary {
                key,
                workspace,
                storage_path: state.dir.to_string_lossy().into_owned(),
                bytes: directory_size(&state.dir)?,
                checkpoint_count: workspace_checkpoint_count,
                turn_count: workspace_turn_count,
                safety_count: workspace_safety_count,
                pending_count: workspace_pending_count,
                last_activity_ms,
                threads,
            });
        }
        workspaces.sort_by_key(|workspace| std::cmp::Reverse(workspace.last_activity_ms));

        Ok(WorkspaceCheckpointManagementSummary {
            root: self.root.to_string_lossy().into_owned(),
            total_bytes: directory_size(&self.root)?,
            workspace_count: workspaces.len(),
            thread_count,
            checkpoint_count,
            turn_count,
            safety_count,
            pending_count,
            retention_rounds: self
                .retention_rounds
                .map(|value| value.min(usize::from(u16::MAX)) as u16)
                .unwrap_or_default(),
            workspaces,
        })
    }

    fn checkpoint_for_revert(
        &self,
        state: &WorkspaceState,
        request: &RestoreForRevertRequest,
    ) -> anyhow::Result<WorkspaceCheckpoint> {
        let thread_id = request.thread_id.trim();
        if thread_id.is_empty() {
            bail!("恢复 AI 修改前必须提供 threadId");
        }
        let checkpoints = self
            .load_checkpoints(&state)?
            .into_iter()
            .filter(|checkpoint| {
                checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                    && checkpoint.accepted
                    && checkpoint.thread_id == thread_id
            })
            .collect::<Vec<_>>();
        let before_turn_id = request.before_turn_id.trim();
        if !before_turn_id.is_empty() {
            checkpoints
                .iter()
                .find(|checkpoint| checkpoint.turn_id.as_deref() == Some(before_turn_id))
                .cloned()
        } else {
            None
        }
        .or_else(|| {
            let num_turns = request.num_turns?;
            if num_turns == 0 || num_turns > checkpoints.len() {
                return None;
            }
            checkpoints.get(checkpoints.len() - num_turns).cloned()
        })
        .ok_or_else(|| anyhow!("未找到与该用户消息对应的工作区 Checkpoint"))
    }

    fn restore_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
        checkpoint: WorkspaceCheckpoint,
    ) -> anyhow::Result<RestoreCheckpointResult> {
        self.verify_commit(state, workspace, &checkpoint.commit_hash)?;
        let safety_checkpoint = self.snapshot_locked(
            workspace,
            state,
            SnapshotMetadata {
                id: Uuid::new_v4().hyphenated().to_string(),
                request_id: Uuid::new_v4().hyphenated().to_string(),
                thread_id: checkpoint.thread_id.clone(),
                prompt_preview: "恢复历史 Checkpoint 前的安全快照".to_string(),
                kind: WorkspaceCheckpointKind::RestoreSafety,
                accepted: true,
                initialization: false,
                change_scope: WorkspaceCheckpointChangeScope::Snapshot,
            },
        )?;
        let changed_paths = self.changed_paths(
            state,
            workspace,
            &checkpoint.commit_hash,
            &safety_checkpoint.commit_hash,
        )?;

        let source = format!("--source={}", checkpoint.commit_hash);
        self.run_git_checked(
            state,
            workspace,
            [
                "restore",
                source.as_str(),
                "--staged",
                "--worktree",
                "--",
                ".",
            ],
            "恢复工作区文件",
        )?;
        self.run_git_checked(
            state,
            workspace,
            ["clean", "-fd", "--", "."],
            "清理 Checkpoint 之后新增的文件",
        )?;

        let worktree_clean = self
            .run_git(state, workspace, ["diff", "--quiet", "--", "."])?
            .status
            .success();
        let untracked = self.run_git_checked(
            state,
            workspace,
            [
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ],
            "检查恢复结果",
        )?;
        let partial = !worktree_clean || !untracked.stdout.is_empty();
        let mut warnings = if partial {
            vec!["部分嵌套仓库或并发写入的文件未能恢复；已保留恢复前安全 Checkpoint。".to_string()]
        } else {
            Vec::new()
        };
        if let Err(error) = self.cleanup_workspace_locked(workspace, state, true) {
            warnings.push(format!(
                "工作区已恢复，但自动清理旧 Checkpoint 失败：{error}"
            ));
        }

        Ok(RestoreCheckpointResult {
            workspace: workspace_string(workspace),
            restored_checkpoint: checkpoint,
            safety_checkpoint,
            changed_paths,
            partial,
            warnings,
        })
    }

    fn snapshot_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
        metadata: SnapshotMetadata,
    ) -> anyhow::Result<WorkspaceCheckpoint> {
        self.prepare_repository(workspace, state)?;
        self.run_git_checked(state, workspace, ["add", "-A", "--", "."], "收集工作区文件")?;
        let head = self.try_head(state, workspace)?;
        let snapshot_changed_files =
            if metadata.change_scope == WorkspaceCheckpointChangeScope::Snapshot {
                self.staged_file_changes(state, workspace)?
            } else {
                Vec::new()
            };
        let snapshot_changed = if head.is_none() {
            true
        } else if metadata.change_scope == WorkspaceCheckpointChangeScope::Snapshot {
            !snapshot_changed_files.is_empty()
        } else {
            self.staged_changes_present(state, workspace)?
        };
        let commit_hash = if head.is_some() && !snapshot_changed {
            head.unwrap_or_default()
        } else {
            let tree =
                self.run_git_checked(state, workspace, ["write-tree"], "写入 Checkpoint 文件树")?;
            let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();
            if tree.is_empty() {
                bail!("Git 未返回 Checkpoint 文件树");
            }
            let message = format!("CodexElves checkpoint {}", metadata.id);
            let commit = self.run_git_checked(
                state,
                workspace,
                ["commit-tree", tree.as_str(), "-m", message.as_str()],
                "创建工作区 Checkpoint",
            )?;
            let commit_hash = String::from_utf8_lossy(&commit.stdout).trim().to_string();
            if commit_hash.is_empty() {
                bail!("Checkpoint 提交创建成功但无法读取 commit");
            }
            self.run_git_checked(
                state,
                workspace,
                ["update-ref", "refs/heads/checkpoint", commit_hash.as_str()],
                "更新 Checkpoint 基线",
            )?;
            commit_hash
        };
        let initial_file_count = if metadata.initialization {
            Some(self.commit_file_count(state, workspace, &commit_hash)?)
        } else {
            None
        };
        let changed_files = if metadata.change_scope == WorkspaceCheckpointChangeScope::Turn {
            Vec::new()
        } else {
            snapshot_changed_files
        };
        let changed_file_count = changed_files.len();
        let checkpoint_ref = checkpoint_ref_name(&metadata.id);
        self.run_git_checked(
            state,
            workspace,
            ["update-ref", checkpoint_ref.as_str(), commit_hash.as_str()],
            "保存 Checkpoint 引用",
        )?;

        let checkpoint = WorkspaceCheckpoint {
            schema_version: SCHEMA_VERSION,
            id: metadata.id,
            request_id: metadata.request_id,
            workspace: workspace_string(workspace),
            thread_id: metadata.thread_id,
            turn_id: None,
            commit_hash,
            created_at_ms: now_ms(),
            prompt_preview: metadata.prompt_preview,
            kind: metadata.kind,
            accepted: metadata.accepted,
            initialization: metadata.initialization,
            initial_file_count,
            change_scope: metadata.change_scope,
            turn_status: None,
            completed_at_ms: None,
            changed_file_count,
            changed_files,
        };
        self.append_event(
            state,
            &CheckpointEvent::Created {
                checkpoint: checkpoint.clone(),
            },
        )?;
        Ok(checkpoint)
    }

    fn staged_file_changes(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
    ) -> anyhow::Result<Vec<WorkspaceCheckpointFileChange>> {
        let status_output = self.run_git_checked(
            state,
            workspace,
            [
                "diff",
                "--cached",
                "--name-status",
                "-z",
                "--no-renames",
                "--",
                ".",
            ],
            "读取 Checkpoint 文件状态",
        )?;
        let numstat_output = self.run_git_checked(
            state,
            workspace,
            [
                "diff",
                "--cached",
                "--numstat",
                "-z",
                "--no-renames",
                "--",
                ".",
            ],
            "统计 Checkpoint 文件增删",
        )?;
        parse_file_changes(&status_output.stdout, &numstat_output.stdout)
    }

    fn staged_changes_present(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
    ) -> anyhow::Result<bool> {
        let output = self.run_git(state, workspace, ["diff", "--cached", "--quiet", "--", "."])?;
        match output.status.code() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => {
                ensure_success(output, "检查 Checkpoint 文件变化")?;
                Ok(false)
            }
        }
    }

    fn commit_file_count(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        commit_hash: &str,
    ) -> anyhow::Result<usize> {
        let output = self.run_git_checked(
            state,
            workspace,
            ["ls-tree", "-r", "--name-only", "-z", commit_hash, "--"],
            "统计初始化 Checkpoint 文件",
        )?;
        Ok(split_nul(&output.stdout).count())
    }

    fn changed_paths(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        target: &str,
        current: &str,
    ) -> anyhow::Result<Vec<String>> {
        let output = self.run_git_checked(
            state,
            workspace,
            ["diff", "--name-only", "-z", target, current, "--", "."],
            "计算 Checkpoint 文件差异",
        )?;
        Ok(split_nul(&output.stdout)
            .map(|path| String::from_utf8_lossy(path).into_owned())
            .collect())
    }

    fn worktree_changed_paths(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        target: &str,
    ) -> anyhow::Result<Vec<String>> {
        let index_path = state.dir.join(format!(
            "preview-index-{}.index",
            Uuid::new_v4().hyphenated()
        ));
        let mut lock_path = index_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);

        let result = (|| {
            self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                ["read-tree", target],
                "准备 Checkpoint 预检",
            )?;
            self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                [
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=no",
                    "--",
                    ".",
                ],
                "刷新 Checkpoint 预检索引",
            )?;
            let tracked = self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                ["diff-files", "--name-only", "-z", "--", "."],
                "检查已跟踪文件变化",
            )?;
            let untracked = self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                [
                    "ls-files",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                    ".",
                ],
                "检查新增文件",
            )?;
            let changed_paths = split_nul(&tracked.stdout)
                .chain(split_nul(&untracked.stdout))
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok(changed_paths)
        })();

        let cleanup_result = [index_path.as_path(), lock_path.as_path()]
            .into_iter()
            .try_for_each(|path| match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            });
        match (result, cleanup_result) {
            (Ok(changed_paths), Ok(())) => Ok(changed_paths),
            (Ok(_), Err(error)) => Err(error).with_context(|| "无法清理 Checkpoint 预检索引"),
            (Err(error), _) => Err(error),
        }
    }

    fn worktree_file_changes(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        target: &str,
    ) -> anyhow::Result<Vec<WorkspaceCheckpointFileChange>> {
        let index_path = state.dir.join(format!(
            "turn-completion-index-{}.index",
            Uuid::new_v4().hyphenated()
        ));
        let mut lock_path = index_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);

        let result = (|| {
            self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                ["read-tree", target],
                "准备轮次文件变化索引",
            )?;
            self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                ["add", "-A", "--", "."],
                "收集本轮文件变化",
            )?;
            let status_output = self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                [
                    "diff",
                    "--cached",
                    "--name-status",
                    "-z",
                    "--no-renames",
                    target,
                    "--",
                    ".",
                ],
                "读取本轮文件状态",
            )?;
            let numstat_output = self.run_git_with_index_checked(
                state,
                workspace,
                &index_path,
                [
                    "diff",
                    "--cached",
                    "--numstat",
                    "-z",
                    "--no-renames",
                    target,
                    "--",
                    ".",
                ],
                "统计本轮文件增删",
            )?;
            parse_file_changes(&status_output.stdout, &numstat_output.stdout)
        })();

        let cleanup_result = [index_path.as_path(), lock_path.as_path()]
            .into_iter()
            .try_for_each(|path| match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            });
        match (result, cleanup_result) {
            (Ok(changed_files), Ok(())) => Ok(changed_files),
            (Ok(_), Err(error)) => Err(error).with_context(|| "无法清理轮次文件变化索引"),
            (Err(error), _) => Err(error),
        }
    }

    fn verify_commit(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        commit_hash: &str,
    ) -> anyhow::Result<()> {
        let object = format!("{commit_hash}^{{commit}}");
        self.run_git_checked(
            state,
            workspace,
            ["cat-file", "-e", object.as_str()],
            "校验 Checkpoint",
        )?;
        Ok(())
    }

    fn try_head(&self, state: &WorkspaceState, workspace: &Path) -> anyhow::Result<Option<String>> {
        let output = self.run_git(state, workspace, ["rev-parse", "--verify", "HEAD"])?;
        if !output.status.success() {
            return Ok(None);
        }
        let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!head.is_empty()).then_some(head))
    }

    fn prepare_repository(&self, workspace: &Path, state: &WorkspaceState) -> anyhow::Result<()> {
        fs::create_dir_all(&state.dir).with_context(|| {
            format!("无法创建 Checkpoint 目录：{}", state.dir.to_string_lossy())
        })?;
        let previous_storage_version = read_workspace_descriptor(&state.dir)
            .map(|descriptor| descriptor.schema_version)
            .unwrap_or_default();
        if !state.git_dir.is_dir() {
            fs::create_dir_all(&state.repository_dir)?;
            let output = self.run_plain_git(
                [
                    OsStr::new("-c"),
                    OsStr::new("init.defaultBranch=checkpoint"),
                    OsStr::new("init"),
                    OsStr::new("--quiet"),
                    state.repository_dir.as_os_str(),
                ],
                &state.global_config,
            )?;
            ensure_success(output, "初始化 shadow Git")?;
        }
        fs::create_dir_all(state.git_dir.join("info"))?;
        fs::create_dir_all(&state.hooks_dir)?;
        if !state.global_config.exists() {
            fs::write(&state.global_config, b"")?;
        }

        let mut exclude = String::from("/.git\n/.git/\n");
        if let Ok(relative) = self.root.strip_prefix(workspace) {
            let relative = git_path(relative);
            if !relative.is_empty() {
                exclude.push('/');
                exclude.push_str(relative.trim_matches('/'));
                exclude.push_str("/\n");
            }
        }
        fs::write(
            state.git_dir.join("info").join("exclude"),
            exclude.as_bytes(),
        )?;
        fs::write(
            state.git_dir.join("info").join("attributes"),
            b"* -text -eol -filter -ident -working-tree-encoding\n",
        )?;

        if previous_storage_version < STORAGE_VERSION && state.events_path.is_file() {
            self.migrate_legacy_history_locked(workspace, state)?;
        }
        let descriptor = WorkspaceDescriptor {
            schema_version: STORAGE_VERSION,
            workspace: workspace_string(workspace),
        };
        fs::write(
            state.dir.join("workspace.json"),
            serde_json::to_vec_pretty(&descriptor)?,
        )?;
        Ok(())
    }

    fn workspace_state(&self, workspace: &Path) -> WorkspaceState {
        let key = workspace_key(workspace);
        self.workspace_state_from_key(&key)
    }

    fn workspace_state_from_key(&self, key: &str) -> WorkspaceState {
        let dir = self.root.join(key);
        let repository_dir = dir.join("repository");
        WorkspaceState {
            git_dir: repository_dir.join(".git"),
            repository_dir,
            events_path: dir.join("events.jsonl"),
            lock_path: dir.join("checkpoint.lock"),
            global_config: dir.join("git-global-config"),
            hooks_dir: dir.join("hooks-disabled"),
            dir,
        }
    }

    fn lock_workspace(&self, state: &WorkspaceState) -> anyhow::Result<File> {
        fs::create_dir_all(&state.dir)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&state.lock_path)?;
        file.lock_exclusive()
            .with_context(|| "无法锁定工作区 Checkpoint 存储")?;
        Ok(file)
    }

    fn lock_root_shared(&self) -> anyhow::Result<File> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "无法创建 Checkpoint 储存目录：{}",
                self.root.to_string_lossy()
            )
        })?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(self.root.join(ROOT_LOCK_FILE))?;
        FileExt::lock_shared(&file).with_context(|| "无法锁定 Checkpoint 储存目录")?;
        Ok(file)
    }

    fn lock_root_exclusive(&self) -> anyhow::Result<File> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "无法创建 Checkpoint 储存目录：{}",
                self.root.to_string_lossy()
            )
        })?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(self.root.join(ROOT_LOCK_FILE))?;
        FileExt::lock_exclusive(&file).with_context(|| "无法独占 Checkpoint 储存目录")?;
        Ok(file)
    }

    fn workspace_states(&self) -> anyhow::Result<Vec<(String, WorkspaceState)>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut states = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let key = entry.file_name().to_string_lossy().into_owned();
            if !valid_workspace_key(&key) {
                continue;
            }
            let state = self.workspace_state_from_key(&key);
            if state.events_path.is_file()
                || state.git_dir.is_dir()
                || state.dir.join("workspace.json").is_file()
            {
                states.push((key, state));
            }
        }
        states.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(states)
    }

    fn state_for_management_key(&self, raw_key: &str) -> anyhow::Result<WorkspaceState> {
        let key = raw_key.trim();
        if !valid_workspace_key(key) {
            bail!("无效的 Checkpoint 工作区标识");
        }
        let state = self.workspace_state_from_key(key);
        if !state.dir.is_dir() {
            bail!("未找到指定的 Checkpoint 工作区");
        }
        Ok(state)
    }

    fn delete_workspace_state(&self, state: WorkspaceState) -> anyhow::Result<usize> {
        if !state.dir.is_dir() {
            return Ok(0);
        }
        let checkpoint_count;
        {
            let _workspace_lock = self.lock_workspace(&state)?;
            checkpoint_count = self.load_checkpoints(&state)?.len();
            for entry in fs::read_dir(&state.dir)? {
                let entry = entry?;
                let path = entry.path();
                if path == state.lock_path {
                    continue;
                }
                remove_path(&path)?;
            }
        }
        match fs::remove_file(&state.lock_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match fs::remove_dir(&state.dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(checkpoint_count)
    }

    fn cleanup_workspace_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
        auto_gc: bool,
    ) -> anyhow::Result<usize> {
        let checkpoints = self.load_checkpoints(state)?;
        if checkpoints.is_empty() {
            return Ok(0);
        }
        let mut deleted_ids = HashSet::<String>::new();
        let now = now_ms();
        let pending_ttl_ms = PENDING_CHECKPOINT_TTL.as_millis() as u64;
        for checkpoint in &checkpoints {
            if checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                && !checkpoint.accepted
                && now.saturating_sub(checkpoint.created_at_ms) >= pending_ttl_ms
            {
                deleted_ids.insert(checkpoint.id.clone());
            }
        }

        if let Some(limit) = self.retention_rounds {
            let mut turns_by_thread = BTreeMap::<String, Vec<&WorkspaceCheckpoint>>::new();
            for checkpoint in &checkpoints {
                if checkpoint.kind == WorkspaceCheckpointKind::TurnStart && checkpoint.accepted {
                    turns_by_thread
                        .entry(checkpoint.thread_id.clone())
                        .or_default()
                        .push(checkpoint);
                }
            }
            for turns in turns_by_thread.values() {
                let remove_count = turns.len().saturating_sub(limit);
                for checkpoint in turns.iter().take(remove_count) {
                    deleted_ids.insert(checkpoint.id.clone());
                }
            }
        }

        let mut safety_by_thread = BTreeMap::<String, Vec<&WorkspaceCheckpoint>>::new();
        for checkpoint in &checkpoints {
            if checkpoint.kind == WorkspaceCheckpointKind::RestoreSafety {
                safety_by_thread
                    .entry(checkpoint.thread_id.clone())
                    .or_default()
                    .push(checkpoint);
            }
        }
        for safety_checkpoints in safety_by_thread.values() {
            let remove_count = safety_checkpoints
                .len()
                .saturating_sub(MAX_RESTORE_SAFETY_CHECKPOINTS_PER_THREAD);
            for checkpoint in safety_checkpoints.iter().take(remove_count) {
                deleted_ids.insert(checkpoint.id.clone());
            }
        }

        if deleted_ids.is_empty() {
            return Ok(0);
        }
        let remaining = checkpoints
            .into_iter()
            .filter(|checkpoint| !deleted_ids.contains(&checkpoint.id))
            .collect::<Vec<_>>();
        self.replace_checkpoints_locked(workspace, state, &remaining)?;
        if auto_gc && state.git_dir.is_dir() {
            self.run_git_checked(
                state,
                workspace,
                ["gc", "--auto", "--quiet"],
                "整理 Checkpoint 存储",
            )?;
        }
        Ok(deleted_ids.len())
    }

    fn replace_checkpoints_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
        remaining: &[WorkspaceCheckpoint],
    ) -> anyhow::Result<()> {
        let previous = self.load_checkpoints(state)?;
        let remaining_ids = remaining
            .iter()
            .map(|checkpoint| checkpoint.id.as_str())
            .collect::<HashSet<_>>();
        let removed = previous
            .iter()
            .filter(|checkpoint| !remaining_ids.contains(checkpoint.id.as_str()))
            .collect::<Vec<_>>();
        self.rewrite_events(state, remaining)?;
        if state.git_dir.is_dir() {
            for checkpoint in removed {
                self.delete_checkpoint_ref(state, workspace, &checkpoint.id)?;
            }
            self.refresh_baseline_ref(state, workspace, remaining)?;
        }
        Ok(())
    }

    fn rewrite_events(
        &self,
        state: &WorkspaceState,
        checkpoints: &[WorkspaceCheckpoint],
    ) -> anyhow::Result<()> {
        fs::create_dir_all(&state.dir)?;
        let temp_path = state
            .dir
            .join(format!("events-{}.jsonl.tmp", Uuid::new_v4().hyphenated()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)?;
            for checkpoint in checkpoints {
                let mut bytes = serde_json::to_vec(&CheckpointEvent::Created {
                    checkpoint: checkpoint.clone(),
                })?;
                bytes.push(b'\n');
                file.write_all(&bytes)?;
                if checkpoint.accepted {
                    let mut bytes = serde_json::to_vec(&CheckpointEvent::Bound {
                        checkpoint_id: checkpoint.id.clone(),
                        thread_id: optional_trimmed(&checkpoint.thread_id),
                        turn_id: checkpoint.turn_id.clone(),
                    })?;
                    bytes.push(b'\n');
                    file.write_all(&bytes)?;
                }
            }
            file.sync_all()?;
            replace_file(&temp_path, &state.events_path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    fn delete_checkpoint_ref(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        checkpoint_id: &str,
    ) -> anyhow::Result<()> {
        let reference = checkpoint_ref_name(checkpoint_id);
        self.run_git_checked(
            state,
            workspace,
            ["update-ref", "-d", reference.as_str()],
            "删除 Checkpoint 引用",
        )?;
        Ok(())
    }

    fn refresh_baseline_ref(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        checkpoints: &[WorkspaceCheckpoint],
    ) -> anyhow::Result<()> {
        if let Some(checkpoint) = checkpoints.last() {
            self.verify_commit(state, workspace, &checkpoint.commit_hash)?;
            self.run_git_checked(
                state,
                workspace,
                [
                    "update-ref",
                    "refs/heads/checkpoint",
                    checkpoint.commit_hash.as_str(),
                ],
                "更新 Checkpoint 基线",
            )?;
            self.run_git_checked(
                state,
                workspace,
                ["read-tree", checkpoint.commit_hash.as_str()],
                "同步 Checkpoint 索引",
            )?;
        } else {
            self.run_git_checked(
                state,
                workspace,
                ["update-ref", "-d", "refs/heads/checkpoint"],
                "清除 Checkpoint 基线",
            )?;
            match fs::remove_file(state.git_dir.join("index")) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn reconcile_checkpoint_refs_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
    ) -> anyhow::Result<()> {
        let checkpoints = self.load_checkpoints(state)?;
        let active_refs = checkpoints
            .iter()
            .map(|checkpoint| checkpoint_ref_name(&checkpoint.id))
            .collect::<HashSet<_>>();
        for checkpoint in &checkpoints {
            let reference = checkpoint_ref_name(&checkpoint.id);
            self.run_git_checked(
                state,
                workspace,
                [
                    "update-ref",
                    reference.as_str(),
                    checkpoint.commit_hash.as_str(),
                ],
                "修复 Checkpoint 引用",
            )?;
        }
        let refs = self.run_git_checked(
            state,
            workspace,
            ["for-each-ref", "--format=%(refname)", CHECKPOINT_REF_PREFIX],
            "读取 Checkpoint 引用",
        )?;
        for reference in String::from_utf8_lossy(&refs.stdout)
            .lines()
            .map(str::trim)
            .filter(|reference| !reference.is_empty())
        {
            if !active_refs.contains(reference) {
                self.run_git_checked(
                    state,
                    workspace,
                    ["update-ref", "-d", reference],
                    "清理失效 Checkpoint 引用",
                )?;
            }
        }
        self.refresh_baseline_ref(state, workspace, &checkpoints)
    }

    fn migrate_legacy_history_locked(
        &self,
        workspace: &Path,
        state: &WorkspaceState,
    ) -> anyhow::Result<()> {
        let mut checkpoints = self.load_checkpoints(state)?;
        if checkpoints.is_empty() {
            return Ok(());
        }
        for checkpoint in &mut checkpoints {
            self.verify_commit(state, workspace, &checkpoint.commit_hash)?;
            let parents = self.run_git_checked(
                state,
                workspace,
                [
                    "rev-list",
                    "--parents",
                    "-n",
                    "1",
                    checkpoint.commit_hash.as_str(),
                ],
                "检查旧版 Checkpoint 历史",
            )?;
            if String::from_utf8_lossy(&parents.stdout)
                .split_whitespace()
                .count()
                > 1
            {
                let object = format!("{}^{{tree}}", checkpoint.commit_hash);
                let tree = self.run_git_checked(
                    state,
                    workspace,
                    ["rev-parse", object.as_str()],
                    "读取旧版 Checkpoint 文件树",
                )?;
                let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();
                let message = format!("CodexElves checkpoint {}", checkpoint.id);
                let commit = self.run_git_checked(
                    state,
                    workspace,
                    ["commit-tree", tree.as_str(), "-m", message.as_str()],
                    "迁移旧版 Checkpoint",
                )?;
                checkpoint.commit_hash = String::from_utf8_lossy(&commit.stdout).trim().to_string();
            }
            let reference = checkpoint_ref_name(&checkpoint.id);
            self.run_git_checked(
                state,
                workspace,
                [
                    "update-ref",
                    reference.as_str(),
                    checkpoint.commit_hash.as_str(),
                ],
                "迁移 Checkpoint 引用",
            )?;
        }
        self.rewrite_events(state, &checkpoints)?;
        self.refresh_baseline_ref(state, workspace, &checkpoints)
    }

    fn load_checkpoints(&self, state: &WorkspaceState) -> anyhow::Result<Vec<WorkspaceCheckpoint>> {
        let file = match File::open(&state.events_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut checkpoints = Vec::<WorkspaceCheckpoint>::new();
        let mut indexes = HashMap::<String, usize>::new();
        for line in BufReader::new(file).lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => continue,
            };
            let event = match serde_json::from_str::<CheckpointEvent>(&line) {
                Ok(event) => event,
                Err(_) => continue,
            };
            match event {
                CheckpointEvent::Created { checkpoint } => {
                    if checkpoint.schema_version != SCHEMA_VERSION {
                        continue;
                    }
                    indexes.insert(checkpoint.id.clone(), checkpoints.len());
                    checkpoints.push(checkpoint);
                }
                CheckpointEvent::Bound {
                    checkpoint_id,
                    thread_id,
                    turn_id,
                } => {
                    if let Some(index) = indexes.get(&checkpoint_id).copied() {
                        checkpoints[index].accepted = true;
                        if let Some(thread_id) = thread_id.filter(|value| !value.trim().is_empty())
                        {
                            checkpoints[index].thread_id = thread_id;
                        }
                        checkpoints[index].turn_id = turn_id;
                    }
                }
                CheckpointEvent::Initialized {
                    checkpoint_id,
                    initial_file_count,
                } => {
                    if let Some(index) = indexes.get(&checkpoint_id).copied() {
                        checkpoints[index].initialization = true;
                        checkpoints[index].initial_file_count = Some(initial_file_count);
                    }
                }
                CheckpointEvent::Completed {
                    checkpoint_id,
                    status,
                    completed_at_ms,
                    changed_files,
                } => {
                    if let Some(index) = indexes.get(&checkpoint_id).copied()
                        && checkpoints[index].turn_status.is_none()
                    {
                        checkpoints[index].change_scope = WorkspaceCheckpointChangeScope::Turn;
                        checkpoints[index].turn_status = Some(status);
                        checkpoints[index].completed_at_ms = Some(completed_at_ms);
                        checkpoints[index].changed_file_count = changed_files.len();
                        checkpoints[index].changed_files = changed_files;
                    }
                }
            }
        }
        Ok(checkpoints)
    }

    fn append_event(&self, state: &WorkspaceState, event: &CheckpointEvent) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&state.events_path)?;
        file.write_all(&bytes)?;
        file.sync_data()?;
        Ok(())
    }

    fn run_git<I, S>(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        args: I,
    ) -> anyhow::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.git_command(&state.global_config);
        command
            .arg("--literal-pathspecs")
            .arg("--git-dir")
            .arg(&state.git_dir)
            .arg("--work-tree")
            .arg(workspace)
            .arg("-c")
            .arg("core.autocrlf=false")
            .arg("-c")
            .arg("core.longpaths=true")
            .arg("-c")
            .arg("commit.gpgSign=false")
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                state.hooks_dir.to_string_lossy()
            ))
            .args(args)
            .current_dir(git_current_dir(workspace, state));
        command
            .output()
            .with_context(|| "无法执行 Git；Checkpoint 功能需要本机 Git")
    }

    fn run_git_checked<I, S>(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        args: I,
        action: &str,
    ) -> anyhow::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        ensure_success(self.run_git(state, workspace, args)?, action)
    }

    fn run_git_with_index<I, S>(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        index_path: &Path,
        args: I,
    ) -> anyhow::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.git_command(&state.global_config);
        command
            .env("GIT_INDEX_FILE", index_path)
            .arg("--literal-pathspecs")
            .arg("--git-dir")
            .arg(&state.git_dir)
            .arg("--work-tree")
            .arg(workspace)
            .arg("-c")
            .arg("core.autocrlf=false")
            .arg("-c")
            .arg("core.longpaths=true")
            .arg("-c")
            .arg("commit.gpgSign=false")
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                state.hooks_dir.to_string_lossy()
            ))
            .args(args)
            .current_dir(git_current_dir(workspace, state));
        command
            .output()
            .with_context(|| "无法执行 Git；Checkpoint 功能需要本机 Git")
    }

    fn run_git_with_index_checked<I, S>(
        &self,
        state: &WorkspaceState,
        workspace: &Path,
        index_path: &Path,
        args: I,
        action: &str,
    ) -> anyhow::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        ensure_success(
            self.run_git_with_index(state, workspace, index_path, args)?,
            action,
        )
    }

    fn run_plain_git<I, S>(&self, args: I, global_config: &Path) -> anyhow::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git_command(global_config)
            .args(args)
            .output()
            .with_context(|| "无法执行 Git；Checkpoint 功能需要本机 Git")
    }

    fn git_command(&self, global_config: &Path) -> Command {
        let mut command = Command::new("git");
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_COMMON_DIR",
            "GIT_CONFIG_COUNT",
        ] {
            command.env_remove(key);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", global_config)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_AUTHOR_NAME", "CodexElves Checkpoint")
            .env("GIT_AUTHOR_EMAIL", "checkpoint@codexelves.local")
            .env("GIT_COMMITTER_NAME", "CodexElves Checkpoint")
            .env("GIT_COMMITTER_EMAIL", "checkpoint@codexelves.local");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(crate::windows_create_no_window());
        }
        command
    }
}

#[derive(Debug)]
struct SnapshotMetadata {
    id: String,
    request_id: String,
    thread_id: String,
    prompt_preview: String,
    kind: WorkspaceCheckpointKind,
    accepted: bool,
    initialization: bool,
    change_scope: WorkspaceCheckpointChangeScope,
}

#[derive(Debug)]
struct WorkspaceState {
    dir: PathBuf,
    repository_dir: PathBuf,
    git_dir: PathBuf,
    events_path: PathBuf,
    lock_path: PathBuf,
    global_config: PathBuf,
    hooks_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceDescriptor {
    schema_version: u32,
    workspace: String,
}

pub async fn handle_create(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<CreateCheckpointRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.create_checkpoint(request)).await
}

pub async fn handle_bind_turn(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<BindTurnRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.bind_turn(request)).await
}

pub async fn handle_complete_turn(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<CompleteTurnRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.complete_turn(request)).await
}

pub async fn handle_list(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<ListCheckpointsRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.list_checkpoints(request)).await
}

pub async fn handle_restore(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<RestoreCheckpointRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.restore_checkpoint(request)).await
}

pub async fn handle_restore_for_revert(
    service: WorkspaceCheckpointService,
    payload: Value,
) -> Value {
    let request = match serde_json::from_value::<RestoreForRevertRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.restore_for_revert(request)).await
}

pub async fn handle_preview_revert(service: WorkspaceCheckpointService, payload: Value) -> Value {
    let request = match serde_json::from_value::<RestoreForRevertRequest>(payload) {
        Ok(request) => request,
        Err(error) => return failed("invalid_input", error),
    };
    blocking_response(move || service.preview_revert(request)).await
}

async fn blocking_response<T, F>(operation: F) -> Value
where
    T: Serialize + Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(Ok(result)) => match serde_json::to_value(result) {
            Ok(mut value) => {
                if let Some(object) = value.as_object_mut() {
                    object.insert("status".to_string(), Value::String("ok".to_string()));
                }
                value
            }
            Err(error) => failed("checkpoint_failed", error),
        },
        Ok(Err(error)) => failed("checkpoint_failed", error),
        Err(error) => failed("checkpoint_unavailable", error),
    }
}

fn failed(code: &str, error: impl std::fmt::Display) -> Value {
    json!({
        "status": "failed",
        "code": code,
        "message": error.to_string(),
    })
}

pub fn configured_root(settings: &crate::settings::BackendSettings) -> anyhow::Result<PathBuf> {
    let raw = settings
        .codex_app_workspace_checkpoint_storage_path
        .trim()
        .trim_matches('"');
    if raw.is_empty() {
        return Ok(crate::paths::default_workspace_checkpoints_dir());
    }
    let path = PathBuf::from(raw);
    validate_storage_root_path(&path)?;
    Ok(path)
}

fn checkpoint_ref_name(checkpoint_id: &str) -> String {
    let digest = Sha256::digest(checkpoint_id.as_bytes());
    let key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{CHECKPOINT_REF_PREFIX}{key}")
}

fn valid_workspace_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_workspace_descriptor(dir: &Path) -> Option<WorkspaceDescriptor> {
    let contents = fs::read(dir.join("workspace.json")).ok()?;
    serde_json::from_slice(&contents).ok()
}

fn workspace_path_for_state(state: &WorkspaceState) -> PathBuf {
    read_workspace_descriptor(&state.dir)
        .map(|descriptor| PathBuf::from(descriptor.workspace))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| state.dir.clone())
}

fn git_current_dir<'a>(workspace: &'a Path, state: &'a WorkspaceState) -> &'a Path {
    if workspace.is_dir() {
        workspace
    } else {
        &state.dir
    }
}

fn directory_size(path: &Path) -> anyhow::Result<u64> {
    directory_size_excluding(path, &[])
}

fn directory_size_excluding(path: &Path, excluded_names: &[&str]) -> anyhow::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            if current == path
                && excluded_names
                    .iter()
                    .any(|name| entry.file_name() == OsStr::new(name))
            {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() || metadata.is_file() {
                total = total.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(total)
}

fn replace_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    match fs::remove_file(target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::rename(source, target)
        .with_context(|| format!("无法替换 Checkpoint 索引：{}", target.to_string_lossy()))
}

fn remove_path(path: &Path) -> anyhow::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn clear_directory_contents(path: &Path, excluded_names: &[&str]) -> anyhow::Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if excluded_names
            .iter()
            .any(|name| entry.file_name() == OsStr::new(name))
        {
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn copy_directory_contents(
    source: &Path,
    target: &Path,
    excluded_names: &[&str],
) -> anyhow::Result<()> {
    fs::create_dir_all(target)?;
    if !source.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if excluded_names
            .iter()
            .any(|name| entry.file_name() == OsStr::new(name))
        {
            continue;
        }
        copy_path(&entry.path(), &target.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_path(source: &Path, target: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        bail!(
            "Checkpoint 储存目录包含不支持迁移的符号链接：{}",
            source.to_string_lossy()
        );
    }
    if metadata.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

fn prepare_storage_target(path: &Path) -> anyhow::Result<PathBuf> {
    validate_storage_root_path(path)?;
    fs::create_dir_all(path)
        .with_context(|| format!("无法创建 Checkpoint 储存目录：{}", path.to_string_lossy()))?;
    let path = without_verbatim_prefix(fs::canonicalize(path)?);
    validate_storage_root_path(&path)?;
    Ok(path)
}

fn validate_storage_root_path(path: &Path) -> anyhow::Result<()> {
    if !path.is_absolute() {
        bail!("Checkpoint 储存目录必须是绝对路径");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("Checkpoint 储存目录不能包含上级目录跳转");
    }
    if !path
        .components()
        .any(|component| matches!(component, Component::Normal(_)))
    {
        bail!("拒绝把磁盘根目录用作 Checkpoint 储存目录");
    }
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalized_path_for_comparison(left) == normalized_path_for_comparison(right)
}

fn path_is_within(path: &Path, parent: &Path) -> bool {
    let path = normalized_path_for_comparison(path);
    let mut parent = normalized_path_for_comparison(parent);
    if path == parent {
        return true;
    }
    if !parent.ends_with('/') {
        parent.push('/');
    }
    path.starts_with(&parent)
}

fn normalized_path_for_comparison(path: &Path) -> String {
    let path = fs::canonicalize(path)
        .map(without_verbatim_prefix)
        .unwrap_or_else(|_| path.to_path_buf());
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        return normalized.to_lowercase();
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

fn resolve_workspace(raw: &str) -> anyhow::Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("未识别到当前会话工作目录");
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        bail!("工作区路径必须是绝对路径");
    }
    let path = fs::canonicalize(&path)
        .with_context(|| format!("工作区不存在或无法访问：{}", path.to_string_lossy()))?;
    let path = without_verbatim_prefix(path);
    if !path.is_dir() {
        bail!("Checkpoint 仅支持文件夹工作区");
    }
    if !path
        .components()
        .any(|component| matches!(component, Component::Normal(_)))
    {
        bail!("拒绝对磁盘根目录创建或恢复 Checkpoint");
    }
    Ok(path)
}

fn validate_thread_scope(
    checkpoint: &WorkspaceCheckpoint,
    requested_thread_id: &str,
) -> anyhow::Result<()> {
    let requested_thread_id = requested_thread_id.trim();
    if !requested_thread_id.is_empty()
        && !checkpoint.thread_id.is_empty()
        && checkpoint.thread_id != requested_thread_id
    {
        bail!("该 Checkpoint 不属于当前 thread");
    }
    Ok(())
}

fn workspace_key(workspace: &Path) -> String {
    let normalized = normalized_workspace_key(workspace);
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalized_workspace_key(workspace: &Path) -> String {
    let normalized = workspace_string(workspace).replace('\\', "/");
    #[cfg(windows)]
    {
        return normalized.to_lowercase();
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

fn workspace_string(workspace: &Path) -> String {
    workspace.to_string_lossy().into_owned()
}

fn git_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.to_string_lossy();
        if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

fn non_empty_or_uuid(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        Uuid::new_v4().hyphenated().to_string()
    } else {
        value.to_string()
    }
}

fn optional_trimmed(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn split_nul(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
}

fn parse_file_changes(
    status_bytes: &[u8],
    numstat_bytes: &[u8],
) -> anyhow::Result<Vec<WorkspaceCheckpointFileChange>> {
    let mut stats = HashMap::<Vec<u8>, (Option<usize>, Option<usize>)>::new();
    for record in split_nul(numstat_bytes) {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let Some(additions) = fields.next() else {
            continue;
        };
        let Some(deletions) = fields.next() else {
            continue;
        };
        let Some(path) = fields.next() else {
            continue;
        };
        stats.insert(
            path.to_vec(),
            (
                parse_numstat_value(additions),
                parse_numstat_value(deletions),
            ),
        );
    }

    let mut changes = Vec::new();
    let mut fields = split_nul(status_bytes);
    while let Some(status) = fields.next() {
        let path = fields
            .next()
            .ok_or_else(|| anyhow!("Git 返回了不完整的 Checkpoint 文件状态"))?;
        let status = String::from_utf8_lossy(status)
            .chars()
            .next()
            .unwrap_or('M')
            .to_string();
        let (additions, deletions) = stats.remove(path).unwrap_or((None, None));
        changes.push(WorkspaceCheckpointFileChange {
            path: String::from_utf8_lossy(path).into_owned(),
            status,
            additions,
            deletions,
        });
    }
    Ok(changes)
}

fn parse_numstat_value(value: &[u8]) -> Option<usize> {
    if value == b"-" {
        return None;
    }
    std::str::from_utf8(value).ok()?.parse().ok()
}

fn ensure_success(output: Output, action: &str) -> anyhow::Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    Err(git_failure(action, output.status, &output.stderr))
}

fn git_failure(action: &str, status: ExitStatus, stderr: &[u8]) -> anyhow::Error {
    let detail = truncate_chars(String::from_utf8_lossy(stderr).trim(), MAX_GIT_ERROR_CHARS);
    if detail.is_empty() {
        anyhow!("{action}失败（Git exit {status}）")
    } else {
        anyhow!("{action}失败：{detail}")
    }
}
