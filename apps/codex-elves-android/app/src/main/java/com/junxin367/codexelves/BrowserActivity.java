package com.junxin367.codexelves;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.IBinder;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.webkit.HttpAuthHandler;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.ImageButton;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.ScrollView;
import android.widget.TextView;
import org.json.JSONObject;
import java.util.ArrayList;
import java.util.List;

/** Browser chrome is native; third-party pages never receive a native message bridge. */
public final class BrowserActivity extends Activity implements BrowserSession.Listener {
    private int background, foreground, muted, line, surface;
    private float fontSize;
    private LinearLayout root, footer;
    private FrameLayout content, webContainer;
    private View homePage, settingsPage;
    private TextView heading, failure;
    private EditText homeInput, addressInput, domainInput;
    private Button saveDomainsButton;
    private ImageButton back, homeButton, reload, settingsButton;
    private ProgressBar progress;
    private BrowserSession session;
    private IBinder authority;
    private boolean settings, resumed, ready;
    private android.window.OnBackInvokedCallback backCallback;
    private AlertDialog authDialog;
    private ValueCallback<Uri[]> fileChooser;
    private BrowserSession fileChooserSession;
    private long fileChooserGeneration;
    private int fileRequest = -1, nextFileRequest = 5300;
    private boolean fileResultReady;
    private Uri[] fileResult;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        background = getIntent().getIntExtra("background", Color.WHITE);
        foreground = getIntent().getIntExtra("foreground", Color.rgb(31, 31, 31));
        muted = getIntent().getIntExtra("muted", Color.rgb(113, 113, 113));
        line = getIntent().getIntExtra("line", Color.rgb(232, 232, 232));
        surface = getIntent().getIntExtra("surface", Color.rgb(249, 249, 249));
        fontSize = Math.max(12, Math.min(24, getIntent().getFloatExtra("fontSize", 14)));
        authority = getIntent().getExtras() == null ? null
            : getIntent().getExtras().getBinder(BrowserAuthority.EXTRA);
        diagnostic("native.browser.lifecycle", DiagnosticsPolicy.fields(
            "phase", "create", "sdk", Build.VERSION.SDK_INT));
        configureWindow();
        buildLayout();
        if (Build.VERSION.SDK_INT >= 33) {
            backCallback = this::onBackPressed;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback);
        }
        BrowserSession.acquire(this, authority, value -> {
                if (isFinishing() || isDestroyed()) return;
                session = value;
                if (resumed) attachSession();
            }, this::invalidated);
    }

    @SuppressWarnings("deprecation")
    private void configureWindow() {
        getWindow().setStatusBarColor(background);
        getWindow().setNavigationBarColor(background);
        boolean light = Color.red(background) * 299 + Color.green(background) * 587
            + Color.blue(background) * 114 > 128000;
        getWindow().getDecorView().setSystemUiVisibility(light
            ? View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR : 0);
        if (Build.VERSION.SDK_INT >= 29) getWindow().setNavigationBarContrastEnforced(false);
    }

    private void buildLayout() {
        root = column(); root.setFitsSystemWindows(true); root.setBackgroundColor(background);
        root.setFocusableInTouchMode(true);
        setContentView(root);
        LinearLayout header = row(); header.setPadding(dp(12), dp(6), dp(12), dp(6));
        header.setMinimumHeight(dp(50));
        heading = text("浏览器", fontSize * 1.07143f, foreground);
        heading.setTypeface(null, Typeface.BOLD); heading.setSingleLine(true);
        header.addView(heading, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        Button returnButton = textButton("返回", () -> {
            if (settings) { settings = false; hideKeyboard(); render(); }
            else finish();
        });
        header.addView(returnButton);
        root.addView(header);
        root.addView(divider(), new LinearLayout.LayoutParams(-1, dp(1)));
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setProgressTintList(ColorStateList.valueOf(Color.rgb(48, 118, 223)));
        root.addView(progress, new LinearLayout.LayoutParams(-1, dp(2)));
        content = new FrameLayout(this);
        root.addView(content, new LinearLayout.LayoutParams(-1, 0, 1));
        webContainer = new FrameLayout(this);
        content.addView(webContainer, new FrameLayout.LayoutParams(-1, -1));
        homePage = buildHome();
        content.addView(homePage, new FrameLayout.LayoutParams(-1, -1));
        settingsPage = buildSettings();
        content.addView(settingsPage, new FrameLayout.LayoutParams(-1, -1));
        failure = text("正在准备浏览器…", fontSize, muted);
        failure.setGravity(Gravity.CENTER); failure.setPadding(dp(24), dp(24), dp(24), dp(24));
        failure.setBackgroundColor(background);
        failure.setOnClickListener(ignored -> { if (session != null) session.reload(); });
        content.addView(failure, new FrameLayout.LayoutParams(-1, -1));
        root.addView(divider(), new LinearLayout.LayoutParams(-1, dp(1)));
        footer = row(); footer.setPadding(dp(4), dp(5), dp(4), dp(7)); footer.setMinimumHeight(dp(58));
        back = icon(R.drawable.browser_back, "网页后退", () -> { if (ready) session.back(); });
        homeButton = icon(R.drawable.browser_home, "浏览器首页", () -> { if (ready) session.showHome(); });
        reload = icon(R.drawable.browser_reload, "刷新网页", () -> { if (ready) session.reload(); });
        settingsButton = icon(R.drawable.browser_settings, "浏览器设置", () -> {
            if (!ready) return;
            settings = true; hideKeyboard(); renderSettings(); render();
        });
        addressInput = input("网址或搜索", fontSize * .92f);
        addressInput.setBackground(rounded(surface, line, 16));
        addressInput.setPadding(dp(12), 0, dp(12), 0);
        addressInput.setOnFocusChangeListener((view, focused) -> {
            if (session == null) return;
            addressInput.setText(focused ? session.url : displayAddress(session.url));
            if (focused) addressInput.selectAll();
        });
        submitOnEnter(addressInput);
        footer.addView(back); footer.addView(homeButton);
        footer.addView(addressInput, new LinearLayout.LayoutParams(0, dp(44), 1));
        footer.addView(reload); footer.addView(settingsButton);
        root.addView(footer);
        root.requestFocus();
        render();
    }

    private View buildHome() {
        LinearLayout page = column(); page.setGravity(Gravity.CENTER);
        page.setPadding(dp(24), dp(16), dp(24), dp(60));
        TextView brand = text("CodexElves", Math.min(fontSize * 2.6f, 44), foreground);
        brand.setTypeface(null, Typeface.BOLD); brand.setGravity(Gravity.CENTER);
        page.addView(brand, new LinearLayout.LayoutParams(-1, -2));
        LinearLayout search = row(); search.setBackground(rounded(background, line, 22));
        search.setPadding(dp(8), dp(5), dp(6), dp(5));
        homeInput = input("输入网址或百度搜索", fontSize);
        homeInput.setBackgroundColor(Color.TRANSPARENT);
        homeInput.setPadding(dp(12), dp(8), dp(4), dp(8));
        search.addView(homeInput, new LinearLayout.LayoutParams(0, dp(52), 1));
        search.addView(icon(R.drawable.browser_search, "访问或搜索", () -> navigate(homeInput)));
        LinearLayout.LayoutParams searchLayout = new LinearLayout.LayoutParams(-1, -2);
        searchLayout.topMargin = dp(32);
        page.addView(search, searchLayout);
        submitOnEnter(homeInput);
        return page;
    }

    private View buildSettings() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true); scroll.setBackgroundColor(background);
        LinearLayout body = column(); body.setPadding(dp(20), dp(20), dp(20), dp(24));
        scroll.addView(body);
        TextView defaultRoute = text("默认使用本机网络", fontSize * 1.07f, foreground);
        defaultRoute.setTypeface(null, Typeface.BOLD); body.addView(defaultRoute);
        TextView description = text("内网地址自动通过电脑访问。下方每行填写一个域名及其子域名规则，空行会自动忽略。",
            fontSize * .9f, muted);
        description.setLineSpacing(dp(4), 1); description.setPadding(0, dp(10), 0, dp(24)); body.addView(description);
        TextView label = text("通过电脑访问的域名", fontSize, foreground);
        label.setTypeface(null, Typeface.BOLD); body.addView(label);
        domainInput = input("每行一个域名，例如：\nexample.com\n*.example.com", fontSize);
        domainInput.setSingleLine(false);
        domainInput.setMaxLines(100);
        domainInput.setHorizontallyScrolling(false);
        domainInput.setGravity(Gravity.TOP | Gravity.START);
        domainInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI
            | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        domainInput.setImeOptions(EditorInfo.IME_ACTION_NONE);
        domainInput.setBackground(rounded(surface, line, 12));
        domainInput.setPadding(dp(12), dp(10), dp(12), dp(10));
        LinearLayout.LayoutParams editorLayout = new LinearLayout.LayoutParams(-1, dp(176));
        editorLayout.topMargin = dp(12);
        body.addView(domainInput, editorLayout);
        saveDomainsButton = textButton("保存", this::saveDomainEditor);
        LinearLayout.LayoutParams saveLayout = new LinearLayout.LayoutParams(-1, -2);
        saveLayout.topMargin = dp(10);
        body.addView(saveDomainsButton, saveLayout);
        return scroll;
    }

    private void renderSettings() {
        if (session == null) return;
        StringBuilder content = new StringBuilder();
        for (String domain : session.domains()) {
            if (content.length() > 0) content.append('\n');
            content.append(domain);
        }
        domainInput.setText(content.toString());
        domainInput.setSelection(domainInput.length());
    }

    private void saveDomainEditor() {
        if (session == null) return;
        try {
            List<String> values = new ArrayList<>();
            java.util.LinkedHashSet<String> unique = new java.util.LinkedHashSet<>();
            String[] lines = domainInput.getText().toString().split("\\R", -1);
            for (int index = 0; index < lines.length; index++) {
                String value = lines[index].trim();
                if (value.isEmpty()) continue;
                try {
                    unique.add(BrowserRoutePolicy.normalizeDomain(value));
                } catch (IllegalArgumentException invalid) {
                    throw new IllegalArgumentException("第 " + (index + 1) + " 行：" + invalid.getMessage());
                }
            }
            values.addAll(unique);
            saveDomains(values);
        } catch (IllegalArgumentException invalid) { domainInput.setError(invalid.getMessage()); }
    }

    private void saveDomains(List<String> values) {
        try {
            session.saveDomains(values, () -> {
                domainInput.setError(null);
                renderSettings();
            });
        }
        catch (Exception failure) { domainInput.setError(failure.getMessage()); }
    }

    private void navigate(EditText input) {
        if (!ready || session == null) return;
        try {
            String value = input.getText().toString();
            session.navigate(value); input.clearFocus(); hideKeyboard(); render();
        } catch (IllegalArgumentException invalid) { input.setError(invalid.getMessage()); }
    }

    private void submitOnEnter(EditText input) {
        input.setOnEditorActionListener((view, action, event) -> {
            if (action == EditorInfo.IME_ACTION_GO) { navigate(input); return true; }
            return false;
        });
    }

    private void render() {
        boolean home = session == null || session.home;
        heading.setText(settings ? "浏览器设置" : "浏览器");
        homePage.setVisibility(!settings && home ? View.VISIBLE : View.GONE);
        webContainer.setVisibility(!settings && !home ? View.VISIBLE : View.GONE);
        settingsPage.setVisibility(settings ? View.VISIBLE : View.GONE);
        footer.setVisibility(settings ? View.GONE : View.VISIBLE);
        back.setVisibility(home ? View.GONE : View.VISIBLE);
        homeButton.setVisibility(home ? View.GONE : View.VISIBLE);
        reload.setVisibility(home ? View.GONE : View.VISIBLE);
        addressInput.setVisibility(home ? View.INVISIBLE : View.VISIBLE);
        settingsButton.setEnabled(ready);
        homeInput.setEnabled(ready);
        addressInput.setEnabled(ready);
        domainInput.setEnabled(ready);
        saveDomainsButton.setEnabled(ready);
        progress.setVisibility(ready && !settings && !home && session.progress < 100 ? View.VISIBLE : View.INVISIBLE);
        if (session != null) {
            progress.setProgress(session.progress);
            if (!addressInput.hasFocus()) addressInput.setText(displayAddress(session.url));
            if (!session.error.isEmpty() && !home && !settings) {
                failure.setText(getString(R.string.browser_retry_message, session.error)); failure.setVisibility(View.VISIBLE);
            } else failure.setVisibility(ready ? View.GONE : View.VISIBLE);
        }
    }

    private String displayAddress(String address) {
        if (address.isEmpty()) return "";
        Uri uri = Uri.parse(address);
        if ("www.baidu.com".equals(uri.getHost()) && "/s".equals(uri.getPath())
            && uri.getQueryParameter("wd") != null) return uri.getQueryParameter("wd");
        return uri.getHost() == null ? address : uri.getHost();
    }

    private void attachSession() {
        if (session == null || isFinishing()) return;
        ready = false;
        session.attach(this, this, () -> {
            if (!resumed || isFinishing()) return;
            if (session.view.getParent() instanceof ViewGroup parent) parent.removeView(session.view);
            webContainer.addView(session.view, new FrameLayout.LayoutParams(-1, -1));
            ready = true; deliverFileResult(); render();
        });
    }

    @Override public void changed() { if (!isFinishing() && !isDestroyed()) render(); }
    @Override public void invalidated(String message) {
        diagnostic("native.browser.lifecycle", DiagnosticsPolicy.fields(
            "phase", "failure", "reason", "failed"));
        cancelFileChooser();
        ready = false;
        failure.setText(message); failure.setVisibility(View.VISIBLE);
        settingsButton.setEnabled(false); homeInput.setEnabled(false); addressInput.setEnabled(false);
        progress.setVisibility(View.INVISIBLE);
    }

    @Override public void requestAuthentication(String host, HttpAuthHandler handler) {
        if (!resumed || isFinishing()) { handler.cancel(); return; }
        if (authDialog != null) authDialog.dismiss();
        LinearLayout fields = column(); fields.setPadding(dp(20), dp(8), dp(20), 0);
        EditText username = input("用户名", fontSize), password = input("密码", fontSize);
        password.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        fields.addView(username); fields.addView(password);
        authDialog = new AlertDialog.Builder(this).setTitle(host + " 需要登录").setView(fields)
            .setPositiveButton("登录", (dialog, which) ->
                handler.proceed(username.getText().toString(), password.getText().toString()))
            .setNegativeButton("取消", (dialog, which) -> handler.cancel())
            .setOnCancelListener(dialog -> handler.cancel()).create();
        authDialog.show();
    }

    @Override public void chooseFiles(ValueCallback<Uri[]> callback, WebChromeClient.FileChooserParams parameters) {
        cancelFileChooser();
        if (!ready || !resumed || session == null) { callback.onReceiveValue(null); return; }
        fileChooser = callback; fileChooserSession = session;
        fileChooserGeneration = session.pageGeneration();
        fileRequest = nextFileRequest++;
        if (nextFileRequest > 60000) nextFileRequest = 5300;
        Intent picker;
        try {
            picker = parameters.createIntent();
            startActivityForResult(picker, fileRequest);
        } catch (ActivityNotFoundException | SecurityException | IllegalArgumentException unavailable) {
            Intent documents = new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE);
            String[] acceptTypes = parameters.getAcceptTypes();
            String mime = acceptTypes != null && acceptTypes.length == 1
                && acceptTypes[0] != null && !acceptTypes[0].trim().isEmpty()
                ? acceptTypes[0].trim() : "*/*";
            documents.setType(mime).putExtra(Intent.EXTRA_ALLOW_MULTIPLE,
                parameters.getMode() == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE);
            if (acceptTypes != null && acceptTypes.length > 1) {
                documents.putExtra(Intent.EXTRA_MIME_TYPES, acceptTypes);
            }
            try { startActivityForResult(documents, fileRequest); }
            catch (ActivityNotFoundException | SecurityException | IllegalArgumentException failed) {
                cancelFileChooser();
            }
        }
    }

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != fileRequest || fileChooser == null) return;
        List<Uri> chosen = new ArrayList<>();
        if (resultCode == RESULT_OK && data != null) {
            ClipData items = data.getClipData();
            if (items != null) {
                for (int i = 0; i < items.getItemCount(); i++) chosen.add(items.getItemAt(i).getUri());
            } else if (data.getData() != null) chosen.add(data.getData());
        }
        boolean safe = true;
        for (Uri uri : chosen) {
            safe &= uri != null && "content".equals(uri.getScheme())
                && !getPackageName().concat(".diagnostics").equals(uri.getAuthority())
                && !getPackageName().concat(".app-updates").equals(uri.getAuthority())
                && checkUriPermission(uri, android.os.Process.myPid(), android.os.Process.myUid(),
                    Intent.FLAG_GRANT_READ_URI_PERMISSION) == android.content.pm.PackageManager.PERMISSION_GRANTED;
        }
        fileResult = safe && !chosen.isEmpty() ? chosen.toArray(new Uri[0]) : null;
        fileResultReady = true;
        deliverFileResult();
    }

    private void deliverFileResult() {
        // Activity results arrive before onResume: wait for proxy/identity reattachment first.
        if (!fileResultReady || !ready || !resumed || fileChooser == null) return;
        ValueCallback<Uri[]> callback = fileChooser;
        Uri[] result = session == fileChooserSession && session.canDeliverFiles(fileChooserGeneration)
            ? fileResult : null;
        fileChooser = null; fileChooserSession = null; fileRequest = -1;
        fileResult = null; fileResultReady = false;
        callback.onReceiveValue(result);
    }

    private void cancelFileChooser() {
        int previousRequest = fileRequest;
        ValueCallback<Uri[]> callback = fileChooser;
        fileChooser = null; fileChooserSession = null; fileRequest = -1;
        fileResult = null; fileResultReady = false;
        if (callback != null) callback.onReceiveValue(null);
        if (previousRequest >= 0) {
            try { finishActivity(previousRequest); }
            catch (IllegalArgumentException | IllegalStateException ignored) {
                // The picker may have already returned or the Activity may be finishing.
            }
        }
    }

    @Override protected void onResume() {
        super.onResume(); resumed = true;
        if (session != null) attachSession();
    }
    @Override protected void onPause() {
        resumed = false; ready = false;
        if (authDialog != null) { authDialog.cancel(); authDialog = null; }
        if (session != null) session.detach(this);
        super.onPause();
    }
    @Override protected void onDestroy() {
        diagnostic("native.browser.lifecycle", DiagnosticsPolicy.fields(
            "phase", "destroy"));
        cancelFileChooser();
        if (session != null) session.detach(this);
        if (Build.VERSION.SDK_INT >= 33 && backCallback != null)
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(backCallback);
        super.onDestroy();
    }
    @Override public void onBackPressed() {
        if (Build.VERSION.SDK_INT >= 30 && root.getRootWindowInsets() != null
            && root.getRootWindowInsets().isVisible(WindowInsets.Type.ime())) {
            hideKeyboard(); return;
        }
        if (settings) { settings = false; render(); return; }
        if (ready && session.back()) return;
        finish();
    }
    @Override public void finish() {
        super.finish();
        overridePendingTransition(R.anim.browser_hold, R.anim.browser_exit);
    }
    private void hideKeyboard() {
        InputMethodManager keyboard = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
        keyboard.hideSoftInputFromWindow(root.getWindowToken(), 0);
        root.requestFocus();
    }
    private void diagnostic(String name, JSONObject fields) {
        BrowserAuthority.recordDiagnostic(authority, name, fields);
    }
    private int dp(float value) { return Math.round(value * getResources().getDisplayMetrics().density); }
    private LinearLayout column() {
        LinearLayout view = new LinearLayout(this); view.setOrientation(LinearLayout.VERTICAL); return view;
    }
    private LinearLayout row() {
        LinearLayout view = new LinearLayout(this); view.setOrientation(LinearLayout.HORIZONTAL);
        view.setGravity(Gravity.CENTER_VERTICAL); return view;
    }
    private TextView text(String value, float size, int color) {
        TextView view = new TextView(this); view.setText(value); view.setTextSize(size); view.setTextColor(color); return view;
    }
    private EditText input(String hint, float size) {
        EditText view = new EditText(this); view.setHint(hint); view.setTextSize(size);
        view.setTextColor(foreground); view.setHintTextColor(muted); view.setSingleLine(true);
        view.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        view.setImeOptions(EditorInfo.IME_ACTION_GO); view.setSaveEnabled(false);
        view.setSelectAllOnFocus(true); return view;
    }
    private Button textButton(String label, Runnable action) {
        Button view = new Button(this); view.setText(label); view.setAllCaps(false);
        view.setTextSize(fontSize * .85714f); view.setTextColor(foreground);
        view.setMinWidth(0); view.setMinimumWidth(dp(48)); view.setMinHeight(0); view.setMinimumHeight(dp(36));
        view.setPadding(dp(8), dp(6), dp(8), dp(6)); view.setBackground(ripple());
        view.setOnClickListener(ignored -> action.run()); return view;
    }
    private ImageButton icon(int resource, String description, Runnable action) {
        ImageButton view = new ImageButton(this); view.setImageResource(resource);
        view.setImageTintList(ColorStateList.valueOf(foreground)); view.setContentDescription(description);
        view.setPadding(dp(10), dp(10), dp(10), dp(10)); view.setBackground(ripple());
        view.setLayoutParams(new LinearLayout.LayoutParams(dp(44), dp(44)));
        view.setOnClickListener(ignored -> action.run()); return view;
    }
    private RippleDrawable ripple() {
        return new RippleDrawable(ColorStateList.valueOf((foreground & 0x00ffffff) | 0x22000000),
            rounded(Color.TRANSPARENT, Color.TRANSPARENT, 10), rounded(Color.WHITE, Color.TRANSPARENT, 10));
    }
    private GradientDrawable rounded(int fill, int border, float radius) {
        GradientDrawable drawable = new GradientDrawable(); drawable.setColor(fill);
        drawable.setCornerRadius(dp(radius)); drawable.setStroke(dp(1), border); return drawable;
    }
    private View divider() { View view = new View(this); view.setBackgroundColor(line); return view; }
}
