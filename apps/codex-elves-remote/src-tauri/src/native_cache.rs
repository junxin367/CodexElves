use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use url::Url;
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, ClearBrowsingDataCompletedHandler,
    Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::core::{HSTRING, Interface, PWSTR};

use crate::resource_cache::ResourceCache;

// 兼容旧服务页面的 PWA 检测：此 WebView 已由原生接管资源，不再注册另一套 Service Worker。
// 只移除当前容器的 Service Worker API；Cookie、localStorage、IndexedDB 及已登录设备保持原状。
const CACHE_BOOTSTRAP: &str = r#"(() => {
  if (window.top !== window) return;
  Object.defineProperty(window, "__CODEX_ELVES_NATIVE_RESOURCES__", {value: Object.freeze({version:1})});
  try { delete Navigator.prototype.serviceWorker; } catch {}
})();"#;

#[cfg(debug_assertions)]
fn test_log(event: &str) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("CODEX_ELVES_REMOTE_CACHE_TEST_LOG") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{event}");
        }
    }
}

pub fn install(
    content: &tauri::Webview,
    cache: Arc<ResourceCache>,
    home: Url,
) -> tauri::Result<()> {
    let content_ready = content.clone();
    let interception_cache = cache.clone();
    let once = Arc::new(AtomicBool::new(false));
    let finish: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |enabled| {
        if once.swap(true, Ordering::SeqCst) {
            return;
        }
        if enabled {
            cache.enable();
        }
        #[cfg(debug_assertions)]
        test_log(if enabled {
            "cache-enabled"
        } else {
            "cache-disabled"
        });
        let content = content_ready.clone();
        let home = home.clone();
        tauri::async_runtime::spawn(async move {
            let _ = content.navigate(home);
        });
    });
    let callback = finish.clone();
    content.with_webview(move |platform| {
        let result = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
                register(&core, &platform.environment(), interception_cache)?;
                initialize(core, callback.clone())
            })
        };
        if result.is_err() {
            callback(false);
        }
    })
}

unsafe fn initialize(
    core: ICoreWebView2,
    finish: Arc<dyn Fn(bool) + Send + Sync>,
) -> windows::core::Result<()> {
    let profile = unsafe {
        core.cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile2>()?
    };
    unsafe {
        profile.ClearBrowsingData(
            COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS,
            &ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
                if result.is_err() {
                    finish(false);
                    return Ok(());
                }
                let complete = finish.clone();
                let result = core.AddScriptToExecuteOnDocumentCreated(
                    &HSTRING::from(CACHE_BOOTSTRAP),
                    &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                        move |result, _| {
                            complete(result.is_ok());
                            Ok(())
                        },
                    )),
                );
                if result.is_err() {
                    finish(false);
                }
                Ok(())
            })),
        )
    }
}

unsafe fn register(
    core: &ICoreWebView2,
    environment: &ICoreWebView2Environment,
    cache: Arc<ResourceCache>,
) -> windows::core::Result<()> {
    unsafe {
        if let Ok(latest) = core.cast::<ICoreWebView2_22>() {
            latest.AddWebResourceRequestedFilterWithRequestSourceKinds(
                &HSTRING::from("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?;
        } else {
            core.AddWebResourceRequestedFilter(
                &HSTRING::from("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            )?;
        }
        let environment = environment.clone();
        let mut token = 0;
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let request = args.Request()?;
                let mut address = PWSTR::null();
                request.Uri(&mut address)?;
                let address = take_pwstr(address);
                let Ok(url) = Url::parse(&address) else {
                    return Ok(());
                };
                let mut method = PWSTR::null();
                request.Method(&mut method)?;
                let method = take_pwstr(method);
                // 缓存入口不再经过公网提示页；与 channels.js 和 Android 媒体请求一致，给隧道请求补公开跳过页标记。
                // API/媒体仍直连原服务，不修改 Cookie、Authorization 或响应内容。
                if let Some(header) = cache.tunnel_header(&url) {
                    if let Ok(headers) = request.Headers() {
                        let _ = headers.SetHeader(&HSTRING::from(header), &HSTRING::from("1"));
                    }
                }
                let Some(cached) = cache.response(&url, &method) else {
                    return Ok(());
                };
                #[cfg(debug_assertions)]
                test_log(&format!("served {}", url.path()));
                let stream = if cached.bytes.is_empty() {
                    None
                } else {
                    windows::Win32::UI::Shell::SHCreateMemStream(Some(cached.bytes.as_ref()))
                };
                if !cached.bytes.is_empty() && stream.is_none() {
                    return Ok(());
                }
                let response = environment.CreateWebResourceResponse(
                    stream.as_ref(),
                    200,
                    &HSTRING::from("OK"),
                    &HSTRING::from(cached.headers),
                )?;
                args.SetResponse(&response)?;
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}
