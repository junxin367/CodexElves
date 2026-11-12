use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use serde::Serialize;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, PhysicalPosition, WebviewUrl, WindowEvent};
use url::Url;

use crate::appearance::{ThemeSnapshot, TitlebarTheme};
use crate::connection::{Completion, Connection};
use crate::resource_cache::{CacheInfo, ResourceCache};
use crate::settings::{self, Settings};

const TITLE: &str = "CodexElves 远程会话";
const WINDOW_LABEL: &str = "main";
const CONTENT_LABEL: &str = "remote-content";
const TITLEBAR_LABEL: &str = "remote-titlebar";
// 用户确认是最上方标题栏，后续调整为固定 36 个逻辑像素，按 Windows DPI 缩放；标题栏和远程内容分别占用独立 WebView。
const TITLEBAR_HEIGHT: f64 = 36.0;

struct ClientState {
    directory: PathBuf,
    settings: Mutex<Settings>,
    home: Mutex<Option<Url>>,
    auto_connect: AtomicBool,
    exiting: AtomicBool,
    connection: Mutex<Connection>,
    titlebar_theme: Mutex<Option<ThemeSnapshot>>,
    cache: Arc<ResourceCache>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientInfo {
    server_url: String,
    auto_connect: bool,
    version: &'static str,
    error: Option<String>,
    detected: bool,
}

fn trusted_connection_page(webview: &tauri::Webview) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    if webview.label() != CONTENT_LABEL
        || webview.window().label() != WINDOW_LABEL
        || !settings::is_connection_page(&url)
    {
        return Err("只有内置连接页可以修改客户端连接设置。".into());
    }
    Ok(())
}

#[tauri::command]
async fn remote_client_state(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
) -> Result<ClientInfo, String> {
    trusted_connection_page(&webview)?;
    let saved = state.settings.lock().unwrap().server_url.clone();
    let (draft, error) = {
        let connection = state.connection.lock().unwrap();
        (connection.draft_url.clone(), connection.error.clone())
    };
    let mut server_url = if draft.is_empty() { saved } else { draft };
    let mut detected = false;
    if server_url.is_empty() && error.is_none() {
        if let Some(home) = directories::BaseDirs::new() {
            if let Some(url) = crate::discovery::detect(
                &home.home_dir().join(".codex-session-delete/settings.json"),
            )
            .await
            {
                server_url = url.to_string();
                detected = true;
            }
        }
    }
    Ok(ClientInfo {
        server_url,
        auto_connect: state.auto_connect.swap(false, Ordering::SeqCst),
        version: env!("CARGO_PKG_VERSION"),
        error,
        detected,
    })
}

#[tauri::command]
async fn connect_remote(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
    server_url: String,
) -> Result<(), String> {
    trusted_connection_page(&webview)?;
    let url = settings::normalize_server_url(&server_url).map_err(|error| error.to_string())?;
    state.connection.lock().unwrap().begin(url.clone());
    let previously_connected = Url::parse(&state.settings.lock().unwrap().server_url)
        .is_ok_and(|saved| saved.origin() == url.origin());
    if let Err(error) = state.cache.prepare(&url, previously_connected).await {
        state.connection.lock().unwrap().cancel();
        return Err(error.to_string());
    }
    // 导航已提交不代表加载成功；只有 WebView2 成功回执到达后才保存下次自动连接地址。
    if let Err(error) = webview.navigate(url) {
        state.connection.lock().unwrap().cancel();
        return Err(format!("打开远程服务失败：{error}"));
    }
    Ok(())
}

fn trusted_service_page(webview: &tauri::Webview, state: &ClientState) -> Result<Url, String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    let saved = state.settings.lock().unwrap().server_url.clone();
    if webview.label() != CONTENT_LABEL
        || webview.window().label() != WINDOW_LABEL
        || !(settings::is_connection_page(&url)
            || matches!(url.scheme(), "http" | "https")
                && state.connection.lock().unwrap().allows(&url, &saved))
    {
        return Err("只有当前服务页面可以管理网页资源缓存。".into());
    }
    Ok(url)
}

#[tauri::command]
async fn remote_cache_status(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
) -> Result<CacheInfo, String> {
    trusted_service_page(&webview, &state)?;
    Ok(state.cache.info())
}

#[tauri::command]
async fn remote_cache_refresh(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
) -> Result<CacheInfo, String> {
    let mut url = trusted_service_page(&webview, &state)?;
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    state
        .cache
        .refresh(&url)
        .await
        .map_err(|_| "网页资源更新检查未完成，仍保留当前可用缓存。".into())
}

#[tauri::command]
async fn remote_cache_activity(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
    busy: bool,
) -> Result<(), String> {
    trusted_service_page(&webview, &state)?;
    state.cache.set_busy(busy);
    Ok(())
}

#[tauri::command]
async fn remote_titlebar_theme(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
    theme: TitlebarTheme,
) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    let saved = state.settings.lock().unwrap().server_url.clone();
    if webview.label() != CONTENT_LABEL
        || webview.window().label() != WINDOW_LABEL
        || !(settings::is_connection_page(&url)
            || matches!(url.scheme(), "http" | "https")
                && state.connection.lock().unwrap().allows(&url, &saved))
    {
        return Err("只有当前服务页面可以同步标题栏配色。".into());
    }
    // 用户要求标题栏跟随外观模式及自定义颜色；跨页面只传严格校验的 RGB 数字，不开放连接设置或窗口控制。
    let snapshot = {
        let mut current = state.titlebar_theme.lock().unwrap();
        if current.as_ref().is_some_and(|value| value.theme == theme) {
            return Ok(());
        }
        let snapshot = ThemeSnapshot {
            version: current
                .as_ref()
                .map_or(1, |value| value.version.saturating_add(1)),
            theme,
        };
        *current = Some(snapshot.clone());
        snapshot
    };
    if let Some(titlebar) = webview.app_handle().get_webview(TITLEBAR_LABEL) {
        let payload = serde_json::to_string(&snapshot).map_err(|error| error.to_string())?;
        titlebar
            .eval(format!("window.__setRemoteTitlebarTheme?.({payload})"))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn remote_titlebar_state(
    webview: tauri::Webview,
    state: tauri::State<'_, ClientState>,
) -> Result<Option<ThemeSnapshot>, String> {
    if webview.label() != TITLEBAR_LABEL
        || webview.window().label() != WINDOW_LABEL
        || !settings::is_titlebar_page(&webview.url().map_err(|error| error.to_string())?)
    {
        return Err("只有本地标题栏可以读取标题栏配色。".into());
    }
    Ok(state.titlebar_theme.lock().unwrap().clone())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_connection_page(app: &tauri::AppHandle) {
    navigate_connection_page(app, true);
}

fn navigate_connection_page(app: &tauri::AppHandle, show: bool) {
    let state = app.state::<ClientState>();
    state.auto_connect.store(false, Ordering::SeqCst);
    state.connection.lock().unwrap().cancel();
    let home = state.home.lock().unwrap().clone();
    if let (Some(webview), Some(home)) = (app.get_webview(CONTENT_LABEL), home) {
        let _ = webview.navigate(home);
        if show {
            show_window(app);
        }
    }
}

fn install_navigation_recovery(content: &tauri::Webview) -> tauri::Result<()> {
    let app = content.app_handle().clone();
    content.with_webview(move |webview| {
        let recovery_app = app.clone();
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| register_navigation_recovery(core, app))
        };
        if result.is_err() {
            recovery_app
                .state::<ClientState>()
                .connection
                .lock()
                .unwrap()
                .error =
                Some("加载状态监听初始化失败；可从托盘菜单打开“连接设置”重新输入。".into());
        }
    })
}

unsafe fn register_navigation_recovery(
    core: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    app: tauri::AppHandle,
) -> windows::core::Result<()> {
    use webview2_com::{
        NavigationCompletedEventHandler, NavigationStartingEventHandler, take_pwstr,
    };
    let start_app = app.clone();
    let mut token = 0;
    unsafe {
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut raw = Default::default();
                args.Uri(&mut raw)?;
                let Ok(url) = Url::parse(&take_pwstr(raw)) else {
                    return Ok(());
                };
                if settings::is_connection_page(&url) {
                    return Ok(());
                }
                let mut id = 0;
                args.NavigationId(&mut id)?;
                let state = start_app.state::<ClientState>();
                state.cache.begin_navigation(&url);
                let saved = state.settings.lock().unwrap().server_url.clone();
                state.connection.lock().unwrap().started(id, &url, &saved);
                Ok(())
            })),
            &mut token,
        )?;
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut successful = Default::default();
                let mut id = 0;
                args.IsSuccess(&mut successful)?;
                args.NavigationId(&mut id)?;
                let state = app.state::<ClientState>();
                let completion = state
                    .connection
                    .lock()
                    .unwrap()
                    .completed(id, successful.as_bool());
                match completion {
                    Some(Completion::Connected(url)) => {
                        let mut saved = state.settings.lock().unwrap();
                        if saved.server_url != url.as_str() {
                            saved.server_url = url.to_string();
                            if settings::save(&state.directory.join("settings.json"), &saved)
                                .is_err()
                            {
                                state.connection.lock().unwrap().error =
                                    Some("已连接，但客户端设置暂时无法保存。".into());
                            }
                        }
                    }
                    Some(Completion::Failed) => {
                        // DNS、证书和网络失败后返回可编辑页面；已收进托盘时不擅自弹出窗口。
                        state.auto_connect.store(false, Ordering::SeqCst);
                        let recovery_app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            navigate_connection_page(&recovery_app, false);
                        });
                    }
                    None => {}
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

fn remember_window(window: &tauri::Window, persist: bool) {
    if window.is_minimized().unwrap_or(false) {
        if persist {
            let state = window.app_handle().state::<ClientState>();
            let _ = settings::save(
                &state.directory.join("settings.json"),
                &state.settings.lock().unwrap(),
            );
        }
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let app = window.app_handle();
    let state = app.state::<ClientState>();
    let mut current = state.settings.lock().unwrap();
    current.window.maximized = maximized;
    if !maximized {
        if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
            let logical = size.to_logical::<f64>(scale);
            if logical.width >= 520.0 && logical.height >= 420.0 {
                current.window.width = logical.width;
                current.window.height = logical.height;
            }
        }
        if let Ok(position) = window.outer_position() {
            current.window.x = Some(position.x);
            current.window.y = Some(position.y);
        }
    }
    // 移动/缩放只更新内存，收起和退出时再原子保存，避免拖动窗口时持续写磁盘。
    if persist {
        let _ = settings::save(&state.directory.join("settings.json"), &current);
    }
}

fn restore_window(window: &tauri::Window, settings: &Settings) -> tauri::Result<()> {
    // 用户要求基本窗口缩放；最小 520×420 保留连接页可用空间，上限仅防止损坏配置产生异常窗口。
    let width = if settings.window.width.is_finite() {
        settings.window.width.clamp(520.0, 7680.0)
    } else {
        1280.0
    };
    let height = if settings.window.height.is_finite() {
        settings.window.height.clamp(420.0, 4320.0)
    } else {
        860.0
    };
    window.set_size(tauri::LogicalSize::new(width, height))?;
    let visible = settings
        .window
        .x
        .zip(settings.window.y)
        .is_some_and(|(x, y)| {
            window
                .available_monitors()
                .unwrap_or_default()
                .iter()
                .any(|monitor| {
                    let p = monitor.position();
                    let s = monitor.size();
                    // 至少保留 80×40 像素操作区（含 36px 标题栏）；显示器移除后回到当前屏幕中央。
                    i64::from(x) + 80 > i64::from(p.x)
                        && i64::from(x) < i64::from(p.x) + i64::from(s.width) - 80
                        && i64::from(y) >= i64::from(p.y)
                        && i64::from(y) < i64::from(p.y) + i64::from(s.height) - 40
                })
        });
    if visible {
        window.set_position(PhysicalPosition::new(
            settings.window.x.unwrap(),
            settings.window.y.unwrap(),
        ))?;
    } else {
        window.center()?;
    }
    if settings.window.maximized {
        window.maximize()?;
    }
    layout_webviews(window)?;
    #[cfg(debug_assertions)]
    if std::env::var("CODEX_ELVES_REMOTE_TEST_HIDDEN").as_deref() == Ok("1") {
        // 调试验收可隐藏独立测试窗口，避免打断用户的实际会话；发布版不启用此入口。
        return Ok(());
    }
    window.show()
}

fn layout_webviews(window: &tauri::Window) -> tauri::Result<()> {
    if window.is_minimized()? {
        return Ok(());
    }
    let size = window.inner_size()?;
    let title_height = ((TITLEBAR_HEIGHT * window.scale_factor()?).round() as u32).min(size.height);
    // 两块区域共用同一物理像素边界；缩放、最大化及跨 DPI 显示器移动时保持标题栏 36px，不按窗口高度拉伸。
    for (label, y, height) in [
        (TITLEBAR_LABEL, 0, title_height),
        (
            CONTENT_LABEL,
            title_height,
            size.height.saturating_sub(title_height),
        ),
    ] {
        if let Some(webview) = window.app_handle().get_webview(label) {
            webview.set_bounds(tauri::Rect {
                position: tauri::PhysicalPosition::new(0, y as i32).into(),
                size: tauri::PhysicalSize::new(size.width, height).into(),
            })?;
        }
    }
    Ok(())
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开远程会话", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "最小化到托盘", true, None::<&str>)?;
    let maximize = MenuItem::with_id(app, "maximize", "最大化 / 还原", true, None::<&str>)?;
    let connection = MenuItem::with_id(app, "connection", "连接设置…", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "刷新页面", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &hide,
            &maximize,
            &connection,
            &refresh,
            &separator,
            &quit,
        ],
    )?;
    let icon = app
        .default_window_icon()
        .cloned()
        .context("missing application icon")
        .map_err(|error| tauri::Error::Anyhow(error))?;
    TrayIconBuilder::with_id("remote-client")
        .icon(icon)
        .tooltip(TITLE)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn handle_menu_event(app: &tauri::AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "show" => show_window(app),
        "hide" => {
            if let Some(window) = app.get_window(WINDOW_LABEL) {
                remember_window(&window, true);
                let _ = window.hide();
            }
        }
        "maximize" => {
            show_window(app);
            if let Some(window) = app.get_window(WINDOW_LABEL) {
                let _ = if window.is_maximized().unwrap_or(false) {
                    window.unmaximize()
                } else {
                    window.maximize()
                };
            }
        }
        "connection" => show_connection_page(app),
        "refresh" => {
            if let Some(webview) = app.get_webview(CONTENT_LABEL) {
                let _ = webview.reload();
            }
        }
        "quit" => {
            let state = app.state::<ClientState>();
            state.exiting.store(true, Ordering::SeqCst);
            if let Some(window) = app.get_window(WINDOW_LABEL) {
                remember_window(&window, true);
            }
            app.exit(0);
        }
        _ => {}
    }
}

fn open_external(url: Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    // 远程网页的新窗口只交给默认浏览器打开 HTTP(S)，不执行文件、自定义协议或网页提供的命令行。
    std::thread::spawn(move || {
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
        unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
        }
    });
}

pub fn run() -> anyhow::Result<()> {
    let app = tauri::Builder::default()
        // 用户要求再次启动唤回托盘中的同一窗口，不额外创建客户端进程或连接。
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .invoke_handler(tauri::generate_handler![
            remote_client_state,
            connect_remote,
            remote_titlebar_theme,
            remote_titlebar_state,
            remote_cache_status,
            remote_cache_refresh,
            remote_cache_activity
        ])
        .on_menu_event(handle_menu_event)
        .setup(|app| {
            let directory = match std::env::var_os("CODEX_ELVES_REMOTE_DATA_DIR") {
                Some(path) => {
                    let path = PathBuf::from(path);
                    if !path.is_absolute() {
                        return Err("客户端数据目录必须为绝对路径。".into());
                    }
                    path
                }
                None => app.path().app_local_data_dir()?,
            };
            std::fs::create_dir_all(&directory)?;
            let settings = settings::load(&directory.join("settings.json"))?;
            let cache = ResourceCache::new(directory.join("web-resources"))?;
            app.manage(ClientState {
                directory: directory.clone(),
                settings: Mutex::new(settings.clone()),
                home: Mutex::new(None),
                auto_connect: AtomicBool::new(!settings.server_url.is_empty()),
                exiting: AtomicBool::new(false),
                connection: Mutex::new(Connection::default()),
                titlebar_theme: Mutex::new(None),
                cache: cache.clone(),
            });
            let window = tauri::window::WindowBuilder::new(app, WINDOW_LABEL)
                .title(TITLE)
                .inner_size(1280.0, 860.0)
                .min_inner_size(520.0, 420.0)
                .decorations(false)
                .shadow(true)
                .resizable(true)
                .minimizable(true)
                .maximizable(true)
                .visible(false)
                .build()?;
            let navigation_app = app.handle().clone();
            let content_builder = tauri::webview::WebviewBuilder::new(
                CONTENT_LABEL,
                WebviewUrl::External(Url::parse("about:blank")?),
            )
            .data_directory(directory.join("webview"))
            .initialization_script(include_str!("../../web/theme-bridge.js"))
            .disable_drag_drop_handler()
            .on_navigation(move |url| {
                if settings::is_connection_page(url) || url.as_str() == "about:blank" {
                    return true;
                }
                let state = navigation_app.state::<ClientState>();
                let configured = state.settings.lock().unwrap().server_url.clone();
                if matches!(url.scheme(), "http" | "https")
                    && state.connection.lock().unwrap().allows(url, &configured)
                {
                    return true;
                }
                open_external(url.clone());
                false
            })
            .on_new_window(|url, _| {
                open_external(url);
                tauri::webview::NewWindowResponse::Deny
            });
            let content = window.add_child(
                content_builder,
                tauri::LogicalPosition::new(0.0, TITLEBAR_HEIGHT),
                tauri::LogicalSize::new(1280.0, 860.0 - TITLEBAR_HEIGHT),
            )?;
            // 标题栏单独加载本地页面，远程服务无法覆盖它，也不获得其最小化、拖动等窗口权限。
            window.add_child(
                tauri::webview::WebviewBuilder::new(
                    TITLEBAR_LABEL,
                    WebviewUrl::App("titlebar.html".into()),
                )
                .data_directory(directory.join("titlebar-webview"))
                .disable_drag_drop_handler()
                .on_navigation(settings::is_titlebar_page)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny),
                tauri::LogicalPosition::new(0.0, 0.0),
                tauri::LogicalSize::new(1280.0, TITLEBAR_HEIGHT),
            )?;
            // Windows Tauri 本地资源默认 Origin；先完成原生缓存接管，再打开连接页，防止自动连接抢在初始化前执行。
            let home = Url::parse("http://tauri.localhost/index.html")?;
            *app.state::<ClientState>().home.lock().unwrap() = Some(home.clone());
            install_navigation_recovery(&content)?;
            crate::native_cache::install(&content, cache, home)?;
            // 用户要求独立程序不显示“连接”“窗口”菜单栏；连接恢复、刷新和退出统一保留在托盘。
            install_tray(app)?;
            let event_window = window.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::Moved(_) => remember_window(&event_window, false),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    let _ = layout_webviews(&event_window);
                    remember_window(&event_window, false);
                }
                WindowEvent::CloseRequested { api, .. } => {
                    let app = event_window.app_handle();
                    if !app.state::<ClientState>().exiting.load(Ordering::SeqCst) {
                        // 用户要求托盘管理：关闭保留客户端连接，只有托盘“退出程序”才结束进程。
                        api.prevent_close();
                        remember_window(&event_window, true);
                        let _ = event_window.hide();
                    }
                }
                _ => {}
            });
            restore_window(&window, &settings)?;
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. })
            && let Some(window) = app.get_window(WINDOW_LABEL)
        {
            remember_window(&window, true);
        }
    });
    Ok(())
}

pub fn show_startup_error(message: &str) {
    let title: Vec<u16> = TITLE.encode_utf16().chain(Some(0)).collect();
    let text: Vec<u16> =
        format!("远程客户端启动失败：{message}\n\n请确认 Microsoft Edge WebView2 Runtime 已安装。")
            .encode_utf16()
            .chain(Some(0))
            .collect();
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}
