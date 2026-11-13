use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Context;
use serde_json::{Map, Value};

#[derive(Default)]
pub(super) struct DesktopProjects {
    saved: Map<String, Value>,
    aliases: HashMap<String, String>,
    assignments: Map<String, Value>,
    projectless: HashSet<String>,
    migration: Value,
    pub order: Vec<String>,
    pub pinned: Vec<String>,
}

impl DesktopProjects {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let bytes = match std::fs::read(home.join(".codex-global-state.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).context("读取桌面项目归属失败"),
        };
        let state: Value = serde_json::from_slice(&bytes).context("桌面项目归属文件格式无效")?;
        let scoped = |key: &str| {
            state[key]
                .as_object()
                .and_then(|hosts| {
                    hosts.iter().find(|(scope, _)| {
                        scope
                            .strip_prefix("local:")
                            .is_some_and(|path| same_path(path, &home.to_string_lossy()))
                    })
                })
                .map(|(_, value)| value.clone())
                .unwrap_or(Value::Null)
        };
        Ok(Self {
            saved: state["local-projects"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
            aliases: scoped("app-server-project-id-by-legacy-project-id-by-host")
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(id, server)| Some((id.clone(), server.as_str()?.to_owned())))
                .collect(),
            assignments: state["thread-project-assignments"]
                .as_object()
                .cloned()
                .unwrap_or_default(),
            projectless: strings(&state["projectless-thread-ids"])
                .into_iter()
                .collect(),
            migration: scoped("app-server-projects-migration-by-host"),
            order: strings(&state["project-order"]),
            pinned: strings(&state["pinned-project-ids"]),
        })
    }

    pub fn server_id<'a>(&'a self, id: &'a str) -> &'a str {
        self.aliases.get(id).map(String::as_str).unwrap_or(id)
    }

    pub fn display_project(&self, project: &mut Value) {
        let Some(server_id) = project["id"].as_str().map(str::to_owned) else {
            return;
        };
        // 用户要求切换模式后仍是同一个项目；桌面迁移保留旧 ID，服务写入使用对应引擎 ID。
        let display_id = self
            .aliases
            .iter()
            .find(|(id, server)| *server == &server_id && self.saved.contains_key(*id))
            .map(|(id, _)| id.as_str())
            .unwrap_or(&server_id);
        project["serverProjectId"] = Value::String(server_id.clone());
        project["id"] = Value::String(display_id.to_owned());
        if let Some(name) = self
            .saved
            .get(display_id)
            .and_then(|saved| saved["name"].as_str())
            .filter(|name| !name.is_empty())
        {
            project["name"] = Value::String(name.to_owned());
        }
    }

    pub fn assigned_index(&self, thread: &Value, projects: &[Value]) -> Option<usize> {
        let find = |id: &str| {
            projects
                .iter()
                .position(|project| project["id"] == id || project["serverProjectId"] == id)
        };
        let id = thread["id"].as_str().unwrap_or("");
        // 用户要求保留桌面项目的真实归属：迁移未完成时读取桌面明确记录，不能把引擎的 null
        // 当成已移出项目；迁移完成后以引擎为准，避免旧记录覆盖后续移动或移出操作。
        let legacy = !self.saved.is_empty()
            && self.migration["threadAssignmentsMigrated"] != true
            && (self.migration["threadAssignmentsReadMigrated"] != true
                || self.migration["pendingThreadAssignmentIds"]
                    .as_array()
                    .is_some_and(|pending| pending.iter().any(|pending| pending == id)));
        if legacy {
            if let Some(assignment) = self.assignments.get(id).filter(|value| !value.is_null()) {
                let project_id = assignment.as_str().or_else(|| {
                    (assignment["projectKind"] == "local")
                        .then(|| assignment["projectId"].as_str())
                        .flatten()
                });
                return project_id.and_then(find);
            }
            if self.projectless.contains(id) {
                return None;
            }
        }
        if let Some(assignment) = thread.get("projectId") {
            if let Some(id) = assignment.as_str() {
                return find(id);
            }
            if !legacy {
                return None;
            }
        }
        // 保留既有项目迁移的兼容规则：缺少明确归属时，仅唯一命中已有项目才能关联。
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
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
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
