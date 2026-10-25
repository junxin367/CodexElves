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
import androidx.webkit.UserAgentMetadata;
import androidx.webkit.WebStorageCompat;
import androidx.webkit.WebSettingsCompat;
import androidx.webkit.WebViewFeature;
import org.json.JSONObject;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/** Main-thread-owned browser state; never retains a detached Activity. */
final class BrowserSession {
    static final class DiagnosticEvent {
        final long timeMs;
        final String phase;
        final String method;
        final String url;
        final String status;
        final String detail;
        final String headers;
        final boolean mainFrame;

        DiagnosticEvent(long timeMs, String phase, String method, String url,
            String status, String detail, String headers, boolean mainFrame) {
            this.timeMs = timeMs;
            this.phase = phase;
            this.method = method;
            this.url = url;
            this.status = status;
            this.detail = detail;
            this.headers = headers;
            this.mainFrame = mainFrame;
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
    private boolean mainFrameStarted;
    private boolean mainFrameFinished;
    private String mobileUserAgent = "";
    private String androidUserAgent = "";
    private String desktopUserAgent = "";
    private String iphoneUserAgent = "";
    private String userAgentMode;
    private String pageTitle = "";
    private boolean desktopRootScrollLocked;
    private boolean desktopFullPageFitReload;
    private final ArrayDeque<DiagnosticEvent> diagnosticEvents = new ArrayDeque<>();
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
                    recordDiagnosticEvent("request", request.getMethod(), resource.toString(), "",
                        request.isRedirect() ? "redirect" : "", request.isForMainFrame(),
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
                recordDiagnosticEvent("page-start", "GET", address, "", "", true, "");
                diagnostic("native.browser.page", DiagnosticsPolicy.urlFields(address,
                    "phase", "start", "generation", pageGeneration));
                webView.setVisibility(desktopFullPageFitReload ? View.INVISIBLE : View.VISIBLE);
                url = address; error = ""; progress = 0; pageTitle = ""; changed();
            }
            @Override public void onPageFinished(WebView webView, String address) {
                if (home || !mainFrameStarted) return;
                mainFrameStarted = true;
                pendingNavigationUrl = address;
                boolean fittingReload = desktopFullPageFitReload;
                desktopFullPageFitReload = false;
                if (BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    webView.scrollTo(0, webView.getScrollY());
                    MAIN.postDelayed(desktopRootScrollPolicyRefresh, 120L);
                    if (!fittingReload) {
                        long fitGeneration = pageGeneration;
                        MAIN.postDelayed(
                            () -> requestDesktopFullPageFit(webView, address, fitGeneration), 180L);
                        MAIN.postDelayed(
                            () -> requestDesktopFullPageFit(webView, address, fitGeneration), 800L);
                    }
                }
                webView.setVisibility(View.VISIBLE);
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
                    recordDiagnosticEvent("request", request.getMethod(), target, "",
                        "resource-cancelled", false, "");
                    return;
                }
                recordDiagnosticEvent("failure", request.getMethod(), target, "", detail,
                    mainFrame, "");
                if (mainFrame) {
                    desktopFullPageFitReload = false;
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

    void attach(Activity activity, Listener listener, Runnable ready) {
        if (closed) { listener.invalidated("浏览器已关闭"); return; }
        this.listener = listener;
        context.setBaseContext(activity);
        applyRules(() -> {
            if (closed || this.listener != listener || activity.isFinishing()) return;
            attached = true;
            proxy.setActive(!home);
            if (!home) { view.onResume(); view.resumeTimers(); }
            ready.run(); changed();
        }, () -> {
            if (!closed && this.listener == listener) {
                listener.invalidated("网络设置应用失败，请返回会话后重试");
            }
        });
    }

    void detach(Listener owner) {
        if (listener != owner && context.getBaseContext() != owner) return;
        listener = null; attached = false; ++proxyGeneration;
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

    long pageGeneration() { return pageGeneration; }
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
            return new ArrayList<>(diagnosticEvents);
        }
    }
    void clearDiagnosticEvents() {
        MAIN.removeCallbacks(diagnosticRefresh);
        synchronized (diagnosticEvents) {
            diagnosticEvents.clear();
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
        String target = BrowserRoutePolicy.navigationUrl(input);
        diagnostic("native.browser.navigation", DiagnosticsPolicy.urlFields(target,
            "phase", "request", "routeName", "computer"));
        if (home) view.clearHistory();
        view.stopLoading();
        view.setVisibility(View.INVISIBLE);
        pendingNavigationUrl = target;
        desktopFullPageFitReload = false;
        mainFrameStarted = false;
        mainFrameFinished = false;
        pageTitle = "";
        progress = 0;
        home = false; error = ""; url = target; proxy.setActive(true); ++pageGeneration;
        view.onResume(); view.resumeTimers(); view.loadUrl(target); changed();
    }
    void showHome() {
        home = true;
        error = "";
        url = "";
        pendingNavigationUrl = "";
        desktopFullPageFitReload = false;
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
            desktopFullPageFitReload = false;
            mainFrameStarted = false;
            mainFrameFinished = false;
            pageTitle = "";
            progress = 0;
            view.setVisibility(View.INVISIBLE);
            view.goBack();
            return true;
        }
        if (!home) { showHome(); return true; }
        return false;
    }
    void reload() {
        if (!closed && attached && !home) {
            error = "";
            pendingNavigationUrl = url;
            desktopFullPageFitReload = false;
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
        DiagnosticEvent event = new DiagnosticEvent(System.currentTimeMillis(),
            phase == null ? "" : phase, method == null ? "" : method,
            safeUrl, status == null ? "" : status, safeDetail,
            headers == null ? "" : headers, mainFrame);
        synchronized (diagnosticEvents) {
            diagnosticEvents.addLast(event);
            while (diagnosticEvents.size() > DIAGNOSTIC_EVENT_LIMIT) diagnosticEvents.removeFirst();
        }
        MAIN.removeCallbacks(diagnosticRefresh);
        MAIN.postDelayed(diagnosticRefresh, 120L);
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
        settings.setLoadWithOverviewMode(!desktop);
        settings.setLayoutAlgorithm(desktop
            ? WebSettings.LayoutAlgorithm.NORMAL
            : WebSettings.LayoutAlgorithm.TEXT_AUTOSIZING);
        view.setInitialScale(100);
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
        diagnostic("native.browser.display", DiagnosticsPolicy.fields(
            "phase", "success", "reason", "success"));
    }
    private void requestDesktopFullPageFit(WebView webView, String address, long generation) {
        if (closed || !attached || home || !BrowserPreferences.UA_DESKTOP.equals(userAgentMode)
            || desktopFullPageFitReload || generation != pageGeneration
            || webView.getWidth() <= 0 || webView.getHeight() <= 0) {
            return;
        }
        webView.evaluateJavascript(
            "(function(){var d=document.documentElement,b=document.body;"
                + "var w=Math.max(d?d.scrollWidth:0,d?d.offsetWidth:0,d?d.clientWidth:0,"
                + "b?b.scrollWidth:0,b?b.offsetWidth:0,b?b.clientWidth:0);"
                + "var h=Math.max(d?d.scrollHeight:0,d?d.offsetHeight:0,d?d.clientHeight:0,"
                + "b?b.scrollHeight:0,b?b.offsetHeight:0,b?b.clientHeight:0);"
                + "return Math.round(w)+','+Math.round(h);})()",
            value -> {
                if (closed || !attached || home || generation != pageGeneration
                    || desktopFullPageFitReload || !sameAddress(address, url)) {
                    return;
                }
                int[] dimensions = pageDimensions(value);
                if (dimensions == null) return;
                int scalePercent = BrowserScalePolicy.fullPageScalePercent(
                    webView.getWidth(), webView.getHeight(), dimensions[0], dimensions[1]);
                diagnostic("native.browser.scale", DiagnosticsPolicy.fields(
                    "phase", "measure", "reason", "full-page-fit",
                    "contentWidth", dimensions[0], "contentHeight", dimensions[1],
                    "viewportWidth", webView.getWidth(), "viewportHeight", webView.getHeight(),
                    "scalePercent", scalePercent, "generation", generation));
                if (scalePercent >= 100) {
                    webView.setVisibility(View.VISIBLE);
                    return;
                }
                desktopFullPageFitReload = true;
                webView.setInitialScale(scalePercent);
                webView.setVisibility(View.INVISIBLE);
                webView.reload();
            });
    }
    private static int[] pageDimensions(String value) {
        if (value == null || value.isEmpty()) return null;
        String decoded = value;
        try {
            Object parsed = new org.json.JSONTokener(value).nextValue();
            if (parsed != null) decoded = parsed.toString();
        } catch (Exception ignored) { }
        String[] parts = decoded.split(",");
        if (parts.length < 2) return null;
        try {
            int width = Integer.parseInt(parts[0].trim());
            int height = Integer.parseInt(parts[1].trim());
            return width > 0 && height > 0 ? new int[] { width, height } : null;
        } catch (NumberFormatException ignored) {
            return null;
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
            current = null; ++openGeneration; afterClear = null; clearProfile(authority);
        }
        if (previous != null) previous.invalidated(message);
    }

    private void destroy() {
        if (closed) return;
        closed = true; attached = false; listener = null; ++proxyGeneration;
        MAIN.removeCallbacks(diagnosticRefresh);
        proxy.close();
        try { observer.close(); } catch (Exception ignored) { }
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        view.stopLoading(); view.setWebChromeClient(null); view.setWebViewClient(null); view.destroy();
        context.setBaseContext(context.getApplicationContext());
    }
}
