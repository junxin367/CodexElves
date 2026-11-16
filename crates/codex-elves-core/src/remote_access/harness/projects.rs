use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::RemoteAuthorization;
use super::{Harness, error, required};

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct SavedProject {
    name: String,
    #[serde(rename = "rootPaths")]
    roots: Vec<String>,
    #[serde(rename = "createdAt")]
    created_at: u64,
    #[serde(rename = "updatedAt")]
    updated_at: u64,
}

// 用户要求 App 添加的项目及置顶排序同样生效：只反序列化同一 Codex Home 的项目展示字段，
// 不连接 App、不读账号/队列/审批/内存快照，也不写回 App 文件；执行仍由独立 app-server 完成。
#[derive(Default, Deserialize)]
#[serde(default)]
struct SavedPreferences {
    #[serde(rename = "local-projects")]
    projects: BTreeMap<String, SavedProject>,
    #[serde(rename = "project-order")]
    order: Vec<String>,
    #[serde(rename = "pinned-project-ids")]
    pinned: Option<Vec<String>>,
    #[serde(rename = "app-server-project-id-by-legacy-project-id-by-host")]
    aliases_by_host: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, Default)]
pub(super) struct ProjectPreferences {
    saved: BTreeMap<String, SavedProject>,
    aliases: BTreeMap<String, String>,
    order: Vec<String>,
    pinned: Option<Vec<String>>,
}

impl ProjectPreferences {
    pub(super) fn load(home: &Path) -> anyhow::Result<Self> {
        let bytes = match std::fs::read(home.join(".codex-global-state.json")) {
            Ok(bytes) => bytes,
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(failure) => return Err(failure).context("读取 Codex App 项目记录失败"),
        };
        let saved: SavedPreferences =
            serde_json::from_slice(&bytes).context("Codex App 项目记录格式无效")?;
        let aliases = saved
            .aliases_by_host
            .into_iter()
            .find(|(host, _)| {
                host.strip_prefix("local:")
                    .is_some_and(|path| same_path(path, &home.to_string_lossy()))
            })
            .map(|(_, aliases)| aliases)
            .unwrap_or_default();
        Ok(Self {
            saved: saved.projects,
            aliases,
            order: saved.order,
            pinned: saved.pinned,
        })
    }

    pub(super) fn merge(&self, native: Vec<Value>) -> Vec<Value> {
        let mut seen = BTreeSet::new();
        let mut projects: Vec<_> = native
            .into_iter()
            .filter_map(|mut project| {
                let id = project["id"].as_str()?.to_owned();
                if id.is_empty() || !seen.insert(id.clone()) {
                    return None;
                }
                let roots: Vec<_> = project["roots"]
                    .as_array()?
                    .iter()
                    .filter_map(|root| root["path"].as_str().map(str::to_owned))
                    .filter(|root| !root.is_empty())
                    .collect();
                project["serverProjectId"] = json!(id);
                initialize_project(&mut project, roots);
                Some(project)
            })
            .collect();
        for (desktop_id, saved) in &self.saved {
            let roots: Vec<_> = saved
                .roots
                .iter()
                .filter(|root| !root.is_empty())
                .cloned()
                .collect();
            if desktop_id.is_empty() || roots.is_empty() {
                continue;
            }
            let explicit = projects.iter().position(|project| {
                project["id"] == *desktop_id
                    || self
                        .aliases
                        .get(desktop_id)
                        .is_some_and(|id| project["id"] == *id)
            });
            // 迁移映射尚未落盘时，只在完整根目录集合唯一相同时去重；同名或仅共享一个根不能合并。
            let matching: Vec<_> = projects
                .iter()
                .enumerate()
                .filter(|(_, project)| {
                    project["serverProjectId"].is_string() && same_roots(&roots, &project["roots"])
                })
                .map(|(index, _)| index)
                .collect();
            let saved_matches = self
                .saved
                .values()
                .filter(|candidate| same_roots(&roots, &json!(candidate.roots)))
                .count();
            let index = explicit
                .or_else(|| (matching.len() == 1 && saved_matches == 1).then(|| matching[0]));
            if let Some(index) = index {
                let project = &mut projects[index];
                project["desktopProjectId"] = json!(desktop_id);
                if !saved.name.is_empty() {
                    project["name"] = json!(saved.name);
                }
            } else {
                // App 中已保存但尚未迁入引擎的项目也必须出现；不能用历史会话 cwd 合成项目。
                let mut project = json!({"id":desktop_id,"desktopProjectId":desktop_id,
                    "serverProjectId":null,"name":saved.name,"createdAt":saved.created_at,
                    "updatedAt":saved.updated_at});
                initialize_project(&mut project, roots);
                projects.push(project);
            }
        }
        projects
    }

    pub(super) fn revision(&self) -> Option<String> {
        self.pinned.as_ref().map(|pinned| {
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&json!([pinned, self.order])).unwrap())
            )
        })
    }

    pub(super) fn pin_revision(&self, project: &Value) -> Option<String> {
        self.revision().filter(|_| self.follows_desktop(project))
    }

    fn follows_desktop(&self, project: &Value) -> bool {
        let ids = project_ids(project);
        project["desktopProjectId"].is_string()
            || self
                .order
                .iter()
                .chain(self.pinned.iter().flatten())
                .any(|id| ids.contains(&id.as_str()))
    }

    pub(super) fn apply_order(
        &self,
        projects: &mut [Value],
        pins: &BTreeSet<String>,
        overrides: &BTreeMap<String, bool>,
        revisions: &BTreeMap<String, String>,
    ) {
        let revision = self.revision();
        for project in projects.iter_mut() {
            let ids = project_ids(project);
            let desktop_pin = self
                .pinned
                .as_ref()
                .filter(|_| self.follows_desktop(project))
                .map(|pinned| pinned.iter().any(|id| ids.contains(&id.as_str())));
            let local = ids
                .iter()
                .find_map(|id| overrides.get(*id).map(|value| (*id, *value)));
            // App 后续置顶、取消或重排必须生效；远程显式覆盖只对操作时的 App 排序快照有效，
            // 没有快照的旧远程记录不能永久遮住 App 的置顶。远程独有项目继续沿用原偏好。
            let pinned = match (desktop_pin, local) {
                (Some(base), Some((id, value))) => {
                    if revisions.get(id) == revision.as_ref() {
                        value
                    } else {
                        base
                    }
                }
                (Some(base), None) => base,
                (None, Some((_, value))) => value,
                (None, None) => ids.iter().any(|id| pins.contains(*id)),
            };
            project["pinned"] = json!(pinned);
        }
        projects.sort_by_key(|project| {
            let pinned = project["pinned"] == true;
            let order = if pinned {
                self.pinned.as_deref().unwrap_or_default()
            } else {
                &self.order
            };
            let ids = project_ids(project);
            (
                !pinned,
                order
                    .iter()
                    .position(|id| ids.contains(&id.as_str()))
                    .unwrap_or(usize::MAX),
            )
        });
    }
}

fn initialize_project(project: &mut Value, roots: Vec<String>) {
    project["cwd"] = roots.first().map(|root| json!(root)).unwrap_or(Value::Null);
    project["roots"] = json!(roots);
    project["threads"] = json!([]);
    project["backend"] = json!("codex");
    project["instanceId"] = json!("codex:local");
}

fn project_ids(project: &Value) -> Vec<&str> {
    ["id", "serverProjectId", "desktopProjectId"]
        .into_iter()
        .filter_map(|key| project[key].as_str())
        .collect()
}

fn same_roots(roots: &[String], other: &Value) -> bool {
    let Some(other) = other.as_array() else {
        return false;
    };
    let normalize = |path: &str| {
        let path = path.trim_end_matches(['/', '\\']).replace('\\', "/");
        if cfg!(windows) {
            path.to_ascii_lowercase()
        } else {
            path
        }
    };
    let roots: BTreeSet<_> = roots.iter().map(|root| normalize(root)).collect();
    let other: BTreeSet<_> = other
        .iter()
        .filter_map(Value::as_str)
        .map(normalize)
        .collect();
    !roots.is_empty() && roots == other
}

impl Harness {
    fn project_home(&self) -> std::path::PathBuf {
        #[cfg(test)]
        {
            self.project_codex_home.clone()
        }
        #[cfg(not(test))]
        {
            crate::codex_home::default_codex_home_dir()
        }
    }

    pub(super) async fn project_preferences(&self) -> anyhow::Result<ProjectPreferences> {
        let home = self.project_home();
        let read_home = home.clone();
        let preferences =
            tokio::task::spawn_blocking(move || ProjectPreferences::load(&read_home)).await??;
        *self.project_preferences.lock().await = Some((home, preferences.clone()));
        Ok(preferences)
    }

    pub(super) async fn sidebar_project_preferences(&self) -> (ProjectPreferences, Option<String>) {
        match self.project_preferences().await {
            Ok(preferences) => (preferences, None),
            Err(failure) => (
                self.project_preferences
                    .lock()
                    .await
                    .as_ref()
                    .filter(|(home, _)| *home == self.project_home())
                    .map(|(_, preferences)| preferences.clone())
                    .unwrap_or_default(),
                Some(format!(
                    "Codex App 项目和置顶排序同步失败，暂保留已有项目记录：{failure:#}"
                )),
            ),
        }
    }

    pub(super) async fn creation_project(
        &self,
        id: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<(String, String)> {
        self.runtime.invalidate_read("project/list").await;
        let context = self.workspace_context(&json!({"projectId":id})).await?;
        let cwd = required(&context, "cwd")?.to_owned();
        if let Some(server_id) = context["serverProjectId"].as_str() {
            return Ok((cwd, server_id.to_owned()));
        }
        // 只有用户明确发送新会话才把 App 独有项目登记到独立引擎；列表和文件读取绝不写入。
        // project/import 的稳定幂等键避免并发新建生成重复项目，不借用 App 的连接或执行上下文。
        let roots: Vec<_> = context["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|path| json!({"path":path}))
            .collect();
        let imported = self.runtime.request("project/import", json!({
            "idempotencyKey":format!("codex-elves:app-project:{}",context["projectId"].as_str().unwrap_or(id)),
            "name":context["name"],"roots":roots
        }), Some(authorization)).await?;
        let server_id = imported["project"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| error(503, "原生项目登记未返回有效项目 ID"))?
            .to_owned();
        let expected_roots: Vec<_> = context["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let actual_roots: Vec<_> = imported["project"]["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|root| root["path"].as_str())
            .collect();
        if !same_roots(&expected_roots, &json!(actual_roots)) {
            return Err(error(
                409,
                "原生项目目录与当前 App 项目不一致，未创建会话，请刷新项目后重试",
            ));
        }
        self.runtime.invalidate_read("project/list").await;
        Ok((cwd, server_id))
    }
}

pub(super) fn assigned_index(thread: &Value, projects: &[Value]) -> Option<usize> {
    // 项目显示可继承 App 记录，但会话明确的原生归属（包括 null）仍优先，不能擅自移动会话。
    if let Some(project) = thread.get("projectId") {
        return project.as_str().and_then(|id| {
            projects
                .iter()
                .position(|project| project_ids(project).contains(&id))
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences(state: Value) -> ProjectPreferences {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(".codex-global-state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        ProjectPreferences::load(directory.path()).unwrap()
    }

    fn ids(projects: &[Value]) -> Vec<&str> {
        projects
            .iter()
            .map(|project| project["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn app_projects_include_empty_saved_entries_without_merging_names_or_partial_roots() {
        let desktop = preferences(json!({
            "local-projects":{
                "app-a":{"name":"App 名称","rootPaths":["/repo/a"]},
                "app-only":{"name":"同名","rootPaths":["/repo/only","/shared"]},
                "app-other":{"name":"同名","rootPaths":["/repo/other","/shared"]},
                "invalid":{"name":"缺少目录","rootPaths":[]}
            },
            "pinned-project-ids":["app-only"],
            "project-order":["app-other","app-a"],
            "unrelated-account-state":{"mustNotReachProjects":"opaque"}
        }));
        let mut projects = desktop.merge(vec![
            json!({"id":"native-a","name":"原生名称","roots":[{"path":"/repo/a/"}]}),
            json!({"id":"native-extra","name":"同名","roots":[{"path":"/repo/extra"},{"path":"/shared"}]})
        ]);
        desktop.apply_order(
            &mut projects,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(
            ids(&projects),
            ["app-only", "app-other", "native-a", "native-extra"]
        );
        assert_eq!(projects[0]["serverProjectId"], Value::Null);
        assert_eq!(projects[2]["desktopProjectId"], "app-a");
        assert_eq!(projects[2]["name"], "App 名称");
        assert!(
            projects
                .iter()
                .all(|project| project["instanceId"] == "codex:local"
                    && project["threads"] == json!([])
                    && project["synthetic"] != true)
        );
        assert!(!serde_json::to_string(&projects).unwrap().contains("opaque"));
    }

    #[test]
    fn app_project_aliases_are_scoped_to_the_configured_home() {
        let directory = tempfile::tempdir().unwrap();
        let state = json!({
            "local-projects":{"app":{"name":"App 项目","rootPaths":["/old"]}},
            "app-server-project-id-by-legacy-project-id-by-host":{
                format!("local:{}",directory.path().display()):{"app":"native"},
                "local:/another-home":{"app":"foreign"}
            }
        });
        std::fs::write(
            directory.path().join(".codex-global-state.json"),
            state.to_string(),
        )
        .unwrap();
        let desktop = ProjectPreferences::load(directory.path()).unwrap();
        let projects = desktop.merge(vec![
            json!({"id":"native","roots":[{"path":"/new"}]}),
            json!({"id":"foreign","roots":[{"path":"/foreign"}]}),
        ]);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0]["desktopProjectId"], "app");
        assert!(projects[1]["desktopProjectId"].is_null());
        assert_eq!(
            projects[0]["cwd"], "/new",
            "已迁移项目的执行目录仍取原生引擎"
        );
        assert_eq!(
            assigned_index(&json!({"projectId":"app"}), &projects),
            Some(0)
        );
        assert_eq!(
            assigned_index(&json!({"projectId":null,"cwd":"/new"}), &projects),
            None
        );
    }

    #[test]
    fn ambiguous_equal_root_projects_keep_their_distinct_ids() {
        let desktop = preferences(json!({"local-projects":{
            "app-a":{"name":"甲","rootPaths":["/same"]},
            "app-b":{"name":"乙","rootPaths":["/same"]}
        }}));
        let projects = desktop.merge(vec![json!({"id":"native","roots":[{"path":"/same"}]})]);
        assert_eq!(ids(&projects), ["native", "app-a", "app-b"]);
        assert_eq!(assigned_index(&json!({"cwd":"/same"}), &projects), None);
    }

    #[test]
    fn app_pin_order_and_unpin_take_effect_despite_legacy_remote_overrides() {
        let mut desktop = preferences(json!({
            "local-projects":{
                "a":{"name":"甲","rootPaths":["/a"]},
                "b":{"name":"乙","rootPaths":["/b"]},
                "c":{"name":"丙","rootPaths":["/c"]}
            },
            "pinned-project-ids":["b","a"],"project-order":["c"]
        }));
        let pins = BTreeSet::from(["a".into(), "native".into()]);
        let overrides = BTreeMap::from([("b".into(), false)]);
        let mut projects = desktop.merge(vec![json!({"id":"native","roots":[{"path":"/native"}]})]);
        desktop.apply_order(&mut projects, &pins, &overrides, &BTreeMap::new());
        assert_eq!(ids(&projects), ["b", "a", "native", "c"]);
        let revisions = BTreeMap::from([("b".into(), desktop.revision().unwrap())]);
        desktop.apply_order(&mut projects, &pins, &overrides, &revisions);
        assert_eq!(
            projects
                .iter()
                .find(|project| project["id"] == "b")
                .unwrap()["pinned"],
            false
        );
        desktop.pinned = Some(vec!["a".into(), "b".into()]);
        desktop.apply_order(&mut projects, &pins, &overrides, &revisions);
        assert_eq!(&ids(&projects)[..2], ["a", "b"]);
        desktop.pinned = Some(vec![]);
        desktop.order = vec!["c".into(), "b".into(), "a".into()];
        desktop.apply_order(&mut projects, &pins, &overrides, &revisions);
        assert_eq!(ids(&projects), ["native", "c", "b", "a"]);
        assert!(
            projects[1..]
                .iter()
                .all(|project| project["pinned"] == false)
        );
    }

    #[tokio::test]
    async fn app_project_read_failures_preserve_the_last_successful_snapshot_and_report_reason() {
        let directory = tempfile::tempdir().unwrap();
        let service = Harness::new(directory.path()).unwrap();
        let path = directory.path().join(".codex-global-state.json");
        std::fs::write(
            &path,
            json!({"local-projects":{
                "app":{"name":"保留项目","rootPaths":["/repo"]}
            }})
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            service
                .sidebar_project_preferences()
                .await
                .0
                .merge(vec![])
                .len(),
            1
        );
        std::fs::write(&path, "{broken").unwrap();
        let (cached, warning) = service.sidebar_project_preferences().await;
        assert_eq!(cached.merge(vec![])[0]["name"], "保留项目");
        assert!(warning.unwrap().contains("项目记录格式无效"));
        assert!(
            service.project_preferences().await.is_err(),
            "实际写入不能使用过期项目配置"
        );
    }

    #[tokio::test]
    #[ignore = "只读核对本机 App 项目与独立原生引擎，需显式运行"]
    async fn installed_engine_app_projects_read_only_smoke() {
        let directory = tempfile::Builder::new()
            .prefix("app-project-readonly-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp"))
            .unwrap();
        let mut service = Harness::new(directory.path()).unwrap();
        std::sync::Arc::get_mut(&mut service)
            .unwrap()
            .project_codex_home = crate::codex_home::default_codex_home_dir();
        let desktop = service.project_preferences().await.unwrap();
        let native = service
            .list_all("project/list", json!({"limit":100}))
            .await
            .unwrap();
        let expected = desktop.merge(native.clone());
        let sidebar = service.sidebar().await.unwrap();
        let projects = sidebar["projects"].as_array().unwrap();
        assert_eq!(projects.len(), expected.len());
        assert!(
            native
                .iter()
                .all(|native| projects.iter().any(|project| project["id"] == native["id"]))
        );
        for saved in desktop.saved.keys() {
            assert!(
                projects
                    .iter()
                    .any(|project| project_ids(project).contains(&saved.as_str()))
            );
        }
        let expected_pins = desktop.pinned.as_deref().unwrap_or_default();
        let actual_pins: Vec<_> = projects
            .iter()
            .filter(|project| project["pinned"] == true)
            .map(|project| {
                project["desktopProjectId"]
                    .as_str()
                    .unwrap_or(project["id"].as_str().unwrap())
            })
            .collect();
        assert_eq!(
            actual_pins,
            expected_pins.iter().map(String::as_str).collect::<Vec<_>>()
        );
        println!(
            "HARNESS_APP_PROJECTS {}",
            json!({"readOnly":true,"nativeProjects":native.len(),
            "appSavedProjects":desktop.saved.len(),"displayedProjects":projects.len(),
            "pinnedProjects":actual_pins.len(),
            "pinnedNames":projects.iter().filter(|project|project["pinned"]==true).map(|project|&project["name"]).collect::<Vec<_>>()})
        );
        service.release().await;
    }
}
