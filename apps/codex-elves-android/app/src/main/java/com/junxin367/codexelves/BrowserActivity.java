package com.junxin367.codexelves;

import android.animation.ValueAnimator;
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
import android.widget.HorizontalScrollView;
import android.widget.ImageButton;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.PopupWindow;
import android.widget.ScrollView;
import android.widget.Space;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;
import org.json.JSONObject;
import org.json.JSONArray;
import org.json.JSONTokener;
import org.json.JSONException;
import java.util.ArrayList;
import java.util.List;
import java.util.HashSet;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import java.util.Locale;
import java.text.SimpleDateFormat;
import java.time.Instant;
import java.time.LocalDate;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;

/** Browser chrome is native; pages receive no native bridge unless recording is explicitly enabled. */
public final class BrowserActivity extends Activity implements BrowserSession.Listener {
    private int background, foreground, muted, line, surface, accent;
    private float fontSize;
    private enum InternalPage { NONE, HISTORY, BOOKMARKS, DIAGNOSTICS, CREDENTIALS }
    private LinearLayout root, header, footer;
    private View headerDivider, footerDivider;
    private FrameLayout content, webContainer;
    private View homePage, settingsPage, historyPage, bookmarksPage, diagnosticsPage, credentialsPage;
    private TextView heading, failure;
    private EditText homeInput, addressInput;
    private LinearLayout userAgentRow;
    private TextView userAgentValue, credentialErrorValue;
    private ImageView userAgentArrow;
    private PopupWindow userAgentPopup;
    private Button back, homeButton, reload, settingsButton, bookmarkButton;
    private Button historyButton, bookmarksButton;
    private Button clearPageButton;
    private Button networkButton, headerNetworkButton;
    private Button credentialsButton;
    private Switch credentialRecordingSwitch;
    private boolean credentialSwitchSyncing;
    private final Set<String> revealedCredentialIds = new HashSet<>();
    private final Map<Long, Integer> diagnosticBodyPages = new HashMap<>();
    private ScrollView homeScroll;
    private LinearLayout homeBody, recentPanel, historyPageBody, bookmarksPageBody, diagnosticsPageBody, credentialsPageBody;
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
        accent = getIntent().getIntExtra("accent", Color.rgb(23, 23, 23));
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
        headerNetworkButton = textButton("网络", this::openDiagnosticsPage);
        header.addView(headerNetworkButton);
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
        progress.setProgressTintList(ColorStateList.valueOf(accent));
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
        credentialsPage = buildCredentialsPage();
        content.addView(credentialsPage, new FrameLayout.LayoutParams(-1, -1));
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
        back = footerAction(R.drawable.browser_back, "后退", "网页后退", () -> { if (ready) session.back(); });
        homeButton = footerAction(R.drawable.browser_home, "首页", "浏览器首页", () -> {
            if (!ready) return;
            internalPage = InternalPage.NONE;
            session.showHome();
        });
        reload = footerAction(R.drawable.browser_reload, "刷新", "刷新网页", () -> { if (ready) session.reload(); });
        networkButton = footerAction(R.drawable.browser_network, "网络", "打开网络诊断", this::openDiagnosticsPage);
        settingsButton = footerAction(R.drawable.browser_settings, "设置", "浏览器设置", () -> {
            if (!ready) return;
            settings = true; hideKeyboard(); renderSettings(); render();
        });
        bookmarkButton = footerAction(R.drawable.browser_bookmark, "收藏", "添加或取消当前页书签", () -> {
            if (!ready || session == null) return;
            if (session.home || internalPage != InternalPage.NONE) openBookmarksPage();
            else session.toggleBookmark();
        });
        historyButton = footerAction(R.drawable.browser_history, "历史", "打开访问历史", this::openHistoryPage);
        bookmarksButton = footerAction(R.drawable.browser_bookmark, "书签", "打开书签", this::openBookmarksPage);
        addressInput = input("网址或搜索", fontSize * .92f);
        addressInput.setBackground(rounded(surface, line, 16));
        addressInput.setPadding(dp(12), 0, dp(12), 0);
        addressInput.setOnFocusChangeListener((view, focused) -> {
            if (session == null) return;
            String localFile = BrowserRoutePolicy.workspaceViewDisplay(session.url);
            addressInput.setText(focused && localFile.isEmpty()
                ? session.url : localFile.isEmpty() ? displayAddress(session.url) : localFile);
            if (focused) addressInput.selectAll();
        });
        submitOnEnter(addressInput);
        footer.addView(back); footer.addView(homeButton);
        footer.addView(addressInput, new LinearLayout.LayoutParams(0, dp(44), 1));
        footer.addView(bookmarksButton); footer.addView(historyButton);
        footer.addView(bookmarkButton); footer.addView(reload);
        footer.addView(networkButton); footer.addView(settingsButton);
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
        addSettingsDivider(body);
        TextView browserTitle = text("浏览器", fontSize * 1.12f, foreground);
        browserTitle.setTypeface(null, Typeface.BOLD); body.addView(browserTitle);
        credentialRecordingSwitch = new Switch(this);
        credentialRecordingSwitch.setText("记录账号密码");
        credentialRecordingSwitch.setTextSize(fontSize * .95f);
        credentialRecordingSwitch.setTextColor(foreground);
        credentialRecordingSwitch.setThumbTintList(new ColorStateList(
            new int[][]{{android.R.attr.state_checked}, {}}, new int[]{accent, muted}));
        credentialRecordingSwitch.setTrackTintList(new ColorStateList(
            new int[][]{{android.R.attr.state_checked}, {}}, new int[]{accentWithAlpha(80), line}));
        credentialRecordingSwitch.setGravity(Gravity.CENTER_VERTICAL);
        credentialRecordingSwitch.setPadding(dp(14), dp(6), dp(14), dp(6));
        credentialRecordingSwitch.setBackground(rounded(surface, line, 10));
        credentialRecordingSwitch.setOnCheckedChangeListener((view, checked) -> {
            if (credentialSwitchSyncing || !ready || session == null) return;
            try { session.setCredentialRecordingEnabled(checked); }
            catch (RuntimeException failure) {
                Toast.makeText(this, failure.getMessage(), Toast.LENGTH_SHORT).show();
            }
            renderSettings();
        });
        LinearLayout.LayoutParams recordingLayout = new LinearLayout.LayoutParams(-1, dp(52));
        recordingLayout.topMargin = dp(12);
        body.addView(credentialRecordingSwitch, recordingLayout);
        credentialsButton = textButton("账号密码记录", this::openCredentialsPage);
        LinearLayout.LayoutParams credentialsLayout = new LinearLayout.LayoutParams(-1, -2);
        credentialsLayout.topMargin = dp(12);
        body.addView(credentialsButton, credentialsLayout);
        credentialErrorValue = text("", fontSize * .78f,
            darkTheme() ? Color.rgb(255, 125, 130) : Color.rgb(216, 55, 65));
        credentialErrorValue.setPadding(dp(4), dp(6), dp(4), 0);
        credentialErrorValue.setVisibility(View.GONE);
        body.addView(credentialErrorValue, new LinearLayout.LayoutParams(-1, -2));
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

    private View buildCredentialsPage() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true); scroll.setBackgroundColor(background);
        credentialsPageBody = column();
        credentialsPageBody.setPadding(dp(16), dp(10), dp(16), dp(20));
        scroll.addView(credentialsPageBody);
        return scroll;
    }
    private void openCredentialsPage() {
        if (!ready || session == null) return;
        settings = false; internalPage = InternalPage.CREDENTIALS;
        revealedCredentialIds.clear(); hideKeyboard();
        renderCredentialsPage(); render();
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
            addHistoryRows(history);
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
        if (internalPage == InternalPage.CREDENTIALS) renderCredentialsPage();
    }

    private void addHistoryRows(List<BrowserPreferences.HistoryEntry> entries) {
        LocalDate today = LocalDate.now(), currentDate = null;
        DateTimeFormatter formatter = DateTimeFormatter.ofPattern("yyyy年M月d日", Locale.getDefault());
        for (BrowserPreferences.HistoryEntry entry : entries) {
            LocalDate visitedDate = Instant.ofEpochMilli(entry.visitedAtMs)
                .atZone(ZoneId.systemDefault()).toLocalDate();
            if (!visitedDate.equals(currentDate)) {
                currentDate = visitedDate;
                String title = visitedDate.equals(today) ? "今天"
                    : visitedDate.equals(today.minusDays(1)) ? "昨天" : visitedDate.format(formatter);
                TextView heading = text(title, fontSize * .82f, muted);
                heading.setTypeface(null, Typeface.BOLD);
                heading.setPadding(dp(8), dp(12), dp(8), dp(2));
                historyPageBody.addView(heading);
            }
            addRecordRow(historyPageBody, entry.title, entry.url,
                () -> navigateSaved(entry.url), () -> session.removeHistory(entry.url), 12);
        }
    }
    private void renderCredentialsPage() {
        if (session == null || credentialsPageBody == null) return;
        credentialsPageBody.removeAllViews();
        String credentialError = session.credentialError();
        if (!credentialError.isEmpty()) {
            TextView warning = text(credentialError, fontSize * .82f,
                darkTheme() ? Color.rgb(255, 125, 130) : Color.rgb(216, 55, 65));
            warning.setPadding(0, 0, 0, dp(10));
            credentialsPageBody.addView(warning);
        }
        List<BrowserCredentialStore.Entry> entries;
        try { entries = session.credentials(); }
        catch (RuntimeException failure) {
            credentialsPageBody.addView(emptyState(failure.getMessage())); return;
        }
        if (entries.isEmpty()) {
            if (credentialError.isEmpty()) credentialsPageBody.addView(emptyState("暂无账号密码记录"));
            return;
        }
        for (BrowserCredentialStore.Entry entry : entries) addCredentialCard(entry);
    }
    private void addCredentialCard(BrowserCredentialStore.Entry entry) {
        LinearLayout card = column();
        card.setPadding(dp(12), dp(10), dp(12), dp(10));
        card.setBackground(rounded(surface, line, 10));
        TextView origin = text(displayAddress(entry.origin), fontSize * .92f, foreground);
        origin.setTypeface(null, Typeface.BOLD); origin.setSingleLine(true);
        origin.setEllipsize(TextUtils.TruncateAt.END); card.addView(origin);
        TextView account = text("账号：" + (entry.username.isEmpty() ? "—" : entry.username),
            fontSize * .82f, foreground);
        account.setSingleLine(true); account.setEllipsize(TextUtils.TruncateAt.END);
        card.addView(account);
        if (!entry.subaccount.isEmpty()) {
            TextView subaccount = text("子账号：" + entry.subaccount, fontSize * .82f, foreground);
            subaccount.setSingleLine(true); subaccount.setEllipsize(TextUtils.TruncateAt.END);
            card.addView(subaccount);
        }
        boolean revealed = revealedCredentialIds.contains(entry.id);
        LinearLayout passwordRow = row();
        TextView password = text("密码：" + (revealed ? entry.password : "••••••••"),
            fontSize * .82f, revealed ? foreground : muted);
        password.setSingleLine(true); password.setEllipsize(TextUtils.TruncateAt.END);
        passwordRow.addView(password, new LinearLayout.LayoutParams(0, -2, 1));
        Button reveal = textButton(revealed ? "隐藏" : "显示", () -> {
            if (revealed) revealedCredentialIds.remove(entry.id);
            else revealedCredentialIds.add(entry.id);
            renderCredentialsPage();
        });
        reveal.setMinimumWidth(0); reveal.setMinimumHeight(dp(36));
        passwordRow.addView(reveal, new LinearLayout.LayoutParams(-2, dp(40)));
        ImageButton delete = icon(R.drawable.browser_delete, "删除账号密码记录", () -> {
            session.deleteCredential(entry.id); revealedCredentialIds.remove(entry.id);
            renderCredentialsPage();
        });
        passwordRow.addView(delete, new LinearLayout.LayoutParams(dp(40), dp(40)));
        card.addView(passwordRow);
        LinearLayout.LayoutParams layout = new LinearLayout.LayoutParams(-1, -2);
        layout.topMargin = dp(8); credentialsPageBody.addView(card, layout);
    }
    private void renderDiagnosticsPage() {
        if (session == null || diagnosticsPageBody == null) return;
        diagnosticsPageBody.removeAllViews();
        List<BrowserSession.DiagnosticEvent> allEvents = new ArrayList<>();
        for (BrowserSession.DiagnosticEvent event : session.diagnosticEvents()) {
            if ("request".equals(event.phase) && !"resource-cancelled".equals(event.status))
                allEvents.add(event);
        }

        int requestCount = 0, responseCount = 0, errorCount = 0;
        for (BrowserSession.DiagnosticEvent event : allEvents) {
            if (event.failure) errorCount++;
            else if (event.response) responseCount++;
            else requestCount++;
        }

        LinearLayout categoryBar = row();
        categoryBar.setPadding(dp(2), dp(4), dp(2), dp(4));
        categoryBar.setBackground(rounded(surface, Color.TRANSPARENT, 6));
        addDiagnosticsChip(categoryBar, "全部  " + allEvents.size(), "all");
        addDiagnosticsChip(categoryBar, "请求  " + requestCount, "request");
        addDiagnosticsChip(categoryBar, "响应  " + responseCount, "response");
        addDiagnosticsChip(categoryBar, "错误  " + errorCount, "error");
        diagnosticsPageBody.addView(categoryBar, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout filterBar = row();
        filterBar.setPadding(0, 0, 0, dp(4));
        addDiagnosticsFilterChip(filterBar, "全部", "all");
        addDiagnosticsFilterChip(filterBar, "Fetch/XHR", "xhr");
        addDiagnosticsFilterChip(filterBar, "CSS", "css");
        addDiagnosticsFilterChip(filterBar, "JS", "js");
        addDiagnosticsFilterChip(filterBar, "媒体", "media");
        LinearLayout.LayoutParams filterBarParams = new LinearLayout.LayoutParams(-1, -2);
        filterBarParams.topMargin = dp(8);
        diagnosticsPageBody.addView(filterBar, filterBarParams);

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
        chip.setTextColor(selected ? onAccent() : foreground);
        chip.setGravity(Gravity.CENTER);
        chip.setMinHeight(0);
        chip.setMinimumHeight(dp(30));
        chip.setPadding(dp(5), 0, dp(5), 0);
        chip.setBackground(rounded(selected ? accent : Color.TRANSPARENT,
            selected ? accent : Color.TRANSPARENT, 6));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(30), 1);
        params.leftMargin = dp(1);
        params.rightMargin = dp(1);
        container.addView(chip, params);
    }

    private void addDiagnosticsFilterChip(LinearLayout container, String label, String filter) {
        boolean selected = diagnosticsFilter.equals(filter);
        Button chip = textButton(label,
            () -> { diagnosticsFilter = filter; renderDiagnosticsPage(); });
        chip.setTextSize(fontSize * .70f);
        chip.setTextColor(selected ? accent : muted);
        chip.setGravity(Gravity.CENTER);
        chip.setMinHeight(0);
        chip.setMinimumHeight(dp(28));
        chip.setPadding(dp(4), 0, dp(4), 0);
        chip.setBackground(rounded(selected ? accentWithAlpha(darkTheme() ? 45 : 20) : Color.TRANSPARENT,
            selected ? accent : Color.TRANSPARENT, 6));
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(28), 1);
        params.leftMargin = dp(1);
        params.rightMargin = dp(1);
        container.addView(chip, params);
    }

    private boolean diagnosticsMatches(BrowserSession.DiagnosticEvent event) {
        if ("error".equals(diagnosticsCategory) && !event.failure) return false;
        if ("request".equals(diagnosticsCategory) && (event.response || event.failure)) return false;
        if ("response".equals(diagnosticsCategory) && (!event.response || event.failure)) return false;
        String type = event.resourceType == null ? "" : event.resourceType.toLowerCase(Locale.ROOT);
        if ("xhr".equals(diagnosticsFilter) && !supportsDiagnosticBodies(event)) return false;
        if ("css".equals(diagnosticsFilter)
            && !("css".equals(type) || "stylesheet".equals(type) || "stylesheets".equals(type))) return false;
        if ("js".equals(diagnosticsFilter)
            && !("script".equals(type) || "javascript".equals(type) || "js".equals(type))) return false;
        if ("media".equals(diagnosticsFilter)
            && !("image".equals(type) || "img".equals(type) || "media".equals(type)
                || "audio".equals(type) || "video".equals(type) || "font".equals(type))) return false;
        return true;
    }

    private void addDiagnosticsCard(BrowserSession.DiagnosticEvent event, long maxDuration) {
        LinearLayout card = column();
        card.setPadding(dp(10), dp(8), dp(10), dp(8));
        card.setBackground(rounded(surface, line, 12));
        card.setOnClickListener(ignored -> {
            haptics.perform(card, "haptic-click");
            expandedDiagnosticId = expandedDiagnosticId == event.id ? -1L : event.id;
            renderDiagnosticsPage();
        });

        LinearLayout top = row();
        TextView method = text(event.method == null || event.method.isEmpty() ? "GET" : event.method,
            fontSize * .68f, onAccent());
        method.setGravity(Gravity.CENTER);
        method.setTypeface(null, Typeface.BOLD);
        method.setBackground(rounded(accent, Color.TRANSPARENT, 6));
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
        status.setPadding(0, 0, 0, 0);
        status.setBackground(rounded(diagnosticStatusColor(event, false),
            diagnosticStatusColor(event, false), 10));
        top.addView(status, new LinearLayout.LayoutParams(dp(48), dp(24)));
        TextView duration = text(durationLabel(event.durationMs), fontSize * .62f, muted);
        duration.setGravity(Gravity.CENTER);
        duration.setSingleLine(true);
        top.addView(duration, new LinearLayout.LayoutParams(dp(56), dp(26)));
        TextView size = text(sizeLabel(event.sizeBytes), fontSize * .62f, muted);
        size.setGravity(Gravity.CENTER);
        size.setSingleLine(true);
        top.addView(size, new LinearLayout.LayoutParams(dp(48), dp(26)));
        ImageView arrow = new ImageView(this);
        arrow.setImageResource(R.drawable.ic_chevron_down);
        arrow.setImageTintList(ColorStateList.valueOf(muted));
        arrow.setRotation(expandedDiagnosticId == event.id ? 180f : 0f);
        arrow.setScaleType(ImageView.ScaleType.CENTER);
        arrow.setContentDescription(expandedDiagnosticId == event.id ? "收起请求详情" : "展开请求详情");
        top.addView(arrow, new LinearLayout.LayoutParams(dp(24), dp(26)));
        card.addView(top, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout bar = row();
        bar.setGravity(Gravity.CENTER_VERTICAL);
        bar.setBackground(rounded(line, Color.TRANSPARENT, 3));
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
            int detailBackground = event.failure
                ? darkTheme() ? Color.argb(32, 216, 55, 65) : Color.rgb(255, 240, 241) : surface;
            int detailLine = event.failure
                ? darkTheme() ? Color.argb(100, 216, 55, 65) : Color.rgb(255, 218, 220) : line;
            details.setBackground(rounded(detailBackground, detailLine, 9));
            TextView title = text(event.failure
                ? "!  " + (event.detail.isEmpty() ? "请求失败" : event.detail)
                : "请求详情", fontSize * .76f,
                event.failure ? diagnosticStatusColor(event, true) : foreground);
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
            if (supportsDiagnosticBodies(event)) addDiagnosticBodyPager(details, event);
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

    private void addDiagnosticBodyPager(LinearLayout container, BrowserSession.DiagnosticEvent event) {
        HorizontalScrollView requestPage = diagnosticBodyPage(event.requestBody, "", "暂无请求数据");
        HorizontalScrollView responsePage = diagnosticBodyPage(event.responseBody, event.responseMime, "暂无接收数据");
        LinearLayout body = column();
        body.addView(requestPage, new LinearLayout.LayoutParams(-1, -2));
        body.addView(responsePage, new LinearLayout.LayoutParams(-1, -2));

        TextView requestLabel = diagnosticBodyLabel("请求数据");
        TextView responseLabel = diagnosticBodyLabel("接收数据");
        View requestIndicator = new View(this), responseIndicator = new View(this);
        LinearLayout requestTab = diagnosticBodyTab(requestLabel, requestIndicator, () ->
            selectDiagnosticBody(body, requestPage, responsePage, requestLabel, responseLabel,
                requestIndicator, responseIndicator, event.id, false, true));
        LinearLayout responseTab = diagnosticBodyTab(responseLabel, responseIndicator, () ->
            selectDiagnosticBody(body, requestPage, responsePage, requestLabel, responseLabel,
                requestIndicator, responseIndicator, event.id, true, true));
        LinearLayout tabs = row();
        tabs.addView(requestTab);
        LinearLayout.LayoutParams responseTabParams = new LinearLayout.LayoutParams(-2, -2);
        responseTabParams.leftMargin = dp(16);
        tabs.addView(responseTab, responseTabParams);
        LinearLayout.LayoutParams tabsParams = new LinearLayout.LayoutParams(-1, -2);
        tabsParams.topMargin = dp(6);
        container.addView(tabs, tabsParams);
        LinearLayout.LayoutParams bodyParams = new LinearLayout.LayoutParams(-1, -2);
        bodyParams.topMargin = dp(4);
        container.addView(body, bodyParams);
        selectDiagnosticBody(body, requestPage, responsePage, requestLabel, responseLabel,
            requestIndicator, responseIndicator, event.id, diagnosticBodyPages.getOrDefault(event.id, 0) == 1, false);
    }

    private void selectDiagnosticBody(LinearLayout body, HorizontalScrollView requestPage,
        HorizontalScrollView responsePage, TextView requestLabel, TextView responseLabel,
        View requestIndicator, View responseIndicator, long eventId, boolean response, boolean animate) {
        diagnosticBodyPages.put(eventId, response ? 1 : 0);
        requestLabel.setTextColor(response ? muted : accent);
        responseLabel.setTextColor(response ? accent : muted);
        requestIndicator.setBackgroundColor(response ? Color.TRANSPARENT : accent);
        responseIndicator.setBackgroundColor(response ? accent : Color.TRANSPARENT);
        HorizontalScrollView showing = response ? responsePage : requestPage;
        HorizontalScrollView hiding = response ? requestPage : responsePage;
        if (showing.getVisibility() == View.VISIBLE && hiding.getVisibility() == View.GONE) return;
        if (body.getTag() instanceof ValueAnimator previous) previous.cancel();
        int start = body.getHeight(), width = body.getWidth();
        requestPage.setVisibility(response ? View.GONE : View.VISIBLE);
        responsePage.setVisibility(response ? View.VISIBLE : View.GONE);
        if (!animate || !ValueAnimator.areAnimatorsEnabled() || width <= 0) {
            ViewGroup.LayoutParams current = body.getLayoutParams();
            current.height = ViewGroup.LayoutParams.WRAP_CONTENT;
            body.setLayoutParams(current);
            return;
        }
        showing.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED));
        int target = showing.getMeasuredHeight();
        ViewGroup.LayoutParams params = body.getLayoutParams();
        params.height = Math.max(0, start);
        body.setLayoutParams(params);
        ValueAnimator animator = ValueAnimator.ofInt(Math.max(0, start), target);
        animator.setDuration(240L);
        animator.addUpdateListener(animation -> {
            params.height = (Integer) animation.getAnimatedValue();
            body.setLayoutParams(params);
        });
        animator.addListener(new android.animation.AnimatorListenerAdapter() {
            @Override public void onAnimationEnd(android.animation.Animator animation) {
                if (body.getTag() != animator) return;
                body.setTag(null);
                params.height = ViewGroup.LayoutParams.WRAP_CONTENT;
                body.setLayoutParams(params);
            }
        });
        body.setTag(animator);
        animator.start();
    }

    private TextView diagnosticBodyLabel(String label) {
        TextView view = text(label, fontSize * .68f, muted);
        view.setGravity(Gravity.CENTER);
        view.setMinHeight(dp(28));
        view.setPadding(dp(8), dp(2), dp(8), dp(2));
        return view;
    }

    private LinearLayout diagnosticBodyTab(TextView label, View indicator, Runnable action) {
        LinearLayout tab = column();
        tab.addView(label, new LinearLayout.LayoutParams(-2, -2));
        tab.addView(indicator, new LinearLayout.LayoutParams(-1, dp(2)));
        tab.setOnClickListener(ignored -> {
            haptics.perform(tab, "haptic-click");
            action.run();
        });
        return tab;
    }

    private boolean supportsDiagnosticBodies(BrowserSession.DiagnosticEvent event) {
        String type = event.resourceType == null ? "" : event.resourceType.toLowerCase(Locale.ROOT);
        return "xhr".equals(type) || "fetch".equals(type) || "xmlhttprequest".equals(type)
            || !event.requestBody.isEmpty() || !event.responseBody.isEmpty();
    }

    private HorizontalScrollView diagnosticBodyPage(String value, String mime, String empty) {
        String text = value == null || value.isEmpty() ? empty : formatDiagnosticBody(value, mime);
        HorizontalScrollView scroll = new HorizontalScrollView(this);
        scroll.setFillViewport(true);
        scroll.setHorizontalScrollBarEnabled(true);
        scroll.setVerticalScrollBarEnabled(false);
        TextView content = text(text, fontSize * .68f, value == null || value.isEmpty() ? muted : foreground);
        content.setTypeface(Typeface.MONOSPACE);
        content.setTextIsSelectable(true);
        content.setSingleLine(false);
        content.setHorizontallyScrolling(true);
        content.setMaxLines(Integer.MAX_VALUE);
        content.setEllipsize(null);
        content.setIncludeFontPadding(false);
        content.setMinimumHeight(dp(72));
        content.setPadding(dp(8), dp(8), dp(8), dp(8));
        content.setBackground(rounded(background, line, 6));
        scroll.addView(content, new HorizontalScrollView.LayoutParams(-2, -2));
        return scroll;
    }

    private String formatDiagnosticBody(String value, String mime) {
        if (value == null) return "";
        String trimmed = value.trim();
        if (trimmed.isEmpty()) return "";
        boolean json = mime != null && mime.toLowerCase(Locale.ROOT).contains("json")
            || trimmed.startsWith("{") || trimmed.startsWith("[");
        if (!json) return value;
        try {
            Object parsed = new JSONTokener(trimmed).nextValue();
            if (parsed instanceof JSONObject) return ((JSONObject) parsed).toString(2);
            if (parsed instanceof JSONArray) return ((JSONArray) parsed).toString(2);
        } catch (JSONException | RuntimeException ignored) { }
        return value;
    }

    private String diagnosticStatus(BrowserSession.DiagnosticEvent event) {
        if (event.failure) return event.statusCode > 0 ? String.valueOf(event.statusCode) : "失败";
        if (event.statusCode > 0) return String.valueOf(event.statusCode);
        return event.response ? "完成" : "请求";
    }

    private int diagnosticStatusColor(BrowserSession.DiagnosticEvent event, boolean textColor) {
        if (event.failure) return textColor
            ? darkTheme() ? Color.rgb(255, 125, 130) : Color.rgb(216, 55, 65)
            : darkTheme() ? Color.argb(55, 216, 55, 65) : Color.rgb(255, 210, 214);
        if (event.statusCode >= 300 && event.statusCode < 400)
            return textColor ? muted : darkTheme() ? Color.argb(60, 113, 113, 113) : Color.rgb(235, 237, 241);
        if (event.response) return textColor
            ? darkTheme() ? Color.rgb(80, 210, 140) : Color.rgb(35, 160, 91)
            : darkTheme() ? Color.argb(55, 35, 160, 91) : Color.rgb(205, 242, 216);
        return textColor ? accent : accentWithAlpha(darkTheme() ? 55 : 32);
    }

    private int accentWithAlpha(int alpha) {
        return Color.argb(alpha, Color.red(accent), Color.green(accent), Color.blue(accent));
    }

    private int onAccent() {
        double brightness = Color.red(accent) * .299 + Color.green(accent) * .587 + Color.blue(accent) * .114;
        return brightness > 150 ? Color.rgb(23, 23, 23) : Color.WHITE;
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
        if (durationMs <= 0) return "--";
        if (durationMs < 1000) return durationMs + "ms";
        if (durationMs < 60_000) return compactDecimal(durationMs / 1000d) + "s";
        if (durationMs < 3_600_000) return compactDecimal(durationMs / 60_000d) + "m";
        return compactDecimal(durationMs / 3_600_000d) + "h";
    }

    private String compactDecimal(double value) {
        if (value >= 100) return String.format(Locale.ROOT, "%.0f", value);
        if (value >= 10) return String.format(Locale.ROOT, "%.1f", value)
            .replaceAll("\\.0$", "");
        return String.format(Locale.ROOT, "%.1f", value)
            .replaceAll("\\.0$", "");
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

    private void renderSettings() {
        if (session == null) return;
        String selectedUserAgent = userAgentLabel(session.userAgentMode());
        userAgentValue.setText(selectedUserAgent);
        userAgentRow.setContentDescription("浏览器标识（UA），当前为" + selectedUserAgent);
        credentialSwitchSyncing = true;
        credentialRecordingSwitch.setChecked(session.credentialRecordingEnabled());
        credentialSwitchSyncing = false;
        try { credentialsButton.setText("账号密码记录（" + session.credentials().size() + "）"); }
        catch (RuntimeException failure) { credentialsButton.setText("账号密码记录（读取失败）"); }
        String credentialError = session.credentialError();
        credentialErrorValue.setText(credentialError);
        credentialErrorValue.setVisibility(credentialError.isEmpty() ? View.GONE : View.VISIBLE);
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
            mode.equals(current) ? onAccent() : muted);
        indicator.setGravity(Gravity.CENTER);
        indicator.setTypeface(null, Typeface.BOLD);
        indicator.setBackground(rounded(mode.equals(current) ? accent : Color.TRANSPARENT,
            mode.equals(current) ? accent : muted, 3));
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
        else if (internalPage == InternalPage.CREDENTIALS) confirmClearCredentials();
        else if (internalPage == InternalPage.DIAGNOSTICS && session != null) {
            diagnosticBodyPages.clear();
            session.clearDiagnosticEvents();
            renderDiagnosticsPage();
            Toast.makeText(this, "网络记录已清空", Toast.LENGTH_SHORT).show();
        }
    }

    private void confirmClearCredentials() {
        if (!ready || session == null) return;
        AlertDialog alert = new AlertDialog.Builder(this)
            .setTitle("清空账号密码记录")
            .setMessage("确定删除当前来源的全部账号密码记录吗？此操作不可恢复。")
            .setNegativeButton("暂不清空", null)
            .setPositiveButton("清空", (dialog, which) -> {
                try {
                    session.clearCredentials(); revealedCredentialIds.clear();
                    Toast.makeText(this, "账号密码记录已清空", Toast.LENGTH_SHORT).show();
                } catch (RuntimeException failure) {
                    Toast.makeText(this, failure.getMessage(), Toast.LENGTH_SHORT).show();
                }
                renderCredentialsPage();
            })
            .create();
        showDialog(alert);
    }
    private void confirmClearHistory() {
        if (!ready || session == null) return;
        AlertDialog alert = new AlertDialog.Builder(this)
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
            .create();
        showDialog(alert);
    }

    private void confirmClearBookmarks() {
        if (!ready || session == null) return;
        AlertDialog alert = new AlertDialog.Builder(this)
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
            .create();
        showDialog(alert);
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
            : internalPage == InternalPage.DIAGNOSTICS ? "网络"
            : internalPage == InternalPage.CREDENTIALS ? "账号密码" : "浏览器");
        homePage.setVisibility(showHome ? View.VISIBLE : View.GONE);
        webContainer.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        settingsPage.setVisibility(settings ? View.VISIBLE : View.GONE);
        historyPage.setVisibility(!settings && internalPage == InternalPage.HISTORY ? View.VISIBLE : View.GONE);
        bookmarksPage.setVisibility(!settings && internalPage == InternalPage.BOOKMARKS ? View.VISIBLE : View.GONE);
        diagnosticsPage.setVisibility(!settings && internalPage == InternalPage.DIAGNOSTICS ? View.VISIBLE : View.GONE);
        credentialsPage.setVisibility(!settings && internalPage == InternalPage.CREDENTIALS ? View.VISIBLE : View.GONE);
        boolean diagnostics = internalPage == InternalPage.DIAGNOSTICS;
        header.setVisibility(View.VISIBLE);
        headerDivider.setVisibility(View.VISIBLE);
        footer.setVisibility(settings ? View.GONE : View.VISIBLE);
        footerDivider.setVisibility(settings ? View.GONE : View.VISIBLE);
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
        networkButton.setVisibility(showHome ? View.VISIBLE : View.GONE);
        headerNetworkButton.setVisibility(showWeb ? View.VISIBLE : View.GONE);
        clearPageButton.setVisibility(internal ? View.VISIBLE : View.GONE);
        clearPageButton.setEnabled(ready && internal);
        settingsButton.setEnabled(ready);
        credentialsButton.setEnabled(ready); credentialRecordingSwitch.setEnabled(ready);
        homeInput.setEnabled(ready && showHome);
        addressInput.setEnabled(ready && showWeb);
        boolean settingsReady = ready && settings;
        userAgentRow.setEnabled(settingsReady);
        userAgentRow.setAlpha(settingsReady ? 1f : .55f);
        progress.setVisibility(diagnostics ? View.GONE
            : ready && showWeb && session.progress < 100 ? View.VISIBLE : View.INVISIBLE);
        if (session != null) {
            progress.setProgress(session.progress);
            if (showWeb && !addressInput.hasFocus()) addressInput.setText(displayAddress(session.url));
            bookmarkButton.setCompoundDrawableTintList(ColorStateList.valueOf(
                session.bookmarked() ? accent : foreground));
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
        addRecordRow(container, title, target, open, remove, 0);
    }
    private void addRecordRow(LinearLayout container, String title, String target,
        Runnable open, Runnable remove, int indentationDp) {
        LinearLayout row = row();
        row.setMinimumHeight(dp(40));
        row.setPadding(dp(8 + indentationDp), dp(1), dp(2), dp(1));
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
        String localFile = BrowserRoutePolicy.workspaceViewDisplay(address);
        if (!localFile.isEmpty()) return localFile;
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
        credentialsButton.setEnabled(false); credentialRecordingSwitch.setEnabled(false);
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
            .setPositiveButton("登录", (dialog, which) -> {
                String user = username.getText().toString(), secret = password.getText().toString();
                if (session != null) session.recordHttpCredentials(user, secret);
                handler.proceed(user, secret);
            })
            .setNegativeButton("取消", (dialog, which) -> handler.cancel())
            .setOnCancelListener(dialog -> handler.cancel()).create();
        showDialog(authDialog);
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
    private void showDialog(AlertDialog dialog) {
        dialog.show();
        Button positive = dialog.getButton(AlertDialog.BUTTON_POSITIVE);
        if (positive != null) positive.setTextColor(accent);
        Button negative = dialog.getButton(AlertDialog.BUTTON_NEGATIVE);
        if (negative != null) negative.setTextColor(foreground);
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
        view.setTextColor(onAccent());
        view.setMinimumHeight(dp(48));
        view.setBackground(rounded(accent, accent, 10));
        return view;
    }
    private Button footerAction(int resource, String label, String description, Runnable action) {
        Button view = textButton(label, action);
        view.setTextSize(fontSize * .70f);
        view.setSingleLine(true);
        view.setIncludeFontPadding(false);
        view.setGravity(Gravity.CENTER);
        view.setMinWidth(0); view.setMinimumWidth(0);
        view.setMinHeight(0); view.setMinimumHeight(0);
        view.setPadding(dp(2), dp(2), dp(2), dp(1));
        view.setCompoundDrawablesWithIntrinsicBounds(0, resource, 0, 0);
        view.setCompoundDrawablePadding(0);
        view.setCompoundDrawableTintList(ColorStateList.valueOf(foreground));
        view.setContentDescription(description);
        view.setLayoutParams(new LinearLayout.LayoutParams(dp(44), dp(44)));
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
    private boolean darkTheme() {
        return Color.red(background) * 299 + Color.green(background) * 587
            + Color.blue(background) * 114 <= 128000;
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
