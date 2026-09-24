#![cfg_attr(not(windows), allow(dead_code))]

use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, Item, Table};

const BUNDLED_MARKETPLACE: &str = "openai-bundled";
const BUNDLED_MARKETPLACE_PLUGINS: &[&str] = &["browser", "chrome", "computer-use", "latex"];
const COMPUTER_USE_PLUGINS: &[&str] = &[
    "browser@openai-bundled",
    "chrome@openai-bundled",
    "computer-use@openai-bundled",
];
const COMPUTER_USE_EXE: &str = "codex-computer-use.exe";
const COMPUTER_USE_CLIENT_SCRIPT: &str = "computer-use-client.mjs";
const SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT: &str =
    "./dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js";
const SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT: &str =
    "@oai/sky/dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js";
const SKY_PACKAGE_EXPORTS_BACKUP: &str = "package.json.bak-codexpp-runtime-exports";
const BROWSER_SERVICE_SCRIPT: &str = "browser-service.mjs";
const BROWSER_SERVICE_LEGACY_BACKUP: &str = "browser-service.mjs.bak-codexelves-apikey-browser";
const BROWSER_SERVICE_BACKUP_PREFIX: &str = "browser-service.mjs.bak-codexelves-";
const BROWSER_REQUEST_HEADER_COMPAT_METADATA: &str = "browser-service.mjs.codexelves-patch.json";
const BROWSER_REQUEST_HEADER_COMPAT_PATCH_VERSION: u32 = 1;
const BROWSER_REQUEST_HEADER_IDENTITY_ERROR: &str =
    "Browser request-header policy requires caller identity.";
const BROWSER_REQUEST_HEADER_FEATURE_FLAG: &str = "codex_browser_use_agent_request_header";
const BROWSER_REQUEST_HEADER_COMPAT_MARKER: &str = "/*codexelves-api-key-browser-compat-v1*/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardResult {
    pub changed: bool,
    pub notify_exe: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardArtifacts {
    pub notify_exe: Option<PathBuf>,
    pub marketplace_path: Option<PathBuf>,
    pub sky_package_json: Option<PathBuf>,
    pub browser_service_script: Option<PathBuf>,
    pub runtime_exports_needed: bool,
}

pub(crate) fn resolve_computer_use_guard_artifacts(home: &Path) -> anyhow::Result<GuardArtifacts> {
    #[cfg(windows)]
    {
        resolve_computer_use_guard_artifacts_windows(home, ensure_openai_bundled_marketplace(home)?)
    }
    #[cfg(not(windows))]
    {
        let _ = home;
        Ok(GuardArtifacts {
            notify_exe: None,
            marketplace_path: None,
            sky_package_json: None,
            browser_service_script: None,
            runtime_exports_needed: false,
        })
    }
}

pub(crate) fn refresh_computer_use_guard_artifacts(
    home: &Path,
    previous: Option<&GuardArtifacts>,
) -> anyhow::Result<GuardArtifacts> {
    #[cfg(windows)]
    {
        let marketplace_path = previous
            .and_then(|artifacts| artifacts.marketplace_path.as_deref())
            .filter(|path| path.is_dir())
            .map(Path::to_path_buf);
        let marketplace_path = match marketplace_path {
            Some(path) => Some(path),
            None => ensure_openai_bundled_marketplace(home)?,
        };
        resolve_computer_use_guard_artifacts_windows(home, marketplace_path)
    }
    #[cfg(not(windows))]
    {
        let _ = (home, previous);
        Ok(GuardArtifacts {
            notify_exe: None,
            marketplace_path: None,
            sky_package_json: None,
            browser_service_script: None,
            runtime_exports_needed: false,
        })
    }
}

#[cfg(windows)]
fn resolve_computer_use_guard_artifacts_windows(
    home: &Path,
    marketplace_path: Option<PathBuf>,
) -> anyhow::Result<GuardArtifacts> {
    let notify_exe = find_computer_use_notify_exe(home);
    let runtime_exports_needed = computer_use_client_needs_sky_internal_export(home)?;
    let browser_service_script = resolve_browser_service_script(home, notify_exe.as_deref());
    Ok(GuardArtifacts {
        sky_package_json: find_sky_package_json_for_notify_exe(notify_exe.as_deref())
            .or_else(find_latest_sky_package_json),
        notify_exe,
        marketplace_path,
        browser_service_script,
        runtime_exports_needed,
    })
}

pub(crate) fn ensure_computer_use_config_with_artifacts(
    home: &Path,
    artifacts: &GuardArtifacts,
    browser_compat_enabled: bool,
) -> anyhow::Result<GuardResult> {
    #[cfg(windows)]
    {
        ensure_computer_use_config_with_artifacts_windows(home, artifacts, browser_compat_enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = (home, artifacts, browser_compat_enabled);
        Ok(GuardResult {
            changed: false,
            notify_exe: None,
        })
    }
}

#[cfg(windows)]
fn ensure_computer_use_config_with_artifacts_windows(
    home: &Path,
    artifacts: &GuardArtifacts,
    browser_compat_enabled: bool,
) -> anyhow::Result<GuardResult> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read(&config_path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let updated = if let Some(marketplace_path) = artifacts.marketplace_path.as_deref() {
        guard_config_text_with_marketplace(
            &existing,
            artifacts.notify_exe.as_deref(),
            Some(marketplace_path),
        )?
    } else {
        guard_config_text(&existing, artifacts.notify_exe.as_deref())?
    };
    let changed = updated.as_bytes() != existing.as_bytes();
    if changed {
        crate::settings::atomic_write(&config_path, updated.as_bytes())?;
    }
    let runtime_compat = ensure_computer_use_runtime_exports_compat_windows(
        home,
        artifacts.sky_package_json.as_deref(),
    )?;
    let browser_auth_compat = ensure_browser_request_header_compat_windows(
        artifacts.browser_service_script.as_deref(),
        browser_compat_enabled,
    )?;
    Ok(GuardResult {
        changed: changed || runtime_compat.changed || browser_auth_compat.changed,
        notify_exe: artifacts.notify_exe.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCompatResult {
    pub changed: bool,
    pub package_json: Option<PathBuf>,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserRequestHeaderCompatResult {
    pub changed: bool,
    pub script_path: Option<PathBuf>,
    pub backup_path: Option<PathBuf>,
    pub metadata_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserRequestHeaderCompatMetadata {
    patch_version: u32,
    original_sha256: String,
    patched_sha256: String,
    backup_file_name: String,
}

#[cfg(not(windows))]
pub(crate) fn ensure_computer_use_runtime_exports_compat(
    home: &Path,
) -> anyhow::Result<RuntimeCompatResult> {
    let _ = home;
    Ok(RuntimeCompatResult {
        changed: false,
        package_json: None,
        backup_path: None,
    })
}

#[cfg(windows)]
#[allow(dead_code)]
pub(crate) fn ensure_computer_use_runtime_exports_compat(
    home: &Path,
) -> anyhow::Result<RuntimeCompatResult> {
    ensure_computer_use_runtime_exports_compat_windows(
        home,
        find_latest_sky_package_json().as_deref(),
    )
}

#[cfg(windows)]
fn ensure_computer_use_runtime_exports_compat_windows(
    home: &Path,
    sky_package_json: Option<&Path>,
) -> anyhow::Result<RuntimeCompatResult> {
    if !computer_use_client_needs_sky_internal_export(home)? {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: sky_package_json.map(Path::to_path_buf),
            backup_path: None,
        });
    }
    let Some(package_json) = sky_package_json else {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: None,
            backup_path: None,
        });
    };
    if !sky_internal_computer_use_client_file_exists(package_json) {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: Some(package_json.to_path_buf()),
            backup_path: None,
        });
    }

    let existing = std::fs::read_to_string(package_json)
        .with_context(|| format!("failed to read {}", package_json.display()))?;
    let Some(updated) = add_sky_internal_computer_use_export(&existing)? else {
        return Ok(RuntimeCompatResult {
            changed: false,
            package_json: Some(package_json.to_path_buf()),
            backup_path: None,
        });
    };

    let backup_path = package_json
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid @oai/sky package.json path"))?
        .join(SKY_PACKAGE_EXPORTS_BACKUP);
    if !backup_path.exists() {
        std::fs::copy(package_json, &backup_path).with_context(|| {
            format!(
                "failed to back up {} to {}",
                package_json.display(),
                backup_path.display()
            )
        })?;
    }
    atomic_write_runtime_file(package_json, updated.as_bytes())?;
    Ok(RuntimeCompatResult {
        changed: true,
        package_json: Some(package_json.to_path_buf()),
        backup_path: Some(backup_path),
    })
}

pub(crate) fn reconcile_browser_request_header_compat(
    home: &Path,
    enabled: bool,
) -> anyhow::Result<BrowserRequestHeaderCompatResult> {
    #[cfg(windows)]
    {
        let notify_exe = find_computer_use_notify_exe(home);
        let browser_service_script = resolve_browser_service_script(home, notify_exe.as_deref());
        ensure_browser_request_header_compat_windows(browser_service_script.as_deref(), enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = (home, enabled);
        Ok(BrowserRequestHeaderCompatResult {
            changed: false,
            script_path: None,
            backup_path: None,
            metadata_path: None,
        })
    }
}

#[cfg(windows)]
fn ensure_browser_request_header_compat_windows(
    browser_service_script: Option<&Path>,
    enabled: bool,
) -> anyhow::Result<BrowserRequestHeaderCompatResult> {
    let Some(browser_service_script) = browser_service_script else {
        return Ok(BrowserRequestHeaderCompatResult {
            changed: false,
            script_path: None,
            backup_path: None,
            metadata_path: None,
        });
    };
    let parent = browser_service_script
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid browser-service.mjs path"))?;
    let metadata_path = parent.join(BROWSER_REQUEST_HEADER_COMPAT_METADATA);
    let existing = std::fs::read(browser_service_script)
        .with_context(|| format!("failed to read {}", browser_service_script.display()))?;
    let existing_hash = sha256_hex(&existing);
    let contains_marker = existing
        .windows(BROWSER_REQUEST_HEADER_COMPAT_MARKER.len())
        .any(|window| window == BROWSER_REQUEST_HEADER_COMPAT_MARKER.as_bytes());

    if !contains_marker {
        if !enabled {
            return Ok(BrowserRequestHeaderCompatResult {
                changed: false,
                script_path: Some(browser_service_script.to_path_buf()),
                backup_path: None,
                metadata_path: Some(metadata_path),
            });
        }
        let existing_text = std::str::from_utf8(&existing).with_context(|| {
            format!("failed to read UTF-8 {}", browser_service_script.display())
        })?;
        let Some(updated) = patch_browser_request_header_policy(existing_text) else {
            return Ok(BrowserRequestHeaderCompatResult {
                changed: false,
                script_path: Some(browser_service_script.to_path_buf()),
                backup_path: None,
                metadata_path: Some(metadata_path),
            });
        };
        let backup_path = browser_service_backup_path(parent, &existing_hash);
        ensure_browser_service_backup(&backup_path, &existing, &existing_hash)?;
        let metadata = BrowserRequestHeaderCompatMetadata {
            patch_version: BROWSER_REQUEST_HEADER_COMPAT_PATCH_VERSION,
            original_sha256: existing_hash,
            patched_sha256: sha256_hex(updated.as_bytes()),
            backup_file_name: backup_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid browser-service.mjs backup path"))?
                .to_string(),
        };
        write_browser_request_header_compat_metadata(&metadata_path, &metadata)?;
        atomic_write_runtime_file(browser_service_script, updated.as_bytes())?;
        return Ok(BrowserRequestHeaderCompatResult {
            changed: true,
            script_path: Some(browser_service_script.to_path_buf()),
            backup_path: Some(backup_path),
            metadata_path: Some(metadata_path),
        });
    }

    let mut metadata_changed = false;
    let validated = match read_browser_request_header_compat_metadata(&metadata_path)? {
        Some(metadata) => validate_browser_request_header_compat_metadata(
            parent,
            &metadata,
            &existing,
            &existing_hash,
        )?,
        None => None,
    };
    let validated = match validated {
        Some(validated) => Some(validated),
        None => {
            let Some(recovered) =
                recover_browser_request_header_compat_metadata(parent, &existing)?
            else {
                return Ok(BrowserRequestHeaderCompatResult {
                    changed: false,
                    script_path: Some(browser_service_script.to_path_buf()),
                    backup_path: None,
                    metadata_path: Some(metadata_path),
                });
            };
            write_browser_request_header_compat_metadata(&metadata_path, &recovered.metadata)?;
            metadata_changed = true;
            Some(recovered)
        }
    };
    let validated = validated.expect("validated browser compatibility metadata");
    if enabled {
        return Ok(BrowserRequestHeaderCompatResult {
            changed: metadata_changed,
            script_path: Some(browser_service_script.to_path_buf()),
            backup_path: Some(validated.backup_path),
            metadata_path: Some(metadata_path),
        });
    }

    atomic_write_runtime_file(browser_service_script, &validated.original)?;
    Ok(BrowserRequestHeaderCompatResult {
        changed: true,
        script_path: Some(browser_service_script.to_path_buf()),
        backup_path: Some(validated.backup_path),
        metadata_path: Some(metadata_path),
    })
}

#[cfg(windows)]
struct ValidatedBrowserRequestHeaderCompat {
    metadata: BrowserRequestHeaderCompatMetadata,
    backup_path: PathBuf,
    original: Vec<u8>,
}

#[cfg(windows)]
fn read_browser_request_header_compat_metadata(
    metadata_path: &Path,
) -> anyhow::Result<Option<BrowserRequestHeaderCompatMetadata>> {
    match std::fs::read(metadata_path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read {}", metadata_path.display()))
        }
    }
}

#[cfg(windows)]
fn validate_browser_request_header_compat_metadata(
    parent: &Path,
    metadata: &BrowserRequestHeaderCompatMetadata,
    current: &[u8],
    current_hash: &str,
) -> anyhow::Result<Option<ValidatedBrowserRequestHeaderCompat>> {
    if metadata.patch_version != BROWSER_REQUEST_HEADER_COMPAT_PATCH_VERSION
        || !is_sha256_hex(&metadata.original_sha256)
        || !is_sha256_hex(&metadata.patched_sha256)
        || metadata.patched_sha256 != current_hash
    {
        return Ok(None);
    }
    let expected_backup_file_name = browser_service_backup_file_name(&metadata.original_sha256);
    if metadata.backup_file_name != expected_backup_file_name {
        return Ok(None);
    }
    let backup_path = parent.join(&expected_backup_file_name);
    let original = match std::fs::read(&backup_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", backup_path.display()));
        }
    };
    if sha256_hex(&original) != metadata.original_sha256 {
        return Ok(None);
    }
    let Ok(original_text) = std::str::from_utf8(&original) else {
        return Ok(None);
    };
    let Some(repatched) = patch_browser_request_header_policy(original_text) else {
        return Ok(None);
    };
    if repatched.as_bytes() != current {
        return Ok(None);
    }
    Ok(Some(ValidatedBrowserRequestHeaderCompat {
        metadata: metadata.clone(),
        backup_path,
        original,
    }))
}

#[cfg(windows)]
fn recover_browser_request_header_compat_metadata(
    parent: &Path,
    current: &[u8],
) -> anyhow::Result<Option<ValidatedBrowserRequestHeaderCompat>> {
    let mut candidates = vec![parent.join(BROWSER_SERVICE_LEGACY_BACKUP)];
    for entry in std::fs::read_dir(parent)
        .with_context(|| format!("failed to read {}", parent.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(BROWSER_SERVICE_BACKUP_PREFIX))
        {
            candidates.push(path);
        }
    }
    candidates.sort();
    candidates.dedup();

    let mut recovered: Option<(String, Vec<u8>)> = None;
    for candidate in candidates {
        let Ok(original) = std::fs::read(&candidate) else {
            continue;
        };
        let Ok(original_text) = std::str::from_utf8(&original) else {
            continue;
        };
        let Some(repatched) = patch_browser_request_header_policy(original_text) else {
            continue;
        };
        if repatched.as_bytes() != current {
            continue;
        }
        let original_hash = sha256_hex(&original);
        if recovered
            .as_ref()
            .is_some_and(|(recovered_hash, _)| recovered_hash != &original_hash)
        {
            return Ok(None);
        }
        recovered = Some((original_hash, original));
    }

    let Some((original_hash, original)) = recovered else {
        return Ok(None);
    };
    let backup_path = browser_service_backup_path(parent, &original_hash);
    ensure_browser_service_backup(&backup_path, &original, &original_hash)?;
    let metadata = BrowserRequestHeaderCompatMetadata {
        patch_version: BROWSER_REQUEST_HEADER_COMPAT_PATCH_VERSION,
        original_sha256: original_hash,
        patched_sha256: sha256_hex(current),
        backup_file_name: backup_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid browser-service.mjs backup path"))?
            .to_string(),
    };
    Ok(Some(ValidatedBrowserRequestHeaderCompat {
        metadata,
        backup_path,
        original,
    }))
}

#[cfg(windows)]
fn ensure_browser_service_backup(
    backup_path: &Path,
    original: &[u8],
    original_hash: &str,
) -> anyhow::Result<()> {
    if std::fs::read(backup_path)
        .ok()
        .is_some_and(|existing| sha256_hex(&existing) == original_hash)
    {
        return Ok(());
    }
    atomic_write_runtime_file(backup_path, original)
}

#[cfg(windows)]
fn write_browser_request_header_compat_metadata(
    metadata_path: &Path,
    metadata: &BrowserRequestHeaderCompatMetadata,
) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(metadata)?;
    bytes.push(b'\n');
    atomic_write_runtime_file(metadata_path, &bytes)
}

fn browser_service_backup_file_name(original_hash: &str) -> String {
    format!("{BROWSER_SERVICE_BACKUP_PREFIX}{original_hash}")
}

fn browser_service_backup_path(parent: &Path, original_hash: &str) -> PathBuf {
    parent.join(browser_service_backup_file_name(original_hash))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(crate) fn guard_config_text(
    config_text: &str,
    notify_exe: Option<&Path>,
) -> anyhow::Result<String> {
    guard_config_text_with_marketplace(config_text, notify_exe, None)
}

pub(crate) fn guard_config_text_with_marketplace(
    config_text: &str,
    notify_exe: Option<&Path>,
    marketplace_path: Option<&Path>,
) -> anyhow::Result<String> {
    let without_bom = config_text.trim_start_matches('\u{feff}');
    let mut doc = parse_toml_document(without_bom)?;

    let features = table_mut_or_insert(&mut doc, "features")?;
    features["js_repl"] = toml_edit::value(true);

    for plugin_id in COMPUTER_USE_PLUGINS {
        ensure_plugin_enabled(&mut doc, plugin_id)?;
    }

    if let Some(notify_exe) = notify_exe {
        let mut notify = Array::default();
        notify.push(notify_exe.to_string_lossy().as_ref());
        notify.push("turn-ended");
        doc["notify"] = toml_edit::value(notify);
    }

    if let Some(marketplace_path) = marketplace_path {
        ensure_openai_bundled_marketplace_config(&mut doc, marketplace_path)?;
    }

    Ok(ensure_trailing_newline(doc.to_string()))
}

pub(crate) fn find_computer_use_notify_exe(home: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        find_computer_use_notify_exe_windows(home)
    }
    #[cfg(not(windows))]
    {
        let _ = home;
        None
    }
}

#[cfg(windows)]
fn find_computer_use_notify_exe_windows(home: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        collect_named_files(
            &PathBuf::from(local_app_data)
                .join("OpenAI")
                .join("Codex")
                .join("runtimes")
                .join("cua_node"),
            COMPUTER_USE_EXE,
            12,
            &mut candidates,
        );
    }
    if candidates.is_empty() {
        collect_named_files(
            &home
                .join("plugins")
                .join("cache")
                .join("openai-bundled")
                .join("computer-use"),
            COMPUTER_USE_EXE,
            12,
            &mut candidates,
        );
    }
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn collect_named_files(root: &Path, file_name: &str, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
            {
                output.push(path);
            }
        } else if path.is_dir() {
            collect_named_files(&path, file_name, depth - 1, output);
        }
    }
}

#[cfg(windows)]
fn modified_millis(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(windows)]
fn computer_use_client_needs_sky_internal_export(home: &Path) -> anyhow::Result<bool> {
    let mut candidates = Vec::new();
    collect_named_files(
        &home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use"),
        COMPUTER_USE_CLIENT_SCRIPT,
        8,
        &mut candidates,
    );
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    for candidate in candidates {
        let contents = std::fs::read_to_string(&candidate)
            .with_context(|| format!("failed to read {}", candidate.display()))?;
        if contents.contains(SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn find_sky_package_json_for_notify_exe(notify_exe: Option<&Path>) -> Option<PathBuf> {
    let notify_exe = notify_exe?;
    for ancestor in notify_exe.ancestors() {
        if ancestor
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("sky"))
            && ancestor
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("@oai"))
        {
            let package_json = ancestor.join("package.json");
            if package_json.is_file() {
                return Some(package_json);
            }
        }
    }
    None
}

#[cfg(windows)]
fn find_browser_service_script_for_notify_exe(notify_exe: Option<&Path>) -> Option<PathBuf> {
    let notify_exe = notify_exe?;
    for ancestor in notify_exe.ancestors() {
        if ancestor
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("@oai"))
        {
            let browser_service_script = ancestor
                .join("browser-desktop")
                .join("scripts")
                .join(BROWSER_SERVICE_SCRIPT);
            if browser_service_script.is_file() {
                return Some(browser_service_script);
            }
        }
    }
    None
}

#[cfg(windows)]
fn resolve_browser_service_script(home: &Path, notify_exe: Option<&Path>) -> Option<PathBuf> {
    configured_browser_service_script(home)
        .or_else(|| find_browser_service_script_for_notify_exe(notify_exe))
        .or_else(find_latest_browser_service_script)
}

#[cfg(windows)]
fn configured_browser_service_script(home: &Path) -> Option<PathBuf> {
    let config = std::fs::read_to_string(home.join("config.toml")).ok()?;
    let without_bom = config.trim_start_matches('\u{feff}');
    let doc = parse_toml_document(without_bom).ok()?;
    let trusted_services = doc
        .get("shell_environment_policy")?
        .as_table_like()?
        .get("set")?
        .as_table_like()?
        .get("NODE_REPL_TRUSTED_SERVICES")?
        .as_str()?;
    let trusted_services: serde_json::Value = serde_json::from_str(trusted_services).ok()?;
    let browser = trusted_services.get("browser")?.as_str()?;
    let browser = PathBuf::from(browser);
    (browser.is_absolute() && browser.is_file()).then_some(browser)
}

#[cfg(windows)]
fn find_latest_sky_package_json() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let runtimes = PathBuf::from(local_app_data)
        .join("OpenAI")
        .join("Codex")
        .join("runtimes")
        .join("cua_node");
    let Ok(entries) = std::fs::read_dir(runtimes) else {
        return None;
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join("bin")
                .join("node_modules")
                .join("@oai")
                .join("sky")
                .join("package.json")
        })
        .filter(|path| path.is_file())
        .collect();
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn find_latest_browser_service_script() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let runtimes = PathBuf::from(local_app_data)
        .join("OpenAI")
        .join("Codex")
        .join("runtimes")
        .join("cua_node");
    let Ok(entries) = std::fs::read_dir(runtimes) else {
        return None;
    };
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join("bin")
                .join("node_modules")
                .join("@oai")
                .join("browser-desktop")
                .join("scripts")
                .join(BROWSER_SERVICE_SCRIPT)
        })
        .filter(|path| path.is_file())
        .collect();
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn sky_internal_computer_use_client_file_exists(package_json: &Path) -> bool {
    let Some(package_root) = package_json.parent() else {
        return false;
    };
    package_root
        .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"))
        .is_file()
}

fn add_sky_internal_computer_use_export(contents: &str) -> anyhow::Result<Option<String>> {
    let mut package: serde_json::Value =
        serde_json::from_str(contents).with_context(|| "@oai/sky package.json parse failed")?;
    let Some(exports) = package
        .get_mut("exports")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(None);
    };
    if exports.contains_key(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT) {
        return Ok(None);
    }
    exports.insert(
        SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.to_string(),
        serde_json::Value::String(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.to_string()),
    );
    let mut updated = serde_json::to_string_pretty(&package)?;
    updated.push('\n');
    Ok(Some(updated))
}

fn patch_browser_request_header_policy(contents: &str) -> Option<String> {
    if contents.contains(BROWSER_REQUEST_HEADER_COMPAT_MARKER) {
        return None;
    }
    let identity_error_matches = contents
        .match_indices(BROWSER_REQUEST_HEADER_IDENTITY_ERROR)
        .collect::<Vec<_>>();
    if identity_error_matches.len() != 1 {
        return None;
    }
    let identity_error_offset = identity_error_matches[0].0;
    let function_start = contents[..identity_error_offset].rfind("async function ")?;
    let open_brace = contents[function_start..]
        .find('{')
        .map(|offset| function_start + offset)?;
    let function_header = contents[function_start..open_brace].trim_end();
    let function_name = function_header
        .strip_prefix("async function ")?
        .strip_suffix("()")?;
    if !is_simple_javascript_identifier(function_name) {
        return None;
    }
    let function_end = contents[open_brace + 1..]
        .find('}')
        .map(|offset| open_brace + 1 + offset)?;
    if identity_error_offset > function_end {
        return None;
    }
    let function_text = &contents[function_start..=function_end];
    if function_text
        .match_indices(BROWSER_REQUEST_HEADER_FEATURE_FLAG)
        .count()
        != 1
    {
        return None;
    }
    let throw_expression = format!("throw new Error(\"{BROWSER_REQUEST_HEADER_IDENTITY_ERROR}\")");
    if function_text.match_indices(&throw_expression).count() != 1 {
        return None;
    }

    let body = &contents[open_brace + 1..function_end];
    if body.len() > 512 || body.contains('{') || body.match_indices(";return await ").count() != 1 {
        return None;
    }
    let (guard_expression, return_expression) = body.split_once(";return await ")?;
    let identity_name = guard_expression
        .strip_suffix(&throw_expression)?
        .strip_prefix("if(")?
        .strip_suffix("==null)")?;
    let feature_flag_suffix = format!("(\"{BROWSER_REQUEST_HEADER_FEATURE_FLAG}\")");
    let feature_flag_function = return_expression
        .strip_prefix(identity_name)?
        .strip_prefix(',')?
        .strip_suffix(&feature_flag_suffix)?;
    if !is_simple_javascript_identifier(identity_name)
        || !is_simple_javascript_identifier(feature_flag_function)
    {
        return None;
    }
    let patched_body = body.replacen(&throw_expression, "return!1", 1);
    let patched_function = format!(
        "{}{{try{{{patched_body}}}catch{{return!1}}}}{BROWSER_REQUEST_HEADER_COMPAT_MARKER}",
        &contents[function_start..open_brace]
    );
    let mut updated =
        String::with_capacity(contents.len() + patched_function.len() - function_text.len());
    updated.push_str(&contents[..function_start]);
    updated.push_str(&patched_function);
    updated.push_str(&contents[function_end + 1..]);
    Some(updated)
}

fn is_simple_javascript_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || matches!(first, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

#[cfg(windows)]
fn atomic_write_runtime_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid runtime file path"))?;
    let temp = parent.join(format!(
        ".{}.codexpp-tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("package.json")
    ));
    std::fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error).with_context(|| format!("failed to replace {}", path.display()))
        }
    }
}

#[cfg(windows)]
pub(crate) fn ensure_openai_bundled_marketplace(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let active = home
        .join(".tmp")
        .join("bundled-marketplaces")
        .join(BUNDLED_MARKETPLACE);
    if is_complete_openai_bundled_marketplace(&active) {
        return Ok(Some(active));
    }
    if let Some(configured) = configured_openai_bundled_marketplace(home) {
        if is_complete_openai_bundled_marketplace(&configured) {
            return Ok(Some(configured));
        }
    }

    let parent = active
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid bundled marketplace path"))?;
    std::fs::create_dir_all(parent)?;

    let staging = parent.join(format!(
        "{BUNDLED_MARKETPLACE}.guard-staging-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }

    if let Some(source) = find_complete_openai_bundled_marketplace(parent, &active) {
        copy_dir_recursive(&source, &staging)?;
    } else if can_build_marketplace_from_cache(home) {
        build_marketplace_from_cache(home, &staging)?;
    } else {
        return Ok(None);
    }

    match replace_active_marketplace(&active, &staging) {
        Ok(()) => Ok(Some(active)),
        Err(_) if is_complete_openai_bundled_marketplace(&staging) => {
            // Windows can keep the active marketplace directory pinned while
            // Codex extension hosts are still alive. Pointing config at the
            // complete staging marketplace still restores plugin discovery.
            Ok(Some(staging))
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to replace active bundled marketplace at {}",
                active.display()
            )
        }),
    }
}

#[cfg(windows)]
fn configured_openai_bundled_marketplace(home: &Path) -> Option<PathBuf> {
    let config = std::fs::read_to_string(home.join("config.toml")).ok()?;
    let without_bom = config.trim_start_matches('\u{feff}');
    let doc = parse_toml_document(without_bom).ok()?;
    let source = doc
        .get("marketplaces")?
        .as_table()?
        .get(BUNDLED_MARKETPLACE)?
        .as_table()?
        .get("source")?
        .as_str()?;
    Some(path_from_configured_marketplace_source(source))
}

#[cfg(windows)]
fn path_from_configured_marketplace_source(source: &str) -> PathBuf {
    source
        .strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(source))
}

#[cfg(windows)]
fn is_complete_openai_bundled_marketplace(path: &Path) -> bool {
    if !path
        .join(".agents")
        .join("plugins")
        .join("marketplace.json")
        .is_file()
    {
        return false;
    }
    BUNDLED_MARKETPLACE_PLUGINS.iter().all(|plugin| {
        path.join("plugins")
            .join(plugin)
            .join(".codex-plugin")
            .join("plugin.json")
            .is_file()
    })
}

#[cfg(windows)]
fn find_complete_openai_bundled_marketplace(parent: &Path, active: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == active || !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(BUNDLED_MARKETPLACE) && is_complete_openai_bundled_marketplace(&path) {
            candidates.push(path);
        }
    }
    candidates.sort_by(|left, right| {
        modified_millis(right)
            .cmp(&modified_millis(left))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn cache_plugin_root(home: &Path, plugin: &str) -> PathBuf {
    home.join("plugins")
        .join("cache")
        .join(BUNDLED_MARKETPLACE)
        .join(plugin)
}

#[cfg(windows)]
fn can_build_marketplace_from_cache(home: &Path) -> bool {
    BUNDLED_MARKETPLACE_PLUGINS
        .iter()
        .all(|plugin| latest_cache_plugin_version(home, plugin).is_some())
}

#[cfg(windows)]
fn latest_cache_plugin_version(home: &Path, plugin: &str) -> Option<PathBuf> {
    let root = cache_plugin_root(home, plugin);
    let mut candidates = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.join(".codex-plugin").join("plugin.json").is_file() {
            candidates.push(path);
        }
    }
    candidates.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let right_name = right
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        right_name
            .cmp(left_name)
            .then_with(|| modified_millis(right).cmp(&modified_millis(left)))
    });
    candidates.into_iter().next()
}

#[cfg(windows)]
fn build_marketplace_from_cache(home: &Path, staging: &Path) -> anyhow::Result<()> {
    let plugins_dir = staging.join("plugins");
    std::fs::create_dir_all(staging.join(".agents").join("plugins"))?;
    std::fs::create_dir_all(&plugins_dir)?;
    std::fs::write(
        staging
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
        bundled_marketplace_json().as_bytes(),
    )?;
    for plugin in BUNDLED_MARKETPLACE_PLUGINS {
        let Some(source) = latest_cache_plugin_version(home, plugin) else {
            anyhow::bail!("missing cached {plugin} plugin for openai-bundled marketplace");
        };
        copy_dir_recursive(&source, &plugins_dir.join(plugin))?;
    }
    Ok(())
}

#[cfg(windows)]
fn bundled_marketplace_json() -> String {
    let plugins = [
        ("browser", "Engineering"),
        ("chrome", "Productivity"),
        ("computer-use", "Productivity"),
        ("latex", "Research"),
    ]
    .into_iter()
    .map(|(name, category)| {
        serde_json::json!({
            "name": name,
            "source": {
                "source": "local",
                "path": format!("./plugins/{name}")
            },
            "policy": {
                "installation": "AVAILABLE",
                "authentication": "ON_INSTALL"
            },
            "category": category
        })
    })
    .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": BUNDLED_MARKETPLACE,
        "interface": {
            "displayName": "OpenAI Bundled"
        },
        "plugins": plugins
    }))
    .expect("bundled marketplace JSON should serialize")
}

#[cfg(windows)]
fn replace_active_marketplace(active: &Path, staging: &Path) -> anyhow::Result<()> {
    if active.exists() {
        let backup = active.with_file_name(format!(
            "{BUNDLED_MARKETPLACE}.bak-guard-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        std::fs::rename(active, backup)?;
    }
    std::fs::rename(staging, active)?;
    Ok(())
}

#[cfg(windows)]
fn copy_dir_recursive(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn ensure_openai_bundled_marketplace_config(
    doc: &mut DocumentMut,
    marketplace_path: &Path,
) -> anyhow::Result<()> {
    let marketplaces = table_mut_or_insert(doc, "marketplaces")?;
    if marketplaces
        .get(BUNDLED_MARKETPLACE)
        .and_then(Item::as_table)
        .is_none()
    {
        marketplaces[BUNDLED_MARKETPLACE] = toml_edit::table();
    }
    marketplaces[BUNDLED_MARKETPLACE]["source_type"] = toml_edit::value("local");
    marketplaces[BUNDLED_MARKETPLACE]["source"] =
        toml_edit::value(windows_extended_path(marketplace_path));
    Ok(())
}

fn windows_extended_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value.starts_with(r"\\?\") {
        value.into_owned()
    } else {
        format!(r"\\?\{value}")
    }
}

fn parse_toml_document(contents: &str) -> anyhow::Result<DocumentMut> {
    if contents.trim().is_empty() {
        Ok(DocumentMut::new())
    } else {
        contents
            .parse::<DocumentMut>()
            .with_context(|| "config.toml TOML parse failed")
    }
}

fn table_mut_or_insert<'a>(doc: &'a mut DocumentMut, key: &str) -> anyhow::Result<&'a mut Table> {
    if !doc.as_table().contains_key(key) {
        doc[key] = toml_edit::table();
    }
    if doc.get(key).and_then(Item::as_table).is_none() {
        doc[key] = toml_edit::table();
    }
    doc.get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("{key} must be a TOML table"))
}

fn ensure_plugin_enabled(doc: &mut DocumentMut, plugin_id: &str) -> anyhow::Result<()> {
    let plugins = table_mut_or_insert(doc, "plugins")?;
    if !plugins.contains_key(plugin_id) {
        plugins[plugin_id] = toml_edit::table();
    }
    if plugins.get(plugin_id).and_then(Item::as_table).is_none() {
        plugins[plugin_id] = toml_edit::table();
    }
    plugins[plugin_id]["enabled"] = toml_edit::value(true);
    Ok(())
}

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

/// Kill orphaned SkyComputerUseClient processes on macOS.
///
/// Codex can leave these subprocesses behind after Computer Use sessions. The
/// next session recreates them lazily, so cleanup is limited to this exact
/// process name and leaves lightweight Node helper processes alone.
#[cfg(target_os = "macos")]
pub fn kill_orphaned_computer_use_processes() {
    let _ = std::process::Command::new("pkill")
        .arg("-x")
        .arg("SkyComputerUseClient")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_config_text_repairs_computer_use_settings() {
        let updated = guard_config_text(
            "\u{feff}notify = [\"old.exe\", \"turn-ended\"]\n\n[features]\njs_repl = false\n\n[plugins.\"computer-use@openai-bundled\"]\nenabled = false\n",
            Some(Path::new(r"C:\tools\codex-computer-use.exe")),
        )
        .unwrap();

        assert!(!updated.as_bytes().starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(updated.contains("js_repl = true"));
        assert!(updated.contains("[plugins.\"browser@openai-bundled\"]"));
        assert!(updated.contains("[plugins.\"chrome@openai-bundled\"]"));
        assert!(updated.contains("[plugins.\"computer-use@openai-bundled\"]"));
        assert!(updated.contains("enabled = true"));
        let parsed = updated.parse::<DocumentMut>().unwrap();
        let notify = parsed["notify"].as_array().unwrap();
        assert_eq!(
            notify.get(0).and_then(|value| value.as_str()),
            Some(r"C:\tools\codex-computer-use.exe")
        );
        assert_eq!(
            notify.get(1).and_then(|value| value.as_str()),
            Some("turn-ended")
        );
        assert!(!updated.contains("old.exe"));
    }

    #[test]
    fn guard_config_text_creates_missing_sections() {
        let updated = guard_config_text("model = \"gpt-5\"\n", None).unwrap();

        assert!(updated.contains("[features]"));
        assert!(updated.contains("js_repl = true"));
        for plugin_id in COMPUTER_USE_PLUGINS {
            assert!(updated.contains(&format!("[plugins.\"{plugin_id}\"]")));
        }
        assert!(!updated.contains("notify ="));
    }

    #[test]
    fn guard_config_text_writes_openai_bundled_marketplace_source() {
        let updated = guard_config_text_with_marketplace(
            "model = \"gpt-5\"\n\n[marketplaces.openai-bundled]\nsource_type = \"remote\"\nsource = \"old\"\n",
            None,
            Some(Path::new(r"C:\Users\me\.codex\.tmp\bundled-marketplaces\openai-bundled")),
        )
        .unwrap();
        let parsed = updated.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-bundled"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-bundled"]["source"].as_str(),
            Some(r"\\?\C:\Users\me\.codex\.tmp\bundled-marketplaces\openai-bundled")
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_service_resolution_prefers_configured_trusted_service() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let configured_browser = temp
            .path()
            .join("plugins")
            .join("browser")
            .join(BROWSER_SERVICE_SCRIPT);
        let oai_root = temp.path().join("runtime").join("@oai");
        let notify_exe = oai_root
            .join("sky")
            .join("bin")
            .join("windows")
            .join(COMPUTER_USE_EXE);
        let runtime_browser = oai_root
            .join("browser-desktop")
            .join("scripts")
            .join(BROWSER_SERVICE_SCRIPT);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(configured_browser.parent().unwrap()).unwrap();
        std::fs::create_dir_all(notify_exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(runtime_browser.parent().unwrap()).unwrap();
        std::fs::write(&configured_browser, "").unwrap();
        std::fs::write(&notify_exe, "").unwrap();
        std::fs::write(&runtime_browser, "").unwrap();
        let trusted_services = serde_json::json!({
            "browser": configured_browser.to_string_lossy()
        });
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[shell_environment_policy.set]\nNODE_REPL_TRUSTED_SERVICES = '{}'\n",
                trusted_services
            ),
        )
        .unwrap();

        assert_eq!(
            resolve_browser_service_script(&home, Some(&notify_exe)).as_deref(),
            Some(configured_browser.as_path())
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_service_resolution_follows_configured_runtime_rotation() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let old_browser = temp.path().join("runtime-old").join(BROWSER_SERVICE_SCRIPT);
        let new_browser = temp.path().join("runtime-new").join(BROWSER_SERVICE_SCRIPT);
        let marketplace_path = temp.path().join("openai-bundled");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(old_browser.parent().unwrap()).unwrap();
        std::fs::create_dir_all(new_browser.parent().unwrap()).unwrap();
        std::fs::create_dir(&marketplace_path).unwrap();
        std::fs::write(&old_browser, "").unwrap();
        std::fs::write(&new_browser, "").unwrap();
        let previous = GuardArtifacts {
            notify_exe: None,
            marketplace_path: Some(marketplace_path.clone()),
            sky_package_json: None,
            browser_service_script: Some(old_browser.clone()),
            runtime_exports_needed: false,
        };
        std::fs::remove_file(&old_browser).unwrap();
        let trusted_services = serde_json::json!({
            "browser": new_browser.to_string_lossy()
        });
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[shell_environment_policy.set]\nNODE_REPL_TRUSTED_SERVICES = '{}'\n",
                trusted_services
            ),
        )
        .unwrap();

        let refreshed = refresh_computer_use_guard_artifacts(&home, Some(&previous)).unwrap();
        assert_eq!(
            refreshed.browser_service_script.as_deref(),
            Some(new_browser.as_path())
        );
        assert_eq!(
            refreshed.marketplace_path.as_deref(),
            Some(marketplace_path.as_path())
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_service_resolution_ignores_invalid_configured_path() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let oai_root = temp.path().join("runtime").join("@oai");
        let notify_exe = oai_root
            .join("sky")
            .join("bin")
            .join("windows")
            .join(COMPUTER_USE_EXE);
        let runtime_browser = oai_root
            .join("browser-desktop")
            .join("scripts")
            .join(BROWSER_SERVICE_SCRIPT);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(notify_exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(runtime_browser.parent().unwrap()).unwrap();
        std::fs::write(&notify_exe, "").unwrap();
        std::fs::write(&runtime_browser, "").unwrap();
        std::fs::write(
            home.join("config.toml"),
            "[shell_environment_policy.set]\nNODE_REPL_TRUSTED_SERVICES = '{\"browser\":\"relative/browser-service.mjs\"}'\n",
        )
        .unwrap();

        assert_eq!(
            resolve_browser_service_script(&home, Some(&notify_exe)).as_deref(),
            Some(runtime_browser.as_path())
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_reconciles_configured_trusted_service() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let browser_service_script = temp
            .path()
            .join("plugins")
            .join("browser")
            .join(BROWSER_SERVICE_SCRIPT);
        let source = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(browser_service_script.parent().unwrap()).unwrap();
        std::fs::write(&browser_service_script, source).unwrap();
        let trusted_services = serde_json::json!({
            "browser": browser_service_script.to_string_lossy()
        });
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[shell_environment_policy.set]\nNODE_REPL_TRUSTED_SERVICES = '{}'\n",
                trusted_services
            ),
        )
        .unwrap();

        let enabled = reconcile_browser_request_header_compat(&home, true).unwrap();
        assert!(enabled.changed);
        assert_eq!(
            enabled.script_path.as_deref(),
            Some(browser_service_script.as_path())
        );
        assert!(
            std::fs::read_to_string(&browser_service_script)
                .unwrap()
                .contains(BROWSER_REQUEST_HEADER_COMPAT_MARKER)
        );

        let disabled = reconcile_browser_request_header_compat(&home, false).unwrap();
        assert!(disabled.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            source
        );
    }

    #[test]
    fn add_sky_internal_computer_use_export_adds_exact_subpath() {
        let updated = add_sky_internal_computer_use_export(
            r#"{
  "name": "@oai/sky",
  "exports": {
    ".": "./dist/project/cua/sky_js/src/index.js"
  }
}"#,
        )
        .unwrap()
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&updated).unwrap();

        assert_eq!(
            parsed["exports"][SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT].as_str(),
            Some(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT)
        );
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn add_sky_internal_computer_use_export_is_idempotent() {
        let updated = add_sky_internal_computer_use_export(&format!(
            r#"{{
  "name": "@oai/sky",
  "exports": {{
    ".": "./dist/project/cua/sky_js/src/index.js",
    "{SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT}": "{SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT}"
  }}
}}"#
        ))
        .unwrap();

        assert!(updated.is_none());
    }

    #[test]
    fn add_sky_internal_computer_use_export_ignores_non_object_exports() {
        let updated =
            add_sky_internal_computer_use_export(r#"{ "name": "@oai/sky", "exports": "." }"#)
                .unwrap();

        assert!(updated.is_none());
    }

    #[test]
    fn patch_browser_request_header_policy_preserves_online_gate_with_local_fallback() {
        let source = concat!(
            "var before=1;",
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}",
            "var after=2;"
        );

        let updated = patch_browser_request_header_policy(source).unwrap();

        assert!(updated.contains(concat!(
            "async function NO(){try{if(Im==null)return!1;",
            "return await Im,$R(\"codex_browser_use_agent_request_header\")}",
            "catch{return!1}}",
            "/*codexelves-api-key-browser-compat-v1*/"
        )));
        assert!(updated.starts_with("var before=1;"));
        assert!(updated.ends_with("var after=2;"));
        assert!(!updated.contains(BROWSER_REQUEST_HEADER_IDENTITY_ERROR));
    }

    #[test]
    fn patch_browser_request_header_policy_is_idempotent() {
        let source = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        let updated = patch_browser_request_header_policy(source).unwrap();

        assert!(patch_browser_request_header_policy(&updated).is_none());
    }

    #[test]
    fn patch_browser_request_header_policy_ignores_unknown_or_ambiguous_builds() {
        let missing_feature_flag = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return false}"
        );
        let duplicate_identity_error = format!(
            "{missing_feature_flag}{missing_feature_flag}{}",
            BROWSER_REQUEST_HEADER_FEATURE_FLAG
        );
        let nested_in_different_outer_function = concat!(
            "async function Outer(){(()=>{if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")})()}"
        );

        assert!(patch_browser_request_header_policy(missing_feature_flag).is_none());
        assert!(patch_browser_request_header_policy(&duplicate_identity_error).is_none());
        assert!(patch_browser_request_header_policy(nested_in_different_outer_function).is_none());
    }

    #[test]
    fn browser_compat_metadata_accepts_only_canonical_sha256() {
        assert!(is_sha256_hex(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_sha256_hex("../0123456789abcdef"));
        assert!(!is_sha256_hex(
            "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
        ));
        assert!(!is_sha256_hex("0123456789abcdef"));
    }

    #[cfg(not(windows))]
    #[test]
    fn runtime_exports_compat_is_noop_off_windows() {
        let temp = tempfile::tempdir().unwrap();
        let result = ensure_computer_use_runtime_exports_compat(temp.path()).unwrap();

        assert!(!result.changed);
        assert!(result.package_json.is_none());
        assert!(result.backup_path.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_adds_missing_exact_export() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            format!("import {{ x }} from \"{SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT}\";\n"),
        )
        .unwrap();

        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        let internal_file = package_json
            .parent()
            .unwrap()
            .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"));
        std::fs::create_dir_all(internal_file.parent().unwrap()).unwrap();
        std::fs::write(
            &internal_file,
            "export class WindowsComputerUseClientBase {}\n",
        )
        .unwrap();
        std::fs::write(
            &package_json,
            r#"{
  "name": "@oai/sky",
  "exports": {
    ".": "./dist/project/cua/sky_js/src/index.js"
  }
}
"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(result.changed);
        assert_eq!(result.package_json.as_deref(), Some(package_json.as_path()));
        assert!(result.backup_path.as_deref().unwrap().is_file());
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&package_json).unwrap()).unwrap();
        assert_eq!(
            parsed["exports"][SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT].as_str(),
            Some(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT)
        );
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_skips_when_internal_file_missing() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            format!("import {{ x }} from \"{SKY_INTERNAL_COMPUTER_USE_CLIENT_IMPORT}\";\n"),
        )
        .unwrap();
        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        std::fs::create_dir_all(package_json.parent().unwrap()).unwrap();
        std::fs::write(
            &package_json,
            r#"{ "name": "@oai/sky", "exports": { ".": "./index.js" } }"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(!result.changed);
        assert!(
            !package_json
                .parent()
                .unwrap()
                .join(SKY_PACKAGE_EXPORTS_BACKUP)
                .exists()
        );
    }

    #[cfg(windows)]
    #[test]
    fn runtime_exports_compat_skips_when_plugin_script_no_longer_needs_patch() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let script = home
            .join("plugins")
            .join("cache")
            .join("openai-bundled")
            .join("computer-use")
            .join("26.608.12217")
            .join("scripts")
            .join(COMPUTER_USE_CLIENT_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "import { sky } from \"@oai/sky\";\n").unwrap();
        let package_json = temp.path().join("@oai").join("sky").join("package.json");
        let internal_file = package_json
            .parent()
            .unwrap()
            .join(SKY_INTERNAL_COMPUTER_USE_CLIENT_EXPORT.trim_start_matches("./"));
        std::fs::create_dir_all(internal_file.parent().unwrap()).unwrap();
        std::fs::write(
            &internal_file,
            "export class WindowsComputerUseClientBase {}\n",
        )
        .unwrap();
        std::fs::write(
            &package_json,
            r#"{ "name": "@oai/sky", "exports": { ".": "./index.js" } }"#,
        )
        .unwrap();

        let result =
            ensure_computer_use_runtime_exports_compat_windows(&home, Some(&package_json)).unwrap();

        assert!(!result.changed);
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_uses_versioned_backup_metadata_and_restores() {
        let temp = tempfile::tempdir().unwrap();
        let browser_service_script = temp.path().join(BROWSER_SERVICE_SCRIPT);
        let source = concat!(
            "var before=1;",
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}",
            "var after=2;"
        );
        std::fs::write(&browser_service_script, source).unwrap();

        let first =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();
        let second =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();

        assert!(first.changed);
        assert_eq!(
            first.script_path.as_deref(),
            Some(browser_service_script.as_path())
        );
        let backup_path = first.backup_path.as_deref().unwrap();
        assert_eq!(
            backup_path.file_name().and_then(|value| value.to_str()),
            Some(
                format!(
                    "{BROWSER_SERVICE_SCRIPT}.bak-codexelves-{}",
                    sha256_hex(source.as_bytes())
                )
                .as_str()
            )
        );
        assert_eq!(std::fs::read_to_string(backup_path).unwrap(), source);
        let patched = std::fs::read_to_string(&browser_service_script).unwrap();
        assert!(patched.contains(BROWSER_REQUEST_HEADER_COMPAT_MARKER));
        let metadata_path = first.metadata_path.as_deref().unwrap();
        let metadata: BrowserRequestHeaderCompatMetadata =
            serde_json::from_str(&std::fs::read_to_string(metadata_path).unwrap()).unwrap();
        assert_eq!(
            metadata.patch_version,
            BROWSER_REQUEST_HEADER_COMPAT_PATCH_VERSION
        );
        assert_eq!(metadata.original_sha256, sha256_hex(source.as_bytes()));
        assert_eq!(metadata.patched_sha256, sha256_hex(patched.as_bytes()));
        assert_eq!(
            metadata.backup_file_name,
            backup_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap()
        );
        assert!(!second.changed);
        assert_eq!(
            second.script_path.as_deref(),
            Some(browser_service_script.as_path())
        );
        assert_eq!(second.backup_path.as_deref(), Some(backup_path));

        let restored =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();
        let restored_again =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();

        assert!(restored.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            source
        );
        assert!(!restored_again.changed);
        assert!(backup_path.is_file());
        assert!(metadata_path.is_file());
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_creates_new_backup_after_same_path_runtime_update() {
        let temp = tempfile::tempdir().unwrap();
        let browser_service_script = temp.path().join(BROWSER_SERVICE_SCRIPT);
        let first_source = concat!(
            "var build=1;",
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        let second_source = concat!(
            "var build=2;",
            "async function NP(){if(Jm==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Jm,$S(\"codex_browser_use_agent_request_header\")}"
        );
        std::fs::write(&browser_service_script, first_source).unwrap();

        let first =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();
        std::fs::write(&browser_service_script, second_source).unwrap();
        let second =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();

        assert!(first.changed);
        assert!(second.changed);
        assert_ne!(first.backup_path, second.backup_path);
        assert_eq!(
            std::fs::read_to_string(first.backup_path.as_deref().unwrap()).unwrap(),
            first_source
        );
        assert_eq!(
            std::fs::read_to_string(second.backup_path.as_deref().unwrap()).unwrap(),
            second_source
        );

        ensure_browser_request_header_compat_windows(Some(&browser_service_script), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            second_source
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_leaves_vendor_fixed_update_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let browser_service_script = temp.path().join(BROWSER_SERVICE_SCRIPT);
        let old_source = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        let vendor_fixed_source = "async function NO(){return false}";
        std::fs::write(&browser_service_script, old_source).unwrap();
        ensure_browser_request_header_compat_windows(Some(&browser_service_script), true).unwrap();
        std::fs::write(&browser_service_script, vendor_fixed_source).unwrap();

        let enabled =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();
        let disabled =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();

        assert!(!enabled.changed);
        assert!(!disabled.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            vendor_fixed_source
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_refuses_unsafe_restore() {
        let temp = tempfile::tempdir().unwrap();
        let browser_service_script = temp.path().join(BROWSER_SERVICE_SCRIPT);
        let source = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        std::fs::write(&browser_service_script, source).unwrap();
        let enabled =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), true)
                .unwrap();
        let patched = std::fs::read_to_string(&browser_service_script).unwrap();

        let modified_patch = format!("{patched}\n// user change");
        std::fs::write(&browser_service_script, &modified_patch).unwrap();
        let current_mismatch =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();
        assert!(!current_mismatch.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            modified_patch
        );

        std::fs::write(&browser_service_script, &patched).unwrap();
        std::fs::write(enabled.backup_path.as_deref().unwrap(), "corrupt backup").unwrap();
        let backup_mismatch =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();
        assert!(!backup_mismatch.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            patched
        );
    }

    #[cfg(windows)]
    #[test]
    fn browser_request_header_compat_migrates_legacy_backup_before_restore() {
        let temp = tempfile::tempdir().unwrap();
        let browser_service_script = temp.path().join(BROWSER_SERVICE_SCRIPT);
        let legacy_backup = temp.path().join(BROWSER_SERVICE_LEGACY_BACKUP);
        let source = concat!(
            "async function NO(){if(Im==null)throw new Error(\"",
            "Browser request-header policy requires caller identity.",
            "\");return await Im,$R(\"codex_browser_use_agent_request_header\")}"
        );
        let patched = patch_browser_request_header_policy(source).unwrap();
        std::fs::write(&browser_service_script, patched).unwrap();
        std::fs::write(&legacy_backup, source).unwrap();

        let result =
            ensure_browser_request_header_compat_windows(Some(&browser_service_script), false)
                .unwrap();

        assert!(result.changed);
        assert_eq!(
            std::fs::read_to_string(&browser_service_script).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read_to_string(result.backup_path.as_deref().unwrap()).unwrap(),
            source
        );
        assert!(result.metadata_path.as_deref().unwrap().is_file());
    }

    #[cfg(windows)]
    #[test]
    fn browser_service_script_resolves_from_notify_executable_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let oai_root = temp.path().join("bin").join("node_modules").join("@oai");
        let notify_exe = oai_root
            .join("sky")
            .join("bin")
            .join("windows")
            .join(COMPUTER_USE_EXE);
        let browser_service_script = oai_root
            .join("browser-desktop")
            .join("scripts")
            .join(BROWSER_SERVICE_SCRIPT);
        std::fs::create_dir_all(notify_exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(browser_service_script.parent().unwrap()).unwrap();
        std::fs::write(&notify_exe, "").unwrap();
        std::fs::write(&browser_service_script, "").unwrap();

        assert_eq!(
            find_browser_service_script_for_notify_exe(Some(&notify_exe)).as_deref(),
            Some(browser_service_script.as_path())
        );
    }

    #[cfg(windows)]
    #[test]
    fn ensure_openai_bundled_marketplace_rebuilds_damaged_active_from_cache() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let active = home
            .join(".tmp")
            .join("bundled-marketplaces")
            .join(BUNDLED_MARKETPLACE);
        std::fs::create_dir_all(active.join("plugins").join("chrome").join(".codex-plugin"))
            .unwrap();
        std::fs::write(
            active
                .join("plugins")
                .join("chrome")
                .join(".codex-plugin")
                .join("plugin.json"),
            "{}",
        )
        .unwrap();

        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            let root = home
                .join("plugins")
                .join("cache")
                .join(BUNDLED_MARKETPLACE)
                .join(plugin)
                .join("26.608.12217");
            std::fs::create_dir_all(root.join(".codex-plugin")).unwrap();
            std::fs::write(root.join(".codex-plugin").join("plugin.json"), "{}").unwrap();
            std::fs::write(root.join("payload.txt"), plugin).unwrap();
        }

        let repaired = ensure_openai_bundled_marketplace(home).unwrap().unwrap();
        assert_eq!(repaired, active);
        assert!(
            active
                .join(".agents")
                .join("plugins")
                .join("marketplace.json")
                .is_file()
        );
        let marketplace = std::fs::read_to_string(
            active
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
        )
        .unwrap();
        assert!(marketplace.contains("\"computer-use\""));
        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            assert!(
                active
                    .join("plugins")
                    .join(plugin)
                    .join(".codex-plugin")
                    .join("plugin.json")
                    .is_file()
            );
            assert_eq!(
                std::fs::read_to_string(active.join("plugins").join(plugin).join("payload.txt"))
                    .unwrap(),
                *plugin
            );
        }
        let backup_count = std::fs::read_dir(active.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("openai-bundled.bak-guard-")
            })
            .count();
        assert_eq!(backup_count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn ensure_openai_bundled_marketplace_reuses_configured_complete_staging() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let parent = home.join(".tmp").join("bundled-marketplaces");
        let active = parent.join(BUNDLED_MARKETPLACE);
        let configured = parent.join("openai-bundled.guard-staging-existing");
        std::fs::create_dir_all(active.join("plugins")).unwrap();
        std::fs::create_dir_all(configured.join(".agents").join("plugins")).unwrap();
        std::fs::write(
            configured
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            "{}",
        )
        .unwrap();
        for plugin in BUNDLED_MARKETPLACE_PLUGINS {
            let plugin_root = configured
                .join("plugins")
                .join(plugin)
                .join(".codex-plugin");
            std::fs::create_dir_all(&plugin_root).unwrap();
            std::fs::write(plugin_root.join("plugin.json"), "{}").unwrap();
        }
        let source = format!(r"\\?\{}", configured.display());
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[marketplaces.openai-bundled]\nsource_type = \"local\"\nsource = '{}'\n",
                source
            ),
        )
        .unwrap();

        let repaired = ensure_openai_bundled_marketplace(home).unwrap().unwrap();
        assert_eq!(repaired, configured);
        let guard_staging_count = std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("openai-bundled.guard-staging-")
            })
            .count();
        assert_eq!(guard_staging_count, 1);
    }
}
