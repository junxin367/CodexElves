//! 独立远程服务的工作区打开能力，不依赖 Codex App、管理器或桌面桥接。
use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{ApiError, WebState};

#[cfg(windows)]
mod windows;

const TARGETS: &[(&str, &str)] = &[
    ("vscode", "VS Code"),
    ("visualStudio", "Visual Studio"),
    // 用户要求补齐本机 Codex 菜单中的 IDEA、WSL 和 PowerShell；前端应用 ID 列表须同步维护。
    ("intellij", "IntelliJ IDEA"),
    ("sublimeText", "Sublime Text"),
    ("githubDesktop", "GitHub Desktop"),
    ("fileManager", "File Explorer"),
    ("gitBash", "Git Bash"),
    ("cmder", "Cmder"),
    ("rider", "Rider"),
    ("powershell", "PowerShell"),
    ("wsl", "WSL"),
];

#[derive(Debug)]
struct Application {
    target: &'static str,
    label: &'static str,
    executable: PathBuf,
    wsl_session: Option<WslSession>,
    file: Option<FileTarget>,
}

#[derive(Debug)]
struct FileTarget {
    root: PathBuf,
    relative: String,
    requested: String,
}

#[derive(Debug, Clone)]
pub(super) struct WslSession {
    pub instance_id: String,
    pub distribution: String,
    pub user: String,
    pub linux_cwd: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub window_title: String,
}

struct LaunchOutcome {
    foreground: bool,
    warning: Option<String>,
}

fn identity(payload: &Value, opening: bool) -> Result<Value, ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "打开工作区参数无效"))?;
    // 用户要求正文文件复用 Open in；只允许会话/项目身份、文件引用路径和固定应用 ID，不能传 cwd、程序或命令参数。
    if object.keys().any(|key| {
        !matches!(key.as_str(), "threadId" | "projectId" | "path") && !(opening && key == "target")
    }) {
        return Err(ApiError::new(400, "只能通过会话或项目标识打开工作区"));
    }
    let mut context = object.clone();
    context.remove("target");
    requested_file(payload)?;
    super::workspace_identity(&Value::Object(context))
}

fn requested_file(payload: &Value) -> Result<Option<&str>, ApiError> {
    match payload.get("path") {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|path| {
                !path.trim().is_empty() && path.len() <= 16 * 1024 && !path.contains('\0')
            })
            .map(Some)
            .ok_or_else(|| ApiError::new(400, "文件路径无效")),
    }
}

fn file_reference_path(cwd: &Path, requested: &str) -> String {
    let path = requested.replace('\\', "/");
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return path;
    }
    // 文件树以 Git 根目录展开，但正文相对引用必须以会话 cwd 为起点；保留 .. 交由既有越界规则拒绝。
    let relative = path
        .split('/')
        .filter(|part| *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "{}/{relative}",
        display_path(cwd).trim_end_matches(['/', '\\'])
    )
}

async fn resolve_file(
    root: &Path,
    metadata: &Value,
    payload: &Value,
    authorization: &super::RemoteAuthorization,
) -> Result<Option<FileTarget>, ApiError> {
    let Some(requested) = requested_file(payload)? else {
        return Ok(None);
    };
    // 相对路径以当前会话工作目录解析；绝对路径及 WSL 映射沿用文件预览的项目/所属引擎范围，不允许越界。
    let resolved = file_reference_path(root, requested);
    let (root, relative) = super::workspace::resolve_file_target(
        root.to_owned(),
        metadata,
        &resolved,
        false,
        authorization,
    )
    .await?;
    let file = FileTarget {
        root,
        relative,
        requested: requested.to_owned(),
    };
    tokio::task::spawn_blocking(move || {
        super::workspace::open_in_file_path(&file.root, &file.relative)?;
        Ok(Some(file))
    })
    .await
    .map_err(|_| ApiError::new(503, "无法确认所选文件"))?
}

fn response(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

#[cfg(not(windows))]
fn applications(_: &Path, _: Option<&WslSession>) -> Vec<Application> {
    Vec::new()
}

#[cfg(windows)]
fn applications(root: &Path, wsl_session: Option<&WslSession>) -> Vec<Application> {
    TARGETS
        .iter()
        .filter_map(|&(target, label)| {
            // 用户要求 WSL 菜单只属于实际由 WSL 实例执行且可恢复的当前对话，不能靠项目目录或本机装有 wsl.exe 判定。
            if target == "wsl" && wsl_session.is_none() {
                return None;
            }
            windows::find_application(target, root).map(|executable| Application {
                target,
                label,
                executable,
                wsl_session: (target == "wsl").then(|| wsl_session.cloned()).flatten(),
                file: None,
            })
        })
        .collect()
}

#[cfg(windows)]
fn application_icon(application: &Application) -> Option<String> {
    // 用户要求 Git 与 WSL 显示已安装程序自身图标；仅传 PNG 数据，不公开安装路径。
    matches!(application.target, "gitBash" | "wsl")
        .then(|| windows::icon_data(&application.executable))
        .flatten()
}

#[cfg(not(windows))]
fn application_icon(_: &Application) -> Option<String> {
    None
}

async fn wsl_session(
    state: &WebState,
    identity: &Value,
    metadata: &Value,
) -> Result<Option<WslSession>, ApiError> {
    let Some(thread_id) = identity["threadId"].as_str() else {
        return Ok(None);
    };
    // 当前只有 Claude 的 WSL 执行适配器已就绪；Codex 本机实例或仅存放在 WSL 目录的项目不得展示此入口。
    if metadata["backend"] != "claude"
        || !metadata["instanceId"]
            .as_str()
            .is_some_and(|id| id.starts_with("claude:wsl:"))
    {
        return Ok(None);
    }
    let session = state
        .controller
        .claude
        .open_in_session(thread_id)
        .await
        .map_err(|error| ApiError::new(409, &format!("无法确认 WSL 会话：{error}")))?;
    if let Some(session) = &session
        && (metadata["instanceId"] != session.instance_id
            || metadata["wslDistribution"] != session.distribution
            || metadata["wslUser"] != session.user
            || metadata["linuxCwd"] != session.linux_cwd)
    {
        return Err(ApiError::new(409, "WSL 会话实例已变化，请重新打开菜单"));
    }
    Ok(session)
}

pub(super) async fn targets(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state.controller.authorization(device, None);
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    let identity = identity(&payload, false)?;
    let (root, metadata) = super::workspace_file_context(&state, &identity, &authorization).await?;
    let file = resolve_file(&root, &metadata, &payload, &authorization).await?;
    let wsl_session = wsl_session(&state, &identity, &metadata).await?;
    let result = tokio::task::spawn_blocking(move || {
        let targets: Vec<_> = applications(&root, wsl_session.as_ref())
            .into_iter()
            .map(|app| {
                let mut value =
                    json!({"target": app.target, "label": app.label, "available": true});
                if let Some(icon) = application_icon(&app) {
                    value["icon"] = json!(icon);
                }
                value
            })
            .collect();
        // 只公开应用 ID、名称、程序图标 PNG 和原生确认的目录，不向网页暴露安装路径。
        let mut value = json!({"ok": true, "root": display_path(&root), "targets": targets});
        if let Some(file) = file {
            value["path"] = json!(file.requested);
        }
        value
    })
    .await
    .map_err(|_| ApiError::new(503, "无法检测可用的工作区应用"))?;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(response(result))
}

pub(super) async fn open(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state
        .controller
        .authorization(device, Some("openWorkspace"));
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    validate_open(&payload)?;
    let request_id = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let result = state
        .controller
        .execute(
            authorization.device.clone(),
            request_id,
            "openWorkspace".into(),
            payload,
        )
        .await?;
    Ok(response(result))
}

fn validate_open(payload: &Value) -> Result<Value, ApiError> {
    let identity = identity(payload, true)?;
    if !payload["target"]
        .as_str()
        .is_some_and(|target| TARGETS.iter().any(|(id, _)| *id == target))
    {
        return Err(ApiError::new(400, "请选择可用的工作区应用"));
    }
    Ok(identity)
}

pub(super) async fn dispatch(
    payload: Value,
    authorization: &super::RemoteAuthorization,
) -> Result<Value, ApiError> {
    let identity = validate_open(&payload)?;
    // 这里只复用目录解析；启动权限始终由原请求的 authorization 和实时策略复核。
    let state = WebState {
        controller: authorization.controller.clone(),
        settings: Default::default(),
    };
    let target = payload["target"]
        .as_str()
        .filter(|target| TARGETS.iter().any(|(id, _)| id == target))
        .ok_or_else(|| ApiError::new(400, "请选择可用的工作区应用"))?
        .to_owned();
    let (root, metadata) = super::workspace_file_context(&state, &identity, authorization).await?;
    let file = resolve_file(&root, &metadata, &payload, authorization).await?;
    let wsl_session = wsl_session(&state, &identity, &metadata).await?;
    let (root, application) = tokio::task::spawn_blocking(move || {
        let mut application = applications(&root, wsl_session.as_ref())
            .into_iter()
            .find(|app| app.target == target)
            .ok_or_else(|| ApiError::new(409, "该应用已不可用，请重新选择打开方式"))?;
        application.file = file;
        Ok::<_, ApiError>((root, application))
    })
    .await
    .map_err(|_| ApiError::new(503, "无法确认工作区应用"))??;
    let mut result = json!({"ok": true, "root": display_path(&root), "target": application.target});
    if let Some(file) = &application.file {
        result["path"] = json!(file.requested);
    }
    let permit = authorization
        .prepare_dispatch()
        .await
        .map_err(super::authorization_error)?;
    // 与原注入功能一致，启动前重新检测应用、复核授权；只在服务所在电脑启动一次，不复用 Codex App。
    let outcome = tokio::task::spawn_blocking(move || {
        permit.commit(|| launch(&application, &root).map_err(anyhow::Error::from))
    })
    .await
    .map_err(|_| ApiError::new(503, "无法启动工作区应用"))?
    .map_err(super::authorization_error)?;
    // 用户要求打开后显示在前台；启动成功与窗口激活分别回执，激活失败不能触发重复启动。
    result["foreground"] = json!(outcome.foreground);
    if let Some(warning) = outcome.warning {
        result["warning"] = json!(warning);
    }
    Ok(result)
}

fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}

#[cfg(windows)]
fn launch(application: &Application, root: &Path) -> Result<LaunchOutcome, ApiError> {
    windows::launch(application, root)
}

#[cfg(not(windows))]
fn launch(_: &Application, _: &Path) -> Result<LaunchOutcome, ApiError> {
    Err(ApiError::new(422, "当前服务系统不支持打开工作区应用"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_open_in_accepts_only_identity_path_and_known_target() {
        let payload = json!({"threadId":"main","path":"src/app.ts","target":"vscode"});
        assert_eq!(validate_open(&payload).unwrap(), json!({"threadId":"main"}));
        assert_eq!(requested_file(&payload).unwrap(), Some("src/app.ts"));
        for payload in [
            json!({"threadId":"main","path":null,"target":"vscode"}),
            json!({"threadId":"main","path":"","target":"vscode"}),
            json!({"threadId":"main","path":"src/app.ts","target":"unknown"}),
            json!({"threadId":"main","path":"src/app.ts","target":"vscode","cwd":"E:/other"}),
            json!({"threadId":"main","path":"src/app.ts","target":"vscode","arguments":["--execute"]}),
        ] {
            assert!(validate_open(&payload).is_err(), "{payload}");
        }
        assert!(validate_open(&json!({"threadId":"main","target":"vscode"})).is_ok());
    }

    #[test]
    fn relative_file_references_are_based_on_session_cwd_instead_of_git_root() {
        let cwd = Path::new(r"E:\repo\packages\client");
        assert_eq!(
            file_reference_path(cwd, "src/app.ts").replace('\\', "/"),
            "E:/repo/packages/client/src/app.ts"
        );
        assert_eq!(
            file_reference_path(cwd, "./README.md").replace('\\', "/"),
            "E:/repo/packages/client/README.md"
        );
        assert_eq!(
            file_reference_path(cwd, r"src\app.ts").replace('\\', "/"),
            "E:/repo/packages/client/src/app.ts"
        );
        assert_eq!(
            file_reference_path(cwd, "/home/junes/app.ts"),
            "/home/junes/app.ts"
        );
        assert!(
            file_reference_path(cwd, "../private.txt").contains("/../"),
            "不能抹掉 .. 绕过现有文件边界检查"
        );
    }
}
