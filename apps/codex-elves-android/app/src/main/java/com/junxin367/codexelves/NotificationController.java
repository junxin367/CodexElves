package com.junxin367.codexelves;

import android.Manifest;
import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;

import org.json.JSONException;
import org.json.JSONObject;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.function.BooleanSupplier;
import java.util.function.Consumer;

/** Activity-scoped requests cannot complete into a replaced WebView or identity. */
final class NotificationController {
    private static final int PERMISSION_REQUEST = 4002;
    private final Activity activity;
    private final Consumer<String> authorizationLost;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final ExecutorService settingsWriter = Executors.newSingleThreadExecutor(
        runnable -> new Thread(runnable, "codex-elves-notification-settings")
    );
    private final Object pageLock = new Object();
    private final NotificationService.Listener stateListener = this::stateChanged;
    private long pageGeneration;
    private boolean destroyed;
    private boolean visible;
    private Request subscription;
    private Request permissionPending;
    private boolean permissionInFlight;

    NotificationController(Activity activity, Consumer<String> authorizationLost) {
        this.activity = activity;
        this.authorizationLost = authorizationLost;
        NotificationPublisher.createChannels(activity);
        NotificationService.addListener(stateListener);
    }

    void onPageChanged() {
        synchronized (pageLock) {
            pageGeneration++;
            subscription = null;
            permissionPending = null;
        }
    }

    void handle(
        JSONObject payload, BooleanSupplier currentPage, Consumer<String> reply
    ) {
        Object rawId = payload.opt("requestId");
        if (!NotificationSettings.validRequestId(rawId)) {
            return;
        }
        Request request;
        synchronized (pageLock) {
            request = new Request(
                (String) rawId, pageGeneration, currentPage, reply,
                NotificationService.currentIdentity(activity)
            );
            if (!request.current()) {
                return;
            }
            subscription = request;
        }
        if (!NotificationSettings.validEnvelope(payload)) {
            respond(request, "invalid-request");
            return;
        }
        String type = payload.optString("type");
        switch (type) {
            case "notification-settings-get":
                respond(request, null);
                break;
            case "notification-settings-set":
                final NotificationSettings settings;
                try {
                    settings = NotificationSettings.parse(payload.getJSONObject("settings"));
                } catch (JSONException error) {
                    respond(request, "invalid-settings");
                    return;
                }
                settingsWriter.execute(() -> {
                    boolean saved = settings.save(activity, pageLock, request::currentIdentity);
                    main.post(() -> {
                        if (!request.currentIdentity()) {
                            return;
                        }
                        if (saved) {
                            refreshDelivery();
                        }
                        respond(request, saved ? null : "settings-save-failed");
                        if (saved) {
                            emitChanged();
                        }
                    });
                });
                break;
            case "notification-permission-request":
                requestPermission(request);
                break;
            case "notification-open-system-settings":
                if (!visible || !request.currentIdentity()) {
                    respond(request, "activity-not-visible");
                    return;
                }
                try {
                    activity.startActivity(new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                        .putExtra(Settings.EXTRA_APP_PACKAGE, activity.getPackageName()));
                    respond(request, null);
                } catch (ActivityNotFoundException error) {
                    try {
                        activity.startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                            .setData(Uri.fromParts("package", activity.getPackageName(), null)));
                        respond(request, null);
                    } catch (ActivityNotFoundException unavailable) {
                        respond(request, "system-settings-unavailable");
                    }
                }
                break;
            default:
                break;
        }
    }

    void onResume() {
        visible = true;
        refreshDelivery();
        emitChanged();
    }

    void onPause() {
        visible = false;
    }

    void onAuthenticated() {
        refreshDelivery();
        emitChanged();
    }

    boolean onRequestPermissionsResult(int requestCode) {
        if (requestCode != PERMISSION_REQUEST) {
            return false;
        }
        Request request = permissionPending;
        permissionPending = null;
        permissionInFlight = false;
        refreshDelivery();
        if (request != null) {
            respond(request, null);
        }
        emitChanged();
        return true;
    }

    void onDestroy() {
        synchronized (pageLock) {
            destroyed = true;
            onPageChanged();
        }
        visible = false;
        NotificationService.removeListener(stateListener);
        settingsWriter.shutdownNow();
    }

    private void requestPermission(Request request) {
        if (!visible || !request.currentIdentity()) {
            respond(request, "activity-not-visible");
            return;
        }
        if (Build.VERSION.SDK_INT < 33 || NotificationPublisher.permissionGranted(activity)) {
            refreshDelivery();
            respond(request, null);
            return;
        }
        if (permissionPending != null || permissionInFlight) {
            respond(request, "permission-request-in-progress");
            return;
        }
        permissionPending = request;
        settingsWriter.execute(() -> {
            boolean saved = NotificationSettings.markPermissionRequested(
                activity, pageLock, request::currentIdentity
            );
            main.post(() -> {
                if (!request.currentIdentity() || permissionPending != request) {
                    return;
                }
                if (!saved || !visible) {
                    permissionPending = null;
                    respond(request, saved ? "activity-not-visible" : "settings-save-failed");
                    return;
                }
                try {
                    permissionInFlight = true;
                    activity.requestPermissions(
                        new String[] { Manifest.permission.POST_NOTIFICATIONS }, PERMISSION_REQUEST
                    );
                } catch (RuntimeException error) {
                    permissionPending = null;
                    permissionInFlight = false;
                    respond(request, "permission-request-failed");
                }
            });
        });
    }

    private void refreshDelivery() {
        if (destroyed) {
            return;
        }
        NotificationSettings settings = NotificationSettings.read(activity);
        NotificationIdentity identity = NotificationService.currentIdentity(activity);
        if (!settings.enabled) {
            NotificationService.stop(activity, "disabled", true);
        } else if (!NotificationPublisher.permissionGranted(activity)) {
            NotificationService.stop(activity, "permission-required", false);
        } else if (identity == null) {
            NotificationService.stop(activity, "auth-required", true);
        } else if (visible) {
            NotificationPublisher.clearDisabled(activity);
            NotificationService.startFromVisibleActivity(activity, identity);
        }
    }

    private void stateChanged(String identityDigest, String state) {
        if (destroyed) {
            return;
        }
        emitChanged();
        if ("auth-required".equals(state) && identityDigest != null) {
            authorizationLost.accept(identityDigest);
        }
    }

    private void respond(Request request, String error) {
        if (!request.currentForResponse()) {
            return;
        }
        try {
            JSONObject response = error == null
                ? status("notification-settings").put("requestId", request.requestId)
                : NotificationSettings.failure(request.requestId, error);
            request.reply.accept(response.toString());
        } catch (JSONException | RuntimeException ignored) {
            // A removed document cannot receive a delayed bridge completion.
        }
    }

    private void emitChanged() {
        Request target = subscription;
        if (target == null || !target.currentForResponse()) {
            return;
        }
        try {
            target.reply.accept(status("notification-settings-changed").toString());
        } catch (JSONException | RuntimeException ignored) {
            // The subscription is scoped to the current document.
        }
    }

    private JSONObject status(String type) throws JSONException {
        NotificationSettings settings = NotificationSettings.read(activity);
        String permission = permission();
        NotificationIdentity identity = NotificationService.currentIdentity(activity);
        String delivery = !settings.enabled ? "disabled"
            : !"granted".equals(permission) ? "permission-required"
                : identity == null ? "auth-required" : NotificationService.delivery(identity.digest);
        JSONObject response = new JSONObject().put("type", type).put("supported", true)
            .put("settings", settings.toJson()).put("permission", permission)
            .put("delivery", delivery);
        if ("permission-required".equals(delivery)) {
            response.put("message", "未获得系统通知权限，请允许通知或打开系统通知设置");
        } else if ("auth-required".equals(delivery)) {
            response.put("message", "请重新登录后接收通知");
        } else if ("unsupported".equals(delivery)) {
            response.put("message", "当前服务尚不支持通知事件，请更新服务");
        } else {
            String blockedChannels = NotificationPublisher.blockedChannelsMessage(activity, settings);
            if (blockedChannels != null) {
                response.put("message", blockedChannels);
            }
        }
        return response;
    }

    private String permission() {
        if (NotificationPublisher.permissionGranted(activity)) {
            return "granted";
        }
        if (Build.VERSION.SDK_INT >= 33 && activity.checkSelfPermission(
            Manifest.permission.POST_NOTIFICATIONS
        ) != PackageManager.PERMISSION_GRANTED
            && !NotificationSettings.wasPermissionRequested(activity)) {
            return "prompt";
        }
        return "denied";
    }

    private final class Request {
        final String requestId;
        final long generation;
        final BooleanSupplier page;
        final Consumer<String> reply;
        final NotificationIdentity identity;

        Request(
            String requestId, long generation, BooleanSupplier page, Consumer<String> reply,
            NotificationIdentity identity
        ) {
            this.requestId = requestId;
            this.generation = generation;
            this.page = page;
            this.reply = reply;
            this.identity = identity;
        }

        boolean current() {
            synchronized (pageLock) {
                return !destroyed && generation == pageGeneration && page.getAsBoolean();
            }
        }

        boolean currentIdentity() {
            return identity != null && currentForResponse();
        }

        boolean currentForResponse() {
            if (!current()) {
                return false;
            }
            NotificationIdentity active = NotificationService.currentIdentity(activity);
            return identity == null ? active == null
                : active != null && identity.digest.equals(active.digest);
        }
    }
}
