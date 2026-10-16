package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;

final class SessionStore {
    private static final String PREFERENCES = "codex_elves_android";
    private static final String ORIGIN = "origin";
    private static final String AUTHORIZED = "authorized";

    private final SharedPreferences preferences;

    SessionStore(Context context) {
        preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
    }

    String getOrigin() {
        return preferences.getString(ORIGIN, "");
    }

    boolean isAuthorized() {
        return preferences.getBoolean(AUTHORIZED, false);
    }

    void saveOrigin(String origin) {
        preferences.edit().putString(ORIGIN, origin).apply();
    }

    void setAuthorized(boolean authorized) {
        preferences.edit().putBoolean(AUTHORIZED, authorized).apply();
    }

    void saveAuthorizedConnection(String origin) {
        preferences.edit()
            .putString(ORIGIN, origin)
            .putBoolean(AUTHORIZED, true)
            .apply();
    }
}
