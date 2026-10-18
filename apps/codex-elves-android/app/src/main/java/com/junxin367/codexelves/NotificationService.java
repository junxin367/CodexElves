package com.junxin367.codexelves;

import android.app.Service;
import android.app.NotificationManager;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.webkit.CookieManager;

import org.json.JSONException;
import org.json.JSONObject;

import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArraySet;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Started exclusively by a visible Activity, without sticky or boot restarts. */
public final class NotificationService extends Service {
    static final Object IDENTITY_LOCK = new Object();
    private static final String EXPECTED_IDENTITY = "expected_notification_identity";
    private static final String JOURNAL = "codex_elves_notification_journal";
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final CopyOnWriteArraySet<Listener> LISTENERS = new CopyOnWriteArraySet<>();
    private static final Set<Object> VISIBLE_ACTIVITIES = new HashSet<>();
    private static long generation;
    private static NotificationService running;
    private static String stateIdentity;
    private static String delivery = "disconnected";

    private final ExecutorService network = Executors.newSingleThreadExecutor(
        runnable -> new Thread(runnable, "codex-elves-notifications")
    );
    private NotificationIdentity identity;
    private NotificationPublisher publisher;
    private NotificationStreamClient client;
    private long activeGeneration;
    private boolean systemReceiverRegistered;
    private final BroadcastReceiver systemNotificationSettings = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            synchronized (IDENTITY_LOCK) {
                if (running != NotificationService.this || publisher == null || client == null) {
                    return;
                }
                if (!NotificationPublisher.permissionGranted(NotificationService.this)) {
                    stop(NotificationService.this, "permission-required", false);
                    return;
                }
                if (Build.VERSION.SDK_INT >= 28
                    && NotificationManager.ACTION_NOTIFICATION_CHANNEL_BLOCK_STATE_CHANGED
                        .equals(intent.getAction())
                    && intent.getBooleanExtra(NotificationManager.EXTRA_BLOCKED_STATE, false)) {
                    String channel = intent.getStringExtra(NotificationManager.EXTRA_NOTIFICATION_CHANNEL_ID);
                    if (channel != null) {
                        publisher.systemChannelBlocked(channel);
                    }
                }
                NotificationPublisher.clearDisabled(NotificationService.this);
                client.requestResync();
            }
        }
    };

    interface Listener {
        void changed(String identityDigest, String state);
    }

    static void setAppVisible(Object owner, boolean visible) {
        synchronized (IDENTITY_LOCK) {
            if (visible) {
                VISIBLE_ACTIVITIES.add(owner);
            } else {
                VISIBLE_ACTIVITIES.remove(owner);
            }
        }
    }

    static boolean isAppVisible() {
        synchronized (IDENTITY_LOCK) {
            return !VISIBLE_ACTIVITIES.isEmpty();
        }
    }

    static void addListener(Listener listener) {
        LISTENERS.add(listener);
    }

    static void removeListener(Listener listener) {
        LISTENERS.remove(listener);
    }

    static NotificationIdentity currentIdentity(Context context) {
        SessionStore store = new SessionStore(context);
        if (!store.isAuthorized()) {
            return null;
        }
        return identityAtOrigin(context, store.getOrigin());
    }

    /** Used only to reject stale callbacks after a 401 has cleared the authorized flag. */
    static NotificationIdentity identityAtOrigin(Context context, String origin) {
        if (origin == null || !origin.equals(new SessionStore(context).getOrigin())) {
            return null;
        }
        try {
            if (!origin.equals(ServiceOrigin.normalize(origin))) {
                return null;
            }
            return NotificationIdentity.create(origin, CookieManager.getInstance().getCookie(origin));
        } catch (IllegalArgumentException | IllegalStateException error) {
            return null;
        }
    }

    static String delivery(String identityDigest) {
        synchronized (IDENTITY_LOCK) {
            return identityDigest.equals(stateIdentity) ? delivery : "disconnected";
        }
    }

    static void startFromVisibleActivity(Context context, NotificationIdentity identity) {
        synchronized (IDENTITY_LOCK) {
            NotificationIdentity current = currentIdentity(context);
            if (current == null || !identity.digest.equals(current.digest)) {
                return;
            }
            if (running != null && running.identity != null
                && identity.digest.equals(running.identity.digest) && running.client != null
                && running.activeGeneration == generation) {
                NotificationPublisher.clearDisabled(context);
                running.client.requestResync();
                return;
            }
            publish(identity.digest, "connecting");
            Intent start = new Intent(context, NotificationService.class)
                .putExtra(EXPECTED_IDENTITY, identity.digest);
            try {
                context.startForegroundService(start);
            } catch (RuntimeException error) {
                publish(identity.digest, "disconnected");
            }
        }
    }

    static void stop(Context context, String nextState, boolean clearJournal) {
        synchronized (IDENTITY_LOCK) {
            generation++;
            NotificationService active = running;
            if (active != null && active.client != null) {
                active.client.close();
                active.client = null;
            }
            context.stopService(new Intent(context, NotificationService.class));
            NotificationPublisher.clearAll(context);
            if (clearJournal || "permission-required".equals(nextState)) {
                // A restored permission establishes current pending requests, never old completions.
                journal(context).edit().clear().apply();
            }
            if ("auth-required".equals(nextState)) {
                NotificationPublisher.clearSuppressed(context);
            }
            publish(stateIdentity, nextState);
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        if (Build.VERSION.SDK_INT >= 28) {
            IntentFilter filter = new IntentFilter();
            filter.addAction(NotificationManager.ACTION_APP_BLOCK_STATE_CHANGED);
            filter.addAction(NotificationManager.ACTION_NOTIFICATION_CHANNEL_BLOCK_STATE_CHANGED);
            filter.addAction(NotificationManager.ACTION_NOTIFICATION_CHANNEL_GROUP_BLOCK_STATE_CHANGED);
            if (Build.VERSION.SDK_INT >= 33) {
                registerReceiver(systemNotificationSettings, filter, Context.RECEIVER_NOT_EXPORTED);
            } else {
                registerReceiver(systemNotificationSettings, filter);
            }
            systemReceiverRegistered = true;
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        synchronized (IDENTITY_LOCK) {
            NotificationIdentity current = currentIdentity(this);
            String expected = intent == null ? null : intent.getStringExtra(EXPECTED_IDENTITY);
            if (current == null || !current.digest.equals(expected)
                || !NotificationSettings.read(this).enabled
                || !NotificationPublisher.permissionGranted(this)) {
                stopSelf(startId);
                return START_NOT_STICKY;
            }
            if (identity != null && identity.digest.equals(current.digest)
                && client != null && activeGeneration == generation) {
                client.requestResync();
                return START_NOT_STICKY;
            }
            if (client != null) {
                client.close();
            }
            if (identity != null && !identity.digest.equals(current.digest)) {
                NotificationPublisher.clearAll(this);
                NotificationPublisher.clearSuppressed(this);
            }
            identity = current;
            publisher = new NotificationPublisher(this, identity);
            activeGeneration = ++generation;
            running = this;
            try {
                if (Build.VERSION.SDK_INT >= 34) {
                    startForeground(
                        NotificationPublisher.CONNECTION_ID, publisher.connection(),
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING
                    );
                } else {
                    startForeground(
                        NotificationPublisher.CONNECTION_ID, publisher.connection()
                    );
                }
            } catch (RuntimeException error) {
                publish(identity.digest, "disconnected");
                running = null;
                stopSelf(startId);
                return START_NOT_STICKY;
            }
            NotificationEventState events = new NotificationEventState();
            SharedPreferences saved = journal(this);
            if (identity.digest.equals(saved.getString("identity", ""))) {
                try {
                    events.restore(new JSONObject(saved.getString("state", "{}")));
                } catch (JSONException ignored) {
                    // The first snapshot will establish a new baseline.
                }
            } else {
                saved.edit().clear().apply();
            }
            NotificationIdentity connectionIdentity = identity;
            long connectionGeneration = activeGeneration;
            client = new NotificationStreamClient(
                identity, events, publisher,
                callback(connectionIdentity, connectionGeneration), IDENTITY_LOCK
            );
            publish(identity.digest, "connecting");
            network.execute(client);
            return START_NOT_STICKY;
        }
    }

    private NotificationStreamClient.Callback callback(
        NotificationIdentity connectionIdentity, long connectionGeneration
    ) {
        return new NotificationStreamClient.Callback() {
            @Override
            public boolean active() {
                synchronized (IDENTITY_LOCK) {
                    NotificationIdentity current = currentIdentity(NotificationService.this);
                    return running == NotificationService.this
                        && connectionGeneration == generation && current != null
                        && connectionIdentity.digest.equals(current.digest)
                        && NotificationSettings.read(NotificationService.this).enabled
                        && NotificationPublisher.permissionGranted(NotificationService.this);
                }
            }

            @Override
            public void state(String next) {
                synchronized (IDENTITY_LOCK) {
                    if (active()) {
                        // 只更新应用内状态，避免重新发出用户已经划掉的服务提示。
                        publish(connectionIdentity.digest, next);
                    }
                }
            }

            @Override
            public void journal(JSONObject state) {
                if (active()) {
                    NotificationService.journal(NotificationService.this).edit()
                        .putString("identity", connectionIdentity.digest)
                        .putString("state", state.toString()).apply();
                }
            }

            @Override
            public void unauthorized() {
                synchronized (IDENTITY_LOCK) {
                    if (!active()) {
                        return;
                    }
                    new SessionStore(NotificationService.this).setAuthorized(false);
                    stop(NotificationService.this, "auth-required", true);
                }
            }

            @Override
            public void unsupported() {
                synchronized (IDENTITY_LOCK) {
                    if (active()) {
                        stop(NotificationService.this, "unsupported", false);
                    }
                }
            }

            @Override
            public void stopped() {
                MAIN.post(() -> {
                    synchronized (IDENTITY_LOCK) {
                        if (running != NotificationService.this
                            || connectionGeneration != generation) {
                            return;
                        }
                        NotificationIdentity current = currentIdentity(NotificationService.this);
                        String next = !NotificationSettings.read(NotificationService.this).enabled
                            ? "disabled"
                            : !NotificationPublisher.permissionGranted(NotificationService.this)
                                ? "permission-required"
                                : current == null
                                    || !connectionIdentity.digest.equals(current.digest)
                                    ? "auth-required" : "disconnected";
                        stop(NotificationService.this, next, "auth-required".equals(next));
                    }
                });
            }

            @Override
            public void deleteThread(String threadId) {
                publisher.deleteThread(threadId);
            }

            @Override
            public void heartbeat() {
                synchronized (IDENTITY_LOCK) {
                    if (active()) {
                        NotificationPublisher.clearDisabled(NotificationService.this);
                    }
                }
            }
        };
    }

    @Override
    public void onDestroy() {
        if (systemReceiverRegistered) {
            unregisterReceiver(systemNotificationSettings);
            systemReceiverRegistered = false;
        }
        synchronized (IDENTITY_LOCK) {
            if (client != null) {
                client.close();
                client = null;
            }
            if (running == this) {
                running = null;
                if (activeGeneration == generation) {
                    generation++;
                    publish(identity == null ? null : identity.digest, "disconnected");
                }
                NotificationPublisher.clearAll(this);
            }
            stopForeground(STOP_FOREGROUND_REMOVE);
        }
        network.shutdownNow();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private static SharedPreferences journal(Context context) {
        return context.getSharedPreferences(JOURNAL, Context.MODE_PRIVATE);
    }

    private static void publish(String identityDigest, String next) {
        stateIdentity = identityDigest;
        delivery = next;
        long emittedGeneration = generation;
        MAIN.post(() -> {
            synchronized (IDENTITY_LOCK) {
                if (emittedGeneration != generation || !next.equals(delivery)
                    || !java.util.Objects.equals(identityDigest, stateIdentity)) {
                    return;
                }
            }
            for (Listener listener : LISTENERS) {
                listener.changed(identityDigest, next);
            }
        });
    }
}
