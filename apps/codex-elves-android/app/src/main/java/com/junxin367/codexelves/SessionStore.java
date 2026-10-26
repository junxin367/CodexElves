package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;

final class SessionStore {
    private static final String PREFERENCES = "codex_elves_android";
    private static final String ORIGIN = "origin";
    private static final String AUTHORIZED = "authorized";
    private static final String DEVICE_COOKIE = "device_cookie";

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

    String getDeviceCookie() {
        return preferences.getString(DEVICE_COOKIE, "");
    }

    void saveOrigin(String origin) {
        preferences.edit().putString(ORIGIN, origin).apply();
    }

    void setAuthorized(boolean authorized) {
        SharedPreferences.Editor update = preferences.edit().putBoolean(AUTHORIZED, authorized);
        if (!authorized) {
            update.remove(DEVICE_COOKIE);
        }
        update.commit();
    }

    void saveAuthorizedConnection(String origin, String deviceCookie) {
        preferences.edit()
            .putString(ORIGIN, origin)
            .putString(DEVICE_COOKIE, deviceCookie)
            .putBoolean(AUTHORIZED, true)
            .commit();
    }

    void markAuthorized(String origin) {
        preferences.edit()
            .putString(ORIGIN, origin)
            .putBoolean(AUTHORIZED, true)
            .commit();
    }
}
