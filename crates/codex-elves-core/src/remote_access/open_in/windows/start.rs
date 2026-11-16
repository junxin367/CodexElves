use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::process::Command;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::GetProcessId;
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain([0]).collect()
}

// ShellExecute 只接受参数字符串；按 Windows argv 规则转义每个独立参数，路径中的引号和尾部反斜杠不能变成额外命令。
fn quoted(value: &OsStr) -> Vec<u16> {
    let mut result = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in value.encode_wide() {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        result.push(unit);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    result.push(b'"' as u16);
    result
}

pub(super) fn launch(command: &mut Command) -> io::Result<Option<u32>> {
    // 保留 Electron 应用的既有环境隔离；存在该变量时继续使用已移除变量的 Command，禁止修改服务的全局环境。
    if std::env::var_os("ELECTRON_RUN_AS_NODE").is_some() {
        let program = command.get_program().to_string_lossy().to_lowercase();
        if ["\\pwsh.exe", "\\powershell.exe", "\\wsl.exe"]
            .iter()
            .any(|name| program.ends_with(name))
        {
            use std::os::windows::process::CommandExt;
            use std::process::Stdio;
            use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;
            // GUI 后台没有可继承的交互控制台；直接启动终端的降级路径必须创建可见控制台并保留交互输入输出。
            command
                .creation_flags(CREATE_NEW_CONSOLE.0)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
        return command.spawn().map(|child| Some(child.id()));
    }
    let _apartment = crate::windows_integration::ComApartment::init().ok();
    let file = wide(command.get_program());
    let directory = command
        .get_current_dir()
        .map(|path| wide(OsStr::new(&super::display_path(path))));
    let terminal = command
        .get_program()
        .to_string_lossy()
        .to_lowercase()
        .ends_with("wt.exe");
    let mut parameters = Vec::new();
    for argument in command.get_args() {
        if !parameters.is_empty() {
            parameters.push(b' ' as u16);
        }
        // Windows Terminal 自己也解析分号；与其原生调用约定一致，保护目录中的分号，不能产生额外标签命令。
        if terminal {
            parameters.extend(quoted(OsStr::new(
                &argument.to_string_lossy().replace(';', r"\;"),
            )));
        } else {
            parameters.extend(quoted(argument));
        }
    }
    parameters.push(0);
    let verb = wide(OsStr::new("open"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: directory
            .as_ref()
            .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
        // 用户要求外部应用/终端可见打开；不用隐藏窗口或最小化启动，也不通过 cmd/PowerShell 中转。
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| io::Error::other(error.to_string()))?;
    if info.hProcess.is_invalid() {
        return Ok(None);
    }
    let process_id = unsafe { GetProcessId(info.hProcess) };
    let _ = unsafe { CloseHandle(info.hProcess) };
    Ok((process_id != 0).then_some(process_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_arguments_preserve_spaces_quotes_and_trailing_slashes() {
        for (input, expected) in [
            ("", "\"\""),
            ("D:\\项目 文件\\", "\"D:\\项目 文件\\\\\""),
            ("a\"b", "\"a\\\"b\""),
            ("E:\\a;b", "\"E:\\a;b\""),
        ] {
            assert_eq!(
                String::from_utf16(&quoted(OsStr::new(input))).unwrap(),
                expected
            );
        }
    }
}
