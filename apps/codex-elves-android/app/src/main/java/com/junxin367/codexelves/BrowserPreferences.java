package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;
import android.net.Uri;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashSet;
import java.util.List;

final class BrowserPreferences {
    static final int HISTORY_LIMIT = 200;
    static final int BOOKMARK_LIMIT = 100;
    static final String UA_ANDROID = "android";
    static final String UA_DESKTOP = "desktop";
    static final String UA_IPHONE = "iphone";
    static final class HistoryEntry {
        final String url;
        final String title;
        final long visitedAtMs;

        HistoryEntry(String url, String title, long visitedAtMs) {
            this.url = url;
            this.title = title;
            this.visitedAtMs = visitedAtMs;
        }
    }

    static final class BookmarkEntry {
        final String url;
        final String title;
        final long savedAtMs;

        BookmarkEntry(String url, String title, long savedAtMs) {
            this.url = url;
            this.title = title;
            this.savedAtMs = savedAtMs;
        }
    }

    private static final String HISTORY_PREFIX = "history:";
    private static final String BOOKMARK_PREFIX = "bookmark:";
    private static final String DISPLAY_MODE_PREFIX = "display-mode:";
    private final SharedPreferences store;

    BrowserPreferences(Context context) {
        store = context.getSharedPreferences("codex_elves_browser_routes", Context.MODE_PRIVATE);
    }

    List<HistoryEntry> loadHistory(String origin) {
        return loadHistory(origin, HISTORY_LIMIT);
    }

    List<HistoryEntry> loadHistory(String origin, int limit) {
        List<HistoryEntry> result = new ArrayList<>();
        try {
            JSONArray values = new JSONArray(store.getString(HISTORY_PREFIX + origin, "[]"));
            for (int i = 0; i < values.length() && result.size() < HISTORY_LIMIT; i++) {
                JSONObject value = values.optJSONObject(i);
                if (value == null) continue;
                String url = value.optString("url", "").trim();
                long visitedAtMs = value.optLong("visitedAtMs", 0L);
                if (!isWebUrl(url) || visitedAtMs <= 0L) continue;
                result.add(new HistoryEntry(url, cleanTitle(value.optString("title", ""), url), visitedAtMs));
            }
        } catch (JSONException ignored) {
            /* A damaged preference is ignored and replaced on the next visit. */
        }
        result.sort(Comparator.comparingLong((HistoryEntry entry) -> entry.visitedAtMs).reversed());
        return new ArrayList<>(uniqueHistory(result, Math.max(0, Math.min(limit, HISTORY_LIMIT))));
    }

    void recordHistory(String origin, String url, String title, long visitedAtMs) {
        if (!isWebUrl(url)) return;
        List<HistoryEntry> values = loadHistory(origin);
        values.removeIf(entry -> entry.url.equals(url));
        values.add(0, new HistoryEntry(url, cleanTitle(title, url), Math.max(1L, visitedAtMs)));
        writeHistory(origin, values);
    }

    void clearHistory(String origin) {
        if (!store.edit().remove(HISTORY_PREFIX + origin).commit())
            throw new IllegalStateException("访问历史清除失败，请重试");
    }

    boolean removeHistory(String origin, String url) {
        List<HistoryEntry> values = loadHistory(origin);
        boolean removed = values.removeIf(entry -> entry.url.equals(url));
        if (removed) writeHistory(origin, values);
        return removed;
    }

    List<BookmarkEntry> loadBookmarks(String origin) {
        List<BookmarkEntry> result = new ArrayList<>();
        try {
            JSONArray values = new JSONArray(store.getString(BOOKMARK_PREFIX + origin, "[]"));
            for (int i = 0; i < values.length() && result.size() < BOOKMARK_LIMIT; i++) {
                JSONObject value = values.optJSONObject(i);
                if (value == null) continue;
                String url = value.optString("url", "").trim();
                long savedAtMs = value.optLong("savedAtMs", 0L);
                if (!isWebUrl(url) || savedAtMs <= 0L) continue;
                result.add(new BookmarkEntry(url, cleanTitle(value.optString("title", ""), url), savedAtMs));
            }
        } catch (JSONException ignored) {
            /* A damaged preference is ignored and replaced when a bookmark changes. */
        }
        result.sort(Comparator.comparingLong((BookmarkEntry entry) -> entry.savedAtMs).reversed());
        return new ArrayList<>(uniqueBookmarks(result));
    }

    boolean isBookmarked(String origin, String url) {
        if (!isWebUrl(url)) return false;
        for (BookmarkEntry entry : loadBookmarks(origin)) {
            if (entry.url.equals(url)) return true;
        }
        return false;
    }

    boolean toggleBookmark(String origin, String url, String title, long savedAtMs) {
        if (!isWebUrl(url)) return false;
        List<BookmarkEntry> values = loadBookmarks(origin);
        boolean removed = values.removeIf(entry -> entry.url.equals(url));
        if (!removed) {
            values.add(0, new BookmarkEntry(url, cleanTitle(title, url), Math.max(1L, savedAtMs)));
            if (values.size() > BOOKMARK_LIMIT)
                values = new ArrayList<>(values.subList(0, BOOKMARK_LIMIT));
        }
        writeBookmarks(origin, values);
        return !removed;
    }

    boolean removeBookmark(String origin, String url) {
        List<BookmarkEntry> values = loadBookmarks(origin);
        boolean removed = values.removeIf(entry -> entry.url.equals(url));
        if (removed) writeBookmarks(origin, values);
        return removed;
    }

    void clearBookmarks(String origin) {
        if (!store.edit().remove(BOOKMARK_PREFIX + origin).commit())
            throw new IllegalStateException("书签清除失败，请重试");
    }

    String loadUserAgentMode(String origin) {
        String mode = store.getString(DISPLAY_MODE_PREFIX + origin, UA_ANDROID);
        return UA_DESKTOP.equals(mode) || UA_IPHONE.equals(mode) ? mode : UA_ANDROID;
    }

    void saveUserAgentMode(String origin, String mode) {
        if (!UA_ANDROID.equals(mode) && !UA_DESKTOP.equals(mode) && !UA_IPHONE.equals(mode))
            throw new IllegalArgumentException("网站显示方式无效");
        if (!store.edit().putString(DISPLAY_MODE_PREFIX + origin, mode).commit())
            throw new IllegalStateException("网站显示方式保存失败，请重试");
    }

    private void writeHistory(String origin, List<HistoryEntry> values) {
        JSONArray result = new JSONArray();
        int count = 0;
        for (HistoryEntry entry : values) {
            if (!isWebUrl(entry.url)) continue;
            if (count >= HISTORY_LIMIT) break;
            JSONObject value = new JSONObject();
            try {
                value.put("url", entry.url);
                value.put("title", cleanTitle(entry.title, entry.url));
                value.put("visitedAtMs", Math.max(1L, entry.visitedAtMs));
                result.put(value);
                count++;
            } catch (JSONException invalid) {
                throw new IllegalStateException("访问历史保存失败，请重试", invalid);
            }
        }
        if (!store.edit().putString(HISTORY_PREFIX + origin, result.toString()).commit())
            throw new IllegalStateException("访问历史保存失败，请重试");
    }

    private void writeBookmarks(String origin, List<BookmarkEntry> values) {
        JSONArray result = new JSONArray();
        int count = 0;
        for (BookmarkEntry entry : values) {
            if (!isWebUrl(entry.url)) continue;
            if (count >= BOOKMARK_LIMIT) break;
            JSONObject value = new JSONObject();
            try {
                value.put("url", entry.url);
                value.put("title", cleanTitle(entry.title, entry.url));
                value.put("savedAtMs", Math.max(1L, entry.savedAtMs));
                result.put(value);
                count++;
            } catch (JSONException invalid) {
                throw new IllegalStateException("书签保存失败，请重试", invalid);
            }
        }
        if (!store.edit().putString(BOOKMARK_PREFIX + origin, result.toString()).commit())
            throw new IllegalStateException("书签保存失败，请重试");
    }

    private static List<HistoryEntry> uniqueHistory(List<HistoryEntry> values, int limit) {
        LinkedHashSet<String> seen = new LinkedHashSet<>();
        List<HistoryEntry> result = new ArrayList<>();
        for (HistoryEntry value : values) {
            if (seen.add(value.url)) {
                result.add(value);
                if (result.size() >= limit) break;
            }
        }
        return result;
    }

    private static List<BookmarkEntry> uniqueBookmarks(List<BookmarkEntry> values) {
        LinkedHashSet<String> seen = new LinkedHashSet<>();
        List<BookmarkEntry> result = new ArrayList<>();
        for (BookmarkEntry value : values) {
            if (seen.add(value.url)) result.add(value);
        }
        return result;
    }

    private static boolean isWebUrl(String value) {
        try {
            Uri uri = Uri.parse(value);
            return ("http".equalsIgnoreCase(uri.getScheme()) || "https".equalsIgnoreCase(uri.getScheme()))
                && uri.getHost() != null && !uri.getHost().isEmpty();
        } catch (RuntimeException invalid) {
            return false;
        }
    }

    private static String cleanTitle(String title, String url) {
        String value = title == null ? "" : title.trim();
        if (!value.isEmpty()) return value;
        try {
            String host = Uri.parse(url).getHost();
            if (host != null && !host.isEmpty()) return host;
        } catch (RuntimeException ignored) { }
        return url;
    }
}
