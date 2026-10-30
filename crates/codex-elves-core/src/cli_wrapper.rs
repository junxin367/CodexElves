use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;

use crate::settings::BackendSettings;

pub const WRAPPER_EXE: &str = "codex-wrapper.exe";
pub const WRAPPER_SOURCE: &str = "codex-wrapper.cs";
const CLI_HOME_DIR: &str = ".codex-elves-cli";
const MAX_VERSION_BYTES: usize = 16 * 1024;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexCliVersion {
    core: Vec<u64>,
    prerelease: Option<Vec<VersionIdentifier>>,
    pub(crate) raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionIdentifier {
    Numeric(u64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperInstall {
    pub wrapper_path: PathBuf,
    pub source_path: PathBuf,
    pub real_codex: PathBuf,
    pub codex_home: PathBuf,
}

pub fn ensure_cli_wrapper(settings: &BackendSettings) -> anyhow::Result<Option<WrapperInstall>> {
    let wrapper_dir = wrapper_dir();
    if !should_refresh_cli_wrapper(settings, &wrapper_dir) {
        return Ok(None);
    }
    let real_codex = resolve_real_codex_for_settings(settings).ok_or_else(|| {
        anyhow::anyhow!("未找到系统 Codex CLI，可先启动一次系统 Codex 或重新安装 Codex")
    })?;
    let codex_home = cli_home_dir();
    let wrapper_settings = wrapper_settings_for_refresh(settings, &wrapper_dir);
    install_cli_wrapper_to(&wrapper_dir, &real_codex, &codex_home, &wrapper_settings).map(Some)
}

pub fn should_refresh_cli_wrapper(settings: &BackendSettings, wrapper_dir: &Path) -> bool {
    settings.cli_wrapper_enabled || wrapper_dir.join(WRAPPER_EXE).is_file()
}

pub fn wrapper_settings_for_refresh(
    settings: &BackendSettings,
    wrapper_dir: &Path,
) -> BackendSettings {
    if settings.cli_wrapper_enabled {
        return settings.clone();
    }

    std::fs::read_to_string(wrapper_dir.join(WRAPPER_SOURCE))
        .ok()
        .and_then(|source| parse_wrapper_source_settings(&source))
        .unwrap_or_else(|| settings.clone())
}

pub fn parse_wrapper_source_settings(source: &str) -> Option<BackendSettings> {
    let mut settings = BackendSettings::default();
    settings.cli_wrapper_api_key_env =
        csharp_string_assignment(source, "apiKeyEnv").unwrap_or(settings.cli_wrapper_api_key_env);
    settings.cli_wrapper_base_url =
        csharp_environment_assignment(source, r#"["OPENAI_BASE_URL"]"#).unwrap_or_default();
    settings.cli_wrapper_api_key = csharp_environment_assignment(source, "[apiKeyEnv]")
        .or_else(|| csharp_string_assignment(source, "apiKey"))
        .unwrap_or_default();

    if settings.cli_wrapper_base_url.is_empty() && settings.cli_wrapper_api_key.is_empty() {
        None
    } else {
        Some(settings)
    }
}

pub fn install_cli_wrapper_to(
    wrapper_dir: &Path,
    real_codex: &Path,
    codex_home: &Path,
    settings: &BackendSettings,
) -> anyhow::Result<WrapperInstall> {
    std::fs::create_dir_all(wrapper_dir)
        .with_context(|| format!("failed to create wrapper dir {}", wrapper_dir.display()))?;
    std::fs::create_dir_all(codex_home).with_context(|| {
        format!(
            "failed to create CodexElves CLI home {}",
            codex_home.display()
        )
    })?;

    let source_path = wrapper_dir.join(WRAPPER_SOURCE);
    let wrapper_path = wrapper_dir.join(WRAPPER_EXE);
    let source = build_wrapper_source(real_codex, codex_home, settings);
    std::fs::write(&source_path, source)
        .with_context(|| format!("failed to write {}", source_path.display()))?;

    compile_wrapper(&source_path, &wrapper_path)?;
    Ok(WrapperInstall {
        wrapper_path,
        source_path,
        real_codex: real_codex.to_path_buf(),
        codex_home: codex_home.to_path_buf(),
    })
}

pub fn resolve_real_codex() -> Option<PathBuf> {
    resolve_real_codex_from_candidates(None, &default_user_runtime_candidates())
}

pub fn resolve_real_codex_for_settings(_settings: &BackendSettings) -> Option<PathBuf> {
    resolve_real_codex_from_candidates(None, &default_user_runtime_candidates())
}

pub fn resolve_real_codex_from_candidates(
    _app_dir: Option<&Path>,
    user_runtime_candidates: &[PathBuf],
) -> Option<PathBuf> {
    let mut candidates = user_runtime_candidates
        .iter()
        .filter(|path| path.is_file() && !is_desktop_codex_path(path))
        .cloned()
        .collect::<Vec<_>>();
    sort_codex_paths(&mut candidates);
    candidates.into_iter().next()
}

pub fn default_user_runtime_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if cfg!(windows) {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            candidates.extend(codex_runtime_candidates_from_bin(
                &local_app_data.join("OpenAI").join("Codex").join("bin"),
            ));
        }
    }
    candidates.retain(|path| !is_desktop_codex_path(path));
    sort_codex_paths(&mut candidates);
    candidates
}

pub(crate) fn codex_runtime_candidates_from_bin(bin: &Path) -> Vec<PathBuf> {
    let executable_name = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut candidates = vec![bin.join(executable_name)];
    let Ok(entries) = std::fs::read_dir(bin) else {
        return candidates;
    };
    let mut nested = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| path.join(executable_name))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    nested.sort_by(|left, right| {
        file_modified_time(right)
            .cmp(&file_modified_time(left))
            .then_with(|| left.cmp(right))
    });
    candidates.extend(nested);
    candidates
}

pub(crate) fn is_desktop_codex_path(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    normalized.contains("\\windowsapps\\openai.codex_")
        || normalized.contains("\\windowsapps\\openai.codexbeta_")
        || normalized.contains("\\app\\resources\\codex.exe")
        || normalized.contains("\\app\\resources\\codex")
}

pub(crate) fn probe_codex_cli_version(executable: &Path) -> Option<CodexCliVersion> {
    probe_codex_cli_version_with_timeout(executable, VERSION_PROBE_TIMEOUT)
}

fn probe_codex_cli_version_with_timeout(
    executable: &Path,
    timeout: Duration,
) -> Option<CodexCliVersion> {
    let extension = executable.extension().and_then(|extension| {
        extension
            .to_str()
            .map(|extension| extension.to_ascii_lowercase())
    });
    let mut command = match (cfg!(windows), extension.as_deref()) {
        (true, Some("cmd" | "bat")) => {
            let command_line = format!("\"{}\" --version", executable.display());
            let mut command = Command::new("cmd.exe");
            command.args(["/d", "/s", "/c", &command_line]);
            command
        }
        (true, Some("ps1")) => {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]);
            command.arg(executable).arg("--version");
            command
        }
        _ => {
            let mut command = Command::new(executable);
            command.arg("--version");
            command
        }
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok()?;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let stdout = stdout_reader.join().ok()?.ok()?;
    let stderr = stderr_reader.join().ok()?.ok()?;
    let output = std::process::Output {
        status,
        stdout,
        stderr,
    };
    if timed_out {
        return None;
    }
    if !output.status.success()
        || output.stdout.len().saturating_add(output.stderr.len()) > MAX_VERSION_BYTES
    {
        return None;
    }
    let mut version_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version_text.is_empty() {
        version_text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    }
    parse_codex_cli_version(&version_text)
}

pub(crate) fn parse_codex_cli_version(output: &str) -> Option<CodexCliVersion> {
    let raw = output.split_whitespace().find_map(|token| {
        let token = token
            .strip_prefix('v')
            .or_else(|| token.strip_prefix('V'))
            .unwrap_or(token);
        token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
            .then_some(token)
    })?;
    let (core, prerelease) = raw
        .split_once('-')
        .map_or((raw, None), |(core, suffix)| (core, Some(suffix)));
    let core = core
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if core.is_empty() {
        return None;
    }
    let prerelease = prerelease.map(|suffix| {
        suffix
            .split('.')
            .map(|part| {
                part.parse::<u64>()
                    .map(VersionIdentifier::Numeric)
                    .unwrap_or_else(|_| VersionIdentifier::Text(part.to_ascii_lowercase()))
            })
            .collect::<Vec<_>>()
    });
    Some(CodexCliVersion {
        core,
        prerelease,
        raw: raw.to_string(),
    })
}

pub(crate) fn compare_codex_versions(left: &CodexCliVersion, right: &CodexCliVersion) -> Ordering {
    compare_numeric_components(&left.core, &right.core)
        .then_with(|| compare_prerelease(&left.prerelease, &right.prerelease))
}

fn compare_numeric_components(left: &[u64], right: &[u64]) -> Ordering {
    let length = left.len().max(right.len());
    (0..length)
        .map(|index| {
            left.get(index)
                .copied()
                .unwrap_or_default()
                .cmp(&right.get(index).copied().unwrap_or_default())
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn compare_prerelease(
    left: &Option<Vec<VersionIdentifier>>,
    right: &Option<Vec<VersionIdentifier>>,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => {
            let length = left.len().max(right.len());
            for index in 0..length {
                match (left.get(index), right.get(index)) {
                    (Some(left), Some(right)) => {
                        let ordering = compare_version_identifiers(left, right);
                        if ordering != Ordering::Equal {
                            return ordering;
                        }
                    }
                    (Some(_), None) => return Ordering::Greater,
                    (None, Some(_)) => return Ordering::Less,
                    (None, None) => break,
                }
            }
            Ordering::Equal
        }
    }
}

fn compare_version_identifiers(left: &VersionIdentifier, right: &VersionIdentifier) -> Ordering {
    match (left, right) {
        (VersionIdentifier::Numeric(left), VersionIdentifier::Numeric(right)) => left.cmp(right),
        (VersionIdentifier::Numeric(_), VersionIdentifier::Text(_)) => Ordering::Less,
        (VersionIdentifier::Text(_), VersionIdentifier::Numeric(_)) => Ordering::Greater,
        (VersionIdentifier::Text(left), VersionIdentifier::Text(right)) => left.cmp(right),
    }
}

fn sort_codex_paths(paths: &mut Vec<PathBuf>) {
    let mut ranked = paths
        .drain(..)
        .enumerate()
        .map(|(ordinal, path)| {
            let version = probe_codex_cli_version(&path);
            (path, version, ordinal)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        match (&left.1, &right.1) {
            (Some(left), Some(right)) => compare_codex_versions(right, left),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => left.2.cmp(&right.2),
        }
        .then_with(|| {
            file_modified_time(&right.0)
                .cmp(&file_modified_time(&left.0))
                .then_with(|| left.0.cmp(&right.0))
        })
    });
    paths.extend(ranked.into_iter().map(|(path, _, _)| path));
}

fn file_modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_cli_versions_sort_newest_first() {
        let old = parse_codex_cli_version("codex-cli 0.130.0-alpha.5").unwrap();
        let current = parse_codex_cli_version("codex-cli 0.166.0-alpha.5.1").unwrap();
        let stable = parse_codex_cli_version("codex-cli 0.166.0").unwrap();

        assert_eq!(compare_codex_versions(&current, &old), Ordering::Greater);
        assert_eq!(compare_codex_versions(&stable, &current), Ordering::Greater);
    }

    #[test]
    fn codex_cli_versions_accept_v_prefix_and_reject_invalid_output() {
        assert_eq!(
            parse_codex_cli_version("codex-cli v0.166.0-alpha.5.1")
                .unwrap()
                .raw,
            "0.166.0-alpha.5.1"
        );
        assert!(parse_codex_cli_version("not a version").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn codex_cli_version_probe_has_a_bounded_wait() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("slow-version.ps1");
        std::fs::write(
            &script,
            "Start-Sleep -Seconds 30\nWrite-Output 'codex-cli 0.166.0'\n",
        )
        .unwrap();
        let started = Instant::now();
        assert!(probe_codex_cli_version_with_timeout(&script, Duration::from_millis(50)).is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn desktop_codex_paths_are_excluded() {
        assert!(is_desktop_codex_path(Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0.0.0_x64__abc\app\Codex.exe"
        )));
        assert!(is_desktop_codex_path(Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0.0.0_x64__abc\app\resources\codex.exe"
        )));
        assert!(!is_desktop_codex_path(Path::new(
            r"C:\Users\me\AppData\Local\OpenAI\Codex\bin\current\codex.exe"
        )));
    }
}

pub fn wrapper_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return wrapper_dir_from_roaming(&roaming);
        }
    }
    crate::paths::default_app_state_dir().join("cli-wrapper")
}

pub fn wrapper_dir_from_roaming(roaming: &Path) -> PathBuf {
    let raw = roaming.as_os_str().to_string_lossy();
    if raw.contains('\\') && (!raw.contains('/') || cfg!(not(windows))) {
        return PathBuf::from(format!("{}\\CodexElves", raw.trim_end_matches(['\\', '/'])));
    }
    roaming.join("CodexElves")
}

pub fn cli_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(CLI_HOME_DIR))
        .unwrap_or_else(|| PathBuf::from(CLI_HOME_DIR))
}

pub fn build_wrapper_source(
    real_codex: &Path,
    codex_home: &Path,
    settings: &BackendSettings,
) -> String {
    let base_url_line = if settings.cli_wrapper_base_url.trim().is_empty() {
        String::new()
    } else {
        format!(
            r#"        startInfo.EnvironmentVariables["OPENAI_BASE_URL"] = @{};"#,
            cs_string_literal(settings.cli_wrapper_base_url.trim())
        )
    };
    let api_key_line = if settings.cli_wrapper_api_key.trim().is_empty() {
        String::new()
    } else {
        format!(
            r#"        startInfo.EnvironmentVariables[apiKeyEnv] = @{};"#,
            cs_string_literal(settings.cli_wrapper_api_key.trim())
        )
    };
    let api_key_present = if settings.cli_wrapper_api_key.trim().is_empty() {
        "false"
    } else {
        "true"
    };

    format!(
        r#"using System;
using System.Diagnostics;
using System.IO;
using System.Text;

class CodexWrapper
{{
    static int Main(string[] args)
    {{
        string realCodex = @{real_codex};
        string codexHome = @{codex_home};
        string apiKeyEnv = @{api_key_env};
        Directory.CreateDirectory(codexHome);
        string logPath = Path.Combine(codexHome, "codex-wrapper.log");
        AppendLog(logPath, "codex-wrapper start args=" + string.Join(" ", args));
        AppendLog(logPath, "real_codex=" + realCodex);
        AppendLog(logPath, "CODEX_HOME=" + codexHome);
        AppendLog(logPath, "api_key_env=" + apiKeyEnv + " api_key_present={api_key_present}");
        var startInfo = new ProcessStartInfo(realCodex);
        startInfo.UseShellExecute = false;
        startInfo.RedirectStandardInput = false;
        startInfo.RedirectStandardOutput = false;
        startInfo.RedirectStandardError = false;
        startInfo.EnvironmentVariables["CODEX_HOME"] = codexHome;
{base_url_line}
{api_key_line}
        foreach (string arg in args) startInfo.Arguments += QuoteArgument(arg) + " ";
        using (var process = Process.Start(startInfo))
        {{
            process.WaitForExit();
            AppendLog(logPath, "exit_code=" + process.ExitCode);
            return process.ExitCode;
        }}
    }}

    static void AppendLog(string path, string message)
    {{
        File.AppendAllText(path, "[" + DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + "] " + message + Environment.NewLine, Encoding.UTF8);
    }}

    static string QuoteArgument(string value)
    {{
        if (value.Length == 0) return "\"\"";
        if (value.IndexOfAny(new char[] {{ ' ', '\t', '\n', '\r', '\"' }}) < 0) return value;
        return "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
    }}
}}
"#,
        real_codex = cs_string_literal(&real_codex.to_string_lossy()),
        codex_home = cs_string_literal(&codex_home.to_string_lossy()),
        api_key_env = cs_string_literal(settings.cli_wrapper_api_key_env.trim()),
    )
}

fn compile_wrapper(source_path: &Path, wrapper_path: &Path) -> anyhow::Result<()> {
    let csc =
        find_csc().ok_or_else(|| anyhow::anyhow!("未找到 csc.exe，无法编译 CodexElves wrapper"))?;
    let output_arg = format!("/out:{}", wrapper_path.display());
    let mut command = Command::new(&csc);
    command
        .args(["/nologo", "/target:exe"])
        .arg(output_arg)
        .arg(source_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to run {}", csc.display()))?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("csc.exe exited with {status}")
    }
}

fn find_csc() -> Option<PathBuf> {
    if let Some(windir) = std::env::var_os("WINDIR").map(PathBuf::from) {
        for relative in [
            ["Microsoft.NET", "Framework64", "v4.0.30319", "csc.exe"],
            ["Microsoft.NET", "Framework", "v4.0.30319", "csc.exe"],
        ] {
            let path = relative
                .iter()
                .fold(windir.clone(), |path, segment| path.join(segment));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    Some(PathBuf::from("csc.exe"))
}

fn cs_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn csharp_string_assignment(source: &str, variable: &str) -> Option<String> {
    let marker = format!("string {variable} = @\"");
    let rest = source.split_once(&marker)?.1;
    parse_csharp_verbatim_string(rest)
}

fn csharp_environment_assignment(source: &str, key: &str) -> Option<String> {
    let marker = format!("startInfo.EnvironmentVariables{key} = @\"");
    let rest = source.split_once(&marker)?.1;
    parse_csharp_verbatim_string(rest)
}

fn parse_csharp_verbatim_string(rest: &str) -> Option<String> {
    let mut value = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if chars.peek() == Some(&'"') {
                chars.next();
                value.push('"');
                continue;
            }
            return Some(value);
        }
        value.push(ch);
    }
    None
}
