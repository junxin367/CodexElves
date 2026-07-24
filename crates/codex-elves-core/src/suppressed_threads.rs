//! 已删除会话的持久抑制集合。
//!
//! Codex 桌面应用升级后，注入脚本用来做原生归档的内部 manager 可能不可用，
//! 导致“删除后展开项目会话复现”。为不依赖任何 App 内部 API，这里维护一份
//! 独立持久化的“已删除 thread ID 集合”。注入运行时据此在 DOM 层拦截并移除
//! 任何复现的会话行，无论 App 如何重渲染或后续更新都能兜底。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 抑制集合上限，避免文件无限增长。超出后丢弃最旧写入的条目。
const MAX_SUPPRESSED_THREADS: usize = 5000;

fn store_path() -> PathBuf {
    crate::paths::default_suppressed_threads_path()
}

/// 把 thread ID 归一化：去掉 `local:` 前缀与首尾空白，统一小写。
///
/// 侧边栏行的 `data-app-action-sidebar-thread-id` 形如 `local:<uuid>`，
/// 而后端删除拿到的 session_id 常是裸 UUID。归一化后两者可稳定匹配。
pub fn normalize_thread_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_prefix = trimmed.strip_prefix("local:").unwrap_or(trimmed);
    without_prefix.trim().to_ascii_lowercase()
}

fn read_list(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

fn write_list(path: &Path, list: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(list).unwrap_or_else(|_| b"[]".to_vec());
    crate::settings::atomic_write(path, &bytes)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

/// 读取当前抑制集合（归一化后的 ID 列表，保持写入顺序）。
pub fn load_suppressed_ids() -> Vec<String> {
    read_list(&store_path())
}

/// 把一个 thread ID 加入抑制集合，返回更新后的完整列表。
pub fn suppress_thread(raw_id: &str) -> Vec<String> {
    let id = normalize_thread_id(raw_id);
    let path = store_path();
    let mut list = read_list(&path);
    if id.is_empty() {
        return list;
    }
    if list.iter().any(|existing| existing == &id) {
        return list;
    }
    list.push(id);
    if list.len() > MAX_SUPPRESSED_THREADS {
        let overflow = list.len() - MAX_SUPPRESSED_THREADS;
        list.drain(0..overflow);
    }
    let _ = write_list(&path, &list);
    list
}

/// 把一个 thread ID 从抑制集合移除（撤销删除时使用），返回更新后的完整列表。
pub fn unsuppress_thread(raw_id: &str) -> Vec<String> {
    let id = normalize_thread_id(raw_id);
    let path = store_path();
    let mut list = read_list(&path);
    let before = list.len();
    list.retain(|existing| existing != &id);
    if list.len() != before {
        let _ = write_list(&path, &list);
    }
    list
}

/// 用于去重/集合运算的辅助：把列表转为 `BTreeSet`。
pub fn suppressed_set() -> BTreeSet<String> {
    load_suppressed_ids().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_local_prefix_and_lowercases() {
        assert_eq!(
            normalize_thread_id("local:019F62F5-95E3-7963"),
            "019f62f5-95e3-7963"
        );
        assert_eq!(normalize_thread_id("  abc-DEF  "), "abc-def");
    }
}
