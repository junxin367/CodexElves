//! 会话类型注册表与运行环境实例；连接方式是实例能力，不是会话类型开关。
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::preferences::Environment;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionType {
    pub id: &'static str,
    pub label: &'static str,
    pub command: &'static str,
    pub detect_local: bool,
    pub local_adapter: bool,
    pub wsl_adapter: bool,
    pub instruction_file: &'static str,
}

// 用户要求未来可扩展多种类型；检测、分组和指令入口共用注册表，适配器就绪情况单独声明。
pub(super) const TYPES: &[SessionType] = &[
    SessionType {
        id: "codex",
        label: "Codex",
        command: "codex",
        detect_local: true,
        local_adapter: true,
        wsl_adapter: false,
        instruction_file: "AGENTS.md",
    },
    SessionType {
        id: "claude",
        label: "Claude",
        command: "claude",
        // 用户要求暂不识别本机 Claude；仅暂停本机检测，WSL 检测与执行能力独立保留。
        detect_local: false,
        local_adapter: false,
        wsl_adapter: true,
        instruction_file: "CLAUDE.md",
    },
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Instance {
    pub id: String,
    pub backend: String,
    pub label: String,
    pub environment_id: String,
    pub distribution: String,
    pub user: String,
    pub executable: String,
    pub ready: bool,
    pub status: String,
    pub legacy_default: bool,
    pub connection_modes: Vec<&'static str>,
}

pub(super) fn id(backend: &str, distribution: &str, user: &str) -> String {
    if distribution.is_empty() {
        return format!("{backend}:local");
    }
    // 实例身份固定为类型、发行版和用户；升级可执行文件或重启进程不能重置开关或串用草稿。
    let identity = serde_json::to_vec(&(distribution.to_lowercase(), user)).unwrap();
    let digest = format!("{:x}", Sha256::digest(identity));
    format!("{backend}:wsl:{}", &digest[..24])
}

pub(super) fn label(backend: &str, distribution: &str, user: &str) -> String {
    // 用户要求实例名不拼接“本机”“WSL”标签，保留实际发行版；Claude 继续不附加 Linux 用户名。
    let kind = TYPES.iter().find(|kind| kind.id == backend);
    let mut parts = vec![kind.map_or(backend, |kind| kind.label)];
    if !distribution.is_empty() {
        parts.push(distribution);
        if backend != "claude" && !user.is_empty() {
            parts.push(user);
        }
    }
    parts.join(" · ")
}

pub(super) fn catalog(environments: &[Environment]) -> Vec<Instance> {
    environments
        .iter()
        .flat_map(|environment| {
            TYPES.iter().filter_map(move |kind| {
                let local = environment.distribution.is_empty();
                // 暂停识别的本机类型不展示旧检测结果，也不清除客户端已保存的实例偏好。
                if local && !kind.detect_local {
                    return None;
                }
                let executable = environment
                    .executables
                    .get(kind.id)
                    .cloned()
                    .unwrap_or_default();
                // 本机 Codex 保留固定实例标识；CLI 的实际发现和连接结果由会话服务报告。
                if executable.is_empty() && !(local && kind.id == "codex") {
                    return None;
                }
                let ready = environment.error.is_none()
                    && if local {
                        kind.local_adapter
                    } else {
                        kind.wsl_adapter
                    };
                Some(Instance {
                    id: id(kind.id, &environment.distribution, &environment.user),
                    backend: kind.id.into(),
                    label: label(kind.id, &environment.distribution, &environment.user),
                    environment_id: environment.id.clone(),
                    distribution: environment.distribution.clone(),
                    user: environment.user.clone(),
                    executable,
                    ready,
                    status: if ready {
                        "可接入"
                    } else {
                        "已检测，执行接入尚未支持"
                    }
                    .into(),
                    legacy_default: local && kind.id == "codex"
                        || environment.configured && kind.id == "claude",
                    // 用户要求 Codex 实例仅保留服务模式；接口不能再宣告桌面注入，未接入的实例保持无可用连接方式。
                    connection_modes: if ready && kind.id == "codex" {
                        vec!["service"]
                    } else if ready {
                        vec!["native"]
                    } else {
                        vec![]
                    },
                })
            })
        })
        .collect()
}

fn codex_row(value: &mut Value) {
    if let Some(row) = value.as_object_mut() {
        row.entry("backend").or_insert(json!("codex"));
        row.entry("instanceId").or_insert(json!("codex:local"));
        row.entry("instanceLabel")
            .or_insert_with(|| json!(label("codex", "", "")));
    }
}

pub(super) fn annotate_codex(value: &mut Value) {
    if let Some(thread) = value.get_mut("thread").filter(|thread| thread.is_object()) {
        codex_row(thread);
    }
    // 会话元数据只能补充已有列表/详情；写回执不能插入空字段，否则破坏幂等核对的原始结构。
    for key in ["recent", "pinnedThreads"] {
        for row in value
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            codex_row(row);
        }
    }
    for project in value
        .get_mut("projects")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        for row in project
            .get_mut("threads")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            codex_row(row);
        }
    }
}

pub(super) fn filter_sidebar(value: &mut Value, ids: &[String]) {
    for key in ["recent", "pinnedThreads"] {
        if let Some(rows) = value.get_mut(key).and_then(Value::as_array_mut) {
            rows.retain(|row| {
                row["instanceId"]
                    .as_str()
                    .is_some_and(|id| ids.iter().any(|enabled| enabled == id))
            });
        }
    }
    for project in value
        .get_mut("projects")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(rows) = project.get_mut("threads").and_then(Value::as_array_mut) {
            rows.retain(|row| {
                row["instanceId"]
                    .as_str()
                    .is_some_and(|id| ids.iter().any(|enabled| enabled == id))
            });
        }
    }
}
