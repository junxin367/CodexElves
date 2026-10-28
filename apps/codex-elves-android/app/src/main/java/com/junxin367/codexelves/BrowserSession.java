package com.junxin367.codexelves;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.content.Context;
import android.content.MutableContextWrapper;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.webkit.CookieManager;
import android.webkit.ConsoleMessage;
import android.webkit.HttpAuthHandler;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebStorage;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.ValueCallback;
import android.view.View;
import android.view.ViewGroup;
import androidx.webkit.ProxyConfig;
import androidx.webkit.ProxyController;
import androidx.webkit.ScriptHandler;
import androidx.webkit.UserAgentMetadata;
import androidx.webkit.WebStorageCompat;
import androidx.webkit.WebSettingsCompat;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;
import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONArray;
import org.json.JSONTokener;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/** Main-thread-owned browser state; never retains a detached Activity. */
final class BrowserSession {
    static final class DiagnosticEvent {
        final long id;
        final long timeMs;
        final String phase;
        final String method;
        final String url;
        String status;
        String detail;
        String headers;
        boolean mainFrame;
        String resourceType;
        long durationMs;
        long sizeBytes;
        int statusCode;
        boolean response;
        boolean failure;

        DiagnosticEvent(long id, long timeMs, String phase, String method, String url,
            String status, String detail, String headers, boolean mainFrame,
            String resourceType, long durationMs, long sizeBytes, int statusCode,
            boolean response, boolean failure) {
            this.id = id;
            this.timeMs = timeMs;
            this.phase = phase;
            this.method = method;
            this.url = url;
            this.status = status;
            this.detail = detail;
            this.headers = headers;
            this.mainFrame = mainFrame;
            this.resourceType = resourceType;
            this.durationMs = durationMs;
            this.sizeBytes = sizeBytes;
            this.statusCode = statusCode;
            this.response = response;
            this.failure = failure;
        }

        DiagnosticEvent copy() {
            return new DiagnosticEvent(id, timeMs, phase, method, url, status, detail, headers,
                mainFrame, resourceType, durationMs, sizeBytes, statusCode, response, failure);
        }
    }

    interface Listener {
        void changed();
        void invalidated(String message);
        void requestAuthentication(String host, HttpAuthHandler handler);
        void chooseFiles(ValueCallback<Uri[]> callback, WebChromeClient.FileChooserParams parameters);
    }
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final int DIAGNOSTIC_EVENT_LIMIT = 300;
    // Desktop mode lays the page out at this width instead of the page's device-width viewport.
    private static final int DESKTOP_VIEWPORT_WIDTH = 1280;
    private static final String DESKTOP_VIEWPORT_SCRIPT = "(function(){if(window.top!==window)return;"
        + "var content='width=" + DESKTOP_VIEWPORT_WIDTH + "';"
        + "function apply(){var metas=document.querySelectorAll('meta[name=viewport]');"
        + "if(metas.length===0){if(!document.head)return;var meta=document.createElement('meta');"
        + "meta.name='viewport';document.head.appendChild(meta);metas=[meta];}"
        + "for(var i=0;i<metas.length;i++){if(metas[i].getAttribute('content')!==content)"
        + "metas[i].setAttribute('content',content);}}"
        + "apply();new MutationObserver(apply).observe(document,{childList:true,subtree:true,"
        + "attributes:true,attributeFilter:['name','content']});})();";
    // One retained view; detach replaces its MutableContextWrapper base with the application.
    @SuppressLint("StaticFieldLeak")
    private static BrowserSession current;
    private static boolean directoryConfigured, clearing;
    private static Runnable afterClear;
    private static long openGeneration;
    final WebView view;
    final BrowserAuthority.Snapshot identity;
    final BrowserPreferences preferences;
    private final IBinder authority;
    private final MutableContextWrapper context;
    private final AutoCloseable observer;
    private final BrowserProxy proxy;
    private Listener listener;
    private boolean closed, attached, routeReady;
    private long proxyGeneration;
    private long pageGeneration;
    private String pendingNavigationUrl = "";
    private String localFileThreadId = "";
    private String localFileOrigin = "";
    private boolean mainFrameStarted;
    private boolean mainFrameFinished;
    private String mobileUserAgent = "";
    private String androidUserAgent = "";
    private String desktopUserAgent = "";
    private String iphoneUserAgent = "";
    private String userAgentMode;
    private String pageTitle = "";
    private boolean desktopRootScrollLocked;
    private ScriptHandler desktopViewportScript;
    private final ArrayDeque<DiagnosticEvent> diagnosticEvents = new ArrayDeque<>();
    private final Map<String, DiagnosticEvent> latestNetworkEvents = new HashMap<>();
    private final Set<String> performanceResourceKeys = new HashSet<>();
    private long nextDiagnosticId;
    private final Runnable desktopRootScrollPolicyRefresh = this::updateDesktopRootScrollPolicy;
    private final Runnable diagnosticRefresh = () -> {
        if (!closed && listener != null) listener.changed();
    };
    boolean home = true;
    String url = "", error = "";
    int progress = 100;

    private static String errorEventSuffix(Throwable error) {
        if (error == null) return "";
        String value = error.getClass().getSimpleName().toLowerCase(Locale.ROOT)
            .replaceAll("[^a-z0-9]+", "-");
        return value.replaceAll("^-+|-+$", "");
    }

    private static void initializationFailure(
        IBinder authority, String stage, Throwable error, JSONObject fields
    ) {
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init", fields);
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init." + stage, fields);
        String suffix = errorEventSuffix(error);
        if (!suffix.isEmpty()) {
            BrowserAuthority.recordDiagnostic(authority,
                "native.browser.init." + stage + "." + suffix, fields);
        }
    }

    private static void initializationFailure(IBinder authority, String stage, JSONObject fields) {
        initializationFailure(authority, stage, null, fields);
    }

    static void prepare(IBinder authority) {
        if (Build.VERSION.SDK_INT < 28) throw new IllegalStateException("浏览器需要 Android 9 或更新版本");
        if (!directoryConfigured) {
            WebView.setDataDirectorySuffix("codex-elves-browser");
            directoryConfigured = true;
        }
        boolean proxyOverride = WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE);
        boolean reverseBypass = WebViewFeature.isFeatureSupported(
            WebViewFeature.PROXY_OVERRIDE_REVERSE_BYPASS);
        boolean deleteBrowsingData = WebViewFeature.isFeatureSupported(
            WebViewFeature.DELETE_BROWSING_DATA);
        int featureMask = (proxyOverride ? 1 : 0) | (reverseBypass ? 2 : 0)
            | (deleteBrowsingData ? 4 : 0);
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
            DiagnosticsPolicy.fields("phase", "request", "routeName", "prepare",
                "errorCode", featureMask, "reason", "request"));
        // The browser now sends every WebView request through the computer.
        // Reverse bypass is no longer required for routing, but it remains in
        // the diagnostic mask so older clients can still report provider capabilities.
        if (!proxyOverride) {
            initializationFailure(authority, "prepare",
                DiagnosticsPolicy.fields("phase", "failure", "routeName", "prepare",
                    "errorCode", featureMask, "reason", "unsupported"));
            throw new IllegalStateException("请更新 Android System WebView 后使用浏览器");
        }
    }

    static void acquire(Activity activity, IBinder authority,
        Consumer<BrowserSession> ready, Consumer<String> failure) {
        long generation = ++openGeneration;
        String stage = "prepare";
        try {
            prepare(authority);
            stage = "authority";
            BrowserAuthority.Snapshot identity = BrowserAuthority.read(authority);
            if (current != null && !current.closed && current.identity.owner().equals(identity.owner())) {
                current.context.setBaseContext(activity);
                ready.accept(current);
                return;
            }
            if (current != null) {
                current.destroy();
                current = null;
            }
            Runnable create = () -> {
                if (generation != openGeneration || activity.isFinishing() || activity.isDestroyed()) return;
                String createStage = "authority";
                AtomicReference<String> sessionStage = new AtomicReference<>("session");
                try {
                    BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                    if (!identity.owner().equals(fresh.owner())) throw new IllegalStateException("浏览器授权已失效");
                    createStage = "session";
                    current = new BrowserSession(activity, authority, fresh, sessionStage::set);
                    createStage = "ready";
                    activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                        .edit().putString("owner", fresh.owner()).apply();
                    ready.accept(current);
                } catch (Exception unavailable) {
                    String failedStage = "session".equals(createStage) ? sessionStage.get() : createStage;
                    initializationFailure(authority, failedStage, unavailable,
                        DiagnosticsPolicy.fields("phase", "failure", "routeName", failedStage,
                            "errorType", DiagnosticsPolicy.errorType(unavailable), "reason", "failed"));
                    failure.accept("无法打开浏览器，请返回会话后重试");
                }
            };
            String previousOwner = activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                .getString("owner", "");
            if (clearing || !identity.owner().equals(previousOwner)) {
                stage = "profile";
                afterClear = create;
                clearProfile(authority);
            } else create.run();
        } catch (Exception unavailable) {
            initializationFailure(authority, stage, unavailable,
                DiagnosticsPolicy.fields("phase", "failure", "routeName", stage,
                    "errorType", DiagnosticsPolicy.errorType(unavailable), "reason", "failed"));
            failure.accept(unavailable.getMessage() == null ? "浏览器当前不可用" : unavailable.getMessage());
        }
    }

    @SuppressLint("RequiresFeature") // DELETE_BROWSING_DATA is checked before the optional call.
    private static void clearProfile(IBinder authority) {
        if (clearing) return;
        clearing = true;
        Runnable complete = () -> {
            clearing = false;
            Runnable next = afterClear; afterClear = null;
            // Let the WebView provider finish its cleanup callback before creating
            // a WebView in the same process.
            if (next != null) MAIN.post(next);
        };
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)) {
            BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
                DiagnosticsPolicy.fields("phase", "success", "routeName", "profile",
                    "reason", "unsupported"));
            complete.run();
            return;
        }
        try {
            WebStorageCompat.deleteBrowsingData(WebStorage.getInstance(), () -> {
                BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
                    DiagnosticsPolicy.fields("phase", "success", "routeName", "profile",
                        "reason", "success"));
                complete.run();
            });
        } catch (RuntimeException failure) {
            initializationFailure(authority, "profile",
                DiagnosticsPolicy.fields("phase", "failure", "routeName", "profile",
                    "errorType", DiagnosticsPolicy.errorType(failure), "reason", "failed"));
            // Profile cleanup is best-effort. It must never prevent the browser from opening.
            complete.run();
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    private BrowserSession(Activity activity, IBinder authority, BrowserAuthority.Snapshot identity,
        Consumer<String> initializationStage) throws Exception {
        this.authority = authority; this.identity = identity;
        initializationStage.accept("preferences");
        context = new MutableContextWrapper(activity);
        preferences = new BrowserPreferences(activity);
        initializationStage.accept("proxy");
        proxy = new BrowserProxy(authority, () -> {
            try {
                BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                if (!identity.owner().equals(fresh.owner())) return null;
                return new BrowserProxy.Credentials(fresh.origin(), fresh.cookie());
            } catch (Exception expired) { MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")); return null; }
        }, () -> MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")));
        proxy.setActive(false);
        initializationStage.accept("webview");
        view = new WebView(context);
        view.setOnScrollChangeListener((webView, scrollX, scrollY, oldScrollX, oldScrollY) -> {
            if (desktopRootScrollLocked && scrollY != 0) {
                webView.scrollTo(scrollX, 0);
            }
        });
        view.setSaveEnabled(false);
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(true);
        settings.setSupportMultipleWindows(false);
        settings.setMediaPlaybackRequiresUserGesture(true);
        try {
            mobileUserAgent = settings.getUserAgentString();
            androidUserAgent = androidUserAgent(mobileUserAgent);
            desktopUserAgent = desktopUserAgent(mobileUserAgent);
            iphoneUserAgent = iphoneUserAgent();
            userAgentMode = preferences.loadUserAgentMode(identity.origin());
            applyDisplayMode(settings);
        } catch (RuntimeException unavailable) {
            // UA customization is optional; keep the WebView default so startup can continue.
            mobileUserAgent = "";
            androidUserAgent = "";
            desktopUserAgent = "";
            iphoneUserAgent = "";
            userAgentMode = BrowserPreferences.UA_ANDROID;
            diagnostic("native.browser.ua", DiagnosticsPolicy.fields(
                "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                "reason", "unsupported"));
        }
        try {
            CookieManager.getInstance().setAcceptCookie(true);
            CookieManager.getInstance().setAcceptThirdPartyCookies(view, false);
        } catch (RuntimeException unavailable) {
            // Cookie policy must not prevent the browser surface from opening.
            diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                "reason", "unsupported"));
        }
        view.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView webView, int value) {
                progress = value;
                changed();
                if (value >= 100 && BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    MAIN.postDelayed(desktopRootScrollPolicyRefresh, 250L);
                }
            }
            @Override public void onReceivedTitle(WebView webView, String title) {
                if (!home && title != null && !title.trim().isEmpty()) {
                    pageTitle = title.trim();
                    if (mainFrameFinished && allowed(Uri.parse(url))) {
                        preferences.recordHistory(identity.origin(), url, pageTitle, System.currentTimeMillis());
                    }
                }
                changed();
            }
            @Override public boolean onConsoleMessage(ConsoleMessage message) {
                recordDiagnosticEvent("console", "CONSOLE", message == null ? "" : message.message(),
                    message == null ? "" : message.sourceId() + ":" + message.lineNumber(),
                    "", false, "");
                return true;
            }
            @Override public boolean onShowFileChooser(WebView webView, ValueCallback<Uri[]> callback,
                FileChooserParams parameters) {
                if (!attached || closed || listener == null) { callback.onReceiveValue(null); return true; }
                listener.chooseFiles(callback, parameters);
                return true;
            }
        });
        view.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                Uri target = request.getUrl();
                if (!allowed(target)) {
                    if (request.isForMainFrame()) { error = "仅支持 HTTP 和 HTTPS 网页"; changed(); }
                    return true;
                }
                return false;
            }
            @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                // No page can impersonate the dedicated proxy's authentication host.
                Uri resource = request.getUrl();
                boolean network = "http".equalsIgnoreCase(resource.getScheme()) || "https".equalsIgnoreCase(resource.getScheme());
                if (network) {
                    recordNetworkRequest(request.getMethod(), resource.toString(),
                        request.isForMainFrame(), request.isRedirect() ? "redirect" : "",
                        sanitizedHeaders(request.getRequestHeaders()));
                }
                if (network && !allowed(resource)) return new WebResourceResponse("text/plain", "UTF-8",
                    403, "Forbidden", java.util.Map.of(),
                    new java.io.ByteArrayInputStream(new byte[0]));
                return null;
            }
            @Override public void onPageStarted(WebView webView, String address, Bitmap favicon) {
                if (home) {
                    webView.setVisibility(View.INVISIBLE);
                    return;
                }
                if (!mainFrameStarted && !sameAddress(address, pendingNavigationUrl)) return;
                mainFrameStarted = true;
                mainFrameFinished = false;
                pendingNavigationUrl = address;
                ++pageGeneration;
                ensureNetworkRequest("GET", address, true, "document");
                recordDiagnosticEvent("page-start", "GET", address, "", "", true, "");
                diagnostic("native.browser.page", DiagnosticsPolicy.urlFields(address,
                    "phase", "start", "generation", pageGeneration));
                webView.setVisibility(View.VISIBLE);
                url = address; error = ""; progress = 0; pageTitle = ""; changed();
            }
            @Override public void onPageFinished(WebView webView, String address) {
                if (home || !mainFrameStarted) return;
                mainFrameStarted = true;
                pendingNavigationUrl = address;
                if (BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    webView.scrollTo(0, webView.getScrollY());
                    MAIN.postDelayed(desktopRootScrollPolicyRefresh, 120L);
                }
                webView.setVisibility(View.VISIBLE);
                completeNetworkRequest("GET", address, 200, "200", "document", 0L, true);
                collectPerformanceSnapshot(webView);
                MAIN.postDelayed(() -> {
                    if (!closed && mainFrameFinished) {
                        collectPerformanceSnapshot(view);
                        finalizeNetworkEvents();
                    }
                }, 350L);
                recordDiagnosticEvent("page-finish", "GET", address, "", "page-loaded", true, "");
                diagnostic("native.browser.page", DiagnosticsPolicy.urlFields(address,
                    "phase", "complete", "generation", pageGeneration));
                url = address;
                if (!home && allowed(Uri.parse(address))) {
                    String title = pageTitle.isEmpty() ? webView.getTitle() : pageTitle;
                    preferences.recordHistory(identity.origin(), address, title, System.currentTimeMillis());
                }
                mainFrameFinished = true;
                changed();
            }
            @Override public void onScaleChanged(WebView webView, float oldScale, float newScale) {
                if (BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    // The page height can cross the viewport boundary while the user
                    // pinches. Recompute the short-page policy instead of retaining
                    // the state from the initial page scale.
                    MAIN.removeCallbacks(desktopRootScrollPolicyRefresh);
                    MAIN.post(desktopRootScrollPolicyRefresh);
                }
            }
            @Override public void onReceivedError(WebView webView, WebResourceRequest request, WebResourceError failure) {
                if (home || !allowed(request.getUrl())) return;
                String target = request.getUrl().toString();
                boolean mainFrame = request.isForMainFrame();
                if (mainFrame && !mainFrameStarted) return;
                int code = failure.getErrorCode();
                String detail = failure.getDescription() == null
                    ? "load-failed" : failure.getDescription().toString();
                if (!mainFrame && ignorableResourceFailure(request.getUrl(), detail)) {
                    finishNetworkRequest(request.getMethod(), target, 0, "resource-cancelled",
                        detail, false, inferResourceType(request.getUrl(), false), false);
                    return;
                }
                finishNetworkRequest(request.getMethod(), target, 0, "失败", detail,
                    mainFrame, inferResourceType(request.getUrl(), mainFrame), true);
                recordDiagnosticEvent("failure", request.getMethod(), target, "", detail,
                    mainFrame, "");
                if (mainFrame) {
                    webView.setVisibility(View.VISIBLE);
                    diagnostic("native.browser.error", DiagnosticsPolicy.urlFields(
                        target, "phase", "failure",
                        "errorCode", code, "errorType",
                        code == WebViewClient.ERROR_TIMEOUT ? "timeout" : "network",
                        "reason", code == WebViewClient.ERROR_TIMEOUT ? "timeout" : "load-failed",
                        "generation", pageGeneration));
                    error = "网页加载失败，请检查地址或网络后重试"; changed();
                }
            }
            @Override public void onReceivedHttpError(WebView webView, WebResourceRequest request, WebResourceResponse response) {
                if (home || !allowed(request.getUrl())) return;
                String target = request.getUrl().toString();
                boolean mainFrame = request.isForMainFrame();
                if (mainFrame && !mainFrameStarted) return;
                int statusCode = response.getStatusCode();
                finishNetworkRequest(request.getMethod(), target, statusCode,
                    Integer.toString(statusCode), response.getReasonPhrase(), mainFrame,
                    inferResourceType(request.getUrl(), mainFrame), statusCode >= 400);
                recordDiagnosticEvent("http-error", request.getMethod(), target,
                    Integer.toString(statusCode), response.getReasonPhrase(), mainFrame,
                    sanitizedHeaders(response.getResponseHeaders()));
                if (mainFrame) {
                    diagnostic("native.browser.http", DiagnosticsPolicy.urlFields(
                        target, "phase", "response",
                        "status", statusCode,
                        "mime", DiagnosticsPolicy.mime(response.getMimeType()),
                        "reason", "http-error", "generation", pageGeneration));
                    if (statusCode >= 500) {
                        error = "网页请求失败（" + statusCode + "），请稍后重试"; changed();
                    }
                }
            }
            @Override public void onReceivedSslError(WebView webView, SslErrorHandler handler,
                android.net.http.SslError failure) {
                handler.cancel();
                boolean mainFrame = mainFrameStarted;
                finishNetworkRequest("GET", failure.getUrl(), 0, "SSL",
                    "SSL " + failure.getPrimaryError(), mainFrame,
                    inferResourceType(Uri.parse(failure.getUrl()), mainFrame), true);
                recordDiagnosticEvent("ssl-error", "GET", failure.getUrl(), "",
                    "SSL " + failure.getPrimaryError(), mainFrame, "");
                diagnostic("native.browser.ssl", DiagnosticsPolicy.urlFields(
                    failure.getUrl(), "phase", "failure", "errorType", "ssl",
                    "errorCode", failure.getPrimaryError(), "reason", "ssl-failed",
                    "generation", pageGeneration));
                // Only onReceivedError's isForMainFrame may replace the page with an error.
            }
            @Override public void onReceivedHttpAuthRequest(WebView webView, HttpAuthHandler handler, String host, String realm) {
                if (attached && !closed && proxy.matchesChallenge(host, realm))
                    handler.proceed(BrowserProxy.USER, proxy.password());
                else if (attached && listener != null && !BrowserRoutePolicy.reservedProxyHost(host))
                    listener.requestAuthentication(host, handler);
                else handler.cancel();
            }
            @Override public boolean onRenderProcessGone(WebView webView, RenderProcessGoneDetail detail) {
                recordDiagnosticEvent("failure", "GET", url, "",
                    detail.didCrash() ? "renderer-crashed" : "renderer-killed", true, "");
                diagnostic("native.browser.renderer", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", "renderer",
                    "reason", detail.didCrash() ? "renderer-crashed" : "renderer-killed"));
                invalidate("浏览器页面已退出，请返回会话后重新打开");
                return true;
            }
        });
        initializationStage.accept("authority");
        AutoCloseable registration;
        try { registration = BrowserAuthority.observe(authority, () -> MAIN.post(() -> invalidate("浏览器授权已失效"))); }
        catch (Exception unavailable) { proxy.close(); view.destroy(); throw unavailable; }
        observer = registration;
    }

    private static boolean allowed(Uri target) {
        if (!"http".equalsIgnoreCase(target.getScheme()) && !"https".equalsIgnoreCase(target.getScheme())) return false;
        try { return !BrowserRoutePolicy.reservedProxyHost(target.getHost()); }
        catch (IllegalArgumentException invalid) { return false; }
    }

    private static boolean sameAddress(String first, String second) {
        if (first == null || second == null || first.isEmpty() || second.isEmpty()) return false;
        if (first.equals(second)) return true;
        Uri left = Uri.parse(first).normalizeScheme();
        Uri right = Uri.parse(second).normalizeScheme();
        String leftPath = left.getPath() == null || left.getPath().isEmpty() ? "/" : left.getPath();
        String rightPath = right.getPath() == null || right.getPath().isEmpty() ? "/" : right.getPath();
        return left.getScheme() != null && left.getScheme().equals(right.getScheme())
            && left.getHost() != null && left.getHost().equalsIgnoreCase(right.getHost())
            && left.getPort() == right.getPort()
            && leftPath.equals(rightPath)
            && java.util.Objects.equals(left.getQuery(), right.getQuery())
            && java.util.Objects.equals(left.getFragment(), right.getFragment());
    }

    private static boolean sameDocument(String loaded, String target) {
        return loaded != null && !loaded.isEmpty()
            && sameAddress(withoutFragment(loaded), withoutFragment(target));
    }

    private static String withoutFragment(String address) {
        int fragment = address.indexOf('#');
        return fragment < 0 ? address : address.substring(0, fragment);
    }

    void attach(Activity activity, Listener listener, Runnable ready) {
        if (closed) { listener.invalidated("浏览器已关闭"); return; }
        this.listener = listener;
        context.setBaseContext(activity);
        syncIdentityCookie(() -> applyRules(() -> {
                if (closed || this.listener != listener || activity.isFinishing()) return;
                attached = true;
                proxy.setActive(!home);
                if (!home) { view.onResume(); view.resumeTimers(); }
                ready.run(); changed();
            }, () -> {
                if (!closed && this.listener == listener) {
                    listener.invalidated("网络设置应用失败，请返回会话后重试");
                }
            }), () -> {
                if (!closed && this.listener == listener) {
                    listener.invalidated("设备会话 Cookie 写入失败，请返回会话后重试");
                }
            });
    }

    void detach(Listener owner) {
        if (listener != owner && context.getBaseContext() != owner) return;
        listener = null; attached = false; ++proxyGeneration;
        flushCookies();
        view.onPause(); view.pauseTimers();
        proxy.setActive(false);
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        context.setBaseContext(context.getApplicationContext());
    }

    @SuppressLint("RequiresFeature") // prepare() checks proxy support before constructing a session.
    private void applyRules(Runnable complete, Runnable failure) {
        long generation = ++proxyGeneration;
        routeReady = false;
        proxy.setActive(false);
        ProxyConfig.Builder configuration = new ProxyConfig.Builder()
            .addProxyRule("http://" + proxy.host() + ":" + proxy.port())
            .removeImplicitRules();
        diagnostic("native.browser.route", DiagnosticsPolicy.fields(
            "phase", "request", "routeName", "proxy-all",
            "host", proxy.host(), "count", 0, "port", proxy.port()));
        try {
            ProxyController.getInstance().setProxyOverride(configuration.build(), MAIN::post, () -> {
                if (!closed && generation == proxyGeneration) {
                    routeReady = true;
                    proxy.setActive(attached && !home);
                    diagnostic("native.browser.route", DiagnosticsPolicy.fields(
                        "phase", "success", "routeName", "proxy-all",
                        "host", proxy.host(), "count", 0, "port", proxy.port()));
                    complete.run();
                }
            });
        } catch (RuntimeException exception) {
            diagnostic("native.browser.route", DiagnosticsPolicy.fields(
                "phase", "failure", "routeName", "proxy-all",
                "errorType", DiagnosticsPolicy.errorType(exception), "reason", "failed"));
            if (!closed && generation == proxyGeneration && failure != null) {
                failure.run();
            }
        }
    }

    private void syncIdentityCookie(Runnable complete, Runnable failure) {
        CookieManager cookies = CookieManager.getInstance();
        try {
            cookies.setAcceptCookie(true);
            cookies.setAcceptThirdPartyCookies(view, false);
            String cookie = identity.cookie()
                + "; Path=/; HttpOnly; SameSite=Strict"
                + (identity.origin().startsWith("https://") ? "; Secure" : "");
            cookies.setCookie(identity.origin(), cookie, accepted -> {
                if (closed) return;
                if (!Boolean.TRUE.equals(accepted)) {
                    diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "write-rejected"));
                    failure.run();
                    return;
                }
                cookies.flush();
                if (!containsCookie(cookies.getCookie(identity.origin()), identity.cookie())) {
                    diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "readback-missing"));
                    failure.run();
                    return;
                }
                diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                    "phase", "success", "reason", "synced"));
                complete.run();
            });
        } catch (RuntimeException unavailable) {
            diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                "phase", "failure", "reason", "unsupported",
                "errorType", DiagnosticsPolicy.errorType(unavailable)));
            failure.run();
        }
    }

    private static boolean containsCookie(String header, String expected) {
        if (header == null || expected == null || expected.isEmpty()) return false;
        for (String value : header.split(";")) {
            if (expected.equals(value.trim())) return true;
        }
        return false;
    }

    long pageGeneration() { return pageGeneration; }
    String diagnosticPageUrl() { return redactDiagnosticUrl(url); }
    List<BrowserPreferences.HistoryEntry> history() {
        return preferences.loadHistory(identity.origin());
    }
    List<BrowserPreferences.HistoryEntry> recentHistory(int limit) {
        return preferences.loadHistory(identity.origin(), limit);
    }
    List<BrowserPreferences.BookmarkEntry> bookmarks() {
        return preferences.loadBookmarks(identity.origin());
    }
    List<DiagnosticEvent> diagnosticEvents() {
        synchronized (diagnosticEvents) {
            List<DiagnosticEvent> snapshot = new ArrayList<>();
            for (DiagnosticEvent event : diagnosticEvents) snapshot.add(event.copy());
            return snapshot;
        }
    }
    void clearDiagnosticEvents() {
        MAIN.removeCallbacks(diagnosticRefresh);
        synchronized (diagnosticEvents) {
            diagnosticEvents.clear();
            latestNetworkEvents.clear();
            performanceResourceKeys.clear();
        }
        changed();
    }
    boolean bookmarked() {
        return !home && preferences.isBookmarked(identity.origin(), url);
    }
    String userAgentMode() { return userAgentMode; }
    void toggleBookmark() {
        if (closed || home || !allowed(Uri.parse(url))) return;
        preferences.toggleBookmark(identity.origin(), url, view.getTitle(), System.currentTimeMillis());
        changed();
    }
    void removeBookmark(String target) {
        if (closed || target == null || target.isEmpty()) return;
        preferences.removeBookmark(identity.origin(), target);
        changed();
    }
    void clearHistory() {
        preferences.clearHistory(identity.origin());
        changed();
    }
    void removeHistory(String target) {
        if (closed || target == null || target.isEmpty()) return;
        preferences.removeHistory(identity.origin(), target);
        changed();
    }
    void clearBookmarks() {
        preferences.clearBookmarks(identity.origin());
        changed();
    }
    void saveUserAgentMode(String mode, Runnable complete) {
        if (!BrowserPreferences.UA_ANDROID.equals(mode)
            && !BrowserPreferences.UA_DESKTOP.equals(mode)
            && !BrowserPreferences.UA_IPHONE.equals(mode)) {
            throw new IllegalArgumentException("网站显示方式无效");
        }
        if (closed || userAgentMode.equals(mode)) {
            if (complete != null) complete.run();
            return;
        }
        userAgentMode = mode;
        preferences.saveUserAgentMode(identity.origin(), mode);
        applyDisplayMode(view.getSettings());
        if (!home && attached) {
            reload();
        }
        changed();
        if (complete != null) complete.run();
    }
    boolean canDeliverFiles(long generation) {
        return !closed && attached && !home && generation == pageGeneration;
    }
    void navigate(String input) {
        if (closed || !attached) return;
        if (!routeReady) throw new IllegalArgumentException("网络通道正在更新，请稍后再试");
        String target;
        if (BrowserRoutePolicy.isWorkspaceViewUrl(input)) {
            localFileThreadId = BrowserRoutePolicy.workspaceViewThread(input);
            localFileOrigin = identity.origin();
            target = input.trim();
        } else if (BrowserRoutePolicy.isLocalFileInput(input)) {
            if (localFileThreadId.isEmpty() || localFileOrigin.isEmpty())
                throw new IllegalArgumentException("本地文件请从远程会话的文件链接打开");
            target = BrowserRoutePolicy.workspaceViewUrl(localFileOrigin, localFileThreadId, input);
        } else {
            target = BrowserRoutePolicy.navigationUrl(input);
        }
        diagnostic("native.browser.navigation", DiagnosticsPolicy.urlFields(target,
            "phase", "request", "routeName", "computer"));
        if (home) view.clearHistory();
        boolean sameDocumentLoad = sameDocument(view.getUrl(), target);
        view.stopLoading();
        if (sameDocumentLoad) {
            // Fragment and same-URL loads fire no page callbacks, so the loaded document must stay visible.
            view.setVisibility(View.VISIBLE);
            mainFrameStarted = true;
            mainFrameFinished = true;
            progress = 100;
        } else {
            view.setVisibility(View.INVISIBLE);
            mainFrameStarted = false;
            mainFrameFinished = false;
            pageTitle = "";
            progress = 0;
        }
        pendingNavigationUrl = target;
        home = false; error = ""; url = target; proxy.setActive(true); ++pageGeneration;
        view.onResume(); view.resumeTimers(); view.loadUrl(target); changed();
    }
    void showHome() {
        home = true;
        error = "";
        url = "";
        pendingNavigationUrl = "";
        mainFrameStarted = false;
        mainFrameFinished = false;
        pageTitle = "";
        progress = 100;
        view.stopLoading();
        view.clearHistory();
        view.setVisibility(View.INVISIBLE);
        proxy.setActive(false);
        view.onPause();
        view.pauseTimers();
        changed();
    }
    boolean back() {
        if (!home && view.canGoBack()) {
            error = "";
            android.webkit.WebBackForwardList history = view.copyBackForwardList();
            int previousIndex = history.getCurrentIndex() - 1;
            pendingNavigationUrl = previousIndex >= 0
                ? history.getItemAtIndex(previousIndex).getUrl() : "";
            view.goBack();
            return true;
        }
        if (!home) { showHome(); return true; }
        return false;
    }
    void reload() {
        if (!closed && attached && !home) {
            error = "";
            String currentUrl = view.getUrl();
            pendingNavigationUrl = currentUrl == null || currentUrl.isEmpty() ? url : currentUrl;
            mainFrameStarted = false;
            mainFrameFinished = false;
            pageTitle = "";
            progress = 0;
            view.setVisibility(View.INVISIBLE);
            view.reload();
            changed();
        }
    }

    private void recordDiagnosticEvent(String phase, String method, String url, String status,
        String detail, boolean mainFrame, String headers) {
        String safeUrl = redactDiagnosticUrl(url);
        String safeDetail = detail == null ? "" : detail.length() > 1024 ? detail.substring(0, 1024) : detail;
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                phase == null ? "" : phase, method == null ? "" : method,
                safeUrl, status == null ? "" : status, safeDetail,
                headers == null ? "" : headers, mainFrame, "", 0L, 0L,
                parseStatusCode(status), phase != null && "http-error".equals(phase),
                phase != null && ("failure".equals(phase) || "ssl-error".equals(phase)));
            diagnosticEvents.addLast(event);
            trimDiagnosticEventsLocked();
        }
        MAIN.removeCallbacks(diagnosticRefresh);
        MAIN.postDelayed(diagnosticRefresh, 120L);
    }

    private void recordNetworkRequest(String method, String url, boolean mainFrame,
        String detail, String headers) {
        String safeUrl = redactDiagnosticUrl(url);
        DiagnosticEvent event;
        synchronized (diagnosticEvents) {
            event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                "request", method == null ? "GET" : method, safeUrl, "", detail, headers,
                mainFrame, inferResourceType(Uri.parse(url), mainFrame), 0L, 0L, 0, false, false);
            diagnosticEvents.addLast(event);
            latestNetworkEvents.put(networkKey(method, safeUrl), event);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void ensureNetworkRequest(String method, String url, boolean mainFrame, String resourceType) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            DiagnosticEvent existing = latestNetworkEvents.get(networkKey(method, safeUrl));
            if (existing != null && !existing.failure) return;
            DiagnosticEvent event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                "request", method == null ? "GET" : method, safeUrl, "", "", "",
                mainFrame, resourceType == null || resourceType.isEmpty()
                    ? inferResourceType(Uri.parse(url), mainFrame) : resourceType,
                0L, 0L, 0, false, false);
            diagnosticEvents.addLast(event);
            latestNetworkEvents.put(networkKey(method, safeUrl), event);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void finishNetworkRequest(String method, String url, int statusCode, String status,
        String detail, boolean mainFrame, String resourceType, boolean failure) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = latestNetworkEvents.get(networkKey(method, safeUrl));
            if (event == null) {
                event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                    "request", method == null ? "GET" : method, safeUrl, "", "", "",
                    mainFrame, resourceType == null ? "" : resourceType, 0L, 0L,
                    0, false, false);
                diagnosticEvents.addLast(event);
                latestNetworkEvents.put(networkKey(method, safeUrl), event);
            }
            event.status = status == null ? "" : status;
            event.detail = detail == null ? "" : detail;
            event.mainFrame = mainFrame;
            if (resourceType != null && !resourceType.isEmpty()) event.resourceType = resourceType;
            event.statusCode = statusCode;
            event.response = statusCode > 0 || "200".equals(status) || "完成".equals(status);
            event.failure = failure;
            event.durationMs = Math.max(0L, System.currentTimeMillis() - event.timeMs);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void completeNetworkRequest(String method, String url, int statusCode, String status,
        String resourceType, long sizeBytes, boolean response) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = latestNetworkEvents.get(networkKey(method, safeUrl));
            if (event == null) {
                event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                    "request", method == null ? "GET" : method, safeUrl, "", "", "",
                    true, resourceType == null ? "document" : resourceType, 0L, 0L,
                    0, false, false);
                diagnosticEvents.addLast(event);
                latestNetworkEvents.put(networkKey(method, safeUrl), event);
            }
            event.status = status == null ? "" : status;
            event.statusCode = statusCode;
            event.resourceType = resourceType == null || resourceType.isEmpty()
                ? event.resourceType : resourceType;
            event.sizeBytes = Math.max(0L, sizeBytes);
            event.response = response;
            event.durationMs = Math.max(event.durationMs, System.currentTimeMillis() - event.timeMs);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void collectPerformanceSnapshot(WebView webView) {
        if (Build.VERSION.SDK_INT < 19 || webView == null) return;
        String script = "(function(){try{var n=performance.getEntriesByType('navigation')[0];"
            + "var r=performance.getEntriesByType('resource').map(function(e){return{"
            + "name:e.name,initiatorType:e.initiatorType,duration:e.duration,"
            + "transferSize:e.transferSize||0,encodedBodySize:e.encodedBodySize||0,"
            + "startTime:e.startTime};});return JSON.stringify({navigation:n?{name:location.href,"
            + "duration:n.duration,transferSize:n.transferSize||0}:null,resources:r});}"
            + "catch(e){return JSON.stringify({resources:[]});}})()";
        webView.evaluateJavascript(script, this::applyPerformanceSnapshot);
    }

    private void applyPerformanceSnapshot(String encoded) {
        if (encoded == null || encoded.isEmpty()) return;
        try {
            Object value = new JSONTokener(encoded).nextValue();
            if (!(value instanceof String)) return;
            JSONObject root = new JSONObject((String) value);
            JSONObject navigation = root.optJSONObject("navigation");
            if (navigation != null) {
                updatePerformanceEvent(navigation.optString("name", url), "document",
                    navigation.optDouble("duration", 0d),
                    navigation.optLong("transferSize", 0L));
            }
            JSONArray resources = root.optJSONArray("resources");
            if (resources == null) return;
            for (int index = 0; index < resources.length(); index++) {
                JSONObject resource = resources.optJSONObject(index);
                if (resource == null) continue;
                String target = resource.optString("name", "");
                if (target.isEmpty()) continue;
                String key = redactDiagnosticUrl(target) + "|" + resource.optDouble("startTime", 0d);
                synchronized (diagnosticEvents) {
                    if (!performanceResourceKeys.add(key)) continue;
                }
                updatePerformanceEvent(target, resource.optString("initiatorType", ""),
                    resource.optDouble("duration", 0d),
                    Math.max(resource.optLong("transferSize", 0L),
                        resource.optLong("encodedBodySize", 0L)));
            }
        } catch (JSONException ignored) {
            // Performance entries are an optional enhancement and must not affect loading.
        } catch (RuntimeException ignored) {
            // Performance entries are an optional enhancement and must not affect loading.
        }
    }

    private void updatePerformanceEvent(String url, String resourceType, double duration,
        long sizeBytes) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = latestNetworkEvents.get(networkKey("GET", safeUrl));
            if (event == null) {
                for (DiagnosticEvent candidate : diagnosticEvents) {
                    if ("request".equals(candidate.phase) && safeUrl.equals(candidate.url)) {
                        event = candidate;
                    }
                }
            }
            if (event == null) return;
            event.resourceType = normalizeResourceType(resourceType, event.resourceType);
            event.durationMs = Math.max(event.durationMs, Math.round(Math.max(0d, duration)));
            event.sizeBytes = Math.max(event.sizeBytes, Math.max(0L, sizeBytes));
            if (!event.failure && event.statusCode == 0) {
                event.statusCode = 200;
                event.status = "200";
            }
            event.response = true;
        }
        scheduleDiagnosticRefresh();
    }

    private void finalizeNetworkEvents() {
        long now = System.currentTimeMillis();
        synchronized (diagnosticEvents) {
            for (DiagnosticEvent event : diagnosticEvents) {
                if (!"request".equals(event.phase) || event.failure || event.response
                    || "resource-cancelled".equals(event.status)) continue;
                event.statusCode = 200;
                event.status = "200";
                event.response = true;
                event.durationMs = Math.max(1L, now - event.timeMs);
            }
        }
        scheduleDiagnosticRefresh();
    }

    private static String networkKey(String method, String url) {
        return (method == null ? "GET" : method.toUpperCase(Locale.ROOT)) + "|" + (url == null ? "" : url);
    }

    private static int parseStatusCode(String status) {
        if (status == null || status.isEmpty()) return 0;
        try { return Integer.parseInt(status); }
        catch (NumberFormatException ignored) { return 0; }
    }

    private void trimDiagnosticEventsLocked() {
        while (diagnosticEvents.size() > DIAGNOSTIC_EVENT_LIMIT) {
            DiagnosticEvent removed = diagnosticEvents.removeFirst();
            if (removed != null) latestNetworkEvents.values().removeIf(value -> value == removed);
        }
    }

    private void scheduleDiagnosticRefresh() {
        MAIN.removeCallbacks(diagnosticRefresh);
        MAIN.postDelayed(diagnosticRefresh, 120L);
    }

    private static String normalizeResourceType(String value, String fallback) {
        if (value == null || value.isEmpty()) return fallback == null ? "other" : fallback;
        if ("script".equalsIgnoreCase(value)) return "script";
        if ("css".equalsIgnoreCase(value) || "stylesheet".equalsIgnoreCase(value)) return "stylesheets";
        if ("img".equalsIgnoreCase(value) || "image".equalsIgnoreCase(value)) return "image";
        if ("font".equalsIgnoreCase(value)) return "font";
        if ("xmlhttprequest".equalsIgnoreCase(value) || "fetch".equalsIgnoreCase(value)) return "xhr";
        if ("document".equalsIgnoreCase(value) || "navigation".equalsIgnoreCase(value)) return "document";
        return value;
    }

    private static String inferResourceType(Uri target, boolean mainFrame) {
        if (mainFrame) return "document";
        if (target == null) return "other";
        String path = target.getPath() == null ? "" : target.getPath().toLowerCase(Locale.ROOT);
        if (path.endsWith(".js") || path.endsWith(".mjs")) return "script";
        if (path.endsWith(".css")) return "stylesheets";
        if (path.matches(".*\\.(png|jpe?g|gif|webp|svg|ico|avif)$")) return "image";
        if (path.matches(".*\\.(woff2?|ttf|otf|eot)$")) return "font";
        if (path.contains("/api/") || path.contains("/graphql") || path.contains("/ajax/")
            || "POST".equalsIgnoreCase(target.getQueryParameter("method"))) return "xhr";
        return "other";
    }

    private static String sanitizedHeaders(java.util.Map<String, String> values) {
        if (values == null || values.isEmpty()) return "";
        StringBuilder result = new StringBuilder();
        for (java.util.Map.Entry<String, String> entry : values.entrySet()) {
            String name = entry.getKey() == null ? "" : entry.getKey().trim();
            if (isSensitiveHeader(name)) continue;
            String value = entry.getValue() == null ? "" : entry.getValue().trim();
            if (value.length() > 512) value = value.substring(0, 512);
            if (result.length() > 0) result.append('\n');
            result.append(name).append(": ").append(value);
            if (result.length() > 4096) break;
        }
        return result.toString();
    }
    private static boolean isSensitiveHeader(String name) {
        String normalized = name == null ? "" : name.trim().toLowerCase(java.util.Locale.ROOT);
        return normalized.isEmpty() || "cookie".equals(normalized)
            || "authorization".equals(normalized) || "proxy-authorization".equals(normalized)
            || "set-cookie".equals(normalized) || "x-api-key".equals(normalized)
            || "x-auth-token".equals(normalized) || "x-access-token".equals(normalized)
            || "x-csrf-token".equals(normalized) || normalized.contains("password")
            || normalized.contains("secret");
    }
    private static String redactDiagnosticUrl(String value) {
        if (value == null || value.isEmpty()) return "";
        String result = value;
        String[] names = {
            "access_token", "accessToken", "api_key", "apiKey", "auth", "authorization",
            "code", "key", "password", "secret", "session", "token"
        };
        for (String name : names) {
            result = result.replaceAll(
                "(?i)([?#&]" + java.util.regex.Pattern.quote(name) + "=)[^&#]*",
                "$1<redacted>");
        }
        return result.length() > 4096 ? result.substring(0, 4096) : result;
    }
    private static boolean ignorableResourceFailure(Uri target, String detail) {
        String path = target == null ? "" : target.getPath();
        String message = detail == null ? "" : detail.toLowerCase(Locale.ROOT);
        return path != null && path.toLowerCase(Locale.ROOT).endsWith("/favicon.ico")
            && (message.contains("err_socket_not_connected")
                || message.contains("err_aborted")
                || message.contains("err_connection_reset")
                || message.contains("err_failed"));
    }
    private void applyDisplayMode(WebSettings settings) {
        boolean desktop = BrowserPreferences.UA_DESKTOP.equals(userAgentMode);
        desktopRootScrollLocked = false;
        String userAgent = BrowserPreferences.UA_DESKTOP.equals(userAgentMode)
            ? desktopUserAgent
            : BrowserPreferences.UA_IPHONE.equals(userAgentMode)
            ? iphoneUserAgent : androidUserAgent;
        settings.setUserAgentString(userAgent);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(true);
        settings.setLayoutAlgorithm(desktop
            ? WebSettings.LayoutAlgorithm.NORMAL
            : WebSettings.LayoutAlgorithm.TEXT_AUTOSIZING);
        // Desktop UA must retain pinch zoom as well. The UA changes layout and
        // server-side content selection; it must not remove the browser's zoom
        // gesture or prevent users from fitting more of a page on screen.
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        view.setHorizontalScrollBarEnabled(false);
        view.setOverScrollMode(View.OVER_SCROLL_NEVER);
        view.setNestedScrollingEnabled(false);
        applyUserAgentMetadata(settings);
        applyDesktopViewport(desktop);
        diagnostic("native.browser.display", DiagnosticsPolicy.fields(
            "phase", "success", "reason", "success"));
    }
    private void applyDesktopViewport(boolean desktop) {
        if (!desktop) {
            if (desktopViewportScript != null) desktopViewportScript.remove();
            desktopViewportScript = null;
            return;
        }
        if (desktopViewportScript == null
            && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            // The override is optional; a rejected script must not stop the browser from opening.
            try {
                desktopViewportScript = WebViewCompat.addDocumentStartJavaScript(
                    view, DESKTOP_VIEWPORT_SCRIPT, Collections.singleton("*"));
            } catch (RuntimeException unavailable) {
                diagnostic("native.browser.display", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                    "reason", "unsupported"));
            }
        }
    }
    private void updateDesktopRootScrollPolicy() {
        if (closed || home || !BrowserPreferences.UA_DESKTOP.equals(userAgentMode)
            || view.getHeight() <= 0) {
            desktopRootScrollLocked = false;
            return;
        }
        int contentHeight = Math.round(view.getContentHeight() * view.getScale());
        desktopRootScrollLocked = contentHeight > 0 && contentHeight <= view.getHeight() + 8;
        if (desktopRootScrollLocked) view.scrollTo(view.getScrollX(), 0);
        String overflowY = desktopRootScrollLocked ? "hidden" : "";
        String overflowX = "auto";
        view.evaluateJavascript(
            "(function(){var d=document.documentElement,b=document.body,oy='" + overflowY
                + "',ox='" + overflowX + "';"
                + "if(d){d.style.overflowY=oy;d.style.overflowX=ox;}"
                + "if(b){b.style.overflowY=oy;b.style.overflowX=ox;}})()",
            null);
    }
    private void applyUserAgentMetadata(WebSettings settings) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA)) return;
        boolean desktop = BrowserPreferences.UA_DESKTOP.equals(userAgentMode);
        boolean iphone = BrowserPreferences.UA_IPHONE.equals(userAgentMode);
        String chrome = chromeVersion(mobileUserAgent);
        UserAgentMetadata.Builder metadata = new UserAgentMetadata.Builder()
            .setMobile(!desktop)
            .setFullVersion(chrome)
            .setPlatform(desktop ? "Windows" : iphone ? "iOS" : "Android")
            .setPlatformVersion(desktop ? "10.0.0"
                : iphone ? "17.0.0" : matchGroup(mobileUserAgent, "Android\\s+([^;)]*)", "13"))
            .setArchitecture(desktop ? "x86" : "arm")
            .setBitness(desktop ? 64 : UserAgentMetadata.BITNESS_DEFAULT)
            .setWow64(false);
        if (WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA_FORM_FACTORS)) {
            metadata.setFormFactors(List.of(
                desktop ? UserAgentMetadata.FORM_FACTOR_DESKTOP : UserAgentMetadata.FORM_FACTOR_MOBILE));
        }
        WebSettingsCompat.setUserAgentMetadata(settings, metadata.build());
    }
    private static String androidUserAgent(String source) {
        String chrome = chromeVersion(source);
        String android = matchGroup(source, "Android\\s+([^;)]*)", "13");
        return "Mozilla/5.0 (Linux; Android " + android + "; K) "
            + "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/" + chrome
            + " Mobile Safari/537.36";
    }
    private static String desktopUserAgent(String source) {
        String chrome = chromeVersion(source);
        return "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
            + "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/" + chrome
            + " Safari/537.36";
    }
    private static String iphoneUserAgent() {
        return "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) "
            + "AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 "
            + "Mobile/15E148 Safari/604.1";
    }
    private static String chromeVersion(String source) {
        return matchGroup(source, "Chrome/([0-9.]+)", "120.0.0.0");
    }
    private static String matchGroup(String source, String expression, String fallback) {
        if (source == null) return fallback;
        Matcher matcher = Pattern.compile(expression).matcher(source);
        return matcher.find() ? matcher.group(1) : fallback;
    }
    private void changed() { if (!closed && listener != null) listener.changed(); }
    private void diagnostic(String name, JSONObject fields) {
        BrowserAuthority.recordDiagnostic(authority, name, fields);
    }

    private void invalidate(String message) {
        if (closed) return;
        Listener previous = listener;
        destroy();
        if (current == this) {
            current = null; ++openGeneration; afterClear = null;
        }
        if (previous != null) previous.invalidated(message);
    }

    private void destroy() {
        if (closed) return;
        closed = true; attached = false; listener = null; ++proxyGeneration;
        flushCookies();
        MAIN.removeCallbacks(diagnosticRefresh);
        proxy.close();
        try { observer.close(); } catch (Exception ignored) { }
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        view.stopLoading(); view.setWebChromeClient(null); view.setWebViewClient(null); view.destroy();
        context.setBaseContext(context.getApplicationContext());
    }

    private static void flushCookies() {
        try {
            CookieManager.getInstance().flush();
        } catch (RuntimeException ignored) {
            // Cookie persistence is best effort during Activity/process teardown.
        }
    }
}
