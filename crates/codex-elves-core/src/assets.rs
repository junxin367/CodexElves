use base64::Engine;
use serde_json::Map;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::Item;

use crate::settings::BackendSettings;

const RENDERER_BOOTSTRAP_SCRIPT: &str = include_str!("../../../assets/inject/renderer-inject.js");
const RENDERER_FEATURES_SCRIPT: &str = include_str!("../../../assets/inject/renderer-features.js");
pub const DIAGNOSTIC_BUILD_ID: &str = "diag-20260831-12";

pub fn renderer_script() -> &'static str {
    RENDERER_FEATURES_SCRIPT
}

pub fn renderer_bootstrap_script() -> &'static str {
    RENDERER_BOOTSTRAP_SCRIPT
}

pub fn renderer_features_script() -> &'static str {
    RENDERER_FEATURES_SCRIPT
}

pub fn injection_script(helper_port: u16) -> String {
    injection_script_with_settings(helper_port, &BackendSettings::default())
}

pub fn injection_script_with_settings(helper_port: u16, settings: &BackendSettings) -> String {
    injection_script_source_with_settings(helper_port, settings, renderer_features_script())
}

pub fn bootstrap_injection_script(helper_port: u16) -> String {
    bootstrap_injection_script_with_settings(helper_port, &BackendSettings::default())
}

pub fn bootstrap_injection_script_with_settings(
    helper_port: u16,
    settings: &BackendSettings,
) -> String {
    injection_script_source_with_settings(helper_port, settings, renderer_bootstrap_script())
}

fn injection_script_source_with_settings(
    helper_port: u16,
    settings: &BackendSettings,
    source: &str,
) -> String {
    let helper_url = format!("http://127.0.0.1:{helper_port}");
    let image_overlay = image_overlay_config(helper_port, settings);
    let codex_home = crate::codex_home::codex_home_dir_for_settings(settings);
    let plugin_marketplaces = local_plugin_marketplaces_from_home(&codex_home);
    let suppressed_threads = crate::suppressed_threads::load_suppressed_ids();
    format!(
        "window.__CODEX_SESSION_DELETE_HELPER__ = {};\nwindow.__CODEX_ELVES_VERSION__ = {};\nwindow.__CODEX_ELVES_BUILD__ = {};\nwindow.__CODEX_ELVES_LAUNCH_CYCLE__ = {};\nwindow.__CODEX_ELVES_IMAGE_OVERLAY__ = {};\nwindow.__CODEX_ELVES_PLUGIN_MARKETPLACES__ = {};\nwindow.__CODEX_ELVES_SUPPRESSED_THREADS__ = {};\n{}",
        serde_json::to_string(&helper_url).expect("helper URL should serialize"),
        serde_json::to_string(crate::version::VERSION).expect("version should serialize"),
        serde_json::to_string(DIAGNOSTIC_BUILD_ID).expect("build id should serialize"),
        serde_json::to_string(injection_launch_cycle_id())
            .expect("launch cycle id should serialize"),
        serde_json::to_string(&image_overlay).expect("image overlay config should serialize"),
        serde_json::to_string(&plugin_marketplaces).expect("plugin marketplaces should serialize"),
        serde_json::to_string(&suppressed_threads).expect("suppressed threads should serialize"),
        source,
    )
}

fn injection_launch_cycle_id() -> &'static str {
    static LAUNCH_CYCLE_ID: OnceLock<String> = OnceLock::new();
    LAUNCH_CYCLE_ID
        .get_or_init(|| {
            let started_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            format!("{}-{started_at}", std::process::id())
        })
        .as_str()
}

fn local_plugin_marketplaces_from_home(home: &Path) -> Value {
    let installed_plugins = installed_plugins_from_config(&home);
    let marketplace_dir = home
        .join(".tmp")
        .join("plugins")
        .join(".agents")
        .join("plugins");
    let candidates = [
        marketplace_dir.join("marketplace.json"),
        marketplace_dir.join("api_marketplace.json"),
        home.join(".tmp")
            .join("plugins-remote")
            .join(".agents")
            .join("plugins")
            .join("marketplace.json"),
    ];
    let mut candidates = candidates.to_vec();
    candidates.extend(marketplace_candidates_from_config(home));
    let mut seen = std::collections::BTreeSet::new();
    let marketplaces = candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let mut marketplace: Value = serde_json::from_str(&text).ok()?;
            expand_local_plugin_marketplace(&mut marketplace, &path, home, &installed_plugins);
            if let Some(object) = marketplace.as_object_mut() {
                object
                    .entry("path")
                    .or_insert_with(|| Value::String(path.to_string_lossy().to_string()));
            }
            Some(marketplace)
        })
        .collect::<Vec<_>>();
    Value::Array(marketplaces)
}

fn marketplace_candidates_from_config(home: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let Ok(doc) = text
        .trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
    else {
        return Vec::new();
    };
    let Some(marketplaces) = doc.get("marketplaces").and_then(Item::as_table) else {
        return Vec::new();
    };
    marketplaces
        .iter()
        .filter_map(|(name, item)| {
            let table = item.as_table()?;
            let source_type = table
                .get("source_type")
                .and_then(Item::as_str)
                .unwrap_or_default();
            let source = table
                .get("source")
                .and_then(Item::as_str)
                .unwrap_or_default()
                .trim();
            if source.is_empty() {
                return None;
            }
            marketplace_root_from_config(home, name, source_type, source)
        })
        .flat_map(|root| {
            [
                root.join(".agents")
                    .join("plugins")
                    .join("marketplace.json"),
                root.join(".agents")
                    .join("plugins")
                    .join("api_marketplace.json"),
            ]
        })
        .collect()
}

fn marketplace_root_from_config(
    home: &Path,
    marketplace_name: &str,
    source_type: &str,
    source: &str,
) -> Option<PathBuf> {
    match source_type {
        "local" => Some(PathBuf::from(normalize_windows_extended_path(source))),
        "git" => {
            let direct = PathBuf::from(normalize_windows_extended_path(source));
            if direct.is_dir() {
                Some(direct)
            } else {
                let checkout = home
                    .join(".tmp")
                    .join("marketplaces")
                    .join(marketplace_name);
                checkout.is_dir().then_some(checkout)
            }
        }
        _ => None,
    }
}

fn expand_local_plugin_marketplace(
    marketplace: &mut Value,
    marketplace_path: &Path,
    home: &Path,
    installed_plugins: &std::collections::BTreeSet<String>,
) {
    let marketplace_name = marketplace
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(plugins) = marketplace.get_mut("plugins").and_then(Value::as_array_mut) else {
        return;
    };
    let marketplace_root = marketplace_path
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".tmp").join("plugins"));
    for plugin in plugins {
        let Some(plugin_object) = plugin.as_object_mut() else {
            continue;
        };
        let plugin_name = plugin_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                plugin_object
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.split('@').next())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if plugin_name.is_empty() {
            continue;
        }
        let plugin_root = plugin_source_relative_path(plugin_object)
            .map(|path| resolve_marketplace_path(&marketplace_root, &path))
            .unwrap_or_else(|| marketplace_root.join("plugins").join(&plugin_name));
        let manifest_path = plugin_root.join(".codex-plugin").join("plugin.json");
        if let Some(manifest) = plugin_manifest(&manifest_path) {
            merge_plugin_manifest(plugin_object, manifest);
        }
        absolutize_plugin_icon_paths(plugin_object, &plugin_root);
        plugin_object
            .entry("name".to_string())
            .or_insert_with(|| Value::String(plugin_name.clone()));
        plugin_object
            .entry("id".to_string())
            .or_insert_with(|| Value::String(format!("{plugin_name}@{marketplace_name}")));
        plugin_object
            .entry("marketplaceName".to_string())
            .or_insert_with(|| Value::String(marketplace_name.clone()));
        plugin_object
            .entry("marketplacePath".to_string())
            .or_insert_with(|| Value::String(format!("remote:{marketplace_name}")));
        plugin_object
            .entry("keywords".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        plugin_object.insert(
            "installed".to_string(),
            Value::Bool(installed_plugins.contains(&format!("{plugin_name}@{marketplace_name}"))),
        );
    }
}

fn plugin_source_relative_path(plugin: &Map<String, Value>) -> Option<PathBuf> {
    let path = plugin
        .get("source")
        .and_then(Value::as_object)
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .or_else(|| {
            plugin
                .get("source")
                .and_then(Value::as_object)
                .and_then(|source| source.get("url"))
                .and_then(Value::as_str)
                .filter(|url| is_local_plugin_source_url(url))
        })
        .or_else(|| plugin.get("path").and_then(Value::as_str))?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed.strip_prefix("./").unwrap_or(trimmed)))
    }
}

fn is_local_plugin_source_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "." || trimmed == ".." || trimmed.starts_with("./") || trimmed.starts_with("../")
    {
        return true;
    }
    if Path::new(trimmed).is_absolute() {
        return true;
    }
    if trimmed.contains("://") || trimmed.starts_with("git@") {
        return false;
    }
    !trimmed.contains(':')
}

fn resolve_marketplace_path(marketplace_root: &Path, path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return marketplace_root.to_path_buf();
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        marketplace_root.join(path)
    }
}

fn absolutize_plugin_icon_paths(plugin: &mut Map<String, Value>, plugin_root: &Path) {
    for key in ["composerIconPath", "logoPath"] {
        absolutize_string_field(plugin, key, plugin_root);
    }
    let Some(interface) = plugin.get_mut("interface").and_then(Value::as_object_mut) else {
        return;
    };
    for key in ["composerIcon", "composerIconUrl", "logo", "logoUrl"] {
        absolutize_string_field(interface, key, plugin_root);
    }
}

fn absolutize_string_field(object: &mut Map<String, Value>, key: &str, root: &Path) {
    let Some(value) = object.get(key).and_then(Value::as_str).map(str::to_string) else {
        return;
    };
    let Some(path) = absolutize_plugin_asset_path(&value, root) else {
        return;
    };
    object.insert(key.to_string(), Value::String(path));
}

fn absolutize_plugin_asset_path(value: &str, root: &Path) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("data:")
        || trimmed.starts_with("http:")
        || trimmed.starts_with("https:")
        || trimmed.starts_with("file:")
        || Path::new(trimmed).is_absolute()
    {
        return None;
    }
    let relative = trimmed.strip_prefix("./").unwrap_or(trimmed);
    Some(root.join(relative).to_string_lossy().to_string())
}

fn plugin_manifest(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .as_object()
        .cloned()
}

fn merge_plugin_manifest(plugin: &mut Map<String, Value>, manifest: Map<String, Value>) {
    for (key, value) in manifest {
        plugin.entry(key).or_insert(value);
    }
}

fn normalize_windows_extended_path(value: &str) -> String {
    value.strip_prefix(r"\\?\").unwrap_or(value).to_string()
}

fn installed_plugins_from_config(home: &Path) -> std::collections::BTreeSet<String> {
    let text = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let doc = text.parse::<toml_edit::DocumentMut>().ok();
    let Some(plugins) = doc
        .as_ref()
        .and_then(|doc| doc.get("plugins"))
        .and_then(toml_edit::Item::as_table)
    else {
        return std::collections::BTreeSet::new();
    };
    plugins
        .iter()
        .filter_map(|(id, item)| {
            let enabled = item
                .get("enabled")
                .and_then(toml_edit::Item::as_bool)
                .unwrap_or(false);
            enabled.then(|| id.to_string())
        })
        .collect()
}

pub fn image_overlay_config(helper_port: u16, settings: &BackendSettings) -> Value {
    // 优先用激活皮肤；无激活皮肤时回退到旧图片覆盖三字段（兼容旧配置）。
    let active = resolve_active_skin_visual(settings);
    let has_path = !active.image_path.trim().is_empty();
    let data_url = if has_path {
        image_file_data_uri(Path::new(active.image_path.trim())).unwrap_or_default()
    } else {
        String::new()
    };
    let enabled = has_path && !data_url.is_empty();
    // 非图片背景（纯色/渐变）无需图片路径即可启用，由前端直接用 CSS 绘制。
    let is_non_image_kind = matches!(active.kind.as_str(), "color" | "gradient");
    let enabled = enabled || is_non_image_kind;
    json!({
        "enabled": enabled,
        "opacity": f64::from(active.opacity.clamp(1, 100)) / 100.0,
        "dataUrl": data_url,
        "appearanceEnabled": active.appearance_enabled,
        "appearance": active.appearance,
        "fit": active.fit,
        "kind": active.kind,
        "backgroundColor": active.background_color,
        "gradientFrom": active.gradient_from,
        "gradientTo": active.gradient_to,
        "gradientAngle": active.gradient_angle,
        "imageUrl": if has_path && !data_url.is_empty() {
            format!("http://127.0.0.1:{helper_port}/overlay/image")
        } else {
            String::new()
        },
    })
}

struct ActiveSkinVisual {
    image_path: String,
    opacity: u8,
    appearance_enabled: bool,
    appearance: String,
    fit: String,
    kind: String,
    background_color: String,
    gradient_from: String,
    gradient_to: String,
    gradient_angle: u16,
}

/// 解析当前应生效的背景视觉：有激活皮肤则用皮肤，否则回退到旧 overlay 字段。
fn resolve_active_skin_visual(settings: &BackendSettings) -> ActiveSkinVisual {
    let active_id = settings.codex_app_active_skin_id.trim();
    if !active_id.is_empty() {
        if let Some(skin) = crate::skin::find_skin(active_id) {
            return ActiveSkinVisual {
                image_path: skin.image_path,
                opacity: skin.opacity,
                appearance_enabled: true,
                appearance: skin.appearance,
                fit: skin.fit,
                kind: skin.kind,
                background_color: skin.background_color,
                gradient_from: skin.gradient_from,
                gradient_to: skin.gradient_to,
                gradient_angle: skin.gradient_angle,
            };
        }
    }
    // 回退：旧版图片覆盖（未启用则 image_path 为空，自然不显示）。
    ActiveSkinVisual {
        image_path: if settings.codex_app_image_overlay_enabled {
            settings.codex_app_image_overlay_path.clone()
        } else {
            String::new()
        },
        opacity: settings.codex_app_image_overlay_opacity,
        appearance_enabled: false,
        appearance: "auto".to_string(),
        fit: "contain".to_string(),
        kind: "image".to_string(),
        background_color: String::new(),
        gradient_from: String::new(),
        gradient_to: String::new(),
        gradient_angle: 135,
    }
}

fn image_data_uri(mime_type: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn image_file_data_uri(path: &Path) -> Option<String> {
    let mime_type = image_content_type(path)?;
    let bytes = std::fs::read(path).ok()?;
    Some(image_data_uri(mime_type, &bytes))
}

/// 供 skin 模块导出皮肤时复用，将本地图片转为 data URI。
pub(crate) fn image_file_data_uri_public(path: &Path) -> Option<String> {
    image_file_data_uri(path)
}

fn image_content_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_plugin_marketplaces_includes_api_marketplace_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let marketplace_dir = home
            .join(".tmp")
            .join("plugins")
            .join(".agents")
            .join("plugins");
        let api_plugin_dir = home
            .join(".tmp")
            .join("plugins")
            .join("plugins")
            .join("build-web-apps");
        let remote_marketplace_dir = home
            .join(".tmp")
            .join("plugins-remote")
            .join(".agents")
            .join("plugins");
        let remote_plugin_dir = home
            .join(".tmp")
            .join("plugins-remote")
            .join("plugins")
            .join("product-design");
        std::fs::create_dir_all(&marketplace_dir).unwrap();
        std::fs::create_dir_all(api_plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(&remote_marketplace_dir).unwrap();
        std::fs::create_dir_all(remote_plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::write(
            marketplace_dir.join("marketplace.json"),
            r#"{"name":"openai-curated","plugins":[{"name":"gmail"}]}"#,
        )
        .unwrap();
        std::fs::write(
            marketplace_dir.join("api_marketplace.json"),
            r#"{"name":"openai-api-curated","plugins":[{"name":"build-web-apps"}]}"#,
        )
        .unwrap();
        std::fs::write(
            remote_marketplace_dir.join("marketplace.json"),
            r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design"}]}"#,
        )
        .unwrap();
        std::fs::write(
            api_plugin_dir.join(".codex-plugin").join("plugin.json"),
            r#"{"interface":{"displayName":"Build Web Apps"}}"#,
        )
        .unwrap();
        std::fs::write(
            remote_plugin_dir.join(".codex-plugin").join("plugin.json"),
            r#"{"interface":{"displayName":"Product Design"}}"#,
        )
        .unwrap();

        let marketplaces = local_plugin_marketplaces_from_home(home);
        let array = marketplaces.as_array().unwrap();

        assert_eq!(array.len(), 3);
        assert_eq!(array[0]["name"].as_str(), Some("openai-curated"));
        assert_eq!(array[1]["name"].as_str(), Some("openai-api-curated"));
        assert_eq!(array[2]["name"].as_str(), Some("openai-curated-remote"));
        assert_eq!(
            array[1]["plugins"][0]["interface"]["displayName"].as_str(),
            Some("Build Web Apps")
        );
        assert_eq!(
            array[2]["plugins"][0]["interface"]["displayName"].as_str(),
            Some("Product Design")
        );
        assert_eq!(
            array[2]["plugins"][0]["marketplaceName"].as_str(),
            Some("openai-curated-remote")
        );
        assert_eq!(
            array[2]["plugins"][0]["marketplacePath"].as_str(),
            Some("remote:openai-curated-remote")
        );
    }

    #[test]
    fn local_plugin_marketplaces_includes_git_marketplace_from_config() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let checkout = home
            .join(".tmp")
            .join("marketplaces")
            .join("superpowers-dev");
        let marketplace_dir = checkout.join(".agents").join("plugins");
        std::fs::create_dir_all(&marketplace_dir).unwrap();
        std::fs::create_dir_all(checkout.join(".codex-plugin")).unwrap();
        std::fs::write(
            home.join("config.toml"),
            r#"[marketplaces.superpowers-dev]
source_type = "git"
source = "https://github.com/obra/superpowers.git"

[plugins."superpowers@superpowers-dev"]
enabled = true
"#,
        )
        .unwrap();
        std::fs::write(
            marketplace_dir.join("marketplace.json"),
            r#"{"name":"superpowers-dev","plugins":[{"name":"superpowers","source":{"source":"url","url":"./"}}]}"#,
        )
        .unwrap();
        std::fs::write(
            checkout.join(".codex-plugin").join("plugin.json"),
            r#"{"version":"6.1.1","interface":{"displayName":"Superpowers","logo":"./assets/app-icon.png"}}"#,
        )
        .unwrap();

        let marketplaces = local_plugin_marketplaces_from_home(home);
        let array = marketplaces.as_array().unwrap();

        assert_eq!(array.len(), 1);
        assert_eq!(array[0]["name"].as_str(), Some("superpowers-dev"));
        assert_eq!(
            array[0]["plugins"][0]["id"].as_str(),
            Some("superpowers@superpowers-dev")
        );
        assert_eq!(array[0]["plugins"][0]["version"].as_str(), Some("6.1.1"));
        assert_eq!(
            array[0]["plugins"][0]["interface"]["displayName"].as_str(),
            Some("Superpowers")
        );
        assert_eq!(array[0]["plugins"][0]["installed"].as_bool(), Some(true));
        assert_eq!(
            array[0]["plugins"][0]["marketplacePath"].as_str(),
            Some("remote:superpowers-dev")
        );
    }

    #[test]
    fn injection_script_uses_settings_codex_home_for_plugin_marketplaces() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("custom-codex-home");
        let remote_marketplace_dir = home
            .join(".tmp")
            .join("plugins-remote")
            .join(".agents")
            .join("plugins");
        let remote_plugin_dir = home
            .join(".tmp")
            .join("plugins-remote")
            .join("plugins")
            .join("product-design");
        std::fs::create_dir_all(&remote_marketplace_dir).unwrap();
        std::fs::create_dir_all(remote_plugin_dir.join(".codex-plugin")).unwrap();
        std::fs::write(
            remote_marketplace_dir.join("marketplace.json"),
            r#"{"name":"openai-curated-remote","plugins":[{"name":"product-design"}]}"#,
        )
        .unwrap();
        std::fs::write(
            remote_plugin_dir.join(".codex-plugin").join("plugin.json"),
            r#"{"interface":{"displayName":"Product Design"}}"#,
        )
        .unwrap();
        let settings = BackendSettings {
            codex_home_path: home.to_string_lossy().to_string(),
            ..BackendSettings::default()
        };

        let script = injection_script_source_with_settings(45221, &settings, "");

        assert!(script.contains("openai-curated-remote"));
        assert!(script.contains("Product Design"));
        assert!(script.contains("remote:openai-curated-remote"));
    }

    // 最小 1x1 PNG（用于验证 image_file_data_uri 能读出 data-uri）。
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    // 无激活皮肤时，image_overlay_config 回退到旧版图片覆盖字段（老配置兼容）。
    #[test]
    fn image_overlay_config_falls_back_to_legacy_overlay_without_active_skin() {
        let temp = tempfile::tempdir().unwrap();
        let image_path = temp.path().join("bg.png");
        std::fs::write(&image_path, TINY_PNG).unwrap();
        let settings = BackendSettings {
            codex_app_active_skin_id: String::new(),
            codex_app_image_overlay_enabled: true,
            codex_app_image_overlay_path: image_path.to_string_lossy().to_string(),
            codex_app_image_overlay_opacity: 50,
            ..BackendSettings::default()
        };

        let config = image_overlay_config(45221, &settings);
        assert_eq!(config["enabled"], serde_json::json!(true));
        assert_eq!(config["opacity"], serde_json::json!(0.5));
        assert_eq!(config["fit"], serde_json::json!("contain"));
        assert_eq!(config["appearanceEnabled"], serde_json::json!(false));
        assert_eq!(config["appearance"], serde_json::json!("auto"));
        assert!(
            config["dataUrl"]
                .as_str()
                .unwrap_or_default()
                .starts_with("data:image/png;base64,")
        );
    }

    // 图片覆盖未启用且无激活皮肤时，输出 disabled。
    #[test]
    fn image_overlay_config_disabled_without_skin_or_legacy_overlay() {
        let settings = BackendSettings::default();
        let config = image_overlay_config(45221, &settings);
        assert_eq!(config["enabled"], serde_json::json!(false));
    }
}
