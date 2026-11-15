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
    ("sublimeText", "Sublime Text"),
    ("githubDesktop", "GitHub Desktop"),
    ("fileManager", "File Explorer"),
    ("gitBash", "Git Bash"),
    ("cmder", "Cmder"),
    ("rider", "Rider"),
];

#[derive(Debug)]
struct Application {
    target: &'static str,
    label: &'static str,
    executable: PathBuf,
}

fn identity(payload: &Value, opening: bool) -> Result<Value, ApiError> {
    let object = payload
        .as_object()
        .ok_or_else(|| ApiError::new(400, "打开工作区参数无效"))?;
    // 用户要求打开当前会话目录；客户端只能提交身份和固定应用 ID，不能传 cwd、程序路径或命令参数。
    if object.keys().any(|key| {
        !matches!(key.as_str(), "threadId" | "projectId") && !(opening && key == "target")
    }) {
        return Err(ApiError::new(400, "只能通过会话或项目标识打开工作区"));
    }
    let mut context = object.clone();
    context.remove("target");
    super::workspace_identity(&Value::Object(context))
}

fn response(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

#[cfg(not(windows))]
fn applications(_: &Path) -> Vec<Application> {
    Vec::new()
}

#[cfg(windows)]
fn applications(root: &Path) -> Vec<Application> {
    TARGETS
        .iter()
        .filter_map(|&(target, label)| {
            windows::find_application(target, root).map(|executable| Application {
                target,
                label,
                executable,
            })
        })
        .collect()
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
    let root = super::workspace_context(&state, &identity, &authorization).await?;
    let result = tokio::task::spawn_blocking(move || {
        let targets: Vec<_> = applications(&root)
            .into_iter()
            .map(|app| json!({"target": app.target, "label": app.label, "available": true}))
            .collect();
        // 只公开应用 ID、名称和原生确认的目录，不向网页暴露安装路径。
        json!({"ok": true, "root": display_path(&root), "targets": targets})
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
    let root = super::workspace_context(&state, &identity, authorization).await?;
    let (root, application) = tokio::task::spawn_blocking(move || {
        let application = applications(&root)
            .into_iter()
            .find(|app| app.target == target)
            .ok_or_else(|| ApiError::new(409, "该应用已不可用，请重新选择打开方式"))?;
        Ok::<_, ApiError>((root, application))
    })
    .await
    .map_err(|_| ApiError::new(503, "无法确认工作区应用"))??;
    let result = json!({"ok": true, "root": display_path(&root), "target": application.target});
    let permit = authorization
        .prepare_dispatch()
        .await
        .map_err(super::authorization_error)?;
    // 与原注入功能一致，启动前重新检测应用、复核授权；只在服务所在电脑启动一次，不复用 Codex App。
    tokio::task::spawn_blocking(move || {
        permit.commit(|| launch(&application, &root).map_err(anyhow::Error::from))
    })
    .await
    .map_err(|_| ApiError::new(503, "无法启动工作区应用"))?
    .map_err(super::authorization_error)?;
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
fn launch(application: &Application, root: &Path) -> Result<(), ApiError> {
    windows::launch(application, root)
}

#[cfg(not(windows))]
fn launch(_: &Application, _: &Path) -> Result<(), ApiError> {
    Err(ApiError::new(422, "当前服务系统不支持打开工作区应用"))
}
