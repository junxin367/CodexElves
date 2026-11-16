use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::core::PCWSTR;

use super::{ApiError, Application, LaunchOutcome, display_path};

mod foreground;
mod start;

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
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&prefix.to_lowercase())
        })
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
        "intellij" => app_path("idea64.exe")
            .or_else(|| app_path("idea.exe"))
            .or_else(|| in_path("idea64.exe"))
            .or_else(|| in_path("idea.exe"))
            .or_else(|| {
                install_roots().iter().find_map(|base| {
                    children(&base.join("JetBrains"), "IntelliJ IDEA")
                        .into_iter()
                        .chain(children(&base.join("Programs"), "IntelliJ IDEA"))
                        .find_map(|root| executable(root.join(r"bin\idea64.exe")))
                })
            })
            .or_else(|| {
                let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
                    .join(r"JetBrains\Toolbox\apps\IDEA");
                children(&root, "").into_iter().find_map(|channel| {
                    executable(channel.join(r"bin\idea64.exe")).or_else(|| {
                        children(&channel, "")
                            .into_iter()
                            .find_map(|version| executable(version.join(r"bin\idea64.exe")))
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
        // 用户要求 PowerShell 独立入口：优先已安装的 PowerShell 7，缺少时才使用系统 Windows PowerShell。
        "powershell" => app_path("pwsh.exe")
            .or_else(|| in_path("pwsh.exe"))
            .or_else(|| installed(&[r"PowerShell\7\pwsh.exe"]))
            .or_else(|| {
                std::env::var_os("WINDIR").and_then(|base| {
                    executable(
                        PathBuf::from(base).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
                    )
                })
            }),
        "wsl" => in_path("wsl.exe").or_else(|| {
            std::env::var_os("WINDIR")
                .and_then(|base| executable(PathBuf::from(base).join(r"System32\wsl.exe")))
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

fn windows_terminal() -> Option<PathBuf> {
    in_path("wt.exe").or_else(|| {
        let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
        executable(base.join(r"Microsoft\WindowsApps\wt.exe"))
    })
}

fn window_title(application: &Application, root: &Path) -> String {
    application
        .wsl_session
        .as_ref()
        .map(|session| session.window_title.clone())
        .unwrap_or_else(|| format!("{} · {}", application.label, display_path(root)))
}

fn command(application: &Application, root: &Path, terminal: Option<&Path>) -> Command {
    let path = display_path(root);
    let mut command = Command::new(terminal.unwrap_or(&application.executable));
    if terminal.is_some() {
        // 用户要求终端在当前会话目录可见打开；显式指定 PowerShell/WSL，不能误用 Windows Terminal 的默认配置。
        command.args([
            "-w",
            "new",
            "new-tab",
            "-d",
            &path,
            "--title",
            &window_title(application, root),
            "--suppressApplicationTitle",
        ]);
        command.arg(&application.executable);
    }
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
        "powershell" => {
            command.arg("-NoExit");
        }
        "wsl" => {
            // 用户要求恢复当前 WSL 对话：发行版、用户、Linux 目录、CLI 和 attach/resume ID 均取自服务端保存的同一会话。
            // 登录环境保留该 WSL 用户的 TZ；固定 exec "$@" 只转发独立参数，不拼接用户目录或会话内容。
            let session = application
                .wsl_session
                .as_ref()
                .expect("WSL launch context was validated");
            command.args(["--distribution", &session.distribution]);
            if !session.user.is_empty() {
                command.args(["--user", &session.user]);
            }
            command.args([
                "--cd",
                &session.linux_cwd,
                "--exec",
                "/bin/bash",
                "-lc",
                "exec \"$@\"",
                "--",
                &session.executable,
            ]);
            command.args(&session.arguments);
        }
        _ => {
            command.arg(&path);
        }
    }
    // 用户选择的是可见应用；不能沿用后台 CLI 的 CREATE_NO_WINDOW，也不经 cmd/PowerShell 拼接命令。
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("ELECTRON_RUN_AS_NODE");
    command
}

pub(super) fn launch(application: &Application, root: &Path) -> Result<LaunchOutcome, ApiError> {
    if application.target == "wsl" && application.wsl_session.is_none() {
        return Err(ApiError::new(409, "当前对话没有可恢复的 WSL 会话"));
    }
    // canonicalize 会产生 \\?\ 前缀，交给 Windows Shell/编辑器前恢复普通盘符或 UNC 形式。
    let root_now = std::fs::canonicalize(root)
        .map_err(|_| ApiError::new(404, "会话工作目录不存在或无法读取"))?;
    if root_now != root || !root_now.is_dir() {
        return Err(ApiError::new(409, "会话工作目录已变化，请重新打开会话"));
    }
    let terminal = matches!(application.target, "powershell" | "wsl")
        .then(windows_terminal)
        .flatten();
    let mut command = command(application, root, terminal.as_deref());
    let snapshot = foreground::Snapshot::capture();
    // 用户要求应用/文件夹打开后可见：通过 Windows Shell 正常显示启动，再恢复并激活对应目标窗口；不模拟键盘或修改系统焦点策略。
    let process_id = start::launch(&mut command)
        .map_err(|error| ApiError::new(422, &format!("无法启动 {}：{error}", application.label)))?;
    let focus = foreground::activate(
        &snapshot,
        application,
        terminal.as_deref(),
        process_id,
        root,
    );
    Ok(LaunchOutcome {
        foreground: focus,
        warning: (!focus).then(|| {
            format!(
                "已打开 {}，但未能确认窗口已切到前台，请在任务栏查看。",
                application.label
            )
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application(target: &'static str, executable: &str) -> Application {
        Application {
            target,
            label: target,
            executable: executable.into(),
            wsl_session: None,
        }
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn editors_receive_one_workspace_path_and_terminals_receive_start_directories() {
        let root = Path::new(r"E:\工作区 with spaces\a;b");
        let idea = command(&application("intellij", r"C:\Apps\idea64.exe"), root, None);
        assert_eq!(args(&idea), [root.to_str().unwrap()]);
        let powershell = command(&application("powershell", r"C:\Apps\pwsh.exe"), root, None);
        assert_eq!(args(&powershell), ["-NoExit"]);
        assert_eq!(powershell.get_current_dir(), Some(root));
        let powershell = command(
            &application("powershell", r"C:\Apps\pwsh.exe"),
            root,
            Some(Path::new(r"C:\Apps\wt.exe")),
        );
        assert_eq!(powershell.get_program(), OsString::from(r"C:\Apps\wt.exe"));
        let values = args(&powershell);
        assert_eq!(
            &values[..6],
            [
                "-w",
                "new",
                "new-tab",
                "-d",
                root.to_str().unwrap(),
                "--title"
            ]
        );
        assert_eq!(
            &values[7..],
            ["--suppressApplicationTitle", r"C:\Apps\pwsh.exe", "-NoExit"]
        );
    }

    #[test]
    fn wsl_keeps_session_identity_even_for_a_windows_mounted_project() {
        let mut application = application("wsl", r"C:\Windows\System32\wsl.exe");
        application.wsl_session = Some(super::super::WslSession {
            instance_id: "claude:wsl:fixture".into(),
            distribution: "codex-elves-claude-test".into(),
            user: "junes".into(),
            linux_cwd: "/mnt/e/项目".into(),
            executable: "/home/junes/.local/bin/claude".into(),
            arguments: vec!["attach".into(), "job-fixture".into()],
            window_title: "WSL · fixture [native-id]".into(),
        });
        for root in [
            r"E:\项目",
            r"\\wsl.localhost\codex-elves-claude-test\home\junes\项目",
        ] {
            let command = command(&application, Path::new(root), None);
            assert_eq!(
                args(&command),
                [
                    "--distribution",
                    "codex-elves-claude-test",
                    "--user",
                    "junes",
                    "--cd",
                    "/mnt/e/项目",
                    "--exec",
                    "/bin/bash",
                    "-lc",
                    "exec \"$@\"",
                    "--",
                    "/home/junes/.local/bin/claude",
                    "attach",
                    "job-fixture"
                ]
            );
        }
    }

    #[test]
    #[ignore = "手动验收：会在当前用户桌面打开指定应用"]
    fn desktop_open_in_smoke() {
        let target = std::env::var("CODEX_ELVES_OPEN_IN_SMOKE_TARGET")
            .unwrap_or_else(|_| "fileManager".into());
        let root =
            std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        let &(target, label) = super::super::TARGETS
            .iter()
            .find(|(id, _)| *id == target)
            .unwrap();
        let executable = find_application(target, &root).expect("目标应用必须已安装");
        eprintln!("检测到 {label}: {}", executable.display());
        let outcome = launch(
            &Application {
                target,
                label,
                executable,
                wsl_session: None,
            },
            &root,
        )
        .unwrap();
        eprintln!(
            "前台确认: {}; 提示: {:?}",
            outcome.foreground, outcome.warning
        );
        assert!(outcome.foreground, "启动成功但前台验收尚未通过");
    }
}
