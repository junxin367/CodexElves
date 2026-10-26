package com.junxin367.codexelves;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.net.ConnectivityManager;
import android.net.NetworkCapabilities;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.os.ext.SdkExtensions;
import android.provider.MediaStore;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.webkit.CookieManager;
import android.webkit.ConsoleMessage;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.URLUtil;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.Toast;

import androidx.webkit.JavaScriptReplyProxy;
import androidx.core.content.FileProvider;
import androidx.webkit.WebMessageCompat;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;

import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayInputStream;
import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.BooleanSupplier;

public final class MainActivity extends Activity {
    private static final String BRIDGE_NAME = "CodexElvesAndroid";
    private static final String DEVICE_COOKIE = "codex_elves_device=";
    private static final String ZROK_HEADER = "skip_zrok_interstitial";
    private static final int FILE_CHOOSER_REQUEST = 4107;
    private static final long READY_TIMEOUT_MS = 15_000L;
    private static final long UPDATE_CHECK_INTERVAL_MS = 15L * 60L * 1000L;
    private static final long MAX_ATTACHMENT_BYTES = 512L * 1024L * 1024L;

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ExecutorService background = Executors.newSingleThreadExecutor();
    private final ExecutorService resources = Executors.newSingleThreadExecutor();
    private final AtomicLong connectionGeneration = new AtomicLong();
    private final AtomicLong updateCheckGeneration = new AtomicLong(-1L);
    private final HapticFeedbackController haptics = new HapticFeedbackController();
    private final Object notificationVisibility = new Object();
    private final TunnelMediaClient tunnelMedia = new TunnelMediaClient();

    private SessionStore sessionStore;
    private ResourceStore resourceStore;
    private AppUpdateController appUpdates;
    private NotificationController notifications;
    private DiagnosticsStore diagnostics;
    private String diagnosticsOrigin = "";
    private String webViewVersion = "unknown";
    private FrameLayout root;
    private volatile WebView webView;
    private ResourceStore.Snapshot snapshot;
    private volatile String currentOrigin;
    private ResourceStore.Cancellation resourceCancellation;
    private TextView resourceStatus;
    private Runnable readyTimeout;
    private boolean pageReady;
    private boolean pageAuthenticated;
    private NotificationPublisher.OpenTarget pendingNotificationOpen;
    private NotificationPublisher.OpenTarget navigatingNotification;
    private long notificationNavigationAttempt;
    private long backAttempt;
    private boolean backPending;
    private android.window.OnBackInvokedCallback nativeBackCallback;
    private ValueCallback<Uri[]> fileChooserCallback;
    private int fileChooserRequestCode = -1;
    private int nextFileChooserRequestCode = FILE_CHOOSER_REQUEST;
    private long lastUpdateCheckAt;
    private BrowserAuthority browserAuthority;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        configureWindow();
        sessionStore = new SessionStore(this);
        resourceStore = new ResourceStore(this);
        diagnosticsOrigin = normalizedDiagnosticsOrigin(sessionStore.getOrigin());
        diagnostics = new DiagnosticsStore(this, diagnosticsOrigin);
        webViewVersion = readWebViewVersion();
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields(
            "phase", "create", "sdk", Build.VERSION.SDK_INT,
            "webViewVersion", webViewVersion, "network", diagnosticsNetwork()
        ));
        root = new FrameLayout(this);
        root.setBackgroundColor(getColor(R.color.login_background));
        root.setFitsSystemWindows(true);
        setContentView(root);
        appUpdates = new AppUpdateController(this);
        notifications = new NotificationController(this, this::handleNotificationAuthorizationLost);
        if (Build.VERSION.SDK_INT >= 33) {
            nativeBackCallback = this::onBackPressed;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, nativeBackCallback
            );
        }
        handleNotificationIntent(getIntent());

        String savedOrigin = sessionStore.getOrigin();
        if (sessionStore.isAuthorized() && !savedOrigin.isEmpty()) {
            try {
                openSession(ServiceOrigin.normalize(savedOrigin));
                return;
            } catch (IllegalArgumentException ignored) {
                sessionStore.setAuthorized(false);
            }
        }
        showLogin(null);
    }

    private void configureWindow() {
        Window window = getWindow();
        window.setStatusBarColor(getColor(R.color.login_background));
        window.setNavigationBarColor(getColor(R.color.login_background));
        window.getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
        );
    }

    private void showLogin(String message) {
        invalidateBrowser();
        notifications.onPageChanged();
        cancelResourceWork();
        connectionGeneration.incrementAndGet();
        pageReady = false;
        pageAuthenticated = false;
        pendingNotificationOpen = null;
        notificationNavigationAttempt++;
        navigatingNotification = null;
        backAttempt++;
        backPending = false;
        cancelReadyTimeout();
        destroyWebView();
        currentOrigin = null;
        snapshot = null;
        resourceStatus = null;
        root.removeAllViews();
        root.addView(buildLoginView(message), matchParent());
    }

    private View buildLoginView(String message) {
        LoginView login = new LoginView(this, haptics);
        login.setInitialValues(sessionStore.getOrigin(), message);
        login.setOnSubmit(() -> authenticate(login));
        login.setOnExport(() -> exportDiagnostics(
            () -> !isFinishing() && !isDestroyed() && webView == null, null, null
        ));
        return login;
    }

    private void authenticate(LoginView login) {
        if (login.isBusy()) {
            return;
        }
        EditText originInput = login.originInput;
        EditText keyInput = login.keyInput;
        TextView status = login.status;
        final String origin;
        try {
            origin = ServiceOrigin.normalize(originInput.getText().toString());
        } catch (IllegalArgumentException error) {
            showStatus(status, error.getMessage());
            return;
        }
        String key = keyInput.getText().toString();
        if (key.isEmpty()) {
            showStatus(status, "请输入访问密钥");
            return;
        }
        diagnostics.activateOrigin(origin);
        diagnosticsOrigin = origin;
        diagnostic("native.login", DiagnosticsPolicy.fields("phase", "request",
            "host", Uri.parse(origin).getHost(), "network", diagnosticsNetwork()));

        notifications.onPageChanged();
        NotificationService.stop(this, "auth-required", true);
        sessionStore.setAuthorized(false);
        sessionStore.saveOrigin(origin);
        login.setBusy(true);
        status.setTextColor(0xFF6B7280);
        showStatus(status, "正在连接…");
        long generation = connectionGeneration.incrementAndGet();

        background.execute(() -> {
            LoginResult result = requestLogin(origin, key, generation);
            runOnUiThread(() -> {
                if (generation != connectionGeneration.get() || isFinishing()) {
                    return;
                }
                login.setBusy(false);
                status.setTextColor(0xFFB42318);
                if (!result.success) {
                    diagnostic("native.login", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "login-failed"));
                    showStatus(status, result.message);
                    return;
                }
                CookieManager cookies = CookieManager.getInstance();
                cookies.setAcceptCookie(true);
                cookies.setCookie(origin, result.cookie, accepted -> {
                    if (generation != connectionGeneration.get() || isFinishing()) {
                        return;
                    }
                    if (!Boolean.TRUE.equals(accepted)) {
                        diagnostic("native.login", DiagnosticsPolicy.fields(
                            "phase", "failure", "reason", "cookie-failed"));
                        showStatus(status, "设备会话保存失败，请重试");
                        return;
                    }
                    cookies.flush();
                    keyInput.setText("");
                    sessionStore.saveAuthorizedConnection(origin, result.cookie);
                    diagnostic("native.login", DiagnosticsPolicy.fields(
                        "phase", "success", "reason", "login-success"));
                    openSession(origin);
                });
            });
        });
    }

    private LoginResult requestLogin(String origin, String key, long generation) {
        HttpURLConnection connection = null;
        long startedAt = android.os.SystemClock.elapsedRealtime();
        try {
            URL url = new URL(origin + "/api/connect");
            connection = (HttpURLConnection) url.openConnection();
            connection.setInstanceFollowRedirects(false);
            connection.setConnectTimeout(15_000);
            connection.setReadTimeout(20_000);
            connection.setRequestMethod("POST");
            connection.setDoOutput(true);
            connection.setRequestProperty("Origin", origin);
            connection.setRequestProperty("Content-Type", "application/json; charset=utf-8");
            connection.setRequestProperty("Accept", "application/json");
            connection.setRequestProperty(ZROK_HEADER, "1");
            byte[] body = new JSONObject().put("key", key)
                .toString()
                .getBytes(StandardCharsets.UTF_8);
            connection.setFixedLengthStreamingMode(body.length);
            try (java.io.OutputStream output = connection.getOutputStream()) {
                output.write(body);
            }

            int status = connection.getResponseCode();
            diagnostics.recordAtOrigin(origin, "native", "native.login.http",
                DiagnosticsPolicy.urlFields(origin + "/api/connect",
                    "method", "POST", "status", status,
                    "mime", DiagnosticsPolicy.mime(connection.getContentType()),
                    "durationMs", android.os.SystemClock.elapsedRealtime() - startedAt),
                () -> generation == connectionGeneration.get());
            if (status == HttpURLConnection.HTTP_UNAUTHORIZED
                || status == HttpURLConnection.HTTP_FORBIDDEN) {
                return LoginResult.failure("域名或访问密钥无效");
            }
            if (status >= 300 && status < 400) {
                return LoginResult.failure("服务拒绝跨地址跳转");
            }
            if (status < 200 || status >= 300) {
                return LoginResult.failure("连接失败，服务返回 " + status);
            }
            String cookie = findDeviceCookie(connection.getHeaderFields());
            if (cookie == null) {
                return LoginResult.failure("服务未返回设备会话");
            }
            return LoginResult.success(cookie);
        } catch (Exception error) {
            diagnostics.recordAtOrigin(origin, "native", "native.login.failed",
                DiagnosticsPolicy.fields("errorType", DiagnosticsPolicy.errorType(error),
                    "reason", "login-failed",
                    "durationMs", android.os.SystemClock.elapsedRealtime() - startedAt),
                () -> generation == connectionGeneration.get());
            return LoginResult.failure("无法连接服务，请检查网络和域名");
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
        }
    }

    private static String findDeviceCookie(Map<String, List<String>> headers) {
        for (Map.Entry<String, List<String>> header : headers.entrySet()) {
            if (header.getKey() == null || !"set-cookie".equalsIgnoreCase(header.getKey())) {
                continue;
            }
            for (String value : header.getValue()) {
                if (value == null || value.indexOf('\r') >= 0 || value.indexOf('\n') >= 0) {
                    continue;
                }
                String first = value.split(";", 2)[0].trim();
                if (first.startsWith(DEVICE_COOKIE) && first.length() > DEVICE_COOKIE.length()) {
                    return value;
                }
            }
        }
        return null;
    }

    private static String readDeviceCookie(String origin) {
        String allCookies = CookieManager.getInstance().getCookie(origin);
        if (allCookies == null) {
            return null;
        }
        for (String item : allCookies.split(";")) {
            String candidate = item.trim();
            if (candidate.matches("codex_elves_device=[A-Za-z0-9._~-]{16,512}")) {
                return candidate;
            }
        }
        return null;
    }

    private void openSession(String origin) {
        invalidateBrowser();
        notifications.onPageChanged();
        if (currentOrigin != null && !origin.equals(currentOrigin)) {
            NotificationService.stop(this, "auth-required", true);
        }
        cancelResourceWork();
        long generation = connectionGeneration.incrementAndGet();
        currentOrigin = origin;
        diagnostics.activateOrigin(origin);
        diagnosticsOrigin = origin;
        diagnostic("native.page", DiagnosticsPolicy.fields(
            "phase", "start", "generation", generation, "host", Uri.parse(origin).getHost()));
        resourceCancellation = new ResourceStore.Cancellation(
            () -> generation == connectionGeneration.get() && origin.equals(currentOrigin)
        );
        pageReady = false;
        pageAuthenticated = false;
        notificationNavigationAttempt++;
        navigatingNotification = null;
        backAttempt++;
        backPending = false;
        snapshot = null;
        resourceStatus = null;
        cancelReadyTimeout();
        destroyWebView();
        root.removeAllViews();
        restoreDeviceCookie(origin, generation, () -> prepareZrokCookie(origin, generation, accepted -> {
            if (generation != connectionGeneration.get() || isFinishing()) {
                return;
            }
            if (!accepted) {
                showSessionLaunchError(origin, "无法准备 zrok 公网访问，请重试");
                return;
            }
            startSessionPage(origin, generation);
        }));
    }

    private void restoreDeviceCookie(String origin, long generation, Runnable completion) {
        String cookie = sessionStore.getDeviceCookie();
        if (cookie == null || cookie.trim().isEmpty()) {
            completion.run();
            return;
        }
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setCookie(origin, cookie, accepted -> runOnUiThread(() -> {
            if (generation != connectionGeneration.get() || isFinishing()) {
                return;
            }
            if (Boolean.TRUE.equals(accepted)) {
                cookies.flush();
            } else {
                diagnostic("native.login", DiagnosticsPolicy.fields(
                    "phase", "cookie-restore-failed", "reason", "cookie-failed"));
            }
            completion.run();
        }));
    }

    private void startSessionPage(String origin, long generation) {
        TextView loading = text("正在校验本地页面资源…", 15, 0xFF6B7280);
        loading.setGravity(Gravity.CENTER);
        root.addView(loading, matchParent());
        ResourceStore.Cancellation cancellation = resourceCancellation;
        resources.execute(() -> {
            try {
                ResourceStore.Snapshot selected = resourceStore.selectForPage(origin, cancellation);
                runOnUiThread(() -> {
                    if (generation != connectionGeneration.get() || cancellation.isCancelled()
                        || isFinishing()) {
                        return;
                    }
                    snapshot = selected;
                    diagnostic("native.resources", DiagnosticsPolicy.fields(
                        "phase", "select", "reason", "success", "resourceVersion",
                        selected.manifest.version, "generation", generation));
                    root.removeAllViews();
                    displaySessionPage(origin, generation, selected);
                });
            } catch (IOException error) {
                runOnUiThread(() -> {
                    if (generation == connectionGeneration.get() && !cancellation.isCancelled()
                        && !isFinishing()) {
                        diagnostic("native.resources", DiagnosticsPolicy.fields(
                            "phase", "select", "reason", "select-failed",
                            "errorType", DiagnosticsPolicy.errorType(error),
                            "generation", generation));
                        showSessionLaunchError(origin, "本地页面资源不可用，请重新安装应用");
                    }
                });
            }
        });
    }

    private void displaySessionPage(
        String origin,
        long generation,
        ResourceStore.Snapshot pageSnapshot
    ) {
        WebView view = new WebView(this);
        webView = view;
        configureWebView(view, origin, generation, pageSnapshot);
        root.addView(view, matchParent());
        if (!registerBridge(view, origin, generation, pageSnapshot)) {
            showLogin("系统 WebView 版本过低，请先更新 Android System WebView");
            return;
        }
        readyTimeout = () -> {
            if (generation == connectionGeneration.get() && !pageReady) {
                handlePageStartupFailure(
                    "页面启动超时",
                    origin,
                    pageSnapshot.version,
                    generation
                );
            }
        };
        mainHandler.postDelayed(readyTimeout, READY_TIMEOUT_MS);
        view.loadUrl(origin + "/");
    }

    private void prepareZrokCookie(
        String origin,
        long generation,
        java.util.function.Consumer<Boolean> completion
    ) {
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        ZrokCookiePolicy.prepare(
            origin,
            generation,
            connectionGeneration::get,
            (cookieOrigin, cookie, callback) ->
                cookies.setCookie(cookieOrigin, cookie, callback::accept),
            accepted -> runOnUiThread(() -> {
                if (accepted) {
                    cookies.flush();
                }
                completion.accept(accepted);
            })
        );
    }

    private void refreshZrokCookie(String origin, long generation) {
        prepareZrokCookie(origin, generation, accepted -> {
            if (!accepted && generation == connectionGeneration.get()) {
                Toast.makeText(
                    this,
                    "zrok 公网访问状态刷新失败，当前页面保持不变",
                    Toast.LENGTH_SHORT
                ).show();
            }
        });
    }

    private void showSessionLaunchError(String origin, String message) {
        pageReady = false;
        cancelReadyTimeout();
        destroyWebView();
        snapshot = null;
        root.removeAllViews();

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setGravity(Gravity.CENTER);
        content.setPadding(dp(28), dp(28), dp(28), dp(28));
        TextView status = text(message, 16, 0xFFB42318);
        status.setGravity(Gravity.CENTER);
        content.addView(status, fullWidthWrap());

        Button retry = new Button(this);
        retry.setText("重试");
        retry.setAllCaps(false);
        retry.setOnClickListener(ignored -> openSession(origin));
        LinearLayout.LayoutParams retryParams =
            new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(52));
        retryParams.topMargin = dp(18);
        content.addView(retry, retryParams);

        long generation = connectionGeneration.get();
        Button export = new Button(this);
        export.setText(R.string.diagnostics_export);
        export.setAllCaps(false);
        export.setOnClickListener(ignored -> exportDiagnostics(
            () -> generation == connectionGeneration.get() && origin.equals(currentOrigin)
                && webView == null && !isFinishing() && !isDestroyed(),
            null,
            null
        ));
        LinearLayout.LayoutParams exportParams =
            new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(52));
        exportParams.topMargin = dp(8);
        content.addView(export, exportParams);
        root.addView(content, matchParent());
    }

    @SuppressLint("SetJavaScriptEnabled")
    private void configureWebView(
        WebView view,
        String origin,
        long generation,
        ResourceStore.Snapshot pageSnapshot
    ) {
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setDatabaseEnabled(false);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSupportMultipleWindows(false);
        settings.setJavaScriptCanOpenWindowsAutomatically(false);
        settings.setMediaPlaybackRequiresUserGesture(true);
        settings.setBuiltInZoomControls(false);
        settings.setDisplayZoomControls(false);
        settings.setSaveFormData(false);
        settings.setUserAgentString(settings.getUserAgentString() + " CodexElvesAndroid/1");
        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(view, false);

        view.setWebChromeClient(new ShellChromeClient(view, origin, generation));
        view.setDownloadListener((url, userAgent, contentDisposition, mimeType, contentLength) ->
            enqueueDownload(url, userAgent, contentDisposition, mimeType)
        );
        view.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageStarted(WebView requestView, String url, android.graphics.Bitmap icon) {
                if (isCurrentDiagnosticsPage(requestView, origin, generation)) {
                    diagnostic("native.page", DiagnosticsPolicy.urlFields(url,
                        "phase", "start", "resourceVersion", pageSnapshot.manifest.version,
                        "generation", generation, "network", diagnosticsNetwork()));
                }
            }

            @Override
            public void onPageFinished(WebView requestView, String url) {
                if (isCurrentDiagnosticsPage(requestView, origin, generation)) {
                    diagnostic("native.page", DiagnosticsPolicy.urlFields(url,
                        "phase", "complete", "resourceVersion", pageSnapshot.manifest.version,
                        "generation", generation));
                }
            }

            @Override
            public WebResourceResponse shouldInterceptRequest(
                WebView requestView,
                WebResourceRequest request
            ) {
                return interceptResource(
                    request,
                    origin,
                    pageSnapshot,
                    generation,
                    requestView
                );
            }

            @Override
            public boolean shouldOverrideUrlLoading(
                WebView requestView,
                WebResourceRequest request
            ) {
                Uri uri = request.getUrl();
                if (ServiceOrigin.matches(origin, uri)) {
                    return false;
                }
                if (request.isRedirect()) {
                    runOnUiThread(() ->
                        Toast.makeText(MainActivity.this, "已阻止跨源跳转", Toast.LENGTH_SHORT).show()
                    );
                    return true;
                }
                if (request.isForMainFrame()) {
                    if (isLocalOrLanAddress(uri)) {
                        diagnostic("native.browser.link", DiagnosticsPolicy.urlFields(
                            uri.toString(), "phase", "open", "routeName", "local-browser"));
                        openBrowserLink(uri);
                        return true;
                    }
                    openExternal(uri);
                }
                return true;
            }

            @Override
            public void onReceivedSslError(
                WebView requestView,
                SslErrorHandler handler,
                android.net.http.SslError error
            ) {
                handler.cancel();
                if (isCurrentDiagnosticsPage(requestView, origin, generation)) {
                    diagnostic("native.webview.ssl", DiagnosticsPolicy.urlFields(error.getUrl(),
                        "reason", "ssl-failed", "errorType", "ssl",
                        "errorCode", error.getPrimaryError(), "generation", generation));
                    runOnUiThread(() ->
                        Toast.makeText(
                            MainActivity.this,
                            "HTTPS 证书校验失败",
                            Toast.LENGTH_LONG
                        ).show()
                    );
                }
            }

            @Override
            public void onReceivedError(
                WebView requestView,
                WebResourceRequest request,
                WebResourceError error
            ) {
                if (isCurrentDiagnosticsPage(requestView, origin, generation)
                    && isDiagnosticResource(request)) {
                    diagnostic("native.webview.error", DiagnosticsPolicy.urlFields(
                        request.getUrl().toString(), "method", request.getMethod(),
                        "errorCode", error.getErrorCode(),
                        "errorType", error.getErrorCode() == WebViewClient.ERROR_TIMEOUT
                            ? "timeout" : "network",
                        "reason", error.getErrorCode() == WebViewClient.ERROR_TIMEOUT
                            ? "timeout" : "load-failed", "generation", generation));
                }
                if (request.isForMainFrame() && generation == connectionGeneration.get()) {
                    handlePageStartupFailure(
                        "页面加载失败",
                        origin,
                        pageSnapshot.version,
                        generation
                    );
                }
            }

            @Override
            public void onReceivedHttpError(
                WebView requestView,
                WebResourceRequest request,
                WebResourceResponse response
            ) {
                if (isCurrentDiagnosticsPage(requestView, origin, generation)
                    && isDiagnosticResource(request)) {
                    diagnostic("native.webview.http", DiagnosticsPolicy.urlFields(
                        request.getUrl().toString(), "method", request.getMethod(),
                        "status", response.getStatusCode(),
                        "mime", DiagnosticsPolicy.mime(response.getMimeType()),
                        "reason", "http-error", "generation", generation));
                }
            }

            @Override
            public boolean onRenderProcessGone(
                WebView requestView,
                RenderProcessGoneDetail detail
            ) {
                if (isCurrentDiagnosticsPage(requestView, origin, generation)) {
                    diagnostic("native.webview.renderer", DiagnosticsPolicy.fields(
                        "reason", detail.didCrash() ? "renderer-crashed" : "renderer-killed",
                        "errorType", "renderer", "generation", generation));
                }
                handleRendererExit(requestView, origin, generation);
                return true;
            }
        });
    }

    private boolean registerBridge(
        WebView view,
        String origin,
        long generation,
        ResourceStore.Snapshot pageSnapshot
    ) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            return false;
        }
        WebViewCompat.addWebMessageListener(
            view,
            BRIDGE_NAME,
            Collections.singleton(origin),
            (WebView sourceView,
             WebMessageCompat message,
             Uri sourceOrigin,
             boolean isMainFrame,
             JavaScriptReplyProxy replyProxy) -> {
                if (generation != connectionGeneration.get()
                    || sourceView != webView
                    || !isMainFrame
                    || !ServiceOrigin.matches(origin, sourceOrigin)) {
                    return;
                }
                String sourcePageUrl = sourceView.getUrl();
                if (sourcePageUrl == null
                    || !ServiceOrigin.matches(origin, Uri.parse(sourcePageUrl))) {
                    return;
                }
                String data = message.getData();
                if (data == null || data.length() > 4096) {
                    return;
                }
                final String type;
                final JSONObject payload;
                try {
                    payload = new JSONObject(data);
                    Object rawType = payload.opt("type");
                    if (!(rawType instanceof String)) {
                        return;
                    }
                    type = (String) rawType;
                    if (DiagnosticsPolicy.isRequestType(type)) {
                        if (!DiagnosticsPolicy.isValidRequest(payload)) return;
                    } else if ("browser-open".equals(type)) {
                        if (!validBrowserOpen(payload)) return;
                    } else if (!NotificationSettings.isRequestType(type) && payload.length() != 1) {
                        return;
                    }
                } catch (JSONException error) {
                    return;
                }
                runOnUiThread(() -> {
                    if (generation != connectionGeneration.get() || sourceView != webView
                        || !origin.equals(currentOrigin) || isFinishing() || isDestroyed()) {
                        return;
                    }
                    if (DiagnosticsPolicy.isRequestType(type)) {
                        BooleanSupplier current = () ->
                            isCurrentDiagnosticsPage(sourceView, origin, generation)
                                && sourcePageUrl.equals(sourceView.getUrl());
                        if (!current.getAsBoolean()) return;
                        if ("diagnostics-log".equals(type)) {
                            JSONObject event = payload.optJSONObject("event");
                            diagnostics.record("web", event.optString("name"),
                                event.optJSONObject("fields"));
                        } else if ("diagnostics-snapshot".equals(type)) {
                            snapshotDiagnostics(payload.optString("requestId"), current, replyProxy);
                        } else {
                            exportDiagnostics(current, payload.optString("requestId"), replyProxy);
                        }
                    } else if (HapticFeedbackController.isRequestType(type)) {
                        String activePageUrl = sourceView.getUrl();
                        if (activePageUrl == null || !sourcePageUrl.equals(activePageUrl)
                            || !ServiceOrigin.matches(origin, Uri.parse(activePageUrl))) {
                            return;
                        }
                        haptics.perform(sourceView, type);
                    } else if (NotificationSettings.isRequestType(type)) {
                        notifications.handle(
                            payload,
                            () -> generation == connectionGeneration.get() && sourceView == webView
                                && origin.equals(currentOrigin) && !isFinishing() && !isDestroyed(),
                            replyProxy::postMessage
                        );
                    } else if ("browser-open".equals(type)) {
                        openBrowser(payload, replyProxy);
                    } else if ("app-info".equals(type)) {
                        try {
                            replyProxy.postMessage(new JSONObject()
                                .put("type", "app-info")
                                .put("versionName", BuildConfig.VERSION_NAME)
                                .put("versionCode", BuildConfig.VERSION_CODE)
                                .put("updateSupported", !BuildConfig.DEBUG)
                                .put("requestRoutingVersion", 2)
                                .put("diagnosticsVersion", 1)
                                .put("browserVersion", 1)
                                .toString());
                        } catch (JSONException ignored) {
                            // These BuildConfig values are always valid JSON primitives.
                        }
                    } else if ("check-app-update".equals(type)) {
                        appUpdates.checkManually();
                    } else {
                        handleBridgeMessage(type, origin, generation, pageSnapshot);
                    }
                });
            }
        );
        return true;
    }

    private void handleBridgeMessage(
        String type,
        String origin,
        long generation,
        ResourceStore.Snapshot pageSnapshot
    ) {
        if (generation != connectionGeneration.get()) {
            return;
        }
        switch (type) {
            case "ready":
                pageReady = true;
                diagnostic("native.page", DiagnosticsPolicy.fields(
                    "phase", "ready", "generation", generation,
                    "resourceVersion", pageSnapshot.manifest.version));
                cancelReadyTimeout();
                ResourceStore.Cancellation cancellation = resourceCancellation;
                resources.execute(() -> {
                    if (!cancellation.isCancelled()) {
                        resourceStore.markReady(origin, pageSnapshot.version);
                    }
                });
                checkForResourceUpdate(origin, pageSnapshot, generation, true);
                openPendingNotification();
                break;
            case "authenticated": {
                String cookie = readDeviceCookie(origin);
                if (cookie == null) {
                    sessionStore.markAuthorized(origin);
                } else {
                    // Older installs did not persist the cookie outside WebView.
                    // Saving the name/value pair here lets the next cold start
                    // rehydrate it even when the WebView cookie database is late.
                    sessionStore.saveAuthorizedConnection(origin, cookie);
                }
                pageAuthenticated = true;
                diagnostic("native.page", DiagnosticsPolicy.fields(
                    "phase", "authenticated", "generation", generation));
                notifications.onAuthenticated();
                openPendingNotification();
                break;
            }
            case "auth-required":
            case "logged-out":
                clearAuthorization(origin);
                showLogin(type.equals("logged-out") ? "已退出当前设备会话" : "设备授权已失效");
                break;
            default:
                break;
        }
    }

    private WebResourceResponse interceptResource(
        WebResourceRequest request,
        String origin,
        ResourceStore.Snapshot pageSnapshot,
        long generation,
        WebView requestView
    ) {
        if (generation != connectionGeneration.get() || requestView != webView) {
            return blockedResponse();
        }
        Uri uri = request.getUrl();
        boolean sameOrigin = ServiceOrigin.matches(origin, uri);
        if (!sameOrigin && !ServiceOrigin.allowsChannelRequest(
            uri.toString(), request.getMethod(), request.getRequestHeaders()
        )) {
            return blockedResponse();
        }
        if (TunnelMediaClient.handles(uri.toString(), request.getMethod())) {
            return interceptTunnelMedia(request, origin, generation, requestView, sameOrigin);
        }
        if (!sameOrigin) return null;
        if (!"GET".equalsIgnoreCase(request.getMethod())) {
            return null;
        }
        final String path;
        try {
            path = ResourceManifest.canonicalPath(uri.getEncodedPath());
        } catch (IOException error) {
            return null;
        }
        ResourceManifest.Entry entry = pageSnapshot.manifest.byPath.get(path);
        if (entry == null) {
            return null;
        }
        try {
            InputStream input = pageSnapshot.open(entry, this);
            String[] contentType = entry.contentType.split(";", 2);
            String charset = entry.contentType.toLowerCase(Locale.ROOT).contains("charset=utf-8")
                ? "UTF-8"
                : null;
            WebResourceResponse response = new WebResourceResponse(
                contentType[0].trim(),
                charset,
                input
            );
            response.setResponseHeaders(localResourceHeaders());
            return response;
        } catch (IOException error) {
            if (isCurrentDiagnosticsPage(requestView, origin, generation)) {
                diagnostics.recordAtOrigin(origin, "native", "native.resources", DiagnosticsPolicy.urlFields(
                    uri.toString(), "reason", "local-read-failed",
                    "errorType", DiagnosticsPolicy.errorType(error),
                    "resourceVersion", pageSnapshot.manifest.version, "generation", generation),
                    () -> generation == connectionGeneration.get());
            }
            return errorResponse(500, "本地资源读取失败");
        }
    }

    private WebResourceResponse interceptTunnelMedia(
        WebResourceRequest request,
        String origin,
        long generation,
        WebView requestView,
        boolean sameOrigin
    ) {
        String url = request.getUrl().toString();
        long started = android.os.SystemClock.elapsedRealtime();
        BooleanSupplier stillCurrent = () -> generation == connectionGeneration.get()
            && requestView == webView && origin.equals(currentOrigin);
        Map<String, String> headers = new LinkedHashMap<>(request.getRequestHeaders());
        headers.keySet().removeIf(name -> "Cookie".equalsIgnoreCase(name)
            || "Origin".equalsIgnoreCase(name));
        headers.put("Origin", origin);
        if (sameOrigin) {
            String cookies = CookieManager.getInstance().getCookie(url);
            if (cookies != null && !cookies.isEmpty()) headers.put("Cookie", cookies);
        }
        try {
            TunnelMediaClient.Response response = tunnelMedia.open(
                url, request.getMethod(), headers, stillCurrent,
                (status, bytes, failure) -> diagnostics.recordAtOrigin(
                    origin, "native", "native.media", DiagnosticsPolicy.urlFields(url,
                        "method", request.getMethod(), "phase", failure == null ? "complete" : "failure",
                        "status", status, "bytes", bytes,
                        "durationMs", android.os.SystemClock.elapsedRealtime() - started,
                        "reason", failure == null ? "success" : !stillCurrent.getAsBoolean()
                            ? "cancelled" : "network-error",
                        "errorType", DiagnosticsPolicy.errorType(failure), "generation", generation),
                    stillCurrent
                )
            );
            diagnostics.recordAtOrigin(origin, "native", "native.media", DiagnosticsPolicy.urlFields(url,
                "method", request.getMethod(), "phase", "body", "status", response.status,
                "mime", DiagnosticsPolicy.mime(response.mime),
                "durationMs", android.os.SystemClock.elapsedRealtime() - started,
                "generation", generation), stillCurrent);
            return new WebResourceResponse(response.mime, response.charset, response.status,
                response.reason, response.headers, response.body);
        } catch (IOException | RuntimeException failure) {
            return errorResponse(502, "媒体读取失败", origin);
        }
    }

    private void checkForResourceUpdate(
        String origin,
        ResourceStore.Snapshot pageSnapshot,
        long generation,
        boolean force
    ) {
        long now = android.os.SystemClock.elapsedRealtime();
        if (!force && now - lastUpdateCheckAt < UPDATE_CHECK_INTERVAL_MS) {
            return;
        }
        long scheduledGeneration = updateCheckGeneration.getAndSet(generation);
        if (scheduledGeneration == generation) {
            return;
        }
        lastUpdateCheckAt = now;
        ResourceStore.Cancellation cancellation = resourceCancellation;
        if (resourceStatus != null) {
            resourceStatus.setVisibility(View.GONE);
        }
        resources.execute(() -> {
            try {
                if (generation == connectionGeneration.get() && !cancellation.isCancelled()) {
                    diagnostics.recordAtOrigin(origin, "native", "native.resources", DiagnosticsPolicy.fields(
                        "phase", "check", "resourceVersion", pageSnapshot.manifest.version,
                        "generation", generation),
                        () -> generation == connectionGeneration.get() && !cancellation.isCancelled());
                }
                ResourceStore.UpdateResult result = resourceStore.checkForUpdate(
                    origin,
                    pageSnapshot,
                    cancellation
                );
                runOnUiThread(() -> {
                    if (generation != connectionGeneration.get() || cancellation.isCancelled()
                        || isFinishing()) {
                        return;
                    }
                    diagnostic("native.resources", DiagnosticsPolicy.fields(
                        "phase", "complete", "reason", result.kind.name().toLowerCase(Locale.ROOT),
                        "resourceVersion", result.kind == ResourceStore.UpdateResult.Kind.DOWNLOADED
                            ? result.detail : pageSnapshot.manifest.version,
                        "generation", generation));
                    switch (result.kind) {
                        case DOWNLOADED:
                            Toast.makeText(
                                MainActivity.this,
                                "页面资源已更新，将在下次启动页面时启用",
                                Toast.LENGTH_SHORT
                            ).show();
                            break;
                        case FAILED:
                            showResourceUpdateStatus(
                                "资源更新失败，点击重试", true, origin, pageSnapshot, generation
                            );
                            break;
                        case UNSUPPORTED:
                            showResourceUpdateStatus(
                                "新版页面需要更新应用", false, origin, pageSnapshot, generation
                            );
                            break;
                        default:
                            break;
                    }
                });
            } finally {
                updateCheckGeneration.compareAndSet(generation, -1L);
            }
        });
    }

    private void showResourceUpdateStatus(
        String message,
        boolean retry,
        String origin,
        ResourceStore.Snapshot pageSnapshot,
        long generation
    ) {
        if (resourceStatus == null) {
            resourceStatus = text(message, 12, 0xFFB42318);
            resourceStatus.setPadding(dp(12), dp(8), dp(12), dp(8));
            resourceStatus.setBackground(rounded(0xFFFFF4ED, 10, 0xFFFED7AA));
            FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.TOP | Gravity.END
            );
            params.setMargins(dp(12), dp(12), dp(12), 0);
            root.addView(resourceStatus, params);
        }
        resourceStatus.setText(message);
        resourceStatus.setVisibility(View.VISIBLE);
        resourceStatus.setClickable(retry);
        resourceStatus.setOnClickListener(retry ? ignored -> {
            if (generation == connectionGeneration.get() && pageReady
                && sessionStore.isAuthorized()) {
                checkForResourceUpdate(origin, pageSnapshot, generation, true);
            }
        } : null);
    }

    private void cancelResourceWork() {
        if (resourceCancellation != null) {
            resourceCancellation.cancel();
            resourceCancellation = null;
        }
    }

    @Override
    protected void onStart() {
        super.onStart();
        NotificationService.setAppVisible(notificationVisibility, true);
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields(
            "phase", "start", "visible", true, "network", diagnosticsNetwork()));
    }

    @Override
    protected void onResume() {
        super.onResume();
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields(
            "phase", "resume", "network", diagnosticsNetwork()));
        haptics.setForeground(true);
        appUpdates.onResume();
        notifications.onResume();
        openPendingNotification();
        if ((pageReady || webView != null)
            && currentOrigin != null
            && sessionStore.isAuthorized()) {
            refreshZrokCookie(currentOrigin, connectionGeneration.get());
        }
        if (pageReady && currentOrigin != null && snapshot != null && sessionStore.isAuthorized()) {
            checkForResourceUpdate(
                currentOrigin,
                snapshot,
                connectionGeneration.get(),
                false
            );
        }
    }

    @Override
    protected void onPause() {
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields("phase", "pause"));
        haptics.setForeground(false);
        appUpdates.onPause();
        notifications.onPause();
        super.onPause();
    }

    @Override
    protected void onStop() {
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields("phase", "stop", "visible", false));
        NotificationService.setAppVisible(notificationVisibility, false);
        super.onStop();
    }

    private void handleRendererExit(WebView failedView, String origin, long generation) {
        if (failedView != webView || generation != connectionGeneration.get()) {
            root.removeView(failedView);
            failedView.destroy();
            return;
        }
        notifications.onPageChanged();
        cancelResourceWork();
        connectionGeneration.incrementAndGet();
        pageReady = false;
        pageAuthenticated = false;
        notificationNavigationAttempt++;
        navigatingNotification = null;
        backAttempt++;
        backPending = false;
        cancelReadyTimeout();
        webView = null;
        cancelFileChooser();
        root.removeView(failedView);
        failedView.destroy();
        resourceStatus = null;
        if (!isFinishing() && !isDestroyed()) {
            showSessionLaunchError(origin, "页面进程已退出，请点击重试恢复会话");
        } else {
            snapshot = null;
        }
    }

    private void handlePageStartupFailure(
        String reason,
        String origin,
        String version,
        long generation
    ) {
        if (generation != connectionGeneration.get()) {
            return;
        }
        cancelReadyTimeout();
        ResourceStore.Cancellation cancellation = resourceCancellation;
        resources.execute(() -> {
            if (cancellation.isCancelled()) {
                return;
            }
            boolean rolledBack = resourceStore.rollbackTrial(origin, version);
            runOnUiThread(() -> {
                if (generation != connectionGeneration.get() || cancellation.isCancelled()
                    || isFinishing()) {
                    return;
                }
                if (rolledBack) {
                    diagnostic("native.resources", DiagnosticsPolicy.fields(
                        "phase", "rollback", "reason", "success", "generation", generation));
                    Toast.makeText(this, "新版页面启动失败，已回退", Toast.LENGTH_LONG).show();
                    openSession(origin);
                } else {
                    diagnostic("native.page", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "页面启动超时".equals(reason)
                            ? "startup-timeout" : "load-failed", "generation", generation));
                    Toast.makeText(this, reason + "，请检查网络后重试", Toast.LENGTH_LONG).show();
                }
            });
        });
    }

    private void clearAuthorization(String origin) {
        invalidateBrowser();
        notifications.onPageChanged();
        NotificationService.stop(this, "auth-required", true);
        pendingNotificationOpen = null;
        pageAuthenticated = false;
        cancelResourceWork();
        connectionGeneration.incrementAndGet();
        sessionStore.setAuthorized(false);
        diagnostics.clear();
        diagnostic("native.authorization", DiagnosticsPolicy.fields("reason", "auth-required"));
        CookieManager cookies = CookieManager.getInstance();
        cookies.setCookie(
            origin,
            DEVICE_COOKIE + "; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"
                + (origin.startsWith("https://") ? "; Secure" : "")
        );
        cookies.flush();
    }

    private void enqueueDownload(
        String url,
        String userAgent,
        String contentDisposition,
        String mimeType
    ) {
        Uri uri = Uri.parse(url);
        if (currentOrigin == null || !ServiceOrigin.matches(currentOrigin, uri)) {
            Toast.makeText(this, "已阻止跨源下载", Toast.LENGTH_SHORT).show();
            return;
        }
        String origin = currentOrigin;
        String fileName = URLUtil.guessFileName(url, contentDisposition, mimeType)
            .replaceAll("[\\\\/:*?\"<>|]", "_");
        Toast.makeText(this, "已开始下载", Toast.LENGTH_SHORT).show();
        background.execute(() -> downloadAttachment(origin, url, userAgent, fileName));
    }

    private void downloadAttachment(
        String origin,
        String url,
        String userAgent,
        String fileName
    ) {
        HttpURLConnection connection = null;
        File partial = null;
        try {
            URL target = new URL(url);
            if (!ServiceOrigin.matches(origin, Uri.parse(target.toString()))) {
                throw new IOException("下载地址跨越服务 Origin");
            }
            connection = (HttpURLConnection) target.openConnection();
            connection.setInstanceFollowRedirects(false);
            connection.setConnectTimeout(15_000);
            connection.setReadTimeout(30_000);
            connection.setRequestProperty(ZROK_HEADER, "1");
            if (userAgent != null) {
                connection.setRequestProperty("User-Agent", userAgent);
            }
            String cookie = CookieManager.getInstance().getCookie(origin);
            if (cookie != null && !cookie.trim().isEmpty()) {
                connection.setRequestProperty("Cookie", cookie);
            }
            int status = connection.getResponseCode();
            if (status >= 300 && status < 400) {
                throw new IOException("已拒绝下载重定向");
            }
            if (status != HttpURLConnection.HTTP_OK) {
                throw new IOException("下载失败：" + status);
            }
            long declaredLength = connection.getContentLengthLong();
            if (declaredLength > MAX_ATTACHMENT_BYTES) {
                throw new IOException("附件大小超出限制");
            }
            File downloads = getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS);
            if (downloads == null || (!downloads.exists() && !downloads.mkdirs())) {
                throw new IOException("无法创建下载目录");
            }
            partial = new File(downloads, "." + fileName + ".part");
            File completed = uniqueDownloadFile(downloads, fileName);
            long total = 0;
            try (
                BufferedInputStream input =
                    new BufferedInputStream(connection.getInputStream());
                BufferedOutputStream output =
                    new BufferedOutputStream(new FileOutputStream(partial))
            ) {
                byte[] buffer = new byte[32 * 1024];
                int read;
                while ((read = input.read(buffer)) != -1) {
                    total += read;
                    if (total > MAX_ATTACHMENT_BYTES) {
                        throw new IOException("附件大小超出限制");
                    }
                    output.write(buffer, 0, read);
                }
            }
            if (declaredLength >= 0 && total != declaredLength) {
                throw new IOException("附件下载不完整");
            }
            if (!partial.renameTo(completed)) {
                throw new IOException("无法保存附件");
            }
            partial = null;
            runOnUiThread(() -> Toast.makeText(
                this,
                "下载完成：" + completed.getName(),
                Toast.LENGTH_LONG
            ).show());
        } catch (Exception error) {
            runOnUiThread(() ->
                Toast.makeText(this, "下载失败，请稍后重试", Toast.LENGTH_LONG).show()
            );
        } finally {
            if (connection != null) {
                connection.disconnect();
            }
            if (partial != null) {
                partial.delete();
            }
        }
    }

    private static File uniqueDownloadFile(File directory, String fileName) {
        File candidate = new File(directory, fileName);
        if (!candidate.exists()) {
            return candidate;
        }
        int dot = fileName.lastIndexOf('.');
        String base = dot > 0 ? fileName.substring(0, dot) : fileName;
        String extension = dot > 0 ? fileName.substring(dot) : "";
        for (int index = 1; index < 10_000; index++) {
            candidate = new File(directory, base + " (" + index + ")" + extension);
            if (!candidate.exists()) {
                return candidate;
            }
        }
        return new File(directory, base + "-" + System.currentTimeMillis() + extension);
    }

    private void openExternal(Uri uri) {
        String scheme = uri.getScheme();
        if (scheme == null
            || !(scheme.equalsIgnoreCase("https")
            || scheme.equalsIgnoreCase("mailto")
            || scheme.equalsIgnoreCase("tel"))) {
            Toast.makeText(this, "已阻止不支持的外部链接", Toast.LENGTH_SHORT).show();
            return;
        }
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, uri));
        } catch (ActivityNotFoundException error) {
            Toast.makeText(this, "没有可打开此链接的应用", Toast.LENGTH_SHORT).show();
        }
    }

    private boolean isLocalOrLanAddress(Uri uri) {
        String scheme = uri.getScheme();
        String host = uri.getHost();
        if (host == null || scheme == null
            || !(scheme.equalsIgnoreCase("http") || scheme.equalsIgnoreCase("https"))) {
            return false;
        }
        try {
            return BrowserRoutePolicy.usesComputer(host);
        } catch (IllegalArgumentException invalid) {
            return false;
        }
    }

    private void openBrowserLink(Uri uri) {
        String message = null;
        try {
            if (!pageAuthenticated || currentOrigin == null)
                throw new IllegalStateException("请先连接远程会话");
            if (Build.VERSION.SDK_INT < 28)
                throw new IllegalStateException("浏览器需要 Android 9 或更新版本");
            String cookie = null;
            String allCookies = CookieManager.getInstance().getCookie(currentOrigin);
            if (allCookies != null) {
                for (String item : allCookies.split(";")) {
                    String candidate = item.trim();
                    if (candidate.matches("codex_elves_device=[A-Za-z0-9._~-]{16,512}")) cookie = candidate;
                }
            }
            if (cookie == null) throw new IllegalStateException("设备授权已失效，请重新连接");
            if (browserAuthority == null || !browserAuthority.matches(currentOrigin, cookie)) {
                invalidateBrowser();
                browserAuthority = new BrowserAuthority(currentOrigin, cookie,
                    (name, fields) -> diagnostics.recordAtOrigin(
                        currentOrigin, "native", name, fields));
            }
            Bundle parameters = new Bundle();
            parameters.putBinder(BrowserAuthority.EXTRA, browserAuthority);
            parameters.putFloat("fontSize", 14f);
            parameters.putInt("background", getColor(R.color.login_background));
            parameters.putInt("foreground", getColor(R.color.login_text));
            parameters.putInt("muted", getColor(R.color.login_muted));
            parameters.putInt("line", getColor(R.color.login_line));
            parameters.putInt("surface", getColor(R.color.login_background));
            parameters.putString("initialUrl", uri.toString());
            Intent browser = new Intent(this, BrowserActivity.class).putExtras(parameters);
            startActivity(browser);
            overridePendingTransition(R.anim.browser_enter, R.anim.browser_hold);
        } catch (IllegalStateException unavailable) {
            message = unavailable.getMessage();
        } catch (Exception unavailable) {
            message = "无法打开浏览器，请返回会话后重试";
        }
        if (message != null)
            Toast.makeText(this, message, Toast.LENGTH_SHORT).show();
    }

    private void cancelReadyTimeout() {
        if (readyTimeout != null) {
            mainHandler.removeCallbacks(readyTimeout);
            readyTimeout = null;
        }
    }

    private void destroyWebView() {
        cancelFileChooser();
        WebView previous = webView;
        webView = null;
        tunnelMedia.cancelAll();
        if (previous == null) {
            return;
        }
        previous.stopLoading();
        previous.setWebChromeClient(null);
        previous.setWebViewClient(null);
        root.removeView(previous);
        previous.destroy();
    }

    @Override
    public void onBackPressed() {
        WebView view = webView;
        if (view == null || !pageReady || currentOrigin == null) {
            performDefaultBack();
            return;
        }
        if (backPending) {
            return;
        }
        long generation = connectionGeneration.get();
        long attempt = ++backAttempt;
        backPending = true;
        try {
            view.evaluateJavascript(
                "(()=>{const e=new Event('codex-elves-native-back',{cancelable:true});"
                    + "window.dispatchEvent(e);return e.defaultPrevented;})()",
                result -> {
                    if (attempt != backAttempt || generation != connectionGeneration.get()
                        || view != webView || isFinishing() || isDestroyed()) {
                        return;
                    }
                    backPending = false;
                    if (!"true".equals(result)) {
                        performDefaultBack();
                    }
                }
            );
        } catch (RuntimeException error) {
            backPending = false;
            performDefaultBack();
        }
    }

    private void performDefaultBack() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleNotificationIntent(intent);
    }

    private void handleNotificationIntent(Intent intent) {
        NotificationPublisher.OpenTarget target = NotificationPublisher.openTarget(intent);
        NotificationIdentity identity = NotificationService.currentIdentity(this);
        if (target == null || identity == null || !target.identity.equals(identity.digest)
            || !NotificationSettings.read(this).enabled) {
            return;
        }
        NotificationPublisher.recordUserOpen(this, target);
        pendingNotificationOpen = target;
        openPendingNotification();
    }

    private void openPendingNotification() {
        NotificationPublisher.OpenTarget target = pendingNotificationOpen;
        WebView view = webView;
        if (target == null || view == null || !pageReady || !pageAuthenticated
            || currentOrigin == null || isFinishing() || isDestroyed()) {
            return;
        }
        NotificationIdentity identity = NotificationService.currentIdentity(this);
        if (identity == null || !identity.origin.equals(currentOrigin)
            || !target.identity.equals(identity.digest)) {
            pendingNotificationOpen = null;
            return;
        }
        long generation = connectionGeneration.get();
        if (navigatingNotification == target) {
            return;
        }
        navigatingNotification = target;
        long attempt = ++notificationNavigationAttempt;
        String script = "(()=>{const e=new CustomEvent('codex-elves-notification-open',"
            + "{detail:{threadId:" + JSONObject.quote(target.threadId) + "},cancelable:true});"
            + "window.dispatchEvent(e);return e.defaultPrevented;})()";
        try {
            view.evaluateJavascript(script, result -> {
                if (attempt == notificationNavigationAttempt
                    && generation == connectionGeneration.get() && view == webView) {
                    navigatingNotification = null;
                    NotificationIdentity active = NotificationService.currentIdentity(this);
                    if (pendingNotificationOpen == target && "true".equals(result)
                        && active != null && target.identity.equals(active.digest)
                        && !isFinishing() && !isDestroyed()) {
                        pendingNotificationOpen = null;
                    }
                }
            });
        } catch (RuntimeException ignored) {
            navigatingNotification = null;
            // Keep the target until a current, authenticated document can handle it.
        }
    }

    private void handleNotificationAuthorizationLost(String identityDigest) {
        if (currentOrigin == null || isFinishing() || isDestroyed()) {
            return;
        }
        NotificationIdentity identity = NotificationService.identityAtOrigin(this, currentOrigin);
        if (identity == null || !identity.digest.equals(identityDigest)) {
            return;
        }
        clearAuthorization(currentOrigin);
        showLogin("设备授权已失效");
    }

    @Override
    public void onRequestPermissionsResult(
        int requestCode, String[] permissions, int[] grantResults
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        notifications.onRequestPermissionsResult(requestCode);
    }

    @Override
    protected void onDestroy() {
        invalidateBrowser();
        diagnostic("native.lifecycle", DiagnosticsPolicy.fields("phase", "destroy"));
        NotificationService.setAppVisible(notificationVisibility, false);
        haptics.setForeground(false);
        appUpdates.onDestroy();
        notifications.onDestroy();
        if (Build.VERSION.SDK_INT >= 33 && nativeBackCallback != null) {
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(nativeBackCallback);
        }
        cancelResourceWork();
        connectionGeneration.incrementAndGet();
        cancelReadyTimeout();
        destroyWebView();
        background.shutdownNow();
        resources.shutdownNow();
        diagnostics.close();
        super.onDestroy();
    }

    private void invalidateBrowser() {
        if (browserAuthority != null) { browserAuthority.invalidate(); browserAuthority = null; }
    }

    private static boolean validBrowserOpen(JSONObject payload) {
        if (payload.length() != 4 || !payload.optString("requestId").matches("[A-Za-z0-9_-]{1,80}")
            || !(payload.opt("fontSize") instanceof Number)) return false;
        double size = payload.optDouble("fontSize", 0);
        if (!Double.isFinite(size) || size < 12 || size > 24) return false;
        JSONObject colors = payload.optJSONObject("colors");
        if (colors == null || colors.length() != 5) return false;
        for (String name : new String[]{"background", "foreground", "muted", "line", "surface"})
            if (!colors.optString(name).matches("#[0-9a-fA-F]{6}")) return false;
        return true;
    }

    @SuppressWarnings("deprecation")
    @SuppressLint("RequiresFeature") // Called only by registerBridge's WEB_MESSAGE_LISTENER callback.
    private void openBrowser(JSONObject payload, JavaScriptReplyProxy reply) {
        String message = null;
        try {
            if (!pageAuthenticated || currentOrigin == null) throw new IllegalStateException("请先连接远程会话");
            if (Build.VERSION.SDK_INT < 28) throw new IllegalStateException("浏览器需要 Android 9 或更新版本");
            String cookie = null;
            String allCookies = CookieManager.getInstance().getCookie(currentOrigin);
            if (allCookies != null) {
                for (String item : allCookies.split(";")) {
                    String candidate = item.trim();
                    if (candidate.matches("codex_elves_device=[A-Za-z0-9._~-]{16,512}")) cookie = candidate;
                }
            }
            if (cookie == null) throw new IllegalStateException("设备授权已失效，请重新连接");
            if (browserAuthority == null || !browserAuthority.matches(currentOrigin, cookie)) {
                invalidateBrowser();
                String authorityOrigin = currentOrigin;
                browserAuthority = new BrowserAuthority(authorityOrigin, cookie,
                    (name, fields) -> diagnostics.recordAtOrigin(
                        authorityOrigin, "native", name, fields));
            }
            Bundle parameters = new Bundle();
            parameters.putBinder(BrowserAuthority.EXTRA, browserAuthority);
            parameters.putFloat("fontSize", (float) payload.getDouble("fontSize"));
            JSONObject colors = payload.getJSONObject("colors");
            for (String name : new String[]{"background", "foreground", "muted", "line", "surface"})
                parameters.putInt(name, android.graphics.Color.parseColor(colors.getString(name)));
            Intent browser = new Intent(this, BrowserActivity.class).putExtras(parameters);
            startActivity(browser);
            overridePendingTransition(R.anim.browser_enter, R.anim.browser_hold);
        } catch (IllegalStateException unavailable) {
            message = unavailable.getMessage();
        } catch (Exception unavailable) {
            message = "无法打开浏览器，请返回会话后重试";
        }
        try {
            JSONObject result = new JSONObject().put("type", "browser-open-result")
                .put("requestId", payload.optString("requestId")).put("ok", message == null);
            if (message != null) result.put("message", message);
            reply.postMessage(result.toString());
        } catch (JSONException ignored) { /* All fields above are native scalar values. */ }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (appUpdates.onActivityResult(requestCode)) {
            return;
        }
        if (requestCode != fileChooserRequestCode || fileChooserCallback == null) {
            return;
        }
        completeFileChooser(parseFileChooserResult(resultCode, data));
    }

    private void completeFileChooser(Uri[] result) {
        ValueCallback<Uri[]> callback = fileChooserCallback;
        fileChooserCallback = null;
        fileChooserRequestCode = -1;
        if (callback != null) {
            callback.onReceiveValue(result);
        }
    }

    private void cancelFileChooser() {
        int requestCode = fileChooserRequestCode;
        completeFileChooser(null);
        if (requestCode >= 0) {
            finishActivity(requestCode);
        }
    }

    private boolean launchFileChooser(Intent intent, int requestCode) {
        try {
            startActivityForResult(intent, requestCode);
            return true;
        } catch (ActivityNotFoundException | SecurityException error) {
            return false;
        }
    }

    private static Intent documentPickerIntent(WebChromeClient.FileChooserParams params) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        String[] acceptTypes = params.getAcceptTypes();
        String type = acceptTypes != null && acceptTypes.length == 1
            && acceptTypes[0] != null && !acceptTypes[0].trim().isEmpty()
            ? acceptTypes[0] : "*/*";
        intent.setType(type);
        if (acceptTypes != null && acceptTypes.length > 1) {
            intent.putExtra(Intent.EXTRA_MIME_TYPES, acceptTypes);
        }
        intent.putExtra(
            Intent.EXTRA_ALLOW_MULTIPLE,
            params.getMode() == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE
        );
        return intent;
    }

    private static Uri[] parseFileChooserResult(int resultCode, Intent data) {
        if (resultCode != RESULT_OK || data == null) {
            return null;
        }
        ClipData selected = data.getClipData();
        if (selected != null && selected.getItemCount() > 0) {
            Uri[] result = new Uri[selected.getItemCount()];
            for (int index = 0; index < selected.getItemCount(); index++) {
                result[index] = selected.getItemAt(index).getUri();
            }
            return result;
        }
        Uri selectedImage = data.getData();
        return selectedImage == null ? null : new Uri[] { selectedImage };
    }

    private static Intent orderedImagePickerIntent(WebChromeClient.FileChooserParams params) {
        String[] acceptTypes = params.getAcceptTypes();
        if (params.getMode() != WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE
            || params.isCaptureEnabled()
            || acceptTypes == null
            || acceptTypes.length != 1
            || !"image/*".equals(acceptTypes[0])) {
            return null;
        }
        boolean supported = Build.VERSION.SDK_INT >= 35
            || Build.VERSION.SDK_INT >= 30
                && SdkExtensions.getExtensionVersion(Build.VERSION_CODES.R) >= 12;
        if (!supported) {
            return null;
        }
        int maximumImages = Math.min(4, MediaStore.getPickImagesMaxLimit());
        if (maximumImages < 2) {
            return null;
        }
        Intent intent = new Intent(MediaStore.ACTION_PICK_IMAGES);
        intent.setType("image/*");
        intent.putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, maximumImages);
        intent.putExtra(MediaStore.EXTRA_PICK_IMAGES_IN_ORDER, true);
        return intent;
    }

    private final class ShellChromeClient extends WebChromeClient {
        private final WebView owner;
        private final String origin;
        private final long generation;

        ShellChromeClient(WebView owner, String origin, long generation) {
            this.owner = owner;
            this.origin = origin;
            this.generation = generation;
        }

        @Override
        public boolean onConsoleMessage(ConsoleMessage message) {
            if (message.messageLevel() == ConsoleMessage.MessageLevel.ERROR
                && isCurrentDiagnosticsPage(owner, origin, generation)) {
                diagnostic("native.console.error", DiagnosticsPolicy.urlFields(
                    message.sourceId(), "line", Math.max(0, Math.min(10_000_000, message.lineNumber())),
                    "reason", DiagnosticsPolicy.consoleReason(message.message()),
                    "generation", generation));
            }
            // Retain only the fixed browser-error category, source path, and line number.
            return true;
        }

        @Override
        public boolean onShowFileChooser(
            WebView webView,
            ValueCallback<Uri[]> callback,
            FileChooserParams params
        ) {
            cancelFileChooser();
            fileChooserCallback = callback;
            int requestCode = nextFileChooserRequestCode;
            nextFileChooserRequestCode = requestCode >= 0xFFFF
                ? FILE_CHOOSER_REQUEST : requestCode + 1;
            fileChooserRequestCode = requestCode;
            Intent orderedPicker = orderedImagePickerIntent(params);
            if (orderedPicker != null) {
                if (launchFileChooser(orderedPicker, requestCode)) {
                    return true;
                }
            }
            Intent intent = null;
            try {
                intent = params.createIntent();
            } catch (Exception error) {
                // Use the document provider when WebView cannot create its picker intent.
            }
            if (intent != null) {
                intent.putExtra(
                    Intent.EXTRA_ALLOW_MULTIPLE,
                    params.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE
                );
                if (launchFileChooser(intent, requestCode)) {
                    return true;
                }
            }
            if (launchFileChooser(documentPickerIntent(params), requestCode)) {
                return true;
            }
            completeFileChooser(null);
            Toast.makeText(MainActivity.this, "没有可用的文件选择器", Toast.LENGTH_SHORT).show();
            return true;
        }
    }

    private void diagnostic(String name, JSONObject fields) {
        if (diagnostics != null) diagnostics.record("native", name, fields);
    }

    private boolean isCurrentDiagnosticsPage(WebView view, String origin, long generation) {
        return generation == connectionGeneration.get() && view == webView
            && origin.equals(currentOrigin) && !isFinishing() && !isDestroyed();
    }

    private static boolean isDiagnosticResource(WebResourceRequest request) {
        String path = request.getUrl().getEncodedPath();
        return request.isForMainFrame() || path != null && path.startsWith("/api/");
    }

    private JSONObject diagnosticsApp() {
        String originHost = Uri.parse(diagnosticsOrigin).getHost();
        try {
            return new JSONObject()
                .put("versionName", BuildConfig.VERSION_NAME)
                .put("versionCode", BuildConfig.VERSION_CODE)
                .put("sdk", Build.VERSION.SDK_INT)
                .put("webViewVersion", webViewVersion)
                .put("resourceVersion", snapshot == null ? "unknown" : snapshot.manifest.version)
                .put("originHost", originHost == null ? "" : originHost)
                .put("generation", connectionGeneration.get());
        } catch (JSONException ignored) {
            return new JSONObject();
        }
    }

    private void snapshotDiagnostics(
        String requestId,
        BooleanSupplier current,
        JavaScriptReplyProxy reply
    ) {
        diagnostics.snapshot(diagnosticsApp(), (bundle, errorCode) -> {
            final String response;
            try {
                JSONObject result = new JSONObject().put("type", "diagnostics-snapshot")
                    .put("requestId", requestId);
                if (bundle != null) {
                    result.put("bundle", bundle);
                } else {
                    result.put("error", errorCode);
                }
                response = result.toString();
            } catch (JSONException ignored) {
                return;
            }
            runOnUiThread(() -> {
                if (current.getAsBoolean()
                    && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
                    reply.postMessage(response);
                }
            });
        });
    }

    private void exportDiagnostics(
        BooleanSupplier current,
        String requestId,
        JavaScriptReplyProxy reply
    ) {
        long generation = connectionGeneration.get();
        diagnostic("native.diagnostics.export", DiagnosticsPolicy.fields("reason", "manual"));
        diagnostics.export(diagnosticsApp(), (file, errorCode) -> {
            Uri sharedUri = null;
            String failure = errorCode;
            if (file != null) {
                try {
                    sharedUri = FileProvider.getUriForFile(
                        this, getPackageName() + ".diagnostics", file);
                } catch (IllegalArgumentException | SecurityException ignored) {
                    failure = "export_failed";
                }
            }
            Uri uri = sharedUri;
            String exportError = failure;
            runOnUiThread(() -> {
                if (generation != connectionGeneration.get() || !current.getAsBoolean()) return;
                boolean ok = false;
                String error = exportError;
                if (uri != null) {
                    Intent share = new Intent(Intent.ACTION_SEND).setType("application/json");
                    share.putExtra(Intent.EXTRA_STREAM, uri);
                    share.setClipData(ClipData.newRawUri(getString(R.string.diagnostics_share_title), uri));
                    share.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                    Intent chooser = Intent.createChooser(
                        share, getString(R.string.diagnostics_share_title));
                    chooser.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                    try {
                        startActivity(chooser);
                        ok = true;
                    } catch (ActivityNotFoundException | SecurityException ignored) {
                        error = "share_unavailable";
                    }
                }
                if (reply != null
                    && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
                    try {
                        JSONObject response = new JSONObject()
                            .put("type", "diagnostics-export").put("requestId", requestId)
                            .put("ok", ok);
                        if (!ok) response.put("error", error == null ? "export_failed" : error);
                        reply.postMessage(response.toString());
                    } catch (JSONException ignored) {
                        // Reply fields are fixed codes and a previously checked request ID.
                    }
                } else if (!ok) {
                    Toast.makeText(this, R.string.diagnostics_export_failed, Toast.LENGTH_LONG).show();
                }
            });
        });
    }

    private String diagnosticsNetwork() {
        try {
            ConnectivityManager manager = getSystemService(ConnectivityManager.class);
            if (manager == null) return "unknown";
            android.net.Network network = manager.getActiveNetwork();
            if (network == null) return "none";
            NetworkCapabilities capabilities = manager.getNetworkCapabilities(network);
            if (capabilities == null) return "unknown";
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) return "vpn";
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) return "wifi";
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) return "cellular";
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) return "ethernet";
            return "other";
        } catch (SecurityException ignored) {
            return "unknown";
        }
    }

    private static String readWebViewVersion() {
        try {
            PackageInfo info = WebView.getCurrentWebViewPackage();
            String version = info == null ? "" : info.versionName;
            return version != null && version.matches("[0-9]+(?:\\.[0-9]+){1,4}")
                ? version : "unknown";
        } catch (RuntimeException ignored) {
            return "unknown";
        }
    }

    private static String normalizedDiagnosticsOrigin(String origin) {
        try {
            return ServiceOrigin.normalize(origin);
        } catch (IllegalArgumentException ignored) {
            return "";
        }
    }

    private static WebResourceResponse blockedResponse() {
        return errorResponse(403, "Forbidden");
    }

    private static WebResourceResponse errorResponse(int status, String body) {
        return errorResponse(status, body, null);
    }

    private static WebResourceResponse errorResponse(int status, String body, String allowedOrigin) {
        Map<String, String> headers = localResourceHeaders();
        // 仅已通过媒体来源校验的本地错误携带当前页面 Origin，避免掩盖为 CORS 故障。
        if (allowedOrigin != null) {
            headers.put("Access-Control-Allow-Origin", allowedOrigin);
            headers.put("Vary", "Origin");
        }
        return new WebResourceResponse(
            "text/plain",
            "UTF-8",
            status,
            status == 403 ? "Forbidden" : status == 502 ? "Bad Gateway" : "Internal Server Error",
            headers,
            new ByteArrayInputStream(body.getBytes(StandardCharsets.UTF_8))
        );
    }

    private static Map<String, String> localResourceHeaders() {
        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("Cache-Control", "no-cache");
        headers.put("X-Content-Type-Options", "nosniff");
        headers.put("Referrer-Policy", "no-referrer");
        headers.put("X-Frame-Options", "DENY");
        headers.put("Strict-Transport-Security", "max-age=31536000");
        headers.put(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; worker-src 'self'; style-src 'self'; "
                + "img-src 'self' data: " + ServiceOrigin.CHANNEL_MEDIA_SOURCES + "; "
                + "media-src 'self' " + ServiceOrigin.CHANNEL_MEDIA_SOURCES + "; "
                + "connect-src 'self' " + ServiceOrigin.CHANNEL_CONNECT_SOURCES + "; object-src 'none'; "
                + "base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
        );
        return headers;
    }

    private void showStatus(TextView status, String message) {
        status.setText(message == null ? "操作失败" : message);
        status.setVisibility(View.VISIBLE);
    }

    private TextView text(String value, int size, int color) {
        TextView text = new TextView(this);
        text.setText(value);
        text.setTextSize(size);
        text.setTextColor(color);
        return text;
    }

    private android.graphics.drawable.GradientDrawable rounded(
        int fill,
        int radiusDp,
        int stroke
    ) {
        android.graphics.drawable.GradientDrawable drawable =
            new android.graphics.drawable.GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(radiusDp));
        drawable.setStroke(dp(1), stroke);
        return drawable;
    }

    private static ViewGroup.LayoutParams matchParent() {
        return new ViewGroup.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT
        );
    }

    private static LinearLayout.LayoutParams fullWidthWrap() {
        return new LinearLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT
        );
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private static final class LoginResult {
        final boolean success;
        final String cookie;
        final String message;

        private LoginResult(boolean success, String cookie, String message) {
            this.success = success;
            this.cookie = cookie;
            this.message = message;
        }

        static LoginResult success(String cookie) {
            return new LoginResult(true, cookie, "");
        }

        static LoginResult failure(String message) {
            return new LoginResult(false, "", message);
        }
    }
}
