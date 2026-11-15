use serde_json::Value;

pub(super) fn assigned_index(thread: &Value, projects: &[Value]) -> Option<usize> {
    // 原生明确的 projectId（包括 null）优先；不读取 Codex App 的迁移、归属或排序私有状态。
    if let Some(project) = thread.get("projectId") {
        return project
            .as_str()
            .and_then(|id| projects.iter().position(|project| project["id"] == id));
    }
    // 兼容未提供 projectId 的旧引擎：仅唯一命中已有项目根目录时关联，不创建合成项目。
    let cwd = thread["cwd"].as_str()?;
    let mut matches = projects.iter().enumerate().filter(|(_, project)| {
        project["roots"].as_array().is_some_and(|roots| {
            roots
                .iter()
                .any(|root| root.as_str().is_some_and(|path| same_path(path, cwd)))
        })
    });
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

fn same_path(left: &str, right: &str) -> bool {
    let left = left.trim_end_matches(['/', '\\']).replace('\\', "/");
    let right = right.trim_end_matches(['/', '\\']).replace('\\', "/");
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}
