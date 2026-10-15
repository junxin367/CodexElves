use std::fs::{File, OpenOptions};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::{DefaultBodyLimit, Path as RoutePath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

const ENDPOINT_FILE: &str = "remote-access-service.json";
const LOCK_FILE: &str = "remote-access-service.lock";
const PROTOCOL_VERSION: u32 = 1;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
const MANAGER_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(60);

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
    manager_heartbeat: watch::Sender<Instant>,
    heartbeat_timeout: Duration,
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

fn load_endpoint(directory: &Path) -> anyhow::Result<Option<Endpoint>> {
    let bytes = match std::fs::read(directory.join(ENDPOINT_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("无法读取远程访问后台状态"),
    };
    Ok(serde_json::from_slice::<Endpoint>(&bytes)
        .ok()
        .filter(|endpoint| {
            endpoint.protocol_version == PROTOCOL_VERSION
                && endpoint.process_id > 0
                && endpoint.port > 0
                && endpoint.token.len() == 64
                && endpoint.token.bytes().all(|byte| byte.is_ascii_hexdigit())
        }))
}

async fn active_endpoint(directory: &Path) -> anyhow::Result<Option<Endpoint>> {
    let Some(endpoint) = load_endpoint(directory)? else {
        return Ok(None);
    };
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
        return Ok(None);
    };
    if !response.status().is_success() {
        return Ok(None);
    }
    let Ok(value) = response.json::<Value>().await else {
        return Ok(None);
    };
    Ok(
        (value["protocolVersion"] == PROTOCOL_VERSION && value["processId"] == endpoint.process_id)
            .then_some(endpoint),
    )
}

pub async fn send_manager_heartbeat() -> anyhow::Result<()> {
    send_heartbeat(&crate::paths::default_app_state_dir()).await
}

async fn send_heartbeat(directory: &Path) -> anyhow::Result<()> {
    let Some(endpoint) = active_endpoint(directory).await? else {
        return Ok(());
    };
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?
        .post(format!("http://127.0.0.1:{}/heartbeat", endpoint.port))
        .bearer_auth(&endpoint.token)
        .send()
        .await
        .context("无法向远程访问后台发送管理器心跳")?
        .error_for_status()
        .context("远程访问后台拒绝了管理器心跳")?;
    Ok(())
}

pub async fn ensure_running() -> anyhow::Result<()> {
    let directory = crate::paths::default_app_state_dir();
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
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if active_endpoint(&directory).await?.is_some() {
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
        "status" | "apply" | "revoke" | "restartQuickTunnel" | "testQuickTunnel"
    )
}

async fn request(endpoint: &Endpoint, action: &str, payload: &Value) -> anyhow::Result<Value> {
    anyhow::ensure!(supported_action(action), "不支持的远程管理操作");
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(60))
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

async fn heartbeat(
    State(state): State<ServiceState>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !authorized(&headers, &state.endpoint) {
        return Err(StatusCode::FORBIDDEN);
    }
    let renewed = state.manager_heartbeat.send_if_modified(|last_seen| {
        let now = Instant::now();
        if now.duration_since(*last_seen) >= state.heartbeat_timeout {
            return false;
        }
        *last_seen = now;
        true
    });
    if !renewed {
        return Err(StatusCode::GONE);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn wait_for_manager_timeout(mut heartbeat: watch::Receiver<Instant>, timeout: Duration) {
    loop {
        let remaining = timeout.saturating_sub(heartbeat.borrow_and_update().elapsed());
        if remaining.is_zero() {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(remaining) => {}
            result = heartbeat.changed() => {
                if result.is_err() {
                    return;
                }
            }
        }
    }
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

pub async fn run() -> anyhow::Result<()> {
    run_with_heartbeat_timeout(MANAGER_HEARTBEAT_TIMEOUT).await
}

async fn run_with_heartbeat_timeout(heartbeat_timeout: Duration) -> anyhow::Result<()> {
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
    let (manager_heartbeat, heartbeat_receiver) = watch::channel(Instant::now());
    let state = ServiceState {
        endpoint: endpoint.clone(),
        manager_heartbeat,
        heartbeat_timeout,
    };
    let router = Router::new()
        .route("/health", get(health))
        .route("/heartbeat", post(heartbeat))
        .route("/admin/{action}", post(admin))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(state);
    let handle = axum_server::Handle::new();
    let server = axum_server::from_tcp(listener)?
        .handle(handle.clone())
        .serve(router.into_make_service());
    crate::settings::atomic_write(
        &directory.join(ENDPOINT_FILE),
        &serde_json::to_vec(&endpoint)?,
    )?;
    let monitor = super::start_monitor()?;
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result,
        _ = wait_for_manager_timeout(heartbeat_receiver, heartbeat_timeout) => {
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.manager_heartbeat_expired",
                json!({
                    "processId": endpoint.process_id,
                    "timeoutSeconds": heartbeat_timeout.as_secs(),
                }),
            );
            handle.shutdown();
            server.await
        }
    };
    let shutdown = tokio::time::timeout(SHUTDOWN_TIMEOUT, monitor.shutdown()).await;
    std::fs::remove_file(directory.join(ENDPOINT_FILE))
        .context("无法清理已停止的远程访问后台状态")?;
    shutdown.context("远程访问后台清理超时，进程将退出并回收隧道")?;
    result.context("远程访问后台管理接口已停止")
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
            json!({"protocolVersion":1,"processId":1,"port":0,"token":"a".repeat(64)}),
            json!({"protocolVersion":2,"processId":1,"port":12345,"token":"a".repeat(64)}),
            json!({"protocolVersion":1,"processId":1,"port":12345,"token":"bad\r\nheader"}),
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
            protocol_version: 1,
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
    async fn only_authenticated_manager_heartbeats_renew_the_three_hour_deadline() {
        assert_eq!(MANAGER_HEARTBEAT_TIMEOUT, Duration::from_secs(10_800));
        let last_seen = Instant::now() - Duration::from_secs(60);
        let (manager_heartbeat, _receiver) = watch::channel(last_seen);
        let state = ServiceState {
            endpoint: Endpoint {
                protocol_version: PROTOCOL_VERSION,
                process_id: 1,
                port: 12345,
                token: "a".repeat(64),
            },
            manager_heartbeat,
            heartbeat_timeout: MANAGER_HEARTBEAT_TIMEOUT,
        };
        let mut headers = HeaderMap::new();
        assert_eq!(
            heartbeat(State(state.clone()), headers.clone()).await,
            Err(StatusCode::FORBIDDEN)
        );
        headers.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert_eq!(
            heartbeat(State(state.clone()), headers.clone()).await,
            Err(StatusCode::FORBIDDEN)
        );
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", state.endpoint.token).parse().unwrap(),
        );
        headers.insert(header::ORIGIN, "http://localhost".parse().unwrap());
        assert_eq!(
            heartbeat(State(state.clone()), headers.clone()).await,
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(*state.manager_heartbeat.borrow(), last_seen);
        headers.remove(header::ORIGIN);
        let health = health(State(state.clone()), headers.clone()).await.unwrap();
        assert_eq!(health.0["status"], "ok");
        assert_eq!(*state.manager_heartbeat.borrow(), last_seen);
        assert_eq!(
            heartbeat(State(state.clone()), headers.clone()).await,
            Ok(StatusCode::NO_CONTENT)
        );
        assert!(*state.manager_heartbeat.borrow() > last_seen);

        let expired = Instant::now() - MANAGER_HEARTBEAT_TIMEOUT;
        state.manager_heartbeat.send_replace(expired);
        assert_eq!(
            heartbeat(State(state.clone()), headers).await,
            Err(StatusCode::GONE)
        );
        assert_eq!(*state.manager_heartbeat.borrow(), expired);
    }

    #[tokio::test]
    async fn watchdog_waits_until_the_latest_heartbeat_expires() {
        let (heartbeat, receiver) =
            watch::channel(Instant::now() - MANAGER_HEARTBEAT_TIMEOUT + Duration::from_millis(100));
        let watchdog = wait_for_manager_timeout(receiver, MANAGER_HEARTBEAT_TIMEOUT);
        tokio::pin!(watchdog);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut watchdog)
                .await
                .is_err()
        );
        heartbeat.send_replace(Instant::now());
        assert!(
            tokio::time::timeout(Duration::from_millis(150), &mut watchdog)
                .await
                .is_err()
        );
        heartbeat.send_replace(Instant::now() - MANAGER_HEARTBEAT_TIMEOUT);
        tokio::time::timeout(Duration::from_secs(1), watchdog)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn manager_heartbeat_does_not_start_a_missing_service() {
        let directory = test_directory();
        send_heartbeat(directory.path()).await.unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    #[ignore = "仅由隔离后台进程测试调用"]
    async fn runtime_worker() {
        if configure_test_paths().is_some() {
            run().await.unwrap();
        }
    }

    #[tokio::test]
    #[ignore = "仅由隔离心跳超时测试调用"]
    async fn expiring_runtime_worker() {
        if configure_test_paths().is_some() {
            run_with_heartbeat_timeout(Duration::from_secs(3))
                .await
                .unwrap();
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
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
        command
    }

    #[test]
    #[ignore = "仅由隔离后台进程测试调用"]
    fn starter_worker() {
        let Some(directory) = configure_test_paths() else {
            return;
        };
        let child = test_command("runtime_worker", &directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        std::fs::write(directory.join("worker-result.txt"), child.id().to_string()).unwrap();
    }

    #[tokio::test]
    #[ignore = "仅由隔离后台进程测试调用"]
    async fn client_worker() {
        let Some(directory) = configure_test_paths() else {
            return;
        };
        ensure_running().await.unwrap();
        let status = super::super::request_local_admin("status", json!({}))
            .await
            .unwrap();
        assert_eq!(status["status"], "ok");
        std::fs::write(
            directory.join("worker-result.txt"),
            status["serviceProcessId"].to_string(),
        )
        .unwrap();
    }

    fn run_test_process(name: &str, directory: &Path) -> u32 {
        let status = test_command(name, directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read_to_string(directory.join("worker-result.txt"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    #[tokio::test]
    async fn service_exits_and_releases_ports_when_manager_heartbeats_stop() {
        use tokio::io::AsyncWriteExt;

        let directory = test_directory();
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut settings = crate::settings::BackendSettings::default();
        settings.remote_access.enabled = true;
        settings.remote_access.port = port;
        settings.remote_access.cloudflare_quick_tunnel_enabled = true;
        let store = crate::settings::SettingsStore::new(directory.path().join("settings.json"));
        store.save(&settings).unwrap();
        let cooldown_path = directory
            .path()
            .join("remote-access-quick-tunnel-cooldown.json");
        let cooldown = serde_json::to_vec(&json!({
            "retryAtMs": super::super::now_ms() + 24 * 60 * 60_000,
        }))
        .unwrap();
        std::fs::write(&cooldown_path, &cooldown).unwrap();
        let mut child = tokio::process::Command::from(test_command(
            "expiring_runtime_worker",
            directory.path(),
        ))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
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
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let heartbeat_url = format!("http://127.0.0.1:{}/heartbeat", endpoint.port);
        let keep_alive_until = Instant::now() + Duration::from_secs(4);
        while Instant::now() < keep_alive_until {
            send_heartbeat(directory.path()).await.unwrap();
            assert!(child.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let retained = active_endpoint(directory.path()).await.unwrap().unwrap();
        assert_eq!(retained.process_id, endpoint.process_id);
        assert_eq!(retained.port, endpoint.port);
        assert!(retained.token == endpoint.token);
        let status = request(&endpoint, "status", &json!({})).await.unwrap();
        assert_eq!(status["running"], true);
        assert_eq!(status["cloudflareQuickTunnel"]["rateLimited"], true);
        assert!(status["cloudflareQuickTunnel"]["processId"].is_null());

        let mut unfinished_request = tokio::net::TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .unwrap();
        unfinished_request
            .write_all(
                b"POST /admin/status HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1024\r\n",
            )
            .await
            .unwrap();
        let traffic = async {
            loop {
                let _ = active_endpoint(directory.path()).await.unwrap();
                let _ = request(&endpoint, "status", &json!({})).await;
                let _ = client.get(format!("http://127.0.0.1:{port}/")).send().await;
                for origin in [None, Some("http://localhost")] {
                    let mut request = client.post(&heartbeat_url);
                    if let Some(origin) = origin {
                        request = request
                            .bearer_auth(&endpoint.token)
                            .header(header::ORIGIN, origin);
                    }
                    if let Ok(response) = request.send().await {
                        assert_eq!(response.status(), StatusCode::FORBIDDEN);
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let stopped = tokio::time::timeout(Duration::from_secs(6), async {
            tokio::select! {
                status = child.wait() => status.unwrap(),
                _ = traffic => unreachable!(),
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "service shutdown wait failed: {error}; process={:?}; endpoint_exists={}",
                child.try_wait().unwrap(),
                directory.path().join(ENDPOINT_FILE).exists(),
            )
        });
        assert!(stopped.success());
        assert!(!directory.path().join(ENDPOINT_FILE).exists());
        assert!(acquire_service_lock(directory.path()).unwrap().is_some());
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
        assert!(std::net::TcpListener::bind(("127.0.0.1", endpoint.port)).is_ok());
        assert_eq!(std::fs::read(&cooldown_path).unwrap(), cooldown);
        assert!(store.load().unwrap().remote_access.enabled);
        send_heartbeat(directory.path()).await.unwrap();
        assert!(!directory.path().join(ENDPOINT_FILE).exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn service_survives_its_starter_and_reuses_one_instance_for_restarted_clients() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
            WaitForSingleObject,
        };

        struct WorkerGuard(OwnedHandle);
        impl Drop for WorkerGuard {
            fn drop(&mut self) {
                let handle = HANDLE(self.0.as_raw_handle());
                unsafe {
                    let _ = TerminateProcess(handle, 0);
                    WaitForSingleObject(handle, 5000);
                }
            }
        }

        let directory = test_directory();
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut settings = crate::settings::BackendSettings::default();
        settings.remote_access.enabled = true;
        settings.remote_access.port = port;
        settings.remote_access.cloudflare_quick_tunnel_enabled = true;
        crate::settings::SettingsStore::new(directory.path().join("settings.json"))
            .save(&settings)
            .unwrap();
        let cooldown_until = super::super::now_ms() + 24 * 60 * 60_000;
        std::fs::write(
            directory
                .path()
                .join("remote-access-quick-tunnel-cooldown.json"),
            serde_json::to_vec(&json!({"retryAtMs":cooldown_until})).unwrap(),
        )
        .unwrap();

        let process_id = run_test_process("starter_worker", directory.path());
        let handle =
            unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, process_id) }
                .unwrap();
        let worker = WorkerGuard(unsafe { OwnedHandle::from_raw_handle(handle.0) });
        let endpoint = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(endpoint) = active_endpoint(directory.path()).await.unwrap() {
                    return endpoint;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(endpoint.process_id, process_id);
        let status = request(&endpoint, "status", &json!({})).await.unwrap();
        assert_eq!(status["running"], true);
        assert_eq!(status["serviceProcessId"], process_id);
        assert_eq!(status["cloudflareQuickTunnel"]["rateLimited"], true);
        assert_eq!(status["cloudflareQuickTunnel"]["retryAtMs"], cooldown_until);
        assert!(status["cloudflareQuickTunnel"]["processId"].is_null());
        assert!(!directory.path().join("latest-status.json").exists());

        for _ in 0..3 {
            let reused = run_test_process("client_worker", directory.path());
            assert_eq!(reused, process_id);
            let current = active_endpoint(directory.path()).await.unwrap().unwrap();
            assert_eq!(current.port, endpoint.port);
            assert!(current.token == endpoint.token);
        }
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let root = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(root.status(), reqwest::StatusCode::OK);
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

        let blocked = request(&endpoint, "restartQuickTunnel", &json!({}))
            .await
            .unwrap();
        assert_eq!(blocked["status"], "failed");
        drop(worker);
        let restarted_id = run_test_process("starter_worker", directory.path());
        let restarted_handle =
            unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, restarted_id) }
                .unwrap();
        let _restarted = WorkerGuard(unsafe { OwnedHandle::from_raw_handle(restarted_handle.0) });
        let restarted_endpoint = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(endpoint) = active_endpoint(directory.path()).await.unwrap() {
                    return endpoint;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let restored = request(&restarted_endpoint, "status", &json!({}))
            .await
            .unwrap();
        assert_eq!(restored["serviceProcessId"], restarted_id);
        assert_eq!(
            restored["cloudflareQuickTunnel"]["retryAtMs"],
            cooldown_until
        );
        assert_eq!(restored["cloudflareQuickTunnel"]["state"], "retry_wait");
        assert!(restored["cloudflareQuickTunnel"]["processId"].is_null());
    }
}
