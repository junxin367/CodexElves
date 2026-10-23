use std::fs::{File, OpenOptions};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::{DefaultBodyLimit, Path as RoutePath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const ENDPOINT_FILE: &str = "remote-access-service.json";
const LOCK_FILE: &str = "remote-access-service.lock";
const PROTOCOL_VERSION: u32 = 2;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(windows)]
static OWNED_SERVICE_JOB: OnceLock<Mutex<Option<std::os::windows::io::OwnedHandle>>> =
    OnceLock::new();

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Endpoint {
    protocol_version: u32,
    process_id: u32,
    port: u16,
    token: String,
}

#[derive(Clone)]
struct ServiceState {
    endpoint: Endpoint,
}

fn acquire_service_lock(directory: &Path) -> anyhow::Result<Option<File>> {
    std::fs::create_dir_all(directory)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(LOCK_FILE))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || matches!(error.raw_os_error(), Some(32 | 33)) =>
        {
            Ok(None)
        }
        Err(error) => Err(error).context("无法锁定远程访问后台"),
    }
}

fn load_endpoint_any(directory: &Path) -> anyhow::Result<Option<Endpoint>> {
    let bytes = match std::fs::read(directory.join(ENDPOINT_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("无法读取远程访问后台状态"),
    };
    Ok(serde_json::from_slice::<Endpoint>(&bytes)
        .ok()
        .filter(|endpoint| {
            endpoint.protocol_version > 0
                && endpoint.process_id > 0
                && endpoint.port > 0
                && endpoint.token.len() == 64
                && endpoint.token.bytes().all(|byte| byte.is_ascii_hexdigit())
        }))
}

fn load_endpoint(directory: &Path) -> anyhow::Result<Option<Endpoint>> {
    Ok(load_endpoint_any(directory)?
        .filter(|endpoint| endpoint.protocol_version == PROTOCOL_VERSION))
}

async fn endpoint_is_active(endpoint: &Endpoint) -> anyhow::Result<bool> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()?;
    let Ok(response) = client
        .get(format!("http://127.0.0.1:{}/health", endpoint.port))
        .bearer_auth(&endpoint.token)
        .send()
        .await
    else {
        return Ok(false);
    };
    if !response.status().is_success() {
        return Ok(false);
    }
    let Ok(value) = response.json::<Value>().await else {
        return Ok(false);
    };
    Ok(value["protocolVersion"] == endpoint.protocol_version
        && value["processId"] == endpoint.process_id)
}

async fn active_endpoint(directory: &Path) -> anyhow::Result<Option<Endpoint>> {
    let Some(endpoint) = load_endpoint(directory)? else {
        return Ok(None);
    };
    Ok(endpoint_is_active(&endpoint).await?.then_some(endpoint))
}

async fn retire_obsolete_service(directory: &Path) -> anyhow::Result<()> {
    let Some(endpoint) = load_endpoint_any(directory)? else {
        return Ok(());
    };
    if endpoint.protocol_version == PROTOCOL_VERSION || !endpoint_is_active(&endpoint).await? {
        return Ok(());
    }
    #[cfg(windows)]
    {
        anyhow::ensure!(
            crate::windows_integration::terminate_process(endpoint.process_id),
            "旧版远程访问后台仍在运行，请退出 CodexElves 后重试"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !endpoint_is_active(&endpoint).await.unwrap_or(false) {
                let _ = std::fs::remove_file(directory.join(ENDPOINT_FILE));
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        anyhow::bail!("旧版远程访问后台未能停止，请退出 CodexElves 后重试");
    }
    #[cfg(not(windows))]
    anyhow::bail!("旧版远程访问后台仍在运行，请退出 CodexElves 后重试")
}

pub async fn ensure_running() -> anyhow::Result<()> {
    let directory = crate::paths::default_app_state_dir();
    retire_obsolete_service(&directory).await?;
    if active_endpoint(&directory).await?.is_some() {
        return Ok(());
    }
    let available = acquire_service_lock(&directory)?;
    let mut child = if available.is_some() {
        drop(available);
        let program = crate::install::companion_binary_path(crate::install::MANAGER_BINARY);
        anyhow::ensure!(program.is_file(), "未找到管理器程序，无法启动远程访问后台");
        let mut command = Command::new(program);
        command
            .arg("--remote-access-service")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        Some(command.spawn().context("无法启动独立远程访问后台")?)
    } else {
        None
    };
    #[cfg(windows)]
    let mut owned_job = if let Some(child) = child.as_mut() {
        match remote_service_job(child) {
            Ok(job) => Some(job),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    } else {
        None
    };
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if active_endpoint(&directory).await?.is_some() {
            #[cfg(windows)]
            if let Some(job) = owned_job.take() {
                *OWNED_SERVICE_JOB
                    .get_or_init(|| Mutex::new(None))
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(job);
            }
            return Ok(());
        }
        if let Some(child) = child.as_mut()
            && let Some(status) = child.try_wait()?
            && !status.success()
        {
            anyhow::bail!("远程访问后台启动失败（{status}），请查看诊断日志");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::bail!("远程访问后台尚未就绪，请稍后重试；现有隧道未被重启")
}

fn supported_action(action: &str) -> bool {
    matches!(
        action,
        "status"
            | "apply"
            | "revoke"
            | "diagnostics"
            | "downloadZrok"
            | "registerZrok"
            | "startZrok"
            | "testZrok"
            | "saveNgrok"
            | "saveFrp"
            | "testPrimaryRoute"
            | "applyPrimaryRoute"
    )
}

async fn request(endpoint: &Endpoint, action: &str, payload: &Value) -> anyhow::Result<Value> {
    anyhow::ensure!(supported_action(action), "不支持的远程管理操作");
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(150))
        .build()?
        .post(format!("http://127.0.0.1:{}/admin/{action}", endpoint.port))
        .bearer_auth(&endpoint.token)
        .json(payload)
        .send()
        .await
        .context("远程访问后台未返回结果，请先刷新状态，不要重复执行操作")?
        .error_for_status()
        .context("远程访问后台拒绝了管理请求")?;
    response.json().await.context("远程访问后台返回了无效状态")
}

pub(super) async fn try_request(action: &str, payload: &Value) -> anyhow::Result<Option<Value>> {
    let Some(endpoint) = active_endpoint(&crate::paths::default_app_state_dir()).await? else {
        return Ok(None);
    };
    request(&endpoint, action, payload).await.map(Some)
}

pub(super) async fn request_or_start(action: &str, payload: Value) -> anyhow::Result<Value> {
    anyhow::ensure!(supported_action(action), "不支持的远程管理操作");
    ensure_running().await?;
    try_request(action, &payload)
        .await?
        .context("远程访问后台连接已断开，请刷新状态后重试")
}

fn authorized(headers: &HeaderMap, endpoint: &Endpoint) -> bool {
    !headers.contains_key(header::ORIGIN)
        && headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|token| super::constant_time_eq(token, &endpoint.token))
}

async fn health(
    State(state): State<ServiceState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let endpoint = &state.endpoint;
    if !authorized(&headers, endpoint) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Json(json!({
        "status":"ok",
        "protocolVersion":endpoint.protocol_version,
        "processId":endpoint.process_id,
    })))
}

async fn admin(
    State(state): State<ServiceState>,
    RoutePath(action): RoutePath<String>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let endpoint = &state.endpoint;
    if !authorized(&headers, endpoint) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !supported_action(&action) {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut response = super::admin_request(&action, payload).await;
    response["serviceProcessId"] = json!(endpoint.process_id);
    Ok(Json(response))
}

async fn save_frp(
    state: State<ServiceState>,
    headers: HeaderMap,
    payload: Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    admin(state, RoutePath("saveFrp".into()), headers, payload).await
}

pub async fn run() -> anyhow::Result<()> {
    let directory = crate::paths::default_app_state_dir();
    let Some(_lock) = acquire_service_lock(&directory)? else {
        return Ok(());
    };
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = Endpoint {
        protocol_version: PROTOCOL_VERSION,
        process_id: std::process::id(),
        port: listener.local_addr()?.port(),
        token: super::random_token(),
    };
    let state = ServiceState {
        endpoint: endpoint.clone(),
    };
    let router = Router::new()
        .route("/health", get(health))
        .route("/admin/{action}", post(admin))
        .route(
            "/admin/saveFrp",
            post(save_frp).layer(DefaultBodyLimit::max(160 * 1024)),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(state);
    let server = axum_server::from_tcp(listener)?.serve(router.into_make_service());
    crate::settings::atomic_write(
        &directory.join(ENDPOINT_FILE),
        &serde_json::to_vec(&endpoint)?,
    )?;
    let monitor = super::start_monitor()?;
    let result = server.await;
    let shutdown = tokio::time::timeout(SHUTDOWN_TIMEOUT, monitor.shutdown()).await;
    std::fs::remove_file(directory.join(ENDPOINT_FILE))
        .context("无法清理已停止的远程访问后台状态")?;
    shutdown.context("远程访问后台清理超时，进程将退出并回收隧道")?;
    result.context("远程访问后台管理接口已停止")
}

#[cfg(windows)]
fn remote_service_job(
    child: &std::process::Child,
) -> anyhow::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::core::PCWSTR;

    let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .context("无法创建远程访问后台生命周期保护")?;
    let job = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )?;
        AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle()))?;
    }
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DIRECTORY_ENV: &str = "CODEX_ELVES_REMOTE_SERVICE_TEST_DIRECTORY";

    fn test_directory() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("remote-service-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../temp"))
            .unwrap()
    }

    fn configure_test_paths() -> Option<std::path::PathBuf> {
        let directory = std::env::var_os(TEST_DIRECTORY_ENV).map(std::path::PathBuf::from)?;
        crate::paths::set_settings_path_for_tests(Some(directory.join("settings.json")));
        crate::paths::set_app_state_dir_for_tests(Some(directory.clone()));
        Some(directory)
    }

    #[test]
    fn only_one_service_can_own_a_state_directory() {
        let directory = test_directory();
        let first = acquire_service_lock(directory.path()).unwrap().unwrap();
        assert!(acquire_service_lock(directory.path()).unwrap().is_none());
        drop(first);
        assert!(acquire_service_lock(directory.path()).unwrap().is_some());
    }

    #[test]
    fn codex_restart_does_not_target_the_remote_service_host() {
        let selected = crate::watcher::filter_killable_launcher_processes(
            [
                (10, 1, "codex-elves-manager.exe"),
                (20, 10, "codex-elves.exe"),
                (30, 20, "codex-elves-manager.exe"),
            ],
            10,
        );
        assert_eq!(selected, vec![20]);
    }

    #[test]
    fn rejects_invalid_service_endpoints() {
        let directory = test_directory();
        for value in [
            json!({"protocolVersion":PROTOCOL_VERSION,"processId":1,"port":0,"token":"a".repeat(64)}),
            json!({"protocolVersion":PROTOCOL_VERSION + 1,"processId":1,"port":12345,"token":"a".repeat(64)}),
            json!({"protocolVersion":PROTOCOL_VERSION,"processId":1,"port":12345,"token":"bad\r\nheader"}),
        ] {
            std::fs::write(
                directory.path().join(ENDPOINT_FILE),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
            assert!(load_endpoint(directory.path()).unwrap().is_none());
        }
    }

    #[test]
    fn management_requires_token_and_rejects_browser_origins() {
        let endpoint = Endpoint {
            protocol_version: PROTOCOL_VERSION,
            process_id: 1,
            port: 12345,
            token: "a".repeat(64),
        };
        let mut headers = HeaderMap::new();
        assert!(!authorized(&headers, &endpoint));
        headers.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert!(!authorized(&headers, &endpoint));
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", endpoint.token).parse().unwrap(),
        );
        assert!(authorized(&headers, &endpoint));
        headers.insert(header::ORIGIN, "http://127.0.0.1:12345".parse().unwrap());
        assert!(!authorized(&headers, &endpoint));
    }

    #[tokio::test]
    #[ignore = "仅由隔离后台进程测试调用"]
    async fn runtime_worker() {
        if configure_test_paths().is_some() {
            run().await.unwrap();
        }
    }

    fn test_command(name: &str, directory: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                &format!("remote_access::service::tests::{name}"),
                "--ignored",
                "--nocapture",
            ])
            .env(TEST_DIRECTORY_ENV, directory)
            .env("PATH", "")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        command
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn owned_service_job_stops_service_and_releases_ports() {
        let directory = test_directory();
        let remote_port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut settings = crate::settings::BackendSettings::default();
        settings.remote_access.enabled = true;
        settings.remote_access.port = remote_port;
        crate::settings::SettingsStore::new(directory.path().join("settings.json"))
            .save(&settings)
            .unwrap();

        let manager_guard = crate::ports::LoopbackPortGuard::listener(
            std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap(),
        );
        let manager_wake = manager_guard.shared_listener().unwrap();
        let launcher_guard = crate::ports::LoopbackPortGuard::listener(
            std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap(),
        );
        let launcher_control = launcher_guard.shared_listener().unwrap();
        let helper = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let starter_ports = [
            manager_wake.local_addr().unwrap().port(),
            launcher_control.local_addr().unwrap().port(),
            helper.local_addr().unwrap().port(),
        ];

        let mut child = test_command("runtime_worker", directory.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let job = remote_service_job(&child).unwrap();
        let endpoint = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(endpoint) = active_endpoint(directory.path()).await.unwrap() {
                    return endpoint;
                }
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "service exited before startup"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();

        drop(manager_wake);
        drop(manager_guard);
        drop(launcher_control);
        drop(launcher_guard);
        drop(helper);
        for port in starter_ports {
            assert!(
                std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
                "remote service retained starter port {port}"
            );
        }

        let status = request(&endpoint, "status", &json!({})).await.unwrap();
        assert_eq!(status["status"], "ok");
        assert_eq!(status["running"], true);
        assert_eq!(status["serviceProcessId"], endpoint.process_id);
        assert_eq!(status["zrok"]["installed"], false);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let health_url = format!("http://127.0.0.1:{}/health", endpoint.port);
        assert_eq!(
            client.get(&health_url).send().await.unwrap().status(),
            reqwest::StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .get(&health_url)
                .bearer_auth(&endpoint.token)
                .header(reqwest::header::ORIGIN, "http://localhost")
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN
        );

        drop(job);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if child.try_wait().unwrap().is_some() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        assert!(active_endpoint(directory.path()).await.unwrap().is_none());
        assert!(acquire_service_lock(directory.path()).unwrap().is_some());
        assert!(std::net::TcpListener::bind(("127.0.0.1", remote_port)).is_ok());
        assert!(std::net::TcpListener::bind(("127.0.0.1", endpoint.port)).is_ok());
    }
}
