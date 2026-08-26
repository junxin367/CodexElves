use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use codex_elves_core::task_board::{
    TaskBoardCatalogProject, TaskBoardCatalogSession, TaskBoardCatalogWarning,
    TaskBoardCatalogWarningCode, TaskBoardSessionCatalog, normalize_task_project_cwd,
    task_board_timestamp_from_bridge_i64,
};
use serde_json::{Map, Value};

use crate::storage::{LocalSessionCatalog, LocalSessionCatalogWarning};

const CODEX_GLOBAL_STATE_FILE: &str = ".codex-global-state.json";
const CODEX_GLOBAL_STATE_BACKUP_FILE: &str = ".codex-global-state.json.bak";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodexProjectCatalog {
    projects: Vec<CodexProject>,
    local_project_id_by_thread: HashMap<String, String>,
    assigned_thread_ids: HashSet<String>,
    projectless_thread_ids: HashSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CodexProject {
    id: String,
    label: String,
    cwd: String,
    root_cwds: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CodexProjectCatalogError {
    #[error("Codex project catalog is unavailable")]
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TaskBoardCatalogBuildError {
    #[error("Task board session catalog contains an invalid timestamp")]
    InvalidTimestamp,
    #[error("Task board project session count is too large")]
    ProjectSessionCountTooLarge,
    #[error("Task board catalog warning count is too large")]
    WarningCountTooLarge,
}

pub fn load_codex_project_catalog(
    codex_home: &Path,
) -> Result<CodexProjectCatalog, CodexProjectCatalogError> {
    let candidates = [
        codex_home.join(CODEX_GLOBAL_STATE_FILE),
        codex_home.join(CODEX_GLOBAL_STATE_BACKUP_FILE),
    ];
    let mut unavailable = false;

    for path in candidates {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => {
                unavailable = true;
                continue;
            }
        };
        match serde_json::from_str::<Value>(&contents) {
            Ok(state) => return Ok(codex_project_catalog_from_state(&state)),
            Err(_) => unavailable = true,
        }
    }

    if unavailable {
        Err(CodexProjectCatalogError::Unavailable)
    } else {
        Ok(CodexProjectCatalog::default())
    }
}

pub fn codex_project_catalog_from_state(state: &Value) -> CodexProjectCatalog {
    let mut catalog = match state.get("local-projects") {
        Some(Value::Object(local_projects)) => modern_project_catalog(state, local_projects),
        Some(_) => CodexProjectCatalog::default(),
        None => legacy_project_catalog(state),
    };
    populate_thread_project_state(state, &mut catalog);
    catalog
}

pub fn task_board_catalog_from_local_catalog(
    local_catalog: LocalSessionCatalog,
    project_catalog: CodexProjectCatalog,
) -> Result<TaskBoardSessionCatalog, TaskBoardCatalogBuildError> {
    let mut project_index_by_id = HashMap::new();
    let mut primary_project_index_by_cwd: HashMap<String, Option<usize>> = HashMap::new();
    let mut root_project_index_by_cwd: HashMap<String, Option<usize>> = HashMap::new();
    let mut projects = project_catalog
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            project_index_by_id.insert(project.id.clone(), index);
            insert_unique_project_index(
                &mut primary_project_index_by_cwd,
                project.cwd.clone(),
                index,
            );
            for root_cwd in &project.root_cwds {
                insert_unique_project_index(
                    &mut root_project_index_by_cwd,
                    root_cwd.clone(),
                    index,
                );
            }
            TaskBoardCatalogProject {
                cwd: project.cwd.clone(),
                label: project.label.clone(),
                session_count: 0,
            }
        })
        .collect::<Vec<_>>();

    let mut sessions = Vec::new();
    for session in local_catalog.sessions {
        let session_identity = normalized_session_identity(&session.id);
        let project_index = if let Some(project_id) = project_catalog
            .local_project_id_by_thread
            .get(&session_identity)
        {
            project_index_by_id.get(project_id).copied()
        } else if project_catalog
            .assigned_thread_ids
            .contains(&session_identity)
            || project_catalog
                .projectless_thread_ids
                .contains(&session_identity)
        {
            None
        } else {
            let Ok(session_cwd) = normalize_task_project_cwd(&session.cwd) else {
                continue;
            };
            unique_project_index(&primary_project_index_by_cwd, &session_cwd)
                .or_else(|| unique_project_index(&root_project_index_by_cwd, &session_cwd))
        };
        let Some(project_index) = project_index else {
            continue;
        };

        let updated_at_ms = task_board_timestamp_from_bridge_i64(session.updated_at_ms)
            .map_err(|_| TaskBoardCatalogBuildError::InvalidTimestamp)?;
        projects[project_index].session_count = projects[project_index]
            .session_count
            .checked_add(1)
            .ok_or(TaskBoardCatalogBuildError::ProjectSessionCountTooLarge)?;
        sessions.push(TaskBoardCatalogSession {
            session_id: session.id,
            title: session.title,
            cwd: projects[project_index].cwd.clone(),
            updated_at_ms,
        });
    }

    let warnings = local_catalog
        .warnings
        .into_iter()
        .map(|warning| match warning {
            LocalSessionCatalogWarning::DatabaseReadFailed { count } => {
                let count = u32::try_from(count)
                    .map_err(|_| TaskBoardCatalogBuildError::WarningCountTooLarge)?;
                Ok(TaskBoardCatalogWarning {
                    code: TaskBoardCatalogWarningCode::CodexDbReadFailed,
                    count,
                })
            }
        })
        .collect::<Result<Vec<_>, TaskBoardCatalogBuildError>>()?;

    Ok(TaskBoardSessionCatalog {
        projects,
        sessions,
        warnings,
    })
}

fn modern_project_catalog(
    state: &Value,
    local_projects: &Map<String, Value>,
) -> CodexProjectCatalog {
    let mut projects = Vec::new();
    for project_key in ordered_modern_project_keys(state, local_projects) {
        let Some(project) = local_projects.get(&project_key).and_then(Value::as_object) else {
            continue;
        };
        let mut root_cwds = project
            .get("rootPaths")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|path| normalize_task_project_cwd(path).ok())
            .collect::<Vec<_>>();
        dedupe_strings(&mut root_cwds);
        let Some(cwd) = root_cwds.first().cloned() else {
            continue;
        };
        let id =
            nonempty_string(project.get("id")).unwrap_or_else(|| project_key.trim().to_string());
        if id.is_empty() {
            continue;
        }
        let label =
            nonempty_string(project.get("name")).unwrap_or_else(|| project_label_from_path(&cwd));
        projects.push(CodexProject {
            id,
            label,
            cwd,
            root_cwds,
        });
    }
    CodexProjectCatalog {
        projects,
        ..CodexProjectCatalog::default()
    }
}

fn legacy_project_catalog(state: &Value) -> CodexProjectCatalog {
    let mut paths = Vec::new();
    append_path_array(&mut paths, state.get("electron-saved-workspace-roots"));
    append_path_array(&mut paths, state.get("project-order"));
    dedupe_strings(&mut paths);

    let labels = state
        .get("electron-workspace-root-labels")
        .and_then(Value::as_object);
    let projects = paths
        .into_iter()
        .map(|cwd| {
            let label = labels
                .and_then(|labels| {
                    labels
                        .iter()
                        .find(|(path, _)| {
                            normalize_task_project_cwd(path).ok().as_deref() == Some(cwd.as_str())
                        })
                        .and_then(|(_, value)| nonempty_string(Some(value)))
                })
                .unwrap_or_else(|| project_label_from_path(&cwd));
            CodexProject {
                id: cwd.clone(),
                label,
                cwd: cwd.clone(),
                root_cwds: vec![cwd],
            }
        })
        .collect();
    CodexProjectCatalog {
        projects,
        ..CodexProjectCatalog::default()
    }
}

fn ordered_modern_project_keys(state: &Value, local_projects: &Map<String, Value>) -> Vec<String> {
    let project_order = string_array(state.get("project-order"))
        .into_iter()
        .filter(|project_id| local_projects.contains_key(project_id))
        .collect::<Vec<_>>();
    let ordered_ids = project_order.iter().cloned().collect::<HashSet<_>>();
    let mut result = string_array(state.get("pinned-project-ids"))
        .into_iter()
        .filter(|project_id| {
            local_projects.contains_key(project_id) && !ordered_ids.contains(project_id)
        })
        .collect::<Vec<_>>();
    result.extend(project_order);
    let selected_ids = result.iter().cloned().collect::<HashSet<_>>();
    result.extend(
        local_projects
            .keys()
            .filter(|project_id| !selected_ids.contains(*project_id))
            .cloned(),
    );
    dedupe_strings(&mut result);
    result
}

fn populate_thread_project_state(state: &Value, catalog: &mut CodexProjectCatalog) {
    if let Some(assignments) = state
        .get("thread-project-assignments")
        .and_then(Value::as_object)
    {
        for (thread_id, assignment) in assignments {
            let thread_id = normalized_session_identity(thread_id);
            if thread_id.is_empty() {
                continue;
            }
            match assignment {
                Value::Object(assignment) => {
                    let project_id = nonempty_string(assignment.get("projectId"));
                    let project_kind = nonempty_string(assignment.get("projectKind"));
                    if project_id.is_some() || project_kind.is_some() {
                        catalog.assigned_thread_ids.insert(thread_id.clone());
                    }
                    if project_kind
                        .as_deref()
                        .is_none_or(|kind| kind.eq_ignore_ascii_case("local"))
                    {
                        if let Some(project_id) = project_id {
                            catalog
                                .local_project_id_by_thread
                                .insert(thread_id, project_id);
                        }
                    }
                }
                Value::String(project_id) if !project_id.trim().is_empty() => {
                    catalog.assigned_thread_ids.insert(thread_id.clone());
                    catalog
                        .local_project_id_by_thread
                        .insert(thread_id, project_id.trim().to_string());
                }
                _ => {}
            }
        }
    }

    catalog.projectless_thread_ids.extend(
        string_array(state.get("projectless-thread-ids"))
            .into_iter()
            .map(|thread_id| normalized_session_identity(&thread_id))
            .filter(|thread_id| !thread_id.is_empty()),
    );
}

fn append_path_array(paths: &mut Vec<String>, value: Option<&Value>) {
    paths.extend(
        string_array(value)
            .into_iter()
            .filter_map(|path| normalize_task_project_cwd(&path).ok()),
    );
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn insert_unique_project_index(
    indexes: &mut HashMap<String, Option<usize>>,
    cwd: String,
    project_index: usize,
) {
    indexes
        .entry(cwd)
        .and_modify(|existing| {
            if *existing != Some(project_index) {
                *existing = None;
            }
        })
        .or_insert(Some(project_index));
}

fn unique_project_index(indexes: &HashMap<String, Option<usize>>, cwd: &str) -> Option<usize> {
    indexes.get(cwd).copied().flatten()
}

fn normalized_session_identity(session_id: &str) -> String {
    let trimmed = session_id.trim();
    let without_local_prefix = if trimmed
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("local:"))
    {
        &trimmed[6..]
    } else {
        trimmed
    };
    without_local_prefix.to_ascii_lowercase()
}

fn project_label_from_path(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .find(|component| !component.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::storage::LocalSessionCatalogEntry;

    fn session(id: &str, cwd: &str, updated_at_ms: Option<i64>) -> LocalSessionCatalogEntry {
        LocalSessionCatalogEntry {
            id: id.to_string(),
            title: format!("Title {id}"),
            cwd: cwd.to_string(),
            model_provider: "ignored".to_string(),
            updated_at_ms,
        }
    }

    #[test]
    fn modern_projects_use_codex_names_and_explicit_thread_assignments() {
        let state = json!({
            "local-projects": {
                "project-a": {
                    "id": "project-a",
                    "name": "用户编辑后的名称",
                    "rootPaths": ["C:/Workspace/real-folder", "C:/Workspace/shared"]
                },
                "project-b": {
                    "id": "project-b",
                    "name": "第二个项目",
                    "rootPaths": ["D:/Projects/second"]
                }
            },
            "project-order": ["project-b", "project-a"],
            "thread-project-assignments": {
                "assigned-temp": {"projectKind": "local", "projectId": "project-a"},
                "assigned-second": {"projectKind": "local", "projectId": "project-b"},
                "assigned-removed": {"projectKind": "local", "projectId": "removed-project"}
            },
            "projectless-thread-ids": ["projectless"]
        });
        let local_catalog = LocalSessionCatalog {
            sessions: vec![
                session("assigned-temp", "C:/Recent/23", Some(50)),
                session("assigned-second", "D:/Projects/second", Some(40)),
                session("legacy-root-match", "C:/Workspace/real-folder", Some(30)),
                session("projectless", "C:/Workspace/real-folder", Some(20)),
                session("assigned-removed", "C:/Workspace/real-folder", Some(10)),
                session("unregistered", "C:/Recent/other", Some(5)),
            ],
            warnings: Vec::new(),
        };

        let catalog = task_board_catalog_from_local_catalog(
            local_catalog,
            codex_project_catalog_from_state(&state),
        )
        .unwrap();

        assert_eq!(
            catalog
                .projects
                .iter()
                .map(|project| (
                    project.cwd.as_str(),
                    project.label.as_str(),
                    project.session_count
                ))
                .collect::<Vec<_>>(),
            vec![
                ("D:\\projects\\second", "第二个项目", 1),
                ("C:\\workspace\\real-folder", "用户编辑后的名称", 2),
            ]
        );
        assert_eq!(
            catalog
                .sessions
                .iter()
                .map(|session| (session.session_id.as_str(), session.cwd.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("assigned-temp", "C:\\workspace\\real-folder"),
                ("assigned-second", "D:\\projects\\second"),
                ("legacy-root-match", "C:\\workspace\\real-folder"),
            ]
        );
    }

    #[test]
    fn an_explicit_empty_modern_project_map_does_not_restore_legacy_roots() {
        let state = json!({
            "local-projects": {},
            "electron-saved-workspace-roots": ["C:/stale/project"]
        });
        let catalog = task_board_catalog_from_local_catalog(
            LocalSessionCatalog {
                sessions: vec![session("stale", "C:/stale/project", Some(1))],
                warnings: Vec::new(),
            },
            codex_project_catalog_from_state(&state),
        )
        .unwrap();

        assert!(catalog.projects.is_empty());
        assert!(catalog.sessions.is_empty());
    }

    #[test]
    fn modern_catalog_keeps_projects_missing_from_project_order() {
        let state = json!({
            "local-projects": {
                "ordered": {
                    "id": "ordered",
                    "name": "Ordered",
                    "rootPaths": ["C:/Workspace/Ordered"]
                },
                "pinned-only": {
                    "id": "pinned-only",
                    "name": "Pinned Only",
                    "rootPaths": ["C:/Workspace/Pinned"]
                }
            },
            "project-order": ["ordered"],
            "pinned-project-ids": ["pinned-only"]
        });
        let catalog = task_board_catalog_from_local_catalog(
            LocalSessionCatalog {
                sessions: Vec::new(),
                warnings: Vec::new(),
            },
            codex_project_catalog_from_state(&state),
        )
        .unwrap();

        assert_eq!(
            catalog
                .projects
                .iter()
                .map(|project| project.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Pinned Only", "Ordered"]
        );
    }

    #[test]
    fn legacy_projects_use_saved_roots_and_custom_labels() {
        let state = json!({
            "electron-saved-workspace-roots": ["\\\\?\\C:\\Workspace\\Legacy"],
            "project-order": ["C:/Workspace/Legacy"],
            "electron-workspace-root-labels": {
                "C:/Workspace/Legacy": "旧版自定义名称"
            }
        });
        let catalog = task_board_catalog_from_local_catalog(
            LocalSessionCatalog {
                sessions: vec![
                    session("legacy", "c:/workspace/legacy", Some(1)),
                    session("recent", "C:/Recent/23", Some(2)),
                ],
                warnings: Vec::new(),
            },
            codex_project_catalog_from_state(&state),
        )
        .unwrap();

        assert_eq!(catalog.projects.len(), 1);
        assert_eq!(catalog.projects[0].cwd, "C:\\workspace\\legacy");
        assert_eq!(catalog.projects[0].label, "旧版自定义名称");
        assert_eq!(catalog.projects[0].session_count, 1);
        assert_eq!(catalog.sessions.len(), 1);
        assert_eq!(catalog.sessions[0].session_id, "legacy");
    }

    #[test]
    fn local_session_prefix_matches_codex_thread_assignment() {
        let state = json!({
            "local-projects": {
                "project": {
                    "id": "project",
                    "name": "Project",
                    "rootPaths": ["C:/Workspace/Project"]
                }
            },
            "thread-project-assignments": {
                "thread-id": {"projectKind": "local", "projectId": "project"}
            }
        });
        let catalog = task_board_catalog_from_local_catalog(
            LocalSessionCatalog {
                sessions: vec![session("LOCAL:THREAD-ID", "C:/Recent/23", Some(1))],
                warnings: Vec::new(),
            },
            codex_project_catalog_from_state(&state),
        )
        .unwrap();

        assert_eq!(catalog.sessions.len(), 1);
        assert_eq!(catalog.sessions[0].cwd, "C:\\workspace\\project");
    }

    #[test]
    fn project_catalog_loader_falls_back_to_the_backup_state() {
        let home = tempdir().unwrap();
        fs::write(home.path().join(CODEX_GLOBAL_STATE_FILE), "{invalid").unwrap();
        fs::write(
            home.path().join(CODEX_GLOBAL_STATE_BACKUP_FILE),
            json!({
                "local-projects": {
                    "project": {
                        "id": "project",
                        "name": "Backup Project",
                        "rootPaths": ["C:/Workspace/Backup"]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let project_catalog = load_codex_project_catalog(home.path()).unwrap();
        let catalog = task_board_catalog_from_local_catalog(
            LocalSessionCatalog {
                sessions: vec![session("session", "C:/Workspace/Backup", Some(1))],
                warnings: Vec::new(),
            },
            project_catalog,
        )
        .unwrap();

        assert_eq!(catalog.projects[0].label, "Backup Project");
        assert_eq!(catalog.sessions.len(), 1);
    }
}
