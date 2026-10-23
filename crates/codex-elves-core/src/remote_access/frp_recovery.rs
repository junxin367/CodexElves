use std::path::Path;

use super::frp_tunnel::Saved;

fn matching_mapping(text: &str, saved: &Saved) -> bool {
    let Ok(config) = text.parse::<toml::Value>() else {
        return false;
    };
    let Some(proxies) = config.get("proxies").and_then(toml::Value::as_array) else {
        return false;
    };
    if proxies.len() != 1 {
        return false;
    }
    let proxy = &proxies[0];
    let token = config
        .get("auth")
        .and_then(|auth| auth.get("token"))
        .and_then(toml::Value::as_str);
    config.get("serverAddr").and_then(toml::Value::as_str)
        == Some(saved.config.server_addr.as_str())
        && config.get("serverPort").and_then(toml::Value::as_integer)
            == Some(i64::from(saved.config.server_port))
        && token.is_some_and(|token| super::constant_time_eq(token, &saved.token))
        && proxy.get("type").and_then(toml::Value::as_str) == Some("tcp")
        && proxy
            .get("name")
            .and_then(toml::Value::as_str)
            .is_some_and(|name| name.starts_with("codex-elves-"))
        && proxy.get("localIP").and_then(toml::Value::as_str) == Some("127.0.0.1")
        && proxy.get("remotePort").and_then(toml::Value::as_integer)
            == Some(i64::from(saved.config.remote_port))
}

#[cfg(not(windows))]
pub(super) async fn release_local_mapping(_: &Path, _: &Path, _: &Saved) -> anyhow::Result<usize> {
    Ok(0)
}

#[cfg(windows)]
pub(super) async fn release_local_mapping(
    directory: &Path,
    program: &Path,
    saved: &Saved,
) -> anyhow::Result<usize> {
    use anyhow::Context;
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::process::Command;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Process {
        id: u32,
        created: u64,
        image: String,
        command_line: String,
    }
    // Enumerate only. The pinned native process handle below checks creation time
    // again before termination, so a recycled PID cannot select a new process.
    let script = r#"
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$rows = @(Get-CimInstance Win32_Process -Filter "Name = 'frpc.exe'" | ForEach-Object {
    try {
        $item = Get-Process -Id $_.ProcessId -ErrorAction Stop
        if ($_.ExecutablePath -and $_.CommandLine) {
            [pscustomobject]@{ id = $_.ProcessId; created = $item.StartTime.ToFileTimeUtc(); image = $_.ExecutablePath; commandLine = $_.CommandLine }
        }
    } catch {}
})
ConvertTo-Json -InputObject $rows -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .context("核对本机旧 FRP 映射超时")??;
    anyhow::ensure!(
        output.status.success(),
        "无法核对本机旧 FRP 映射，未停止任何未知进程"
    );
    let processes: Vec<Process> =
        serde_json::from_slice(&output.stdout).context("无法读取旧 FRP 映射归属")?;
    let expected_program = program.canonicalize()?;
    let legacy_program = directory
        .join("tools/frp/0.54.0/frpc.exe")
        .canonicalize()
        .ok();
    let roots = [
        directory.join("remote-access-frp"),
        directory.join("remote-access-secrets/frp-test"),
    ];
    let mut released = 0;
    for process in processes {
        let Ok(image) = Path::new(&process.image).canonicalize() else {
            continue;
        };
        if image != expected_program {
            // Upgrading the managed client must also release the previously
            // shipped 0.54.0 mapping, but never trust an arbitrary old binary.
            use sha2::{Digest, Sha256};
            if legacy_program.as_ref() != Some(&image)
                || !std::fs::metadata(&image).is_ok_and(|metadata| metadata.len() == 14_196_736)
                || !std::fs::read(&image).is_ok_and(|bytes| {
                    format!("{:x}", Sha256::digest(bytes))
                        == "15846490706281424a0f5b0b7d3e6ea92173dbeaf6a10a566f4dc8a6a1e977bb"
                })
            {
                continue;
            }
        }
        let Some(path) = config_argument(&process.command_line) else {
            continue;
        };
        let Ok(path) = Path::new(&path).canonicalize() else {
            continue;
        };
        if !roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .any(|root| path.starts_with(root))
        {
            continue;
        }
        if !std::fs::metadata(&path).is_ok_and(|meta| meta.len() <= 64 * 1024) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if matching_mapping(&text, saved) && terminate_same_process(process.id, process.created) {
            released += 1;
        }
    }
    Ok(released)
}

#[cfg(windows)]
fn config_argument(command: &str) -> Option<String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::UI::Shell::CommandLineToArgvW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let mut count = 0;
    unsafe {
        let args = CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut count);
        if args.is_null() {
            return None;
        }
        let values: Option<Vec<String>> = std::slice::from_raw_parts(args, count as usize)
            .iter()
            .map(|arg| arg.to_string().ok())
            .collect();
        let _ = LocalFree(HLOCAL(args.cast()));
        let values = values?;
        if values.len() == 3 && matches!(values[1].as_str(), "-c" | "--config") {
            return Some(values[2].clone());
        }
        if values.len() == 2 {
            return values[1].strip_prefix("--config=").map(str::to_owned);
        }
        None
    }
}

#[cfg(windows)]
fn terminate_same_process(id: u32, created: u64) -> bool {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };
    unsafe {
        let Ok(handle) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            id,
        ) else {
            return false;
        };
        let mut start = FILETIME::default();
        let mut end = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let matched = GetProcessTimes(handle, &mut start, &mut end, &mut kernel, &mut user).is_ok()
            && ((u64::from(start.dwHighDateTime) << 32) | u64::from(start.dwLowDateTime))
                == created;
        let terminated = matched && TerminateProcess(handle, 0).is_ok();
        let _ = CloseHandle(handle);
        terminated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frp_recovery_only_selects_the_same_single_owned_mapping() {
        let saved = Saved {
            config: super::super::frp_tunnel::Config {
                server_addr: "relay.example.com".into(),
                server_port: 7000,
                remote_port: 39399,
                ..Default::default()
            },
            token: "test-secret".into(),
            ..Default::default()
        };
        let config = r#"serverAddr = "relay.example.com"
serverPort = 7000
auth.token = "test-secret"
[[proxies]]
name = "codex-elves-test"
type = "tcp"
localIP = "127.0.0.1"
localPort = 45222
remotePort = 39399
"#;
        assert!(matching_mapping(config, &saved));
        for changed in [
            config.replace("test-secret", "another-secret"),
            config.replace("39399", "39400"),
            config.replace("relay.example.com", "another.example.com"),
            config.replace("7000", "7001"),
            config.replace("codex-elves-test", "other-application"),
            format!("{config}\n[[proxies]]\nname = \"unrelated\"\n"),
        ] {
            assert!(!matching_mapping(&changed, &saved));
        }
    }
}
