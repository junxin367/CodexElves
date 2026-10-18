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
            while (!closed && callback.active()) {
                HttpURLConnection stream = null;
                long connectedAt = 0;
                try {
                    callback.state("connecting");
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
                    callback.state("connected");
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
                    callback.state("disconnected");
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
        URL url = new URL(identity.origin + path);
        HttpURLConnection connection = (HttpURLConnection) url.openConnection();
        connection.setInstanceFollowRedirects(false);
        connection.setConnectTimeout(15_000);
        connection.setReadTimeout(45_000);
        connection.setRequestMethod(method);
        connection.setRequestProperty("Origin", identity.origin);
        connection.setRequestProperty("Cookie", identity.requestCookie());
        connection.setRequestProperty("Accept", accept);
        connection.setRequestProperty("Cache-Control", "no-cache");
        connection.setRequestProperty("skip_zrok_interstitial", "1");
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
