use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const CREATE_PATH: &str = "/workspace-checkpoint/create";
pub const BIND_TURN_PATH: &str = "/workspace-checkpoint/bind-turn";
pub const LIST_PATH: &str = "/workspace-checkpoint/list";
pub const RESTORE_PATH: &str = "/workspace-checkpoint/restore";
pub const PREVIEW_REVERT_PATH: &str = "/workspace-checkpoint/preview-revert";
pub const RESTORE_FOR_REVERT_PATH: &str = "/workspace-checkpoint/restore-for-revert";

const SCHEMA_VERSION: u32 = 1;
const MAX_LIST_LIMIT: usize = 200;
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_PROMPT_PREVIEW_CHARS: usize = 280;
const MAX_GIT_ERROR_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceCheckpointKind {
    TurnStart,
    RestoreSafety,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum CheckpointEvent {
    Created {
        checkpoint: WorkspaceCheckpoint,
    },
    Bound {
        checkpoint_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct WorkspaceCheckpointService {
    root: PathBuf,
}

impl Default for WorkspaceCheckpointService {
    fn default() -> Self {
        Self::new(crate::paths::default_workspace_checkpoints_dir())
    }
}

impl WorkspaceCheckpointService {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_checkpoint(
        &self,
        request: CreateCheckpointRequest,
    ) -> anyhow::Result<CreateCheckpointResult> {
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        self.prepare_repository(&workspace, &state)?;

        let request_id = non_empty_or_uuid(&request.request_id);
        let existing = self
            .load_checkpoints(&state)?
            .into_iter()
            .find(|checkpoint| {
                checkpoint.kind == WorkspaceCheckpointKind::TurnStart
                    && checkpoint.thread_id == request.thread_id.trim()
                    && checkpoint.request_id == request_id
            });
        if let Some(checkpoint) = existing {
            return Ok(CreateCheckpointResult { checkpoint });
        }

        let checkpoint = self.snapshot_locked(
            &workspace,
            &state,
            SnapshotMetadata {
                id: Uuid::new_v4().hyphenated().to_string(),
                request_id,
                thread_id: request.thread_id.trim().to_string(),
                prompt_preview: truncate_chars(
                    request.prompt_preview.trim(),
                    MAX_PROMPT_PREVIEW_CHARS,
                ),
                kind: WorkspaceCheckpointKind::TurnStart,
                accepted: false,
            },
        )?;
        Ok(CreateCheckpointResult { checkpoint })
    }

    pub fn bind_turn(&self, request: BindTurnRequest) -> anyhow::Result<CreateCheckpointResult> {
        let workspace = resolve_workspace(&request.cwd)?;
        let state = self.workspace_state(&workspace);
        let _lock = self.lock_workspace(&state)?;
        let checkpoints = self.load_checkpoints(&state)?;
        let checkpoint = checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == request.checkpoint_id.trim())
            .cloned()
            .ok_or_else(|| anyhow!("未找到待绑定的工作区 Checkpoint"))?;
        validate_thread_scope(&checkpoint, &request.thread_id)?;

        let turn_id = optional_trimmed(&request.turn_id);
        if !checkpoint.accepted || checkpoint.turn_id != turn_id {
            self.append_event(
                &state,
                &CheckpointEvent::Bound {
                    checkpoint_id: checkpoint.id.clone(),
                    turn_id,
                },
            )?;
        }
        let checkpoint = self
            .load_checkpoints(&state)?
            .into_iter()
            .find(|candidate| candidate.id == checkpoint.id)
            .ok_or_else(|| anyhow!("Checkpoint 绑定后状态读取失败"))?;
        Ok(CreateCheckpointResult { checkpoint })
    }

    pub fn list_checkpoints(
        &self,
        request: ListCheckpointsRequest,
    ) -> anyhow::Result<ListCheckpointsResult> {
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
        let warnings = if partial {
            vec!["部分嵌套仓库或并发写入的文件未能恢复；已保留恢复前安全 Checkpoint。".to_string()]
        } else {
            Vec::new()
        };

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
        let changed_files = self.staged_file_changes(state, workspace)?;
        let changed_file_count = changed_files.len();
        let head = self.try_head(state, workspace)?;
        let commit_hash = if head.is_some() && changed_file_count == 0 {
            head.unwrap_or_default()
        } else {
            let message = format!("CodexElves checkpoint {}", metadata.id);
            self.run_git_checked(
                state,
                workspace,
                [
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "--no-verify",
                    "--no-gpg-sign",
                    "-m",
                    message.as_str(),
                ],
                "创建工作区 Checkpoint",
            )?;
            self.try_head(state, workspace)?
                .ok_or_else(|| anyhow!("Checkpoint 提交创建成功但无法读取 commit"))?
        };

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

        let mut stats = HashMap::<Vec<u8>, (Option<usize>, Option<usize>)>::new();
        for record in split_nul(&numstat_output.stdout) {
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
        let mut fields = split_nul(&status_output.stdout);
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

        let descriptor = json!({
            "schemaVersion": SCHEMA_VERSION,
            "workspace": workspace_string(workspace),
        });
        fs::write(
            state.dir.join("workspace.json"),
            serde_json::to_vec_pretty(&descriptor)?,
        )?;
        Ok(())
    }

    fn workspace_state(&self, workspace: &Path) -> WorkspaceState {
        let key = workspace_key(workspace);
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
                    turn_id,
                } => {
                    if let Some(index) = indexes.get(&checkpoint_id).copied() {
                        checkpoints[index].accepted = true;
                        checkpoints[index].turn_id = turn_id;
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
            .arg(format!(
                "core.hooksPath={}",
                state.hooks_dir.to_string_lossy()
            ))
            .args(args)
            .current_dir(workspace);
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
            .arg(format!(
                "core.hooksPath={}",
                state.hooks_dir.to_string_lossy()
            ))
            .args(args)
            .current_dir(workspace);
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
