package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;
import org.json.JSONArray;
import org.json.JSONException;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;

final class BrowserPreferences {
    private final SharedPreferences store;

    BrowserPreferences(Context context) {
        store = context.getSharedPreferences("codex_elves_browser_routes", Context.MODE_PRIVATE);
    }

    List<String> load(String origin) {
        List<String> result = new ArrayList<>();
        try {
            JSONArray values = new JSONArray(store.getString(origin, "[]"));
            for (int i = 0; i < Math.min(values.length(), 100); i++) {
                try { result.add(BrowserRoutePolicy.normalizeDomain(values.getString(i))); }
                catch (IllegalArgumentException ignored) { /* Ignore obsolete invalid settings. */ }
            }
        } catch (JSONException ignored) { /* A damaged preference does not change the default route. */ }
        return new ArrayList<>(new LinkedHashSet<>(result));
    }

    void save(String origin, List<String> values) {
        if (values.size() > 100) throw new IllegalArgumentException("最多保存 100 个域名");
        LinkedHashSet<String> normalized = new LinkedHashSet<>();
        for (String value : values) normalized.add(BrowserRoutePolicy.normalizeDomain(value));
        if (!store.edit().putString(origin, new JSONArray(normalized).toString()).commit())
            throw new IllegalStateException("域名设置保存失败，请重试");
    }
}
