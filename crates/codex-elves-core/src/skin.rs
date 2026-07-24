//! 皮肤管理：Codex 界面背景主题（“图片覆盖”的升级版）。
//!
//! 一个皮肤 = 背景图 + 透明度 + 界面明暗 + 铺法。相比旧的单张
//! 图片覆盖，支持保存多套主题、命名并一键切换。皮肤列表持久化到独立
//! `~/.codex-session-delete/skins.json`，避免 settings 膨胀；当前激活的皮肤
//! id 存在 settings 里。注入运行时按激活皮肤应用背景，并可经 CDP 即时生效。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 皮肤数量上限，避免文件无限增长。
const MAX_SKINS: usize = 200;
const BUILTIN_ARINA_HASHIMOTO_IMAGE: &[u8] =
    include_bytes!("../../../assets/skins/builtin/arina-hashimoto.png");
const BUILTIN_JACKSON_YEE_IMAGE: &[u8] =
    include_bytes!("../../../assets/skins/builtin/jackson-yee.png");
const BUILTIN_DILRABA_IMAGE: &[u8] = include_bytes!("../../../assets/skins/builtin/dilraba.png");

/// 单个皮肤主题。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skin {
    /// 唯一 id（新建时由前端或后端生成）。
    pub id: String,
    /// 主题名称。
    #[serde(default)]
    pub name: String,
    /// 背景类型：image（图片）/ color（纯色）/ gradient（渐变）。
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 背景图片的本地路径。
    #[serde(default)]
    pub image_path: String,
    /// 纯色背景色值（kind=color 时使用），如 "#1e293b"。
    #[serde(default)]
    pub background_color: String,
    /// 渐变起始色（kind=gradient 时使用）。
    #[serde(default)]
    pub gradient_from: String,
    /// 渐变终止色（kind=gradient 时使用）。
    #[serde(default)]
    pub gradient_to: String,
    /// 渐变角度，0-360（kind=gradient 时使用）。
    #[serde(default = "default_gradient_angle")]
    pub gradient_angle: u16,
    /// 透明度 1-100。
    #[serde(default = "default_opacity")]
    pub opacity: u8,
    /// 界面明暗：auto / light / dark。
    #[serde(default = "default_appearance")]
    pub appearance: String,
    /// 铺法：cover（铺满裁剪）/ contain（完整不裁剪）。
    #[serde(default = "default_fit")]
    pub fit: String,
}

fn default_opacity() -> u8 {
    35
}

fn default_appearance() -> String {
    "auto".to_string()
}

fn default_fit() -> String {
    "cover".to_string()
}

fn default_kind() -> String {
    "image".to_string()
}

fn default_gradient_angle() -> u16 {
    135
}

impl Skin {
    /// 归一化字段到合法范围，防止前端传入越界值。
    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        self.image_path = self.image_path.trim().to_string();
        self.opacity = self.opacity.clamp(1, 100);
        if !matches!(self.appearance.as_str(), "auto" | "light" | "dark") {
            self.appearance = "auto".to_string();
        }
        if !matches!(self.fit.as_str(), "cover" | "contain") {
            self.fit = "cover".to_string();
        }
        if !matches!(self.kind.as_str(), "image" | "color" | "gradient") {
            self.kind = "image".to_string();
        }
        self.background_color = self.background_color.trim().to_string();
        self.gradient_from = self.gradient_from.trim().to_string();
        self.gradient_to = self.gradient_to.trim().to_string();
        self.gradient_angle = self.gradient_angle.min(360);
    }
}

fn store_path() -> PathBuf {
    crate::paths::default_skins_path()
}

fn skin_images_dir() -> PathBuf {
    store_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::paths::default_app_state_dir)
        .join("skin-images")
}

fn read_list(path: &Path) -> Vec<Skin> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Skin>>(&text).unwrap_or_default()
}

fn write_list(path: &Path, list: &[Skin]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(list).unwrap_or_else(|_| b"[]".to_vec());
    crate::settings::atomic_write(path, &bytes)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

/// 读取全部皮肤（归一化后）。
pub fn load_skins() -> Vec<Skin> {
    let mut list = read_list(&store_path());
    for skin in list.iter_mut() {
        skin.normalize();
    }
    list.retain(|skin| !skin.id.is_empty());
    list
}

/// 新增或更新一个皮肤（按 id upsert），返回更新后的完整列表。
pub fn upsert_skin(mut skin: Skin) -> Vec<Skin> {
    skin.normalize();
    let path = store_path();
    let mut list = load_skins();
    if skin.id.is_empty() {
        return list;
    }
    if let Some(existing) = list.iter_mut().find(|item| item.id == skin.id) {
        *existing = skin;
    } else {
        if list.len() >= MAX_SKINS {
            return list;
        }
        list.push(skin);
    }
    let _ = write_list(&path, &list);
    list
}

/// 删除一个皮肤，返回更新后的完整列表。
pub fn delete_skin(id: &str) -> Vec<Skin> {
    let id = id.trim();
    let path = store_path();
    let mut list = load_skins();
    let before = list.len();
    list.retain(|item| item.id != id);
    if list.len() != before {
        let _ = write_list(&path, &list);
    }
    list
}

/// 按 id 找皮肤。
pub fn find_skin(id: &str) -> Option<Skin> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    load_skins().into_iter().find(|item| item.id == id)
}

/// 克隆一个已存在的皮肤（新 id，名称加“副本”后缀），返回新皮肤。
pub fn clone_skin(id: &str) -> Option<Skin> {
    let source = find_skin(id)?;
    let mut cloned = source;
    cloned.id = uuid::Uuid::new_v4().to_string();
    cloned.name = format!("{} 副本", cloned.name.trim());
    let list = upsert_skin(cloned.clone());
    list.into_iter().find(|item| item.id == cloned.id)
}

/// 导出为 json 字符串：图片转 base64 内嵌，方便跨机分享（不依赖本地图片路径）。
pub fn export_skin_json(id: &str) -> Option<String> {
    let mut skin = find_skin(id)?;
    let inline_image = if skin.kind == "image" && !skin.image_path.trim().is_empty() {
        crate::assets::image_file_data_uri_public(Path::new(skin.image_path.trim()))
    } else {
        None
    };
    // 导出时不带本地绝对路径，避免泄露本机目录结构；图片内容改用 base64 字段携带。
    skin.image_path = String::new();
    let mut value = serde_json::to_value(&skin).ok()?;
    if let Some(data_uri) = inline_image {
        value.as_object_mut()?.insert(
            "imageDataUrl".to_string(),
            serde_json::Value::String(data_uri),
        );
    }
    serde_json::to_string_pretty(&value).ok()
}

/// 从 json 导入一个皮肤：若带 imageDataUrl 则落盘到本地图片目录，并分配新 id。
pub fn import_skin_json(text: &str) -> Result<Skin, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("json 解析失败：{error}"))?;
    let image_data_url = value
        .as_object_mut()
        .and_then(|object| object.remove("imageDataUrl"))
        .and_then(|item| item.as_str().map(str::to_string));
    let mut skin: Skin =
        serde_json::from_value(value).map_err(|error| format!("皮肤格式不合法：{error}"))?;
    skin.id = uuid::Uuid::new_v4().to_string();
    if let Some(data_url) = image_data_url {
        let saved_path = save_imported_image(&skin.id, &data_url)
            .map_err(|error| format!("保存导入图片失败：{error}"))?;
        skin.image_path = saved_path;
    }
    skin.normalize();
    if skin.name.is_empty() {
        skin.name = "导入的皮肤".to_string();
    }
    upsert_skin(skin.clone());
    Ok(skin)
}

fn save_imported_image(skin_id: &str, data_url: &str) -> std::io::Result<String> {
    let (mime, base64_data) = data_url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
        .ok_or_else(|| std::io::Error::other("不是合法的 data URI"))?;
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        _ => return Err(std::io::Error::other("不支持的图片类型")),
    };
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_data)
        .map_err(std::io::Error::other)?;
    let dir = skin_images_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{skin_id}.{extension}"));
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().to_string())
}

/// 内置预设主题：纯色、渐变与已授权的图片皮肤。
pub fn builtin_presets() -> Vec<Skin> {
    vec![
        preset_image(
            "builtin-arina-hashimoto",
            "桥本有菜专属定制皮肤",
            "arina-hashimoto-v1.png",
            BUILTIN_ARINA_HASHIMOTO_IMAGE,
            35,
        ),
        preset_image(
            "builtin-jackson-yee",
            "易烊千玺专属定制皮肤",
            "jackson-yee-v1.png",
            BUILTIN_JACKSON_YEE_IMAGE,
            32,
        ),
        preset_image(
            "builtin-dilraba",
            "迪丽热巴专属定制皮肤",
            "dilraba-v1.png",
            BUILTIN_DILRABA_IMAGE,
            32,
        ),
        preset_color("builtin-slate", "墨墨灰", "#1e293b"),
        preset_color("builtin-ink", "深葡黑", "#170b26"),
        preset_gradient("builtin-aurora", "极光紫", "#4338ca", "#0ea5e9", 135),
        preset_gradient("builtin-sunset", "日落橘", "#f97316", "#7c3aed", 120),
        preset_gradient("builtin-forest", "深林绿", "#065f46", "#134e4a", 150),
    ]
}

fn preset_color(id: &str, name: &str, color: &str) -> Skin {
    let mut skin = builtin_base(id, name);
    skin.kind = "color".to_string();
    skin.background_color = color.to_string();
    skin
}

fn preset_gradient(id: &str, name: &str, from: &str, to: &str, angle: u16) -> Skin {
    let mut skin = builtin_base(id, name);
    skin.kind = "gradient".to_string();
    skin.gradient_from = from.to_string();
    skin.gradient_to = to.to_string();
    skin.gradient_angle = angle;
    skin
}

fn preset_image(id: &str, name: &str, filename: &str, bytes: &[u8], opacity: u8) -> Skin {
    let mut skin = builtin_base(id, name);
    skin.image_path = materialize_builtin_image(filename, bytes).unwrap_or_default();
    skin.opacity = opacity;
    skin
}

fn materialize_builtin_image(filename: &str, bytes: &[u8]) -> std::io::Result<String> {
    let dir = skin_images_dir().join("builtin");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    let current_length = std::fs::metadata(&path).map(|metadata| metadata.len()).ok();
    if current_length != Some(bytes.len() as u64) {
        crate::settings::atomic_write(&path, bytes)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
    }
    Ok(path.to_string_lossy().to_string())
}

fn builtin_base(id: &str, name: &str) -> Skin {
    Skin {
        id: id.to_string(),
        name: name.to_string(),
        kind: "image".to_string(),
        image_path: String::new(),
        background_color: String::new(),
        gradient_from: String::new(),
        gradient_to: String::new(),
        gradient_angle: 135,
        opacity: 100,
        appearance: "auto".to_string(),
        fit: "cover".to_string(),
    }
}

/// 一键将内置预设写入用户皮肤列表（若不存在），返回完整列表。
pub fn ensure_builtin_presets_installed() -> Vec<Skin> {
    let list = load_skins();
    let mut presets = builtin_presets();
    for preset in &mut presets {
        preset.normalize();
    }
    let preset_ids = presets
        .iter()
        .map(|preset| preset.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut reordered = list
        .iter()
        .filter(|skin| !preset_ids.contains(skin.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    reordered.extend(presets);
    if reordered != list {
        let _ = write_list(&store_path(), &reordered);
    }
    reordered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static SKINS_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn sample(id: &str) -> Skin {
        Skin {
            id: id.to_string(),
            name: format!("theme {id}"),
            kind: "image".to_string(),
            image_path: "C:/img.png".to_string(),
            background_color: String::new(),
            gradient_from: String::new(),
            gradient_to: String::new(),
            gradient_angle: 135,
            opacity: 50,
            appearance: "dark".to_string(),
            fit: "cover".to_string(),
        }
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut skin = sample("s1");
        skin.opacity = 200;
        skin.appearance = "weird".to_string();
        skin.fit = "stretch".to_string();
        skin.normalize();
        assert_eq!(skin.opacity, 100);
        assert_eq!(skin.appearance, "auto");
        assert_eq!(skin.fit, "cover");
    }

    #[test]
    fn normalize_falls_back_invalid_kind_to_image() {
        let mut skin = sample("s2");
        skin.kind = "video".to_string();
        skin.gradient_angle = 999;
        skin.normalize();
        assert_eq!(skin.kind, "image");
        assert_eq!(skin.gradient_angle, 360);
    }

    struct SkinsPathGuard {
        _lock: MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
        previous: Option<PathBuf>,
    }

    impl SkinsPathGuard {
        fn new() -> Self {
            let lock = SKINS_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("skins.json");
            let previous = crate::paths::set_skins_path_for_tests(Some(path));
            Self {
                _lock: lock,
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for SkinsPathGuard {
        fn drop(&mut self) {
            crate::paths::set_skins_path_for_tests(self.previous.take());
        }
    }

    #[test]
    fn clone_skin_generates_new_id_and_copy_name() {
        let _guard = SkinsPathGuard::new();
        upsert_skin(sample("origin"));
        let cloned = clone_skin("origin").expect("clone should succeed");
        assert_ne!(cloned.id, "origin");
        assert!(cloned.name.ends_with("副本"));
        assert_eq!(load_skins().len(), 2);
    }

    #[test]
    fn export_then_import_roundtrips_non_image_skin() {
        let _guard = SkinsPathGuard::new();
        let mut skin = sample("gradient-1");
        skin.kind = "gradient".to_string();
        skin.image_path = String::new();
        skin.gradient_from = "#111111".to_string();
        skin.gradient_to = "#222222".to_string();
        upsert_skin(skin);
        let json = export_skin_json("gradient-1").expect("export should succeed");
        assert!(!json.contains("imageDataUrl"));
        let imported = import_skin_json(&json).expect("import should succeed");
        assert_ne!(imported.id, "gradient-1");
        assert_eq!(imported.kind, "gradient");
        assert_eq!(imported.gradient_from, "#111111");
    }

    #[test]
    fn export_image_skin_inlines_base64_and_import_saves_file() {
        let _guard = SkinsPathGuard::new();
        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let temp = tempfile::tempdir().unwrap();
        let image_path = temp.path().join("bg.png");
        std::fs::write(&image_path, TINY_PNG).unwrap();
        let mut skin = sample("img-1");
        skin.image_path = image_path.to_string_lossy().to_string();
        upsert_skin(skin);
        let json = export_skin_json("img-1").expect("export should succeed");
        assert!(json.contains("data:image/png;base64,"));
        let imported = import_skin_json(&json).expect("import should succeed");
        assert!(!imported.image_path.is_empty());
        assert!(Path::new(&imported.image_path).is_file());
    }

    #[test]
    fn ensure_builtin_presets_installed_is_idempotent() {
        let _guard = SkinsPathGuard::new();
        let first = ensure_builtin_presets_installed();
        let preset_count = builtin_presets().len();
        assert_eq!(first.len(), preset_count);
        assert_eq!(
            first
                .iter()
                .take(3)
                .map(|skin| skin.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "builtin-arina-hashimoto",
                "builtin-jackson-yee",
                "builtin-dilraba",
            ]
        );
        for id in [
            "builtin-arina-hashimoto",
            "builtin-jackson-yee",
            "builtin-dilraba",
        ] {
            let skin = first
                .iter()
                .find(|skin| skin.id == id)
                .expect("image preset should exist");
            assert_eq!(skin.kind, "image");
            assert!(Path::new(&skin.image_path).is_file());
        }
        let second = ensure_builtin_presets_installed();
        assert_eq!(second.len(), preset_count);

        let mut old_order = second;
        old_order.reverse();
        write_list(&store_path(), &old_order).unwrap();
        let reordered = ensure_builtin_presets_installed();
        assert_eq!(
            reordered
                .iter()
                .take(3)
                .map(|skin| skin.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "builtin-arina-hashimoto",
                "builtin-jackson-yee",
                "builtin-dilraba",
            ]
        );
    }
}
