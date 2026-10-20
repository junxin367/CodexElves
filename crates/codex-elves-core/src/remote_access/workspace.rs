//! Read-only Git views scoped to the workspace resolved by the native session bridge.
#[cfg(windows)]
use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::{ApiError, RemoteAuthorization, WebState};

const MAX_STATUS_BYTES: usize = 2 * 1024 * 1024;
const MAX_FILES: usize = 2_000;
const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIFF_BYTES: usize = 512 * 1024;
const MAX_STDERR_BYTES: usize = 8 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeEntry {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    status: String,
    section: String,
}

struct Snapshot {
    files: Vec<ChangeEntry>,
    truncated: bool,
}

struct Repository {
    root: PathBuf,
    head: Option<String>,
    authorization: RemoteAuthorization,
}

struct GitOutput {
    bytes: Vec<u8>,
    success: bool,
    truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Blob {
    oid: String,
    mode: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: Option<SystemTime>,
}

struct Side {
    exists: bool,
    size: u64,
    bytes: Vec<u8>,
    truncated: bool,
    unsupported: bool,
    stamp: Option<FileStamp>,
}

impl Side {
    fn absent() -> Self {
        Self {
            exists: false,
            size: 0,
            bytes: Vec::new(),
            truncated: false,
            unsupported: false,
            stamp: None,
        }
    }

    fn public(&self, image: bool, path: &str) -> Value {
        let mime = raster_mime(&self.bytes)
            .or_else(|| raster_extension_mime(path))
            .unwrap_or(if !self.truncated && is_text(&self.bytes) {
                "text/plain"
            } else {
                "application/octet-stream"
            });
        let data_url = (image && self.exists && !self.truncated && !self.unsupported)
            .then(|| raster_mime(&self.bytes))
            .flatten()
            .map(|mime| format!("data:{mime};base64,{}", STANDARD.encode(&self.bytes)));
        json!({
            "exists": self.exists,
            "mime": self.exists.then_some(mime),
            "size": self.size,
            "dataUrl": data_url,
        })
    }
}

pub(super) async fn changes(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state.controller.authorization(device, None);
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    let if_revision = requested_revision(&payload)?;
    let cwd = super::workspace_context(&state, &payload, &authorization).await?;
    let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let repo = Repository::discover(cwd, authorization.clone()).await?;
        repo.listing().await
    })
    .await
    .map_err(|_| ApiError::new(504, "读取 Git 变更超时，请稍后刷新"))??;
    let result = revision_payload(result, if_revision)?;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(no_store(result))
}

pub(super) async fn change(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state.controller.authorization(device, None);
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    let if_revision = requested_revision(&payload)?;
    let path = payload
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| safe_relative_path(path))
        .ok_or_else(|| ApiError::new(400, "文件路径无效，请从变更列表重新选择"))?
        .to_owned();
    let section = payload
        .get("section")
        .and_then(Value::as_str)
        .filter(|section| matches!(*section, "staged" | "unstaged" | "untracked"))
        .ok_or_else(|| ApiError::new(400, "变更分区无效"))?
        .to_owned();
    let cwd = super::workspace_context(&state, &payload, &authorization).await?;
    let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let repo = Repository::discover(cwd, authorization.clone()).await?;
        let snapshot = repo.snapshot().await?;
        let entry = snapshot
            .files
            .into_iter()
            .find(|entry| entry.path == path && entry.section == section)
            .ok_or_else(|| ApiError::new(409, "该文件的变更状态已更新，请刷新变更列表"))?;
        repo.detail(&entry).await
    })
    .await
    .map_err(|_| ApiError::new(504, "读取文件差异超时，请稍后重试"))??;
    let result = revision_payload(result, if_revision)?;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(no_store(result))
}

fn no_store(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

fn requested_revision(payload: &Value) -> Result<Option<&str>, ApiError> {
    match payload.get("ifRevision") {
        None => Ok(None),
        Some(Value::String(revision))
            if revision.len() == 64
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')) =>
        {
            Ok(Some(revision))
        }
        Some(_) => Err(ApiError::new(
            400,
            "ifRevision 必须是 64 位小写十六进制 SHA-256 版本",
        )),
    }
}

fn revision_payload(mut payload: Value, if_revision: Option<&str>) -> Result<Value, ApiError> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| ApiError::new(503, "变更响应格式无效"))?;
    // A revision describes the complete visible response, including its root,
    // diff text and image data URLs. Never hash a previous revision into itself.
    object.remove("revision");
    let root = object
        .get("root")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::new(503, "变更响应缺少仓库目录"))?
        .to_owned();
    let bytes =
        serde_json::to_vec(&payload).map_err(|_| ApiError::new(503, "无法生成变更内容版本"))?;
    let revision = format!("{:x}", Sha256::digest(bytes));
    if if_revision == Some(revision.as_str()) {
        return Ok(json!({
            "ok": true,
            "notModified": true,
            "revision": revision,
            "root": root,
        }));
    }
    payload["revision"] = json!(revision);
    Ok(payload)
}

impl Repository {
    async fn discover(cwd: PathBuf, authorization: RemoteAuthorization) -> Result<Self, ApiError> {
        let cwd = tokio::task::spawn_blocking(move || {
            std::fs::canonicalize(cwd)
                .map_err(|_| ApiError::new(404, "当前会话的工作目录不存在或无法读取"))
        })
        .await
        .map_err(|_| ApiError::new(503, "无法读取当前会话的工作目录"))??;
        let result = git(
            &cwd,
            &["rev-parse", "--show-toplevel"],
            32 * 1024,
            &authorization,
        )
        .await?;
        if !result.success || result.truncated {
            return Err(ApiError::new(422, "当前会话的工作目录不在 Git 仓库中"));
        }
        let root = String::from_utf8(result.bytes)
            .map_err(|_| ApiError::new(422, "Git 仓库路径编码无法识别"))?;
        let root = root.strip_suffix('\n').unwrap_or(&root);
        let root = root.strip_suffix('\r').unwrap_or(root);
        let root = PathBuf::from(root);
        let root = tokio::task::spawn_blocking(move || std::fs::canonicalize(root))
            .await
            .map_err(|_| ApiError::new(503, "无法读取 Git 仓库目录"))?
            .map_err(|_| ApiError::new(404, "Git 仓库目录已失效"))?;
        let head = git(
            &root,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
            256,
            &authorization,
        )
        .await?;
        let head = if head.success {
            let oid = String::from_utf8_lossy(&head.bytes).trim().to_owned();
            if !valid_oid(&oid) {
                return Err(ApiError::new(503, "Git HEAD 无效"));
            }
            Some(oid)
        } else {
            None
        };
        Ok(Self {
            root,
            head,
            authorization,
        })
    }

    async fn snapshot(&self) -> Result<Snapshot, ApiError> {
        let output = git(
            &self.root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=all",
                "--renames",
            ],
            MAX_STATUS_BYTES,
            &self.authorization,
        )
        .await?;
        if !output.success && !output.truncated {
            return Err(ApiError::new(503, "无法读取 Git 变更，请确认仓库仍可访问"));
        }
        parse_status(&output.bytes, output.truncated)
    }

    async fn listing(&self) -> Result<Value, ApiError> {
        let snapshot = self.snapshot().await?;
        Ok(json!({
            "ok": true,
            "root": self.root.to_string_lossy(),
            "files": snapshot.files,
            "truncated": snapshot.truncated,
            "message": snapshot.truncated.then_some("部分文件因列表大小、路径安全或文件名编码限制未显示；请在电脑端查看完整列表。"),
        }))
    }

    async fn index_blob(&self, path: &str, conflict: bool) -> Result<Option<Blob>, ApiError> {
        let output = git(
            &self.root,
            &["ls-files", "--stage", "-z", "--", path],
            32 * 1024,
            &self.authorization,
        )
        .await?;
        require_complete(&output)?;
        let mut candidates = Vec::new();
        for record in output
            .bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
        {
            let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
                return Err(ApiError::new(503, "Git index 返回了无效条目"));
            };
            if record[tab + 1..] != *path.as_bytes() {
                continue;
            }
            let header = std::str::from_utf8(&record[..tab])
                .map_err(|_| ApiError::new(503, "Git index 返回了无效条目"))?;
            let parts: Vec<_> = header.split(' ').collect();
            if parts.len() == 3 && valid_oid(parts[1]) {
                candidates.push((
                    parts[2].to_owned(),
                    parts[0].to_owned(),
                    parts[1].to_owned(),
                ));
            }
        }
        let candidate = candidates
            .iter()
            .find(|(stage, _, _)| stage == "0")
            .or_else(|| {
                conflict
                    .then(|| candidates.iter().find(|(stage, _, _)| stage == "2"))
                    .flatten()
            });
        if let Some((_, mode, oid)) = candidate {
            Ok(Some(self.blob_metadata(mode.clone(), oid.clone()).await?))
        } else {
            Ok(None)
        }
    }

    async fn head_blob(&self, path: &str) -> Result<Option<Blob>, ApiError> {
        let Some(head) = self.head.as_deref() else {
            return Ok(None);
        };
        let output = git(
            &self.root,
            &["ls-tree", "-z", head, "--", path],
            32 * 1024,
            &self.authorization,
        )
        .await?;
        require_complete(&output)?;
        for record in output
            .bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
        {
            let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
                return Err(ApiError::new(503, "Git tree 返回了无效条目"));
            };
            if record[tab + 1..] != *path.as_bytes() {
                continue;
            }
            let header = std::str::from_utf8(&record[..tab])
                .map_err(|_| ApiError::new(503, "Git tree 返回了无效条目"))?;
            let parts: Vec<_> = header.split(' ').collect();
            if parts.len() == 3 && valid_oid(parts[2]) {
                return Ok(Some(
                    self.blob_metadata(parts[0].to_owned(), parts[2].to_owned())
                        .await?,
                ));
            }
        }
        Ok(None)
    }

    async fn blob_metadata(&self, mode: String, oid: String) -> Result<Blob, ApiError> {
        // Gitlinks refer to commits, not file blobs. Never traverse a submodule.
        if mode == "160000" {
            return Ok(Blob { mode, oid, size: 0 });
        }
        let output = git(
            &self.root,
            &["cat-file", "-s", &oid],
            128,
            &self.authorization,
        )
        .await?;
        require_complete(&output)?;
        let size = std::str::from_utf8(&output.bytes)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .ok_or_else(|| ApiError::new(503, "无法读取 Git 文件大小"))?;
        Ok(Blob { mode, oid, size })
    }

    async fn read_blob(&self, blob: Option<&Blob>) -> Result<Side, ApiError> {
        let Some(blob) = blob else {
            return Ok(Side::absent());
        };
        let mut side = Side {
            exists: true,
            size: blob.size,
            bytes: Vec::new(),
            truncated: blob.size > MAX_IMAGE_BYTES as u64,
            unsupported: !matches!(blob.mode.as_str(), "100644" | "100755"),
            stamp: None,
        };
        if side.truncated || side.unsupported {
            return Ok(side);
        }
        let output = git(
            &self.root,
            &["cat-file", "blob", &blob.oid],
            MAX_IMAGE_BYTES,
            &self.authorization,
        )
        .await?;
        require_complete(&output)?;
        side.bytes = output.bytes;
        if side.bytes.len() as u64 != blob.size {
            return Err(ApiError::new(
                409,
                "Git 文件在读取期间发生变化，请刷新后重试",
            ));
        }
        Ok(side)
    }

    async fn read_worktree(&self, path: &str) -> Result<Side, ApiError> {
        self.authorization
            .check()
            .await
            .map_err(super::authorization_error)?;
        let root = self.root.clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || read_local(&root, &path))
            .await
            .map_err(|_| ApiError::new(503, "无法读取工作目录文件"))?
    }

    async fn detail(&self, entry: &ChangeEntry) -> Result<Value, ApiError> {
        let old_path = entry.old_path.as_deref().unwrap_or(&entry.path);
        let conflict = entry.status == "U";
        let before_blob = match entry.section.as_str() {
            "staged" => self.head_blob(old_path).await?,
            "unstaged" => self.index_blob(old_path, conflict).await?,
            _ => None,
        };
        let after_blob = if entry.section == "staged" {
            self.index_blob(&entry.path, false).await?
        } else {
            None
        };
        let before = self.read_blob(before_blob.as_ref()).await?;
        let after = if entry.section == "staged" {
            self.read_blob(after_blob.as_ref()).await?
        } else {
            self.read_worktree(&entry.path).await?
        };
        let image = [(&before, old_path), (&after, entry.path.as_str())]
            .iter()
            .any(|(side, path)| raster_mime(&side.bytes).is_some() || raster_extension(path));
        let mut truncated = before.truncated || after.truncated;
        let mut message: Option<&str> = None;
        let mut diff = String::new();
        let kind = if before.unsupported || after.unsupported {
            message = Some("符号链接、目录和子模块不提供内容预览，避免读取仓库之外的文件。");
            "unsupported"
        } else if old_path.to_ascii_lowercase().ends_with(".svg")
            || entry.path.to_ascii_lowercase().ends_with(".svg")
        {
            message = Some("SVG 暂不提供预览，避免执行图像中包含的活动内容。");
            "unsupported"
        } else if image {
            if truncated {
                message = Some("图片超过每侧 4 MiB 的预览上限，仅显示类型和大小。");
            } else if [&before, &after]
                .iter()
                .any(|side| side.exists && raster_mime(&side.bytes).is_none())
            {
                message = Some("文件内容不是受支持的 PNG、JPEG、GIF 或 WebP 图片。");
            }
            "image"
        } else if truncated {
            message = Some(
                "文件超过每侧 4 MiB 的读取上限，不生成内容预览，仅显示类型和大小；请在电脑端查看完整差异。",
            );
            "unsupported"
        } else if !is_text(&before.bytes) || !is_text(&after.bytes) {
            message = Some("该资源为二进制文件，无法显示文本差异。");
            "binary"
        } else if before.size > MAX_TEXT_BYTES as u64 || after.size > MAX_TEXT_BYTES as u64 {
            truncated = true;
            message = Some(
                "文本超过每侧 512 KiB 的差异上限，不生成文本差异，仅显示类型和大小；请在电脑端查看完整差异。",
            );
            "text"
        } else {
            let output = if entry.section == "untracked" || (conflict && before_blob.is_none()) {
                // A delete/modify conflict can have no ours (stage 2). The
                // selected before side is absent, so the worktree is an addition.
                added_diff(&entry.path, &after.bytes)
            } else {
                let mut args = vec![
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--ignore-submodules=all",
                    "--find-renames=50%",
                    "--src-prefix=a/",
                    "--dst-prefix=b/",
                ];
                if entry.section == "staged" {
                    args.push("--cached");
                    if let Some(head) = self.head.as_deref() {
                        args.push(head);
                    }
                } else if conflict {
                    // Match index_blob's stage-2 selection with a regular
                    // two-sided patch. --cc produces @@@ hunks which cannot be
                    // rendered as the standard @@ line-numbered diff contract.
                    args.push("--ours");
                }
                args.extend(["--", &entry.path]);
                if old_path != entry.path {
                    args.push(old_path);
                }
                let output = git(&self.root, &args, MAX_DIFF_BYTES, &self.authorization).await?;
                if !output.success && !output.truncated {
                    return Err(ApiError::new(
                        409,
                        "文件状态已变化或无法生成差异，请刷新后重试",
                    ));
                }
                output
            };
            diff = String::from_utf8_lossy(&output.bytes).into_owned();
            truncated |= output.truncated;
            if truncated {
                message = Some("差异超过 512 KiB，已截断；请在电脑端查看完整差异。");
            }
            "text"
        };
        // Reject stale selections and changing source identities rather than returning a
        // preview from a different section or a mixture of two index versions.
        let current = self.snapshot().await?;
        if !current.files.iter().any(|current| current == entry)
            || (entry.section == "staged"
                && self.index_blob(&entry.path, false).await? != after_blob)
            || (entry.section == "unstaged"
                && self.index_blob(old_path, conflict).await? != before_blob)
        {
            return Err(ApiError::new(
                409,
                "文件变更状态在读取期间更新，请刷新后重试",
            ));
        }
        if let Some(stamp) = &after.stamp {
            self.authorization
                .check()
                .await
                .map_err(super::authorization_error)?;
            let root = self.root.clone();
            let path = entry.path.clone();
            let current = tokio::task::spawn_blocking(move || local_stamp(&root, &path))
                .await
                .map_err(|_| ApiError::new(503, "无法校验文件状态"))??;
            if current.as_ref() != Some(stamp) {
                return Err(ApiError::new(409, "文件内容在读取期间更新，请刷新后重试"));
            }
        }
        Ok(json!({
            "ok": true,
            "root": self.root.to_string_lossy(),
            "path": entry.path,
            "oldPath": entry.old_path,
            "section": entry.section,
            "kind": kind,
            "diff": diff,
            "before": before.public(kind == "image", old_path),
            "after": after.public(kind == "image", &entry.path),
            "truncated": truncated,
            "message": message,
        }))
    }
}

fn parse_status(bytes: &[u8], input_truncated: bool) -> Result<Snapshot, ApiError> {
    // A capped command may stop inside a filename. Never turn that tail into a
    // selectable path, including the old-path half of a rename.
    let bytes = if input_truncated {
        &bytes[..bytes
            .iter()
            .rposition(|byte| *byte == 0)
            .map_or(0, |index| index + 1)]
    } else {
        bytes
    };
    let mut records = bytes.split(|byte| *byte == 0);
    let mut files = Vec::new();
    let mut truncated = input_truncated;
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(ApiError::new(503, "Git 状态返回了无效条目"));
        }
        let x = record[0];
        let y = record[1];
        let renamed = matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C');
        let old_path = if renamed {
            let Some(old) = records.next() else {
                if input_truncated {
                    break;
                }
                return Err(ApiError::new(503, "Git 重命名条目不完整"));
            };
            Some(std::str::from_utf8(old))
        } else {
            None
        };
        let Ok(path) = std::str::from_utf8(&record[3..]) else {
            truncated = true;
            continue;
        };
        let old_path = match old_path {
            Some(Ok(path)) => Some(path),
            Some(Err(_)) => {
                truncated = true;
                continue;
            }
            None => None,
        };
        if !safe_relative_path(path) || old_path.is_some_and(|path| !safe_relative_path(path)) {
            truncated = true;
            continue;
        }
        let mut push = |section: &str, status: u8, old_path: Option<&str>| {
            if files.len() < MAX_FILES {
                files.push(ChangeEntry {
                    path: path.to_owned(),
                    old_path: old_path.map(str::to_owned),
                    status: char::from(status).to_string(),
                    section: section.to_owned(),
                });
            } else {
                truncated = true;
            }
        };
        if x == b'?' && y == b'?' {
            push("untracked", b'A', None);
        } else if x == b'U' || y == b'U' || matches!([x, y], [b'A', b'A'] | [b'D', b'D']) {
            push("unstaged", b'U', None);
        } else {
            if x != b' ' && x != b'!' {
                push(
                    "staged",
                    x,
                    matches!(x, b'R' | b'C').then_some(old_path).flatten(),
                );
            }
            if y != b' ' && y != b'!' {
                push(
                    "unstaged",
                    y,
                    matches!(y, b'R' | b'C').then_some(old_path).flatten(),
                );
            }
        }
    }
    Ok(Snapshot { files, truncated })
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 16 * 1024
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.eq_ignore_ascii_case(".git")
                && !part.ends_with([' ', '.'])
        })
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn require_complete(output: &GitOutput) -> Result<(), ApiError> {
    if !output.success || output.truncated {
        return Err(ApiError::new(503, "Git 文件读取失败或返回结果超过上限"));
    }
    Ok(())
}

async fn git(
    cwd: &Path,
    args: &[&str],
    limit: usize,
    authorization: &RemoteAuthorization,
) -> Result<GitOutput, ApiError> {
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    git_configured(cwd, args, limit, Some(authorization)).await
}

#[cfg(test)]
async fn git_unchecked(cwd: &Path, args: &[&str], limit: usize) -> Result<GitOutput, ApiError> {
    git_configured(cwd, args, limit, None).await
}

async fn git_configured(
    cwd: &Path,
    args: &[&str],
    limit: usize,
    authorization: Option<&RemoteAuthorization>,
) -> Result<GitOutput, ApiError> {
    let mut config = Vec::new();
    // status/diff can invoke arbitrary clean/process filters while hashing a
    // worktree file. Clear every configured driver, not just the common LFS one.
    if matches!(args.first(), Some(&"status") | Some(&"diff")) {
        let filters = git_process(
            cwd,
            &[
                "config",
                "--null",
                "--name-only",
                "--get-regexp",
                r"^filter\..*\.(clean|smudge|process|required)$",
            ],
            64 * 1024,
            &[],
        )
        .await?;
        if filters.truncated {
            return Err(ApiError::new(503, "Git 过滤器配置过多，无法安全读取变更"));
        }
        for key in filters
            .bytes
            .split(|byte| *byte == 0)
            .filter(|key| !key.is_empty())
        {
            let key = std::str::from_utf8(key)
                .map_err(|_| ApiError::new(503, "Git 过滤器配置编码无效"))?;
            config.push(format!(
                "{key}={}",
                if key.ends_with(".required") {
                    "false"
                } else {
                    ""
                }
            ));
        }
    }
    if let Some(authorization) = authorization {
        authorization
            .check()
            .await
            .map_err(super::authorization_error)?;
    }
    git_process(cwd, args, limit, &config).await
}

async fn git_process(
    cwd: &Path,
    args: &[&str],
    limit: usize,
    config: &[String],
) -> Result<GitOutput, ApiError> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
        .args(["-c", "core.fsmonitor=false"])
        .args([
            "-c",
            if cfg!(windows) {
                "core.hooksPath=NUL"
            } else {
                "core.hooksPath=/dev/null"
            },
        ])
        .args(["-c", "diff.external="])
        .args(["-c", "core.quotePath=false"])
        .args(["-c", "status.submoduleSummary=false"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for setting in config {
        command.args(["-c", setting]);
    }
    command.args(args);
    // Ignore inherited Git redirection, configuration injection and helper settings.
    for (key, _) in std::env::vars_os().filter(|(key, _)| {
        key.to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
    }) {
        command.env_remove(key);
    }
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|_| ApiError::new(503, "无法启动 Git，请在电脑端安装 Git 并确认 PATH 可用"))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let read = async {
        let (output, error, status) = tokio::join!(
            bounded_read(stdout, limit),
            bounded_read(stderr, MAX_STDERR_BYTES),
            child.wait(),
        );
        let (bytes, truncated) = output.map_err(|_| ApiError::new(503, "无法读取 Git 命令结果"))?;
        let (_, stderr_truncated) =
            error.map_err(|_| ApiError::new(503, "无法读取 Git 错误结果"))?;
        let status = status.map_err(|_| ApiError::new(503, "无法等待 Git 命令完成"))?;
        if stderr_truncated && !truncated {
            return Err(ApiError::new(
                503,
                "Git 错误输出超过上限，请在电脑端检查仓库",
            ));
        }
        Ok(GitOutput {
            bytes,
            success: status.success(),
            truncated,
        })
    };
    match tokio::time::timeout(GIT_TIMEOUT, read).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(ApiError::new(504, "Git 读取超时，请在电脑端检查仓库后重试"))
        }
    }
}

async fn bounded_read(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

fn safe_local_path(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    if !safe_relative_path(relative) {
        return Err(ApiError::new(400, "文件路径无效"));
    }
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(ApiError::new(422, "不读取符号链接或 Windows 重解析点"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(ApiError::new(403, "无法安全访问该文件路径")),
        }
    }
    // Build the complete path even when an intermediate directory was deleted.
    let path = root.join(relative);
    if let Ok(canonical) = std::fs::canonicalize(&path) {
        if !canonical.starts_with(root) {
            return Err(ApiError::new(403, "文件路径已越出当前 Git 仓库"));
        }
    }
    Ok(path)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn open_local(root: &Path, relative: &str) -> Result<Option<File>, ApiError> {
    let path = safe_local_path(root, relative)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if is_link(&metadata) || !metadata.is_file() => {
            return Err(ApiError::new(422, "符号链接、目录和特殊文件不提供内容预览"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ApiError::new(403, "文件属性无法安全读取")),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ApiError::new(403, "文件无法读取，请确认文件权限及变更状态")),
    };
    let metadata = file
        .metadata()
        .map_err(|_| ApiError::new(503, "无法读取文件属性"))?;
    if is_link(&metadata) {
        return Err(ApiError::new(422, "不读取符号链接或 Windows 重解析点"));
    }
    verify_open_file(&file, &path, root)?;
    Ok(Some(file))
}

#[cfg(windows)]
fn verify_open_file(file: &File, _: &Path, root: &Path) -> Result<(), ApiError> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW};
    let mut buffer = vec![0u16; 32_768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut buffer,
            FILE_NAME_NORMALIZED,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(ApiError::new(403, "无法确认文件实际路径"));
    }
    let actual = PathBuf::from(OsString::from_wide(&buffer[..length]));
    if !actual.starts_with(root) {
        return Err(ApiError::new(403, "文件实际路径已越出当前 Git 仓库"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_open_file(_: &File, path: &Path, root: &Path) -> Result<(), ApiError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| ApiError::new(409, "文件路径在读取期间更新，请刷新后重试"))?;
    if !canonical.starts_with(root) {
        return Err(ApiError::new(403, "文件实际路径已越出当前 Git 仓库"));
    }
    Ok(())
}

fn stamp(metadata: &Metadata) -> FileStamp {
    FileStamp {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    }
}

fn read_local(root: &Path, relative: &str) -> Result<Side, ApiError> {
    let file = match open_local(root, relative) {
        Ok(file) => file,
        Err(error) if error.code == 422 => {
            return Ok(Side {
                exists: true,
                size: 0,
                bytes: Vec::new(),
                truncated: false,
                unsupported: true,
                stamp: None,
            });
        }
        Err(error) => return Err(error),
    };
    let Some(mut file) = file else {
        return Ok(Side::absent());
    };
    let metadata = file
        .metadata()
        .map_err(|_| ApiError::new(503, "无法读取文件属性"))?;
    let mut side = Side {
        exists: true,
        size: metadata.len(),
        bytes: Vec::new(),
        truncated: metadata.len() > MAX_IMAGE_BYTES as u64,
        unsupported: !metadata.is_file(),
        stamp: Some(stamp(&metadata)),
    };
    if !side.truncated && !side.unsupported {
        file.by_ref()
            .take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut side.bytes)
            .map_err(|_| ApiError::new(503, "无法读取工作目录文件内容"))?;
        if side.bytes.len() > MAX_IMAGE_BYTES
            || side.bytes.len() as u64 != metadata.len()
            || stamp(
                &file
                    .metadata()
                    .map_err(|_| ApiError::new(503, "无法校验文件属性"))?,
            ) != stamp(&metadata)
        {
            return Err(ApiError::new(409, "文件在读取期间更新，请刷新后重试"));
        }
    }
    Ok(side)
}

fn local_stamp(root: &Path, relative: &str) -> Result<Option<FileStamp>, ApiError> {
    open_local(root, relative)?
        .map(|file| file.metadata().map(|metadata| stamp(&metadata)))
        .transpose()
        .map_err(|_| ApiError::new(503, "无法校验文件属性"))
}

fn raster_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn raster_extension(path: &str) -> bool {
    raster_extension_mime(path).is_some()
}

fn raster_extension_mime(path: &str) -> Option<&'static str> {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            _ => None,
        })
}

fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

fn added_diff(path: &str, bytes: &[u8]) -> GitOutput {
    let text = String::from_utf8_lossy(bytes);
    let count = text.lines().count();
    let label = serde_json::to_string(&format!("b/{path}")).unwrap_or_default();
    let mut diff = format!("--- /dev/null\n+++ {label}\n");
    if count > 0 {
        diff.push_str(&format!("@@ -0,0 +1,{count} @@\n"));
    }
    let mut truncated = false;
    for line in text.split_inclusive('\n') {
        let missing_newline = if line.ends_with('\n') {
            ""
        } else {
            "\n\\ No newline at end of file\n"
        };
        if diff.len() + line.len() + 1 + missing_newline.len() > MAX_DIFF_BYTES {
            truncated = true;
            break;
        }
        diff.push('+');
        diff.push_str(line);
        diff.push_str(missing_newline);
    }
    GitOutput {
        bytes: diff.into_bytes(),
        success: true,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("remote-workspace-")
            .tempdir_in(root)
            .unwrap()
    }

    struct WorkspaceTestBridge {
        cwd: PathBuf,
    }

    #[async_trait::async_trait]
    impl super::super::NativeRemoteBridge for WorkspaceTestBridge {
        async fn call(&self, action: &str, mut payload: Value) -> anyhow::Result<Value> {
            anyhow::ensure!(action == "workspaceContext", "此测试仅解析工作目录");
            payload["ok"] = json!(true);
            payload["hostId"] = json!("local");
            payload["cwd"] = json!(self.cwd);
            Ok(payload)
        }
    }

    async fn fixture_repo() -> (tempfile::TempDir, tempfile::TempDir, Repository) {
        use super::super::{
            Device, RemoteAccessController, RemoteAccessSettings, configure_access_key, hash,
            now_ms,
        };
        use std::sync::Arc;
        use std::sync::atomic::Ordering;
        let fixture = fixture_dir();
        let root = std::fs::canonicalize(fixture.path()).unwrap();
        assert!(
            git_unchecked(&root, &["init", "--quiet"], 16 * 1024)
                .await
                .unwrap()
                .success
        );
        let state = fixture_dir();
        configure_access_key(state.path(), "workspace-detail-test-key").unwrap();
        let controller = RemoteAccessController::new(
            Arc::new(WorkspaceTestBridge { cwd: root.clone() }),
            state.path().to_owned(),
        )
        .unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            read_only: true,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        let device = {
            let mut auth = controller.auth.lock().await;
            let device = Device {
                id: "workspace-detail-device".into(),
                name: "工作区详情回归".into(),
                token_hash: hash("workspace-detail-cookie"),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 120_000,
                last_seen_at_ms: now_ms(),
                key_id: auth.access_key.as_ref().unwrap().id.clone(),
                public_origin: None,
            };
            auth.devices.push(device.clone());
            device
        };
        let repo = Repository::discover(root, controller.authorization(device, None))
            .await
            .unwrap();
        (fixture, state, repo)
    }

    async fn fixture_detail(repo: &Repository, path: &str, section: &str) -> Value {
        let entry = repo
            .snapshot()
            .await
            .unwrap()
            .files
            .into_iter()
            .find(|entry| entry.path == path && entry.section == section)
            .unwrap();
        repo.detail(&entry).await.unwrap()
    }

    fn fixture_web_state(repo: &Repository) -> (WebState, HeaderMap) {
        let state = WebState {
            controller: repo.authorization.controller.clone(),
            settings: super::super::RemoteAccessSettings::default(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{}=workspace-detail-cookie", super::super::COOKIE_NAME)
                .parse()
                .unwrap(),
        );
        (state, headers)
    }

    async fn fixture_response_json(response: Response) -> Value {
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn fixture_conflict_index(repo: &Repository, path: &str, stages: &[(u8, &str)]) {
        use tokio::io::AsyncWriteExt;
        let mut entries = Vec::new();
        for (stage, content) in stages {
            let source = format!("conflict-stage-{stage}.txt");
            std::fs::write(repo.root.join(&source), content.as_bytes()).unwrap();
            let output = git_unchecked(&repo.root, &["hash-object", "-w", "--", &source], 256)
                .await
                .unwrap();
            assert!(output.success);
            let oid = String::from_utf8(output.bytes).unwrap().trim().to_owned();
            assert!(valid_oid(&oid));
            entries.push((*stage, oid));
        }
        // The initial zero-mode entry removes earlier stages when the same
        // fixture is changed from UU to a delete/modify conflict.
        let mut input = format!("0 {}\t{path}\0", "0".repeat(entries[0].1.len()));
        for (stage, oid) in entries {
            input.push_str(&format!("100644 {oid} {stage}\t{path}\0"));
        }
        let mut command = Command::new("git");
        command
            .args(["-c", "core.fsmonitor=false", "-c"])
            .arg(if cfg!(windows) {
                "core.hooksPath=NUL"
            } else {
                "core.hooksPath=/dev/null"
            })
            .args(["update-index", "-z", "--index-info"])
            .current_dir(&repo.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, _) in std::env::vars_os().filter(|(key, _)| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
        }) {
            command.env_remove(key);
        }
        command.env("GIT_CONFIG_NOSYSTEM", "1").env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).await.unwrap();
        drop(stdin);
        let output = tokio::time::timeout(GIT_TIMEOUT, child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn workspace_status_preserves_nul_paths_and_both_sections() {
        let snapshot = parse_status(
            b"MM \xe4\xb8\xad\xe6\x96\x87 file.txt\0R  new\nname.txt\0old name.txt\0?? literal[1].txt\0UU conflict.txt\0",
            false,
        ).unwrap();
        assert_eq!(snapshot.files.len(), 5);
        assert_eq!(snapshot.files[0].path, "中文 file.txt");
        assert_eq!(snapshot.files[0].section, "staged");
        assert_eq!(snapshot.files[1].section, "unstaged");
        assert_eq!(snapshot.files[2].path, "new\nname.txt");
        assert_eq!(snapshot.files[2].old_path.as_deref(), Some("old name.txt"));
        assert_eq!(snapshot.files[3].section, "untracked");
        assert_eq!(snapshot.files[4].status, "U");
    }

    #[test]
    fn workspace_if_revision_requires_lowercase_sha256() {
        assert_eq!(requested_revision(&json!({})).unwrap(), None);
        let valid = "0123456789abcdef".repeat(4);
        let payload = json!({"ifRevision": valid});
        assert_eq!(requested_revision(&payload).unwrap(), Some(valid.as_str()));
        for invalid in [
            Value::Null,
            json!(0),
            json!(true),
            json!([]),
            json!({}),
            json!(""),
            json!("a".repeat(63)),
            json!("a".repeat(65)),
            json!("A".repeat(64)),
            json!("g".repeat(64)),
            json!(format!("{} ", "a".repeat(63))),
            json!("中".repeat(64)),
        ] {
            assert_eq!(
                requested_revision(&json!({"ifRevision": invalid}))
                    .unwrap_err()
                    .code,
                400
            );
        }
    }

    #[test]
    fn workspace_revision_is_stable_root_scoped_and_compact() {
        let payload = json!({
            "ok": true,
            "root": "repo-one",
            "path": "large.txt",
            "section": "unstaged",
            "kind": "text",
            "diff": "+content\n".repeat(1_000),
            "before": {"exists": true, "size": 7},
            "after": {"exists": true, "size": 8},
            "truncated": false,
            "message": null,
        });
        let full = revision_payload(payload.clone(), None).unwrap();
        let revision = full["revision"].as_str().unwrap();
        assert!(requested_revision(&json!({"ifRevision": revision})).is_ok());
        assert_eq!(revision_payload(payload.clone(), None).unwrap(), full);
        assert_eq!(revision_payload(full.clone(), None).unwrap(), full);
        let compact = revision_payload(payload.clone(), Some(revision)).unwrap();
        assert_eq!(
            compact,
            json!({"ok":true,"notModified":true,"revision":revision,"root":"repo-one"})
        );
        assert!(serde_json::to_vec(&compact).unwrap().len() < 180);
        let mut other_root = payload;
        other_root["root"] = json!("repo-two");
        let changed = revision_payload(other_root, Some(revision)).unwrap();
        assert_ne!(changed["revision"], full["revision"]);
        assert!(changed.get("notModified").is_none());
        assert!(changed["diff"].is_string());
    }

    #[test]
    fn workspace_paths_reject_escape_and_git_metadata() {
        for path in [
            "/etc/passwd",
            "C:/secret",
            "../secret",
            "a/../b",
            ".git/config",
            "a/.GIT/index",
            "a\\b",
            "a:b",
            "a//b",
            "a./b",
        ] {
            assert!(!safe_relative_path(path), "{path}");
        }
        assert!(safe_relative_path("目录/a b\n[1].txt"));
    }

    #[test]
    fn workspace_status_drops_incomplete_truncated_rename() {
        let snapshot = parse_status(b" M first.txt\0R  second.txt\0old", true).unwrap();
        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot.truncated);
    }

    #[test]
    fn workspace_raster_mime_never_accepts_svg_or_html() {
        assert_eq!(raster_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(raster_mime(b"<svg onload='alert(1)'>"), None);
        assert_eq!(raster_mime(b"<html>"), None);
        assert!(!is_text(b"a\0b"));
    }

    #[test]
    fn workspace_added_diff_is_bounded() {
        let bytes = vec![b'a'; MAX_DIFF_BYTES];
        let diff = added_diff("large.txt", &bytes);
        assert!(diff.truncated);
        assert!(diff.bytes.len() <= MAX_DIFF_BYTES);
        let diff = added_diff("new.txt", b"one\ntwo");
        let text = String::from_utf8(diff.bytes).unwrap();
        assert!(text.contains("@@ -0,0 +1,2 @@"));
        assert!(text.contains("+one\n+two\n\\ No newline at end of file"));
    }

    #[tokio::test]
    async fn workspace_reader_stops_at_limit() {
        let (bytes, truncated) = bounded_read(&b"abcdef"[..], 3).await.unwrap();
        assert_eq!(bytes, b"abc");
        assert!(truncated);
    }

    #[test]
    fn workspace_local_reads_are_bounded_and_detect_missing_files() {
        let fixture = fixture_dir();
        let root = std::fs::canonicalize(fixture.path()).unwrap();
        assert!(!read_local(&root, "missing.txt").unwrap().exists);
        std::fs::write(root.join("small.txt"), b"content").unwrap();
        assert_eq!(read_local(&root, "small.txt").unwrap().bytes, b"content");
        let file = File::create(root.join("large.bin")).unwrap();
        file.set_len(MAX_IMAGE_BYTES as u64 + 1).unwrap();
        let side = read_local(&root, "large.bin").unwrap();
        assert!(side.truncated);
        assert!(side.bytes.is_empty());
    }

    #[test]
    fn workspace_image_sides_preserve_missing_and_actual_mime() {
        let fixture = fixture_dir();
        let root = std::fs::canonicalize(fixture.path()).unwrap();
        std::fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\npayload").unwrap();
        let side = read_local(&root, "image.png").unwrap();
        let value = side.public(true, "image.png");
        assert_eq!(value["mime"], "image/png");
        assert!(
            value["dataUrl"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let value = Side::absent().public(true, "image.png");
        assert_eq!(value["exists"], false);
        assert!(value["dataUrl"].is_null());
    }

    #[tokio::test]
    async fn workspace_git_empty_head_index_and_literal_paths() {
        let fixture = fixture_dir();
        let root = std::fs::canonicalize(fixture.path()).unwrap();
        let output = git_unchecked(&root, &["init", "--quiet"], 16 * 1024)
            .await
            .unwrap();
        assert!(output.success);
        let name = "中文 [1] name.txt";
        std::fs::write(root.join(name), b"before\n").unwrap();
        let output = git_unchecked(
            &root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            MAX_STATUS_BYTES,
        )
        .await
        .unwrap();
        let snapshot = parse_status(&output.bytes, output.truncated).unwrap();
        assert_eq!(snapshot.files[0].path, name);
        assert_eq!(snapshot.files[0].section, "untracked");
        let output = git_unchecked(&root, &["add", "--", name], 16 * 1024)
            .await
            .unwrap();
        assert!(output.success);
        let original_index = std::fs::read(root.join(".git/index")).unwrap();
        std::fs::write(root.join(name), b"after\n").unwrap();
        // A failing configured clean driver must never execute during remote reads.
        std::fs::write(root.join(".gitattributes"), b"* filter=workspace-test\n").unwrap();
        for (key, value) in [
            (
                "filter.workspace-test.clean",
                "workspace-filter-that-must-not-execute",
            ),
            ("filter.workspace-test.required", "true"),
        ] {
            let output = git_unchecked(&root, &["config", key, value], 16 * 1024)
                .await
                .unwrap();
            assert!(output.success);
        }
        let output = git_unchecked(
            &root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            MAX_STATUS_BYTES,
        )
        .await
        .unwrap();
        assert!(output.success);
        let snapshot = parse_status(&output.bytes, output.truncated).unwrap();
        assert!(
            snapshot.files.iter().any(|entry| entry.path == name
                && entry.section == "staged"
                && entry.status == "A")
        );
        assert!(
            snapshot.files.iter().any(|entry| entry.path == name
                && entry.section == "unstaged"
                && entry.status == "M")
        );
        let output = git_unchecked(
            &root,
            &[
                "diff",
                "--cached",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                name,
            ],
            MAX_DIFF_BYTES,
        )
        .await
        .unwrap();
        assert!(output.success);
        assert!(String::from_utf8_lossy(&output.bytes).contains("+before"));
        assert_eq!(
            std::fs::read(root.join(".git/index")).unwrap(),
            original_index
        );
        std::fs::remove_file(root.join(name)).unwrap();
        let output = git_unchecked(
            &root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            MAX_STATUS_BYTES,
        )
        .await
        .unwrap();
        let snapshot = parse_status(&output.bytes, output.truncated).unwrap();
        assert!(
            snapshot.files.iter().any(|entry| entry.path == name
                && entry.section == "unstaged"
                && entry.status == "D")
        );
    }

    #[tokio::test]
    async fn workspace_details_use_index_worktree_images_deletions_and_binary() {
        let (_fixture, _state, repo) = fixture_repo().await;
        std::fs::write(repo.root.join("notes.txt"), b"index version\n").unwrap();
        std::fs::write(repo.root.join("deleted.txt"), b"deleted index content\n").unwrap();
        std::fs::write(repo.root.join("binary.bin"), b"\0index").unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0u8, 0, 255, 255]))
            .save(repo.root.join("picture.png"))
            .unwrap();
        let added = git_unchecked(
            &repo.root,
            &[
                "add",
                "--",
                "notes.txt",
                "deleted.txt",
                "binary.bin",
                "picture.png",
            ],
            16 * 1024,
        )
        .await
        .unwrap();
        assert!(added.success);
        let original_index = std::fs::read(repo.root.join(".git/index")).unwrap();
        std::fs::write(repo.root.join("notes.txt"), b"working version\n").unwrap();
        std::fs::write(repo.root.join("binary.bin"), b"\0working").unwrap();
        std::fs::remove_file(repo.root.join("deleted.txt")).unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0u8, 255, 0, 255]))
            .save(repo.root.join("picture.png"))
            .unwrap();

        let staged = fixture_detail(&repo, "notes.txt", "staged").await;
        assert_eq!(staged["kind"], "text");
        assert_eq!(staged["before"]["exists"], false);
        assert!(staged["diff"].as_str().unwrap().contains("+index version"));
        assert!(!staged["diff"].as_str().unwrap().contains("working version"));
        let unstaged = fixture_detail(&repo, "notes.txt", "unstaged").await;
        assert!(
            unstaged["diff"]
                .as_str()
                .unwrap()
                .contains("-index version")
        );
        assert!(
            unstaged["diff"]
                .as_str()
                .unwrap()
                .contains("+working version")
        );

        let unstaged_image = fixture_detail(&repo, "picture.png", "unstaged").await;
        assert_eq!(unstaged_image["kind"], "image");
        let pixel = |value: &Value| {
            let encoded = value["dataUrl"]
                .as_str()
                .unwrap()
                .split_once(',')
                .unwrap()
                .1;
            let bytes = STANDARD.decode(encoded).unwrap();
            image::load_from_memory(&bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0
        };
        assert_eq!(pixel(&unstaged_image["before"]), [0, 0, 255, 255]);
        assert_eq!(pixel(&unstaged_image["after"]), [0, 255, 0, 255]);
        let staged_image = fixture_detail(&repo, "picture.png", "staged").await;
        assert_eq!(staged_image["before"]["exists"], false);
        assert_eq!(pixel(&staged_image["after"]), [0, 0, 255, 255]);

        let deleted = fixture_detail(&repo, "deleted.txt", "unstaged").await;
        assert_eq!(deleted["before"]["exists"], true);
        assert_eq!(deleted["after"]["exists"], false);
        assert!(
            deleted["diff"]
                .as_str()
                .unwrap()
                .contains("-deleted index content")
        );
        let binary = fixture_detail(&repo, "binary.bin", "unstaged").await;
        assert_eq!(binary["kind"], "binary");
        assert_eq!(binary["before"]["size"], 6);
        assert_eq!(binary["after"]["size"], 8);
        assert_eq!(binary["after"]["mime"], "application/octet-stream");
        assert!(binary["after"]["dataUrl"].is_null());
        assert_eq!(
            std::fs::read(repo.root.join(".git/index")).unwrap(),
            original_index
        );
    }

    #[tokio::test]
    async fn workspace_large_text_and_replaced_directory_are_explicit() {
        let (_fixture, _state, repo) = fixture_repo().await;
        std::fs::write(repo.root.join("large.txt"), vec![b'a'; MAX_TEXT_BYTES + 1]).unwrap();
        let detail = fixture_detail(&repo, "large.txt", "untracked").await;
        assert_eq!(detail["kind"], "text");
        assert_eq!(detail["truncated"], true);
        assert_eq!(detail["diff"], "");
        assert!(
            detail["message"]
                .as_str()
                .unwrap()
                .contains("不生成文本差异")
        );

        std::fs::write(repo.root.join("replaced.txt"), b"index content\n").unwrap();
        assert!(
            git_unchecked(&repo.root, &["add", "--", "replaced.txt"], 16 * 1024)
                .await
                .unwrap()
                .success
        );
        let index = git_unchecked(
            &repo.root,
            &["ls-files", "--stage", "-z", "--", "replaced.txt"],
            16 * 1024,
        )
        .await
        .unwrap();
        let index = String::from_utf8(index.bytes).unwrap();
        let oid = index.split_whitespace().nth(1).unwrap();
        assert!(valid_oid(oid));
        // An index symlink requires no OS symlink privilege and its target blob
        // must remain unreadable through this preview API.
        let link = format!("120000,{oid},staged-link");
        assert!(
            git_unchecked(
                &repo.root,
                &["update-index", "--add", "--cacheinfo", &link],
                16 * 1024
            )
            .await
            .unwrap()
            .success
        );
        let symlink = fixture_detail(&repo, "staged-link", "staged").await;
        assert_eq!(symlink["kind"], "unsupported");
        assert!(symlink["after"]["dataUrl"].is_null());
        std::fs::remove_file(repo.root.join("replaced.txt")).unwrap();
        std::fs::create_dir(repo.root.join("replaced.txt")).unwrap();
        let detail = fixture_detail(&repo, "replaced.txt", "unstaged").await;
        assert_eq!(detail["kind"], "unsupported");
        assert!(detail["message"].as_str().unwrap().contains("目录"));
    }

    #[tokio::test]
    async fn workspace_conflict_diff_uses_ours_and_standard_two_sided_hunks() {
        let (_fixture, _state, repo) = fixture_repo().await;
        let path = "冲突 [1].txt";
        let stages = [
            (1, "common\nbase\n"),
            (2, "common\nours\n"),
            (3, "common\ntheirs\n"),
        ];
        fixture_conflict_index(&repo, path, &stages).await;
        std::fs::write(repo.root.join(path), b"common\nresolved\n").unwrap();
        let index = std::fs::read(repo.root.join(".git/index")).unwrap();

        // Reproduce the rejected combined format against a real multistage
        // index, then verify the API's replacement uses the stage-2 old side.
        let combined = git_unchecked(
            &repo.root,
            &["diff", "--cc", "--no-ext-diff", "--no-textconv", "--", path],
            MAX_DIFF_BYTES,
        )
        .await
        .unwrap();
        assert!(combined.success);
        assert!(String::from_utf8_lossy(&combined.bytes).contains("@@@ "));
        let detail = fixture_detail(&repo, path, "unstaged").await;
        assert_eq!(detail["kind"], "text");
        assert_eq!(detail["before"]["exists"], true);
        assert_eq!(detail["before"]["size"], stages[1].1.len());
        assert_eq!(detail["after"]["exists"], true);
        let diff = detail["diff"].as_str().unwrap();
        assert!(diff.lines().any(|line| line.starts_with("@@ -")));
        assert!(diff.lines().any(|line| line == "-ours"));
        assert!(diff.lines().any(|line| line == "+resolved"));
        assert!(!diff.contains("@@@"));
        assert!(!diff.contains("diff --cc"));
        assert!(!diff.contains("theirs"));
        assert_eq!(std::fs::read(repo.root.join(".git/index")).unwrap(), index);

        // Ours can be absent in a delete/modify conflict. Its preview must have
        // the same missing before side and addition semantics as its metadata.
        fixture_conflict_index(&repo, path, &[(1, stages[0].1), (3, stages[2].1)]).await;
        let detail = fixture_detail(&repo, path, "unstaged").await;
        assert_eq!(detail["before"]["exists"], false);
        let diff = detail["diff"].as_str().unwrap();
        assert!(diff.lines().any(|line| line.starts_with("@@ -0,0 ")));
        assert!(diff.lines().any(|line| line == "+common"));
        assert!(diff.lines().any(|line| line == "+resolved"));
        assert!(!diff.contains("@@@"));

        fixture_conflict_index(&repo, path, &stages).await;
        std::fs::remove_file(repo.root.join(path)).unwrap();
        let detail = fixture_detail(&repo, path, "unstaged").await;
        assert_eq!(detail["after"]["exists"], false);
        let diff = detail["diff"].as_str().unwrap();
        assert!(diff.lines().any(|line| line.starts_with("@@ -")));
        assert!(diff.lines().any(|line| line == "-ours"));
        assert!(!diff.contains("@@@"));
    }

    #[tokio::test]
    async fn workspace_revisions_track_visible_lists_and_same_length_content() {
        let (_fixture, _state, repo) = fixture_repo().await;
        std::fs::write(repo.root.join("notes.txt"), b"first\n").unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0u8, 0, 255, 255]))
            .save(repo.root.join("picture.png"))
            .unwrap();
        let list = revision_payload(repo.listing().await.unwrap(), None).unwrap();
        assert_eq!(
            list,
            revision_payload(repo.listing().await.unwrap(), None).unwrap()
        );
        let text =
            revision_payload(fixture_detail(&repo, "notes.txt", "untracked").await, None).unwrap();
        let image = revision_payload(
            fixture_detail(&repo, "picture.png", "untracked").await,
            None,
        )
        .unwrap();
        assert_eq!(text["root"], list["root"]);
        assert_eq!(
            image,
            revision_payload(
                fixture_detail(&repo, "picture.png", "untracked").await,
                None
            )
            .unwrap()
        );
        let original_size = std::fs::metadata(repo.root.join("notes.txt"))
            .unwrap()
            .len();
        std::fs::write(repo.root.join("notes.txt"), b"other\n").unwrap();
        assert_eq!(
            std::fs::metadata(repo.root.join("notes.txt"))
                .unwrap()
                .len(),
            original_size
        );
        let changed_text = revision_payload(
            fixture_detail(&repo, "notes.txt", "untracked").await,
            text["revision"].as_str(),
        )
        .unwrap();
        assert_ne!(changed_text["revision"], text["revision"]);
        assert!(changed_text["diff"].as_str().unwrap().contains("+other"));
        assert_eq!(
            list,
            revision_payload(repo.listing().await.unwrap(), None).unwrap()
        );

        image::RgbaImage::from_pixel(1, 1, image::Rgba([0u8, 255, 0, 255]))
            .save(repo.root.join("picture.png"))
            .unwrap();
        let changed_image = revision_payload(
            fixture_detail(&repo, "picture.png", "untracked").await,
            image["revision"].as_str(),
        )
        .unwrap();
        assert_ne!(changed_image["after"]["dataUrl"], image["after"]["dataUrl"]);
        assert_ne!(changed_image["revision"], image["revision"]);
        assert_eq!(
            list,
            revision_payload(repo.listing().await.unwrap(), None).unwrap()
        );
        std::fs::write(repo.root.join("new.txt"), b"new\n").unwrap();
        let changed_list =
            revision_payload(repo.listing().await.unwrap(), list["revision"].as_str()).unwrap();
        assert_ne!(changed_list["revision"], list["revision"]);
        assert!(changed_list["files"].is_array());
    }

    #[tokio::test]
    async fn workspace_conditional_handlers_still_validate_resource_and_authorization() {
        let (fixture, _state_dir, repo) = fixture_repo().await;
        let (state, headers) = fixture_web_state(&repo);
        std::fs::write(repo.root.join("notes.txt"), b"first\n").unwrap();
        let identity = json!({"threadId":"workspace-test-thread"});
        let list = fixture_response_json(
            changes(
                State(state.clone()),
                headers.clone(),
                Json(identity.clone()),
            )
            .await
            .unwrap(),
        )
        .await;
        let mut conditional_list = identity.clone();
        conditional_list["ifRevision"] = list["revision"].clone();
        let unchanged = fixture_response_json(
            changes(
                State(state.clone()),
                headers.clone(),
                Json(conditional_list.clone()),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(
            unchanged,
            json!({
                "ok": true,
                "notModified": true,
                "revision": list["revision"],
                "root": list["root"],
            })
        );
        let detail_request = json!({
            "threadId":"workspace-test-thread",
            "path":"notes.txt",
            "section":"untracked",
        });
        let detail = fixture_response_json(
            change(
                State(state.clone()),
                headers.clone(),
                Json(detail_request.clone()),
            )
            .await
            .unwrap(),
        )
        .await;
        let mut conditional_detail = detail_request.clone();
        conditional_detail["ifRevision"] = detail["revision"].clone();
        let unchanged = fixture_response_json(
            change(
                State(state.clone()),
                headers.clone(),
                Json(conditional_detail.clone()),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(unchanged.as_object().unwrap().len(), 4);
        assert_eq!(unchanged["notModified"], true);
        assert_eq!(unchanged["root"], list["root"]);
        for field in ["files", "diff", "before", "after"] {
            assert!(unchanged.get(field).is_none());
        }
        for invalid in [Value::Null, json!("A".repeat(64)), json!(7)] {
            let mut request = conditional_list.clone();
            request["ifRevision"] = invalid.clone();
            assert_eq!(
                changes(State(state.clone()), headers.clone(), Json(request))
                    .await
                    .unwrap_err()
                    .code,
                400
            );
            let mut request = conditional_detail.clone();
            request["ifRevision"] = invalid;
            assert_eq!(
                change(State(state.clone()), headers.clone(), Json(request))
                    .await
                    .unwrap_err()
                    .code,
                400
            );
        }
        let mut injected = conditional_detail.clone();
        injected["cwd"] = json!(repo.root);
        assert_eq!(
            change(State(state.clone()), headers.clone(), Json(injected))
                .await
                .unwrap_err()
                .code,
            400
        );
        std::fs::remove_file(repo.root.join("notes.txt")).unwrap();
        assert_eq!(
            change(
                State(state.clone()),
                headers.clone(),
                Json(conditional_detail.clone())
            )
            .await
            .unwrap_err()
            .code,
            409
        );
        fixture.close().unwrap();
        assert_eq!(
            changes(
                State(state.clone()),
                headers.clone(),
                Json(conditional_list.clone())
            )
            .await
            .unwrap_err()
            .code,
            404
        );
        state.controller.auth.lock().await.devices.clear();
        assert_eq!(
            changes(
                State(state.clone()),
                headers.clone(),
                Json(conditional_list)
            )
            .await
            .unwrap_err()
            .code,
            401
        );
        assert_eq!(
            change(State(state), headers, Json(conditional_detail))
                .await
                .unwrap_err()
                .code,
            401
        );
    }
}
