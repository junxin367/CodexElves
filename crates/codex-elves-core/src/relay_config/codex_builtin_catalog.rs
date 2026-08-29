use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

const CACHE_SCHEMA_VERSION: u32 = 1;
const CACHE_FILENAME: &str = "codex-elves-codex-bundled-model-catalog-cache.json";
const REFRESH_INTERVAL: Duration = Duration::from_secs(48 * 60 * 60);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_EXPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_VERSION_BYTES: usize = 16 * 1024;
const MAX_ERROR_CHARS: usize = 1_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedBundledCatalog {
    schema_version: u32,
    checked_at_ms: u64,
    refreshed_at_ms: Option<u64>,
    catalog: Option<Value>,
    last_error: Option<String>,
    #[serde(default)]
    source_executable: Option<String>,
    #[serde(default)]
    source_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RefreshOutcome {
    pub attempted: bool,
    pub refreshed: bool,
    pub catalog_available: bool,
    pub warning: Option<String>,
    pub source_executable: Option<String>,
    pub source_version: Option<String>,
}

#[derive(Debug, Clone)]
struct ExportedCatalog {
    catalog: Value,
    source_executable: String,
    source_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCliVersion {
    core: Vec<u64>,
    prerelease: Option<Vec<VersionIdentifier>>,
    raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionIdentifier {
    Numeric(u64),
    Text(String),
}

#[derive(Debug, Clone)]
struct CodexCandidate {
    executable: PathBuf,
    version: Option<CodexCliVersion>,
    ordinal: usize,
    path_fallback: bool,
}

pub(super) fn load_cached_catalog(home: &Path) -> Option<Value> {
    read_cache(home).catalog.filter(catalog_has_usable_prompt)
}

pub(super) async fn refresh_if_stale(
    home: &Path,
    codex_executable: Option<&Path>,
) -> anyhow::Result<RefreshOutcome> {
    let executable = codex_executable.map(Path::to_path_buf);
    refresh_if_stale_with(home, current_timestamp_ms(), move || async move {
        export_bundled_catalog(executable.as_deref()).await
    })
    .await
}

async fn refresh_if_stale_with<F, Fut>(
    home: &Path,
    now_ms: u64,
    exporter: F,
) -> anyhow::Result<RefreshOutcome>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<ExportedCatalog>>,
{
    let mut cache = read_cache(home);
    let refresh_interval_ms = REFRESH_INTERVAL.as_millis() as u64;
    if cache.checked_at_ms > 0
        && cache.checked_at_ms <= now_ms
        && now_ms.saturating_sub(cache.checked_at_ms) < refresh_interval_ms
    {
        return Ok(RefreshOutcome {
            attempted: false,
            refreshed: false,
            catalog_available: cache
                .catalog
                .as_ref()
                .is_some_and(catalog_has_usable_prompt),
            warning: cache.last_error.clone(),
            source_executable: cache.source_executable.clone(),
            source_version: cache.source_version.clone(),
        });
    }

    cache.schema_version = CACHE_SCHEMA_VERSION;
    cache.checked_at_ms = now_ms;
    match exporter().await {
        Ok(export) if catalog_has_usable_prompt(&export.catalog) => {
            cache.refreshed_at_ms = Some(now_ms);
            cache.catalog = Some(export.catalog);
            cache.last_error = None;
            cache.source_executable = Some(export.source_executable.clone());
            cache.source_version = export.source_version.clone();
            write_cache(home, &cache)?;
            Ok(RefreshOutcome {
                attempted: true,
                refreshed: true,
                catalog_available: true,
                warning: None,
                source_executable: Some(export.source_executable),
                source_version: export.source_version,
            })
        }
        Ok(_) => {
            let warning = "Codex 导出的内置模型目录没有可用系统提示词".to_string();
            cache.last_error = Some(warning.clone());
            let catalog_available = cache
                .catalog
                .as_ref()
                .is_some_and(catalog_has_usable_prompt);
            write_cache(home, &cache)?;
            Ok(RefreshOutcome {
                attempted: true,
                refreshed: false,
                catalog_available,
                warning: Some(warning),
                source_executable: cache.source_executable.clone(),
                source_version: cache.source_version.clone(),
            })
        }
        Err(error) => {
            let warning = truncate_error(error.to_string());
            cache.last_error = Some(warning.clone());
            let catalog_available = cache
                .catalog
                .as_ref()
                .is_some_and(catalog_has_usable_prompt);
            write_cache(home, &cache)?;
            Ok(RefreshOutcome {
                attempted: true,
                refreshed: false,
                catalog_available,
                warning: Some(warning),
                source_executable: cache.source_executable.clone(),
                source_version: cache.source_version.clone(),
            })
        }
    }
}

async fn export_bundled_catalog(
    codex_executable: Option<&Path>,
) -> anyhow::Result<ExportedCatalog> {
    let candidates = codex_executable_candidates(codex_executable);
    let probes = candidates
        .into_iter()
        .enumerate()
        .map(|(ordinal, executable)| async move {
            let version = probe_codex_cli_version(&executable).await;
            let path_fallback = executable == Path::new("codex");
            CodexCandidate {
                executable,
                version,
                ordinal,
                path_fallback,
            }
        });
    let candidates = futures_util::future::join_all(probes).await;
    let mut candidates = current_codex_candidates(candidates);
    candidates.sort_by(compare_codex_candidates);

    let mut errors = Vec::new();
    for candidate in candidates {
        match export_bundled_catalog_from(&candidate.executable).await {
            Ok(catalog) if catalog_has_usable_prompt(&catalog) => {
                return Ok(ExportedCatalog {
                    catalog,
                    source_executable: candidate.executable.to_string_lossy().to_string(),
                    source_version: candidate.version.map(|version| version.raw),
                });
            }
            Ok(_) => errors.push(format!(
                "{}: 导出的目录没有可用系统提示词",
                candidate.executable.display()
            )),
            Err(error) => errors.push(format!("{}: {error}", candidate.executable.display())),
        }
    }
    anyhow::bail!("无法从当前 Codex 导出内置模型目录：{}", errors.join("；"))
}

fn codex_executable_candidates(preferred: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(executable) = preferred {
        candidates.push(executable.to_path_buf());
    }
    #[cfg(windows)]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        candidates.extend(codex_runtime_candidates_from_bin(
            &local_app_data.join("OpenAI").join("Codex").join("bin"),
        ));
    }
    candidates.push(PathBuf::from("codex"));

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate_key(candidate)));
    candidates
}

fn codex_runtime_candidates_from_bin(bin: &Path) -> Vec<PathBuf> {
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

fn candidate_key(path: &Path) -> String {
    let key = path.to_string_lossy().to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn file_modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

async fn probe_codex_cli_version(executable: &Path) -> Option<CodexCliVersion> {
    let mut command = Command::new(executable);
    command
        .arg("--version")
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(VERSION_PROBE_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
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

fn parse_codex_cli_version(output: &str) -> Option<CodexCliVersion> {
    let raw = output.split_whitespace().find(|token| {
        token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
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

fn compare_codex_candidates(left: &CodexCandidate, right: &CodexCandidate) -> Ordering {
    let source_order = left.path_fallback.cmp(&right.path_fallback);
    if source_order != Ordering::Equal {
        return source_order;
    }
    match (&left.version, &right.version) {
        (Some(left_version), Some(right_version)) => {
            compare_codex_versions(right_version, left_version)
                .then_with(|| left_version.raw.cmp(&right_version.raw))
                .then_with(|| left.ordinal.cmp(&right.ordinal))
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.ordinal.cmp(&right.ordinal),
    }
}

fn current_codex_candidates(mut candidates: Vec<CodexCandidate>) -> Vec<CodexCandidate> {
    let newest_managed_version = candidates
        .iter()
        .filter(|candidate| !candidate.path_fallback)
        .filter_map(|candidate| candidate.version.as_ref())
        .cloned()
        .max_by(compare_codex_versions);
    let Some(newest_managed_version) = newest_managed_version else {
        return candidates;
    };

    candidates.retain(|candidate| {
        candidate.version.as_ref().is_none_or(|version| {
            compare_codex_versions(version, &newest_managed_version) == Ordering::Equal
        })
    });
    candidates
}

fn compare_codex_versions(left: &CodexCliVersion, right: &CodexCliVersion) -> Ordering {
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

async fn export_bundled_catalog_from(executable: &Path) -> anyhow::Result<Value> {
    let mut command = Command::new(executable);
    command
        .args(["debug", "models", "--bundled"])
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(EXPORT_TIMEOUT, command.output())
        .await
        .with_context(|| {
            format!(
                "{} 导出内置模型目录超过 {} 秒",
                executable.display(),
                EXPORT_TIMEOUT.as_secs()
            )
        })?
        .with_context(|| format!("无法执行 {}", executable.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("命令退出状态 {}：{}", output.status, stderr.trim());
    }
    if output.stdout.len() > MAX_EXPORT_BYTES {
        anyhow::bail!("Codex 内置模型目录输出超过 {} 字节限制", MAX_EXPORT_BYTES);
    }
    let catalog: Value =
        serde_json::from_slice(&output.stdout).context("Codex 内置模型目录不是有效 JSON")?;
    Ok(catalog)
}

fn read_cache(home: &Path) -> CachedBundledCatalog {
    let path = home.join(CACHE_FILENAME);
    let Ok(contents) = std::fs::read(&path) else {
        return CachedBundledCatalog::default();
    };
    let cache = serde_json::from_slice::<CachedBundledCatalog>(&contents).unwrap_or_default();
    if cache.schema_version == CACHE_SCHEMA_VERSION {
        cache
    } else {
        CachedBundledCatalog::default()
    }
}

fn write_cache(home: &Path, cache: &CachedBundledCatalog) -> anyhow::Result<()> {
    let path = home.join(CACHE_FILENAME);
    let bytes = serde_json::to_vec_pretty(cache)?;
    crate::settings::atomic_write(&path, &bytes).context("写入 Codex 内置模型目录缓存失败")
}

fn catalog_has_usable_prompt(catalog: &Value) -> bool {
    catalog
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                model
                    .get("slug")
                    .and_then(Value::as_str)
                    .is_some_and(|slug| !slug.trim().is_empty())
                    && model
                        .get("base_instructions")
                        .and_then(Value::as_str)
                        .is_some_and(|prompt| !prompt.trim().is_empty())
            })
        })
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn truncate_error(error: String) -> String {
    error.chars().take(MAX_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    fn test_catalog(prompt: &str) -> Value {
        json!({
            "models": [{
                "slug": "gpt-test",
                "base_instructions": prompt,
                "model_messages": {
                    "instructions_template": prompt,
                    "instructions_variables": {}
                }
            }]
        })
    }

    fn test_export(prompt: &str) -> ExportedCatalog {
        ExportedCatalog {
            catalog: test_catalog(prompt),
            source_executable: "codex-test".to_string(),
            source_version: Some("0.150.0".to_string()),
        }
    }

    #[tokio::test]
    async fn refreshes_only_after_48_hours() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&calls);
        let first = refresh_if_stale_with(temp.path(), 1_000, move || async move {
            first_calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(test_export("first"))
        })
        .await
        .unwrap();
        assert!(first.attempted);
        assert!(first.refreshed);
        assert_eq!(first.source_version.as_deref(), Some("0.150.0"));

        let fresh_calls = Arc::clone(&calls);
        let fresh = refresh_if_stale_with(
            temp.path(),
            1_000 + REFRESH_INTERVAL.as_millis() as u64 - 1,
            move || async move {
                fresh_calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(test_export("unexpected"))
            },
        )
        .await
        .unwrap();
        assert!(!fresh.attempted);
        assert!(!fresh.refreshed);

        let stale_calls = Arc::clone(&calls);
        let stale = refresh_if_stale_with(
            temp.path(),
            1_000 + REFRESH_INTERVAL.as_millis() as u64,
            move || async move {
                stale_calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(test_export("second"))
            },
        )
        .await
        .unwrap();
        assert!(stale.attempted);
        assert!(stale.refreshed);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(
            load_cached_catalog(temp.path()).unwrap()["models"][0]["base_instructions"],
            "second"
        );
    }

    #[tokio::test]
    async fn failed_refresh_is_throttled_and_keeps_last_successful_catalog() {
        let temp = tempfile::tempdir().unwrap();
        refresh_if_stale_with(temp.path(), 1_000, || async {
            Ok(test_export("last good"))
        })
        .await
        .unwrap();

        let stale_at = 1_000 + REFRESH_INTERVAL.as_millis() as u64;
        let failed = refresh_if_stale_with(temp.path(), stale_at, || async {
            anyhow::bail!("temporary failure")
        })
        .await
        .unwrap();
        assert!(failed.attempted);
        assert!(!failed.refreshed);
        assert!(failed.catalog_available);
        assert!(failed.warning.unwrap().contains("temporary failure"));

        let calls = Arc::new(AtomicUsize::new(0));
        let retry_calls = Arc::clone(&calls);
        let throttled = refresh_if_stale_with(temp.path(), stale_at + 1, move || async move {
            retry_calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(test_export("should not run"))
        })
        .await
        .unwrap();
        assert!(!throttled.attempted);
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            load_cached_catalog(temp.path()).unwrap()["models"][0]["base_instructions"],
            "last good"
        );
    }

    #[tokio::test]
    async fn future_checked_timestamp_does_not_suppress_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let cache = CachedBundledCatalog {
            schema_version: CACHE_SCHEMA_VERSION,
            checked_at_ms: 10_000,
            refreshed_at_ms: Some(10_000),
            catalog: Some(test_catalog("future")),
            last_error: None,
            source_executable: Some("future-codex".to_string()),
            source_version: Some("0.999.0".to_string()),
        };
        write_cache(temp.path(), &cache).unwrap();

        let outcome =
            refresh_if_stale_with(temp.path(), 5_000, || async { Ok(test_export("current")) })
                .await
                .unwrap();
        assert!(outcome.attempted);
        assert!(outcome.refreshed);
        assert_eq!(
            load_cached_catalog(temp.path()).unwrap()["models"][0]["base_instructions"],
            "current"
        );
    }

    #[test]
    fn parses_and_orders_codex_cli_versions() {
        let old = parse_codex_cli_version("codex-cli 0.130.0-alpha.5").unwrap();
        let stable = parse_codex_cli_version("codex-cli 0.147.0").unwrap();
        let current = parse_codex_cli_version("codex-cli 0.150.0-alpha.12.2").unwrap();
        assert_eq!(compare_codex_versions(&stable, &old), Ordering::Greater);
        assert_eq!(compare_codex_versions(&current, &stable), Ordering::Greater);

        let alpha_9 = parse_codex_cli_version("codex-cli 0.150.0-alpha.9").unwrap();
        assert_eq!(
            compare_codex_versions(&current, &alpha_9),
            Ordering::Greater
        );

        let final_release = parse_codex_cli_version("codex-cli 0.150.0").unwrap();
        assert_eq!(
            compare_codex_versions(&final_release, &current),
            Ordering::Greater
        );
    }

    #[test]
    fn sorts_newest_managed_runtime_before_older_and_path_fallback() {
        let mut candidates = vec![
            CodexCandidate {
                executable: PathBuf::from("managed-old"),
                version: parse_codex_cli_version("codex-cli 0.130.0-alpha.5"),
                ordinal: 0,
                path_fallback: false,
            },
            CodexCandidate {
                executable: PathBuf::from("codex"),
                version: parse_codex_cli_version("codex-cli 0.999.0"),
                ordinal: 2,
                path_fallback: true,
            },
            CodexCandidate {
                executable: PathBuf::from("managed-current"),
                version: parse_codex_cli_version("codex-cli 0.150.0-alpha.12.2"),
                ordinal: 1,
                path_fallback: false,
            },
        ];
        candidates.sort_by(compare_codex_candidates);

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.executable.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
            vec!["managed-current", "managed-old", "codex"]
        );
    }

    #[test]
    fn current_candidates_exclude_known_old_managed_and_unrelated_path_versions() {
        let candidates = vec![
            CodexCandidate {
                executable: PathBuf::from("managed-old"),
                version: parse_codex_cli_version("codex-cli 0.130.0-alpha.5"),
                ordinal: 0,
                path_fallback: false,
            },
            CodexCandidate {
                executable: PathBuf::from("managed-current"),
                version: parse_codex_cli_version("codex-cli 0.150.0-alpha.12.2"),
                ordinal: 1,
                path_fallback: false,
            },
            CodexCandidate {
                executable: PathBuf::from("codex"),
                version: parse_codex_cli_version("codex-cli 0.999.0"),
                ordinal: 2,
                path_fallback: true,
            },
        ];

        let current = current_codex_candidates(candidates);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].executable, PathBuf::from("managed-current"));
    }

    #[test]
    fn discovers_root_and_nested_codex_runtimes() {
        let temp = tempfile::tempdir().unwrap();
        let executable_name = if cfg!(windows) { "codex.exe" } else { "codex" };
        std::fs::write(temp.path().join(executable_name), b"root").unwrap();
        let nested = temp.path().join("runtime-new");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join(executable_name), b"nested").unwrap();

        let candidates = codex_runtime_candidates_from_bin(temp.path());
        assert!(candidates.contains(&temp.path().join(executable_name)));
        assert!(candidates.contains(&nested.join(executable_name)));
    }
}
