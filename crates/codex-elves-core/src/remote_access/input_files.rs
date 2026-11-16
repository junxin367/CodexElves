//! 客户端本地附件只能写入服务专用目录；路径、大小和目录结构在派发前统一校验。
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::Value;

use super::{ApiError, RemoteAuthorization};

// 与独立客户端 local_inputs.rs 和网页 local-inputs.js 同步；不调整原有图片 8 张 / 12 MiB 限额。
const MAX_ROOTS: usize = 16;
const MAX_ENTRIES: usize = 512;
const MAX_BYTES: usize = 12 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: String,
    name: String,
    size: u64,
    files: Vec<InputFile>,
    directories: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    path: String,
    data: String,
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(400, message)
}

fn safe_component(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    !value.is_empty()
        && value.len() <= 255
        && !matches!(value, "." | "..")
        && !value.ends_with(['.', ' '])
        && !value
            .chars()
            .any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch))
        && !matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value.split('/').count() <= 32
        && value.split('/').all(safe_component)
}

fn parse(value: &Value) -> Result<Vec<Input>, ApiError> {
    let Some(files) = value.get("files") else {
        return Ok(Vec::new());
    };
    let inputs: Vec<Input> =
        serde_json::from_value(files.clone()).map_err(|_| invalid("文件和文件夹附件参数无效"))?;
    if inputs.len() > MAX_ROOTS {
        return Err(invalid("每条消息最多添加 16 个文件或文件夹"));
    }
    let mut total = 0usize;
    let mut count = inputs.len();
    for input in &inputs {
        if !matches!(input.kind.as_str(), "file" | "folder") || !safe_component(&input.name) {
            return Err(invalid("附件名称或类型无效"));
        }
        if input.kind == "file"
            && (!input.directories.is_empty()
                || input.files.len() != 1
                || input.files[0].path != input.name)
        {
            return Err(invalid("文件附件的目录结构无效"));
        }
        let mut paths = HashSet::new();
        for path in input
            .directories
            .iter()
            .chain(input.files.iter().map(|file| &file.path))
        {
            if !safe_relative(path) || !paths.insert(path.to_lowercase()) {
                return Err(invalid("附件包含越界、重复或无效路径"));
            }
        }
        // 文件不能同时成为另一个附件项的父目录，Windows 大小写折叠后同样校验。
        let file_paths: HashSet<_> = input
            .files
            .iter()
            .map(|file| file.path.to_lowercase())
            .collect();
        for path in &paths {
            let mut parent = path.as_str();
            while let Some((prefix, _)) = parent.rsplit_once('/') {
                if file_paths.contains(prefix) {
                    return Err(invalid("附件文件与目录路径冲突"));
                }
                parent = prefix;
            }
        }
        count += input.directories.len() + input.files.len();
        if count > MAX_ENTRIES {
            return Err(invalid("每条消息最多包含 512 个文件和目录项"));
        }
        let mut size = 0usize;
        for file in &input.files {
            if file.data.len() > MAX_BYTES.div_ceil(3) * 4 {
                return Err(invalid("文件和文件夹总大小不能超过 12 MiB"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&file.data)
                .map_err(|_| invalid("附件内容编码无效"))?;
            size = size.saturating_add(bytes.len());
        }
        total = total.saturating_add(size);
        if total > MAX_BYTES {
            return Err(invalid("文件和文件夹总大小不能超过 12 MiB"));
        }
        if size as u64 != input.size {
            return Err(invalid("附件大小与实际内容不一致"));
        }
    }
    Ok(inputs)
}

pub(super) fn validate(value: &Value) -> Result<(), ApiError> {
    parse(value).map(|_| ())
}

fn plain_directory(path: &Path) -> Result<(), ApiError> {
    if !path.exists() {
        std::fs::create_dir(path).map_err(|_| invalid("无法创建附件目录"))?;
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid("无法核对附件目录"))?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if !metadata.is_dir() || metadata.is_symlink() || reparse {
        return Err(invalid("附件目录不允许符号链接或重解析点"));
    }
    Ok(())
}

fn directories(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    let mut current = root.to_owned();
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        current.push(part);
        plain_directory(&current)?;
    }
    Ok(current)
}

fn store(
    directory: &Path,
    owner: &str,
    request: &str,
    inputs: &[Input],
) -> Result<Vec<PathBuf>, ApiError> {
    let root = directory.join("remote-input-files");
    plain_directory(&root)?;
    let owner = root.join(super::hash(owner));
    plain_directory(&owner)?;
    let batch = owner.join(super::hash(request));
    plain_directory(&batch)?;
    let mut paths = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let slot = batch.join(index.to_string());
        plain_directory(&slot)?;
        let root = if input.kind == "folder" {
            let root = slot.join(&input.name);
            plain_directory(&root)?;
            for path in &input.directories {
                directories(&root, path)?;
            }
            root
        } else {
            slot.clone()
        };
        for file in &input.files {
            let (parent, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
            let parent = directories(&root, parent)?;
            let path = parent.join(name);
            // 只创建新文件，不覆盖现有文件，更不能跟随用户文件夹中的符号链接。
            use std::io::Write;
            let mut target = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| invalid("无法保存附件，文件已存在或目录不可写"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&file.data)
                .map_err(|_| invalid("附件内容编码无效"))?;
            target
                .write_all(&bytes)
                .map_err(|_| invalid("无法写入附件内容"))?;
        }
        paths.push(if input.kind == "folder" {
            root
        } else {
            slot.join(&input.name)
        });
    }
    Ok(paths)
}

pub(super) async fn stage(
    directory: PathBuf,
    payload: &Value,
    authorization: &RemoteAuthorization,
) -> anyhow::Result<Vec<PathBuf>> {
    let inputs = parse(payload)?;
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    authorization.check().await?;
    let owner = authorization.device.id.clone();
    // 同一回执只有首次执行会写附件，回执未知时仍沿用已有核对流程，不能重新上传并发送。
    let request = authorization
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let result = tokio::task::spawn_blocking(move || store(&directory, &owner, &request, &inputs))
        .await
        .map_err(|_| ApiError::new(503, "保存附件任务已中断，消息尚未发送"))??;
    authorization.check().await?;
    Ok(result)
}

pub(super) fn append_paths(payload: &mut Value, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    // 文件路径来自服务端落盘结果；用 JSON 字符串保留空格/中文，不能拼接为 shell 命令。
    let references = paths
        .iter()
        .map(|path| serde_json::to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = payload["prompt"].as_str().unwrap_or("");
    payload["prompt"] = serde_json::json!(format!(
        "{prompt}\n\n附加的本地文件和文件夹：\n{references}"
    ));
    payload.as_object_mut().unwrap().remove("files");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({"files":[
            {"kind":"folder","name":"素材","size":3,"directories":["空目录","nested"],
                "files":[{"path":"nested/a.txt","data":"YWJj"}]},
            {"kind":"file","name":"empty.txt","size":0,"directories":[],
                "files":[{"path":"empty.txt","data":""}]}
        ]})
    }

    #[test]
    fn input_files_validate_names_paths_base64_sizes_and_collisions() {
        assert!(validate(&sample()).is_ok());
        for path in [
            "../escape",
            "/absolute",
            "C:/absolute",
            "a\\b",
            "a/../b",
            "NUL",
            "a:stream",
            "a.",
        ] {
            let mut value = sample();
            value["files"][0]["files"][0]["path"] = json!(path);
            assert!(validate(&value).is_err(), "{path}");
        }
        let mut value = sample();
        value["files"][0]["size"] = json!(2);
        assert!(validate(&value).is_err());
        let mut value = sample();
        value["files"][0]["files"][0]["data"] = json!("bad");
        assert!(validate(&value).is_err());
        let mut value = sample();
        value["files"][0]["directories"] = json!(["nested/A.TXT"]);
        assert!(validate(&value).is_err());
        let mut value = sample();
        value["files"][0]["directories"] = json!(["nested/a.txt/child"]);
        assert!(validate(&value).is_err());
    }

    #[test]
    fn input_files_store_preserves_structure_separates_devices_and_never_overwrites() {
        let temporary = tempfile::tempdir().unwrap();
        let parsed = parse(&sample()).unwrap();
        let first = store(temporary.path(), "device-a", "request-a", &parsed).unwrap();
        let other = store(temporary.path(), "device-b", "request-a", &parsed).unwrap();
        assert_ne!(first, other);
        assert!(first[0].join("空目录").is_dir());
        assert_eq!(
            std::fs::read(first[0].join("nested/a.txt")).unwrap(),
            b"abc"
        );
        assert_eq!(std::fs::read(&first[1]).unwrap(), b"");
        assert!(store(temporary.path(), "device-a", "request-a", &parsed).is_err());
        assert_eq!(
            std::fs::read(first[0].join("nested/a.txt")).unwrap(),
            b"abc"
        );
    }

    #[test]
    fn input_files_append_server_paths_and_remove_uploaded_content_before_dispatch() {
        let mut value = sample();
        value["prompt"] = json!("请处理");
        append_paths(&mut value, vec!["/tmp/含 空格/a.txt".into()]);
        assert!(value.get("files").is_none());
        assert!(value["prompt"].as_str().unwrap().starts_with("请处理\n\n"));
        assert!(
            value["prompt"]
                .as_str()
                .unwrap()
                .contains("\"/tmp/含 空格/a.txt\"")
        );
        let mut empty = json!({"prompt":"","files":[]});
        append_paths(&mut empty, vec![]);
        assert_eq!(empty["prompt"], "");
    }

    #[test]
    fn input_files_reject_oversized_trees_and_non_directory_storage_roots() {
        let mut value = sample();
        value["files"][0]["directories"] =
            json!((0..512).map(|i| format!("d{i}")).collect::<Vec<_>>());
        assert!(validate(&value).is_err());
        let temporary = tempfile::tempdir().unwrap();
        std::fs::write(temporary.path().join("remote-input-files"), b"occupied").unwrap();
        assert!(
            store(
                temporary.path(),
                "device",
                "request",
                &parse(&sample()).unwrap()
            )
            .is_err()
        );
    }
}
