use std::ffi::OsStr;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

const CODEX_PACKAGE_IDENTITIES: &[&str] = &["OpenAI.Codex", "OpenAI.CodexBeta"];
const WINDOWS_DESKTOP_EXECUTABLE_NAMES: &[&str] = &["ChatGPT.exe", "Codex.exe", "codex.exe"];
const MACOS_DESKTOP_EXECUTABLE_NAMES: &[&str] = &["ChatGPT", "Codex"];
const MACOS_CHATGPT_APP_NAMES: &[&str] =
    &["ChatGPT.app", "OpenAI ChatGPT.app", "OpenAI.ChatGPT.app"];
const MACOS_LEGACY_CODEX_APP_NAMES: &[&str] =
    &["Codex.app", "OpenAI Codex.app", "OpenAI.Codex.app"];

pub fn find_latest_codex_app_dir(root: &Path) -> Option<PathBuf> {
    let mut matches = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| version_tuple(&path).map(|version| (version, path)))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    let (_, latest) = matches.pop()?;
    let app = latest.join("app");
    Some(if app.is_dir() { app } else { latest })
}

pub fn find_latest_codex_app_dir_from_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .filter_map(|root| find_latest_codex_app_dir(root))
        .max_by(|left, right| {
            version_tuple(left.parent().unwrap_or(left))
                .cmp(&version_tuple(right.parent().unwrap_or(right)))
        })
}

pub fn find_latest_codex_app_dir_default() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        find_latest_codex_app_dir_from_roots(&windows_app_package_roots())
            .or_else(find_latest_codex_app_dir_from_appx_package)
    }

    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn find_latest_codex_app_dir_from_appx_package() -> Option<PathBuf> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Get-AppxPackage -Name OpenAI.Codex* | Where-Object { @('OpenAI.Codex','OpenAI.CodexBeta') -contains $_.Name } | Sort-Object Version -Descending | Select-Object -First 1 -ExpandProperty InstallLocation",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    latest_appx_install_location_from_output(&String::from_utf8_lossy(&output.stdout))
        .and_then(|location| normalize_codex_app_path(Path::new(&location)))
}

pub fn latest_appx_install_location_from_output(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

#[cfg(windows)]
fn windows_app_package_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    if let Some(program_files) = std::env::var_os("ProgramW6432") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    roots.push(PathBuf::from(r"C:\Program Files\WindowsApps"));
    roots.sort();
    roots.dedup();
    roots
}

pub fn user_data_candidates() -> Vec<PathBuf> {
    user_data_candidates_from(
        std::env::var_os("LOCALAPPDATA").as_deref().map(Path::new),
        std::env::var_os("APPDATA").as_deref().map(Path::new),
    )
}

pub fn user_data_candidates_from(local: Option<&Path>, roaming: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = local {
        append_user_data_variants(&mut candidates, local);
    }
    if let Some(roaming) = roaming {
        append_user_data_variants(&mut candidates, roaming);
    }
    candidates
}

pub fn find_macos_codex_app(search_roots: &[PathBuf]) -> Option<PathBuf> {
    for root in search_roots {
        if is_macos_app_bundle(root) && root.is_dir() {
            return Some(root.to_path_buf());
        }
    }
    for app_name in MACOS_CHATGPT_APP_NAMES {
        for root in search_roots
            .iter()
            .filter(|root| !is_macos_app_bundle(root))
        {
            let candidate = root.join(app_name);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }
    for root in search_roots
        .iter()
        .filter(|root| !is_macos_app_bundle(root))
    {
        for app_name in MACOS_LEGACY_CODEX_APP_NAMES {
            let candidate = root.join(app_name);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn find_macos_codex_app_default() -> Option<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
        roots.push(home.join("Applications"));
    }
    find_macos_codex_app(&roots)
}

pub fn resolve_codex_app_dir(app_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(app_dir) = app_dir {
        return normalize_codex_app_path(app_dir);
    }
    if cfg!(target_os = "macos") {
        return find_macos_codex_app_default();
    }
    // Windows: try MS Store version first, then standalone install
    find_latest_codex_app_dir_default().or_else(|| find_standalone_codex_app_dir())
}

/// Search for standalone Codex installations (non-MS Store).
///
/// Common paths:
/// - %LOCALAPPDATA%\OpenAI\Codex\bin\  (standalone installer)
/// - %LOCALAPPDATA%\OpenAI\Codex\      (user data root)
/// - %LOCALAPPDATA%\Programs\OpenAI\Codex\ (alternative)
/// - matching ChatGPT paths when a Codex runtime marker is present
pub fn find_standalone_codex_app_dir() -> Option<PathBuf> {
    let local_appdata = std::env::var_os("LOCALAPPDATA")?;
    find_standalone_codex_app_dir_from(Path::new(&local_appdata))
}

pub fn find_standalone_codex_app_dir_from(local_appdata: &Path) -> Option<PathBuf> {
    let codex_candidates = [
        local_appdata.join("OpenAI").join("Codex").join("bin"),
        local_appdata.join("OpenAI").join("Codex"),
        local_appdata.join("Programs").join("OpenAI").join("Codex"),
    ];
    if let Some(path) = find_standalone_app_candidate(&codex_candidates, false) {
        return Some(path);
    }

    let chatgpt_candidates = [
        local_appdata.join("OpenAI").join("ChatGPT").join("bin"),
        local_appdata.join("OpenAI").join("ChatGPT"),
        local_appdata
            .join("Programs")
            .join("OpenAI")
            .join("ChatGPT"),
        local_appdata.join("Programs").join("ChatGPT"),
    ];
    find_standalone_app_candidate(&chatgpt_candidates, true)
}

pub fn resolve_codex_app_dir_with_saved(
    app_dir: Option<&Path>,
    saved_app_path: Option<&str>,
) -> Option<PathBuf> {
    if let Some(app_dir) = app_dir {
        return normalize_codex_app_path(app_dir);
    }
    if let Some(saved) = saved_app_path
        .map(str::trim)
        .filter(|saved| !saved.is_empty())
    {
        if let Some(path) = normalize_codex_app_path(Path::new(saved)) {
            return Some(path);
        }
    }
    resolve_codex_app_dir(None)
}

pub fn normalize_codex_app_path(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }

    let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
    if is_windows_desktop_executable_name(file_name) {
        let app_dir = normalize_windows_desktop_executable_parent(path)?;
        if path.is_file() || windows_desktop_executable(&app_dir).is_some() {
            return Some(app_dir);
        }
        return None;
    }

    if is_macos_app_bundle(path) {
        if path.is_dir() {
            return Some(path.to_path_buf());
        }
        let parent = path.parent()?;
        for app_name in MACOS_CHATGPT_APP_NAMES {
            let candidate = parent.join(app_name);
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
        return None;
    }

    if path.is_file() {
        return None;
    }

    if windows_desktop_executable(path).is_some() {
        return Some(path.to_path_buf());
    }

    let nested_app = path.join("app");
    if nested_app.is_dir() && windows_desktop_executable(&nested_app).is_some() {
        return Some(nested_app);
    }

    if path.is_dir() {
        return Some(path.to_path_buf());
    }

    None
}

pub fn build_codex_executable(app_dir: &Path) -> PathBuf {
    if is_macos_app_bundle(app_dir) {
        let macos_dir = app_dir.join("Contents").join("MacOS");
        if let Some(executable) = macos_bundle_executable_name(app_dir) {
            return macos_dir.join(executable);
        }
        if let Some(executable) = MACOS_DESKTOP_EXECUTABLE_NAMES
            .iter()
            .map(|name| macos_dir.join(name))
            .find(|candidate| candidate.exists())
        {
            return executable;
        }
        let fallback = if app_dir
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.to_ascii_lowercase().contains("chatgpt"))
        {
            "ChatGPT"
        } else {
            "Codex"
        };
        return macos_dir.join(fallback);
    }
    windows_desktop_executable(app_dir).unwrap_or_else(|| app_dir.join("ChatGPT.exe"))
}

pub fn codex_app_version(app_dir: &Path) -> Option<String> {
    if is_macos_app_bundle(app_dir) {
        return macos_app_version(app_dir);
    }
    let package_dir = if app_dir
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("app"))
    {
        app_dir.parent()?
    } else {
        app_dir
    };
    codex_package_version(package_dir)
}

pub fn packaged_app_user_model_id(app_dir: &Path) -> Option<String> {
    let package_name = package_name_from_app_dir(app_dir)?;
    let (identity_name, _, publisher_id) = codex_package_parts(&package_name)?;
    if publisher_id.is_empty() {
        return None;
    }
    Some(format!("{identity_name}_{publisher_id}!App"))
}

fn package_name_from_app_dir(app_dir: &Path) -> Option<String> {
    let path = app_dir.to_string_lossy().replace('\\', "/");
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let mut package_name = parts.next_back()?;
    if package_name.eq_ignore_ascii_case("app") {
        package_name = parts.next_back()?;
    }
    Some(package_name.to_string())
}

fn codex_package_version(package_dir: &Path) -> Option<String> {
    let path = package_dir.to_string_lossy().replace('\\', "/");
    let name = path
        .split('/')
        .rev()
        .find(|part| codex_package_parts(part).is_some())?;
    let (_, version, _) = codex_package_parts(name)?;
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

fn macos_app_version(app_dir: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app_dir.join("Contents").join("Info.plist")).ok()?;
    plist_string_value(&plist, "CFBundleShortVersionString")
        .or_else(|| plist_string_value(&plist, "CFBundleVersion"))
}

fn macos_bundle_executable_name(app_dir: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app_dir.join("Contents").join("Info.plist")).ok()?;
    plist_string_value(&plist, "CFBundleExecutable")
}

fn plist_string_value(plist: &str, key: &str) -> Option<String> {
    let (_, after_key) = plist.split_once(&format!("<key>{key}</key>"))?;
    let (_, after_string_open) = after_key.split_once("<string>")?;
    let (value, _) = after_string_open.split_once("</string>")?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn append_user_data_variants(candidates: &mut Vec<PathBuf>, base: &Path) {
    candidates.push(base.join("OpenAI").join("Codex"));
    candidates.push(base.join("OpenAI.Codex"));
    candidates.push(base.join("Codex"));
}

fn is_macos_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn is_windows_desktop_executable_name(file_name: &str) -> bool {
    WINDOWS_DESKTOP_EXECUTABLE_NAMES
        .iter()
        .any(|candidate| file_name.eq_ignore_ascii_case(candidate))
}

fn windows_desktop_executable(app_dir: &Path) -> Option<PathBuf> {
    WINDOWS_DESKTOP_EXECUTABLE_NAMES
        .iter()
        .map(|name| app_dir.join(name))
        .find(|candidate| candidate.exists())
}

fn find_standalone_app_candidate(
    candidates: &[PathBuf],
    require_codex_runtime_marker: bool,
) -> Option<PathBuf> {
    for candidate in candidates {
        let Some(path) = normalize_codex_app_path(candidate) else {
            continue;
        };
        if build_codex_executable(&path).exists()
            && (!require_codex_runtime_marker || has_codex_runtime_marker(&path))
        {
            return Some(path);
        }
    }
    None
}

fn has_codex_runtime_marker(app_dir: &Path) -> bool {
    app_dir.join("Codex.exe").exists()
        || app_dir.join("codex.exe").exists()
        || app_dir.join("resources").join("codex.exe").exists()
}

fn normalize_windows_desktop_executable_parent(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    if parent
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("resources"))
    {
        if let Some(app_dir) = parent.parent()
            && windows_desktop_executable(app_dir).is_some()
        {
            return Some(app_dir.to_path_buf());
        }
    }
    Some(parent.to_path_buf())
}

fn version_tuple(path: &Path) -> Option<Vec<u32>> {
    let name = path.file_name()?.to_str()?;
    let (_, version, _) = codex_package_parts(name)?;
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if parts.is_empty() { None } else { Some(parts) }
}

fn codex_package_parts(package_name: &str) -> Option<(&str, &str, &str)> {
    for identity in CODEX_PACKAGE_IDENTITIES {
        let Some(rest) = package_name.strip_prefix(identity) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('_') else {
            continue;
        };
        let Some((version, rest)) = rest.split_once('_') else {
            continue;
        };
        let Some((_, publisher_id)) = rest.rsplit_once("__") else {
            continue;
        };
        return Some((*identity, version, publisher_id));
    }
    None
}
