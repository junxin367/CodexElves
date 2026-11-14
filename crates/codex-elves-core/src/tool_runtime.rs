use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::process::Command;

const TOOL_MANIFEST: &str = "tool-manifest.json";
const RG_TOOL_ID: &str = "rg";

#[derive(Debug, Clone)]
pub struct ToolRuntimeInfo {
    pub tools_dir: Option<PathBuf>,
    pub rg_path: Option<PathBuf>,
    pub source: &'static str,
    pub verification: &'static str,
}

impl ToolRuntimeInfo {
    pub fn diagnostic_value(&self) -> Value {
        serde_json::json!({
            "source": self.source,
            "verification": self.verification,
            "paths": {
                "toolsDir": self.tools_dir.as_ref().map(|path| path.to_string_lossy().to_string()),
                "rgPath": self.rg_path.as_ref().map(|path| path.to_string_lossy().to_string()),
            },
            "toolsDir": self.tools_dir.as_ref().map(|path| path.to_string_lossy().to_string()),
            "rgPath": self.rg_path.as_ref().map(|path| path.to_string_lossy().to_string()),
        })
    }
}

/// 为 Codex app-server 及其子进程注入 CodexElves 自带工具目录。
///
/// 用户要求服务模式不依赖 Codex App 或用户 PATH；只修改新建子进程的环境，
/// 不写入系统/用户 PATH。自带文件缺失或校验失败时保留外部 PATH，允许降级运行。
pub fn apply_to_command(command: &mut Command) -> ToolRuntimeInfo {
    let original_path = env::var_os("PATH");
    apply_to_command_with(command, tool_root_candidates(), original_path)
}

/// 返回当前服务可用的检索工具状态，供受保护的诊断接口展示。
pub fn current() -> ToolRuntimeInfo {
    discover()
}

fn apply_to_command_with(
    command: &mut Command,
    candidates: Vec<PathBuf>,
    original_path: Option<OsString>,
) -> ToolRuntimeInfo {
    let info = discover_with(candidates, original_path.clone());
    if let Some(tools_dir) = info.tools_dir.as_ref() {
        let runtime_dir = info
            .rg_path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(tools_dir);
        let path = if same_path(runtime_dir, tools_dir) {
            prepend_path(tools_dir, original_path)
        } else {
            prepend_paths([runtime_dir, tools_dir], original_path)
        };
        if let Some(path) = path {
            command.env("PATH", path);
        }
        command.env("CODEX_ELVES_TOOL_DIR", tools_dir);
    }
    if let Some(path) = info.rg_path.as_ref() {
        command.env("CODEX_ELVES_RG_PATH", path);
    }
    info
}

fn discover() -> ToolRuntimeInfo {
    discover_with(tool_root_candidates(), env::var_os("PATH"))
}

fn discover_with(candidates: Vec<PathBuf>, original_path: Option<OsString>) -> ToolRuntimeInfo {
    for tools_dir in candidates {
        let Some(manifest) = read_manifest(&tools_dir) else {
            continue;
        };
        let Some(path) = verified_rg_path(&tools_dir, &manifest) else {
            continue;
        };
        return ToolRuntimeInfo {
            tools_dir: Some(tools_dir),
            rg_path: Some(path),
            source: "bundled",
            verification: "sha256",
        };
    }
    if let Some(path) = find_on_path("rg", original_path.as_ref()) {
        return ToolRuntimeInfo {
            tools_dir: None,
            rg_path: Some(path),
            source: "path",
            verification: "external",
        };
    }
    ToolRuntimeInfo {
        tools_dir: None,
        rg_path: None,
        source: "unavailable",
        verification: "none",
    }
}

fn tool_root_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(value) = env::var_os("CODEX_ELVES_TOOL_DIR") {
        candidates.push(PathBuf::from(value));
    }
    if let Ok(executable) = env::current_exe() {
        let mut current = executable.parent().map(Path::to_path_buf);
        for _ in 0..=6 {
            let Some(directory) = current else {
                break;
            };
            candidates.push(directory.join("tools"));
            current = directory.parent().map(Path::to_path_buf);
        }
    }
    // 开发构建从仓库 tools/ 读取；安装构建优先使用可执行文件旁的 tools/。
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tools"),
    );
    dedupe_paths(candidates)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut seen = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if seen.iter().any(|value| value == &key) {
            continue;
        }
        seen.push(key);
        result.push(path);
    }
    result
}

fn read_manifest(tools_dir: &Path) -> Option<Value> {
    let path = tools_dir.join(TOOL_MANIFEST);
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn verified_rg_path(tools_dir: &Path, manifest: &Value) -> Option<PathBuf> {
    let target = if cfg!(windows) && env::consts::ARCH == "x86_64" {
        "windows-x64"
    } else {
        return None;
    };
    let relative = manifest["tools"][RG_TOOL_ID]["targets"][target]["path"].as_str()?;
    let expected = manifest["tools"][RG_TOOL_ID]["targets"][target]["sha256"]
        .as_str()?
        .to_ascii_lowercase();
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
                    | std::path::Component::ParentDir
            )
        })
    {
        return None;
    }
    let path = tools_dir.join(relative_path);
    if !path.is_file() || expected.len() != 64 {
        return None;
    }
    let actual = sha256_file(&path).ok()?;
    (actual == expected).then_some(path)
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn prepend_path(directory: &Path, original: Option<OsString>) -> Option<OsString> {
    prepend_paths(std::iter::once(directory), original)
}

fn prepend_paths<'a, I>(directories: I, original: Option<OsString>) -> Option<OsString>
where
    I: IntoIterator<Item = &'a Path>,
{
    let mut paths = directories
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    if let Some(original) = original {
        for path in env::split_paths(&original) {
            if !paths.iter().any(|existing| same_path(existing, &path)) {
                paths.push(path);
            }
        }
    }
    env::join_paths(paths).ok()
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn find_on_path(name: &str, original_path: Option<&OsString>) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    original_path
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join(&executable))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepends_tool_directory_without_duplicate_path_entries() {
        let directory = PathBuf::from(if cfg!(windows) {
            r"C:\CodexElves\tools"
        } else {
            "/opt/CodexElves/tools"
        });
        let original = env::join_paths([
            directory.clone(),
            directory.join("nested"),
            PathBuf::from("existing"),
        ])
        .unwrap();
        let merged = prepend_path(&directory, Some(original)).unwrap();
        let entries: Vec<_> = env::split_paths(&merged).collect();
        assert_eq!(entries[0], directory);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2], PathBuf::from("existing"));
    }

    #[test]
    fn manifest_rejects_hash_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let tool_dir = temp.path().join("tools");
        fs::create_dir_all(tool_dir.join("windows-x64")).unwrap();
        fs::write(tool_dir.join("windows-x64").join("rg.exe"), b"wrong").unwrap();
        let manifest = serde_json::json!({
            "tools":{"rg":{"targets":{"windows-x64":{
                "path":"windows-x64/rg.exe",
                "sha256":"0000000000000000000000000000000000000000000000000000000000000000"
            }}}}
        });
        assert!(verified_rg_path(&tool_dir, &manifest).is_none());
        let traversal = serde_json::json!({
            "tools":{"rg":{"targets":{"windows-x64":{
                "path":"../rg.exe",
                "sha256":"0000000000000000000000000000000000000000000000000000000000000000"
            }}}}
        });
        assert!(verified_rg_path(&tool_dir, &traversal).is_none());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn bundled_rg_is_resolved_without_codex_app_or_parent_path() {
        let tools_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tools");
        let system_root =
            env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
        let parent_path = env::var_os("PATH");
        let where_path = PathBuf::from(&system_root)
            .join("System32")
            .join("where.exe");
        let mut command = Command::new(where_path);
        command.arg("rg.exe");
        command.env_clear();
        command.env("SystemRoot", system_root);
        command.env("PATHEXT", ".COM;.EXE;.BAT;.CMD");
        let info = apply_to_command_with(
            &mut command,
            vec![tools_dir],
            Some(OsString::from(r"C:\codex-elves-test-empty-path")),
        );

        assert_eq!(info.source, "bundled");
        assert_eq!(info.verification, "sha256");
        let rg_path = info.rg_path.clone().unwrap();
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "where.exe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let resolved = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .and_then(|path| fs::canonicalize(path).ok())
            .unwrap();
        assert_eq!(
            fs::canonicalize(rg_path).unwrap(),
            resolved,
            "where.exe returned {}",
            resolved.display()
        );
        assert_eq!(env::var_os("PATH"), parent_path);
    }
}
