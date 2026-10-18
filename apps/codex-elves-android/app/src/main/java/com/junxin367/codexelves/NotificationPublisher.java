package com.junxin367.codexelves;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationChannelGroup;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.service.notification.StatusBarNotification;

import org.json.JSONException;
import org.json.JSONObject;

import java.util.List;
import java.util.HashSet;
import java.util.Set;

/** Notification bodies are built from fixed labels and sanitized server titles only. */
final class NotificationPublisher implements NotificationEventState.Sink {
    static final int CONNECTION_ID = 6100;
    static final String OPEN_ACTION = "com.junxin367.codexelves.OPEN_NOTIFICATION_THREAD";
    private static final String DISMISS_ACTION =
        "com.junxin367.codexelves.DISMISS_NOTIFICATION";
    private static final String EVENT_TAG = "codex-elves-event:";
    private static final String CONNECTION_CHANNEL = "codex_elves_connection";
    private static final String IDENTITY = "codex_elves_notification_identity";
    private static final String NOTIFICATION_ID = "codex_elves_notification_id";
    private static final String THREAD_ID = "codex_elves_notification_thread";
    private static final String KIND = "codex_elves_notification_kind";
    private static final String SUPPRESSED = "codex_elves_notification_suppressed";
    private static final String[] KINDS = { "approval", "question", "completed", "failed" };
    private final Context context;
    private final NotificationManager manager;
    private final NotificationIdentity identity;

    NotificationPublisher(Context context, NotificationIdentity identity) {
        this.context = context.getApplicationContext();
        this.identity = identity;
        manager = context.getSystemService(NotificationManager.class);
        createChannels(context);
        android.content.SharedPreferences preferences =
            context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE);
        android.content.SharedPreferences.Editor cleanup = preferences.edit();
        for (String key : preferences.getAll().keySet()) {
            if (!key.equals(identity.digest) && !key.equals("pending:" + identity.digest)
                && !key.equals("dismissed:" + identity.digest)) {
                cleanup.remove(key);
            }
        }
        cleanup.apply();
    }

    static void createChannels(Context context) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        for (String kind : KINDS) {
            NotificationChannel channel = new NotificationChannel(
                channelId(kind), label(context, kind), NotificationManager.IMPORTANCE_DEFAULT
            );
            channel.setLockscreenVisibility(Notification.VISIBILITY_PRIVATE);
            manager.createNotificationChannel(channel);
        }
        NotificationChannel connection = new NotificationChannel(
            CONNECTION_CHANNEL, context.getString(R.string.notification_connection_channel),
            NotificationManager.IMPORTANCE_LOW
        );
        connection.setSound(null, null);
        connection.enableVibration(false);
        connection.setLockscreenVisibility(Notification.VISIBILITY_PRIVATE);
        manager.createNotificationChannel(connection);
    }

    static boolean permissionGranted(Context context) {
        return (Build.VERSION.SDK_INT < 33 || context.checkSelfPermission(
            Manifest.permission.POST_NOTIFICATIONS
        ) == PackageManager.PERMISSION_GRANTED)
            && context.getSystemService(NotificationManager.class).areNotificationsEnabled();
    }

    static String blockedChannelsMessage(Context context, NotificationSettings settings) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        StringBuilder blocked = new StringBuilder();
        for (String kind : KINDS) {
            if (settings.allows(kind) && !channelEnabled(manager, kind)) {
                if (blocked.length() > 0) {
                    blocked.append("、");
                }
                blocked.append(label(context, kind));
            }
        }
        return blocked.length() == 0 ? null
            : "以下通知类别已在系统中关闭：" + blocked + "。请打开系统通知设置重新开启";
    }

    @Override
    public boolean notify(NotificationEventState.Event event, boolean previouslySeen) {
        String tag = tag(event.notificationId);
        if (userDismissed().contains(event.notificationId)) {
            // The legacy dismissed set includes both explicit swipes and opened pending reminders.
            manager.cancel(tag, 0);
            return true;
        }
        if (!permissionGranted(context) || !NotificationSettings.read(context).allows(event.kind)
            || !channelEnabled(manager, event.kind)) {
            manager.cancel(tag, 0);
            return false;
        }
        if (event.isPending()) {
            JSONObject ledger = ledger(context, identity.digest);
            try {
                ledger.put(event.notificationId, new JSONObject()
                    .put("kind", event.kind).put("threadId", event.threadId));
                saveLedger(context, identity.digest, ledger);
            } catch (JSONException ignored) {
                // Event fields have already been validated.
            }
        }
        if (!event.isPending() && previouslySeen && !isActive(tag)
            && !suppressed().contains(event.notificationId)) {
            // Pending reminders use explicit swipe/open records, even if a channel change was missed.
            return true;
        }
        Intent open = new Intent(context, MainActivity.class)
            .setAction(OPEN_ACTION)
            .setData(new Uri.Builder().scheme("codex-elves").authority("notification")
                .appendPath(identity.digest).appendPath(event.threadId)
                .appendPath(event.notificationId).build())
            .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pending = PendingIntent.getActivity(
            context, 0, open, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE
        );
        Intent deleted = new Intent(context, NotificationDismissReceiver.class)
            .setAction(DISMISS_ACTION)
            .setData(new Uri.Builder().scheme("codex-elves").authority("notification-dismiss")
                .appendPath(identity.digest).appendPath(event.notificationId).build());
        PendingIntent deleteIntent = PendingIntent.getBroadcast(
            context, 2, deleted, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE
        );
        Bundle metadata = new Bundle();
        metadata.putString(IDENTITY, identity.digest);
        metadata.putString(NOTIFICATION_ID, event.notificationId);
        metadata.putString(THREAD_ID, event.threadId);
        metadata.putString(KIND, event.kind);
        Notification notification = new Notification.Builder(context, channelId(event.kind))
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(label(context, event.kind))
            .setContentText(event.title.isEmpty()
                ? context.getString(R.string.notification_untitled_thread) : event.title)
            .setCategory(event.isPending() ? Notification.CATEGORY_MESSAGE : Notification.CATEGORY_STATUS)
            .setContentIntent(pending)
            .setDeleteIntent(deleteIntent)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion(channelId(event.kind)))
            .addExtras(metadata)
            .build();
        manager.notify(tag, 0, notification);
        Set<String> suppressed = suppressed();
        if (suppressed.remove(event.notificationId)) {
            saveSuppressed(context, identity.digest, suppressed);
        }
        return true;
    }

    Notification connection(String state) {
        Intent open = new Intent(context, MainActivity.class)
            .setAction(Intent.ACTION_MAIN)
            .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pending = PendingIntent.getActivity(
            context, 1, open, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE
        );
        int message = "connected".equals(state)
            ? R.string.notification_connection_connected
            : "connecting".equals(state)
                ? R.string.notification_connection_connecting
                : R.string.notification_connection_disconnected;
        return new Notification.Builder(context, CONNECTION_CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.notification_connection_title))
            .setContentText(context.getString(message))
            .setContentIntent(pending)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setOngoing(true)
            .setShowWhen(false)
            .setOnlyAlertOnce(true)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion(CONNECTION_CHANNEL))
            .build();
    }

    void updateConnection(String state) {
        manager.notify(CONNECTION_ID, connection(state));
    }

    @Override
    public void dismiss(String notificationId) {
        manager.cancel(tag(notificationId), 0);
        JSONObject ledger = ledger(context, identity.digest);
        if (ledger.remove(notificationId) != null) {
            saveLedger(context, identity.digest, ledger);
        }
        Set<String> suppressed = suppressed();
        if (suppressed.remove(notificationId)) {
            saveSuppressed(context, identity.digest, suppressed);
        }
        Set<String> dismissed = userDismissed();
        if (dismissed.remove(notificationId)) {
            saveDismissed(context, identity.digest, dismissed);
        }
    }

    @Override
    public void reset() {
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            if (isEvent(active) && identity.digest.equals(active.getNotification().extras
                .getString(IDENTITY))) {
                manager.cancel(active.getTag(), active.getId());
            }
        }
        context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE).edit()
            .remove(identity.digest).remove("pending:" + identity.digest).apply();
    }

    @Override
    public void reconcilePending(Set<String> notificationIds) {
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            Bundle extras = active.getNotification().extras;
            String kind = extras.getString(KIND);
            if (isEvent(active) && identity.digest.equals(extras.getString(IDENTITY))
                && ("approval".equals(kind) || "question".equals(kind))
                && !notificationIds.contains(extras.getString(NOTIFICATION_ID))) {
                manager.cancel(active.getTag(), active.getId());
            }
        }
        JSONObject ledger = ledger(context, identity.digest);
        java.util.Iterator<String> keys = ledger.keys();
        while (keys.hasNext()) {
            if (!notificationIds.contains(keys.next())) {
                keys.remove();
            }
        }
        saveLedger(context, identity.digest, ledger);
        Set<String> suppressed = suppressed();
        if (suppressed.retainAll(notificationIds)) {
            saveSuppressed(context, identity.digest, suppressed);
        }
        Set<String> dismissed = userDismissed();
        if (dismissed.retainAll(notificationIds)) {
            saveDismissed(context, identity.digest, dismissed);
        }
    }

    void deleteThread(String threadId) {
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            Bundle extras = active.getNotification().extras;
            if (isEvent(active) && identity.digest.equals(extras.getString(IDENTITY))
                && threadId.equals(extras.getString(THREAD_ID))) {
                manager.cancel(active.getTag(), active.getId());
            }
        }
        JSONObject ledger = ledger(context, identity.digest);
        Set<String> dismissed = userDismissed();
        Set<String> suppressed = suppressed();
        java.util.Iterator<String> keys = ledger.keys();
        while (keys.hasNext()) {
            String id = keys.next();
            JSONObject entry = ledger.optJSONObject(id);
            if (entry != null && threadId.equals(entry.optString("threadId"))) {
                keys.remove();
                dismissed.remove(id);
                suppressed.remove(id);
            }
        }
        saveLedger(context, identity.digest, ledger);
        saveDismissed(context, identity.digest, dismissed);
        saveSuppressed(context, identity.digest, suppressed);
    }

    static void clearDisabled(Context context) {
        synchronized (NotificationService.IDENTITY_LOCK) {
            clearDisabledLocked(context);
        }
    }

    private static void clearDisabledLocked(Context context) {
        NotificationSettings settings = NotificationSettings.read(context);
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        NotificationIdentity identity = NotificationService.currentIdentity(context);
        if (identity != null) {
            JSONObject ledger = ledger(context, identity.digest);
            Set<String> suppressed = new HashSet<>(context.getSharedPreferences(
                SUPPRESSED, Context.MODE_PRIVATE
            ).getStringSet(identity.digest, java.util.Collections.emptySet()));
            Set<String> dismissed = context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
                .getStringSet("dismissed:" + identity.digest, java.util.Collections.emptySet());
            java.util.Iterator<String> ids = ledger.keys();
            while (ids.hasNext()) {
                String id = ids.next();
                JSONObject entry = ledger.optJSONObject(id);
                if (!dismissed.contains(id) && entry != null
                    && (!settings.allows(entry.optString("kind"))
                    || !channelEnabled(manager, entry.optString("kind")))) {
                    suppressed.add(id);
                }
            }
            saveSuppressed(context, identity.digest, suppressed);
        }
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            String kind = active.getNotification().extras.getString(KIND);
            if (isEvent(active) && (!settings.allows(kind) || !channelEnabled(manager, kind))) {
                rememberSuppressed(context, active);
                manager.cancel(active.getTag(), active.getId());
            }
        }
    }

    static void clearAll(Context context) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            if (isEvent(active)) {
                rememberSuppressed(context, active);
                manager.cancel(active.getTag(), active.getId());
            }
        }
        manager.cancel(CONNECTION_ID);
    }

    static void clearSuppressed(Context context) {
        context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE).edit().clear().apply();
    }

    void systemChannelBlocked(String channelId) {
        JSONObject ledger = ledger(context, identity.digest);
        Set<String> suppressed = suppressed();
        Set<String> dismissed = userDismissed();
        java.util.Iterator<String> ids = ledger.keys();
        while (ids.hasNext()) {
            String id = ids.next();
            JSONObject entry = ledger.optJSONObject(id);
            if (!dismissed.contains(id) && entry != null
                && channelId(entry.optString("kind")).equals(channelId)) {
                suppressed.add(id);
            }
        }
        saveSuppressed(context, identity.digest, suppressed);
    }

    private static JSONObject ledger(Context context, String identity) {
        try {
            return new JSONObject(context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
                .getString("pending:" + identity, "{}"));
        } catch (JSONException | ClassCastException error) {
            return new JSONObject();
        }
    }

    private static void saveLedger(Context context, String identity, JSONObject ledger) {
        context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE).edit()
            .putString("pending:" + identity, ledger.toString()).apply();
    }

    private Set<String> suppressed() {
        return new HashSet<>(context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
            .getStringSet(identity.digest, java.util.Collections.emptySet()));
    }

    private Set<String> userDismissed() {
        return new HashSet<>(context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
            .getStringSet("dismissed:" + identity.digest, java.util.Collections.emptySet()));
    }

    static void recordUserDismiss(Context context, Intent intent) {
        if (intent == null || !DISMISS_ACTION.equals(intent.getAction())) {
            return;
        }
        synchronized (NotificationService.IDENTITY_LOCK) {
            Uri data = intent.getData();
            NotificationIdentity identity = NotificationService.currentIdentity(context);
            if (data == null || identity == null || !"codex-elves".equals(data.getScheme())
                || !"notification-dismiss".equals(data.getAuthority()) || data.getQuery() != null
                || data.getFragment() != null) {
                return;
            }
            List<String> paths = data.getPathSegments();
            if (paths.size() != 2 || !identity.digest.equals(paths.get(0))
                || !ledger(context, identity.digest).has(paths.get(1))) {
                return;
            }
            silencePending(context, identity.digest, paths.get(1));
        }
    }

    static void recordUserOpen(Context context, OpenTarget target) {
        if (target == null) {
            return;
        }
        synchronized (NotificationService.IDENTITY_LOCK) {
            NotificationIdentity identity = NotificationService.currentIdentity(context);
            if (identity == null || !identity.digest.equals(target.identity)) {
                return;
            }
            JSONObject entry = ledger(context, identity.digest).optJSONObject(target.notificationId);
            if (entry == null || !target.threadId.equals(entry.optString("threadId"))
                || !("approval".equals(entry.optString("kind"))
                    || "question".equals(entry.optString("kind")))) {
                return;
            }
            silencePending(context, identity.digest, target.notificationId);
        }
    }

    private static void silencePending(Context context, String identity, String notificationId) {
        android.content.SharedPreferences preferences =
            context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE);
        Set<String> dismissed = new HashSet<>(preferences.getStringSet(
            "dismissed:" + identity, java.util.Collections.emptySet()
        ));
        dismissed.add(notificationId);
        // Keep the persisted key compatible with existing user-swipe records.
        saveDismissed(context, identity, dismissed);
        Set<String> suppressed = new HashSet<>(preferences.getStringSet(
            identity, java.util.Collections.emptySet()
        ));
        if (suppressed.remove(notificationId)) {
            saveSuppressed(context, identity, suppressed);
        }
        // A resync can beat the Activity callback after autoCancel; remove that same-ID repost too.
        context.getSystemService(NotificationManager.class).cancel(tag(identity, notificationId), 0);
    }

    private static void saveDismissed(Context context, String identity, Set<String> ids) {
        context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE).edit()
            .putStringSet("dismissed:" + identity, ids).apply();
    }

    private static void rememberSuppressed(Context context, StatusBarNotification active) {
        Bundle extras = active.getNotification().extras;
        String kind = extras.getString(KIND);
        String identity = extras.getString(IDENTITY);
        String id = extras.getString(NOTIFICATION_ID);
        if (!("approval".equals(kind) || "question".equals(kind))
            || !NotificationIdentity.validDigest(identity) || id == null) {
            return;
        }
        if (context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
            .getStringSet("dismissed:" + identity, java.util.Collections.emptySet()).contains(id)) {
            return;
        }
        Set<String> pending = new HashSet<>(context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE)
            .getStringSet(identity, java.util.Collections.emptySet()));
        pending.add(id);
        if (pending.size() <= 2048) {
            saveSuppressed(context, identity, pending);
        }
    }

    private static void saveSuppressed(Context context, String identity, Set<String> ids) {
        context.getSharedPreferences(SUPPRESSED, Context.MODE_PRIVATE).edit()
            .putStringSet(identity, ids).apply();
    }

    static OpenTarget openTarget(Intent intent) {
        if (intent == null || !OPEN_ACTION.equals(intent.getAction())) {
            return null;
        }
        Uri data = intent.getData();
        if (data == null || !"codex-elves".equals(data.getScheme())
            || !"notification".equals(data.getAuthority()) || data.getQuery() != null
            || data.getFragment() != null) {
            return null;
        }
        List<String> parts = data.getPathSegments();
        if (parts.size() != 3 || !NotificationIdentity.validDigest(parts.get(0))
            || !NotificationIdentity.validThreadId(parts.get(1))
            || parts.get(2).isEmpty() || parts.get(2).length() > 200) {
            return null;
        }
        return new OpenTarget(parts.get(0), parts.get(1), parts.get(2));
    }

    static final class OpenTarget {
        final String identity;
        final String threadId;
        final String notificationId;

        OpenTarget(String identity, String threadId, String notificationId) {
            this.identity = identity;
            this.threadId = threadId;
            this.notificationId = notificationId;
        }
    }

    private Notification publicVersion(String channel) {
        return new Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(R.string.notification_public_summary))
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .build();
    }

    private String tag(String id) {
        return tag(identity.digest, id);
    }

    private static String tag(String identity, String id) {
        return EVENT_TAG + identity + ":" + id;
    }

    private boolean isActive(String tag) {
        for (StatusBarNotification active : manager.getActiveNotifications()) {
            if (tag.equals(active.getTag())) {
                return true;
            }
        }
        return false;
    }

    private static boolean isEvent(StatusBarNotification notification) {
        return notification.getTag() != null && notification.getTag().startsWith(EVENT_TAG);
    }

    private static String channelId(String kind) {
        return "codex_elves_" + kind;
    }

    private static boolean channelEnabled(NotificationManager manager, String kind) {
        NotificationChannel channel = manager.getNotificationChannel(channelId(kind));
        if (channel == null || channel.getImportance() == NotificationManager.IMPORTANCE_NONE) {
            return false;
        }
        if (Build.VERSION.SDK_INT >= 28 && channel.getGroup() != null) {
            NotificationChannelGroup group = manager.getNotificationChannelGroup(channel.getGroup());
            return group == null || !group.isBlocked();
        }
        return true;
    }

    private static String label(Context context, String kind) {
        switch (kind) {
            case "approval": return context.getString(R.string.notification_approval);
            case "question": return context.getString(R.string.notification_question);
            case "completed": return context.getString(R.string.notification_completed);
            case "failed": return context.getString(R.string.notification_failed);
            default: throw new IllegalArgumentException("Unknown notification kind");
        }
    }
}
