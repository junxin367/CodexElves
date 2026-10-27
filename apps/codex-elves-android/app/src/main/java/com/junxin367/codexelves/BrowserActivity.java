package com.junxin367.codexelves;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.ClipboardManager;
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
import android.text.TextUtils;
import android.view.Gravity;
import android.view.KeyEvent;
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
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.PopupWindow;
import android.widget.ScrollView;
import android.widget.Space;
import android.widget.TextView;
import android.widget.Toast;
import org.json.JSONObject;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.text.SimpleDateFormat;

/** Browser chrome is native; third-party pages never receive a native message bridge. */
public final class BrowserActivity extends Activity implements BrowserSession.Listener {
    private int background, foreground, muted, line, surface;
    private float fontSize;
    private enum InternalPage { NONE, HISTORY, BOOKMARKS, DIAGNOSTICS }
    private LinearLayout root, header, footer;
    private View headerDivider, footerDivider;
    private FrameLayout content, webContainer;
    private View homePage, settingsPage, historyPage, bookmarksPage, diagnosticsPage;
    private TextView heading, failure;
    private EditText homeInput, addressInput;
    private LinearLayout userAgentRow;
    private TextView userAgentValue;
    private ImageView userAgentArrow;
    private PopupWindow userAgentPopup;
    private ImageButton back, homeButton, reload, settingsButton, bookmarkButton;
    private ImageButton historyButton, bookmarksButton;
    private Button clearPageButton;
    private Button diagnosticsButton;
    private ScrollView homeScroll;
    private LinearLayout homeBody, recentPanel, historyPageBody, bookmarksPageBody, diagnosticsPageBody;
    private Space homeTopSpacer, homeBottomSpacer;
    private ProgressBar progress;
    private BrowserSession session;
    private IBinder authority;
    private String initialUrl = "";
    private boolean settings, resumed, ready, acquiring;
    private InternalPage internalPage = InternalPage.NONE;
    private String diagnosticsCategory = "all";
    private String diagnosticsFilter = "all";
    private long expandedDiagnosticId = -1L;
    private final HapticFeedbackController haptics = new HapticFeedbackController();
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
        initialUrl = getIntent().getStringExtra("initialUrl");
        if (initialUrl == null) initialUrl = "";
        diagnostic("native.browser.lifecycle", DiagnosticsPolicy.fields(
            "phase", "create", "sdk", Build.VERSION.SDK_INT));
        configureWindow();
        buildLayout();
        if (Build.VERSION.SDK_INT >= 33) {
            backCallback = this::onBackPressed;
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback);
        }
        acquireSession();
    }

    private void acquireSession() {
        if (acquiring || isFinishing() || isDestroyed()) return;
        if (authority == null) {
            invalidated("浏览器授权已失效，请返回会话后重试");
            return;
        }
        acquiring = true;
        failure.setText("正在准备浏览器…");
        BrowserSession.acquire(this, authority, value -> {
                acquiring = false;
                if (isFinishing() || isDestroyed()) return;
                session = value;
                if (resumed) attachSession();
            }, message -> {
                acquiring = false;
                invalidated(message);
            });
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
        header = row(); header.setPadding(dp(12), dp(6), dp(12), dp(6));
        header.setMinimumHeight(dp(50));
        heading = text("浏览器", fontSize * 1.07143f, foreground);
        heading.setTypeface(null, Typeface.BOLD); heading.setSingleLine(true);
        header.addView(heading, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        clearPageButton = textButton("清空", this::clearCurrentPage);
        header.addView(clearPageButton);
        Button returnButton = textButton("返回", () -> {
            if (settings) { settings = false; hideKeyboard(); render(); }
            else if (internalPage != InternalPage.NONE) { internalPage = InternalPage.NONE; render(); }
            else finish();
        });
        header.addView(returnButton);
        root.addView(header);
        headerDivider = divider();
        root.addView(headerDivider, new LinearLayout.LayoutParams(-1, dp(1)));
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
        historyPage = buildHistoryPage();
        content.addView(historyPage, new FrameLayout.LayoutParams(-1, -1));
        bookmarksPage = buildBookmarksPage();
        content.addView(bookmarksPage, new FrameLayout.LayoutParams(-1, -1));
        diagnosticsPage = buildDiagnosticsPage();
        content.addView(diagnosticsPage, new FrameLayout.LayoutParams(-1, -1));
        failure = text("正在准备浏览器…", fontSize, muted);
        failure.setGravity(Gravity.CENTER); failure.setPadding(dp(24), dp(24), dp(24), dp(24));
        failure.setBackgroundColor(background);
        failure.setOnClickListener(ignored -> {
            haptics.perform(failure, "haptic-click");
            if (session != null) session.reload();
            else acquireSession();
        });
        content.addView(failure, new FrameLayout.LayoutParams(-1, -1));
        footerDivider = divider();
        root.addView(footerDivider, new LinearLayout.LayoutParams(-1, dp(1)));
        footer = row(); footer.setGravity(Gravity.CENTER_VERTICAL | Gravity.END);
        footer.setPadding(dp(4), dp(5), dp(4), dp(7)); footer.setMinimumHeight(dp(58));
        back = icon(R.drawable.browser_back, "网页后退", () -> { if (ready) session.back(); });
        homeButton = icon(R.drawable.browser_home, "浏览器首页", () -> {
            if (!ready) return;
            internalPage = InternalPage.NONE;
            session.showHome();
        });
        reload = icon(R.drawable.browser_reload, "刷新网页", () -> { if (ready) session.reload(); });
        settingsButton = icon(R.drawable.browser_settings, "浏览器设置", () -> {
            if (!ready) return;
            settings = true; hideKeyboard(); renderSettings(); render();
        });
        bookmarkButton = icon(R.drawable.browser_bookmark, "添加或取消当前页书签", () -> {
            if (!ready || session == null) return;
            if (session.home || internalPage != InternalPage.NONE) openBookmarksPage();
            else session.toggleBookmark();
        });
        historyButton = icon(R.drawable.browser_history, "打开访问历史", this::openHistoryPage);
        bookmarksButton = icon(R.drawable.browser_bookmark, "打开书签", this::openBookmarksPage);
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
        footer.addView(bookmarksButton); footer.addView(historyButton);
        footer.addView(bookmarkButton); footer.addView(reload); footer.addView(settingsButton);
        root.addView(footer);
        root.requestFocus();
        render();
    }

    private View buildHome() {
        homeScroll = new ScrollView(this);
        homeScroll.setFillViewport(true);
        homeScroll.setClipToPadding(false);
        homeScroll.setBackgroundColor(background);
        homeBody = column();
        homeBody.setGravity(Gravity.TOP | Gravity.CENTER_HORIZONTAL);
        homeBody.setPadding(dp(24), 0, dp(24), 0);
        homeTopSpacer = new Space(this);
        homeBody.addView(homeTopSpacer, new LinearLayout.LayoutParams(-1, dp(16)));
        TextView brand = text("CodexElves", Math.min(fontSize * 2.6f, 44), foreground);
        brand.setTypeface(null, Typeface.BOLD); brand.setGravity(Gravity.CENTER);
        homeBody.addView(brand, new LinearLayout.LayoutParams(-1, -2));
        LinearLayout search = row(); search.setBackground(rounded(background, line, 22));
        search.setPadding(dp(8), dp(5), dp(6), dp(5));
        homeInput = input("输入网址或百度搜索", fontSize);
        homeInput.setBackgroundColor(Color.TRANSPARENT);
        homeInput.setPadding(dp(12), dp(8), dp(4), dp(8));
        search.addView(homeInput, new LinearLayout.LayoutParams(0, dp(52), 1));
        search.addView(icon(R.drawable.browser_search, "访问或搜索", () -> navigate(homeInput)));
        LinearLayout.LayoutParams searchLayout = new LinearLayout.LayoutParams(-1, -2);
        searchLayout.topMargin = dp(24);
        homeBody.addView(search, searchLayout);
        recentPanel = column();
        LinearLayout.LayoutParams recentLayout = new LinearLayout.LayoutParams(-1, -2);
        recentLayout.topMargin = dp(8);
        homeBody.addView(recentPanel, recentLayout);
        homeBottomSpacer = new Space(this);
        homeBody.addView(homeBottomSpacer, new LinearLayout.LayoutParams(-1, dp(60)));

        homeScroll.addView(homeBody, new ScrollView.LayoutParams(-1, -2));
        homeScroll.addOnLayoutChangeListener((view, left, top, right, bottom,
            oldLeft, oldTop, oldRight, oldBottom) -> updateHomeOffset());
        homeInput.setOnFocusChangeListener((view, focused) -> {
            renderHomeSections();
            if (focused) homeInput.selectAll();
        });
        homeInput.setOnClickListener(ignored -> renderHomeSections());
        submitOnEnter(homeInput);
        homeScroll.post(this::updateHomeOffset);
        return homeScroll;
    }

    private View buildSettings() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true); scroll.setBackgroundColor(background);
        LinearLayout body = column(); body.setPadding(dp(20), dp(20), dp(20), dp(24));
        scroll.addView(body);
        TextView routeTitle = text("网络访问", fontSize * 1.12f, foreground);
        routeTitle.setTypeface(null, Typeface.BOLD); body.addView(routeTitle);
        LinearLayout routeRow = row();
        routeRow.setMinimumHeight(dp(52));
        routeRow.setPadding(dp(14), dp(6), dp(14), dp(6));
        routeRow.setBackground(rounded(surface, line, 10));
        TextView routeLabel = text("请求通道", fontSize, foreground);
        routeRow.addView(routeLabel, new LinearLayout.LayoutParams(0, -2, 1));
        TextView routeValue = text("电脑端网络", fontSize * .92f, muted);
        routeValue.setGravity(Gravity.CENTER_VERTICAL | Gravity.END);
        routeRow.addView(routeValue, new LinearLayout.LayoutParams(-2, -1));
        LinearLayout.LayoutParams routeLayout = new LinearLayout.LayoutParams(-1, dp(52));
        routeLayout.topMargin = dp(12);
        body.addView(routeRow, routeLayout);

        addSettingsDivider(body);
        TextView displayTitle = text("网站显示方式", fontSize * 1.12f, foreground);
        displayTitle.setTypeface(null, Typeface.BOLD); body.addView(displayTitle);
        userAgentRow = row();
        userAgentRow.setMinimumHeight(dp(52));
        userAgentRow.setPadding(dp(14), dp(6), dp(12), dp(6));
        userAgentRow.setBackground(rounded(surface, line, 10));
        TextView userAgentLabel = text("浏览器标识（UA）", fontSize, foreground);
        userAgentRow.addView(userAgentLabel, new LinearLayout.LayoutParams(0, -2, 1));
        userAgentValue = text("", fontSize * .92f, muted);
        userAgentValue.setGravity(Gravity.CENTER_VERTICAL | Gravity.END);
        userAgentRow.addView(userAgentValue, new LinearLayout.LayoutParams(-2, -1));
        userAgentArrow = new ImageView(this);
        userAgentArrow.setImageResource(R.drawable.ic_chevron_down);
        userAgentArrow.setImageTintList(ColorStateList.valueOf(muted));
        userAgentArrow.setContentDescription("展开网站显示方式");
        userAgentArrow.setScaleType(ImageView.ScaleType.CENTER);
        LinearLayout.LayoutParams arrowLayout = new LinearLayout.LayoutParams(dp(28), -1);
        userAgentRow.addView(userAgentArrow, arrowLayout);
        userAgentRow.setOnClickListener(ignored -> showUserAgentMenu());
        userAgentArrow.setOnClickListener(ignored -> showUserAgentMenu());
        LinearLayout.LayoutParams modeRowLayout = new LinearLayout.LayoutParams(-1, dp(52));
        modeRowLayout.topMargin = dp(12);
        body.addView(userAgentRow, modeRowLayout);
        diagnosticsButton = textButton("网页诊断", this::openDiagnosticsPage);
        LinearLayout.LayoutParams diagnosticsLayout = new LinearLayout.LayoutParams(-1, -2);
        diagnosticsLayout.topMargin = dp(16);
        body.addView(diagnosticsButton, diagnosticsLayout);
        return scroll;
    }

    private View buildHistoryPage() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(background);
        historyPageBody = column();
        historyPageBody.setPadding(dp(16), dp(10), dp(16), dp(20));
        scroll.addView(historyPageBody);
        return scroll;
    }

    private View buildBookmarksPage() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(background);
        bookmarksPageBody = column();
        bookmarksPageBody.setPadding(dp(16), dp(10), dp(16), dp(20));
        scroll.addView(bookmarksPageBody);
        return scroll;
    }

    private View buildDiagnosticsPage() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(background);
        diagnosticsPageBody = column();
        diagnosticsPageBody.setPadding(dp(16), dp(10), dp(16), dp(20));
        scroll.addView(diagnosticsPageBody);
        return scroll;
    }

    private void openHistoryPage() {
        if (!ready || session == null) return;
        settings = false;
        internalPage = InternalPage.HISTORY;
        hideKeyboard();
        renderInternalPages();
        render();
    }

    private void openBookmarksPage() {
        if (!ready || session == null) return;
        settings = false;
        internalPage = InternalPage.BOOKMARKS;
        hideKeyboard();
        renderInternalPages();
        render();
    }

    private void openDiagnosticsPage() {
        if (!ready || session == null) return;
        settings = false;
        internalPage = InternalPage.DIAGNOSTICS;
        hideKeyboard();
        renderDiagnosticsPage();
        render();
    }

    private void renderInternalPages() {
        if (session == null || historyPageBody == null || bookmarksPageBody == null) return;
        historyPageBody.removeAllViews();
        List<BrowserPreferences.HistoryEntry> history = session.history();
        if (history.isEmpty()) {
            historyPageBody.addView(emptyState("还没有访问记录"));
        } else {
            for (BrowserPreferences.HistoryEntry entry : history)
                addRecordRow(historyPageBody, entry.title, entry.url,
                    () -> navigateSaved(entry.url),
                    () -> session.removeHistory(entry.url));
        }

        bookmarksPageBody.removeAllViews();
        List<BrowserPreferences.BookmarkEntry> bookmarks = session.bookmarks();
        if (bookmarks.isEmpty()) {
            bookmarksPageBody.addView(emptyState("还没有收藏网页"));
        } else {
            for (BrowserPreferences.BookmarkEntry entry : bookmarks)
                addRecordRow(bookmarksPageBody, entry.title, entry.url,
                    () -> navigateSaved(entry.url),
                    () -> session.removeBookmark(entry.url));
        }
        if (internalPage == InternalPage.DIAGNOSTICS) renderDiagnosticsPage();
    }

    private void renderDiagnosticsPage() {
        if (session == null || diagnosticsPageBody == null) return;
        diagnosticsPageBody.removeAllViews();
        List<BrowserSession.DiagnosticEvent> allEvents = new ArrayList<>();
        for (BrowserSession.DiagnosticEvent event : session.diagnosticEvents()) {
            if ("request".equals(event.phase) && !"resource-cancelled".equals(event.status))
                allEvents.add(event);
        }

        LinearLayout summary = column();
        summary.setPadding(dp(14), dp(10), dp(14), dp(10));
        summary.setBackground(rounded(Color.WHITE, line, 14));
        LinearLayout pageRow = row();
        String diagnosticPageUrl = session.diagnosticPageUrl();
        pageRow.addView(text("页面", fontSize * .78f, muted),
            new LinearLayout.LayoutParams(dp(42), -2));
        TextView pageUrl = text(diagnosticPageUrl.isEmpty() ? "首页" : shortUrl(diagnosticPageUrl),
            fontSize * .82f, foreground);
        pageUrl.setSingleLine(true);
        pageUrl.setEllipsize(TextUtils.TruncateAt.END);
        pageRow.addView(pageUrl, new LinearLayout.LayoutParams(0, -2, 1));
        Button copyButton = textButton("复制", () -> copyDiagnosticText(diagnosticPageUrl));
        copyButton.setMinimumHeight(dp(30));
        copyButton.setMinimumWidth(dp(48));
        copyButton.setTextSize(fontSize * .72f);
        pageRow.addView(copyButton, new LinearLayout.LayoutParams(dp(52), dp(32)));
        summary.addView(pageRow, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout infoRow = row();
        infoRow.addView(text("UA  " + userAgentLabel(session.userAgentMode()),
            fontSize * .74f, muted), new LinearLayout.LayoutParams(0, -2, 1));
        infoRow.addView(text("记录  " + allEvents.size() + " 条",
            fontSize * .74f, muted), new LinearLayout.LayoutParams(-2, -2));
        summary.addView(infoRow, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout liveRow = row();
        TextView live = text("● 实时诊断", fontSize * .74f, Color.rgb(35, 160, 91));
        live.setTypeface(null, Typeface.BOLD);
        liveRow.addView(live, new LinearLayout.LayoutParams(0, -2, 1));
        liveRow.addView(text("已捕获 " + allEvents.size() + " 条请求",
            fontSize * .72f, muted), new LinearLayout.LayoutParams(-2, -2));
        summary.addView(liveRow, new LinearLayout.LayoutParams(-1, -2));
        diagnosticsPageBody.addView(summary);

        int requestCount = 0, responseCount = 0, errorCount = 0;
        for (BrowserSession.DiagnosticEvent event : allEvents) {
            if (event.failure) errorCount++;
            else if (event.response) responseCount++;
            else requestCount++;
        }

        LinearLayout categoryBar = row();
        categoryBar.setPadding(dp(2), dp(10), dp(2), dp(6));
        categoryBar.setBackground(rounded(Color.rgb(245, 247, 250), Color.TRANSPARENT, 18));
        addDiagnosticsChip(categoryBar, "全部  " + allEvents.size(), "all");
        addDiagnosticsChip(categoryBar, "请求  " + requestCount, "request");
        addDiagnosticsChip(categoryBar, "响应  " + responseCount, "response");
        addDiagnosticsChip(categoryBar, "错误  " + errorCount, "error");
        diagnosticsPageBody.addView(categoryBar, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout filterBar = row();
        filterBar.setPadding(0, 0, 0, dp(8));
        addDiagnosticsFilterChip(filterBar, "全部", "all");
        addDiagnosticsFilterChip(filterBar, "仅错误", "error");
        addDiagnosticsFilterChip(filterBar, "耗时", "duration");
        addDiagnosticsFilterChip(filterBar, "资源", "resource");
        diagnosticsPageBody.addView(filterBar, new LinearLayout.LayoutParams(-1, -2));

        List<BrowserSession.DiagnosticEvent> events = new ArrayList<>();
        long maxDuration = 1L;
        for (BrowserSession.DiagnosticEvent event : allEvents) {
            if (!diagnosticsMatches(event)) continue;
            events.add(event);
            maxDuration = Math.max(maxDuration, event.durationMs);
        }
        if (events.isEmpty()) {
            diagnosticsPageBody.addView(emptyState(
                allEvents.isEmpty() ? "暂无网页请求" : "当前筛选没有记录"));
            return;
        }
        for (int index = events.size() - 1; index >= 0; index--) {
            addDiagnosticsCard(events.get(index), maxDuration);
        }
    }

    private void addDiagnosticsChip(LinearLayout container, String label, String category) {
        boolean selected = diagnosticsCategory.equals(category);
        Button chip = textButton(label, () -> {
            diagnosticsCategory = category;
            renderDiagnosticsPage();
        });
        chip.setTextSize(fontSize * .72f);
        chip.setTextColor(selected ? Color.WHITE : foreground);
        chip.setGravity(Gravity.CENTER);
        chip.setMinHeight(0);
        chip.setMinimumHeight(dp(34));
        chip.setPadding(dp(5), 0, dp(5), 0);
        chip.setBackground(rounded(selected ? Color.rgb(39, 139, 241) : Color.TRANSPARENT,
            selected ? Color.rgb(39, 139, 241) : Color.TRANSPARENT, 18));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(34), 1);
        params.leftMargin = dp(2);
        params.rightMargin = dp(2);
        container.addView(chip, params);
    }

    private void addDiagnosticsFilterChip(LinearLayout container, String label, String filter) {
        boolean selected = diagnosticsFilter.equals(filter);
        Button chip = textButton(label + ("all".equals(filter) ? "⌄" : ""),
            () -> { diagnosticsFilter = filter; renderDiagnosticsPage(); });
        chip.setTextSize(fontSize * .70f);
        chip.setTextColor(selected ? Color.rgb(39, 139, 241) : muted);
        chip.setGravity(Gravity.CENTER);
        chip.setMinHeight(0);
        chip.setMinimumHeight(dp(32));
        chip.setPadding(dp(4), 0, dp(4), 0);
        chip.setBackground(rounded(selected ? Color.rgb(237, 245, 255) : Color.TRANSPARENT,
            selected ? Color.rgb(39, 139, 241) : Color.TRANSPARENT, 16));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(32), 1);
        params.leftMargin = dp(2);
        params.rightMargin = dp(2);
        container.addView(chip, params);
    }

    private boolean diagnosticsMatches(BrowserSession.DiagnosticEvent event) {
        if ("error".equals(diagnosticsCategory) && !event.failure) return false;
        if ("request".equals(diagnosticsCategory) && (event.response || event.failure)) return false;
        if ("response".equals(diagnosticsCategory) && (!event.response || event.failure)) return false;
        if ("error".equals(diagnosticsFilter) && !event.failure) return false;
        if ("duration".equals(diagnosticsFilter) && event.durationMs <= 0) return false;
        if ("resource".equals(diagnosticsFilter) && (event.resourceType == null
            || event.resourceType.isEmpty() || "other".equals(event.resourceType))) return false;
        return true;
    }

    private void addDiagnosticsCard(BrowserSession.DiagnosticEvent event, long maxDuration) {
        LinearLayout card = column();
        card.setPadding(dp(10), dp(8), dp(10), dp(8));
        card.setBackground(rounded(Color.WHITE, line, 12));
        card.setOnClickListener(ignored -> {
            haptics.perform(card, "haptic-click");
            expandedDiagnosticId = expandedDiagnosticId == event.id ? -1L : event.id;
            renderDiagnosticsPage();
        });

        LinearLayout top = row();
        TextView method = text(event.method == null || event.method.isEmpty() ? "GET" : event.method,
            fontSize * .68f, Color.WHITE);
        method.setGravity(Gravity.CENTER);
        method.setTypeface(null, Typeface.BOLD);
        method.setBackground(rounded("POST".equalsIgnoreCase(event.method)
            ? Color.rgb(129, 81, 214) : Color.rgb(39, 139, 241),
            Color.TRANSPARENT, 6));
        top.addView(method, new LinearLayout.LayoutParams(dp(42), dp(26)));

        LinearLayout address = column();
        TextView url = text(shortUrl(event.url), fontSize * .78f, foreground);
        url.setSingleLine(true);
        url.setEllipsize(TextUtils.TruncateAt.END);
        TextView type = text(resourceTypeLabel(event.resourceType), fontSize * .66f, muted);
        address.addView(url, new LinearLayout.LayoutParams(-1, -2));
        address.addView(type, new LinearLayout.LayoutParams(-1, -2));
        LinearLayout.LayoutParams addressParams = new LinearLayout.LayoutParams(0, -2, 1);
        addressParams.leftMargin = dp(8);
        top.addView(address, addressParams);

        TextView status = text(diagnosticStatus(event), fontSize * .66f,
            diagnosticStatusColor(event, true));
        status.setGravity(Gravity.CENTER);
        status.setPadding(dp(7), 0, dp(7), 0);
        status.setBackground(rounded(diagnosticStatusColor(event, false),
            diagnosticStatusColor(event, false), 10));
        top.addView(status, new LinearLayout.LayoutParams(-2, dp(24)));
        TextView duration = text(durationLabel(event.durationMs), fontSize * .62f, muted);
        duration.setGravity(Gravity.CENTER);
        duration.setSingleLine(true);
        top.addView(duration, new LinearLayout.LayoutParams(-2, dp(26)));
        TextView size = text(sizeLabel(event.sizeBytes), fontSize * .62f, muted);
        size.setGravity(Gravity.CENTER);
        size.setSingleLine(true);
        top.addView(size, new LinearLayout.LayoutParams(-2, dp(26)));
        TextView arrow = text(expandedDiagnosticId == event.id ? "⌃" : "⌄",
            fontSize * .90f, muted);
        arrow.setGravity(Gravity.CENTER);
        top.addView(arrow, new LinearLayout.LayoutParams(dp(24), dp(26)));
        card.addView(top, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout bar = row();
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setBackground(rounded(Color.rgb(231, 235, 241), Color.TRANSPARENT, 3));
        float ratio = event.durationMs <= 0 ? .08f
            : Math.max(.08f, Math.min(1f, event.durationMs / (float) Math.max(1L, maxDuration)));
        View progress = new View(this);
        progress.setBackground(rounded(diagnosticStatusColor(event, false),
            Color.TRANSPARENT, 3));
        bar.addView(progress, new LinearLayout.LayoutParams(0, dp(4), ratio));
        View remaining = new View(this);
        bar.addView(remaining, new LinearLayout.LayoutParams(0, dp(4), 1f - ratio));
        LinearLayout.LayoutParams barParams = new LinearLayout.LayoutParams(-1, dp(4));
        barParams.topMargin = dp(5);
        card.addView(bar, barParams);

        if (expandedDiagnosticId == event.id) {
            LinearLayout details = column();
            details.setPadding(dp(10), dp(8), dp(10), dp(8));
            int detailBackground = event.failure ? Color.rgb(255, 240, 241)
                : Color.rgb(245, 248, 252);
            int detailLine = event.failure ? Color.rgb(255, 218, 220) : line;
            details.setBackground(rounded(detailBackground, detailLine, 9));
            TextView title = text(event.failure
                ? "!  " + (event.detail.isEmpty() ? "请求失败" : event.detail)
                : "请求详情", fontSize * .76f,
                event.failure ? Color.rgb(216, 55, 65) : foreground);
            title.setTypeface(null, Typeface.BOLD);
            details.addView(title);
            addDiagnosticDetail(details, "请求 URL", event.url);
            addDiagnosticDetail(details, "请求方法", event.method);
            addDiagnosticDetail(details, "资源类型", resourceTypeLabel(event.resourceType));
            addDiagnosticDetail(details, "状态", diagnosticStatus(event));
            if (event.failure) addDiagnosticDetail(details, "错误类型", event.status.isEmpty()
                ? "network" : event.status);
            addDiagnosticDetail(details, "发生时间",
                new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault())
                    .format(event.timeMs));
            if (event.failure) addDiagnosticDetail(details, "重试建议", "检查网络连接后重试");
            if (!event.headers.isEmpty()) addDiagnosticDetail(details, "请求头", event.headers);
            card.addView(details, new LinearLayout.LayoutParams(-1, -2));
        }

        LinearLayout.LayoutParams layout = new LinearLayout.LayoutParams(-1, -2);
        layout.topMargin = dp(4);
        diagnosticsPageBody.addView(card, layout);
    }

    private void addDiagnosticDetail(LinearLayout container, String label, String value) {
        LinearLayout line = row();
        line.addView(text(label, fontSize * .66f, muted),
            new LinearLayout.LayoutParams(dp(72), -2));
        TextView content = text(value == null ? "" : value, fontSize * .66f, foreground);
        content.setSingleLine(true);
        content.setEllipsize(TextUtils.TruncateAt.END);
        line.addView(content, new LinearLayout.LayoutParams(0, -2, 1));
        container.addView(line, new LinearLayout.LayoutParams(-1, -2));
    }

    private String diagnosticStatus(BrowserSession.DiagnosticEvent event) {
        if (event.failure) return event.statusCode > 0 ? String.valueOf(event.statusCode) : "失败";
        if (event.statusCode > 0) return String.valueOf(event.statusCode);
        return event.response ? "完成" : "请求";
    }

    private int diagnosticStatusColor(BrowserSession.DiagnosticEvent event, boolean textColor) {
        if (event.failure) return textColor ? Color.rgb(216, 55, 65) : Color.rgb(255, 210, 214);
        if (event.statusCode >= 300 && event.statusCode < 400)
            return textColor ? Color.rgb(111, 117, 128) : Color.rgb(235, 237, 241);
        if (event.response) return textColor ? Color.rgb(35, 160, 91) : Color.rgb(205, 242, 216);
        return textColor ? Color.rgb(39, 139, 241) : Color.rgb(224, 239, 255);
    }

    private String resourceTypeLabel(String value) {
        if ("stylesheets".equals(value)) return "stylesheets";
        if ("document".equals(value)) return "document";
        if ("script".equals(value)) return "script";
        if ("xhr".equals(value)) return "xhr";
        if ("image".equals(value)) return "image";
        if ("font".equals(value)) return "font";
        return value == null || value.isEmpty() ? "resource" : value;
    }

    private String durationLabel(long durationMs) {
        return durationMs <= 0 ? "--" : durationMs + "ms";
    }

    private String sizeLabel(long sizeBytes) {
        if (sizeBytes <= 0) return "0 B";
        if (sizeBytes < 1024) return sizeBytes + " B";
        if (sizeBytes < 1024 * 1024) return String.format(Locale.getDefault(), "%.1f KB",
            sizeBytes / 1024f);
        return String.format(Locale.getDefault(), "%.1f MB", sizeBytes / (1024f * 1024f));
    }

    private String shortUrl(String value) {
        if (value == null) return "";
        return value.length() > 100 ? value.substring(0, 97) + "..." : value;
    }

    private void copyDiagnosticText(String value) {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null) return;
        clipboard.setPrimaryClip(ClipData.newPlainText("网页诊断", value == null ? "" : value));
        Toast.makeText(this, "已复制", Toast.LENGTH_SHORT).show();
    }

    private void renderSettings() {
        if (session == null) return;
        String selectedUserAgent = userAgentLabel(session.userAgentMode());
        userAgentValue.setText(selectedUserAgent);
        userAgentRow.setContentDescription("浏览器标识（UA），当前为" + selectedUserAgent);
    }

    private void showUserAgentMenu() {
        if (!ready || session == null || userAgentRow == null) return;
        dismissUserAgentMenu();
        haptics.perform(userAgentRow, "haptic-click");
        userAgentArrow.setRotation(180f);
        String current = session.userAgentMode();
        LinearLayout menuBody = column();
        menuBody.setPadding(dp(4), dp(4), dp(4), dp(4));
        menuBody.setBackground(rounded(background, line, 12));

        addUserAgentMenuItem(menuBody, "手机版", BrowserPreferences.UA_ANDROID, current);
        addUserAgentMenuItem(menuBody, "电脑版", BrowserPreferences.UA_DESKTOP, current);
        addUserAgentMenuItem(menuBody, "iPhone", BrowserPreferences.UA_IPHONE, current);

        int maxWidth = Math.max(dp(168), userAgentRow.getWidth() - dp(24));
        int menuWidth = Math.min(dp(220), maxWidth);
        userAgentPopup = new PopupWindow(menuBody, menuWidth, ViewGroup.LayoutParams.WRAP_CONTENT, true);
        userAgentPopup.setBackgroundDrawable(new android.graphics.drawable.ColorDrawable(Color.TRANSPARENT));
        userAgentPopup.setOutsideTouchable(true);
        userAgentPopup.setClippingEnabled(true);
        userAgentPopup.setInputMethodMode(PopupWindow.INPUT_METHOD_NOT_NEEDED);
        if (Build.VERSION.SDK_INT >= 21) userAgentPopup.setElevation(dp(8));
        userAgentPopup.setOnDismissListener(this::dismissedUserAgentMenu);
        userAgentPopup.showAsDropDown(userAgentRow, dp(0), dp(4), Gravity.END);
    }

    private void addUserAgentMenuItem(LinearLayout menuBody, String label, String mode, String current) {
        LinearLayout item = row();
        item.setMinimumHeight(dp(42));
        item.setPadding(dp(12), dp(4), dp(8), dp(4));
        item.setBackground(ripple());
        item.setContentDescription(label + (mode.equals(current) ? "，当前已选择" : ""));
        item.setOnClickListener(ignored -> {
            haptics.perform(item, "haptic-click");
            dismissUserAgentMenu();
            setUserAgentMode(mode);
        });

        TextView labelView = text(label, fontSize, foreground);
        labelView.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        item.addView(labelView, new LinearLayout.LayoutParams(0, -1, 1));

        TextView indicator = text(mode.equals(current) ? "✓" : "", fontSize * 1.05f,
            mode.equals(current) ? background : muted);
        indicator.setGravity(Gravity.CENTER);
        indicator.setTypeface(null, Typeface.BOLD);
        indicator.setBackground(rounded(mode.equals(current) ? foreground : Color.TRANSPARENT,
            mode.equals(current) ? foreground : muted, 3));
        item.addView(indicator, new LinearLayout.LayoutParams(dp(24), dp(24)));
        menuBody.addView(item, new LinearLayout.LayoutParams(-1, dp(42)));
    }

    private void dismissUserAgentMenu() {
        if (userAgentPopup != null) {
            userAgentPopup.setOnDismissListener(null);
            userAgentPopup.dismiss();
            userAgentPopup = null;
        }
        if (userAgentArrow != null) userAgentArrow.setRotation(0f);
    }

    private void dismissedUserAgentMenu() {
        userAgentPopup = null;
        if (userAgentArrow != null) userAgentArrow.setRotation(0f);
    }

    private String userAgentLabel(String mode) {
        if (BrowserPreferences.UA_DESKTOP.equals(mode)) return "电脑版";
        if (BrowserPreferences.UA_IPHONE.equals(mode)) return "iPhone";
        return "手机版";
    }

    private void setUserAgentMode(String mode) {
        if (!ready || session == null) return;
        try {
            session.saveUserAgentMode(mode, this::renderSettings);
        } catch (RuntimeException failure) {
            Toast.makeText(this, failure.getMessage(), Toast.LENGTH_SHORT).show();
        }
    }

    private void clearCurrentPage() {
        if (internalPage == InternalPage.HISTORY) confirmClearHistory();
        else if (internalPage == InternalPage.BOOKMARKS) confirmClearBookmarks();
        else if (internalPage == InternalPage.DIAGNOSTICS && session != null) {
            session.clearDiagnosticEvents();
            renderDiagnosticsPage();
            Toast.makeText(this, "网页诊断记录已清空", Toast.LENGTH_SHORT).show();
        }
    }

    private void confirmClearHistory() {
        if (!ready || session == null) return;
        new AlertDialog.Builder(this)
            .setTitle("清空访问历史")
            .setMessage("确定删除当前来源的全部访问历史吗？")
            .setNegativeButton("暂不清空", null)
            .setPositiveButton("清空", (dialog, which) -> {
                try {
                    session.clearHistory();
                    Toast.makeText(this, "访问历史已清空", Toast.LENGTH_SHORT).show();
                } catch (RuntimeException failure) {
                    Toast.makeText(this, failure.getMessage(), Toast.LENGTH_SHORT).show();
                }
                renderInternalPages();
                renderHomeSections();
            })
            .show();
    }

    private void confirmClearBookmarks() {
        if (!ready || session == null) return;
        new AlertDialog.Builder(this)
            .setTitle("清空全部书签")
            .setMessage("确定删除当前来源的全部书签吗？")
            .setNegativeButton("暂不清空", null)
            .setPositiveButton("清空", (dialog, which) -> {
                try {
                    session.clearBookmarks();
                    Toast.makeText(this, "全部书签已清空", Toast.LENGTH_SHORT).show();
                } catch (RuntimeException failure) {
                    Toast.makeText(this, failure.getMessage(), Toast.LENGTH_SHORT).show();
                }
                renderInternalPages();
                renderHomeSections();
            })
            .show();
    }

    private void navigate(EditText input) {
        if (!ready || session == null) return;
        try {
            String value = input.getText().toString();
            session.navigate(value); input.clearFocus(); hideKeyboard(); render();
        } catch (IllegalArgumentException invalid) { input.setError(invalid.getMessage()); }
    }

    private void navigateSaved(String target) {
        if (!ready || session == null || target == null || target.isEmpty()) return;
        try {
            internalPage = InternalPage.NONE;
            session.navigate(target);
            homeInput.clearFocus();
            hideKeyboard();
            render();
        } catch (IllegalArgumentException invalid) { homeInput.setError(invalid.getMessage()); }
    }

    private void submitOnEnter(EditText input) {
        input.setOnEditorActionListener((view, action, event) -> {
            boolean enter = event != null && event.getKeyCode() == KeyEvent.KEYCODE_ENTER
                && event.getAction() == KeyEvent.ACTION_DOWN;
            if (action == EditorInfo.IME_ACTION_GO || action == EditorInfo.IME_ACTION_DONE || enter) {
                navigate(input); return true;
            }
            return false;
        });
    }

    private void render() {
        boolean home = session == null || session.home;
        boolean internal = internalPage != InternalPage.NONE;
        boolean showHome = !settings && !internal && home;
        boolean showWeb = !settings && !internal && !home;
        heading.setText(settings ? "浏览器设置"
            : internalPage == InternalPage.HISTORY ? "访问历史"
            : internalPage == InternalPage.BOOKMARKS ? "书签"
            : internalPage == InternalPage.DIAGNOSTICS ? "网页诊断" : "浏览器");
        homePage.setVisibility(showHome ? View.VISIBLE : View.GONE);
        webContainer.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        settingsPage.setVisibility(settings ? View.VISIBLE : View.GONE);
        historyPage.setVisibility(!settings && internalPage == InternalPage.HISTORY ? View.VISIBLE : View.GONE);
        bookmarksPage.setVisibility(!settings && internalPage == InternalPage.BOOKMARKS ? View.VISIBLE : View.GONE);
        diagnosticsPage.setVisibility(!settings && internalPage == InternalPage.DIAGNOSTICS ? View.VISIBLE : View.GONE);
        boolean diagnostics = internalPage == InternalPage.DIAGNOSTICS;
        header.setVisibility(diagnostics ? View.GONE : View.VISIBLE);
        headerDivider.setVisibility(diagnostics ? View.GONE : View.VISIBLE);
        footer.setVisibility(settings || diagnostics ? View.GONE : View.VISIBLE);
        footerDivider.setVisibility(settings || diagnostics ? View.GONE : View.VISIBLE);
        back.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        homeButton.setVisibility(showWeb || internal ? View.VISIBLE : View.GONE);
        reload.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        bookmarkButton.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        bookmarksButton.setVisibility(!settings && (showHome
            || internalPage == InternalPage.HISTORY || internalPage == InternalPage.BOOKMARKS) ? View.VISIBLE : View.GONE);
        historyButton.setVisibility(!settings && (showHome
            || internalPage == InternalPage.HISTORY || internalPage == InternalPage.BOOKMARKS) ? View.VISIBLE : View.GONE);
        addressInput.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        settingsButton.setVisibility(settings ? View.GONE : View.VISIBLE);
        clearPageButton.setVisibility(internal ? View.VISIBLE : View.GONE);
        clearPageButton.setEnabled(ready && internal);
        settingsButton.setEnabled(ready);
        homeInput.setEnabled(ready && showHome);
        addressInput.setEnabled(ready && showWeb);
        boolean settingsReady = ready && settings;
        userAgentRow.setEnabled(settingsReady);
        userAgentRow.setAlpha(settingsReady ? 1f : .55f);
        diagnosticsButton.setEnabled(settingsReady);
        progress.setVisibility(diagnostics ? View.GONE
            : ready && showWeb && session.progress < 100 ? View.VISIBLE : View.INVISIBLE);
        if (session != null) {
            progress.setProgress(session.progress);
            if (showWeb && !addressInput.hasFocus()) addressInput.setText(displayAddress(session.url));
            bookmarkButton.setImageTintList(ColorStateList.valueOf(
                session.bookmarked() ? Color.rgb(48, 118, 223) : foreground));
            bookmarkButton.setContentDescription(session.bookmarked() ? "取消当前页书签" : "添加当前页书签");
            if (!session.error.isEmpty() && showWeb) {
                failure.setText(getString(R.string.browser_retry_message, session.error)); failure.setVisibility(View.VISIBLE);
            } else failure.setVisibility(ready ? View.GONE : View.VISIBLE);
        }
        if (internal) renderInternalPages();
        renderHomeSections();
    }

    private void renderHomeSections() {
        if (recentPanel == null) return;
        recentPanel.removeAllViews();
        boolean showHome = ready && session != null && session.home && !settings;
        if (!showHome) {
            recentPanel.setVisibility(View.GONE);
            return;
        }

        List<BrowserPreferences.HistoryEntry> recent = session.recentHistory(6);
        if (homeInput.hasFocus() && !recent.isEmpty()) {
            for (BrowserPreferences.HistoryEntry entry : recent)
                addRecordRow(recentPanel, entry.title, entry.url,
                    () -> navigateSaved(entry.url), () -> session.removeHistory(entry.url));
            recentPanel.setVisibility(View.VISIBLE);
        } else {
            recentPanel.setVisibility(View.GONE);
        }
    }

    private TextView emptyState(String value) {
        TextView view = text(value, fontSize * .9f, muted);
        view.setPadding(dp(8), dp(8), dp(8), dp(8));
        return view;
    }

    private void addRecordRow(LinearLayout container, String title, String target,
        Runnable open, Runnable remove) {
        LinearLayout row = row();
        row.setMinimumHeight(dp(40));
        row.setPadding(dp(8), dp(1), dp(2), dp(1));
        row.setBackground(ripple());
        row.setOnClickListener(ignored -> {
            haptics.perform(row, "haptic-click");
            open.run();
        });
        LinearLayout labels = column();
        String primary = recordLabel(title, target);
        TextView titleView = text(primary, fontSize * .9f, foreground);
        titleView.setSingleLine(true);
        titleView.setEllipsize(TextUtils.TruncateAt.END);
        TextView targetView = text(recordSubtitle(target), fontSize * .74f, muted);
        targetView.setSingleLine(true);
        targetView.setEllipsize(TextUtils.TruncateAt.END);
        labels.addView(titleView, new LinearLayout.LayoutParams(-1, -2));
        labels.addView(targetView, new LinearLayout.LayoutParams(-1, -2));
        row.addView(labels, new LinearLayout.LayoutParams(0, -2, 1));
        if (remove != null) {
            ImageButton removeButton = icon(R.drawable.browser_delete, "删除记录", remove);
            row.addView(removeButton, new LinearLayout.LayoutParams(dp(40), dp(40)));
        }
        LinearLayout.LayoutParams layout = new LinearLayout.LayoutParams(-1, -2);
        layout.topMargin = 0;
        container.addView(row, layout);
    }

    private String recordLabel(String title, String target) {
        Uri uri = Uri.parse(target);
        if ("www.baidu.com".equals(uri.getHost()) && "/s".equals(uri.getPath())
            && uri.getQueryParameter("wd") != null) return uri.getQueryParameter("wd");
        return title == null || title.trim().isEmpty() ? displayAddress(target) : title.trim();
    }

    private String recordSubtitle(String target) {
        Uri uri = Uri.parse(target);
        if ("www.baidu.com".equals(uri.getHost()) && "/s".equals(uri.getPath())
            && uri.getQueryParameter("wd") != null) return "搜索：" + uri.getQueryParameter("wd");
        return target;
    }

    private void updateHomeOffset() {
        if (homeScroll == null || homeBody == null || homeTopSpacer == null || homeBottomSpacer == null) return;
        int top = Math.round(homeScroll.getHeight() * .28f);
        updateSpacer(homeTopSpacer, top);
        updateSpacer(homeBottomSpacer, dp(60));
        homeBody.setTranslationY(0);
    }

    private void updateSpacer(Space spacer, int height) {
        ViewGroup.LayoutParams params = spacer.getLayoutParams();
        if (params.height == height) return;
        params.height = height;
        spacer.setLayoutParams(params);
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
            ready = true;
            String pendingUrl = initialUrl;
            initialUrl = "";
            if (!pendingUrl.isEmpty()) {
                try { session.navigate(pendingUrl); }
                catch (IllegalArgumentException invalid) { homeInput.setError(invalid.getMessage()); }
            }
            deliverFileResult(); render();
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
        bookmarkButton.setEnabled(false); userAgentRow.setEnabled(false);
        diagnosticsButton.setEnabled(false);
        bookmarksButton.setEnabled(false); historyButton.setEnabled(false);
        clearPageButton.setEnabled(false);
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
        super.onResume(); resumed = true; haptics.setForeground(true);
        if (session != null) attachSession();
    }
    @Override protected void onPause() {
        resumed = false; ready = false; haptics.setForeground(false);
        dismissUserAgentMenu();
        if (authDialog != null) { authDialog.cancel(); authDialog = null; }
        if (session != null) session.detach(this);
        super.onPause();
    }
    @Override protected void onDestroy() {
        diagnostic("native.browser.lifecycle", DiagnosticsPolicy.fields(
            "phase", "destroy"));
        haptics.setForeground(false);
        dismissUserAgentMenu();
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
        if (internalPage != InternalPage.NONE) { internalPage = InternalPage.NONE; render(); return; }
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
    private void addSettingsDivider(LinearLayout body) {
        View divider = divider();
        LinearLayout.LayoutParams layout = new LinearLayout.LayoutParams(-1, dp(1));
        layout.topMargin = dp(28);
        layout.bottomMargin = dp(24);
        body.addView(divider, layout);
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
        view.setOnClickListener(ignored -> {
            haptics.perform(view, "haptic-click");
            action.run();
        }); return view;
    }
    private Button primaryButton(String label, Runnable action) {
        Button view = textButton(label, action);
        view.setTextSize(fontSize * .95f);
        view.setTextColor(background);
        view.setMinimumHeight(dp(48));
        view.setBackground(rounded(foreground, foreground, 10));
        return view;
    }
    private ImageButton icon(int resource, String description, Runnable action) {
        ImageButton view = new ImageButton(this); view.setImageResource(resource);
        view.setImageTintList(ColorStateList.valueOf(foreground)); view.setContentDescription(description);
        view.setPadding(dp(10), dp(10), dp(10), dp(10)); view.setBackground(ripple());
        view.setLayoutParams(new LinearLayout.LayoutParams(dp(44), dp(44)));
        view.setOnClickListener(ignored -> {
            haptics.perform(view, "haptic-click");
            action.run();
        }); return view;
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
