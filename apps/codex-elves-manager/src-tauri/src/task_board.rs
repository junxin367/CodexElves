use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    FileTaskBoardStore, TaskBoardAttachConversationsCommand, TaskBoardCatalogProject,
    TaskBoardCatalogSession, TaskBoardCatalogWarning, TaskBoardCatalogWarningCode,
    TaskBoardConversation, TaskBoardCreateCommand, TaskBoardDetachConversationsCommand,
    TaskBoardDocument, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardSessionCatalog, TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
    normalize_task_project_cwd, task_board_timestamp_from_bridge_i64,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl, WindowEvent};

const TASK_BOARD_WINDOW_STATE_FILE: &str = "task-board-window-state.json";
const DEV_TASK_BOARD_WINDOW_STATE_FILE: &str = "task-board-window-state-dev.json";
const TASK_BOARD_WAKE_SHOW: u8 = 1;
const TASK_BOARD_WAKE_ACK: &[u8] = b"codex-elves-task-board:shown\n";
const TASK_BOARD_CONTROL_TIMEOUT: Duration = Duration::from_millis(750);
const DEFAULT_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 860.0;
const MIN_WINDOW_WIDTH: u32 = 840;
const MIN_WINDOW_HEIGHT: u32 = 680;
const MIN_VISIBLE_EDGE: i64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskBoardWindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTaskRequest {
    task_id: String,
    expected_revision: u64,
    title: String,
    project: TaskBoardProject,
    session_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachConversationsRequest {
    task_id: String,
    expected_revision: u64,
    session_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetachConversationsRequest {
    task_id: String,
    expected_revision: u64,
    session_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveTaskRequest {
    task_id: String,
    to_status: TaskBoardStatus,
    target_index: u32,
    expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenSessionRequest {
    session_id: String,
    title: String,
    cwd: String,
    updated_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartConversationRequest {
    project: TaskBoardProject,
    first_instruction: String,
    #[serde(default)]
    model_id: String,
    #[serde(default)]
    effort_id: String,
}

pub fn run() {
    install_panic_logger();
    let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
        "task_board_app.start",
        json!({
            "version": env!("CARGO_PKG_VERSION")
        }),
    );
    let Some(guard) = acquire_single_instance_guard() else {
        return;
    };
    let wake_listener = guard.try_clone_listener().ok().flatten();

    let run_result = tauri::Builder::default()
        .setup(move |app| {
            let restore_state = load_window_state()
                .map(clamp_window_state)
                .filter(|state| window_state_is_visible(&app.handle(), state));
            let mut builder =
                tauri::WebviewWindowBuilder::new(app, "task-board", task_board_webview_url()?)
                    .title(task_board_window_title())
                    .inner_size(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
                    .min_inner_size(f64::from(MIN_WINDOW_WIDTH), f64::from(MIN_WINDOW_HEIGHT))
                    .visible(false)
                    .data_directory(task_board_webview_data_directory())
                    .disable_drag_drop_handler();
            if restore_state.is_none() {
                builder = builder.center();
            }
            let window = builder.build()?;
            if let Some(state) = restore_state {
                apply_window_state(&window, state);
            }
            register_window_events(window.clone());
            if let Some(listener) = wake_listener {
                spawn_wake_listener(listener, app.handle().clone());
            }
            window.show()?;
            window.set_focus()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            task_board_load_snapshot,
            task_board_load_catalog,
            task_board_create_task,
            task_board_attach_conversations,
            task_board_detach_conversations,
            task_board_move_task,
            task_board_open_session,
            task_board_probe_host,
            task_board_load_create_options,
            task_board_start_conversation,
        ])
        .run(tauri::generate_context!());

    drop(guard);
    if let Err(error) = run_result {
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "task_board_app.run_failed",
            json!({
                "error": error.to_string()
            }),
        );
    }
}

#[tauri::command]
pub async fn task_board_load_snapshot() -> Value {
    match tauri::async_runtime::spawn_blocking(|| {
        FileTaskBoardStore::from_default_paths().snapshot()
    })
    .await
    {
        Ok(Ok(document)) => snapshot_value(document),
        Ok(Err(error)) => store_error_value(error),
        Err(_) => failed("task_board_unavailable", "任务看板存储暂不可用"),
    }
}

#[tauri::command]
pub async fn task_board_load_catalog() -> Value {
    match tauri::async_runtime::spawn_blocking(load_session_catalog).await {
        Ok(Ok(catalog)) => catalog_value(catalog),
        Ok(Err(error)) => failed("session_catalog_unavailable", error.to_string()),
        Err(_) => failed("session_catalog_unavailable", "会话目录暂不可用"),
    }
}

#[tauri::command]
pub async fn task_board_create_task(request: CreateTaskRequest) -> Value {
    mutate_with_catalog(request.session_ids.clone(), move |conversations| {
        FileTaskBoardStore::from_default_paths().create_task(TaskBoardCreateCommand {
            task_id: request.task_id,
            expected_revision: request.expected_revision,
            title: request.title,
            project: request.project,
            conversations,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_attach_conversations(request: AttachConversationsRequest) -> Value {
    mutate_with_catalog(request.session_ids.clone(), move |conversations| {
        FileTaskBoardStore::from_default_paths().attach_conversations(
            TaskBoardAttachConversationsCommand {
                task_id: request.task_id,
                expected_revision: request.expected_revision,
                conversations,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn task_board_detach_conversations(request: DetachConversationsRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().detach_conversations(
            TaskBoardDetachConversationsCommand {
                task_id: request.task_id,
                expected_revision: request.expected_revision,
                session_ids: request.session_ids,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn task_board_move_task(request: MoveTaskRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().move_task(TaskBoardMoveCommand {
            task_id: request.task_id,
            to_status: request.to_status,
            target_index: request.target_index,
            expected_revision: request.expected_revision,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_open_session(request: OpenSessionRequest) -> Value {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return failed("invalid_input", "会话 ID 不能为空");
    }
    call_codex_host(
        "openSession",
        json!([
            session_id,
            {
                "sessionId": request.session_id,
                "title": request.title,
                "cwd": request.cwd,
                "updatedAtMs": request.updated_at_ms
            }
        ]),
    )
    .await
}

#[tauri::command]
pub async fn task_board_probe_host(project: TaskBoardProject) -> Value {
    call_codex_host("probe", json!([project])).await
}

#[tauri::command]
pub async fn task_board_load_create_options() -> Value {
    call_codex_host("createOptions", json!([])).await
}

#[tauri::command]
pub async fn task_board_start_conversation(request: StartConversationRequest) -> Value {
    if request.first_instruction.trim().is_empty() {
        return failed("invalid_input", "请输入首条指令");
    }
    call_codex_host(
        "startConversation",
        json!([
            request.project,
            request.first_instruction,
            request.model_id,
            request.effort_id
        ]),
    )
    .await
}

async fn mutate_with_catalog(
    session_ids: Vec<String>,
    mutation: impl FnOnce(
        Vec<TaskBoardConversation>,
    ) -> Result<TaskBoardMutationResult, TaskBoardStoreError>
    + Send
    + 'static,
) -> Value {
    match tauri::async_runtime::spawn_blocking(move || {
        let catalog =
            load_session_catalog().map_err(|error| TaskBoardStoreError::InvalidInput {
                message: error.to_string(),
            })?;
        let conversations = conversations_from_catalog(&catalog, &session_ids)?;
        mutation(conversations)
    })
    .await
    {
        Ok(Ok(result)) => mutation_value(result),
        Ok(Err(error)) => store_error_value(error),
        Err(_) => failed("task_board_unavailable", "任务看板存储暂不可用"),
    }
}

async fn mutate_store(
    mutation: impl FnOnce() -> Result<TaskBoardMutationResult, TaskBoardStoreError> + Send + 'static,
) -> Value {
    match tauri::async_runtime::spawn_blocking(mutation).await {
        Ok(Ok(result)) => mutation_value(result),
        Ok(Err(error)) => store_error_value(error),
        Err(_) => failed("task_board_unavailable", "任务看板存储暂不可用"),
    }
}

fn conversations_from_catalog(
    catalog: &TaskBoardSessionCatalog,
    session_ids: &[String],
) -> Result<Vec<TaskBoardConversation>, TaskBoardStoreError> {
    if session_ids.is_empty() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "至少选择一个会话".to_string(),
        });
    }
    let mut sessions = HashMap::new();
    for session in &catalog.sessions {
        sessions.insert(session.session_id.to_ascii_lowercase(), session);
        sessions.insert(
            session
                .session_id
                .trim_start_matches("local:")
                .to_ascii_lowercase(),
            session,
        );
    }
    let mut seen = HashSet::new();
    let mut conversations = Vec::with_capacity(session_ids.len());
    for raw_id in session_ids {
        let session_id = raw_id.trim();
        let key = session_id.trim_start_matches("local:").to_ascii_lowercase();
        if session_id.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let Some(session) = sessions.get(&key).copied() else {
            return Err(TaskBoardStoreError::InvalidInput {
                message: format!("会话不存在或尚未写入本地目录：{session_id}"),
            });
        };
        conversations.push(TaskBoardConversation {
            session_id: session.session_id.clone(),
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            updated_at_ms: session.updated_at_ms,
        });
    }
    if conversations.is_empty() {
        return Err(TaskBoardStoreError::InvalidInput {
            message: "至少选择一个有效会话".to_string(),
        });
    }
    Ok(conversations)
}

fn load_session_catalog() -> anyhow::Result<TaskBoardSessionCatalog> {
    let db_paths = candidate_codex_db_paths();
    let local_catalog = codex_elves_data::aggregate_local_session_catalog(&db_paths)
        .map_err(|_| anyhow::anyhow!("无法读取 Codex 本地会话目录"))?;
    catalog_from_local_catalog(local_catalog)
}

fn candidate_codex_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in codex_elves_core::codex_sqlite::codex_session_db_paths_from_home(
        &codex_elves_core::codex_sqlite::default_codex_home_dir(),
    ) {
        if !paths.iter().any(|candidate| candidate == &path) {
            paths.push(path);
        }
    }
    let default = codex_elves_core::codex_sqlite::codex_session_db_path();
    if !paths.iter().any(|candidate| candidate == &default) {
        paths.push(default);
    }
    paths
}

fn catalog_from_local_catalog(
    local_catalog: codex_elves_data::LocalSessionCatalog,
) -> anyhow::Result<TaskBoardSessionCatalog> {
    let mut projects: Vec<TaskBoardCatalogProject> = Vec::new();
    let mut project_indexes: HashMap<String, usize> = HashMap::new();
    let mut sessions = Vec::new();

    for session in local_catalog.sessions {
        let Ok(cwd) = normalize_task_project_cwd(&session.cwd) else {
            continue;
        };
        let updated_at_ms = task_board_timestamp_from_bridge_i64(session.updated_at_ms)
            .map_err(|_| anyhow::anyhow!("会话目录包含无效时间戳"))?;
        if let Some(index) = project_indexes.get(&cwd).copied() {
            projects[index].session_count = projects[index]
                .session_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("项目会话数量超出支持范围"))?;
        } else {
            project_indexes.insert(cwd.clone(), projects.len());
            projects.push(TaskBoardCatalogProject {
                label: project_label(&cwd),
                cwd: cwd.clone(),
                session_count: 1,
            });
        }
        sessions.push(TaskBoardCatalogSession {
            session_id: session.id,
            title: session.title,
            cwd,
            updated_at_ms,
        });
    }

    let warnings = local_catalog
        .warnings
        .into_iter()
        .map(|warning| match warning {
            codex_elves_data::LocalSessionCatalogWarning::DatabaseReadFailed { count } => {
                Ok(TaskBoardCatalogWarning {
                    code: TaskBoardCatalogWarningCode::CodexDbReadFailed,
                    count: u32::try_from(count)
                        .map_err(|_| anyhow::anyhow!("数据库失败数量超出支持范围"))?,
                })
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(TaskBoardSessionCatalog {
        projects,
        sessions,
        warnings,
    })
}

fn project_label(cwd: &str) -> String {
    cwd.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .find(|component| !component.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

async fn call_codex_host(method: &str, arguments: Value) -> Value {
    let latest = match StatusStore::default().load_latest() {
        Ok(Some(latest)) if latest.status == "running" => latest,
        _ => return failed("codex_unavailable", "Codex 当前未通过 CodexElves 运行"),
    };
    let Some(debug_port) = latest.debug_port else {
        return failed("codex_unavailable", "Codex 调试端口不可用");
    };
    let targets = match codex_elves_core::cdp::list_targets(debug_port).await {
        Ok(targets) => targets,
        Err(_) => return failed("codex_unavailable", "无法连接 Codex 页面"),
    };
    let target = match codex_elves_core::cdp::pick_injectable_codex_page_target(&targets) {
        Ok(target) => target,
        Err(_) => return failed("codex_unavailable", "未找到可用的 Codex 主页面"),
    };
    let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
        return failed("codex_unavailable", "Codex 页面调试通道不可用");
    };
    let method_json = serde_json::to_string(method).unwrap_or_else(|_| "\"\"".to_string());
    let arguments_json = serde_json::to_string(&arguments).unwrap_or_else(|_| "[]".to_string());
    let script = format!(
        r#"
(() => {{
  const method = {method_json};
  const args = {arguments_json};
  const host = window.__codexElvesTaskBoardHost;
  if (!host || typeof host[method] !== "function") {{
    return {{ status: "failed", code: "host_unavailable", message: "Codex 任务看板宿主接口尚未就绪" }};
  }}
  return Promise.resolve(host[method](...args))
    .then((result) => JSON.stringify(result ?? null))
    .catch((error) => JSON.stringify({{
      status: "failed",
      code: "host_action_failed",
      message: String(error?.message || error || "宿主动作失败"),
    }}));
}})()
"#
    );
    match codex_elves_core::bridge::evaluate_script_with_await_promise(websocket_url, &script, true)
        .await
    {
        Ok(response) => runtime_evaluate_value(&response)
            .unwrap_or_else(|| failed("host_action_failed", "Codex 宿主动作没有返回有效结果")),
        Err(_) => failed("host_action_failed", "Codex 宿主动作执行失败"),
    }
}

fn runtime_evaluate_value(response: &Value) -> Option<Value> {
    if response.pointer("/result/exceptionDetails").is_some() {
        return None;
    }
    let value = response
        .pointer("/result/result/value")
        .cloned()
        .or_else(|| response.pointer("/result/value").cloned())?;
    match value {
        Value::String(serialized) => serde_json::from_str(&serialized).ok(),
        value => Some(value),
    }
}

fn snapshot_value(document: TaskBoardDocument) -> Value {
    json!({
        "status": "ok",
        "schemaVersion": document.schema_version,
        "revision": document.revision,
        "tasks": document.tasks,
    })
}

fn catalog_value(catalog: TaskBoardSessionCatalog) -> Value {
    json!({
        "status": "ok",
        "projects": catalog.projects,
        "sessions": catalog.sessions,
        "warnings": catalog.warnings,
    })
}

fn mutation_value(result: TaskBoardMutationResult) -> Value {
    let mut value = snapshot_value(result.document);
    if let Some(object) = value.as_object_mut() {
        object.insert("changed".to_string(), json!(result.changed));
        object.insert("idempotent".to_string(), json!(result.idempotent));
    }
    value
}

fn store_error_value(error: TaskBoardStoreError) -> Value {
    match error {
        TaskBoardStoreError::Busy => failed("task_board_busy", "任务看板存储正忙，请稍后重试"),
        TaskBoardStoreError::InvalidFile { path, message } => json!({
            "status": "failed",
            "code": "task_file_invalid",
            "message": format!("任务看板文件无效：{message}"),
            "path": path.to_string_lossy(),
        }),
        TaskBoardStoreError::InvalidInput { message } => failed("invalid_input", message),
        TaskBoardStoreError::RevisionConflict { current } => json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "任务看板已发生变化，请重试",
            "schemaVersion": current.schema_version,
            "revision": current.revision,
            "tasks": current.tasks,
        }),
        TaskBoardStoreError::TaskIdConflict => failed("task_id_conflict", "任务 ID 与现有任务冲突"),
        TaskBoardStoreError::ProjectMismatch => {
            failed("project_mismatch", "只能关联属于同一项目的会话")
        }
        TaskBoardStoreError::TaskNotFound => failed("task_not_found", "任务不存在"),
        TaskBoardStoreError::Unavailable { .. } => {
            failed("task_board_unavailable", "任务看板存储暂不可用")
        }
    }
}

fn failed(code: &str, message: impl Into<String>) -> Value {
    json!({
        "status": "failed",
        "code": code,
        "message": message.into(),
    })
}

fn task_board_webview_url() -> anyhow::Result<WebviewUrl> {
    if task_board_dev_mode() {
        return Ok(WebviewUrl::External(tauri::Url::parse(
            "http://localhost:1420/?taskBoard=1",
        )?));
    }
    Ok(WebviewUrl::App("index.html?taskBoard=1".into()))
}

fn task_board_window_title() -> &'static str {
    if task_board_dev_mode() {
        "CodexElves 任务看板 Dev"
    } else {
        "CodexElves 任务看板"
    }
}

fn task_board_dev_mode() -> bool {
    std::env::var("CODEX_ELVES_TASK_BOARD_DEV")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn task_board_webview_data_directory() -> PathBuf {
    std::env::var_os("CODEX_ELVES_TASK_BOARD_WEBVIEW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            codex_elves_core::paths::default_app_state_dir().join("task-board-webview2")
        })
}

fn window_state_path() -> PathBuf {
    std::env::var_os("CODEX_ELVES_TASK_BOARD_WINDOW_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            codex_elves_core::paths::default_app_state_dir().join(if task_board_dev_mode() {
                DEV_TASK_BOARD_WINDOW_STATE_FILE
            } else {
                TASK_BOARD_WINDOW_STATE_FILE
            })
        })
}

fn load_window_state() -> Option<TaskBoardWindowState> {
    serde_json::from_slice(&std::fs::read(window_state_path()).ok()?).ok()
}

fn save_window_state(state: TaskBoardWindowState) {
    let path = window_state_path();
    if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, bytes);
    }
}

fn clamp_window_state(state: TaskBoardWindowState) -> TaskBoardWindowState {
    TaskBoardWindowState {
        width: state.width.max(MIN_WINDOW_WIDTH),
        height: state.height.max(MIN_WINDOW_HEIGHT),
        ..state
    }
}

fn persist_window_state<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if matches!(window.is_minimized(), Ok(true)) || matches!(window.is_fullscreen(), Ok(true)) {
        return;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    save_window_state(clamp_window_state(TaskBoardWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }));
}

fn apply_window_state<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    state: TaskBoardWindowState,
) {
    let state = clamp_window_state(state);
    let _ = window.set_size(Size::Physical(PhysicalSize::new(state.width, state.height)));
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(state.x, state.y)));
}

fn window_state_is_visible<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &TaskBoardWindowState,
) -> bool {
    app.available_monitors()
        .map(|monitors| {
            monitors.iter().any(|monitor| {
                let area = monitor.work_area();
                state_intersects_work_area(
                    state,
                    area.position.x,
                    area.position.y,
                    area.size.width,
                    area.size.height,
                )
            })
        })
        .unwrap_or(false)
}

fn state_intersects_work_area(
    state: &TaskBoardWindowState,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
) -> bool {
    let left = i64::from(state.x).max(i64::from(monitor_x));
    let top = i64::from(state.y).max(i64::from(monitor_y));
    let right = (i64::from(state.x) + i64::from(state.width))
        .min(i64::from(monitor_x) + i64::from(monitor_width));
    let bottom = (i64::from(state.y) + i64::from(state.height))
        .min(i64::from(monitor_y) + i64::from(monitor_height));
    right - left >= MIN_VISIBLE_EDGE && bottom - top >= MIN_VISIBLE_EDGE
}

fn register_window_events<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) {
    let moved_window = window.clone();
    let resized_window = window.clone();
    let closing_window = window.clone();
    let app_handle = window.app_handle().clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) => persist_window_state(&moved_window),
        WindowEvent::Resized(_) => persist_window_state(&resized_window),
        WindowEvent::CloseRequested { .. } => persist_window_state(&closing_window),
        WindowEvent::Destroyed => app_handle.exit(0),
        _ => {}
    });
}

fn acquire_single_instance_guard() -> Option<codex_elves_core::ports::LoopbackPortGuard> {
    let port = codex_elves_core::ports::task_board_guard_port();
    match codex_elves_core::ports::acquire_resilient_loopback_port_guard(port) {
        Ok(guard) => Some(guard),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::AddrInUse
            ) =>
        {
            let delivered = request_existing_window_to_show(port);
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "task_board_app.already_running",
                json!({
                    "guard_port": port,
                    "wake_delivered": delivered,
                }),
            );
            None
        }
        Err(error) => {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "task_board_app.guard_failed",
                json!({
                    "guard_port": port,
                    "error": error.to_string(),
                }),
            );
            None
        }
    }
}

fn spawn_wake_listener<R: tauri::Runtime>(
    listener: std::net::TcpListener,
    app_handle: tauri::AppHandle<R>,
) {
    let _ = std::thread::Builder::new()
        .name("codex-elves-task-board-wake".to_string())
        .spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else {
                    continue;
                };
                let _ = stream.set_read_timeout(Some(TASK_BOARD_CONTROL_TIMEOUT));
                let mut command = [0_u8; 1];
                if stream.read_exact(&mut command).is_err() || command[0] != TASK_BOARD_WAKE_SHOW {
                    continue;
                }
                show_window(&app_handle);
                let _ = stream.write_all(TASK_BOARD_WAKE_ACK);
                let _ = stream.flush();
            }
        });
}

fn request_existing_window_to_show(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, TASK_BOARD_CONTROL_TIMEOUT)
        .and_then(|mut stream| {
            stream.set_read_timeout(Some(TASK_BOARD_CONTROL_TIMEOUT))?;
            stream.set_write_timeout(Some(TASK_BOARD_CONTROL_TIMEOUT))?;
            stream.write_all(&[TASK_BOARD_WAKE_SHOW])?;
            stream.flush()?;
            let mut ack = vec![0_u8; TASK_BOARD_WAKE_ACK.len()];
            stream.read_exact(&mut ack)?;
            Ok(ack == TASK_BOARD_WAKE_ACK)
        })
        .unwrap_or(false)
}

fn show_window<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    if let Some(window) = app_handle.get_webview_window("task-board") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
            "task_board_app.panic",
            json!({
                "message": panic_info.to_string(),
            }),
        );
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_window_requires_a_stable_visible_area() {
        let visible = TaskBoardWindowState {
            x: 100,
            y: 100,
            width: 1000,
            height: 800,
        };
        let almost_hidden = TaskBoardWindowState {
            x: 1910,
            y: 1070,
            width: 1000,
            height: 800,
        };

        assert!(state_intersects_work_area(&visible, 0, 0, 1920, 1080));
        assert!(!state_intersects_work_area(
            &almost_hidden,
            0,
            0,
            1920,
            1080
        ));
    }

    #[test]
    fn window_state_clamps_only_minimum_size_without_moving_user_position() {
        let state = clamp_window_state(TaskBoardWindowState {
            x: -720,
            y: 48,
            width: 400,
            height: 300,
        });

        assert_eq!(state.x, -720);
        assert_eq!(state.y, 48);
        assert_eq!(state.width, MIN_WINDOW_WIDTH);
        assert_eq!(state.height, MIN_WINDOW_HEIGHT);
    }

    #[test]
    fn runtime_evaluate_value_extracts_awaited_host_result() {
        let response = json!({
            "result": {
                "result": {
                    "type": "string",
                    "value": "{\"status\":\"ok\",\"sessionId\":\"session-1\"}"
                }
            }
        });

        assert_eq!(
            runtime_evaluate_value(&response),
            Some(json!({"status": "ok", "sessionId": "session-1"}))
        );
    }

    #[test]
    fn javascript_safe_revision_limit_is_preserved() {
        assert!(codex_elves_core::task_board::TASK_BOARD_MAX_SAFE_INTEGER > 0);
    }
}
