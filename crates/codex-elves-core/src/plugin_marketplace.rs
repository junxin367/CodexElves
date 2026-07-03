use std::cmp::Ordering;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table};

const OPENAI_CURATED_MARKETPLACE: &str = "openai-curated";
const OPENAI_API_CURATED_MARKETPLACE: &str = "openai-api-curated";
const OPENAI_CURATED_REMOTE_MARKETPLACE: &str = "openai-curated-remote";
const OPENAI_PLUGINS_ZIP_URL: &str =
    "https://codeload.github.com/openai/plugins/zip/refs/heads/main";
const OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const OPENAI_CURATED_REMOTE_MARKETPLACE_ZIP: &[u8] =
    include_bytes!("../../../assets/plugin-marketplaces/openai-curated-remote.zip");

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCacheInfo {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub cached: bool,
    pub cached_versions: Vec<String>,
    pub current_version: Option<String>,
    pub source_version: Option<String>,
    pub cache_path: Option<String>,
    pub source_path: Option<String>,
    pub can_refresh: bool,
    pub refresh_reason: String,
}

struct LocalPluginSource {
    root: PathBuf,
    version: Option<String>,
}

pub fn list_plugin_cache_infos(home: &Path, plugin_ids: &[String]) -> Vec<PluginCacheInfo> {
    plugin_ids
        .iter()
        .map(|plugin_id| plugin_cache_info(home, plugin_id))
        .collect()
}

pub fn plugin_cache_info(home: &Path, plugin_id: &str) -> PluginCacheInfo {
    let Some((name, marketplace)) = split_plugin_id(plugin_id) else {
        return PluginCacheInfo {
            id: plugin_id.to_string(),
            name: plugin_id.to_string(),
            marketplace: String::new(),
            cached: false,
            cached_versions: Vec::new(),
            current_version: None,
            source_version: None,
            cache_path: None,
            source_path: None,
            can_refresh: false,
            refresh_reason: "插件 ID 需要形如 name@marketplace。".to_string(),
        };
    };
    let cache_root = plugin_cache_root(home, &marketplace, &name);
    let cached_versions = cached_plugin_versions(&cache_root);
    let current_version = cached_versions.last().cloned();
    let cache_path = current_version
        .as_ref()
        .map(|version| cache_root.join(version).to_string_lossy().to_string());
    let source = local_plugin_source(home, &marketplace, &name)
        .ok()
        .flatten();
    let source_version = source.as_ref().and_then(|source| source.version.clone());
    let source_path = source
        .as_ref()
        .map(|source| source.root.to_string_lossy().to_string());
    let (can_refresh, refresh_reason) = match (&source, &source_version) {
        (Some(_), Some(source_version)) => {
            plugin_refresh_state(current_version.as_deref(), source_version)
        }
        (Some(_), None) => (
            false,
            "本地 source 缺少 .codex-plugin/plugin.json 或 version。".to_string(),
        ),
        (None, _) => (
            false,
            "未找到本地 marketplace source，不能直接强制刷新。".to_string(),
        ),
    };
    PluginCacheInfo {
        id: plugin_id.to_string(),
        name,
        marketplace,
        cached: !cached_versions.is_empty(),
        cached_versions,
        current_version,
        source_version,
        cache_path,
        source_path,
        can_refresh,
        refresh_reason,
    }
}

pub fn force_refresh_plugin_cache(home: &Path, plugin_id: &str) -> anyhow::Result<PluginCacheInfo> {
    let (name, marketplace) = split_plugin_id(plugin_id)
        .ok_or_else(|| anyhow::anyhow!("插件 ID 需要形如 name@marketplace"))?;
    let source = local_plugin_source(home, &marketplace, &name)?
        .ok_or_else(|| anyhow::anyhow!("未找到本地 marketplace source，不能直接强制刷新"))?;
    let version = source
        .version
        .clone()
        .ok_or_else(|| anyhow::anyhow!("本地 source 缺少 .codex-plugin/plugin.json 或 version"))?;
    let cache_root = plugin_cache_root(home, &marketplace, &name);
    let current_version = cached_plugin_versions(&cache_root).last().cloned();
    if let Some(current_version) = current_version.as_deref()
        && compare_plugin_versions(&version, current_version) == Ordering::Less
    {
        anyhow::bail!(
            "源版本 {version} 低于当前缓存版本 {current_version}，强制刷新会降级，已阻止"
        );
    }
    std::fs::create_dir_all(&cache_root)
        .with_context(|| format!("failed to create {}", cache_root.display()))?;
    let staging = cache_root.join(format!(
        ".refresh-{}-{}",
        sanitize_path_segment(&version),
        timestamp_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to remove stale {}", staging.display()))?;
    }
    copy_dir_recursive(&source.root, &staging)?;
    let copied_version =
        plugin_manifest_version(&staging.join(".codex-plugin").join("plugin.json"))
            .ok_or_else(|| anyhow::anyhow!("刷新后的插件缓存缺少 plugin.json version"))?;
    if copied_version != version {
        let _ = std::fs::remove_dir_all(&staging);
        anyhow::bail!("刷新后的插件版本不一致：source={version}, copied={copied_version}");
    }
    let destination = cache_root.join(&version);
    let result = replace_plugin_cache_directory(&staging, &destination);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    Ok(plugin_cache_info(home, plugin_id))
}

pub fn ensure_openai_curated_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_marketplace_root(home)? else {
        return Ok(false);
    };
    let mut changed = ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_MARKETPLACE, OPENAI_API_CURATED_MARKETPLACE],
        &marketplace_root,
    )?;
    if let Some(remote_marketplace_root) = local_openai_curated_remote_marketplace_root(home)? {
        changed |= ensure_marketplace_configs(
            home,
            &[OPENAI_CURATED_REMOTE_MARKETPLACE],
            &remote_marketplace_root,
        )?;
    }
    Ok(changed)
}

pub fn ensure_openai_curated_remote_marketplace_config(home: &Path) -> anyhow::Result<bool> {
    let Some(marketplace_root) = local_openai_curated_remote_marketplace_root(home)? else {
        return Ok(false);
    };
    ensure_marketplace_configs(
        home,
        &[OPENAI_CURATED_REMOTE_MARKETPLACE],
        &marketplace_root,
    )
}

pub fn ensure_openai_curated_remote_marketplace_available(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let mut initialized = false;
    if local_openai_curated_remote_marketplace_root(home)?.is_none() {
        install_openai_curated_remote_marketplace_zip(home, OPENAI_CURATED_REMOTE_MARKETPLACE_ZIP)?;
        initialized = true;
    }
    let configured = ensure_openai_curated_remote_marketplace_config(home)?;
    Ok(MarketplaceEnsureResult {
        initialized,
        configured,
    })
}

pub fn openai_curated_marketplace_status(home: &Path) -> MarketplaceStatus {
    let marketplace_root = local_openai_curated_marketplace_root(home).ok().flatten();
    let config_registered = marketplace_root
        .as_deref()
        .map(|root| {
            marketplace_config_points_to_root(home, OPENAI_CURATED_MARKETPLACE, root)
                && marketplace_config_points_to_root(home, OPENAI_API_CURATED_MARKETPLACE, root)
        })
        .unwrap_or(false);
    MarketplaceStatus {
        marketplace_root,
        config_registered,
    }
}

pub fn openai_curated_remote_marketplace_status(home: &Path) -> MarketplaceStatus {
    let marketplace_root = local_openai_curated_remote_marketplace_root(home)
        .ok()
        .flatten();
    let config_registered = marketplace_root
        .as_deref()
        .map(|root| {
            marketplace_config_points_to_root(home, OPENAI_CURATED_REMOTE_MARKETPLACE, root)
        })
        .unwrap_or(false);
    MarketplaceStatus {
        marketplace_root,
        config_registered,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplaceStatus {
    pub marketplace_root: Option<PathBuf>,
    pub config_registered: bool,
}

impl MarketplaceStatus {
    pub fn needs_repair(&self) -> bool {
        self.marketplace_root.is_none() || !self.config_registered
    }
}

pub async fn initialize_openai_curated_marketplace_and_configure(
    home: &Path,
) -> anyhow::Result<MarketplaceEnsureResult> {
    let mut initialized = false;
    if local_openai_curated_marketplace_root(home)?.is_none() {
        initialize_openai_curated_marketplace_from_github(home).await?;
        initialized = true;
    }
    let configured = ensure_openai_curated_marketplace_config(home)?;
    Ok(MarketplaceEnsureResult {
        initialized,
        configured,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketplaceEnsureResult {
    pub initialized: bool,
    pub configured: bool,
}

fn local_openai_curated_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins");
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root))
}

fn local_openai_curated_remote_marketplace_root(home: &Path) -> anyhow::Result<Option<PathBuf>> {
    let root = home.join(".tmp").join("plugins-remote");
    local_openai_curated_remote_marketplace_root_from_root(&root)
}

async fn initialize_openai_curated_marketplace_from_github(home: &Path) -> anyhow::Result<()> {
    let bytes = download_openai_plugins_zip().await?;
    install_openai_plugins_zip(home, &bytes)
}

async fn download_openai_plugins_zip() -> anyhow::Result<Vec<u8>> {
    let client =
        crate::http_client::proxied_client(&format!("CodexElves/{}", crate::version::VERSION))?;
    let bytes = client
        .get(OPENAI_PLUGINS_ZIP_URL)
        .header(reqwest::header::ACCEPT, "application/zip")
        .send()
        .await
        .context("failed to download openai/plugins marketplace")?
        .error_for_status()
        .context("openai/plugins marketplace download returned an error status")?
        .bytes()
        .await
        .context("failed to read openai/plugins marketplace download body")?;
    if bytes.len() > OPENAI_PLUGINS_DOWNLOAD_LIMIT_BYTES {
        anyhow::bail!(
            "openai/plugins marketplace download is too large: {} bytes",
            bytes.len()
        );
    }
    Ok(bytes.to_vec())
}

fn install_openai_plugins_zip(home: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let destination = home.join(".tmp").join("plugins");
    let staging_parent = home.join(".tmp");
    std::fs::create_dir_all(&staging_parent)
        .with_context(|| format!("failed to create {}", staging_parent.display()))?;
    let staging = staging_parent.join(format!(
        "plugins-download-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to remove stale {}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("failed to create {}", staging.display()))?;

    let result = extract_openai_plugins_zip(bytes, &staging)
        .and_then(|_| validate_openai_plugins_marketplace_root(&staging))
        .and_then(|_| replace_directory(&staging, &destination));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn install_openai_curated_remote_marketplace_zip(home: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let destination = home.join(".tmp").join("plugins-remote");
    let staging_parent = home.join(".tmp");
    std::fs::create_dir_all(&staging_parent)
        .with_context(|| format!("failed to create {}", staging_parent.display()))?;
    let staging = staging_parent.join(format!(
        "plugins-remote-embedded-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to remove stale {}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("failed to create {}", staging.display()))?;

    let result = extract_zip_exact(bytes, &staging)
        .and_then(|_| validate_openai_curated_remote_marketplace_root(&staging))
        .and_then(|_| {
            replace_directory_with_backup_name(
                &staging,
                &destination,
                "plugins-remote.previous-codex-elves",
            )
        });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn extract_openai_plugins_zip(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to read openai/plugins zip")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;
        let Some(relative_path) = zip_entry_relative_path(file.name()) else {
            continue;
        };
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .with_context(|| format!("failed to read zip entry {}", file.name()))?;
        std::fs::write(&output_path, contents)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
    }
    Ok(())
}

fn extract_zip_exact(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to read embedded plugin zip")?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry {index}"))?;
        let relative_path = safe_zip_path(file.name())?;
        let output_path = destination.join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .with_context(|| format!("failed to read zip entry {}", file.name()))?;
        std::fs::write(&output_path, contents)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
    }
    Ok(())
}

fn safe_zip_path(name: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(name);
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => anyhow::bail!("zip entry escapes destination: {name}"),
        }
    }
    if relative.as_os_str().is_empty() {
        anyhow::bail!("zip entry has empty path");
    }
    Ok(relative)
}

fn zip_entry_relative_path(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut components = path.components();
    match components.next()? {
        Component::Normal(_) => {}
        _ => return None,
    }
    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn validate_openai_plugins_marketplace_root(root: &Path) -> anyhow::Result<()> {
    let marketplace = local_openai_curated_marketplace_root_from_root(root)?
        .ok_or_else(|| anyhow::anyhow!("downloaded openai/plugins marketplace is invalid"))?;
    if marketplace != root {
        anyhow::bail!("downloaded openai/plugins marketplace root mismatch");
    }
    Ok(())
}

fn validate_openai_curated_remote_marketplace_root(root: &Path) -> anyhow::Result<()> {
    let marketplace = local_openai_curated_remote_marketplace_root_from_root(root)?
        .ok_or_else(|| anyhow::anyhow!("embedded official remote plugin marketplace is invalid"))?;
    if marketplace != root {
        anyhow::bail!("embedded official remote plugin marketplace root mismatch");
    }
    Ok(())
}

fn local_openai_curated_marketplace_root_from_root(root: &Path) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn local_openai_curated_remote_marketplace_root_from_root(
    root: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    let marketplace_path = root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    if !marketplace_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marketplace_path)
        .with_context(|| format!("failed to read {}", marketplace_path.display()))?;
    let marketplace: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    if marketplace.get("name").and_then(serde_json::Value::as_str)
        != Some(OPENAI_CURATED_REMOTE_MARKETPLACE)
    {
        return Ok(None);
    }
    let has_plugins = marketplace
        .get("plugins")
        .and_then(serde_json::Value::as_array)
        .map(|plugins| !plugins.is_empty())
        .unwrap_or(false);
    if !has_plugins || !root.join("plugins").is_dir() {
        return Ok(None);
    }
    Ok(Some(root.to_path_buf()))
}

fn replace_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    replace_directory_with_backup_name(source, destination, "plugins.previous-codex-elves")
}

fn replace_directory_with_backup_name(
    source: &Path,
    destination: &Path,
    backup_name: &str,
) -> anyhow::Result<()> {
    let backup = destination.with_file_name(backup_name);
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove {}", backup.display()))?;
    }
    if destination.exists() {
        std::fs::rename(destination, &backup).with_context(|| {
            format!(
                "failed to move {} to {}",
                destination.display(),
                backup.display()
            )
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => {
            if backup.exists() {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, destination);
            }
            Err(error).with_context(|| {
                format!(
                    "failed to move {} to {}",
                    source.display(),
                    destination.display()
                )
            })
        }
    }
}

fn replace_plugin_cache_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let backup = destination.with_file_name(format!(
        "{}.previous-codex-elves",
        destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("plugin-cache")
    ));
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .with_context(|| format!("failed to remove {}", backup.display()))?;
    }
    if destination.exists() {
        std::fs::rename(destination, &backup).with_context(|| {
            format!(
                "failed to move {} to {}",
                destination.display(),
                backup.display()
            )
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => {
            if backup.exists() {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, destination);
            }
            Err(error).with_context(|| {
                format!(
                    "failed to move {} to {}",
                    source.display(),
                    destination.display()
                )
            })
        }
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    for entry in
        std::fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn ensure_marketplace_configs(
    home: &Path,
    marketplace_names: &[&str],
    marketplace_root: &Path,
) -> anyhow::Result<bool> {
    let config_path = home.join("config.toml");
    let existing = match std::fs::read(&config_path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("failed to read UTF-8 {}", config_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let without_bom = existing.trim_start_matches('\u{feff}');
    let mut doc = parse_toml_document(without_bom)?;
    let marketplaces = table_mut_or_insert(&mut doc, "marketplaces")?;
    for marketplace_name in marketplace_names {
        if marketplaces
            .get(marketplace_name)
            .and_then(Item::as_table)
            .is_none()
        {
            marketplaces[marketplace_name] = toml_edit::table();
        }
        marketplaces[marketplace_name]["source_type"] = toml_edit::value("local");
        marketplaces[marketplace_name]["source"] =
            toml_edit::value(windows_extended_path(marketplace_root));
    }

    let updated = ensure_trailing_newline(doc.to_string());
    if updated.as_bytes() == without_bom.as_bytes() {
        return Ok(false);
    }
    crate::settings::atomic_write(&config_path, updated.as_bytes())?;
    Ok(true)
}

fn marketplace_config_points_to_root(home: &Path, marketplace_name: &str, root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(home.join("config.toml")) else {
        return false;
    };
    let Ok(doc) = text.trim_start_matches('\u{feff}').parse::<DocumentMut>() else {
        return false;
    };
    let Some(table) = doc
        .get("marketplaces")
        .and_then(Item::as_table)
        .and_then(|marketplaces| marketplaces.get(marketplace_name))
        .and_then(Item::as_table)
    else {
        return false;
    };
    let source_type = table
        .get("source_type")
        .and_then(Item::as_str)
        .unwrap_or_default();
    let source = table
        .get("source")
        .and_then(Item::as_str)
        .unwrap_or_default();
    source_type == "local" && normalize_windows_extended_path(source) == root.to_string_lossy()
}

fn split_plugin_id(plugin_id: &str) -> Option<(String, String)> {
    let (name, marketplace) = plugin_id.trim().rsplit_once('@')?;
    let name = name.trim();
    let marketplace = marketplace.trim();
    if name.is_empty() || marketplace.is_empty() {
        return None;
    }
    Some((name.to_string(), marketplace.to_string()))
}

fn plugin_cache_root(home: &Path, marketplace: &str, name: &str) -> PathBuf {
    home.join("plugins")
        .join("cache")
        .join(marketplace)
        .join(name)
}

fn cached_plugin_versions(cache_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return Vec::new();
    };
    let mut versions = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let dir_version = entry.file_name().to_string_lossy().to_string();
            let version = plugin_manifest_version(&path.join(".codex-plugin").join("plugin.json"))
                .unwrap_or(dir_version);
            Some(version)
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| compare_plugin_versions(left, right));
    versions.dedup();
    versions
}

fn plugin_refresh_state(current_version: Option<&str>, source_version: &str) -> (bool, String) {
    let Some(current_version) = current_version else {
        return (
            true,
            "未缓存，可从本地 marketplace source 生成缓存。".to_string(),
        );
    };
    match compare_plugin_versions(source_version, current_version) {
        Ordering::Less => (
            false,
            "源版本低于缓存版本，强制刷新会降级，已阻止。".to_string(),
        ),
        Ordering::Equal => (true, "可从本地 marketplace source 重建缓存。".to_string()),
        Ordering::Greater => (true, "源版本更高，可强制刷新缓存。".to_string()),
    }
}

fn compare_plugin_versions(left: &str, right: &str) -> Ordering {
    let (left_core, left_pre) = split_plugin_version(left);
    let (right_core, right_pre) = split_plugin_version(right);
    let core_order = compare_version_core(left_core, right_core);
    if core_order != Ordering::Equal {
        return core_order;
    }
    match (left_pre, right_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left_pre), Some(right_pre)) => compare_prerelease(left_pre, right_pre),
    }
}

fn split_plugin_version(version: &str) -> (&str, Option<&str>) {
    version
        .split_once('-')
        .map(|(core, prerelease)| (core, Some(prerelease)))
        .unwrap_or((version, None))
}

fn compare_version_core(left: &str, right: &str) -> Ordering {
    let left_parts = left.split('.').collect::<Vec<_>>();
    let right_parts = right.split('.').collect::<Vec<_>>();
    let max_len = left_parts.len().max(right_parts.len());
    for index in 0..max_len {
        let left_part = left_parts.get(index).copied().unwrap_or("0");
        let right_part = right_parts.get(index).copied().unwrap_or("0");
        let order = compare_version_identifier(left_part, right_part);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let left_parts = left.split('.').collect::<Vec<_>>();
    let right_parts = right.split('.').collect::<Vec<_>>();
    let max_len = left_parts.len().max(right_parts.len());
    for index in 0..max_len {
        let Some(left_part) = left_parts.get(index).copied() else {
            return Ordering::Less;
        };
        let Some(right_part) = right_parts.get(index).copied() else {
            return Ordering::Greater;
        };
        let order = compare_version_identifier(left_part, right_part);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn compare_version_identifier(left: &str, right: &str) -> Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

fn local_plugin_source(
    home: &Path,
    marketplace: &str,
    name: &str,
) -> anyhow::Result<Option<LocalPluginSource>> {
    let Some(marketplace_root) = marketplace_source_path_from_config(home, marketplace)? else {
        return Ok(None);
    };
    let marketplace_path = marketplace_root
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    let text = match std::fs::read_to_string(&marketplace_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", marketplace_path.display()));
        }
    };
    let marketplace_json: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", marketplace_path.display()))?;
    let plugin = marketplace_json
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some(name))
        });
    let plugin_root = plugin
        .and_then(plugin_source_relative_path)
        .map(|path| resolve_marketplace_path(&marketplace_root, &path))
        .unwrap_or_else(|| marketplace_root.join("plugins").join(name));
    if !plugin_root.is_dir() {
        return Ok(None);
    }
    let version = plugin_manifest_version(&plugin_root.join(".codex-plugin").join("plugin.json"));
    Ok(Some(LocalPluginSource {
        root: plugin_root,
        version,
    }))
}

fn marketplace_source_path_from_config(
    home: &Path,
    marketplace_name: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let text = match std::fs::read_to_string(home.join("config.toml")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| "failed to read config.toml"),
    };
    let doc = parse_toml_document(text.trim_start_matches('\u{feff}'))?;
    let Some(table) = doc
        .get("marketplaces")
        .and_then(Item::as_table)
        .and_then(|marketplaces| marketplaces.get(marketplace_name))
        .and_then(Item::as_table)
    else {
        return Ok(None);
    };
    let source_type = table
        .get("source_type")
        .and_then(Item::as_str)
        .unwrap_or_default();
    if source_type != "local" {
        return Ok(None);
    }
    let source = table
        .get("source")
        .and_then(Item::as_str)
        .unwrap_or_default()
        .trim();
    if source.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(normalize_windows_extended_path(source))))
}

fn plugin_source_relative_path(plugin: &Value) -> Option<PathBuf> {
    let path = plugin
        .get("source")
        .and_then(Value::as_object)
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .or_else(|| plugin.get("path").and_then(Value::as_str))?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed.strip_prefix("./").unwrap_or(trimmed)))
    }
}

fn resolve_marketplace_path(marketplace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        marketplace_root.join(path)
    }
}

fn plugin_manifest_version(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let manifest: Value = serde_json::from_str(&text).ok()?;
    manifest
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn normalize_windows_extended_path(value: &str) -> String {
    value.strip_prefix(r"\\?\").unwrap_or(value).to_string()
}

fn windows_extended_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if !cfg!(windows) || value.starts_with(r"\\?\") {
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

fn ensure_trailing_newline(mut contents: String) -> String {
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_marketplace(home: &Path) {
        let root = home.join(".tmp").join("plugins");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins").join("gmail")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
        )
        .unwrap();
    }

    fn write_remote_marketplace(home: &Path) {
        let root = home.join(".tmp").join("plugins-remote");
        std::fs::create_dir_all(root.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("plugins").join("product-design")).unwrap();
        std::fs::write(
            root.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design","path":"./plugins/product-design"}]}"#,
        )
        .unwrap();
    }

    fn write_local_plugin_marketplace(home: &Path, source: &Path, version: &str, marker: &str) {
        std::fs::create_dir_all(source.join(".agents").join("plugins")).unwrap();
        std::fs::create_dir_all(source.join("src").join("plugins").join(".codex-plugin")).unwrap();
        std::fs::write(
            source
                .join(".agents")
                .join("plugins")
                .join("marketplace.json"),
            r#"{"name":"zeroone","plugins":[{"name":"zeroone","source":{"source":"local","path":"./src/plugins"}}]}"#,
        )
        .unwrap();
        std::fs::write(
            source
                .join("src")
                .join("plugins")
                .join(".codex-plugin")
                .join("plugin.json"),
            format!(r#"{{"name":"zeroone","version":"{version}"}}"#),
        )
        .unwrap();
        std::fs::write(
            source.join("src").join("plugins").join("marker.txt"),
            marker,
        )
        .unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!(
                r#"[marketplaces.zeroone]
source_type = "local"
source = '{}'

[plugins."zeroone@zeroone"]
enabled = true
"#,
                source.display()
            ),
        )
        .unwrap();
    }

    #[test]
    fn ensure_openai_curated_marketplace_config_registers_local_marketplace() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_marketplace(home);

        let changed = ensure_openai_curated_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-curated"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-curated"]["source"].as_str(),
            Some(format!(r"\\?\{}", home.join(".tmp").join("plugins").display()).as_str())
        );
        assert_eq!(
            parsed["marketplaces"]["openai-api-curated"]["source_type"].as_str(),
            Some("local")
        );
        assert_eq!(
            parsed["marketplaces"]["openai-api-curated"]["source"].as_str(),
            Some(format!(r"\\?\{}", home.join(".tmp").join("plugins").display()).as_str())
        );
    }

    #[test]
    fn ensure_openai_curated_marketplace_config_registers_remote_when_present() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_marketplace(home);
        write_remote_marketplace(home);

        let changed = ensure_openai_curated_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source_type"].as_str(),
            Some("local")
        );
        let expected_source = windows_extended_path(&home.join(".tmp").join("plugins-remote"));
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source"].as_str(),
            Some(expected_source.as_str())
        );
    }

    #[test]
    fn ensure_openai_curated_marketplace_config_skips_when_snapshot_missing() {
        let temp = tempfile::tempdir().unwrap();

        let changed = ensure_openai_curated_marketplace_config(temp.path()).unwrap();

        assert!(!changed);
        assert!(!temp.path().join("config.toml").exists());
    }

    #[test]
    fn openai_curated_marketplace_status_detects_missing_config() {
        let temp = tempfile::tempdir().unwrap();
        write_marketplace(temp.path());

        let status = openai_curated_marketplace_status(temp.path());

        assert!(status.marketplace_root.is_some());
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn openai_curated_marketplace_status_requires_api_marketplace_config() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        write_marketplace(home);
        ensure_marketplace_configs(home, &[OPENAI_CURATED_MARKETPLACE], &root).unwrap();

        let status = openai_curated_marketplace_status(home);

        assert!(status.marketplace_root.is_some());
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn openai_curated_marketplace_status_ignores_remote_config_when_cached() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join(".tmp").join("plugins");
        write_marketplace(home);
        write_remote_marketplace(home);
        ensure_marketplace_configs(
            home,
            &[OPENAI_CURATED_MARKETPLACE, OPENAI_API_CURATED_MARKETPLACE],
            &root,
        )
        .unwrap();

        let status = openai_curated_marketplace_status(home);

        assert!(status.marketplace_root.is_some());
        assert!(status.config_registered);
        assert!(!status.needs_repair());
    }

    #[test]
    fn openai_curated_remote_marketplace_status_detects_cached_marketplace() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_remote_marketplace(home);

        let status = openai_curated_remote_marketplace_status(home);

        let expected_root = home.join(".tmp").join("plugins-remote");
        assert_eq!(
            status.marketplace_root.as_deref(),
            Some(expected_root.as_path())
        );
        assert!(!status.config_registered);
        assert!(status.needs_repair());
    }

    #[test]
    fn ensure_openai_curated_remote_marketplace_config_registers_remote_only() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write_remote_marketplace(home);

        let changed = ensure_openai_curated_remote_marketplace_config(home).unwrap();

        assert!(changed);
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = config.parse::<DocumentMut>().unwrap();
        assert!(parsed["marketplaces"].get("openai-curated").is_none());
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source_type"].as_str(),
            Some("local")
        );
        let expected_source = windows_extended_path(&home.join(".tmp").join("plugins-remote"));
        assert_eq!(
            parsed["marketplaces"]["openai-curated-remote"]["source"].as_str(),
            Some(expected_source.as_str())
        );
    }

    #[test]
    fn install_openai_curated_remote_marketplace_zip_installs_valid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file(".agents/plugins/marketplace.json", options)
                .unwrap();
            std::io::Write::write_all(
                &mut writer,
                br#"{"name":"openai-curated-remote","plugins":[{"name":"product-design","path":"./plugins/product-design"}]}"#,
            )
            .unwrap();
            writer
                .start_file("plugins/product-design/.codex-plugin/plugin.json", options)
                .unwrap();
            std::io::Write::write_all(&mut writer, br#"{"name":"product-design"}"#).unwrap();
            writer.finish().unwrap();
        }

        install_openai_curated_remote_marketplace_zip(temp.path(), bytes.get_ref()).unwrap();
        let changed = ensure_openai_curated_remote_marketplace_config(temp.path()).unwrap();

        assert!(changed);
        assert!(
            temp.path()
                .join(".tmp/plugins-remote/.agents/plugins/marketplace.json")
                .is_file()
        );
        assert!(
            temp.path()
                .join(".tmp/plugins-remote/plugins/product-design/.codex-plugin/plugin.json")
                .is_file()
        );
    }

    #[test]
    fn embedded_openai_curated_remote_marketplace_zip_uses_codex_elves_branding() {
        let cursor = Cursor::new(OPENAI_CURATED_REMOTE_MARKETPLACE_ZIP);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let mut marketplace = String::new();
        archive
            .by_name(".agents/plugins/marketplace.json")
            .unwrap()
            .read_to_string(&mut marketplace)
            .unwrap();

        assert!(marketplace.contains("openai-curated-remote"));
        assert!(marketplace.contains("CodexElves"));
        assert!(!marketplace.contains("Codex++"));
        assert!(!marketplace.contains("CodexPlusPlus"));
    }

    #[test]
    fn zip_entry_relative_path_strips_archive_root_and_rejects_escape() {
        assert_eq!(
            zip_entry_relative_path("plugins-main/plugins/gmail/file.txt"),
            Some(PathBuf::from("plugins").join("gmail").join("file.txt"))
        );
        assert_eq!(zip_entry_relative_path("plugins-main/../evil.txt"), None);
        assert_eq!(zip_entry_relative_path("../evil.txt"), None);
    }

    #[test]
    fn safe_zip_path_rejects_escape() {
        assert_eq!(
            safe_zip_path("plugins/product-design/file.txt").unwrap(),
            PathBuf::from("plugins")
                .join("product-design")
                .join("file.txt")
        );
        assert!(safe_zip_path("../evil.txt").is_err());
        assert!(safe_zip_path("/absolute/evil.txt").is_err());
    }

    #[test]
    fn install_openai_plugins_zip_installs_valid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let mut bytes = Cursor::new(Vec::<u8>::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("plugins-main/.agents/plugins/marketplace.json", options)
                .unwrap();
            std::io::Write::write_all(
                &mut writer,
                br#"{"name":"openai-curated","plugins":[{"name":"gmail","path":"./plugins/gmail"}]}"#,
            )
            .unwrap();
            writer
                .start_file(
                    "plugins-main/plugins/gmail/.codex-plugin/plugin.json",
                    options,
                )
                .unwrap();
            std::io::Write::write_all(&mut writer, br#"{"name":"gmail"}"#).unwrap();
            writer.finish().unwrap();
        }

        install_openai_plugins_zip(temp.path(), bytes.get_ref()).unwrap();
        let changed = ensure_openai_curated_marketplace_config(temp.path()).unwrap();

        assert!(changed);
        assert!(
            temp.path()
                .join(".tmp/plugins/.agents/plugins/marketplace.json")
                .is_file()
        );
        assert!(
            temp.path()
                .join(".tmp/plugins/plugins/gmail/.codex-plugin/plugin.json")
                .is_file()
        );
    }

    #[test]
    fn plugin_cache_info_reads_cached_and_source_versions() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("marketplace");
        std::fs::create_dir_all(&home).unwrap();
        write_local_plugin_marketplace(&home, &source, "0.1.2-alpha.7", "source");
        let cache = home
            .join("plugins")
            .join("cache")
            .join("zeroone")
            .join("zeroone")
            .join("0.1.2-alpha.6");
        std::fs::create_dir_all(cache.join(".codex-plugin")).unwrap();
        std::fs::write(
            cache.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"zeroone","version":"0.1.2-alpha.6"}"#,
        )
        .unwrap();

        let info = plugin_cache_info(&home, "zeroone@zeroone");

        assert!(info.cached);
        assert_eq!(info.current_version.as_deref(), Some("0.1.2-alpha.6"));
        assert_eq!(info.source_version.as_deref(), Some("0.1.2-alpha.7"));
        assert!(info.can_refresh);
    }

    #[test]
    fn plugin_cache_info_blocks_refresh_when_source_version_is_lower() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("marketplace");
        std::fs::create_dir_all(&home).unwrap();
        write_local_plugin_marketplace(&home, &source, "0.1.2-alpha.6", "source");
        let cache = home
            .join("plugins")
            .join("cache")
            .join("zeroone")
            .join("zeroone")
            .join("0.1.2-alpha.7");
        std::fs::create_dir_all(cache.join(".codex-plugin")).unwrap();
        std::fs::write(
            cache.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"zeroone","version":"0.1.2-alpha.7"}"#,
        )
        .unwrap();

        let info = plugin_cache_info(&home, "zeroone@zeroone");

        assert!(info.cached);
        assert_eq!(info.current_version.as_deref(), Some("0.1.2-alpha.7"));
        assert_eq!(info.source_version.as_deref(), Some("0.1.2-alpha.6"));
        assert!(!info.can_refresh);
        assert!(info.refresh_reason.contains("降级"));
    }

    #[test]
    fn force_refresh_plugin_cache_rebuilds_cache_from_local_source() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("marketplace");
        std::fs::create_dir_all(&home).unwrap();
        write_local_plugin_marketplace(&home, &source, "0.1.2-alpha.7", "fresh");
        let cache = home
            .join("plugins")
            .join("cache")
            .join("zeroone")
            .join("zeroone")
            .join("0.1.2-alpha.7");
        std::fs::create_dir_all(cache.join(".codex-plugin")).unwrap();
        std::fs::write(
            cache.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"zeroone","version":"0.1.2-alpha.7"}"#,
        )
        .unwrap();
        std::fs::write(cache.join("marker.txt"), "stale").unwrap();

        let info = force_refresh_plugin_cache(&home, "zeroone@zeroone").unwrap();

        assert_eq!(info.current_version.as_deref(), Some("0.1.2-alpha.7"));
        assert_eq!(
            std::fs::read_to_string(cache.join("marker.txt")).unwrap(),
            "fresh"
        );
    }

    #[test]
    fn force_refresh_plugin_cache_rejects_lower_source_version() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("marketplace");
        std::fs::create_dir_all(&home).unwrap();
        write_local_plugin_marketplace(&home, &source, "0.1.2-alpha.6", "fresh");
        let cache = home
            .join("plugins")
            .join("cache")
            .join("zeroone")
            .join("zeroone")
            .join("0.1.2-alpha.7");
        std::fs::create_dir_all(cache.join(".codex-plugin")).unwrap();
        std::fs::write(
            cache.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"zeroone","version":"0.1.2-alpha.7"}"#,
        )
        .unwrap();
        std::fs::write(cache.join("marker.txt"), "keep").unwrap();

        let error = force_refresh_plugin_cache(&home, "zeroone@zeroone").unwrap_err();

        assert!(error.to_string().contains("降级"));
        assert_eq!(
            std::fs::read_to_string(cache.join("marker.txt")).unwrap(),
            "keep"
        );
    }
}
