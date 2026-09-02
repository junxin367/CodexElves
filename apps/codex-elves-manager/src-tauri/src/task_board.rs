use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use codex_elves_core::models::SessionRef;
use codex_elves_core::status::StatusStore;
use codex_elves_core::task_board::{
    FileTaskBoardStore, TaskBoardAttachConversationsCommand, TaskBoardConversation,
    TaskBoardCreateBoardCommand, TaskBoardCreateCommand, TaskBoardDeleteBoardCommand,
    TaskBoardDeleteCommand, TaskBoardDetachConversationsCommand, TaskBoardDocument,
    TaskBoardMoveBoardCommand, TaskBoardMoveCommand, TaskBoardMutationResult, TaskBoardProject,
    TaskBoardRenameBoardCommand, TaskBoardRenameTaskCommand, TaskBoardSessionCatalog,
    TaskBoardStatus, TaskBoardStore, TaskBoardStoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl, WindowEvent};

const TASK_BOARD_WINDOW_STATE_FILE: &str = "task-board-window-state.json";
const DEV_TASK_BOARD_WINDOW_STATE_FILE: &str = "task-board-window-state-dev.json";
const TASK_BOARD_ICON_PNG: &[u8] = include_bytes!("../icons/task-board.png");
const TASK_BOARD_WAKE_SHOW: u8 = 1;
const TASK_BOARD_WAKE_ACK: &[u8] = b"codex-elves-task-board:shown\n";
const TASK_BOARD_CONTROL_TIMEOUT: Duration = Duration::from_millis(750);
const TASK_BOARD_HOST_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const TASK_BOARD_HOST_OPERATION_POLL_DELAY: Duration = Duration::from_millis(350);
const TASK_BOARD_HOST_OPERATION_MAX_CONSECUTIVE_POLL_FAILURES: u32 = 3;
const TASK_BOARD_HOST_OPERATION_RETENTION_MS: u64 = 5 * 60 * 1_000;
const TASK_BOARD_HOST_OPERATION_MAX_ENTRIES: usize = 32;
const TASK_BOARD_MIN_HOST_VERSION: u64 = 3;
const TASK_BOARD_MIN_RUNTIME_VERSION: u64 = 62;
const TASK_BOARD_MIN_CONVERSATION_STATUS_RUNTIME_VERSION: u64 = 58;
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
pub struct DeleteTaskRequest {
    task_id: String,
    expected_revision: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameTaskRequest {
    task_id: String,
    expected_revision: u64,
    title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateBoardRequest {
    board_id: TaskBoardStatus,
    expected_revision: u64,
    label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteBoardRequest {
    board_id: TaskBoardStatus,
    expected_revision: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameBoardRequest {
    board_id: TaskBoardStatus,
    expected_revision: u64,
    label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveBoardRequest {
    board_id: TaskBoardStatus,
    target_index: u32,
    expected_revision: u64,
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
    #[serde(default)]
    project_label: String,
    updated_at_ms: Option<u64>,
    #[serde(default)]
    session_aliases: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationStatusRef {
    session_id: String,
    title: String,
    #[serde(default)]
    session_aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationStatusesRequest {
    conversations: Vec<ConversationStatusRef>,
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
    let task_board_icon = match task_board_window_icon() {
        Ok(icon) => icon,
        Err(error) => {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "task_board_app.icon_failed",
                json!({
                    "error": error.to_string()
                }),
            );
            return;
        }
    };
    let mut context = tauri::generate_context!();
    context.set_default_window_icon(Some(task_board_icon.clone()));

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
                    .disable_drag_drop_handler()
                    .icon(task_board_icon.clone())?;
            if restore_state.is_none() {
                builder = builder.center();
            }
            let window = builder.build()?;
            set_task_board_windows_taskbar_icon(&window)?;
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
            task_board_delete_task,
            task_board_rename_task,
            task_board_create_board,
            task_board_delete_board,
            task_board_rename_board,
            task_board_move_board,
            task_board_move_task,
            task_board_open_session,
            task_board_probe_host,
            task_board_load_create_options,
            task_board_load_host_appearance,
            task_board_load_conversation_statuses,
            task_board_start_conversation,
        ])
        .run(context);

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

fn task_board_window_icon() -> tauri::Result<tauri::image::Image<'static>> {
    Ok(tauri::image::Image::from_bytes(TASK_BOARD_ICON_PNG)?.to_owned())
}

#[cfg(windows)]
fn set_task_board_windows_taskbar_icon<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> anyhow::Result<()> {
    const WM_SETICON: u32 = 0x0080;
    const WM_GETICON: u32 = 0x007f;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SendMessageW(
            window: *mut std::ffi::c_void,
            message: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize;
    }

    let window = window.hwnd()?;
    let small_icon = unsafe { SendMessageW(window.0, WM_GETICON, ICON_SMALL, 0) };
    anyhow::ensure!(
        small_icon != 0,
        "task-board window icon was not available for the Windows taskbar"
    );
    unsafe {
        SendMessageW(window.0, WM_SETICON, ICON_BIG, small_icon);
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_task_board_windows_taskbar_icon<R: tauri::Runtime>(
    _window: &tauri::WebviewWindow<R>,
) -> anyhow::Result<()> {
    Ok(())
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
pub async fn task_board_delete_task(request: DeleteTaskRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().delete_task(TaskBoardDeleteCommand {
            task_id: request.task_id,
            expected_revision: request.expected_revision,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_rename_task(request: RenameTaskRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().rename_task(TaskBoardRenameTaskCommand {
            task_id: request.task_id,
            expected_revision: request.expected_revision,
            title: request.title,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_create_board(request: CreateBoardRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().create_board(TaskBoardCreateBoardCommand {
            board_id: request.board_id,
            expected_revision: request.expected_revision,
            label: request.label,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_delete_board(request: DeleteBoardRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().delete_board(TaskBoardDeleteBoardCommand {
            board_id: request.board_id,
            expected_revision: request.expected_revision,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_rename_board(request: RenameBoardRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().rename_board(TaskBoardRenameBoardCommand {
            board_id: request.board_id,
            expected_revision: request.expected_revision,
            label: request.label,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_board_move_board(request: MoveBoardRequest) -> Value {
    mutate_store(move || {
        FileTaskBoardStore::from_default_paths().move_board(TaskBoardMoveBoardCommand {
            board_id: request.board_id,
            target_index: request.target_index,
            expected_revision: request.expected_revision,
        })
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
    let arguments = task_board_open_session_arguments(&request, &session_id);
    call_codex_host("openSession", arguments).await
}

fn task_board_open_session_arguments(request: &OpenSessionRequest, session_id: &str) -> Value {
    json!([
        session_id,
        {
            "sessionId": request.session_id,
            "title": request.title,
            "cwd": request.cwd,
            "projectLabel": request.project_label,
            "updatedAtMs": request.updated_at_ms,
            "sessionAliases": request.session_aliases,
        }
    ])
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
pub async fn task_board_load_host_appearance() -> Value {
    call_codex_host("appearance", json!([])).await
}

#[tauri::command]
pub async fn task_board_load_conversation_statuses(request: ConversationStatusesRequest) -> Value {
    let conversations = normalized_conversation_status_refs(request);
    let host_arguments = task_board_conversation_status_arguments(&conversations);
    let local_conversations = conversations.clone();
    let local_statuses = tauri::async_runtime::spawn_blocking(move || {
        task_board_local_conversation_statuses(&local_conversations, candidate_codex_db_paths())
    });
    let host_statuses = call_codex_host_with_min_runtime(
        "conversationStatuses",
        host_arguments,
        TASK_BOARD_MIN_CONVERSATION_STATUS_RUNTIME_VERSION,
    );
    let (host_statuses, local_statuses) = tokio::join!(host_statuses, local_statuses);
    let local_statuses = local_statuses.unwrap_or_else(|_| {
        conversations
            .iter()
            .map(|conversation| task_board_unknown_conversation_status(conversation, false))
            .collect()
    });
    task_board_merge_conversation_statuses(&conversations, host_statuses, local_statuses)
}

fn normalized_conversation_status_refs(
    request: ConversationStatusesRequest,
) -> Vec<ConversationStatusRef> {
    let mut seen = HashSet::new();
    request
        .conversations
        .into_iter()
        .filter_map(|conversation| {
            let session_id = conversation.session_id.trim().to_string();
            let key = task_board_session_id_key(&session_id);
            if session_id.is_empty() || !seen.insert(key) {
                return None;
            }
            let mut seen_aliases = HashSet::new();
            let session_aliases = conversation
                .session_aliases
                .into_iter()
                .filter_map(|alias| {
                    let alias = alias.trim().to_string();
                    let alias_key = task_board_session_id_key(&alias);
                    if alias.is_empty()
                        || alias_key == task_board_session_id_key(&session_id)
                        || !seen_aliases.insert(alias_key)
                    {
                        return None;
                    }
                    Some(alias)
                })
                .collect();
            Some(ConversationStatusRef {
                session_id,
                title: conversation.title,
                session_aliases,
            })
        })
        .take(256)
        .collect()
}

fn task_board_conversation_status_arguments(conversations: &[ConversationStatusRef]) -> Value {
    json!([conversations])
}

fn task_board_local_conversation_statuses(
    conversations: &[ConversationStatusRef],
    db_paths: Vec<PathBuf>,
) -> Vec<Value> {
    // 会话状态查询是 IO 密集型；会话多时串行磁盘读取会放大看板刷新耗时。
    const MAX_LOCAL_STATUS_THREADS: usize = 8;
    if conversations.len() <= 1 || MAX_LOCAL_STATUS_THREADS <= 1 {
        return conversations
            .iter()
            .map(|conversation| task_board_local_conversation_status(conversation, &db_paths))
            .collect();
    }
    let chunk_size =
        (conversations.len() + MAX_LOCAL_STATUS_THREADS - 1) / MAX_LOCAL_STATUS_THREADS;
    let joined_chunks = std::thread::scope(|scope| {
        let db_paths = &db_paths;
        conversations
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|conversation| {
                            task_board_local_conversation_status(conversation, &db_paths)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join())
            .collect::<Vec<_>>()
    });
    let mut statuses = Vec::with_capacity(conversations.len());
    let mut index = 0;
    for joined in joined_chunks {
        match joined {
            Ok(chunk) => statuses.extend(chunk),
            Err(_) => {
                let end = (index + chunk_size).min(conversations.len());
                statuses.extend(conversations[index..end].iter().map(|conversation| {
                    task_board_unknown_conversation_status(conversation, false)
                }));
            }
        }
        index += chunk_size;
    }
    statuses
}

fn task_board_local_conversation_status(
    conversation: &ConversationStatusRef,
    db_paths: &[PathBuf],
) -> Value {
    let usage = codex_elves_data::codex_thread_usage_summary_from_paths(
        db_paths.to_vec(),
        &SessionRef {
            session_id: conversation.session_id.clone(),
            title: conversation.title.clone(),
        },
    );
    task_board_conversation_status_from_usage(conversation, &usage)
}

fn task_board_conversation_status_from_usage(
    conversation: &ConversationStatusRef,
    usage: &Value,
) -> Value {
    let summary = usage
        .get("summary")
        .filter(|summary| summary.is_object())
        .filter(|_| usage.get("status").and_then(Value::as_str) == Some("ok"));
    json!({
        "sessionId": conversation.session_id,
        "known": summary.is_some(),
        "checking": false,
        "isRunning": summary.is_some_and(|summary| {
            summary.get("isRunning").and_then(Value::as_bool) == Some(true)
                || summary.get("lastTurnRunning").and_then(Value::as_bool) == Some(true)
        }),
        "unread": false,
    })
}

fn task_board_unknown_conversation_status(
    conversation: &ConversationStatusRef,
    unread: bool,
) -> Value {
    json!({
        "sessionId": conversation.session_id,
        "known": false,
        "checking": false,
        "isRunning": false,
        "unread": unread,
    })
}

fn task_board_merge_conversation_statuses(
    conversations: &[ConversationStatusRef],
    host_result: Value,
    local_statuses: Vec<Value>,
) -> Value {
    let host_statuses = host_result
        .get("statuses")
        .and_then(Value::as_array)
        .map(|statuses| {
            statuses
                .iter()
                .filter_map(|status| {
                    let session_id = status.get("sessionId").and_then(Value::as_str)?;
                    let key = task_board_session_id_key(session_id);
                    (!key.is_empty()).then(|| (key, status))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let local_statuses = local_statuses
        .iter()
        .filter_map(|status| {
            let session_id = status.get("sessionId").and_then(Value::as_str)?;
            let key = task_board_session_id_key(session_id);
            (!key.is_empty()).then(|| (key, status))
        })
        .collect::<HashMap<_, _>>();
    let statuses = conversations
        .iter()
        .map(|conversation| {
            let key = task_board_session_id_key(&conversation.session_id);
            let host = host_statuses.get(&key).copied();
            let local = local_statuses.get(&key).copied();
            json!({
                "sessionId": conversation.session_id,
                "known": host.and_then(|status| status.get("known")).and_then(Value::as_bool) == Some(true)
                    || local.and_then(|status| status.get("known")).and_then(Value::as_bool) == Some(true),
                "checking": false,
                "isRunning": host.and_then(|status| status.get("isRunning")).and_then(Value::as_bool) == Some(true)
                    || local.and_then(|status| status.get("isRunning")).and_then(Value::as_bool) == Some(true),
                "unread": host.and_then(|status| status.get("unread")).and_then(Value::as_bool) == Some(true),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "status": "ok",
        "statuses": statuses,
    })
}

fn task_board_session_id_key(value: &str) -> String {
    let value = value.trim();
    let value = value
        .get(..6)
        .filter(|prefix| prefix.eq_ignore_ascii_case("local:"))
        .map(|_| &value[6..])
        .unwrap_or(value);
    value.to_ascii_lowercase()
}

#[tauri::command]
pub async fn task_board_start_conversation(request: StartConversationRequest) -> Value {
    if request.first_instruction.trim().is_empty() {
        return failed("invalid_input", "请输入首条指令");
    }
    call_codex_host_operation(
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
            return Err(TaskBoardStoreError::SessionNotFound {
                session_id: session_id.to_string(),
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
    let codex_home = codex_elves_core::codex_sqlite::default_codex_home_dir();
    let db_paths = candidate_codex_db_paths();
    let local_catalog = codex_elves_data::aggregate_local_session_catalog(&db_paths)
        .map_err(|_| anyhow::anyhow!("无法读取 Codex 本地会话目录"))?;
    let project_catalog = codex_elves_data::load_codex_project_catalog(&codex_home)
        .map_err(|_| anyhow::anyhow!("无法读取 Codex 项目目录"))?;
    codex_elves_data::task_board_catalog_from_local_catalog(local_catalog, project_catalog)
        .map_err(|_| anyhow::anyhow!("无法构建 Codex 项目与会话目录"))
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

async fn call_codex_host(method: &str, arguments: Value) -> Value {
    call_codex_host_with_min_runtime(method, arguments, TASK_BOARD_MIN_RUNTIME_VERSION).await
}

async fn call_codex_host_with_min_runtime(
    method: &str,
    arguments: Value,
    min_runtime_version: u64,
) -> Value {
    let websocket_url = match codex_host_websocket_url().await {
        Ok(websocket_url) => websocket_url,
        Err(failure) => return failure,
    };
    let script = task_board_host_call_script(method, &arguments, min_runtime_version);
    match codex_elves_core::bridge::evaluate_script_with_await_promise(
        &websocket_url,
        &script,
        true,
    )
    .await
    {
        Ok(response) => runtime_evaluate_value(&response)
            .unwrap_or_else(|| failed("host_action_failed", "Codex 宿主动作没有返回有效结果")),
        Err(_) => failed("host_action_failed", "Codex 宿主动作执行失败"),
    }
}

async fn call_codex_host_operation(method: &str, arguments: Value) -> Value {
    let websocket_url = match codex_host_websocket_url().await {
        Ok(websocket_url) => websocket_url,
        Err(failure) => return failure,
    };
    let operation_id = uuid::Uuid::new_v4().to_string();
    let start_script = task_board_host_operation_start_script(&operation_id, method, &arguments);
    let start_result =
        match evaluate_codex_host_operation_script(&websocket_url, &start_script).await {
            Ok(result) => result,
            Err(error) => {
                return abandon_codex_host_operation(
                    &websocket_url,
                    &operation_id,
                    "start_failed",
                    format!("启动 Codex 宿主动作时调试通道失败：{error}"),
                )
                .await;
            }
        };
    match task_board_host_operation_poll_outcome(start_result, &operation_id) {
        TaskBoardHostOperationPollOutcome::Pending => {}
        TaskBoardHostOperationPollOutcome::Complete(result) => return result,
    }

    let deadline = tokio::time::Instant::now() + TASK_BOARD_HOST_OPERATION_TIMEOUT;
    let mut consecutive_poll_failures = 0_u32;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return abandon_codex_host_operation(
                &websocket_url,
                &operation_id,
                "timeout",
                format!(
                    "等待 Codex 宿主动作超过 {} 秒，最终结果未知",
                    TASK_BOARD_HOST_OPERATION_TIMEOUT.as_secs()
                ),
            )
            .await;
        }

        let poll_script = task_board_host_operation_poll_script(&operation_id);
        match evaluate_codex_host_operation_script(&websocket_url, &poll_script).await {
            Ok(result) => {
                consecutive_poll_failures = 0;
                match task_board_host_operation_poll_outcome(result, &operation_id) {
                    TaskBoardHostOperationPollOutcome::Pending => {}
                    TaskBoardHostOperationPollOutcome::Complete(result) => return result,
                }
            }
            Err(error) => {
                consecutive_poll_failures = consecutive_poll_failures.saturating_add(1);
                if consecutive_poll_failures
                    >= TASK_BOARD_HOST_OPERATION_MAX_CONSECUTIVE_POLL_FAILURES
                {
                    return abandon_codex_host_operation(
                        &websocket_url,
                        &operation_id,
                        "poll_failed",
                        format!(
                            "连续 {consecutive_poll_failures} 次查询 Codex 宿主动作失败，最终结果未知：{error}"
                        ),
                    )
                    .await;
                }
            }
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            continue;
        }
        tokio::time::sleep(std::cmp::min(
            TASK_BOARD_HOST_OPERATION_POLL_DELAY,
            remaining,
        ))
        .await;
    }
}

async fn evaluate_codex_host_operation_script(
    websocket_url: &str,
    script: &str,
) -> Result<Value, String> {
    let response =
        codex_elves_core::bridge::evaluate_script_with_await_promise(websocket_url, script, false)
            .await
            .map_err(|error| error.to_string())?;
    runtime_evaluate_value(&response)
        .ok_or_else(|| "Codex 调试通道没有返回可解析的宿主结果".to_string())
}

async fn abandon_codex_host_operation(
    websocket_url: &str,
    operation_id: &str,
    reason: &str,
    message: String,
) -> Value {
    let abandon_script = task_board_host_operation_abandon_script(operation_id);
    match evaluate_codex_host_operation_script(websocket_url, &abandon_script).await {
        Ok(result) => {
            task_board_host_operation_abandon_resolution(result, operation_id, reason, message)
        }
        Err(cleanup_error) => task_board_host_outcome_unknown(
            operation_id,
            reason,
            format!("{message}；清理页面 operation 时失败：{cleanup_error}"),
        ),
    }
}

async fn codex_host_websocket_url() -> Result<String, Value> {
    let latest = match StatusStore::default().load_latest() {
        Ok(Some(latest)) if latest.status == "running" => latest,
        _ => {
            return Err(failed(
                "codex_unavailable",
                "Codex 当前未通过 CodexElves 运行",
            ));
        }
    };
    let Some(debug_port) = latest.debug_port else {
        return Err(failed("codex_unavailable", "Codex 调试端口不可用"));
    };
    let targets = match codex_elves_core::cdp::list_targets(debug_port).await {
        Ok(targets) => targets,
        Err(_) => return Err(failed("codex_unavailable", "无法连接 Codex 页面")),
    };
    let target = match codex_elves_core::cdp::pick_injectable_codex_page_target(&targets) {
        Ok(target) => target,
        Err(_) => return Err(failed("codex_unavailable", "未找到可用的 Codex 主页面")),
    };
    let Some(websocket_url) = target.web_socket_debugger_url.as_deref() else {
        return Err(failed("codex_unavailable", "Codex 页面调试通道不可用"));
    };
    Ok(websocket_url.to_string())
}

fn task_board_host_call_script(
    method: &str,
    arguments: &Value,
    min_runtime_version: u64,
) -> String {
    let method_json = serde_json::to_string(method).unwrap_or_else(|_| "\"\"".to_string());
    let arguments_json = serde_json::to_string(arguments).unwrap_or_else(|_| "[]".to_string());
    format!(
        r#"
(() => {{
  const method = {method_json};
  const args = {arguments_json};
  const host = window.__codexElvesTaskBoardHost;
  if (!host || typeof host[method] !== "function") {{
    return {{ status: "failed", code: "host_unavailable", message: "Codex 任务看板宿主接口尚未就绪" }};
  }}
  const hostVersion = Number(host.version);
  const runtimeVersionText = String(
    window.__codexElvesTaskBoardRuntimeVersion || "",
  ).trim();
  const runtimeVersion = Number.parseInt(runtimeVersionText, 10);
  if (
    !Number.isSafeInteger(hostVersion) ||
    hostVersion < {min_host_version} ||
    !Number.isSafeInteger(runtimeVersion) ||
    runtimeVersion < {min_runtime_version}
  ) {{
    return {{
      status: "failed",
      code: "host_version_unsupported",
      message: "Codex 任务看板宿主版本过旧，请重启 CodexElves 完成升级",
      hostVersion: Number.isFinite(hostVersion) ? hostVersion : null,
      runtimeVersion: runtimeVersionText,
    }};
  }}
  return Promise.resolve(host[method](...args))
    .then((result) => JSON.stringify(result ?? null))
    .catch((error) => JSON.stringify({{
      status: "failed",
      code: "host_action_failed",
      message: String(error?.message || error || "宿主动作失败"),
    }}));
}})()
"#,
        min_host_version = TASK_BOARD_MIN_HOST_VERSION,
        min_runtime_version = min_runtime_version,
    )
}

fn task_board_host_operation_start_script(
    operation_id: &str,
    method: &str,
    arguments: &Value,
) -> String {
    let operation_id_json =
        serde_json::to_string(operation_id).unwrap_or_else(|_| "\"\"".to_string());
    let method_json = serde_json::to_string(method).unwrap_or_else(|_| "\"\"".to_string());
    let arguments_json = serde_json::to_string(arguments).unwrap_or_else(|_| "[]".to_string());
    format!(
        r#"
(() => {{
  const operationId = {operation_id_json};
  const method = {method_json};
  const args = {arguments_json};
  const retentionMs = {retention_ms};
  const maxEntries = {max_entries};
  const operations = window.__codexElvesTaskBoardStandaloneOperations ||
    (window.__codexElvesTaskBoardStandaloneOperations = Object.create(null));
  const releaseOperationLease = (candidateOperationId, runtimeId = 0) => {{
    const lease = window.__codexElvesTaskBoardNativeOperationLease;
    if (
      String(lease?.operationId || "") === `standalone:${{candidateOperationId}}` &&
      (!runtimeId || Number(lease?.runtimeId) === Number(runtimeId))
    ) {{
      delete window.__codexElvesTaskBoardNativeOperationLease;
    }}
  }};
  const removeOperation = (candidateOperationId, operation, abandon = false) => {{
    if (abandon && operation) {{
      operation.abandoned = true;
      operation.settledAtMs ||= Date.now();
    }}
    if (operation?.cleanupTimer) clearTimeout(operation.cleanupTimer);
    releaseOperationLease(candidateOperationId, operation?.runtimeId);
    delete operations[candidateOperationId];
  }};
  const cleanupStaleOperations = () => {{
    const now = Date.now();
    Object.entries(operations).forEach(([candidateOperationId, operation]) => {{
      const createdAtMs = Number(operation?.createdAtMs);
      const settledAtMs = Number(operation?.settledAtMs);
      const terminalAtMs = operation?.settled || operation?.abandoned
        ? settledAtMs || createdAtMs
        : createdAtMs;
      const invalid = !Number.isFinite(createdAtMs) || createdAtMs <= 0;
      const stale = Number.isFinite(terminalAtMs) &&
        terminalAtMs > 0 &&
        now - terminalAtMs >= retentionMs;
      if (invalid || stale) {{
        removeOperation(candidateOperationId, operation, !operation?.settled);
      }}
    }});
  }};
  cleanupStaleOperations();
  if (operations[operationId]) {{
    return JSON.stringify({{ status: "pending", operationId }});
  }}
  if (Object.keys(operations).length >= maxEntries) {{
    return JSON.stringify({{
      status: "failed",
      code: "host_operation_capacity",
      message: "Codex 宿主操作队列已满，请稍后重试",
    }});
  }}
  const host = window.__codexElvesTaskBoardHost;
  if (!host || typeof host[method] !== "function") {{
    return JSON.stringify({{
      status: "failed",
      code: "host_unavailable",
      message: "Codex 任务看板宿主接口尚未就绪",
    }});
  }}
  const hostVersion = Number(host.version);
  const runtimeVersionText = String(
    window.__codexElvesTaskBoardRuntimeVersion || "",
  ).trim();
  const runtimeVersion = Number.parseInt(runtimeVersionText, 10);
  const capabilities = host.capabilities;
  const supportsNativeCreateLease =
    capabilities?.nativeCreateLease === true;
  const nativeCreateRuntime = Number(capabilities?.nativeCreateRuntime);
  const supportsNativeCreateRuntime =
    Number.isSafeInteger(nativeCreateRuntime) &&
    nativeCreateRuntime >= {min_runtime_version} &&
    nativeCreateRuntime === runtimeVersion;
  if (
    !Number.isSafeInteger(hostVersion) ||
    hostVersion < {min_host_version} ||
    !Number.isSafeInteger(runtimeVersion) ||
    runtimeVersion < {min_runtime_version} ||
    !supportsNativeCreateLease ||
    !supportsNativeCreateRuntime
  ) {{
    return JSON.stringify({{
      status: "failed",
      code: "host_version_unsupported",
      message: "Codex 任务看板宿主版本过旧，请重启 CodexElves 完成升级",
      hostVersion: Number.isFinite(hostVersion) ? hostVersion : null,
      runtimeVersion: runtimeVersionText,
    }});
  }}
  const nativeRuntimeId = Number(
    window.__codexElvesTaskBoardNativeRuntimeId,
  );
  const nativeOperationLeaseId = `standalone:${{operationId}}`;
  const existingNativeLease =
    window.__codexElvesTaskBoardNativeOperationLease;
  const existingNativeLeaseActive =
    String(existingNativeLease?.operationId || "") &&
    Number.isSafeInteger(Number(existingNativeLease?.runtimeId)) &&
    Number(existingNativeLease?.runtimeId) > 0 &&
    Number.isFinite(Number(existingNativeLease?.createdAtMs)) &&
    Date.now() - Number(existingNativeLease?.createdAtMs) <= 120_000;
  if (
    existingNativeLeaseActive &&
    existingNativeLease.operationId !== nativeOperationLeaseId
  ) {{
    return JSON.stringify({{
      status: "failed",
      code: "native_create_busy",
      message: "Codex 正在创建另一个会话，请稍后重试",
    }});
  }}
  if (!Number.isSafeInteger(nativeRuntimeId) || nativeRuntimeId <= 0) {{
    return JSON.stringify({{
      status: "failed",
      code: "runtime_replaced",
      message: "Codex 页面已更新，请重试",
    }});
  }}
  const createdAtMs = Date.now();
  window.__codexElvesTaskBoardNativeOperationLease = {{
    operationId: nativeOperationLeaseId,
    runtimeId: nativeRuntimeId,
    createdAtMs,
  }};

  const operation = {{
    method,
    runtimeId: nativeRuntimeId,
    createdAtMs,
    settledAtMs: 0,
    abandoned: false,
    settled: false,
    result: null,
    promise: null,
    cleanupTimer: null,
  }};
  operations[operationId] = operation;
  operation.cleanupTimer = setTimeout(() => {{
    if (operations[operationId] !== operation) return;
    removeOperation(operationId, operation, !operation.settled);
  }}, retentionMs);
  try {{
    operation.promise = Promise.resolve(host[method](...args)).then(
      (result) => {{
        if (operation.abandoned) {{
          releaseOperationLease(operationId, operation.runtimeId);
          return;
        }}
        operation.result = result ?? {{
          status: "failed",
          code: "host_action_failed",
          message: "Codex 宿主动作没有返回有效结果",
        }};
        operation.settled = true;
        operation.settledAtMs = Date.now();
        releaseOperationLease(operationId, operation.runtimeId);
      }},
      (error) => {{
        if (operation.abandoned) {{
          releaseOperationLease(operationId, operation.runtimeId);
          return;
        }}
        operation.result = {{
          status: "failed",
          code: "host_action_failed",
          message: String(error?.message || error || "宿主动作失败"),
        }};
        operation.settled = true;
        operation.settledAtMs = Date.now();
        releaseOperationLease(operationId, operation.runtimeId);
      }},
    );
  }} catch (error) {{
    removeOperation(operationId, operation, true);
    return JSON.stringify({{
      status: "failed",
      code: "host_action_failed",
      message: String(error?.message || error || "宿主动作失败"),
    }});
  }}
  return JSON.stringify({{ status: "pending", operationId }});
}})()
"#,
        retention_ms = TASK_BOARD_HOST_OPERATION_RETENTION_MS,
        max_entries = TASK_BOARD_HOST_OPERATION_MAX_ENTRIES,
        min_host_version = TASK_BOARD_MIN_HOST_VERSION,
        min_runtime_version = TASK_BOARD_MIN_RUNTIME_VERSION,
    )
}

fn task_board_host_operation_poll_script(operation_id: &str) -> String {
    let operation_id_json =
        serde_json::to_string(operation_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
(() => {{
  const operationId = {operation_id_json};
  const operations = window.__codexElvesTaskBoardStandaloneOperations;
  const operation = operations?.[operationId];
  const releaseOperationLease = () => {{
    const lease = window.__codexElvesTaskBoardNativeOperationLease;
    if (
      String(lease?.operationId || "") === `standalone:${{operationId}}` &&
      (!operation?.runtimeId ||
        Number(lease?.runtimeId) === Number(operation.runtimeId))
    ) {{
      delete window.__codexElvesTaskBoardNativeOperationLease;
    }}
  }};
  if (!operation) {{
    releaseOperationLease();
    return JSON.stringify({{
      status: "failed",
      code: "runtime_replaced",
      message: "Codex 页面 operation 已丢失，最终结果未知",
      outcomeUnknown: true,
      operationId,
    }});
  }}
  if (operation.abandoned) {{
    if (operation.cleanupTimer) clearTimeout(operation.cleanupTimer);
    delete operations[operationId];
    releaseOperationLease();
    return JSON.stringify({{
      status: "failed",
      code: "host_outcome_unknown",
      message: "Codex 宿主动作已放弃，最终结果未知",
      outcomeUnknown: true,
      operationId,
    }});
  }}
  if (!operation.settled) {{
    return JSON.stringify({{
      status: "pending",
      operationId,
      createdAtMs: operation.createdAtMs,
    }});
  }}
  const result = operation.result ?? {{
    status: "failed",
    code: "host_action_failed",
    message: "Codex 宿主动作没有返回有效结果",
  }};
  if (operation.cleanupTimer) clearTimeout(operation.cleanupTimer);
  delete operations[operationId];
  releaseOperationLease();
  return JSON.stringify(result);
}})()
"#
    )
}

fn task_board_host_operation_abandon_script(operation_id: &str) -> String {
    let operation_id_json =
        serde_json::to_string(operation_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"
(() => {{
  const operationId = {operation_id_json};
  const operations = window.__codexElvesTaskBoardStandaloneOperations;
  const operation = operations?.[operationId];
  const releaseOperationLease = () => {{
    const lease = window.__codexElvesTaskBoardNativeOperationLease;
    if (
      String(lease?.operationId || "") === `standalone:${{operationId}}` &&
      (!operation?.runtimeId ||
        Number(lease?.runtimeId) === Number(operation.runtimeId))
    ) {{
      delete window.__codexElvesTaskBoardNativeOperationLease;
    }}
  }};
  if (!operation) {{
    releaseOperationLease();
    return JSON.stringify({{
      status: "failed",
      code: "runtime_replaced",
      message: "Codex 页面 operation 已丢失，最终结果未知",
      outcomeUnknown: true,
      operationId,
    }});
  }}
  if (operation.settled) {{
    const result = operation.result ?? {{
      status: "failed",
      code: "host_action_failed",
      message: "Codex 宿主动作没有返回有效结果",
    }};
    if (operation.cleanupTimer) clearTimeout(operation.cleanupTimer);
    delete operations[operationId];
    releaseOperationLease();
    return JSON.stringify({{
      status: "settled",
      operationId,
      result,
    }});
  }}
  operation.abandoned = true;
  operation.settledAtMs = Date.now();
  if (operation.cleanupTimer) clearTimeout(operation.cleanupTimer);
  delete operations[operationId];
  releaseOperationLease();
  return JSON.stringify({{
    status: "abandoned",
    operationId,
    createdAtMs: operation.createdAtMs,
    settledAtMs: operation.settledAtMs,
    abandoned: true,
  }});
}})()
"#
    )
}

#[derive(Debug, PartialEq)]
enum TaskBoardHostOperationPollOutcome {
    Pending,
    Complete(Value),
}

fn task_board_host_operation_poll_outcome(
    value: Value,
    operation_id: &str,
) -> TaskBoardHostOperationPollOutcome {
    if value.get("status").and_then(Value::as_str) != Some("pending") {
        return TaskBoardHostOperationPollOutcome::Complete(value);
    }
    if value.get("operationId").and_then(Value::as_str) == Some(operation_id) {
        return TaskBoardHostOperationPollOutcome::Pending;
    }
    TaskBoardHostOperationPollOutcome::Complete(failed(
        "host_operation_protocol_error",
        "Codex 宿主 operation 标识不匹配",
    ))
}

fn task_board_host_operation_abandon_resolution(
    value: Value,
    operation_id: &str,
    reason: &str,
    message: String,
) -> Value {
    if value.get("status").and_then(Value::as_str) == Some("settled") {
        return value.get("result").cloned().unwrap_or_else(|| {
            failed(
                "host_operation_protocol_error",
                "Codex 宿主 operation 已完成但缺少结果",
            )
        });
    }
    if value.get("status").and_then(Value::as_str) == Some("failed") {
        return value;
    }
    task_board_host_outcome_unknown(operation_id, reason, message)
}

fn task_board_host_outcome_unknown(operation_id: &str, reason: &str, message: String) -> Value {
    json!({
        "status": "failed",
        "code": "host_outcome_unknown",
        "message": message,
        "outcomeUnknown": true,
        "reason": reason,
        "operationId": operation_id,
    })
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
        "boards": document.boards,
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
        TaskBoardStoreError::SessionNotFound { session_id } => failed(
            "session_not_found",
            format!("会话不存在或尚未写入本地目录：{session_id}"),
        ),
        TaskBoardStoreError::RevisionConflict { current } => json!({
            "status": "conflict",
            "code": "revision_conflict",
            "message": "任务看板已发生变化，请重试",
            "schemaVersion": current.schema_version,
            "revision": current.revision,
            "boards": current.boards,
            "tasks": current.tasks,
        }),
        TaskBoardStoreError::TaskIdConflict => failed("task_id_conflict", "任务 ID 与现有任务冲突"),
        TaskBoardStoreError::BoardIdConflict => {
            failed("board_id_conflict", "看板 ID 与现有看板冲突")
        }
        TaskBoardStoreError::BoardNotFound => failed("board_not_found", "看板不存在"),
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
    fn standalone_missing_catalog_session_is_retryable() {
        let catalog = TaskBoardSessionCatalog {
            projects: Vec::new(),
            sessions: Vec::new(),
            warnings: Vec::new(),
        };

        let error = conversations_from_catalog(&catalog, &["session-missing".to_string()])
            .expect_err("missing catalog session should fail");
        let response = store_error_value(error);

        assert_eq!(response["status"], "failed");
        assert_eq!(response["code"], "session_not_found");
        assert_eq!(
            response["message"],
            "会话不存在或尚未写入本地目录：session-missing"
        );
    }

    #[test]
    fn standalone_open_session_forwards_session_aliases_to_host() {
        let request = OpenSessionRequest {
            session_id: "session-permanent".to_string(),
            title: "会话标题".to_string(),
            cwd: "E:\\code\\project".to_string(),
            project_label: "项目".to_string(),
            updated_at_ms: Some(123),
            session_aliases: vec!["client-new-thread:temporary".to_string()],
        };

        assert_eq!(
            task_board_open_session_arguments(&request, "session-permanent"),
            json!([
                "session-permanent",
                {
                    "sessionId": "session-permanent",
                    "title": "会话标题",
                    "cwd": "E:\\code\\project",
                    "projectLabel": "项目",
                    "updatedAtMs": 123,
                    "sessionAliases": ["client-new-thread:temporary"]
                }
            ])
        );
    }

    #[test]
    fn standalone_status_request_forwards_normalized_session_aliases_to_host() {
        let conversations = normalized_conversation_status_refs(ConversationStatusesRequest {
            conversations: vec![ConversationStatusRef {
                session_id: " session-permanent ".to_string(),
                title: "会话标题".to_string(),
                session_aliases: vec![
                    " client-new-thread:temporary ".to_string(),
                    "local:client-new-thread:temporary".to_string(),
                    String::new(),
                    "local:session-permanent".to_string(),
                ],
            }],
        });
        let arguments = task_board_conversation_status_arguments(&conversations);

        assert_eq!(
            arguments,
            json!([[
                {
                    "sessionId": "session-permanent",
                    "title": "会话标题",
                    "sessionAliases": ["client-new-thread:temporary"]
                }
            ]])
        );
    }

    #[test]
    fn local_conversation_statuses_run_in_parallel_and_preserve_order() {
        let conversations = (0..20)
            .map(|index| ConversationStatusRef {
                session_id: format!("session-{index:02}"),
                title: format!("会话 {index}"),
                session_aliases: Vec::new(),
            })
            .collect::<Vec<_>>();

        let statuses = task_board_local_conversation_statuses(&conversations, Vec::new());

        assert_eq!(statuses.len(), conversations.len());
        for (conversation, status) in conversations.iter().zip(statuses.iter()) {
            assert_eq!(status["sessionId"], conversation.session_id);
            assert_eq!(status["known"], false);
            assert_eq!(status["checking"], false);
        }
    }

    #[test]
    fn standalone_status_uses_local_summary_when_host_reports_unknown() {
        let conversations = vec![ConversationStatusRef {
            session_id: "session-1".to_string(),
            title: "会话".to_string(),
            session_aliases: Vec::new(),
        }];
        let local = vec![task_board_conversation_status_from_usage(
            &conversations[0],
            &json!({
                "status": "ok",
                "summary": {
                    "isRunning": true,
                    "lastTurnRunning": true
                }
            }),
        )];
        let merged = task_board_merge_conversation_statuses(
            &conversations,
            json!({
                "status": "ok",
                "statuses": [{
                    "sessionId": "session-1",
                    "known": false,
                    "checking": false,
                    "isRunning": false,
                    "unread": false
                }]
            }),
            local,
        );

        assert_eq!(merged["status"], "ok");
        assert_eq!(merged["statuses"][0]["known"], true);
        assert_eq!(merged["statuses"][0]["checking"], false);
        assert_eq!(merged["statuses"][0]["isRunning"], true);
    }

    #[test]
    fn standalone_status_preserves_host_unread_while_local_summary_supplies_known_state() {
        let conversations = vec![ConversationStatusRef {
            session_id: "local:session-1".to_string(),
            title: "会话".to_string(),
            session_aliases: Vec::new(),
        }];
        let local = vec![task_board_conversation_status_from_usage(
            &conversations[0],
            &json!({
                "status": "ok",
                "summary": {}
            }),
        )];
        let merged = task_board_merge_conversation_statuses(
            &conversations,
            json!({
                "status": "ok",
                "statuses": [{
                    "sessionId": "session-1",
                    "known": false,
                    "checking": false,
                    "isRunning": false,
                    "unread": true
                }]
            }),
            local,
        );

        assert_eq!(merged["statuses"][0]["sessionId"], "local:session-1");
        assert_eq!(merged["statuses"][0]["known"], true);
        assert_eq!(merged["statuses"][0]["isRunning"], false);
        assert_eq!(merged["statuses"][0]["unread"], true);
    }

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
    fn standalone_host_calls_require_current_runtime() {
        let script = task_board_host_call_script(
            "openSession",
            &json!(["session-1", {"projectLabel": "项目"}]),
            TASK_BOARD_MIN_RUNTIME_VERSION,
        );

        assert!(script.contains(r#"code: "host_version_unsupported""#));
        assert!(script.contains("hostVersion < 3"));
        assert!(script.contains("runtimeVersion < 62"));
        assert!(script.contains("Codex 任务看板宿主版本过旧，请重启 CodexElves 完成升级"));
    }

    #[test]
    fn standalone_conversation_statuses_accept_the_stable_v58_host_contract() {
        let script = task_board_host_call_script(
            "conversationStatuses",
            &json!([[{"sessionId": "session-1", "title": "会话"}]]),
            TASK_BOARD_MIN_CONVERSATION_STATUS_RUNTIME_VERSION,
        );

        assert!(script.contains("runtimeVersion < 58"));
        assert!(!script.contains("runtimeVersion < 62"));
    }

    #[test]
    fn standalone_start_operation_requires_v3_capabilities_without_dom_overrides() {
        let script = task_board_host_operation_start_script(
            "operation-1",
            "startConversation",
            &json!([
                {"cwd": "E:\\code\\junes\\github\\CodexElves", "label": "CodexElves"},
                "验证首条指令",
                "gpt-5.6-sol",
                "max"
            ]),
        );

        assert!(script.contains("window.__codexElvesTaskBoardStandaloneOperations"));
        assert!(script.contains("__codexElvesTaskBoardNativeOperationLease"));
        assert!(script.contains("native_create_busy"));
        assert!(script.contains(r#"code: "host_version_unsupported""#));
        assert!(script.contains("hostVersion < 3"));
        assert!(script.contains("runtimeVersion < 62"));
        assert!(script.contains("supportsNativeCreateLease"));
        assert!(script.contains("supportsNativeCreateRuntime"));
        assert!(script.contains("capabilities?.nativeCreateLease === true"));
        assert!(script.contains("Number(capabilities?.nativeCreateRuntime)"));
        assert!(script.contains("nativeCreateRuntime === runtimeVersion"));
        assert!(script.contains("createdAtMs"));
        assert!(script.contains("settledAtMs: 0"));
        assert!(script.contains("abandoned: false"));
        assert!(script.contains("cleanupStaleOperations();"));
        assert!(script.contains("operation.cleanupTimer = setTimeout"));
        assert!(script.contains(r#"return JSON.stringify({ status: "pending", operationId });"#));
        assert!(!script.contains("Document.prototype"));
        assert!(!script.contains("querySelector"));
        assert!(!script.contains("dispatchEvent"));
        assert!(!script.contains("sendButton"));
    }

    #[test]
    fn standalone_host_operation_poll_is_immediate_and_preserves_detailed_result() {
        let script = task_board_host_operation_poll_script("operation-2");

        assert!(script.contains("if (!operation.settled)"));
        assert!(script.contains("operation.result ??"));
        assert!(script.contains("delete operations[operationId]"));
        assert!(script.contains(r#"code: "runtime_replaced""#));
        assert!(!script.contains("Promise.race"));
        assert!(!script.contains("operation.promise.then"));
        assert_eq!(
            task_board_host_operation_poll_outcome(
                json!({"status": "pending", "operationId": "operation-2"}),
                "operation-2"
            ),
            TaskBoardHostOperationPollOutcome::Pending
        );
        assert_eq!(
            task_board_host_operation_poll_outcome(
                json!({
                    "status": "failed",
                    "code": "native_model_not_found",
                    "message": "模型不存在"
                }),
                "operation-2"
            ),
            TaskBoardHostOperationPollOutcome::Complete(json!({
                "status": "failed",
                "code": "native_model_not_found",
                "message": "模型不存在"
            }))
        );
        let mismatch = task_board_host_operation_poll_outcome(
            json!({"status": "pending", "operationId": "another-operation"}),
            "operation-2",
        );
        assert!(matches!(
            mismatch,
            TaskBoardHostOperationPollOutcome::Complete(value)
                if value.get("code").and_then(Value::as_str)
                    == Some("host_operation_protocol_error")
        ));
    }

    #[test]
    fn standalone_host_operation_abandon_cleans_state_and_preserves_settled_result() {
        let script = task_board_host_operation_abandon_script("operation-3");

        assert!(script.contains("operation.abandoned = true"));
        assert!(script.contains("operation.settledAtMs = Date.now()"));
        assert!(script.contains("clearTimeout(operation.cleanupTimer)"));
        assert!(script.contains("delete operations[operationId]"));
        assert!(script.contains("releaseOperationLease();"));
        assert_eq!(
            task_board_host_operation_abandon_resolution(
                json!({
                    "status": "settled",
                    "operationId": "operation-3",
                    "result": {
                        "status": "failed",
                        "code": "native_create_timeout",
                        "message": "等待新会话就绪超时"
                    }
                }),
                "operation-3",
                "timeout",
                "外层超时".to_string(),
            ),
            json!({
                "status": "failed",
                "code": "native_create_timeout",
                "message": "等待新会话就绪超时"
            })
        );
        assert_eq!(
            task_board_host_operation_abandon_resolution(
                json!({
                    "status": "failed",
                    "code": "runtime_replaced",
                    "message": "operation 已丢失",
                    "outcomeUnknown": true
                }),
                "operation-3",
                "poll_failed",
                "轮询失败".to_string(),
            ),
            json!({
                "status": "failed",
                "code": "runtime_replaced",
                "message": "operation 已丢失",
                "outcomeUnknown": true
            })
        );
        let abandoned = task_board_host_operation_abandon_resolution(
            json!({"status": "abandoned", "operationId": "operation-3"}),
            "operation-3",
            "timeout",
            "结果未知".to_string(),
        );
        assert_eq!(
            abandoned.get("code").and_then(Value::as_str),
            Some("host_outcome_unknown")
        );
        assert_eq!(
            abandoned.get("outcomeUnknown").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            abandoned.get("reason").and_then(Value::as_str),
            Some("timeout")
        );
    }

    #[test]
    fn standalone_host_operation_timeout_covers_hidden_page_throttling() {
        assert_eq!(TASK_BOARD_HOST_OPERATION_TIMEOUT, Duration::from_secs(120));
        assert!(TASK_BOARD_HOST_OPERATION_TIMEOUT > Duration::from_millis(66_700));
        assert!(TASK_BOARD_HOST_OPERATION_POLL_DELAY >= Duration::from_millis(250));
        assert!(TASK_BOARD_HOST_OPERATION_POLL_DELAY <= Duration::from_millis(500));
        assert_eq!(TASK_BOARD_HOST_OPERATION_MAX_CONSECUTIVE_POLL_FAILURES, 3);
    }

    #[test]
    fn javascript_safe_revision_limit_is_preserved() {
        assert!(codex_elves_core::task_board::TASK_BOARD_MAX_SAFE_INTEGER > 0);
    }
}
