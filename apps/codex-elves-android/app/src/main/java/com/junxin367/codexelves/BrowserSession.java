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
import android.view.ViewGroup;
import androidx.webkit.ProxyConfig;
import androidx.webkit.ProxyController;
import androidx.webkit.WebStorageCompat;
import androidx.webkit.WebViewFeature;
import java.util.List;
import java.util.function.Consumer;

/** Main-thread-owned browser state; never retains a detached Activity. */
final class BrowserSession {
    interface Listener {
        void changed();
        void invalidated(String message);
        void requestAuthentication(String host, HttpAuthHandler handler);
        void chooseFiles(ValueCallback<Uri[]> callback, WebChromeClient.FileChooserParams parameters);
    }
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
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
    private List<String> domains;
    boolean home = true;
    String url = "", error = "";
    int progress = 100;

    static void prepare() {
        if (Build.VERSION.SDK_INT < 28) throw new IllegalStateException("浏览器需要 Android 9 或更新版本");
        if (!directoryConfigured) {
            WebView.setDataDirectorySuffix("codex-elves-browser");
            directoryConfigured = true;
        }
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)
            || !WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE_REVERSE_BYPASS)
            || !WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA))
            throw new IllegalStateException("请更新 Android System WebView 后使用浏览器");
    }

    static void acquire(Activity activity, IBinder authority,
        Consumer<BrowserSession> ready, Consumer<String> failure) {
        long generation = ++openGeneration;
        try {
            prepare();
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
                try {
                    BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                    if (!identity.owner().equals(fresh.owner())) throw new IllegalStateException("浏览器授权已失效");
                    current = new BrowserSession(activity, authority, fresh);
                    activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                        .edit().putString("owner", fresh.owner()).apply();
                    ready.accept(current);
                } catch (Exception unavailable) { failure.accept("无法打开浏览器，请返回会话后重试"); }
            };
            String previousOwner = activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                .getString("owner", "");
            if (clearing || !identity.owner().equals(previousOwner)) {
                afterClear = create;
                clearProfile();
            } else create.run();
        } catch (Exception unavailable) {
            failure.accept(unavailable.getMessage() == null ? "浏览器当前不可用" : unavailable.getMessage());
        }
    }

    @SuppressLint("RequiresFeature") // prepare() checks DELETE_BROWSING_DATA before any session exists.
    private static void clearProfile() {
        if (clearing) return;
        clearing = true;
        WebStorageCompat.deleteBrowsingData(WebStorage.getInstance(), () -> {
            clearing = false;
            Runnable next = afterClear; afterClear = null;
            if (next != null) next.run();
        });
    }

    @SuppressLint("SetJavaScriptEnabled")
    private BrowserSession(Activity activity, IBinder authority, BrowserAuthority.Snapshot identity) throws Exception {
        this.authority = authority; this.identity = identity;
        context = new MutableContextWrapper(activity);
        preferences = new BrowserPreferences(activity);
        domains = preferences.load(identity.origin());
        proxy = new BrowserProxy(() -> {
            try {
                BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                if (!identity.owner().equals(fresh.owner())) return null;
                return new BrowserProxy.Credentials(fresh.origin(), fresh.cookie());
            } catch (Exception expired) { MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")); return null; }
        }, domains, () -> MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")));
        proxy.setActive(false);
        view = new WebView(context);
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
        CookieManager.getInstance().setAcceptCookie(true);
        CookieManager.getInstance().setAcceptThirdPartyCookies(view, false);
        view.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView webView, int value) { progress = value; changed(); }
            @Override public void onReceivedTitle(WebView webView, String title) { changed(); }
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
                if (network && !allowed(resource)) return new WebResourceResponse("text/plain", "UTF-8",
                    403, "Forbidden", java.util.Map.of(),
                    new java.io.ByteArrayInputStream(new byte[0]));
                return null;
            }
            @Override public void onPageStarted(WebView webView, String address, Bitmap favicon) {
                ++pageGeneration;
                url = address; error = ""; changed();
            }
            @Override public void onPageFinished(WebView webView, String address) {
                url = address; changed();
            }
            @Override public void onReceivedError(WebView webView, WebResourceRequest request, WebResourceError failure) {
                if (request.isForMainFrame()) { error = "网页加载失败，请检查地址或网络后重试"; changed(); }
            }
            @Override public void onReceivedHttpError(WebView webView, WebResourceRequest request, WebResourceResponse response) {
                if (request.isForMainFrame() && response.getStatusCode() >= 500) {
                    error = "网页请求失败（" + response.getStatusCode() + "），请稍后重试"; changed();
                }
            }
            @Override public void onReceivedSslError(WebView webView, SslErrorHandler handler,
                android.net.http.SslError failure) {
                handler.cancel();
                // Only onReceivedError's isForMainFrame may replace the page with an error.
            }
            @Override public void onReceivedHttpAuthRequest(WebView webView, HttpAuthHandler handler, String host, String realm) {
                if (attached && !closed && proxy.matchesChallenge(host, realm))
                    handler.proceed(BrowserProxy.USER, proxy.password());
                else if (attached && listener != null && !BrowserRoutePolicy.PROXY_HOST.equals(host))
                    listener.requestAuthentication(host, handler);
                else handler.cancel();
            }
            @Override public boolean onRenderProcessGone(WebView webView, RenderProcessGoneDetail detail) {
                invalidate("浏览器页面已退出，请返回会话后重新打开");
                return true;
            }
        });
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

    @SuppressLint("RequiresFeature") // prepare() checks both proxy features before constructing a session.
    private void applyRules(Runnable complete) {
        long generation = ++proxyGeneration;
        routeReady = false;
        proxy.setActive(false);
        ProxyConfig.Builder configuration = new ProxyConfig.Builder()
            .addProxyRule("http://" + BrowserRoutePolicy.PROXY_HOST + ":" + proxy.port())
            .removeImplicitRules().setReverseBypassEnabled(true);
        for (String rule : BrowserRoutePolicy.proxyRules(domains)) configuration.addBypassRule(rule);
        ProxyController.getInstance().setProxyOverride(configuration.build(), MAIN::post, () -> {
            if (!closed && generation == proxyGeneration) {
                routeReady = true;
                proxy.setActive(attached && !home);
                complete.run();
            }
        });
    }

    List<String> domains() { return List.copyOf(domains); }
    long pageGeneration() { return pageGeneration; }
    boolean canDeliverFiles(long generation) {
        return !closed && attached && !home && generation == pageGeneration;
    }
    void saveDomains(List<String> values, Runnable complete) {
        preferences.save(identity.origin(), values);
        List<String> previous = domains;
        domains = preferences.load(identity.origin());
        boolean changed = !previous.equals(domains);
        proxy.updateRules(domains);
        applyRules(() -> {
            if (changed && attached && !home) {
                error = "";
                view.reload();
            }
            complete.run();
        });
    }

    void navigate(String input) {
        if (closed || !attached) return;
        if (!routeReady) throw new IllegalArgumentException("网络设置正在更新，请稍后再试");
        String target = BrowserRoutePolicy.navigationUrl(input);
        home = false; error = ""; url = target; proxy.setActive(true); ++pageGeneration;
        view.onResume(); view.resumeTimers(); view.loadUrl(target); changed();
    }
    void showHome() { home = true; proxy.setActive(false); view.onPause(); view.pauseTimers(); changed(); }
    boolean back() {
        if (!home && view.canGoBack()) { error = ""; view.goBack(); return true; }
        if (!home) { showHome(); return true; }
        return false;
    }
    void reload() { if (!closed && attached && !home) { error = ""; view.reload(); changed(); } }
    private void changed() { if (!closed && listener != null) listener.changed(); }

    private void invalidate(String message) {
        if (closed) return;
        Listener previous = listener;
        destroy();
        if (current == this) { current = null; ++openGeneration; afterClear = null; clearProfile(); }
        if (previous != null) previous.invalidated(message);
    }

    private void destroy() {
        if (closed) return;
        closed = true; attached = false; listener = null; ++proxyGeneration;
        proxy.close();
        try { observer.close(); } catch (Exception ignored) { }
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        view.stopLoading(); view.setWebChromeClient(null); view.setWebViewClient(null); view.destroy();
        context.setBaseContext(context.getApplicationContext());
    }
}
