use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine as _;
use image::ImageEncoder as _;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
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

struct IconHandle(HICON);
impl Drop for IconHandle {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = DestroyIcon(self.0);
            }
        }
    }
}
struct BitmapHandle(HBITMAP);
impl Drop for BitmapHandle {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.0.0));
            }
        }
    }
}
struct DeviceContext(HDC);
impl Drop for DeviceContext {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

fn bitmap_pixels(dc: HDC, bitmap: HBITMAP, width: u32, height: u32) -> Option<Vec<u8>> {
    let mut pixels = vec![0_u8; width.checked_mul(height)?.checked_mul(4)? as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // 负高度按顶向下读取，后续无需再翻转图标。
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    (unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    } == height as i32)
        .then_some(pixels)
}

fn png_data_url(width: u32, height: u32, rgba: Vec<u8>) -> Option<String> {
    if rgba.len() != width.checked_mul(height)?.checked_mul(4)? as usize {
        return None;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

pub(super) fn icon_data(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut large = HICON(std::ptr::null_mut());
    let mut small = HICON(std::ptr::null_mut());
    if unsafe {
        ExtractIconExW(
            PCWSTR(path.as_ptr()),
            0,
            Some(&mut large),
            Some(&mut small),
            1,
        )
    } == 0
    {
        return None;
    }
    let _large = IconHandle(large);
    let _small = IconHandle(small);
    let icon = if !large.0.is_null() { large } else { small };
    if icon.0.is_null() {
        return None;
    }
    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let color = BitmapHandle(info.hbmColor);
    let mask = BitmapHandle(info.hbmMask);
    let source_is_mask = color.0.0.is_null();
    let source = if source_is_mask { mask.0 } else { color.0 };
    if source.0.is_null() {
        return None;
    }
    let mut bitmap = BITMAP::default();
    if unsafe {
        GetObjectW(
            HGDIOBJ(source.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut _ as *mut c_void),
        )
    } == 0
    {
        return None;
    }
    let width = bitmap.bmWidth.unsigned_abs();
    let mut height = bitmap.bmHeight.unsigned_abs();
    if source_is_mask && height % 2 == 0 {
        height /= 2;
    }
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return None;
    }
    let dc = DeviceContext(unsafe { CreateCompatibleDC(HDC(std::ptr::null_mut())) });
    if dc.0.0.is_null() {
        return None;
    }
    let bgra = bitmap_pixels(dc.0, source, width, height)?;
    let mask = if source_is_mask {
        Some(bgra.clone())
    } else if !mask.0.0.is_null() {
        bitmap_pixels(dc.0, mask.0, width, height)
    } else {
        None
    };
    let has_alpha = !source_is_mask && bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for (index, pixel) in bgra.chunks_exact(4).enumerate() {
        let alpha = if has_alpha {
            pixel[3]
        } else if mask.as_ref().is_some_and(|mask| mask[index * 4] != 0) {
            0
        } else {
            255
        };
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
    }
    png_data_url(width, height, rgba)
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

fn command(
    application: &Application,
    root: &Path,
    terminal: Option<&Path>,
    file: Option<&Path>,
) -> Command {
    let path = display_path(file.unwrap_or(root));
    // 正文打开编辑器时传入原文件；终端进入文件所在目录，WSL 仍按原会话身份恢复，不能执行文件内容或另建对话。
    let directory = if application.target == "wsl" || application.target == "githubDesktop" {
        root
    } else {
        file.and_then(Path::parent).unwrap_or(root)
    };
    let directory_path = display_path(directory);
    let mut command = Command::new(terminal.unwrap_or(&application.executable));
    if terminal.is_some() {
        // 用户要求终端在当前会话目录可见打开；显式指定 PowerShell/WSL，不能误用 Windows Terminal 的默认配置。
        command.args([
            "-w",
            "new",
            "new-tab",
            "-d",
            &directory_path,
            "--title",
            &window_title(application, directory),
            "--suppressApplicationTitle",
        ]);
        command.arg(&application.executable);
    }
    match application.target {
        "githubDesktop" => {
            // GitHub Desktop 原生 CLI 使用 --cli-open；--path 只会启动程序而不会切到指定仓库。
            command.arg("--cli-open").arg(display_path(root));
        }
        "fileManager" if file.is_some() => {
            // 用户要求在 File Explorer 定位所点文件；固定 /select 参数与路径作为一个参数，不经 shell 拼接执行。
            command.arg(format!("/select,{path}"));
        }
        "gitBash" => {
            command.arg(format!("--cd={directory_path}"));
        }
        "cmder" => {
            command.arg("/START").arg(&directory_path);
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
        .current_dir(directory)
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
    // 应用列表读取后文件可能被替换；实际启动前再次核对同一个允许根目录内的原文件，不能只相信网页路径。
    let file = application
        .file
        .as_ref()
        .map(|file| super::super::workspace::open_in_file_path(&file.root, &file.relative))
        .transpose()?;
    let mut command = command(application, root, terminal.as_deref(), file.as_deref());
    let snapshot = foreground::Snapshot::capture();
    // 用户要求应用/文件夹打开后可见：通过 Windows Shell 正常显示启动，再恢复并激活对应目标窗口；不模拟键盘或修改系统焦点策略。
    let process_id = start::launch(&mut command)
        .map_err(|error| ApiError::new(422, &format!("无法启动 {}：{error}", application.label)))?;
    let focus = foreground::activate(
        &snapshot,
        application,
        terminal.as_deref(),
        process_id,
        if application.target == "wsl" || application.target == "githubDesktop" {
            root
        } else if application.target == "fileManager"
            || matches!(application.target, "powershell" | "gitBash" | "cmder")
        {
            file.as_deref().and_then(Path::parent).unwrap_or(root)
        } else {
            file.as_deref().unwrap_or(root)
        },
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
            file: None,
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
        let idea = command(
            &application("intellij", r"C:\Apps\idea64.exe"),
            root,
            None,
            None,
        );
        assert_eq!(args(&idea), [root.to_str().unwrap()]);
        let powershell = command(
            &application("powershell", r"C:\Apps\pwsh.exe"),
            root,
            None,
            None,
        );
        assert_eq!(args(&powershell), ["-NoExit"]);
        assert_eq!(powershell.get_current_dir(), Some(root));
        let powershell = command(
            &application("powershell", r"C:\Apps\pwsh.exe"),
            root,
            Some(Path::new(r"C:\Apps\wt.exe")),
            None,
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
    fn file_editors_receive_exact_file_and_explorer_selects_it_without_a_shell() {
        let root = Path::new(r"E:\repo");
        let file = Path::new(r"E:\repo\子目录 with spaces\a;b.ts");
        for target in ["vscode", "visualStudio", "intellij", "sublimeText", "rider"] {
            let editor = command(
                &application(target, r"C:\Apps\editor.exe"),
                root,
                None,
                Some(file),
            );
            assert_eq!(args(&editor), [file.to_str().unwrap()], "{target}");
            assert_eq!(editor.get_current_dir(), file.parent());
        }
        let explorer = command(
            &application("fileManager", r"C:\Windows\explorer.exe"),
            root,
            None,
            Some(file),
        );
        assert_eq!(args(&explorer), [format!("/select,{}", file.display())]);
        let bash = command(
            &application("gitBash", r"C:\Apps\git-bash.exe"),
            root,
            None,
            Some(file),
        );
        assert_eq!(
            args(&bash),
            [format!("--cd={}", file.parent().unwrap().display())]
        );
        let powershell = command(
            &application("powershell", r"C:\Apps\pwsh.exe"),
            root,
            None,
            Some(file),
        );
        assert_eq!(args(&powershell), ["-NoExit"]);
        assert_eq!(powershell.get_current_dir(), file.parent());
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
            let command = command(&application, Path::new(root), None, None);
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
    fn icon_png_encoding_preserves_rgba_pixels() {
        use base64::Engine as _;
        let pixels = vec![255, 0, 0, 255, 0, 0, 255, 128];
        let url = png_data_url(2, 1, pixels.clone()).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(url.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.into_raw(), pixels);
    }

    #[test]
    fn installed_git_and_wsl_icons_are_valid_png_when_available() {
        use base64::Engine as _;
        for target in ["gitBash", "wsl"] {
            let Some(executable) = find_application(target, Path::new(r"E:\\")) else {
                continue;
            };
            let url = icon_data(&executable)
                .unwrap_or_else(|| panic!("无法提取 {} 的图标", executable.display()));
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(url.strip_prefix("data:image/png;base64,").unwrap())
                .unwrap();
            let image = image::load_from_memory(&bytes).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
            assert!(image.width() <= 256 && image.height() <= 256);
            assert!(
                image.to_rgba8().pixels().any(|pixel| pixel.0[3] > 0),
                "{target} 图标不能全透明"
            );
            eprintln!(
                "已提取 {target} 图标：{} ({}x{})",
                executable.display(),
                image.width(),
                image.height()
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
                file: None,
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
