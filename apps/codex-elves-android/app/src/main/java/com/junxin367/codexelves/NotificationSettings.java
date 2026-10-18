package com.junxin367.codexelves;

import android.content.Context;
import android.content.SharedPreferences;

import org.json.JSONException;
import org.json.JSONObject;

import java.util.function.BooleanSupplier;

final class NotificationSettings {
    private static final String PREFERENCES = "codex_elves_notifications";
    private static final String SETTINGS = "settings";
    private static final String REQUESTED = "permission_requested";
    private static final String[] FIELDS = {
        "enabled", "approval", "question", "completed", "failed"
    };
    static final NotificationSettings DEFAULT =
        new NotificationSettings(false, true, true, true, true);
    private static final Object INITIALIZATION_LOCK = new Object();
    private static final Object WRITE_LOCK = new Object();
    private static volatile Confirmed confirmed;
    final boolean enabled;
    final boolean approval;
    final boolean question;
    final boolean completed;
    final boolean failed;

    NotificationSettings(
        boolean enabled, boolean approval, boolean question, boolean completed, boolean failed
    ) {
        this.enabled = enabled;
        this.approval = approval;
        this.question = question;
        this.completed = completed;
        this.failed = failed;
    }

    static NotificationSettings parse(JSONObject value) throws JSONException {
        if (value == null || value.length() != FIELDS.length) {
            throw new JSONException("Invalid notification settings");
        }
        for (String field : FIELDS) {
            if (!(value.opt(field) instanceof Boolean)) {
                throw new JSONException("Invalid notification setting");
            }
        }
        return new NotificationSettings(
            value.getBoolean("enabled"), value.getBoolean("approval"),
            value.getBoolean("question"), value.getBoolean("completed"), value.getBoolean("failed")
        );
    }

    JSONObject toJson() throws JSONException {
        return new JSONObject().put("enabled", enabled).put("approval", approval)
            .put("question", question).put("completed", completed).put("failed", failed);
    }

    boolean allows(String kind) {
        if (!enabled) {
            return false;
        }
        switch (kind) {
            case "approval": return approval;
            case "question": return question;
            case "completed": return completed;
            case "failed": return failed;
            default: return false;
        }
    }

    static boolean isRequestType(String type) {
        return "notification-settings-get".equals(type)
            || "notification-settings-set".equals(type)
            || "notification-permission-request".equals(type)
            || "notification-open-system-settings".equals(type);
    }

    static boolean validRequestId(Object requestId) {
        return requestId instanceof String
            && ((String) requestId).matches("[a-zA-Z0-9_-]{1,80}");
    }

    static boolean validEnvelope(JSONObject request) {
        Object type = request.opt("type");
        if (!(type instanceof String) || !isRequestType((String) type)
            || !validRequestId(request.opt("requestId"))) {
            return false;
        }
        return request.length() == ("notification-settings-set".equals(type) ? 3 : 2)
            && (!"notification-settings-set".equals(type)
                || request.opt("settings") instanceof JSONObject);
    }

    static JSONObject failure(String requestId, String error) throws JSONException {
        return new JSONObject().put("type", "notification-settings")
            .put("requestId", requestId).put("error", error);
    }

    static NotificationSettings read(Context context) {
        return confirmed(context).settings;
    }

    boolean save(Context context) {
        return save(context, WRITE_LOCK, () -> true);
    }

    boolean save(Context context, Object requestLock, BooleanSupplier currentRequest) {
        try {
            String serialized = toJson().toString();
            synchronized (WRITE_LOCK) {
                synchronized (requestLock) {
                    if (!currentRequest.getAsBoolean()) {
                        return false;
                    }
                }
                Confirmed previous = confirmed(context);
                SharedPreferences preferences = preferences(context);
                // Disk I/O must not hold the Activity's page/lifecycle lock.
                boolean saved = preferences.edit().putString(SETTINGS, serialized).commit();
                synchronized (requestLock) {
                    if (saved && currentRequest.getAsBoolean()) {
                        confirmed = new Confirmed(
                            this, serialized, previous.permissionRequested, previous.requestedPresent
                        );
                        return true;
                    }
                }
                restore(preferences, SETTINGS, previous.serializedSettings);
                return false;
            }
        } catch (JSONException error) {
            return false;
        }
    }

    static boolean wasPermissionRequested(Context context) {
        return confirmed(context).permissionRequested;
    }

    static boolean markPermissionRequested(Context context) {
        return markPermissionRequested(context, WRITE_LOCK, () -> true);
    }

    static boolean markPermissionRequested(
        Context context, Object requestLock, BooleanSupplier currentRequest
    ) {
        synchronized (WRITE_LOCK) {
            synchronized (requestLock) {
                if (!currentRequest.getAsBoolean()) {
                    return false;
                }
            }
            Confirmed previous = confirmed(context);
            SharedPreferences preferences = preferences(context);
            boolean saved = preferences.edit().putBoolean(REQUESTED, true).commit();
            synchronized (requestLock) {
                if (saved && currentRequest.getAsBoolean()) {
                    confirmed = new Confirmed(
                        previous.settings, previous.serializedSettings, true, true
                    );
                    return true;
                }
            }
            SharedPreferences.Editor rollback = preferences.edit();
            if (previous.requestedPresent) {
                rollback.putBoolean(REQUESTED, previous.permissionRequested);
            } else {
                rollback.remove(REQUESTED);
            }
            rollback.apply();
            return false;
        }
    }

    private static Confirmed confirmed(Context context) {
        Confirmed value = confirmed;
        if (value != null) {
            return value;
        }
        // Readers never acquire WRITE_LOCK or wait for an in-progress disk commit.
        synchronized (INITIALIZATION_LOCK) {
            value = confirmed;
            if (value == null) {
                SharedPreferences preferences = preferences(context);
                String serialized = null;
                NotificationSettings settings = DEFAULT;
                try {
                    serialized = preferences.getString(SETTINGS, null);
                    settings = parse(new JSONObject(serialized == null ? "{}" : serialized));
                } catch (JSONException | ClassCastException ignored) {
                    // Preserve the default behavior for missing or invalid legacy settings.
                }
                value = new Confirmed(
                    settings, serialized, preferences.getBoolean(REQUESTED, false),
                    preferences.contains(REQUESTED)
                );
                confirmed = value;
            }
        }
        return value;
    }

    private static void restore(SharedPreferences preferences, String key, String previous) {
        SharedPreferences.Editor rollback = preferences.edit();
        if (previous == null) {
            rollback.remove(key);
        } else {
            rollback.putString(key, previous);
        }
        rollback.apply();
    }

    private static final class Confirmed {
        final NotificationSettings settings;
        final String serializedSettings;
        final boolean permissionRequested;
        final boolean requestedPresent;

        Confirmed(
            NotificationSettings settings, String serializedSettings,
            boolean permissionRequested, boolean requestedPresent
        ) {
            this.settings = settings;
            this.serializedSettings = serializedSettings;
            this.permissionRequested = permissionRequested;
            this.requestedPresent = requestedPresent;
        }
    }

    private static SharedPreferences preferences(Context context) {
        return context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
    }
}
