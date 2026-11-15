use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::core::PCWSTR;

use super::{ApiError, Application, display_path};

fn registry_string(subkey: &str, name: &str) -> Option<PathBuf> {
    let subkey: Vec<_> = subkey.encode_utf16().chain([0]).collect();
    let name: Vec<_> = name.encode_utf16().chain([0]).collect();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY] {
            let mut buffer = vec![0u16; 32_768];
            let mut bytes = (buffer.len() * 2) as u32;
            let result = unsafe {
                RegGetValueW(
                    hive,
                    PCWSTR(subkey.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | view,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&mut bytes),
                )
            };
            if result.is_ok() && bytes >= 2 && bytes as usize <= buffer.len() * 2 {
                let units = &buffer[..bytes as usize / 2];
                let end = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(units.len());
                let value = OsString::from_wide(&units[..end]);
                let path = PathBuf::from(value.to_string_lossy().trim_matches('"'));
                if path.is_absolute() && path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn executable(path: PathBuf) -> Option<PathBuf> {
    (path.is_absolute()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path.is_file())
    .then_some(path)
}

fn app_path(name: &str) -> Option<PathBuf> {
    registry_string(
        &format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{name}"),
        "",
    )
    .and_then(executable)
}

fn in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .find_map(|directory| executable(directory.join(name)))
}

fn install_roots() -> Vec<PathBuf> {
    [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
    ]
    .iter()
    .filter_map(std::env::var_os)
    .map(PathBuf::from)
    .filter(|path| path.is_absolute())
    .collect()
}

fn installed(relative: &[&str]) -> Option<PathBuf> {
    install_roots()
        .into_iter()
        .find_map(|root| relative.iter().find_map(|path| executable(root.join(path))))
}

fn children(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    // 仅扫描已知安装目录的一层，限制条数；不能遍历会话项目来查找可执行文件。
    let mut paths: Vec<_> = entries
        .take(256)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        })
        .map(|entry| entry.path())
        .collect();
    paths.sort_by(|left, right| right.cmp(left));
    paths
}

pub(super) fn find_application(target: &str, root: &Path) -> Option<PathBuf> {
    let wsl = super::super::preferences::wsl_distribution(root).is_some();
    // Git Bash/Cmder 的 Windows shell 不能可靠保留 WSL 用户及 Linux 目录，WSL 会话不提供这两个目标。
    if wsl && matches!(target, "gitBash" | "cmder") {
        return None;
    }
    match target {
        "vscode" => app_path("Code.exe")
            .or_else(|| {
                installed(&[
                    r"Programs\Microsoft VS Code\Code.exe",
                    r"Microsoft VS Code\Code.exe",
                ])
            })
            .or_else(|| {
                in_path("code.exe").or_else(|| {
                    std::env::var_os("PATH")
                        .into_iter()
                        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
                        .find_map(|directory| executable(directory.join("../Code.exe")))
                })
            }),
        "visualStudio" => app_path("devenv.exe")
            .or_else(|| in_path("devenv.exe"))
            .or_else(|| {
                install_roots().iter().find_map(|base| {
                    children(&base.join("Microsoft Visual Studio"), "")
                        .into_iter()
                        .find_map(|year| {
                            children(&year, "").into_iter().find_map(|edition| {
                                executable(edition.join(r"Common7\IDE\devenv.exe"))
                            })
                        })
                })
            }),
        "sublimeText" => app_path("sublime_text.exe")
            .or_else(|| {
                installed(&[
                    r"Sublime Text\sublime_text.exe",
                    r"Sublime Text 3\sublime_text.exe",
                ])
            })
            .or_else(|| in_path("sublime_text.exe")),
        "githubDesktop" => app_path("GitHubDesktop.exe").or_else(|| {
            let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("GitHubDesktop");
            executable(root.join("GitHubDesktop.exe")).or_else(|| {
                children(&root, "app-")
                    .into_iter()
                    .find_map(|version| executable(version.join("GitHubDesktop.exe")))
            })
        }),
        "fileManager" => std::env::var_os("WINDIR")
            .and_then(|root| executable(PathBuf::from(root).join("explorer.exe"))),
        "gitBash" => registry_string(r"Software\GitForWindows", "InstallPath")
            .and_then(|root| executable(root.join("git-bash.exe")))
            .or_else(|| installed(&[r"Git\git-bash.exe", r"Programs\Git\git-bash.exe"]))
            .or_else(|| in_path("git-bash.exe"))
            .or_else(|| {
                in_path("git.exe")
                    .and_then(|path| executable(path.parent()?.parent()?.join("git-bash.exe")))
            }),
        "cmder" => app_path("Cmder.exe")
            .or_else(|| in_path("Cmder.exe"))
            .or_else(|| {
                std::env::var_os("CMDER_ROOT")
                    .and_then(|root| executable(PathBuf::from(root).join("Cmder.exe")))
            }),
        "rider" => app_path("rider64.exe")
            .or_else(|| app_path("rider.exe"))
            .or_else(|| in_path("rider64.exe"))
            .or_else(|| {
                install_roots().iter().find_map(|base| {
                    children(&base.join("JetBrains"), "JetBrains Rider")
                        .into_iter()
                        .chain(children(&base.join("Programs"), "Rider"))
                        .find_map(|root| executable(root.join(r"bin\rider64.exe")))
                })
            })
            .or_else(|| {
                let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
                    .join(r"JetBrains\Toolbox\apps\Rider");
                children(&root, "").into_iter().find_map(|channel| {
                    executable(channel.join(r"bin\rider64.exe")).or_else(|| {
                        children(&channel, "")
                            .into_iter()
                            .find_map(|version| executable(version.join(r"bin\rider64.exe")))
                    })
                })
            }),
        _ => None,
    }
}

fn command(application: &Application, root: &Path) -> Command {
    let path = display_path(root);
    let mut command = Command::new(&application.executable);
    match application.target {
        "githubDesktop" => {
            // GitHub Desktop 原生 CLI 使用 --cli-open；--path 只会启动程序而不会切到指定仓库。
            command.arg("--cli-open").arg(&path);
        }
        "gitBash" => {
            command.arg(format!("--cd={path}"));
        }
        "cmder" => {
            command.arg("/START").arg(&path);
        }
        _ => {
            command.arg(&path);
        }
    }
    // 用户选择的是可见应用；不能沿用后台 CLI 的 CREATE_NO_WINDOW，也不经 cmd/PowerShell 拼接命令。
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("ELECTRON_RUN_AS_NODE");
    command
}

pub(super) fn launch(application: &Application, root: &Path) -> Result<(), ApiError> {
    // canonicalize 会产生 \\?\ 前缀，交给 Windows Shell/编辑器前恢复普通盘符或 UNC 形式。
    let root_now = std::fs::canonicalize(root)
        .map_err(|_| ApiError::new(404, "会话工作目录不存在或无法读取"))?;
    if root_now != root || !root_now.is_dir() {
        return Err(ApiError::new(409, "会话工作目录已变化，请重新打开会话"));
    }
    command(application, root)
        .spawn()
        .map(|_| ())
        .map_err(|error| ApiError::new(422, &format!("无法启动 {}：{error}", application.label)))
}
