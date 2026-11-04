use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, bail};
use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table, value};

static POLICY_LOCK: Mutex<()> = Mutex::new(());
const UPDATE_FEATURE: &str = "in_app_updates";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUpdatePolicy {
    pub disabled: bool,
    pub requirements_path: String,
}

pub fn system_requirements_path() -> anyhow::Result<PathBuf> {
    #[cfg(windows)]
    {
        let program_data = std::env::var_os("ProgramData")
            .filter(|path| !path.is_empty())
            .context("未找到 ProgramData 系统目录")?;
        Ok(PathBuf::from(program_data)
            .join("OpenAI")
            .join("Codex")
            .join("requirements.toml"))
    }
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("/etc/codex/requirements.toml"))
    }
}

pub fn read(path: &Path) -> anyhow::Result<CodexUpdatePolicy> {
    let contents = read_contents(path)?;
    let document = parse(&contents)?;
    policy(&document, path)
}

pub fn set_disabled(path: &Path, disabled: bool) -> anyhow::Result<CodexUpdatePolicy> {
    let _guard = POLICY_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("更新策略正在处理，请重试"))?;
    let contents = read_contents(path)?;
    let mut document = parse(&contents)?;
    policy(&document, path)?;

    // 用户要求开关控制 Codex 应用内自动更新：只修改系统 requirements.toml，
    // 不能写入个人 config.toml；关闭开关时移除限制，必须保留其他策略和注释。
    if disabled {
        if !document.contains_key("features") {
            document["features"] = Item::Table(Table::new());
        }
        let features = document["features"]
            .as_table_like_mut()
            .context("requirements.toml 中的 features 必须是表")?;
        if let Some(existing) = features.get_mut(UPDATE_FEATURE) {
            if existing.as_bool() == Some(false) {
                return policy(&document, path);
            }
            let decor = existing.as_value().map(|value| value.decor().clone());
            *existing = value(false);
            if let Some(decor) = decor {
                *existing.as_value_mut().unwrap().decor_mut() = decor;
            }
        } else {
            features.insert(UPDATE_FEATURE, value(false));
        }
    } else {
        let Some(features) = document
            .get_mut("features")
            .and_then(Item::as_table_like_mut)
        else {
            return policy(&document, path);
        };
        if features.remove(UPDATE_FEATURE).is_none() {
            return policy(&document, path);
        }
    }

    let mut updated = document.to_string();
    if contents.starts_with('\u{feff}') {
        updated.insert(0, '\u{feff}');
    }
    crate::settings::atomic_write(path, updated.as_bytes()).with_context(|| {
        format!(
            "无法保存更新策略 {}；若权限不足，请以管理员身份运行 CodexElves 管理器后重试",
            path.display()
        )
    })?;
    let saved = read(path)?;
    if saved.disabled != disabled {
        bail!("更新策略保存后校验不一致，请刷新后重试");
    }
    Ok(saved)
}

fn read_contents(path: &Path) -> anyhow::Result<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("无法读取更新策略 {}", path.display())),
    }
}

fn parse(contents: &str) -> anyhow::Result<DocumentMut> {
    contents
        .trim_start_matches('\u{feff}')
        .parse::<DocumentMut>()
        .context("requirements.toml 格式无效，未修改文件")
}

fn policy(document: &DocumentMut, path: &Path) -> anyhow::Result<CodexUpdatePolicy> {
    let mut disabled = false;
    if let Some(features) = document.get("features") {
        let features = features
            .as_table_like()
            .context("requirements.toml 中的 features 必须是表，未修改文件")?;
        if let Some(setting) = features.get(UPDATE_FEATURE) {
            disabled = !setting
                .as_bool()
                .context("in_app_updates 必须是布尔值，未修改文件")?;
        }
    }
    Ok(CodexUpdatePolicy {
        disabled,
        requirements_path: path.to_string_lossy().into_owned(),
    })
}
