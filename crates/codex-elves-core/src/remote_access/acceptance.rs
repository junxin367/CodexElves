//! 独立的本机 Claude 验收后台：不启动 launcher、代理、隧道或 Codex 执行进程。
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use fs2::FileExt;
use serde_json::{Value, json};

use super::{
    ApiError, NativeRemoteBridge, RemoteAccessController, RemoteAuthorization, claude, now_ms,
    web_router,
};
use crate::settings::{RemoteAccessSettings, SettingsStore};

const STOP_FILE: &str = "stop-request";

struct ProjectSnapshot {
    projects: Vec<Value>,
    sidebar: Value,
    desktop: Option<Arc<DesktopReader>>,
}

impl ProjectSnapshot {
    fn from_value(value: &Value) -> anyhow::Result<Self> {
        let mut ids = HashSet::new();
        let projects = value["projects"]
            .as_array()
            .context("原程序未返回项目清单，未创建替代项目")?
            .iter()
            .map(|project| {
                let id = project["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("原项目缺少 ID")?;
                anyhow::ensure!(ids.insert(id.to_owned()), "原项目 ID 重复");
                let roots: Vec<_> = project["roots"]
                    .as_array()
                    .context("原项目缺少目录信息")?
                    .iter()
                    .map(|root| root.as_str().context("原项目目录格式无效"))
                    .collect::<anyhow::Result<_>>()?;
                // 保留原生项目身份及根目录，不按 WSL 路径或目录名称生成另一套项目。
                Ok(
                    json!({"id":id,"name":project["name"].as_str().unwrap_or(id),
                    "roots":roots,"pinned":project["pinned"]==true,
                    "threads":project["threads"].as_array().cloned().unwrap_or_default(),
                    "recencyAt":project["recencyAt"].as_u64().unwrap_or(0)}),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            !projects.is_empty(),
            "原程序项目清单为空，请先在原程序打开项目"
        );
        let mut sidebar = value.clone();
        sidebar["ok"] = json!(true);
        sidebar["projects"] = json!(projects);
        for field in ["recent", "pinnedThreads", "sideThreadIds"] {
            if !sidebar[field].is_array() {
                sidebar[field] = json!([]);
            }
        }
        Ok(Self {
            projects,
            sidebar,
            desktop: None,
        })
    }
}

fn native_read(action: &str) -> bool {
    matches!(
        action,
        "sidebar"
            | "thread"
            | "history"
            | "workspaceContext"
            | "models"
            | "queue"
            | "notifications"
            | "events"
            | "skills"
            | "relatedThreads"
            | "queueEdit"
            | "sendState"
    )
}

fn native_write(action: &str) -> bool {
    matches!(
        action,
        "create"
            | "send"
            | "stop"
            | "approve"
            | "answer"
            | "markRead"
            | "pinThread"
            | "rename"
            | "deleteThread"
            | "compact"
            | "setGoal"
            | "updateGoal"
            | "createSideThread"
            | "closeSideThread"
            | "removeQueued"
            | "reorderQueued"
            | "beginQueuedEdit"
            | "editQueued"
            | "cancelQueuedEdit"
            | "dismissQueuedEdit"
            | "steerQueued"
            | "editMessage"
    )
}

struct DesktopReader {
    // 独立进程覆盖自己的状态目录后，仍只从原状态文件读取桌面连接信息。
    status_path: PathBuf,
}

impl DesktopReader {
    async fn read(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        anyhow::ensure!(native_read(action), "Codex 写操作必须通过派发授权检查");
        self.invoke(action, payload, None).await
    }

    async fn invoke(
        &self,
        action: &str,
        payload: Value,
        authorization: Option<&RemoteAuthorization>,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(
            native_read(action) || native_write(action),
            "此操作不属于桌面会话接口"
        );
        anyhow::ensure!(
            !native_write(action) || authorization.is_some(),
            "Codex 写操作缺少派发授权"
        );
        let status = crate::status::StatusStore::new(self.status_path.clone())
            .load_latest()?
            .filter(|s| matches!(s.status.as_str(), "running" | "running_degraded"))
            .context("原 Codex 桌面暂时不可读，请保持原程序运行")?;
        let port = status.debug_port.context("当前程序没有可读取的桌面连接")?;
        let targets = crate::cdp::list_targets(port).await?;
        let target = crate::cdp::pick_injectable_codex_page_target(&targets)?;
        let websocket = target
            .web_socket_debugger_url
            .context("当前桌面连接不可用")?;
        // 用户要求不打断原代理；复用已运行的原会话入口，绝不注入 NATIVE_SCRIPT、dispose 或重启原程序。
        let expression = format!(
            r#"(async () => {{
            const bridge = window.__codexElvesRemoteAccess;
            if (typeof bridge?.call !== "function") throw new Error("原程序的会话读取入口尚未就绪");
            return await bridge.call({}, {});
        }})()"#,
            serde_json::to_string(action)?,
            serde_json::to_string(&payload)?
        );
        let response = if let Some(authorization) = authorization {
            crate::bridge::evaluate_script_value_with_authorization(
                &websocket,
                &expression,
                Duration::from_secs(45),
                || authorization.prepare_dispatch(),
            )
            .await?
        } else {
            crate::bridge::evaluate_script_value_with_timeout(
                &websocket,
                &expression,
                Duration::from_secs(45),
            )
            .await?
        };
        anyhow::ensure!(
            response.pointer("/result/exceptionDetails").is_none(),
            "读取原 Codex 会话失败；未注入或替换原桌面脚本"
        );
        let value = response
            .pointer("/result/result/value")
            .cloned()
            .context("原会话读取没有返回结果")?;
        Ok(value)
    }
}

#[async_trait]
impl NativeRemoteBridge for ProjectSnapshot {
    async fn call(&self, action: &str, payload: Value) -> anyhow::Result<Value> {
        if native_read(action)
            && let Some(desktop) = &self.desktop
        {
            return desktop.read(action, payload).await;
        }
        match action {
            "sidebar" => Ok(self.sidebar.clone()),
            "workspaceContext" => {
                let project = self
                    .projects
                    .iter()
                    .find(|p| p["id"] == payload["projectId"])
                    .ok_or_else(|| ApiError::new(404, "验收清单中没有这个原项目"))?;
                let cwd = project["roots"][0]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ApiError::new(404, "原项目没有本机目录"))?;
                Ok(json!({"ok":true,"hostId":"local","projectId":project["id"],"cwd":cwd}))
            }
            "models" | "skills" => Ok(json!({"ok":true,"data":[],
                "defaultModel":"","defaultReasoningEffort":"","errors":[]})),
            // 原程序未运行时不能另起 Codex 兜底；有授权的写操作走下方已有桌面入口。
            _ => Err(ApiError::new(409, "此独立入口只验收 Claude，请在新建页选择 Claude").into()),
        }
    }

    async fn call_authorized(
        &self,
        action: &str,
        payload: Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if (native_read(action) || native_write(action))
            && let Some(desktop) = &self.desktop
        {
            return desktop.invoke(action, payload, Some(authorization)).await;
        }
        authorization.check().await?;
        self.call(action, payload).await
    }
}

fn isolated_directory(requested: &Path, live: &Path, codex_home: &Path) -> anyhow::Result<PathBuf> {
    // 创建前先解析已有父目录，不能在原程序目录写入后才判断是否重叠。
    let mut ancestor = std::path::absolute(requested)?;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().context("验收目录无效")?.to_owned());
        anyhow::ensure!(ancestor.pop(), "验收目录没有有效父目录");
    }
    let mut directory = std::fs::canonicalize(ancestor)?;
    for part in suffix.into_iter().rev() {
        directory.push(part);
    }
    let normalized = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    let candidate = normalized(&directory);
    for protected in [live, codex_home] {
        let protected = normalized(&std::fs::canonicalize(protected)?);
        let inside = |a: &str, b: &str| {
            a == b
                || a.strip_prefix(b)
                    .is_some_and(|tail| tail.starts_with(['\\', '/']))
        };
        anyhow::ensure!(
            !inside(&candidate, &protected) && !inside(&protected, &candidate),
            "验收目录不能与现有程序状态目录或 Codex 用户目录重叠"
        );
    }
    // canonicalize 的扩展路径前缀不能传给 WSL 的 wslpath；验收仅使用本机磁盘。
    #[cfg(windows)]
    {
        let text = directory.to_string_lossy();
        anyhow::ensure!(!text.starts_with(r"\\?\UNC\"), "验收目录必须位于本机磁盘");
        let directory = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
        std::fs::create_dir_all(&directory)?;
        Ok(directory)
    }
    #[cfg(not(windows))]
    {
        std::fs::create_dir_all(&directory)?;
        Ok(directory)
    }
}

fn allowed_write(action: &str, payload: &Value) -> bool {
    if action == "create" {
        return payload["backend"].is_null()
            || matches!(payload["backend"].as_str(), Some("codex" | "claude"));
    }
    if payload["threadId"]
        .as_str()
        .is_some_and(|id| !claude::Claude::owns_id(id))
    {
        return native_write(action);
    }
    matches!(
        action,
        "send"
            | "stop"
            // Claude 网页待发送队列由独立服务保存；这些操作仍由后端核对消息状态和设备授权。
            | "steerQueued"
            | "removeQueued"
            | "reorderQueued"
            | "beginQueuedEdit"
            | "editQueued"
            | "cancelQueuedEdit"
            | "dismissQueuedEdit"
            | "approve"
            | "answer"
            | "markRead"
            | "pinThread"
            | "rename"
            | "deleteThread"
    ) && payload["threadId"]
        .as_str()
        .is_some_and(claude::Claude::owns_id)
}

fn isolated_cookie(headers: &mut HeaderMap, name: &str) {
    let token = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split(';')
                .map(str::trim)
                .find_map(|cookie| cookie.strip_prefix(&format!("{name}=")))
        })
        .map(str::to_owned);
    // 浏览器 Cookie 不按端口隔离；不能读取或覆盖安装版的 codex_elves_device。
    headers.remove(header::COOKIE);
    if let Some(token) = token
        && let Ok(value) = HeaderValue::from_str(&format!("{}={token}", super::COOKIE_NAME))
    {
        headers.insert(header::COOKIE, value);
    }
}

async fn acceptance_boundary(
    State(cookie_name): State<String>,
    mut request: Request,
    next: Next,
) -> Response {
    isolated_cookie(request.headers_mut(), &cookie_name);
    let mut response = acceptance_request(request, next).await;
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();
    response.headers_mut().remove(header::SET_COOKIE);
    for cookie in cookies {
        let mapped = cookie
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix(&format!("{}=", super::COOKIE_NAME)))
            .and_then(|tail| HeaderValue::from_str(&format!("{cookie_name}={tail}")).ok())
            .unwrap_or(cookie);
        response.headers_mut().append(header::SET_COOKIE, mapped);
    }
    response
}

async fn acceptance_request(request: Request, next: Next) -> Response {
    let Some(action) = request
        .uri()
        .path()
        .strip_prefix("/api/action/")
        .map(str::to_owned)
    else {
        return next.run(request).await;
    };
    let (parts, body) = request.into_parts();
    let Ok(bytes) = to_bytes(body, super::MAX_ACTION_BODY_BYTES).await else {
        return ApiError::new(413, "验收请求超过大小限制").into_response();
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
        return ApiError::new(400, "请求 JSON 无效").into_response();
    };
    // 不开放会另起引擎的运行模式切换；Codex 复用原桌面，Claude 走独立 WSL。
    if !allowed_write(&action, &payload) {
        // 白名单拒绝发生在派发前；明确未提交，网页不能误留为“结果未知”并反复查询不存在的回执。
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"ok":false,"notSent":true,"message":"验收入口未开放此操作"})),
        )
            .into_response();
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// 供 debug example 使用；每个验收目录只运行一个进程。
pub async fn run(directory: PathBuf, port: u16) -> anyhow::Result<()> {
    anyhow::ensure!(port > 0, "验收端口无效");
    let live = crate::paths::default_app_state_dir();
    let desktop = Arc::new(DesktopReader {
        status_path: crate::paths::default_latest_status_path(),
    });
    let settings = SettingsStore::default().load()?;
    let codex_home = crate::codex_home::codex_home_dir_for_settings(&settings);
    let directory = isolated_directory(&directory, &live, &codex_home)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("acceptance.lock"))?;
    lock.try_lock_exclusive()
        .context("这个验收目录已经有后台在运行")?;
    let stop = directory.join(STOP_FILE);
    if stop.exists() {
        std::fs::remove_file(&stop)?;
    }
    // 先占用独立端口；失败立即退出，不能接管或关闭已有服务。
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .context("验收端口已占用，请选择其他端口；现有程序未停止")?;
    let snapshot = desktop.read("sidebar", json!({"recentLimit":80})).await?;
    let mut projects = ProjectSnapshot::from_value(&snapshot)?;
    let project_count = projects.projects.len();
    projects.desktop = Some(desktop.clone());
    let captured_at = now_ms();
    crate::settings::atomic_write(
        &directory.join("projects-snapshot.json"),
        &serde_json::to_vec_pretty(&json!({"capturedAt":captured_at,"source":snapshot}))?,
    )?;
    // 只覆盖本进程的路径，保证诊断等辅助模块也不写原程序状态；不改变系统环境变量。
    crate::paths::set_app_state_dir_for_tests(Some(directory.clone()));
    crate::paths::set_settings_path_for_tests(Some(directory.join("settings.json")));
    super::execution::require_explicit_cdp();
    let key_path = directory.join("login-key.txt");
    let key = match std::fs::read_to_string(&key_path) {
        Ok(value) => value.trim().to_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let value = format!("elves-test-{}", uuid::Uuid::new_v4().simple());
            crate::settings::atomic_write(&key_path, value.as_bytes())?;
            value
        }
        Err(error) => return Err(error.into()),
    };
    super::configure_access_key(&directory, &key)?;
    // 本入口不调用 reconcile/start_monitor，避免预热 Codex 或触发 zrok / FRP / ngrok。
    let controller = RemoteAccessController::new(Arc::new(projects), directory.clone())?;
    anyhow::ensure!(
        controller
            .runtime_modes
            .read()
            .await
            .values()
            .all(|mode| mode.cdp_injection_enabled),
        "验收目录含有服务模式切换记录，请使用新的独立目录"
    );
    let remote_settings = RemoteAccessSettings {
        enabled: true,
        port,
        allow_approvals: true,
        ..Default::default()
    };
    controller.publish_access_policy(&remote_settings);
    controller.running.store(true, Ordering::SeqCst);
    // 原桌面会话保持可写；只连已有入口，不启动或替换原 launcher / 本地代理。
    controller.native_connected.store(true, Ordering::SeqCst);
    controller
        .native_write_available
        .store(true, Ordering::SeqCst);
    let weak = Arc::downgrade(&controller);
    let event_pump = tokio::spawn(async move {
        let mut cursor = 0;
        let mut epoch = String::new();
        while let Some(controller) = weak.upgrade() {
            if !controller.running.load(Ordering::SeqCst) {
                break;
            }
            let payload = json!({"cursor":cursor,"epoch":epoch});
            let scope = super::execution::Scope::new(
                controller.harness.audit.clone(),
                false,
                "events",
                &payload,
                None,
                None,
            );
            let result = scope.recorded(desktop.read("events", payload)).await;
            if let Ok(response) = result
                && response["ok"] == true
            {
                cursor = response["cursor"].as_u64().unwrap_or(cursor);
                epoch = response["epoch"].as_str().unwrap_or(&epoch).to_owned();
                if response["reset"] == true {
                    controller.publish_backend_event("cdp", json!({"method":"remote/resync"}));
                }
                for event in response["events"].as_array().into_iter().flatten() {
                    let mut event = event.clone();
                    controller.attach_event_images(&mut event);
                    controller.publish_backend_event("cdp", event);
                }
            }
            drop(controller);
            tokio::time::sleep(Duration::from_millis(650)).await;
        }
    });
    let process_id = std::process::id();
    let health = json!({"mode":"claude-acceptance","processId":process_id,
        "port":port,"projectCount":project_count,"capturedAt":captured_at});
    let router = web_router(controller.clone(), remote_settings)
        .route(
            "/acceptance/health",
            get(move || async move { Json(health) }),
        )
        .layer(middleware::from_fn_with_state(
            format!("codex_elves_acceptance_device_{port}"),
            acceptance_boundary,
        ));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    });
    let manifest = json!({"processId":process_id,"port":port,"url":format!("http://127.0.0.1:{port}/"),
        "stateDirectory":directory,"loginKeyFile":key_path,"projectCount":project_count,
        "capturedAt":captured_at,"mode":"claude-acceptance","stopped":false});
    crate::settings::atomic_write(
        &directory.join("acceptance.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Claude 本机验收入口：http://127.0.0.1:{port}/");
    println!(
        "项目数：{project_count}；登录密钥文件：{}",
        key_path.display()
    );
    println!("停止方式：创建 {}", stop.display());
    while !stop.is_file() {
        anyhow::ensure!(!server.is_finished(), "验收 HTTP 服务意外停止");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    controller.running.store(false, Ordering::SeqCst);
    controller.publish_access_policy(&RemoteAccessSettings::default());
    event_pump.abort();
    let _ = event_pump.await;
    server.abort();
    let _ = server.await;
    controller.claude.release().await;
    let mut manifest = manifest;
    manifest["stopped"] = json!(true);
    crate::settings::atomic_write(
        &directory.join("acceptance.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("独立验收后台已停止，原程序与代理保持运行");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acceptance_preserves_identity_and_rejects_writes_without_a_live_authorized_bridge() {
        let source = json!({"projects":[{"id":"original-id","name":"原项目",
            "roots":["E:\\shared"],"threads":[{"id":"old-codex-thread"}]}]});
        let bridge = ProjectSnapshot::from_value(&source).unwrap();
        let sidebar = bridge.call("sidebar", json!({})).await.unwrap();
        assert_eq!(sidebar["projects"][0]["id"], "original-id");
        assert_eq!(
            sidebar["projects"][0]["threads"][0]["id"],
            "old-codex-thread"
        );
        assert_ne!(sidebar["projects"][0]["threads"][0]["readOnly"], true);
        let context = bridge
            .call("workspaceContext", json!({"projectId":"original-id"}))
            .await
            .unwrap();
        assert_eq!(context["cwd"], "E:\\shared");
        assert!(bridge.call("create", json!({})).await.is_err());
        assert!(bridge.call("dispose", json!({})).await.is_err());
    }

    #[test]
    fn acceptance_native_actions_never_manage_or_replace_the_existing_process() {
        assert!(native_read("history"));
        assert!(native_write("send"));
        assert!(native_write("stop"));
        for action in ["dispose", "resume", "setRuntime", "restart"] {
            assert!(!native_write(action) && !native_read(action));
        }
    }

    #[test]
    fn acceptance_write_boundary_does_not_allow_other_engines_or_runtime_switching() {
        let thread = format!("claude-{}", uuid::Uuid::new_v4());
        assert!(allowed_write("create", &json!({"backend":"claude"})));
        assert!(allowed_write("send", &json!({"threadId":thread})));
        assert!(allowed_write(
            "steerQueued",
            &json!({"threadId":thread,"messageId":"queued"})
        ));
        assert!(allowed_write("reorderQueued", &json!({"threadId":thread})));
        assert!(allowed_write("editQueued", &json!({"threadId":thread})));
        assert!(allowed_write("create", &json!({})));
        assert!(allowed_write(
            "send",
            &json!({"threadId":uuid::Uuid::new_v4().to_string()})
        ));
        assert!(!allowed_write("setRuntime", &json!({"threadId":thread})));
        assert!(!allowed_write(
            "optimizePrompt",
            &json!({"threadId":thread})
        ));
    }

    #[test]
    fn acceptance_cookies_do_not_reuse_the_installed_service_or_other_test_ports() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static(
            "codex_elves_device=installed; codex_elves_acceptance_device_47137=other; codex_elves_acceptance_device_47136=test"));
        isolated_cookie(&mut headers, "codex_elves_acceptance_device_47136");
        assert_eq!(headers[header::COOKIE], "codex_elves_device=test");
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("codex_elves_device=installed"),
        );
        isolated_cookie(&mut headers, "codex_elves_acceptance_device_47136");
        assert!(!headers.contains_key(header::COOKIE));
    }

    #[test]
    fn acceptance_state_directory_must_not_overlap_live_state() {
        let root =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp")).unwrap();
        let live = root.path().join("live");
        let codex = root.path().join("codex");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        assert!(isolated_directory(&live, &live, &codex).is_err());
        assert!(isolated_directory(&live.join("nested"), &live, &codex).is_err());
        assert!(!live.join("nested").exists());
        assert!(isolated_directory(root.path(), &live, &codex).is_err());
        assert!(isolated_directory(&root.path().join("acceptance"), &live, &codex).is_ok());
    }
}
