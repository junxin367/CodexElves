package com.junxin367.codexelves;

import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.Reader;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Set;

/** Only metadata endpoints are reachable; redirects and caller-supplied URLs are never used. */
final class NotificationStreamClient implements Runnable {
    private static final int MAX_SNAPSHOT_BYTES = 1_048_576;
    private final NotificationIdentity identity;
    private final NotificationEventState events;
    private final NotificationEventState.Sink sink;
    private final Callback callback;
    private final Object identityLock;
    private final Set<HttpURLConnection> connections = new HashSet<>();
    private volatile boolean closed;
    private volatile boolean resyncRequested;
    private String routeOrigin;
    private volatile String routeToken;
    private volatile String routeServerId;
    private volatile NotificationStreamClient secondary;
    private volatile Thread secondaryThread;
    private volatile Thread discoveryThread;
    private volatile boolean primaryConnected;
    private volatile boolean secondaryConnected;
    private long nextDiscoveryAt;
    private String channelDeviceId;
    private volatile int secondaryGeneration;

    interface Callback {
        boolean active();
        void state(String state);
        void journal(JSONObject journal);
        void unauthorized();
        void unsupported();
        void stopped();
        void deleteThread(String threadId);
        default void heartbeat() { }
    }

    NotificationStreamClient(
        NotificationIdentity identity,
        NotificationEventState events,
        NotificationEventState.Sink sink,
        Callback callback,
        Object identityLock
    ) {
        this.identity = identity;
        this.events = events;
        this.sink = sink;
        this.callback = callback;
        this.identityLock = identityLock;
    }

    @Override
    public void run() {
        int failures = 0;
        try {
            startDiscovery();
            while (!closed && callback.active()) {
                HttpURLConnection stream = null;
                long connectedAt = 0;
                try {
                    deliveryState("connecting");
                    synchronizeSnapshot();
                    stream = open("/api/events?notifications=true", "GET", "text/event-stream");
                    checkStatus(stream);
                    String contentType = stream.getContentType();
                    if (contentType == null
                        || !contentType.toLowerCase(java.util.Locale.ROOT)
                            .startsWith("text/event-stream")) {
                        throw new IOException("Invalid notification stream");
                    }
                    // SSE is already subscribed while the baseline gap is read.
                    synchronizeSnapshot();
                    deliveryState("connected");
                    connectedAt = System.nanoTime();
                    try (Reader input = new InputStreamReader(
                        stream.getInputStream(), StandardCharsets.UTF_8
                    )) {
                        while (!closed) {
                            String frame = readSseData(input);
                            ensureActive();
                            if (frame == null) {
                                throw new IOException("Notification stream ended");
                            }
                            if (resyncRequested) {
                                resyncRequested = false;
                                synchronizeSnapshot();
                            }
                            handleFrame(new JSONObject(frame));
                        }
                    }
                } catch (Unauthorized error) {
                    callback.unauthorized();
                    return;
                } catch (Unsupported error) {
                    callback.unsupported();
                    return;
                } catch (Stopped error) {
                    return;
                } catch (IOException | JSONException | RuntimeException error) {
                    if (closed || !callback.active()) {
                        return;
                    }
                    deliveryState("disconnected");
                    if (connectedAt != 0 && System.nanoTime() - connectedAt > 60_000_000_000L) {
                        failures = 0;
                    }
                    failures = Math.min(failures + 1, 6);
                } finally {
                    release(stream);
                }
                if (!backoff(backoffMillis(failures))) {
                    return;
                }
            }
        } finally {
            close();
            callback.stopped();
        }
    }

    void requestResync() {
        resyncRequested = true;
        NotificationStreamClient other = secondary;
        if (other != null) other.resyncRequested = true;
    }

    void close() {
        ArrayList<HttpURLConnection> active;
        synchronized (this) {
            closed = true;
            active = new ArrayList<>(connections);
            connections.clear();
            notifyAll();
        }
        for (HttpURLConnection connection : active) {
            connection.disconnect();
        }
        NotificationStreamClient other = secondary;
        if (other != null) other.close();
        Thread thread = secondaryThread;
        if (thread != null) thread.interrupt();
        Thread discovery = discoveryThread;
        if (discovery != null) discovery.interrupt();
    }

    private void deliveryState(String state) {
        primaryConnected = "connected".equals(state);
        callback.state(primaryConnected || secondaryConnected ? "connected" : state);
    }

    private synchronized void startDiscovery() {
        if (routeOrigin != null || closed || discoveryThread != null) return;
        discoveryThread = new Thread(() -> {
            while (!closed && callback.active()) {
                refreshSecondary();
                if (!backoff(60_000)) return;
            }
        }, "codex-elves-notifications-discovery");
        discoveryThread.setDaemon(true);
        discoveryThread.start();
    }

    private JSONObject readDiscovery(NotificationStreamClient route) throws IOException, JSONException {
        HttpURLConnection connection = null;
        try {
            connection = route.open("/api/channels", "GET", "application/json");
            connection.setConnectTimeout(10_000);
            connection.setReadTimeout(10_000);
            route.checkStatus(connection);
            JSONObject discovery;
            try (InputStream input = connection.getInputStream();
                 ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                byte[] buffer = new byte[4096];
                int count;
                while ((count = input.read(buffer)) != -1) {
                    ensureActive();
                    if (output.size() + count > 16384) throw new IOException("Channel discovery too large");
                    output.write(buffer, 0, count);
                }
                discovery = new JSONObject(output.toString(StandardCharsets.UTF_8.name()));
            }
            String serverId = discovery.getString("serverId");
            if (!discovery.getBoolean("ok") || discovery.getInt("schemaVersion") != 1
                || !discovery.getString("token").matches("[a-f0-9]{64}")
                || !serverId.matches("[a-f0-9-]{36}")
                || !serverId.equals(connection.getHeaderField("x-codex-channel-server"))
                || discovery.getLong("expiresAtMs") <= System.currentTimeMillis()
                || discovery.getJSONArray("origins").length() > 2
                || discovery.getString("deviceId").isEmpty()
                || channelDeviceId != null && !channelDeviceId.equals(discovery.getString("deviceId"))) {
                throw new IOException("Invalid channel discovery");
            }
            return discovery;
        } finally {
            route.release(connection);
        }
    }

    private void refreshSecondary() {
        if (routeOrigin != null || closed || System.currentTimeMillis() < nextDiscoveryAt) return;
        nextDiscoveryAt = System.currentTimeMillis() + 60_000;
        try {
            JSONObject discovery;
            try {
                discovery = readDiscovery(this);
            } catch (IOException | JSONException | IllegalArgumentException error) {
                NotificationStreamClient other = secondary;
                if (other == null || other.closed) return;
                discovery = readDiscovery(other);
            }
            String token = discovery.getString("token");
            org.json.JSONArray origins = discovery.getJSONArray("origins");
            String candidate = null;
            for (int index = 0; index < origins.length(); index++) {
                String origin = ServiceOrigin.normalize(origins.getString(index));
                String host = new URL(origin).getHost();
                if (!origin.equals(identity.origin)
                    && host.matches("[a-zA-Z0-9-]+\\.(ngrok-free\\.app|ngrok-free\\.dev|ngrok\\.app|ngrok\\.dev|ngrok\\.io)"))
                    candidate = origin;
            }
            ensureActive();
            synchronized (this) {
                if (closed) return;
                channelDeviceId = discovery.getString("deviceId");
                if (candidate == null) {
                    ++secondaryGeneration;
                    if (secondary != null) secondary.close();
                    if (secondaryThread != null) secondaryThread.interrupt();
                    secondary = null;
                    secondaryThread = null;
                    secondaryConnected = false;
                } else if (secondary != null && !secondary.closed && candidate.equals(secondary.routeOrigin)) {
                    secondary.routeToken = token;
                    secondary.routeServerId = discovery.getString("serverId");
                    return;
                } else {
                    int sourceGeneration = ++secondaryGeneration;
                    if (secondary != null) secondary.close();
                    if (secondaryThread != null) secondaryThread.interrupt();
                    secondaryConnected = false;
                    Callback secondaryCallback = new Callback() {
                        public boolean active() { return !closed && sourceGeneration == secondaryGeneration && callback.active(); }
                        public void state(String state) {
                            if (!active()) return;
                            secondaryConnected = "connected".equals(state);
                            callback.state(primaryConnected || secondaryConnected ? "connected" : "connecting");
                        }
                        public void journal(JSONObject journal) { callback.journal(journal); }
                        public void unauthorized() { /* Short-lived ticket failures do not revoke the primary login. */ }
                        public void unsupported() { }
                        public void stopped() {
                            if (sourceGeneration != secondaryGeneration) return;
                            secondaryConnected = false;
                            if (!closed && !primaryConnected) callback.state("disconnected");
                        }
                        public void deleteThread(String threadId) { callback.deleteThread(threadId); }
                        public void heartbeat() { callback.heartbeat(); }
                    };
                    NotificationStreamClient other = new NotificationStreamClient(identity, events, sink, secondaryCallback, identityLock);
                    other.routeOrigin = candidate;
                    other.routeToken = token;
                    other.routeServerId = discovery.getString("serverId");
                    secondary = other;
                    secondaryThread = new Thread(other, "codex-elves-notifications-secondary");
                    secondaryThread.setDaemon(true);
                    secondaryThread.start();
                }
            }
            if (candidate == null && !closed && callback.active())
                callback.state(primaryConnected ? "connected" : "connecting");
        } catch (IOException | JSONException | IllegalArgumentException ignored) {
            // Discovery is optional; older servers and a failed second tunnel keep the primary subscription.
        }
    }

    private void handleFrame(JSONObject frame) throws IOException, JSONException {
        Object method = frame.opt("method");
        if (!(method instanceof String)) {
            throw new JSONException("Invalid notification stream frame");
        }
        switch ((String) method) {
            case "remote/notification":
                NotificationEventState.Event event =
                    new NotificationEventState.Event(frame.getJSONObject("params"));
                boolean accepted;
                synchronized (identityLock) {
                    ensureActive();
                    accepted = events.stream(event, sink);
                    if (accepted) {
                        callback.journal(events.journal());
                    }
                }
                if (!accepted) {
                    synchronizeSnapshot();
                    synchronized (identityLock) {
                        ensureActive();
                        if (events.stream(event, sink)) {
                            callback.journal(events.journal());
                        }
                    }
                }
                break;
            case "remote/resync":
                synchronizeSnapshot();
                break;
            case "remote/unauthorized":
                throw new Unauthorized();
            case "remote/disconnected":
                throw new IOException("Notification service disconnected");
            case "thread/deleted":
                JSONObject params = frame.optJSONObject("params");
                Object thread = params == null ? null : params.opt("threadId");
                if (thread instanceof String && NotificationIdentity.validThreadId((String) thread)) {
                    synchronized (identityLock) {
                        ensureActive();
                        events.deleteThread((String) thread, sink);
                        callback.deleteThread((String) thread);
                        callback.journal(events.journal());
                    }
                }
                break;
            case "remote/heartbeat":
                callback.heartbeat();
                break;
            case "remote/connected":
                break;
            default:
                // The filtered subscription may gain metadata methods in a future server.
                break;
        }
    }

    private void synchronizeSnapshot() throws IOException, JSONException {
        HttpURLConnection connection = null;
        try {
            ensureActive();
            connection = open("/api/read/notifications", "POST", "application/json");
            JSONObject request;
            synchronized (identityLock) {
                ensureActive();
                request = events.request();
            }
            byte[] body = request.toString().getBytes(StandardCharsets.UTF_8);
            connection.setDoOutput(true);
            connection.setRequestProperty("Content-Type", "application/json; charset=utf-8");
            connection.setFixedLengthStreamingMode(body.length);
            try (java.io.OutputStream output = connection.getOutputStream()) {
                output.write(body);
            }
            checkStatus(connection);
            if (connection.getContentLengthLong() > MAX_SNAPSHOT_BYTES) {
                throw new IOException("Notification snapshot too large");
            }
            byte[] bytes;
            try (InputStream input = connection.getInputStream();
                 ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                byte[] buffer = new byte[8192];
                int count;
                while ((count = input.read(buffer)) != -1) {
                    ensureActive();
                    if (output.size() + count > MAX_SNAPSHOT_BYTES) {
                        throw new IOException("Notification snapshot too large");
                    }
                    output.write(buffer, 0, count);
                }
                bytes = output.toByteArray();
            }
            JSONObject snapshot = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
            synchronized (identityLock) {
                ensureActive();
                events.snapshot(snapshot, sink);
                callback.journal(events.journal());
            }
        } finally {
            release(connection);
        }
    }

    private HttpURLConnection open(String path, String method, String accept) throws IOException {
        ensureActive();
        URL url = new URL((routeOrigin == null ? identity.origin : routeOrigin) + path);
        HttpURLConnection connection = (HttpURLConnection) url.openConnection();
        connection.setInstanceFollowRedirects(false);
        connection.setConnectTimeout(15_000);
        connection.setReadTimeout(45_000);
        connection.setRequestMethod(method);
        connection.setRequestProperty("Origin", identity.origin);
        if (routeOrigin == null) connection.setRequestProperty("Cookie", identity.requestCookie());
        else connection.setRequestProperty("Authorization", "Bearer " + routeToken);
        connection.setRequestProperty("Accept", accept);
        connection.setRequestProperty("Cache-Control", "no-cache");
        connection.setRequestProperty("skip_zrok_interstitial", "1");
        connection.setRequestProperty("ngrok-skip-browser-warning", "1");
        synchronized (this) {
            if (closed) {
                connection.disconnect();
                throw new Stopped();
            }
            connections.add(connection);
        }
        return connection;
    }

    private void checkStatus(HttpURLConnection connection) throws IOException {
        int status = connection.getResponseCode();
        ensureActive();
        if (status == 401 || status == 403) {
            throw new Unauthorized();
        }
        if (status == 404 || status == 405 || status == 501) {
            throw new Unsupported();
        }
        if (status < 200 || status >= 300) {
            // All redirects, including same-origin redirects, are rejected.
            throw new IOException("Notification request failed");
        }
        String expectedServer = routeServerId;
        if (routeOrigin != null && expectedServer != null
            && !expectedServer.equals(connection.getHeaderField("x-codex-channel-server"))) {
            throw new IOException("Notification channel server mismatch");
        }
    }

    private void ensureActive() throws Stopped {
        if (closed || Thread.currentThread().isInterrupted() || !callback.active()) {
            throw new Stopped();
        }
    }

    private void release(HttpURLConnection connection) {
        if (connection == null) {
            return;
        }
        synchronized (this) {
            connections.remove(connection);
        }
        connection.disconnect();
    }

    private boolean backoff(long delay) {
        synchronized (this) {
            if (closed) {
                return false;
            }
            try {
                wait(delay);
            } catch (InterruptedException error) {
                Thread.currentThread().interrupt();
                return false;
            }
        }
        return !closed && callback.active();
    }

    static long backoffMillis(int failures) {
        return Math.min(30_000L, 1000L << Math.max(0, Math.min(failures - 1, 5)));
    }

    static String readSseData(Reader reader) throws IOException {
        StringBuilder line = new StringBuilder();
        StringBuilder data = new StringBuilder();
        while (true) {
            int next = reader.read();
            if (next == -1) {
                return data.length() == 0 ? null : data.toString();
            }
            if (next != '\n') {
                if (line.length() >= 32768) {
                    throw new IOException("Notification stream line too large");
                }
                line.append((char) next);
                continue;
            }
            if (line.length() > 0 && line.charAt(line.length() - 1) == '\r') {
                line.setLength(line.length() - 1);
            }
            if (line.length() == 0 && data.length() > 0) {
                data.setLength(data.length() - 1);
                return data.toString();
            }
            if (line.indexOf("data:") == 0) {
                int start = line.length() > 5 && line.charAt(5) == ' ' ? 6 : 5;
                data.append(line, start, line.length()).append('\n');
                if (data.length() > 262144) {
                    throw new IOException("Notification stream event too large");
                }
            }
            line.setLength(0);
        }
    }

    private static final class Unauthorized extends IOException { }
    private static final class Unsupported extends IOException { }
    private static final class Stopped extends IOException { }
}
